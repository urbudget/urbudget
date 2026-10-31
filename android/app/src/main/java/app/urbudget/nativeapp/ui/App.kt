package app.urbudget.nativeapp.ui

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.*
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import app.urbudget.nativeapp.AssistantController
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.NativePreferences
import app.urbudget.nativeapp.BudgetUiState
import app.urbudget.nativeapp.R
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.fixImportText
import app.urbudget.nativeapp.fixOutcome
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.time.LocalDate
import java.util.Currency
import java.util.Locale

@Composable
fun UrBudgetApp(vm: BudgetViewModel = viewModel()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val preferences = remember { UiPreferences(NativePreferences(context.applicationContext)) }
    // Every form, picker and report belongs to one library entry, even when two budgets are identical.
    CompositionLocalProvider(LocalPreferences provides preferences) { key(state.budgetIdentity) { BudgetContent(vm, state) } }
}

/** Shows `message` above the tab bar, with Undo for the last change when asked. */
private fun SnackbarHostState.toast(scope: kotlinx.coroutines.CoroutineScope, vm: BudgetViewModel, message: String, undo: Boolean, undoLabel: String) {
    scope.launch {
        currentSnackbarData?.dismiss()
        if (showSnackbar(message, actionLabel = if (undo) undoLabel else null, duration = SnackbarDuration.Short) == SnackbarResult.ActionPerformed) vm.undo()
    }
}

/** Budget, Transactions, Accounts and Reports are tabs; More opens a drawer of everything else. */
private val tabs = listOf(R.string.tab_budget to Icons.Outlined.GridView, R.string.tab_transactions to Icons.Outlined.SwapHoriz,
    R.string.tab_accounts to Icons.Outlined.AccountBalance, R.string.tab_reports to Icons.Outlined.BarChart, R.string.tab_more to Icons.Outlined.Menu)

/**
 * The pages More leads to, with what each is for. A page is known by its English `key`, which the
 * navigation state keeps; people see its title in their language.
 */
internal data class MorePage(val key: String, val title: Int, val detail: Int, val icon: ImageVector)
internal val morePages = listOf(
    MorePage("Scheduled", R.string.page_scheduled, R.string.page_scheduled_detail, Icons.Outlined.EventRepeat),
    MorePage("Loan planner", R.string.page_loans, R.string.page_loans_detail, Icons.Outlined.Calculate),
    MorePage("Payees & rules", R.string.page_payees, R.string.page_payees_detail, Icons.Outlined.People),
    MorePage("Bank connections", R.string.page_banks, R.string.page_banks_detail, Icons.Outlined.AccountBalance),
    MorePage("Bank messages", R.string.sms_title, R.string.sms_detail, Icons.Outlined.Sms),
    MorePage("Account & sync", R.string.page_account_sync, R.string.page_account_sync_detail, Icons.Outlined.CloudSync),
    MorePage("Budget library", R.string.page_library, R.string.page_library_detail, Icons.Outlined.FolderOpen),
    MorePage("Settings & data", R.string.page_settings, R.string.page_settings_detail, Icons.Outlined.Settings),
)

/** A page's title in the interface's language, from its key. */
@Composable
internal fun pageTitle(page: String): String = morePages.firstOrNull { it.key == page }?.let { stringResource(it.title) }
    ?: if (page == "Assistant") stringResource(R.string.assistant_title) else page

@Composable
private fun BudgetContent(vm: BudgetViewModel, state: BudgetUiState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val startPage = LocalPreferences.current.startPage
    var tab by rememberSaveable { mutableStateOf(if (startPage == "transactions") 1 else 0) }
    var lastTab by rememberSaveable { mutableStateOf(0) }
    var morePage by rememberSaveable { mutableStateOf("") }
    // A page opened from another page (Settings › Budget library) goes back to it.
    var parentPage by rememberSaveable { mutableStateOf("") }
    // The tab a filtered register was opened from, so Back returns there.
    var returnTab by rememberSaveable { mutableStateOf(-1) }
    var drawer by remember { mutableStateOf(false) }
    var assistant by rememberSaveable { mutableStateOf(false) }
    val assistantController: AssistantController = viewModel()
    var editor by remember { mutableStateOf<String?>(null) }
    var selected by remember { mutableStateOf<JSONObject?>(null) }
    // A bank file being imported: kept while the assistant is open over it, to fix its lines.
    var bankImport by remember { mutableStateOf<BankImportDraft?>(null) }
    var accountFilter by rememberSaveable { mutableStateOf("") }
    var categoryFilter by rememberSaveable { mutableStateOf("") }
    // A date range the register opens with, when a budget figure leads to one month's transactions.
    var rangeFrom by rememberSaveable { mutableStateOf("") }
    var rangeThrough by rememberSaveable { mutableStateOf("") }
    var rangeSearch by rememberSaveable { mutableStateOf("") }
    var dockHidden by remember { mutableStateOf(false) }
    var addExpanded by remember { mutableStateOf(true) }
    var fileError by remember { mutableStateOf<String?>(null) }
    var pendingImport by remember { mutableStateOf<String?>(null) }
    var confirmRestore by remember { mutableStateOf(false) }
    var pendingCreate by remember { mutableStateOf<Triple<String, String, Boolean>?>(null) }
    val snack = remember { SnackbarHostState() }
    // Each tab keeps its place (scroll position, the report open) while another is shown; a filtered
    // register keeps its own, so opening a new filter starts fresh.
    val pageStates = rememberSaveableStateHolder()
    val openFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        val output = java.io.ByteArrayOutputStream()
                        val buffer = ByteArray(8192)
                        while (true) {
                            val count = input.read(buffer)
                            if (count < 0) break
                            require(output.size() + count <= 32 * 1024 * 1024) { context.getString(R.string.backup_too_large) }
                            output.write(buffer, 0, count)
                        }
                        output.toString("UTF-8")
                    } ?: error(context.getString(R.string.error_file_not_opened))
                }
            }.onSuccess { json ->
                if (vm.exportBudget() == null) vm.importBudget(json) else pendingImport = json
            }.onFailure { fileError = it.message ?: context.getString(R.string.backup_open_failed) }
        }
    }
    val saveFile = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) {
            val json = vm.exportBudget()
            if (json != null) scope.launch {
                runCatching {
                    withContext(Dispatchers.IO) { context.contentResolver.openOutputStream(uri)?.use { it.write(json.toByteArray(Charsets.UTF_8)) } ?: error(context.getString(R.string.backup_not_saved)) }
                }.onSuccess { snack.showSnackbar(context.getString(R.string.backup_exported)) }.onFailure { fileError = it.message ?: context.getString(R.string.backup_save_failed) }
            }
        }
    }
    fun openEditor(kind: String, record: JSONObject? = null) {
        selected = record; editor = kind
        if (kind == "bankImport" && bankImport == null) state.budget?.let { bankImport = newBankImport(it) }
    }
    // The import the assistant is open over: it's told the file's lines, and its fixes apply there.
    val importing = bankImport?.takeIf { editor == "bankImport" }
    val importScreen = remember(importing) {
        importing?.session?.let { JSONObject().put("account", importing.account).put("content", importing.content).put("session", it) }
    }
    fun applyFix(thread: String, proposal: JSONObject) {
        val open = importing
        val fixes = proposal.optJSONObject("fixes")
        val session = open?.session
        if (open == null || fixes == null || session == null) return assistantController.note(context.getString(R.string.assistant_fix_open_import))
        if (fixes.optJSONObject("scope")?.optString("session") != open.sessionId) return assistantController.note(context.getString(R.string.assistant_fix_other_import))
        vm.query("bankApplyFixes", JSONObject().put("accountId", open.account).put("content", open.content).put("session", session).put("fixes", fixes),
            onError = { assistantController.note(it) }, anyRevision = true) { applied ->
            val outcome = fixOutcome(applied)
            bankImport = bankImport?.let {
                if (it.sessionId == open.sessionId) it.copy(session = applied.getJSONObject("session"), view = "lines", assisted = it.assisted + outcome.lines, notice = outcome.notice) else it
            }
            if (outcome.applied > 0) assistantController.imported(thread, proposal.text("id"))
            // Back to the import, to see them.
            assistant = false
        }
    }
    fun openTab(index: Int) {
        if (index == 4) { drawer = true; return }
        tab = index; morePage = ""; parentPage = ""; accountFilter = ""; categoryFilter = ""; rangeFrom = ""; rangeThrough = ""; rangeSearch = ""; returnTab = -1
        dockHidden = false; addExpanded = true
    }
    fun openPage(page: String) {
        drawer = false
        if (page == "Assistant") { assistant = true; return }
        if (tab != 4) lastTab = tab
        tab = 4; morePage = page; parentPage = ""; dockHidden = false
    }
    fun openChildPage(page: String) {
        if (page == "Assistant") { assistant = true; return }
        parentPage = morePage; morePage = page
    }
    fun closePage() {
        if (parentPage.isNotEmpty()) { morePage = parentPage; parentPage = "" } else { morePage = ""; tab = lastTab }
    }
    fun openRegister(target: RegisterTarget, from: Int) {
        categoryFilter = target.category; accountFilter = target.account; rangeFrom = target.from; rangeThrough = target.through; rangeSearch = target.search
        returnTab = from; tab = 1
    }
    fun closeRegister() {
        val back = returnTab
        categoryFilter = ""; accountFilter = ""; rangeFrom = ""; rangeThrough = ""; rangeSearch = ""; returnTab = -1; tab = back
    }
    val importFile = { openFile.launch(arrayOf("application/json", "text/plain", "application/octet-stream")) }
    val exportFile = { saveFile.launch("UrBudget-${LocalDate.now()}.json") }
    // Back closes what's open, newest first; pages register their own handlers (search, selection)
    // after this one, so theirs come first. Then an account goes back to Accounts, a page to the first tab.
    BackHandler(editor == null && (assistant || morePage.isNotEmpty() || tab != 0 || accountFilter.isNotEmpty() || categoryFilter.isNotEmpty())) {
        when {
            assistant -> assistant = false
            morePage.isNotEmpty() -> closePage()
            tab == 1 && returnTab >= 0 -> closeRegister()
            tab == 1 && accountFilter.isNotEmpty() -> { accountFilter = ""; tab = 2 }
            tab == 1 && categoryFilter.isNotEmpty() -> { categoryFilter = ""; tab = 0 }
            else -> openTab(0)
        }
    }
    // The add button shrinks to its plus while a list scrolls down, and grows back on the way up.
    val scrollWatcher = remember {
        object : NestedScrollConnection {
            override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset {
                if (available.y < -6f) addExpanded = false else if (available.y > 6f) addExpanded = true
                return Offset.Zero
            }
        }
    }
    // Captures only stable holders, so it stays the same lambda across state emissions.
    val common: @Composable RowScope.() -> Unit = {
        RoundIconButton(Icons.AutoMirrored.Outlined.Chat, stringResource(R.string.assistant_title), { assistant = true })
        OverflowMenu(vm) { openPage("Settings & data") }
    }
    val budget = state.budget
    val undoLabel = stringResource(R.string.common_undo)
    val toast = remember(snack, scope, vm, undoLabel) { { message: String, undo: Boolean -> snack.toast(scope, vm, message, undo, undoLabel) } }
    CompositionLocalProvider(LocalToast provides toast) {
    Box(Modifier.fillMaxSize().background(ur.bg).nestedScroll(scrollWatcher)) {
        when {
            state.loading -> CircularProgressIndicator(Modifier.align(Alignment.Center), color = ur.accent)
            morePage == "Account & sync" && budget == null -> Column(Modifier.fillMaxSize()) {
                PhoneTopBar(pageTitle("Account & sync"), onBack = { morePage = ""; tab = 0 })
                Box(Modifier.weight(1f)) { CloudScreen(vm) }
            }
            budget == null -> WelcomeScreen(state.busy, { name, currency, sample ->
                if (vm.exportBudget() != null) pendingCreate = Triple(name, currency, sample) else vm.createBudget(name, currency, sample)
            }, importFile, if (vm.exportBudget() != null) exportFile else null, if (state.recoveryAvailable) ({ confirmRestore = true }) else null,
                { tab = 4; morePage = "Account & sync" })
            tab == 4 && morePage.isNotEmpty() -> pageStates.SaveableStateProvider("page:$morePage") { Column(Modifier.fillMaxSize()) {
                PhoneTopBar(pageTitle(morePage), budget.text("name"), onBack = ::closePage, actions = common)
                Box(Modifier.weight(1f)) {
                    when (morePage) {
                        "Account & sync" -> CloudScreen(vm)
                        "Bank connections" -> BankConnectionsScreen(budget, vm)
                        "Bank messages" -> SmsImportScreen(budget, vm)
                        "Budget library" -> LibraryScreen(budget, vm, importFile, exportFile)
                        else -> MoreScreen(morePage, ::openChildPage, budget, vm, importFile, exportFile,
                            if (state.recoveryAvailable) ({ confirmRestore = true }) else null) { kind, item -> openEditor(kind, item) }
                    }
                }
            } }
            else -> pageStates.SaveableStateProvider(if (tab == 1) "register:$accountFilter|$categoryFilter|$rangeFrom|$rangeThrough|$rangeSearch" else "tab:$tab") { when (tab) {
                0 -> BudgetScreen(budget, state.summary, state.month, vm::selectMonth, vm, common,
                    onTransactions = { target -> openRegister(target, 0) },
                    onPayCard = { card -> openEditor("cardPayment", JSONObject().put("id", card)) })
                1 -> TransactionsScreen(budget, state.summary, accountFilter, categoryFilter,
                    onBack = when {
                        returnTab >= 0 -> ({ closeRegister() })
                        accountFilter.isNotEmpty() -> ({ accountFilter = ""; tab = 2 })
                        categoryFilter.isNotEmpty() -> ({ categoryFilter = ""; tab = 0 })
                        else -> null
                    }, initialFrom = rangeFrom, initialThrough = rangeThrough, initialSearch = rangeSearch, payCard = { card -> openEditor("cardPayment", JSONObject().put("id", card)) }, setCategory = { categoryFilter = it }, edit = { openEditor("transaction", it) }, add = { openEditor("transaction") },
                    editAccount = { openEditor("account", it) }, reconcile = { openEditor("reconcile", it) },
                    editSchedule = { openEditor("schedule", it) }, vm = vm, bar = common, setDockHidden = { dockHidden = it },
                    openAccount = { id -> accountFilter = id; categoryFilter = ""; rangeFrom = ""; rangeThrough = "" })
                2 -> AccountsScreen(budget, state.summary, open = { account -> accountFilter = account.text("id"); categoryFilter = ""; tab = 1 },
                    edit = { openEditor("account", it) }, reconcile = { openEditor("reconcile", it) }, add = { openEditor("account") },
                    vm = vm, bar = common)
                else -> ReportsScreen(budget, state.reports, state.summary, state.month, vm::selectMonth, vm, common,
                    openRegister = { target -> openRegister(target, 3) }) { openEditor("transaction", it) }
            } }
        }
        BusyBar(vm, Modifier.align(Alignment.TopCenter))
        if (budget != null) {
            val showAdd = tab in 0..2 && morePage.isEmpty() && !dockHidden && !state.loading
            AnimatedVisibility(showAdd, Modifier.align(Alignment.BottomEnd).navigationBarsPadding().padding(end = 16.dp, bottom = 92.dp),
                enter = fadeIn() + scaleIn(initialScale = .8f), exit = fadeOut() + scaleOut(targetScale = .8f)) {
                AddButton(addExpanded) { openEditor("transaction") }
            }
            AnimatedVisibility(!dockHidden, Modifier.align(Alignment.BottomCenter), enter = slideInVertically { it } + fadeIn(), exit = slideOutVertically { it } + fadeOut()) {
                TabDock(if (tab == 4) 4 else tab, ::openTab)
            }
        }
        SnackbarHost(snack, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 96.dp)) { data ->
            Snackbar(data, Modifier.padding(horizontal = 12.dp), shape = RoundedCornerShape(16.dp),
                containerColor = if (ur.dark) androidx.compose.ui.graphics.Color(0xFF262C3E) else androidx.compose.ui.graphics.Color(0xFF1D1D1F),
                contentColor = if (ur.dark) ur.text else androidx.compose.ui.graphics.Color(0xFFF5F5F7), actionColor = if (ur.dark) ur.accent else androidx.compose.ui.graphics.Color(0xFF78A7FF))
        }
        AnimatedVisibility(assistant && budget != null, enter = fadeIn(tween(180)), exit = fadeOut(tween(180))) {
            Box(Modifier.fillMaxSize().background(ur.scrim).clickable(remember { MutableInteractionSource() }, null) { assistant = false })
        }
        AnimatedVisibility(assistant && budget != null, Modifier.align(Alignment.BottomCenter),
            enter = slideInVertically(tween(260)) { it }, exit = slideOutVertically(tween(220)) { it }) {
            Column(Modifier.fillMaxWidth().fillMaxHeight(.94f).clip(RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)).background(ur.surface)
                .then(if (ur.dark) Modifier.border(1.dp, ur.border, RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)) else Modifier)
                .clickable(remember { MutableInteractionSource() }, null) {}) {
                Box(Modifier.fillMaxWidth().padding(top = 8.dp), contentAlignment = Alignment.Center) {
                    Box(Modifier.size(36.dp, 5.dp).clip(CircleShape).background(ur.borderStrong))
                }
                Box(Modifier.weight(1f).navigationBarsPadding()) { AssistantScreen(vm, onClose = { assistant = false }, importing = importScreen, applyFix = ::applyFix) }
            }
        }
    }
    if (drawer && budget != null) MoreDrawer(budget, { drawer = false }, ::openPage, { openEditor("bankImport"); drawer = false },
        { openEditor("ynabImport"); drawer = false })
    if (budget != null) when (editor) {
        "transaction" -> TransactionDialog(selected, budget, accountFilter, vm, close = { editor = null }, summary = state.summary)
        "cardPayment" -> selected?.let { CardPaymentSheet(it.text("id"), budget, state.summary, vm) { editor = null } }
        "account" -> AccountDialog(selected, budget, vm) { editor = null }
        "reconcile" -> selected?.let { ReconcileDialog(it, budget, state.summary, vm) { editor = null } }
        "schedule" -> ScheduleDialog(selected, budget, vm) { editor = null }
        "loan" -> LoanDialog(selected, budget, state.summary, vm) { editor = null }
        "payee" -> PayeeDialog(selected, budget, vm) { editor = null }
        "rule" -> RuleDialog(selected, budget, vm) { editor = null }
        // Hidden while the assistant is open over it. A PDF statement goes to the assistant,
        // attached to its next message.
        "bankImport" -> bankImport?.let { draft ->
            if (!assistant) BankImportDialog(budget, vm, draft, { change -> bankImport = bankImport?.let(change) },
                askAssistant = { uri -> assistantController.attach(context, uri); editor = null; bankImport = null; assistant = true },
                fixWithAssistant = { assistantController.prefill(fixImportText); assistant = true }) { editor = null; bankImport = null }
        }
        "ynabImport" -> YnabImportDialog(budget, vm) { editor = null }
    }
    val error = state.error ?: fileError
    if (error != null) AlertDialog(onDismissRequest = { vm.dismissError(); fileError = null }, title = { Text(stringResource(R.string.error_title)) },
        text = { Text(error) }, confirmButton = { TextButton({ vm.dismissError(); fileError = null }) { Text(stringResource(R.string.common_ok)) } }, containerColor = ur.surface)
    if (pendingImport != null) AlertDialog(onDismissRequest = { pendingImport = null }, title = { Text(stringResource(R.string.import_replace_title)) },
        text = { Text(stringResource(R.string.import_replace_message)) },
        confirmButton = { TextButton({ vm.importBudget(pendingImport!!); pendingImport = null }) { Text(stringResource(R.string.import_backup_action)) } },
        dismissButton = { TextButton({ pendingImport = null }) { Text(stringResource(R.string.common_cancel)) } }, containerColor = ur.surface)
    if (confirmRestore) AlertDialog(onDismissRequest = { confirmRestore = false }, title = { Text(stringResource(R.string.restore_previous_title)) },
        text = { Text(stringResource(R.string.restore_previous_message)) },
        confirmButton = { TextButton({ vm.restorePreviousBudget(); confirmRestore = false }) { Text(stringResource(R.string.common_restore)) } }, dismissButton = { TextButton({ confirmRestore = false }) { Text(stringResource(R.string.common_cancel)) } },
        containerColor = ur.surface)
    if (pendingCreate != null) AlertDialog(onDismissRequest = { pendingCreate = null }, title = { Text(stringResource(R.string.create_replace_title)) },
        text = { Text(stringResource(R.string.create_replace_message)) },
        confirmButton = { TextButton({ pendingCreate?.let { vm.createBudget(it.first, it.second, it.third) }; pendingCreate = null }) { Text(stringResource(R.string.create_budget_action)) } },
        dismissButton = { TextButton({ pendingCreate = null }) { Text(stringResource(R.string.common_cancel)) } }, containerColor = ur.surface)
    }
}

/** The tab bar floats over the bottom of the page as frosted glass, and the lists scroll on under it. */
@Composable
private fun TabDock(tab: Int, onTab: (Int) -> Unit) {
    val shape = RoundedCornerShape(32.dp)
    Row(Modifier.navigationBarsPadding().padding(start = 12.dp, end = 12.dp, bottom = 10.dp).fillMaxWidth().height(66.dp)
        .floating(shape, ur.dark, ur.border).clip(shape).background(ur.glass).border(0.5.dp, ur.border, shape).padding(5.dp)) {
        tabs.forEachIndexed { index, (labelId, icon) ->
            val active = index == tab
            val label = stringResource(labelId)
            Column(Modifier.weight(if (index == 1) 1.35f else 1f).fillMaxHeight().clip(RoundedCornerShape(28.dp))
                .background(if (active) ur.tabActive else androidx.compose.ui.graphics.Color.Transparent)
                .clickable(role = Role.Tab, onClickLabel = label) { onTab(index) },
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                Icon(icon, null, Modifier.size(22.dp), tint = if (active) ur.accent else ur.text)
                Text(label, fontSize = 11.5.sp, fontWeight = FontWeight.SemiBold, color = if (active) ur.accent else ur.text, maxLines = 1)
            }
        }
    }
}

/** "+ Transaction" floats above the tab bar, and shrinks to its plus while a list scrolls down. */
@Composable
private fun AddButton(expanded: Boolean, onClick: () -> Unit) {
    val shape = RoundedCornerShape(26.dp)
    Row(Modifier.height(52.dp).floating(shape, ur.dark, ur.borderStrong).clip(shape).background(ur.glass).border(0.5.dp, ur.border, shape)
        .clickable(role = Role.Button, onClickLabel = stringResource(R.string.transaction_add), onClick = onClick).animateContentSize()
        .padding(horizontal = if (expanded) 20.dp else 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Outlined.Add, stringResource(R.string.transaction_add), Modifier.size(24.dp), tint = ur.text)
        if (expanded) { Spacer(Modifier.width(8.dp)); Text(stringResource(R.string.transaction_add_short), fontSize = 17.sp, fontWeight = FontWeight.Bold, color = ur.text) }
    }
}

/** A thin bar while an edit is being saved; it reads the busy flag itself. */
@Composable
private fun BusyBar(vm: BudgetViewModel, modifier: Modifier) {
    val flow = remember(vm) { vm.state.map { it.busy }.distinctUntilChanged() }
    val busy by flow.collectAsStateWithLifecycle(remember(vm) { vm.state.value.busy })
    if (busy) LinearProgressIndicator(modifier.fillMaxWidth().statusBarsPadding().height(2.dp), color = ur.accent, trackColor = ur.bg)
}

/** The ⋯ menu: undo, redo and Settings. It follows only the undo, redo and busy flags. */
@Composable
private fun OverflowMenu(vm: BudgetViewModel, settings: () -> Unit) {
    val state by remember(vm) { vm.state.map { Triple(it.undoAvailable, it.redoAvailable, it.busy) }.distinctUntilChanged() }
        .collectAsStateWithLifecycle(Triple(false, false, false))
    val (undoAvailable, redoAvailable, busy) = state
    var open by remember { mutableStateOf(false) }
    Box {
        RoundIconButton(Icons.Outlined.MoreHoriz, stringResource(R.string.common_more_actions), { open = true })
        DropdownMenu(open, { open = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
            DropdownMenuItem(text = { Text(stringResource(R.string.common_undo)) }, leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Undo, null) }, enabled = undoAvailable && !busy,
                onClick = { vm.undo(); open = false })
            DropdownMenuItem(text = { Text(stringResource(R.string.common_redo)) }, leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Redo, null) }, enabled = redoAvailable && !busy,
                onClick = { vm.redo(); open = false })
            HorizontalDivider(color = ur.border)
            DropdownMenuItem(text = { Text(stringResource(R.string.page_settings)) }, leadingIcon = { Icon(Icons.Outlined.Settings, null) }, onClick = { settings(); open = false })
        }
    }
}

/** More: the budget, then every page the tabs don't cover, as a floating panel. */
@Composable
private fun MoreDrawer(budget: JSONObject, close: () -> Unit, open: (String) -> Unit, bankImport: () -> Unit, ynabImport: () -> Unit) {
    UrSheet(close) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = Gutter).padding(bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(ur.surface2).clickable { open("Budget library") }.padding(14.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(44.dp).clip(RoundedCornerShape(12.dp)).background(ur.accent), contentAlignment = Alignment.Center) {
                    Text(budget.text("name").filter { it.isLetterOrDigit() }.take(2).uppercase().ifEmpty { "UB" }, color = if (ur.dark) ur.bg else androidx.compose.ui.graphics.Color.White,
                        fontWeight = FontWeight.Bold, fontSize = 16.sp)
                }
                Column(Modifier.weight(1f).padding(start = 12.dp)) {
                    Text(budget.text("name"), style = MaterialTheme.typography.titleMedium, color = ur.text)
                    Text(stringResource(R.string.drawer_budget_detail, budget.text("currency")), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                }
                Icon(Icons.Outlined.UnfoldMore, null, tint = ur.muted)
            }
            ListCard {
                morePages.filter { it.key != "Budget library" && it.key != "Settings & data" }.forEachIndexed { index, page ->
                    ListRow(stringResource(page.title), subtitle = stringResource(page.detail), icon = page.icon, onClick = { open(page.key) }, divider = true)
                }
                ListRow(stringResource(R.string.assistant_title), subtitle = stringResource(R.string.drawer_assistant_detail), icon = Icons.AutoMirrored.Outlined.Chat, onClick = { open("Assistant") })
            }
            ListCard {
                ListRow(stringResource(R.string.import_bank_file), subtitle = stringResource(R.string.import_bank_file_detail), icon = Icons.Outlined.FileOpen, onClick = bankImport, divider = true)
                ListRow(stringResource(R.string.import_ynab), subtitle = stringResource(R.string.import_ynab_detail), icon = Icons.Outlined.FileDownload, onClick = ynabImport)
            }
            ListCard { ListRow(stringResource(R.string.page_settings), icon = Icons.Outlined.Settings, iconTint = ur.text2, iconFill = ur.zeroSoft, onClick = { open("Settings & data") }) }
        }
    }
}

@Composable
private fun WelcomeScreen(busy: Boolean, create: (String, String, Boolean) -> Unit, importFile: () -> Unit, exportFile: (() -> Unit)?, restore: (() -> Unit)?, signIn: () -> Unit) {
    // A new budget's name starts in the language in use; after that it's the person's data.
    val defaultName = stringResource(R.string.welcome_default_name)
    val sampleName = stringResource(R.string.welcome_sample_name)
    var name by rememberSaveable { mutableStateOf(defaultName) }
    // The device's region suggests the currency, whatever language the app is in.
    var currency by rememberSaveable { mutableStateOf(runCatching { Currency.getInstance(android.content.res.Resources.getSystem().configuration.locales[0]).currencyCode }.getOrDefault("SAR")) }
    Column(Modifier.fillMaxSize().background(ur.bg).verticalScroll(rememberScrollState()).statusBarsPadding().navigationBarsPadding().padding(horizontal = 24.dp, vertical = 28.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp)) {
        Spacer(Modifier.height(24.dp))
        Box(Modifier.size(64.dp).clip(RoundedCornerShape(18.dp)).background(androidx.compose.ui.graphics.Color(0xFF6D4AE8)), contentAlignment = Alignment.Center) {
            Text("Ur", color = androidx.compose.ui.graphics.Color.White, fontWeight = FontWeight.ExtraBold, fontSize = 26.sp)
        }
        Spacer(Modifier.height(8.dp))
        Text(stringResource(R.string.welcome_title), style = MaterialTheme.typography.displaySmall, color = ur.text)
        Text(stringResource(R.string.welcome_message),
            style = MaterialTheme.typography.bodyLarge, color = ur.text2)
        Spacer(Modifier.height(8.dp))
        ListCard {
            Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Field(name, { name = it }, stringResource(R.string.budget_name))
                Choice(stringResource(R.string.budget_currency), currency, (listOf(currency, "SAR", "USD", "EUR", "GBP", "CAD", "AUD", "AED", "INR").distinct()).map { it to it }, { currency = it })
                PrimaryButton(stringResource(R.string.welcome_create), { create(name, currency, false) }, Modifier.fillMaxWidth(), enabled = !busy && name.isNotBlank())
            }
        }
        ListCard {
            ListRow(stringResource(R.string.welcome_sample), subtitle = stringResource(R.string.welcome_sample_detail), icon = Icons.Outlined.AutoAwesome, onClick = { if (!busy) create(sampleName, currency, true) }, divider = true)
            ListRow(stringResource(R.string.welcome_import), subtitle = stringResource(R.string.welcome_import_detail), icon = Icons.Outlined.FileOpen, onClick = { if (!busy) importFile() }, divider = true)
            ListRow(stringResource(R.string.welcome_sign_in), subtitle = stringResource(R.string.welcome_sign_in_detail), icon = Icons.Outlined.CloudSync, onClick = { if (!busy) signIn() })
        }
        if (exportFile != null || restore != null) ListCard {
            if (exportFile != null) ListRow(stringResource(R.string.welcome_export_recovery), icon = Icons.Outlined.FileUpload, onClick = exportFile, divider = restore != null)
            if (restore != null) ListRow(stringResource(R.string.welcome_restore_previous), icon = Icons.Outlined.Restore, onClick = restore)
        }
    }
}
