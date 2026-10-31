package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardReturn
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.AccountTypes
import app.urbudget.nativeapp.AppLanguage
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.NativePreferences
import app.urbudget.nativeapp.ScheduleSummary
import app.urbudget.nativeapp.rateIn
import app.urbudget.nativeapp.rateText
import app.urbudget.nativeapp.scheduleSummary
import app.urbudget.nativeapp.upcomingSchedules
import org.json.JSONObject
import java.time.LocalDate
import java.util.UUID

/** The pages More leads to: cards of rows, as Settings and Payees are on the desktop's phone layout. */
@Composable
internal fun MoreScreen(page: String, navigate: (String) -> Unit, budget: JSONObject, vm: BudgetViewModel,
                        importFile: () -> Unit, exportFile: () -> Unit, restore: (() -> Unit)?, edit: (String, JSONObject?) -> Unit) {
    when (page) {
        "Scheduled" -> SchedulesPage(budget, vm, edit)
        "Loan planner" -> LoanPlannerPage(budget, vm, edit)
        "Payees & rules" -> PayeesPage(budget, vm, edit)
        "Settings & data" -> SettingsPage(navigate, budget, vm, exportFile, importFile, restore, edit)
        else -> LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom)) {
            item {
                ListCard {
                    morePages.forEachIndexed { index, item -> ListRow(stringResource(item.title), subtitle = stringResource(item.detail), icon = item.icon, onClick = { navigate(item.key) }, divider = index < morePages.lastIndex) }
                }
            }
        }
    }
}

/** How often a schedule repeats: its stored value and how it reads, in the interface's language. */
internal fun frequencies(): List<Pair<String, String>> = listOf("never" to Words[R.string.repeat_never], "daily" to Words[R.string.repeat_daily],
    "weekly" to Words[R.string.repeat_weekly], "everyOtherWeek" to Words[R.string.repeat_every_other_week], "every4Weeks" to Words[R.string.repeat_every_4_weeks],
    "twiceAMonth" to Words[R.string.repeat_twice_a_month], "monthly" to Words[R.string.repeat_monthly], "everyOtherMonth" to Words[R.string.repeat_every_other_month],
    "every3Months" to Words[R.string.repeat_every_3_months], "every4Months" to Words[R.string.repeat_every_4_months], "twiceAYear" to Words[R.string.repeat_twice_a_year],
    "yearly" to Words[R.string.repeat_yearly], "everyOtherYear" to Words[R.string.repeat_every_other_year])

/** A schedule row, prepared away from composition. */
private data class ScheduleItem(val id: String, val schedule: JSONObject, val title: String, val subtitle: String, val amount: String, val positive: Boolean,
                                val due: Boolean, val paused: Boolean, val once: Boolean, val date: String)

private data class SchedulesModel(val lines: List<ScheduleItem>, val summary: ScheduleSummary, val goingOut: String, val comingIn: String)

private fun schedulesModel(budget: JSONObject, today: LocalDate): SchedulesModel {
    val currency = budget.text("currency")
    val frequencies = frequencies()
    val accounts = budget.rows("accounts").associate { it.text("id") to it.text("name") }
    val categories = budget.rows("categories").associate { it.text("id") to it.text("name") }
    val upcoming = upcomingSchedules(budget.rows("schedules"))
    val summary = scheduleSummary(upcoming, today)
    val now = today.toString()
    val lines = upcoming.map { schedule ->
        val t = schedule.optJSONObject("transaction") ?: JSONObject()
        val next = schedule.text("nextDate")
        val paused = schedule.optBoolean("paused")
        val due = !paused && next <= now
        val splits = t.rows("splits")
        val category = when {
            t.text("transferAccountId").isNotBlank() -> null
            splits.size > 1 -> Words[R.string.category_split]
            splits.firstOrNull()?.optBoolean("unassigned") == true -> Words[R.string.category_unassigned]
            else -> splits.firstOrNull()?.text("categoryId")?.let { categories[it] }
        }
        val frequency = schedule.text("frequency")
        ScheduleItem(schedule.text("id"), schedule,
            t.text("payee").ifBlank { accounts[t.text("transferAccountId")]?.let { Words[R.string.transfer_to_account, it] } ?: Words[R.string.schedule_unnamed] },
            Money.joined(listOfNotNull(if (due) Words[R.string.schedule_due_on, Money.date(next)] else Money.date(next), frequencies.firstOrNull { it.first == frequency }?.second ?: frequency,
                accounts[t.text("accountId")], category, if (paused) Words[R.string.schedule_paused] else null)),
            money(t.optLong("amount"), currency), t.optLong("amount") > 0, due, paused, frequency == "never" || frequency.isBlank(), Money.date(next))
    }
    return SchedulesModel(lines, summary, money(summary.goingOut, currency), money(summary.comingIn, currency))
}

/**
 * Scheduled: what's due in the next 30 days, then every schedule soonest first with paused ones
 * after (SchedulesModal.tsx). Each opens in the editor; its menu enters, skips, pauses or deletes it.
 */
@Composable
private fun SchedulesPage(budget: JSONObject, vm: BudgetViewModel, edit: (String, JSONObject?) -> Unit) {
    val model = rememberPrepared(budget, languageKey()) { schedulesModel(budget, LocalDate.now()) }
    var deleting by remember { mutableStateOf<ScheduleItem?>(null) }
    fun apply(line: ScheduleItem, skip: Boolean) = withCurrentBudget(budget, vm) {
        vm.runCommand("applySchedules", JSONObject().put("through", LocalDate.now().toString()).put("selected", line.id).put("skip", skip))
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom)) {
        item(key = "schedules:actions") {
            Row(Modifier.padding(bottom = 12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PrimaryButton(stringResource(R.string.schedule_add), { edit("schedule", null) }, Modifier.weight(1f), icon = Icons.Outlined.Add)
                SecondaryButton(stringResource(R.string.schedule_enter_all_due), { withCurrentBudget(budget, vm) { vm.runCommand("applySchedules", JSONObject().put("through", LocalDate.now().toString())) } },
                    Modifier.weight(1f), enabled = model?.lines?.any { it.due } == true)
            }
        }
        when {
            model == null -> item(key = "schedules:loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
            model.lines.isEmpty() -> item(key = "schedules:empty") {
                EmptyState(stringResource(R.string.schedules_empty_title), stringResource(R.string.schedules_empty_detail), stringResource(R.string.schedule_add)) { edit("schedule", null) }
            }
            else -> {
                item(key = "schedules:summary") {
                    ListCard(Modifier.padding(bottom = 8.dp)) {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                            Row {
                                Figure(stringResource(R.string.page_scheduled), if (model.summary.paused > 0) stringResource(R.string.schedules_active_paused, "${model.summary.active}", "${model.summary.paused}")
                                    else "${model.summary.active}", Modifier.weight(1f))
                                Figure(stringResource(R.string.schedules_due_soon), "${model.summary.dueSoon}", Modifier.weight(1f))
                            }
                            Row {
                                Figure(stringResource(R.string.schedules_going_out), model.goingOut, Modifier.weight(1f))
                                Figure(stringResource(R.string.schedules_coming_in), model.comingIn, Modifier.weight(1f), color = if (model.summary.comingIn > 0) ur.positive else ur.text)
                            }
                        }
                    }
                }
                item(key = "schedules:help") {
                    Text(stringResource(R.string.schedules_help),
                        Modifier.padding(start = 4.dp, end = 4.dp, bottom = 12.dp), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                }
                itemsIndexed(model.lines, key = { _, line -> "schedule:${line.id}" }, contentType = { _, _ -> "schedule" }) { index, line ->
                    ScheduleRow(line, Modifier.cardSegment(index, model.lines.size, cardFill()), index < model.lines.lastIndex,
                        edit = { edit("schedule", line.schedule) }, enter = { apply(line, false) }, skip = { apply(line, true) },
                        pause = { withCurrentBudget(budget, vm) { vm.upsert("schedules", JSONObject().put("id", line.id).put("paused", !line.paused)) } },
                        delete = { deleting = line })
                }
            }
        }
    }
    deleting?.let { line ->
        AlertDialog(onDismissRequest = { deleting = null }, containerColor = ur.surface, title = { Text(stringResource(R.string.schedule_delete_title)) },
            text = { Text(stringResource(R.string.schedule_delete_message, line.title)) },
            confirmButton = { TextButton({ deleting = null; withCurrentBudget(budget, vm) { vm.remove("schedules", line.id) } }) { Text(stringResource(R.string.common_delete), color = ur.negative) } },
            dismissButton = { TextButton({ deleting = null }) { Text(stringResource(R.string.common_cancel)) } })
    }
}

@Composable
private fun ScheduleRow(line: ScheduleItem, modifier: Modifier, divider: Boolean, edit: () -> Unit, enter: () -> Unit, skip: () -> Unit, pause: () -> Unit, delete: () -> Unit) {
    var menu by remember { mutableStateOf(false) }
    Box(modifier) {
        ListRow(line.title, Modifier.alpha(if (line.paused) .6f else 1f), subtitle = line.subtitle, value = line.amount, valueColor = if (line.positive) ur.positive else ur.text,
            icon = if (line.paused) Icons.Outlined.Pause else Icons.Outlined.Repeat, iconTint = if (line.due) ur.warning else if (line.paused) ur.muted else ur.accent,
            iconFill = if (line.due) ur.warningSoft else if (line.paused) ur.zeroSoft else ur.accentSoft,
            onClick = edit, onLongClick = { menu = true }, chevron = false, divider = divider, trailing = {
                Box {
                    IconButton({ menu = true }, Modifier.size(36.dp)) { Icon(Icons.Outlined.MoreHoriz, stringResource(R.string.common_actions_for, line.title), Modifier.size(20.dp), tint = ur.muted) }
                    DropdownMenu(menu, { menu = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.schedule_enter_now)) }, leadingIcon = { Icon(Icons.AutoMirrored.Outlined.KeyboardReturn, null) }, onClick = { menu = false; enter() })
                        DropdownMenuItem(text = { Text(if (line.once) stringResource(R.string.schedule_skip_once) else stringResource(R.string.schedule_skip_date, line.date)) }, leadingIcon = { Icon(Icons.Outlined.SkipNext, null) }, onClick = { menu = false; skip() })
                        DropdownMenuItem(text = { Text(if (line.paused) stringResource(R.string.schedule_resume) else stringResource(R.string.schedule_pause)) }, leadingIcon = { Icon(if (line.paused) Icons.Outlined.PlayArrow else Icons.Outlined.Pause, null) },
                            onClick = { menu = false; pause() })
                        DropdownMenuItem(text = { Text(stringResource(R.string.common_edit)) }, leadingIcon = { Icon(Icons.Outlined.Edit, null) }, onClick = { menu = false; edit() })
                        HorizontalDivider(color = ur.border)
                        DropdownMenuItem(text = { Text(stringResource(R.string.common_delete), color = ur.negative) }, leadingIcon = { Icon(Icons.Outlined.DeleteOutline, null, tint = ur.negative) },
                            onClick = { menu = false; delete() })
                    }
                }
            })
    }
}

/** A loan row: a debt with its details, or one still waiting for them. */
private data class LoanLine(val accountId: String, val record: JSONObject, val name: String, val subtitle: String, val owed: String, val type: String)

private fun loanLines(budget: JSONObject, summary: JSONObject?): List<LoanLine> {
    val currency = budget.text("currency")
    val balances = summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty()
    val loans = budget.rows("loans").associateBy { it.text("accountId") }
    val month = LocalDate.now().toString().take(7)
    return budget.rows("accounts").mapNotNull { account ->
        val id = account.text("id")
        val balance = balances[id] ?: account.optLong("openingBalance")
        val saved = loans[id]
        if (account.optBoolean("archived") && saved == null || !AccountTypes.hasLoan(account, balance, loans.keys)) return@mapNotNull null
        LoanLine(id, saved ?: JSONObject().put("accountId", id), account.text("name"),
            saved?.let { Words[R.string.loan_line_payment, money(it.optLong("monthlyPayment"), currency), Money.ltr("${rateText(rateIn(it, month))}%")] } ?: Words[R.string.loan_line_needs_details],
            money(maxOf(0L, -balance), currency), AccountTypes.infer(account, balance))
    }
}

/** Loan planner: each debt with its payment and what's owed; a tap opens its details and planner. */
@Composable
private fun LoanPlannerPage(budget: JSONObject, vm: BudgetViewModel, edit: (String, JSONObject?) -> Unit) {
    val summary = vm.state.collectAsStateWithLifecycle().value.summary
    val lines = rememberPrepared(budget, summary, languageKey()) { loanLines(budget, summary) }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom)) {
        item(key = "loans:new") { PrimaryButton(stringResource(R.string.loans_new_plan), { edit("loan", null) }, Modifier.fillMaxWidth().padding(bottom = 12.dp), icon = Icons.Outlined.Add) }
        when {
            lines == null -> item(key = "loans:loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
            lines.isEmpty() -> item(key = "loans:empty") {
                EmptyState(stringResource(R.string.loans_empty_title), stringResource(R.string.loans_empty_detail),
                    stringResource(R.string.loans_create_plan)) { edit("loan", null) }
            }
            else -> itemsIndexed(lines, key = { _, line -> "loan:${line.accountId}" }, contentType = { _, _ -> "loan" }) { index, line ->
                ListRow(line.name, Modifier.cardSegment(index, lines.size, cardFill()), subtitle = line.subtitle, value = line.owed, icon = iconForType(line.type, "tracking"),
                    iconTint = ur.text2, iconFill = ur.zeroSoft, onClick = { edit("loan", line.record) }, divider = index < lines.lastIndex)
            }
        }
        item(key = "loans:help") {
            Text(stringResource(R.string.loans_help),
                Modifier.padding(start = 4.dp, end = 4.dp, top = 12.dp), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
    }
}

/**
 * Settings as the desktop arranges them for a phone: the budget's name and formats, appearance and
 * the preferences that mean something here, then backups, imports and the rest.
 */
@Composable
private fun SettingsPage(navigate: (String) -> Unit, budget: JSONObject, vm: BudgetViewModel, exportFile: () -> Unit, importFile: () -> Unit, restore: (() -> Unit)?,
                         edit: (String, JSONObject?) -> Unit) {
    val context = LocalContext.current
    val preferences = remember { NativePreferences(context) }
    val prefs = LocalPreferences.current
    var theme by remember { mutableStateOf(preferences.theme) }
    var language by remember { mutableStateOf(AppLanguage.choice()) }
    var dialog by remember { mutableStateOf("") }
    val flagNames = remember(budget) { budget.optJSONObject("flagNames")?.let { names -> flagColors.mapNotNull { names.text(it).ifBlank { null } } }.orEmpty() }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item { SectionTitle(stringResource(R.string.tab_budget)) }
        item {
            ListCard {
                ListRow(stringResource(R.string.settings_budget_formats), subtitle = "${Money.own(budget.text("name"))} · ${formatSummary(budget)}", icon = Icons.Outlined.Tune, onClick = { dialog = "budget" }, divider = true)
                ListRow(stringResource(R.string.settings_flag_names), subtitle = Money.joined(flagNames).ifBlank { stringResource(R.string.settings_flag_names_hint) }, icon = Icons.Outlined.Flag, onClick = { dialog = "flags" })
            }
        }
        item { SectionTitle(stringResource(R.string.settings_appearance)) }
        item { Segmented(listOf("system" to stringResource(R.string.settings_theme_device), "light" to stringResource(R.string.settings_theme_light), "dark" to stringResource(R.string.settings_theme_dark)), theme, { theme = it; preferences.theme = it }) }
        // Language names are in their own language; System follows the phone (docs/LOCALIZATION.md).
        item { SectionTitle(stringResource(R.string.settings_language), stringResource(R.string.settings_language_detail)) }
        item {
            Segmented(listOf(AppLanguage.SYSTEM to stringResource(R.string.language_system), "en" to stringResource(R.string.language_english), "ar" to stringResource(R.string.language_arabic)),
                language, { if (it != language) { language = it; AppLanguage.choose(it) } })
        }
        item { SectionTitle(stringResource(R.string.settings_preferences), stringResource(R.string.settings_preferences_detail)) }
        item {
            ListCard {
                SwitchRow(stringResource(R.string.settings_show_needed), prefs.showNeeded, prefs::setShowNeeded, subtitle = stringResource(R.string.settings_show_needed_detail), divider = true)
                SwitchRow(stringResource(R.string.settings_new_cleared), prefs.newCleared, prefs::setNewCleared, subtitle = stringResource(R.string.settings_new_cleared_detail), divider = true)
                SwitchRow(stringResource(R.string.settings_open_scheduled), prefs.showUpcoming, prefs::setShowUpcoming, subtitle = stringResource(R.string.settings_open_scheduled_detail), divider = true)
                SwitchRow(stringResource(R.string.settings_enter_due), prefs.automaticSchedules, prefs::setAutomaticSchedules, subtitle = stringResource(R.string.settings_enter_due_detail))
            }
        }
        item {
            Choice(stringResource(R.string.settings_open_on), prefs.startPage, listOf("budget" to stringResource(R.string.tab_budget), "transactions" to stringResource(R.string.settings_all_transactions)), prefs::setStartPage)
            Text(stringResource(R.string.settings_open_on_detail), Modifier.padding(top = 6.dp, start = 4.dp), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
        item { SectionTitle(stringResource(R.string.settings_backups)) }
        item {
            ListCard {
                ListRow(stringResource(R.string.settings_export_backup), subtitle = stringResource(R.string.settings_export_backup_detail), icon = Icons.Outlined.FileUpload, onClick = exportFile, divider = true)
                ListRow(stringResource(R.string.settings_import_backup), subtitle = stringResource(R.string.settings_import_backup_detail), icon = Icons.Outlined.FileDownload, onClick = importFile, divider = restore != null)
                if (restore != null) ListRow(stringResource(R.string.welcome_restore_previous), subtitle = stringResource(R.string.settings_restore_previous_detail), icon = Icons.Outlined.Restore, onClick = restore)
            }
        }
        item { SectionTitle(stringResource(R.string.settings_imports)) }
        item {
            ListCard {
                ListRow(stringResource(R.string.import_bank_file), subtitle = stringResource(R.string.import_bank_file_detail), icon = Icons.Outlined.FileOpen, onClick = { edit("bankImport", null) }, divider = true)
                ListRow(stringResource(R.string.sms_title), subtitle = stringResource(R.string.sms_detail), icon = Icons.Outlined.Sms, onClick = { navigate("Bank messages") }, divider = true)
                ListRow(stringResource(R.string.import_ynab), subtitle = stringResource(R.string.import_ynab_detail), icon = Icons.Outlined.FileDownload, onClick = { edit("ynabImport", null) })
            }
        }
        item { SectionTitle(stringResource(R.string.tab_more)) }
        item {
            ListCard {
                ListRow(stringResource(R.string.page_library), subtitle = stringResource(R.string.page_library_detail), icon = Icons.Outlined.FolderOpen, onClick = { navigate("Budget library") }, divider = true)
                ListRow(stringResource(R.string.page_account_sync), subtitle = stringResource(R.string.page_account_sync_detail), icon = Icons.Outlined.CloudSync, onClick = { navigate("Account & sync") }, divider = true)
                ListRow(stringResource(R.string.settings_privacy), subtitle = stringResource(R.string.settings_privacy_detail), icon = Icons.Outlined.Shield,
                    onClick = { vm.service("account_open", JSONObject().put("next", "/privacy/app/")) })
            }
        }
        item {
            Text(stringResource(R.string.settings_footer, budget.text("name")),
                Modifier.padding(top = 8.dp, start = 4.dp, end = 4.dp), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
    }
    if (dialog == "budget") BudgetSettingsDialog(budget, vm) { dialog = "" }
    if (dialog == "flags") FlagNamesDialog(budget, vm) { dialog = "" }
}

/**
 * A schedule in the transaction editor, with how often it repeats. One that's due (dated today or
 * earlier) is entered as soon as it's saved, as the desktop does (Register.tsx's saveSchedule).
 */
@Composable
internal fun ScheduleDialog(original: JSONObject?, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val id = remember { original?.text("id") ?: UUID.randomUUID().toString() }
    var frequency by remember { mutableStateOf(original?.text("frequency") ?: "monthly") }
    var paused by remember { mutableStateOf(original?.optBoolean("paused") ?: false) }
    val transaction = remember { (original?.optJSONObject("transaction")?.copyJson() ?: JSONObject().put("id", "schedule:$id").put("status", "uncleared").put("amount", 0)).put("date", original?.text("nextDate") ?: LocalDate.now().plusDays(1).toString()) }
    // The saved schedule's next date, while waiting to enter it once the save is published.
    var entering by remember { mutableStateOf<String?>(null) }
    val state by vm.state.collectAsStateWithLifecycle()
    LaunchedEffect(entering, state.budget, state.error) {
        val due = entering ?: return@LaunchedEffect
        if (state.error != null) { close(); return@LaunchedEffect }
        val current = state.budget ?: return@LaunchedEffect
        if (current === openedBudget) return@LaunchedEffect
        val saved = current.rows("schedules").firstOrNull { it.text("id") == id }
        // Enter only the schedule as it was just saved, never one changed meanwhile.
        if (saved != null && !saved.optBoolean("paused") && saved.text("nextDate") == due)
            vm.runCommand("applySchedules", JSONObject().put("through", LocalDate.now().toString()).put("selected", id))
        close()
    }
    retainEditorDraft(vm, "schedule", openedBudget, JSONObject().put("id", id).put("frequency", frequency).put("paused", paused))
    TransactionDialog(transaction, budget, "", vm, close = { if (entering == null) close() }, saveOverride = { record ->
        val date = record.text("date")
        vm.upsert("schedules", (original?.copyJson() ?: JSONObject()).put("id", id).put("transaction", record).put("nextDate", date)
            .put("anchorDate", if (date == original?.text("nextDate")) original.text("anchorDate") else date).put("frequency", frequency).put("paused", paused))
        if (!paused && date <= LocalDate.now().toString()) entering = date
    }, dialogTitle = if (original == null) stringResource(R.string.schedule_new) else stringResource(R.string.schedule_edit), extraFields = {
        Choice(stringResource(R.string.schedule_repeat), frequency, remember(languageKey()) { frequencies() }, { frequency = it })
        ListCard { SwitchRow(stringResource(R.string.schedule_paused), paused, { paused = it }) }
        if (!paused) Text(stringResource(R.string.schedule_due_entered_on_save), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        if (original != null) TextButton({ withCurrentBudget(openedBudget, vm) { vm.remove("schedules", id); close() } }) { Text(stringResource(R.string.schedule_delete), color = ur.negative) }
    })
}
