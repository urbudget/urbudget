package app.urbudget.nativeapp.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.BoundsTransform
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.animateBounds
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.outlined.Sort
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.LookaheadScope
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.ColumnModel
import app.urbudget.nativeapp.ColumnSeries
import app.urbudget.nativeapp.HoldingsPage
import app.urbudget.nativeapp.Investments
import app.urbudget.nativeapp.LegendItem
import app.urbudget.nativeapp.LineSeries
import app.urbudget.nativeapp.LinesModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Readout
import app.urbudget.nativeapp.ReportColors
import app.urbudget.nativeapp.SliceModel
import app.urbudget.nativeapp.Tone
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.XTick
import app.urbudget.nativeapp.columnModel
import app.urbudget.nativeapp.compactMoney
import app.urbudget.nativeapp.linesModel
import app.urbudget.nativeapp.plainTick
import app.urbudget.nativeapp.reportMonthName
import org.json.JSONObject
import java.time.LocalDate
import java.util.Locale

/*
 * An investment account's page (the desktop's Holdings view, Investments.tsx): how it did over a
 * period with a chart of its value or its gain, the period's ups and downs, what it holds with
 * each holding's share and return, each month's gain, and what it earned and was charged. The
 * figures are the engine's (`investmentPage`, investment_stats.rs); nothing is worked out here.
 *
 * It opens from the account's card (HoldingsCard) as a page of its own in the account's place, and
 * the app holds it open, so folding or unfolding the phone keeps it as it was: the period, the chart
 * and the order chosen. Its layout follows the screen. A phone, or a Fold's cover screen, reads it
 * as one column. With room for two, how it did stands on the start side and what it holds on the end
 * side, each scrolling on its own. Half open like a book, one is on each side of the hinge; half open
 * on a table, the chart stands on the upright half and what's touched (the periods first) lies on the
 * flat half.
 */

/** A figure with its name, and a line of detail under it. */
private data class Stat(val label: String, val value: String, val note: String? = null, val tone: Tone = Tone.None)

/**
 * One thing the account holds, as its row shows it. [mark] is the few letters on its tile (a symbol,
 * a currency); none for interest still to be paid, which has a clock. The investment reports list
 * holdings and accounts in the same rows.
 */
private typealias HoldingLine = app.urbudget.nativeapp.HoldingRowModel

/** A line of what the account earned or was charged. */
private data class IncomeLine(val label: String, val note: String?, val amount: String, val tone: Tone)

/** The page's figures, written out away from composition. */
private class PageView(
    val total: String, val asOf: String, val stale: Boolean, val gainLabel: String, val gain: String, val gainTone: Tone, val rate: String?,
    val valueChart: LinesModel?, val gainChart: LinesModel?, val periodStats: List<Stat>,
    val lacking: List<String>, val estimated: String?, val estimatedDays: List<Pair<String, String>>,
    val kinds: List<SliceModel>, val currencies: String?, val holdStats: List<Stat>,
    val count: Int, val holdings: List<HoldingLine>, val others: List<HoldingLine>,
    val months: ColumnModel?, val incomeTitle: String?, val incomeStats: List<Stat>, val income: List<IncomeLine>,
    val footnote: String,
)

/** What the engine calls a kind of holding ("Stocks", "Stock"), in the interface's language. */
private fun kindName(key: String) = HoldingsPage.kindName(key)

private fun toned(amount: Long) = if (amount > 0) Tone.Positive else if (amount < 0) Tone.Negative else Tone.None
/**
 * A percentage, left to right in an Arabic sentence too. One that opens with "<" has no letter or
 * digit at its head to say so, and would come out as "0.1%>": a left-to-right mark leads it.
 */
private fun figure(text: String) = Money.ltr(if (text.startsWith("<")) "‎$text" else text)
private fun percent(share: Double, signed: Boolean = false) = figure(HoldingsPage.percent(share, signed))

/** The chart's dates under its axis: "3 Nov" by the week, "Nov" by the month, the year where it turns. */
private fun dayTicks(days: List<String>): List<XTick?> {
    val marks = HoldingsPage.ticks(days)
    return days.mapIndexed { i, day ->
        val mark = marks[i] ?: return@mapIndexed null
        val date = runCatching { LocalDate.parse(day) }.getOrNull() ?: return@mapIndexed null
        plainTick(when (mark) {
            HoldingsPage.Tick.Day -> date.format(Money.pattern("d MMM"))
            HoldingsPage.Tick.Month -> date.format(Money.pattern("MMM"))
            HoldingsPage.Tick.Year -> date.year.toString()
        })
    }
}

/** The currency named beside the axis, when amounts are written with a sign or code of their own. */
private fun axisUnit(currency: String) = Money.format(0, currency).takeIf { text -> text.any { !it.isDigit() && it !in ".,' - " } }?.let { currency }

/**
 * What the account is worth, over what it would be worth with no gain or loss since the period
 * began: the gap between the two lines is the gain. The axis fits the values. A finger on it reads
 * the day's value and gain first, then the line under.
 */
private fun valueChart(series: HoldingsPage.Series, label: String, currency: String): LinesModel {
    val value = LineSeries("value", Words[R.string.ibkr_value], 0, series.values.map { it.toDouble() })
    val before = LineSeries("before", Words[R.string.ibkr_value_before_gains], ReportColors.OTHER, HoldingsPage.before(series), dashed = true)
    // The dashed line goes down first, so the value and its end dot are drawn over it where they meet.
    val lines = listOf(before, value)
    val known = lines.flatMap { it.values.filterNotNull() }
    val ticks = HoldingsPage.fittedTicks(known.minOrNull() ?: 0.0, known.maxOrNull() ?: 1.0)
    val readouts = series.days.indices.map { i ->
        listOfNotNull(value.values[i]?.let { Readout(value.label, Money.format(Math.round(it), currency), value.color, line = true) },
            Readout(Words[R.string.ibkr_gain], Money.signed(series.gains[i], currency), null),
            before.values[i]?.let { Readout(before.label, Money.format(Math.round(it), currency), before.color, line = true, dashed = true) })
    }
    // No wash under the line: the axis doesn't reach zero, where a wash would end.
    return LinesModel(label, series.days, lines, false, ticks, ticks.map { compactMoney(it) }, axisUnit(currency), dayTicks(series.days),
        series.days.map(Money::date), readouts, lines.map { null })
}

/** The gain so far on each day of the period, from nothing: green when it ends up, red when it ends down. */
private fun gainChart(series: HoldingsPage.Series, label: String, currency: String): LinesModel {
    val color = if ((series.gains.lastOrNull() ?: 0L) < 0) ReportColors.NEGATIVE else ReportColors.POSITIVE
    val ticks = dayTicks(series.days)
    val model = linesModel(label, series.days, listOf(LineSeries("gain", Words[R.string.ibkr_gain], color, series.gains.map { it.toDouble() })),
        { Money.signed(Math.round(it), currency) }, { compactMoney(it) }, axisUnit(currency), xTick = { _, i -> ticks[i] }, title = Money::date, wash = true, current = null)
    return model.copy(endLabels = listOf(null),
        readouts = model.readouts.mapIndexed { i, row -> row + Readout(Words[R.string.ibkr_value], Money.format(series.values[i], currency), null) })
}

/** Each month's gain as a column, up or down. */
private fun monthsChart(months: List<JSONObject>, currency: String): ColumnModel {
    val gains = months.map { it.optLong("gain").toDouble() }
    val model = columnModel(Words[R.string.ibkr_chart_months], months.map { it.optString("month") },
        listOf(ColumnSeries("gain", Words[R.string.ibkr_gain], ReportColors.POSITIVE, gains.map { maxOf(0.0, it) }),
            ColumnSeries("loss", Words[R.string.ibkr_loss], ReportColors.NEGATIVE, gains.map { minOf(0.0, it) })),
        { Money.signed(Math.round(it), currency) }, { compactMoney(it) }, axisUnit(currency), current = null)
    // A month is a gain or a loss, never both: their total would only say it twice.
    return model.copy(readouts = model.readouts.map { row -> row.filter { it.color != null } })
}

private fun buildView(page: JSONObject, period: String, sort: String, currency: String): PageView {
    val total = page.optLong("total")
    fun money(amount: Long) = Money.format(amount, currency)
    fun signed(amount: Long) = Money.signed(amount, currency)
    fun ofAccount(share: String) = Words[R.string.ibkr_share_of_account, share]
    val did = HoldingsPage.period(page, period) ?: HoldingsPage.period(page, "all")
    val series = did?.let { HoldingsPage.series(page, it) }
    val charted = series != null && series.days.size > 2
    val from = did?.let { Money.date(it.optString("from")) }.orEmpty()
    val to = did?.let { Money.date(it.optString("to")) }.orEmpty()
    val gain = did?.optLong("gain") ?: 0L

    // How the period went.
    val periodStats = buildList {
        page.optJSONObject("dayChange")?.let { add(Stat(Words[R.string.ibkr_change_on, Money.date(page.optString("asOf"))], signed(it.optLong("amount")), tone = toned(it.optLong("amount")))) }
        if (did != null) add(Stat(Words[R.string.ibkr_money_put_in], signed(did.optLong("put")), Words[R.string.ibkr_since, from]))
        did?.optJSONObject("swings")?.let { moves ->
            moves.optJSONObject("best")?.let { add(Stat(Words[R.string.ibkr_best_day], signed(it.optLong("amount")), Money.date(it.optString("date")), Tone.Positive)) }
            moves.optJSONObject("worst")?.let { add(Stat(Words[R.string.ibkr_worst_day], signed(it.optLong("amount")), Money.date(it.optString("date")), Tone.Negative)) }
            moves.optJSONObject("drop")?.let { drop ->
                val span = Words[R.string.ibkr_from_to, Money.date(drop.optString("from")), Money.date(drop.optString("to"))]
                add(Stat(Words[R.string.ibkr_largest_drop], signed(drop.optLong("amount")), Money.joined(listOfNotNull(HoldingsPage.rate(drop, "rate")?.let { percent(it, true) }, span)), Tone.Negative))
            }
            val moved = moves.optInt("moved")
            if (moved > 0) add(Stat(Words[R.string.ibkr_days_up], percent(moves.optInt("up").toDouble() / moved),
                Words.count(R.plurals.ibkr_days_up_of, moved.toLong(), Money.count(moves.optLong("up")))))
        }
    }

    // What it's made of, among the parts worth something: a short option isn't a share of it.
    val sums = page.optJSONObject("totals") ?: JSONObject()
    val byKind = sums.rows("byKind")
    fun kindColor(key: String) = byKind.indexOfFirst { it.optString("key") == key }.let { if (it in 0..7) it else ReportColors.OTHER }
    val kinds = byKind.filter { it.optLong("value") > 0 }.map { part ->
        SliceModel(part.optString("key"), kindName(part.optString("key")), part.optLong("value"), kindColor(part.optString("key")), money(part.optLong("value")), percent(maxOf(0.0, part.optDouble("share"))))
    }
    val byCurrency = sums.rows("byCurrency").filter { it.optLong("value") > 0 }
    val currencies = if (byCurrency.size > 1) Money.ltr(byCurrency.joinToString(" · ") { "${it.optString("key")} ${HoldingsPage.percent(maxOf(0.0, it.optDouble("share")))}" }) else null

    val holdStats = buildList {
        HoldingsPage.amount(sums, "unrealised")?.let { unrealised ->
            add(Stat(Words[R.string.ibkr_gain_on_held], signed(unrealised),
                Words[R.string.ibkr_rate_on_cost, HoldingsPage.rate(sums, "unrealisedRate")?.let { percent(it, true) } ?: "—", money(sums.optLong("cost"))], toned(unrealised)))
        }
        val cash = sums.optLong("cash")
        add(Stat(Words[R.string.ibkr_cash], money(cash), if (total > 0) ofAccount(if (cash > 0) percent(cash.toDouble() / total) else percent(0.0)) else null))
        page.optJSONObject("biggest")?.let { add(Stat(Words[R.string.ibkr_largest_holding], percent(it.optDouble("rate")), Money.own(it.optString("name")))) }
        page.optJSONObject("best")?.let { add(Stat(Words[R.string.ibkr_best_performer], percent(it.optDouble("rate"), true), Money.own(it.optString("name")), toned(Math.round(it.optDouble("rate") * 1e6)))) }
        page.optJSONObject("worst")?.let { add(Stat(Words[R.string.ibkr_worst_performer], percent(it.optDouble("rate"), true), Money.own(it.optString("name")), toned(Math.round(it.optDouble("rate") * 1e6)))) }
    }

    // Each holding, with its bar drawn against the largest line of the account.
    val largest = maxOf(1L, page.optLong("largest"))
    fun bar(value: Long) = if (value > 0) (value.toFloat() / largest).coerceIn(0.02f, 1f) else 0f
    val positions = page.rows("holdings")
    // A holding keeps its key in any order, so a new order moves its row rather than making it again.
    val place = java.util.IdentityHashMap<JSONObject, Int>().apply { positions.forEachIndexed { i, row -> put(row, i) } }
    val holdings = HoldingsPage.sorted(positions, sort).map { row ->
        val rowGain = HoldingsPage.amount(row, "gain")
        HoldingLine("holding:${place[row]}:${row.optString("symbol")}:${row.optString("currency")}", row.optString("name"),
            // What it is (an ETF, a put option), else its kind; a source that doesn't say leaves it out.
            Money.joined(listOfNotNull(HoldingsPage.instrument(row)?.let(HoldingsPage::instrumentName), Money.ltr(HoldingsPage.line(row)))), money(row.optLong("value")),
            rowGain?.let(::signed), HoldingsPage.rate(row, "rate")?.let { percent(it, true) }, rowGain ?: 0L,
            HoldingsPage.rate(row, "share")?.let { percent(it) }, bar(row.optLong("value")), kindColor(row.optString("group")),
            HoldingsPage.mark(row.optString("symbol"), row.optString("name")))
    }
    val others = page.rows("cash").map { row ->
        HoldingLine("cash:${row.optString("currency")}", Words[R.string.ibkr_cash], Money.ltr("${Investments.plain(row.optString("amount"), 2)} ${row.optString("currency")}"),
            money(row.optLong("value")), null, null, 0L, HoldingsPage.rate(row, "share")?.let { percent(it) }, bar(row.optLong("value")), kindColor("Cash"),
            row.optString("currency").uppercase(Locale.ROOT))
    } + listOfNotNull(page.optLong("accrued").takeIf { it != 0L }?.let { accrued ->
        HoldingLine("accrued", Words[R.string.ibkr_accrued], Words[R.string.ibkr_accrued_detail], money(accrued), null, null, 0L,
            if (total > 0 && accrued > 0) percent(accrued.toDouble() / total) else null, bar(accrued), kindColor("Accrued"), null)
    })

    // What it earned and was charged.
    val paid = page.optJSONObject("earned")
    val commissions = paid?.optJSONObject("commissions")
    val incomeStats = if (paid == null) emptyList() else {
        val income = paid.optLong("dividends") + paid.optLong("interest")
        val costs = paid.optLong("fees") + paid.optLong("tax") + (commissions?.optLong("amount") ?: 0L)
        fun share(amount: Long) = if (total > 0 && amount != 0L) ofAccount(figure(HoldingsPage.tenths(amount.toDouble() / total))) else null
        listOf(Stat(Words[R.string.ibkr_dividends_interest], signed(income), share(income), toned(income)),
            Stat(Words[R.string.ibkr_fees_tax_commissions], signed(costs), share(costs), if (costs < 0) Tone.Negative else Tone.None))
    }
    val income = if (paid == null) emptyList() else listOf(
        Triple(R.string.ibkr_dividends, "dividends", null), Triple(R.string.ibkr_interest, "interest", null), Triple(R.string.ibkr_tax_withheld, "tax", null),
        Triple(R.string.ibkr_fees, "fees", R.string.ibkr_fees_note), Triple(R.string.ibkr_one_other, "other", null),
    ).filter { (_, key, _) -> paid.optLong(key) != 0L }.map { (label, key, note) -> IncomeLine(Words[label], note?.let { Words[it] }, signed(paid.optLong(key)), toned(paid.optLong(key))) } +
        listOfNotNull(commissions?.let {
            IncomeLine(Words[R.string.ibkr_commissions], Words[R.string.ibkr_from_to, Money.date(it.optString("from")), Money.date(it.optString("to"))], signed(it.optLong("amount")), toned(it.optLong("amount")))
        })

    val estimated = page.rows("estimated")
    val months = page.rows("monthly")
    val asOf = page.optString("asOf")
    val stale = Investments.stale(asOf)
    val byBroker = page.optString("source", "ibkr") == "ibkr"
    return PageView(
        total = money(total), asOf = Words[R.string.ibkr_prices_from_date, Money.date(asOf)], stale = stale,
        gainLabel = when (period) {
            "1m" -> Words[R.string.ibkr_gain_1m]; "3m" -> Words[R.string.ibkr_gain_3m]; "ytd" -> Words[R.string.ibkr_gain_ytd]; "1y" -> Words[R.string.ibkr_gain_1y]
            else -> Words[R.string.ibkr_gain_since, from]
        },
        gain = signed(gain), gainTone = toned(gain), rate = did?.let { HoldingsPage.rate(it, "rate") }?.let { percent(it, true) },
        valueChart = if (charted) valueChart(series!!, Words[R.string.ibkr_chart_value, from, to], currency) else null,
        gainChart = if (charted) gainChart(series!!, Words[R.string.ibkr_chart_gain, from, to], currency) else null,
        periodStats = periodStats,
        lacking = page.optJSONArray("missing")?.let { list -> (0 until list.length()).mapNotNull { HoldingsPage.lackName(list.optString(it)) } }.orEmpty(),
        estimated = if (estimated.isEmpty()) null else Words.count(R.plurals.ibkr_estimated_days, estimated.size.toLong()),
        estimatedDays = estimated.map { Money.date(it.optString("date")) to signed(it.optLong("amount")) },
        kinds = kinds, currencies = currencies, holdStats = holdStats,
        count = positions.size, holdings = holdings, others = others,
        months = if (months.size > 1) monthsChart(months, currency) else null,
        incomeTitle = paid?.let { Words[R.string.ibkr_from_to, reportMonthName(it.optString("from"), short = true), reportMonthName(it.optString("to"), short = true)] },
        incomeStats = incomeStats, income = income,
        // The prices' date stands under the value; a broker's note says where the figures come from.
        footnote = if (byBroker) listOfNotNull(Words[R.string.ibkr_source_note, Money.ltr(page.optString("sourceAccount"))], if (stale) Words[R.string.ibkr_stale] else null).joinToString(" ")
        else Investments.sourceNote(page),
    )
}

/** What the page lets a person choose, kept while the phone folds and unfolds. */
@Stable
private class PageChoices(
    period: MutableState<String>, chart: MutableState<String>, sort: MutableState<String>,
    all: MutableState<Boolean>, days: MutableState<Boolean>, kind: MutableState<String?>,
) {
    var period by period
    var chart by chart
    var sort by sort
    var all by all
    var days by days
    var kind by kind
}

/** From this much room the page stands in two columns. */
private val TwoColumns = 700.dp

/** The gap between the page's two columns, where no hinge runs between them. */
private val ColumnGap = 16.dp

@Composable
internal fun HoldingsScreen(account: JSONObject, budget: JSONObject, vm: BudgetViewModel, onBack: () -> Unit, bar: @Composable RowScope.() -> Unit) {
    val accountId = account.text("id")
    val currency = budget.text("currency")
    val requests = rememberPreviewJobs(accountId)
    var page by remember(accountId) { mutableStateOf<JSONObject?>(null) }
    var error by remember(accountId) { mutableStateOf<String?>(null) }
    val choices = PageChoices(
        rememberSaveable(accountId) { mutableStateOf("1y") }, rememberSaveable(accountId) { mutableStateOf("value") },
        rememberSaveable(accountId) { mutableStateOf("value") }, rememberSaveable(accountId) { mutableStateOf(false) },
        rememberSaveable(accountId) { mutableStateOf(false) }, remember(accountId) { mutableStateOf(null) })
    // Read again when the budget changes under the page: a sync brings new days and prices.
    LaunchedEffect(budget, accountId) {
        requests.track(vm.query("investmentPage", JSONObject().put("accountId", accountId), onError = { error = it }) { read ->
            if (read.has("periods")) { page = read; error = null }
        })
    }
    val language = languageKey()
    val view = rememberPrepared(page, choices.period, choices.sort, language) { page?.let { buildView(it, choices.period, choices.sort, currency) } }
    // Each layout's lists keep their place while the phone folds and unfolds and comes back; opened
    // again, the page starts at its top (the choices above are kept).
    val single = remember { LazyListState() }
    val start = remember { LazyListState() }
    val end = remember { LazyListState() }
    // On a table the flat half's list leads with the periods, so it has a place of its own.
    val flat = remember { LazyListState() }
    val topBar: @Composable ColumnScope.() -> Unit = {
        PhoneTopBar(account.text("name"), stringResource(R.string.ibkr_details), onBack = onBack, actions = bar)
        if (view == null && error == null) LinearProgressIndicator(Modifier.fillMaxWidth().padding(horizontal = Gutter).height(2.dp), color = ur.accent, trackColor = Color.Transparent)
    }
    val hinge = verticalHinge()
    val table = tableTop()
    // Half open like a book: a phone on each side of the hinge, the bar and the dock on the start side.
    if (hinge != null) Row(Modifier.fillMaxSize()) {
        Box(Modifier.width(hinge.start).fillMaxHeight()) {
            GlassTop(Modifier.fillMaxSize(), top = topBar) {
                val room = glassRoom(start)
                LazyColumn(Modifier.fillMaxSize(), start, PaddingValues(start = Gutter, end = Gutter, top = room + 4.dp, bottom = DockRoom),
                    verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    status(error)
                    if (view != null) { performance(view, choices, 240.dp); months(view) }
                }
            }
        }
        Spacer(Modifier.width(hinge.width))
        // The far side has no bar: it starts level with the near side's first card, and scrolls up only as far as the status bar.
        LazyColumn(Modifier.weight(1f).fillMaxHeight().statusBarsPadding(), end, PaddingValues(start = Gutter, end = Gutter, top = BarHeight + 4.dp, bottom = footRoom()),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (view != null) { holds(view, choices); income(view); footnote(view) }
        }
    } else GlassTop(Modifier.fillMaxSize(), top = topBar) {
        BoxWithConstraints(Modifier.fillMaxSize()) {
            when {
                // Half open on a table: the chart on the upright half, everything to touch on the flat half.
                table != null && view != null -> Column(Modifier.fillMaxSize()) {
                    val room = LocalGlassPage.current?.room ?: 0.dp
                    Box(Modifier.fillMaxWidth().height(table).padding(start = Gutter, end = Gutter, top = room + 4.dp, bottom = 12.dp)) {
                        PerformanceCard(view, choices, null, Modifier.fillMaxSize(), controls = false)
                    }
                    LazyColumn(Modifier.weight(1f).fillMaxWidth(), flat, PaddingValues(start = Gutter, end = Gutter, top = 12.dp, bottom = footRoom()),
                        verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        item(key = "holdings:controls") { ListCard(Modifier.animateItem()) { ChartControls(view, choices, Modifier.padding(12.dp)) } }
                        notices(view, choices)
                        stats(view)
                        holds(view, choices); months(view); income(view); footnote(view)
                    }
                }
                maxWidth >= TwoColumns -> Row(Modifier.fillMaxSize()) {
                    val room = glassRoom(start, end)
                    // The holdings' side a little wider: a row's name, value and return sit side by side.
                    LazyColumn(Modifier.weight(1f).fillMaxHeight(), start, PaddingValues(start = Gutter, top = room + 4.dp, bottom = footRoom()),
                        verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        status(error)
                        if (view != null) { performance(view, choices, 260.dp); months(view) }
                    }
                    Spacer(Modifier.width(ColumnGap))
                    LazyColumn(Modifier.weight(1.15f).fillMaxHeight(), end, PaddingValues(end = Gutter, top = room + 4.dp, bottom = footRoom()),
                        verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        if (view != null) { holds(view, choices); income(view); footnote(view) }
                    }
                }
                else -> {
                    val room = glassRoom(single)
                    LazyColumn(Modifier.fillMaxSize(), single, PaddingValues(start = Gutter, end = Gutter, top = room + 4.dp, bottom = if (wideWindow()) footRoom() else DockRoom),
                        verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        status(error)
                        if (view != null) { performance(view, choices, 220.dp); holds(view, choices); months(view); income(view); footnote(view) }
                    }
                }
            }
        }
    }
}

/** A page's top bar under the status bar (PhoneTopBar). */
private val BarHeight = 64.dp

/** Room under a list with no dock over its foot: the navigation bar, and a little more. */
@Composable
private fun footRoom(): Dp = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding() + 24.dp

private fun LazyListScope.status(error: String?) {
    error?.let { item(key = "holdings:error") { Box(Modifier.animateItem()) { FormError(it) } } }
}

/** How it did: the figures and chart, what the statement lacks, and the period's ups and downs. */
private fun LazyListScope.performance(view: PageView, choices: PageChoices, chartHeight: Dp) {
    item(key = "holdings:performance") { PerformanceCard(view, choices, chartHeight, Modifier.animateItem()) }
    notices(view, choices)
    stats(view)
}

private fun LazyListScope.notices(view: PageView, choices: PageChoices) {
    if (view.lacking.isNotEmpty()) item(key = "holdings:lacking") {
        Notice((listOf(stringResource(R.string.ibkr_lacking)) + view.lacking.map { "• $it" }).joinToString("\n"), Modifier.animateItem(), tone = "info")
    }
    view.estimated?.let { text ->
        item(key = "holdings:estimated") {
            Notice(text, Modifier.animateItem(), tone = "info", action = if (choices.days) null else stringResource(R.string.ibkr_estimated_title), onAction = { choices.days = true })
        }
        if (choices.days) item(key = "holdings:estimated-days") {
            ListCard(Modifier.animateItem()) {
                Text(stringResource(R.string.ibkr_estimated_title), Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 4.dp), style = MaterialTheme.typography.labelMedium, color = ur.muted)
                view.estimatedDays.forEachIndexed { i, (day, amount) ->
                    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 9.dp)) {
                        Text(day, Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium, color = ur.text)
                        Text(amount, style = MaterialTheme.typography.bodyMedium.merge(Tabular), color = ur.text, fontWeight = FontWeight.SemiBold)
                    }
                    if (i < view.estimatedDays.lastIndex) Hairline(start = 16.dp)
                }
            }
        }
    }
}

private fun LazyListScope.stats(view: PageView) {
    if (view.periodStats.isNotEmpty()) item(key = "holdings:period") { ListCard(Modifier.animateItem()) { StatGrid(view.periodStats, Modifier.padding(16.dp)) } }
}

/** What it holds: what it's made of and a few facts about it, then each holding. Nothing for an account whose value alone is known. */
private fun LazyListScope.holds(view: PageView, choices: PageChoices) {
    if (view.holdings.isEmpty() && view.others.isEmpty()) return
    item(key = "holdings:holds-title") {
        Text(stringResource(R.string.ibkr_what_it_holds), Modifier.animateItem().padding(start = 4.dp, top = 8.dp), style = MaterialTheme.typography.titleMedium, color = ur.text)
    }
    item(key = "holdings:made-of") { MadeOf(view, choices, Modifier.animateItem()) }
    item(key = "holdings:list") { HoldingsList(view, choices, Modifier.animateItem()) }
}

private fun LazyListScope.months(view: PageView) {
    val model = view.months ?: return
    item(key = "holdings:months") {
        ListCard(Modifier.animateItem()) {
            Column(Modifier.padding(start = 12.dp, end = 12.dp, top = 14.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(stringResource(R.string.ibkr_month_by_month), Modifier.padding(horizontal = 4.dp), style = MaterialTheme.typography.titleSmall, color = ur.text)
                ColumnChart(model, height = 180.dp, readout = false)
            }
        }
    }
}

private fun LazyListScope.income(view: PageView) {
    if (view.incomeStats.isEmpty()) return
    item(key = "holdings:income") {
        ListCard(Modifier.animateItem()) {
            Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.ibkr_income_costs), Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, color = ur.text)
                    view.incomeTitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted) }
                }
                StatGrid(view.incomeStats)
            }
            Spacer(Modifier.height(8.dp))
            view.income.forEach { line ->
                Hairline(start = 16.dp)
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(line.label, style = MaterialTheme.typography.bodyMedium, color = ur.text)
                        line.note?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted) }
                    }
                    Text(line.amount, Modifier.padding(start = 10.dp), style = MaterialTheme.typography.bodyMedium.merge(Tabular), color = toneColor(line.tone), fontWeight = FontWeight.SemiBold)
                }
            }
            Hairline(start = 16.dp)
            Text(stringResource(R.string.ibkr_income_note), Modifier.padding(horizontal = 16.dp, vertical = 12.dp), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
    }
}

private fun LazyListScope.footnote(view: PageView) {
    item(key = "holdings:note") { Text(view.footnote, Modifier.animateItem().padding(horizontal = 4.dp), style = MaterialTheme.typography.bodySmall, color = ur.muted) }
}

@Composable
internal fun toneColor(tone: Tone): Color = when (tone) { Tone.Positive -> ur.positive; Tone.Negative -> ur.negative; Tone.None -> ur.text }

/**
 * The account's value and its gain over the period as two tabs over the chart: the chosen one sits in
 * the card's colour over an accent line, joined to the chart it draws, and the two slide across when
 * the other is chosen. Under them the chart, and (unless they're kept for the flat half of a phone on
 * a table) its key and the periods. With no [chartHeight] the chart takes what the card leaves it.
 */
@Composable
private fun PerformanceCard(view: PageView, choices: PageChoices, chartHeight: Dp?, modifier: Modifier = Modifier, controls: Boolean = true) {
    val charted = view.valueChart != null
    val gain = charted && choices.chart == "gain"
    val ltr = LocalLayoutDirection.current == LayoutDirection.Ltr
    val at by animateFloatAsState(if (gain) 1f else 0f, spring(dampingRatio = .9f, stiffness = Spring.StiffnessMediumLow), label = "tab")
    val accent = ur.accent
    ListCard(modifier) {
        Row(Modifier.fillMaxWidth().drawBehind {
            if (!charted) return@drawBehind
            // The chosen figure over a rounded accent bar the width of its words' column, sliding to the other.
            val half = size.width / 2
            val thick = 3.dp.toPx()
            val start = 16.dp.toPx()
            val width = half - start - 12.dp.toPx()
            val x = half * at + start
            drawRoundRect(accent, Offset(if (ltr) x else size.width - x - width, size.height - thick - 2.dp.toPx()), Size(width, thick), CornerRadius(thick / 2))
        }) {
            Metric(stringResource(R.string.ibkr_value), view.total, Tone.None, charted && !gain, if (charted) ({ choices.chart = "value" }) else null, Modifier.weight(1f),
                note = view.asOf, noteColor = if (view.stale) ur.warning else ur.muted)
            Metric(view.gainLabel, view.gain, view.gainTone, gain, if (charted) ({ choices.chart = "gain" }) else null, Modifier.weight(1f), pill = view.rate)
        }
        if (charted || controls) Column(Modifier.fillMaxWidth().then(if (chartHeight == null) Modifier.weight(1f) else Modifier)
            .padding(start = 12.dp, end = 12.dp, top = 12.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            val model = if (choices.chart == "gain") view.gainChart else view.valueChart
            if (model != null) {
                if (chartHeight != null) LinesChart(model, height = chartHeight, readout = false)
                else BoxWithConstraints(Modifier.fillMaxWidth().weight(1f)) { LinesChart(model, height = maxHeight, readout = false) }
            }
            if (controls) ChartControls(view, choices)
        }
    }
}

/** The chart's key, and the periods. Each chart has a key of one line, so the card keeps its height as one gives way to the other. */
@Composable
private fun ChartControls(view: PageView, choices: PageChoices, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        val gains = view.gainChart
        if (choices.chart == "gain" && gains != null) ChartLegend(listOf(LegendItem(stringResource(R.string.ibkr_gain), gains.series.first().color, line = true)),
            Modifier.padding(horizontal = 4.dp))
        else if (view.valueChart != null) ChartLegend(listOf(LegendItem(stringResource(R.string.ibkr_value), 0, line = true),
            LegendItem(stringResource(R.string.ibkr_value_before_gains), ReportColors.OTHER, line = true, dashed = true)), Modifier.padding(horizontal = 4.dp))
        Segmented(listOf("1m" to stringResource(R.string.ibkr_period_1m), "3m" to stringResource(R.string.ibkr_period_3m), "ytd" to stringResource(R.string.ibkr_period_ytd),
            "1y" to stringResource(R.string.ibkr_period_1y), "all" to stringResource(R.string.ibkr_period_all)), choices.period, { choices.period = it })
    }
}

/**
 * One of the two figures over the chart; with a chart under it, tapping it chooses what the chart
 * draws. The figure shrinks to fit rather than be cut short, as a long amount in SAR would be on a
 * cover screen. A new period's name, figure and return roll in over the old ones.
 */
@Composable
private fun Metric(label: String, value: String, tone: Tone, chosen: Boolean, onClick: (() -> Unit)?, modifier: Modifier,
                   note: String? = null, noteColor: Color = ur.muted, pill: String? = null) {
    val labelInk by animateColorAsState(if (chosen) ur.accent else ur.muted, label = "tab label")
    Column(modifier.then(if (onClick != null) Modifier.selectable(chosen, role = Role.Tab, onClick = onClick) else Modifier)
        .padding(start = 16.dp, end = 12.dp, top = 14.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(3.dp)) {
        Rolling(label) { Text(it, style = MaterialTheme.typography.bodySmall, color = labelInk, fontWeight = if (chosen) FontWeight.SemiBold else FontWeight.Normal,
            maxLines = 1, overflow = TextOverflow.Ellipsis) }
        Rolling(value to tone) { (figure, ink) ->
            val style = MaterialTheme.typography.titleLarge
            BasicText(figure, style = style.merge(Tabular).merge(TextStyle(color = toneColor(ink), fontWeight = FontWeight.Bold)), maxLines = 1,
                autoSize = TextAutoSize.StepBased(minFontSize = 14.sp, maxFontSize = style.fontSize.takeIf { it.isSp } ?: 22.sp, stepSize = 1.sp))
        }
        note?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = noteColor, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        pill?.let { Rolling(it to tone) { (rate, ink) -> RatePill(rate, ink) } }
    }
}

/** What a figure says, rolling up into place when it changes (a new period), the old one rising away. */
@Composable
private fun <T> Rolling(target: T, content: @Composable (T) -> Unit) {
    AnimatedContent(target, transitionSpec = {
        (fadeIn(tween(220, delayMillis = 50)) + slideInVertically(tween(300, easing = FastOutSlowInEasing)) { it / 2 }) togetherWith
            (fadeOut(tween(130)) + slideOutVertically(tween(220, easing = FastOutSlowInEasing)) { -it / 2 }) using SizeTransform(clip = false)
    }, contentAlignment = Alignment.CenterStart, label = "rolling") { content(it) }
}

/** A return as a small tinted pill: green up, red down. */
@Composable
internal fun RatePill(rate: String, tone: Tone) {
    val fill = when (tone) { Tone.Positive -> ur.positiveSoft; Tone.Negative -> ur.negativeSoft; Tone.None -> ur.zeroSoft }
    val ink = when (tone) { Tone.Positive -> ur.positive; Tone.Negative -> ur.negative; Tone.None -> ur.text2 }
    Text(rate, Modifier.clip(RoundedCornerShape(7.dp)).background(fill).padding(horizontal = 7.dp, vertical = 1.dp),
        style = MaterialTheme.typography.labelMedium.merge(Tabular), color = ink, fontWeight = FontWeight.Bold, maxLines = 1)
}

/** Figures two to a row: each one's name, its amount, and a line of detail under it. */
@Composable
private fun StatGrid(stats: List<Stat>, modifier: Modifier = Modifier) {
    Column(modifier, verticalArrangement = Arrangement.spacedBy(16.dp)) {
        stats.chunked(2).forEach { pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                pair.forEach { stat ->
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(stat.label, style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        Text(stat.value, style = MaterialTheme.typography.titleMedium.merge(Tabular), color = toneColor(stat.tone), fontWeight = FontWeight.SemiBold,
                            maxLines = 1, overflow = TextOverflow.Ellipsis)
                        stat.note?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 2, overflow = TextOverflow.Ellipsis) }
                    }
                }
                if (pair.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

/** What the account is made of: a ring by kind beside its key, then a few facts. */
@Composable
private fun MadeOf(view: PageView, choices: PageChoices, modifier: Modifier = Modifier) {
    ListCard(modifier) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
            if (view.kinds.size > 1) {
                val palette = reportPalette()
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(18.dp)) {
                    Donut(view.kinds, view.kinds[0].share, view.kinds[0].label, choices.kind, { choices.kind = it }, null, size = 128.dp, openLabel = stringResource(R.string.ibkr_by_kind))
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        view.kinds.forEach { part ->
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Box(Modifier.size(10.dp).clip(CircleShape).background(palette.color(part.color)))
                                Column(Modifier.padding(start = 8.dp).weight(1f)) {
                                    Text(part.label, style = MaterialTheme.typography.bodyMedium, color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    Text(part.valueText, style = MaterialTheme.typography.bodySmall.merge(Tabular), color = ur.muted, maxLines = 1)
                                }
                                Text(part.share, style = MaterialTheme.typography.bodyMedium.merge(Tabular), color = ur.text2, fontWeight = FontWeight.SemiBold)
                            }
                        }
                    }
                }
                Hairline()
            }
            StatGrid(view.holdStats)
            view.currencies?.let { Text(it, style = MaterialTheme.typography.bodySmall.merge(Tabular), color = ur.muted) }
        }
    }
}

/**
 * Each holding, then the cash and the interest still to be paid, under how many there are and their
 * order. A new order slides the rows to their places, more rows open the card smoothly, and the
 * share bars grow in the first time the list shows (not each time it scrolls back into view).
 */
@OptIn(ExperimentalSharedTransitionApi::class)
@Composable
private fun HoldingsList(view: PageView, choices: PageChoices, modifier: Modifier = Modifier) {
    val shown = if (choices.all) view.holdings else view.holdings.take(HoldingsPage.FIRST)
    val more = view.holdings.size - HoldingsPage.FIRST
    val seen = rememberSaveable { mutableStateOf(false) }
    val grow = !seen.value
    LaunchedEffect(Unit) { seen.value = true }
    ListCard(modifier.animateContentSize(spring(stiffness = Spring.StiffnessMediumLow))) {
        if (view.count > 0) Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 6.dp, top = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(countText(R.plurals.ibkr_holdings, view.count), Modifier.weight(1f), style = MaterialTheme.typography.titleSmall, color = ur.text)
            if (view.holdings.size > 1) SortMenu(choices.sort) { choices.sort = it } else Spacer(Modifier.height(40.dp))
        }
        LookaheadScope {
            val moves = Modifier.animateBounds(this, boundsTransform = BoundsTransform { _, _ -> spring(dampingRatio = .88f, stiffness = Spring.StiffnessMediumLow) })
            Column {
                shown.forEachIndexed { i, row ->
                    key(row.key) { HoldingRow(row, divider = i < shown.lastIndex || more > 0 || view.others.isNotEmpty(), grow, moves) }
                }
                if (more > 0) Column(moves) {
                    Text(if (choices.all) stringResource(R.string.ibkr_show_fewer) else countText(R.plurals.ibkr_show_more, more),
                        Modifier.fillMaxWidth().clickable(role = Role.Button) { choices.all = !choices.all }.padding(horizontal = 16.dp, vertical = 14.dp),
                        style = MaterialTheme.typography.bodyMedium, color = ur.accent, fontWeight = FontWeight.SemiBold)
                    if (view.others.isNotEmpty()) Hairline()
                }
                view.others.forEachIndexed { i, row -> key(row.key) { HoldingRow(row, divider = i < view.others.lastIndex, grow, moves) } }
            }
        }
    }
}

/** The order the holdings are listed in, as a small menu by their count. */
@Composable
private fun SortMenu(sort: String, onSort: (String) -> Unit) {
    val options = listOf("value" to stringResource(R.string.ibkr_value), "gain" to stringResource(R.string.ibkr_gain),
        "rate" to stringResource(R.string.ibkr_return), "name" to stringResource(R.string.ibkr_holding))
    val label = stringResource(R.string.sort_label)
    var open by remember { mutableStateOf(false) }
    Box {
        Row(Modifier.heightIn(min = 40.dp).clip(RoundedCornerShape(20.dp)).clickable(role = Role.Button, onClickLabel = label) { open = true }.padding(horizontal = 10.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.AutoMirrored.Outlined.Sort, label, Modifier.size(18.dp), tint = ur.accent)
            Spacer(Modifier.width(6.dp))
            Text(options.firstOrNull { it.first == sort }?.second.orEmpty(), style = MaterialTheme.typography.bodyMedium, color = ur.accent, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        UrMenu(open, { open = false }) {
            Text(label, Modifier.padding(start = 18.dp, end = 18.dp, top = 6.dp, bottom = 4.dp), style = MaterialTheme.typography.labelMedium, color = ur.muted)
            options.forEach { (id, name) ->
                UrMenuItem(text = { Text(name, fontWeight = if (id == sort) FontWeight.SemiBold else FontWeight.Normal) },
                    trailingIcon = if (id == sort) ({ Icon(Icons.Outlined.Check, null, tint = ur.accent) }) else null,
                    onClick = { onSort(id); open = false })
            }
        }
    }
}

/**
 * One thing the account holds: a tile in its kind's colour with its symbol, its name over its own
 * line, its value over its gain and return, and a bar of its share of the account. The investment
 * reports list their holdings and accounts in it too; with [onClick] the row opens something.
 */
@Composable
internal fun HoldingRow(row: app.urbudget.nativeapp.HoldingRowModel, divider: Boolean, grow: Boolean, modifier: Modifier, onClick: (() -> Unit)? = null) {
    val tint = reportPalette().color(row.color)
    // The share bar grows to its length the first time the list shows, and moves to a new length with new figures.
    val bar = remember { Animatable(if (grow) 0f else row.bar) }
    LaunchedEffect(row.bar) { bar.animateTo(row.bar, tween(if (grow) 760 else 380, delayMillis = if (grow) 120 else 0, easing = FastOutSlowInEasing)) }
    // In the card's own colour, so a row sliding to its new place covers the ones it passes.
    Column(modifier.background(if (LocalSheet.current) ur.surface2 else ur.surface)) {
    Row(Modifier.fillMaxWidth().then(if (onClick != null) Modifier.clickable(role = Role.Button, onClick = onClick) else Modifier)
        .padding(start = 14.dp, end = 16.dp, top = 12.dp, bottom = 12.dp)) {
        Box(Modifier.size(42.dp).clip(RoundedCornerShape(12.dp)).background(tint.copy(alpha = if (ur.dark) .2f else .14f)).padding(horizontal = 4.dp),
            contentAlignment = Alignment.Center) {
            if (row.mark != null) BasicText(Money.own(row.mark), style = TextStyle(color = tint, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center), maxLines = 1,
                autoSize = TextAutoSize.StepBased(minFontSize = 8.sp, maxFontSize = 12.sp, stepSize = .5.sp))
            else Icon(Icons.Outlined.Schedule, null, Modifier.size(20.dp), tint = tint)
        }
        Column(Modifier.padding(start = 12.dp).weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(Money.own(row.name), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, color = ur.text, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    // Three lines, for an account's row in the reports: how it's kept up to date, then two figures.
                    row.line?.let { Text(it, style = MaterialTheme.typography.bodySmall.merge(Tabular), color = ur.muted, maxLines = 3, overflow = TextOverflow.Ellipsis) }
                }
                Column(Modifier.padding(start = 10.dp), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(row.value, style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, color = ur.text, maxLines = 1)
                    if (row.gain != null || row.rate != null) Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        val tone = toned(row.sign)
                        row.gain?.let { Text(it, style = MaterialTheme.typography.bodySmall.merge(Tabular), color = if (tone == Tone.None) ur.muted else toneColor(tone), maxLines = 1) }
                        row.rate?.let { RatePill(it, tone) }
                    }
                }
                if (onClick != null) Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.padding(start = 4.dp).size(18.dp).align(Alignment.CenterVertically), tint = ur.muted)
            }
            row.share?.let { share ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.weight(1f).height(4.dp).clip(RoundedCornerShape(2.dp)).background(ur.zeroSoft)) {
                        Box(Modifier.fillMaxWidth(bar.value.coerceIn(0f, 1f)).fillMaxHeight().clip(RoundedCornerShape(2.dp)).background(tint))
                    }
                    Text(share, Modifier.padding(start = 10.dp).widthIn(min = 36.dp), style = MaterialTheme.typography.bodySmall.merge(Tabular), color = ur.muted, textAlign = TextAlign.End)
                }
            }
        }
    }
    if (divider) Hairline(start = 68.dp)
    }
}
