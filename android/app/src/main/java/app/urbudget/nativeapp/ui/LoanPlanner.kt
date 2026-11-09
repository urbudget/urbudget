package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.AccountTypes
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.LegendItem
import app.urbudget.nativeapp.LineSeries
import app.urbudget.nativeapp.LinesModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.ReportColors
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.compactMoney
import app.urbudget.nativeapp.linesModel
import app.urbudget.nativeapp.monthXTick
import app.urbudget.nativeapp.nextLoanPayment
import app.urbudget.nativeapp.plainTick
import app.urbudget.nativeapp.rateIn
import app.urbudget.nativeapp.rateText
import app.urbudget.nativeapp.span
import kotlinx.coroutines.delay
import org.json.JSONObject
import java.time.LocalDate

/** A typed planner amount: blank is 0, null when it doesn't read. */
private fun plannerAmount(text: String): Long? = if (text.isBlank()) 0L else runCatching { maxOf(0L, amountValue(text)) }.getOrNull()

private data class CompareRow(val label: String, val current: String, val changed: String?)
private data class PaymentLine(val key: String, val date: String, val payment: String, val detail: String)
/** A year of the payment schedule, one card. */
private data class ScheduleYear(val year: String, val lines: List<PaymentLine>)

/** The planner's figures, formatted away from composition. */
private data class PlanView(val rows: List<CompareRow>, val savings: List<Pair<String, String>>, val chart: LinesModel?, val legend: List<LegendItem>,
                            val years: List<ScheduleYear>, val payments: Int, val scheduleTitle: String)

/** The balance after each payment, the current plan against the changes, from today's (LoanPlanner.tsx's PayoffChart). */
private fun payoffChart(owed: Long, start: Long, base: JSONObject, then: JSONObject?, currency: String): LinesModel? {
    val longest = listOfNotNull(base, then).maxBy { it.rows("schedule").size }
    val dates = longest.rows("schedule").map { it.text("date") }
    if (dates.isEmpty()) return null
    // The first point is now; then one per payment, keyed by its date.
    val keys = listOf("now") + dates
    fun values(p: JSONObject, from: Long): List<Double?> {
        val schedule = p.rows("schedule")
        return keys.indices.map { i -> if (i == 0) from.toDouble() else schedule.getOrNull(i - 1)?.optLong("balance")?.toDouble() }
    }
    val series = listOfNotNull(LineSeries("current", Words[R.string.loan_current_plan], ReportColors.OTHER, values(base, owed), dashed = then != null),
        then?.let { LineSeries("changed", Words[R.string.loan_with_changes], 0, values(it, start)) })
    // Every month's name when there are few, else each new year.
    val many = keys.size > 25
    val unit = Money.format(0, currency).takeIf { text -> text.any { !it.isDigit() && it !in ".,' - " } }?.let { currency }
    // A plan ahead has no month under way.
    return linesModel(Words[R.string.loan_chart_label], keys, series, { Money.format(Math.round(it), currency) }, { compactMoney(it) }, unit, current = null,
        xTick = { key, _ ->
            when {
                key == "now" -> plainTick(Words[R.string.loan_now])
                many -> if (key.substring(5, 7) == "01") plainTick(key.take(4)) else null
                else -> monthXTick(key.take(7))
            }
        }, title = { key -> if (key == "now") Words[R.string.report_today] else Money.date(key) }, wash = then == null)
        // Both lines end at nothing owed: the readout above the chart says what each point is.
        .let { it.copy(endLabels = it.series.map { null }) }
}

private fun buildView(base: JSONObject, then: JSONObject?, changed: Boolean, paidNow: Boolean, owed: Long, left: Long, fees: Long, feeLabel: String, currency: String): PlanView {
    fun payoff(p: JSONObject) = monthLabel(p.text("payoffDate").take(7))
    // Each plan's figures; the total includes what's owed now (a one-off payment is part of it).
    val rows = buildList {
        fun row(label: String, value: (JSONObject) -> String) = add(CompareRow(label, value(base), if (!changed) null else then?.let(value) ?: if (paidNow) "—" else ""))
        row(Words[R.string.loan_paid_off]) { payoff(it) }
        row(Words[R.string.loan_time_left]) { span(it.optInt("months")) }
        row(Words[R.string.loan_interest]) { money(it.optLong("totalInterest"), currency) }
        if (fees > 0) row(feeLabel) { money(it.optLong("totalFees"), currency) }
        row(Words[R.string.loan_total_cost]) { money(owed + it.optLong("totalInterest") + it.optLong("totalFees"), currency) }
    }
    val savings = if (changed && (then != null || paidNow)) listOf(
        Words[R.string.loan_interest_saved] to money(base.optLong("totalInterest") - (then?.optLong("totalInterest") ?: 0L), currency),
        Words[R.string.loan_time_saved] to span(base.optInt("months") - (then?.optInt("months") ?: 0)),
        Words[R.string.loan_paid_off] to if (paidNow || then == null) Words[R.string.loan_now] else payoff(then)) else emptyList()
    val legend = if (then == null) emptyList() else listOf(LegendItem(Words[R.string.loan_current_plan], ReportColors.OTHER, line = true, dashed = true),
        LegendItem(Words[R.string.loan_with_changes], 0, line = true))
    val shown = (then ?: base).rows("schedule")
    val years = shown.mapIndexed { index, r ->
        r.text("date").take(4) to PaymentLine("$index:${r.text("date")}", Money.date(r.text("date")), money(r.optLong("payment"), currency),
            Words[R.string.loan_schedule_line, money(r.optLong("interest"), currency), feeLabel, money(r.optLong("fees"), currency), money(r.optLong("principal"), currency), money(r.optLong("balance"), currency)])
    }.groupBy({ it.first }, { it.second }).map { (year, lines) -> ScheduleYear(year, lines) }
    return PlanView(rows, savings, payoffChart(owed, left, base, then, currency), legend, years, shown.size,
        Words.count(if (then != null) R.plurals.loan_schedule_title_changes else R.plurals.loan_schedule_title, shown.size.toLong()))
}

/**
 * Trying out ways to pay a loan off sooner (LoanPlanner.tsx): more each month, a one-off payment
 * now, or the payment that clears it by a date. A sandbox: it never changes the budget or the
 * loan's details, which the account form edits. The maths is the shared engine's.
 */
@Composable
internal fun LoanPlannerSheet(account: JSONObject, terms: JSONObject, budget: JSONObject, summary: JSONObject?, vm: BudgetViewModel, close: () -> Unit) {
    val currency = budget.text("currency")
    val accountId = account.text("id")
    val today = remember { LocalDate.now().toString() }
    val balance = remember(summary, accountId) { summary?.rows("accounts")?.firstOrNull { it.text("id") == accountId }?.optLong("balance") } ?: account.optLong("openingBalance")
    val owed = maxOf(0L, -balance)
    val start = rememberPrepared(budget, accountId) { nextLoanPayment(budget, accountId, today) }
    var extraText by remember { mutableStateOf("") }
    var oneOffText by remember { mutableStateOf("") }
    var by by remember { mutableStateOf("") }
    var showAll by remember { mutableStateOf(false) }
    val extra = plannerAmount(extraText)
    val oneOff = plannerAmount(oneOffText)
    // Recording a payment adds the month's interest and fees with the month's first payment. While
    // this month's is still to come, a one-off now goes in with it, after them; once it's in, they're
    // charged and the one-off comes off what's owed.
    val firstThisMonth = start?.take(7) == today.take(7)
    val lump = if (firstThisMonth) oneOff ?: 0L else 0L
    val left = if (firstThisMonth) owed else maxOf(0L, owed - (oneOff ?: 0L))
    val changed = (extra ?: 0L) > 0 || (oneOff ?: 0L) > 0
    val base = rememberProjection(vm, owed, terms, start)
    val scenario = rememberProjection(vm, if (changed) left else 0L, terms, start, extra ?: 0L, lump, settle = 200)
    val payment = terms.optLong("monthlyPayment")
    // The payment that clears what's left by the chosen date; the difference becomes the extra.
    val termsText = remember(terms) { terms.toString() }
    val neededKey = listOf(by, left, lump, start, termsText)
    val neededJobs = rememberPreviewJobs(neededKey)
    var needed by remember { mutableStateOf<ScopedResult<Result<Long>>?>(null) }
    LaunchedEffect(neededKey) {
        if (by.isBlank() || left <= 0 || start == null) return@LaunchedEffect
        delay(200)
        neededJobs.track(vm.query("loanPaymentByDate", JSONObject().put("balance", left).put("terms", JSONObject(termsText)).put("start", start).put("due", by).put("lump", lump),
            anyRevision = true, onError = { needed = ScopedResult(neededKey, Result.failure(IllegalStateException(it))) }) { value ->
            val amount = value.optLong("items")
            needed = ScopedResult(neededKey, Result.success(amount))
            val more = amount - payment
            extraText = if (more > 0) amountText(more) else ""
        })
    }
    val now = base.projection
    val then = if (changed && left > 0) scenario.projection else null
    // Paid off now: the one-off alone clears the loan, with any interest and fees it brings.
    val paidNow = changed && (if (then != null) then.optInt("months") == 1 &&
        (oneOff ?: 0L) >= (then.rows("schedule").firstOrNull()?.optLong("payment") ?: Long.MAX_VALUE) else left == 0L)
    val mortgage = AccountTypes.infer(account, balance) == "mortgage"
    val feeLabel = stringResource(if (mortgage) R.string.loan_fee_escrow else R.string.loan_fee_fees)
    val fees = terms.optLong("monthlyFees")
    val language = languageKey()
    val view = rememberPrepared(now, then, changed, paidNow, owed, left, language) {
        now?.let { buildView(it, then, changed, paidNow, owed, left, fees, feeLabel, currency) }
    }
    val waiting = base.waiting || changed && left > 0 && scenario.waiting
    val neededResult = needed?.forScope(neededKey)

    LazyFormDialog(stringResource(R.string.loan_planner_title, account.text("name")), close, onConfirm = null) {
        item(key = "plan:facts") {
            ListCard {
                Row(Modifier.padding(16.dp)) {
                    Figure(stringResource(R.string.loan_owed), money(owed, currency), Modifier.weight(1f))
                    Figure(stringResource(R.string.loan_interest_rate), Money.ltr("${rateText(rateIn(terms, today.take(7)))}%"), Modifier.weight(1f))
                    Figure(stringResource(R.string.loan_monthly_payment), money(payment, currency), Modifier.weight(1f))
                }
            }
        }
        if (owed == 0L) item(key = "plan:paid") { Notice(stringResource(R.string.loan_nothing_owed), tone = "positive", icon = Icons.Outlined.CheckCircle) }
        item(key = "plan:extra", contentType = "field") { Field(extraText, { extraText = it }, stringResource(R.string.loan_extra_field, currency), true) }
        item(key = "plan:lump", contentType = "field") { Field(oneOffText, { oneOffText = it }, stringResource(R.string.loan_lump_field, currency), true) }
        item(key = "plan:by") {
            if (by.isBlank()) SecondaryButton(stringResource(R.string.loan_pay_off_by_date), { by = LocalDate.now().plusYears(5).toString() }, Modifier.fillMaxWidth(), icon = Icons.Outlined.Event)
            else Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.weight(1f)) { DateField(by, { by = it }, stringResource(R.string.loan_pay_off_by)) }
                IconButton({ by = ""; needed = null }) { Icon(Icons.Outlined.Close, stringResource(R.string.loan_clear_payoff_date), tint = ur.muted) }
            }
        }
        if (extra == null || oneOff == null) item(key = "plan:amount-error") { FormError(stringResource(R.string.loan_amounts_format)) }
        if (by.isNotBlank() && neededResult != null) item(key = "plan:needed") {
            val amount = neededResult.getOrNull()
            val month = monthLabel(by.take(7))
            Notice(when {
                amount == null -> neededResult.exceptionOrNull()?.message ?: stringResource(R.string.loan_no_payment_repays)
                amount > payment -> stringResource(R.string.loan_pay_by_needed, month, money(amount, currency), money(amount - payment, currency))
                else -> stringResource(R.string.loan_already_pays_by, month)
            }, tone = if (amount == null) "attention" else "info", icon = Icons.Outlined.Event)
        }
        base.error?.let { problem -> item(key = "plan:base-error") { FormError(problem) } }
        if (changed) scenario.error?.let { problem -> item(key = "plan:then-error") { FormError(problem) } }
        if (waiting) item(key = "plan:loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        view?.let { plan ->
            if (plan.savings.isNotEmpty()) item(key = "plan:savings") {
                ListCard(fill = ur.positiveSoft) {
                    // "7 years 1 month" and "September 2044" take a second line on a phone.
                    Row(Modifier.padding(16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        plan.savings.forEach { (label, value) -> Figure(label, value, Modifier.weight(1f), color = ur.positive, lines = 2) }
                    }
                }
            }
            item(key = "plan:compare") {
                ListCard {
                    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 4.dp)) {
                        Spacer(Modifier.weight(1.1f))
                        Text(stringResource(R.string.loan_current_plan), Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, color = ur.muted)
                        if (changed) Text(stringResource(R.string.loan_with_changes), Modifier.weight(1f), style = MaterialTheme.typography.labelMedium, color = ur.accent)
                    }
                    plan.rows.forEachIndexed { index, row ->
                        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 9.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(row.label, Modifier.weight(1.1f), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                            Text(row.current, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium.merge(Tabular), color = ur.text, fontWeight = FontWeight.SemiBold)
                            if (row.changed != null) Text(row.changed, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium.merge(Tabular), color = ur.accent, fontWeight = FontWeight.SemiBold)
                        }
                        if (index < plan.rows.lastIndex) Hairline(start = 16.dp)
                    }
                    Spacer(Modifier.height(6.dp))
                }
            }
            plan.chart?.let { chart ->
                item(key = "plan:chart") {
                    ListCard {
                        Column(Modifier.padding(start = 12.dp, end = 12.dp, top = 12.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            LinesChart(chart, height = 200.dp)
                            if (plan.legend.isNotEmpty()) ChartLegend(plan.legend)
                        }
                    }
                }
            }
            if (plan.years.isNotEmpty()) {
                item(key = "plan:schedule-title") { Text(plan.scheduleTitle, style = MaterialTheme.typography.titleSmall, color = ur.text) }
                val shown = if (showAll) plan.years else plan.years.take(2)
                items(shown, key = { "plan:year:${it.year}" }, contentType = { "year" }) { year ->
                    Column {
                        Text(year.year, Modifier.padding(start = 4.dp, bottom = 6.dp), style = MaterialTheme.typography.labelMedium, color = ur.muted)
                        ListCard {
                            year.lines.forEachIndexed { index, line ->
                                Column(Modifier.padding(horizontal = 16.dp, vertical = 10.dp)) {
                                    Row {
                                        Text(line.date, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, color = ur.text, fontWeight = FontWeight.SemiBold)
                                        Text(line.payment, style = MaterialTheme.typography.bodyMedium.merge(Tabular), color = ur.text, fontWeight = FontWeight.SemiBold)
                                    }
                                    Text(line.detail, style = MaterialTheme.typography.bodySmall, color = ur.muted)
                                }
                                if (index < year.lines.lastIndex) Hairline(start = 16.dp)
                            }
                        }
                    }
                }
                if (plan.years.size > 2) item(key = "plan:schedule-more") {
                    TextButton({ showAll = !showAll }) { Text(if (showAll) stringResource(R.string.show_fewer) else countText(R.plurals.loan_show_all_payments, plan.payments), color = ur.accent, fontWeight = FontWeight.SemiBold) }
                }
            }
        }
        item(key = "plan:note") {
            Text(stringResource(R.string.loan_planner_note), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
    }
}
