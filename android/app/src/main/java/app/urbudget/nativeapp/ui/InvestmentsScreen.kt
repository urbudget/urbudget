package app.urbudget.nativeapp.ui

import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.FactCheck
import androidx.compose.material.icons.automirrored.outlined.HelpOutline
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.BankReviewRow
import app.urbudget.nativeapp.BankReviewSummary
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Investments
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.bankReviewChoices
import app.urbudget.nativeapp.bankReviewCounts
import app.urbudget.nativeapp.prepareBankReview
import app.urbudget.nativeapp.summarizeBankReview
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.time.LocalDate
import java.util.UUID

/*
 * Investment accounts that follow Interactive Brokers (the desktop's Investments.tsx): the
 * connection, under More, and what an account holds, over its register. The statement is read by
 * the shared engine (services/ibkr.rs) straight from Interactive Brokers with a token that can only
 * read; what it brings lives in the budget, so a phone without the token shows the same holdings.
 */

/** What an investment account's card can open. App provides it. */
@Stable
internal class InvestmentActions(
    /** Opens Interactive Brokers' page (under More → Investments), where this device connects. */
    val connection: () -> Unit,
    /** The other ways an account is kept up to date: a positions file, or a value typed in (HoldingsSetup.kt). */
    val setup: HoldingsSetup,
    /** Opens an account's page of performance and holdings (HoldingsScreen.kt) in its register's place. */
    val holdings: (String) -> Unit = {},
)
internal val LocalInvestmentActions = staticCompositionLocalOf<InvestmentActions?> { null }

/** A Flex statement saved from Client Portal can run to a year of days; larger than this isn't one. */
private const val MAX_STATEMENT_BYTES = 32 * 1024 * 1024

/** The chosen file's text. */
private fun readStatement(context: Context, uri: Uri): String {
    val bytes = context.contentResolver.openInputStream(uri)?.use { input ->
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (true) {
            val size = input.read(buffer)
            if (size < 0) break
            require(output.size() + size <= MAX_STATEMENT_BYTES) { context.getString(R.string.ibkr_file_too_large) }
            output.write(buffer, 0, size)
        }
        output.toByteArray()
    } ?: error(context.getString(R.string.bank_statement_unopened))
    return bytes.toString(Charsets.UTF_8)
}

/**
 * Brings the last statement into the budget (fetching a new one first when `fetch`), then reads
 * its accounts against the budget as it then is: which are linked, and their deposits to review.
 */
private fun bringStatement(vm: BudgetViewModel, jobs: PreviewJobs, fetch: Boolean, links: JSONArray, onError: (String) -> Unit,
                           done: (status: JSONObject?, review: JSONObject) -> Unit) {
    fun review(status: JSONObject?) { jobs.track(vm.service("ibkr_review_compact", onError = onError) { (it as? JSONObject)?.let { read -> done(status, read) } }) }
    fun apply(status: JSONObject?) { jobs.track(vm.service("ibkr_apply", JSONObject().put("links", links), onError = onError) { review(status) }) }
    if (fetch) jobs.track(vm.service("ibkr_fetch", onError = onError) { apply(it as? JSONObject) }) else apply(null)
}

/** What a statement's account is worth, in its own currency; without one, the bare figure. */
private fun brokerValue(account: JSONObject): String = account.text("currency").let { currency ->
    if (currency.isNotBlank()) money(account.optLong("value"), currency)
    else Money.ltr(Investments.plain(java.math.BigDecimal.valueOf(account.optLong("value"), 2).toPlainString(), 2))
}

/**
 * More → Investments → Interactive Brokers (the desktop's IbkrSettings): the three steps that make a Flex Query
 * and a token and paste them here, the account each of the statement's accounts fills, Sync, the
 * logins with a way to remove each, and whether to sync once a day when the app opens.
 */
@Composable
internal fun IbkrScreen(budget: JSONObject, vm: BudgetViewModel) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val toast = LocalToast.current
    var status by remember { mutableStateOf<JSONObject?>(null) }
    var review by remember { mutableStateOf<JSONObject?>(null) }
    // The budget the review's rows were checked against.
    var reviewBudget by remember { mutableStateOf<JSONObject?>(null) }
    var query by remember { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    // Where each new Interactive Brokers account goes: a new account, a tracking account's id, or "" for nowhere.
    var into by remember { mutableStateOf(mapOf<String, String>()) }
    var rates by remember { mutableStateOf(mapOf<String, String>()) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf("status") }
    // What's being asked about before it goes: one login, by its query id, or ("") the whole connection.
    var removing by remember { mutableStateOf<String?>(null) }
    // The request for Client Portal's query builder was copied; the folds of the setup that are open.
    var copied by remember { mutableStateOf(false) }
    var byHand by rememberSaveable { mutableStateOf(false) }
    var how by rememberSaveable { mutableStateOf(false) }
    // Someone who brought in a file is pasting a token after all.
    var withToken by rememberSaveable { mutableStateOf(false) }
    val clipboard = LocalClipboardManager.current
    val preferences = LocalPreferences.current
    // The fields for a second login at Interactive Brokers are showing.
    var another by remember { mutableStateOf(false) }
    var reviewing by remember { mutableStateOf<String?>(null) }
    val requests = rememberPreviewJobs()
    val fail: (String) -> Unit = { error = it; busy = "" }
    fun readReview() {
        requests.track(vm.service("ibkr_review_compact", onError = fail) { value ->
            (value as? JSONObject)?.let { review = it; reviewBudget = vm.state.value.budget }
            busy = ""
        })
    }
    fun show(value: Any?) {
        val next = value as? JSONObject
        if (next != null) status = next
        if (next == null || next.rows("accounts").isEmpty()) { review = null; busy = "" } else readReview()
    }
    fun call(what: String, method: String, params: JSONObject = JSONObject(), then: (Any?) -> Unit = { show(it) }) {
        busy = what; error = null
        requests.track(vm.service(method, params, onError = fail, callback = then))
    }
    val connected = status?.optBoolean("connected") == true
    // Logins that couldn't be fetched the last time they were asked, each with why.
    val refused = status?.rows("logins").orEmpty().filter { it.text("error").isNotEmpty() }
    val logins = status?.rows("logins").orEmpty()
    val hasStatement = status?.rows("accounts").orEmpty().isNotEmpty()
    // The statement's accounts. One the budget keeps from another device's login (`kept`) is on its own page, not here.
    val accounts = remember(review) { review?.rows("accounts").orEmpty().filter { !it.optBoolean("kept") } }
    val adding = remember(accounts, into) { Investments.adding(accounts, into) }
    val ready = remember(accounts, into, rates) { Investments.ready(accounts, into, rates) }
    val names = remember(budget) { budget.rows("accounts").associate { it.text("id") to it.text("name") } }
    val newAccount = stringResource(R.string.ibkr_new_account)
    val dontAdd = stringResource(R.string.ibkr_dont_add)
    val targets = remember(budget, newAccount, dontAdd) {
        listOf(Investments.NEW to newAccount) + Investments.targets(budget).map { it.text("id") to it.text("name") } + listOf("" to dontAdd)
    }
    // Accounts a broker already fills, shown before this device has a statement of its own.
    val filled = remember(budget) { budget.rows(Investments.KEY).filter { it.text("source") == "ibkr" }.mapNotNull { held -> budget.row("accounts", held.text("accountId"))?.let { it to held } } }
    fun bring(fetch: Boolean) {
        val links = Investments.links(accounts, into, rates) { UUID.randomUUID().toString() }
        val count = adding.size
        busy = if (fetch) "fetch" else "sync"; error = null
        bringStatement(vm, requests, fetch, links, fail) { fetched, next ->
            fetched?.let { status = it }
            review = next; reviewBudget = vm.state.value.budget; busy = ""; into = emptyMap()
            val failed = next.rows("accounts").firstOrNull { it.text("accountId").isNotEmpty() && it.text("error").isNotEmpty() }
            if (failed != null) error = failed.text("error")
            // A login that was refused is named in its own notice: nothing claims all is well.
            else if (fetched?.rows("logins").orEmpty().none { it.text("error").isNotEmpty() }) toast(if (count > 0) Words.count(R.plurals.ibkr_added_accounts, count.toLong()) else Words[R.string.ibkr_up_to_date], false)
        }
    }
    fun unlink(accountId: String) = withCurrentBudget(budget, vm) {
        vm.editBudget { Investments.unlink(it, accountId) }
        vm.afterPending { if (hasStatement) { busy = "status"; readReview() } }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            busy = "file"; error = null
            requests.track(scope.launch {
                try {
                    val content = withContext(Dispatchers.IO) { readStatement(context, uri) }
                    call("file", "ibkr_file", JSONObject().put("content", content))
                } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { fail(failure.message ?: context.getString(R.string.bank_statement_unopened)) }
            })
        }
    }
    // What to do in Client Portal, step by step, with the two things to paste at the step they come from.
    val setupCard: @Composable () -> Unit = {
        ListCard {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
                SetupStep(1, stringResource(R.string.ibkr_step1_title), stringResource(R.string.ibkr_step1_text)) {
                    // The request is written to Client Portal in English, left to right whatever the interface.
                    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(fieldFill()).padding(start = 12.dp, end = 12.dp, top = 12.dp, bottom = 4.dp)) {
                        Text(Investments.FLEX_REQUEST, Modifier.fillMaxWidth(), style = MaterialTheme.typography.bodySmall.copy(textDirection = TextDirection.Ltr), color = ur.text2)
                        Text(stringResource(if (copied) R.string.ibkr_copied else R.string.ibkr_copy),
                            Modifier.clickable(role = Role.Button) { clipboard.setText(AnnotatedString(Investments.FLEX_REQUEST)); copied = true }.padding(vertical = 10.dp),
                            style = MaterialTheme.typography.bodyMedium, color = ur.accent, fontWeight = FontWeight.SemiBold)
                    }
                    Text(stringResource(R.string.ibkr_by_hand), Modifier.clickable(role = Role.Button) { byHand = !byHand }.padding(vertical = 4.dp),
                        style = MaterialTheme.typography.bodyMedium, color = ur.accent, fontWeight = FontWeight.SemiBold)
                    if (byHand) Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        GuideLine("1.", stringResource(R.string.ibkr_hand_name), small = true)
                        GuideLine("2.", stringResource(R.string.ibkr_hand_sections), small = true) {
                            // Client Portal's own names, in English whatever the interface.
                            listOf("Account Information" to true, "Cash Report" to true, "Cash Transactions" to true, "Net Asset Value (NAV) in Base" to true,
                                stringResource(R.string.ibkr_hand_positions) to false, stringResource(R.string.ibkr_hand_trades) to false, "Transfers" to true,
                            ).forEach { (name, english) -> GuideLine("•", name, small = true, ltr = english) }
                        }
                        GuideLine("3.", stringResource(R.string.ibkr_hand_delivery), small = true)
                        GuideLine("4.", stringResource(R.string.ibkr_hand_general), small = true)
                        GuideLine("5.", stringResource(R.string.ibkr_hand_save), small = true)
                    }
                }
                SetupStep(2, stringResource(R.string.ibkr_step2_title)) {
                    GuideLine("•", stringResource(R.string.ibkr_token_turn_on))
                    GuideLine("•", stringResource(R.string.ibkr_token_generate))
                }
                SetupStep(3, stringResource(R.string.ibkr_step3_title)) {
                    GuideLine("•", stringResource(R.string.ibkr_paste_query_id))
                    GuideLine("•", stringResource(R.string.ibkr_paste_token))
                    FilledField(query, { query = it }, Modifier.fillMaxWidth(), label = { Text(stringResource(R.string.ibkr_query_id)) }, singleLine = true)
                    FilledField(token, { token = it }, Modifier.fillMaxWidth(), label = { Text(stringResource(R.string.ibkr_token)) }, visualTransformation = PasswordVisualTransformation(), singleLine = true)
                    PrimaryButton(stringResource(if (busy == "connect") R.string.ibkr_connecting else R.string.bank_connect), {
                        val (pastedToken, pastedQuery) = token to query
                        call("connect", "ibkr_connect", JSONObject().put("token", pastedToken).put("query", pastedQuery)) { token = ""; query = ""; another = false; withToken = false; show(it) }
                    }, Modifier.fillMaxWidth(), enabled = token.isNotBlank() && query.isNotBlank() && busy.isEmpty())
                    status?.text("store")?.ifBlank { null }?.let {
                        Text(stringResource(R.string.ibkr_token_kept, it), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                    }
                }
            }
        }
    }
    // What the token can do and when the app syncs, folded away.
    val howCard: @Composable () -> Unit = {
        ListCard {
            ListRow(stringResource(R.string.ibkr_how), icon = Icons.AutoMirrored.Outlined.HelpOutline, onClick = { how = !how }, chevron = false, divider = how, trailing = {
                Icon(if (how) Icons.Outlined.KeyboardArrowDown else Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = ur.muted)
            })
            if (how) Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 6.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                listOf(R.string.ibkr_how_token, R.string.ibkr_how_sync, R.string.ibkr_how_when, R.string.ibkr_how_balance).forEach { line ->
                    Row {
                        Text("•", Modifier.width(18.dp), style = MaterialTheme.typography.bodyMedium, color = ur.muted)
                        Text(stringResource(line), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                    }
                }
                TextButton({ call("guide", "ibkr_open") { busy = "" } }, enabled = busy.isEmpty()) { Text(stringResource(R.string.ibkr_guide), color = ur.accent, fontWeight = FontWeight.SemiBold) }
            }
        }
    }
    // Accounts a broker already fills, as the budget keeps them: all there is to show without a statement.
    fun LazyListScope.filledRows() {
        if (filled.isEmpty()) return
        item(key = "ibkr:filled-title") { SectionTitle(stringResource(R.string.ibkr_in_budget), stringResource(R.string.ibkr_in_budget_detail)) }
        item(key = "ibkr:filled") {
            ListCard {
                filled.forEachIndexed { index, (account, held) ->
                    ListRow(account.text("name"), subtitle = Money.joined(listOf(held.text("sourceAccount"), stringResource(R.string.ibkr_prices_from_date, Money.date(held.text("asOf"))))),
                        value = money(held.optLong("total"), budget.text("currency")), icon = Icons.Outlined.PieChart, divider = index < filled.lastIndex)
                }
            }
        }
    }
    LaunchedEffect(Unit) { call("status", "ibkr_status") }
    // The budget changed under the review (an edit elsewhere, a sync from another device): read it again.
    LaunchedEffect(budget) { if (review != null && reviewBudget !== budget && busy.isEmpty()) readReview() }
    PageList(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (busy.isNotEmpty()) item(key = "ibkr:busy") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        if (busy == "connect" || busy == "fetch") item(key = "ibkr:preparing") {
            Text(stringResource(R.string.ibkr_preparing), Modifier.padding(horizontal = 4.dp), color = ur.text2, style = MaterialTheme.typography.bodyMedium)
        }
        error?.let { item(key = "ibkr:error") { FormError(it) } }
        // Until the first answer says whether there is a connection, nothing is shown.
        if (status == null) Unit
        else if (!connected && !hasStatement) {
            // Nothing here yet: the three steps, or a statement file.
            item(key = "ibkr:lead") { Text(stringResource(R.string.ibkr_lead), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium) }
            item(key = "ibkr:setup") { setupCard() }
            item(key = "ibkr:how") { howCard() }
            item(key = "ibkr:file") { SecondaryButton(stringResource(R.string.ibkr_use_file), { picker.launch(arrayOf("*/*")) }, Modifier.fillMaxWidth(), enabled = busy.isEmpty(), icon = Icons.Outlined.FileOpen) }
            filledRows()
        } else {
            item(key = "ibkr:state") {
                Text(stringResource(if (connected) R.string.ibkr_connected_lead else R.string.ibkr_file_lead), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
            }
            items(refused, key = { "ibkr:refused:${it.text("query")}" }) { login ->
                Notice(stringResource(R.string.ibkr_login_refused, Money.ltr(login.text("query")), login.text("error")))
            }
            if (hasStatement) {
                item(key = "ibkr:accounts-title") { SectionTitle(stringResource(R.string.ibkr_at_broker)) }
                items(accounts, key = { "ibkr:account:${it.text("id")}" }) { account ->
                    val id = account.text("id")
                    IbkrAccountCard(account, names[account.text("accountId")], targets, into[id] ?: Investments.NEW, rates[id].orEmpty(), busy.isEmpty(),
                        choose = { into = into + (id to it) }, setRate = { rates = rates + (id to it) },
                        review = { reviewing = account.text("accountId") }, unlink = { unlink(account.text("accountId")) })
                }
                if (accounts.any { it.optJSONArray("needsRate") != null }) item(key = "ibkr:rate-note") {
                    Text(stringResource(R.string.ibkr_rate_note), Modifier.padding(horizontal = 4.dp), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                }
                accounts.firstOrNull { (it.optJSONArray("missing")?.length() ?: 0) > 0 }?.let { account ->
                    val sections = account.optJSONArray("missing")?.let { list -> (0 until list.length()).joinToString(", ") { list.optString(it) } }.orEmpty()
                    item(key = "ibkr:missing") { Notice(stringResource(R.string.ibkr_missing, Money.ltr(sections)), tone = "info") }
                }
            } else {
                // Connected, with no statement fetched since the app opened: what the budget holds, and Sync.
                filledRows()
            }
            // Without a token there is nothing to fetch: the next statement comes from a file.
            if (adding.isNotEmpty() || connected) item(key = "ibkr:primary") {
                if (adding.isNotEmpty()) PrimaryButton(stringResource(if (busy == "sync") R.string.ibkr_adding else R.string.ibkr_add), { bring(false) }, Modifier.fillMaxWidth(),
                    enabled = ready && busy.isEmpty() && review != null)
                else PrimaryButton(stringResource(R.string.ibkr_sync_now), { bring(true) }, Modifier.fillMaxWidth(), enabled = busy.isEmpty(), icon = Icons.Outlined.Refresh)
            }
            item(key = "ibkr:file") { SecondaryButton(stringResource(R.string.ibkr_use_file), { picker.launch(arrayOf("*/*")) }, Modifier.fillMaxWidth(), enabled = busy.isEmpty(), icon = Icons.Outlined.FileOpen) }
            if (connected) {
                // The logins, once there is more than one: each is removed on its own.
                if (logins.size > 1) {
                    item(key = "ibkr:logins-title") { SectionTitle(stringResource(R.string.ibkr_logins)) }
                    item(key = "ibkr:logins") {
                        ListCard {
                            logins.forEachIndexed { index, login ->
                                val (title, detail) = Investments.loginName(login, accounts)
                                val named = stringResource(R.string.ibkr_query, Money.ltr(login.text("query")))
                                ListRow(title ?: named, subtitle = if (title != null) named else detail, chevron = false, divider = index < logins.lastIndex, trailing = {
                                    TextButton({ removing = login.text("query") }, enabled = busy.isEmpty()) { Text(stringResource(R.string.common_remove), color = ur.negative, fontWeight = FontWeight.SemiBold) }
                                })
                            }
                        }
                    }
                }
                item(key = "ibkr:daily") { ListCard { SwitchRow(stringResource(R.string.ibkr_daily), preferences.ibkrDaily, preferences::setIbkrDaily) } }
                // Someone with a second login at Interactive Brokers pastes its query and token too.
                item(key = "ibkr:another") {
                    TextButton({ another = !another }, enabled = busy.isEmpty()) {
                        Text(stringResource(if (another) R.string.common_cancel else R.string.ibkr_add_login), color = ur.accent, fontWeight = FontWeight.SemiBold)
                    }
                }
                if (another) {
                    item(key = "ibkr:another-help") {
                        Text(stringResource(R.string.ibkr_add_login_help), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
                    }
                    item(key = "ibkr:setup") { setupCard() }
                }
            } else {
                // A statement file brought these in: a token keeps them up to date without one.
                item(key = "ibkr:with-token") {
                    TextButton({ withToken = !withToken }, enabled = busy.isEmpty()) {
                        Text(stringResource(if (withToken) R.string.common_cancel else R.string.ibkr_with_token), color = ur.accent, fontWeight = FontWeight.SemiBold)
                    }
                }
                if (withToken) item(key = "ibkr:setup") { setupCard() }
            }
            item(key = "ibkr:how") { howCard() }
            // With several logins each is removed on its own line above; without a token there is only the statement to forget.
            if (!connected || logins.size < 2) item(key = "ibkr:remove") {
                TextButton({ if (connected) removing = "" else call("remove", "ibkr_disconnect") }, enabled = busy.isEmpty()) {
                    Text(stringResource(if (connected) R.string.ibkr_remove else R.string.ibkr_start_over), color = if (connected) ur.negative else ur.accent, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
    removing?.let { which ->
        val one = which.isNotEmpty()
        val action = stringResource(if (one) R.string.ibkr_remove_login else R.string.ibkr_remove)
        AlertDialog(onDismissRequest = { removing = null }, containerColor = ur.surface, title = { Text(action) },
            text = { Text(stringResource(if (one) R.string.ibkr_remove_login_message else R.string.ibkr_remove_message)) },
            confirmButton = {
                TextButton({ removing = null; call("remove", "ibkr_disconnect", if (one) JSONObject().put("query", which) else JSONObject()) }) { Text(action, color = ur.negative) }
            },
            dismissButton = { TextButton({ removing = null }) { Text(stringResource(R.string.ibkr_keep)) } })
    }
    val entry = reviewing?.let { id -> accounts.firstOrNull { it.text("accountId") == id } }
    if (entry != null && busy.isEmpty()) key(entry.text("accountId")) {
        IbkrDepositsReview(entry, names[entry.text("accountId")].orEmpty(), reviewBudget, budget, vm, close = { reviewing = null }) { count ->
            reviewing = null; busy = "status"; readReview()
            if (count > 0) toast(Words.count(R.plurals.ibkr_deposits_added, count.toLong()), false)
        }
    }
}

/** A step of the setup: its number, what to do, and whatever it needs under it. */
@Composable
private fun SetupStep(number: Int, title: String, text: String? = null, content: @Composable ColumnScope.() -> Unit = {}) {
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.size(26.dp).clip(CircleShape).background(ur.accentSoft), contentAlignment = Alignment.Center) {
            Text(Money.ltr("$number"), style = MaterialTheme.typography.labelLarge, color = ur.accent, fontWeight = FontWeight.SemiBold)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(title, style = MaterialTheme.typography.titleSmall, color = ur.text)
            if (text != null) Text(text, style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            content()
        }
    }
}

/**
 * One part of a setup step, behind its marker: "1." where the order matters, "•" where it
 * doesn't. [ltr] keeps a name from Client Portal's English interface reading left to right.
 */
@Composable
private fun GuideLine(marker: String, text: String, small: Boolean = false, ltr: Boolean = false, content: @Composable ColumnScope.() -> Unit = {}) {
    val style = if (small) MaterialTheme.typography.bodySmall else MaterialTheme.typography.bodyMedium
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(Money.ltr(marker), Modifier.widthIn(min = if (small) 14.dp else 12.dp), style = style, color = ur.muted)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(text, style = if (ltr) style.copy(textDirection = TextDirection.Ltr) else style, color = if (small) ur.muted else ur.text2)
            content()
        }
    }
}

/**
 * Syncs the budget's Interactive Brokers accounts once a day, a few seconds after the app opens,
 * when this device has the token (the desktop's useDailyIbkrSync). The broker makes one statement
 * a day, so once is enough; a failure stays quiet (the account's card says when prices are old)
 * and isn't tried again until tomorrow.
 */
@Composable
internal fun DailyIbkrSync(budget: JSONObject?, vm: BudgetViewModel, toast: (String, Boolean) -> Unit) {
    val preferences = LocalPreferences.current
    val requests = rememberPreviewJobs()
    val filled = remember(budget) { Investments.filled(budget) }
    val latest by rememberUpdatedState(budget)
    val say by rememberUpdatedState(toast)
    LaunchedEffect(filled) {
        fun due() = Investments.dailyDue(filled, preferences.ibkrDaily, preferences.ibkrDailyAt, LocalDate.now().toString())
        if (!due()) return@LaunchedEffect
        delay(4000)
        if (!due()) return@LaunchedEffect
        requests.track(vm.service("ibkr_status", onError = {}) { status ->
            if ((status as? JSONObject)?.optBoolean("connected") != true) return@service
            preferences.ibkrDailyAt = LocalDate.now().toString()
            val had = Investments.newestPrices(latest)
            // Offline, or the statement isn't ready: tomorrow's try, or Sync, brings it.
            bringStatement(vm, requests, true, JSONArray(), {}) { _, review ->
                val has = review.rows("accounts").filter { it.text("accountId").isNotEmpty() }.maxOfOrNull { it.text("asOf") }.orEmpty()
                if (has > had) say(Words[R.string.ibkr_daily_synced, Money.date(has)], false)
            }
        })
    }
}

/**
 * On an open investment account nothing fills: the offer to see what it holds (the desktop's
 * prompt over such an account's register). "Set up" offers the ways to keep it up to date;
 * "Not now" puts the question away on every account.
 */
@Composable
internal fun IbkrPrompt(accountId: String, modifier: Modifier = Modifier) {
    val preferences = LocalPreferences.current
    val actions = LocalInvestmentActions.current ?: return
    ListCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(stringResource(R.string.holdings_prompt_title), style = MaterialTheme.typography.titleSmall, color = ur.text)
            Text(stringResource(R.string.holdings_prompt_text), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            Row(Modifier.fillMaxWidth().padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End), verticalAlignment = Alignment.CenterVertically) {
                TextButton({ preferences.setIbkrPromptHidden(true) }) { Text(stringResource(R.string.sms_not_now), color = ur.muted, fontWeight = FontWeight.SemiBold) }
                SecondaryButton(stringResource(R.string.holdings_set_up), { actions.setup.choose(accountId) }, icon = Icons.Outlined.Insights)
            }
        }
    }
}

/** One of the statement's accounts: what it's worth, and the budget account it fills or is to fill. */
@Composable
private fun IbkrAccountCard(account: JSONObject, linkedName: String?, targets: List<Pair<String, String>>, choice: String, rate: String, enabled: Boolean,
                            choose: (String) -> Unit, setRate: (String) -> Unit, review: () -> Unit, unlink: () -> Unit) {
    val alias = account.text("alias")
    val linked = account.text("accountId").isNotEmpty()
    val needsRate = account.optJSONArray("needsRate")
    val fresh = Investments.fresh(account)
    ListCard {
        ListRow(alias.ifBlank { account.text("id") }, subtitle = Money.joined(listOfNotNull(account.text("id").takeIf { alias.isNotBlank() },
            countText(R.plurals.ibkr_holdings, account.optInt("positions")), account.text("asOf").ifBlank { null }?.let(Money::date))),
            value = brokerValue(account), icon = Icons.Outlined.PieChart, chevron = false, divider = true)
        if (linked) {
            ListRow(stringResource(R.string.ibkr_goes_into), value = Money.own(linkedName.orEmpty()), divider = true)
            if (fresh > 0) ListRow(countText(R.plurals.ibkr_review_deposits, fresh), titleColor = ur.accent, icon = Icons.AutoMirrored.Outlined.FactCheck,
                onClick = if (enabled) review else null, chevron = true, divider = true)
        }
        if (!linked || needsRate != null) Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (!linked) Choice(stringResource(R.string.ibkr_goes_into), choice, targets) { if (enabled) choose(it) }
            if (needsRate != null && (linked || choice.isNotEmpty()))
                Field(rate, setRate, stringResource(R.string.ibkr_rate_label, needsRate.optString(0), needsRate.optString(1)), numeric = true, ltr = true)
        }
        account.text("error").ifBlank { null }?.let { Text(it, Modifier.padding(horizontal = 16.dp).padding(bottom = 12.dp), style = MaterialTheme.typography.bodySmall, color = ur.negative) }
        if (linked) {
            if (needsRate != null) Hairline(start = 16.dp)
            ListRow(stringResource(R.string.ibkr_unlink), titleColor = ur.negative, onClick = if (enabled) unlink else null, chevron = false)
        }
    }
}

/**
 * The deposits, withdrawals and transfers the broker listed, reviewed like a bank's rows
 * (Investments.tsx's Deposits): each is matched to the transfer already in the budget, joined to
 * its other side, or added (as a transfer with the account chosen for it, or with none), so it
 * counts as money put in and not as a gain. `imported` gets how many were added or matched.
 */
@Composable
internal fun IbkrDepositsReview(review: JSONObject, accountName: String, reviewedBudget: JSONObject?, budget: JSONObject, vm: BudgetViewModel,
                                close: () -> Unit, imported: (Int) -> Unit) {
    val scope = rememberCoroutineScope()
    val requests = rememberPreviewJobs(review)
    // The options name themselves in the interface language; the choices made so far stay.
    val preparationKey = remember(review, reviewedBudget, budget, languageKey()) { Any() }
    val prepared by produceState<ScopedResult<Pair<List<BankReviewRow>, BankReviewSummary>>?>(null, preparationKey) {
        value = withContext(Dispatchers.Default) {
            // The broker says money moved, not where from: any other account can be named as its other side.
            val checked = reviewedBudget ?: budget
            val rows = prepareBankReview(review.rows("rows"), checked, false, review.optJSONArray("candidateSets"),
                Investments.others(checked, review.text("accountId")), Words[R.string.ibkr_option_no_other])
            ScopedResult(preparationKey, rows to summarizeBankReview(rows))
        }
    }
    val ready = prepared?.forScope(preparationKey)
    val rows = ready?.first
    val decisions = remember(review) { mutableStateMapOf<Int, String>() }
    // Keyed by the rows: they arrive after the first composition, and the count must be of them.
    val counts by remember(rows) { derivedStateOf { bankReviewCounts(rows.orEmpty(), decisions) } }
    val (importing, undecided) = counts
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    LazyFormDialog(stringResource(R.string.ibkr_deposits_title), close,
        confirm = if (loading) stringResource(R.string.bank_importing) else if (importing > 0) stringResource(R.string.bank_import_count, "$importing") else stringResource(R.string.common_done),
        enabled = rows != null && !loading && undecided == 0, onConfirm = {
        if (importing == 0) close()
        else if (reviewedBudget !== vm.state.value.budget) error = Words[R.string.ibkr_budget_changed]
        else {
            loading = true
            val selected = decisions.toMap(); val sourceRows = rows.orEmpty(); val count = importing
            scope.launch {
                try {
                    val choices = withContext(Dispatchers.Default) { bankReviewChoices(sourceRows, selected, false) }
                    require(reviewedBudget === vm.state.value.budget) { Words[R.string.ibkr_budget_changed] }
                    requests.track(vm.service("ibkr_import", JSONObject().put("account", review.text("accountId")).put("choices", choices), onError = { loading = false; error = it }) {
                        imported(count)
                    })
                } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { loading = false; error = failure.message }
            }
        }
    }) {
        item(key = "deposits:title") {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                if (accountName.isNotBlank()) Text(Money.own(accountName), style = MaterialTheme.typography.titleSmall, color = ur.text)
                Text(stringResource(R.string.ibkr_deposits_lead), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
        }
        if (loading || rows == null) item(key = "deposits:loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        error?.let { item(key = "deposits:error") { FormError(it) } }
        ready?.second?.let { bankReviewNotes(it, undecided) }
        rows?.let { suggestedTransfers(it, decisions, !loading) }
        bankReviewRows(rows.orEmpty(), decisions, !loading) { index, value -> decisions[index] = value }
    }
}

/**
 * An investment account's card, over its register: its value as the broker reported it, the
 * gain beyond the money put in, its deposits to review and Sync. The whole picture (the period's
 * return and chart, each holding, income and costs) is a page of its own that opens from here in
 * the register's place (HoldingsScreen.kt); the app holds it, so it stays open as the phone folds
 * and unfolds. Without holdings there is nothing to show.
 */
@Composable
internal fun HoldingsCard(account: JSONObject, budget: JSONObject, vm: BudgetViewModel, modifier: Modifier = Modifier) {
    val accountId = account.text("id")
    val held = remember(budget, accountId) { Investments.of(budget, accountId) } ?: return
    val actions = LocalInvestmentActions.current
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val gain = rememberPrepared(budget, accountId) { Investments.gain(budget, accountId) }
    val since = remember(held) { Investments.since(held) }
    val positions = remember(held) { held.rows("positions") }
    var status by remember(accountId) { mutableStateOf<JSONObject?>(null) }
    var review by remember(accountId) { mutableStateOf<JSONObject?>(null) }
    var reviewBudget by remember(accountId) { mutableStateOf<JSONObject?>(null) }
    var busy by remember(accountId) { mutableStateOf(false) }
    var message by remember(accountId) { mutableStateOf<String?>(null) }
    var reviewing by remember(accountId) { mutableStateOf(false) }
    val requests = rememberPreviewJobs(accountId)
    fun keep(next: JSONObject) { review = next.rows("accounts").firstOrNull { it.text("accountId") == accountId }; reviewBudget = vm.state.value.budget }
    // The budget keeps what the broker listed, so the deposits still to review show with or without a
    // statement this session; a failure to read them isn't worth a word here.
    fun check() { requests.track(vm.service("ibkr_review_compact", onError = {}) { (it as? JSONObject)?.let(::keep) }) }
    // Only Interactive Brokers has a connection to ask about and deposits to review: an account kept
    // from a file, a bank connection or a typed value shows its figures and nothing to sync here.
    val byBroker = held.text("source") == "ibkr"
    LaunchedEffect(accountId, byBroker) {
        if (!byBroker) return@LaunchedEffect
        requests.track(vm.service("ibkr_status", onError = {}) { value -> status = value as? JSONObject })
        check()
    }
    LaunchedEffect(budget) { if (review != null && reviewBudget !== budget && !busy) check() }
    fun sync() {
        busy = true; message = null
        bringStatement(vm, requests, true, JSONArray(), { message = it; busy = false }) { fetched, next ->
            fetched?.let { status = it }
            keep(next); busy = false
            val mine = review
            when {
                mine?.text("error")?.isNotEmpty() == true -> message = mine.text("error")
                mine?.optJSONArray("needsRate") != null -> message = Words[R.string.ibkr_needs_rate]
                else -> toast(Words[R.string.ibkr_account_synced, account.text("name")], false)
            }
        }
    }
    val fresh = Investments.fresh(review)
    ListCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row {
                Figure(stringResource(R.string.ibkr_value), money(held.optLong("total"), currency), Modifier.weight(1f))
                if (gain != null && since != null) Figure(stringResource(R.string.ibkr_gain_since, Money.date(since)), Money.signed(gain, currency), Modifier.weight(1f),
                    color = if (gain > 0) ur.positive else if (gain < 0) ur.negative else ur.text)
            }
            val stale = Investments.stale(held.text("asOf"))
            Text(listOfNotNull(stringResource(R.string.ibkr_prices_from_date, Money.date(held.text("asOf"))), if (stale) stringResource(R.string.ibkr_stale) else null).joinToString(" "),
                style = MaterialTheme.typography.bodySmall, color = if (stale) ur.warning else ur.muted)
            if (fresh > 0) PrimaryButton(countText(R.plurals.ibkr_review_deposits, fresh), { reviewing = true }, Modifier.fillMaxWidth(), enabled = !busy)
            when {
                // Kept from a file or a typed value: the next one comes the same way.
                held.text("source") == "file" -> if (actions != null)
                    SecondaryButton(stringResource(R.string.holdings_update_file), { actions.setup.fromFile(accountId) }, Modifier.fillMaxWidth(), icon = Icons.Outlined.FileOpen)
                held.text("source") == "manual" -> if (actions != null)
                    SecondaryButton(stringResource(R.string.holdings_update_value), { actions.setup.type(accountId) }, Modifier.fillMaxWidth(), icon = Icons.Outlined.Edit)
                !byBroker -> Unit
                status?.optBoolean("connected") == true ->
                    SecondaryButton(stringResource(if (busy) R.string.ibkr_syncing else R.string.ibkr_sync), ::sync, Modifier.fillMaxWidth(), enabled = !busy, icon = Icons.Outlined.Refresh)
                status != null && actions != null ->
                    SecondaryButton(stringResource(R.string.ibkr_connect_here), actions.connection, Modifier.fillMaxWidth(), icon = Icons.Outlined.Link)
            }
            if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
            message?.let { FormError(it) }
        }
        Hairline()
        if (actions != null) ListRow(stringResource(R.string.ibkr_details), subtitle = if (positions.isEmpty()) null else countText(R.plurals.ibkr_holdings, positions.size),
            icon = Icons.Outlined.Insights, onClick = { actions.holdings(accountId) }, divider = true)
        Text(Investments.sourceNote(held), Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
    val entry = review
    if (reviewing && entry != null && !busy) IbkrDepositsReview(entry, account.text("name"), reviewBudget, budget, vm, close = { reviewing = false }) { count ->
        reviewing = false; check()
        if (count > 0) toast(Words.count(R.plurals.ibkr_deposits_added, count.toLong()), false)
    }
}
