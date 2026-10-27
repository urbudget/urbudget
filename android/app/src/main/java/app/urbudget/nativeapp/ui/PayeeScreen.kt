package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.PayeeInfo
import app.urbudget.nativeapp.PayeeRules
import app.urbudget.nativeapp.payeeDetail
import app.urbudget.nativeapp.payeeIndex
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import androidx.compose.ui.res.stringResource
import java.util.Locale
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONArray
import org.json.JSONObject
import java.time.YearMonth
import java.util.UUID

/** "1,234 transactions": a plural from the resources, the count grouped and in Western digits. */
private fun counted(id: Int, n: Int, vararg more: Any) = Words.countAs(id, n.toLong(), "%,d".format(Locale.ROOT, n), *more)

/** A payee row, prepared away from composition. */
private data class PayeeLine(val name: String, val record: JSONObject, val subtitle: String)

/** An import rule row. */
private data class RuleLine(val rule: JSONObject, val title: String, val subtitle: String)

private suspend fun payeeLines(budget: JSONObject): List<PayeeLine> {
    val categories = budget.rows("categories").associate { it.text("id") to it.text("name") }
    val context = currentCoroutineContext()
    return payeeIndex(budget).mapIndexed { index, info ->
        if (index % 256 == 0) context.ensureActive()
        PayeeLine(info.name, info.record, Money.joined(listOfNotNull(
            if (info.count > 0) "${counted(R.plurals.payee_transactions, info.count)} · ${Money.date(info.last)}" else Words[R.string.payee_not_used],
            info.default?.let { Words[R.string.payee_default_named, if (it == Unassigned) Words[R.string.category_unassigned] else categories[it] ?: Words[R.string.category_deleted]] })))
    }
}

private fun ruleLines(budget: JSONObject): List<RuleLine> {
    val categories = budget.rows("categories").associate { it.text("id") to it.text("name") }
    return budget.rows("payeeRules").map { rule ->
        RuleLine(rule, Words[R.string.rule_contains, rule.text("match")], Money.joined(listOfNotNull(rule.text("payee").ifBlank { null }?.let { Words[R.string.rule_rename_to_named, it] },
            rule.text("categoryId").ifBlank { null }?.let { if (it == Unassigned) Words[R.string.category_unassigned] else categories[it] ?: Words[R.string.category_deleted] }))
            .ifBlank { Words[R.string.rule_keep_name] })
    }
}

/** Payees & rules: search the payees, open one for its details; then the import rules. */
@Composable
internal fun PayeesPage(budget: JSONObject, vm: BudgetViewModel, edit: (String, JSONObject?) -> Unit) {
    var search by rememberSaveable { mutableStateOf("") }
    val language = languageKey()
    val all = rememberPrepared(budget, language) { payeeLines(budget) }
    val shown = rememberPrepared(all, search) {
        val query = search.trim()
        if (query.isEmpty()) all else all?.filter { it.name.contains(query, ignoreCase = true) }
    }
    val rules = rememberPrepared(budget, language) { ruleLines(budget) }
    val pending = rememberPrepared(budget) { PayeeRules.changes(budget, budget.rows("payeeRules")) }
    val reapply = { withCurrentBudget(budget, vm) { vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "reapplyPayeeRules"))) } }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom)) {
        item(key = "payees:title") {
            SectionTitle(stringResource(R.string.payees_title), stringResource(R.string.payees_detail)) { IconButton({ edit("payee", null) }) { Icon(Icons.Outlined.Add, stringResource(R.string.payee_add), tint = ur.accent) } }
        }
        item(key = "payees:search") { SearchField(search, { search = it }, stringResource(R.string.payees_search), Modifier.padding(top = 4.dp, bottom = 10.dp)) }
        when {
            shown == null -> item(key = "payees:loading") { LinearProgressIndicator(Modifier.fillMaxWidth().padding(vertical = 12.dp), color = ur.accent) }
            shown.isEmpty() -> item(key = "payees:empty") {
                Text(if (search.isBlank()) stringResource(R.string.payees_empty) else stringResource(R.string.reports_no_payees_match), Modifier.padding(4.dp), color = ur.muted)
            }
            else -> itemsIndexed(shown, key = { _, line -> "payee:${line.name}" }, contentType = { _, _ -> "payee" }) { index, line ->
                ListRow(line.name, Modifier.cardSegment(index, shown.size, cardFill()), subtitle = line.subtitle, onClick = { edit("payee", line.record) }, divider = index < shown.lastIndex)
            }
        }
        item(key = "rules:title") {
            Box(Modifier.padding(top = 16.dp)) {
                SectionTitle(stringResource(R.string.rules_title), stringResource(R.string.rules_detail)) { IconButton({ edit("rule", null) }) { Icon(Icons.Outlined.Add, stringResource(R.string.rule_add), tint = ur.accent) } }
            }
        }
        if ((pending ?: 0) > 0) item(key = "rules:pending") {
            Notice(counted(R.plurals.rules_pending, pending ?: 0), Modifier.padding(bottom = 10.dp),
                tone = "info", action = stringResource(R.string.common_apply), onAction = reapply, icon = Icons.Outlined.AutoFixHigh)
        }
        val ruleList = rules.orEmpty()
        if (ruleList.isEmpty()) item(key = "rules:empty") {
            Text(stringResource(R.string.rules_empty), Modifier.padding(4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
        } else itemsIndexed(ruleList, key = { _, line -> "rule:${line.rule.text("id")}" }, contentType = { _, _ -> "rule" }) { index, line ->
            ListRow(line.title, Modifier.cardSegment(index, ruleList.size, cardFill()), subtitle = line.subtitle, onClick = { edit("rule", line.rule) }, divider = index < ruleList.lastIndex)
        }
        if (ruleList.isNotEmpty()) item(key = "rules:apply") {
            TextButton(reapply, Modifier.padding(top = 4.dp)) { Text(stringResource(R.string.rules_apply_existing), color = ur.accent) }
        }
    }
}

private data class RecentLine(val key: String, val title: String, val detail: String, val amount: String, val positive: Boolean)

/** A payee's page, prepared away from composition. */
private data class PayeeSheet(val info: PayeeInfo?, val count: Int, val total: Long, val totalText: String, val recent: List<RecentLine>,
                              val names: Set<String>, val merge: List<Pair<String, String>>)

private suspend fun payeeSheet(budget: JSONObject, name: String?): PayeeSheet {
    val currency = budget.text("currency")
    val index = payeeIndex(budget)
    val accounts = budget.rows("accounts").associate { it.text("id") to it.text("name") }
    val categories = budget.rows("categories").associate { it.text("id") to it.text("name") }
    val detail = name?.let { payeeDetail(budget, it) }
    val recent = detail?.recent.orEmpty().map { t ->
        val category = if (t.splits > 1) Words[R.string.category_split_count, "${t.splits}"] else categories[t.categoryId] ?: Words[R.string.category_unassigned]
        RecentLine(t.id.ifBlank { "${t.date}:${t.amount}" }, Money.date(t.date), Money.joined(listOfNotNull(accounts[t.accountId], category)), money(t.amount, currency), t.amount > 0)
    }
    return PayeeSheet(index.firstOrNull { it.name == name }, detail?.count ?: 0, detail?.total ?: 0L, money(detail?.total ?: 0L, currency), recent,
        index.map { it.name }.toSet(), listOf("" to Words[R.string.payee_choose_a]) + index.filter { it.name != name }.map { it.name to it.name })
}

/**
 * A payee: how often it's used, its latest transactions, its default category (or the one used
 * last), and renaming or merging it into another payee (PayeesPage.tsx).
 */
@Composable
internal fun PayeeDialog(original: JSONObject?, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val originalName = original?.text("name")?.ifBlank { null }
    var name by remember { mutableStateOf(originalName ?: "") }
    var category by remember { mutableStateOf(original?.text("categoryId") ?: "") }
    var picking by remember { mutableStateOf(false) }
    // The categories' available amounts as the picker opens.
    var pickerSummary by remember { mutableStateOf<JSONObject?>(null) }
    val sheet = rememberPrepared(openedBudget, originalName, languageKey()) { payeeSheet(openedBudget, originalName) }
    val categories = remember(budget) { budget.rows("categories").associate { it.text("id") to it.text("name") } }
    val unassignedName = stringResource(R.string.category_unassigned)
    val deletedName = stringResource(R.string.category_deleted)
    fun categoryName(id: String) = when (id) { "" -> null; Unassigned -> unassignedName; else -> categories[id] ?: deletedName }
    val lastUsed = sheet?.info?.lastUsed
    val placeholder = if (lastUsed != null) stringResource(R.string.payee_last_used, if (lastUsed.isBlank()) stringResource(R.string.payee_no_category) else categoryName(lastUsed).orEmpty())
        else stringResource(R.string.payee_no_default)
    val merging = name.trim().let { it.isNotEmpty() && it != originalName && sheet?.names?.contains(it) == true }
    FormDialog(if (original == null) stringResource(R.string.payee_add) else stringResource(R.string.payee_label), close, onConfirm = {
        withCurrentBudget(openedBudget, vm) {
            vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "savePayee").put("from", originalName ?: JSONObject.NULL)
                .put("record", (original?.copyJson() ?: JSONObject().put("id", UUID.randomUUID().toString())).put("name", name.trim()).put("categoryId", category.ifBlank { null } ?: JSONObject.NULL))))
            close()
        }
    }, enabled = name.isNotBlank()) {
        if (originalName != null) Column {
            Text(originalName, style = MaterialTheme.typography.titleLarge, color = ur.text)
            Text(if (sheet == null) "…" else if (sheet.count == 0) stringResource(R.string.payee_no_transactions) else counted(R.plurals.payee_count_total, sheet.count, sheet.totalText),
                style = MaterialTheme.typography.bodyMedium, color = ur.muted)
        }
        PickerField(stringResource(R.string.payee_default_category), categoryName(category), placeholder, { pickerSummary = vm.state.value.summary; picking = true }, onClear = { category = "" })
        Text(stringResource(R.string.payee_default_category_help),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
        Field(name, { name = it }, if (original == null) stringResource(R.string.payee_name) else stringResource(R.string.name_label))
        if (originalName != null) {
            Choice(stringResource(R.string.payee_merge), "", sheet?.merge ?: listOf("" to stringResource(R.string.payee_choose_a))) { if (it.isNotBlank()) name = it }
            Text(if (merging) stringResource(R.string.payee_merge_exists, name.trim()) else stringResource(R.string.payee_rename_help),
                style = MaterialTheme.typography.bodySmall, color = if (merging) ur.warning else ur.muted)
        }
        if (sheet != null && sheet.recent.isNotEmpty()) {
            Text(stringResource(R.string.payee_recent), style = MaterialTheme.typography.titleSmall, color = ur.text)
            ListCard {
                sheet.recent.forEachIndexed { index, line ->
                    ListRow(line.title, subtitle = line.detail, value = line.amount, valueColor = if (line.positive) ur.positive else ur.text, divider = index < sheet.recent.lastIndex)
                }
            }
        }
    }
    if (picking) CategoryPickerSheet(budget, pickerSummary, YearMonth.now().toString(), category, stringResource(R.string.payee_default_category), includeUnassigned = true, includePayment = false,
        onPick = { category = it; picking = false }) { picking = false }
}

@Composable
internal fun RuleDialog(original: JSONObject?, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val id = remember { original?.text("id")?.ifBlank { null } ?: UUID.randomUUID().toString() }
    var match by remember { mutableStateOf(original?.text("match") ?: "") }
    var payee by remember { mutableStateOf(original?.text("payee") ?: "") }
    var category by remember { mutableStateOf(original?.text("categoryId") ?: "") }
    val categories = remember(budget, languageKey()) { listOf("" to Words[R.string.rule_keep_category], Unassigned to Words[R.string.category_unassigned]) + budget.rows("categories").filter { it.text("paymentAccountId").isBlank() }.map { it.text("id") to it.text("name") } }
    val ready = match.isNotBlank() && (payee.isNotBlank() || category.isNotBlank())
    // What this rule, as typed, would change in the transactions you already have.
    val changes = rememberPrepared(openedBudget, match, payee, category) {
        if (match.isBlank()) 0 else {
            val draft = (original?.copyJson() ?: JSONObject().put("id", id)).put("match", match.trim()).put("payee", payee.trim()).put("categoryId", category.ifBlank { null } ?: JSONObject.NULL)
            val rules = openedBudget.rows("payeeRules").map { if (it.text("id") == id) draft else it }.let { if (it.any { rule -> rule === draft }) it else it + draft }
            PayeeRules.changes(openedBudget, rules, setOf(id))
        }
    }
    FormDialog(if (original == null) stringResource(R.string.rule_add) else stringResource(R.string.rule_title), close, onConfirm = {
        withCurrentBudget(openedBudget, vm) {
            vm.upsert("payeeRules", (original?.copyJson() ?: JSONObject().put("id", id)).put("match", match.trim()).put("payee", payee.trim())
                .put("categoryId", category.ifBlank { null } ?: JSONObject.NULL)); close()
        }
    }, enabled = ready) {
        Field(match, { match = it }, stringResource(R.string.rule_match_field))
        Field(payee, { payee = it }, stringResource(R.string.rule_rename_to))
        Choice(stringResource(R.string.category_label), category, categories, { category = it })
        when {
            match.isBlank() -> Text(stringResource(R.string.rule_match_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            !ready -> Text(stringResource(R.string.rule_choose_something), style = MaterialTheme.typography.bodySmall, color = ur.warning)
            (changes ?: 0) > 0 -> Text(counted(R.plurals.rule_would_change, changes ?: 0),
                style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
        Text(stringResource(R.string.rule_how_it_matches),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
        if (original != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                listOf("up" to stringResource(R.string.rule_move_earlier), "down" to stringResource(R.string.rule_move_later)).forEach { (direction, label) ->
                    SecondaryButton(label, { withCurrentBudget(openedBudget, vm) { vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "reorderPayeeRule").put("id", id).put("direction", direction))); close() } }, Modifier.weight(1f))
                }
            }
            Text(stringResource(R.string.rule_order_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            Row(verticalAlignment = Alignment.CenterVertically) {
                TextButton({ withCurrentBudget(openedBudget, vm) { vm.runCommand("workflow", JSONObject().put("action", JSONObject().put("type", "reapplyPayeeRules").put("ruleIds", JSONArray().put(id)))); close() } }) {
                    Text(stringResource(R.string.rule_apply_existing), color = ur.accent, fontWeight = FontWeight.SemiBold)
                }
            }
            TextButton({ withCurrentBudget(openedBudget, vm) { vm.remove("payeeRules", id); close() } }) { Text(stringResource(R.string.rule_delete), color = ur.negative) }
        }
    }
}
