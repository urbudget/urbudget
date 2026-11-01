package app.urbudget.nativeapp.ui

import android.Manifest
import android.content.Intent
import android.database.ContentObserver
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.provider.Telephony
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

private data class SmsLoaded(val sources: List<SmsSource>, val messages: List<BankSms>, val count: Int, val batch: SmsBatch?, val mappings: JSONArray,
                             val consent: String?, val auto: Boolean, val coverage: List<SmsCoverage>, val last: JSONObject?)
/** Texts to read, into [base] when they join the review that's open. */
private data class SmsPrepare(val messages: List<BankSms>, val note: String, val connection: JSONObject, val base: SmsBatch?)

/** How far back to collect, as the sender picker and the setting offer it: "all" is everything on the phone. */
private val smsRanges = listOf("0" to R.string.sms_new_only, "30" to R.string.sms_last_30, "90" to R.string.sms_last_90,
    "180" to R.string.sms_last_180, "365" to R.string.sms_last_365, "all" to R.string.sms_everything)
private fun smsSince(range: String): Long = if (range == "all") 0L else System.currentTimeMillis() - range.toLong() * 86_400_000L

/** Who reads the messages, as the consent and the status name it: "Anthropic · claude-…". */
@Composable
private fun reader(connection: JSONObject): String {
    val provider = when (connection.text("provider")) { "anthropic" -> "Anthropic"; "openai" -> "OpenAI"; "chatgpt" -> "ChatGPT"; else -> connection.text("provider") }
    return provider + " · " + connection.text("model").ifBlank { stringResource(R.string.sms_default_model) }
}

/** Message text keeps its own direction: an Arabic alert reads right to left in an English app. */
internal val SmsBodyStyle @Composable get() = MaterialTheme.typography.bodyMedium.copy(textDirection = TextDirection.Content)

@Composable
internal fun SmsImportScreen(budget: JSONObject, vm: BudgetViewModel, setDockHidden: (Boolean) -> Unit = {}, analyzer: SmsAnalyzer = NativeSmsAnalyzer) {
    val identity = vm.state.value.budgetIdentity ?: return
    key(identity) { SmsHome(budget, vm, identity, analyzer, setDockHidden) }
}

/**
 * Bank messages, built around what needs the user. With permission to send, opening the page reads
 * what's waiting; codes, alerts and texts already in the budget are handled without a question, and
 * what's left is counted with one button to review it. The texts themselves, and the settings, wait
 * below.
 */
@Composable
private fun SmsHome(budget: JSONObject, vm: BudgetViewModel, identity: String, analyzer: SmsAnalyzer, setDockHidden: (Boolean) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current
    val inbox = remember { SmsInbox.get(context) }
    var allowed by remember { mutableStateOf(SmsReader.canRead(context)) }
    var receiving by remember { mutableStateOf(SmsReader.canReceive(context)) }
    var sources by remember { mutableStateOf<List<SmsSource>>(emptyList()) }
    // Oldest first, the order they're read in; the list shows them newest first.
    var messages by remember { mutableStateOf<List<BankSms>>(emptyList()) }
    var pendingCount by remember { mutableIntStateOf(0) }
    var batch by remember { mutableStateOf<SmsBatch?>(null) }
    var mappings by remember { mutableStateOf(JSONArray()) }
    var consent by remember { mutableStateOf<String?>(null) }
    var auto by remember { mutableStateOf(false) }
    var coverage by remember { mutableStateOf<List<SmsCoverage>>(emptyList()) }
    var last by remember { mutableStateOf<JSONObject?>(null) }
    var connection by remember { mutableStateOf<JSONObject?>(null) }
    var loading by remember { mutableStateOf(true) }
    var error by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    val readings by SmsReadings.state.collectAsState()
    val vmState by vm.state.collectAsState()
    val reading = readings?.takeIf { it.budget == identity }
    // Opening the page reads what's waiting once a visit; a reading the user stopped stays stopped.
    var started by remember { mutableStateOf(false) }
    var sorted by remember { mutableStateOf<SmsSorted?>(null) }
    var settling by remember { mutableStateOf(false) }
    var asking by remember { mutableStateOf<SmsPrepare?>(null) }
    var reviewing by remember { mutableStateOf(false) }
    var listing by remember { mutableStateOf(false) }
    var selecting by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf<Set<String>>(emptySet()) }
    var pickSenders by remember { mutableStateOf(false) }
    var collecting by remember { mutableStateOf(false) }
    var showCards by remember { mutableStateOf(false) }
    var revoke by remember { mutableStateOf(false) }
    var clear by remember { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        allowed = SmsReader.canRead(context); receiving = SmsReader.canReceive(context)
        if (allowed) pickSenders = sources.isEmpty() else error = context.getString(R.string.sms_permission_needed)
        reload++
    }
    fun save(name: String, value: String?) {
        scope.launch { try { withContext(smsStorageLane) { inbox.saveState(identity, name, value) } } catch (failure: Exception) { error = failure.message } }
    }
    fun setAuto(on: Boolean) { auto = on; save("auto", if (on) "on" else null) }
    // Reading in the background works without notifications; with them, it says when something needs the user.
    val notifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { setAuto(true) }
    DisposableEffect(lifecycle, context) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) {
            allowed = SmsReader.canRead(context); receiving = SmsReader.canReceive(context); reload++
        } }
        lifecycle.lifecycle.addObserver(observer)
        onDispose { lifecycle.lifecycle.removeObserver(observer) }
    }
    DisposableEffect(allowed) {
        val observer = object : ContentObserver(Handler(Looper.getMainLooper())) { override fun onChange(selfChange: Boolean) { reload++ } }
        if (allowed) context.contentResolver.registerContentObserver(Telephony.Sms.CONTENT_URI, true, observer)
        onDispose { context.contentResolver.unregisterContentObserver(observer) }
    }
    val reviewBatch = batch?.takeIf { reviewing && allowed && reading == null }
    DisposableEffect(selecting, reviewBatch != null) { setDockHidden(selecting || reviewBatch != null); onDispose { setDockHidden(false) } }
    androidx.activity.compose.BackHandler(selecting) { selecting = false; selected = emptySet() }
    LaunchedEffect(reload, allowed) {
        if (!allowed) { batch = null; messages = emptyList(); reviewing = false }
        try {
            val loaded = withContext(smsStorageLane) {
                val found = if (allowed) SmsReader.refresh(context, identity) else emptyList()
                val pending = if (allowed) inbox.pending(identity) else emptyList()
                val draft = inbox.state(identity, "draft")
                // A draft whose messages are gone, or from before this review, can't be shown: its messages wait again.
                val restored = if (allowed && batch == null) SmsBatch.restore(draft) { ids -> inbox.waiting(identity, ids) }.also { if (it == null && draft != null) inbox.saveState(identity, "draft", null) } else batch
                SmsLoaded(inbox.sources(identity), pending, inbox.pendingCount(identity), restored, JSONArray(inbox.state(identity, "mappings") ?: "[]"),
                    inbox.state(identity, "consent"), inbox.state(identity, "auto") == "on", found, inbox.state(identity, "last")?.let { runCatching { JSONObject(it) }.getOrNull() })
            }
            sources = loaded.sources; messages = loaded.messages; pendingCount = loaded.count
            batch = loaded.batch; mappings = loaded.mappings; consent = loaded.consent; auto = loaded.auto; coverage = loaded.coverage; last = loaded.last
            selected = selected.intersect(messages.map { it.id }.toSet())
            connection = runCatching { analyzer.status() }.getOrNull()
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        finally { loading = false }
    }
    // A reading that ends, here or in the background, leaves its review saved: show it from there.
    val finished by SmsReadings.finished.collectAsState()
    var seen by remember { mutableLongStateOf(SmsReadings.finished.value) }
    LaunchedEffect(finished) { if (finished != seen) { seen = finished; batch = null; sorted = null; reload++ } }
    fun saveBatch(value: SmsBatch?) {
        batch = value
        save("draft", value?.saved())
    }
    fun saveMappings(value: JSONArray) {
        mappings = value
        save("mappings", value.toString())
    }
    /** Ends the review, the texts it settled leaving Bank messages; [summary] is what was handled without a question. */
    fun finish(ids: List<String>, summary: JSONObject = JSONObject()) {
        scope.launch {
            try {
                withContext(smsStorageLane) {
                    inbox.finish(identity, ids)
                    inbox.saveState(identity, "last", summary.put("at", System.currentTimeMillis()).toString())
                }
                batch = null; sorted = null; reviewing = false; selected = emptySet(); reload++
            }
            catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
            finally { settling = false }
        }
    }
    // Dismissing waiting messages leaves an open review as it is.
    fun dismiss(ids: List<String>) {
        scope.launch {
            try { withContext(smsStorageLane) { inbox.finish(identity, ids, endReview = false) }; selecting = false; selected = emptySet(); reload++ }
            catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    }
    fun send(prepare: SmsPrepare) {
        error = null; reviewing = false; selecting = false; selected = emptySet(); sorted = null
        SmsReadings.start(context, analyzer, identity, prepare.connection, prepare.messages, prepare.note, prepare.base,
            still = { SmsReader.canRead(context) && vm.state.value.budgetIdentity == identity }, failed = { error = it })
    }
    fun prepare(chosen: List<BankSms>, note: String = "", base: SmsBatch? = null) {
        scope.launch {
            try {
                // Asked fresh: the assistant's connection may have changed since the page opened.
                val current = analyzer.status().also { connection = it }
                check(current.optBoolean("ready")) { context.getString(R.string.sms_connect_assistant) }
                val request = SmsPrepare(chosen, note, current, base)
                if (smsConsented(consent, current)) send(request) else asking = request
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    }
    val ready = connection?.optBoolean("ready") == true
    val consented = connection?.let { smsConsented(consent, it) } == true
    val names = sources.joinToString(", ") { Money.own(it.sender) }
    // What's waiting is what the review doesn't hold yet.
    val inReview = remember(batch) { batch?.messages?.map { it.id }?.toSet().orEmpty() }
    val waiting = remember(messages, inReview) { messages.filter { it.id !in inReview } }
    val waitingCount = (pendingCount - inReview.size).coerceAtLeast(0)
    val since = sources.minOfOrNull { it.since }
    LaunchedEffect(loading, consented, ready, reading == null, waiting.isEmpty()) {
        if (!started && !loading && allowed && ready && consented && reading == null && waiting.isNotEmpty()) {
            started = true
            send(SmsPrepare(waiting, "", connection!!, batch))
        }
    }
    // What the review holds, sorted; when nothing in it needs the user, matching what's already in
    // the budget and clearing the rest happens without a question.
    val accounts = remember(budget) { budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") } }
    LaunchedEffect(batch, budget, mappings, reading == null, reviewing) {
        val current = batch ?: return@LaunchedEffect
        if (reading != null || reviewing || settling) return@LaunchedEffect
        sorted = null
        try {
            val items = current.items(SmsAccounts(accounts, mappings))
            val resolved = current.resolved(items)
            val known = smsKnown(vm.smsPreview(budget, current, resolved), items, budget)
            val result = smsSort(current, items, known)
            if (result.needs > 0) { sorted = result; return@LaunchedEffect }
            settling = true
            val matching = result.existing.filter { current.ticked[it.index] != false }.map { it.index to known.getValue(it.index) }
            val summary = JSONObject().put("known", matching.size)
                .put("skipped", JSONObject(result.skipped.groupingBy { it.skip.ifBlank { "unclear" } }.eachCount()))
            val handled = items.flatMap { item -> item.sources.map { it.id } }
            if (matching.isEmpty()) finish(handled, summary)
            else vm.runCommand("smsBatchApply", current.params(resolved).put("decisions", smsDecisions(matching))) { finish(handled, summary) }
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message; settling = false }
    }
    LaunchedEffect(vmState.error) { if (settling && vmState.error != null) { error = vmState.error; settling = false } }
    if (reviewBatch != null) SmsReview(budget, vm, reviewBatch, mappings, close = { reviewing = false }, change = ::saveBatch, saveCards = ::saveMappings, finish = { finish(it) },
        again = { note -> prepare(reviewBatch.messages, note) }, discard = { saveBatch(null); reviewing = false })
    else Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize().testTag("sms-inbox"), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 8.dp, bottom = if (selecting) 120.dp else DockRoom),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (!allowed) item {
                ListCard { Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(stringResource(R.string.sms_intro_title), style = MaterialTheme.typography.titleMedium, color = ur.text)
                    Text(stringResource(R.string.sms_intro), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                    Notice(stringResource(R.string.sms_disclosure), tone = "info")
                    PrimaryButton(stringResource(R.string.sms_allow), { permission.launch(arrayOf(Manifest.permission.READ_SMS, Manifest.permission.RECEIVE_SMS)) }, Modifier.fillMaxWidth())
                    TextButton({ context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}"))) },
                        Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.sms_open_settings), color = ur.accent) }
                } }
            }
            error?.let { item { FormError(it) } }
            if (allowed && sources.isEmpty() && !loading) item {
                EmptyState(stringResource(R.string.sms_choose_bank_title), stringResource(R.string.sms_choose_bank_detail), stringResource(R.string.sms_choose_senders)) { pickSenders = true }
            }
            if (allowed && sources.isNotEmpty()) {
                item(key = "status") {
                    ListCard(Modifier.testTag("sms-status")) { Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        val current = batch
                        val found = sorted
                        when {
                            reading != null -> {
                                // More than one request: stopping keeps what's been read, so it says Stop rather than Cancel.
                                val requests = reading.total > SmsBatchSize
                                Text(stringResource(R.string.sms_reading_by, connection?.let { reader(it) }.orEmpty()), style = MaterialTheme.typography.titleMedium, color = ur.text)
                                Text(countText(R.plurals.sms_reading_progress, reading.total, reading.read.toString()), style = MaterialTheme.typography.bodyMedium, color = ur.text2,
                                    modifier = Modifier.testTag("sms-progress"))
                                LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
                                if (requests) Text(stringResource(R.string.sms_reading_keep), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                                TextButton({ SmsReadings.stop() }, Modifier.align(Alignment.End).testTag("sms-stop")) {
                                    Text(stringResource(if (requests) R.string.sms_stop_reading else R.string.common_cancel), color = ur.accent)
                                }
                            }
                            current != null && found == null -> {
                                Text(stringResource(R.string.sms_checking), style = MaterialTheme.typography.titleMedium, color = ur.text)
                                LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
                            }
                            current != null && found != null -> {
                                Text(countText(R.plurals.sms_to_review, found.needs), style = MaterialTheme.typography.titleMedium, color = ur.text, modifier = Modifier.testTag("sms-needs"))
                                if (found.existing.isNotEmpty() || found.skipped.isNotEmpty())
                                    Text(handledSummary(found.existing.size, found.skipped), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                                PrimaryButton(stringResource(R.string.sms_review_open), { reviewing = true }, Modifier.fillMaxWidth().testTag("sms-review"))
                                if (waitingCount > 0) {
                                    Text(countText(R.plurals.sms_more_waiting, waitingCount), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                                    if (ready) SecondaryButton(countText(R.plurals.sms_read_more, waitingCount), { prepare(waiting, base = current) },
                                        Modifier.fillMaxWidth().testTag("sms-read-more"), enabled = !loading && waiting.isNotEmpty())
                                }
                                TextButton({ saveBatch(null); sorted = null }, Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.sms_discard_review), color = ur.accent) }
                            }
                            waitingCount > 0 -> {
                                Text(countText(R.plurals.sms_new_messages, waitingCount), style = MaterialTheme.typography.titleMedium, color = ur.text)
                                Text(stringResource(R.string.sms_from, names), style = MaterialTheme.typography.bodyMedium, color = ur.text2, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                if (ready) {
                                    PrimaryButton(if (consented) countText(R.plurals.sms_read_count, waitingCount) else stringResource(R.string.sms_read_with, connection?.let { reader(it) }.orEmpty()),
                                        { prepare(waiting) }, Modifier.fillMaxWidth().testTag("sms-prepare"), enabled = !loading && waiting.isNotEmpty())
                                    Text(stringResource(R.string.sms_reads_note), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                                } else if (connection != null) Notice(stringResource(R.string.sms_connect_assistant), tone = "info")
                            }
                            else -> {
                                Text(stringResource(R.string.sms_up_to_date), style = MaterialTheme.typography.titleMedium, color = ur.text, modifier = Modifier.testTag("sms-up-to-date"))
                                Text(last?.let { lastText(it) } ?: stringResource(R.string.sms_up_to_date_detail, names), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                            }
                        }
                    } }
                }
                if (!receiving) item { Notice(stringResource(R.string.sms_receive_needed), action = stringResource(R.string.sms_allow), onAction = { permission.launch(arrayOf(Manifest.permission.READ_SMS, Manifest.permission.RECEIVE_SMS)) }) }
                if (waiting.isNotEmpty()) {
                    // The texts themselves, mostly codes and alerts, wait folded below what needs the user.
                    item(key = "messages") {
                        ListCard(Modifier.testTag("sms-waiting")) {
                            ListRow(stringResource(R.string.sms_waiting_title), subtitle = stringResource(R.string.sms_waiting_detail), onClick = { listing = !listing; if (!listing) { selecting = false; selected = emptySet() } },
                                chevron = false, trailing = { Text(stringResource(if (listing) R.string.sms_hide else R.string.sms_show), color = ur.accent, fontWeight = FontWeight.SemiBold) })
                        }
                    }
                    if (listing) {
                        item(key = "choose") {
                            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                                if (reading == null) TextButton({ selecting = !selecting; selected = emptySet() }, Modifier.testTag("sms-choose")) {
                                    Text(stringResource(if (selecting) R.string.common_done else R.string.sms_choose_messages), color = ur.accent, fontWeight = FontWeight.SemiBold)
                                }
                            }
                        }
                        val shown = waiting.asReversed()
                        itemsIndexed(shown, key = { _, it -> it.id }) { index, message ->
                            SmsMessageRow(message, index, shown.size, sources.size > 1, if (selecting) message.id in selected else null,
                                enabled = true) { on -> selected = if (on) selected + message.id else selected - message.id }
                        }
                    }
                }
                item(key = "settings") {
                    Column(Modifier.padding(top = 12.dp)) { ListCard {
                        ListRow(stringResource(R.string.sms_senders), subtitle = names, onClick = { if (reading == null) pickSenders = true }, divider = true)
                        ListRow(stringResource(R.string.sms_collect), subtitle = since?.let { smsSinceText(it) }, onClick = { if (reading == null) collecting = true }, divider = true)
                        if (consented) ListRow(stringResource(R.string.sms_auto), subtitle = stringResource(R.string.sms_auto_detail), onClick = {
                            if (auto) setAuto(false)
                            else if (android.os.Build.VERSION.SDK_INT >= 33 && androidx.core.content.ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED)
                                notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                            else setAuto(true)
                        }, divider = true, chevron = false, trailing = { Switch(auto, null, Modifier.testTag("sms-auto")) })
                        ListRow(stringResource(R.string.sms_cards), subtitle = if (mappings.length() == 0) stringResource(R.string.sms_cards_none) else countText(R.plurals.sms_cards_count, mappings.length()),
                            onClick = { showCards = true }, divider = true)
                        // A permission for another assistant, or from before reading on open, no longer counts: the next reading asks.
                        if (consented) ListRow(stringResource(R.string.sms_sending), subtitle = stringResource(R.string.sms_sending_allowed), onClick = { revoke = true }, divider = true)
                        ListRow(stringResource(R.string.sms_stop), titleColor = ur.negative, onClick = { if (reading == null) clear = true }, chevron = false)
                    } }
                }
                // How to know it's reading everything: what the phone holds from each sender, against what's collected.
                if (coverage.isNotEmpty()) item(key = "coverage") {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(stringResource(R.string.sms_on_phone), style = MaterialTheme.typography.titleSmall, color = ur.text, modifier = Modifier.padding(top = 4.dp))
                        ListCard(Modifier.testTag("sms-coverage")) {
                            coverage.forEachIndexed { index, c ->
                                ListRow(Money.own(c.sender), subtitle = coverageText(c), chevron = false, divider = index < coverage.lastIndex)
                            }
                        }
                    }
                }
            }
        }
        if (selecting) Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().background(ur.bg).navigationBarsPadding().padding(horizontal = Gutter, vertical = 12.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                SecondaryButton(countText(R.plurals.sms_dismiss_count, selected.size), { dismiss(selected.toList()) }, Modifier.weight(1f), enabled = selected.isNotEmpty())
                PrimaryButton(countText(R.plurals.sms_read_count, selected.size), { prepare(waiting.filter { it.id in selected }, base = batch) }, Modifier.weight(1f).testTag("sms-prepare-selected"),
                    enabled = selected.isNotEmpty() && ready && reading == null)
            }
        }
    }
    if (pickSenders && allowed) SmsSenderPicker(sources, { pickSenders = false }, save = { chosen, since ->
        scope.launch {
            try { withContext(smsStorageLane) { inbox.configure(identity, chosen, since) }; batch = null; sorted = null; pickSenders = false; reload++ }
            catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    })
    if (collecting) {
        var range by remember { mutableStateOf("") }
        FormDialog(stringResource(R.string.sms_collect), { collecting = false }, enabled = range.isNotEmpty(), onConfirm = {
            val start = smsSince(range)
            collecting = false
            scope.launch {
                try { withContext(smsStorageLane) { inbox.collectFrom(identity, start, inReview) }; started = false; reload++ }
                catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
            }
        }) {
            Text(stringResource(R.string.sms_collect_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            since?.let { Text(stringResource(R.string.sms_collect_now, smsSinceText(it)), style = MaterialTheme.typography.bodySmall, color = ur.muted) }
            Box(Modifier.testTag("sms-collect")) { Choice(stringResource(R.string.sms_history), range, smsRanges.map { it.first to stringResource(it.second) }) { range = it } }
        }
    }
    if (showCards) SmsCardsSheet(budget, mappings, { showCards = false }, ::saveMappings)
    asking?.let { request ->
        FormDialog(stringResource(R.string.sms_send_title), { asking = null }, confirm = stringResource(R.string.sms_send_confirm), onConfirm = {
            asking = null
            val saved = smsConsent(request.connection)
            consent = saved
            save("consent", saved)
            started = true
            send(request)
        }) {
            Text(stringResource(R.string.sms_send_notice, reader(request.connection)), style = MaterialTheme.typography.bodyMedium, color = ur.text, modifier = Modifier.testTag("sms-consent"))
            Text(stringResource(R.string.sms_send_later), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
    }
    if (revoke) AlertDialog(onDismissRequest = { revoke = false }, title = { Text(stringResource(R.string.sms_revoke_title)) }, text = { Text(stringResource(R.string.sms_revoke_detail)) },
        confirmButton = { TextButton({
            revoke = false; consent = null; setAuto(false)
            save("consent", null)
        }) { Text(stringResource(R.string.sms_revoke)) } },
        dismissButton = { TextButton({ revoke = false }) { Text(stringResource(R.string.common_cancel)) } })
    if (clear) AlertDialog(onDismissRequest = { clear = false }, title = { Text(stringResource(R.string.sms_stop)) }, text = { Text(stringResource(R.string.sms_stop_detail)) },
        confirmButton = { TextButton({ scope.launch { try { withContext(smsStorageLane) { inbox.clear(identity) }; clear = false; batch = null; sorted = null; consent = null; auto = false; reload++ } catch (failure: Exception) { error = failure.message } } }) {
            Text(stringResource(R.string.sms_stop), color = ur.negative)
        } },
        dismissButton = { TextButton({ clear = false }) { Text(stringResource(R.string.common_cancel)) } })
}

/** "Checked 11:38 · 2 already in your budget · 3 codes": when the page last finished, and what it handled. */
@Composable
private fun lastText(last: JSONObject): String {
    val at = last.optLong("at").takeIf { it > 0 } ?: return ""
    val time = remember(at) {
        val moment = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault())
        if (moment.toLocalDate() == java.time.LocalDate.now()) moment.toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
        else Money.date(moment.toLocalDate().toString())
    }
    val skipped = last.optJSONObject("skipped")
    val handled = listOfNotNull(
        last.optInt("known").takeIf { it > 0 }?.let { countText(R.plurals.sms_known, it) },
        skipped?.let { s -> s.keys().asSequence().toList().sortedByDescending { s.optInt(it) }.map { skip -> skip to s.optInt(skip) } }?.map { (skip, n) ->
            countText(when (skip) {
                "code" -> R.plurals.sms_skip_code; "security" -> R.plurals.sms_skip_security; "balance" -> R.plurals.sms_skip_balance
                "declined" -> R.plurals.sms_skip_declined; "rewards" -> R.plurals.sms_skip_rewards; "marketing" -> R.plurals.sms_skip_marketing
                "unclear" -> R.plurals.sms_skip_dismissed; else -> R.plurals.sms_skip_other
            }, n)
        }?.joinToString(" · ")?.ifBlank { null },
    )
    return (listOf(stringResource(R.string.sms_checked, time)) + handled).joinToString(" · ")
}

/** "2,913 texts, 2024-12-13 to 2026-11-01 · SMS: 55 · MMS: 2,858 · All collected". */
@Composable
private fun coverageText(c: SmsCoverage): String {
    val total = c.sms + c.mms + c.unreadable
    if (total == 0 || c.first == null || c.latest == null) return stringResource(R.string.sms_coverage_none)
    fun day(at: Long) = Money.date(Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()).toLocalDate().toString())
    return listOfNotNull(
        countText(R.plurals.sms_coverage, total, day(c.first), day(c.latest)),
        stringResource(R.string.sms_coverage_split, c.sms.toString(), c.mms.toString()),
        if (c.collected >= c.sms + c.mms) stringResource(R.string.sms_coverage_all) else countText(R.plurals.sms_coverage_collected, c.sms + c.mms, c.collected.toString()),
        if (c.unreadable > 0) countText(R.plurals.sms_coverage_unreadable, c.unreadable) else null,
    ).joinToString(" · ")
}

/** Where collecting starts: "Since 1 Oct 2026", or everything on the phone. */
@Composable
private fun smsSinceText(since: Long): String = if (since <= 0L) stringResource(R.string.sms_everything)
    else stringResource(R.string.sms_since, Money.date(Instant.ofEpochMilli(since).atZone(ZoneId.systemDefault()).toLocalDate().toString()))

/** A message in the list: when it came, and its text in its own direction. [checked] is null outside choosing. */
@Composable
private fun SmsMessageRow(message: BankSms, index: Int, count: Int, showSender: Boolean, checked: Boolean?, enabled: Boolean, toggle: (Boolean) -> Unit) {
    val time = remember(message.received) {
        Instant.ofEpochMilli(message.received).atZone(ZoneId.systemDefault()).toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
    }
    Column(Modifier.cardSegment(0, 1, cardFill()).testTag("sms-message:${message.id}")
        .then(if (checked != null) Modifier.toggleable(checked, enabled = enabled, role = Role.Checkbox, onValueChange = toggle) else Modifier)) {
        Row(Modifier.padding(start = if (checked != null) 4.dp else 16.dp, end = 16.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.Top) {
            if (checked != null) Checkbox(checked, null, Modifier.testTag("sms-select:${message.id}"), enabled = enabled, colors = CheckboxDefaults.colors(checkedColor = ur.accent))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(listOfNotNull(Money.own(message.sender).takeIf { showSender }, Money.date(message.receivedDate), time).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 1)
                Text(message.body, style = SmsBodyStyle, color = ur.text, maxLines = 5, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
internal fun SmsSenderPicker(sources: List<SmsSource>, close: () -> Unit, save: (Set<String>, Long) -> Unit,
                             load: (android.content.Context) -> List<SmsSender> = SmsReader::senders) {
    val context = LocalContext.current
    var found by remember { mutableStateOf<List<SmsSender>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var chosen by remember { mutableStateOf(sources.map { it.sender }.toSet()) }
    var search by remember { mutableStateOf("") }
    var range by remember { mutableStateOf("30") }
    var numbers by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        try {
            found = withContext(smsStorageLane) {
                val loaded = load(context)
                // Senders chosen before stay listed even when the phone no longer has their messages.
                loaded + sources.filter { s -> loaded.none { smsSenderKey(it.address) == smsSenderKey(s.sender) } }.map { SmsSender(it.sender, 0, null) }
            }
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
    }
    // Senders already chosen come first; the rest by how many texts they sent.
    val picked = remember(sources) { sources.map { smsSenderKey(it.sender) }.toSet() }
    val matching = found.orEmpty().filter { it.address.contains(search, ignoreCase = true) }.sortedByDescending { smsSenderKey(it.address) in picked }
    val (people, banks) = matching.partition { smsPersonal(it.address) }
    fun toggle(sender: SmsSender, on: Boolean) { chosen = if (on) chosen + sender.address else chosen - sender.address }
    LazyFormDialog(stringResource(R.string.sms_senders), close, confirm = stringResource(R.string.common_save), enabled = found != null && chosen.isNotEmpty() && chosen.size <= 50, onConfirm = {
        // A sender added later collects from where the others do; the setting changes it for all.
        save(chosen, if (sources.isEmpty()) smsSince(range) else sources.minOf { it.since })
    }) {
        item { Text(stringResource(R.string.sms_sender_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
        item { SearchField(search, { search = it }, stringResource(R.string.common_search)) }
        if (sources.isEmpty()) item { Choice(stringResource(R.string.sms_history), range, smsRanges.map { it.first to stringResource(it.second) }) { range = it } }
        error?.let { item { FormError(it) } }
        if (found == null && error == null) item { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        if (found?.isEmpty() == true) item { Text(stringResource(R.string.sms_no_senders), color = ur.muted) }
        if (banks.isNotEmpty()) item(key = "banks") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.sms_banks), style = MaterialTheme.typography.titleSmall, color = ur.text)
                ListCard(Modifier.testTag("sms-senders")) {
                    banks.forEachIndexed { index, sender ->
                        SenderRow(sender, sender.address in chosen, sender.address in chosen || chosen.size < 50, index < banks.lastIndex) { toggle(sender, it) }
                    }
                }
            }
        }
        if (people.isNotEmpty()) item(key = "people") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (search.isBlank()) TextButton({ numbers = !numbers }) {
                    Text(if (numbers) stringResource(R.string.sms_hide_numbers) else countText(R.plurals.sms_show_numbers, people.size), color = ur.accent)
                }
                if (numbers || search.isNotBlank()) ListCard {
                    people.forEachIndexed { index, sender ->
                        SenderRow(sender, sender.address in chosen, sender.address in chosen || chosen.size < 50, index < people.lastIndex) { toggle(sender, it) }
                    }
                }
            }
        }
    }
}

/** A sender in the picker: its name and how many texts it sent, and a tick at the end like the app's other toggles. */
@Composable
private fun SenderRow(sender: SmsSender, checked: Boolean, enabled: Boolean, divider: Boolean, toggle: (Boolean) -> Unit) {
    // A short code's latest text, kept in line with the names above it in either script.
    val align = if (LocalLayoutDirection.current == LayoutDirection.Ltr) TextAlign.Left else TextAlign.Right
    Column(Modifier.fillMaxWidth().toggleable(checked, enabled = enabled, role = Role.Checkbox, onValueChange = toggle).testTag("sms-sender:${sender.address}")) {
        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 16.dp, end = 6.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(Money.own(sender.address), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (sender.count > 0) Text(countText(R.plurals.sms_message_count, sender.count), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                sender.preview?.let { Text(it, style = SmsBodyStyle.merge(MaterialTheme.typography.bodySmall), color = ur.text2, textAlign = align, maxLines = 2, overflow = TextOverflow.Ellipsis) }
            }
            Checkbox(checked, null, enabled = enabled, colors = CheckboxDefaults.colors(checkedColor = ur.accent))
        }
        if (divider) Hairline(start = 16.dp)
    }
}

/** The cards remembered for their senders: which account each is, and a way to forget one. */
@Composable
private fun SmsCardsSheet(budget: JSONObject, mappings: JSONArray, close: () -> Unit, save: (JSONArray) -> Unit) {
    val cards = (0 until mappings.length()).map { mappings.getJSONObject(it) }
    LazyFormDialog(stringResource(R.string.sms_cards), close) {
        item { Text(stringResource(R.string.sms_cards_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
        if (cards.isEmpty()) item { Text(stringResource(R.string.sms_cards_none), color = ur.muted) }
        itemsIndexed(cards, key = { i, card -> "$i:" + card.text("sender") + card.text("reference") }) { index, card ->
            Column(Modifier.cardSegment(0, 1, cardFill())) {
                ListRow(smsRefLabel(card.text("reference")),
                    subtitle = Money.own(card.text("sender")) + " · " + (budget.row("accounts", card.text("accountId"))?.text("name")?.let(Money::own) ?: stringResource(R.string.sms_account_gone)),
                    chevron = false, trailing = {
                        IconButton({ save(JSONArray(cards.filterIndexed { i, _ -> i != index })) }) { Icon(Icons.Outlined.Close, stringResource(R.string.sms_forget), tint = ur.muted) }
                    })
            }
        }
    }
}
