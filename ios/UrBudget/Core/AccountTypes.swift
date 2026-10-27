import Foundation

/// Account types as the desktop reads them (desktop/src/accounts.ts): the type chosen, else the
/// type a YNAB import gave the account, else a guess from its kind, tracking accounts by the sign
/// of their balance. The type sets the kind; a kind is never changed by reading a type.
enum AccountTypes {
    /// Each type's id (stored) and its name in the app's language.
    static let all: [(id: String, label: String)] = [
        ("checking", String(localized: "Checking")), ("savings", String(localized: "Savings")), ("cash", String(localized: "Cash")),
        ("creditCard", String(localized: "Credit card")), ("lineOfCredit", String(localized: "Line of credit")),
        ("investment", String(localized: "Investment")), ("retirement", String(localized: "Retirement")),
        ("mortgage", String(localized: "Mortgage")), ("autoLoan", String(localized: "Auto loan")),
        ("studentLoan", String(localized: "Student loan")), ("personalLoan", String(localized: "Personal loan")),
        ("medicalDebt", String(localized: "Medical debt")), ("property", String(localized: "Property")),
        ("vehicle", String(localized: "Vehicle")), ("receivable", String(localized: "Receivable")),
        ("otherAsset", String(localized: "Other asset")), ("otherDebt", String(localized: "Other debt")),
    ]
    private static let known = Set(all.map(\.id))
    private static let liabilities: Set<String> = ["creditCard", "lineOfCredit", "mortgage", "autoLoan", "studentLoan", "personalLoan", "medicalDebt", "otherDebt"]

    static func kind(for type: String) -> String {
        switch type {
        case "checking", "savings", "cash": return "cash"
        case "creditCard", "lineOfCredit": return "credit"
        default: return "tracking"
        }
    }

    static func isLiability(_ type: String) -> Bool { liabilities.contains(type) }

    static func label(_ type: String) -> String { all.first { $0.id == type }?.label ?? type }

    /// The account's type, as accounts.ts's accountType reads it.
    static func infer(_ account: JSON, balance: Int64? = nil) -> String {
        let chosen = account.text("accountType")
        if known.contains(chosen) { return chosen }
        let imported = account["source"]["type"].string
        if imported == "otherLiability" { return "otherDebt" }
        if known.contains(imported) { return imported }
        switch account.text("kind") {
        case "credit": return "creditCard"
        case "cash": return "checking"
        default: return (balance ?? account.long("openingBalance")) < 0 ? "otherDebt" : "otherAsset"
        }
    }

    /// The type an account form starts with. A type of another kind than the stored one falls back
    /// to the guess from the kind alone, so editing never moves history into or out of the budget.
    static func forEditing(_ account: JSON, balance: Int64) -> String {
        let selected = infer(account, balance: balance)
        if kind(for: selected) == account.text("kind") { return selected }
        return infer(["kind": .string(account.text("kind")), "openingBalance": .int(account.long("openingBalance"))], balance: balance)
    }

    /// Tracking accounts can opt out of net worth; nothing else does.
    static func inNetWorth(_ account: JSON) -> Bool { account["netWorth"] != .bool(false) }

    /// Whether the account's money is in the budget.
    static func onBudget(_ account: JSON) -> Bool { account.text("kind") != "tracking" && account["onBudget"] != .bool(false) }

    /// The words under an account's name.
    static func typeLabel(_ account: JSON) -> String {
        let chosen = account.text("accountType")
        if known.contains(chosen) { return label(chosen) }
        switch account.text("kind") {
        case "credit": return String(localized: "Credit card")
        case "tracking": return String(localized: "Tracking")
        default: return String(localized: "Cash")
        }
    }

    /// An SF Symbol for the type, as the desktop's accounts list draws them.
    static func symbol(type: String, kind: String) -> String {
        switch type {
        case "checking": return "building.columns"
        case "savings": return "banknote"
        case "cash": return "dollarsign.circle"
        case "creditCard", "lineOfCredit": return "creditcard"
        case "mortgage", "property": return "house"
        case "autoLoan", "vehicle": return "car"
        case "studentLoan": return "graduationcap"
        case "investment", "retirement": return "chart.line.uptrend.xyaxis"
        case "receivable", "personalLoan": return "hand.raised"
        case "medicalDebt": return "cross.case"
        case "otherDebt": return "doc.text"
        default: return kind == "credit" ? "creditcard" : kind == "tracking" ? "chart.pie" : "building.columns"
        }
    }

    /// The accounts with `id` moved next to `neighbour` (after it when `after`).
    static func move(_ accounts: [JSON], id: String, neighbour: String, after: Bool) -> [JSON] {
        guard id != neighbour, let moving = accounts.first(where: { $0.text("id") == id }) else { return accounts }
        var rest = accounts.filter { $0.text("id") != id }
        guard let at = rest.firstIndex(where: { $0.text("id") == neighbour }) else { return accounts }
        rest.insert(moving, at: at + (after ? 1 : 0))
        return rest
    }
}

/// What an account row marks: transactions waiting for approval, and spending that needs a category.
struct AccountBadges {
    var approvals: [String: Int] = [:]
    var uncategorized: [String: Int] = [:]

    init(budget: JSON, today: String = Dates.today) {
        let checker = Uncategorized(budget: budget, today: today)
        let tracking = Set(budget.rows("accounts").filter { !AccountTypes.onBudget($0) }.map { $0.text("id") })
        for t in budget.rows("transactions") {
            let account = t.text("accountId")
            if t["approved"] == .bool(false) { approvals[account, default: 0] += 1 }
            var affected: Set<String> = []
            for s in t.rows("splits") where checker.needs(t, s) {
                let peer = s.text("transferAccountId").isEmpty ? t.text("transferAccountId") : s.text("transferAccountId")
                let owner = tracking.contains(account) ? peer : account
                if !owner.isEmpty { affected.insert(owner) }
            }
            for owner in affected { uncategorized[owner, default: 0] += 1 }
        }
    }
}
