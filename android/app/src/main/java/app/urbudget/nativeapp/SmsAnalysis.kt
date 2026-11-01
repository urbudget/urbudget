package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.math.BigDecimal
import java.security.MessageDigest
import java.time.Instant
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

/** Which account a card is: remembered for its sender, or the one open account whose name carries its digits. */
internal class SmsAccounts(private val open: List<Pair<String, String>>, mappings: JSONArray) {
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
 * in the budget's currency, or a transfer's other account.
 */
internal data class SmsItem(
    val index: Int, val disposition: String, val note: String, val skip: String, val unsure: Set<String>,
    val sources: List<BankSms>, val transaction: JSONObject?, val accountSource: SmsAccountSource?, val needs: String?,
) {
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
        SmsItem(index, item.text("disposition"), item.text("note"), item.text("skip"), unsure, sources, t, source, needs)
    }

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
                val result = analyzer.analyze(request)
                check(still()) { context.getString(R.string.sms_permission_needed) }
                val grown = read?.plus(part, result) ?: SmsBatch(part, result)
                withContext(smsStorageLane) { inbox.saveState(identity, "draft", grown.saved()) }
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
