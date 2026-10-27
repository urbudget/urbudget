package app.urbudget.nativeapp.ui

import android.app.Activity
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.ui.Alignment
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle

@Composable
internal fun CloudScreen(vm: BudgetViewModel) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val preferences = remember { NativePreferences(context) }
    val billing = remember { BillingGateway(context as Activity) }
    DisposableEffect(billing) { onDispose { billing.close() } }
    var status by remember { mutableStateOf<JSONObject?>(null) }
    var overview by remember { mutableStateOf<JSONObject?>(null) }
    var catalog by remember { mutableStateOf<JSONObject?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    var email by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var sent by remember { mutableStateOf(false) }
    var dialog by remember { mutableStateOf("") }
    var phrase by remember { mutableStateOf("") }
    var repeated by remember { mutableStateOf("") }
    var confirmation by remember { mutableStateOf("") }
    var pendingBudget by remember { mutableStateOf<JSONObject?>(null) }
    var automatic by remember { mutableStateOf(preferences.automaticSync) }
    var overviewRevision by remember { mutableIntStateOf(0) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    suspend fun reloadStatus() {
        val previous = status
        val next = NativeServices.read("cloud_status") as JSONObject
        val changed = withContext(Dispatchers.Default) { !sameJson(previous, next) }
        if (changed) status = next
    }
    fun work(operation: suspend () -> Unit) {
        if (busy) return
        busy = true; error = null; notice = null
        scope.launch {
            try { operation(); reloadStatus(); overviewRevision++ }
            catch (cancel: CancellationException) { throw cancel }
            catch (failure: Exception) { error = failure.message ?: context.getString(R.string.common_try_again) }
            finally { busy = false }
        }
    }
    fun call(method: String, params: JSONObject = JSONObject(), done: (Any?) -> Unit = {}) = work {
        var invoked = false
        try {
            val result = withContext(Dispatchers.IO) {
                invoked = true
                NativeServices.call(method, params)
            }
            done(result)
        } finally {
            // JNI can complete a sync commit after this screen is disposed. Queue the
            // revision probe even when cancellation prevents delivering the result.
            if (invoked && NativeServicePolicy.budgetResult(method)) vm.refresh()
        }
    }
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                try { reloadStatus() }
                catch (cancel: CancellationException) { throw cancel }
                catch (failure: Exception) { error = failure.message }
                delay(5_000)
            }
        }
    }
    val signedIn = status?.text("email").orEmpty()
    LaunchedEffect(signedIn, overviewRevision, lifecycle) {
        if (signedIn.isBlank()) { overview = null; catalog = null }
        else lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            // Fetch optional subscription details independently: a slow /me request must not
            // disable local sync controls or delay status polling after every successful action.
            try { overview = NativeServices.read("account_overview") as JSONObject }
            catch (cancel: CancellationException) { throw cancel }
            catch (_: Exception) { /* Existing status remains usable when offline. */ }
        }
    }
    val sync = status?.optJSONObject("sync")
    val report = sync?.optJSONObject("last")
    val reportedBudgets = remember(report) { report?.rows("budgets").orEmpty() }
    fun purchaseMessage(result: JSONObject) {
        notice = context.getString(when {
            result.optBoolean("cancelled") -> R.string.billing_cancelled
            result.optBoolean("pending") -> R.string.billing_pending
            result.optInt("restored") > 0 -> R.string.billing_restored
            else -> R.string.billing_none_found
        })
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item { Text(stringResource(R.string.cloud_intro), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium) }
        if (busy) item { LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent) }
        error?.let { item { FormError(it) } }
        notice?.let { item { Notice(it, tone = "info") } }
        item { SectionTitle(stringResource(R.string.cloud_account)) }
        item {
            if (signedIn.isBlank()) CloudCard {
                Text(stringResource(R.string.cloud_sign_in_intro), color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                Field(email, { email = it; sent = false; code = "" }, stringResource(R.string.cloud_email))
                if (sent) Field(code, { code = it }, stringResource(R.string.cloud_code))
                val sendCode = { call("account_start", JSONObject().put("email", email.trim())) { sent = true; notice = context.getString(R.string.cloud_check_email) } }
                if (sent) Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    PrimaryButton(stringResource(R.string.cloud_sign_in), { call("account_verify", JSONObject().put("email", email.trim()).put("code", code.trim())) { code = "" } }, Modifier.weight(1f), enabled = !busy && code.isNotBlank())
                    SecondaryButton(stringResource(R.string.cloud_new_code), sendCode, Modifier.weight(1f), enabled = !busy && email.contains('@'))
                } else PrimaryButton(stringResource(R.string.cloud_send_code), sendCode, Modifier.fillMaxWidth(), enabled = !busy && email.contains('@'))
            } else ListCard {
                ListRow(signedIn, subtitle = status?.text("credentialStore").orEmpty().ifBlank { stringResource(R.string.cloud_session_stored) }, icon = Icons.Outlined.Person, chevron = false, divider = true)
                ListRow(stringResource(R.string.cloud_sign_out), titleColor = ur.negative, onClick = { if (!busy) call("account_sign_out") }, chevron = false)
            }
        }
        item { SectionTitle(stringResource(R.string.cloud_subscription)) }
        item {
            CloudCard {
                Text(stringResource(R.string.cloud_subscription_intro), color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                if (signedIn.isBlank()) Text(stringResource(R.string.cloud_sign_in_to_subscribe), color = ur.muted, style = MaterialTheme.typography.bodySmall) else {
                    val product = catalog?.optJSONObject("sync")
                    if (catalog == null) PrimaryButton(stringResource(R.string.cloud_show_price), { work { catalog = billing.products() } }, Modifier.fillMaxWidth(), enabled = !busy)
                    if (product != null) PrimaryButton(stringResource(R.string.cloud_subscribe, product.text("price")), { work { purchaseMessage(billing.subscribe()) } }, Modifier.fillMaxWidth(), enabled = !busy)
                    catalog?.optJSONObject("errors")?.text("sync")?.takeIf(String::isNotBlank)?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = ur.negative) }
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        TextButton({ work { purchaseMessage(billing.restore()) } }, enabled = !busy) { Text(stringResource(R.string.cloud_restore_purchases)) }
                        TextButton({ work { billing.manage() } }, enabled = !busy) { Text(stringResource(R.string.cloud_manage_subscription)) }
                    }
                }
            }
        }
        item { SectionTitle(stringResource(R.string.cloud_encrypted_sync), stringResource(R.string.cloud_encrypted_sync_detail)) }
        if (signedIn.isNotBlank()) {
            item {
                ListCard {
                    overview?.optJSONObject("sync")?.let { ListRow(if (it.optBoolean("active")) stringResource(R.string.cloud_subscription_active) else stringResource(R.string.cloud_subscription_needed),
                        icon = Icons.Outlined.CloudSync, iconTint = if (it.optBoolean("active")) ur.positive else ur.warning, iconFill = if (it.optBoolean("active")) ur.positiveSoft else ur.warningSoft, chevron = false, divider = true) }
                    SwitchRow(stringResource(R.string.cloud_sync_automatically), automatic, { automatic = it; preferences.automaticSync = it }, subtitle = stringResource(R.string.cloud_while_open), divider = true)
                    if (sync?.optBoolean("unlocked") == true) {
                        ListRow(stringResource(R.string.cloud_sync_now), icon = Icons.Outlined.Sync, onClick = { if (!busy) call("sync_now") }, chevron = false, divider = true)
                        ListRow(stringResource(R.string.cloud_lock), icon = Icons.Outlined.Lock, iconTint = ur.text2, iconFill = ur.zeroSoft, onClick = { if (!busy) call("sync_lock") }, chevron = false)
                    } else ListRow(stringResource(R.string.cloud_set_up_or_unlock), icon = Icons.Outlined.Key, onClick = { if (!busy) call("sync_has_key") { exists -> phrase = ""; repeated = ""; dialog = if (exists == true) "unlock" else "setup" } })
                }
            }
            if (report != null && (report.text("message").isNotBlank() || report.text("at").isNotBlank())) item {
                Text(Money.joined(listOfNotNull(report.text("message").ifBlank { null }, report.text("at").ifBlank { null }?.let { stringResource(R.string.cloud_last_attempt, it) })),
                    Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall)
            }
            if (reportedBudgets.isNotEmpty()) item {
                ListCard {
                    reportedBudgets.forEachIndexed { index, entry ->
                        val state = entry.text("state")
                        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(entry.text("name"), Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold, color = ur.text)
                                TagChip(syncStateLabel(state), fill = if (state == "conflict") ur.warningSoft else ur.zeroSoft, color = if (state == "conflict") ur.warning else ur.text2)
                            }
                            entry.text("message").takeIf(String::isNotBlank)?.let { Text(it, color = ur.text2, style = MaterialTheme.typography.bodyMedium) }
                            entry.optJSONArray("notes")?.let { notes -> for (i in 0 until notes.length()) Text(notes.optString(i), style = MaterialTheme.typography.bodySmall, color = ur.muted) }
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                                when (state) {
                                    "cloud_only" -> SecondaryButton(stringResource(R.string.cloud_download_open), { call("sync_download", JSONObject().put("id", entry.text("id"))) { id -> vm.service("open_budget", JSONObject().put("id", id)) } }, enabled = !busy)
                                    "local_only" -> SecondaryButton(stringResource(R.string.cloud_start_syncing), { call("sync_track", JSONObject().put("ids", JSONArray().put(entry.text("id")))) { vm.service("sync_now") } }, enabled = !busy)
                                    "conflict" -> PrimaryButton(stringResource(R.string.cloud_review_differences), { pendingBudget = entry; dialog = "conflict" }, enabled = !busy, color = ur.warning)
                                }
                                if (state != "local_only") TextButton({ pendingBudget = entry; dialog = "removeCloud" }, enabled = !busy) { Text(stringResource(R.string.cloud_remove_copy), color = ur.negative) }
                            }
                        }
                        if (index < reportedBudgets.lastIndex) Hairline(start = 16.dp)
                    }
                }
            }
            item {
                ListCard {
                    ListRow(stringResource(R.string.cloud_reset_passphrase), onClick = { if (!busy) { phrase = ""; repeated = ""; dialog = "reset" } }, divider = true)
                    ListRow(stringResource(R.string.cloud_delete_account), titleColor = ur.negative, onClick = { if (!busy) { confirmation = ""; dialog = "deleteAccount" } })
                }
            }
        } else item { Text(stringResource(R.string.cloud_sign_in_to_sync), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium) }
        item { TextButton({ call("account_open", JSONObject().put("next", "/privacy/app/")) }, enabled = !busy) { Text(stringResource(R.string.cloud_privacy)) } }
    }
    if (dialog in setOf("setup", "unlock", "reset")) FormDialog(stringResource(if (dialog == "reset") R.string.cloud_reset_title else if (dialog == "setup") R.string.cloud_setup_title else R.string.cloud_unlock_title),
        { dialog = ""; phrase = ""; repeated = "" }, confirm = stringResource(if (dialog == "reset") R.string.cloud_reset_confirm else R.string.common_continue), enabled = !busy && phrase.length >= 8 && (dialog == "unlock" || phrase == repeated), onConfirm = {
        val method = if (dialog == "reset") "sync_reset" else "sync_setup"
        call(method, JSONObject().put("passphrase", phrase)) { dialog = ""; phrase = ""; repeated = ""; vm.service("sync_now") }
    }) {
        if (dialog == "reset") Text(stringResource(R.string.cloud_reset_message))
        else Text(stringResource(R.string.cloud_passphrase_safe))
        FilledField(phrase, { phrase = it }, label = { Text(stringResource(R.string.cloud_passphrase_field)) }, visualTransformation = PasswordVisualTransformation(), singleLine = true)
        if (dialog != "unlock") FilledField(repeated, { repeated = it }, label = { Text(stringResource(R.string.cloud_passphrase_repeat)) }, visualTransformation = PasswordVisualTransformation(), singleLine = true)
    }
    if (dialog == "deleteAccount") FormDialog(stringResource(R.string.cloud_delete_title), { dialog = "" }, stringResource(R.string.cloud_delete_confirm), enabled = !busy && confirmation.trim().equals(signedIn, ignoreCase = true), onConfirm = {
        call("account_delete", JSONObject().put("confirm", confirmation)) { dialog = "" }
    }) { Text(stringResource(R.string.cloud_delete_message)); Field(confirmation, { confirmation = it }, stringResource(R.string.cloud_delete_type, signedIn)) }
    if (dialog == "removeCloud") AlertDialog(onDismissRequest = { dialog = "" }, title = { Text(stringResource(R.string.cloud_remove_title)) }, text = { Text(stringResource(R.string.cloud_remove_message, pendingBudget?.text("name").orEmpty())) }, confirmButton = { TextButton({
        call("sync_remove", JSONObject().put("id", pendingBudget!!.text("id"))) { dialog = ""; vm.service("sync_now") }
    }, enabled = !busy) { Text(stringResource(R.string.common_remove)) } }, dismissButton = { TextButton({ dialog = "" }) { Text(stringResource(R.string.common_cancel)) } })
    if (dialog == "conflict" && pendingBudget != null) SyncConflictDialog(pendingBudget!!, busy, { dialog = "" }) { keep, choices ->
        call("sync_resolve", JSONObject().put("id", pendingBudget!!.text("id")).put("keep", keep).put("choices", choices)) { dialog = "" }
    }
}

/** A white card with its contents padded, for a section of controls. */
@Composable
private fun CloudCard(content: @Composable ColumnScope.() -> Unit) {
    ListCard { Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp), content = content) }
}

/** A budget's sync state as its chip names it (sync.rs's BudgetSync states). */
@Composable
private fun syncStateLabel(state: String): String = when (state) {
    "cloud_only" -> stringResource(R.string.sync_state_cloud_only); "local_only" -> stringResource(R.string.sync_state_local_only)
    "conflict" -> stringResource(R.string.sync_state_conflict); "synced" -> stringResource(R.string.sync_state_synced)
    "uploaded" -> stringResource(R.string.sync_state_uploaded); "downloaded" -> stringResource(R.string.sync_state_downloaded)
    "merged" -> stringResource(R.string.sync_state_merged); "waiting" -> stringResource(R.string.sync_state_waiting)
    "update_app" -> stringResource(R.string.sync_state_update_app); "error" -> stringResource(R.string.sync_state_error)
    "" -> stringResource(R.string.sync_state_unknown)
    else -> state.replace('_', ' ').replaceFirstChar(Char::uppercase)
}

@Composable
private fun SyncConflictDialog(entry: JSONObject, busy: Boolean, close: () -> Unit, resolve: (String, JSONObject) -> Unit) {
    val conflicts = remember(entry) { entry.rows("conflicts") }
    var choices by remember(entry) { mutableStateOf(mapOf<String, String>()) }
    var whole by remember { mutableStateOf("") }
    FormDialog(stringResource(R.string.conflict_title, entry.text("name")), close, stringResource(R.string.conflict_apply), enabled = !busy && (if (conflicts.isEmpty()) whole.isNotBlank() else conflicts.all { choices.containsKey(it.text("key")) }), onConfirm = {
        resolve(if (conflicts.isEmpty()) whole else "merge", JSONObject(choices))
    }) {
        Text(stringResource(R.string.conflict_help))
        if (conflicts.isEmpty()) Choice(stringResource(R.string.conflict_keep_whole), whole, listOf("local" to stringResource(R.string.conflict_local_replaces), "cloud" to stringResource(R.string.conflict_cloud_replaces)), { whole = it })
        if (conflicts.isNotEmpty()) LazyColumn(Modifier.fillMaxWidth().heightIn(max = 420.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            items(conflicts, key = { it.text("key") }) { difference ->
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(Money.joined(listOf(difference.text("label"), difference.text("field"))), fontWeight = FontWeight.Bold)
            Text(stringResource(R.string.conflict_this_device_value, (difference.opt("local") ?: stringResource(R.string.conflict_deleted)).toString()))
            Text(stringResource(R.string.conflict_cloud_value, (difference.opt("remote") ?: stringResource(R.string.conflict_deleted)).toString()))
            Choice(stringResource(R.string.conflict_keep), choices[difference.text("key")].orEmpty(), listOf("local" to stringResource(R.string.conflict_this_device), "cloud" to stringResource(R.string.conflict_cloud)), { choices = choices + (difference.text("key") to it) })
            }
            }
        }
    }
}
