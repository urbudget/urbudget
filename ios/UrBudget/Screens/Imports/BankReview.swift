import SwiftUI

/// One thing a bank row can do: "review" (still to decide), "new", "skip", or "match:<candidate>".
struct BankReviewOption: Identifiable, Hashable, Sendable {
    var id: String
    var label: String
}

/// A bank row as its review shows it, built off the main thread from the engine's preview; the
/// source JSON stays read-only. A `locked` row is already imported and has nothing to decide.
/// "review" is a decision still to make: the import waits until no row has it.
struct BankReviewRow: Identifiable, Sendable {
    var id: String
    var source: JSON
    var options: [BankReviewOption]
    var initial: String
    var manualImportId: String
    var previouslyMatched: Set<String>
    var locked = false
    var date = ""
    var payee = ""
    var bankText: String?
    var memo = ""
    var amount: Int64 = 0
    var amountText = ""
    var duplicate = false
    var possibleDuplicate = false
    var idless = false
    var detail = ""
    /// The transaction in another account it's offered as the other side of, when there is one.
    var transfer: String?
    /// The bank text backs that transfer up, so it's chosen; otherwise it's only suggested.
    var transferSure = false
    /// Money in that may be a refund of a purchase: what it's of ("Refund of Shop · 23.99 on 7 Oct 2026 · Shopping").
    var refund: String?

    /// The row's decision, given the choices made so far.
    func decision(_ chosen: String?) -> String { locked ? "skip" : chosen ?? initial }
}

/// The review's figures: rows already imported, rows a payee rule renamed, and what a note should say.
struct BankReviewSummary: Equatable, Sendable {
    var rows: Int
    var duplicates: Int
    var renamed: Int
    var idless: Bool
    var allIn: Bool
    var transfers = false
    var refunds = 0
}

/// The bank review the desktop's BankImport.tsx and Android's EditorPreparation.kt build: each
/// row's choices and default, and the choices `bankApply` and `simplefin_import` take.
enum BankReview {
    /// Desktop's defaultChoice: skip known bank IDs; take a single match only when that account
    /// side was not imported before, and not a transfer the bank text doesn't back up
    /// (`suggestedTransfer`), which amount and date alone can't tell from a card payment.
    static func defaultChoice(duplicate: Bool, possibleDuplicate: Bool, candidates: [String], previouslyMatched: Set<String>,
                              suggestedTransfer: String? = nil) -> String {
        if duplicate { return "skip" }
        if possibleDuplicate { return "review" }
        if candidates.isEmpty { return "new" }
        if candidates.count == 1 && !previouslyMatched.contains(candidates[0]) && candidates[0] != suggestedTransfer {
            return "match:\(candidates[0])"
        }
        return "review"
    }

    /// How many rows a review imports or matches, and how many still need a choice.
    static func counts(_ rows: [BankReviewRow], _ chosen: [Int: String]) -> (importing: Int, undecided: Int) {
        var importing = 0
        var undecided = 0
        for (index, row) in rows.enumerated() {
            switch row.decision(chosen[index]) {
            case "skip": break
            case "review": undecided += 1
            default: importing += 1
            }
        }
        return (importing, undecided)
    }

    static func summarize(_ rows: [BankReviewRow]) -> BankReviewSummary {
        BankReviewSummary(rows: rows.count, duplicates: rows.count(where: \.duplicate),
                          renamed: rows.count { !$0.duplicate && $0.bankText != nil },
                          idless: rows.contains { $0.possibleDuplicate || $0.idless && $0.duplicate },
                          allIn: rows.count >= 3 && rows.allSatisfy { $0.amount > 0 },
                          transfers: rows.contains { $0.transfer != nil }, refunds: rows.count { $0.refund != nil })
    }

    /// "12 rows · 3 already imported · 2 renamed by your payee rules".
    static func countsText(_ summary: BankReviewSummary) -> String {
        var text = summary.rows == 1 ? "1 row" : "\(summary.rows) rows"
        if summary.duplicates > 0 { text += " · \(summary.duplicates) already imported" }
        if summary.renamed > 0 { text += " · \(summary.renamed) renamed by your payee rules" }
        if summary.refunds > 0 { text += " · \(summary.refunds) \(summary.refunds == 1 ? "refund" : "refunds")" }
        return text
    }

    /// Under every review's rows.
    static let matchingNote = "Matching keeps your category and memo and marks the transaction cleared. Imported transactions wait for your approval."

    private static let alreadyImported = [BankReviewOption(id: "skip", label: "Already imported — skip"),
                                          BankReviewOption(id: "new", label: "Add as a separate transaction")]

    /// One candidate pool's choices, built once and shared by every row that uses the pool.
    private struct Pool {
        var choose: [BankReviewOption]
        var possible: [BankReviewOption]
        var candidates: [String]
        var previouslyMatched: Set<String>
        var transfer: String?
        var transferSure: Bool
    }

    /// Review rows for the engine's items. Candidates come as a shared pool (`candidateSets`), or
    /// on each row; neither is expanded once per row.
    static func prepare(rows: [JSON], budget: JSON, allowIdlessDuplicate: Bool, candidateSets: [JSON]? = nil) throws -> [BankReviewRow] {
        let currency = budget.text("currency").isEmpty ? "SAR" : budget.text("currency")
        var transactions: [String: JSON] = [:]
        for transaction in budget.rows("transactions") { transactions[transaction.text("id")] = transaction }
        var accountNames: [String: String] = [:]
        for account in budget.rows("accounts") { accountNames[account.text("id")] = account.text("name") }
        var categoryNames: [String: String] = [:]
        for category in budget.rows("categories") { categoryNames[category.text("id")] = category.text("name") }
        var pools: [Int: Pool] = [:]

        func pool(_ source: JSON) -> Pool {
            var details: [String: JSON] = [:]
            for detail in source.rows("candidateDetails") { details[detail.text("id")] = detail }
            let previouslyMatched = Set(source["previouslyMatchedCandidates"].array.map(\.string))
            let candidates = source["candidates"].array.map(\.string)
            let transfers = Set(source["transferCandidates"].array.map(\.string))
            let transfer = source["transferCandidates"].array.first?.string
            let matches = candidates.map { id -> BankReviewOption in
                let detail = details[id]
                let transactionId = detail?.text("transactionId") ?? ""
                let saved = transactions[transactionId.isEmpty ? id : transactionId]
                let other = transfers.contains(id) ? saved.flatMap { accountNames[$0.text("accountId")] } : nil
                return BankReviewOption(id: "match:\(id)", label: matchLabel(id, saved, detail, previouslyMatched.contains(id), other: other))
            }
            return Pool(
                choose: [.init(id: "review", label: "Choose a match or add it"), .init(id: "new", label: "Add as a new transaction"), .init(id: "skip", label: "Skip")] + matches,
                possible: [.init(id: "review", label: "May already be imported — choose"), .init(id: "new", label: "Add as a new transaction"), .init(id: "skip", label: "Skip")] + matches,
                candidates: candidates, previouslyMatched: previouslyMatched, transfer: transfer, transferSure: source.flag("transferSure"))
        }

        var prepared: [BankReviewRow] = []
        prepared.reserveCapacity(rows.count)
        for (index, row) in rows.enumerated() {
            if index % 128 == 0 { try Task.checkCancellation() }
            let set: Pool
            if row.has("candidateSet") {
                let setIndex = Int(row.long("candidateSet"))
                if let cached = pools[setIndex] {
                    set = cached
                } else {
                    guard let sets = candidateSets, sets.indices.contains(setIndex) else {
                        throw AppError("Bank match details are missing. Preview again.")
                    }
                    set = pool(sets[setIndex])
                    pools[setIndex] = set
                }
            } else {
                set = pool(row)
            }
            let duplicate = row.flag("duplicate")
            let possible = row.flag("possibleDuplicate")
            let idless = row.flag("idless")
            // An already-imported row can only be added again when it has no bank ID and that's allowed.
            let separate = duplicate && allowIdlessDuplicate && idless
            let options = separate ? alreadyImported : duplicate ? [] : possible ? set.possible : set.choose
            let bankPayee = row.text("payee")
            let renamed = row.text("rename")
            let rename: String? = !renamed.trimmingCharacters(in: .whitespaces).isEmpty && renamed != bankPayee ? renamed : nil
            let shownPayee = rename ?? bankPayee
            let date = Money.date(row.text("date"))
            let memo = row.text("memo")
            let amount = row.long("amount")
            prepared.append(BankReviewRow(
                id: "\(index):\(row.text("id"))", source: small(row), options: options,
                initial: defaultChoice(duplicate: duplicate, possibleDuplicate: possible, candidates: set.candidates, previouslyMatched: set.previouslyMatched,
                                       suggestedTransfer: set.transferSure ? nil : set.transfer),
                manualImportId: "bank:manual:\(UUID().uuidString.lowercased())", previouslyMatched: set.previouslyMatched,
                locked: duplicate && !separate, date: date,
                payee: shownPayee.trimmingCharacters(in: .whitespaces).isEmpty ? "No payee" : shownPayee,
                bankText: rename == nil ? nil : bankPayee, memo: memo, amount: amount,
                amountText: Money.format(amount, currency: currency), duplicate: duplicate, possibleDuplicate: possible, idless: idless,
                detail: [date, memo].filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.joined(separator: " · "),
                transfer: duplicate ? nil : set.transfer, transferSure: set.transferSure,
                refund: duplicate || !row["refundOf"].isObject ? nil : BankStatementText.refund(row["refundOf"], categories: categoryNames, currency: currency)))
        }
        return prepared
    }

    /// "Match: 3 Oct 2026 · Shop · Split 2 · memo · Previously imported", as the desktop labels a match;
    /// "Transfer from Savings: 2 Oct 2026 · TO EVERYDAY" for the other side of money moved from `other`.
    private static func matchLabel(_ id: String, _ saved: JSON?, _ detail: JSON?, _ previously: Bool, other: String? = nil) -> String {
        guard let saved else { return "Match: \(id)\(previously ? " · Previously imported" : "")" }
        let splits = saved.rows("splits")
        let splitIndex = detail.map { Int($0.long("splitIndex")) }
        let part = splitIndex.flatMap { splits.indices.contains($0) ? splits[$0] : nil }
        let partMemo = part?.text("memo") ?? ""
        let memo = partMemo.trimmingCharacters(in: .whitespaces).isEmpty ? saved.text("memo") : partMemo
        let payee = saved.text("payee")
        let start = other.map { "Transfer \(saved.long("amount") < 0 ? "from" : "to") \($0): " } ?? "Match: "
        var label = "\(start)\(Money.date(saved.text("date"))) · \(payee.trimmingCharacters(in: .whitespaces).isEmpty ? "Transfer" : payee)"
        if let splitIndex { label += " · Split \(splitIndex + 1)" }
        if !memo.trimmingCharacters(in: .whitespaces).isEmpty { label += " · \(memo)" }
        if previously { label += " · Previously imported" }
        return label
    }

    /// Matching revalidates against the current ledger; only the selected prior match is needed.
    private static func small(_ source: JSON) -> JSON {
        var row = source
        for key in ["candidateSet", "candidates", "candidateDetails", "previouslyMatchedCandidates", "transferCandidates", "transferSure"] { row.remove(key) }
        return row
    }

    /// The choices for `bankApply` and `simplefin_import`. A row nobody decided on is skipped,
    /// never imported or matched.
    static func choices(_ rows: [BankReviewRow], _ overrides: [Int: String], allowIdlessDuplicate: Bool) -> JSON {
        var choices: [JSON] = []
        choices.reserveCapacity(rows.count)
        for (index, prepared) in rows.enumerated() {
            var decision = prepared.decision(overrides[index])
            if decision == "review" { decision = "skip" }
            let source = prepared.source
            var row = small(source)
            if allowIdlessDuplicate && decision == "new" && source.flag("idless") && (source.flag("duplicate") || source.flag("possibleDuplicate")) {
                row["importId"] = .string(prepared.manualImportId)
            }
            let matchId: String? = decision.hasPrefix("match:") ? String(decision.dropFirst("match:".count)) : nil
            if let matchId, prepared.previouslyMatched.contains(matchId) { row["previouslyMatchedCandidates"] = [.string(matchId)] }
            choices.append(["row": row, "action": .string(matchId == nil ? decision : "match"), "matchId": matchId.map(JSON.string) ?? .null])
        }
        return .array(choices)
    }
}

// MARK: Review rows

/// One bank row: what the bank says, and what to do with it (add, match or skip).
struct BankReviewItem: View {
    var row: BankReviewRow
    var decision: String
    var enabled: Bool
    var choose: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(row.payee).font(.body.weight(.semibold)).lineLimit(2)
                    .foregroundStyle(row.locked ? .secondary : .primary)
                Spacer(minLength: 0)
                Text(row.amountText).font(.body.weight(.semibold)).monospacedDigit().lineLimit(1)
                    .foregroundStyle(row.locked ? .secondary : row.amount > 0 ? Color.urPositive : .primary)
            }
            if let bank = row.bankText {
                Text("Bank: \(bank)").font(.footnote).foregroundStyle(Color.secondary).lineLimit(1)
            }
            // Imported, a refund takes its purchase's category when it has none.
            if let refund = row.refund {
                Label(refund, systemImage: "arrow.uturn.backward").font(.footnote.weight(.semibold)).foregroundStyle(Color.urAccent).lineLimit(2)
            }
            if !row.detail.isEmpty {
                Text(row.detail).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2)
            }
            if row.locked {
                Label("Already imported · skipped", systemImage: "checkmark.circle")
                    .font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary)
            } else {
                if decision == "review" {
                    Text(row.possibleDuplicate ? "May already be imported: compare the memo and amount, then choose" : "Choose the matching transaction, or add it as new")
                        .font(.footnote.weight(.semibold)).foregroundStyle(Color.urWarning)
                }
                chooser.padding(.top, 4)
            }
        }
        .padding(.vertical, 4)
    }

    private var chooser: some View {
        let label = row.options.first { $0.id == decision }?.label ?? decision
        let attention = decision == "review"
        return Menu {
            Picker("What to do", selection: Binding(get: { decision }, set: { choose($0) })) {
                ForEach(row.options) { option in
                    Label(option.label, systemImage: Self.symbol(option.id)).tag(option.id)
                }
            }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: Self.symbol(decision))
                Text(label).multilineTextAlignment(.leading).lineLimit(2)
                Spacer(minLength: 4)
                Image(systemName: "chevron.up.chevron.down").font(.caption.weight(.semibold))
            }
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(attention ? Color.urWarning : Color.urAccent)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background((attention ? Color.urWarning : Color.urAccent).opacity(0.1), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
        .disabled(!enabled)
        .accessibilityLabel("What to do with \(row.payee)")
        .accessibilityValue(label)
    }

    static func symbol(_ choice: String) -> String {
        switch choice {
        case "review": return "questionmark.circle"
        case "new": return "plus.circle"
        case "skip": return "forward"
        default: return "link"
        }
    }
}

/// A review's rows, each with its choice; swipe right to add a row, left to skip it.
struct BankReviewRows: View {
    var rows: [BankReviewRow]
    @Binding var decisions: [Int: String]
    var enabled: Bool

    var body: some View {
        ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
            let decision = row.decision(decisions[index])
            BankReviewItem(row: row, decision: decision, enabled: enabled) { decisions[index] = $0 }
                .swipeActions(edge: .leading, allowsFullSwipe: true) {
                    if enabled && !row.locked && decision != "new" && row.options.contains(where: { $0.id == "new" }) {
                        Button { decisions[index] = "new" } label: { Label("Add", systemImage: "plus.circle") }.tint(.urPositive)
                    }
                }
                .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                    if enabled && !row.locked && decision != "skip" && row.options.contains(where: { $0.id == "skip" }) {
                        Button { decisions[index] = "skip" } label: { Label("Skip", systemImage: "forward") }.tint(.gray)
                    }
                }
        }
    }
}

/// The rows section's header: how many rows, and a shortcut for every row still to decide.
struct BankReviewHeader: View {
    var rows: [BankReviewRow]
    @Binding var decisions: [Int: String]
    var undecided: Int
    var enabled: Bool

    var body: some View {
        HStack {
            Text(rows.count == 1 ? "1 row" : "\(rows.count) rows")
            Spacer()
            if undecided > 0 && enabled {
                Menu {
                    let suggested = rows.indices.filter { rows[$0].transfer != nil && rows[$0].decision(decisions[$0]) == "review" }
                    if !suggested.isEmpty {
                        Button {
                            for index in suggested { decisions[index] = "match:\(rows[index].transfer!)" }
                        } label: {
                            Label(suggested.count == 1 ? "Pair the suggested transfer" : "Pair \(suggested.count) suggested transfers",
                                  systemImage: "arrow.left.arrow.right")
                        }
                    }
                    Button { decide("new") } label: { Label("Add them as new", systemImage: "plus.circle") }
                    Button { decide("skip") } label: { Label("Skip them", systemImage: "forward") }
                } label: {
                    Text(undecided == 1 ? "Decide 1 row" : "Decide \(undecided) rows").font(.footnote.weight(.semibold)).textCase(nil)
                }
            }
        }
    }

    private func decide(_ choice: String) {
        for (index, row) in rows.enumerated() where row.decision(decisions[index]) == "review" && row.options.contains(where: { $0.id == choice }) {
            decisions[index] = choice
        }
    }
}

/// What the review says before its rows: files without bank IDs, and rows still to decide.
struct BankReviewNotes: View {
    var summary: BankReviewSummary
    var undecided: Int

    var body: some View {
        if summary.transfers {
            Notice(text: "A transfer joins a row to the uncategorized entry it mirrors in another account, so the money you moved isn't counted as spending or income. It's chosen for you when the bank's text names the other account or matches earlier transfers between them; otherwise it's suggested for you to check.",
                   tone: "info", icon: "arrow.left.arrow.right")
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
        if summary.idless {
            Notice(text: "This file has no bank transaction IDs. A row may already be imported; compare its memo and amount. Skip it if it's the same payment, or add it separately if it's a different one.",
                   tone: "info", icon: "info.circle.fill")
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
        if undecided > 0 {
            Notice(text: undecided == 1 ? "1 row needs a choice before importing." : "\(undecided) rows need a choice before importing.",
                   icon: "exclamationmark.triangle.fill")
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
    }
}
