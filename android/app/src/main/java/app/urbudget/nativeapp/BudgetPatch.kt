package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject

/*
 * The Kotlin twin of crates/urbudget-core/src/patch.rs, for the patches `edit_budget` replies with.
 * Nothing here may change `base` or `put` into an existing array: published snapshots, guards and
 * query workers all read them.
 */

internal fun JSONObject.shallowCopy(): JSONObject {
    val copy = JSONObject()
    keys().forEach { copy.put(it, get(it)) }
    return copy
}

private fun JSONArray?.objects(): List<JSONObject> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it) }
private fun JSONArray?.strings(): List<String> =
    if (this == null) emptyList() else (0 until length()).map { optString(it) }

/** `base` with `patch` applied. Untouched records stay the same objects; an empty patch returns `base`. */
internal fun applyBudgetPatch(base: JSONObject, patch: JSONObject): JSONObject {
    if (patch.length() == 0) return base
    val next = base.shallowCopy()
    for (key in listOf("schemaVersion", "name", "currency")) if (patch.has(key)) next.put(key, patch.get(key))
    for (key in listOf("accounts", "categories", "transactions")) {
        patch.optJSONObject(key)?.let { next.put(key, applyCollection(base.optJSONArray(key), it)) }
    }
    patch.optJSONObject("assignments")?.let { next.put("assignments", applyAssignments(base.optJSONArray("assignments"), it)) }
    patch.optJSONObject("extras")?.let { extras ->
        extras.optJSONArray("delete").strings().forEach(next::remove)
        extras.optJSONObject("set")?.let { set -> set.keys().forEach { next.put(it, set.get(it)) } }
    }
    return next
}

private fun applyCollection(base: JSONArray?, change: JSONObject): JSONArray {
    val gone = change.optJSONArray("delete").strings().toHashSet()
    val rows = base.objects().filterTo(ArrayList()) { it.optString("id") !in gone }
    val index = HashMap<String, Int>()
    rows.forEachIndexed { i, row -> index[row.optString("id")] = i }
    for (row in change.optJSONArray("upsert").objects()) {
        val id = row.optString("id")
        val at = index[id]
        if (at != null) rows[at] = row else { index[id] = rows.size; rows.add(row) }
    }
    change.optJSONArray("order")?.let { order ->
        val rank = HashMap<String, Int>()
        order.strings().forEachIndexed { i, id -> rank[id] = i }
        rows.sortBy { rank[it.optString("id")] ?: Int.MAX_VALUE } // Stable: unknown ids last, in place.
    }
    return JSONArray(rows)
}

private fun assignmentKey(row: JSONObject) = row.optString("month") to row.optString("categoryId")

private fun applyAssignments(base: JSONArray?, change: JSONObject): JSONArray {
    val gone = change.optJSONArray("delete").objects().mapTo(HashSet(), ::assignmentKey)
    val rows = base.objects().filterTo(ArrayList()) { assignmentKey(it) !in gone }
    val index = HashMap<Pair<String, String>, Int>()
    rows.forEachIndexed { i, row -> index[assignmentKey(row)] = i }
    for (row in change.optJSONArray("upsert").objects()) {
        val key = assignmentKey(row)
        val at = index[key]
        if (at != null) rows[at] = row else { index[key] = rows.size; rows.add(row) }
    }
    return JSONArray(rows)
}

private val TYPED = setOf("schemaVersion", "name", "currency", "accounts", "categories", "transactions", "assignments")

/**
 * What a transform changed on a shallow copy of `base`: a `Patch` with whole records, or empty. It
 * goes by identity, so a rebuilt but equal array or record comes along: the engine diffs what it
 * ends up with, so that costs nothing. A record changed where it lies wouldn't be seen at all.
 */
internal fun budgetPatch(base: JSONObject, edited: JSONObject): JSONObject {
    val patch = JSONObject()
    for (key in listOf("schemaVersion", "name", "currency")) {
        if (edited.has(key) && base.opt(key) != edited.opt(key)) patch.put(key, edited.get(key))
    }
    for (key in listOf("accounts", "categories", "transactions")) {
        if (base.opt(key) !== edited.opt(key)) {
            collectionChange(base.optJSONArray(key).objects(), edited.optJSONArray(key).objects())?.let { patch.put(key, it) }
        }
    }
    if (base.opt("assignments") !== edited.opt("assignments")) {
        assignmentChange(base.optJSONArray("assignments").objects(), edited.optJSONArray("assignments").objects())?.let { patch.put("assignments", it) }
    }
    val set = JSONObject()
    val delete = JSONArray()
    edited.keys().forEach { key -> if (key !in TYPED && (!base.has(key) || base.opt(key) !== edited.opt(key))) set.put(key, edited.get(key)) }
    base.keys().forEach { key -> if (key !in TYPED && !edited.has(key)) delete.put(key) }
    if (set.length() > 0 || delete.length() > 0) patch.put("extras", JSONObject().put("set", set).put("delete", delete))
    return patch
}

private fun collectionChange(before: List<JSONObject>, after: List<JSONObject>): JSONObject? {
    val previous = before.associateBy { it.optString("id") }
    val present = after.mapTo(HashSet()) { it.optString("id") }
    val upsert = after.filter { previous[it.optString("id")] !== it }
    val delete = before.map { it.optString("id") }.filter { it !in present }
    // Without an order, survivors keep their place and new records go last.
    val implied = before.map { it.optString("id") }.filter { it in present } + after.map { it.optString("id") }.filter { it !in previous }
    val order = after.map { it.optString("id") }
    if (upsert.isEmpty() && delete.isEmpty() && implied == order) return null
    val change = JSONObject().put("upsert", JSONArray(upsert)).put("delete", JSONArray(delete))
    if (implied != order) change.put("order", JSONArray(order))
    return change
}

private fun assignmentChange(before: List<JSONObject>, after: List<JSONObject>): JSONObject? {
    val previous = before.associateBy(::assignmentKey)
    val present = after.mapTo(HashSet(), ::assignmentKey)
    val upsert = after.filter { previous[assignmentKey(it)] !== it }
    val delete = before.filter { assignmentKey(it) !in present }
        .map { JSONObject().put("month", it.optString("month")).put("categoryId", it.optString("categoryId")) }
    if (upsert.isEmpty() && delete.isEmpty()) return null
    return JSONObject().put("upsert", JSONArray(upsert)).put("delete", JSONArray(delete))
}

/** Whether two copies hold the same budget. A reload sorts assignments; a patched copy keeps them where they were added. */
internal fun sameBudget(left: JSONObject, right: JSONObject): Boolean {
    fun sorted(budget: JSONObject) = budget.shallowCopy().put("assignments",
        JSONArray(budget.optJSONArray("assignments").objects().sortedBy { "${it.optString("month")}\u0000${it.optString("categoryId")}" }))
    return sameJson(sorted(left), sorted(right))
}
