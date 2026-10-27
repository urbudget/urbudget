package app.urbudget.nativeapp

import java.math.BigDecimal
import java.math.RoundingMode
import java.text.NumberFormat
import java.util.Locale
import java.util.Currency
import java.util.concurrent.ConcurrentHashMap
import java.time.format.DateTimeFormatter
import org.json.JSONObject

/** Exact text conversion only; budget arithmetic belongs to Rust. The portable schema uses cents. */
object Money {
    @Volatile private var settings = Format()
    @Volatile private var dateFormat = "iso"
    /** The interface's language, for month and day names; digits stay Western whatever it is. */
    @Volatile private var language: Locale = Locale.ENGLISH
    /** The interface's language, for names that aren't formats (currency names). */
    val interfaceLocale: Locale get() = language
    /** Whether the interface reads right to left, so amounts and dates are kept as left-to-right runs inside it. */
    @Volatile var rightToLeft = false
        private set
    data class Format(val decimal: String = ".", val group: String = ",", val symbol: String = "after") {
        private val grouped = if (group.isNotEmpty() && group != decimal) "|[0-9]{1,3}(?:${Regex.escape(group)}[0-9]{3})+" else ""
        val input = Regex("[+-]?(?:[0-9]+$grouped)(?:${Regex.escape(decimal)}[0-9]{1,2})?")
    }
    private val symbols = ConcurrentHashMap<String, String>()
    private val dates = mapOf("dmy" to "dd/MM/yyyy", "mdy" to "MM/dd/yyyy", "dmmmy" to "d MMM yyyy", "iso" to "yyyy-MM-dd")
    private val formatters = ConcurrentHashMap<Pair<String, Locale>, DateTimeFormatter>()
    private val bidiMarks = Regex("[\u200E\u200F\u2066-\u2069]")
    fun configure(budget: JSONObject) {
        dateFormat = budget.optString("dateFormat", "iso")
        val saved = budget.optJSONObject("numberFormat")
        val currency = budget.optString("currency", "SAR")
        val symbol = currencySymbol(currency)
        settings = Format(saved?.optString("decimal")?.takeIf { it in setOf(".", ",") } ?: ".",
            saved?.optString("group")?.takeIf { it in setOf(",", ".", " ", "'", "") } ?: ",",
            saved?.optString("symbol")?.takeIf { it in setOf("before", "after", "none") } ?: if (symbol == currency) "after" else "before")
    }

    /**
     * The interface's language: month and day names follow it, while the budget's settings keep
     * deciding the order of a date and how amounts are written (docs/LOCALIZATION.md).
     */
    fun useInterface(locale: Locale, rightToLeft: Boolean) {
        language = locale
        this.rightToLeft = rightToLeft
    }

    /**
     * Text that must read left to right (an amount, a date, a signed figure or a sum), isolated as one
     * run when the interface is right to left. Already-isolated pieces inside it join that run.
     */
    fun ltr(text: String): String = if (rightToLeft && text.isNotEmpty()) "\u2066${text.replace("\u2066", "").replace("\u2069", "")}\u2069" else text

    /**
     * Text the person wrote (a name, a memo, an email), isolated in its own direction when the
     * interface is right to left, so "Car (2024)" never comes out as "(Car (2024". Display only.
     */
    fun own(text: String): String = if (rightToLeft && needsIsolating(text)) "$FSI$text$PDI" else text

    /**
     * Whether a right-to-left line needs [text] isolated: it starts left to right, or it is bare
     * figures ("2026-10-15") that an Arabic word before them would turn into "15-10-2026". Arabic
     * text, and lines already made of isolated pieces, read right as they are.
     */
    private fun needsIsolating(text: String): Boolean {
        var depth = 0
        var isolated = false
        for (c in text) {
            when (c) {
                '\u2066', '\u2067', '\u2068' -> { depth++; isolated = true; continue }
                '\u2069' -> { if (depth > 0) depth--; continue }
            }
            if (depth > 0) continue
            when (Character.getDirectionality(c)) {
                Character.DIRECTIONALITY_LEFT_TO_RIGHT -> return true
                Character.DIRECTIONALITY_RIGHT_TO_LEFT, Character.DIRECTIONALITY_RIGHT_TO_LEFT_ARABIC -> return false
            }
        }
        return text.isNotEmpty() && !isolated
    }

    /**
     * Short pieces joined by " · ", each isolated, so a right-to-left line reads them in order even
     * when neighbours are both Latin ("Checking · Groceries") or a name runs into a date.
     */
    fun joined(pieces: Iterable<String>): String = pieces.joinToString(" · ") { own(it) }
    private const val FSI = "\u2068"
    private const val PDI = "\u2069"

    /** A date pattern with the interface's month and day names and Western digits. */
    fun pattern(pattern: String): DateTimeFormatter = formatters.computeIfAbsent(pattern to language) { (p, locale) -> DateTimeFormatter.ofPattern(p, locale) }

    /** A day's name in the interface's language: "Monday", or "Mon" when short. */
    fun weekday(day: java.time.DayOfWeek, short: Boolean = false): String =
        day.getDisplayName(if (short) java.time.format.TextStyle.SHORT else java.time.format.TextStyle.FULL, language)

    private fun currencySymbol(currency: String) = symbols.computeIfAbsent(currency) { runCatching { Currency.getInstance(it).getSymbol(Locale.ENGLISH) }.getOrDefault(it) }
    fun parse(text: String, currency: String = "SAR"): Long {
        val format = settings
        val normalized = text.replace(bidiMarks, "").trim().replace("٫", format.decimal).replace("٬", format.group)
            .map { char -> if (char.isDigit()) Character.digit(char, 10).digitToChar() else char }.joinToString("")
        require(format.input.matches(normalized)) { Words[R.string.money_invalid_amount, format.decimal] }
        val ungrouped = if (format.group.isNotEmpty() && format.group != format.decimal) normalized.replace(format.group, "") else normalized
        val cents = try { BigDecimal(ungrouped.replace(format.decimal, ".")).movePointRight(2).longValueExact() }
            catch (_: ArithmeticException) { throw IllegalArgumentException(Words[R.string.money_too_large]) }
        require(cents in -1_000_000_000_000L..1_000_000_000_000L) { Words[R.string.money_too_large] }
        return cents
    }

    fun format(amount: Long, currency: String = "SAR"): String = ltr(plain(amount, currency, plus = false))

    /** An amount with its sign either way: "+12.00" for money in, "-12.00" for money out. */
    fun signed(amount: Long, currency: String = "SAR"): String = ltr(plain(amount, currency, plus = true))

    private fun plain(amount: Long, currency: String, plus: Boolean): String {
        val format = settings
        // Slice the integer's decimal text: no floating point and no abs(Long.MIN_VALUE).
        val digits = amount.toString().removePrefix("-").padStart(3, '0')
        val whole = digits.dropLast(2)
        val grouped = buildString(whole.length + whole.length / 3) {
            whole.forEachIndexed { index, digit ->
                if (index > 0 && (whole.length - index) % 3 == 0) append(format.group)
                append(digit)
            }
        }
        val number = "$grouped${format.decimal}${digits.takeLast(2)}"
        val sign = if (amount < 0) "-" else if (plus && amount > 0) "+" else ""
        val symbol = currencySymbol(currency)
        return when (format.symbol) {
            "before" -> "$sign$symbol${if (symbol == currency) " " else ""}$number"
            "none" -> "$sign$number"
            else -> "$sign$number $symbol"
        }
    }

    fun editable(amount: Long): String = BigDecimal.valueOf(amount, 2).toPlainString().replace(".", settings.decimal)
    /** The budget's decimal mark, for the keypad's display. */
    fun decimalMark(): String = settings.decimal
    fun date(iso: String): String = runCatching {
        ltr(java.time.LocalDate.parse(iso).format(pattern(dates[dateFormat] ?: dates.getValue("iso"))))
    }.getOrDefault(iso)
}
