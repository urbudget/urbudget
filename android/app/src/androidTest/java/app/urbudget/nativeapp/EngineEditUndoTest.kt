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
import java.time.LocalDate
import java.time.YearMonth
import java.util.Random

/** Undo and redo through `patchOnly`: the app's copy has to follow the store at every step. */
@RunWith(AndroidJUnit4::class)
class EngineEditUndoTest {
    private lateinit var session: EditSession
    private val today = LocalDate.now().toString()
    private val month = YearMonth.now().toString()

    @Before fun setup() { requireEmulator(); session = EditSession() }
    @After fun clean() { if (::session.isInitialized) session.close() }

    private fun opened(budget: JSONObject = SyntheticBudget.build(transactions = sized(1_500), seed = 11).put("name", "Undo"), strict: Boolean = false): Pair<BudgetViewModel, EditDriver> {
        session.make(budget)
        val vm = session.model()
        return vm to EditDriver(vm, strict)
    }

    private fun history(vm: BudgetViewModel, d: EditDriver, name: String, run: () -> Unit): String {
        val epoch = vm.operationFailureEpoch
        run()
        vm.settle()
        assertNull("$name: error", vm.state.value.error)
        assertEquals("$name: a failure was recorded", epoch, vm.operationFailureEpoch)
        val stored = Canon.digest(d.verify(name))
        if (d.strict) d.loadsBack(name)
        return stored
    }
    private fun stored() = Canon.digest(storedBudget())
    private fun plainRows(vm: BudgetViewModel) = vm.state.value.budget!!.rows("transactions").filter(Edits::plain).map { it.text("id") }

    /** One of a few kinds of edit. Each row is used once: a guard covers what was shown, so a queue can't reuse one. */
    private fun mixed(d: EditDriver, vm: BudgetViewModel, rows: List<String>, n: Int) {
        val row = listOf(rows[n])
        when (n % 6) {
            0 -> d.workflow("memo $n", Edits.bulk(row, "memo", "Memo $n").getJSONObject("action"))
            1 -> d.workflow("assign $n", Edits.assign(month, "c-home-${n % 6}", 1_000L + n).getJSONObject("action"))
            2 -> Edits.transaction("t-mixed-$n", today).let { record ->
                val expected = changedBudget(stateless("saveTransaction", storedBudget(), JSONObject().put("transaction", JSONObject(record.toString())).put("through", today)))
                d.step("new $n", expected) { vm.saveTransaction(record) }
            }
            // Not a flag it has: that's no change, and on a small budget it can happen.
            3 -> d.workflow("flag $n", Edits.bulk(row, "flag", if (vm.state.value.budget!!.row("transactions", rows[n])!!.text("flag") == "blue") "red" else "blue").getJSONObject("action"))
            4 -> d.workflow("delete $n", Edits.bulk(row, "delete").getJSONObject("action"))
            else -> d.workflow("status $n", Edits.bulk(row, "status", "uncleared").getJSONObject("action"))
        }
    }

    /** Each script runs quiet, and strict, reading the budget back whole after each step, undo and redo as well. */
    @Test fun sixtyEditsUndoToTheLimitAndRedoToTheSameDigests() = sixtyEdits(strict = false)
    @Test fun sixtyEditsStrict() = sixtyEdits(strict = true)

    private fun sixtyEdits(strict: Boolean) {
        val (vm, d) = opened(strict = strict)
        val rows = plainRows(vm)
        repeat(60) { mixed(d, vm, rows, it) }
        val after = d.digests.map { it.second }
        assertEquals(60, after.size)
        assertEquals(after.last(), stored())

        for (back in 1..50) {
            val digest = history(vm, d, "undo $back") { vm.undo() }
            assertEquals("undo $back", after[59 - back], digest)
            assertEquals("undo $back", back < 50, vm.state.value.undoAvailable)
            assertTrue("undo $back", vm.state.value.redoAvailable)
        }
        // Fifty is the limit: one more does nothing.
        val last = vm.state.value
        history(vm, d, "extra undo") { vm.undo() }
        assertSame(last.budget, vm.state.value.budget)
        assertEquals(last.budgetRevision, vm.state.value.budgetRevision)
        assertEquals(after[9], stored())

        for (forward in 1..50) {
            val digest = history(vm, d, "redo $forward") { vm.redo() }
            assertEquals("redo $forward", after[9 + forward], digest)
            assertEquals("redo $forward", forward < 50, vm.state.value.redoAvailable)
        }
        d.end()
    }

    @Test fun aNewEditAfterUndoClearsRedo() = newEditClearsRedo(strict = false)
    @Test fun aNewEditAfterUndoClearsRedoStrict() = newEditClearsRedo(strict = true)

    private fun newEditClearsRedo(strict: Boolean) {
        val (vm, d) = opened(strict = strict)
        val rows = plainRows(vm)
        repeat(6) { mixed(d, vm, rows, it) }
        repeat(3) { history(vm, d, "undo $it") { vm.undo() } }
        assertTrue(vm.state.value.redoAvailable)
        d.workflow("a new edit", Edits.bulk(listOf(rows[50]), "memo", "Later").getJSONObject("action"))
        assertFalse(vm.state.value.redoAvailable)
        val state = vm.state.value
        history(vm, d, "redo with nothing to redo") { vm.redo() }
        assertSame(state.budget, vm.state.value.budget)
        d.end()
    }

    @Test fun oneUndoEachAcrossEditBlocksCommandsAndARepeatingTransaction() = oneUndoEach(strict = false)
    @Test fun oneUndoEachStrict() = oneUndoEach(strict = true)

    private fun oneUndoEach(strict: Boolean) {
        val (vm, d) = opened(strict = strict)
        val start = stored()
        val rows = plainRows(vm)
        d.step("edit block", null) { vm.upsert("categories", JSONObject().put("id", "c-home-1").put("name", "Renamed by an edit block")) }
        val afterBlock = stored()
        d.workflow("command", Edits.bulk(listOf(rows[0]), "memo", "Command").getJSONObject("action"))
        val afterCommand = stored()
        val record = Edits.transaction("t-repeating", today)
        val schedule = JSONObject().put("id", "t-repeating").put("transaction", JSONObject(record.toString())).put("nextDate", today).put("anchorDate", today).put("frequency", "monthly").put("paused", false)
        d.step("repeating", null) { vm.saveRepeating(schedule, record) }
        assertNotNull(vm.state.value.budget!!.row("transactions", "t-repeating"))
        assertNotNull(vm.state.value.budget!!.row("schedules", "t-repeating"))
        val afterRepeating = stored()

        assertEquals("One undo takes back both", afterCommand, history(vm, d, "undo the repeating") { vm.undo() })
        assertNull(vm.state.value.budget!!.row("transactions", "t-repeating"))
        assertNull(vm.state.value.budget!!.row("schedules", "t-repeating"))
        assertEquals(afterBlock, history(vm, d, "undo the command") { vm.undo() })
        assertEquals(start, history(vm, d, "undo the block") { vm.undo() })
        assertFalse(vm.state.value.undoAvailable)
        assertEquals(afterBlock, history(vm, d, "redo the block") { vm.redo() })
        assertEquals(afterCommand, history(vm, d, "redo the command") { vm.redo() })
        assertEquals(afterRepeating, history(vm, d, "redo the repeating") { vm.redo() })
        d.end()
    }

    @Test fun undoingADeletePutsBackTheRowByteForByte() = undoingADelete(strict = false)
    @Test fun undoingADeleteStrict() = undoingADelete(strict = true)

    private fun undoingADelete(strict: Boolean) {
        val (vm, d) = opened(strict = strict)
        val budget = vm.state.value.budget!!
        val imported = budget.rows("transactions").filter { it.has("hasImportSource") && it.rows("splits").size > 1 && it.rows("splits").any { part -> part.has("source") } }.take(1) +
            budget.rows("transactions").filter(Edits::plain).filter { it.has("hasImportSource") }.take(2)
        assertEquals(3, imported.size)
        val ids = imported.map { it.text("id") }
        fun rows() = storedBudget().rows("transactions")
        val before = rows().filter { it.text("id") in ids }.associate { it.text("id") to Canon.text(it) }
        val positions = rows().map { it.text("id") }

        d.workflow("delete one", Edits.bulk(ids.take(1), "delete").getJSONObject("action"))
        assertNull(storedRow(ids[0]))
        history(vm, d, "undo the delete") { vm.undo() }
        assertEquals(before[ids[0]], Canon.text(storedRow(ids[0])))
        assertEquals(Canon.text(Canon.windowRow(storedRow(ids[0])!!)), Canon.text(vm.state.value.budget!!.row("transactions", ids[0])))

        d.workflow("delete all three", Edits.bulk(ids, "delete").getJSONObject("action"))
        history(vm, d, "undo the bulk delete") { vm.undo() }
        assertEquals(before, rows().filter { it.text("id") in ids }.associate { it.text("id") to Canon.text(it) })
        assertEquals("Back where they were", positions, rows().map { it.text("id") })
        d.end()
    }

    @Test fun undoingTheFirstTextImportRemovesItsList() = undoingTheFirstTexts(strict = false)
    @Test fun undoingTheFirstTextImportStrict() = undoingTheFirstTexts(strict = true)

    private fun undoingTheFirstTexts(strict: Boolean) {
        val budget = SyntheticBudget.build(transactions = 400, seed = 12).put("name", "Undo texts")
        budget.remove("smsImportedMessageIds")
        val (vm, d) = opened(budget, strict)
        val params = JSONObject("""{"messages":[{"id":"sms:first","sender":"Bank","receivedDate":"$today","receivedTime":"","body":"Purchase EUR 12.34 at Bakery card *1234"}],
            "analysis":{"items":[{"sourceIds":["sms:first"],"disposition":"transaction","skip":null,"note":"Purchase","evidence":[{"sourceId":"sms:first","quote":"EUR 12.34"}],"unsure":[],
            "transaction":{"date":"$today","amount":"-12.34","currency":"EUR","writtenAmount":"12.34","payee":"Bakery","memo":"","kind":"expense","accountRef":"*1234","otherRef":null,"categoryId":null,"accountId":"checking","transferAccountId":null}}]},
            "decisions":[{"index":0,"action":"new","matchId":null}]}""")
        val start = stored()
        d.command("first texts", "smsBatchApply", params)
        assertEquals(1, vm.state.value.budget!!.getJSONArray("smsImportedMessageIds").length())
        assertEquals(start, history(vm, d, "undo the texts") { vm.undo() })
        assertFalse(vm.state.value.budget!!.has("smsImportedMessageIds"))
        assertFalse(storedBudget().has("smsImportedMessageIds"))
        d.end()
    }

    @Test fun anExternalWriteResetsUndoAsItAlwaysHas() = anExternalWrite(strict = false)
    @Test fun anExternalWriteResetsUndoStrict() = anExternalWrite(strict = true)

    private fun anExternalWrite(strict: Boolean) {
        val (vm, d) = opened(strict = strict)
        val rows = plainRows(vm)
        repeat(3) { mixed(d, vm, rows, it) }
        assertTrue(vm.state.value.undoAvailable)
        moveTheToken()
        vm.refresh()
        vm.settle()
        d.verify("after the write")
        assertFalse(vm.state.value.undoAvailable)
        val state = vm.state.value
        history(vm, d, "undo with nothing to undo") { vm.undo() }
        assertSame(state.budget, vm.state.value.budget)
        assertEquals(state.budgetRevision, vm.state.value.budgetRevision)
        d.end()
    }

    @Test fun anUndoOfTheOldPathsSaveWorksThroughThePatchPath() = anUndoOfAHeavySave(strict = false)
    @Test fun anUndoOfTheOldPathsSaveStrict() = anUndoOfAHeavySave(strict = true)

    private fun anUndoOfAHeavySave(strict: Boolean) {
        // Undoing a heavy change goes through `Store::save`. The app's copy holds none of it, so it stays as it is.
        val (vm, d) = opened(strict = strict)
        fun export() = storedBudget().getJSONObject("sourceExport")
        val budget = storedBudget().put("sourceExport", JSONObject().put("data", JSONObject().put("server_knowledge", 8)))
        val first = export().toString()
        val shown = vm.state.value.budget
        val before = d.mark()
        vm.importBudget(budget.toString())
        vm.settle()
        d.check("a new export", before, change = EditDriver.Change.Hidden)
        assertEquals(8, export().getJSONObject("data").getInt("server_knowledge"))
        history(vm, d, "undo the import") { vm.undo() }
        assertEquals(first, export().toString())
        history(vm, d, "redo the import") { vm.redo() }
        assertEquals(8, export().getJSONObject("data").getInt("server_knowledge"))
        assertSame(shown, vm.state.value.budget)
        d.end()
    }

    /**
     * What the old path's differential test did on random edits, now that path is gone: an edit is
     * checked against the stateless call on the budget before it, which is what that path ran, and
     * an undo or a redo against the digest it goes back to, which is what it kept.
     */
    @Test fun threeHundredRandomEditsFollowTheStatelessPath() = randomEdits(strict = false)
    @Test fun threeHundredRandomEditsStrict() = randomEdits(strict = true)

    private fun randomEdits(strict: Boolean) {
        val (vm, d) = opened(SyntheticBudget.build(transactions = sized(800), seed = 41).put("name", "Random"), strict)
        val random = Random(41)
        val categories = listOf("c-bills-0", "c-home-1", "c-health-2", "c-fun-3", "c-transport-4", "c-giving-5")
        val past = ArrayDeque<String>()
        val undone = ArrayDeque<String>()
        var current = stored()
        fun edit(name: String, method: String, params: JSONObject, whole: JSONObject, run: () -> Unit) {
            val expected = changedBudget(stateless(method, whole, params))
            // A flag a row already has, say: no change, so no undo either.
            val changes = Canon.digest(Canon.forStateless(expected)) != Canon.digest(Canon.forStateless(whole))
            d.step(name, expected, changes, run)
            if (!changes) return
            past.addLast(current)
            if (past.size > 50) past.removeFirst()
            undone.clear()
            current = d.digests.last().second
        }
        repeat(300) { n ->
            val rows = vm.state.value.budget!!.rows("transactions").filter(Edits::plain)
            fun row() = rows[random.nextInt(rows.size)]
            fun category() = categories[random.nextInt(categories.size)]
            fun amount() = 1_000L + random.nextInt(50_000)
            fun workflow(action: JSONObject) {
                val params = JSONObject().put("action", action.getJSONObject("action"))
                edit("edit $n", "workflow", params, storedBudget()) { vm.runCommand("workflow", params) }
            }
            fun save(record: JSONObject) {
                val whole = storedBudget()
                val params = JSONObject().put("transaction", Canon.restored(record, whole)).put("through", today)
                edit("save $n", "saveTransaction", params, whole) { vm.saveTransaction(record) }
            }
            fun bulk(operation: String, value: Any? = null) = workflow(Edits.bulk(listOf(row().text("id")), operation, value))
            when (random.nextInt(14)) {
                0 -> bulk("memo", "Memo $n مذكرة")
                1 -> bulk("status", "uncleared")
                2 -> bulk("flag", "green")
                3 -> bulk("category", category())
                4 -> bulk("delete")
                5 -> bulk("duplicate")
                6 -> workflow(Edits.assign(month, category(), amount()))
                7 -> workflow(Edits.assign(YearMonth.now().plusMonths(1).toString(), category(), amount()))
                8 -> save(Edits.transaction("t-new-$n", today, amount = -(100L + random.nextInt(9_000))))
                9 -> save(JSONObject(row().toString()).put("payee", "Paid $n"))
                10 -> save(Edits.transaction("t-transfer-$n", today, amount = -2_000).put("splits", JSONArray()).put("transferAccountId", "savings").put("transferStatus", "cleared"))
                11, 12 -> {
                    val digest = history(vm, d, "undo $n") { vm.undo() }
                    assertEquals("undo $n", past.lastOrNull() ?: current, digest)
                    if (past.isNotEmpty()) { past.removeLast(); undone.addLast(current); current = digest }
                }
                else -> {
                    val digest = history(vm, d, "redo $n") { vm.redo() }
                    assertEquals("redo $n", undone.lastOrNull() ?: current, digest)
                    if (undone.isNotEmpty()) { undone.removeLast(); past.addLast(current); current = digest }
                }
            }
            assertEquals("undo $n", past.isNotEmpty(), vm.state.value.undoAvailable)
            assertEquals("redo $n", undone.isNotEmpty(), vm.state.value.redoAvailable)
        }
        d.end()
    }
}
