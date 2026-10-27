package app.urbudget.nativeapp

import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONArray
import org.json.JSONObject
import java.math.BigDecimal
import java.time.LocalDate
import java.time.YearMonth

private fun JSONObject.str(key: String) = if (isNull(key)) "" else optString(key)

/** The Scheduled page's figures, as the desktop's schedules list counts them. */
internal data class ScheduleSummary(val active: Int, val paused: Int, val dueSoon: Int, val goingOut: Long, val comingIn: Long)

/**
 * Schedules still to come, soonest first with paused ones after. A finished schedule has no next
 * date and nothing to list (desktop Register.tsx's upcoming).
 */
internal fun upcomingSchedules(schedules: List<JSONObject>): List<JSONObject> =
    schedules.filter { it.str("nextDate").isNotBlank() }
        .sortedWith(compareBy<JSONObject>({ it.optBoolean("paused") }, { it.str("nextDate") }))

/** How many schedules there are, and what's due in the next 30 days: count, money out, money in. */
internal fun scheduleSummary(upcoming: List<JSONObject>, today: LocalDate): ScheduleSummary {
    val end = today.plusDays(30).toString()
    val paused = upcoming.count { it.optBoolean("paused") }
    val due = upcoming.filter { !it.optBoolean("paused") && it.str("nextDate") <= end }
    val amounts = due.map { it.optJSONObject("transaction")?.optLong("amount") ?: 0L }
    return ScheduleSummary(upcoming.size - paused, paused, due.size, amounts.sumOf { maxOf(0L, -it) }, amounts.sumOf { maxOf(0L, it) })
}

/** Whether a transaction pays money into the loan account (Planning.tsx's paysInto). */
private fun paysInto(t: JSONObject, accountId: String): Boolean {
    if (t.str("transferAccountId") == accountId && t.optLong("amount") < 0) return true
    val splits = t.optJSONArray("splits") ?: JSONArray()
    for (i in 0 until splits.length()) {
        val s = splits.optJSONObject(i) ?: continue
        if (s.str("transferAccountId") == accountId && s.optLong("amount") < 0) return true
    }
    return t.str("accountId") == accountId && t.optLong("amount") > 0 && t.str("transferAccountId").isNotBlank()
}

/**
 * When a loan's next payment falls, for projections: the 1st of next month once this month's
 * payment is in (its interest is charged), else today (Planning.tsx's nextPayment).
 */
internal suspend fun nextLoanPayment(budget: JSONObject, accountId: String, today: String): String {
    val context = currentCoroutineContext()
    val month = today.take(7)
    val transactions = budget.optJSONArray("transactions") ?: JSONArray()
    for (i in 0 until transactions.length()) {
        if (i % 256 == 0) context.ensureActive()
        val t = transactions.optJSONObject(i) ?: continue
        val date = t.str("date")
        if (date.startsWith(month) && date <= today && paysInto(t, accountId)) return "${YearMonth.parse(month).plusMonths(1)}-01"
    }
    return today
}

/**
 * The account a loan payment comes from by default: the one that paid this loan last, else a
 * checking account, then the one with the most in it (LoanPayment.tsx).
 */
internal fun defaultLoanSource(budget: JSONObject, loanId: String, sources: List<JSONObject>, balances: Map<String, Long>): String {
    val ids = sources.map { it.str("id") }.toSet()
    var latest: JSONObject? = null
    for (t in budget.records("transactions")) {
        if (t.str("transferAccountId") == loanId && t.optLong("amount") < 0 && t.str("accountId") in ids && (latest == null || t.str("date") > latest.str("date"))) latest = t
    }
    latest?.let { return it.str("accountId") }
    return sources.sortedWith(compareBy<JSONObject>({ if (AccountTypes.infer(it) == "checking") 0 else 1 }, { -(balances[it.str("id")] ?: 0L) }))
        .firstOrNull()?.str("id").orEmpty()
}

/** "2 years 3 months", in the interface's language. */
internal fun span(months: Int): String {
    val years = months / 12
    val rest = months % 12
    return when {
        years != 0 && rest != 0 -> Words[R.string.span_years_months, Words.count(R.plurals.span_years, years.toLong()), Words.count(R.plurals.span_months, rest.toLong())]
        years != 0 -> Words.count(R.plurals.span_years, years.toLong())
        else -> Words.count(R.plurals.span_months, rest.toLong())
    }
}

/** The annual rate in effect in a month, in thousandths of a percent, as loans.rs reads it. */
internal fun rateIn(terms: JSONObject, month: String): Long {
    val history = terms.optJSONArray("rateHistory") ?: JSONArray()
    var best: JSONObject? = null
    for (i in 0 until history.length()) {
        val rate = history.optJSONObject(i) ?: continue
        if (rate.str("month") <= month && (best == null || rate.str("month") > best.str("month"))) best = rate
    }
    return best?.optLong("annualRateMilliPercent") ?: terms.optLong("annualRateBasisPoints") * 10
}

/** Thousandths of a percent as someone writes the rate: 5125 as "5.125". */
internal fun rateText(milli: Long, decimal: String = "."): String =
    BigDecimal.valueOf(milli, 3).stripTrailingZeros().toPlainString().replace(".", decimal)

/**
 * A dated rate change as typed ("5.125", or "5,125" with a comma decimal mark) in thousandths of a
 * percent: up to three decimals, from 0 to 1,000% (Planning.tsx's parseRateChange).
 */
internal fun parseRateChange(text: String, decimal: String = "."): Long {
    val typed = text.trim().removeSuffix("%").trim().let { if (decimal == ",") it.replace(",", ".") else it }
    require(Regex("^\\d+(\\.\\d{1,3})?$").matches(typed)) { Words[R.string.rate_three_decimals] }
    val value = BigDecimal(typed).movePointRight(3)
    require(value <= BigDecimal(1_000_000)) { Words[R.string.rate_range] }
    return value.longValueExact()
}
