import Foundation
import SwiftUI

// Presentation-side helpers for the planning screens, ported from Android's PlanningMath.kt and
// PayeeIndex.kt. Projections, payments and rules themselves run in the shared engine (loans.rs,
// schedules.rs, workflows.rs); these only read the budget to prepare what the screens show.

// MARK: Schedules

/// How often a schedule repeats, in Android's words (MoreScreen.kt's frequencies); "never" is once.
enum ScheduleFrequency {
    static let all: [(id: String, label: String)] = [
        ("never", String(localized: "Once")), ("daily", String(localized: "Daily")), ("weekly", String(localized: "Weekly")),
        ("everyOtherWeek", String(localized: "Every other week")), ("every4Weeks", String(localized: "Every 4 weeks")),
        ("twiceAMonth", String(localized: "Twice a month")), ("monthly", String(localized: "Monthly")),
        ("everyOtherMonth", String(localized: "Every other month")), ("every3Months", String(localized: "Every 3 months")),
        ("every4Months", String(localized: "Every 4 months")), ("twiceAYear", String(localized: "Twice a year")),
        ("yearly", String(localized: "Yearly")), ("everyOtherYear", String(localized: "Every other year")),
    ]

    static func label(_ id: String) -> String {
        if id.isEmpty { return String(localized: "Once") }
        return all.first { $0.id == id }?.label ?? id
    }

    static func repeats(_ id: String) -> Bool { !id.isEmpty && id != "never" }
}

/// The Scheduled page's figures, as the desktop's schedules list counts them: each schedule due
/// in the next 30 days once, by its next date.
struct ScheduleSummary: Equatable {
    var active: Int
    var paused: Int
    var dueSoon: Int
    var goingOut: Int64
    var comingIn: Int64
}

enum ScheduleMath {
    /// Schedules still to come, soonest first with paused ones after (Register.tsx's upcoming).
    /// A finished schedule has no next date and isn't listed.
    static func upcoming(_ schedules: [JSON]) -> [JSON] {
        schedules.filter { !$0.text("nextDate").isEmpty }.enumerated()
            .sorted { a, b in
                let x = (a.element.flag("paused") ? 1 : 0, a.element.text("nextDate"), a.offset)
                let y = (b.element.flag("paused") ? 1 : 0, b.element.text("nextDate"), b.offset)
                return x < y
            }
            .map(\.element)
    }

    /// How many schedules there are, and what's due in the next 30 days: count, money out, money in.
    static func summary(_ upcoming: [JSON], today: String) -> ScheduleSummary {
        let end = Dates.addDays(today, 30)
        let paused = upcoming.count { $0.flag("paused") }
        let due = upcoming.filter { !$0.flag("paused") && $0.text("nextDate") <= end }
        let amounts = due.map { $0["transaction"].long("amount") }
        return ScheduleSummary(active: upcoming.count - paused, paused: paused, dueSoon: due.count,
                               goingOut: amounts.reduce(0) { $0 + max(0, -$1) }, comingIn: amounts.reduce(0) { $0 + max(0, $1) })
    }

    /// The occurrence after `current`, as schedules.rs's `next` works it out; nil once a one-time
    /// schedule is done, or for a frequency the engine doesn't know. For showing the dates ahead only:
    /// the engine moves schedules on.
    static func next(_ current: String, anchor: String, frequency: String) -> String? {
        guard let (year, month, day) = parts(current), let (_, _, anchorDay) = parts(anchor) else { return nil }
        let months: Int
        switch frequency {
        case "monthly": months = 1
        case "everyOtherMonth": months = 2
        case "every3Months": months = 3
        case "every4Months": months = 4
        case "twiceAYear": months = 6
        case "yearly": months = 12
        case "everyOtherYear": months = 24
        default: months = 0
        }
        let result: String
        if months > 0 {
            let total = year * 12 + (month - 1) + months
            let (y, m) = (total / 12, total % 12 + 1)
            result = iso(y, m, min(anchorDay, daysIn(y, m)))
        } else if frequency == "twiceAMonth" {
            let base = (anchorDay - 1) % 15 + 1
            var candidate = Dates.addDays(iso(year, month, day), 1)
            var guardrail = 0
            while let (y, m, d) = parts(candidate), guardrail < 62 {
                if d == base || d == min(base + 15, daysIn(y, m)) { break }
                candidate = Dates.addDays(candidate, 1)
                guardrail += 1
            }
            result = candidate
        } else {
            let days: Int
            switch frequency {
            case "daily": days = 1
            case "weekly": days = 7
            case "everyOtherWeek": days = 14
            case "every4Weeks": days = 28
            default: return nil
            }
            result = Dates.addDays(iso(year, month, day), days)
        }
        return Dates.day(result) == nil ? nil : result
    }

    /// Up to `count` dates from `first`, the first included.
    static func dates(from first: String, anchor: String, frequency: String, count: Int) -> [String] {
        guard Dates.day(first) != nil, count > 0 else { return [] }
        var dates = [first]
        while dates.count < count, let following = next(dates[dates.count - 1], anchor: anchor, frequency: frequency), following > dates[dates.count - 1] {
            dates.append(following)
        }
        return dates
    }

    private static func parts(_ value: String) -> (Int, Int, Int)? {
        guard Dates.day(value) != nil else { return nil }
        let numbers = value.split(separator: "-").compactMap { Int($0) }
        return numbers.count == 3 ? (numbers[0], numbers[1], numbers[2]) : nil
    }

    private static func iso(_ year: Int, _ month: Int, _ day: Int) -> String {
        String(format: "%04d-%02d-%02d", year, month, day)
    }

    static func daysIn(_ year: Int, _ month: Int) -> Int {
        switch month {
        case 2: return (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 ? 29 : 28
        case 4, 6, 9, 11: return 30
        default: return 31
        }
    }
}

// MARK: Loans

enum LoanMath {
    /// A debt kept as a tracking account (desktop isLoanType): mortgage, auto, student, personal and
    /// medical loans, and other debt.
    static func isLoanType(_ type: String) -> Bool { AccountTypes.isLiability(type) && AccountTypes.kind(for: type) == "tracking" }

    /// Whether an account has loan details to show or add: a tracking debt, or any account with them saved.
    static func hasLoan(_ account: JSON, balance: Int64, saved: Set<String>) -> Bool {
        saved.contains(account.text("id")) || account.text("kind") == "tracking" && isLoanType(AccountTypes.infer(account, balance: balance))
    }

    /// The annual rate in effect in a month, in thousandths of a percent, as loans.rs reads it.
    static func rateIn(_ terms: JSON, month: String) -> Int64 {
        let best = terms.rows("rateHistory").filter { $0.text("month") <= month }.max { $0.text("month") < $1.text("month") }
        return best?.long("annualRateMilliPercent") ?? terms.long("annualRateBasisPoints") * 10
    }

    /// Thousandths of a percent as someone writes the rate: 5125 as "5.125", 5000 as "5".
    static func rateText(_ milli: Int64, decimal: String = ".") -> String {
        let whole = milli.magnitude / 1000
        var fraction = String(milli.magnitude % 1000)
        fraction = String(repeating: "0", count: 3 - fraction.count) + fraction
        while fraction.hasSuffix("0") { fraction.removeLast() }
        return (milli < 0 ? "-" : "") + String(whole) + (fraction.isEmpty ? "" : decimal + fraction)
    }

    /// A typed annual rate ("5.25") in basis points: two decimals at most, from 0 to 1,000%.
    static func parseRate(_ text: String) throws -> Int64 {
        var typed = text.trimmingCharacters(in: .whitespaces)
        if typed.hasSuffix("%") { typed = String(typed.dropLast()).trimmingCharacters(in: .whitespaces) }
        if typed.isEmpty { return 0 }
        guard let value = try? Money.parse(typed) else { throw AppError(String(localized: "Enter the interest rate as a percentage, like 5.25.")) }
        guard (0...100_000).contains(value) else { throw AppError(String(localized: "The interest rate must be from 0 to 1,000%.")) }
        return value
    }

    /// A dated rate change as typed ("5.125", or "5,125" with a comma decimal mark) in thousandths of
    /// a percent: up to three decimals, from 0 to 1,000% (Planning.tsx's parseRateChange).
    static func parseRateChange(_ text: String, decimal: String = ".") throws -> Int64 {
        var typed = text.trimmingCharacters(in: .whitespaces)
        if typed.hasSuffix("%") { typed = String(typed.dropLast()).trimmingCharacters(in: .whitespaces) }
        if decimal == "," { typed = typed.replacingOccurrences(of: ",", with: ".") }
        guard typed.range(of: #"^[0-9]+(\.[0-9]{1,3})?$"#, options: .regularExpression) != nil else {
            throw AppError(String(localized: "Enter a rate with up to three decimals."))
        }
        let pieces = typed.split(separator: ".", omittingEmptySubsequences: false)
        let fraction = pieces.count > 1 ? String(pieces[1]).padding(toLength: 3, withPad: "0", startingAt: 0) : "000"
        guard pieces[0].count <= 7, let whole = Int64(pieces[0]), let part = Int64(fraction), whole * 1000 + part <= 1_000_000 else {
            throw AppError(String(localized: "Rate must be between 0 and 1,000%."))
        }
        return whole * 1000 + part
    }

    /// "2 years 3 months".
    static func span(_ months: Int) -> String {
        let years = months / 12, rest = months % 12
        if years != 0 && rest != 0 { return String(localized: "\(years) years \(rest) months", locale: .app) }
        if years != 0 { return String(localized: "\(years) years", locale: .app) }
        return String(localized: "\(rest) months", locale: .app)
    }

    /// Whether a transaction pays money into the loan account (Planning.tsx's paysInto).
    static func paysInto(_ t: JSON, _ accountId: String) -> Bool {
        if t.text("transferAccountId") == accountId && t.long("amount") < 0 { return true }
        if t.rows("splits").contains(where: { $0.text("transferAccountId") == accountId && $0.long("amount") < 0 }) { return true }
        return t.text("accountId") == accountId && t.long("amount") > 0 && !t.text("transferAccountId").isEmpty
    }

    /// When the next payment falls, for projections: the 1st of next month once this month's
    /// payment is in (its interest is charged), else today (Planning.tsx's nextPayment).
    static func nextPayment(_ budget: JSON, accountId: String, today: String) -> String {
        let month = String(today.prefix(7))
        let paid = budget.rows("transactions").contains { t in
            let date = t.text("date")
            return date.hasPrefix(month) && date <= today && paysInto(t, accountId)
        }
        return paid ? Dates.addMonths(month, 1) + "-01" : today
    }

    /// This month's payments into the loan that came without its interest: a transfer entered
    /// before the loan had details, say. Empty once the month's interest is in.
    static func paymentsWithoutInterest(_ budget: JSON, accountId: String, today: String) -> [JSON] {
        let month = String(today.prefix(7))
        let transactions = budget.rows("transactions")
        let charged = transactions.contains { t in
            t.text("id") == "loan-interest:\(accountId):\(month)"
                || t.text("accountId") == accountId && t.text("date").hasPrefix(month)
                && (t.flag("loanInterest") || t["source"].text("debt_transaction_type") == "interest")
        }
        if charged { return [] }
        return transactions.filter { t in
            let date = t.text("date")
            return date.hasPrefix(month) && date <= today && paysInto(t, accountId)
        }
    }

    /// The account a loan payment comes from by default: the one that paid this loan last, else a
    /// checking account, then the one with the most in it (LoanPayment.tsx).
    static func defaultSource(_ budget: JSON, loanId: String, sources: [JSON], balances: [String: Int64]) -> String {
        let ids = Set(sources.map { $0.text("id") })
        let latest = budget.rows("transactions")
            .filter { $0.text("transferAccountId") == loanId && $0.long("amount") < 0 && ids.contains($0.text("accountId")) }
            .max { $0.text("date") < $1.text("date") }
        if let latest { return latest.text("accountId") }
        return sources.enumerated().sorted { a, b in
            let x = (AccountTypes.infer(a.element) == "checking" ? 0 : 1, -(balances[a.element.text("id")] ?? 0), a.offset)
            let y = (AccountTypes.infer(b.element) == "checking" ? 0 : 1, -(balances[b.element.text("id")] ?? 0), b.offset)
            return x < y
        }.first?.element.text("id") ?? ""
    }

    /// A typed planner amount: blank is 0, nil when it doesn't read.
    static func plannerAmount(_ text: String) -> Int64? {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return 0 }
        guard let value = try? Money.parse(trimmed) else { return nil }
        return max(0, value)
    }
}

/// A dated rate change as typed; `source` keeps any fields this app doesn't edit.
struct LoanRateDraft: Identifiable, Equatable {
    var id = UUID()
    var month: String
    var rate: String
    var source: JSON?
}

/// A loan's details as typed (LoanDetails.tsx's LoanDraft).
struct LoanDraft: Equatable {
    var rate = ""
    var payment = ""
    var fees = ""
    var category = ""
    var rates: [LoanRateDraft] = []

    init() {}

    init(saved: JSON?) {
        guard let saved else { return }
        rate = Money.editable(saved.long("annualRateBasisPoints"))
        payment = saved.long("monthlyPayment") > 0 ? Money.editable(saved.long("monthlyPayment")) : ""
        fees = saved.long("monthlyFees") > 0 ? Money.editable(saved.long("monthlyFees")) : ""
        category = saved.text("categoryId")
        rates = saved.rows("rateHistory").map {
            LoanRateDraft(month: $0.text("month"), rate: LoanMath.rateText($0.long("annualRateMilliPercent"), decimal: Money.decimalMark()), source: $0)
        }
    }

    var isEmpty: Bool { rate.isEmpty && payment.isEmpty && fees.isEmpty && category.isEmpty && rates.isEmpty }

    /// The typed details as loan terms (loans.rs's Terms, with any fields kept from `saved`), or a
    /// sentence saying what doesn't read.
    func terms(accountId: String, saved: JSON?, requireCategory: Bool) throws -> JSON {
        guard !accountId.isEmpty else { throw AppError(String(localized: "Choose the loan account.")) }
        let typedPayment = payment.trimmingCharacters(in: .whitespaces)
        let monthly = typedPayment.isEmpty ? 0 : try Money.parse(typedPayment)
        guard monthly > 0 else { throw AppError(String(localized: "Enter the loan's monthly payment as a positive amount.")) }
        let typedFees = fees.trimmingCharacters(in: .whitespaces)
        let charges = typedFees.isEmpty ? 0 : try Money.parse(typedFees)
        guard charges >= 0 else { throw AppError(String(localized: "Enter the monthly fees as a positive amount.")) }
        let basisPoints = try LoanMath.parseRate(rate)
        let history = try rates.map { row -> JSON in
            guard Dates.isMonth(row.month) else { throw AppError(String(localized: "Choose the month each rate change starts.")) }
            var record = row.source ?? [:]
            record["month"] = .string(row.month)
            record["annualRateMilliPercent"] = .int(try LoanMath.parseRateChange(row.rate, decimal: Money.decimalMark()))
            return record
        }.sorted { $0.text("month") < $1.text("month") }
        guard Set(history.map { $0.text("month") }).count == history.count else { throw AppError(String(localized: "Keep one interest rate per month.")) }
        if requireCategory && category.isEmpty { throw AppError(String(localized: "Choose the category the payment comes from.")) }
        var record = saved ?? [:]
        record["accountId"] = .string(accountId)
        record["categoryId"] = .string(category)
        record["annualRateBasisPoints"] = .int(basisPoints)
        record["monthlyPayment"] = .int(monthly)
        record["monthlyFees"] = .int(charges)
        record["rateHistory"] = .array(history)
        return record
    }
}

// MARK: Card payments

enum PlanningCardMath {
    /// The account a card payment comes from by default: the one that paid this card last; else
    /// checking, then anything but savings, then the one with the most in it (CardPayment.tsx).
    static func defaultSource(_ budget: JSON, cardId: String, sources: [JSON], balances: [String: Int64]) -> String {
        let ids = Set(sources.map { $0.text("id") })
        let latest = budget.rows("transactions")
            .filter { $0.text("transferAccountId") == cardId && $0.long("amount") < 0 && ids.contains($0.text("accountId")) }
            .max { $0.text("date") < $1.text("date") }
        if let latest { return latest.text("accountId") }
        func rank(_ account: JSON) -> Int {
            let type = account.text("accountType")
            if type == "checking" { return 0 }
            return type == "savings" || account.text("name").localizedCaseInsensitiveContains("saving") ? 2 : 1
        }
        return sources.enumerated().sorted { a, b in
            (rank(a.element), -(balances[a.element.text("id")] ?? 0), a.offset) < (rank(b.element), -(balances[b.element.text("id")] ?? 0), b.offset)
        }.first?.element.text("id") ?? ""
    }
}

// MARK: Payees

/// One of a payee's recent transactions, as its page lists them.
struct PayeeRecent: Identifiable {
    var id: String
    var transaction: JSON
    var date: String
    var accountId: String
    var splits: Int
    var categoryId: String
    var unassigned: Bool
    var amount: Int64
}

/// A payee's page: how many transactions, their total, and the latest six (PayeeIndex.kt's payeeDetail).
struct PayeeDetail {
    var count: Int
    var total: Int64
    var recent: [PayeeRecent]

    init(budget: JSON, name: String, limit: Int = 6) {
        let matching = budget.rows("transactions").filter { $0.text("payee") == name }
        count = matching.count
        total = matching.reduce(0) { $0 + $1.long("amount") }
        let newest = matching.sorted { ($0.text("date"), $0.text("createdAt")) > ($1.text("date"), $1.text("createdAt")) }
        recent = newest.prefix(limit).enumerated().map { index, t in
            let splits = t.rows("splits")
            return PayeeRecent(id: t.text("id").isEmpty ? "\(index):\(t.text("date"))" : t.text("id"), transaction: t, date: t.text("date"),
                               accountId: t.text("accountId"), splits: splits.count, categoryId: splits.first?.text("categoryId") ?? "",
                               unassigned: splits.first?.flag("unassigned") ?? false, amount: t.long("amount"))
        }
    }
}

/// What renaming a payee would change: transactions (with split lines), schedules and rules (PayeesPage.tsx's payeeUsage).
struct PayeeUsage: Equatable {
    var transactions: Int
    var schedules: Int
    var rules: Int

    init(budget: JSON, name: String) {
        transactions = budget.rows("transactions").count { t in t.text("payee") == name || t.rows("splits").contains { $0.text("payee") == name } }
        schedules = budget.rows("schedules").count { s in
            let t = s["transaction"]
            return t.text("payee") == name || t.rows("splits").contains { $0.text("payee") == name }
        }
        rules = budget.rows("payeeRules").count { $0.text("payee") == name }
    }

    /// "12 transactions, 1 schedule, 2 rules".
    var description: String {
        planningList([PayeeText.transactions(transactions), schedules > 0 ? String(localized: "\(schedules) schedules", locale: .app) : nil,
                      rules > 0 ? String(localized: "\(rules) rules", locale: .app) : nil].compactMap { $0 })
    }
}

enum PayeeText {
    static func plural(_ n: Int, _ word: String) -> String {
        "\(n.formatted()) \(word)\(n == 1 ? "" : "s")"
    }

    /// "12 transactions", or "1,234 transactions" grouped as the number always was.
    static func transactions(_ n: Int) -> String {
        if n < 1000 { return String(localized: "\(n) transactions", locale: .app) }
        let grouped = n.formatted(.number.locale(.app))
        return String(localized: "\(grouped) transactions")
    }
}

/// Items in a sentence ("21/10/2026, 21/11/2026"), separated in the app's language. In Arabic each
/// item keeps its own direction and the list reads right to left.
func planningList(_ items: [String]) -> String {
    planningJoined(items, separator: String(localized: ", ", comment: "Between the items of a list in a sentence, as in “3 transactions, 1 rule”."))
}

/// Parts of a line ("Due 21/10/2026 · Monthly · Checking"). In Arabic each part keeps its own
/// direction (a date, a name), and the line reads right to left.
func planningJoined(_ parts: [String], separator: String) -> String {
    guard AppLanguage.isRightToLeft else { return parts.joined(separator: separator) }
    return "\u{200F}" + parts.map { "\u{2068}" + $0 + "\u{2069}" }.joined(separator: separator)
}

/// The saved payee record for a name, as payeeIndex reads them: the first that isn't a YNAB transfer
/// payee or deleted.
func planningPayeeRecord(_ budget: JSON, name: String) -> JSON? {
    budget.rows("payees").first { p in
        p.text("name") == name && p.text("transfer_account_id").isEmpty && !p.flag("deleted") && !name.hasPrefix("Transfer : ")
    }
}

/// What the payee rules would change, worked out by running them in the shared engine on a copy of
/// the budget (workflows.rs's reapplyPayeeRules), so the preview is exactly what Apply does.
enum PayeeRulePreview {
    /// How many transactions running `rules` (default: the budget's) again would rename or
    /// categorize; with `only`, just where a transaction's best rule is one of those ids.
    static func changes(budget: JSON, rules: [JSON]? = nil, only: [String]? = nil) throws -> Int {
        var candidate = budget
        if let rules { candidate["payeeRules"] = .array(rules) }
        if candidate.rows("payeeRules").allSatisfy({ $0.text("match").trimmingCharacters(in: .whitespaces).isEmpty }) { return 0 }
        var action: JSON = ["type": "reapplyPayeeRules"]
        if let only { action["ruleIds"] = JSON(only) }
        let changed = try NativeCore.engine("workflow", budget: candidate, params: ["action": action])
        return differences(before: candidate["transactions"].array, after: changed["transactions"].array)
    }

    /// Transactions whose payee or categories differ, matched by id.
    static func differences(before: [JSON], after: [JSON]) -> Int {
        func key(_ t: JSON) -> [String] { [t.text("payee")] + t.rows("splits").map { $0.text("categoryId") } }
        if before.count == after.count && zip(before, after).allSatisfy({ $0.text("id") == $1.text("id") }) {
            return zip(before, after).count { key($0) != key($1) }
        }
        var earlier: [String: [String]] = [:]
        for t in before { earlier[t.text("id")] = key(t) }
        return after.count { t in earlier[t.text("id")].map { $0 != key(t) } ?? false }
    }
}

// MARK: Store

extension BudgetStore {
    /// An engine command committed in the edit queue, returning once it's saved (or throwing its
    /// error), so a sheet can stay open with the error in place. With `original`, it's refused when
    /// that record changed since the editor opened it.
    func planningCommand(_ method: String, _ params: JSON, original: (collection: String, record: JSON)? = nil) async throws {
        try await perform { repository in ((), try await repository.command(method, params, original: original)) }
    }

    /// An edit, then engine commands each on the last one's result, saved as one change (one undo).
    func planningEdit(original: (collection: String, record: JSON)? = nil, then commands: [(String, JSON)] = [],
                      _ transform: @escaping @Sendable (inout JSON) throws -> Void) async throws {
        try await perform { repository in
            ((), try await repository.edit({ budget in
                if let original { try BudgetRepository.verify(budget, original.collection, original.record) }
                try transform(&budget)
            }, then: commands))
        }
    }

    /// An engine request that needs no budget (loan projections), off the main thread.
    nonisolated func planningPure(_ method: String, _ params: JSON) async throws -> JSON {
        try await Task.detached(priority: .userInitiated) { try NativeCore.engine(method, params: params) }.value
    }

    /// Accounts' balances from the published figures.
    var planningBalances: [String: Int64] {
        var balances: [String: Int64] = [:]
        for row in summary?.rows("accounts") ?? [] { balances[row.text("id")] = row.long("balance") }
        return balances
    }
}

/// A category's name as the planning screens show it, Unassigned included.
func planningCategoryName(_ budget: JSON?, _ id: String) -> String? {
    switch id {
    case "": return nil
    case Unassigned: return String(localized: "Unassigned")
    default: return budget?.row("categories", id: id)?.text("name") ?? String(localized: "Deleted category")
    }
}

/// A label over a value, in a row that leads to a picker (as TransactionEditor's rows read).
struct PlanningPickerRow: View {
    var icon: String
    var label: String
    var value: String?
    var placeholder: String
    var attention = false

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 1) {
                if value != nil { Text(label).font(.caption).foregroundStyle(Color.secondary) }
                Text(value ?? placeholder).foregroundStyle(value != nil ? .primary : attention ? Color.urWarning : .secondary).lineLimit(1)
            }
        } icon: {
            Image(systemName: icon).foregroundStyle(attention ? Color.urWarning : .secondary)
        }
    }
}

/// A round symbol before a row's title, tinted as Android's list icons are.
struct PlanningRowIcon: View {
    var symbol: String
    var tint: Color

    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(tint)
            .frame(width: 34, height: 34)
            .background(tint.opacity(0.14), in: Circle())
            .accessibilityHidden(true)
    }
}
