package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

@Composable
internal fun SmsAnalysisReview(budget: JSONObject, vm: BudgetViewModel, batch: SmsBatch, close: () -> Unit, change: (SmsBatch?) -> Unit, refine: (String) -> Unit, finished: (List<String>) -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var prepared by remember { mutableStateOf<Map<Int, BankReviewRow>?>(null) }
    var checked by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    var instruction by remember { mutableStateOf("") }
    var editing by remember { mutableStateOf<Int?>(null) }
    val decisions = remember { mutableStateMapOf<Int, String>() }
    val answers = remember(batch.analysis) { mutableStateMapOf<Int, String>() }
    val accountAnswers = remember(batch.analysis) { mutableStateMapOf<Int, String>() }
    val remembered = remember(batch.analysis) { mutableStateMapOf<Int, Boolean>() }
    val items = batch.analysis.rows("items")
    val accounts = budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") }
    val allDecided = prepared?.all { (i, row) -> row.decision(decisions[i]) != "review" } == true
    val needsRefinement = instruction.isNotBlank() || answers.values.any { it.isNotBlank() } || accountAnswers.isNotEmpty()
    val handled = items.filter { it.text("disposition") in setOf("transaction", "ignore") }.flatMap { it.getJSONArray("sourceIds").let { a -> (0 until a.length()).map { a.getString(it) } } }
    LaunchedEffect(budget, batch.analysis) {
        prepared = null; checked = null; decisions.clear(); error = null
        try {
            val reply = vm.smsQuery("smsBatchPreview", batch.params())
            val raw = reply.rows("items")
            val rows = withContext(Dispatchers.Default) { prepareBankReview(raw.map { it.getJSONObject("row") }, budget, false) }
            prepared = raw.mapIndexed { i, item -> item.getInt("index") to rows[i] }.toMap(); checked = budget
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
    }
    LaunchedEffect(state.error) { if (state.error != null) { error = state.error; saving = false } }
    LazyFormDialog(stringResource(R.string.sms_review_title), { if (!saving) close() }, confirm = stringResource(R.string.sms_confirm_review),
        enabled = !saving && !needsRefinement && allDecided && checked === state.budget && handled.isNotEmpty(), onConfirm = {
            if (checked === vm.state.value.budget && allDecided) {
                saving = true
                val choices = JSONArray(prepared.orEmpty().map { (i, row) ->
                    val decision = row.decision(decisions[i])
                    JSONObject().put("index", i).put("action", if (decision.startsWith("match:")) "match" else decision)
                        .put("matchId", if (decision.startsWith("match:")) decision.removePrefix("match:") else JSONObject.NULL)
                })
                vm.runCommand("smsBatchApply", batch.params().put("decisions", choices)) { finished(handled) }
            }
        }) {
        item { Text(stringResource(R.string.sms_analysis_counts, items.count { it.text("disposition") == "transaction" }.toString(), items.count { it.text("disposition") == "question" }.toString(), items.count { it.text("disposition") in setOf("ignore", "pending") }.toString()), color = ur.text2) }
        item { Text(stringResource(R.string.sms_import_note), color = ur.muted) }
        if (prepared == null || saving) item { LinearProgressIndicator(Modifier.fillMaxWidth()) }
        error?.let { item { FormError(it) } }
        item {
            Field(instruction, { instruction = it }, stringResource(R.string.sms_instruction))
            TextButton({
                val replies = answers.map { (i, answer) -> "Sources ${items[i].getJSONArray("sourceIds")}: $answer" } + accountAnswers.map { (i, account) -> "Sources ${items[i].getJSONArray("sourceIds")}: use accountId=$account" }
                refine((listOf(instruction) + replies).joinToString("\n"))
            }, enabled = !saving && needsRefinement) { Text(stringResource(R.string.sms_refine)) }
        }
        itemsIndexed(items) { index, item ->
            Column(Modifier.fillMaxWidth().testTag("sms-proposal:$index").padding(vertical = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(when (item.text("disposition")) { "transaction" -> R.string.sms_ready; "question" -> R.string.sms_needs_answer; "pending" -> R.string.sms_pending; else -> R.string.sms_excluded }), style = MaterialTheme.typography.titleSmall)
                Text(Money.own(item.text("reason")), color = ur.text2)
                val transaction = item.optJSONObject("transaction")
                transaction?.let { t ->
                    Text(accounts.firstOrNull { it.first == t.text("accountId") }?.second.orEmpty(), color = ur.muted)
                    val category = budget.rows("categories").firstOrNull { it.text("id") == t.text("categoryId") }?.text("name")
                    Text(category ?: stringResource(R.string.sms_uncategorized), color = ur.muted)
                    prepared?.get(index)?.let { row -> BankReviewItem(row, row.decision(decisions[index]), !saving, Modifier.fillMaxWidth(), false) { decisions[index] = it } }
                    TextButton({ editing = index }, enabled = !saving) { Text(stringResource(R.string.sms_edit)) }
                }
                if (item.text("disposition") == "question") {
                    Text(Money.own(item.text("question")), color = ur.warning)
                    Field(answers[index].orEmpty(), { answers[index] = it }, stringResource(R.string.sms_answer))
                }
                if (!item.isNull("accountReference") && item.text("accountReference").isNotBlank()) {
                    Text(Money.own(item.text("accountReference")), color = ur.muted)
                    if (transaction == null) Choice(stringResource(R.string.bank_into_account), accountAnswers[index].orEmpty(), accounts) { accountAnswers[index] = it; remembered.remove(index) }
                    TextButton({
                        val account = accountAnswers[index] ?: transaction?.text("accountId").orEmpty()
                        scope.launch {
                            try {
                                withContext(smsStorageLane) {
                                    val inbox = SmsInbox.get(context); val identity = state.budgetIdentity!!
                                    val mappings = smsRememberMapping(JSONArray(inbox.state(identity, "mappings") ?: "[]"), item, batch.messages, account)
                                    inbox.saveState(identity, "mappings", mappings.toString())
                                }
                                remembered[index] = true
                                if (transaction == null) accountAnswers[index] = account
                            } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
                        }
                    }, enabled = !saving && remembered[index] != true && (accountAnswers[index] ?: transaction?.text("accountId").orEmpty()).isNotBlank()) { Text(stringResource(if (remembered[index] == true) R.string.sms_mapping_saved else R.string.sms_remember_mapping)) }
                }
                item.rows("evidence").forEach { evidence -> Text(Money.own("“${evidence.text("quote")}”"), style = MaterialTheme.typography.bodySmall, color = ur.muted) }
                val sourceIds = item.getJSONArray("sourceIds").let { a -> (0 until a.length()).map { a.getString(it) }.toSet() }
                batch.messages.filter { it.id in sourceIds }.forEach { source ->
                    Text(Money.own(source.sender + " · " + source.json().text("receivedDate")), style = MaterialTheme.typography.labelMedium)
                    Text(Money.own(source.body), style = MaterialTheme.typography.bodySmall, color = ur.text2)
                }
            }
        }
    }
    editing?.let { index -> SmsTransactionEditor(items[index].getJSONObject("transaction"), budget, { editing = null }) { edited ->
        val revised = JSONObject(batch.analysis.toString()); revised.getJSONArray("items").getJSONObject(index).put("transaction", edited)
        change(batch.copy(analysis = revised)); editing = null
    } }
}

@Composable
private fun SmsTransactionEditor(transaction: JSONObject, budget: JSONObject, close: () -> Unit, save: (JSONObject) -> Unit) {
    var date by remember { mutableStateOf(transaction.text("date")) }
    var payee by remember { mutableStateOf(transaction.text("payee")) }
    var amount by remember { mutableStateOf(transaction.text("amount")) }
    var memo by remember { mutableStateOf(transaction.text("memo")) }
    var account by remember { mutableStateOf(transaction.text("accountId")) }
    var category by remember { mutableStateOf(transaction.text("categoryId")) }
    var transfer by remember { mutableStateOf(transaction.text("transferAccountId")) }
    val accounts = budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") }
    val categories = budget.rows("categories").filter { !it.has("paymentAccountId") }.map { it.text("id") to it.text("name") }
    val valid = smsCents(amount) != null && payee.isNotBlank() && runCatching { java.time.LocalDate.parse(date) }.isSuccess && account != transfer
    FormDialog(stringResource(R.string.sms_edit), close, enabled = valid, onConfirm = {
        val kind = if (transfer.isNotBlank()) "transfer" else if (smsCents(amount)!! < 0) "expense" else if (transaction.text("kind") == "refund") "refund" else "income"
        save(JSONObject(transaction.toString()).put("date", date).put("payee", payee).put("amount", amount).put("memo", memo).put("accountId", account).put("kind", kind)
            .put("categoryId", category.takeIf { it.isNotBlank() && transfer.isBlank() } ?: JSONObject.NULL).put("transferAccountId", transfer.takeIf { it.isNotBlank() } ?: JSONObject.NULL))
    }) {
        Choice(stringResource(R.string.bank_into_account), account, accounts) { account = it }
        DateField(date, { date = it }); Field(payee, { payee = it }, stringResource(R.string.sms_payee))
        Field(amount, { amount = it }, stringResource(R.string.sms_amount, budget.text("currency")), ltr = true)
        Field(memo, { memo = it }, stringResource(R.string.sms_memo))
        Choice(stringResource(R.string.sms_category), category, listOf("" to stringResource(R.string.sms_uncategorized)) + categories) { category = it }
        Choice(stringResource(R.string.sms_transfer), transfer, listOf("" to stringResource(R.string.sms_not_transfer)) + accounts.filter { it.first != account }) { transfer = it }
    }
}

private suspend fun BudgetViewModel.smsQuery(method: String, params: JSONObject): JSONObject = suspendCancellableCoroutine { waiting ->
    val job = query(method, params, onError = { if (waiting.isActive) waiting.resumeWithException(IllegalStateException(it)) }) { if (waiting.isActive) waiting.resume(it) }
    waiting.invokeOnCancellation { job.cancel() }
}
