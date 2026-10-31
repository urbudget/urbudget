package app.urbudget.nativeapp

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
    fun json(): JSONObject = JSONObject().put("id", id).put("sender", sender).put("body", body)
        .put("receivedDate", Instant.ofEpochMilli(received).atZone(ZoneId.systemDefault()).toLocalDate().toString())
}
internal fun smsSenderKey(sender: String): String {
    val text = sender.trim()
    return if (text.matches(Regex("[+0-9 ()-]+"))) text.replace(Regex("[ ()-]"), "") else text.lowercase(Locale.ROOT)
}
internal fun smsId(sender: String, sent: Long, body: String): String = "sms:" + MessageDigest.getInstance("SHA-256")
    .digest("${smsSenderKey(sender)}\u0000$sent\u0000${body.replace("\r\n", "\n")}".toByteArray(Charsets.UTF_8))
    .joinToString("") { "%02x".format(it) }

/** Exact user-edited money validation, never interpretation of an SMS. */
internal fun smsCents(text: String): Long? = runCatching {
    require(text.matches(Regex("-?[0-9]+(?:\\.[0-9]{1,2})?")))
    BigDecimal(text).movePointRight(2).longValueExact().also { require(it != 0L && it in -1_000_000_000_000L..1_000_000_000_000L) }
}.getOrNull()

internal data class SmsBatch(val messages: List<BankSms>, val analysis: JSONObject) {
    fun params(): JSONObject = JSONObject().put("messages", JSONArray(messages.map { it.json() })).put("analysis", analysis)
    // Drafts reference inbox originals by ID; evidence excerpts are part of the local review.
    fun saved(): String = JSONObject().put("ids", JSONArray(messages.map { it.id })).put("analysis", analysis).toString()
    companion object {
        fun restore(saved: String?, pending: List<BankSms>): SmsBatch? = runCatching {
            val json = JSONObject(saved ?: return null)
            val ids = json.getJSONArray("ids").let { a -> (0 until a.length()).map { a.getString(it) } }
            val messages = ids.map { id -> pending.first { it.id == id } }
            SmsBatch(messages, json.getJSONObject("analysis"))
        }.getOrNull()
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

internal fun smsRememberMapping(mappings: JSONArray, item: JSONObject, messages: List<BankSms>, accountId: String): JSONArray {
    val reference = item.optString("accountReference").takeIf { !item.isNull("accountReference") && it.isNotBlank() } ?: error("Missing account reference")
    val ids = item.getJSONArray("sourceIds").let { a -> (0 until a.length()).map { a.getString(it) }.toSet() }
    val sources = messages.filter { it.id in ids }
    require(sources.size == ids.size && sources.isNotEmpty() && sources.map { smsSenderKey(it.sender) }.distinct().size == 1 && sources.all { reference in it.body })
    val sender = sources.first().sender
    val kept = (0 until mappings.length()).map { mappings.getJSONObject(it) }.filterNot { smsSenderKey(it.getString("sender")) == smsSenderKey(sender) && it.getString("reference") == reference }
    require(kept.size < 200)
    return JSONArray(kept).put(JSONObject().put("sender", sender).put("reference", reference).put("accountId", accountId))
}
