package app.urbudget.nativeapp.ui

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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import app.urbudget.nativeapp.AssistantController
import app.urbudget.nativeapp.AssistantState
import app.urbudget.nativeapp.BudgetViewModel
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
                             importing: JSONObject? = null, applyFix: ((thread: String, proposal: JSONObject) -> Unit)? = null) {
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
    var pendingExport by remember { mutableStateOf<Pair<String, ByteArray>?>(null) }
    var exportError by remember { mutableStateOf<String?>(null) }
    val list = rememberLazyListState()
    val entries = state.entries
    val attach = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) controller.attach(context, uri)
    }
    val export = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        val data = pendingExport
        if (uri != null && data != null) scope.launch {
            runCatching { withContext(Dispatchers.IO) {
                context.contentResolver.openOutputStream(uri)?.use { it.write(data.second) } ?: error("The export could not be saved.")
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

    Column(Modifier.fillMaxSize().imePadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Assistant", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
                Text(state.thread?.text("title").orEmpty().ifEmpty { "Your budget, with a second pair of eyes" }, maxLines = 1, style = MaterialTheme.typography.labelSmall)
            }
            IconButton({ history = true }) { Icon(Icons.Outlined.History, "Conversation history") }
            IconButton({ controller.newChat(); message = ""; selected = emptyList() }) { Icon(Icons.Outlined.AddComment, "New conversation") }
            IconButton({ settings = true }) { Icon(Icons.Outlined.Settings, "Assistant settings") }
            if (onClose != null) IconButton(onClose) { Icon(Icons.Outlined.Close, "Close assistant") }
        }
        if (state.busy.isNotEmpty()) LinearProgressIndicator(Modifier.fillMaxWidth())
        state.notice?.let { notice ->
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(notice, Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
                IconButton(controller::dismissNotice) { Icon(Icons.Outlined.Close, "Dismiss notice") }
            }
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), state = list, contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            if (entries.isEmpty()) item {
                Text("Ask about your budget. Review every change.", style = MaterialTheme.typography.headlineSmall)
                Spacer(Modifier.height(8.dp))
                Text("Look up spending, cover overspending, categorize selected transactions, read statements, or create a report. Proposed changes are checked again before you apply them.", style = MaterialTheme.typography.bodyMedium)
                if (state.status?.optBoolean("ready") != true) FilledTonalButton({ settings = true }, Modifier.padding(top = 12.dp)) { Text("Set up assistant") }
                else listOf("Where is my money going this month?", "Help me cover overspending", "Make a report of this month's budget").forEach { prompt ->
                    OutlinedButton({ message = prompt }, Modifier.fillMaxWidth()) { Text(prompt) }
                }
            }
            itemsIndexed(entries, key = { index, _ -> "${state.threadId}:$index" }, contentType = { _, entry -> entry.text("kind") }) { _, entry ->
                when (entry.text("kind")) {
                    "proposal" -> entry.optJSONObject("proposal")?.let { proposal ->
                        AssistantProposalCard(proposal, budget.text("currency"), state.busy.contains("proposal"), state.undoProposal == proposal.text("id"),
                            apply = { controller.apply(proposal.text("id"), vm::refresh) },
                            applyFix = applyFix?.let { fix -> { fix(state.threadId, proposal) } },
                            undo = { controller.undoProposal(proposal.text("id"), vm::refresh) },
                            discard = { controller.discard(proposal.text("id")) },
                            refine = { message = "Revise the proposal ‘${proposal.text("title")}’: " },
                            reviewImport = { val thread = state.threadId; controller.importFile(proposal.text("id")) {
                                importFile = it.put("assistantThreadId", thread).put("assistantProposalId", proposal.text("id"))
                            } },
                            saveImport = { controller.saveImport(proposal.text("id"), ::savePath) },
                            report = { controller.report(proposal.text("id")) { report = it } },
                            spreadsheet = { open -> controller.spreadsheet(proposal.text("id"), open) { if (!open) savePath(it) } })
                    }
                    "step" -> Text((if (entry.optBoolean("failed")) "Couldn’t complete: " else "✓ ") + entry.text("text"), style = MaterialTheme.typography.labelMedium,
                        color = if (entry.optBoolean("failed")) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
                    "stopped" -> Text("Reply stopped", style = MaterialTheme.typography.labelMedium)
                    else -> {
                        val user = entry.text("kind") == "user"
                        Surface(color = if (user) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerLow,
                            shape = MaterialTheme.shapes.large, modifier = Modifier.fillMaxWidth()) {
                            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                Text(if (user) "You" else if (entry.text("kind") == "error") "Couldn’t finish" else "Assistant", style = MaterialTheme.typography.labelMedium)
                                SelectionContainer {
                                    if (entry.text("kind") == "assistant") AssistantMarkdown(entry.text("text"))
                                    else Text(entry.text("text"), color = if (entry.text("kind") == "error") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
                                }
                                entry.rows("attachments").forEach { Text("📎 ${it.text("name")}", style = MaterialTheme.typography.labelMedium) }
                                if (entry.text("kind") == "error") TextButton({ controller.retry(budgetState.month, importing) }, enabled = !state.running) { Text("Try again") }
                                if (entry.text("kind") == "assistant") TextButton({ reportReply = entry.text("text").take(4000) }) { Text("Report reply") }
                            }
                        }
                    }
                }
            }
            item(key = "live-reply") { AssistantLiveReply(controller, list) }
            if (state.running) item { Row(verticalAlignment = Alignment.CenterVertically) { CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp); Text(" Working…", style = MaterialTheme.typography.labelMedium) } }
            else if (entries.lastOrNull()?.text("kind") == "user") item {
                TextButton({ controller.retry(budgetState.month, importing) }, enabled = "send" !in state.busy) { Text("Continue interrupted reply") }
            }
        }
        if (state.attachments.isNotEmpty() || selected.isNotEmpty()) Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Column(Modifier.weight(1f)) {
                state.attachments.forEach { file -> InputChip(true, { controller.removeAttachment(file.text("id")) }, label = { Text(file.text("name"), maxLines = 1) }, trailingIcon = { Icon(Icons.Outlined.Close, "Remove attachment") }) }
                if (selected.isNotEmpty()) InputChip(true, { contextPicker = true }, label = { Text("${selected.size} selected transactions") })
            }
        }
        Row(Modifier.fillMaxWidth().padding(12.dp), verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Column {
                IconButton({ attach.launch(arrayOf("*/*")) }, enabled = !state.running && state.attachments.size < 5 && "attach" !in state.busy) { Icon(Icons.Outlined.AttachFile, "Attach statement, document, or image") }
                IconButton({ contextPicker = true }, enabled = !state.running) { Icon(Icons.Outlined.Checklist, "Select budget transactions") }
            }
            FilledField(message, { message = it.take(20_000) }, Modifier.weight(1f), label = { Text("Ask about your budget") }, maxLines = 6)
            if (state.running) IconButton(controller::stop) { Icon(Icons.Outlined.StopCircle, "Stop reply") }
            else IconButton({ controller.send(message, budgetState.month, selected, importing) { message = ""; selected = emptyList() } },
                enabled = state.status?.optBoolean("ready") == true && "send" !in state.busy && (message.isNotBlank() || state.attachments.isNotEmpty())) { Icon(Icons.AutoMirrored.Outlined.Send, "Send message") }
        }
    }

    if (settings) AssistantSettingsDialog(controller) { settings = false }
    if (history) ModalBottomSheet(onDismissRequest = { history = false }) {
        Text("Conversations", Modifier.padding(horizontal = 20.dp), style = MaterialTheme.typography.titleLarge)
        if (state.thread != null) TextButton({
            val thread = state.thread!!
            val currency = budget.text("currency")
            scope.launch {
                runCatching { withContext(Dispatchers.Default) {
                    "${thread.text("title").take(60).replace(Regex("[^\\p{L}\\p{N} _-]"), "_")}.md" to conversationMarkdown(thread, currency).toByteArray()
                } }.onSuccess { saveBytes(it.first, it.second) }.onFailure { exportError = it.message }
            }
        }, Modifier.padding(horizontal = 12.dp)) { Text("Export current conversation") }
        LazyColumn(Modifier.heightIn(max = 500.dp)) {
            if (state.threads.isEmpty()) item { Text("No saved conversations yet", Modifier.padding(20.dp)) }
            items(state.threads, key = { it.text("id") }) { thread ->
                ListItem(headlineContent = { TextButton({ controller.openThread(thread.text("id")); history = false }) { Text(thread.text("title")) } },
                    supportingContent = { Text(thread.text("updated").take(10)) }, trailingContent = { IconButton({ deleteId = thread.text("id") }) { Icon(Icons.Outlined.DeleteOutline, "Delete conversation") } })
            }
        }
    }
    deleteId?.let { id -> AlertDialog(onDismissRequest = { deleteId = null }, title = { Text("Delete this conversation?") }, text = { Text("This removes its saved history from this device.") },
        confirmButton = { TextButton({ controller.deleteThread(id); deleteId = null }) { Text("Delete") } }, dismissButton = { TextButton({ deleteId = null }) { Text("Cancel") } }) }
    importFile?.let { file ->
        var draft by remember(file) { mutableStateOf(preparedBankImport(file, file.text("assistantThreadId"), file.text("assistantProposalId"))) }
        BankImportDialog(budget, vm, draft, { change -> draft = change(draft) },
            onImported = { controller.imported(file.text("assistantThreadId"), file.text("assistantProposalId")) }) { importFile = null }
    }
    report?.let { document -> AssistantReportDialog(document, { report = null }, {
        scope.launch { val bytes = withContext(Dispatchers.Default) { document.text("body").toByteArray() }; saveBytes("${document.text("title").take(60)}.md", bytes) }
    }) }
    reportReply?.let { excerpt -> AssistantReplyReportDialog(excerpt, controller, { reportReply = null }) }
    if (contextPicker) Dialog(onDismissRequest = { contextPicker = false }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        val transactions by produceState<List<JSONObject>?>(null, budget) {
            value = null
            value = withContext(Dispatchers.Default) { budget.rows("transactions").sortedByDescending { it.text("date") } }
        }
        val selectedIds = remember(selected) { selected.toSet() }
        Surface(Modifier.fillMaxWidth().fillMaxHeight(.85f).padding(16.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(16.dp)) {
                Text("Transactions to discuss", style = MaterialTheme.typography.titleLarge)
                Text("Select up to 200 transactions for your next message.", style = MaterialTheme.typography.bodySmall)
                if (transactions == null) LinearProgressIndicator(Modifier.fillMaxWidth())
                LazyColumn(Modifier.weight(1f)) {
                    items(transactions.orEmpty(), key = { it.text("id") }) { row ->
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(row.text("id") in selectedIds, { checked -> selected = if (checked) selected + row.text("id") else selected - row.text("id") }, enabled = row.text("id") in selectedIds || selected.size < 200)
                            Column { Text(row.text("payee")); Text("${row.text("date")} · ${money(row.optLong("amount"), budget.text("currency"))}", style = MaterialTheme.typography.bodySmall) }
                        }
                    }
                }
                Row { TextButton({ selected = emptyList() }) { Text("Clear") }; Spacer(Modifier.weight(1f)); Button({ contextPicker = false }) { Text("Done (${selected.size})") } }
            }
        }
    }
    val error = exportError ?: state.error
    if (error != null) AlertDialog(onDismissRequest = { controller.dismissError(); exportError = null }, title = { Text("Couldn’t complete that") }, text = { Text(error) },
        confirmButton = { TextButton({ controller.dismissError(); exportError = null }) { Text("OK") } })
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

@Composable
private fun AssistantProposalCard(proposal: JSONObject, currency: String, busy: Boolean, canUndo: Boolean, apply: () -> Unit, undo: () -> Unit, discard: () -> Unit,
    refine: () -> Unit, reviewImport: () -> Unit, saveImport: () -> Unit, report: () -> Unit, spreadsheet: (Boolean) -> Unit, applyFix: (() -> Unit)? = null) {
    val card = proposal.optJSONObject("card") ?: JSONObject()
    val rows = remember(card) { card.rows("rows") }
    var page by remember(proposal.text("id")) { mutableIntStateOf(0) }
    val pages = ((rows.size + 19) / 20).coerceAtLeast(1)
    val currentPage = page.coerceAtMost(pages - 1)
    val status = proposal.text("status")
    if (status == "applied" || status == "discarded") {
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp)) {
                Text("${if (status == "applied") "✓ Applied" else "Discarded"}: ${proposal.text("title")}", style = MaterialTheme.typography.titleSmall)
                if (status == "applied" && canUndo) TextButton(undo, enabled = !busy) { Text("Undo this proposal") }
            }
        }
        return
    }
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(card.text("kicker"), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
            Text(proposal.text("title"), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.Bold)
            card.rows("stats").forEach { stat -> Text("${stat.text("label")}: ${if (stat.has("amount") && !stat.isNull("amount")) money(stat.optLong("amount"), currency) else stat.text("text")}") }
            (rows.subList(currentPage * 20, minOf(rows.size, (currentPage + 1) * 20)) + listOfNotNull(card.optJSONObject("total"))).forEach { row ->
                Column(Modifier.padding(start = if (row.optBoolean("indent")) 16.dp else 0.dp)) {
                    Row { Text(row.text("label"), Modifier.weight(1f)); if (row.has("amount")) Text(money(row.optLong("amount"), currency), fontWeight = FontWeight.SemiBold) }
                    if (row.text("detail").isNotEmpty()) Text(row.text("detail"), style = MaterialTheme.typography.bodySmall)
                    if (row.has("before") || row.has("after")) Text("${money(row.optLong("before"), currency)} → ${money(row.optLong("after"), currency)}", style = MaterialTheme.typography.bodySmall)
                    if (row.text("badge").isNotEmpty()) Text(row.text("badge"), style = MaterialTheme.typography.labelSmall, color = if (row.text("tone") == "warn") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
                }
            }
            if (pages > 1) Row {
                TextButton({ page = currentPage - 1 }, enabled = currentPage > 0) { Text("Previous rows") }
                Text("${currentPage + 1} / $pages", Modifier.padding(12.dp))
                TextButton({ page = currentPage + 1 }, enabled = currentPage + 1 < pages) { Text("Next rows") }
            }
            if (card.optInt("more") > 0) Text("${card.optInt("more")} more rows", style = MaterialTheme.typography.labelSmall)
            card.optJSONArray("check")?.let { Text((if (it.optBoolean(0)) "✓ " else "Check: ") + it.optString(1), color = if (it.optBoolean(0)) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error) }
            listOf("warning", "note").forEach { if (card.text(it).isNotEmpty()) Text(card.text(it), style = MaterialTheme.typography.bodySmall) }
            if (proposal.text("problem").isNotEmpty() && proposal.text("kind") != "fix") Text(proposal.text("problem"), color = MaterialTheme.colorScheme.error)
            if (status == "failed") TextButton(discard, enabled = !busy) { Text("Dismiss") }
            else when (proposal.text("kind")) {
                // Fixes for a bank file being imported: the import applies them, each only to a line that hasn't changed.
                "fix" -> if (status == "proposed" || status == "undone") {
                    Button(applyFix ?: {}, enabled = !busy && applyFix != null) { Text(if (applyFix != null) "Apply to the import" else "Apply from the import") }
                    Text(if (applyFix != null) "Each applies only to a line that hasn't changed." else "Open the import these fixes are for.", style = MaterialTheme.typography.bodySmall)
                    Row { TextButton(refine, enabled = !busy) { Text("Ask for changes") }; TextButton(discard, enabled = !busy) { Text("Discard") } }
                } else Text(if (status == "applied") "✓ Applied to the import" else "Discarded", style = MaterialTheme.typography.labelMedium)
                "report" -> Button(report, enabled = !busy) { Text("Read report · Print / PDF") }
                "import" -> { Button(reviewImport, enabled = !busy) { Text("Review ${proposal.optInt("rows")} import rows") }; TextButton(saveImport) { Text("Export converted CSV") } }
                "spreadsheet" -> { Button({ spreadsheet(true) }, enabled = !busy) { Text("Open spreadsheet") }; TextButton({ spreadsheet(false) }) { Text("Export Excel file") } }
                else -> when (status) {
                    "proposed", "undone" -> {
                        Button(apply, enabled = !busy) { Text("Apply reviewed changes") }
                        Row { TextButton(refine, enabled = !busy) { Text("Ask for changes") }; TextButton(discard, enabled = !busy) { Text("Discard") } }
                    }
                    "applied" -> { Text("✓ Applied", color = MaterialTheme.colorScheme.primary); if (canUndo) TextButton(undo, enabled = !busy) { Text("Undo this proposal") } }
                    "discarded" -> Text("Discarded", style = MaterialTheme.typography.labelMedium)
                    "failed" -> Text("Could not apply", color = MaterialTheme.colorScheme.error)
                }
            }
            if (proposal.text("kind") == "report" && proposal.optInt("sheets") > 0) TextButton({ spreadsheet(false) }) { Text("Export report tables to Excel") }
            if (status != "failed" && proposal.text("kind") in listOf("report", "spreadsheet", "import")) TextButton(discard, enabled = !busy) { Text("Discard") }
        }
    }
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
                Text("${report.text("budget")} · ${report.text("period")} · ${report.text("created").take(10)}", style = MaterialTheme.typography.labelMedium)
                SelectionContainer(Modifier.weight(1f)) { AssistantMarkdownDocument(report.text("body"), Modifier.fillMaxSize()) }
                Row(Modifier.fillMaxWidth()) {
                    TextButton(export, Modifier.weight(1f)) { Text("Save text") }
                    TextButton({ context.startActivity(Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_SUBJECT, report.text("title")).putExtra(Intent.EXTRA_TEXT, report.text("body")), "Share report")) }, Modifier.weight(1f)) { Text("Share") }
                }
                Row(Modifier.fillMaxWidth()) {
                    TextButton({ printAssistantReport(context, report) }, Modifier.weight(1f)) { Text("Print / PDF") }
                    TextButton(close, Modifier.weight(1f)) { Text("Done") }
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
    FormDialog("Report a reply", close, "Send report", { controller.reportReply(text, reason, close) }, text.isNotBlank() && "reportReply" !in state.busy) {
        Text("Only the excerpt and explanation below will be sent to UrBudget. Edit out anything private before sending.")
        Field(text, { text = it.take(4000) }, "Selected excerpt", lines = 8)
        Field(reason, { reason = it.take(1000) }, "What went wrong?", lines = 4)
    }
}
