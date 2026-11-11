package app.urbudget.nativeapp.ui

import android.content.Context
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.urbudget.nativeapp.*
import kotlinx.coroutines.*
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicReference

/**
 * Writes the review's edits in the storage lane, outside any page's scope: a tick made just before
 * the page closes is still saved.
 */
private val smsDraftWrites = CoroutineScope(SupervisorJob() + smsStorageLane)

/** What's stored for a budget's bank messages, read in one go and shown as one. */
internal data class SmsStored(
    val sources: List<SmsSource> = emptyList(),
    /** The waiting texts, oldest first, the order they're read in; [count] is all of them. */
    val messages: List<BankSms> = emptyList(),
    val count: Int = 0,
    val mappings: JSONArray = JSONArray(),
    val consent: String? = null,
    val auto: Boolean = false,
    val coverage: List<SmsCoverage> = emptyList(),
    val last: JSONObject? = null,
    /** The newest text handled, and how long this budget's last full request took a text. */
    val handled: Long? = null,
    val pace: Long? = null,
    /** The formats learned for this budget, how their readings fared in review, and each one for the list. */
    val templates: SmsTemplateSummary = SmsTemplateSummary(0, 0, 0),
    val formats: List<SmsTemplateRow> = emptyList(),
) {
    val since: Long? get() = sources.minOfOrNull { it.since }
}

/** Texts to read, into [base] when they join the review that's open. */
internal data class SmsPrepare(val messages: List<BankSms>, val note: String, val connection: JSONObject, val base: SmsBatch?, val open: Boolean = false)

/**
 * What the page is doing besides reading, which [SmsReadings] owns: loading what's stored, saving a
 * change to it, or settling a review that needs nothing from the user. Nothing is read by itself
 * while it's busy.
 */
internal enum class SmsStage { Idle, Loading, Saving, Settling }

/**
 * Bank messages for one budget: what's stored, the open review and what the page is doing, changed
 * only here. Every change to storage runs in one lane and ends by reading everything back and
 * showing it at once, so the page never shows, or reads by itself from, texts a change has just
 * settled or taken out. The page reads what's waiting by itself once a visit, and only from a
 * fresh snapshot.
 */
internal class SmsPage(private val context: Context, val identity: String, private val vm: BudgetViewModel, private val analyzer: SmsAnalyzer,
                       private val scope: CoroutineScope) {
    private val inbox = SmsInbox.get(context)
    private val lane = Mutex()

    var allowed by mutableStateOf(SmsReader.canRead(context)); private set
    var receiving by mutableStateOf(SmsReader.canReceive(context)); private set
    var stored by mutableStateOf(SmsStored()); private set
    /** Whether what's stored has been read once since the page opened. */
    var loaded by mutableStateOf(false); private set
    var stage by mutableStateOf(SmsStage.Loading); private set
    /** The open review: what's been read and not settled, with the user's choices. */
    var batch by mutableStateOf<SmsBatch?>(null); private set
    /** The review by what it needs; null while it's checked against the budget. */
    var sorted by mutableStateOf<SmsSorted?>(null); private set
    var connection by mutableStateOf<JSONObject?>(null); private set
    var error by mutableStateOf<String?>(null)
    /** A reading waiting on permission to send, asked in the sending notice. */
    var asking by mutableStateOf<SmsPrepare?>(null)
    /** Whether the review page is open; the page checks the review itself while it is. */
    var reviewing by mutableStateOf(false); private set

    val ready: Boolean get() = connection?.optBoolean("ready") == true
    val consented: Boolean get() = connection?.let { smsConsented(stored.consent, it) } == true
    val busy: Boolean get() = stage != SmsStage.Idle
    private val inReview by derivedStateOf { batch?.messages?.mapTo(HashSet()) { it.id }.orEmpty() }
    /** What's waiting is what the review doesn't hold yet. */
    val waiting by derivedStateOf { stored.messages.filter { it.id !in inReview } }
    val waitingCount: Int get() = (stored.count - inReview.size).coerceAtLeast(0)

    // Opening the page reads what's waiting once a visit; a reading the user stopped stays stopped.
    private var started = false
    private var budget: JSONObject? = null
    private var sorting: Job? = null
    @Volatile private var reloadQueued = false
    /** The review as last edited, waiting to be written: a run of ticks is one write, of the latest. */
    private val draft = AtomicReference<SmsBatch?>(null)
    /** Changes under way that replace the review from storage: an edit to the one still shown is dropped. */
    private var restoring = 0
    /**
     * A reading the user asked for with a tap opens its review when it's done: asked for when it's
     * sent ([openWhenRead]), and due once a reading has ended ([opening]), so nothing else that
     * sorts the review opens it.
     */
    private var openWhenRead = false
    private var opening = false

    /** Reads what's stored, then follows readings: one that ends, here or in the background, leaves its review saved. */
    suspend fun start() {
        commit(SmsStage.Loading)
        var seen = SmsReadings.finished.value
        SmsReadings.finished.collect { if (it != seen) { seen = it; opening = openWhenRead; openWhenRead = false; commit(SmsStage.Idle, restore = true) } }
    }

    /** Permissions as Android has them now (after a prompt, or back from settings), then what's stored. */
    fun permissions() {
        val was = allowed
        allowed = SmsReader.canRead(context); receiving = SmsReader.canReceive(context)
        // Allowed since it was last said to be off: that's no longer so.
        if (allowed && !was) error = null
        reload()
    }

    /** After something failed: what's stored again, and what's waiting read as on a first visit. */
    fun retry() {
        error = null; started = false
        reload()
    }

    /** Stops the reading under way, keeping what it has read; its review waits for a tap. */
    fun stop() { openWhenRead = false; opening = false; SmsReadings.stop() }

    /** What's stored, again: the phone's texts may have changed. Requests while one waits are one. */
    fun reload() {
        if (reloadQueued) return
        reloadQueued = true
        commit(SmsStage.Idle) { reloadQueued = false }
    }

    /** The budget as it is now: the review is checked against it. */
    fun budget(value: JSONObject) {
        if (value === budget) return
        budget = value
        resort()
    }

    fun open() { reviewing = true; sorting?.cancel() }
    fun close() { reviewing = false; resort() }

    /**
     * Runs [change] on storage, then reads everything back and shows it at once. [restore] takes the
     * review from storage too (a change wrote it); otherwise the open one stays as the user left it.
     */
    private fun commit(stage: SmsStage, restore: Boolean = false, change: () -> Unit = {}): Job {
        if (restore) restoring++
        return scope.launch {
            try {
                lane.withLock {
                    if (stage != SmsStage.Idle) this@SmsPage.stage = stage
                    try {
                        val keep = if (restore) null else batch
                        // The review's last edit is written first: the change may read the review from storage.
                        val (next, review) = withContext(smsStorageLane) { writeDraft(); change(); read(keep) }
                        stored = next
                        if (restore || batch == null) { batch = review; resort() }
                        connection = try { analyzer.status() } catch (cancel: CancellationException) { throw cancel } catch (_: Exception) { null }
                    } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
                    finally { this@SmsPage.stage = SmsStage.Idle; loaded = true }
                }
            } finally { if (restore) restoring-- }
            readWaiting()
        }
    }

    /** In the storage lane: the review as last edited, if an edit still waits to be written. */
    private fun writeDraft() { draft.getAndSet(null)?.let { inbox.saveState(identity, "draft", it.saved()) } }

    /** Everything stored for the budget, and the review: [keep], or the saved one when there's none. */
    private fun read(keep: SmsBatch?): Pair<SmsStored, SmsBatch?> {
        val can = SmsReader.canRead(context)
        val coverage = if (can) SmsReader.refresh(context, identity) else emptyList()
        val draft = inbox.state(identity, "draft")
        // A draft whose messages are gone, or from before this review, can't be shown: its messages wait again.
        val review = if (!can) null else keep ?: SmsBatch.restore(draft) { ids -> inbox.waiting(identity, ids) }
            .also { if (it == null && draft != null) inbox.saveState(identity, "draft", null) }
        return SmsStored(inbox.sources(identity), if (can) inbox.pending(identity) else emptyList(), inbox.pendingCount(identity),
            JSONArray(inbox.state(identity, "mappings") ?: "[]"), inbox.state(identity, "consent"), inbox.state(identity, "auto") == "on", coverage,
            inbox.state(identity, "last")?.let { runCatching { JSONObject(it) }.getOrNull() }, inbox.handledThrough(identity),
            inbox.state(identity, "pace")?.toLongOrNull(), inbox.templateSummary(identity), inbox.templateList(identity)) to review
    }

    /** Reads what's waiting by itself, once a visit, from a fresh snapshot and only when nothing else is under way. */
    private fun readWaiting() {
        val current = connection ?: return
        if (started || !allowed || busy || lane.isLocked || SmsReadings.state.value != null || !current.optBoolean("ready") ||
            !smsConsented(stored.consent, current) || waiting.isEmpty()) return
        send(SmsPrepare(waiting, "", current, batch), open = false)
    }

    /** Starts the reading; with [open], its review opens when it's done and something needs the user. */
    private fun send(request: SmsPrepare, open: Boolean) {
        // A visit reads by itself once: what this reading leaves waiting is the user's to read.
        started = true
        openWhenRead = open
        error = null; reviewing = false; sorted = null; sorting?.cancel()
        SmsReadings.start(context, analyzer, identity, request.connection, request.messages, request.note, request.base,
            still = { SmsReader.canRead(context) && vm.state.value.budgetIdentity == identity }, failed = { error = it; openWhenRead = false; opening = false })
    }

    /**
     * Reads [chosen], into [base] when given; the first time for this assistant, the sending notice
     * asks. Asked for with a tap ([open]), its review opens when it's done.
     */
    fun prepare(chosen: List<BankSms>, note: String = "", base: SmsBatch? = null, open: Boolean = true) {
        scope.launch {
            try {
                // Asked fresh: the assistant's connection may have changed since the page opened.
                val current = analyzer.status().also { connection = it }
                check(current.optBoolean("ready")) { context.getString(R.string.sms_connect_assistant) }
                error = null
                val request = SmsPrepare(chosen, note, current, base, open)
                if (smsConsented(stored.consent, current)) send(request, open) else asking = request
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    }

    /** Permission to send, given in the sending notice: kept for this assistant, and the reading starts. */
    fun allow(request: SmsPrepare) {
        asking = null
        val saved = smsConsent(request.connection)
        stored = stored.copy(consent = saved)
        save("consent", saved)
        send(request, request.open)
    }

    fun revoke() {
        stored = stored.copy(consent = null, auto = false)
        save("consent", null); save("auto", null)
    }

    fun auto(on: Boolean) {
        stored = stored.copy(auto = on)
        save("auto", if (on) "on" else null)
    }

    /** Saves a small value in the storage lane, in order with every other change. */
    private fun save(name: String, value: String?) {
        scope.launch { try { withContext(smsStorageLane) { inbox.saveState(identity, name, value) } } catch (failure: Exception) { error = failure.message } }
    }

    /**
     * Changes the open review as it is now; a row drawn before the last change can't undo it. It's
     * shown at once and written in the storage lane, where a run of ticks is one write of the latest:
     * a review can hold a phone's whole history, too much to write out on every tick as it's made.
     */
    fun edit(change: (SmsBatch) -> SmsBatch) {
        // The review is being replaced from storage (it was just settled, or a reading added to it):
        // an edit to the one still shown would be lost, or write it back over what replaced it.
        if (restoring > 0) return
        val next = change(batch ?: return)
        batch = next
        draft.set(next)
        smsDraftWrites.launch { try { writeDraft() } catch (failure: Exception) { error = failure.message } }
    }

    fun cards(value: JSONArray) {
        stored = stored.copy(mappings = value)
        save("mappings", value.toString())
        resort()
    }

    /**
     * Ends the review, the texts it settled leaving Bank messages; [summary] is what was handled
     * without a question, and [rest] what still waits on a choice, kept as it was read.
     */
    fun finish(ids: List<String>, summary: JSONObject = JSONObject(), rest: SmsBatch? = null, outcomes: Map<String, JSONObject> = emptyMap()) {
        reviewing = false; sorting?.cancel(); sorted = null
        commit(SmsStage.Saving, restore = true) {
            inbox.finish(identity, ids, outcomes = outcomes)
            // The bank's latest figure for each account, from what was added or matched.
            val balances = inbox.state(identity, "balances")
            smsKeepBalances(balances, outcomes.values)?.takeIf { it != balances }?.let { inbox.saveState(identity, "balances", it) }
            rest?.let { inbox.saveState(identity, "draft", it.saved()) }
            inbox.saveState(identity, "last", summary.put("at", System.currentTimeMillis()).toString())
        }
    }

    /** Dismisses waiting texts; an open review stays as it is. */
    fun dismiss(ids: List<String>): Job {
        error = null
        return commit(SmsStage.Saving) { inbox.finish(identity, ids, endReview = false, outcomes = smsPlainOutcomes(ids, SmsOutcome.Dismissed)) }
    }

    /** Clears the open review's texts without adding anything: left waiting, they'd only be read again. */
    fun discard() { batch?.let { b -> b.messages.map { it.id }.let { ids -> finish(ids, outcomes = smsPlainOutcomes(ids, SmsOutcome.Discarded)) } } }

    /**
     * A reading the assistant's card asked for: what's waiting (texts the card brought back
     * included), with its note. With a review open and a note, its texts are read again too.
     */
    fun assistantRead(note: String) {
        started = true
        scope.launch {
            commit(SmsStage.Idle).join()
            if (SmsReadings.state.value != null) return@launch
            val open = batch
            // The card was applied in the chat: the page shows what the reading found, and the review waits for a tap.
            when {
                note.isNotBlank() && open != null -> prepare(open.messages + waiting, note, open = false)
                waiting.isNotEmpty() -> prepare(waiting, note, base = open, open = false)
            }
        }
    }

    /**
     * Collects from [from] for every sender. A later start takes earlier texts out of the review and
     * what's waiting; an earlier one brings older texts in, which this visit reads by itself.
     */
    fun collectFrom(from: Long) {
        started = false; sorted = null; error = null
        commit(SmsStage.Saving, restore = true) {
            val kept = SmsBatch.restore(inbox.state(identity, "draft")) { ids -> inbox.waiting(identity, ids) }?.from(from)
            inbox.saveState(identity, "draft", kept?.saved())
            inbox.collectFrom(identity, from, kept?.messages?.map { it.id }?.toSet().orEmpty())
        }
    }

    /** The senders to collect from; the open review keeps what's read from the senders that stay. */
    fun senders(chosen: Set<String>, since: Long): Job {
        // Whatever failed before isn't this save's: the picker closes when this one works.
        sorted = null; error = null
        return commit(SmsStage.Saving, restore = true) {
            val kept = SmsBatch.restore(inbox.state(identity, "draft")) { ids -> inbox.waiting(identity, ids) }?.fromSenders(chosen.map(::smsSenderKey).toSet())
            inbox.configure(identity, chosen, since)
            kept?.let { inbox.saveState(identity, "draft", it.saved()) }
        }
    }

    /** Forgets the learned formats and payees: the assistant reads every format in full, and learns them again. */
    fun forgetTemplates(): Job { error = null; return commit(SmsStage.Saving) { inbox.forgetTemplates(identity) } }

    /** Forgets one format: its texts are read in full again, and it's learned again. */
    fun forgetTemplate(id: String): Job { error = null; return commit(SmsStage.Saving) { inbox.forgetTemplate(identity, id) } }

    /**
     * The user changed [fields] of a row before adding it. A changed date or amount counts against
     * the template that read it, if one did; a changed payee or category is the user's name for the
     * bank's ([SmsItem.raw]), learned for its next texts whoever reads them.
     */
    fun edited(item: SmsItem, fields: Set<String>, payee: String, category: String?) {
        if (item.template == null && item.raw == null) return
        scope.launch {
            try {
                withContext(smsStorageLane) {
                    item.template?.let { template -> (fields - setOf("payee", "category")).takeIf { it.isNotEmpty() }?.let { inbox.edited(identity, template, it) } }
                    if (item.raw != null && ("payee" in fields || "category" in fields)) inbox.learnPayee(identity, smsSenderKey(item.sender), item.raw, payee, category)
                }
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    }

    /** Stop and clear: the budget's texts, review, cards, senders and permissions. */
    fun clear() { sorted = null; error = null; commit(SmsStage.Saving, restore = true) { inbox.clear(identity) } }

    /**
     * Sorts the open review by what it needs. When nothing does, it's settled without a question:
     * texts already in the budget are matched, and the rest are cleared.
     */
    private fun resort() {
        sorting?.cancel()
        sorted = null
        val current = batch ?: return
        val budget = budget ?: return
        if (reviewing || SmsReadings.state.value != null || stage == SmsStage.Settling) return
        val mappings = stored.mappings
        sorting = scope.launch {
            try {
                val items = withContext(Dispatchers.Default) { current.items(SmsAccounts.of(budget, mappings, current.starts(budget))) }
                val resolved = current.resolved(items)
                val known = smsKnown(vm.smsPreview(budget, current, resolved), items, budget)
                val result = smsSort(current, items, known)
                if (result.needs > 0 || batch !== current) {
                    if (batch === current) {
                        sorted = result
                        // The reading the user asked for is done and something needs them: its review opens.
                        if (opening) { opening = false; reviewing = true }
                    }
                    return@launch
                }
                opening = false
                settle(current, items, resolved, known, result)
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    }

    private fun settle(current: SmsBatch, items: List<SmsItem>, resolved: JSONObject, known: Map<Int, String>, result: SmsSorted) {
        stage = SmsStage.Settling
        val matching = result.existing.filter { current.ticked[it.index] != false }.map { it.index to known.getValue(it.index) }
        val summary = JSONObject().put("known", matching.size).put("skipped", JSONObject(result.skipped.groupingBy { it.skip.ifBlank { "unclear" } }.eachCount()))
        val handled = items.flatMap { item -> item.sources.map { it.id } }
        val outcomes = smsOutcomes(items, matching, current.dismissed)
        if (matching.isEmpty()) { finish(handled, summary, outcomes = outcomes); return }
        val failures = vm.operationFailureEpoch
        vm.runCommand("smsBatchApply", current.params(resolved).put("decisions", smsDecisions(matching))) { finish(handled, summary, outcomes = outcomes) }
        // Said by the count of failures, not by the message: the same one twice changes nothing to see.
        vm.afterPending { if (vm.operationFailureEpoch != failures) failed(vm.state.value.error ?: context.getString(R.string.model_action_not_saved)) }
    }

    /** A budget command failed: settling stops, and says why. */
    fun failed(message: String) {
        if (stage != SmsStage.Settling) return
        error = message; stage = SmsStage.Idle
    }
}
