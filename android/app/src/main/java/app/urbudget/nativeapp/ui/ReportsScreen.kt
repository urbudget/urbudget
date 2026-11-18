package app.urbudget.nativeapp.ui

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.CompareArrows
import androidx.compose.material.icons.automirrored.outlined.ReceiptLong
import androidx.compose.material.icons.outlined.AccountBalance
import androidx.compose.material.icons.outlined.AccountBalanceWallet
import androidx.compose.material.icons.outlined.Dashboard
import androidx.compose.material.icons.outlined.CalendarMonth
import androidx.compose.material.icons.outlined.Category
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.outlined.FileDownload
import androidx.compose.material.icons.outlined.Insights
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.outlined.PieChart
import androidx.compose.material.icons.outlined.Timeline
import androidx.compose.material.icons.outlined.Tune
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import app.urbudget.nativeapp.BalancesView
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.MiniBarsModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.DrillModel
import app.urbudget.nativeapp.DrillTarget
import app.urbudget.nativeapp.ForecastData
import app.urbudget.nativeapp.ForecastView
import app.urbudget.nativeapp.INVESTMENTS_REPORT
import app.urbudget.nativeapp.LegendItem
import app.urbudget.nativeapp.MonthRange
import app.urbudget.nativeapp.NEEDS_CATEGORY
import app.urbudget.nativeapp.OverviewCard
import app.urbudget.nativeapp.OverviewModel
import app.urbudget.nativeapp.PayeeRow
import app.urbudget.nativeapp.PayeeView
import app.urbudget.nativeapp.Portfolio
import app.urbudget.nativeapp.RangePreset
import app.urbudget.nativeapp.ReportBudget
import app.urbudget.nativeapp.ReportData
import app.urbudget.nativeapp.ReportOptions
import app.urbudget.nativeapp.ReportScope
import app.urbudget.nativeapp.ReportView
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.UNCATEGORIZED_BUCKET
import app.urbudget.nativeapp.accountFilterGroups
import app.urbudget.nativeapp.balanceAccounts
import app.urbudget.nativeapp.categoriesApply
import app.urbudget.nativeapp.categoryFilterGroups
import app.urbudget.nativeapp.defaultRange
import app.urbudget.nativeapp.describeRange
import app.urbudget.nativeapp.drillFromDetails
import app.urbudget.nativeapp.filterLine
import app.urbudget.nativeapp.filterSummary
import app.urbudget.nativeapp.firstDay
import app.urbudget.nativeapp.forecastView
import app.urbudget.nativeapp.holdingAccountIds
import app.urbudget.nativeapp.incomeDrill
import app.urbudget.nativeapp.investedNote
import app.urbudget.nativeapp.investmentsView
import app.urbudget.nativeapp.lastDay
import app.urbudget.nativeapp.localeWeekStart
import app.urbudget.nativeapp.overviewModel
import app.urbudget.nativeapp.parseForecast
import app.urbudget.nativeapp.parsePortfolio
import app.urbudget.nativeapp.parseReport
import app.urbudget.nativeapp.parseReportBudget
import app.urbudget.nativeapp.rangeApplies
import app.urbudget.nativeapp.rememberView
import app.urbudget.nativeapp.reportOfView
import app.urbudget.nativeapp.reportPlace
import app.urbudget.nativeapp.reportView
import app.urbudget.nativeapp.shiftMonth
import app.urbudget.nativeapp.toggleChartAccount
import app.urbudget.nativeapp.viewToOpen
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth
import kotlin.math.abs

/** One of the six reports as the screen names it: its name, the question it answers, and its icon. */
private data class ReportInfo(val id: String, val name: Int, val question: Int, val icon: ImageVector)

private val ReportList = listOf(
    ReportInfo("spending", R.string.report_spending, R.string.reports_ask_spending, Icons.Outlined.PieChart),
    ReportInfo("income", R.string.report_income, R.string.reports_ask_income, Icons.AutoMirrored.Outlined.CompareArrows),
    ReportInfo("networth", R.string.net_worth, R.string.reports_ask_net_worth, RisingChart),
    ReportInfo("balance", R.string.reports_balance_sheet, R.string.reports_ask_balance, Icons.Outlined.AccountBalance),
    ReportInfo(INVESTMENTS_REPORT, R.string.section_investments, R.string.reports_ask_investments, Icons.Outlined.Insights),
    ReportInfo("forecast", R.string.report_forecast, R.string.reports_ask_forecast, Icons.Outlined.Timeline),
)

/** What each view is called on its report's switch: short, since the report's name says the rest. */
private val ViewLabels = mapOf(
    "categories" to R.string.category_label, "payees" to R.string.payee_label, "trends" to R.string.reports_trends_short, "calendar" to R.string.reports_calendar_short,
    "incomeExpense" to R.string.reports_view_vs_spending, "income" to R.string.reports_sources, "cashflow" to R.string.reports_view_in_out,
    "networth" to R.string.reports_trends_short, "balances" to R.string.reports_by_account,
    "performance" to R.string.reports_performance, "allocation" to R.string.reports_allocation, "investIncome" to R.string.ibkr_income_costs,
    "forecast" to R.string.reports_view_balances, "age" to R.string.report_cash_age,
)

/** The view of a report that is open, by the id its body has always had. */
private data class ViewInfo(val id: String) {
    /** Whether the Categories filter applies to it. */
    val categories: Boolean get() = categoriesApply(id)
}

private val RangeSaver = listSaver<MonthRange, String>({ listOf(it.preset.name, it.from, it.to) },
    { MonthRange(runCatching { RangePreset.valueOf(it[0]) }.getOrDefault(RangePreset.Custom), it[1], it[2]) })
private val IdSetSaver = listSaver<Set<String>, String>({ it.toList() }, { it.toSet() })

/** A report request: the budget snapshot (by identity), the filter, and the retry attempt. */
private data class ReportKey(val budget: JSONObject, val from: String, val to: String, val accounts: List<String>, val categories: List<String>, val attempt: Int) {
    fun filter(): JSONObject = JSONObject().put("from", from).put("to", to).put("accountIds", JSONArray(accounts)).put("categoryIds", JSONArray(categories))
}
/** Models carry words, so each key holds the interface's language too: a new language rebuilds them. */
private data class OverviewKey(val budget: JSONObject, val month: String, val attempt: Int, val language: Any?, val holdings: Boolean, val today: LocalDate)
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

/** A thin bar under the top bar while new figures load over the last ones. */
@Composable
private fun ProgressLine(active: Boolean) {
    Box(Modifier.fillMaxWidth().height(2.dp)) {
        if (active) LinearProgressIndicator(Modifier.fillMaxSize(), color = ur.accent, trackColor = Color.Transparent)
    }
}

/**
 * Reports open on an overview of six cards, one for each report: its question, one figure and a
 * small chart, over the last six months. A card opens its report. A report answers its question
 * in a few views, chosen by one switch under its title, with one line saying which months and
 * accounts it covers; each report comes back on the view it was last on. The Investments report
 * and its card are there once an account has holdings.
 */
@Composable
internal fun ReportsScreen(budget: JSONObject, report: JSONObject?, summary: JSONObject?, month: String, onMonth: (String) -> Unit, vm: BudgetViewModel,
                           bar: @Composable RowScope.() -> Unit, openRegister: ((RegisterTarget) -> Unit)? = null, openHoldings: ((String) -> Unit)? = null,
                           setUpHoldings: ((String) -> Unit)? = null, editTransaction: (JSONObject) -> Unit) {
    // The open view, by its id: one of the reports there were, so a place saved before still lands on its view.
    var open by rememberSaveable { mutableStateOf<String?>(null) }
    var lastViews by rememberSaveable { mutableStateOf("") }
    // The months and accounts chosen carry from one report to the next.
    var range by rememberSaveable(stateSaver = RangeSaver) { mutableStateOf(defaultRange(month, YearMonth.now())) }
    var accounts by rememberSaveable(stateSaver = IdSetSaver) { mutableStateOf(emptySet<String>()) }
    var categories by rememberSaveable(stateSaver = IdSetSaver) { mutableStateOf(emptySet<String>()) }
    val today = remember { LocalDate.now() }
    val cache = remember { ReportBudgetCache() }
    // Names, groups and accounts for the filters; the last snapshot's stay until the next is read.
    val context by produceState<ReportBudget?>(null, budget, languageKey()) { value = withContext(Dispatchers.Default) { cache.get(budget, today) } }
    val chooseView: (String) -> Unit = { view -> lastViews = rememberView(lastViews, view); open = view }
    val openReport: (String) -> Unit = { id -> viewToOpen(id, lastViews)?.let(chooseView) }
    // The Investments report is there only while an account has holdings; left open, it goes with them.
    val holdings = remember(budget) { holdingAccountIds(budget).isNotEmpty() }
    val place = open?.let(::reportPlace)?.takeIf { (id, _) -> id != INVESTMENTS_REPORT || holdings }
    val info = place?.let { (id, _) -> ReportList.firstOrNull { it.id == id } }
    @Composable
    fun page(shown: ReportInfo, view: String, bar: @Composable RowScope.() -> Unit, back: (() -> Unit)?) =
        ReportPage(ViewInfo(view), shown, budget, vm, bar, cache, context, today, range, { range = it }, accounts, { accounts = it }, categories, { categories = it },
            chooseView, back, openRegister, openHoldings, setUpHoldings, holdings, editTransaction)
    ListDetail(
        single = {
            BackHandler(open != null) { open = null }
            if (info == null || place == null) ReportsOverview(budget, month, onMonth, vm, bar, cache, today, openReport, holdings)
            else page(info, place.second, bar, back = { open = null })
        },
        // Side by side the list is the overview and the six reports by name, and the wide side shows
        // the overview's cards or the report chosen, with the page's buttons. Back from a report
        // returns to the overview.
        list = { ReportsIndex(info?.id, openReport, holdings) { open = null } },
        detail = {
            BackHandler(open != null) { open = null }
            if (info == null || place == null) ReportsOverview(budget, month, onMonth, vm, bar, cache, today, openReport, holdings, title = stringResource(R.string.reports_overview))
            else page(info, place.second, bar, back = null)
        },
    )
}

/** Reports' list beside the open report on a large screen: the overview, then the six reports, the one open marked. */
@Composable
private fun ReportsIndex(open: String?, onOpen: (String) -> Unit, holdings: Boolean, overview: () -> Unit) {
    GlassTop(Modifier.fillMaxSize(), top = { PhoneTopBar(stringResource(R.string.tab_reports)) }) {
        PageList(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = 10.dp, end = 10.dp, top = 4.dp, bottom = DockRoom)) {
            item(key = "reports", contentType = "reports") {
                CardPart(top = true, bottom = true, live = true) {
                    @Composable
                    fun row(name: String, icon: ImageVector, chosen: Boolean, divider: Boolean, onClick: () -> Unit) =
                        ListRow(name, Modifier.chosenWash(chosen, ur.accentSoft, Color.Transparent).semantics { selected = chosen },
                            icon = icon, titleColor = if (chosen) ur.accent else ur.text, onClick = onClick, chevron = false, divider = divider)
                    row(stringResource(R.string.reports_overview), Icons.Outlined.Dashboard, open == null, true, overview)
                    val reports = ReportList.filter { it.id != INVESTMENTS_REPORT || holdings }
                    reports.forEachIndexed { i, report -> row(stringResource(report.name), report.icon, report.id == open, i < reports.lastIndex) { onOpen(report.id) } }
                }
            }
        }
    }
}

// ---------- Overview ----------

/** From this much room the overview's cards stand two across. */
private val TwoCards = 640.dp

@Composable
private fun ReportsOverview(budget: JSONObject, month: String, onMonth: (String) -> Unit, vm: BudgetViewModel, bar: @Composable RowScope.() -> Unit,
                            cache: ReportBudgetCache, today: LocalDate, onOpen: (String) -> Unit, holdings: Boolean,
                            title: String = stringResource(R.string.tab_reports)) {
    var attempt by remember { mutableIntStateOf(0) }
    val key = OverviewKey(budget, month, attempt, languageKey(), holdings, today)
    var result by remember { mutableStateOf<ScopedResult<OverviewModel?>?>(null) }
    var error by remember { mutableStateOf<ScopedResult<String>?>(null) }
    LaunchedEffect(key) {
        val filter = JSONObject().put("from", shiftMonth(key.month, -5)).put("to", key.month).put("accountIds", JSONArray()).put("categoryIds", JSONArray())
        coroutineScope {
            // The investments and the forecast have figures of their own; without them the other cards still show.
            val held = async { if (key.holdings) vm.await("investmentsReport", filter).getOrNull() else null }
            val ahead = async {
                vm.await("forecast", JSONObject().put("today", key.today.toString()).put("through", key.today.plusMonths(6).toString()).put("accountIds", JSONArray())).getOrNull()
            }
            vm.await("reports", filter).fold(
                { json ->
                    val portfolio = held.await()
                    val forecast = ahead.await()
                    result = ScopedResult(key, withContext(Dispatchers.Default) {
                        overviewModel(parseReport(json), cache.get(key.budget, today), portfolio?.let(::parsePortfolio), forecast?.let(::parseForecast), key.today)
                    })
                },
                { held.cancel(); ahead.cancel(); error = ScopedResult(key, it.message ?: Words[R.string.reports_failed]) })
        }
    }
    val failed = error?.forScope(key)
    val current = result?.scope == key
    val model = result?.value
    GlassTop(Modifier.fillMaxSize(), top = {
        PhoneTopBar(title, actions = bar)
        ProgressLine(failed == null && !current)
    }) {
        CompositionLocalProvider(LocalStale provides !current) {
            BoxWithConstraints(Modifier.fillMaxSize()) {
                // One card to a row on a phone, two where the pane has the room.
                val across = if (maxWidth >= TwoCards) 2 else 1
                PageList(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, bottom = DockRoom)) {
                    item(key = "month", contentType = "month") { Row(Modifier.fillMaxWidth()) { MonthSelector(month, onMonth) } }
                    if (failed != null) item(key = "error", contentType = "error") {
                        Column(Modifier.padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            FormError(failed)
                            PrimaryButton(stringResource(R.string.reports_retry_all), { attempt++ })
                        }
                    } else if (result == null) item(key = "loading", contentType = "loading") {
                        Box(Modifier.fillMaxWidth().padding(vertical = 48.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = ur.accent) }
                    } else model?.cards?.filter { it.report != INVESTMENTS_REPORT || holdings }?.chunked(across)?.forEach { row ->
                        item(key = row.first().report, contentType = "cards") {
                            if (across == 1) OverviewCardView(row.first(), Modifier, onOpen)
                            else Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
                                row.forEach { card -> OverviewCardView(card, Modifier.weight(1f), onOpen) }
                                if (row.size < across) Spacer(Modifier.weight(1f))
                            }
                        }
                    }
                }
            }
        }
    }
}

/**
 * A report's card: its name, which opens it, and its question; one figure with a short note; and a
 * small chart. A finger on a month of the chart reads that month in the figure's place.
 */
@Composable
private fun OverviewCardView(card: OverviewCard, modifier: Modifier, onOpen: (String) -> Unit) {
    val info = ReportList.firstOrNull { it.id == card.report } ?: return
    var picked by remember(card) { mutableIntStateOf(-1) }
    val title = stringResource(info.name)
    CardPart(top = true, bottom = true, modifier) {
        Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable(onClickLabel = stringResource(R.string.common_open_named, title)) { onOpen(card.report) }.heightIn(min = 40.dp),
                verticalAlignment = Alignment.CenterVertically) {
                Icon(info.icon, null, Modifier.size(18.dp), tint = ur.accent)
                Spacer(Modifier.width(8.dp))
                Text(title, Modifier.weight(1f), color = ur.accent, fontSize = 16.sp, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = ur.muted)
            }
            Text(stringResource(info.question), Modifier.padding(bottom = 4.dp), color = ur.muted, fontSize = 14.sp, lineHeight = 18.sp)
            val bars = card.bars
            if (bars != null && picked in bars.titles.indices) {
                if (bars.series.size > 1) PickedMonth(bars, picked)
                else {
                    BigFigure(bars.texts[0][picked], if (bars.series[0].values[picked] < 0) ur.negative else ur.text)
                    Text(bars.titles[picked], color = ur.muted, fontSize = 15.sp)
                }
            } else {
                BigFigure(card.figure, toneColor(card.tone))
                Text(card.note, color = ur.text2, fontSize = 15.sp, lineHeight = 20.sp)
            }
            if (bars != null) MiniBars(bars, selected = picked, onSelect = { picked = it })
            if (card.split.isNotEmpty()) {
                SegmentBar(card.split, Modifier.padding(top = 6.dp))
                ChartLegend(card.split.map { LegendItem("${Money.own(it.label)} ${it.valueText}", it.color) })
            }
        }
    }
}

/** A month picked on a card's small chart of two series: its name, then each series' figure. */
@Composable
private fun PickedMonth(bars: MiniBarsModel, i: Int) {
    val palette = reportPalette()
    Column(Modifier.padding(bottom = 4.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(bars.titles.getOrElse(i) { "" }, color = ur.muted, fontSize = 15.sp)
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            bars.series.forEachIndexed { k, s ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    LegendKey(palette.color(s.color))
                    Spacer(Modifier.width(6.dp))
                    Text(bars.texts.getOrNull(k)?.getOrNull(i).orEmpty(), color = ur.text, fontSize = 18.sp, fontWeight = FontWeight.SemiBold, style = Tabular, maxLines = 1)
                }
            }
        }
    }
}

@Composable
private fun BigFigure(text: String, color: Color) {
    Text(text, color = color, fontSize = 32.sp, lineHeight = 38.sp, fontWeight = FontWeight.ExtraBold, style = Tabular, maxLines = 1, overflow = TextOverflow.Ellipsis)
}

// ---------- A report ----------

/** The report's one filter: a line saying which months and accounts are in force, and the way to change them. */
@Composable
private fun FilterLine(text: String, active: Boolean, onClick: () -> Unit) {
    val shape = RoundedCornerShape(19.dp)
    val ink = if (active) ur.accent else ur.text2
    Row(Modifier.padding(top = 10.dp, bottom = 2.dp).heightIn(min = 38.dp).clip(shape).background(if (active) ur.accentSoft else ur.surface)
        .border(1.dp, if (active) ur.accent.copy(alpha = .35f) else ur.border, shape)
        .clickable(role = Role.Button, onClickLabel = stringResource(R.string.filters_title), onClick = onClick).padding(start = 13.dp, end = 8.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Outlined.Tune, null, Modifier.size(16.dp), tint = ink)
        Spacer(Modifier.width(6.dp))
        Text(text, Modifier.weight(1f, fill = false), color = ink, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Icon(Icons.Outlined.KeyboardArrowDown, null, Modifier.padding(start = 2.dp).size(18.dp), tint = ink)
    }
}

/**
 * What the filter's line opens: a row for the months, the accounts and (where they apply) the
 * categories, each leading to its own picker, and the export as the last row.
 */
@Composable
private fun FilterSheet(range: String?, accounts: String, categories: String?, onRange: () -> Unit, onAccounts: () -> Unit, onCategories: () -> Unit,
                        onExport: (() -> Unit)?, onDismiss: () -> Unit) {
    UrSheet(onDismiss) {
        SheetHeader(stringResource(R.string.filters_title), onDismiss, null, null)
        ListCard(Modifier.padding(start = Gutter, end = Gutter, bottom = 28.dp)) {
            if (range != null) ListRow(stringResource(R.string.reports_months), value = range, icon = Icons.Outlined.CalendarMonth, onClick = onRange, divider = true)
            ListRow(stringResource(R.string.tab_accounts), value = accounts, icon = Icons.Outlined.AccountBalanceWallet, onClick = onAccounts, divider = categories != null || onExport != null)
            if (categories != null) ListRow(stringResource(R.string.reports_categories_filter), value = categories, icon = Icons.Outlined.Category, onClick = onCategories, divider = onExport != null)
            if (onExport != null) ListRow(stringResource(R.string.reports_export_csv), icon = Icons.Outlined.FileDownload, onClick = onExport, chevron = false)
        }
    }
}

@Composable
private fun ReportPage(
    info: ViewInfo, place: ReportInfo, budget: JSONObject, vm: BudgetViewModel, bar: @Composable RowScope.() -> Unit, cache: ReportBudgetCache, context: ReportBudget?,
    today: LocalDate, range: MonthRange, setRange: (MonthRange) -> Unit, accounts: Set<String>, setAccounts: (Set<String>) -> Unit,
    categories: Set<String>, setCategories: (Set<String>) -> Unit, choose: (String) -> Unit, back: (() -> Unit)?,
    openRegister: ((RegisterTarget) -> Unit)?, openHoldings: ((String) -> Unit)?, setUpHoldings: ((String) -> Unit)?, holdings: Boolean,
    editTransaction: (JSONObject) -> Unit,
) {
    val forecast = info.id == "forecast"
    // The Investments report's views read the portfolio in place of the report; the Balance sheet reads both, to split what the investment accounts hold.
    val investing = place.id == INVESTMENTS_REPORT
    val needsPortfolio = investing || (info.id == "balance" && holdings)
    val thisMonth = YearMonth.from(today).toString()
    var attempt by remember { mutableIntStateOf(0) }
    // Reports that don't split by category ignore the Categories filter.
    val key = ReportKey(budget, range.from.ifEmpty { context?.earliestMonth ?: thisMonth }, range.to.ifEmpty { thisMonth }, accounts.sorted(),
        if (info.categories) categories.sorted() else emptyList(), attempt)
    val latestKey by rememberUpdatedState(key)
    var data by remember { mutableStateOf<ScopedResult<ReportData>?>(null) }
    var error by remember { mutableStateOf<ScopedResult<String>?>(null) }
    LaunchedEffect(key, forecast, investing) {
        if (forecast || investing) return@LaunchedEffect
        // A report belongs to its exact snapshot and filters; new labels must never describe old figures.
        vm.await("reports", key.filter()).fold(
            { json -> data = ScopedResult(key, withContext(Dispatchers.Default) { parseReport(json) }) },
            { error = ScopedResult(key, it.message ?: Words[R.string.reports_failed_one]) })
    }
    val current = data?.takeIf { it.scope == key }
    var portfolio by remember { mutableStateOf<ScopedResult<Portfolio>?>(null) }
    LaunchedEffect(key, needsPortfolio) {
        if (!needsPortfolio) return@LaunchedEffect
        vm.await("investmentsReport", key.filter()).fold(
            { json -> portfolio = ScopedResult(key, withContext(Dispatchers.Default) { parsePortfolio(json) }) },
            { error = ScopedResult(key, it.message ?: Words[R.string.reports_failed_one]) })
    }
    val held = portfolio?.takeIf { it.scope == key }

    // A report's own choices start afresh when another report opens, as the desktop's do.
    var group by rememberSaveable(info.id) { mutableStateOf<String?>(null) }
    // Back leaves a group drilled into before it leaves the report.
    BackHandler(info.id == "categories" && group != null) { group = null }
    var byGroup by rememberSaveable(info.id) { mutableStateOf(true) }
    var chartAccounts by remember(info.id) { mutableStateOf<Map<String, Int>?>(null) }
    var expanded by rememberSaveable(info.id, stateSaver = IdSetSaver) { mutableStateOf(emptySet<String>()) }
    var payeeQuery by rememberSaveable(info.id) { mutableStateOf("") }
    var allocationBy by rememberSaveable(info.id) { mutableStateOf("kind") }
    // The week starts where the phone's region starts it, whatever language the app is in.
    val weekStart = remember { localeWeekStart(android.content.res.Resources.getSystem().configuration.locales[0]) }
    val scope = ReportScope(key.from, key.to, accounts, info.categories && categories.isNotEmpty(), today, weekStart)
    val options = ReportOptions(group, byGroup, chartAccounts, allocationBy)
    val language = languageKey()
    val viewKey = ViewKey(key, info.id, options, scope, language)
    var view by remember { mutableStateOf<ScopedResult<ReportView?>?>(null) }
    LaunchedEffect(viewKey, current != null, held != null) {
        val owned = held?.value
        if (needsPortfolio && owned == null) return@LaunchedEffect
        if (investing) view = ScopedResult(viewKey, withContext(Dispatchers.Default) { owned?.let { investmentsView(info.id, it, cache.get(key.budget, today), scope, options) } })
        else {
            val figures = current ?: return@LaunchedEffect
            view = ScopedResult(viewKey, withContext(Dispatchers.Default) { reportView(info.id, figures.value, cache.get(key.budget, today), scope, options, owned) })
        }
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
    // Money moved into or out of investments is neither spending nor income: Spending and Income say how much they leave out.
    val invested = current?.value?.invested ?: 0L
    val investedText = remember(info.id, invested, currency, language) { investedNote(info.id, invested, currency) }

    // Filters
    var filterOpen by remember { mutableStateOf(false) }
    var rangeOpen by remember { mutableStateOf(false) }
    var accountsOpen by remember { mutableStateOf(false) }
    var categoriesOpen by remember { mutableStateOf(false) }
    var everydayOpen by remember { mutableStateOf(false) }
    val accountGroups = remember(context, forecast, investing) { context?.let { accountFilterGroups(it, forecast, investing) }.orEmpty() }
    val categoryGroups = remember(context) { context?.let { categoryFilterGroups(it) }.orEmpty() }
    val rangeLabel = describeRange(range)
    val accountLabel = filterSummary(accountGroups, accounts, stringResource(if (forecast) R.string.reports_all_budget_accounts else R.string.reports_all_accounts)) {
        Words.count(R.plurals.filter_accounts, it.toLong())
    }
    val categoryLabel = filterSummary(categoryGroups, categories, stringResource(R.string.category_all)) { Words.count(R.plurals.filter_categories, it.toLong()) }
    // The filter's line: the months where the view has them, the accounts, and the categories only when they're narrowed.
    val ranged = rangeApplies(info.id)
    val filterText = filterLine(rangeLabel.takeIf { ranged }, accountLabel, categoryLabel.takeIf { info.categories && categories.isNotEmpty() })

    // Export: the engine writes the CSV for these exact filters, then the system asks where to save it.
    var csv by remember { mutableStateOf("") }
    var exportError by remember { mutableStateOf<String?>(null) }
    val androidContext = LocalContext.current
    val exporter = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("text/csv")) { uri ->
        val content = csv
        if (uri != null) coroutines.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    replacing { mode -> androidContext.contentResolver.openOutputStream(uri, mode) }?.bufferedWriter()?.use { it.write(content) } ?: error(androidContext.getString(R.string.export_destination_failed))
                }
            }.onFailure { exportError = it.message }
        }
    }
    val exportCsv: () -> Unit = {
        val exportKey = key
        exportError = null
        coroutines.launch {
            vm.await(if (investing) "investmentsCsv" else "reportCsv", exportKey.filter()).fold(
                { json -> if (exportKey == latestKey) { csv = json.text("items"); exporter.launch("urbudget-${if (investing) "investments" else "report"}-${exportKey.from}-${exportKey.to}.csv") } },
                { if (exportKey == latestKey) exportError = it.message })
        }
    }

    // The ways out of a report are the app's of the moment, without making the report's actions anew each time.
    val openHoldingsNow by rememberUpdatedState(openHoldings)
    val setUpHoldingsNow by rememberUpdatedState(setUpHoldings)
    val actions = remember(key, info.id, openHoldings != null, setUpHoldings != null) {
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
            setBy = { allocationBy = it },
            openHoldings = if (openHoldings != null) ({ account -> openHoldingsNow?.invoke(account) }) else null,
            setUpHoldings = if (setUpHoldings != null) ({ account -> setUpHoldingsNow?.invoke(account) }) else null,
        )
    }
    // Nothing stays picked once the figures change.
    val pick = remember(key, info.id, shown?.value) { ChartPick() }
    val views = reportOfView(info.id)?.views.orEmpty()
    val chromeItems = (if (views.size > 1) 3 else 2) + (if (investedText != null) 1 else 0) + (if (notice != null) 1 else 0) + (if (failed != null && !forecast) 1 else 0) + (if (exportError != null) 1 else 0)
    val jumpToPayees: () -> Unit = { payeeView?.let { v -> coroutines.launch { listState.animateScrollToItem(chromeItems + payeeListOffset(v)) } } }
    val bodyActions = remember(actions, payeeView, chromeItems) {
        ReportActions(actions.drill, actions.setGroup, actions.setByGroup, actions.toggleChart, actions.toggleExpand, jumpToPayees, actions.setBy, actions.openHoldings, actions.setUpHoldings)
    }

    GlassTop(Modifier.fillMaxSize(), top = {
        PhoneTopBar(stringResource(place.name), onBack = back, actions = bar)
        ProgressLine(if (forecast) forecastLoading else failed == null && !ready)
    }) {
        CompositionLocalProvider(LocalStale provides (if (forecast) forecastStale else stale)) {
            PageList(Modifier.fillMaxSize(), state = listState, contentPadding = PaddingValues(start = Gutter, end = Gutter, bottom = DockRoom)) {
                item(key = "question", contentType = "chrome") {
                    Text(stringResource(place.question), Modifier.padding(top = 2.dp, bottom = 10.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
                }
                // The report's views, on one switch; a report with one view has none.
                if (views.size > 1) item(key = "views", contentType = "chrome") {
                    Segmented(views.map { id -> id to stringResource(ViewLabels[id] ?: place.name) }, info.id, choose)
                }
                item(key = "filter", contentType = "chrome") {
                    Row(Modifier.fillMaxWidth()) {
                        FilterLine(filterText, accounts.isNotEmpty() || (info.categories && categories.isNotEmpty())) { filterOpen = true }
                    }
                }
                investedText?.let { text ->
                    item(key = "invested", contentType = "chrome") { HelpText(text, Modifier.padding(top = 8.dp, start = 4.dp, end = 4.dp)) }
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
                if (forecast) forecastBody(forecastShown?.value, ahead, { ahead = it }, forecastFailed, { attempt++ }, { everydayOpen = true }, { everyday = null },
                    "fold:forecast" in expanded) { actions.toggleExpand("fold:forecast") }
                else if (failed == null) {
                    val body = shown?.value
                    if (body != null) reportBody(body, bodyActions, expanded, payeeQuery, { payeeQuery = it }, filteredPayees ?: payeeView?.rows.orEmpty(), pick)
                    else if (view?.scope == viewKey) item(key = "empty", contentType = "empty") { EmptyState(stringResource(R.string.reports_empty_title), stringResource(R.string.reports_empty_detail)) }
                    else item(key = "loading", contentType = "loading") {
                        Box(Modifier.fillMaxWidth().padding(vertical = 48.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(color = ur.accent) }
                    }
                }
            }
        }
    }

    // The forecast has no file to export: it looks ahead from what's scheduled.
    if (filterOpen) FilterSheet(rangeLabel.takeIf { ranged }, accountLabel, categoryLabel.takeIf { info.categories },
        onRange = { filterOpen = false; rangeOpen = true }, onAccounts = { filterOpen = false; accountsOpen = true }, onCategories = { filterOpen = false; categoriesOpen = true },
        onExport = if (forecast) null else ({ filterOpen = false; exportCsv() })) { filterOpen = false }
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
