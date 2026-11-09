package app.urbudget.nativeapp.ui

import android.Manifest
import android.content.Intent
import android.database.ContentObserver
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.provider.Telephony
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * Where collecting starts, as the sender picker and the setting offer it: right after the last text
 * handled ([handled], once there is one), new messages, a day, or everything on the phone; and what
 * the choice brings in or takes out ([size]).
 */
@Composable
private fun SmsStartFields(start: SmsStart, handled: Long?, size: String?, change: (SmsStart) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Box(Modifier.testTag("sms-collect")) {
            Choice(stringResource(R.string.sms_history), start.mode, listOfNotNull(handled?.let { "after" to stringResource(R.string.sms_after_handled) },
                "now" to stringResource(R.string.sms_new_only), "date" to stringResource(R.string.sms_from_date), "all" to stringResource(R.string.sms_everything))) { mode ->
                change(if (mode == "after" && handled != null) SmsStart.after(handled) else start.copy(mode = mode))
            }
        }
        if (start.mode == "date") DateField(start.day, { change(start.copy(day = it)) }, stringResource(R.string.sms_first_day))
        size?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = ur.text2, modifier = Modifier.testTag("sms-start-size")) }
        Text(stringResource(R.string.sms_collect_help), style = MaterialTheme.typography.bodySmall, color = ur.muted)
    }
}

/**
 * Where collecting has caught up to, as starting points: the newest text this phone handled for the
 * budget, and, from the budget itself (so it holds on another phone or after a reinstall), the last
 * day of each account's bank data and reconciling. Each sets the start in one tap.
 */
@Composable
private fun SmsWhereYouAre(handled: Long?, marks: List<SmsAccountMark>, choose: (SmsStart) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(stringResource(R.string.sms_where_title), style = MaterialTheme.typography.titleSmall, color = ur.text)
        handled?.let { SmsMarkRow(stringResource(R.string.sms_last_handled), smsMoment(it), stringResource(R.string.sms_start_after), "sms-mark-handled") { choose(SmsStart.after(it)) } }
        marks.take(5).forEach { mark ->
            SmsMarkRow(Money.own(mark.name), listOfNotNull(mark.bank?.let { stringResource(R.string.sms_bank_through, Money.date(it)) },
                mark.reconciled?.let { stringResource(R.string.sms_reconciled_through, Money.date(it)) }).joinToString(" · "),
                stringResource(R.string.sms_start_day, Money.date(mark.day)), "sms-mark:${mark.account}") { choose(SmsStart("date", mark.day)) }
        }
    }
}

/** A starting point: what it is, when, and the tap that starts there. */
@Composable
private fun SmsMarkRow(title: String, detail: String, action: String, tag: String, start: () -> Unit) {
    Row(Modifier.fillMaxWidth().testTag(tag), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyMedium, color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(detail, style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
        SmallAction(action, start)
    }
}

/** "2 Nov 2026, 07:47": when a text came, to the minute. */
@Composable
private fun smsMoment(at: Long): String {
    val moment = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault())
    return stringResource(R.string.sms_moment, Money.date(moment.toLocalDate().toString()), moment.toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)))
}

/** "2 requests · about 5 min": what reading takes, with "Brings in about 207 texts" before it when [brings]. */
@Composable
private fun sizeText(size: SmsReadingSize, brings: Boolean): String = listOfNotNull(
    if (brings) countText(R.plurals.sms_size_brings, size.texts) else null,
    if (size.requests > 0) countText(R.plurals.sms_size_requests, size.requests) else null,
    size.minutes?.let { countText(R.plurals.sms_size_minutes, it) },
).joinToString(" · ")

/** Who reads the messages, as the consent and the status name it: "Anthropic · claude-…". */
@Composable
private fun reader(connection: JSONObject): String {
    val provider = when (connection.text("provider")) { "anthropic" -> "Anthropic"; "openai" -> "OpenAI"; "chatgpt" -> "ChatGPT"; else -> connection.text("provider") }
    return provider + " · " + connection.text("model").ifBlank { stringResource(R.string.sms_default_model) }
}

/** What the assistant is doing with the texts sent now, in a few words: [part] texts, 0 while they're planned. */
@Composable
internal fun smsLiveText(live: SmsLive?, part: Int): String = when (live?.stage) {
    "waiting" -> stringResource(R.string.sms_live_waiting)
    "thinking" -> countText(R.plurals.sms_live_thinking, live.read + live.reviewed)
    "writing" -> when {
        live.section == "corrections" -> stringResource(R.string.sms_live_corrections)
        live.section == "templates" -> stringResource(R.string.sms_live_formats)
        live.read > 0 -> countText(R.plurals.sms_live_writing, live.read, Money.count(live.written.toLong()))
        else -> stringResource(R.string.sms_live_writing_review)
    }
    "fixing" -> stringResource(R.string.sms_live_fixing)
    else -> if (part > 0) countText(R.plurals.sms_live_sending, part) else stringResource(R.string.sms_live_ready)
}

/** Message text keeps its own direction: an Arabic alert reads right to left in an English app. */
internal val SmsBodyStyle @Composable get() = MaterialTheme.typography.bodyMedium.copy(textDirection = TextDirection.Content)

@Composable
internal fun SmsImportScreen(budget: JSONObject, vm: BudgetViewModel, setDockHidden: (Boolean) -> Unit = {}, analyzer: SmsAnalyzer = NativeSmsAnalyzer,
                             decline: () -> Unit = {}) {
    val identity = vm.state.value.budgetIdentity ?: return
    key(identity) { SmsHome(budget, vm, identity, analyzer, setDockHidden, decline) }
}

/**
 * Bank messages, built around what needs the user. With permission to send, opening the page reads
 * what's waiting; codes, alerts and texts already in the budget are handled without a question, and
 * what's left is counted with one button to review it. The texts themselves, and the settings, wait
 * below. What's stored and what the page is doing live in [SmsPage]; this draws it and keeps what's
 * only on screen (sheets, the list's choosing).
 */
@Composable
private fun SmsHome(budget: JSONObject, vm: BudgetViewModel, identity: String, analyzer: SmsAnalyzer, setDockHidden: (Boolean) -> Unit, decline: () -> Unit) {
    val context = LocalContext.current
    // Named, because Compose's test clock would otherwise carry on from a `withContext` on the worker that ended it, drawing there.
    val scope = rememberCoroutineScope { Dispatchers.Main }
    val lifecycle = LocalLifecycleOwner.current
    val page = remember { SmsPage(context.applicationContext, identity, vm, analyzer, scope) }
    LaunchedEffect(page) { page.start() }
    // What the assistant's cards asked of Bank messages: a reading to start, or stored changes to read again.
    LaunchedEffect(page) { BankTextsAssistant.reads.collect { read -> if (read != null) { BankTextsAssistant.reads.value = null; page.assistantRead(read.note) } } }
    LaunchedEffect(page) { var seen = BankTextsAssistant.changed.value; BankTextsAssistant.changed.collect { if (it != seen) { seen = it; page.reload() } } }
    val readings by SmsReadings.state.collectAsState()
    val vmState by vm.state.collectAsState()
    val reading = readings?.takeIf { it.budget == identity }
    val stored = page.stored
    val batch = page.batch
    var listing by remember { mutableStateOf(false) }
    var selecting by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf<Set<String>>(emptySet()) }
    var pickSenders by remember { mutableStateOf(false) }
    var collecting by remember { mutableStateOf(false) }
    var showCards by remember { mutableStateOf(false) }
    var showFormats by remember { mutableStateOf(false) }
    var revoke by remember { mutableStateOf(false) }
    var clear by remember { mutableStateOf(false) }
    var discarding by remember { mutableStateOf(false) }
    var forgetting by remember { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        page.permissions()
        if (page.allowed) pickSenders = page.stored.sources.isEmpty() else page.error = context.getString(R.string.sms_permission_needed)
    }
    // Reading in the background works without notifications; with them, it says when something needs the user.
    val notifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { page.auto(true) }
    DisposableEffect(lifecycle, context) {
        val observer = LifecycleEventObserver { _, event -> if (event == Lifecycle.Event.ON_RESUME) page.permissions() }
        lifecycle.lifecycle.addObserver(observer)
        onDispose { lifecycle.lifecycle.removeObserver(observer) }
    }
    DisposableEffect(page.allowed) {
        val observer = object : ContentObserver(Handler(Looper.getMainLooper())) { override fun onChange(selfChange: Boolean) { page.reload() } }
        if (page.allowed) context.contentResolver.registerContentObserver(Telephony.Sms.CONTENT_URI, true, observer)
        onDispose { context.contentResolver.unregisterContentObserver(observer) }
    }
    SideEffect { page.budget(budget) }
    LaunchedEffect(vmState.error) { vmState.error?.let(page::failed) }
    // Reading takes what was chosen; the list stops choosing.
    LaunchedEffect(reading != null) { if (reading != null) { selecting = false; selected = emptySet() } }
    LaunchedEffect(stored.messages) { selected = selected.intersect(stored.messages.mapTo(HashSet()) { it.id }) }
    LaunchedEffect(batch == null) { if (batch == null) discarding = false }
    val reviewBatch = batch?.takeIf { page.reviewing && page.allowed && reading == null }
    DisposableEffect(selecting, reviewBatch != null) { setDockHidden(selecting || reviewBatch != null); onDispose { setDockHidden(false) } }
    androidx.activity.compose.BackHandler(selecting) { selecting = false; selected = emptySet() }
    val connection = page.connection
    val ready = page.ready
    val consented = page.consented
    val names = stored.sources.joinToString(", ") { Money.own(it.sender) }
    val waiting = page.waiting
    val waitingCount = page.waitingCount
    val since = stored.since
    if (reviewBatch != null) SmsReview(budget, vm, reviewBatch, stored.mappings, close = page::close, change = page::edit,
        saveCards = page::cards, finish = { ids, rest, outcomes -> page.finish(ids, rest = rest, outcomes = outcomes) }, again = { note -> page.prepare(reviewBatch.messages, note) }, discard = { discarding = true },
        edited = page::edited, accuracy = stored.templates.accuracy)
    else Box(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.fillMaxSize().testTag("sms-inbox"), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 8.dp, bottom = if (selecting) 120.dp else DockRoom),
            verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (!page.allowed) item {
                ListCard { Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text(stringResource(R.string.sms_intro_title), style = MaterialTheme.typography.titleMedium, color = ur.text)
                    Text(stringResource(R.string.sms_intro), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                    Notice(stringResource(R.string.sms_disclosure), tone = "info")
                    PrimaryButton(stringResource(R.string.sms_allow), { permission.launch(arrayOf(Manifest.permission.READ_SMS, Manifest.permission.RECEIVE_SMS)) }, Modifier.fillMaxWidth())
                    // A plain no beside Allow: the page closes and nothing is asked.
                    TextButton(decline, Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.sms_not_now), color = ur.accent) }
                    TextButton({ context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}"))) },
                        Modifier.align(Alignment.CenterHorizontally)) { Text(stringResource(R.string.sms_open_settings), color = ur.accent) }
                } }
            }
            page.error?.let { item { FormError(it) } }
            if (page.allowed && stored.sources.isEmpty() && page.loaded) item {
                EmptyState(stringResource(R.string.sms_choose_bank_title), stringResource(R.string.sms_choose_bank_detail), stringResource(R.string.sms_choose_senders)) { pickSenders = true }
            }
            if (page.allowed && stored.sources.isNotEmpty()) {
                item(key = "status") {
                    ListCard(Modifier.testTag("sms-status")) { Column(Modifier.padding(18.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        val found = page.sorted
                        when {
                            reading != null -> {
                                // More than one request: stopping keeps what's been read, so it says Stop rather than Cancel.
                                val requests = reading.total > (reading.part.takeIf { it > 0 } ?: SmsBatchSize)
                                val live = reading.live
                                Text(stringResource(R.string.sms_reading_by, connection?.let { reader(it) }.orEmpty()), style = MaterialTheme.typography.titleMedium, color = ur.text)
                                // What the assistant is doing, its latest thought and the time so far: a long answer
                                // shouldn't look stuck. Its answer fills the bar as it covers the texts it reads in full; its
                                // corrections and formats, of no known length, run the bar instead.
                                val seconds by produceState(0L, reading.since) { while (true) { value = (System.currentTimeMillis() - reading.since) / 1000; delay(1000) } }
                                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                    Text(smsLiveText(live, reading.part), Modifier.weight(1f).testTag("sms-live"), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                                    Text("${seconds / 60}:${(seconds % 60).toString().padStart(2, '0')}", style = MaterialTheme.typography.bodySmall, color = ur.muted)
                                }
                                val share = live?.takeIf { it.stage == "writing" && it.read > 0 && it.section !in setOf("corrections", "templates") }
                                    ?.let { (it.written.toFloat() / it.read).coerceIn(0f, 1f) }
                                val shown by androidx.compose.animation.core.animateFloatAsState(share ?: 0f, label = "sms-written")
                                if (share != null) LinearProgressIndicator(progress = { shown }, modifier = Modifier.fillMaxWidth(), color = ur.accent)
                                else LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
                                live?.thought?.takeIf { it.isNotBlank() }?.let {
                                    Text(it, Modifier.testTag("sms-thought"), style = MaterialTheme.typography.bodySmall.copy(textDirection = TextDirection.Content), color = ur.muted,
                                        fontStyle = androidx.compose.ui.text.font.FontStyle.Italic, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                }
                                if (requests) {
                                    Text(countText(R.plurals.sms_reading_progress, reading.total, Money.count(reading.read.toLong())), style = MaterialTheme.typography.bodySmall, color = ur.text2,
                                        modifier = Modifier.testTag("sms-progress"))
                                    Text(stringResource(R.string.sms_reading_keep), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                                }
                                TextButton({ SmsReadings.stop() }, Modifier.align(Alignment.End).testTag("sms-stop")) {
                                    Text(stringResource(if (requests) R.string.sms_stop_reading else R.string.common_cancel), color = ur.accent)
                                }
                            }
                            batch != null && found == null -> {
                                Text(stringResource(R.string.sms_checking), style = MaterialTheme.typography.titleMedium, color = ur.text)
                                LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
                            }
                            batch != null && found != null -> {
                                Text(countText(R.plurals.sms_to_review, found.needs), style = MaterialTheme.typography.titleMedium, color = ur.text, modifier = Modifier.testTag("sms-needs"))
                                if (found.existing.isNotEmpty() || found.skipped.isNotEmpty())
                                    Text(handledSummary(found.existing.size, found.skipped), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                                // Texts in a format the budget knows were read on the phone and checked by the assistant;
                                // codes and formats it has confirmed enough to skip never left the phone.
                                val templated = batch.templated; val kept = batch.kept
                                if (templated > 0 || kept > 0) Text(listOfNotNull(
                                    templated.takeIf { it > 0 }?.let { countText(R.plurals.sms_by_template, it, Money.count((batch.messages.size - it - kept).toLong())) },
                                    kept.takeIf { it > 0 }?.let { countText(R.plurals.sms_kept_on_phone, it) },
                                ).joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = ur.muted, modifier = Modifier.testTag("sms-by-template"))
                                PrimaryButton(stringResource(R.string.sms_review_open), page::open, Modifier.fillMaxWidth().testTag("sms-review"))
                                if (waitingCount > 0) {
                                    Text(countText(R.plurals.sms_more_waiting, waitingCount), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                                    if (ready) SecondaryButton(countText(R.plurals.sms_read_more, waitingCount), { page.prepare(waiting, base = batch) },
                                        Modifier.fillMaxWidth().testTag("sms-read-more"), enabled = !page.busy && waiting.isNotEmpty())
                                }
                                TextButton({ discarding = true }, Modifier.align(Alignment.CenterHorizontally).testTag("sms-discard"), enabled = !page.busy) {
                                    Text(stringResource(R.string.sms_discard_review), color = ur.accent)
                                }
                            }
                            waitingCount > 0 -> {
                                Text(countText(R.plurals.sms_new_messages, waitingCount), style = MaterialTheme.typography.titleMedium, color = ur.text)
                                Text(stringResource(R.string.sms_from, names), style = MaterialTheme.typography.bodyMedium, color = ur.text2, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                if (ready) {
                                    PrimaryButton(if (consented) countText(R.plurals.sms_read_count, waitingCount) else stringResource(R.string.sms_read_with, connection?.let { reader(it) }.orEmpty()),
                                        { page.prepare(waiting) }, Modifier.fillMaxWidth().testTag("sms-prepare"), enabled = !page.busy && waiting.isNotEmpty())
                                    if (waitingCount > SmsBatchSize) Text(sizeText(smsReadingSize(waitingCount, stored.pace, stored.templates.count > 0), brings = false), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                                    Text(stringResource(R.string.sms_reads_note), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                                } else if (connection != null) Notice(stringResource(R.string.sms_connect_assistant), tone = "info")
                            }
                            else -> {
                                Text(stringResource(R.string.sms_up_to_date), style = MaterialTheme.typography.titleMedium, color = ur.text, modifier = Modifier.testTag("sms-up-to-date"))
                                Text(stored.last?.let { lastText(it) } ?: stringResource(R.string.sms_up_to_date_detail, names), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
                            }
                        }
                    } }
                }
                if (!page.receiving) item { Notice(stringResource(R.string.sms_receive_needed), action = stringResource(R.string.sms_allow), onAction = { permission.launch(arrayOf(Manifest.permission.READ_SMS, Manifest.permission.RECEIVE_SMS)) }) }
                if (waiting.isNotEmpty()) {
                    // The texts themselves, mostly codes and alerts, wait folded below what needs the user.
                    item(key = "messages") {
                        ListCard(Modifier.testTag("sms-waiting")) {
                            ListRow(stringResource(R.string.sms_waiting_title), subtitle = stringResource(R.string.sms_waiting_detail), onClick = { listing = !listing; if (!listing) { selecting = false; selected = emptySet() } },
                                chevron = false, trailing = { Text(stringResource(if (listing) R.string.sms_hide else R.string.sms_show), color = ur.accent, fontWeight = FontWeight.SemiBold) })
                        }
                    }
                    if (listing) {
                        item(key = "choose") {
                            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                                if (reading == null) TextButton({ selecting = !selecting; selected = emptySet() }, Modifier.testTag("sms-choose")) {
                                    Text(stringResource(if (selecting) R.string.common_done else R.string.sms_choose_messages), color = ur.accent, fontWeight = FontWeight.SemiBold)
                                }
                            }
                        }
                        val shown = waiting.asReversed()
                        itemsIndexed(shown, key = { _, it -> it.id }) { index, message ->
                            SmsMessageRow(message, index, shown.size, stored.sources.size > 1, if (selecting) message.id in selected else null,
                                enabled = true) { on -> selected = if (on) selected + message.id else selected - message.id }
                        }
                    }
                }
                item(key = "settings") {
                    Column(Modifier.padding(top = 12.dp)) { ListCard {
                        ListRow(stringResource(R.string.sms_senders), subtitle = names, onClick = { if (reading == null && !page.busy) pickSenders = true }, divider = true)
                        ListRow(stringResource(R.string.sms_collect), subtitle = listOfNotNull(since?.let { smsSinceText(it) }, stored.handled?.let { stringResource(R.string.sms_last_handled_at, smsMoment(it)) })
                            .joinToString(" · "), onClick = { if (reading == null && !page.busy) collecting = true }, divider = true)
                        if (consented) ListRow(stringResource(R.string.sms_auto), subtitle = stringResource(R.string.sms_auto_detail), onClick = {
                            if (stored.auto) page.auto(false)
                            else if (android.os.Build.VERSION.SDK_INT >= 33 && androidx.core.content.ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED)
                                notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                            else page.auto(true)
                        }, divider = true, chevron = false, trailing = { Switch(stored.auto, null, Modifier.testTag("sms-auto")) })
                        ListRow(stringResource(R.string.sms_cards), subtitle = if (stored.mappings.length() == 0) stringResource(R.string.sms_cards_none) else countText(R.plurals.sms_cards_count, stored.mappings.length()),
                            onClick = { showCards = true }, divider = true)
                        // Formats learned as the assistant reads: how many, and how often their readings needed no correction.
                        val learned = stored.templates
                        ListRow(stringResource(R.string.sms_templates_title), subtitle = if (learned.count == 0) stringResource(R.string.sms_templates_none) else listOfNotNull(
                            countText(R.plurals.sms_templates_count, learned.count),
                            learned.filled.takeIf { it > 0 }?.let { stringResource(R.string.sms_templates_confirmed, Money.count((learned.confirmed * 100L) / it)) },
                        ).joinToString(" · "), modifier = Modifier.testTag("sms-formats"), onClick = { if (learned.count > 0) showFormats = true }, divider = true,
                            chevron = learned.count > 0)
                        // A permission for another assistant, or from before reading on open, no longer counts: the next reading asks.
                        if (consented) ListRow(stringResource(R.string.sms_sending), subtitle = stringResource(R.string.sms_sending_allowed), onClick = { revoke = true }, divider = true)
                        ListRow(stringResource(R.string.sms_stop), titleColor = ur.negative, onClick = { if (reading == null && !page.busy) clear = true }, chevron = false)
                    } }
                }
                // How to know it's reading everything: what the phone holds from each sender, against what's collected.
                if (stored.coverage.isNotEmpty()) item(key = "coverage") {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(stringResource(R.string.sms_on_phone), style = MaterialTheme.typography.titleSmall, color = ur.text, modifier = Modifier.padding(top = 4.dp))
                        ListCard(Modifier.testTag("sms-coverage")) {
                            stored.coverage.forEachIndexed { index, c ->
                                ListRow(Money.own(c.sender), subtitle = coverageText(c), chevron = false, divider = index < stored.coverage.lastIndex)
                            }
                        }
                    }
                }
            }
        }
        if (selecting) Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth().background(ur.bg).navigationBarsPadding().padding(horizontal = Gutter, vertical = 12.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                SecondaryButton(countText(R.plurals.sms_dismiss_count, selected.size), { page.dismiss(selected.toList()); selecting = false; selected = emptySet() }, Modifier.weight(1f),
                    enabled = selected.isNotEmpty() && !page.busy)
                PrimaryButton(countText(R.plurals.sms_read_count, selected.size), { page.prepare(waiting.filter { it.id in selected }, base = batch) }, Modifier.weight(1f).testTag("sms-prepare-selected"),
                    enabled = selected.isNotEmpty() && ready && reading == null && !page.busy)
            }
        }
    }
    if (pickSenders && page.allowed) SmsSenderPicker(stored.sources, { pickSenders = false }, suggested = remember(budget) { SmsStart.suggested(budget) },
        building = remember(budget) { budget.rows("accounts").none { !it.optBoolean("archived") } }, save = { chosen, start ->
        scope.launch { page.senders(chosen, start).join(); if (page.error == null) pickSenders = false }
    })
    if (collecting) {
        val current = remember { since?.let { SmsStart.of(it, stored.handled) } ?: SmsStart.suggested(budget) }
        var start by remember { mutableStateOf(current) }
        val marks = remember(budget) { smsAccountMarks(budget) }
        // What the choice brings in (texts on the phone not collected yet) or takes out (texts waiting before it).
        var change by remember { mutableStateOf<Pair<Int, Int>?>(null) }
        var confirming by remember { mutableStateOf(false) }
        val inbox = remember { SmsInbox.get(context) }
        LaunchedEffect(start) {
            change = null
            val from = start.since()
            val old = since ?: from
            change = runCatching { withContext(smsStorageLane) { when {
                from < old -> (SmsReader.count(context, stored.sources.map { it.sender }, from, old) - inbox.collectedBetween(identity, from, old)).coerceAtLeast(0) to 0
                from > old -> 0 to inbox.waitingBefore(identity, from)
                else -> 0 to 0
            } } }.getOrNull()
        }
        fun apply() { collecting = false; page.collectFrom(start.since()) }
        val brings = change?.first ?: 0
        FormDialog(stringResource(R.string.sms_collect), { collecting = false }, enabled = start != current && start.valid, onConfirm = {
            // Texts brought in are read as soon as they're collected: more than one request is asked first.
            if (consented && brings > SmsBatchSize) confirming = true else apply()
        }) {
            since?.let { Text(stringResource(R.string.sms_collect_now, smsSinceText(it)), style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
            if (stored.handled != null || marks.isNotEmpty()) SmsWhereYouAre(stored.handled, marks) { start = it }
            SmsStartFields(start, stored.handled, change?.let { (inn, out) -> when {
                inn > 0 -> sizeText(smsReadingSize(inn, stored.pace, stored.templates.count > 0), brings = true)
                out > 0 -> countText(R.plurals.sms_size_takes_out, out)
                start != current -> stringResource(R.string.sms_size_none)
                else -> null
            } }) { start = it }
        }
        if (confirming) AlertDialog(onDismissRequest = { confirming = false }, title = { Text(countText(R.plurals.sms_read_many_title, brings)) },
            text = { Text(stringResource(R.string.sms_read_many_detail, connection?.let { reader(it) }.orEmpty(), sizeText(smsReadingSize(brings, stored.pace, stored.templates.count > 0), brings = false))) },
            confirmButton = { TextButton({ confirming = false; apply() }, Modifier.testTag("sms-read-many")) { Text(stringResource(R.string.sms_read_many_confirm)) } },
            dismissButton = { TextButton({ confirming = false }) { Text(stringResource(R.string.common_cancel)) } })
    }
    if (discarding) batch?.let { open ->
        AlertDialog(onDismissRequest = { discarding = false }, title = { Text(stringResource(R.string.sms_discard_title)) },
            text = { Text(countText(R.plurals.sms_discard_detail, open.messages.size)) },
            confirmButton = { TextButton({ discarding = false; page.discard() }, Modifier.testTag("sms-discard-confirm")) {
                Text(stringResource(R.string.sms_discard), color = ur.negative)
            } },
            dismissButton = { TextButton({ discarding = false }) { Text(stringResource(R.string.common_cancel)) } })
    }
    if (showCards) SmsCardsSheet(budget, stored.mappings, stored.formats, { showCards = false }, page::cards)
    if (showFormats) SmsFormatsSheet(stored, { showFormats = false }, forget = { if (reading == null && !page.busy) page.forgetTemplate(it) },
        forgetAll = { if (reading == null && !page.busy) { showFormats = false; forgetting = true } })
    if (forgetting) AlertDialog(onDismissRequest = { forgetting = false }, title = { Text(stringResource(R.string.sms_templates_forget_title)) },
        text = { Text(stringResource(R.string.sms_templates_forget_detail)) },
        confirmButton = { TextButton({ forgetting = false; page.forgetTemplates() }) { Text(stringResource(R.string.sms_templates_forget), color = ur.negative) } },
        dismissButton = { TextButton({ forgetting = false }) { Text(stringResource(R.string.common_cancel)) } })
    page.asking?.let { request ->
        FormDialog(stringResource(R.string.sms_send_title), { page.asking = null }, confirm = stringResource(R.string.sms_send_confirm), onConfirm = { page.allow(request) }) {
            Text(stringResource(R.string.sms_send_notice, reader(request.connection)), style = MaterialTheme.typography.bodyMedium, color = ur.text, modifier = Modifier.testTag("sms-consent"))
            Text(sizeText(smsReadingSize(request.messages.size, stored.pace, stored.templates.count > 0), brings = false), style = MaterialTheme.typography.bodyMedium, color = ur.text2)
            Text(stringResource(R.string.sms_send_later), style = MaterialTheme.typography.bodySmall, color = ur.muted)
        }
    }
    if (revoke) AlertDialog(onDismissRequest = { revoke = false }, title = { Text(stringResource(R.string.sms_revoke_title)) }, text = { Text(stringResource(R.string.sms_revoke_detail)) },
        confirmButton = { TextButton({ revoke = false; page.revoke() }) { Text(stringResource(R.string.sms_revoke)) } },
        dismissButton = { TextButton({ revoke = false }) { Text(stringResource(R.string.common_cancel)) } })
    if (clear) AlertDialog(onDismissRequest = { clear = false }, title = { Text(stringResource(R.string.sms_stop)) }, text = { Text(stringResource(R.string.sms_stop_detail)) },
        confirmButton = { TextButton({ clear = false; page.clear() }) { Text(stringResource(R.string.sms_stop), color = ur.negative) } },
        dismissButton = { TextButton({ clear = false }) { Text(stringResource(R.string.common_cancel)) } })
}

/** "Checked 11:38 · 2 already in your budget · 3 codes": when the page last finished, and what it handled. */
@Composable
private fun lastText(last: JSONObject): String {
    val at = last.optLong("at").takeIf { it > 0 } ?: return ""
    val time = remember(at) {
        val moment = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault())
        if (moment.toLocalDate() == java.time.LocalDate.now()) moment.toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
        else Money.date(moment.toLocalDate().toString())
    }
    val skipped = last.optJSONObject("skipped")
    val handled = listOfNotNull(
        last.optInt("known").takeIf { it > 0 }?.let { countText(R.plurals.sms_known, it) },
        skipped?.let { s -> s.keys().asSequence().toList().sortedByDescending { s.optInt(it) }.map { skip -> skip to s.optInt(skip) } }?.map { (skip, n) ->
            countText(when (skip) {
                "code" -> R.plurals.sms_skip_code; "security" -> R.plurals.sms_skip_security; "balance" -> R.plurals.sms_skip_balance
                "declined" -> R.plurals.sms_skip_declined; "rewards" -> R.plurals.sms_skip_rewards; "marketing" -> R.plurals.sms_skip_marketing
                "unclear" -> R.plurals.sms_skip_dismissed; else -> R.plurals.sms_skip_other
            }, n)
        }?.joinToString(" · ")?.ifBlank { null },
    )
    return (listOf(stringResource(R.string.sms_checked, time)) + handled).joinToString(" · ")
}

/** "2,913 texts, 2024-12-13 to 2026-11-01 · SMS: 55 · MMS: 2,858 · All collected". */
@Composable
private fun coverageText(c: SmsCoverage): String {
    val total = c.sms + c.mms + c.unreadable
    if (total == 0 || c.first == null || c.latest == null) return stringResource(R.string.sms_coverage_none)
    fun day(at: Long) = Money.date(Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()).toLocalDate().toString())
    return listOfNotNull(
        countText(R.plurals.sms_coverage, total, day(c.first), day(c.latest)),
        stringResource(R.string.sms_coverage_split, c.sms.toString(), c.mms.toString()),
        if (c.collected >= c.sms + c.mms) stringResource(R.string.sms_coverage_all) else countText(R.plurals.sms_coverage_collected, c.sms + c.mms, c.collected.toString()),
        if (c.unreadable > 0) countText(R.plurals.sms_coverage_unreadable, c.unreadable) else null,
    ).joinToString(" · ")
}

/** Where collecting starts: "Since 1 Oct 2026", "Since 2 Nov 2026, 19:45" when it's new messages from then, or everything on the phone. */
@Composable
private fun smsSinceText(since: Long): String {
    if (since <= 0L) return stringResource(R.string.sms_everything)
    val at = Instant.ofEpochMilli(since).atZone(ZoneId.systemDefault())
    val day = Money.date(at.toLocalDate().toString())
    return if (SmsStart.of(since).mode == "date") stringResource(R.string.sms_since, day)
        else stringResource(R.string.sms_since_time, day, at.toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)))
}

/** A message in the list: when it came, and its text in its own direction. [checked] is null outside choosing. */
@Composable
private fun SmsMessageRow(message: BankSms, index: Int, count: Int, showSender: Boolean, checked: Boolean?, enabled: Boolean, toggle: (Boolean) -> Unit) {
    val time = remember(message.received) {
        Instant.ofEpochMilli(message.received).atZone(ZoneId.systemDefault()).toLocalTime().format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
    }
    Column(Modifier.cardSegment(0, 1, cardFill()).testTag("sms-message:${message.id}")
        .then(if (checked != null) Modifier.toggleable(checked, enabled = enabled, role = Role.Checkbox, onValueChange = toggle) else Modifier)) {
        Row(Modifier.padding(start = if (checked != null) 4.dp else 16.dp, end = 16.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.Top) {
            if (checked != null) Checkbox(checked, null, Modifier.testTag("sms-select:${message.id}"), enabled = enabled, colors = CheckboxDefaults.colors(checkedColor = ur.accent))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(listOfNotNull(Money.own(message.sender).takeIf { showSender }, Money.date(message.receivedDate), time).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall, color = ur.muted, maxLines = 1)
                Text(message.body, style = SmsBodyStyle, color = ur.text, maxLines = 5, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
internal fun SmsSenderPicker(sources: List<SmsSource>, close: () -> Unit, save: (Set<String>, Long) -> Unit,
                             load: (android.content.Context) -> List<SmsSender> = SmsReader::senders, suggested: SmsStart = SmsStart("now", java.time.LocalDate.now().toString()),
                             building: Boolean = false) {
    val context = LocalContext.current
    var found by remember { mutableStateOf<List<SmsSender>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var chosen by remember { mutableStateOf(sources.map { it.sender }.toSet()) }
    var search by remember { mutableStateOf("") }
    var start by remember { mutableStateOf(suggested) }
    var numbers by remember { mutableStateOf(false) }
    // What the first start brings in from the chosen senders.
    var brings by remember { mutableStateOf<Int?>(null) }
    if (sources.isEmpty()) LaunchedEffect(chosen, start) {
        brings = null
        brings = runCatching { withContext(smsStorageLane) { SmsReader.count(context, chosen, start.since()) } }.getOrNull()
    }
    LaunchedEffect(Unit) {
        try {
            found = withContext(smsStorageLane) {
                val loaded = load(context)
                // Senders chosen before stay listed even when the phone no longer has their messages.
                loaded + sources.filter { s -> loaded.none { smsSenderKey(it.address) == smsSenderKey(s.sender) } }.map { SmsSender(it.sender, 0, null) }
            }
        } catch (cancel: CancellationException) { throw cancel } catch (failure: Exception) { error = failure.message }
    }
    // Senders already chosen come first; the rest by how many texts they sent.
    val picked = remember(sources) { sources.map { smsSenderKey(it.sender) }.toSet() }
    val matching = found.orEmpty().filter { it.address.contains(search, ignoreCase = true) }.sortedByDescending { smsSenderKey(it.address) in picked }
    val (people, banks) = matching.partition { smsPersonal(it.address) }
    fun toggle(sender: SmsSender, on: Boolean) { chosen = if (on) chosen + sender.address else chosen - sender.address }
    LazyFormDialog(stringResource(R.string.sms_senders), close, confirm = stringResource(R.string.common_save),
        enabled = found != null && chosen.isNotEmpty() && chosen.size <= 50 && (sources.isNotEmpty() || start.valid), onConfirm = {
        // A sender added later collects from where the others do; the setting changes it for all.
        save(chosen, if (sources.isEmpty()) start.since() else sources.minOf { it.since })
    }) {
        item { Text(stringResource(R.string.sms_sender_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
        if (sources.isEmpty()) item { SmsStartFields(start, null, brings?.takeIf { chosen.isNotEmpty() }?.let { sizeText(smsReadingSize(it, null), brings = true) }) { start = it } }
        // A budget with no accounts is set up from these texts.
        if (sources.isEmpty() && building) item { Text(stringResource(R.string.sms_start_builds), Modifier.testTag("sms-start-builds"), style = MaterialTheme.typography.bodySmall, color = ur.muted) }
        item { SearchField(search, { search = it }, stringResource(R.string.common_search)) }
        error?.let { item { FormError(it) } }
        if (found == null && error == null) item { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        if (found?.isEmpty() == true) item { Text(stringResource(R.string.sms_no_senders), color = ur.muted) }
        if (banks.isNotEmpty()) item(key = "banks") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.sms_banks), style = MaterialTheme.typography.titleSmall, color = ur.text)
                ListCard(Modifier.testTag("sms-senders")) {
                    banks.forEachIndexed { index, sender ->
                        SenderRow(sender, sender.address in chosen, sender.address in chosen || chosen.size < 50, index < banks.lastIndex) { toggle(sender, it) }
                    }
                }
            }
        }
        if (people.isNotEmpty()) item(key = "people") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (search.isBlank()) TextButton({ numbers = !numbers }) {
                    Text(if (numbers) stringResource(R.string.sms_hide_numbers) else countText(R.plurals.sms_show_numbers, people.size), color = ur.accent)
                }
                if (numbers || search.isNotBlank()) ListCard {
                    people.forEachIndexed { index, sender ->
                        SenderRow(sender, sender.address in chosen, sender.address in chosen || chosen.size < 50, index < people.lastIndex) { toggle(sender, it) }
                    }
                }
            }
        }
    }
}

/** A sender in the picker: its name and how many texts it sent, and a tick at the end like the app's other toggles. */
@Composable
private fun SenderRow(sender: SmsSender, checked: Boolean, enabled: Boolean, divider: Boolean, toggle: (Boolean) -> Unit) {
    // A short code's latest text, kept in line with the names above it in either script.
    val align = if (LocalLayoutDirection.current == LayoutDirection.Ltr) TextAlign.Left else TextAlign.Right
    Column(Modifier.fillMaxWidth().toggleable(checked, enabled = enabled, role = Role.Checkbox, onValueChange = toggle).testTag("sms-sender:${sender.address}")) {
        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 16.dp, end = 6.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(Money.own(sender.address), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.Medium, color = ur.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (sender.count > 0) Text(countText(R.plurals.sms_message_count, sender.count), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                sender.preview?.let { Text(it, style = SmsBodyStyle.merge(MaterialTheme.typography.bodySmall), color = ur.text2, textAlign = align, maxLines = 2, overflow = TextOverflow.Ellipsis) }
            }
            Checkbox(checked, null, enabled = enabled, colors = CheckboxDefaults.colors(checkedColor = ur.accent))
        }
        if (divider) Hairline(start = 16.dp)
    }
}

/** The cards remembered for their senders: which account each is, and a way to forget one. */
/**
 * The formats learned for the budget, by sender: what each reads, a text it read, and how its
 * readings fared. Forgetting one has its texts read in full again; a sender that changed how it
 * writes is pointed out, since its new formats are being learned.
 */
@Composable
private fun SmsFormatsSheet(stored: SmsStored, close: () -> Unit, forget: (String) -> Unit, forgetAll: () -> Unit) {
    val names = stored.sources.associate { smsSenderKey(it.sender) to it.sender }
    val bySender = stored.formats.groupBy { it.sender }
    LazyFormDialog(stringResource(R.string.sms_templates_title), close) {
        item { Text(stringResource(R.string.sms_templates_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
        stored.templates.drifting.forEach { sender ->
            item(key = "drift:$sender") {
                Text(stringResource(R.string.sms_templates_drifting, Money.own(names[sender] ?: sender)), Modifier.testTag("sms-drifting:$sender"),
                    style = MaterialTheme.typography.bodyMedium, color = ur.text)
            }
        }
        bySender.forEach { (sender, formats) ->
            item(key = "sender:$sender") {
                Text(Money.own(names[sender] ?: sender), style = MaterialTheme.typography.titleSmall, color = ur.text, modifier = Modifier.padding(top = 8.dp))
            }
            itemsIndexed(formats, key = { _, f -> f.id }) { _, format ->
                Column(Modifier.cardSegment(0, 1, cardFill()).padding(start = 16.dp, top = 12.dp, bottom = 12.dp).testTag("sms-format:" + format.id),
                    verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(verticalAlignment = Alignment.Top) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(smsFormatTitle(format), style = MaterialTheme.typography.bodyLarge, color = ur.text)
                            Text(smsFormatStatus(format), style = MaterialTheme.typography.bodySmall, color = ur.muted)
                        }
                        IconButton({ forget(format.id) }, Modifier.testTag("sms-format-forget:" + format.id)) {
                            Icon(Icons.Outlined.Close, stringResource(R.string.sms_templates_forget_one), tint = ur.muted)
                        }
                    }
                    if (format.example.isNotBlank()) Text(format.example, Modifier.padding(end = 16.dp), style = SmsBodyStyle, color = ur.text2, maxLines = 3, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        item(key = "forget-all") {
            Column(Modifier.padding(top = 8.dp)) { ListCard {
                ListRow(stringResource(R.string.sms_templates_forget_all), titleColor = ur.negative, onClick = forgetAll, chevron = false)
            } }
        }
    }
}

/** What a format reads: its review note (the payee left out), or what it skips. */
@Composable
private fun smsFormatTitle(format: SmsTemplateRow): String {
    val note = format.body.optString("note").replace("{payee}", "…").trim()
    val skip = format.skip
    return when {
        note.isNotEmpty() -> note
        skip != null -> stringResource(smsSkipLabel(skip))
        else -> format.kind.ifBlank { stringResource(R.string.sms_templates_title) }
    }
}

/** How a format's readings fared: kept on the phone once trusted, or the share confirmed in review. */
@Composable
private fun smsFormatStatus(format: SmsTemplateRow): String {
    val settled = format.stats.optInt("settled")
    val trusted = format.state == "trusted"
    return listOfNotNull(
        stringResource(R.string.sms_templates_trusted).takeIf { trusted },
        format.accuracy?.let { stringResource(R.string.sms_templates_confirmed, Money.count(it.toLong())) } ?: stringResource(R.string.sms_templates_unreviewed).takeIf { !trusted },
        settled.takeIf { it > 0 }?.let { countText(R.plurals.sms_templates_settled, it) },
    ).joinToString(" · ")
}

@Composable
private fun SmsCardsSheet(budget: JSONObject, mappings: JSONArray, formats: List<SmsTemplateRow>, close: () -> Unit, save: (JSONArray) -> Unit) {
    val cards = (0 until mappings.length()).map { mappings.getJSONObject(it) }
    LazyFormDialog(stringResource(R.string.sms_cards), close) {
        item { Text(stringResource(R.string.sms_cards_help), style = MaterialTheme.typography.bodyMedium, color = ur.text2) }
        if (cards.isEmpty()) item { Text(stringResource(R.string.sms_cards_none), color = ur.muted) }
        itemsIndexed(cards, key = { i, card -> "$i:" + card.text("sender") + card.text("reference") + card.text("for") }) { index, card ->
            Column(Modifier.cardSegment(0, 1, cardFill())) {
                val name = if (card.text("accountId") == SmsSkipCard) stringResource(R.string.sms_card_skipped)
                    else budget.row("accounts", card.text("accountId"))?.text("name")?.let(Money::own) ?: stringResource(R.string.sms_account_gone)
                // Beside a card's account: the account that pays its transfers, or the user's own account a name is.
                val account = when (card.text("for")) {
                    SmsForTransfer, SmsForFormat -> stringResource(R.string.sms_card_transfers, name)
                    SmsForOwn -> stringResource(R.string.sms_own_account, name)
                    else -> name
                }
                // A format is said by what it reads ("Transfer to … account"), not its id.
                val title = if (card.text("for") == SmsForFormat) formats.firstOrNull { it.id == card.text("reference") }?.let { smsFormatTitle(it) }
                    ?: stringResource(R.string.sms_templates_title) else smsRefLabel(card.text("reference"))
                ListRow(title, subtitle = Money.own(card.text("sender")) + " · " + account,
                    chevron = false, trailing = {
                        IconButton({ save(JSONArray(cards.filterIndexed { i, _ -> i != index })) }) { Icon(Icons.Outlined.Close, stringResource(R.string.sms_forget), tint = ur.muted) }
                    })
            }
        }
    }
}
