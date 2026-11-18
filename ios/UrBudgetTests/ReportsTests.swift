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
        XCTAssertEqual(lines.map(\.section), ["Cash and bank", "Overdrawn accounts", "Credit cards", "Accounts in credit", "Other investments", "Other assets"])
        XCTAssertEqual(lines[1], ReportSheetLine(account: lines[1].account, section: "Overdrawn accounts", debt: true, start: -100, end: 200))
        let view = ReportMath.balanceSheetView(report, ctx, accounts: [])
        XCTAssertEqual(view.tiles.map(\.value), ["53.00", "17.00", "36.00", "32.1%", "50.00"])
        XCTAssertEqual(view.tiles[2].note, "+5.00 in this period")
        XCTAssertEqual(view.tiles[4].note, "2.5 months of spending")
        XCTAssertEqual(view.own.map(\.label), ["Cash and bank", "Accounts in credit"])
        XCTAssertEqual(view.own.map(\.detail), ["94.3%", "5.7%"])
        XCTAssertEqual(view.owe.map(\.label), ["Credit cards", "Overdrawn accounts"])
        XCTAssertTrue(view.statement.contains { $0.title == "Invest · other investments" })
        XCTAssertTrue(view.help.hasSuffix("whatever their type."))
    }

    func testAScheduledMoveIntoInvestmentsIsNotScheduledSpending() {
        let forecast = ReportForecastData(balances: ReportAmounts(["b1": 100_000]), events: [
            ReportForecastEvent(date: "2026-10-22", amount: -3000, payee: "Rent", accountId: "b1", peerId: nil, categoryId: "c1", scheduleId: "s1"),
            ReportForecastEvent(date: "2026-10-25", amount: -60000, payee: "Broker", accountId: "b1", peerId: nil, categoryId: "c9", scheduleId: "s9", invested: true),
        ], recentSpending: 30000)
        let plan = ReportMath.forecastPlan(forecast, accounts: [], months: 3, start: today, through: ReportMath.addMonths(day: today, 3))
        XCTAssertEqual(plan.scheduledSpending, 1000.0, accuracy: 0)
        XCTAssertEqual(plan.suggested, 29000)
        // It still moves the balance.
        XCTAssertEqual(plan.balances, [97_000, 37_000])
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
        XCTAssertEqual(ReportMath.monthXTick("2026-10"), ReportXTick(short: "Oct", long: "Oct 2026", year: 2026))
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
        {"months":[{"month":"2026-10","income":5,"spending":3,"invested":-700,"categories":{"b":2,"a":1}}],
         "payees":{"B":{"amount":100,"count":2},"A":{"amount":300,"count":1}},
         "ageOfMoney":null,"ageOfMoneyByMonth":{"2026-10":null,"2026-09":12},
         "transactionCount":3,"income":5,"spending":3,"invested":-700}
        """)
        let report = ReportMath.parseReport(json)
        XCTAssertEqual(report.payees.map(\.name), ["A", "B"])
        XCTAssertEqual(report.months.first?.categories.keys, ["a", "b"])
        XCTAssertNil(report.ageOfMoney)
        XCTAssertEqual(report.ageOfMoneyByMonth["2026-09"], .some(12))
        XCTAssertEqual(report.ageOfMoneyByMonth["2026-10"], .some(nil))
        XCTAssertEqual(report.transactionCount, 3)
        XCTAssertNil(report.openingAccounts)
        // Money moved into or out of investments, left out of spending and income, by the month and over the range.
        XCTAssertEqual(report.invested, -700)
        XCTAssertEqual(report.months.first?.invested, -700)
        // An engine that doesn't send it moved nothing.
        let older = ReportMath.parseReport(["months": [["month": "2026-10", "spending": 3]], "spending": 3])
        XCTAssertEqual(older.invested, 0)
        XCTAssertEqual(older.months.first?.invested, 0)
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

    func testIncomeCardSaysHowSpendingComparesWithIncome() {
        XCTAssertEqual(ReportMath.incomeLead([month("2026-10")]), "Nothing earned or spent in these months.")
        XCTAssertEqual(ReportMath.incomeLead([month("2026-10", income: 1000, spending: 990)]), "On average, you're spending about what you make.")
        XCTAssertEqual(ReportMath.incomeLead([month("2026-10", income: 1000, spending: 500)]), "On average, you're spending less than you make.")
        XCTAssertEqual(ReportMath.incomeLead([month("2026-10", income: 100, spending: 9000)]), "On average, you're spending more than you make.")
    }

    /// The overview: a card for each report, with its headline, its note and its small chart.
    func testOverviewHasACardForEachReport() throws {
        let ctx = investingBudget()
        let months = [month("2026-09", income: 100_000, spending: 60_000, assets: 300_000, debt: 50_000),
                      month("2026-10", income: 100_000, spending: 90_000, assets: 400_000, debt: 80_000)]
        let forecast = ReportForecastData(balances: ReportAmounts(["chk": 100_000]), events: [
            ReportForecastEvent(date: "2026-10-25", amount: -30_000, payee: "Rent", accountId: "chk", peerId: nil, categoryId: "c1", scheduleId: "s1"),
            ReportForecastEvent(date: "2026-11-01", amount: 200_000, payee: "Salary", accountId: "chk", peerId: nil, categoryId: nil, scheduleId: "s2"),
        ], recentSpending: 0)
        let model = try XCTUnwrap(ReportMath.overviewModel(data(months), ctx, current: "2026-10", portfolio: try portfolio(), forecast: forecast,
                                                           today: today))
        // Six cards, in the reports' order; five without holdings; and none for figures that couldn't be had.
        XCTAssertEqual(model.cards.map(\.id), ["spending", "income", "networth", "balance", "investments", "forecast"])
        XCTAssertEqual(model.cards.map(\.id), ReportTopic.shown(holdings: true).map(\.id))
        let plain = try XCTUnwrap(ReportMath.overviewModel(data(months), ctx, current: "2026-10", forecast: forecast, today: today))
        XCTAssertEqual(plain.cards.count, 5)
        XCTAssertEqual(plain.cards.map(\.id), ReportTopic.shown(holdings: false).map(\.id))
        XCTAssertEqual(try XCTUnwrap(ReportMath.overviewModel(data(months), ctx)).cards.map(\.id), ["spending", "income", "networth", "balance"])
        XCTAssertNil(ReportMath.overviewModel(data([]), ctx))

        // Spending: this month's, with the monthly average, over spending by month.
        let spending = model.cards[0]
        XCTAssertEqual(spending.figure, "900.00")
        XCTAssertEqual(spending.note, "avg 750.00 a month")
        XCTAssertEqual(spending.bars?.series.map(\.values), [[60_000, 90_000]])
        XCTAssertEqual(spending.bars?.short, [["600", "900"]])
        // The month under way says so, and is the one drawn lighter.
        XCTAssertEqual(spending.bars?.titles, ["September 2026", "October 2026 so far"])
        XCTAssertEqual(spending.bars?.partial, 1)
        XCTAssertEqual(try XCTUnwrap(ReportMath.overviewModel(data(months), ctx, current: "2026-11")).cards[0].bars?.partial, -1)

        // Income: what was earned less what was spent, over the two side by side.
        let income = model.cards[1]
        XCTAssertEqual(income.figure, "+500.00")
        XCTAssertFalse(income.negative)
        XCTAssertEqual(income.note, "On average, you're spending less than you make.")
        XCTAssertEqual(income.bars?.series.map(\.label), ["Income", "Spending"])
        XCTAssertEqual(income.bars?.series.map(\.texts), [["1,000.00", "1,000.00"], ["600.00", "900.00"]])

        // Net worth: what it is now, with its change over the months.
        let worth = model.cards[2]
        XCTAssertEqual(worth.figure, "3,200.00")
        XCTAssertEqual(worth.note, "+700.00 in this period")
        XCTAssertEqual(worth.noteTone, .positive)
        XCTAssertEqual(worth.bars?.short, [["2.5K", "3.2K"]])

        // Balance sheet: what is owned, with what is owed, as one bar split between them.
        let sheet = model.cards[3]
        XCTAssertEqual(sheet.figure, "4,000.00")
        XCTAssertEqual(sheet.note, "Total liabilities 800.00")
        XCTAssertEqual(sheet.parts?.map(\.label), ["Assets", "Debts"])
        XCTAssertEqual(sheet.parts?.map(\.weight), [400_000, 80_000])
        XCTAssertNil(sheet.bars)
        XCTAssertNil(try XCTUnwrap(ReportMath.overviewModel(data([month("2026-10")]), ctx)).cards[3].parts)

        // Investments: the value now, with the gain.
        let invested = model.cards[4]
        XCTAssertEqual(invested.figure, "11,000.00")
        XCTAssertEqual(invested.note, "Gain +500.00 · +5.0%")

        // Forecast: the lowest balance ahead and when, over the forecast's line alone.
        let ahead = model.cards[5]
        XCTAssertEqual(ahead.figure, "700.00")
        XCTAssertEqual(ahead.note, "Lowest balance ahead · 2026-10-25")
        XCTAssertFalse(ahead.negative)
        XCTAssertEqual(ahead.line?.series.map(\.label), ["Forecast"])
        XCTAssertEqual(ahead.line?.keys.first, today)
        XCTAssertEqual(ahead.line?.keys.last, "2027-04-21")

        // Only a single series labels its highest and latest columns.
        let bars = try XCTUnwrap(worth.bars)
        XCTAssertEqual(ReportMiniBars.labelled(try XCTUnwrap(income.bars)), [])
        XCTAssertEqual(ReportMiniBars.labelled(bars), [1])
        var uneven = bars
        uneven.series[0].values = [900, 100, 500]
        uneven.short = [["9", "1", "5"]]
        XCTAssertEqual(ReportMiniBars.labelled(uneven), [0, 2])
    }

    /// Android's chartsOpenOnTheLastCompleteMonthAndSayWhichIsUnderWay.
    func testChartsOpenOnTheLastCompleteMonthAndSayWhichIsUnderWay() {
        let keys = ["2026-08", "2026-09", "2026-10"]
        let series = [ReportColumnSeries(key: "in", label: "Income", color: 0, values: [100, 200, 50]),
                      ReportColumnSeries(key: "out", label: "Spending", color: 1, values: [-80, -120, -10])]
        let whole: (Double) -> String = { "\(Int64($0))" }
        let chart = ReportMath.columnModel(label: "Chart", keys: keys, series: series, format: whole, axis: whole, unit: nil, current: "2026-10")
        XCTAssertEqual(chart.partial, 2)
        XCTAssertEqual(chart.opening, 1)
        XCTAssertEqual(chart.titles[2], "October 2026 so far")
        XCTAssertEqual(chart.titles[1], "September 2026")
        // A finished range opens on its last month; a plan ahead has none under way.
        XCTAssertEqual(ReportMath.columnModel(label: "Chart", keys: keys, series: series, format: { _ in "" }, axis: { _ in "" }, unit: nil, current: "2026-11").opening, 2)
        let plan = ReportMath.linesModel(label: "Plan", keys: keys, series: [ReportLineSeries(key: "a", label: "A", color: 0, values: [1, 2, 3])],
                                         format: { _ in "" }, axis: { _ in "" }, unit: nil, current: nil)
        XCTAssertEqual(plan.partial, -1)
        XCTAssertEqual(plan.opening, 2)
        // One month alone is opened on even while it's under way.
        XCTAssertEqual(ReportMath.columnModel(label: "Chart", keys: ["2026-10"], series: series, format: whole, axis: whole, unit: nil, current: "2026-10").opening, 0)
    }

    /// Android's hidingASeriesRescalesTheRestAndDropsTheTotal.
    func testHidingASeriesRescalesTheRestAndDropsTheTotal() {
        let whole: (Double) -> String = { "\(Int64($0))" }
        let chart = ReportMath.columnModel(label: "Chart", keys: ["2026-09", "2026-10"], series: [
            ReportColumnSeries(key: "a", label: "Assets", color: 0, values: [90_000, 100_000]),
            ReportColumnSeries(key: "d", label: "Debts", color: 1, values: [-2_000, -3_000]),
        ], format: whole, axis: whole, unit: nil, line: ReportLineOverlay(label: "Net worth", color: ReportColors.line, values: [88_000, 97_000]), current: nil)
        XCTAssertEqual(chart.readouts[1].map(\.label), ["Assets", "Debts", "Net worth"], "The line is the stack's total already")
        let debts = chart.without(["Assets", "Net worth"])
        XCTAssertEqual(debts.series.map(\.key), ["d"])
        XCTAssertNil(debts.line)
        XCTAssertEqual(debts.texts, [["-2000", "-3000"]])
        // The scale is the debts' alone now, and says so.
        XCTAssertTrue((debts.ticks.last ?? 1) <= 0 && (debts.ticks.first ?? -9_999) >= -5_000)
        XCTAssertEqual(debts.tickLabels, debts.ticks.map(whole))
        XCTAssertEqual(debts.readouts[1].map(\.label), ["Debts"])
        // Hiding everything hides nothing.
        XCTAssertEqual(chart.without(["Assets", "Debts", "Net worth"]), chart)
        XCTAssertEqual(chart.without([]), chart)

        let lines = ReportMath.linesModel(label: "Lines", keys: ["2026-09", "2026-10"], series: [
            ReportLineSeries(key: "big", label: "Savings", color: 0, values: [900_000, 950_000]),
            ReportLineSeries(key: "small", label: "Wallet", color: 1, values: [40, 80]),
        ], format: whole, axis: whole, unit: nil, current: nil)
        let wallet = lines.without(["Savings"])
        XCTAssertEqual(wallet.series.map(\.key), ["small"])
        XCTAssertEqual(wallet.endLabels, ["80"])
        XCTAssertLessThanOrEqual(wallet.ticks.last ?? .infinity, 200)
        XCTAssertEqual(wallet.readouts[0].map(\.label), ["Wallet"])
        XCTAssertEqual(lines.without(["Savings", "Wallet"]), lines)

        // A legend's last name showing stays.
        let legend = [ReportLegendItem(label: "Savings", color: 0), ReportLegendItem(label: "Wallet", color: 1)]
        XCTAssertEqual(ReportLegendView.toggled([], "Savings", legend: legend), ["Savings"])
        XCTAssertEqual(ReportLegendView.toggled(["Savings"], "Wallet", legend: legend), ["Savings"])
        XCTAssertEqual(ReportLegendView.toggled(["Savings"], "Savings", legend: legend), [])
    }

    /// Android's payeeTilesTakeTheirGroupsColour.
    func testPayeeTilesTakeTheirGroupsColour() {
        let ctx = ReportMath.parseBudget(budget([("c1", "Groceries", "Food"), ("c2", "Rent", "Bills")], groupOrder: ["Bills", "Food"]), today: today)
        var report = data([month("2026-10", spending: 16_000)])
        report.payees = [ReportPayeeSpend(name: "Landlord", amount: 10_000, count: 1, category: "c2"), ReportPayeeSpend(name: "Market", amount: 4_000, count: 6, category: "c1")]
            + (1...8).map { ReportPayeeSpend(name: "Shop \($0)", amount: 500, count: 1) }
        let view = ReportMath.payeeView(report, ctx)
        var colors: [String: Int] = [:]
        for tile in view.map { colors[tile.key] = tile.color }
        XCTAssertEqual(colors["Landlord"], ctx.colorOf("Bills"))
        XCTAssertEqual(colors["Market"], ctx.colorOf("Food"))
        XCTAssertEqual(colors["Shop 1"], ReportColors.other)
        XCTAssertEqual(view.legend.map(\.label), ["Bills", "Food"])
        // The engine names each payee's category; a payee with refunds alone has none.
        let parsed = ReportMath.parseReport(["months": [], "payees": ["Market": ["amount": 4_000, "count": 6, "category": "c1"], "Refunder": ["amount": -500, "count": 1]]])
        XCTAssertEqual(parsed.payees.first { $0.name == "Market" }?.category, "c1")
        XCTAssertNil(parsed.payees.first { $0.name == "Refunder" }?.category)
    }

    func testAxisLabelsStepBackFromTheLatestMonth() {
        let keys = (1...12).map { String(format: "2026-%02d", $0) } + ["2027-01"]
        let ticks = keys.map(ReportMath.monthXTick)
        // Room for every third label: the latest always shows, and the year where it's first said or changes.
        let labels = ReportAxis.xLabels(ticks, keys: keys, band: 20, spacing: 52)
        XCTAssertEqual(labels.keys.sorted(), ["2026-01", "2026-04", "2026-07", "2026-10", "2027-01"])
        XCTAssertEqual(labels["2026-01"], "Jan 2026")
        XCTAssertEqual(labels["2026-04"], "Apr")
        XCTAssertEqual(labels["2027-01"], "Jan 2027")
        // Room for all of them.
        XCTAssertEqual(ReportAxis.xLabels(ticks, keys: keys, band: 60, spacing: 52).count, 13)
        XCTAssertEqual(ReportAxis.xLabels([], keys: [], band: 60, spacing: 52), [:])
    }

    func testTheMonthCalendarLaysOutEachMonthOfTheRange() {
        // September to 5 November 2026, weeks starting on Sunday. 1 September 2026 is a Tuesday, two days in.
        let days = ReportMath.calendarDays(from: "2026-09", to: "2026-11", today: "2026-11-05")
        XCTAssertEqual(days.first, "2026-09-01")
        XCTAssertEqual(days.last, "2026-11-05")
        let heat = ReportHeatModel(days: days, levels: days.map { _ in 0 }, lead: 2, weeks: 10, months: [], weekdayLabels: [],
                                   titles: days, amounts: days.map { _ in "0.00" }, spent: days.map { _ in false })
        XCTAssertEqual(ReportMonthCalendar.months(heat), ["2026-09", "2026-10", "2026-11"])
        XCTAssertEqual(ReportMonthCalendar.weekStart(heat), 0)
        // October 2026 starts on a Thursday: four gaps, then its 31 days.
        let october = ReportMonthCalendar.cells("2026-10", weekStart: 0)
        XCTAssertEqual(october.prefix(5), [nil, nil, nil, nil, "2026-10-01"])
        XCTAssertEqual(october.count, 35)
        XCTAssertEqual(october.last, "2026-10-31")
        // A week starting on Monday moves the gaps; February in a leap year keeps its 29th.
        XCTAssertEqual(ReportMonthCalendar.cells("2026-10", weekStart: 1).prefix(4), [nil, nil, nil, "2026-10-01"])
        XCTAssertEqual(ReportMonthCalendar.cells("2028-02", weekStart: 0).last, "2028-02-29")
        // The report's week start is read back from its first day's place in the week.
        var monday = heat
        monday.lead = 1
        XCTAssertEqual(ReportMonthCalendar.weekStart(monday), 1)
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

    // MARK: Investments

    /// The engine's `investmentsReport` as docs/plans/holdings-in-reports.md, Appendix A, writes it:
    /// a broker's account with no snapshot of the start, and a pension valued by hand that joins in October.
    private func portfolio() throws -> Portfolio {
        ReportMath.parsePortfolio(try JSON.parse("""
        {
          "from": "2026-09", "to": "2026-10",
          "total": { "start": 900000, "end": 1100000, "put": 150000, "gain": 50000, "rate": 0.05, "yearly": null, "days": 61 },
          "months": [
            { "month": "2026-09", "value": 1000000, "gain": 20000, "put": 80000, "byAccount": { "inv": 1000000 } },
            { "month": "2026-10", "value": 1100000, "gain": 30000, "put": 70000, "byAccount": { "inv": 1000000, "pen": 100000 } }
          ],
          "accounts": [
            { "accountId": "inv", "source": "ibkr", "sourceAccount": "U123", "asOf": "2026-10-20", "joined": "2025-01-02",
              "start": 900000, "end": 1000000, "put": 60000, "gain": 40000, "rate": 0.043, "estimatedDays": 2, "missing": ["Transfers"], "sparse": false,
              "startParts": null,
              "endParts": { "month": "2026-10", "asOf": "2026-10-20",
                            "byKind": [ { "key": "Stocks", "value": 600000 }, { "key": "Bonds", "value": 250000 }, { "key": "Holdings", "value": 20000 },
                                        { "key": "Other holdings", "value": 10000 } ],
                            "cash": 100000, "accrued": 5000, "rest": 15000 } },
            { "accountId": "pen", "source": "manual", "sourceAccount": "", "asOf": "2026-10-01", "joined": "2026-10-01",
              "start": 0, "end": 100000, "put": 90000, "gain": 10000, "rate": null, "estimatedDays": 0, "missing": [], "sparse": true,
              "startParts": null, "endParts": null }
          ],
          "allocation": {
            "asOf": "2026-10-01", "total": 1100000, "count": 3, "cash": 100000, "accrued": 5000, "cost": 800000, "unrealised": 80000, "unrealisedRate": 0.1,
            "largestShare": 0.55, "topFiveShare": 0.8,
            "byKind": [ { "key": "Stocks", "value": 600000, "share": 0.55 }, { "key": "Bonds", "value": 250000, "share": 0.23 },
                        { "key": "Cash", "value": 100000, "share": 0.09 }, { "key": "Holdings", "value": 145000, "share": 0.13 },
                        { "key": "Accrued", "value": 5000, "share": 0.004 } ],
            "byAccount": [ { "key": "inv", "value": 1000000, "share": 0.91 }, { "key": "pen", "value": 100000, "share": 0.09 } ],
            "byCurrency": [ { "key": "USD", "value": 1100000, "share": 1 } ]
          },
          "holdings": [
            { "key": "IE00B4L5Y983", "symbol": "IWDA", "name": "World Fund", "group": "Stocks", "one": "Stock", "currency": "USD", "value": 600000,
              "cost": 520000, "gain": 80000, "rate": 0.154, "share": 0.55, "accounts": ["inv", "pen"] },
            { "key": "912810", "symbol": "T 4 1/4", "name": "Treasury", "group": "Bonds", "one": "Bond", "currency": "USD", "value": 250000,
              "cost": null, "gain": null, "rate": null, "share": 0.23, "accounts": ["inv"] }
          ],
          "income": {
            "months": [ { "month": "2026-09", "dividends": 3000, "interest": 500, "tax": -450, "fees": -100, "other": 0 },
                        { "month": "2026-10", "dividends": 0, "interest": 700, "tax": 0, "fees": -100, "other": 0 } ],
            "byAccount": [ { "accountId": "inv", "dividends": 3000, "interest": 1200, "tax": -450, "fees": -200, "other": 0, "commissions": -350 } ],
            "dividends": 3000, "interest": 1200, "tax": -450, "fees": -200, "other": 0, "yield": 0.004,
            "commissions": [ { "accountId": "inv", "from": "2025-10-21", "to": "2026-10-20", "amount": -350 } ],
            "reporting": ["inv"], "silent": ["pen"]
          },
          "unfilled": ["old"]
        }
        """))
    }

    /// A budget with a bank account, the two accounts of `portfolio()`, an investment account nothing fills, a house and a card.
    private func investingBudget() -> ReportBudget {
        var json = budget(accounts: [
            account("chk", "Checking", "cash", type: "checking"), account("inv", "Broker", "tracking", type: "investment"),
            account("pen", "Pension", "tracking", type: "retirement"), account("old", "Old fund", "tracking", type: "investment"),
            account("house", "House", "tracking", type: "property"), account("visa", "Visa", "credit"),
        ])
        json["investments"] = [["accountId": "inv"], ["accountId": "pen"]]
        return ReportMath.parseBudget(json, today: today)
    }

    func testPortfolioParsesTheEnginesShape() throws {
        let held = try portfolio()
        // An engine before plan section 17 says nothing of what a holding is or its own currency's amount.
        XCTAssertEqual(held.holdings[0].instrument, "")
        XCTAssertNil(held.holdings[0].amount)
        XCTAssertNil(held.allocation.byCurrency[0].amount)
        XCTAssertTrue(held.accounts[0].cash.isEmpty)
        let named = ReportMath.parsePortfolio(try JSON.parse("""
        { "accounts": [ { "accountId": "inv", "cash": [ { "currency": "EUR", "amount": "796.50", "value": 90000 },
                                                        { "currency": "SAR", "amount": null, "value": 1000 } ] } ],
          "allocation": { "byCurrency": [ { "key": "EUR", "value": 690000, "share": 0.9, "amount": "6316.50" },
                                          { "key": "SAR", "value": 1000, "share": 0.1, "amount": null } ] },
          "holdings": [ { "key": "IE00B4L5Y983", "symbol": "IWDA", "name": "World Fund", "group": "Stocks", "one": "Stock", "currency": "EUR",
                          "value": 600000, "accounts": ["inv"], "instrument": "ETF", "amount": "5520.50" } ] }
        """))
        XCTAssertEqual(named.holdings[0].instrument, "ETF")
        XCTAssertEqual(named.holdings[0].amount, "5520.50")
        XCTAssertEqual(named.allocation.byCurrency.map(\.amount), ["6316.50", nil])
        XCTAssertEqual(named.accounts[0].cash, [PortfolioCash(currency: "EUR", amount: "796.50", value: 90_000),
                                                PortfolioCash(currency: "SAR", amount: nil, value: 1_000)])
        XCTAssertEqual(held.from, "2026-09")
        XCTAssertEqual(held.total, PortfolioTotal(start: 900_000, end: 1_100_000, put: 150_000, gain: 50_000, rate: 0.05, yearly: nil, days: 61))
        XCTAssertEqual(held.months.map(\.month), ["2026-09", "2026-10"])
        XCTAssertEqual(held.months[1].byAccount["pen"], 100_000)
        XCTAssertNil(held.months[0].byAccount["pen"])
        let broker = held.accounts[0]
        XCTAssertEqual(broker.source, "ibkr")
        XCTAssertEqual(broker.rate, 0.043)
        XCTAssertEqual(broker.estimatedDays, 2)
        XCTAssertEqual(broker.missing, ["Transfers"])
        XCTAssertNil(broker.startParts)
        XCTAssertEqual(broker.endParts?.byKind.map(\.key), ["Stocks", "Bonds", "Holdings", "Other holdings"])
        XCTAssertEqual(broker.endParts?.cash, 100_000)
        XCTAssertEqual(broker.endParts?.rest, 15_000)
        let pension = held.accounts[1]
        XCTAssertTrue(pension.sparse)
        XCTAssertNil(pension.rate)
        XCTAssertNil(pension.endParts)
        XCTAssertEqual(held.allocation.count, 3)
        XCTAssertEqual(held.allocation.unrealised, 80_000)
        XCTAssertEqual(held.allocation.byCurrency, [PortfolioPart(key: "USD", value: 1_100_000, share: 1)])
        XCTAssertEqual(held.holdings.map(\.name), ["World Fund", "Treasury"])
        XCTAssertEqual(held.holdings[0].accounts, ["inv", "pen"])
        XCTAssertNil(held.holdings[1].cost)
        XCTAssertNil(held.holdings[1].rate)
        XCTAssertEqual(held.income.yieldRate, 0.004)
        XCTAssertEqual(held.income.byAccount,
                       [PortfolioIncomeAccount(accountId: "inv", dividends: 3_000, interest: 1_200, tax: -450, fees: -200, other: 0, commissions: -350)])        XCTAssertEqual(held.income.commissions, [PortfolioCommission(accountId: "inv", from: "2025-10-21", to: "2026-10-20", amount: -350)])
        XCTAssertEqual(held.income.silent, ["pen"])
        XCTAssertEqual(held.unfilled, ["old"])
        // An engine that doesn't know of trades says nothing of them; one that does names what they realised and cost.
        XCTAssertNil(held.total.realised)
        XCTAssertNil(held.accounts[0].realised)
        XCTAssertNil(held.months[0].realised)
        XCTAssertNil(held.income.months[0].commissions)
        XCTAssertTrue(held.income.byHolding.isEmpty)
        let traded = ReportMath.parsePortfolio(try JSON.parse("""
        { "total": { "realised": 1200 },
          "months": [ { "month": "2026-09", "realised": null }, { "month": "2026-10", "realised": 1200 } ],
          "accounts": [ { "accountId": "inv", "realised": 1200 }, { "accountId": "pen", "realised": null } ],
          "income": { "months": [ { "month": "2026-09", "commissions": null }, { "month": "2026-10", "commissions": -420 } ],
                      "byAccount": [ { "accountId": "inv", "commissions": -420 }, { "accountId": "pen", "commissions": null } ],
                      "byHolding": [ { "key": "IE00B4L5Y983", "symbol": "IWDA", "name": "World Fund", "dividends": 3000, "tax": -450,
                                       "value": 600000, "yield": 0.005 },
                                     { "key": "OLD USD", "symbol": "OLD", "name": "Sold", "dividends": 500, "tax": 0, "value": null, "yield": null } ] } }
        """))
        XCTAssertEqual(traded.total.realised, 1_200)
        XCTAssertEqual(traded.months.map(\.realised), [nil, 1_200])
        XCTAssertEqual(traded.accounts.map(\.realised), [1_200, nil])
        XCTAssertEqual(traded.income.months.map(\.commissions), [nil, -420])
        XCTAssertEqual(traded.income.byAccount.map(\.commissions), [-420, nil])
        XCTAssertEqual(traded.income.byHolding, [
            PortfolioIncomeHolding(key: "IE00B4L5Y983", symbol: "IWDA", name: "World Fund", dividends: 3_000, tax: -450, value: 600_000, yieldRate: 0.005),
            PortfolioIncomeHolding(key: "OLD USD", symbol: "OLD", name: "Sold", dividends: 500, tax: 0, value: nil, yieldRate: nil),
        ])
        // An empty answer still reads: nothing held, nothing known.
        let nothing = ReportMath.parsePortfolio([:])
        XCTAssertTrue(nothing.accounts.isEmpty)
        XCTAssertNil(nothing.total.rate)
        XCTAssertNil(nothing.allocation.cost)
        XCTAssertTrue(nothing.income.byAccount.isEmpty)
        // A report's month says what markets did, and nothing where the engine doesn't.
        let report = ReportMath.parseReport(["months": [["month": "2026-09", "market": 500], ["month": "2026-10"]]])
        XCTAssertEqual(report.months.map(\.market), [500, 0])
    }

    func testSixReportsHoldTheViews() {
        XCTAssertEqual(ReportTopic.all.map(\.id), ["spending", "income", "networth", "balance", "investments", "forecast"])
        // Investments is listed only with holdings.
        XCTAssertEqual(ReportTopic.shown(holdings: false).map(\.id), ["spending", "income", "networth", "balance", "forecast"])
        XCTAssertEqual(ReportTopic.shown(holdings: true).count, 6)
        // Each report's views, in the order of its switch, by the ids the reports always had.
        XCTAssertEqual(ReportTopic.find("spending").views.map(\.id), ["categories", "payees", "trends", "calendar"])
        XCTAssertEqual(ReportTopic.find("spending").views.map(\.label), ["Category", "Payee", "Over time", "Calendar"])
        XCTAssertEqual(ReportTopic.find("income").views.map(\.id), ["incomeExpense", "income", "cashflow"])
        XCTAssertEqual(ReportTopic.find("income").views.map(\.label), ["Vs. spending", "Sources", "In and out"])
        XCTAssertEqual(ReportTopic.find("networth").views.map(\.id), ["networth", "balances"])
        XCTAssertEqual(ReportTopic.find("networth").views.map(\.label), ["Over time", "By account"])
        XCTAssertEqual(ReportTopic.find("balance").views.map(\.id), ["balance"])
        XCTAssertEqual(ReportTopic.find(ReportTopic.investments).views.map(\.id), ["performance", "allocation", "investmentIncome"])
        XCTAssertEqual(ReportTopic.find(ReportTopic.investments).views.map(\.label), ["Performance", "Allocation", "Income and costs"])
        XCTAssertEqual(ReportTopic.find("forecast").views.map(\.id), ["forecast", "age"])
        XCTAssertEqual(ReportTopic.find("forecast").views.map(\.label), ["Balances", "Cash age"])
        // Every one of the fifteen is a view of one of the six.
        XCTAssertEqual(ReportInfo.all.count, 15)
        XCTAssertTrue(ReportInfo.all.allSatisfy { view in ReportTopic.all.contains { $0.id == view.report } })
        XCTAssertEqual(ReportTopic.find("spending").label, "Spending")
        XCTAssertEqual(ReportTopic.find("income").question, "Am I spending less than I earn?")
    }

    func testAnOldReportIdOpensItsReportAtThatView() {
        let old: [(id: String, report: String)] = [
            ("categories", "spending"), ("payees", "spending"), ("trends", "spending"), ("calendar", "spending"),
            ("incomeExpense", "income"), ("income", "income"), ("cashflow", "income"),
            ("networth", "networth"), ("balances", "networth"), ("balance", "balance"),
            ("performance", "investments"), ("allocation", "investments"), ("investmentIncome", "investments"),
            ("forecast", "forecast"), ("age", "forecast"),
        ]
        for entry in old {
            XCTAssertEqual(ReportTopic.resolve(entry.id), entry.id, entry.id)
            XCTAssertEqual(ReportInfo.find(ReportTopic.resolve(entry.id)).report, entry.report, entry.id)
        }
        // A report's own id opens it on its first view, or on the one last used this session.
        XCTAssertEqual(ReportTopic.resolve("spending"), "categories")
        XCTAssertEqual(ReportTopic.resolve("investments"), "performance")
        XCTAssertEqual(ReportTopic.resolve("spending", remembered: ["spending": "payees"]), "payees")
        XCTAssertEqual(ReportTopic.view(of: "forecast", remembered: ["forecast": "age"]), "age")
        XCTAssertEqual(ReportTopic.view(of: "networth"), "networth")
        // A view remembered for a report it isn't in is passed over, and an id that is nothing opens the first report.
        XCTAssertEqual(ReportTopic.view(of: "income", remembered: ["income": "payees"]), "incomeExpense")
        XCTAssertEqual(ReportTopic.resolve("nothing"), "categories")
    }

    func testTheFilterChipSaysWhatIsInForce() {
        XCTAssertEqual(ReportMath.filterLabel(range: "Last 12 months", accounts: "All accounts", categories: nil), "Last 12 months · All accounts")
        XCTAssertEqual(ReportMath.filterLabel(range: "Last 12 months", accounts: "2 accounts", categories: "3 categories"),
                       "Last 12 months · 2 accounts · 3 categories")
        // A view that takes no months leaves them out.
        XCTAssertEqual(ReportMath.filterLabel(range: nil, accounts: "All budget accounts", categories: nil), "All budget accounts")
        let several: (Int) -> String = { String(localized: "\($0) categories", locale: .app) }
        XCTAssertEqual(several(3), "3 categories")
    }

    /// Plan section 16.1: money moved into or out of investment accounts counts as neither spending
    /// nor income, and every view of the two reports says how much is left out, under its filter chip.
    func testSpendingAndIncomeSayWhatMovedIntoInvestments() {
        XCTAssertEqual(ReportMath.investedNote(250_000, currency: "USD"), "2,500.00 moved into investments isn't counted here.")
        // Taken out: the amount without its sign.
        XCTAssertEqual(ReportMath.investedNote(-40_050, currency: "USD"), "400.50 taken out of investments isn't counted here.")
        XCTAssertNil(ReportMath.investedNote(0, currency: "USD"))
        XCTAssertEqual(ReportTopic.all.filter(\.leavesOutInvested).map(\.id), ["spending", "income"])
        let views = ReportInfo.all.filter { ReportTopic.find($0.report).leavesOutInvested }.map(\.id)
        XCTAssertEqual(views, ["categories", "payees", "trends", "calendar", "incomeExpense", "income", "cashflow"])
    }

    func testListsShowSixRowsUntilTheyAreOpened() {
        XCTAssertEqual(ReportMath.foldAt, 6)
        XCTAssertEqual(ReportMath.folded(Array(1...9), open: false), [1, 2, 3, 4, 5, 6])
        XCTAssertEqual(ReportMath.folded(Array(1...9), open: true).count, 9)
        XCTAssertEqual(ReportMath.folded(Array(1...6), open: false).count, 6)
        // A fold that would hide only one or two rows isn't worth its button.
        XCTAssertEqual(ReportMath.folded(Array(1...8), open: false).count, 8)
        XCTAssertFalse(ReportMath.folds(8))
        XCTAssertTrue(ReportMath.folds(9))
        XCTAssertEqual(ReportMath.folded([Int](), open: false), [])
        // Totals, and a sentence about them, stay in view under a folded list.
        let styles: [ReportRowStyle] = [.plain, .child, .group, .total, .net, .note, .muted]
        XCTAssertEqual(styles.filter { ReportMath.staysInView($0) }, [.total, .net, .note])
    }

    func testInvestmentsFilterListsOnlyAccountsWithHoldings() {
        XCTAssertFalse(ReportInfo.hasHoldings(nil))
        XCTAssertFalse(ReportInfo.hasHoldings(budget(accounts: [account("inv", "Broker", "tracking", type: "investment")])))
        var filled = budget(accounts: [account("inv", "Broker", "tracking", type: "investment")])
        filled["investments"] = [["accountId": "inv"]]
        XCTAssertTrue(ReportInfo.hasHoldings(filled))
        let ctx = investingBudget()
        XCTAssertEqual(ctx.held, ["inv", "pen"])
        XCTAssertEqual(ReportMath.holdingsFilterGroups(ctx).flatMap(\.items).map(\.label), ["Broker", "Pension"])
        XCTAssertTrue(ReportMath.holdingsFilterGroups(ReportMath.parseBudget(budget(), today: today)).isEmpty)
    }

    func testNetWorthSplitsEachMonthsChangeIntoMarketsAndEverythingElse() {
        let ctx = ReportMath.parseBudget(budget(accounts: [account("a1", "Savings", "cash", type: "savings")]), today: today)
        var september = month("2026-09", accounts: ["a1": 100_000], assets: 100_000)
        var october = month("2026-10", accounts: ["a1": 97_000], assets: 97_000)
        let plain = ReportMath.netWorthView(data([september, october], opening: ["a1": 90_000]), ctx, accounts: [])
        XCTAssertEqual(plain.change.series.map(\.label), ["Grew", "Shrank"])
        XCTAssertEqual(plain.tiles.count, 4)
        // The same months once markets have a figure: the two parts add up to what each month showed before.
        september.market = 4_000
        october.market = -5_000
        let view = ReportMath.netWorthView(data([september, october], opening: ["a1": 90_000]), ctx, accounts: [])
        XCTAssertEqual(view.change.series.map(\.label), ["Markets", "Everything else"])
        XCTAssertEqual(view.changeLegend.map(\.label), ["Markets", "Everything else"])
        XCTAssertEqual(view.change.series[0].values, [4_000, -5_000])
        XCTAssertEqual(view.change.series[1].values, [6_000, 2_000])
        for index in 0..<2 {
            let before = plain.change.series[0].values[index] + plain.change.series[1].values[index]
            XCTAssertEqual(view.change.series[0].values[index] + view.change.series[1].values[index], before)
        }
        XCTAssertEqual(view.tiles.count, 5)
        XCTAssertEqual(view.tiles[4].label, "From markets")
        XCTAssertEqual(view.tiles[4].value, "-10.00")
        XCTAssertEqual(view.tiles[4].tone, .negative)
        // Markets fell while net worth rose: no share of the change to speak of.
        XCTAssertNil(view.tiles[4].note)
        october.market = 3_500
        let up = ReportMath.netWorthView(data([september, october], opening: ["a1": 90_000]), ctx, accounts: [])
        XCTAssertEqual(up.tiles[4].value, "+75.00")
        XCTAssertEqual(up.tiles[4].note, "107.1% of the change")
        XCTAssertEqual(up.tiles[4].tone, .positive)
    }

    func testBalanceSheetGroupsWhatIsOwnedByWhatItIsIn() throws {
        let ctx = investingBudget()
        var held = try portfolio()
        let last: [String: Int64] = ["chk": 500_000, "inv": 1_000_000, "pen": 100_000, "old": 0, "house": 30_000_000, "visa": -200_000]
        let opening: [String: Int64] = ["chk": 400_000, "inv": 900_000, "pen": 0, "old": 0, "house": 30_000_000, "visa": -100_000]
        let report = data([month("2026-09", spending: 100_000), month("2026-10", spending: 100_000, accounts: last)], opening: opening)
        let assets = ReportMath.balanceSheetLines(report, ctx, accounts: []).filter { !$0.debt }
        XCTAssertEqual(assets.map(\.section), ["Cash and bank", "Other investments", "Other investments", "Other investments", "Property"])

        // The broker's account has a line in each section it has something in, and they add up to its balance.
        let split = ReportMath.balanceSheetParts(assets, portfolio: held)
        let broker = split.parts.filter { $0.key.hasPrefix("a:inv") }
        XCTAssertEqual(broker.map(\.section), ["Stocks", "Bonds", "Other investments", "Cash and bank", "Receivables", "Other investments"])
        XCTAssertEqual(broker.map(\.title), ["Broker", "Broker", "Broker", "Cash at Broker", "Accrued income at Broker", "Broker · Since prices from 2026-10-20"])
        XCTAssertEqual(broker.map(\.end), [600_000, 250_000, 30_000, 100_000, 5_000, 15_000])
        XCTAssertEqual(broker.map(\.end).reduce(0, +), 1_000_000)
        // What it held at the start isn't kept, so none of its lines has a start.
        XCTAssertTrue(broker.allSatisfy { $0.start == nil })
        XCTAssertEqual(split.noStart, ["Broker"])
        // The pension can't be broken down at this range's end: it stays whole.
        XCTAssertEqual(split.whole, ["Pension"])
        XCTAssertEqual(split.parts.first { $0.key == "a:pen" }, ReportSheetPart(key: "a:pen", title: "Pension", section: "Other investments", start: 0, end: 100_000))
        XCTAssertEqual(split.parts.map(\.end).reduce(0, +), assets.map(\.end).reduce(0, +))
        // Without the investments report every account is one line, as it was.
        XCTAssertEqual(ReportMath.balanceSheetParts(assets, portfolio: nil).parts.map(\.key), ["a:chk", "a:inv", "a:pen", "a:old", "a:house"])

        let before = ReportMath.balanceSheetView(report, ctx, accounts: [], current: "2026-11")
        // A range that ended before this month keeps the accounts' lines.
        let view = ReportMath.balanceSheetView(report, ctx, accounts: [], portfolio: held, current: "2026-11")
        // The totals are the accounts' own, so they don't move.
        XCTAssertEqual(view.tiles.prefix(4).map(\.value), ["316,000.00", "2,000.00", "314,000.00", "0.6%"])
        XCTAssertEqual(view.tiles.prefix(4).map(\.value), before.tiles.prefix(4).map(\.value))
        XCTAssertEqual(view.ownTotal, before.ownTotal)
        // Cash on hand counts the cash at the broker.
        XCTAssertEqual(before.tiles[4].value, "5,000.00")
        XCTAssertEqual(view.tiles[4].value, "6,000.00")
        XCTAssertEqual(view.own.map(\.label), ["Cash and bank", "Stocks", "Bonds", "Other investments", "Property", "Receivables"])
        func subtitle(_ key: String) -> String? { view.statement.first { $0.key == key }?.subtitle }
        // A line whose start isn't known says only its share of what is owned, as does a section holding it.
        XCTAssertEqual(subtitle("a:inv:k:Stocks"), "1.9% of assets")
        XCTAssertEqual(subtitle("s:Assets:Stocks"), "1.9% of assets")
        XCTAssertEqual(subtitle("s:Assets:Cash and bank"), "1.9% of assets")
        XCTAssertEqual(subtitle("a:inv:cash"), "0.3% of assets")
        XCTAssertEqual(subtitle("a:chk"), "From 4,000.00 · Change 1,000.00 · 1.6%")
        XCTAssertEqual(subtitle("a:pen"), "From 0.00 · Change 1,000.00 · 0.3%")
        XCTAssertEqual(subtitle("s:Assets:Property"), "From 300,000.00 · Change 0.00 · 94.9%")
        XCTAssertEqual(subtitle("t:Assets"), "From 313,000.00 · Change 3,000.00")
        XCTAssertEqual(view.statement.first { $0.key == "note:Assets" }?.title, "Investments changed by +2,000.00: +1,500.00 put in, +500.00 from markets.")
        XCTAssertEqual(view.statement.first { $0.key == "note:Assets" }?.style, .note)
        XCTAssertTrue(view.help.contains("Start and change not shown by type, because what was held then isn't kept: Broker."))
        XCTAssertTrue(view.help.hasSuffix("Not broken down, because what was held then isn't kept: Pension."))
        XCTAssertNil(before.statement.first { $0.key == "note:Assets" })

        // With a snapshot of the month before the range, the start is split too.
        held.accounts[0].startParts = PortfolioParts(month: "2026-08", asOf: "2026-08-31",
                                                     byKind: [PortfolioPart(key: "Stocks", value: 500_000, share: 0), PortfolioPart(key: "Bonds", value: 300_000, share: 0)],
                                                     cash: 100_000, accrued: 0, rest: 0)
        let known = ReportMath.balanceSheetView(report, ctx, accounts: [], portfolio: held, current: "2026-11")
        func knownSubtitle(_ key: String) -> String? { known.statement.first { $0.key == key }?.subtitle }
        XCTAssertEqual(knownSubtitle("a:inv:k:Stocks"), "From 5,000.00 · Change 1,000.00 · 1.9%")
        XCTAssertEqual(knownSubtitle("a:inv:k:Bonds"), "From 3,000.00 · Change -500.00 · 0.8%")
        XCTAssertEqual(knownSubtitle("a:inv:k:Other investments"), "From 0.00 · Change 300.00 · 0.1%")
        XCTAssertEqual(knownSubtitle("s:Assets:Cash and bank"), "From 5,000.00 · Change 1,000.00 · 1.9%")
        XCTAssertFalse(known.help.contains("Start and change not shown by type"))

        // Parts that don't add up to the balance leave the rest with what moved since the prices.
        held.accounts[0].endParts?.rest = 0
        let mended = ReportMath.balanceSheetParts(assets, portfolio: held).parts.filter { $0.key.hasPrefix("a:inv") }
        XCTAssertEqual(mended.map(\.end).reduce(0, +), 1_000_000)
        XCTAssertEqual(mended.first { $0.key == "a:inv:rest" }?.end, 15_000)

        // An account whose value alone is known (parts with nothing but the remainder) is one line under its own name.
        held.accounts[1].endParts = PortfolioParts(month: "2026-10", asOf: "2026-10-01", byKind: [], cash: 0, accrued: 0, rest: 100_000)
        let valued = ReportMath.balanceSheetParts(assets, portfolio: held)
        XCTAssertEqual(valued.parts.first { $0.key == "a:pen" }, ReportSheetPart(key: "a:pen", title: "Pension", section: "Other investments", start: 0, end: 100_000))
        XCTAssertTrue(valued.whole.isEmpty)
        XCTAssertTrue(valued.noStart.isEmpty)

        // An account below zero is owed, and stays whole there.
        var overdrawn = last
        overdrawn["inv"] = -1_000
        let owed = ReportMath.balanceSheetView(data([month("2026-10", accounts: overdrawn)], opening: opening), ctx, accounts: [], portfolio: held)
        XCTAssertTrue(owed.statement.contains { $0.key == "a:inv" })
        XCTAssertFalse(owed.statement.contains { $0.key.hasPrefix("a:inv:") })
    }

    /// Plan section 17.2: a Balance sheet ending this month lists what is held in each investment
    /// section, merged across accounts, and the cash at a broker by currency.
    func testBalanceSheetListsWhatIsHeldThisMonth() throws {
        let ctx = investingBudget()
        var held = try portfolio()
        func holding(_ key: String, _ name: String, _ symbol: String, _ group: String, _ value: Int64, currency: String = "USD", instrument: String = "",
                     amount: String? = nil) -> PortfolioHolding {
            PortfolioHolding(key: key, symbol: symbol, name: name, group: group, one: "", currency: currency, value: value, cost: nil, gain: nil, rate: nil,
                             share: 0, accounts: ["inv"], instrument: instrument, amount: amount)
        }
        held.holdings = [
            holding("IE00B4L5Y983", "World Fund", "IWDA", "Stocks", 600_000, currency: "EUR", instrument: "ETF", amount: "5520.50"),
            holding("912810", "T 4 1/4", "T 4 1/4", "Bonds", 250_000, instrument: "Government bond", amount: "2500.00"),
            holding("X USD", "Private fund", "", "Holdings", 20_000),
            holding("Y USD", "Old note", "", "Other holdings", 10_000),
            holding("AAPL  261218C00250000", "AAPL 18DEC26 250 C", "AAPL  261218C00250000", "Options", 84_000, instrument: "Call option", amount: "840.00"),
        ]
        // The broker holds the option too: what moved since the prices is that much less, so its lines still add up to its balance.
        held.accounts[0].endParts?.byKind.append(PortfolioPart(key: "Options", value: 84_000, share: 0))
        held.accounts[0].endParts?.rest = 15_000 - 84_000
        held.accounts[0].cash = [PortfolioCash(currency: "EUR", amount: "796.50", value: 90_000), PortfolioCash(currency: "USD", amount: "100.00", value: 10_000)]
        let last: [String: Int64] = ["chk": 500_000, "inv": 1_000_000, "pen": 100_000, "old": 0, "house": 30_000_000, "visa": -200_000]
        let opening: [String: Int64] = ["chk": 400_000, "inv": 900_000, "pen": 0, "old": 0, "house": 30_000_000, "visa": -100_000]
        let report = data([month("2026-09", spending: 100_000), month("2026-10", spending: 100_000, accounts: last)], opening: opening)
        let before = ReportMath.balanceSheetView(report, ctx, accounts: [], portfolio: held, current: "2026-11")
        let view = ReportMath.balanceSheetView(report, ctx, accounts: [], portfolio: held, current: "2026-10")
        func row(_ key: String) -> ReportFigureRow? { view.statement.first { $0.key == key } }

        // The holdings stand in for the broker's lines of their kinds; the sections and totals don't move.
        XCTAssertNil(row("a:inv:k:Stocks"))
        XCTAssertNil(row("a:inv:k:Bonds"))
        XCTAssertNil(row("a:inv:k:Other investments"))
        XCTAssertEqual(row("h:0:IE00B4L5Y983")?.title, "World Fund")
        XCTAssertEqual(row("h:0:IE00B4L5Y983")?.value, "6,000.00")
        XCTAssertEqual(row("h:0:IE00B4L5Y983")?.subtitle, "ETF · IWDA · 5,520.50 EUR")
        // Its symbol is its name, and its currency is the budget's: what it is says it all.
        XCTAssertEqual(row("h:0:912810")?.subtitle, "Government bond")
        // An option's code only spells its name out again at length, so it's left out; its dollars are the budget's own.
        XCTAssertEqual(row("h:0:AAPL  261218C00250000")?.title, "AAPL 18DEC26 250 C")
        XCTAssertEqual(row("h:0:AAPL  261218C00250000")?.subtitle, "Call option")
        // Holdings no source gave a kind for sit in Other investments, which says what they are already.
        XCTAssertEqual(row("h:0:X USD")?.title, "Private fund")
        XCTAssertNil(row("h:0:X USD")?.subtitle)
        XCTAssertNotNil(row("h:1:Y USD"))
        // What the holdings don't explain stays one line, named for the account.
        XCTAssertNotNil(row("a:inv:rest"))
        XCTAssertNotNil(row("a:pen"))
        XCTAssertEqual(view.tiles.map(\.value), before.tiles.map(\.value))
        XCTAssertEqual(view.own, before.own)
        XCTAssertEqual(row("s:Assets:Stocks")?.value, "6,000.00")
        XCTAssertEqual(row("s:Assets:Other investments")?.value, before.statement.first { $0.key == "s:Assets:Other investments" }?.value)
        // The cash at the broker says what it is in, over its share.
        XCTAssertEqual(row("a:inv:cash")?.subtitle, "796.50 EUR · 100.00 USD\n0.3% of assets")
        XCTAssertEqual(before.statement.first { $0.key == "a:inv:cash" }?.subtitle, "0.3% of assets")
        // Each section's holdings are one list in the statement, which folds when it is long.
        let runs = ReportMath.foldRuns(view.statement)
        XCTAssertEqual(runs.filter(\.folds).map { $0.rows.map(\.key) }, [["h:0:IE00B4L5Y983"], ["h:0:912810"], ["h:0:AAPL  261218C00250000"], ["h:0:X USD", "h:1:Y USD"]])
        XCTAssertEqual(runs.flatMap(\.rows).map(\.key), view.statement.map(\.key))
        XCTAssertTrue(ReportMath.foldRuns(before.statement).allSatisfy { !$0.folds })

        // Ten holdings of one kind fold to six; with another account holding investments, each names its accounts.
        var many = held
        many.holdings = (0..<10).map { holding("S\($0)", "Share \($0)", "S\($0)", "Stocks", 60_000) } + Array(held.holdings.dropFirst())
        many.holdings.append(PortfolioHolding(key: "P", symbol: "", name: "Pension fund", group: "Funds", one: "", currency: "USD", value: 100_000,
                                              cost: nil, gain: nil, rate: nil, share: 0, accounts: ["pen"]))
        let long = ReportMath.balanceSheetView(report, ctx, accounts: [], portfolio: many, current: "2026-10")
        let stocks = ReportMath.foldRuns(long.statement).filter(\.folds).first
        XCTAssertEqual(stocks?.rows.count, 10)
        XCTAssertTrue(ReportMath.folds(stocks?.rows.count ?? 0))
        XCTAssertEqual(ReportMath.folded(stocks?.rows ?? [], open: false).count, 6)
        XCTAssertEqual(long.statement.first { $0.key == "h:0:S0" }?.subtitle, "S0 · Broker")
        XCTAssertEqual(long.statement.first { $0.key == "h:0:912810" }?.subtitle, "Government bond · Broker")

        // A holding also held by an account that isn't broken down leaves its section's lines as they were.
        var shared = held
        shared.holdings[0].accounts = ["inv", "pen"]
        let kept = ReportMath.balanceSheetView(report, ctx, accounts: [], portfolio: shared, current: "2026-10")
        XCTAssertNotNil(kept.statement.first { $0.key == "a:inv:k:Stocks" })
        XCTAssertNil(kept.statement.first { $0.key == "h:0:IE00B4L5Y983" })
        XCTAssertNotNil(kept.statement.first { $0.key == "h:0:912810" })
    }

    /// Plan section 17.2: amounts in their own currency, and what a holding is, in words.
    func testOwnCurrencyAmountsAndInstrumentNames() {
        XCTAssertEqual(HoldingsPage.ownText("61476.00", code: "USD"), "61,476.00 USD")
        // The engine writes each currency's usual decimals, and they are kept as written.
        XCTAssertEqual(HoldingsPage.ownText("-1234567.50", code: "EUR"), "-1,234,567.50 EUR")
        XCTAssertEqual(HoldingsPage.ownText("1.250", code: "KWD"), "1.250 KWD")
        XCTAssertEqual(HoldingsPage.ownText("150000", code: "JPY"), "150,000 JPY")
        // A symbol shows where it adds to the title and is short enough to read, as on the Holdings page.
        XCTAssertEqual(ReportMath.shownSymbol("IWDA", title: "World Fund"), "IWDA")
        XCTAssertNil(ReportMath.shownSymbol("T 4 1/4", title: "T 4 1/4"))
        XCTAssertNil(ReportMath.shownSymbol("", title: "World Fund"))
        XCTAssertEqual(ReportMath.shownSymbol("ABCDEFGHIJKL", title: "x"), "ABCDEFGHIJKL")
        XCTAssertNil(ReportMath.shownSymbol("AAPL  261218C00250000", title: "AAPL 18DEC26 250 C"))
        XCTAssertEqual(HoldingsPage.ownText("n/a", code: "USD"), "n/a USD")
        // Only where the currency isn't the budget's, and only where it's known.
        XCTAssertEqual(HoldingsPage.ownAmount("796.50", code: "EUR", budget: "USD"), "796.50 EUR")
        XCTAssertNil(HoldingsPage.ownAmount("796.50", code: "usd", budget: "USD"))
        XCTAssertNil(HoldingsPage.ownAmount(nil, code: "EUR", budget: "USD"))
        XCTAssertNil(HoldingsPage.ownAmount("", code: "EUR", budget: "USD"))
        XCTAssertNil(ReportMath.cashByCurrency([PortfolioCash(currency: "USD", amount: "10.00", value: 1_000)], currency: "USD"))
        XCTAssertEqual(ReportMath.cashByCurrency([PortfolioCash(currency: "EUR", amount: nil, value: 9_000), PortfolioCash(currency: "USD", amount: "10.00", value: 1_000)],
                                                 currency: "USD"), "90.00 · 10.00 USD")
        XCTAssertEqual(["ETF", "Exchange-traded note", "REIT", "Depositary receipt", "Preferred share", "Right", "Closed-end fund", "Treasury bill",
                        "Government bond", "Corporate bond", "Call option", "Put option"].map { HoldingsPage.instrumentName($0) },
                       ["ETF", "Exchange-traded note", "REIT", "Depositary receipt", "Preferred share", "Right", "Closed-end fund", "Treasury bill",
                        "Government bond", "Corporate bond", "Call option", "Put option"])
        // The rest are the kinds' singulars, as before.
        XCTAssertEqual(["Stock", "Fund", "Option", "Crypto", "Other"].map { HoldingsPage.instrumentName($0) }, ["Stock", "Fund", "Option", "Crypto", "Other"])
    }

    /// Plan section 17.2: a Holdings page row says what the holding is, where it said its kind.
    func testHoldingsPageRowsSayWhatEachHoldingIs() throws {
        let page = try JSON.parse("""
        { "total": 1709930, "holdings": [
          { "symbol": "VWRA", "name": "World", "quantity": "120.5", "price": "141.9087", "currency": "USD", "value": 1709930, "group": "Stocks",
            "one": "Stock", "instrument": "ETF", "amount": "17099.00" },
          { "symbol": "AAPL", "name": "Apple", "quantity": "10", "price": "250", "currency": "USD", "value": 250000, "group": "Stocks", "one": "Stock" },
          { "symbol": "X", "name": "Thing", "quantity": "1", "price": "1", "currency": "USD", "value": 100, "group": "Holdings" } ] }
        """)
        let model = HoldingsPage.model(page, period: "1y", sort: "value", currency: "USD")
        XCTAssertEqual(model.holdings.map(\.line), ["ETF · VWRA · 120.5 × 141.9087 USD", "Stock · AAPL · 10 × 250 USD", "X · 1 × 1 USD"])
    }

    func testInvestmentsReportsAreBuiltFromThePortfolio() throws {
        let ctx = investingBudget()
        let held = try portfolio()
        guard case .performance(let performance)? = ReportMath.investmentsView("performance", held, ctx, today: today) else {
            return XCTFail("No performance report")
        }
        XCTAssertEqual(performance.tiles.map(\.label), ["Value now", "Gain", "Return", "Money put in"])
        XCTAssertEqual(performance.tiles.map(\.value), ["11,000.00", "+500.00", "+5.0%", "+1,500.00"])
        XCTAssertNil(performance.tiles[2].note)
        // Each month's value by account, under the money put in: the gap between them is the gain so far.
        XCTAssertEqual(performance.value.series.map(\.label), ["Broker", "Pension"])
        XCTAssertEqual(performance.value.series[1].values, [0, 100_000])
        XCTAssertEqual(performance.value.line?.values, [980_000, 1_050_000])
        XCTAssertEqual(performance.valueLegend.map(\.label), ["Broker", "Pension", "Money put in"])
        XCTAssertEqual(performance.gain.series.map(\.values), [[20_000, 30_000], [0, 0]])
        XCTAssertEqual(performance.gainNote, "Valued only when updated, so a gain shows in the month of the next update: Pension.")
        XCTAssertEqual(performance.accounts.map(\.title), ["Broker", "Pension"])
        XCTAssertEqual(performance.accounts.map(\.value), ["10,000.00", "1,000.00"])
        XCTAssertEqual(performance.accounts.map(\.subtitle), ["Gain +400.00 · Return +4.3%", "Gain +100.00 · Return —"])
        XCTAssertEqual(performance.details["inv"]?.map(\.title), ["Kept up to date by", "Start", "Put in", "Gain", "End"])
        XCTAssertEqual(performance.details["inv"]?.map(\.value), ["Interactive Brokers", "9,000.00", "+600.00", "+400.00", "10,000.00"])
        XCTAssertEqual(performance.notes.count, 3)
        XCTAssertTrue(performance.notes[0].hasPrefix("Broker: On 2 days the value jumped"))
        XCTAssertEqual(performance.notes[1], "Pension: Prices from 2026-10-01 · These prices are a few days old.")
        XCTAssertTrue(performance.notes[2].hasPrefix("Not counted here, because nothing keeps them up to date: Old fund."))
        var longer = held
        longer.total.yearly = 0.031
        XCTAssertEqual(ReportMath.performanceView(longer, ctx, today: today).tiles[2].note, "+3.1% a year")

        guard case .allocation(let allocation)? = ReportMath.investmentsView("allocation", held, ctx, today: today) else {
            return XCTFail("No allocation report")
        }
        XCTAssertEqual(allocation.tiles.map(\.label), ["Holdings", "Largest holding", "Top five holdings", "Cash", "Unrealised gain"])
        XCTAssertEqual(allocation.tiles.map(\.value), ["3", "55%", "80%", "9%", "+800.00"])
        XCTAssertEqual(allocation.tiles[1].note, "World Fund")
        XCTAssertEqual(allocation.tiles[4].note, "Return on cost +10.0%")
        XCTAssertEqual(allocation.asOf, "Prices from 2026-10-01")
        // Each way of splitting is ranked, largest first.
        XCTAssertEqual(allocation.byKind.rows.map(\.label), ["Stocks", "Bonds", "Other investments", "Cash", "Accrued"])
        XCTAssertEqual(allocation.byKind.rows.prefix(3).map(\.detail), ["55%", "23%", "13%"])
        XCTAssertEqual(allocation.byKind.slices.map(\.color), [0, 1, 2, 3, 4])
        XCTAssertEqual(allocation.byAccount.rows.map(\.label), ["Broker", "Pension"])
        XCTAssertEqual(allocation.byCurrency.rows.map(\.label), ["USD"])
        // A holding in two accounts is one row naming both.
        XCTAssertEqual(allocation.holdings.map(\.title), ["World Fund", "Treasury"])
        // An engine before plan section 17 names the kind's singular, then the symbol where it isn't the name.
        XCTAssertEqual(allocation.holdings[0].subtitle, "Stock · IWDA · Broker, Pension · Share 55% · Unrealised gain +800.00 · Return on cost +15.4%")
        XCTAssertEqual(allocation.holdings[1].subtitle, "Bond · T 4 1/4 · Broker · Share 23%")
        XCTAssertTrue(allocation.byCurrency.rows.allSatisfy { $0.note == nil })
        XCTAssertEqual(allocation.notes, ["Broker: Prices from 2026-10-20", "Pension: Prices from 2026-10-01 · These prices are a few days old."])
        // Holdings no source gave a kind for are one part, called what the balance sheet calls them.
        var unnamed = held
        unnamed.allocation.byKind.append(PortfolioPart(key: "Other holdings", value: 5_000, share: 0.004))
        unnamed.holdings[1].group = "Holdings"
        unnamed.holdings[1].one = ""
        let merged = ReportMath.allocationView(unnamed, ctx, today: today)
        XCTAssertEqual(merged.byKind.rows.map(\.label), ["Stocks", "Bonds", "Other investments", "Cash", "Accrued"])
        XCTAssertEqual(merged.byKind.rows[2].valueText, "1,500.00")
        XCTAssertEqual(merged.holdings[1].subtitle, "Other investments · T 4 1/4 · Broker · Share 23%")

        // What each holding is, and its worth in its own currency where that isn't the budget's; a
        // currency's own amount under it, likewise.
        var named = held
        named.holdings[0].instrument = "ETF"
        named.holdings[0].currency = "EUR"
        named.holdings[0].amount = "5520.50"
        named.holdings[1].instrument = "Government bond"
        named.holdings[1].amount = "2500.00"
        named.allocation.byCurrency = [PortfolioPart(key: "USD", value: 1_000_000, share: 0.91, amount: "10000.00"),
                                       PortfolioPart(key: "EUR", value: 100_000, share: 0.09, amount: "920.40"),
                                       PortfolioPart(key: "GBP", value: 1_000, share: 0.001)]
        let instruments = ReportMath.allocationView(named, ctx, today: today)
        XCTAssertEqual(instruments.holdings[0].subtitle, "ETF · IWDA · 5,520.50 EUR · Broker, Pension · Share 55% · Unrealised gain +800.00 · Return on cost +15.4%")
        XCTAssertEqual(instruments.holdings[1].subtitle, "Government bond · T 4 1/4 · Broker · Share 23%")
        XCTAssertEqual(instruments.byCurrency.rows.map(\.label), ["USD", "EUR", "GBP"])
        XCTAssertEqual(instruments.byCurrency.rows.map(\.note), [nil, "920.40 EUR", nil])

        guard case .investmentIncome(let income)? = ReportMath.investmentsView("investmentIncome", held, ctx, today: today) else {
            return XCTFail("No income report")
        }
        XCTAssertEqual(income.tiles.map(\.label), ["Dividends and interest", "Fees, tax and commissions", "Net", "Yield"])
        XCTAssertEqual(income.tiles.map(\.value), ["+42.00", "-10.00", "+32.00", "0.4%"])
        XCTAssertTrue(income.reporting)
        XCTAssertEqual(income.chart?.series.map(\.label), ["Dividends", "Interest", "Tax withheld", "Fees"])
        XCTAssertEqual(income.months.map(\.value), ["+29.50", "+6.00"])
        XCTAssertEqual(income.months[0].subtitle, "Dividends +30.00 · Interest +5.00 · Tax withheld -4.50 · Fees -1.00")
        XCTAssertEqual(income.lines.map(\.title), ["Dividends", "Interest", "Tax withheld", "Fees", "Commissions on trades", "Net"])
        XCTAssertEqual(income.lines[4].subtitle, "Broker · 2025-10-21 to 2026-10-20")
        XCTAssertEqual(income.lines.last?.value, "+32.00")
        XCTAssertEqual(income.note, "No income or costs reported: Pension.")
        // One account reporting is the totals already; with two, each has its row, and the rows add up to the tiles.
        XCTAssertTrue(income.accounts.isEmpty)
        var two = held
        two.income.byAccount.append(PortfolioIncomeAccount(accountId: "pen", dividends: 500, interest: 0, tax: -75, fees: 0, other: 0))
        two.income.dividends += 500
        two.income.tax -= 75
        let byAccount = ReportMath.investmentIncomeView(two, ctx)
        XCTAssertEqual(byAccount.accounts.map(\.title), ["Broker", "Pension"])
        // The broker's commissions count in its costs; the pension has none on record. -10.00 and -0.75 are the
        // tile's -10.75, and +32.00 and +4.25 its net.
        XCTAssertEqual(byAccount.accounts.map(\.value), ["+32.00", "+4.25"])
        XCTAssertEqual(byAccount.accounts.map(\.subtitle), ["Dividends and interest +42.00 · Fees, tax and commissions -10.00",
                                                            "Dividends and interest +5.00 · Fees, tax and commissions -0.75"])
        XCTAssertEqual(byAccount.tiles.prefix(3).map(\.value), ["+47.00", "-10.75", "+36.25"])
        // The same once the broker's commissions are known by the month: -10.70 and -0.75 are -11.45.
        two.income.months[1].commissions = -420
        two.income.commissions = []
        two.income.byAccount[0].commissions = -420
        let byMonth = ReportMath.investmentIncomeView(two, ctx)
        XCTAssertEqual(byMonth.accounts.map(\.value), ["+31.30", "+4.25"])
        XCTAssertEqual(byMonth.accounts[0].subtitle, "Dividends and interest +42.00 · Fees, tax and commissions -10.70")
        XCTAssertEqual(byMonth.tiles.prefix(3).map(\.value), ["+47.00", "-11.45", "+35.55"])

        // With trades on record, what they realised is under the gain, and their commissions are a
        // series and a line of their own, counted with the statement totals of the accounts still listed.
        XCTAssertNil(performance.tiles[1].note)
        XCTAssertTrue(income.holdings.isEmpty)
        var traded = held
        traded.total.realised = 1_200
        XCTAssertEqual(ReportMath.performanceView(traded, ctx, today: today).tiles[1].note, "Realised +12.00")
        traded.income.months[1].commissions = -420
        traded.income.commissions = [PortfolioCommission(accountId: "pen", from: "2026-01-01", to: "2026-10-20", amount: -100)]
        traded.income.byHolding = [
            PortfolioIncomeHolding(key: "IE00B4L5Y983", symbol: "IWDA", name: "World Fund", dividends: 3_000, tax: -450, value: 600_000, yieldRate: 0.005),
            PortfolioIncomeHolding(key: "OLD USD", symbol: "OLD", name: "Sold", dividends: 500, tax: 0, value: nil, yieldRate: nil),
        ]
        let trades = ReportMath.investmentIncomeView(traded, ctx)
        XCTAssertEqual(trades.tiles.prefix(3).map(\.value), ["+42.00", "-11.70", "+30.30"])
        XCTAssertEqual(trades.chart?.series.map(\.label), ["Dividends", "Interest", "Tax withheld", "Fees", "Commissions on trades"])
        XCTAssertEqual(trades.chart?.series.last?.values, [0, -420])
        XCTAssertEqual(trades.months.map(\.value), ["+29.50", "+1.80"])
        XCTAssertEqual(trades.months[1].subtitle, "Interest +7.00 · Fees -1.00 · Commissions on trades -4.20")
        XCTAssertEqual(trades.lines.map(\.title), ["Dividends", "Interest", "Tax withheld", "Fees", "Commissions on trades", "Commissions on trades", "Net"])
        XCTAssertEqual(trades.lines.map(\.value), ["+30.00", "+12.00", "-4.50", "-2.00", "-4.20", "-1.00", "+30.30"])
        XCTAssertNil(trades.lines[4].subtitle)
        XCTAssertEqual(trades.lines[5].subtitle, "Pension · 2026-01-01 to 2026-10-20")
        // What each holding paid, with its symbol and the tax withheld beneath.
        XCTAssertEqual(trades.holdings.map(\.title), ["World Fund", "Sold"])
        XCTAssertEqual(trades.holdings.map(\.value), ["+30.00", "+5.00"])
        XCTAssertEqual(trades.holdings.map(\.subtitle), ["IWDA · Tax withheld -4.50", "OLD"])

        // The overview's card: the value, and the gain over its months.
        let card = try XCTUnwrap(ReportMath.investmentsCard(held, ctx, current: "2026-10"))
        XCTAssertEqual(card.id, "investments")
        XCTAssertEqual(card.figure, "11,000.00")
        XCTAssertEqual(card.note, "Gain +500.00 · +5.0%")
        XCTAssertEqual(card.noteTone, .positive)
        XCTAssertEqual(card.bars?.series.first?.values, [1_000_000, 1_100_000])
        XCTAssertEqual(card.bars?.partial, 1)

        // With no account with holdings chosen there is nothing to show.
        var none = held
        none.accounts = []
        XCTAssertNil(ReportMath.investmentsView("performance", none, ctx, today: today))
        XCTAssertNil(ReportMath.investmentsCard(none, ctx))
        XCTAssertNil(ReportMath.investmentsView("networth", held, ctx, today: today))
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
