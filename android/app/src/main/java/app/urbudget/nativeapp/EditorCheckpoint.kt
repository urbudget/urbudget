package app.urbudget.nativeapp

import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/** UI lifetime does not own this lane: forced unmounts keep the latest raw form for recovery. */
class EditorCheckpoint(private val identity: String?, private val target: String, baseline: JSONObject,
                       private val failed: (String) -> Unit) {
    private val opening = baseline
    private var baseline: JSONObject? = null
    private val id = UUID.randomUUID().toString()
    private var epoch = 0L
    private var previous = ""
    @Volatile private var checkpointFailed = false
    private var budgetId: String? = null
    private var tail: Job? = null
    private fun enqueue(work: () -> Unit) {
        val before = tail
        tail = scope.launch {
            before?.join()
            try { work() } catch (e: Exception) { checkpointFailed = true; withContext(Dispatchers.Main) { failed(e.message ?: Words[R.string.editor_checkpoint_failed]) } }
        }
    }
    private fun owner(): String {
        budgetId?.let { return it }
        val entries = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries")
        val row = (0 until entries.length()).map { entries.getJSONObject(it) }.firstOrNull { it.optString("path") == identity }
            ?: error(Words[R.string.editor_budget_missing])
        return row.getString("id").also { budgetId = it }
    }
    fun save(values: JSONObject) {
        if (baseline == null) baseline = editorBaseline(opening, values)
        val encoded = values.toString()
        if (encoded == previous) return
        previous = encoded
        val next = ++epoch
        enqueue { if (next != epoch) return@enqueue
            NativeServices.call("editor_checkpoint", JSONObject().put("budgetId", owner()).put("id", id).put("epoch", next.toString())
            .put("target", target).put("baseline", baseline).put("values", JSONObject(encoded)))
            checkpointFailed = false }
    }
    fun discard(onlyIfActive: Boolean = false) {
        val savedEpoch = epoch
        enqueue {
            val budget = owner()
            if (onlyIfActive && checkpointFailed) return@enqueue
            NativeServices.call("editor_discard", JSONObject().put("budgetId", budget).put("id", id)
                .put("epoch", savedEpoch.toString()).put("onlyIfActive", onlyIfActive))
        }
    }
    companion object {
        /** Raw drafts are reviewed manually. Preserve the affected opening rows for comparison. */
        fun editorBaseline(budget: JSONObject, values: JSONObject): JSONObject {
            if (!budget.has("transactions") || !budget.has("accounts")) return budget
            val ids = mutableSetOf<String>()
            fun collect(value: Any?) {
                when (value) {
                    is String -> if (value.isNotBlank()) ids.add(value)
                    is JSONObject -> value.keys().forEach { collect(value.opt(it)) }
                    is JSONArray -> (0 until value.length()).forEach { collect(value.opt(it)) }
                }
            }
            collect(values)
            val base = JSONObject().put("name", budget.optString("name")).put("currency", budget.optString("currency"))
            for (table in listOf("accounts", "categories", "loans", "schedules", "payees", "payeeRules", "ynabMonths", "assignments")) {
                val rows = budget.optJSONArray(table) ?: continue
                base.put(table, JSONArray((0 until rows.length()).mapNotNull { rows.optJSONObject(it) }.filter { row ->
                    (if (table == "schedules") listOf("id") else listOf("id", "accountId", "categoryId", "name", "month")).any { row.optString(it).isNotBlank() && row.optString(it) in ids }
                }))
            }
            return base
        }
 private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO) }
}
