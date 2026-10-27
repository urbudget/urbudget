package app.urbudget.nativeapp.ui

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.urbudget.nativeapp.BankReviewRow
import app.urbudget.nativeapp.BankReviewSummary
import app.urbudget.nativeapp.BankStatement
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.DecodedStatement
import app.urbudget.nativeapp.StatementLine
import app.urbudget.nativeapp.StatementSource
import app.urbudget.nativeapp.bankReviewChoices
import app.urbudget.nativeapp.bankReviewCounts
import app.urbudget.nativeapp.dateReadings
import app.urbudget.nativeapp.decodeStatementFile
import app.urbudget.nativeapp.fieldFixes
import app.urbudget.nativeapp.includeFix
import app.urbudget.nativeapp.leftOutText
import app.urbudget.nativeapp.lineFields
import app.urbudget.nativeapp.parseStatement
import app.urbudget.nativeapp.prepareBankReview
import app.urbudget.nativeapp.restructured
import app.urbudget.nativeapp.revertFix
import app.urbudget.nativeapp.statementCounts
import app.urbudget.nativeapp.summarizeBankReview
import app.urbudget.nativeapp.wholeBatchFix
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.util.UUID
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * A bank file import in progress, kept by whoever opened it so it outlasts the sheet: the
 * assistant can open over it and fix its lines, and the import is there again after.
 */
internal data class BankImportDraft(
    val account: String,
    val fileName: String = "",
    /** The file's text: a workbook's sheet, as CSV. */
    val content: String? = null,
    /** A workbook's sheets, and the one read. */
    val sheets: List<String> = emptyList(),
    val sheet: String? = null,
    /** The file chosen, to read another of its sheets. */
    val file: Uri? = null,
    /** The file is a PDF, which the assistant reads. */
    val pdf: Boolean = false,
    /** The import (bank_statement.rs `Session`), passed back as it came. */
    val session: JSONObject? = null,
    /** "lines" to check the file's lines, or "review"; null until the file has read. */
    val view: String? = null,
    /** Lines the assistant changed. */
    val assisted: Set<String> = emptySet(),
    /** The assistant prepared the file: its thread and proposal, marked applied once imported. */
    val prepared: Pair<String, String>? = null,
    /** What became of the assistant's fixes, until the next change. */
    val notice: String? = null,
) {
    val sessionId get() = session?.optJSONObject("scope")?.optString("session")
}

/** A new import into `account`, else the first open account. */
internal fun newBankImport(budget: JSONObject, account: String? = null) =
    BankImportDraft(account ?: budget.rows("accounts").firstOrNull { !it.optBoolean("archived") }?.text("id").orEmpty())

/** A file the assistant converted (`assistant_import`), for its review: written the standard way. */
internal fun preparedBankImport(file: JSONObject, thread: String, proposal: String) =
    BankImportDraft(file.text("account"), file.text("name"), file.text("csv"), prepared = thread to proposal)

/** The engine's answer, or why there's none; cancelled with whoever waits for it. */
private suspend fun BudgetViewModel.ask(method: String, params: JSONObject, anyRevision: Boolean = false): JSONObject = suspendCancellableCoroutine { waiting ->
    val job = query(method, params, onError = { if (waiting.isActive) waiting.resumeWithException(IllegalStateException(it)) }, anyRevision = anyRevision) {
        if (waiting.isActive) waiting.resume(it)
    }
    waiting.invokeOnCancellation { job.cancel() }
}

/** A chosen file's name, and its text unless it's a PDF; a workbook at `sheet` when it has one by that name. */
private suspend fun openStatement(context: Context, uri: Uri, sheet: String?): Pair<String, DecodedStatement?> = withContext(Dispatchers.IO) {
    val name = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
        if (it.moveToFirst()) it.getString(0) else null
    } ?: "Bank statement"
    val bytes = context.contentResolver.openInputStream(uri)?.use { input ->
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (true) {
            val size = input.read(buffer)
            if (size < 0) break
            require(output.size() + size <= 8 * 1024 * 1024) { "Choose a statement smaller than 8 MiB." }
            output.write(buffer, 0, size)
        }
        output.toByteArray()
    } ?: error("The selected statement could not be opened.")
    // A PDF is the assistant's to read: no text.
    val pdf = name.endsWith(".pdf", ignoreCase = true) || bytes.take(4) == "%PDF".toByteArray().toList()
    name to (if (pdf) null else decodeStatementFile(bytes, sheet))
}

/**
 * Imports a file a bank exported (the desktop's Import page): choose the file, fix or include the
 * lines that need it (this opens by itself when something does), then decide what each line that
 * reads does (add it, match an existing transaction, or skip it), then one atomic budget change.
 */
@Composable
internal fun BankImportDialog(budget: JSONObject, vm: BudgetViewModel, draft: BankImportDraft, update: ((BankImportDraft) -> BankImportDraft) -> Unit,
                              onImported: () -> Unit = {}, askAssistant: ((Uri) -> Unit)? = null, fixWithAssistant: (() -> Unit)? = null, close: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val budgetState by vm.state.collectAsStateWithLifecycle()
    var statement by remember { mutableStateOf<BankStatement?>(null) }
    var rows by remember { mutableStateOf<List<BankReviewRow>?>(null) }
    var summary by remember { mutableStateOf<BankReviewSummary?>(null) }
    // The statement as the review read it, and the budget it was checked against.
    var reviewed by remember { mutableStateOf<BankStatement?>(null) }
    var previewBudget by remember { mutableStateOf<JSONObject?>(null) }
    val decisions = remember { mutableStateMapOf<Int, String>() }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    var fixing by remember { mutableStateOf<StatementLine?>(null) }
    val accountChoices = remember(budget) { budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") } }
    val accountName = accountChoices.firstOrNull { it.first == draft.account }?.second.orEmpty()
    val session = draft.session
    val content = draft.content
    val counts by remember { derivedStateOf { bankReviewCounts(rows.orEmpty(), decisions) } }
    val (importing, undecided) = counts
    val st = statement?.takeIf { it.session == draft.sessionId }
    // Until the file reads again after a change, a line's version is the one before it.
    val busy = loading || saving || session != null && (st == null || st.revision != session.optLong("revision"))
    // Most changes take a moment: the sheet only looks busy (greyed, with a progress line) for one
    // that lasts, so a quick one doesn't flash it.
    val slow by produceState(false, busy) {
        value = false
        if (busy) {
            delay(300)
            value = true
        }
    }
    fun read(more: JSONObject.() -> Unit = {}) = JSONObject().put("accountId", draft.account).put("content", content).put("session", session).apply(more)

    LaunchedEffect(budgetState.error) {
        if (saving && budgetState.error != null) { error = budgetState.error; loading = false; saving = false }
    }

    // A file chosen, or the assistant's: an import of it, read as the account's last file was
    // (the assistant's the standard way).
    LaunchedEffect(content, session == null) {
        if (content == null || session != null) return@LaunchedEffect
        loading = true
        error = null
        try {
            val fresh = vm.ask("bankSession", JSONObject().put("accountId", draft.account).put("content", content)
                .put("sessionId", UUID.randomUUID().toString()).put("budgetId", budgetState.budgetIdentity.orEmpty())
                .put("sheet", draft.sheet ?: JSONObject.NULL).put("prepared", draft.prepared != null))
            update { if (it.content === content && it.session == null) it.copy(session = fresh) else it }
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
        loading = false
    }

    // The lines that need attention (or were changed), read again after each change; in the
    // review, its own reading of the file is the statement.
    LaunchedEffect(session, content, budget) {
        if (session == null || content == null || draft.view == "review") return@LaunchedEffect
        try {
            val reply = vm.ask("bankStatement", read { put("problemsOnly", true) })
            val lines = withContext(Dispatchers.Default) { parseStatement(reply) }
            statement = lines
            update { if (it.session === session && it.view == null) it.copy(view = if (lines.needsChecking) "lines" else "review") else it }
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
    }

    // The review, and again whenever the budget or the import changes.
    LaunchedEffect(draft.view, session, content, budget) {
        if (draft.view != "review" || session == null || content == null) return@LaunchedEffect
        // A review of the import before this change is never the one imported.
        rows = null; summary = null
        try {
            val reply = vm.ask("bankImportPreview", read { put("problemsOnly", true) })
            val review = reply.getJSONObject("review")
            val prepared = withContext(Dispatchers.Default) {
                val list = prepareBankReview(review.rows("items"), budget, true, review.optJSONArray("candidateSets"))
                Triple(list, summarizeBankReview(list), parseStatement(reply.getJSONObject("statement")))
            }
            rows = prepared.first; summary = prepared.second; reviewed = prepared.third; statement = prepared.third; previewBudget = budget; decisions.clear()
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
    }

    fun open(uri: Uri, sheet: String?) {
        loading = true
        error = null
        scope.launch {
            try {
                val (name, text) = openStatement(context, uri, sheet)
                statement = null; rows = null; summary = null
                update { BankImportDraft(it.account, name, text?.text, text?.sheets.orEmpty(), text?.sheet, uri, pdf = text == null) }
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
            loading = false
        }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        // A workbook opens at the sheet the account's last one was read from.
        val kept = budget.rows("accounts").firstOrNull { it.text("id") == draft.account }?.optJSONObject("fileImport")
        if (uri != null) open(uri, kept?.text("sheet")?.ifBlank { null })
    }

    fun fix(fixes: JSONObject) {
        // While a change applies, the lines' versions are the ones before it.
        if (busy) return
        val was = session ?: return
        loading = true
        error = null
        update { it.copy(notice = null) }
        scope.launch {
            try {
                // A change to the import still fits after an edit to the budget meanwhile.
                val applied = vm.ask("bankApplyFixes", read { put("fixes", fixes) }, anyRevision = true)
                val next = applied.getJSONObject("session")
                update { if (it.session === was) it.copy(session = next) else it }
                val results = applied.optJSONArray("results")
                (0 until (results?.length() ?: 0)).map { results!!.getJSONObject(it) }.firstOrNull { !it.optBoolean("applied") }
                    ?.let { error = it.text("problem").ifBlank { "That change no longer fits the file." } }
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
            loading = false
        }
    }

    fun importRows() {
        if (previewBudget !== vm.state.value.budget || reviewed?.revision != session?.optLong("revision")) {
            error = "Your budget changed. The review is up to date again: check it and import."
            return
        }
        loading = true
        saving = true
        val imported = rows.orEmpty(); val selected = decisions.toMap(); val accountId = draft.account; val source = previewBudget
        // The account keeps how its own file read for the next one, not how the assistant's did.
        val remember = reviewed?.remember?.takeIf { draft.prepared == null }
        scope.launch {
            try {
                val choices = withContext(Dispatchers.Default) { bankReviewChoices(imported, selected, true) }
                require(source === vm.state.value.budget) { "Your budget changed. The review is up to date again: check it and import." }
                vm.runCommand("bankApply", JSONObject().put("accountId", accountId).put("choices", choices).put("remember", remember ?: JSONObject.NULL)) { onImported(); close() }
            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message; loading = false; saving = false }
        }
    }

    val reviewing = rows
    val view = if (st == null) null else draft.view ?: "lines"
    LazyFormDialog("Import bank file", close,
        confirm = if (view == "review") (if (saving) "Importing…" else "Import $importing") else "Continue",
        onConfirm = when (view) { null -> null; "review" -> ({ if (!busy) importRows() }); else -> ({ if (!busy) update { it.copy(view = "review") } }) },
        enabled = !slow && !saving && when (view) {
            "review" -> reviewing != null && undecided == 0 && importing > 0
            // Nothing goes on to the review on a guess the file leaves open.
            else -> st != null && st.canContinue
        }) {
        // Always there, shown only for a change that lasts: it never pushes the sheet's content down.
        item(key = "import:loading") { LinearProgressIndicator(Modifier.fillMaxWidth().height(2.dp).alpha(if (slow || saving) 1f else 0f), color = ur.accent) }
        error?.let { item(key = "import:error") { FormError(it) } }
        draft.notice?.let { item(key = "import:notice") { Notice(it, tone = "info", icon = Icons.Outlined.AutoAwesome) } }
        when {
            session == null -> chooser(draft, accountChoices, loading, askAssistant, { account -> update { newBankImport(budget, account) } }) { picker.launch(arrayOf("*/*")) }
            st == null -> item(key = "import:reading") { Text("Reading ${draft.fileName.ifEmpty { "the file" }}…", style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
            view == "review" -> {
                item(key = "import:summary") {
                    ImportHeading("${draft.fileName.ifEmpty { "Statement" }} into $accountName", summary?.let(::reviewCounts),
                        if (draft.prepared == null) "Change file" else null, !saving) { update { newBankImport(budget, it.account) } }
                }
                val out = st.leftOutCount
                if (st.attention > 0 || out > 0) item(key = "import:check") {
                    Notice(listOfNotNull(st.attention.takeIf { it > 0 }?.let { "$it need attention" }, out.takeIf { it > 0 }?.let { "$it left out" }).joinToString(" · ") +
                        ", not imported.", tone = "info", action = "Check the file", onAction = { update { it.copy(view = "lines") } }, icon = Icons.Outlined.TableRows)
                }
                if (reviewing == null) item(key = "import:checking") { Text("Checking the lines against your budget…", style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
                else {
                    val all = summary
                    if (all?.allIn == true && st.format == "csv" && session.optJSONObject("structure")?.optBoolean("flipSigns") != true) item(key = "import:all-in") {
                        Notice("Every row came out as money in. If these are purchases, the file writes them as positive numbers.", action = "Reverse signs",
                            onAction = { fix(restructured(session) { put("flipSigns", true) }) }, icon = Icons.Outlined.SwapVert)
                    }
                    all?.let { bankReviewNotes(it, undecided) }
                    suggestedTransfers(reviewing, decisions, !slow && !saving)
                    bankReviewRows(reviewing, decisions, !slow && !saving) { index, value -> decisions[index] = value }
                }
            }
            else -> statementLines(st, session, draft, accountName, busy, slow, saving, fixWithAssistant,
                restart = { update { newBankImport(budget, it.account) } },
                sheet = { sheet -> draft.file?.let { open(it, sheet) } },
                edit = { fixing = it }, fix = ::fix)
        }
    }

    fixing?.let { line ->
        LineFixSheet(line, { fixing = null }) { values ->
            fixing = null
            session?.let { s -> fieldFixes(s, line, values)?.let(::fix) }
        }
    }
}

/** Choosing the account and the file. */
private fun LazyListScope.chooser(draft: BankImportDraft, accounts: List<Pair<String, String>>, loading: Boolean, askAssistant: ((Uri) -> Unit)?,
                                  account: (String) -> Unit, choose: () -> Unit) {
    item(key = "import:lead") {
        Text("Bring in the transactions your bank exported. You see the lines that don't read and fix them, then check every transaction before anything is saved.",
            style = MaterialTheme.typography.bodyMedium, color = ur.text2)
    }
    item(key = "import:account") { Choice("Into account", draft.account, accounts) { if (!loading) account(it) } }
    item(key = "import:file") {
        ListCard {
            ListRow(draft.fileName.ifEmpty { "Choose a file" }, subtitle = if (draft.pdf) "PDF · tap to choose another" else if (draft.content == null) "CSV, Excel, OFX, QIF, CAMT.053 or MT940; PDF through the assistant" else "Reading…",
                icon = if (draft.fileName.isEmpty()) Icons.Outlined.FileOpen else Icons.Outlined.Description, onClick = { if (!loading) choose() })
        }
    }
    val file = draft.file
    if (draft.pdf && file != null) item(key = "import:pdf") {
        if (askAssistant != null) Notice("PDF statements need the assistant. It reads the PDF, finds the transactions, checks them against the statement's totals and brings them to a review like this one, where you decide on each row.", tone = "info",
            action = "Read it with the assistant", onAction = { askAssistant(file) }, icon = Icons.Outlined.AutoAwesome)
        else Notice("PDF statements need the assistant. Open the assistant, attach this PDF and ask it to import the statement.", tone = "info")
    }
    item(key = "import:help") {
        Text("Lines that don't read, or that the import leaves out (pending payments, another account's, balances), are shown for you to fix or include. The account remembers how its file read; on the desktop you can also choose which column holds what.",
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
}

/** The file's lines that need something, as a phone checks them: the questions the file leaves open, then each line with what can be done. */
private fun LazyListScope.statementLines(st: BankStatement, session: JSONObject, draft: BankImportDraft, accountName: String, busy: Boolean, slow: Boolean, saving: Boolean,
                                         fixWithAssistant: (() -> Unit)?, restart: () -> Unit, sheet: (String) -> Unit,
                                         edit: (StatementLine) -> Unit, fix: (JSONObject) -> Unit) {
    val structure = session.optJSONObject("structure") ?: JSONObject()
    item(key = "lines:summary") {
        ImportHeading("${draft.fileName.ifEmpty { "Statement" }} into $accountName", statementCounts(st), if (draft.prepared == null) "Change file" else null, !saving, restart)
    }
    if (draft.sheets.size > 1) item(key = "lines:sheet") { Choice("Sheet", draft.sheet ?: draft.sheets.first(), draft.sheets.map { it to it }) { if (!busy && it != draft.sheet) sheet(it) } }
    if (st.sources.size > 1) item(key = "lines:source") {
        Choice("Account in the file", st.sources.getOrNull(st.source ?: 0)?.label.orEmpty(), st.sources.map { it.label to sourceText(it) }) { label ->
            if (!busy) fix(restructured(session) { put("source", label) })
        }
    }
    if (st.format == "csv") item(key = "lines:signs") {
        // Some card issuers write a purchase as a positive number and a payment as a negative one.
        Choice("Purchases in the file", if (structure.optBoolean("flipSigns")) "positive" else "negative",
            listOf("negative" to "Negative, as bank accounts write them", "positive" to "Positive, as many card statements write them")) { which ->
            if (!busy && (which == "positive") != structure.optBoolean("flipSigns")) fix(restructured(session) { put("flipSigns", which == "positive") })
        }
    }
    st.elsewhere?.let { other -> item(key = "lines:elsewhere") {
        Notice("The file's amounts are in $other. Include a line to enter its amount in your budget's currency, or import the file into a budget in $other.", icon = Icons.Outlined.WarningAmber)
    } }
    st.currency?.let { other -> item(key = "lines:currency") {
        Notice("This account's amounts are in $other. Choose another account in the file, or include its lines with their amounts in your budget's currency.", icon = Icons.Outlined.WarningAmber)
    } }
    val example = st.dateExample
    // Answered once, the date order and the signs stay one choice away.
    if (!st.dateUnconfirmed && example != null) item(key = "lines:dates-chosen") {
        val named = dateReadings(example)
        Choice("Dates", st.dateOrder ?: "dmy", listOf("dmy" to "Day first${named?.let { " ($example is ${it.first})" }.orEmpty()}",
            "mdy" to "Month first${named?.let { " ($example is ${it.second})" }.orEmpty()}")) { which ->
            if (!busy && which != st.dateOrder) fix(restructured(session) { put("dateOrder", JSONObject().put("order", which).put("confirmed", true)) })
        }
    }
    if (!st.signsUnconfirmed && st.signsByType != null) item(key = "lines:signs-chosen") {
        Choice("Money out", if (st.signsByType) "type" else "written", listOf("type" to "Told by the transaction types", "written" to "As the amounts are written")) { which ->
            if (!busy && (which == "type") != st.signsByType) fix(restructured(session) { put("signsByType", which == "type") })
        }
    }
    if (st.dateUnconfirmed && example != null) item(key = "lines:dates") {
        val named = dateReadings(example)
        val order = { which: String -> fix(restructured(session) { put("dateOrder", JSONObject().put("order", which).put("confirmed", true)) }) }
        ImportQuestion("The dates don't say whether the day or the month comes first: is $example", listOf(
            (named?.first ?: "Day first") to { order("dmy") }, (named?.second ?: "Month first") to { order("mdy") }), if (st.dateOrder == "mdy") 1 else 0, !slow)
    }
    if (st.signsUnconfirmed) item(key = "lines:types") {
        ImportQuestion("Every amount that reads is positive. Is money out told by the transaction types (DEBIT, POS…)?", listOf(
            "Yes, use the types" to { fix(restructured(session) { put("signsByType", true) }) },
            "No, as written" to { fix(restructured(session) { put("signsByType", false) }) }), 0, !slow)
    }
    val attention = st.attentionLines
    val out = st.leftOutLines
    val changed = st.changedLines
    val place = st.places
    // A batch's payments that don't add up to its entry: imported as the entry instead, or each again.
    fun batchAction(line: StatementLine): Pair<String, () -> Unit>? {
        val batch = line.batch?.let { st.batches.getOrNull(it) } ?: return null
        return when {
            batch.addsUp == false && !batch.asOne && batch.payments.firstOrNull() == place[line.id] -> "Import as one entry" to { fix(wholeBatchFix(session, batch.entry, true)) }
            batch.asOne && batch.line == place[line.id] -> "Import its payments" to { fix(wholeBatchFix(session, batch.entry, false)) }
            else -> null
        }
    }
    fun undo(line: StatementLine) = if (line.fixed || line.included) (if (line.id in draft.assisted) "Undo the assistant's fix" else "Undo") to { fix(revertFix(session, line)) } else null
    if (attention.isNotEmpty()) {
        item(key = "lines:attention") { SectionTitle("Needs attention", "Not imported until fixed: tap a line to give what doesn't read.") }
        if (fixWithAssistant != null) item(key = "lines:assistant") {
            Notice("The assistant can fix these lines from what the file says, and asks you about anything unclear. You see each fix before it's applied.", tone = "info",
                action = "Fix with the assistant", onAction = { fixWithAssistant() }, icon = Icons.Outlined.AutoAwesome)
        }
        items(attention, key = { "attention:${it.id}" }, contentType = { "statement-line" }) { line ->
            StatementLineItem(line, line.problem, ur.warning, listOfNotNull(batchAction(line), undo(line)), !slow) { edit(line) }
        }
    }
    if (out.isNotEmpty()) {
        item(key = "lines:out") { SectionTitle("Left out", "Not imported. Include a line if it belongs in the account.") }
        items(out, key = { "out:${it.id}" }, contentType = { "statement-line" }) { line ->
            val include = if (line.includable && !line.included) "Include" to { fix(includeFix(session, line, true)) } else null
            StatementLineItem(line, leftOutText(line), ur.muted, listOfNotNull(include, batchAction(line), undo(line)), !slow)
        }
    }
    if (changed.isNotEmpty()) {
        item(key = "lines:changed") { SectionTitle("Changed", "Imported as you changed them.") }
        items(changed, key = { "changed:${it.id}" }, contentType = { "statement-line" }) { line ->
            StatementLineItem(line, if (line.id in draft.assisted) "Fixed by the assistant" else if (line.included) "Included" else "Changed from the file", ur.accent,
                listOfNotNull(undo(line)), !slow) { edit(line) }
        }
    }
}

/** "Account 1234 · EUR · 12 lines" */
private fun sourceText(source: StatementSource) =
    "${source.label}${source.currency?.let { " · $it" }.orEmpty()} · ${source.lines} ${if (source.lines == 1) "line" else "lines"}"

/** Where the file goes and how it reads, with a way to start again. */
@Composable
private fun ImportHeading(title: String, detail: String?, action: String?, enabled: Boolean, onAction: () -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall, color = ur.text)
            detail?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted) }
        }
        if (action != null) TextButton(onAction, enabled = enabled) { Text(action, color = ur.accent, fontWeight = FontWeight.SemiBold) }
    }
}

/** Something the file doesn't say, asked once: the answer chosen for now first. */
@Composable
private fun ImportQuestion(text: String, answers: List<Pair<String, () -> Unit>>, chosen: Int, enabled: Boolean) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(ur.warningSoft).padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.Top) {
            Icon(Icons.Outlined.WarningAmber, null, Modifier.size(20.dp), tint = ur.warning)
            Spacer(Modifier.width(10.dp))
            Text(text, style = MaterialTheme.typography.bodyMedium, color = ur.text)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            answers.forEachIndexed { index, (label, answer) ->
                if (index == chosen) Button(answer, Modifier.weight(1f), enabled = enabled, colors = ButtonDefaults.buttonColors(containerColor = ur.accent)) { Text(label) }
                else OutlinedButton(answer, Modifier.weight(1f), enabled = enabled) { Text(label, color = ur.accent) }
            }
        }
    }
}

/** A line of the file, a card of its own: what it says, what became of it, and what can be done. */
@Composable
private fun StatementLineItem(line: StatementLine, state: String, tone: androidx.compose.ui.graphics.Color, actions: List<Pair<String, () -> Unit>>, enabled: Boolean,
                              onClick: (() -> Unit)? = null) {
    Column(Modifier.cardSegment(0, 1, cardFill())) {
        ListRow(line.title, subtitle = listOf(line.fields["date"].orEmpty(), line.fields["memo"].orEmpty()).filter { it.isNotBlank() }.joinToString(" · ").ifBlank { null },
            value = line.fields["amount"]?.ifBlank { null }, onClick = onClick?.takeIf { enabled }, chevron = onClick != null)
        Column(Modifier.padding(start = 16.dp, end = 8.dp, bottom = 8.dp)) {
            Text(state, style = MaterialTheme.typography.bodySmall, color = tone, fontWeight = FontWeight.SemiBold, maxLines = 3, overflow = TextOverflow.Ellipsis)
            if (actions.isNotEmpty()) Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                actions.forEach { (label, act) -> TextButton(act, enabled = enabled) { Text(label, color = ur.accent, fontWeight = FontWeight.SemiBold) } }
            }
        }
    }
}

/** A line's date, payee, memo and amount, to give what doesn't read; the file's own cells shown for reference. */
@Composable
private fun LineFixSheet(line: StatementLine, close: () -> Unit, save: (Map<String, String>) -> Unit) {
    val values = remember(line.id, line.version) { mutableStateMapOf<String, String>().apply { lineFields.forEach { (field, _) -> put(field, line.fields[field].orEmpty()) } } }
    FormDialog("Fix line", close, confirm = "Done", onConfirm = { save(values.toMap()) }) {
        Text("In the file: ${line.cells.filter { it.isNotBlank() }.joinToString(" · ").ifBlank { "nothing" }}", style = MaterialTheme.typography.bodySmall, color = ur.muted)
        if (line.problem.isNotEmpty()) Notice(line.problem, icon = Icons.Outlined.WarningAmber)
        lineFields.forEach { (field, label) ->
            // The amount takes a minus sign, which a decimal keypad may not have.
            Field(values[field].orEmpty(), { values[field] = it }, when (field) {
                "date" -> "$label (YYYY-MM-DD)"
                "amount" -> "$label (negative for money out)"
                else -> label
            })
        }
    }
}
