import Foundation

/// Budget dates are ISO text ("2026-10-21", months "2026-10"), as the portable schema stores them.
/// Calendar arithmetic uses UTC so a day never shifts with daylight saving.
enum Dates {
    private static let utc: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        return calendar
    }()

    private static func formatter(_ format: String, utc: Bool = true) -> DateFormatter {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.timeZone = utc ? TimeZone(identifier: "UTC") : .current
        formatter.dateFormat = format
        return formatter
    }

    /// Month and weekday names for people: English as written here, or in Arabic the names and
    /// order of the person's Arabic region, on the Gregorian calendar with Western digits.
    private static func names(_ english: String, arabic template: String) -> DateFormatter {
        guard AppLanguage.isRightToLeft else { return formatter(english) }
        let arabic = DateFormatter()
        arabic.locale = .app
        arabic.calendar = Calendar(identifier: .gregorian)
        arabic.timeZone = TimeZone(identifier: "UTC")
        arabic.setLocalizedDateFormatFromTemplate(template)
        return arabic
    }

    private static let isoDay = formatter("yyyy-MM-dd")
    private static let localDay = formatter("yyyy-MM-dd", utc: false)
    private static let monthYear = names("MMMM yyyy", arabic: "MMMMy")
    private static let shortMonthYear = names("MMM yyyy", arabic: "MMMy")
    private static let shortMonth = names("MMM", arabic: "MMM")
    private static let monthName = names("MMMM", arabic: "MMMM")
    private static let dayMonthFirst = names("EEEE, MMMM d, yyyy", arabic: "EEEEdMMMMy")
    private static let dayFirst = names("EEEE, d MMMM yyyy", arabic: "EEEEdMMMMy")
    private static let dayShort = names("d MMM yyyy", arabic: "dMMMy")
    private static let dayMonth = names("d MMM", arabic: "dMMM")

    /// Today on this device, as the budget's day.
    static var today: String { localDay.string(from: Date()) }
    static var currentMonth: String { String(today.prefix(7)) }

    static func day(_ iso: String) -> Date? {
        guard iso.count == 10, let date = isoDay.date(from: iso), isoDay.string(from: date) == iso else { return nil }
        return date
    }

    static func iso(_ date: Date) -> String { isoDay.string(from: date) }

    /// A date picker works in local time; the budget day is the local calendar day it shows.
    static func pickerDate(_ iso: String) -> Date {
        localDay.date(from: iso) ?? Date()
    }

    static func fromPicker(_ date: Date) -> String { localDay.string(from: date) }

    static func isMonth(_ month: String) -> Bool {
        month.count == 7 && day(month + "-01") != nil
    }

    static func addMonths(_ month: String, _ count: Int) -> String {
        guard let first = day(month + "-01"), let moved = utc.date(byAdding: .month, value: count, to: first) else { return month }
        return String(iso(moved).prefix(7))
    }

    static func addDays(_ iso: String, _ count: Int) -> String {
        guard let date = day(iso), let moved = utc.date(byAdding: .day, value: count, to: date) else { return iso }
        return self.iso(moved)
    }

    /// The first and last day of a month.
    static func monthBounds(_ month: String) -> (String, String) {
        guard let first = day(month + "-01"), let next = utc.date(byAdding: .month, value: 1, to: first),
              let last = utc.date(byAdding: .day, value: -1, to: next) else { return ("", "") }
        return (iso(first), iso(last))
    }

    static func year(_ month: String) -> Int { Int(month.prefix(4)) ?? 2026 }
    static func monthNumber(_ month: String) -> Int { Int(month.dropFirst(5).prefix(2)) ?? 1 }
    static func month(year: Int, number: Int) -> String { String(format: "%04d-%02d", year, number) }

    /// "October 2026".
    static func monthLabel(_ month: String) -> String {
        day(month + "-01").map(monthYear.string(from:)) ?? month
    }

    /// "Oct 2026", as the budget's month header shows it.
    static func shortMonthLabel(_ month: String) -> String {
        day(month + "-01").map(shortMonthYear.string(from:)) ?? month
    }

    /// "Oct".
    static func shortMonthName(_ month: String) -> String {
        day(month + "-01").map(shortMonth.string(from:)) ?? month
    }

    /// "October".
    static func monthName(_ month: String) -> String {
        day(month + "-01").map(monthName.string(from:)) ?? month
    }

    /// "Wednesday, October 21, 2026", or "Wednesday, 21 October 2026" when dates go day first.
    static func dayLabel(_ iso: String, monthFirst: Bool) -> String {
        guard let date = day(iso) else { return iso }
        return (monthFirst ? dayMonthFirst : dayFirst).string(from: date)
    }

    /// "21 Oct 2026".
    static func shortDay(_ iso: String) -> String {
        day(String(iso.prefix(10))).map(dayShort.string(from:)) ?? iso
    }

    /// "21 Oct", for moves and changes.
    static func dayAndMonth(_ iso: String?) -> String? {
        guard let iso, let date = day(String(iso.prefix(10))) else { return nil }
        return dayMonth.string(from: date)
    }

    /// "1st", "22nd"; Arabic writes the day's number alone ("يوم 22").
    static func ordinal(_ n: Int) -> String {
        if AppLanguage.isRightToLeft { return String(n) }
        let suffix: String
        if (11...13).contains(n % 100) { suffix = "th" } else {
            switch n % 10 { case 1: suffix = "st"; case 2: suffix = "nd"; case 3: suffix = "rd"; default: suffix = "th" }
        }
        return "\(n)\(suffix)"
    }

    /// "2026-10-01" in a sentence: "October 1st" when dates go month first, else "1 October".
    static func deadline(_ iso: String, monthFirst: Bool) -> String {
        guard let date = day(iso) else { return iso }
        let name = monthName.string(from: date)
        let dayNumber = utc.component(.day, from: date)
        // Arabic puts the day first whatever the budget's order: "1 أكتوبر".
        return monthFirst && !AppLanguage.isRightToLeft ? "\(name) \(ordinal(dayNumber))" : "\(dayNumber) \(name)"
    }
}
