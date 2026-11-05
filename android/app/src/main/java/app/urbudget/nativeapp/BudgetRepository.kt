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

/** What's stored now, as `load` needs it. */
sealed interface StoredBudget {
    /** The same revision the storage last gave or wrote. */
    data object Unchanged : StoredBudget
    class Parsed(val budget: JSONObject) : StoredBudget
    /** Text, kept exact; null when there's no budget. */
    class Text(val json: String?) : StoredBudget
}

class BudgetEdit(val patch: JSONObject, val summary: JSONObject?)

interface BudgetStorage {
    fun read(): String?
    /** Must either durably replace the entire value or leave the previous value readable. */
    fun write(json: String)
    fun retainRecovery(json: String) {}
    fun readRecovery(): String? = null
    fun hasRecovery(): Boolean = false
    fun create(json: String) { write(json) }
    fun createLocal(json: String) { create(json) }
    val identity: String? get() = null
    val revision: String? get() = null
    val managesHistory: Boolean get() = false
    val canUndo: Boolean get() = false
    val canRedo: Boolean get() = false
    /** Edits run on the engine's own copy (`edit_budget`), so this storage needs no whole budgets. */
    val editsInStore: Boolean get() = false
    fun open(): StoredBudget = StoredBudget.Text(read())
    /** Forget the revision, so the next `open` reads the budget. */
    fun invalidate() {}
    /** The whole stored budget as compact text, on a worker: exports and recovery copies. Null when none is open. */
    fun complete(): String? = read()
    fun edit(steps: JSONArray, month: String, asOfMonth: String): BudgetEdit = error("Not an engine-edit storage")
    fun undoEdit(month: String, asOfMonth: String): BudgetEdit = error("Not an engine-edit storage")
    fun redoEdit(month: String, asOfMonth: String): BudgetEdit = error("Not an engine-edit storage")
    /** A read or a dry run on the store's copy. With `revision`, throws `StaleBudget` if that's no longer what's stored. */
    fun query(method: String, params: JSONObject, revision: String?): Any? = error("Not an engine-edit storage")
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

/** Refused by the store, with what it holds now already loaded. Publish it, then show `message`. Nothing was saved. */
internal class BudgetConflict(val snapshot: BudgetSnapshot, message: String) : IllegalStateException(message)

/** Saved, but it can't be shown as it should. `snapshot` is the best view of it, if there is one. Never a failed edit. */
internal class SavedNotShown(val snapshot: BudgetSnapshot?, message: String) : RuntimeException(message)

/** The store has moved past the revision this asked about, or closed the budget. Nothing changed. */
internal class StaleBudget(message: String) : IllegalArgumentException(message)

private fun Throwable.isConflict() = this is IllegalStateException && message?.startsWith("CONFLICT:") == true

internal fun commandStep(method: String, params: JSONObject) = JSONObject().put("method", method).put("params", params)
internal fun patchStep(patch: JSONObject) = JSONObject().put("patch", patch)

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
    /** Exact text: for storages without `editsInStore`, and for any storage, bytes that won't open. */
    @Volatile private var current: String? = null
    /**
     * The working copy, published as it stands and never changed afterwards. In the store's case it's
     * light, with no `source` or `sourceExport`, so it must never go back as a whole budget.
     */
    @Volatile private var root: JSONObject? = null
    private var month: String = initialMonth
    private val history = ArrayDeque<String>()
    private val future = ArrayDeque<String>()
    @Volatile private var exportedIdentity: String? = null
    @Volatile private var recoveryReady: Boolean = false
    /**
     * `root`'s exact text, or null. Never the text of an older budget: `persist` trusts it. `persist`
     * and `snapshot` set it, and every other line that moves `root` clears it.
     */
    private var cachedJson: String? = null
    private var cachedIdentity: String? = null
    private var cachedSummary: JSONObject? = null
    private var cachedMonth: String? = null
    private var cachedAsOfMonth: String? = null

    fun hasExport(): Boolean = root != null || current != null
    /** The whole budget as text. Blocks on the engine: never on the main thread. */
    fun exportBudget(): String? = current ?: root?.let { storage.complete() }
    fun hasRecovery(): Boolean = recoveryReady
    fun replacementGuard(shown: JSONObject?, identity: String?): BudgetReplacementGuard = BudgetReplacementGuard(
        shown ?: root, if (shown == null && root == null) current else null,
        if (shown != null) identity else exportedIdentity)

    suspend fun load(): BudgetSnapshot = mutex.withLock { reload() }

    /** `reloadOnce`, until rows and totals come from one revision. */
    private fun reload(): BudgetSnapshot {
        repeat(2) {
            try { return reloadOnce() } catch (moved: StaleBudget) { /* Sync committed between the read and its totals. `open` sees the new token. */ }
        }
        return reloadOnce()
    }

    /** What's stored now. The same revision keeps the same object: editors compare by identity. */
    private fun reloadOnce(): BudgetSnapshot {
        recoveryReady = storage.hasRecovery()
        val first = storage.open()
        val stored = if (first === StoredBudget.Unchanged && root == null) { storage.invalidate(); storage.open() } else first
        return when (stored) {
            StoredBudget.Unchanged -> { exportedIdentity = storage.identity; summarised(root, month) }
            is StoredBudget.Parsed -> {
                current = null; exportedIdentity = storage.identity
                // A token can move with no change at all (a second connection, v2.rs:651).
                val kept = root?.takeIf { cachedIdentity == storage.identity && sameBudget(it, stored.budget) }
                if (kept == null) { root = stored.budget; cachedJson = null; cachedSummary = null; cachedMonth = null }
                cachedIdentity = storage.identity
                summarised(root, month)
            }
            is StoredBudget.Text -> {
                // Keep the raw bytes for a recovery export even if the engine can't open this file.
                current = stored.json; exportedIdentity = storage.identity
                snapshot(stored.json, month)
            }
        }
    }

    suspend fun selectMonth(value: String): BudgetSnapshot = mutex.withLock {
        require(YearMonth.parse(value).toString() == value) { Words[R.string.model_choose_valid_month] }
        // Text: bytes that won't open, or a storage that keeps budgets as text.
        if (current != null) { val result = snapshot(current, value); month = value; return@withLock result }
        val previous = month
        month = value // settled's reload reads it.
        try { settled(root, value) } catch (failed: Exception) { month = previous; throw failed }
    }

    suspend fun create(name: String, currency: String, sample: Boolean, localOnly: Boolean = sample): BudgetSnapshot = mutex.withLock {
        val params = JSONObject().put("name", name.trim()).put("currency", currency.uppercase()).put("month", month)
        val value = call(if (sample) "sampleBudget" else "defaultBudget", null, params) as? JSONObject
            ?: error(Words[R.string.model_invalid_budget])
        persist(value, retainPrevious = true, createNew = true, localOnly = localOnly)
    }

    suspend fun importBudget(json: String, guard: BudgetReplacementGuard? = null): BudgetSnapshot = mutex.withLock {
        guard?.verify(root, current, storage.identity)
        require(json.toByteArray(Charsets.UTF_8).size <= MAX_IMPORT_BYTES) { Words[R.string.model_backup_too_large] }
        val parsed = JSONObject(json.removePrefix("\uFEFF"))
        // Desktop JSON backups are bare budgets; also accept the documented versioned export envelope.
        val value = if (parsed.has("schemaVersion")) parsed else parsed.optJSONObject("budget")
            ?: throw IllegalArgumentException(Words[R.string.model_not_a_backup])
        replacing(guard) { persist(value, retainPrevious = true) }
    }

    suspend fun edit(guard: BudgetEditGuard? = null, transform: (JSONObject) -> Unit): BudgetSnapshot = mutex.withLock {
        traced("repo.edit") {
            if (storage.editsInStore) return@traced submit(guard) { base -> edited(base, transform)?.let { JSONArray().put(patchStep(it)) } }
            val budget = budget()
            guard?.verify(budget, storage.identity)
            transform(budget)
            persist(budget)
        }
    }

    suspend fun command(method: String, params: JSONObject, guard: BudgetEditGuard? = null): BudgetSnapshot = mutex.withLock {
        traced("repo.command") {
            if (storage.editsInStore) return@traced submit(guard) { JSONArray().put(commandStep(method, params)) }
            val currentBudget = budget()
            guard?.verify(currentBudget, storage.identity)
            val value = call(method, currentBudget, params) as? JSONObject
                ?: throw IllegalArgumentException(Words[R.string.model_no_editable_budget])
            val changed = value.optJSONObject("budget") ?: value
            require(changed.has("schemaVersion")) { Words[R.string.model_no_editable_budget] }
            persist(changed)
        }
    }

    /** An edit, then commands each on the last one's result, saved as one change (one undo). */
    suspend fun editThenCommands(guard: BudgetEditGuard?, transform: (JSONObject) -> Unit, commands: List<Pair<String, JSONObject>>): BudgetSnapshot = mutex.withLock {
        traced("repo.editThenCommands") {
            if (storage.editsInStore) return@traced submit(guard) { base ->
                val steps = JSONArray()
                edited(base, transform)?.let { steps.put(patchStep(it)) }
                commands.forEach { (method, params) -> steps.put(commandStep(method, params)) }
                steps.takeIf { it.length() > 0 }
            }
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
    }

    suspend fun verify(guard: BudgetEditGuard) = mutex.withLock {
        guard.verify(if (storage.editsInStore) root ?: error(Words[R.string.model_no_budget]) else budget(), storage.identity)
    }

    // Published snapshots are read-only. Queries can calculate against the captured snapshot
    // without holding the persistence mutex or delaying a later edit behind a report request.
    fun querySnapshot(method: String, params: JSONObject, budget: JSONObject?, revision: String? = null): Any? = when {
        method in BUDGET_FREE -> call(method, null, params)
        storage.editsInStore -> storage.query(method, params, revision)
        else -> call(method, budget, params)
    }

    suspend fun undo(): BudgetSnapshot = mutex.withLock {
        traced("repo.undo") {
            if (storage.editsInStore) return@traced historyStep(storage::undoEdit)
            val previous = history.lastOrNull() ?: return@traced snapshot(current, month)
            val prepared = snapshot(previous, month)
            storage.write(previous)
            current?.let(future::addLast)
            history.removeLast()
            current = previous
            prepared.copy(undoAvailable = history.isNotEmpty(), redoAvailable = future.isNotEmpty())
        }
    }

    suspend fun redo(): BudgetSnapshot = mutex.withLock {
        traced("repo.redo") {
            if (storage.editsInStore) return@traced historyStep(storage::redoEdit)
            val next = future.lastOrNull() ?: return@traced snapshot(current, month)
            val prepared = snapshot(next, month)
            storage.write(next)
            current?.let(history::addLast)
            future.removeLast(); current = next
            prepared.copy(undoAvailable = history.isNotEmpty(), redoAvailable = future.isNotEmpty())
        }
    }

    suspend fun restoreRecovery(guard: BudgetReplacementGuard? = null): BudgetSnapshot = mutex.withLock {
        guard?.verify(root, current, storage.identity)
        val value = JSONObject(storage.readRecovery() ?: error(Words[R.string.model_no_recovery]))
        replacing(guard) { persist(value) }
    }

    /** `transform`'s change to a shallow copy of `base`, or null if it made none. */
    private fun edited(base: JSONObject, transform: (JSONObject) -> Unit): JSONObject? {
        val copy = base.shallowCopy()
        transform(copy)
        return budgetPatch(base, copy).takeIf { it.length() > 0 }
    }

    /** A private copy for a storage that keeps text, so a transform there may change it freely. */
    private fun budget(): JSONObject = current?.let(::JSONObject) ?: error(Words[R.string.model_no_budget])

    private fun persist(value: JSONObject, retainPrevious: Boolean = false, createNew: Boolean = false, localOnly: Boolean = false): BudgetSnapshot = traced("repo.persist") {
        val json = value.toString()
        // An unchanged budget keeps its object: editors compare budgets by identity.
        val same = root != null && cachedJson == json && cachedIdentity == storage.identity
        val budget = if (same) root!! else value
        val asOf = YearMonth.now().toString()
        // Stateless: `value` isn't in the store yet, and it must validate before anything is written.
        val summary = cachedSummary?.takeIf { same && cachedMonth == month && cachedAsOfMonth == asOf }
            ?: call("calculate", budget, JSONObject().put("month", month).put("asOfMonth", asOf)) as JSONObject
        // Null with no budget open. A real failure stops here: the copy would be lost.
        if (retainPrevious) exportBudget()?.let(storage::retainRecovery)
        recoveryReady = storage.hasRecovery()
        if (createNew && localOnly) storage.createLocal(json) else if (createNew) storage.create(json) else storage.write(json)
        if (storage.editsInStore) {
            cachedJson = null // The text is the whole budget; what's kept from here is the light one.
            storage.invalidate()
            // The write is durable, so this can't read as a failed one: `retry()` would write it again.
            return@traced shownAfterSave(null) { reload() }
        }
        // All of this after the write, so a failed write changes nothing.
        root = budget; cachedJson = json; cachedIdentity = storage.identity
        cachedSummary = summary; cachedMonth = month; cachedAsOfMonth = asOf
        if (!storage.managesHistory) current?.takeIf { it != json }?.let {
            history.addLast(it)
            while (history.size > 20) history.removeFirst()
        }
        current = json
        exportedIdentity = storage.identity
        future.clear()
        published(budget, summary, month)
    }

    private fun snapshot(json: String?, month: String): BudgetSnapshot = traced("repo.snapshot") {
        if (json == null) {
            cachedJson = null; root = null; cachedSummary = null; cachedMonth = null
            return@traced summarised(null, month)
        }
        val reuse = root != null && cachedJson == json && cachedIdentity == storage.identity
        val budget = if (reuse) root!! else JSONObject(json)
        if (!reuse) { cachedSummary = null; cachedMonth = null }
        val result = summarised(budget, month, stored = false) // Throws for a budget the engine refuses.
        root = budget; cachedJson = json; cachedIdentity = storage.identity
        result
    }

    /**
     * Totals for `budget`. `stored`: it's what the store holds, so they're asked of the store, at the
     * revision they'll be published with, and `StaleBudget` means it has moved on.
     */
    private fun summarised(budget: JSONObject?, month: String, stored: Boolean = storage.editsInStore): BudgetSnapshot {
        if (budget == null) return BudgetSnapshot(null, null, null, month, history.isNotEmpty(), storage.hasRecovery(), identity = storage.identity, revision = storage.revision)
        val asOf = YearMonth.now().toString()
        if (cachedSummary == null || cachedMonth != month || cachedAsOfMonth != asOf) {
            val params = JSONObject().put("month", month).put("asOfMonth", asOf)
            // calculate validates through the shared engine before producing any totals.
            cachedSummary = (if (stored) storage.query("calculate", params, storage.revision) else call("calculate", budget, params)) as JSONObject
            cachedMonth = month; cachedAsOfMonth = asOf
        }
        return published(budget, cachedSummary, month)
    }

    /** Totals for what's shown. If the store has moved on, load it, so rows and totals move together. */
    private fun settled(budget: JSONObject?, month: String) = try { summarised(budget, month) } catch (moved: StaleBudget) { reload() }

    // Reports are computed when their screen requests its actual range and filters.
    private fun published(budget: JSONObject, summary: JSONObject?, month: String) = BudgetSnapshot(budget, summary, null, month,
        if (storage.managesHistory) storage.canUndo else history.isNotEmpty(), storage.hasRecovery(),
        if (storage.managesHistory) storage.canRedo else future.isNotEmpty(), storage.identity, storage.revision)

    /**
     * One edit, run in the store. `steps` builds it from a budget, or gives null for no change: a
     * patch holds whole records and arrays, so after a refusal it's rebuilt on what's stored, never
     * resent.
     */
    private fun submit(guard: BudgetEditGuard?, steps: (JSONObject) -> JSONArray?): BudgetSnapshot {
        val base = root ?: error(Words[R.string.model_no_budget])
        guard?.verify(base, storage.identity)
        val plan = steps(base) ?: return settled(base, month)
        val asOf = YearMonth.now().toString()
        val first = try { storage.edit(plan, month, asOf) } catch (refused: IllegalStateException) {
            if (!refused.isConflict()) throw refused
            null
        }
        if (first != null) return applied(base, first, asOf)
        // Something else wrote and nothing of this edit was saved. Once more on what's stored now.
        val fresh = reload()
        val (next, second) = try {
            val next = fresh.budget ?: error(Words[R.string.model_no_budget])
            guard?.verify(next, storage.identity)
            val again = steps(next) ?: return fresh
            next to storage.edit(again, month, asOf)
        } catch (failed: Exception) {
            throw BudgetConflict(fresh, failed.message ?: Words[R.string.model_action_not_saved])
        }
        return applied(next, second, asOf) // Outside the try: from here it's saved, not a conflict.
    }

    /**
     * The edit is saved. From here nothing may read as a failed edit: the screen would stay on the
     * budget from before it, and `retry()` would apply it twice. Whatever fails is `SavedNotShown`.
     * Never call this inside a try that turns failures into something else.
     */
    private fun applied(base: JSONObject, edit: BudgetEdit, asOf: String): BudgetSnapshot {
        val next = try { applyBudgetPatch(base, edit.patch) } catch (broken: Exception) {
            storage.invalidate()
            return shownAfterSave(null) { reload() }
        }
        if (next !== base) { root = next; cachedJson = null }
        cachedIdentity = storage.identity
        val keep = next === base && cachedSummary != null && cachedMonth == month && cachedAsOfMonth == asOf
        if (!keep) { cachedSummary = edit.summary; cachedMonth = month.takeIf { edit.summary != null }; cachedAsOfMonth = asOf }
        return shownAfterSave(next) { settled(next, month) }
    }

    /** `budget` without totals rather than with another revision's. Every screen takes a null summary. */
    private inline fun shownAfterSave(budget: JSONObject?, show: () -> BudgetSnapshot): BudgetSnapshot = try { show() } catch (failed: Exception) {
        throw SavedNotShown(budget?.let { published(it, null, month) }, failed.message ?: Words[R.string.model_engine_failed])
    }

    /**
     * A whole budget replaces what's stored, whatever that is. After a refusal it goes again once, if
     * the guard still holds on what's stored now. `persist` changes nothing before its write but the
     * recovery copy, which should be of what's being replaced.
     */
    private fun replacing(guard: BudgetReplacementGuard?, write: () -> BudgetSnapshot): BudgetSnapshot {
        try { return write() } catch (refused: IllegalStateException) { if (!refused.isConflict()) throw refused }
        val fresh = reload()
        try {
            guard?.verify(root, current, storage.identity)
            return write()
        } catch (saved: SavedNotShown) {
            throw saved // `persist` says so once the file is in.
        } catch (failed: Exception) {
            throw BudgetConflict(fresh, failed.message ?: Words[R.string.model_action_not_saved])
        }
    }

    private fun historyStep(run: (String, String) -> BudgetEdit): BudgetSnapshot {
        val base = root ?: error(Words[R.string.model_no_budget])
        val asOf = YearMonth.now().toString()
        val edit = try { run(month, asOf) } catch (refused: IllegalStateException) {
            if (refused.isConflict()) throw BudgetConflict(reload(), refused.message!!) else throw refused
        }
        return applied(base, edit, asOf)
    }

    private fun call(method: String, budget: JSONObject?, params: JSONObject = JSONObject()): Any? = traced("repo.call:$method") {
        val request = JSONObject().put("version", 1).put("method", method).put("params", params)
        if (budget != null) request.put("budget", budget)
        val response = JSONObject(engine.invoke(request.toString()))
        if (!response.optBoolean("ok")) throw IllegalArgumentException(if (response.has("error")) response.optString("error") else Words[R.string.model_engine_failed])
        response.opt("value").takeUnless { it == JSONObject.NULL }
    }

    companion object {
        const val MAX_IMPORT_BYTES = 32 * 1024 * 1024
        /** Methods the engine answers from `params` alone. */
        private val BUDGET_FREE = setOf("loanProjection", "loanPaymentByDate", "importYnabFiles")
    }
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
