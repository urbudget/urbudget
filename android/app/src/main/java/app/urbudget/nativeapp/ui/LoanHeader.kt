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
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.UnfoldMore
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.AccountTypes
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.defaultLoanSource
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.time.LocalDate
import java.util.UUID

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
            vm.runCommand("loanRecord", JSONObject().put("terms", terms.copyJson().put("categoryId", category)).put("from", from)
                .put("amount", amount).put("date", date).put("id", paymentId)) { close() }
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
