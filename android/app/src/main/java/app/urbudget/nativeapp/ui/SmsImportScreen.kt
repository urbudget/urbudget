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
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject

private data class SmsConsent(val messages: List<BankSms>, val previous: JSONObject?, val instruction: String, val connection: JSONObject)

@Composable
internal fun SmsImportScreen(budget: JSONObject, vm: BudgetViewModel, analyzer: SmsAnalyzer = NativeSmsAnalyzer) {
    val identity = vm.state.value.budgetIdentity ?: return
    key(identity) { SmsInboxContent(budget, vm, identity, analyzer) }
}

@Composable
private fun SmsInboxContent(budget: JSONObject, vm: BudgetViewModel, identity: String, analyzer: SmsAnalyzer) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current
    val inbox = remember { SmsInbox.get(context) }
    var allowed by remember { mutableStateOf(SmsReader.canRead(context)) }
    var receiving by remember { mutableStateOf(SmsReader.canReceive(context)) }
    var sources by remember { mutableStateOf<List<SmsSource>>(emptyList()) }
    var messages by remember { mutableStateOf<List<BankSms>>(emptyList()) }
    var selected by remember { mutableStateOf<Set<String>>(emptySet()) }
    var batch by remember { mutableStateOf<SmsBatch?>(null) }
    var showReview by remember { mutableStateOf(true) }
    var pendingCount by remember { mutableIntStateOf(0) }
    var loading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var pickSenders by remember { mutableStateOf(false) }
    var clear by remember { mutableStateOf(false) }
    var reload by remember { mutableIntStateOf(0) }
    var consent by remember { mutableStateOf<SmsConsent?>(null) }
    var analyzing by remember { mutableStateOf<Job?>(null) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        allowed = SmsReader.canRead(context); receiving = SmsReader.canReceive(context)
        if (allowed) pickSenders = true else error = context.getString(R.string.sms_permission_needed)
        reload++
    }
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
    LaunchedEffect(reload, allowed) {
        loading = true
        if (!allowed) { analyzing?.cancel(); consent = null; batch = null; messages = emptyList() }
        try {
            val loaded = withContext(smsStorageLane) {
                if (allowed) SmsReader.refresh(context, identity)
                Triple(inbox.sources(identity), if (allowed) inbox.pending(identity) else emptyList(), inbox.pendingCount(identity))
            }
            sources = loaded.first; messages = loaded.second; pendingCount = loaded.third
            selected = selected.intersect(messages.map { it.id }.toSet())
            if (allowed && batch == null) batch = withContext(smsStorageLane) { SmsBatch.restore(inbox.state(identity, "draft"), messages) }
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        finally { loading = false }
    }
    fun saveBatch(value: SmsBatch?) {
        batch = value
        scope.launch { try { withContext(smsStorageLane) { inbox.saveState(identity, "draft", value?.saved()) } } catch (failure: Exception) { error = failure.message } }
    }
    fun finish(ids: List<String>) {
        scope.launch {
            try { withContext(smsStorageLane) { inbox.finish(identity, ids) }; batch = null; selected = emptySet(); reload++ }
            catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    }
    fun requestAnalysis(chosen: List<BankSms>, previous: JSONObject? = null, instruction: String = "") {
        scope.launch {
            try {
                val connection = analyzer.status()
                check(connection.optBoolean("ready")) { context.getString(R.string.sms_connect_assistant) }
                consent = SmsConsent(chosen.toList(), previous, instruction, connection); showReview = false
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message; showReview = false }
        }
    }
    LazyColumn(Modifier.fillMaxSize().testTag("sms-inbox"), contentPadding = PaddingValues(start = 18.dp, end = 18.dp, top = 16.dp, bottom = 110.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item { Text(stringResource(R.string.sms_intro), color = ur.text2) }
        if (!allowed) item {
            Notice(stringResource(R.string.sms_disclosure), tone = "info")
            Button({ permission.launch(arrayOf(Manifest.permission.READ_SMS, Manifest.permission.RECEIVE_SMS)) }, Modifier.fillMaxWidth()) { Text(stringResource(R.string.sms_allow)) }
            TextButton({ context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}"))) }) { Text(stringResource(R.string.sms_open_settings)) }
        }
        error?.let { item { FormError(it) } }
        if (allowed) {
            item { ListCard { ListRow(stringResource(R.string.sms_senders), subtitle = if (sources.isEmpty()) stringResource(R.string.sms_choose_senders) else sources.joinToString(" · ") { it.sender }, onClick = { if (analyzing == null) pickSenders = true }) } }
            if (!receiving) item { Notice(stringResource(R.string.sms_receive_needed), action = stringResource(R.string.sms_allow), onAction = { permission.launch(arrayOf(Manifest.permission.READ_SMS, Manifest.permission.RECEIVE_SMS)) }) }
            item { Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                Text(stringResource(R.string.sms_waiting, pendingCount.toString()), color = ur.text2)
                TextButton({ reload++ }, enabled = !loading) { Text(stringResource(R.string.sms_refresh)) }
            } }
            if (loading) item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
            if (analyzing != null) item {
                Text(stringResource(R.string.sms_analyzing)); LinearProgressIndicator(Modifier.fillMaxWidth())
                TextButton({ analyzing?.cancel() }) { Text(stringResource(R.string.common_cancel)) }
            }
            if (batch != null) item {
                Button({ showReview = true }, Modifier.fillMaxWidth().testTag("sms-review"), enabled = analyzing == null) { Text(stringResource(R.string.sms_review_title)) }
                TextButton({ saveBatch(null) }, enabled = analyzing == null) { Text(stringResource(R.string.sms_discard_draft)) }
            }
            if (!loading && sources.isNotEmpty() && messages.isEmpty()) item { Text(stringResource(R.string.sms_empty), color = ur.muted) }
            if (pendingCount > messages.size) item { Notice(stringResource(R.string.sms_batch_limit), tone = "info") }
            if (messages.isNotEmpty() && batch == null) {
                item { Text(stringResource(R.string.sms_select_note), color = ur.muted) }
                item {
                    TextButton({ selected = messages.take(50).map { it.id }.toSet() }, enabled = analyzing == null) { Text(stringResource(R.string.sms_select_batch)) }
                    Button({ requestAnalysis(messages.filter { it.id in selected }) }, Modifier.fillMaxWidth().testTag("sms-analyze"), enabled = selected.isNotEmpty() && selected.size <= 50 && !loading && analyzing == null) {
                        Text(stringResource(R.string.sms_analyze_count, selected.size.toString()))
                    }
                }
            }
            items(messages, key = { it.id }) { message ->
                Card(Modifier.testTag("sms-message:${message.id}"), colors = CardDefaults.cardColors(containerColor = ur.surface)) {
                    Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(message.id in selected, { checked -> selected = if (checked) selected + message.id else selected - message.id }, modifier = Modifier.testTag("sms-select:${message.id}"), enabled = batch == null && analyzing == null && (message.id in selected || selected.size < 50))
                            Text(Money.own(message.sender), style = MaterialTheme.typography.titleSmall)
                        }
                        Text(Money.own(message.body), style = MaterialTheme.typography.bodySmall, color = ur.text2)
                        TextButton({ finish(listOf(message.id)) }, enabled = batch == null && analyzing == null) { Text(stringResource(R.string.sms_dismiss)) }
                    }
                }
            }
        }
        if (sources.isNotEmpty() || pendingCount > 0) item { TextButton({ clear = true }, enabled = analyzing == null) { Text(stringResource(R.string.sms_stop)) } }
    }
    if (pickSenders && allowed) SmsSenderPicker(sources, { pickSenders = false }, save = { chosen, since ->
        scope.launch {
            try { withContext(smsStorageLane) { inbox.configure(identity, chosen, since) }; batch = null; pickSenders = false; reload++ }
            catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        }
    })
    if (clear) AlertDialog(onDismissRequest = { clear = false }, title = { Text(stringResource(R.string.sms_stop)) }, text = { Text(stringResource(R.string.sms_stop_detail)) },
        confirmButton = { TextButton({ scope.launch { try { withContext(smsStorageLane) { inbox.clear(identity) }; clear = false; batch = null; reload++ } catch (failure: Exception) { error = failure.message } } }) { Text(stringResource(R.string.sms_stop)) } },
        dismissButton = { TextButton({ clear = false }) { Text(stringResource(R.string.common_cancel)) } })
    consent?.let { send ->
        AlertDialog(onDismissRequest = { consent = null; showReview = true }, title = { Text(stringResource(R.string.sms_send_title)) },
            text = { Text(stringResource(R.string.sms_send_notice, send.messages.size.toString(), send.connection.text("provider"), send.connection.text("model").ifBlank { stringResource(R.string.sms_default_model) })) },
            confirmButton = { TextButton({
                consent = null; error = null; showReview = false
                analyzing = scope.launch {
                    try {
                        check(SmsReader.canRead(context) && vm.state.value.budgetIdentity == identity) { context.getString(R.string.sms_permission_needed) }
                        val mappings = withContext(smsStorageLane) { JSONArray(inbox.state(identity, "mappings") ?: "[]") }
                        val activeAccounts = budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") }.toSet()
                        val relevant = (0 until mappings.length()).map { mappings.getJSONObject(it) }.filter { m -> m.text("accountId") in activeAccounts && send.messages.any { smsSenderKey(it.sender) == smsSenderKey(m.text("sender")) && m.text("reference") in it.body } }
                        val request = JSONObject().put("budgetId", identity).put("provider", send.connection.getString("provider")).put("model", send.connection.opt("model") ?: JSONObject.NULL)
                            .put("messages", JSONArray(send.messages.map { it.json() })).put("mappings", JSONArray(relevant)).put("instruction", send.instruction).put("previous", send.previous ?: JSONObject.NULL)
                        val result = analyzer.analyze(request)
                        check(vm.state.value.budgetIdentity == identity && SmsReader.canRead(context)) { context.getString(R.string.sms_permission_needed) }
                        val completed = SmsBatch(send.messages, result)
                        withContext(smsStorageLane) { inbox.saveState(identity, "draft", completed.saved()) }
                        batch = completed; showReview = true; selected = emptySet()
                    } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
                    finally { analyzing = null }
                }
            }, Modifier.testTag("sms-consent")) { Text(stringResource(R.string.sms_send_confirm)) } },
            dismissButton = { TextButton({ consent = null; showReview = true }) { Text(stringResource(R.string.common_cancel)) } })
    }
    batch?.takeIf { showReview && allowed && analyzing == null }?.let { current ->
        SmsAnalysisReview(budget, vm, current, { showReview = false }, ::saveBatch, { instruction -> requestAnalysis(current.messages, current.analysis, instruction) }, ::finish)
    }
}

@Composable
internal fun SmsSenderPicker(sources: List<SmsSource>, close: () -> Unit, save: (Set<String>, Long) -> Unit,
                             load: (android.content.Context) -> List<String> = SmsReader::senders) {
    val context = LocalContext.current
    var addresses by remember { mutableStateOf<List<String>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var chosen by remember { mutableStateOf(sources.map { it.sender }.toSet()) }
    var search by remember { mutableStateOf("") }
    var range by remember { mutableStateOf("30") }
    LaunchedEffect(Unit) {
        try { addresses = withContext(smsStorageLane) { (sources.map { it.sender } + load(context)).distinctBy(::smsSenderKey).sortedBy { it.lowercase() } } }
        catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
    }
    LazyFormDialog(stringResource(R.string.sms_senders), close, confirm = stringResource(R.string.common_save), enabled = addresses != null && chosen.size <= 50, onConfirm = {
        save(chosen, System.currentTimeMillis() - (range.toLong() * 86_400_000L))
    }) {
        item { Text(stringResource(R.string.sms_sender_help), color = ur.text2) }
        item { Field(search, { search = it }, stringResource(R.string.common_search)) }
        item { Choice(stringResource(R.string.sms_history), range, listOf("0" to stringResource(R.string.sms_new_only), "30" to stringResource(R.string.sms_last_30), "90" to stringResource(R.string.sms_last_90))) { range = it } }
        error?.let { item { FormError(it) } }
        if (addresses == null && error == null) item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
        if (addresses?.isEmpty() == true) item { Text(stringResource(R.string.sms_no_senders), color = ur.muted) }
        items(addresses.orEmpty().filter { it.contains(search, ignoreCase = true) }, key = { it }) { address ->
            Row(Modifier.fillMaxWidth().toggleable(address in chosen, role = Role.Checkbox, enabled = address in chosen || chosen.size < 50) { on -> chosen = if (on) chosen + address else chosen - address }.padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Checkbox(address in chosen, onCheckedChange = null); Text(Money.own(address), Modifier.padding(start = 10.dp))
            }
        }
    }
}
