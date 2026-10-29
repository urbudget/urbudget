package app.urbudget.nativeapp

import android.content.Context
import org.json.JSONObject

/** Android shares desktop's revision-checked SQLite commits, automatic backups and sync bases. */
class SqliteBudgetStorage(private val context: Context) : BudgetStorage {
    private val legacy = AtomicBudgetStorage(context)
    private var token = ""
    private var isNew = true
    private var cached: String? = null
    private var hasSnapshot = false
    private val migration = context.getSharedPreferences("library-migration", Context.MODE_PRIVATE)
    override var identity: String? = null; private set
    override val revision: String? get() = token.takeIf(String::isNotBlank)
    override val managesHistory = true
    override var canUndo = false; private set
    override var canRedo = false; private set

    override fun read(): String? {
        NativeServices.initialize(context)
        if (hasSnapshot && !isNew) {
            val status = NativeServices.call("native_budget_status") as JSONObject
            if (status.getString("token") == token) { update(status); return cached }
        }
        var snapshot = NativeServices.call("load_budget") as JSONObject
        update(snapshot)
        if (isNew && !migration.getBoolean("complete", false) &&
            (NativeServices.call("library_snapshot") as JSONObject).optBoolean("firstRun", false)) {
            // Keep the original file for recovery. A failed migration never marks it complete.
            legacy.read()?.let { json ->
                // Give the repository the original bytes for a recovery export if validation
                // fails. A valid file still requires a durable SQLite commit before publishing.
                val parsed = runCatching { JSONObject(json) }.getOrNull() ?: return json
                val validation = JSONObject(NativeBridge.invoke(JSONObject().put("version", 1).put("method", "validate").put("budget", parsed).toString()))
                if (!validation.optBoolean("ok")) return json
                val id = NativeServices.call("create_budget", JSONObject().put("budget", parsed).put("localOnly", true)) as String
                snapshot = NativeServices.call("library_open", JSONObject().put("id", id)) as JSONObject
                update(snapshot)
            }
        }
        migration.edit().putBoolean("complete", true).commit()
        return if (isNew) null else cached
    }

    override fun write(json: String) {
        val answer = NativeServices.call("save_budget", JSONObject().put("budget", JSONObject(json)).put("expectedToken", token).put("ackOnly", true)) as JSONObject
        update(answer)
        cached = json; hasSnapshot = true
    }

    override fun create(json: String) { createWithScope(json, false) }
    override fun createLocal(json: String) { createWithScope(json, true) }
    private fun createWithScope(json: String, localOnly: Boolean) {
        val id = NativeServices.call("create_budget", JSONObject().put("budget", JSONObject(json)).put("localOnly", localOnly)) as String
        update(NativeServices.call("library_open", JSONObject().put("id", id)) as JSONObject)
    }

    override fun undo() { update(NativeServices.call("undo_budget", JSONObject().put("expectedToken", token)) as JSONObject) }
    override fun redo() { update(NativeServices.call("redo_budget", JSONObject().put("expectedToken", token)) as JSONObject) }
    override fun retainRecovery(json: String) = legacy.retainRecovery(json)
    override fun readRecovery() = legacy.readRecovery()
    override fun hasRecovery() = legacy.hasRecovery()

    private fun update(snapshot: JSONObject) {
        if (snapshot.has("budget")) {
            cached = snapshot.optJSONObject("budget")?.toString()
            hasSnapshot = true
        }
        token = snapshot.getString("token")
        identity = snapshot.optString("databasePath").ifBlank { null }
        isNew = snapshot.optBoolean("isNew")
        canUndo = snapshot.optBoolean("undoAvailable")
        canRedo = snapshot.optBoolean("redoAvailable")
    }
}
