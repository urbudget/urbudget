package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject

/** Expectations retain the read-only published snapshot; ledger comparisons run on the worker. */
class BudgetEditGuard private constructor(
    private val identity: String?,
    private val source: JSONObject?,
    private val whole: Boolean,
    private val selected: List<Pair<String, Map<String, String>>>,
    private val fields: List<String>,
) {
    internal fun verify(current: JSONObject, currentIdentity: String?) {
        require(identity == currentIdentity) { Words[R.string.guard_budget_switched] }
        require(!whole || source == null || sameJson(source, current)) { STALE }
        for ((collection, keys) in selected) {
            val before = source?.records(collection)?.firstOrNull { row -> matches(row, collection, keys) }
            val actual = current.records(collection).firstOrNull { row -> matches(row, collection, keys) }
            require(if (before == null) actual == null else actual != null && sameJson(before, actual)) { STALE }
        }
        for (key in fields) {
            require((source?.has(key) == true) == current.has(key) && sameJson(source?.opt(key), current.opt(key))) { STALE }
        }
    }

    companion object {
        private val STALE get() = Words[R.string.guard_values_changed]
        private fun matches(row: JSONObject, collection: String, keys: Map<String, String>) = keys.all { (key, value) ->
            if (collection == "ynabMonths" && key == "month") row.optString(key).take(7) == value.take(7) else row.optString(key) == value
        }
        fun whole(budget: JSONObject?, identity: String?) = BudgetEditGuard(identity, budget, true, emptyList(), emptyList())
        fun record(budget: JSONObject?, identity: String?, collection: String, id: String): BudgetEditGuard = records(budget, identity, listOf(collection to mapOf((if (collection == "loans") "accountId" else "id") to id)))
        fun recordSet(budget: JSONObject?, identity: String?, collection: String, ids: List<String>): BudgetEditGuard = records(budget, identity, ids.map { collection to mapOf((if (collection == "loans") "accountId" else "id") to it) })
        private fun records(budget: JSONObject?, identity: String?, selected: List<Pair<String, Map<String, String>>>, fields: List<String> = emptyList()): BudgetEditGuard = BudgetEditGuard(identity, budget, false, selected, fields)
        fun command(budget: JSONObject?, identity: String?, method: String, params: JSONObject): BudgetEditGuard {
            if (method == "saveTransaction") return record(budget, identity, "transactions", params.optJSONObject("transaction")?.optString("id").orEmpty())
            if (method != "workflow") return whole(budget, identity)
            val action = params.optJSONObject("action") ?: return whole(budget, identity)
            return when (action.optString("type")) {
                "bulk" -> { val ids = action.optJSONArray("ids") ?: JSONArray(); records(budget, identity, (0 until ids.length()).map { "transactions" to mapOf("id" to ids.optString(it)) }) }
                "savePayee" -> record(budget, identity, "payees", action.optJSONObject("record")?.optString("id").orEmpty())
                "assignAmount" -> records(budget, identity, listOf("assignments" to mapOf("month" to action.optString("month"), "categoryId" to action.optString("categoryId"))))
                "setBudgetPlanning" -> records(budget, identity, listOf("ynabMonths" to mapOf("month" to action.optString("month"))), listOf("expectedIncome"))
                "organizeCategory" -> records(budget, identity, listOf("categories" to mapOf("id" to action.optString("id"))), listOf("categoryOrder", "groupOrder"))
                "reorderPayeeRule" -> records(budget, identity, emptyList(), listOf("payeeRules"))
                else -> whole(budget, identity)
            }
        }
    }
}

/** JSONObject key order is not a revision; arrays and scalar values remain significant. */
internal fun sameJson(left: Any?, right: Any?): Boolean = when {
    left is JSONObject && right is JSONObject -> {
        val keys = left.keys().asSequence().toSet()
        keys == right.keys().asSequence().toSet() && keys.all { sameJson(left.opt(it), right.opt(it)) }
    }
    left is JSONArray && right is JSONArray -> left.length() == right.length() && (0 until left.length()).all { sameJson(left.opt(it), right.opt(it)) }
    left is Number && right is Number -> {
        val a = left.toString().toBigDecimalOrNull(); val b = right.toString().toBigDecimalOrNull()
        a != null && b != null && a.compareTo(b) == 0
    }
    else -> left == right
}

/** Whole-budget replacement also supports unreadable source bytes and an empty new file. */
class BudgetReplacementGuard internal constructor(private val source: JSONObject?, private val expected: String?, private val identity: String?) {
    internal fun verify(current: String?, currentIdentity: String?) {
        require(identity == currentIdentity) { Words[R.string.guard_replacement_switched] }
        val before = source ?: expected?.let { runCatching { JSONObject(it) }.getOrNull() }
        val now = current?.let { runCatching { JSONObject(it) }.getOrNull() }
        require(if (before != null && now != null) sameJson(before, now) else source == null && expected == current) {
            Words[R.string.guard_replacement_changed]
        }
    }
}
