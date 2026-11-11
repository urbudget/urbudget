package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.math.BigDecimal
import java.security.MessageDigest
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.util.Locale

/** Orders draft writes, acknowledgements and reloads so an older save cannot resurrect a draft. */
internal val smsStorageLane = Dispatchers.IO.limitedParallelism(1)

internal data class BankSms(val id: String, val sender: String, val received: Long, val body: String) {
    val receivedDate: String get() = Instant.ofEpochMilli(received).atZone(ZoneId.systemDefault()).toLocalDate().toString()
    /** "20:31": with the date, it tells apart a day's alerts. */
    val receivedTime: String get() = Instant.ofEpochMilli(received).atZone(ZoneId.systemDefault()).toLocalTime().format(java.time.format.DateTimeFormatter.ofPattern("HH:mm", Locale.ROOT))
    fun json(): JSONObject = JSONObject().put("id", id).put("sender", sender).put("body", body).put("receivedDate", receivedDate).put("receivedTime", receivedTime)
}
internal fun smsSenderKey(sender: String): String {
    val text = sender.trim()
    return if (text.matches(Regex("[+0-9 ()-]+"))) text.replace(Regex("[ ()-]"), "") else text.lowercase(Locale.ROOT)
}
internal fun smsId(sender: String, sent: Long, body: String): String = "sms:" + MessageDigest.getInstance("SHA-256")
    .digest("${smsSenderKey(sender)}\u0000$sent\u0000${body.replace("\r\n", "\n")}".toByteArray(Charsets.UTF_8))
    .joinToString("") { "%02x".format(it) }

/** A person's number rather than a bank's name or short code: listed apart, and never previewed. */
internal fun smsPersonal(sender: String): Boolean = sender.trim().matches(Regex("[+0-9 ()-]+")) && sender.count { it in '0'..'9' } >= 8

/** Exact user-edited money validation, never interpretation of an SMS. */
internal fun smsCents(text: String): Long? = runCatching {
    // A third decimal that's 0 ("12.500", as a currency with three writes it) is exact.
    require(text.matches(Regex("-?[0-9]+(?:\\.[0-9]{1,2}0*)?")))
    BigDecimal(text).movePointRight(2).longValueExact().also { require(it != 0L && it in -1_000_000_000_000L..1_000_000_000_000L) }
}.getOrNull()
/** Cents as the exact decimal the core reads back ("-12.30"). */
internal fun smsDecimal(cents: Long): String = BigDecimal.valueOf(cents, 2).toPlainString()

/** A text's digits as 0–9, in whatever script it writes them ("٤٥٦٧" is "4567"). */
internal fun smsDigits(text: String): String = text.mapNotNull { c -> Character.digit(c, 10).takeIf { it >= 0 } }.joinToString("")
/** The digits that name a card or account ("***0091" is "0091"), or its words when it has too few. */
internal fun smsRefKey(ref: String): String = smsDigits(ref).takeIf { it.length >= 3 } ?: ref.trim().lowercase(Locale.ROOT)
/** A sender's card or account by its digits: one choice answers all its texts. */
internal fun smsCardKey(sender: String, ref: String) = smsSenderKey(sender) + "\u0000" + smsRefKey(ref)

/**
 * How an account was found: chosen in the review, remembered, the one whose name holds the text's
 * digits, or the only one. A transfer's other account may also be the other side of the transfer
 * the budget already has ([Budget]), the account such money usually moves with ([Usual]), or the
 * one the assistant named from what the text says it is ([Assistant]); those two are worth a look.
 */
internal enum class SmsAccountSource { Chosen, Remembered, Name, Only, Budget, Usual, Assistant }

/** A transfer in the budget as one of its accounts sees it: the other account, the amount on this side, its day, and whether a bank text matched this side. */
internal data class SmsTransferSide(val account: String, val other: String, val amount: Long, val date: String, val matched: Boolean = false)

/** Every transfer in the budget from both its sides, split parts included. */
internal fun smsTransferSides(budget: JSONObject): List<SmsTransferSide> = budget.rows("transactions").flatMap { t ->
    val account = t.text("accountId")
    val matched = t.optJSONObject("bankImportMatches")
    val parts = if (t.text("transferAccountId").isNotBlank()) listOf(t.text("transferAccountId") to t.optLong("amount"))
        else t.rows("splits").filter { it.text("transferAccountId").isNotBlank() }.map { it.text("transferAccountId") to it.optLong("amount") }
    parts.filter { (other, amount) -> other != account && amount != 0L && account.isNotBlank() }.flatMap { (other, amount) ->
        listOf(SmsTransferSide(account, other, amount, t.text("date"), matched?.has(account) == true),
            SmsTransferSide(other, account, -amount, t.text("date"), matched?.has(other) == true))
    }
}

/** How many days apart a text and the transfer the budget already has for it can be, as banks book the two sides. */
private const val SmsTransferDays = 5L

/**
 * The day each open account's records start: its first transaction, or today for one with none
 * yet, whose balance was entered as it is now. A transaction before it is already in the
 * account's starting balance.
 */
internal fun smsStarts(budget: JSONObject, today: String = LocalDate.now().toString()): Map<String, String> {
    val first = mutableMapOf<String, String>()
    budget.rows("transactions").forEach { t ->
        val date = t.text("date")
        listOf(t.text("accountId"), t.text("transferAccountId")).filter { it.isNotBlank() }.forEach { first.merge(it, date, ::minOf) }
    }
    return budget.rows("accounts").filterNot { it.optBoolean("archived") }.associate { it.text("id") to minOf(first[it.text("id")] ?: today, today) }
}

/**
 * Where collecting starts: from now on ("now"), from the start of [day] ("date"), right after the
 * last text handled, received at [at] ("after"), or everything on the phone ("all"). A day counts
 * whole, so "Since 3 Oct" holds all of 3 Oct.
 */
internal data class SmsStart(val mode: String, val day: String, val at: Long = 0L) {
    val valid: Boolean get() = mode != "date" || runCatching { !LocalDate.parse(day).isAfter(LocalDate.now()) }.getOrDefault(false)
    fun since(now: Long = System.currentTimeMillis()): Long = when (mode) {
        "all" -> 0L
        "date" -> LocalDate.parse(day).atStartOfDay(ZoneId.systemDefault()).toInstant().toEpochMilli()
        "after" -> at + 1
        else -> now
    }

    companion object {
        /** Right after the newest text handled, received at [handled]: carrying on with nothing missed or read twice. */
        fun after(handled: Long) = SmsStart("after", Instant.ofEpochMilli(handled).atZone(ZoneId.systemDefault()).toLocalDate().toString(), handled)

        /**
         * A saved start as it reads back: right after the last text handled when it's that, a day when
         * it falls at one's start, otherwise the moment it was set.
         */
        fun of(since: Long, handled: Long? = null): SmsStart {
            if (since <= 0L) return SmsStart("all", LocalDate.now().toString())
            if (handled != null && since == handled + 1) return after(handled)
            val at = Instant.ofEpochMilli(since).atZone(ZoneId.systemDefault())
            return SmsStart(if (at.toLocalTime() == java.time.LocalTime.MIDNIGHT) "date" else "now", at.toLocalDate().toString())
        }

        /**
         * The first start offered: two months back for a budget with no accounts, which the texts set
         * up and fill; new messages for one with accounts but no transactions yet, whose balances are
         * as they are now; otherwise 30 days back, but not before its records begin.
         */
        fun suggested(budget: JSONObject, today: LocalDate = LocalDate.now()): SmsStart {
            val open = budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") }.toSet()
            if (open.isEmpty()) return SmsStart("date", today.minusMonths(2).toString())
            val first = budget.rows("transactions").filter { it.text("accountId") in open }.minOfOrNull { it.text("date") }
                ?.let { runCatching { LocalDate.parse(it) }.getOrNull() }?.takeIf { !it.isAfter(today) }
                ?: return SmsStart("now", today.toString())
            return SmsStart("date", maxOf(today.minusDays(30), first).toString())
        }
    }
}

/**
 * What the budget itself says about an open account, wherever it was synced from: the last day of
 * transactions that came from the bank (texts or files), and the last reconciled day. Collecting
 * from there leaves no gap, and texts for what's already in are matched, not added again.
 */
internal data class SmsAccountMark(val account: String, val name: String, val bank: String?, val reconciled: String?) {
    /** The day to collect from: where the budget's own record of this account ends. */
    val day: String get() = listOfNotNull(bank, reconciled).max()
}
internal fun smsAccountMarks(budget: JSONObject, today: String = LocalDate.now().toString()): List<SmsAccountMark> {
    val bank = mutableMapOf<String, String>()
    val reconciled = mutableMapOf<String, String>()
    budget.rows("transactions").forEach { t ->
        val date = t.text("date").takeIf { it <= today } ?: return@forEach
        if ((t.optJSONArray("bankImportIds")?.length() ?: 0) > 0) bank.merge(t.text("accountId"), date, ::maxOf)
        if (t.text("status") == "reconciled") reconciled.merge(t.text("accountId"), date, ::maxOf)
        if (t.text("transferStatus") == "reconciled" && t.text("transferAccountId").isNotBlank()) reconciled.merge(t.text("transferAccountId"), date, ::maxOf)
    }
    return budget.rows("accounts").filterNot { it.optBoolean("archived") }.mapNotNull { a ->
        val id = a.text("id")
        if (id !in bank && id !in reconciled) null else SmsAccountMark(id, a.text("name"), bank[id], reconciled[id])
    }
}

/**
 * What reading [texts] takes: requests of [SmsBatchSize], and minutes going by [pace], the
 * milliseconds a text took in this budget's last large request (none until one has run). With
 * [learned] formats a request carries more, as many as its templates read, so only the minutes
 * say how long; the requests are an upper bound, shown when there are no minutes to show.
 */
internal data class SmsReadingSize(val texts: Int, val requests: Int, val minutes: Int?)
internal fun smsReadingSize(texts: Int, pace: Long?, learned: Boolean = false): SmsReadingSize {
    val minutes = pace?.takeIf { it > 0 && texts > 0 }?.let { ((texts * it + 59_999) / 60_000).toInt().coerceAtLeast(1) }
    return SmsReadingSize(texts, if (learned && minutes != null) 0 else (texts + SmsBatchSize - 1) / SmsBatchSize, minutes)
}

/**
 * How adding these items moves each account's balance, by account: a new transaction's amount in its
 * account, and the opposite in a transfer's other account. Matches change nothing.
 */
internal fun smsBalanceChanges(items: List<SmsItem>, adding: List<Pair<Int, String>>): Map<String, Long> {
    val byIndex = items.associateBy { it.index }
    val changes = linkedMapOf<String, Long>()
    adding.filter { it.second == "new" }.forEach { (index, _) ->
        val item = byIndex[index] ?: return@forEach
        val t = item.transaction ?: return@forEach
        val amount = item.amount ?: return@forEach
        t.text("accountId").takeIf { it.isNotBlank() }?.let { changes.merge(it, amount, Long::plus) }
        t.text("transferAccountId").takeIf { it.isNotBlank() }?.let { changes.merge(it, -amount, Long::plus) }
    }
    return changes.filterValues { it != 0L }
}

/**
 * Which account a card is: remembered for its sender, or the one open account whose name carries its
 * digits; and a transfer's other account ([other]), from what the user chose before and the
 * budget's own [transfers].
 */
internal class SmsAccounts(private val open: List<Pair<String, String>>, mappings: JSONArray, private val starts: Map<String, String> = emptyMap(),
                           private val transfers: List<SmsTransferSide> = emptyList()) {
    /** Dated before one of these accounts' records start ([smsStarts]): already in its starting balance. */
    fun early(date: String, vararg accounts: String): Boolean = accounts.any { account -> starts[account]?.let { date < it } == true }

    private val entries = (0 until mappings.length()).map { mappings.getJSONObject(it) }
    private fun isOpen(account: String) = open.any { it.first == account }
    /** What's remembered of one kind, by its key, for an open account or [SmsSkipCard]. */
    private fun kept(kind: String) = entries.filter { it.text("for") == kind }.associate { smsEntryKey(kind, it.text("sender"), it.text("reference")) to it.text("accountId") }
    private val remembered = kept("").filterValues { isOpen(it) }
    private val skipping = kept("").filterValues { it == SmsSkipCard }.keys
    private val others = kept(SmsForTransfer).filterValues { isOpen(it) }
    private val formats = kept(SmsForFormat).filterValues { isOpen(it) }
    private val own = kept(SmsForOwn).filterValues { isOpen(it) }

    /** A card the user said not to add, or a sender whose texts name none: its texts are settled without a question. */
    fun skips(sender: String, ref: String?): Boolean = smsEntryKey("", sender, ref ?: SmsAnyCard) in skipping

    /**
     * The account a card is, or for a text naming none the one chosen for its sender's texts that
     * name none ([SmsAnyCard]); the budget's one account when [only] allows it.
     */
    fun find(sender: String, ref: String?, only: Boolean = true): Pair<String, SmsAccountSource>? {
        if (ref != null) {
            remembered[smsEntryKey("", sender, ref)]?.let { return it to SmsAccountSource.Remembered }
            val digits = smsDigits(ref)
            // The same digits, or one ending the other when both are at least a card's last four:
            // "4417" is "Visa 4417", but "5000" isn't the limit in "Visa (50,000)".
            fun same(run: String) = run == digits || (minOf(run.length, digits.length) >= 4 && (run.endsWith(digits) || digits.endsWith(run)))
            if (digits.length >= 3) open.filter { (_, name) -> Regex("[0-9]+").findAll(name).any { same(it.value) } }
                .singleOrNull()?.let { return it.first to SmsAccountSource.Name }
        } else remembered[smsEntryKey("", sender, SmsAnyCard)]?.let { return it to SmsAccountSource.Remembered }
        return open.singleOrNull()?.takeIf { only }?.let { it.first to SmsAccountSource.Only }
    }

    /**
     * A transfer's other account, and how it was found: the account [otherRef] names; the other side
     * of the transfer the budget already has for this money; for a text naming none, the account
     * the user chose for this card's earlier ones; the one they chose for texts in this [template]'s
     * format (a bank that gives each deposit its own number); the account the assistant [named]; and
     * for a text naming none, the account this one's money moves with that way nearly every time.
     */
    fun other(sender: String, ref: String?, otherRef: String?, named: String?, account: String, amount: Long?, date: String, template: String? = null): Pair<String, SmsAccountSource>? = sequence {
        otherRef?.let { find(sender, it) }?.takeIf { it.second != SmsAccountSource.Only }?.let { yield(it) }
        if (account.isNotBlank() && amount != null) existing(account, amount, date)?.let { yield(it to SmsAccountSource.Budget) }
        if (otherRef == null) ref?.let { others[smsEntryKey(SmsForTransfer, sender, it)] }?.let { yield(it to SmsAccountSource.Remembered) }
        template?.let { formats[smsEntryKey(SmsForFormat, sender, it)] }?.let { yield(it to SmsAccountSource.Remembered) }
        named?.let { name -> open.singleOrNull { it.second.trim().equals(name.trim(), ignoreCase = true) } }?.let { yield(it.first to SmsAccountSource.Assistant) }
        if (otherRef == null && account.isNotBlank() && amount != null) usual(account, amount)?.let { yield(it to SmsAccountSource.Usual) }
    }.firstOrNull { it.first != account }

    /** The user's own account at another bank that a bank's name for a person is ([smsRememberCard] with [SmsForOwn]). */
    fun own(sender: String, name: String?): String? = name?.takeIf { it.isNotBlank() }?.let { own[smsEntryKey(SmsForOwn, sender, it)] }

    /** The other account of the one transfer of this money the budget has in [account] within days of [date], when no bank text matched that side yet. */
    private fun existing(account: String, amount: Long, date: String): String? {
        val day = runCatching { LocalDate.parse(date) }.getOrNull() ?: return null
        fun near(other: String) = runCatching { kotlin.math.abs(java.time.temporal.ChronoUnit.DAYS.between(day, LocalDate.parse(other))) <= SmsTransferDays }.getOrDefault(false)
        return transfers.filter { it.account == account && it.amount == amount && !it.matched && near(it.date) }.map { it.other }.distinct().singleOrNull()?.takeIf { isOpen(it) }
    }

    /** The account [account]'s money moves with this way in nearly all of its last transfers: the account that pays a credit card. */
    private fun usual(account: String, amount: Long): String? {
        val recent = transfers.filter { it.account == account && (it.amount > 0) == (amount > 0) }.sortedByDescending { it.date }.take(10)
        val (other, times) = recent.groupingBy { it.other }.eachCount().maxByOrNull { it.value } ?: return null
        return other.takeIf { times >= 3 && times * 5 >= recent.size * 4 && isOpen(it) }
    }

    companion object {
        /** The budget's open accounts and its transfers, with what the user remembered. */
        fun of(budget: JSONObject, mappings: JSONArray, starts: Map<String, String>): SmsAccounts =
            SmsAccounts(budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") }, mappings, starts, smsTransferSides(budget))
    }
}

/** What a remembered card maps to in place of an account when the user doesn't add its texts. */
internal const val SmsSkipCard = "skip"
/**
 * What else is remembered beside cards: the other account of a card's transfers whose texts name
 * none (the account that pays it), of the transfers a format reads (by its template's id), and a
 * name a bank gives the user's own account at another bank.
 */
internal const val SmsForTransfer = "transfer"
internal const val SmsForFormat = "format"
internal const val SmsForOwn = "own"

/** One remembered entry per sender, kind and card (by its digits), name (by its words) or format (by its id). */
private fun smsEntryKey(kind: String, sender: String, ref: String): String = kind + "\u0000" + when (kind) {
    SmsForOwn -> smsSenderKey(sender) + "\u0000" + ref.trim().lowercase(Locale.ROOT).replace(Regex("\\s+"), " ")
    SmsForFormat -> smsSenderKey(sender) + "\u0000" + ref
    else -> smsCardKey(sender, ref)
}

/**
 * What a choice of other account for these transfers is remembered by: the one account they all
 * name, or the card whose texts name none, and the formats that read them, so a bank that numbers
 * each deposit afresh is answered once. Each as (reference, kind) for [smsRememberCard].
 */
internal fun smsOtherKeys(group: List<SmsItem>): List<Pair<String, String>> {
    val others = group.map { it.transaction?.text("otherRef").orEmpty() }
    val first = group.firstOrNull() ?: return emptyList()
    val named = when {
        others.all { it.isNotBlank() } && others.map(::smsRefKey).distinct().size == 1 -> others[0] to ""
        others.all { it.isBlank() } && group.mapNotNull { it.ref?.let(::smsRefKey) }.distinct().size == 1 && first.ref != null -> first.ref!! to SmsForTransfer
        else -> null
    }
    return listOfNotNull(named) + group.mapNotNull { it.template }.distinct().map { it to SmsForFormat }
}

/** [accountId] remembered for a card, or with [kind] for its transfers or as an own account's name, in place of what was there. */
internal fun smsRememberCard(mappings: JSONArray, sender: String, ref: String, accountId: String, kind: String = ""): JSONArray {
    require(sender.isNotBlank() && sender.length <= 200 && ref.isNotBlank() && ref.length <= 80 && accountId.isNotBlank())
    val key = smsEntryKey(kind, sender, ref)
    val kept = (0 until mappings.length()).map { mappings.getJSONObject(it) }.filterNot { smsEntryKey(it.text("for"), it.text("sender"), it.text("reference")) == key }
    require(kept.size < 200)
    return JSONArray(kept).put(JSONObject().put("sender", sender).put("reference", ref).put("accountId", accountId).apply { if (kind.isNotEmpty()) put("for", kind) })
}

/**
 * One message, or messages about one event, as the review shows it. [transaction] carries the
 * accounts it resolves to; [needs] is what the user still has to give: an account, the amount
 * in the budget's currency, or a transfer's other account. [early] is a transaction from before
 * its account's records start, already in that account's starting balance.
 */
internal data class SmsItem(
    val index: Int, val disposition: String, val note: String, val skip: String, val unsure: Set<String>,
    val sources: List<BankSms>, val transaction: JSONObject?, val accountSource: SmsAccountSource?, val needs: String?,
    val early: Boolean = false,
    /** The template that read it on the phone (the assistant reviewed the reading), and the payee as the bank wrote it. */
    val template: String? = null, val raw: String? = null,
    /** Skipped on the phone without going to the assistant: a code, or a format it has confirmed enough. */
    val kept: Boolean = false,
    /** How a transfer's other account was found. */
    val otherSource: SmsAccountSource? = null,
) {
    /** Ticked to add unless the user said otherwise: a transaction, but not a hold or one already in a starting balance. */
    fun tickedIn(batch: SmsBatch): Boolean = batch.ticked[index] ?: (disposition == "transaction" && !early)
    val sender: String get() = sources.first().sender
    val ref: String? get() = transaction?.text("accountRef")?.ifBlank { null }
    /** Messages of one sender naming one card share this, so one choice answers them all. */
    val card: String? get() = ref?.takeIf { needs == "account" }?.let { smsCardKey(sender, it) }
    /**
     * A transfer still needing its other account: a sender's texts about one account that say the
     * same of it ("Transfer to your term deposit", "Credit card payment") share this, so one choice
     * answers them all, whatever number the bank gave each deposit.
     */
    val transfer: String? get() = transaction?.takeIf { needs == "transfer" }?.let { t ->
        listOf("", smsSenderKey(sender), t.text("accountId"), note.trim().lowercase(Locale.ROOT)).joinToString("\u0000")
    }
    /** The other account was found from the budget's habits or the assistant's reading, not the text's digits: worth a look. */
    val otherGuessed: Boolean get() = otherSource == SmsAccountSource.Usual || otherSource == SmsAccountSource.Assistant
    val amount: Long? get() = transaction?.text("amount")?.let(::smsCents)
}

/**
 * Two texts about one transfer, one from each account (the payment out of checking, and the card
 * receiving it), that the assistant read as two items: the later is the other side of the first
 * ("pair"), settled with it rather than added again. Both accounts are known, the amounts are
 * opposite, and the days at most [SmsTransferDays] apart.
 */
internal fun smsOtherSides(items: List<SmsItem>, dismissed: Set<Int>): List<SmsItem> {
    val transfers = items.filter { it.disposition == "transaction" && it.index !in dismissed && it.needs == null && it.transaction?.text("kind") == "transfer" }
    fun day(item: SmsItem) = runCatching { LocalDate.parse(item.transaction!!.text("date")) }.getOrNull()
    val used = mutableSetOf<Int>()
    val sides = mutableSetOf<Int>()
    transfers.forEach { a ->
        if (a.index in used) return@forEach
        val t = a.transaction!!
        val amount = a.amount ?: return@forEach
        val b = transfers.firstOrNull { b ->
            val u = b.transaction!!
            b.index != a.index && b.index !in used && b.amount == -amount && u.text("accountId") == t.text("transferAccountId") && u.text("transferAccountId") == t.text("accountId") &&
                day(a)?.let { x -> day(b)?.let { y -> kotlin.math.abs(java.time.temporal.ChronoUnit.DAYS.between(x, y)) <= SmsTransferDays } } == true
        } ?: return@forEach
        used += a.index; used += b.index; sides += maxOf(a.index, b.index)
    }
    return items.map { if (it.index in sides) it.copy(disposition = "ignore", skip = "pair") else it }
}

/**
 * A review: the texts read, the assistant's analysis, and the user's choices. [fresh] are accounts
 * made from the review for its texts (a CD a transfer went to): their balance is from before these
 * texts, so no text in them is held back as already in a starting balance. [setup] holds what the
 * user answered for the cards and accounts to set up, by [smsCardKey].
 */
internal data class SmsBatch(val messages: List<BankSms>, val analysis: JSONObject, val ticked: Map<Int, Boolean> = emptyMap(), val dismissed: Set<Int> = emptySet(),
                             val fresh: Set<String> = emptySet(), val setup: Map<String, SmsSetupPlan> = emptyMap(),
                             /** The user's answers for rows that may already be in the budget ("new" or "match:id"), by item. */
                             val decisions: Map<Int, String> = emptyMap()) {
    fun params(analysis: JSONObject = this.analysis): JSONObject = JSONObject().put("messages", JSONArray(messages.map { it.json() })).put("analysis", analysis)
    // Drafts reference inbox originals by ID; evidence excerpts are part of the local review.
    fun saved(): String = JSONObject().put("v", 2).put("ids", JSONArray(messages.map { it.id })).put("analysis", analysis)
        .put("ticked", JSONObject(ticked.mapKeys { it.key.toString() })).put("dismissed", JSONArray(dismissed.toList())).put("fresh", JSONArray(fresh.toList()))
        .put("setup", JSONObject(setup.mapValues { it.value.json() })).put("decisions", JSONObject(decisions.mapKeys { it.key.toString() })).toString()

    /** Texts a template read on the phone, which the assistant reviewed rather than read in full. */
    val templated: Int get() = analysis.rows("items").filter { it.text("template").isNotBlank() && !it.optBoolean("kept") }.sumOf { sourceIds(it).size }
    /** Texts skipped on the phone without going to the assistant. */
    val kept: Int get() = analysis.rows("items").filter { it.optBoolean("kept") }.sumOf { sourceIds(it).size }

    /** Where each account's records start for this review: an account made for its texts has none. */
    fun starts(budget: JSONObject): Map<String, String> = smsStarts(budget) - fresh

    fun items(accounts: SmsAccounts): List<SmsItem> {
        val rows = analysis.rows("items")
        val senders = messages.associate { it.id to it.sender }
        // With two or more cards the budget doesn't know, none goes to its one account unasked: each is set up.
        val unknown = rows.mapNotNull { item ->
            val t = item.optJSONObject("transaction")?.takeIf { it.text("accountId").isBlank() } ?: return@mapNotNull null
            val sender = sourceIds(item).firstNotNullOfOrNull { senders[it] } ?: return@mapNotNull null
            t.text("accountRef").ifBlank { null }?.takeIf { !accounts.skips(sender, it) && accounts.find(sender, it, only = false) == null }?.let { smsCardKey(sender, it) }
        }.distinct().size
        return smsOtherSides(rows.mapIndexed { index, item -> item(index, item, accounts, unknown < 2) }, dismissed)
    }

    private fun item(index: Int, item: JSONObject, accounts: SmsAccounts, only: Boolean): SmsItem {
        val ids = sourceIds(item)
        val sources = messages.filter { it.id in ids }
        val sender = sources.firstOrNull()?.sender.orEmpty()
        val t = item.optJSONObject("transaction")?.let { JSONObject(it.toString()) }
        var source: SmsAccountSource? = null
        var other: SmsAccountSource? = null
        var needs: String? = null
        if (t != null) {
            if (t.text("accountId").isNotBlank()) source = SmsAccountSource.Chosen
            else accounts.find(sender, t.text("accountRef").ifBlank { null }, only)?.let { (id, how) -> t.put("accountId", id); source = how }
            val account = t.text("accountId")
            // Money to or from a name the user said is their own account at another bank is a transfer with it.
            if (t.text("kind") in setOf("income", "expense") && t.text("transferAccountId").isBlank())
                accounts.own(sender, item.text("raw").ifBlank { null } ?: t.text("payee"))?.takeIf { it != account }?.let { id ->
                    t.put("kind", "transfer").put("transferAccountId", id).put("categoryId", JSONObject.NULL)
                    other = SmsAccountSource.Remembered
                }
            if (t.text("kind") == "transfer") {
                if (other == null && t.text("transferAccountId").isNotBlank()) other = SmsAccountSource.Chosen
                if (t.text("transferAccountId").isBlank()) accounts.other(sender, t.text("accountRef").ifBlank { null }, t.text("otherRef").ifBlank { null },
                    t.text("otherAccount").ifBlank { null }, account, smsCents(t.text("amount")), t.text("date"), item.text("template").ifBlank { null })
                    ?.let { (id, how) -> t.put("transferAccountId", id); other = how }
            }
            needs = when {
                account.isBlank() -> "account"
                t.isNull("amount") || t.text("amount").isBlank() -> "amount"
                t.text("kind") == "transfer" && t.text("transferAccountId").isBlank() -> "transfer"
                else -> null
            }
        }
        val unsure = item.optJSONArray("unsure")?.let { a -> (0 until a.length()).map { a.getString(it) }.toSet() }.orEmpty()
        val early = t != null && accounts.early(t.text("date"), t.text("accountId"), t.text("transferAccountId"))
        // From a card the user doesn't add: settled like a code, and still there to add by hand.
        val skipped = needs == "account" && accounts.skips(sender, t?.text("accountRef")?.ifBlank { null })
        return SmsItem(index, if (skipped) "ignore" else item.text("disposition"), item.text("note"), if (skipped) "card" else item.text("skip"), unsure, sources, t, source,
            needs.takeUnless { skipped }, early, item.text("template").ifBlank { null }, item.text("raw").ifBlank { null }, item.optBoolean("kept"), other)
    }

    /**
     * The review with only these items and the messages they read, in order; what was ticked and
     * dismissed stays with its item. Null when none are left.
     */
    fun only(indices: Collection<Int>): SmsBatch? {
        val keep = indices.toSortedSet().toList().takeIf { it.isNotEmpty() } ?: return null
        val all = analysis.getJSONArray("items")
        val ids = keep.flatMap { sourceIds(all.getJSONObject(it)) }.toSet()
        val at = keep.withIndex().associate { (new, old) -> old to new }
        return SmsBatch(messages.filter { it.id in ids }, JSONObject(analysis.toString()).put("items", JSONArray(keep.map { JSONObject(all.getJSONObject(it).toString()) })),
            ticked.mapNotNull { (i, on) -> at[i]?.let { it to on } }.toMap(), dismissed.mapNotNull { at[it] }.toSet(), fresh, setup,
            decisions.mapNotNull { (i, d) -> at[i]?.let { it to d } }.toMap())
    }

    /** The items with a text from [since] on: a later start takes the rest out of the review. */
    fun from(since: Long): SmsBatch? {
        val received = messages.associate { it.id to it.received }
        return only(analysis.rows("items").withIndex().filter { (_, item) -> sourceIds(item).any { (received[it] ?: 0L) >= since } }.map { it.index })
    }

    /** The items whose texts all come from these senders (by [smsSenderKey]): removing a sender takes its texts out. */
    fun fromSenders(keys: Set<String>): SmsBatch? {
        val sender = messages.associate { it.id to smsSenderKey(it.sender) }
        return only(analysis.rows("items").withIndex().filter { (_, item) -> sourceIds(item).all { sender[it] in keys } }.map { it.index })
    }

    private fun sourceIds(item: JSONObject): List<String> = item.optJSONArray("sourceIds")?.let { a -> (0 until a.length()).map { a.getString(it) } }.orEmpty()

    /** The analysis with every account the review resolved, for the core's preview and import. */
    fun resolved(items: List<SmsItem>): JSONObject {
        val copy = JSONObject(analysis.toString())
        val list = copy.getJSONArray("items")
        items.forEach { item -> item.transaction?.let { t ->
            list.getJSONObject(item.index).put("transaction", JSONObject(t.toString())
                .put("accountId", t.text("accountId").ifBlank { null } ?: JSONObject.NULL)
                .put("transferAccountId", t.text("transferAccountId").ifBlank { null } ?: JSONObject.NULL))
        } }
        return copy
    }

    /** The same analysis with one item changed, keeping what the user ticked and dismissed. */
    fun edit(index: Int, change: (JSONObject) -> Unit): SmsBatch {
        val copy = JSONObject(analysis.toString())
        change(copy.getJSONArray("items").getJSONObject(index))
        return copy(analysis = copy)
    }

    /** More messages read into this review: their items come after, so every choice made so far keeps its item. */
    fun plus(more: List<BankSms>, read: JSONObject): SmsBatch {
        val copy = JSONObject(analysis.toString())
        val items = copy.getJSONArray("items")
        read.rows("items").forEach { items.put(it) }
        return copy(messages = messages + more, analysis = copy)
    }

    companion object {
        /** The saved review, its messages found by [load] among those still waiting. */
        fun restore(saved: String?, load: (List<String>) -> List<BankSms>): SmsBatch? = runCatching {
            val json = JSONObject(saved ?: return null)
            // A draft from before the review was redesigned can't be shown; its messages wait again.
            require(json.optInt("v") == 2)
            val ids = json.getJSONArray("ids").let { a -> (0 until a.length()).map { a.getString(it) } }
            val found = load(ids).associateBy { it.id }
            val messages = ids.map { id -> found.getValue(id) }
            val ticked = json.optJSONObject("ticked")?.let { t -> t.keys().asSequence().associate { it.toInt() to t.getBoolean(it) } }.orEmpty()
            val dismissed = json.optJSONArray("dismissed")?.let { a -> (0 until a.length()).map { a.getInt(it) }.toSet() }.orEmpty()
            val fresh = json.optJSONArray("fresh")?.let { a -> (0 until a.length()).map { a.getString(it) }.toSet() }.orEmpty()
            val setup = json.optJSONObject("setup")?.let { s -> s.keys().asSequence().mapNotNull { key -> s.optJSONObject(key)?.let(SmsSetupPlan::of)?.let { key to it } }.toMap() }.orEmpty()
            val decisions = json.optJSONObject("decisions")?.let { d -> d.keys().asSequence().associate { it.toInt() to d.getString(it) } }.orEmpty()
            SmsBatch(messages, json.getJSONObject("analysis"), ticked, dismissed, fresh, setup, decisions)
        }.getOrNull()
    }
}

/** What one request to the assistant reads in full: its reply lists only the money, so a few hundred fit in its time. */
internal const val SmsBatchSize = 200
/**
 * What a budget's first request reads in full, before it knows any format: a new budget's two
 * months of real texts, 200 at a time, didn't come back in time. What it learns makes the next
 * requests mostly reviews.
 */
internal const val SmsFirstRead = 60
/** What one request carries at most: texts it only reviews, and junk read on the phone, cost little of its time. */
internal const val SmsRequestTexts = 1_000
private const val SmsReviewTexts = 400
/** The fewest texts a request reads in full once failures have halved it: one that still fails then says so. */
internal const val SmsLeastRead = 15
/**
 * How long a request must have run for its failure to mean it was too much for the assistant's
 * time (it ran out, or its answer never came out right): the next reads half as many in full. One
 * that fails sooner has no connection or no sign-in, and fewer texts wouldn't help.
 */
internal const val SmsSlowFailure = 45_000L

/**
 * How many of these texts one request takes, by their lanes ("junk", "review", "read"): up to
 * [most] read in full and [SmsReviewTexts] reviewed, any number of junk, within
 * [SmsRequestTexts]. Of those read in full, at most [SmsFirstRead] come from senders the budget
 * has no format for yet ([fresh], a flag for each text): a bank added later is as new as a
 * budget's first. Without lanes, [SmsBatchSize]: which would be read in full isn't known.
 */
internal fun smsRequestSize(lanes: List<String>?, most: Int = SmsBatchSize, fresh: List<Boolean>? = null): Int {
    if (lanes == null) return SmsBatchSize
    var read = 0; var review = 0; var new = 0
    lanes.forEachIndexed { i, lane ->
        when (lane) { "read" -> { read++; if (fresh?.getOrNull(i) == true) new++ }; "review" -> review++ }
        if (read > most || review > SmsReviewTexts || new > SmsFirstRead) return i
    }
    return lanes.size.coerceAtLeast(1)
}

/**
 * Reads waiting texts into a budget's review a request at a time, for the page and the background
 * reader alike: one reading at a time, saved after every request, so stopping, or a request that
 * fails, keeps what's been read and the rest wait.
 */
internal object SmsReadings {
    /**
     * A reading under way: [read] of [total] texts done, [part] being sent now (0 while it's planned),
     * what the assistant is doing with them, and when the reading began.
     */
    data class Reading(val budget: String, val read: Int, val total: Int, val part: Int = 0, val live: SmsLive? = null,
                       val since: Long = System.currentTimeMillis())
    private val current = kotlinx.coroutines.flow.MutableStateFlow<Reading?>(null)
    val state: kotlinx.coroutines.flow.StateFlow<Reading?> = current
    private val lock = kotlinx.coroutines.sync.Mutex()
    @Volatile private var job: Job? = null
    private val ended = kotlinx.coroutines.flow.MutableStateFlow(0L)
    /** Counts readings that ended, however quickly: one can start and end between two frames. */
    val finished: kotlinx.coroutines.flow.StateFlow<Long> = ended

    /** Stops the reading under way, keeping what it has read. */
    fun stop() { job?.cancel() }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    /**
     * Starts a reading that carries on when the page closes, and when the app is left: a service
     * keeps it running in front of Android ([SmsReadingService]), which otherwise pauses an app
     * that's out of sight within seconds, mid-request. [failed] hears why one stopped short.
     */
    fun start(context: android.content.Context, analyzer: SmsAnalyzer, identity: String, connection: JSONObject, messages: List<BankSms>, note: String,
              base: SmsBatch?, still: () -> Boolean, failed: (String) -> Unit) {
        if (messages.isNotEmpty()) SmsReadingService.start(context.applicationContext)
        scope.launch {
            try { read(context.applicationContext, analyzer, identity, connection, messages, note, base, still) }
            catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { failed(failure.message.orEmpty()) }
        }
    }

    /**
     * Reads [messages] into [base] (or a new review) and returns the review; null when another
     * reading is under way. [still] is asked before and after each request: the permission and
     * the open budget can change while the assistant reads.
     */
    suspend fun read(context: android.content.Context, analyzer: SmsAnalyzer, identity: String, connection: JSONObject, messages: List<BankSms>, note: String,
                     base: SmsBatch?, still: () -> Boolean): SmsBatch? {
        if (messages.isEmpty() || !lock.tryLock()) return null
        val inbox = SmsInbox.get(context)
        job = currentCoroutineContext()[Job]
        try {
            current.value = Reading(identity, 0, messages.size)
            var read = base
            var remaining = messages
            // The most texts a request reads in full: halved when one fails after running a while.
            var most = SmsBatchSize
            while (remaining.isNotEmpty()) {
                check(still()) { context.getString(R.string.sms_permission_needed) }
                // The budget's templates read the texts in formats it knows first: junk stays on the phone, the
                // assistant reviews the rest of what they read, and reads only new formats in full. What one
                // request teaches makes the next one smaller, so each is planned when it's sent.
                val (templates, payees, trusted) = withContext(smsStorageLane) { Triple(inbox.templates(identity), inbox.payees(identity), inbox.trusted(identity)) }
                fun request(texts: List<BankSms>) = JSONObject().put("budgetId", identity).put("provider", connection.getString("provider"))
                    .put("model", connection.opt("model") ?: JSONObject.NULL).put("messages", JSONArray(texts.map { it.json() })).put("note", note)
                    .put("templates", templates).put("payees", payees).put("trusted", trusted)
                val probe = remaining.take(SmsRequestTexts)
                // A sender the budget has no format for yet is read fewer at a time: its first request teaches the rest.
                val known = (0 until templates.length()).mapTo(HashSet()) { templates.getJSONObject(it).optString("sender") }
                val lanes = analyzer.lanes(request(probe))
                val part = probe.take(if (lanes == null) minOf(SmsBatchSize, most) else smsRequestSize(lanes, most, probe.map { smsSenderKey(it.sender) !in known }))
                current.value = current.value?.copy(part = part.size, live = null)
                val began = System.currentTimeMillis()
                val result = try {
                    analyzer.analyze(request(part)) { live -> current.value = current.value?.copy(live = live) }
                } catch (cancel: CancellationException) { throw cancel
                } catch (failure: Exception) {
                    // Too much for one request (it ran out of time, or its answer never passed the check):
                    // what's been read is saved, and these texts are tried again half as many at a time.
                    val inFull = lanes?.take(part.size)?.count { it == "read" } ?: part.size
                    if (System.currentTimeMillis() - began < SmsSlowFailure || inFull <= SmsLeastRead) throw failure
                    most = (inFull / 2).coerceAtLeast(SmsLeastRead)
                    current.value = current.value?.copy(part = 0, live = null)
                    continue
                }
                // What the reading taught (templates, how theirs fared, payees) isn't part of the review.
                val learning = result.remove("learning") as? JSONObject
                check(still()) { context.getString(R.string.sms_permission_needed) }
                val grown = read?.plus(part, result) ?: SmsBatch(part, result)
                // A request whose texts the assistant saw a hundred or more of says how long reading takes ("pace",
                // milliseconds a text sent); junk read on the phone alone would say it takes no time.
                val seen = learning?.let { it.optInt("read") + it.optInt("reviewed") } ?: part.size
                val pace = (System.currentTimeMillis() - began).takeIf { seen >= 100 }?.div(part.size)
                withContext(smsStorageLane) {
                    inbox.saveState(identity, "draft", grown.saved())
                    pace?.let { inbox.saveState(identity, "pace", it.toString()) }
                    learning?.let { inbox.learn(identity, it) }
                }
                read = grown
                remaining = remaining.drop(part.size)
                current.value = current.value?.let { it.copy(read = it.read + part.size, part = 0, live = null) }
            }
            return read
        } finally { job = null; current.value = null; lock.unlock(); ended.value++ }
    }
}

/**
 * What the assistant is doing with a request while it answers: its stage ("waiting" for it, "thinking",
 * "writing" its answer, or "fixing" what the app's check found), its latest thought, how many of the
 * [read] texts it reads in full its answer has covered, and the part of the answer it's writing
 * ([section]: items, skipped, corrections or templates); [reviewed] texts it only checks.
 */
internal data class SmsLive(val stage: String, val thought: String, val written: Int, val read: Int, val reviewed: Int, val section: String = "") {
    companion object {
        fun of(json: JSONObject) = SmsLive(json.text("stage"), json.text("thought"), json.optInt("written"), json.optInt("read"), json.optInt("reviewed"), json.text("section"))
    }
}

internal interface SmsAnalyzer {
    suspend fun status(): JSONObject
    /** Reads a request; [live] hears what the assistant is doing until it answers. */
    suspend fun analyze(request: JSONObject, live: (SmsLive) -> Unit = {}): JSONObject
    /** Each text's lane by the budget's templates, to size requests; null reads them [SmsBatchSize] at a time. */
    suspend fun lanes(request: JSONObject): List<String>? = null
}
internal object NativeSmsAnalyzer : SmsAnalyzer {
    override suspend fun status(): JSONObject = withContext(Dispatchers.IO) { NativeServices.call("assistant_status") as JSONObject }
    override suspend fun lanes(request: JSONObject): List<String>? = try {
        withContext(Dispatchers.IO) { (NativeServices.call("assistant_sms_lanes", JSONObject().put("request", request)) as JSONObject).getJSONArray("lanes") }
            .let { a -> (0 until a.length()).map { a.getString(it) } }
    } catch (e: kotlinx.coroutines.CancellationException) { throw e } catch (_: Exception) { null }
    override suspend fun analyze(request: JSONObject, live: (SmsLive) -> Unit): JSONObject {
        var id: String? = null
        try {
            // Capture the returned job even if cancellation arrives during the JNI call.
            withContext(NonCancellable + Dispatchers.IO) {
                id = (NativeServices.call("assistant_sms_start", JSONObject().put("request", request)) as JSONObject).getString("id")
            }
            currentCoroutineContext().ensureActive()
            while (true) {
                delay(400)
                val reply = withContext(Dispatchers.IO) { NativeServices.call("assistant_sms_poll", JSONObject().put("id", id)) as JSONObject }
                if (!reply.optBoolean("running")) return reply.getJSONObject("analysis").apply { reply.optJSONObject("learning")?.let { put("learning", it) } }
                reply.optJSONObject("progress")?.let { live(SmsLive.of(it)) }
            }
        } finally {
            id?.let { job -> withContext(NonCancellable + Dispatchers.IO) { runCatching { NativeServices.call("assistant_sms_cancel", JSONObject().put("id", job)) } } }
        }
    }
}

/**
 * Sending is allowed once per provider and model; a change of either asks again. Since the page reads
 * by itself when it opens, a permission given for reading only on a tap (before version 2) asks again too.
 */
internal fun smsConsented(saved: String?, connection: JSONObject): Boolean = runCatching {
    val consent = JSONObject(saved ?: return false)
    consent.optInt("v") == 2 && consent.text("provider") == connection.text("provider") && consent.text("model") == connection.text("model")
}.getOrDefault(false)
internal fun smsConsent(connection: JSONObject): String = JSONObject().put("v", 2).put("provider", connection.text("provider")).put("model", connection.text("model")).toString()
