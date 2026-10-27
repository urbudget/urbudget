package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Check
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowLeft
import androidx.compose.material.icons.outlined.KeyboardArrowDown
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.AccountChip
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.AgeView
import app.urbudget.nativeapp.BalanceSheetView
import app.urbudget.nativeapp.BalancesView
import app.urbudget.nativeapp.CalendarView
import app.urbudget.nativeapp.CashFlowView
import app.urbudget.nativeapp.CategoryView
import app.urbudget.nativeapp.DrillLine
import app.urbudget.nativeapp.DrillTarget
import app.urbudget.nativeapp.FigureRow
import app.urbudget.nativeapp.FilterGroup
import app.urbudget.nativeapp.ForecastRow
import app.urbudget.nativeapp.ForecastView
import app.urbudget.nativeapp.HeatModel
import app.urbudget.nativeapp.IncomeExpenseView
import app.urbudget.nativeapp.IncomeView
import app.urbudget.nativeapp.MonthRange
import app.urbudget.nativeapp.NetWorthView
import app.urbudget.nativeapp.OverTimeView
import app.urbudget.nativeapp.PayeeRow
import app.urbudget.nativeapp.PayeeView
import app.urbudget.nativeapp.RangePreset
import app.urbudget.nativeapp.ReportView
import app.urbudget.nativeapp.RowStyle
import app.urbudget.nativeapp.Tile
import app.urbudget.nativeapp.Tone
import app.urbudget.nativeapp.customRange
import app.urbudget.nativeapp.filterIsOn
import app.urbudget.nativeapp.filterToggle
import app.urbudget.nativeapp.rangeFor
import java.time.YearMonth

/** Whether the figures on screen belong to an earlier request while a new one loads: they dim and stop taking taps. */
internal val LocalStale = compositionLocalOf { false }

/** What a report's rows and charts can ask of the screen. */
@Stable
internal class ReportActions(
    val drill: (DrillTarget) -> Unit,
    val setGroup: (String?) -> Unit,
    val setByGroup: (Boolean) -> Unit,
    val toggleChart: (String) -> Unit,
    val toggleExpand: (String) -> Unit,
    val showAllPayees: () -> Unit,
)

/** A tap that only counts while the figures are current. */
@Composable
private fun live(onClick: (() -> Unit)?): (() -> Unit)? = if (LocalStale.current) null else onClick

/**
 * One part of a white card on the grey page. A card's head, rows and foot are separate lazy items,
 * so a long list inside a card still scrolls lazily; the first part rounds the top, the last the bottom.
 */
@Composable
internal fun CardPart(top: Boolean, bottom: Boolean, modifier: Modifier = Modifier, live: Boolean = false, content: @Composable ColumnScope.() -> Unit) {
    val dim = !live && LocalStale.current
    val shape = RoundedCornerShape(topStart = if (top) 18.dp else 0.dp, topEnd = if (top) 18.dp else 0.dp,
        bottomStart = if (bottom) 18.dp else 0.dp, bottomEnd = if (bottom) 18.dp else 0.dp)
    Column(modifier.fillMaxWidth().padding(top = if (top) 12.dp else 0.dp).graphicsLayer { alpha = if (dim) 0.55f else 1f }
        .clip(shape).background(ur.surface), content = content)
}

@Composable
internal fun PanelTitle(title: String, aside: String? = null) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(title, color = ur.text, style = MaterialTheme.typography.titleMedium)
        aside?.let { Text(it, color = ur.muted, style = MaterialTheme.typography.bodySmall) }
    }
}

@Composable
internal fun HelpText(text: String, modifier: Modifier = Modifier) {
    Text(text, modifier, color = ur.muted, style = MaterialTheme.typography.bodySmall)
}

@Composable
private fun EmptyText(text: String) {
    Text(text, Modifier.fillMaxWidth().padding(vertical = 18.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
}

/** A card of one part: a title and its chart. */
internal fun LazyListScope.panel(key: String, content: @Composable ColumnScope.() -> Unit) {
    item(key = key, contentType = "panel") {
        CardPart(top = true, bottom = true) { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp), content = content) }
    }
}

/** A card of rows, each its own lazy item, between an optional head and foot. */
internal fun <T> LazyListScope.card(
    key: String, rows: List<T>, rowKey: (T) -> Any, rowType: String,
    head: (@Composable ColumnScope.() -> Unit)? = null, foot: (@Composable ColumnScope.() -> Unit)? = null,
    row: @Composable (T, Boolean) -> Unit,
) {
    val hasFoot = foot != null
    if (head != null) item(key = "$key:head", contentType = "$rowType:head") {
        CardPart(top = true, bottom = rows.isEmpty() && !hasFoot) { Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp), content = head) }
    }
    itemsIndexed(rows, key = { _, r -> "$key:${rowKey(r)}" }, contentType = { _, _ -> rowType }) { i, r ->
        CardPart(top = head == null && i == 0, bottom = !hasFoot && i == rows.lastIndex) { row(r, i < rows.lastIndex) }
    }
    if (foot != null) item(key = "$key:foot", contentType = "$rowType:foot") {
        CardPart(top = head == null && rows.isEmpty(), bottom = true) { Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp), content = foot) }
    }
}

/** The headline figures, two to a row. */
internal fun LazyListScope.tileRows(key: String, tiles: List<Tile>) {
    tiles.chunked(2).forEachIndexed { i, pair ->
        item(key = "$key:tiles:$i", contentType = "tiles") {
            val dim = LocalStale.current
            TilePair(pair, Modifier.padding(top = if (i == 0) 12.dp else 10.dp).graphicsLayer { alpha = if (dim) 0.55f else 1f })
        }
    }
}

/** The number of lazy items [tileRows] emits. */
internal fun tileRowCount(tiles: List<Tile>) = (tiles.size + 1) / 2

/** A row of a report's list: its title over a detail line, and its figure. */
@Composable
internal fun FigureRowView(row: FigureRow, onClick: (() -> Unit)?, divider: Boolean, expanded: Boolean = false) {
    val click = live(onClick)
    if (row.style == RowStyle.Section) {
        Text(row.title, Modifier.fillMaxWidth().background(ur.surface2).padding(horizontal = 16.dp, vertical = 8.dp),
            color = ur.muted, style = MaterialTheme.typography.labelMedium, fontWeight = FontWeight.Bold)
        return
    }
    val strong = row.style == RowStyle.Total || row.style == RowStyle.Net || row.style == RowStyle.Group
    Column {
        Row(Modifier.fillMaxWidth().then(if (click != null) Modifier.clickable(onClick = click) else Modifier).heightIn(min = 50.dp)
            .padding(start = if (row.style == RowStyle.Child) 34.dp else 16.dp, end = 16.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically) {
            if (row.expand != null) {
                Icon(if (expanded) Icons.Outlined.KeyboardArrowDown else Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.size(20.dp), tint = ur.text2)
                Spacer(Modifier.width(4.dp))
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(Money.own(row.title), color = if (row.style == RowStyle.Muted) ur.muted else ur.text, style = MaterialTheme.typography.bodyLarge,
                    fontWeight = if (strong) FontWeight.Bold else FontWeight.Medium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                row.subtitle?.let { Text(it, color = ur.muted, style = MaterialTheme.typography.bodySmall.merge(Tabular), maxLines = 3, overflow = TextOverflow.Ellipsis) }
            }
            if (row.value.isNotEmpty()) Text(row.value, Modifier.padding(start = 10.dp),
                color = when (row.tone) { Tone.Negative -> ur.negative; Tone.Positive -> ur.positive; Tone.None -> ur.text },
                style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = if (strong) FontWeight.Bold else FontWeight.SemiBold, maxLines = 1)
            if (onClick != null && row.expand == null) Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, null, Modifier.padding(start = 4.dp).size(18.dp), tint = ur.muted)
        }
        if (divider) Hairline(start = 16.dp)
    }
}

/** Lays out a report's body as lazy items. */
internal fun LazyListScope.reportBody(view: ReportView, actions: ReportActions, expanded: Set<String>, payeeQuery: String, onPayeeQuery: (String) -> Unit, payees: List<PayeeRow>) {
    when (view) {
        is CategoryView -> categoryBody(view, actions)
        is PayeeView -> payeeBody(view, actions, payeeQuery, onPayeeQuery, payees)
        is OverTimeView -> overTimeBody(view, actions)
        is CalendarView -> calendarBody(view, actions)
        is IncomeExpenseView -> incomeExpenseBody(view, actions, expanded)
        is IncomeView -> incomeBody(view, actions)
        is CashFlowView -> cashFlowBody(view)
        is NetWorthView -> netWorthBody(view)
        is BalanceSheetView -> balanceSheetBody(view)
        is BalancesView -> balancesBody(view, actions)
        is AgeView -> ageBody(view)
    }
}

/** Where the "All payees" card starts within a payee report's body, for the jump to it. */
internal fun payeeListOffset(view: PayeeView) = tileRowCount(view.tiles) + 1

private fun LazyListScope.categoryBody(v: CategoryView, a: ReportActions) {
    tileRows("categories", v.tiles)
    val open: (String) -> Unit = { key ->
        if (key != "__other") {
            if (v.group != null) a.drill(DrillTarget(v.rows.firstOrNull { it.key == key }?.label ?: key, "category", key)) else a.setGroup(key)
        }
    }
    card("categories:list", v.rows, { it.key }, "bar", head = {
        if (v.group != null) Row(verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.clip(RoundedCornerShape(8.dp)).clickable { a.setGroup(null) }.padding(end = 6.dp, top = 2.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, null, Modifier.size(20.dp), tint = ur.accent)
                Text(stringResource(R.string.reports_all_groups), color = ur.accent, style = MaterialTheme.typography.titleMedium)
            }
            Text("/ ${v.centerLabel}", color = ur.text, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
        } else PanelTitle(stringResource(R.string.reports_spending_by_group))
        if (v.rows.isNotEmpty()) Donut(v.slices, v.centerValue, v.centerLabel, if (LocalStale.current) null else open, Modifier.align(Alignment.CenterHorizontally))
        else EmptyText(stringResource(R.string.reports_no_spending))
    }, foot = { HelpText(v.help) }) { row, divider ->
        BarRowView(row, 0, live { open(row.key) })
        if (divider) Hairline(start = 16.dp)
    }
    v.changes?.let { c ->
        card("categories:changes", c.rows, { it.key }, "diverging", head = {
            PanelTitle(c.title, c.aside)
            if (c.rows.isEmpty()) EmptyText(stringResource(R.string.reports_matched_average))
        }, foot = if (c.rows.isEmpty()) null else ({ ChartLegend(c.legend) })) { row, divider ->
            DivergingRowView(row)
            if (divider) Hairline(start = 16.dp)
        }
    }
}

private fun LazyListScope.payeeBody(v: PayeeView, a: ReportActions, query: String, onQuery: (String) -> Unit, payees: List<PayeeRow>) {
    tileRows("payees", v.tiles)
    panel("payees:map") {
        PanelTitle(v.title, v.aside)
        if (v.map.isNotEmpty()) {
            val stale = LocalStale.current
            Treemap(v.map, { if (!stale) a.drill(DrillTarget(it.label, "payee", it.key)) })
        } else EmptyText(stringResource(R.string.reports_no_payees))
        HelpText(v.help)
        v.more?.let { more ->
            Text("$more ↓", Modifier.clip(RoundedCornerShape(8.dp)).clickable(onClick = a.showAllPayees).padding(vertical = 4.dp),
                color = ur.accent, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
        }
    }
    card("payees:all", payees, { it.key }, "payee", head = {
        PanelTitle(stringResource(R.string.reports_all_payees))
        SearchField(query, onQuery, stringResource(R.string.reports_find_payee))
        if (payees.isEmpty()) EmptyText(stringResource(R.string.reports_no_payees_match))
    }) { p, divider ->
        val click = live { a.drill(DrillTarget(p.label, "payee", p.name)) }
        Column {
            Row(Modifier.fillMaxWidth().then(if (click != null) Modifier.clickable(onClick = click) else Modifier).heightIn(min = 52.dp)
                .padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(p.label, color = ur.text, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(p.detail, color = ur.muted, style = MaterialTheme.typography.bodySmall.merge(Tabular), maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
                Text(p.amountText, Modifier.padding(start = 10.dp), color = ur.text, style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1)
            }
            if (divider) Hairline(start = 16.dp)
        }
    }
}

private fun LazyListScope.overTimeBody(v: OverTimeView, a: ReportActions) {
    tileRows("trends", v.tiles)
    card("trends:months", v.rows, { it.key }, "month", head = {
        PanelTitle(stringResource(R.string.report_chart_spending_by_month))
        Segmented(listOf("groups" to stringResource(R.string.reports_by_group), "total" to stringResource(R.string.report_total)), if (v.byGroup) "groups" else "total", { a.setByGroup(it == "groups") })
        ColumnChart(v.chart)
        if (v.legend.isNotEmpty()) ChartLegend(v.legend)
    }) { row, divider -> FigureRowView(row, null, divider) }
}

@Composable
private fun HeatPanel(heat: HeatModel, drill: (DrillTarget) -> Unit) {
    var day by remember(heat) { mutableIntStateOf(-1) }
    val stale = LocalStale.current
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(if (day in heat.days.indices) stringResource(if (heat.spent[day]) R.string.reports_heat_spent else R.string.reports_heat_nothing, heat.titles[day], heat.amounts[day])
            else stringResource(R.string.reports_heat_help), color = if (day >= 0) ur.text else ur.muted, style = MaterialTheme.typography.bodySmall)
        HeatCalendar(heat, day, { i ->
            day = i
            if (!stale && heat.spent[i]) drill(DrillTarget(heat.titles[i], "day", heat.days[i], heat.days[i], heat.days[i]))
        })
        HeatLegend()
    }
}

private fun LazyListScope.calendarBody(v: CalendarView, a: ReportActions) {
    tileRows("calendar", v.tiles)
    panel("calendar:heat") {
        PanelTitle(stringResource(R.string.reports_spending_by_day))
        if (v.heat != null) HeatPanel(v.heat, a.drill) else EmptyText(stringResource(R.string.reports_not_started))
    }
    panel("calendar:weekdays") {
        PanelTitle(stringResource(R.string.reports_by_weekday), stringResource(R.string.reports_average_spent))
        ColumnChart(v.weekdays, height = 200.dp)
    }
    card("calendar:biggest", v.biggest, { it.key }, "day", head = {
        PanelTitle(stringResource(R.string.reports_biggest_days))
        if (v.biggest.isEmpty()) EmptyText(stringResource(R.string.reports_no_spending))
    }) { row, divider -> FigureRowView(row, row.drill?.let { t -> { a.drill(t) } }, divider) }
}

private fun LazyListScope.incomeExpenseBody(v: IncomeExpenseView, a: ReportActions, expanded: Set<String>) {
    tileRows("incomeExpense", v.tiles)
    item(key = "incomeExpense:help", contentType = "help") { HelpText(v.help, Modifier.padding(top = 12.dp, start = 4.dp, end = 4.dp)) }
    panel("incomeExpense:chart") {
        PanelTitle(stringResource(R.string.reports_income_spending_by_month))
        ColumnChart(v.chart)
        ChartLegend(v.legend)
    }
    panel("incomeExpense:rate") {
        PanelTitle(stringResource(R.string.report_budget_surplus_rate), stringResource(R.string.reports_surplus_share))
        LinesChart(v.rate, height = 200.dp)
    }
    panel("incomeExpense:cumulative") {
        PanelTitle(stringResource(R.string.report_cumulative_surplus), stringResource(R.string.reports_surplus_added_up))
        LinesChart(v.cumulative, height = 200.dp)
    }
    val rows = v.table.flatMap { r -> if (r.expand != null && r.expand in expanded) listOf(r) + v.children[r.expand].orEmpty() else listOf(r) }
    card("incomeExpense:table", rows, { it.key }, "figure", head = { PanelTitle(stringResource(R.string.reports_income_and_spending), stringResource(R.string.reports_select_group)) }) { row, divider ->
        FigureRowView(row, row.expand?.let { g -> { a.toggleExpand(g) } }, divider, expanded = row.expand in expanded)
    }
    card("incomeExpense:months", v.months, { it.key }, "figure", head = { PanelTitle(stringResource(R.string.reports_by_month), stringResource(R.string.reports_surplus_each_month)) }) { row, divider -> FigureRowView(row, null, divider) }
}

private fun LazyListScope.incomeBody(v: IncomeView, a: ReportActions) {
    tileRows("income", v.tiles)
    card("income:bars", v.bars, { it.key }, "bar", head = {
        PanelTitle(stringResource(R.string.reports_income_from))
        if (v.bars.isNotEmpty()) Donut(v.slices, v.centerValue, stringResource(R.string.reports_positive_sources), null, Modifier.align(Alignment.CenterHorizontally))
        else EmptyText(stringResource(R.string.reports_no_income))
    }, foot = v.chartHelp?.let { help -> { HelpText(help) } }) { row, divider ->
        BarRowView(row, 0, null)
        if (divider) Hairline(start = 16.dp)
    }
    v.monthly?.let { chart ->
        panel("income:monthly") {
            PanelTitle(stringResource(R.string.reports_income_by_month))
            ColumnChart(chart)
            ChartLegend(v.legend)
        }
    }
    card("income:sources", v.sources, { it.key }, "figure", head = { PanelTitle(stringResource(R.string.reports_sources)) }, foot = v.sourcesHelp?.let { help -> { HelpText(help) } }) { row, divider ->
        FigureRowView(row, row.drill?.let { t -> { a.drill(t) } }, divider)
    }
}

private fun LazyListScope.cashFlowBody(v: CashFlowView) {
    tileRows("cashflow", v.tiles)
    panel("cashflow:chart") {
        PanelTitle(stringResource(R.string.reports_money_in_out_by_month))
        ColumnChart(v.chart)
        ChartLegend(v.legend)
        HelpText(stringResource(R.string.reports_cash_flow_help))
    }
    panel("cashflow:running") {
        PanelTitle(stringResource(R.string.report_running_total), stringResource(R.string.reports_net_flow_added_up))
        LinesChart(v.running, height = 200.dp)
    }
    card("cashflow:months", v.months, { it.key }, "figure", head = { PanelTitle(stringResource(R.string.reports_by_month), stringResource(R.string.reports_net_flow_each_month)) }) { row, divider -> FigureRowView(row, null, divider) }
}

private fun LazyListScope.netWorthBody(v: NetWorthView) {
    tileRows("networth", v.tiles)
    panel("networth:chart") {
        PanelTitle(stringResource(R.string.reports_assets_debts_net_worth))
        ColumnChart(v.chart)
        ChartLegend(v.legend)
    }
    panel("networth:change") {
        PanelTitle(stringResource(R.string.reports_change_each_month))
        ColumnChart(v.change, height = 200.dp)
        ChartLegend(v.changeLegend)
    }
    card("networth:accounts", v.accounts, { it.key }, "figure", head = { PanelTitle(stringResource(R.string.reports_by_account), v.dates) }) { row, divider -> FigureRowView(row, null, divider) }
}

private fun LazyListScope.balanceSheetBody(v: BalanceSheetView) {
    tileRows("balance", v.tiles)
    card("balance:own", v.own, { it.key }, "bar", head = {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { PanelTitle(stringResource(R.string.reports_what_you_own)) }
            Text(v.ownTotal, color = ur.text, style = MaterialTheme.typography.titleMedium.merge(Tabular))
        }
        if (v.own.isEmpty()) EmptyText(stringResource(R.string.reports_no_assets))
    }) { row, divider ->
        BarRowView(row, 0, null)
        if (divider) Hairline(start = 16.dp)
    }
    card("balance:owe", v.owe, { it.key }, "bar", head = {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.weight(1f)) { PanelTitle(stringResource(R.string.reports_what_you_owe)) }
            Text(v.oweTotal, color = ur.text, style = MaterialTheme.typography.titleMedium.merge(Tabular))
        }
        if (v.owe.isEmpty()) EmptyText(stringResource(R.string.reports_nothing_owed))
    }) { row, divider ->
        BarRowView(row, 1, null)
        if (divider) Hairline(start = 16.dp)
    }
    card("balance:statement", v.statement, { it.key }, "figure", head = { PanelTitle(stringResource(R.string.reports_statement), v.statementAside); HelpText(v.dates) },
        foot = { HelpText(v.help) }) { row, divider -> FigureRowView(row, null, divider && row.style != RowStyle.Section) }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AccountChips(chips: List<AccountChip>, toggle: (String) -> Unit) {
    val palette = reportPalette()
    val stale = LocalStale.current
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        chips.forEach { chip ->
            val on = chip.slot != null
            val shape = RoundedCornerShape(17.dp)
            Row(Modifier.heightIn(min = 34.dp).clip(shape).background(if (on) ur.accentSoft else ur.surface2)
                .border(1.dp, if (on) ur.accent.copy(alpha = .35f) else ur.border, shape)
                .clickable(enabled = chip.enabled && !stale) { toggle(chip.id) }.padding(horizontal = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(10.dp).clip(RoundedCornerShape(2.dp)).background(if (on) palette.color(chip.slot) else Color.Transparent)
                    .border(1.dp, if (on) Color.Transparent else ur.borderStrong, RoundedCornerShape(2.dp)))
                Spacer(Modifier.width(7.dp))
                Text(chip.name, color = if (chip.enabled) ur.text2 else ur.muted, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

private fun LazyListScope.balancesBody(v: BalancesView, a: ReportActions) {
    tileRows("balances", v.tiles)
    panel("balances:chart") {
        PanelTitle(stringResource(R.string.reports_balances_month_end))
        AccountChips(v.chips, a.toggleChart)
        if (v.chart != null) {
            LinesChart(v.chart, height = 260.dp)
            ChartLegend(v.legend)
        } else EmptyText(stringResource(R.string.reports_choose_accounts))
    }
    card("balances:accounts", v.rows, { it.key }, "figure", head = { PanelTitle(stringResource(R.string.reports_all_accounts)) }, foot = { HelpText(stringResource(R.string.reports_balances_help)) }) { row, divider ->
        FigureRowView(row, null, divider)
    }
}

private fun LazyListScope.ageBody(v: AgeView) {
    tileRows("age", v.tiles)
    card("age:months", v.rows, { it.key }, "figure", head = {
        PanelTitle(stringResource(R.string.reports_cash_age_month_end))
        LinesChart(v.chart)
        HelpText(stringResource(R.string.reports_cash_age_help))
    }) { row, divider -> FigureRowView(row, null, divider) }
}

@Composable
private fun ForecastRowView(row: ForecastRow, divider: Boolean) {
    Column {
        Row(Modifier.fillMaxWidth().heightIn(min = 54.dp).padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(Money.own(row.title), color = ur.text, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(row.detail, color = ur.muted, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Column(Modifier.padding(start = 10.dp), horizontalAlignment = Alignment.End) {
                Text(row.amount, color = if (row.amountPositive) ur.positive else ur.text, style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1)
                Text(row.balance, color = if (row.balanceNegative) ur.negative else ur.muted, style = MaterialTheme.typography.bodySmall.merge(Tabular), maxLines = 1)
            }
        }
        if (divider) Hairline(start = 16.dp)
    }
}

/** The forecast's body: what's known ahead, the line with everyday spending, and what's coming up. */
internal fun LazyListScope.forecastBody(
    v: ForecastView?, ahead: Int, setAhead: (Int) -> Unit, error: String?, retry: () -> Unit, editEveryday: () -> Unit, useSuggested: () -> Unit,
) {
    if (v != null) tileRows("forecast", v.tiles)
    panel("forecast:chart") {
        PanelTitle(stringResource(R.string.reports_balance_ahead))
        Segmented(listOf(3, 6, 12).map { it.toString() to countText(R.plurals.report_months_option, it) }, ahead.toString(), { setAhead(it.toInt()) })
        when {
            v != null -> {
                LinesChart(v.chart, height = 260.dp)
                ChartLegend(v.legend)
                Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(fieldFill()).clickable(onClickLabel = stringResource(R.string.reports_change_everyday), onClick = editEveryday)
                    .padding(horizontal = 14.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.reports_everyday_a_month), color = ur.muted, style = MaterialTheme.typography.bodySmall)
                        Text(v.everydayText, color = ur.text, style = MaterialTheme.typography.titleMedium.merge(Tabular))
                    }
                    Text(stringResource(R.string.common_change), color = ur.accent, fontWeight = FontWeight.SemiBold)
                }
                v.suggestion?.let { TextButton(useSuggested) { Text(it, color = ur.accent, fontWeight = FontWeight.SemiBold) } }
                HelpText(v.help)
            }
            error != null -> {
                FormError(error)
                TextButton(retry) { Text(stringResource(R.string.reports_retry_forecast), color = ur.accent, fontWeight = FontWeight.Bold) }
            }
            else -> LinearProgressIndicator(Modifier.fillMaxWidth().padding(vertical = 24.dp), color = ur.accent)
        }
    }
    if (v != null) card("forecast:events", v.rows, { it.key }, "event", head = {
        PanelTitle(stringResource(R.string.reports_coming_up), v.count)
        if (v.rows.isEmpty()) {
            EmptyText(stringResource(R.string.reports_nothing_ahead))
            HelpText(stringResource(R.string.reports_schedule_more))
        }
    }, foot = { HelpText(stringResource(R.string.reports_known_balances)) }) { row, divider -> ForecastRowView(row, divider) }
}

// ---------- Sheets ----------

/** The months a report covers: presets, or a custom from and to. */
@Composable
internal fun RangeSheet(range: MonthRange, earliest: String, onPick: (MonthRange) -> Unit, onDismiss: () -> Unit) {
    val now = remember { YearMonth.now() }
    var from by remember { mutableStateOf(range.from.ifEmpty { earliest }) }
    var to by remember { mutableStateOf(range.to.ifEmpty { now.toString() }) }
    FormDialog(stringResource(R.string.reports_months), onDismiss, confirm = stringResource(R.string.common_apply), onConfirm = { onPick(customRange(from, to)) }) {
        ListCard {
            val presets = RangePreset.entries.filter { it != RangePreset.Custom }
            presets.forEachIndexed { i, preset ->
                ListRow(stringResource(preset.label), onClick = { onPick(rangeFor(preset, now, earliest)) }, chevron = false, divider = i < presets.lastIndex,
                    trailing = if (range.preset == preset) ({ Icon(Icons.Outlined.Check, null, tint = ur.accent) }) else null)
            }
        }
        Text(stringResource(R.string.range_custom), color = ur.text, style = MaterialTheme.typography.titleMedium)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Box(Modifier.weight(1f)) { MonthField(from, { from = it }, stringResource(R.string.date_from)) }
            Box(Modifier.weight(1f)) { MonthField(to, { to = it }, stringResource(R.string.move_to_label)) }
        }
        HelpText(stringResource(R.string.reports_range_help))
    }
}

/** A grouped checklist: an empty selection means all, the empty id alone means none. */
@Composable
internal fun MultiFilterSheet(title: String, groups: List<FilterGroup>, selected: Set<String>, onChange: (Set<String>) -> Unit, onDismiss: () -> Unit) {
    var query by remember { mutableStateOf("") }
    val shown = remember(groups, query) {
        val q = query.trim().lowercase()
        if (q.isEmpty()) groups else groups.map { g -> g.copy(items = g.items.filter { it.label.lowercase().contains(q) }) }.filter { it.items.isNotEmpty() }
    }
    val height = screenHeight()
    UrSheet(onDismiss) {
        SheetHeader(title, onDismiss, null, null)
        Row(Modifier.fillMaxWidth().padding(horizontal = Gutter), verticalAlignment = Alignment.CenterVertically) {
            SearchField(query, { query = it }, stringResource(R.string.search_named, title.lowercase()), Modifier.weight(1f))
            TextButton({ onChange(emptySet()) }) { Text(stringResource(R.string.view_all), color = ur.accent, fontWeight = FontWeight.SemiBold) }
            TextButton({ onChange(setOf("")) }) { Text(stringResource(R.string.filter_none), color = ur.accent, fontWeight = FontWeight.SemiBold) }
        }
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = height * .62f), contentPadding = PaddingValues(start = 10.dp, end = 16.dp, top = 6.dp, bottom = 28.dp)) {
            shown.forEach { g ->
                item(key = "g:${g.name}", contentType = "group") {
                    val ids = g.items.map { it.id }
                    val every = ids.all { filterIsOn(selected, it) }
                    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { onChange(filterToggle(groups, selected, ids, !every)) }.padding(vertical = 2.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(every, { onChange(filterToggle(groups, selected, ids, it)) }, colors = CheckboxDefaults.colors(checkedColor = ur.accent))
                        Text(g.name, color = ur.text, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Bold)
                    }
                }
                items(g.items, key = { "i:${g.name}:${it.id}" }, contentType = { "item" }) { item ->
                    Box(Modifier.padding(start = 24.dp)) { CheckRow(item.label, filterIsOn(selected, item.id), { onChange(filterToggle(groups, selected, listOf(item.id), it)) }) }
                }
            }
            if (shown.isEmpty()) item(key = "none") { Text(stringResource(R.string.nothing_matches), Modifier.padding(16.dp), color = ur.muted) }
        }
    }
}

/** A transaction behind a figure: payee, date · account · category, and its amount. */
@Composable
internal fun DrillLineRow(line: DrillLine, onClick: () -> Unit) {
    Column {
        Row(Modifier.fillMaxWidth().clickable(onClick = onClick).heightIn(min = 56.dp).padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(Money.own(line.payee), color = ur.text, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(line.detail, color = ur.muted, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            Text(line.amount, Modifier.padding(start = 10.dp), color = if (line.positive) ur.positive else ur.text,
                style = MaterialTheme.typography.bodyLarge.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        Hairline()
    }
}
