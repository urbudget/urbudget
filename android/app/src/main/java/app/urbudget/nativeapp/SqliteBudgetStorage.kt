package app.urbudget.nativeapp

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/** Android shares desktop's revision-checked SQLite commits, automatic backups and sync bases. */
class SqliteBudgetStorage(private val context: Context) : BudgetStorage {
    private val legacy = AtomicBudgetStorage(context)
    private var token = ""
    private var isNew = true
    private var hasSnapshot = false
    private val migration = context.getSharedPreferences("library-migration", Context.MODE_PRIVATE)
    override var identity: String? = null; private set
    override val revision: String? get() = token.takeIf(String::isNotBlank)
    override val managesHistory = true
    override var canUndo = false; private set
    override var canRedo = false; private set
    override val editsInStore = true

    override fun open(): StoredBudget = traced("storage.open") {
        NativeServices.initialize(context)
        if (hasSnapshot && !isNew) {
            val status = NativeServices.call("native_budget_status") as JSONObject
            if (status.getString("token") == token) { update(status); return@traced StoredBudget.Unchanged }
        }
        var snapshot = loadLight()
        update(snapshot)
        if (isNew && !migration.getBoolean("complete", false) &&
            (NativeServices.call("library_snapshot") as JSONObject).optBoolean("firstRun", false)) {
            // Keep the original file for recovery. A failed migration never marks it complete.
            legacy.read()?.let { json ->
                // Give the repository the original bytes for a recovery export if validation
                // fails. A valid file still requires a durable SQLite commit before publishing.
                val parsed = runCatching { JSONObject(json) }.getOrNull() ?: return@traced StoredBudget.Text(json)
                val validation = JSONObject(NativeBridge.invoke(JSONObject().put("version", 1).put("method", "validate").put("budget", parsed).toString()))
                if (!validation.optBoolean("ok")) return@traced StoredBudget.Text(json)
                val id = NativeServices.call("create_budget", JSONObject().put("budget", parsed).put("localOnly", true)) as String
                NativeServices.call("library_open", JSONObject().put("id", id))
                // library_open's reply has no sourceExport: never adopt it.
                snapshot = loadLight()
                update(snapshot)
            }
        }
        migration.edit().putBoolean("complete", true).commit()
        snapshot.optJSONObject("budget")?.takeUnless { isNew }?.let(StoredBudget::Parsed) ?: StoredBudget.Text(null)
    }

    /** The light copy: what the app keeps of a budget. The rest stays in the store, and `complete` reads it. */
    private fun loadLight() = NativeServices.call("load_budget", JSONObject().put("light", true)) as JSONObject

    // Never the light copy: anything read here could be written back whole.
    override fun read(): String? = when (val stored = open()) {
        is StoredBudget.Text -> stored.json
        else -> complete()
    }

    override fun invalidate() { hasSnapshot = false; token = "" }
    // Null once sync has closed the budget: there's nothing to copy, and that must not stop a new one.
    override fun complete() = NativeServices.call("export_budget", JSONObject().put("compact", true)) as String?

    override fun write(json: String) = traced("storage.write") {
        val params = "{\"budget\":$json,\"expectedToken\":${JSONObject.quote(token)},\"ackOnly\":true}"
        val answer = NativeServices.callText("save_budget", params) as JSONObject
        update(answer)
        hasSnapshot = true
    }

    override fun edit(steps: JSONArray, month: String, asOfMonth: String): BudgetEdit = traced("storage.edit") {
        // The storage's own token, not the shown one: a queued second tap would conflict with the first.
        val reply = NativeServices.call("edit_budget", JSONObject().put("steps", steps)
            .put("expectedToken", token).put("month", month).put("asOfMonth", asOfMonth).put("light", true)) as JSONObject
        update(reply)
        BudgetEdit(reply.getJSONObject("patch"), reply.optJSONObject("summary"))
    }
    override fun query(method: String, params: JSONObject, revision: String?): Any? = traced("storage.query") {
        try {
            NativeServices.call("query_budget", JSONObject().put("method", method).put("params", params)
                .put("expectedToken", revision ?: JSONObject.NULL))
        } catch (refused: IllegalStateException) {
            if (refused.message?.startsWith("CONFLICT:") == true) throw StaleBudget(Words[R.string.model_changed_preview]) else throw refused
        }
    }
    override fun undoEdit(month: String, asOfMonth: String) = historyEdit("undo_budget", month, asOfMonth)
    override fun redoEdit(month: String, asOfMonth: String) = historyEdit("redo_budget", month, asOfMonth)
    private fun historyEdit(method: String, month: String, asOfMonth: String): BudgetEdit = traced("storage.historyEdit") {
        val reply = NativeServices.call(method, JSONObject().put("patchOnly", true).put("expectedToken", token)
            .put("month", month).put("asOfMonth", asOfMonth).put("light", true)) as JSONObject
        update(reply)
        BudgetEdit(reply.getJSONObject("patch"), reply.optJSONObject("summary"))
    }

    override fun create(json: String) { createWithScope(json, false) }
    override fun createLocal(json: String) { createWithScope(json, true) }
    private fun createWithScope(json: String, localOnly: Boolean) {
        val id = NativeServices.call("create_budget", JSONObject().put("budget", JSONObject(json)).put("localOnly", localOnly)) as String
        update(NativeServices.call("library_open", JSONObject().put("id", id)) as JSONObject)
    }

    override fun retainRecovery(json: String) = legacy.retainRecovery(json)
    override fun readRecovery() = legacy.readRecovery()
    override fun hasRecovery() = legacy.hasRecovery()

    private fun update(snapshot: JSONObject) = traced("storage.update") {
        if (snapshot.has("budget")) hasSnapshot = true
        token = snapshot.getString("token")
        identity = snapshot.optString("databasePath").ifBlank { null }
        isNew = snapshot.optBoolean("isNew")
        canUndo = snapshot.optBoolean("undoAvailable")
        canRedo = snapshot.optBoolean("redoAvailable")
    }
}
