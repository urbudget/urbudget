import SwiftUI

/// An investment account's page (the desktop's Holdings view, Investments.tsx; Android's
/// HoldingsScreen.kt): how it did over a period with a chart of its value or its gain, the
/// period's ups and downs, what it holds with each holding's share and return, each month's
/// gain, and what it earned and was charged. The figures are the engine's `investmentPage`
/// (crates/urbudget-core investment_stats.rs); here they are only picked apart for the screen.
enum HoldingsPage {
    /// A month, three, since January, a year, or since tracking began: the engine's own names.
    static let periods = ["1m", "3m", "ytd", "1y", "all"]
    /// Holdings listed before "Show N more holdings".
    static let first = 10

    /// A share as a whole percent ("63%"), a sliver as "<1%"; signed for a rate of return ("+5.4%").
    static func percent(_ share: Double, signed: Bool = false) -> String {
        if signed {
            let tenths = Double(ReportMath.jround(share * 1000)) / 10
            return (tenths > 0 ? "+" : "") + String(format: "%.1f", tenths) + "%"
        }
        let whole = ReportMath.jround(share * 100)
        return share > 0 && whole == 0 ? "<1%" : "\(whole)%"
    }

    /// A small share to a tenth of a percent ("0.3%"), for the ones a whole percent would round
    /// away; a sliver as "<0.1%".
    static func tenths(_ share: Double) -> String {
        let part = abs(share)
        if part > 0 && part < 0.0005 { return "<0.1%" }
        let text = percent(part, signed: true)
        return text.hasPrefix("+") ? String(text.dropFirst()) : text
    }

    /// A percentage, left to right in an Arabic sentence too. One that opens with "<" has no
    /// letter or digit at its head to say so: a left-to-right mark leads it.
    static func figure(_ text: String) -> String {
        AppLanguage.leftToRight(text.hasPrefix("<") ? "\u{200E}" + text : text)
    }

    /// A share or rate the engine may leave out.
    static func number(_ row: JSON, _ key: String) -> Double? {
        switch row[key] {
        case .double(let value): return value.isFinite ? value : nil
        case .int(let value): return Double(value)
        default: return nil
        }
    }

    /// A figure the engine may leave out.
    static func amount(_ row: JSON, _ key: String) -> Int64? {
        row[key].isNull ? nil : row.long(key)
    }

    static func period(_ page: JSON, _ id: String) -> JSON? {
        page.rows("periods").first { $0.text("period") == id }
    }

    /// A period's days, the value on each, and the gain so far on each: it starts at nothing.
    struct Series: Equatable, Sendable {
        var days: [String]
        var values: [Int64]
        var gains: [Int64]
    }

    /// A period's days out of the page's whole run: its gains are the run's, less the figure on
    /// its first day.
    static func series(_ page: JSON, _ period: JSON) -> Series {
        let days = page["days"].array.map(\.string)
        let values = page["values"].array.map(\.int)
        let gains = page["gains"].array.map(\.int)
        let count = min(days.count, values.count, gains.count)
        guard count > 0 else { return Series(days: [], values: [], gains: []) }
        let first = min(max(0, Int(period.long("first"))), count - 1)
        let base = gains[first]
        return Series(days: Array(days[first..<count]), values: Array(values[first..<count]), gains: gains[first..<count].map { $0 - base })
    }

    /// What the account would be worth with no gain or loss since the period began: the gap to
    /// its value is the gain. Nothing where more was taken out than was put in.
    static func before(_ series: Series) -> [Double?] {
        series.values.indices.map { index in
            let value = series.values[index]
            let gain = series.gains[index]
            return value >= 0 && value < gain ? nil : Double(value - gain)
        }
    }

    /// What a day under the chart is marked with.
    enum Tick: Equatable, Sendable { case day, month, year }

    /// Days since 1 January 1970 for a day written "2026-11-13".
    static func epochDay(_ iso: String) -> Int? {
        let parts = iso.prefix(10).split(separator: "-").compactMap { Int($0) }
        guard parts.count == 3 else { return nil }
        let month = parts[1]
        let year = month <= 2 ? parts[0] - 1 : parts[0]
        let era = (year >= 0 ? year : year - 399) / 400
        let yearOfEra = year - era * 400
        let dayOfYear = (153 * (month + (month > 2 ? -3 : 9)) + 2) / 5 + parts[2] - 1
        let dayOfEra = yearOfEra * 365 + yearOfEra / 4 - yearOfEra / 100 + dayOfYear
        return era * 146_097 + dayOfEra - 719_468
    }

    /// The week a day falls in, turning on Monday.
    private static func week(_ day: String) -> Int {
        Int((Double((epochDay(day) ?? 0) + 3) / 7).rounded(.down))
    }

    /// The chart's dates: a month or two is marked by the week, up to four months every other
    /// week, and anything longer by the month, with the year where it turns.
    static func ticks(_ days: [String]) -> [Tick?] {
        let months = Set(days.map { String($0.prefix(7)) }).count
        return days.indices.map { index -> Tick? in
            if index == 0 { return nil }
            let day = days[index]
            let earlier = days[index - 1]
            if months <= 4 {
                let odd = ((week(day) % 2) + 2) % 2 != 0
                return week(day) == week(earlier) || (months > 2 && odd) ? nil : .day
            }
            let january = day.dropFirst(5).prefix(2) == "01"
            if earlier.prefix(7) == day.prefix(7) || (months > 14 && !january) { return nil }
            return january ? .year : .month
        }
    }

    /// Round marks that cover `low` to `high` without going down to zero: an axis from zero
    /// would flatten an account that moves by a hundredth of its size.
    static func fittedTicks(_ low: Double, _ high: Double, count: Int = 4) -> [Double] {
        guard high > low, low.isFinite, high.isFinite else { return ReportMath.niceTicks(low, high) }
        let raw = (high - low) / Double(count)
        let power = pow(10, log10(raw).rounded(.down))
        let step = [1.0, 2.0, 2.5, 5.0, 10.0].map { $0 * power }.first { raw <= $0 } ?? 10 * power
        var ticks: [Double] = []
        var value = (low / step).rounded(.down) * step
        repeat {
            ticks.append(Double(ReportMath.jround(value * 1e6)) / 1e6)
            value += step
        } while (ticks.last ?? high) < high && ticks.count < 64
        return ticks
    }

    /// The holdings by value, gain or return, largest first, or by name; ones without the
    /// figure come last.
    static func sorted(_ rows: [JSON], by key: String) -> [JSON] { order(rows, by: key).map { rows[$0] } }

    /// Where each holding stands in that order, as its place among `rows`.
    static func order(_ rows: [JSON], by key: String) -> [Int] {
        func ordered(_ before: (JSON, JSON) -> Bool?) -> [Int] {
            rows.enumerated().sorted { first, second in before(first.element, second.element) ?? (first.offset < second.offset) }.map(\.offset)
        }
        switch key {
        case "name":
            return ordered { first, second in
                let a = first.text("name").lowercased()
                let b = second.text("name").lowercased()
                return a == b ? nil : a < b
            }
        case "gain":
            return ordered { first, second in
                let a = amount(first, "gain") ?? Int64.min
                let b = amount(second, "gain") ?? Int64.min
                return a == b ? nil : a > b
            }
        case "rate":
            return ordered { first, second in
                let a = number(first, "rate") ?? -Double.infinity
                let b = number(second, "rate") ?? -Double.infinity
                return a == b ? nil : a > b
            }
        default:
            return ordered { first, second in
                let a = first.long("value")
                let b = second.long("value")
                return a == b ? nil : a > b
            }
        }
    }

    /// The letters on a holding's tile: its symbol's first word ("SPY" of "SPY 18DEC26 710 P",
    /// "ARCC" of a bond's "ARCC 5 1/4 04/12/31"), or its name's when it has no symbol.
    static func mark(symbol: String, name: String) -> String {
        let from = symbol.trimmingCharacters(in: .whitespaces).isEmpty ? name : symbol
        let word = from.split(whereSeparator: { " ./".contains($0) }).first.map(String.init) ?? ""
        return String(word.prefix(6)).uppercased(with: Locale(identifier: "en_US_POSIX"))
    }

    /// A holding's own line: its symbol when that adds to its name, then "120.5 × 141.9087 USD".
    static func line(_ row: JSON) -> String {
        let symbol = row.text("symbol")
        // A bond's name is its symbol, and an option's code only spells its name out again at length.
        let shown = row.text("name") != symbol && symbol.count <= 12 && !symbol.isEmpty ? "\(symbol) · " : ""
        return "\(shown)\(Investments.plain(row.text("quantity"))) × \(Investments.plain(row.text("price"))) \(row.text("currency"))"
    }

    /// What the engine calls a kind of holding ("Stocks", "Stock"), in the interface's language.
    static func kindName(_ key: String) -> String {
        switch key {
        case "Stocks": return String(localized: "Stocks")
        case "Funds": return String(localized: "Funds")
        case "Bonds": return String(localized: "Bonds")
        case "Options": return String(localized: "Options")
        case "Futures": return String(localized: "Futures")
        case "Currencies": return String(localized: "Currencies")
        case "Crypto": return String(localized: "Crypto")
        case "Commodities": return String(localized: "Commodities")
        case "CFDs": return String(localized: "CFDs")
        case "Cash": return String(localized: "Cash")
        case "Accrued": return String(localized: "Accrued")
        case "Stock": return String(localized: "Stock")
        case "Fund": return String(localized: "Fund")
        case "Bond": return String(localized: "Bond")
        case "Option": return String(localized: "Option")
        case "Future": return String(localized: "Future")
        case "Currency": return String(localized: "Currency")
        case "Commodity": return String(localized: "Commodity")
        case "CFD": return String(localized: "CFD")
        case "Other": return String(localized: "Other")
        case "Holdings": return String(localized: "Holdings")
        default: return String(localized: "Other holdings")
        }
    }

    /// What a holding is, as the engine names it in English ("ETF", "Call option"), in the interface's
    /// language. The names it shares with the kinds' singulars ("Stock", "Fund", "Option") are the kinds' own.
    static func instrumentName(_ key: String) -> String {
        switch key {
        case "ETF": return String(localized: "ETF")
        case "Exchange-traded note": return String(localized: "Exchange-traded note")
        case "REIT": return String(localized: "REIT")
        case "Depositary receipt": return String(localized: "Depositary receipt")
        case "Preferred share": return String(localized: "Preferred share")
        case "Right": return String(localized: "instrument.right", defaultValue: "Right", comment: "A holding that is a right to buy new shares.")
        case "Closed-end fund": return String(localized: "Closed-end fund")
        case "Treasury bill": return String(localized: "Treasury bill")
        case "Government bond": return String(localized: "Government bond")
        case "Corporate bond": return String(localized: "Corporate bond")
        case "Call option": return String(localized: "Call option")
        case "Put option": return String(localized: "Put option")
        default: return kindName(key)
        }
    }

    /// An amount in its own currency, "61,476.00 USD": the decimal as the engine writes it, grouped
    /// and with the decimal mark of the budget's number format, and kept whole left to right in Arabic.
    static func ownText(_ amount: String, code: String) -> String {
        var number = amount
        if amount.range(of: #"^-?[0-9]+(\.[0-9]+)?$"#, options: .regularExpression) != nil {
            let negative = amount.hasPrefix("-")
            let digits = negative ? String(amount.dropFirst()) : amount
            let parts = digits.split(separator: ".", maxSplits: 1).map(String.init)
            let whole = Array(parts.first ?? "0")
            let style = Money.format
            var grouped = ""
            for (index, digit) in whole.enumerated() {
                if index > 0 && (whole.count - index) % 3 == 0 { grouped += style.group }
                grouped.append(digit)
            }
            number = (negative ? "-" : "") + grouped + (parts.count > 1 ? style.decimal + parts[1] : "")
        }
        return AppLanguage.leftToRight(code.isEmpty ? number : "\(number) \(code)")
    }

    /// An amount in its own currency, shown only where that currency isn't the budget's (a riyal
    /// holding in a riyal budget already shows its value); nil where it isn't known.
    static func ownAmount(_ amount: String?, code: String, budget: String) -> String? {
        guard let amount, !amount.isEmpty, !code.isEmpty, code.uppercased() != budget.uppercased() else { return nil }
        return ownText(amount, code: code)
    }

    /// What a section the Flex Query leaves out costs this page, by the section's name in the statement.
    static func lack(_ section: String) -> String? {
        switch section {
        case "Net Asset Value (NAV) in Base":
            return String(localized: "Net Asset Value (NAV) in Base: without it the value is only known on the days you sync, so the chart and the gains start from there.")
        case "Open Positions": return String(localized: "Open Positions: without it the holdings aren't listed.")
        case "Cash Transactions":
            return String(localized: "Cash Transactions: without it deposits, dividends, interest and fees aren't known, and money you put in can be counted as a gain.")
        case "Cash Report": return String(localized: "Cash Report: without it cash and the commissions on trades aren't shown.")
        case "Transfers": return String(localized: "Transfers: without it money moved in from another account is only estimated, from jumps in the value.")
        case "Trades": return String(localized: "Trades: without it realised gains and commissions on trades aren't shown by month.")
        default: return nil
        }
    }

    static func periodName(_ period: String) -> String {
        switch period {
        case "1m": return String(localized: "1M")
        case "3m": return String(localized: "3M")
        case "ytd": return String(localized: "YTD")
        case "1y": return String(localized: "1Y")
        default: return String(localized: "All")
        }
    }

    static func tone(_ amount: Int64) -> ReportTone { amount > 0 ? .positive : amount < 0 ? .negative : .none }

    static func color(_ tone: ReportTone) -> Color {
        switch tone {
        case .positive: return .urPositive
        case .negative: return .urNegative
        case .none: return .primary
        }
    }
}

/// A figure with its name, and a line of detail under it.
struct HoldingsStat: Identifiable, Equatable {
    var label: String
    var value: String
    var note: String?
    var tone = ReportTone.none
    var id: String { label }
}

/// One thing the account holds, as its row shows it. Its id is its place in the statement, so a
/// new order moves its row rather than making it again.
struct HoldingsLine: Identifiable, Equatable {
    var id: String
    var name: String
    var line: String?
    var value: String
    var gain: String?
    /// Its return, in a pill beside the gain.
    var rate: String?
    var sign: Int64 = 0
    /// The few letters on its tile (a symbol, a currency); none for interest still to be paid, which has a clock.
    var mark: String?
    var share: String?
    /// How much of the bar is filled: its value against the account's largest line.
    var bar: Double = 0
    var color: Int = ReportColors.other
}

/// A line of what the account earned or was charged.
struct HoldingsIncomeLine: Identifiable, Equatable {
    var label: String
    var note: String?
    var amount: String
    var tone: ReportTone
    var id: String { label }
}

/// A day counted as money moved on an estimate.
struct HoldingsEstimate: Identifiable, Equatable {
    var day: String
    var amount: String
    var id: String { day }
}

/// The page's figures, written out for the screen.
struct HoldingsPageModel {
    var total = ""
    /// "Prices from 2026-11-17", under the value; amber once they're a few days old.
    var asOf = ""
    var stale = false
    var gainLabel = ""
    var gain = ""
    var gainTone = ReportTone.none
    var rate: String?
    var valueChart: ReportLinesModel?
    var gainChart: ReportLinesModel?
    var periodStats: [HoldingsStat] = []
    var lacking: [String] = []
    var estimated: String?
    var estimatedDays: [HoldingsEstimate] = []
    var kinds: [ReportSlice] = []
    /// Each kind's share of the account, by its key.
    var kindShares: [String: String] = [:]
    var currencies: String?
    var holdStats: [HoldingsStat] = []
    var count = 0
    var holdings: [HoldingsLine] = []
    var others: [HoldingsLine] = []
    var months: ReportColumnModel?
    var incomeSpan: String?
    var incomeStats: [HoldingsStat] = []
    var income: [HoldingsIncomeLine] = []
    var footnote = ""
}

extension HoldingsPage {
    private static func percentText(_ share: Double, signed: Bool = false) -> String { figure(percent(share, signed: signed)) }

    /// The currency named beside the axis, when amounts are written with a sign or code of their own.
    private static func axisUnit(_ currency: String) -> String? {
        let zero = Money.format(0, currency: currency)
        return zero.contains { !("0"..."9").contains($0) && !".,' -".contains($0) } ? currency : nil
    }

    /// The chart's dates under its axis: "3 Nov" by the week, "Nov" by the month, the year where it turns.
    private static func dayTicks(_ days: [String]) -> [ReportXTick?] {
        let marks = ticks(days)
        return days.indices.map { index -> ReportXTick? in
            guard let mark = marks[index] else { return nil }
            let day = days[index]
            switch mark {
            case .day: return ReportMath.plainTick(Dates.dayAndMonth(day) ?? day)
            case .month: return ReportMath.plainTick(Dates.shortMonthName(String(day.prefix(7))))
            case .year: return ReportMath.plainTick(String(day.prefix(4)))
            }
        }
    }

    /// What the account is worth, over what it would be worth with no gain or loss since the
    /// period began: the gap between the two lines is the gain. The axis fits the values.
    private static func valueChart(_ series: Series, label: String, currency: String) -> ReportLinesModel {
        let value = ReportLineSeries(key: "value", label: String(localized: "Value"), color: 0, values: series.values.map { Double($0) })
        let earlier = ReportLineSeries(key: "before", label: String(localized: "Value before gains"), color: ReportColors.other, values: before(series), dashed: true)
        // The dashed line goes down first, so the value and its end dot are drawn over it where they meet.
        let lines = [earlier, value]
        let known = lines.flatMap { $0.values.compactMap { $0 } }
        let marks = fittedTicks(known.min() ?? 0, known.max() ?? 1)
        func written(_ value: Double) -> String { Money.format(ReportMath.jround(value), currency: currency) }
        // A finger on the chart reads the day's value and gain first, then the line under.
        let readouts = series.days.indices.map { index -> [ReportReadout] in
            var rows: [ReportReadout] = []
            if let shown = value.values[index] { rows.append(ReportReadout(label: value.label, value: written(shown), color: value.color, line: true)) }
            rows.append(ReportReadout(label: String(localized: "Gain"), value: Investments.signed(series.gains[index], currency), color: nil))
            if let shown = earlier.values[index] { rows.append(ReportReadout(label: earlier.label, value: written(shown), color: earlier.color, line: true, dashed: true)) }
            return rows
        }
        // No wash under the line: the axis doesn't reach zero, where a wash would end.
        return ReportLinesModel(label: label, keys: series.days, series: lines, wash: false, ticks: marks, tickLabels: marks.map { ReportMath.compactMoney($0) },
                                unit: axisUnit(currency), xTicks: dayTicks(series.days), titles: series.days.map { Money.date($0) }, readouts: readouts,
                                endLabels: [String?](repeating: nil, count: lines.count), texts: lines.map { line in line.values.map { $0.map(written) } })
    }

    /// The gain so far on each day of the period, from nothing: green when it ends up, red when it ends down.
    private static func gainChart(_ series: Series, label: String, currency: String) -> ReportLinesModel {
        let color = (series.gains.last ?? 0) < 0 ? ReportColors.negative : ReportColors.positive
        let marks = dayTicks(series.days)
        var model = ReportMath.linesModel(
            label: label, keys: series.days,
            series: [ReportLineSeries(key: "gain", label: String(localized: "Gain"), color: color, values: series.gains.map { Double($0) })],
            format: { Investments.signed(ReportMath.jround($0), currency) }, axis: { ReportMath.compactMoney($0) }, unit: axisUnit(currency),
            xTick: { _, index in index < marks.count ? marks[index] : nil }, title: { Money.date($0) }, wash: true, current: nil)
        model.endLabels = [nil]
        model.readouts = model.readouts.enumerated().map { index, row in
            row + [ReportReadout(label: String(localized: "Value"), value: Money.format(series.values[index], currency: currency), color: nil)]
        }
        return model
    }

    /// Each month's gain as a column, up or down.
    private static func monthsChart(_ months: [JSON], currency: String) -> ReportColumnModel {
        let gains = months.map { Double($0.long("gain")) }
        var model = ReportMath.columnModel(
            label: String(localized: "The account's gain in each month"), keys: months.map { $0.text("month") },
            series: [ReportColumnSeries(key: "gain", label: String(localized: "Gain"), color: ReportColors.positive, values: gains.map { max(0, $0) }),
                     ReportColumnSeries(key: "loss", label: String(localized: "Loss"), color: ReportColors.negative, values: gains.map { min(0, $0) })],
            format: { Investments.signed(ReportMath.jround($0), currency) }, axis: { ReportMath.compactMoney($0) }, unit: axisUnit(currency), current: nil)
        // A month is a gain or a loss, never both: their total would only say it twice.
        model.readouts = model.readouts.map { row in row.filter { $0.color != nil } }
        return model
    }

    /// The page's figures for a period, with the holdings in the order asked for.
    static func model(_ page: JSON, period: String, sort: String, currency: String) -> HoldingsPageModel {
        var view = HoldingsPageModel()
        let total = page.long("total")
        func signed(_ amount: Int64) -> String { Investments.signed(amount, currency) }
        func ofAccount(_ share: String) -> String { String(localized: "\(share) of the account") }
        let did = HoldingsPage.period(page, period) ?? HoldingsPage.period(page, "all")
        let run = did.map { series(page, $0) }
        let from = did.map { Money.date($0.text("from")) } ?? ""
        let to = did.map { Money.date($0.text("to")) } ?? ""
        let gain = did?.long("gain") ?? 0

        view.total = money(total, currency)
        switch period {
        case "1m": view.gainLabel = String(localized: "Gain over 1 month")
        case "3m": view.gainLabel = String(localized: "Gain over 3 months")
        case "ytd": view.gainLabel = String(localized: "Gain this year")
        case "1y": view.gainLabel = String(localized: "Gain over 1 year")
        default: view.gainLabel = String(localized: "Gain since \(from)")
        }
        view.gain = signed(gain)
        view.gainTone = tone(gain)
        view.rate = did.flatMap { number($0, "rate") }.map { percentText($0, signed: true) }
        if let run, run.days.count > 2 {
            view.valueChart = valueChart(run, label: String(localized: "The account's value from \(from) to \(to)"), currency: currency)
            view.gainChart = gainChart(run, label: String(localized: "The account's gain from \(from) to \(to)"), currency: currency)
        }

        // How the period went.
        let change = page["dayChange"]
        if change.isObject {
            let asOf = Money.date(page.text("asOf"))
            view.periodStats.append(HoldingsStat(label: String(localized: "Change on \(asOf)"), value: signed(change.long("amount")), tone: tone(change.long("amount"))))
        }
        if let did {
            view.periodStats.append(HoldingsStat(label: String(localized: "Money put in"), value: signed(did.long("put")), note: String(localized: "Since \(from)")))
            let moves = did["swings"]
            if moves.isObject {
                let best = moves["best"]
                if best.isObject {
                    view.periodStats.append(HoldingsStat(label: String(localized: "Best day"), value: signed(best.long("amount")), note: Money.date(best.text("date")), tone: .positive))
                }
                let worst = moves["worst"]
                if worst.isObject {
                    view.periodStats.append(HoldingsStat(label: String(localized: "Worst day"), value: signed(worst.long("amount")), note: Money.date(worst.text("date")), tone: .negative))
                }
                let drop = moves["drop"]
                if drop.isObject {
                    let start = Money.date(drop.text("from"))
                    let end = Money.date(drop.text("to"))
                    let span = String(localized: "\(start) to \(end)")
                    let note = number(drop, "rate").map { "\(percentText($0, signed: true)) · \(span)" } ?? span
                    view.periodStats.append(HoldingsStat(label: String(localized: "Largest drop"), value: signed(drop.long("amount")), note: note, tone: .negative))
                }
                let moved = Int(moves.long("moved"))
                let up = Int(moves.long("up"))
                if moved > 0 {
                    view.periodStats.append(HoldingsStat(label: String(localized: "Days up"), value: percentText(Double(up) / Double(moved)),
                                                         note: String(localized: "\(up) of \(moved) days", locale: .app)))
                }
            }
        }

        // What it's made of, among the parts worth something: a short option isn't a share of it.
        let sums = page["totals"]
        let byKind = sums.rows("byKind")
        func kindColor(_ key: String) -> Int {
            guard let index = byKind.firstIndex(where: { $0.text("key") == key }), index <= 7 else { return ReportColors.other }
            return index
        }
        for part in byKind where part.long("value") > 0 {
            let key = part.text("key")
            view.kinds.append(ReportSlice(key: key, label: kindName(key), value: part.long("value"), valueText: money(part.long("value"), currency), color: kindColor(key)))
            view.kindShares[key] = percentText(max(0, number(part, "share") ?? 0))
        }
        let byCurrency = sums.rows("byCurrency").filter { $0.long("value") > 0 }
        if byCurrency.count > 1 {
            view.currencies = AppLanguage.leftToRight(byCurrency.map { "\($0.text("key")) \(percent(max(0, number($0, "share") ?? 0)))" }.joined(separator: " · "))
        }
        if let unrealised = amount(sums, "unrealised") {
            let rate = number(sums, "unrealisedRate").map { percentText($0, signed: true) } ?? "—"
            let cost = money(sums.long("cost"), currency)
            view.holdStats.append(HoldingsStat(label: String(localized: "Gain on what you hold"), value: signed(unrealised),
                                               note: String(localized: "\(rate) on a cost of \(cost)"), tone: tone(unrealised)))
        }
        let cash = sums.long("cash")
        view.holdStats.append(HoldingsStat(label: String(localized: "Cash"), value: money(cash, currency),
                                           note: total > 0 ? ofAccount(percentText(cash > 0 ? Double(cash) / Double(total) : 0)) : nil))
        let biggest = page["biggest"]
        if biggest.isObject, let share = number(biggest, "rate") {
            view.holdStats.append(HoldingsStat(label: String(localized: "Largest holding"), value: percentText(share), note: biggest.text("name")))
        }
        for (key, label) in [("best", String(localized: "Best performer")), ("worst", String(localized: "Worst performer"))] {
            let named = page[key]
            guard named.isObject, let rate = number(named, "rate") else { continue }
            view.holdStats.append(HoldingsStat(label: label, value: percentText(rate, signed: true), note: named.text("name"),
                                               tone: rate > 0 ? .positive : rate < 0 ? .negative : .none))
        }

        // Each holding, with its bar drawn against the largest line of the account.
        let largest = max(1, page.long("largest"))
        func bar(_ value: Int64) -> Double { value > 0 ? min(1, max(0.02, Double(value) / Double(largest))) : 0 }
        let positions = page.rows("holdings")
        view.count = positions.count
        view.holdings = order(positions, by: sort).map { place -> HoldingsLine in
            let row = positions[place]
            let rowGain = amount(row, "gain")
            // What it is: the engine's instrument ("ETF"), else, from an older engine, its kind's singular.
            let what = row.text("instrument").isEmpty ? row.text("one") : row.text("instrument")
            return HoldingsLine(
                id: "holding:\(place):\(row.text("symbol")):\(row.text("currency"))", name: row.text("name"),
                // A source that doesn't say what a holding is leaves its kind out.
                line: (what.isEmpty ? "" : "\(instrumentName(what)) · ") + AppLanguage.leftToRight(line(row)), value: money(row.long("value"), currency),
                gain: rowGain.map(signed), rate: number(row, "rate").map { percentText($0, signed: true) }, sign: rowGain ?? 0,
                mark: mark(symbol: row.text("symbol"), name: row.text("name")),
                share: number(row, "share").map { percentText($0) }, bar: bar(row.long("value")), color: kindColor(row.text("group")))
        }
        view.others = page.rows("cash").map { row -> HoldingsLine in
            HoldingsLine(id: "cash:\(row.text("currency"))", name: String(localized: "Cash"), line: Investments.cashLine(row), value: money(row.long("value"), currency),
                         mark: row.text("currency").uppercased(), share: number(row, "share").map { percentText($0) }, bar: bar(row.long("value")), color: kindColor("Cash"))
        }
        let accrued = page.long("accrued")
        if accrued != 0 {
            view.others.append(HoldingsLine(id: "accrued", name: String(localized: "Accrued"), line: String(localized: "Dividends and interest earned but not yet paid"),
                                            value: money(accrued, currency), share: total > 0 && accrued > 0 ? percentText(Double(accrued) / Double(total)) : nil,
                                            bar: bar(accrued), color: kindColor("Accrued")))
        }

        // What it earned and was charged.
        let paid = page["earned"]
        if paid.isObject {
            let commissions = paid["commissions"]
            let income = paid.long("dividends") + paid.long("interest")
            let costs = paid.long("fees") + paid.long("tax") + (commissions.isObject ? commissions.long("amount") : 0)
            func share(_ amount: Int64) -> String? { total > 0 && amount != 0 ? ofAccount(figure(tenths(Double(amount) / Double(total)))) : nil }
            view.incomeStats = [
                HoldingsStat(label: String(localized: "Dividends and interest"), value: signed(income), note: share(income), tone: tone(income)),
                HoldingsStat(label: String(localized: "Fees, tax and commissions"), value: signed(costs), note: share(costs), tone: costs < 0 ? .negative : .none),
            ]
            let rows: [(String, String, String?)] = [
                (String(localized: "Dividends"), "dividends", nil), (String(localized: "Interest"), "interest", nil), (String(localized: "Tax withheld"), "tax", nil),
                (String(localized: "Fees"), "fees", String(localized: "Market data, advisers and other charges apart from trades")), (String(localized: "Other"), "other", nil),
            ]
            for (label, key, note) in rows where paid.long(key) != 0 {
                view.income.append(HoldingsIncomeLine(label: label, note: note, amount: signed(paid.long(key)), tone: tone(paid.long(key))))
            }
            if commissions.isObject {
                let start = Money.date(commissions.text("from"))
                let end = Money.date(commissions.text("to"))
                view.income.append(HoldingsIncomeLine(label: String(localized: "Commissions on trades"), note: String(localized: "\(start) to \(end)"),
                                                      amount: signed(commissions.long("amount")), tone: tone(commissions.long("amount"))))
            }
            let first = ReportMath.monthName(paid.text("from"), short: true)
            let last = ReportMath.monthName(paid.text("to"), short: true)
            view.incomeSpan = String(localized: "\(first) to \(last)")
        }

        view.lacking = page["missing"].array.compactMap { lack($0.string) }
        let estimated = page.rows("estimated")
        if !estimated.isEmpty {
            let count = estimated.count
            view.estimated = String(localized: "On \(count) days the value jumped with no deposit or transfer listed, so those changes are counted as money moved, not as gains. Add the Transfers section to your Flex Query for exact figures.", locale: .app)
            view.estimatedDays = estimated.map { HoldingsEstimate(day: Money.date($0.text("date")), amount: signed($0.long("amount"))) }
        }
        let months = page.rows("monthly")
        if months.count > 1 { view.months = monthsChart(months, currency: currency) }

        let id = page.text("sourceAccount")
        let asOf = page.text("asOf")
        view.asOf = String(localized: "Prices from \(Money.date(asOf))")
        view.stale = !asOf.isEmpty && asOf < Dates.addDays(Dates.today, -4)
        // The prices' date stands under the value; a broker's note says where the figures come from.
        if page.text("source", "ibkr") == "ibkr" {
            var notes = [String(localized: "From Interactive Brokers account \(id). Each month's change in value is added to the register as Market change.")]
            if view.stale { notes.append(String(localized: "These prices are a few days old.")) }
            view.footnote = notes.joined(separator: " ")
        } else {
            view.footnote = Investments.sourceNote(page)
        }
        return view
    }
}

/// The page itself, pushed over the account's register (TransactionsList), as Android shows it in
/// the register's place. An iPhone reads it as one column of cards; with room for two (an iPad, a
/// large iPhone on its side) how it did stands on the leading side and what it holds on the
/// trailing side. A new period's figures roll into place, the Value and Gain tabs slide across, a
/// new order slides the holdings to their places, and their share bars grow in the first time.
struct HoldingsPageView: View {
    @Environment(BudgetStore.self) private var store
    var account: JSON

    @State private var page: JSON?
    @State private var model: HoldingsPageModel?
    @State private var problem: String?
    @State private var period = "1y"
    /// Which line the chart draws: "value" or "gain".
    @State private var chart = "value"
    @State private var sort = "value"
    @State private var showAll = false
    @State private var showDays = false
    @State private var kind: String?
    /// Whether the share bars have grown in: they do once, not each time the list comes back into view.
    @State private var grown = false

    /// From this much room the page stands in two columns.
    private static let twoColumns: CGFloat = 700

    var body: some View {
        GeometryReader { geometry in
            ScrollView {
                if let model {
                    if geometry.size.width >= Self.twoColumns {
                        HStack(alignment: .top, spacing: 16) {
                            LazyVStack(spacing: 12) { start(model, chartHeight: 260) }
                                .frame(maxWidth: .infinity)
                            // The holdings' side a little wider: a row's name, value and return sit side by side.
                            LazyVStack(spacing: 12) { end(model) }
                                .frame(maxWidth: .infinity)
                                .layoutPriority(1)
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                    } else {
                        LazyVStack(spacing: 12) {
                            start(model, chartHeight: 220, withMonths: false)
                            end(model, withMonths: true)
                        }
                        .padding(.horizontal, 16)
                        .padding(.vertical, 8)
                    }
                } else if let problem {
                    FormErrorView(message: problem).padding(16)
                } else {
                    HStack(spacing: 10) { ProgressView(); Text("Loading").foregroundStyle(Color.secondary) }
                        .frame(maxWidth: .infinity)
                        .padding(.top, 40)
                }
            }
            .background(Color.groupedBackground)
        }
        .navigationTitle(account.text("name"))
        .inlineTitle()
        .toolbar {
            ToolbarItem(placement: .principal) {
                VStack(spacing: 0) {
                    Text(account.text("name")).font(.headline).lineLimit(1)
                    Text("Performance and holdings").font(.caption).foregroundStyle(Color.secondary).lineLimit(1)
                }
                .accessibilityElement(children: .combine)
                .accessibilityAddTraits(.isHeader)
            }
        }
        // Read again when the budget changes under the page: a sync brings new days and prices.
        .task(id: store.version) { await load() }
    }

    private func load() async {
        do {
            let read = try await store.query("investmentPage", ["accountId": .string(account.text("id"))])
            guard !Task.isCancelled else { return }
            if read.isObject {
                page = read
                problem = nil
                rebuild()
            }
        } catch is CancellationError {
        } catch {
            problem = BudgetStore.readable(error)
        }
    }

    /// The figures for the period and order chosen, written out once rather than on every redraw.
    private func rebuild() {
        guard let page else { return }
        model = HoldingsPage.model(page, period: period, sort: sort, currency: store.currency)
    }

    private var periodChoice: Binding<String> {
        Binding(get: { period }, set: { next in
            period = next
            withAnimation(.snappy(duration: 0.35)) { rebuild() }
        })
    }

    private var sortChoice: Binding<String> {
        Binding(get: { sort }, set: { next in
            sort = next
            withAnimation(.spring(response: 0.45, dampingFraction: 0.88)) { rebuild() }
        })
    }

    /// How it did: the figures and chart, what the statement lacks, the period's ups and downs and,
    /// with room for two columns, each month's gain.
    @ViewBuilder private func start(_ view: HoldingsPageModel, chartHeight: CGFloat, withMonths: Bool = true) -> some View {
        performance(view, chartHeight: chartHeight)
        notices(view)
        if !view.periodStats.isEmpty {
            HoldingsStatGrid(stats: view.periodStats).padding(16).holdingsCard()
        }
        if withMonths { months(view) }
    }

    /// What it holds, then (on one column) each month's gain, and what it earned and was charged.
    @ViewBuilder private func end(_ view: HoldingsPageModel, withMonths: Bool = false) -> some View {
        // Nothing of what it holds for an account whose value alone is known.
        if !view.holdings.isEmpty || !view.others.isEmpty {
            Text("What it holds").font(.title3.weight(.semibold))
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 4)
                .padding(.top, 8)
                .accessibilityAddTraits(.isHeader)
            madeOf(view)
            holdings(view)
        }
        if withMonths { months(view) }
        income(view)
        Text(view.footnote).font(.footnote).foregroundStyle(Color.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 4)
    }

    /// The account's value and its gain over the period as two tabs over the chart: the chosen one
    /// sits in the card's colour over an accent line, joined to the chart it draws, and the two
    /// slide across when the other is chosen. Under them the chart, its key and the periods.
    private func performance(_ view: HoldingsPageModel, chartHeight: CGFloat) -> some View {
        let charted = view.valueChart != nil
        let gain = charted && chart == "gain"
        func pick(_ which: String) -> (() -> Void)? {
            charted ? { withAnimation(.spring(response: 0.35, dampingFraction: 0.9)) { chart = which } } : nil
        }
        return VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 0) {
                HoldingsMetric(label: String(localized: "Value"), value: view.total, tone: .none, chosen: charted && !gain, pick: pick("value"),
                               note: view.asOf, noteColor: view.stale ? .urWarning : .secondary)
                HoldingsMetric(label: view.gainLabel, value: view.gain, tone: view.gainTone, chosen: gain, pick: pick("gain"), pill: view.rate)
            }
            .background {
                // The other tab on the field colour.
                if charted {
                    GeometryReader { box in
                        Color.insetBackground.frame(width: box.size.width / 2)
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: gain ? .leading : .trailing)
                    }
                }
            }
            .overlay {
                // The chosen one over the accent line.
                if charted {
                    GeometryReader { box in
                        Color.urAccent.frame(width: box.size.width / 2, height: 2)
                            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: gain ? .bottomTrailing : .bottomLeading)
                    }
                    .allowsHitTesting(false)
                }
            }
            VStack(alignment: .leading, spacing: 10) {
                if let model = gain ? view.gainChart : view.valueChart {
                    ReportLinesChart(model: model, height: chartHeight, readout: false)
                        .id(chart)
                        .transition(.opacity)
                }
                // Each chart has a key of one line, so the card keeps its height as one gives way to the other.
                if gain, let gains = view.gainChart {
                    ReportLegendView(items: [ReportLegendItem(label: String(localized: "Gain"), color: gains.series.first?.color ?? ReportColors.positive, line: true)])
                } else if charted {
                    ReportLegendView(items: [ReportLegendItem(label: String(localized: "Value"), color: 0, line: true),
                                             ReportLegendItem(label: String(localized: "Value before gains"), color: ReportColors.other, line: true, dashed: true)])
                }
                Picker("Period", selection: periodChoice) {
                    ForEach(HoldingsPage.periods, id: \.self) { id in Text(HoldingsPage.periodName(id)).tag(id) }
                }
                .pickerStyle(.segmented)
                .sensoryFeedback(.selection, trigger: period)
            }
            .padding(12)
        }
        .holdingsCard()
    }

    @ViewBuilder private func notices(_ view: HoldingsPageModel) -> some View {
        if !view.lacking.isEmpty {
            let lines = [String(localized: "Your Flex Query leaves some sections out, so parts of this page are missing:")] + view.lacking.map { "• " + $0 }
            Notice(text: lines.joined(separator: "\n"), tone: "info")
        }
        if let estimated = view.estimated {
            Notice(text: estimated, tone: "info", action: showDays ? nil : String(localized: "The days counted as money moved"),
                   onAction: { withAnimation(.snappy) { showDays = true } })
            if showDays {
                VStack(alignment: .leading, spacing: 0) {
                    Text("The days counted as money moved").font(.footnote).foregroundStyle(Color.secondary)
                        .padding(.horizontal, 16).padding(.top, 12).padding(.bottom, 4)
                    ForEach(Array(view.estimatedDays.enumerated()), id: \.element.id) { index, row in
                        HStack {
                            Text(row.day)
                            Spacer(minLength: 8)
                            Text(row.amount).monospacedDigit().fontWeight(.semibold)
                        }
                        .padding(.horizontal, 16).padding(.vertical, 9)
                        .accessibilityElement(children: .combine)
                        if index < view.estimatedDays.count - 1 { Hairline().padding(.leading, 16) }
                    }
                }
                .holdingsCard()
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
    }

    /// What it is made of: a ring by kind beside its key, then a few facts.
    private func madeOf(_ view: HoldingsPageModel) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            if view.kinds.count > 1 {
                HStack(alignment: .center, spacing: 16) {
                    ReportDonut(slices: view.kinds, centerValue: view.kindShares[view.kinds[0].key] ?? "", centerLabel: view.kinds[0].label,
                                picked: kind, onPick: { kind = $0 }, openLabel: String(localized: "What the account is made of, by kind"), largest: 128)
                        .frame(width: 128)
                    VStack(alignment: .leading, spacing: 10) {
                        ForEach(view.kinds) { part in
                            HStack(spacing: 8) {
                                Circle().fill(ReportPalette.color(part.color)).frame(width: 10, height: 10)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(part.label).font(.subheadline).lineLimit(1)
                                    Text(part.valueText).font(.footnote).monospacedDigit().foregroundStyle(Color.secondary).lineLimit(1).minimumScaleFactor(0.8)
                                }
                                Spacer(minLength: 4)
                                Text(view.kindShares[part.key] ?? "").font(.subheadline.weight(.semibold)).monospacedDigit()
                            }
                            .accessibilityElement(children: .combine)
                        }
                    }
                }
                Hairline()
            }
            HoldingsStatGrid(stats: view.holdStats)
            if let currencies = view.currencies {
                Text(currencies).font(.footnote).monospacedDigit().foregroundStyle(Color.secondary)
            }
        }
        .padding(16)
        .holdingsCard()
    }

    /// Each holding, then cash and what's accrued, under how many there are and their order.
    private func holdings(_ view: HoldingsPageModel) -> some View {
        let shown = showAll ? view.holdings : Array(view.holdings.prefix(HoldingsPage.first))
        let more = view.holdings.count - HoldingsPage.first
        let grow = !grown
        return VStack(spacing: 0) {
            if view.count > 0 {
                HStack {
                    Text("\(view.count) holdings").font(.subheadline.weight(.semibold))
                    Spacer(minLength: 8)
                    if view.holdings.count > 1 { sortMenu }
                }
                .padding(.leading, 16)
                .padding(.trailing, 8)
                .padding(.top, 6)
                .frame(minHeight: 44)
            }
            ForEach(Array(shown.enumerated()), id: \.element.id) { index, row in
                HoldingsLineRow(row: row, grow: grow, divider: index < shown.count - 1 || more > 0 || !view.others.isEmpty)
            }
            if more > 0 {
                Button(showAll ? String(localized: "Show fewer") : String(localized: "Show \(more) more holdings", locale: .app)) {
                    withAnimation(.snappy) { showAll.toggle() }
                }
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(Color.urAccent)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .background(Color.cardBackground)
                if !view.others.isEmpty { Hairline() }
            }
            ForEach(Array(view.others.enumerated()), id: \.element.id) { index, row in
                HoldingsLineRow(row: row, grow: grow, divider: index < view.others.count - 1)
            }
        }
        .holdingsCard()
        // Rows made after this (more of them, a later visit) are drawn at their length at once.
        .onAppear { if !grown { grown = true } }
    }

    /// The order the holdings are listed in, as a small menu by their count.
    private var sortMenu: some View {
        let options = [("value", String(localized: "Value")), ("gain", String(localized: "Gain")), ("rate", String(localized: "Return")), ("name", String(localized: "Holding"))]
        let name = options.first { $0.0 == sort }?.1 ?? ""
        return Menu {
            Picker("Sort", selection: sortChoice) {
                ForEach(options.indices, id: \.self) { index in Text(options[index].1).tag(options[index].0) }
            }
        } label: {
            Label(name, systemImage: "arrow.up.arrow.down")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(Color.urAccent)
                .padding(.horizontal, 8)
                .frame(minHeight: 44)
                .contentShape(Rectangle())
        }
        .accessibilityLabel(Text("Sort"))
        .accessibilityValue(name)
    }

    private func months(_ view: HoldingsPageModel) -> some View {
        Group {
            if let months = view.months {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Month by month").font(.subheadline.weight(.semibold)).padding(.horizontal, 4).accessibilityAddTraits(.isHeader)
                    ReportColumnChart(model: months, height: 180, readout: false)
                }
                .padding(EdgeInsets(top: 14, leading: 12, bottom: 10, trailing: 12))
                .holdingsCard()
            }
        }
    }

    /// What it earned and was charged over the last twelve months.
    @ViewBuilder private func income(_ view: HoldingsPageModel) -> some View {
        if !view.incomeStats.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
                VStack(alignment: .leading, spacing: 12) {
                    HStack {
                        Text("Income and costs").font(.subheadline.weight(.semibold)).accessibilityAddTraits(.isHeader)
                        Spacer(minLength: 8)
                        if let span = view.incomeSpan { Text(span).font(.footnote).foregroundStyle(Color.secondary) }
                    }
                    HoldingsStatGrid(stats: view.incomeStats)
                }
                .padding(EdgeInsets(top: 14, leading: 16, bottom: 8, trailing: 16))
                ForEach(view.income) { line in
                    Hairline().padding(.leading, 16)
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(line.label)
                            if let note = line.note { Text(note).font(.footnote).foregroundStyle(Color.secondary) }
                        }
                        Spacer(minLength: 8)
                        Text(line.amount).monospacedDigit().fontWeight(.semibold).foregroundStyle(HoldingsPage.color(line.tone))
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .accessibilityElement(children: .combine)
                }
                Hairline().padding(.leading, 16)
                Text("All of these are already in the gain above. Dividends, interest, tax and fees arrive as cash; a trade's commission is part of what the trade cost.")
                    .font(.footnote).foregroundStyle(Color.secondary)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
            }
            .holdingsCard()
        }
    }
}

private extension View {
    /// The page's cards: the register's own, rounded on the grouped background.
    func holdingsCard() -> some View {
        frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.cardBackground)
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
}

/// One of the two figures over the chart; with a chart under them, tapping one chooses what it
/// draws. A new period's figures roll into place, digit by digit.
private struct HoldingsMetric: View {
    var label: String
    var value: String
    var tone: ReportTone
    var chosen: Bool
    var pick: (() -> Void)?
    var note: String?
    var noteColor: Color = .secondary
    var pill: String?

    var body: some View {
        let figures = VStack(alignment: .leading, spacing: 3) {
            Text(label).font(.footnote.weight(chosen ? .semibold : .regular)).foregroundStyle(chosen ? Color.urAccent : Color.secondary)
                .lineLimit(1).minimumScaleFactor(0.8)
                .contentTransition(.opacity)
            Text(value).figure(22, weight: .bold).foregroundStyle(HoldingsPage.color(tone)).lineLimit(1).minimumScaleFactor(0.6)
                .contentTransition(.numericText())
            if let note { Text(note).font(.footnote).foregroundStyle(noteColor).lineLimit(1).minimumScaleFactor(0.8) }
            if let pill { HoldingsRatePill(rate: pill, tone: tone).contentTransition(.numericText()) }
        }
        .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 12))
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        if let pick {
            Button(action: pick) { figures }
                .buttonStyle(.plain)
                .accessibilityAddTraits(chosen ? [.isSelected] : [])
        } else {
            figures
        }
    }
}

/// A return as a small tinted pill: green up, red down.
private struct HoldingsRatePill: View {
    var rate: String
    var tone: ReportTone

    var body: some View {
        Text(rate).font(.caption.weight(.bold)).monospacedDigit()
            .foregroundStyle(tone == .none ? Color.secondary : HoldingsPage.color(tone))
            .padding(.horizontal, 7)
            .padding(.vertical, 1)
            .background(tone == .none ? Color.urZero : HoldingsPage.color(tone).opacity(0.15), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
    }
}

/// Figures two to a row: each one's name, its amount, and a line of detail under it.
private struct HoldingsStatGrid: View {
    var stats: [HoldingsStat]

    var body: some View {
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 14, alignment: .topLeading), GridItem(.flexible(), alignment: .topLeading)],
                  alignment: .leading, spacing: 16) {
            ForEach(stats) { stat in
                VStack(alignment: .leading, spacing: 2) {
                    Text(stat.label).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2)
                    Text(stat.value).figure(17, weight: .semibold).foregroundStyle(HoldingsPage.color(stat.tone)).lineLimit(1).minimumScaleFactor(0.7)
                        .contentTransition(.numericText())
                    if let note = stat.note { Text(note).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2) }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityElement(children: .combine)
            }
        }
    }
}

/// One thing the account holds: a tile in its kind's colour with its symbol, its name over its
/// own line, its value over its gain and return, and a bar of its share of the account. In the
/// card's colour, so a row sliding to its new place covers the ones it passes.
private struct HoldingsLineRow: View {
    var row: HoldingsLine
    var divider: Bool
    /// How much of the bar is drawn: it grows to its length the first time the list shows.
    @State private var drawn: Double

    init(row: HoldingsLine, grow: Bool, divider: Bool) {
        self.row = row
        self.divider = divider
        _drawn = State(initialValue: grow ? 0 : row.bar)
    }

    var body: some View {
        let tint = ReportPalette.color(row.color)
        VStack(spacing: 0) {
            HStack(alignment: .top, spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 12, style: .continuous).fill(tint.opacity(0.16))
                    if let mark = row.mark {
                        Text(mark).font(.caption.weight(.bold)).foregroundStyle(tint).lineLimit(1).minimumScaleFactor(0.6).padding(.horizontal, 4)
                    } else {
                        Image(systemName: "clock").font(.body).foregroundStyle(tint)
                    }
                }
                .frame(width: 42, height: 42)
                .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 8) {
                    HStack(alignment: .top, spacing: 10) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(row.name).fontWeight(.medium).lineLimit(2)
                            if let line = row.line { Text(line).font(.footnote).monospacedDigit().foregroundStyle(Color.secondary).lineLimit(2) }
                        }
                        Spacer(minLength: 8)
                        VStack(alignment: .trailing, spacing: 4) {
                            Text(row.value).monospacedDigit().fontWeight(.semibold).lineLimit(1)
                            if row.gain != nil || row.rate != nil {
                                HStack(spacing: 6) {
                                    if let gain = row.gain {
                                        Text(gain).font(.footnote).monospacedDigit().lineLimit(1)
                                            .foregroundStyle(row.sign < 0 ? Color.urNegative : row.sign > 0 ? Color.urPositive : Color.secondary)
                                    }
                                    if let rate = row.rate { HoldingsRatePill(rate: rate, tone: HoldingsPage.tone(row.sign)) }
                                }
                            }
                        }
                        .layoutPriority(1)
                    }
                    if let share = row.share {
                        HStack(spacing: 10) {
                            GeometryReader { geometry in
                                ZStack(alignment: .leading) {
                                    Capsule().fill(Color.insetBackground)
                                    Capsule().fill(tint).frame(width: max(drawn > 0 ? 2 : 0, geometry.size.width * drawn))
                                }
                            }
                            .frame(height: 4)
                            Text(share).font(.footnote).monospacedDigit().foregroundStyle(Color.secondary).frame(minWidth: 36, alignment: .trailing)
                        }
                        .accessibilityHidden(true)
                    }
                }
            }
            .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 16))
            if divider { Hairline().padding(.leading, 68) }
        }
        .background(Color.cardBackground)
        .accessibilityElement(children: .combine)
        .accessibilityValue(row.share.map { String(localized: "\($0) of the account") } ?? "")
        .onAppear {
            guard drawn != row.bar else { return }
            withAnimation(.easeOut(duration: 0.75).delay(0.12)) { drawn = row.bar }
        }
        .onChange(of: row.bar) { _, bar in withAnimation(.easeOut(duration: 0.35)) { drawn = bar } }
    }
}
