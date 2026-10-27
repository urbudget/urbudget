import XCTest
@testable import UrBudget

/// Android's ReportMathTest and ReportScopeTest, against the Swift port of the report presentation maths.
final class ReportsTests: XCTestCase {
    private let today = "2026-10-21"

    override func setUp() {
        super.setUp()
        Money.configure(budget: ["currency": "USD", "dateFormat": "iso", "numberFormat": ["decimal": ".", "group": ",", "symbol": "none"]])
    }

    override func tearDown() {
        Money.configure(budget: ["currency": "SAR"])
        super.tearDown()
    }

    // MARK: Fixtures

    private func account(_ id: String, _ name: String, _ kind: String, type: String = "", archived: Bool = false, netWorth: Bool = true) -> JSON {
        ["id": .string(id), "name": .string(name), "kind": .string(kind), "accountType": .string(type), "archived": .bool(archived),
         "netWorth": .bool(netWorth), "openingBalance": 0]
    }

    private func split(_ amount: Int64, _ category: String? = nil, unassigned: Bool? = nil) -> JSON {
        var split: JSON = ["amount": .int(amount), "categoryId": category.map(JSON.string) ?? .null]
        if let unassigned { split["unassigned"] = .bool(unassigned) }
        return split
    }

    private func transaction(_ id: String, _ date: String, _ accountId: String, _ payee: String, _ splits: JSON..., transferTo: String? = nil) -> JSON {
        var transaction: JSON = ["id": .string(id), "date": .string(date), "accountId": .string(accountId), "payee": .string(payee), "splits": .array(splits)]
        if let transferTo { transaction["transferAccountId"] = .string(transferTo) }
        return transaction
    }

    private func budget(_ categories: [(String, String, String)] = [], accounts: [JSON] = [], transactions: [JSON] = [], groupOrder: [String] = []) -> JSON {
        ["currency": "USD",
         "categories": .array(categories.map { ["id": .string($0.0), "name": .string($0.1), "group": .string($0.2)] }),
         "accounts": .array(accounts), "transactions": .array(transactions), "groupOrder": JSON(groupOrder)]
    }

    private func month(_ m: String, income: Int64 = 0, spending: Int64 = 0, categories: [String: Int64] = [:], accounts: [String: Int64] = [:],
                       assets: Int64 = 0, debt: Int64 = 0, sources: [String: Int64] = [:]) -> ReportMonth {
        ReportMonth(month: m, income: income, spending: spending, netIncome: income - spending, assets: assets, debt: debt, netWorth: assets - debt,
                    inflow: 0, outflow: 0, categories: ReportAmounts(categories), incomeSources: ReportAmounts(sources), accounts: ReportAmounts(accounts))
    }

    private func data(_ months: [ReportMonth], opening: [String: Int64]? = nil, days: [String: Int64] = [:], age: Int64? = nil) -> ReportData {
        var categories: [String: Int64] = [:]
        var sources: [String: Int64] = [:]
        for m in months {
            for (key, value) in m.categories.entries { categories[key, default: 0] += value }
            for (key, value) in m.incomeSources.entries { sources[key, default: 0] += value }
        }
        var ages: [String: Int64?] = [:]
        for m in months { ages[m.month] = .some(nil) }
        return ReportData(months: months, categories: ReportAmounts(categories), incomeSources: ReportAmounts(sources), payees: [],
                          days: ReportAmounts(days), income: months.map(\.income).reduce(0, +), spending: months.map(\.spending).reduce(0, +),
                          transactionCount: nil, ageOfMoney: age, openingAccounts: opening.map { ReportAmounts($0) }, ageOfMoneyByMonth: ages)
    }

    // MARK: ReportMathTest

    func testPresetsCoverTheDesktopsMonths() {
        let now = "2026-10"
        XCTAssertEqual(ReportMath.rangeFor(.thisMonth, today: now, earliest: ""), ReportRange(preset: .thisMonth, from: "2026-10", to: "2026-10"))
        XCTAssertEqual(ReportMath.rangeFor(.lastMonth, today: now, earliest: ""), ReportRange(preset: .lastMonth, from: "2026-09", to: "2026-09"))
        XCTAssertEqual(ReportMath.rangeFor(.last3, today: now, earliest: ""), ReportRange(preset: .last3, from: "2026-08", to: "2026-10"))
        XCTAssertEqual(ReportMath.rangeFor(.last6, today: now, earliest: ""), ReportRange(preset: .last6, from: "2026-05", to: "2026-10"))
        XCTAssertEqual(ReportMath.rangeFor(.last12, today: now, earliest: ""), ReportRange(preset: .last12, from: "2025-11", to: "2026-10"))
        XCTAssertEqual(ReportMath.rangeFor(.ytd, today: now, earliest: ""), ReportRange(preset: .ytd, from: "2026-01", to: "2026-10"))
        XCTAssertEqual(ReportMath.rangeFor(.lastYear, today: now, earliest: ""), ReportRange(preset: .lastYear, from: "2025-01", to: "2025-12"))
        XCTAssertEqual(ReportMath.rangeFor(.all, today: now, earliest: "2024-03-15"), ReportRange(preset: .all, from: "2024-03", to: "2026-10"))
        XCTAssertEqual(ReportMath.customRange(from: "2026-09", to: "2026-01"), ReportRange(preset: .custom, from: "2026-01", to: "2026-09"))
        XCTAssertEqual(ReportMath.defaultRange(month: "2026-10", today: now), ReportMath.rangeFor(.last12, today: now, earliest: ""))
        XCTAssertEqual(ReportMath.defaultRange(month: "2026-03", today: now), ReportRange(preset: .custom, from: "2025-04", to: "2026-03"))
        XCTAssertEqual(ReportMath.describeRange(ReportMath.rangeFor(.last6, today: now, earliest: "")), "Last 6 months")
        XCTAssertEqual(ReportMath.describeRange(ReportMath.customRange(from: "2025-04", to: "2026-03")), "Apr 2025 – Mar 2026")
    }

    func testEarliestMonthIsTheFirstTransactionOrThisMonth() {
        let ctx = ReportMath.parseBudget(budget(transactions: [transaction("t", "2024-02-29", "a", "x", split(1))]), today: today)
        XCTAssertEqual(ctx.earliestMonth, "2024-02")
        XCTAssertEqual(ReportMath.parseBudget(budget(), today: today).earliestMonth, "2026-10")
    }

    func testPayeesFoldIntoTilesTheWayTheDesktopDoes() {
        let same = [Int64](repeating: 100, count: 20)
        XCTAssertEqual(ReportMath.foldPayees(same) { $0 }.big.count, 14)
        let eightAndTwoSmall = [Int64](repeating: 1000, count: 8) + [10, 10]
        let folded = ReportMath.foldPayees(eightAndTwoSmall) { $0 }
        XCTAssertEqual(folded.big.count, 8)
        XCTAssertEqual(folded.rest.count, 2)
        let eightAndOneSmall = [Int64](repeating: 1000, count: 8) + [10]
        let kept = ReportMath.foldPayees(eightAndOneSmall) { $0 }
        XCTAssertEqual(kept.big.count, 9)
        XCTAssertEqual(kept.rest.count, 0)
        XCTAssertEqual(ReportMath.foldPayees([5, 3, 1] as [Int64]) { $0 }.big.count, 3)
    }

    func testTrendComparesTheLaterHalfWithTheEarlier() {
        let up = ReportMath.trendTile([100, 100, 200, 200])
        XCTAssertEqual(up.value, "+100%")
        XCTAssertEqual(up.tone, .negative)
        XCTAssertEqual(up.note, "last 2 months against the 2 before")
        let down = ReportMath.trendTile([300, 100])
        XCTAssertEqual(down.value, "-67%")
        XCTAssertEqual(down.tone, .positive)
        XCTAssertEqual(ReportMath.trendTile([100, 999, 200]).value, "+100%")
        let single = ReportMath.trendTile([500])
        XCTAssertEqual(single.value, "—")
        XCTAssertEqual(single.note, "needs two months or more")
        XCTAssertEqual(ReportMath.trendTile([0, 0, 50, 50]).value, "—")
    }

    func testChangesSetTheLastFullMonthAgainstTheAverageBefore() {
        let ctx = ReportMath.parseBudget(budget([("c1", "Groceries", "Food"), ("c2", "Takeaway", "Food"), ("c3", "Rent", "Bills")],
                                                groupOrder: ["Bills", "Food"]), today: today)
        XCTAssertEqual(ctx.spendingGroups, ["Bills", "Food", ReportMath.needsCategory])
        let report = data([
            month("2026-07", categories: ["c1": 100, "c3": 300]),
            month("2026-08", categories: ["c1": 200, "c3": 300]),
            month("2026-09", categories: ["c1": 600, "c2": 50, "c3": 300]),
            month("2026-10", categories: ["c1": 10]),
        ])
        let groups = ReportMath.changesView(report, ctx, group: nil, todayMonth: "2026-10")
        XCTAssertEqual(groups.title, "September 2026 against your average")
        XCTAssertEqual(groups.aside, "the 2-month average before it")
        XCTAssertEqual(groups.rows.map(\.label), ["Food"])
        XCTAssertEqual(groups.rows[0].valueText, "+5.00")
        XCTAssertEqual(groups.rows[0].detail, "avg 1.50")
        XCTAssertEqual(groups.rows[0].color, ReportColors.divergingNegative)
        let food = ReportMath.changesView(report, ctx, group: "Food", todayMonth: "2026-10")
        XCTAssertEqual(food.rows.map(\.label), ["Groceries", "Takeaway"])
        XCTAssertEqual(food.rows.map(\.valueText), ["+4.50", "+0.50"])
        let partial = ReportMath.changesView(data([month("2026-09", categories: ["c1": 100]), month("2026-10", categories: ["c1": 40])]),
                                             ctx, group: nil, todayMonth: "2026-10")
        XCTAssertEqual(partial.title, "October 2026 so far against your average")
        XCTAssertEqual(partial.rows.map(\.valueText), ["-0.60"])
        XCTAssertEqual(partial.rows.first?.color, ReportColors.divergingPositive)
    }

    func testGroupsPastTheSeventhShareTheOtherColour() {
        let categories = (1...8).map { ("c\($0)", "Cat \($0)", "G\($0)") }
        let ctx = ReportMath.parseBudget(budget(categories), today: today)
        XCTAssertEqual(ctx.colorOf("G7"), 6)
        XCTAssertEqual(ctx.colorOf("G8"), ReportColors.other)
        let view = ReportMath.categoryView(data([month("2026-09", spending: 1500, categories: ["c8": 1000, "c1": 500])]), ctx, group: nil, today: today)
        XCTAssertEqual(view.rows.map(\.label), ["G8", "G1"])
        XCTAssertEqual(view.slices.map(\.label), ["G1", "Other"])
        XCTAssertEqual(view.tiles[2].value, "G8")
        XCTAssertEqual(view.tiles[2].note, "10.00 · 66.7%")
        XCTAssertEqual(view.tiles[0].note, "over 1 month")
        let trends = ReportMath.overTimeView(data([month("2026-09", spending: 1500, categories: ["c8": 1000, "c1": 500])]), ctx, byGroup: true)
        XCTAssertEqual(trends.chart.series.map(\.label), ["G1", "Other"])
    }

    func testCalendarCountsDaysUpToTodayAndAveragesWeekdays() {
        let days = ReportMath.calendarDays(from: "2026-10", to: "2026-10", today: "2026-10-05")
        XCTAssertEqual(days, ["2026-10-01", "2026-10-02", "2026-10-03", "2026-10-04", "2026-10-05"])
        let values = ReportAmounts(["2026-10-01": 1000, "2026-10-03": 500, "2026-10-05": 301])
        XCTAssertEqual(ReportMath.weekdayAverages(days, values, weekStart: 1), [301, 0, 0, 1000, 0, 500, 0])
        XCTAssertEqual(ReportMath.weekdayAverages(days, values, weekStart: 0), [0, 301, 0, 0, 1000, 0, 500])
        let ctx = ReportMath.parseBudget(budget(), today: today)
        let scope = ReportScope(from: "2026-10", to: "2026-10", accounts: [], someCategories: false, today: "2026-10-05", weekStart: 1)
        let view = ReportMath.calendarView(data([month("2026-10")], days: ["2026-10-01": 1000, "2026-10-03": 500, "2026-10-05": 301]), ctx, scope)
        XCTAssertEqual(view.tiles.map(\.value), ["3.60", "2", "10.00", "Thursday"])
        XCTAssertEqual(view.tiles[1].note, "of 5 days · 40.0%")
        XCTAssertEqual(view.tiles[2].note, "2026-10-01")
        XCTAssertEqual(view.tiles[3].note, "10.00 on average")
        let heat = try! XCTUnwrap(view.heat)
        XCTAssertEqual(heat.lead, 3)
        XCTAssertEqual(heat.weeks, 2)
        XCTAssertEqual(heat.levels, [4, 0, 2, 0, 1])
        XCTAssertEqual(heat.weekdayLabels, ["Tue", "Thu", "Sat"])
        XCTAssertEqual(view.biggest.map(\.key), ["2026-10-01", "2026-10-03", "2026-10-05"])
        XCTAssertEqual(view.biggest[0].drill, ReportDrillTarget(title: "Thursday 2026-10-01", dimension: "day", key: "2026-10-01", from: "2026-10-01", to: "2026-10-01"))
    }

    func testBalanceSheetSidesFollowTheBalance() {
        let ctx = ReportMath.parseBudget(budget(accounts: [
            account("a1", "Checking", "cash", type: "checking"), account("a2", "Savings", "cash", type: "savings"), account("a3", "Visa", "credit"),
            account("a4", "Amex", "credit"), account("a5", "Invest", "tracking", type: "investment", netWorth: false),
            account("a6", "Old", "tracking", archived: true), account("a7", "Spare", "tracking"),
        ]), today: today)
        let last: [String: Int64] = ["a1": 5000, "a2": -200, "a3": -1500, "a4": 300, "a5": 10000, "a6": 0, "a7": 0]
        let opening: [String: Int64] = ["a1": 4000, "a2": 100, "a3": -1000, "a4": 0, "a5": 9000, "a6": 0, "a7": 0]
        let report = data([month("2026-10", spending: 2000, accounts: last)], opening: opening)
        let lines = ReportMath.balanceSheetLines(report, ctx, accounts: [])
        XCTAssertEqual(lines.map(\.account.id), ["a1", "a2", "a3", "a4", "a5", "a7"])
        XCTAssertEqual(lines.map(\.section), ["Cash and bank", "Overdrawn accounts", "Credit cards", "Accounts in credit", "Investments", "Other assets"])
        XCTAssertEqual(lines[1], ReportSheetLine(account: lines[1].account, section: "Overdrawn accounts", debt: true, start: -100, end: 200))
        let view = ReportMath.balanceSheetView(report, ctx, accounts: [])
        XCTAssertEqual(view.tiles.map(\.value), ["53.00", "17.00", "36.00", "32.1%", "50.00"])
        XCTAssertEqual(view.tiles[2].note, "+5.00 in this period")
        XCTAssertEqual(view.tiles[4].note, "2.5 months of spending")
        XCTAssertEqual(view.own.map(\.label), ["Cash and bank", "Accounts in credit"])
        XCTAssertEqual(view.own.map(\.detail), ["94.3%", "5.7%"])
        XCTAssertEqual(view.owe.map(\.label), ["Credit cards", "Overdrawn accounts"])
        XCTAssertTrue(view.statement.contains { $0.title == "Invest · investments" })
        XCTAssertTrue(view.help.hasSuffix("whatever their type."))
    }

    func testForecastDropsTransfersInsideTheChosenAccounts() {
        let forecast = ReportForecastData(balances: ReportAmounts(["b1": 100_000, "b2": 50000, "b3": 20000]), events: [
            ReportForecastEvent(date: "2026-10-22", amount: -3000, payee: "Rent", accountId: "b1", peerId: nil, categoryId: "c1", scheduleId: "s1"),
            ReportForecastEvent(date: "2026-10-23", amount: -1000, payee: "Shop", accountId: "b3", peerId: nil, categoryId: "c2", scheduleId: nil),
            ReportForecastEvent(date: "2026-10-25", amount: -5000, payee: "Move", accountId: "b1", peerId: "b2", categoryId: nil, scheduleId: "s2"),
            ReportForecastEvent(date: "2026-10-25", amount: 5000, payee: "Move", accountId: "b2", peerId: "b1", categoryId: nil, scheduleId: "s2"),
            ReportForecastEvent(date: "2026-11-01", amount: 200_000, payee: "Salary", accountId: "b1", peerId: nil, categoryId: nil, scheduleId: "s3"),
        ], recentSpending: 30000)
        let through = ReportMath.addMonths(day: today, 3)
        XCTAssertEqual(through, "2027-01-21")
        let plan = ReportMath.forecastPlan(forecast, accounts: ["b1", "b2"], months: 3, start: today, through: through)
        XCTAssertEqual(plan.events.map(\.payee), ["Rent", "Salary"])
        XCTAssertEqual(plan.opening, 150_000)
        XCTAssertEqual(plan.scheduledSpending, 1000.0, accuracy: 0)
        XCTAssertEqual(plan.suggested, 29000)
        XCTAssertEqual(plan.days.count, 93)
        XCTAssertEqual(Array(plan.knownLine.prefix(2)), [150_000, 147_000])
        XCTAssertEqual(plan.knownLine.last, 347_000)
        XCTAssertEqual(plan.balances, [147_000, 347_000])
        let line = ReportMath.forecastLine(plan.knownLine, monthly: 36500)
        XCTAssertEqual(Array(line.prefix(2)), [150_000, 145_800])
        XCTAssertEqual(line.last, 236_600)
        let all = ReportMath.forecastPlan(forecast, accounts: [], months: 3, start: today, through: through)
        XCTAssertEqual(all.events.map(\.payee), ["Rent", "Shop", "Salary"])
        let ctx = ReportMath.parseBudget(budget(), today: today)
        let view = ReportMath.forecastView(forecast, ctx, accounts: ["b1", "b2"], months: 3, everyday: nil, start: today)
        XCTAssertEqual(view.tiles[0].value, "1,500.00")
        XCTAssertEqual(view.tiles[0].note, "in 2 budget accounts")
        XCTAssertEqual(view.everydayText, "290.00")
        XCTAssertNil(view.suggestion)
        XCTAssertEqual(ReportMath.forecastView(forecast, ctx, accounts: ["b1", "b2"], months: 3, everyday: 100, start: today).suggestion,
                       "Use the suggested 290.00")
    }

    func testIncomeAgainstExpenseMatchesTheDesktopTiles() {
        let ctx = ReportMath.parseBudget(budget([("c1", "Groceries", "Food")]), today: today)
        let view = ReportMath.incomeExpenseView(data([
            month("2026-09", income: 100_000, spending: 80000, categories: ["c1": 80000], sources: ["Employer": 100_000]),
            month("2026-10", spending: 30000, categories: ["c1": 30000]),
        ]), ctx, someCategories: false)
        XCTAssertEqual(view.tiles.map(\.value), ["1,000.00", "1,100.00", "-100.00", "-10.0%", "1 of 2"])
        XCTAssertEqual(view.tiles[0].note, "500.00 a month")
        XCTAssertEqual(view.tiles[2].note, "-50.00 a month")
        XCTAssertEqual(view.tiles[2].tone, .negative)
        XCTAssertEqual(view.tiles[4].note, "best: September 2026")
        XCTAssertEqual(view.rate.series.first?.values, [20.0, nil])
        XCTAssertEqual(view.cumulative.series.first?.values, [20000.0, -10000.0])
        XCTAssertEqual(view.months[0].subtitle, "Income 1,000.00 · Spending 800.00 · Rate 20%")
        XCTAssertEqual(view.months[1].subtitle, "Income 0.00 · Spending 300.00 · Rate —")
        XCTAssertEqual(view.months[1].tone, .negative)
        XCTAssertEqual(view.table.map(\.title),
                       ["Income", "Employer", "Total income", "Spending", "Food", "Total spending", "Budget surplus", "Budget surplus rate"])
        XCTAssertEqual(view.children["Food"]?.map(\.title), ["Groceries"])
    }

    func testAxisLabelsAreShort() {
        XCTAssertEqual(ReportMath.compactMoney(0, decimal: "."), "0")
        XCTAssertEqual(ReportMath.compactMoney(95000, decimal: "."), "950")
        XCTAssertEqual(ReportMath.compactMoney(99999, decimal: "."), "1000")
        XCTAssertEqual(ReportMath.compactMoney(100_000, decimal: "."), "1K")
        XCTAssertEqual(ReportMath.compactMoney(150_000, decimal: "."), "1.5K")
        XCTAssertEqual(ReportMath.compactMoney(1_200_000, decimal: "."), "12K")
        XCTAssertEqual(ReportMath.compactMoney(120_000_000, decimal: "."), "1.2M")
        XCTAssertEqual(ReportMath.compactMoney(1_500_000_000, decimal: "."), "15M")
        XCTAssertEqual(ReportMath.compactMoney(-150_000, decimal: "."), "-1.5K")
        XCTAssertEqual(ReportMath.compactMoney(150_000, decimal: ","), "1,5K")
        XCTAssertEqual(ReportMath.monthXTick("2026-10"), ReportXTick(short: "Oct", long: "Oct 26", year: 2026))
        XCTAssertEqual(ReportMath.monthAbbrev("2026-09-01"), "Sep")
        XCTAssertEqual(ReportMath.share(1, 3), "33.3%")
        XCTAssertEqual(ReportMath.share(5, 0), "—")
        XCTAssertEqual(ReportMath.share(-1, 4), "-25.0%")
        XCTAssertEqual(ReportMath.jsNumber(12.3), "12.3")
        XCTAssertEqual(ReportMath.jsNumber(20.0), "20")
    }

    func testFixedRoundsHalvesAwayFromZeroOnTheExactValue() {
        XCTAssertEqual(ReportMath.fixed(2.5, 0), "3")
        XCTAssertEqual(ReportMath.fixed(-2.5, 0), "-3")
        XCTAssertEqual(ReportMath.fixed(2.675, 2), "2.67")
        XCTAssertEqual(ReportMath.fixed(9.96, 1), "10.0")
        XCTAssertEqual(ReportMath.fixed(-0.04, 1), "-0.0")
        XCTAssertEqual(ReportMath.fixed(66.666_666, 1), "66.7")
    }

    func testTicksAreRoundAndIncludeZero() {
        XCTAssertEqual(ReportMath.niceTicks(0, 1234), [0, 250, 500, 750, 1000, 1250])
        XCTAssertEqual(ReportMath.niceTicks(-300, 900), [-500, -250, 0, 250, 500, 750, 1000])
        XCTAssertEqual(ReportMath.niceTicks(0, 0), [0, 1])
        XCTAssertEqual(ReportMath.niceTicks(0, 1), [0, 0.2, 0.4, 0.6, 0.8, 1.0])
    }

    func testHeatLevelsAreQuintiles() {
        let scale = ReportMath.heatLevels([0, 10, 20, 30, 40, 50])
        XCTAssertEqual([0, 10, 20, 25, 50, 60].map(scale.level), [0, 1, 1, 2, 4, 5])
        XCTAssertEqual(ReportMath.heatLevels([0, -5]).level(100), 0)
    }

    func testSquarifiedTilesFillTheBox() {
        let values: [Double] = [6, 6, 4, 3, 2, 2, 1]
        let tiles = ReportMath.squarify(values, value: { $0 }, width: 6, height: 4)
        XCTAssertEqual(tiles.count, values.count)
        for tile in tiles {
            XCTAssertEqual(tile.item, tile.w * tile.h, accuracy: 1e-9)
            XCTAssertTrue(tile.x >= -1e-9 && tile.y >= -1e-9 && tile.x + tile.w <= 6 + 1e-9 && tile.y + tile.h <= 4 + 1e-9)
        }
        XCTAssertEqual(tiles.reduce(0) { $0 + $1.w * $1.h }, 24, accuracy: 1e-9)
        XCTAssertTrue(ReportMath.squarify(values, value: { $0 }, width: 0, height: 4).isEmpty)
    }

    func testFiltersReadAsAllNoneOneOrACount() {
        let groups = [ReportFilterGroup(name: "Budget", items: [ReportFilterItem(id: "a", label: "Checking"), ReportFilterItem(id: "b", label: "Savings")]),
                      ReportFilterGroup(name: "Tracking", items: [ReportFilterItem(id: "c", label: "House")])]
        let several: (Int) -> String = { String(localized: "\($0) accounts", locale: .app) }
        XCTAssertEqual(ReportMath.filterSummary(groups, [], allLabel: "All accounts", several: several), "All accounts")
        XCTAssertEqual(ReportMath.filterSummary(groups, ["a"], allLabel: "All accounts", several: several), "Checking")
        XCTAssertEqual(ReportMath.filterSummary(groups, ["a", "c"], allLabel: "All accounts", several: several), "2 accounts")
        XCTAssertEqual(ReportMath.filterSummary(groups, [""], allLabel: "All accounts", several: several), "None")
        XCTAssertEqual(ReportMath.filterToggle(groups, [], ["a"], on: false), ["b", "c"])
        XCTAssertEqual(ReportMath.filterToggle(groups, ["b", "c"], ["a"], on: true), [])
        XCTAssertEqual(ReportMath.filterToggle(groups, ["a"], ["a"], on: false), [""])
        XCTAssertEqual(ReportMath.filterToggle(groups, [], ["a", "b"], on: false), ["c"])
        XCTAssertTrue(ReportMath.filterIsOn([], "x"))
        XCTAssertFalse(ReportMath.filterIsOn([""], "a"))
    }

    func testAccountTypesFallBackLikeTheDesktop() {
        XCTAssertEqual(ReportMath.accountType(chosen: "", imported: "", kind: "cash", balance: 0), "checking")
        XCTAssertEqual(ReportMath.accountType(chosen: "", imported: "", kind: "credit", balance: 0), "creditCard")
        XCTAssertEqual(ReportMath.accountType(chosen: "", imported: "otherLiability", kind: "tracking", balance: 5), "otherDebt")
        XCTAssertEqual(ReportMath.accountType(chosen: "", imported: "", kind: "tracking", balance: -5), "otherDebt")
        XCTAssertEqual(ReportMath.accountType(chosen: "", imported: "", kind: "tracking", balance: 5), "otherAsset")
        XCTAssertEqual(ReportMath.accountType(chosen: "mortgage", imported: "investment", kind: "tracking", balance: 5), "mortgage")
        XCTAssertEqual(ReportMath.sectionOf("autoLoan"), "Loans")
        XCTAssertEqual(ReportMath.sectionOf("somethingElse"), "Other assets")
    }

    func testReportsParseInEngineOrder() throws {
        let json = try JSON.parse("""
        {"months":[{"month":"2026-10","income":5,"spending":3,"categories":{"b":2,"a":1}}],
         "payees":{"B":{"amount":100,"count":2},"A":{"amount":300,"count":1}},
         "ageOfMoney":null,"ageOfMoneyByMonth":{"2026-10":null,"2026-09":12},
         "transactionCount":3,"income":5,"spending":3}
        """)
        let report = ReportMath.parseReport(json)
        XCTAssertEqual(report.payees.map(\.name), ["A", "B"])
        XCTAssertEqual(report.months.first?.categories.keys, ["a", "b"])
        XCTAssertNil(report.ageOfMoney)
        XCTAssertEqual(report.ageOfMoneyByMonth["2026-09"], .some(12))
        XCTAssertEqual(report.ageOfMoneyByMonth["2026-10"], .some(nil))
        XCTAssertEqual(report.transactionCount, 3)
        XCTAssertNil(report.openingAccounts)
    }

    func testIncomeLinesFollowTheBudgetSideOfTransfers() {
        let budget = budget([("c1", "Food", "Everyday")], accounts: [account("chk", "Checking", "cash"), account("inv", "Investments", "tracking")], transactions: [
            transaction("t1", "2026-10-05", "chk", "Employer", split(500_000, unassigned: true)),
            transaction("t2", "2026-10-10", "chk", "Employer", split(100_000)),
            transaction("t3", "2026-10-12", "chk", "Employer", split(-2000, "c1")),
            transaction("t4", "2026-11-01", "chk", "Employer", split(1000, unassigned: true)),
            transaction("t5", "2026-10-15", "inv", "Employer", split(3000)),
            transaction("t6", "2026-10-20", "inv", "Employer", split(-7000), transferTo: "chk"),
            transaction("t7", "2026-10-21", "chk", "Employer", split(-400)),
        ])
        let ctx = ReportMath.parseBudget(budget, today: today)
        let model = ReportMath.incomeDrill(budget, ctx, payee: "Employer", fromMonth: "2026-10", toMonth: "2026-10", accounts: [])
        XCTAssertEqual(model.lines.map(\.transactionId), ["t6", "t2", "t1"])
        XCTAssertEqual(model.lines[0].payee, "Transfer: Investments")
        XCTAssertEqual(model.lines[0].amount, "70.00")
        XCTAssertEqual(model.figureLabel, "Received")
        XCTAssertEqual(model.figure, "6,070.00")
        XCTAssertEqual(model.count, "3")
        XCTAssertEqual(model.average, "2,023.33")
        XCTAssertEqual(model.range, "2026-10-01 – 2026-10-31")
        let chosen = ReportMath.incomeDrill(budget, ctx, payee: "Employer", fromMonth: "2026-10", toMonth: "2026-10", accounts: ["inv"])
        XCTAssertTrue(chosen.lines.isEmpty)
        XCTAssertEqual(chosen.average, "—")
        XCTAssertTrue(chosen.range.hasSuffix(" · chosen accounts"))

        let items: [JSON] = [["transactionId": "t3", "date": "2026-10-12", "accountId": "chk", "categoryId": "c1", "payee": "Employer", "memo": "", "amount": 2000]]
        let details = ReportMath.drillFromDetails(items, budget, ctx, from: "2026-10-01", to: "2026-10-31", accounts: [])
        XCTAssertEqual(details.lines.map(\.amount), ["-20.00"])
        XCTAssertEqual(details.lines.first?.detail, "2026-10-12 · Checking · Food")
        XCTAssertEqual(details.figureLabel, "Spent")
        XCTAssertEqual(details.figure, "20.00")
        XCTAssertEqual(Array(details.records.keys), ["t3"])
    }

    func testOverviewCardsReadLikeThePhonesReports() throws {
        let ctx = ReportMath.parseBudget(budget([("c1", "Groceries", "Food"), ("c2", "Fuel", "Car"), ("c3", "Gifts", "Fun")]), today: today)
        let model = try XCTUnwrap(ReportMath.overviewModel(data([
            month("2026-09", income: 100, spending: 100),
            month("2026-10", income: 100, spending: 9000, categories: ["c1": 5000, "c2": 3000, "c3": -1000, ReportMath.uncategorized: 2000],
                  assets: 1000, debt: 500),
        ], age: 1), ctx))
        XCTAssertEqual(model.monthName, "October 2026")
        XCTAssertEqual(model.rows.map(\.label), ["Groceries", "Fuel", ReportMath.needsCategory])
        XCTAssertEqual(model.rows.map(\.color), [0, 1, 2])
        XCTAssertEqual(model.refundLead, "Includes 10.00 in net refunds. The bar shows categories with net spending.")
        XCTAssertEqual(model.refunds.map(\.label), ["Gifts"])
        XCTAssertEqual(model.refunds.map(\.valueText), ["-10.00"])
        XCTAssertEqual(model.incomeLead, "On average, you're spending more than you make.")
        XCTAssertEqual(model.debtsText, "-5.00")
        XCTAssertTrue(model.debtsNegative)
        XCTAssertEqual(model.ageText, "1 day")
        XCTAssertEqual(ReportMath.incomeLead([month("2026-10")]), "Nothing earned or spent in these months.")
        XCTAssertEqual(ReportMath.incomeLead([month("2026-10", income: 1000, spending: 990)]), "On average, you're spending about what you make.")
        XCTAssertEqual(ReportMath.incomeLead([month("2026-10", income: 1000, spending: 500)]), "On average, you're spending less than you make.")
        var many: [String: Int64] = [:]
        for i in 1...7 { many["x\(i)"] = Int64(i) * 100 }
        let crowded = try XCTUnwrap(ReportMath.overviewModel(data([month("2026-10", categories: many)]), ctx))
        XCTAssertEqual(crowded.rows.map(\.label), ["Unassigned", "Unassigned", "Unassigned", "Unassigned", "Unassigned", "All others"])
        XCTAssertEqual(crowded.rows.last?.valueText, "3.00")
        XCTAssertEqual(crowded.rows.last?.color, ReportColors.other)
    }

    func testChartAccountsTakeTheFirstFreeSlot() {
        let ctx = ReportMath.parseBudget(budget(accounts: (1...9).map { account("a\($0)", "A\($0)", "cash") }), today: today)
        var balances: [String: Int64] = [:]
        for i in 1...9 { balances["a\(i)"] = Int64(i) * 100 }
        let report = data([month("2026-10", accounts: balances)])
        let eligible = ReportMath.balanceAccounts(report, ctx, accounts: [])
        let chosen = ReportMath.defaultChartAccounts(report, eligible: eligible)
        XCTAssertEqual(chosen, ["a5": 0, "a6": 1, "a7": 2, "a8": 3, "a9": 4])
        let fewer = ReportMath.toggleChartAccount(chosen, eligible: eligible, id: "a6")
        XCTAssertEqual(ReportMath.toggleChartAccount(fewer, eligible: eligible, id: "a1"), ["a5": 0, "a7": 2, "a8": 3, "a9": 4, "a1": 1])
        var full: [String: Int] = [:]
        for i in 1...8 { full["a\(i)"] = i - 1 }
        XCTAssertEqual(ReportMath.toggleChartAccount(full, eligible: eligible, id: "a9"), full)
        let view = ReportMath.balancesView(report, ctx, accounts: [], picked: nil)
        XCTAssertEqual(view.tiles[0].value, "5 of 9")
        XCTAssertEqual(view.chart?.series.count, 5)
    }

    func testDaysAndWeekdaysUseTheCalendar() {
        XCTAssertEqual(ReportMath.jsWeekday("2026-10-01"), 4)
        XCTAssertEqual(ReportMath.jsWeekday("2026-10-04"), 0)
        XCTAssertEqual(ReportMath.addMonths(day: "2026-01-31", 1), "2026-02-28")
        XCTAssertEqual(ReportMath.addMonths(day: "2024-01-31", 1), "2024-02-29")
        XCTAssertEqual(ReportMath.addMonths(day: "2026-11-15", 3), "2027-02-15")
        XCTAssertEqual(ReportMath.days(from: "2024-02-28", through: "2024-03-01", limit: 10), ["2024-02-28", "2024-02-29", "2024-03-01"])
        XCTAssertEqual(ReportMath.lastDay("2026-02"), "2026-02-28")
    }

    // MARK: ReportScopeTest

    func testChangedFiltersHidePreviousFiguresBeforeReplacementArrives() {
        let previous = ReportScoped(scope: "budget-a/october/all", value: Int64(2500))
        XCTAssertEqual(previous.forScope("budget-a/october/all"), 2500)
        XCTAssertNil(previous.forScope("budget-a/november/all"))
        XCTAssertNil(previous.forScope("budget-a/october/cash"))
        XCTAssertNil(previous.forScope("budget-b/october/all"))
    }

    func testDelayedOldResponseCannotBecomeCurrentReportOrForecast() {
        let delayed = ReportScoped(scope: "old-request", value: [Int64(100)])
        XCTAssertNil(delayed.forScope("new-request"))
        XCTAssertEqual(ReportScoped(scope: "new-request", value: [Int64(200)]).forScope("new-request"), [200])
    }
}
