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
    /** A line over columns (net worth, net flow, the surplus): a series colour of its own, never the grid's ink. */
    const val LINE = 6
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
    /** What markets did to net worth this month: the investment accounts' market changes added up. */
    val market: Long = 0,
    /** Money moved into investment accounts this month less what came back, left out of spending and income. */
    val invested: Long = 0,
)

@Immutable
data class PayeeSpend(val name: String, val amount: Long, val count: Long, val category: String? = null)

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
    /** Money moved into investment accounts over the range less what came back, left out of spending and income. */
    val invested: Long = 0,
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
            m.optJSONObject("incomeSources").amounts(), m.optJSONObject("accounts").amounts(), m.optLong("market"), m.optLong("invested"))
    }
    val payees = json.optJSONObject("payees")?.let { o ->
        val names = TreeMap<String, JSONObject>()
        val keys = o.keys()
        while (keys.hasNext()) { val key = keys.next(); o.optJSONObject(key)?.let { names[key] = it } }
        names.map { (name, p) -> PayeeSpend(name, p.optLong("amount"), p.optLong("count"), p.str("category").ifEmpty { null }) }.sortedByDescending { it.amount }
    }.orEmpty()
    val ages = TreeMap<String, Long?>()
    json.optJSONObject("ageOfMoneyByMonth")?.let { o -> val keys = o.keys(); while (keys.hasNext()) { val k = keys.next(); ages[k] = if (o.isNull(k)) null else o.optLong(k) } }
    return ReportData(months, json.optJSONObject("categories").amounts(), json.optJSONObject("incomeSources").amounts(), payees,
        json.optJSONObject("days").amounts(), json.optLong("income"), json.optLong("spending"),
        if (json.has("transactionCount")) json.optLong("transactionCount") else null,
        if (json.isNull("ageOfMoney")) null else json.optLong("ageOfMoney"),
        json.optJSONObject("openingAccounts")?.amounts(), ages, json.optLong("invested"))
}

/**
 * The line under Spending's and Income's filter when money moved into or out of investment
 * accounts over the range: it counts as neither spending nor income, and this says how much was
 * left out. None for the other reports, or when nothing moved.
 */
fun investedNote(view: String, invested: Long, currency: String): String? {
    if (invested == 0L || reportOfView(view)?.id !in setOf("spending", "income")) return null
    return Words[if (invested > 0) R.string.report_invested_in else R.string.report_invested_out, Money.format(abs(invested), currency)]
}

// ---------- The budget, as reports read it ----------

/** Where an investment account goes whole when nothing says what it holds, with the holdings no source gave a kind for. */
private const val OTHER_INVESTMENTS = "Other investments"
/** The kinds of holding with a balance sheet section of their own, by the engine's names for them. */
private val KindSections = listOf("Stocks", "Funds", "Bonds", "Options", "Futures", "Currencies", "Crypto", "Commodities", "CFDs")
private val Sections = mapOf(
    "checking" to "Cash and bank", "savings" to "Cash and bank", "cash" to "Cash and bank",
    "investment" to OTHER_INVESTMENTS, "retirement" to OTHER_INVESTMENTS, "property" to "Property", "vehicle" to "Vehicles",
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
    in KindSections -> HoldingsPage.kindName(section)
    OTHER_INVESTMENTS -> Words[R.string.section_other_investments]
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
    /** The accounts something fills with holdings. */
    val holdings: Set<String> = emptySet(),
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
    return ReportBudget(budget.str("currency"), accounts, categories, groupOf, nameOf, spending.toList(), earliest.take(7), holdingAccountIds(budget))
}


/** The budget's accounts that something fills with holdings (its `investments` entries). */
fun holdingAccountIds(budget: JSONObject): Set<String> {
    val filled = budget.list("investments").mapTo(HashSet()) { it.str("accountId") }
    return budget.list("accounts").map { it.str("id") }.filterTo(LinkedHashSet()) { it.isNotEmpty() && it in filled }
}

// ---------- The six reports ----------

/**
 * One of the six reports, over the views it switches between: the reports there were before, by
 * their ids, the one it opens on first. A report with one view has no switch.
 */
@Immutable data class ReportPlace(val id: String, val views: List<String>)

/** The report about the accounts with holdings: there only once an account has some. */
const val INVESTMENTS_REPORT = "investments"

val ReportPlaces = listOf(
    ReportPlace("spending", listOf("categories", "payees", "trends", "calendar")),
    ReportPlace("income", listOf("incomeExpense", "income", "cashflow")),
    ReportPlace("networth", listOf("networth", "balances")),
    ReportPlace("balance", listOf("balance")),
    ReportPlace(INVESTMENTS_REPORT, listOf("performance", "allocation", "investIncome")),
    ReportPlace("forecast", listOf("forecast", "age")),
)

/** The reports there is something to show for: Investments only once an account has holdings. */
fun shownReports(holdings: Boolean): List<ReportPlace> = ReportPlaces.filter { it.id != INVESTMENTS_REPORT || holdings }

/** The report a view belongs to. */
fun reportOfView(view: String): ReportPlace? = ReportPlaces.firstOrNull { view in it.views }

/**
 * Where an id opens, as a report and its view. A view's id is one of the reports there were, as a
 * place saved before or a link still names it: it opens its report at that view ("income" was the
 * income sources, and still means them). A report's own id opens it at its first view.
 */
fun reportPlace(id: String): Pair<String, String>? =
    reportOfView(id)?.let { it.id to id } ?: ReportPlaces.firstOrNull { it.id == id }?.let { it.id to it.views.first() }

/** The view each report was last on ("spending=payees|income=cashflow"), with [view] as its report's. */
fun rememberView(saved: String, view: String): String {
    val report = reportOfView(view) ?: return saved
    return (lastViews(saved) + (report.id to view)).entries.joinToString("|") { "${it.key}=${it.value}" }
}
private fun lastViews(saved: String): Map<String, String> =
    saved.split('|').mapNotNull { part -> part.split('=').takeIf { it.size == 2 }?.let { it[0] to it[1] } }.toMap()

/** The view a report opens on: the one it was last on, else its first. */
fun viewToOpen(report: String, saved: String): String? {
    val place = ReportPlaces.firstOrNull { it.id == report } ?: return null
    return lastViews(saved)[report]?.takeIf { it in place.views } ?: place.views.first()
}

private val CategoryViews = setOf("categories", "payees", "trends", "calendar", "incomeExpense", "cashflow")
/** Whether a view splits spending by category, so the Categories filter applies to it. */
fun categoriesApply(view: String): Boolean = view in CategoryViews
/** Whether a view covers a range of months: the forecast looks ahead from today, and the allocation is today's holdings. */
fun rangeApplies(view: String): Boolean = view != "forecast" && view != "allocation"

/**
 * The filter's one line, saying what is in force: the range (for a view that has one), the
 * accounts, and the categories only where they are narrowed ("Last 12 months · All accounts · 3 categories").
 */
fun filterLine(range: String?, accounts: String, categories: String?): String = Money.joined(listOfNotNull(range, accounts, categories))

/** The rows a list shows before the rest fold away behind "Show all". */
const val FOLD_ROWS = 6
/** The fewest rows a fold hides: "Show all 7" over one hidden row is more to read than the row. */
const val FOLD_HIDES = 3
/** Whether a list is long enough to fold: nine rows or more, so the fold hides at least three. */
fun folds(count: Int): Boolean = count >= FOLD_ROWS + FOLD_HIDES
/** A list's rows as shown: the first six of a long one until it is opened out. */
fun <T> foldRows(rows: List<T>, open: Boolean): List<T> = if (open || !folds(rows.size)) rows else rows.take(FOLD_ROWS)

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

/**
 * The accounts filter: budget, tracking and closed accounts (only budget accounts for the forecast,
 * only accounts with holdings for the investment reports).
 */
fun accountFilterGroups(ctx: ReportBudget, budgetOnly: Boolean, holdingsOnly: Boolean = false): List<FilterGroup> {
    val accounts = if (holdingsOnly) ctx.accounts.filter { it.id in ctx.holdings } else ctx.accounts
    return listOf(
        Triple("budget", R.string.report_accounts_budget, accounts.filter { !it.archived && it.kind != "tracking" }),
        Triple("tracking", R.string.report_accounts_tracking, accounts.filter { !it.archived && it.kind == "tracking" }),
        Triple("closed", R.string.report_accounts_closed, accounts.filter { it.archived }),
    ).filter { it.third.isNotEmpty() && (!budgetOnly || it.first == "budget") }
        .map { (_, name, items) -> FilterGroup(Words[name], items.map { FilterItem(it.id, it.name) }) }
}

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
    /** The month still under way (drawn lighter, never opened on), or -1. */
    val partial: Int = -1,
    /** How the axis writes a value, to relabel it when series are hidden. */
    val axis: ((Double) -> String)? = null,
) {
    /** The column a chart opens on: the latest, unless it's the month still under way. */
    val opening: Int get() = if (partial == keys.lastIndex && keys.size > 1) keys.lastIndex - 1 else keys.lastIndex
}

@Immutable data class LineSeries(val key: String, val label: String, val color: Int, val values: List<Double?>, val dashed: Boolean = false)

@Immutable
data class LinesModel(
    val label: String, val keys: List<String>, val series: List<LineSeries>, val wash: Boolean, val ticks: List<Double>, val tickLabels: List<String>,
    val unit: String?, val xTicks: List<XTick?>, val titles: List<String>, val readouts: List<List<Readout>>, val endLabels: List<String?>,
    /** The month still under way (its stretch of line dashed), or -1. */
    val partial: Int = -1,
    val axis: ((Double) -> String)? = null,
) {
    val opening: Int get() = if (partial == keys.lastIndex && keys.size > 1) keys.lastIndex - 1 else keys.lastIndex
}

fun monthXTick(month: String): XTick? = runCatching { val m = YearMonth.parse(month); val name = m.format(Money.pattern("MMM")); XTick(name, "$name ${m.year}", m.year) }.getOrNull()
fun monthTitle(month: String): String = reportMonthName(month)
/** A title for the month still under way: "November 2026 so far". */
fun soFar(title: String): String = Words[R.string.report_month_so_far, title]
/** Which of [keys] is this month, still under way, or -1. */
fun partialIndex(keys: List<String>, current: String?): Int = if (current == null) -1 else keys.indexOf(current)
fun plainTick(text: String) = XTick(text, text, 0)

fun columnModel(
    label: String, keys: List<String>, series: List<ColumnSeries>, format: (Double) -> String, axis: (Double) -> String, unit: String?,
    grouped: Boolean = false, line: LineOverlay? = null, average: Pair<String, Double>? = null,
    xTick: (String, Int) -> XTick? = { k, _ -> monthXTick(k) }, title: (String) -> String = ::monthTitle,
    current: String? = YearMonth.now().toString(),
): ColumnModel {
    fun at(s: ColumnSeries, i: Int) = s.values.getOrElse(i) { 0.0 }
    val ticks = columnTicks(keys.size, series, grouped, line, average?.second)
    val partial = partialIndex(keys, current)
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
        ticks.map(axis), unit, keys.mapIndexed { i, k -> xTick(k, i) }, keys.mapIndexed { i, k -> if (i == partial) soFar(title(k)) else title(k) }, readouts,
        partial, axis)
}

/** A column chart's scale: its stacks (or tallest bars, grouped) and line, with zero. */
private fun columnTicks(count: Int, series: List<ColumnSeries>, grouped: Boolean, line: LineOverlay?, average: Double?): List<Double> {
    fun at(s: ColumnSeries, i: Int) = s.values.getOrElse(i) { 0.0 }
    val ups = (0 until count).map { i -> if (grouped) max(0.0, series.maxOfOrNull { at(it, i) } ?: 0.0) else series.sumOf { max(0.0, at(it, i)) } }
    val downs = (0 until count).map { i -> if (grouped) min(0.0, series.minOfOrNull { at(it, i) } ?: 0.0) else series.sumOf { min(0.0, at(it, i)) } }
    val lineValues = line?.values?.filterNotNull().orEmpty()
    return niceTicks(minOf(0.0, downs.minOrNull() ?: 0.0, lineValues.minOrNull() ?: 0.0),
        maxOf(0.0, ups.maxOrNull() ?: 0.0, lineValues.maxOrNull() ?: 0.0, average ?: 0.0))
}

/**
 * The chart with the series named in [hidden] taken out (by their legend label), rescaled to what's
 * left. The total and the average are about every series, so they go too. Hiding everything hides nothing.
 */
fun ColumnModel.without(hidden: Set<String>): ColumnModel {
    if (hidden.isEmpty()) return this
    val kept = series.filter { it.label !in hidden }
    val overlay = line?.takeIf { it.label !in hidden }
    if (kept.isEmpty() && overlay == null) return this
    val ticks = columnTicks(keys.size, kept, grouped, overlay, null)
    return copy(series = kept, line = overlay, average = null, averageText = null,
        ticks = if (axis != null) ticks else this.ticks, tickLabels = axis?.let { ticks.map(it) } ?: tickLabels,
        readouts = readouts.map { row -> row.filter { it.label !in hidden && it.color != null } })
}

fun LinesModel.without(hidden: Set<String>): LinesModel {
    if (hidden.isEmpty()) return this
    val kept = series.indices.filter { series[it].label !in hidden }
    if (kept.isEmpty()) return this
    val known = kept.flatMap { series[it].values.filterNotNull() }
    val ticks = niceTicks(minOf(0.0, known.minOrNull() ?: 0.0), maxOf(1.0, known.maxOrNull() ?: 1.0))
    return copy(series = kept.map { series[it] }, ticks = if (axis != null) ticks else this.ticks, tickLabels = axis?.let { ticks.map(it) } ?: tickLabels,
        readouts = readouts.map { row -> row.filter { it.label !in hidden } }, endLabels = kept.map { endLabels.getOrNull(it) })
}

fun linesModel(
    label: String, keys: List<String>, series: List<LineSeries>, format: (Double) -> String, axis: (Double) -> String, unit: String?,
    xTick: (String, Int) -> XTick? = { k, _ -> monthXTick(k) }, title: (String) -> String = ::monthTitle, wash: Boolean = series.size == 1,
    current: String? = YearMonth.now().toString(),
): LinesModel {
    val partial = partialIndex(keys, current)
    val known = series.flatMap { s -> s.values.filterNotNull() }
    val ticks = niceTicks(minOf(0.0, known.minOrNull() ?: 0.0), maxOf(1.0, known.maxOrNull() ?: 1.0))
    val readouts = keys.indices.map { i ->
        series.mapNotNull { s -> s.values.getOrNull(i)?.let { Readout(s.label, format(it), s.color, line = true, dashed = s.dashed) } }
    }
    val ends = series.map { s -> if (series.size <= 4) s.values.lastOrNull { it != null }?.let(format) else null }
    return LinesModel(label, keys, series, wash, ticks, ticks.map(axis), unit, keys.mapIndexed { i, k -> xTick(k, i) },
        keys.mapIndexed { i, k -> if (i == partial) soFar(title(k)) else title(k) }, readouts, ends, partial, axis)
}

@Immutable data class MiniSeries(val label: String, val color: Int, val values: List<Long>)

@Immutable
data class MiniBarsModel(
    val labels: List<String>, val series: List<MiniSeries>,
    /** Each month's title, and each series' figure that month written out (a card's headline follows a finger with it). */
    val titles: List<String> = emptyList(), val texts: List<List<String>> = emptyList(),
    /** Each series' figure in short, for the labels on the highest and latest columns. */
    val short: List<List<String>> = emptyList(), val partial: Int = -1,
) {
    val top: Long = max(0L, series.flatMap { it.values }.maxOrNull() ?: 0L)
    val bottom: Long = min(0L, series.flatMap { it.values }.minOrNull() ?: 0L)
}

@Immutable data class SliceModel(val key: String, val label: String, val value: Long, val color: Int, val valueText: String = "", val share: String = "")
/** A ranked bar: its name, figure and share; [note] is a line under the name (a currency's own amount). */
@Immutable
data class BarRowModel(val key: String, val label: String, val valueText: String, val detail: String?, val color: Int?, val fraction: Float, val drill: Boolean = false,
                       val note: String? = null)
@Immutable data class DivRowModel(val key: String, val label: String, val valueText: String, val detail: String, val fraction: Float, val color: Int)
@Immutable data class TreemapItem(
    val key: String, val label: String, val value: Long, val valueText: String, val detail: String, val more: String, val level: Int,
    /** The colour of the group most of it went to, as the donut and the month chart colour it. */
    val color: Int = ReportColors.OTHER,
)

/** What a report figure's transactions are: a category, payee, day or income source over a range. */
@Immutable data class DrillTarget(val title: String, val dimension: String, val key: String, val from: String? = null, val to: String? = null)

/** How a row of a report's list reads; a Note is a sentence of small print between the rows, More the "Show all" of the rows folded above it. */
enum class RowStyle { Plain, Section, Group, Child, Total, Net, Muted, Note, More }

/** A row of a report's list: a month, an account, a source. */
@Immutable
data class FigureRow(
    val key: String, val title: String, val value: String = "", val subtitle: String? = null, val tone: Tone = Tone.None,
    val style: RowStyle = RowStyle.Plain, val drill: DrillTarget? = null, val expand: String? = null,
    /** The fold the row is hidden behind until it's opened out; a More row's [expand] names the fold it opens. */
    val fold: String? = null,
)

/** A list's rows as shown: those folded away only once their fold is open. */
fun shownRows(rows: List<FigureRow>, open: (String) -> Boolean): List<FigureRow> = rows.filter { row -> row.fold?.let(open) ?: true }

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
    /** The groups the tiles are coloured by. */
    val legend: List<LegendItem> = emptyList(),
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
    val positiveTotal = rows.filter { it.value > 0 }.sumOf { it.value }
    val slices = rows.filter { it.color != ReportColors.OTHER && it.value > 0 }.map { SliceModel(it.key, it.label, it.value, it.color, ctx.money(it.value), share(it.value, positiveTotal)) } +
        (if (other.isNotEmpty()) other.sumOf { it.value }.let { v -> listOf(SliceModel("__other", Words[R.string.report_other], v, ReportColors.OTHER, ctx.money(v), share(v, positiveTotal))) } else emptyList())
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
    fun groupOf(p: PayeeSpend) = p.category?.let { ctx.groupOf[it] }?.takeIf { it.isNotEmpty() }
    val groups = big.mapNotNull(::groupOf).toSet()
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
                Words.count(R.plurals.report_payee_tile_more, p.count, average(p)), levels.level(p.amount), groupOf(p)?.let(ctx::colorOf) ?: ReportColors.OTHER)
        },
        Words[R.string.report_payees_help] + (if (refunds > 0) " " + Words[R.string.report_payees_help_refunds, ctx.money(refunds)] else ""),
        if (rest.isNotEmpty()) Words.count(R.plurals.report_more_payees, rest.size.toLong(), ctx.money(restTotal)) else null,
        all.map { p ->
            PayeeRow(p.name, p.name, p.name.ifEmpty { noPayee }, ctx.money(p.amount), Words.count(R.plurals.report_payee_row_detail, p.count, share(p.amount, total), average(p)))
        },
        ctx.spendingGroups.filter { it in groups }.map { LegendItem(ctx.groupLabel(it), ctx.colorOf(it)) },
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
    ), fmt, moneyAxis, ctx.axisUnit, grouped = true, line = LineOverlay(surplus, ReportColors.LINE, net.map { it.toDouble() }))
    val legend = listOf(LegendItem(income, 0), LegendItem(spending, 1), LegendItem(surplus, ReportColors.LINE, line = true))
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
    val total = positive.sumOf { it.third }
    val coloured = positive.mapIndexed { i, s -> SliceModel(s.first, s.second, s.third, if (i < 7) i else ReportColors.OTHER, ctx.money(s.third), share(s.third, total)) }
    val rest = coloured.drop(7)
    val folded = if (rest.isNotEmpty()) coloured.take(7) + rest.sumOf { it.value }.let { v -> SliceModel("__other", Words[R.string.report_other_sources], v, ReportColors.OTHER, ctx.money(v), share(v, total)) } else coloured
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
    ), fmt, moneyAxis, ctx.axisUnit, line = LineOverlay(netFlow, ReportColors.LINE, net.map { it.toDouble() }))
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
    ), chart, listOf(LegendItem(moneyIn, 0), LegendItem(moneyOut, 1), LegendItem(netFlow, ReportColors.LINE, line = true)),
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
    // With investments, each month's change is what markets did and everything else (saving, debt paid down).
    val split = data.months.any { it.market != 0L }
    val markets = data.months.sumOf { it.market }
    val marketsName = Words[R.string.report_markets]
    val restName = Words[R.string.report_everything_else]
    val signedMoney: (Double) -> String = { ctx.signed(Math.round(it)) }
    return NetWorthView(listOfNotNull(
        Tile(netWorth, ctx.money(last.netWorth), Words[R.string.report_end_of, reportMonthName(last.month)]),
        Tile(Words[R.string.report_change], ctx.signed(change),
            if (opening > 0) Words[R.string.report_since_began_percent, reportMonthName(first.month), signedPercent(change.toDouble() / opening * 100, 1)]
            else Words[R.string.report_since_began, reportMonthName(first.month)],
            if (change < 0) Tone.Negative else Tone.Positive),
        Tile(assets, ctx.money(last.assets)),
        Tile(debts, ctx.money(last.debt), if (last.assets > 0) Words[R.string.report_of_assets, share(last.debt, last.assets)] else null),
        // A share only where markets and the change went the same way: "-40% of the change" says nothing.
        if (split) Tile(Words[R.string.report_from_markets], ctx.signed(markets),
            if (change != 0L && markets.toDouble() / change >= 0) Words[R.string.report_share_of_change, Money.ltr("${fixed(markets.toDouble() / change * 100, 0)}%")] else null,
            if (markets < 0) Tone.Negative else Tone.Positive) else null,
    ), columnModel(Words[R.string.report_chart_net_worth], keys, listOf(
        ColumnSeries("assets", assets, 0, data.months.map { it.assets.toDouble() }),
        ColumnSeries("debts", debts, 1, data.months.map { -it.debt.toDouble() }),
    ), fmt, moneyAxis, ctx.axisUnit, line = LineOverlay(netWorth, ReportColors.LINE, data.months.map { it.netWorth.toDouble() })),
        listOf(LegendItem(assets, 0), LegendItem(debts, 1), LegendItem(netWorth, ReportColors.LINE, line = true)),
        if (split) columnModel(Words[R.string.report_chart_net_worth_change], keys, listOf(
            ColumnSeries("markets", marketsName, MARKETS_COLOR, data.months.map { it.market.toDouble() }),
            ColumnSeries("rest", restName, 0, data.months.mapIndexed { i, m -> (monthly[i] - m.market).toDouble() }),
        ), signedMoney, moneyAxis, ctx.axisUnit)
        else columnModel(Words[R.string.report_chart_net_worth_change], keys, listOf(
            ColumnSeries("up", grew, ReportColors.DIVERGING_POSITIVE, monthly.map { max(0L, it).toDouble() }),
            ColumnSeries("down", shrank, ReportColors.DIVERGING_NEGATIVE, monthly.map { min(0L, it).toDouble() }),
        ), fmt, moneyAxis, ctx.axisUnit),
        if (split) listOf(LegendItem(marketsName, MARKETS_COLOR), LegendItem(restName, 0))
        else listOf(LegendItem(grew, ReportColors.DIVERGING_POSITIVE), LegendItem(shrank, ReportColors.DIVERGING_NEGATIVE)),
        Words[R.string.report_balances_on, startDate, endDate], rows)
}

/** Markets' series beside "Everything else" in blue: amber, a pair that reads apart without relying on red and green. */
private const val MARKETS_COLOR = 3

/** An account's name in a report row, marked when it's closed (and, where it matters, left out of net worth). */
private fun accountTitle(account: ReportAccount, outsideNetWorth: Boolean = false): String {
    var title = account.name
    if (account.archived) title = Words[R.string.report_account_closed, title]
    if (outsideNetWorth && !account.inNetWorth) title = Words[R.string.report_account_not_in_net_worth, title]
    return title
}

private const val IN_CREDIT = "Accounts in credit"
private const val OVERDRAWN = "Overdrawn accounts"
/** What is owned, by what the money is in: cash, each kind of holding, then the things owned outright. */
private val AssetSections = listOf("Cash and bank") + KindSections + listOf(OTHER_INVESTMENTS, "Property", "Vehicles", "Receivables", "Other assets", IN_CREDIT)
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

/**
 * A line of the statement: an account whole, or one part of what an investment account holds (its
 * cash, a kind of holding, income still to be paid, what its balance has moved since its prices).
 * No [start] where what the account held then isn't kept.
 */
@Immutable
data class SheetPart(
    val key: String, val account: ReportAccount, val section: String, val title: String, val start: Long?, val end: Long,
    /** An account's holdings of one kind, which the holdings themselves can stand in for. */
    val held: Boolean = false,
)

/** The assets by what the money is in, with the accounts whose start isn't known by type and those that can't be broken down. */
@Immutable data class SheetAssets(val parts: List<SheetPart>, val startUnknown: List<ReportAccount>, val whole: List<ReportAccount>)

/** Account names in a sentence, each in its own direction. */
private fun accountNames(accounts: List<ReportAccount>): String = accounts.joinToString(Words[R.string.list_separator]) { Money.own(it.name) }

/**
 * The asset side regrouped by what the money is in. An account with holdings is split into its
 * cash, its kinds of holding and its income still to be paid, from the [portfolio]'s split of its
 * balance at each end of the range; what its ledger balance holds beyond that is its last line, so
 * its lines always add up to the account. Every other account stays whole in its type's section.
 */
fun sheetAssets(lines: List<SheetLine>, portfolio: Portfolio?): SheetAssets {
    val held = portfolio?.accounts.orEmpty().associateBy { it.accountId }
    val parts = ArrayList<SheetPart>()
    val startUnknown = ArrayList<ReportAccount>()
    val whole = ArrayList<ReportAccount>()
    fun amounts(split: AccountParts?, balance: Long): Map<String, Long> {
        val out = LinkedHashMap<String, Long>()
        if (split != null) {
            out["cash"] = split.cash
            split.byKind.forEach { kind -> val key = if (kind.key in KindSections) kind.key else OTHER_INVESTMENTS; out[key] = (out[key] ?: 0L) + kind.value }
            out["accrued"] = split.accrued
        }
        out["rest"] = balance - out.values.sum()
        return out
    }
    for (line in lines) {
        val account = line.account
        val name = accountTitle(account)
        val own = held[account.id]
        val end = own?.endParts
        if (own == null || end == null) {
            // Nothing says what it held on the range's last day: it goes whole, an investment account under Other investments.
            if (own != null) whole.add(account)
            parts.add(SheetPart("a:${account.id}", account, if (own != null) OTHER_INVESTMENTS else line.section, name, line.start, line.end))
            continue
        }
        // An account that held nothing when the range began has nothing to split there.
        val startKnown = own.startParts != null || line.start == 0L
        val atEnd = amounts(end, line.end)
        val atStart = if (startKnown) amounts(own.startParts, line.start) else emptyMap()
        // Nothing says what's in it (a value typed in, say): it is one line, whole, at both ends.
        if (listOf(atEnd, atStart).all { split -> split.all { (key, amount) -> key == "rest" || amount == 0L } }) {
            parts.add(SheetPart("a:${account.id}", account, OTHER_INVESTMENTS, name, line.start, line.end))
            continue
        }
        if (!startKnown) startUnknown.add(account)
        // What's left over is the move since the prices' day, unless nothing at the end says what's in the account at all.
        val bare = atEnd.all { (key, amount) -> key == "rest" || amount == 0L }
        for (key in listOf("cash") + KindSections + listOf(OTHER_INVESTMENTS, "accrued", "rest")) {
            val amount = atEnd[key] ?: 0L
            val began = atStart[key] ?: 0L
            if (amount == 0L && began == 0L) continue
            val (section, title) = when (key) {
                "cash" -> "Cash and bank" to Words[R.string.report_cash_at, Money.own(name)]
                "accrued" -> "Receivables" to Words[R.string.report_accrued_at, Money.own(name)]
                "rest" -> OTHER_INVESTMENTS to (if (bare) name else Money.joined(listOf(name, Words[R.string.report_since_prices, Money.date(end.asOf.ifEmpty { own.asOf })])))
                else -> key to name
            }
            parts.add(SheetPart("a:${account.id}:$key", account, section, title, if (startKnown) began else null, amount, held = key !in setOf("cash", "accrued", "rest")))
        }
    }
    return SheetAssets(parts, startUnknown, whole)
}

fun balanceSheetView(data: ReportData, ctx: ReportBudget, accounts: Set<String>, portfolio: Portfolio? = null, today: LocalDate = LocalDate.now()): BalanceSheetView {
    val first = data.months.first()
    val last = data.months.last()
    val lines = balanceSheetLines(data, ctx, accounts)
    val counted = lines.filter { it.account.inNetWorth }
    val apart = lines.filter { !it.account.inNetWorth }
    val assets = counted.filter { !it.debt }
    val debts = counted.filter { it.debt }
    // The totals come from the accounts, so splitting one never moves them.
    val totalAssets = assets.sumOf { it.end }
    val totalDebts = debts.sumOf { it.end }
    val netStart = assets.sumOf { it.start } - debts.sumOf { it.start }
    val netEnd = totalAssets - totalDebts
    val owned = sheetAssets(assets, portfolio)
    val owed = debts.map { SheetPart("a:${it.account.id}", it.account, it.section, accountTitle(it.account), it.start, it.end) }
    val liquid = owned.parts.filter { it.section in Liquid }.sumOf { it.end }
    val monthlySpending = data.spending.toDouble() / max(1, data.months.size)
    // The list's own colour fills every bar (series-1 for what you own, series-2 for what you owe).
    fun sections(order: List<String>, parts: List<SheetPart>, of: Long): List<BarRowModel> {
        val rows = order.map { s -> Triple(s, sectionLabel(s), parts.filter { it.section == s }.sumOf { it.end }) }.filter { it.third > 0 }
        return barRows(rows, { null }, of, ctx, drill = false)
    }
    // A line whose start isn't known says only its share of what's owned.
    fun subtitle(start: Long?, end: Long, of: Long?): String {
        if (start == null && of != null) return Words[R.string.report_of_assets, share(end, of)]
        val from = start?.let { ctx.money(it) } ?: "—"
        val change = start?.let { ctx.money(end - it) } ?: "—"
        return if (of != null) Words[R.string.report_sheet_subtitle_share, from, change, share(end, of)] else Words[R.string.report_sheet_subtitle, from, change]
    }
    // Ending this month, what's held is today's: each investment section lists the holdings themselves, merged across the accounts.
    val current = portfolio != null && last.month >= YearMonth.from(today).toString()
    val heldParts = owned.parts.filter { it.held }
    val splitAccounts = heldParts.mapTo(HashSet()) { it.account.id }
    val holdingsIn: Map<String, List<MergedHolding>> = if (!current || portfolio == null) emptyMap() else portfolio.holdings
        .filter { h -> h.accounts.isNotEmpty() && h.accounts.all { it in splitAccounts } }
        .groupBy { if (it.group in KindSections) it.group else OTHER_INVESTMENTS }
        // Only where they add up to what the accounts hold of that kind, so a section's lines always come to its figure.
        .filter { (section, list) -> list.sumOf { it.value } == heldParts.filter { it.section == section }.sumOf { it.end } }
    val several = (portfolio?.accounts?.size ?: 0) >= 2
    fun holdingRow(section: String, i: Int, h: MergedHolding, fold: String?): FigureRow {
        val title = h.name.ifEmpty { h.symbol }
        // The section already says a holding of no known kind is among Other investments.
        val what = if (h.group in KindSections) h.instrument.ifEmpty { h.one }.ifEmpty { null }?.let(HoldingsPage::instrumentName) else null
        val line = Money.joined(listOfNotNull(what, HoldingsPage.shownSymbol(h.symbol, title)?.let { Money.ltr(it) },
            HoldingsPage.ownAmount(h.amount, h.currency, ctx.currency), if (several) accountNames(h.accounts, ctx).ifEmpty { null } else null))
        return FigureRow("held:$section:$i:${h.key}", title, ctx.money(h.value), line.ifEmpty { null }, style = RowStyle.Child, fold = fold)
    }
    // Cash at a broker in more than the budget's currency says how much of each it is.
    val cashOf = if (!current || portfolio == null) emptyMap() else portfolio.accounts.associate { it.accountId to it.cash }
    fun partSubtitle(p: SheetPart, of: Long): String {
        val cash = if (p.key.endsWith(":cash")) cashOf[p.account.id].orEmpty() else emptyList()
        val currencies = if (cash.none { !it.currency.trim().equals(ctx.currency, ignoreCase = true) }) null
            else Money.joined(cash.mapNotNull { c -> c.amount?.let { HoldingsPage.amountIn(it, c.currency) } }).ifEmpty { null }
        return listOfNotNull(currencies, subtitle(p.start, p.end, of)).joinToString("\n")
    }
    // How the investments changed, in a sentence: the accounts' change on this statement, less what markets did, is what was put in.
    val invested = assets.mapNotNull { line -> portfolio?.accounts?.firstOrNull { it.accountId == line.account.id }?.let { line to it } }
    val notes = buildList {
        if (invested.isNotEmpty()) {
            val change = invested.sumOf { (line, _) -> line.end - line.start }
            val gain = invested.sumOf { (_, own) -> own.gain }
            add(Words[R.string.report_investments_changed, ctx.signed(change), ctx.signed(change - gain), ctx.signed(gain)])
        }
        if (owned.startUnknown.isNotEmpty()) add(Words[R.string.report_sheet_start_unknown, accountNames(owned.startUnknown)])
        if (owned.whole.isNotEmpty()) add(Words[R.string.report_sheet_unsplit, accountNames(owned.whole)])
    }
    val netWorth = Words[R.string.net_worth]
    val statement = buildList {
        fun part(key: String, title: String, order: List<String>, parts: List<SheetPart>, totalLabel: String, start: Long, end: Long, notes: List<String> = emptyList()) {
            add(FigureRow("h:$key", title, style = RowStyle.Section))
            order.filter { s -> parts.any { it.section == s } }.forEach { s ->
                val inSection = parts.filter { it.section == s }
                // A section with a line whose start isn't known has no start of its own to show.
                val sectionStart = if (inSection.any { it.start == null }) null else inSection.sumOf { it.start ?: 0L }
                val sectionEnd = inSection.sumOf { it.end }
                add(FigureRow("s:$key:$s", sectionLabel(s), ctx.money(sectionEnd), subtitle(sectionStart, sectionEnd, end), style = RowStyle.Group))
                // The holdings stand in for the accounts' lines of their kind; nine or more fold to six.
                val holdings = holdingsIn[s]
                if (holdings != null) {
                    val fold = "sheet:$s".takeIf { folds(holdings.size) }
                    holdings.forEachIndexed { i, h -> add(holdingRow(s, i, h, if (fold != null && i >= FOLD_ROWS) fold else null)) }
                    if (fold != null) add(FigureRow("more:$s", Words[R.string.show_all_count, Money.count(holdings.size.toLong())], style = RowStyle.More, expand = fold))
                }
                inSection.filter { holdings == null || !it.held }.forEach { p -> add(FigureRow(p.key, p.title, ctx.money(p.end), partSubtitle(p, end), style = RowStyle.Child)) }
            }
            if (parts.isEmpty()) add(FigureRow("n:$key", Words[R.string.filter_none], style = RowStyle.Muted))
            add(FigureRow("t:$key", totalLabel, ctx.money(end), subtitle(start, end, null), style = RowStyle.Total))
            notes.forEachIndexed { i, text -> add(FigureRow("note:$key:$i", text, style = RowStyle.Note)) }
        }
        part("Assets", Words[R.string.report_assets], AssetSections, owned.parts, Words[R.string.report_total_assets], assets.sumOf { it.start }, totalAssets, notes)
        part("Liabilities", Words[R.string.report_liabilities_owed], DebtSections, owed, Words[R.string.report_total_liabilities], debts.sumOf { it.start }, totalDebts)
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
    ), ctx.money(totalAssets), if (totalAssets > 0) sections(AssetSections, owned.parts, totalAssets) else emptyList(),
        ctx.money(totalDebts), if (totalDebts > 0) sections(DebtSections, owed, totalDebts) else emptyList(),
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

// ---------- Investments ----------
// The engine's `investmentsReport` (investment_stats.rs `portfolio`): every account with holdings
// together, over the report's range. Nothing is worked out again here; the figures are only laid out.

/** The whole portfolio over the range. A rate that can't be known is null; [yearly] is there for ranges over a year. */
@Immutable
data class PortfolioTotal(
    val start: Long, val end: Long, val put: Long, val gain: Long, val rate: Double?, val yearly: Double?, val days: Long,
    /** Gains taken by selling in the range, where a statement lists its trades; none where no account's does. */
    val realised: Long? = null,
)

/** A month of the range: the accounts' value at its end, what markets did and what was put in, and each account's value. */
@Immutable data class PortfolioMonth(val month: String, val value: Long, val gain: Long, val put: Long, val byAccount: Map<String, Long>, val realised: Long? = null)

@Immutable data class KindAmount(val key: String, val value: Long)

/** An account's balance on a day, split by what it was in: its lines add up to its ledger balance that day. */
@Immutable data class AccountParts(val month: String, val asOf: String, val byKind: List<KindAmount>, val cash: Long, val accrued: Long, val rest: Long)

@Immutable
data class PortfolioAccount(
    val accountId: String, val source: String, val sourceAccount: String, val asOf: String, val joined: String,
    val start: Long, val end: Long, val put: Long, val gain: Long, val rate: Double?, val estimatedDays: Int, val missing: List<String>,
    /** Valued only when it's updated (every source but Interactive Brokers), so a gain lands in the month of the next update. */
    val sparse: Boolean,
    /** The split of its balance just before the range and at its end; none where what it held then isn't kept. */
    val startParts: AccountParts?, val endParts: AccountParts?,
    val realised: Long? = null,
    /** Today's cash by currency. */
    val cash: List<CashAmount> = emptyList(),
)

/** A part of a whole with its share of the positive parts' total. */
/** A part of a whole with its share of the positive parts' total; [amount] is a currency's own amount, where every part of it is known. */
@Immutable data class SharePart(val key: String, val value: Long, val share: Double, val amount: String? = null)

/** An account's cash in one currency: its own amount, and its value in the budget's. */
@Immutable data class CashAmount(val currency: String, val amount: String?, val value: Long)

/** Today's holdings across the accounts, whatever the range. */
@Immutable
data class Allocation(
    val asOf: String, val total: Long, val count: Int, val cash: Long, val accrued: Long, val cost: Long?, val unrealised: Long?, val unrealisedRate: Double?,
    val largestShare: Double?, val topFiveShare: Double?, val byKind: List<SharePart>, val byAccount: List<SharePart>, val byCurrency: List<SharePart>,
)

/** One holding, merged across the accounts that hold it. */
@Immutable
data class MergedHolding(
    val key: String, val symbol: String, val name: String, val group: String, val one: String, val currency: String, val value: Long,
    val cost: Long?, val gain: Long?, val rate: Double?, val share: Double?, val accounts: List<String>,
    /** What it is ("ETF", "Put option"), and what it's worth in its own currency; empty and none until the next sync names them. */
    val instrument: String = "", val amount: String? = null,
)

/** A month's income and costs; [commissions] on trades only where a statement lists its trades by month. */
@Immutable data class IncomeMonth(val month: String, val dividends: Long, val interest: Long, val tax: Long, val fees: Long, val other: Long, val commissions: Long? = null)
/** The dividends one holding paid in the range and the tax withheld from them; [value] is what it's worth today, where still held. */
@Immutable data class HoldingIncome(val key: String, val symbol: String, val name: String, val dividends: Long, val tax: Long, val value: Long?, val yield: Double?)
@Immutable data class Commission(val accountId: String, val from: String, val to: String, val amount: Long)
/**
 * One account's income and costs over the range, as the totals have them. [commissions] are its
 * trades' in the range: by month where its statement lists them, else the statement's one figure, else none.
 */
@Immutable
data class AccountIncome(val accountId: String, val dividends: Long, val interest: Long, val tax: Long, val fees: Long, val other: Long, val commissions: Long? = null)

/** What the accounts earned and were charged: received is positive, paid is negative. */
@Immutable
data class PortfolioIncome(
    val months: List<IncomeMonth>, val dividends: Long, val interest: Long, val tax: Long, val fees: Long, val other: Long, val yield: Double?,
    val commissions: List<Commission>, val reporting: List<String>, val silent: List<String>,
    /** The totals by account: one row for each account that reports income, adding up to them. */
    val byAccount: List<AccountIncome> = emptyList(),
    /** Dividends by holding, largest first, where a statement names the holding that paid. */
    val byHolding: List<HoldingIncome> = emptyList(),
)

@Immutable
data class Portfolio(
    val from: String, val to: String, val total: PortfolioTotal, val months: List<PortfolioMonth>, val accounts: List<PortfolioAccount>,
    val allocation: Allocation, val holdings: List<MergedHolding>, val income: PortfolioIncome,
    /** Open investment accounts nothing fills: they have a balance, and no known gain. */
    val unfilled: List<String>,
)

private fun JSONObject.amount(key: String): Long? = if (isNull(key)) null else optLong(key)
private fun JSONObject.fraction(key: String): Double? = if (isNull(key)) null else optDouble(key).takeIf { !it.isNaN() }
/** A decimal the engine writes as text ("61476.00"), or none. */
private fun JSONObject.decimal(key: String): String? = if (isNull(key)) null else optString(key).trim().ifEmpty { null }
private fun JSONObject.strings(key: String): List<String> = optJSONArray(key)?.let { a -> (0 until a.length()).map { a.optString(it) } }.orEmpty()

fun parsePortfolio(json: JSONObject): Portfolio {
    fun parts(o: JSONObject?) = o?.let { AccountParts(it.str("month"), it.str("asOf"), it.list("byKind").map { k -> KindAmount(k.str("key"), k.optLong("value")) },
        it.optLong("cash"), it.optLong("accrued"), it.optLong("rest")) }
    fun shares(o: JSONObject, key: String) = o.list(key).map { SharePart(it.str("key"), it.optLong("value"), it.fraction("share") ?: 0.0, it.decimal("amount")) }
    val total = json.optJSONObject("total") ?: JSONObject()
    val allocation = json.optJSONObject("allocation") ?: JSONObject()
    val income = json.optJSONObject("income") ?: JSONObject()
    return Portfolio(
        json.str("from"), json.str("to"),
        PortfolioTotal(total.optLong("start"), total.optLong("end"), total.optLong("put"), total.optLong("gain"), total.fraction("rate"), total.fraction("yearly"), total.optLong("days"), total.amount("realised")),
        json.list("months").map { PortfolioMonth(it.str("month"), it.optLong("value"), it.optLong("gain"), it.optLong("put"), it.optJSONObject("byAccount").amounts(), it.amount("realised")) },
        json.list("accounts").map { a ->
            PortfolioAccount(a.str("accountId"), a.str("source"), a.str("sourceAccount"), a.str("asOf"), a.str("joined"), a.optLong("start"), a.optLong("end"), a.optLong("put"),
                a.optLong("gain"), a.fraction("rate"), a.optInt("estimatedDays"), a.strings("missing"), a.optBoolean("sparse"),
                parts(a.optJSONObject("startParts")), parts(a.optJSONObject("endParts")), a.amount("realised"),
                a.list("cash").map { CashAmount(it.str("currency"), it.decimal("amount"), it.optLong("value")) })
        },
        Allocation(allocation.str("asOf"), allocation.optLong("total"), allocation.optInt("count"), allocation.optLong("cash"), allocation.optLong("accrued"),
            allocation.amount("cost"), allocation.amount("unrealised"), allocation.fraction("unrealisedRate"), allocation.fraction("largestShare"), allocation.fraction("topFiveShare"),
            shares(allocation, "byKind"), shares(allocation, "byAccount"), shares(allocation, "byCurrency")),
        json.list("holdings").map { h ->
            MergedHolding(h.str("key"), h.str("symbol"), h.str("name"), h.str("group"), h.str("one"), h.str("currency"), h.optLong("value"),
                h.amount("cost"), h.amount("gain"), h.fraction("rate"), h.fraction("share"), h.strings("accounts"), h.str("instrument").trim(), h.decimal("amount"))
        },
        PortfolioIncome(income.list("months").map { IncomeMonth(it.str("month"), it.optLong("dividends"), it.optLong("interest"), it.optLong("tax"), it.optLong("fees"), it.optLong("other"), it.amount("commissions")) },
            income.optLong("dividends"), income.optLong("interest"), income.optLong("tax"), income.optLong("fees"), income.optLong("other"), income.fraction("yield"),
            income.list("commissions").map { Commission(it.str("accountId"), it.str("from"), it.str("to"), it.optLong("amount")) },
            income.strings("reporting"), income.strings("silent"),
            income.list("byAccount").map { AccountIncome(it.str("accountId"), it.optLong("dividends"), it.optLong("interest"), it.optLong("tax"), it.optLong("fees"), it.optLong("other"), it.amount("commissions")) },
            income.list("byHolding").map { HoldingIncome(it.str("key"), it.str("symbol"), it.str("name"), it.optLong("dividends"), it.optLong("tax"), it.amount("value"), it.fraction("yield")) }),
        json.strings("unfilled"),
    )
}

/**
 * One holding, or one account, as a row: a tile in its colour with a few letters, its name over a
 * line of its own, its value over its gain and return, and a bar of its share. [open] is the
 * account whose Holdings page the row opens.
 */
@Immutable
data class HoldingRowModel(
    val key: String, val name: String, val line: String?, val value: String, val gain: String?, val rate: String?, val sign: Long,
    val share: String?, val bar: Float, val color: Int, val mark: String?, val open: String? = null,
)

/** A note under a report. [setUp] offers the way to keep an account up to date: its id, or empty for the Investments page. */
@Immutable data class ReportNote(val key: String, val text: String, val setUp: String? = null)

@Immutable
data class PerformanceView(
    val tiles: List<Tile>, val value: ColumnModel, val valueLegend: List<LegendItem>, val gains: ColumnModel?, val accounts: List<HoldingRowModel>, val notes: List<ReportNote>,
) : ReportView { override val id get() = "performance" }

@Immutable
data class AllocationView(
    val tiles: List<Tile>, val asOf: String, val by: String, val slices: List<SliceModel>, val centerValue: String, val centerLabel: String, val bars: List<BarRowModel>,
    val holdingsAside: String, val onCost: String?, val holdings: List<HoldingRowModel>, val prices: List<FigureRow>, val notes: List<ReportNote>,
) : ReportView { override val id get() = "allocation" }

@Immutable
data class InvestIncomeView(
    val tiles: List<Tile>, val chart: ColumnModel?, val legend: List<LegendItem>, val dates: String, val lines: List<FigureRow>, val notes: List<ReportNote>,
    /** Each account's part of the totals, when more than one reports income: its row, then its commissions. */
    val accounts: List<FigureRow> = emptyList(),
    /** The dividends each holding paid, largest first, with the tax withheld from them. */
    val holdings: List<FigureRow> = emptyList(),
) : ReportView { override val id get() = "investIncome" }

private fun toned(amount: Long) = if (amount > 0) Tone.Positive else if (amount < 0) Tone.Negative else Tone.None

/** A share or a return as the Holdings page writes it, left to right in an Arabic line too ("<1%" has no letter to say so). */
private fun percentText(share: Double, signed: Boolean = false): String =
    HoldingsPage.percent(share, signed).let { Money.ltr(if (it.startsWith("<")) "‎$it" else it) }

private fun accountNames(ids: List<String>, ctx: ReportBudget): String = ids.joinToString(Words[R.string.list_separator]) { Money.own(ctx.accountName(it)) }

/** Each part's colour by its place in the list, largest first: eight colours, then the grey of "Other". */
private fun partColor(index: Int) = if (index in 0..7) index else ReportColors.OTHER

/**
 * How the investments did over the range: their value month by month under the money put in, each
 * month's gain, and each account's own figures. Nothing when no account with holdings is chosen.
 */
fun performanceView(p: Portfolio, ctx: ReportBudget, today: LocalDate): PerformanceView? {
    if (p.accounts.isEmpty() || p.months.isEmpty()) return null
    val keys = p.months.map { it.month }
    val fmt: (Double) -> String = { ctx.money(it) }
    // The six largest accounts keep a colour each, in the budget's order; the rest share "Other".
    val largest = p.accounts.sortedByDescending { it.end }.take(6).mapTo(HashSet()) { it.accountId }
    val shown = p.accounts.filter { it.accountId in largest }
    val colors = shown.mapIndexed { i, a -> a.accountId to i }.toMap()
    val others = p.accounts.filter { it.accountId !in largest }
    val series = shown.map { a -> ColumnSeries(a.accountId, ctx.accountName(a.accountId), colors[a.accountId] ?: 0, p.months.map { (it.byAccount[a.accountId] ?: 0L).toDouble() }) } +
        (if (others.isEmpty()) emptyList() else listOf(ColumnSeries("__other", Words[R.string.report_other], ReportColors.OTHER,
            p.months.map { m -> others.sumOf { m.byAccount[it.accountId] ?: 0L }.toDouble() })))
    // What was there to begin with plus what was added since: the gap up to the columns is the gain.
    val putIn = Words[R.string.ibkr_money_put_in]
    var put = p.total.start
    val line = LineOverlay(putIn, ReportColors.LINE, p.months.map { put += it.put; put.toDouble() })
    val value = columnModel(Words[R.string.ibkr_value], keys, series, fmt, moneyAxis, ctx.axisUnit, line = line)
    val gains = if (p.months.size < 2) null else columnModel(Words[R.string.reports_gain_each_month], keys, listOf(
        ColumnSeries("gain", Words[R.string.ibkr_gain], ReportColors.POSITIVE, p.months.map { max(0L, it.gain).toDouble() }),
        ColumnSeries("loss", Words[R.string.ibkr_loss], ReportColors.NEGATIVE, p.months.map { min(0L, it.gain).toDouble() }),
        // A month is a gain or a loss, never both: their total would only say it twice.
    ), { ctx.signed(Math.round(it)) }, moneyAxis, ctx.axisUnit).let { model -> model.copy(readouts = model.readouts.map { row -> row.filter { it.color != null } }) }
    val current = p.to >= YearMonth.from(today).toString()
    val stale = p.accounts.filter { current && Investments.stale(it.asOf, today) }
    val tiles = listOf(
        if (current) Tile(Words[R.string.report_value_now], ctx.money(p.total.end), p.allocation.asOf.takeIf { it.isNotEmpty() }?.let { Words[R.string.ibkr_prices_from_date, Money.date(it)] })
        else Tile(Words[R.string.ibkr_value], ctx.money(p.total.end), Words[R.string.report_end_of, reportMonthName(p.to)]),
        // Where a statement lists its trades, the gain's note says how much of it was taken by selling.
        Tile(Words[R.string.ibkr_gain], ctx.signed(p.total.gain),
            p.total.realised?.let { Words[R.string.report_realised, ctx.signed(it)] } ?: Words.count(R.plurals.report_over_months, p.months.size.toLong()), toned(p.total.gain)),
        Tile(Words[R.string.ibkr_return], p.total.rate?.let { percentText(it, true) } ?: "—", p.total.yearly?.let { Words[R.string.report_rate_a_year, percentText(it, true)] },
            p.total.rate?.let { toned(Math.round(it * 1e6)) } ?: Tone.None),
        Tile(putIn, ctx.signed(p.total.put)),
    )
    val rows = p.accounts.map { a ->
        val name = ctx.accountName(a.accountId)
        HoldingRowModel(a.accountId, name,
            Money.joined(listOfNotNull(Investments.sourceName(a.source), "${Words[R.string.report_start]} ${ctx.money(a.start)}", "${Words[R.string.report_put_in]} ${ctx.signed(a.put)}",
                if (a in stale) Words[R.string.ibkr_prices_from_date, Money.date(a.asOf)] else null)),
            ctx.money(a.end), ctx.signed(a.gain), a.rate?.let { percentText(it, true) }, a.gain, null, 0f, colors[a.accountId] ?: ReportColors.OTHER,
            HoldingsPage.mark("", name), open = a.accountId)
    }
    val lacking = p.accounts.flatMap { it.missing }.distinct().mapNotNull(HoldingsPage::lackName)
    val estimated = p.accounts.sumOf { it.estimatedDays }
    val sparse = p.accounts.filter { it.sparse }.map { it.accountId }
    val notes = listOfNotNull(
        if (lacking.isNotEmpty()) ReportNote("lacking", (listOf(Words[R.string.ibkr_lacking]) + lacking.map { "• $it" }).joinToString("\n")) else null,
        if (estimated > 0) ReportNote("estimated", Words.count(R.plurals.ibkr_estimated_days, estimated.toLong())) else null,
        if (stale.isNotEmpty()) ReportNote("stale", Words[R.string.ibkr_stale]) else null,
        if (sparse.isNotEmpty()) ReportNote("sparse", Words[R.string.report_sparse_note, accountNames(sparse, ctx)]) else null,
        if (p.unfilled.isNotEmpty()) ReportNote("unfilled", Words[R.string.report_unfilled_note, accountNames(p.unfilled, ctx)], setUp = p.unfilled.singleOrNull().orEmpty()) else null,
    )
    return PerformanceView(tiles, value, series.map { LegendItem(it.label, it.color) } + LegendItem(putIn, ReportColors.LINE, line = true), gains, rows, notes)
}

/**
 * What the investments are in today, across the accounts: a ring by kind, account or currency
 * ([by]), and the holdings merged across accounts, largest first. Nothing when nothing is held.
 */
fun allocationView(p: Portfolio, ctx: ReportBudget, by: String, today: LocalDate): AllocationView? {
    val all = p.allocation
    if (p.accounts.isEmpty() || (all.total == 0L && p.holdings.isEmpty())) return null
    fun part(key: String, value: Long) = SharePart(key, value, if (all.total > 0) value.toDouble() / all.total else 0.0)
    // What no source gave a kind for is "Other investments" here, as on the Balance sheet.
    fun kindOf(key: String) = if (key == "Holdings" || key == "Other holdings") OTHER_INVESTMENTS else key
    // Cash and income still to be paid are kinds too, as on an account's own page.
    val kinds = (all.byKind + listOfNotNull(
        if (all.cash != 0L && all.byKind.none { it.key == "Cash" }) part("Cash", all.cash) else null,
        if (all.accrued != 0L && all.byKind.none { it.key == "Accrued" }) part("Accrued", all.accrued) else null,
    )).groupBy { kindOf(it.key) }.map { (key, same) -> SharePart(key, same.sumOf { it.value }, same.sumOf { it.share }) }.sortedByDescending { it.value }
    val kindOrder = kinds.map { it.key }
    val (parts, label) = when (by) {
        "account" -> all.byAccount to { key: String -> ctx.accountName(key) }
        "currency" -> all.byCurrency to { key: String -> Money.ltr(key) }
        else -> kinds to { key: String -> if (key == OTHER_INVESTMENTS) sectionLabel(key) else HoldingsPage.kindName(key) }
    }
    val slices = parts.mapIndexedNotNull { i, part ->
        if (part.value > 0) SliceModel(part.key, label(part.key), part.value, partColor(i), ctx.money(part.value), percentText(max(0.0, part.share))) else null
    }
    val most = max(1L, parts.maxOfOrNull { it.value } ?: 1L)
    val bars = parts.mapIndexed { i, part ->
        // A part below zero (money borrowed, an option sold) is listed, and is no share of the whole.
        BarRowModel(part.key, label(part.key), ctx.money(part.value), if (part.value > 0) percentText(part.share) else "—", partColor(i), (max(0L, part.value).toDouble() / most).toFloat(),
            // A currency other than the budget's says how much of it there is.
            note = if (by == "currency") HoldingsPage.ownAmount(part.amount, part.key, ctx.currency) else null)
    }
    val cash = kinds.firstOrNull { it.key == "Cash" }?.value ?: all.cash
    val first = p.holdings.firstOrNull()
    val tiles = listOf(
        Tile(Words[R.string.ibkr_holdings_title], Money.count(all.count.toLong()), ctx.money(all.total)),
        Tile(Words[R.string.ibkr_largest_holding], all.largestShare?.let { percentText(it) } ?: "—", first?.let { Money.own(it.name) }),
        Tile(Words[R.string.report_top_five_holdings], all.topFiveShare?.let { percentText(it) } ?: "—"),
        Tile(Words[R.string.ibkr_cash], if (all.total > 0) percentText(max(0.0, cash.toDouble() / all.total)) else "—", ctx.money(cash)),
    )
    val several = p.accounts.size > 1
    val largest = max(1L, p.holdings.maxOfOrNull { it.value } ?: 1L)
    val holdings = p.holdings.mapIndexed { i, h ->
        val symbol = HoldingsPage.shownSymbol(h.symbol, h.name)
        // What it is, its symbol and what it's worth in its own currency, then which accounts hold it.
        val what = if (kindOf(h.group) == OTHER_INVESTMENTS) sectionLabel(OTHER_INVESTMENTS) else h.instrument.ifEmpty { h.one }.ifEmpty { null }?.let(HoldingsPage::instrumentName)
        HoldingRowModel("$i:${h.key}", h.name,
            Money.joined(listOfNotNull(what, symbol?.let { Money.ltr(it) }, HoldingsPage.ownAmount(h.amount, h.currency, ctx.currency),
                if (several) accountNames(h.accounts, ctx).ifEmpty { null } else null)).ifEmpty { null },
            // A holding below zero (an option sold) is listed, and is no share of the whole.
            ctx.money(h.value), h.gain?.let { ctx.signed(it) }, h.rate?.let { percentText(it, true) }, h.gain ?: 0L, if (h.value > 0) h.share?.let { percentText(it) } else null,
            if (h.value > 0) (h.value.toFloat() / largest).coerceIn(0.02f, 1f) else 0f, partColor(kindOrder.indexOf(kindOf(h.group)).let { if (it < 0) 8 else it }),
            HoldingsPage.mark(h.symbol, h.name))
    }
    val onCost = all.unrealised?.let { gain ->
        Money.joined(listOfNotNull("${Words[R.string.report_unrealised_gain]} ${ctx.signed(gain)}",
            all.unrealisedRate?.let { "${Words[R.string.report_return_on_cost]} ${percentText(it, true)}" }))
    }
    // Each account's prices have their own day; the report is dated by the oldest.
    val stale = p.accounts.filter { Investments.stale(it.asOf, today) }
    val prices = if (p.accounts.map { it.asOf }.distinct().size < 2) emptyList() else p.accounts.map { a ->
        FigureRow(a.accountId, ctx.accountName(a.accountId), Money.date(a.asOf), if (a in stale) Words[R.string.ibkr_stale] else null)
    }
    return AllocationView(tiles, Words[R.string.ibkr_prices_from_date, Money.date(all.asOf)], by, slices, ctx.money(all.total), Words[R.string.ibkr_value], bars,
        Words.count(R.plurals.ibkr_holdings, p.holdings.size.toLong()), onCost, holdings, prices,
        if (stale.isNotEmpty() && prices.isEmpty()) listOf(ReportNote("stale", Words[R.string.ibkr_stale])) else emptyList())
}

/**
 * What the investments earned and cost over the range: dividends and interest against tax, fees
 * and commissions, month by month. Accounts whose source reports none of it are named.
 */
fun investIncomeView(p: Portfolio, ctx: ReportBudget): InvestIncomeView? {
    if (p.accounts.isEmpty()) return null
    val paid = p.income
    val silent = if (paid.silent.isEmpty()) emptyList() else listOf(ReportNote("silent", Words[R.string.report_silent_note, accountNames(paid.silent, ctx)]))
    val dates = if (p.from == p.to) reportMonthName(p.from, short = true) else Words[R.string.date_range, reportMonthName(p.from, short = true), reportMonthName(p.to, short = true)]
    if (paid.reporting.isEmpty()) return InvestIncomeView(emptyList(), null, emptyList(), dates, emptyList(), silent)
    // Commissions come by month where a statement lists its trades, and as one figure a statement for the accounts still listed.
    val byMonth = paid.months.any { it.commissions != null }
    val commissions = paid.months.sumOf { it.commissions ?: 0L } + paid.commissions.sumOf { it.amount }
    val income = paid.dividends + paid.interest
    val costs = paid.tax + paid.fees + commissions
    val net = income + costs + paid.other
    val tiles = listOf(
        Tile(Words[R.string.ibkr_dividends_interest], ctx.signed(income), tone = toned(income)),
        Tile(Words[R.string.ibkr_fees_tax_commissions], ctx.signed(costs), tone = if (costs < 0) Tone.Negative else Tone.None),
        Tile(Words[R.string.report_net], ctx.signed(net), tone = toned(net)),
        Tile(Words[R.string.report_yield], paid.yield?.let { Money.ltr(HoldingsPage.percent(it, true).removePrefix("+")) } ?: "—", Words[R.string.report_yield_note]),
    )
    // What arrived goes up and what was paid goes down, each kind in a colour of its own.
    data class Kind(val name: String, val color: Int, val of: (IncomeMonth) -> Long)
    val kinds = listOf(
        Kind(Words[R.string.ibkr_dividends], 0) { it.dividends }, Kind(Words[R.string.ibkr_interest], 2) { it.interest },
        Kind(Words[R.string.ibkr_tax_withheld], 1) { it.tax }, Kind(Words[R.string.ibkr_fees], 3) { it.fees }, Kind(Words[R.string.ibkr_commissions], 4) { it.commissions ?: 0L },
        Kind(Words[R.string.ibkr_one_other], ReportColors.OTHER) { it.other },
    )
    val series = kinds.mapIndexed { i, kind -> ColumnSeries("k$i", kind.name, kind.color, paid.months.map { kind.of(it).toDouble() }) }.filter { s -> s.values.any { it != 0.0 } }
    val chart = if (series.isEmpty()) null else columnModel(Words[R.string.ibkr_income_costs], paid.months.map { it.month }, series, { ctx.signed(Math.round(it)) }, moneyAxis, ctx.axisUnit)
    val several = paid.reporting.size > 1
    // A statement gives its commissions as one figure, over its own dates.
    fun commissionRow(key: String, c: Commission, account: Boolean, style: RowStyle = RowStyle.Plain) = FigureRow(key, Words[R.string.ibkr_commissions], ctx.signed(c.amount),
        Money.joined(listOfNotNull(if (account) ctx.accountName(c.accountId).ifEmpty { null } else null, Words[R.string.ibkr_from_to, Money.date(c.from), Money.date(c.to)])),
        toned(c.amount), style)
    val charged = paid.commissions.filter { it.amount != 0L }
    // With more than one account reporting, each has a row of its own under the totals, its commissions with it.
    val split = several && paid.byAccount.isNotEmpty()
    val accounts = if (!split) emptyList() else paid.byAccount.flatMap { a ->
        val own = charged.filter { it.accountId == a.accountId }
        val earned = a.dividends + a.interest
        // Its commissions as the engine gives them for the account, so the rows add up to the net; an engine that doesn't leaves its statement's figure.
        val cost = a.tax + a.fees + (a.commissions ?: own.sumOf { it.amount })
        listOf(FigureRow("a:${a.accountId}", ctx.accountName(a.accountId), ctx.signed(earned + cost + a.other),
            Money.joined(listOf("${Words[R.string.ibkr_dividends_interest]} ${ctx.signed(earned)}", "${Words[R.string.ibkr_fees_tax_commissions]} ${ctx.signed(cost)}")),
            toned(earned + cost + a.other))) + own.mapIndexed { i, c -> commissionRow("c:${a.accountId}:$i", c, account = false, style = RowStyle.Child) }
    }
    val lines = listOf(
        Triple("dividends", Words[R.string.ibkr_dividends], paid.dividends), Triple("interest", Words[R.string.ibkr_interest], paid.interest),
        Triple("tax", Words[R.string.ibkr_tax_withheld], paid.tax), Triple("fees", Words[R.string.ibkr_fees], paid.fees), Triple("other", Words[R.string.ibkr_one_other], paid.other),
    ).filter { it.third != 0L }.map { (key, name, amount) ->
        FigureRow(key, name, ctx.signed(amount), if (key == "fees") Words[R.string.ibkr_fees_note] else null, toned(amount))
    } + (if (split || byMonth) listOfNotNull(if (commissions != 0L) FigureRow("commissions", Words[R.string.ibkr_commissions], ctx.signed(commissions), tone = toned(commissions)) else null)
        else charged.mapIndexed { i, c -> commissionRow("commissions:$i", c, account = several) }) +
        FigureRow("__net", Words[R.string.report_net], ctx.signed(net), tone = toned(net), style = RowStyle.Total)
    val holdings = paid.byHolding.mapIndexed { i, h ->
        FigureRow("$i:${h.key}", h.name.ifEmpty { h.symbol }, ctx.signed(h.dividends),
            Money.joined(listOfNotNull(h.symbol.takeIf { it.isNotEmpty() && it != h.name }?.let { Money.ltr(it) },
                if (h.tax != 0L) "${Words[R.string.ibkr_tax_withheld]} ${ctx.signed(h.tax)}" else null)).ifEmpty { null }, toned(h.dividends))
    }
    return InvestIncomeView(tiles, chart, series.map { LegendItem(it.label, it.color) }, dates, lines, silent, accounts, holdings)
}

/** Builds an investment report's body from the portfolio. */
fun investmentsView(id: String, p: Portfolio, ctx: ReportBudget, scope: ReportScope, options: ReportOptions): ReportView? = when (id) {
    "performance" -> performanceView(p, ctx, scope.today)
    "allocation" -> allocationView(p, ctx, options.by, scope.today)
    "investIncome" -> investIncomeView(p, ctx)
    else -> null
}

/** Builds a report's body. Forecast has its own data and builder, and so do the investment reports ([investmentsView]). */
fun reportView(id: String, data: ReportData, ctx: ReportBudget, scope: ReportScope, options: ReportOptions, portfolio: Portfolio? = null): ReportView? {
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
        "balance" -> balanceSheetView(data, ctx, scope.accounts, portfolio, scope.today)
        "balances" -> balancesView(data, ctx, scope.accounts, options.chartAccounts)
        "age" -> ageView(data)
        else -> null
    }
}

/** A report's own choices: the group drilled into, by group or total, the accounts on the chart, what the allocation's ring is by. */
@Immutable
data class ReportOptions(val group: String? = null, val byGroup: Boolean = true, val chartAccounts: Map<String, Int>? = null, val by: String = "kind")

// ---------- Overview (the six cards Reports opens on) ----------

/** A part of a bar split by what it's made of: its name, its figure, its colour and its weight in the bar. */
@Immutable data class SpendRow(val key: String, val label: String, val valueText: String, val color: Int, val weight: Long)

/**
 * A report's card on the overview: one headline figure with a short note, over a small chart of
 * months ([bars]) or one bar split in two ([split]). [report] is the report the card opens.
 */
@Immutable
data class OverviewCard(val report: String, val figure: String, val tone: Tone, val note: String, val bars: MiniBarsModel? = null, val split: List<SpendRow> = emptyList())

/** The overview's cards, in the reports' order: Investments only with holdings, the forecast only when it could be read. */
@Immutable data class OverviewModel(val cards: List<OverviewCard>)

/** The Investments card: what they're worth now, their gain over the months shown, and their value each month. */
fun investmentsCard(p: Portfolio, ctx: ReportBudget): OverviewCard? {
    if (p.accounts.isEmpty() || p.months.isEmpty()) return null
    val keys = p.months.map { it.month }
    val partial = partialIndex(keys, YearMonth.now().toString())
    val values = p.months.map { it.value }
    return OverviewCard(INVESTMENTS_REPORT, ctx.money(p.total.end), Tone.None,
        Money.joined(listOfNotNull("${Words[R.string.ibkr_gain]} ${ctx.signed(p.total.gain)}", p.total.rate?.let { percentText(it, true) })),
        MiniBarsModel(keys.map(::monthAbbrev), listOf(MiniSeries(Words[R.string.ibkr_value], 0, values)),
            keys.mapIndexed { i, m -> if (i == partial) soFar(reportMonthName(m)) else reportMonthName(m) },
            listOf(values.map { ctx.money(it) }), listOf(values.map { compactMoney(it.toDouble()) }), partial))
}

/**
 * The Forecast card, from the forecast the report reads: the lowest the budget accounts' balance
 * gets in the months ahead and when, over the balance at the end of each of those months.
 */
fun forecastCard(data: ForecastData, ctx: ReportBudget, today: LocalDate, months: Int = 6): OverviewCard? {
    if (data.balances.isEmpty()) return null
    val plan = forecastPlan(data, emptySet(), months, today, today.plusMonths(months.toLong()))
    val line = forecastLine(plan.knownLine, plan.suggested)
    if (line.isEmpty()) return null
    var low = 0
    line.forEachIndexed { i, v -> if (v < line[low]) low = i }
    val ends = plan.days.indices.filter { i -> i == plan.days.lastIndex || plan.days[i + 1].take(7) != plan.days[i].take(7) }
    val values = ends.map { line[it] }
    return OverviewCard("forecast", ctx.money(line[low]), if (line[low] < 0) Tone.Negative else Tone.None,
        Money.joined(listOf(Words[R.string.report_lowest_ahead], if (low == 0) Words[R.string.report_today_lower] else Money.date(plan.days[low]))),
        MiniBarsModel(ends.map { monthAbbrev(plan.days[it]) }, listOf(MiniSeries(Words[R.string.report_forecast], 0, values)), ends.map { Money.date(plan.days[it]) },
            listOf(values.map { ctx.money(it) }), listOf(values.map { compactMoney(it.toDouble()) })))
}

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

/**
 * The overview, over the months of [data] (six, ending at the budget's month): a card for each
 * report with its headline figure, a short note and a small chart.
 */
fun overviewModel(data: ReportData, ctx: ReportBudget, portfolio: Portfolio? = null, forecast: ForecastData? = null, today: LocalDate = LocalDate.now()): OverviewModel? {
    val months = data.months
    val last = months.lastOrNull() ?: return null
    val labels = months.map { monthAbbrev(it.month) }
    val partial = partialIndex(months.map { it.month }, YearMonth.now().toString())
    val titles = months.mapIndexed { i, m -> if (i == partial) soFar(reportMonthName(m.month)) else reportMonthName(m.month) }
    fun money(values: List<Long>) = values.map { ctx.money(it) }
    fun bars(name: String, values: List<Long>) =
        MiniBarsModel(labels, listOf(MiniSeries(name, 0, values)), titles, listOf(money(values)), listOf(values.map { compactMoney(it.toDouble()) }), partial)
    val spending = months.map { it.spending }
    val kept = months.sumOf { it.income - it.spending }
    val change = last.netWorth - openingNetWorth(data, ctx.accounts)
    return OverviewModel(listOfNotNull(
        OverviewCard("spending", ctx.money(last.spending), Tone.None,
            Money.joined(listOf(reportMonthName(last.month), "${Words[R.string.report_monthly_average]} ${ctx.money(roundDiv(spending.sum(), months.size))}")),
            bars(Words[R.string.report_spending], spending)),
        OverviewCard("income", ctx.signed(kept), if (kept < 0) Tone.Negative else Tone.None, incomeLead(months),
            MiniBarsModel(labels, listOf(MiniSeries(Words[R.string.report_income], ReportColors.POSITIVE, months.map { it.income }), MiniSeries(Words[R.string.report_spending], 0, spending)),
                titles, listOf(money(months.map { it.income }), money(spending)), emptyList(), partial)),
        OverviewCard("networth", ctx.money(last.netWorth), if (last.netWorth < 0) Tone.Negative else Tone.None, Words[R.string.report_in_this_period, ctx.signed(change)],
            bars(Words[R.string.net_worth], months.map { it.netWorth })),
        // What is owned, with what is owed beside it in one bar.
        OverviewCard("balance", ctx.money(last.assets), Tone.None,
            Money.joined(listOf(Words[R.string.report_total_assets], "${Words[R.string.report_total_liabilities]} ${ctx.money(last.debt)}")),
            split = listOfNotNull(if (last.assets > 0) SpendRow("assets", Words[R.string.report_assets], ctx.money(last.assets), 0, last.assets) else null,
                if (last.debt > 0) SpendRow("debts", Words[R.string.report_debts], ctx.money(last.debt), 1, last.debt) else null)),
        portfolio?.let { investmentsCard(it, ctx) },
        forecast?.let { forecastCard(it, ctx, today) },
    ))
}

// ---------- Forecast ----------

@Immutable
/** [invested]: money moved to or from an investment account, which Reports doesn't count as spending. */
data class ForecastEvent(val date: String, val amount: Long, val payee: String, val accountId: String, val peerId: String?, val categoryId: String?, val scheduleId: String?,
    val invested: Boolean = false)

@Immutable data class ForecastData(val balances: Map<String, Long>, val events: List<ForecastEvent>, val recentSpending: Long)

fun parseForecast(json: JSONObject): ForecastData = ForecastData(
    json.optJSONObject("balances").amounts(),
    json.list("events").map { e ->
        ForecastEvent(e.str("date"), e.optLong("amount"), e.str("payee"), e.str("accountId"), e.str("peerId").ifEmpty { null },
            e.str("categoryId").ifEmpty { null }, e.str("scheduleId").ifEmpty { null }, e.optBoolean("invested"))
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
    val scheduled = events.filter { it.scheduleId != null && it.categoryId != null && it.amount < 0 && !it.invested }.sumOf { -it.amount }.toDouble() / months
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
