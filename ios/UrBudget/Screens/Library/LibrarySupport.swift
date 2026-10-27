import SwiftUI
import UniformTypeIdentifiers

// Pure helpers for the budget library and Settings & data, kept apart so tests can reach them.

// MARK: Number format

/// The budget's number format as Settings edits it, resolved the way `Money.configure` reads it.
struct LibraryNumberFormat: Equatable, Sendable {
    var decimal = "."
    var group = ","
    var symbol = "after"

    static let decimalChoices: [(id: String, label: String)] = [(".", String(localized: "Dot · 1234.56")), (",", String(localized: "Comma · 1234,56"))]
    static let groupChoices: [(id: String, label: String)] = [
        (",", String(localized: "Comma")), (".", String(localized: "Dot")), (" ", String(localized: "Space")), ("'", String(localized: "Apostrophe")), ("", String(localized: "None")),
    ]
    static let placementChoices: [(id: String, label: String)] = [
        ("before", String(localized: "Before amount")), ("after", String(localized: "After amount")), ("none", String(localized: "Hide symbol")),
    ]

    init(decimal: String = ".", group: String = ",", symbol: String = "after") {
        self.decimal = decimal
        self.group = group
        self.symbol = symbol
    }

    init(budget: JSON) {
        let saved = budget["numberFormat"]
        let decimal = saved.text("decimal")
        let group = saved.text("group", ",")
        let placement = saved.text("symbol")
        self.decimal = [".", ","].contains(decimal) ? decimal : "."
        self.group = [",", ".", " ", "'", ""].contains(group) && saved.has("group") ? group : ","
        self.symbol = ["before", "after", "none"].contains(placement) ? placement : Self.defaultSymbol(budget.text("currency", "SAR"))
    }

    /// Where a currency's symbol goes unless the budget says otherwise: before a sign ($1.00), after a code (1.00 SAR).
    static func defaultSymbol(_ currency: String) -> String {
        Money.currencySymbol(currency) == currency ? "after" : "before"
    }

    /// Thousands separators other than the decimal mark.
    var groups: [(id: String, label: String)] { Self.groupChoices.filter { $0.id != decimal } }

    /// A new decimal mark. A thousands separator that would match it switches to the other mark.
    func with(decimal value: String) -> Self {
        var next = self
        next.decimal = value
        if next.group == value { next.group = value == "." ? "," : "." }
        return next
    }

    /// Cents as text in this format, exactly as `Money.format` writes them (left to right as a whole
    /// in Arabic), for a live example.
    func format(_ amount: Int64, currency: String) -> String {
        AppLanguage.leftToRight(plain(amount, currency: currency))
    }

    private func plain(_ amount: Int64, currency: String) -> String {
        var digits = String(amount.magnitude)
        while digits.count < 3 { digits = "0" + digits }
        let whole = Array(digits.dropLast(2))
        var grouped = ""
        for (index, digit) in whole.enumerated() {
            if index > 0 && (whole.count - index) % 3 == 0 { grouped += group }
            grouped.append(digit)
        }
        let number = grouped + decimal + digits.suffix(2)
        let sign = amount < 0 ? "-" : ""
        let mark = Money.currencySymbol(currency)
        switch symbol {
        case "before": return sign + mark + (mark == currency ? " " : "") + number
        case "none": return sign + number
        default: return sign + number + " " + mark
        }
    }

    /// Writes this format into the budget, keeping any numberFormat fields this app doesn't know.
    func write(into budget: inout JSON) {
        var saved: JSON = budget["numberFormat"].isObject ? budget["numberFormat"] : [:]
        saved["decimal"] = .string(decimal)
        saved["group"] = .string(group)
        saved["symbol"] = .string(symbol)
        budget["numberFormat"] = saved
    }
}

// MARK: Budget settings

enum LibrarySettings {
    /// The date formats the budget can use, as the desktop names them.
    static let dateFormats = ["iso", "dmy", "mdy", "dmmmy"]

    /// How many automatic and manual backups each budget keeps (0 keeps all), as `set_backup_retention` accepts.
    static let retentionChoices: [(keep: Int, label: String)] = [
        (0, String(localized: "All of them")), (5, String(localized: "The newest 5")), (10, String(localized: "The newest 10")), (30, String(localized: "The newest 30")),
    ]

    private static let datePatterns: [String: DateFormatter] = {
        var formatters: [String: DateFormatter] = [:]
        for (id, pattern) in ["dmy": "dd/MM/yyyy", "mdy": "MM/dd/yyyy", "dmmmy": "d MMM yyyy", "iso": "yyyy-MM-dd"] {
            let formatter = DateFormatter()
            formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.calendar = Calendar(identifier: .gregorian)
            formatter.timeZone = TimeZone(identifier: "UTC")
            formatter.dateFormat = pattern
            formatters[id] = formatter
        }
        return formatters
    }()

    /// An ISO day ("2026-10-21") in one of the budget's date formats ("21/10/2026"), whatever format is open.
    static func dateExample(_ format: String, day iso: String) -> String {
        guard let date = Dates.day(iso), let formatter = datePatterns[format] else { return iso }
        return formatter.string(from: date)
    }

    /// A budget name the engine accepts: not blank and at most 200 bytes.
    static func validName(_ name: String) -> Bool {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return !trimmed.isEmpty && trimmed.utf8.count <= 200
    }

    /// Changes the currency label (amounts are never converted). A symbol saved where the old
    /// currency puts it moves to where the new one does, as the desktop's format fields do.
    static func change(currency: String, in budget: inout JSON) {
        let old = budget.text("currency", "SAR")
        var saved = budget["numberFormat"]
        if saved.isObject, saved.text("symbol") == LibraryNumberFormat.defaultSymbol(old) {
            saved["symbol"] = .string(LibraryNumberFormat.defaultSymbol(currency))
            budget["numberFormat"] = saved
        }
        budget["currency"] = .string(currency)
    }

    /// Saves the flag colours' names (up to 40 characters each); an empty name goes back to the colour.
    /// Without any names, `flagNames` is removed, as Android saves it.
    static func set(flagNames names: [String: String], in budget: inout JSON) {
        var saved: JSON = budget["flagNames"].isObject ? budget["flagNames"] : [:]
        for color in flagColors {
            let name = String((names[color] ?? "").trimmingCharacters(in: .whitespacesAndNewlines).prefix(40))
            if name.isEmpty { saved.remove(color) } else { saved[color] = .string(name) }
        }
        if saved.object.isEmpty { budget.remove("flagNames") } else { budget["flagNames"] = saved }
    }
}

// MARK: Dates

enum LibraryDates {
    private static func utc(_ pattern: String) -> DateFormatter {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = TimeZone(identifier: "UTC")
        formatter.dateFormat = pattern
        formatter.isLenient = false
        return formatter
    }

    private static let seconds = utc("yyyy-MM-dd'T'HH:mm:ss")
    private static let stamp = utc("yyyyMMdd-HHmmss")

    /// An RFC 3339 time as the library records it ("2026-10-21T14:30:05.123456789+00:00").
    static func instant(_ text: String) -> Date? {
        guard text.count >= 19, let base = seconds.date(from: String(text.prefix(19))) else { return nil }
        var rest = Substring(text.dropFirst(19))
        if rest.first == "." { rest = rest.dropFirst().drop { $0.isASCII && $0.isNumber } }
        if rest.isEmpty || rest == "Z" || rest == "z" { return base }
        guard let sign = rest.first, sign == "+" || sign == "-", rest.count == 6 else { return nil }
        let parts = rest.dropFirst().split(separator: ":")
        guard parts.count == 2, let hours = Int(parts[0]), let minutes = Int(parts[1]), hours < 24, minutes < 60 else { return nil }
        let offset = (hours * 3600 + minutes * 60) * (sign == "-" ? -1 : 1)
        return base.addingTimeInterval(TimeInterval(-offset))
    }

    /// The UTC time in a backup's name: "budget-20261021-143005-123456789.sqlite3".
    static func backupDate(_ name: String) -> Date? {
        guard name.hasPrefix("budget-") else { return nil }
        let text = String(name.dropFirst(7).prefix(15))
        guard let date = stamp.date(from: text), stamp.string(from: date) == text else { return nil }
        return date
    }

    /// A moment as this budget writes dates, with the time: "21/10/2026, 14:30".
    static func label(_ date: Date) -> String {
        let day = Money.date(Dates.fromPicker(date))
        let time = date.formatted(Date.FormatStyle(date: .omitted, time: .shortened).locale(.app))
        return String(localized: "\(day), \(time)", comment: "A date and a time, as in “21/10/2026, 14:30”.")
    }

    /// A file's size ("2.1 MB"), with Western digits in Arabic as budgets write numbers.
    static func size(_ bytes: Int64) -> String {
        guard AppLanguage.isRightToLeft else { return ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file) }
        return bytes.formatted(.byteCount(style: .file).locale(.app))
    }
}

// MARK: Backups

/// One file in the open budget's backups folder, as `list_backups` lists it.
struct LibraryBackup: Identifiable, Equatable, Sendable {
    var name: String
    var path: String
    var bytes: Int64

    init(_ value: JSON) {
        name = value.text("name")
        path = value.text("path")
        bytes = value.long("bytes")
    }

    var id: String { path }
    var date: Date? { LibraryDates.backupDate(name) }
    /// Back up now writes portable JSON; restores, imports and sync leave database safety copies.
    var portable: Bool { name.hasSuffix(".json") }
    var title: String { date.map(LibraryDates.label) ?? name }
    var detail: String {
        let size = LibraryDates.size(bytes)
        return portable ? String(localized: "Portable backup · \(size)") : String(localized: "Safety copy · \(size)")
    }

    /// How many backups `prune_backups` would remove to keep the newest `keep` (0 keeps all).
    static func excess(_ count: Int, keep: Int) -> Int { keep > 0 ? max(0, count - keep) : 0 }
}

/// A file waiting for the exporter: what it holds, its type and name, and the toast once saved.
struct LibraryExport {
    var file: TextFile
    var type: UTType
    var name: String
    var done: String
}

// MARK: Currencies

struct LibraryCurrency: Sendable {
    var code: String
    var name: String
    var symbol: String
}

enum LibraryCurrencies {
    private static let english = Locale(identifier: "en")

    /// The ISO 4217 currencies in common use, A to Z, as the engine accepts them (three capitals).
    static let all: [LibraryCurrency] = Locale.commonISOCurrencyCodes
        .filter { Self.valid($0) }
        .sorted()
        .map { Self.entry($0) }

    static func valid(_ code: String) -> Bool {
        code.count == 3 && code.allSatisfy { $0.isASCII && $0.isUppercase }
    }

    static func entry(_ code: String) -> LibraryCurrency {
        let symbol = Money.currencySymbol(code)
        // Names in the app's language (English as before, Arabic in Arabic); the code and symbol stay as budgets write them.
        let names = AppLanguage.isRightToLeft ? Locale.app : english
        return LibraryCurrency(code: code, name: names.localizedString(forCurrencyCode: code) ?? code, symbol: symbol == code ? "" : symbol)
    }

    /// Every currency, with `current` added when the list doesn't have it (an older code from a backup).
    static func list(including current: String) -> [LibraryCurrency] {
        all.contains { $0.code == current } || !valid(current) ? all : ([entry(current)] + all)
    }
}

/// The budget's currency as a form row that opens a searchable list of every currency.
struct LibraryCurrencyPicker: View {
    var title = String(localized: "Currency")
    @Binding var selection: String

    var body: some View {
        NavigationLink {
            LibraryCurrencyList(selection: $selection)
        } label: {
            LabeledContent(title) { Text(selection) }
        }
    }
}

private struct LibraryCurrencyList: View {
    @Binding var selection: String
    @Environment(\.dismiss) private var dismiss
    @State private var search = ""

    var body: some View {
        let query = search.trimmingCharacters(in: .whitespaces)
        let all = LibraryCurrencies.list(including: selection)
        let matches = query.isEmpty ? all : all.filter {
            $0.code.localizedCaseInsensitiveContains(query) || $0.name.localizedCaseInsensitiveContains(query) || (!$0.symbol.isEmpty && $0.symbol == query)
        }
        List {
            if query.isEmpty {
                Section("Suggested") {
                    ForEach(currencyChoices(selection).filter(LibraryCurrencies.valid), id: \.self) { row(LibraryCurrencies.entry($0)) }
                }
            }
            Section(query.isEmpty ? String(localized: "All currencies") : String(localized: "Results")) {
                ForEach(matches, id: \.code) { row($0) }
            }
        }
        .groupedList()
        .overlay { if matches.isEmpty { ContentUnavailableView.search(text: query) } }
        .searchable(text: $search, prompt: "Code or name")
        .navigationTitle("Currency")
        .inlineTitle()
    }

    private func row(_ currency: LibraryCurrency) -> some View {
        let chosen = currency.code == selection
        return Button {
            selection = currency.code
            dismiss()
        } label: {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(currency.code).foregroundStyle(Color.primary)
                    Text(currency.name).font(.footnote).foregroundStyle(Color.secondary)
                }
                Spacer(minLength: 8)
                if !currency.symbol.isEmpty { Text(currency.symbol).foregroundStyle(Color.secondary) }
                Image(systemName: "checkmark").fontWeight(.semibold).foregroundStyle(Color.urAccent).opacity(chosen ? 1 : 0)
                    .accessibilityHidden(true)
            }
            .contentShape(Rectangle())
        }
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}
