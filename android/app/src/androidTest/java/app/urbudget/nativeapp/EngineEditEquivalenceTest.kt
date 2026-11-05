package app.urbudget.nativeapp

import android.util.Log
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

/**
 * Every command and `edit { }` through `edit_budget`, on a budget of a real size, against the same
 * change on the whole budget, and every kind of preview through `query_budget`.
 */
@RunWith(AndroidJUnit4::class)
class EngineEditEquivalenceTest {
    private lateinit var session: EditSession
    private var currency = "EUR"
    private val today = LocalDate.now()
    private val month = YearMonth.now()

    @Before fun setup() { requireEmulator(); session = EditSession() }
    @After fun clean() { if (::session.isInitialized) session.close() }

    private fun bulk(ids: List<String>, operation: String, value: Any?) = JSONObject().put("type", "bulk").put("ids", JSONArray(ids))
        .put("operation", operation).put("value", value ?: JSONObject.NULL).put("accountId", "all")
    private fun assign(month: YearMonth, category: String, amount: Long) = JSONObject().put("type", "assignAmount").put("month", month.toString()).put("categoryId", category).put("amount", amount)
    private fun auto(mode: String) = JSONObject().put("type", "autoAssign").put("month", month.toString()).put("mode", mode)
        .put("categories", JSONArray(listOf("c-bills-0", "c-home-0", "c-health-3"))).put("limitToAvailable", false)

    private fun transaction(id: String, payee: String = "Added", amount: Long = -1_250, date: LocalDate = today, category: String? = "c-fun-1") = JSONObject()
        .put("id", id).put("date", date.toString()).put("accountId", "checking").put("payee", payee).put("memo", "").put("amount", amount).put("status", "uncleared")
        .put("splits", JSONArray().put(JSONObject().put("categoryId", category ?: JSONObject.NULL).put("amount", amount)))
        .put("transferAccountId", JSONObject.NULL).put("transferStatus", JSONObject.NULL)

    private fun EditDriver.saved(vm: BudgetViewModel, name: String, record: JSONObject) {
        val whole = storedBudget()
        val params = JSONObject().put("transaction", Canon.restored(record, whole)).put("through", today.toString())
        val expected = changedBudget(stateless("saveTransaction", whole, params))
        step(name, expected) { vm.saveTransaction(record) }
    }

    private fun EditDriver.repeating(vm: BudgetViewModel, name: String, record: JSONObject) {
        val date = record.getString("date")
        val id = record.getString("id")
        val schedule = JSONObject().put("id", id).put("transaction", JSONObject(record.toString())).put("nextDate", date).put("anchorDate", date).put("frequency", "monthly").put("paused", false)
        val due = date <= today.toString()
        val copy = storedBudget()
        copy.upsertRecord("schedules", JSONObject(schedule.toString()))
        val expected = if (!due) copy else {
            val entered = changedBudget(stateless("saveTransaction", copy, JSONObject().put("transaction", JSONObject(record.toString())).put("through", today.toString())))
            changedBudget(stateless("applySchedules", entered, JSONObject().put("through", date).put("selected", id).put("skip", true)))
        }
        step(name, expected) { vm.saveRepeating(schedule, record.takeIf { due }) }
    }

    /** `run` does through the view model what `transform` does to a whole budget. */
    private fun EditDriver.block(vm: BudgetViewModel, name: String, transform: (JSONObject) -> Unit, changes: Boolean = true, run: () -> Unit) {
        val expected = storedBudget().also(transform)
        step(name, expected, changes, run = run)
    }

    /** A row of `collection` through `vm.upsert`, against the same merge on the whole budget. */
    private fun EditDriver.upsert(vm: BudgetViewModel, name: String, collection: String, record: JSONObject) =
        block(vm, name, { it.upsertRecord(collection, JSONObject(record.toString())) }) { vm.upsert(collection, record) }

    private fun removeRow(budget: JSONObject, collection: String, identity: String, id: String) {
        budget.put(collection, JSONArray(budget.rows(collection).filter { it.text(identity) != id }))
    }

    private fun sms(id: String, amount: String, decision: JSONObject) = JSONObject("""{"messages":[{"id":"$id","sender":"Bank","receivedDate":"$today","receivedTime":"","body":"Purchase $currency $amount at Bakery card *1234"}],
        "analysis":{"items":[{"sourceIds":["$id"],"disposition":"transaction","skip":null,"note":"Purchase","evidence":[{"sourceId":"$id","quote":"$currency $amount"}],"unsure":[],
        "transaction":{"date":"$today","amount":"-$amount","currency":"$currency","writtenAmount":"$amount","payee":"Bakery","memo":"","kind":"expense","accountRef":"*1234","otherRef":null,"categoryId":null,"accountId":"checking","transferAccountId":null}}]}}""")
        .put("decisions", JSONArray().put(decision))

    /** Checks a parsed export against the store's after a group of rows. */
    private fun exportCheck(vm: BudgetViewModel, name: String) {
        val export = JSONObject(vm.exportBudget()!!)
        assertSameJson("$name: export", JSONObject(NativeServices.call("export_budget") as String), export)
    }

    /** Every `edit { }`: each goes as a patch, and nothing but what it touched changes. */
    private fun editBlocks(d: EditDriver, vm: BudgetViewModel) {
        fun budget() = vm.state.value.budget!!
        d.upsert(vm, "a transaction", "transactions", JSONObject().put("id", budget().rows("transactions").first(Edits::plain).text("id")).put("memo", "Through a block"))
        d.upsert(vm, "a category", "categories", JSONObject().put("id", "c-home-2").put("name", "Renamed home").put("group", "Home"))
        d.upsert(vm, "an account by its id alone", "accounts", JSONObject().put("id", "savings").put("note", "A note"))
        d.upsert(vm, "a new schedule", "schedules", JSONObject().put("id", "s-block").put("transaction", Edits.transaction("template-block", today.plusDays(9).toString(), category = "c-fun-1")).put("nextDate", today.plusDays(9).toString())
            .put("anchorDate", today.plusDays(9).toString()).put("frequency", "monthly").put("paused", false))
        d.upsert(vm, "a schedule paused by its id alone", "schedules", JSONObject().put("id", "s-block").put("paused", true))
        d.upsert(vm, "a payee rule", "payeeRules", JSONObject().put("id", "rule-block").put("match", "Bakery").put("payee", "The bakery"))
        d.upsert(vm, "a loan", "loans", JSONObject(budget().rows("loans").first { it.text("accountId") == "car-loan" }.toString()).put("monthlyFees", 700))
        assertEquals("keep", storedBudget().rows("loans").first { it.text("accountId") == "car-loan" }.text("futureLoanField"))
        d.block(vm, "many categories", { b -> listOf("c-home-3", "c-home-4").forEach { b.upsertRecord("categories", JSONObject().put("id", it).put("hidden", true)) } }) {
            vm.upsertMany("categories", budget().rows("categories").filter { it.text("id") in listOf("c-home-3", "c-home-4") }.map { JSONObject(it.toString()).put("hidden", true) })
        }
        d.block(vm, "a schedule removed", { removeRow(it, "schedules", "id", "s-block") }) { vm.remove("schedules", "s-block") }
        d.block(vm, "a payee rule removed", { removeRow(it, "payeeRules", "id", "rule-block") }) { vm.remove("payeeRules", "rule-block") }
        d.block(vm, "a loan removed", { removeRow(it, "loans", "accountId", "car-loan") }) { vm.remove("loans", "car-loan") }
        val row = budget().rows("transactions").first { Edits.plain(it) && it.has("hasImportSource") }.text("id")
        d.block(vm, "a transaction deleted", { removeRow(it, "transactions", "id", row) }) { vm.deleteTransaction(row) }
        exportCheck(vm, "records")

        d.step("a credit account", null) { vm.saveAccount("card-block", "New card", "credit", -5_000, "creditCard") }
        assertEquals("Its payment category came with it", 1, budget().rows("categories").count { it.text("paymentAccountId") == "card-block" })
        d.step("the same credit account, renamed", null) { vm.saveAccount("card-block", "Renamed card", "credit", -5_000, "creditCard") }
        assertEquals("Only one payment category", 1, budget().rows("categories").count { it.text("paymentAccountId") == "card-block" })
        d.step("accounts set up together", null) {
            vm.saveAccounts(listOf(JSONObject().put("id", "cash-block").put("name", "Wallet").put("kind", "cash").put("openingBalance", 700),
                JSONObject().put("id", "card-texts").put("name", "Card from texts").put("kind", "credit").put("openingBalance", 0)))
        }
        assertEquals(1, budget().rows("categories").count { it.text("paymentAccountId") == "card-texts" })
        d.step("a new category", null) { vm.saveCategory(name = "Added ✨", group = "Fun") }
        assertEquals(1, budget().rows("categories").count { it.text("name") == "Added ✨" })
        d.step("a category saved", null) { vm.saveCategory("c-fun-2", "Renamed fun", "Fun") }
        exportCheck(vm, "accounts")

        val goal = JSONObject().put("goal_type", "TB").put("goal_amount", 60_000).put("goal_target", 600_000)
        d.block(vm, "a target set", { b -> b.upsertRecord("categories", JSONObject().put("id", "c-fun-3").put("goal", JSONObject(goal.toString())).put("target", 60_000)) }) { vm.saveTarget("c-fun-3", goal) }
        d.block(vm, "a target cleared", { b ->
            val record = JSONObject(b.rows("categories").first { it.text("id") == "c-fun-3" }.toString()).put("target", 0)
            record.remove("goal")
            b.put("categories", JSONArray(b.rows("categories").map { if (it.text("id") == "c-fun-3") record else it }))
        }) { vm.saveTarget("c-fun-3", null) }
        assertFalse(budget().rows("categories").first { it.text("id") == "c-fun-3" }.has("goal"))

        val move: (JSONObject) -> Unit = { it.put("accounts", AccountTypes.moveAccount(it.getJSONArray("accounts"), "checking", "savings", true)) }
        d.block(vm, "an account moved", move) { vm.editBudget(move) }
        val settings: (JSONObject) -> Unit = { it.put("name", "Renamed budget").put("numberFormat", JSONObject().put("decimal", ",").put("group", ".").put("symbol", "after")).put("dateFormat", "dmy") }
        d.block(vm, "budget settings", settings) { vm.editBudget(settings) }
        val flags: (JSONObject) -> Unit = { it.put("flagNames", JSONObject().put("red", "Tax").put("blue", "Work")) }
        d.block(vm, "flag names", flags) { vm.editBudget(flags) }
        val noFlags: (JSONObject) -> Unit = { it.remove("flagNames") }
        d.block(vm, "no flag names", noFlags) { vm.editBudget(noFlags) }
        val group: (JSONObject) -> Unit = { b ->
            val order = b.getJSONArray("groupOrder")
            b.put("groupOrder", JSONArray((0 until order.length()).map { order.optString(it) } + "A new group"))
        }
        d.block(vm, "a new group", group) { vm.editBudget(group) }
        val noGroup: (JSONObject) -> Unit = { b ->
            val order = b.getJSONArray("groupOrder")
            b.put("groupOrder", JSONArray((0 until order.length()).map { order.optString(it) }.filter { it != "A new group" }))
        }
        d.block(vm, "that group gone", noGroup) { vm.editBudget(noGroup) }
        exportCheck(vm, "settings")

        d.step("a block that changes nothing", null, changes = false) { vm.editBudget { } }
        // It goes to the store, which finds nothing to do, and so the root stays.
        val schedule = JSONObject(budget().rows("schedules").first { it.text("frequency") == "monthly" }.toString())
        d.step("a schedule saved as it was", null, changes = false) { vm.upsert("schedules", schedule) }
    }

    /** Previews leave everything as it was, and answer as the same call on the whole budget does. */
    private fun previews(vm: BudgetViewModel) {
        val state = vm.state.value
        val budget = state.budget!!
        val whole = storedBudget()
        val text = budget.toString()
        val token = (NativeServices.call("native_budget_status") as JSONObject).getString("token")
        val draft = Canon.text(NativeServices.call("snapshot_budget"))
        fun unchanged(name: String) {
            assertSame(name, budget, vm.state.value.budget)
            assertSame(name, state.summary, vm.state.value.summary)
            assertEquals(name, text, budget.toString())
            assertEquals(name, token, (NativeServices.call("native_budget_status") as JSONObject).getString("token"))
            assertEquals(name, draft, Canon.text(NativeServices.call("snapshot_budget")))
            assertNull(name, vm.state.value.error)
        }
        val cash = state.summary!!.rows("accounts").first { it.text("id") == "checking" }.getLong("cleared")
        val reconcile = JSONObject().put("accountId", "checking").put("balance", cash + 777).put("date", today.toString()).put("adjustmentId", "adjustment-preview")
        val stateful = changedBudget(stateless("reconcile", whole, reconcile)).rows("transactions").first { it.text("id") == "adjustment-preview" }
        val adjustment = vm.asked("reconcile", reconcile).rows("transactions").first { it.text("id") == "adjustment-preview" }
        assertEquals(777, adjustment.getLong("amount"))
        assertSameJson("reconcile", Canon.forStateless(JSONObject().put("transactions", JSONArray().put(stateful))), Canon.forStateless(JSONObject().put("transactions", JSONArray().put(adjustment))))
        unchanged("reconcile")

        for (mode in listOf("underfunded", "assignedLastMonth", "averageSpent")) {
            val params = JSONObject().put("action", auto(mode)).put("previewMonth", month.toString())
            val expected = stateless("workflow", whole, params).getJSONObject("summary")
            assertSameJson(mode, expected, vm.asked("workflow", params).getJSONObject("summary"))
            unchanged(mode)
        }
        val filter = JSONObject().put("from", month.minusMonths(3).toString()).put("to", month.toString()).put("accountIds", JSONArray()).put("categoryIds", JSONArray())
        assertSameJson("reports", stateless("reports", whole, filter), vm.asked("reports", filter))
        unchanged("reports")
        val forecast = JSONObject().put("today", today.toString()).put("through", today.plusMonths(3).toString())
        assertSameJson("forecast", stateless("forecast", whole, forecast), vm.asked("forecast", forecast))
        unchanged("forecast")
        assertTrue(vm.asked("exportRegister").getString("items").isNotBlank())
        unchanged("exportRegister")
        // These need no budget at all.
        val loan = budget.rows("loans").firstOrNull()
        if (loan != null) {
            vm.asked("loanProjection", JSONObject().put("balance", 1_000_000).put("terms", loan).put("start", today.toString()))
            unchanged("loanProjection")
        }
    }

    private fun script(d: EditDriver, vm: BudgetViewModel) {
        fun budget() = vm.state.value.budget!!
        val next = month.plusMonths(1)
        val used = HashSet<String>()
        /** A row nothing else in the script has touched, so each one tests what it says. */
        fun pick(test: (JSONObject) -> Boolean) = budget().rows("transactions").first { it.text("id") !in used && test(it) }.also { used += it.text("id") }
        val plain = Edits::plain
        val fromImport = { row: JSONObject -> plain(row) && row.has("hasImportSource") }

        d.workflow("assign new", assign(next, "c-bills-0", 12_345))
        d.workflow("assign existing", assign(month, "c-bills-0", 25_000))
        d.workflow("assign zero", assign(month, "c-bills-1", 0))
        d.workflow("planning", JSONObject().put("type", "setBudgetPlanning").put("month", month.toString()).put("note", "Holiday plan ✈").put("expectedIncome", 500_000))
        d.workflow("organize", JSONObject().put("type", "organizeCategory").put("id", "c-fun-2").put("group", "Everyday").put("beforeId", JSONObject.NULL).put("groupBefore", JSONObject.NULL))
        d.workflow("rename group", JSONObject().put("type", "renameGroup").put("from", "Family").put("to", "Family & friends"))
        d.workflow("delete into a category", JSONObject().put("type", "deleteCategory").put("id", "c-fun-5").put("replacement", "c-fun-1"))
        d.workflow("delete into ready to assign", JSONObject().put("type", "deleteCategory").put("id", "c-fun-4").put("replacement", "__rta"))
        exportCheck(vm, "categories")
        // Whether a mode has anything to do depends on the figures, as it does for a person.
        val modes = listOf("underfunded", "assignedLastMonth", "spentLastMonth", "averageAssigned", "averageSpent", "reduceOverfunding", "resetAvailable", "resetAssigned")
        var moved = 0
        for (mode in modes) {
            val before = storedBudget()
            val expected = changedBudget(stateless("workflow", before, JSONObject().put("action", auto(mode))))
            val changes = firstDifference(Canon.forStateless(before), Canon.forStateless(expected)) != null
            if (changes) moved++
            d.step("auto assign $mode", expected, changes) { vm.runCommand("workflow", JSONObject().put("action", auto(mode))) }
        }
        assertTrue("Most modes should do something here: $moved", moved >= 5)
        d.workflow("move money", JSONObject().put("type", "moveMoney").put("month", month.toString()).put("from", "__rta").put("to", "c-home-0").put("amount", 100))
        exportCheck(vm, "assigning")

        d.workflow("rename payee", JSONObject().put("type", "renamePayee").put("from", "Spotify").put("to", "Spotify AB"))
        d.workflow("save payee", JSONObject().put("type", "savePayee").put("record", JSONObject().put("id", "payee-new").put("name", "Butcher").put("categoryId", "c-home-1").put("futureField", 17)))
        d.workflow("reorder rule", JSONObject().put("type", "reorderPayeeRule").put("id", "rule-2").put("direction", "up"))
        val rule = budget().rows("payeeRules").first { it.text("id") == "rule-0" }
        val unrouted = budget().rows("transactions").first { it.text("payee") != rule.text("match") && plain(it) }
        used += unrouted.text("id")
        d.saved(vm, "a row the rules can reach", JSONObject(unrouted.toString()).put("payee", rule.text("match")))
        val unapplied = storedBudget()
        val reapplied = changedBudget(stateless("workflow", unapplied, JSONObject().put("action", JSONObject().put("type", "reapplyPayeeRules"))))
        val changes = firstDifference(Canon.forStateless(unapplied), Canon.forStateless(reapplied)) != null
        d.step("reapply rules", reapplied, changes) { vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "reapplyPayeeRules"))) }
        d.workflow("reapply rules, nothing left", JSONObject().put("type", "reapplyPayeeRules"), changes = false)
        exportCheck(vm, "payees")

        val status = pick(plain)
        d.workflow("bulk status", bulk(listOf(status.text("id")), "status", "uncleared"))
        val unapproved = pick { it.has("approved") && !it.optBoolean("approved", true) }
        d.workflow("bulk approve", bulk(listOf(unapproved.text("id")), "approve", null))
        d.workflow("bulk flag", bulk(listOf(pick(plain).text("id")), "flag", "red"))
        d.workflow("bulk payee", bulk(listOf(pick(plain).text("id")), "payee", "Renamed row"))
        d.workflow("bulk memo", bulk(listOf(pick(plain).text("id"), pick(fromImport).text("id")), "memo", "Bulk memo مذكرة"))
        d.workflow("bulk category", bulk(listOf(pick(plain).text("id")), "category", "c-home-1"))
        d.workflow("bulk category, unassigned on an imported row", bulk(listOf(pick(fromImport).text("id")), "category", "__rta"))
        d.workflow("bulk duplicate", bulk(listOf(pick(fromImport).text("id")), "duplicate", null))
        d.workflow("bulk delete", bulk(listOf(pick(plain).text("id"), pick(fromImport).text("id")), "delete", null))
        exportCheck(vm, "bulk")

        d.saved(vm, "new transaction", transaction("t-new"))
        d.saved(vm, "edited transaction", JSONObject(pick(plain).toString()).put("memo", "Edited 編集"))
        val imported = JSONObject(pick(fromImport).toString()).put("memo", "Edited with its source")
        val source = storedRow(imported.text("id"))!!.getJSONObject("source").toString()
        d.saved(vm, "imported transaction", imported)
        assertEquals(source, storedRow(imported.text("id"))!!.getJSONObject("source").toString())
        d.saved(vm, "split", transaction("t-split", amount = -700).put("splits", JSONArray()
            .put(JSONObject().put("categoryId", "c-home-1").put("amount", -400)).put(JSONObject().put("categoryId", "c-fun-1").put("amount", -300))))
        d.saved(vm, "split with a transfer", transaction("t-split-transfer", amount = -700).put("splits", JSONArray()
            .put(JSONObject().put("categoryId", "c-home-1").put("amount", -400))
            .put(JSONObject().put("categoryId", JSONObject.NULL).put("amount", -300).put("transferAccountId", "savings").put("transferStatus", "cleared"))))
        d.saved(vm, "transfer", transaction("t-transfer", amount = -2_000).put("splits", JSONArray()).put("transferAccountId", "savings").put("transferStatus", "cleared"))
        d.saved(vm, "reconciled", JSONObject(pick { it.text("status") == "reconciled" && it.rows("splits").size == 1 && it.isNull("transferAccountId") && it.rows("splits")[0].isNull("transferAccountId") }.toString()).put("memo", "A note on a closed row"))
        // Future-dated: a schedule takes its place, and the row itself never appears.
        d.saved(vm, "dated ahead", transaction("t-ahead", date = today.plusDays(12)))
        assertNull(budget().row("transactions", "t-ahead"))
        d.saved(vm, "a payee rule hit", transaction("t-rule", payee = rule.text("match"), category = null))
        d.saved(vm, "loan payment", transaction("t-loan", payee = "Car loan", amount = -45_000, category = "c-debts-0").put("transferAccountId", "car-loan").put("transferStatus", "cleared"))
        exportCheck(vm, "transactions")

        val savings = vm.state.value.summary!!.rows("accounts").first { it.text("id") == "savings" }.getLong("balance")
        d.command("reconcile", "reconcile", JSONObject().put("accountId", "savings").put("balance", savings + 12_345).put("date", today.toString()).put("adjustmentId", "adjustment-1"))
        val schedules = budget().rows("schedules").filter { !it.optBoolean("paused") && it.text("nextDate").isNotBlank() && it.text("nextDate") <= today.toString() }
        assertTrue("The budget needs due schedules: ${schedules.size}", schedules.size >= 3)
        d.command("a selected schedule", "applySchedules", JSONObject().put("through", today.toString()).put("selected", schedules[0].text("id")))
        d.command("a skipped schedule", "applySchedules", JSONObject().put("through", today.toString()).put("selected", schedules[1].text("id")).put("skip", true))
        d.command("all due schedules", "applySchedules", JSONObject().put("through", today.toString()))
        d.command("nothing due", "applySchedules", JSONObject().put("through", today.minusYears(3).toString()), changes = false)
        d.repeating(vm, "repeating and due", transaction("t-repeat-due"))
        d.repeating(vm, "repeating and ahead", transaction("t-repeat-ahead", date = today.plusDays(20)))
        exportCheck(vm, "schedules")

        d.command("loan record", "loanRecord", JSONObject().put("terms", JSONObject().put("accountId", "car-loan").put("categoryId", "c-debts-0").put("annualRateBasisPoints", 690).put("monthlyPayment", 45_000).put("monthlyFees", 500))
            .put("from", "checking").put("amount", 5_000).put("date", today.toString()).put("id", "loan-payment-1"))
        d.command("bank import that remembers", "bankApply", JSONObject().put("accountId", "checking").put("remember", JSONObject().put("dateFormat", "dmy").put("dateConfirmed", true))
            .put("choices", JSONArray().put(JSONObject().put("row", JSONObject().put("id", "bank-1").put("date", today.toString()).put("payee", "Cafe").put("memo", "").put("amount", -700)).put("action", "new").put("matchId", JSONObject.NULL))))
        d.command("sms, a new row", "smsBatchApply", sms("sms:new", "12.34", JSONObject().put("index", 0).put("action", "new").put("matchId", JSONObject.NULL)))
        d.saved(vm, "a row a text will match", transaction("t-sms", payee = "Bakery", amount = -5_678))
        d.command("sms, a match", "smsBatchApply", sms("sms:match", "56.78", JSONObject().put("index", 0).put("action", "match").put("matchId", "t-sms")))
        assertTrue(budget().row("transactions", "t-sms")!!.has("bankImportIds"))
        exportCheck(vm, "imports")
        previews(vm)
        editBlocks(d, vm)
        previews(vm)
    }

    private fun run(strict: Boolean, transactions: Int = sized(3000), currency: String = "EUR", driverCheck: (EditDriver, BudgetViewModel) -> Unit = { _, _ -> }) {
        this.currency = currency
        session.make(SyntheticBudget.build(transactions = transactions, seed = 7, currency = currency).put("name", "Equivalence"))
        val vm = session.model()
        val d = EditDriver(vm, strict)
        try {
            script(d, vm)
            driverCheck(d, vm)
            d.loadsBack("the end")
        } finally { d.close() }
        Log.i("EngineEditEquivalence", "${if (strict) "strict" else "quiet"}: ${d.digests.size} steps, last ${d.digests.last()}")
    }

    @Test fun everyCommandStrict() = run(strict = true)
    @Test fun everyCommandQuiet() = run(strict = false)

    /** Another currency all the way through, undo included. Its own size check only runs at a real size. */
    @Test fun everyCommandInYen() {
        SyntheticBudget.build(transactions = 3_000, seed = 8, currency = "JPY")
        run(strict = false, transactions = sized(500), currency = "JPY") { d, vm ->
            assertEquals("JPY", vm.state.value.budget!!.text("currency"))
            val stored = Canon.digest(d.verify("the end of the script"))
            vm.undo()
            vm.settle()
            assertNull(vm.state.value.error)
            assertNotEquals(stored, Canon.digest(d.verify("an undo")))
            vm.redo()
            vm.settle()
            assertNull(vm.state.value.error)
            assertEquals(stored, Canon.digest(d.verify("a redo")))
        }
    }

    /**
     * Phase 3's size check. The old path's export was the whole budget as org.json writes it, so that's
     * the yardstick, on this budget. It's read from the store, because the app's copy isn't whole. A
     * second run of the script wouldn't do: its times and copied ids come out a few bytes either way.
     */
    @Test fun anExportIsNoLargerThanTheAppsOwnTextAndReimports() = run(strict = false) { d, vm ->
        val export = vm.exportBudget()!!
        val bytes = export.toByteArray().size
        val old = storedBudget().toString().toByteArray().size
        Log.i("EngineEditEquivalence", "Export: $bytes bytes, the whole budget as text $old")
        assertTrue("$bytes bytes, the whole budget as text $old", bytes <= old)
        val whole = d.verify("before the import")
        val stored = Canon.digest(whole)
        // The same budget, so nothing is written, and it still has to read back whole.
        vm.importBudget(export)
        vm.settle()
        assertNull(vm.state.value.error)
        assertEquals(stored, Canon.digest(d.verify("after the import")))
        assertEquals(Canon.digest(Canon.windowOf(whole)), Canon.digest(vm.state.value.budget!!))
    }
}
