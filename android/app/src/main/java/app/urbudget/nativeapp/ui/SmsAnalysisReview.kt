package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CalendarToday
import androidx.compose.material.icons.outlined.MoreHoriz
import androidx.compose.material.icons.outlined.SwapHoriz
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * The prepared batch as transactions to add, not messages to read. Cards and accounts the phone
 * can't match are set up first, once for every message that names them ([smsSetupSection]);
 * amounts in another currency are typed in the row;
 * anything else is changed in the transaction editor. Ticked rows are added in one undoable step,
 * and every message the review settles leaves Bank messages; those still waiting on a choice stay
 * in the review, as they were read. [change] applies an edit to the review as it is when it runs:
 * a row drawn before the last tick must not bring back what that tick changed.
 */
@Composable
internal fun SmsReview(budget: JSONObject, vm: BudgetViewModel, batch: SmsBatch, mappings: JSONArray, close: () -> Unit, change: ((SmsBatch) -> SmsBatch) -> Unit,
                       saveCards: (JSONArray) -> Unit, finish: (List<String>, SmsBatch?, Map<String, JSONObject>) -> Unit, again: (String) -> Unit, discard: () -> Unit,
                       edited: (SmsItem, Set<String>, String, String?) -> Unit = { _, _, _, _ -> }, accuracy: Map<String, Int> = emptyMap()) {
    val state by vm.state.collectAsStateWithLifecycle()
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val accounts = remember(budget) { budget.rows("accounts").filterNot { it.optBoolean("archived") }.map { it.text("id") to it.text("name") } }
    val names = remember(budget) { SmsRowNames.of(budget) }
    val starts = remember(budget, batch.fresh) { batch.starts(budget) }
    // Ticking and dismissing don't change what's previewed; only the analysis and the cards do.
    val items = remember(batch.analysis, batch.messages, mappings, budget, starts) { batch.items(SmsAccounts.of(budget, mappings, starts)) }
    val resolved = remember(items) { batch.resolved(items) }
    // The rows as last checked against the budget. They stay on screen while a change is checked
    // again: one row changed, and the others read as they did, so the list neither blanks nor moves.
    var preview by remember { mutableStateOf<SmsPreviewed?>(null) }
    // The rows that asked add or match. They keep their place once answered, and while the review is prepared again.
    var asks by remember { mutableStateOf(emptySet<Int>()) }
    var loading by remember { mutableStateOf(true) }
    var previewFailed by remember { mutableStateOf(false) }
    var attempt by remember { mutableIntStateOf(0) }
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
    var settingUp by remember { mutableStateOf<String?>(null) }
    var clearing by remember { mutableStateOf(false) }
    val keepCard = remember { mutableStateMapOf<String, Boolean>() }
    LaunchedEffect(budget, resolved, attempt) {
        error = null; previewFailed = false; loading = true
        try {
            val rows = vm.smsPreview(budget, batch, resolved)
            asks = rows.filterValues { it.initial == "review" }.keys
            preview = SmsPreviewed(batch.messages, budget, resolved, rows)
            loading = false
        }
        // Cancelled, a newer check is under way: it says when it's done.
        catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message; previewFailed = true; loading = false }
    }
    LaunchedEffect(state.error) { if (state.error != null) { error = state.error; saving = false } }
    androidx.activity.compose.BackHandler(onBack = close)

    // Rows of another set of texts (more were read into the review) are no rows of these.
    val prepared = preview?.takeIf { it.messages === batch.messages }?.rows
    // Checked against the budget and the review as they are now: until then nothing can be added.
    val checked = preview?.let { it.budget === state.budget && it.resolved === resolved && it.messages === batch.messages } == true
    val known = remember(prepared, items, budget) { smsKnown(prepared, items, budget) }
    // Read through state, so a row the list didn't redraw still shows the review as it is now.
    val latest = rememberUpdatedState(batch)
    val changeNow = rememberUpdatedState(change)
    // A choice made before an edit holds while the row still offers it. Kept with the review, so Later doesn't lose it.
    fun decision(index: Int, row: BankReviewRow) = smsDecision(row, latest.value.decisions[index], known[index])
    fun ticked(item: SmsItem) = item.tickedIn(latest.value)
    fun tickAll(rows: List<SmsItem>, on: Boolean) = change { b -> b.copy(ticked = b.ticked + rows.filter { it.needs == null }.associate { it.index to on }) }
    /** Ticks what's dated [day] or later and unticks the rest: a first import that starts where the budget does. */
    fun tickFrom(rows: List<SmsItem>, day: String) = change { b -> b.copy(ticked = b.ticked + rows.filter { it.needs == null }.associate { it.index to (it.transaction?.text("date").orEmpty() >= day) }) }
    val sorted = remember(items, known, batch.dismissed) { smsSort(batch, items, known) }
    val (waiting, ready, existing, holds, skipped) = sorted
    // Newest first, as the bank's app lists them, under the day they're dated; what asks comes before them all, since Add waits for it.
    val (asking, readyShown) = remember(ready, asks) { smsNewestFirst(ready).partition { it.index in asks } }
    val holdsShown = remember(holds) { smsNewestFirst(holds) }
    val askingRows = remember(asking) { listOf("" to asking) }
    val readyDays = remember(readyShown) { smsByDay(readyShown) }
    val holdDays = remember(holdsShown) { smsByDay(holdsShown) }
    val existingRows = remember(existing) { listOf("" to existing) }
    // The cards and accounts to set up, and what's suggested for each; the rows they answer wait under them.
    val typeNames = remember(languageKey()) { accountTypes().toMap() }
    val candidates = remember(items, batch.dismissed) { smsSetupCandidates(batch, items) }
    val suggested = remember(candidates, budget, typeNames) { smsSetupSuggestions(candidates, budget) { typeNames[it].orEmpty() } }
    val plans = remember(candidates, suggested, batch.setup, accounts) { smsSetupPlans(candidates, batch.setup, suggested, accounts.map { it.first }.toSet()) }
    val inSetup = remember(candidates) { candidates.flatMap { c -> (c.items + c.via).map { it.index } }.toSet() }
    val choosing = remember(waiting, inSetup) { waiting.filter { it.index !in inSetup } }
    // With nothing ready to add, setting up is the next step: its button takes the page's place.
    val setupNext = candidates.isNotEmpty() && ready.isEmpty()
    val transfers = remember(choosing) { choosing.filter { it.transfer != null }.groupBy { it.transfer!! } }
    val singles = remember(choosing) { choosing.filter { it.transfer == null } }
    val adding = remember(prepared, sorted, batch.ticked, batch.decisions, known) {
        prepared?.let { rows -> (ready + existing + holds).filter { it.needs == null && it.tickedIn(batch) }.mapNotNull { item ->
            rows[item.index]?.takeIf { !it.locked }?.let { row -> item.index to smsDecision(row, batch.decisions[item.index], known[item.index]) }?.takeIf { it.second != "skip" }
        } }.orEmpty()
    }
    val undecided = adding.count { it.second == "review" }
    val matching = adding.count { it.second.startsWith("match:") }
    // Settled: added, unticked, held, not a transaction, or dismissed. Waiting for a choice: kept for later.
    val settled = remember(items, sorted, batch.ticked) {
        val waits = waiting.mapTo(HashSet()) { it.index }
        items.filter { it.index !in waits && !(it.disposition == "pending" && it.needs != null && it.tickedIn(batch)) }
    }
    val later = items.size - settled.size
    // Texts the tap settles without adding anything: they leave Bank messages and can't be collected again.
    val cleared = settled.size - adding.size
    // Transactions the user unticked: the tap that adds the others drops these for good.
    val dropped = remember(ready, batch.ticked) { ready.count { batch.ticked[it.index] == false } }
    val balances = remember(items, adding, budget, state.summary, batch.ticked) {
        smsBalanceRows(smsBalanceChanges(items, adding), budget, state.summary) { account, now -> smsBankCheck(account, budget, now, items, adding) { it.tickedIn(batch) } }
    }
    fun confirm() {
        if (!checked || state.budget !== vm.state.value.budget || undecided > 0 || saving) return
        val handled = settled.flatMap { item -> item.sources.map { it.id } }
        // What still waits on a choice stays in the review as it was read, not read again.
        val done = settled.mapTo(HashSet()) { it.index }
        val rest = batch.only(items.filter { it.index !in done }.map { it.index })
        // What became of each text, kept once it leaves Bank messages: for the assistant, and the bank's balances.
        val outcomes = smsOutcomes(settled, adding, batch.dismissed)
        if (adding.isEmpty()) { finish(handled, rest, outcomes); return }
        saving = true
        val choices = smsDecisions(adding)
        val count = adding.count { it.second == "new" }.toLong()
        val failures = vm.operationFailureEpoch
        vm.runCommand("smsBatchApply", batch.params(resolved).put("decisions", choices)) { if (count > 0) toast(Words.count(R.plurals.sms_added, count), false); finish(handled, rest, outcomes) }
        // It failed, whatever it said: the same message twice changes nothing to see, and Add must come back.
        vm.afterPending { if (vm.operationFailureEpoch != failures) { saving = false; error = vm.state.value.error ?: error } }
    }
    /** An account made for these texts: its balance is from before them, so none is held back as already in it. */
    fun made(account: String) = change { it.copy(fresh = it.fresh + account) }
    val mappingsNow = rememberUpdatedState(mappings)
    /**
     * The setup as it stands: the accounts made in one change, then, once the budget has them, each
     * card remembered with its account and transfers to an untracked one made spending or income.
     */
    fun applySetup() {
        val changes = runCatching { smsSetupChanges(candidates, plans, ::ticked) { java.util.UUID.randomUUID().toString() } }.getOrElse { error = it.message; return }
        val ids = changes.accounts.map { it.text("id") }
        val answered = candidates.map { it.key }.toSet()
        if (ids.isNotEmpty()) vm.saveAccounts(changes.accounts)
        vm.afterPending {
            if (ids.any { vm.state.value.budget?.row("accounts", it) == null }) return@afterPending
            runCatching { changes.remember.fold(mappingsNow.value) { kept, r -> smsRememberCard(kept, r.sender, r.ref, r.account, r.kind) } }
                .onSuccess(saveCards).onFailure { error = it.message }
            change { b -> changes.spending.fold(b.copy(fresh = b.fresh + ids, setup = b.setup - answered)) { next, index -> next.edit(index, ::smsUntracked) } }
        }
    }
    /**
     * The other account of these transfers, for each of them; and, unless the user said not to,
     * remembered by what names them ([smsOtherKeys]), so the sender's next texts find it.
     */
    fun chooseOther(group: List<SmsItem>, account: String) {
        val first = group.first()
        val keys = smsOtherKeys(group)
        if (keepCard[first.transfer] != false && keys.isNotEmpty()) runCatching {
            keys.fold(mappings) { kept, (ref, kind) -> smsRememberCard(kept, first.sender, ref, account, kind) }
        }.onSuccess(saveCards).onFailure { error = it.message }
        change { b -> group.fold(b) { next, item -> next.edit(item.index) { it.getJSONObject("transaction").put("transferAccountId", account) } } }
    }
    /** The other account of these transfers, made here: a term deposit the money went to. */
    fun newForTransfer(group: List<SmsItem>) {
        val other = group.map { it.transaction?.text("otherRef").orEmpty() }.distinct().singleOrNull()?.ifBlank { null }
        creating = SmsNewAccount(other?.let { smsAccountName(group.first().sender, it) }.orEmpty()) { id -> made(id); chooseOther(group, id) }
    }
    // What a row's taps do: one object for every row and every redraw, so a row whose own values
    // didn't change isn't drawn again when another is ticked. Each reads the review as it is when tapped.
    val actions = remember {
        SmsRowActions(
            show = { shown = shown.toggle(it) },
            tick = { item, on -> changeNow.value { it.copy(ticked = it.ticked + (item.index to on)) } },
            decide = { index, d -> changeNow.value { it.copy(decisions = it.decisions + (index to d)) } },
            open = { editing = it },
            enterAmount = { item, cents ->
                val out = item.transaction?.text("kind") in setOf("expense", "transfer")
                changeNow.value { it.edit(item.index) { json -> json.getJSONObject("transaction").put("amount", smsDecimal(if (out) -cents else cents)) } }
            },
            dismiss = { index -> changeNow.value { it.copy(dismissed = it.dismissed + index) } },
        )
    }

    val askingTitle = stringResource(R.string.sms_section_check)
    val askingDetail = stringResource(R.string.sms_section_check_detail)
    val readyTitle = stringResource(R.string.sms_section_ready)
    val holdsTitle = stringResource(R.string.sms_section_holds)
    val holdsDetail = stringResource(R.string.sms_holds_detail)
    val knownDetail = stringResource(R.string.sms_known_detail)
    val knownTitle = countText(R.plurals.sms_known, existing.size)
    val skippedTitle = countText(R.plurals.sms_not_transactions, skipped.size)
    Column(Modifier.fillMaxSize().testTag("sms-review-page")) {
        // Checking against the budget shows above the list, in room that's always there: no row moves for it.
        Box(Modifier.fillMaxWidth().height(4.dp)) { if (loading) LinearProgressIndicator(Modifier.fillMaxWidth().testTag("sms-review-checking"), color = ur.accent) }
        LazyColumn(Modifier.weight(1f).fillMaxWidth().testTag("sms-review-list"), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 0.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            item(key = "head") {
                SectionTitle(stringResource(R.string.sms_review_title), countText(R.plurals.sms_message_count, batch.messages.size)) {
                    Box {
                        IconButton({ menu = true }, enabled = !saving) { Icon(Icons.Outlined.MoreHoriz, stringResource(R.string.common_more_actions), tint = ur.text) }
                        DropdownMenu(menu, { menu = false }, shape = RoundedCornerShape(14.dp), containerColor = ur.surface) {
                            DropdownMenuItem(text = { Text(stringResource(R.string.sms_prepare_again)) }, onClick = { menu = false; noting = true })
                            DropdownMenuItem(text = { Text(stringResource(R.string.sms_discard_review), color = ur.negative) }, onClick = { menu = false; discard() })
                        }
                    }
                }
            }
            error?.let { message ->
                item(key = "error") {
                    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        FormError(message)
                        // The check against the budget failed, so nothing can be added: it's tried again from here.
                        if (previewFailed) TextButton({ attempt++ }, Modifier.align(Alignment.End).testTag("sms-review-retry")) {
                            Text(stringResource(R.string.assistant_try_again), color = ur.accent, fontWeight = FontWeight.SemiBold)
                        }
                    }
                }
            }
            // With rows ready to add, Add is the page's button and setting up is the list's, a quieter one.
            if (candidates.isNotEmpty()) smsSetupSection(candidates, plans, budget, accounts, typeNames, !saving, ::ticked,
                { key, plan -> change { it.copy(setup = it.setup + (key to plan)) } }, { settingUp = it }, if (setupNext) null else ::applySetup)
            rowSection("asking", askingTitle, askingDetail, asking, askingRows, names, prepared, ::decision, ::ticked, shown, !saving, actions,
                tickAll = { tickAll(asking, it) }, fromDay = { fromDay = ready })
            if (choosing.isNotEmpty()) {
                item(key = "choose") { SectionTitle(stringResource(R.string.sms_section_choose), stringResource(R.string.sms_section_choose_detail)) }
                transfers.forEach { (key, group) ->
                    item(key = "transfer:$key") {
                        // Remembered for what names these transfers, when anything does.
                        val keep = (keepCard[key] != false).takeIf { smsOtherKeys(group).isNotEmpty() }
                        SmsTransferChoice(group, budget, accounts, keep, { keepCard[key] = it }, { editing = it }, { newForTransfer(group) },
                            { change { b -> b.copy(dismissed = b.dismissed + group.map { it.index }) } }) { chooseOther(group, it) }
                    }
                }
                itemsIndexed(singles, key = { _, it -> "single:${it.index}" }) { i, item ->
                    if (item.transaction == null) SmsUnclearRow(item, i, singles.size, { editing = item.index }) { actions.dismiss(item.index) }
                    else SmsTransactionRow(item, names, null, null, null, item.index in shown, !saving, actions, canDismiss = true)
                }
            }
            rowSection("ready", readyTitle, null, readyShown, readyDays, names, prepared, ::decision, ::ticked, shown, !saving, actions,
                tickAll = { tickAll(ready, it) }, fromDay = { fromDay = ready })
            rowSection("holds", holdsTitle, holdsDetail, holdsShown, holdDays, names, prepared, ::decision, ::ticked, shown, !saving, actions,
                tickAll = { tickAll(holds, it) }, fromDay = { fromDay = holds })
            // What changes nothing in the budget is handled without a question, and open to look at.
            if (existing.isNotEmpty() || skipped.isNotEmpty()) {
                item(key = "handled") {
                    Column(Modifier.padding(top = 4.dp)) { ListCard(Modifier.testTag("sms-handled")) {
                        ListRow(countText(R.plurals.sms_handled, existing.size + skipped.size), subtitle = handledSummary(existing.size, skipped), onClick = { others = !others }, chevron = false,
                            trailing = { Text(stringResource(if (others) R.string.sms_hide else R.string.sms_show), color = ur.accent, fontWeight = FontWeight.SemiBold) })
                    } }
                }
                if (others && existing.isNotEmpty()) rowSection("known", knownTitle, knownDetail, existing, existingRows, names, prepared, ::decision, ::ticked, shown, !saving, actions)
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
            if (balances.first.isNotEmpty()) SmsBalances(balances.first, balances.second, currency)
            val caption = listOfNotNull(
                if (undecided > 0) countText(R.plurals.sms_undecided, undecided) else null,
                if (matching > 0) countText(R.plurals.sms_matches, matching) else null,
                if (cleared > 0) countText(R.plurals.sms_clears, cleared) else null,
                if (later > 0) countText(R.plurals.sms_later, later) else null,
            ).joinToString(" · ")
            // Unticked transactions leave with the ones added: said in the colour of a warning, not a footnote.
            if (caption.isNotEmpty()) Text(caption, Modifier.fillMaxWidth().testTag("sms-caption"), style = MaterialTheme.typography.bodySmall,
                color = if (undecided > 0 || (dropped > 0 && adding.isNotEmpty())) ur.warning else ur.muted, textAlign = TextAlign.Center)
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                SecondaryButton(stringResource(R.string.sms_later_action), close, Modifier.weight(1f), enabled = !saving)
                val adds = adding.size > matching
                val action = when {
                    adds -> countText(R.plurals.sms_add, adding.size - matching)
                    settled.isNotEmpty() -> countText(R.plurals.sms_clear, settled.size)
                    else -> stringResource(R.string.sms_nothing)
                }
                // A tap that adds nothing only takes texts out, for good: it always asks first, and isn't
                // drawn as the page's main action. One that adds asks when it takes a lot out with it.
                val asksFirst = cleared >= SmsClearAsks || (!adds && cleared > 0)
                val enabled = !saving && checked && undecided == 0 && settled.isNotEmpty()
                val tap = { if (asksFirst) clearing = true else confirm() }
                when {
                    setupNext -> PrimaryButton(smsSetupAction(plans), ::applySetup, Modifier.weight(2f).testTag("sms-setup-apply"), enabled = !saving)
                    adds -> PrimaryButton(action, tap, Modifier.weight(2f).testTag("sms-add"), enabled = enabled)
                    else -> SecondaryButton(action, tap, Modifier.weight(2f).testTag("sms-add"), enabled = enabled)
                }
                if (clearing) AlertDialog(onDismissRequest = { clearing = false }, title = { Text(countText(R.plurals.sms_clear_title, cleared)) },
                    text = { Text(listOfNotNull(countText(R.plurals.sms_clear_detail, cleared), later.takeIf { it > 0 }?.let { countText(R.plurals.sms_later, it) + "." })
                        .joinToString(" "), Modifier.testTag("sms-clear-detail")) },
                    confirmButton = { TextButton({ clearing = false; confirm() }, Modifier.testTag("sms-clear-confirm")) { Text(action) } },
                    dismissButton = { TextButton({ clearing = false }) { Text(stringResource(R.string.common_cancel)) } })
            }
        }
    }
    fromDay?.let { rows ->
        DatePickerSheet(java.time.LocalDate.now().toString(), { day -> tickFrom(rows, day); fromDay = null }) { fromDay = null }
    }
    creating?.let { request -> SmsNewAccountDialog(request, budget, vm) { creating = null } }
    settingUp?.let { key -> candidates.firstOrNull { it.key == key } }?.let { c ->
        val plan = plans.getValue(c.key)
        SmsSetupDialog(plan, smsSetupBalance(plan, smsOpening(smsSetupMembers(c.key, candidates, plans), ::ticked, smsPlanLimit(plan))), currency, { settingUp = null }) { answer ->
            change { it.copy(setup = it.setup + (c.key to answer)) }
        }
    }
    editing?.let { index -> items.firstOrNull { it.index == index } }?.let { item ->
        TransactionDialog(smsEditable(item, currency), budget, "", vm, close = { editing = null }, dialogTitle = stringResource(R.string.sms_check_title), saveOverride = { record ->
            val (fields, category) = smsChanged(item, record)
            // The other rows with the bank's name for this payee get the user's name and category for it too.
            change { smsSameName(smsEdited(it, item, record, budget), item, fields, record.text("payee"), category) }
            // Counted against the template that read it, if one did, and its payee learned for the next texts.
            if (fields.isNotEmpty()) edited(item, fields, record.text("payee"), category)
            // Money to or from a person the user made a transfer: the bank's name for their own account at another bank.
            smsOwnName(item, record)?.let { name -> runCatching { saveCards(smsRememberCard(mappings, item.sender, name, record.text("transferAccountId"), SmsForOwn)) } }
        }, extraFields = { SmsSources(item, item.template?.let { accuracy[it] }) })
    }
    if (noting) {
        var note by remember { mutableStateOf("") }
        FormDialog(stringResource(R.string.sms_prepare_again), { noting = false }, confirm = stringResource(R.string.sms_prepare_again_action), onConfirm = { noting = false; again(note.trim()) }) {
            Text(stringResource(R.string.sms_note_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            Field(note, { if (it.length <= 2000) note = it }, stringResource(R.string.sms_note_label), lines = 4)
        }
    }
}

/** The review's rows as last checked against the budget: for which texts, budget and resolved analysis. */
private class SmsPreviewed(val messages: List<BankSms>, val budget: JSONObject, val resolved: JSONObject, val rows: Map<Int, BankReviewRow>)

/** A row's decision: the user's answer while the row still offers it, else what the phone is sure of, else the row's own. */
internal fun smsDecision(row: BankReviewRow, answer: String?, known: String?): String =
    row.decision(answer?.takeIf { d -> row.options.any { it.first == d } } ?: known)

/** What a row shows of the budget, worked out once for all of them: its currency, and accounts and categories by id. */
@Immutable
internal class SmsRowNames(val currency: String, val accounts: Map<String, String>, val categories: Map<String, String>) {
    companion object {
        fun of(budget: JSONObject) = SmsRowNames(budget.text("currency"), budget.rows("accounts").associate { it.text("id") to it.text("name") },
            budget.rows("categories").associate { it.text("id") to it.text("name") })
    }
}

/** What a row's taps do, the same for every row: by the item's index, or the item itself. */
@Stable
internal class SmsRowActions(val show: (Int) -> Unit, val tick: (SmsItem, Boolean) -> Unit, val decide: (Int, String) -> Unit, val open: (Int) -> Unit,
                             val enterAmount: (SmsItem, Long) -> Unit, val dismiss: (Int) -> Unit)

/** Rows under the day they're dated, in the order they come. */
internal fun smsByDay(rows: List<SmsItem>): List<Pair<String, List<SmsItem>>> = rows.groupBy { it.transaction?.text("date").orEmpty() }.toList()

/** The choice that makes a new account, beside the budget's own. */
private const val NEW_ACCOUNT = "\u0000new"

private fun Set<Int>.toggle(value: Int) = if (value in this) this - value else this + value

/** An account's balance now and after adding, with the bank's own figure to check it against when a text gives one. */
internal class SmsBalanceRow(val account: String, val name: String, val before: Long, val after: Long, val check: SmsBankCheck?)

/** The first three accounts the ticked rows change, each with its balance now and after, and how many more there are. */
internal fun smsBalanceRows(changes: Map<String, Long>, budget: JSONObject, summary: JSONObject?, bank: (String, Long) -> SmsBankCheck?): Pair<List<SmsBalanceRow>, Int> {
    val now = summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty()
    val rows = changes.entries.take(3).map { (account, change) ->
        val record = budget.row("accounts", account)
        val before = now[account] ?: record?.optLong("openingBalance") ?: 0L
        SmsBalanceRow(account, record?.text("name").orEmpty(), before, before + change, bank(account, before))
    }
    return rows to changes.size - rows.size
}

/**
 * Each account's balance now and after adding ("ANB 3358  5,000.00 → −13,506.45"): a month of texts
 * added on top of a balance entered today shows here before it's added. A balance that would go
 * below zero from zero or more is in the warning colour. The rows are worked out when what's added
 * changes ([smsBalanceRows]), not each time the page draws.
 */
@Composable
private fun SmsBalances(rows: List<SmsBalanceRow>, more: Int, currency: String) {
    Column(Modifier.fillMaxWidth().testTag("sms-balances"), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(stringResource(R.string.sms_balances_title), style = MaterialTheme.typography.labelMedium, color = ur.muted)
        rows.forEach { row ->
            Row(Modifier.fillMaxWidth().testTag("sms-balance:${row.account}"), verticalAlignment = Alignment.CenterVertically) {
                Text(Money.own(row.name), Modifier.weight(1f).padding(end = 10.dp), style = MaterialTheme.typography.bodySmall,
                    color = ur.text2, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(stringResource(R.string.sms_balance_change, money(row.before, currency), money(row.after, currency)), style = MaterialTheme.typography.bodySmall.merge(Tabular),
                    fontWeight = FontWeight.SemiBold, color = if (row.after < 0 && row.before >= 0) ur.warning else ur.text, maxLines = 1)
            }
            // The bank's own figure, from the latest text in the review that gives one, against the budget at that text.
            row.check?.let { check ->
                val figure = money(check.bank, currency)
                val day = Money.date(check.date)
                val (words, tone) = when {
                    check.difference == 0L -> stringResource(R.string.sms_bank_matches, figure, day) to ur.positive
                    check.held != 0L && check.difference == check.held -> stringResource(R.string.sms_bank_held, figure, day, money(kotlin.math.abs(check.held), currency)) to ur.muted
                    else -> stringResource(R.string.sms_bank_differs, figure, day, money(kotlin.math.abs(check.difference), currency)) to ur.warning
                }
                Text(words, Modifier.fillMaxWidth().testTag("sms-bank-check:${row.account}"), style = MaterialTheme.typography.bodySmall, color = tone, maxLines = 2)
            }
        }
        if (more > 0) Text(countText(R.plurals.sms_balance_more, more), style = MaterialTheme.typography.bodySmall, color = ur.muted)
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
    // Dismissed: a text that isn't one transaction, or one the user won't add, waits for nothing.
    fun dismissed(item: SmsItem) = item.index in batch.dismissed && item.disposition in setOf("transaction", "unclear")
    val ready = items.filter { it.disposition == "transaction" && it.needs == null && !dismissed(it) }
    return SmsSorted(
        items.filter { !dismissed(it) && ((it.disposition == "transaction" && it.needs != null) || it.disposition == "unclear") },
        ready.filter { it.index !in known }, ready.filter { it.index in known }, items.filter { it.disposition == "pending" },
        items.filter { it.disposition == "ignore" || dismissed(it) })
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
    val claimed = mutableMapOf<String, Set<String>>()
    val sure = open.mapNotNull { (index, matches) ->
        val t = texts[index]?.transaction ?: return@mapNotNull null
        val taken = claimed.getOrPut(t.text("accountId")) { smsBankIds(t.text("accountId"), imported) }
        matches.filter { match -> saved[match.removePrefix("match:")]?.let { smsSameTransaction(t, it, taken) } == true }.singleOrNull()?.let { index to it }
    }
    val shared = sure.groupingBy { it.second }.eachCount()
    return sure.filter { shared[it.second] == 1 }.toMap()
}

/** The bank review's rows for the review's items that can be added, by item. Comes back on the main thread: callers set Compose state with them. */
internal suspend fun BudgetViewModel.smsPreview(budget: JSONObject, batch: SmsBatch, resolved: JSONObject): Map<Int, BankReviewRow> = withContext(Dispatchers.Main.immediate) {
    // A review can hold a phone's whole history: what's asked is put together off the main thread.
    val params = withContext(Dispatchers.Default) { batch.params(resolved) }
    val raw = smsQuery("smsBatchPreview", params).rows("items")
    val rows = withContext(Dispatchers.Default) { prepareBankReview(raw.map { it.getJSONObject("row") }, budget, false) }
    raw.mapIndexed { i, item -> item.getInt("index") to rows[i] }.toMap()
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
 * within ten days the bank review offers: in the same account on the same day, or the same
 * transfer entered from its other account's side (the card payment entered from the current
 * account). Its payee may be the user's own name for it (the restaurant behind a HungerStation
 * order). On another day it may be last week's identical coffee, and one another text already
 * matched (`claimed`, the engine's ids for the texts imported into that account) is a second
 * purchase, so neither is.
 */
internal fun smsSameTransaction(text: JSONObject, there: JSONObject, claimed: Set<String>): Boolean {
    val ids = there.optJSONArray("bankImportIds")
    val account = text.text("accountId")
    val other = text.text("transferAccountId")
    val same = there.text("accountId") == account || (other.isNotBlank() && there.text("accountId") == other && there.text("transferAccountId") == account)
    return same && there.text("date") == text.text("date") && (0 until (ids?.length() ?: 0)).none { ids!!.optString(it) in claimed }
}

/** The ids the bank import gives texts it took into `account`: "bank:" and the SHA-256 of "account|fit:sms:…". */
internal fun smsBankIds(account: String, smsIds: List<String>): Set<String> {
    val digest = java.security.MessageDigest.getInstance("SHA-256")
    return smsIds.mapTo(HashSet()) { id -> "bank:" + digest.digest("$account|fit:$id".toByteArray()).joinToString("") { "%02x".format(it) } }
}

/**
 * A section of rows to tick, under the days they're dated when [groups] has them (the order is the
 * caller's, newest first). Each row gets only its own values and the shared [names] and [actions],
 * so ticking one doesn't draw the others again.
 */
private fun LazyListScope.rowSection(key: String, title: String?, detail: String?, rows: List<SmsItem>, groups: List<Pair<String, List<SmsItem>>>, names: SmsRowNames,
                                     prepared: Map<Int, BankReviewRow>?, decision: (Int, BankReviewRow) -> String, ticked: (SmsItem) -> Boolean, shown: Set<Int>, enabled: Boolean,
                                     actions: SmsRowActions, tickAll: ((Boolean) -> Unit)? = null, fromDay: (() -> Unit)? = null) {
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
    groups.forEach { (day, group) ->
        if (day.isNotBlank()) item(key = "$key-day:$day") {
            Text(Money.joined(listOf(Money.date(day), Words.count(R.plurals.sms_transaction_count, group.size.toLong()))), Modifier.padding(top = 2.dp).testTag("sms-day:$day"),
                style = MaterialTheme.typography.labelLarge, color = ur.muted)
        }
        itemsIndexed(group, key = { _, it -> "$key:${it.index}" }) { _, item ->
            val row = prepared?.get(item.index)
            SmsTransactionRow(item, names, row, row?.let { decision(item.index, it) }, if (item.needs == null) ticked(item) && row?.locked != true else null,
                item.index in shown, enabled && row != null && !row.locked, actions)
        }
    }
}

/** Newest first, as a bank lists them; a day's texts in the order they came, latest first. */
internal fun smsNewestFirst(items: List<SmsItem>): List<SmsItem> =
    items.sortedWith(compareByDescending<SmsItem> { it.transaction?.text("date").orEmpty() }.thenByDescending { it.index })

/** A name for an account a text names that the budget doesn't have yet: "ANB 0070". */
internal fun smsAccountName(sender: String, ref: String): String =
    listOf(smsSenderName(sender), smsDigits(ref).ifEmpty { ref.trim() }).filter { it.isNotBlank() }.joinToString(" ")

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
        "card" -> R.plurals.sms_skip_card; "pair" -> R.plurals.sms_skip_pair; "unclear" -> R.plurals.sms_skip_dismissed; else -> R.plurals.sms_skip_other
    }, n)
}.joinToString(" · ")

/** A card or account by the digits the bank gives ("Ending in 4417"), or its words when it has none. */
@Composable
internal fun smsRefLabel(ref: String): String = if (ref == SmsAnyCard) stringResource(R.string.sms_setup_no_card)
    else smsDigits(ref).takeIf { it.length >= 3 }?.let { stringResource(R.string.sms_card, it) } ?: Money.own(ref)

/**
 * Transfers that need their other account, answered together: those naming one the phone can't
 * match, or a card's that name none (its payments). Each opens to change it; [keep] (null when
 * nothing names them) remembers the choice for the sender's next texts.
 */
@Composable
private fun SmsTransferChoice(group: List<SmsItem>, budget: JSONObject, accounts: List<Pair<String, String>>, keep: Boolean?, setKeep: (Boolean) -> Unit, open: (Int) -> Unit,
                              newAccount: () -> Unit, skip: () -> Unit, choose: (String) -> Unit) {
    val first = group.first()
    val t = first.transaction ?: return
    val currency = budget.text("currency")
    val here = t.text("accountId")
    ListCard(Modifier.testTag("sms-transfer:${first.index}")) { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.SwapHoriz, null, Modifier.size(20.dp), tint = ur.warning)
            Spacer(Modifier.width(10.dp))
            Column(Modifier.weight(1f)) {
                Text(first.note.ifBlank { Money.own(t.text("payee")) }, style = MaterialTheme.typography.titleMedium.copy(textDirection = androidx.compose.ui.text.style.TextDirection.Content),
                    color = ur.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                // The other account's digits when they all give the same; a deposit numbered afresh each time shows in its rows' messages.
                val other = group.map { it.transaction?.text("otherRef").orEmpty() }.distinct().singleOrNull()?.ifBlank { null }
                Text(listOfNotNull(budget.row("accounts", here)?.text("name")?.let { Money.own(it) }, other?.let { smsRefLabel(it) },
                    countText(R.plurals.sms_transaction_count, group.size)).joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
        }
        // What moved, and when: each opens to check or change it.
        group.take(4).forEach { item ->
            Text(listOfNotNull(Money.date(item.transaction?.text("date").orEmpty()), item.amount?.let { money(it, currency) }).joinToString(" · "),
                Modifier.fillMaxWidth().clickable(role = Role.Button) { open(item.index) }.testTag("sms-transfer-row:${item.index}"),
                style = MaterialTheme.typography.bodyMedium.merge(Tabular), color = ur.text2)
        }
        if (group.size > 4) Text("+" + countText(R.plurals.sms_transaction_count, group.size - 4), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        val new = stringResource(R.string.sms_new_account)
        Choice(stringResource(R.string.sms_which_other), "", accounts.filter { it.first != here } + (NEW_ACCOUNT to new) + (SmsSkipCard to stringResource(R.string.sms_card_skip))) {
            when (it) { NEW_ACCOUNT -> newAccount(); SmsSkipCard -> skip(); else -> choose(it) }
        }
        keep?.let { CheckRow(stringResource(R.string.sms_remember_other), it, setKeep) }
    } }
}

/**
 * A proposed transaction: tick to add it, tap to change it, and what's still needed or worth a look.
 * One still needing a choice can be dismissed instead ([canDismiss]): not added. Its inputs are its
 * own values and what every row shares, so it's drawn again only when one of its own changes.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SmsTransactionRow(item: SmsItem, names: SmsRowNames, row: BankReviewRow?, decision: String?, ticked: Boolean?, showing: Boolean, enabled: Boolean,
                              actions: SmsRowActions, canDismiss: Boolean = false) {
    val t = item.transaction ?: return
    val currency = names.currency
    val amount = item.amount
    val transfer = t.text("transferAccountId").ifBlank { null }?.let { names.accounts[it] }
    val category = t.text("categoryId").ifBlank { null }?.let { names.categories[it] }
    Column(Modifier.cardSegment(0, 1, cardFill()).testTag("sms-row:${item.index}")) {
        Row(Modifier.padding(start = if (ticked != null) 4.dp else 16.dp, end = 16.dp, top = 10.dp, bottom = 12.dp), verticalAlignment = Alignment.Top) {
            if (ticked != null) {
                // Said aloud as what it adds: a bare tick names nothing.
                val label = stringResource(R.string.sms_tick_label, Money.own(t.text("payee")))
                Checkbox(ticked, { actions.tick(item, it) }, Modifier.testTag("sms-tick:${item.index}").semantics { contentDescription = label }, enabled = enabled,
                    colors = CheckboxDefaults.colors(checkedColor = ur.accent))
            }
            Column(Modifier.weight(1f).padding(top = if (ticked != null) 10.dp else 0.dp).clickable(enabled = item.needs != "amount", role = Role.Button) { actions.open(item.index) },
                verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(Money.own(t.text("payee")), Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold, color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(amount?.let { money(it, currency) } ?: (t.text("currency") + " " + t.text("writtenAmount")), Modifier.padding(start = 10.dp),
                        style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, color = if ((amount ?: 0) > 0) ur.positive else if (amount == null) ur.muted else ur.text, maxLines = 1)
                }
                val account = names.accounts[t.text("accountId")]
                // Which way the money went: a card payment comes from the current account into the card. The
                // arrow is the language's, so it points the way the line reads.
                val moved = account?.let { here -> transfer?.let { other ->
                    if ((amount ?: 0) > 0) stringResource(R.string.sms_transfer_way, Money.own(other), Money.own(here)) else stringResource(R.string.sms_transfer_way, Money.own(here), Money.own(other))
                } ?: Money.own(here) }
                Text(listOfNotNull(Money.date(t.text("date")), moved).joinToString(" · "), Modifier.testTag("sms-accounts:${item.index}"),
                    style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                FlowRow(Modifier.padding(top = 2.dp), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    // Unticked, it leaves with the review: say so.
                    if (ticked == false && row?.locked != true) TagChip(stringResource(R.string.sms_wont_add), Modifier.testTag("sms-wont-add:${item.index}"))
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
                    // Found from where such money usually goes, or from what the text says the account is: worth a look.
                    if (item.otherGuessed && !matching) TagChip(stringResource(R.string.sms_check_other), Modifier.testTag("sms-check-other:${item.index}"), fill = ur.warningSoft, color = ur.warning)
                    if (!matching) item.unsure.filterNot { it == "category" && category == null }.forEach { field -> TagChip(stringResource(when (field) {
                        "date" -> R.string.sms_check_date; "amount" -> R.string.sms_check_amount; "payee" -> R.string.sms_check_payee
                        "kind" -> R.string.sms_check_kind; else -> R.string.sms_check_category
                    }), fill = ur.warningSoft, color = ur.warning) }
                    if (row?.locked == true) TagChip(stringResource(R.string.sms_already_added))
                    TagChip(stringResource(if (showing) R.string.sms_hide_message else R.string.sms_show_message), Modifier.clickable(role = Role.Button) { actions.show(item.index) }, color = ur.accent)
                }
                // Unticked, there's nothing to ask: it won't be added either way.
                if (row != null && !row.locked && ticked != false && decision != null && (decision == "review" || row.options.any { it.first.startsWith("match:") }))
                    SmsDecision(row, decision, enabled) { actions.decide(item.index, it) }
                if (item.needs == "amount") SmsAmountField(currency, t.text("currency") + " " + t.text("writtenAmount")) { actions.enterAmount(item, it) }
                if (canDismiss) Box(Modifier.padding(top = 6.dp).testTag("sms-dont-add:${item.index}")) { SmallAction(stringResource(R.string.sms_dont_add), { actions.dismiss(item.index) }) }
                if (showing) item.sources.forEach { Text(it.body, Modifier.padding(top = 4.dp), style = SmsBodyStyle, color = ur.text2) }
            }
        }
    }
}

/**
 * Add as new, or which existing transaction it is. Unanswered, this holds back the page's Add
 * button, so it's a bar with a button in it rather than a tag among the row's tags. Answered, it
 * says what was chosen and can be changed.
 */
@Composable
private fun SmsDecision(row: BankReviewRow, decision: String, enabled: Boolean, decide: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val asking = decision == "review"
    val shape = RoundedCornerShape(12.dp)
    Row(Modifier.fillMaxWidth().padding(top = 4.dp).heightIn(min = 48.dp).clip(shape).background(if (asking) ur.warningSoft else ur.surface2)
        .then(if (asking) Modifier.border(1.dp, ur.warning.copy(alpha = .45f), shape) else Modifier)
        .clickable(enabled, role = Role.Button) { open = true }.testTag("sms-decision").padding(start = 12.dp, end = 8.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(if (asking) stringResource(R.string.sms_decide_title) else Money.own(row.options.firstOrNull { it.first == decision }?.second.orEmpty()), Modifier.weight(1f),
            style = MaterialTheme.typography.bodyMedium, fontWeight = if (asking) FontWeight.SemiBold else FontWeight.Normal, color = if (asking) ur.warning else ur.text,
            maxLines = 2, overflow = TextOverflow.Ellipsis)
        if (asking) Box(Modifier.heightIn(min = 34.dp).clip(RoundedCornerShape(17.dp)).background(ur.accent).padding(horizontal = 14.dp), contentAlignment = Alignment.Center) {
            Text(stringResource(R.string.common_choose), color = if (ur.dark) ur.bg else Color.White, fontSize = 14.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        } else Text(stringResource(R.string.common_change), Modifier.padding(end = 4.dp), color = ur.accent, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
    }
    if (open) SmsDecisionSheet(row, decision, { open = false }) { decide(it); open = false }
}

/** The question on its own: which text it's about, and each answer as something to pick. */
@Composable
private fun SmsDecisionSheet(row: BankReviewRow, decision: String, close: () -> Unit, choose: (String) -> Unit) {
    UrSheet(close) {
        Column(Modifier.fillMaxWidth().weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 18.dp).padding(top = 8.dp, bottom = 20.dp).testTag("sms-decision-sheet"),
            verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(stringResource(R.string.sms_decide_title), Modifier.semantics { heading() }, style = MaterialTheme.typography.titleLarge, color = ur.text)
                Text(stringResource(R.string.sms_decide_detail), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            }
            ListCard { Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(Money.own(row.payee), Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold, color = ur.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(row.amountText, Modifier.padding(start = 10.dp), style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold,
                        color = if (row.amount > 0) ur.positive else ur.text, maxLines = 1)
                }
                if (row.detail.isNotEmpty()) Text(row.detail, style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
            } }
            Column(Modifier.selectableGroup(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                row.options.filter { it.first != "review" && it.first != "skip" }.forEach { (id, name) ->
                    val chosen = id == decision
                    val shape = RoundedCornerShape(14.dp)
                    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).clip(shape).border(if (chosen) 2.dp else 1.dp, if (chosen) ur.accent else ur.borderStrong, shape)
                        .selectable(chosen, role = Role.RadioButton) { choose(id) }.testTag("sms-decision-option:$id").padding(start = 8.dp, end = 14.dp, top = 6.dp, bottom = 6.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        RadioButton(chosen, null, Modifier.padding(horizontal = 8.dp), colors = RadioButtonDefaults.colors(selectedColor = ur.accent, unselectedColor = ur.muted))
                        Text(Money.own(name), Modifier.padding(start = 4.dp), style = MaterialTheme.typography.bodyLarge, color = ur.text)
                    }
                }
            }
            SecondaryButton(stringResource(R.string.sms_later_action), close, Modifier.fillMaxWidth().testTag("sms-decision-later"))
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

/** How many texts a tap that adds transactions may take out with them before the review asks; one that adds nothing always asks. */
internal const val SmsClearAsks = 50

/** What a skipped text is, by its reason. */
internal fun smsSkipLabel(skip: String): Int = when (skip) {
    "code" -> R.string.sms_skipped_code; "security" -> R.string.sms_skipped_security; "balance" -> R.string.sms_skipped_balance
    "declined" -> R.string.sms_skipped_declined; "rewards" -> R.string.sms_skipped_rewards; "marketing" -> R.string.sms_skipped_marketing
    "card" -> R.string.sms_skipped_card; "pair" -> R.string.sms_skipped_pair; else -> R.string.sms_skipped_other
}

/** A message the assistant couldn't turn into one transaction, or that isn't one: enter it, or let it go. */
@Composable
private fun SmsUnclearRow(item: SmsItem, index: Int, count: Int, enter: () -> Unit, dismiss: (() -> Unit)?) {
    Column(Modifier.cardSegment(0, 1, cardFill()).testTag("sms-other:${item.index}")) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            // A skipped text is said by its reason; the assistant writes no note for it.
            Text(item.note.ifBlank { stringResource(smsSkipLabel(item.skip)) }, style = MaterialTheme.typography.bodyLarge.copy(textDirection = androidx.compose.ui.text.style.TextDirection.Content), fontWeight = FontWeight.SemiBold, color = ur.text)
            item.sources.forEach { Text(it.body, style = SmsBodyStyle, color = ur.text2, maxLines = 6, overflow = TextOverflow.Ellipsis) }
            Row(Modifier.padding(top = 4.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SmallAction(stringResource(if (dismiss != null) R.string.sms_enter else R.string.sms_is_transaction), enter, primary = dismiss != null)
                dismiss?.let { SmallAction(stringResource(R.string.sms_dismiss), it) }
            }
        }
    }
}

/** The messages behind a transaction, under its editor, to check it against; a format's readings, how often they were right. */
@Composable
private fun SmsSources(item: SmsItem, accuracy: Int?) {
    ListCard { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.sms_from_messages), style = MaterialTheme.typography.titleSmall, color = ur.text)
        if (item.template != null) Text(listOfNotNull(stringResource(R.string.sms_from_template),
            accuracy?.let { stringResource(R.string.sms_template_accuracy, Money.count(it.toLong())) }).joinToString(" "),
            Modifier.testTag("sms-from-template"), style = MaterialTheme.typography.bodySmall, color = ur.muted)
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
    // A transfer has neither: there's no payee or category of it to learn.
    val transfer = record.text("transferAccountId").isNotBlank()
    return buildSet {
        if (record.text("date") != t.text("date")) add("date")
        if (record.optLong("amount") != item.amount) add("amount")
        if (!transfer && record.text("payee") != t.text("payee")) add("payee")
        if (!transfer && category != t.text("categoryId").ifBlank { null }) add("category")
    } to category
}

/**
 * A bank's name as it's compared with another: as payee rules read it, without the words that are
 * only numbers (a branch, a terminal), as the core's `bank_key` does.
 */
internal fun smsBankKey(name: String): String {
    val all = PayeeRules.normalize(name)
    val words = all.split(' ').filter { w -> w.isNotEmpty() && !w.all { it.isDigit() } }
    return if (words.isEmpty()) all else words.joinToString(" ")
}

/**
 * The review after the user renamed [item]'s payee or changed its category ([fields]): the other
 * rows from its sender with the same bank's name ([SmsItem.raw], or the same payee when none is
 * known) get the same, unless the user gave one of them a name of its own. A transfer is left.
 */
internal fun smsSameName(batch: SmsBatch, item: SmsItem, fields: Set<String>, payee: String, category: String?): SmsBatch {
    val before = item.transaction ?: return batch
    if ("payee" !in fields && "category" !in fields) return batch
    val raw = item.raw?.let(::smsBankKey)?.ifEmpty { null }
    val name = smsBankKey(before.text("payee"))
    val sender = smsSenderKey(item.sender)
    val senders = batch.messages.associate { it.id to smsSenderKey(it.sender) }
    val items = batch.analysis.rows("items")
    var next = batch
    items.forEachIndexed { index, other ->
        if (index == item.index) return@forEachIndexed
        val t = other.optJSONObject("transaction") ?: return@forEachIndexed
        if (t.text("kind") == "transfer" || t.text("transferAccountId").isNotBlank()) return@forEachIndexed
        val ids = other.optJSONArray("sourceIds") ?: return@forEachIndexed
        if ((0 until ids.length()).none { senders[ids.getString(it)] == sender }) return@forEachIndexed
        val theirs = other.text("raw").ifBlank { null }?.let(::smsBankKey)
        val same = if (raw != null && theirs != null) theirs == raw else name.isNotEmpty() && smsBankKey(t.text("payee")) == name
        // Still the bank's name, or what this row had before the user changed it: not a name of its own.
        val shown = smsBankKey(t.text("payee"))
        if (!same || (shown != name && shown != theirs)) return@forEachIndexed
        next = next.edit(index) { json ->
            val tx = json.getJSONObject("transaction")
            if ("payee" in fields) {
                val old = tx.text("payee")
                tx.put("payee", payee)
                if (old.length >= 3 && json.text("note").contains(old)) json.put("note", json.text("note").replace(old, payee))
            }
            if ("category" in fields && tx.text("categoryId") == before.text("categoryId")) tx.put("categoryId", category ?: JSONObject.NULL)
            json.optJSONArray("unsure")?.let { list -> json.put("unsure", JSONArray((0 until list.length()).map { list.getString(it) }.filter { it !in setOf("payee", "category") || it !in fields })) }
        }
    }
    return next
}

/**
 * The bank's name for a person when the user turned money to or from them into a transfer: their own
 * account at another bank, which the bank names by the account holder. Null for anything else.
 */
internal fun smsOwnName(item: SmsItem, record: JSONObject): String? {
    val t = item.transaction ?: return null
    if (record.text("transferAccountId").isBlank() || t.text("kind") !in setOf("income", "expense")) return null
    return (item.raw ?: t.text("payee")).trim().takeIf { it.isNotEmpty() && it.length <= 80 }
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
