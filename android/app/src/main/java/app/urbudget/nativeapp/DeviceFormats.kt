package app.urbudget.nativeapp

import org.json.JSONObject
import java.text.DecimalFormatSymbols
import java.time.chrono.IsoChronology
import java.time.format.DateTimeFormatterBuilder
import java.time.format.FormatStyle
import java.util.Locale

/**
 * The device's way of writing dates and numbers, as a new budget's starting formats: a budget made
 * in the US shows 10/15/2026 and 1,234.56, one made in Germany 15/10/2026 and 1.234,56. The
 * desktop (locale.ts) and iPhone (StarterBudget.localFormats) start a budget the same way; the
 * formats are the budget's from then on, changed in Settings.
 */
internal object DeviceFormats {
    /**
     * The device's own region, whatever language the app is set to: the system's first locale.
     * Where there's no system to ask (a test on the PC), the process's default.
     */
    fun deviceLocale(): Locale =
        runCatching { android.content.res.Resources.getSystem().configuration.locales[0] }.getOrNull() ?: Locale.getDefault()

    /** "mdy", "dmy" or "iso": the order the locale writes a short date in. */
    fun dateFormat(locale: Locale): String {
        val pattern = runCatching { DateTimeFormatterBuilder.getLocalizedDateTimePattern(FormatStyle.SHORT, null, IsoChronology.INSTANCE, locale) }.getOrDefault("")
        return when (pattern.firstOrNull { it in "yMd" }) { 'M' -> "mdy"; 'd' -> "dmy"; else -> "iso" }
    }

    /** The decimal mark and digit grouping, where the budget can show them; "." and "," otherwise. */
    fun separators(locale: Locale): Pair<String, String> {
        val symbols = DecimalFormatSymbols.getInstance(locale)
        val decimal = if (symbols.decimalSeparator == ',') "," else "."
        // Spaces of any width group as a space; a typographic apostrophe as an apostrophe.
        val group = when (symbols.groupingSeparator) {
            ',' -> ","; '.' -> "."; ' ', ' ', ' ' -> " "; '\'', '’' -> "'"; else -> ","
        }
        return decimal to if (group == decimal) (if (decimal == ".") "," else ".") else group
    }

    /** Writes the device's formats into a budget that has none of its own yet. */
    fun applyTo(budget: JSONObject, locale: Locale) {
        if (!budget.has("dateFormat")) budget.put("dateFormat", dateFormat(locale))
        if (!budget.has("numberFormat")) {
            val (decimal, group) = separators(locale)
            budget.put("numberFormat", JSONObject().put("decimal", decimal).put("group", group))
        }
    }
}
