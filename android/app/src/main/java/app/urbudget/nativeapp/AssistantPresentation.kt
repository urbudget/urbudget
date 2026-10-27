package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject

/** Append tokens once; publish at a bounded cadence instead of copying a reply for every token. */
internal class AssistantStreamBuffer {
    private val reply = StringBuilder()
    private val reasoning = StringBuilder()
    private var changed = false

    fun append(text: String, thinking: Boolean = false) {
        if (text.isEmpty()) return
        (if (thinking) reasoning else reply).append(text)
        changed = true
    }

    fun clearReply() { reply.clear(); changed = true }
    fun clear() { reply.clear(); reasoning.clear(); changed = false }
    fun drain(): Pair<String, String>? = if (!changed) null else {
        changed = false
        reply.toString() to reasoning.toString()
    }
}

/** All comparisons run on a worker. Unchanged messages retain their Compose/render caches. */
internal data class AssistantThreadPresentation(val thread: JSONObject, val entries: List<JSONObject>)

internal fun assistantThreadPresentation(thread: JSONObject, previous: AssistantThreadPresentation?): AssistantThreadPresentation {
    val sameThread = previous?.thread?.optString("id") == thread.optString("id")
    if (sameThread && sameJson(previous?.thread, thread)) return previous!!
    val entries = thread.records("entries").mapIndexed { index, entry ->
        val old = if (sameThread) previous?.entries?.getOrNull(index) else null
        if (old != null && sameJson(old, entry)) old else entry
    }
    return AssistantThreadPresentation(thread, entries)
}

/**
 * What the user chose on a proposal card, as `assistant_revise` takes it (desktop ProposalCard):
 * `skip` lists the keys of rows left out, `set` the rows given another category (null: Ready to
 * Assign) or amount. Each choice is made from the card's current `choices`.
 */
internal object ProposalChoices {
    /** The assistant's own version: nothing left out or changed. */
    fun original(): JSONObject = revision(emptyList(), emptyList())

    /** `choices` with the row `key` left out, or put back when it was; one put back loses an amount of nothing given to it. */
    fun toggle(choices: JSONObject?, key: String, skipped: Boolean): JSONObject {
        val skip = choices.strings("skip")
        val set = choices?.records("set").orEmpty()
        return if (skipped) revision(skip.filter { it != key }, set.filterNot { it.optString("key") == key && it.has("amount") && !it.isNull("amount") && it.optLong("amount") == 0L })
        else revision(skip + key, set)
    }

    /** `choices` with `change` (a category and/or an amount) given to the row `key`, over what it was given before. */
    fun change(choices: JSONObject?, key: String, change: JSONObject): JSONObject {
        val set = choices?.records("set").orEmpty()
        val given = JSONObject(set.firstOrNull { it.optString("key") == key }?.toString() ?: "{}")
        change.keys().forEach { given.put(it, change.get(it)) }
        return revision(choices.strings("skip"), set.filterNot { it.optString("key") == key } + given.put("key", key))
    }

    /**
     * What saving a row's editor gives it: `category` (null: Ready to Assign) when it isn't the one
     * the row has, and the typed amount when it isn't the row's; null when nothing changed. An
     * amount typed without a sign keeps the row's sign. Throws when the amount doesn't read.
     */
    fun edit(row: JSONObject, category: String?, amount: String): JSONObject? {
        val edit = row.optJSONObject("edit") ?: return null
        val change = JSONObject()
        val had = edit.optString("categoryId").takeUnless { edit.isNull("categoryId") || it.isEmpty() }
        if (edit.optBoolean("category") && category != had) change.put("category", category ?: JSONObject.NULL)
        if (edit.optBoolean("amount")) {
            val was = if (row.has("amount") && !row.isNull("amount")) row.optLong("amount") else null
            val typed = amount.trim()
            var cents = Money.parse(typed)
            if (!typed.startsWith("-") && !typed.startsWith("+") && (was ?: 0L) < 0) cents = -Math.abs(cents)
            if (cents != was) change.put("amount", cents)
        }
        return change.takeIf { it.length() > 0 }
    }

    /** How much of a card Apply makes: `chosen` of its `total` parts ("Apply 2 of 3"). */
    data class Partial(val chosen: Int, val total: Int)

    /** How much of the card Apply makes while the user is choosing and some of its rows are left out; null otherwise. The card words it. */
    fun partial(rows: List<JSONObject>, choosing: Boolean): Partial? {
        val keyed = rows.filter { it.optString("key").isNotEmpty() }
        val chosen = keyed.count { !it.optBoolean("skipped") }
        return if (choosing && chosen < keyed.size) Partial(chosen, keyed.size) else null
    }

    private fun revision(skip: List<String>, set: List<JSONObject>) = JSONObject().put("skip", JSONArray(skip)).put("set", JSONArray(set))
    private fun JSONObject?.strings(key: String): List<String> =
        this?.optJSONArray(key)?.let { list -> (0 until list.length()).map(list::optString) }.orEmpty()
}
