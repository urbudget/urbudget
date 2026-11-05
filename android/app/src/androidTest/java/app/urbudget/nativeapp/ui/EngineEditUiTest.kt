package app.urbudget.nativeapp.ui

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleCallback
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.EditDriver.Change
import app.urbudget.nativeapp.R
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.time.LocalDate
import java.time.YearMonth
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * The real app, tapped the way a person taps it, with the kit's checks after each scenario: what the
 * screen did has to be what's stored. Only on an isolated emulator.
 */
@RunWith(AndroidJUnit4::class)
class EngineEditUiTest {
    @get:Rule val compose = createEmptyComposeRule()
    private lateinit var session: EditSession
    private lateinit var preferences: NativePreferences
    private lateinit var scenario: ActivityScenario<MainActivity>
    private lateinit var vm: BudgetViewModel
    private lateinit var driver: EditDriver
    private lateinit var mark: EditDriver.Mark
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = session.context
    private val today = LocalDate.now().toString()
    private val month = YearMonth.now()
    /** Its own for each run: the library lists what's in the Trash too, under the same names. */
    private val name = "Tapped ${System.nanoTime()}"

    private fun words(id: Int, vararg args: Any) = context.getString(id, *args)
    private fun count(id: Int, n: Int) = context.resources.getQuantityString(id, n, n.toString())

    @Before fun setup() {
        requireEmulator()
        session = EditSession()
        preferences = NativePreferences(context)
        preferences.automaticSync = false
        preferences.automaticSchedules = false
        preferences.startPage = "budget"
        session.make(SyntheticBudget.build(transactions = 400, seed = 61).put("name", name))
        launch()
    }

    private fun launch() {
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario.onActivity { vm = ViewModelProvider(it)[BudgetViewModel::class.java] }
        compose.waitUntil(30_000) { vm.state.value.budget != null }
        settle()
        driver = EditDriver(vm, strict = false)
        mark = driver.mark()
    }

    @After fun clean() {
        try {
            // A scenario's one full read, unless it ended with no budget.
            if (::vm.isInitialized && vm.state.value.budget != null) driver.loadsBack("the end")
        } finally {
            if (::driver.isInitialized) driver.close()
            if (::scenario.isInitialized) scenario.close()
            if (::preferences.isInitialized) { preferences.automaticSync = true; preferences.automaticSchedules = true }
            if (::session.isInitialized) session.close()
        }
    }

    private fun settle() { vm.settle(); compose.waitForIdle() }
    private fun budget() = vm.state.value.budget!!
    private fun row(id: String) = budget().row("transactions", id)

    /** The kit's checks on all that's happened since the last of these, which was `change`. */
    private fun finish(name: String, change: Change = Change.Edit) {
        settle()
        driver.check(name, mark, change = change)
        mark = driver.mark()
    }

    private fun waitFor(matcher: SemanticsMatcher, timeout: Long = 20_000) =
        compose.waitUntil(timeout) { compose.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty() }
    private fun waitForTag(tag: String) = waitFor(hasTestTag(tag))
    private fun waitForText(text: String, substring: Boolean = false) = waitFor(hasText(text, substring = substring))
    private fun press(text: String) {
        val button = hasText(text) and hasClickAction()
        waitFor(button)
        val node = compose.onAllNodes(button).onLast()
        // Some of these sit below the fold of a scrolling form, where a tap would miss.
        runCatching { node.performScrollTo() }
        node.performClick()
    }
    private val inPicker = hasAnyAncestor(hasTestTag("category-picker"))
    private fun pick(category: String) {
        val row = hasText(category) and hasClickAction() and inPicker
        waitFor(row)
        compose.onNode(row).performClick()
        compose.waitForIdle()
    }
    private fun pressDescribed(label: String) {
        val button = hasContentDescription(label) and hasClickAction()
        waitFor(button)
        compose.onAllNodes(button).onLast().performClick()
    }
    private fun keys(digits: String) = digits.forEach { press(it.toString()) }
    private fun tab(index: Int) { compose.onNodeWithTag("tab:$index").performClick(); compose.waitForIdle() }

    /**
     * Puts a row of the register at the top of the list, clear of the floating dock. The list is built
     * on a worker, so it can take a moment.
     */
    private fun reach(id: String) {
        compose.waitUntil(20_000) { runCatching { compose.onNodeWithTag("register").performScrollToKey(id); true }.getOrDefault(false) }
        compose.waitForIdle()
    }
    private fun rowsToTap(n: Int) = budget().rows("transactions").filter(Edits::plain).sortedByDescending { it.text("date") + it.text("createdAt") }.take(n).map { it.text("id") }

    private fun newTransaction(amount: String, payee: String, category: String = "Bills 2") {
        compose.onNodeWithTag("tx-add").performClick()
        waitForTag("tx-save")
        keys(amount)
        press(words(R.string.common_done))
        waitFor(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput(payee)
        press(words(R.string.payee_add_named, payee))
        pick(category)
    }

    private fun longPress(tag: String) = compose.onNodeWithTag(tag).performTouchInput { longClick() }
    private fun menu(item: String) { pressDescribed(words(R.string.common_more_actions)); press(item) }

    @Test fun u01ClearARowThenTapItAgain() {
        tab(1)
        val id = rowsToTap(1).single()
        reach(id)
        compose.onNodeWithTag("tx-cleared:$id").performClick()
        settle()
        assertEquals("uncleared", row(id)!!.text("status"))
        compose.onNodeWithTag("tx-cleared:$id").performClick()
        settle()
        assertEquals("cleared", row(id)!!.text("status"))
        assertEquals("cleared", storedRow(id)!!.text("status"))
        finish("U1")
    }

    @Test fun u02ThreeQuickTapsOnThreeRows() {
        tab(1)
        val ids = rowsToTap(10)
        reach(ids[0])
        val rows = ids.filter { compose.onAllNodesWithTag("tx-cleared:$it").fetchSemanticsNodes().isNotEmpty() }.take(3)
        assertEquals("Three rows on screen: $rows", 3, rows.size)
        rows.forEach { compose.onNodeWithTag("tx-cleared:$it").performClick() }
        settle()
        rows.forEach { assertEquals(it, "uncleared", storedRow(it)!!.text("status")) }
        finish("U2")
    }

    @Test fun u03ApprovingDropsTheNotice() {
        tab(1)
        val waiting = budget().rows("transactions").count { it.has("approved") && !it.optBoolean("approved", true) }
        waitForText(count(R.plurals.transactions_to_approve, waiting))
        val id = budget().rows("transactions").filter { it.has("approved") && !it.optBoolean("approved", true) && it.text("status") != "reconciled" }
            .maxBy { it.text("date") }.text("id")
        reach(id)
        compose.onNodeWithTag("tx-approve:$id").performClick()
        settle()
        assertTrue(row(id)!!.optBoolean("approved", true))
        compose.onNodeWithTag("register").performScrollToIndex(0)
        waitForText(count(R.plurals.transactions_to_approve, waiting - 1))
        finish("U3")
    }

    @Test fun u04SelectingRowsAndActingOnThem() {
        tab(1)
        val (a, b, c) = rowsToTap(3)
        reach(a)
        longPress("tx:$a")
        reach(b)
        compose.onNodeWithTag("tx:$b").performClick()
        press(words(R.string.category_label))
        pick("Bills 1")
        settle()
        for (id in listOf(a, b)) assertEquals("c-bills-0", row(id)!!.rows("splits")[0].text("categoryId"))
        finish("U4 category")

        reach(a)
        longPress("tx:$a")
        press(words(R.string.flag_label))
        press(words(R.string.flag_blue).let { name -> budget().optJSONObject("flagNames")?.text("blue")?.ifBlank { null } ?: name })
        settle()
        assertEquals("blue", row(a)!!.text("flag"))
        finish("U4 flag")

        // The bar only clears, so this needs a row that isn't.
        reach(b)
        compose.onNodeWithTag("tx-cleared:$b").performClick()
        settle()
        assertEquals("uncleared", row(b)!!.text("status"))
        finish("U4 uncleared")
        reach(b)
        longPress("tx:$b")
        press(words(R.string.status_cleared))
        settle()
        assertEquals("cleared", row(b)!!.text("status"))
        finish("U4 status")

        reach(c)
        longPress("tx:$c")
        press(words(R.string.common_delete))
        compose.waitForIdle()
        press(words(R.string.common_delete))
        settle()
        assertNull(row(c))
        assertNull(storedRow(c))
        finish("U4 delete")
    }

    @Test fun u05AddingThroughTheKeypad() {
        tab(1)
        val before = budget().rows("transactions").map { it.text("id") }.toSet()
        newTransaction("1234", "Keypad shop")
        compose.onNodeWithTag("tx-save").performClick()
        settle()
        val added = budget().rows("transactions").single { it.text("id") !in before }
        assertEquals(-1234L, added.getLong("amount"))
        assertEquals("Keypad shop", added.text("payee"))
        assertEquals("c-bills-1", added.rows("splits")[0].text("categoryId"))
        assertTrue("A time, or the ledger can't order it in the day", added.text("createdAt").isNotBlank())
        assertEquals(added.text("createdAt"), storedRow(added.text("id"))!!.text("createdAt"))
        finish("U5")
    }

    @Test fun u06EditingAnImportedRowKeepsItsSource() {
        tab(1)
        val id = budget().rows("transactions").filter { Edits.plain(it) && it.has("hasImportSource") }.maxBy { it.text("date") }.text("id")
        val source = Canon.text(storedRow(id)!!.get("source"))
        reach(id)
        compose.onNodeWithTag("tx:$id").performClick()
        waitForTag("tx-memo")
        compose.onNodeWithTag("tx-memo").performTextReplacement("Edited on the phone")
        compose.onNodeWithTag("tx-save").performClick()
        settle()
        assertEquals("Edited on the phone", storedRow(id)!!.text("memo"))
        assertEquals(source, Canon.text(storedRow(id)!!.get("source")))
        assertFalse("The app's copy stays light", row(id)!!.has("source"))
        finish("U6")
    }

    @Test fun u07SplittingARow() {
        tab(1)
        val line = budget().rows("transactions").filter(Edits::plain).filter { it.getLong("amount") <= -2_000 }.maxBy { it.text("date") }
        val id = line.text("id")
        reach(id)
        compose.onNodeWithTag("tx:$id").performClick()
        waitForTag("tx-save")
        press(words(R.string.split_between_categories))
        val total = -line.getLong("amount")
        val whole = hasSetTextAction() and hasText(amountText(total))
        waitFor(whole)
        compose.onNode(whole).performTextReplacement(amountText(total - 1_000))
        // The keyboard moves everything just as the next tap lands.
        androidx.test.espresso.Espresso.closeSoftKeyboard()
        compose.waitForIdle()
        press(words(R.string.split_use_left, money(1_000, budget().text("currency"))))
        press(words(R.string.category_choose))
        pick("Bills 1")
        compose.onNodeWithTag("tx-save").performClick()
        settle()
        val parts = storedRow(id)!!.rows("splits")
        assertEquals(2, parts.size)
        assertEquals(-1_000L, parts[1].getLong("amount"))
        assertEquals("c-bills-0", parts[1].text("categoryId"))
        finish("U7")
    }

    @Test fun u08ATransfer() {
        tab(1)
        val before = budget().rows("transactions").map { it.text("id") }.toSet()
        compose.onNodeWithTag("tx-add").performClick()
        waitForTag("tx-save")
        keys("2500")
        press(words(R.string.common_done))
        waitForText(words(R.string.payee_section_transfers))
        press("Savings")
        compose.waitForIdle()
        compose.onNodeWithTag("tx-save").performClick()
        settle()
        val added = budget().rows("transactions").single { it.text("id") !in before }
        assertEquals("savings", added.text("transferAccountId"))
        assertEquals(-2_500L, added.getLong("amount"))
        finish("U8")
    }

    @Test fun u09DeletingFromTheEditor() {
        tab(1)
        val id = rowsToTap(1).single()
        reach(id)
        compose.onNodeWithTag("tx:$id").performClick()
        waitForTag("tx-save")
        compose.onNode(hasText(words(R.string.transaction_delete)) and hasClickAction()).performScrollTo().performClick()
        press(words(R.string.common_delete))
        settle()
        assertNull(row(id))
        assertNull(storedRow(id))
        finish("U9")
    }

    @Test fun u10AssigningTwiceInOneSheet() {
        waitForTag("assign:c-bills-0")
        compose.onNodeWithTag("assign:c-bills-0").performClick()
        waitForText(words(R.string.assign_action))
        keys("5000")
        press(words(R.string.assign_action))
        settle()
        fun assigned() = storedBudget().rows("assignments").first { it.text("month") == month.toString() && it.text("categoryId") == "c-bills-0" }.getLong("amount")
        assertEquals(5_000L, assigned())
        finish("U10 first")
        press(words(R.string.category_assigned_in, shortMonthLabel(month.toString())))
        waitForText(words(R.string.assign_action))
        keys("7000")
        press(words(R.string.assign_action))
        settle()
        assertEquals(7_000L, assigned())
        finish("U10 second")
    }

    @Test fun u11MovingMoney() {
        waitForTag("category:c-bills-1")
        compose.onNodeWithTag("category:c-bills-1").performClick()
        press(words(R.string.move_money))
        waitForText(words(R.string.category_choose_a))
        fun available(id: String) = vm.state.value.summary!!.rows("categories").first { it.text("id") == id }
        val (source, target) = available("c-bills-1").getLong("available") to available("c-bills-0").getLong("available")
        // From this category, to the first one on the list.
        press(words(R.string.category_choose_a))
        pick("Bills 1")
        press(words(R.string.move_action))
        settle()
        assertEquals("All of it moved", 0L, available("c-bills-1").getLong("available"))
        assertEquals(source + target, available("c-bills-0").getLong("available"))
        finish("U11")
    }

    /** Opens Quick assign's Assigned last month, ready to apply. */
    private fun quickAssign() {
        waitFor(hasClickAction() and hasText(words(R.string.category_unassigned)))
        compose.onAllNodes(hasClickAction() and hasText(words(R.string.category_unassigned))).onFirst().performClick()
        val option = hasText(words(R.string.quick_assigned_last_month), substring = true) and hasClickAction()
        waitFor(option)
        compose.onAllNodes(option).onFirst().performScrollTo().performClick()
    }
    private fun applyButton(): SemanticsMatcher {
        val start = context.resources.getQuantityString(R.plurals.quick_apply_to, 2, "2").substringBefore("2")
        return SemanticsMatcher("Apply") { node -> node.config.getOrNull(SemanticsProperties.Text)?.any { it.text.startsWith(start) } == true } and hasClickAction()
    }

    @Test fun u12QuickAssignPreviewThenApply() {
        val shown = vm.state.value
        quickAssign()
        waitFor(applyButton())
        // A preview saves nothing.
        assertSame(shown.budget, vm.state.value.budget)
        assertEquals(shown.budgetRevision, vm.state.value.budgetRevision)
        driver.verify("U12 preview")
        compose.onNode(applyButton()).performClick()
        settle()
        assertNotEquals(shown.budgetRevision, vm.state.value.budgetRevision)
        finish("U12")
    }

    @Test fun u13AssigningInAnotherMonth() {
        waitForTag("assign:c-bills-0")
        pressDescribed(words(R.string.month_previous))
        settle()
        val previous = month.minusMonths(1).toString()
        assertEquals(previous, vm.state.value.month)
        waitForTag("assign:c-bills-0")
        compose.onNodeWithTag("assign:c-bills-0").performClick()
        waitForText(words(R.string.assign_action))
        keys("6100")
        press(words(R.string.assign_action))
        settle()
        assertEquals(6_100L, storedBudget().rows("assignments").first { it.text("month") == previous && it.text("categoryId") == "c-bills-0" }.getLong("amount"))
        assertEquals(previous, vm.state.value.month)
        assertEquals(6_100L, vm.state.value.summary!!.rows("categories").first { it.text("id") == "c-bills-0" }.getLong("assigned"))
        finish("U13")
    }

    @Test fun u14ReconcilingBothWays() {
        fun open() {
            tab(2)
            waitForText("Savings")
            compose.onAllNodesWithText("Savings").onFirst().performTouchInput { longClick() }
            press(words(R.string.reconcile_action))
        }
        open()
        val shown = vm.state.value
        press(words(R.string.common_yes))
        waitForText(words(R.string.reconcile_action))
        assertSame("A preview leaves nothing behind", shown.budget, vm.state.value.budget)
        driver.verify("U14 preview")
        press(words(R.string.reconcile_action))
        settle()
        finish("U14 matching")

        open()
        val before = budget().rows("transactions").map { it.text("id") }.toSet()
        press(words(R.string.common_no))
        val cleared = vm.state.value.summary!!.rows("accounts").first { it.text("id") == "savings" }.getLong("cleared")
        waitFor(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextReplacement(amountText(cleared + 12_300))
        press(words(R.string.common_review))
        waitForText(words(R.string.reconcile_adjust_finish))
        assertEquals("The preview's adjustment isn't in the budget", before, budget().rows("transactions").map { it.text("id") }.toSet())
        press(words(R.string.reconcile_adjust_finish))
        settle()
        val added = budget().rows("transactions").single { it.text("id") !in before }
        assertEquals(12_300L, added.getLong("amount"))
        finish("U14 with an adjustment")
    }

    @Test fun u15Schedules() {
        tab(1)
        fun due() = budget().rows("schedules").filter { !it.optBoolean("paused") && it.text("nextDate").isNotBlank() && it.text("nextDate") <= today }.sortedBy { it.text("nextDate") }
        val first = due()[0]
        waitForText(words(R.string.schedule_enter))
        compose.onAllNodes(hasText(words(R.string.schedule_enter)) and hasClickAction()).onFirst().performClick()
        settle()
        assertNotEquals(first.text("nextDate"), budget().row("schedules", first.text("id"))!!.text("nextDate"))
        finish("U15 enter")

        val skipped = due()[0]
        val count = budget().getJSONArray("transactions").length()
        compose.onAllNodes(hasText(words(R.string.schedule_skip)) and hasClickAction()).onFirst().performClick()
        settle()
        assertNotEquals(skipped.text("nextDate"), budget().row("schedules", skipped.text("id"))!!.text("nextDate"))
        assertEquals("Skipping enters nothing", count, budget().getJSONArray("transactions").length())
        finish("U15 skip")

        // A due schedule saved with no change enters it, and the dialog closes.
        val opened = due()[0]
        compose.onAllNodes(hasContentDescription(words(R.string.schedule_actions)) and hasClickAction()).onFirst().performClick()
        press(words(R.string.common_edit))
        waitForTag("tx-save")
        compose.onNodeWithTag("tx-save").performClick()
        settle()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("tx-save").fetchSemanticsNodes().isEmpty() }
        settle()
        assertNotEquals(opened.text("nextDate"), budget().row("schedules", opened.text("id"))!!.text("nextDate"))
        finish("U15 no change")

        tab(4)
        press(words(R.string.page_scheduled))
        press(words(R.string.schedule_enter_all_due))
        settle()
        assertTrue(due().isEmpty())
        finish("U15 enter all")
    }

    @Test fun u16ARepeatingTransactionIsOneUndo() {
        tab(1)
        val before = budget().rows("transactions").map { it.text("id") }.toSet()
        val schedules = budget().rows("schedules").map { it.text("id") }.toSet()
        newTransaction("3100", "Gym membership")
        compose.onNode(hasText(words(R.string.schedule_repeat), substring = true) and hasClickAction()).performScrollTo().performClick()
        press(frequencies().first { it.first == "monthly" }.second)
        compose.waitForIdle()
        compose.onNodeWithTag("tx-save").performClick()
        settle()
        val added = budget().rows("transactions").single { it.text("id") !in before }
        assertTrue(budget().rows("schedules").any { it.text("id") == added.text("id") })
        finish("U16")
        menu(words(R.string.common_undo))
        settle()
        assertNull(row(added.text("id")))
        assertEquals(schedules, budget().rows("schedules").map { it.text("id") }.toSet())
        finish("U16 undone", Change.History)
    }

    /** Opens a category's sheet, wherever its row has got to. */
    private fun openCategory(id: String) {
        waitForTag("budget-list")
        compose.onNodeWithTag("budget-list").performScrollToKey(id)
        waitForTag("category:$id")
        compose.onNodeWithTag("category:$id").performClick()
        compose.waitForIdle()
    }
    private fun categoryMenu(item: Int) {
        pressDescribed(words(R.string.category_actions))
        press(words(item))
    }

    @Test fun u17ACategoryFromStartToFinish() {
        pressDescribed(words(R.string.group_actions_for, "Bills"))
        press(words(R.string.category_add))
        waitFor(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onLast().performTextInput("Added U17")
        press(words(R.string.common_add))
        settle()
        val id = budget().rows("categories").single { it.text("name") == "Added U17" }.text("id")
        fun category() = budget().row("categories", id)
        assertEquals("Bills", category()!!.text("group"))
        finish("U17 create")

        openCategory(id)
        categoryMenu(R.string.category_details)
        waitFor(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextReplacement("Renamed U17")
        press(words(R.string.common_save))
        settle()
        assertEquals("Renamed U17", category()!!.text("name"))
        finish("U17 rename")

        openCategory(id)
        press(words(R.string.target_create))
        keys("5000")
        press(words(R.string.common_save))
        settle()
        assertEquals(5_000L, category()!!.getLong("target"))
        assertTrue(category()!!.has("goal"))
        finish("U17 target")

        openCategory(id)
        press(words(R.string.target_edit))
        waitFor(hasScrollToKeyAction())
        compose.onAllNodes(hasScrollToKeyAction()).onLast().performScrollToKey("cadence")
        press(words(R.string.target_remove))
        settle()
        assertEquals(0L, category()!!.getLong("target"))
        assertFalse("Saved with the goal gone, as a merge couldn't", category()!!.has("goal"))
        assertFalse(storedBudget().row("categories", id)!!.has("goal"))
        finish("U17 target removed")

        openCategory(id)
        categoryMenu(R.string.category_delete)
        press(words(R.string.common_delete))
        settle()
        assertNull(category())
        finish("U17 delete")

        openCategory("c-bills-2")
        categoryMenu(R.string.category_hide)
        settle()
        assertTrue(budget().row("categories", "c-bills-2")!!.optBoolean("hidden"))
        finish("U17 hide")
    }

    @Test fun u18APayeeRenamedAndARuleApplied() {
        fun payees(name: String) = budget().rows("transactions").count { it.text("payee") == name }
        val before = payees("Spotify")
        assertTrue("The budget needs the payee", before > 0)
        tab(4)
        press(words(R.string.page_payees))
        waitFor(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextInput("Spotify")
        val line = hasText("Spotify") and hasClickAction() and !hasSetTextAction()
        waitFor(line)
        compose.onAllNodes(line).onFirst().performClick()
        val name = hasSetTextAction() and hasText(words(R.string.name_label))
        waitFor(name)
        compose.onNode(name).performTextReplacement("Spotify AB")
        press(words(R.string.common_save))
        settle()
        assertEquals(0, payees("Spotify"))
        assertEquals(before, payees("Spotify AB"))
        finish("U18 rename")

        pressDescribed(words(R.string.rule_add))
        val match = hasSetTextAction() and hasText(words(R.string.rule_match_field))
        waitFor(match)
        compose.onNode(match).performTextInput("Spotify AB")
        compose.onNode(hasSetTextAction() and hasText(words(R.string.rule_rename_to))).performTextInput("Spotify Premium")
        press(words(R.string.common_save))
        settle()
        assertEquals(1, budget().rows("payeeRules").count { it.text("payee") == "Spotify Premium" })
        assertEquals("A rule changes nothing until it's applied", before, payees("Spotify AB"))
        finish("U18 rule")

        // What rules leave alone, such as a closed row, is the engine's to say.
        val expected = changedBudget(stateless("workflow", storedBudget(), JSONObject().put("action", JSONObject().put("type", "reapplyPayeeRules"))))
        press(words(R.string.common_apply))
        settle()
        assertTrue(payees("Spotify Premium") > 0)
        assertSameJson("U18 reapply", Canon.forStateless(expected), Canon.forStateless(storedBudget()))
        finish("U18 reapply")
    }

    @Test fun u19RecordingALoanPayment() {
        val before = budget().rows("transactions").map { it.text("id") }.toSet()
        val terms = budget().rows("loans").first { it.text("accountId") == "car-loan" }
        tab(2)
        waitFor(hasScrollToKeyAction())
        compose.onAllNodes(hasScrollToKeyAction()).onFirst().performScrollToKey("account:car-loan")
        waitForText("Car loan")
        compose.onAllNodesWithText("Car loan").onFirst().performTouchInput { longClick() }
        press(words(R.string.account_loan_details))
        press(words(R.string.loan_record_payment))
        val amount = terms.getLong("monthlyPayment")
        press(words(R.string.loan_pay_amount, money(amount, budget().text("currency"))))
        settle()
        val added = budget().rows("transactions").filter { it.text("id") !in before }
        assertTrue("A payment and the month's interest: $added", added.isNotEmpty())
        val payment = added.first { it.text("transferAccountId") == "car-loan" }
        assertEquals(-amount, payment.getLong("amount"))
        finish("U19")
    }

    @Test fun u20ABankImport() {
        val text = "Date,Payee,Memo,Amount\n$today,Cafe U20,,-12.34\n$today,Refund U20,,56.78\n"
        var draft by mutableStateOf(BankImportDraft("checking", "statement.csv", text))
        var open by mutableStateOf(true)
        scenario.onActivity { activity ->
            activity.setContent {
                val state by vm.state.collectAsState()
                MaterialTheme { state.budget?.takeIf { open }?.let { BankImportDialog(it, vm, draft, { change -> draft = change(draft) }) { open = false } } }
            }
        }
        val shown = vm.state.value
        val ready = hasText(words(R.string.bank_import_count, "2")) and hasClickAction() and isEnabled()
        val onwards = hasText(words(R.string.common_continue)) and hasClickAction() and isEnabled()
        waitFor(ready or onwards)
        if (compose.onAllNodes(ready).fetchSemanticsNodes().isEmpty()) compose.onAllNodes(onwards).onLast().performClick()
        waitFor(ready)
        assertSame("Reviewing saves nothing", shown.budget, vm.state.value.budget)
        assertEquals(shown.budgetRevision, vm.state.value.budgetRevision)
        driver.verify("U20 review")
        compose.onAllNodes(ready).onLast().performClick()
        settle()
        compose.waitUntil(10_000) { !open }
        val added = budget().rows("transactions").filter { it.text("payee") in setOf("Cafe U20", "Refund U20") }.associateBy { it.text("payee") }
        assertEquals(-1_234L, added.getValue("Cafe U20").getLong("amount"))
        assertEquals(5_678L, added.getValue("Refund U20").getLong("amount"))
        assertEquals("checking", added.getValue("Cafe U20").text("accountId"))
        assertTrue("One undo for the two", vm.state.value.undoAvailable)
        finish("U20")
    }

    @Test fun u21UndoFromTheSnackbarAndTheMenu() {
        val start = Canon.digest(storedBudget())
        quickAssign()
        waitFor(applyButton())
        compose.onNode(applyButton()).performClick()
        settle()
        val after = Canon.digest(storedBudget())
        assertNotEquals(start, after)
        press(words(R.string.common_undo))
        settle()
        assertEquals("Undone from the snackbar", start, Canon.digest(storedBudget()))
        finish("U21 snackbar", Change.History)

        fun states(undo: Boolean, redo: Boolean) {
            pressDescribed(words(R.string.common_more_actions))
            waitForText(words(R.string.common_redo))
            val undoItem = compose.onNode(hasText(words(R.string.common_undo)))
            val redoItem = compose.onNode(hasText(words(R.string.common_redo)))
            if (undo) undoItem.assertIsEnabled() else undoItem.assertIsNotEnabled()
            if (redo) redoItem.assertIsEnabled() else redoItem.assertIsNotEnabled()
        }
        states(undo = false, redo = true)
        press(words(R.string.common_redo))
        settle()
        assertEquals(after, Canon.digest(storedBudget()))
        states(undo = true, redo = false)
        press(words(R.string.common_undo))
        settle()
        assertEquals(start, Canon.digest(storedBudget()))
        states(undo = false, redo = true)
        press(words(R.string.common_redo))
        settle()
        assertEquals(after, Canon.digest(storedBudget()))
        finish("U21 menu", Change.History)
    }

    @Test fun u22SwitchingBudgetAndBack() {
        val other = "Other budget ${System.nanoTime()}"
        session.make(SyntheticBudget.build(transactions = 300, seed = 62).put("name", other), open = false)
        val first = vm.state.value.budgetIdentity
        tab(4)
        press(name)
        waitForText(other)
        press(other)
        settle()
        compose.waitUntil(20_000) { vm.state.value.budget?.text("name") == other }
        assertNotEquals(first, vm.state.value.budgetIdentity)
        finish("U22 there", Change.Replaced)
        val id = rowsToTap(1).single()
        vm.runCommand("workflow", Edits.bulk(listOf(id), "memo", "On the other budget"))
        finish("U22 an edit there")
        tab(4)
        press(other)
        waitForText(name)
        press(name)
        settle()
        compose.waitUntil(20_000) { vm.state.value.budget?.text("name") == name }
        assertEquals(first, vm.state.value.budgetIdentity)
        assertNotEquals("On the other budget", row(id)?.text("memo"))
        finish("U22 back", Change.Replaced)
    }

    @Test fun u23SearchAndReportsSeeANewRow() {
        tab(1)
        vm.saveTransaction(Edits.transaction("t-found", today, amount = -98_765, category = "c-bills-1", payee = "Zebra Crossing Cafe"))
        settle()
        pressDescribed(words(R.string.common_search))
        waitFor(hasSetTextAction())
        compose.onAllNodes(hasSetTextAction()).onFirst().performTextInput("Zebra")
        waitForTag("tx:t-found")
        val params = JSONObject().put("from", month.toString()).put("to", month.toString())
        var report: JSONObject? = null
        vm.query("reports", params) { report = it }
        compose.waitUntil(20_000) { report != null }
        assertSameJson("U23 report", stateless("reports", storedBudget(), params), report)
        tab(3)
        settle()
        finish("U23")
    }

    @Test fun u24ARelaunchWithAnEditorOpenRecoversTheDraft() {
        tab(1)
        val id = rowsToTap(1).single()
        reach(id)
        compose.onNodeWithTag("tx:$id").performClick()
        waitForTag("tx-memo")
        compose.onNodeWithTag("tx-memo").performTextReplacement("Typed before the app went away")
        compose.waitForIdle()
        fun drafts() = NativeServices.call("editor_recovery") as JSONArray
        compose.waitUntil(10_000) { drafts().toString().contains("Typed before the app went away") }
        assertNotEquals("Nothing's saved yet", "Typed before the app went away", storedRow(id)!!.text("memo"))
        driver.close()
        scenario.close()
        launch()
        assertTrue("The draft came through", drafts().toString().contains("Typed before the app went away"))
        tab(1)
        finish("U24", Change.Nothing)
    }

    @Test fun u25AnExternalWriteWithAnEditorOpenIsRefused() {
        tab(2)
        waitForText("Savings")
        compose.onAllNodesWithText("Savings").onFirst().performTouchInput { longClick() }
        press(words(R.string.reconcile_action))
        press(words(R.string.common_no))
        val row = rowsToTap(1).single()
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == row }.put("memo", "From elsewhere") }
        vm.refresh()
        settle()
        val stored = Canon.digest(storedBudget())
        press(words(R.string.common_review))
        waitForText(words(R.string.error_editor_budget_changed))
        assertEquals("Nothing saved", stored, Canon.digest(storedBudget()))
        press(words(R.string.common_cancel))
        finish("U25", Change.Replaced)
    }

    @Test fun u26AnExternalWriteThenATapKeepsBoth() {
        tab(1)
        val (tapped, other) = rowsToTap(2)
        reach(tapped)
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == other }.put("memo", "From elsewhere") }
        compose.onNodeWithTag("tx-cleared:$tapped").performClick()
        settle()
        assertEquals("uncleared", storedRow(tapped)!!.text("status"))
        assertEquals("From elsewhere", storedRow(other)!!.text("memo"))
        assertEquals("From elsewhere", row(other)!!.text("memo"))
        finish("U26 both kept", Change.Rebuilt)

        // The same row: the tap was made on what's no longer there.
        saveFromElsewhere { it.rows("transactions").first { t -> t.text("id") == tapped }.put("memo", "Changed under the tap") }
        compose.onNodeWithTag("tx-cleared:$tapped").performClick()
        settle()
        assertEquals(words(R.string.guard_values_changed), vm.state.value.error)
        waitForText(words(R.string.guard_values_changed))
        assertEquals("uncleared", storedRow(tapped)!!.text("status"))
        press(words(R.string.common_ok))
        assertEquals("Changed under the tap", row(tapped)!!.text("memo"))
        // The refusal is a recorded failure, rightly. From here, nothing else has moved.
        mark = driver.mark()
        finish("U26 stale", Change.Nothing)
    }

    @Test fun u27AnExportHoldsWhatsStored() {
        tab(1)
        val id = rowsToTap(1).single()
        reach(id)
        compose.onNodeWithTag("tx-cleared:$id").performClick()
        settle()
        var export: String? = null
        vm.exportBudget { export = it }
        settle()
        compose.waitUntil(10_000) { export != null }
        assertSameJson("U27", Canon.storeOrder(storedBudget()), Canon.storeOrder(JSONObject(export!!)))
        assertTrue("It has what the app's copy leaves out", JSONObject(export!!).has("sourceExport"))
        assertFalse(budget().has("sourceExport"))
        finish("U27")
    }

    @Test fun u28AutomaticSchedulesRacingATap() {
        driver.close()
        scenario.close()
        val due = storedBudget().rows("schedules").first { !it.optBoolean("paused") && it.text("nextDate").isNotBlank() && it.text("nextDate") <= today }
        preferences.automaticSchedules = true
        launch()
        tab(1)
        val id = rowsToTap(1).single()
        reach(id)
        compose.onNodeWithTag("tx-cleared:$id").performClick()
        settle()
        // Whichever got there first, neither is lost.
        compose.waitUntil(20_000) { storedBudget().rows("schedules").first { it.text("id") == due.text("id") }.text("nextDate") != due.text("nextDate") }
        vm.refresh()
        settle()
        assertEquals("uncleared", storedRow(id)!!.text("status"))
        assertEquals("uncleared", row(id)!!.text("status"))
        assertNotEquals(due.text("nextDate"), budget().row("schedules", due.text("id"))!!.text("nextDate"))
        finish("U28", Change.Replaced)
    }

    /** Answers for the system's file picker, which can't be driven, with a file of the test's own. */
    private fun picker(file: File) = instrumentation.addMonitor(IntentFilter(Intent.ACTION_CREATE_DOCUMENT, "application/json"),
        Instrumentation.ActivityResult(Activity.RESULT_OK, Intent().setData(Uri.fromFile(file))), true)

    /** Settings, with the export row on screen. */
    private fun exportRow(): String {
        tab(4)
        press(words(R.string.page_settings))
        compose.waitForIdle()
        waitFor(hasScrollToKeyAction())
        val row = words(R.string.settings_export_backup)
        compose.onAllNodes(hasScrollToKeyAction()).onLast().performScrollToNode(hasText(row))
        return row
    }

    @Test fun u29AnExportOutlivesTheScreenThatAskedForIt() {
        val file = File(context.cacheDir, "export-${System.nanoTime()}.json").apply { writeText("") }
        val monitor = picker(file)
        val release = CountDownLatch(1)
        try {
            val row = exportRow()
            // Anything queued ahead of the export keeps it waiting while the phone rotates.
            val running = CountDownLatch(1)
            vm.editBudget { running.countDown(); release.await() }
            assertTrue(running.await(10, TimeUnit.SECONDS))
            press(row)
            compose.waitUntil(10_000) { monitor.hits > 0 }
            instrumentation.waitForIdleSync()
            scenario.recreate()
            release.countDown()
            settle()
            compose.waitUntil(20_000) { file.length() > 0 }
            assertSameJson("U29", Canon.storeOrder(storedBudget()), Canon.storeOrder(JSONObject(file.readText())))
            waitForText(words(R.string.backup_exported))
        } finally {
            release.countDown()
            instrumentation.removeMonitor(monitor)
            file.delete()
        }
        finish("U29", Change.Nothing)
    }

    @Test fun u30AnExportOfABudgetSyncHasClosedSaysSo() {
        val file = File(context.cacheDir, "export-${System.nanoTime()}.json").apply { writeText("") }
        val monitor = picker(file)
        try {
            val row = exportRow()
            // What sync's removal does: the store closes, and the app hears a moment later.
            NativeServices.call("library_trash", JSONObject().put("id", (NativeServices.call("library_snapshot") as JSONObject).getString("activeId")))
            press(row)
            val failed = words(R.string.backup_save_failed)
            waitForText(failed)
            assertEquals("Nothing to write", 0L, file.length())
            // That moment later: a publish clears errors, and this one has to outlast it.
            vm.refresh()
            settle()
            assertNull(vm.state.value.budget)
            waitForText(failed)
            press(words(R.string.common_ok))
            compose.waitUntil(10_000) { compose.onAllNodesWithText(failed).fetchSemanticsNodes().isEmpty() }
        } finally {
            instrumentation.removeMonitor(monitor)
            file.delete()
        }
    }

    @Test fun u31ABudgetInYen() {
        driver.close()
        scenario.close()
        session.make(SyntheticBudget.build(transactions = 400, seed = 63, currency = "JPY").put("name", "Yen ${System.nanoTime()}"))
        launch()
        tab(1)
        val before = budget().rows("transactions").map { it.text("id") }.toSet()
        newTransaction("1234", "Keypad yen")
        compose.onNodeWithTag("tx-save").performClick()
        settle()
        assertEquals(-1234L, budget().rows("transactions").single { it.text("id") !in before }.getLong("amount"))
        finish("U31 added")

        tab(0)
        waitForTag("assign:c-bills-0")
        compose.onNodeWithTag("assign:c-bills-0").performClick()
        waitForText(words(R.string.assign_action))
        keys("5000")
        press(words(R.string.assign_action))
        settle()
        assertEquals(5_000L, storedBudget().rows("assignments").first { it.text("month") == month.toString() && it.text("categoryId") == "c-bills-0" }.getLong("amount"))
        finish("U31 assigned")

        menu(words(R.string.common_undo))
        settle()
        finish("U31 undone", Change.History)
    }

    /**
     * Workers ask for the view model too, and a worker asking while the first frame does makes a second
     * one. U28 failed on the screen left holding the one the store cleared. The race can't be timed from
     * here, so this pins what ends it.
     */
    @Test fun u32TheViewModelExistsBeforeAnyWorkerCanAskForIt() {
        driver.close()
        scenario.close()
        var made: Set<String>? = null
        val created = ActivityLifecycleCallback { activity, stage ->
            if (stage == Stage.CREATED && activity is MainActivity) made = activity.viewModelStore.keys().toSet()
        }
        val monitor = ActivityLifecycleMonitorRegistry.getInstance()
        monitor.addLifecycleCallback(created)
        try { launch() } finally { monitor.removeLifecycleCallback(created) }
        assertTrue("In the store once onCreate ended: $made", made.orEmpty().any { it.endsWith(BudgetViewModel::class.java.name) })
    }

    /**
     * The picker can hand back a file that's already there, and a `file:` Uri opens it as Android's own
     * providers do. The app used to write each "/" as "\/", so this budget's old export is longer than
     * its new one with nothing changed. What's left after the JSON makes desktop and iPhone refuse the file.
     */
    @Test fun u33AnExportOverALongerFileLeavesNothingOfIt() {
        driver.close()
        scenario.close()
        session.make(SyntheticBudget.build(transactions = 400, seed = 64).put("name", "Cash/ATM ${System.nanoTime()}"))
        launch()
        val exported = vm.exportBudget()!!
        val older = JSONObject(exported).toString()
        assertTrue("The old export is longer", older.length > exported.length)
        val file = File(context.cacheDir, "export-${System.nanoTime()}.json").apply { writeText(older) }
        val monitor = picker(file)
        try {
            press(exportRow())
            waitForText(words(R.string.backup_exported))
            assertEquals(exported, file.readText())
        } finally {
            instrumentation.removeMonitor(monitor)
            file.delete()
        }
        finish("U33", Change.Nothing)
    }
}
