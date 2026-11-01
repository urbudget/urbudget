package app.urbudget.nativeapp

import android.Manifest
import android.content.BroadcastReceiver
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.net.Uri
import android.provider.Telephony
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.io.File
import java.util.Locale

internal data class SmsSource(val sender: String, val since: Long)
internal data class SmsSender(val address: String, val count: Int, val preview: String?)
/**
 * What the phone holds from one chosen sender since collecting starts, and what UrBudget collected:
 * texts as SMS, texts as MMS (a phone moved from an iPhone keeps its history that way), MMS it can't
 * read (pictures or attachments), and the first and latest of them.
 */
internal data class SmsCoverage(val sender: String, val sms: Int, val mms: Int, val unreadable: Int, val first: Long?, val latest: Long?, val collected: Int)

/** Separate from the budget database: capture cannot change the ledger or require the Rust engine. */
internal class SmsInbox private constructor(private val context: Context) : SQLiteOpenHelper(context, "sms-inbox.sqlite3", null, 2) {
    override fun onConfigure(db: SQLiteDatabase) { db.rawQuery("PRAGMA secure_delete=ON", null).use { it.moveToFirst() } }

    // A review can hold a phone's whole history, more than SQLite hands back in one value, so it's
    // a private file per budget beside the inbox, written whole and swapped in.
    private fun reviewFile(budget: String): File = File(context.noBackupFilesDir, "sms-reviews").also { it.mkdirs() }
        .resolve(java.security.MessageDigest.getInstance("SHA-256").digest(budget.toByteArray()).joinToString("") { "%02x".format(it) }.take(32) + ".json")
    private fun saveReview(budget: String, value: String?) {
        val file = reviewFile(budget)
        if (value == null) { file.delete(); return }
        val next = File(file.parentFile, file.name + ".new")
        next.writeText(value)
        check(next.renameTo(file) || (file.delete() && next.renameTo(file))) { "The review couldn't be saved." }
    }
    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE sources (budget TEXT NOT NULL, sender TEXT NOT NULL, sender_key TEXT NOT NULL, since INTEGER NOT NULL, PRIMARY KEY(budget,sender_key))")
        db.execSQL("CREATE TABLE messages (budget TEXT NOT NULL, id TEXT NOT NULL, sender TEXT NOT NULL, received INTEGER NOT NULL, body TEXT NOT NULL, done INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(budget,id))")
        db.execSQL("CREATE INDEX inbox_pending ON messages(budget,done,received)")
        createAnalysis(db)
    }
    private fun createAnalysis(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE sms_state (budget TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(budget,name))")
    }
    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) { if (oldVersion < 2) createAnalysis(db) }
    /** A small saved value for a budget; "draft" is its review, kept in a file (one saved here before then is still read). */
    fun state(budget: String, name: String): String? {
        if (name == "draft") reviewFile(budget).takeIf { it.isFile }?.let { return it.readText() }
        return readableDatabase.query("sms_state", arrayOf("value"), "budget=? AND name=?", arrayOf(budget, name), null, null, null).use { if (it.moveToFirst()) it.getString(0) else null }
    }
    fun saveState(budget: String, name: String, value: String?) {
        if (name == "draft") { saveReview(budget, value); writableDatabase.delete("sms_state", "budget=? AND name=?", arrayOf(budget, name)); return }
        if (value == null) writableDatabase.delete("sms_state", "budget=? AND name=?", arrayOf(budget, name))
        else writableDatabase.insertWithOnConflict("sms_state", null, ContentValues().apply { put("budget", budget); put("name", name); put("value", value) }, SQLiteDatabase.CONFLICT_REPLACE)
    }

    fun sources(budget: String): List<SmsSource> = readableDatabase.query("sources", arrayOf("sender", "since"), "budget=?", arrayOf(budget), null, null, "sender").use { c ->
        buildList { while (c.moveToNext()) add(SmsSource(c.getString(0), c.getLong(1))) }
    }

    /** Changing senders removes their unimported message text. Existing import fingerprints survive. */
    fun configure(budget: String, senders: Set<String>, since: Long) {
        require(budget.isNotBlank() && senders.size <= 50 && senders.all { it.isNotBlank() && it.length <= 200 })
        val db = writableDatabase
        db.beginTransaction()
        try {
            val old = sources(budget).associateBy { smsSenderKey(it.sender) }
            saveState(budget, "draft", null)
            db.delete("sources", "budget=?", arrayOf(budget))
            senders.distinctBy(::smsSenderKey).forEach { sender ->
                db.insertOrThrow("sources", null, ContentValues().apply {
                    put("budget", budget); put("sender", sender); put("sender_key", smsSenderKey(sender))
                    put("since", old[smsSenderKey(sender)]?.since ?: since)
                })
            }
            db.query("messages", arrayOf("id", "sender"), "budget=? AND done=0", arrayOf(budget), null, null, null).use { c ->
                val kept = senders.map(::smsSenderKey).toSet()
                while (c.moveToNext()) if (smsSenderKey(c.getString(1)) !in kept) db.delete("messages", "budget=? AND id=?", arrayOf(budget, c.getString(0)))
            }
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }

    /** One message as it arrives; returns the budgets that collected it. */
    fun capture(message: BankSms): List<String> = captureAll(listOf(message))

    /** Messages from a scan of the phone, in one transaction: thousands take a moment, not one write each. */
    fun captureAll(messages: List<BankSms>): List<String> {
        val db = writableDatabase
        val collected = linkedSetOf<String>()
        db.beginTransaction()
        try {
            for (message in messages) {
                if (message.body.isBlank() || message.body.length > 16_384) continue
                val budgets = db.query("sources", arrayOf("budget"), "sender_key=? AND since<=?", arrayOf(smsSenderKey(message.sender), message.received.toString()), null, null, null).use { c ->
                    buildList { while (c.moveToNext()) add(c.getString(0)) }
                }
                // Capture is deliberately semantic-free. The user excludes sensitive messages before upload.
                for (budget in budgets) {
                    // A deleted local budget must not continue collecting messages.
                    if (File(budget).isAbsolute && !File(budget).isFile) {
                        db.delete("sources", "budget=?", arrayOf(budget)); db.delete("messages", "budget=?", arrayOf(budget)); db.delete("sms_state", "budget=?", arrayOf(budget))
                        saveReview(budget, null)
                    } else if (insert(db, budget, message)) collected += budget
                }
            }
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
        return collected.toList()
    }

    /** How many messages each sender (by its key) has here from [since], waiting or done. */
    fun collected(budget: String, since: Long): Map<String, Int> = readableDatabase.rawQuery(
        "SELECT sender, COUNT(*) FROM messages WHERE budget=? AND received>=? GROUP BY sender", arrayOf(budget, since.toString())).use { c ->
        buildMap { while (c.moveToNext()) merge(smsSenderKey(c.getString(0)), c.getInt(1), Int::plus) }
    }

    /** Whether the message is new here. */
    private fun insert(db: SQLiteDatabase, budget: String, message: BankSms): Boolean = db.insertWithOnConflict("messages", null, ContentValues().apply {
        put("budget", budget); put("id", message.id); put("sender", message.sender)
        put("received", message.received); put("body", message.body)
    }, SQLiteDatabase.CONFLICT_IGNORE) != -1L

    /** The waiting messages, oldest first, the order they're read in; as many as a review holds. */
    fun pending(budget: String): List<BankSms> = readableDatabase.query("messages", arrayOf("id", "sender", "received", "body"),
        "budget=? AND done=0", arrayOf(budget), null, null, "received ASC, id ASC", "10000").use { c ->
        buildList { while (c.moveToNext()) add(BankSms(c.getString(0), c.getString(1), c.getLong(2), c.getString(3))) }
    }

    /** These waiting messages, wherever they fall among the rest: a review's, found again. */
    fun waiting(budget: String, ids: List<String>): List<BankSms> = ids.chunked(500).flatMap { group ->
        readableDatabase.query("messages", arrayOf("id", "sender", "received", "body"), "budget=? AND done=0 AND id IN (${group.joinToString(",") { "?" }})",
            arrayOf(budget, *group.toTypedArray()), null, null, null).use { c ->
            buildList { while (c.moveToNext()) add(BankSms(c.getString(0), c.getString(1), c.getLong(2), c.getString(3))) }
        }
    }

    /**
     * Collect from `since` for every sender. Moving it later removes the waiting messages from
     * before it, except those in the open review ([keep]); moving it earlier lets the next scan
     * bring older ones in.
     */
    fun collectFrom(budget: String, since: Long, keep: Set<String>) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            db.update("sources", ContentValues().apply { put("since", since) }, "budget=?", arrayOf(budget))
            db.query("messages", arrayOf("id"), "budget=? AND done=0 AND received<?", arrayOf(budget, since.toString()), null, null, null).use { c ->
                while (c.moveToNext()) if (c.getString(0) !in keep) db.delete("messages", "budget=? AND id=?", arrayOf(budget, c.getString(0)))
            }
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }

    fun pendingCount(budget: String): Int = readableDatabase.rawQuery("SELECT COUNT(*) FROM messages WHERE budget=? AND done=0", arrayOf(budget)).use { it.moveToFirst(); it.getInt(0) }

    /**
     * Only called after a successful import or an explicit dismissal. Erase raw text at that point.
     * A finished review ends with it; dismissing messages that wait outside it ([endReview] false) doesn't.
     */
    fun finish(budget: String, ids: List<String>, endReview: Boolean = true) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            ids.forEach { id -> db.update("messages", ContentValues().apply { put("done", 1); put("body", "") }, "budget=? AND id=?", arrayOf(budget, id)) }
            if (endReview) saveState(budget, "draft", null)
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }

    /** Explicit Stop & clear also forgets local dismissal history; the budget retains imported IDs. */
    fun clear(budget: String) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            db.delete("sources", "budget=?", arrayOf(budget)); db.delete("messages", "budget=?", arrayOf(budget))
            db.delete("sms_state", "budget=?", arrayOf(budget))
            saveReview(budget, null)
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }

    /** Budgets reading new texts as they arrive, for the background reader. */
    fun reading(): List<String> = readableDatabase.query("sms_state", arrayOf("budget"), "name='auto' AND value='on'", null, null, null, null).use { c ->
        buildList { while (c.moveToNext()) add(c.getString(0)) }
    }

    companion object {
        @Volatile private var instance: SmsInbox? = null
        fun get(context: Context): SmsInbox = instance ?: synchronized(this) {
            instance ?: SmsInbox(context.applicationContext).also { instance = it }
        }
    }
}

internal object SmsReader {
    fun canRead(context: Context) = ContextCompat.checkSelfPermission(context, Manifest.permission.READ_SMS) == PackageManager.PERMISSION_GRANTED
    fun canReceive(context: Context) = ContextCompat.checkSelfPermission(context, Manifest.permission.RECEIVE_SMS) == PackageManager.PERMISSION_GRANTED

    /**
     * Senders, most messages first, so a bank stands out from the rest. A short code ("900") shows
     * its latest message to help recognise it; a named sender is recognised by its name, and neither
     * it (a ministry's or a court's notices) nor a person's number is previewed. Nothing read here is
     * kept, and no contacts are read.
     */
    fun senders(context: Context): List<SmsSender> {
        check(canRead(context)) { context.getString(R.string.sms_permission_needed) }
        val counts = linkedMapOf<String, Pair<String, Int>>()
        val cursor = context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI, arrayOf(Telephony.Sms.ADDRESS), null, null, "date DESC")
            ?: error(context.getString(R.string.sms_read_failed))
        cursor.use { c -> while (c.moveToNext()) c.getString(0)?.trim()?.takeIf { it.isNotEmpty() }?.let { address ->
            val key = smsSenderKey(address)
            counts[key] = (counts[key]?.first ?: address) to ((counts[key]?.second ?: 0) + 1)
        } }
        // Texts kept as MMS (history from an iPhone) count for their sender too.
        runCatching {
            val threads = mmsThreads(context) { true }
            val perThread = mutableMapOf<Long, Int>()
            context.contentResolver.query(Telephony.Mms.CONTENT_URI, arrayOf("thread_id"), "msg_box=?", arrayOf(Telephony.Mms.MESSAGE_BOX_INBOX.toString()), null)?.use { c ->
                while (c.moveToNext()) perThread.merge(c.getLong(0), 1, Int::plus)
            }
            perThread.forEach { (thread, n) -> threads[thread]?.let { address ->
                val key = smsSenderKey(address)
                counts[key] = (counts[key]?.first ?: address) to ((counts[key]?.second ?: 0) + n)
            } }
        }
        val previews = mutableMapOf<String, String>()
        counts.values.map { it.first }.filter { !smsPersonal(it) && it.none(Char::isLetter) }.chunked(400).forEach { group ->
            context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI, arrayOf(Telephony.Sms.ADDRESS, Telephony.Sms.BODY),
                "address IN (${group.joinToString(",") { "?" }})", group.toTypedArray(), "date DESC")?.use { c ->
                while (c.moveToNext() && previews.size < counts.size) {
                    val key = smsSenderKey(c.getString(0) ?: continue)
                    if (key !in previews) previews[key] = (c.getString(1) ?: "").replace(Regex("\\s+"), " ").trim().take(120)
                }
            }
        }
        return counts.map { (key, value) -> SmsSender(value.first, value.second, previews[key]) }.sortedByDescending { it.count }
    }

    private fun addresses(context: Context): List<String> {
        check(canRead(context)) { context.getString(R.string.sms_permission_needed) }
        val found = linkedSetOf<String>()
        val cursor = context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI, arrayOf(Telephony.Sms.ADDRESS), null, null, "date DESC")
            ?: error(context.getString(R.string.sms_read_failed))
        cursor.use { c -> while (c.moveToNext()) c.getString(0)?.trim()?.takeIf { it.isNotEmpty() }?.let(found::add) }
        return found.sortedBy { it.lowercase() }
    }

    private val canonical = Uri.parse("content://mms-sms/canonical-addresses")
    private val conversations = Uri.parse("content://mms-sms/conversations?simple=true")
    private val parts = Uri.parse("content://mms/part")

    /** Conversations with one other party, to that party's address: whose MMS each holds. */
    private fun mmsThreads(context: Context, wanted: (String) -> Boolean): Map<Long, String> {
        val resolver = context.contentResolver
        val addresses = mutableMapOf<Long, String>()
        resolver.query(canonical, arrayOf("_id", "address"), null, null, null)?.use { c ->
            while (c.moveToNext()) c.getString(1)?.trim()?.takeIf { it.isNotEmpty() && wanted(it) }?.let { addresses[c.getLong(0)] = it }
        }
        if (addresses.isEmpty()) return emptyMap()
        return buildMap {
            resolver.query(conversations, arrayOf("_id", "recipient_ids"), null, null, null)?.use { c ->
                while (c.moveToNext()) c.getString(1)?.trim()?.toLongOrNull()?.let(addresses::get)?.let { put(c.getLong(0), it) }
            }
        }
    }

    /**
     * Received MMS in these conversations from [since], oldest first: who sent it, when it came and
     * was sent, and its text, or null when it carries more than text. Dates here are in seconds.
     */
    private fun mms(context: Context, threads: Map<Long, String>, since: Long, each: (String, Long, Long, String?) -> Unit) {
        val resolver = context.contentResolver
        threads.keys.toList().chunked(400).forEach { group ->
            val heads = buildList {
                resolver.query(Telephony.Mms.CONTENT_URI, arrayOf("_id", "thread_id", "date", "date_sent"),
                    "thread_id IN (${group.joinToString(",") { "?" }}) AND msg_box=? AND date>=?",
                    (group.map(Long::toString) + Telephony.Mms.MESSAGE_BOX_INBOX.toString() + (since / 1000).toString()).toTypedArray(), "date ASC")?.use { c ->
                    while (c.moveToNext()) add(longArrayOf(c.getLong(0), c.getLong(1), c.getLong(2), c.getLong(3)))
                }
            }
            heads.chunked(400).forEach { chunk ->
                val found = mutableMapOf<Long, MutableList<Pair<String, String?>>>()
                resolver.query(parts, arrayOf("_id", "mid", "ct", "text"), "mid IN (${chunk.joinToString(",") { "?" }})",
                    chunk.map { it[0].toString() }.toTypedArray(), "_id ASC")?.use { c ->
                    while (c.moveToNext()) {
                        val type = c.getString(2).orEmpty().lowercase(Locale.ROOT)
                        // Long text can sit in the part's file rather than its column.
                        val text = if (type == "text/plain") c.getString(3) ?: runCatching {
                            resolver.openInputStream(Uri.parse("content://mms/part/${c.getLong(0)}"))?.use { it.readBytes().toString(Charsets.UTF_8) }
                        }.getOrNull() else null
                        found.getOrPut(c.getLong(1)) { mutableListOf() } += type to text
                    }
                }
                chunk.forEach { (id, thread, date, sent) -> each(threads.getValue(thread), date * 1000, (sent.takeIf { it > 0 } ?: date) * 1000, smsMmsText(found[id].orEmpty())) }
            }
        }
    }

    /**
     * Catches up after process death, reboot, denied background delivery, or a change of where
     * collecting starts: the chosen senders' texts, as SMS and as text-only MMS, saved in one go.
     * Returns, for each sender, what the phone holds and what's collected.
     */
    fun refresh(context: Context, budget: String): List<SmsCoverage> {
        check(canRead(context)) { context.getString(R.string.sms_permission_needed) }
        val inbox = SmsInbox.get(context)
        val sources = inbox.sources(budget)
        if (sources.isEmpty()) return emptyList()
        val keys = sources.map { smsSenderKey(it.sender) }.toSet()
        val since = sources.minOf { it.since }
        class Tally { var sms = 0; var mms = 0; var unreadable = 0; var first: Long? = null; var latest: Long? = null }
        val tallies = mutableMapOf<String, Tally>()
        fun tally(sender: String, received: Long) = tallies.getOrPut(smsSenderKey(sender)) { Tally() }.also { t ->
            t.first = minOf(t.first ?: received, received); t.latest = maxOf(t.latest ?: received, received)
        }
        val found = mutableListOf<BankSms>()
        // The same alert stored twice (same sender, time and text) is one text, here and in the inbox.
        val distinct = hashSetOf<String>()
        // Enumerate only addresses to include capitalization/format variants of a selected sender.
        // The body query is still restricted to that allowlist, with SQL placeholders throughout.
        val matching = (addresses(context).filter { smsSenderKey(it) in keys } + sources.map { it.sender }).distinct()
        matching.chunked(400).forEach { group ->
            val selection = "address IN (${group.joinToString(",") { "?" }}) AND date>=?"
            val args = (group + since.toString()).toTypedArray()
            val cursor = context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI,
                arrayOf(Telephony.Sms.ADDRESS, Telephony.Sms.DATE, Telephony.Sms.DATE_SENT, Telephony.Sms.BODY), selection, args, "date ASC")
                ?: error(context.getString(R.string.sms_read_failed))
            cursor.use { c ->
                while (c.moveToNext()) {
                    val sender = c.getString(0) ?: continue
                    val received = c.getLong(1)
                    val sent = c.getLong(2).takeIf { it > 0 } ?: received
                    val body = c.getString(3) ?: continue
                    val sms = BankSms(smsId(sender, sent, body), sender, received, body)
                    if (distinct.add(sms.id)) { tally(sender, received).sms++; found += sms }
                }
            }
        }
        // A phone moved from an iPhone keeps that history as MMS; a phone without them answers nothing.
        runCatching {
            mms(context, mmsThreads(context) { smsSenderKey(it) in keys }, since) { sender, received, sent, text ->
                if (text == null) tally(sender, received).unreadable++
                else BankSms(smsId(sender, sent, text), sender, received, text).let { if (distinct.add(it.id)) { tally(sender, received).mms++; found += it } }
            }
        }
        inbox.captureAll(found)
        val collected = inbox.collected(budget, since)
        return sources.map { s ->
            val key = smsSenderKey(s.sender)
            val t = tallies[key]
            SmsCoverage(s.sender, t?.sms ?: 0, t?.mms ?: 0, t?.unreadable ?: 0, t?.first, t?.latest, collected[key] ?: 0)
        }
    }
}

/** An MMS's text from its parts' types and texts, or null when it carries more than text (a picture, a file). */
internal fun smsMmsText(parts: List<Pair<String, String?>>): String? {
    if (parts.isEmpty() || parts.any { (type, _) -> type != "text/plain" && type != "application/smil" }) return null
    return parts.filter { it.first == "text/plain" }.joinToString("") { it.second.orEmpty() }.takeIf { it.isNotBlank() }
}

/** Android supplies this broadcast; the manifest requires the system-only BROADCAST_SMS permission. */
class BankSmsReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Telephony.Sms.Intents.SMS_RECEIVED_ACTION || !SmsReader.canRead(context) || !SmsReader.canReceive(context)) return
        val pending = goAsync()
        scope.launch {
            try {
                val parts = Telephony.Sms.Intents.getMessagesFromIntent(intent).orEmpty()
                val first = parts.firstOrNull() ?: return@launch
                val sender = first.originatingAddress ?: return@launch
                if (parts.any { it.originatingAddress != sender }) return@launch
                val body = parts.joinToString("") { it.messageBody.orEmpty() }
                val inbox = SmsInbox.get(context)
                val collected = inbox.capture(BankSms(smsId(sender, first.timestampMillis, body), sender, System.currentTimeMillis(), body))
                // A budget reading texts as they arrive reads this one soon.
                if (collected.isNotEmpty() && collected.any { it in inbox.reading() }) SmsWorker.enqueue(context)
            } catch (_: Exception) {
                // The next foreground scan recovers from the system inbox. Never log message content.
            } finally { pending.finish() }
        }
    }
    companion object { private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO) }
}
