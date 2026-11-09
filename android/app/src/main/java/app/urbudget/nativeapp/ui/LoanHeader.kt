package app.urbudget.nativeapp.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ShowChart
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.AccountTypes
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.defaultLoanSource
import app.urbudget.nativeapp.nextLoanPayment
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.rateIn
import app.urbudget.nativeapp.rateText
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth
import java.util.UUID

/*
 * Loans live on their accounts, as on the desktop. A debt account's details (rate, payment, fees or
 * escrow, payment category and dated rate changes) are in its account form (LoanDetails.kt); its
 * register shows where it stands (LoanHeader below) and opens the payoff planner (LoanPlanner.kt),
 * which only reads; its payment category's sheet shows the loans it pays (CategoryLoans). The
 * maths is the shared engine's (loans.rs).
 */

/** What a loan's header, its payment category and its account menu can open (Planning.tsx's LoanActions). App provides them. */
@Stable
internal class LoanActions(
    /** Records a payment to the loan; without details, asks for them first. */
    val record: (JSONObject) -> Unit,
    /** Opens the account's register with its payoff planner; without details or anything owed, the details. */
    val plan: (JSONObject) -> Unit,
    /** Edits the account at its loan details. */
    val details: (JSONObject) -> Unit,
)
internal val LocalLoanActions = staticCompositionLocalOf<LoanActions?> { null }

/** A loan's projection from the engine; `waiting` until it's worked out. */
@Immutable
internal class Projection(val projection: JSONObject?, val error: String?, val waiting: Boolean)

/**
 * The loan's payments from `start` until it's paid off (loans.rs's project; Planning.tsx's
 * useProjection), with `extra` on top of each and `lump` once with the first. Nothing to project
 * when nothing is owed; a payment too small to cover the interest comes back as `error`.
 * `settle` waits that long first, so typing doesn't ask for every keystroke.
 */
@Composable
internal fun rememberProjection(vm: BudgetViewModel, owed: Long, terms: JSONObject?, start: String?, extra: Long = 0, lump: Long = 0, settle: Long = 0): Projection {
    val termsText = remember(terms) { terms?.toString() }
    val active = termsText != null && start != null && owed > 0
    val key = listOf(owed, termsText, start, extra, lump)
    val jobs = rememberPreviewJobs(key)
    var result by remember { mutableStateOf<ScopedResult<Projection>?>(null) }
    LaunchedEffect(key) {
        if (!active) return@LaunchedEffect
        if (settle > 0) delay(settle)
        val params = JSONObject().put("balance", owed).put("terms", JSONObject(termsText ?: return@LaunchedEffect)).put("start", start).put("extra", extra).put("lump", lump)
        jobs.track(vm.query("loanProjection", params, anyRevision = true, onError = { result = ScopedResult(key, Projection(null, it, false)) }) {
            result = ScopedResult(key, Projection(it, null, false))
        })
    }
    return if (!active) Projection(null, null, false) else result?.forScope(key) ?: Projection(null, null, true)
}

/** When a loan is paid off, as its header and its category say it ("Oct 2031"). */
@Composable
private fun payoffText(owed: Long, plan: Projection): String = when {
    owed == 0L -> stringResource(R.string.loan_status_paid_off)
    plan.projection != null -> shortMonthLabel(plan.projection.text("payoffDate").take(7))
    else -> "—"
}

/**
 * Where a loan stands, over its register (LoanHeader.tsx): what's owed, the rate and payment, when
 * it's paid off and the interest still to pay, with Record payment, the payoff planner and its
 * details. A debt account without loan details asks for them instead.
 */
@Composable
internal fun LoanHeader(account: JSONObject, budget: JSONObject, balance: Long, vm: BudgetViewModel, modifier: Modifier = Modifier) {
    val actions = LocalLoanActions.current ?: return
    val currency = budget.text("currency")
    val accountId = account.text("id")
    val terms = remember(budget, accountId) { budget.rows("loans").firstOrNull { it.text("accountId") == accountId } }
    val type = AccountTypes.infer(account, balance)
    val loanType = account.text("kind") == "tracking" && AccountTypes.isLoanType(type)
    val archived = account.optBoolean("archived")
    val owed = maxOf(0L, -balance)
    val today = remember { LocalDate.now().toString() }
    val start = rememberPrepared(budget, accountId) { nextLoanPayment(budget, accountId, today) }
    val plan = rememberProjection(vm, owed, terms, start)
    if (terms == null) {
        if (loanType && !archived) LoanPrompt(modifier) { actions.details(account) }
        return
    }
    val charges = when {
        terms.optLong("monthlyFees") <= 0 -> R.string.loan_interest_added
        type == "mortgage" -> R.string.loan_interest_escrow_added
        else -> R.string.loan_interest_fees_added
    }
    LoanFacts(money(owed, currency), Money.ltr("${rateText(rateIn(terms, today.take(7)))}%"), money(terms.optLong("monthlyPayment"), currency),
        payoffText(owed, plan), plan.projection?.let { money(it.optLong("totalInterest"), currency) } ?: if (owed > 0) "—" else money(0, currency), modifier) {
        if (owed > 0 && !archived) PrimaryButton(stringResource(R.string.loan_record_payment), { actions.record(account) }, Modifier.fillMaxWidth(), icon = Icons.Outlined.Payments)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            if (owed > 0) SecondaryButton(stringResource(R.string.loan_planner), { actions.plan(account) }, Modifier.weight(1f), icon = Icons.AutoMirrored.Outlined.ShowChart)
            SecondaryButton(stringResource(R.string.loan_details), { actions.details(account) }, Modifier.weight(1f), icon = Icons.Outlined.Edit)
        }
        Text(plan.error ?: stringResource(charges), style = MaterialTheme.typography.bodySmall, color = if (plan.error != null) ur.warning else ur.muted)
        if (!loanType) Text(stringResource(R.string.loan_not_loan_type, accountTypes().firstOrNull { it.first == type }?.second?.lowercase() ?: type),
            style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
}

/** A debt with no loan details yet: what they'd show, and a way to add them. */
@Composable
private fun LoanPrompt(modifier: Modifier, add: () -> Unit) {
    ListCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row {
                Icon(Icons.Outlined.AccountBalance, null, Modifier.padding(top = 2.dp).size(20.dp), tint = ur.accent)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.loan_prompt_title), style = MaterialTheme.typography.titleSmall, color = ur.text)
                    Text(stringResource(R.string.loan_prompt_detail), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                }
            }
            SecondaryButton(stringResource(R.string.loan_add_details), add, Modifier.fillMaxWidth(), icon = Icons.Outlined.Add)
        }
    }
}

/**
 * A loan's payment category, in its sheet (LoanHeader.tsx's CategoryLoans): each loan it pays,
 * with what's owed, the payoff date and the payment, recording a payment, the planner, and the
 * payment as the category's target. `close` closes the sheet before an action opens elsewhere.
 */
@Composable
internal fun CategoryLoans(category: JSONObject, budget: JSONObject, summary: JSONObject?, vm: BudgetViewModel, close: () -> Unit) {
    val id = category.text("id")
    val loans = remember(budget, id) {
        budget.rows("loans").mapNotNull { terms -> if (terms.text("categoryId") != id) null else budget.row("accounts", terms.text("accountId"))?.let { terms to it } }
    }
    loans.forEach { (terms, account) -> key(account.text("id")) { CategoryLoan(category, terms, account, budget, summary, vm, close) } }
}

@Composable
private fun CategoryLoan(category: JSONObject, terms: JSONObject, account: JSONObject, budget: JSONObject, summary: JSONObject?, vm: BudgetViewModel, close: () -> Unit) {
    val actions = LocalLoanActions.current
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val accountId = account.text("id")
    val balance = remember(summary, accountId) { summary?.rows("accounts")?.firstOrNull { it.text("id") == accountId }?.optLong("balance") } ?: account.optLong("openingBalance")
    val owed = maxOf(0L, -balance)
    val today = remember { LocalDate.now().toString() }
    val start = rememberPrepared(budget, accountId) { nextLoanPayment(budget, accountId, today) }
    val plan = rememberProjection(vm, owed, terms, start)
    val payment = terms.optLong("monthlyPayment")
    val targeted = category.optLong("target") == payment && category.optJSONObject("goal")?.text("goal_type") == "DEBT"
    // A monthly debt payment target, as an imported one is saved.
    fun useAsTarget() = withCurrentBudget(budget, vm) {
        val existing = category.optJSONObject("goal")?.takeIf { it.text("goal_type") == "DEBT" }
        val goal = (existing?.copyJson() ?: JSONObject().put("goal_creation_month", "${YearMonth.now()}-01"))
            .put("goal_type", "DEBT").put("goal_needs_whole_amount", JSONObject.NULL).put("goal_target", Math.multiplyExact(payment, 10L))
        vm.upsert("categories", category.copyJson().put("target", payment).put("goal", goal))
        toast(Words[R.string.loan_target_set, category.text("name"), money(payment, currency), account.text("name")], true)
    }
    Text(stringResource(R.string.loan_pays, account.text("name")), Modifier.padding(start = Gutter + 4.dp, end = Gutter, top = 16.dp, bottom = 6.dp),
        style = MaterialTheme.typography.titleSmall, color = ur.text)
    ListCard(Modifier.padding(horizontal = Gutter)) {
        ListRow(stringResource(R.string.loan_still_owed), value = money(owed, currency), divider = true)
        ListRow(stringResource(R.string.loan_paid_off), value = payoffText(owed, plan), divider = true)
        ListRow(stringResource(R.string.loan_monthly_payment), value = money(payment, currency), divider = !targeted)
        if (!targeted) ListRow(stringResource(R.string.loan_use_as_target), titleColor = ur.accent, icon = Icons.Outlined.TrackChanges, onClick = ::useAsTarget, chevron = false)
    }
    if (actions != null && owed > 0) Row(Modifier.fillMaxWidth().padding(horizontal = Gutter).padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        if (!account.optBoolean("archived")) PrimaryButton(stringResource(R.string.loan_record_payment), { close(); actions.record(account) }, Modifier.weight(1f))
        SecondaryButton(stringResource(R.string.loan_open_planner), { close(); actions.plan(account) }, Modifier.weight(1f))
    }
}

/** Where a loan stands: what's owed, the rate and payment, when it's paid off and the interest still to pay (LoanHeader.tsx). */
@Composable
internal fun LoanFacts(owed: String, rate: String, payment: String, payoff: String, interest: String, modifier: Modifier = Modifier,
                       action: (@Composable () -> Unit)? = null) {
    ListCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row { Figure(stringResource(R.string.loan_owed), owed, Modifier.weight(1f)); Figure(stringResource(R.string.loan_rate), rate, Modifier.weight(1f)); Figure(stringResource(R.string.loan_payment), payment, Modifier.weight(1f)) }
            Row { Figure(stringResource(R.string.loan_paid_off), payoff, Modifier.weight(1f)); Figure(stringResource(R.string.loan_interest_to_pay), interest, Modifier.weight(2f)) }
            action?.invoke()
        }
    }
}

/** A labelled value that opens a picker of its own, such as the category sheet. */
@Composable
internal fun PickerField(label: String, value: String?, placeholder: String, onClick: () -> Unit, onClear: (() -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(fieldFill()).clickable(onClickLabel = stringResource(R.string.choose_field, label), onClick = onClick)
        .padding(start = 16.dp, end = 6.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodySmall, color = ur.muted)
            Text(value ?: placeholder, style = MaterialTheme.typography.bodyLarge, color = if (value != null) ur.text else ur.muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (onClear != null && value != null) IconButton(onClear) { Icon(Icons.Outlined.Close, stringResource(R.string.clear_field, label), tint = ur.muted) }
        else Icon(Icons.Outlined.UnfoldMore, null, Modifier.padding(end = 4.dp), tint = ur.muted)
    }
}

/**
 * Recording a loan payment (LoanPayment.tsx): a transfer from a budget account to the loan,
 * spending its payment category, with the saved loan details. The month's interest and fees go
 * onto the loan with its first payment that month (loans.rs's record).
 */
@Composable
internal fun LoanPaymentSheet(loan: JSONObject, terms: JSONObject, budget: JSONObject, summary: JSONObject?, vm: BudgetViewModel, close: () -> Unit) {
    val openedBudget = remember { budget }
    val toast = LocalToast.current
    val currency = budget.text("currency")
    val loanId = loan.text("id")
    // Stable across a retry, so a save that failed late records one payment, not two.
    val paymentId = remember { UUID.randomUUID().toString() }
    val balances = remember(summary) { summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty() }
    val sources = remember(budget) { budget.rows("accounts").filter { it.text("kind") == "cash" && !it.optBoolean("archived") && AccountTypes.onBudget(it) } }
    val sourceChoices = remember(sources, balances) { sources.map { it.text("id") to "${it.text("name")} (${money(balances[it.text("id")] ?: 0L, currency)})" } }
    val categoryNames = remember(budget) { budget.rows("categories").associate { it.text("id") to it.text("name") } }
    var from by remember { mutableStateOf("") }
    LaunchedEffect(Unit) {
        val chosen = withContext(Dispatchers.Default) { defaultLoanSource(openedBudget, loanId, sources, balances) }
        if (from.isBlank()) from = chosen
    }
    val pad = rememberPad(terms.optLong("monthlyPayment"))
    var keypad by remember { mutableStateOf(false) }
    var date by remember { mutableStateOf(LocalDate.now().toString()) }
    var category by remember { mutableStateOf(terms.text("categoryId").takeIf { it in categoryNames }.orEmpty()) }
    var picking by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val owed = maxOf(0L, -(balances[loanId] ?: loan.optLong("openingBalance")))
    val mortgage = AccountTypes.infer(loan, balances[loanId] ?: loan.optLong("openingBalance")) == "mortgage"
    val note = when {
        terms.optLong("monthlyFees") <= 0 -> R.string.loan_payment_note_interest
        mortgage -> R.string.loan_payment_note_escrow
        else -> R.string.loan_payment_note_fees
    }
    val save = {
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            val amount = pad.cents ?: error(Words[R.string.transaction_finish_sum])
            require(from.isNotBlank()) { Words[R.string.loan_payment_account_required] }
            require(amount > 0) { Words[R.string.loan_payment_amount_required] }
            require(category.isNotBlank()) { Words[R.string.loan_category_required] }
            LocalDate.parse(date)
            // A category chosen here becomes the loan's payment category when it had none.
            val missing = terms.text("categoryId") !in categoryNames
            val name = loan.text("name")
            vm.recordLoanPayment(JSONObject().put("terms", terms.copyJson().put("categoryId", category)).put("from", from)
                .put("amount", amount).put("date", date).put("id", paymentId), saveCategory = missing) {
                close()
                toast(if (date > LocalDate.now().toString()) Words[R.string.loan_paid_toast_date, money(amount, currency), name, Money.date(date)]
                    else Words[R.string.loan_paid_toast, money(amount, currency), name], true)
            }
        }.onFailure { error = it.message }
        Unit
    }
    UrSheet(close) {
        SheetHeader(stringResource(R.string.loan_pay_title, loan.text("name")), close, stringResource(R.string.loan_pay_amount, money(pad.cents ?: 0L, currency)), save, enabled = sources.isNotEmpty() && (pad.cents ?: 0L) > 0)
        Column(Modifier.fillMaxWidth().weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 18.dp).padding(bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row { Figure(stringResource(R.string.loan_owed_on_loan), money(owed, currency), Modifier.weight(1f)); Figure(stringResource(R.string.loan_monthly_payment), money(terms.optLong("monthlyPayment"), currency), Modifier.weight(1f)) }
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(if (keypad) ur.accentSoft else ur.surface2)
                .clickable(onClickLabel = stringResource(R.string.amount_edit)) { keypad = true; pad.fresh = true }.padding(vertical = 14.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                PadAmount(pad, currency)
                Text(if (keypad) stringResource(R.string.amount_type) else stringResource(R.string.amount_tap_to_change), color = ur.muted, style = MaterialTheme.typography.bodySmall)
            }
            if (sources.isEmpty()) Notice(stringResource(R.string.loan_needs_cash_account), tone = "info")
            else Choice(stringResource(R.string.date_from), from, sourceChoices) { from = it }
            DateField(date, { date = it })
            PickerField(stringResource(R.string.category_label), categoryNames[category], stringResource(R.string.loan_payment_category_hint), { keypad = false; picking = true })
            Text(stringResource(note, sources.firstOrNull { it.text("id") == from }?.text("name") ?: stringResource(R.string.loan_account_you_choose), loan.text("name")),
                style = MaterialTheme.typography.bodySmall, color = ur.muted)
            FormError(error)
        }
        AnimatedVisibility(keypad, enter = slideInVertically { it } + fadeIn(), exit = slideOutVertically { it } + fadeOut()) {
            AmountKeypad(pad, { keypad = false })
        }
    }
    if (picking) CategoryPickerSheet(budget, summary, date.take(7), category, stringResource(R.string.loan_payment_category), includePayment = false,
        onPick = { category = it; picking = false }) { picking = false }
}
