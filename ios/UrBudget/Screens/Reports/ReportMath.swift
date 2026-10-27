import Foundation

/*
 * The reports' figures, ported from Android's ReportMath.kt (itself from the desktop's React
 * reports). The shared engine (crates/urbudget-core/src/reports.rs) does the accounting; this file
 * and ReportModels.swift shape its JSON into value models with every label and amount already
 * formatted, so lists and charts only lay out and draw.
 */

/// Chart colours: 0 to 7 are the report series (series-1..8 in reports.css); the rest are roles.
enum ReportColors {
    static let other = -1
    static let ink = -2
    static let positive = -3
    static let negative = -4
    static let divergingPositive = 0
    static let divergingNegative = 7
}

enum ReportTone: Hashable, Sendable { case none, positive, negative }

/// Amounts by key in a fixed order: key order as the engine's BTreeMap writes them, or the order
/// keys were first added in.
struct ReportAmounts: Equatable, Sendable {
    private(set) var keys: [String] = []
    private var values: [String: Int64] = [:]

    init() {}

    /// Amounts in key order.
    init(_ amounts: [String: Int64]) {
        values = amounts
        keys = amounts.keys.sorted()
    }

    init(json: JSON) {
        var amounts: [String: Int64] = [:]
        for (key, value) in json.object { amounts[key] = value.int }
        self.init(amounts)
    }

    subscript(key: String) -> Int64? { values[key] }
    var isEmpty: Bool { keys.isEmpty }
    var count: Int { keys.count }
    var entries: [(key: String, value: Int64)] { keys.map { ($0, values[$0] ?? 0) } }
    func contains(_ key: String) -> Bool { values[key] != nil }

    /// Adds to a key's amount; a new key goes last.
    mutating func add(_ key: String, _ amount: Int64) {
        if let existing = values[key] {
            values[key] = existing &+ amount
        } else {
            keys.append(key)
            values[key] = amount
        }
    }
}

extension Sequence where Element == Int64 {
    /// The total, wrapping rather than trapping (the engine keeps totals in the safe range).
    var reportSum: Int64 { reduce(0, &+) }
}

extension Sequence {
    /// Highest first; ties keep their order, as Kotlin's sortedByDescending.
    func reportSorted<T: Comparable>(descending key: (Element) -> T) -> [Element] {
        enumerated().map { (offset: $0.offset, element: $0.element, key: key($0.element)) }
            .sorted { $0.key != $1.key ? $0.key > $1.key : $0.offset < $1.offset }
            .map(\.element)
    }

    /// Lowest first; ties keep their order, as Kotlin's sortedBy.
    func reportSorted<T: Comparable>(ascending key: (Element) -> T) -> [Element] {
        enumerated().map { (offset: $0.offset, element: $0.element, key: key($0.element)) }
            .sorted { $0.key != $1.key ? $0.key < $1.key : $0.offset < $1.offset }
            .map(\.element)
    }
}

/// A result that belongs to one request: a figure from an older range or snapshot never reads as current.
struct ReportScoped<Scope: Equatable, Value> {
    var scope: Scope
    var value: Value

    func forScope(_ scope: Scope) -> Value? { self.scope == scope ? value : nil }
}

enum ReportMath {
    /// The report-only bucket for spending without a category, as the engine names it.
    static let uncategorized = "__uncategorized"
    /// The bucket's name in the app's language; reports use it as that bucket's group and name throughout.
    static let needsCategory = String(localized: "Needs a category")

    // MARK: Text

    /// "2026-10" as "October 2026", or "Oct 2026" when short.
    static func monthName(_ month: String, short: Bool = false) -> String {
        short ? Dates.shortMonthLabel(month) : Dates.monthLabel(month)
    }

    /// "2026-10" (or a day in it) as "Oct".
    static func monthAbbrev(_ month: String) -> String { Dates.shortMonthName(String(month.prefix(7))) }

    static func shiftMonth(_ month: String, _ delta: Int) -> String { Dates.addMonths(month, delta) }
    static func firstDay(_ month: String) -> String { "\(month)-01" }

    static func lastDay(_ month: String) -> String {
        let last = Dates.monthBounds(month).1
        return last.isEmpty ? "\(month)-28" : last
    }

    /// A day's parts, or nil when it isn't a real date.
    static func dayParts(_ iso: String) -> (year: Int, month: Int, day: Int)? {
        let parts = iso.split(separator: "-", omittingEmptySubsequences: false)
        guard iso.count == 10, parts.count == 3, let year = Int(parts[0]), let month = Int(parts[1]), let day = Int(parts[2]),
              (1...12).contains(month), day >= 1, day <= daysIn(year, month) else { return nil }
        return (year, month, day)
    }

    static func daysIn(_ year: Int, _ month: Int) -> Int {
        switch month {
        case 2: return (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 ? 29 : 28
        case 4, 6, 9, 11: return 30
        default: return 31
        }
    }

    static func dayText(_ year: Int, _ month: Int, _ day: Int) -> String {
        String(format: "%04d-%02d-%02d", year, month, day)
    }

    private static func nextDay(_ day: (year: Int, month: Int, day: Int)) -> (year: Int, month: Int, day: Int) {
        if day.day < daysIn(day.year, day.month) { return (day.year, day.month, day.day + 1) }
        return day.month == 12 ? (day.year + 1, 1, 1) : (day.year, day.month + 1, 1)
    }

    /// Every day from `start` through `end`, at most `limit` of them.
    static func days(from start: String, through end: String, limit: Int) -> [String] {
        guard var day = dayParts(start), dayParts(end) != nil else { return [] }
        var out: [String] = []
        var text = dayText(day.year, day.month, day.day)
        while text <= end && out.count < limit {
            out.append(text)
            day = nextDay(day)
            text = dayText(day.year, day.month, day.day)
        }
        return out
    }

    /// Months added to a day, the day kept within the month (java.time's plusMonths).
    static func addMonths(day iso: String, _ count: Int) -> String {
        guard let parts = dayParts(iso) else { return iso }
        let total = parts.year * 12 + parts.month - 1 + count
        let year = total >= 0 ? total / 12 : (total - 11) / 12
        let month = total - year * 12 + 1
        return dayText(year, month, min(parts.day, daysIn(year, month)))
    }

    /// JavaScript's weekday numbering (0 is Sunday), which the desktop's calendar uses.
    static func jsWeekday(_ iso: String) -> Int {
        guard let parts = dayParts(iso) else { return 0 }
        // Sakamoto's method.
        let offsets = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4]
        let year = parts.month < 3 ? parts.year - 1 : parts.year
        let value = year + year / 4 - year / 100 + year / 400 + offsets[parts.month - 1] + parts.day
        return ((value % 7) + 7) % 7
    }

    private static let weekdayNames = [
        String(localized: "Sunday"), String(localized: "Monday"), String(localized: "Tuesday"), String(localized: "Wednesday"),
        String(localized: "Thursday"), String(localized: "Friday"), String(localized: "Saturday"),
    ]
    private static let shortWeekdayNames = [
        String(localized: "Sun"), String(localized: "Mon"), String(localized: "Tue"), String(localized: "Wed"),
        String(localized: "Thu"), String(localized: "Fri"), String(localized: "Sat"),
    ]

    static func weekdayName(_ day: Int, short: Bool = false) -> String {
        let names = short ? shortWeekdayNames : weekdayNames
        return names[((day % 7) + 7) % 7]
    }

    /// The first day of the week where the phone is set, in JavaScript's numbering.
    static func localeWeekStart(_ calendar: Calendar = .current) -> Int { (calendar.firstWeekday - 1 + 7) % 7 }

    /// Kotlin's Math.round: the nearest whole number, halves up.
    static func jround(_ value: Double) -> Int64 {
        guard value.isFinite else { return 0 }
        let rounded = (value + 0.5).rounded(.down)
        if rounded >= 9.2e18 { return Int64.max }
        if rounded <= -9.2e18 { return Int64.min }
        return Int64(rounded)
    }

    static func roundDiv(_ total: Int64, _ count: Int) -> Int64 { jround(Double(total) / Double(max(1, count))) }

    /// JavaScript's `toFixed`: half away from zero on the exact value, the sign kept for small negatives.
    static func fixed(_ value: Double, _ digits: Int) -> String {
        guard value.isFinite else { return value.isNaN ? "NaN" : (value < 0 ? "-Infinity" : "Infinity") }
        // Sixty places hold every double used here exactly, far past where a tie could hide.
        let exact = String(format: "%.60f", abs(value))
        let parts = exact.split(separator: ".", omittingEmptySubsequences: false)
        let whole = Array(parts[0])
        let fraction = parts.count > 1 ? Array(parts[1]) : []
        var kept = whole + fraction.prefix(digits)
        while kept.count < whole.count + digits { kept.append("0") }
        if digits < fraction.count, fraction[digits] >= "5" {
            var index = kept.count - 1
            var carry = true
            while carry && index >= 0 {
                if kept[index] == "9" {
                    kept[index] = "0"
                    index -= 1
                } else {
                    kept[index] = Character(String((kept[index].wholeNumberValue ?? 0) + 1))
                    carry = false
                }
            }
            if carry { kept.insert("1", at: 0) }
        }
        let wholeCount = kept.count - digits
        let text = String(kept.prefix(wholeCount)) + (digits > 0 ? "." + String(kept.suffix(digits)) : "")
        return value < 0 ? "-" + text : text
    }

    /// A number as JavaScript prints it in a template string: 12 not 12.0.
    static func jsNumber(_ value: Double) -> String {
        if value.isFinite && value == value.rounded(.down) && abs(value) < 1e15 { return String(Int64(value + 0.0)) }
        return String(value)
    }

    /// "12.5%" of a positive total, else a dash. A negative share keeps its sign in front in Arabic.
    static func share(_ part: Int64, _ total: Int64) -> String {
        total > 0 ? AppLanguage.leftToRight("\(fixed(Double(part) / Double(total) * 100, 1))%") : "—"
    }

    static func daysText(_ days: Int64?) -> String {
        guard let days else { return "—" }
        return String(localized: "\(days) days", locale: .app)
    }

    /// Axis label for an amount in cents: 0, 950, 1.5K, 12K, 1.2M.
    static func compactMoney(_ cents: Double, decimal: String = Money.decimalMark()) -> String {
        let value = cents / 100 + 0.0
        let size = abs(value)
        func part(_ number: Double, _ digits: Int) -> String {
            fixed(number, digits).replacingOccurrences(of: #"\.0+$"#, with: "", options: .regularExpression)
                .replacingOccurrences(of: ".", with: decimal)
        }
        if size >= 1e6 {
            let millions = part(value / 1e6, size >= 1e7 ? 0 : 1)
            return String(localized: "\(millions)M", comment: "An axis amount in millions, as 1.2M")
        }
        if size >= 1e3 {
            let thousands = part(value / 1e3, size >= 1e4 ? 0 : 1)
            return String(localized: "\(thousands)K", comment: "An axis amount in thousands, as 1.5K")
        }
        return AppLanguage.leftToRight(part(value, 0))
    }

    // MARK: Scales

    /// Round tick values (in the data's units) covering [low, high], always including 0.
    static func niceTicks(_ low: Double, _ high: Double, count: Int = 5) -> [Double] {
        let lo = Swift.min(0, low)
        let hi = Swift.max(0, high)
        if lo == hi || lo.isNaN || hi.isNaN { return [0, 1] }
        let raw = (hi - lo) / Double(count)
        let power = pow(10, (log10(raw)).rounded(.down))
        let step = [1.0, 2.0, 2.5, 5.0, 10.0].map { $0 * power }.first { raw <= $0 } ?? 10 * power
        var ticks: [Double] = []
        var value = (lo / step).rounded(.down) * step
        while value <= hi + step / 2 && ticks.count < 64 {
            ticks.append(abs(value) < 1e12 ? Double(jround(value * 1e6)) / 1e6 : value)
            value += step
        }
        if let last = ticks.last, last < hi { ticks.append(last + step) }
        return ticks
    }

    /// Five steps of magnitude: each positive value gets 1 to 5 by its quintile among the others.
    static func heatLevels(_ values: [Int64]) -> ReportHeatScale {
        let sorted = values.filter { $0 > 0 }.sorted()
        guard !sorted.isEmpty else { return ReportHeatScale(cuts: nil) }
        func at(_ q: Double) -> Int64 { sorted[Swift.min(sorted.count - 1, Int((q * Double(sorted.count)).rounded(.down)))] }
        return ReportHeatScale(cuts: [at(0.2), at(0.4), at(0.6), at(0.8)])
    }

    /// Squarified treemap: areas in proportion to values, rows kept as square as possible. Largest first.
    static func squarify<T>(_ items: [T], value: (T) -> Double, width: Double, height: Double) -> [ReportTileBox<T>] {
        let values = items.map(value)
        let total = values.reduce(0, +)
        guard total > 0, width > 0, height > 0 else { return [] }
        let scale = width * height / total
        let areas = values.map { $0 * scale }
        var out: [ReportTileBox<T>] = []
        out.reserveCapacity(items.count)
        var rx = 0.0, ry = 0.0, rw = width, rh = height
        func worst(_ row: [Int], _ side: Double) -> Double {
            let sum = row.reduce(0) { $0 + areas[$1] }
            let most = row.map { areas[$0] }.max() ?? 0
            let least = row.map { areas[$0] }.min() ?? 0
            return Swift.max(side * side * most / (sum * sum), sum * sum / (side * side * least))
        }
        func place(_ row: [Int]) {
            let sum = row.reduce(0) { $0 + areas[$1] }
            if rw >= rh {
                let w = sum / rh
                var y = ry
                for index in row {
                    let h = areas[index] / w
                    out.append(ReportTileBox(item: items[index], x: rx, y: y, w: w, h: h))
                    y += h
                }
                rx += w
                rw -= w
            } else {
                let h = sum / rw
                var x = rx
                for index in row {
                    let w = areas[index] / h
                    out.append(ReportTileBox(item: items[index], x: x, y: ry, w: w, h: h))
                    x += w
                }
                ry += h
                rh -= h
            }
        }
        var row: [Int] = []
        for index in items.indices {
            let side = Swift.min(rw, rh)
            if row.isEmpty || worst(row + [index], side) <= worst(row, side) {
                row.append(index)
            } else {
                place(row)
                row = [index]
            }
        }
        if !row.isEmpty { place(row) }
        return out
    }

    // MARK: The engine's report

    static func parseReport(_ json: JSON) -> ReportData {
        let months = json.rows("months").map { m in
            ReportMonth(month: m.text("month"), income: m.long("income"), spending: m.long("spending"), netIncome: m.long("netIncome"),
                        assets: m.long("assets"), debt: m.long("debt"), netWorth: m.long("netWorth"), inflow: m.long("inflow"),
                        outflow: m.long("outflow"), categories: ReportAmounts(json: m["categories"]),
                        incomeSources: ReportAmounts(json: m["incomeSources"]), accounts: ReportAmounts(json: m["accounts"]))
        }
        let payees = json["payees"].object.filter { $0.value.isObject }.sorted { $0.key < $1.key }
            .map { ReportPayeeSpend(name: $0.key, amount: $0.value.long("amount"), count: $0.value.long("count")) }
            .reportSorted(descending: { $0.amount })
        var ages: [String: Int64?] = [:]
        for (key, value) in json["ageOfMoneyByMonth"].object { ages[key] = value.isNull ? nil : value.int }
        return ReportData(months: months, categories: ReportAmounts(json: json["categories"]), incomeSources: ReportAmounts(json: json["incomeSources"]),
                          payees: payees, days: ReportAmounts(json: json["days"]), income: json.long("income"), spending: json.long("spending"),
                          transactionCount: json.has("transactionCount") ? json.long("transactionCount") : nil,
                          ageOfMoney: json["ageOfMoney"].isNull ? nil : json.long("ageOfMoney"),
                          openingAccounts: json["openingAccounts"].isObject ? ReportAmounts(json: json["openingAccounts"]) : nil,
                          ageOfMoneyByMonth: ages)
    }

    // MARK: The budget, as reports read it

    /// The balance sheet's sections, named in the app's language; a section's name is also its key.
    enum SheetSection {
        static let cash = String(localized: "Cash and bank")
        static let investments = String(localized: "Investments")
        static let property = String(localized: "reports.section.property", defaultValue: "Property", comment: "Balance sheet section: houses and land.")
        static let vehicles = String(localized: "Vehicles")
        static let receivables = String(localized: "Receivables")
        static let otherAssets = String(localized: "Other assets")
        static let creditCards = String(localized: "Credit cards")
        static let linesOfCredit = String(localized: "Lines of credit")
        static let mortgages = String(localized: "Mortgages")
        static let loans = String(localized: "Loans")
        static let otherDebts = String(localized: "Other debts")
        static let inCredit = String(localized: "Accounts in credit")
        static let overdrawn = String(localized: "Overdrawn accounts")
    }

    private static let sections: [String: String] = [
        "checking": SheetSection.cash, "savings": SheetSection.cash, "cash": SheetSection.cash,
        "investment": SheetSection.investments, "retirement": SheetSection.investments, "property": SheetSection.property,
        "vehicle": SheetSection.vehicles, "receivable": SheetSection.receivables, "otherAsset": SheetSection.otherAssets,
        "creditCard": SheetSection.creditCards, "lineOfCredit": SheetSection.linesOfCredit, "mortgage": SheetSection.mortgages,
        "autoLoan": SheetSection.loans, "studentLoan": SheetSection.loans, "personalLoan": SheetSection.loans,
        "medicalDebt": SheetSection.otherDebts, "otherDebt": SheetSection.otherDebts,
    ]
    private static let liabilities: Set<String> = ["creditCard", "lineOfCredit", "mortgage", "autoLoan", "studentLoan", "personalLoan", "medicalDebt", "otherDebt"]

    static func isLiability(_ type: String) -> Bool { liabilities.contains(type) }
    static func sectionOf(_ type: String) -> String { sections[type] ?? (isLiability(type) ? SheetSection.otherDebts : SheetSection.otherAssets) }

    /// An account's type: the one chosen, else the imported one, else a guess from its kind and balance.
    static func accountType(chosen: String, imported: String, kind: String, balance: Int64) -> String {
        if sections[chosen] != nil { return chosen }
        if imported == "otherLiability" { return "otherDebt" }
        if sections[imported] != nil { return imported }
        if kind == "credit" { return "creditCard" }
        if kind == "cash" { return "checking" }
        return balance < 0 ? "otherDebt" : "otherAsset"
    }

    /// Groups in the budget's order, then in the order their first category appears.
    static func groupNames(_ categories: [ReportCategory], order: [String]) -> [String] {
        var rank: [String: Int] = [:]
        for (index, group) in order.enumerated() where rank[group] == nil { rank[group] = index }
        var names: [String] = []
        var seen = Set<String>()
        for category in categories where seen.insert(category.group).inserted { names.append(category.group) }
        return names.enumerated()
            .sorted { (rank[$0.element] ?? Int.max, $0.offset) < (rank[$1.element] ?? Int.max, $1.offset) }
            .map(\.element)
    }

    /// Names, groups, accounts and the first month with data, read once per budget snapshot.
    static func parseBudget(_ budget: JSON, today: String) -> ReportBudget {
        let accounts = budget.rows("accounts").map { a -> ReportAccount in
            let kind = a.text("kind")
            var imported = ""
            if case .string(let type) = a["source"]["type"] { imported = type }
            return ReportAccount(id: a.text("id"), name: a.text("name"), kind: kind, archived: a.flag("archived"),
                                 onBudget: kind != "tracking" && a.flag("onBudget", true), inNetWorth: a.flag("netWorth", true),
                                 chosenType: a.text("accountType"), importedType: imported, openingBalance: a.long("openingBalance"))
        }
        let categories = budget.rows("categories").map {
            ReportCategory(id: $0.text("id"), name: $0.text("name"), group: $0.text("group"),
                           payment: !$0.text("paymentAccountId").trimmingCharacters(in: .whitespaces).isEmpty)
        }
        let order = budget["groupOrder"].array.map(\.string)
        var groupOf: [String: String] = [:]
        var nameOf: [String: String] = [:]
        for category in categories {
            groupOf[category.id] = category.group
            nameOf[category.id] = category.name
        }
        groupOf[uncategorized] = needsCategory
        nameOf[uncategorized] = needsCategory
        var spending = groupNames(categories, order: order).filter { group in categories.contains { $0.group == group && !$0.payment } }
        if !spending.contains(needsCategory) { spending.append(needsCategory) }
        var earliest = today
        for transaction in budget["transactions"].array {
            let date = transaction.text("date")
            if !date.isEmpty && date < earliest { earliest = date }
        }
        return ReportBudget(currency: budget.text("currency"), accounts: accounts, categories: categories, groupOf: groupOf, nameOf: nameOf,
                            spendingGroups: spending, earliestMonth: String(earliest.prefix(7)))
    }

    // MARK: Ranges and filters

    /// The months a preset covers, counted from this month (desktop rangeFor at month granularity).
    static func rangeFor(_ preset: ReportRangePreset, today: String, earliest: String) -> ReportRange {
        let year = Dates.year(today)
        let from: String?
        let to: String
        switch preset {
        case .thisMonth: (from, to) = (today, today)
        case .lastMonth: (from, to) = (shiftMonth(today, -1), shiftMonth(today, -1))
        case .last3: (from, to) = (shiftMonth(today, -2), today)
        case .last6: (from, to) = (shiftMonth(today, -5), today)
        case .last12: (from, to) = (shiftMonth(today, -11), today)
        case .ytd: (from, to) = (Dates.month(year: year, number: 1), today)
        case .lastYear: (from, to) = (Dates.month(year: year - 1, number: 1), Dates.month(year: year - 1, number: 12))
        case .all, .custom: (from, to) = (nil, today)
        }
        return ReportRange(preset: preset, from: from ?? String(earliest.prefix(7)), to: to)
    }

    /// A custom range, its ends swapped when they come the wrong way round.
    static func customRange(from: String, to: String) -> ReportRange {
        if !from.isEmpty && !to.isEmpty && from > to { return ReportRange(preset: .custom, from: to, to: from) }
        return ReportRange(preset: .custom, from: from, to: to)
    }

    /// The last twelve months when the budget shows this month, else the twelve ending at the budget's month.
    static func defaultRange(month: String, today: String) -> ReportRange {
        month == today ? rangeFor(.last12, today: today, earliest: "") : ReportRange(preset: .custom, from: shiftMonth(month, -11), to: month)
    }

    static func describeRange(_ range: ReportRange) -> String {
        if range.preset != .custom { return range.preset.label }
        func show(_ month: String) -> String { month.isEmpty ? "…" : monthName(month, short: true) }
        return "\(show(range.from)) – \(show(range.to))"
    }

    /// An empty selection means all; a selection of only unknown ids (the empty id) means none.
    static func filterIsOn(_ selected: Set<String>, _ id: String) -> Bool { selected.isEmpty || selected.contains(id) }

    /// What a filter has chosen: `allLabel`, one item's name, `several(count)` ("3 accounts"), or None.
    static func filterSummary(_ groups: [ReportFilterGroup], _ selected: Set<String>, allLabel: String, several: (Int) -> String) -> String {
        if selected.isEmpty { return allLabel }
        let chosen = groups.flatMap(\.items).filter { selected.contains($0.id) }
        switch chosen.count {
        case 0: return String(localized: "None")
        case 1: return chosen[0].label
        default: return several(chosen.count)
        }
    }

    static func filterToggle(_ groups: [ReportFilterGroup], _ selected: Set<String>, _ ids: [String], on: Bool) -> Set<String> {
        let all = groups.flatMap(\.items).map(\.id)
        var next = Set(selected.isEmpty ? all : all.filter { selected.contains($0) })
        for id in ids {
            if on { next.insert(id) } else { next.remove(id) }
        }
        if next.count == Set(all).count { return [] }
        if next.isEmpty { return [""] }
        return next
    }

    /// The accounts filter: budget, tracking and closed accounts (only budget accounts for the forecast).
    static func accountFilterGroups(_ ctx: ReportBudget, budgetOnly: Bool) -> [ReportFilterGroup] {
        let groups: [(name: String, budget: Bool, accounts: [ReportAccount])] = [
            (String(localized: "Budget"), true, ctx.accounts.filter { !$0.archived && $0.kind != "tracking" }),
            (String(localized: "Tracking"), false, ctx.accounts.filter { !$0.archived && $0.kind == "tracking" }),
            (String(localized: "Closed"), false, ctx.accounts.filter(\.archived)),
        ]
        return groups.filter { !$0.accounts.isEmpty && (!budgetOnly || $0.budget) }
            .map { ReportFilterGroup(name: $0.name, items: $0.accounts.map { ReportFilterItem(id: $0.id, label: $0.name) }) }
    }

    /// The categories filter: categories that can hold spending, by group, "Needs a category" last.
    static func categoryFilterGroups(_ ctx: ReportBudget) -> [ReportFilterGroup] {
        ctx.spendingGroups.map { group in
            let items = ctx.categories.filter { $0.group == group && !$0.payment }.map { ReportFilterItem(id: $0.id, label: $0.name) }
            return ReportFilterGroup(name: group, items: group == needsCategory ? items + [ReportFilterItem(id: uncategorized, label: needsCategory)] : items)
        }
    }

    // MARK: Chart models

    static func monthXTick(_ month: String) -> ReportXTick? {
        guard Dates.isMonth(month) else { return nil }
        let name = Dates.shortMonthName(month)
        return ReportXTick(short: name, long: "\(name) \(month.dropFirst(2).prefix(2))", year: Dates.year(month))
    }

    static func monthTitle(_ month: String) -> String { monthName(month) }
    static func plainTick(_ text: String) -> ReportXTick { ReportXTick(short: text, long: text, year: 0) }

    static func columnModel(
        label: String, keys: [String], series: [ReportColumnSeries], format: (Double) -> String, axis: (Double) -> String, unit: String?,
        grouped: Bool = false, line: ReportLineOverlay? = nil, average: (label: String, value: Double)? = nil,
        xTick: (String, Int) -> ReportXTick? = { key, _ in ReportMath.monthXTick(key) }, title: (String) -> String = { ReportMath.monthTitle($0) }
    ) -> ReportColumnModel {
        func at(_ s: ReportColumnSeries, _ i: Int) -> Double { i < s.values.count ? s.values[i] : 0 }
        let ups = keys.indices.map { i in grouped ? Swift.max(0, series.map { at($0, i) }.max() ?? 0) : series.reduce(0) { $0 + Swift.max(0, at($1, i)) } }
        let downs = keys.indices.map { i in grouped ? Swift.min(0, series.map { at($0, i) }.min() ?? 0) : series.reduce(0) { $0 + Swift.min(0, at($1, i)) } }
        let lineValues = line?.values.compactMap { $0 } ?? []
        let ticks = niceTicks(Swift.min(0, downs.min() ?? 0, lineValues.min() ?? 0),
                              Swift.max(0, ups.max() ?? 0, lineValues.max() ?? 0, average?.value ?? 0))
        let readouts = keys.indices.map { i -> [ReportReadout] in
            var rows = series.filter { at($0, i) != 0 }.map { ReportReadout(label: $0.label, value: format(at($0, i)), color: $0.color) }
            let total = series.reduce(0) { $0 + at($1, i) }
            let overlay: Double? = line.flatMap { i < $0.values.count ? $0.values[i] : nil }
            // A line that is the stack's total (net worth) is its total already.
            if series.count > 1 && !grouped && (overlay == nil || abs((overlay ?? 0) - total) >= 0.005) {
                rows.append(ReportReadout(label: String(localized: "Total"), value: format(total), color: nil))
            }
            if let overlay, let line { rows.append(ReportReadout(label: line.label, value: format(overlay), color: line.color, line: true)) }
            return rows
        }
        let texts = series.map { s in keys.indices.map { format(at(s, $0)) } }
        let lineTexts = line.map { line in keys.indices.map { i -> String? in (i < line.values.count ? line.values[i] : nil).map(format) } }
        return ReportColumnModel(label: label, keys: keys, series: series, grouped: grouped, line: line, average: average?.value,
                                 averageText: average.map { "\($0.label) \(format($0.value))" }, ticks: ticks, tickLabels: ticks.map(axis), unit: unit,
                                 xTicks: keys.enumerated().map { xTick($0.element, $0.offset) }, titles: keys.map(title), readouts: readouts,
                                 texts: texts, lineTexts: lineTexts ?? [])
    }

    static func linesModel(
        label: String, keys: [String], series: [ReportLineSeries], format: (Double) -> String, axis: (Double) -> String, unit: String?,
        xTick: (String, Int) -> ReportXTick? = { key, _ in ReportMath.monthXTick(key) }, title: (String) -> String = { ReportMath.monthTitle($0) },
        wash: Bool? = nil
    ) -> ReportLinesModel {
        let known = series.flatMap { $0.values.compactMap { $0 } }
        let ticks = niceTicks(Swift.min(0, known.min() ?? 0), Swift.max(1, known.max() ?? 1))
        func at(_ s: ReportLineSeries, _ i: Int) -> Double? { i < s.values.count ? s.values[i] : nil }
        let readouts = keys.indices.map { i in
            series.compactMap { s in at(s, i).map { ReportReadout(label: s.label, value: format($0), color: s.color, line: true, dashed: s.dashed) } }
        }
        let ends = series.map { s -> String? in
            guard series.count <= 4, let last = s.values.last(where: { $0 != nil }) ?? nil else { return nil }
            return format(last)
        }
        let texts = series.map { s in keys.indices.map { at(s, $0).map(format) } }
        return ReportLinesModel(label: label, keys: keys, series: series, wash: wash ?? (series.count == 1), ticks: ticks, tickLabels: ticks.map(axis),
                                unit: unit, xTicks: keys.enumerated().map { xTick($0.element, $0.offset) }, titles: keys.map(title),
                                readouts: readouts, endLabels: ends, texts: texts)
    }
}

/// Five steps of magnitude for a heat calendar or treemap.
struct ReportHeatScale: Sendable {
    var cuts: [Int64]?

    func level(_ value: Int64) -> Int {
        guard value > 0, let cuts else { return 0 }
        return 1 + cuts.filter { value > $0 }.count
    }
}

/// A tile placed by `squarify`.
struct ReportTileBox<T> {
    var item: T
    var x: Double
    var y: Double
    var w: Double
    var h: Double
}

// MARK: Engine figures

struct ReportMonth: Sendable {
    var month: String
    var income: Int64
    var spending: Int64
    var netIncome: Int64
    var assets: Int64
    var debt: Int64
    var netWorth: Int64
    var inflow: Int64
    var outflow: Int64
    var categories: ReportAmounts
    var incomeSources: ReportAmounts
    var accounts: ReportAmounts
}

struct ReportPayeeSpend: Equatable, Sendable {
    var name: String
    var amount: Int64
    var count: Int64
}

struct ReportData: Sendable {
    var months: [ReportMonth]
    var categories: ReportAmounts
    var incomeSources: ReportAmounts
    /// Spending by payee, most first.
    var payees: [ReportPayeeSpend]
    var days: ReportAmounts
    var income: Int64
    var spending: Int64
    var transactionCount: Int64?
    var ageOfMoney: Int64?
    var openingAccounts: ReportAmounts?
    var ageOfMoneyByMonth: [String: Int64?]
}

struct ReportAccount: Equatable, Sendable {
    var id: String
    var name: String
    var kind: String
    var archived: Bool
    var onBudget: Bool
    var inNetWorth: Bool
    var chosenType: String
    var importedType: String
    var openingBalance: Int64

    func type(_ balance: Int64? = nil) -> String {
        ReportMath.accountType(chosen: chosenType, imported: importedType, kind: kind, balance: balance ?? openingBalance)
    }
}

struct ReportCategory: Equatable, Sendable {
    var id: String
    var name: String
    var group: String
    var payment: Bool
}

/// What reports need from a budget snapshot: names, groups in order, accounts and the first month with data.
struct ReportBudget: Sendable {
    var currency: String
    var accounts: [ReportAccount]
    /// The budget's own categories, in their saved order (no report-only bucket).
    var categories: [ReportCategory]
    /// Group and name of every category, the "Needs a category" bucket included.
    var groupOf: [String: String]
    var nameOf: [String: String]
    /// Groups that can hold spending, in budget order, "Needs a category" last.
    var spendingGroups: [String]
    var earliestMonth: String
    var accountsById: [String: ReportAccount]
    /// The currency over a money axis, unless amounts are shown without one.
    var axisUnit: String?

    init(currency: String, accounts: [ReportAccount], categories: [ReportCategory], groupOf: [String: String], nameOf: [String: String],
         spendingGroups: [String], earliestMonth: String) {
        self.currency = currency
        self.accounts = accounts
        self.categories = categories
        self.groupOf = groupOf
        self.nameOf = nameOf
        self.spendingGroups = spendingGroups
        self.earliestMonth = earliestMonth
        accountsById = Dictionary(accounts.map { ($0.id, $0) }, uniquingKeysWith: { _, last in last })
        // Without the direction marks Arabic adds around an amount, which aren't a currency sign.
        let zero = Money.plain(0, currency: currency)
        axisUnit = zero.contains { !("0"..."9").contains($0) && !".,' -".contains($0) } ? currency : nil
    }

    func money(_ cents: Int64) -> String { Money.format(cents, currency: currency) }
    func money(rounding cents: Double) -> String { Money.format(ReportMath.jround(cents), currency: currency) }
    /// "+12.00" for a gain, kept whole left to right in Arabic.
    func signed(_ cents: Int64) -> String {
        cents > 0 ? AppLanguage.leftToRight("+" + Money.plain(cents, currency: currency)) : money(cents)
    }

    /// A group's colour: the first seven groups in budget order have their own.
    func colorOf(_ group: String) -> Int {
        if let index = spendingGroups.firstIndex(of: group), index <= 6 { return index }
        return ReportColors.other
    }

    func accountName(_ id: String?) -> String { id.flatMap { accountsById[$0]?.name } ?? "" }
}

// MARK: Ranges and filters

enum ReportRangePreset: String, CaseIterable, Hashable, Sendable {
    case thisMonth, lastMonth, last3, last6, last12, ytd, lastYear, all, custom

    var label: String {
        switch self {
        case .thisMonth: return String(localized: "This month")
        case .lastMonth: return String(localized: "Last month")
        case .last3: return String(localized: "Last 3 months")
        case .last6: return String(localized: "Last 6 months")
        case .last12: return String(localized: "Last 12 months")
        case .ytd: return String(localized: "Year to date")
        case .lastYear: return String(localized: "Last year")
        case .all: return String(localized: "All dates")
        case .custom: return String(localized: "Custom range")
        }
    }
}

/// Inclusive months ("YYYY-MM"); an empty end means unbounded.
struct ReportRange: Hashable, Sendable {
    var preset: ReportRangePreset
    var from: String
    var to: String
}

struct ReportFilterItem: Identifiable, Hashable, Sendable {
    var id: String
    var label: String
}

struct ReportFilterGroup: Identifiable, Hashable, Sendable {
    var name: String
    var items: [ReportFilterItem]
    var id: String { name }
}

// MARK: Chart models

/// An x-axis label: "Oct", or "Oct 26" when the year changes; `year` is 0 for labels that aren't months.
struct ReportXTick: Hashable, Sendable {
    var short: String
    var long: String
    var year: Int
}

/// One line of a chart's readout, the phone's stand-in for the desktop's hover tooltip.
struct ReportReadout: Hashable, Sendable {
    var label: String
    var value: String
    var color: Int?
    var line = false
    var dashed = false
}

struct ReportLegendItem: Hashable, Sendable {
    var label: String
    var color: Int
    var line = false
    var dashed = false
}

struct ReportColumnSeries: Hashable, Sendable {
    var key: String
    var label: String
    var color: Int
    var values: [Double]
}

struct ReportLineOverlay: Hashable, Sendable {
    var label: String
    var color: Int
    var values: [Double?]
}

struct ReportColumnModel: Hashable, Sendable {
    var label: String
    var keys: [String]
    var series: [ReportColumnSeries]
    var grouped: Bool
    var line: ReportLineOverlay?
    var average: Double?
    var averageText: String?
    var ticks: [Double]
    var tickLabels: [String]
    var unit: String?
    var xTicks: [ReportXTick?]
    var titles: [String]
    var readouts: [[ReportReadout]]
    /// Each series' value at each key, formatted, for VoiceOver.
    var texts: [[String]]
    var lineTexts: [String?]
}

struct ReportLineSeries: Hashable, Sendable {
    var key: String
    var label: String
    var color: Int
    var values: [Double?]
    var dashed = false
}

struct ReportLinesModel: Hashable, Sendable {
    var label: String
    var keys: [String]
    var series: [ReportLineSeries]
    var wash: Bool
    var ticks: [Double]
    var tickLabels: [String]
    var unit: String?
    var xTicks: [ReportXTick?]
    var titles: [String]
    var readouts: [[ReportReadout]]
    var endLabels: [String?]
    /// Each series' value at each key, formatted, for VoiceOver.
    var texts: [[String?]]
}
