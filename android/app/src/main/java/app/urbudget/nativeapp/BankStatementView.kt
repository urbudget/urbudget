package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Locale

/** What a line left out is, by why (bank_statement.rs `Reason`, the desktop's LEFT_OUT). */
internal val leftOutNames = mapOf(
    "pending" to "Not booked yet", "declined" to "Declined or cancelled", "otherAccount" to "Another account's",
    "otherCurrency" to "In another currency", "ownBalances" to "Between your own balances", "balance" to "A balance or a total",
    "aboveHeader" to "Above the header", "noDate" to "No date", "noAmount" to "No amount", "zero" to "Nothing moved",
    "corrected" to "Corrected by the bank", "repeatedHeader" to "The header row again", "inWholeBatch" to "In a batch imported as one entry",
    "imported" to "Imported before",
)
/** Lines left out for these reasons can be included anyway (`Reason::includable`). */
internal val includableReasons = setOf("pending", "declined", "otherCurrency", "ownBalances", "balance", "noDate", "noAmount")
/** The parts of a transaction a line gives, as a phone edits them. */
internal val lineFields = listOf("date" to "Date", "payee" to "Payee", "memo" to "Memo", "amount" to "Amount")

/** What the assistant is asked about the lines of an import that need attention: ready to send, not sent. */
internal const val fixImportText = "Fix the lines of this import that need attention. Use only what the file itself supports, and ask me about anything that isn't clear."

/** A line of a bank file as the import reads it now; `kind` is "ready", "attention" or "leftOut". */
internal data class StatementLine(
    val id: String, val version: Long, val cells: List<String>, val fields: Map<String, String>, val kind: String,
    /** The part that doesn't read, and why. */
    val field: String = "", val problem: String = "",
    /** Why it's left out, and what the file says of it (its state, currency, account). */
    val reason: String = "", val detail: String = "",
    val included: Boolean = false, val fixed: Boolean = false, val batch: Int? = null,
) {
    val includable get() = kind == "leftOut" && reason in includableReasons
    /** Its payee, else what the file writes on it. */
    val title get() = fields["payee"].orEmpty().ifBlank { cells.filter { it.isNotBlank() }.joinToString(" · ").ifBlank { "Line $id" } }
}

internal data class StatementSource(val label: String, val currency: String?, val lines: Int)
internal data class StatementBatch(val entry: String, val line: Int?, val payments: List<Int>, val addsUp: Boolean?, val asOne: Boolean)

/** A bank file as an import session reads it (bank_statement.rs `Statement`); a phone gets the lines that need it. */
internal data class BankStatement(
    val session: String, val revision: Long, val format: String, val lines: List<StatementLine>,
    val ready: Int, val attention: Int, val leftOut: Map<String, Int>,
    val sources: List<StatementSource>, val source: Int?,
    /** The date order, an ambiguous date, and whether it's still to confirm. */
    val dateOrder: String?, val dateExample: String?, val dateUnconfirmed: Boolean,
    /** Whether money out is told by the transaction types, when the file leaves that open; null when it doesn't. */
    val signsByType: Boolean?, val signsUnconfirmed: Boolean,
    /** The chosen account's currency, when it isn't the budget's. */
    val currency: String?,
    val batches: List<StatementBatch>,
    /** What the account keeps for its next file, once this one is imported. */
    val remember: JSONObject,
) {
    val leftOutCount get() = leftOut.values.sum()
    /** It opens at the lines by itself for: lines to fix, none that read, or a question to answer. */
    val needsChecking get() = attention > 0 || ready == 0 || dateUnconfirmed || signsUnconfirmed
    /** Nothing goes on to the review on a guess the file leaves open. */
    val canContinue get() = ready > 0 && !dateUnconfirmed && !signsUnconfirmed
    // Each list the phone shows, worked out once for this reading of the file.
    val attentionLines by lazy { lines.filter { it.kind == "attention" } }
    val leftOutLines by lazy { lines.filter { it.kind == "leftOut" } }
    val changedLines by lazy { lines.filter { it.kind == "ready" && (it.fixed || it.included) } }
    /** Where each line is among the file's, as a batch names its payments. */
    val places by lazy { lines.withIndex().associate { (i, line) -> line.id to i } }
    /** Every line is in a currency other than the budget's, which one; the file says nothing else. */
    val elsewhere get() = if (ready == 0 && currency == null) lines.firstOrNull { it.reason == "otherCurrency" }?.detail?.ifBlank { null } else null
}

private fun JSONObject.string(key: String) = if (isNull(key)) "" else optString(key)
private fun JSONArray?.objects(): List<JSONObject> = this?.let { a -> (0 until a.length()).mapNotNull(a::optJSONObject) }.orEmpty()

internal fun parseStatement(json: JSONObject): BankStatement {
    val lines = json.optJSONArray("lines").objects().map { line ->
        val status = line.optJSONObject("status") ?: JSONObject()
        val fields = line.optJSONObject("fields") ?: JSONObject()
        val cells = line.optJSONArray("cells")
        StatementLine(line.string("id"), line.optLong("version"), (0 until (cells?.length() ?: 0)).map { cells!!.optString(it) },
            lineFields.associate { (f, _) -> f to fields.string(f) }, status.string("kind").ifEmpty { "ready" },
            field = status.string("field"), problem = status.string("reason").takeIf { status.string("kind") == "attention" }.orEmpty(),
            reason = status.string("reason").takeIf { status.string("kind") == "leftOut" }.orEmpty(), detail = status.string("detail"),
            included = line.optBoolean("included"), fixed = line.optBoolean("fixed"),
            batch = if (line.has("batch") && !line.isNull("batch")) line.optInt("batch") else null)
    }
    val counts = json.optJSONObject("counts") ?: JSONObject()
    val out = counts.optJSONObject("leftOut")
    val date = json.optJSONObject("dateOrder")
    return BankStatement(
        json.optJSONObject("scope")?.string("session").orEmpty(), json.optLong("revision"), json.string("format"), lines,
        counts.optInt("ready"), counts.optInt("attention"), out?.keys()?.asSequence()?.associateWith { out.optInt(it) }.orEmpty(),
        json.optJSONArray("sources").objects().map { StatementSource(it.string("label"), it.string("currency").ifBlank { null }, it.optInt("lines")) },
        if (json.has("source") && !json.isNull("source")) json.optInt("source") else null,
        date?.string("order"), date?.string("example")?.ifBlank { null }, date?.optBoolean("unconfirmed") == true,
        json.optJSONObject("signs")?.optBoolean("byType"), json.optJSONObject("signs")?.optBoolean("unconfirmed") == true,
        json.string("currency").ifBlank { null },
        json.optJSONArray("batches").objects().map { b ->
            val payments = b.optJSONArray("payments")
            StatementBatch(b.string("entry"), if (b.has("line") && !b.isNull("line")) b.optInt("line") else null,
                (0 until (payments?.length() ?: 0)).map { payments!!.optInt(it) },
                if (b.has("addsUp") && !b.isNull("addsUp")) b.optBoolean("addsUp") else null, b.optBoolean("asOne"))
        },
        json.optJSONObject("remember") ?: JSONObject(),
    )
}

/** Why a line is left out, with what the file says of it. */
internal fun leftOutText(line: StatementLine): String {
    val name = leftOutNames[line.reason] ?: "Left out"
    return when {
        line.reason == "otherAccount" && line.detail.isNotBlank() -> "Account ${line.detail}"
        line.reason == "otherCurrency" && line.detail.isNotBlank() -> "In ${line.detail}"
        line.detail.isNotBlank() && line.reason in setOf("pending", "declined", "ownBalances") -> "$name (${line.detail})"
        else -> name
    }
}

/** Changes to an import session, made against its current revision (bank_statement.rs `Fixes`). */
internal fun bankFixes(session: JSONObject, build: JSONObject.() -> Unit = {}): JSONObject = JSONObject()
    .put("scope", session.getJSONObject("scope")).put("baseRevision", session.optLong("revision"))
    .put("baseStructure", session.getJSONObject("structure")).apply(build)

/** The session read another way: its structure with `change` made to a copy. */
internal fun restructured(session: JSONObject, change: JSONObject.() -> Unit): JSONObject =
    bankFixes(session) { put("structure", JSONObject(session.getJSONObject("structure").toString()).apply(change)) }

/** A line's parts given new values: those changed, each against the value it has now; null when none changed. */
internal fun fieldFixes(session: JSONObject, line: StatementLine, values: Map<String, String>): JSONObject? {
    val changed = values.mapValues { it.value.trim() }.filter { (field, value) -> value != line.fields[field].orEmpty() }
    if (changed.isEmpty()) return null
    return bankFixes(session) {
        put("fields", JSONArray(changed.map { (field, value) ->
            JSONObject().put("line", line.id).put("lineVersion", line.version).put("field", field).put("from", line.fields[field].orEmpty()).put("to", value)
        }))
    }
}

internal fun includeFix(session: JSONObject, line: StatementLine, include: Boolean) = bankFixes(session) {
    put("include", JSONArray().put(JSONObject().put("line", line.id).put("lineVersion", line.version).put("include", include)))
}

/** The line put back as the file has it. */
internal fun revertFix(session: JSONObject, line: StatementLine) = bankFixes(session) {
    put("revert", JSONArray().put(JSONObject().put("line", line.id).put("lineVersion", line.version)))
}

/** A batch's payments imported as its one entry, or (`on` false) each again. */
internal fun wholeBatchFix(session: JSONObject, entry: String, on: Boolean) = restructured(session) {
    val was = optJSONArray("wholeBatches")?.let { a -> (0 until a.length()).map(a::optString) }.orEmpty()
    put("wholeBatches", JSONArray(if (on) (was + entry).distinct() else was - entry))
}

/** A date the orders read differently, read day first and month first: "1 February 2026" and "2 January 2026". */
internal fun dateReadings(example: String): Pair<String, String>? {
    val (a, b, y) = Regex("(\\d{1,2})\\D(\\d{1,2})\\D(\\d{2,4})").find(example)?.destructured ?: return null
    val year = if (y.length == 2) 2000 + y.toInt() else y.toInt()
    val named = DateTimeFormatter.ofPattern("d MMMM yyyy", Locale.UK)
    fun read(day: Int, month: Int) = runCatching { LocalDate.of(year, month, day).format(named) }.getOrNull()
    return (read(a.toInt(), b.toInt()) ?: return null) to (read(b.toInt(), a.toInt()) ?: return null)
}

/** What became of fixes applied to an import (`bankApplyFixes`): the lines changed, how many applied, and what to say. */
internal data class FixOutcome(val lines: List<String>, val applied: Int, val notice: String)

internal fun fixOutcome(applied: JSONObject): FixOutcome {
    val results = applied.optJSONArray("results").objects()
    val refused = results.filterNot { it.optBoolean("applied") }
    val done = results.size - refused.size
    val notice = when {
        refused.isNotEmpty() -> "Applied $done of ${results.size} fixes. ${if (refused.size == 1) "One no longer fits" else "${refused.size} no longer fit"}: " +
            refused[0].string("problem").ifBlank { "the line changed" }
        done == 1 -> "Applied the assistant's fix."
        else -> "Applied the assistant's $done fixes."
    }
    return FixOutcome(results.filter { it.optBoolean("applied") && !it.isNull("line") }.map { it.string("line") }, done, notice)
}

/** "2 lines read · 1 needs attention and won't be imported" */
internal fun statementCounts(statement: BankStatement) = buildString {
    append(statement.ready).append(if (statement.ready == 1) " line reads" else " lines read")
    if (statement.attention > 0) append(" · ").append(statement.attention).append(if (statement.attention == 1) " needs" else " need").append(" attention and won't be imported")
    if (statement.leftOutCount > 0) append(" · ").append(statement.leftOutCount).append(" left out")
}

/** What money in may be: a refund of a purchase, with its category when the purchases agree on one. */
internal fun refundText(refund: JSONObject, categories: Map<String, String>, currency: String): String {
    val payee = refund.string("payee")
    val category = categories[refund.string("categoryId")]
    if (refund.optInt("purchases", 1) > 1 && refund.string("categoryId").isEmpty()) return "Possible refund · ${refund.optInt("purchases")} purchases from $payee match"
    return "Refund of $payee · ${Money.format(-refund.optLong("amount"), currency)} on ${Money.date(refund.string("date"))}${category?.let { " · $it" }.orEmpty()}"
}
