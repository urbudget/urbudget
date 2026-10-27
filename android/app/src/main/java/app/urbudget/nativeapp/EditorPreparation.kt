package app.urbudget.nativeapp

import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONArray
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.Charset
import java.nio.charset.CodingErrorAction
import java.util.UUID

/**
 * Small immutable view records built away from composition; source JSON stays read-only. A
 * `locked` row is already imported and has nothing to decide. "review" is a decision still to make:
 * the import waits until no row has it.
 */
internal data class BankReviewRow(
    val source: JSONObject, val options: List<Pair<String, String>>, val initial: String, val manualImportId: String, val previouslyMatched: Set<String>,
    val key: String = manualImportId, val locked: Boolean = false, val date: String = "", val payee: String = "", val bankText: String? = null,
    val memo: String = "", val amount: Long = 0, val amountText: String = "", val duplicate: Boolean = false, val possibleDuplicate: Boolean = false,
    val idless: Boolean = false, val detail: String = "",
    /** The transaction in another account it's offered as the other side of, when there is one. */
    val transfer: String? = null,
    /** The bank text backs that transfer up, so it's chosen; otherwise it's only suggested. */
    val transferSure: Boolean = false,
    /** Money in that may be a refund of a purchase: what it's of ("Refund of Shop · 23.99 on 7 Oct 2026 · Shopping"). */
    val refund: String? = null,
) {
    /** The row's decision, given the choices made so far. */
    fun decision(chosen: String?): String = if (locked) "skip" else chosen ?: initial
}

/** How many rows a review imports or matches, and how many still need a choice. */
internal fun bankReviewCounts(rows: List<BankReviewRow>, chosen: Map<Int, String>): Pair<Int, Int> {
    var importing = 0
    var undecided = 0
    rows.forEachIndexed { index, row ->
        when (row.decision(chosen[index])) { "skip" -> Unit; "review" -> undecided++; else -> importing++ }
    }
    return importing to undecided
}

/** The review's figures: rows already imported, rows a payee rule renamed, and what a note should say. */
internal data class BankReviewSummary(val rows: Int, val duplicates: Int, val renamed: Int, val idless: Boolean, val allIn: Boolean, val transfers: Boolean = false,
                                      val refunds: Int = 0)

internal fun summarizeBankReview(rows: List<BankReviewRow>) = BankReviewSummary(rows.size, rows.count { it.duplicate },
    rows.count { !it.duplicate && it.bankText != null }, rows.any { it.possibleDuplicate || it.idless && it.duplicate },
    rows.size >= 3 && rows.all { it.amount > 0 }, rows.any { it.transfer != null }, rows.count { it.refund != null })

/**
 * Desktop's defaultChoice: skip known bank IDs; take a single match only when that account side was not imported before,
 * and not a transfer the bank text doesn't back up (`suggestedTransfer`), which amount and date alone can't tell from a card payment.
 */
internal fun defaultBankChoice(duplicate: Boolean, possibleDuplicate: Boolean, candidates: List<String>, previouslyMatched: Set<String>,
                               suggestedTransfer: String? = null): String = when {
    duplicate -> "skip"
    possibleDuplicate -> "review"
    candidates.isEmpty() -> "new"
    candidates.size == 1 && candidates[0] !in previouslyMatched && candidates[0] != suggestedTransfer -> "match:${candidates[0]}"
    else -> "review"
}

/**
 * The format a statement is in: what its content says, as the engine reads it (an MT940 statement
 * saved as .txt), else what its extension says.
 */
internal fun statementFormat(name: String, text: String): String? {
    val head = text.trimStart('\uFEFF', ' ', '\t', '\r', '\n').take(4096).uppercase()
    val lines = head.lineSequence().map { it.trimStart() }.filter { it.isNotEmpty() }.toList()
    val extension = name.substringAfterLast('.', "").lowercase()
    return when {
        head.startsWith("OFXHEADER") || "<OFX>" in head -> if (extension == "qfx") "qfx" else "ofx"
        "CAMT.053" in head || "BKTOCSTMRSTMT>" in head -> "camt053"
        lines.any { it.startsWith(":20:") } && lines.any { it.startsWith(":61:") } -> "mt940"
        listOf("!TYPE:", "!ACCOUNT", "!OPTION:", "!CLEAR:").any { lines.firstOrNull()?.startsWith(it) == true } -> "qif"
        extension in listOf("csv", "ofx", "qfx", "qif") -> extension
        extension in listOf("xlsx", "xlsm", "xlsb", "xls", "ods", "zip") -> "csv"
        extension == "xml" -> "camt053"
        extension in listOf("sta", "940", "mt940") -> "mt940"
        else -> null
    }
}

/**
 * The engine's reading of a statement's bytes, given as ISO-8859-1: its text (a workbook's sheet,
 * the one named or else the one most like a statement, and its sheets), or why it can't be read.
 */
private fun engineDecoded(latin1: String, sheet: String? = null): JSONObject {
    val params = JSONObject().put("latin1", latin1).put("sheet", sheet ?: JSONObject.NULL)
    val request = JSONObject().put("version", 1).put("method", "decodeStatement").put("params", params)
    val reply = JSONObject(NativeBridge.invoke(request.toString()))
    if (!reply.optBoolean("ok")) error(if (reply.has("error")) reply.optString("error") else Words[R.string.bank_file_unreadable])
    return reply.getJSONObject("value")
}
private fun engineRead(latin1: String): String = engineDecoded(latin1).getString("text")

/** A statement's text, and a workbook's sheets with the one read. */
internal data class DecodedStatement(val text: String, val sheet: String? = null, val sheets: List<String> = emptyList())

/** A statement's text (see `decodeStatement`); a workbook is read at `sheet` when it has one by that name. */
internal fun decodeStatementFile(bytes: ByteArray, sheet: String? = null, workbook: (String, String?) -> JSONObject = ::engineDecoded): DecodedStatement {
    var book: JSONObject? = null
    val text = decodeStatement(bytes, binary = { latin1 -> workbook(latin1, sheet).also { book = it }.getString("text") })
    return book?.let { read ->
        val sheets = read.optJSONArray("sheets")
        DecodedStatement(text, if (read.isNull("sheet")) null else read.optString("sheet"), (0 until (sheets?.length() ?: 0)).map { sheets!!.optString(it) })
    } ?: DecodedStatement(text)
}

/** The engine's reading of a statement that isn't UTF-8; Windows-1252 if it can't say. */
private fun engineDecode(latin1: String): String = runCatching { engineRead(latin1) }
    .getOrElse { String(latin1.toByteArray(Charsets.ISO_8859_1), Charset.forName("windows-1252")) }

/**
 * A statement's text: UTF-16 or UTF-8 as its byte-order mark says, else UTF-8, else the encoding the
 * engine finds its bytes fit (Windows-1250 or -1252 in Europe, Shift_JIS, GBK, Windows-1251…), as
 * banks still write. The engine gets the bytes as ISO-8859-1, a character for each.
 */
internal fun decodeStatement(bytes: ByteArray, legacy: (String) -> String = ::engineDecode, binary: (String) -> String = ::engineRead): String {
    fun at(i: Int) = bytes.getOrNull(i)?.toInt()?.and(255)
    // An Excel or OpenDocument workbook, a ZIP of statements, a PDF: the engine reads a workbook's
    // sheet as CSV and a ZIP's statements, and says what a PDF needs.
    val signatures = listOf(listOf(0x50, 0x4B, 0x03, 0x04), listOf(0xD0, 0xCF, 0x11, 0xE0), listOf(0x25, 0x50, 0x44, 0x46))
    if (signatures.any { signature -> signature.indices.all { at(it) == signature[it] } }) return binary(String(bytes, Charsets.ISO_8859_1))
    if (at(0) == 0xFF && at(1) == 0xFE) return String(bytes, 2, bytes.size - 2, Charsets.UTF_16LE)
    if (at(0) == 0xFE && at(1) == 0xFF) return String(bytes, 2, bytes.size - 2, Charsets.UTF_16BE)
    // UTF-16 without a mark: plain text has a zero in every other byte.
    val head = minOf(bytes.size, 400)
    fun zeros(odd: Int) = (odd until head step 2).count { bytes[it].toInt() == 0 }
    if (head >= 4 && zeros(1) > head / 4) return String(bytes, Charsets.UTF_16LE)
    if (head >= 4 && zeros(0) > head / 4) return String(bytes, Charsets.UTF_16BE)
    val strict = Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT)
    return try {
        strict.decode(ByteBuffer.wrap(bytes)).toString().removePrefix(Char(0xFEFF).toString())
    } catch (_: CharacterCodingException) {
        legacy(String(bytes, Charsets.ISO_8859_1))
    }
}

private fun alreadyImportedOptions() = listOf("skip" to Words[R.string.bank_option_already_imported], "new" to Words[R.string.bank_option_add_separate])

private fun JSONObject.editorText(key: String) = if (isNull(key)) "" else optString(key)

/** Avoid a second boxed copy of every export byte while retaining the engine's u8 protocol. */
internal suspend fun encodeFileBytes(bytes: ByteArray): JSONArray {
    val context = currentCoroutineContext()
    context.ensureActive()
    return JSONArray().also { encoded ->
        bytes.forEachIndexed { index, byte ->
            if (index % 4096 == 0) context.ensureActive()
            encoded.put(byte.toInt() and 255)
        }
    }
}

/** One candidate pool's choices, built once and shared by every row that uses the pool. */
private class CandidateOptions(val matches: List<Pair<String, String>>, val candidates: List<String>, val previouslyMatched: Set<String>,
                               val transfer: String?, val transferSure: Boolean) {
    /** Choose, add, skip, then each match: the label of the undecided choice says why it's undecided. */
    val choose by lazy { listOf("review" to Words[R.string.bank_option_choose], "new" to Words[R.string.bank_option_add_new], "skip" to Words[R.string.bank_option_skip]) + matches }
    val possible by lazy { listOf("review" to Words[R.string.bank_option_possible], "new" to Words[R.string.bank_option_add_new], "skip" to Words[R.string.bank_option_skip]) + matches }
}

/** Candidates are a shared pool on Android; do not expand them once per bank row. */
internal suspend fun prepareBankReview(
    rows: List<JSONObject>, budget: JSONObject, allowIdlessDuplicate: Boolean, candidateSets: JSONArray? = null
): List<BankReviewRow> {
    val context = currentCoroutineContext()
    context.ensureActive()
    val currency = budget.editorText("currency").ifBlank { "SAR" }
    val transactions = budget.records("transactions").associateBy { it.editorText("id") }
    val accountNames = budget.records("accounts").associate { it.editorText("id") to it.editorText("name") }
    val categoryNames by lazy { budget.records("categories").associate { it.editorText("id") to it.editorText("name") } }
    val preparedSets = mutableMapOf<Int, CandidateOptions>()
    val alreadyImported by lazy { alreadyImportedOptions() }
    val noPayee = Words[R.string.payee_none]
    fun prepareOptions(source: JSONObject): CandidateOptions {
        val details = source.records("candidateDetails").associateBy { it.editorText("id") }
        val previous = source.optJSONArray("previouslyMatchedCandidates")
        val previouslyMatched = HashSet<String>()
        for (i in 0 until (previous?.length() ?: 0)) {
            if (i % 128 == 0) context.ensureActive()
            previouslyMatched.add(previous!!.getString(i))
        }
        val candidates = source.optJSONArray("candidates") ?: JSONArray()
        val transfers = source.optJSONArray("transferCandidates")?.let { list -> (0 until list.length()).map { list.optString(it) }.toSet() }.orEmpty()
        val ids = ArrayList<String>(candidates.length())
        val matches = ArrayList<Pair<String, String>>(candidates.length())
        for (i in 0 until candidates.length()) {
            if (i % 128 == 0) context.ensureActive()
            val id = candidates.optString(i)
            ids += id
            val detail = details[id]
            val saved = transactions[detail?.editorText("transactionId")?.takeIf { it.isNotBlank() } ?: id]
            val other = if (id in transfers) saved?.let { accountNames[it.editorText("accountId")] } else null
            matches += "match:$id" to matchLabel(id, saved, detail, id in previouslyMatched, other)
        }
        return CandidateOptions(matches, ids, previouslyMatched, transfers.firstOrNull(), source.optBoolean("transferSure"))
    }
    return rows.mapIndexed { index, row ->
        if (index % 128 == 0) context.ensureActive()
        val set = if (row.has("candidateSet")) {
            val setIndex = row.optInt("candidateSet", -1)
            preparedSets.getOrPut(setIndex) { prepareOptions(requireNotNull(candidateSets?.optJSONObject(setIndex)) { Words[R.string.bank_match_details_missing] }) }
        } else prepareOptions(row)
        val duplicate = row.optBoolean("duplicate")
        val possible = row.optBoolean("possibleDuplicate")
        val idless = row.optBoolean("idless")
        // An already-imported row can only be added again when it has no bank ID and that's allowed.
        val separate = duplicate && allowIdlessDuplicate && idless
        val options = when {
            separate -> alreadyImported
            duplicate -> emptyList()
            possible -> set.possible
            else -> set.choose
        }
        val manual = "bank:manual:${UUID.randomUUID()}"
        val rename = row.editorText("rename").takeIf { it.isNotBlank() && it != row.editorText("payee") }
        val initial = defaultBankChoice(duplicate, possible, set.candidates, set.previouslyMatched, set.transfer.takeUnless { set.transferSure })
        BankReviewRow(smallBankRow(row), options, initial, manual, set.previouslyMatched,
            key = "$index:${row.editorText("id")}", locked = duplicate && !separate, date = Money.date(row.editorText("date")),
            payee = (rename ?: row.editorText("payee")).ifBlank { noPayee }, bankText = rename?.let { row.editorText("payee") },
            memo = row.editorText("memo"), amount = row.optLong("amount"), amountText = Money.format(row.optLong("amount"), currency),
            duplicate = duplicate, possibleDuplicate = possible, idless = idless,
            detail = Money.joined(listOf(Money.date(row.editorText("date")), row.editorText("memo")).filter { it.isNotBlank() }),
            transfer = set.transfer.takeUnless { duplicate }, transferSure = set.transferSure,
            refund = row.optJSONObject("refundOf")?.takeUnless { duplicate }?.let { refundText(it, categoryNames, currency) })
    }
}

/**
 * "Match: 3 Oct 2026 · Shop · Split 2 · memo · Previously imported", as the desktop labels a match;
 * "Transfer from Savings: 2 Oct 2026 · TO EVERYDAY" for the other side of money moved from `other`.
 * The payee and memo keep their own direction inside a right-to-left label.
 */
private fun matchLabel(id: String, saved: JSONObject?, detail: JSONObject?, previously: Boolean, other: String? = null): String {
    val previous = if (previously) Words[R.string.bank_previously_imported] else null
    if (saved == null) return Words[R.string.bank_match, listOfNotNull(Money.own(id), previous).joinToString(" · ")]
    val part = detail?.let { saved.records("splits").getOrNull(it.optInt("splitIndex")) }
    val memo = part?.editorText("memo")?.ifBlank { null } ?: saved.editorText("memo")
    val pieces = listOfNotNull(
        Money.date(saved.editorText("date")),
        saved.editorText("payee").ifBlank { null }?.let(Money::own) ?: Words[R.string.transfer_label],
        detail?.let { Words[R.string.bank_match_split, (it.optInt("splitIndex") + 1).toString()] },
        memo.ifBlank { null }?.let(Money::own),
        previous,
    ).joinToString(" · ")
    return if (other == null) Words[R.string.bank_match, pieces]
    else Words[if (saved.optLong("amount") < 0) R.string.bank_transfer_from_match else R.string.bank_transfer_to_match, other, pieces]
}

/** Matching revalidates against the current ledger; only the selected prior match is needed. */
private fun smallBankRow(source: JSONObject): JSONObject = JSONObject().also { row ->
    source.keys().forEach { key ->
        if (key !in setOf("candidateSet", "candidates", "candidateDetails", "previouslyMatchedCandidates", "transferCandidates", "transferSure")) row.put(key, source.get(key))
    }
}

internal suspend fun bankReviewChoices(rows: List<BankReviewRow>, overrides: Map<Int, String>, allowIdlessDuplicate: Boolean): JSONArray {
    val context = currentCoroutineContext()
    context.ensureActive()
    val choices = JSONArray()
    rows.forEachIndexed { index, prepared ->
        if (index % 128 == 0) context.ensureActive()
        // The review screens wait until no row is undecided; any other caller never imports or
        // matches a row nobody decided on.
        val decision = prepared.decision(overrides[index]).let { if (it == "review") "skip" else it }
        val source = prepared.source
        val row = smallBankRow(source)
        if (allowIdlessDuplicate && decision == "new" && source.optBoolean("idless") && (source.optBoolean("duplicate") || source.optBoolean("possibleDuplicate"))) row.put("importId", prepared.manualImportId)
        val matchId = if (decision.startsWith("match:")) decision.removePrefix("match:") else null
        if (matchId != null && matchId in prepared.previouslyMatched) row.put("previouslyMatchedCandidates", JSONArray().put(matchId))
        choices.put(JSONObject().put("row", row).put("action", if (matchId != null) "match" else decision)
            .put("matchId", matchId ?: JSONObject.NULL))
    }
    return choices
}

internal suspend fun preparePayees(budget: JSONObject): List<JSONObject> {
    val context = currentCoroutineContext()
    context.ensureActive()
    val saved = budget.records("payees").associateBy { it.editorText("name") }.toMutableMap()
    val names = linkedSetOf<String>()
    fun add(value: JSONObject, key: String) { if (!value.isNull(key)) value.optString(key).takeIf { it.isNotBlank() }?.let(names::add) }
    var visited = 0
    fun transaction(record: JSONObject) {
        if (++visited % 128 == 0) context.ensureActive()
        add(record, "payee")
        record.records("splits").forEach { add(it, "payee") }
    }
    val transactions = budget.optJSONArray("transactions")
    for (i in 0 until (transactions?.length() ?: 0)) transactions?.optJSONObject(i)?.let(::transaction)
    budget.records("schedules").forEach { it.optJSONObject("transaction")?.let(::transaction) }
    budget.records("payeeRules").forEach { add(it, "payee") }
    names.forEach { name -> if (name !in saved) saved[name] = JSONObject().put("id", UUID.nameUUIDFromBytes(("payee:" + name).toByteArray(Charsets.UTF_8)).toString()).put("name", name) }
    context.ensureActive()
    return saved.values.sortedBy { it.editorText("name") }
}
