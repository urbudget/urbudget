package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.io.IOException
import java.time.YearMonth
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * What the screen is left with when an edit ends badly. A scripted storage, because totals that fail
 * only after an undo are impractical to arrange through the real store: `edit_budget` checks them
 * before each commit. Needs no `NativeServices`, so it runs in the same process as anything.
 */
@RunWith(AndroidJUnit4::class)
class BudgetViewModelFailureTest {
    private lateinit var session: EditSession
    @Volatile private var totalsFail = false
    private val engine = BudgetEngine { text ->
        if (JSONObject(text).getString("method") == "calculate" && totalsFail) JSONObject().put("ok", false).put("error", "Totals failed").toString()
        else JSONObject().put("ok", true).put("value", JSONObject().put("readyToAssign", 0)).toString()
    }
    private val budget get() = JSONObject("""{"schemaVersion":2,"name":"Scripted","currency":"USD","accounts":[],"categories":[],"assignments":[],
        "transactions":[{"id":"t1","memo":""},{"id":"t2","memo":""}]}""")
    private val tap = JSONObject().put("action", JSONObject().put("type", "bulk").put("ids", JSONArray().put("t1")))

    /** Applying this throws, as a patch nothing can be read from would. */
    private class BrokenPatch : JSONObject() {
        override fun length() = 1
        override fun has(name: String): Boolean = throw IllegalStateException("Broken patch")
    }

    @Before fun setup() { session = EditSession(services = false) }
    @After fun clean() { session.close() }

    private fun model(storage: ScriptedEditStorage): BudgetViewModel {
        storage.answer = { method, _ ->
            if (method == "calculate" && totalsFail) throw IOException("Totals failed")
            JSONObject().put("readyToAssign", 0)
        }
        return session.scripted(BudgetRepository(storage, engine, YearMonth.now().toString()))
    }

    private val filter get() = JSONObject().put("from", "2026-01").put("to", "2026-12")
    private val changed get() = session.context.getString(R.string.model_changed_preview)

    /** What `reports` comes back with: its reply, or its error. */
    private fun asked(vm: BudgetViewModel): Pair<JSONObject?, String?> {
        val done = CountDownLatch(1)
        var reply: JSONObject? = null
        var error: String? = null
        vm.query("reports", filter, onError = { error = it; done.countDown() }) { reply = it; done.countDown() }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        return reply to error
    }

    /** Runs `action`, and reads the state right after it, ahead of the refresh it may queue. */
    private fun afterIt(vm: BudgetViewModel, storage: ScriptedEditStorage, action: () -> Unit): Pair<BudgetUiState, Long> {
        var state: BudgetUiState? = null
        var epoch = -1L
        val gate = CountDownLatch(1)
        storage.gate = gate
        session.onMain { action(); vm.afterPending { state = vm.state.value; epoch = vm.operationFailureEpoch }; gate.countDown() }
        vm.settle()
        return state!! to epoch
    }

    @Test fun aScriptedSessionLeavesTheAppsRecoveryCopyAlone() {
        val copy = File(session.context.filesDir, "budget-before-replacement.json")
        copy.writeText("Kept")
        try {
            EditSession(services = false).close()
            assertEquals("Kept", copy.readText())
        } finally { copy.delete() }
    }

    @Test fun anUndoWhoseTotalsFailIsSavedAndNeverRunsAgain() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        val shown = vm.state.value
        assertEquals("Scripted", shown.budget!!.getString("name"))
        storage.undoPatch = JSONObject().put("name", "Undone")
        storage.undoSummary = null
        val epoch = vm.operationFailureEpoch
        val opens = storage.count("open")
        totalsFail = true

        val (state, failures) = afterIt(vm, storage) { vm.undo() }
        assertEquals("Undone", state.budget!!.getString("name"))
        assertNull("Never a summary from another revision", state.summary)
        assertEquals("Totals failed", state.error)
        assertEquals("Saved, so no editor keeps its draft over it", epoch, failures)
        assertEquals(storage.revision, state.budgetRevision)
        val until = System.currentTimeMillis() + 5_000
        while (storage.count("open") == opens && System.currentTimeMillis() < until) Thread.sleep(10)
        assertTrue("A refresh ran", storage.count("open") > opens)
        vm.settle()
        assertEquals(1, storage.count("undoEdit"))

        totalsFail = false
        vm.retry()
        vm.settle()
        assertEquals(1, storage.count("undoEdit"))
        assertSame(state.budget, vm.state.value.budget)
        assertNotNull(vm.state.value.summary)
        assertNull(vm.state.value.error)
    }

    @Test fun aBrokenPatchAndAFailedReloadPublishNothingAndARefreshHeals() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        val shown = vm.state.value
        storage.editPatch = { JSONObject().put("name", "Saved") }
        storage.brokenReply = BrokenPatch()
        storage.fail("open", ScriptedEditStorage.Fault.Failure)
        val epoch = vm.operationFailureEpoch

        val (state, failures) = afterIt(vm, storage) { vm.runCommand("workflow", tap) }
        assertSame("No root to publish", shown.budget, state.budget)
        assertEquals("Scripted failure of open", state.error)
        assertEquals(epoch, failures)
        assertEquals(1, storage.count("edit"))

        vm.settle()
        vm.retry()
        vm.settle()
        assertEquals(1, storage.count("edit"))
        assertEquals("Saved", vm.state.value.budget!!.getString("name"))
        assertEquals(storage.revision, vm.state.value.budgetRevision)
        assertNull(vm.state.value.error)
    }

    @Test fun aRefusedEditShowsTheFreshBudgetAndRetryRunsIt() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        val fresh = budget
        fresh.getJSONArray("transactions").getJSONObject(1).put("memo", "From sync")
        storage.external(fresh)
        storage.fail("edit", ScriptedEditStorage.Fault.Conflict, times = 2)
        storage.editPatch = { JSONObject().put("name", "Saved") }
        val epoch = vm.operationFailureEpoch

        vm.runCommand("workflow", tap)
        vm.settle()
        val state = vm.state.value
        assertEquals("From sync", state.budget!!.getJSONArray("transactions").getJSONObject(1).getString("memo"))
        assertEquals("Scripted", state.budget!!.getString("name"))
        assertTrue(state.error!!.startsWith("CONFLICT:"))
        assertEquals(epoch + 1, vm.operationFailureEpoch)
        assertEquals(storage.revision, state.budgetRevision)
        assertEquals(2, storage.count("edit"))

        vm.retry()
        vm.settle()
        assertEquals(3, storage.count("edit"))
        assertEquals("Saved", vm.state.value.budget!!.getString("name"))
        assertNull(vm.state.value.error)
    }

    @Test fun aRefusedUndoShowsTheFreshBudget() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        val fresh = budget.put("name", "From sync")
        storage.external(fresh)
        storage.fail("undoEdit", ScriptedEditStorage.Fault.Conflict)
        val epoch = vm.operationFailureEpoch

        vm.undo()
        vm.settle()
        assertEquals("From sync", vm.state.value.budget!!.getString("name"))
        assertEquals(epoch + 1, vm.operationFailureEpoch)
        assertTrue(vm.state.value.error!!.startsWith("CONFLICT:"))
    }

    @Test fun aQuestionTheStoreHasMovedPastCatchesUpAndIsAnswered() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        val shown = vm.state.value
        storage.external(budget.put("name", "From sync"))

        val (reply, error) = asked(vm)
        assertNotNull(reply)
        assertNull(error)
        assertEquals("The first try, refused, and one more", 2, storage.count("query:reports"))
        val state = vm.state.value
        assertNotSame(shown.budget, state.budget)
        assertEquals("From sync", state.budget!!.getString("name"))
        assertEquals(storage.revision, state.budgetRevision)
        assertNull(state.error)
    }

    @Test fun aQuestionRefusedTwiceReachesOnError() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        storage.fail("query:reports", ScriptedEditStorage.Fault.Stale, times = 2)

        val (reply, error) = asked(vm)
        assertNull(reply)
        assertEquals(changed, error)
        assertEquals(2, storage.count("query:reports"))
    }

    @Test fun aQuestionAboutABudgetThatsGoneIsNotAskedAgain() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        storage.external(budget.put("name", "Another budget"))
        storage.identity = "another"

        val (reply, error) = asked(vm)
        assertNull(reply)
        assertEquals(changed, error)
        assertEquals(1, storage.count("query:reports"))
        assertEquals("another", vm.state.value.budgetIdentity)
    }

    @Test fun aQuestionCancelledWhileWaitingForTheQueueCallsNothingBack() {
        val storage = ScriptedEditStorage(budget)
        val vm = model(storage)
        val release = CountDownLatch(1)
        val started = CountDownLatch(1)
        storage.editPatch = { started.countDown(); release.await(); JSONObject() }
        vm.runCommand("workflow", tap)
        assertTrue(started.await(10, TimeUnit.SECONDS))
        storage.fail("query:reports", ScriptedEditStorage.Fault.Stale)
        var called = false
        val job = vm.query("reports", filter, onError = { called = true }) { called = true }
        val until = System.currentTimeMillis() + 5_000
        while (storage.count("query:reports") == 0 && System.currentTimeMillis() < until) Thread.sleep(5)
        assertEquals(1, storage.count("query:reports"))
        job.cancel()
        release.countDown()
        vm.settle()
        assertTrue(job.isCompleted)
        assertFalse(called)
        assertEquals(1, storage.count("query:reports"))
    }
}
