package app.urbudget.nativeapp.ui

import androidx.compose.runtime.Immutable
import app.urbudget.nativeapp.CategoryStatuses
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.Uncategorized
import org.json.JSONArray
import org.json.JSONObject

/** How a category's line under its name reads: muted, amber, red or green. */
internal enum class Tone { Muted, Warning, Negative, Positive }

/**
 * One category as the budget list draws it. Plain values with value equality, so after an edit
 * only the rows whose figures changed recompose; nothing here is parsed or formatted while drawing.
 */
@Immutable
internal data class CategoryRowModel(
    val id: String, val name: String, val group: String, val hidden: Boolean, val payment: Boolean,
    val detail: String?, val tone: Tone, val snoozed: Boolean, val progress: Int?, val status: String,
    val assigned: Long, val assignedText: String, val available: Long, val availableText: String,
    val underfunded: Boolean, val overfunded: Boolean, val moved: Boolean,
    /** The detail is what a target still needs (Settings can hide it). */
    val needs: Boolean = false,
)

@Immutable
internal data class GroupModel(val name: String, val rows: List<CategoryRowModel>)

@Immutable
internal data class BudgetModel(
    val rta: Long,
    val groups: List<GroupModel>,
    val uncategorized: Long,
    val uncategorizedCredit: Long,
    val needsCategory: Uncategorized.Counts,
    val shortfalls: List<Pair<String, Long>>,
    val futureAssigned: Long,
    val futureReadyToAssign: Long,
    val monthNote: String?,
    /** The longest figures each column must fit, for measuring its width once. */
    val widestAssigned: List<String>,
    val widestAvailable: List<String>,
)

/** The budget's groups in budget order, empty ones included (desktop's groupNames with empty). */
internal fun groupNames(budget: JSONObject): List<String> {
    val order = budget.optJSONArray("groupOrder") ?: JSONArray()
    val names = LinkedHashSet<String>()
    for (i in 0 until order.length()) order.optString(i).takeIf { it.isNotBlank() }?.let(names::add)
    budget.rows("categories").forEach { if (!it.optBoolean("hidden")) names.add(it.text("group", "Everyday")) }
    return names.toList()
}

/** Builds the list's model off the main thread, once per budget snapshot and month. */
internal fun buildBudgetModel(budget: JSONObject, summary: JSONObject, month: String): BudgetModel {
    val currency = budget.text("currency")
    val monthFirst = budget.text("dateFormat") == "mdy"
    val totals = HashMap<String, JSONObject>()
    summary.rows("categories").forEach { totals[it.text("id")] = it }
    val order = budget.optJSONArray("categoryOrder") ?: JSONArray()
    val ranks = HashMap<String, Int>(order.length())
    for (i in 0 until order.length()) ranks[order.optString(i)] = i
    // Categories with money moved to or from another one this month get a marker by Assigned.
    val moved = HashSet<String>()
    budget.rows("moneyMoves").forEach { m ->
        if (m.text("month") != month) return@forEach
        val changes = m.rows("changes")
        if (m.text("kind") == "merge" || m.text("kind") == "move" && changes.size > 1) changes.forEach { c -> if (c.optLong("amount") != 0L) moved.add(c.text("categoryId")) }
    }
    val rows = budget.rows("categories").sortedBy { ranks[it.text("id")] ?: Int.MAX_VALUE }.map { category ->
        val id = category.text("id")
        val total = totals[id]
        val state = CategoryStatuses.of(category, total, month)
        val available = total?.optLong("available") ?: 0
        val assigned = total?.optLong("assigned") ?: 0
        val target = category.optLong("target")
        val words = if (state.status == "underfunded" || state.eventually > 0) CategoryStatuses.targetWords(category, total, state, monthFirst) else null
        val detail = when {
            state.status == "credit" -> Words[R.string.category_overspent_card]
            state.status == "negative" -> Words[R.string.category_overspent]
            words != null -> words.text(money(words.amount ?: 0, currency))
            state.snoozed -> Words[R.string.category_snoozed]
            else -> null
        }
        CategoryRowModel(
            id = id, name = category.text("name"), group = category.text("group", "Everyday"), hidden = category.optBoolean("hidden"),
            payment = category.text("paymentAccountId").isNotBlank(), detail = detail,
            tone = when (state.status) { "negative", "credit" -> Tone.Negative; "underfunded" -> Tone.Warning; else -> Tone.Muted },
            snoozed = state.snoozed, progress = state.progress, status = state.status,
            assigned = assigned, assignedText = money(assigned, currency), available = available, availableText = money(available, currency),
            underfunded = state.status == "underfunded", overfunded = target > 0 && available > target, moved = id in moved,
            needs = state.status != "credit" && state.status != "negative" && words != null,
        )
    }
    val byGroup = rows.filterNot { it.hidden }.groupBy { it.group }
    val groups = groupNames(budget).map { GroupModel(it, byGroup[it].orEmpty()) } +
        rows.filter { it.hidden }.takeIf { it.isNotEmpty() }.let { hidden -> if (hidden == null) emptyList() else listOf(GroupModel(HiddenGroup, hidden)) }
    val groupTotals = groups.flatMap { g -> listOf(money(g.rows.sumOf { it.assigned }, currency) to money(g.rows.sumOf { it.available }, currency)) }
    fun widest(values: List<String>) = values.sortedByDescending { it.length }.take(2)
    val note = budget.rows("ynabMonths").firstOrNull { it.text("month").startsWith(month) }?.text("note")?.takeIf { it.isNotBlank() }
    return BudgetModel(
        rta = summary.optLong("readyToAssign"), groups = groups,
        uncategorized = summary.optLong("uncategorized"), uncategorizedCredit = summary.optLong("uncategorizedCredit"),
        needsCategory = Uncategorized(budget).counts(budget),
        shortfalls = summary.rows("futureShortfalls").map { it.text("month") to it.optLong("amount") },
        futureAssigned = summary.optLong("futureAssigned"), futureReadyToAssign = summary.optLong("futureReadyToAssign"),
        monthNote = note,
        widestAssigned = widest(rows.map { it.assignedText } + groupTotals.map { it.first } + money(0, currency)),
        widestAvailable = widest(rows.map { it.availableText } + groupTotals.map { it.second } + money(summary.optLong("uncategorized"), currency)),
    )
}

/** The views a phone's filter button offers, with what each counts. */
internal val BudgetViews = listOf("all" to R.string.view_all, "underfunded" to R.string.view_underfunded, "overfunded" to R.string.view_overfunded,
    "available" to R.string.view_available, "overspent" to R.string.view_overspent, "snoozed" to R.string.view_snoozed)

internal fun CategoryRowModel.inView(view: String) = when (view) {
    "underfunded" -> underfunded
    "overfunded" -> overfunded
    "available" -> available > 0
    "overspent" -> available < 0
    "snoozed" -> snoozed
    else -> true
}
