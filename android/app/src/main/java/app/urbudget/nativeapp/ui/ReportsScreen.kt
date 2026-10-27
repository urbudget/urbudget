package app.urbudget.nativeapp.ui

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.CompareArrows
import androidx.compose.material.icons.automirrored.outlined.ReceiptLong
import androidx.compose.material.icons.automirrored.outlined.ShowChart
import androidx.compose.material.icons.outlined.AccountBalance
import androidx.compose.material.icons.outlined.AccountBalanceWallet
import androidx.compose.material.icons.outlined.BarChart
import androidx.compose.material.icons.outlined.CalendarMonth
import androidx.compose.material.icons.outlined.Category
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.FileDownload
import androidx.compose.material.icons.outlined.HourglassBottom
import androidx.compose.material.icons.outlined.Payments
import androidx.compose.material.icons.outlined.PieChart
import androidx.compose.material.icons.outlined.StackedLineChart
import androidx.compose.material.icons.outlined.Storefront
import androidx.compose.material.icons.outlined.SwapHoriz
import androidx.compose.material.icons.outlined.Timeline
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.BalancesView
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.DrillModel
import app.urbudget.nativeapp.DrillTarget
import app.urbudget.nativeapp.ForecastData
import app.urbudget.nativeapp.ForecastView
import app.urbudget.nativeapp.MonthRange
import app.urbudget.nativeapp.NEEDS_CATEGORY
import app.urbudget.nativeapp.OverviewModel
import app.urbudget.nativeapp.PayeeRow
import app.urbudget.nativeapp.PayeeView
import app.urbudget.nativeapp.RangePreset
import app.urbudget.nativeapp.ReportBudget
import app.urbudget.nativeapp.ReportData
import app.urbudget.nativeapp.ReportOptions
import app.urbudget.nativeapp.ReportScope
import app.urbudget.nativeapp.ReportView
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.SpendRow
import app.urbudget.nativeapp.UNCATEGORIZED_BUCKET
import app.urbudget.nativeapp.accountFilterGroups
import app.urbudget.nativeapp.balanceAccounts
import app.urbudget.nativeapp.categoryFilterGroups
import app.urbudget.nativeapp.defaultRange
import app.urbudget.nativeapp.describeRange
import app.urbudget.nativeapp.drillFromDetails
import app.urbudget.nativeapp.filterSummary
import app.urbudget.nativeapp.firstDay
import app.urbudget.nativeapp.forecastView
import app.urbudget.nativeapp.incomeDrill
import app.urbudget.nativeapp.lastDay
import app.urbudget.nativeapp.localeWeekStart
import app.urbudget.nativeapp.overviewModel
import app.urbudget.nativeapp.parseForecast
import app.urbudget.nativeapp.parseReport
import app.urbudget.nativeapp.parseReportBudget
import app.urbudget.nativeapp.reportView
import app.urbudget.nativeapp.shiftMonth
import app.urbudget.nativeapp.toggleChartAccount
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth
import kotlin.math.abs

/**
 * A report: its page name, its chip name where the group already says the rest, and whether the
 * Categories filter applies. Groups are kept by English key; the words are string resources.
 */
private data class ReportInfo(val id: String, val label: Int, val short: Int?, val group: String, val icon: ImageVector, val blurb: Int,
                              val categories: Boolean = false)

private val ReportList = listOf(
    ReportInfo("categories", R.string.reports_categories, R.string.reports_categories_short, "Spending", Icons.Outlined.PieChart, R.string.reports_categories_blurb, true),
    ReportInfo("payees", R.string.reports_payees, R.string.reports_payees_short, "Spending", Icons.Outlined.Storefront, R.string.reports_payees_blurb, true),
    ReportInfo("trends", R.string.reports_trends, R.string.reports_trends_short, "Spending", Icons.Outlined.BarChart, R.string.reports_trends_blurb, true),
    ReportInfo("calendar", R.string.reports_calendar, R.string.reports_calendar_short, "Spending", Icons.Outlined.CalendarMonth, R.string.reports_calendar_blurb, true),
    ReportInfo("incomeExpense", R.string.reports_income_expense, null, "Income", Icons.AutoMirrored.Outlined.CompareArrows, R.string.reports_income_expense_blurb, true),
    ReportInfo("income", R.string.reports_income, null, "Income", Icons.Outlined.Payments, R.string.reports_income_blurb),
    ReportInfo("cashflow", R.string.reports_cash_flow, null, "Income", Icons.Outlined.SwapHoriz, R.string.reports_cash_flow_blurb, true),
    ReportInfo("networth", R.string.net_worth, null, "Wealth", Icons.AutoMirrored.Outlined.ShowChart, R.string.reports_net_worth_blurb),
    ReportInfo("balance", R.string.reports_balance_sheet, null, "Wealth", Icons.Outlined.AccountBalance, R.string.reports_balance_sheet_blurb),
    ReportInfo("balances", R.string.reports_balances, null, "Wealth", Icons.Outlined.StackedLineChart, R.string.reports_balances_blurb),
    ReportInfo("forecast", R.string.report_forecast, null, "Wealth", Icons.Outlined.Timeline, R.string.reports_forecast_blurb),
    ReportInfo("age", R.string.report_cash_age, null, "Wealth", Icons.Outlined.HourglassBottom, R.string.reports_age_blurb),
)
private val ReportGroups = listOf("Spending" to R.string.report_spending, "Income" to R.string.report_income, "Wealth" to R.string.reports_group_wealth)

private val RangeSaver = listSaver<MonthRange, String>({ listOf(it.preset.name, it.from, it.to) },
    { MonthRange(runCatching { RangePreset.valueOf(it[0]) }.getOrDefault(RangePreset.Custom), it[1], it[2]) })
private val IdSetSaver = listSaver<Set<String>, String>({ it.toList() }, { it.toSet() })

/** The last report opened in each group ("Spending=payees|Income=income"), so going back to a group reopens it. */
private fun groupViews(saved: String): Map<String, String> =
    saved.split('|').mapNotNull { part -> part.split('=').takeIf { it.size == 2 }?.let { it[0] to it[1] } }.toMap()
private fun rememberView(saved: String, group: String, id: String) = (groupViews(saved) + (group to id)).entries.joinToString("|") { "${it.key}=${it.value}" }
private fun lastInGroup(saved: String, group: String) =
    groupViews(saved)[group]?.takeIf { id -> ReportList.any { it.id == id && it.group == group } } ?: ReportList.first { it.group == group }.id

/** A report request: the budget snapshot (by identity), the filter, and the retry attempt. */
private data class ReportKey(val budget: JSONObject, val from: String, val to: String, val accounts: List<String>, val categories: List<String>, val attempt: Int) {
    fun filter(): JSONObject = JSONObject().put("from", from).put("to", to).put("accountIds", JSONArray(accounts)).put("categoryIds", JSONArray(categories))
}
/** Models carry words, so each key holds the interface's language too: a new language rebuilds them. */
private data class OverviewKey(val budget: JSONObject, val month: String, val attempt: Int, val language: Any?)
private data class ViewKey(val data: ReportKey, val id: String, val options: ReportOptions, val scope: ReportScope, val language: Any?)
private data class ForecastKey(val budget: JSONObject, val today: LocalDate, val months: Int, val accounts: List<String>, val attempt: Int)
private data class ForecastViewKey(val data: ForecastKey, val everyday: Long?, val language: Any?)
private data class DrillRequest(val key: ReportKey, val target: DrillTarget)

/** Parses a budget for reports once per snapshot and language, from whichever worker asks first. */
private class ReportBudgetCache {
    private var last: Triple<JSONObject, Any?, ReportBudget>? = null
    @Synchronized
    fun get(budget: JSONObject, today: LocalDate): ReportBudget {
        val words = Words.source
        last?.let { if (it.first === budget && it.second === words) return it.third }
        return parseReportBudget(budget, today).also { last = Triple(budget, words, it) }
    }
}

/** A query as a suspending call; leaving the caller's scope cancels the engine's work. */
private suspend fun BudgetViewModel.await(method: String, params: JSONObject): Result<JSONObject> {
    val response = CompletableDeferred<Result<JSONObject>>()
    val request = query(method, params, onError = { response.complete(Result.failure(IllegalStateException(it))) }) { response.complete(Result.success(it)) }
    return try { response.await() } finally { request.cancel() }
}

/** Lets a row scroll sideways from edge to edge of the screen inside the page's gutters. */
private fun Modifier.bleed(horizontal: Dp) = this.layout { measurable, constraints ->
    val extra = horizontal.roundToPx() * 2
    if (!constraints.hasBoundedWidth) return@layout measurable.measure(constraints).let { p -> layout(p.width, p.height) { p.place(0, 0) } }
    val placeable = measurable.measure(constraints.copy(minWidth = constraints.maxWidth + extra, maxWidth = constraints.maxWidth + extra))
    layout(constraints.maxWidth, placeable.height) { placeable.place(-extra / 2, 0) }
}

/** A thin bar under the top bar while new figures load over the last ones. */
@Composable
private fun ProgressLine(active: Boolean) {
    Box(Modifier.fillMaxWidth().height(2.dp)) {
        if (active) LinearProgressIndicator(Modifier.fillMaxSize(), color = ur.accent, trackColor = Color.Transparent)
    }
}

/**
 * Reports on a phone open on cards for this month's spending, income against spending, net worth
 * and cash age over six months, and a list of every report, as YNAB's app does (PhoneReports.tsx).
 * A report then opens with its group, its siblings as chips, and its range and filters.
 */
@Composable
internal fun ReportsScreen(budget: JSONObject, report: JSONObject?, summary: JSONObject?, month: String, onMonth: (String) -> Unit, vm: BudgetViewModel,
                           bar: @Composable RowScope.() -> Unit, openRegister: ((RegisterTarget) -> Unit)? = null, editTransaction: (JSONObject) -> Unit) {
    var open by rememberSaveable { mutableStateOf<String?>(null) }
    var lastViews by rememberSaveable { mutableStateOf("") }
    var range by rememberSaveable(stateSaver = RangeSaver) { mutableStateOf(defaultRange(month, YearMonth.now())) }
    var accounts by rememberSaveable(stateSaver = IdSetSaver) { mutableStateOf(emptySet<String>()) }
    var categories by rememberSaveable(stateSaver = IdSetSaver) { mutableStateOf(emptySet<String>()) }
    val today = remember { LocalDate.now() }
    val cache = remember { ReportBudgetCache() }
    // Names, groups and accounts for the filters; the last snapshot's stay until the next is read.
    val context by produceState<ReportBudget?>(null, budget, languageKey()) { value = withContext(Dispatchers.Default) { cache.get(budget, today) } }
    BackHandler(open != null) { open = null }
    val choose: (String) -> Unit = { id ->
        ReportList.firstOrNull { it.id == id }?.let { info -> lastViews = rememberView(lastViews, info.group, id); open = id }
    }
    val info = ReportList.firstOrNull { it.id == open }
    if (info == null) ReportsOverview(budget, month, onMonth, vm, bar, cache, today, choose)
    else ReportPage(info, budget, vm, bar, cache, context, today, range, { range = it }, accounts, { accounts = it }, categories, { categories = it },
        choose, { group -> choose(lastInGroup(lastViews, group)) }, { open = null }, openRegister, editTransaction)
}

// ---------- Overview ----------

@Composable
private fun ReportsOverview(budget: JSONObject, month: String, onMonth: (String) -> Unit, vm: BudgetViewModel, bar: @Composable RowScope.() -> Unit,
                            cache: ReportBudgetCache, today: LocalDate, onOpen: (String) -> Unit) {
    var attempt by remember { mutableIntStateOf(0) }
    val key = OverviewKey(budget, month, attempt, languageKey())
    var result by remember { mutableStateOf<ScopedResult<OverviewModel?>?>(null) }
    var error by remember { mutableStateOf<ScopedResult<String>?>(null) }
    LaunchedEffect(key) {
        val filter = JSONObject().put("from", shiftMonth(key.month, -5)).put("to", key.month).put("accountIds", JSONArray()).put("categoryIds", JSONArray())
        vm.await("reports", filter).fold(
            { json -> result = ScopedResult(key, withContext(Dispatchers.Default) { overviewModel(parseReport(json), cache.get(key.budget, today)) }) },
            { error = ScopedResult(key, it.message ?: Words[R.string.reports_failed]) })
    }
    val failed = error?.forScope(key)
    val current = result?.scope == key
    val model = result?.value
    Column(Modifier.fillMaxSize()) {
        PhoneTopBar(stringResource(R.string.tab_reports), actions = bar)
        ProgressLine(failed == null && !current)
        CompositionLocalProvider(LocalStale provides !current) {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, bottom = DockRoom)) {
                item(key = "month", contentType = "month") { Row(Modifier.fillMaxWidth()) { MonthSelector(month, onMonth) } }
                if (failed != null) item(key = "error", contentType = "error") {
                    Column(Modifier.padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        FormError(failed)
                        PrimaryButton(stringResource(R.string.reports_retry_all), { attempt++ })
                    }
                } else if (result == null) item(key = "loading", contentType = "loading") {
                    Box(Modifier.fillMaxWidth().padding(vertical = 48.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = ur.accent) }
                } else if (model != null) {
                    item(key = "spending", contentType = "card") { SpendingCard(model) { onOpen("categories") } }
                    item(key = "income", contentType = "card") {
                        OverviewCard(Icons.AutoMirrored.Outlined.CompareArrows, stringResource(R.string.reports_income_vs_spending), { onOpen("incomeExpense") }) {
                            Lead(model.incomeLead)
                            MiniBars(model.incomeBars)
                        }
                    }
                    item(key = "networth", contentType = "card") {
                        OverviewCard(Icons.AutoMirrored.Outlined.ShowChart, stringResource(R.string.net_worth), { onOpen("networth") }) {
                            BigFigure(model.netWorthText, if (model.netWorthNegative) ur.negative else ur.text)
                            SplitRow(stringResource(R.string.report_assets), model.assetsText, ur.text)
                            SplitRow(stringResource(R.string.report_debts), model.debtsText, if (model.debtsNegative) ur.negative else ur.text)
                            MiniBars(model.netWorthBars)
                        }
                    }
                    item(key = "age", contentType = "card") {
                        OverviewCard(Icons.Outlined.HourglassBottom, stringResource(R.string.report_cash_age), { onOpen("age") }) {
                            BigFigure(model.ageText, ur.text)
                            MiniBars(model.ageBars)
                        }
                    }
                }
                item(key = "all", contentType = "all") { AllReports(onOpen) }
            }
        }
    }
}

/** A report card: its name opens the full report, then its figures. */
@Composable
private fun OverviewCard(icon: ImageVector, title: String, onOpen: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    CardPart(top = true, bottom = true) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable(onClickLabel = stringResource(R.string.common_open_named, title), onClick = onOpen).heightIn(min = 40.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Icon(icon, null, Modifier.size(18.dp), tint = ur.accent)
                Spacer(Modifier.width(8.dp))
                Text(title, Modifier.weight(1f), color = ur.accent, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = ur.muted)
            }
            content()
        }
    }
}

@Composable
private fun BigFigure(text: String, color: Color) {
    Text(text, color = color, fontSize = 32.sp, lineHeight = 38.sp, fontWeight = FontWeight.ExtraBold, style = Tabular, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

@Composable
private fun Lead(text: String) {
    Text(text, Modifier.padding(bottom = 4.dp), color = ur.text, fontSize = 18.sp, lineHeight = 23.sp, fontWeight = FontWeight.SemiBold)
}

@Composable
private fun SplitRow(label: String, value: String, color: Color) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, Modifier.weight(1f), color = ur.text2, fontSize = 16.sp, fontWeight = FontWeight.Medium)
        Text(value, color = color, fontSize = 16.sp, style = Tabular, maxLines = 1)
    }
}

@Composable
private fun SpendList(rows: List<SpendRow>, dots: Boolean, head: Boolean) {
    val palette = reportPalette()
    Column {
        if (head) Row(Modifier.fillMaxWidth().heightIn(min = 26.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.reports_top_categories), Modifier.weight(1f), color = ur.text2, fontSize = 14.sp)
            Text(stringResource(R.string.report_spent), color = ur.text2, fontSize = 14.sp)
        }
        rows.forEach { row ->
            Row(Modifier.fillMaxWidth().heightIn(min = 34.dp), verticalAlignment = Alignment.CenterVertically) {
                if (dots) {
                    Box(Modifier.size(6.dp, 18.dp).clip(RoundedCornerShape(3.dp)).background(palette.color(row.color)))
                    Spacer(Modifier.width(10.dp))
                }
                Text(row.label, Modifier.weight(1f), color = ur.text, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(row.valueText, Modifier.padding(start = 12.dp), color = ur.text, fontSize = 16.sp, style = Tabular, maxLines = 1)
            }
        }
    }
}

/** This month's spending: the total, a bar split by category, the top categories and any refunds. */
@Composable
private fun SpendingCard(model: OverviewModel, onOpen: () -> Unit) {
    OverviewCard(Icons.Outlined.PieChart, stringResource(R.string.reports_spending_breakdown), onOpen) {
        Text(model.monthName, color = ur.text2, fontSize = 15.sp)
        BigFigure(model.spendingText, ur.text)
        if (model.rows.isNotEmpty()) {
            SegmentBar(model.rows)
            SpendList(model.rows, dots = true, head = true)
        } else if (model.nothingSpent) Lead(stringResource(R.string.reports_nothing_spent))
        model.refundLead?.let {
            Lead(it)
            SpendList(model.refunds, dots = false, head = false)
        }
    }
}

/** Every report, by group. */
@Composable
private fun AllReports(onOpen: (String) -> Unit) {
    CardPart(top = true, bottom = true, live = true) {
        Text(stringResource(R.string.reports_all), Modifier.padding(start = 16.dp, top = 14.dp, bottom = 2.dp), color = ur.text, fontSize = 17.sp, fontWeight = FontWeight.Bold)
        ReportGroups.forEach { (group, name) ->
            Text(stringResource(name), Modifier.padding(start = 16.dp, top = 10.dp, bottom = 2.dp), color = ur.muted, style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.Bold)
            val reports = ReportList.filter { it.group == group }
            reports.forEachIndexed { i, report ->
                ListRow(stringResource(report.label), icon = report.icon, onClick = { onOpen(report.id) }, divider = i < reports.lastIndex)
            }
        }
        Spacer(Modifier.height(6.dp))
    }
}

// ---------- A report ----------

@Composable
private fun ChipRow(content: @Composable RowScope.() -> Unit) {
    Row(Modifier.fillMaxWidth().bleed(Gutter).horizontalScroll(rememberScrollState()).padding(horizontal = Gutter, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically, content = content)
}

@Composable
private fun ReportPage(
    info: ReportInfo, budget: JSONObject, vm: BudgetViewModel, bar: @Composable RowScope.() -> Unit, cache: ReportBudgetCache, context: ReportBudget?,
    today: LocalDate, range: MonthRange, setRange: (MonthRange) -> Unit, accounts: Set<String>, setAccounts: (Set<String>) -> Unit,
    categories: Set<String>, setCategories: (Set<String>) -> Unit, choose: (String) -> Unit, chooseGroup: (String) -> Unit, back: () -> Unit,
    openRegister: ((RegisterTarget) -> Unit)?, editTransaction: (JSONObject) -> Unit,
) {
    val forecast = info.id == "forecast"
    val thisMonth = YearMonth.from(today).toString()
    var attempt by remember { mutableIntStateOf(0) }
    // Reports that don't split by category ignore the Categories filter.
    val key = ReportKey(budget, range.from.ifEmpty { context?.earliestMonth ?: thisMonth }, range.to.ifEmpty { thisMonth }, accounts.sorted(),
        if (info.categories) categories.sorted() else emptyList(), attempt)
    val latestKey by rememberUpdatedState(key)
    var data by remember { mutableStateOf<ScopedResult<ReportData>?>(null) }
    var error by remember { mutableStateOf<ScopedResult<String>?>(null) }
    LaunchedEffect(key, forecast) {
        if (forecast) return@LaunchedEffect
        // A report belongs to its exact snapshot and filters; new labels must never describe old figures.
        vm.await("reports", key.filter()).fold(
            { json -> data = ScopedResult(key, withContext(Dispatchers.Default) { parseReport(json) }) },
            { error = ScopedResult(key, it.message ?: Words[R.string.reports_failed_one]) })
    }
    val current = data?.takeIf { it.scope == key }

    // A report's own choices start afresh when another report opens, as the desktop's do.
    var group by rememberSaveable(info.id) { mutableStateOf<String?>(null) }
    // Back leaves a group drilled into before it leaves the report.
    BackHandler(info.id == "categories" && group != null) { group = null }
    var byGroup by rememberSaveable(info.id) { mutableStateOf(true) }
    var chartAccounts by remember(info.id) { mutableStateOf<Map<String, Int>?>(null) }
    var expanded by rememberSaveable(info.id, stateSaver = IdSetSaver) { mutableStateOf(emptySet<String>()) }
    var payeeQuery by rememberSaveable(info.id) { mutableStateOf("") }
    // The week starts where the phone's region starts it, whatever language the app is in.
    val weekStart = remember { localeWeekStart(android.content.res.Resources.getSystem().configuration.locales[0]) }
    val scope = ReportScope(key.from, key.to, accounts, info.categories && categories.isNotEmpty(), today, weekStart)
    val options = ReportOptions(group, byGroup, chartAccounts)
    val language = languageKey()
    val viewKey = ViewKey(key, info.id, options, scope, language)
    var view by remember { mutableStateOf<ScopedResult<ReportView?>?>(null) }
    LaunchedEffect(viewKey, current != null) {
        val figures = current ?: return@LaunchedEffect
        view = ScopedResult(viewKey, withContext(Dispatchers.Default) { reportView(info.id, figures.value, cache.get(key.budget, today), scope, options) })
    }
    // The last report of this kind stays on screen, dimmed, while the next range or snapshot loads.
    val shown = view?.takeIf { it.value?.id == info.id }
    val shownKey = shown?.scope as? ViewKey
    val stale = shownKey != null && shownKey.data != key
    val failed = error?.forScope(key)
    val ready = shownKey?.data == key

    // Forecast
    var ahead by rememberSaveable(info.id) { mutableIntStateOf(6) }
    var everyday by rememberSaveable(info.id) { mutableStateOf<Long?>(null) }
    val forecastKey = ForecastKey(budget, today, ahead, accounts.sorted(), attempt)
    var forecastData by remember { mutableStateOf<ScopedResult<ForecastData>?>(null) }
    var forecastError by remember { mutableStateOf<ScopedResult<String>?>(null) }
    LaunchedEffect(forecastKey, forecast) {
        if (!forecast) return@LaunchedEffect
        val params = JSONObject().put("today", today.toString()).put("through", today.plusMonths(forecastKey.months.toLong()).toString())
            .put("accountIds", JSONArray(forecastKey.accounts))
        vm.await("forecast", params).fold(
            { json -> forecastData = ScopedResult(forecastKey, withContext(Dispatchers.Default) { parseForecast(json) }) },
            { forecastError = ScopedResult(forecastKey, it.message ?: Words[R.string.reports_forecast_failed]) })
    }
    val forecastCurrent = forecastData?.takeIf { it.scope == forecastKey }
    val forecastViewKey = ForecastViewKey(forecastKey, everyday, language)
    var forecastShown by remember { mutableStateOf<ScopedResult<ForecastView>?>(null) }
    LaunchedEffect(forecastViewKey, forecastCurrent != null) {
        val figures = forecastCurrent ?: return@LaunchedEffect
        forecastShown = ScopedResult(forecastViewKey, withContext(Dispatchers.Default) {
            forecastView(figures.value, cache.get(forecastKey.budget, today), forecastKey.accounts.toSet(), forecastKey.months, forecastViewKey.everyday, today)
        })
    }
    val forecastShownKey = forecastShown?.scope as? ForecastViewKey
    val forecastStale = forecastShownKey != null && forecastShownKey.data != forecastKey
    val forecastFailed = forecastError?.forScope(forecastKey)
    val forecastLoading = forecast && forecastFailed == null && forecastShownKey?.data != forecastKey

    // The transactions behind a figure, for this exact report.
    var drill by remember { mutableStateOf<DrillRequest?>(null) }
    val listState = rememberLazyListState()
    val coroutines = rememberCoroutineScope()
    val payeeView = shown?.value as? PayeeView
    val filteredPayees by produceState<List<PayeeRow>?>(null, payeeView, payeeQuery) {
        val rows = payeeView?.rows.orEmpty()
        val q = payeeQuery.trim().lowercase()
        value = if (q.isEmpty()) rows else withContext(Dispatchers.Default) { rows.filter { it.name.lowercase().contains(q) } }
    }
    val currency = budget.text("currency")
    val uncategorized = if (info.categories) current?.value?.categories?.get(UNCATEGORIZED_BUCKET) ?: 0L else 0L
    val notice = remember(uncategorized, currency, language) {
        if (uncategorized != 0L) Words[R.string.reports_uncategorized_notice, money(uncategorized, currency)] else null
    }

    // Filters
    var rangeOpen by remember { mutableStateOf(false) }
    var accountsOpen by remember { mutableStateOf(false) }
    var categoriesOpen by remember { mutableStateOf(false) }
    var everydayOpen by remember { mutableStateOf(false) }
    val accountGroups = remember(context, forecast) { context?.let { accountFilterGroups(it, forecast) }.orEmpty() }
    val categoryGroups = remember(context) { context?.let { categoryFilterGroups(it) }.orEmpty() }
    val rangeLabel = describeRange(range)
    val accountLabel = filterSummary(accountGroups, accounts, stringResource(if (forecast) R.string.reports_all_budget_accounts else R.string.reports_all_accounts)) {
        Words.count(R.plurals.filter_accounts, it.toLong())
    }
    val categoryLabel = filterSummary(categoryGroups, categories, stringResource(R.string.category_all)) { Words.count(R.plurals.filter_categories, it.toLong()) }

    // Export: the engine writes the CSV for these exact filters, then the system asks where to save it.
    var csv by remember { mutableStateOf("") }
    var exportError by remember { mutableStateOf<String?>(null) }
    val androidContext = LocalContext.current
    val exporter = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/csv")) { uri ->
        val content = csv
        if (uri != null) coroutines.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    androidContext.contentResolver.openOutputStream(uri)?.bufferedWriter()?.use { it.write(content) } ?: error(androidContext.getString(R.string.export_destination_failed))
                }
            }.onFailure { exportError = it.message }
        }
    }
    val exportCsv: () -> Unit = {
        val exportKey = key
        exportError = null
        coroutines.launch {
            vm.await("reportCsv", exportKey.filter()).fold(
                { json -> if (exportKey == latestKey) { csv = json.text("items"); exporter.launch("urbudget-report-${exportKey.from}-${exportKey.to}.csv") } },
                { if (exportKey == latestKey) exportError = it.message })
        }
    }

    val actions = remember(key, info.id) {
        ReportActions(
            drill = { target -> drill = DrillRequest(key, target) },
            setGroup = { group = it },
            setByGroup = { byGroup = it },
            toggleChart = { id ->
                val figures = data?.takeIf { it.scope == key }?.value
                val currentView = view?.value as? BalancesView
                if (figures != null && currentView != null) {
                    chartAccounts = toggleChartAccount(currentView.chosen, balanceAccounts(figures, cache.get(key.budget, today), accounts), id)
                }
            },
            toggleExpand = { g -> expanded = if (g in expanded) expanded - g else expanded + g },
            showAllPayees = {},
        )
    }
    val chromeItems = 4 + (if (notice != null) 1 else 0) + (if (failed != null && !forecast) 1 else 0) + (if (exportError != null) 1 else 0)
    val jumpToPayees: () -> Unit = { payeeView?.let { v -> coroutines.launch { listState.animateScrollToItem(chromeItems + payeeListOffset(v)) } } }
    val bodyActions = remember(actions, payeeView, chromeItems) {
        ReportActions(actions.drill, actions.setGroup, actions.setByGroup, actions.toggleChart, actions.toggleExpand, jumpToPayees)
    }

    Column(Modifier.fillMaxSize()) {
        PhoneTopBar(stringResource(info.label), onBack = back, actions = bar)
        ProgressLine(if (forecast) forecastLoading else failed == null && !ready)
        CompositionLocalProvider(LocalStale provides (if (forecast) forecastStale else stale)) {
            LazyColumn(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(start = Gutter, end = Gutter, bottom = DockRoom)) {
                item(key = "blurb", contentType = "chrome") {
                    Text(stringResource(info.blurb), Modifier.padding(top = 2.dp, bottom = 10.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
                }
                item(key = "groups", contentType = "chrome") {
                    Segmented(ReportGroups.map { (group, name) -> group to stringResource(name) }, info.group, chooseGroup)
                }
                item(key = "reports", contentType = "chrome") {
                    ChipRow {
                        ReportList.filter { it.group == info.group }.forEach { r -> FilterPill(stringResource(r.short ?: r.label), r.id == info.id, { choose(r.id) }, r.icon) }
                    }
                }
                item(key = "filters", contentType = "chrome") {
                    ChipRow {
                        if (!forecast) FilterPill(rangeLabel, false, { rangeOpen = true }, Icons.Outlined.CalendarMonth)
                        FilterPill(accountLabel, accounts.isNotEmpty(), { accountsOpen = true }, Icons.Outlined.AccountBalanceWallet)
                        if (info.categories) FilterPill(categoryLabel, categories.isNotEmpty(), { categoriesOpen = true }, Icons.Outlined.Category)
                        if (!forecast) FilterPill(stringResource(R.string.reports_export_csv), false, exportCsv, Icons.Outlined.FileDownload)
                    }
                }
                notice?.let { text ->
                    item(key = "notice", contentType = "notice") {
                        val needsCategory = stringResource(R.string.category_needed)
                        Notice(text, Modifier.padding(top = 10.dp), action = stringResource(R.string.common_review),
                            onAction = { drill = DrillRequest(key, DrillTarget(needsCategory, "category", UNCATEGORIZED_BUCKET)) })
                    }
                }
                if (failed != null && !forecast) item(key = "error", contentType = "error") {
                    Column(Modifier.padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        FormError(failed)
                        SecondaryButton(stringResource(R.string.reports_retry), { attempt++ })
                    }
                }
                exportError?.let { message -> item(key = "exportError", contentType = "error") { Box(Modifier.padding(top = 10.dp)) { FormError(message) } } }
                if (forecast) forecastBody(forecastShown?.value, ahead, { ahead = it }, forecastFailed, { attempt++ }, { everydayOpen = true }, { everyday = null })
                else if (failed == null) {
                    val body = shown?.value
                    if (body != null) reportBody(body, bodyActions, expanded, payeeQuery, { payeeQuery = it }, filteredPayees ?: payeeView?.rows.orEmpty())
                    else if (view?.scope == viewKey) item(key = "empty", contentType = "empty") { EmptyState(stringResource(R.string.reports_empty_title), stringResource(R.string.reports_empty_detail)) }
                    else item(key = "loading", contentType = "loading") {
                        Box(Modifier.fillMaxWidth().padding(vertical = 48.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = ur.accent) }
                    }
                }
            }
        }
    }

    if (rangeOpen) RangeSheet(range, context?.earliestMonth ?: thisMonth, { setRange(it); rangeOpen = false }) { rangeOpen = false }
    if (accountsOpen) MultiFilterSheet(stringResource(R.string.tab_accounts), accountGroups, accounts, setAccounts) { accountsOpen = false }
    if (categoriesOpen) MultiFilterSheet(stringResource(R.string.reports_categories_filter), categoryGroups, categories, setCategories) { categoriesOpen = false }
    if (everydayOpen) forecastShown?.value?.let { v ->
        EverydaySheet(v.everyday, currency, { everyday = it }) { everydayOpen = false }
    }
    drill?.takeIf { it.key == key }?.let { request ->
        DrillSheet(request, vm, cache, today, close = { drill = null }, register = openRegister?.let { go -> registerTarget(request)?.let { target -> { drill = null; go(target) } } }) { record ->
            drill = null; editTransaction(record)
        }
    }
}

/** The everyday spending a forecast assumes, typed in. */
@Composable
private fun EverydaySheet(current: Long, currency: String, onSave: (Long) -> Unit, onDismiss: () -> Unit) {
    var text by remember { mutableStateOf(amountText(current)) }
    var error by remember { mutableStateOf<String?>(null) }
    FormDialog(stringResource(R.string.reports_everyday), onDismiss, confirm = stringResource(R.string.common_save), onConfirm = {
        runCatching { amountValue(text) }.onSuccess { onSave(abs(it)); onDismiss() }.onFailure { error = it.message }
    }) {
        HelpText(stringResource(R.string.reports_everyday_help))
        Field(text, { text = it }, stringResource(R.string.reports_everyday_field, currency), numeric = true)
        FormError(error)
    }
}

/**
 * Where the register shows a figure's transactions: its category or payee over the report's months,
 * or the day, in the one account the report is filtered to. Income sources have no register filter.
 */
private fun registerTarget(request: DrillRequest): RegisterTarget? {
    val target = request.target
    val from = target.from ?: firstDay(request.key.from)
    val through = target.to ?: lastDay(request.key.to)
    val account = request.key.accounts.singleOrNull().orEmpty()
    return when (target.dimension) {
        "category" -> RegisterTarget(if (target.key == UNCATEGORIZED_BUCKET) "__none" else target.key, account, from, through)
        "payee" -> target.key.takeIf { it.isNotBlank() }?.let { RegisterTarget(account = account, from = from, through = through, search = it) }
        "day" -> RegisterTarget(account = account, from = from, through = through)
        else -> null
    }
}

/** The transactions behind a report figure, newest first; one opens in the editor, or all of them in the register. */
@Composable
private fun DrillSheet(request: DrillRequest, vm: BudgetViewModel, cache: ReportBudgetCache, today: LocalDate, close: () -> Unit, register: (() -> Unit)? = null,
                       open: (JSONObject) -> Unit) {
    var model by remember(request) { mutableStateOf<DrillModel?>(null) }
    var error by remember(request) { mutableStateOf<String?>(null) }
    LaunchedEffect(request) {
        val key = request.key
        val target = request.target
        val accounts = key.accounts.toSet()
        if (target.dimension == "income") {
            // The engine's details cover spending; income lines come from the same snapshot here.
            model = withContext(Dispatchers.Default) { incomeDrill(key.budget, cache.get(key.budget, today), target.key, key.from, key.to, accounts) }
        } else vm.await("reportDetails", key.filter().put("dimension", target.dimension).put("key", target.key)).fold(
            { json ->
                model = withContext(Dispatchers.Default) {
                    drillFromDetails(json.rows("items"), key.budget, cache.get(key.budget, today), target.from ?: firstDay(key.from), target.to ?: lastDay(key.to), accounts)
                }
            },
            { error = it.message ?: Words[R.string.reports_drill_failed] })
    }
    val height = screenHeight()
    val income = request.target.dimension == "income"
    UrSheet(close) {
        SheetHeader(request.target.title, close, null, null)
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = height * .8f), contentPadding = PaddingValues(start = Gutter, end = Gutter, bottom = 28.dp)) {
            item(key = "range", contentType = "range") { Text(model?.range.orEmpty(), color = ur.muted, style = MaterialTheme.typography.bodyMedium) }
            item(key = "figures", contentType = "figures") {
                Row(Modifier.fillMaxWidth().padding(vertical = 12.dp)) {
                    Figure(model?.figureLabel ?: stringResource(if (income) R.string.report_received else R.string.report_spent), model?.figure ?: "—", Modifier.weight(1f))
                    Figure(stringResource(R.string.tab_transactions), model?.count ?: "—", Modifier.weight(1f))
                    Figure(stringResource(R.string.report_average), model?.average ?: "—", Modifier.weight(1f), alignEnd = true)
                }
                if (register != null) SecondaryButton(stringResource(R.string.reports_open_in_register), register, Modifier.fillMaxWidth().padding(bottom = 12.dp), icon = Icons.AutoMirrored.Outlined.ReceiptLong)
                Hairline()
            }
            if (model == null && error == null) item(key = "loading", contentType = "loading") { LinearProgressIndicator(Modifier.fillMaxWidth().padding(vertical = 16.dp), color = ur.accent) }
            error?.let { message -> item(key = "error", contentType = "error") { Box(Modifier.padding(top = 12.dp)) { FormError(message) } } }
            model?.let { m ->
                if (m.lines.isEmpty()) item(key = "empty", contentType = "empty") { Text(stringResource(R.string.reports_no_transactions), Modifier.padding(vertical = 16.dp), color = ur.muted) }
                items(m.lines, key = { it.key }, contentType = { "line" }) { line -> DrillLineRow(line) { m.records[line.transactionId]?.let(open) } }
            }
        }
    }
}
