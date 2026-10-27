import Foundation

/// One choice for a statement row: add it, skip it, or match an existing transaction.
struct AssistantBankOption: Equatable, Sendable {
    var value: String
    var label: String
}

/// A statement row ready to review, built off the main thread as Android's BankReviewRow. A
/// `locked` row is already imported and has nothing to decide; "review" is a decision still to
/// make, and the import waits until no row has it.
struct AssistantBankRow: Identifiable, Equatable, Sendable {
    var id: String
    var source: JSON
    var options: [AssistantBankOption]
    var initial: String
    var manualImportId: String
    var previouslyMatched: Set<String>
    var locked = false
    var payee = ""
    var bankText: String?
    var amount: Int64 = 0
    var amountText = ""
    var duplicate = false
    var possibleDuplicate = false
    var idless = false
    var detail = ""

    func decision(_ chosen: String?) -> String { locked ? "skip" : chosen ?? initial }
}

/// The review's figures: rows already imported, rows a payee rule renamed, and what a note should say.
struct AssistantBankSummary: Equatable, Sendable {
    var rows: Int
    var duplicates: Int
    var renamed: Int
    var idless: Bool
    var allIn: Bool

    /// "12 rows · 3 already imported · 2 renamed by your payee rules".
    var text: String {
        var out = String(localized: "\(rows) rows", locale: .app)
        if duplicates > 0 { out += " · " + String(localized: "\(duplicates) already imported", locale: .app) }
        if renamed > 0 { out += " · " + String(localized: "\(renamed) renamed by your payee rules", locale: .app) }
        return out
    }
}

/// The bank-file review's logic (Android's EditorPreparation), for the statements the assistant converts.
enum AssistantBankReview {
    /// Desktop's defaultChoice: skip known bank IDs; take a single match only when that account side was not imported before.
    static func defaultChoice(duplicate: Bool, possibleDuplicate: Bool, candidates: [String], previouslyMatched: Set<String>) -> String {
        if duplicate { return "skip" }
        if possibleDuplicate { return "review" }
        if candidates.isEmpty { return "new" }
        if candidates.count == 1 && !previouslyMatched.contains(candidates[0]) { return "match:\(candidates[0])" }
        return "review"
    }

    private struct Pool {
        var matches: [AssistantBankOption]
        var candidates: [String]
        var previouslyMatched: Set<String>
        var choose: [AssistantBankOption] {
            let review = AssistantBankOption(value: "review", label: String(localized: "Choose a match or add it"))
            return [review, Self.add, Self.skip] + matches
        }
        var possible: [AssistantBankOption] {
            let review = AssistantBankOption(value: "review", label: String(localized: "May already be imported — choose"))
            return [review, Self.add, Self.skip] + matches
        }
        static var add: AssistantBankOption { AssistantBankOption(value: "new", label: String(localized: "Add as a new transaction")) }
        static var skip: AssistantBankOption { AssistantBankOption(value: "skip", label: String(localized: "Skip")) }
    }

    private static let alreadyImported = [AssistantBankOption(value: "skip", label: String(localized: "Already imported — skip")),
                                          AssistantBankOption(value: "new", label: String(localized: "Add as a separate transaction"))]

    /// Rows from the engine's `bankPreview`, whose candidates are a shared pool (`candidateSets`).
    static func prepare(_ rows: [JSON], budget: JSON, candidateSets: [JSON], allowIdlessDuplicate: Bool = true) throws -> [AssistantBankRow] {
        let currency = budget.text("currency", "SAR")
        let transactions = Dictionary(budget.rows("transactions").map { ($0.text("id"), $0) }, uniquingKeysWith: { first, _ in first })
        var pools: [Int: Pool] = [:]
        func pool(_ source: JSON) -> Pool {
            let details = Dictionary(source.rows("candidateDetails").map { ($0.text("id"), $0) }, uniquingKeysWith: { first, _ in first })
            let previously = Set(source["previouslyMatchedCandidates"].array.map(\.string))
            let ids = source["candidates"].array.map(\.string)
            let matches = ids.map { id -> AssistantBankOption in
                let detail = details[id]
                let transactionId = detail?.text("transactionId") ?? ""
                let saved = transactions[transactionId.isEmpty ? id : transactionId]
                return AssistantBankOption(value: "match:\(id)", label: matchLabel(id, saved, detail, previously.contains(id)))
            }
            return Pool(matches: matches, candidates: ids, previouslyMatched: previously)
        }
        return try rows.enumerated().map { index, row in
            let set: Pool
            if row.has("candidateSet") {
                let number = Int(row.long("candidateSet"))
                if let known = pools[number] {
                    set = known
                } else {
                    guard number >= 0, number < candidateSets.count else { throw AppError(String(localized: "Bank match details are missing. Preview again.")) }
                    set = pool(candidateSets[number])
                    pools[number] = set
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
            let rename = row.text("rename")
            let renamed = !rename.trimmingCharacters(in: .whitespaces).isEmpty && rename != row.text("payee")
            let payee = renamed ? rename : row.text("payee")
            return AssistantBankRow(
                id: "\(index):\(row.text("id"))", source: small(row), options: options,
                initial: defaultChoice(duplicate: duplicate, possibleDuplicate: possible, candidates: set.candidates, previouslyMatched: set.previouslyMatched),
                manualImportId: "bank:manual:\(UUID().uuidString.lowercased())", previouslyMatched: set.previouslyMatched,
                locked: duplicate && !separate, payee: payee.trimmingCharacters(in: .whitespaces).isEmpty ? String(localized: "No payee") : payee,
                bankText: renamed ? row.text("payee") : nil, amount: row.long("amount"), amountText: money(row.long("amount"), currency),
                duplicate: duplicate, possibleDuplicate: possible, idless: idless,
                detail: [Money.date(row.text("date")), row.text("memo")].filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }.joined(separator: " · "))
        }
    }

    /// "Match: 3 Oct 2026 · Shop · Split 2 · memo · Previously imported", as the desktop labels a match.
    private static func matchLabel(_ id: String, _ saved: JSON?, _ detail: JSON?, _ previously: Bool) -> String {
        guard let saved else {
            let parts = previously ? [id, String(localized: "Previously imported")] : [id]
            return String(localized: "Match: \(parts.joined(separator: " · "))")
        }
        let splits = saved.rows("splits")
        let splitIndex = detail.map { Int($0.long("splitIndex")) }
        let part = splitIndex.flatMap { $0 >= 0 && $0 < splits.count ? splits[$0] : nil }
        let memo = part.map { $0.text("memo") }.flatMap { $0.trimmingCharacters(in: .whitespaces).isEmpty ? nil : $0 } ?? saved.text("memo")
        var parts = [Money.date(saved.text("date")), saved.text("payee").isEmpty ? String(localized: "Transfer") : saved.text("payee")]
        if let splitIndex, detail != nil { parts.append(String(localized: "Split \(splitIndex + 1)", locale: .app)) }
        if !memo.trimmingCharacters(in: .whitespaces).isEmpty { parts.append(memo) }
        if previously { parts.append(String(localized: "Previously imported")) }
        return String(localized: "Match: \(parts.joined(separator: " · "))")
    }

    /// The row without its candidate pool: matching revalidates against the current ledger.
    private static func small(_ source: JSON) -> JSON {
        var row = source
        for key in ["candidateSet", "candidates", "candidateDetails", "previouslyMatchedCandidates"] { row.remove(key) }
        return row
    }

    static func summary(_ rows: [AssistantBankRow]) -> AssistantBankSummary {
        AssistantBankSummary(rows: rows.count, duplicates: rows.count(where: \.duplicate),
                             renamed: rows.count { !$0.duplicate && $0.bankText != nil },
                             idless: rows.contains { $0.possibleDuplicate || ($0.idless && $0.duplicate) },
                             allIn: rows.count >= 3 && rows.allSatisfy { $0.amount > 0 })
    }

    /// How many rows a review imports or matches, and how many still need a choice.
    static func counts(_ rows: [AssistantBankRow], _ chosen: [Int: String]) -> (importing: Int, undecided: Int) {
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

    /// The engine's `bankApply` choices. A row nobody decided on is never imported or matched.
    static func choices(_ rows: [AssistantBankRow], _ overrides: [Int: String], allowIdlessDuplicate: Bool = true) -> [JSON] {
        rows.enumerated().map { index, prepared in
            var decision = prepared.decision(overrides[index])
            if decision == "review" { decision = "skip" }
            var row = prepared.source
            if allowIdlessDuplicate && decision == "new" && prepared.source.flag("idless")
                && (prepared.source.flag("duplicate") || prepared.source.flag("possibleDuplicate")) {
                row["importId"] = .string(prepared.manualImportId)
            }
            let matchId = decision.hasPrefix("match:") ? String(decision.dropFirst(6)) : nil
            if let matchId, prepared.previouslyMatched.contains(matchId) { row["previouslyMatchedCandidates"] = [.string(matchId)] }
            return ["row": row, "action": .string(matchId == nil ? decision : "match"), "matchId": matchId.map(JSON.string) ?? .null]
        }
    }
}
