import Foundation

/// What a category needs this month, as YNAB shows it (desktop/src/CategoryRow.tsx categoryState):
/// the status picks the Available pill's colour, the progress fills the target bar.
struct CategoryStatus: Equatable, Sendable {
    var needed: Int64
    var eventually: Int64
    var snoozed: Bool
    var credit: Bool
    /// 0–100 when the category has a target.
    var progress: Int?
    /// credit, negative, underfunded, positive or zero.
    var status: String

    /// A target in words: an amount that leads the sentence (if any) and the rest.
    struct Words: Equatable { var amount: Int64?; var words: String }

    init(category: JSON, summary: JSON?, month: String) {
        let s = summary ?? [:]
        let goal = category["goal"]
        snoozed = goal.isObject && !goal["goal_snoozed_at"].isNull && goal.text("goal_snoozed_at").hasPrefix(month)
        let balanceTarget = goal.text("goal_type") == "TB"
        let available = s.long("available")
        let target = category.long("target")
        let commitments = max(0, -available, s.long("upcoming") - available)
        // Snoozing suppresses the target, not scheduled payments or money already overspent. An
        // undated balance asks for nothing this month: what it lacks is `eventually`.
        needed = snoozed || balanceTarget ? commitments
            : max(commitments, s.has("needed") ? s.long("needed") : target - s.long("assigned"))
        let creditOverspending = s.long("creditOverspending")
        credit = creditOverspending > 0 && creditOverspending >= -available
        let required = s.long("targetFundingRequired")
        let funded = s.long("targetFundingFunded")
        eventually = balanceTarget && !snoozed ? max(0, required - max(0, funded)) : 0
        if target > 0 {
            if required > 0 { progress = Int(min(100, max(0, Double(funded) / Double(required) * 100))) }
            else { progress = needed > 0 ? 0 : 100 }
        } else {
            progress = nil
        }
        if credit { status = "credit" }
        else if available < 0 { status = "negative" }
        else if needed > 0 { status = "underfunded" }
        else if available > 0 { status = "positive" }
        else { status = "zero" }
    }

    /// "120.00 needed by the 31st", "20.00 more needed by 1 October", "45.00 more needed this month",
    /// "2,000.00 more needed eventually", or "Funded" / "Fully spent" once the month's target is met.
    func targetWords(category: JSON, summary: JSON?, monthFirst: Bool) -> Words? {
        let s = summary ?? [:]
        let available = s.long("available")
        if status == "underfunded" {
            let due = snoozed ? JSON.null : s["targetDue"]
            let more = available > 0 || (s.has("targetFundingFunded") ? s.long("targetFundingFunded") : s.long("assigned")) > 0
            return Words(amount: needed, words: Self.neededWords(due: due, more: more, monthFirst: monthFirst))
        }
        if status == "negative" || status == "credit" { return nil }
        if eventually > 0 {
            return Words(amount: eventually, words: available > 0 ? String(localized: "more needed eventually") : String(localized: "needed eventually"))
        }
        if category.long("target") <= 0 || snoozed || (progress ?? 0) < 100 { return nil }
        if available > 0 { return Words(amount: nil, words: String(localized: "Funded")) }
        return s.long("activity") < 0 ? Words(amount: nil, words: String(localized: "Fully spent")) : nil
    }

    /// "needed this month", "more needed by the 31st", "needed by 1 October": the words after the amount.
    private static func neededWords(due: JSON, more: Bool, monthFirst: Bool) -> String {
        if !due.isObject {
            return more ? String(localized: "more needed this month") : String(localized: "needed this month")
        }
        if due["monthly"].bool {
            let day = Dates.ordinal(Int(due.text("date").dropFirst(8).prefix(2)) ?? 1)
            return more ? String(localized: "more needed by the \(day)") : String(localized: "needed by the \(day)")
        }
        let date = Dates.deadline(due.text("date"), monthFirst: monthFirst)
        return more ? String(localized: "more needed by \(date)") : String(localized: "needed by \(date)")
    }
}
