package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import java.math.BigDecimal
import java.math.RoundingMode
import java.time.LocalDate

/*
 * Investment accounts that mirror a broker (crates/urbudget-core/src/investments.rs; the desktop's
 * Investments.tsx). What an account holds is in the budget's `investments`, one entry per account
 * id, so a phone without the broker's token shows what another device brought in. The account's
 * balance follows the broker's value through transactions the engine writes: a starting value and
 * a market change a month, marked `investment`. Nothing here works a figure out again.
 */
/**
 * An account a deposit could be added as a transfer with, and the day of its first transaction
 * (none when it has none).
 */
internal data class OtherAccount(val id: String, val name: String, val since: String?) {
    /**
     * Whether its history reaches back to [date]: a transaction on or before that day. One that
     * starts later, or has none, opened with a balance that already holds the money, so a transfer
     * would take it out twice.
     */
    fun reaches(date: String): Boolean = since != null && since <= date.take(10)
}

internal object Investments {
    const val KEY = "investments"
    /** Where a new Interactive Brokers account goes unless told otherwise: an account of its own. */
    const val NEW = "new"

    /**
     * What to ask Client Portal's "Configure Query with AI" for, in its own words: the sections
     * the reader uses, each with all of its fields (the desktop's FLEX_REQUEST). It takes at most
     * 200 characters, and is English whatever the interface.
     */
    const val FLEX_REQUEST =
        "Activity Flex Query, XML, Last 365 Calendar Days, Currency Rates Yes. All fields of: Account Information, Cash Report, Cash Transactions, NAV in Base, Open Positions (Summary), Trades, Transfers."

    /**
     * Where an account's figures come from, for the line under them: Interactive Brokers' account
     * there, a bank connection, a positions file, or a value typed in (ui/HoldingsSetup.kt brings
     * the last two in; a bank sync brings the bank's).
     */
    fun sourceNote(held: JSONObject): String {
        val date = Money.date(held.optString("asOf"))
        return when (held.optString("source")) {
            "ibkr" -> Words[R.string.ibkr_source_note, Money.ltr(held.optString("sourceAccount"))]
            "simplefin" -> Words[R.string.holdings_note_bank, date]
            "file" -> Words[R.string.holdings_note_file, date]
            else -> Words[R.string.holdings_note_typed, date]
        }
    }

    /** How an account is kept up to date, by where its holdings come from. */
    fun sourceName(held: JSONObject): String = sourceName(held.optString("source"))

    /** The same, by the source's own name ("ibkr", "simplefin", "file", "manual"). */
    fun sourceName(source: String): String = when (source) {
        "ibkr" -> "Interactive Brokers"
        "simplefin" -> Words[R.string.holdings_by_bank]
        "file" -> Words[R.string.holdings_by_file]
        else -> Words[R.string.holdings_by_typed]
    }

    /**
     * The accounts the Investments page lists: the open ones that can hold investments (an
     * investment or retirement account, or a tracking asset with no type of its own), and any
     * other account something already fills.
     */
    fun rows(budget: JSONObject): List<JSONObject> {
        val filled = budget.records(KEY).mapTo(HashSet()) { it.optString("accountId") }
        return budget.records("accounts").filter {
            !it.optBoolean("archived") && (it.optString("id") in filled ||
                (it.optString("kind") == "tracking" && AccountTypes.infer(it) in setOf("investment", "retirement", "otherAsset")))
        }
    }

    /** A plain decimal the engine wrote ("1234.5") in hundredths; nothing reads as zero. */
    fun cents(text: String): Long = text.trim().toBigDecimalOrNull()?.setScale(2, RoundingMode.HALF_UP)?.unscaledValue()?.toLong() ?: 0L

    /** Hundredths as the plain decimal the engine reads ("1234.50"). */
    fun decimal(cents: Long): String = BigDecimal.valueOf(cents, 2).toPlainString()

    /** A currency code as typed, and whether the figures in it need a rate to the budget's. */
    fun foreign(typed: String, budgetCurrency: String): String? = typed.trim().uppercase().takeIf { it.isNotEmpty() && it != budgetCurrency }

    /** The rate given for this account before, when its figures were in `code`: used again unless another is typed. */
    fun rememberedRate(held: JSONObject?, code: String?): String =
        if (held != null && code != null && held.optString("currency") == code && !held.isNull("manualRate")) held.optString("manualRate") else ""

    /**
     * What a positions file says an account holds, to record (investments.rs `Snapshot`): its
     * holdings and cash as the engine read them, the day they're from, and, when they aren't in
     * the budget's currency, that currency with what one unit of it is worth.
     */
    fun fileSnapshot(read: JSONObject, fileName: String, asOf: String, currency: String, budgetCurrency: String, rate: String): JSONObject {
        val code = foreign(currency, budgetCurrency)
        return JSONObject().put("source", "file").put("sourceAccount", fileName).put("asOf", asOf)
            .put("currency", code.orEmpty()).put("rate", if (code == null) "" else rate(rate).orEmpty())
            .put("positions", read.optJSONArray("positions") ?: JSONArray()).put("cash", read.optString("cash"))
    }

    /** An account's whole value on a day, typed in. */
    fun typedSnapshot(cents: Long, asOf: String): JSONObject = JSONObject().put("source", "manual").put("asOf", asOf).put("total", decimal(cents))

    /** Whether Interactive Brokers fills any account of the budget. */
    fun filled(budget: JSONObject?): Boolean = budget?.records(KEY).orEmpty().any { it.optString("source") == "ibkr" }

    /**
     * Whether to sync by itself now: the budget has an account Interactive Brokers fills, the
     * person hasn't turned it off, and it wasn't tried yet today. The broker makes one statement
     * a day, so once is enough, and a failure isn't tried again until tomorrow.
     */
    fun dailyDue(filled: Boolean, on: Boolean, triedOn: String, today: String): Boolean = filled && on && triedOn != today

    /** The day the newest prices among the budget's Interactive Brokers accounts are from, or empty. */
    fun newestPrices(budget: JSONObject?): String =
        budget?.records(KEY).orEmpty().filter { it.optString("source") == "ibkr" }.maxOfOrNull { it.optString("asOf") }.orEmpty()

    /**
     * What a login is called in the list of logins: the accounts its last statement named (by
     * their alias in the statement at hand, else their id), with nothing more to say; or, before
     * it has named any, nothing, so its row is called by its query.
     */
    fun loginName(login: JSONObject, accounts: List<JSONObject>): Pair<String?, String?> {
        val ids = login.optJSONArray("accounts")?.let { list -> (0 until list.length()).map { list.optString(it) } }.orEmpty()
        if (ids.isEmpty()) return null to null
        val names = ids.map { id -> accounts.firstOrNull { it.optString("id") == id }?.optString("alias")?.ifBlank { null } ?: id }
        return names.joinToString(", ") to null
    }

    /** An open investment account nothing fills yet: the one that's offered a way to see what it holds. */
    fun promptable(account: JSONObject, budget: JSONObject): Boolean =
        account.optString("kind") == "tracking" && !account.optBoolean("archived") && AccountTypes.infer(account) == "investment" &&
            of(budget, account.optString("id")) == null

    /** An account's holdings, when a broker fills it. */
    fun of(budget: JSONObject, accountId: String): JSONObject? = budget.records(KEY).firstOrNull { it.optString("accountId") == accountId }

    /** What the account gained or lost beyond the money put in or taken out: its market changes added up. None before the first. */
    fun gain(budget: JSONObject, accountId: String): Long? {
        var total = 0L
        var any = false
        for (t in budget.records("transactions")) {
            if (t.optString("accountId") == accountId && t.optString("investment") == "change") { total += t.optLong("amount"); any = true }
        }
        return if (any) total else null
    }

    /** The first day the broker valued the account: where its gain is counted from. */
    fun since(holdings: JSONObject): String? = holdings.optJSONArray("values")?.optJSONArray(0)?.optString(0)?.ifBlank { null }

    /** Prices more than four days old: past a weekend, the broker has newer ones. */
    fun stale(asOf: String, today: LocalDate = LocalDate.now()): Boolean =
        runCatching { LocalDate.parse(asOf).isBefore(today.minusDays(4)) }.getOrDefault(false)

    /** The budget without an account's holdings: it stops following its broker, and its transactions stay. For an `editBudget`. */
    fun unlink(budget: JSONObject, accountId: String) {
        val kept = budget.records(KEY).filter { it.optString("accountId") != accountId }
        if (kept.isEmpty()) budget.remove(KEY) else budget.put(KEY, JSONArray(kept))
    }

    /** A quantity or price as the broker wrote it, to at most `places` decimals, grouped as the budget writes amounts. */
    fun plain(text: String, places: Int = 4): String {
        val number = text.trim().toBigDecimalOrNull() ?: return text
        val shown = number.abs().setScale(places, RoundingMode.HALF_UP).stripTrailingZeros()
        val parts = (if (shown.signum() == 0) BigDecimal.ZERO else shown).toPlainString().split('.')
        val whole = parts[0].toLongOrNull()?.let(Money::count) ?: parts[0]
        val sign = if (number.signum() < 0 && shown.signum() != 0) "-" else ""
        return sign + whole + (parts.getOrNull(1)?.let { Money.decimalMark() + it } ?: "")
    }

    /** Tracking accounts a broker account could fill: open, not a loan, and not already filled. */
    fun targets(budget: JSONObject): List<JSONObject> {
        val filled = budget.records(KEY).mapTo(HashSet()) { it.optString("accountId") }
        return budget.records("accounts").filter {
            it.optString("kind") == "tracking" && !it.optBoolean("archived") && it.optString("id") !in filled && !AccountTypes.isLoanType(AccountTypes.infer(it))
        }
    }

    /**
     * The budget's other open accounts, the ones this account has moved money with first: where a
     * deposit the broker lists could have come from. Each carries the day of its first transaction
     * (either side of a transfer counts), so a deposit is offered only the accounts whose history
     * reaches back to it.
     */
    fun others(budget: JSONObject, accountId: String): List<OtherAccount> {
        val moved = HashMap<String, Int>()
        val first = HashMap<String, String>()
        for (t in budget.records("transactions")) {
            val from = t.optString("accountId")
            val to = if (t.isNull("transferAccountId")) "" else t.optString("transferAccountId")
            val other = if (from == accountId) to else if (to == accountId) from else ""
            if (other.isNotEmpty()) moved[other] = (moved[other] ?: 0) + 1
            val date = if (t.isNull("date")) "" else t.optString("date").take(10)
            if (date.isNotEmpty()) for (id in listOf(from, to)) if (id.isNotEmpty() && (first[id] ?: date) >= date) first[id] = date
        }
        return budget.records("accounts").filter { it.optString("id") != accountId && !it.optBoolean("archived") }
            .sortedByDescending { moved[it.optString("id")] ?: 0 }
            .map { OtherAccount(it.optString("id"), it.optString("name"), first[it.optString("id")]) }
    }

    /** Rows of an account's review that aren't in the budget yet: its deposits, withdrawals and transfers still to review. */
    fun fresh(review: JSONObject?): Int = review?.records("rows")?.count { !it.optBoolean("duplicate") } ?: 0

    /** A typed exchange rate as the engine reads it ("3,75" is 3.75), or null when it isn't a number above zero. */
    fun rate(typed: String?): String? {
        val text = typed.orEmpty().trim().replace(',', '.')
        return text.takeIf { (it.toBigDecimalOrNull() ?: BigDecimal.ZERO).signum() > 0 }
    }

    private fun needsRate(account: JSONObject) = account.optJSONArray("needsRate") != null
    private fun linked(account: JSONObject) = !account.isNull("accountId") && account.optString("accountId").isNotEmpty()
    private fun choice(account: JSONObject, into: Map<String, String>) = into[account.optString("id")] ?: NEW

    /** The statement's accounts that aren't in the budget and are to be added. */
    fun adding(accounts: List<JSONObject>, into: Map<String, String>): List<JSONObject> = accounts.filter { !linked(it) && choice(it, into).isNotEmpty() }

    /** Whether every account to add has the exchange rate it needs. */
    fun ready(accounts: List<JSONObject>, into: Map<String, String>, rates: Map<String, String>): Boolean =
        adding(accounts, into).all { !needsRate(it) || rate(rates[it.optString("id")]) != null }

    /**
     * What `ibkr_apply` is told: each account to add with where it goes (a new Investment account
     * under a fresh id, or a tracking account of the budget's), and a linked account waiting for
     * its exchange rate.
     */
    fun links(accounts: List<JSONObject>, into: Map<String, String>, rates: Map<String, String>, newId: () -> String): JSONArray {
        val links = JSONArray()
        for (account in adding(accounts, into)) {
            val target = choice(account, into)
            val link = JSONObject().put("source", account.optString("id")).put("account", if (target == NEW) newId() else target).put("create", target == NEW)
            if (needsRate(account)) rate(rates[account.optString("id")])?.let { link.put("rate", it) }
            links.put(link)
        }
        for (account in accounts) {
            val given = rate(rates[account.optString("id")])
            if (linked(account) && needsRate(account) && given != null)
                links.put(JSONObject().put("source", account.optString("id")).put("account", account.optString("accountId")).put("rate", given))
        }
        return links
    }
}
