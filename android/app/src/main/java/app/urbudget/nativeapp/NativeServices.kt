package app.urbudget.nativeapp

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicBoolean

/** IO-only calls to the same account, sync, bank and assistant implementations used by desktop. */
object NativeServices {
    @Volatile private var ready = false
    private val eventSource = MutableSharedFlow<JSONObject>(extraBufferCapacity = 256)
    val events = eventSource.asSharedFlow()
    private val savedSource = MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    private val reads = SharedServiceReads(CoroutineScope(SupervisorJob() + Dispatchers.IO))
    private val readEpoch = AtomicLong()
    private val automaticSyncRunning = AtomicBoolean(false)
    val saved = savedSource.asSharedFlow()
    private val syncCompletedSource = MutableSharedFlow<JSONObject>(replay = 1, extraBufferCapacity = 16)
    val syncCompleted = syncCompletedSource.asSharedFlow()
    @Volatile private var application: Context? = null
    private val syncStatusSource = MutableStateFlow<JSONObject?>(null)
    val syncStatus = syncStatusSource.asStateFlow()
    fun notifySaved() {
        syncStatusSource.value?.let { syncStatusSource.value = JSONObject(it.toString()).put("libraryPending", true) }
        savedSource.tryEmit(Unit)
        application?.let { SyncWorker.enqueue(it) }
    }
    /** The interface's language, "en" or "ar", for the engine's own messages (docs/LOCALIZATION.md). */
    @Volatile private var language: String? = null
    /** One at a time and in order, so the engine always ends on the latest language. */
    private val languageLane = Dispatchers.IO.limitedParallelism(1)
    private val languageScope = CoroutineScope(SupervisorJob() + languageLane)
    @Volatile private var foreground = false
    private val foregroundScope = CoroutineScope(SupervisorJob() + Dispatchers.IO.limitedParallelism(1))

    fun setForeground(active: Boolean) {
        foreground = active
        if (ready) foregroundScope.launch { sendForeground() }
    }
    private fun sendForeground() {
        runCatching { call("assistant_foreground", JSONObject().put("active", foreground)) }
    }

    @Synchronized fun initialize(context: Context) {
        if (ready) return
        // In-place upgrade from the released Tauri Android app keeps its library and sealed vault.
        val directory = if (File(context.filesDir, "budgets.json").exists() || File(context.filesDir, "budget.sqlite3").exists()) context.filesDir else File(context.filesDir, "native")
        decode(NativeBridge.initialize(context.applicationContext, directory.absolutePath))
        ready = true
        application = context.applicationContext
        refreshSyncStatus()
        foregroundScope.launch { sendForeground() }
        runBlocking(languageLane) { sendLanguage() }
    }

    /** Tells the engine the language the interface resolved to, now if it has started, else as it starts. */
    fun setLanguage(code: String) {
        if (language == code) return
        language = code
        if (ready) languageScope.launch { sendLanguage() }
    }

    /** An engine without `set_language` keeps its own messages in English; that's not an error here. */
    private fun sendLanguage() {
        val code = language ?: return
        runCatching { call("set_language", JSONObject().put("language", code)) }
    }

    fun call(method: String, params: JSONObject = JSONObject()): Any? {
        check(ready) { Words[R.string.error_services_starting] }
        val changesReadContext = method in setOf("account_verify", "account_sign_out", "account_delete", "simplefin_connect", "simplefin_disconnect", "billing_verify")
        if (changesReadContext) readEpoch.incrementAndGet()
        try {
            val result = decode(NativeBridge.service(JSONObject().put("version", 1).put("method", method).put("params", params).toString()))
            if (method in setOf("create_budget", "library_trash", "library_restore", "library_recover", "sync_enable", "sync_setup", "save_budget", "native_apply_patch", "assistant_apply_commit", "assistant_undo_commit", "simplefin_import", "accept_ynab_import", "resolve_ynab", "undo_budget", "redo_budget", "restore_backup_file", "restore_budget", "apply_due_schedules")) notifySaved()
            if (method in setOf("sync_now", "sync_resolve") && result is JSONObject) syncCompletedSource.tryEmit(result)
            if (method in setOf("account_verify", "account_sign_out", "account_delete", "sync_setup", "sync_enable", "sync_reset", "sync_lock")) refreshSyncStatus()
            return result
        } finally {
            // Reads started while a credential transition was in progress must not be joined
            // by a new screen after the transition has finished (including partial failures).
            if (changesReadContext) readEpoch.incrementAndGet()
        }
    }

    /** Results are read-only shared snapshots. Only idempotent reads/downloads may be joined. */
    suspend fun read(method: String, params: JSONObject = JSONObject(), scopeKey: String = ""): Any? {
        require(NativeServicePolicy.coalesce(method)) { "This operation must run as an ordered command." }
        val encoded = withContext(Dispatchers.Default) { params.toString() }
        return reads.request("$method\u0000${readEpoch.get()}\u0000$scopeKey\u0000$encoded") { call(method, JSONObject(encoded)) }
    }

    /** Periodic, resume and save notifications share one automatic attempt across Activities. */
    fun syncAutomatically(): JSONObject? {
        if (!automaticSyncRunning.compareAndSet(false, true)) return null
        try {
            val status = call("cloud_status") as JSONObject
            if (!syncCanReconcile(status)) return null
            return (call("sync_now") as JSONObject).also {
                refreshSyncStatus()
            }
        } finally {
            runCatching { refreshSyncStatus() }
            automaticSyncRunning.set(false)
        }
    }

    private fun refreshSyncStatus() {
        val status = call("cloud_status") as JSONObject
        val entries = (call("library_snapshot") as JSONObject).getJSONArray("entries")
        status.put("libraryPending", (0 until entries.length()).any { entries.getJSONObject(it).optBoolean("pending") })
        syncStatusSource.value = status
        application?.let { SyncWorker.configure(it, syncBackgroundEligible(status)) }
    }

    suspend fun pollEvents() {
        if (!ready) return
        val events = call("host_events") as? JSONArray ?: return
        for (index in 0 until events.length()) events.optJSONObject(index)?.let { eventSource.emit(it) }
    }

    private fun decode(json: String): Any? {
        val response = JSONObject(json)
        check(response.optBoolean("ok")) { response.optString("error").ifEmpty { Words[R.string.error_service_failed] } }
        return response.opt("value").takeUnless { it == JSONObject.NULL }
    }
}
