package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.AccountTypes
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.nextLoanPayment
import app.urbudget.nativeapp.parseRateChange
import app.urbudget.nativeapp.rateIn
import app.urbudget.nativeapp.rateText
import app.urbudget.nativeapp.span
import kotlinx.coroutines.delay
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth

/** A dated rate change as typed; `source` keeps any fields Android doesn't edit. */
private data class RateRow(val month: String, val rate: String, val source: JSONObject? = null)

/** The loan details as typed (LoanDetails.tsx's LoanDraft). */
private data class LoanDraft(val rate: String = "", val payment: String = "", val fees: String = "", val category: String = "", val rates: List<RateRow> = emptyList())

private fun draftOf(saved: JSONObject?): LoanDraft = if (saved == null) LoanDraft() else LoanDraft(
    rate = amountText(saved.optLong("annualRateBasisPoints")),
    payment = saved.optLong("monthlyPayment").takeIf { it > 0 }?.let(::amountText).orEmpty(),
    fees = saved.optLong("monthlyFees").takeIf { it > 0 }?.let(::amountText).orEmpty(),
    category = saved.text("categoryId"),
    rates = saved.rows("rateHistory").map { RateRow(it.text("month"), rateText(it.optLong("annualRateMilliPercent"), Money.decimalMark()), it) })

/** The typed details as loan terms, or a sentence saying what doesn't read. */
private fun termsOf(draft: LoanDraft, accountId: String, saved: JSONObject?, requireCategory: Boolean): JSONObject {
    require(accountId.isNotBlank()) { Words[R.string.loan_choose_account] }
    val payment = if (draft.payment.isBlank()) 0L else amountValue(draft.payment)
    require(payment > 0) { Words[R.string.loan_payment_positive] }
    val fees = if (draft.fees.isBlank()) 0L else amountValue(draft.fees)
    require(fees >= 0) { Words[R.string.loan_fees_positive] }
    val rate = if (draft.rate.isBlank()) 0L else runCatching { amountValue(draft.rate.trim().removeSuffix("%").trim()) }
        .getOrElse { throw IllegalArgumentException(Words[R.string.loan_rate_format]) }
    require(rate in 0..100_000) { Words[R.string.loan_rate_range] }
    val history = draft.rates.map { row ->
        val month = runCatching { YearMonth.parse(row.month).toString() }.getOrElse { throw IllegalArgumentException(Words[R.string.loan_rate_month_required]) }
        (row.source?.copyJson() ?: JSONObject()).put("month", month).put("annualRateMilliPercent", parseRateChange(row.rate, Money.decimalMark()))
    }.sortedBy { it.text("month") }
    require(history.map { it.text("month") }.distinct().size == history.size) { Words[R.string.loan_one_rate_per_month] }
    if (requireCategory) require(draft.category.isNotBlank()) { Words[R.string.loan_category_required] }
    return (saved?.copyJson() ?: JSONObject()).put("accountId", accountId).put("categoryId", draft.category)
        .put("annualRateBasisPoints", rate).put("monthlyPayment", payment).put("monthlyFees", fees).put("rateHistory", JSONArray(history))
}

/** A typed planner amount: blank is 0, null when it doesn't read. */
private fun plannerAmount(text: String): Long? = if (text.isBlank()) 0L else runCatching { maxOf(0L, amountValue(text)) }.getOrNull()

private data class CompareRow(val label: String, val current: String, val changed: String?)
private data class PaymentLine(val key: String, val date: String, val payment: String, val detail: String)
/** A year of the payment schedule, one card. */
private data class ScheduleYear(val year: String, val lines: List<PaymentLine>)

/** The planner's figures, formatted away from composition. */
private data class LoanView(val payoff: String, val interest: String, val rows: List<CompareRow>, val savings: List<Pair<String, String>>,
                            val years: List<ScheduleYear>, val payments: Int, val scheduleTitle: String)

private fun buildView(base: JSONObject, then: JSONObject?, changed: Boolean, paidNow: Boolean, owed: Long, fees: Long, feeLabel: String, currency: String): LoanView {
    fun payoff(p: JSONObject) = monthLabel(p.text("payoffDate").take(7))
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
    val shown = (then ?: base).rows("schedule")
    val years = shown.mapIndexed { index, r ->
        r.text("date").take(4) to PaymentLine("$index:${r.text("date")}", Money.date(r.text("date")), money(r.optLong("payment"), currency),
            Words[R.string.loan_schedule_line, money(r.optLong("interest"), currency), feeLabel, money(r.optLong("fees"), currency), money(r.optLong("principal"), currency), money(r.optLong("balance"), currency)])
    }.groupBy({ it.first }, { it.second }).map { (year, lines) -> ScheduleYear(year, lines) }
    return LoanView(payoff(base), money(base.optLong("totalInterest"), currency), rows, savings, years, shown.size,
        Words.count(if (then != null) R.plurals.loan_schedule_title_changes else R.plurals.loan_schedule_title, shown.size.toLong()))
}

/**
 * A loan: where it stands, its details (rate, payment, fees or escrow, payment category and dated
 * rate changes), and a payoff planner that only tries things out (LoanPlanner.tsx). Recording a
 * payment uses the saved details, never unsaved ones; the maths is the shared engine's.
 * `original` is a loan's saved details, or just {accountId} for a debt that has none yet.
 */
@Composable
internal fun LoanDialog(original: JSONObject?, budget: JSONObject, summary: JSONObject?, vm: BudgetViewModel, close: () -> Unit) {
    val currency = budget.text("currency")
    val today = remember { LocalDate.now().toString() }
    val balances = remember(summary) { summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty() }
    val locked = original?.text("accountId")?.ifBlank { null }
    var account by remember { mutableStateOf(locked ?: "") }
    // Debts by loan type (desktop isLoanType), and any account with loan details saved.
    val loanAccounts = remember(budget, balances) {
        val saved = budget.rows("loans").map { it.text("accountId") }.toSet()
        budget.rows("accounts").filter { a ->
            val id = a.text("id")
            (id == locked || !a.optBoolean("archived")) && AccountTypes.hasLoan(a, balances[id] ?: a.optLong("openingBalance"), saved)
        }
    }
    LaunchedEffect(loanAccounts) { if (account.isBlank()) loanAccounts.firstOrNull()?.text("id")?.let { account = it } }
    val accountRecord = remember(budget, account) { budget.row("accounts", account) }
    val saved = remember(budget, account) { budget.rows("loans").firstOrNull { it.text("accountId") == account } }
    var baseline by remember { mutableStateOf(draftOf(saved)) }
    var draft by remember { mutableStateOf(baseline) }
    var openedBudget by remember { mutableStateOf(budget) }
    val dirty = draft != baseline
    // Without unsaved edits the form follows the budget (a payment just recorded, a sync).
    LaunchedEffect(budget, account) {
        val fresh = draftOf(saved)
        if (draft == baseline) { baseline = fresh; draft = fresh; openedBudget = budget }
    }
    var extraText by remember { mutableStateOf("") }
    var oneOffText by remember { mutableStateOf("") }
    var by by remember { mutableStateOf("") }
    var showAll by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf(false) }
    var paying by remember { mutableStateOf(false) }
    var confirmRemove by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    retainEditorDraft(vm, "loan", openedBudget, JSONObject().put("account", account).put("rate", draft.rate).put("payment", draft.payment)
        .put("fees", draft.fees).put("category", draft.category).put("extra", extraText).put("oneOff", oneOffText)
        .put("rates", org.json.JSONArray(draft.rates.map { JSONObject().put("month", it.month).put("rate", it.rate) })))
    val balance = balances[account] ?: accountRecord?.optLong("openingBalance") ?: 0L
    val owed = maxOf(0L, -balance)
    val mortgage = accountRecord != null && AccountTypes.infer(accountRecord, balance) == "mortgage"
    val feeLabel = stringResource(if (mortgage) R.string.loan_fee_escrow else R.string.loan_fee_fees)
    val start = rememberPrepared(budget, account) { if (account.isBlank()) null else nextLoanPayment(budget, account, today) }
    val language = languageKey()
    val terms = remember(draft, account, saved, language) { runCatching { termsOf(draft, account, saved, requireCategory = false) } }
    val termsValue = terms.getOrNull()
    val termsText = remember(termsValue) { termsValue?.toString() }
    val extra = plannerAmount(extraText)
    val oneOff = plannerAmount(oneOffText)
    // Recording a payment adds the month's interest and fees with the month's first payment. While
    // this month's is still to come, a one-off now goes in with it, after them; once it's in, they're
    // charged and the one-off comes off what's owed.
    val firstThisMonth = start?.take(7) == today.take(7)
    val lump = if (firstThisMonth) oneOff ?: 0L else 0L
    val left = if (firstThisMonth) owed else maxOf(0L, owed - (oneOff ?: 0L))
    val changed = (extra ?: 0L) > 0 || (oneOff ?: 0L) > 0

    val planKey = remember(budget, termsText, start, owed, left, extra, lump, changed) { Any() }
    val planJobs = rememberPreviewJobs(planKey)
    var base by remember { mutableStateOf<ScopedResult<Result<JSONObject>>?>(null) }
    var scenario by remember { mutableStateOf<ScopedResult<Result<JSONObject>>?>(null) }
    LaunchedEffect(planKey) {
        val t = termsValue ?: return@LaunchedEffect
        if (start == null || owed <= 0) return@LaunchedEffect
        delay(200)
        val params = JSONObject().put("balance", owed).put("terms", t).put("start", start)
        planJobs.track(vm.query("loanProjection", params, onError = { base = ScopedResult(planKey, Result.failure(IllegalStateException(it))) }) {
            base = ScopedResult(planKey, Result.success(it))
        })
        if (changed && left > 0) planJobs.track(vm.query("loanProjection", JSONObject().put("balance", left).put("terms", t).put("start", start).put("extra", extra ?: 0L).put("lump", lump),
            onError = { scenario = ScopedResult(planKey, Result.failure(IllegalStateException(it))) }) { scenario = ScopedResult(planKey, Result.success(it)) })
    }
    // The payment that clears what's left by the chosen date; the difference becomes the extra.
    val neededKey = remember(by, left, lump, start, termsText) { Any() }
    val neededJobs = rememberPreviewJobs(neededKey)
    var needed by remember { mutableStateOf<ScopedResult<Result<Long>>?>(null) }
    LaunchedEffect(neededKey) {
        val t = termsValue ?: return@LaunchedEffect
        if (by.isBlank() || left <= 0 || start == null) return@LaunchedEffect
        delay(200)
        neededJobs.track(vm.query("loanPaymentByDate", JSONObject().put("balance", left).put("terms", t).put("start", start).put("due", by).put("lump", lump),
            onError = { needed = ScopedResult(neededKey, Result.failure(IllegalStateException(it))) }) { value ->
            val amount = value.optLong("items")
            needed = ScopedResult(neededKey, Result.success(amount))
            val more = amount - t.optLong("monthlyPayment")
            extraText = if (more > 0) amountText(more) else ""
        })
    }
    val now = base?.forScope(planKey)
    val then = if (changed && left > 0) scenario?.forScope(planKey) else null
    val baseProjection = now?.getOrNull()
    val thenProjection = then?.getOrNull()
    val paidNow = changed && (if (thenProjection != null) thenProjection.optInt("months") == 1 &&
        (oneOff ?: 0L) >= (thenProjection.rows("schedule").firstOrNull()?.optLong("payment") ?: Long.MAX_VALUE) else left == 0L)
    val fees = termsValue?.optLong("monthlyFees") ?: 0L
    val view = rememberPrepared(baseProjection, thenProjection, changed, paidNow, owed, language) {
        baseProjection?.let { buildView(it, thenProjection, changed, paidNow, owed, fees, feeLabel, currency) }
    }
    val waiting = termsValue != null && owed > 0 && start != null && (now == null || changed && left > 0 && then == null)
    val neededResult = needed?.forScope(neededKey)
    val categoryNames = remember(budget) { budget.rows("categories").associate { it.text("id") to it.text("name") } }
    val accountChoices = remember(loanAccounts) { loanAccounts.map { it.text("id") to it.text("name") } }

    fun save() {
        runCatching {
            requireCurrentBudget(openedBudget, vm)
            if (dirty || saved == null) vm.upsert("loans", termsOf(draft, account, saved, requireCategory = true))
        }.onSuccess { close() }.onFailure { error = it.message }
    }

    LazyFormDialog(accountRecord?.text("name")?.let { stringResource(R.string.loan_title_named, it) } ?: stringResource(R.string.loan_title), close, confirm = stringResource(R.string.common_save),
        onConfirm = ::save, enabled = account.isNotBlank() && termsValue != null) {
        if (locked == null) item(key = "loan:account") {
            if (loanAccounts.isEmpty()) Notice(stringResource(R.string.loan_needs_account), tone = "info")
            else Choice(stringResource(R.string.loan_debt_account), account, accountChoices) {
                if (it != account) {
                    account = it
                    val fresh = draftOf(budget.rows("loans").firstOrNull { loan -> loan.text("accountId") == it })
                    baseline = fresh; draft = fresh; openedBudget = budget; extraText = ""; oneOffText = ""; by = ""; error = null
                }
            }
        }
        error?.let { item(key = "loan:error") { FormError(it) } }
        if (account.isNotBlank()) item(key = "loan:facts") {
            val rate = termsValue?.let { Money.ltr("${rateText(rateIn(it, today.take(7)))}%") } ?: "—"
            LoanFacts(money(owed, currency), rate, termsValue?.let { money(it.optLong("monthlyPayment"), currency) } ?: "—",
                when { owed == 0L -> stringResource(R.string.loan_status_paid_off); view != null -> view.payoff; else -> "—" }, when { owed == 0L -> money(0, currency); view != null -> view.interest; else -> "—" }) {
                if (owed > 0 && accountRecord?.optBoolean("archived") != true) {
                    PrimaryButton(stringResource(R.string.loan_record_payment), { paying = true }, Modifier.fillMaxWidth(), enabled = saved != null && !dirty, icon = Icons.Outlined.Payments)
                    Text(when {
                        saved == null -> stringResource(R.string.loan_save_first)
                        dirty -> stringResource(R.string.loan_save_changes_first)
                        fees > 0 -> stringResource(if (mortgage) R.string.loan_interest_escrow_added else R.string.loan_interest_fees_added)
                        else -> stringResource(R.string.loan_interest_added)
                    }, style = MaterialTheme.typography.bodySmall, color = ur.muted)
                }
            }
        }
        item(key = "loan:details-title") { SectionTitle(stringResource(R.string.loan_details), stringResource(R.string.loan_details_detail)) }
        item(key = "loan:rate", contentType = "field") { Field(draft.rate, { draft = draft.copy(rate = it) }, stringResource(R.string.loan_rate_field), true) }
        item(key = "loan:payment", contentType = "field") { Field(draft.payment, { draft = draft.copy(payment = it) }, stringResource(R.string.loan_payment_field, currency), true) }
        item(key = "loan:fees", contentType = "field") { Field(draft.fees, { draft = draft.copy(fees = it) }, stringResource(if (mortgage) R.string.loan_escrow_field else R.string.loan_fees_field, currency), true) }
        item(key = "loan:category") {
            PickerField(stringResource(R.string.loan_payment_category), categoryNames[draft.category], stringResource(R.string.loan_payment_category_hint), { picking = true })
        }
        item(key = "loan:rates-title") {
            Column {
                Text(stringResource(R.string.loan_rate_changes), style = MaterialTheme.typography.titleSmall, color = ur.text)
                Text(stringResource(R.string.loan_rate_changes_detail), style = MaterialTheme.typography.bodySmall, color = ur.muted)
            }
        }
        itemsIndexed(draft.rates, key = { index, _ -> "loan:rate:$index" }, contentType = { _, _ -> "rate" }) { index, row ->
            ListCard {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    MonthField(row.month, { month -> draft = draft.copy(rates = draft.rates.mapIndexed { i, r -> if (i == index) r.copy(month = month) else r }) }, stringResource(R.string.date_from))
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.weight(1f)) {
                            Field(row.rate, { rate -> draft = draft.copy(rates = draft.rates.mapIndexed { i, r -> if (i == index) r.copy(rate = rate) else r }) }, stringResource(R.string.loan_new_rate_field), true)
                        }
                        IconButton({ draft = draft.copy(rates = draft.rates.filterIndexed { i, _ -> i != index }) }) {
                            Icon(Icons.Outlined.DeleteOutline, stringResource(R.string.loan_remove_rate, monthLabel(row.month)), tint = ur.negative)
                        }
                    }
                }
            }
        }
        item(key = "loan:add-rate") {
            TextButton({
                val month = YearMonth.now().toString()
                draft = draft.copy(rates = draft.rates + RateRow(if (draft.rates.any { it.month == month }) YearMonth.now().plusMonths(1).toString() else month, draft.rate))
            }) { Icon(Icons.Outlined.Add, null, tint = ur.accent); Spacer(Modifier.width(6.dp)); Text(stringResource(R.string.loan_add_rate_change), color = ur.accent, fontWeight = FontWeight.SemiBold) }
        }
        terms.exceptionOrNull()?.let { problem -> if (draft != LoanDraft()) item(key = "loan:terms-error") { FormError(problem.message) } }
        if (dirty && saved != null) item(key = "loan:unsaved") { Notice(stringResource(R.string.loan_unsaved), tone = "info") }

        item(key = "loan:planner-title") { SectionTitle(stringResource(R.string.loan_planner), stringResource(R.string.loan_planner_detail)) }
        if (owed == 0L && account.isNotBlank()) item(key = "loan:paid") { Notice(stringResource(R.string.loan_nothing_owed), tone = "positive", icon = Icons.Outlined.CheckCircle) }
        item(key = "loan:extra", contentType = "field") { Field(extraText, { extraText = it }, stringResource(R.string.loan_extra_field, currency), true) }
        item(key = "loan:lump", contentType = "field") { Field(oneOffText, { oneOffText = it }, stringResource(R.string.loan_lump_field, currency), true) }
        item(key = "loan:by") {
            if (by.isBlank()) SecondaryButton(stringResource(R.string.loan_pay_off_by_date), { by = LocalDate.now().plusYears(5).toString() }, Modifier.fillMaxWidth(), icon = Icons.Outlined.Event)
            else Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.weight(1f)) { DateField(by, { by = it }, stringResource(R.string.loan_pay_off_by)) }
                IconButton({ by = ""; needed = null }) { Icon(Icons.Outlined.Close, stringResource(R.string.loan_clear_payoff_date), tint = ur.muted) }
            }
        }
        if (extra == null || oneOff == null) item(key = "loan:amount-error") { FormError(stringResource(R.string.loan_amounts_format)) }
        if (by.isNotBlank() && neededResult != null && termsValue != null) item(key = "loan:needed") {
            val amount = neededResult.getOrNull()
            val month = monthLabel(by.take(7))
            Notice(when {
                amount == null -> neededResult.exceptionOrNull()?.message ?: stringResource(R.string.loan_no_payment_repays)
                amount > termsValue.optLong("monthlyPayment") -> stringResource(R.string.loan_pay_by_needed, month, money(amount, currency), money(amount - termsValue.optLong("monthlyPayment"), currency))
                else -> stringResource(R.string.loan_already_pays_by, month)
            }, tone = if (amount == null) "attention" else "info", icon = Icons.Outlined.Event)
        }
        now?.exceptionOrNull()?.let { problem -> item(key = "loan:base-error") { FormError(problem.message) } }
        then?.exceptionOrNull()?.let { problem -> item(key = "loan:then-error") { FormError(problem.message) } }
        if (waiting) item(key = "loan:loading") { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        view?.let { plan ->
            if (plan.savings.isNotEmpty()) item(key = "loan:savings") {
                ListCard(fill = ur.positiveSoft) {
                    Row(Modifier.padding(16.dp)) { plan.savings.forEach { (label, value) -> Figure(label, value, Modifier.weight(1f), color = ur.positive) } }
                }
            }
            item(key = "loan:compare") {
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
            if (plan.years.isNotEmpty()) {
                item(key = "loan:schedule-title") { Text(plan.scheduleTitle, style = MaterialTheme.typography.titleSmall, color = ur.text) }
                val shown = if (showAll) plan.years else plan.years.take(2)
                items(shown, key = { "loan:year:${it.year}" }, contentType = { "year" }) { year ->
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
                if (plan.years.size > 2) item(key = "loan:schedule-more") {
                    TextButton({ showAll = !showAll }) { Text(if (showAll) stringResource(R.string.show_fewer) else countText(R.plurals.loan_show_all_payments, plan.payments), color = ur.accent, fontWeight = FontWeight.SemiBold) }
                }
            }
        }
        item(key = "loan:note") {
            Text(stringResource(R.string.loan_note),
                style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
        if (saved != null) item(key = "loan:remove") {
            TextButton({ confirmRemove = true }) { Text(stringResource(R.string.loan_remove_details), color = ur.negative) }
        }
    }
    if (picking) CategoryPickerSheet(budget, summary, today.take(7), draft.category, stringResource(R.string.loan_payment_category), includePayment = false,
        onPick = { draft = draft.copy(category = it); picking = false }) { picking = false }
    if (paying && saved != null && accountRecord != null) LoanPaymentSheet(accountRecord, saved, budget, summary, vm) { paying = false }
    if (confirmRemove) AlertDialog(onDismissRequest = { confirmRemove = false }, containerColor = ur.surface, title = { Text(stringResource(R.string.loan_remove_title)) },
        text = { Text(stringResource(R.string.loan_remove_message, accountRecord?.text("name") ?: stringResource(R.string.account_this))) },
        confirmButton = { TextButton({ confirmRemove = false; withCurrentBudget(openedBudget, vm) { vm.remove("loans", account); close() } }) { Text(stringResource(R.string.common_remove), color = ur.negative) } },
        dismissButton = { TextButton({ confirmRemove = false }) { Text(stringResource(R.string.common_cancel)) } })
}
