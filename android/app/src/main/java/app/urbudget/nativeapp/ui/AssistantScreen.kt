package app.urbudget.nativeapp.ui

import app.urbudget.nativeapp.BankTextsAssistant
import app.urbudget.nativeapp.PageRequests
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import app.urbudget.nativeapp.AssistantController
import app.urbudget.nativeapp.AssistantErrorAction
import app.urbudget.nativeapp.AssistantPlace
import app.urbudget.nativeapp.SignInService
import app.urbudget.nativeapp.assistantProviderCompany
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import app.urbudget.nativeapp.AssistantState
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.ProposalChoices
import app.urbudget.nativeapp.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import org.json.JSONObject
import java.io.File

/**
 * Native chat and proposal review for the same Rust assistant used by desktop. `importing` is the
 * bank file being imported when the assistant is open over its import ({account, content,
 * session}); fixes it proposes for that import go to `applyFix`.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun AssistantScreen(vm: BudgetViewModel, controller: AssistantController = viewModel(), onClose: (() -> Unit)? = null,
                             importing: JSONObject? = null, applyFix: ((thread: String, proposal: JSONObject) -> Unit)? = null,
                             place: AssistantPlace = AssistantPlace(), dragArea: Modifier = Modifier) {
    // Token updates only recompose the live reply, not history, proposal controls or composer.
    val panel = remember(controller) { controller.state.map { it.copy(streaming = "", thinking = "") }.distinctUntilChanged() }
    val state by panel.collectAsStateWithLifecycle(AssistantState())
    val budgetState by vm.state.collectAsStateWithLifecycle()
    val budget = budgetState.budget ?: return
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var message by rememberSaveable { mutableStateOf("") }
    var settings by rememberSaveable { mutableStateOf(false) }
    var history by rememberSaveable { mutableStateOf(false) }
    var contextPicker by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf<List<String>>(emptyList()) }
    var deleteId by remember { mutableStateOf<String?>(null) }
    var importFile by remember { mutableStateOf<JSONObject?>(null) }
    var report by remember { mutableStateOf<JSONObject?>(null) }
    var reportReply by remember { mutableStateOf<String?>(null) }
    // Whether chat may search this budget's bank texts here: the phone then answers the assistant's
    // bank-text tools. The permission to send texts is for one service and model, so it's asked
    // again when either changes, and as each message goes: senders, SMS access or the permission
    // itself may have changed in Bank messages while this panel stood beside it.
    var bankTexts by remember { mutableStateOf(false) }
    suspend fun checkBankTexts(): Boolean {
        val identity = budgetState.budgetIdentity
        val connection = controller.state.value.status
        return withContext(Dispatchers.IO) { BankTextsAssistant.available(context, identity, connection) }.also { bankTexts = it }
    }
    LaunchedEffect(budgetState.budgetIdentity, state.status?.text("provider"), state.status?.text("model")) { checkBankTexts() }
    // What waits on the notice of what's sent: a message, or a reply asked for again.
    var afterNotice by remember { mutableStateOf<(() -> Unit)?>(null) }
    var pendingExport by remember { mutableStateOf<Pair<String, ByteArray>?>(null) }
    var exportError by remember { mutableStateOf<String?>(null) }
    // A proposal card's row being given another category or amount: the proposal, and the row.
    var changing by remember { mutableStateOf<Pair<JSONObject, JSONObject>?>(null) }
    val list = rememberLazyListState()
    val entries = state.entries
    val attach = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) controller.attach(context, uri)
    }
    val export = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        val data = pendingExport
        if (uri != null && data != null) scope.launch {
            runCatching { withContext(Dispatchers.IO) {
                replacing { mode -> context.contentResolver.openOutputStream(uri, mode) }?.use { it.write(data.second) } ?: error(context.getString(R.string.assistant_export_failed))
            } }.onFailure { exportError = it.message }
            pendingExport = null
        }
    }
    fun saveBytes(name: String, bytes: ByteArray) { pendingExport = name to bytes; export.launch(name) }
    fun savePath(path: String) { scope.launch {
        runCatching { withContext(Dispatchers.IO) { File(path).let { it.name to it.readBytes() } } }
            .onSuccess { saveBytes(it.first, it.second) }.onFailure { exportError = it.message }
    } }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { controller.refresh() }
    // A message put ready to send, such as one asking to fix an import's lines.
    LaunchedEffect(state.draft) { state.draft?.let { message = it; controller.prefilled() } }
    LaunchedEffect(entries.size, state.running) {
        if (list.layoutInfo.totalItemsCount > 0 && (list.firstVisibleItemIndex >= list.layoutInfo.totalItemsCount - 5 || entries.size < 4))
            list.animateScrollToItem(list.layoutInfo.totalItemsCount - 1)
    }

    // Steps in a row read as one line, the latest; the rest are a tap away.
    val rows = remember(entries) { assistantRows(entries) }
    val clipboard = LocalClipboardManager.current
    /**
     * Sends at once, or once the user has read the notice of what's sent when it's due for the
     * service in use (the first message to it, or a reply asked for again after another was chosen):
     * nothing goes to a service before that.
     */
    fun whenTold(send: () -> Unit) {
        val status = controller.state.value.status
        if (status?.optBoolean("ready") == true && status.optBoolean("noticeDue")) afterNotice = send else send()
    }
    fun ask(text: String) = whenTold {
        scope.launch {
            val texts = checkBankTexts()
            controller.send(text, budgetState.month, selected, importing, texts, place) { message = ""; selected = emptyList() }
        }
    }
    fun retry() = whenTold { scope.launch { controller.retry(budgetState.month, importing, checkBankTexts(), place) } }
    val signingIn = "signIn" in state.busy || state.status?.optBoolean("signingIn") == true
    Column(Modifier.fillMaxSize().imePadding()) {
        Row(Modifier.fillMaxWidth().then(dragArea).padding(start = 20.dp, end = 4.dp, top = 2.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.assistant_title), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, color = ur.text)
                Text(state.thread?.text("title").orEmpty().ifEmpty { stringResource(R.string.assistant_tagline) }, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
            IconButton({ history = true }) { Icon(Icons.Outlined.History, stringResource(R.string.assistant_history), tint = ur.text) }
            IconButton({ controller.newChat(); message = ""; selected = emptyList() }) { Icon(Icons.Outlined.AddComment, stringResource(R.string.assistant_new), tint = ur.text) }
            IconButton({ settings = true }) { Icon(Icons.Outlined.Settings, stringResource(R.string.assistant_settings), tint = ur.text) }
            if (onClose != null) IconButton(onClose) { Icon(Icons.Outlined.Close, stringResource(R.string.assistant_close), tint = ur.text) }
        }
        if (state.busy.isNotEmpty()) LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
        state.notice?.let { notice ->
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(notice, Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
                IconButton(controller::dismissNotice) { Icon(Icons.Outlined.Close, stringResource(R.string.assistant_dismiss_notice)) }
            }
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            if (entries.isEmpty()) item {
                Text(stringResource(R.string.assistant_empty_title), style = MaterialTheme.typography.headlineSmall, color = ur.text)
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.assistant_empty_detail), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                if (state.status?.optBoolean("ready") != true) FilledTonalButton({ settings = true }, Modifier.padding(top = 12.dp)) { Text(stringResource(R.string.assistant_set_up)) }
                else Column(Modifier.fillMaxWidth().padding(top = 20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    // Where this phone reads bank texts, those come first: what's waiting, and whether the accounts agree with the bank.
                    val prompts = (if (bankTexts) listOf(R.string.assistant_prompt_bank_waiting, R.string.assistant_prompt_bank_balance) else emptyList()) +
                        listOf(R.string.assistant_prompt_where, R.string.assistant_prompt_cover, R.string.assistant_prompt_report)
                    prompts.take(4).map { stringResource(it) }.forEach { prompt ->
                        // A prompt is a whole question: tapping it asks.
                        Surface({ ask(prompt) }, Modifier.fillMaxWidth().testTag("assistant-prompt"), enabled = "send" !in state.busy, shape = RoundedCornerShape(16.dp), color = ur.surface2) {
                            Row(Modifier.padding(horizontal = 16.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                Text(prompt, Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, color = ur.text)
                                Icon(Icons.AutoMirrored.Outlined.Send, null, Modifier.size(18.dp), tint = ur.accent)
                            }
                        }
                    }
                }
            }
            itemsIndexed(rows, key = { _, row -> "${state.threadId}:${row.first}" }, contentType = { _, row -> row.second.first().text("kind") }) { _, row ->
                val entry = row.second.first()
                when (entry.text("kind")) {
                    "proposal" -> entry.optJSONObject("proposal")?.let { proposal ->
                        val refinePrompt = stringResource(R.string.assistant_refine_prompt, proposal.text("title"))
                        AssistantProposalCard(proposal, budget.text("currency"), state.busy.contains("proposal"), state.undoProposal == proposal.text("id"),
                            apply = { controller.apply(proposal.text("id"), vm::refresh) },
                            applyFix = applyFix?.let { fix -> { fix(state.threadId, proposal) } },
                            undo = { controller.undoProposal(proposal.text("id"), vm::refresh) },
                            discard = { controller.discard(proposal.text("id")) },
                            refine = { message = "$refinePrompt " },
                            reviewImport = { val thread = state.threadId; controller.importFile(proposal.text("id")) {
                                importFile = it.put("assistantThreadId", thread).put("assistantProposalId", proposal.text("id"))
                            } },
                            saveImport = { controller.saveImport(proposal.text("id"), ::savePath) },
                            report = { controller.report(proposal.text("id")) { report = it } },
                            spreadsheet = { open -> controller.spreadsheet(proposal.text("id"), open) { if (!open) savePath(it) } },
                            revise = { controller.revise(proposal.text("id"), it) },
                            change = { row -> changing = proposal to row },
                            // A bank-text card is done on the phone: Bank messages opens when it reads.
                            applyBankTexts = proposal.optJSONObject("bankTexts")?.let { payload -> {
                                val thread = state.threadId
                                val id = proposal.text("id")
                                scope.launch {
                                    runCatching { BankTextsAssistant.apply(context.applicationContext, vm, payload) }
                                        .onSuccess { done ->
                                            controller.imported(thread, id)
                                            // Texts that have gone from the phone since: the panel stays open to say so.
                                            done.notice?.let(controller::note)
                                            done.page?.let { PageRequests.page.value = it; if (done.notice == null) onClose?.invoke() }
                                        }
                                        .onFailure { controller.showError(it.message) }
                                }
                            } })
                    }
                    "step" -> AssistantSteps(row.second)
                    "stopped" -> Text(stringResource(R.string.assistant_reply_stopped), style = MaterialTheme.typography.labelMedium, color = ur.muted)
                    // The user's words on the right, as a phone's messages are.
                    "user" -> Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Surface(color = ur.accentSoft, shape = RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp, bottomStart = 18.dp, bottomEnd = 6.dp), modifier = Modifier.widthIn(max = 320.dp)) {
                            SelectionContainer { Text(entry.text("text"), Modifier.padding(horizontal = 14.dp, vertical = 10.dp), style = MaterialTheme.typography.bodyLarge, color = ur.text) }
                        }
                        entry.rows("attachments").forEach { Text("\uD83D\uDCCE ${Money.own(it.text("name"))}", style = MaterialTheme.typography.labelMedium, color = ur.muted) }
                    }
                    "error" -> Surface(color = ur.warningSoft, shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text(stringResource(R.string.assistant_couldnt_finish), style = MaterialTheme.typography.labelLarge, color = ur.warning)
                            SelectionContainer { Text(entry.text("text"), color = ur.text) }
                            // What helps is the core's to say, and only for the latest error: an earlier one, or one asking again wouldn't help, has no button.
                            AssistantErrorAction.of(entry)?.let { action ->
                                TextButton({
                                    when (action) {
                                        AssistantErrorAction.Retry -> retry()
                                        AssistantErrorAction.Settings -> settings = true
                                        AssistantErrorAction.Usage -> controller.openLink("usage")
                                        // The sign-in the settings start, with its service kept answering the browser; signed in, the reply is asked for again.
                                        AssistantErrorAction.SignIn -> { SignInService.start(context); controller.signIn { retry() } }
                                    }
                                }, enabled = when (action) {
                                    AssistantErrorAction.Retry -> !state.running
                                    AssistantErrorAction.SignIn -> !state.running && !signingIn
                                    else -> true
                                }) { Text(stringResource(action.label)) }
                            }
                        }
                    }
                    // The assistant's reply as text, with copying and reporting it to hand.
                    else -> Column(Modifier.fillMaxWidth()) {
                        SelectionContainer { AssistantMarkdown(entry.text("text")) }
                        Row(Modifier.offset(x = (-12).dp)) {
                            IconButton({ clipboard.setText(AnnotatedString(entry.text("text"))) }) {
                                Icon(Icons.Outlined.ContentCopy, stringResource(R.string.assistant_copy_reply), Modifier.size(18.dp), tint = ur.muted)
                            }
                            IconButton({ reportReply = entry.text("text").take(4000) }) {
                                Icon(Icons.Outlined.OutlinedFlag, stringResource(R.string.assistant_report_reply), Modifier.size(18.dp), tint = ur.muted)
                            }
                        }
                    }
                }
            }
            item(key = "live-reply") { AssistantLiveReply(controller, list) }
            // What it's doing, while a lookup runs: the phone's search can take a few seconds.
            if (state.running) item {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp, color = ur.accent)
                    Text(state.activity?.let { "$it\u2026" } ?: stringResource(R.string.assistant_working), Modifier.testTag("assistant-activity"),
                        style = MaterialTheme.typography.labelLarge, color = ur.text2)
                }
            }
            else if (entries.lastOrNull()?.text("kind") == "user") item {
                TextButton({ retry() }, enabled = "send" !in state.busy) { Text(stringResource(R.string.assistant_continue_reply)) }
            }
        }
        if (state.attachments.isNotEmpty() || selected.isNotEmpty()) Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Column(Modifier.weight(1f)) {
                state.attachments.forEach { file -> InputChip(true, { controller.removeAttachment(file.text("id")) }, label = { Text(Money.own(file.text("name")), maxLines = 1) }, trailingIcon = { Icon(Icons.Outlined.Close, stringResource(R.string.assistant_remove_attachment)) }) }
                if (selected.isNotEmpty()) InputChip(true, { contextPicker = true }, label = { Text(countText(R.plurals.assistant_selected_transactions, selected.size)) })
            }
        }
        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp), verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            // A file or transactions to go with the message, from one button.
            Box {
                var adding by remember { mutableStateOf(false) }
                IconButton({ adding = true }, enabled = !state.running) { Icon(Icons.Outlined.AddCircleOutline, stringResource(R.string.assistant_add_to_message), tint = ur.text) }
                DropdownMenu(adding, { adding = false }, containerColor = ur.surface) {
                    DropdownMenuItem(text = { Text(stringResource(R.string.assistant_attach)) }, onClick = { adding = false; attach.launch(arrayOf("*/*")) },
                        leadingIcon = { Icon(Icons.Outlined.AttachFile, null) }, enabled = state.attachments.size < 5 && "attach" !in state.busy)
                    DropdownMenuItem(text = { Text(stringResource(R.string.assistant_select_transactions)) }, onClick = { adding = false; contextPicker = true },
                        leadingIcon = { Icon(Icons.Outlined.Checklist, null) })
                }
            }
            FilledField(message, { message = it.take(20_000) }, Modifier.weight(1f), label = { Text(stringResource(R.string.assistant_ask)) }, maxLines = 6)
            if (state.running) IconButton(controller::stop) { Icon(Icons.Outlined.StopCircle, stringResource(R.string.assistant_stop), tint = ur.text) }
            else FilledIconButton({ ask(message) }, enabled = state.status?.optBoolean("ready") == true && "send" !in state.busy && (message.isNotBlank() || state.attachments.isNotEmpty()),
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = ur.accent, contentColor = if (ur.dark) ur.bg else Color.White)) {
                Icon(Icons.AutoMirrored.Outlined.Send, stringResource(R.string.assistant_send))
            }
        }
    }

    if (settings) AssistantSettingsDialog(controller) { settings = false }
    // The notice of what's sent, and to whom, before the first message to a service. Continue keeps
    // that it was read and sends what waited; Not now sends nothing and leaves the message as typed.
    afterNotice?.let { send ->
        AlertDialog(onDismissRequest = { afterNotice = null }, title = { Text(stringResource(R.string.assistant_notice_title)) },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(stringResource(R.string.assistant_notice_sent, assistantProviderCompany(state.status?.text("provider").orEmpty())))
                    Text(stringResource(R.string.assistant_notice_kept))
                    ShareMemosSwitch(state.status, controller)
                }
            },
            // Not while the memo switch is still being saved: the message goes with the choice made here.
            confirmButton = { TextButton({ afterNotice = null; controller.told(send) }, enabled = "settings" !in state.busy) { Text(stringResource(R.string.common_continue)) } },
            dismissButton = { TextButton({ afterNotice = null }) { Text(stringResource(R.string.sms_not_now)) } })
    }
    if (history) ModalBottomSheet(onDismissRequest = { history = false }) {
        Text(stringResource(R.string.assistant_conversations), Modifier.padding(horizontal = 20.dp), style = MaterialTheme.typography.titleLarge)
        if (state.thread != null) TextButton({
            val thread = state.thread!!
            val currency = budget.text("currency")
            scope.launch {
                runCatching { withContext(Dispatchers.Default) {
                    "${thread.text("title").take(60).replace(Regex("[^\\p{L}\\p{N} _-]"), "_")}.md" to conversationMarkdown(thread, currency).toByteArray()
                } }.onSuccess { saveBytes(it.first, it.second) }.onFailure { exportError = it.message }
            }
        }, Modifier.padding(horizontal = 12.dp)) { Text(stringResource(R.string.assistant_export_conversation)) }
        LazyColumn(Modifier.heightIn(max = 500.dp)) {
            if (state.threads.isEmpty()) item { Text(stringResource(R.string.assistant_no_conversations), Modifier.padding(20.dp)) }
            items(state.threads, key = { it.text("id") }) { thread ->
                ListItem(headlineContent = { TextButton({ controller.openThread(thread.text("id")); history = false }) { Text(thread.text("title")) } },
                    supportingContent = { Text(Money.ltr(thread.text("updated").take(10))) }, trailingContent = { IconButton({ deleteId = thread.text("id") }) { Icon(Icons.Outlined.DeleteOutline, stringResource(R.string.assistant_delete_conversation)) } })
            }
        }
    }
    deleteId?.let { id -> AlertDialog(onDismissRequest = { deleteId = null }, title = { Text(stringResource(R.string.assistant_delete_conversation_title)) }, text = { Text(stringResource(R.string.assistant_delete_conversation_message)) },
        confirmButton = { TextButton({ controller.deleteThread(id); deleteId = null }) { Text(stringResource(R.string.common_delete)) } }, dismissButton = { TextButton({ deleteId = null }) { Text(stringResource(R.string.common_cancel)) } }) }
    importFile?.let { file ->
        var draft by remember(file) { mutableStateOf(preparedBankImport(file, file.text("assistantThreadId"), file.text("assistantProposalId"))) }
        BankImportDialog(budget, vm, draft, { change -> draft = change(draft) },
            onImported = { controller.imported(file.text("assistantThreadId"), file.text("assistantProposalId")) }) { importFile = null }
    }
    report?.let { document -> AssistantReportDialog(document, { report = null }, {
        scope.launch { val bytes = withContext(Dispatchers.Default) { document.text("body").toByteArray() }; saveBytes("${document.text("title").take(60)}.md", bytes) }
    }) }
    reportReply?.let { excerpt -> AssistantReplyReportDialog(excerpt, controller, { reportReply = null }) }
    changing?.let { (proposal, row) ->
        ProposalRowEditor(row, budget, vm, budgetState.summary, budgetState.month, { change ->
            controller.revise(proposal.text("id"), ProposalChoices.change(proposal.optJSONObject("choices"), row.text("key"), change)); changing = null
        }) { changing = null }
    }
    if (contextPicker) Dialog(onDismissRequest = { contextPicker = false }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        val transactions by produceState<List<JSONObject>?>(null, budget) {
            value = null
            value = withContext(Dispatchers.Default) { budget.rows("transactions").sortedByDescending { it.text("date") } }
        }
        val selectedIds = remember(selected) { selected.toSet() }
        Surface(Modifier.fillMaxWidth().fillMaxHeight(.85f).padding(16.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(16.dp)) {
                Text(stringResource(R.string.assistant_transactions_to_discuss), style = MaterialTheme.typography.titleLarge)
                Text(stringResource(R.string.assistant_select_up_to), style = MaterialTheme.typography.bodySmall)
                if (transactions == null) LinearProgressIndicator(Modifier.fillMaxWidth())
                LazyColumn(Modifier.weight(1f)) {
                    items(transactions.orEmpty(), key = { it.text("id") }) { row ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(row.text("id") in selectedIds, { checked -> selected = if (checked) selected + row.text("id") else selected - row.text("id") }, enabled = row.text("id") in selectedIds || selected.size < 200)
                            Column { Text(Money.own(row.text("payee"))); Text("${Money.ltr(row.text("date"))} · ${money(row.optLong("amount"), budget.text("currency"))}", style = MaterialTheme.typography.bodySmall) }
                        }
                    }
                }
                Row { TextButton({ selected = emptyList() }) { Text(stringResource(R.string.assistant_clear)) }; Spacer(Modifier.weight(1f)); Button({ contextPicker = false }) { Text(stringResource(R.string.assistant_done_count, "${selected.size}")) } }
            }
        }
    }
    val error = exportError ?: state.error
    if (error != null) AlertDialog(onDismissRequest = { controller.dismissError(); exportError = null }, title = { Text(stringResource(R.string.assistant_error_title)) }, text = { Text(error) },
        confirmButton = { TextButton({ controller.dismissError(); exportError = null }) { Text(stringResource(R.string.common_ok)) } })
}

@Composable
private fun AssistantLiveReply(controller: AssistantController, list: LazyListState) {
    val tokens = remember(controller) { controller.state.map { it.streaming to it.thinking }.distinctUntilChanged() }
    val reply by tokens.collectAsStateWithLifecycle("" to "")
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (reply.second.isNotEmpty()) Text(reply.second, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        // Formatting is applied to the completed message; partial Markdown need not be reparsed per token.
        if (reply.first.isNotEmpty()) SelectionContainer { Text(reply.first, style = MaterialTheme.typography.bodyMedium) }
    }
    LaunchedEffect(reply) {
        val last = list.layoutInfo.totalItemsCount - 1
        if (last >= 0 && !list.isScrollInProgress && (list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: -1) >= last - 1)
            list.scrollToItem(last)
    }
}

/**
 * A change the assistant prepared, applied only from here. While it waits, each part of it can be
 * left out (its checkbox) or given another category or amount (its pencil, `change`); `revise`
 * makes the card again with those choices, and Apply makes what's left (desktop ProposalCard).
 */
@Composable
internal fun AssistantProposalCard(proposal: JSONObject, currency: String, busy: Boolean, canUndo: Boolean, apply: () -> Unit, undo: () -> Unit, discard: () -> Unit,
    refine: () -> Unit, reviewImport: () -> Unit, saveImport: () -> Unit, report: () -> Unit, spreadsheet: (Boolean) -> Unit, applyFix: (() -> Unit)? = null,
    revise: ((JSONObject) -> Unit)? = null, change: ((JSONObject) -> Unit)? = null, applyBankTexts: (() -> Unit)? = null) {
    val card = proposal.optJSONObject("card") ?: JSONObject()
    val rows = remember(card) { card.rows("rows") }
    var page by remember(proposal.text("id")) { mutableIntStateOf(0) }
    val pages = ((rows.size + 19) / 20).coerceAtLeast(1)
    val currentPage = page.coerceAtMost(pages - 1)
    val status = proposal.text("status")
    // Its parts, to leave out or change, while it waits.
    val choices = proposal.optJSONObject("choices")
    val choosing = revise != null && status == "proposed" && rows.any { it.text("key").isNotEmpty() }
    val chosen = rows.count { it.text("key").isNotEmpty() && !it.optBoolean("skipped") }
    val partial = ProposalChoices.partial(rows, choosing)
    val original: (() -> Unit)? = if (revise != null && choices != null) ({ revise(ProposalChoices.original()) }) else null
    if (status == "applied" || status == "discarded") {
        Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(18.dp), colors = CardDefaults.cardColors(containerColor = ur.surface2, contentColor = ur.text)) {
            Column(Modifier.padding(16.dp)) {
                Text(stringResource(if (status == "applied") R.string.assistant_applied_title else R.string.assistant_discarded_title, proposal.text("title")), style = MaterialTheme.typography.titleSmall)
                if (status == "applied" && canUndo) TextButton(undo, enabled = !busy) { Text(stringResource(R.string.assistant_undo_proposal)) }
            }
        }
        return
    }
    Card(Modifier.fillMaxWidth(), shape = RoundedCornerShape(18.dp), colors = CardDefaults.cardColors(containerColor = ur.surface2, contentColor = ur.text)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(card.text("kicker"), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
            Text(proposal.text("title"), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            card.rows("stats").forEach { stat -> Text("${stat.text("label")}: ${if (stat.has("amount") && !stat.isNull("amount")) money(stat.optLong("amount"), currency) else stat.text("text")}") }
            rows.subList(currentPage * 20, minOf(rows.size, (currentPage + 1) * 20)).forEach { row ->
                val key = row.text("key")
                val edit = row.optJSONObject("edit")
                val editable = !row.optBoolean("skipped") && (edit?.optBoolean("category") == true || edit?.optBoolean("amount") == true)
                ProposalRow(row, currency, busy,
                    toggle = if (choosing && key.isNotEmpty()) ({ revise?.invoke(ProposalChoices.toggle(choices, key, row.optBoolean("skipped"))) }) else null,
                    change = if (choosing && key.isNotEmpty() && editable && change != null) ({ change(row) }) else null)
            }
            card.optJSONObject("total")?.let { ProposalRow(it, currency, busy) }
            if (pages > 1) Row {
                TextButton({ page = currentPage - 1 }, enabled = currentPage > 0) { Text(stringResource(R.string.assistant_previous_rows)) }
                Text(stringResource(R.string.assistant_page, "${currentPage + 1}", "$pages"), Modifier.padding(12.dp))
                TextButton({ page = currentPage + 1 }, enabled = currentPage + 1 < pages) { Text(stringResource(R.string.assistant_next_rows)) }
            }
            if (card.optInt("more") > 0) Text(countText(R.plurals.assistant_more_rows, card.optInt("more")), style = MaterialTheme.typography.labelSmall)
            card.optJSONArray("check")?.let { Text(if (it.optBoolean(0)) "✓ " + it.optString(1) else stringResource(R.string.assistant_check, it.optString(1)), color = if (it.optBoolean(0)) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error) }
            listOf("warning", "note").forEach { if (card.text(it).isNotEmpty()) Text(card.text(it), style = MaterialTheme.typography.bodySmall) }
            if (proposal.text("problem").isNotEmpty() && proposal.text("kind") != "fix") Text(proposal.text("problem"), color = MaterialTheme.colorScheme.error)
            if (status == "failed") TextButton(discard, enabled = !busy) { Text(stringResource(R.string.assistant_dismiss)) }
            else when (proposal.text("kind")) {
                // Fixes for a bank file being imported: the import applies them, each only to a line that hasn't changed.
                "fix" -> if (status == "proposed" || status == "undone") {
                    Button(applyFix ?: {}, enabled = !busy && applyFix != null && !(choosing && chosen == 0)) {
                        Text(when {
                            applyFix == null -> stringResource(R.string.assistant_fix_apply_from_import)
                            partial != null -> stringResource(R.string.assistant_fix_apply_some, "${partial.chosen}", "${partial.total}")
                            else -> stringResource(R.string.assistant_fix_apply)
                        })
                    }
                    Text(stringResource(if (applyFix != null) R.string.assistant_fix_each_line else R.string.assistant_fix_open_import), style = MaterialTheme.typography.bodySmall)
                    Row { TextButton(refine, enabled = !busy) { Text(stringResource(R.string.assistant_ask_changes)) }; TextButton(discard, enabled = !busy) { Text(stringResource(R.string.assistant_discard)) } }
                    if (original != null) TextButton(original, enabled = !busy) { Text(stringResource(R.string.assistant_use_original)) }
                } else Text(stringResource(if (status == "applied") R.string.assistant_fix_applied else R.string.assistant_discarded), style = MaterialTheme.typography.labelMedium)
                "bankTexts" -> if (status == "proposed") {
                    Button(applyBankTexts ?: {}, enabled = !busy && applyBankTexts != null) {
                        Text(stringResource(when (proposal.optJSONObject("bankTexts")?.optString("action")) {
                            "read" -> R.string.bank_texts_apply_read; "add" -> R.string.bank_texts_apply_add
                            "remember" -> R.string.bank_texts_apply_remember; else -> R.string.bank_texts_apply_limit
                        }))
                    }
                    Row { TextButton(refine, enabled = !busy) { Text(stringResource(R.string.assistant_ask_changes)) }; TextButton(discard, enabled = !busy) { Text(stringResource(R.string.assistant_discard)) } }
                } else Text(stringResource(if (status == "applied") R.string.assistant_applied else R.string.assistant_discarded), style = MaterialTheme.typography.labelMedium)
                "report" -> Button(report, enabled = !busy) { Text(stringResource(R.string.assistant_read_report)) }
                "import" -> { Button(reviewImport, enabled = !busy) { Text(countText(R.plurals.assistant_review_import_rows, proposal.optInt("rows"))) }; TextButton(saveImport) { Text(stringResource(R.string.assistant_export_csv)) } }
                "spreadsheet" -> { Button({ spreadsheet(true) }, enabled = !busy) { Text(stringResource(R.string.assistant_open_spreadsheet)) }; TextButton({ spreadsheet(false) }) { Text(stringResource(R.string.assistant_export_excel)) } }
                else -> when (status) {
                    "proposed", "undone" -> {
                        Button(apply, enabled = !busy && !(choosing && chosen == 0)) {
                            Text(if (partial != null) stringResource(R.string.assistant_apply_some, "${partial.chosen}", "${partial.total}") else stringResource(R.string.assistant_apply_changes))
                        }
                        Row { TextButton(refine, enabled = !busy) { Text(stringResource(R.string.assistant_ask_changes)) }; TextButton(discard, enabled = !busy) { Text(stringResource(R.string.assistant_discard)) } }
                        if (original != null) TextButton(original, enabled = !busy) { Text(stringResource(R.string.assistant_use_original)) }
                    }
                    "applied" -> { Text(stringResource(R.string.assistant_applied), color = MaterialTheme.colorScheme.primary); if (canUndo) TextButton(undo, enabled = !busy) { Text(stringResource(R.string.assistant_undo_proposal)) } }
                    "discarded" -> Text(stringResource(R.string.assistant_discarded), style = MaterialTheme.typography.labelMedium)
                    "failed" -> Text(stringResource(R.string.assistant_could_not_apply), color = MaterialTheme.colorScheme.error)
                }
            }
            if (proposal.text("kind") == "report" && proposal.optInt("sheets") > 0) TextButton({ spreadsheet(false) }) { Text(stringResource(R.string.assistant_export_report_tables)) }
            if (status != "failed" && proposal.text("kind") in listOf("report", "spreadsheet", "import")) TextButton(discard, enabled = !busy) { Text(stringResource(R.string.assistant_discard)) }
        }
    }
}

/**
 * A card row. While the card waits, its checkbox (`toggle`) leaves it out or puts it back, and its
 * pencil (`change`) gives it another category or amount. A row left out stays, dimmed and struck
 * through; an amount the user gave shows in the accent colour.
 */
@Composable
private fun ProposalRow(row: JSONObject, currency: String, busy: Boolean, toggle: (() -> Unit)? = null, change: (() -> Unit)? = null) {
    val skipped = row.optBoolean("skipped")
    val label = row.text("label")
    val applyLabel = stringResource(R.string.assistant_apply_row, label)
    Row(Modifier.padding(start = if (row.optBoolean("indent")) 16.dp else 0.dp).alpha(if (skipped) .5f else 1f), verticalAlignment = Alignment.CenterVertically) {
        if (toggle != null) Checkbox(!skipped, { toggle() }, Modifier.semantics { contentDescription = applyLabel }, enabled = !busy)
        Column(Modifier.weight(1f)) {
            Row {
                Text(label, Modifier.weight(1f), textDecoration = if (skipped) TextDecoration.LineThrough else null)
                if (row.has("amount")) Text(money(row.optLong("amount"), currency), fontWeight = FontWeight.SemiBold,
                    color = if (row.optBoolean("edited")) MaterialTheme.colorScheme.primary else Color.Unspecified)
            }
            if (row.text("detail").isNotEmpty()) Text(row.text("detail"), style = MaterialTheme.typography.bodySmall)
            if (row.has("before") || row.has("after")) Text(stringResource(R.string.change_from_to, money(row.optLong("before"), currency), money(row.optLong("after"), currency)), style = MaterialTheme.typography.bodySmall)
            if (row.text("badge").isNotEmpty()) Text(row.text("badge"), style = MaterialTheme.typography.labelSmall, color = if (row.text("tone") == "warn") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
        }
        if (change != null) IconButton(change, enabled = !busy) { Icon(Icons.Outlined.Edit, stringResource(R.string.assistant_change_row, label)) }
    }
}

/**
 * A card row's category and/or amount, as its `edit` allows (desktop RowEditor). The category
 * starts at the row's (none is Unassigned, Ready to Assign), the amount at the row's signed amount;
 * one typed without a sign keeps the row's. Saving with nothing changed just closes.
 */
@Composable
internal fun ProposalRowEditor(row: JSONObject, budget: JSONObject, vm: BudgetViewModel?, summary: JSONObject?, month: String, save: (JSONObject) -> Unit, close: () -> Unit) {
    val edit = row.optJSONObject("edit") ?: JSONObject()
    val label = row.text("label")
    var category by remember { mutableStateOf(edit.text("categoryId").ifEmpty { Unassigned }) }
    var amount by remember { mutableStateOf(if (row.has("amount")) amountText(row.optLong("amount")) else "") }
    var problem by remember { mutableStateOf<String?>(null) }
    var picking by remember { mutableStateOf(false) }
    val names = remember(budget) { budget.rows("categories").associate { it.text("id") to it.text("name") } }
    if (vm != null) retainEditorDraft(vm, "assistantRow", row, JSONObject().put("row", row).put("category", category).put("amount", amount))
    val amountRequired = stringResource(R.string.assistant_amount_required)
    FormDialog(stringResource(R.string.assistant_change_row, label), close, onConfirm = {
        runCatching { ProposalChoices.edit(row, category.takeUnless { it == Unassigned }, amount) }
            .onSuccess { change -> if (change == null) close() else save(change) }
            .onFailure { problem = it.message ?: amountRequired }
    }) {
        if (row.text("detail").isNotEmpty()) Text(row.text("detail"), style = MaterialTheme.typography.bodyMedium, color = ur.muted)
        if (edit.optBoolean("category")) PickerField(stringResource(R.string.category_label),
            if (category == Unassigned) stringResource(R.string.category_unassigned) else names[category] ?: stringResource(R.string.category_deleted),
            stringResource(R.string.category_choose_a), { picking = true })
        if (edit.optBoolean("amount")) Field(amount, { amount = it; problem = null }, stringResource(R.string.amount_label), numeric = true)
        problem?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.negative) }
    }
    if (picking) CategoryPickerSheet(budget, summary, month, category, stringResource(R.string.assistant_category_for, label), includeUnassigned = true, includePayment = false,
        onPick = { category = it; picking = false }) { picking = false }
}

private fun conversationMarkdown(thread: JSONObject, currency: String): String = buildString {
    append("# ${thread.text("title")}\n\n")
    thread.rows("entries").forEach { entry ->
        if (entry.text("kind") == "proposal") {
            val proposal = entry.optJSONObject("proposal") ?: return@forEach
            append("## Proposal: ${proposal.text("title")} (${proposal.text("status")})\n\n")
            proposal.optJSONObject("card")?.let { card ->
                card.rows("stats").forEach { append("- ${it.text("label")}: ${if (it.has("amount")) money(it.optLong("amount"), currency) else it.text("text")}\n") }
                (card.rows("rows") + listOfNotNull(card.optJSONObject("total"))).forEach { row ->
                    append("- ${row.text("label")}: ${row.text("detail")}")
                    if (row.has("amount")) append(" ${money(row.optLong("amount"), currency)}")
                    if (row.has("before") && row.has("after")) append(" (${money(row.optLong("before"), currency)} → ${money(row.optLong("after"), currency)})")
                    if (row.text("badge").isNotBlank()) append(" · ${row.text("badge")}")
                    append("\n")
                }
                listOf("warning", "note").forEach { if (card.text(it).isNotBlank()) append("\n${card.text(it)}\n") }
            }
            append("\n")
        } else {
            append("## ${entry.text("kind").replaceFirstChar(Char::uppercase)}\n\n${entry.text("text")}\n\n")
            entry.rows("attachments").forEach { append("Attachment: ${it.text("name")} (${it.optLong("size")} bytes)\n") }
        }
    }
}

@Composable
private fun AssistantReportDialog(report: JSONObject, close: () -> Unit, export: () -> Unit) {
    val context = LocalContext.current
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize().padding(12.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(report.text("title"), style = MaterialTheme.typography.headlineSmall)
                Text("${Money.own(report.text("budget"))} · ${Money.own(report.text("period"))} · ${Money.ltr(report.text("created").take(10))}", style = MaterialTheme.typography.labelMedium)
                SelectionContainer(Modifier.weight(1f)) { AssistantMarkdownDocument(report.text("body"), Modifier.fillMaxSize()) }
                Row(Modifier.fillMaxWidth()) {
                    TextButton(export, Modifier.weight(1f)) { Text(stringResource(R.string.assistant_save_text)) }
                    TextButton({ context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_SUBJECT, report.text("title")).putExtra(Intent.EXTRA_TEXT, report.text("body")), context.getString(R.string.assistant_share_report))) }, Modifier.weight(1f)) { Text(stringResource(R.string.assistant_share)) }
                }
                Row(Modifier.fillMaxWidth()) {
                    TextButton({ printAssistantReport(context, report) }, Modifier.weight(1f)) { Text(stringResource(R.string.assistant_print)) }
                    TextButton(close, Modifier.weight(1f)) { Text(stringResource(R.string.common_done)) }
                }
            }
        }
    }
}

@Composable
private fun AssistantReplyReportDialog(initial: String, controller: AssistantController, close: () -> Unit) {
    val state by controller.state.collectAsStateWithLifecycle()
    var text by remember { mutableStateOf(initial) }
    var reason by remember { mutableStateOf("") }
    FormDialog(stringResource(R.string.assistant_report_a_reply), close, stringResource(R.string.assistant_send_report), { controller.reportReply(text, reason, close) }, text.isNotBlank() && "reportReply" !in state.busy) {
        Text(stringResource(R.string.assistant_report_note))
        Field(text, { text = it.take(4000) }, stringResource(R.string.assistant_excerpt), lines = 8)
        Field(reason, { reason = it.take(1000) }, stringResource(R.string.assistant_what_went_wrong), lines = 4)
    }
}

/** The conversation as the panel lists it: steps in a row together, everything else on its own, each with its first index. */
internal fun assistantRows(entries: List<JSONObject>): List<Pair<Int, List<JSONObject>>> {
    val rows = mutableListOf<Pair<Int, MutableList<JSONObject>>>()
    entries.forEachIndexed { index, entry ->
        val last = rows.lastOrNull()
        if (entry.text("kind") == "step" && last != null && last.second.first().text("kind") == "step") last.second += entry
        else rows += index to mutableListOf(entry)
    }
    return rows
}

/** Lookups in a row: the latest and any that failed, with the rest a tap away. */
@Composable
private fun AssistantSteps(steps: List<JSONObject>) {
    var open by remember(steps.size) { mutableStateOf(false) }
    val shown = if (open) steps else steps.filterIndexed { i, step -> i == steps.lastIndex || step.optBoolean("failed") }
    Column(Modifier.fillMaxWidth().testTag("assistant-steps"), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        shown.forEach { step ->
            val failed = step.optBoolean("failed")
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Icon(if (failed) Icons.Outlined.ErrorOutline else Icons.Outlined.Check, null, Modifier.size(16.dp), tint = if (failed) ur.warning else ur.muted)
                Text(if (failed) stringResource(R.string.assistant_step_failed, step.text("text")) else step.text("text"), style = MaterialTheme.typography.labelLarge,
                    color = if (failed) ur.warning else ur.text2)
            }
        }
        if (open || steps.size > shown.size) Text(if (open) stringResource(R.string.assistant_steps_fewer) else countText(R.plurals.assistant_steps_more, steps.size - shown.size),
            Modifier.padding(start = 22.dp, top = 2.dp).clickable(role = Role.Button) { open = !open }.testTag("assistant-steps-toggle"),
            style = MaterialTheme.typography.labelLarge, color = ur.accent)
    }
}
