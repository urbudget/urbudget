package app.urbudget.nativeapp

import android.Manifest
import android.content.BroadcastReceiver
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.provider.Telephony
import androidx.core.content.ContextCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.io.File

internal data class SmsSource(val sender: String, val since: Long)

/** Separate from the budget database: capture cannot change the ledger or require the Rust engine. */
internal class SmsInbox private constructor(context: Context) : SQLiteOpenHelper(context, "sms-inbox.sqlite3", null, 2) {
    override fun onConfigure(db: SQLiteDatabase) { db.rawQuery("PRAGMA secure_delete=ON", null).use { it.moveToFirst() } }
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
    fun state(budget: String, name: String): String? = readableDatabase.query("sms_state", arrayOf("value"), "budget=? AND name=?", arrayOf(budget, name), null, null, null).use { if (it.moveToFirst()) it.getString(0) else null }
    fun saveState(budget: String, name: String, value: String?) {
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

    fun capture(message: BankSms) {
        if (message.body.isBlank() || message.body.length > 16_384) return
        val db = writableDatabase
        db.beginTransaction()
        try {
            val budgets = db.query("sources", arrayOf("budget"), "sender_key=? AND since<=?", arrayOf(smsSenderKey(message.sender), message.received.toString()), null, null, null).use { c ->
                buildList { while (c.moveToNext()) add(c.getString(0)) }
            }
            // Capture is deliberately semantic-free. The user excludes sensitive messages before upload.
            if (budgets.isNotEmpty()) {
                for (budget in budgets) {
                    // A deleted local budget must not continue collecting messages.
                    if (File(budget).isAbsolute && !File(budget).isFile) {
                        db.delete("sources", "budget=?", arrayOf(budget)); db.delete("messages", "budget=?", arrayOf(budget)); db.delete("sms_state", "budget=?", arrayOf(budget))
                    } else insert(db, budget, message)
                }
            }
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }

    private fun insert(db: SQLiteDatabase, budget: String, message: BankSms) {
        db.insertWithOnConflict("messages", null, ContentValues().apply {
            put("budget", budget); put("id", message.id); put("sender", message.sender)
            put("received", message.received); put("body", message.body)
        }, SQLiteDatabase.CONFLICT_IGNORE)
    }

    fun pending(budget: String): List<BankSms> = readableDatabase.query("messages", arrayOf("id", "sender", "received", "body"),
        "budget=? AND done=0", arrayOf(budget), null, null, "received ASC, id ASC", "500").use { c ->
        buildList { while (c.moveToNext()) add(BankSms(c.getString(0), c.getString(1), c.getLong(2), c.getString(3))) }
    }

    fun pendingCount(budget: String): Int = readableDatabase.rawQuery("SELECT COUNT(*) FROM messages WHERE budget=? AND done=0", arrayOf(budget)).use { it.moveToFirst(); it.getInt(0) }

    /** Only called after a successful import or an explicit dismissal. Erase raw text at that point. */
    fun finish(budget: String, ids: List<String>) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            ids.forEach { id -> db.update("messages", ContentValues().apply { put("done", 1); put("body", "") }, "budget=? AND id=?", arrayOf(budget, id)) }
            saveState(budget, "draft", null)
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
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
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

    /** List addresses only. No bodies, contacts permission, or contact names leave the SMS provider. */
    fun senders(context: Context): List<String> = addresses(context).distinctBy(::smsSenderKey)

    private fun addresses(context: Context): List<String> {
        check(canRead(context)) { context.getString(R.string.sms_permission_needed) }
        val found = linkedSetOf<String>()
        val cursor = context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI, arrayOf(Telephony.Sms.ADDRESS), null, null, "date DESC")
            ?: error(context.getString(R.string.sms_read_failed))
        cursor.use { c -> while (c.moveToNext()) c.getString(0)?.trim()?.takeIf { it.isNotEmpty() }?.let(found::add) }
        return found.sortedBy { it.lowercase() }
    }

    /** Catches up after process death, reboot, denied background delivery, or an initial history choice. */
    fun refresh(context: Context, budget: String) {
        check(canRead(context)) { context.getString(R.string.sms_permission_needed) }
        val inbox = SmsInbox.get(context)
        val sources = inbox.sources(budget)
        if (sources.isEmpty()) return
        val keys = sources.map { smsSenderKey(it.sender) }.toSet()
        // Enumerate only addresses to include capitalization/format variants of a selected sender.
        // The body query is still restricted to that allowlist, with SQL placeholders throughout.
        val matching = (addresses(context).filter { smsSenderKey(it) in keys } + sources.map { it.sender }).distinct()
        matching.chunked(400).forEach { group ->
            val selection = "address IN (${group.joinToString(",") { "?" }}) AND date>=?"
            val args = (group + sources.minOf { it.since }.toString()).toTypedArray()
            val cursor = context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI,
                arrayOf(Telephony.Sms.ADDRESS, Telephony.Sms.DATE, Telephony.Sms.DATE_SENT, Telephony.Sms.BODY), selection, args, "date ASC")
                ?: error(context.getString(R.string.sms_read_failed))
            cursor.use { c ->
                while (c.moveToNext()) {
                    val sender = c.getString(0) ?: continue
                    val received = c.getLong(1)
                    val sent = c.getLong(2).takeIf { it > 0 } ?: received
                    val body = c.getString(3) ?: continue
                    inbox.capture(BankSms(smsId(sender, sent, body), sender, received, body))
                }
            }
        }
    }
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
                SmsInbox.get(context).capture(BankSms(smsId(sender, first.timestampMillis, body), sender, System.currentTimeMillis(), body))
            } catch (_: Exception) {
                // The next foreground scan recovers from the system inbox. Never log message content.
            } finally { pending.finish() }
        }
    }
    companion object { private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO) }
}
