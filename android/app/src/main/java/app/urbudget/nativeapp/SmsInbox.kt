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
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.Locale

internal data class SmsSource(val sender: String, val since: Long)
/**
 * The budget's templates in use, their rows filled and confirmed by the assistant in review, each
 * template's share confirmed ([accuracy], by id), and senders whose formats look to have changed.
 */
internal data class SmsTemplateSummary(val count: Int, val filled: Int, val confirmed: Int, val accuracy: Map<String, Int> = emptyMap(), val drifting: Set<String> = emptySet())

/** A template as the settings list shows it: its format, a text it read, its state and how its readings fared. */
internal data class SmsTemplateRow(val id: String, val sender: String, val body: JSONObject, val example: String, val state: String, val stats: JSONObject) {
    val skip: String? get() = body.optJSONObject("outcome")?.let { o -> o.optString("skip").takeUnless { o.isNull("skip") || it.isBlank() } }
    val kind: String get() = body.optJSONObject("outcome")?.optString("kind").orEmpty()
    val accuracy: Int? get() = smsAccuracy(stats)
}

/** The share of a template's reviewed rows the assistant confirmed unchanged, once it has some. */
internal fun smsAccuracy(stats: JSONObject): Int? = stats.optInt("filled").takeIf { it > 0 }?.let { (stats.optInt("confirmed") * 100) / it }

/**
 * Whether a skip template's texts can stay on the phone: its readings agreed with the assistant
 * three times or more, over two requests or more (writing it counts as one), with no correction
 * by the assistant or the user. Money templates are always reviewed.
 */
internal fun smsTrusted(body: JSONObject, stats: JSONObject): Boolean {
    val outcome = body.optJSONObject("outcome") ?: return false
    if (outcome.isNull("skip") || outcome.optString("skip").isBlank()) return false
    fun none(name: String) = stats.optJSONObject(name)?.let { o -> o.keys().asSequence().all { o.optInt(it) == 0 } } ?: true
    return stats.optInt("agreed") + stats.optInt("confirmed") >= 3 && 1 + stats.optInt("requests") >= 2 && none("corrected") && none("edited")
}

/** A sender's formats look to have changed: more than 5 of its last 20 texts were new, where before that hardly any were. */
internal fun smsDrifting(known: List<Boolean>): Boolean {
    if (known.size < 30) return false
    val last = known.takeLast(20); val before = known.dropLast(20).takeLast(20)
    return last.count { !it } > 5 && before.count { !it } <= 1
}
internal data class SmsSender(val address: String, val count: Int, val preview: String?)
/** A text as a budget has it: [done] once a review settled it, and what became of it then ([SmsOutcome]). */
internal data class SmsRecord(val done: Boolean, val outcome: JSONObject?)
/**
 * What the phone holds from one chosen sender since collecting starts, and what UrBudget collected:
 * texts as SMS, texts as MMS (a phone moved from an iPhone keeps its history that way), MMS it can't
 * read (pictures or attachments), and the first and latest of them.
 */
internal data class SmsCoverage(val sender: String, val sms: Int, val mms: Int, val unreadable: Int, val first: Long?, val latest: Long?, val collected: Int)

/** Separate from the budget database: capture cannot change the ledger or require the Rust engine. */
internal class SmsInbox private constructor(private val context: Context) : SQLiteOpenHelper(context, "sms-inbox.sqlite3", null, 6) {
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
        db.execSQL("CREATE TABLE messages (budget TEXT NOT NULL, id TEXT NOT NULL, sender TEXT NOT NULL, received INTEGER NOT NULL, body TEXT NOT NULL, done INTEGER NOT NULL DEFAULT 0, outcome TEXT, PRIMARY KEY(budget,id))")
        db.execSQL("CREATE INDEX inbox_pending ON messages(budget,done,received)")
        createAnalysis(db)
        createTemplates(db)
        createPayees(db)
    }
    private fun createAnalysis(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE sms_state (budget TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(budget,name))")
    }
    /** Formats the assistant wrote, per budget and sender: the template, a text it was written from, and how its readings fared. */
    private fun createTemplates(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE sms_templates (budget TEXT NOT NULL, id TEXT NOT NULL, sender_key TEXT NOT NULL, body TEXT NOT NULL, example TEXT NOT NULL, " +
            "state TEXT NOT NULL, stats TEXT NOT NULL, first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL, PRIMARY KEY(budget,id))")
    }
    /** The bank's names for payees, per budget and sender, to the user's payee and category; mine when the user chose it. */
    private fun createPayees(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE sms_payees (budget TEXT NOT NULL, sender_key TEXT NOT NULL, raw_key TEXT NOT NULL, raw TEXT NOT NULL, payee TEXT NOT NULL, " +
            "category_id TEXT, seen INTEGER NOT NULL, mine INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(budget,sender_key,raw_key))")
    }
    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        if (oldVersion < 2) createAnalysis(db)
        if (oldVersion < 3) createTemplates(db)
        if (oldVersion < 4) {
            createPayees(db)
            // Payees learned before the table were a list in sms_state.
            db.query("sms_state", arrayOf("budget", "value"), "name='payees'", null, null, null, null).use { c ->
                while (c.moveToNext()) runCatching { mergePayees(db, c.getString(0), JSONArray(c.getString(1)), System.currentTimeMillis(), mine = false) }
            }
            db.delete("sms_state", "name='payees'", null)
        }
        // What became of each text a review settled ([SmsOutcome]), kept once its body is gone.
        if (oldVersion < 5) db.execSQL("ALTER TABLE messages ADD COLUMN outcome TEXT")
        // Which learned payees the user chose, which the assistant's reading never replaces. A table
        // made before version 4 is made with it.
        if (oldVersion in 4..5) db.execSQL("ALTER TABLE sms_payees ADD COLUMN mine INTEGER NOT NULL DEFAULT 0")
    }
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
            val kept = senders.map(::smsSenderKey).toSet()
            db.query("messages", arrayOf("id", "sender"), "budget=? AND done=0", arrayOf(budget), null, null, null).use { c ->
                while (c.moveToNext()) if (smsSenderKey(c.getString(1)) !in kept) db.delete("messages", "budget=? AND id=?", arrayOf(budget, c.getString(0)))
            }
            db.query("sms_templates", arrayOf("id", "sender_key"), "budget=?", arrayOf(budget), null, null, null).use { c ->
                while (c.moveToNext()) if (c.getString(1) !in kept) db.delete("sms_templates", "budget=? AND id=?", arrayOf(budget, c.getString(0)))
            }
            db.query("sms_payees", arrayOf("sender_key"), "budget=?", arrayOf(budget), "sender_key", null, null).use { c ->
                while (c.moveToNext()) if (c.getString(0) !in kept) db.delete("sms_payees", "budget=? AND sender_key=?", arrayOf(budget, c.getString(0)))
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
                        db.delete("sms_templates", "budget=?", arrayOf(budget)); db.delete("sms_payees", "budget=?", arrayOf(budget))
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

    /** These texts as this budget has them: whether each is done, and what became of it ([SmsOutcome]). Texts never collected are missing. */
    fun records(budget: String, ids: Collection<String>): Map<String, SmsRecord> = ids.distinct().chunked(500).flatMap { group ->
        readableDatabase.query("messages", arrayOf("id", "done", "outcome"), "budget=? AND id IN (${group.joinToString(",") { "?" }})",
            arrayOf(budget, *group.toTypedArray()), null, null, null).use { c ->
            buildList { while (c.moveToNext()) add(c.getString(0) to SmsRecord(c.getInt(1) == 1, c.getString(2)?.let { runCatching { JSONObject(it) }.getOrNull() })) }
        }
    }.toMap()

    /**
     * Texts the assistant found, back in Bank messages to be read: from this budget's chosen senders
     * only, whatever day they came, collected afresh or waiting again when a review finished them.
     * Returns how many now wait.
     */
    fun bring(budget: String, texts: List<BankSms>): Int {
        val keys = sources(budget).map { smsSenderKey(it.sender) }.toSet()
        val db = writableDatabase
        var waiting = 0
        db.beginTransaction()
        try {
            texts.filter { smsSenderKey(it.sender) in keys && it.body.isNotBlank() && it.body.length <= 16_384 }.forEach { text ->
                if (insert(db, budget, text)) waiting++
                else waiting += db.update("messages", ContentValues().apply { put("done", 0); put("body", text.body); putNull("outcome") }, "budget=? AND id=?", arrayOf(budget, text.id))
            }
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
        return waiting
    }

    /** When the first and last waiting texts came, or null when none wait. */
    fun waitingRange(budget: String): Pair<Long, Long>? = readableDatabase.rawQuery("SELECT MIN(received), MAX(received) FROM messages WHERE budget=? AND done=0", arrayOf(budget))
        .use { if (it.moveToFirst() && !it.isNull(0)) it.getLong(0) to it.getLong(1) else null }

    fun pendingCount(budget: String): Int = readableDatabase.rawQuery("SELECT COUNT(*) FROM messages WHERE budget=? AND done=0", arrayOf(budget)).use { it.moveToFirst(); it.getInt(0) }

    /** When the newest text this budget finished with (added, matched, cleared or dismissed) came: where collecting has caught up to. */
    fun handledThrough(budget: String): Long? = readableDatabase.rawQuery("SELECT MAX(received) FROM messages WHERE budget=? AND done=1", arrayOf(budget))
        .use { if (it.moveToFirst() && !it.isNull(0)) it.getLong(0) else null }

    /** Texts here received from [from] up to [until], waiting or done: what an earlier start won't bring in again. */
    fun collectedBetween(budget: String, from: Long, until: Long): Int = readableDatabase.rawQuery(
        "SELECT COUNT(*) FROM messages WHERE budget=? AND received>=? AND received<?", arrayOf(budget, from.toString(), until.toString())).use { it.moveToFirst(); it.getInt(0) }

    /** Waiting texts received before [until]: what a later start takes out. */
    fun waitingBefore(budget: String, until: Long): Int = readableDatabase.rawQuery(
        "SELECT COUNT(*) FROM messages WHERE budget=? AND done=0 AND received<?", arrayOf(budget, until.toString())).use { it.moveToFirst(); it.getInt(0) }

    /**
     * Only called after a successful import or an explicit dismissal. Erase raw text at that point.
     * A finished review ends with it; dismissing messages that wait outside it ([endReview] false) doesn't.
     */
    fun finish(budget: String, ids: List<String>, endReview: Boolean = true, outcomes: Map<String, JSONObject> = emptyMap()) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            ids.forEach { id ->
                db.update("messages", ContentValues().apply { put("done", 1); put("body", ""); outcomes[id]?.let { put("outcome", it.toString()) } }, "budget=? AND id=?", arrayOf(budget, id))
            }
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
            db.delete("sms_state", "budget=?", arrayOf(budget)); db.delete("sms_templates", "budget=?", arrayOf(budget)); db.delete("sms_payees", "budget=?", arrayOf(budget))
            saveReview(budget, null)
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }

    /** The budget's templates in use, as the assistant reads them (the core's `Template`). */
    fun templates(budget: String): JSONArray = readableDatabase.query("sms_templates", arrayOf("body"), "budget=? AND state!='retired'", arrayOf(budget), null, null, "first_seen").use { c ->
        JSONArray().also { list -> while (c.moveToNext()) list.put(JSONObject(c.getString(0))) }
    }

    /** Skip templates whose readings the assistant has confirmed enough: their texts stay on the phone. */
    fun trusted(budget: String): JSONArray = readableDatabase.query("sms_templates", arrayOf("id"), "budget=? AND state='trusted'", arrayOf(budget), null, null, null).use { c ->
        JSONArray().also { list -> while (c.moveToNext()) list.put(c.getString(0)) }
    }

    /** How the budget's templates are doing: in use, and their rows filled and confirmed by the assistant. */
    fun templateSummary(budget: String): SmsTemplateSummary {
        var count = 0; var filled = 0; var confirmed = 0
        val accuracy = mutableMapOf<String, Int>()
        readableDatabase.query("sms_templates", arrayOf("id", "stats"), "budget=? AND state!='retired'", arrayOf(budget), null, null, null).use { c ->
            while (c.moveToNext()) {
                count++
                val s = JSONObject(c.getString(1))
                filled += s.optInt("filled"); confirmed += s.optInt("confirmed")
                smsAccuracy(s)?.let { accuracy[c.getString(0)] = it }
            }
        }
        return SmsTemplateSummary(count, filled, confirmed, accuracy, drifting(budget))
    }

    /** Every template in use, grouped by sender in the settings list: what it is, an example, and how it's doing. */
    fun templateList(budget: String): List<SmsTemplateRow> = readableDatabase.query("sms_templates", arrayOf("id", "sender_key", "body", "example", "state", "stats"),
        "budget=? AND state!='retired'", arrayOf(budget), null, null, "sender_key, first_seen").use { c ->
        buildList { while (c.moveToNext()) add(SmsTemplateRow(c.getString(0), c.getString(1), JSONObject(c.getString(2)), c.getString(3), c.getString(4), JSONObject(c.getString(5)))) }
    }

    /** Payees as the bank writes them, to the user's (the core's `Payee`). */
    fun payees(budget: String): JSONArray = readableDatabase.query("sms_payees", arrayOf("sender_key", "raw", "payee", "category_id", "mine"), "budget=?", arrayOf(budget), null, null, "seen DESC").use { c ->
        JSONArray().also { list -> while (c.moveToNext()) list.put(JSONObject().put("sender", c.getString(0)).put("raw", c.getString(1)).put("payee", c.getString(2))
            .put("categoryId", c.getString(3) ?: JSONObject.NULL).put("mine", c.getInt(4) == 1)) }
    }

    /**
     * What a reading taught: new templates (each with the text it was written from), templates a
     * rewrite retired, how each template's rows fared, payees learned, and which texts each sender
     * sent that no template knew. States follow: a skip template becomes trusted once its readings
     * are confirmed often enough and loses it on any correction; a format rewritten three times in
     * a month, or unused for 180 days, retires.
     */
    fun learn(budget: String, learning: JSONObject, now: Long = System.currentTimeMillis()) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            val touched = mutableSetOf<String>()
            learning.optJSONArray("templates")?.let { list -> for (i in 0 until list.length()) {
                val learned = list.getJSONObject(i)
                val template = learned.getJSONObject("template")
                val id = template.getString("id")
                val stats = JSONObject().put("agreed", learned.optInt("agreed"))
                // A rewrite carries how often its format was rewritten, and since when.
                learned.optString("replaces").takeIf { it.isNotBlank() && !learned.isNull("replaces") }?.let { old -> statsOf(db, budget, old)?.let { before ->
                    stats.put("rewrites", before.optInt("rewrites") + 1).put("rewritesSince", before.optLong("rewritesSince").takeIf { it > 0 } ?: now)
                } }
                db.insertWithOnConflict("sms_templates", null, ContentValues().apply {
                    put("budget", budget); put("id", id); put("sender_key", template.getString("sender"))
                    put("body", template.toString()); put("example", learned.optString("example")); put("state", "candidate")
                    put("stats", stats.toString()); put("first_seen", now); put("last_seen", now)
                }, SQLiteDatabase.CONFLICT_IGNORE)
                touched += id
            } }
            learning.optJSONArray("retired")?.let { list -> for (i in 0 until list.length())
                db.update("sms_templates", ContentValues().apply { put("state", "retired") }, "budget=? AND id=?", arrayOf(budget, list.getString(i)))
            }
            learning.optJSONObject("stats")?.let { stats -> stats.keys().forEach { id ->
                val got = stats.getJSONObject(id)
                addStats(db, budget, id, now) { s ->
                    s.put("filled", s.optInt("filled") + got.optInt("filled")).put("confirmed", s.optInt("confirmed") + got.optInt("confirmed"))
                        .put("settled", s.optInt("settled") + got.optInt("settled"))
                    if (got.optInt("filled") > 0) s.put("requests", s.optInt("requests") + 1)
                    val corrected = s.optJSONObject("corrected") ?: JSONObject()
                    got.optJSONObject("corrected")?.let { c -> c.keys().forEach { f -> corrected.put(f, corrected.optInt(f) + c.getInt(f)) } }
                    s.put("corrected", corrected)
                }
                touched += id
            } }
            touched.forEach { settle(db, budget, it, now) }
            // Formats unused for half a year have been replaced by the bank.
            db.update("sms_templates", ContentValues().apply { put("state", "retired") }, "budget=? AND state!='retired' AND last_seen<?",
                arrayOf(budget, (now - 180L * 86_400_000L).toString()))
            learning.optJSONArray("payees")?.takeIf { it.length() > 0 }?.let { mergePayees(db, budget, it, now, mine = false) }
            learning.optJSONObject("senders")?.let { senders -> track(budget, senders) }
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }

    /** A template's state from its stats: trusted, still a candidate, or retired for being rewritten too often. */
    private fun settle(db: SQLiteDatabase, budget: String, id: String, now: Long) {
        val (state, stats, body) = db.query("sms_templates", arrayOf("state", "stats", "body"), "budget=? AND id=?", arrayOf(budget, id), null, null, null).use { c ->
            if (c.moveToFirst()) Triple(c.getString(0), JSONObject(c.getString(1)), JSONObject(c.getString(2))) else null
        } ?: return
        if (state == "retired") return
        val next = when {
            stats.optInt("rewrites") >= 3 && now - stats.optLong("rewritesSince") < 30L * 86_400_000L -> "retired"
            smsTrusted(body, stats) -> "trusted"
            else -> "candidate"
        }
        if (next != state) db.update("sms_templates", ContentValues().apply { put("state", next) }, "budget=? AND id=?", arrayOf(budget, id))
    }

    /** The user changed these [fields] of a row a template read, before adding it: counted against the template. */
    fun edited(budget: String, template: String, fields: Collection<String>) {
        if (fields.isEmpty()) return
        val now = System.currentTimeMillis()
        addStats(writableDatabase, budget, template, now) { s ->
            val edited = s.optJSONObject("edited") ?: JSONObject()
            fields.forEach { edited.put(it, edited.optInt(it) + 1) }
            s.put("edited", edited)
        }
        settle(writableDatabase, budget, template, now)
    }

    /** The user's payee for a name the bank writes: every next text with it gets the user's, whoever reads it. */
    fun learnPayee(budget: String, sender: String, raw: String, payee: String, categoryId: String?) = mergePayees(writableDatabase, budget,
        JSONArray().put(JSONObject().put("sender", sender).put("raw", raw).put("payee", payee).put("categoryId", categoryId ?: JSONObject.NULL)), System.currentTimeMillis(), mine = true)

    /**
     * Payees for the bank's names: the user's ([mine]) replace what was there; the assistant's
     * readings replace only its own earlier ones, so a name the user chose stays theirs.
     */
    private fun mergePayees(db: SQLiteDatabase, budget: String, more: JSONArray, now: Long, mine: Boolean) {
        for (i in 0 until more.length()) {
            val p = more.getJSONObject(i)
            val key = arrayOf(budget, p.getString("sender"), p.getString("raw").trim().replace(Regex("\\s+"), " ").lowercase())
            val values = ContentValues().apply {
                put("budget", key[0]); put("sender_key", key[1]); put("raw_key", key[2])
                put("raw", p.getString("raw")); put("payee", p.getString("payee")); put("category_id", p.optString("categoryId").takeUnless { p.isNull("categoryId") || it.isBlank() })
                put("seen", now + i); put("mine", if (mine) 1 else 0)
            }
            if (mine) db.insertWithOnConflict("sms_payees", null, values, SQLiteDatabase.CONFLICT_REPLACE)
            // The user's name for it stands: the assistant only saw it again.
            else if (db.update("sms_payees", values, "budget=? AND sender_key=? AND raw_key=? AND mine=0", key) == 0 &&
                db.insertWithOnConflict("sms_payees", null, values, SQLiteDatabase.CONFLICT_IGNORE) == -1L)
                db.update("sms_payees", ContentValues().apply { put("seen", now + i) }, "budget=? AND sender_key=? AND raw_key=?", key)
        }
        // The most recent 2,000 are plenty for a bank's merchants.
        db.execSQL("DELETE FROM sms_payees WHERE budget=? AND rowid NOT IN (SELECT rowid FROM sms_payees WHERE budget=? ORDER BY seen DESC LIMIT 2000)", arrayOf(budget, budget))
    }

    private fun statsOf(db: SQLiteDatabase, budget: String, id: String): JSONObject? =
        db.query("sms_templates", arrayOf("stats"), "budget=? AND id=?", arrayOf(budget, id), null, null, null).use { c -> if (c.moveToFirst()) JSONObject(c.getString(0)) else null }

    private fun addStats(db: SQLiteDatabase, budget: String, id: String, now: Long, change: (JSONObject) -> Unit) {
        val stats = statsOf(db, budget, id) ?: return
        change(stats)
        db.update("sms_templates", ContentValues().apply { put("stats", stats.toString()); put("last_seen", now) }, "budget=? AND id=?", arrayOf(budget, id))
    }

    /** Each sender's latest texts, whether a template knew them, newest last: a run of unknown ones is a bank changing its wording. */
    private fun track(budget: String, senders: JSONObject) {
        val all = JSONObject(state(budget, "drift") ?: "{}")
        senders.keys().forEach { sender ->
            val flags = all.optJSONArray(sender)?.let { a -> (0 until a.length()).map { a.getBoolean(it) } }.orEmpty() +
                senders.getJSONArray(sender).let { a -> (0 until a.length()).map { a.getBoolean(it) } }
            all.put(sender, JSONArray(flags.takeLast(40)))
        }
        saveState(budget, "drift", all.toString())
    }

    /** Senders whose formats look to have changed: of their last 20 texts more than 5 were new, where before that hardly any were. */
    fun drifting(budget: String): Set<String> {
        val all = JSONObject(state(budget, "drift") ?: "{}")
        return all.keys().asSequence().filter { sender ->
            val flags = all.getJSONArray(sender).let { a -> (0 until a.length()).map { a.getBoolean(it) } }
            smsDrifting(flags)
        }.toSet()
    }

    /** Forgets one template: its format is read in full again, and learned again. */
    fun forgetTemplate(budget: String, id: String) { writableDatabase.delete("sms_templates", "budget=? AND id=?", arrayOf(budget, id)) }

    /** Forgets the budget's templates and learned payees: the assistant reads every format in full again. */
    fun forgetTemplates(budget: String) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            db.delete("sms_templates", "budget=?", arrayOf(budget))
            db.delete("sms_payees", "budget=?", arrayOf(budget))
            db.delete("sms_state", "budget=? AND name IN ('payees','drift')", arrayOf(budget))
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
     * Received MMS in these conversations from [since] until before [until], oldest first: who sent
     * it, when it came and was sent, and its text, or null when it carries more than text. Dates
     * here are in seconds.
     */
    private fun mms(context: Context, threads: Map<Long, String>, since: Long, until: Long = Long.MAX_VALUE, each: (String, Long, Long, String?) -> Unit) {
        val resolver = context.contentResolver
        threads.keys.toList().chunked(400).forEach { group ->
            val heads = buildList {
                resolver.query(Telephony.Mms.CONTENT_URI, arrayOf("_id", "thread_id", "date", "date_sent"),
                    "thread_id IN (${group.joinToString(",") { "?" }}) AND msg_box=? AND date>=? AND date<?",
                    (group.map(Long::toString) + Telephony.Mms.MESSAGE_BOX_INBOX.toString() + (since / 1000).toString() +
                        (if (until == Long.MAX_VALUE) until else (until + 999) / 1000).toString()).toTypedArray(), "date ASC")?.use { c ->
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
     * About how many texts the phone holds from these senders, received from [from] up to [until]:
     * what a start would bring in. Counts only, so no text is read; MMS with pictures count too.
     */
    fun count(context: Context, senders: Collection<String>, from: Long, until: Long = Long.MAX_VALUE): Int {
        check(canRead(context)) { context.getString(R.string.sms_permission_needed) }
        val keys = senders.map(::smsSenderKey).toSet()
        if (keys.isEmpty() || from >= until) return 0
        var total = 0
        (addresses(context).filter { smsSenderKey(it) in keys } + senders).distinct().chunked(400).forEach { group ->
            context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI, arrayOf(Telephony.Sms._ID), "address IN (${group.joinToString(",") { "?" }}) AND date>=? AND date<?",
                (group + from.toString() + until.toString()).toTypedArray(), null)?.use { total += it.count }
        }
        runCatching {
            mmsThreads(context) { smsSenderKey(it) in keys }.keys.toList().chunked(400).forEach { group ->
                context.contentResolver.query(Telephony.Mms.CONTENT_URI, arrayOf("_id"), "thread_id IN (${group.joinToString(",") { "?" }}) AND msg_box=? AND date>=? AND date<?",
                    (group.map(Long::toString) + Telephony.Mms.MESSAGE_BOX_INBOX.toString() + (from / 1000).toString() + (until / 1000).toString()).toTypedArray(), null)?.use { total += it.count }
            }
        }
        return total
    }

    /**
     * The chosen senders' texts on the phone received from [from] until before [until], SMS and
     * text-only MMS, newest first, without collecting them: what the assistant searches. Only the
     * senders chosen for [budget] (or [sender] among them), never another's.
     */
    fun search(context: Context, budget: String, from: Long, until: Long, sender: String? = null): List<BankSms> {
        check(canRead(context)) { context.getString(R.string.sms_permission_needed) }
        val sources = SmsInbox.get(context).sources(budget).filter { sender == null || smsSenderKey(it.sender) == smsSenderKey(sender) }
        if (sources.isEmpty()) return emptyList()
        val keys = sources.map { smsSenderKey(it.sender) }.toSet()
        val found = mutableListOf<BankSms>()
        val distinct = hashSetOf<String>()
        val matching = (addresses(context).filter { smsSenderKey(it) in keys } + sources.map { it.sender }).distinct()
        matching.chunked(400).forEach { group ->
            context.contentResolver.query(Telephony.Sms.Inbox.CONTENT_URI, arrayOf(Telephony.Sms.ADDRESS, Telephony.Sms.DATE, Telephony.Sms.DATE_SENT, Telephony.Sms.BODY),
                "address IN (${group.joinToString(",") { "?" }}) AND date>=? AND date<?", (group + from.toString() + until.toString()).toTypedArray(), "date DESC")
                ?.use { c ->
                    while (c.moveToNext()) {
                        val address = c.getString(0) ?: continue
                        val received = c.getLong(1)
                        val sent = c.getLong(2).takeIf { it > 0 } ?: received
                        val body = c.getString(3) ?: continue
                        BankSms(smsId(address, sent, body), address, received, body).let { if (distinct.add(it.id)) found += it }
                    }
                } ?: error(context.getString(R.string.sms_read_failed))
        }
        runCatching {
            mms(context, mmsThreads(context) { smsSenderKey(it) in keys }, from, until) { address, received, sent, text ->
                if (text != null && received in from until until) BankSms(smsId(address, sent, text), address, received, text).let { if (distinct.add(it.id)) found += it }
            }
        }
        return found.sortedByDescending { it.received }
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
