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
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

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
)

class BudgetViewModel(application: Application) : AndroidViewModel(application) {
    private val repository = BudgetRepository(SqliteBudgetStorage(application))
    private val mutableState = MutableStateFlow(BudgetUiState())
    val state: StateFlow<BudgetUiState> = mutableState.asStateFlow()
    private val operations = Channel<Pair<Boolean, suspend () -> Unit>>(Channel.UNLIMITED)
    private val queryWorkers = Semaphore(2)
    private val refreshQueued = AtomicBoolean(false)
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
                    retryOperation = operation
                    mutableState.update { it.copy(error = error.message ?: Words[R.string.model_action_not_saved], recoveryAvailable = repository.hasRecovery()) }
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

    private suspend fun publish(snapshot: BudgetSnapshot) = withContext(Dispatchers.Main.immediate) {
        // Configure before emission so the first frame and amount editors use this
        // snapshot's preferences. Unchanged loads/month changes reuse the same object.
        if (snapshot.budget !== mutableState.value.budget) snapshot.budget?.let(Money::configure)
        retryOperation = null
        mutableState.update { it.copy(budget = snapshot.budget, summary = snapshot.summary, reports = snapshot.reports,
            month = snapshot.month, undoAvailable = snapshot.undoAvailable, recoveryAvailable = snapshot.recoveryAvailable, redoAvailable = snapshot.redoAvailable, budgetIdentity = snapshot.identity, budgetRevision = snapshot.revision, error = null, loading = false) }
    }

    fun notifyError(message: String) { mutableState.update { it.copy(error = message) } }
    fun dismissError() { mutableState.update { it.copy(error = null) } }
    fun retry() { retryOperation?.let { enqueue(operation = it) } ?: refresh() }
    fun createBudget(name: String, currency: String, sample: Boolean = false) = enqueue { publish(repository.create(name, currency, sample)) }
    fun importBudget(json: String) {
        val shown = state.value
        val guard = repository.replacementGuard(shown.budget, shown.budgetIdentity)
        enqueue { publish(repository.importBudget(json, guard)) }
    }
    fun exportBudget(): String? = repository.exportBudget()
    fun selectMonth(month: String) = enqueue { publish(repository.selectMonth(month)) }
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
        val guard = if (method in setOf("simplefin_import", "sync_ynab", "accept_ynab_import", "resolve_ynab", "assistant_apply", "native_apply_patch") && shown.budget != null) BudgetEditGuard.whole(shown.budget, shown.budgetIdentity) else null
        val independent = NativeServicePolicy.independent(method)
        suspend fun perform(active: () -> Boolean, canInvoke: () -> Boolean = active) {
            var invoked = false
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
                if (independent && invoked && NativeServicePolicy.budgetResult(method)) refresh()
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

    fun applyServicePatch(patch: JSONObject) = service("native_apply_patch", JSONObject().put("patch", patch))
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
            queryWorkers.withPermit {
                try {
                    ensureActive()
                    val value = repository.querySnapshot(method, params, shown.budget)
                    ensureActive()
                    val objectValue = value as? JSONObject ?: JSONObject().put("items", value ?: JSONObject.NULL)
                    withContext(Dispatchers.Main) {
                        if ((anyRevision || state.value.budget === shown.budget) && state.value.budgetIdentity == shown.budgetIdentity) callback(objectValue)
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
    fun saveRepeating(schedule: JSONObject, entered: JSONObject?) {
        val frozen = schedule.toString()
        val id = schedule.optString("id")
        val guard = recordGuard("schedules", id)
        val today = LocalDate.now().toString()
        val commands = entered?.let { t ->
            val date = t.optString("date")
            listOf("saveTransaction" to JSONObject().put("transaction", JSONObject(t.toString())).put("through", today),
                "applySchedules" to JSONObject().put("through", date).put("selected", id).put("skip", true))
        }.orEmpty()
        enqueue { publish(repository.editThenCommands(guard, { it.upsertRecord("schedules", JSONObject(frozen)) }, commands)) }
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
            if (kind == "credit" && budget.records("categories").none { it.optString("paymentAccountId") == recordId }) {
                budget.upsertRecord("categories", JSONObject().put("id", UUID.randomUUID().toString()).put("name", name.trim())
                    .put("group", "Credit Card Payments").put("target", 0).put("paymentAccountId", recordId))
            }
        }) }
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
            if (frozen == null) { record.remove("goal"); record.put("target", 0) }
            else { val goal = JSONObject(frozen); record.put("goal", goal); goal.opt("goal_amount")?.let { record.put("target", it) } }
        }) }
    }

    fun saveTransaction(record: JSONObject) {
        val patch = JSONObject(record.toString())
        if (patch.optString("id").isBlank()) patch.put("id", UUID.randomUUID().toString())
        if (!patch.has("memo")) patch.put("memo", "")
        if (!patch.has("status")) patch.put("status", "uncleared")
        if (!patch.has("transferAccountId")) patch.put("transferAccountId", JSONObject.NULL)
        if (!patch.has("transferStatus")) patch.put("transferStatus", JSONObject.NULL)
        if (!patch.has("splits")) patch.put("splits", JSONArray())
        runCommand("saveTransaction", JSONObject().put("transaction", patch).put("through", LocalDate.now().toString()))
    }

    fun deleteTransaction(id: String) = remove("transactions", id)

    override fun onCleared() { operations.close(); super.onCleared() }
}
