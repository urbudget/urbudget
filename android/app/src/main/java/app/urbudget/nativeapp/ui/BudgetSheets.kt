package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.CategoryStatuses
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth

/** "5 Oct", with the interface's month names. */
private fun shortDay(iso: String?) = iso?.take(10)?.let { runCatching { Money.ltr(LocalDate.parse(it).format(Money.pattern("d MMM"))) }.getOrNull() }

/** One line of a category's moves in a month (desktop/src/moves.ts). */
internal data class MoveLine(val key: String, val at: String?, val what: String, val detail: String?, val amount: Long)

/**
 * A category's moves in a month, oldest first, each named by where the money came from or went.
 * When they don't add up to the month's Assigned, a first line makes up the difference.
 */
internal fun categoryMoves(budget: JSONObject, categoryId: String, month: String, assigned: Long): List<MoveLine> {
    val names = budget.rows("categories").associate { it.text("id") to it.text("name") }
    val lines = ArrayList<MoveLine>()
    budget.rows("moneyMoves").forEachIndexed { i, m ->
        if (m.text("month") != month) return@forEachIndexed
        val changes = m.rows("changes")
        val amount = changes.firstOrNull { it.text("categoryId") == categoryId }?.optLong("amount") ?: 0
        if (amount == 0L) return@forEachIndexed
        val other = changes.firstOrNull { it.text("categoryId") != categoryId }
        val what = when (m.text("kind")) {
            "quickAssign" -> m.text("detail").ifBlank { null }?.let { Words[R.string.move_quick_assign_named, it] } ?: Words[R.string.quick_assign]
            "merge" -> Words[R.string.move_merged_from, m.text("detail").ifBlank { Words[R.string.move_deleted_category] }]
            "move" -> if (other != null) { val name = names[other.text("categoryId")] ?: Words[R.string.move_deleted_category]; Words[if (amount > 0) R.string.move_moved_from else R.string.move_moved_to, name] }
                else Words[if (amount > 0) R.string.move_assigned else R.string.move_returned]
            "assign" -> Words[if (amount > 0) R.string.move_assigned else R.string.move_returned]
            else -> m.text("description")
        }
        lines.add(MoveLine("$i", m.text("at"), what, null, amount))
    }
    val recorded = lines.sumOf { it.amount }
    if (lines.isNotEmpty() && recorded != assigned) lines.add(0, MoveLine("earlier", null, Words[R.string.move_earlier], Words[R.string.move_not_itemised], assigned - recorded))
    return lines
}

/** A target's kind, as the desktop's inspector names it. */
private fun targetLabel(category: JSONObject): String {
    val goal = category.optJSONObject("goal") ?: return Words[R.string.target_kind_set_aside]
    return when (goal.text("goal_type")) {
        "NEED" -> Words[if (goal.optBoolean("goal_needs_whole_amount", true)) R.string.target_kind_set_aside else R.string.target_kind_refill]
        "TB", "TBD" -> Words[R.string.target_kind_balance]
        "DEBT" -> Words[R.string.target_kind_debt]
        "MF" -> Words[R.string.target_kind_monthly_funding]
        else -> Words[R.string.target_kind_set_aside]
    }
}

private fun cadenceLabel(goal: JSONObject?): String? {
    if (goal == null || goal.text("goal_type") != "NEED") return goal?.takeIf { it.text("goal_type") == "TBD" }?.let { Words[R.string.cadence_by_date, Money.date(it.text("goal_target_date"))] }
    val every = goal.optInt("goal_cadence_frequency", 1)
    return when (val cadence = goal.optInt("goal_cadence", 1)) {
        0 -> Words[R.string.cadence_once_by, Money.date(goal.text("goal_target_date"))]
        1 -> if (every > 1) Words.count(R.plurals.cadence_every_months, every.toLong()) else Words[R.string.cadence_each_month]
        2 -> if (every > 1) Words.count(R.plurals.cadence_every_weeks, every.toLong()) else Words[R.string.cadence_each_week]
        13 -> if (every > 1) Words.count(R.plurals.cadence_every_years, every.toLong()) else Words[R.string.cadence_each_year]
        14 -> Words[R.string.cadence_every_two_years]
        else -> Words.count(R.plurals.cadence_every_months, (cadence - 1).toLong())
    }
}

/**
 * A category's details, as a sheet: what's available and why, Assigned edited on the keypad (with
 * YNAB's per-category shortcuts), this month's moves, the note, and the category's other editors
 * a step further in. A card payment category says what's set aside and offers to make a payment.
 */
@Composable
internal fun CategorySheet(category: JSONObject, budget: JSONObject, total: JSONObject?, summary: JSONObject?, month: String, vm: BudgetViewModel,
                           startAssigning: Boolean, close: () -> Unit, move: (String) -> Unit, transactions: (RegisterTarget) -> Unit, payCard: (String) -> Unit) {
    val currency = budget.text("currency")
    val id = category.text("id")
    val toast = LocalToast.current
    val state = remember(category, total, month) { CategoryStatuses.of(category, total, month) }
    val language = languageKey()
    val words = remember(category, total, state, language) { CategoryStatuses.targetWords(category, total, state, budget.text("dateFormat") == "mdy") }
    var page by remember { mutableStateOf("main") }
    var editingAssigned by remember { mutableStateOf(startAssigning) }
    var noting by remember { mutableStateOf(false) }
    val assigned = total?.optLong("assigned") ?: 0
    val available = total?.optLong("available") ?: 0
    // Keyed on the category, so a sync arriving while typing doesn't wipe the keypad.
    val pad = rememberPad(assigned, id)
    var error by remember { mutableStateOf<String?>(null) }
    if (editingAssigned) retainEditorDraft(vm, "assignment", budget, JSONObject().put("categoryId", id).put("month", month).put("expression", pad.text))
    val paymentAccount = category.text("paymentAccountId")
    val payment = paymentAccount.isNotBlank()
    val moves = remember(budget, id, month, assigned, language) { categoryMoves(budget, id, month, assigned) }
    // What the per-category Quick assign options would set Assigned to, worked out while the keypad is open.
    val suggestions = remember(id, month) { mutableStateMapOf<String, Long>() }
    val jobs = rememberPreviewJobs(id, month, editingAssigned)
    LaunchedEffect(editingAssigned, budget, month) {
        if (!editingAssigned) return@LaunchedEffect
        listOf("assignedLastMonth", "spentLastMonth", "averageAssigned", "averageSpent").forEach { mode ->
            jobs.track(vm.query("workflow", quickAssignParams(budget, month, mode, listOf(id), limit = false).put("previewMonth", month), onError = {}) { result ->
                result.optJSONObject("summary")?.rows("categories")?.firstOrNull { it.text("id") == id }?.optLong("assigned")?.let { suggestions[mode] = it }
            })
        }
    }
    if (page != "main") {
        CategoryEditor(page, category, budget, total, month, vm, back = { page = "main"; error = null }, close = close)
        return
    }
    UrSheet(close) {
        Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(bottom = if (editingAssigned) 0.dp else 20.dp)) {
            Row(Modifier.fillMaxWidth().padding(start = 20.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(category.text("name"), style = MaterialTheme.typography.headlineSmall, color = ur.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(if (category.optBoolean("hidden")) stringResource(R.string.category_group_hidden, category.text("group")) else category.text("group"), style = MaterialTheme.typography.bodyMedium, color = ur.muted)
                }
                CategoryMenu(payment, category.optBoolean("hidden"), { action ->
                    if (action == "hide" || action == "show") withCurrentBudget(budget, vm) {
                        vm.upsert("categories", category.copyJson().put("hidden", action == "hide"))
                        toast(Words[if (action == "hide") R.string.category_hid else R.string.category_shown_again, category.text("name")], true)
                        close()
                    } else page = action
                })
                IconButton(close) { Icon(Icons.Outlined.Close, stringResource(R.string.common_close), tint = ur.muted) }
            }
            val colors = statusColors(state.status)
            Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 14.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(money(available, currency), color = if (state.status == "zero") ur.text else colors.ink, fontSize = 36.sp, fontWeight = FontWeight.ExtraBold, style = Tabular)
                Text(when {
                    state.status == "negative" -> stringResource(R.string.category_overspent)
                    state.status == "credit" -> stringResource(R.string.category_overspent_card)
                    payment -> stringResource(R.string.card_set_aside)
                    else -> stringResource(R.string.budget_available)
                }, color = ur.muted, style = MaterialTheme.typography.bodyMedium)
                if (words != null) Text(words.text(money(words.amount ?: 0, currency)),
                    Modifier.padding(top = 6.dp), fontWeight = FontWeight.SemiBold, style = MaterialTheme.typography.bodyMedium,
                    color = when { state.status == "underfunded" -> ur.warning; state.eventually > 0 -> ur.muted; else -> ur.positive })
                if (state.progress != null && !state.snoozed) TargetBar(state.progress, state.status, Modifier.padding(top = 10.dp).fillMaxWidth(.7f))
            }
            ListCard(Modifier.padding(horizontal = Gutter)) {
                ListRow(stringResource(R.string.category_carried), value = money(total?.optLong("carried") ?: 0, currency), divider = true)
                Column(Modifier.fillMaxWidth().background(if (editingAssigned) ur.accentSoft else Color.Transparent)) {
                    ListRow(stringResource(R.string.category_assigned_in, shortMonthLabel(month)), onClick = { editingAssigned = true; pad.set(assigned) }, chevron = false,
                        trailing = {
                            if (editingAssigned) Text(if (pad.isSum) Money.ltr(pad.display()) else money(pad.cents ?: 0, currency), color = ur.accent, fontSize = 20.sp, fontWeight = FontWeight.ExtraBold, style = Tabular)
                            else Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(money(assigned, currency), style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, color = ur.accent)
                                Icon(Icons.Outlined.Edit, null, Modifier.padding(start = 6.dp).size(16.dp), tint = ur.accent)
                            }
                        }, divider = true)
                }
                // A card payment's activity is the card's own register for the month.
                val (from, through) = monthBounds(month)
                ListRow(stringResource(R.string.budget_activity), value = money(total?.optLong("activity") ?: 0, currency),
                    onClick = { transactions(if (payment) RegisterTarget(account = paymentAccount, from = from, through = through) else RegisterTarget(category = id, from = from, through = through)) },
                    divider = (total?.optLong("upcoming") ?: 0) > 0 || category.optLong("target") > 0)
                if ((total?.optLong("upcoming") ?: 0) > 0) ListRow(stringResource(R.string.category_upcoming), subtitle = stringResource(R.string.category_upcoming_detail), value = money(total!!.optLong("upcoming"), currency), divider = category.optLong("target") > 0)
                if (category.optLong("target") > 0) ListRow(stringResource(R.string.target_label), subtitle = listOfNotNull(targetLabel(category), cadenceLabel(category.optJSONObject("goal"))).joinToString(" "),
                    value = money(category.optLong("target"), currency), onClick = { page = "target" })
            }
            error?.let { Box(Modifier.padding(horizontal = Gutter, vertical = 8.dp)) { FormError(it) } }
            if (!editingAssigned) {
                if (payment) PrimaryButton(stringResource(R.string.card_make_payment), { payCard(paymentAccount) }, Modifier.fillMaxWidth().padding(horizontal = Gutter).padding(top = 14.dp), icon = Icons.Outlined.CreditCard)
                Row(Modifier.fillMaxWidth().padding(horizontal = Gutter, vertical = 14.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    SecondaryButton(if (available < 0) stringResource(R.string.category_cover) else stringResource(R.string.move_money), { move(if (available < 0) "from" else "to") }, Modifier.weight(1f), icon = Icons.Outlined.SwapHoriz)
                    SecondaryButton(if (category.optLong("target") > 0 || category.optJSONObject("goal") != null) stringResource(R.string.target_edit) else stringResource(R.string.target_create), { page = "target" },
                        Modifier.weight(1f), icon = Icons.Outlined.TrackChanges)
                }
                ListCard(Modifier.padding(horizontal = Gutter)) {
                    ListRow(if (category.text("note").isBlank()) stringResource(R.string.note_add) else stringResource(R.string.note_label), subtitle = category.text("note").ifBlank { null },
                        icon = Icons.AutoMirrored.Outlined.Notes, iconTint = ur.text2, iconFill = ur.zeroSoft,
                        titleColor = if (category.text("note").isBlank()) ur.accent else ur.text, onClick = { noting = true })
                }
                if (moves.isNotEmpty()) {
                    Text(stringResource(R.string.category_moves_this_month), Modifier.padding(start = Gutter + 4.dp, top = 16.dp, bottom = 6.dp), style = MaterialTheme.typography.titleSmall, color = ur.text)
                    ListCard(Modifier.padding(horizontal = Gutter)) {
                        val shownMoves = moves.takeLast(5)
                        shownMoves.forEachIndexed { index, line ->
                            ListRow(line.what, subtitle = line.detail ?: shortDay(line.at), value = Money.signed(line.amount, currency),
                                valueColor = if (line.amount < 0) ur.negative else ur.text, divider = index < shownMoves.lastIndex || moves.size > 5)
                        }
                        if (moves.size > 5) ListRow(stringResource(R.string.show_all_count, "${moves.size}"), titleColor = ur.accent, onClick = { page = "history" })
                    }
                }
            } else {
                Row(Modifier.fillMaxWidth().horizontalScrollRow().padding(horizontal = Gutter, vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically) {
                    if (state.needed > 0 && !payment) FilterPill(stringResource(R.string.assign_needed, Money.signed(state.needed, currency)), false, { pad.set(assigned + state.needed) })
                    if (available < 0) FilterPill(stringResource(R.string.assign_cover, Money.signed(-available, currency)), false, { pad.set(assigned - available) })
                    listOf("assignedLastMonth" to R.string.range_last_month, "spentLastMonth" to R.string.assign_spent_last_month, "averageAssigned" to R.string.assign_average_assigned,
                        "averageSpent" to R.string.assign_average_spent).forEach { (mode, label) ->
                        suggestions[mode]?.takeIf { it != assigned }?.let { value -> FilterPill(stringResource(R.string.assign_suggestion, stringResource(label), money(value, currency)), false, { pad.set(value) }) }
                    }
                    TextButton({ editingAssigned = false; error = null }) { Text(stringResource(R.string.common_cancel), color = ur.muted) }
                }
            }
        }
        if (editingAssigned) AmountKeypad(pad, onDone = {
            val cents = pad.cents
            if (cents == null) { error = Words[R.string.sum_finish_before_saving]; return@AmountKeypad }
            // The sheet stays open on the new figures, as the desktop's inspector does.
            withCurrentBudget(budget, vm) {
                vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "assignAmount").put("month", month).put("categoryId", id).put("amount", cents)))
                editingAssigned = false
                error = null
            }
        }, doneLabel = stringResource(R.string.assign_action))
    }
    if (noting) NoteDialog(category, budget, vm) { noting = false }
}

/** A row that scrolls sideways when its chips don't fit. */
@Composable
private fun Modifier.horizontalScrollRow() = this.then(Modifier.horizontalScroll(rememberScrollState()))

/** The first and last day of a month, for limiting the register to it. */
internal fun monthBounds(month: String): Pair<String, String> = runCatching {
    val ym = YearMonth.parse(month); ym.atDay(1).toString() to ym.atEndOfMonth().toString()
}.getOrDefault("" to "")

@Composable
private fun CategoryMenu(payment: Boolean, hidden: Boolean, open: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        IconButton({ expanded = true }) { Icon(Icons.Outlined.MoreHoriz, stringResource(R.string.category_actions), tint = ur.text2) }
        DropdownMenu(expanded, { expanded = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
            val items = buildList {
                if (!payment) add(Triple("details", R.string.category_details, Icons.Outlined.Edit))
                add(Triple("organize", R.string.category_organize, Icons.Outlined.SwapVert))
                if (!payment) add(Triple("renameGroup", R.string.group_rename_or_merge, Icons.Outlined.DriveFileRenameOutline))
                add(Triple("history", R.string.category_moves_menu, Icons.Outlined.History))
                add(if (hidden) Triple("show", R.string.category_show, Icons.Outlined.Visibility) else Triple("hide", R.string.category_hide, Icons.Outlined.VisibilityOff))
            }
            items.forEach { (id, label, icon) -> DropdownMenuItem(text = { Text(stringResource(label)) }, leadingIcon = { Icon(icon, null) }, onClick = { open(id); expanded = false }) }
            if (!payment) {
                HorizontalDivider(color = ur.border)
                DropdownMenuItem(text = { Text(stringResource(R.string.category_delete), color = ur.negative) }, leadingIcon = { Icon(Icons.Outlined.DeleteOutline, null, tint = ur.negative) },
                    onClick = { open("delete"); expanded = false })
            }
        }
    }
}

@Composable
private fun NoteDialog(category: JSONObject, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    var note by remember { mutableStateOf(category.text("note")) }
    FormDialog(stringResource(R.string.note_label), close, onConfirm = { withCurrentBudget(budget, vm) { vm.upsert("categories", category.copyJson().put("note", note)) }; close() }) {
        Field(note, { note = it }, category.text("name"), lines = 6)
    }
}

/** The target kinds the desktop offers, in its words. Imported kinds stay available only to targets that already use them. */
private val targetBehaviours = listOf(
    Triple("set", R.string.target_behaviour_set, R.string.target_behaviour_set_hint),
    Triple("refill", R.string.target_behaviour_refill, R.string.target_behaviour_refill_hint),
    Triple("TB", R.string.target_behaviour_balance, R.string.target_behaviour_balance_hint),
    Triple("TBD", R.string.target_behaviour_by_date, R.string.target_behaviour_by_date_hint),
)

/** The category's editors a step in from its details: target, name and note, order, group, history, delete. */
@Composable
private fun CategoryEditor(section: String, category: JSONObject, budget: JSONObject, total: JSONObject?, month: String, vm: BudgetViewModel,
                           back: () -> Unit, close: () -> Unit) {
    if (section == "target") { TargetEditor(category, budget, month, vm, back, close); return }
    val openedBudget = remember { budget }
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val categories = remember(budget, category) { budget.rows("categories").filter { it.text("id") != category.text("id") } }
    val unassignedName = stringResource(R.string.category_unassigned)
    val replacements = remember(categories, unassignedName) { listOf(Unassigned to unassignedName) + categories.filter { it.text("paymentAccountId").isBlank() }.map { it.text("id") to it.text("name") } }
    val groups = remember(budget) { groupNames(budget) }
    var note by remember { mutableStateOf(category.text("note")) }
    var name by remember { mutableStateOf(category.text("name")) }
    var group by remember { mutableStateOf(category.text("group")) }
    var hidden by remember { mutableStateOf(category.optBoolean("hidden")) }
    var before by remember { mutableStateOf("") }
    var groupBefore by remember { mutableStateOf("__keep") }
    var renameGroup by remember { mutableStateOf(category.text("group")) }
    var replacement by remember { mutableStateOf(Unassigned) }
    var deleteConfirmed by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    // Whether anything refers to the category: an unused one is deleted without choosing where its history goes.
    val used = remember(budget, category) {
        val id = category.text("id")
        budget.rows("assignments").any { it.text("categoryId") == id && it.optLong("amount") != 0L } ||
            budget.rows("transactions").any { t -> t.rows("splits").any { it.text("categoryId") == id } }
    }
    val moves = remember(budget, category, month, total, languageKey()) { categoryMoves(budget, category.text("id"), month, total?.optLong("assigned") ?: 0) }
    if (section != "history") retainEditorDraft(vm, "category:$section", openedBudget,
        JSONObject().put("categoryId", category.text("id")).put("name", name).put("note", note).put("group", group)
            .put("hidden", hidden).put("before", before).put("groupBefore", groupBefore).put("renameGroup", renameGroup).put("replacement", replacement))
    val save: (() -> Unit)? = if (section == "history") null else ({
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            when (section) {
                "organize" -> vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "organizeCategory").put("id", category.text("id")).put("group", group).put("beforeId", before.ifBlank { null } ?: JSONObject.NULL).also { if (groupBefore != "__keep") it.put("groupBefore", groupBefore.ifBlank { null } ?: JSONObject.NULL) }))
                "renameGroup" -> {
                    require(renameGroup.isNotBlank()) { Words[R.string.group_name_required] }
                    vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "renameGroup").put("from", category.text("group")).put("to", renameGroup.trim())))
                    if (renameGroup.trim() in groups && renameGroup.trim() != category.text("group")) toast(Words[R.string.group_merged, category.text("group"), renameGroup.trim()], true)
                }
                "delete" -> {
                    require(!used || deleteConfirmed) { Words[R.string.category_delete_confirm_required] }
                    vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "deleteCategory").put("id", category.text("id")).put("replacement", if (used) replacement else Unassigned)))
                    toast(Words[R.string.deleted_named, category.text("name")], true)
                }
                "details" -> { require(name.isNotBlank() && group.isNotBlank()) { Words[R.string.category_name_group_required] }; vm.upsert("categories", category.copyJson().put("name", name.trim()).put("group", group.trim()).put("note", note).put("hidden", hidden)) }
            }
        }.onSuccess { close() }.onFailure { error = it.message }
    })
    val title = when (section) { "details" -> stringResource(R.string.category_details); "organize" -> stringResource(R.string.category_organize)
        "renameGroup" -> stringResource(R.string.group_rename); "history" -> stringResource(R.string.category_money_moves); else -> stringResource(R.string.category_delete) }
    val endOfGroup = stringResource(R.string.organize_end_of_group)
    val keepPosition = stringResource(R.string.organize_keep_group_position)
    val endOfGroups = stringResource(R.string.organize_end_of_groups)
    UrSheet(close) {
        SheetHeader(title, close, if (section == "delete") stringResource(R.string.common_delete) else stringResource(R.string.common_save), save, enabled = section != "delete" || !used || deleteConfirmed, onBack = back)
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = screenHeight() * .8f),
            contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (section == "history") {
                if (moves.isEmpty()) item { Text(stringResource(R.string.category_no_moves), color = ur.muted) }
                else item {
                    ListCard {
                        moves.forEachIndexed { index, line ->
                            ListRow(line.what, subtitle = line.detail ?: shortDay(line.at), value = Money.signed(line.amount, currency),
                                valueColor = if (line.amount < 0) ur.negative else ur.text, divider = index < moves.lastIndex)
                        }
                    }
                }
                item { Text(stringResource(R.string.category_moves_add_up, monthLabel(month)), color = ur.muted, style = MaterialTheme.typography.bodySmall) }
            } else item {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    when (section) {
                        "details" -> {
                            Field(name, { name = it }, stringResource(R.string.name_label))
                            Choice(stringResource(R.string.group_label), group, groups.map { it to it } + if (group !in groups) listOf(group to group) else emptyList(), { group = it })
                            Field(note, { note = it }, stringResource(R.string.note_label), lines = 4)
                            ListCard { SwitchRow(stringResource(R.string.category_hidden), hidden, { hidden = it }, subtitle = stringResource(R.string.category_hidden_detail)) }
                        }
                        "organize" -> {
                            Choice(stringResource(R.string.group_label), group, groups.map { it to it }, { group = it; before = "" })
                            Choice(stringResource(R.string.organize_place_before), before, remember(categories, group, endOfGroup) { listOf("" to endOfGroup) + categories.filter { it.text("group") == group }.map { it.text("id") to it.text("name") } }, { before = it })
                            Choice(stringResource(R.string.organize_place_group_before), groupBefore, remember(budget, group, keepPosition, endOfGroups) { listOf("__keep" to keepPosition, "" to endOfGroups) + groups.filter { it != group }.map { it to it } }, { groupBefore = it })
                        }
                        "renameGroup" -> { Field(renameGroup, { renameGroup = it }, stringResource(R.string.group_name)); Text(stringResource(R.string.group_rename_help), color = ur.muted, style = MaterialTheme.typography.bodySmall) }
                        "delete" -> if (!used) {
                            Text(stringResource(R.string.category_delete_unused), color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                        } else {
                            Choice(stringResource(R.string.category_delete_move_history), replacement, replacements, { replacement = it })
                            Text(stringResource(R.string.category_delete_help), color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                            CheckRow(stringResource(R.string.category_delete_confirm), deleteConfirmed, { deleteConfirmed = it })
                        }
                    }
                    FormError(error)
                }
            }
        }
    }
}

/**
 * Creating or changing a target, as the desktop's editor does: the four behaviours, the amount on
 * the keypad, how often it repeats and from when, and snoozing for this month.
 */
@Composable
private fun TargetEditor(category: JSONObject, budget: JSONObject, month: String, vm: BudgetViewModel, back: () -> Unit, close: () -> Unit) {
    val openedBudget = remember { budget }
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val existing = category.optJSONObject("goal")
    val existingType = existing?.text("goal_type")
    var behaviour by remember { mutableStateOf(when { existing == null -> "set"; existingType == "NEED" -> if (existing.optBoolean("goal_needs_whole_amount", true)) "set" else "refill"; else -> existingType ?: "set" }) }
    val pad = rememberPad(category.optLong("target").takeIf { it > 0 }, category.text("id"))
    var keypad by remember { mutableStateOf(category.optLong("target") <= 0) }
    var due by remember { mutableStateOf(existing?.text("goal_target_date")?.ifBlank { null } ?: "$month-28") }
    var cadence by remember { mutableStateOf((existing?.optInt("goal_cadence", 1) ?: 1).toString()) }
    var interval by remember { mutableStateOf((existing?.optInt("goal_cadence_frequency", 1) ?: 1).toString()) }
    var weekday by remember { mutableStateOf((existing?.optInt("goal_day", 0) ?: 0).coerceIn(0, 6).toString()) }
    var start by remember { mutableStateOf(existing?.text("goal_creation_month")?.take(7)?.ifBlank { null } ?: month) }
    var snoozed by remember { mutableStateOf(existing?.text("goal_snoozed_at")?.startsWith(month) ?: false) }
    var error by remember { mutableStateOf<String?>(null) }
    val need = behaviour == "set" || behaviour == "refill"
    val payment = category.text("paymentAccountId").isNotBlank()
    val options = targetBehaviours + listOfNotNull(
        if (existingType == "MF") Triple("MF", R.string.target_kind_monthly_funding, R.string.target_behaviour_monthly_funding_hint) else null,
        if (existingType == "DEBT") Triple("DEBT", R.string.target_behaviour_debt, R.string.target_behaviour_debt_hint) else null,
    )
    retainEditorDraft(vm, "target", openedBudget, JSONObject().put("categoryId", category.text("id")).put("behaviour", behaviour)
        .put("amountExpression", pad.text).put("due", due).put("cadence", cadence).put("interval", interval).put("weekday", weekday)
        .put("start", start).put("snoozed", snoozed))
    val save = {
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            val amount = pad.cents ?: error(Words[R.string.transaction_finish_sum])
            require(amount > 0) { Words[R.string.target_amount_required] }
            val type = if (need) "NEED" else behaviour
            val sameGoal = existingType == type
            val goal = (if (sameGoal) existing!!.copyJson() else JSONObject().put("goal_creation_month", "${YearMonth.now()}-01")).put("goal_type", type)
            goal.put("goal_target", Math.multiplyExact(amount, 10L))
            if (type != "TB") goal.put("goal_creation_month", "${YearMonth.parse(start)}-01")
            if (month == YearMonth.now().toString() && !payment) goal.put("goal_snoozed_at", if (snoozed) "$month-01" else JSONObject.NULL)
            if (type == "NEED") {
                val every = interval.toIntOrNull() ?: 1
                require(every in 1..100) { Words[R.string.target_interval_range] }
                goal.put("goal_cadence", cadence.toInt()).put("goal_cadence_frequency", every).put("goal_needs_whole_amount", behaviour == "set")
                if (cadence == "2") { goal.put("goal_day", weekday.toInt()); goal.remove("goal_target_date"); goal.remove("goal_target_month") }
                else if (cadence != "1" || every > 1) { LocalDate.parse(due); goal.put("goal_target_date", due); goal.remove("goal_target_month"); goal.remove("goal_day") }
                else { goal.remove("goal_target_date"); goal.remove("goal_target_month"); goal.remove("goal_day") }
            } else if (type == "TBD") { LocalDate.parse(due); goal.put("goal_target_date", due) }
            vm.upsert("categories", category.copyJson().put("target", amount).put("goal", goal))
            toast(Words[R.string.target_set_toast, category.text("name"), money(amount, currency)], true)
        }.onSuccess { close() }.onFailure { error = it.message }
    }
    UrSheet(close) {
        SheetHeader(if (existing != null || category.optLong("target") > 0) stringResource(R.string.target_edit) else stringResource(R.string.target_create), close, stringResource(R.string.common_save), { save() }, onBack = back)
        LazyColumn(Modifier.fillMaxWidth().weight(1f, fill = false), contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item(key = "amount") {
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(if (keypad) ur.accentSoft else ur.surface2).clickable { keypad = true; pad.fresh = true }.padding(vertical = 14.dp),
                    horizontalAlignment = Alignment.CenterHorizontally) {
                    PadAmount(pad, currency, color = if (keypad) ur.accent else ur.text)
                    Text(if (behaviour == "TB" || behaviour == "TBD") stringResource(R.string.target_balance_to_reach) else stringResource(R.string.target_amount_each_period), color = ur.muted, style = MaterialTheme.typography.bodySmall)
                }
            }
            item(key = "behaviours") {
                ListCard {
                    options.forEachIndexed { index, (id, label, hint) ->
                        Row(Modifier.fillMaxWidth().clickable(role = Role.RadioButton) { behaviour = id }.padding(horizontal = 12.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                            RadioButton(behaviour == id, { behaviour = id }, colors = RadioButtonDefaults.colors(selectedColor = ur.accent))
                            Column(Modifier.weight(1f).padding(start = 4.dp)) {
                                Text(stringResource(label), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, color = ur.text)
                                Text(stringResource(hint), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                            }
                        }
                        if (index < options.lastIndex) Hairline(start = 52.dp)
                    }
                }
            }
            item(key = "cadence") {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    if (need) {
                        val language = languageKey()
                        val cadences = remember(language) {
                            listOf("0" to Words[R.string.cadence_option_once], "1" to Words[R.string.repeat_monthly], "2" to Words[R.string.repeat_weekly], "13" to Words[R.string.repeat_yearly]) +
                                (3..12).map { it.toString() to Words.count(R.plurals.cadence_option_every_months, (it - 1).toLong()) } + listOf("14" to Words[R.string.cadence_option_every_two_years])
                        }
                        Choice(stringResource(R.string.target_repeats), cadence, cadences, { cadence = it; interval = "1" })
                        if (cadence in listOf("1", "2", "13")) Field(interval, { interval = it }, stringResource(when (cadence) { "2" -> R.string.target_interval_weeks; "13" -> R.string.target_interval_years; else -> R.string.target_interval_months }), true)
                        // Day 0 is Sunday, as the desktop and YNAB number them.
                        if (cadence == "2") Choice(stringResource(R.string.target_day_of_week), weekday, remember(language) { (0..6).map { i -> i.toString() to Money.weekday(java.time.DayOfWeek.of(if (i == 0) 7 else i)) } }, { weekday = it })
                        else if (cadence != "1" || interval != "1") DateField(due, { due = it }, if (cadence == "0") stringResource(R.string.target_needed_by) else stringResource(R.string.target_first_due))
                    }
                    if (behaviour == "TBD") DateField(due, { due = it }, stringResource(R.string.target_reach_by))
                    if (behaviour != "TB") MonthField(start, { start = it }, stringResource(R.string.target_starts))
                    if (month == YearMonth.now().toString() && !payment && (existing != null || category.optLong("target") > 0)) ListCard { SwitchRow(stringResource(R.string.target_snooze), snoozed, { snoozed = it }, subtitle = stringResource(R.string.target_snooze_detail)) }
                    FormError(error)
                    if (existing != null || category.optLong("target") > 0) TextButton({
                        withCurrentBudget(openedBudget, vm) { vm.saveTarget(category.text("id"), null); toast(Words[R.string.target_removed_toast, category.text("name")], true); close() }
                    }) { Text(stringResource(R.string.target_remove), color = ur.negative) }
                }
            }
        }
        if (keypad) AmountKeypad(pad, { keypad = false })
    }
}

/** A month field: the month, tapping opens the month sheet. */
@Composable
internal fun MonthField(month: String, onChange: (String) -> Unit, label: String) {
    var picking by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(fieldFill()).clickable { picking = true }
        .padding(start = 16.dp, end = 12.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodySmall, color = ur.muted)
            Text(monthLabel(month), style = MaterialTheme.typography.bodyLarge, color = ur.text)
        }
        Icon(Icons.Outlined.CalendarMonth, null, tint = ur.muted)
    }
    if (picking) MonthPickerSheet(month, { onChange(it); picking = false }) { picking = false }
}

/**
 * Moving money, as a sheet: from one category (or Unassigned) to another, the amount on the keypad.
 * Covering overspending opens with the overspent amount, taken from Unassigned when it has money.
 */
@Composable
internal fun MoveMoneySheet(request: MoveRequest, budget: JSONObject, summary: JSONObject?, month: String, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val totals = remember(summary) { summary?.rows("categories")?.associateBy { it.text("id") }.orEmpty() }
    val names = remember(budget) { budget.rows("categories").associate { it.text("id") to it.text("name") } }
    fun available(id: String) = if (id == Unassigned) summary?.optLong("readyToAssign") ?: 0 else totals[id]?.optLong("available") ?: 0
    val unassignedName = stringResource(R.string.category_unassigned)
    val chooseCategory = stringResource(R.string.category_choose_a)
    fun name(id: String) = if (id == Unassigned) unassignedName else names[id] ?: chooseCategory
    val start = available(request.category)
    val rta = summary?.optLong("readyToAssign") ?: 0
    var from by remember { mutableStateOf(when { request.direction == "to" -> request.category; start < 0 && rta > 0 && request.category != Unassigned -> Unassigned; else -> "" }) }
    var to by remember { mutableStateOf(if (request.direction == "from") request.category else "") }
    val pad = rememberPad(if (start < 0) -start else maxOf(0, start))
    var keypad by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    retainEditorDraft(vm, "moveMoney", openedBudget, JSONObject().put("from", from).put("to", to).put("month", month).put("expression", pad.text))
    val title = when {
        request.category == Unassigned && start >= 0 -> stringResource(R.string.unassigned_assign)
        start < 0 && request.category != Unassigned -> stringResource(R.string.cover_overspending)
        else -> stringResource(R.string.move_money)
    }
    UrSheet(close) {
        SheetHeader(title, close, null, null, cancelFirst = true)
        Column(Modifier.padding(horizontal = Gutter).padding(bottom = if (keypad) 8.dp else 20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(if (keypad) ur.accentSoft else ur.surface2).clickable { keypad = true; pad.fresh = true }.padding(vertical = 18.dp),
                contentAlignment = Alignment.Center) {
                PadAmount(pad, currency, color = if (keypad) ur.accent else ur.text)
            }
            ListCard(Modifier.border(1.dp, ur.border, RoundedCornerShape(18.dp))) {
                MoveEnd(stringResource(R.string.date_from), from, ::name, ::available, currency) { picking = "from" }
                Box(Modifier.fillMaxWidth()) {
                    Hairline(Modifier.align(Alignment.Center))
                    Box(Modifier.align(Alignment.Center).size(34.dp).clip(CircleShape).background(ur.surface).border(1.dp, ur.border, CircleShape)
                        .clickable(onClickLabel = stringResource(R.string.move_swap)) { val f = from; from = to; to = f }, contentAlignment = Alignment.Center) {
                        Icon(Icons.Outlined.SwapVert, stringResource(R.string.move_swap), Modifier.size(18.dp), tint = ur.accent)
                    }
                }
                MoveEnd(stringResource(R.string.move_to_label), to, ::name, ::available, currency) { picking = "to" }
            }
            FormError(error)
            if (!keypad) PrimaryButton(if (from == Unassigned) stringResource(R.string.assign_action) else stringResource(R.string.move_action), {
                runCatching {
                    requireCurrentBudget(openedBudget, vm)
                    require(from.isNotBlank() && to.isNotBlank()) { Words[R.string.move_choose_both] }
                    require(from != to) { Words[R.string.move_choose_different] }
                    val amount = pad.cents ?: error(Words[R.string.transaction_finish_sum])
                    require(amount > 0) { Words[R.string.move_amount_required] }
                    vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "moveMoney").put("month", month).put("from", from).put("to", to).put("amount", amount)))
                    toast(Words[R.string.move_toast, money(amount, currency), name(from), name(to)], true)
                }.onSuccess { close() }.onFailure { error = it.message }
            }, Modifier.fillMaxWidth(), icon = Icons.Outlined.SwapHoriz)
        }
        if (keypad) AmountKeypad(pad, { keypad = false })
    }
    picking?.let { end ->
        CategoryPickerSheet(budget, summary, month, if (end == "from") from else to, if (end == "from") stringResource(R.string.move_from_title) else stringResource(R.string.move_to_title),
            includeUnassigned = true, includePayment = false, onPick = { if (end == "from") from = it else to = it; picking = null }) { picking = null }
    }
}

@Composable
private fun MoveEnd(label: String, id: String, name: (String) -> String, available: (String) -> Long, currency: String, pick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClickLabel = stringResource(R.string.choose_field, label), onClick = pick).heightIn(min = 64.dp).padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodySmall, color = ur.muted)
            Text(if (id.isBlank()) stringResource(R.string.category_choose_a) else name(id), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold,
                color = if (id.isBlank()) ur.accent else ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (id.isNotBlank()) {
            val amount = available(id)
            AvailablePill(amount, currency, if (amount < 0) "negative" else if (amount > 0) "positive" else "zero")
        }
        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.padding(start = 6.dp), tint = ur.muted)
    }
}

/** One entry of the category picker, prepared once per budget so typing a search does no JSON work. */
@Immutable
private data class PickerEntry(val id: String, val name: String, val group: String, val available: Long?, val status: String, val search: String)

/**
 * Choosing a category: search, then the groups with each category's available pill, as YNAB's
 * picker shows them. Unassigned and Uncategorized come first when offered. Hidden categories are
 * listed last, as the desktop's picker keeps them.
 */
@Composable
internal fun CategoryPickerSheet(budget: JSONObject, summary: JSONObject?, month: String, selected: String, title: String,
                                 includeUnassigned: Boolean = false, includeUncategorized: Boolean = false, includePayment: Boolean = true,
                                 top: (@Composable () -> Unit)? = null, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val currency = budget.text("currency")
    var search by remember { mutableStateOf("") }
    val entries by produceState(emptyList<Pair<String, List<PickerEntry>>>(), budget, summary, month, includePayment) {
        value = withContext(Dispatchers.Default) {
            val totals = summary?.rows("categories")?.associateBy { it.text("id") }.orEmpty()
            orderedGroups(budget) { includePayment || it.text("paymentAccountId").isBlank() }.map { (group, categories) ->
                group to categories.map { c ->
                    val id = c.text("id")
                    PickerEntry(id, c.text("name"), group, totals[id]?.optLong("available"), CategoryStatuses.of(c, totals[id], month).status, "${c.text("name")} $group".lowercase())
                }
            }
        }
    }
    val shown = remember(entries, search) {
        val q = search.trim().lowercase()
        if (q.isEmpty()) entries else entries.map { (g, list) -> g to list.filter { q in it.search } }.filter { it.second.isNotEmpty() }
    }
    UrSheet(onDismiss) {
        SheetHeader(title, onDismiss, null, null)
        SearchField(search, { search = it }, stringResource(R.string.category_search), Modifier.padding(horizontal = Gutter))
        LazyColumn(Modifier.fillMaxWidth().height(screenHeight() * .66f), contentPadding = PaddingValues(top = 8.dp, bottom = 24.dp)) {
            if (top != null && search.isBlank()) item(key = "top") { top() }
            if (includeUnassigned && search.isBlank()) item(key = "unassigned") {
                PickerRow(stringResource(R.string.category_unassigned), stringResource(R.string.unassigned_available_to_assign), summary?.optLong("readyToAssign"), currency, selected == Unassigned) { onPick(Unassigned) }
            }
            if (includeUncategorized && search.isBlank()) item(key = "uncategorized") { PickerRow(stringResource(R.string.category_uncategorized), null, null, currency, selected == "") { onPick("") } }
            shown.forEach { (group, categories) ->
                item(key = "g:$group", contentType = "group") {
                    Text(groupTitle(group).uppercase(), Modifier.fillMaxWidth().background(ur.surface2).padding(horizontal = 20.dp, vertical = 8.dp),
                        style = MaterialTheme.typography.labelSmall, color = ur.muted, letterSpacing = headerTracking)
                }
                items(categories, key = { "c:${it.id}" }, contentType = { "category" }) { entry ->
                    PickerRow(entry.name, null, entry.available, currency, selected == entry.id, status = entry.status) { onPick(entry.id) }
                }
            }
            if (shown.isEmpty() && search.isNotBlank()) item(key = "none") { Text(stringResource(R.string.category_search_none, search), Modifier.padding(24.dp), color = ur.muted) }
        }
    }
}

@Composable
private fun PickerRow(name: String, detail: String?, available: Long?, currency: String, chosen: Boolean, status: String? = null, pick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = pick).heightIn(min = 54.dp).padding(horizontal = 20.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(Money.own(name), style = MaterialTheme.typography.bodyLarge, fontWeight = if (chosen) FontWeight.Bold else FontWeight.Medium, color = ur.text)
            detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted) }
        }
        if (chosen) Icon(Icons.Outlined.Check, null, Modifier.padding(end = 8.dp), tint = ur.accent)
        if (available != null) AvailablePill(available, currency, status ?: if (available < 0) "negative" else if (available > 0) "positive" else "zero")
    }
}

/** Quick assign's options, grouped as the desktop groups them; the words are string resources. */
internal data class QuickMode(val id: String, val label: Int, val group: Int, val hint: Int)
internal val quickAssignModes = listOf(
    QuickMode("underfunded", R.string.quick_underfunded, R.string.quick_group_fund, R.string.quick_underfunded_hint),
    QuickMode("assignedLastMonth", R.string.quick_assigned_last_month, R.string.quick_group_repeat, R.string.quick_assigned_last_month_hint),
    QuickMode("spentLastMonth", R.string.quick_spent_last_month, R.string.quick_group_repeat, R.string.quick_spent_last_month_hint),
    QuickMode("averageAssigned", R.string.quick_average_assigned, R.string.quick_group_repeat, R.string.quick_average_assigned_hint),
    QuickMode("averageSpent", R.string.quick_average_spent, R.string.quick_group_repeat, R.string.quick_average_spent_hint),
    QuickMode("reduceOverfunding", R.string.quick_reduce_overfunding, R.string.quick_group_clean, R.string.quick_reduce_overfunding_hint),
    QuickMode("resetAvailable", R.string.quick_reset_available, R.string.quick_group_clean, R.string.quick_reset_available_hint),
    QuickMode("resetAssigned", R.string.quick_reset_assigned, R.string.quick_group_clean, R.string.quick_reset_assigned_hint),
)
private val resets = setOf("resetAvailable", "resetAssigned")

/** The engine request for a Quick assign option over the given (default: every visible) categories. */
internal fun quickAssignParams(budget: JSONObject, month: String, mode: String, categories: List<String>? = null, limit: Boolean = true): JSONObject {
    val reference = YearMonth.now().toString()
    val ids = categories ?: budget.rows("categories").filterNot { it.optBoolean("hidden") }.map { it.text("id") }
    return JSONObject().put("action", JSONObject().put("type", "autoAssign").put("month", month).put("mode", mode).put("categories", JSONArray(ids))
        .put("limitToAvailable", limit).put("expectedAsOfMonth", reference)).put("asOfMonth", reference)
}

/** What an option does, in words, for this month (desktop QuickAssign.tsx explain): this month's name, then last month's. */
private fun explain(mode: String, month: String): String {
    val now = runCatching { YearMonth.parse(month).format(Money.pattern("MMMM")) }.getOrDefault(month)
    val last = runCatching { YearMonth.parse(month).minusMonths(1).format(Money.pattern("MMMM")) }.getOrDefault(Words[R.string.last_month_lower])
    val words = when (mode) {
        "underfunded" -> R.string.quick_explain_underfunded
        "assignedLastMonth" -> R.string.quick_explain_assigned_last_month
        "spentLastMonth" -> R.string.quick_explain_spent_last_month
        "averageAssigned" -> R.string.quick_explain_average_assigned
        "averageSpent" -> R.string.quick_explain_average_spent
        "reduceOverfunding" -> R.string.quick_explain_reduce_overfunding
        "resetAvailable" -> R.string.quick_explain_reset_available
        "resetAssigned" -> R.string.quick_explain_reset_assigned
        else -> return ""
    }
    return Words[words, now, last]
}

/**
 * Unassigned's sheet: the month in figures, how far this month's targets are funded, targets against
 * expected income, ways to give the money jobs (Quick assign with what each option would do, or one
 * category), and the month's recent changes.
 */
@Composable
internal fun UnassignedSheet(budget: JSONObject, summary: JSONObject, month: String, vm: BudgetViewModel, close: () -> Unit, assignTo: () -> Unit, editIncome: () -> Unit) {
    val currency = budget.text("currency")
    var strategy by remember { mutableStateOf<String?>(null) }
    var allChanges by remember { mutableStateOf(false) }
    val rta = summary.optLong("readyToAssign")
    // What each option would change in total, worked out while the sheet is open.
    val outcomes = remember(budget, month) { mutableStateMapOf<String, Long>() }
    val jobs = rememberPreviewJobs(budget, month)
    LaunchedEffect(budget, summary, month) {
        val before = summary.optLong("assigned")
        quickAssignModes.forEach { mode ->
            jobs.track(vm.query("workflow", quickAssignParams(budget, month, mode.id).put("previewMonth", month), onError = {}) { result ->
                result.optJSONObject("summary")?.let { outcomes[mode.id] = it.optLong("assigned") - before }
            })
        }
    }
    val figures = remember(budget, summary, month) {
        val totals = summary.rows("categories").associateBy { it.text("id") }
        val visible = budget.rows("categories").filterNot { it.optBoolean("hidden") }
        val needed = visible.sumOf { c -> CategoryStatuses.of(c, totals[c.text("id")], month).needed }
        val targets = visible.sumOf { c -> totals[c.text("id")]?.optLong("monthTarget") ?: 0 }
        val carried = visible.sumOf { c -> totals[c.text("id")]?.optLong("carried") ?: 0 }
        Triple(needed, targets, carried)
    }
    val (needed, targets, carried) = figures
    val income = if (budget.isNull("expectedIncome") || !budget.has("expectedIncome")) null else budget.optLong("expectedIncome")
    val changes = remember(budget, month) { budget.rows("moneyMoves").filter { it.text("month") == month }.reversed() }
    strategy?.let { mode -> QuickAssignDialog(budget, summary, month, vm, mode) { strategy = null; close() }; return }
    UrSheet(close) {
        SheetHeader(monthLabel(month), close, null, null)
        Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = Gutter).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(money(rta, currency), fontSize = 36.sp, fontWeight = FontWeight.ExtraBold, style = Tabular,
                    color = when { rta > 0 -> ur.positive; rta < 0 -> ur.negative; else -> ur.text })
                Text(when { rta > 0 -> stringResource(R.string.unassigned_ready); rta < 0 -> stringResource(R.string.unassigned_over_assigned); else -> stringResource(R.string.unassigned_all_assigned) }, color = ur.muted)
            }
            if (needed > 0) Notice(stringResource(R.string.unassigned_still_needed, money(needed, currency)), tone = "attention", icon = Icons.Outlined.TrackChanges)
            else Notice(stringResource(R.string.unassigned_funding_complete), tone = "positive", icon = Icons.Outlined.CheckCircle)
            PrimaryButton(if (rta < 0) stringResource(R.string.unassigned_take_back) else stringResource(R.string.unassigned_assign_to_category), assignTo, Modifier.fillMaxWidth(), icon = Icons.Outlined.SwapHoriz)
            ListCard {
                ListRow(stringResource(R.string.unassigned_carried), value = money(carried, currency), divider = true)
                ListRow(stringResource(R.string.unassigned_income), value = money(summary.optLong("income"), currency), divider = true)
                ListRow(stringResource(R.string.budget_assigned), value = money(summary.optLong("assigned"), currency), divider = true)
                // The month's activity and available include spending that still needs a category.
                ListRow(stringResource(R.string.budget_activity), value = money(summary.optLong("activity") + summary.optLong("uncategorized"), currency), divider = true)
                val available = summary.optLong("available") + summary.optLong("uncategorized")
                ListRow(stringResource(R.string.budget_available), value = money(available, currency), valueColor = if (available < 0) ur.negative else ur.text)
            }
            SectionTitle(stringResource(R.string.unassigned_targets_income))
            ListCard {
                ListRow(stringResource(R.string.unassigned_month_targets, shortMonthLabel(month)), value = money(targets, currency), divider = true)
                ListRow(stringResource(R.string.expected_income), value = income?.let { money(it, currency) } ?: stringResource(R.string.common_add), valueColor = if (income == null) ur.accent else ur.text, onClick = editIncome,
                    subtitle = when {
                        income == null -> stringResource(R.string.expected_income_compare)
                        targets > income -> stringResource(R.string.expected_income_short, money(targets - income, currency))
                        else -> stringResource(R.string.expected_income_left, money(income - targets, currency))
                    })
            }
            SectionTitle(stringResource(R.string.quick_assign))
            quickAssignModes.groupBy { it.group }.forEach { (group, modes) ->
                Text(stringResource(group).uppercase(), Modifier.padding(start = 4.dp), style = MaterialTheme.typography.labelSmall, color = ur.muted, letterSpacing = headerTracking)
                ListCard {
                    modes.forEachIndexed { index, mode ->
                        val outcome = outcomes[mode.id]
                        val changes0 = outcome != null && outcome != 0L
                        ListRow(stringResource(mode.label), subtitle = stringResource(mode.hint), onClick = if (outcome == null || changes0) ({ strategy = mode.id }) else null, chevron = false,
                            titleColor = if (outcome == 0L) ur.muted else ur.text,
                            value = when { outcome == null -> "…"; outcome == 0L -> stringResource(R.string.quick_no_change); else -> Money.signed(outcome, currency) },
                            valueColor = when { outcome == null || outcome == 0L -> ur.muted; outcome > 0 -> ur.positive; else -> ur.negative },
                            divider = index < modes.lastIndex)
                    }
                }
            }
            if (changes.isNotEmpty()) {
                SectionTitle(stringResource(R.string.unassigned_recent_changes))
                ListCard {
                    val shownChanges = if (allChanges) changes else changes.take(4)
                    shownChanges.forEachIndexed { index, m ->
                        ListRow(m.text("description").ifBlank { stringResource(R.string.unassigned_change) }, subtitle = shortDay(m.text("at")), divider = index < shownChanges.lastIndex || changes.size > 4 && !allChanges)
                    }
                    if (changes.size > 4 && !allChanges) ListRow(stringResource(R.string.show_all_count, "${changes.size}"), titleColor = ur.accent, onClick = { allChanges = true })
                }
            }
        }
    }
}

/** One category's change in a Quick assign preview. */
private data class QuickRow(val id: String, val name: String, val before: Long, val after: Long, val availableBefore: Long, val availableAfter: Long, val card: Boolean)

/**
 * A Quick assign option explained with the budget's own numbers before applying it: what it assigns
 * and returns, Unassigned before and after, each category's change, and warnings when it would
 * over-assign or leave categories overspent.
 */
@Composable
internal fun QuickAssignDialog(budget: JSONObject, summary: JSONObject, month: String, vm: BudgetViewModel, mode: String, close: () -> Unit) {
    val currency = budget.text("currency")
    val toast = LocalToast.current
    val option = quickAssignModes.first { it.id == mode }
    val params = remember(budget, month, mode) { quickAssignParams(budget, month, mode) }
    var preview by remember(budget, month, mode) { mutableStateOf<JSONObject?>(null) }
    var failed by remember(budget, month, mode) { mutableStateOf<String?>(null) }
    val jobs = rememberPreviewJobs(budget, month, mode)
    LaunchedEffect(params) { jobs.track(vm.query("workflow", params.copyJson().put("previewMonth", month), onError = { failed = it }) { preview = it }) }
    val rows by produceState<List<QuickRow>?>(null, preview) {
        val result = preview ?: return@produceState
        value = withContext(Dispatchers.Default) {
            val before = summary.rows("categories").associateBy { it.text("id") }
            val after = result.optJSONObject("summary")?.rows("categories")?.associateBy { it.text("id") }.orEmpty()
            val ranks = orderedGroups(budget).flatMap { it.second }.mapIndexed { i, c -> c.text("id") to i }.toMap()
            budget.rows("categories").mapNotNull { c ->
                val id = c.text("id")
                val b = before[id]?.optLong("assigned") ?: 0
                val a = after[id]?.optLong("assigned") ?: b
                if (a == b) null else QuickRow(id, c.text("name"), b, a, before[id]?.optLong("available") ?: 0, after[id]?.optLong("available") ?: 0, c.text("paymentAccountId").isNotBlank())
            }.sortedBy { ranks[it.id] ?: Int.MAX_VALUE }
        }
    }
    val after = preview?.optJSONObject("summary")
    val visible = remember(budget) { budget.rows("categories").count { !it.optBoolean("hidden") } }
    val list = rows
    val assigns = list?.filter { it.after > it.before }?.sumOf { it.after - it.before } ?: 0
    val returns = list?.filter { it.after < it.before }?.sumOf { it.before - it.after } ?: 0
    val apply = {
        withCurrentBudget(budget, vm) {
            vm.runCommand("workflow", params)
            val change = assigns - returns
            toast(Words[if (change >= 0) R.string.quick_toast_assigned else R.string.quick_toast_removed, Words[option.label], money(kotlin.math.abs(change), currency)], true)
            close()
        }
    }
    UrSheet(close) {
        SheetHeader(stringResource(option.label), close, null, null, cancelFirst = true)
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = screenHeight() * .78f), contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item(key = "explain") {
                Text(remember(mode, month, languageKey()) { explain(mode, month) }, color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                Text(countText(R.plurals.quick_visible_categories, visible, monthLabel(month)), Modifier.padding(top = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall)
            }
            failed?.let { item(key = "error") { FormError(it) } }
            if (list == null && failed == null) item(key = "loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
            if (list != null && after != null) {
                item(key = "figures") {
                    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(ur.surface2).padding(14.dp)) {
                        Figure(if (returns > assigns) stringResource(R.string.quick_returns) else stringResource(R.string.quick_assigns), money(if (returns > assigns) returns - assigns else assigns - returns, currency), Modifier.weight(1f),
                            color = if (returns > assigns) ur.negative else ur.positive)
                        Figure(stringResource(R.string.category_unassigned), stringResource(R.string.change_from_to, money(summary.optLong("readyToAssign"), currency), money(after.optLong("readyToAssign"), currency)), Modifier.weight(1.6f),
                            color = if (after.optLong("readyToAssign") < 0) ur.negative else ur.text)
                    }
                    Text(stringResource(R.string.quick_categories_change, "${list.size}", "$visible"), Modifier.padding(top = 6.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall)
                }
                if (after.optLong("readyToAssign") < 0) item(key = "over") {
                    Notice(stringResource(R.string.quick_over_assigns, money(after.optLong("readyToAssign"), currency)), tone = "negative", icon = Icons.Outlined.WarningAmber)
                }
                val overspentAfter = after.rows("categories").count { it.optLong("available") < 0 }
                if (overspentAfter > 0) item(key = "overspent") {
                    Notice(countText(R.plurals.quick_categories_stay_overspent, overspentAfter), tone = "attention", icon = Icons.Outlined.Info)
                }
                if (list.isEmpty()) item(key = "nothing") { Notice(stringResource(R.string.quick_nothing_to_change, monthLabel(month)), tone = "info") }
                else item(key = "rows") {
                    ListCard {
                        list.forEachIndexed { index, row ->
                            val change = row.after - row.before
                            ListRow(row.name, subtitle = stringResource(R.string.quick_row_detail, money(row.before, currency), money(row.after, currency), money(row.availableAfter, currency)),
                                value = Money.signed(change, currency), valueColor = if (change > 0) ur.positive else ur.negative,
                                divider = index < list.lastIndex)
                        }
                    }
                }
            }
        }
        val count = list?.size ?: 0
        PrimaryButton(if (count == 0) stringResource(R.string.quick_nothing_to_apply) else countText(R.plurals.quick_apply_to, count), apply,
            Modifier.fillMaxWidth().padding(horizontal = Gutter).padding(bottom = 16.dp), enabled = count > 0, color = if (mode in resets) ur.negative else ur.accent)
    }
}

@Composable
internal fun NewCategoryDialog(budget: JSONObject, vm: BudgetViewModel, presetGroup: String? = null, close: () -> Unit) {
    val existing = remember(budget) { groupNames(budget) }
    var name by remember { mutableStateOf("") }
    // A budget's first group is named in the language in use; after that it's the person's own name.
    val starterGroup = stringResource(R.string.starter_group_everyday)
    var group by remember { mutableStateOf(presetGroup ?: existing.firstOrNull() ?: starterGroup) }
    var newGroup by remember { mutableStateOf(existing.isEmpty()) }
    retainEditorDraft(vm, "newCategory", budget, JSONObject().put("name", name).put("group", group))
    FormDialog(stringResource(R.string.category_new), close, confirm = stringResource(R.string.common_add), onConfirm = { vm.saveCategory(name = name.trim(), group = group.trim()); close() }, enabled = name.isNotBlank() && group.isNotBlank()) {
        Field(name, { name = it }, stringResource(R.string.category_name))
        if (!newGroup) Choice(stringResource(R.string.group_label), group, existing.map { it to it } + ("__new" to stringResource(R.string.group_new_ellipsis)), { if (it == "__new") { newGroup = true; group = "" } else group = it })
        else Field(group, { group = it }, stringResource(R.string.group_new_name))
    }
}

/** A new, empty group: it waits in the group order with an "Add a category" row. */
@Composable
internal fun NewGroupDialog(budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val existing = remember(budget) { groupNames(budget) }
    var name by remember { mutableStateOf("") }
    val taken = name.trim() in existing
    retainEditorDraft(vm, "newGroup", budget, JSONObject().put("name", name))
    FormDialog(stringResource(R.string.group_new), close, confirm = stringResource(R.string.common_add), onConfirm = {
        withCurrentBudget(budget, vm) {
            vm.editBudget { b -> b.put("groupOrder", (b.optJSONArray("groupOrder") ?: JSONArray(existing)).put(name.trim())) }
        }
        close()
    }, enabled = name.isNotBlank() && !taken) {
        Field(name, { name = it }, stringResource(R.string.group_name))
        if (taken) FormError(stringResource(R.string.group_name_taken, name.trim()))
    }
}

@Composable
internal fun RenameGroupDialog(group: String, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val toast = LocalToast.current
    val existing = remember(budget) { groupNames(budget) }
    var name by remember { mutableStateOf(group) }
    retainEditorDraft(vm, "renameGroup", budget, JSONObject().put("group", group).put("name", name))
    FormDialog(stringResource(R.string.group_rename), close, onConfirm = {
        withCurrentBudget(budget, vm) {
            vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "renameGroup").put("from", group).put("to", name.trim())))
            if (name.trim() in existing) toast(Words[R.string.group_merged, group, name.trim()], true)
        }
        close()
    }, enabled = name.isNotBlank() && name != group) {
        Field(name, { name = it }, stringResource(R.string.group_name))
        Text(stringResource(R.string.group_rename_merges), color = ur.muted, style = MaterialTheme.typography.bodySmall)
    }
}

/** Deleting a group moves its categories to another group (a merge), or drops an empty group from the order. */
@Composable
internal fun DeleteGroupDialog(group: String, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val toast = LocalToast.current
    val others = remember(budget) { groupNames(budget).filter { it != group } }
    val count = remember(budget) { budget.rows("categories").count { !it.optBoolean("hidden") && it.text("group") == group } }
    var target by remember { mutableStateOf(others.firstOrNull().orEmpty()) }
    retainEditorDraft(vm, "deleteGroup", budget, JSONObject().put("group", group).put("target", target))
    FormDialog(stringResource(R.string.delete_named, group), close, confirm = stringResource(R.string.common_delete), onConfirm = {
        withCurrentBudget(budget, vm) {
            if (count == 0) vm.editBudget { b ->
                val order = b.optJSONArray("groupOrder") ?: JSONArray()
                b.put("groupOrder", JSONArray((0 until order.length()).map { order.optString(it) }.filter { it != group }))
            } else vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "renameGroup").put("from", group).put("to", target)))
            toast(if (count == 0) Words[R.string.deleted_named, group] else Words[R.string.group_deleted_moved, group, target], true)
        }
        close()
    }, enabled = count == 0 || target.isNotBlank()) {
        if (count == 0) Text(stringResource(R.string.group_empty), color = ur.text2)
        else {
            Text(countText(R.plurals.group_move_categories_to, count), color = ur.text2)
            Choice(stringResource(R.string.group_label), target, others.map { it to it }, { target = it })
        }
    }
}

@Composable
internal fun BudgetPlanningDialog(budget: JSONObject, month: String, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    var note by remember { mutableStateOf(budget.rows("ynabMonths").firstOrNull { it.text("month").startsWith(month) }?.text("note").orEmpty()) }
    var income by remember { mutableStateOf(if (budget.isNull("expectedIncome") || !budget.has("expectedIncome")) "" else amountText(budget.optLong("expectedIncome"))) }
    var error by remember { mutableStateOf<String?>(null) }
    retainEditorDraft(vm, "budgetPlanning", openedBudget, JSONObject().put("month", month).put("note", note).put("income", income))
    FormDialog(monthLabel(month), close, onConfirm = {
        runCatching { requireCurrentBudget(openedBudget, vm); val expected = if (income.isBlank()) null else amountValue(income).also { require(it >= 0) { Words[R.string.expected_income_not_negative] } }; vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "setBudgetPlanning").put("month", month).put("note", note).put("expectedIncome", expected ?: JSONObject.NULL))); close() }.onFailure { error = it.message }
    }) {
        Field(note, { note = it }, stringResource(R.string.month_note), lines = 5)
        Field(income, { income = it }, stringResource(R.string.expected_income_field, budget.text("currency")), true)
        Text(stringResource(R.string.expected_income_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        FormError(error)
    }
}
