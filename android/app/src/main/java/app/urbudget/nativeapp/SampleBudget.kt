package app.urbudget.nativeapp

import org.json.JSONObject

/**
 * Whether a budget is a sample to try things on: marked when it's made, as the desktop marks its
 * sample. Samples made before they were marked are known by the engine sample's account and
 * purchase (a fresh start from one doesn't carry the purchase).
 */
internal fun isSampleBudget(budget: JSONObject): Boolean =
    budget.optJSONObject("sample") != null ||
        (budget.records("accounts").any { it.optString("id") == "sample-checking" } &&
            budget.records("transactions").any { it.optString("id") == "sample-market" })

/** The budget leaving a sample goes to: the one it was opened from, else the one opened most recently. */
internal fun sampleExit(sample: JSONObject, library: List<JSONObject>): JSONObject? {
    val others = library.filter { !it.optBoolean("current") && !it.optBoolean("missing") }
    val from = sample.optJSONObject("sample")?.optString("from").orEmpty()
    return others.firstOrNull { it.optString("id") == from } ?: others.firstOrNull()
}
