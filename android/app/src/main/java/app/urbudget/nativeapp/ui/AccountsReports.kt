package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ShowChart
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.urbudget.nativeapp.AccountTypes
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.accountBadges
import app.urbudget.nativeapp.records
import app.urbudget.nativeapp.upsertRecord
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.util.UUID

/** An account type's icon, as the desktop's accounts list draws them. */
internal fun iconForType(type: String, kind: String): ImageVector = when (type) {
    "checking" -> Icons.Outlined.AccountBalance
    "savings" -> Icons.Outlined.Savings
    "cash" -> Icons.Outlined.Payments
    "creditCard", "lineOfCredit" -> Icons.Outlined.CreditCard
    "mortgage", "property" -> Icons.Outlined.House
    "autoLoan", "vehicle" -> Icons.Outlined.DirectionsCar
    "studentLoan" -> Icons.Outlined.School
    "investment", "retirement" -> Icons.AutoMirrored.Outlined.ShowChart
    "receivable", "personalLoan" -> Icons.Outlined.VolunteerActivism
    "medicalDebt" -> Icons.Outlined.LocalHospital
    "otherDebt" -> Icons.Outlined.Receipt
    else -> when (kind) { "credit" -> Icons.Outlined.CreditCard; "tracking" -> Icons.Outlined.MonetizationOn; else -> Icons.Outlined.AccountBalance }
}

internal fun accountIcon(account: JSONObject): ImageVector = iconForType(AccountTypes.infer(account), account.text("kind"))

private fun typeLabel(type: String) = accountTypes().firstOrNull { it.first == type }?.second ?: type

/** One account as its row shows it, prepared away from composition. */
private data class AccountLine(val id: String, val account: JSONObject, val name: String, val type: String, val kind: String, val subtitle: String,
                               val balance: Long, val balanceText: String, val approvals: Int, val needsCategory: Int, val inNetWorth: Boolean,
                               val loan: Boolean, val archived: Boolean)

private data class AccountSection(val key: String, val name: String, val lines: List<AccountLine>, val total: Long, val totalText: String)

private suspend fun accountSections(budget: JSONObject, summary: JSONObject?, today: String): List<AccountSection> {
    val currency = budget.text("currency")
    val balances = summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }
    val badges = accountBadges(budget, today)
    val loans = budget.rows("loans").map { it.text("accountId") }.toSet()
    val all = budget.rows("accounts")
    fun line(account: JSONObject): AccountLine {
        val id = account.text("id")
        val balance = balances?.get(id) ?: account.optLong("openingBalance")
        val type = AccountTypes.infer(account, balance)
        val worth = AccountTypes.inNetWorth(account)
        return AccountLine(id, account, account.text("name"), type, account.text("kind"),
            if (worth) typeLabel(type) else Words[R.string.account_not_in_net_worth, typeLabel(type)], balance, if (balances == null) "—" else money(balance, currency),
            badges.approvals[id] ?: 0, badges.uncategorized[id] ?: 0, worth, AccountTypes.hasLoan(account, balance, loans), account.optBoolean("archived"))
    }
    return listOf("cash" to Words[R.string.accounts_section_cash], "credit" to Words[R.string.accounts_section_credit], "tracking" to Words[R.string.accounts_section_tracking],
        "closed" to Words[R.string.accounts_section_closed]).mapNotNull { (key, name) ->
        val lines = all.filter { if (key == "closed") it.optBoolean("archived") else !it.optBoolean("archived") && it.text("kind") == key }.map(::line)
        // Accounts left out of net worth are left out of the total too, as in the sidebar.
        val total = lines.sumOf { if (it.inNetWorth) it.balance else 0L }
        if (lines.isEmpty()) null else AccountSection(key, name, lines, total, money(total, currency))
    }
}

/** Folded sections, remembered for the session per budget, as the desktop remembers them. */
private object AccountSectionMemory {
    private val folded = HashMap<String, Set<String>>()
    fun get(budget: String) = folded[budget] ?: setOf("closed")
    fun set(budget: String, value: Set<String>) { folded[budget] = value }
}

/**
 * Accounts on a phone: cash, credit, tracking and closed accounts, each kind a card with its total.
 * A tap opens the account's transactions; a long press, its actions.
 */
@Composable
internal fun AccountsScreen(budget: JSONObject, summary: JSONObject?, open: (JSONObject) -> Unit, edit: (JSONObject) -> Unit, reconcile: (JSONObject) -> Unit,
                            add: () -> Unit, vm: BudgetViewModel, bar: @Composable RowScope.() -> Unit) {
    val currency = budget.text("currency")
    val identity = remember { vm.state.value.budgetIdentity.orEmpty() }
    var folded by remember(identity) { mutableStateOf(AccountSectionMemory.get(identity)) }
    val sections = rememberPrepared(budget, summary, languageKey()) { accountSections(budget, summary, LocalDate.now().toString()) }
    val undo = stringResource(R.string.common_undo)
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    var loanFor by remember { mutableStateOf<JSONObject?>(null) }
    fun move(line: AccountLine, section: AccountSection, delta: Int) {
        val index = section.lines.indexOfFirst { it.id == line.id }
        val neighbour = section.lines.getOrNull(index + delta) ?: return
        withCurrentBudget(budget, vm) {
            vm.editBudget { draft -> draft.put("accounts", AccountTypes.moveAccount(draft.optJSONArray("accounts") ?: JSONArray(), line.id, neighbour.id, delta > 0)) }
        }
    }
    fun setClosed(line: AccountLine, archived: Boolean) = withCurrentBudget(budget, vm) {
        vm.upsert("accounts", JSONObject().put("id", line.id).put("archived", archived))
        val message = when {
            !archived -> Words[R.string.account_reopened, line.name]
            line.balance != 0L -> Words[R.string.account_closed_with_balance, line.name, line.balanceText]
            else -> Words[R.string.account_closed_toast, line.name]
        }
        scope.launch {
            snackbar.currentSnackbarData?.dismiss()
            if (snackbar.showSnackbar(message, actionLabel = undo, withDismissAction = true, duration = SnackbarDuration.Long) == SnackbarResult.ActionPerformed)
                vm.upsert("accounts", JSONObject().put("id", line.id).put("archived", !archived))
        }
    }
    Box(Modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize()) {
            PhoneTopBar(stringResource(R.string.tab_accounts), actions = { RoundIconButton(Icons.Outlined.Add, stringResource(R.string.account_add), add); bar() })
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, bottom = DockRoom)) {
                if (summary != null) item(key = "networth", contentType = "networth") {
                    val worth = summary.optLong("netWorth")
                    Row(Modifier.fillMaxWidth().padding(bottom = 4.dp).clip(RoundedCornerShape(18.dp)).background(ur.surface).padding(horizontal = 18.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(stringResource(R.string.net_worth), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
                            Text(money(worth, currency), fontSize = 26.sp, fontWeight = FontWeight.ExtraBold, style = Tabular, color = if (worth < 0) ur.negative else ur.text)
                        }
                        Icon(Icons.Outlined.AccountBalanceWallet, null, Modifier.size(28.dp), tint = ur.accent)
                    }
                }
                when {
                    sections == null -> item(key = "loading") { LinearProgressIndicator(Modifier.fillMaxWidth().padding(vertical = 16.dp), color = ur.accent) }
                    sections.isEmpty() -> item(key = "empty") { EmptyState(stringResource(R.string.accounts_empty_title), stringResource(R.string.accounts_empty_detail), stringResource(R.string.account_add), add) }
                }
                sections.orEmpty().forEach { section ->
                    val isOpen = section.key !in folded
                    item(key = "section:${section.key}", contentType = "section") {
                        Box(Modifier.padding(top = 6.dp, bottom = 6.dp)) {
                            FoldingHeader(section.name, isOpen, {
                                folded = if (isOpen) folded + section.key else folded - section.key
                                AccountSectionMemory.set(identity, folded)
                            }, section.totalText, if (section.key != "closed" && section.total < 0) ur.negative else ur.text)
                        }
                    }
                    if (isOpen) itemsIndexed(section.lines, key = { _, line -> "account:${line.id}" }, contentType = { _, _ -> "account" }) { index, line ->
                        AccountRow(line, Modifier.cardSegment(index, section.lines.size, cardFill()), divider = index < section.lines.lastIndex,
                            open = { open(line.account) }, edit = { edit(line.account) }, reconcile = { reconcile(line.account) },
                            loan = if (line.loan) ({ loanFor = budget.rows("loans").firstOrNull { it.text("accountId") == line.id } ?: JSONObject().put("accountId", line.id) }) else null,
                            moveUp = if (index > 0) ({ move(line, section, -1) }) else null,
                            moveDown = if (index < section.lines.lastIndex) ({ move(line, section, 1) }) else null,
                            toggleClosed = { setClosed(line, !line.archived) })
                    }
                }
            }
        }
        SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 96.dp))
    }
    loanFor?.let { LoanDialog(it, budget, summary, vm) { loanFor = null } }
}

@Composable
private fun AccountRow(line: AccountLine, modifier: Modifier, divider: Boolean, open: () -> Unit, edit: () -> Unit, reconcile: () -> Unit, loan: (() -> Unit)?,
                       moveUp: (() -> Unit)?, moveDown: (() -> Unit)?, toggleClosed: () -> Unit) {
    var menu by remember { mutableStateOf(false) }
    val outside = !line.inNetWorth
    Box(modifier) {
        ListRow(line.name, subtitle = line.subtitle, icon = iconForType(line.type, line.kind),
            iconTint = when { outside -> ur.muted; line.kind == "cash" -> ur.positive; line.kind == "credit" -> ur.accent; else -> ur.text2 },
            iconFill = when { outside -> ur.zeroSoft; line.kind == "cash" -> ur.positiveSoft; line.kind == "credit" -> ur.accentSoft; else -> ur.zeroSoft },
            titleColor = if (outside) ur.text2 else ur.text, onClick = open, onLongClick = { menu = true }, divider = divider, trailing = {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (line.needsCategory > 0) AttentionDot()
                    if (line.approvals > 0) CountBadge(line.approvals)
                    Text(line.balanceText, style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1,
                        color = when { outside -> ur.muted; line.balance < 0 -> ur.negative; else -> ur.text })
                }
            })
        DropdownMenu(menu, { menu = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
            DropdownMenuItem(text = { Text(stringResource(R.string.account_edit)) }, leadingIcon = { Icon(Icons.Outlined.Edit, null) }, onClick = { menu = false; edit() })
            DropdownMenuItem(text = { Text(stringResource(R.string.reconcile_action)) }, leadingIcon = { Icon(Icons.Outlined.Lock, null) }, enabled = !line.archived, onClick = { menu = false; reconcile() })
            if (loan != null) DropdownMenuItem(text = { Text(stringResource(R.string.account_loan_details)) }, leadingIcon = { Icon(Icons.Outlined.Calculate, null) }, onClick = { menu = false; loan() })
            HorizontalDivider(color = ur.border)
            DropdownMenuItem(text = { Text(stringResource(R.string.common_move_up)) }, leadingIcon = { Icon(Icons.Outlined.ArrowUpward, null) }, enabled = moveUp != null, onClick = { menu = false; moveUp?.invoke() })
            DropdownMenuItem(text = { Text(stringResource(R.string.common_move_down)) }, leadingIcon = { Icon(Icons.Outlined.ArrowDownward, null) }, enabled = moveDown != null, onClick = { menu = false; moveDown?.invoke() })
            HorizontalDivider(color = ur.border)
            DropdownMenuItem(text = { Text(if (line.archived) stringResource(R.string.account_reopen) else stringResource(R.string.account_close)) },
                leadingIcon = { Icon(if (line.archived) Icons.Outlined.Unarchive else Icons.Outlined.Archive, null) }, onClick = { menu = false; toggleClosed() })
        }
    }
}

/** Adds an account as the view model's saveAccount does, with the note and net-worth choice the form also takes. */
private fun createAccount(budget: JSONObject, record: JSONObject) {
    val id = record.getString("id")
    require(budget.records("accounts").none { it.optString("id") == id }) { Words[R.string.account_exists] }
    budget.upsertRecord("accounts", record)
    // The payment group keeps the name every device and the YNAB import know it by, whatever the app's language.
    if (record.optString("kind") == "credit" && budget.records("categories").none { it.optString("paymentAccountId") == id }) {
        budget.upsertRecord("categories", JSONObject().put("id", UUID.randomUUID().toString()).put("name", record.getString("name"))
            .put("group", "Credit Card Payments").put("target", 0).put("paymentAccountId", id))
    }
}

@Composable
internal fun AccountDialog(original: JSONObject?, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    // An untyped account is read as the accounts list reads it: an imported debt stays a debt.
    val balanceNow = remember { original?.let { account -> vm.state.value.summary?.rows("accounts")?.firstOrNull { it.text("id") == account.text("id") }?.optLong("balance") ?: account.optLong("openingBalance") } ?: 0L }
    var name by remember { mutableStateOf(original?.text("name") ?: "") }
    val existingKind = original?.text("kind")?.ifBlank { null }
    var type by remember { mutableStateOf(if (original == null) "checking" else AccountTypes.forEditing(original, balanceNow)) }
    val kind = AccountTypes.kindFor(type)
    var balance by remember { mutableStateOf(amountText(original?.optLong("openingBalance") ?: 0)) }
    var note by remember { mutableStateOf(original?.text("note") ?: "") }
    var archived by remember { mutableStateOf(original?.optBoolean("archived") ?: false) }
    var netWorth by remember { mutableStateOf(original?.let(AccountTypes::inNetWorth) ?: true) }
    var error by remember { mutableStateOf<String?>(null) }
    val typeChoices = remember(existingKind, languageKey()) { accountTypes().filter { existingKind == null || AccountTypes.kindFor(it.first) == existingKind } }
    retainEditorDraft(vm, "account", openedBudget, JSONObject().put("name", name).put("type", type).put("balance", balance)
        .put("note", note).put("archived", archived).put("netWorth", netWorth))
    FormDialog(if (original == null) stringResource(R.string.account_add) else stringResource(R.string.account_edit), close,
        confirm = if (original == null) stringResource(R.string.common_add) else stringResource(R.string.common_save), onConfirm = {
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            require(name.isNotBlank()) { Words[R.string.account_name_required] }
            val opening = amountValue(balance)
            // Only tracking accounts can stay out of net worth.
            val counted = kind != "tracking" || netWorth
            if (original == null) {
                if (note.isBlank() && counted) vm.saveAccount(name = name, kind = kind, openingBalance = opening, accountType = type)
                else {
                    val record = JSONObject().put("id", UUID.randomUUID().toString()).put("name", name.trim()).put("kind", kind).put("openingBalance", opening)
                        .put("archived", false).put("accountType", type).put("note", note).put("netWorth", counted)
                    vm.editBudget { draft -> createAccount(draft, record) }
                }
            } else vm.upsert("accounts", original.copyJson().put("name", name.trim()).put("accountType", type).put("note", note).put("openingBalance", opening)
                .put("archived", archived).put("netWorth", counted))
        }.onSuccess { close() }.onFailure { error = it.message }
    }) {
        Field(name, { name = it }, stringResource(R.string.account_name))
        Choice(stringResource(R.string.account_type), type, typeChoices, { type = it })
        Notice(when (kind) { "cash" -> stringResource(R.string.account_kind_cash_help); "credit" -> stringResource(R.string.account_kind_credit_help); else -> stringResource(R.string.account_kind_tracking_help) },
            tone = "info", icon = iconForType(type, kind))
        Field(balance, { balance = it }, if (original == null) stringResource(R.string.account_current_balance, budget.text("currency")) else stringResource(R.string.account_opening_balance, budget.text("currency")), true)
        Text(listOfNotNull(if (AccountTypes.isLiability(type)) stringResource(R.string.account_owed_negative) else if (original == null) stringResource(R.string.account_debt_negative) else null,
            if (original == null) null else stringResource(R.string.account_opening_changes_history)).joinToString(" "),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
        if (original != null) Text(stringResource(R.string.account_kind_switch_help),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
        Field(note, { note = it }, stringResource(R.string.note_label), lines = 3)
        if (kind == "tracking" || original != null) ListCard {
            if (kind == "tracking") SwitchRow(stringResource(R.string.account_count_in_net_worth), netWorth, { netWorth = it }, divider = original != null,
                subtitle = if (netWorth) null else stringResource(R.string.account_count_in_net_worth_off))
            if (original != null) SwitchRow(stringResource(R.string.account_closed), archived, { archived = it }, subtitle = stringResource(R.string.account_closed_detail))
        }
        FormError(error)
    }
}

/** Reconciling, as YNAB asks it: is the cleared balance what the bank says? If not, enter the bank's. */
@Composable
internal fun ReconcileDialog(account: JSONObject, budget: JSONObject, summary: JSONObject?, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val currency = budget.text("currency")
    val total = remember(summary, account) { summary?.rows("accounts")?.firstOrNull { it.text("id") == account.text("id") } }
    var differs by remember { mutableStateOf(false) }
    var balance by remember { mutableStateOf(amountText(total?.optLong("cleared") ?: 0)) }
    var date by remember { mutableStateOf(LocalDate.now().toString()) }
    var preview by remember { mutableStateOf<Long?>(null) }
    var loading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val adjustmentId = remember { UUID.randomUUID().toString() }
    val contextKey = remember(budget, balance, date) { Any() }
    val currentContext by rememberUpdatedState(contextKey)
    val previews = rememberPreviewJobs(contextKey)
    LaunchedEffect(budget) { preview = null; loading = false }
    fun review() = runCatching {
        requireCurrentBudget(openedBudget, vm)
        val params = JSONObject().put("accountId", account.text("id")).put("balance", amountValue(balance)).put("date", date).put("adjustmentId", adjustmentId)
        loading = true
        previews.track(vm.query("reconcile", params, onError = { if (currentContext == contextKey) loading = false }) { candidate ->
            if (currentContext == contextKey) { preview = candidate.row("transactions", adjustmentId)?.optLong("amount") ?: 0; loading = false }
        })
    }.onFailure { error = it.message }
    retainEditorDraft(vm, "reconcile", openedBudget, JSONObject().put("account", account).put("balance", balance).put("date", date).put("differs", differs))
    FormDialog(stringResource(R.string.reconcile_title, account.text("name")), close,
        confirm = if (preview == null) stringResource(R.string.common_review) else if (preview == 0L) stringResource(R.string.reconcile_action) else stringResource(R.string.reconcile_adjust_finish), onConfirm = {
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            if (preview == null) review()
            else { vm.runCommand("reconcile", JSONObject().put("accountId", account.text("id")).put("balance", amountValue(balance)).put("date", date).put("adjustmentId", adjustmentId)); close() }
        }.onFailure { error = it.message }
    }, enabled = total != null && !loading && (differs || preview != null)) {
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
            Text(stringResource(R.string.reconcile_question_start), color = ur.text2, style = MaterialTheme.typography.bodyLarge)
            Text(money(total?.optLong("cleared") ?: 0, currency), fontSize = 34.sp, fontWeight = FontWeight.ExtraBold, style = Tabular, color = ur.text)
            Text(stringResource(R.string.reconcile_question_end), color = ur.text2, style = MaterialTheme.typography.bodyLarge)
        }
        if (!differs && preview == null) Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            SecondaryButton(stringResource(R.string.common_no), { differs = true }, Modifier.weight(1f))
            PrimaryButton(stringResource(R.string.common_yes), { balance = amountText(total?.optLong("cleared") ?: 0); review() }, Modifier.weight(1f), enabled = total != null && !loading, color = ur.positive)
        }
        if (differs) {
            Field(balance, { balance = it; preview = null; loading = false }, stringResource(R.string.reconcile_bank_balance), true)
            DateField(date, { date = it; preview = null; loading = false }, stringResource(R.string.reconcile_statement_date))
            Text(stringResource(R.string.reconcile_help), color = ur.muted, style = MaterialTheme.typography.bodySmall)
        }
        if (loading) LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
        preview?.let {
            if (it == 0L) Notice(stringResource(R.string.reconcile_match), tone = "positive", icon = Icons.Outlined.Lock)
            else Notice(stringResource(R.string.reconcile_adjustment, money(it, currency)), tone = "attention", icon = Icons.Outlined.WarningAmber)
        }
        FormError(error)
    }
}

/** The account types, as stored ids and their names in the interface's language. */
internal fun accountTypes(): List<Pair<String, String>> = listOf(
    "checking" to R.string.account_type_checking, "savings" to R.string.account_type_savings, "cash" to R.string.account_type_cash,
    "creditCard" to R.string.account_kind_credit, "lineOfCredit" to R.string.account_type_line_of_credit, "investment" to R.string.account_type_investment,
    "retirement" to R.string.account_type_retirement, "mortgage" to R.string.account_type_mortgage, "autoLoan" to R.string.account_type_auto_loan,
    "studentLoan" to R.string.account_type_student_loan, "personalLoan" to R.string.account_type_personal_loan, "medicalDebt" to R.string.account_type_medical_debt,
    "property" to R.string.account_type_property, "vehicle" to R.string.account_type_vehicle, "receivable" to R.string.account_type_receivable,
    "otherAsset" to R.string.account_type_other_asset, "otherDebt" to R.string.account_type_other_debt,
).map { (id, name) -> id to Words[name] }
