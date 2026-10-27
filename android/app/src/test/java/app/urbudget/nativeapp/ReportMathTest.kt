package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.time.LocalDate
import java.time.YearMonth

class ReportMathTest {
    private val today = LocalDate.of(2026, 10, 21)

    @Before fun english() = EnglishWords.install()

    @Before fun plainAmounts() {
        Money.configure(JSONObject().put("currency", "USD").put("dateFormat", "iso")
            .put("numberFormat", JSONObject().put("decimal", ".").put("group", ",").put("symbol", "none")))
    }

    @After fun resetFormat() { Money.configure(JSONObject().put("currency", "SAR")) }

    private fun account(id: String, name: String, kind: String, type: String = "", archived: Boolean = false, netWorth: Boolean = true) =
        JSONObject().put("id", id).put("name", name).put("kind", kind).put("accountType", type).put("archived", archived).put("netWorth", netWorth).put("openingBalance", 0)

    private fun split(amount: Long, category: String? = null, unassigned: Boolean? = null) = JSONObject().put("amount", amount)
        .put("categoryId", category ?: JSONObject.NULL).also { if (unassigned != null) it.put("unassigned", unassigned) }

    private fun transaction(id: String, date: String, accountId: String, payee: String, vararg splits: JSONObject, transferTo: String? = null) =
        JSONObject().put("id", id).put("date", date).put("accountId", accountId).put("payee", payee).put("splits", JSONArray(splits.toList()))
            .also { if (transferTo != null) it.put("transferAccountId", transferTo) }

    private fun budget(categories: List<Triple<String, String, String>> = emptyList(), accounts: List<JSONObject> = emptyList(),
                       transactions: List<JSONObject> = emptyList(), groupOrder: List<String> = emptyList()) =
        JSONObject().put("currency", "USD")
            .put("categories", JSONArray(categories.map { (id, name, group) -> JSONObject().put("id", id).put("name", name).put("group", group) }))
            .put("accounts", JSONArray(accounts)).put("transactions", JSONArray(transactions)).put("groupOrder", JSONArray(groupOrder))

    private fun month(m: String, income: Long = 0, spending: Long = 0, categories: Map<String, Long> = emptyMap(), accounts: Map<String, Long> = emptyMap(),
                      assets: Long = 0, debt: Long = 0, sources: Map<String, Long> = emptyMap()) =
        ReportMonth(m, income, spending, income - spending, assets, debt, assets - debt, 0, 0, categories, sources, accounts)

    private fun data(months: List<ReportMonth>, opening: Map<String, Long>? = null, days: Map<String, Long> = emptyMap(), age: Long? = null) = ReportData(
        months, months.flatMap { it.categories.entries }.groupBy({ it.key }, { it.value }).mapValues { it.value.sum() },
        months.flatMap { it.incomeSources.entries }.groupBy({ it.key }, { it.value }).mapValues { it.value.sum() }, emptyList(), days,
        months.sumOf { it.income }, months.sumOf { it.spending }, null, age, opening, months.associate { it.month to null })

    @Test fun presetsCoverTheDesktopsMonths() {
        val now = YearMonth.of(2026, 10)
        assertEquals(MonthRange(RangePreset.ThisMonth, "2026-10", "2026-10"), rangeFor(RangePreset.ThisMonth, now, ""))
        assertEquals(MonthRange(RangePreset.LastMonth, "2026-09", "2026-09"), rangeFor(RangePreset.LastMonth, now, ""))
        assertEquals(MonthRange(RangePreset.Last3, "2026-08", "2026-10"), rangeFor(RangePreset.Last3, now, ""))
        assertEquals(MonthRange(RangePreset.Last6, "2026-05", "2026-10"), rangeFor(RangePreset.Last6, now, ""))
        assertEquals(MonthRange(RangePreset.Last12, "2025-11", "2026-10"), rangeFor(RangePreset.Last12, now, ""))
        assertEquals(MonthRange(RangePreset.Ytd, "2026-01", "2026-10"), rangeFor(RangePreset.Ytd, now, ""))
        assertEquals(MonthRange(RangePreset.LastYear, "2025-01", "2025-12"), rangeFor(RangePreset.LastYear, now, ""))
        assertEquals(MonthRange(RangePreset.All, "2024-03", "2026-10"), rangeFor(RangePreset.All, now, "2024-03-15"))
        assertEquals(MonthRange(RangePreset.Custom, "2026-01", "2026-09"), customRange("2026-09", "2026-01"))
        assertEquals(rangeFor(RangePreset.Last12, now, ""), defaultRange("2026-10", now))
        assertEquals(MonthRange(RangePreset.Custom, "2025-04", "2026-03"), defaultRange("2026-03", now))
        assertEquals("Last 6 months", describeRange(rangeFor(RangePreset.Last6, now, "")))
        assertEquals("Apr 2025 – Mar 2026", describeRange(customRange("2025-04", "2026-03")))
    }

    @Test fun earliestMonthIsTheFirstTransactionOrThisMonth() {
        val ctx = parseReportBudget(budget(transactions = listOf(transaction("t", "2024-02-29", "a", "x", split(1)))), today)
        assertEquals("2024-02", ctx.earliestMonth)
        assertEquals("2026-10", parseReportBudget(budget(), today).earliestMonth)
    }

    @Test fun payeesFoldIntoTilesTheWayTheDesktopDoes() {
        val same = List(20) { 100L }
        assertEquals(14, foldPayees(same) { it }.first.size)
        val eightAndTwoSmall = List(8) { 1000L } + listOf(10L, 10L)
        assertEquals(8 to 2, foldPayees(eightAndTwoSmall) { it }.let { it.first.size to it.second.size })
        val eightAndOneSmall = List(8) { 1000L } + listOf(10L)
        assertEquals(9 to 0, foldPayees(eightAndOneSmall) { it }.let { it.first.size to it.second.size })
        assertEquals(3, foldPayees(listOf(5L, 3L, 1L)) { it }.first.size)
    }

    @Test fun trendComparesTheLaterHalfWithTheEarlier() {
        val up = trendTile(listOf(100, 100, 200, 200))
        assertEquals("+100%", up.value)
        assertEquals(Tone.Negative, up.tone)
        assertEquals("last 2 months against the 2 before", up.note)
        val down = trendTile(listOf(300, 100))
        assertEquals("-67%", down.value)
        assertEquals(Tone.Positive, down.tone)
        assertEquals("+100%", trendTile(listOf(100, 999, 200)).value)
        val single = trendTile(listOf(500))
        assertEquals("—", single.value)
        assertEquals("needs two months or more", single.note)
        assertEquals("—", trendTile(listOf(0, 0, 50, 50)).value)
    }

    @Test fun changesSetTheLastFullMonthAgainstTheAverageBefore() {
        val ctx = parseReportBudget(budget(listOf(Triple("c1", "Groceries", "Food"), Triple("c2", "Takeaway", "Food"), Triple("c3", "Rent", "Bills")),
            groupOrder = listOf("Bills", "Food")), today)
        assertEquals(listOf("Bills", "Food", NEEDS_CATEGORY), ctx.spendingGroups)
        val report = data(listOf(
            month("2026-07", categories = mapOf("c1" to 100L, "c3" to 300L)),
            month("2026-08", categories = mapOf("c1" to 200L, "c3" to 300L)),
            month("2026-09", categories = mapOf("c1" to 600L, "c2" to 50L, "c3" to 300L)),
            month("2026-10", categories = mapOf("c1" to 10L)),
        ))
        val groups = changesView(report, ctx, null, "2026-10")
        assertEquals("September 2026 against your average", groups.title)
        assertEquals("the 2-month average before it", groups.aside)
        assertEquals(listOf("Food"), groups.rows.map { it.label })
        assertEquals("+5.00", groups.rows[0].valueText)
        assertEquals("avg 1.50", groups.rows[0].detail)
        assertEquals(ReportColors.DIVERGING_NEGATIVE, groups.rows[0].color)
        val food = changesView(report, ctx, "Food", "2026-10")
        assertEquals(listOf("Groceries" to "+4.50", "Takeaway" to "+0.50"), food.rows.map { it.label to it.valueText })
        val partial = changesView(data(listOf(month("2026-09", categories = mapOf("c1" to 100L)), month("2026-10", categories = mapOf("c1" to 40L)))), ctx, null, "2026-10")
        assertEquals("October 2026 so far against your average", partial.title)
        assertEquals("-0.60", partial.rows.single().valueText)
        assertEquals(ReportColors.DIVERGING_POSITIVE, partial.rows.single().color)
    }

    @Test fun groupsPastTheSeventhShareTheOtherColour() {
        val categories = (1..8).map { Triple("c$it", "Cat $it", "G$it") }
        val ctx = parseReportBudget(budget(categories), today)
        assertEquals(6, ctx.colorOf("G7"))
        assertEquals(ReportColors.OTHER, ctx.colorOf("G8"))
        val view = categoryView(data(listOf(month("2026-09", spending = 1500, categories = mapOf("c8" to 1000L, "c1" to 500L)))), ctx, null, today)
        assertEquals(listOf("G8", "G1"), view.rows.map { it.label })
        assertEquals(listOf("G1", "Other"), view.slices.map { it.label })
        assertEquals("G8", view.tiles[2].value)
        assertEquals("10.00 · 66.7%", view.tiles[2].note)
        assertEquals("over 1 month", view.tiles[0].note)
        val trends = overTimeView(data(listOf(month("2026-09", spending = 1500, categories = mapOf("c8" to 1000L, "c1" to 500L)))), ctx, byGroup = true)
        assertEquals(listOf("G1", "Other"), trends.chart.series.map { it.label })
    }

    @Test fun calendarCountsDaysUpToTodayAndAveragesWeekdays() {
        val days = calendarDays("2026-10", "2026-10", LocalDate.of(2026, 10, 5))
        assertEquals(listOf("2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05"), days)
        val values = mapOf("2026-10-01" to 1000L, "2026-10-03" to 500L, "2026-10-05" to 301L)
        assertEquals(listOf(301L, 0, 0, 1000, 0, 500, 0), weekdayAverages(days, values, 1))
        assertEquals(listOf(0L, 301, 0, 0, 1000, 0, 500), weekdayAverages(days, values, 0))
        val ctx = parseReportBudget(budget(), today)
        val view = calendarView(data(listOf(month("2026-10")), days = values), ctx, ReportScope("2026-10", "2026-10", emptySet(), false, LocalDate.of(2026, 10, 5), 1))
        assertEquals(listOf("3.60", "2", "10.00", "Thursday"), view.tiles.map { it.value })
        assertEquals("of 5 days · 40.0%", view.tiles[1].note)
        assertEquals("2026-10-01", view.tiles[2].note)
        assertEquals("10.00 on average", view.tiles[3].note)
        val heat = view.heat!!
        assertEquals(3, heat.lead)
        assertEquals(2, heat.weeks)
        assertEquals(listOf(4, 0, 2, 0, 1), heat.levels)
        assertEquals(listOf("Tue", "Thu", "Sat"), heat.weekdayLabels)
        assertEquals(listOf("2026-10-01", "2026-10-03", "2026-10-05"), view.biggest.map { it.key })
        assertEquals(DrillTarget("Thursday 2026-10-01", "day", "2026-10-01", "2026-10-01", "2026-10-01"), view.biggest[0].drill)
    }

    @Test fun balanceSheetSidesFollowTheBalance() {
        val ctx = parseReportBudget(budget(accounts = listOf(
            account("a1", "Checking", "cash", "checking"), account("a2", "Savings", "cash", "savings"), account("a3", "Visa", "credit"),
            account("a4", "Amex", "credit"), account("a5", "Invest", "tracking", "investment", netWorth = false),
            account("a6", "Old", "tracking", archived = true), account("a7", "Spare", "tracking"),
        )), today)
        val last = mapOf("a1" to 5000L, "a2" to -200L, "a3" to -1500L, "a4" to 300L, "a5" to 10000L, "a6" to 0L, "a7" to 0L)
        val opening = mapOf("a1" to 4000L, "a2" to 100L, "a3" to -1000L, "a4" to 0L, "a5" to 9000L, "a6" to 0L, "a7" to 0L)
        val report = data(listOf(month("2026-10", spending = 2000, accounts = last)), opening = opening)
        val lines = balanceSheetLines(report, ctx, emptySet())
        assertEquals(listOf("a1" to "Cash and bank", "a2" to "Overdrawn accounts", "a3" to "Credit cards", "a4" to "Accounts in credit",
            "a5" to "Investments", "a7" to "Other assets"), lines.map { it.account.id to it.section })
        assertEquals(SheetLine(lines[1].account, "Overdrawn accounts", true, -100, 200), lines[1])
        val view = balanceSheetView(report, ctx, emptySet())
        assertEquals(listOf("53.00", "17.00", "36.00", "32.1%", "50.00"), view.tiles.map { it.value })
        assertEquals("+5.00 in this period", view.tiles[2].note)
        assertEquals("2.5 months of spending", view.tiles[4].note)
        assertEquals(listOf("Cash and bank", "Accounts in credit"), view.own.map { it.label })
        assertEquals(listOf("94.3%", "5.7%"), view.own.map { it.detail })
        assertEquals(listOf("Credit cards", "Overdrawn accounts"), view.owe.map { it.label })
        assertTrue(view.statement.any { it.title == "Invest · investments" })
        assertTrue(view.help.endsWith("whatever their type."))
    }

    @Test fun forecastDropsTransfersInsideTheChosenAccounts() {
        val data = ForecastData(mapOf("b1" to 100000L, "b2" to 50000L, "b3" to 20000L), listOf(
            ForecastEvent("2026-10-22", -3000, "Rent", "b1", null, "c1", "s1"),
            ForecastEvent("2026-10-23", -1000, "Shop", "b3", null, "c2", null),
            ForecastEvent("2026-10-25", -5000, "Move", "b1", "b2", null, "s2"),
            ForecastEvent("2026-10-25", 5000, "Move", "b2", "b1", null, "s2"),
            ForecastEvent("2026-11-01", 200000, "Salary", "b1", null, null, "s3"),
        ), 30000)
        val plan = forecastPlan(data, setOf("b1", "b2"), 3, today, today.plusMonths(3))
        assertEquals(listOf("Rent", "Salary"), plan.events.map { it.payee })
        assertEquals(150000L, plan.opening)
        assertEquals(1000.0, plan.scheduledSpending, 0.0)
        assertEquals(29000L, plan.suggested)
        assertEquals(93, plan.days.size)
        assertEquals(listOf(150000L, 147000L), plan.knownLine.take(2))
        assertEquals(347000L, plan.knownLine.last())
        assertEquals(listOf(147000L, 347000L), plan.balances)
        val line = forecastLine(plan.knownLine, 36500)
        assertEquals(listOf(150000L, 145800L), line.take(2))
        assertEquals(236600L, line.last())
        val all = forecastPlan(data, emptySet(), 3, today, today.plusMonths(3))
        assertEquals(listOf("Rent", "Shop", "Salary"), all.events.map { it.payee })
        val view = forecastView(data, parseReportBudget(budget(), today), setOf("b1", "b2"), 3, null, today)
        assertEquals("1,500.00", view.tiles[0].value)
        assertEquals("in 2 budget accounts", view.tiles[0].note)
        assertEquals("290.00", view.everydayText)
        assertNull(view.suggestion)
        assertEquals("Use the suggested 290.00", forecastView(data, parseReportBudget(budget(), today), setOf("b1", "b2"), 3, 100, today).suggestion)
    }

    @Test fun incomeAgainstExpenseMatchesTheDesktopTiles() {
        val ctx = parseReportBudget(budget(listOf(Triple("c1", "Groceries", "Food"))), today)
        val view = incomeExpenseView(data(listOf(
            month("2026-09", income = 100000, spending = 80000, categories = mapOf("c1" to 80000L), sources = mapOf("Employer" to 100000L)),
            month("2026-10", spending = 30000, categories = mapOf("c1" to 30000L)),
        )), ctx, someCategories = false)
        assertEquals(listOf("1,000.00", "1,100.00", "-100.00", "-10.0%", "1 of 2"), view.tiles.map { it.value })
        assertEquals("500.00 a month", view.tiles[0].note)
        assertEquals("-50.00 a month", view.tiles[2].note)
        assertEquals(Tone.Negative, view.tiles[2].tone)
        assertEquals("best: September 2026", view.tiles[4].note)
        assertEquals(listOf(20.0, null), view.rate.series.single().values)
        assertEquals(listOf(20000.0, -10000.0), view.cumulative.series.single().values)
        assertEquals("Income 1,000.00 · Spending 800.00 · Rate 20%", view.months[0].subtitle)
        assertEquals("Income 0.00 · Spending 300.00 · Rate —", view.months[1].subtitle)
        assertEquals(Tone.Negative, view.months[1].tone)
        assertEquals(listOf("Income", "Employer", "Total income", "Spending", "Food", "Total spending", "Budget surplus", "Budget surplus rate"), view.table.map { it.title })
        assertEquals(listOf("Groceries"), view.children["Food"]?.map { it.title })
    }

    @Test fun axisLabelsAreShort() {
        assertEquals("0", compactMoney(0.0, "."))
        assertEquals("950", compactMoney(95000.0, "."))
        assertEquals("1000", compactMoney(99999.0, "."))
        assertEquals("1K", compactMoney(100000.0, "."))
        assertEquals("1.5K", compactMoney(150000.0, "."))
        assertEquals("12K", compactMoney(1200000.0, "."))
        assertEquals("1.2M", compactMoney(120000000.0, "."))
        assertEquals("15M", compactMoney(1500000000.0, "."))
        assertEquals("-1.5K", compactMoney(-150000.0, "."))
        assertEquals("1,5K", compactMoney(150000.0, ","))
        assertEquals(XTick("Oct", "Oct 26", 2026), monthXTick("2026-10"))
        assertEquals("Sep", monthAbbrev("2026-09-01"))
        assertEquals("33.3%", share(1, 3))
        assertEquals("—", share(5, 0))
        assertEquals("-25.0%", share(-1, 4))
        assertEquals("12.3", jsNumber(12.3))
        assertEquals("20", jsNumber(20.0))
    }

    @Test fun ticksAreRoundAndIncludeZero() {
        assertEquals(listOf(0.0, 250.0, 500.0, 750.0, 1000.0, 1250.0), niceTicks(0.0, 1234.0))
        assertEquals(listOf(-500.0, -250.0, 0.0, 250.0, 500.0, 750.0, 1000.0), niceTicks(-300.0, 900.0))
        assertEquals(listOf(0.0, 1.0), niceTicks(0.0, 0.0))
        assertEquals(listOf(0.0, 0.2, 0.4, 0.6, 0.8, 1.0), niceTicks(0.0, 1.0))
    }

    @Test fun heatLevelsAreQuintiles() {
        val scale = heatLevels(listOf(0L, 10, 20, 30, 40, 50))
        assertEquals(listOf(0, 1, 1, 2, 4, 5), listOf(0L, 10, 20, 25, 50, 60).map(scale::level))
        assertEquals(0, heatLevels(listOf(0L, -5)).level(100))
    }

    @Test fun squarifiedTilesFillTheBox() {
        val values = listOf(6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0)
        val tiles = squarify(values, { it }, 6.0, 4.0)
        assertEquals(values.size, tiles.size)
        tiles.forEach { t ->
            assertEquals(t.item, t.w * t.h, 1e-9)
            assertTrue(t.x >= -1e-9 && t.y >= -1e-9 && t.x + t.w <= 6 + 1e-9 && t.y + t.h <= 4 + 1e-9)
        }
        assertEquals(24.0, tiles.sumOf { it.w * it.h }, 1e-9)
        assertTrue(squarify(values, { it }, 0.0, 4.0).isEmpty())
    }

    @Test fun filtersReadAsAllNoneOneOrACount() {
        val groups = listOf(FilterGroup("Budget", listOf(FilterItem("a", "Checking"), FilterItem("b", "Savings"))), FilterGroup("Tracking", listOf(FilterItem("c", "House"))))
        assertEquals("All accounts", filterSummary(groups, emptySet(), "All accounts", "accounts"))
        assertEquals("Checking", filterSummary(groups, setOf("a"), "All accounts", "accounts"))
        assertEquals("2 accounts", filterSummary(groups, setOf("a", "c"), "All accounts", "accounts"))
        assertEquals("None", filterSummary(groups, setOf(""), "All accounts", "accounts"))
        assertEquals(setOf("b", "c"), filterToggle(groups, emptySet(), listOf("a"), false))
        assertEquals(emptySet<String>(), filterToggle(groups, setOf("b", "c"), listOf("a"), true))
        assertEquals(setOf(""), filterToggle(groups, setOf("a"), listOf("a"), false))
        assertEquals(setOf("c"), filterToggle(groups, emptySet(), listOf("a", "b"), false))
        assertTrue(filterIsOn(emptySet(), "x"))
        assertTrue(!filterIsOn(setOf(""), "a"))
    }

    @Test fun accountTypesFallBackLikeTheDesktop() {
        assertEquals("checking", accountType("", "", "cash", 0))
        assertEquals("creditCard", accountType("", "", "credit", 0))
        assertEquals("otherDebt", accountType("", "otherLiability", "tracking", 5))
        assertEquals("otherDebt", accountType("", "", "tracking", -5))
        assertEquals("otherAsset", accountType("", "", "tracking", 5))
        assertEquals("mortgage", accountType("mortgage", "investment", "tracking", 5))
        assertEquals("Loans", sectionOf("autoLoan"))
        assertEquals("Other assets", sectionOf("somethingElse"))
    }

    @Test fun reportsParseInEngineOrder() {
        val json = JSONObject().put("months", JSONArray().put(JSONObject().put("month", "2026-10").put("income", 5).put("spending", 3)
            .put("categories", JSONObject().put("b", 2).put("a", 1))))
            .put("payees", JSONObject().put("B", JSONObject().put("amount", 100).put("count", 2)).put("A", JSONObject().put("amount", 300).put("count", 1)))
            .put("ageOfMoney", JSONObject.NULL).put("ageOfMoneyByMonth", JSONObject().put("2026-10", JSONObject.NULL).put("2026-09", 12))
            .put("transactionCount", 3).put("income", 5).put("spending", 3)
        val report = parseReport(json)
        assertEquals(listOf("A", "B"), report.payees.map { it.name })
        assertEquals(listOf("a", "b"), report.months.single().categories.keys.toList())
        assertNull(report.ageOfMoney)
        assertEquals(mapOf("2026-09" to 12L, "2026-10" to null), report.ageOfMoneyByMonth)
        assertEquals(3L, report.transactionCount)
        assertNull(report.openingAccounts)
    }

    @Test fun incomeLinesFollowTheBudgetSideOfTransfers() {
        val budget = budget(listOf(Triple("c1", "Food", "Everyday")), listOf(account("chk", "Checking", "cash"), account("inv", "Investments", "tracking")), listOf(
            transaction("t1", "2026-10-05", "chk", "Employer", split(500000, unassigned = true)),
            transaction("t2", "2026-10-10", "chk", "Employer", split(100000)),
            transaction("t3", "2026-10-12", "chk", "Employer", split(-2000, "c1")),
            transaction("t4", "2026-11-01", "chk", "Employer", split(1000, unassigned = true)),
            transaction("t5", "2026-10-15", "inv", "Employer", split(3000)),
            transaction("t6", "2026-10-20", "inv", "Employer", split(-7000), transferTo = "chk"),
            transaction("t7", "2026-10-21", "chk", "Employer", split(-400)),
        ))
        val ctx = parseReportBudget(budget, today)
        val model = incomeDrill(budget, ctx, "Employer", "2026-10", "2026-10", emptySet())
        assertEquals(listOf("t6", "t2", "t1"), model.lines.map { it.transactionId })
        assertEquals("Transfer: Investments", model.lines[0].payee)
        assertEquals("70.00", model.lines[0].amount)
        assertEquals("Received", model.figureLabel)
        assertEquals("6,070.00", model.figure)
        assertEquals("3", model.count)
        assertEquals("2,023.33", model.average)
        assertEquals("2026-10-01 – 2026-10-31", model.range)
        val chosen = incomeDrill(budget, ctx, "Employer", "2026-10", "2026-10", setOf("inv"))
        assertTrue(chosen.lines.isEmpty())
        assertEquals("—", chosen.average)
        assertTrue(chosen.range.endsWith(" · chosen accounts"))

        val details = drillFromDetails(listOf(JSONObject().put("transactionId", "t3").put("date", "2026-10-12").put("accountId", "chk")
            .put("categoryId", "c1").put("payee", "Employer").put("memo", "").put("amount", 2000)), budget, ctx, "2026-10-01", "2026-10-31", emptySet())
        assertEquals("-20.00", details.lines.single().amount)
        assertEquals("2026-10-12 · Checking · Food", details.lines.single().detail)
        assertEquals("Spent", details.figureLabel)
        assertEquals("20.00", details.figure)
        assertEquals("t3", details.records.keys.single())
    }

    @Test fun overviewCardsReadLikeThePhonesReports() {
        val ctx = parseReportBudget(budget(listOf(Triple("c1", "Groceries", "Food"), Triple("c2", "Fuel", "Car"), Triple("c3", "Gifts", "Fun"))), today)
        val model = overviewModel(data(listOf(
            month("2026-09", income = 100, spending = 100),
            month("2026-10", income = 100, spending = 9000, categories = mapOf("c1" to 5000L, "c2" to 3000L, "c3" to -1000L, UNCATEGORIZED_BUCKET to 2000L),
                assets = 1000, debt = 500),
        ), age = 1), ctx)!!
        assertEquals("October 2026", model.monthName)
        assertEquals(listOf("Groceries", "Fuel", NEEDS_CATEGORY), model.rows.map { it.label })
        assertEquals(listOf(0, 1, 2), model.rows.map { it.color })
        assertEquals("Includes 10.00 in net refunds. The bar shows categories with net spending.", model.refundLead)
        assertEquals(listOf("Gifts" to "-10.00"), model.refunds.map { it.label to it.valueText })
        assertEquals("On average, you're spending more than you make.", model.incomeLead)
        assertEquals("-5.00", model.debtsText)
        assertTrue(model.debtsNegative)
        assertEquals("1 day", model.ageText)
        assertEquals("Nothing earned or spent in these months.", incomeLead(listOf(month("2026-10"))))
        assertEquals("On average, you're spending about what you make.", incomeLead(listOf(month("2026-10", income = 1000, spending = 990))))
        assertEquals("On average, you're spending less than you make.", incomeLead(listOf(month("2026-10", income = 1000, spending = 500))))
        val many = overviewModel(data(listOf(month("2026-10", categories = (1..7).associate { "x$it" to it * 100L }))), ctx)!!
        assertEquals(listOf("Unassigned", "Unassigned", "Unassigned", "Unassigned", "Unassigned", "All others"), many.rows.map { it.label })
        assertEquals("3.00", many.rows.last().valueText)
        assertEquals(ReportColors.OTHER, many.rows.last().color)
    }

    @Test fun chartAccountsTakeTheFirstFreeSlot() {
        val ctx = parseReportBudget(budget(accounts = (1..9).map { account("a$it", "A$it", "cash") }), today)
        val report = data(listOf(month("2026-10", accounts = (1..9).associate { "a$it" to it * 100L })))
        val eligible = balanceAccounts(report, ctx, emptySet())
        val chosen = defaultChartAccounts(report, eligible)
        assertEquals(mapOf("a5" to 0, "a6" to 1, "a7" to 2, "a8" to 3, "a9" to 4), chosen)
        val fewer = toggleChartAccount(chosen, eligible, "a6")
        assertEquals(mapOf("a5" to 0, "a7" to 2, "a8" to 3, "a9" to 4, "a1" to 1), toggleChartAccount(fewer, eligible, "a1"))
        val full = (1..8).associate { "a$it" to it - 1 }
        assertEquals(full, toggleChartAccount(full, eligible, "a9"))
        val view = balancesView(report, ctx, emptySet(), null)
        assertEquals("5 of 9", view.tiles[0].value)
        assertEquals(5, view.chart?.series?.size)
    }
}
