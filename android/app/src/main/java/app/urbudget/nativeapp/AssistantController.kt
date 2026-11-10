package app.urbudget.nativeapp

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Base64
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.urbudget.nativeapp.ui.warmAssistantMarkdown
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.time.LocalDate

data class AssistantState(
    val status: JSONObject? = null,
    /** The status couldn't be read and none is known: the panel says so, rather than offer set-up to someone who is set up. */
    val statusFailed: Boolean = false,
    val models: List<JSONObject> = emptyList(),
    val threads: List<JSONObject> = emptyList(),
    val thread: JSONObject? = null,
    val entries: List<JSONObject> = emptyList(),
    val running: Boolean = false,
    val attachments: List<JSONObject> = emptyList(),
    val busy: Set<String> = emptySet(),
    /** The live reply: what's being written, and what was just written until the conversation shows it. */
    val streaming: String = "",
    /** The latest line of the model's thinking, while it thinks. */
    val thinking: String = "",
    /** Something the user just asked for failed: said in a dialog. */
    val error: String? = null,
    /** Something done unasked failed (the conversation read again, a refresh on coming back): said in a line of the panel, once. */
    val quiet: String? = null,
    val notice: String? = null,
    val undoProposal: String? = null,
    /** What the reply is doing now ("Searching bank texts"), while a lookup runs. */
    val activity: String? = null,
) {
    val threadId get() = thread?.optString("id").orEmpty()
}

/** Where the user is when they ask: the page, and the account or category its register shows. */
data class AssistantPlace(val page: String = "assistant", val accountId: String? = null, val categoryId: String? = null)

/** Where a conversation's list was scrolled to when the panel closed. */
data class AssistantScroll(val index: Int, val offset: Int)

/**
 * The next message as it's being written: its words, the transactions picked to go with it, and
 * what was taken off it. It lives with the controller, not the screen, so closing the panel keeps
 * it; and it's Compose state, so typing shows at once.
 */
class AssistantComposer {
    var text by mutableStateOf("")
    var selected by mutableStateOf<List<String>>(emptyList())
    /** How many were selected where the selection was handed over, when only its first 200 could come. */
    var selectedOf by mutableStateOf<Int?>(null)
    /** Parts taken off the next message: where the user is, or the file being imported. */
    var without by mutableStateOf<Set<AssistantPart>>(emptySet())
    /** A category handed over with "Ask the assistant": the one in view, for the next message. */
    var category by mutableStateOf<String?>(null)
    /** Where the open conversation's list was when the panel closed; none at its end, where a conversation opens. */
    var scroll: AssistantScroll? = null

    /** Picks the transactions to go with the next message. */
    fun select(ids: List<String>) { selected = ids; selectedOf = null }

    /** The message went: the next one starts with every part again. */
    internal fun sent() { text = ""; selected = emptyList(); selectedOf = null; without = emptySet(); category = null }

    internal fun kept(attachments: List<JSONObject>) = AssistantDraft(text, selected, selectedOf, without, category, attachments)
    internal fun take(draft: AssistantDraft) {
        text = draft.text; selected = draft.selected; selectedOf = draft.selectedOf; without = draft.without; category = draft.category; scroll = null
    }
}

/** A conversation's unsent message, set aside while another conversation is open. */
internal data class AssistantDraft(val text: String = "", val selected: List<String> = emptyList(), val selectedOf: Int? = null,
                                   val without: Set<AssistantPart> = emptySet(), val category: String? = null,
                                   val attachments: List<JSONObject> = emptyList()) {
    val blank get() = text.isBlank() && selected.isEmpty() && category == null && attachments.isEmpty()
}

/** Native presentation state; providers, tools, history, validation and proposals live in Rust. */
class AssistantController(
    private val service: suspend (String, JSONObject) -> Any? = { method, params ->
        withContext(Dispatchers.IO) { NativeServices.call(method, params) }
    },
    events: Flow<JSONObject> = NativeServices.events,
) : ViewModel() {
    private val mutable = MutableStateFlow(AssistantState())
    val state = mutable.asStateFlow()
    /** The message being written for the open conversation. */
    val composer = AssistantComposer()
    /** Unsent messages of the conversations that aren't open, by conversation; "" is one not begun yet. */
    private val drafts = HashMap<String, AssistantDraft>()
    private var undoToken: String? = null
    private val stream = AssistantStreamBuffer()
    private var streamPublish: Job? = null
    /** The reply a `message` event announced, until the conversation is read back with it. */
    private var awaited: String? = null
    /** The open conversation was opened while its reply ran: what's left of the round under way would show as a fragment, so it waits for the next. */
    private var joined = false
    private val reloads = Channel<String>(Channel.CONFLATED)
    private val threadLoader = Mutex()
    private var presentation: AssistantThreadPresentation? = null
    private var conversationGeneration = 0L
    /** The conversations with a reply running, whichever is open. */
    private val replying = MutableStateFlow(emptySet<String>())
    /** Whether any reply is running: the app then reads the core's events faster, and keeps reading them behind another app. */
    val replyRunning: Boolean get() = replying.value.isNotEmpty()
    /** How many replies have ended: a conversation read before one ended can't say its reply still runs. */
    private var endings = 0
    /** Failures of unasked work already said, by the work: the same one coming again isn't said again. */
    private val said = HashMap<String, String>()

    init {
        refresh()
        viewModelScope.launch {
            events.collect { envelope ->
                if (envelope.optString("event") != "assistant://event") return@collect
                val event = envelope.optJSONObject("payload") ?: return@collect
                val thread = event.optString("threadId")
                val type = event.optString("type")
                when (type) {
                    "started" -> replying.update { it + thread }
                    "done" -> { endings++; replying.update { it - thread } }
                }
                if (thread != state.value.threadId) return@collect
                // Anything but text marks the edge of a round: from here its text is whole, and its thinking is done.
                val edge = type != "delta" && type != "thinking"
                if (edge) { joined = false; stream.rested() }
                when (type) {
                    "started" -> mutable.update { it.copy(running = true, activity = null) }
                    "activity" -> mutable.update { it.copy(activity = event.optString("text").ifBlank { null }) }
                    // Only a reply under way has a live text. Events are polled, so what was written
                    // just before Stop can arrive after the conversation was read back with that text
                    // kept in it, and would show a second time under it.
                    "delta", "thinking" -> if (state.value.running && !joined) {
                        stream.append(event.optString("text"), thinking = type == "thinking")
                        scheduleStreamPublish()
                    }
                    "message" -> {
                        // The live text stays until the conversation is read back with the reply in it.
                        val text = event.optString("text")
                        if (assistantShows(state.value.entries, text)) stream.clearReply() else { stream.settle(); awaited = text }
                        mutable.update { it.copy(activity = null) }
                        reloadThread()
                    }
                    "step", "proposal", "error", "cut", "done" -> {
                        mutable.update { it.copy(activity = null) }
                        reloadThread()
                        if (type == "done") reloadHistory()
                    }
                }
                if (edge) publishStream()
            }
        }
        viewModelScope.launch {
            while (true) {
                delay(1200)
                if (state.value.running) reloadThread()
                if (state.value.status?.optBoolean("signingIn") == true) refreshSignInStatus()
            }
        }
        viewModelScope.launch {
            for (first in reloads) {
                delay(80) // Tool/message bursts need one history fetch, not one per event.
                var id = first
                while (true) { id = reloads.tryReceive().getOrNull() ?: break }
                if (state.value.threadId != id) continue
                try { loadThread(id); worked("reload") }
                catch (cancel: CancellationException) { throw cancel }
                // Nobody asked for this read, and while a reply runs it comes again every second.
                catch (failure: Exception) { if (state.value.threadId == id) quietly("reload", failure.message) }
            }
        }
    }

    private fun scheduleStreamPublish() {
        if (streamPublish?.isActive == true) return
        streamPublish = viewModelScope.launch { delay(64); publishStream() }
    }
    private fun publishStream() { stream.drain()?.let { (reply, thinking) -> mutable.update { it.copy(streaming = reply, thinking = thinking) } } }
    private fun clearStream() { streamPublish?.cancel(); streamPublish = null; stream.clear(); awaited = null; joined = false }

    private suspend fun call(method: String, params: JSONObject = JSONObject()): Any? =
        service(method, params)

    /**
     * Runs [work] as [label], one at a time. A failure is said in a dialog, or for [quiet] work
     * (what the user didn't just ask for) in a line of the panel, which leaves a dialog that's up alone.
     */
    private fun operation(label: String, clearError: Boolean = true, quiet: Boolean = false, work: suspend () -> Unit) {
        if (label in state.value.busy) return
        mutable.update { it.copy(busy = it.busy + label, error = if (clearError && !quiet) null else it.error) }
        viewModelScope.launch {
            try { work(); if (quiet) worked(label) }
            catch (cancel: CancellationException) { throw cancel }
            catch (failure: Exception) {
                if (quiet) quietly(label, failure.message)
                else mutable.update { it.copy(error = failure.message ?: Words[R.string.assistant_action_failed]) }
            }
            finally { mutable.update { it.copy(busy = it.busy - label) } }
        }
    }

    /** Says a failure of unasked [work] in the panel's line, once: again only after that work has succeeded in between. */
    private fun quietly(work: String, failure: String?) {
        val message = failure ?: Words[R.string.assistant_action_failed]
        if (said.put(work, message) != message) mutable.update { it.copy(quiet = message) }
    }
    private fun worked(work: String) {
        val message = said.remove(work) ?: return
        mutable.update { if (it.quiet == message) it.copy(quiet = null) else it }
    }
    fun dismissQuiet() { mutable.update { it.copy(quiet = null) } }

    private suspend fun loadStatus() {
        val previous = state.value.status
        val status = call("assistant_status") as JSONObject
        val phase = status.optString("signInPhase")
        if (phase != previous?.optString("signInPhase") && phase in setOf("waiting", "returning", "exchanging", "validating", "saving", "complete", "failed", "cancelled")) {
            // Stage names only: never log account details, authorization URLs, codes or tokens.
            android.util.Log.i("UrBudgetSignIn", "Phase: $phase")
        }
        mutable.update { it.copy(status = status, statusFailed = false) }
        if (status.optBoolean("ready") && (previous?.optString("provider") != status.optString("provider") || previous?.optBoolean("ready") != true)) {
            operation("models", clearError = false) { loadModels(false) }
        }
    }
    private suspend fun loadModels(refresh: Boolean) {
        val models = runCatching { call("assistant_models", JSONObject().put("refresh", refresh)).objects() }
        mutable.update { it.copy(models = models.getOrDefault(emptyList())) }
        if (refresh) models.exceptionOrNull()?.let { throw it }
    }
    private suspend fun loadHistory() {
        val threads = call("assistant_threads").objects()
        mutable.update { current -> current.copy(threads = threads) }
    }
    private suspend fun loadThread(id: String) = threadLoader.withLock {
        val generation = conversationGeneration
        if (state.value.threadId != id) return@withLock
        val ended = endings
        val thread = call("assistant_thread", JSONObject().put("id", id)) as JSONObject
        val prior = presentation
        val prepared = withContext(Dispatchers.Default) {
            assistantThreadPresentation(thread, prior).also { now ->
                // The latest replies are laid out before they're shown, so one never takes the live text's place empty.
                if (now !== prior) now.entries.filter { it.optString("kind") == "assistant" }.takeLast(2).forEach { warmAssistantMarkdown(it.optString("text")) }
            }
        }
        // A reply from a conversation left while this request ran must not navigate back.
        if (state.value.threadId == id && generation == conversationGeneration) {
            presentation = prepared
            val running = thread.optBoolean("running")
            replying.update { if (!running) it - id else if (ended == endings) it + id else it }
            if (!running) clearStream()
            else awaited?.let { if (assistantShows(prepared.entries, it)) { stream.shown(); awaited = null } }
            // With the conversation, in one step: the reply and the live text it replaces never show together, nor neither.
            val live = if (running) stream.drain() else null
            mutable.update {
                it.copy(thread = prepared.thread, entries = prepared.entries, running = running,
                    streaming = if (running) live?.first ?: it.streaming else "", thinking = if (running) live?.second ?: it.thinking else "")
            }
        }
    }
    /**
     * Reads the assistant's state again: as the panel opens, when the app comes back, when another
     * budget opens. Nobody asked for it, so a failure is a line in the panel; with no status known
     * at all, the panel says the assistant couldn't be loaded and offers this again.
     */
    fun refresh() = operation("refresh", clearError = false, quiet = true) {
        mutable.update { it.copy(statusFailed = false) }
        try { loadStatus() }
        catch (cancel: CancellationException) { throw cancel }
        catch (failure: Exception) {
            if (state.value.status != null) throw failure
            mutable.update { it.copy(statusFailed = true) }
            return@operation
        }
        loadHistory()
        val id = state.value.threadId
        if (id.isNotEmpty() && state.value.threads.any { it.optString("id") == id }) loadThread(id)
        else if (id.isNotEmpty()) close(aside = false)
    }
    fun reloadThread() { state.value.threadId.takeIf(String::isNotEmpty)?.let(reloads::trySend) }
    private fun reloadHistory() = operation("history", quiet = true) { loadHistory() }
    fun dismissError() { mutable.update { it.copy(error = null) } }
    private var budget: String? = null
    /**
     * The open budget. Another one has its own conversations, so the one open here closes; and an
     * error from before a budget was open (listing conversations) no longer applies. The same one
     * again (the screen made anew, as unfolding the phone does) changes nothing.
     */
    fun budgetOpened(identity: String) {
        if (identity == budget) return
        val switched = budget != null
        budget = identity
        if (switched) {
            // The other budget's unsent messages, files and running replies are not this one's.
            drafts.clear(); composer.sent(); replying.value = emptySet()
            mutable.update { it.copy(attachments = emptyList()) }
            close(aside = false)
        }
        mutable.update { it.copy(threads = if (switched) emptyList() else it.threads, error = null, quiet = null) }
        said.clear()
        refresh()
    }
    fun showError(message: String?) { mutable.update { it.copy(error = message) } }
    fun dismissNotice() { mutable.update { it.copy(notice = null) } }
    fun note(text: String) { mutable.update { it.copy(notice = text) } }
    /** Puts `text` in the composer, unsent: after what's already typed there, never over it. */
    fun prefill(text: String) { composer.text = assistantDraftWith(composer.text, text) }

    /**
     * Something handed over from a page with "Ask the assistant": transactions to go with the next
     * message (the first 200 of them), a category to ask about, words to start from. It's for a new
     * conversation unless the open one has no messages yet, and nothing is sent.
     */
    fun handOver(text: String = "", selected: List<String> = emptyList(), categoryId: String? = null) {
        val open = state.value.thread
        if (open != null && !(open.has("entries") && state.value.entries.isEmpty())) newChat()
        if (selected.isNotEmpty()) assistantSelection(selected).let { (ids, of) -> composer.selected = ids; composer.selectedOf = of }
        if (categoryId != null) { composer.category = categoryId; composer.without -= AssistantPart.Place }
        if (text.isNotEmpty()) prefill(text)
    }

    /** Sets the open conversation's unsent message aside, for when it's opened again. */
    private fun leave() {
        val draft = composer.kept(state.value.attachments)
        if (draft.blank) drafts.remove(state.value.threadId) else drafts[state.value.threadId] = draft
    }
    /** Brings back the unsent message of the conversation [id]. */
    private fun enter(id: String) {
        val draft = drafts.remove(id) ?: AssistantDraft()
        composer.take(draft)
        mutable.update { it.copy(attachments = draft.attachments) }
    }
    /** Forgets the unsent message of a conversation that's gone, with its files. */
    private fun forget(id: String) { drafts.remove(id)?.let { detach(it.attachments) } }

    /** Where the open conversation's list is, as the panel closes: reopening the panel finds it there. None at its end. */
    fun keepScroll(scroll: AssistantScroll?) { composer.scroll = scroll }

    /**
     * Shows no conversation. [aside] sets the open one's unsent message aside and brings back a
     * new conversation's own; otherwise what's typed, picked and attached stays in the composer.
     */
    private fun close(aside: Boolean) {
        if (aside) leave()
        conversationGeneration++; clearStream(); presentation = null
        mutable.update { it.copy(thread = null, entries = emptyList(), running = false, streaming = "", thinking = "", undoProposal = null, activity = null) }
        undoToken = null
        // A conversation opens at its end.
        if (aside) enter("") else composer.scroll = null
    }
    /** The panel with no conversation open. The one that was open keeps its unsent message. */
    fun newChat() = close(aside = true)
    /**
     * A new conversation, as the user asks for one. What's typed, picked and attached comes along,
     * unless a new conversation's own unsent message is waiting. A reply still running in the
     * conversation left carries on there.
     */
    fun startNew() {
        if (state.value.thread != null) close(aside = drafts[""]?.blank == false)
    }
    fun openThread(id: String) = operation("open") { open(id, aside = true) }
    private suspend fun open(id: String, aside: Boolean) {
        if (aside) leave()
        conversationGeneration++; clearStream(); presentation = null
        mutable.update { it.copy(thread = JSONObject().put("id", id), entries = emptyList(), running = false, streaming = "", thinking = "", undoProposal = null, activity = null) }
        undoToken = null
        if (aside) enter(id) else composer.scroll = null
        // Its reply may be part way through a round: only whole rounds are shown from here.
        joined = true
        loadThread(id)
    }
    /** Carries the open conversation on in a new one, which begins with what was said last, and opens it. What's typed comes along. */
    fun continueInNew() = operation("open") {
        val from = state.value.threadId
        val id = call("assistant_continue", JSONObject().put("threadId", from)) as String
        open(id, aside = state.value.threadId != from)
        loadHistory()
    }
    fun renameThread(id: String, title: String, done: () -> Unit = {}) = operation("rename") {
        call("assistant_rename_thread", JSONObject().put("id", id).put("title", title)); done()
        loadHistory()
        if (state.value.threadId == id) loadThread(id)
    }
    /** The user's thumb on a reply, by the reply's `index`: up, down, or off again (null). It stays on this device. */
    fun rate(index: Int, up: Boolean?) = operation("rate") {
        val id = state.value.threadId
        call("assistant_rate", JSONObject().put("threadId", id).put("index", index).put("up", up ?: JSONObject.NULL))
        loadThread(id)
    }
    fun deleteThread(id: String) = operation("delete") {
        call("assistant_delete_thread", JSONObject().put("id", id))
        replying.update { it - id }
        // What's typed for the open one isn't lost with it; another's unsent message goes with it.
        if (state.value.threadId == id) close(aside = false) else forget(id)
        loadHistory()
    }
    fun deleteAll() = operation("delete") {
        call("assistant_delete_all")
        // Every attached file went too, so no unsent message can keep one. What's typed stays.
        drafts.clear(); replying.value = emptySet()
        mutable.update { it.copy(attachments = emptyList()) }
        close(aside = false); loadHistory()
    }
    fun settings(change: JSONObject) = operation("settings") {
        call("assistant_settings", JSONObject().put("change", change)); loadStatus()
    }
    /**
     * The user read the notice of what's sent, and to whom: kept for the service in use, so it
     * isn't due again until another is chosen. [done] then sends what waited on it. It takes the
     * settings' turn, which the notice's memo switch takes too: the notice offers Continue only
     * once a change of that switch is saved, so the message goes with the choice made there.
     */
    fun told(done: () -> Unit) = operation("settings") {
        call("assistant_settings", JSONObject().put("change", JSONObject().put("told", true))); loadStatus(); done()
    }
    fun models() = operation("models") { loadModels(true) }
    /** Browser return and Activity recreation may miss the original sign-in coroutine's result. */
    fun refreshSignInStatus() {
        if ("signInStatus" in state.value.busy) return
        // A status read must not dismiss a sign-in failure before the user can see it.
        mutable.update { it.copy(busy = it.busy + "signInStatus") }
        viewModelScope.launch {
            try { loadStatus(); worked("signInStatus") }
            catch (cancel: CancellationException) { throw cancel }
            // Asked every second while a sign-in is under way: not a dialog each time.
            catch (failure: Exception) { quietly("signInStatus", failure.message) }
            finally { mutable.update { it.copy(busy = it.busy - "signInStatus") } }
        }
    }
    /** [done] runs once signed in: a reply that failed for want of it is asked for again. */
    fun signIn(done: () -> Unit = {}) = operation("signIn") {
        mutable.update { it.copy(status = it.status?.copy()?.put("signingIn", true)) }
        try { call("assistant_sign_in", JSONObject().put("returnPackage", BuildConfig.APPLICATION_ID)) }
        finally { loadStatus() }
        if (state.value.status?.optBoolean("ready") == true) done()
    }
    fun cancelSignIn() = operation("cancel") { call("assistant_cancel_sign_in"); loadStatus() }
    fun signOut() = operation("signOut") { try { call("assistant_sign_out") } finally { loadStatus() } }
    fun setKey(provider: String, key: String, done: () -> Unit) = operation("key") {
        call("assistant_set_key", JSONObject().put("provider", provider).put("key", key)); done(); loadStatus()
    }
    fun forgetKey(provider: String) = operation("key") { call("assistant_forget_key", JSONObject().put("provider", provider)); loadStatus() }
    fun openLink(link: String) = operation("link") { call("assistant_open", JSONObject().put("link", link)) }

    fun attach(context: Context, uri: Uri) = operation("attach") {
        require(state.value.attachments.size < 5) { Words[R.string.assistant_attach_limit] }
        val generation = conversationGeneration
        val params = withContext(Dispatchers.IO) {
            val file = readAttachment(context, uri)
            JSONObject().put("name", file.first).put("data", Base64.encodeToString(file.second, Base64.NO_WRAP))
        }
        val attachment = call("assistant_attach", params) as JSONObject
        // Attached to a conversation that was left meanwhile: it has no message to go with.
        if (generation == conversationGeneration) mutable.update { it.copy(attachments = it.attachments + attachment) } else detach(listOf(attachment))
    }
    fun removeAttachment(id: String) {
        detach(state.value.attachments.filter { it.optString("id") == id })
        mutable.update { it.copy(attachments = it.attachments.filterNot { file -> file.optString("id") == id }) }
    }
    /**
     * Forgets files that were attached and won't be sent: taken off the message, or left behind with
     * its conversation. The core keeps an attached file aside until Send. A message on its way
     * takes its files with it, so nothing is forgotten then; and one the core couldn't forget is
     * cleared with the rest it has aside, later.
     */
    private fun detach(files: List<JSONObject>) {
        val ids = files.map { it.optString("id") }.filter(String::isNotEmpty)
        if (ids.isEmpty() || "send" in state.value.busy) return
        viewModelScope.launch {
            for (id in ids) try { call("assistant_detach", JSONObject().put("id", id)) }
                catch (cancel: CancellationException) { throw cancel } catch (_: Exception) {}
        }
    }
    /**
     * Sends [text] with what [sending] says goes with it (`assistantSending`): where the user is,
     * the transactions picked, the bank file being imported. Once it has gone, the composer is
     * empty and the next message starts with every part again.
     */
    fun send(text: String, sending: AssistantSending = AssistantSending(), bankTexts: Boolean = false, done: () -> Unit = {}) = operation("send") {
        require(sending.selected.size <= AssistantSelectionLimit) { Words[R.string.assistant_select_limit] }
        val current = state.value
        val generation = conversationGeneration
        val files = current.attachments.map { it.optString("id") }
        clearStream()
        mutable.update { it.copy(streaming = "", thinking = "") }
        val result = call("assistant_send", JSONObject().put("threadId", current.threadId.takeIf(String::isNotEmpty) ?: JSONObject.NULL)
            .put("text", text).put("attachments", JSONArray(files))
            .put("screen", assistantScreen(sending, LocalDate.now().toString(), bankTexts))) as String
        replying.update { it + result }
        // Its files went with it, wherever the composer is by now.
        mutable.update { it.copy(attachments = it.attachments.filterNot { file -> file.optString("id") in files }) }
        if (generation != conversationGeneration) {
            // Another conversation was opened meanwhile: this one's message, set aside there, has gone. One carried into a new conversation has too.
            drafts.remove(current.threadId)
            if (composer.text == text) composer.sent()
            return@operation
        }
        // A suggested question goes as it is, and leaves what's being typed alone.
        val typed = composer.text.takeIf { it != text }.orEmpty()
        composer.sent(); composer.text = typed
        mutable.update { it.copy(thread = if (it.threadId == result) it.thread else JSONObject().put("id", result), running = true) }
        done(); loadThread(result); loadHistory()
    }
    /** Asks for the reply again: after an error, after one was interrupted, and for the rest of one cut short. */
    fun retry(sending: AssistantSending = AssistantSending(), bankTexts: Boolean = false) = operation("send") {
        val id = state.value.threadId
        clearStream()
        mutable.update { it.copy(streaming = "", thinking = "") }
        call("assistant_retry", JSONObject().put("threadId", id).put("screen", assistantScreen(sending.copy(selected = emptyList()), LocalDate.now().toString(), bankTexts)))
        replying.update { it + id }
        if (state.value.threadId == id) loadThread(id)
    }
    fun stop() = operation("stop") { call("assistant_stop", JSONObject().put("threadId", state.value.threadId)); loadThread(state.value.threadId) }

    fun apply(proposal: String, changed: () -> Unit) = operation("proposal") {
        val id = state.value.threadId
        val params = proposalParams(id, proposal)
        val outcome = call("assistant_apply_commit", params) as JSONObject
        val patch = outcome.optJSONObject("patch")
        if (patch != null) {
            undoToken = outcome.optJSONObject("snapshot")?.optString("token")
                ?: (call("database_status") as JSONObject).optString("token")
            mutable.update { it.copy(undoProposal = if (outcome.optBoolean("undoAvailable")) proposal else null, notice = Words[R.string.assistant_proposal_applied]) }
            NativeServices.notifySaved(); changed()
        } else mutable.update { it.copy(notice = Words[R.string.assistant_budget_changed_review]) }
        loadThread(id)
    }
    fun undoProposal(proposal: String, changed: () -> Unit) = operation("proposal") {
        val id = state.value.threadId
        val token = (call("database_status") as JSONObject).optString("token")
        require(token == undoToken && proposal == state.value.undoProposal) { Words[R.string.assistant_budget_changed_undo] }
        call("assistant_undo_commit", proposalParams(id, proposal).put("expectedToken", token))
        undoToken = null; mutable.update { it.copy(undoProposal = null) }; NativeServices.notifySaved(); changed(); loadThread(id)
    }
    /** The proposal made again with the user's choices on its card (`ProposalChoices`): rows left out, categories or amounts given. */
    fun revise(proposal: String, revision: JSONObject) = operation("proposal") {
        val id = state.value.threadId
        call("assistant_revise", proposalParams(id, proposal).put("revision", revision)); loadThread(id)
    }
    fun discard(proposal: String) = operation("proposal") {
        call("assistant_mark", proposalParams(state.value.threadId, proposal).put("status", "discarded")); loadThread(state.value.threadId)
    }
    fun imported(thread: String, proposal: String) = operation("proposal") {
        call("assistant_mark", proposalParams(thread, proposal).put("status", "applied"))
        if (state.value.threadId == thread) loadThread(thread)
    }
    fun report(proposal: String, done: (JSONObject) -> Unit) = operation("report") {
        done(call("assistant_report", proposalParams(state.value.threadId, proposal)) as JSONObject)
    }
    fun importFile(proposal: String, done: (JSONObject) -> Unit) = operation("import") {
        done(call("assistant_import", proposalParams(state.value.threadId, proposal)) as JSONObject)
    }
    fun saveImport(proposal: String, done: (String) -> Unit) = operation("export") {
        done(call("assistant_save_import", proposalParams(state.value.threadId, proposal)) as String)
    }
    fun spreadsheet(proposal: String, open: Boolean, done: (String) -> Unit = {}) = operation("export") {
        done(call("assistant_spreadsheet", proposalParams(state.value.threadId, proposal).put("open", open)) as String)
    }
    fun reportReply(text: String, reason: String, done: () -> Unit) = operation("reportReply") {
        val reference = call("assistant_report_reply", JSONObject().put("text", text).put("reason", reason)) as String
        mutable.update { it.copy(notice = Words[R.string.assistant_report_sent, reference]) }; done()
    }

    override fun onCleared() { reloads.close(); super.onCleared() }

    companion object {
        private fun proposalParams(thread: String, proposal: String) = JSONObject().put("threadId", thread).put("proposalId", proposal)
        private fun JSONObject.copy() = JSONObject(toString())
        private fun Any?.objects(): List<JSONObject> = (this as? JSONArray)?.let { list -> (0 until list.length()).mapNotNull(list::optJSONObject) }.orEmpty()

        private fun readAttachment(context: Context, uri: Uri): Pair<String, ByteArray> {
            var name = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                if (it.moveToFirst()) it.getString(0) else null
            } ?: "attachment.txt"
            var bytes = context.contentResolver.openInputStream(uri)?.use { input ->
                val output = ByteArrayOutputStream(); val buffer = ByteArray(8192)
                while (true) { val n = input.read(buffer); if (n < 0) break
                    require(output.size() + n <= 25 * 1024 * 1024) { Words[R.string.assistant_file_too_large] }; output.write(buffer, 0, n) }
                output.toByteArray()
            } ?: error(Words[R.string.assistant_attachment_unopened])
            if (bytes.size > 3_500_000 && name.substringAfterLast('.').lowercase() in setOf("jpg", "jpeg", "png", "webp")) {
                val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                val options = BitmapFactory.Options().apply { inSampleSize = (maxOf(bounds.outWidth, bounds.outHeight) / 2400).coerceAtLeast(1) }
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)?.let { bitmap ->
                    val scale = (2400f / maxOf(bitmap.width, bitmap.height)).coerceAtMost(1f)
                    val scaled = Bitmap.createScaledBitmap(bitmap, (bitmap.width * scale).toInt().coerceAtLeast(1), (bitmap.height * scale).toInt().coerceAtLeast(1), true)
                    val output = ByteArrayOutputStream(); scaled.compress(Bitmap.CompressFormat.JPEG, 85, output); bytes = output.toByteArray()
                    if (scaled !== bitmap) scaled.recycle(); bitmap.recycle(); name = name.substringBeforeLast('.') + ".jpg"
                }
            }
            return name to bytes
        }
    }
}
