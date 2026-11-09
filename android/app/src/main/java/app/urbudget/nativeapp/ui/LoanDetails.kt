package app.urbudget.nativeapp.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.parseRateChange
import app.urbudget.nativeapp.rateText
import kotlinx.coroutines.delay
import org.json.JSONArray
import org.json.JSONObject
import java.time.YearMonth

/** A dated rate change as typed; `source` keeps any fields Android doesn't edit. */
internal data class RateRow(val month: String, val rate: String, val source: JSONObject? = null)

/** A debt's loan details as typed in the account form (LoanDetails.tsx's LoanDraft). */
internal data class LoanDraft(val rate: String = "", val payment: String = "", val fees: String = "", val category: String = "", val rates: List<RateRow> = emptyList())

internal fun loanDraft(saved: JSONObject?): LoanDraft = if (saved == null) LoanDraft() else LoanDraft(
    rate = amountText(saved.optLong("annualRateBasisPoints")).takeIf { saved.optLong("annualRateBasisPoints") > 0 }.orEmpty(),
    payment = saved.optLong("monthlyPayment").takeIf { it > 0 }?.let(::amountText).orEmpty(),
    fees = saved.optLong("monthlyFees").takeIf { it > 0 }?.let(::amountText).orEmpty(),
    category = saved.text("categoryId"),
    rates = saved.rows("rateHistory").map { RateRow(it.text("month"), rateText(it.optLong("annualRateMilliPercent"), Money.decimalMark()), it) })

/**
 * The typed details as loan terms to save, or null for none: a blank payment, with nothing else
 * filled in, clears them (LoanDetails.tsx's loanTermsOf). Throws a sentence when something doesn't read.
 */
internal fun loanTermsOf(accountId: String, draft: LoanDraft, saved: JSONObject?): JSONObject? {
    val payment = if (draft.payment.isBlank()) 0L else amountValue(draft.payment)
    val fees = if (draft.fees.isBlank()) 0L else amountValue(draft.fees)
    val rate = if (draft.rate.isBlank()) 0L else runCatching { amountValue(draft.rate.trim().removeSuffix("%").trim()) }
        .getOrElse { throw IllegalArgumentException(Words[R.string.loan_rate_format]) }
    if (payment == 0L) {
        require(rate == 0L && fees == 0L && draft.rates.isEmpty()) { Words[R.string.loan_payment_or_clear] }
        return null
    }
    require(payment > 0) { Words[R.string.loan_payment_positive] }
    require(fees >= 0) { Words[R.string.loan_fees_positive] }
    require(rate in 0..100_000) { Words[R.string.loan_rate_range] }
    val history = draft.rates.map { row ->
        val month = runCatching { YearMonth.parse(row.month).toString() }.getOrElse { throw IllegalArgumentException(Words[R.string.loan_rate_month_required]) }
        (row.source?.copyJson() ?: JSONObject()).put("month", month).put("annualRateMilliPercent", parseRateChange(row.rate, Money.decimalMark()))
    }.sortedBy { it.text("month") }
    require(history.map { it.text("month") }.distinct().size == history.size) { Words[R.string.loan_one_rate_per_month] }
    return (saved?.copyJson() ?: JSONObject()).put("accountId", accountId).put("categoryId", draft.category)
        .put("annualRateBasisPoints", rate).put("monthlyPayment", payment).put("monthlyFees", fees).put("rateHistory", JSONArray(history))
}

/**
 * A debt's loan details in the account form (LoanDetails.tsx's LoanDetailsFields): what the payoff
 * date, the interest left and the interest added with each payment are worked out from.
 * `imported`: filled in from the YNAB import; `onUseImported` fills them in from it, when it has
 * terms to offer. `focus` starts in the rate field (opened from a loan's "Loan details").
 */
@Composable
internal fun LoanDetailsFields(budget: JSONObject, summary: JSONObject?, type: String, value: LoanDraft, onChange: (LoanDraft) -> Unit,
                               imported: Boolean, onUseImported: (() -> Unit)?, focus: Boolean) {
    val currency = budget.text("currency")
    val mortgage = type == "mortgage"
    var picking by remember { mutableStateOf(false) }
    var ratesOpen by remember { mutableStateOf(value.rates.isNotEmpty()) }
    val categoryNames = remember(budget) { budget.rows("categories").associate { it.text("id") to it.text("name") } }
    val rateFocus = remember { FocusRequester() }
    // Opened from a loan's details: start in its rate, as the desktop's form does. The sheet settles first.
    LaunchedEffect(focus) { if (focus) { delay(350); runCatching { rateFocus.requestFocus() } } }
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(stringResource(R.string.loan_details), Modifier.padding(top = 4.dp), style = MaterialTheme.typography.titleMedium, color = ur.text)
        if (imported) Text(stringResource(R.string.loan_imported), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        else if (onUseImported != null) Notice(stringResource(R.string.loan_importable), tone = "info", action = stringResource(R.string.loan_use_imported), onAction = onUseImported)
        Field(value.rate, { onChange(value.copy(rate = it)) }, stringResource(R.string.loan_rate_field), true, modifier = Modifier.focusRequester(rateFocus))
        Field(value.payment, { onChange(value.copy(payment = it)) }, stringResource(R.string.loan_payment_field, currency), true)
        Field(value.fees, { onChange(value.copy(fees = it)) }, stringResource(if (mortgage) R.string.loan_escrow_field else R.string.loan_fees_field, currency), true)
        PickerField(stringResource(R.string.loan_payment_category), categoryNames[value.category], stringResource(R.string.loan_payment_category_hint), { picking = true },
            onClear = if (value.category.isNotBlank()) ({ onChange(value.copy(category = "")) }) else null)
        // Dated rates for a variable-rate loan, folded away until wanted, as the desktop folds them.
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable(onClickLabel = stringResource(R.string.loan_rate_changes)) { ratesOpen = !ratesOpen }
            .padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.loan_rate_changes_count, Money.count(value.rates.size.toLong())), Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, color = ur.text)
            Icon(if (ratesOpen) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, null, tint = ur.muted)
        }
        if (ratesOpen) {
            Text(stringResource(R.string.loan_rate_changes_detail), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            value.rates.forEachIndexed { index, row ->
                ListCard {
                    Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        MonthField(row.month, { month -> onChange(value.copy(rates = value.rates.mapIndexed { i, r -> if (i == index) r.copy(month = month) else r })) }, stringResource(R.string.date_from))
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.weight(1f)) {
                                Field(row.rate, { rate -> onChange(value.copy(rates = value.rates.mapIndexed { i, r -> if (i == index) r.copy(rate = rate) else r })) }, stringResource(R.string.loan_new_rate_field), true)
                            }
                            IconButton({ onChange(value.copy(rates = value.rates.filterIndexed { i, _ -> i != index })) }) {
                                Icon(Icons.Outlined.DeleteOutline, stringResource(R.string.loan_remove_rate, monthLabel(row.month)), tint = ur.negative)
                            }
                        }
                    }
                }
            }
            TextButton({
                val month = YearMonth.now().toString()
                onChange(value.copy(rates = value.rates + RateRow(if (value.rates.any { it.month == month }) YearMonth.now().plusMonths(1).toString() else month, value.rate)))
            }) { Icon(Icons.Outlined.Add, null, tint = ur.accent); Spacer(Modifier.width(6.dp)); Text(stringResource(R.string.loan_add_rate_change), color = ur.accent, fontWeight = FontWeight.SemiBold) }
        }
        Text(stringResource(if (mortgage) R.string.loan_details_help_escrow else R.string.loan_details_help_fees), style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
    if (picking) CategoryPickerSheet(budget, summary, YearMonth.now().toString(), value.category, stringResource(R.string.loan_payment_category), includePayment = false,
        onPick = { onChange(value.copy(category = it)); picking = false }) { picking = false }
}
