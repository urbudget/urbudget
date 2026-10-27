package app.urbudget.nativeapp.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.json.JSONArray
import org.json.JSONObject
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.BudgetViewModel
import java.time.Instant
import java.time.LocalDate
import java.time.YearMonth
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.Job
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import app.urbudget.nativeapp.ScopedResult
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Words
import androidx.compose.ui.res.stringResource
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight

internal fun JSONObject.rows(key: String): List<JSONObject> = optJSONArray(key)?.let { a ->
    (0 until a.length()).mapNotNull { a.optJSONObject(it) }
}.orEmpty()
internal fun JSONObject.copyJson() = JSONObject(toString())
internal fun JSONObject.text(key: String, fallback: String = "") = if (isNull(key)) fallback else optString(key, fallback)
internal fun JSONObject.row(key: String, id: String) = rows(key).firstOrNull { it.text("id") == id }
internal fun money(value: Long, currency: String) = Money.format(value, currency)
internal fun amountText(value: Long) = Money.editable(value)
internal fun amountValue(value: String) = Money.parse(value)
/** Month names follow the interface's language (Money.pattern), digits stay Western. */
internal fun monthLabel(month: String) = runCatching { YearMonth.parse(month).format(Money.pattern("MMMM yyyy")) }.getOrDefault(month)
/** "Oct 2026", as the phone's month header shows it. */
internal fun shortMonthLabel(month: String) = runCatching { YearMonth.parse(month).format(Money.pattern("MMM yyyy")) }.getOrDefault(month)
/** A month on its own, as the month picker's buttons show it: "Oct". */
internal fun shortMonthName(month: YearMonth): String = month.format(Money.pattern("MMM"))
internal fun jsonArray(values: List<JSONObject>) = JSONArray().apply { values.forEach { put(it) } }
internal val flagColors = listOf("red", "orange", "yellow", "green", "blue", "purple")
/** A flag's default name, until the budget names it. */
internal fun flagDefaultName(color: String): String = when (color) {
    "red" -> Words[R.string.flag_red]; "orange" -> Words[R.string.flag_orange]; "yellow" -> Words[R.string.flag_yellow]
    "green" -> Words[R.string.flag_green]; "blue" -> Words[R.string.flag_blue]; "purple" -> Words[R.string.flag_purple]
    else -> color.replaceFirstChar(Char::uppercase)
}
internal fun flagChoices(budget: JSONObject) = flagColors.map { color ->
    color to (budget.optJSONObject("flagNames")?.text(color)?.takeIf(String::isNotBlank) ?: flagDefaultName(color))
}

/** A form must not apply the full record it opened over changes arriving from sync. */
internal fun requireCurrentBudget(opened: JSONObject, vm: BudgetViewModel) {
    require(opened === vm.state.value.budget) { Words[R.string.error_editor_budget_changed] }
}
internal fun withCurrentBudget(opened: JSONObject, vm: BudgetViewModel, action: () -> Unit) {
    runCatching { requireCurrentBudget(opened, vm); action() }.onFailure { vm.notifyError(it.message ?: Words[R.string.error_reopen_editor]) }
}

/** Stop obsolete preview work when its inputs change or its form closes. */
internal class PreviewJobs {
    private val jobs = mutableListOf<Job>()
    private var closed = false
    fun track(job: Job) { if (closed) job.cancel() else { jobs.removeAll { it.isCompleted }; jobs.add(job) } }
    fun cancel() { closed = true; jobs.forEach { it.cancel() }; jobs.clear() }
}
@Composable
internal fun rememberPreviewJobs(vararg keys: Any?): PreviewJobs {
    val requests = remember(*keys) { PreviewJobs() }
    DisposableEffect(requests) { onDispose(requests::cancel) }
    return requests
}

/** A heading over a group of cards: 20sp bold, with an optional total or button at the end. */
@Composable
internal fun SectionTitle(title: String, subtitle: String? = null, action: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().padding(top = 10.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleLarge, color = ur.text)
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.muted) }
        }
        action?.invoke()
    }
}

@Composable
internal fun MoneyText(value: Long, currency: String, modifier: Modifier = Modifier, large: Boolean = false) {
    Text(money(value, currency), modifier, color = if (value < 0) ur.negative else ur.text,
        style = (if (large) MaterialTheme.typography.headlineMedium else MaterialTheme.typography.titleMedium).merge(Tabular),
        fontWeight = if (large) FontWeight.ExtraBold else FontWeight.SemiBold)
}

/** "Oct 2026 ⌄": tapping opens a sheet of months, as YNAB's month header does. */
@Composable
internal fun MonthSelector(month: String, onChange: (String) -> Unit) {
    var picking by remember { mutableStateOf(false) }
    Row(Modifier.clip(RoundedCornerShape(12.dp)).clickable { picking = true }.padding(horizontal = 10.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Text(shortMonthLabel(month), style = MaterialTheme.typography.titleLarge, color = ur.text)
        Icon(Icons.Outlined.KeyboardArrowDown, stringResource(R.string.month_choose), Modifier.padding(start = 2.dp), tint = ur.text)
    }
    if (picking) MonthPickerSheet(month, { onChange(it); picking = false }) { picking = false }
}

@Composable
internal fun MonthPickerSheet(month: String, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val current = remember(month) { runCatching { YearMonth.parse(month) }.getOrDefault(YearMonth.now()) }
    var year by remember(month) { mutableStateOf(current.year) }
    UrSheet(onDismiss) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton({ year-- }) { Icon(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, stringResource(R.string.month_previous_year), tint = ur.text) }
            Text("$year", Modifier.weight(1f), style = MaterialTheme.typography.titleLarge, color = ur.text, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
            IconButton({ year++ }) { Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, stringResource(R.string.month_next_year), tint = ur.text) }
        }
        val now = YearMonth.now()
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            (1..12).chunked(4).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    row.forEach { m ->
                        val value = YearMonth.of(year, m)
                        val chosen = value == current
                        Box(Modifier.weight(1f).height(48.dp).clip(RoundedCornerShape(14.dp))
                            .background(if (chosen) ur.accent else if (value == now) ur.accentSoft else ur.surface2)
                            .clickable { onPick(value.toString()) }, contentAlignment = Alignment.Center) {
                            Text(shortMonthName(value), fontWeight = FontWeight.SemiBold,
                                color = if (chosen) MaterialTheme.colorScheme.onPrimary else if (value == now) ur.accent else ur.text)
                        }
                    }
                }
            }
            if (current != now) TextButton({ onPick(now.toString()) }, Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.month_go_to_current)) }
        }
    }
}

/**
 * A form rises from the bottom as a sheet over the dimmed page, with Cancel and its action in a
 * header so they stay in reach above the keyboard.
 */
@Composable
internal fun FormDialog(title: String, onDismiss: () -> Unit, confirm: String = stringResource(R.string.common_save), onConfirm: (() -> Unit)? = null,
                        enabled: Boolean = true, content: @Composable ColumnScope.() -> Unit) {
    UrSheet(onDismiss) {
        SheetHeader(title, onDismiss, if (onConfirm == null) null else confirm, onConfirm, enabled)
        Column(Modifier.fillMaxWidth().weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 18.dp).padding(bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp), content = content)
    }
}

/** Fixed viewport keeps arbitrarily long editors virtualized without nested scrolling. */
@Composable
internal fun LazyFormDialog(title: String, onDismiss: () -> Unit, confirm: String = stringResource(R.string.common_save), onConfirm: (() -> Unit)? = null,
                            enabled: Boolean = true, content: LazyListScope.() -> Unit) {
    val height = screenHeight()
    UrSheet(onDismiss) {
        SheetHeader(title, onDismiss, if (onConfirm == null) null else confirm, onConfirm, enabled)
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = height * .8f).testTag("lazy-form:$title"),
            contentPadding = PaddingValues(start = 18.dp, end = 18.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp), content = content)
    }
}

/** `ltr`: what is typed reads left to right whatever the interface's direction (a date or an amount the person writes out). */
@Composable
internal fun Field(value: String, onChange: (String) -> Unit, label: String, numeric: Boolean = false, lines: Int = 1, ltr: Boolean = false) {
    val style = if (numeric) LocalTextStyle.current.merge(Tabular) else LocalTextStyle.current
    TextField(value, onChange, label = { Text(label) }, modifier = Modifier.fillMaxWidth(), singleLine = lines == 1, maxLines = lines,
        minLines = if (lines > 1) 2 else 1, shape = RoundedCornerShape(12.dp), colors = fieldColors(),
        textStyle = if (ltr) style.copy(textDirection = androidx.compose.ui.text.style.TextDirection.Ltr) else style,
        keyboardOptions = KeyboardOptions(keyboardType = if (numeric) KeyboardType.Decimal else KeyboardType.Text))
}

/** A free text field in the app's filled style, for secrets, chat and other inputs Field doesn't cover. */
@Composable
internal fun FilledField(value: String, onValueChange: (String) -> Unit, modifier: Modifier = Modifier, label: (@Composable () -> Unit)? = null,
                         visualTransformation: androidx.compose.ui.text.input.VisualTransformation = androidx.compose.ui.text.input.VisualTransformation.None,
                         singleLine: Boolean = false, maxLines: Int = if (singleLine) 1 else Int.MAX_VALUE) {
    TextField(value, onValueChange, modifier, label = label, visualTransformation = visualTransformation, singleLine = singleLine, maxLines = maxLines,
        shape = RoundedCornerShape(12.dp), colors = fieldColors())
}

@Composable
internal fun fieldColors() = TextFieldDefaults.colors(
    focusedContainerColor = fieldFill(), unfocusedContainerColor = fieldFill(), disabledContainerColor = fieldFill(),
    focusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent, unfocusedIndicatorColor = androidx.compose.ui.graphics.Color.Transparent,
    disabledIndicatorColor = androidx.compose.ui.graphics.Color.Transparent, focusedLabelColor = ur.accent, unfocusedLabelColor = ur.muted,
    cursorColor = ur.accent, focusedTextColor = ur.text, unfocusedTextColor = ur.text,
)

/** A labelled value that opens its options: a menu by the field, or a searchable sheet for long lists. */
@Composable
internal fun Choice(label: String, selected: String, choices: List<Pair<String, String>>, onChange: (String) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    val choose = stringResource(R.string.common_choose)
    val chosen = remember(choices, selected, choose) { choices.firstOrNull { it.first == selected }?.second ?: choose }
    Box {
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(fieldFill()).clickable { expanded = true }
            .testTag("choice-open:$label").padding(start = 16.dp, end = 10.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(label, style = MaterialTheme.typography.bodySmall, color = ur.muted)
                Text(Money.own(chosen), style = MaterialTheme.typography.bodyLarge, color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Icon(Icons.Outlined.UnfoldMore, null, tint = ur.muted)
        }
        DropdownMenu(expanded && choices.size <= 20, { expanded = false }, modifier = Modifier.heightIn(max = 360.dp),
            shape = RoundedCornerShape(14.dp), containerColor = ur.surface) {
            choices.forEach { (id, name) ->
                DropdownMenuItem(text = { Text(Money.own(name), fontWeight = if (id == selected) FontWeight.SemiBold else FontWeight.Normal) },
                    trailingIcon = if (id == selected) ({ Icon(Icons.Outlined.Check, null, tint = ur.accent) }) else null,
                    onClick = { onChange(id); expanded = false })
            }
        }
    }
    if (expanded && choices.size > 20) {
        var search by remember { mutableStateOf("") }
        val request = remember(choices, search) { Any() }
        val filtered by produceState<ScopedResult<List<Pair<String, String>>>?>(null, request) {
            value = withContext(Dispatchers.Default) { ScopedResult(request, filterChoices(choices, search)) }
        }
        val visible = filtered?.forScope(request)
        val height = screenHeight()
        UrSheet({ expanded = false }) {
            SheetHeader(label, { expanded = false }, null, null)
            SearchField(search, { search = it }, stringResource(R.string.common_search), Modifier.padding(horizontal = 16.dp), Modifier.testTag("choice-search:$label"))
            if (visible == null) LinearProgressIndicator(Modifier.fillMaxWidth().padding(16.dp))
            LazyColumn(Modifier.fillMaxWidth().height(height * .6f).testTag("choice-options:$label"), contentPadding = PaddingValues(vertical = 8.dp)) {
                itemsIndexed(visible.orEmpty()) { _, option ->
                    Row(Modifier.fillMaxWidth().clickable { onChange(option.first); expanded = false }.padding(horizontal = 20.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically) {
                        Text(Money.own(option.second), Modifier.weight(1f), color = ur.text, fontWeight = if (option.first == selected) FontWeight.SemiBold else FontWeight.Normal)
                        if (option.first == selected) Icon(Icons.Outlined.Check, null, tint = ur.accent)
                    }
                }
                if (visible?.isEmpty() == true) item { Text(stringResource(R.string.common_no_matching_options), Modifier.padding(20.dp), color = ur.muted) }
            }
        }
    }
}

internal suspend fun filterChoices(choices: List<Pair<String, String>>, search: String): List<Pair<String, String>> {
    val context = currentCoroutineContext()
    context.ensureActive()
    if (search.isBlank()) return choices
    val result = ArrayList<Pair<String, String>>()
    choices.forEachIndexed { index, option ->
        if (index % 128 == 0) context.ensureActive()
        if (option.second.contains(search, ignoreCase = true)) result.add(option)
    }
    return result
}

/** A date row that opens the Material date picker. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DateField(value: String, onChange: (String) -> Unit, label: String = stringResource(R.string.common_date)) {
    var picking by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(fieldFill()).clickable { picking = true }
        .padding(start = 16.dp, end = 12.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodySmall, color = ur.muted)
            Text(Money.date(value), style = MaterialTheme.typography.bodyLarge, color = ur.text)
        }
        Icon(Icons.Outlined.CalendarToday, null, tint = ur.muted)
    }
    if (picking) DatePickerSheet(value, { onChange(it); picking = false }) { picking = false }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DatePickerSheet(value: String, onPick: (String) -> Unit, onDismiss: () -> Unit) {
    val initial = remember(value) { runCatching { LocalDate.parse(value) }.getOrDefault(LocalDate.now()).atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli() }
    val state = rememberDatePickerState(initialSelectedDateMillis = initial)
    DatePickerDialog(onDismissRequest = onDismiss, confirmButton = {
        TextButton({ state.selectedDateMillis?.let { onPick(Instant.ofEpochMilli(it).atZone(ZoneOffset.UTC).toLocalDate().toString()) } ?: onDismiss() }) { Text(stringResource(R.string.common_done)) }
    }, dismissButton = { TextButton(onDismiss) { Text(stringResource(R.string.common_cancel)) } },
        colors = DatePickerDefaults.colors(containerColor = ur.surface)) {
        DatePicker(state, showModeToggle = false, colors = DatePickerDefaults.colors(containerColor = ur.surface,
            selectedDayContainerColor = ur.accent, todayDateBorderColor = ur.accent, todayContentColor = ur.accent))
    }
}

@Composable
internal fun FormError(error: String?) {
    error?.let {
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(ur.negativeSoft).padding(12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(Icons.Outlined.ErrorOutline, null, Modifier.size(18.dp), tint = ur.negative)
            Text(it, color = ur.negative, style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
internal fun EmptyState(title: String, detail: String, action: String? = null, onAction: (() -> Unit)? = null) {
    Column(Modifier.fillMaxWidth().padding(vertical = 36.dp, horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.size(56.dp).clip(RoundedCornerShape(28.dp)).background(ur.accentSoft), contentAlignment = Alignment.Center) {
            Icon(Icons.Outlined.Savings, null, Modifier.size(28.dp), tint = ur.accent)
        }
        Text(title, style = MaterialTheme.typography.titleMedium, color = ur.text, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = ur.muted, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        if (action != null && onAction != null) PrimaryButton(action, onAction, Modifier.padding(top = 6.dp))
    }
}
