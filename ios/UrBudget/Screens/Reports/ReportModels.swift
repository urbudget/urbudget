import Foundation

// MARK: Report models

struct ReportTile: Hashable, Sendable {
    var label: String
    var value: String
    var note: String? = nil
    var tone: ReportTone = .none
}

struct ReportMiniSeries: Hashable, Sendable {
    var label: String
    var color: Int
    var values: [Int64]
    /// Each value as text, for VoiceOver.
    var texts: [String]
}

struct ReportMiniBarsModel: Hashable, Sendable {
    var labels: [String]
    var series: [ReportMiniSeries]
    var top: Int64 { Swift.max(0, series.flatMap(\.values).max() ?? 0) }
    var bottom: Int64 { Swift.min(0, series.flatMap(\.values).min() ?? 0) }
}

struct ReportSlice: Identifiable, Hashable, Sendable {
    var key: String
    var label: String
    var value: Int64
    var valueText: String
    var color: Int
    var id: String { key }
}

struct ReportBarRow: Identifiable, Hashable, Sendable {
    var key: String
    var label: String
    var valueText: String
    var detail: String?
    var color: Int?
    var fraction: Double
    var drill = false
    var id: String { key }
}

struct ReportDivRow: Identifiable, Hashable, Sendable {
    var key: String
    var label: String
    var valueText: String
    var detail: String
    var fraction: Double
    var color: Int
    var id: String { key }
}

struct ReportTreemapItem: Identifiable, Hashable, Sendable {
    var key: String
    var label: String
    var value: Int64
    var valueText: String
    var detail: String
    var more: String
    var level: Int
    var id: String { key }
}

/// What a report figure's transactions are: a category, payee, day or income source over a range.
struct ReportDrillTarget: Hashable, Sendable {
    var title: String
    var dimension: String
    var key: String
    var from: String? = nil
    var to: String? = nil
}

enum ReportRowStyle: Hashable, Sendable { case plain, section, group, child, total, net, muted }

/// A row of a report's list: a month, an account, a source.
struct ReportFigureRow: Identifiable, Hashable, Sendable {
    var key: String
    var title: String
    var value = ""
    var subtitle: String? = nil
    var tone: ReportTone = .none
    var style: ReportRowStyle = .plain
    var drill: ReportDrillTarget? = nil
    var expand: String? = nil
    var id: String { key }
}

struct ReportChanges: Hashable, Sendable {
    var title: String
    var aside: String
    var rows: [ReportDivRow]
    var legend: [ReportLegendItem]
}

struct ReportCategoryModel: Hashable, Sendable {
    var group: String?
    var tiles: [ReportTile]
    var slices: [ReportSlice]
    var centerValue: String
    var centerLabel: String
    var rows: [ReportBarRow]
    var help: String
    var changes: ReportChanges?
}

struct ReportPayeeRow: Identifiable, Hashable, Sendable {
    var key: String
    var name: String
    var label: String
    var amountText: String
    var detail: String
    var id: String { key }
}

struct ReportPayeeModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var title: String
    var aside: String
    var map: [ReportTreemapItem]
    var help: String
    var more: String?
    var rows: [ReportPayeeRow]
}

struct ReportOverTimeModel: Hashable, Sendable {
    var byGroup: Bool
    var tiles: [ReportTile]
    var chart: ReportColumnModel
    var legend: [ReportLegendItem]
    var rows: [ReportFigureRow]
}

struct ReportHeatMonth: Hashable, Sendable {
    var index: Int
    var text: String
}

struct ReportHeatModel: Hashable, Sendable {
    var days: [String]
    var levels: [Int]
    var lead: Int
    var weeks: Int
    var months: [ReportHeatMonth]
    var weekdayLabels: [String]
    var titles: [String]
    var amounts: [String]
    var spent: [Bool]
}

struct ReportCalendarModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var heat: ReportHeatModel?
    var weekdays: ReportColumnModel
    var biggest: [ReportFigureRow]
}

struct ReportIncomeExpenseModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var help: String
    var chart: ReportColumnModel
    var legend: [ReportLegendItem]
    var rate: ReportLinesModel
    var cumulative: ReportLinesModel
    var table: [ReportFigureRow]
    var children: [String: [ReportFigureRow]]
    var months: [ReportFigureRow]
}

struct ReportIncomeModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var slices: [ReportSlice]
    var centerValue: String
    var bars: [ReportBarRow]
    var chartHelp: String?
    var monthly: ReportColumnModel?
    var legend: [ReportLegendItem]
    var sources: [ReportFigureRow]
    var sourcesHelp: String?
}

struct ReportCashFlowModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var chart: ReportColumnModel
    var legend: [ReportLegendItem]
    var running: ReportLinesModel
    var months: [ReportFigureRow]
}

struct ReportNetWorthModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var chart: ReportColumnModel
    var legend: [ReportLegendItem]
    var change: ReportColumnModel
    var changeLegend: [ReportLegendItem]
    var dates: String
    var accounts: [ReportFigureRow]
}

struct ReportBalanceSheetModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var ownTotal: String
    var own: [ReportBarRow]
    var oweTotal: String
    var owe: [ReportBarRow]
    var statementAside: String
    var dates: String
    var statement: [ReportFigureRow]
    var help: String
}

struct ReportAccountChip: Identifiable, Hashable, Sendable {
    var id: String
    var name: String
    var slot: Int?
    var enabled: Bool
}

struct ReportBalancesModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var chips: [ReportAccountChip]
    var chosen: [String: Int]
    var chart: ReportLinesModel?
    var legend: [ReportLegendItem]
    var rows: [ReportFigureRow]
}

struct ReportAgeModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var chart: ReportLinesModel
    var rows: [ReportFigureRow]
}

/// A report's body, ready to lay out. Forecast has its own data and model.
enum ReportBody: Hashable, Sendable {
    case categories(ReportCategoryModel)
    case payees(ReportPayeeModel)
    case trends(ReportOverTimeModel)
    case calendar(ReportCalendarModel)
    case incomeExpense(ReportIncomeExpenseModel)
    case income(ReportIncomeModel)
    case cashflow(ReportCashFlowModel)
    case networth(ReportNetWorthModel)
    case balance(ReportBalanceSheetModel)
    case balances(ReportBalancesModel)
    case age(ReportAgeModel)

    var id: String {
        switch self {
        case .categories: return "categories"
        case .payees: return "payees"
        case .trends: return "trends"
        case .calendar: return "calendar"
        case .incomeExpense: return "incomeExpense"
        case .income: return "income"
        case .cashflow: return "cashflow"
        case .networth: return "networth"
        case .balance: return "balance"
        case .balances: return "balances"
        case .age: return "age"
        }
    }
}

/// What a report needs besides the engine's figures: its range, accounts and today.
struct ReportScope: Hashable, Sendable {
    var from: String
    var to: String
    var accounts: Set<String>
    var someCategories: Bool
    /// Today, as an ISO day.
    var today: String
    var weekStart: Int
}

/// A report's own choices: the group drilled into, by group or total, the accounts on the chart.
struct ReportOptions: Hashable, Sendable {
    var group: String? = nil
    var byGroup = true
    var chartAccounts: [String: Int]? = nil
}

/// An account on the balance sheet: its section, its side, and balances as amounts held or owed.
struct ReportSheetLine: Equatable, Sendable {
    var account: ReportAccount
    var section: String
    var debt: Bool
    var start: Int64
    var end: Int64
}

struct ReportSpendRow: Identifiable, Hashable, Sendable {
    var key: String
    var label: String
    var valueText: String
    var color: Int
    var weight: Int64
    var id: String { key }
}

struct ReportOverviewModel: Hashable, Sendable {
    var month: String
    var monthName: String
    var spendingText: String
    var rows: [ReportSpendRow]
    var nothingSpent: Bool
    var refundLead: String?
    var refunds: [ReportSpendRow]
    var incomeLead: String
    var incomeBars: ReportMiniBarsModel
    var netWorthText: String
    var netWorthNegative: Bool
    var assetsText: String
    var debtsText: String
    var debtsNegative: Bool
    var netWorthBars: ReportMiniBarsModel
    var ageText: String
    var ageBars: ReportMiniBarsModel
}

struct ReportForecastEvent: Equatable, Sendable {
    var date: String
    var amount: Int64
    var payee: String
    var accountId: String
    var peerId: String?
    var categoryId: String?
    var scheduleId: String?
}

struct ReportForecastData: Sendable {
    var balances: ReportAmounts
    var events: [ReportForecastEvent]
    var recentSpending: Int64
}

/// The known part of a forecast: chosen accounts, their events and the balance at the end of each day.
struct ReportForecastPlan: Sendable {
    var ids: [String]
    var events: [ReportForecastEvent]
    var opening: Int64
    var scheduledSpending: Double
    var suggested: Int64
    var days: [String]
    var knownLine: [Int64]
    var balances: [Int64]
}

struct ReportForecastRow: Identifiable, Hashable, Sendable {
    var key: String
    var title: String
    var detail: String
    var amount: String
    var amountPositive: Bool
    var balance: String
    var balanceNegative: Bool
    var id: String { key }
}

struct ReportForecastModel: Hashable, Sendable {
    var tiles: [ReportTile]
    var chart: ReportLinesModel
    var legend: [ReportLegendItem]
    var everyday: Int64
    var everydayText: String
    var suggestion: String?
    var help: String
    var count: String
    var rows: [ReportForecastRow]
}

struct ReportDrillLine: Identifiable, Hashable, Sendable {
    var key: String
    var transactionId: String
    var payee: String
    var detail: String
    var amount: String
    var positive: Bool
    var id: String { key }
}

struct ReportDrillModel: Sendable {
    var range: String
    var figureLabel: String
    var figure: String
    var count: String
    var average: String
    var lines: [ReportDrillLine]
    /// The saved records the lines open, from the snapshot the figures came from.
    var records: [String: JSON]
}

private struct ReportDrillEntry {
    var transaction: JSON
    var index: Int
    var date: String
    var payee: String
    var account: String
    var category: String
    var amount: Int64
}

// MARK: Builders

extension ReportMath {
    private static let moneyAxis: (Double) -> String = { compactMoney($0) }

    private static func chosen(_ accounts: Set<String>, _ id: String) -> Bool { accounts.isEmpty || accounts.contains(id) }

    /// Totals by group, or by category within one group.
    static func totalsBy(_ categories: ReportAmounts, _ ctx: ReportBudget, group: String?) -> ReportAmounts {
        var out = ReportAmounts()
        for (id, value) in categories.entries {
            guard let g = ctx.groupOf[id], !g.isEmpty else { continue }
            let key: String? = group == nil ? g : (g == group ? id : nil)
            if let key { out.add(key, value) }
        }
        return out
    }

    private static func barRows(_ rows: [(key: String, label: String, value: Int64)], colors: (String) -> Int?, total: Int64?,
                                _ ctx: ReportBudget, drill: Bool) -> [ReportBarRow] {
        let most = Swift.max(1, rows.map(\.value).max() ?? 1)
        return rows.map { row in
            ReportBarRow(key: row.key, label: row.label, valueText: ctx.money(row.value), detail: total.map { share(row.value, $0) },
                         color: colors(row.key), fraction: Double(Swift.max(0, row.value)) / Double(most), drill: drill)
        }
    }

    /// Where spending went, by group or by category within one group, with the last month against the average.
    static func categoryView(_ data: ReportData, _ ctx: ReportBudget, group: String?, today: String) -> ReportCategoryModel {
        let n = Swift.max(1, data.months.count)
        var order: [String] = []
        if let group {
            var seen = Set<String>()
            for category in ctx.categories where category.group == group && !category.payment && seen.insert(category.id).inserted { order.append(category.id) }
            for id in data.categories.keys where ctx.groupOf[id] == group && seen.insert(id).inserted { order.append(id) }
        }
        var rank: [String: Int] = [:]
        for (index, id) in order.enumerated() { rank[id] = index }
        func categoryColor(_ id: String) -> Int {
            if let index = rank[id], index <= 6 { return index }
            return ReportColors.other
        }
        let totals = totalsBy(data.categories, ctx, group: group)
        struct Row { var key: String; var label: String; var value: Int64; var color: Int }
        let keys = group != nil ? totals.keys : ctx.spendingGroups
        let rows = keys.map { key in
            Row(key: key, label: group != nil ? (ctx.nameOf[key] ?? key) : key, value: totals[key] ?? 0,
                color: group != nil ? categoryColor(key) : ctx.colorOf(key))
        }.filter { $0.value != 0 }.reportSorted(descending: { $0.value })
        let total = rows.map(\.value).reportSum
        let other = rows.filter { $0.color == ReportColors.other && $0.value > 0 }
        var slices = rows.filter { $0.color != ReportColors.other && $0.value > 0 }
            .map { ReportSlice(key: $0.key, label: $0.label, value: $0.value, valueText: ctx.money($0.value), color: $0.color) }
        if !other.isEmpty {
            let sum = other.map(\.value).reportSum
            slices.append(ReportSlice(key: "__other", label: String(localized: "Other"), value: sum, valueText: ctx.money(sum), color: ReportColors.other))
        }
        let top = rows.first
        let allShare = share(total, data.spending)
        let tiles = [
            ReportTile(label: group.map { String(localized: "Spent in \($0)") } ?? String(localized: "Spending"), value: ctx.money(total),
                       note: group != nil ? String(localized: "\(allShare) of all spending") : String(localized: "over \(n) months", locale: .app)),
            ReportTile(label: String(localized: "Monthly average"), value: ctx.money(roundDiv(total, n))),
            ReportTile(label: group != nil ? String(localized: "Biggest category") : String(localized: "Biggest group"), value: top?.label ?? "—",
                       note: top.map { "\(ctx.money($0.value)) · \(share($0.value, total))" }),
            ReportTile(label: group != nil ? String(localized: "Categories with spending") : String(localized: "Groups with spending"),
                       value: "\(rows.filter { $0.value > 0 }.count)"),
        ]
        var colors: [String: Int] = [:]
        for row in rows { colors[row.key] = row.color }
        let select = group != nil ? String(localized: "Select a category to see its transactions.") : String(localized: "Select a group to see its categories.")
        let help = select + " " + String(localized: "Refunds reduce spending, so a category can be negative. The ring shows only groups or categories with net spending; the total and list include refunds.")
        return ReportCategoryModel(
            group: group, tiles: tiles, slices: slices, centerValue: ctx.money(total), centerLabel: group ?? String(localized: "Spent"),
            rows: barRows(rows.map { ($0.key, $0.label, $0.value) }, colors: { colors[$0] }, total: total, ctx, drill: true), help: help,
            changes: data.months.count >= 2 ? changesView(data, ctx, group: group, todayMonth: String(today.prefix(7))) : nil)
    }

    /// The last month of the range against the average of the months before it.
    static func changesView(_ data: ReportData, _ ctx: ReportBudget, group: String?, todayMonth: String) -> ReportChanges {
        let all = data.months
        let months = all.last?.month == todayMonth && all.count > 2 ? Array(all.dropLast()) : all
        guard let last = months.last else { return ReportChanges(title: "", aside: "", rows: [], legend: []) }
        let earlier = months.dropLast()
        let now = totalsBy(last.categories, ctx, group: group)
        var average: [String: Double] = [:]
        var averageKeys: [String] = []
        for month in earlier {
            for (key, value) in totalsBy(month.categories, ctx, group: group).entries {
                if average[key] == nil { averageKeys.append(key) }
                average[key, default: 0] += Double(value) / Double(earlier.count)
            }
        }
        var keys = now.keys
        for key in averageKeys where !now.contains(key) { keys.append(key) }
        struct Row { var key: String; var label: String; var value: Int64; var detail: String }
        let rows = keys.map { key -> Row in
            let avg = average[key] ?? 0
            let averageText = ctx.money(jround(avg))
            return Row(key: key, label: group != nil ? (ctx.nameOf[key] ?? key) : key, value: jround(Double(now[key] ?? 0) - avg),
                       detail: String(localized: "avg \(averageText)"))
        }.filter { $0.value != 0 }.reportSorted(descending: { $0.value.magnitude }).prefix(10)
        let most = Swift.max(1, rows.map { Int64(clamping: $0.value.magnitude) }.max() ?? 1)
        let partial = last.month == todayMonth
        let name = monthName(last.month)
        let earlierCount = earlier.count
        return ReportChanges(
            title: partial ? String(localized: "\(name) so far against your average") : String(localized: "\(name) against your average"),
            aside: String(localized: "the \(earlierCount)-month average before it", locale: .app),
            rows: rows.map {
                ReportDivRow(key: $0.key, label: $0.label, valueText: ctx.signed($0.value), detail: $0.detail,
                             fraction: Double($0.value) / Double(most),
                             color: $0.value > 0 ? ReportColors.divergingNegative : ReportColors.divergingPositive)
            },
            legend: [ReportLegendItem(label: String(localized: "Spent less than average"), color: ReportColors.divergingPositive),
                     ReportLegendItem(label: String(localized: "Spent more than average"), color: ReportColors.divergingNegative)])
    }

    /// The payees big enough to read as tiles (at least 1.5% of the set, 8 to 14 of them) and the
    /// rest, which share one "others" line. A single leftover payee keeps its own tile.
    static func foldPayees<T>(_ list: [T], amount: (T) -> Int64) -> (big: [T], rest: [T]) {
        let total = Double(list.map(amount).reportSum)
        var n = 0
        while n < list.count && n < 14 && (n < 8 || Double(amount(list[n])) / total >= 0.015) { n += 1 }
        return list.count - n <= 1 ? (list, []) : (Array(list.prefix(n)), Array(list.dropFirst(n)))
    }

    static func payeeView(_ data: ReportData, _ ctx: ReportBudget) -> ReportPayeeModel {
        let all = data.payees
        let total = all.map(\.amount).reportSum
        let count = data.transactionCount ?? all.map(\.count).reportSum
        let positive = all.filter { $0.amount > 0 }
        let positiveTotal = positive.map(\.amount).reportSum
        let refunds = -all.filter { $0.amount < 0 }.map(\.amount).reportSum
        let top = positive.first
        let (big, rest) = foldPayees(positive) { $0.amount }
        let restTotal = rest.map(\.amount).reportSum
        func average(_ p: ReportPayeeSpend) -> String {
            let amount = ctx.money(rounding: Double(p.amount) / Double(Swift.max(1, p.count)))
            return String(localized: "avg \(amount)")
        }
        func transactions(_ count: Int64) -> String { String(localized: "\(count) transactions", locale: .app) }
        func shown(_ part: Int64) -> String {
            let percent = share(part, positiveTotal)
            return String(localized: "\(percent) of spending shown")
        }
        let noPayee = String(localized: "No payee")
        let levels = heatLevels(big.map(\.amount))
        let tiles = [
            ReportTile(label: String(localized: "Spending"), value: ctx.money(total), note: String(localized: "at \(all.count) payees", locale: .app)),
            ReportTile(label: String(localized: "Top payee"), value: top.map { $0.name.isEmpty ? noPayee : $0.name } ?? "—",
                       note: top.map { "\(ctx.money($0.amount)) · \(shown($0.amount))" }),
            ReportTile(label: String(localized: "Average per transaction"), value: count != 0 ? ctx.money(rounding: Double(total) / Double(count)) : "—",
                       note: transactions(count)),
            ReportTile(label: String(localized: "Top 5 payees"), value: share(positive.prefix(5).map(\.amount).reportSum, positiveTotal),
                       note: String(localized: "of spending shown")),
        ]
        let map = big.map { p in
            ReportTreemapItem(key: p.name, label: p.name.isEmpty ? noPayee : p.name, value: p.amount, valueText: ctx.money(p.amount),
                              detail: shown(p.amount), more: "\(transactions(p.count)) · \(average(p))", level: levels.level(p.amount))
        }
        let rows = all.map { p in
            ReportPayeeRow(key: p.name, name: p.name, label: p.name.isEmpty ? noPayee : p.name, amountText: ctx.money(p.amount),
                           detail: "\(share(p.amount, total)) · \(transactions(p.count)) · \(average(p))")
        }
        let tilesNote = String(localized: "Bigger tiles, more spent")
        let help = String(localized: "Select a payee to see its transactions. Tiles show payees with net spending; the total and table include refunds.")
        let refundsText = ctx.money(refunds)
        let morePayees = String(localized: "\(rest.count) more payees", locale: .app)
        return ReportPayeeModel(
            tiles: tiles, title: rest.isEmpty ? String(localized: "Where your money goes") : String(localized: "Top \(big.count) payees", locale: .app),
            aside: rest.isEmpty ? tilesNote : "\(shown(positiveTotal &- restTotal)) · \(tilesNote)",
            map: map,
            help: help + (refunds > 0 ? " " + String(localized: "Net refunds of \(refundsText) at other payees reduce the total.") : ""),
            more: rest.isEmpty ? nil : "\(morePayees) · \(ctx.money(restTotal))",
            rows: rows)
    }

    /// The trend tile: the later half of the months against the earlier half.
    static func trendTile(_ totals: [Int64]) -> ReportTile {
        let half = totals.count / 2
        let recent = half > 0 ? Double(totals.suffix(half).reportSum) / Double(half) : 0
        let before = half > 0 ? Double(totals.prefix(half).reportSum) / Double(half) : 0
        let known = half > 0 && before != 0
        return ReportTile(label: String(localized: "Trend"),
                          value: known ? AppLanguage.leftToRight("\(recent >= before ? "+" : "")\(fixed((recent - before) / before * 100, 0))%") : "—",
                          note: half > 0 ? String(localized: "last \(half) months against the \(half) before", locale: .app)
                              : String(localized: "needs two months or more"),
                          tone: known ? (recent > before ? .negative : .positive) : .none)
    }

    static func overTimeView(_ data: ReportData, _ ctx: ReportBudget, byGroup: Bool) -> ReportOverTimeModel {
        let keys = data.months.map(\.month)
        var series: [ReportColumnSeries] = []
        for group in ctx.spendingGroups {
            let color = ctx.colorOf(group)
            let label = color == ReportColors.other ? String(localized: "Other") : group
            let values = data.months.map { m in Double(m.categories.entries.filter { ctx.groupOf[$0.key] == group }.map(\.value).reportSum) }
            if let existing = series.firstIndex(where: { $0.label == label }) {
                series[existing].values = zip(series[existing].values, values).map { $0 + $1 }
            } else {
                series.append(ReportColumnSeries(key: group, label: label, color: color, values: values))
            }
        }
        let shown = series.filter { $0.values.contains { $0 != 0 } }
        let totals = data.months.map(\.spending)
        let average = totals.isEmpty ? 0 : roundDiv(totals.reportSum, totals.count)
        let byAmount = data.months.reportSorted(descending: { $0.spending })
        let chart = columnModel(label: byGroup ? String(localized: "Spending by month, stacked by category group") : String(localized: "Spending by month"),
                                keys: keys,
                                series: byGroup ? shown
                                    : [ReportColumnSeries(key: "total", label: String(localized: "Spending"), color: 0, values: totals.map { Double($0) })],
                                format: { ctx.money(rounding: $0) }, axis: moneyAxis, unit: ctx.axisUnit,
                                average: (String(localized: "Average"), Double(average)))
        var rows = data.months.enumerated().map { i, m in
            let parts = shown.filter { $0.values[i] != 0 }.map { "\($0.label) \(ctx.money(rounding: $0.values[i]))" }.joined(separator: " · ")
            return ReportFigureRow(key: m.month, title: monthName(m.month, short: true), value: ctx.money(m.spending),
                                   subtitle: byGroup && !parts.isEmpty ? parts : nil)
        }
        let averages = shown.map { s in "\(s.label) \(ctx.money(rounding: s.values.reduce(0, +) / Double(Swift.max(1, keys.count))))" }.joined(separator: " · ")
        rows.append(ReportFigureRow(key: "__average", title: String(localized: "Average"), value: ctx.money(average),
                                    subtitle: byGroup && !averages.isEmpty ? averages : nil, style: .total))
        return ReportOverTimeModel(byGroup: byGroup, tiles: [
            ReportTile(label: String(localized: "Monthly average"), value: ctx.money(average)),
            ReportTile(label: String(localized: "Highest month"), value: byAmount.first.map { ctx.money($0.spending) } ?? "—",
                       note: byAmount.first.map { monthName($0.month) }),
            ReportTile(label: String(localized: "Lowest month"), value: byAmount.last.map { ctx.money($0.spending) } ?? "—",
                       note: byAmount.last.map { monthName($0.month) }),
            trendTile(totals),
        ], chart: chart, legend: byGroup ? shown.map { ReportLegendItem(label: $0.label, color: $0.color) } : [], rows: rows)
    }

    /// Every day of the range up to today, at most 3,700 of them.
    static func calendarDays(from: String, to: String, today: String) -> [String] {
        guard dayParts(firstDay(from)) != nil, dayParts(lastDay(to)) != nil else { return [] }
        return days(from: firstDay(from), through: Swift.min(lastDay(to), today), limit: 3700)
    }

    /// Average spending on each weekday, starting from the week's first day.
    static func weekdayAverages(_ days: [String], _ values: ReportAmounts, weekStart: Int) -> [Int64] {
        var totals = [Int64](repeating: 0, count: 7)
        var counts = [Int](repeating: 0, count: 7)
        for day in days {
            let weekday = jsWeekday(day)
            totals[weekday] = totals[weekday] &+ (values[day] ?? 0)
            counts[weekday] += 1
        }
        return (0..<7).map { i in
            let weekday = (i + weekStart) % 7
            return counts[weekday] > 0 ? roundDiv(totals[weekday], counts[weekday]) : 0
        }
    }

    static func calendarView(_ data: ReportData, _ ctx: ReportBudget, _ scope: ReportScope) -> ReportCalendarModel {
        let values = data.days
        let days = calendarDays(from: scope.from, to: scope.to, today: scope.today)
        func spentOn(_ day: String) -> Int64 { values[day] ?? 0 }
        let spent = days.filter { spentOn($0) > 0 }
        let busiest = spent.reportSorted(descending: spentOn)
        let weekdays = (0..<7).map { ($0 + scope.weekStart) % 7 }
        let byWeekday = weekdayAverages(days, values, weekStart: scope.weekStart)
        let top = byWeekday.firstIndex(of: byWeekday.max() ?? 0) ?? 0
        func dayTitle(_ day: String) -> String { "\(weekdayName(jsWeekday(day))) \(Money.date(day))" }
        let noSpend = days.count - spent.count
        let ofDays = String(localized: "of \(days.count) days", locale: .app)
        let topAverage = ctx.money(byWeekday[top])
        let tiles = [
            ReportTile(label: String(localized: "Average a day"), value: days.isEmpty ? "—" : ctx.money(roundDiv(days.map(spentOn).reportSum, days.count))),
            ReportTile(label: String(localized: "No-spend days"), value: "\(noSpend)", note: "\(ofDays) · \(share(Int64(noSpend), Int64(days.count)))"),
            ReportTile(label: String(localized: "Biggest day"), value: busiest.first.map { ctx.money(spentOn($0)) } ?? "—", note: busiest.first.map { Money.date($0) }),
            ReportTile(label: String(localized: "Busiest weekday"), value: spent.isEmpty ? "—" : weekdayName(weekdays[top]),
                       note: spent.isEmpty ? nil : String(localized: "\(topAverage) on average")),
        ]
        var heat: ReportHeatModel?
        if let first = days.first {
            let scale = heatLevels(days.map(spentOn))
            let lead = (jsWeekday(first) - scope.weekStart + 7) % 7
            var months: [ReportHeatMonth] = []
            for (index, day) in days.enumerated() where index == 0 || day.hasSuffix("-01") {
                months.append(ReportHeatMonth(index: index, text: monthAbbrev(day)))
            }
            heat = ReportHeatModel(days: days, levels: days.map { scale.level(spentOn($0)) }, lead: lead,
                                   weeks: Int((Double(days.count + lead) / 7).rounded(.up)), months: months,
                                   weekdayLabels: [1, 3, 5].map { weekdayName(($0 + scope.weekStart) % 7, short: true) },
                                   titles: days.map(dayTitle), amounts: days.map { ctx.money(spentOn($0)) }, spent: days.map { spentOn($0) > 0 })
        }
        let weekdayChart = columnModel(label: String(localized: "Average spending on each day of the week"), keys: weekdays.map(String.init),
                                       series: [ReportColumnSeries(key: "avg", label: String(localized: "Average"), color: 0, values: byWeekday.map { Double($0) })],
                                       format: { ctx.money(rounding: $0) }, axis: moneyAxis, unit: ctx.axisUnit,
                                       xTick: { key, _ in plainTick(weekdayName(Int(key) ?? 0, short: true)) }, title: { weekdayName(Int($0) ?? 0) })
        let biggest = busiest.prefix(10).map { day in
            ReportFigureRow(key: day, title: Money.date(day), value: ctx.money(spentOn(day)), subtitle: weekdayName(jsWeekday(day), short: true),
                            drill: ReportDrillTarget(title: dayTitle(day), dimension: "day", key: day, from: day, to: day))
        }
        return ReportCalendarModel(tiles: tiles, heat: heat, weekdays: weekdayChart, biggest: biggest)
    }

    /// Income against spending, the surplus and its rate, month by month.
    static func incomeExpenseView(_ data: ReportData, _ ctx: ReportBudget, someCategories: Bool) -> ReportIncomeExpenseModel {
        let keys = data.months.map(\.month)
        let n = Swift.max(1, keys.count)
        let net = data.months.map(\.netIncome)
        let kept = data.income &- data.spending
        let rates: [Double?] = data.months.map { $0.income > 0 ? Double(jround(Double($0.netIncome) / Double($0.income) * 1000)) / 10 : nil }
        var running: Int64 = 0
        let cumulative = net.map { value -> Int64 in
            running = running &+ value
            return running
        }
        let saved = net.filter { $0 > 0 }.count
        var best: ReportMonth?
        for month in data.months where best.map({ month.netIncome > $0.netIncome }) ?? true { best = month }
        let rateText = data.income > 0 ? AppLanguage.leftToRight("\(fixed(Double(kept) / Double(data.income) * 100, 1))%") : "—"
        let format: (Double) -> String = { ctx.money(rounding: $0) }
        let percent: (Double) -> String = { AppLanguage.leftToRight("\(jsNumber($0))%") }
        func aMonth(_ amount: String) -> String { String(localized: "\(amount) a month") }
        let income = String(localized: "Income")
        let spending = String(localized: "Spending")
        let surplus = String(localized: "Budget surplus")
        let surplusRate = String(localized: "Budget surplus rate")
        var bestNote: String?
        if let best, best.netIncome > 0 {
            let name = monthName(best.month)
            bestNote = String(localized: "best: \(name)")
        }
        let tiles = [
            ReportTile(label: income, value: ctx.money(data.income), note: aMonth(ctx.money(roundDiv(data.income, n)))),
            ReportTile(label: spending, value: ctx.money(data.spending), note: aMonth(ctx.money(roundDiv(data.spending, n)))),
            ReportTile(label: surplus, value: ctx.money(kept), note: aMonth(ctx.signed(roundDiv(kept, n))), tone: kept < 0 ? .negative : .positive),
            ReportTile(label: surplusRate, value: rateText, note: String(localized: "surplus as a share of income")),
            ReportTile(label: String(localized: "Positive-surplus months"), value: String(localized: "\(saved) of \(data.months.count)", locale: .app),
                       note: bestNote),
        ]
        let help = String(localized: "Budget surplus compares income with spending. Categorized transfers to tracking investments and loans reduce it; use Net worth to see changes in wealth.") +
            (someCategories ? " " + String(localized: "Income stays included; spending reflects the chosen categories.") : "")
        let chart = columnModel(label: String(localized: "Income and spending side by side each month, with budget surplus as a line"), keys: keys, series: [
            ReportColumnSeries(key: "income", label: income, color: 0, values: data.months.map { Double($0.income) }),
            ReportColumnSeries(key: "spending", label: spending, color: 1, values: data.months.map { Double($0.spending) }),
        ], format: format, axis: moneyAxis, unit: ctx.axisUnit, grouped: true,
           line: ReportLineOverlay(label: surplus, color: ReportColors.ink, values: net.map { Double($0) }))
        let legend = [ReportLegendItem(label: income, color: 0), ReportLegendItem(label: spending, color: 1),
                      ReportLegendItem(label: surplus, color: ReportColors.ink, line: true)]
        let rate = linesModel(label: String(localized: "Budget surplus each month, as a percentage of income"), keys: keys,
                              series: [ReportLineSeries(key: "rate", label: surplusRate, color: 0, values: rates)],
                              format: percent, axis: percent, unit: nil)
        let cumulativeChart = linesModel(label: String(localized: "Budget surplus added up month by month"), keys: keys,
                                         series: [ReportLineSeries(key: "surplus", label: String(localized: "Cumulative budget surplus"), color: 2,
                                                                   values: cumulative.map { Double($0) })],
                                         format: format, axis: moneyAxis, unit: ctx.axisUnit)
        func totalRow(_ key: String, _ title: String, _ values: [Int64], _ style: ReportRowStyle, expand: String? = nil, tone: Bool = false) -> ReportFigureRow {
            let sum = values.reportSum
            let average = ctx.money(roundDiv(sum, n))
            return ReportFigureRow(key: key, title: title, value: ctx.money(sum), subtitle: String(localized: "avg \(average) a month"),
                                   tone: tone && sum < 0 ? .negative : .none, style: style, expand: expand)
        }
        let sources = data.incomeSources.entries.reportSorted(descending: { $0.value })
        struct GroupRow { var name: String; var values: [Int64]; var categories: [(id: String, values: [Int64])] }
        let groupRows = ctx.spendingGroups.map { group -> GroupRow in
            let ids = data.categories.keys.filter { ctx.groupOf[$0] == group }
            let values = data.months.map { m in ids.map { m.categories[$0] ?? 0 }.reportSum }
            let categories = ids.map { id in (id: id, values: data.months.map { $0.categories[id] ?? 0 }) }.reportSorted(descending: { $0.values.reportSum })
            return GroupRow(name: group, values: values, categories: categories)
        }.filter { $0.values.contains { $0 != 0 } }
        var table: [ReportFigureRow] = [ReportFigureRow(key: "__income", title: income, style: .section)]
        for source in sources {
            table.append(totalRow("in:\(source.key)", source.key.isEmpty ? String(localized: "No payee") : source.key,
                                  data.months.map { $0.incomeSources[source.key] ?? 0 }, .plain))
        }
        table.append(totalRow("__totalIncome", String(localized: "Total income"), data.months.map(\.income), .total, tone: true))
        table.append(ReportFigureRow(key: "__spending", title: spending, style: .section))
        for group in groupRows { table.append(totalRow("g:\(group.name)", group.name, group.values, .group, expand: group.name)) }
        table.append(totalRow("__totalSpending", String(localized: "Total spending"), data.months.map(\.spending), .total, tone: true))
        table.append(totalRow("__surplus", surplus, net, .net, tone: true))
        table.append(ReportFigureRow(key: "__rate", title: surplusRate, value: rateText, tone: kept < 0 && data.income > 0 ? .negative : .none, style: .net))
        var children: [String: [ReportFigureRow]] = [:]
        for group in groupRows {
            children[group.name] = group.categories.map { totalRow("c:\($0.id)", ctx.nameOf[$0.id] ?? $0.id, $0.values, .child) }
        }
        let months = data.months.enumerated().map { i, m -> ReportFigureRow in
            let monthIncome = ctx.money(m.income)
            let monthSpending = ctx.money(m.spending)
            let monthRate = rates[i].map { AppLanguage.leftToRight("\(jsNumber($0))%") } ?? "—"
            return ReportFigureRow(key: m.month, title: monthName(m.month, short: true), value: ctx.money(m.netIncome),
                                   subtitle: String(localized: "Income \(monthIncome) · Spending \(monthSpending) · Rate \(monthRate)"),
                                   tone: m.netIncome < 0 ? .negative : .none)
        }
        return ReportIncomeExpenseModel(tiles: tiles, help: help, chart: chart, legend: legend, rate: rate, cumulative: cumulativeChart,
                                        table: table, children: children, months: months)
    }

    /// Where income comes from: sources ranked, as a whole and month by month.
    static func incomeView(_ data: ReportData, _ ctx: ReportBudget) -> ReportIncomeModel {
        let n = Swift.max(1, data.months.count)
        let noPayee = String(localized: "No payee")
        let all = data.incomeSources.entries.map { (key: $0.key, label: $0.key.isEmpty ? noPayee : $0.key, value: $0.value) }
        let positive = all.filter { $0.value > 0 }.reportSorted(descending: { $0.value })
        let negative = all.filter { $0.value < 0 }.reportSorted(ascending: { $0.value })
        let coloured = positive.enumerated().map { i, s in
            ReportSlice(key: s.key, label: s.label, value: s.value, valueText: ctx.money(s.value), color: i < 7 ? i : ReportColors.other)
        }
        let rest = coloured.dropFirst(7)
        let restTotal = rest.map(\.value).reportSum
        let folded = rest.isEmpty ? coloured : Array(coloured.prefix(7)) + [
            ReportSlice(key: "__other", label: String(localized: "Other sources"), value: restTotal, valueText: ctx.money(restTotal), color: ReportColors.other),
        ]
        let total = positive.map(\.value).reportSum
        let topKeys = Set(coloured.prefix(7).map(\.key))
        let series = folded.map { slice in
            ReportColumnSeries(key: slice.key, label: slice.label, color: slice.color, values: data.months.map { m in
                slice.key == "__other"
                    ? Double(m.incomeSources.entries.filter { !topKeys.contains($0.key) && $0.value > 0 }.map(\.value).reportSum)
                    : Double(Swift.max(0, m.incomeSources[slice.key] ?? 0))
            })
        }
        let top = positive.first
        let taken = -negative.map(\.value).reportSum
        let takenText = ctx.money(taken)
        let withIncome = data.months.filter { $0.income > 0 }.count
        let tiles = [
            ReportTile(label: String(localized: "Income"), value: ctx.money(data.income),
                       note: negative.isEmpty ? String(localized: "from \(positive.count) sources", locale: .app) : String(localized: "after \(takenText) taken out")),
            ReportTile(label: String(localized: "Monthly average"), value: ctx.money(roundDiv(data.income, n))),
            ReportTile(label: String(localized: "Largest source"), value: top?.label ?? "—", note: top.map { "\(ctx.money($0.value)) · \(share($0.value, total))" }),
            ReportTile(label: String(localized: "Months with income"), value: String(localized: "\(withIncome) of \(data.months.count)", locale: .app)),
        ]
        let most = Swift.max(1, coloured.map(\.value).max() ?? 1)
        let bars = coloured.map {
            ReportBarRow(key: $0.key, label: $0.label, valueText: ctx.money($0.value), detail: share($0.value, total), color: $0.color,
                         fraction: Double($0.value) / Double(most))
        }
        let monthly = positive.isEmpty ? nil : columnModel(label: String(localized: "Income each month, stacked by source"), keys: data.months.map(\.month),
                                                           series: series, format: { ctx.money(rounding: $0) }, axis: moneyAxis, unit: ctx.axisUnit)
        let sources = (positive + negative).map { s -> ReportFigureRow in
            let months = data.months.filter { ($0.incomeSources[s.key] ?? 0) != 0 }.count
            let average = ctx.money(roundDiv(s.value, n))
            let part = s.value > 0 ? share(s.value, total) : "—"
            return ReportFigureRow(key: "s:\(s.key)", title: s.label, value: ctx.money(s.value),
                                   subtitle: String(localized: "Average \(average) · Share \(part) · Months \(months)", locale: .app),
                                   tone: s.value < 0 ? .negative : .none, drill: ReportDrillTarget(title: s.label, dimension: "income", key: s.key))
        }
        return ReportIncomeModel(
            tiles: tiles, slices: folded, centerValue: ctx.money(total), bars: bars,
            chartHelp: negative.isEmpty ? nil
                : String(localized: "The charts show positive source contributions. The income total and source table include amounts taken out of Unassigned."),
            monthly: monthly, legend: series.map { ReportLegendItem(label: $0.label, color: $0.color) }, sources: sources,
            sourcesHelp: negative.isEmpty ? nil
                : String(localized: "Negative sources are amounts explicitly taken from Unassigned, such as a reconciliation adjustment or a transfer to a tracking account assigned to Unassigned."))
    }

    /// Flows across the budget-account boundary, from recorded account movements.
    static func cashFlowView(_ data: ReportData, _ ctx: ReportBudget) -> ReportCashFlowModel {
        let keys = data.months.map(\.month)
        let n = Swift.max(1, keys.count)
        let inflow = data.months.map(\.inflow)
        let outflow = data.months.map(\.outflow)
        let net = zip(inflow, outflow).map { $0 &- $1 }
        var running: Int64 = 0
        let cumulative = net.map { value -> Int64 in
            running = running &+ value
            return running
        }
        let totalIn = inflow.reportSum
        let totalOut = outflow.reportSum
        let format: (Double) -> String = { ctx.money(rounding: $0) }
        let moneyIn = String(localized: "Money in")
        let moneyOut = String(localized: "Money out")
        let netFlow = String(localized: "Net flow")
        let chart = columnModel(label: String(localized: "Money into budget accounts above the line, money out below, with the net as a line"), keys: keys, series: [
            ReportColumnSeries(key: "in", label: moneyIn, color: 0, values: inflow.map { Double($0) }),
            ReportColumnSeries(key: "out", label: moneyOut, color: 1, values: outflow.map { -Double($0) }),
        ], format: format, axis: moneyAxis, unit: ctx.axisUnit, line: ReportLineOverlay(label: netFlow, color: ReportColors.ink, values: net.map { Double($0) }))
        var months = keys.enumerated().map { i, m -> ReportFigureRow in
            let monthIn = ctx.money(inflow[i])
            let monthOut = ctx.money(outflow[i])
            let runningTotal = ctx.money(cumulative[i])
            return ReportFigureRow(key: m, title: monthName(m, short: true), value: ctx.money(net[i]),
                                   subtitle: String(localized: "In \(monthIn) · Out \(monthOut) · Running total \(runningTotal)"),
                                   tone: net[i] < 0 ? .negative : .none)
        }
        let averageIn = ctx.money(roundDiv(totalIn, n))
        let averageOut = ctx.money(roundDiv(totalOut, n))
        months.append(ReportFigureRow(key: "__average", title: String(localized: "Average"), value: ctx.money(roundDiv(totalIn &- totalOut, n)),
                                      subtitle: String(localized: "In \(averageIn) · Out \(averageOut)"), style: .total))
        let blackMonths = net.filter { $0 > 0 }.count
        return ReportCashFlowModel(tiles: [
            ReportTile(label: moneyIn, value: ctx.money(totalIn), note: String(localized: "\(averageIn) a month")),
            ReportTile(label: moneyOut, value: ctx.money(totalOut), note: String(localized: "\(averageOut) a month")),
            ReportTile(label: netFlow, value: ctx.money(totalIn &- totalOut), tone: totalIn &- totalOut < 0 ? .negative : .positive),
            ReportTile(label: String(localized: "Months in the black"), value: String(localized: "\(blackMonths) of \(keys.count)", locale: .app)),
        ], chart: chart, legend: [ReportLegendItem(label: moneyIn, color: 0), ReportLegendItem(label: moneyOut, color: 1),
                                  ReportLegendItem(label: netFlow, color: ReportColors.ink, line: true)],
           running: linesModel(label: String(localized: "Net flow added up month by month"), keys: keys,
                               series: [ReportLineSeries(key: "running", label: String(localized: "Running total"), color: 2, values: cumulative.map { Double($0) })],
                               format: format, axis: moneyAxis, unit: ctx.axisUnit),
           months: months)
    }

    /// "1,000.00 → 1,250.00": a balance then and now (the arrow points the reading way in Arabic).
    private static func fromTo(_ start: String, _ end: String) -> String { String(localized: "\(start) → \(end)") }

    /// Net worth before the range starts, from the balances just before it.
    static func openingNetWorth(_ data: ReportData, _ accounts: [ReportAccount]) -> Int64 {
        guard let opening = data.openingAccounts else { return data.months.first?.netWorth ?? 0 }
        return accounts.filter { $0.inNetWorth && opening.contains($0.id) }.map { opening[$0.id] ?? 0 }.reportSum
    }

    static func netWorthView(_ data: ReportData, _ ctx: ReportBudget, accounts: Set<String>) -> ReportNetWorthModel {
        let keys = data.months.map(\.month)
        let first = data.months[0]
        let last = data.months[data.months.count - 1]
        let shownAccounts = ctx.accounts.filter { chosen(accounts, $0.id) }
        let opening = openingNetWorth(data, shownAccounts)
        let change = last.netWorth &- opening
        let monthly = data.months.enumerated().map { i, m in m.netWorth &- (i > 0 ? data.months[i - 1].netWorth : opening) }
        let format: (Double) -> String = { ctx.money(rounding: $0) }
        let startDate = Money.date(firstDay(first.month))
        let endDate = Money.date(lastDay(last.month))
        let netWorth = String(localized: "Net worth")
        let assets = String(localized: "Assets")
        let debts = String(localized: "Debts")
        let grew = String(localized: "Grew")
        let shrank = String(localized: "Shrank")
        var rows = shownAccounts.map { a -> ReportFigureRow in
            let start = data.openingAccounts?[a.id] ?? first.accounts[a.id] ?? 0
            let end = last.accounts[a.id] ?? 0
            let title = a.name + (a.archived ? " · " + String(localized: "closed") : "") + (a.inNetWorth ? "" : " · " + String(localized: "not in net worth"))
            return ReportFigureRow(key: a.id, title: title, value: ctx.money(end &- start), subtitle: fromTo(ctx.money(start), ctx.money(end)),
                                   tone: end &- start < 0 ? .negative : .none)
        }
        rows.append(ReportFigureRow(key: "__networth", title: netWorth, value: ctx.money(change),
                                    subtitle: fromTo(ctx.money(opening), ctx.money(last.netWorth)), style: .total))
        let percent = opening > 0 ? " · " + AppLanguage.leftToRight("\(change >= 0 ? "+" : "")\(fixed(Double(change) / Double(opening) * 100, 1))%") : ""
        let lastName = monthName(last.month)
        let firstName = monthName(first.month)
        let debtShare = share(last.debt, last.assets)
        return ReportNetWorthModel(tiles: [
            ReportTile(label: netWorth, value: ctx.money(last.netWorth), note: String(localized: "end of \(lastName)")),
            ReportTile(label: String(localized: "Change"), value: ctx.signed(change), note: String(localized: "since \(firstName) began") + percent,
                       tone: change < 0 ? .negative : .positive),
            ReportTile(label: assets, value: ctx.money(last.assets)),
            ReportTile(label: debts, value: ctx.money(last.debt), note: last.assets > 0 ? String(localized: "\(debtShare) of assets") : nil),
        ], chart: columnModel(label: String(localized: "Assets and debts by month with net worth"), keys: keys, series: [
            ReportColumnSeries(key: "assets", label: assets, color: 0, values: data.months.map { Double($0.assets) }),
            ReportColumnSeries(key: "debts", label: debts, color: 1, values: data.months.map { -Double($0.debt) }),
        ], format: format, axis: moneyAxis, unit: ctx.axisUnit, line: ReportLineOverlay(label: netWorth, color: ReportColors.ink, values: data.months.map { Double($0.netWorth) })),
           legend: [ReportLegendItem(label: assets, color: 0), ReportLegendItem(label: debts, color: 1),
                    ReportLegendItem(label: netWorth, color: ReportColors.ink, line: true)],
           change: columnModel(label: String(localized: "How much net worth grew or shrank each month"), keys: keys, series: [
            ReportColumnSeries(key: "up", label: grew, color: ReportColors.divergingPositive, values: monthly.map { Double(Swift.max(0, $0)) }),
            ReportColumnSeries(key: "down", label: shrank, color: ReportColors.divergingNegative, values: monthly.map { Double(Swift.min(0, $0)) }),
           ], format: format, axis: moneyAxis, unit: ctx.axisUnit),
           changeLegend: [ReportLegendItem(label: grew, color: ReportColors.divergingPositive),
                          ReportLegendItem(label: shrank, color: ReportColors.divergingNegative)],
           dates: String(localized: "Balances on \(startDate) and \(endDate)"), accounts: rows)
    }

    private static let inCredit = SheetSection.inCredit
    private static let overdrawn = SheetSection.overdrawn
    private static let assetSections = [SheetSection.cash, SheetSection.investments, SheetSection.property, SheetSection.vehicles,
                                        SheetSection.receivables, SheetSection.otherAssets, inCredit]
    private static let debtSections = [SheetSection.creditCards, SheetSection.linesOfCredit, SheetSection.mortgages, SheetSection.loans,
                                       SheetSection.otherDebts, overdrawn]
    private static let liquid: Set<String> = [SheetSection.cash]

    /// The balance decides the side, as it does for net worth: an overdrawn bank account is owed, a card in credit is owned.
    static func balanceSheetLines(_ data: ReportData, _ ctx: ReportBudget, accounts: Set<String>) -> [ReportSheetLine] {
        guard let last = data.months.last else { return [] }
        let opening = data.openingAccounts ?? ReportAmounts()
        return ctx.accounts.filter { chosen(accounts, $0.id) && last.accounts.contains($0.id) }.map { a in
            let end = last.accounts[a.id] ?? 0
            let start = opening[a.id] ?? 0
            let type = a.type(end)
            let debt = end == 0 ? isLiability(type) : end < 0
            let section = debt == isLiability(type) ? sectionOf(type) : (debt ? overdrawn : inCredit)
            let sign: Int64 = debt ? -1 : 1
            return ReportSheetLine(account: a, section: section, debt: debt, start: start &* sign, end: end &* sign)
        }.filter { $0.start != 0 || $0.end != 0 || !$0.account.archived }
    }

    static func balanceSheetView(_ data: ReportData, _ ctx: ReportBudget, accounts: Set<String>) -> ReportBalanceSheetModel {
        let first = data.months[0]
        let last = data.months[data.months.count - 1]
        let lines = balanceSheetLines(data, ctx, accounts: accounts)
        let counted = lines.filter(\.account.inNetWorth)
        let apart = lines.filter { !$0.account.inNetWorth }
        let assets = counted.filter { !$0.debt }
        let debts = counted.filter(\.debt)
        let totalAssets = assets.map(\.end).reportSum
        let totalDebts = debts.map(\.end).reportSum
        let netStart = assets.map(\.start).reportSum &- debts.map(\.start).reportSum
        let netEnd = totalAssets &- totalDebts
        let liquidTotal = assets.filter { liquid.contains($0.section) }.map(\.end).reportSum
        let monthlySpending = Double(data.spending) / Double(Swift.max(1, data.months.count))
        // The list's own colour fills every bar (series-1 for what you own, series-2 for what you owe).
        func sections(_ order: [String], _ list: [ReportSheetLine], of total: Int64) -> [ReportBarRow] {
            let rows = order.map { s in (key: s, label: s, value: list.filter { $0.section == s }.map(\.end).reportSum) }.filter { $0.value > 0 }
            return barRows(rows, colors: { _ in nil }, total: total, ctx, drill: false)
        }
        func subtitle(_ start: Int64, _ end: Int64, _ of: Int64?) -> String {
            let from = ctx.money(start)
            let change = ctx.money(end &- start)
            return String(localized: "From \(from) · Change \(change)") + (of.map { " · \(share(end, $0))" } ?? "")
        }
        let closed = " · " + String(localized: "closed")
        var statement: [ReportFigureRow] = []
        func part(_ title: String, _ order: [String], _ list: [ReportSheetLine], _ totalLabel: String, _ total: Int64) {
            statement.append(ReportFigureRow(key: "h:\(title)", title: title, style: .section))
            for section in order where list.contains(where: { $0.section == section }) {
                let inSection = list.filter { $0.section == section }
                let start = inSection.map(\.start).reportSum
                let end = inSection.map(\.end).reportSum
                statement.append(ReportFigureRow(key: "s:\(title):\(section)", title: section, value: ctx.money(end), subtitle: subtitle(start, end, total), style: .group))
                for line in inSection {
                    statement.append(ReportFigureRow(key: "a:\(line.account.id)", title: line.account.name + (line.account.archived ? closed : ""),
                                                     value: ctx.money(line.end), subtitle: subtitle(line.start, line.end, total), style: .child))
                }
            }
            if list.isEmpty { statement.append(ReportFigureRow(key: "n:\(title)", title: String(localized: "None"), style: .muted)) }
            let start = list.map(\.start).reportSum
            let end = list.map(\.end).reportSum
            statement.append(ReportFigureRow(key: "t:\(title)", title: totalLabel, value: ctx.money(end), subtitle: subtitle(start, end, nil), style: .total))
        }
        let totalAssetsLabel = String(localized: "Total assets")
        let totalDebtsLabel = String(localized: "Total liabilities")
        let netWorth = String(localized: "Net worth")
        part(String(localized: "Assets"), assetSections, assets, totalAssetsLabel, totalAssets)
        part(String(localized: "Liabilities (owed)"), debtSections, debts, totalDebtsLabel, totalDebts)
        statement.append(ReportFigureRow(key: "__net", title: netWorth, value: ctx.money(netEnd), subtitle: subtitle(netStart, netEnd, nil),
                                         tone: netEnd < 0 ? .negative : .none, style: .net))
        if !apart.isEmpty {
            statement.append(ReportFigureRow(key: "h:apart", title: String(localized: "Not counted in net worth"), style: .section))
            for line in apart {
                statement.append(ReportFigureRow(key: "x:\(line.account.id)", title: "\(line.account.name) · \(line.section.lowercased())",
                                                 value: ctx.money(line.end), subtitle: subtitle(line.start, line.end, nil), style: .child))
            }
        }
        let mixed = lines.contains { $0.section == overdrawn || $0.section == inCredit }
        let periodChange = ctx.signed(netEnd &- netStart)
        let months = monthlySpending > 0 ? fixed(Double(liquidTotal) / monthlySpending, 1) : ""
        let endDate = Money.date(lastDay(last.month))
        let netText = ctx.money(netEnd)
        let startDate = Money.date(firstDay(first.month))
        let firstName = monthName(first.month)
        let lastName = monthName(last.month)
        let help = String(localized: "Balances at the start of \(firstName) and the end of \(lastName). Set an account's type, or leave it out of net worth, in its settings.")
        return ReportBalanceSheetModel(tiles: [
            ReportTile(label: totalAssetsLabel, value: ctx.money(totalAssets)),
            ReportTile(label: totalDebtsLabel, value: ctx.money(totalDebts)),
            ReportTile(label: netWorth, value: netText, note: String(localized: "\(periodChange) in this period"), tone: netEnd < 0 ? .negative : .none),
            ReportTile(label: String(localized: "Debt to assets"), value: totalAssets > 0 ? share(totalDebts, totalAssets) : "—",
                       note: totalDebts > totalAssets ? String(localized: "you owe more than you own") : String(localized: "of what you own is owed")),
            ReportTile(label: String(localized: "Cash on hand"), value: ctx.money(liquidTotal),
                       note: monthlySpending > 0 ? String(localized: "\(months) months of spending") : String(localized: "in cash and bank accounts")),
        ], ownTotal: ctx.money(totalAssets), own: totalAssets > 0 ? sections(assetSections, assets, of: totalAssets) : [],
           oweTotal: ctx.money(totalDebts), owe: totalDebts > 0 ? sections(debtSections, debts, of: totalDebts) : [],
           statementAside: String(localized: "Net worth on \(endDate): \(netText)"),
           dates: String(localized: "Balances on \(startDate) and \(endDate)"), statement: statement,
           help: help + (mixed ? " " + String(localized: "Accounts below zero count as owed and loans or cards above zero as owned, whatever their type.") : ""))
    }

    /// Accounts that can be charted: chosen, with balances in the range, closed ones only if they held money.
    static func balanceAccounts(_ data: ReportData, _ ctx: ReportBudget, accounts: Set<String>) -> [ReportAccount] {
        guard let last = data.months.last else { return [] }
        return ctx.accounts.filter { a in
            chosen(accounts, a.id) && last.accounts.contains(a.id) && (!a.archived || data.months.contains { ($0.accounts[a.id] ?? 0) != 0 })
        }
    }

    /// The five largest balances, each with its own colour slot.
    static func defaultChartAccounts(_ data: ReportData, eligible: [ReportAccount]) -> [String: Int] {
        guard let last = data.months.last else { return [:] }
        let biggest = Set(eligible.reportSorted(descending: { (last.accounts[$0.id] ?? 0).magnitude }).prefix(5).map(\.id))
        var out: [String: Int] = [:]
        for (slot, account) in eligible.filter({ biggest.contains($0.id) }).enumerated() { out[account.id] = slot }
        return out
    }

    /// Adds an account to the chart in the first free colour slot (of eight), or takes it off.
    static func toggleChartAccount(_ chosen: [String: Int], eligible: [ReportAccount], id: String) -> [String: Int] {
        var next = chosen
        if chosen[id] != nil {
            next[id] = nil
            return next
        }
        let used = Set(eligible.compactMap { chosen[$0.id] })
        guard let slot = (0...7).first(where: { !used.contains($0) }) else { return chosen }
        next[id] = slot
        return next
    }

    static func balancesView(_ data: ReportData, _ ctx: ReportBudget, accounts: Set<String>, picked: [String: Int]?) -> ReportBalancesModel {
        let keys = data.months.map(\.month)
        let last = data.months[data.months.count - 1]
        let eligible = balanceAccounts(data, ctx, accounts: accounts)
        let chosen = picked ?? defaultChartAccounts(data, eligible: eligible)
        let shown = eligible.filter { chosen[$0.id] != nil }
        func values(_ id: String) -> [Int64] { data.months.map { $0.accounts[id] ?? 0 } }
        struct Stat { var account: ReportAccount; var start: Int64; var end: Int64; var low: Int64; var high: Int64 }
        let stats = eligible.map { a -> Stat in
            let v = values(a.id)
            return Stat(account: a, start: data.openingAccounts?[a.id] ?? v.first ?? 0, end: v.last ?? 0, low: v.min() ?? 0, high: v.max() ?? 0)
        }
        let mover = stats.reportSorted(descending: { ($0.end &- $0.start).magnitude }).first
        let chart = shown.isEmpty ? nil : linesModel(
            label: String(localized: "Balance of each chosen account at the end of each month"), keys: keys,
            series: shown.map { a in ReportLineSeries(key: a.id, label: a.name, color: chosen[a.id] ?? 0, values: values(a.id).map { Double($0) }) },
            format: { ctx.money(rounding: $0) }, axis: moneyAxis, unit: ctx.axisUnit)
        let firstName = monthName(keys[0])
        var moverNote: String?
        if let mover {
            let moved = ctx.signed(mover.end &- mover.start)
            moverNote = String(localized: "\(moved) since \(firstName) began")
        }
        let closed = " · " + String(localized: "closed")
        let rows = stats.map { s -> ReportFigureRow in
            let start = ctx.money(s.start)
            let change = ctx.money(s.end &- s.start)
            let low = ctx.money(s.low)
            let high = ctx.money(s.high)
            return ReportFigureRow(key: s.account.id, title: s.account.name + (s.account.archived ? closed : ""), value: ctx.money(s.end),
                                   subtitle: String(localized: "Start \(start) · Change \(change) · Lowest \(low) · Highest \(high)"),
                                   tone: s.end &- s.start < 0 ? .negative : .none)
        }
        return ReportBalancesModel(tiles: [
            ReportTile(label: String(localized: "On the chart"), value: String(localized: "\(shown.count) of \(eligible.count)", locale: .app),
                       note: String(localized: "up to 8 at a time")),
            ReportTile(label: String(localized: "Their balance now"), value: ctx.money(shown.map { last.accounts[$0.id] ?? 0 }.reportSum)),
            ReportTile(label: String(localized: "Biggest change"), value: mover?.account.name ?? "—", note: moverNote),
        ], chips: eligible.map { a in ReportAccountChip(id: a.id, name: a.name, slot: chosen[a.id], enabled: chosen[a.id] != nil || shown.count < 8) },
           chosen: chosen, chart: chart, legend: shown.map { ReportLegendItem(label: $0.name, color: chosen[$0.id] ?? 0, line: true) },
           rows: rows)
    }

    static func ageView(_ data: ReportData) -> ReportAgeModel {
        let keys = data.months.map(\.month)
        let values: [Int64?] = keys.map { data.ageOfMoneyByMonth[$0] ?? nil }
        let known = values.compactMap { $0 }
        let changed = known.count > 1
        let delta = changed ? (known[known.count - 1] &- known[0]) : 0
        // "+3 days" or "-3 days", the sign kept with the number.
        let lost = -delta
        let deltaText = delta >= 0 ? String(localized: "+\(delta) days", locale: .app) : String(localized: "-\(lost) days", locale: .app)
        let cashAge = String(localized: "Cash age")
        return ReportAgeModel(tiles: [
            ReportTile(label: cashAge, value: daysText(data.ageOfMoney)),
            ReportTile(label: String(localized: "Change"), value: changed ? deltaText : "—", note: changed ? String(localized: "over these months") : nil,
                       tone: changed ? (delta >= 0 ? .positive : .negative) : .none),
            ReportTile(label: String(localized: "Highest"), value: known.max().map { daysText($0) } ?? "—"),
            ReportTile(label: String(localized: "Lowest"), value: known.min().map { daysText($0) } ?? "—"),
        ], chart: linesModel(label: String(localized: "Cash age at the end of each month, in days"), keys: keys,
                             series: [ReportLineSeries(key: "line", label: cashAge, color: 0, values: values.map { $0.map { Double($0) } })],
                             format: { value in
                                 let number = jsNumber(value)
                                 return String(localized: "\(number) days", comment: "Days on a chart's readout; the number may have decimals")
                             },
                             axis: { value in
                                 let number = jsNumber(value)
                                 return String(localized: "\(number)d", comment: "Days on a chart's axis, as 30d")
                             }, unit: nil),
           rows: keys.enumerated().map { i, m in ReportFigureRow(key: m, title: monthName(m, short: true), value: values[i].map { "\($0)" } ?? "—") })
    }

    /// Builds a report's body. Forecast has its own data and builder.
    static func reportView(_ id: String, _ data: ReportData, _ ctx: ReportBudget, _ scope: ReportScope, _ options: ReportOptions) -> ReportBody? {
        guard !data.months.isEmpty else { return nil }
        switch id {
        case "categories": return .categories(categoryView(data, ctx, group: options.group, today: scope.today))
        case "payees": return .payees(payeeView(data, ctx))
        case "trends": return .trends(overTimeView(data, ctx, byGroup: options.byGroup))
        case "calendar": return .calendar(calendarView(data, ctx, scope))
        case "incomeExpense": return .incomeExpense(incomeExpenseView(data, ctx, someCategories: scope.someCategories))
        case "income": return .income(incomeView(data, ctx))
        case "cashflow": return .cashflow(cashFlowView(data, ctx))
        case "networth": return .networth(netWorthView(data, ctx, accounts: scope.accounts))
        case "balance": return .balance(balanceSheetView(data, ctx, accounts: scope.accounts))
        case "balances": return .balances(balancesView(data, ctx, accounts: scope.accounts, picked: options.chartAccounts))
        case "age": return .age(ageView(data))
        default: return nil
        }
    }

    // MARK: Overview

    /// The income card's sentence, from the months with any income or spending.
    static func incomeLead(_ months: [ReportMonth]) -> String {
        let active = months.filter { $0.income != 0 || $0.spending != 0 }
        if active.isEmpty { return String(localized: "Nothing earned or spent in these months.") }
        let income = Double(active.map(\.income).reportSum) / Double(active.count)
        let spending = Double(active.map(\.spending).reportSum) / Double(active.count)
        if abs(income - spending) < Swift.max(income, spending) * 0.02 { return String(localized: "On average, you're spending about what you make.") }
        return spending > income ? String(localized: "On average, you're spending more than you make.")
            : String(localized: "On average, you're spending less than you make.")
    }

    /// The Reports tab's cards: this month's spending, income against spending, net worth and cash age.
    static func overviewModel(_ data: ReportData, _ ctx: ReportBudget) -> ReportOverviewModel? {
        guard let last = data.months.last else { return nil }
        let top = 5
        let ranked = last.categories.entries.filter { $0.value > 0 }.reportSorted(descending: { $0.value })
        let refunds = last.categories.entries.filter { $0.value < 0 }.reportSorted(ascending: { $0.value })
        let refundTotal = refunds.map(\.value).reportSum
        var refundRows = refunds.prefix(top).map {
            ReportSpendRow(key: $0.key, label: ctx.nameOf[$0.key] ?? $0.key, valueText: ctx.money($0.value), color: ReportColors.other, weight: -$0.value)
        }
        let otherRefunds = refunds.dropFirst(top).map(\.value).reportSum
        if otherRefunds != 0 {
            refundRows.append(ReportSpendRow(key: "__refunds", label: String(localized: "Other refund categories"), valueText: ctx.money(otherRefunds),
                                             color: ReportColors.other, weight: -otherRefunds))
        }
        let unassigned = String(localized: "Unassigned")
        var rows = ranked.prefix(top).enumerated().map { i, entry in
            ReportSpendRow(key: entry.key, label: ctx.nameOf[entry.key] ?? unassigned, valueText: ctx.money(entry.value), color: i, weight: entry.value)
        }
        let rest = ranked.dropFirst(top).map(\.value).reportSum
        if rest > 0 {
            rows.append(ReportSpendRow(key: "__rest", label: String(localized: "All others"), valueText: ctx.money(rest), color: ReportColors.other, weight: rest))
        }
        let labels = data.months.map { monthAbbrev($0.month) }
        func mini(_ label: String, _ color: Int, _ values: [Int64], _ text: (Int64) -> String) -> ReportMiniSeries {
            ReportMiniSeries(label: label, color: color, values: values, texts: values.map(text))
        }
        let ages = data.months.map { (data.ageOfMoneyByMonth[$0.month] ?? nil) ?? 0 }
        let refundText = ctx.money(-refundTotal)
        let refundLead = String(localized: "Includes \(refundText) in net refunds.")
            + (ranked.isEmpty ? "" : " " + String(localized: "The bar shows categories with net spending."))
        return ReportOverviewModel(
            month: last.month, monthName: monthName(last.month), spendingText: ctx.money(last.spending), rows: rows,
            nothingSpent: ranked.isEmpty && refunds.isEmpty,
            refundLead: refunds.isEmpty ? nil : refundLead,
            refunds: refundRows, incomeLead: incomeLead(data.months),
            incomeBars: ReportMiniBarsModel(labels: labels, series: [
                mini(String(localized: "Income"), ReportColors.positive, data.months.map(\.income), ctx.money),
                mini(String(localized: "Spending"), 0, data.months.map(\.spending), ctx.money),
            ]),
            netWorthText: ctx.money(last.netWorth), netWorthNegative: last.netWorth < 0, assetsText: ctx.money(last.assets),
            debtsText: ctx.money(-last.debt), debtsNegative: last.debt != 0,
            netWorthBars: ReportMiniBarsModel(labels: labels, series: [mini(String(localized: "Net worth"), 0, data.months.map(\.netWorth), ctx.money)]),
            ageText: daysText(data.ageOfMoney),
            ageBars: ReportMiniBarsModel(labels: labels, series: [mini(String(localized: "Cash age"), ReportColors.positive, ages, { daysText($0) })]))
    }

    // MARK: Forecast

    static func parseForecast(_ json: JSON) -> ReportForecastData {
        func optional(_ value: JSON, _ key: String) -> String? {
            let text = value.text(key)
            return text.isEmpty ? nil : text
        }
        return ReportForecastData(
            balances: ReportAmounts(json: json["balances"]),
            events: json.rows("events").map { e in
                ReportForecastEvent(date: e.text("date"), amount: e.long("amount"), payee: e.text("payee"), accountId: e.text("accountId"),
                                    peerId: optional(e, "peerId"), categoryId: optional(e, "categoryId"), scheduleId: optional(e, "scheduleId"))
            },
            recentSpending: json.long("recentSpending"))
    }

    static func forecastPlan(_ data: ReportForecastData, accounts: Set<String>, months: Int, start: String, through: String) -> ReportForecastPlan {
        let ids = data.balances.keys.filter { chosen(accounts, $0) }
        let idSet = Set(ids)
        // Moves between two of the chosen accounts don't change their total.
        let events = data.events.filter { idSet.contains($0.accountId) && !($0.peerId.map(idSet.contains) ?? false) }
        let opening = ids.map { data.balances[$0] ?? 0 }.reportSum
        let scheduled = Double(events.filter { $0.scheduleId != nil && $0.categoryId != nil && $0.amount < 0 }.map { -$0.amount }.reportSum) / Double(Swift.max(1, months))
        let suggested = Swift.max(0, jround(Double(data.recentSpending) - scheduled))
        let days = days(from: start, through: through, limit: 2000)
        var byDay: [String: Int64] = [:]
        for event in events { byDay[event.date] = (byDay[event.date] ?? 0) &+ event.amount }
        var known = opening
        let knownLine = days.map { day -> Int64 in
            known = known &+ (byDay[day] ?? 0)
            return known
        }
        var running = opening
        let balances = events.map { event -> Int64 in
            running = running &+ event.amount
            return running
        }
        return ReportForecastPlan(ids: ids, events: events, opening: opening, scheduledSpending: scheduled, suggested: suggested,
                                  days: days, knownLine: knownLine, balances: balances)
    }

    /// The known balance less an even amount of everyday spending each day.
    static func forecastLine(_ knownLine: [Int64], monthly: Int64) -> [Int64] {
        let perDay = Double(monthly) * 12 / 365
        return knownLine.enumerated().map { i, value in jround(Double(value) - perDay * Double(i)) }
    }

    static func forecastView(_ data: ReportForecastData, _ ctx: ReportBudget, accounts: Set<String>, months: Int, everyday: Int64?, start: String) -> ReportForecastModel {
        let plan = forecastPlan(data, accounts: accounts, months: months, start: start, through: addMonths(day: start, months))
        let monthly = everyday ?? plan.suggested
        let line = forecastLine(plan.knownLine, monthly: monthly)
        var low = 0
        for (index, value) in line.enumerated() where value < line[low] { low = index }
        let lowest = line.indices.contains(low) ? line[low] : plan.opening
        let end = line.last ?? plan.opening
        let knownIn = plan.events.filter { $0.amount > 0 }.map(\.amount).reportSum
        let knownOut = plan.events.filter { $0.amount < 0 }.map { -$0.amount }.reportSum
        var categoryName: [String: String] = [:]
        for category in ctx.categories { categoryName[category.id] = category.name }
        let forecast = String(localized: "Forecast")
        let knownOnly = String(localized: "Known transactions only")
        let today = String(localized: "Today")
        let chart = linesModel(label: String(localized: "Forecast balance of budget accounts for the next \(months) months", locale: .app), keys: plan.days, series: [
            ReportLineSeries(key: "forecast", label: forecast, color: 0, values: line.map { Double($0) }),
            ReportLineSeries(key: "known", label: knownOnly, color: 1, values: plan.knownLine.map { Double($0) }, dashed: true),
        ], format: { ctx.money(rounding: $0) }, axis: { compactMoney($0) }, unit: ctx.axisUnit,
           xTick: { day, index in index == 0 ? plainTick(today) : day.hasSuffix("-01") ? plainTick(monthAbbrev(day)) : nil },
           title: { Money.date($0) })
        let noPayee = String(localized: "No payee")
        let entered = " · " + String(localized: "entered")
        let rows = plan.events.enumerated().map { i, e -> ReportForecastRow in
            let peer = ctx.accountName(e.peerId)
            let other = e.peerId != nil ? String(localized: "Transfer: \(peer)") : (e.categoryId.flatMap { categoryName[$0] } ?? "")
            return ReportForecastRow(key: "\(i):\(e.date):\(e.scheduleId ?? "null"):\(e.accountId)",
                                     title: (e.payee.isEmpty ? noPayee : e.payee) + (e.scheduleId == nil ? entered : ""),
                                     detail: [Money.date(e.date), ctx.accountName(e.accountId), other].filter { !$0.isEmpty }.joined(separator: " · "),
                                     amount: ctx.money(e.amount), amountPositive: e.amount >= 0, balance: ctx.money(plan.balances[i]),
                                     balanceNegative: plan.balances[i] < 0)
        }
        let fromToday = ctx.signed(end &- plan.opening)
        let suggested = ctx.money(plan.suggested)
        let recent = ctx.money(data.recentSpending)
        let scheduled = ctx.money(rounding: plan.scheduledSpending)
        let scheduledOrEntered = String(localized: "scheduled or entered")
        return ReportForecastModel(tiles: [
            ReportTile(label: today, value: ctx.money(plan.opening), note: String(localized: "in \(plan.ids.count) budget accounts", locale: .app)),
            ReportTile(label: String(localized: "Lowest point"), value: ctx.money(lowest), note: low == 0 ? String(localized: "today") : Money.date(plan.days[low]),
                       tone: lowest < 0 ? .negative : .none),
            ReportTile(label: String(localized: "In \(months) months", locale: .app), value: ctx.money(end), note: String(localized: "\(fromToday) from today"),
                       tone: end < 0 ? .negative : .none),
            ReportTile(label: String(localized: "Known money in"), value: ctx.money(knownIn), note: scheduledOrEntered),
            ReportTile(label: String(localized: "Known money out"), value: ctx.money(knownOut), note: scheduledOrEntered),
        ], chart: chart, legend: [ReportLegendItem(label: forecast, color: 0, line: true),
                                  ReportLegendItem(label: knownOnly, color: 1, line: true, dashed: true)],
           everyday: monthly, everydayText: ctx.money(monthly),
           suggestion: everyday != nil && everyday != plan.suggested ? String(localized: "Use the suggested \(suggested)") : nil,
           help: String(localized: "Spending that isn't scheduled, spread evenly over each day. Your last three months averaged \(recent) a month; scheduled spending covers about \(scheduled) a month, which suggests \(suggested) for the rest."),
           count: String(localized: "\(plan.events.count) transactions", locale: .app), rows: rows)
    }

    // MARK: The transactions behind a figure

    /// The order transactions were added in (date, then time added, then id), newest first as the register lists them.
    private static func newestFirst(_ entries: [ReportDrillEntry]) -> [ReportDrillEntry] {
        func before(_ a: ReportDrillEntry, _ b: ReportDrillEntry) -> Bool {
            if a.date != b.date { return a.date < b.date }
            let ac = a.transaction.text("createdAt")
            let bc = b.transaction.text("createdAt")
            if !ac.isEmpty || !bc.isEmpty { return ac < bc }
            return a.transaction.text("id") < b.transaction.text("id")
        }
        let sorted = entries.enumerated().sorted { x, y in
            before(x.element, y.element) || (!before(y.element, x.element) && x.offset < y.offset)
        }
        return sorted.reversed().map(\.element)
    }

    private static func drillRange(_ from: String, _ to: String, _ accounts: Set<String>) -> String {
        (from == to ? Money.date(from) : "\(Money.date(from)) – \(Money.date(to))") + (accounts.isEmpty ? "" : " · " + String(localized: "chosen accounts"))
    }

    private static func drillModel(_ entries: [ReportDrillEntry], _ ctx: ReportBudget, income: Bool, from: String, to: String,
                                   accounts: Set<String>, total: Int64) -> ReportDrillModel {
        let ordered = newestFirst(entries)
        let count = Set(entries.map { $0.transaction.text("id") }).count
        var records: [String: JSON] = [:]
        for entry in entries { records[entry.transaction.text("id")] = entry.transaction }
        return ReportDrillModel(
            range: drillRange(from, to, accounts), figureLabel: income ? String(localized: "Received") : String(localized: "Spent"),
            figure: ctx.money(total), count: "\(count)",
            average: count > 0 ? ctx.money(rounding: Double(total) / Double(count)) : "—",
            lines: ordered.map { e in
                ReportDrillLine(key: "\(e.transaction.text("id")):\(e.index)", transactionId: e.transaction.text("id"), payee: e.payee,
                                detail: [Money.date(e.date), e.account, e.category].filter { !$0.isEmpty }.joined(separator: " · "),
                                amount: ctx.money(e.amount), positive: e.amount > 0)
            },
            records: records)
    }

    /// Lines from the engine's report details: spending is positive there, so lines show it as the register does (negative).
    static func drillFromDetails(_ items: [JSON], _ budget: JSON, _ ctx: ReportBudget, from: String, to: String, accounts: Set<String>) -> ReportDrillModel {
        let wanted = Set(items.map { $0.text("transactionId") })
        var records: [String: JSON] = [:]
        for transaction in budget.rows("transactions") where wanted.contains(transaction.text("id")) { records[transaction.text("id")] = transaction }
        let entries = items.enumerated().compactMap { i, part -> ReportDrillEntry? in
            guard let t = records[part.text("transactionId")] else { return nil }
            let account = part.text("accountId")
            let transfer = t.text("transferAccountId")
            let peer: String? = transfer.isEmpty ? nil : (account == t.text("accountId") ? transfer : t.text("accountId"))
            let categoryId = part.text("categoryId")
            let payee = part.text("payee")
            return ReportDrillEntry(transaction: t, index: i, date: part.text("date"),
                                    payee: peer.map { transferText(ctx.accountName($0)) } ?? (payee.isEmpty ? String(localized: "No payee") : payee),
                                    account: ctx.accountName(account),
                                    category: categoryId == uncategorized ? needsCategory : (ctx.nameOf[categoryId] ?? String(localized: "Unassigned")),
                                    amount: -part.long("amount"))
        }
        return drillModel(entries, ctx, income: false, from: from, to: to, accounts: accounts, total: items.map { $0.long("amount") }.reportSum)
    }

    /// A transfer as a drill line's payee: "Transfer: Savings".
    private static func transferText(_ account: String) -> String { String(localized: "Transfer: \(account)") }

    private static func leftUnassigned(_ t: JSON, _ s: JSON, splits: Int) -> Bool {
        if case .bool(let value) = s["unassigned"] { return value }
        let source = s["source"].isObject ? s["source"] : (splits == 1 && t["source"].isObject ? t["source"] : .null)
        let name = source.text("category_name")
        return name == "Inflow: Ready to Assign" || name == "Inflow: To be Budgeted" || t.text("payee") == "Reconciliation adjustment"
    }

    private static func startingBalance(_ t: JSON, _ splits: [JSON]) -> Bool {
        t.flag("startingBalance") && t.text("payee") == "Starting Balance" && t.text("transferAccountId").isEmpty && !splits.isEmpty &&
            splits.allSatisfy { $0.text("categoryId").isEmpty && $0["unassigned"] != .bool(false) && $0.text("transferAccountId").isEmpty }
    }

    /// Income from one source: money to Unassigned in budget accounts, transfers across the budget
    /// boundary signed from the budget side (the desktop's reportEntries with income set).
    static func incomeDrill(_ budget: JSON, _ ctx: ReportBudget, payee: String, fromMonth: String, toMonth: String, accounts: Set<String>) -> ReportDrillModel {
        let from = firstDay(fromMonth)
        let to = lastDay(toMonth)
        func onBudget(_ id: String) -> Bool { ctx.accountsById[id]?.onBudget == true }
        let noPayee = String(localized: "No payee")
        let unassigned = String(localized: "Unassigned")
        var entries: [ReportDrillEntry] = []
        for t in budget.rows("transactions") {
            let date = t.text("date")
            if date < from || date > to { continue }
            let splits = t.rows("splits")
            let opening = startingBalance(t, splits)
            for (index, s) in splits.enumerated() {
                let splitTransfer = s.text("transferAccountId")
                let other = splitTransfer.isEmpty ? t.text("transferAccountId") : splitTransfer
                var amount = s.long("amount")
                var account = t.text("accountId")
                let splitPayee = s.text("payee")
                let effective = splitPayee.isEmpty ? t.text("payee") : splitPayee
                var shown = effective.isEmpty ? noPayee : effective
                if !other.isEmpty {
                    let fromBudget = onBudget(t.text("accountId"))
                    if fromBudget == onBudget(other) { continue }
                    if !fromBudget {
                        amount = -amount
                        account = other
                    }
                    shown = transferText(ctx.accountName(fromBudget ? other : t.text("accountId")))
                } else if !onBudget(t.text("accountId")) {
                    continue
                }
                if !accounts.isEmpty && !accounts.contains(account) { continue }
                if effective != payee { continue }
                let income = !opening && s.text("categoryId").isEmpty && (amount >= 0 || leftUnassigned(t, s, splits: splits.count))
                if !income { continue }
                entries.append(ReportDrillEntry(transaction: t, index: index, date: date, payee: shown, account: ctx.accountName(account),
                                                category: unassigned, amount: amount))
            }
        }
        return drillModel(entries, ctx, income: true, from: from, to: to, accounts: accounts, total: entries.map(\.amount).reportSum)
    }
}
