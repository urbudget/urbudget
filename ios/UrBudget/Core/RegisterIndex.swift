import Foundation

/// Read-only presentation index for the register, prepared off the main thread once per budget snapshot.
struct RegisterIndex: Sendable {
    struct Entry: Sendable {
        var transaction: JSON
        var id: String
        var date: String
        var created: String
        var payeeOrder: String
        var amount: Int64
        var accountAmounts: [String: Int64]
        var status: String
        var accountStatuses: [String: [String]]
        var approved: Bool
        var categories: Set<String>
        var flag: String
        var search: [String]
        /// Which lines need a category (desktop's uncategorized rule), by split index.
        var needsCategory: Set<Int>

        func amount(for account: String) -> Int64 { account.isEmpty ? amount : accountAmounts[account] ?? 0 }
    }

    struct Filter: Equatable, Sendable {
        var account = ""
        var status = ""
        var category = ""
        var flag = ""
        var from = ""
        var through = ""
        var minimum: Int64?
        var maximum: Int64?
        var invalidAmount = false
        var search = ""
        var order = "newest"
        /// Only these transactions, by id; empty for any.
        var only: Set<String> = []
    }

    let entries: [Entry]

    /// How many transactions wait for approval in an account (or all of them).
    func awaitingApproval(_ account: String = "") -> Int { awaitingApprovalIds(account).count }

    func awaitingApprovalIds(_ account: String = "") -> Set<String> {
        Set(entries.lazy.filter { !$0.approved && (account.isEmpty || $0.accountAmounts[account] != nil) }.map(\.id))
    }

    func matching(_ filter: Filter) -> [Entry] {
        if filter.invalidAmount { return [] }
        let terms = filter.search.split(whereSeparator: \.isWhitespace).map(String.init)
        var found = entries.filter { row in
            if !filter.only.isEmpty && !filter.only.contains(row.id) { return false }
            if !filter.account.isEmpty && row.accountAmounts[filter.account] == nil { return false }
            let statuses = filter.account.isEmpty ? [row.status] : row.accountStatuses[filter.account] ?? []
            switch filter.status {
            case "": break
            case "approval": if row.approved { return false }
            case "notReconciled": if statuses.contains("reconciled") { return false }
            default: if !statuses.contains(filter.status) { return false }
            }
            if !filter.category.isEmpty && !row.categories.contains(filter.category) { return false }
            switch filter.flag {
            case "": break
            case "__any": if row.flag.isEmpty { return false }
            case "__none": if !row.flag.isEmpty { return false }
            default: if row.flag != filter.flag { return false }
            }
            if !filter.from.isEmpty && row.date < filter.from || !filter.through.isEmpty && row.date > filter.through { return false }
            // Amounts match by size, so a range finds outflows and inflows alike, as on the desktop.
            let size = row.amount(for: filter.account).magnitude
            if let minimum = filter.minimum, size < minimum.magnitude { return false }
            if let maximum = filter.maximum, size > maximum.magnitude { return false }
            // Every word must appear in some field.
            return terms.allSatisfy { term in row.search.contains { $0.localizedCaseInsensitiveContains(term) } }
        }
        switch filter.order {
        case "oldest": found.sort { $0.date < $1.date }
        case "amount": found.sort { $0.amount(for: filter.account) < $1.amount(for: filter.account) }
        case "payee": found.sort { $0.payeeOrder < $1.payeeOrder }
        default: found.sort { $0.date != $1.date ? $0.date > $1.date : $0.created > $1.created }
        }
        return found
    }

    init(budget: JSON) {
        let currency = budget.text("currency")
        var accounts: [String: String] = [:]
        for a in budget.rows("accounts") { accounts[a.text("id")] = a.text("name") }
        var categories: [String: String] = [:]
        for c in budget.rows("categories") { categories[c.text("id")] = c.text("name") }
        let uncategorized = Uncategorized(budget: budget)
        entries = budget.rows("transactions").map { t in
            let splits = t.rows("splits")
            var accountIds: Set<String> = [t.text("accountId")]
            if !t.text("transferAccountId").isEmpty { accountIds.insert(t.text("transferAccountId")) }
            for part in splits where !part.text("transferAccountId").isEmpty { accountIds.insert(part.text("transferAccountId")) }
            var needs: Set<Int> = []
            var tags: Set<String> = []
            for (index, part) in splits.enumerated() {
                if part.flag("unassigned") { tags.insert(Unassigned) }
                let category = part.text("categoryId")
                if !category.isEmpty { tags.insert(category) }
                // "Uncategorized" means what desktop counts: money leaving the budget with no category.
                if uncategorized.needs(t, part) { tags.insert(NeedsCategory); needs.insert(index) }
            }
            let amount = t.long("amount")
            var amounts: [String: Int64] = [:]
            var statuses: [String: [String]] = [:]
            for id in accountIds {
                amounts[id] = registerAmount(t, account: id)
                statuses[id] = registerStatuses(t, account: id)
            }
            let splitWords = splits.map { "\(categories[$0.text("categoryId")] ?? "") \($0.text("memo")) \($0.text("payee"))" }.joined(separator: " ")
            return Entry(
                transaction: t, id: t.text("id"), date: t.text("date"), created: t.text("createdAt"),
                payeeOrder: t.text("payee").lowercased(), amount: amount, accountAmounts: amounts, status: t.text("status"),
                accountStatuses: statuses, approved: t.flag("approved", true), categories: tags, flag: t.text("flag"),
                search: [t.text("payee"), t.text("memo"), t.text("date"), accounts[t.text("accountId")] ?? "",
                         accounts[t.text("transferAccountId")] ?? "", splitWords, Money.editable(Int64(amount.magnitude)),
                         money(Int64(amount.magnitude), currency)],
                needsCategory: needs)
        }
    }
}

/// The category filter for spending that still needs a category.
let NeedsCategory = "__none"
/// The budget's Unassigned (YNAB's Ready to Assign), where a category id would go.
let Unassigned = "__rta"

/// A transaction's amount as one account's register shows it: its own side, or the other side of a transfer.
func registerAmount(_ t: JSON, account: String) -> Int64 {
    if account.isEmpty || t.text("accountId") == account { return t.long("amount") }
    if t.text("transferAccountId") == account { return -t.long("amount") }
    return -t.rows("splits").filter { $0.text("transferAccountId") == account }.reduce(0) { $0 + $1.long("amount") }
}

/// The cleared statuses one account's side of a transaction has.
func registerStatuses(_ t: JSON, account: String) -> [String] {
    if account.isEmpty || t.text("accountId") == account { return [t.text("status")] }
    if t.text("transferAccountId") == account { return [t.text("transferStatus", "uncleared")] }
    return t.rows("splits").filter { $0.text("transferAccountId") == account }.map { $0.text("transferStatus", "uncleared") }
}
