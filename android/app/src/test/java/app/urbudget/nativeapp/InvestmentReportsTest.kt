package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.time.LocalDate

/** The reports that read holdings: the Investments group, net worth's market split, and the Balance sheet by what the money is in. */
class InvestmentReportsTest {
    private val today = LocalDate.of(2026, 10, 21)

    @Before fun english() = EnglishWords.install()

    @Before fun plainAmounts() {
        Money.configure(JSONObject().put("currency", "USD").put("dateFormat", "iso")
            .put("numberFormat", JSONObject().put("decimal", ".").put("group", ",").put("symbol", "none")))
    }

    @After fun resetFormat() { Money.configure(JSONObject().put("currency", "SAR")) }

    private fun account(id: String, name: String, kind: String, type: String = "") =
        JSONObject().put("id", id).put("name", name).put("kind", kind).put("accountType", type).put("archived", false).put("netWorth", true).put("openingBalance", 0)

    /** A budget with a bank account, two accounts with holdings, a pension nothing fills and a card. */
    private fun budget(held: List<String> = listOf("ib", "fd")) = JSONObject().put("currency", "USD")
        .put("accounts", JSONArray(listOf(account("chk", "Checking", "cash", "checking"), account("ib", "Brokerage", "tracking", "investment"),
            account("fd", "Fund platform", "tracking", "investment"), account("rt", "Pension", "tracking", "retirement"), account("visa", "Visa", "credit"))))
        .put("investments", JSONArray(held.map { JSONObject().put("accountId", it) }))

    private fun month(m: String, accounts: Map<String, Long> = emptyMap(), netWorth: Long = 0, market: Long = 0) =
        ReportMonth(m, 0, 0, 0, maxOf(0L, netWorth), 0, netWorth, 0, 0, emptyMap(), emptyMap(), accounts, market)

    private fun data(months: List<ReportMonth>, opening: Map<String, Long>?) =
        ReportData(months, emptyMap(), emptyMap(), emptyList(), emptyMap(), 0, 0, null, null, opening, months.associate { it.month to null })

    /** The engine's `investmentsReport`, written to the shape the engine and the three apps agreed (the plan's Appendix A). */
    private val wire = """
    {
      "from": "2026-08", "to": "2026-10",
      "total": { "start": 100000, "end": 150000, "put": 30000, "gain": 20000, "rate": 0.16, "yearly": null, "days": 92 },
      "months": [
        { "month": "2026-08", "value": 110000, "gain": 10000, "put": 0, "byAccount": { "ib": 110000 } },
        { "month": "2026-09", "value": 135000, "gain": 5000, "put": 20000, "byAccount": { "ib": 115000, "fd": 20000 } },
        { "month": "2026-10", "value": 150000, "gain": 5000, "put": 10000, "byAccount": { "ib": 128000, "fd": 22000 } }
      ],
      "accounts": [ {
        "accountId": "ib", "source": "ibkr", "sourceAccount": "U1234567", "asOf": "2026-10-20", "joined": "2025-11-14",
        "start": 100000, "end": 128000, "put": 10000, "gain": 18000, "rate": 0.17, "estimatedDays": 2, "missing": ["Transfers"], "sparse": false,
        "startParts": { "month": "2026-07", "asOf": "2026-07-31", "byKind": [ { "key": "Stocks", "value": 60000 }, { "key": "Bonds", "value": 30000 } ], "cash": 8000, "accrued": 2000, "rest": 0 },
        "endParts": { "month": "2026-10", "asOf": "2026-10-20",
          "byKind": [ { "key": "Stocks", "value": 90000 }, { "key": "Options", "value": -2000 }, { "key": "Holdings", "value": 5000 } ], "cash": 30000, "accrued": 3000, "rest": 2000 }
      }, {
        "accountId": "fd", "source": "file", "sourceAccount": "positions.csv", "asOf": "2026-10-19", "joined": "2026-09-05",
        "start": 0, "end": 22000, "put": 20000, "gain": 2000, "rate": null, "estimatedDays": 0, "missing": [], "sparse": true,
        "startParts": null,
        "endParts": { "month": "2026-10", "asOf": "2026-10-19", "byKind": [ { "key": "Funds", "value": 20000 } ], "cash": 2000, "accrued": 0, "rest": 0 }
      } ],
      "allocation": {
        "asOf": "2026-10-19", "total": 150000, "count": 4, "cash": 32000, "accrued": 3000, "cost": 86500, "unrealised": 21500, "unrealisedRate": 0.2486,
        "largestShare": 0.6, "topFiveShare": 0.7566,
        "byKind": [ { "key": "Stocks", "value": 90000, "share": 0.6 }, { "key": "Funds", "value": 20000, "share": 0.1333 }, { "key": "Holdings", "value": 5000, "share": 0.0333 },
          { "key": "Options", "value": -2000, "share": 0 } ],
        "byAccount": [ { "key": "ib", "value": 128000, "share": 0.8533 }, { "key": "fd", "value": 22000, "share": 0.1467 } ],
        "byCurrency": [ { "key": "USD", "value": 130000, "share": 0.8667 }, { "key": "EUR", "value": 20000, "share": 0.1333 } ]
      },
      "holdings": [
        { "key": "US0378331005", "symbol": "AAPL", "name": "Apple", "group": "Stocks", "one": "Stock", "currency": "USD", "value": 90000, "cost": 70000, "gain": 20000, "rate": 0.2857, "share": 0.6, "accounts": ["ib"] },
        { "key": "IE00B4L5Y983", "symbol": "IWDA", "name": "World fund", "group": "Funds", "one": "Fund", "currency": "EUR", "value": 20000, "cost": 18000, "gain": 2000, "rate": 0.1111, "share": 0.1333, "accounts": ["fd", "ib"] },
        { "key": "MYST USD", "symbol": "", "name": "Mystery", "group": "Holdings", "one": "", "currency": "USD", "value": 5000, "cost": null, "gain": null, "rate": null, "share": 0.0333, "accounts": ["ib"] },
        { "key": "SPY P", "symbol": "SPY 18DEC26 710 P", "name": "SPY put", "group": "Options", "one": "Option", "currency": "USD", "value": -2000, "cost": -1500, "gain": -500, "rate": null, "share": null, "accounts": ["ib"] }
      ],
      "income": {
        "months": [ { "month": "2026-08", "dividends": 400, "interest": 100, "tax": -60, "fees": 0, "other": 0 },
          { "month": "2026-09", "dividends": 0, "interest": 100, "tax": 0, "fees": -50, "other": 0 },
          { "month": "2026-10", "dividends": 800, "interest": 100, "tax": -120, "fees": 0, "other": 0 } ],
        "dividends": 1200, "interest": 300, "tax": -180, "fees": -50, "other": 0, "yield": 0.0123,
        "commissions": [ { "accountId": "ib", "from": "2025-10-21", "to": "2026-10-20", "amount": -700 } ],
        "reporting": ["ib"], "silent": ["fd"],
        "byAccount": [ { "accountId": "ib", "dividends": 1200, "interest": 300, "tax": -180, "fees": -50, "other": 0 } ]
      },
      "unfilled": ["rt"]
    }
    """

    private fun portfolio(change: (JSONObject) -> Unit = {}) = parsePortfolio(JSONObject(wire).also(change))

    /** Balances at the end of July and of October, the range the portfolio above covers. */
    private val report = data(listOf(month("2026-08"), month("2026-09"),
        month("2026-10", accounts = mapOf("chk" to 60000L, "ib" to 128000L, "fd" to 22000L, "rt" to 41000L, "visa" to -5000L))),
        opening = mapOf("chk" to 50000L, "ib" to 100000L, "fd" to 0L, "rt" to 40000L, "visa" to -10000L))

    @Test fun thePortfolioParsesFromTheEnginesShape() {
        val p = portfolio()
        assertEquals("2026-08" to "2026-10", p.from to p.to)
        assertEquals(PortfolioTotal(100000, 150000, 30000, 20000, 0.16, null, 92), p.total)
        assertEquals(listOf(110000L, 135000L, 150000L), p.months.map { it.value })
        assertEquals(mapOf("fd" to 20000L, "ib" to 115000L), p.months[1].byAccount)
        val (broker, file) = p.accounts
        assertEquals(listOf("Transfers"), broker.missing)
        assertEquals(2, broker.estimatedDays)
        assertEquals(AccountParts("2026-07", "2026-07-31", listOf(KindAmount("Stocks", 60000), KindAmount("Bonds", 30000)), 8000, 2000, 0), broker.startParts)
        assertEquals(2000L, broker.endParts?.rest)
        // What can't be known arrives as null, and stays unknown.
        assertNull(file.rate)
        assertNull(file.startParts)
        assertTrue(file.sparse)
        assertEquals("2026-09-05", file.joined)
        assertEquals(86500L, p.allocation.cost)
        assertEquals(0.6, p.allocation.largestShare!!, 0.0)
        assertEquals(listOf("USD", "EUR"), p.allocation.byCurrency.map { it.key })
        assertEquals(listOf("fd", "ib"), p.holdings[1].accounts)
        assertNull(p.holdings[2].cost)
        assertNull(p.holdings[3].share)
        assertEquals(IncomeMonth("2026-09", 0, 100, 0, -50, 0), p.income.months[1])
        assertEquals(0.0123, p.income.yield!!, 0.0)
        assertEquals(listOf(Commission("ib", "2025-10-21", "2026-10-20", -700)), p.income.commissions)
        assertEquals(listOf("ib") to listOf("fd"), p.income.reporting to p.income.silent)
        assertEquals(listOf(AccountIncome("ib", 1200, 300, -180, -50, 0)), p.income.byAccount)
        // An engine that knows nothing of trades says nothing of them.
        assertNull(p.total.realised)
        assertTrue(p.months.all { it.realised == null } && p.accounts.all { it.realised == null } && p.income.months.all { it.commissions == null })
        assertTrue(p.income.byHolding.isEmpty())
        assertEquals(listOf("rt"), p.unfilled)
        // An older engine's months carry no market figure: it reads as none.
        assertEquals(0L, parseReport(JSONObject().put("months", JSONArray().put(JSONObject().put("month", "2026-10")))).months.single().market)
        assertEquals(250L, parseReport(JSONObject().put("months", JSONArray().put(JSONObject().put("month", "2026-10").put("market", 250)))).months.single().market)
    }

    @Test fun theInvestmentsGroupShowsOnlyWithHoldings() {
        assertTrue(holdingAccountIds(budget(emptyList())).isEmpty())
        // An entry for an account that's gone doesn't count.
        assertEquals(setOf("ib"), holdingAccountIds(budget(listOf("ib", "deleted"))))
        assertFalse(shownReports(holdingAccountIds(budget(emptyList())).isNotEmpty()).any { it.id == INVESTMENTS_REPORT })
        assertTrue(shownReports(holdingAccountIds(budget()).isNotEmpty()).any { it.id == INVESTMENTS_REPORT })
        assertEquals(5, shownReports(false).size)
        val ctx = parseReportBudget(budget(), today)
        assertEquals(setOf("ib", "fd"), ctx.holdings)
        // The investment reports' account filter lists only the accounts with holdings.
        assertEquals(listOf("Brokerage", "Fund platform"), accountFilterGroups(ctx, budgetOnly = false, holdingsOnly = true).flatMap { it.items }.map { it.label })
        assertEquals(5, accountFilterGroups(ctx, budgetOnly = false).flatMap { it.items }.size)
    }

    @Test fun netWorthsChangeSplitsIntoMarketsAndEverythingElse() {
        val ctx = parseReportBudget(JSONObject().put("currency", "USD").put("accounts", JSONArray(listOf(account("a1", "All", "tracking")))), today)
        fun months(markets: List<Long>) = listOf("2026-08" to 1000L, "2026-09" to 1500L, "2026-10" to 1400L).mapIndexed { i, (name, worth) -> month(name, mapOf("a1" to worth), worth, markets[i]) }
        val plain = netWorthView(data(months(listOf(0, 0, 0)), mapOf("a1" to 800L)), ctx, emptySet())
        val split = netWorthView(data(months(listOf(300, -200, 0)), mapOf("a1" to 800L)), ctx, emptySet())
        // A budget without investments sees what it always did.
        assertEquals(listOf("Grew", "Shrank"), plain.change.series.map { it.label })
        assertEquals(4, plain.tiles.size)
        assertEquals(listOf("Markets", "Everything else"), split.change.series.map { it.label })
        assertEquals(listOf("Markets", "Everything else"), split.changeLegend.map { it.label })
        assertEquals(listOf(300.0, -200.0, 0.0), split.change.series[0].values)
        assertEquals(listOf(-100.0, 700.0, -100.0), split.change.series[1].values)
        // The two add up to the one series there was, month by month.
        fun sums(view: NetWorthView) = view.change.keys.indices.map { i -> view.change.series.sumOf { it.values[i] } }
        assertEquals(listOf(200.0, 500.0, -100.0), sums(plain))
        assertEquals(sums(plain), sums(split))
        assertEquals("+5.00", split.change.readouts[1].last().value)
        assertEquals(plain.tiles, split.tiles.take(4))
        assertEquals(Tile("From markets", "+1.00", "17% of the change", Tone.Positive), split.tiles[4])
        // Markets against the change's direction are no share of it.
        assertNull(netWorthView(data(months(listOf(-300, 0, 0)), mapOf("a1" to 800L)), ctx, emptySet()).tiles[4].note)
    }

    @Test fun theBalanceSheetGroupsAssetsByWhatTheMoneyIsIn() {
        val ctx = parseReportBudget(budget(), today)
        val whole = balanceSheetView(report, ctx, emptySet())
        val view = balanceSheetView(report, ctx, emptySet(), portfolio())
        fun row(view: BalanceSheetView, key: String) = view.statement.first { it.key == key }

        // The totals are the accounts', so they are what they were, and what Net worth says.
        assertEquals(listOf("2,510.00", "50.00", "2,460.00"), view.tiles.take(3).map { it.value })
        assertEquals(whole.tiles.take(4), view.tiles.take(4))
        assertEquals(row(whole, "t:Assets"), row(view, "t:Assets"))
        assertEquals(row(whole, "__net"), row(view, "__net"))
        assertEquals(whole.owe, view.owe)
        // Cash on hand is the Cash and bank section, the cash at brokers with it.
        assertEquals("600.00" to "920.00", whole.tiles[4].value to view.tiles[4].value)

        // An account is split across the sections, and its lines add up to its balance at each end.
        val assets = balanceSheetLines(report, ctx, emptySet()).filter { !it.debt }
        val owned = sheetAssets(assets, portfolio())
        val broker = owned.parts.filter { it.account.id == "ib" }
        assertEquals(listOf("Cash and bank", "Stocks", "Bonds", "Options", "Other investments", "Receivables", "Other investments"), broker.map { it.section })
        assertEquals(128000L, broker.sumOf { it.end })
        assertEquals(100000L, broker.sumOf { it.start!! })
        assertEquals(listOf("Cash at Brokerage", "Brokerage", "Brokerage", "Brokerage", "Brokerage", "Accrued income at Brokerage", "Brokerage · Since prices from 2026-10-20"),
            broker.map { it.title })
        // What was sold keeps its line (it was there at the start), and an option sold is a line below zero, not a debt.
        assertEquals(30000L to 0L, broker[2].start to broker[2].end)
        assertEquals(-2000L, broker[3].end)
        // An account that held nothing at the start has a start of nothing, known without a snapshot.
        assertEquals(listOf(0L, 0L), owned.parts.filter { it.account.id == "fd" }.map { it.start })
        assertTrue(owned.startUnknown.isEmpty() && owned.whole.isEmpty())

        // The sections in order, each only when something is in it; the pension nothing fills goes whole.
        assertEquals(listOf("Cash and bank", "Stocks", "Funds", "Bonds", "Options", "Other investments", "Receivables"),
            view.statement.filter { it.key.startsWith("s:Assets:") }.map { it.title })
        assertEquals("920.00" to "From 580.00 · Change 340.00 · 36.7%", row(view, "s:Assets:Cash and bank").let { it.value to it.subtitle })
        assertEquals("480.00" to "From 400.00 · Change 80.00 · 19.1%", row(view, "s:Assets:Other investments").let { it.value to it.subtitle })
        assertEquals("From 400.00 · Change 10.00 · 16.3%", row(view, "a:rt").subtitle)
        assertEquals("-20.00", row(view, "a:ib:Options").value)
        // The bars of what's owned follow the same sections.
        assertEquals(listOf("Cash and bank", "Stocks", "Funds", "Other investments", "Receivables"), view.own.map { it.label })
        // One sentence says how the investments changed: put in is the change less what markets did.
        assertEquals(FigureRow("note:Assets:0", "Investments changed by +500.00: +300.00 put in, +200.00 from markets.", style = RowStyle.Note),
            view.statement[view.statement.indexOfFirst { it.key == "t:Assets" } + 1])
        assertEquals(1, view.statement.count { it.style == RowStyle.Note })
    }

    @Test fun aStartThatIsntKeptShowsOnlyItsShare() {
        val ctx = parseReportBudget(budget(), today)
        val view = balanceSheetView(report, ctx, emptySet(), portfolio { it.getJSONArray("accounts").getJSONObject(0).put("startParts", JSONObject.NULL) })
        fun row(key: String) = view.statement.first { it.key == key }
        // Each of the account's lines, and every section holding one, has no start and no change: it says its share of what's owned.
        assertEquals("35.9% of assets", row("a:ib:Stocks").subtitle)
        assertEquals("12.0% of assets", row("a:ib:cash").subtitle)
        assertEquals("36.7% of assets", row("s:Assets:Cash and bank").subtitle)
        assertEquals("35.9% of assets", row("s:Assets:Stocks").subtitle)
        // A section without one keeps its figures, and the totals come from the accounts.
        assertEquals("From 0.00 · Change 200.00 · 8.0%", row("s:Assets:Funds").subtitle)
        assertEquals("From 1,900.00 · Change 610.00", row("t:Assets").subtitle)
        assertEquals("From 1,800.00 · Change 660.00", row("__net").subtitle)
        // What it sold before the range's end can't be known, so it has no line.
        assertTrue(view.statement.none { it.key == "a:ib:Bonds" })
        assertEquals(listOf("Investments changed by +500.00: +300.00 put in, +200.00 from markets.",
            "Start and change not shown by type, because what was held then isn't kept: Brokerage."),
            view.statement.filter { it.style == RowStyle.Note }.map { it.title })
    }

    @Test fun anAccountThatCantBeSplitGoesWholeUnderOtherInvestments() {
        val ctx = parseReportBudget(budget(), today)
        val view = balanceSheetView(report, ctx, emptySet(), portfolio { it.getJSONArray("accounts").getJSONObject(1).put("endParts", JSONObject.NULL) })
        val fund = view.statement.first { it.key == "a:fd" }
        assertEquals("Fund platform" to "220.00", fund.title to fund.value)
        assertEquals("From 0.00 · Change 220.00 · 8.8%", fund.subtitle)
        val other = view.statement.indexOfFirst { it.key == "s:Assets:Other investments" }
        assertTrue(view.statement.indexOf(fund) > other && view.statement.none { it.key == "s:Assets:Funds" })
        assertEquals("700.00", view.statement[other].value)
        assertEquals("Not broken down, because what was held then isn't kept: Fund platform.", view.statement.last { it.style == RowStyle.Note }.title)
        assertEquals("2,510.00", view.tiles[0].value)
        // Without the portfolio (a budget with no holdings), an investment account is one line there too.
        assertEquals("Other investments", balanceSheetLines(report, ctx, emptySet()).first { it.account.id == "ib" }.section)
    }

    @Test fun performanceLaysOutThePortfolio() {
        val ctx = parseReportBudget(budget(), today)
        val view = performanceView(portfolio(), ctx, today)!!
        assertEquals(listOf("Value now", "Gain", "Return", "Money put in"), view.tiles.map { it.label })
        assertEquals(listOf("1,500.00", "+200.00", "+16.0%", "+300.00"), view.tiles.map { it.value })
        assertEquals(listOf("Prices from 2026-10-19", "over 3 months", null, null), view.tiles.map { it.note })
        assertEquals(listOf("Brokerage", "Fund platform"), view.value.series.map { it.label })
        assertEquals(listOf(0.0, 20000.0, 22000.0), view.value.series[1].values)
        // The line is what was there to begin with and what was put in since.
        assertEquals(listOf(100000.0, 120000.0, 130000.0), view.value.line?.values)
        assertEquals(listOf("Brokerage", "Fund platform", "Money put in"), view.valueLegend.map { it.label })
        assertEquals(listOf("+100.00" to "Gain"), view.gains!!.readouts[0].map { it.value to it.label })
        val (broker, file) = view.accounts
        assertEquals("Interactive Brokers · Start 1,000.00 · Put in +100.00", broker.line)
        assertEquals(listOf("1,280.00", "+180.00", "+17.0%", "ib"), listOf(broker.value, broker.gain, broker.rate, broker.open))
        assertEquals("Positions file · Start 0.00 · Put in +200.00", file.line)
        assertNull(file.rate)
        assertEquals(listOf(0, 1), view.accounts.map { it.color })
        assertEquals(listOf("lacking", "estimated", "sparse", "unfilled"), view.notes.map { it.key })
        assertEquals("Valued only when updated, so a gain shows in the month of the next update: Fund platform.", view.notes[2].text)
        assertEquals("Not counted here, because nothing keeps them up to date: Pension." to "rt", view.notes[3].let { it.text to it.setUp })
        // A range that ended before this month is a value on that day, and prices a few days old are said.
        val past = performanceView(portfolio(), ctx, LocalDate.of(2026, 11, 20))!!
        assertEquals(Tile("Value", "1,500.00", "end of October 2026"), past.tiles[0])
        assertTrue(performanceView(portfolio(), ctx, LocalDate.of(2026, 10, 30))!!.notes.any { it.key == "stale" })
        // With no account chosen there is nothing to show.
        assertNull(performanceView(portfolio { it.put("accounts", JSONArray()) }, ctx, today))
    }

    @Test fun allocationMergesWhatTheAccountsHold() {
        val ctx = parseReportBudget(budget(), today)
        val view = allocationView(portfolio(), ctx, "kind", today)!!
        assertEquals(listOf("4", "60%", "76%", "21%"), view.tiles.map { it.value })
        assertEquals("Apple", view.tiles[1].note)
        assertEquals("Prices from 2026-10-19", view.asOf)
        // Cash and income still to be paid are kinds too; an option sold is listed and is no share.
        // What no source gave a kind for is "Other investments", as on the Balance sheet.
        assertEquals(listOf("Stocks", "Cash", "Funds", "Other investments", "Accrued"), view.slices.map { it.label })
        assertEquals(view.slices.map { it.label } + "Options", view.bars.map { it.label })
        assertEquals(listOf("60%", "21%", "13%", "3%", "2%"), view.slices.map { it.share })
        assertEquals("Options" to "—", view.bars.last().let { it.label to it.detail })
        assertEquals(listOf("Brokerage", "Fund platform"), allocationView(portfolio(), ctx, "account", today)!!.slices.map { it.label })
        assertEquals(listOf("USD", "EUR"), allocationView(portfolio(), ctx, "currency", today)!!.slices.map { it.label })
        assertEquals("4 holdings", view.holdingsAside)
        assertEquals("Unrealised gain +215.00 · Return on cost +24.9%", view.onCost)
        // Before the next sync names what each one is, a holding is called by its kind's singular; an option's long code is left out.
        assertEquals(listOf("Stock · AAPL · Brokerage", "Fund · IWDA · Fund platform, Brokerage", "Other investments · Brokerage", "Option · Brokerage"),
            view.holdings.map { it.line })
        assertEquals(listOf("AAPL", "IWDA", "MYSTER", "SPY"), view.holdings.map { it.mark })
        assertEquals(listOf("60%", "13%", "3%", null), view.holdings.map { it.share })
        assertEquals(listOf("+28.6%", "+11.1%", null, null), view.holdings.map { it.rate })
        // A holding wears its kind's colour in the ring.
        assertEquals(view.slices.first { it.key == "Funds" }.color, view.holdings[1].color)
        assertEquals(view.slices.first { it.label == "Other investments" }.color, view.holdings[2].color)
        // Holdings of both unnamed kinds are one part of the ring.
        val both = allocationView(portfolio { it.getJSONObject("allocation").getJSONArray("byKind").put(JSONObject().put("key", "Other holdings").put("value", 1000).put("share", 0.0067)) }, ctx, "kind", today)!!
        assertEquals(1, both.bars.count { it.label == "Other investments" })
        assertEquals("60.00", both.bars.first { it.label == "Other investments" }.valueText)
        // The accounts' prices are from different days, so each is dated.
        assertEquals(listOf("Brokerage" to "2026-10-20", "Fund platform" to "2026-10-19"), view.prices.map { it.title to it.value })
    }

    @Test fun incomeAndCostsAddUp() {
        val ctx = parseReportBudget(budget(), today)
        val view = investIncomeView(portfolio(), ctx)!!
        assertEquals(listOf("Dividends and interest", "Fees, tax and commissions", "Net", "Yield"), view.tiles.map { it.label })
        assertEquals(listOf("+15.00", "-9.30", "+5.70", "1.2%"), view.tiles.map { it.value })
        assertEquals("Income over the average value", view.tiles[3].note)
        // Only what has a figure is charted: what arrived goes up, what was paid goes down.
        assertEquals(listOf("Dividends", "Interest", "Tax withheld", "Fees"), view.chart!!.series.map { it.label })
        assertEquals(listOf(-60.0, 0.0, -120.0), view.chart!!.series[2].values)
        assertEquals("+4.40", view.chart!!.readouts[0].last().value)
        assertEquals(listOf("dividends", "interest", "tax", "fees", "commissions:0", "__net"), view.lines.map { it.key })
        assertEquals("-7.00" to "2025-10-21 to 2026-10-20", view.lines[4].let { it.value to it.subtitle })
        assertEquals("Aug 2026 – Oct 2026", view.dates)
        assertEquals(listOf("No income or costs reported: Fund platform."), view.notes.map { it.text })
        // One account reporting is the totals already: it has no row of its own.
        assertTrue(view.accounts.isEmpty())
        // When no account reports income, the note is all there is to say.
        val silent = investIncomeView(portfolio { it.getJSONObject("income").put("reporting", JSONArray()).put("silent", JSONArray(listOf("ib", "fd"))) }, ctx)!!
        assertTrue(silent.tiles.isEmpty() && silent.chart == null && silent.lines.isEmpty())
        assertEquals("No income or costs reported: Brokerage, Fund platform.", silent.notes.single().text)
    }

    /**
     * The engine's own answer for the budget the engine and the three apps share
     * (shared/fixtures/investments-report.json): what it writes is what these reports read. The
     * figures are the engine's to change, so what's checked is how they hang together.
     */
    @Test fun theSharedFixtureBuildsEveryReport() {
        val fixtures = java.io.File("../../shared/fixtures")
        val budget = JSONObject(java.io.File(fixtures, "investments-report.json").readText()).getJSONObject("budget")
        val p = parsePortfolio(JSONObject(java.io.File(fixtures, "investments-report-expected.json").readText()))
        val ctx = parseReportBudget(budget, today)
        assertEquals(ctx.holdings, p.accounts.mapTo(HashSet()) { it.accountId })
        assertTrue(p.accounts.isNotEmpty() && p.months.isNotEmpty() && p.holdings.isNotEmpty())
        // The months are the accounts added up, and the range's figures the months'.
        p.months.forEach { m -> assertEquals(m.month, m.value, m.byAccount.values.sum()) }
        assertEquals(p.total.gain, p.months.sumOf { it.gain })
        assertEquals(p.total.end - p.total.start, p.total.put + p.total.gain)
        // An account's split adds up to its balance, which is what lets the Balance sheet take it apart.
        p.accounts.forEach { a -> a.endParts?.let { parts -> assertEquals(a.accountId, a.end, parts.byKind.sumOf { it.value } + parts.cash + parts.accrued + parts.rest) } }

        val performance = performanceView(p, ctx, today)!!
        assertEquals(p.months.size, performance.value.keys.size)
        assertEquals((p.total.end - p.total.gain).toDouble(), performance.value.line!!.values.last()!!, 0.0)
        assertEquals(p.accounts.map { ctx.accountName(it.accountId) }, performance.accounts.map { it.name })
        assertTrue(performance.notes.any { it.key == "unfilled" && it.setUp == p.unfilled.single() })
        val allocation = allocationView(p, ctx, "kind", today)!!
        assertEquals(p.allocation.total, allocation.slices.sumOf { it.value })
        assertEquals(p.holdings.map { it.name }, allocation.holdings.map { it.name })
        assertEquals(p.accounts.size, allocationView(p, ctx, "account", today)!!.bars.size)
        val income = investIncomeView(p, ctx)!!
        // More than one account reports income here: each has its row, adding up to the totals, with its commissions under it.
        assertEquals(p.income.reporting, p.income.byAccount.map { it.accountId })
        assertEquals(p.income.dividends + p.income.interest + p.income.tax + p.income.fees + p.income.other,
            p.income.byAccount.sumOf { it.dividends + it.interest + it.tax + it.fees + it.other })
        assertEquals(p.income.reporting.map { "a:$it" }, income.accounts.filter { it.style != RowStyle.Child }.map { it.key })
        assertEquals(p.income.commissions.size, income.accounts.count { it.style == RowStyle.Child })
        assertEquals(1, income.lines.count { it.key == "commissions" })
        // The fixture's trading account lists its trades: realised gains, commissions by month and dividends by holding.
        assertEquals(p.months.mapNotNull { it.realised }.sum(), p.total.realised)
        assertTrue(performance.tiles[1].note!!.startsWith("Realised "))
        val byMonth = p.income.months.sumOf { it.commissions ?: 0L }
        assertTrue(byMonth != 0L)
        assertEquals(p.income.months.map { (it.commissions ?: 0L).toDouble() }, income.chart!!.series.first { it.label == "Commissions on trades" }.values)
        assertEquals(ctx.signed(byMonth + p.income.commissions.sumOf { it.amount }), income.lines.first { it.key == "commissions" }.value)
        assertEquals(p.income.byHolding.map { it.name }, income.holdings.map { it.title })
        // Each account's row carries its own commissions, so the rows add up to the report's net.
        val nets = p.income.byAccount.map { it.dividends + it.interest + it.tax + it.fees + it.other + (it.commissions ?: 0L) }
        assertEquals(byMonth + p.income.commissions.sumOf { it.amount }, p.income.byAccount.sumOf { it.commissions ?: 0L })
        assertEquals(nets.map { ctx.signed(it) }, income.accounts.filter { it.style != RowStyle.Child }.map { it.value })
        assertEquals(ctx.signed(nets.sum()), income.lines.first { it.key == "__net" }.value)
        assertEquals(ctx.signed(nets.sum()), income.tiles[2].value)
        assertTrue(income.notes.single().text.startsWith("No income or costs reported: "))

        // The Balance sheet over the same range, from the balances the portfolio's months carry.
        val last = p.months.last()
        val balances = data(listOf(month(last.month, accounts = last.byAccount)), opening = p.accounts.associate { it.accountId to it.start })
        val lines = balanceSheetLines(balances, ctx, emptySet())
        val owned = sheetAssets(lines, p)
        p.accounts.forEach { a ->
            val parts = owned.parts.filter { it.account.id == a.accountId }
            assertEquals(a.accountId, a.end, parts.sumOf { it.end })
            if (parts.all { it.start != null }) assertEquals(a.accountId, a.start, parts.sumOf { it.start!! })
        }
        // A value typed in says nothing of what's in it: the account is one line, under Other investments.
        val typed = p.accounts.first { it.source == "manual" }
        assertEquals(listOf("a:${typed.accountId}" to "Other investments"), owned.parts.filter { it.account.id == typed.accountId }.map { it.key to it.section })
        val sheet = balanceSheetView(balances, ctx, emptySet(), p)
        assertEquals(balanceSheetView(balances, ctx, emptySet()).tiles.take(4), sheet.tiles.take(4))
        assertEquals(ctx.money(lines.sumOf { it.end }), sheet.statement.first { it.key == "t:Assets" }.value)
        assertEquals(ctx.money(p.accounts.sumOf { it.endParts?.cash ?: 0L }), sheet.statement.first { it.key == "s:Assets:Cash and bank" }.value)
    }

    @Test fun incomeAndCostsAreSaidByAccountWhenSeveralReport() {
        val ctx = parseReportBudget(budget(), today)
        val view = investIncomeView(portfolio {
            it.getJSONObject("income").put("reporting", JSONArray(listOf("ib", "fd"))).put("silent", JSONArray())
                .put("byAccount", JSONArray(listOf(
                    JSONObject().put("accountId", "ib").put("dividends", 1000).put("interest", 300).put("tax", -150).put("fees", -50).put("other", 0),
                    JSONObject().put("accountId", "fd").put("dividends", 200).put("interest", 0).put("tax", -30).put("fees", 0).put("other", 0))))
        }, ctx)!!
        // Each account's row: what it earned, and what it was charged with its commissions; then its commissions' own line.
        assertEquals(listOf("a:ib", "c:ib:0", "a:fd"), view.accounts.map { it.key })
        val (broker, traded, fund) = view.accounts
        assertEquals(listOf("Brokerage", "+4.00", "Dividends and interest +13.00 · Fees, tax and commissions -9.00"), listOf(broker.title, broker.value, broker.subtitle))
        assertEquals(listOf("Commissions on trades", "-7.00", "2025-10-21 to 2026-10-20"), listOf(traded.title, traded.value, traded.subtitle))
        assertEquals(RowStyle.Child, traded.style)
        assertEquals(listOf("Fund platform", "+1.70", "Dividends and interest +2.00 · Fees, tax and commissions -0.30"), listOf(fund.title, fund.value, fund.subtitle))
        // The accounts' rows add up to the net (4.00 and 1.70), and the totals say the commissions once, without an account's dates.
        assertEquals("+5.70", view.lines.last().value)
        assertEquals(listOf("dividends", "interest", "tax", "fees", "commissions", "__net"), view.lines.map { it.key })
        assertEquals("-7.00" to null, view.lines[4].let { it.value to it.subtitle })
        assertTrue(view.notes.isEmpty())
        // An engine that doesn't say it by account leaves the card out, and the commissions stay listed by account.
        val older = investIncomeView(portfolio { it.getJSONObject("income").put("reporting", JSONArray(listOf("ib", "fd"))).remove("byAccount") }, ctx)!!
        assertTrue(older.accounts.isEmpty())
        assertEquals("Brokerage · 2025-10-21 to 2026-10-20", older.lines.first { it.key == "commissions:0" }.subtitle)
    }

    /** The engine's figures from a statement that lists its trades and names the holdings its dividends came from. */
    private fun traded() = portfolio {
        it.getJSONObject("total").put("realised", 1700)
        it.getJSONArray("accounts").getJSONObject(0).put("realised", 1700)
        it.getJSONArray("months").getJSONObject(2).put("realised", 1700)
        val income = it.getJSONObject("income")
        // The account's commissions come by month now, so its statement's one figure is gone.
        income.put("commissions", JSONArray())
        listOf(JSONObject.NULL, -120, -80).forEachIndexed { i, paid -> income.getJSONArray("months").getJSONObject(i).put("commissions", paid) }
        income.put("byHolding", JSONArray((1..12).map { n ->
            JSONObject().put("key", "K$n").put("symbol", if (n == 2) "" else "S$n").put("name", "Holding $n").put("dividends", 1300 - n * 100).put("tax", if (n == 1) -180 else 0)
                .put("value", if (n == 1) 90000 else JSONObject.NULL).put("yield", if (n == 1) 0.0133 else JSONObject.NULL)
        }))
    }

    @Test fun tradesAddRealisedGainsAndCommissionsByMonth() {
        val ctx = parseReportBudget(budget(), today)
        val p = traded()
        assertEquals(1700L, p.total.realised)
        assertEquals(listOf(null, null, 1700L), p.months.map { it.realised })
        assertEquals(listOf(1700L, null), p.accounts.map { it.realised })
        assertEquals(listOf(null, -120L, -80L), p.income.months.map { it.commissions })
        // The Gain tile says how much of the gain was taken by selling.
        assertEquals(Tile("Gain", "+200.00", "Realised +17.00", Tone.Positive), performanceView(p, ctx, today)!!.tiles[1])
        val view = investIncomeView(p, ctx)!!
        // Commissions are a series of the chart, and are in the month's total and the range's.
        assertEquals(listOf("Dividends", "Interest", "Tax withheld", "Fees", "Commissions on trades"), view.chart!!.series.map { it.label })
        assertEquals(listOf(0.0, -120.0, -80.0), view.chart!!.series.last().values)
        assertEquals("-0.70", view.chart!!.readouts[1].last().value)
        assertEquals(listOf("+15.00", "-4.30", "+10.70"), view.tiles.take(3).map { it.value })
        assertEquals(listOf("dividends", "interest", "tax", "fees", "commissions", "__net"), view.lines.map { it.key })
        assertEquals("-2.00" to null, view.lines[4].let { it.value to it.subtitle })
        assertEquals("+10.70", view.lines.last().value)
        // Each account's row counts its own commissions, by month or from its statement, so the rows add up to the net.
        val split = investIncomeView(portfolio { json ->
            val income = json.getJSONObject("income").put("reporting", JSONArray(listOf("ib", "fd"))).put("silent", JSONArray())
            income.getJSONArray("commissions").getJSONObject(0).put("accountId", "fd")
            income.getJSONArray("months").getJSONObject(1).put("commissions", -200)
            income.put("byAccount", JSONArray(listOf(
                JSONObject().put("accountId", "ib").put("dividends", 1000).put("interest", 300).put("tax", -150).put("fees", -50).put("other", 0).put("commissions", -200),
                JSONObject().put("accountId", "fd").put("dividends", 200).put("interest", 0).put("tax", -30).put("fees", 0).put("other", 0).put("commissions", -700))))
        }, ctx)!!
        assertEquals(listOf("a:ib", "a:fd", "c:fd:0"), split.accounts.map { it.key })
        assertEquals("+9.00" to "Dividends and interest +13.00 · Fees, tax and commissions -4.00", split.accounts[0].let { it.value to it.subtitle })
        assertEquals("-5.30" to "Dividends and interest +2.00 · Fees, tax and commissions -7.30", split.accounts[1].let { it.value to it.subtitle })
        // The account still listed with its statement's figure keeps the dated line under it.
        assertEquals("-7.00" to "2025-10-21 to 2026-10-20", split.accounts[2].let { it.value to it.subtitle })
        assertEquals("+3.70", split.lines.last().value)
        // What's known by month and a statement's one figure for another account add up in the one line.
        val both = investIncomeView(portfolio { json ->
            json.getJSONObject("income").getJSONArray("months").getJSONObject(1).put("commissions", -120)
        }, ctx)!!
        assertEquals("-8.20", both.lines.first { it.key == "commissions" }.value)
        assertTrue(both.lines.none { it.key.startsWith("commissions:") })
    }

    @Test fun dividendsAreListedByHolding() {
        val ctx = parseReportBudget(budget(), today)
        val p = traded()
        assertEquals(HoldingIncome("K1", "S1", "Holding 1", 1200, -180, 90000, 0.0133), p.income.byHolding.first())
        assertNull(p.income.byHolding[1].value)
        val view = investIncomeView(p, ctx)!!
        // Every holding is there, largest first as the engine sends them; the screen shows ten and folds the rest.
        assertEquals(12, view.holdings.size)
        assertEquals(listOf("Holding 1", "+12.00", "S1 · Tax withheld -1.80"), view.holdings[0].let { listOf(it.title, it.value, it.subtitle) })
        // No tax withheld and no symbol leave nothing to say under the name; no yield is shown for a holding.
        assertEquals(listOf("Holding 2", "+11.00", null), view.holdings[1].let { listOf(it.title, it.value, it.subtitle) })
        assertEquals("S3", view.holdings[2].subtitle)
        assertEquals(12, view.holdings.map { it.key }.toSet().size)
        assertTrue(investIncomeView(portfolio(), ctx)!!.holdings.isEmpty())
    }

    /** What a sync on a build that names instruments and own-currency amounts brings: the same portfolio, with them. */
    private fun named(change: (JSONObject) -> Unit = {}) = portfolio { json ->
        val holdings = json.getJSONArray("holdings")
        holdings.getJSONObject(0).put("instrument", "Stock").put("amount", "900.00")
        holdings.getJSONObject(1).put("instrument", "ETF").put("amount", "18540.00")
        holdings.getJSONObject(3).put("instrument", "Put option").put("amount", "-20.00")
        val currencies = json.getJSONObject("allocation").getJSONArray("byCurrency")
        currencies.getJSONObject(0).put("amount", "1300.00")
        currencies.getJSONObject(1).put("amount", "18540.00")
        json.getJSONArray("accounts").getJSONObject(0).put("cash", JSONArray(listOf(
            JSONObject().put("currency", "USD").put("amount", "250.00").put("value", 25000),
            JSONObject().put("currency", "EUR").put("amount", "46.30").put("value", 5000))))
        json.getJSONArray("accounts").getJSONObject(1).put("cash", JSONArray(listOf(JSONObject().put("currency", "USD").put("amount", "20.00").put("value", 2000))))
        change(json)
    }

    @Test fun whatsHeldIsNamedWithItsOwnCurrency() {
        val p = named()
        assertEquals(listOf("Stock" to "900.00", "ETF" to "18540.00", "" to null, "Put option" to "-20.00"), p.holdings.map { it.instrument to it.amount })
        assertEquals(listOf("1300.00", "18540.00"), p.allocation.byCurrency.map { it.amount })
        assertNull(p.allocation.byKind.first().amount)
        assertEquals(listOf(CashAmount("USD", "250.00", 25000), CashAmount("EUR", "46.30", 5000)), p.accounts[0].cash)
        // Stored before the engine kept them: no instrument, no own amount, no cash by currency.
        val old = portfolio()
        assertTrue(old.holdings.all { it.instrument.isEmpty() && it.amount == null })
        assertTrue(old.allocation.byCurrency.all { it.amount == null } && old.accounts.all { it.cash.isEmpty() })

        // An own-currency amount keeps its currency's decimals as the engine gives them, grouped as the budget writes amounts.
        assertEquals("61,476.00 USD", HoldingsPage.amountIn("61476.00", "usd"))
        assertEquals("1.250 KWD", HoldingsPage.amountIn("1.250", "KWD"))
        assertEquals("150,000 JPY", HoldingsPage.amountIn("150000", "JPY"))
        assertEquals("-4,000.00 EUR", HoldingsPage.amountIn("-4000.00", "EUR"))
        // It's said only where it adds something: not in the budget's own currency, and not when it isn't known.
        assertNull(HoldingsPage.ownAmount("900.00", "USD", "USD"))
        assertNull(HoldingsPage.ownAmount(null, "EUR", "USD"))
        assertNull(HoldingsPage.ownAmount("5.00", "", "USD"))
        assertEquals("18,540.00 EUR", HoldingsPage.ownAmount("18540.00", "EUR", "USD"))

        // What it is: the engine's names, the kinds' singulars for the rest, and a stored holding's kind until the next sync.
        assertEquals(listOf("ETF", "Put option", "Government bond", "Stock", "Fund"), listOf("ETF", "Put option", "Government bond", "Stock", "Fund").map(HoldingsPage::instrumentName))
        assertEquals("ETF", HoldingsPage.instrument(JSONObject().put("instrument", "ETF").put("one", "Stock")))
        assertEquals("Stock", HoldingsPage.instrument(JSONObject().put("one", "Stock")))
        assertNull(HoldingsPage.instrument(JSONObject().put("instrument", JSONObject.NULL).put("one", "")))

        // Allocation: each holding's line, and each currency's own amount under it.
        val ctx = parseReportBudget(budget(), today)
        val allocation = allocationView(p, ctx, "kind", today)!!
        assertEquals(listOf("Stock · AAPL · Brokerage", "ETF · IWDA · 18,540.00 EUR · Fund platform, Brokerage", "Other investments · Brokerage",
            "Put option · Brokerage"), allocation.holdings.map { it.line })
        // A symbol is there when it adds to the title and is twelve characters or fewer, as on an account's own page.
        assertEquals("AAPL", HoldingsPage.shownSymbol("AAPL", "Apple"))
        assertEquals("T 4 02/15/34", HoldingsPage.shownSymbol("T 4 02/15/34", "Treasury note"))
        assertNull(HoldingsPage.shownSymbol("AAPL 261218C00250000", "AAPL 18DEC26 250 C"))
        assertNull(HoldingsPage.shownSymbol("BUND", "BUND"))
        assertNull(HoldingsPage.shownSymbol("", "Mystery"))
        assertEquals(listOf(null, "18,540.00 EUR"), allocationView(p, ctx, "currency", today)!!.bars.map { it.note })
        assertTrue(allocation.bars.all { it.note == null })
    }

    @Test fun theBalanceSheetEndingThisMonthListsWhatsHeld() {
        val ctx = parseReportBudget(budget(), today)
        val view = balanceSheetView(report, ctx, emptySet(), named(), today)
        fun row(key: String) = view.statement.first { it.key == key }
        fun section(name: String): List<FigureRow> {
            val from = view.statement.indexOfFirst { it.key == "s:Assets:$name" }
            return view.statement.drop(from + 1).takeWhile { it.style == RowStyle.Child || it.style == RowStyle.More }
        }
        // Each investment section lists the holdings themselves, largest first, with what they are, their symbol and their own-currency amount;
        // two accounts hold investments, so each line says which.
        assertEquals(listOf(Triple("Apple", "900.00", "Stock · AAPL · Brokerage")), section("Stocks").map { Triple(it.title, it.value, it.subtitle) })
        assertEquals(listOf(Triple("World fund", "200.00", "ETF · IWDA · 18,540.00 EUR · Fund platform, Brokerage")), section("Funds").map { Triple(it.title, it.value, it.subtitle) })
        assertEquals(listOf(Triple("SPY put", "-20.00", "Put option · Brokerage")), section("Options").map { Triple(it.title, it.value, it.subtitle) })
        // What the holdings don't explain stays one line for the account, and so does an account nothing breaks down.
        assertEquals(listOf("Mystery", "Brokerage · Since prices from 2026-10-20", "Pension"), section("Other investments").map { it.title })
        assertEquals("Brokerage", section("Other investments").first().subtitle)
        // A kind nothing is held of any more keeps the account's line, with its start.
        assertEquals(listOf("a:ib:Bonds"), section("Bonds").map { it.key })
        // The sections' figures and the totals are the accounts', as before.
        val past = balanceSheetView(report, ctx, emptySet(), named(), LocalDate.of(2026, 11, 20))
        assertEquals(past.statement.filter { it.style == RowStyle.Group || it.style == RowStyle.Total || it.style == RowStyle.Net }.map { it.title to it.value },
            view.statement.filter { it.style == RowStyle.Group || it.style == RowStyle.Total || it.style == RowStyle.Net }.map { it.title to it.value })
        assertEquals(past.tiles, view.tiles)
        // Cash at the broker says how much of each currency, then its figures; cash all in the budget's currency doesn't.
        assertEquals("250.00 USD · 46.30 EUR\nFrom 80.00 · Change 220.00 · 12.0%", row("a:ib:cash").subtitle)
        assertEquals("From 0.00 · Change 20.00 · 0.8%", row("a:fd:cash").subtitle)
        // A range that ended before this month keeps the accounts' lines.
        assertTrue(past.statement.none { it.key.startsWith("held:") })
        assertEquals(listOf("a:ib:Stocks"), past.statement.filter { it.key.startsWith("a:ib:Stocks") }.map { it.key })
        assertEquals("From 80.00 · Change 220.00 · 12.0%", past.statement.first { it.key == "a:ib:cash" }.subtitle)
        // Old data lists the holdings too, by their kind's singular, with no own-currency amounts.
        val old = balanceSheetView(report, ctx, emptySet(), portfolio(), today)
        assertEquals("Fund · IWDA · Fund platform, Brokerage", old.statement.first { it.title == "World fund" }.subtitle)
        assertEquals("From 80.00 · Change 220.00 · 12.0%", old.statement.first { it.key == "a:ib:cash" }.subtitle)
        // Holdings that don't come to what the accounts hold of their kind leave that section's account lines in place.
        val off = balanceSheetView(report, ctx, emptySet(), named { it.getJSONArray("holdings").getJSONObject(0).put("value", 80000) }, today)
        assertTrue(off.statement.any { it.key == "a:ib:Stocks" } && off.statement.none { it.title == "Apple" })
        assertTrue(off.statement.any { it.title == "World fund" })
    }

    @Test fun aSectionOfManyHoldingsFoldsToSix() {
        val ctx = parseReportBudget(budget(), today)
        val many = named { json ->
            json.put("holdings", JSONArray((1..10).map { n ->
                JSONObject().put("key", "K$n").put("symbol", "S$n").put("name", "Stock $n").put("group", "Stocks").put("one", "Stock").put("instrument", "Stock")
                    .put("currency", "USD").put("value", 9000).put("accounts", JSONArray(listOf("ib")))
            } + listOf(JSONObject().put("key", "M").put("symbol", "").put("name", "Mystery").put("group", "Holdings").put("currency", "USD").put("value", 5000)
                .put("accounts", JSONArray(listOf("ib"))))))
        }
        val view = balanceSheetView(report, ctx, emptySet(), many, today)
        val stocks = view.statement.filter { it.key.startsWith("held:Stocks:") }
        assertEquals(10, stocks.size)
        assertEquals(listOf(null, null, null, null, null, null, "sheet:Stocks", "sheet:Stocks", "sheet:Stocks", "sheet:Stocks"), stocks.map { it.fold })
        val more = view.statement.single { it.style == RowStyle.More }
        assertEquals(Triple("more:Stocks", "Show all 10", "sheet:Stocks"), Triple(more.key, more.title, more.expand))
        // Folded, the statement shows six of them and the row that opens the rest; opened out, all ten.
        assertEquals(6, shownRows(view.statement) { false }.count { it.key.startsWith("held:Stocks:") })
        assertEquals(10, shownRows(view.statement) { it == "sheet:Stocks" }.count { it.key.startsWith("held:Stocks:") })
        assertEquals(view.statement.size - 4, shownRows(view.statement) { false }.size)
        // Fewer than nine don't fold.
        assertTrue(balanceSheetView(report, ctx, emptySet(), named(), today).statement.none { it.fold != null || it.style == RowStyle.More })
    }

    @Test fun theOverviewHasAnInvestmentsCard() {
        val ctx = parseReportBudget(budget(), today)
        val ahead = ForecastData(mapOf("chk" to 60000L), emptyList(), 0)
        // Six cards with holdings, Investments before the forecast; five without.
        val model = overviewModel(report, ctx, portfolio(), ahead, today)!!
        assertEquals(listOf("spending", "income", "networth", "balance", "investments", "forecast"), model.cards.map { it.report })
        assertEquals(listOf("spending", "income", "networth", "balance", "forecast"), overviewModel(report, ctx, null, ahead, today)!!.cards.map { it.report })
        val card = model.cards[4]
        // Value now, with the gain as the note, over the value by month.
        assertEquals("1,500.00" to "Gain +200.00 · +16.0%", card.figure to card.note)
        assertEquals(listOf("1,100.00", "1,350.00", "1,500.00"), card.bars!!.texts[0])
        assertNull(investmentsCard(portfolio { it.put("accounts", JSONArray()) }, ctx))
    }
}
