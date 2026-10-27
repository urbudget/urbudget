package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.FactCheck
import androidx.compose.material.icons.outlined.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.urbudget.nativeapp.BankReviewRow
import app.urbudget.nativeapp.BankReviewSummary
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.bankReviewChoices
import app.urbudget.nativeapp.bankReviewCounts
import app.urbudget.nativeapp.prepareBankReview
import app.urbudget.nativeapp.summarizeBankReview
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate

/** Nothing to import: every row is already in the budget (BankSync.tsx's nothingNew). */
private fun nothingNew(review: JSONObject) = review.text("error").isBlank() && review.rows("rows").all { it.optBoolean("duplicate") }

/** One linked account's download, as the screen and the review show it. */
private data class SyncEntry(val accountId: String, val review: JSONObject, val name: String, val bank: String, val fresh: Int, val quiet: Boolean,
                             val error: String, val bankBalance: String?, val bankDate: String?)

private fun syncEntries(download: JSONObject?, budget: JSONObject): List<SyncEntry> {
    val currency = budget.text("currency")
    val names = budget.rows("accounts").associate { it.text("id") to it.text("name") }
    return download?.rows("reviews").orEmpty().map { review ->
        val rows = review.rows("rows")
        SyncEntry(review.text("accountId"), review, names[review.text("accountId")] ?: review.text("name"), review.text("name"), rows.count { !it.optBoolean("duplicate") },
            nothingNew(review), review.text("error"),
            if (review.isNull("balance") || !review.has("balance")) null
            else money(review.optLong("balance"), currency), review.text("balanceDate").ifBlank { null }?.let(Money::date))
    }
}

@Composable
internal fun BankConnectionsScreen(budget: JSONObject, vm: BudgetViewModel) {
    val scope = rememberCoroutineScope()
    val summary = vm.state.collectAsStateWithLifecycle().value.summary
    var status by remember { mutableStateOf<JSONObject?>(null) }
    var token by remember { mutableStateOf("") }
    var download by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    var linking by remember { mutableStateOf<JSONObject?>(null) }
    var reviewBudget by remember { mutableStateOf<JSONObject?>(null) }
    // A sync reviews one account at a time; skipped and imported ones are left for later.
    var reviewing by remember { mutableStateOf(false) }
    var skipped by remember { mutableStateOf(setOf<String>()) }
    var imported by remember { mutableStateOf(setOf<String>()) }
    var first by remember { mutableStateOf<String?>(null) }
    val requests = rememberPreviewJobs()
    val accounts = remember(budget) { budget.rows("accounts") }
    val linkedAccounts = remember(accounts) { accounts.mapNotNull { account -> account.optJSONObject("simplefin")?.text("id")?.let { it to account } }.toMap() }
    val bankAccounts = remember(status) { status?.rows("accounts").orEmpty() }
    val prepared = rememberPrepared(download, budget) { download to syncEntries(download, budget) }
    // The rows on screen belong to this download, not one still being prepared.
    val ready = prepared != null && prepared.first === download
    val entries = prepared?.second.orEmpty()
    val pending = remember(entries, skipped, imported, first) {
        entries.filter { !it.quiet && it.error.isBlank() && it.accountId !in skipped && it.accountId !in imported }.sortedByDescending { it.accountId == first }
    }
    fun call(method: String, params: JSONObject = JSONObject(), callback: (JSONObject) -> Unit = { status = it }) {
        loading = true; error = null
        requests.track(vm.service(method, params, onError = { error = it; loading = false }) { value -> loading = false; (value as? JSONObject)?.let(callback) })
    }
    fun sync(refresh: Boolean, after: (JSONObject) -> Unit = {}) = call("simplefin_sync", JSONObject().put("refresh", refresh)) {
        download = it; reviewBudget = vm.state.value.budget; after(it)
    }
    // Accounts with nothing new are recorded as checked through the download, one import each with
    // no rows; ones already marked are left alone, so a quiet sync adds no empty undo step.
    fun markChecked(ids: List<String>, through: String) {
        if (ids.isEmpty()) { sync(false) { reviewing = true }; return }
        loading = true; error = null
        requests.track(vm.service("simplefin_import", JSONObject().put("account", ids.first()).put("through", through).put("choices", JSONArray()),
            onError = { error = it; loading = false }) { markChecked(ids.drop(1), through) })
    }
    fun downloadActivity() {
        skipped = emptySet(); imported = emptySet(); first = null; reviewing = false
        sync(true) { result ->
            requests.track(scope.launch {
                val through = result.text("through")
                val current = vm.state.value.budget
                val quiet = withContext(Dispatchers.Default) {
                    result.rows("reviews").filter { nothingNew(it) && current?.row("accounts", it.text("accountId"))?.optJSONObject("simplefin")?.text("lastSync") != through }
                        .map { it.text("accountId") }
                }
                if (quiet.isEmpty()) reviewing = true else markChecked(quiet, through)
            })
        }
    }
    LaunchedEffect(Unit) { call("simplefin_status") }
    LaunchedEffect(reviewing, pending, loading, ready) { if (reviewing && ready && !loading && pending.isEmpty()) reviewing = false }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item { Text("SimpleFIN Bridge · Nothing is added until you review it.", Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium) }
        if (loading) item { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        error?.let { item { FormError(it) } }
        if (status?.optBoolean("connected") != true) {
            item {
                ListCard {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        Text("Your bank login stays with SimpleFIN. The connection token is protected by this device's credential store and kept out of budget backups.",
                            color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                        FilledField(token, { token = it }, Modifier.fillMaxWidth(), label = { Text("SimpleFIN setup token") }, visualTransformation = PasswordVisualTransformation(), singleLine = true)
                        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            PrimaryButton("Connect", { val pasted = token; token = ""; call("simplefin_connect", JSONObject().put("token", pasted)) }, Modifier.weight(1f), enabled = token.isNotBlank() && !loading)
                            SecondaryButton("Get a token", { call("simplefin_open") }, Modifier.weight(1f), enabled = !loading)
                        }
                    }
                }
            }
        } else {
            item {
                ListCard {
                    ListRow("Connected", subtitle = "${status?.optInt("requestsToday")} of ${status?.optInt("dailyLimit")} requests used today", icon = Icons.Outlined.Link,
                        iconTint = ur.positive, iconFill = ur.positiveSoft, chevron = false, divider = true)
                    ListRow("Download activity", subtitle = if (linkedAccounts.isEmpty()) "Link a budget account first" else "From every linked account, then review one account at a time",
                        icon = Icons.Outlined.CloudDownload, onClick = if (!loading && linkedAccounts.isNotEmpty()) ({ downloadActivity() }) else null, chevron = false, divider = true)
                    ListRow("Refresh accounts", icon = Icons.Outlined.Refresh, onClick = { if (!loading) call("simplefin_accounts") }, chevron = false, divider = true)
                    ListRow("Disconnect this device", titleColor = ur.negative, onClick = { if (!loading) call("simplefin_disconnect") { status = it; download = null; reviewing = false } }, chevron = false)
                }
            }
            if (bankAccounts.isNotEmpty()) {
                item { SectionTitle("Bank accounts", "Tap one to link it to a budget account.") }
                item {
                    ListCard {
                        bankAccounts.forEachIndexed { index, bank ->
                            val linked = linkedAccounts[bank.text("id")]
                            ListRow(bank.text("name"), subtitle = listOfNotNull(bank.text("org").ifBlank { null },
                                linked?.let { "Linked to ${it.text("name")} · Last sync ${it.optJSONObject("simplefin")?.text("lastSync").orEmpty().ifBlank { "never" }}" } ?: "Not linked").joinToString(" · "),
                                value = if (bank.isNull("balance")) null else money(bank.optLong("balance"), bank.text("currency", budget.text("currency"))),
                                icon = Icons.Outlined.AccountBalance, iconTint = if (linked != null) ur.accent else ur.text2, iconFill = if (linked != null) ur.accentSoft else ur.zeroSoft,
                                onClick = { linking = bank }, divider = index < bankAccounts.lastIndex)
                        }
                    }
                }
            }
        }
        val messages = (status?.optJSONArray("messages")?.let { (0 until it.length()).map(it::optString) }.orEmpty() + download?.optJSONArray("messages")?.let { (0 until it.length()).map(it::optString) }.orEmpty()).distinct()
        items(messages) { Notice(it, tone = "attention", icon = Icons.Outlined.WarningAmber) }
        if (download != null) {
            item { SectionTitle("Downloaded activity", "Through ${Money.date(download?.text("through").orEmpty())} · ${download?.optInt("requestsToday")} of ${download?.optInt("dailyLimit")} requests today") }
            if (entries.isNotEmpty()) item {
                ListCard {
                    entries.forEachIndexed { index, entry ->
                        val open = !entry.quiet && entry.error.isBlank() && entry.accountId !in imported
                        ListRow(entry.name, subtitle = listOfNotNull(when {
                            entry.error.isNotBlank() -> entry.error
                            entry.accountId in imported -> "Reviewed"
                            entry.quiet -> "Up to date"
                            entry.accountId in skipped -> "Skipped for now · ${entry.fresh} new"
                            else -> "${entry.fresh} new to review"
                        }, entry.bankBalance?.let { "Bank balance $it${entry.bankDate?.let { date -> " on $date" }.orEmpty()}" }).joinToString(" · "),
                            icon = Icons.AutoMirrored.Outlined.FactCheck, iconTint = if (entry.error.isNotBlank()) ur.negative else if (open) ur.accent else ur.positive,
                            iconFill = if (entry.error.isNotBlank()) ur.negativeSoft else if (open) ur.accentSoft else ur.positiveSoft,
                            onClick = if (open && !loading) ({ skipped = skipped - entry.accountId; first = entry.accountId; reviewing = true }) else null, divider = index < entries.lastIndex)
                    }
                }
            }
            if (pending.isNotEmpty() && !reviewing) item {
                PrimaryButton(if (pending.size == 1) "Review 1 account" else "Review ${pending.size} accounts", { first = null; reviewing = true }, Modifier.fillMaxWidth(), enabled = !loading)
            }
        }
    }
    linking?.let { bank -> BankLinkDialog(bank, budget, vm) { linking = null; download = null; reviewing = false } }
    val current = pending.firstOrNull()
    if (reviewing && ready && !loading && current != null) key(current.accountId) {
        val ours = summary?.rows("accounts")?.firstOrNull { it.text("id") == current.accountId }?.optLong("balance")
            ?: budget.row("accounts", current.accountId)?.optLong("openingBalance") ?: 0L
        BankConnectionReview(current, download?.text("through").orEmpty(), reviewBudget, budget, vm, pending.size - 1, money(ours, budget.text("currency")),
            close = { reviewing = false }, skip = { skipped = skipped + current.accountId; if (first == current.accountId) first = null }) {
            imported = imported + current.accountId; if (first == current.accountId) first = null
            sync(false)
        }
    }
}

@Composable
private fun BankLinkDialog(bank: JSONObject, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val existing = remember(budget, bank) { budget.rows("accounts").firstOrNull { it.optJSONObject("simplefin")?.text("id") == bank.text("id") } }
    val accountChoices = remember(budget) { listOf("" to "Choose account") + budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") } }
    var account by remember { mutableStateOf(existing?.text("id") ?: "") }
    var from by remember { mutableStateOf(existing?.optJSONObject("simplefin")?.text("from")?.ifBlank { null } ?: LocalDate.now().minusDays(30).toString()) }
    var flip by remember { mutableStateOf(existing?.optJSONObject("simplefin")?.optBoolean("flipSigns") ?: false) }
    var error by remember { mutableStateOf<String?>(null) }
    FormDialog("Link ${bank.text("name")}", close, enabled = account.isNotBlank(), onConfirm = {
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            val date = LocalDate.parse(from); require(!date.isAfter(LocalDate.now())) { "Choose today or an earlier date." }
            val chosen = budget.row("accounts", account) ?: error("Choose an account.")
            require(bank.text("currency").isBlank() || bank.text("currency") == budget.text("currency")) { "This bank account uses a different currency." }
            val link = (if (existing?.text("id") == account) existing.optJSONObject("simplefin")?.copyJson() else null) ?: JSONObject()
            link.put("id", bank.text("id")).put("from", from).put("flipSigns", flip)
            val changes = mutableListOf(chosen.copyJson().put("simplefin", link))
            if (existing != null && existing.text("id") != account) changes += existing.copyJson().put("simplefin", JSONObject.NULL)
            vm.upsertMany("accounts", changes); close()
        }.onFailure { error = it.message }
    }) {
        Choice("Budget account", account, accountChoices, { account = it })
        DateField(from, { from = it }, "First import from")
        CheckRow("Reverse bank amount signs", flip, { flip = it })
        Text("Up to one year of history, downloaded in 45-day windows. Each window uses one request.", style = MaterialTheme.typography.bodySmall, color = ur.muted)
        if (existing != null) TextButton({ withCurrentBudget(openedBudget, vm) { vm.upsert("accounts", existing.copyJson().put("simplefin", JSONObject.NULL)); close() } }) { Text("Unlink account", color = ur.negative) }
        FormError(error)
    }
}

/**
 * One account's downloaded rows (BankSync.tsx): what each does, the bank's balance beside
 * UrBudget's, and Skip this account to come back to it later.
 */
@Composable
private fun BankConnectionReview(entry: SyncEntry, through: String, reviewedBudget: JSONObject?, budget: JSONObject, vm: BudgetViewModel, after: Int, ours: String,
                                 close: () -> Unit, skip: () -> Unit, imported: () -> Unit) {
    val review = entry.review
    val scope = rememberCoroutineScope()
    val requests = rememberPreviewJobs(review)
    val preparationKey = remember(review, reviewedBudget, budget) { Any() }
    val prepared by produceState<ScopedResult<Pair<List<BankReviewRow>, BankReviewSummary>>?>(null, preparationKey) {
        value = withContext(Dispatchers.Default) {
            val rows = prepareBankReview(review.rows("rows"), reviewedBudget ?: budget, false, review.optJSONArray("candidateSets"))
            ScopedResult(preparationKey, rows to summarizeBankReview(rows))
        }
    }
    val ready = prepared?.forScope(preparationKey)
    val rows = ready?.first
    val decisions = remember(review) { mutableStateMapOf<Int, String>() }
    val counts by remember { derivedStateOf { bankReviewCounts(rows.orEmpty(), decisions) } }
    val (importing, undecided) = counts
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    LazyFormDialog("Review bank activity", close, confirm = if (loading) "Importing…" else if (importing > 0) "Import $importing" else "Mark checked",
        enabled = rows != null && !loading && undecided == 0 && review.text("error").isBlank(), onConfirm = {
        if (reviewedBudget !== vm.state.value.budget) error = "The budget changed. Close and download activity again to review current matches."
        else {
            loading = true
            val selected = decisions.toMap(); val sourceRows = rows.orEmpty()
            scope.launch {
                try {
                    val choices = withContext(Dispatchers.Default) { bankReviewChoices(sourceRows, selected, false) }
                    require(reviewedBudget === vm.state.value.budget) { "The budget changed. Download activity again to review current matches." }
                    requests.track(vm.service("simplefin_import", JSONObject().put("account", review.text("accountId")).put("through", through).put("choices", choices), onError = { loading = false; error = it }) { imported() })
                } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { loading = false; error = failure.message }
            }
        }
    }) {
        item(key = "sync:title") {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("${entry.bank.ifBlank { "SimpleFIN" }} into ${entry.name}", style = MaterialTheme.typography.titleSmall, color = ur.text)
                    Text(listOfNotNull(ready?.second?.let(::reviewCounts), if (after > 0) "$after more after this" else null).joinToString(" · ").ifEmpty { "Checking matches…" },
                        style = MaterialTheme.typography.bodySmall, color = ur.muted)
                }
                TextButton(skip, enabled = !loading) { Text("Skip this account", color = ur.accent, fontWeight = FontWeight.SemiBold) }
            }
        }
        item(key = "sync:balances") {
            ListCard {
                Row(Modifier.padding(16.dp)) {
                    Figure(entry.bankDate?.let { "At the bank on $it" } ?: "At the bank", entry.bankBalance ?: "Not sent", Modifier.weight(1f))
                    Figure("UrBudget, before this import", ours, Modifier.weight(1f))
                }
            }
        }
        if (loading || rows == null) item(key = "sync:loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        error?.let { item(key = "sync:error") { FormError(it) } }
        ready?.second?.let { bankReviewNotes(it, undecided) }
        rows?.let { suggestedTransfers(it, decisions, !loading) }
        if (rows?.isEmpty() == true) item(key = "sync:none") { Text("No new transactions. Confirm to record the account as checked through ${Money.date(through)}.", color = ur.text2) }
        bankReviewRows(rows.orEmpty(), decisions, !loading) { index, value -> decisions[index] = value }
    }
}
