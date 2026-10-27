package app.urbudget.nativeapp.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.res.stringResource
import app.urbudget.nativeapp.BudgetViewModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import org.json.JSONObject
import java.util.Currency
import java.util.Locale

/** The desktop's number formats (newBudget.ts numberPresets): decimal and thousands separators together. */
private val numberPresets = listOf(
    "1,234.56" to ("." to ","), "1.234,56" to ("," to "."), "1 234,56" to ("," to " "), "1'234.56" to ("." to "'"), "1234.56" to ("." to ""),
)
/** The date formats, each shown by an example date; month names follow the interface's language. */
internal fun dateFormatChoices() = listOf("iso" to "2026-10-15", "dmy" to "15/10/2026", "mdy" to "10/15/2026",
    "dmmmy" to java.time.LocalDate.of(2026, 10, 15).format(Money.pattern("d MMM yyyy")))

private fun symbolOf(code: String) = runCatching { Currency.getInstance(code).getSymbol(Locale.ENGLISH) }.getOrDefault(code)
/** Where a currency's symbol goes unless the budget says otherwise: before a sign ($1.00), after a code (1.00 SAR). */
private fun defaultSymbol(code: String) = if (symbolOf(code) == code) "after" else "before"

/** Every ISO currency the phone knows, "SAR · Saudi Riyal" (named in the interface's language), with `current` kept even if it isn't one of them. */
internal fun currencyChoices(current: String): List<Pair<String, String>> {
    val known = Currency.getAvailableCurrencies().sortedBy { it.currencyCode }.map { it.currencyCode to "${it.currencyCode} · ${it.getDisplayName(Money.interfaceLocale)}" }
    return if (current.isBlank() || known.any { it.first == current }) known else listOf(current to current) + known
}

/**
 * The open budget's name, currency, number and date formats, as the desktop's Settings › Budget
 * edits them. The currency only relabels amounts.
 */
@Composable
internal fun BudgetSettingsDialog(budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    val opened = remember { budget }
    val identity = remember { vm.state.value.budgetIdentity }
    val toast = LocalToast.current
    val saved = budget.optJSONObject("numberFormat")
    var name by remember { mutableStateOf(budget.text("name")) }
    var currency by remember { mutableStateOf(budget.text("currency", "SAR")) }
    var decimal by remember { mutableStateOf(saved?.text("decimal", ".") ?: ".") }
    var group by remember { mutableStateOf(saved?.text("group", ",") ?: ",") }
    var symbol by remember { mutableStateOf(saved?.text("symbol")?.ifBlank { null } ?: defaultSymbol(budget.text("currency", "SAR"))) }
    var dateFormat by remember { mutableStateOf(budget.text("dateFormat", "iso")) }
    val currencies = remember(languageKey()) { currencyChoices(budget.text("currency")) }
    val preset = numberPresets.indexOfFirst { it.second == decimal to group }.coerceAtLeast(0)
    fun example(place: String): String {
        val number = numberPresets[preset].first
        val mark = symbolOf(currency)
        return when (place) { "before" -> if (mark.length == 1) "$mark$number" else "$mark $number"; "after" -> "$number $mark"; else -> number }
    }
    FormDialog(stringResource(R.string.budget_settings), close, enabled = !state.busy && name.isNotBlank(), onConfirm = {
        if (identity != state.budgetIdentity) vm.notifyError(Words[R.string.budget_settings_changed])
        else withCurrentBudget(opened, vm) {
            vm.editBudget {
                it.put("name", name.trim()).put("currency", currency).put("numberFormat", JSONObject().put("decimal", decimal).put("group", group).put("symbol", symbol))
                    .put("dateFormat", dateFormat)
            }
            toast(Words[R.string.budget_settings_saved], true)
            close()
        }
    }) {
        Field(name, { name = it.take(200) }, stringResource(R.string.budget_name))
        Choice(stringResource(R.string.budget_currency), currency, currencies, { next ->
            // A symbol left where the old currency puts it moves to where the new one does.
            if (symbol == defaultSymbol(currency)) symbol = defaultSymbol(next)
            currency = next
        })
        Text(stringResource(R.string.budget_currency_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        Choice(stringResource(R.string.budget_number_format), preset.toString(), numberPresets.mapIndexed { i, p -> i.toString() to Money.ltr(p.first) }, { i ->
            numberPresets.getOrNull(i.toInt())?.second?.let { (d, g) -> decimal = d; group = g }
        })
        Choice(stringResource(R.string.budget_symbol_placement), symbol, listOf("before" to stringResource(R.string.budget_symbol_before, Money.ltr(example("before"))),
            "after" to stringResource(R.string.budget_symbol_after, Money.ltr(example("after"))), "none" to stringResource(R.string.budget_symbol_none, Money.ltr(example("none")))), { symbol = it })
        Choice(stringResource(R.string.budget_date_format), dateFormat, remember(languageKey()) { dateFormatChoices().map { (id, example) -> id to Money.ltr(example) } }, { dateFormat = it })
    }
}

/** Names for the flag colours, such as Reimburse or Tax. */
@Composable
internal fun FlagNamesDialog(budget: JSONObject, vm: BudgetViewModel, close: () -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    val opened = remember { budget }
    val identity = remember { vm.state.value.budgetIdentity }
    var names by remember { mutableStateOf(flagColors.associateWith { budget.optJSONObject("flagNames")?.text(it).orEmpty() }) }
    FormDialog(stringResource(R.string.settings_flag_names), close, enabled = !state.busy, onConfirm = {
        if (identity != state.budgetIdentity) vm.notifyError(Words[R.string.editor_budget_switched])
        else withCurrentBudget(opened, vm) {
            val saved = JSONObject().apply { names.forEach { (color, value) -> value.trim().takeIf(String::isNotEmpty)?.let { put(color, it) } } }
            vm.editBudget { if (saved.length() == 0) it.remove("flagNames") else it.put("flagNames", saved) }
            close()
        }
    }) {
        Text(stringResource(R.string.flag_names_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        flagColors.forEach { color ->
            val label = when (color) {
                "red" -> R.string.flag_field_red; "orange" -> R.string.flag_field_orange; "yellow" -> R.string.flag_field_yellow
                "green" -> R.string.flag_field_green; "blue" -> R.string.flag_field_blue; else -> R.string.flag_field_purple
            }
            Field(names[color].orEmpty(), { names = names + (color to it.take(40)) }, stringResource(label))
        }
    }
}

/** How the open budget writes amounts and dates, for a settings row. */
internal fun formatSummary(budget: JSONObject): String {
    val saved = budget.optJSONObject("numberFormat")
    val number = numberPresets.firstOrNull { it.second == (saved?.text("decimal", ".") ?: ".") to (saved?.text("group", ",") ?: ",") }?.first ?: "1,234.56"
    val date = dateFormatChoices().firstOrNull { it.first == budget.text("dateFormat", "iso") }?.second ?: "2026-10-15"
    return Money.joined(listOf(budget.text("currency"), number, date))
}
