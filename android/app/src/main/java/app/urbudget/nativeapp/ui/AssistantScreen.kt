package app.urbudget.nativeapp.ui

import app.urbudget.nativeapp.BankTextsAssistant
import app.urbudget.nativeapp.PageRequests
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Send
import androidx.compose.material.icons.filled.ThumbDown
import androidx.compose.material.icons.filled.ThumbUp
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import app.urbudget.nativeapp.AssistantAnnouncement
import app.urbudget.nativeapp.AssistantCards
import app.urbudget.nativeapp.AssistantController
import app.urbudget.nativeapp.AssistantEntryKind
import app.urbudget.nativeapp.AssistantErrorAction
import app.urbudget.nativeapp.AssistantHeard
import app.urbudget.nativeapp.AssistantListFrame
import app.urbudget.nativeapp.AssistantPart
import app.urbudget.nativeapp.AssistantPlace
import app.urbudget.nativeapp.AssistantScroll
import app.urbudget.nativeapp.AssistantSelectionLimit
import app.urbudget.nativeapp.AssistantShow
import app.urbudget.nativeapp.AssistantUsage
import app.urbudget.nativeapp.BuildConfig
import app.urbudget.nativeapp.CardDone
import app.urbudget.nativeapp.ProposalAction
import app.urbudget.nativeapp.ProposalCards
import app.urbudget.nativeapp.SignInService
import app.urbudget.nativeapp.assistantFollows
import app.urbudget.nativeapp.assistantPageName
import app.urbudget.nativeapp.assistantProviderCompany
import app.urbudget.nativeapp.assistantReportDetails
import app.urbudget.nativeapp.assistantReportReason
import app.urbudget.nativeapp.assistantSending
import app.urbudget.nativeapp.assistantThreadsMatching
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.ProposalChoices
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import org.json.JSONObject
import java.io.File
import java.time.Instant
import java.time.ZoneId

/**
 * Native chat and proposal review for the same Rust assistant used by desktop. `importing` is the
 * bank file being imported when the assistant is open over its import ({account, content,
 * session}); fixes it proposes for that import go to `applyFix`. `place` is where the app is,
 * which goes with a message unless the user takes it off. `covers` says whether the panel is
 * over the app (a phone) rather than beside it, and `show` takes the app to what a card names.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
internal fun AssistantScreen(vm: BudgetViewModel, controller: AssistantController = viewModel(), onClose: (() -> Unit)? = null,
                             importing: JSONObject? = null, applyFix: ((proposal: JSONObject) -> Unit)? = null,
                             place: AssistantPlace = AssistantPlace(), dragArea: Modifier = Modifier,
                             covers: () -> Boolean = { true }, show: (AssistantShow) -> Unit = {}) {
    // Token updates only recompose the live reply, not history, proposal controls or composer.
    val panel = remember(controller) { controller.state.map { it.copy(streaming = "", thinking = "") }.distinctUntilChanged() }
    // From what the controller holds now: a panel opened again shows its conversation at once, not an empty one first.
    val state by panel.collectAsStateWithLifecycle(remember(controller) { controller.state.value.copy(streaming = "", thinking = "") })
    // Whether a reply is being written; its words are the live reply's own to follow.
    val writing by remember(controller) { controller.state.map { it.streaming.isNotEmpty() }.distinctUntilChanged() }
        .collectAsStateWithLifecycle(remember(controller) { controller.state.value.streaming.isNotEmpty() })
    val budgetState by vm.state.collectAsStateWithLifecycle()
    val budget = budgetState.budget ?: return
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // The message being written is the controller's, so closing the panel keeps it.
    val composer = controller.composer
    var settings by rememberSaveable { mutableStateOf(false) }
    var history by rememberSaveable { mutableStateOf(false) }
    var contextPicker by remember { mutableStateOf(false) }
    var deleteId by remember { mutableStateOf<String?>(null) }
    // A conversation being renamed: its id, and its title as it is.
    var renaming by remember { mutableStateOf<Pair<String, String>?>(null) }
    var importFile by remember { mutableStateOf<JSONObject?>(null) }
    var report by remember { mutableStateOf<JSONObject?>(null) }
    // A reply being reported: its entry, for its words and the tools its turn called.
    var reportReply by remember { mutableStateOf<JSONObject?>(null) }
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
    // A proposal card's row being given another category or amount: the card, and the row as it shows.
    var changing by remember { mutableStateOf<Pair<String, JSONObject>?>(null) }
    // The import the panel is open over, by its session: a card of fixes applies only there.
    val importSession = importing?.optJSONObject("session")?.optJSONObject("scope")?.text("session")?.ifEmpty { null }
    // A category's name, for a card's row to show the one just given it.
    val categories = remember(budget) { budget.rows("categories").associate { it.text("id") to it.text("name") } }
    val unassigned = stringResource(R.string.category_unassigned)
    val deleted = stringResource(R.string.category_deleted)
    val categoryName = remember(categories, unassigned, deleted) { { id: String? -> if (id == null) unassigned else categories[id] ?: deleted } }
    val entries = state.entries
    // Steps in a row read as one line, the latest; the rest are a tap away.
    val rows = remember(entries) { assistantRows(entries) }
    val ready = state.status?.optBoolean("ready") == true
    // A conversation still being read shows nothing yet, rather than the start screen.
    val opening = state.thread?.has("entries") == false
    val continuedFrom = state.thread?.text("continuedFrom").orEmpty()
    val started = entries.isNotEmpty() || continuedFrom.isNotEmpty()
    val interrupted = !state.running && entries.lastOrNull()?.let(AssistantEntryKind::of) == AssistantEntryKind.User
    // What's waiting in the budget that the assistant can help with: offered under the last reply, while nothing runs.
    val next = remember(state.thread, state.running, writing) { if (state.running || writing) emptyList() else state.next }
    // The list's items, to know its last: where it carries on from, the start screen, the conversation, the reply being written, what it's doing, what's next.
    val count = (if (continuedFrom.isNotEmpty()) 1 else 0) + (if (!started && !opening) 1 else 0) + rows.size + (if (writing) 1 else 0) +
        (if (state.running || interrupted) 1 else 0) + (if (next.isNotEmpty()) 1 else 0)
    val end by rememberUpdatedState(count - 1)
    // The list opens where the panel left it, or at its end.
    val list = remember(controller) { composer.scroll?.let { LazyListState(it.index, it.offset) } ?: LazyListState((count - 1).coerceAtLeast(0)) }
    // One rule for scrolling: new text is followed only from the end. Scrolling away stops it and offers "Jump to latest".
    var following by remember(controller) { mutableStateOf(composer.scroll == null) }
    LaunchedEffect(list) {
        var before: AssistantListFrame? = null
        snapshotFlow { AssistantListFrame(list.firstVisibleItemIndex, list.firstVisibleItemScrollOffset, list.canScrollForward, list.isScrollInProgress) }.collect { now ->
            following = assistantFollows(following, before, now)
            before = now
        }
    }
    LaunchedEffect(list) {
        // More lies below while following (text arrived, the keyboard rose) and no finger is on the list: down to it.
        snapshotFlow { following && list.canScrollForward && !list.isScrollInProgress }.collect { behind ->
            // A scroll the user starts at that moment takes over; this one just gives way.
            if (behind) try { list.toEnd(end) } catch (refused: CancellationException) { ensureActive() }
        }
    }
    // A conversation lands at its end as it opens, and again once it has been read. It's asked for
    // before the list is next laid out, so the conversation's start doesn't show for a frame first.
    val landing = "${state.threadId}:$opening"
    val landed = remember(controller) { arrayOf(landing) }
    SideEffect {
        if (landed[0] != landing) {
            landed[0] = landing
            following = true
            if (count > 0) list.requestScrollToItem(count - 1)
        }
    }
    // Closing the panel keeps the list's place, for when it opens again.
    DisposableEffect(controller, list) {
        onDispose { controller.keepScroll(if (following) null else AssistantScroll(list.firstVisibleItemIndex, list.firstVisibleItemScrollOffset)) }
    }
    // Read aloud once each: a reply when it has finished, a card when it arrives. Never the text as it's written.
    val view = LocalView.current
    val announcements = AssistantAnnouncement.values().associateWith { stringResource(it.words) }
    val heard = remember(controller) { mutableStateOf<AssistantHeard?>(null) }
    LaunchedEffect(state.threadId, state.running, entries) {
        val now = AssistantHeard.of(state.threadId, state.running, entries)
        @Suppress("DEPRECATION")
        AssistantAnnouncement.between(heard.value, now).forEach { view.announceForAccessibility(announcements.getValue(it)) }
        heard.value = now
    }
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
    val clipboard = LocalClipboardManager.current
    /** What goes with a message sent now: where the app is, the transactions picked, the file being imported, less what the user took off. */
    fun sending() = assistantSending(place, budgetState.month, composer.selected, importing, composer.without, composer.category)
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
            // Wherever the list was, the message just sent is shown.
            following = true
            controller.send(text, sending(), texts)
        }
    }
    fun retry() = whenTold { scope.launch { controller.retry(sending(), checkBankTexts()) } }
    // Whether a message can go now: a card's answer, a new version asked for and what's next all go as one.
    val canSend = ready && !state.running && "send" !in state.busy
    /** What a card's button does. The card says which it offers; this is where each leads. */
    fun act(proposal: JSONObject, action: ProposalAction) {
        val id = proposal.text("id")
        when (action) {
            ProposalAction.Apply -> controller.apply(id, vm::refresh)
            ProposalAction.ApplyToImport -> applyFix?.invoke(proposal)
            // A bank-text card is done on the phone: Bank messages opens when it reads.
            ProposalAction.ApplyBankTexts -> proposal.optJSONObject("bankTexts")?.let { payload ->
                controller.carryOut(id) {
                    val done = BankTextsAssistant.apply(context.applicationContext, vm, payload)
                    // Over the app the panel gives way to the page; beside it both stay in view. With texts
                    // gone from the phone since, it stays: the card says so, where a card says what it has to.
                    done.page?.let { PageRequests.page.value = it; if (done.notice == null && covers()) onClose?.invoke() }
                    CardDone(note = done.notice)
                }
            }
            // After what's already typed, on a line of its own: never over it.
            ProposalAction.AskChanges -> controller.prefill(Words[R.string.assistant_refine_prompt, proposal.text("title")] + " ")
            ProposalAction.UseOriginal -> controller.useOriginal(id)
            ProposalAction.Discard, ProposalAction.Remove -> controller.mark(id, "discarded")
            ProposalAction.Restore -> controller.mark(id, "proposed")
            ProposalAction.Undo -> controller.undoProposal(id, vm::refresh)
            ProposalAction.AskAgain -> ask(ProposalCards.askAgain(proposal))
            ProposalAction.Show -> ProposalCards.show(proposal)?.let(show)
            ProposalAction.Preview -> controller.report(id) { report = it }
            ProposalAction.Print -> controller.report(id) { printAssistantReport(context, it) }
            ProposalAction.Excel, ProposalAction.Save -> controller.spreadsheet(id, false, ::savePath)
            ProposalAction.Open -> controller.spreadsheet(id, true)
            ProposalAction.ReviewImport -> { val thread = state.threadId; controller.importFile(id) { importFile = it.put("assistantThreadId", thread).put("assistantProposalId", id) } }
            ProposalAction.SaveCsv -> controller.saveImport(id, ::savePath)
            // A question's answers are its rows.
            ProposalAction.Answer -> {}
        }
    }
    val signingIn = "signIn" in state.busy || state.status?.optBoolean("signingIn") == true
    Column(Modifier.fillMaxSize().imePadding()) {
        Row(Modifier.fillMaxWidth().then(dragArea).padding(start = 20.dp, end = 4.dp, top = 2.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            val title = state.thread?.text("title").orEmpty()
            val renameLabel = stringResource(R.string.assistant_rename)
            // Under the panel's name, the open conversation's: a tap renames it.
            Column(Modifier.weight(1f).then(if (title.isEmpty()) Modifier else Modifier.clip(RoundedCornerShape(10.dp))
                .clickable(onClickLabel = renameLabel, role = Role.Button) { renaming = state.threadId to title }.testTag("assistant-title"))) {
                Text(stringResource(R.string.assistant_title), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, color = ur.text)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(title.ifEmpty { stringResource(R.string.assistant_tagline) }, Modifier.weight(1f, fill = false), maxLines = 1, overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodySmall.own, color = ur.muted)
                    if (title.isNotEmpty()) Icon(Icons.Outlined.Edit, null, Modifier.size(13.dp), tint = ur.muted)
                }
            }
            IconButton({ history = true }) { Icon(Icons.Outlined.History, stringResource(R.string.assistant_history), tint = ur.text) }
            // What's typed comes along; a reply still running in the conversation left carries on there.
            IconButton(controller::startNew) { Icon(Icons.Outlined.AddComment, stringResource(R.string.assistant_new), tint = ur.text) }
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
        // Something done unasked failed (reading the conversation again, a refresh on coming back): a line here, not a dialog.
        state.quiet?.let { quiet ->
            Row(Modifier.padding(horizontal = 16.dp).testTag("assistant-quiet"), verticalAlignment = Alignment.CenterVertically) {
                Text(quiet, Modifier.weight(1f), style = MaterialTheme.typography.bodySmall, color = ur.warning)
                IconButton(controller::dismissQuiet) { Icon(Icons.Outlined.Close, stringResource(R.string.assistant_dismiss_notice)) }
            }
        }
        Box(Modifier.weight(1f).fillMaxWidth()) {
        LazyColumn(Modifier.fillMaxSize(), state = list, contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            if (continuedFrom.isNotEmpty()) item(key = "continued", contentType = "note") {
                Text(stringResource(R.string.assistant_continued_from, continuedFrom), style = MaterialTheme.typography.labelMedium.own, color = ur.muted)
            }
            if (!started && !opening) item(key = "start", contentType = "start") {
                Text(stringResource(R.string.assistant_empty_title), style = MaterialTheme.typography.headlineSmall, color = ur.text)
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.assistant_empty_detail), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                // Each state its own: still finding out, couldn't find out, not set up, ready to ask.
                if (state.status == null && !state.statusFailed) Row(Modifier.padding(top = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp, color = ur.accent)
                    Text(stringResource(R.string.assistant_checking), Modifier.testTag("assistant-checking"), style = MaterialTheme.typography.labelLarge, color = ur.text2)
                }
                else if (state.status == null) Column(Modifier.padding(top = 12.dp)) {
                    Text(stringResource(R.string.assistant_not_loaded), style = MaterialTheme.typography.bodyMedium, color = ur.text)
                    TextButton(controller::refresh, Modifier.offset(x = (-12).dp), enabled = "refresh" !in state.busy) { Text(stringResource(R.string.assistant_try_again)) }
                }
                else if (!ready) FilledTonalButton({ settings = true }, Modifier.padding(top = 12.dp)) { Text(stringResource(R.string.assistant_set_up)) }
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
                when (AssistantEntryKind.of(entry)) {
                    AssistantEntryKind.Proposal -> entry.optJSONObject("proposal")?.let { proposal ->
                        val id = proposal.text("id")
                        // Only the card being changed waits; its ticks show at once and go to the core together.
                        AssistantProposalCard(proposal, budget.text("currency"), state.cardBusy(id), act = { act(proposal, it) }, undoable = state.undoable,
                            cards = controller.cards, choose = { controller.choose(id, it) }, change = { row -> changing = id to row },
                            // An answer goes as the user's message, by the way Send does.
                            answer = { ask(it) }, canSend = canSend,
                            fixHere = importSession != null && proposal.optJSONObject("fixes")?.optJSONObject("scope")?.text("session") == importSession,
                            category = categoryName)
                    }
                    AssistantEntryKind.Step -> AssistantSteps(row.second)
                    AssistantEntryKind.Stopped -> Text(stringResource(R.string.assistant_reply_stopped), style = MaterialTheme.typography.labelMedium, color = ur.muted)
                    // The user's words on the right, as a phone's messages are, and under them what went with them.
                    AssistantEntryKind.User -> Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        if (entry.text("text").isNotEmpty()) Surface(color = ur.accentSoft, shape = RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp, bottomStart = 18.dp, bottomEnd = 6.dp), modifier = Modifier.widthIn(max = 320.dp)) {
                            SelectionContainer { Text(entry.text("text"), Modifier.padding(horizontal = 14.dp, vertical = 10.dp), style = MaterialTheme.typography.bodyLarge.own, color = ur.text) }
                        }
                        entry.rows("attachments").forEach { Text("\uD83D\uDCCE ${Money.own(it.text("name"))}", style = MaterialTheme.typography.labelMedium, color = ur.muted) }
                        assistantWithLine(entry.optJSONObject("with"))?.let {
                            Text(it, Modifier.testTag("assistant-sent-with"), style = MaterialTheme.typography.labelSmall, color = ur.muted, textAlign = TextAlign.End)
                        }
                    }
                    AssistantEntryKind.Error -> Surface(color = ur.warningSoft, shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text(stringResource(R.string.assistant_couldnt_finish), style = MaterialTheme.typography.labelLarge, color = ur.warning)
                            SelectionContainer { Text(entry.text("text"), style = LocalTextStyle.current.own, color = ur.text) }
                            // What helps is the core's to say, and only for the latest error: an earlier one, or one asking again wouldn't help, has no button.
                            AssistantErrorAction.of(entry)?.let { action ->
                                TextButton({
                                    when (action) {
                                        AssistantErrorAction.Retry -> retry()
                                        AssistantErrorAction.Settings -> settings = true
                                        AssistantErrorAction.Usage -> controller.openLink("usage")
                                        AssistantErrorAction.NewConversation -> controller.continueInNew()
                                        // The sign-in the settings start, with its service kept answering the browser; signed in, the reply is asked for again.
                                        AssistantErrorAction.SignIn -> { SignInService.start(context); controller.signIn { retry() } }
                                    }
                                }, enabled = when (action) {
                                    AssistantErrorAction.Retry -> !state.running
                                    AssistantErrorAction.SignIn -> !state.running && !signingIn
                                    AssistantErrorAction.NewConversation -> !state.running && "open" !in state.busy
                                    else -> true
                                }) { Text(stringResource(action.label)) }
                            }
                        }
                    }
                    // A reply that stopped at its length limit. The latest can be asked for its rest.
                    AssistantEntryKind.Cut -> Column(Modifier.fillMaxWidth()) {
                        Text(stringResource(R.string.assistant_cut), style = MaterialTheme.typography.labelMedium, color = ur.muted)
                        if (row.first == entries.lastIndex && !state.running) TextButton({ retry() }, Modifier.offset(x = (-12).dp).testTag("assistant-continue-cut"), enabled = "send" !in state.busy) {
                            Text(stringResource(R.string.common_continue))
                        }
                    }
                    // The assistant's reply as text, with copying, reporting and rating it to hand.
                    AssistantEntryKind.Assistant -> Column(Modifier.fillMaxWidth()) {
                        SelectionContainer { AssistantMarkdown(entry.text("text")) }
                        Row(Modifier.offset(x = (-12).dp)) {
                            IconButton({ clipboard.setText(AnnotatedString(entry.text("text"))) }) {
                                Icon(Icons.Outlined.ContentCopy, stringResource(R.string.assistant_copy_reply), Modifier.size(18.dp), tint = ur.muted)
                            }
                            IconButton({ reportReply = entry }) {
                                Icon(Icons.Outlined.OutlinedFlag, stringResource(R.string.assistant_report_reply), Modifier.size(18.dp), tint = ur.muted)
                            }
                            // The user's thumb on the reply, kept on this device; the one pressed again comes off.
                            if (entry.has("index")) {
                                val rating = entry.text("rating")
                                ReplyThumb(up = true, chosen = rating == "up", enabled = "rate" !in state.busy) { controller.rate(entry.optInt("index"), if (rating == "up") null else true) }
                                ReplyThumb(up = false, chosen = rating == "down", enabled = "rate" !in state.busy) { controller.rate(entry.optInt("index"), if (rating == "down") null else false) }
                            }
                        }
                    }
                    // A kind of entry this app doesn't know: `assistantRows` leaves those out.
                    null -> {}
                }
            }
            // The reply being written stands where its entry will, under the same key, so the list holds still when one becomes the other.
            if (writing) item(key = "${state.threadId}:${entries.size}", contentType = "assistant") { AssistantLiveReply(controller) }
            // What it's doing now, while no words are arriving: the lookup under way, and the latest line of its thinking.
            if (state.running) item(key = "working", contentType = "working") {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp, color = ur.accent)
                        Text(state.activity?.let { "$it\u2026" } ?: stringResource(R.string.assistant_working), Modifier.testTag("assistant-activity"),
                            style = MaterialTheme.typography.labelLarge, color = ur.text2)
                    }
                    AssistantThinking(controller)
                }
            }
            else if (interrupted) item(key = "interrupted", contentType = "working") {
                TextButton({ retry() }, enabled = "send" !in state.busy) { Text(stringResource(R.string.assistant_continue_reply)) }
            }
            // The core's words, already in the app's language: each sends its own message.
            if (next.isNotEmpty()) item(key = "next", contentType = "next") {
                FlowRow(Modifier.testTag("assistant-next"), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    next.forEach { step ->
                        AssistChip({ ask(step.text("prompt")) }, label = { Text(step.text("text"), style = LocalTextStyle.current.own) }, enabled = canSend)
                    }
                }
            }
        }
        // Away from the end new text isn't followed. This goes back to it, and follows again.
        if (!following && count > 0) Surface({ scope.launch { following = true; list.toEnd(end) } },
            Modifier.align(Alignment.BottomCenter).padding(bottom = 6.dp).testTag("assistant-jump"), shape = RoundedCornerShape(18.dp), color = ur.surface,
            border = BorderStroke(1.dp, ur.border), shadowElevation = 3.dp) {
            Row(Modifier.padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Icon(Icons.Outlined.ArrowDownward, null, Modifier.size(16.dp), tint = ur.accent)
                Text(stringResource(R.string.assistant_jump_latest), style = MaterialTheme.typography.labelLarge, color = ur.accent)
            }
        }
        }
        // A conversation that can't be replied to as things stand gets the one thing that helps, not just a greyed Send.
        if (started) when {
            state.status == null -> if (state.statusFailed) AssistantBanner(stringResource(R.string.assistant_not_loaded), stringResource(R.string.assistant_try_again), "refresh" !in state.busy, controller::refresh)
            !ready && state.status?.text("provider") == "chatgpt" -> {
                if (signingIn) AssistantBanner(stringResource(R.string.assistant_sign_in_waiting), stringResource(R.string.assistant_cancel_sign_in), true, controller::cancelSignIn)
                else AssistantBanner(stringResource(R.string.assistant_sign_in_browser), stringResource(R.string.assistant_continue_chatgpt), true) { SignInService.start(context); controller.signIn() }
            }
            !ready -> AssistantBanner(null, stringResource(R.string.assistant_set_up), true) { settings = true }
            // Long enough that each reply is slower and costs more: it can be carried on in a new conversation.
            state.thread?.optBoolean("long") == true && !state.running ->
                AssistantBanner(stringResource(R.string.assistant_long), stringResource(R.string.assistant_continue_new), "open" !in state.busy, controller::continueInNew)
        }
        // What goes with the next message. Each part can be taken off on its own, and a new message starts with them all again.
        val next = sending()
        val here = next.place?.let { at ->
            assistantPlaceLine(assistantPageName(at.page)?.let { stringResource(it) } ?: at.page, next.month,
                at.accountId?.let { id -> remember(budget, id) { budget.row("accounts", id)?.text("name") } },
                at.categoryId?.let { id -> remember(budget, id) { budget.row("categories", id)?.text("name") } })
        }
        if (state.attachments.isNotEmpty() || here != null || next.selected.isNotEmpty() || next.import != null || composer.selectedOf != null) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 4.dp).testTag("assistant-with")) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(stringResource(R.string.assistant_with_message), Modifier.align(Alignment.CenterVertically), style = MaterialTheme.typography.labelMedium, color = ur.muted)
                    state.attachments.forEach { file -> WithPart(Money.own(file.text("name"))) { controller.removeAttachment(file.text("id")) } }
                    // Taken off, where the user is doesn't go: no page, month, account or category.
                    here?.let { WithPart(it) { composer.without += AssistantPart.Place; composer.category = null } }
                    if (next.selected.isNotEmpty()) WithPart(countText(R.plurals.assistant_selected_transactions, next.selected.size)) { composer.select(emptyList()) }
                    if (next.import != null) WithPart(stringResource(R.string.assistant_with_import)) { composer.without += AssistantPart.Import }
                }
                // More were selected than can go with a message: said here, where they were handed over.
                composer.selectedOf?.let { Text(countText(R.plurals.assistant_only_first, it), Modifier.padding(bottom = 4.dp), style = MaterialTheme.typography.bodySmall, color = ur.text2) }
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
            FilledField(composer.text, { composer.text = it.take(20_000) }, Modifier.weight(1f), label = { Text(stringResource(R.string.assistant_ask)) }, maxLines = 6)
            if (state.running) IconButton(controller::stop) { Icon(Icons.Outlined.StopCircle, stringResource(R.string.assistant_stop), tint = ur.text) }
            else FilledIconButton({ ask(composer.text) }, enabled = ready && "send" !in state.busy && (composer.text.isNotBlank() || state.attachments.isNotEmpty()),
                colors = IconButtonDefaults.filledIconButtonColors(containerColor = ur.accent, contentColor = if (ur.dark) ur.bg else Color.White)) {
                Icon(Icons.AutoMirrored.Outlined.Send, stringResource(R.string.assistant_send))
            }
        }
    }

    if (settings) AssistantSettingsDialog(controller, vm) { settings = false }
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
    // The conversations, in the app's own sheet: on a large screen it floats at the window's end, clear of a hinge.
    if (history) {
        var search by remember { mutableStateOf("") }
        val found = remember(state.threads, search) { assistantThreadsMatching(state.threads, search) }
        val height = screenHeight()
        UrSheet({ history = false }) {
            SheetHeader(stringResource(R.string.assistant_conversations), { history = false }, null, null)
            SearchField(search, { search = it }, stringResource(R.string.assistant_search_conversations), Modifier.padding(horizontal = 16.dp), Modifier.testTag("assistant-search"))
            if (state.thread != null) TextButton({
                val thread = state.thread!!
                val currency = budget.text("currency")
                scope.launch {
                    runCatching { withContext(Dispatchers.Default) {
                        "${thread.text("title").take(60).replace(Regex("[^\\p{L}\\p{N} _-]"), "_")}.md" to conversationMarkdown(thread, currency).toByteArray()
                    } }.onSuccess { saveBytes(it.first, it.second) }.onFailure { exportError = it.message }
                }
            }, Modifier.padding(horizontal = 8.dp)) { Text(stringResource(R.string.assistant_export_conversation)) }
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = height * .6f), contentPadding = PaddingValues(bottom = 12.dp)) {
                if (state.threads.isEmpty()) item { Text(stringResource(R.string.assistant_no_conversations), Modifier.padding(20.dp), color = ur.muted) }
                else if (found.isEmpty()) item { Text(stringResource(R.string.assistant_no_match), Modifier.padding(20.dp), color = ur.muted) }
                items(found, key = { it.text("id") }) { thread ->
                    val id = thread.text("id")
                    val open = id == state.threadId
                    Row(Modifier.fillMaxWidth().clickable { controller.openThread(id); history = false }.padding(start = 20.dp, end = 4.dp, top = 6.dp, bottom = 6.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(thread.text("title"), style = MaterialTheme.typography.bodyLarge.own, fontWeight = if (open) FontWeight.SemiBold else FontWeight.Normal,
                                color = ur.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text(conversationDay(thread.text("updated")), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                            // What the open conversation's requests used, in tokens. No prices.
                            if (open) AssistantUsage.of(state.thread)?.let { used ->
                                Text(stringResource(R.string.assistant_tokens, Money.count(used.sent), Money.count(used.cached), Money.count(used.received)),
                                    Modifier.testTag("assistant-usage"), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                            }
                        }
                        IconButton({ renaming = id to thread.text("title") }) { Icon(Icons.Outlined.Edit, stringResource(R.string.assistant_rename), Modifier.size(20.dp), tint = ur.muted) }
                        IconButton({ deleteId = id }) { Icon(Icons.Outlined.DeleteOutline, stringResource(R.string.assistant_delete_conversation), Modifier.size(20.dp), tint = ur.muted) }
                    }
                }
            }
        }
    }
    deleteId?.let { id -> AlertDialog(onDismissRequest = { deleteId = null }, title = { Text(stringResource(R.string.assistant_delete_conversation_title)) }, text = { Text(stringResource(R.string.assistant_delete_conversation_message)) },
        confirmButton = { TextButton({ controller.deleteThread(id); deleteId = null }) { Text(stringResource(R.string.common_delete)) } }, dismissButton = { TextButton({ deleteId = null }) { Text(stringResource(R.string.common_cancel)) } }) }
    renaming?.let { (id, title) ->
        var name by remember(id) { mutableStateOf(title) }
        FormDialog(stringResource(R.string.assistant_rename), { renaming = null }, onConfirm = { controller.renameThread(id, name) { renaming = null } },
            enabled = name.isNotBlank() && "rename" !in state.busy) {
            Field(name, { name = it.take(80) }, stringResource(R.string.assistant_conversation_name))
        }
    }
    importFile?.let { file ->
        var draft by remember(file) { mutableStateOf(preparedBankImport(file, file.text("assistantThreadId"), file.text("assistantProposalId"))) }
        BankImportDialog(budget, vm, draft, { change -> draft = change(draft) },
            onImported = { controller.imported(file.text("assistantThreadId"), file.text("assistantProposalId")) }) { importFile = null }
    }
    report?.let { document -> AssistantReportDialog(document, { report = null }, {
        scope.launch { val bytes = withContext(Dispatchers.Default) { document.text("body").toByteArray() }; saveBytes("${document.text("title").take(60)}.md", bytes) }
    }) }
    reportReply?.let { entry -> AssistantReplyReportDialog(entry, state.status, controller) { reportReply = null } }
    changing?.let { (id, row) ->
        ProposalRowEditor(row, budget, vm, budgetState.summary, budgetState.month, { change ->
            // On top of the card's choices as they stand now: those on their way to the core, or else the core's.
            val choices = controller.cards.choices(id)
                ?: entries.firstNotNullOfOrNull { entry -> entry.optJSONObject("proposal")?.takeIf { it.text("id") == id } }?.optJSONObject("choices")
            controller.choose(id, ProposalChoices.change(choices, row.text("key"), change)); changing = null
        }) { changing = null }
    }
    if (contextPicker) Dialog(onDismissRequest = { contextPicker = false }, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        val transactions by produceState<List<JSONObject>?>(null, budget) {
            value = null
            value = withContext(Dispatchers.Default) { budget.rows("transactions").sortedByDescending { it.text("date") } }
        }
        val selected = composer.selected
        val selectedIds = remember(selected) { selected.toSet() }
        // On a wide window it keeps the pages' readable width rather than the window's.
        Surface(Modifier.widthIn(max = ReadableWidth).fillMaxWidth().fillMaxHeight(.85f).padding(16.dp), shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(16.dp)) {
                Text(stringResource(R.string.assistant_transactions_to_discuss), style = MaterialTheme.typography.titleLarge)
                Text(stringResource(R.string.assistant_select_up_to), style = MaterialTheme.typography.bodySmall)
                if (transactions == null) LinearProgressIndicator(Modifier.fillMaxWidth())
                LazyColumn(Modifier.weight(1f)) {
                    items(transactions.orEmpty(), key = { it.text("id") }) { row ->
                        val id = row.text("id")
                        val checked = id in selectedIds
                        val enabled = checked || selected.size < AssistantSelectionLimit
                        // The box and what it picks are one thing to tap, and one to a screen reader.
                        Row(Modifier.fillMaxWidth().toggleable(checked, enabled = enabled, role = Role.Checkbox) { on -> composer.select(if (on) selected + id else selected - id) }
                            .heightIn(min = 48.dp).padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                            Checkbox(checked, null, Modifier.padding(horizontal = 12.dp), enabled = enabled)
                            Column { Text(Money.own(row.text("payee"))); Text("${Money.date(row.text("date"))} · ${money(row.optLong("amount"), budget.text("currency"))}", style = MaterialTheme.typography.bodySmall) }
                        }
                    }
                }
                Row { TextButton({ composer.select(emptyList()) }) { Text(stringResource(R.string.assistant_clear)) }; Spacer(Modifier.weight(1f)); Button({ contextPicker = false }) { Text(stringResource(R.string.assistant_done_count, "${selected.size}")) } }
            }
        }
    }
    val error = exportError ?: state.error
    if (error != null) AlertDialog(onDismissRequest = { controller.dismissError(); exportError = null }, title = { Text(stringResource(R.string.assistant_error_title)) }, text = { Text(error) },
        confirmButton = { TextButton({ controller.dismissError(); exportError = null }) { Text(stringResource(R.string.common_ok)) } })
}

/** The reply as it's written, formatted as it will be once it's whole. Only this recomposes as its words arrive. */
@Composable
private fun AssistantLiveReply(controller: AssistantController) {
    val text by remember(controller) { controller.state.map { it.streaming }.distinctUntilChanged() }
        .collectAsStateWithLifecycle(remember(controller) { controller.state.value.streaming })
    if (text.isNotEmpty()) SelectionContainer { AssistantLiveMarkdown(text, Modifier.fillMaxWidth().testTag("assistant-live")) }
}

/** The latest line of the model's thinking, labelled: one line, however long it thinks. */
@Composable
private fun AssistantThinking(controller: AssistantController) {
    val line by remember(controller) { controller.state.map { it.thinking }.distinctUntilChanged() }
        .collectAsStateWithLifecycle(remember(controller) { controller.state.value.thinking })
    if (line.isNotEmpty()) Text(stringResource(R.string.assistant_thinking, line), Modifier.testTag("assistant-thinking"),
        style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

/** Brings the conversation's list to its very end: to its last item, then down to that item's foot however tall it is. */
private suspend fun LazyListState.toEnd(last: Int) {
    if (last < 0) return
    // Straight to the last item when it's out of sight, so nothing on the way there is composed.
    if (layoutInfo.visibleItemsInfo.none { it.index >= last }) scrollToItem(last)
    scrollBy(100_000f)
}

/** A thumb under a reply: up or down, filled in while it's the user's. */
@Composable
private fun ReplyThumb(up: Boolean, chosen: Boolean, enabled: Boolean, press: () -> Unit) {
    IconToggleButton(chosen, { press() }, enabled = enabled) {
        Icon(if (up) (if (chosen) Icons.Filled.ThumbUp else Icons.Outlined.ThumbUp) else if (chosen) Icons.Filled.ThumbDown else Icons.Outlined.ThumbDown,
            stringResource(if (up) R.string.assistant_good_reply else R.string.assistant_bad_reply), Modifier.size(18.dp), tint = if (chosen) ur.accent else ur.muted)
    }
}

/** One thing that goes with the next message. Tapping it takes it off, which a screen reader says. */
@Composable
private fun WithPart(label: String, remove: () -> Unit) {
    val removeLabel = stringResource(R.string.common_remove)
    InputChip(false, remove, label = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis, style = LocalTextStyle.current.own) },
        modifier = Modifier.semantics { onClick(removeLabel) { remove(); true } },
        trailingIcon = { Icon(Icons.Outlined.Close, null, Modifier.size(16.dp)) })
}

/** A line above the message field with the one thing to do about it; with no [text], just that. */
@Composable
private fun AssistantBanner(text: String?, action: String, enabled: Boolean, act: () -> Unit) {
    Surface(Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 6.dp).testTag("assistant-banner"), shape = RoundedCornerShape(16.dp), color = ur.surface2) {
        Column(Modifier.padding(start = 14.dp, end = 6.dp, top = if (text != null) 10.dp else 0.dp)) {
            if (text != null) Text(text, Modifier.padding(end = 8.dp), style = MaterialTheme.typography.bodySmall, color = ur.text2)
            TextButton(act, Modifier.align(Alignment.End), enabled = enabled) { Text(action) }
        }
    }
}

/**
 * Where the user is, as "With this message" and a sent message say it: the page, its month when
 * it shows one ([month], as YYYY-MM), and the account or category in view.
 */
internal fun assistantPlaceLine(page: String, month: String?, account: String?, category: String?): String =
    Money.joined(listOfNotNull(page, month?.takeIf(String::isNotEmpty)?.let(::monthLabel), account?.takeIf(String::isNotEmpty), category?.takeIf(String::isNotEmpty)))

/** What went with a sent message, from its entry's `with`: the same line "With this message" showed. Nothing for a message from before that was kept. */
@Composable
private fun assistantWithLine(with: JSONObject?): String? {
    if (with == null) return null
    val pieces = mutableListOf<String>()
    val page = with.text("page")
    if (page.isNotEmpty()) pieces += assistantPlaceLine(assistantPageName(page)?.let { stringResource(it) } ?: page, with.text("month"), with.text("account"), with.text("category"))
    val transactions = with.optInt("transactions")
    if (transactions > 0) pieces += countText(R.plurals.assistant_selected_transactions, transactions)
    if (with.optBoolean("import")) pieces += stringResource(R.string.assistant_with_import)
    return pieces.takeIf { it.isNotEmpty() }?.let(Money::joined)
}

/** A conversation's day, written as the app writes dates. The core's time is in UTC. */
private fun conversationDay(updated: String): String =
    Money.date(runCatching { Instant.parse(updated).atZone(ZoneId.systemDefault()).toLocalDate().toString() }.getOrDefault(updated.take(10)))

/**
 * A card the assistant made, drawn as the core's view of it says: a change to apply, a thing it
 * made (a report, a spreadsheet, a converted file, figures), or a question whose answers are
 * buttons. Its buttons are the core's `actions`, the first the main one, and one this app
 * doesn't know is left out; what it has to say is its `message`, in one strip. While a change
 * waits, each part of it can be left out (its checkbox) or given another category or amount (its
 * pencil); the choices show at once and Apply makes what's left. A finished card folds to a line
 * that opens on its rows, as they were.
 *
 * [cards] keeps what the user did with the card (its page, whether it's opened, choices on their
 * way to the core); [choose] takes the user's choices as a revision (`ProposalChoices`), [change]
 * opens a row's editor and [answer] sends a question's answer. [fixHere] says the import a card
 * of fixes is for is the one open, and [category] names a category for a row just given one.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun AssistantProposalCard(proposal: JSONObject, currency: String, busy: Boolean, act: (ProposalAction) -> Unit, undoable: String? = null,
    cards: AssistantCards = remember { AssistantCards() }, choose: ((JSONObject) -> Unit)? = null, change: ((JSONObject) -> Unit)? = null,
    answer: ((String) -> Unit)? = null, canSend: Boolean = true, fixHere: Boolean = false, category: (String?) -> String = { it.orEmpty() }) {
    val id = proposal.text("id")
    val card = proposal.optJSONObject("card") ?: JSONObject()
    val kind = proposal.text("kind")
    // A kind this app doesn't know is drawn plainly: its rows to read, and what it offers that the app knows.
    val known = ProposalCards.known(proposal)
    val finished = ProposalCards.finished(proposal)
    val actions = remember(proposal, undoable) { ProposalCards.actions(proposal, undoable) }
    val buttons = remember(proposal, undoable) { if (finished) ProposalCards.openedButtons(proposal, undoable) else ProposalCards.buttons(proposal, undoable) }
    val cardRows = remember(card) { card.rows("rows") }
    // Its parts, to leave out or change, while it waits to be applied.
    val choosing = choose != null && known && !finished && !proposal.optBoolean("made") && kind != "question" &&
        buttons.any { it == ProposalAction.Apply || it == ProposalAction.ApplyToImport } && cardRows.any { it.text("key").isNotEmpty() }
    // What the user chose that the core doesn't have yet shows on the rows at once.
    val pending = if (choosing) cards.choices(id) else null
    val choices = pending ?: proposal.optJSONObject("choices")
    val rows = remember(cardRows, pending) { ProposalChoices.shown(cardRows, pending, category) }
    val chosen = rows.count { it.text("key").isNotEmpty() && !it.optBoolean("skipped") }
    val partial = ProposalChoices.partial(rows, choosing)
    val destructive = kind == "delete" && known
    val opened = !finished || cards.opened(id)
    val language = languageKey()

    @Composable
    fun label(action: ProposalAction): String = when (action) {
        ProposalAction.Apply -> when {
            destructive -> countText(R.plurals.assistant_delete_transactions, ProposalCards.deleting(rows, card.optInt("more")))
            partial != null -> stringResource(R.string.assistant_apply_some, "${partial.chosen}", "${partial.total}")
            else -> stringResource(R.string.assistant_apply_changes)
        }
        // Fixes for a bank file being imported: the import applies them, each only to a line that hasn't changed.
        ProposalAction.ApplyToImport -> if (partial != null) stringResource(R.string.assistant_fix_apply_some, "${partial.chosen}", "${partial.total}") else stringResource(R.string.assistant_fix_apply)
        ProposalAction.ApplyBankTexts -> stringResource(when (proposal.optJSONObject("bankTexts")?.optString("action")) {
            "read" -> R.string.bank_texts_apply_read; "add" -> R.string.bank_texts_apply_add
            "remember" -> R.string.bank_texts_apply_remember; else -> R.string.bank_texts_apply_limit
        })
        ProposalAction.AskChanges -> stringResource(R.string.assistant_ask_changes)
        ProposalAction.UseOriginal -> stringResource(R.string.assistant_use_original)
        ProposalAction.Discard -> stringResource(R.string.assistant_discard)
        ProposalAction.Remove -> stringResource(R.string.common_remove)
        ProposalAction.Restore -> stringResource(R.string.common_restore)
        ProposalAction.Undo -> stringResource(R.string.assistant_undo_proposal)
        ProposalAction.AskAgain -> stringResource(R.string.assistant_ask_again)
        ProposalAction.Show -> stringResource(R.string.assistant_show_in_budget)
        ProposalAction.Preview -> stringResource(R.string.assistant_preview)
        ProposalAction.Print -> stringResource(R.string.assistant_print)
        ProposalAction.Excel -> stringResource(R.string.assistant_export_report_tables)
        ProposalAction.Open -> stringResource(R.string.assistant_open_spreadsheet)
        ProposalAction.Save -> stringResource(R.string.assistant_export_excel)
        ProposalAction.ReviewImport -> countText(R.plurals.assistant_review_import_rows, proposal.optInt("rows"))
        ProposalAction.SaveCsv -> stringResource(R.string.assistant_export_csv)
        ProposalAction.Answer -> ""
    }
    fun enabled(action: ProposalAction): Boolean = !busy && when (action) {
        // Nothing chosen: nothing to apply.
        ProposalAction.Apply -> !(choosing && chosen == 0)
        // Only over the import the fixes are for.
        ProposalAction.ApplyToImport -> fixHere && !(choosing && chosen == 0)
        ProposalAction.AskAgain -> canSend
        else -> true
    }

    Card(Modifier.fillMaxWidth().testTag("assistant-card:$id"), shape = RoundedCornerShape(18.dp), colors = CardDefaults.cardColors(containerColor = ur.surface2, contentColor = ur.text),
        // A card that deletes wears the app's colour for what can't be taken lightly.
        border = if (destructive && !finished) BorderStroke(1.dp, ur.negative) else null) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            // The card's words are the assistant's or the budget's, in whatever language: each reads in its own direction.
            val own = LocalTextStyle.current.own
            if (finished) {
                // One line: where the card stands, then its title. A tap opens its rows, as they were.
                val line = remember(proposal, language) { ProposalCards.finishedLine(proposal) }
                Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp))
                    .clickable(onClickLabel = stringResource(if (opened) R.string.assistant_card_close else R.string.assistant_card_open), role = Role.Button) { cards.open(id, !opened) }
                    .testTag("assistant-card-line"), verticalAlignment = Alignment.CenterVertically) {
                    Text(line, Modifier.weight(1f).padding(vertical = 4.dp), style = MaterialTheme.typography.titleSmall.own)
                    Icon(if (opened) Icons.Outlined.KeyboardArrowUp else Icons.Outlined.KeyboardArrowDown, null, tint = ur.muted)
                }
                // What can still be done about it stays on the line: take it back, put it back, go to what it names.
                val onLine = ProposalCards.lineButtons(proposal, undoable)
                if (onLine.isNotEmpty()) FlowRow(Modifier.offset(x = (-12).dp)) {
                    onLine.forEach { action -> TextButton({ act(action) }, enabled = enabled(action)) { Text(label(action)) } }
                }
            }
            if (opened) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(card.text("kicker"), Modifier.weight(1f), style = MaterialTheme.typography.labelMedium.own, color = if (destructive) ur.negative else MaterialTheme.colorScheme.primary)
                    // What sets a thing the assistant made aside is in its menu, not beside its main buttons.
                    if (ProposalAction.Remove in actions) CardMenu(!busy) { act(ProposalAction.Remove) }
                }
                if (!finished) Text(proposal.text("title"), Modifier.fillMaxWidth(), style = MaterialTheme.typography.titleMedium.own, fontWeight = FontWeight.Bold)
                card.rows("stats").forEach { stat -> Text("${stat.text("label")}: ${if (stat.has("amount") && !stat.isNull("amount")) money(stat.optLong("amount"), currency) else stat.text("text")}", Modifier.fillMaxWidth(), style = own) }
                if (kind == "question" && ProposalAction.Answer in actions) {
                    // Each answer is a button: pressing one sends its words as the user's message.
                    rows.forEach { row ->
                        OutlinedButton({ answer?.invoke(row.text("label")) }, Modifier.fillMaxWidth().testTag("assistant-answer"), enabled = !busy && canSend && answer != null) {
                            Text(row.text("label"), Modifier.fillMaxWidth(), style = own)
                        }
                    }
                } else {
                    // With many parts to tick, all of them at once: every one, none, or only those marked to check.
                    if (choosing && rows.count { it.text("key").isNotEmpty() } > 5) FlowRow(Modifier.offset(x = (-12).dp).testTag("assistant-card-select")) {
                        TextButton({ choose?.invoke(ProposalChoices.select(choices, rows) { true }) }, enabled = !busy) { Text(stringResource(R.string.selection_all)) }
                        TextButton({ choose?.invoke(ProposalChoices.select(choices, rows) { false }) }, enabled = !busy) { Text(stringResource(R.string.assistant_select_none)) }
                        if (rows.any { it.text("key").isNotEmpty() && ProposalChoices.toCheck(it) }) {
                            TextButton({ choose?.invoke(ProposalChoices.select(choices, rows, ProposalChoices::toCheck)) }, enabled = !busy) { Text(stringResource(R.string.assistant_select_check)) }
                        }
                    }
                    // Figures show each row's share of the largest amount as a bar.
                    val shares = remember(rows, kind) { if (kind == "figures") ProposalCards.barShares(rows) else null }
                    val pages = ((rows.size + 19) / 20).coerceAtLeast(1)
                    val page = cards.page(id).coerceIn(0, pages - 1)
                    for (at in page * 20 until minOf(rows.size, (page + 1) * 20)) {
                        val row = rows[at]
                        val key = row.text("key")
                        val edit = row.optJSONObject("edit")
                        val editable = !row.optBoolean("skipped") && (edit?.optBoolean("category") == true || edit?.optBoolean("amount") == true)
                        ProposalRow(row, currency, busy,
                            toggle = if (choosing && key.isNotEmpty()) ({ choose?.invoke(ProposalChoices.toggle(choices, key, row.optBoolean("skipped"))) }) else null,
                            change = if (choosing && key.isNotEmpty() && editable && change != null) ({ change(row) }) else null, bar = shares?.get(at))
                    }
                    card.optJSONObject("total")?.let { ProposalRow(it, currency, busy) }
                    if (pages > 1) Row {
                        TextButton({ cards.page(id, page - 1) }, enabled = page > 0) { Text(stringResource(R.string.assistant_previous_rows)) }
                        Text(stringResource(R.string.assistant_page, "${page + 1}", "$pages"), Modifier.padding(12.dp))
                        TextButton({ cards.page(id, page + 1) }, enabled = page + 1 < pages) { Text(stringResource(R.string.assistant_next_rows)) }
                    }
                }
                if (card.optInt("more") > 0) Text(countText(R.plurals.assistant_more_rows, card.optInt("more")), style = MaterialTheme.typography.labelSmall)
                card.optJSONArray("check")?.let { Text(if (it.optBoolean(0)) "✓ " + it.optString(1) else stringResource(R.string.assistant_check, it.optString(1)), color = if (it.optBoolean(0)) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error) }
                // The warning that's part of what the card proposes, apart from what the card says about itself.
                if (card.text("warning").isNotEmpty()) CardStrip(card.text("warning"), "warn")
                if (card.text("note").isNotEmpty()) Text(card.text("note"), Modifier.fillMaxWidth(), style = MaterialTheme.typography.bodySmall.own)
            }
            // What the card has to say about itself, in the one place for it. Folded, only what still matters: news of a card that's done with is past.
            ProposalCards.message(proposal)?.takeIf { (tone, _) -> opened || tone != "info" }?.let { (tone, text) -> CardStrip(text, tone) }
            if (!known) Text(stringResource(R.string.assistant_update_app), Modifier.testTag("assistant-card-update"), style = MaterialTheme.typography.bodySmall, color = ur.text2)
            if (opened && buttons.isNotEmpty()) {
                val main = buttons.first()
                Button({ act(main) }, Modifier.testTag("assistant-card-main"), enabled = enabled(main),
                    colors = if (destructive && main == ProposalAction.Apply) ButtonDefaults.buttonColors(containerColor = ur.negative, contentColor = if (ur.dark) ur.bg else Color.White) else ButtonDefaults.buttonColors()) {
                    Text(label(main))
                }
                if (ProposalAction.ApplyToImport in buttons) Text(stringResource(if (fixHere) R.string.assistant_fix_each_line else R.string.assistant_fix_open_import), style = MaterialTheme.typography.bodySmall)
                if (buttons.size > 1) FlowRow(Modifier.offset(x = (-12).dp)) {
                    buttons.drop(1).forEach { action -> TextButton({ act(action) }, enabled = enabled(action)) { Text(label(action)) } }
                }
            }
        }
    }
}

/** A card's ⋯ menu: Remove, which sets a thing the assistant made aside. */
@Composable
private fun CardMenu(enabled: Boolean, remove: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton({ open = true }, Modifier.testTag("assistant-card-menu"), enabled = enabled) {
            Icon(Icons.Outlined.MoreHoriz, stringResource(R.string.common_more_actions), tint = ur.muted)
        }
        DropdownMenu(open, { open = false }, containerColor = ur.surface) {
            DropdownMenuItem(text = { Text(stringResource(R.string.common_remove)) }, onClick = { open = false; remove() }, leadingIcon = { Icon(Icons.Outlined.DeleteOutline, null) })
        }
    }
}

/**
 * What a card says, in one strip: quiet for news of it ("info"), the app's warning for something
 * to check ("warn"), its error for what can't be done ("error").
 */
@Composable
private fun CardStrip(text: String, tone: String) {
    val fill = when (tone) { "error" -> ur.negativeSoft; "warn" -> ur.warningSoft; else -> ur.surface }
    val ink = when (tone) { "error" -> ur.negative; "warn" -> ur.warning; else -> ur.muted }
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(fill).padding(horizontal = 10.dp, vertical = 8.dp).testTag("assistant-card-message:$tone"),
        horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Icon(when (tone) { "error" -> Icons.Outlined.ErrorOutline; "warn" -> Icons.Outlined.WarningAmber; else -> Icons.Outlined.Info }, null, Modifier.size(18.dp), tint = ink)
        Text(text, Modifier.weight(1f), style = MaterialTheme.typography.bodySmall.own, color = if (tone == "info") ur.text2 else ur.text)
    }
}

/**
 * A card row. While the card waits, its checkbox (`toggle`) leaves it out or puts it back, and its
 * pencil (`change`) gives it another category or amount. A row left out stays, dimmed and struck
 * through; an amount the user gave shows in the accent colour, and so does a category just given
 * (`given`) until the core's card says it. On a figures card [bar] is the row's share of the
 * card's largest amount, from 0 to 1.
 */
@Composable
private fun ProposalRow(row: JSONObject, currency: String, busy: Boolean, toggle: (() -> Unit)? = null, change: (() -> Unit)? = null, bar: Float? = null) {
    val skipped = row.optBoolean("skipped")
    val label = row.text("label")
    val applyLabel = stringResource(R.string.assistant_apply_row, label)
    Row(Modifier.padding(start = if (row.optBoolean("indent")) 16.dp else 0.dp).alpha(if (skipped) .5f else 1f), verticalAlignment = Alignment.CenterVertically) {
        if (toggle != null) Checkbox(!skipped, { toggle() }, Modifier.semantics { contentDescription = applyLabel }, enabled = !busy)
        Column(Modifier.weight(1f)) {
            Row {
                Text(label, Modifier.weight(1f), style = LocalTextStyle.current.own, textDecoration = if (skipped) TextDecoration.LineThrough else null)
                if (row.has("amount")) Text(money(row.optLong("amount"), currency), fontWeight = FontWeight.SemiBold,
                    color = if (row.optBoolean("edited")) MaterialTheme.colorScheme.primary else Color.Unspecified)
            }
            if (row.text("detail").isNotEmpty()) Text(row.text("detail"), Modifier.fillMaxWidth(), style = MaterialTheme.typography.bodySmall.own)
            if (bar != null) Box(Modifier.padding(vertical = 4.dp).fillMaxWidth().height(6.dp).clip(RoundedCornerShape(3.dp)).background(ur.border).testTag("assistant-figure-bar")) {
                Box(Modifier.fillMaxWidth(bar).fillMaxHeight().background(ur.accent))
            }
            if (row.has("before") || row.has("after")) Text(stringResource(R.string.change_from_to, money(row.optLong("before"), currency), money(row.optLong("after"), currency)), style = MaterialTheme.typography.bodySmall)
            if (row.text("given").isNotEmpty()) Text(row.text("given"), style = MaterialTheme.typography.labelSmall.own, color = MaterialTheme.colorScheme.primary)
            if (row.text("badge").isNotEmpty()) Text(row.text("badge"), style = MaterialTheme.typography.labelSmall.own, color = if (row.text("tone") == "warn") MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
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
        // A report reads at the pages' readable width, however wide the window.
        Surface(Modifier.widthIn(max = ReadableWidth).fillMaxSize().padding(12.dp), shape = MaterialTheme.shapes.large) {
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

/**
 * Reports a reply: an excerpt of it, what went wrong, and a line of details about the app and the
 * service (`assistantReportDetails`) that the user can change or clear. The details go at the end
 * of the reason, cut to fit; the user's own words never are.
 */
@Composable
private fun AssistantReplyReportDialog(entry: JSONObject, status: JSONObject?, controller: AssistantController, close: () -> Unit) {
    val state by controller.state.collectAsStateWithLifecycle()
    var text by remember { mutableStateOf(entry.text("text").take(4000)) }
    var reason by remember { mutableStateOf("") }
    val language = stringResource(R.string.language_code)
    var details by remember {
        val tools = entry.optJSONArray("tools")?.let { list -> (0 until list.length()).map(list::optString) }.orEmpty()
        mutableStateOf(assistantReportDetails(BuildConfig.VERSION_NAME, status?.text("provider").orEmpty(), status?.text("model"), tools, language))
    }
    FormDialog(stringResource(R.string.assistant_report_a_reply), close, stringResource(R.string.assistant_send_report),
        { controller.reportReply(text, assistantReportReason(reason, details), close) }, text.isNotBlank() && "reportReply" !in state.busy) {
        Text(stringResource(R.string.assistant_report_note))
        Field(text, { text = it.take(4000) }, stringResource(R.string.assistant_excerpt), lines = 8)
        Field(reason, { reason = it.take(1000) }, stringResource(R.string.assistant_what_went_wrong), lines = 4)
        Field(details, { details = it.take(1000) }, stringResource(R.string.assistant_report_details), lines = 3, ltr = true, modifier = Modifier.testTag("assistant-report-details"))
        Text(stringResource(R.string.assistant_report_details_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
}

/**
 * The conversation as the panel lists it: steps in a row together, everything else on its own,
 * each with its first index. An entry of a kind this app doesn't know is left out, rather than
 * drawn as an empty reply.
 */
internal fun assistantRows(entries: List<JSONObject>): List<Pair<Int, List<JSONObject>>> {
    val rows = mutableListOf<Pair<Int, MutableList<JSONObject>>>()
    entries.forEachIndexed { index, entry ->
        val kind = AssistantEntryKind.of(entry) ?: return@forEachIndexed
        val last = rows.lastOrNull()
        if (kind == AssistantEntryKind.Step && last != null && last.second.first().text("kind") == "step") last.second += entry
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
        // Tall enough to tap, though its words are small.
        if (open || steps.size > shown.size) Box(Modifier.padding(start = 22.dp).heightIn(min = 48.dp).clickable(role = Role.Button) { open = !open }.testTag("assistant-steps-toggle"),
            contentAlignment = Alignment.CenterStart) {
            Text(if (open) stringResource(R.string.assistant_steps_fewer) else countText(R.plurals.assistant_steps_more, steps.size - shown.size),
                style = MaterialTheme.typography.labelLarge, color = ur.accent)
        }
    }
}
