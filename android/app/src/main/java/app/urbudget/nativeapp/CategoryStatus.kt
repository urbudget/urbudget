package app.urbudget.nativeapp

import org.json.JSONObject
import java.time.LocalDate

/**
 * What a category needs this month, as YNAB shows it (desktop/src/CategoryRow.tsx categoryState):
 * the status picks the Available pill's colour, the progress fills the target bar.
 */
data class CategoryStatus(
    val needed: Long, val eventually: Long, val snoozed: Boolean, val credit: Boolean,
    /** 0–100 when the category has a target. */
    val progress: Int?,
    /** credit, negative, underfunded, positive or zero. */
    val status: String,
)

/** A target in words: an amount that leads the sentence (if any) and the rest. */
data class TargetWords(val amount: Long?, val words: String) {
    /** The whole line, with the amount written as [amountText] where the language puts it. */
    fun text(amountText: String): String = if (amount == null) words else Words[R.string.target_amount_words, amountText, words]
}

object CategoryStatuses {
    private fun JSONObject.long(key: String) = if (isNull(key)) 0L else optLong(key)

    fun of(category: JSONObject, summary: JSONObject?, month: String): CategoryStatus {
        val s = summary ?: JSONObject()
        val goal = category.optJSONObject("goal")
        val snoozed = goal?.let { if (it.isNull("goal_snoozed_at")) null else it.optString("goal_snoozed_at") }?.startsWith(month) == true
        val balanceTarget = goal?.optString("goal_type") == "TB"
        val available = s.long("available")
        val target = category.long("target")
        val commitments = maxOf(0L, -available, s.long("upcoming") - available)
        // Snoozing suppresses the target, not scheduled payments or money already overspent. An
        // undated balance asks for nothing this month: what it lacks is `eventually`.
        val needed = if (snoozed || balanceTarget) commitments else maxOf(commitments, if (s.has("needed")) s.long("needed") else target - s.long("assigned"))
        val creditOverspending = s.long("creditOverspending")
        val credit = creditOverspending > 0 && creditOverspending >= -available
        val required = s.long("targetFundingRequired")
        val funded = s.long("targetFundingFunded")
        val eventually = if (balanceTarget && !snoozed) maxOf(0L, required - maxOf(0L, funded)) else 0L
        val progress = if (target > 0) {
            if (required > 0) (funded.toDouble() / required * 100).coerceIn(0.0, 100.0).toInt() else if (needed > 0) 0 else 100
        } else null
        val status = when {
            credit -> "credit"
            available < 0 -> "negative"
            needed > 0 -> "underfunded"
            available > 0 -> "positive"
            else -> "zero"
        }
        return CategoryStatus(needed, eventually, snoozed, credit, progress, status)
    }

    /**
     * "120.00 needed by the 31st", "20.00 more needed by 1 October", "45.00 more needed this month",
     * "2,000.00 more needed eventually", or "Funded" / "Fully spent" once the month's target is met.
     */
    fun targetWords(category: JSONObject, summary: JSONObject?, state: CategoryStatus, monthFirst: Boolean): TargetWords? {
        val s = summary ?: JSONObject()
        val available = s.long("available")
        if (state.status == "underfunded") {
            val due = if (state.snoozed) null else s.optJSONObject("targetDue")
            val more = available > 0 || (if (s.has("targetFundingFunded")) s.long("targetFundingFunded") else s.long("assigned")) > 0
            val words = when {
                due == null -> Words[if (more) R.string.target_more_needed_this_month else R.string.target_needed_this_month]
                due.optBoolean("monthly") -> Words[if (more) R.string.target_more_needed_by_day else R.string.target_needed_by_day,
                    ordinal(due.optString("date").drop(8).take(2).toIntOrNull() ?: 1)]
                else -> Words[if (more) R.string.target_more_needed_by_date else R.string.target_needed_by_date, deadline(due.optString("date"), monthFirst)]
            }
            return TargetWords(state.needed, words)
        }
        if (state.status == "negative" || state.status == "credit") return null
        if (state.eventually > 0) return TargetWords(state.eventually, Words[if (available > 0) R.string.target_more_needed_eventually else R.string.target_needed_eventually])
        if (category.long("target") <= 0 || state.snoozed || (state.progress ?: 0) < 100) return null
        if (available > 0) return TargetWords(null, Words[R.string.target_funded])
        return if (s.long("activity") < 0) TargetWords(null, Words[R.string.target_fully_spent]) else null
    }

    /** A day of the month as an ordinal: "31st" in English; Arabic writes the number alone. */
    fun ordinal(n: Int): String {
        val form = if (n % 100 in 11..13) R.string.ordinal_other else when (n % 10) {
            1 -> R.string.ordinal_ends_1; 2 -> R.string.ordinal_ends_2; 3 -> R.string.ordinal_ends_3; else -> R.string.ordinal_other
        }
        return Words[form, n.toString()]
    }

    /** "2026-10-01" in a sentence: "October 1st" when dates go month first, else "1 October", in the interface's language. */
    fun deadline(iso: String, monthFirst: Boolean): String = runCatching {
        val date = LocalDate.parse(iso)
        val month = date.format(Money.pattern("MMMM"))
        if (monthFirst) Words[R.string.deadline_month_first, month, ordinal(date.dayOfMonth)] else Words[R.string.deadline_day_first, date.dayOfMonth.toString(), month]
    }.getOrDefault(iso)
}
