package app.urbudget.nativeapp

import androidx.compose.runtime.Immutable
import org.json.JSONObject
import java.math.BigDecimal
import java.math.RoundingMode
import java.time.DayOfWeek
import java.time.LocalDate
import java.time.YearMonth
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.util.Locale
import java.util.TreeMap
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

/*
 * The reports' figures, ported from the desktop's React reports (the desktop reports in desktop/src/reports,
 * PhoneReports.tsx, charts/scale.ts). The shared engine (crates/urbudget-core/src/reports.rs)
 * does the accounting; this file shapes its JSON into immutable models with every label and
 * amount already formatted, so lists and charts only lay out and draw.
 */

/** Chart colours: 0 to 7 are the report series (series-1..8 in reports.css); the rest are roles. */
object ReportColors {
    const val OTHER = -1
    const val INK = -2
    const val POSITIVE = -3
    const val NEGATIVE = -4
    const val DIVERGING_POSITIVE = 0
    const val DIVERGING_NEGATIVE = 7
}

/** The report-only bucket for spending without a category, as the engine names it. */
const val UNCATEGORIZED_BUCKET = "__uncategorized"
/** The spending group of that bucket, as reports key it; [ReportBudget.groupLabel] says it in the interface's language. */
const val NEEDS_CATEGORY = "Needs a category"

// ---------- Text ----------
// Month and day names follow the interface's language (Money.pattern); the words come from Words.

/** "2026-10" as "October 2026", or "Oct 2026" when short. */
fun reportMonthName(month: String, short: Boolean = false): String =
    runCatching { YearMonth.parse(month).format(Money.pattern(if (short) "MMM yyyy" else "MMMM yyyy")) }.getOrDefault(month)

/** "2026-10" (or a day in it) as "Oct". */
fun monthAbbrev(month: String): String = runCatching { YearMonth.parse(month.take(7)).format(Money.pattern("MMM")) }.getOrDefault(month)

fun shiftMonth(month: String, delta: Long): String = runCatching { YearMonth.parse(month).plusMonths(delta).toString() }.getOrDefault(month)
fun firstDay(month: String) = "$month-01"
fun lastDay(month: String): String = runCatching { YearMonth.parse(month).atEndOfMonth().toString() }.getOrDefault("$month-28")

/** JavaScript's weekday numbering (0 is Sunday), which the desktop's calendar uses. */
fun jsWeekday(date: LocalDate): Int = date.dayOfWeek.value % 7
fun weekdayName(day: Int, short: Boolean = false): String = Money.weekday(DayOfWeek.of(if (day % 7 == 0) 7 else day % 7), short)

/** The first day of the week where the phone is set, in JavaScript's numbering. */
fun localeWeekStart(locale: Locale = Locale.getDefault()): Int = java.time.temporal.WeekFields.of(locale).firstDayOfWeek.value % 7

/** JavaScript's `toFixed`: half away from zero on the exact value, the sign kept for small negatives. */
fun fixed(value: Double, digits: Int): String {
    if (value.isNaN()) return "NaN"
    val text = BigDecimal(abs(value)).setScale(digits, RoundingMode.HALF_UP).toPlainString()
    return if (value < 0) "-$text" else text
}

/** A number as JavaScript prints it in a template string: 12 not 12.0. */
fun jsNumber(value: Double): String = when {
    value == floor(value) && abs(value) < 1e15 -> (value + 0.0).toLong().toString()
    else -> value.toString()
}

/** "12.5%" of a positive total, else a dash. */
fun share(n: Long, total: Long): String = if (total > 0) Money.ltr("${fixed(n.toDouble() / total * 100, 1)}%") else "—"
fun daysText(v: Long?): String = if (v == null) "—" else Words.count(R.plurals.report_days, v)
/** A percentage with its sign either way, as trends and changes show it. */
private fun signedPercent(value: Double, digits: Int): String = Money.ltr("${if (value >= 0) "+" else ""}${fixed(value, digits)}%")
private fun roundDiv(total: Long, n: Int): Long = Math.round(total.toDouble() / n)

/** Axis label for an amount in cents: 0, 950, 1.5K, 12K, 1.2M. */
fun compactMoney(cents: Double, decimal: String = Money.decimalMark()): String {
    val v = cents / 100 + 0.0
    val size = abs(v)
    fun part(n: Double, digits: Int) = fixed(n, digits).replace(Regex("\\.0+$"), "").replace(".", decimal)
    // K and M stay as they are in Arabic too: they are part of how the budget writes numbers.
    return Money.ltr(when {
        size >= 1e6 -> "${part(v / 1e6, if (size >= 1e7) 0 else 1)}M"
        size >= 1e3 -> "${part(v / 1e3, if (size >= 1e4) 0 else 1)}K"
        else -> part(v, 0)
    })
}

// ---------- Scales ----------

/** Round tick values (in the data's units) covering [min, max], always including 0. */
fun niceTicks(min: Double, max: Double, count: Int = 5): List<Double> {
    val lo = min(0.0, min)
    val hi = max(0.0, max)
    if (lo == hi || lo.isNaN() || hi.isNaN()) return listOf(0.0, 1.0)
    val raw = (hi - lo) / count
    val power = 10.0.pow(floor(log10(raw)))
    val step = listOf(1.0, 2.0, 2.5, 5.0, 10.0).map { it * power }.firstOrNull { raw <= it } ?: (10 * power)
    val ticks = ArrayList<Double>()
    var v = floor(lo / step) * step
    while (v <= hi + step / 2 && ticks.size < 64) {
        ticks.add(if (abs(v) < 1e12) Math.round(v * 1e6) / 1e6 else v)
        v += step
    }
    if (ticks.last() < hi) ticks.add(ticks.last() + step)
    return ticks
}

/** Five steps of magnitude: each positive value gets 1 to 5 by its quintile among the others. */
class HeatScale internal constructor(private val cuts: LongArray?) {
    fun level(value: Long): Int = if (value <= 0 || cuts == null) 0 else 1 + cuts.count { value > it }
}

fun heatLevels(values: Collection<Long>): HeatScale {
    val sorted = values.filter { it > 0 }.sorted()
    if (sorted.isEmpty()) return HeatScale(null)
    fun at(q: Double) = sorted[min(sorted.size - 1, floor(q * sorted.size).toInt())]
    return HeatScale(longArrayOf(at(0.2), at(0.4), at(0.6), at(0.8)))
}

/** A tile placed by [squarify]. */
data class TileBox<T>(val item: T, val x: Double, val y: Double, val w: Double, val h: Double)

/** Squarified treemap: areas in proportion to values, rows kept as square as possible. Largest first. */
fun <T> squarify(items: List<T>, value: (T) -> Double, width: Double, height: Double): List<TileBox<T>> {
    val total = items.sumOf(value)
    if (total <= 0 || width <= 0 || height <= 0) return emptyList()
    val scale = width * height / total
    val out = ArrayList<TileBox<T>>(items.size)
    var rx = 0.0; var ry = 0.0; var rw = width; var rh = height
    fun area(item: T) = value(item) * scale
    fun worst(row: List<T>, side: Double): Double {
        val s = row.sumOf { area(it) }
        val most = row.maxOf { area(it) }
        val least = row.minOf { area(it) }
        return max(side * side * most / (s * s), s * s / (side * side * least))
    }
    fun place(row: List<T>) {
        val s = row.sumOf { area(it) }
        if (rw >= rh) {
            val w = s / rh
            var y = ry
            for (item in row) { val h = area(item) / w; out.add(TileBox(item, rx, y, w, h)); y += h }
            rx += w; rw -= w
        } else {
            val h = s / rw
            var x = rx
            for (item in row) { val w = area(item) / h; out.add(TileBox(item, x, ry, w, h)); x += w }
            ry += h; rh -= h
        }
    }
    var row = ArrayList<T>()
    for (item in items) {
        val side = min(rw, rh)
        if (row.isEmpty() || worst(row + item, side) <= worst(row, side)) row.add(item)
        else { place(row); row = arrayListOf(item) }
    }
    if (row.isNotEmpty()) place(row)
    return out
}

// ---------- The engine's report ----------

@Immutable
data class ReportMonth(
    val month: String, val income: Long, val spending: Long, val netIncome: Long, val assets: Long, val debt: Long, val netWorth: Long,
    val inflow: Long, val outflow: Long, val categories: Map<String, Long>, val incomeSources: Map<String, Long>, val accounts: Map<String, Long>,
)

@Immutable
data class PayeeSpend(val name: String, val amount: Long, val count: Long)

@Immutable
data class ReportData(
    val months: List<ReportMonth>,
    val categories: Map<String, Long>,
    val incomeSources: Map<String, Long>,
    /** Spending by payee, most first. */
    val payees: List<PayeeSpend>,
    val days: Map<String, Long>,
    val income: Long,
    val spending: Long,
    val transactionCount: Long?,
    val ageOfMoney: Long?,
    val openingAccounts: Map<String, Long>?,
    val ageOfMoneyByMonth: Map<String, Long?>,
)

private fun JSONObject.str(key: String) = if (isNull(key)) "" else optString(key, "")
private fun JSONObject.list(key: String): List<JSONObject> = optJSONArray(key)?.let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) } }.orEmpty()

/** A JSON object of amounts in key order, as the engine's BTreeMap writes it. */
private fun JSONObject?.amounts(): Map<String, Long> {
    if (this == null) return emptyMap()
    val out = TreeMap<String, Long>()
    val keys = keys()
    while (keys.hasNext()) { val key = keys.next(); out[key] = optLong(key) }
    return out
}

fun parseReport(json: JSONObject): ReportData {
    val months = json.list("months").map { m ->
        ReportMonth(m.str("month"), m.optLong("income"), m.optLong("spending"), m.optLong("netIncome"), m.optLong("assets"), m.optLong("debt"),
            m.optLong("netWorth"), m.optLong("inflow"), m.optLong("outflow"), m.optJSONObject("categories").amounts(),
            m.optJSONObject("incomeSources").amounts(), m.optJSONObject("accounts").amounts())
    }
    val payees = json.optJSONObject("payees")?.let { o ->
        val names = TreeMap<String, JSONObject>()
        val keys = o.keys()
        while (keys.hasNext()) { val key = keys.next(); o.optJSONObject(key)?.let { names[key] = it } }
        names.map { (name, p) -> PayeeSpend(name, p.optLong("amount"), p.optLong("count")) }.sortedByDescending { it.amount }
    }.orEmpty()
    val ages = TreeMap<String, Long?>()
    json.optJSONObject("ageOfMoneyByMonth")?.let { o -> val keys = o.keys(); while (keys.hasNext()) { val k = keys.next(); ages[k] = if (o.isNull(k)) null else o.optLong(k) } }
    return ReportData(months, json.optJSONObject("categories").amounts(), json.optJSONObject("incomeSources").amounts(), payees,
        json.optJSONObject("days").amounts(), json.optLong("income"), json.optLong("spending"),
        if (json.has("transactionCount")) json.optLong("transactionCount") else null,
        if (json.isNull("ageOfMoney")) null else json.optLong("ageOfMoney"),
        json.optJSONObject("openingAccounts")?.amounts(), ages)
}

// ---------- The budget, as reports read it ----------

private val Sections = mapOf(
    "checking" to "Cash and bank", "savings" to "Cash and bank", "cash" to "Cash and bank",
    "investment" to "Investments", "retirement" to "Investments", "property" to "Property", "vehicle" to "Vehicles",
    "receivable" to "Receivables", "otherAsset" to "Other assets", "creditCard" to "Credit cards", "lineOfCredit" to "Lines of credit",
    "mortgage" to "Mortgages", "autoLoan" to "Loans", "studentLoan" to "Loans", "personalLoan" to "Loans",
    "medicalDebt" to "Other debts", "otherDebt" to "Other debts",
)
private val Liabilities = setOf("creditCard", "lineOfCredit", "mortgage", "autoLoan", "studentLoan", "personalLoan", "medicalDebt", "otherDebt")
fun isLiability(type: String) = type in Liabilities
/** The balance sheet section an account type falls in, by its English key; [sectionLabel] names it. */
fun sectionOf(type: String) = Sections[type] ?: if (isLiability(type)) "Other debts" else "Other assets"

/** A balance sheet section's name in the interface's language. */
fun sectionLabel(section: String): String = when (section) {
    "Cash and bank" -> Words[R.string.section_cash_bank]
    "Investments" -> Words[R.string.section_investments]
    "Property" -> Words[R.string.section_property]
    "Vehicles" -> Words[R.string.section_vehicles]
    "Receivables" -> Words[R.string.section_receivables]
    "Other assets" -> Words[R.string.section_other_assets]
    "Credit cards" -> Words[R.string.section_credit_cards]
    "Lines of credit" -> Words[R.string.section_lines_of_credit]
    "Mortgages" -> Words[R.string.section_mortgages]
    "Loans" -> Words[R.string.section_loans]
    "Other debts" -> Words[R.string.section_other_debts]
    "Accounts in credit" -> Words[R.string.section_in_credit]
    "Overdrawn accounts" -> Words[R.string.section_overdrawn]
    else -> section
}

/** An account's type: the one chosen, else the imported one, else a guess from its kind and balance. */
fun accountType(chosen: String, imported: String, kind: String, balance: Long): String = when {
    chosen in Sections -> chosen
    imported == "otherLiability" -> "otherDebt"
    imported in Sections -> imported
    kind == "credit" -> "creditCard"
    kind == "cash" -> "checking"
    balance < 0 -> "otherDebt"
    else -> "otherAsset"
}

@Immutable
data class ReportAccount(
    val id: String, val name: String, val kind: String, val archived: Boolean, val onBudget: Boolean, val inNetWorth: Boolean,
    val chosenType: String, val importedType: String, val openingBalance: Long,
) {
    fun type(balance: Long = openingBalance) = accountType(chosenType, importedType, kind, balance)
}

@Immutable
data class ReportCategory(val id: String, val name: String, val group: String, val payment: Boolean)

/** What reports need from a budget snapshot: names, groups in order, accounts and the first month with data. */
@Immutable
class ReportBudget(
    val currency: String,
    val accounts: List<ReportAccount>,
    /** The budget's own categories, in their saved order (no report-only bucket). */
    val categories: List<ReportCategory>,
    /** Group and name of every category, the "Needs a category" bucket included. */
    val groupOf: Map<String, String>,
    val nameOf: Map<String, String>,
    /** Groups that can hold spending, in budget order, "Needs a category" last. */
    val spendingGroups: List<String>,
    val earliestMonth: String,
) {
    val accountsById: Map<String, ReportAccount> = accounts.associateBy { it.id }
    /** The spending-without-a-category group's name, in the language these reports were built in. */
    val needsCategory: String = Words[R.string.category_needed]
    fun money(cents: Long): String = Money.format(cents, currency)
    fun money(cents: Double): String = Money.format(Math.round(cents), currency)
    fun signed(cents: Long) = Money.signed(cents, currency)
    /** A group as reports show it: the person's own name, or the spending that still needs a category. */
    fun groupLabel(group: String) = if (group == NEEDS_CATEGORY) needsCategory else group
    /** A group's colour: the first seven groups in budget order have their own. */
    fun colorOf(group: String): Int = spendingGroups.indexOf(group).let { if (it in 0..6) it else ReportColors.OTHER }
    fun accountName(id: String?): String = accountsById[id]?.name.orEmpty()
    /** The currency over a money axis, unless amounts are shown without one. */
    val axisUnit: String? = Money.format(0, currency).takeIf { text -> text.any { !it.isDigit() && it !in ".,' - " } }?.let { currency }
}

/** Groups in the budget's order, then in the order their first category appears. */
fun groupNames(categories: List<ReportCategory>, groupOrder: List<String>): List<String> {
    val rank = HashMap<String, Int>()
    groupOrder.forEachIndexed { i, g -> rank[g] = i }
    val names = LinkedHashSet<String>()
    categories.forEach { names.add(it.group) }
    return names.withIndex().sortedWith(compareBy({ rank[it.value] ?: Int.MAX_VALUE }, { it.index })).map { it.value }
}

fun parseReportBudget(budget: JSONObject, today: LocalDate): ReportBudget {
    val accounts = budget.list("accounts").map { a ->
        val kind = a.str("kind")
        ReportAccount(a.str("id"), a.str("name"), kind, a.optBoolean("archived"), kind != "tracking" && a.optBoolean("onBudget", true),
            a.optBoolean("netWorth", true), a.str("accountType"), (a.optJSONObject("source")?.opt("type") as? String).orEmpty(), a.optLong("openingBalance"))
    }
    val categories = budget.list("categories").map { c -> ReportCategory(c.str("id"), c.str("name"), c.str("group"), c.str("paymentAccountId").isNotBlank()) }
    val order = budget.optJSONArray("groupOrder")?.let { a -> (0 until a.length()).map { a.optString(it) } }.orEmpty()
    val groupOf = LinkedHashMap<String, String>()
    val nameOf = LinkedHashMap<String, String>()
    categories.forEach { groupOf[it.id] = it.group; nameOf[it.id] = it.name }
    groupOf[UNCATEGORIZED_BUCKET] = NEEDS_CATEGORY
    nameOf[UNCATEGORIZED_BUCKET] = Words[R.string.category_needed]
    val spending = LinkedHashSet<String>()
    groupNames(categories, order).filter { g -> categories.any { it.group == g && !it.payment } }.forEach { spending.add(it) }
    spending.add(NEEDS_CATEGORY)
    var earliest = today.toString()
    budget.optJSONArray("transactions")?.let { a ->
        for (i in 0 until a.length()) { val date = a.optJSONObject(i)?.str("date").orEmpty(); if (date.isNotEmpty() && date < earliest) earliest = date }
    }
    return ReportBudget(budget.str("currency"), accounts, categories, groupOf, nameOf, spending.toList(), earliest.take(7))
}

// ---------- Ranges and filters ----------

enum class RangePreset(val label: Int) {
    ThisMonth(R.string.range_this_month), LastMonth(R.string.range_last_month), Last3(R.string.range_last_3_months), Last6(R.string.range_last_6_months),
    Last12(R.string.range_last_12_months), Ytd(R.string.range_year_to_date), LastYear(R.string.range_last_year), All(R.string.range_all_dates), Custom(R.string.range_custom),
}

/** Inclusive months ("YYYY-MM"); an empty end means unbounded. */
@Immutable
data class MonthRange(val preset: RangePreset, val from: String, val to: String)

/** The months a preset covers, counted from this month (desktop rangeFor at month granularity). */
fun rangeFor(preset: RangePreset, today: YearMonth, earliest: String): MonthRange {
    val (from, to) = when (preset) {
        RangePreset.ThisMonth -> today to today
        RangePreset.LastMonth -> today.minusMonths(1) to today.minusMonths(1)
        RangePreset.Last3 -> today.minusMonths(2) to today
        RangePreset.Last6 -> today.minusMonths(5) to today
        RangePreset.Last12 -> today.minusMonths(11) to today
        RangePreset.Ytd -> YearMonth.of(today.year, 1) to today
        RangePreset.LastYear -> YearMonth.of(today.year - 1, 1) to YearMonth.of(today.year - 1, 12)
        RangePreset.All, RangePreset.Custom -> null to today
    }
    return MonthRange(preset, from?.toString() ?: earliest.take(7), to.toString())
}

/** A custom range, its ends swapped when they come the wrong way round. */
fun customRange(from: String, to: String): MonthRange =
    if (from.isNotEmpty() && to.isNotEmpty() && from > to) MonthRange(RangePreset.Custom, to, from) else MonthRange(RangePreset.Custom, from, to)

/** The last twelve months when the budget shows this month, else the twelve ending at the budget's month. */
fun defaultRange(month: String, today: YearMonth): MonthRange =
    if (month == today.toString()) rangeFor(RangePreset.Last12, today, "") else MonthRange(RangePreset.Custom, shiftMonth(month, -11), month)

fun describeRange(range: MonthRange): String {
    if (range.preset != RangePreset.Custom) return Words[range.preset.label]
    fun show(v: String) = if (v.isEmpty()) "…" else reportMonthName(v, short = true)
    return Words[R.string.date_range, show(range.from), show(range.to)]
}

@Immutable data class FilterItem(val id: String, val label: String)
@Immutable data class FilterGroup(val name: String, val items: List<FilterItem>)

/** An empty selection means all; a selection of only unknown ids (the empty id) means none. */
fun filterIsOn(selected: Set<String>, id: String) = selected.isEmpty() || id in selected

fun filterSummary(groups: List<FilterGroup>, selected: Set<String>, allLabel: String, noun: String): String =
    filterSummary(groups, selected, allLabel) { "$it $noun" }

/** All, none, the one chosen, or how many ([count] words the number). */
fun filterSummary(groups: List<FilterGroup>, selected: Set<String>, allLabel: String, count: (Int) -> String): String {
    if (selected.isEmpty()) return allLabel
    val chosen = groups.flatMap { it.items }.filter { it.id in selected }
    return when (chosen.size) { 0 -> Words[R.string.filter_none]; 1 -> chosen[0].label; else -> count(chosen.size) }
}

fun filterToggle(groups: List<FilterGroup>, selected: Set<String>, ids: List<String>, on: Boolean): Set<String> {
    val all = groups.flatMap { it.items }.map { it.id }
    val next = LinkedHashSet(if (selected.isNotEmpty()) all.filter { it in selected } else all)
    ids.forEach { if (on) next.add(it) else next.remove(it) }
    return when { next.size == all.size -> emptySet(); next.isEmpty() -> setOf(""); else -> next }
}

/** The accounts filter: budget, tracking and closed accounts (only budget accounts for the forecast). */
fun accountFilterGroups(ctx: ReportBudget, budgetOnly: Boolean): List<FilterGroup> = listOf(
    Triple("budget", R.string.report_accounts_budget, ctx.accounts.filter { !it.archived && it.kind != "tracking" }),
    Triple("tracking", R.string.report_accounts_tracking, ctx.accounts.filter { !it.archived && it.kind == "tracking" }),
    Triple("closed", R.string.report_accounts_closed, ctx.accounts.filter { it.archived }),
).filter { it.third.isNotEmpty() && (!budgetOnly || it.first == "budget") }
    .map { (_, name, items) -> FilterGroup(Words[name], items.map { FilterItem(it.id, it.name) }) }

/** The categories filter: categories that can hold spending, by group, "Needs a category" last. */
fun categoryFilterGroups(ctx: ReportBudget): List<FilterGroup> = ctx.spendingGroups.map { g ->
    val items = ctx.categories.filter { it.group == g && !it.payment }.map { FilterItem(it.id, it.name) }
    FilterGroup(ctx.groupLabel(g), if (g == NEEDS_CATEGORY) items + FilterItem(UNCATEGORIZED_BUCKET, ctx.needsCategory) else items)
}

// ---------- Chart models ----------

enum class Tone { None, Positive, Negative }

@Immutable data class Tile(val label: String, val value: String, val note: String? = null, val tone: Tone = Tone.None)
/** An x-axis label: "Oct", or "Oct 26" when the year changes; [year] is 0 for labels that aren't months. */
@Immutable data class XTick(val short: String, val long: String, val year: Int)
/** One line of a chart's readout, the phone's stand-in for the desktop's hover tooltip. */
@Immutable data class Readout(val label: String, val value: String, val color: Int?, val line: Boolean = false, val dashed: Boolean = false)
@Immutable data class LegendItem(val label: String, val color: Int, val line: Boolean = false, val dashed: Boolean = false)
@Immutable data class ColumnSeries(val key: String, val label: String, val color: Int, val values: List<Double>)
@Immutable data class LineOverlay(val label: String, val color: Int, val values: List<Double?>)

@Immutable
data class ColumnModel(
    val label: String, val keys: List<String>, val series: List<ColumnSeries>, val grouped: Boolean, val line: LineOverlay?,
    val average: Double?, val averageText: String?, val ticks: List<Double>, val tickLabels: List<String>, val unit: String?,
    val xTicks: List<XTick?>, val titles: List<String>, val readouts: List<List<Readout>>,
)

@Immutable data class LineSeries(val key: String, val label: String, val color: Int, val values: List<Double?>, val dashed: Boolean = false)

@Immutable
data class LinesModel(
    val label: String, val keys: List<String>, val series: List<LineSeries>, val wash: Boolean, val ticks: List<Double>, val tickLabels: List<String>,
    val unit: String?, val xTicks: List<XTick?>, val titles: List<String>, val readouts: List<List<Readout>>, val endLabels: List<String?>,
)

fun monthXTick(month: String): XTick? = runCatching { val m = YearMonth.parse(month); val name = m.format(Money.pattern("MMM")); XTick(name, "$name ${month.substring(2, 4)}", m.year) }.getOrNull()
fun monthTitle(month: String): String = reportMonthName(month)
fun plainTick(text: String) = XTick(text, text, 0)

fun columnModel(
    label: String, keys: List<String>, series: List<ColumnSeries>, format: (Double) -> String, axis: (Double) -> String, unit: String?,
    grouped: Boolean = false, line: LineOverlay? = null, average: Pair<String, Double>? = null,
    xTick: (String, Int) -> XTick? = { k, _ -> monthXTick(k) }, title: (String) -> String = ::monthTitle,
): ColumnModel {
    fun at(s: ColumnSeries, i: Int) = s.values.getOrElse(i) { 0.0 }
    val ups = keys.indices.map { i -> if (grouped) max(0.0, series.maxOfOrNull { at(it, i) } ?: 0.0) else series.sumOf { max(0.0, at(it, i)) } }
    val downs = keys.indices.map { i -> if (grouped) min(0.0, series.minOfOrNull { at(it, i) } ?: 0.0) else series.sumOf { min(0.0, at(it, i)) } }
    val lineValues = line?.values?.filterNotNull().orEmpty()
    val ticks = niceTicks(minOf(0.0, downs.minOrNull() ?: 0.0, lineValues.minOrNull() ?: 0.0),
        maxOf(0.0, ups.maxOrNull() ?: 0.0, lineValues.maxOrNull() ?: 0.0, average?.second ?: 0.0))
    val readouts = keys.indices.map { i ->
        buildList {
            series.filter { at(it, i) != 0.0 }.forEach { add(Readout(it.label, format(at(it, i)), it.color)) }
            val total = series.sumOf { at(it, i) }
            val overlay = line?.values?.getOrNull(i)
            // A line that is the stack's total (net worth) is its total already.
            if (series.size > 1 && !grouped && (overlay == null || kotlin.math.abs(overlay - total) >= 0.005)) add(Readout(Words[R.string.report_total], format(total), null))
            overlay?.let { add(Readout(line.label, format(it), line.color, line = true)) }
        }
    }
    return ColumnModel(label, keys, series, grouped, line, average?.second, average?.let { Words[R.string.report_average_line, it.first, format(it.second)] }, ticks,
        ticks.map(axis), unit, keys.mapIndexed { i, k -> xTick(k, i) }, keys.map(title), readouts)
}

fun linesModel(
    label: String, keys: List<String>, series: List<LineSeries>, format: (Double) -> String, axis: (Double) -> String, unit: String?,
    xTick: (String, Int) -> XTick? = { k, _ -> monthXTick(k) }, title: (String) -> String = ::monthTitle, wash: Boolean = series.size == 1,
): LinesModel {
    val known = series.flatMap { s -> s.values.filterNotNull() }
    val ticks = niceTicks(minOf(0.0, known.minOrNull() ?: 0.0), maxOf(1.0, known.maxOrNull() ?: 1.0))
    val readouts = keys.indices.map { i ->
        series.mapNotNull { s -> s.values.getOrNull(i)?.let { Readout(s.label, format(it), s.color, line = true, dashed = s.dashed) } }
    }
    val ends = series.map { s -> if (series.size <= 4) s.values.lastOrNull { it != null }?.let(format) else null }
    return LinesModel(label, keys, series, wash, ticks, ticks.map(axis), unit, keys.mapIndexed { i, k -> xTick(k, i) }, keys.map(title), readouts, ends)
}

@Immutable data class MiniSeries(val label: String, val color: Int, val values: List<Long>)

@Immutable
data class MiniBarsModel(val labels: List<String>, val series: List<MiniSeries>) {
    val top: Long = max(0L, series.flatMap { it.values }.maxOrNull() ?: 0L)
    val bottom: Long = min(0L, series.flatMap { it.values }.minOrNull() ?: 0L)
}

@Immutable data class SliceModel(val key: String, val label: String, val value: Long, val color: Int)
@Immutable data class BarRowModel(val key: String, val label: String, val valueText: String, val detail: String?, val color: Int?, val fraction: Float, val drill: Boolean = false)
@Immutable data class DivRowModel(val key: String, val label: String, val valueText: String, val detail: String, val fraction: Float, val color: Int)
@Immutable data class TreemapItem(val key: String, val label: String, val value: Long, val valueText: String, val detail: String, val more: String, val level: Int)

/** What a report figure's transactions are: a category, payee, day or income source over a range. */
@Immutable data class DrillTarget(val title: String, val dimension: String, val key: String, val from: String? = null, val to: String? = null)

enum class RowStyle { Plain, Section, Group, Child, Total, Net, Muted }

/** A row of a report's list: a month, an account, a source. */
@Immutable
data class FigureRow(
    val key: String, val title: String, val value: String = "", val subtitle: String? = null, val tone: Tone = Tone.None,
    val style: RowStyle = RowStyle.Plain, val drill: DrillTarget? = null, val expand: String? = null,
)

// ---------- Report views ----------

/** A report's body, ready to lay out. */
sealed interface ReportView { val id: String }

@Immutable
data class ChangesView(val title: String, val aside: String, val rows: List<DivRowModel>, val legend: List<LegendItem>)

@Immutable
data class CategoryView(
    val group: String?, val tiles: List<Tile>, val slices: List<SliceModel>, val centerValue: String, val centerLabel: String,
    val rows: List<BarRowModel>, val help: String, val changes: ChangesView?,
) : ReportView { override val id get() = "categories" }

@Immutable data class PayeeRow(val key: String, val name: String, val label: String, val amountText: String, val detail: String)

@Immutable
data class PayeeView(
    val tiles: List<Tile>, val title: String, val aside: String, val map: List<TreemapItem>, val help: String, val more: String?, val rows: List<PayeeRow>,
) : ReportView { override val id get() = "payees" }

@Immutable
data class OverTimeView(val byGroup: Boolean, val tiles: List<Tile>, val chart: ColumnModel, val legend: List<LegendItem>, val rows: List<FigureRow>) : ReportView { override val id get() = "trends" }

@Immutable data class HeatMonth(val index: Int, val text: String)

@Immutable
data class HeatModel(
    val days: List<String>, val levels: List<Int>, val lead: Int, val weeks: Int, val months: List<HeatMonth>, val weekdayLabels: List<String>,
    val titles: List<String>, val amounts: List<String>, val spent: List<Boolean>,
)

@Immutable
data class CalendarView(val tiles: List<Tile>, val heat: HeatModel?, val weekdays: ColumnModel, val biggest: List<FigureRow>) : ReportView { override val id get() = "calendar" }

@Immutable
data class IncomeExpenseView(
    val tiles: List<Tile>, val help: String, val chart: ColumnModel, val legend: List<LegendItem>, val rate: LinesModel, val cumulative: LinesModel,
    val table: List<FigureRow>, val children: Map<String, List<FigureRow>>, val months: List<FigureRow>,
) : ReportView { override val id get() = "incomeExpense" }

@Immutable
data class IncomeView(
    val tiles: List<Tile>, val slices: List<SliceModel>, val centerValue: String, val bars: List<BarRowModel>, val chartHelp: String?,
    val monthly: ColumnModel?, val legend: List<LegendItem>, val sources: List<FigureRow>, val sourcesHelp: String?,
) : ReportView { override val id get() = "income" }

@Immutable
data class CashFlowView(val tiles: List<Tile>, val chart: ColumnModel, val legend: List<LegendItem>, val running: LinesModel, val months: List<FigureRow>) :
    ReportView { override val id get() = "cashflow" }

@Immutable
data class NetWorthView(
    val tiles: List<Tile>, val chart: ColumnModel, val legend: List<LegendItem>, val change: ColumnModel, val changeLegend: List<LegendItem>,
    val dates: String, val accounts: List<FigureRow>,
) : ReportView { override val id get() = "networth" }

@Immutable
data class BalanceSheetView(
    val tiles: List<Tile>, val ownTotal: String, val own: List<BarRowModel>, val oweTotal: String, val owe: List<BarRowModel>,
    val statementAside: String, val dates: String, val statement: List<FigureRow>, val help: String,
) : ReportView { override val id get() = "balance" }

@Immutable data class AccountChip(val id: String, val name: String, val slot: Int?, val enabled: Boolean)

@Immutable
data class BalancesView(
    val tiles: List<Tile>, val chips: List<AccountChip>, val chosen: Map<String, Int>, val chart: LinesModel?, val legend: List<LegendItem>, val rows: List<FigureRow>,
) : ReportView { override val id get() = "balances" }

@Immutable
data class AgeView(val tiles: List<Tile>, val chart: LinesModel, val rows: List<FigureRow>) : ReportView { override val id get() = "age" }

/** What a report needs besides the engine's figures: its range, accounts and today. */
@Immutable
data class ReportScope(val from: String, val to: String, val accounts: Set<String>, val someCategories: Boolean, val today: LocalDate, val weekStart: Int)

private val moneyAxis: (Double) -> String = { compactMoney(it) }

/** Totals by group, or by category within one group. */
fun totalsBy(categories: Map<String, Long>, ctx: ReportBudget, group: String?): LinkedHashMap<String, Long> {
    val out = LinkedHashMap<String, Long>()
    for ((id, v) in categories) {
        val g = ctx.groupOf[id]?.takeIf { it.isNotEmpty() } ?: continue
        val key = if (group == null) g else if (g == group) id else null
        if (key != null) out[key] = (out[key] ?: 0L) + v
    }
    return out
}

private fun barRows(rows: List<Triple<String, String, Long>>, colors: (String) -> Int?, total: Long?, ctx: ReportBudget, drill: Boolean): List<BarRowModel> {
    val most = max(1L, rows.maxOfOrNull { it.third } ?: 1L)
    return rows.map { (key, label, value) ->
        BarRowModel(key, label, ctx.money(value), total?.let { share(value, it) }, colors(key), (max(0L, value).toDouble() / most).toFloat(), drill)
    }
}

/** Where spending went, by group or by category within one group, with the last month against the average. */
fun categoryView(data: ReportData, ctx: ReportBudget, group: String?, today: LocalDate): CategoryView {
    val n = max(1, data.months.size)
    val order = if (group != null) LinkedHashSet<String>().apply {
        ctx.categories.filter { it.group == group && !it.payment }.forEach { add(it.id) }
        data.categories.keys.filter { ctx.groupOf[it] == group }.forEach { add(it) }
    }.toList() else emptyList()
    fun catColor(id: String) = order.indexOf(id).let { if (it in 0..6) it else ReportColors.OTHER }
    val totals = totalsBy(data.categories, ctx, group)
    data class Row(val key: String, val label: String, val value: Long, val color: Int)
    val rows = (if (group != null) totals.keys.toList() else ctx.spendingGroups)
        .map { key -> Row(key, if (group != null) ctx.nameOf[key] ?: key else ctx.groupLabel(key), totals[key] ?: 0L, if (group != null) catColor(key) else ctx.colorOf(key)) }
        .filter { it.value != 0L }.sortedByDescending { it.value }
    val total = rows.sumOf { it.value }
    val other = rows.filter { it.color == ReportColors.OTHER && it.value > 0 }
    val slices = rows.filter { it.color != ReportColors.OTHER && it.value > 0 }.map { SliceModel(it.key, it.label, it.value, it.color) } +
        (if (other.isNotEmpty()) listOf(SliceModel("__other", Words[R.string.report_other], other.sumOf { it.value }, ReportColors.OTHER)) else emptyList())
    val top = rows.firstOrNull()
    val tiles = listOf(
        Tile(if (group != null) Words[R.string.report_spent_in, ctx.groupLabel(group)] else Words[R.string.report_spending], ctx.money(total),
            if (group != null) Words[R.string.report_of_all_spending, share(total, data.spending)] else Words.count(R.plurals.report_over_months, n.toLong())),
        Tile(Words[R.string.report_monthly_average], ctx.money(roundDiv(total, n))),
        Tile(Words[if (group != null) R.string.report_biggest_category else R.string.report_biggest_group], top?.label ?: "—", top?.let { "${ctx.money(it.value)} · ${share(it.value, total)}" }),
        Tile(Words[if (group != null) R.string.report_categories_with_spending else R.string.report_groups_with_spending], rows.count { it.value > 0 }.toString()),
    )
    val colors = rows.associate { it.key to it.color }
    val help = Words[if (group != null) R.string.report_categories_help_group else R.string.report_categories_help_all] + " " + Words[R.string.report_categories_help_refunds]
    return CategoryView(group, tiles, slices, ctx.money(total), group?.let(ctx::groupLabel) ?: Words[R.string.report_spent],
        barRows(rows.map { Triple(it.key, it.label, it.value) }, { colors[it] }, total, ctx, drill = true), help,
        if (data.months.size >= 2) changesView(data, ctx, group, YearMonth.from(today).toString()) else null)
}

/** The last month of the range against the average of the months before it. */
fun changesView(data: ReportData, ctx: ReportBudget, group: String?, todayMonth: String): ChangesView {
    val all = data.months
    val months = if (all.last().month == todayMonth && all.size > 2) all.dropLast(1) else all
    val last = months.last()
    val earlier = months.dropLast(1)
    val now = totalsBy(last.categories, ctx, group)
    val average = LinkedHashMap<String, Double>()
    for (m in earlier) for ((k, v) in totalsBy(m.categories, ctx, group)) average[k] = (average[k] ?: 0.0) + v.toDouble() / earlier.size
    val keys = LinkedHashSet<String>(now.keys).apply { addAll(average.keys) }
    data class Row(val key: String, val label: String, val value: Long, val detail: String)
    val rows = keys.map { key ->
        val avg = average[key] ?: 0.0
        Row(key, if (group != null) ctx.nameOf[key] ?: key else ctx.groupLabel(key), Math.round((now[key] ?: 0L) - avg), Words[R.string.report_avg, ctx.money(Math.round(avg))])
    }.filter { it.value != 0L }.sortedByDescending { abs(it.value) }.take(10)
    val most = max(1L, rows.maxOfOrNull { abs(it.value) } ?: 1L)
    val partial = last.month == todayMonth
    return ChangesView(
        Words[if (partial) R.string.report_changes_title_partial else R.string.report_changes_title, reportMonthName(last.month)],
        Words.count(R.plurals.report_changes_aside, earlier.size.toLong()),
        rows.map { DivRowModel(it.key, it.label, ctx.signed(it.value), it.detail, (it.value.toDouble() / most).toFloat(),
            if (it.value > 0) ReportColors.DIVERGING_NEGATIVE else ReportColors.DIVERGING_POSITIVE) },
        listOf(LegendItem(Words[R.string.report_spent_less], ReportColors.DIVERGING_POSITIVE), LegendItem(Words[R.string.report_spent_more], ReportColors.DIVERGING_NEGATIVE)),
    )
}

/**
 * The payees big enough to read as tiles (at least 1.5% of the set, 8 to 14 of them) and the
 * rest, which share one "others" line. A single leftover payee keeps its own tile.
 */
fun <T> foldPayees(list: List<T>, amount: (T) -> Long): Pair<List<T>, List<T>> {
    val total = list.sumOf(amount).toDouble()
    var n = 0
    while (n < list.size && n < 14 && (n < 8 || amount(list[n]) / total >= 0.015)) n++
    return if (list.size - n <= 1) list to emptyList() else list.take(n) to list.drop(n)
}

fun payeeView(data: ReportData, ctx: ReportBudget): PayeeView {
    val all = data.payees
    val total = all.sumOf { it.amount }
    val count = data.transactionCount ?: all.sumOf { it.count }
    val positive = all.filter { it.amount > 0 }
    val positiveTotal = positive.sumOf { it.amount }
    val refunds = -all.filter { it.amount < 0 }.sumOf { it.amount }
    val top = positive.firstOrNull()
    val (big, rest) = foldPayees(positive) { it.amount }
    val restTotal = rest.sumOf { it.amount }
    fun average(p: PayeeSpend) = ctx.money(Math.round(p.amount.toDouble() / max(1L, p.count)))
    val levels = heatLevels(big.map { it.amount })
    val noPayee = Words[R.string.payee_none]
    val tiles = listOf(
        Tile(Words[R.string.report_spending], ctx.money(total), Words.count(R.plurals.report_at_payees, all.size.toLong())),
        Tile(Words[R.string.report_top_payee], top?.let { it.name.ifEmpty { noPayee } } ?: "—", top?.let { Words[R.string.report_amount_share_shown, ctx.money(it.amount), share(it.amount, positiveTotal)] }),
        Tile(Words[R.string.report_average_per_transaction], if (count != 0L) ctx.money(Math.round(total.toDouble() / count)) else "—", Words.count(R.plurals.report_transactions_count, count)),
        Tile(Words[R.string.report_top_5_payees], share(positive.take(5).sumOf { it.amount }, positiveTotal), Words[R.string.report_of_spending_shown]),
    )
    return PayeeView(
        tiles,
        if (rest.isNotEmpty()) Words.count(R.plurals.report_top_payees, big.size.toLong()) else Words[R.string.report_where_money_goes],
        if (rest.isNotEmpty()) Words[R.string.report_payees_aside_share, share(positiveTotal - restTotal, positiveTotal)] else Words[R.string.report_payees_aside],
        big.map { p ->
            TreemapItem(p.name, p.name.ifEmpty { noPayee }, p.amount, ctx.money(p.amount), Words[R.string.report_share_of_spending_shown, share(p.amount, positiveTotal)],
                Words.count(R.plurals.report_payee_tile_more, p.count, average(p)), levels.level(p.amount))
        },
        Words[R.string.report_payees_help] + (if (refunds > 0) " " + Words[R.string.report_payees_help_refunds, ctx.money(refunds)] else ""),
        if (rest.isNotEmpty()) Words.count(R.plurals.report_more_payees, rest.size.toLong(), ctx.money(restTotal)) else null,
        all.map { p ->
            PayeeRow(p.name, p.name, p.name.ifEmpty { noPayee }, ctx.money(p.amount), Words.count(R.plurals.report_payee_row_detail, p.count, share(p.amount, total), average(p)))
        },
    )
}

/** The trend tile: the later half of the months against the earlier half. */
fun trendTile(totals: List<Long>): Tile {
    val half = totals.size / 2
    val recent = if (half > 0) totals.takeLast(half).sum().toDouble() / half else 0.0
    val before = if (half > 0) totals.take(half).sum().toDouble() / half else 0.0
    val known = half > 0 && before != 0.0
    return Tile(Words[R.string.report_trend],
        if (known) signedPercent((recent - before) / before * 100, 0) else "—",
        if (half > 0) Words.count(R.plurals.report_trend_note, half.toLong()) else Words[R.string.report_trend_needs_two],
        if (known) (if (recent > before) Tone.Negative else Tone.Positive) else Tone.None)
}

fun overTimeView(data: ReportData, ctx: ReportBudget, byGroup: Boolean): OverTimeView {
    val keys = data.months.map { it.month }
    val series = ArrayList<ColumnSeries>()
    for (g in ctx.spendingGroups) {
        val color = ctx.colorOf(g)
        val label = if (color == ReportColors.OTHER) Words[R.string.report_other] else ctx.groupLabel(g)
        val values = data.months.map { m -> m.categories.entries.filter { ctx.groupOf[it.key] == g }.sumOf { it.value }.toDouble() }
        val existing = series.indexOfFirst { it.label == label }
        if (existing >= 0) series[existing] = series[existing].copy(values = series[existing].values.mapIndexed { i, v -> v + values[i] })
        else series.add(ColumnSeries(g, label, color, values))
    }
    val shown = series.filter { s -> s.values.any { it != 0.0 } }
    val totals = data.months.map { it.spending }
    val average = if (totals.isNotEmpty()) roundDiv(totals.sum(), totals.size) else 0L
    val byAmount = data.months.sortedByDescending { it.spending }
    val fmt: (Double) -> String = { ctx.money(it) }
    val chart = columnModel(Words[if (byGroup) R.string.report_chart_spending_by_group else R.string.report_chart_spending_by_month], keys,
        if (byGroup) shown else listOf(ColumnSeries("total", Words[R.string.report_spending], 0, totals.map { it.toDouble() })), fmt, moneyAxis, ctx.axisUnit,
        average = Words[R.string.report_average] to average.toDouble())
    val rows = data.months.mapIndexed { i, m ->
        FigureRow(m.month, reportMonthName(m.month, short = true), ctx.money(m.spending),
            if (byGroup) shown.filter { it.values[i] != 0.0 }.joinToString(" · ") { "${Money.own(it.label)} ${ctx.money(it.values[i])}" }.ifEmpty { null } else null)
    } + FigureRow("__average", Words[R.string.report_average], ctx.money(average),
        if (byGroup) shown.joinToString(" · ") { s -> "${Money.own(s.label)} ${ctx.money(Math.round(s.values.sum() / max(1, keys.size)))}" }.ifEmpty { null } else null,
        style = RowStyle.Total)
    return OverTimeView(byGroup, listOf(
        Tile(Words[R.string.report_monthly_average], ctx.money(average)),
        Tile(Words[R.string.report_highest_month], byAmount.firstOrNull()?.let { ctx.money(it.spending) } ?: "—", byAmount.firstOrNull()?.let { reportMonthName(it.month) }),
        Tile(Words[R.string.report_lowest_month], byAmount.lastOrNull()?.let { ctx.money(it.spending) } ?: "—", byAmount.lastOrNull()?.let { reportMonthName(it.month) }),
        trendTile(totals),
    ), chart, if (byGroup) shown.map { LegendItem(it.label, it.color) } else emptyList(), rows)
}

/** Every day of the range up to today, at most 3,700 of them. */
fun calendarDays(from: String, to: String, today: LocalDate): List<String> {
    val start = runCatching { LocalDate.parse(firstDay(from)) }.getOrNull() ?: return emptyList()
    val end = minOf(runCatching { LocalDate.parse(lastDay(to)) }.getOrNull() ?: return emptyList(), today)
    val out = ArrayList<String>()
    var d = start
    while (!d.isAfter(end) && out.size < 3700) { out.add(d.toString()); d = d.plusDays(1) }
    return out
}

/** Average spending on each weekday, starting from the week's first day. */
fun weekdayAverages(days: List<String>, values: Map<String, Long>, weekStart: Int): List<Long> {
    val totals = LongArray(7)
    val counts = IntArray(7)
    days.forEach { d -> val w = jsWeekday(LocalDate.parse(d)); totals[w] += values[d] ?: 0L; counts[w]++ }
    return (0 until 7).map { i -> val w = (i + weekStart) % 7; if (counts[w] > 0) roundDiv(totals[w], counts[w]) else 0L }
}

fun calendarView(data: ReportData, ctx: ReportBudget, scope: ReportScope): CalendarView {
    val values = data.days
    val days = calendarDays(scope.from, scope.to, scope.today)
    val spent = days.filter { (values[it] ?: 0L) > 0 }
    val busiest = spent.sortedByDescending { values[it] ?: 0L }
    val weekdays = (0 until 7).map { (it + scope.weekStart) % 7 }
    val byWeekday = weekdayAverages(days, values, scope.weekStart)
    val top = byWeekday.indexOf(byWeekday.maxOrNull() ?: 0L)
    fun dayTitle(d: String) = Words[R.string.report_day_title, weekdayName(jsWeekday(LocalDate.parse(d))), Money.date(d)]
    val tiles = listOf(
        Tile(Words[R.string.report_average_a_day], if (days.isNotEmpty()) ctx.money(roundDiv(days.sumOf { values[it] ?: 0L }, days.size)) else "—"),
        Tile(Words[R.string.report_no_spend_days], (days.size - spent.size).toString(), Words.count(R.plurals.report_of_days, days.size.toLong(), share((days.size - spent.size).toLong(), days.size.toLong()))),
        Tile(Words[R.string.report_biggest_day], busiest.firstOrNull()?.let { ctx.money(values[it] ?: 0L) } ?: "—", busiest.firstOrNull()?.let { Money.date(it) }),
        Tile(Words[R.string.report_busiest_weekday], if (spent.isNotEmpty()) weekdayName(weekdays[top]) else "—", if (spent.isNotEmpty()) Words[R.string.report_on_average, ctx.money(byWeekday[top])] else null),
    )
    val heat = if (days.isEmpty()) null else {
        val scale = heatLevels(days.map { values[it] ?: 0L })
        val first = LocalDate.parse(days.first())
        val lead = (jsWeekday(first) - scope.weekStart + 7) % 7
        HeatModel(days, days.map { scale.level(values[it] ?: 0L) }, lead, ceil((days.size + lead) / 7.0).toInt(),
            days.mapIndexedNotNull { i, d -> if (d.endsWith("-01") || i == 0) HeatMonth(i, monthAbbrev(d)) else null },
            listOf(1, 3, 5).map { weekdayName((it + scope.weekStart) % 7, short = true) },
            days.map(::dayTitle), days.map { ctx.money(values[it] ?: 0L) }, days.map { (values[it] ?: 0L) > 0 })
    }
    val weekdayChart = columnModel(Words[R.string.report_chart_weekdays], weekdays.map { it.toString() },
        listOf(ColumnSeries("avg", Words[R.string.report_average], 0, byWeekday.map { it.toDouble() })), { ctx.money(it) }, moneyAxis, ctx.axisUnit,
        xTick = { k, _ -> plainTick(weekdayName(k.toInt(), short = true)) }, title = { weekdayName(it.toInt()) })
    return CalendarView(tiles, heat, weekdayChart, busiest.take(10).map { d ->
        FigureRow(d, Money.date(d), ctx.money(values[d] ?: 0L), weekdayName(jsWeekday(LocalDate.parse(d)), short = true),
            drill = DrillTarget(dayTitle(d), "day", d, d, d))
    })
}

/** Income against spending, the surplus and its rate, month by month. */
fun incomeExpenseView(data: ReportData, ctx: ReportBudget, someCategories: Boolean): IncomeExpenseView {
    val keys = data.months.map { it.month }
    val n = max(1, keys.size)
    val net = data.months.map { it.netIncome }
    val kept = data.income - data.spending
    val rates = data.months.map { if (it.income > 0) Math.round(it.netIncome.toDouble() / it.income * 1000) / 10.0 else null }
    var running = 0L
    val cumulative = net.map { running += it; running }
    val saved = net.count { it > 0 }
    var best: ReportMonth? = null
    data.months.forEach { m -> if (best == null || m.netIncome > best!!.netIncome) best = m }
    val rateText = if (data.income > 0) Money.ltr("${fixed(kept.toDouble() / data.income * 100, 1)}%") else "—"
    val fmt: (Double) -> String = { ctx.money(it) }
    val percent: (Double) -> String = { Money.ltr("${jsNumber(it)}%") }
    val income = Words[R.string.report_income]
    val spending = Words[R.string.report_spending]
    val surplus = Words[R.string.report_budget_surplus]
    val surplusRate = Words[R.string.report_budget_surplus_rate]
    val tiles = listOf(
        Tile(income, ctx.money(data.income), Words[R.string.report_a_month, ctx.money(roundDiv(data.income, n))]),
        Tile(spending, ctx.money(data.spending), Words[R.string.report_a_month, ctx.money(roundDiv(data.spending, n))]),
        Tile(surplus, ctx.money(kept), Words[R.string.report_a_month, ctx.signed(roundDiv(kept, n))], if (kept < 0) Tone.Negative else Tone.Positive),
        Tile(surplusRate, rateText, Words[R.string.report_surplus_share_of_income]),
        Tile(Words[R.string.report_positive_surplus_months], Words[R.string.report_x_of_y, "$saved", "${data.months.size}"], best?.takeIf { it.netIncome > 0 }?.let { Words[R.string.report_best_month, reportMonthName(it.month)] }),
    )
    val help = Words[R.string.report_surplus_help] + (if (someCategories) " " + Words[R.string.report_surplus_help_categories] else "")
    val chart = columnModel(Words[R.string.report_chart_income_spending], keys, listOf(
        ColumnSeries("income", income, 0, data.months.map { it.income.toDouble() }),
        ColumnSeries("spending", spending, 1, data.months.map { it.spending.toDouble() }),
    ), fmt, moneyAxis, ctx.axisUnit, grouped = true, line = LineOverlay(surplus, ReportColors.INK, net.map { it.toDouble() }))
    val legend = listOf(LegendItem(income, 0), LegendItem(spending, 1), LegendItem(surplus, ReportColors.INK, line = true))
    val rate = linesModel(Words[R.string.report_chart_surplus_rate], keys, listOf(LineSeries("rate", surplusRate, 0, rates)), percent, percent, null)
    val cumulativeChart = linesModel(Words[R.string.report_chart_surplus_cumulative], keys,
        listOf(LineSeries("surplus", Words[R.string.report_cumulative_surplus], 2, cumulative.map { it.toDouble() })), fmt, moneyAxis, ctx.axisUnit)
    fun totalRow(key: String, title: String, values: List<Long>, style: RowStyle, expand: String? = null, tone: Boolean = false): FigureRow {
        val sum = values.sum()
        return FigureRow(key, title, ctx.money(sum), Words[R.string.report_avg_a_month, ctx.money(roundDiv(sum, n))], if (tone && sum < 0) Tone.Negative else Tone.None, style, expand = expand)
    }
    val sources = data.incomeSources.entries.sortedByDescending { it.value }
    val groupRows = ctx.spendingGroups.map { g ->
        val ids = data.categories.keys.filter { ctx.groupOf[it] == g }
        Triple(g, data.months.map { m -> ids.sumOf { m.categories[it] ?: 0L } },
            ids.map { id -> id to data.months.map { it.categories[id] ?: 0L } }.sortedByDescending { it.second.sum() })
    }.filter { (_, values, _) -> values.any { it != 0L } }
    val table = buildList {
        add(FigureRow("__income", income, style = RowStyle.Section))
        sources.forEach { (payee, _) -> add(totalRow("in:$payee", payee.ifEmpty { Words[R.string.payee_none] }, data.months.map { it.incomeSources[payee] ?: 0L }, RowStyle.Plain)) }
        add(totalRow("__totalIncome", Words[R.string.report_total_income], data.months.map { it.income }, RowStyle.Total, tone = true))
        add(FigureRow("__spending", spending, style = RowStyle.Section))
        groupRows.forEach { (g, values, _) -> add(totalRow("g:$g", ctx.groupLabel(g), values, RowStyle.Group, expand = g)) }
        add(totalRow("__totalSpending", Words[R.string.report_total_spending], data.months.map { it.spending }, RowStyle.Total, tone = true))
        add(totalRow("__surplus", surplus, net, RowStyle.Net, tone = true))
        add(FigureRow("__rate", surplusRate, rateText, style = RowStyle.Net, tone = if (kept < 0 && data.income > 0) Tone.Negative else Tone.None))
    }
    val children = groupRows.associate { (g, _, categories) ->
        g to categories.map { (id, values) -> totalRow("c:$id", ctx.nameOf[id] ?: id, values, RowStyle.Child) }
    }
    val months = data.months.mapIndexed { i, m ->
        FigureRow(m.month, reportMonthName(m.month, short = true), ctx.money(m.netIncome),
            Words[R.string.report_income_month_detail, ctx.money(m.income), ctx.money(m.spending), rates[i]?.let { Money.ltr("${jsNumber(it)}%") } ?: "—"],
            if (m.netIncome < 0) Tone.Negative else Tone.None)
    }
    return IncomeExpenseView(tiles, help, chart, legend, rate, cumulativeChart, table, children, months)
}

/** Where income comes from: sources ranked, as a whole and month by month. */
fun incomeView(data: ReportData, ctx: ReportBudget): IncomeView {
    val n = max(1, data.months.size)
    val noPayee = Words[R.string.payee_none]
    val all = data.incomeSources.entries.map { Triple(it.key, it.key.ifEmpty { noPayee }, it.value) }
    val positive = all.filter { it.third > 0 }.sortedByDescending { it.third }
    val negative = all.filter { it.third < 0 }.sortedBy { it.third }
    val coloured = positive.mapIndexed { i, s -> SliceModel(s.first, s.second, s.third, if (i < 7) i else ReportColors.OTHER) }
    val rest = coloured.drop(7)
    val folded = if (rest.isNotEmpty()) coloured.take(7) + SliceModel("__other", Words[R.string.report_other_sources], rest.sumOf { it.value }, ReportColors.OTHER) else coloured
    val total = positive.sumOf { it.third }
    val topKeys = coloured.take(7).map { it.key }.toSet()
    val series = folded.map { s ->
        ColumnSeries(s.key, s.label, s.color, data.months.map { m ->
            if (s.key == "__other") m.incomeSources.entries.filter { it.key !in topKeys && it.value > 0 }.sumOf { it.value }.toDouble()
            else max(0L, m.incomeSources[s.key] ?: 0L).toDouble()
        })
    }
    val top = positive.firstOrNull()
    val tiles = listOf(
        Tile(Words[R.string.report_income], ctx.money(data.income),
            if (negative.isNotEmpty()) Words[R.string.report_after_taken_out, ctx.money(-negative.sumOf { it.third })] else Words.count(R.plurals.report_from_sources, positive.size.toLong())),
        Tile(Words[R.string.report_monthly_average], ctx.money(roundDiv(data.income, n))),
        Tile(Words[R.string.report_largest_source], top?.second ?: "—", top?.let { "${ctx.money(it.third)} · ${share(it.third, total)}" }),
        Tile(Words[R.string.report_months_with_income], Words[R.string.report_x_of_y, "${data.months.count { it.income > 0 }}", "${data.months.size}"]),
    )
    val most = max(1L, coloured.maxOfOrNull { it.value } ?: 1L)
    val bars = coloured.map { BarRowModel(it.key, it.label, ctx.money(it.value), share(it.value, total), it.color, (it.value.toDouble() / most).toFloat()) }
    val monthly = if (positive.isNotEmpty()) columnModel(Words[R.string.report_chart_income_by_source], data.months.map { it.month }, series, { ctx.money(it) }, moneyAxis, ctx.axisUnit) else null
    val sources = (positive + negative).map { (key, label, value) ->
        FigureRow("s:$key", label, ctx.money(value),
            Words[R.string.report_source_detail, ctx.money(roundDiv(value, n)), if (value > 0) share(value, total) else "—", "${data.months.count { (it.incomeSources[key] ?: 0L) != 0L }}"],
            if (value < 0) Tone.Negative else Tone.None, drill = DrillTarget(label, "income", key))
    }
    return IncomeView(tiles, folded, ctx.money(total), bars,
        if (negative.isNotEmpty()) Words[R.string.report_income_chart_help] else null,
        monthly, series.map { LegendItem(it.label, it.color) }, sources,
        if (negative.isNotEmpty()) Words[R.string.report_income_sources_help] else null)
}

/** Flows across the budget-account boundary, from recorded account movements. */
fun cashFlowView(data: ReportData, ctx: ReportBudget): CashFlowView {
    val keys = data.months.map { it.month }
    val n = max(1, keys.size)
    val inflow = data.months.map { it.inflow }
    val outflow = data.months.map { it.outflow }
    val net = inflow.mapIndexed { i, v -> v - outflow[i] }
    var running = 0L
    val cumulative = net.map { running += it; running }
    val totalIn = inflow.sum()
    val totalOut = outflow.sum()
    val fmt: (Double) -> String = { ctx.money(it) }
    val moneyIn = Words[R.string.report_money_in]
    val moneyOut = Words[R.string.report_money_out]
    val netFlow = Words[R.string.report_net_flow]
    val chart = columnModel(Words[R.string.report_chart_cash_flow], keys, listOf(
        ColumnSeries("in", moneyIn, 0, inflow.map { it.toDouble() }),
        ColumnSeries("out", moneyOut, 1, outflow.map { -it.toDouble() }),
    ), fmt, moneyAxis, ctx.axisUnit, line = LineOverlay(netFlow, ReportColors.INK, net.map { it.toDouble() }))
    val months = keys.mapIndexed { i, m ->
        FigureRow(m, reportMonthName(m, short = true), ctx.money(net[i]),
            Words[R.string.report_cash_flow_month_detail, ctx.money(inflow[i]), ctx.money(outflow[i]), ctx.money(cumulative[i])], if (net[i] < 0) Tone.Negative else Tone.None)
    } + FigureRow("__average", Words[R.string.report_average], ctx.money(roundDiv(totalIn - totalOut, n)),
        Words[R.string.report_cash_flow_average_detail, ctx.money(roundDiv(totalIn, n)), ctx.money(roundDiv(totalOut, n))], style = RowStyle.Total)
    return CashFlowView(listOf(
        Tile(moneyIn, ctx.money(totalIn), Words[R.string.report_a_month, ctx.money(roundDiv(totalIn, n))]),
        Tile(moneyOut, ctx.money(totalOut), Words[R.string.report_a_month, ctx.money(roundDiv(totalOut, n))]),
        Tile(netFlow, ctx.money(totalIn - totalOut), tone = if (totalIn - totalOut < 0) Tone.Negative else Tone.Positive),
        Tile(Words[R.string.report_months_in_black], Words[R.string.report_x_of_y, "${net.count { it > 0 }}", "${keys.size}"]),
    ), chart, listOf(LegendItem(moneyIn, 0), LegendItem(moneyOut, 1), LegendItem(netFlow, ReportColors.INK, line = true)),
        linesModel(Words[R.string.report_chart_net_flow_cumulative], keys, listOf(LineSeries("running", Words[R.string.report_running_total], 2, cumulative.map { it.toDouble() })), fmt, moneyAxis, ctx.axisUnit),
        months)
}

private fun chosen(accounts: Set<String>, id: String) = accounts.isEmpty() || id in accounts

/** Net worth before the range starts, from the balances just before it. */
fun openingNetWorth(data: ReportData, accounts: List<ReportAccount>): Long {
    val opening = data.openingAccounts ?: return data.months.first().netWorth
    return accounts.filter { it.inNetWorth && it.id in opening }.sumOf { opening[it.id] ?: 0L }
}

fun netWorthView(data: ReportData, ctx: ReportBudget, accounts: Set<String>): NetWorthView {
    val keys = data.months.map { it.month }
    val first = data.months.first()
    val last = data.months.last()
    val shownAccounts = ctx.accounts.filter { chosen(accounts, it.id) }
    val opening = openingNetWorth(data, shownAccounts)
    val change = last.netWorth - opening
    val monthly = data.months.mapIndexed { i, m -> m.netWorth - (if (i > 0) data.months[i - 1].netWorth else opening) }
    val fmt: (Double) -> String = { ctx.money(it) }
    val startDate = Money.date(firstDay(first.month))
    val endDate = Money.date(lastDay(last.month))
    val netWorth = Words[R.string.net_worth]
    val assets = Words[R.string.report_assets]
    val debts = Words[R.string.report_debts]
    val grew = Words[R.string.report_grew]
    val shrank = Words[R.string.report_shrank]
    val rows = shownAccounts.map { a ->
        val start = data.openingAccounts?.get(a.id) ?: first.accounts[a.id] ?: 0L
        val end = last.accounts[a.id] ?: 0L
        FigureRow(a.id, accountTitle(a, outsideNetWorth = true), ctx.money(end - start),
            Words[R.string.change_from_to, ctx.money(start), ctx.money(end)], if (end - start < 0) Tone.Negative else Tone.None)
    } + FigureRow("__networth", netWorth, ctx.money(change), Words[R.string.change_from_to, ctx.money(opening), ctx.money(last.netWorth)], style = RowStyle.Total)
    return NetWorthView(listOf(
        Tile(netWorth, ctx.money(last.netWorth), Words[R.string.report_end_of, reportMonthName(last.month)]),
        Tile(Words[R.string.report_change], ctx.signed(change),
            if (opening > 0) Words[R.string.report_since_began_percent, reportMonthName(first.month), signedPercent(change.toDouble() / opening * 100, 1)]
            else Words[R.string.report_since_began, reportMonthName(first.month)],
            if (change < 0) Tone.Negative else Tone.Positive),
        Tile(assets, ctx.money(last.assets)),
        Tile(debts, ctx.money(last.debt), if (last.assets > 0) Words[R.string.report_of_assets, share(last.debt, last.assets)] else null),
    ), columnModel(Words[R.string.report_chart_net_worth], keys, listOf(
        ColumnSeries("assets", assets, 0, data.months.map { it.assets.toDouble() }),
        ColumnSeries("debts", debts, 1, data.months.map { -it.debt.toDouble() }),
    ), fmt, moneyAxis, ctx.axisUnit, line = LineOverlay(netWorth, ReportColors.INK, data.months.map { it.netWorth.toDouble() })),
        listOf(LegendItem(assets, 0), LegendItem(debts, 1), LegendItem(netWorth, ReportColors.INK, line = true)),
        columnModel(Words[R.string.report_chart_net_worth_change], keys, listOf(
            ColumnSeries("up", grew, ReportColors.DIVERGING_POSITIVE, monthly.map { max(0L, it).toDouble() }),
            ColumnSeries("down", shrank, ReportColors.DIVERGING_NEGATIVE, monthly.map { min(0L, it).toDouble() }),
        ), fmt, moneyAxis, ctx.axisUnit),
        listOf(LegendItem(grew, ReportColors.DIVERGING_POSITIVE), LegendItem(shrank, ReportColors.DIVERGING_NEGATIVE)),
        Words[R.string.report_balances_on, startDate, endDate], rows)
}

/** An account's name in a report row, marked when it's closed (and, where it matters, left out of net worth). */
private fun accountTitle(account: ReportAccount, outsideNetWorth: Boolean = false): String {
    var title = account.name
    if (account.archived) title = Words[R.string.report_account_closed, title]
    if (outsideNetWorth && !account.inNetWorth) title = Words[R.string.report_account_not_in_net_worth, title]
    return title
}

private const val IN_CREDIT = "Accounts in credit"
private const val OVERDRAWN = "Overdrawn accounts"
private val AssetSections = listOf("Cash and bank", "Investments", "Property", "Vehicles", "Receivables", "Other assets", IN_CREDIT)
private val DebtSections = listOf("Credit cards", "Lines of credit", "Mortgages", "Loans", "Other debts", OVERDRAWN)
private val Liquid = setOf("Cash and bank")

/** An account on the balance sheet: its section, its side, and balances as amounts held or owed. */
@Immutable data class SheetLine(val account: ReportAccount, val section: String, val debt: Boolean, val start: Long, val end: Long)

/** The balance decides the side, as it does for net worth: an overdrawn bank account is owed, a card in credit is owned. */
fun balanceSheetLines(data: ReportData, ctx: ReportBudget, accounts: Set<String>): List<SheetLine> {
    val last = data.months.last()
    val opening = data.openingAccounts ?: emptyMap()
    return ctx.accounts.filter { chosen(accounts, it.id) && it.id in last.accounts }.map { a ->
        val end = last.accounts[a.id] ?: 0L
        val start = opening[a.id] ?: 0L
        val type = a.type(end)
        val debt = if (end == 0L) isLiability(type) else end < 0
        val section = if (debt == isLiability(type)) sectionOf(type) else if (debt) OVERDRAWN else IN_CREDIT
        val sign = if (debt) -1 else 1
        SheetLine(a, section, debt, start * sign, end * sign)
    }.filter { it.start != 0L || it.end != 0L || !it.account.archived }
}

fun balanceSheetView(data: ReportData, ctx: ReportBudget, accounts: Set<String>): BalanceSheetView {
    val first = data.months.first()
    val last = data.months.last()
    val lines = balanceSheetLines(data, ctx, accounts)
    val counted = lines.filter { it.account.inNetWorth }
    val apart = lines.filter { !it.account.inNetWorth }
    val assets = counted.filter { !it.debt }
    val debts = counted.filter { it.debt }
    val totalAssets = assets.sumOf { it.end }
    val totalDebts = debts.sumOf { it.end }
    val netStart = assets.sumOf { it.start } - debts.sumOf { it.start }
    val netEnd = totalAssets - totalDebts
    val liquid = assets.filter { it.section in Liquid }.sumOf { it.end }
    val monthlySpending = data.spending.toDouble() / max(1, data.months.size)
    // The list's own colour fills every bar (series-1 for what you own, series-2 for what you owe).
    fun sections(order: List<String>, ls: List<SheetLine>, of: Long): List<BarRowModel> {
        val rows = order.map { s -> Triple(s, sectionLabel(s), ls.filter { it.section == s }.sumOf { it.end }) }.filter { it.third > 0 }
        return barRows(rows, { null }, of, ctx, drill = false)
    }
    fun subtitle(start: Long, end: Long, of: Long?) =
        if (of != null) Words[R.string.report_sheet_subtitle_share, ctx.money(start), ctx.money(end - start), share(end, of)]
        else Words[R.string.report_sheet_subtitle, ctx.money(start), ctx.money(end - start)]
    val netWorth = Words[R.string.net_worth]
    val statement = buildList {
        fun part(key: String, title: String, order: List<String>, ls: List<SheetLine>, totalLabel: String, of: Long) {
            add(FigureRow("h:$key", title, style = RowStyle.Section))
            order.filter { s -> ls.any { it.section == s } }.forEach { s ->
                val inSection = ls.filter { it.section == s }
                val start = inSection.sumOf { it.start }
                val end = inSection.sumOf { it.end }
                add(FigureRow("s:$key:$s", sectionLabel(s), ctx.money(end), subtitle(start, end, of), style = RowStyle.Group))
                inSection.forEach { l ->
                    add(FigureRow("a:${l.account.id}", accountTitle(l.account), ctx.money(l.end),
                        subtitle(l.start, l.end, of), style = RowStyle.Child))
                }
            }
            if (ls.isEmpty()) add(FigureRow("n:$key", Words[R.string.filter_none], style = RowStyle.Muted))
            add(FigureRow("t:$key", totalLabel, ctx.money(ls.sumOf { it.end }), subtitle(ls.sumOf { it.start }, ls.sumOf { it.end }, null), style = RowStyle.Total))
        }
        part("Assets", Words[R.string.report_assets], AssetSections, assets, Words[R.string.report_total_assets], totalAssets)
        part("Liabilities", Words[R.string.report_liabilities_owed], DebtSections, debts, Words[R.string.report_total_liabilities], totalDebts)
        add(FigureRow("__net", netWorth, ctx.money(netEnd), subtitle(netStart, netEnd, null), if (netEnd < 0) Tone.Negative else Tone.None, RowStyle.Net))
        if (apart.isNotEmpty()) {
            add(FigureRow("h:apart", Words[R.string.report_not_counted], style = RowStyle.Section))
            apart.forEach { l -> add(FigureRow("x:${l.account.id}", "${Money.own(l.account.name)} · ${sectionLabel(l.section).lowercase(Locale.ROOT)}", ctx.money(l.end), subtitle(l.start, l.end, null), style = RowStyle.Child)) }
        }
    }
    return BalanceSheetView(listOf(
        Tile(Words[R.string.report_total_assets], ctx.money(totalAssets)),
        Tile(Words[R.string.report_total_liabilities], ctx.money(totalDebts)),
        Tile(netWorth, ctx.money(netEnd), Words[R.string.report_in_this_period, ctx.signed(netEnd - netStart)], if (netEnd < 0) Tone.Negative else Tone.None),
        Tile(Words[R.string.report_debt_to_assets], if (totalAssets > 0) share(totalDebts, totalAssets) else "—",
            Words[if (totalDebts > totalAssets) R.string.report_owe_more_than_own else R.string.report_owned_is_owed]),
        Tile(Words[R.string.report_cash_on_hand], ctx.money(liquid),
            if (monthlySpending > 0) Words[R.string.report_months_of_spending, fixed(liquid / monthlySpending, 1)] else Words[R.string.report_in_cash_accounts]),
    ), ctx.money(totalAssets), if (totalAssets > 0) sections(AssetSections, assets, totalAssets) else emptyList(),
        ctx.money(totalDebts), if (totalDebts > 0) sections(DebtSections, debts, totalDebts) else emptyList(),
        Words[R.string.report_net_worth_on, Money.date(lastDay(last.month)), ctx.money(netEnd)],
        Words[R.string.report_balances_on, Money.date(firstDay(first.month)), Money.date(lastDay(last.month))], statement,
        Words[R.string.report_sheet_help, reportMonthName(first.month), reportMonthName(last.month)] +
            (if (lines.any { it.section == OVERDRAWN || it.section == IN_CREDIT }) " " + Words[R.string.report_sheet_help_sides] else ""))
}

/** Accounts that can be charted: chosen, with balances in the range, closed ones only if they held money. */
fun balanceAccounts(data: ReportData, ctx: ReportBudget, accounts: Set<String>): List<ReportAccount> {
    val last = data.months.last()
    return ctx.accounts.filter { a -> chosen(accounts, a.id) && a.id in last.accounts && (!a.archived || data.months.any { (it.accounts[a.id] ?: 0L) != 0L }) }
}

/** The five largest balances, each with its own colour slot. */
fun defaultChartAccounts(data: ReportData, eligible: List<ReportAccount>): Map<String, Int> {
    val last = data.months.last()
    val biggest = eligible.sortedByDescending { abs(last.accounts[it.id] ?: 0L) }.take(5).map { it.id }.toSet()
    return eligible.filter { it.id in biggest }.mapIndexed { i, a -> a.id to i }.toMap()
}

/** Adds an account to the chart in the first free colour slot (of eight), or takes it off. */
fun toggleChartAccount(chosen: Map<String, Int>, eligible: List<ReportAccount>, id: String): Map<String, Int> {
    if (id in chosen) return chosen - id
    val used = eligible.filter { it.id in chosen }.mapNotNull { chosen[it.id] }.toSet()
    val slot = (0..7).firstOrNull { it !in used } ?: return chosen
    return chosen + (id to slot)
}

fun balancesView(data: ReportData, ctx: ReportBudget, accounts: Set<String>, picked: Map<String, Int>?): BalancesView {
    val keys = data.months.map { it.month }
    val last = data.months.last()
    val eligible = balanceAccounts(data, ctx, accounts)
    val chosen = picked ?: defaultChartAccounts(data, eligible)
    val shown = eligible.filter { it.id in chosen }
    fun values(id: String) = data.months.map { it.accounts[id] ?: 0L }
    data class Stat(val account: ReportAccount, val start: Long, val end: Long, val low: Long, val high: Long)
    val stats = eligible.map { a -> val v = values(a.id); Stat(a, data.openingAccounts?.get(a.id) ?: v.first(), v.last(), v.min(), v.max()) }
    val mover = stats.sortedByDescending { abs(it.end - it.start) }.firstOrNull()
    val fmt: (Double) -> String = { ctx.money(it) }
    val chart = if (shown.isEmpty()) null else linesModel(Words[R.string.report_chart_balances], keys,
        shown.map { a -> LineSeries(a.id, a.name, chosen[a.id] ?: 0, values(a.id).map { it.toDouble() }) }, fmt, moneyAxis, ctx.axisUnit)
    return BalancesView(listOf(
        Tile(Words[R.string.report_on_the_chart], Words[R.string.report_x_of_y, "${shown.size}", "${eligible.size}"], Words[R.string.report_up_to_8]),
        Tile(Words[R.string.report_their_balance_now], ctx.money(shown.sumOf { last.accounts[it.id] ?: 0L })),
        Tile(Words[R.string.report_biggest_change], mover?.account?.name ?: "—", mover?.let { Words[R.string.report_change_since_began, ctx.signed(it.end - it.start), reportMonthName(keys.first())] }),
    ), eligible.map { a -> AccountChip(a.id, a.name, chosen[a.id].takeIf { a.id in chosen }, a.id in chosen || shown.size < 8) }, chosen, chart,
        shown.map { LegendItem(it.name, chosen[it.id] ?: 0, line = true) },
        stats.map { s ->
            FigureRow(s.account.id, accountTitle(s.account), ctx.money(s.end),
                Words[R.string.report_balance_stats, ctx.money(s.start), ctx.money(s.end - s.start), ctx.money(s.low), ctx.money(s.high)],
                if (s.end - s.start < 0) Tone.Negative else Tone.None)
        })
}

fun ageView(data: ReportData): AgeView {
    val keys = data.months.map { it.month }
    val values = keys.map { data.ageOfMoneyByMonth[it] }
    val known = values.filterNotNull()
    val changed = known.size > 1
    val delta = if (changed) known.last() - known.first() else 0L
    val cashAge = Words[R.string.report_cash_age]
    return AgeView(listOf(
        Tile(cashAge, daysText(data.ageOfMoney)),
        Tile(Words[R.string.report_change], if (changed) Words.count(R.plurals.report_days_change, abs(delta), Money.ltr("${if (delta >= 0) "+" else ""}$delta")) else "—",
            if (changed) Words[R.string.report_over_these_months] else null,
            if (changed) (if (known.last() >= known.first()) Tone.Positive else Tone.Negative) else Tone.None),
        Tile(Words[R.string.report_highest], known.maxOrNull()?.let(::daysText) ?: "—"),
        Tile(Words[R.string.report_lowest], known.minOrNull()?.let(::daysText) ?: "—"),
    ), linesModel(Words[R.string.report_chart_cash_age], keys, listOf(LineSeries("line", cashAge, 0, values.map { it?.toDouble() })),
        { Words.count(R.plurals.report_days_value, Math.round(it)) }, { Words[R.string.report_days_axis, jsNumber(it)] }, null),
        keys.mapIndexed { i, m -> FigureRow(m, reportMonthName(m, short = true), values[i]?.toString() ?: "—") })
}

/** Builds a report's body. Forecast has its own data and builder. */
fun reportView(id: String, data: ReportData, ctx: ReportBudget, scope: ReportScope, options: ReportOptions): ReportView? {
    if (data.months.isEmpty()) return null
    return when (id) {
        "categories" -> categoryView(data, ctx, options.group, scope.today)
        "payees" -> payeeView(data, ctx)
        "trends" -> overTimeView(data, ctx, options.byGroup)
        "calendar" -> calendarView(data, ctx, scope)
        "incomeExpense" -> incomeExpenseView(data, ctx, scope.someCategories)
        "income" -> incomeView(data, ctx)
        "cashflow" -> cashFlowView(data, ctx)
        "networth" -> netWorthView(data, ctx, scope.accounts)
        "balance" -> balanceSheetView(data, ctx, scope.accounts)
        "balances" -> balancesView(data, ctx, scope.accounts, options.chartAccounts)
        "age" -> ageView(data)
        else -> null
    }
}

/** A report's own choices: the group drilled into, by group or total, the accounts on the chart. */
@Immutable
data class ReportOptions(val group: String? = null, val byGroup: Boolean = true, val chartAccounts: Map<String, Int>? = null)

// ---------- Overview (the phone's Reports tab) ----------

@Immutable data class SpendRow(val key: String, val label: String, val valueText: String, val color: Int, val weight: Long)

@Immutable
data class OverviewModel(
    val monthName: String, val spendingText: String, val rows: List<SpendRow>, val nothingSpent: Boolean, val refundLead: String?, val refunds: List<SpendRow>,
    val incomeLead: String, val incomeBars: MiniBarsModel, val netWorthText: String, val netWorthNegative: Boolean, val assetsText: String, val debtsText: String,
    val debtsNegative: Boolean, val netWorthBars: MiniBarsModel, val ageText: String, val ageBars: MiniBarsModel,
)

/** The income card's sentence, from the months with any income or spending. */
fun incomeLead(months: List<ReportMonth>): String {
    val active = months.filter { it.income != 0L || it.spending != 0L }
    if (active.isEmpty()) return Words[R.string.report_lead_nothing]
    val income = active.sumOf { it.income }.toDouble() / active.size
    val spending = active.sumOf { it.spending }.toDouble() / active.size
    return when {
        abs(income - spending) < max(income, spending) * 0.02 -> Words[R.string.report_lead_about]
        spending > income -> Words[R.string.report_lead_more]
        else -> Words[R.string.report_lead_less]
    }
}

fun overviewModel(data: ReportData, ctx: ReportBudget): OverviewModel? {
    val last = data.months.lastOrNull() ?: return null
    val top = 5
    val ranked = last.categories.entries.filter { it.value > 0 }.sortedByDescending { it.value }
    val refunds = last.categories.entries.filter { it.value < 0 }.sortedBy { it.value }
    val refundTotal = refunds.sumOf { it.value }
    val refundRows = refunds.take(top).map { SpendRow(it.key, ctx.nameOf[it.key] ?: it.key, ctx.money(it.value), ReportColors.OTHER, -it.value) }.toMutableList()
    val otherRefunds = refunds.drop(top).sumOf { it.value }
    if (otherRefunds != 0L) refundRows.add(SpendRow("__refunds", Words[R.string.report_other_refund_categories], ctx.money(otherRefunds), ReportColors.OTHER, -otherRefunds))
    val unassigned = Words[R.string.category_unassigned]
    val rows = ranked.take(top).mapIndexed { i, e -> SpendRow(e.key, ctx.nameOf[e.key] ?: unassigned, ctx.money(e.value), i, e.value) }.toMutableList()
    val rest = ranked.drop(top).sumOf { it.value }
    if (rest > 0) rows.add(SpendRow("__rest", Words[R.string.report_all_others], ctx.money(rest), ReportColors.OTHER, rest))
    val labels = data.months.map { monthAbbrev(it.month) }
    return OverviewModel(
        reportMonthName(last.month), ctx.money(last.spending), rows, ranked.isEmpty() && refunds.isEmpty(),
        if (refunds.isNotEmpty()) Words[R.string.report_refund_lead, ctx.money(-refundTotal)] + (if (ranked.isNotEmpty()) " " + Words[R.string.report_refund_lead_bar] else "") else null,
        refundRows, incomeLead(data.months),
        MiniBarsModel(labels, listOf(MiniSeries(Words[R.string.report_income], ReportColors.POSITIVE, data.months.map { it.income }), MiniSeries(Words[R.string.report_spending], 0, data.months.map { it.spending }))),
        ctx.money(last.netWorth), last.netWorth < 0, ctx.money(last.assets), ctx.money(-last.debt), last.debt != 0L,
        MiniBarsModel(labels, listOf(MiniSeries(Words[R.string.net_worth], 0, data.months.map { it.netWorth }))),
        daysText(data.ageOfMoney),
        MiniBarsModel(labels, listOf(MiniSeries(Words[R.string.report_cash_age], ReportColors.POSITIVE, data.months.map { data.ageOfMoneyByMonth[it.month] ?: 0L }))),
    )
}

// ---------- Forecast ----------

@Immutable
data class ForecastEvent(val date: String, val amount: Long, val payee: String, val accountId: String, val peerId: String?, val categoryId: String?, val scheduleId: String?)

@Immutable data class ForecastData(val balances: Map<String, Long>, val events: List<ForecastEvent>, val recentSpending: Long)

fun parseForecast(json: JSONObject): ForecastData = ForecastData(
    json.optJSONObject("balances").amounts(),
    json.list("events").map { e ->
        ForecastEvent(e.str("date"), e.optLong("amount"), e.str("payee"), e.str("accountId"), e.str("peerId").ifEmpty { null },
            e.str("categoryId").ifEmpty { null }, e.str("scheduleId").ifEmpty { null })
    },
    json.optLong("recentSpending"),
)

/** The known part of a forecast: chosen accounts, their events and the balance at the end of each day. */
@Immutable
data class ForecastPlan(
    val ids: Set<String>, val events: List<ForecastEvent>, val opening: Long, val scheduledSpending: Double, val suggested: Long,
    val days: List<String>, val knownLine: List<Long>, val balances: List<Long>,
)

fun forecastPlan(data: ForecastData, accounts: Set<String>, months: Int, start: LocalDate, through: LocalDate): ForecastPlan {
    val ids = data.balances.keys.filter { chosen(accounts, it) }.toCollection(LinkedHashSet())
    // Moves between two of the chosen accounts don't change their total.
    val events = data.events.filter { it.accountId in ids && !(it.peerId != null && it.peerId in ids) }
    val opening = ids.sumOf { data.balances[it] ?: 0L }
    val scheduled = events.filter { it.scheduleId != null && it.categoryId != null && it.amount < 0 }.sumOf { -it.amount }.toDouble() / months
    val suggested = max(0L, Math.round(data.recentSpending - scheduled))
    val days = ArrayList<String>()
    var d = start
    while (!d.isAfter(through) && days.size < 2000) { days.add(d.toString()); d = d.plusDays(1) }
    val byDay = HashMap<String, Long>()
    events.forEach { byDay[it.date] = (byDay[it.date] ?: 0L) + it.amount }
    var known = opening
    val knownLine = days.map { known += byDay[it] ?: 0L; known }
    var running = opening
    val balances = events.map { running += it.amount; running }
    return ForecastPlan(ids, events, opening, scheduled, suggested, days, knownLine, balances)
}

/** The known balance less an even amount of everyday spending each day. */
fun forecastLine(knownLine: List<Long>, monthly: Long): List<Long> {
    val perDay = monthly * 12 / 365.0
    return knownLine.mapIndexed { i, v -> Math.round(v - perDay * i) }
}

@Immutable data class ForecastRow(val key: String, val title: String, val detail: String, val amount: String, val amountPositive: Boolean, val balance: String, val balanceNegative: Boolean)

@Immutable
data class ForecastView(
    val tiles: List<Tile>, val chart: LinesModel, val legend: List<LegendItem>, val everyday: Long, val everydayText: String, val suggestion: String?,
    val help: String, val count: String, val rows: List<ForecastRow>,
)

fun forecastView(data: ForecastData, ctx: ReportBudget, accounts: Set<String>, months: Int, everyday: Long?, start: LocalDate): ForecastView {
    val plan = forecastPlan(data, accounts, months, start, start.plusMonths(months.toLong()))
    val monthly = everyday ?: plan.suggested
    val line = forecastLine(plan.knownLine, monthly)
    var low = 0
    line.forEachIndexed { i, v -> if (v < line[low]) low = i }
    val end = line.lastOrNull() ?: plan.opening
    val knownIn = plan.events.filter { it.amount > 0 }.sumOf { it.amount }
    val knownOut = plan.events.filter { it.amount < 0 }.sumOf { -it.amount }
    val categoryName = ctx.categories.associate { it.id to it.name }
    val fmt: (Double) -> String = { ctx.money(it) }
    val forecast = Words[R.string.report_forecast]
    val knownOnly = Words[R.string.report_known_only]
    val chart = linesModel(Words.count(R.plurals.report_chart_forecast, months.toLong()), plan.days, listOf(
        LineSeries("forecast", forecast, 0, line.map { it.toDouble() }),
        LineSeries("known", knownOnly, 1, plan.knownLine.map { it.toDouble() }, dashed = true),
    ), fmt, { compactMoney(it) }, ctx.axisUnit,
        xTick = { d, i -> if (i == 0) plainTick(Words[R.string.report_today]) else if (d.endsWith("-01")) plainTick(monthAbbrev(d)) else null }, title = { Money.date(it) })
    val rows = plan.events.mapIndexed { i, e ->
        val payee = e.payee.ifEmpty { Words[R.string.payee_none] }
        ForecastRow("$i:${e.date}:${e.scheduleId}:${e.accountId}", if (e.scheduleId == null) Words[R.string.report_entered_suffix, payee] else payee,
            Money.joined(listOf(Money.date(e.date), ctx.accountName(e.accountId), if (e.peerId != null) Words[R.string.transfer_to_account, ctx.accountName(e.peerId)] else categoryName[e.categoryId].orEmpty())
                .filter { it.isNotEmpty() }),
            ctx.money(e.amount), e.amount >= 0, ctx.money(plan.balances[i]), plan.balances[i] < 0)
    }
    return ForecastView(listOf(
        Tile(Words[R.string.report_today], ctx.money(plan.opening), Words.count(R.plurals.report_in_budget_accounts, plan.ids.size.toLong())),
        Tile(Words[R.string.report_lowest_point], ctx.money(line.getOrElse(low) { plan.opening }), if (low == 0) Words[R.string.report_today_lower] else Money.date(plan.days[low]),
            if (line.getOrElse(low) { plan.opening } < 0) Tone.Negative else Tone.None),
        Tile(Words.count(R.plurals.report_in_months, months.toLong()), ctx.money(end), Words[R.string.report_from_today, ctx.signed(end - plan.opening)], if (end < 0) Tone.Negative else Tone.None),
        Tile(Words[R.string.report_known_money_in], ctx.money(knownIn), Words[R.string.report_scheduled_or_entered]),
        Tile(Words[R.string.report_known_money_out], ctx.money(knownOut), Words[R.string.report_scheduled_or_entered]),
    ), chart, listOf(LegendItem(forecast, 0, line = true), LegendItem(knownOnly, 1, line = true, dashed = true)),
        monthly, ctx.money(monthly), if (everyday != null && everyday != plan.suggested) Words[R.string.report_use_suggested, ctx.money(plan.suggested)] else null,
        Words[R.string.report_forecast_help, ctx.money(data.recentSpending), ctx.money(Math.round(plan.scheduledSpending)), ctx.money(plan.suggested)],
        Words.count(R.plurals.report_transactions_plural, plan.events.size.toLong()), rows)
}

// ---------- The transactions behind a figure ----------

@Immutable data class DrillLine(val key: String, val transactionId: String, val payee: String, val detail: String, val amount: String, val positive: Boolean)

@Immutable
data class DrillModel(
    val range: String, val figureLabel: String, val figure: String, val count: String, val average: String, val lines: List<DrillLine>,
    /** The saved records the lines open, from the snapshot the figures came from. */
    val records: Map<String, JSONObject>,
)

private data class DrillEntry(val transaction: JSONObject, val index: Int, val date: String, val payee: String, val account: String, val category: String, val amount: Long)

/** The order transactions were added in (date, then time added, then id), newest first as the register lists them. */
private fun newestFirst(entries: List<DrillEntry>): List<DrillEntry> {
    val added = Comparator<DrillEntry> { a, b ->
        val date = a.date.compareTo(b.date)
        if (date != 0) return@Comparator date
        val ac = a.transaction.str("createdAt")
        val bc = b.transaction.str("createdAt")
        if (ac.isNotEmpty() || bc.isNotEmpty()) ac.compareTo(bc) else a.transaction.str("id").compareTo(b.transaction.str("id"))
    }
    return entries.sortedWith(added).reversed()
}

private fun drillRange(from: String, to: String, accounts: Set<String>): String {
    val range = if (from == to) Money.date(from) else Words[R.string.date_range, Money.date(from), Money.date(to)]
    return if (accounts.isNotEmpty()) Words[R.string.report_chosen_accounts_suffix, range] else range
}

private fun drillModel(entries: List<DrillEntry>, ctx: ReportBudget, income: Boolean, from: String, to: String, accounts: Set<String>, spentTotal: Long): DrillModel {
    val ordered = newestFirst(entries)
    val count = entries.map { it.transaction.str("id") }.toSet().size
    val records = LinkedHashMap<String, JSONObject>()
    entries.forEach { records[it.transaction.str("id")] = it.transaction }
    return DrillModel(drillRange(from, to, accounts), Words[if (income) R.string.report_received else R.string.report_spent], ctx.money(spentTotal), count.toString(),
        if (count > 0) ctx.money(Math.round(spentTotal.toDouble() / count)) else "—",
        ordered.map { e ->
            DrillLine("${e.transaction.str("id")}:${e.index}", e.transaction.str("id"), e.payee,
                Money.joined(listOf(Money.date(e.date), e.account, e.category).filter { it.isNotEmpty() }), ctx.money(e.amount), e.amount > 0)
        }, records)
}

/** Lines from the engine's report details: spending is positive there, so lines show it as the register does (negative). */
fun drillFromDetails(items: List<JSONObject>, budget: JSONObject, ctx: ReportBudget, from: String, to: String, accounts: Set<String>): DrillModel {
    val wanted = items.map { it.str("transactionId") }.toSet()
    val records = HashMap<String, JSONObject>()
    budget.optJSONArray("transactions")?.let { a ->
        for (i in 0 until a.length()) { val t = a.optJSONObject(i) ?: continue; val id = t.str("id"); if (id in wanted) records[id] = t }
    }
    val entries = items.mapIndexedNotNull { i, part ->
        val t = records[part.str("transactionId")] ?: return@mapIndexedNotNull null
        val account = part.str("accountId")
        val transferPeer = t.str("transferAccountId").takeIf { it.isNotEmpty() }?.let { peer -> if (account == t.str("accountId")) peer else t.str("accountId") }
        val categoryId = part.str("categoryId")
        DrillEntry(t, i, part.str("date"), if (transferPeer != null) Words[R.string.transfer_to_account, ctx.accountName(transferPeer)] else part.str("payee").ifEmpty { Words[R.string.payee_none] },
            ctx.accountName(account), if (categoryId == UNCATEGORIZED_BUCKET) ctx.needsCategory else ctx.nameOf[categoryId] ?: Words[R.string.category_unassigned], -part.optLong("amount"))
    }
    return drillModel(entries, ctx, false, from, to, accounts, items.sumOf { it.optLong("amount") })
}

private fun leftUnassigned(t: JSONObject, s: JSONObject, splits: Int): Boolean {
    if (s.has("unassigned") && s.opt("unassigned") is Boolean) return s.optBoolean("unassigned")
    val source = s.optJSONObject("source") ?: if (splits == 1) t.optJSONObject("source") else null
    val name = source?.str("category_name")
    return name == "Inflow: Ready to Assign" || name == "Inflow: To be Budgeted" || t.str("payee") == "Reconciliation adjustment"
}

private fun startingBalance(t: JSONObject, splits: List<JSONObject>): Boolean =
    t.optBoolean("startingBalance") && t.str("payee") == "Starting Balance" && t.str("transferAccountId").isEmpty() && splits.isNotEmpty() &&
        splits.all { s -> s.str("categoryId").isEmpty() && !(s.opt("unassigned") == false) && s.str("transferAccountId").isEmpty() }

/**
 * Income from one source: money to Unassigned in budget accounts, transfers across the budget
 * boundary signed from the budget side (the desktop's reportEntries with income set).
 */
fun incomeDrill(budget: JSONObject, ctx: ReportBudget, payee: String, fromMonth: String, toMonth: String, accounts: Set<String>): DrillModel {
    val from = firstDay(fromMonth)
    val to = lastDay(toMonth)
    fun onBudget(id: String) = ctx.accountsById[id]?.onBudget == true
    val entries = ArrayList<DrillEntry>()
    budget.optJSONArray("transactions")?.let { a ->
        for (i in 0 until a.length()) {
            val t = a.optJSONObject(i) ?: continue
            val date = t.str("date")
            if (date < from || date > to) continue
            val splits = t.list("splits")
            val opening = startingBalance(t, splits)
            splits.forEachIndexed { index, s ->
                val other = s.str("transferAccountId").ifEmpty { t.str("transferAccountId") }
                var amount = s.optLong("amount")
                var account = t.str("accountId")
                val effective = s.str("payee").ifEmpty { t.str("payee") }
                var shown = effective.ifEmpty { Words[R.string.payee_none] }
                if (other.isNotEmpty()) {
                    val fromBudget = onBudget(t.str("accountId"))
                    if (fromBudget == onBudget(other)) return@forEachIndexed
                    if (!fromBudget) { amount = -amount; account = other }
                    shown = Words[R.string.transfer_to_account, ctx.accountName(if (fromBudget) other else t.str("accountId"))]
                } else if (!onBudget(t.str("accountId"))) return@forEachIndexed
                if (accounts.isNotEmpty() && account !in accounts) return@forEachIndexed
                if (effective != payee) return@forEachIndexed
                val income = !opening && s.str("categoryId").isEmpty() && (amount >= 0 || leftUnassigned(t, s, splits.size))
                if (!income) return@forEachIndexed
                entries.add(DrillEntry(t, index, date, shown, ctx.accountName(account), Words[R.string.category_unassigned], amount))
            }
        }
    }
    return drillModel(entries, ctx, true, from, to, accounts, entries.sumOf { it.amount })
}
