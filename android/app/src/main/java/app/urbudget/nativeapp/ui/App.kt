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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import app.urbudget.nativeapp.AccountTypes
import app.urbudget.nativeapp.AssistantController
import app.urbudget.nativeapp.AssistantPlace
import app.urbudget.nativeapp.AssistantShow
import app.urbudget.nativeapp.AssistantStart
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.CardDone
import app.urbudget.nativeapp.NativePreferences
import app.urbudget.nativeapp.NativeServices
import app.urbudget.nativeapp.PageRequests
import app.urbudget.nativeapp.ProposalCards
import app.urbudget.nativeapp.BudgetUiState
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
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
    CompositionLocalProvider(LocalPreferences provides preferences) { ProvideFold { key(state.budgetIdentity) { BudgetContent(vm, state) } } }
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
    MorePage("Payees & rules", R.string.page_payees, R.string.page_payees_detail, Icons.Outlined.People),
    MorePage("Bank connections", R.string.page_banks, R.string.page_banks_detail, Icons.Outlined.AccountBalance),
    MorePage("Bank messages", R.string.sms_title, R.string.sms_detail, Icons.Outlined.Sms),
    MorePage("Account & sync", R.string.page_account_sync, R.string.page_account_sync_detail, Icons.Outlined.CloudSync),
    MorePage("Budget library", R.string.page_library, R.string.page_library_detail, Icons.Outlined.FolderOpen),
    MorePage("Settings & data", R.string.page_settings, R.string.page_settings_detail, Icons.Outlined.Settings),
)

/**
 * "Ask the assistant" on a page: hands the assistant the selected transactions, a category to
 * ask about, or a request in words with the transactions it's about, and opens its panel on
 * them with nothing sent.
 */
internal class AssistantAsk(val transactions: (List<String>) -> Unit, val category: (JSONObject) -> Unit,
                            val request: (text: String, transactions: List<String>) -> Unit)
internal val LocalAssistantAsk = staticCompositionLocalOf<AssistantAsk?> { null }

/**
 * Says that an import added transactions, once the budget has them: [before] and [after] are the
 * budget either side of it, and [fromBank] tells a bank sync from a bank file. With some of them
 * still without a category, the message offers to categorize those with the assistant.
 */
internal val LocalImportSaid = staticCompositionLocalOf<(before: JSONObject?, after: JSONObject?, fromBank: Boolean) -> Unit> { { _, _, _ -> } }

/** Where an assistant card's "Show in the budget" last led, for the page it led to: [key] tells one request from the next. */
internal class Showing(val key: Int, val what: AssistantShow)

/** A message with something to do about it besides Undo: the offer is the snackbar's action. */
private class OfferVisuals(override val message: String, val offer: String) : SnackbarVisuals {
    override val actionLabel: String get() = offer
    override val withDismissAction: Boolean get() = false
    // Long enough to read the offer and reach it.
    override val duration: SnackbarDuration get() = SnackbarDuration.Long
}

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
    // Another budget has its own conversations; one opened clears an error from before it was.
    LaunchedEffect(state.budgetIdentity) { state.budgetIdentity?.let(assistantController::budgetOpened) }
    // The budget changed, by whatever hand (an edit, the ⋯ menu's Undo or Redo, a message's Undo, a sync):
    // the open conversation's cards that the budget moves show where they now stand.
    LaunchedEffect(state.budgetRevision) { assistantController.budgetChanged() }
    // "Ask the assistant" on a page: the panel opens on what's handed over, in a new conversation unless the open one is empty, and nothing is sent.
    val askAssistant = remember(assistantController, context) {
        AssistantAsk(
            transactions = { ids -> assistantController.handOver(selected = ids); assistant = true },
            category = { category ->
                assistantController.handOver(context.getString(R.string.assistant_help_category, category.text("name")), categoryId = category.text("id"))
                assistant = true
            },
            request = { text, ids -> assistantController.handOver(text, selected = ids); assistant = true })
    }
    // Where a card's "Show in the budget" led: the page it led to takes it from here.
    var showing by remember { mutableStateOf<Showing?>(null) }
    // A bank file being imported: kept while the assistant is open over it, to fix its lines. A rotation
    // or the Fold opening no longer remakes the screen, so the review keeps its choices too; this
    // holder still brings the import back if the activity is made again (a new text size, which it
    // doesn't follow in place).
    val imports = viewModel<BankImportHolder>().forBudget(state.budgetIdentity)
    var bankImport by imports::draft
    var editor by remember { mutableStateOf(if (imports.draft != null) "bankImport" else null) }
    var selected by remember { mutableStateOf<JSONObject?>(null) }
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
    // A large screen has the tabs in a rail at its start instead of the dock. With room for both, the
    // register and Reports show their list and detail side by side, and the register's transaction
    // form opens beside it rather than over the screen (Adaptive.kt).
    val fullWide = wideWindow() && state.budget != null
    val fullHinge = if (fullWide) verticalHinge() else null
    // The assistant on a large screen shares it as a second screen: it stands at the window's end (the
    // far side of a hinge) and the app keeps the rest as a screen of its own, both in use at once. A
    // phone-wide side lays the app out as a phone (the dock, sheets from its own foot) (ProvideAppSide).
    val assistantBeside = fullWide && assistant
    val assistantWidth = panelWidth(max = 520.dp)
    val appWidth = when {
        !assistantBeside -> null
        fullHinge != null -> fullHinge.start
        else -> windowWidth() - assistantWidth - PanelMargin * 2 -
            RailInsets.only(WindowInsetsSides.End).asPaddingValues().calculateEndPadding(androidx.compose.ui.platform.LocalLayoutDirection.current)
    }
    val appWindow = appWidth ?: windowWidth()
    val wide = appWindow >= MediumWidth && state.budget != null
    // A hinge splitting the screen top to bottom (a Fold half open, a dual-screen phone): its start side
    // is a phone of its own, with the dock, and a list ends where the hinge runs, with what's chosen
    // from it (or anything opened, floating) on the other side. Without one the tabs stand in a rail.
    val hinge = if (wide && appWidth == null) fullHinge else null
    val rail = wide && hinge == null
    val room = if (rail) appWindow - RailRoom - railStartInset() else appWindow
    val pageStart = if (rail) railStartInset() + RailRoom else 0.dp
    val hingeRoom = hinge?.let { it.start - pageStart }?.takeIf { it >= 280.dp }
    val split = wide && (hingeRoom != null || (rail && room >= SplitWidth))
    val formBeside = split && tab == 1 && morePage.isEmpty()
    // Elsewhere on a large screen the form floats at the side, over the page, rather than taking the screen.
    val formAtSide = appWindow >= MediumWidth && !formBeside
    val paneForm = remember { PaneForm() }
    // A new form each time one opens, so the next row's opens fresh in the same place.
    var formKey by remember { mutableIntStateOf(0) }
    // Where the person was going when the form beside the register still held changes.
    var afterForm by remember { mutableStateOf<(() -> Unit)?>(null) }
    /** Moves on from the form beside the register: at once if it holds nothing new, else once the person agrees to drop it. */
    fun leaveForm(then: () -> Unit) {
        if (!formBeside || editor != "transaction") return then()
        if (paneForm.dirty) afterForm = then else { paneForm.discard(); editor = null; then() }
    }
    val leaveFormLater by rememberUpdatedState<(() -> Unit) -> Unit> { then -> leaveForm(then) }
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
                if (!vm.hasExport()) vm.importBudget(json) else pendingImport = json
            }.onFailure { fileError = it.message ?: context.getString(R.string.backup_open_failed) }
        }
    }
    val saveFile = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        // Written after this screen may have gone, so it holds nothing of the screen's.
        val resolver = context.applicationContext.contentResolver
        val notSaved = context.getString(R.string.backup_not_saved)
        if (uri != null) vm.exportBudget { json -> replacing { mode -> resolver.openOutputStream(uri, mode) }?.use { it.write(json.toByteArray(Charsets.UTF_8)) } ?: error(notSaved) }
    }
    fun openEditor(kind: String, record: JSONObject? = null) {
        // The row already open beside the register stays as it is.
        if (formBeside && editor == "transaction" && kind == "transaction" && record != null && record.text("id") == selected?.text("id")) return
        leaveForm {
            selected = record; editor = kind; formKey++
            if (kind == "bankImport" && bankImport == null) state.budget?.let { bankImport = newBankImport(it) }
        }
    }
    // The import the assistant is open over: it's told the file's lines, and its fixes apply there.
    val importing = bankImport?.takeIf { editor == "bankImport" }
    val importScreen = remember(importing) {
        importing?.session?.let { JSONObject().put("account", importing.account).put("content", importing.content).put("session", it) }
    }
    /**
     * Applies a card's fixes to the import it's for. The card offers this only over that import,
     * so anything else here is said and nothing is done. The card is then marked as the import
     * took them: applied when any did, with a note naming the ones that didn't.
     */
    fun applyFix(proposal: JSONObject) {
        val open = importing
        val fixes = proposal.optJSONObject("fixes")
        val session = open?.session
        if (open == null || fixes == null || session == null) return assistantController.showError(context.getString(R.string.assistant_fix_open_import))
        if (fixes.optJSONObject("scope")?.optString("session") != open.sessionId) return assistantController.showError(context.getString(R.string.assistant_fix_other_import))
        val beside = assistantBeside
        assistantController.carryOut(proposal.text("id")) {
            // A change to the import still fits after an edit to the budget meanwhile.
            val applied = vm.ask("bankApplyFixes", JSONObject().put("accountId", open.account).put("content", open.content).put("session", session).put("fixes", fixes), anyRevision = true)
            val outcome = fixOutcome(applied)
            bankImport = bankImport?.let {
                if (it.sessionId == open.sessionId) it.copy(session = applied.getJSONObject("session"), view = "lines", assisted = it.assisted + outcome.lines, notice = outcome.notice) else it
            }
            // Back to the import, to see them; beside the assistant it's already in view.
            if (!beside) assistant = false
            // With none taken the card still waits, and says which didn't take.
            CardDone(if (outcome.applied > 0) "applied" else "proposed", ProposalCards.fixNote(proposal, applied))
        }
    }
    /**
     * Goes to what an assistant's card names: the register with its transactions selected, or
     * the Budget tab on its month with its categories marked. Over the app the panel gives way;
     * beside it both stay in view.
     */
    fun show(target: AssistantShow) {
        leaveForm {
            morePage = ""; parentPage = ""; accountFilter = ""; categoryFilter = ""; rangeFrom = ""; rangeThrough = ""; rangeSearch = ""; returnTab = -1
            dockHidden = false; addExpanded = true
            if (target.transactions.isNotEmpty()) tab = 1
            else { target.month?.takeIf { it != state.month }?.let(vm::selectMonth); tab = 0 }
            showing = Showing((showing?.key ?: 0) + 1, target)
            if (!assistantBeside) assistant = false
        }
    }
    fun openTab(index: Int) {
        if (index == 4) { drawer = true; return }
        leaveForm {
            tab = index; morePage = ""; parentPage = ""; accountFilter = ""; categoryFilter = ""; rangeFrom = ""; rangeThrough = ""; rangeSearch = ""; returnTab = -1
            dockHidden = false; addExpanded = true
        }
    }
    // Where the user is, for the assistant: the page, and the account or category a register shows.
    val assistantPlace = when {
        tab == 4 && morePage.isNotEmpty() -> AssistantPlace(when (morePage) {
            "Bank messages" -> "bankMessages"; "Bank connections" -> "bankConnections"; "Scheduled" -> "scheduled"
            "Payees & rules" -> "payees"; "Settings & data" -> "settings"; else -> morePage
        })
        tab == 1 -> AssistantPlace("transactions", accountFilter.ifBlank { null }, categoryFilter.ifBlank { null })
        tab == 2 -> AssistantPlace("accounts")
        tab == 3 -> AssistantPlace("reports")
        else -> AssistantPlace("budget")
    }
    fun openPage(page: String) {
        drawer = false
        if (page == "Assistant") { assistant = true; return }
        leaveForm {
            if (tab != 4) lastTab = tab
            tab = 4; morePage = page; parentPage = ""; dockHidden = false
        }
    }
    // A notification's page, such as Bank messages when texts need the user.
    val requested by PageRequests.page.collectAsStateWithLifecycle()
    // Taken only once loading is over: at a cold start the budget's arrival replaces this group (the
    // key() above), and a page opened before then would be lost. With no budget there's nothing to open.
    LaunchedEffect(requested, state.loading) {
        val page = requested ?: return@LaunchedEffect
        if (state.loading) return@LaunchedEffect
        if (state.budget != null) openPage(page)
        PageRequests.page.value = null
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
    // A loan's actions, from its register, its payment category and its account menu (Planning.tsx's
    // LoanActions). Without details saved, each opens the account form at them.
    val loanActions = remember(vm) {
        fun terms(id: String) = vm.state.value.budget?.rows("loans")?.firstOrNull { it.text("accountId") == id }
        fun open(kind: String, account: JSONObject) { leaveFormLater { selected = account; editor = kind } }
        LoanActions(
            record = { account -> open(if (terms(account.text("id")) != null) "loanPayment" else "loanDetails", account) },
            plan = { account ->
                val id = account.text("id")
                val balance = vm.state.value.summary?.rows("accounts")?.firstOrNull { it.text("id") == id }?.optLong("balance") ?: account.optLong("openingBalance")
                // Nothing to plan without details, or once it's paid off.
                if (terms(id) == null || balance >= 0) open("loanDetails", account)
                else {
                    // The planner opens over the loan's register; Back returns to where it was opened from.
                    if (tab != 1 || accountFilter != id || categoryFilter.isNotEmpty()) openRegister(RegisterTarget(account = id), tab)
                    open("loanPlanner", account)
                }
            },
            details = { account -> open("loanDetails", account) })
    }
    // The old Loan planner page, which a restored session may still be on: loans are on their
    // accounts now, so it opens the first loan's register (one with details first), as the desktop does.
    LaunchedEffect(morePage, state.budget) {
        val budget = state.budget ?: return@LaunchedEffect
        if (morePage != "Loan planner") return@LaunchedEffect
        val open = budget.rows("accounts").filter { !it.optBoolean("archived") }
        val saved = budget.rows("loans").map { it.text("accountId") }.toSet()
        val balances = state.summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty()
        val loan = open.firstOrNull { it.text("id") in saved } ?: open.firstOrNull { AccountTypes.hasLoan(it, balances[it.text("id")] ?: it.optLong("openingBalance"), saved) }
        morePage = ""; parentPage = ""
        if (loan != null) openRegister(RegisterTarget(account = loan.text("id")), 0) else tab = 0
    }
    val importFile = { openFile.launch(arrayOf("application/json", "text/plain", "application/octet-stream")) }
    val exportFile = { saveFile.launch("UrBudget-${LocalDate.now()}.json") }
    // Back closes what's open, newest first; pages register their own handlers (search, selection)
    // after this one, so theirs come first. Then an account goes back to Accounts, a page to the first tab.
    // Beside the app the assistant closes once the app has nowhere left to go back to.
    val appBack = morePage.isNotEmpty() || tab != 0 || accountFilter.isNotEmpty() || categoryFilter.isNotEmpty()
    // A bank import the assistant covers on a phone isn't drawn, so it has no Back of its own and
    // Back would leave the app: it closes the assistant instead, back to the import.
    val assistantOverImport = assistant && !assistantBeside && editor == "bankImport"
    BackHandler((editor == null && (assistant || appBack)) || assistantOverImport) {
        when {
            assistant && (!assistantBeside || !appBack) -> assistant = false
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
    val cloudStatus by NativeServices.syncStatus.collectAsStateWithLifecycle()
    val common: @Composable RowScope.() -> Unit = {
        RoundIconButton(Icons.AutoMirrored.Outlined.Chat, stringResource(R.string.assistant_title), { assistant = true })
        // Sync's state sits in the menu, so a narrow screen keeps room for the month beside these buttons.
        OverflowMenu(vm, if (cloudStatus?.isNull("email") == false) syncState(cloudStatus) else null, { openPage("Account & sync") },
            library = { openPage("Budget library") }) { openPage("Settings & data") }
    }
    val budget = state.budget
    // The register and Reports split into list and detail (ListDetail) with room for both, and have it.
    val fontScale = androidx.compose.ui.platform.LocalDensity.current.fontScale
    // The budget splits only while a category is open (BudgetScreen docks it beside the budget).
    val splitList = if (split && morePage.isEmpty() && tab in 0..3 && tab != 2) {
        when {
            hinge != null && hingeRoom != null -> Split(hingeRoom, hinge.width, hinge = true)
            // Reports' list is the reports by name; the report beside it takes the room.
            tab == 3 -> Split(reportsListWidth(room))
            tab == 0 -> Split(budgetListWidth(room))
            else -> Split(splitListWidth(room, fontScale))
        }
    } else null
    val pageWidth = when {
        hingeRoom != null && splitList != null -> androidx.compose.ui.unit.Dp.Infinity
        hingeRoom != null -> minOf(hingeRoom, ReadableWidth)
        splitList != null -> SplitPageWidth
        else -> ReadableWidth
    }
    val formOpenBeside = formBeside && editor == "transaction" && budget != null
    // On a tall enough large screen "+ Transaction" heads the rail, under the thumb that holds that
    // side; half open on a table the rail keeps to the flat half, where there's no room for it.
    val table = tableTop()
    val railHeight = windowHeight() - RailInsets.only(WindowInsetsSides.Vertical).asPaddingValues().let { it.calculateTopPadding() + it.calculateBottomPadding() }
    val railAdd = rail && table == null && railHeight >= 500.dp
    // One form, wherever it shows: beside the register, floating at the side, or over the screen. It
    // moves between them as the phone folds and unfolds, and keeps what's been typed.
    val transactionForm = remember(formKey) {
        movableContentOf { budget: JSONObject, summary: JSONObject?, host: FormHost ->
            TransactionDialog(selected, budget, accountFilter, vm, close = { editor = null }, summary = summary, host = host)
        }
    }
    val besideList = remember(paneForm) { FormHost.Beside(paneForm) }
    val undoLabel = stringResource(R.string.common_undo)
    val toast = remember(snack, scope, vm, undoLabel) { { message: String, undo: Boolean -> snack.toast(scope, vm, message, undo, undoLabel) } }
    LaunchedEffect(state.exported) { if (state.exported) { vm.exportSaid(); toast(context.getString(R.string.backup_exported), false) } }
    // An import added transactions: said with Undo, and with those still without a category offered to the assistant.
    val categorizeLabel = stringResource(R.string.assistant_categorize)
    val importSaid = remember(snack, scope, toast, askAssistant, categorizeLabel) {
        { before: JSONObject?, after: JSONObject?, fromBank: Boolean ->
            if (before != null && after != null) scope.launch {
                // Two budgets are compared, off the main thread.
                val imported = withContext(Dispatchers.Default) { AssistantStart.uncategorized(before, after) }
                if (imported.added > 0) {
                    val message = Words.count(if (fromBank) R.plurals.bank_sync_imported_review else R.plurals.bank_imported_review, imported.added.toLong())
                    if (imported.ids.isEmpty()) toast(message, true)
                    else {
                        snack.currentSnackbarData?.dismiss()
                        if (snack.showSnackbar(OfferVisuals(message, categorizeLabel)) == SnackbarResult.ActionPerformed) askAssistant.request(AssistantStart.categorize(imported), imported.ids)
                    }
                }
            }
            Unit
        }
    }
    CompositionLocalProvider(LocalToast provides toast, LocalLoanActions provides loanActions, LocalSplitList provides splitList, LocalAssistantAsk provides askAssistant,
        LocalImportSaid provides importSaid) {
    Box(Modifier.fillMaxSize().background(ur.bg).nestedScroll(scrollWatcher)) {
    // The app's own screen: the whole window, or its start side while the assistant shares it. Under the
    // assistant on a phone it's out of a screen reader's way, as a page under a sheet is.
    Box(Modifier.align(Alignment.TopStart).fillMaxHeight().then(if (appWidth != null) Modifier.width(appWidth) else Modifier.fillMaxWidth())
        .then(if (assistant && !assistantBeside && budget != null) Modifier.clearAndSetSemantics { } else Modifier)) {
    ProvideAppSide(appWidth) {
    Box(Modifier.fillMaxSize()) {
        // The page, its add button and messages. On a large screen they keep a readable width, centred
        // beside the rail; on a phone the width is never reached and the page fills the screen as before.
        Box(Modifier.fillMaxSize().then(if (rail) Modifier.windowInsetsPadding(RailInsets.only(WindowInsetsSides.Start)).padding(start = RailRoom) else Modifier),
            contentAlignment = if (hingeRoom != null) Alignment.TopStart else Alignment.TopCenter) {
        // A page that doesn't split leaves the hinge's far side to what's opened from it.
        if (hingeRoom != null && hinge != null && splitList == null && budget != null && !state.loading) Box(Modifier.fillMaxSize().padding(start = hinge.end - pageStart)) {
            FoldSideHint()
        }
        Box(Modifier.widthIn(max = pageWidth).fillMaxSize()) {
        when {
            state.loading -> CircularProgressIndicator(Modifier.align(Alignment.Center), color = ur.accent)
            morePage == "Account & sync" && budget == null -> GlassTop(Modifier.fillMaxSize(), top = {
                PhoneTopBar(pageTitle("Account & sync"), onBack = { morePage = ""; tab = 0 })
            }) { CloudScreen(vm) }
            morePage == "Budget library" && budget == null -> GlassTop(Modifier.fillMaxSize(), top = {
                PhoneTopBar(pageTitle("Budget library"), onBack = { morePage = ""; tab = 0 })
            }) { LibraryScreen(null, vm, importFile, exportFile) }
            // Below the status bar; the welcome page's own padding then has no inset left to add.
            budget == null -> Column(Modifier.fillMaxSize().statusBarsPadding()) {
                TextButton({ morePage = "Budget library" }) { Text(stringResource(R.string.page_library)) }
                WelcomeScreen(state.busy, { name, currency, sample ->
                if (vm.hasExport()) pendingCreate = Triple(name, currency, sample) else vm.createBudget(name, currency, sample)
            }, importFile, if (vm.hasExport()) exportFile else null, if (state.recoveryAvailable) ({ confirmRestore = true }) else null,
                { tab = 4; morePage = "Account & sync" })
            }
            // Each page's list scrolls under the bar, which turns to glass over it. Bank messages has screens of its own inside and keeps below the bar.
            tab == 4 && morePage.isNotEmpty() -> pageStates.SaveableStateProvider("page:$morePage") { GlassTop(Modifier.fillMaxSize(), padded = morePage == "Bank messages", top = {
                PhoneTopBar(pageTitle(morePage), budget.text("name"), onBack = ::closePage, actions = common)
            }) {
                Box(Modifier.fillMaxSize()) {
                    when (morePage) {
                        "Account & sync" -> CloudScreen(vm)
                        "Bank connections" -> BankConnectionsScreen(budget, vm)
                        "Bank messages" -> SmsImportScreen(budget, vm, setDockHidden = { dockHidden = it }, decline = ::closePage)
                        "Budget library" -> LibraryScreen(budget, vm, importFile, exportFile)
                        else -> MoreScreen(morePage, ::openChildPage, budget, vm, importFile, exportFile,
                            if (state.recoveryAvailable) ({ confirmRestore = true }) else null) { kind, item -> openEditor(kind, item) }
                    }
                }
            } }
            else -> pageStates.SaveableStateProvider(if (tab == 1) "register:$accountFilter|$categoryFilter|$rangeFrom|$rangeThrough|$rangeSearch" else "tab:$tab") { when (tab) {
                0 -> BudgetScreen(budget, state.summary, state.month, vm::selectMonth, vm, common,
                    onTransactions = { target -> openRegister(target, 0) },
                    onPayCard = { card -> openEditor("cardPayment", JSONObject().put("id", card)) }, showing = showing)
                1 -> TransactionsScreen(budget, state.summary, accountFilter, categoryFilter,
                    onBack = when {
                        returnTab >= 0 -> ({ leaveForm { closeRegister() } })
                        accountFilter.isNotEmpty() -> ({ leaveForm { accountFilter = ""; tab = 2 } })
                        categoryFilter.isNotEmpty() -> ({ leaveForm { categoryFilter = ""; tab = 0 } })
                        else -> null
                    }, initialFrom = rangeFrom, initialThrough = rangeThrough, initialSearch = rangeSearch, payCard = { card -> openEditor("cardPayment", JSONObject().put("id", card)) }, setCategory = { categoryFilter = it }, edit = { openEditor("transaction", it) }, add = { openEditor("transaction") },
                    editAccount = { openEditor("account", it) }, reconcile = { openEditor("reconcile", it) },
                    editSchedule = { openEditor("schedule", it) }, vm = vm, bar = common, setDockHidden = { dockHidden = it },
                    openAccount = { id -> accountFilter = id; categoryFilter = ""; rangeFrom = ""; rangeThrough = "" },
                    openId = if (formOpenBeside) selected?.text("id") else null,
                    // An assistant card's transactions are shown in the whole register, where every one of them is.
                    showing = showing?.takeIf { accountFilter.isEmpty() && categoryFilter.isEmpty() && rangeFrom.isEmpty() && rangeThrough.isEmpty() && rangeSearch.isEmpty() })
                2 -> AccountsScreen(budget, state.summary, open = { account -> accountFilter = account.text("id"); categoryFilter = ""; tab = 1 },
                    edit = { openEditor("account", it) }, reconcile = { openEditor("reconcile", it) }, add = { openEditor("account") },
                    vm = vm, bar = common)
                else -> ReportsScreen(budget, state.reports, state.summary, state.month, vm::selectMonth, vm, common,
                    openRegister = { target -> openRegister(target, 3) }) { openEditor("transaction", it) }
            } }
        }
        // The form beside the register, over the pane the register leaves for it. It's here, not in the
        // register, so a change of the register's filter can't reset what's being typed.
        if (formOpenBeside && budget != null && splitList != null) {
            Box(Modifier.fillMaxSize().padding(start = splitList.detailStart)) { transactionForm(budget, state.summary, besideList) }
        }
        // Over the page: above the dock on a phone, at the foot of the page beside the rail (or in the
        // rail itself, where it's tall enough). Beside a hinge they keep to the dock's side of it, clear
        // of whatever is open on the other.
        val dockSide = if (hingeRoom != null) Modifier.width(hingeRoom) else Modifier.fillMaxWidth()
        if (budget != null) {
            val showAdd = tab in 0..2 && morePage.isEmpty() && !dockHidden && !state.loading && !formOpenBeside && !railAdd
            Box(Modifier.align(Alignment.BottomStart).then(dockSide), contentAlignment = Alignment.BottomEnd) {
                AnimatedVisibility(showAdd, Modifier.navigationBarsPadding().padding(end = 16.dp, bottom = if (rail) 16.dp else 92.dp),
                    enter = fadeIn() + scaleIn(initialScale = .8f), exit = fadeOut() + scaleOut(targetScale = .8f)) {
                    AddButton(addExpanded) { openEditor("transaction") }
                }
            }
        }
        Box(Modifier.align(Alignment.BottomStart).then(dockSide), contentAlignment = Alignment.BottomCenter) {
        SnackbarHost(snack, Modifier.navigationBarsPadding().padding(bottom = if (rail) 16.dp else 96.dp)) { data ->
            val fill = if (ur.dark) androidx.compose.ui.graphics.Color(0xFF262C3E) else androidx.compose.ui.graphics.Color(0xFF1D1D1F)
            val ink = if (ur.dark) ur.text else androidx.compose.ui.graphics.Color(0xFFF5F5F7)
            val action = if (ur.dark) ur.accent else androidx.compose.ui.graphics.Color(0xFF78A7FF)
            val offer = data.visuals as? OfferVisuals
            if (offer == null) Snackbar(data, Modifier.padding(horizontal = 12.dp), shape = RoundedCornerShape(16.dp), containerColor = fill, contentColor = ink, actionColor = action)
            // A message with an offer keeps Undo beside it, on a line of their own under the words.
            else Snackbar(Modifier.padding(horizontal = 12.dp).testTag("import-said"), shape = RoundedCornerShape(16.dp), containerColor = fill, contentColor = ink, actionOnNewLine = true,
                action = {
                    Row {
                        TextButton({ vm.undo(); data.dismiss() }) { Text(undoLabel, color = action) }
                        TextButton({ data.performAction() }) { Text(offer.offer, color = action) }
                    }
                }) { Text(offer.message) }
        }
        }
        } }
        BusyBar(vm, Modifier.align(Alignment.TopCenter))
        // The rail stays while a list selects or the SMS review shows its own bar: those sit at the
        // foot of the page, clear of it, where on a phone they'd take the dock's place.
        if (rail) {
            // Half open on a table the rail sits on the flat half, below the fold, if it fits there.
            val onTable = table != null && windowHeight() - table >= 360.dp
            Box(Modifier.fillMaxSize().padding(top = if (onTable) table!! else 0.dp)) {
                TabRail(if (tab == 4) 4 else tab, ::openTab, Modifier.align(Alignment.CenterStart)
                    .windowInsetsPadding(RailInsets.only(WindowInsetsSides.Start + (if (onTable) WindowInsetsSides.Bottom else WindowInsetsSides.Vertical))).padding(start = RailMargin),
                    add = if (railAdd && !state.loading) ({ openEditor("transaction") }) else null)
            }
        }
        // Beside a hinge the dock spans only the start side, the phone it makes.
        else if (budget != null) AnimatedVisibility(!dockHidden, Modifier.align(Alignment.BottomStart).then(if (hingeRoom != null) Modifier.width(hingeRoom) else Modifier),
            enter = slideInVertically { it } + fadeIn(), exit = slideOutVertically { it } + fadeOut()) {
            TabDock(if (tab == 4) 4 else tab, ::openTab)
        }
    }
    }
    }
        // On a phone the assistant rises over a dimmed page; beside the app on a large screen both stay in use.
        AnimatedVisibility(assistant && budget != null && !assistantBeside, enter = fadeIn(tween(180)), exit = fadeOut(tween(180))) {
            // Named for what a tap on it does, as the scrim of a sheet is.
            val closeLabel = stringResource(R.string.assistant_close)
            Box(Modifier.fillMaxSize().background(ur.scrim).clickable(remember { MutableInteractionSource() }, null, onClickLabel = closeLabel) { assistant = false }
                .semantics { contentDescription = closeLabel })
        }
        val assistantDrag = rememberPanelDrag(assistant) { assistant = false }
        // One assistant, wherever it stands: folding moves it between the side and the phone's panel
        // with what's being typed in it.
        // Whether the panel is over the app or beside it, and where a card's Show leads, as they are when asked.
        val covering by rememberUpdatedState(!assistantBeside)
        val showTarget by rememberUpdatedState<(AssistantShow) -> Unit> { show(it) }
        val assistantPanel = remember {
            movableContentOf { importing: JSONObject?, fix: (JSONObject) -> Unit, place: AssistantPlace, drag: Modifier ->
                AssistantScreen(vm, onClose = { assistant = false }, importing = importing, applyFix = fix, place = place, dragArea = drag,
                    covers = { covering }, show = { showTarget(it) })
            }
        }
        // On a large screen the assistant stands at the window's end, full height, as a screen of its own
        // beside the app (the end side, when a hinge splits the screen).
        if (fullWide) AnimatedVisibility(assistant && budget != null, Modifier.align(Alignment.CenterEnd), enter = panelEnter(), exit = panelExit()) {
            Box(Modifier.windowInsetsPadding(RailInsets).padding(PanelMargin).width(assistantWidth).fillMaxHeight()
                .floatingPanel(ur.surface, ur.dark, ur.border).padding(top = 6.dp)) {
                assistantPanel(importScreen, ::applyFix, assistantPlace, Modifier)
            }
        }
        else AnimatedVisibility(assistant && budget != null, Modifier.align(Alignment.BottomCenter),
            enter = slideInVertically(tween(260)) { it }, exit = slideOutVertically(tween(220)) { it }) {
            Column(Modifier.panelDrag(assistantDrag).fillMaxWidth().fillMaxHeight(.94f).clip(RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)).background(ur.surface)
                .then(if (ur.dark) Modifier.border(1.dp, ur.border, RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)) else Modifier)
                .clickable(remember { MutableInteractionSource() }, null) {}) {
                Box(Modifier.fillMaxWidth().then(assistantDrag.dragArea).padding(top = 8.dp, bottom = 4.dp), contentAlignment = Alignment.Center) {
                    Box(Modifier.size(36.dp, 5.dp).clip(CircleShape).background(ur.borderStrong))
                }
                Box(Modifier.weight(1f).navigationBarsPadding()) {
                    assistantPanel(importScreen, ::applyFix, assistantPlace, assistantDrag.dragArea)
                }
            }
        }
    }
    // What the app opens keeps to its own screen while the assistant shares the window.
    ProvideAppSide(appWidth) {
    if (drawer && budget != null) MoreDrawer(budget, { drawer = false }, ::openPage, { openEditor("bankImport"); drawer = false },
        { openEditor("ynabImport"); drawer = false })
    if (budget != null) when (editor) {
        // Over the screen, unless it opened beside the register (above).
        "transaction" -> if (!formBeside) transactionForm(budget, state.summary, if (formAtSide) FormHost.Side else FormHost.Screen)
        "cardPayment" -> selected?.let { CardPaymentSheet(it.text("id"), budget, state.summary, vm) { editor = null } }
        "account" -> AccountDialog(selected, budget, vm) { editor = null }
        // A loan's details are in its account form; its payment and planner need them saved.
        "loanDetails" -> selected?.let { budget.row("accounts", it.text("id")) }?.let { AccountDialog(it, budget, vm, focusLoan = true) { editor = null } }
            ?: LaunchedEffect(Unit) { editor = null }
        "loanPayment", "loanPlanner" -> selected?.let { budget.row("accounts", it.text("id")) }?.let { account ->
            val terms = remember(budget, account) { budget.rows("loans").firstOrNull { it.text("accountId") == account.text("id") } }
            if (terms == null) LaunchedEffect(Unit) { editor = null }
            else if (editor == "loanPayment") LoanPaymentSheet(account, terms, budget, state.summary, vm) { editor = null }
            else LoanPlannerSheet(account, terms, budget, state.summary, vm) { editor = null }
        }
        "reconcile" -> selected?.let { ReconcileDialog(it, budget, state.summary, vm) { editor = null } }
        "schedule" -> ScheduleDialog(selected, budget, vm) { editor = null }
        "payee" -> PayeeDialog(selected, budget, vm) { editor = null }
        "rule" -> RuleDialog(selected, budget, vm) { editor = null }
        // Hidden while the assistant is open over it on a phone; beside it on a large screen, the two in
        // view at once. A PDF statement goes to the assistant, attached to its next message.
        "bankImport" -> bankImport?.let { draft ->
            if (!assistant || assistantBeside) BankImportDialog(budget, vm, draft, { change -> bankImport = bankImport?.let(change) },
                askAssistant = { uri -> assistantController.attach(context, uri); editor = null; bankImport = null; assistant = true },
                fixWithAssistant = { assistantController.prefill(fixImportText); assistant = true }) { editor = null; bankImport = null }
        }
        "ynabImport" -> YnabImportDialog(budget, vm) { editor = null }
    }
    }
    afterForm?.let { then ->
        AlertDialog(onDismissRequest = { afterForm = null }, title = { Text(stringResource(R.string.transaction_unsaved_title)) },
            text = { Text(stringResource(R.string.transaction_unsaved_message)) },
            confirmButton = { TextButton({ afterForm = null; paneForm.discard(); editor = null; then() }) { Text(stringResource(R.string.transaction_unsaved_discard), color = ur.negative) } },
            dismissButton = { TextButton({ afterForm = null }) { Text(stringResource(R.string.transaction_unsaved_keep)) } }, containerColor = ur.surface)
    }
    val error = state.error ?: fileError ?: state.exportFailure
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
        tabs.indices.forEach { index -> TabButton(index, index == tab, onTab, Modifier.weight(if (index == 1) 1.35f else 1f).fillMaxHeight()) }
    }
}

/**
 * On a large screen the tabs stand in a rail at the window's start, in the dock's frosted glass. With
 * [add], "+ Transaction" heads it as a round button in the accent, as Material's rail carries its action.
 */
@Composable
private fun TabRail(tab: Int, onTab: (Int) -> Unit, modifier: Modifier, add: (() -> Unit)? = null) {
    val shape = RoundedCornerShape(28.dp)
    Column(modifier.width(RailWidth).floating(shape, ur.dark, ur.border).clip(shape).background(ur.glass).border(0.5.dp, ur.border, shape).padding(4.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        AnimatedVisibility(add != null, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                val label = stringResource(R.string.transaction_add)
                Box(Modifier.padding(top = 6.dp).size(50.dp).clip(RoundedCornerShape(17.dp)).background(ur.accent).testTag("rail-add")
                    .clickable(role = Role.Button, onClickLabel = label) { add?.invoke() }, contentAlignment = Alignment.Center) {
                    Icon(Icons.Outlined.Add, label, Modifier.size(26.dp), tint = if (ur.dark) ur.bg else androidx.compose.ui.graphics.Color.White)
                }
                HorizontalDivider(Modifier.padding(top = 10.dp, bottom = 4.dp).width(32.dp), thickness = 0.5.dp, color = ur.borderStrong)
            }
        }
        tabs.indices.forEach { index -> TabButton(index, index == tab, onTab, Modifier.fillMaxWidth().height(58.dp), compact = true) }
    }
}

/**
 * A tab of the dock or the rail: its icon over its name, on a tint while it's the one open. In the
 * narrow rail ([compact]) a long name (Transactions, at a large text size) steps down to fit rather
 * than being cut.
 */
@Composable
private fun TabButton(index: Int, active: Boolean, onTab: (Int) -> Unit, modifier: Modifier, compact: Boolean = false) {
    val (labelId, icon) = tabs[index]
    val label = stringResource(labelId)
    val tint = if (active) ur.accent else ur.text
    Column(modifier.clip(RoundedCornerShape(if (compact) 22.dp else 28.dp)).testTag("tab:$index")
        .background(if (active) ur.tabActive else androidx.compose.ui.graphics.Color.Transparent)
        .clickable(role = Role.Tab, onClickLabel = label) { onTab(index) },
        horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        Icon(icon, null, Modifier.size(22.dp), tint = tint)
        if (compact) androidx.compose.foundation.text.BasicText(label, Modifier.padding(horizontal = 2.dp),
            style = androidx.compose.ui.text.TextStyle(color = tint, fontWeight = FontWeight.SemiBold, fontSize = 11.sp), maxLines = 1,
            autoSize = androidx.compose.foundation.text.TextAutoSize.StepBased(minFontSize = 8.5.sp, maxFontSize = 11.sp, stepSize = .5.sp))
        else Text(label, fontSize = 11.5.sp, fontWeight = FontWeight.SemiBold, color = tint, maxLines = 1)
    }
}

/** "+ Transaction" floats above the tab bar, and shrinks to its plus while a list scrolls down. */
@Composable
private fun AddButton(expanded: Boolean, onClick: () -> Unit) {
    val shape = RoundedCornerShape(26.dp)
    Row(Modifier.height(52.dp).floating(shape, ur.dark, ur.borderStrong).clip(shape).background(ur.glass).border(0.5.dp, ur.border, shape).testTag("tx-add")
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

/** The ⋯ menu: undo, redo, the budget library and Settings. It follows only the undo, redo and busy flags. */
@Composable
private fun OverflowMenu(vm: BudgetViewModel, sync: Int?, openSync: () -> Unit, library: () -> Unit, settings: () -> Unit) {
    val state by remember(vm) { vm.state.map { Triple(it.undoAvailable, it.redoAvailable, it.busy) }.distinctUntilChanged() }
        .collectAsStateWithLifecycle(Triple(false, false, false))
    val (undoAvailable, redoAvailable, busy) = state
    var open by remember { mutableStateOf(false) }
    val alert = sync == R.string.sync_status_paused || sync == R.string.sync_status_attention
    Box {
        RoundIconButton(Icons.Outlined.MoreHoriz, stringResource(R.string.common_more_actions), { open = true }, dot = alert)
        DropdownMenu(open, { open = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
            if (sync != null) {
                val icon = when (sync) {
                    R.string.sync_status_synced -> Icons.Outlined.CloudDone
                    R.string.sync_status_device -> Icons.Outlined.Smartphone
                    R.string.sync_status_waiting -> Icons.Outlined.CloudSync
                    R.string.sync_status_offline, R.string.sync_status_paused -> Icons.Outlined.CloudOff
                    else -> Icons.Outlined.SyncProblem
                }
                DropdownMenuItem(text = { Text(stringResource(sync)) }, leadingIcon = { Icon(icon, null, tint = if (alert) ur.warning else LocalContentColor.current) },
                    onClick = { openSync(); open = false })
                HorizontalDivider(color = ur.border)
            }
            DropdownMenuItem(text = { Text(stringResource(R.string.common_undo)) }, leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Undo, null) }, enabled = undoAvailable && !busy,
                onClick = { vm.undo(); open = false })
            DropdownMenuItem(text = { Text(stringResource(R.string.common_redo)) }, leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Redo, null) }, enabled = redoAvailable && !busy,
                onClick = { vm.redo(); open = false })
            HorizontalDivider(color = ur.border)
            DropdownMenuItem(text = { Text("Budgets") }, leadingIcon = { Icon(Icons.Outlined.FolderOpen, null) }, onClick = { library(); open = false })
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
        // The app icon's two layers: the mark on the brand purple, as on the home screen.
        Box(Modifier.size(64.dp).clip(RoundedCornerShape(18.dp)).background(androidx.compose.ui.graphics.Color(0xFF6D4AE8)), contentAlignment = Alignment.Center) {
            androidx.compose.foundation.Image(androidx.compose.ui.res.painterResource(R.drawable.ic_launcher_foreground), contentDescription = null, modifier = Modifier.fillMaxSize())
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

/** Where sync stands, as the string the menu shows. */
private fun syncState(status: JSONObject?): Int {
    val sync = status?.optJSONObject("sync")
    val report = sync?.optJSONObject("last")
    val rows = report?.optJSONArray("budgets")
    val states = (0 until (rows?.length() ?: 0)).map { rows?.optJSONObject(it)?.optString("state") }
    return when {
        sync?.optBoolean("unlocked") != true && (sync?.optBoolean("enabled") == true || report != null) -> R.string.sync_status_paused
        states.any { it in setOf("conflict", "error", "update_app", "recovery", "subscription_required", "deletion_conflict") } -> R.string.sync_status_attention
        report?.optString("state") == "offline" -> R.string.sync_status_offline
        report?.optString("state") == "waiting" || status?.optBoolean("libraryPending") == true || states.any { it in setOf("waiting", "queued") } -> R.string.sync_status_waiting
        states.any { it in setOf("paused", "locked", "signed_out") } -> R.string.sync_status_paused
        states.any { it == "local_only" } || states.isEmpty() -> R.string.sync_status_device
        report?.optString("state") == "ok" -> R.string.sync_status_synced
        else -> R.string.sync_status_attention
    }
}
