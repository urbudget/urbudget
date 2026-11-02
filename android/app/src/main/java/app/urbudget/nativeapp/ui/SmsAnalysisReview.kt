package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CalendarToday
import androidx.compose.material.icons.outlined.MoreHoriz
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * The prepared batch as transactions to add, not messages to read. Cards the phone can't match
 * ask once for every message that names them; amounts in another currency are typed in the row;
 * anything else is changed in the transaction editor. Ticked rows are added in one undoable step,
 * and every message the review settles leaves Bank messages; those still waiting on a choice stay
 * in the review, as they were read. [change] applies an edit to the review as it is when it runs:
 * a row drawn before the last tick must not bring back what that tick changed.
 */
@Composable
internal fun SmsReview(budget: JSONObject, vm: BudgetViewModel, batch: SmsBatch, mappings: JSONArray, close: () -> Unit, change: ((SmsBatch) -> SmsBatch) -> Unit,
                       saveCards: (JSONArray) -> Unit, finish: (List<String>, SmsBatch?) -> Unit, again: (String) -> Unit, discard: () -> Unit,
                       edited: (SmsItem, Set<String>, String, String?) -> Unit = { _, _, _, _ -> }) {
    val state by vm.state.collectAsStateWithLifecycle()
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val accounts = remember(budget) { budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") } }
    val starts = remember(budget, batch.fresh) { batch.starts(budget) }
    // Ticking and dismissing don't change what's previewed; only the analysis and the cards do.
    val items = remember(batch.analysis, batch.messages, mappings, accounts, starts) { batch.items(SmsAccounts(accounts, mappings, starts)) }
    val resolved = remember(items) { batch.resolved(items) }
    var prepared by remember { mutableStateOf<Map<Int, BankReviewRow>?>(null) }
    var checked by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf<Int?>(null) }
    var others by remember { mutableStateOf(false) }
    var menu by remember { mutableStateOf(false) }
    var noting by remember { mutableStateOf(false) }
    var shown by remember { mutableStateOf<Set<Int>>(emptySet()) }
    // The rows a day is picked for ("Tick from a day"), and the account being made for a text.
    var fromDay by remember { mutableStateOf<List<SmsItem>?>(null) }
    var creating by remember { mutableStateOf<SmsNewAccount?>(null) }
    val decisions = remember { mutableStateMapOf<Int, String>() }
    val keepCard = remember { mutableStateMapOf<String, Boolean>() }
    LaunchedEffect(budget, resolved) {
        prepared = null; checked = null; error = null
        try { prepared = vm.smsPreview(budget, batch, resolved); checked = budget }
        catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
    }
    LaunchedEffect(state.error) { if (state.error != null) { error = state.error; saving = false } }
    androidx.activity.compose.BackHandler(onBack = close)

    val known = remember(prepared, items, budget) { smsKnown(prepared, items, budget) }
    // A choice made before an edit holds while the row still offers it.
    fun decision(index: Int, row: BankReviewRow) = row.decision(decisions[index]?.takeIf { d -> row.options.any { it.first == d } } ?: known[index])
    // Read through state, so a row the list didn't redraw still shows the review as it is now.
    val latest = rememberUpdatedState(batch)
    fun ticked(item: SmsItem) = item.tickedIn(latest.value)
    fun tick(item: SmsItem, on: Boolean) = change { it.copy(ticked = it.ticked + (item.index to on)) }
    fun tickAll(rows: List<SmsItem>, on: Boolean) = change { b -> b.copy(ticked = b.ticked + rows.filter { it.needs == null }.associate { it.index to on }) }
    /** Ticks what's dated [day] or later and unticks the rest: a first import that starts where the budget does. */
    fun tickFrom(rows: List<SmsItem>, day: String) = change { b -> b.copy(ticked = b.ticked + rows.filter { it.needs == null }.associate { it.index to (it.transaction?.text("date").orEmpty() >= day) }) }
    val sorted = smsSort(batch, items, known)
    val (waiting, ready, existing, holds, skipped) = sorted
    // Newest first, as the bank's app lists them, under the day they're dated.
    val readyShown = remember(ready) { smsNewestFirst(ready) }
    val holdsShown = remember(holds) { smsNewestFirst(holds) }
    val cards = waiting.filter { it.card != null }.groupBy { it.card!! }
    val adding = prepared?.let { rows -> (ready + existing + holds).filter { it.needs == null && ticked(it) }.mapNotNull { item ->
        rows[item.index]?.takeIf { !it.locked }?.let { row -> item.index to decision(item.index, row) }?.takeIf { it.second != "skip" }
    } }.orEmpty()
    val undecided = adding.count { it.second == "review" }
    // Settled: added, unticked, held, not a transaction, or dismissed. Waiting for a choice: kept for later.
    val settled = items.filter { it !in waiting && !(it.disposition == "pending" && it.needs != null && ticked(it)) }
    val later = items.size - settled.size
    fun confirm() {
        if (checked !== vm.state.value.budget || undecided > 0 || saving) return
        val handled = settled.flatMap { item -> item.sources.map { it.id } }
        // What still waits on a choice stays in the review as it was read, not read again.
        val rest = batch.only(items.filter { it !in settled }.map { it.index })
        if (adding.isEmpty()) { finish(handled, rest); return }
        saving = true
        val choices = smsDecisions(adding)
        val count = adding.count { it.second == "new" }.toLong()
        vm.runCommand("smsBatchApply", batch.params(resolved).put("decisions", choices)) { if (count > 0) toast(Words.count(R.plurals.sms_added, count), false); finish(handled, rest) }
    }
    fun chooseCard(group: List<SmsItem>, account: String) {
        val first = group.first()
        if (keepCard[first.card] != false) runCatching { saveCards(smsRememberCard(mappings, first.sender, first.ref!!, account)) }.onFailure { error = it.message }
        else change { b -> group.fold(b) { next, item -> next.edit(item.index) { it.getJSONObject("transaction").put("accountId", account) } } }
    }
    /** An account made for these texts: its balance is from before them, so none is held back as already in it. */
    fun made(account: String) = change { it.copy(fresh = it.fresh + account) }
    fun newForCard(group: List<SmsItem>) {
        val first = group.first()
        creating = SmsNewAccount(smsAccountName(first.sender, first.ref!!)) { id -> made(id); chooseCard(group, id) }
    }
    /** The other account of a transfer, made here and remembered for its digits, so the sender's next texts naming it find it. */
    fun newForTransfer(item: SmsItem) {
        val other = item.transaction?.text("otherRef")?.ifBlank { null }
        creating = SmsNewAccount(other?.let { smsAccountName(item.sender, it) }.orEmpty()) { id ->
            made(id)
            if (other != null) runCatching { saveCards(smsRememberCard(mappings, item.sender, other, id)) }.onFailure { error = it.message }
            else change { it.edit(item.index) { json -> json.getJSONObject("transaction").put("transferAccountId", id) } }
        }
    }
    fun enterAmount(item: SmsItem, cents: Long) {
        val out = item.transaction?.text("kind") in setOf("expense", "transfer")
        change { it.edit(item.index) { json -> json.getJSONObject("transaction").put("amount", smsDecimal(if (out) -cents else cents)) } }
    }

    val readyTitle = stringResource(R.string.sms_section_ready)
    val holdsTitle = stringResource(R.string.sms_section_holds)
    val holdsDetail = stringResource(R.string.sms_holds_detail)
    val knownDetail = stringResource(R.string.sms_known_detail)
    val knownTitle = countText(R.plurals.sms_known, existing.size)
    val skippedTitle = countText(R.plurals.sms_not_transactions, skipped.size)
    Column(Modifier.fillMaxSize().testTag("sms-review-page")) {
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item(key = "head") {
                SectionTitle(stringResource(R.string.sms_review_title), countText(R.plurals.sms_message_count, batch.messages.size)) {
                    Box {
                        IconButton({ menu = true }, enabled = !saving) { Icon(Icons.Outlined.MoreHoriz, stringResource(R.string.common_more_actions), tint = ur.text) }
                        DropdownMenu(menu, { menu = false }, shape = androidx.compose.foundation.shape.RoundedCornerShape(14.dp), containerColor = ur.surface) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.sms_prepare_again)) }, onClick = { menu = false; noting = true })
                            DropdownMenuItem(text = { Text(stringResource(R.string.sms_discard_review), color = ur.negative) }, onClick = { menu = false; discard() })
                        }
                    }
                }
            }
            error?.let { item(key = "error") { FormError(it) } }
            if (prepared == null && error == null) item(key = "loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
            if (waiting.isNotEmpty()) {
                item(key = "choose") { SectionTitle(stringResource(R.string.sms_section_choose), stringResource(R.string.sms_section_choose_detail)) }
                cards.forEach { (key, group) ->
                    item(key = "card:$key") {
                        SmsCardChoice(group, accounts, keepCard[key] != false, { keepCard[key] = it }, { newForCard(group) }) { chooseCard(group, it) }
                    }
                }
                val singles = waiting.filter { it.card == null }
                itemsIndexed(singles, key = { _, it -> "single:${it.index}" }) { i, item ->
                    if (item.transaction == null) SmsUnclearRow(item, i, singles.size, { editing = item.index }) { change { it.copy(dismissed = it.dismissed + item.index) } }
                    else SmsTransactionRow(item, budget, null, null, null, i, singles.size, item.index in shown, !saving,
                        { shown = shown.toggle(item.index) }, {}, {}, { editing = item.index }, newAccount = { newForTransfer(item) }.takeIf { item.needs == "transfer" }) { enterAmount(item, it) }
                }
            }
            rowSection("ready", readyTitle, null, readyShown, budget, prepared, ::decision, ::ticked, shown, !saving,
                { shown = shown.toggle(it) }, ::tick, { i, d -> decisions[i] = d }, { editing = it }, ::enterAmount, tickAll = { tickAll(ready, it) }, fromDay = { fromDay = ready }, days = true)
            rowSection("holds", holdsTitle, holdsDetail, holdsShown, budget, prepared, ::decision, ::ticked, shown, !saving,
                { shown = shown.toggle(it) }, ::tick, { i, d -> decisions[i] = d }, { editing = it }, ::enterAmount, tickAll = { tickAll(holds, it) }, fromDay = { fromDay = holds }, days = true)
            // What changes nothing in the budget is handled without a question, and open to look at.
            if (existing.isNotEmpty() || skipped.isNotEmpty()) {
                item(key = "handled") {
                    Column(Modifier.padding(top = 4.dp)) { ListCard(Modifier.testTag("sms-handled")) {
                        ListRow(countText(R.plurals.sms_handled, existing.size + skipped.size), subtitle = handledSummary(existing.size, skipped), onClick = { others = !others }, chevron = false,
                            trailing = { Text(stringResource(if (others) R.string.sms_hide else R.string.sms_show), color = ur.accent, fontWeight = FontWeight.SemiBold) })
                    } }
                }
                if (others && existing.isNotEmpty()) rowSection("known", knownTitle, knownDetail, existing, budget, prepared, ::decision, ::ticked, shown, !saving,
                    { shown = shown.toggle(it) }, ::tick, { i, d -> decisions[i] = d }, { editing = it }, ::enterAmount)
                if (others && skipped.isNotEmpty()) {
                    item(key = "skipped") { SectionTitle(skippedTitle, null) }
                    itemsIndexed(skipped, key = { _, it -> "skip:${it.index}" }) { i, item ->
                        SmsUnclearRow(item, i, skipped.size, { editing = item.index }, null)
                    }
                }
            }
        }
        // The decision stays in reach: what this adds, what it does to each balance, and what else it settles.
        Column(Modifier.fillMaxWidth().background(ur.bg).navigationBarsPadding().padding(horizontal = Gutter, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            val changes = smsBalanceChanges(items, adding)
            if (changes.isNotEmpty()) SmsBalances(changes, budget, state.summary)
            val cleared = settled.size - adding.size
            val matching = adding.count { it.second.startsWith("match:") }
            val caption = listOfNotNull(
                if (undecided > 0) countText(R.plurals.sms_undecided, undecided) else null,
                if (matching > 0) countText(R.plurals.sms_matches, matching) else null,
                if (cleared > 0) countText(R.plurals.sms_clears, cleared) else null,
                if (later > 0) countText(R.plurals.sms_later, later) else null,
            ).joinToString(" · ")
            if (caption.isNotEmpty()) Text(caption, Modifier.fillMaxWidth(), style = MaterialTheme.typography.bodySmall, color = if (undecided > 0) ur.warning else ur.muted, textAlign = TextAlign.Center)
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                SecondaryButton(stringResource(R.string.sms_later_action), close, Modifier.weight(1f), enabled = !saving)
                PrimaryButton(when {
                    adding.size > matching -> countText(R.plurals.sms_add, adding.size - matching)
                    settled.isNotEmpty() -> countText(R.plurals.sms_clear, settled.size)
                    else -> stringResource(R.string.sms_nothing)
                }, ::confirm, Modifier.weight(2f).testTag("sms-add"),
                    enabled = !saving && checked === state.budget && undecided == 0 && settled.isNotEmpty())
            }
        }
    }
    fromDay?.let { rows ->
        DatePickerSheet(java.time.LocalDate.now().toString(), { day -> tickFrom(rows, day); fromDay = null }) { fromDay = null }
    }
    creating?.let { request -> SmsNewAccountDialog(request, budget, vm) { creating = null } }
    editing?.let { index -> items.firstOrNull { it.index == index } }?.let { item ->
        TransactionDialog(smsEditable(item, currency), budget, "", vm, close = { editing = null }, dialogTitle = stringResource(R.string.sms_check_title), saveOverride = { record ->
            change { smsEdited(it, item, record, budget) }
            // A row a template read, changed by the user: counted against the template, and its payee learned.
            val (fields, category) = smsChanged(item, record)
            if (fields.isNotEmpty()) edited(item, fields, record.text("payee"), category)
        }, extraFields = { SmsSources(item) })
    }
    if (noting) {
        var note by remember { mutableStateOf("") }
        FormDialog(stringResource(R.string.sms_prepare_again), { noting = false }, confirm = stringResource(R.string.sms_prepare_again_action), onConfirm = { noting = false; again(note.trim()) }) {
            Text(stringResource(R.string.sms_note_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            Field(note, { if (it.length <= 2000) note = it }, stringResource(R.string.sms_note_label), lines = 4)
        }
    }
}

/** The choice that makes a new account, beside the budget's own. */
private const val NEW_ACCOUNT = "\u0000new"

private fun Set<Int>.toggle(value: Int) = if (value in this) this - value else this + value

/**
 * Each account's balance now and after adding ("ANB 3358  5,000.00 → −13,506.45"): a month of texts
 * added on top of a balance entered today shows here before it's added. A balance that would go
 * below zero from zero or more is in the warning colour.
 */
@Composable
private fun SmsBalances(changes: Map<String, Long>, budget: JSONObject, summary: JSONObject?) {
    val currency = budget.text("currency")
    val now = remember(summary) { summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty() }
    Column(Modifier.fillMaxWidth().testTag("sms-balances"), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(stringResource(R.string.sms_balances_title), style = MaterialTheme.typography.labelMedium, color = ur.muted)
        changes.entries.take(3).forEach { (account, change) ->
            val before = now[account] ?: budget.row("accounts", account)?.optLong("openingBalance") ?: 0L
            val after = before + change
            Row(Modifier.fillMaxWidth().testTag("sms-balance:$account"), verticalAlignment = Alignment.CenterVertically) {
                Text(Money.own(budget.row("accounts", account)?.text("name").orEmpty()), Modifier.weight(1f).padding(end = 10.dp), style = MaterialTheme.typography.bodySmall,
                    color = ur.text2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(stringResource(R.string.sms_balance_change, money(before, currency), money(after, currency)), style = MaterialTheme.typography.bodySmall.merge(Tabular),
                    fontWeight = FontWeight.SemiBold, color = if (after < 0 && before >= 0) ur.warning else ur.text, maxLines = 1)
            }
        }
        if (changes.size > 3) Text(countText(R.plurals.sms_balance_more, changes.size - 3), style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
}

/**
 * A review's items by what they need. [waiting] needs a choice, [ready] are new transactions to
 * add and [holds] are holds to decide; [existing] (already in the budget) and [skipped] (no money
 * moved) are handled without a question.
 */
internal data class SmsSorted(val waiting: List<SmsItem>, val ready: List<SmsItem>, val existing: List<SmsItem>, val holds: List<SmsItem>, val skipped: List<SmsItem>) {
    val needs: Int get() = waiting.size + ready.size + holds.size
}
internal fun smsSort(batch: SmsBatch, items: List<SmsItem>, known: Map<Int, String>): SmsSorted {
    val ready = items.filter { it.disposition == "transaction" && it.needs == null }
    return SmsSorted(
        items.filter { (it.disposition == "transaction" && it.needs != null) || (it.disposition == "unclear" && it.index !in batch.dismissed) },
        ready.filter { it.index !in known }, ready.filter { it.index in known }, items.filter { it.disposition == "pending" },
        items.filter { it.disposition == "ignore" || (it.disposition == "unclear" && it.index in batch.dismissed) })
}

/**
 * Texts for purchases the budget already has (a bank file brought them in), each to the one
 * transaction the bank review offers that is surely it, even when that import claimed it or the
 * same amount recurs on other days (the 8.00 laundry every few days). Two texts that would match
 * the same transaction, a possible duplicate or a transfer aren't here, and still ask.
 */
internal fun smsKnown(prepared: Map<Int, BankReviewRow>?, items: List<SmsItem>, budget: JSONObject): Map<Int, String> {
    val open = prepared.orEmpty().filterValues { row ->
        !row.locked && !row.possibleDuplicate && row.transfer == null && (row.initial == "review" || row.initial.startsWith("match:"))
    }.mapValues { (_, row) -> row.options.map { it.first }.filter { it.startsWith("match:") } }
    val wanted = open.values.flatten().map { it.removePrefix("match:") }.toSet()
    val saved = budget.rows("transactions").filter { it.text("id") in wanted }.associateBy { it.text("id") }
    val texts = items.associateBy { it.index }
    val imported = budget.optJSONArray("smsImportedMessageIds").let { ids -> (0 until (ids?.length() ?: 0)).map { ids!!.getString(it) } }
    val claimed = saved.values.map { it.text("accountId") }.distinct().associateWith { smsBankIds(it, imported) }
    val sure = open.mapNotNull { (index, matches) ->
        val t = texts[index]?.transaction ?: return@mapNotNull null
        matches.filter { match ->
            saved[match.removePrefix("match:")]?.let { smsSameTransaction(t, it, claimed[it.text("accountId")].orEmpty()) } == true
        }.singleOrNull()?.let { index to it }
    }
    val shared = sure.groupingBy { it.second }.eachCount()
    return sure.filter { shared[it.second] == 1 }.toMap()
}

/** The bank review's rows for the review's items that can be added, by item. */
internal suspend fun BudgetViewModel.smsPreview(budget: JSONObject, batch: SmsBatch, resolved: JSONObject): Map<Int, BankReviewRow> {
    val raw = smsQuery("smsBatchPreview", batch.params(resolved)).rows("items")
    val rows = withContext(Dispatchers.Default) { prepareBankReview(raw.map { it.getJSONObject("row") }, budget, false) }
    return raw.mapIndexed { i, item -> item.getInt("index") to rows[i] }.toMap()
}

/** Each item's decision as the core takes it: add it new, or match an existing transaction. */
internal fun smsDecisions(adding: List<Pair<Int, String>>): JSONArray = JSONArray(adding.map { (index, d) ->
    JSONObject().put("index", index).put("action", if (d.startsWith("match:")) "match" else d).put("matchId", if (d.startsWith("match:")) d.removePrefix("match:") else JSONObject.NULL)
})

/** "17 already in your budget · 24 codes · 4 security alerts". */
@Composable
internal fun handledSummary(existing: Int, skipped: List<SmsItem>): String =
    listOfNotNull(if (existing > 0) countText(R.plurals.sms_known, existing) else null, if (skipped.isNotEmpty()) skipSummary(skipped) else null).joinToString(" · ")

/**
 * Whether a text's transaction is surely `there`, an existing transaction of the same amount
 * within ten days the bank review offers: in the same account on the same day. Its payee may be
 * the user's own name for it (the restaurant behind a HungerStation order). On another day it
 * may be last week's identical coffee, and one another text already matched (`claimed`, the
 * engine's ids for the texts imported into that account) is a second purchase, so neither is.
 */
internal fun smsSameTransaction(text: JSONObject, there: JSONObject, claimed: Set<String>): Boolean {
    val ids = there.optJSONArray("bankImportIds")
    return there.text("accountId") == text.text("accountId") && there.text("date") == text.text("date") &&
        (0 until (ids?.length() ?: 0)).none { ids!!.optString(it) in claimed }
}

/** The ids the bank import gives texts it took into `account`: "bank:" and the SHA-256 of "account|fit:sms:…". */
internal fun smsBankIds(account: String, smsIds: List<String>): Set<String> {
    val digest = java.security.MessageDigest.getInstance("SHA-256")
    return smsIds.mapTo(HashSet()) { id -> "bank:" + digest.digest("$account|fit:$id".toByteArray()).joinToString("") { "%02x".format(it) } }
}

private fun LazyListScope.rowSection(key: String, title: String?, detail: String?, rows: List<SmsItem>, budget: JSONObject, prepared: Map<Int, BankReviewRow>?,
                                     decision: (Int, BankReviewRow) -> String, ticked: (SmsItem) -> Boolean, shown: Set<Int>, enabled: Boolean, show: (Int) -> Unit,
                                     tick: (SmsItem, Boolean) -> Unit, decide: (Int, String) -> Unit, open: (Int) -> Unit, enter: (SmsItem, Long) -> Unit,
                                     tickAll: ((Boolean) -> Unit)? = null, fromDay: (() -> Unit)? = null, days: Boolean = false) {
    if (rows.isEmpty()) return
    // Two or more rows to tick are ticked or unticked together, or from a day on.
    val tickable = rows.filter { it.needs == null }
    title?.let { item(key = key) { SectionTitle(it, detail, if (tickAll == null || tickable.size < 2) null else { {
        Row(verticalAlignment = Alignment.CenterVertically) {
            fromDay?.let { pick ->
                IconButton(pick, Modifier.testTag("sms-tick-from:$key"), enabled = enabled) {
                    Icon(Icons.Outlined.CalendarToday, stringResource(R.string.sms_tick_from_day), tint = ur.accent, modifier = Modifier.size(20.dp))
                }
            }
            val any = tickable.any(ticked)
            TextButton({ tickAll(!any) }, Modifier.testTag("sms-tick-all:$key"), enabled = enabled) {
                Text(stringResource(if (any) R.string.sms_untick_all else R.string.sms_tick_all), color = ur.accent, fontWeight = FontWeight.SemiBold)
            }
        }
    } }) } }
    // Under the day they're dated, when [days]: the order is the caller's, newest first.
    val groups = if (days) rows.groupBy { it.transaction?.text("date").orEmpty() } else mapOf("" to rows)
    groups.forEach { (day, group) ->
        if (day.isNotBlank()) item(key = "$key-day:$day") {
            Text(Money.joined(listOf(Money.date(day), Words.count(R.plurals.sms_transaction_count, group.size.toLong()))), Modifier.padding(top = 2.dp).testTag("sms-day:$day"),
                style = MaterialTheme.typography.labelLarge, color = ur.muted)
        }
        itemsIndexed(group, key = { _, it -> "$key:${it.index}" }) { i, item ->
            val row = prepared?.get(item.index)
            SmsTransactionRow(item, budget, row, row?.let { decision(item.index, it) }, if (item.needs == null) ticked(item) && row?.locked != true else null, i, group.size,
                item.index in shown, enabled && row != null && !row.locked, { show(item.index) }, { tick(item, it) }, { decide(item.index, it) }, { open(item.index) }) { enter(item, it) }
        }
    }
}

/** Newest first, as a bank lists them; a day's texts in the order they came, latest first. */
internal fun smsNewestFirst(items: List<SmsItem>): List<SmsItem> =
    items.sortedWith(compareByDescending<SmsItem> { it.transaction?.text("date").orEmpty() }.thenByDescending { it.index })

/** A name for an account a text names that the budget doesn't have yet: "ANB 0070". */
internal fun smsAccountName(sender: String, ref: String): String =
    listOf(sender.trim().uppercase(java.util.Locale.ROOT), ref.filter { it in '0'..'9' }.ifEmpty { ref.trim() }).filter { it.isNotBlank() }.joinToString(" ")

/** An account to make from the review, named [name] to start with; [made] gets its id once the budget has it. */
internal class SmsNewAccount(val name: String, val made: (String) -> Unit)

/**
 * A new account for texts the budget has nowhere to put (the CD a transfer went to): its name, type
 * and its balance before these texts, which are added on top of it.
 */
@Composable
private fun SmsNewAccountDialog(request: SmsNewAccount, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    var name by remember { mutableStateOf(request.name) }
    var type by remember { mutableStateOf("checking") }
    var balance by remember { mutableStateOf(amountText(0)) }
    var error by remember { mutableStateOf<String?>(null) }
    val types = remember(languageKey()) { accountTypes() }
    FormDialog(stringResource(R.string.sms_new_account_title), close, confirm = stringResource(R.string.common_add), enabled = name.isNotBlank(), onConfirm = {
        runCatching {
            val id = java.util.UUID.randomUUID().toString()
            vm.saveAccount(id, name.trim(), AccountTypes.kindFor(type), amountValue(balance), type)
            // Chosen once the budget has it, so the review checks against an account that's there.
            vm.afterPending { if (vm.state.value.budget?.row("accounts", id) != null) request.made(id) }
            close()
        }.onFailure { error = it.message }
    }) {
        Field(name, { name = it }, stringResource(R.string.account_name))
        Choice(stringResource(R.string.account_type), type, types, { type = it })
        Field(balance, { balance = it }, stringResource(R.string.sms_new_account_balance, budget.text("currency")), numeric = true)
        Text(stringResource(R.string.sms_new_account_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        FormError(error)
    }
}

@Composable
private fun skipSummary(items: List<SmsItem>): String = items.groupingBy { it.skip.ifBlank { "unclear" } }.eachCount().entries.sortedByDescending { it.value }.map { (skip, n) ->
    countText(when (skip) {
        "code" -> R.plurals.sms_skip_code; "security" -> R.plurals.sms_skip_security; "balance" -> R.plurals.sms_skip_balance
        "declined" -> R.plurals.sms_skip_declined; "rewards" -> R.plurals.sms_skip_rewards; "marketing" -> R.plurals.sms_skip_marketing
        "unclear" -> R.plurals.sms_skip_dismissed; else -> R.plurals.sms_skip_other
    }, n)
}.joinToString(" · ")

/** A card or account by the digits the bank gives ("Ending in 4417"), or its words when it has none. */
@Composable
internal fun smsRefLabel(ref: String): String = ref.filter { it in '0'..'9' }.takeIf { it.length >= 3 }?.let { stringResource(R.string.sms_card, it) } ?: Money.own(ref)

/** One choice of account for every message that names a card the phone can't match yet. */
@Composable
private fun SmsCardChoice(group: List<SmsItem>, accounts: List<Pair<String, String>>, keep: Boolean, setKeep: (Boolean) -> Unit, newAccount: () -> Unit, choose: (String) -> Unit) {
    val first = group.first()
    ListCard(Modifier.testTag("sms-card:${first.ref}")) { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.WarningAmber, null, Modifier.size(20.dp), tint = ur.warning)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(smsRefLabel(first.ref!!), style = MaterialTheme.typography.titleMedium, color = ur.text)
                Text(Money.own(first.sender) + " · " + countText(R.plurals.sms_transaction_count, group.size), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
        }
        Text(group.mapNotNull { it.transaction?.text("payee")?.ifBlank { null } }.distinct().take(4).joinToString(", ") { Money.own(it) },
            style = MaterialTheme.typography.bodyMedium, color = ur.text2, maxLines = 2, overflow = TextOverflow.Ellipsis)
        // A card the budget has no account for yet can have one made here.
        val new = stringResource(R.string.sms_new_account)
        Choice(stringResource(R.string.sms_which_account), "", accounts + (NEW_ACCOUNT to new)) { if (it == NEW_ACCOUNT) newAccount() else choose(it) }
        CheckRow(stringResource(R.string.sms_remember_card), keep, setKeep)
    } }
}

/** A proposed transaction: tick to add it, tap to change it, and what's still needed or worth a look. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SmsTransactionRow(item: SmsItem, budget: JSONObject, row: BankReviewRow?, decision: String?, ticked: Boolean?, index: Int, count: Int, showing: Boolean, enabled: Boolean,
                              show: () -> Unit, tick: (Boolean) -> Unit, decide: (String) -> Unit, open: () -> Unit, newAccount: (() -> Unit)? = null, enterAmount: (Long) -> Unit) {
    val t = item.transaction ?: return
    val currency = budget.text("currency")
    val amount = item.amount
    val transfer = t.text("transferAccountId").ifBlank { null }?.let { budget.row("accounts", it)?.text("name") }
    val category = t.text("categoryId").ifBlank { null }?.let { budget.row("categories", it)?.text("name") }
    Column(Modifier.cardSegment(0, 1, cardFill()).testTag("sms-row:${item.index}")) {
        Row(Modifier.padding(start = if (ticked != null) 4.dp else 16.dp, end = 16.dp, top = 10.dp, bottom = 12.dp), verticalAlignment = Alignment.Top) {
            if (ticked != null) Checkbox(ticked, tick, Modifier.testTag("sms-tick:${item.index}"), enabled = enabled, colors = CheckboxDefaults.colors(checkedColor = ur.accent))
            Column(Modifier.weight(1f).padding(top = if (ticked != null) 10.dp else 0.dp).clickable(enabled = item.needs != "amount", role = Role.Button, onClick = open),
                verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(Money.own(t.text("payee")), Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold, color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(amount?.let { money(it, currency) } ?: (t.text("currency") + " " + t.text("writtenAmount")), Modifier.padding(start = 10.dp),
                        style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, color = if ((amount ?: 0) > 0) ur.positive else if (amount == null) ur.muted else ur.text, maxLines = 1)
                }
                val account = budget.row("accounts", t.text("accountId"))?.text("name")
                Text(listOfNotNull(Money.date(t.text("date")), account?.let { Money.own(it) + (transfer?.let { other -> " → " + Money.own(other) } ?: "") }).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                FlowRow(Modifier.padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    when (item.needs) {
                        "account" -> TagChip(stringResource(R.string.sms_needs_account), fill = ur.warningSoft, color = ur.warning)
                        "transfer" -> TagChip(stringResource(R.string.sms_needs_transfer), fill = ur.warningSoft, color = ur.warning)
                    }
                    // Matched, the transaction keeps its own category and details: nothing here to check.
                    val matching = decision?.startsWith("match:") == true
                    val checkCategory = "category" in item.unsure
                    if (t.text("kind") != "transfer" && !matching) TagChip(category ?: stringResource(R.string.sms_uncategorized),
                        fill = if (category == null) (if (checkCategory) ur.warningSoft else ur.zeroSoft) else ur.accentSoft, color = if (category == null) (if (checkCategory) ur.warning else ur.text2) else ur.accent)
                    if (item.disposition == "pending") TagChip(stringResource(R.string.sms_hold))
                    if (item.early && !matching) TagChip(stringResource(R.string.sms_before_start), fill = ur.warningSoft, color = ur.warning)
                    if (!matching) item.unsure.filterNot { it == "category" && category == null }.forEach { field -> TagChip(stringResource(when (field) {
                        "date" -> R.string.sms_check_date; "amount" -> R.string.sms_check_amount; "payee" -> R.string.sms_check_payee
                        "kind" -> R.string.sms_check_kind; else -> R.string.sms_check_category
                    }), fill = ur.warningSoft, color = ur.warning) }
                    if (row?.locked == true) TagChip(stringResource(R.string.sms_already_added))
                    else if (row != null && decision != null && (decision == "review" || row.options.any { it.first.startsWith("match:") })) SmsDecisionChip(row, decision, enabled, decide)
                    TagChip(stringResource(if (showing) R.string.sms_hide_message else R.string.sms_show_message), Modifier.clickable(role = Role.Button, onClick = show), color = ur.accent)
                }
                if (item.needs == "amount") SmsAmountField(currency, t.text("currency") + " " + t.text("writtenAmount"), enterAmount)
                // The other account of a transfer the budget doesn't have yet (a new CD) is made here.
                newAccount?.let { Box(Modifier.padding(top = 6.dp).testTag("sms-new-account:${item.index}")) { SmallAction(stringResource(R.string.sms_new_account), it, primary = true) } }
                if (showing) item.sources.forEach { Text(it.body, Modifier.padding(top = 4.dp), style = SmsBodyStyle, color = ur.text2) }
            }
        }
    }
}

/** Add as new, or which existing transaction it is: the bank review's own choices. */
@Composable
private fun SmsDecisionChip(row: BankReviewRow, decision: String, enabled: Boolean, decide: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        val label = if (decision == "review") stringResource(R.string.sms_add_or_match) else row.options.firstOrNull { it.first == decision }?.second.orEmpty()
        TagChip(label, Modifier.clickable(enabled, role = Role.Button) { open = true }.testTag("sms-decision"), fill = if (decision == "review") ur.warningSoft else ur.zeroSoft,
            color = if (decision == "review") ur.warning else ur.text)
        DropdownMenu(open, { open = false }, shape = androidx.compose.foundation.shape.RoundedCornerShape(14.dp), containerColor = ur.surface) {
            row.options.filter { it.first != "review" && it.first != "skip" }.forEach { (id, name) ->
                DropdownMenuItem(text = { Text(Money.own(name), fontWeight = if (id == decision) FontWeight.SemiBold else FontWeight.Normal) }, onClick = { decide(id); open = false })
            }
        }
    }
}

/** What a purchase in another currency cost in the budget's, typed where it's asked. */
@Composable
private fun SmsAmountField(currency: String, written: String, enter: (Long) -> Unit) {
    var text by remember { mutableStateOf("") }
    val cents = smsCents(text.trim())?.takeIf { it > 0 }
    Row(Modifier.fillMaxWidth().padding(top = 6.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.weight(1f)) { Field(text, { text = it }, stringResource(R.string.sms_needs_amount, currency, written), numeric = true, ltr = true) }
        SmallAction(stringResource(R.string.common_save), { cents?.let(enter) }, primary = cents != null)
    }
}

/** A message the assistant couldn't turn into one transaction, or that isn't one: enter it, or let it go. */
@Composable
private fun SmsUnclearRow(item: SmsItem, index: Int, count: Int, enter: () -> Unit, dismiss: (() -> Unit)?) {
    Column(Modifier.cardSegment(0, 1, cardFill()).testTag("sms-other:${item.index}")) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            // A skipped text is said by its reason; the assistant writes no note for it.
            Text(item.note.ifBlank { stringResource(when (item.skip) {
                "code" -> R.string.sms_skipped_code; "security" -> R.string.sms_skipped_security; "balance" -> R.string.sms_skipped_balance
                "declined" -> R.string.sms_skipped_declined; "rewards" -> R.string.sms_skipped_rewards; "marketing" -> R.string.sms_skipped_marketing
                else -> R.string.sms_skipped_other
            }) }, style = MaterialTheme.typography.bodyLarge.copy(textDirection = androidx.compose.ui.text.style.TextDirection.Content), fontWeight = FontWeight.SemiBold, color = ur.text)
            item.sources.forEach { Text(it.body, style = SmsBodyStyle, color = ur.text2, maxLines = 6, overflow = TextOverflow.Ellipsis) }
            Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SmallAction(stringResource(if (dismiss != null) R.string.sms_enter else R.string.sms_is_transaction), enter, primary = dismiss != null)
                dismiss?.let { SmallAction(stringResource(R.string.sms_dismiss), it) }
            }
        }
    }
}

/** The messages behind a transaction, under its editor, to check it against. */
@Composable
private fun SmsSources(item: SmsItem) {
    ListCard { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.sms_from_messages), style = MaterialTheme.typography.titleSmall, color = ur.text)
        if (item.template != null) Text(stringResource(R.string.sms_from_template), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        item.sources.forEach { source ->
            Text(Money.own(source.sender) + " · " + Money.date(source.receivedDate), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            Text(source.body, style = SmsBodyStyle, color = ur.text2)
        }
    } }
}

/**
 * What the user changed in the editor of a row, among what a template reads (date, amount, payee,
 * category), and the category chosen.
 */
internal fun smsChanged(item: SmsItem, record: JSONObject): Pair<Set<String>, String?> {
    val t = item.transaction ?: return emptySet<String>() to null
    val category = record.rows("splits").singleOrNull()?.takeUnless { it.optBoolean("unassigned") }?.text("categoryId")?.ifBlank { null }
    return buildSet {
        if (record.text("date") != t.text("date")) add("date")
        if (record.optLong("amount") != item.amount) add("amount")
        if (record.text("payee") != t.text("payee")) add("payee")
        if (category != t.text("categoryId").ifBlank { null }) add("category")
    } to category
}

/** The transaction editor's record for an item: its date, accounts, payee, amount and category. */
internal fun smsEditable(item: SmsItem, currency: String): JSONObject {
    val t = item.transaction ?: JSONObject()
    val cents = item.amount ?: 0L
    val transfer = t.text("transferAccountId").ifBlank { null }
    return JSONObject().put("id", "sms-review").put("date", t.text("date").ifBlank { item.sources.first().receivedDate }).put("accountId", t.text("accountId"))
        .put("payee", t.text("payee")).put("memo", t.text("memo")).put("amount", cents).put("status", "uncleared").put("approved", true).put("flag", JSONObject.NULL)
        .put("transferAccountId", transfer ?: JSONObject.NULL).put("transferStatus", if (transfer != null) "uncleared" else JSONObject.NULL)
        .put("splits", JSONArray().put(JSONObject().put("categoryId", t.text("categoryId").ifBlank { null } ?: JSONObject.NULL).put("amount", cents).put("unassigned", false)))
        .put("currency", currency)
}

/** The editor's record back into the item: the user's fields win, and the item becomes a transaction. */
internal fun smsEdited(batch: SmsBatch, item: SmsItem, record: JSONObject, budget: JSONObject): SmsBatch {
    val cents = record.optLong("amount")
    require(cents != 0L) { Words[R.string.sms_amount_needed] }
    val parts = record.rows("splits")
    require(parts.size == 1) { Words[R.string.sms_no_splits] }
    val transfer = record.text("transferAccountId")
    val category = parts[0].takeUnless { it.optBoolean("unassigned") }?.text("categoryId").orEmpty()
    return batch.edit(item.index) { json ->
        val old = json.optJSONObject("transaction")
        val currency = budget.text("currency")
        val payee = record.text("payee").ifBlank { old?.text("payee").orEmpty() }.ifBlank { budget.row("accounts", transfer)?.text("name").orEmpty() }
        require(payee.isNotBlank()) { Words[R.string.sms_payee_needed] }
        val kind = when { transfer.isNotBlank() -> "transfer"; cents < 0 -> "expense"; old?.text("kind") == "refund" -> "refund"; else -> "income" }
        val t = old ?: JSONObject().put("currency", currency).put("accountRef", JSONObject.NULL).put("otherRef", JSONObject.NULL)
        // An amount the message wrote in the budget's own currency is the one the user just typed.
        if (t.text("currency") == currency || !t.has("writtenAmount")) t.put("currency", currency).put("writtenAmount", smsDecimal(kotlin.math.abs(cents)))
        t.put("date", record.text("date")).put("amount", smsDecimal(cents)).put("payee", payee).put("memo", record.text("memo")).put("kind", kind)
            .put("accountId", record.text("accountId")).put("transferAccountId", transfer.ifBlank { null } ?: JSONObject.NULL)
            .put("categoryId", category.takeIf { it.isNotBlank() && transfer.isBlank() } ?: JSONObject.NULL)
        json.put("transaction", t).put("unsure", JSONArray())
        if (json.text("disposition") !in setOf("transaction", "pending")) json.put("disposition", "transaction").put("skip", JSONObject.NULL)
    }.let { edited -> edited.copy(dismissed = edited.dismissed - item.index, ticked = edited.ticked + (item.index to true)) }
}

private suspend fun BudgetViewModel.smsQuery(method: String, params: JSONObject): JSONObject = suspendCancellableCoroutine { waiting ->
    val job = query(method, params, onError = { if (waiting.isActive) waiting.resumeWithException(IllegalStateException(it)) }) { if (waiting.isActive) waiting.resume(it) }
    waiting.invokeOnCancellation { job.cancel() }
}
