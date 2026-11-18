package app.urbudget.nativeapp.ui

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.HelpOutline
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Investments
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import kotlinx.coroutines.CancellationException
import org.json.JSONObject
import java.time.LocalDate

/*
 * More → Investments, and the ways an investment account is kept up to date besides Interactive
 * Brokers (the desktop's InvestmentsSettings and HoldingsSetup.tsx): a positions file the broker
 * lets you download, read by the engine (positions_file.rs) and looked over before it's kept, or,
 * for someone who asked for it, a value typed in. Both go into the budget through one engine
 * command, `holdingsRecord`. A brokerage account linked under Bank connections gets its holdings
 * with each bank sync (BankConnectionsScreen.kt).
 */

/**
 * What's being set up, by the account's id: the choice between ways, a positions file being
 * looked over, or a value being typed. One of these is held for the whole app (App.kt), so an
 * account's card, its prompt and the Investments page all open the same sheets.
 */
@Stable
internal class HoldingsSetup {
    var choosing by mutableStateOf<String?>(null)
        private set
    var file by mutableStateOf<Pair<String, Uri>?>(null)
    var typing by mutableStateOf<String?>(null)
    /** The account a file is being picked for. */
    internal var picking: String? = null
    internal var launch: () -> Unit = {}

    /** Offers the ways `accountId` can be kept up to date, or what can be done with the way it is. */
    fun choose(accountId: String) { choosing = accountId }
    /** Asks for a positions file for `accountId`. */
    fun fromFile(accountId: String) { choosing = null; picking = accountId; launch() }
    /** Asks for `accountId`'s value. */
    fun type(accountId: String) { choosing = null; typing = accountId }
    fun closeChoice() { choosing = null }
}

@Composable
internal fun rememberHoldingsSetup(): HoldingsSetup {
    val setup = remember { HoldingsSetup() }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        val account = setup.picking
        setup.picking = null
        if (uri != null && account != null) setup.file = account to uri
    }
    SideEffect { setup.launch = { picker.launch(arrayOf("*/*")) } }
    return setup
}

/**
 * The sheets `setup` asks for. `broker` opens Interactive Brokers' own page; `ask` hands a file
 * to the assistant with what to do with it, where there is an assistant to ask.
 */
@Composable
internal fun HoldingsSetupDialogs(setup: HoldingsSetup, budget: JSONObject, vm: BudgetViewModel, broker: () -> Unit, ask: ((Uri, String) -> Unit)?) {
    val toast = LocalToast.current
    val typed = LocalPreferences.current.typedValues
    fun account(id: String?) = id?.let { budget.row("accounts", it) }
    // Said with Undo: the account's balance just moved to this value.
    fun done(account: JSONObject) = toast(Words[R.string.holdings_done, account.text("name")], true)
    account(setup.choosing)?.let { account ->
        val id = account.text("id")
        HoldingsChoice(account, Investments.of(budget, id), typed, file = { setup.fromFile(id) }, type = { setup.type(id) },
            broker = { setup.closeChoice(); broker() },
            unlink = { setup.closeChoice(); withCurrentBudget(budget, vm) { vm.editBudget { Investments.unlink(it, id) } } },
            close = setup::closeChoice)
    }
    setup.file?.let { (id, uri) ->
        account(id)?.let { account ->
            key(uri) {
                PositionsFileSheet(account, uri, budget, vm,
                    ask = ask?.let { hand -> { setup.file = null; hand(uri, Words[R.string.holdings_read_request, account.text("name")]) } },
                    close = { setup.file = null }) { setup.file = null; done(account) }
            }
        }
    }
    account(setup.typing)?.let { account ->
        key(account.text("id")) { TypedValueSheet(account, budget, vm, close = { setup.typing = null }) { setup.typing = null; done(account) } }
    }
}

/**
 * More → Investments: every account that can hold investments with how it's kept up to date, the
 * way into Interactive Brokers' own connection, and the switch that offers typing a value.
 */
@Composable
internal fun InvestmentsPage(budget: JSONObject, openBroker: () -> Unit) {
    val preferences = LocalPreferences.current
    val setup = LocalInvestmentActions.current?.setup
    val currency = budget.text("currency")
    val rows = remember(budget) { Investments.rows(budget).map { it to Investments.of(budget, it.text("id")) } }
    var how by rememberSaveable { mutableStateOf(false) }
    PageList(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item(key = "investments:lead") {
            Text(stringResource(R.string.holdings_lead), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
        }
        if (rows.isEmpty()) item(key = "investments:none") { Notice(stringResource(R.string.holdings_none), tone = "info") }
        else item(key = "investments:accounts") {
            ListCard {
                rows.forEachIndexed { index, (account, held) ->
                    ListRow(account.text("name"),
                        subtitle = held?.let { Money.joined(listOf(Investments.sourceName(it), Money.date(it.text("asOf")))) } ?: stringResource(R.string.holdings_nothing_yet),
                        value = held?.let { money(it.optLong("total"), currency) }, icon = Icons.Outlined.PieChart,
                        onClick = { if (held?.text("source") == "ibkr") openBroker() else setup?.choose(account.text("id")) }, divider = index < rows.lastIndex)
                }
            }
        }
        item(key = "investments:broker") {
            ListCard { ListRow("Interactive Brokers", subtitle = stringResource(R.string.holdings_broker_detail), icon = Icons.Outlined.Link, onClick = openBroker) }
        }
        item(key = "investments:how") {
            ListCard {
                ListRow(stringResource(R.string.holdings_how), icon = Icons.AutoMirrored.Outlined.HelpOutline, onClick = { how = !how }, chevron = false, divider = how, trailing = {
                    Icon(if (how) Icons.Outlined.KeyboardArrowDown else Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = ur.muted)
                })
                if (how) Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    listOf(R.string.holdings_how_broker, R.string.holdings_how_bank, R.string.holdings_how_file, R.string.holdings_how_typed).forEach { line ->
                        Row {
                            Text("•", Modifier.width(18.dp), style = MaterialTheme.typography.bodyMedium, color = ur.muted)
                            Text(stringResource(line), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                        }
                    }
                }
            }
        }
        item(key = "investments:typed") {
            ListCard { SwitchRow(stringResource(R.string.holdings_typed_switch), preferences.typedValues, preferences::setTypedValues, subtitle = stringResource(R.string.holdings_typed_switch_detail)) }
        }
    }
}

/**
 * For one account: the ways to keep it up to date when nothing fills it yet, else what can be
 * done with the way it is (a newer file, a newer value, or no longer following it).
 */
@Composable
private fun HoldingsChoice(account: JSONObject, held: JSONObject?, typed: Boolean, file: () -> Unit, type: () -> Unit, broker: () -> Unit,
                           unlink: () -> Unit, close: () -> Unit) {
    FormDialog(stringResource(R.string.holdings_choice_title), close, onConfirm = null) {
        Text(Money.own(account.text("name")), style = MaterialTheme.typography.titleSmall, color = ur.text)
        val source = held?.text("source")
        if (held != null) Text(Investments.sourceNote(held), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        ListCard {
            when (source) {
                null -> {
                    ListRow(stringResource(R.string.holdings_by_file), subtitle = stringResource(R.string.holdings_file_detail), icon = Icons.Outlined.FileOpen, onClick = file, divider = true)
                    ListRow("Interactive Brokers", subtitle = stringResource(R.string.holdings_broker_detail), icon = Icons.Outlined.Link, onClick = broker, divider = typed)
                    if (typed) ListRow(stringResource(R.string.holdings_type_value), subtitle = stringResource(R.string.holdings_type_detail), icon = Icons.Outlined.Edit, onClick = type)
                }
                "ibkr" -> ListRow("Interactive Brokers", subtitle = stringResource(R.string.holdings_broker_detail), icon = Icons.Outlined.Link, onClick = broker)
                else -> {
                    if (source == "file") ListRow(stringResource(R.string.holdings_update_file), icon = Icons.Outlined.FileOpen, onClick = file, divider = true)
                    if (source == "manual") ListRow(stringResource(R.string.holdings_update_value), icon = Icons.Outlined.Edit, onClick = type, divider = true)
                    ListRow(stringResource(R.string.ibkr_unlink), titleColor = ur.negative, onClick = unlink, chevron = false)
                }
            }
        }
        if (source != null && source != "ibkr") Text(stringResource(R.string.holdings_unlink_note), style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
}

/** Records what an account holds; `failed` hears why when the engine refuses it or the save fails. */
private fun record(vm: BudgetViewModel, accountId: String, snapshot: JSONObject, failed: (String) -> Unit, done: () -> Unit) {
    val failures = vm.operationFailureEpoch
    vm.runCommand("holdingsRecord", JSONObject().put("accountId", accountId).put("snapshot", snapshot)) { done() }
    vm.afterPending { if (vm.operationFailureEpoch != failures) failed(vm.state.value.error ?: Words[R.string.model_action_not_saved]) }
}

/**
 * What a positions file says an account holds, looked over before it's kept: each holding with
 * its value and cost, the day the figures are from, and the currency they're in. A file that
 * can't be read, or that left rows out, can go to the assistant, which reads other layouts and
 * proposes the same thing.
 */
@Composable
private fun PositionsFileSheet(account: JSONObject, uri: Uri, budget: JSONObject, vm: BudgetViewModel, ask: (() -> Unit)?, close: () -> Unit, done: () -> Unit) {
    val context = LocalContext.current
    val accountId = account.text("id")
    val budgetCurrency = budget.text("currency")
    val known = remember(budget, accountId) { Investments.of(budget, accountId) }
    var name by remember { mutableStateOf("") }
    var read by remember { mutableStateOf<JSONObject?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var date by remember { mutableStateOf(LocalDate.now().toString()) }
    var currency by remember { mutableStateOf(budgetCurrency) }
    var rate by remember { mutableStateOf("") }
    var saving by remember { mutableStateOf(false) }
    val requests = rememberPreviewJobs(uri)
    LaunchedEffect(uri) {
        try {
            val (fileName, text) = openStatement(context, uri, null)
            name = fileName
            // A PDF has no text to read here: it's the assistant's.
            if (text == null) { error = Words[R.string.holdings_file_pdf]; return@LaunchedEffect }
            requests.track(vm.query("holdingsFile", JSONObject().put("content", text.text), onError = { error = it }, anyRevision = true) { found ->
                read = found
                found.text("asOf").ifBlank { null }?.let { date = it }
                found.text("currency").ifBlank { null }?.let { currency = it }
            })
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message ?: Words[R.string.bank_statement_unopened] }
    }
    val code = Investments.foreign(currency, budgetCurrency)
    // A rate given for this account before is used again unless another is typed.
    val remembered = remember(known, code) { Investments.rememberedRate(known, code) }
    LaunchedEffect(remembered) { if (rate.isBlank() && remembered.isNotEmpty()) rate = remembered }
    val shown = code ?: budgetCurrency
    fun figure(text: String) = money(Investments.cents(text), shown)
    val found = read
    val positions = remember(found) { found?.rows("positions").orEmpty() }
    val skipped = found?.optInt("skipped") ?: 0
    LazyFormDialog(stringResource(R.string.holdings_by_file), close, confirm = stringResource(if (saving) R.string.holdings_saving else R.string.holdings_save),
        enabled = found != null && !saving && (code == null || Investments.rate(rate) != null),
        onConfirm = if (found == null) null else ({
            saving = true; error = null
            record(vm, accountId, Investments.fileSnapshot(found, name, date, currency, budgetCurrency, rate), failed = { error = it; saving = false }, done = done)
        })) {
        item(key = "positions:account") {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(Money.own(account.text("name")), style = MaterialTheme.typography.titleSmall, color = ur.text)
                if (found != null) Text(listOfNotNull(
                    countText(R.plurals.holdings_file_summary, positions.size, figure(found.text("value"))),
                    found.text("cash").ifBlank { null }?.let { stringResource(R.string.holdings_file_cash, figure(it)) }).joinToString(" "),
                    style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
        }
        if (saving) item(key = "positions:saving") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        error?.let { item(key = "positions:error") { FormError(it) } }
        if (found == null) {
            if (error == null) item(key = "positions:reading") {
                Text(stringResource(R.string.bank_reading_file), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            } else if (ask != null) item(key = "positions:assistant") {
                Notice(stringResource(R.string.holdings_file_help), tone = "info", action = stringResource(R.string.holdings_ask_assistant), onAction = ask, icon = Icons.Outlined.AutoAwesome)
            }
        } else {
            if (positions.isNotEmpty()) item(key = "positions:rows") {
                ListCard {
                    positions.forEachIndexed { index, position ->
                        val title = position.text("name").ifBlank { position.text("symbol") }
                        val symbol = position.text("symbol").takeIf { position.text("name").isNotBlank() && it.isNotBlank() && it.length <= 12 }
                        val quantity = position.text("quantity"); val price = position.text("price")
                        val amount = when {
                            quantity.isNotBlank() && price.isNotBlank() -> "${Investments.plain(quantity)} × ${Investments.plain(price)}"
                            quantity.isNotBlank() -> Investments.plain(quantity)
                            else -> null
                        }
                        val cost = position.text("cost").ifBlank { null }?.let { stringResource(R.string.holdings_cost, figure(it)) }
                        ListRow(title, subtitle = Money.joined(listOfNotNull(symbol?.let(Money::ltr), amount?.let(Money::ltr), cost)).ifBlank { null },
                            value = figure(position.text("value")), divider = index < positions.lastIndex)
                    }
                }
            }
            if (skipped > 0) item(key = "positions:skipped") {
                Notice(countText(R.plurals.holdings_file_skipped, skipped), tone = "info", action = if (ask != null) stringResource(R.string.holdings_ask_assistant) else null,
                    onAction = ask, icon = Icons.Outlined.AutoAwesome)
            }
            item(key = "positions:date") { DateField(date, { date = it }, stringResource(R.string.holdings_prices_from)) }
            item(key = "positions:currency") { Field(currency, { currency = it.take(3) }, stringResource(R.string.budget_currency), ltr = true) }
            if (code != null) item(key = "positions:rate") {
                Field(rate, { rate = it }, stringResource(R.string.ibkr_rate_label, code, budgetCurrency), numeric = true, ltr = true)
            }
            item(key = "positions:help") { Text(stringResource(R.string.holdings_balance_help), style = MaterialTheme.typography.bodySmall, color = ur.muted) }
        }
    }
}

/**
 * The account's value on a day, typed in: for a broker with nothing to download. Its balance
 * follows the value, and a day in the past fills its history in.
 */
@Composable
private fun TypedValueSheet(account: JSONObject, budget: JSONObject, vm: BudgetViewModel, close: () -> Unit, done: () -> Unit) {
    val accountId = account.text("id")
    val currency = budget.text("currency")
    val known = remember(budget, accountId) { Investments.of(budget, accountId) }
    var value by remember { mutableStateOf("") }
    var date by remember { mutableStateOf(LocalDate.now().toString()) }
    var error by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    FormDialog(stringResource(R.string.holdings_value_title, account.text("name")), close,
        confirm = stringResource(if (saving) R.string.holdings_saving else R.string.holdings_save_value), enabled = !saving && value.isNotBlank(), onConfirm = {
        runCatching { Money.parse(value, currency) }.onFailure { error = it.message }.onSuccess { cents ->
            saving = true; error = null
            record(vm, accountId, Investments.typedSnapshot(cents, date), failed = { error = it; saving = false }, done = done)
        }
    }) {
        Text(stringResource(R.string.holdings_value_lead), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
        FormError(error)
        Field(value, { value = it }, stringResource(R.string.ibkr_value), numeric = true, ltr = true)
        DateField(date, { date = it }, stringResource(R.string.holdings_value_on))
        if (known != null && (known.optJSONArray("values")?.length() ?: 0) > 0) Text(
            stringResource(R.string.holdings_value_last, money(known.optLong("total"), currency), Money.date(known.text("asOf"))),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
        Text(stringResource(R.string.holdings_balance_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
}
