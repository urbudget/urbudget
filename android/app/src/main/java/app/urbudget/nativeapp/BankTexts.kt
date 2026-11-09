package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.row
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import org.json.JSONObject
import java.math.BigDecimal

/**
 * What became of a text a review settled, kept with it once its body is gone ([SmsInbox.finish]):
 * the assistant's search says it, and the bank's latest balances come from it.
 */
internal object SmsOutcome {
    const val Added = "added"
    const val Matched = "matched"
    const val Skipped = "skipped"
    const val Cleared = "cleared"
    const val Dismissed = "dismissed"
    const val Discarded = "discarded"
}

/** One outcome: what happened, and what the text was read as (date, account, amount, payee, the balance or credit left it gave). */
internal fun smsOutcome(item: SmsItem, outcome: String, transactionId: String? = null, at: Long = System.currentTimeMillis()): JSONObject =
    JSONObject().put("outcome", outcome).put("at", at).put("received", item.sources.maxOf { it.received }).apply {
        if (outcome == SmsOutcome.Skipped) put("reason", item.skip.ifBlank { "other" })
        transactionId?.let { put("transactionId", it) }
        item.transaction?.let { t ->
            put("date", t.text("date")); put("payee", t.text("payee"))
            t.text("accountId").ifBlank { null }?.let { put("accountId", it) }
            item.amount?.let { put("amount", it) }
            smsBalanceCents(t.text("balance"))?.let { put("balance", it) }
            smsBalanceCents(t.text("available"))?.let { put("available", it) }
        }
    }

/**
 * Each settled item's outcome, for every text it was read from: added or matched by the review's
 * decision ([adding]: "new" or "match:id"), skipped when it isn't money, dismissed, or else cleared
 * (left unticked).
 */
internal fun smsOutcomes(settled: List<SmsItem>, adding: List<Pair<Int, String>>, dismissed: Set<Int>, at: Long = System.currentTimeMillis()): Map<String, JSONObject> {
    val decided = adding.toMap()
    return settled.flatMap { item ->
        val decision = decided[item.index]
        val outcome = when {
            decision == "new" -> smsOutcome(item, SmsOutcome.Added, at = at)
            decision?.startsWith("match:") == true -> smsOutcome(item, SmsOutcome.Matched, decision.removePrefix("match:"), at)
            item.disposition == "ignore" -> smsOutcome(item, SmsOutcome.Skipped, at = at)
            item.index in dismissed -> smsOutcome(item, SmsOutcome.Dismissed, at = at)
            else -> smsOutcome(item, SmsOutcome.Cleared, at = at)
        }
        item.sources.map { it.id to outcome }
    }.toMap()
}

/** Outcomes with nothing read: texts dismissed or discarded from Bank messages without a reading. */
internal fun smsPlainOutcomes(ids: List<String>, outcome: String, at: Long = System.currentTimeMillis()): Map<String, JSONObject> =
    ids.associateWith { JSONObject().put("outcome", outcome).put("at", at) }

/**
 * The bank's latest figure for each account, kept in `sms_state` as `"balances"`: [saved] updated
 * from texts that were added or matched. A text gives a balance, or for a card its credit left
 * (`available`), which is kept in preference. Each is kept with when it came, so an older text
 * never replaces a newer one.
 */
internal fun smsKeepBalances(saved: String?, outcomes: Collection<JSONObject>): String? {
    val kept = runCatching { JSONObject(saved ?: "{}") }.getOrDefault(JSONObject())
    var changed = false
    outcomes.filter { it.optString("outcome") in setOf(SmsOutcome.Added, SmsOutcome.Matched) }.forEach { o ->
        val account = o.optString("accountId").ifBlank { null } ?: return@forEach
        val (kind, cents) = when {
            o.has("available") -> "available" to o.getLong("available")
            o.has("balance") -> "balance" to o.getLong("balance")
            else -> return@forEach
        }
        val received = o.optLong("received")
        if ((kept.optJSONObject(account)?.optLong("received") ?: Long.MIN_VALUE) > received) return@forEach
        kept.put(account, JSONObject().put("kind", kind).put("cents", cents).put("received", received).put("date", o.optString("date")))
        changed = true
    }
    return if (changed) kept.toString() else saved
}

/** A credit account's limit in cents (`creditLimit`), when it has one. */
internal fun smsCreditLimit(account: JSONObject?): Long? =
    account?.takeIf { it.text("kind") == "credit" }?.optLong("creditLimit")?.takeIf { it > 0 }

/**
 * The bank's figure for an account, as the budget writes its balance: a bank account's balance, or
 * for a card the credit left less its limit (negative when money is owed or held). A card's
 * "balance" is its credit left, as most banks text it (ANB's "Balance" falls as purchases come),
 * unless [owes]: its texts show it's what's owed ([smsCardOwes]). Null when it can't be told: a
 * card's credit left with no limit to read it against.
 */
internal fun smsBankFigure(account: JSONObject?, kind: String, cents: Long, owes: Boolean = false): Long? = when {
    account == null -> null
    account.text("kind") == "credit" -> if (kind == "balance" && owes) -cents else smsCreditLimit(account)?.let { cents - it }
    kind == "balance" -> cents
    else -> null
}

/**
 * Whether a card's texts give what's owed rather than the credit left, from how their "balance"
 * moves from one text to the next: a purchase lowers the credit left and raises what's owed. The
 * credit left, as most banks text it, until more pairs of texts say otherwise.
 */
internal fun smsCardOwes(items: List<SmsItem>, account: String): Boolean {
    val texts = items.filter { item ->
        val t = item.transaction
        t?.text("accountId") == account && item.amount != null && smsBalanceCents(t.text("balance")) != null && smsBalanceCents(t.text("available")) == null
    }.sortedWith(compareBy<SmsItem> { it.sources.maxOf { s -> s.received } }.thenBy { it.index })
    var left = 0
    var owed = 0
    texts.zipWithNext { before, after ->
        val moved = smsBalanceCents(after.transaction!!.text("balance"))!! - smsBalanceCents(before.transaction!!.text("balance"))!!
        if (moved == after.amount) left++ else if (moved == -after.amount!!) owed++
    }
    return owed > left
}

/** What a transaction does to [account]: its amount there, or the opposite for the other side of a transfer. */
private fun effect(accountId: String, transferAccountId: String, amount: Long, account: String): Long =
    (if (accountId == account) amount else 0L) - (if (transferAccountId == account) amount else 0L)

/** An account's balance in the budget at the end of [day]: [now] (every transaction counted) less what's dated after it. */
internal fun smsBudgetBalanceAt(budget: JSONObject, now: Long, account: String, day: String): Long =
    now - budget.rows("transactions").sumOf { t ->
        if (t.text("date") <= day) 0L else effect(t.text("accountId"), t.text("transferAccountId"), t.optLong("amount"), account)
    }

/** How the bank's figure in the review's latest text that gives one compares with the budget's at that text. */
internal data class SmsBankCheck(val bank: Long, val budget: Long, val date: String, val held: Long) {
    val difference: Long get() = bank - budget
}

/**
 * The check beside **Balances after adding** for [account]: the bank's figure in the latest text
 * about it that gives one, against the budget at that text. The budget's side is what it holds
 * dated through that text's day ([now] is its balance with everything), plus what the review
 * adds ([adding]'s new rows) received up to that text. Holds left unticked ([held]) are already
 * in a bank's available figure, so a difference that equals them is explained. Null when no text
 * in the review gives a figure the account can be checked against.
 */
internal fun smsBankCheck(account: String, budget: JSONObject, now: Long, items: List<SmsItem>, adding: List<Pair<Int, String>>, ticked: (SmsItem) -> Boolean): SmsBankCheck? {
    val record = budget.row("accounts", account) ?: return null
    val owes = record.text("kind") == "credit" && smsCardOwes(items, account)
    fun at(item: SmsItem) = item.sources.maxOf { it.received }
    fun change(item: SmsItem): Long {
        val t = item.transaction ?: return 0L
        return effect(t.text("accountId"), t.text("transferAccountId"), item.amount ?: return 0L, account)
    }
    val (alert, figure) = items.filter { it.disposition in setOf("transaction", "pending") && it.transaction?.text("accountId") == account }
        .mapNotNull { item ->
            val t = item.transaction!!
            val figure = smsBalanceCents(t.text("available"))?.let { smsBankFigure(record, "available", it) }
                ?: smsBalanceCents(t.text("balance"))?.let { smsBankFigure(record, "balance", it, owes) }
            figure?.let { item to it }
        }
        .maxWithOrNull(compareBy<Pair<SmsItem, Long>> { at(it.first) }.thenBy { it.first.index }) ?: return null
    val time = at(alert)
    val day = alert.transaction!!.text("date")
    val news = adding.filter { it.second == "new" }.mapTo(HashSet()) { it.first }
    val added = items.filter { it.index in news && at(it) <= time }.sumOf(::change)
    val held = items.filter { it.disposition == "pending" && !ticked(it) && at(it) <= time }.sumOf(::change)
    return SmsBankCheck(figure, smsBudgetBalanceAt(budget, now, account, day) + added, day, held)
}

/**
 * The amounts a text writes, in cents, read every way a number can be written: "1,234.50",
 * "1.234,50", "1234.5", "12.500" and Arabic-Indic digits. A search by amount matches any of them.
 */
internal fun smsWrittenAmounts(body: String): Set<Long> {
    val western = buildString(body.length) {
        body.forEach { c ->
            append(when (c) {
                in '٠'..'٩' -> '0' + (c - '٠')
                in '۰'..'۹' -> '0' + (c - '۰')
                '٫' -> '.'
                '٬' -> ','
                else -> c
            })
        }
    }
    val found = HashSet<Long>()
    fun add(number: String) {
        runCatching { BigDecimal(number).movePointRight(2).longValueExact() }.getOrNull()?.takeIf { it in 0..1_000_000_000_000L }?.let(found::add)
    }
    Regex("[0-9][0-9.,']*").findAll(western).forEach { match ->
        val token = match.value.trimEnd('.', ',', '\'')
        val digits = token.filter(Char::isDigit)
        if (digits.isEmpty() || digits.length > 15) return@forEach
        // Every separator a thousands mark: a whole amount.
        add(digits)
        // The last separator a decimal point, the others thousands marks.
        val last = token.indexOfLast { it == '.' || it == ',' }
        if (last >= 0) {
            val fraction = token.substring(last + 1).filter(Char::isDigit)
            val whole = token.substring(0, last).filter(Char::isDigit).ifEmpty { "0" }
            if (fraction.length in 1..3) add("$whole.$fraction")
        }
    }
    return found
}

/** Whether [body] has every one of [words] (any case), and when [amount] is given, writes it. */
internal fun smsTextMatches(body: String, words: String, amount: Long?): Boolean {
    val lower = body.lowercase()
    if (words.split(Regex("\\s+")).filter { it.isNotBlank() }.any { it.lowercase() !in lower }) return false
    return amount == null || amount in smsWrittenAmounts(body)
}
