package app.urbudget.nativeapp

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.isActive
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.coroutines.resume

data class BudgetUiState(
    val loading: Boolean = true,
    val busy: Boolean = false,
    val budget: JSONObject? = null,
    val summary: JSONObject? = null,
    val ledger: JSONObject? = null,
    val reports: JSONObject? = null,
    val month: String = YearMonth.now().toString(),
    val error: String? = null,
    val undoAvailable: Boolean = false,
    val recoveryAvailable: Boolean = false,
    val redoAvailable: Boolean = false,
    val budgetIdentity: String? = null,
    val budgetRevision: String? = null,
    /** How the last export went, until it's been said. A publish leaves these: the refresh after sync closes a budget would take an `error` away unseen. */
    val exported: Boolean = false,
    val exportFailure: String? = null,
)

class BudgetViewModel internal constructor(application: Application, private val repository: BudgetRepository) : AndroidViewModel(application) {
    constructor(application: Application) : this(application, BudgetRepository(SqliteBudgetStorage(application)))
    private val mutableState = MutableStateFlow(BudgetUiState())
    val state: StateFlow<BudgetUiState> = mutableState.asStateFlow()
    private val operations = Channel<Pair<Boolean, suspend () -> Unit>>(Channel.UNLIMITED)
    private val queryWorkers = Semaphore(2)
    private val refreshQueued = AtomicBoolean(false)
    var operationFailureEpoch = 0L; private set
    private var retryOperation: (suspend () -> Unit)? = null
    /** Display models screens build off the main thread, kept so revisiting a tab shows them at once. */
    internal val screenCache = java.util.concurrent.ConcurrentHashMap<String, Any>()

    init {
        viewModelScope.launch {
            for ((showBusy, operation) in operations) {
                if (showBusy) mutableState.update { it.copy(busy = true) }
                try {
                    withContext(Dispatchers.IO) { operation() }
                } catch (cancel: CancellationException) {
                    throw cancel
                } catch (error: Throwable) {
                    // publish clears the error and the retry, so it goes first.
                    when (error) {
                        is BudgetConflict -> publish(error.snapshot)
                        is SavedNotShown -> error.snapshot?.let { publish(it) }
                    }
                    // Saved is saved: nothing to retry, and no failure for an editor to keep its draft over.
                    if (error is SavedNotShown) retryOperation = null else { operationFailureEpoch++; retryOperation = operation }
                    mutableState.update { it.copy(error = error.message ?: Words[R.string.model_action_not_saved], recoveryAvailable = repository.hasRecovery()) }
                    if (error is SavedNotShown) refresh()
                } finally {
                    mutableState.update { it.copy(busy = false, loading = false) }
                }
            }
        }
        enqueue { publish(repository.load()) }
    }

    private fun enqueue(showBusy: Boolean = true, operation: suspend () -> Unit) {
        operations.trySend(showBusy to operation)
    }

    fun afterPending(action: () -> Unit) { enqueue(showBusy = false) { withContext(Dispatchers.Main) { action() } } }

    private suspend fun publish(snapshot: BudgetSnapshot) = withContext(Dispatchers.Main.immediate) {
        traced("vm.publish") {
            // Configure before emission so the first frame and amount editors use this
            // snapshot's preferences. Unchanged loads/month changes reuse the same object.
            if (snapshot.budget !== mutableState.value.budget) snapshot.budget?.let(Money::configure)
            retryOperation = null
            mutableState.update { it.copy(budget = snapshot.budget, summary = snapshot.summary, reports = snapshot.reports,
                month = snapshot.month, undoAvailable = snapshot.undoAvailable, recoveryAvailable = snapshot.recoveryAvailable, redoAvailable = snapshot.redoAvailable, budgetIdentity = snapshot.identity, budgetRevision = snapshot.revision, error = null, loading = false) }
        }
    }

    fun notifyError(message: String) { mutableState.update { it.copy(error = message) } }
    fun dismissError() { mutableState.update { it.copy(error = null, exportFailure = null) } }
    fun retry() { retryOperation?.let { enqueue(operation = it) } ?: refresh() }
    fun createBudget(name: String, currency: String, sample: Boolean = false, localOnly: Boolean = sample) = enqueue { publish(repository.create(name, currency, sample, localOnly)) }
    fun importBudget(json: String) {
        val shown = state.value
        val guard = repository.replacementGuard(shown.budget, shown.budgetIdentity)
        enqueue { publish(repository.importBudget(json, guard)) }
    }
    fun hasExport(): Boolean = repository.hasExport()
    /**
     * Writes the whole budget with `write`, after any queued edits, and leaves how it went in the state.
     * The picker has made the file by now. So this can't run in a screen's scope, where a rotation would
     * leave it empty with nothing said, and it runs off the queue, so a slow file provider holds up no edit.
     */
    fun exportBudget(write: (String) -> Unit) = enqueue {
        // Null when sync has closed the budget and the refresh that says so hasn't come yet.
        val read = runCatching { repository.exportBudget() ?: error(Words[R.string.backup_save_failed]) }
        viewModelScope.launch(Dispatchers.IO) {
            val failure = read.mapCatching(write).exceptionOrNull()
            mutableState.update { it.copy(exported = failure == null, exportFailure = failure?.let { failed -> failed.message ?: Words[R.string.backup_save_failed] }) }
        }
    }
    fun exportSaid() { mutableState.update { it.copy(exported = false) } }
    /** Blocks on the engine: for tests and workers. */
    internal fun exportBudget(): String? = repository.exportBudget()
    fun selectMonth(month: String) = enqueue { publish(repository.selectMonth(month)) }
    private var consumedSyncReport: String? = null
    @Synchronized fun refreshAfterSync(report: JSONObject) {
        val encoded = report.toString()
        if (encoded == consumedSyncReport) return
        val library = NativeServices.call("library_snapshot") as JSONObject
        val current = state.value
        consumedSyncReport = encoded
        if (syncInvalidatesActive(report, library, current.budgetIdentity, current.budgetRevision)) refresh()
    }
    fun refresh() {
        if (refreshQueued.compareAndSet(false, true)) enqueue(showBusy = false) {
            refreshQueued.set(false)
            publish(repository.load())
        }
    }
    fun undo() = enqueue { publish(repository.undo()) }
    fun redo() = enqueue { publish(repository.redo()) }
    fun restorePreviousBudget() {
        val shown = state.value
        val guard = repository.replacementGuard(shown.budget, shown.budgetIdentity)
        enqueue { publish(repository.restoreRecovery(guard)) }
    }
    fun editBudget(transform: (JSONObject) -> Unit) {
        val shown = state.value
        val guard = BudgetEditGuard.whole(shown.budget, shown.budgetIdentity)
        enqueue { publish(repository.edit(guard, transform)) }
    }

    private fun recordGuard(collection: String, id: String): BudgetEditGuard {
        val shown = state.value
        return BudgetEditGuard.record(shown.budget, shown.budgetIdentity, collection, id)
    }

    fun service(method: String, params: JSONObject = JSONObject(), onError: (String) -> Unit = { notifyError(it) }, callback: (Any?) -> Unit = {}): Job {
        val shown = state.value
        val guard = if (method in setOf("simplefin_import", "sync_ynab", "accept_ynab_import", "resolve_ynab", "assistant_apply") && shown.budget != null) BudgetEditGuard.whole(shown.budget, shown.budgetIdentity) else null
        val independent = NativeServicePolicy.independent(method)
        suspend fun perform(active: () -> Boolean, canInvoke: () -> Boolean = active) {
            var invoked = false
            var syncReport: JSONObject? = null
            try {
                guard?.let { repository.verify(it) }
                NativeServices.initialize(getApplication())
                // Requests become read-only when submitted. Large bank choices/file payloads
                // are encoded on this worker, never while handling a tap on the main thread.
                val request = JSONObject(params.toString())
                if ((guard != null || NativeServicePolicy.review(method)) && !request.has("expectedToken")) shown.budgetRevision?.let { request.put("expectedToken", it) }
                if (!canInvoke()) return
                invoked = true
                val result = if (NativeServicePolicy.coalesce(method)) {
                    val scope = if (NativeServicePolicy.budgetRead(method)) "${shown.budgetIdentity}:${shown.budgetRevision}" else ""
                    NativeServices.read(method, request, scope)
                } else NativeServices.call(method, request)
                if (method in setOf("sync_now", "sync_resolve")) syncReport = result as? JSONObject
                if (!independent) publish(repository.load())
                withContext(Dispatchers.Main) {
                    if (active()) {
                        if (NativeServicePolicy.budgetRead(method) && (state.value.budget !== shown.budget || state.value.budgetIdentity != shown.budgetIdentity || state.value.budgetRevision != shown.budgetRevision)) {
                            onError(Words[if (NativeServicePolicy.review(method)) R.string.model_changed_while_downloading else R.string.model_changed_refresh])
                        } else callback(result)
                    }
                }
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (error: Exception) {
                withContext(Dispatchers.Main) { if (active()) onError(error.message ?: Words[R.string.model_service_failed]) }
                if (!independent) throw error
            } finally {
                // A JNI commit can finish after its screen's coroutine was cancelled. Always
                // refresh the published revision, while suppressing that screen's callback.
                if (independent && invoked && NativeServicePolicy.budgetResult(method)) {
                    val report = syncReport
                    if (report != null) refreshAfterSync(report) else refresh()
                }
            }
        }
        if (independent) return viewModelScope.launch(Dispatchers.IO) { perform(active = { isActive }) }
        val completion = Job(viewModelScope.coroutineContext[Job])
        var attempted = false
        enqueue {
            val retry = attempted
            try {
                if (retry || completion.isActive) {
                    attempted = true
                    // The global Retry action can replay a failed command after its original
                    // subscriber finished. Keep its guard, but never revive its UI callbacks.
                    perform(active = { completion.isActive }, canInvoke = { retry || completion.isActive })
                }
            }
            finally { completion.complete() }
        }
        return completion
    }

    fun runCommand(method: String, params: JSONObject = JSONObject(), onSuccess: () -> Unit = {}) {
        val shown = state.value
        // Command parameters are read-only after submission; guards are built from the captured
        // snapshot on the worker so large bulk selections cannot stall Compose input handling.
        enqueue {
            val request = JSONObject(params.toString())
            val guard = BudgetEditGuard.command(shown.budget, shown.budgetIdentity, method, request)
            publish(repository.command(method, request, guard)); withContext(Dispatchers.Main) { onSuccess() }
        }
    }

    /** `anyRevision`: the reply still counts after an edit to the same budget (it doesn't depend on its transactions). */
    fun query(method: String, params: JSONObject = JSONObject(), onError: (String) -> Unit = { notifyError(it) }, anyRevision: Boolean = false,
              callback: (JSONObject) -> Unit): Job {
        val shown = state.value
        // Capture published data before scheduling; query work never occupies the write queue.
        // Parameters and published budgets are read-only after passing them to this method.
        return viewModelScope.launch(Dispatchers.Default) {
            suspend fun ask(at: BudgetUiState): Any? = queryWorkers.withPermit {
                ensureActive()
                repository.querySnapshot(method, params, at.budget, if (anyRevision) null else at.budgetRevision)
            }
            try {
                var used = shown
                val value = try { ask(shown) } catch (moved: StaleBudget) {
                    // Sync committed and hasn't reported yet. Catch up now, as an edit does, with no permit held.
                    refresh()
                    queueDrained()
                    used = state.value
                    if (used.budgetIdentity != shown.budgetIdentity) throw moved
                    ask(used)
                }
                ensureActive()
                val objectValue = value as? JSONObject ?: JSONObject().put("items", value ?: JSONObject.NULL)
                withContext(Dispatchers.Main) {
                    if ((anyRevision || state.value.budget === used.budget) && state.value.budgetIdentity == shown.budgetIdentity) callback(objectValue)
                    else onError(Words[R.string.model_changed_preview])
                }
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (error: Exception) {
                val message = error.message ?: Words[R.string.model_engine_failed]
                withContext(Dispatchers.Main) { onError(message) }
            }
        }
    }

    private suspend fun queueDrained() = suspendCancellableCoroutine<Unit> { done ->
        afterPending { if (done.isActive) done.resume(Unit) }
    }

    fun upsert(collection: String, record: JSONObject) {
        val frozen = record.toString()
        val guard = recordGuard(collection, record.optString(if (collection == "loans") "accountId" else "id"))
        enqueue { publish(repository.edit(guard) { it.upsertRecord(collection, JSONObject(frozen)) }) }
    }

    /**
     * A new transaction that repeats, as the desktop saves one: its schedule from the
     * transaction's date and, when that date has come, the transaction itself entered with the
     * schedule moved on past it, as one change.
     */
    fun saveRepeating(schedule: JSONObject, entered: JSONObject?, editorGuard: BudgetEditGuard? = null, onSuccess: () -> Unit = {}) {
        val frozen = schedule.toString()
        val id = schedule.optString("id")
        val guard = editorGuard ?: recordGuard("schedules", id)
        val today = LocalDate.now().toString()
        val commands = entered?.let { t ->
            val date = t.optString("date")
            listOf("saveTransaction" to JSONObject().put("transaction", JSONObject(t.toString())).put("through", today),
                "applySchedules" to JSONObject().put("through", date).put("selected", id).put("skip", true))
        }.orEmpty()
        enqueue { publish(repository.editThenCommands(guard, { it.upsertRecord("schedules", JSONObject(frozen)) }, commands)); withContext(Dispatchers.Main) { onSuccess() } }
    }

    fun upsertMany(collection: String, records: List<JSONObject>) {
        val frozen = records.map(JSONObject::toString)
        val shown = state.value
        val guard = BudgetEditGuard.recordSet(shown.budget, shown.budgetIdentity, collection, records.map { it.optString(if (collection == "loans") "accountId" else "id") })
        enqueue { publish(repository.edit(guard) { budget -> frozen.forEach { budget.upsertRecord(collection, JSONObject(it)) } }) }
    }

    fun remove(collection: String, id: String) {
        val guard = recordGuard(collection, id)
        enqueue { publish(repository.edit(guard) { budget ->
            require(collection in setOf("accounts", "categories", "transactions", "schedules", "loans", "payeeRules", "payees")) { "Unsupported record collection." }
            val identity = if (collection == "loans") "accountId" else "id"
            budget.put(collection, JSONArray(budget.records(collection).filter { it.optString(identity) != id }))
        }) }
    }

    fun saveAccount(id: String? = null, name: String, kind: String, openingBalance: Long = 0, accountType: String? = null) {
        val recordId = id ?: UUID.randomUUID().toString()
        val guard = recordGuard("accounts", recordId)
        enqueue { publish(repository.edit(guard) { budget ->
            val existing = budget.records("accounts").firstOrNull { it.optString("id") == recordId }
            val record = JSONObject().put("id", recordId).put("name", name.trim()).put("kind", kind)
                .put("openingBalance", openingBalance).put("archived", existing?.optBoolean("archived") ?: false)
            if (accountType != null) record.put("accountType", accountType)
            budget.upsertRecord("accounts", record)
            paymentCategory(budget, record)
        }) }
    }

    /** New accounts as one change, which one Undo takes back: those set up from bank texts. */
    fun saveAccounts(records: List<JSONObject>) {
        val frozen = records.map(JSONObject::toString)
        val shown = state.value
        val guard = BudgetEditGuard.recordSet(shown.budget, shown.budgetIdentity, "accounts", records.map { it.optString("id") })
        enqueue { publish(repository.edit(guard) { budget ->
            frozen.map(::JSONObject).forEach { record -> budget.upsertRecord("accounts", record.put("archived", false)); paymentCategory(budget, record) }
        }) }
    }

    /** A credit card's payment category, made with the card unless it has one. */
    private fun paymentCategory(budget: JSONObject, account: JSONObject) {
        val id = account.optString("id")
        if (account.optString("kind") != "credit" || budget.records("categories").any { it.optString("paymentAccountId") == id }) return
        budget.upsertRecord("categories", JSONObject().put("id", UUID.randomUUID().toString()).put("name", account.optString("name").trim())
            .put("group", "Credit Card Payments").put("target", 0).put("paymentAccountId", id))
    }

    fun saveCategory(id: String? = null, name: String, group: String) = upsert("categories", JSONObject()
        .put("id", id ?: UUID.randomUUID().toString()).put("name", name.trim()).put("group", group.trim())
        .also { if (id == null) it.put("target", 0) })

    fun assign(category: String, amount: Long) = runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "assignAmount").put("month", state.value.month).put("categoryId", category).put("amount", amount)))

    fun saveTarget(category: String, target: JSONObject?) {
        val guard = recordGuard("categories", category)
        val frozen = target?.toString()
        enqueue { publish(repository.edit(guard) { budget ->
            val record = budget.records("categories").firstOrNull { it.optString("id") == category } ?: error(Words[R.string.model_category_not_found])
            // A copy, and not `upsertRecord`: that merges, and so can't remove `goal`.
            val next = JSONObject(record.toString())
            if (frozen == null) { next.remove("goal"); next.put("target", 0) }
            else { val goal = JSONObject(frozen); next.put("goal", goal); goal.opt("goal_amount")?.let { next.put("target", it) } }
            budget.put("categories", JSONArray(budget.records("categories").map { if (it === record) next else it }))
        }) }
    }

    fun saveTransaction(record: JSONObject, editorGuard: BudgetEditGuard? = null, onSuccess: () -> Unit = {}) {
        val patch = JSONObject(record.toString())
        if (patch.optString("id").isBlank()) patch.put("id", UUID.randomUUID().toString())
        if (!patch.has("memo")) patch.put("memo", "")
        if (!patch.has("status")) patch.put("status", "uncleared")
        if (!patch.has("transferAccountId")) patch.put("transferAccountId", JSONObject.NULL)
        if (!patch.has("transferStatus")) patch.put("transferStatus", JSONObject.NULL)
        if (!patch.has("splits")) patch.put("splits", JSONArray())
        val guard = editorGuard ?: recordGuard("transactions", patch.getString("id"))
        enqueue {
            publish(repository.command("saveTransaction", JSONObject().put("transaction", patch).put("through", LocalDate.now().toString()), guard))
            withContext(Dispatchers.Main) { onSuccess() }
        }
    }

    fun deleteTransaction(id: String) = remove("transactions", id)

    override fun onCleared() { operations.close(); super.onCleared() }
}
