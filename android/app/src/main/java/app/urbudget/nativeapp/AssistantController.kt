package app.urbudget.nativeapp

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Base64
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
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
    val models: List<JSONObject> = emptyList(),
    val threads: List<JSONObject> = emptyList(),
    val thread: JSONObject? = null,
    val entries: List<JSONObject> = emptyList(),
    val running: Boolean = false,
    val attachments: List<JSONObject> = emptyList(),
    val busy: Set<String> = emptySet(),
    val streaming: String = "",
    val thinking: String = "",
    val error: String? = null,
    val notice: String? = null,
    val undoProposal: String? = null,
    /** A message put in the composer for the user to send (or not), such as one asking to fix an import. */
    val draft: String? = null,
) {
    val threadId get() = thread?.optString("id").orEmpty()
}

/** Native presentation state; providers, tools, history, validation and proposals live in Rust. */
class AssistantController : ViewModel() {
    private val mutable = MutableStateFlow(AssistantState())
    val state = mutable.asStateFlow()
    private var undoToken: String? = null
    private val stream = AssistantStreamBuffer()
    private var streamPublish: Job? = null
    private val reloads = Channel<String>(Channel.CONFLATED)
    private val threadLoader = Mutex()
    private var presentation: AssistantThreadPresentation? = null
    private var conversationGeneration = 0L

    init {
        refresh()
        viewModelScope.launch {
            NativeServices.events.collect { envelope ->
                if (envelope.optString("event") != "assistant://event") return@collect
                val event = envelope.optJSONObject("payload") ?: return@collect
                if (event.optString("threadId") != state.value.threadId) return@collect
                when (event.optString("type")) {
                    "started" -> mutable.update { it.copy(running = true) }
                    "delta", "thinking" -> {
                        stream.append(event.optString("text"), thinking = event.optString("type") == "thinking")
                        scheduleStreamPublish()
                    }
                    "message", "step", "proposal", "error", "done" -> {
                        if (event.optString("type") == "message") { stream.clearReply(); publishStream() }
                        reloadThread()
                        if (event.optString("type") == "done") reloadHistory()
                    }
                }
            }
        }
        viewModelScope.launch {
            while (true) { delay(1200); if (state.value.running) reloadThread() }
        }
        viewModelScope.launch {
            for (first in reloads) {
                delay(80) // Tool/message bursts need one history fetch, not one per event.
                var id = first
                while (true) { id = reloads.tryReceive().getOrNull() ?: break }
                if (state.value.threadId != id) continue
                try { loadThread(id) }
                catch (cancel: CancellationException) { throw cancel }
                catch (failure: Exception) { if (state.value.threadId == id) mutable.update { it.copy(error = failure.message) } }
            }
        }
    }

    private fun scheduleStreamPublish() {
        if (streamPublish?.isActive == true) return
        streamPublish = viewModelScope.launch { delay(64); publishStream() }
    }
    private fun publishStream() { stream.drain()?.let { (reply, thinking) -> mutable.update { it.copy(streaming = reply, thinking = thinking) } } }
    private fun clearStream() { streamPublish?.cancel(); streamPublish = null; stream.clear() }

    private suspend fun call(method: String, params: JSONObject = JSONObject()): Any? =
        withContext(Dispatchers.IO) { NativeServices.call(method, params) }

    private fun operation(label: String, work: suspend () -> Unit) {
        if (label in state.value.busy) return
        mutable.update { it.copy(busy = it.busy + label, error = null) }
        viewModelScope.launch {
            try { work() }
            catch (cancel: CancellationException) { throw cancel }
            catch (failure: Exception) { mutable.update { it.copy(error = failure.message ?: Words[R.string.assistant_action_failed]) } }
            finally { mutable.update { it.copy(busy = it.busy - label) } }
        }
    }

    private suspend fun loadStatus() {
        val previous = state.value.status
        val status = call("assistant_status") as JSONObject
        mutable.update { it.copy(status = status) }
        if (status.optBoolean("ready") && (previous?.optString("provider") != status.optString("provider") || previous?.optBoolean("ready") != true)) {
            operation("models") { loadModels(false) }
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
        val thread = call("assistant_thread", JSONObject().put("id", id)) as JSONObject
        val prior = presentation
        val prepared = withContext(Dispatchers.Default) { assistantThreadPresentation(thread, prior) }
        // A reply from a conversation left while this request ran must not navigate back.
        if (state.value.threadId == id && generation == conversationGeneration) {
            presentation = prepared
            val running = thread.optBoolean("running")
            if (!running) clearStream()
            mutable.update {
                it.copy(thread = prepared.thread, entries = prepared.entries, running = running,
                    streaming = if (running) it.streaming else "", thinking = if (running) it.thinking else "")
            }
        }
    }
    fun refresh() = operation("refresh") {
        loadStatus(); loadHistory()
        val id = state.value.threadId
        if (id.isNotEmpty() && state.value.threads.any { it.optString("id") == id }) loadThread(id)
        else if (id.isNotEmpty()) newChat()
    }
    fun reloadThread() { state.value.threadId.takeIf(String::isNotEmpty)?.let(reloads::trySend) }
    private fun reloadHistory() = operation("history") { loadHistory() }
    fun dismissError() { mutable.update { it.copy(error = null) } }
    fun dismissNotice() { mutable.update { it.copy(notice = null) } }
    fun note(text: String) { mutable.update { it.copy(notice = text) } }
    /** Puts `text` in the composer, unsent. */
    fun prefill(text: String) { mutable.update { it.copy(draft = text) } }
    fun prefilled() { mutable.update { it.copy(draft = null) } }
    fun newChat() {
        conversationGeneration++; clearStream(); presentation = null
        mutable.update { it.copy(thread = null, entries = emptyList(), running = false, attachments = emptyList(), streaming = "", thinking = "", undoProposal = null) }; undoToken = null
    }
    fun openThread(id: String) = operation("open") {
        conversationGeneration++; clearStream(); presentation = null
        mutable.update { it.copy(thread = JSONObject().put("id", id), entries = emptyList(), running = false, streaming = "", thinking = "", attachments = emptyList(), undoProposal = null) }
        undoToken = null; loadThread(id)
    }
    fun deleteThread(id: String) = operation("delete") {
        call("assistant_delete_thread", JSONObject().put("id", id))
        if (state.value.threadId == id) newChat()
        loadHistory()
    }
    fun deleteAll() = operation("delete") { call("assistant_delete_all"); newChat(); loadHistory() }
    fun settings(change: JSONObject) = operation("settings") {
        call("assistant_settings", JSONObject().put("change", change)); loadStatus()
    }
    fun models() = operation("models") { loadModels(true) }
    fun signIn() = operation("signIn") {
        mutable.update { it.copy(status = it.status?.copy()?.put("signingIn", true)) }
        try { call("assistant_sign_in") } finally { loadStatus() }
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
        if (generation == conversationGeneration) mutable.update { it.copy(attachments = it.attachments + attachment) }
    }
    fun removeAttachment(id: String) { mutable.update { it.copy(attachments = it.attachments.filterNot { file -> file.optString("id") == id }) } }
    /** `import` is the bank file being imported, when the assistant is open over its import: the assistant is told its lines. */
    fun send(text: String, month: String, selected: List<String> = emptyList(), import: JSONObject? = null, done: () -> Unit) = operation("send") {
        require(selected.size <= 200) { Words[R.string.assistant_select_limit] }
        val current = state.value
        val generation = conversationGeneration
        clearStream()
        mutable.update { it.copy(streaming = "", thinking = "") }
        val result = call("assistant_send", JSONObject().put("threadId", current.threadId.takeIf(String::isNotEmpty) ?: JSONObject.NULL)
            .put("text", text).put("attachments", JSONArray(current.attachments.map { it.optString("id") }))
            .put("screen", screen(month, selected, import))) as String
        if (generation != conversationGeneration) return@operation
        mutable.update { it.copy(thread = if (it.threadId == result) it.thread else JSONObject().put("id", result), running = true, attachments = emptyList()) }
        done(); loadThread(result); loadHistory()
    }
    fun retry(month: String, import: JSONObject? = null) = operation("send") {
        call("assistant_retry", JSONObject().put("threadId", state.value.threadId).put("screen", screen(month, emptyList(), import)))
        loadThread(state.value.threadId)
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
        private fun screen(month: String, selected: List<String>, import: JSONObject? = null) = JSONObject().put("platform", "android")
            .put("page", if (import != null) "import" else "assistant").put("month", month)
            .put("today", LocalDate.now().toString()).put("selected", JSONArray(selected)).put("import", import ?: JSONObject.NULL)
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
