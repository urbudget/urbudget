package app.urbudget.nativeapp.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.time.YearMonth

/** The hidden categories' place in the list, kept as a group named in English; [groupTitle] says it in the interface's language. */
internal const val HiddenGroup = "Hidden categories"
internal const val Unassigned = "__rta"

/** A group's name as the list shows it: the person's own, or the hidden categories' heading. */
@Composable
internal fun groupTitle(name: String): String = if (name == HiddenGroup) stringResource(R.string.group_hidden) else name

/** The budget's groups in their saved order, each with its categories in order. */
internal fun orderedGroups(budget: JSONObject, include: (JSONObject) -> Boolean = { true }): List<Pair<String, List<JSONObject>>> {
    val order = budget.optJSONArray("categoryOrder") ?: JSONArray()
    val ranks = (0 until order.length()).associate { order.optString(it) to it }
    val groupOrder = budget.optJSONArray("groupOrder") ?: JSONArray()
    val groupRanks = (0 until groupOrder.length()).associate { groupOrder.optString(it) to it }
    return budget.rows("categories").filter(include).sortedBy { ranks[it.text("id")] ?: Int.MAX_VALUE }
        .groupBy { if (it.optBoolean("hidden")) HiddenGroup else it.text("group", "Everyday") }.toList()
        .sortedBy { if (it.first == HiddenGroup) Int.MAX_VALUE else groupRanks[it.first] ?: (Int.MAX_VALUE - 1) }
}

/** Where a budget figure leads in the register: a category or an account, optionally for a month. */
internal data class RegisterTarget(val category: String = "", val account: String = "", val from: String = "", val through: String = "", val search: String = "")

/**
 * The budget on a phone, as YNAB's app lays it out: Unassigned as a solid banner, then each group a
 * band across the screen with its Assigned and Available totals, and each category a row with both,
 * Available as a solid pill. A tap on a category opens its details as a sheet; a tap on Assigned
 * opens them on the keypad; a tap on the pill moves money. The list draws a model built off the
 * main thread once per snapshot, so an edit recomposes only the rows that changed.
 */
@Composable
internal fun BudgetScreen(budget: JSONObject, summary: JSONObject?, month: String, onMonth: (String) -> Unit, vm: BudgetViewModel,
                          bar: @Composable RowScope.() -> Unit, onTransactions: (RegisterTarget) -> Unit, onPayCard: (String) -> Unit) {
    val currency = budget.text("currency")
    var filter by rememberSaveable { mutableStateOf("all") }
    var showHidden by rememberSaveable { mutableStateOf(false) }
    // Hidden categories start folded, as on the desktop.
    var collapsed by rememberSaveable { mutableStateOf(listOf(HiddenGroup)) }
    var viewsOpen by remember { mutableStateOf(false) }
    var openCategory by remember { mutableStateOf<String?>(null) }
    var assignOnOpen by remember { mutableStateOf(false) }
    var moving by remember { mutableStateOf<MoveRequest?>(null) }
    var assigning by remember { mutableStateOf(false) }
    var planning by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf<String?>(null) }
    var addingGroup by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<String?>(null) }
    var deletingGroup by remember { mutableStateOf<String?>(null) }
    val toast = LocalToast.current
    BackHandler(filter != "all") { filter = "all" }
    // The model for this snapshot: from the cache when the tab is revisited, else built on a worker.
    // While a newer snapshot builds, the last model stays on screen.
    // The model has words in it (what a target needs), so a new language builds a new one.
    val language = languageKey()
    val cached = remember { (vm.screenCache["budget"] as? Pair<*, *>)?.takeIf { it.first == listOf(budget, summary, month, language) }?.second as? BudgetModel }
    val model by produceState(cached, budget, summary, month, language) {
        if (summary == null) return@produceState
        val key = listOf(budget, summary, month, language)
        val hit = (vm.screenCache["budget"] as? Pair<*, *>)?.takeIf { it.first == key }?.second as? BudgetModel
        value = hit ?: withContext(Dispatchers.Default) { buildBudgetModel(budget, summary, month) }.also { vm.screenCache["budget"] = key to it }
    }
    val visibleGroups = remember(model, filter, showHidden) {
        model?.groups.orEmpty().filter { showHidden || it.name != HiddenGroup }.map { g -> g to g.rows.filter { it.inView(filter) } }
            .filter { (g, rows) -> rows.isNotEmpty() || filter == "all" && g.name != HiddenGroup }
    }
    val counts = remember(model, showHidden) {
        val rows = model?.groups.orEmpty().filter { showHidden || it.name != HiddenGroup }.flatMap { it.rows }
        BudgetViews.associate { (id, _) -> id to rows.count { it.inView(id) } }
    }
    // Columns as wide as the widest figure: with tabular digits only the longest strings matter.
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val widths = remember(model?.widestAssigned, model?.widestAvailable, density) {
        val style = TextStyle(fontSize = 15.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")
        fun widest(values: List<String>?) = values.orEmpty().maxOfOrNull { measurer.measure(it, style).size.width } ?: 0
        with(density) { maxOf(84.dp, widest(model?.widestAssigned).toDp() + 4.dp) to maxOf(96.dp, widest(model?.widestAvailable).toDp() + 28.dp) }
    }
    val (assignedWidth, availableWidth) = widths
    val current = YearMonth.now().toString()
    Column(Modifier.fillMaxSize()) {
        PhoneTopBar(leading = {
            Box {
                RoundIconButton(Icons.Outlined.FilterList, stringResource(R.string.budget_views), { viewsOpen = true }, tint = if (filter != "all") ur.accent else ur.text, dot = filter != "all")
                ViewsMenu(viewsOpen, { viewsOpen = false }, filter, { filter = it }, counts, showHidden, { showHidden = it },
                    allCollapsed = visibleGroups.isNotEmpty() && visibleGroups.all { it.first.name in collapsed },
                    toggleAll = {
                        val names = visibleGroups.map { it.first.name }
                        // Expanding keeps Hidden folded; folding folds it too.
                        collapsed = if (names.all { it in collapsed }) listOf(HiddenGroup) else (names + HiddenGroup).distinct()
                    },
                    addCategory = { adding = "" }, addGroup = { addingGroup = true }, planMonth = { planning = true })
            }
        }, center = { MonthSwitcher(month, onMonth) }, actions = bar)
        val shown = model
        if (summary == null) {
            EmptyState(stringResource(R.string.budget_unavailable_title), stringResource(R.string.budget_unavailable_detail))
            return@Column
        }
        if (shown == null) {
            LinearProgressIndicator(Modifier.fillMaxWidth().padding(Gutter), color = ur.accent)
            return@Column
        }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = DockRoom)) {
            if (month != current) item(key = "today", contentType = "today") {
                Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), horizontalArrangement = Arrangement.Center) {
                    FilterPill(stringResource(R.string.month_back_to, shortMonthLabel(current)), false, { onMonth(current) }, Icons.Outlined.Today)
                }
            }
            item(key = "unassigned", contentType = "banner") {
                UnassignedBanner(shown.rta, currency, Modifier.padding(horizontal = Gutter).padding(top = 4.dp, bottom = 12.dp)) { assigning = true }
            }
            if (shown.futureAssigned != 0L || shown.shortfalls.isNotEmpty()) item(key = "future", contentType = "notice") {
                val text = buildList {
                    if (shown.futureAssigned != 0L) add(stringResource(R.string.future_assigned, money(shown.futureAssigned, currency)))
                    if (shown.shortfalls.isNotEmpty()) {
                        shown.shortfalls.forEach { add(stringResource(R.string.future_short_by, monthLabel(it.first), money(it.second, currency))) }
                        add(stringResource(R.string.future_move_back))
                    } else if (shown.futureAssigned > 0 && shown.futureReadyToAssign + shown.futureAssigned >= 0) add(stringResource(R.string.future_held_back))
                }.joinToString(" ")
                Notice(text, Modifier.padding(horizontal = Gutter).padding(bottom = 12.dp), tone = if (shown.shortfalls.isNotEmpty()) "negative" else "info",
                    icon = if (shown.shortfalls.isNotEmpty()) Icons.Outlined.WarningAmber else Icons.Outlined.Info)
            }
            if (shown.needsCategory.total > 0) item(key = "needs", contentType = "notice") {
                val n = shown.needsCategory.total
                Notice(countText(R.plurals.needs_category_notice, n, money(shown.needsCategory.amount, currency)),
                    Modifier.padding(horizontal = Gutter).padding(bottom = 12.dp), tone = "attention", action = stringResource(R.string.common_review),
                    onAction = { onTransactions(RegisterTarget(category = "__none")) })
            }
            val overspent = counts["overspent"] ?: 0
            if (overspent > 0 && filter == "all") item(key = "overspent", contentType = "notice") {
                OverspentCard(overspent, Modifier.padding(horizontal = Gutter).padding(bottom = 12.dp)) { filter = "overspent" }
            }
            if (filter != "all") item(key = "view", contentType = "view") {
                Row(Modifier.padding(horizontal = Gutter).padding(bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.budget_view_showing) + " ", color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                    Text(stringResource(BudgetViews.first { it.first == filter }.second), Modifier.weight(1f), color = ur.text, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
                    FilterPill(stringResource(R.string.view_show_all), false, { filter = "all" }, Icons.Outlined.Close)
                }
            }
            if (shown.monthNote != null && filter == "all") item(key = "note", contentType = "note") {
                Row(Modifier.padding(horizontal = Gutter).padding(bottom = 12.dp).fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(ur.surface).clickable { planning = true }.padding(14.dp)) {
                    Icon(Icons.AutoMirrored.Outlined.Notes, null, Modifier.size(20.dp), tint = ur.muted)
                    Text(shown.monthNote, Modifier.padding(start = 10.dp), color = ur.text2, style = MaterialTheme.typography.bodyMedium, maxLines = 4, overflow = TextOverflow.Ellipsis)
                }
            }
            if (shown.uncategorized < 0 && (filter == "all" || filter == "overspent")) item(key = "uncategorized", contentType = "category") {
                UncategorizedLine(shown.uncategorized, shown.uncategorizedCredit > 0, currency, assignedWidth, availableWidth) { onTransactions(RegisterTarget(category = "__none")) }
            }
            if (visibleGroups.isEmpty()) item(key = "empty", contentType = "empty") {
                if (filter == "all") EmptyState(stringResource(R.string.budget_empty_title), stringResource(R.string.budget_empty_detail), stringResource(R.string.category_add)) { adding = "" }
                else EmptyState(stringResource(R.string.view_empty_title), stringResource(R.string.view_empty_detail), stringResource(R.string.view_show_all)) { filter = "all" }
            }
            visibleGroups.forEach { (group, rows) ->
                val open = group.name !in collapsed
                item(key = "group:${group.name}", contentType = "group") {
                    val hiddenToast = stringResource(R.string.group_hid_categories, group.name)
                    GroupBand(groupTitle(group.name), open, rows, currency, assignedWidth, availableWidth,
                        toggle = { collapsed = if (open) collapsed + group.name else collapsed - group.name },
                        menu = if (group.name == HiddenGroup) null else ({ action ->
                            when (action) {
                                "add" -> adding = group.name
                                "rename" -> renaming = group.name
                                "delete" -> deletingGroup = group.name
                                "hide" -> {
                                    val ids = group.rows.map { it.id }.toSet()
                                    withCurrentBudget(budget, vm) {
                                        vm.upsertMany("categories", budget.rows("categories").filter { it.text("id") in ids }.map { it.copyJson().put("hidden", true) })
                                        toast(hiddenToast, true)
                                    }
                                }
                                "up", "down" -> moveGroup(budget, vm, group.name, action == "up")
                            }
                        }))
                }
                if (open) {
                    items(rows, key = { it.id }, contentType = { "category" }) { row ->
                        CategoryLine(row, assignedWidth, availableWidth,
                            open = { assignOnOpen = false; openCategory = row.id },
                            assign = { assignOnOpen = true; openCategory = row.id },
                            move = { moving = MoveRequest(row.id, if (row.available < 0) "from" else "to") },
                            last = row === rows.last())
                    }
                    if (rows.isEmpty() && group.name != HiddenGroup) item(key = "add:${group.name}", contentType = "add") {
                        Column(Modifier.background(ur.surface)) {
                            ListRow(stringResource(R.string.category_add_a), icon = Icons.Outlined.Add, titleColor = ur.accent, onClick = { adding = group.name }, chevron = false)
                        }
                    }
                }
            }
        }
    }
    openCategory?.let { id ->
        val category = remember(budget, id) { budget.row("categories", id) }
        val total = remember(summary, id) { summary?.rows("categories")?.firstOrNull { it.text("id") == id } }
        if (category != null) CategorySheet(category, budget, total, summary, month, vm, assignOnOpen, close = { openCategory = null },
            move = { direction -> moving = MoveRequest(id, direction) },
            transactions = { target -> openCategory = null; onTransactions(target) },
            payCard = { account -> openCategory = null; onPayCard(account) })
    }
    moving?.let { request -> MoveMoneySheet(request, budget, summary, month, vm) { moving = null } }
    if (assigning && summary != null) UnassignedSheet(budget, summary, month, vm, close = { assigning = false },
        assignTo = { assigning = false; moving = MoveRequest(Unassigned, if (summary.optLong("readyToAssign") < 0) "from" else "to") },
        editIncome = { assigning = false; planning = true })
    if (planning) BudgetPlanningDialog(budget, month, vm) { planning = false }
    adding?.let { group -> NewCategoryDialog(budget, vm, group.ifBlank { null }) { adding = null } }
    if (addingGroup) NewGroupDialog(budget, vm) { addingGroup = false }
    renaming?.let { group -> RenameGroupDialog(group, budget, vm) { renaming = null } }
    deletingGroup?.let { group -> DeleteGroupDialog(group, budget, vm) { deletingGroup = null } }
}

/** A request to move money: the category it starts from, and whether money goes to or comes from another. */
internal data class MoveRequest(val category: String, val direction: String)

private fun moveGroup(budget: JSONObject, vm: BudgetViewModel, group: String, up: Boolean) {
    val groups = orderedGroups(budget) { !it.optBoolean("hidden") }
    val index = groups.indexOfFirst { it.first == group }
    if (index < 0) return
    val target = if (up) index - 1 else index + 1
    if (target !in groups.indices) return
    // groupBefore places the group before another; after the last group is the end ("").
    val before = if (up) groups[target].first else groups.getOrNull(target + 1)?.first
    val categories = groups[index].second
    val first = categories.first()
    withCurrentBudget(budget, vm) {
        vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "organizeCategory").put("id", first.text("id")).put("group", group)
            .put("beforeId", categories.getOrNull(1)?.text("id") ?: JSONObject.NULL).put("groupBefore", before ?: JSONObject.NULL)))
    }
}

/** The month in the middle of the top bar, with the months either side a tap away. */
@Composable
private fun MonthSwitcher(month: String, onMonth: (String) -> Unit) {
    val ym = remember(month) { runCatching { YearMonth.parse(month) }.getOrDefault(YearMonth.now()) }
    Row(verticalAlignment = Alignment.CenterVertically) {
        IconButton({ onMonth(ym.minusMonths(1).toString()) }, Modifier.size(36.dp)) { Icon(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, stringResource(R.string.month_previous), tint = ur.muted) }
        MonthSelector(month, onMonth)
        IconButton({ onMonth(ym.plusMonths(1).toString()) }, Modifier.size(36.dp)) { Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, stringResource(R.string.month_next), tint = ur.muted) }
    }
}

/** Unassigned as a solid banner: green with money to give jobs to, red when more is assigned than there is. */
@Composable
private fun UnassignedBanner(amount: Long, currency: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val fill = when { amount > 0 -> ur.positive; amount < 0 -> ur.negative; else -> ur.zeroSoft }
    val ink = if (amount == 0L) ur.text else if (ur.dark) ur.bg else Color.White
    Row(modifier.fillMaxWidth().heightIn(min = 68.dp).clip(RoundedCornerShape(18.dp)).background(fill)
        .clickable(role = Role.Button, onClickLabel = stringResource(R.string.unassigned_assign), onClick = onClick).padding(horizontal = 20.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(money(amount, currency), color = ink, fontSize = 28.sp, fontWeight = FontWeight.ExtraBold, style = Tabular, maxLines = 1)
            Text(when { amount > 0 -> stringResource(R.string.unassigned_tap_to_assign); amount < 0 -> stringResource(R.string.unassigned_over_assigned); else -> stringResource(R.string.unassigned_all_assigned) },
                color = ink.copy(alpha = .85f), style = MaterialTheme.typography.bodySmall)
        }
        Text(stringResource(R.string.category_unassigned), color = ink, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
        Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, tint = ink)
    }
}

@Composable
private fun OverspentCard(count: Int, modifier: Modifier = Modifier, show: () -> Unit) {
    Row(modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(ur.surface).clickable(onClick = show).padding(start = 14.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(28.dp).clip(CircleShape).background(ur.negative), contentAlignment = Alignment.Center) {
            Text("$count", color = if (ur.dark) ur.bg else Color.White, fontWeight = FontWeight.Bold, fontSize = 14.sp)
        }
        Text(pluralStringResource(R.plurals.overspent_categories, count), Modifier.weight(1f).padding(start = 12.dp), color = ur.text,
            style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium)
        TextButton(show) { Text(stringResource(R.string.common_show), color = ur.accent, fontWeight = FontWeight.Bold) }
    }
}

@Composable
private fun ViewsMenu(open: Boolean, close: () -> Unit, filter: String, setFilter: (String) -> Unit, counts: Map<String, Int>, showHidden: Boolean,
                      setHidden: (Boolean) -> Unit, allCollapsed: Boolean, toggleAll: () -> Unit, addCategory: () -> Unit, addGroup: () -> Unit, planMonth: () -> Unit) {
    DropdownMenu(open, close, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
        BudgetViews.forEach { (id, label) ->
            val count = counts[id] ?: 0
            DropdownMenuItem(text = { Text(stringResource(label), fontWeight = if (id == filter) FontWeight.SemiBold else FontWeight.Normal) },
                enabled = id == "all" || count > 0,
                trailingIcon = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (id != "all") Text("$count", color = ur.muted, style = MaterialTheme.typography.bodySmall.merge(Tabular))
                        if (id == filter) Icon(Icons.Outlined.Check, null, Modifier.padding(start = 6.dp), tint = ur.accent)
                    }
                },
                onClick = { setFilter(id); close() })
        }
        HorizontalDivider(color = ur.border)
        DropdownMenuItem(text = { Text(stringResource(R.string.budget_show_hidden)) }, leadingIcon = { Icon(if (showHidden) Icons.Outlined.Visibility else Icons.Outlined.VisibilityOff, null) },
            trailingIcon = if (showHidden) ({ Icon(Icons.Outlined.Check, null, tint = ur.accent) }) else null, onClick = { setHidden(!showHidden); close() })
        DropdownMenuItem(text = { Text(if (allCollapsed) stringResource(R.string.common_expand_all) else stringResource(R.string.common_collapse_all)) }, leadingIcon = { Icon(if (allCollapsed) Icons.Outlined.UnfoldMore else Icons.Outlined.UnfoldLess, null) },
            onClick = { toggleAll(); close() })
        DropdownMenuItem(text = { Text(stringResource(R.string.category_new)) }, leadingIcon = { Icon(Icons.Outlined.Add, null) }, onClick = { addCategory(); close() })
        DropdownMenuItem(text = { Text(stringResource(R.string.group_new)) }, leadingIcon = { Icon(Icons.Outlined.CreateNewFolder, null) }, onClick = { addGroup(); close() })
        DropdownMenuItem(text = { Text(stringResource(R.string.month_note_income)) }, leadingIcon = { Icon(Icons.AutoMirrored.Outlined.Notes, null) }, onClick = { planMonth(); close() })
    }
}

/** A group as a band across the screen, with its Assigned and Available totals for the rows shown. */
@Composable
private fun GroupBand(name: String, open: Boolean, rows: List<CategoryRowModel>, currency: String, assignedWidth: Dp, availableWidth: Dp,
                      toggle: () -> Unit, menu: ((String) -> Unit)?) {
    var menuOpen by remember { mutableStateOf(false) }
    val totals = remember(rows) { rows.sumOf { it.assigned } to rows.sumOf { it.available } }
    Column(Modifier.background(ur.surface2)) {
        Hairline()
        Row(Modifier.fillMaxWidth().clickable(onClickLabel = if (open) stringResource(R.string.fold_collapse, name) else stringResource(R.string.fold_expand, name), onClick = toggle)
            .heightIn(min = 56.dp).padding(start = 8.dp, end = Gutter), verticalAlignment = Alignment.CenterVertically) {
            Icon(if (open) Icons.Outlined.KeyboardArrowDown else Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(22.dp), tint = ur.text2)
            Row(Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                Text(Money.own(name), Modifier.weight(1f, fill = false).padding(start = 4.dp), fontSize = 17.sp, fontWeight = FontWeight.Bold, color = ur.text,
                    maxLines = 2, overflow = TextOverflow.Ellipsis, lineHeight = 20.sp)
                if (menu != null) Box {
                    IconButton({ menuOpen = true }, Modifier.size(36.dp)) { Icon(Icons.Outlined.MoreHoriz, stringResource(R.string.group_actions_for, name), Modifier.size(18.dp), tint = ur.muted) }
                    DropdownMenu(menuOpen, { menuOpen = false }, shape = RoundedCornerShape(16.dp), containerColor = ur.surface) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.category_add)) }, leadingIcon = { Icon(Icons.Outlined.Add, null) }, onClick = { menu("add"); menuOpen = false })
                        DropdownMenuItem(text = { Text(stringResource(R.string.group_rename)) }, leadingIcon = { Icon(Icons.Outlined.Edit, null) }, onClick = { menu("rename"); menuOpen = false })
                        if (rows.isNotEmpty()) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.common_move_up)) }, leadingIcon = { Icon(Icons.Outlined.ArrowUpward, null) }, onClick = { menu("up"); menuOpen = false })
                            DropdownMenuItem(text = { Text(stringResource(R.string.common_move_down)) }, leadingIcon = { Icon(Icons.Outlined.ArrowDownward, null) }, onClick = { menu("down"); menuOpen = false })
                            DropdownMenuItem(text = { Text(stringResource(R.string.group_hide_all)) }, leadingIcon = { Icon(Icons.Outlined.VisibilityOff, null) }, onClick = { menu("hide"); menuOpen = false })
                        }
                        HorizontalDivider(color = ur.border)
                        DropdownMenuItem(text = { Text(stringResource(R.string.group_delete_menu), color = ur.negative) }, leadingIcon = { Icon(Icons.Outlined.DeleteOutline, null, tint = ur.negative) },
                            onClick = { menu("delete"); menuOpen = false })
                    }
                }
            }
            Column(Modifier.width(assignedWidth), horizontalAlignment = Alignment.End) {
                Text(stringResource(R.string.budget_assigned), fontSize = 12.sp, color = ur.muted, fontWeight = FontWeight.Medium)
                Text(money(totals.first, currency), fontSize = 15.sp, fontWeight = FontWeight.Bold, color = ur.text, style = Tabular, maxLines = 1)
            }
            Column(Modifier.width(availableWidth), horizontalAlignment = Alignment.End) {
                Text(stringResource(R.string.budget_available), fontSize = 12.sp, color = ur.muted, fontWeight = FontWeight.Medium)
                Text(money(totals.second, currency), fontSize = 15.sp, fontWeight = FontWeight.Bold, color = if (totals.second < 0) ur.negative else ur.text, style = Tabular, maxLines = 1)
            }
        }
        Hairline()
    }
}

/** A category: its name over what it needs (only when there's something to do), Assigned, and the pill. */
@Composable
private fun CategoryLine(row: CategoryRowModel, assignedWidth: Dp, availableWidth: Dp, open: () -> Unit, assign: () -> Unit, move: () -> Unit, last: Boolean) {
    // Settings can hide what targets still need; overspending always shows.
    val detail = if (row.needs && !LocalPreferences.current.showNeeded) null else row.detail
    Column(Modifier.background(ur.surface)) {
        Row(Modifier.fillMaxWidth().clickable(onClickLabel = stringResource(R.string.common_open_named, row.name), onClick = open).heightIn(min = 58.dp).padding(start = Gutter, end = Gutter, top = 9.dp, bottom = 9.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f).padding(end = 8.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(Money.own(row.name), fontSize = 16.sp, fontWeight = FontWeight.Medium, color = ur.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (detail != null) Row(verticalAlignment = Alignment.CenterVertically) {
                    if (row.snoozed && row.status != "negative") Icon(Icons.Outlined.Bedtime, null, Modifier.size(12.dp).padding(end = 2.dp), tint = ur.muted)
                    Text(detail, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis, lineHeight = 16.sp,
                        color = when (row.tone) { Tone.Negative -> ur.negative; Tone.Warning -> ur.warning; Tone.Positive -> ur.positive; Tone.Muted -> ur.muted })
                }
                if (row.progress != null && !row.snoozed) TargetBar(row.progress, row.status, Modifier.fillMaxWidth().padding(top = 3.dp))
            }
            Row(Modifier.width(assignedWidth).clip(RoundedCornerShape(8.dp)).clickable(role = Role.Button, onClickLabel = stringResource(R.string.category_assign_to, row.name), onClick = assign).padding(vertical = 8.dp),
                horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
                if (row.moved) Box(Modifier.padding(end = 4.dp).size(6.dp).clip(CircleShape).background(ur.accent))
                Text(row.assignedText, fontSize = 15.sp, color = if (row.assigned == 0L) ur.muted else ur.text2, style = Tabular, maxLines = 1)
            }
            Box(Modifier.width(availableWidth), contentAlignment = Alignment.CenterEnd) {
                PillText(row.availableText, row.status, move, stringResource(R.string.category_move_money_for, row.name))
            }
        }
        if (!last) Hairline(start = Gutter)
    }
}

/** The Available pill from text already formatted in the model. */
@Composable
private fun PillText(text: String, status: String, onClick: () -> Unit, label: String) {
    val colors = statusColors(status)
    Row(Modifier.heightIn(min = 32.dp).clip(RoundedCornerShape(16.dp)).background(colors.fill)
        .clickable(role = Role.Button, onClickLabel = label, onClick = onClick).padding(horizontal = 11.dp), verticalAlignment = Alignment.CenterVertically) {
        if (status == "credit") { Icon(Icons.Outlined.CreditCard, null, Modifier.size(13.dp), tint = colors.content); Spacer(Modifier.width(4.dp)) }
        Text(text, color = colors.content, fontSize = 15.sp, fontWeight = FontWeight.Bold, style = Tabular, maxLines = 1)
    }
}

/** YNAB's Uncategorized row: this month's spending with no category. */
@Composable
private fun UncategorizedLine(activity: Long, credit: Boolean, currency: String, assignedWidth: Dp, availableWidth: Dp, review: () -> Unit) {
    val status = if (credit) "credit" else "negative"
    Column(Modifier.background(ur.surface)) {
        Hairline()
        Row(Modifier.fillMaxWidth().clickable(onClick = review).heightIn(min = 58.dp).padding(horizontal = Gutter, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.category_uncategorized), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, color = ur.text)
                Text(stringResource(R.string.category_needed), style = MaterialTheme.typography.bodySmall, color = ur.negative, fontWeight = FontWeight.SemiBold)
            }
            Text(money(0, currency), Modifier.width(assignedWidth), textAlign = TextAlign.End, color = ur.muted, fontSize = 15.sp, style = Tabular)
            Box(Modifier.width(availableWidth), contentAlignment = Alignment.CenterEnd) {
                AvailablePill(activity, currency, status, onClick = review, label = stringResource(R.string.uncategorized_show))
            }
        }
        Hairline()
    }
}
