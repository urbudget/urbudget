package app.urbudget.nativeapp

import org.json.JSONObject
import java.time.LocalDate
import java.util.Locale
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.pow

/*
 * An investment account's page (the desktop's Holdings view, Investments.tsx). Its figures are
 * the engine's `investmentPage` (crates/urbudget-core/src/investment_stats.rs): how the account
 * did over each period, its ups and downs, each month's gain, what it holds and what it earned
 * and was charged. Here they are only picked apart for the screen: a period's days out of the
 * whole run, the chart's axis and dates, the holdings in the order asked for.
 */
internal object HoldingsPage {
    /** A month, three, since January, a year, or since tracking began: the engine's own names. */
    val PERIODS = listOf("1m", "3m", "ytd", "1y", "all")
    /** Holdings listed before "Show N more holdings". */
    const val FIRST = 10

    private val KINDS = mapOf(
        "Stocks" to R.string.ibkr_kind_stocks, "Funds" to R.string.ibkr_kind_funds, "Bonds" to R.string.ibkr_kind_bonds, "Options" to R.string.ibkr_kind_options,
        "Futures" to R.string.ibkr_kind_futures, "Currencies" to R.string.ibkr_kind_currencies, "Crypto" to R.string.ibkr_kind_crypto,
        "Commodities" to R.string.ibkr_kind_commodities, "CFDs" to R.string.ibkr_kind_cfds, "Cash" to R.string.ibkr_cash, "Accrued" to R.string.ibkr_accrued,
        "Stock" to R.string.ibkr_one_stock, "Fund" to R.string.ibkr_one_fund, "Bond" to R.string.ibkr_one_bond, "Option" to R.string.ibkr_one_option,
        "Future" to R.string.ibkr_one_future, "Currency" to R.string.ibkr_one_currency, "Commodity" to R.string.ibkr_one_commodity, "CFD" to R.string.ibkr_one_cfd,
        "Other" to R.string.ibkr_one_other, "Holdings" to R.string.ibkr_holdings_title,
    )
    /** What the engine calls a kind of holding ("Stocks", "Stock"), in the interface's language. */
    fun kindName(key: String): String = Words[KINDS[key] ?: R.string.ibkr_kind_other]

    private val INSTRUMENTS = mapOf(
        "ETF" to R.string.instrument_etf, "Exchange-traded note" to R.string.instrument_etn, "REIT" to R.string.instrument_reit,
        "Depositary receipt" to R.string.instrument_depositary, "Preferred share" to R.string.instrument_preferred, "Right" to R.string.instrument_right,
        "Closed-end fund" to R.string.instrument_closed_end, "Treasury bill" to R.string.instrument_bill, "Government bond" to R.string.instrument_government_bond,
        "Corporate bond" to R.string.instrument_corporate_bond, "Call option" to R.string.instrument_call, "Put option" to R.string.instrument_put,
    )
    /** What a holding is as the engine names it ("ETF", "Put option", else its kind's singular, "Stock"), in the interface's language. */
    fun instrumentName(key: String): String = INSTRUMENTS[key]?.let { Words[it] } ?: kindName(key)

    /**
     * A holding's instrument, else (a holding brought in before the engine named them) its kind's
     * singular; none for a holding no source said anything about.
     */
    fun instrument(row: JSONObject): String? = text(row, "instrument").ifEmpty { text(row, "one") }.ifEmpty { null }
    private fun text(row: JSONObject, key: String) = if (row.isNull(key)) "" else row.optString(key).trim()

    /**
     * An amount in its own currency, "61,476.00 USD": grouped as the budget writes amounts, with
     * the decimals the statement gave it, and left to right in an Arabic line too.
     */
    fun amountIn(amount: String, currency: String): String {
        val code = currency.trim().uppercase(Locale.ROOT)
        val plain = amount.trim().toBigDecimalOrNull()?.toPlainString() ?: return Money.ltr("${amount.trim()} $code".trim())
        val parts = plain.removePrefix("-").split('.')
        val whole = parts[0].toLongOrNull()?.let(Money::count) ?: parts[0]
        return Money.ltr("${if (plain.startsWith("-")) "-" else ""}$whole${parts.getOrNull(1)?.let { Money.decimalMark() + it } ?: ""} $code".trim())
    }

    /** The same, only where it adds something: none for an amount in the budget's own currency, whose value says it already, or one not known. */
    fun ownAmount(amount: String?, currency: String, budgetCurrency: String): String? =
        if (amount.isNullOrBlank() || currency.isBlank() || currency.trim().equals(budgetCurrency.trim(), ignoreCase = true)) null else amountIn(amount, currency)

    private val LACKS = mapOf(
        "Net Asset Value (NAV) in Base" to R.string.ibkr_lack_nav, "Open Positions" to R.string.ibkr_lack_positions,
        "Cash Transactions" to R.string.ibkr_lack_cash_transactions, "Cash Report" to R.string.ibkr_lack_cash_report, "Transfers" to R.string.ibkr_lack_transfers,
        "Trades" to R.string.ibkr_lack_trades,
    )
    /** What a section the broker's statement left out costs the page, by the name the broker shows; nothing for one the page doesn't use. */
    fun lackName(section: String): String? = LACKS[section]?.let { Words[it] }

    /** A share as a whole percent ("63%"), a sliver as "<1%"; signed for a rate of return ("+5.4%"). */
    fun percent(share: Double, signed: Boolean = false): String {
        if (signed) {
            val tenths = Math.round(share * 1000) / 10.0
            return (if (tenths > 0) "+" else "") + String.format(Locale.ROOT, "%.1f", tenths) + "%"
        }
        val whole = Math.round(share * 100)
        return if (share > 0 && whole == 0L) "<1%" else "$whole%"
    }

    /** A small share to a tenth of a percent ("0.3%"), for the ones a whole percent would round away; a sliver as "<0.1%". */
    fun tenths(share: Double): String {
        val part = kotlin.math.abs(share)
        return if (part > 0 && part < 0.0005) "<0.1%" else percent(part, true).removePrefix("+")
    }

    /** A share or rate the engine may leave out. */
    fun rate(row: JSONObject, key: String): Double? = if (row.isNull(key)) null else row.optDouble(key).takeIf { !it.isNaN() }

    /** A figure the engine may leave out. */
    fun amount(row: JSONObject, key: String): Long? = if (row.isNull(key)) null else row.optLong(key)

    fun period(page: JSONObject, id: String): JSONObject? = page.records("periods").firstOrNull { it.optString("period") == id }

    /** A period's days, the value on each, and the gain so far on each: it starts at nothing. */
    class Series(val days: List<String>, val values: List<Long>, val gains: List<Long>)

    /** A period's days out of the page's whole run: its gains are the run's, less the figure on its first day. */
    fun series(page: JSONObject, period: JSONObject): Series {
        val days = page.optJSONArray("days")
        val values = page.optJSONArray("values")
        val gains = page.optJSONArray("gains")
        val count = minOf(days?.length() ?: 0, values?.length() ?: 0, gains?.length() ?: 0)
        val first = period.optInt("first").coerceIn(0, maxOf(0, count - 1))
        val base = gains?.optLong(first) ?: 0L
        val range = first until count
        return Series(range.map { days!!.optString(it) }, range.map { values!!.optLong(it) }, range.map { gains!!.optLong(it) - base })
    }

    /**
     * What the account would be worth with no gain or loss since the period began: the gap to its
     * value is the gain. Nothing where more was taken out than was put in, which means nothing.
     */
    fun before(series: Series): List<Double?> = series.values.indices.map { i ->
        val value = series.values[i]
        val gain = series.gains[i]
        if (value >= 0 && value < gain) null else (value - gain).toDouble()
    }

    /** What a day under the chart is marked with. */
    enum class Tick { Day, Month, Year }

    /**
     * The chart's dates: a month or two is marked by the week, up to four months every other
     * week, and anything longer by the month, with the year where it turns.
     */
    fun ticks(days: List<String>): List<Tick?> {
        val months = days.map { it.take(7) }.toSet().size
        fun week(day: String) = runCatching { Math.floorDiv(LocalDate.parse(day).toEpochDay() + 3, 7L) }.getOrDefault(0L)
        return days.mapIndexed { i, day ->
            if (i == 0) return@mapIndexed null
            val earlier = days[i - 1]
            if (months <= 4) {
                if (week(day) == week(earlier) || months > 2 && Math.floorMod(week(day), 2L) != 0L) null else Tick.Day
            } else {
                val january = day.substring(5, 7) == "01"
                if (earlier.take(7) == day.take(7) || months > 14 && !january) null else if (january) Tick.Year else Tick.Month
            }
        }
    }

    /**
     * Round marks that cover `min` to `max` without going down to zero: an axis from zero would
     * flatten an account that moves by a hundredth of its size.
     */
    fun fittedTicks(min: Double, max: Double, count: Int = 4): List<Double> {
        if (!(max > min) || min.isNaN() || max.isNaN()) return niceTicks(min, max)
        val raw = (max - min) / count
        val power = 10.0.pow(floor(log10(raw)))
        val step = listOf(1.0, 2.0, 2.5, 5.0, 10.0).map { it * power }.firstOrNull { raw <= it } ?: (10 * power)
        val ticks = ArrayList<Double>()
        var value = floor(min / step) * step
        do {
            ticks.add(Math.round(value * 1e6) / 1e6)
            value += step
        } while (ticks.last() < max && ticks.size < 64)
        return ticks
    }

    /** The holdings by value, gain or return, largest first, or by name; ones without the figure come last. */
    fun sorted(rows: List<JSONObject>, by: String): List<JSONObject> = when (by) {
        "name" -> rows.sortedBy { it.optString("name").lowercase() }
        "gain" -> rows.sortedByDescending { amount(it, "gain") ?: Long.MIN_VALUE }
        "rate" -> rows.sortedByDescending { rate(it, "rate") ?: Double.NEGATIVE_INFINITY }
        else -> rows.sortedByDescending { it.optLong("value") }
    }

    /**
     * The letters on a holding's tile: its symbol's first word ("SPY" of "SPY 18DEC26 710 P", "ARCC"
     * of a bond's "ARCC 5 1/4 04/12/31"), or its name's when it has no symbol.
     */
    fun mark(symbol: String, name: String): String =
        symbol.ifBlank { name }.trim().split(' ', '.', '/').firstOrNull { it.isNotBlank() }.orEmpty().take(6).uppercase(Locale.ROOT)

    /**
     * A holding's symbol where it adds to its title: not when it is the title (a bond's name is its
     * symbol), and not past twelve characters (an option's code only spells its name out again at length).
     */
    fun shownSymbol(symbol: String, title: String): String? = symbol.trim().takeIf { it.isNotEmpty() && it != title && it.length <= 12 }

    /** A holding's own line: its symbol when that adds to its name, then "120.5 × 141.9087 USD". */
    fun line(row: JSONObject): String {
        val shown = shownSymbol(row.optString("symbol"), row.optString("name"))?.let { "$it · " }.orEmpty()
        return "$shown${Investments.plain(row.optString("quantity"))} × ${Investments.plain(row.optString("price"))} ${row.optString("currency")}"
    }
}
