package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlinx.coroutines.runBlocking
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

/** Months, budgets opening and closing, and new view models: a patch must always land on the right copy. */
@RunWith(AndroidJUnit4::class)
class EngineEditLifecycleTest {
    private lateinit var session: EditSession
    private val today = LocalDate.now().toString()
    private val month = YearMonth.now()

    @Before fun setup() { requireEmulator(); session = EditSession() }
    @After fun clean() { if (::session.isInitialized) session.close() }

    private fun budget(name: String, seed: Long = 31) = SyntheticBudget.build(transactions = 500, seed = seed).put("name", name)
    private fun plainRows(vm: BudgetViewModel) = vm.state.value.budget!!.rows("transactions").filter(Edits::plain).map { it.text("id") }
    private fun tap(vm: BudgetViewModel, id: String, memo: String) { vm.runCommand("workflow", Edits.bulk(listOf(id), "memo", memo)); vm.settle() }
    private fun openVia(vm: BudgetViewModel, id: String) { vm.service("library_open", JSONObject().put("id", id)); vm.settle() }

    /** What holds after anything that replaces the budget: the copy is the store's, and the next edit lands there. */
    private fun landsHere(vm: BudgetViewModel, d: EditDriver, name: String, holds: (String) -> Any = d::verify) {
        assertNull("$name: error", vm.state.value.error)
        holds(name)
        val row = plainRows(vm).first()
        tap(vm, row, "After $name")
        assertNull("$name: error", vm.state.value.error)
        assertEquals("After $name", vm.state.value.budget!!.row("transactions", row)!!.text("memo"))
        assertEquals("After $name", storedRow(row)!!.text("memo"))
        holds("$name, then an edit")
    }

    @Test fun selectingAMonthKeepsTheRootAndAQueuedEditLandsInIt() {
        session.make(budget("Months"))
        val vm = session.model()
        val d = EditDriver(vm, strict = false)
        val root = vm.state.value.budget
        val other = month.minusMonths(2).toString()
        vm.selectMonth(other)
        vm.settle()
        assertSame(root, vm.state.value.budget)
        assertEquals(other, vm.state.value.month)
        d.verify("a month")

        // An edit queued right behind a change of month, and one in the month it's now showing.
        session.onMain {
            vm.selectMonth(month.toString())
            vm.runCommand("workflow", Edits.assign(month.toString(), "c-home-1", 4_321))
        }
        vm.settle()
        assertEquals(month.toString(), vm.state.value.month)
        d.verify("an edit behind a month")
        session.onMain {
            vm.runCommand("workflow", Edits.assign(month.toString(), "c-home-2", 1_234))
            vm.selectMonth(other)
        }
        vm.settle()
        assertEquals(other, vm.state.value.month)
        d.verify("a month behind an edit")
        d.end()
    }

    /** What undo offers on each of A, B, A, and what it does. */
    private fun switching(): List<Boolean> {
        val a = session.make(budget("Switch A"))
        val b = session.make(budget("Switch B"), open = false)
        val vm = session.model()
        val offered = mutableListOf<Boolean>()
        val rowA = plainRows(vm).first()
        tap(vm, rowA, "On A")
        offered += vm.state.value.undoAvailable
        openVia(vm, b)
        offered += vm.state.value.undoAvailable
        tap(vm, plainRows(vm).first(), "On B")
        offered += vm.state.value.undoAvailable
        openVia(vm, a)
        offered += vm.state.value.undoAvailable
        assertEquals("On A", vm.state.value.budget!!.row("transactions", rowA)!!.text("memo"))
        vm.undo()
        vm.settle()
        assertNull(vm.state.value.error)
        offered += vm.state.value.undoAvailable
        offered += vm.state.value.budget!!.row("transactions", rowA)!!.text("memo") == "On A"
        return offered
    }

    /** What the old path did, pinned while it could still be compared: each budget keeps its own undo. */
    @Test fun switchingBudgetsUndoesAsTheOldPathDid() {
        assertEquals(listOf(true, false, true, true, false, false), switching())
    }

    /** The acceptance line: with a budget open, the repository holds it as a working copy, never as text. */
    @Test fun noTextIsHeldWhileABudgetIsOpen() {
        session.make(budget("No text"))
        val vm = session.model()
        val repository = BudgetViewModel::class.java.getDeclaredField("repository").apply { isAccessible = true }.get(vm)
        fun current() = BudgetRepository::class.java.getDeclaredField("current").apply { isAccessible = true }.get(repository)
        fun check(step: String) { vm.settle(); assertNull(vm.state.value.error); assertNotNull(vm.state.value.budget); assertNull(step, current()) }
        check("after a load")
        val row = plainRows(vm).first()
        vm.runCommand("workflow", Edits.bulk(listOf(row), "memo", "A command"))
        check("after a command")
        vm.upsert("categories", JSONObject().put("id", "c-home-1").put("name", "An edit block"))
        check("after an edit block")
        vm.undo()
        check("after an undo")
        vm.redo()
        check("after a redo")
        vm.selectMonth(month.minusMonths(1).toString())
        check("after a month")
        vm.importBudget(storedBudget().put("name", "Imported").toString())
        check("after an import")
        vm.refresh()
        check("after a refresh")
    }

    /** The acceptance line, and what's worked out from the app's copy: the same without `source`, which only the store holds. */
    @Test fun aLightCopyAnswersAsTheWholeOneDoes() = runBlocking {
        session.make(SyntheticBudget.build(transactions = sized(3_000), seed = 33).put("name", "Light"))
        val vm = session.model()
        val light = vm.state.value.budget!!
        val stored = storedBudget()
        val whole = (NativeServices.call("load_budget") as JSONObject).getJSONObject("budget")
        assertFalse(light.has("sourceExport"))
        assertTrue(light.rows("transactions").none { it.has("source") })
        assertTrue(light.rows("transactions").any { it.has("hasImportSource") })
        assertTrue(whole.has("sourceExport"))
        assertEquals(light.rows("transactions").count { it.has("hasImportSource") }, whole.rows("transactions").count { it.has("source") })

        val date = LocalDate.now()
        assertEquals(Uncategorized(whole, date.toString()).counts(whole), Uncategorized(light, date.toString()).counts(light))
        assertEquals(accountBadges(whole, date.toString()), accountBadges(light, date.toString()))
        val (wholeReport, lightReport) = parseReportBudget(whole, date) to parseReportBudget(light, date)
        val from = month.minusMonths(23).toString()
        for (payee in whole.rows("transactions").map { it.text("payee") }.distinct().take(20)) {
            val (a, b) = incomeDrill(whole, wholeReport, payee, from, month.toString(), emptySet()) to incomeDrill(light, lightReport, payee, from, month.toString(), emptySet())
            assertEquals(payee, a.lines, b.lines)
            assertEquals(payee, a.figure, b.figure)
        }

        val export = JSONObject(vm.exportBudget()!!)
        assertSameJson("export and rows", Canon.storeOrder(stored), Canon.storeOrder(export))
        assertSameJson("export and a full load", Canon.storeOrder(whole), Canon.storeOrder(export))
    }

    @Test fun aLightCopyTakesLessHeapThanAWholeOne() {
        session.make(SyntheticBudget.build(transactions = 3_000, seed = 34).put("name", "Heap"))
        fun used(): Long {
            repeat(3) { System.gc(); System.runFinalization() }
            return Runtime.getRuntime().run { totalMemory() - freeMemory() }
        }
        fun held(params: JSONObject): Long {
            val before = used()
            val reply = NativeServices.call("load_budget", params) as JSONObject
            val after = used()
            assertTrue(reply.has("budget")) // Holds it until here.
            return after - before
        }
        val light = held(JSONObject().put("light", true))
        val whole = held(JSONObject())
        android.util.Log.i("EngineEditLifecycle", "Held: light $light bytes, whole $whole bytes")
        assertTrue("Light $light bytes, whole $whole bytes", light < whole)
    }

    @Test fun aGuardFromOneBudgetIsRefusedOnAnother() {
        val a = session.make(budget("Guard A"))
        val b = session.make(budget("Guard B"), open = false)
        val vm = session.model()
        val row = plainRows(vm).first()
        val shown = vm.state.value
        val guard = BudgetEditGuard.record(shown.budget, shown.budgetIdentity, "transactions", row)
        val record = JSONObject(shown.budget!!.row("transactions", row).toString()).put("memo", "Wrong budget")
        openVia(vm, b)
        vm.saveTransaction(record, guard)
        vm.settle()
        assertEquals(session.context.getString(R.string.guard_budget_switched), vm.state.value.error)
        vm.dismissError()
        assertNotEquals("Wrong budget", storedRow(row)!!.text("memo"))
        openVia(vm, a)
        assertNotEquals("Wrong budget", vm.state.value.budget!!.row("transactions", row)!!.text("memo"))
    }

    @Test fun twoBudgetsWithTheSameContentStayApart() {
        val a = session.make(budget("Twins"))
        val b = session.make(budget("Twins"), open = false)
        val vm = session.model()
        val d = EditDriver(vm, strict = false)
        val row = plainRows(vm).first()
        val first = vm.state.value
        openVia(vm, b)
        assertNotEquals(first.budgetIdentity, vm.state.value.budgetIdentity)
        assertNotSame("Same content, another budget", first.budget, vm.state.value.budget)
        tap(vm, row, "Only on B")
        d.verify("on B")
        openVia(vm, a)
        assertNotEquals("Only on B", vm.state.value.budget!!.row("transactions", row)!!.text("memo"))
        landsHere(vm, d, "back on A")
        d.end()
    }

    @Test fun createImportAndRestoreLeaveARightCopy() {
        session.make(budget("Lifecycle"))
        val vm = session.model()
        val d = EditDriver(vm, strict = false)
        val name = "Created ${System.nanoTime()}"
        vm.createBudget(name, "EUR", sample = true)
        vm.settle()
        assertNull(vm.state.value.error)
        session.track((NativeServices.call("library_snapshot") as JSONObject).getString("activeId"))
        assertEquals(name, vm.state.value.budget!!.getString("name"))
        landsHere(vm, d, "create")
        vm.importBudget(budget("Imported", seed = 32).toString())
        vm.settle()
        assertEquals("Imported", vm.state.value.budget!!.getString("name"))
        // A budget that came in whole stays in the draft as it was sent, and only rows read back fill in
        // what the engine works out (`unassigned`), so `verify`'s draft and rows differ until a full read.
        landsHere(vm, d, "import", d::onTheDraft)
        vm.restorePreviousBudget()
        vm.settle()
        assertEquals(name, vm.state.value.budget!!.getString("name"))
        assertEquals(name, storedBudget().getString("name"))
        landsHere(vm, d, "restore", d::onTheDraft)
        d.end()
    }

    @Test fun aFreshStartAndATripThroughTheTrashLeaveARightCopy() {
        val id = session.make(budget("Trash"))
        val vm = session.model()
        val d = EditDriver(vm, strict = false)
        var fresh: String? = null
        vm.service("fresh_start", JSONObject().put("name", "Fresh").put("asOf", today)) { fresh = it as String }
        vm.settle()
        val started = fresh!!
        session.track(started)
        openVia(vm, started)
        assertEquals("Fresh", vm.state.value.budget!!.getString("name"))
        vm.saveTransaction(Edits.transaction("t-fresh", today))
        vm.settle()
        assertNull(vm.state.value.error)
        d.verify("fresh start")

        openVia(vm, id)
        vm.service("library_trash", JSONObject().put("id", started))
        vm.settle()
        assertNull(vm.state.value.error)
        landsHere(vm, d, "trashed another")

        vm.service("library_trash", JSONObject().put("id", id))
        vm.settle()
        assertNull("Nothing's open", vm.state.value.budget)
        vm.service("library_restore", JSONObject().put("id", id))
        vm.settle()
        openVia(vm, id)
        assertEquals("Trash", vm.state.value.budget!!.getString("name"))
        landsHere(vm, d, "restored")
        d.end()
    }

    @Test fun aNewViewModelPicksUpWhereTheOldOneLeftOff() {
        session.make(budget("Second model"))
        val first = session.model()
        val d = EditDriver(first, strict = false)
        tap(first, plainRows(first).first(), "With the first")
        d.verify("the first")
        d.close()
        val second = session.model()
        val again = EditDriver(second, strict = false)
        again.verify("a new view model")
        assertTrue("Undo lives in the service, not the view model", second.state.value.undoAvailable)
        landsHere(second, again, "the second")
        second.undo()
        second.settle()
        assertNull(second.state.value.error)
        again.verify("an undo in the second")
        again.end()
    }

    @Test fun retryAfterARefusalLandsOnce() {
        session.make(budget("Retry"))
        val vm = session.model()
        val d = EditDriver(vm, strict = false)
        val rows = plainRows(vm)
        val count = vm.state.value.budget!!.getJSONArray("transactions").length()
        val guard = vm.state.value.let { BudgetEditGuard.record(it.budget, it.budgetIdentity, "transactions", "t-retried") }
        // Refused for a real reason, then free to go: what's in the way is gone by the retry.
        vm.saveTransaction(Edits.transaction("t-retried", today, account = "no-such-account"), guard)
        vm.settle()
        assertNotNull(vm.state.value.error)
        vm.retry()
        vm.settle()
        assertNotNull(vm.state.value.error)
        assertEquals(count, storedBudget().getJSONArray("transactions").length())
        vm.dismissError()
        tap(vm, rows[0], "Unrelated")
        assertNull(vm.state.value.error)
        d.verify("after the refusals")
        d.end()
    }

    @Test fun importingTheSameBudgetAgainReplacesTheEdit() {
        session.make(budget("Same import"))
        val vm = session.model()
        val d = EditDriver(vm, strict = false)
        val x = storedBudget().put("name", "X").toString()
        vm.importBudget(x)
        vm.settle()
        val row = plainRows(vm).first()
        val before = vm.state.value.budget!!.row("transactions", row)!!.text("memo")
        tap(vm, row, "Edited")
        val edited = vm.state.value.budget
        vm.importBudget(x)
        vm.settle()
        assertNull(vm.state.value.error)
        assertNotSame(edited, vm.state.value.budget)
        assertEquals(before, vm.state.value.budget!!.row("transactions", row)!!.text("memo"))
        assertEquals(before, storedRow(row)!!.text("memo"))
        d.verify("the same import")
        tap(vm, row, "Edited again")
        assertEquals("Edited again", storedRow(row)!!.text("memo"))
        d.end()
    }

    @Test fun creatingTheSameBudgetTwiceWithAnEditBetweenKeepsThemApart() {
        session.make(budget("Same create"))
        val vm = session.model()
        val d = EditDriver(vm, strict = false)
        val name = "Twice ${System.nanoTime()}"
        vm.createBudget(name, "EUR", sample = true)
        vm.settle()
        val first = vm.state.value
        session.track((NativeServices.call("library_snapshot") as JSONObject).getString("activeId"))
        val category = first.budget!!.rows("categories").first().text("id")
        vm.runCommand("workflow", Edits.assign(month.toString(), category, 5_123))
        vm.settle()
        assertNull(vm.state.value.error)
        vm.createBudget(name, "EUR", sample = true)
        vm.settle()
        assertNull(vm.state.value.error)
        session.track((NativeServices.call("library_snapshot") as JSONObject).getString("activeId"))
        assertNotEquals(first.budgetIdentity, vm.state.value.budgetIdentity)
        assertEquals("The first one's edit stays there", 0, vm.state.value.budget!!.rows("assignments").count { it.getLong("amount") == 5_123L })
        d.verify("the second create")
        vm.runCommand("workflow", Edits.assign(month.toString(), category, 7_123))
        vm.settle()
        assertEquals(1, storedBudget().rows("assignments").count { it.getLong("amount") == 7_123L })
        assertEquals(0, storedBudget().rows("assignments").count { it.getLong("amount") == 5_123L })
        d.end()
    }
}
