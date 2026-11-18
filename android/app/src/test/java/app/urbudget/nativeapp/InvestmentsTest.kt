package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.time.LocalDate

/** What an investment account holds, as the app reads it from the budget (investments.rs writes it). */
class InvestmentsTest {
    @Before fun plainNumbers() { Money.configure(JSONObject().put("currency", "USD")) }

    private fun change(id: String, date: String, amount: Long, kind: String) = JSONObject().put("id", id).put("date", date).put("accountId", "broker")
        .put("payee", if (kind == "start") "Starting value" else "Market change").put("memo", "").put("amount", amount).put("status", "cleared")
        .put("splits", JSONArray().put(JSONObject().put("categoryId", JSONObject.NULL).put("amount", amount).put("unassigned", true)))
        .put("transferAccountId", JSONObject.NULL).put("transferStatus", JSONObject.NULL).put("investment", kind)

    private fun held(accountId: String = "broker", source: String = "U1234567") = JSONObject("""
        {"accountId":"$accountId","source":"ibkr","sourceAccount":"$source","asOf":"2026-11-13","currency":"USD","rate":"1","total":2701250,
         "positions":[{"symbol":"VWRA","name":"VANG FTSE AW USDA","kind":"STK","currency":"USD","quantity":"120.5","price":"141.9087137","value":1710000,"cost":1500000,"gain":210000},
                      {"symbol":"2222","name":"SAUDI ARABIAN OIL CO","kind":"STK","currency":"SAR","quantity":"400","price":"27","value":288000,"cost":320000,"gain":-32000}],
         "cash":[{"currency":"USD","amount":"820","value":82000}],"other":1250,
         "values":[["2026-09-30",2422000],["2026-10-30",2550000],["2026-11-13",2701250]],"syncedAt":"2026-11-14T06:00:00.000Z","futureField":{"kept":true}}""")

    private fun account(id: String, kind: String, type: String? = null, archived: Boolean = false) =
        JSONObject().put("id", id).put("name", id).put("kind", kind).put("openingBalance", 0).put("archived", archived).apply { type?.let { put("accountType", it) } }

    private fun budget() = JSONObject().put("schemaVersion", 2).put("name", "Test").put("currency", "USD")
        .put("accounts", JSONArray().put(account("cash", "cash", "checking")).put(account("broker", "tracking", "investment")).put(account("pension", "tracking", "retirement"))
            .put(account("house", "tracking", "property")).put(account("car loan", "tracking", "autoLoan")).put(account("old", "tracking", "investment", archived = true)))
        .put("categories", JSONArray()).put("assignments", JSONArray())
        .put("transactions", JSONArray()
            .put(change("investment-start:broker", "2026-09-30", 2422000, "start"))
            .put(change("investment-change:broker:2026-10", "2026-10-30", 128000, "change"))
            .put(change("investment-change:broker:2026-11", "2026-11-13", 151250, "change"))
            .put(JSONObject(change("deposit", "2026-10-05", 100000, "change").toString()).apply { remove("investment") }))
        .put(Investments.KEY, JSONArray().put(held()))

    @Test fun anAccountsHoldingsGainAndStartAreReadFromTheBudget() {
        val b = budget()
        assertEquals("U1234567", Investments.of(b, "broker")?.optString("sourceAccount"))
        assertNull(Investments.of(b, "pension"))
        // The starting value and a deposit aren't gains; the two months' changes are.
        assertEquals(279250L, Investments.gain(b, "broker"))
        assertNull("No market change yet", Investments.gain(b, "pension"))
        assertEquals("2026-09-30", Investments.since(held()))
        assertNull(Investments.since(JSONObject().put("values", JSONArray())))
    }

    @Test fun pricesAreStaleOnceTheyArePastAWeekend() {
        assertFalse(Investments.stale("2026-11-13", LocalDate.parse("2026-11-16")))
        assertFalse(Investments.stale("2026-11-13", LocalDate.parse("2026-11-17")))
        assertTrue(Investments.stale("2026-11-13", LocalDate.parse("2026-11-18")))
        assertFalse("A date that doesn't read isn't called old", Investments.stale("", LocalDate.parse("2026-11-18")))
    }

    @Test fun quantitiesAndPricesKeepTheBrokersDigitsTrimmedForReading() {
        assertEquals("141.9087", Investments.plain("141.9087137"))
        assertEquals("120.5", Investments.plain("120.5"))
        assertEquals("1,234,567", Investments.plain("1234567"))
        assertEquals("820", Investments.plain("820.00", 2))
        assertEquals("-3.5", Investments.plain("-3.5"))
        assertEquals("0", Investments.plain("-0.00001"))
        assertEquals("n/a", Investments.plain("n/a"))
        assertEquals("VWRA · 120.5 × 141.9087 USD", HoldingsPage.line(held().getJSONArray("positions").getJSONObject(0)))
        // The budget's own separators, as its amounts use.
        Money.configure(JSONObject().put("currency", "EUR").put("numberFormat", JSONObject().put("decimal", ",").put("group", ".")))
        assertEquals("1.234,5", Investments.plain("1234.5"))
    }

    @Test fun onlyOpenTrackingAssetsWithoutABrokerCanTakeOne() {
        // Not the cash account, the one already filled, a loan, or a closed account.
        assertEquals(listOf("pension", "house"), Investments.targets(budget()).map { it.optString("id") })
    }

    @Test fun theDailySyncRunsOnceADayForABudgetInteractiveBrokersFills() {
        assertTrue(Investments.filled(budget()))
        assertFalse(Investments.filled(budget().put(Investments.KEY, JSONArray())))
        assertFalse(Investments.filled(null))
        // Not without such an account, not when turned off, and not twice in a day, even after a failure.
        assertTrue(Investments.dailyDue(filled = true, on = true, triedOn = "2026-11-16", today = "2026-11-17"))
        assertTrue(Investments.dailyDue(filled = true, on = true, triedOn = "", today = "2026-11-17"))
        assertFalse(Investments.dailyDue(filled = true, on = true, triedOn = "2026-11-17", today = "2026-11-17"))
        assertFalse(Investments.dailyDue(filled = true, on = false, triedOn = "", today = "2026-11-17"))
        assertFalse(Investments.dailyDue(filled = false, on = true, triedOn = "", today = "2026-11-17"))
        // What a sync is measured against: the newest prices the budget has.
        assertEquals("2026-11-13", Investments.newestPrices(budget()))
        assertEquals("", Investments.newestPrices(JSONObject()))
    }

    @Test fun onlyAnOpenInvestmentAccountNoBrokerFillsIsAskedAbout() {
        val b = budget().apply { getJSONArray("accounts").put(account("brokerage", "tracking", "investment")) }
        fun asked(id: String) = Investments.promptable(b.getJSONArray("accounts").let { list -> (0 until list.length()).map { list.getJSONObject(it) }.first { it.optString("id") == id } }, b)
        assertTrue(asked("brokerage"))
        // Not the one already filled, a closed one, a pension, a house or a cash account.
        assertEquals(listOf(false, false, false, false, false), listOf("broker", "old", "pension", "house", "cash").map(::asked))
    }

    @Test fun aLoginIsCalledByTheAccountsItBrings() {
        val accounts = listOf(JSONObject().put("id", "U1234567").put("alias", "Long term"), JSONObject().put("id", "U7654321").put("alias", ""))
        val login = JSONObject().put("query", "987654").put("accounts", JSONArray().put("U1234567").put("U7654321").put("U9999999"))
        // An alias from the statement at hand, else the account's id.
        assertEquals("Long term, U7654321, U9999999", Investments.loginName(login, accounts).first)
        // One that has named none yet is called by its query.
        assertNull(Investments.loginName(JSONObject().put("query", "111222").put("accounts", JSONArray()), accounts).first)
        assertTrue(Investments.FLEX_REQUEST.length <= 200)
    }

    private fun found(id: String, linked: String? = null, rate: Boolean = false, rows: JSONArray = JSONArray()) = JSONObject().put("id", id)
        .put("accountId", linked ?: JSONObject.NULL).put("needsRate", if (rate) JSONArray().put("USD").put("JPY") else JSONObject.NULL).put("rows", rows)

    @Test fun newAccountsGoWhereTheyAreToldAndAskForARateOnlyWhenNeeded() {
        val accounts = listOf(found("U1"), found("U2"), found("U3", rate = true), found("U4", linked = "pension", rate = true), found("U5", linked = "house"))
        val into = mapOf("U2" to "pension2", "U3" to "")
        var made = 0
        val links = Investments.links(accounts, into, mapOf("U4" to " 150,5 ", "U1" to "9")) { "new-${++made}" }
        // U1 becomes a new account, U2 fills a chosen one, U3 is left out, U4 (linked) gets its rate, U5 needs nothing.
        assertEquals(3, links.length())
        assertTrue(sameJson(JSONObject().put("source", "U1").put("account", "new-1").put("create", true), links.getJSONObject(0)))
        assertTrue(sameJson(JSONObject().put("source", "U2").put("account", "pension2").put("create", false), links.getJSONObject(1)))
        assertTrue(sameJson(JSONObject().put("source", "U4").put("account", "pension").put("rate", "150.5"), links.getJSONObject(2)))
        assertEquals(listOf("U1", "U2"), Investments.adding(accounts, into).map { it.optString("id") })
        assertTrue(Investments.ready(accounts, into, emptyMap()))
        // Chosen and needing a rate: nothing is added until it has one above zero.
        val chosen = into + ("U3" to Investments.NEW)
        assertFalse(Investments.ready(accounts, chosen, emptyMap()))
        assertFalse(Investments.ready(accounts, chosen, mapOf("U3" to "0")))
        assertFalse(Investments.ready(accounts, chosen, mapOf("U3" to "abc")))
        assertTrue(Investments.ready(accounts, chosen, mapOf("U3" to "3.75")))
        assertEquals("3.75", Investments.links(accounts, chosen, mapOf("U3" to "3.75")) { "n" }.getJSONObject(2).optString("rate"))
        assertEquals("0.27", Investments.rate("0,27"))
        assertNull(Investments.rate("-1"))
        assertNull(Investments.rate(null))
    }

    @Test fun theAccountsADepositCouldComeFromStartWithTheOnesItMovedMoneyWith() {
        val budget = JSONObject("""{"accounts":[{"id":"cash","name":"Cash"},{"id":"broker","name":"Broker"},{"id":"old","name":"Old","archived":true},
            {"id":"bank","name":"Bank"},{"id":"card","name":"Card"}],
            "transactions":[{"id":"1","accountId":"bank","transferAccountId":"broker"},{"id":"2","accountId":"broker","transferAccountId":"bank"},
            {"id":"3","accountId":"broker","transferAccountId":"card"},{"id":"4","accountId":"cash","transferAccountId":"card"},
            {"id":"5","accountId":"broker","transferAccountId":null},{"id":"6","accountId":"old","transferAccountId":"broker"}]}""")
        assertEquals(listOf("bank" to "Bank", "card" to "Card", "cash" to "Cash"), Investments.others(budget, "broker").map { it.id to it.name })
    }

    @Test fun aDepositIsOfferedOnlyAccountsWhoseHistoryReachesBackToIt() {
        // A bank whose first transaction is a transfer in from the broker, a card that only took a transfer, and a new account with only its opening balance.
        val budget = JSONObject("""{"accounts":[{"id":"broker","name":"Broker"},{"id":"bank","name":"Bank"},{"id":"card","name":"Card"},{"id":"new","name":"New"}],
            "transactions":[{"id":"1","date":"2026-03-10","accountId":"bank","transferAccountId":null},{"id":"2","date":"2026-02-01","accountId":"broker","transferAccountId":"bank"},
            {"id":"3","date":"2026-06-15","accountId":"broker","transferAccountId":"card"},{"id":"4","date":"2026-05-01","accountId":"broker","transferAccountId":null}]}""")
        val others = Investments.others(budget, "broker").associateBy { it.id }
        // Either side of a transfer counts: the bank's history starts with the transfer from the broker.
        assertEquals(mapOf("bank" to "2026-02-01", "card" to "2026-06-15", "new" to null), others.mapValues { it.value.since })
        val bank = others.getValue("bank")
        assertFalse(bank.reaches("2026-01-31"))
        assertTrue(bank.reaches("2026-02-01"))
        assertTrue(bank.reaches("2026-11-01T10:00"))
        assertFalse(others.getValue("card").reaches("2026-06-14"))
        // An account with no transactions has nothing but a balance that already holds the money.
        assertFalse(others.getValue("new").reaches("2026-11-18"))
    }

    @Test fun depositsStillToReviewAreTheRowsNotInTheBudget() {
        val rows = JSONArray().put(JSONObject().put("duplicate", false)).put(JSONObject().put("duplicate", true)).put(JSONObject())
        assertEquals(2, Investments.fresh(found("U1", linked = "broker", rows = rows)))
        assertEquals(0, Investments.fresh(null))
    }

    /** As `BudgetRepository.edit` runs a transform: on a shallow copy, sending what changed. */
    private fun edited(base: JSONObject, transform: (JSONObject) -> Unit): Pair<JSONObject, JSONObject> {
        val copy = base.shallowCopy()
        transform(copy)
        val patch = budgetPatch(base, copy)
        return patch to applyBudgetPatch(base, patch)
    }

    @Test fun editsElsewhereLeaveHoldingsAndTheirTransactionsAsTheyWere() {
        val base = budget()
        // Renaming an account says nothing of the holdings: they stay the same objects.
        val (patch, after) = edited(base) { it.upsertRecord("accounts", JSONObject().put("id", "pension").put("name", "Pension fund")) }
        assertFalse(patch.has("extras"))
        assertSame(base.get(Investments.KEY), after.get(Investments.KEY))
        assertEquals(279250L, Investments.gain(after, "broker"))
        // Editing a market change's memo keeps what marks it, so the gain still counts it.
        val (memo, noted) = edited(base) { it.upsertRecord("transactions", JSONObject().put("id", "investment-change:broker:2026-10").put("memo", "Noted")) }
        val sent = memo.getJSONObject("transactions").getJSONArray("upsert").getJSONObject(0)
        assertEquals("change", sent.optString("investment"))
        assertEquals("Noted", sent.optString("memo"))
        assertEquals(279250L, Investments.gain(noted, "broker"))
        // Fields this app doesn't know come through with the entry.
        assertTrue(Investments.of(noted, "broker")!!.getJSONObject("futureField").getBoolean("kept"))
    }

    @Test fun unlinkingTakesOnlyThatAccountsHoldingsAndKeepsItsHistory() {
        val two = budget().apply { put(Investments.KEY, JSONArray().put(held()).put(held("pension", "U7654321"))) }
        val (patch, after) = edited(two) { Investments.unlink(it, "broker") }
        val kept = patch.getJSONObject("extras").getJSONObject("set").getJSONArray(Investments.KEY)
        assertEquals(1, kept.length())
        assertEquals("pension", kept.getJSONObject(0).optString("accountId"))
        assertNull(Investments.of(after, "broker"))
        assertNotNull(Investments.of(after, "pension"))
        assertEquals("Its transactions stay, so its balance does", two.getJSONArray("transactions").length(), after.getJSONArray("transactions").length())
        // The last one takes the list away rather than leaving it empty, as the engine does.
        val (last, none) = edited(after) { Investments.unlink(it, "pension") }
        assertEquals(Investments.KEY, last.getJSONObject("extras").getJSONArray("delete").getString(0))
        assertFalse(none.has(Investments.KEY))
        // Unlinking an account that has none changes nothing.
        assertEquals(0, edited(none) { Investments.unlink(it, "broker") }.first.length())
    }

    @Test fun theInvestmentsPageListsWhatCanHoldInvestmentsAndWhatSomethingFills() {
        val b = budget().apply {
            getJSONArray("accounts").put(account("untyped", "tracking")).put(account("savings", "cash", "savings"))
            // A cash account something fills is listed too, so it can be unlinked.
            put(Investments.KEY, JSONArray().put(held()).put(held("savings").put("source", "manual")))
        }
        assertEquals(listOf("broker", "pension", "untyped", "savings"), Investments.rows(b).map { it.optString("id") })
        assertEquals("Interactive Brokers", Investments.sourceName(held()))
        assertEquals(listOf("Bank connection", "Positions file", "Value you type in"),
            listOf("simplefin", "file", "manual").map { Investments.sourceName(JSONObject().put("source", it)) })
    }

    @Test fun aPositionsFileGoesToTheEngineAsItWasReadWithTheDayAndTheRate() {
        val read = JSONObject("""{"format":"csv","asOf":"2026-11-14","currency":"USD","cash":"820.5","value":"20820.5","skipped":0,
            "positions":[{"symbol":"VWRA","name":"Vanguard FTSE All-World","kind":"FUND","currency":"USD","quantity":"120.5","price":"165.975","value":"20000","cost":"18000"}]}""")
        // In the budget's own currency no rate goes along, whatever was typed for one.
        val same = Investments.fileSnapshot(read, "positions.csv", "2026-11-14", " usd ", "USD", "3.75")
        assertEquals(listOf("file", "positions.csv", "2026-11-14", "", "", "820.5"),
            listOf("source", "sourceAccount", "asOf", "currency", "rate", "cash").map { same.optString(it) })
        assertEquals("20000", same.getJSONArray("positions").getJSONObject(0).optString("value"))
        // In another, the currency goes with what one unit of it is worth, read as the person typed it.
        val other = Investments.fileSnapshot(read, "positions.csv", "2026-11-14", "usd", "SAR", "3,75")
        assertEquals("USD" to "3.75", other.optString("currency") to other.optString("rate"))
        assertEquals("USD", Investments.foreign("usd", "SAR"))
        assertNull(Investments.foreign("", "SAR"))
        // The rate given before is offered again for the same currency only.
        val before = JSONObject().put("currency", "USD").put("manualRate", "3.75")
        assertEquals("3.75", Investments.rememberedRate(before, "USD"))
        assertEquals("", Investments.rememberedRate(before, "EUR"))
        assertEquals("", Investments.rememberedRate(JSONObject().put("currency", "USD"), "USD"))
        assertEquals("", Investments.rememberedRate(null, "USD"))
    }

    @Test fun aTypedValueAndTheEnginesFiguresAreReadInHundredths() {
        val typed = Investments.typedSnapshot(3_100_050, "2026-11-17")
        assertEquals(listOf("manual", "2026-11-17", "31000.50"), listOf("source", "asOf", "total").map { typed.optString(it) })
        assertEquals(2_082_050L, Investments.cents("20820.5"))
        assertEquals(-1_250L, Investments.cents("-12.499"))
        assertEquals(0L, Investments.cents(""))
        assertEquals("0.05", Investments.decimal(5))
    }
}
