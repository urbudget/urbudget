import Foundation

/// Exact text conversion only; budget arithmetic belongs to Rust. The portable schema uses cents.
/// The budget's number and date preferences apply everywhere, so they're held here once.
enum Money {
    struct Format: Equatable, Sendable {
        var decimal = "."
        var group = ","
        var symbol = "after"
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var settings = Format()
    nonisolated(unsafe) private static var dateFormat = "iso"
    nonisolated(unsafe) private static var symbols: [String: String] = [:]
    private static let pinnedSymbols = ["SAR": "SAR"]

    private static let dates: [String: DateFormatter] = {
        var formatters: [String: DateFormatter] = [:]
        for (id, pattern) in ["dmy": "dd/MM/yyyy", "mdy": "MM/dd/yyyy", "dmmmy": "d MMM yyyy", "iso": "yyyy-MM-dd"] {
            let formatter = DateFormatter()
            formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.timeZone = TimeZone(identifier: "UTC")
            formatter.dateFormat = pattern
            formatters[id] = formatter
        }
        return formatters
    }()

    static var format: Format { lock.withLock { settings } }

    /// Reads the budget's number format, date format and currency symbol placement.
    static func configure(budget: JSON) {
        let saved = budget["numberFormat"]
        let currency = budget.text("currency", "SAR")
        let symbol = currencySymbol(currency)
        let decimal = saved.text("decimal")
        let group = saved.text("group", ",")
        let placement = saved.text("symbol")
        let next = Format(
            decimal: [".", ","].contains(decimal) ? decimal : ".",
            group: [",", ".", " ", "'", ""].contains(group) && saved.has("group") ? group : ",",
            symbol: ["before", "after", "none"].contains(placement) ? placement : (symbol == currency ? "after" : "before"))
        lock.withLock {
            settings = next
            dateFormat = budget.text("dateFormat", "iso")
        }
    }

    /// The currency's English symbol ("$", "€", "SAR"), as the desktop shows it.
    static func currencySymbol(_ currency: String) -> String {
        if let known = lock.withLock({ symbols[currency] }) { return known }
        // Newer systems give the riyal its new sign (U+20C1), which Android, desktop and many fonts
        // don't show yet. The code keeps a synced budget's amounts identical on every device.
        if let pinned = pinnedSymbols[currency] { return pinned }
        let formatter = NumberFormatter()
        formatter.locale = Locale(identifier: "en")
        formatter.numberStyle = .currency
        formatter.currencyCode = currency
        let symbol = formatter.currencySymbol.flatMap { $0.isEmpty ? nil : $0 } ?? currency
        lock.withLock { symbols[currency] = symbol }
        return symbol
    }

    /// Cents as text in the budget's format: no floating point, and no abs(Int64.min). In Arabic the
    /// amount is kept left to right as a whole, so its sign and symbol stay where the budget puts them.
    static func format(_ amount: Int64, currency: String) -> String {
        AppLanguage.leftToRight(plain(amount, currency: currency))
    }

    /// `format` without the direction marks, for text that's built on further (a sign added in front).
    static func plain(_ amount: Int64, currency: String) -> String {
        let style = format
        var digits = String(amount.magnitude)
        while digits.count < 3 { digits = "0" + digits }
        let whole = Array(digits.dropLast(2))
        var grouped = ""
        for (index, digit) in whole.enumerated() {
            if index > 0 && (whole.count - index) % 3 == 0 { grouped += style.group }
            grouped.append(digit)
        }
        let number = grouped + style.decimal + digits.suffix(2)
        let sign = amount < 0 ? "-" : ""
        let symbol = currencySymbol(currency)
        switch style.symbol {
        case "before": return sign + symbol + (symbol == currency ? " " : "") + number
        case "none": return sign + number
        default: return sign + number + " " + symbol
        }
    }

    /// Text typed in a field ("1,200.50", "-12", "7") in cents.
    static func parse(_ text: String) throws -> Int64 {
        let style = format
        var normalized = text.trimmingCharacters(in: .whitespaces)
            .replacingOccurrences(of: "\u{066B}", with: style.decimal)
            .replacingOccurrences(of: "\u{066C}", with: style.group)
        // Digits from other scripts (Arabic-Indic and others) count as their values.
        normalized = String(normalized.map { character -> Character in
            if let value = character.wholeNumberValue, character.isNumber, value < 10, !character.isASCII {
                return Character(String(value))
            }
            return character
        })
        let separators = style.group.isEmpty || style.group == style.decimal ? "" : NSRegularExpression.escapedPattern(for: style.group)
        let grouped = separators.isEmpty ? "" : "|[0-9]{1,3}(?:\(separators)[0-9]{3})+"
        let pattern = "^[+-]?(?:[0-9]+\(grouped))(?:\(NSRegularExpression.escapedPattern(for: style.decimal))[0-9]{1,2})?$"
        guard normalized.range(of: pattern, options: .regularExpression) != nil else {
            throw AppError(String(localized: "Enter an amount with up to two decimal places. Decimal separator: \(style.decimal)"))
        }
        var plain = separators.isEmpty ? normalized : normalized.replacingOccurrences(of: style.group, with: "")
        plain = plain.replacingOccurrences(of: style.decimal, with: ".")
        let negative = plain.hasPrefix("-")
        plain = plain.trimmingCharacters(in: CharacterSet(charactersIn: "+-"))
        let parts = plain.split(separator: ".", omittingEmptySubsequences: false)
        let fraction = parts.count > 1 ? String(parts[1]).padding(toLength: 2, withPad: "0", startingAt: 0) : "00"
        guard let whole = Int64(parts[0]), whole <= 10_000_000_000, let cents = Int64(fraction) else {
            throw AppError(String(localized: "The amount is too large."))
        }
        let value = whole * 100 + cents
        return negative ? -value : value
    }

    /// Cents as a field's starting text, with the budget's decimal mark and no grouping.
    static func editable(_ amount: Int64) -> String {
        let digits = String(amount.magnitude)
        let padded = String(repeating: "0", count: max(0, 3 - digits.count)) + digits
        return (amount < 0 ? "-" : "") + String(padded.dropLast(2)) + format.decimal + String(padded.suffix(2))
    }

    /// The budget's decimal mark, for the keypad's display.
    static func decimalMark() -> String { format.decimal }

    /// An ISO date in the budget's chosen format.
    static func date(_ iso: String) -> String {
        let id = lock.withLock { dateFormat }
        guard let date = Dates.day(iso), let formatter = dates[id] ?? dates["iso"] else { return iso }
        return formatter.string(from: date)
    }

    static var monthFirst: Bool { lock.withLock { dateFormat } == "mdy" }
}

/// Money as the budget writes it.
func money(_ amount: Int64, _ currency: String) -> String { Money.format(amount, currency: currency) }
