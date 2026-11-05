package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import app.urbudget.nativeapp.ui.row
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.time.LocalDate
import java.time.YearMonth

/** Other writers, edits that fail, stale screens and a full queue: nothing lost, nothing saved twice. */
@RunWith(AndroidJUnit4::class)
class EngineEditConflictTest {
    private lateinit var session: EditSession
    private val today = LocalDate.now().toString()
    private val month = YearMonth.now().toString()

    @Before fun setup() { requireEmulator(); session = EditSession() }
    @After fun clean() { if (::session.isInitialized) session.close() }

    private fun opened(transactions: Int = 800, seed: Long = 21): Pair<BudgetViewModel, EditDriver> {
        session.make(SyntheticBudget.build(transactions = transactions, seed = seed).put("name", "Conflict"))
        val vm = session.model()
        return vm to EditDriver(vm, strict = false)
    }
    private fun plainRows(vm: BudgetViewModel) = vm.state.value.budget!!.rows("transactions").filter(Edits::plain).map { it.text("id") }
    private fun memo(id: String) = storedRow(id)!!.text("memo")
    private fun appMemo(vm: BudgetViewModel, id: String) = vm.state.value.budget!!.row("transactions", id)!!.text("memo")
    private fun tap(vm: BudgetViewModel, id: String, memo: String) { vm.runCommand("workflow", Edits.bulk(listOf(id), "memo", memo)); vm.settle() }
    private fun recovery() = NativeServices.call("read_recovery") as JSONObject?
    private fun unsaved() = (NativeServices.call("saved_budget") as JSONObject).getBoolean("unsavedDraft")

    /** After `write`, which the app isn't told of, a tap lands on the new revision and neither change is lost. */
    private fun survives(vm: BudgetViewModel, d: EditDriver, name: String, row: String, write: () -> Unit, check: () -> Unit) {
        val shown = vm.state.value.budget
        write()
        assertSame("The app wasn't told", shown, vm.state.value.budget)
        tap(vm, row, "Tapped after $name")
        assertNull("$name: error", vm.state.value.error)
        assertEquals("Tapped after $name", memo(row))
        assertEquals("Tapped after $name", appMemo(vm, row))
        d.verify(name)
        check()
    }

    @Test fun aSaveFromElsewhereIsKept() {
        val (vm, d) = opened()
        val (row, other) = plainRows(vm)
        survives(vm, d, "a save", row, { saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == other }.put("memo", "From elsewhere") } }) {
            assertEquals("From elsewhere", appMemo(vm, other))
            assertEquals("From elsewhere", memo(other))
        }
        d.end()
    }

    @Test fun dueSchedulesAppliedElsewhereAreKept() {
        val (vm, d) = opened()
        val due = vm.state.value.budget!!.rows("schedules").first { !it.optBoolean("paused") && it.text("nextDate").isNotBlank() && it.text("nextDate") <= today }
        val row = plainRows(vm).first()
        survives(vm, d, "due schedules", row, {
            // As the app starts: it only wants the token, so no budget crosses.
            val reply = NativeServices.call("apply_due_schedules", JSONObject().put("through", today).put("ackOnly", true)) as JSONObject
            assertFalse(reply.has("budget"))
        }) {
            assertNotEquals(due.text("nextDate"), vm.state.value.budget!!.row("schedules", due.text("id"))!!.text("nextDate"))
        }
        d.end()
    }

    @Test fun aWriteOnASecondConnectionCostsOneMoreTry() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        val shown = vm.state.value.budget
        survives(vm, d, "a second connection", row, ::moveTheToken) {
            // Reloaded and found the same: every row but the edited one is the object it was.
            assertSame(shown!!.rows("transactions").first { it.text("id") != row }, vm.state.value.budget!!.rows("transactions").first { it.text("id") != row })
        }
        d.end()
    }

    @Test fun aTokenThatMovesWithNoChangeKeepsTheRoot() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        val shown = vm.state.value
        moveTheToken()
        assertNotEquals(shown.budgetRevision, (NativeServices.call("native_budget_status") as JSONObject).getString("token"))
        vm.refresh()
        vm.settle()
        assertSame("Open editors compare by identity", shown.budget, vm.state.value.budget)
        assertSame(shown.summary, vm.state.value.summary)
        d.verify("after the write")
        tap(vm, row, "After a move")
        assertEquals("After a move", memo(row))
        d.end()
    }

    @Test fun aRestoreFromElsewhereIsKept() {
        val (vm, d) = opened()
        val (row, other) = plainRows(vm)
        survives(vm, d, "a restore", row, {
            val answer = NativeServices.call("saved_budget") as JSONObject
            val budget = answer.getJSONObject("budget")
            budget.rows("transactions").first { it.text("id") == other }.put("memo", "Restored")
            NativeServices.call("restore_budget", JSONObject().put("budget", budget).put("expectedToken", answer.getString("token")))
        }) { assertEquals("Restored", appMemo(vm, other)) }
        d.end()
    }

    @Test fun aCommandTheEngineRefusesSavesNothing() {
        val (vm, d) = opened()
        val state = vm.state.value
        val epoch = vm.operationFailureEpoch
        vm.saveTransaction(Edits.transaction("t-nowhere", today, account = "no-such-account"))
        vm.settle()
        assertNotNull(vm.state.value.error)
        assertEquals(epoch + 1, vm.operationFailureEpoch)
        assertSame(state.budget, vm.state.value.budget)
        assertEquals(state.budgetRevision, vm.state.value.budgetRevision)
        assertFalse(unsaved())
        assertNull("A refused edit leaves no recovery file", recovery())
        vm.dismissError()
        d.verify("after the refusal")
        d.end()
    }

    @Test fun aBudgetThatWontValidateSavesNothing() {
        val (vm, d) = opened()
        val state = vm.state.value
        val broken = Edits.transaction("t-unbalanced", today).put("amount", -999)
        vm.saveTransaction(broken)
        vm.settle()
        assertNotNull(vm.state.value.error)
        assertSame(state.budget, vm.state.value.budget)
        assertFalse(unsaved())
        assertNull(storedRow("t-unbalanced"))
        vm.dismissError()
        d.verify("after the refusal")
        d.end()
    }

    @Test fun aFailedCommitChangesNothingAndRetryDoesntApplyItTwice() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        d.workflow("something to undo", Edits.bulk(listOf(row), "memo", "Saved").getJSONObject("action"))
        secondConnection { it.execSQL("CREATE TRIGGER reject_native_save BEFORE UPDATE ON metadata BEGIN SELECT RAISE(ABORT, 'Simulated disk failure'); END;") }
        // The trigger is a write, so without this the edit would be a CONFLICT and never reach it.
        vm.refresh()
        vm.settle()
        val state = vm.state.value
        val count = state.budget!!.getJSONArray("transactions").length()
        val epoch = vm.operationFailureEpoch
        val stored = Canon.digest(storedBudget())

        vm.runCommand("workflow", Edits.bulk(listOf(row), "duplicate"))
        vm.settle()
        assertTrue(vm.state.value.error, vm.state.value.error!!.contains("Simulated disk failure"))
        assertEquals(epoch + 1, vm.operationFailureEpoch)
        assertSame(state.budget, vm.state.value.budget)
        assertEquals(state.budgetRevision, vm.state.value.budgetRevision)
        assertEquals(state.undoAvailable, vm.state.value.undoAvailable)
        assertFalse(unsaved())
        assertEquals(stored, Canon.digest(storedBudget()))
        assertNotNull(recovery())

        // Still failing: a retry saves nothing either.
        vm.retry()
        vm.settle()
        assertEquals(stored, Canon.digest(storedBudget()))
        assertEquals(epoch + 2, vm.operationFailureEpoch)

        secondConnection { it.execSQL("DROP TRIGGER reject_native_save") }
        vm.retry()
        vm.settle()
        assertNull(vm.state.value.error)
        assertEquals("Once, and only once", count + 1, vm.state.value.budget!!.getJSONArray("transactions").length())
        assertEquals(count + 1, storedBudget().getJSONArray("transactions").length())
        d.verify("after the retry")
        assertNull("A saved edit clears it", recovery())
        d.end()
    }

    @Test fun theSameEditWorksOnceTheFailureHasGone() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        secondConnection { it.execSQL("CREATE TRIGGER reject_native_save BEFORE UPDATE ON metadata BEGIN SELECT RAISE(ABORT, 'Simulated disk failure'); END;") }
        vm.refresh()
        vm.settle()
        tap(vm, row, "Refused")
        assertNotNull(vm.state.value.error)
        vm.dismissError()
        secondConnection { it.execSQL("DROP TRIGGER reject_native_save") }
        vm.refresh()
        vm.settle()
        d.step("the same edit", null) { vm.runCommand("workflow", Edits.bulk(listOf(row), "memo", "Refused")) }
        assertEquals("Refused", memo(row))
        d.end()
    }

    @Test fun anUndoAfterAFailureLeavesTheRecoveryAndTheNextEditClearsIt() {
        val (vm, d) = opened()
        val (first, second) = plainRows(vm)
        // In place before the edit, because putting it there moves the token and clears the undo.
        secondConnection { it.execSQL("CREATE TRIGGER reject_memo BEFORE INSERT ON transactions WHEN NEW.data LIKE '%Refused memo%' BEGIN SELECT RAISE(ABORT, 'Simulated disk failure'); END;") }
        vm.refresh()
        vm.settle()
        d.workflow("an edit to undo", Edits.bulk(listOf(first), "memo", "Kept").getJSONObject("action"))
        tap(vm, second, "Refused memo")
        assertTrue(vm.state.value.error, vm.state.value.error!!.contains("Simulated disk failure"))
        vm.dismissError()
        val held = recovery()
        assertNotNull(held)

        vm.undo()
        vm.settle()
        assertNull(vm.state.value.error)
        assertNotEquals("A real undo", "Kept", memo(first))
        assertNotNull("An undo leaves it", recovery())
        tap(vm, second, "Fine")
        assertNull(vm.state.value.error)
        assertNull("The next saved edit clears it", recovery())
        d.verify("after the edit")
        d.end()
        // After the last look: this is a write, so it moves the token and clears the undo.
        secondConnection { it.execSQL("DROP TRIGGER reject_memo") }
    }

    @Test fun aRecordChangedElsewhereIsRefusedWhenItsGuardIsStale() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        val guard = vm.state.value.let { BudgetEditGuard.record(it.budget, it.budgetIdentity, "transactions", row) }
        val record = JSONObject(vm.state.value.budget!!.row("transactions", row).toString()).put("memo", "From a stale editor")
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == row }.put("memo", "From elsewhere") }
        vm.refresh()
        vm.settle()
        vm.saveTransaction(record, guard)
        vm.settle()
        assertEquals(session.context.getString(R.string.guard_values_changed), vm.state.value.error)
        assertEquals("From elsewhere", memo(row))
        vm.dismissError()
        d.end()
    }

    @Test fun aStaleScreenCantReplaceWhatTheStoreMoved() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        // A guard that covers the whole budget, queued behind an edit it didn't see.
        val balance = vm.state.value.summary!!.rows("accounts").first { it.text("id") == "savings" }.getLong("balance")
        session.onMain {
            vm.runCommand("workflow", Edits.bulk(listOf(row), "memo", "First"))
            vm.runCommand("reconcile", JSONObject().put("accountId", "savings").put("balance", balance + 1_000).put("date", today).put("adjustmentId", "adjustment-stale"))
        }
        vm.settle()
        assertEquals(session.context.getString(R.string.guard_values_changed), vm.state.value.error)
        assertEquals("First", memo(row))
        assertNull(storedRow("adjustment-stale"))
        vm.dismissError()
        d.verify("after the refusal")
        d.end()
    }

    @Test fun twoNewRecordsWithOneIdCantBothLand() {
        val (vm, d) = opened()
        val shown = vm.state.value
        val guard = BudgetEditGuard.record(shown.budget, shown.budgetIdentity, "transactions", "t-collision")
        d.step("the first", null) { vm.saveTransaction(Edits.transaction("t-collision", today)) }
        vm.saveTransaction(Edits.transaction("t-collision", today, amount = -9_999), guard)
        vm.settle()
        assertEquals(session.context.getString(R.string.guard_values_changed), vm.state.value.error)
        assertEquals(-1_250L, storedRow("t-collision")!!.getLong("amount"))
        vm.dismissError()
        d.end()
    }

    @Test fun aGuardFromAnotherBudgetIsRefused() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        val shown = vm.state.value
        val guard = BudgetEditGuard.record(shown.budget, shown.budgetIdentity, "transactions", row)
        val record = JSONObject(shown.budget!!.row("transactions", row).toString()).put("memo", "Into the wrong budget")
        session.make(SyntheticBudget.build(transactions = 800, seed = 21).put("name", "Conflict too"))
        vm.refresh()
        vm.settle()
        assertNotEquals(shown.budgetIdentity, vm.state.value.budgetIdentity)
        vm.saveTransaction(record, guard)
        vm.settle()
        assertEquals(session.context.getString(R.string.guard_budget_switched), vm.state.value.error)
        assertNotEquals("Into the wrong budget", memo(row))
        vm.dismissError()
        d.end()
    }

    @Test fun twoHundredEditsQueuedWithReadsLandInOrder() {
        val (vm, d) = opened(transactions = 1_200, seed = 22)
        val rows = plainRows(vm).take(200)
        assertEquals(200, rows.size)
        val answers = java.util.concurrent.atomic.AtomicInteger()
        val reports = JSONObject().put("from", YearMonth.now().minusMonths(2).toString()).put("to", month)
        val running = java.util.concurrent.atomic.AtomicBoolean(true)
        val prober = Thread {
            while (running.get()) { NativeServices.call("native_budget_status"); Thread.sleep(2) }
        }.apply { start() }
        session.onMain {
            rows.forEachIndexed { n, row ->
                vm.runCommand("workflow", Edits.bulk(listOf(row), "memo", "Queued $n"))
                if (n % 20 == 0) vm.query("reports", reports, onError = { answers.incrementAndGet() }) { answers.incrementAndGet() }
                if (n % 25 == 0) vm.refresh()
            }
        }
        vm.settle()
        running.set(false)
        prober.join()
        assertNull(vm.state.value.error)
        d.verify("after the queue")
        val queued = Canon.digest(Canon.forStateless(vm.state.value.budget!!))

        session.make(SyntheticBudget.build(transactions = 1_200, seed = 22).put("name", "Conflict"))
        vm.refresh()
        vm.settle()
        rows.forEachIndexed { n, row -> vm.runCommand("workflow", Edits.bulk(listOf(row), "memo", "Queued $n")); vm.settle() }
        assertEquals("One at a time, the same budget", Canon.digest(Canon.forStateless(vm.state.value.budget!!)), queued)
        assertEquals(10, waitForAnswers(answers, 10))
        d.end()
    }
    private fun waitForAnswers(answers: java.util.concurrent.atomic.AtomicInteger, wanted: Int): Int {
        val until = System.currentTimeMillis() + 30_000
        while (answers.get() < wanted && System.currentTimeMillis() < until) Thread.sleep(20)
        return answers.get()
    }

    private val filter get() = JSONObject().put("from", YearMonth.now().minusMonths(2).toString()).put("to", month).put("accountIds", JSONArray()).put("categoryIds", JSONArray())

    @Test fun aReportWhileTheTokenHasMovedIsAnsweredOnTheNewRevision() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        val shown = vm.state.value.budget
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == row }.put("memo", "From elsewhere") }
        assertSame("The app wasn't told", shown, vm.state.value.budget)
        val report = vm.asked("reports", filter)
        assertNull(vm.state.value.error)
        assertEquals("It caught up", "From elsewhere", appMemo(vm, row))
        assertSameJson("The new revision's report", stateless("reports", storedBudget(), filter), report)
        d.verify("after the report")
        d.end()
    }

    @Test fun aMonthChosenWhileTheTokenHasMovedShowsThatRevision() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        val next = YearMonth.now().plusMonths(1).toString()
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == row }.put("memo", "From elsewhere") }
        vm.selectMonth(next)
        vm.settle()
        assertNull(vm.state.value.error)
        assertEquals(next, vm.state.value.month)
        assertEquals("From elsewhere", appMemo(vm, row))
        // Rows, totals and revision are the one revision.
        d.verify("after the month")
        d.end()
    }

    @Test fun aReportAfterATokenOnlyMoveKeepsTheRoot() {
        val (vm, d) = opened()
        val shown = vm.state.value
        moveTheToken()
        val report = vm.asked("reports", filter)
        assertNull(vm.state.value.error)
        assertSame("Open editors and screens keyed on it stay", shown.budget, vm.state.value.budget)
        assertSame(shown.summary, vm.state.value.summary)
        assertNotEquals(shown.budgetRevision, vm.state.value.budgetRevision)
        assertSameJson("report", stateless("reports", storedBudget(), filter), report)
        d.verify("after the report")
        d.end()
    }

    @Test fun aBlockRefusedByAnExternalWriteKeepsWhatThatWriteAdded() {
        val (vm, d) = opened()
        val added = JSONObject().put("id", "rule-elsewhere").put("match", "Elsewhere").put("payee", "From elsewhere")
        saveFromElsewhere { it.put("payeeRules", JSONArray(it.rows("payeeRules") + added)) }
        val ours = JSONObject().put("id", "rule-ours").put("match", "Ours").put("payee", "From here")
        vm.upsert("payeeRules", ours)
        vm.settle()
        assertNull(vm.state.value.error)
        val ids = storedBudget().rows("payeeRules").map { it.text("id") }
        assertTrue("Neither is lost: $ids", "rule-elsewhere" in ids && "rule-ours" in ids)
        d.verify("after the block")
        d.end()
    }

    private fun recoveryCopy() = File(session.context.filesDir, "budget-before-replacement.json")

    @Test fun anImportAfterAnExternalWriteIsWrittenOnceAndShown() {
        val (vm, d) = opened()
        val (row, _) = plainRows(vm)
        val incoming = JSONObject(storedBudget().toString()).put("name", "Imported over a move")
        moveTheToken()
        vm.importBudget(incoming.toString())
        vm.settle()
        assertNull(vm.state.value.error)
        assertEquals("Imported over a move", vm.state.value.budget!!.getString("name"))
        assertEquals("Imported over a move", storedBudget().getString("name"))
        d.verify("after the import")
        tap(vm, row, "After the import")
        assertEquals("After the import", memo(row))
        d.end()
    }

    @Test fun anImportThatFindsTheBudgetChangedShowsWhatsThere() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        val incoming = JSONObject(storedBudget().toString()).put("name", "Imported over a change")
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == row }.put("memo", "From elsewhere") }
        vm.importBudget(incoming.toString())
        vm.settle()
        assertEquals(session.context.getString(R.string.guard_replacement_changed), vm.state.value.error)
        assertEquals("Conflict", vm.state.value.budget!!.getString("name"))
        assertEquals("From elsewhere", appMemo(vm, row))
        assertEquals("From elsewhere", memo(row))
        vm.dismissError()
        d.verify("after the refusal")
        d.end()
    }

    @Test fun aRestoreAfterAnExternalWriteIsWrittenOnceAndShown() {
        val (vm, d) = opened()
        val incoming = JSONObject(storedBudget().toString()).put("name", "Replacing it")
        vm.importBudget(incoming.toString())
        vm.settle()
        assertTrue(recoveryCopy().exists())
        moveTheToken()
        vm.restorePreviousBudget()
        vm.settle()
        assertNull(vm.state.value.error)
        assertEquals("Conflict", vm.state.value.budget!!.getString("name"))
        assertEquals("Conflict", storedBudget().getString("name"))
        d.verify("after the restore")
        d.end()
    }

    @Test fun aRestoreThatFindsTheBudgetChangedShowsWhatsThere() {
        val (vm, d) = opened()
        val row = plainRows(vm).first()
        vm.importBudget(JSONObject(storedBudget().toString()).put("name", "Replacing it").toString())
        vm.settle()
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == row }.put("memo", "From elsewhere") }
        vm.restorePreviousBudget()
        vm.settle()
        assertEquals(session.context.getString(R.string.guard_replacement_changed), vm.state.value.error)
        assertEquals("Replacing it", vm.state.value.budget!!.getString("name"))
        assertEquals("From elsewhere", appMemo(vm, row))
        vm.dismissError()
        d.verify("after the refusal")
        d.end()
    }

    @Test fun aNewBudgetAfterTheOpenOneWasTakenAwayOpensAndKeepsNoCopy() {
        val (vm, d) = opened()
        val id = (NativeServices.call("library_snapshot") as JSONObject).getString("activeId")
        NativeServices.call("library_trash", JSONObject().put("id", id))
        assertFalse("Nothing's open", (NativeServices.call("native_budget_status") as JSONObject).optString("token").isNotBlank())
        val name = "After the trash ${System.nanoTime()}"
        vm.createBudget(name, "EUR")
        vm.settle()
        assertNull(vm.state.value.error)
        assertEquals(name, vm.state.value.budget!!.getString("name"))
        assertFalse("No copy to keep: nothing was open", recoveryCopy().exists())
        d.verify("after the create")
        (NativeServices.call("library_snapshot") as JSONObject).getString("activeId").let(session::track)
        d.end()
    }
}
