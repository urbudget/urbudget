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
    require(text.matches(Regex("-?[0-9]+(?:\\.[0-9]{1,2})?")))
    BigDecimal(text).movePointRight(2).longValueExact().also { require(it != 0L && it in -1_000_000_000_000L..1_000_000_000_000L) }
}.getOrNull()
/** Cents as the exact decimal the core reads back ("-12.30"). */
internal fun smsDecimal(cents: Long): String = BigDecimal.valueOf(cents, 2).toPlainString()

/** The digits that name a card or account ("***0091" is "0091"), or its words when it has too few. */
internal fun smsRefKey(ref: String): String = ref.filter { it in '0'..'9' }.takeIf { it.length >= 3 } ?: ref.trim().lowercase(Locale.ROOT)
private fun cardKey(sender: String, ref: String) = smsSenderKey(sender) + "\u0000" + smsRefKey(ref)

internal enum class SmsAccountSource { Chosen, Remembered, Name, Only }

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
         * The first start offered: new messages for a budget with no transactions yet, whose balances
         * are as they are now; otherwise 30 days back, but not before its records begin.
         */
        fun suggested(budget: JSONObject, today: LocalDate = LocalDate.now()): SmsStart {
            val open = budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") }.toSet()
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
 * milliseconds a text took in this budget's last full request (none until one has run).
 */
internal data class SmsReadingSize(val texts: Int, val requests: Int, val minutes: Int?)
internal fun smsReadingSize(texts: Int, pace: Long?): SmsReadingSize = SmsReadingSize(texts, (texts + SmsBatchSize - 1) / SmsBatchSize,
    pace?.takeIf { it > 0 && texts > 0 }?.let { ((texts * it + 59_999) / 60_000).toInt().coerceAtLeast(1) })

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

/** Which account a card is: remembered for its sender, or the one open account whose name carries its digits. */
internal class SmsAccounts(private val open: List<Pair<String, String>>, mappings: JSONArray, private val starts: Map<String, String> = emptyMap()) {
    /** Dated before one of these accounts' records start ([smsStarts]): already in its starting balance. */
    fun early(date: String, vararg accounts: String): Boolean = accounts.any { account -> starts[account]?.let { date < it } == true }

    private val remembered = (0 until mappings.length()).map { mappings.getJSONObject(it) }
        .filter { m -> open.any { it.first == m.text("accountId") } }
        .associate { cardKey(it.text("sender"), it.text("reference")) to it.text("accountId") }

    fun find(sender: String, ref: String?): Pair<String, SmsAccountSource>? {
        if (ref != null) {
            remembered[cardKey(sender, ref)]?.let { return it to SmsAccountSource.Remembered }
            val digits = ref.filter { it in '0'..'9' }
            // The same digits, or one ending the other when both are at least a card's last four:
            // "4417" is "Visa 4417", but "5000" isn't the limit in "Visa (50,000)".
            fun same(run: String) = run == digits || (minOf(run.length, digits.length) >= 4 && (run.endsWith(digits) || digits.endsWith(run)))
            if (digits.length >= 3) open.filter { (_, name) -> Regex("[0-9]+").findAll(name).any { same(it.value) } }
                .singleOrNull()?.let { return it.first to SmsAccountSource.Name }
        }
        return open.singleOrNull()?.let { it.first to SmsAccountSource.Only }
    }
}

internal fun smsRememberCard(mappings: JSONArray, sender: String, ref: String, accountId: String): JSONArray {
    require(sender.isNotBlank() && sender.length <= 200 && ref.isNotBlank() && ref.length <= 80 && accountId.isNotBlank())
    val kept = (0 until mappings.length()).map { mappings.getJSONObject(it) }.filterNot { cardKey(it.text("sender"), it.text("reference")) == cardKey(sender, ref) }
    require(kept.size < 200)
    return JSONArray(kept).put(JSONObject().put("sender", sender).put("reference", ref).put("accountId", accountId))
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
) {
    /** Ticked to add unless the user said otherwise: a transaction, but not a hold or one already in a starting balance. */
    fun tickedIn(batch: SmsBatch): Boolean = batch.ticked[index] ?: (disposition == "transaction" && !early)
    val sender: String get() = sources.first().sender
    val ref: String? get() = transaction?.text("accountRef")?.ifBlank { null }
    /** Messages of one sender naming one card share this, so one choice answers them all. */
    val card: String? get() = ref?.takeIf { needs == "account" }?.let { cardKey(sender, it) }
    val amount: Long? get() = transaction?.text("amount")?.let(::smsCents)
}

internal data class SmsBatch(val messages: List<BankSms>, val analysis: JSONObject, val ticked: Map<Int, Boolean> = emptyMap(), val dismissed: Set<Int> = emptySet()) {
    fun params(analysis: JSONObject = this.analysis): JSONObject = JSONObject().put("messages", JSONArray(messages.map { it.json() })).put("analysis", analysis)
    // Drafts reference inbox originals by ID; evidence excerpts are part of the local review.
    fun saved(): String = JSONObject().put("v", 2).put("ids", JSONArray(messages.map { it.id })).put("analysis", analysis)
        .put("ticked", JSONObject(ticked.mapKeys { it.key.toString() })).put("dismissed", JSONArray(dismissed.toList())).toString()

    fun items(accounts: SmsAccounts): List<SmsItem> = analysis.rows("items").mapIndexed { index, item ->
        val ids = item.optJSONArray("sourceIds")?.let { a -> (0 until a.length()).map { a.getString(it) } }.orEmpty()
        val sources = messages.filter { it.id in ids }
        val sender = sources.firstOrNull()?.sender.orEmpty()
        val t = item.optJSONObject("transaction")?.let { JSONObject(it.toString()) }
        var source: SmsAccountSource? = null
        var needs: String? = null
        if (t != null) {
            if (t.text("accountId").isNotBlank()) source = SmsAccountSource.Chosen
            else accounts.find(sender, t.text("accountRef").ifBlank { null })?.let { (id, how) -> t.put("accountId", id); source = how }
            val account = t.text("accountId")
            if (t.text("kind") == "transfer" && t.text("transferAccountId").isBlank()) t.text("otherRef").ifBlank { null }
                ?.let { accounts.find(sender, it) }?.takeIf { it.second != SmsAccountSource.Only && it.first != account }?.let { t.put("transferAccountId", it.first) }
            needs = when {
                account.isBlank() -> "account"
                t.isNull("amount") || t.text("amount").isBlank() -> "amount"
                t.text("kind") == "transfer" && t.text("transferAccountId").isBlank() -> "transfer"
                else -> null
            }
        }
        val unsure = item.optJSONArray("unsure")?.let { a -> (0 until a.length()).map { a.getString(it) }.toSet() }.orEmpty()
        val early = t != null && accounts.early(t.text("date"), t.text("accountId"), t.text("transferAccountId"))
        SmsItem(index, item.text("disposition"), item.text("note"), item.text("skip"), unsure, sources, t, source, needs, early)
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
            ticked.mapNotNull { (i, on) -> at[i]?.let { it to on } }.toMap(), dismissed.mapNotNull { at[it] }.toSet())
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
            SmsBatch(messages, json.getJSONObject("analysis"), ticked, dismissed)
        }.getOrNull()
    }
}

/** What one request to the assistant reads: its reply lists only the money, so a few hundred fit in its time. */
internal const val SmsBatchSize = 200

/**
 * Reads waiting texts into a budget's review a request at a time, for the page and the background
 * reader alike: one reading at a time, saved after every request, so stopping, or a request that
 * fails, keeps what's been read and the rest wait.
 */
internal object SmsReadings {
    data class Reading(val budget: String, val read: Int, val total: Int)
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
    /** Starts a reading that carries on when the page closes; [failed] hears why one stopped short. */
    fun start(context: android.content.Context, analyzer: SmsAnalyzer, identity: String, connection: JSONObject, messages: List<BankSms>, note: String,
              base: SmsBatch?, still: () -> Boolean, failed: (String) -> Unit) {
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
            for (part in messages.chunked(SmsBatchSize)) {
                check(still()) { context.getString(R.string.sms_permission_needed) }
                val request = JSONObject().put("budgetId", identity).put("provider", connection.getString("provider")).put("model", connection.opt("model") ?: JSONObject.NULL)
                    .put("messages", JSONArray(part.map { it.json() })).put("note", note)
                val began = System.currentTimeMillis()
                val result = analyzer.analyze(request)
                check(still()) { context.getString(R.string.sms_permission_needed) }
                val grown = read?.plus(part, result) ?: SmsBatch(part, result)
                // A request of a hundred texts or more says how long reading takes ("pace", milliseconds a text).
                val pace = (System.currentTimeMillis() - began).takeIf { part.size >= 100 }?.div(part.size)
                withContext(smsStorageLane) {
                    inbox.saveState(identity, "draft", grown.saved())
                    pace?.let { inbox.saveState(identity, "pace", it.toString()) }
                }
                read = grown
                current.value = Reading(identity, (current.value?.read ?: 0) + part.size, messages.size)
            }
            return read
        } finally { job = null; current.value = null; lock.unlock(); ended.value++ }
    }
}

internal interface SmsAnalyzer {
    suspend fun status(): JSONObject
    suspend fun analyze(request: JSONObject): JSONObject
}
internal object NativeSmsAnalyzer : SmsAnalyzer {
    override suspend fun status(): JSONObject = withContext(Dispatchers.IO) { NativeServices.call("assistant_status") as JSONObject }
    override suspend fun analyze(request: JSONObject): JSONObject {
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
                if (!reply.optBoolean("running")) return reply.getJSONObject("analysis")
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
