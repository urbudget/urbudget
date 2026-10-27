package app.urbudget.nativeapp

import android.content.Context
import android.util.AtomicFile
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.FileNotFoundException
import java.time.YearMonth

interface BudgetStorage {
    fun read(): String?
    /** Must either durably replace the entire value or leave the previous value readable. */
    fun write(json: String)
    fun retainRecovery(json: String) {}
    fun readRecovery(): String? = null
    fun hasRecovery(): Boolean = false
    fun create(json: String) { write(json) }
    val identity: String? get() = null
    val revision: String? get() = null
    val managesHistory: Boolean get() = false
    val canUndo: Boolean get() = false
    val canRedo: Boolean get() = false
    fun undo() {}
    fun redo() {}
}

class AtomicBudgetStorage(context: Context, directory: File = context.filesDir) : BudgetStorage {
    private val file = AtomicFile(File(directory, "budget.json"))
    private val recovery = AtomicFile(File(directory, "budget-before-replacement.json"))
    override fun read(): String? = try {
        file.openRead().bufferedReader(Charsets.UTF_8).use { it.readText() }
    } catch (error: FileNotFoundException) {
        if (file.baseFile.exists()) throw error else null
    }

    override fun write(json: String) {
        writeAtomic(file, json)
    }

    override fun retainRecovery(json: String) { writeAtomic(recovery, json) }
    override fun readRecovery(): String? = if (hasRecovery()) recovery.openRead().bufferedReader(Charsets.UTF_8).use { it.readText() } else null
    override fun hasRecovery(): Boolean = recovery.baseFile.exists()

    private fun writeAtomic(target: AtomicFile, json: String) {
        val stream = target.startWrite()
        try {
            stream.write(json.toByteArray(Charsets.UTF_8))
            stream.fd.sync()
        } catch (error: Exception) {
            target.failWrite(stream)
            throw error
        }
        // AtomicFile owns the final rename. Do not perform fallible work after that commit:
        // a post-commit read error must never report that the old file is still current.
        target.finishWrite(stream)
    }
}

data class BudgetSnapshot(
    val budget: JSONObject?,
    val summary: JSONObject?,
    val reports: JSONObject?,
    val month: String,
    val undoAvailable: Boolean,
    val recoveryAvailable: Boolean = false,
    val redoAvailable: Boolean = false,
    val identity: String? = null,
    val revision: String? = null,
)

/**
 * Serial transactional boundary around the shared engine. JSON is the portable desktop format:
 * unknown fields survive imports and local record edits. A new snapshot is published only after
 * validation, calculation and an atomic, durable write have all succeeded.
 */
class BudgetRepository(
    private val storage: BudgetStorage,
    private val engine: BudgetEngine = BudgetEngine { NativeBridge.invoke(it) },
    initialMonth: String = YearMonth.now().toString(),
) {
    private val mutex = Mutex()
    private var current: String? = null
    private var month: String = initialMonth
    private val history = ArrayDeque<String>()
    private val future = ArrayDeque<String>()
    @Volatile private var exported: String? = null
    @Volatile private var exportedIdentity: String? = null
    @Volatile private var recoveryReady: Boolean = false
    private var cachedJson: String? = null
    private var cachedIdentity: String? = null
    private var cachedBudget: JSONObject? = null
    private var cachedSummary: JSONObject? = null
    private var cachedMonth: String? = null
    private var cachedAsOfMonth: String? = null

    fun exportBudget(): String? = exported
    fun hasRecovery(): Boolean = recoveryReady
    fun replacementGuard(shown: JSONObject?, identity: String?): BudgetReplacementGuard =
        BudgetReplacementGuard(shown, if (shown == null) exported else null, if (shown != null) identity else exportedIdentity)

    suspend fun load(): BudgetSnapshot = mutex.withLock {
        recoveryReady = storage.hasRecovery()
        val saved = storage.read()
        // Preserve the raw bytes for a recovery export even if a newer schema or corruption
        // prevents the shared engine from opening this file.
        current = saved
        exported = saved
        exportedIdentity = storage.identity
        snapshot(saved, month)
    }

    suspend fun selectMonth(value: String): BudgetSnapshot = mutex.withLock {
        require(YearMonth.parse(value).toString() == value) { Words[R.string.model_choose_valid_month] }
        val snapshot = snapshot(current, value)
        month = value
        snapshot
    }

    suspend fun create(name: String, currency: String, sample: Boolean): BudgetSnapshot = mutex.withLock {
        val params = JSONObject().put("name", name.trim()).put("currency", currency.uppercase()).put("month", month)
        val value = call(if (sample) "sampleBudget" else "defaultBudget", null, params) as? JSONObject
            ?: error(Words[R.string.model_invalid_budget])
        persist(value, retainPrevious = true, createNew = true)
    }

    suspend fun importBudget(json: String, guard: BudgetReplacementGuard? = null): BudgetSnapshot = mutex.withLock {
        guard?.verify(current, storage.identity)
        require(json.toByteArray(Charsets.UTF_8).size <= MAX_IMPORT_BYTES) { Words[R.string.model_backup_too_large] }
        val root = JSONObject(json.removePrefix("\uFEFF"))
        // Desktop JSON backups are bare budgets; also accept the documented versioned export envelope.
        val value = if (root.has("schemaVersion")) root else root.optJSONObject("budget")
            ?: throw IllegalArgumentException(Words[R.string.model_not_a_backup])
        persist(value, retainPrevious = true)
    }

    suspend fun edit(guard: BudgetEditGuard? = null, transform: (JSONObject) -> Unit): BudgetSnapshot = mutex.withLock {
        val budget = budget()
        guard?.verify(budget, storage.identity)
        transform(budget)
        persist(budget)
    }

    suspend fun command(method: String, params: JSONObject, guard: BudgetEditGuard? = null): BudgetSnapshot = mutex.withLock {
        val currentBudget = budget()
        guard?.verify(currentBudget, storage.identity)
        val value = call(method, currentBudget, params) as? JSONObject
            ?: throw IllegalArgumentException(Words[R.string.model_no_editable_budget])
        val changed = value.optJSONObject("budget") ?: value
        require(changed.has("schemaVersion")) { Words[R.string.model_no_editable_budget] }
        persist(changed)
    }

    /** An edit, then commands each on the last one's result, saved as one change (one undo). */
    suspend fun editThenCommands(guard: BudgetEditGuard?, transform: (JSONObject) -> Unit, commands: List<Pair<String, JSONObject>>): BudgetSnapshot = mutex.withLock {
        var changed = budget()
        guard?.verify(changed, storage.identity)
        transform(changed)
        for ((method, params) in commands) {
            val value = call(method, changed, params) as? JSONObject
                ?: throw IllegalArgumentException(Words[R.string.model_no_editable_budget])
            changed = value.optJSONObject("budget") ?: value
            require(changed.has("schemaVersion")) { Words[R.string.model_no_editable_budget] }
        }
        persist(changed)
    }

    suspend fun verify(guard: BudgetEditGuard) = mutex.withLock { guard.verify(budget(), storage.identity) }

    // Published snapshots are read-only. Queries can calculate against the captured snapshot
    // without holding the persistence mutex or delaying a later edit behind a report request.
    fun querySnapshot(method: String, params: JSONObject, budget: JSONObject?): Any? = call(method, budget, params)

    suspend fun undo(): BudgetSnapshot = mutex.withLock {
        if (storage.managesHistory) {
            storage.undo()
            current = storage.read(); exported = current; exportedIdentity = storage.identity
            return@withLock snapshot(current, month)
        }
        val previous = history.lastOrNull() ?: return@withLock snapshot(current, month)
        val prepared = snapshot(previous, month)
        storage.write(previous)
        current?.let(future::addLast)
        history.removeLast()
        current = previous
        exported = previous
        prepared.copy(undoAvailable = history.isNotEmpty(), redoAvailable = future.isNotEmpty())
    }

    suspend fun redo(): BudgetSnapshot = mutex.withLock {
        if (storage.managesHistory) {
            storage.redo()
            current = storage.read(); exported = current; exportedIdentity = storage.identity
            return@withLock snapshot(current, month)
        }
        val next = future.lastOrNull() ?: return@withLock snapshot(current, month)
        val prepared = snapshot(next, month)
        storage.write(next)
        current?.let(history::addLast)
        future.removeLast(); current = next; exported = next
        prepared.copy(undoAvailable = history.isNotEmpty(), redoAvailable = future.isNotEmpty())
    }

    suspend fun restoreRecovery(guard: BudgetReplacementGuard? = null): BudgetSnapshot = mutex.withLock {
        guard?.verify(current, storage.identity)
        val previous = storage.readRecovery() ?: error(Words[R.string.model_no_recovery])
        persist(JSONObject(previous))
    }

    private fun budget(): JSONObject = current?.let(::JSONObject) ?: error(Words[R.string.model_no_budget])

    private fun persist(value: JSONObject, retainPrevious: Boolean = false, createNew: Boolean = false): BudgetSnapshot {
        val json = value.toString()
        val prepared = snapshot(json, month)
        if (retainPrevious) current?.let(storage::retainRecovery)
        recoveryReady = storage.hasRecovery()
        if (createNew) storage.create(json) else storage.write(json)
        cachedIdentity = storage.identity
        if (!storage.managesHistory) current?.takeIf { it != json }?.let {
            history.addLast(it)
            while (history.size > 20) history.removeFirst()
        }
        current = json
        exported = json
        exportedIdentity = storage.identity
        future.clear()
        return prepared.copy(undoAvailable = if (storage.managesHistory) storage.canUndo else history.isNotEmpty(), redoAvailable = storage.canRedo, recoveryAvailable = storage.hasRecovery(), identity = storage.identity, revision = storage.revision)
    }

    private fun snapshot(json: String?, month: String): BudgetSnapshot {
        if (json == null) {
            cachedJson = null; cachedBudget = null; cachedSummary = null; cachedMonth = null
            return BudgetSnapshot(null, null, null, month, history.isNotEmpty(), storage.hasRecovery(), identity = storage.identity, revision = storage.revision)
        }
        val asOf = YearMonth.now().toString()
        if (cachedBudget == null || cachedJson != json || cachedIdentity != storage.identity) {
            val parsed = JSONObject(json)
            cachedBudget = parsed; cachedJson = json; cachedIdentity = storage.identity
            cachedSummary = null; cachedMonth = null
        }
        val budget = cachedBudget!!
        if (cachedSummary == null || cachedMonth != month || cachedAsOfMonth != asOf) {
            // calculate validates through the shared engine before producing any totals.
            cachedSummary = call("calculate", budget, JSONObject().put("month", month).put("asOfMonth", asOf)) as JSONObject
            cachedMonth = month; cachedAsOfMonth = asOf
        }
        // Reports are computed when their screen requests its actual range and filters.
        return BudgetSnapshot(budget, cachedSummary, null, month, if (storage.managesHistory) storage.canUndo else history.isNotEmpty(), storage.hasRecovery(), if (storage.managesHistory) storage.canRedo else future.isNotEmpty(), storage.identity, storage.revision)
    }

    private fun call(method: String, budget: JSONObject?, params: JSONObject = JSONObject()): Any? {
        val request = JSONObject().put("version", 1).put("method", method).put("params", params)
        if (budget != null) request.put("budget", budget)
        val response = JSONObject(engine.invoke(request.toString()))
        if (!response.optBoolean("ok")) throw IllegalArgumentException(if (response.has("error")) response.optString("error") else Words[R.string.model_engine_failed])
        return response.opt("value").takeUnless { it == JSONObject.NULL }
    }

    companion object { const val MAX_IMPORT_BYTES = 32 * 1024 * 1024 }
}

internal fun JSONObject.records(key: String): List<JSONObject> {
    val array = optJSONArray(key) ?: return emptyList()
    return (0 until array.length()).mapNotNull(array::optJSONObject)
}

/** Merge edited fields into a record; never discard source metadata unknown to Android. */
internal fun JSONObject.upsertRecord(collection: String, patch: JSONObject) {
    require(collection in setOf("accounts", "categories", "transactions", "schedules", "loans", "payeeRules", "payees")) { "Unsupported record collection." }
    val rows = records(collection).toMutableList()
    val identity = if (collection == "loans") "accountId" else "id"
    val id = patch.optString(identity)
    require(id.isNotBlank()) { "Every record needs an ID." }
    val index = rows.indexOfFirst { it.optString(identity) == id }
    val merged = if (index >= 0) JSONObject(rows[index].toString()) else JSONObject()
    patch.keys().forEach { key -> merged.put(key, patch.get(key)) }
    if (index >= 0) rows[index] = merged else rows.add(merged)
    put(collection, JSONArray(rows))
}
