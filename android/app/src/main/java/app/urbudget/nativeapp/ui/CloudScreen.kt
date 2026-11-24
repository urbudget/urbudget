package app.urbudget.nativeapp.ui

import android.app.Activity
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.ui.Alignment
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextDirection
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
    val toast = LocalToast.current
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
    var confirmation by remember { mutableStateOf("") }
    var pendingBudget by remember { mutableStateOf<JSONObject?>(null) }
    // What the account's sync key is, as the engine last answered `sync_key_info` while sync is locked here.
    var keyInfo by remember { mutableStateOf<JSONObject?>(null) }
    var askingKey by remember { mutableStateOf(false) }
    // A recovery code the person hasn't said they've saved, and the one they last said they had.
    var recoveryCode by remember { mutableStateOf("") }
    var savedCode by remember { mutableStateOf("") }
    // What's typed to unlock sync, and what went wrong with it, said beside the box rather than up the page.
    var secret by remember { mutableStateOf("") }
    var unlockError by remember { mutableStateOf<String?>(null) }
    var scanning by remember { mutableStateOf(false) }
    // Sync's seldom used rows (a new recovery code, reset, lock) stay folded away until asked for.
    var more by remember { mutableStateOf(false) }
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
        var syncReport: JSONObject? = null
        try {
            val result = withContext(Dispatchers.IO) {
                invoked = true
                NativeServices.call(method, params)
            }
            if (method in setOf("sync_now", "sync_resolve")) syncReport = result as? JSONObject
            // A key made here, or a code that replaces a lost one: shown until it's saved (RecoveryCodeDialog).
            recoveryCodeIn(result)?.let { recoveryCode = it }
            done(result)
        } finally {
            // JNI can complete a sync commit after this screen is disposed. Queue the
            // revision probe even when cancellation prevents delivering the result.
            if (invoked && NativeServicePolicy.budgetResult(method)) {
                val report = syncReport
                if (report != null) withContext(Dispatchers.IO) { vm.refreshAfterSync(report) } else vm.refresh()
            }
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
    val unlocked = sync?.optBoolean("unlocked") == true
    // While sync is locked here, what this screen offers depends on the account's key, asked fresh:
    // turning sync on for an account that has none, or unlocking with what its key is made from.
    LaunchedEffect(signedIn, unlocked) {
        keyInfo = null
        if (signedIn.isBlank()) { secret = ""; unlockError = null }
        else if (!unlocked) try { askingKey = true; keyInfo = NativeServices.read("sync_key_info") as? JSONObject }
            catch (cancel: CancellationException) { throw cancel }
            catch (_: Exception) { /* Offline: the row asks again when it's tapped. */ }
            finally { askingKey = false }
    }
    val key = syncKey(status, keyInfo)
    val secretKind = syncSecret(status, keyInfo)
    // Turning sync on is offered to an account with no key, unless it's known to have no plan to sync with.
    val turnOn = key == SyncKey.None && overview?.optJSONObject("sync")?.let { !it.optBoolean("active") } != true
    val askKey = key == SyncKey.Unknown && !askingKey
    // The engine keeps a recovery code on this device until it's confirmed saved, so one that was
    // never confirmed (the app closed, the answer was lost) is shown again when this screen opens.
    val unsaved = sync?.optBoolean("recoveryUnsaved") == true
    LaunchedEffect(unsaved) {
        if (unsaved && recoveryCode.isBlank()) try {
            recoveryCodeIn(NativeServices.read("sync_recovery_unsaved"))?.takeIf { it != savedCode }?.let { recoveryCode = it }
        } catch (cancel: CancellationException) { throw cancel }
        catch (_: Exception) { /* Asked for again the next time the status says one is kept. */ }
    }
    /** Unlocks sync here with whatever was scanned or typed: the engine works out which kind of code, or a passphrase, it is. */
    fun unlock(given: String) = work {
        unlockError = null
        try {
            val answer = withContext(Dispatchers.IO) { NativeServices.call("sync_unlock", JSONObject().put("secret", given)) }
            recoveryCodeIn(answer)?.let { recoveryCode = it }
            secret = ""
            vm.service("sync_now")
        } catch (cancel: CancellationException) { throw cancel }
        catch (failure: Exception) { unlockError = failure.message ?: context.getString(R.string.common_try_again) }
    }
    /** The person says the code is saved, so it's put away even if the engine couldn't let go of it just now: then it shows again the next time this screen opens. */
    fun codeSaved() {
        val code = recoveryCode
        work {
            try { withContext(Dispatchers.IO) { NativeServices.call("sync_recovery_saved") } }
            finally { savedCode = code; recoveryCode = "" }
        }
    }
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
    PageList(Modifier.fillMaxSize(), contentPadding = PaddingValues(start = Gutter, end = Gutter, top = 4.dp, bottom = DockRoom), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item { Text(stringResource(R.string.cloud_intro), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium) }
        if (syncMoveOffered(status)) item {
            CloudCard {
                Text(stringResource(R.string.cloud_move_title), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = ur.text)
                Text(stringResource(R.string.cloud_move_detail), color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                PrimaryButton(stringResource(R.string.cloud_move_action), { if (!busy) { phrase = ""; error = null; dialog = "move" } }, Modifier.fillMaxWidth(), enabled = !busy)
            }
        }
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
                    SwitchRow(stringResource(R.string.cloud_sync_automatically), automatic, { automatic = it; preferences.automaticSync = it }, subtitle = stringResource(R.string.cloud_while_open), divider = key == SyncKey.Unlocked || turnOn || askKey)
                    when (key) {
                        SyncKey.Unlocked -> {
                            ListRow(stringResource(R.string.cloud_sync_now), icon = Icons.Outlined.Sync, onClick = { if (!busy) call("sync_now") }, chevron = false, divider = true)
                            ListRow(stringResource(R.string.pair_add_device), icon = Icons.Outlined.Devices, onClick = { if (!busy) { error = null; dialog = "addDevice" } }, divider = true)
                            // What few people need, and nobody often, folds away.
                            ListRow(stringResource(R.string.cloud_more), icon = Icons.Outlined.MoreHoriz, iconTint = ur.text2, iconFill = ur.zeroSoft, onClick = { more = !more }, chevron = false, divider = more,
                                trailing = { Icon(if (more) Icons.Outlined.ExpandLess else Icons.Outlined.ExpandMore, null, Modifier.size(20.dp), tint = ur.muted) })
                            if (more) {
                                // Under More's own words, past its icon. A replacement code needs the new sync (cloud/recovery.rs).
                                if (sync?.optInt("protocol", 1) == 2) ListRow(stringResource(R.string.cloud_new_recovery_code), Modifier.padding(start = 52.dp), onClick = { if (!busy) { error = null; dialog = "newCode" } }, divider = true)
                                ListRow(stringResource(R.string.cloud_reset_sync), Modifier.padding(start = 52.dp), onClick = { if (!busy) { error = null; dialog = "reset" } }, divider = true)
                                ListRow(stringResource(R.string.cloud_lock), Modifier.padding(start = 52.dp), onClick = { if (!busy) call("sync_lock") }, chevron = false)
                            }
                        }
                        // The account has a key: the card below unlocks it.
                        SyncKey.Locked -> {}
                        // One tap: the key is made here with its recovery code, which comes back to be shown. Without the plan there's nothing to turn on yet.
                        SyncKey.None -> if (turnOn) ListRow(stringResource(R.string.cloud_turn_on_sync), icon = Icons.Outlined.Key, onClick = { if (!busy) call("sync_setup") { vm.service("sync_now") } }, chevron = false)
                        // The engine couldn't say (offline, most likely): tapping asks again and says what's wrong.
                        SyncKey.Unknown -> if (askKey) ListRow(stringResource(R.string.cloud_set_up_or_unlock), icon = Icons.Outlined.Key, onClick = { if (!busy) work { keyInfo = NativeServices.read("sync_key_info") as? JSONObject } })
                    }
                }
            }
            if (key == SyncKey.Locked) item {
                CloudCard {
                    Text(stringResource(R.string.cloud_locked_title), style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold, color = ur.text)
                    Text(stringResource(R.string.cloud_locked_detail), color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                    // Stretching a typed code into a key takes a moment, and this card may be all that's in view.
                    if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
                    unlockError?.let { FormError(it) }
                    PrimaryButton(stringResource(R.string.cloud_scan_code), {
                        unlockError = null
                        scanning = true
                    }, Modifier.fillMaxWidth(), enabled = !busy, icon = Icons.Outlined.QrCodeScanner)
                    // One box for whatever they have: the engine tells a device code, a recovery code and a passphrase apart.
                    val label = stringResource(syncUnlockLabel(secretKind))
                    Text(label, color = ur.text2, style = MaterialTheme.typography.bodyMedium)
                    SecretField(secret, { secret = it; unlockError = null }, label, hidden = secretKind == "passphrase") { if (!busy && secret.isNotBlank()) unlock(secret) }
                    SecondaryButton(stringResource(R.string.cloud_unlock), { unlock(secret) }, Modifier.fillMaxWidth(), enabled = !busy && secret.isNotBlank())
                    TextButton({ if (!busy) { error = null; dialog = "reset" } }, contentPadding = PaddingValues(horizontal = 0.dp, vertical = 8.dp)) { Text(stringResource(R.string.cloud_cant_unlock), color = ur.muted) }
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
                                    "cloud_only" -> if (sync?.optInt("protocol", 1) != 2) SecondaryButton(stringResource(R.string.cloud_download_open), { call("sync_download", JSONObject().put("id", entry.text("id"))) { id -> vm.service("open_budget", JSONObject().put("id", id)) } }, enabled = !busy)
                                    // A budget kept only on this device, on either sync: one tap adds it to the account.
                                    "local_only" -> SecondaryButton(stringResource(R.string.cloud_start_syncing), {
                                        val ids = JSONArray().put(entry.text("id"))
                                        if (sync?.optInt("protocol", 1) == 2) call("sync_enable", JSONObject().put("enabled", true).put("ids", ids)) { vm.service("sync_now") }
                                        else call("sync_track", JSONObject().put("ids", ids)) { vm.service("sync_now") }
                                    }, enabled = !busy)
                                    "conflict" -> PrimaryButton(stringResource(R.string.cloud_review_differences), { pendingBudget = entry; dialog = "conflict" }, enabled = !busy, color = ur.warning)
                                }
                                if (sync?.optInt("protocol", 1) != 2 && state != "local_only") TextButton({ pendingBudget = entry; dialog = "removeCloud" }, enabled = !busy) { Text(stringResource(R.string.cloud_remove_copy), color = ur.negative) }
                            }
                        }
                        if (index < reportedBudgets.lastIndex) Hairline(start = 16.dp)
                    }
                }
            }
            item {
                ListCard {
                    ListRow(stringResource(R.string.cloud_delete_account), titleColor = ur.negative, onClick = { if (!busy) { confirmation = ""; dialog = "deleteAccount" } })
                }
            }
        } else item { Text(stringResource(R.string.cloud_sign_in_to_sync), Modifier.padding(horizontal = 4.dp), color = ur.muted, style = MaterialTheme.typography.bodyMedium) }
        item { TextButton({ call("account_open", JSONObject().put("next", "/privacy/app/")) }, enabled = !busy) { Text(stringResource(R.string.cloud_privacy)) } }
    }
    if (dialog == "addDevice") AddDeviceDialog({ dialog = "" }) { dialog = ""; toast(context.getString(R.string.pair_device_added), false) }
    // What was scanned goes to the engine as it is: it says what the code is, and what's wrong with one that isn't UrBudget's.
    if (scanning) SyncCodeScanner(
        found = { scanning = false; unlock(it) },
        unavailable = { scanning = false; unlockError = context.getString(R.string.pair_scanner_unavailable) },
        close = { scanning = false })
    // Nothing to choose or type: a reset makes the new key here, and its recovery code comes back to be shown.
    if (dialog == "reset") AlertDialog(onDismissRequest = { if (!busy) dialog = "" }, title = { Text(stringResource(R.string.cloud_reset_title)) },
        text = { Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(stringResource(R.string.cloud_reset_message))
            // The new sync keeps each budget that was syncing as a copy to bring back from the budget library, on every
            // device, and uploads nothing by itself. The older one uploads this device's budgets again.
            Text(stringResource(if (syncProtocol(status, keyInfo) == 2) R.string.cloud_reset_keeps_recover else R.string.cloud_reset_keeps_upload))
            Text(stringResource(R.string.cloud_reset_devices))
            // Asked from a device that can't unlock: there may be a way that deletes nothing.
            if (!unlocked) Text(stringResource(R.string.cloud_reset_instead))
            error?.let { FormError(it) }
        } },
        confirmButton = { TextButton({ call("sync_reset") { dialog = ""; vm.service("sync_now") } }, enabled = !busy) { Text(stringResource(R.string.cloud_reset_sync), color = ur.negative) } },
        dismissButton = { TextButton({ dialog = "" }, enabled = !busy) { Text(stringResource(R.string.common_cancel)) } })
    // The code or passphrase the key was made from can't be shown again, so a lost one is replaced. Only a reset retires it.
    if (dialog == "newCode") AlertDialog(onDismissRequest = { if (!busy) dialog = "" }, title = { Text(stringResource(R.string.cloud_new_code_title)) },
        text = { Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(stringResource(R.string.cloud_new_code_message))
            // The server keeps one replacement at a time (`recovery` in the status).
            if (sync?.optBoolean("recovery") == true) Text(stringResource(R.string.cloud_new_code_replaces))
            error?.let { FormError(it) }
        } },
        confirmButton = { TextButton({ call("sync_recovery_create") { dialog = "" } }, enabled = !busy) { Text(stringResource(R.string.cloud_new_code_confirm)) } },
        dismissButton = { TextButton({ dialog = "" }, enabled = !busy) { Text(stringResource(R.string.common_cancel)) } })
    if (dialog == "move") {
        // Moving is the person's choice: devices on older versions stop syncing until updated.
        val needsPassphrase = syncMoveNeedsPassphrase(status)
        FormDialog(stringResource(R.string.cloud_move_confirm_title), { dialog = ""; phrase = "" }, confirm = stringResource(R.string.cloud_move_action),
            enabled = !busy && (!needsPassphrase || phrase.length >= 8), onConfirm = {
            call("sync_upgrade", syncMoveParams(status, phrase)) { dialog = ""; phrase = ""; notice = context.getString(R.string.cloud_move_done); vm.service("sync_now") }
        }) {
            Text(stringResource(R.string.cloud_move_warning))
            if (needsPassphrase) FilledField(phrase, { phrase = it }, label = { Text(stringResource(R.string.cloud_passphrase_field)) }, visualTransformation = PasswordVisualTransformation(), singleLine = true)
            // A saved key from before a passphrase reset elsewhere fails here; the status poll then asks for the passphrase.
            error?.let { FormError(it) }
        }
    }
    if (dialog == "deleteAccount") FormDialog(stringResource(R.string.cloud_delete_title), { dialog = "" }, stringResource(R.string.cloud_delete_confirm), enabled = !busy && confirmation.trim().equals(signedIn, ignoreCase = true), onConfirm = {
        call("account_delete", JSONObject().put("confirm", confirmation)) { dialog = "" }
    }) { Text(stringResource(R.string.cloud_delete_message)); Field(confirmation, { confirmation = it }, stringResource(R.string.cloud_delete_type, signedIn)) }
    if (dialog == "removeCloud") AlertDialog(onDismissRequest = { dialog = "" }, title = { Text(stringResource(R.string.cloud_remove_title)) }, text = { Text(stringResource(R.string.cloud_remove_message, pendingBudget?.text("name").orEmpty())) }, confirmButton = { TextButton({
        call("sync_remove", JSONObject().put("id", pendingBudget!!.text("id"))) { dialog = ""; vm.service("sync_now") }
    }, enabled = !busy) { Text(stringResource(R.string.common_remove)) } }, dismissButton = { TextButton({ dialog = "" }) { Text(stringResource(R.string.common_cancel)) } })
    if (dialog == "conflict" && pendingBudget != null) SyncConflictDialog(pendingBudget!!, busy, { dialog = "" }) { keep, choices ->
        call("sync_resolve", JSONObject().put("id", pendingBudget!!.text("id")).put("keep", keep).put("choices", choices).put("review", pendingBudget!!.opt("review") ?: JSONObject.NULL)) { dialog = "" }
    }
    // Last, so it rises over whatever else is open.
    if (recoveryCode.isNotBlank()) RecoveryCodeDialog(recoveryCode, busy) { codeSaved() }
}

/**
 * The box a sync code or passphrase is typed into: left to right in any language, and with no
 * suggestions or corrections from the keyboard. A passphrase is [hidden] as it's typed until the
 * eye is tapped; a code is 24 characters to check against another screen, so it shows.
 */
@Composable
private fun SecretField(value: String, onChange: (String) -> Unit, label: String, hidden: Boolean, onDone: () -> Unit) {
    var shown by remember(hidden) { mutableStateOf(!hidden) }
    TextField(value, onChange, Modifier.fillMaxWidth().semantics { contentDescription = label }, singleLine = true, shape = RoundedCornerShape(12.dp), colors = fieldColors(),
        textStyle = LocalTextStyle.current.copy(fontFamily = FontFamily.Monospace, textDirection = TextDirection.Ltr),
        visualTransformation = if (shown) VisualTransformation.None else PasswordVisualTransformation(),
        trailingIcon = if (hidden) ({ IconButton({ shown = !shown }) {
            Icon(if (shown) Icons.Outlined.VisibilityOff else Icons.Outlined.Visibility, stringResource(if (shown) R.string.cloud_hide_typed else R.string.cloud_show_typed), tint = ur.muted)
        } }) else null,
        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = { onDone() }))
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
    "deletion_conflict" -> stringResource(R.string.library_deletion_conflict)
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
            Text(Money.joined(listOf(difference.text("label"), if (difference.text("field") == "budgetVersion") stringResource(R.string.conflict_whole_budget) else difference.text("field"))), fontWeight = FontWeight.Bold)
            if (difference.optJSONObject("about")?.optBoolean("wholeBudget") == true) Text(stringResource(R.string.conflict_whole_budget_help))
            Text(stringResource(R.string.conflict_this_device_value, describeSyncConflict(difference.opt("local"))))
            Text(stringResource(R.string.conflict_cloud_value, describeSyncConflict(difference.opt("remote"))))
            Choice(stringResource(R.string.conflict_keep), choices[difference.text("key")].orEmpty(), listOf("local" to stringResource(R.string.conflict_this_device), "cloud" to stringResource(R.string.conflict_cloud)), { choices = choices + (difference.text("key") to it) })
            }
            }
        }
    }
}

internal fun describeSyncConflict(value: Any?): String {
    if (value == null || value == JSONObject.NULL) return Words[R.string.conflict_deleted]
    val plan = value as? JSONObject ?: return value.toString()
    if (plan.has("amount") && (plan.has("lines") || plan.has("transfer"))) {
        fun state(value: String) = when (value) { "cleared" -> Words[R.string.status_cleared]; "reconciled" -> Words[R.string.status_reconciled]; "uncleared" -> Words[R.string.status_uncleared]; else -> value }
        val parts = mutableListOf(plan.optString("account"), Money.format(plan.optLong("amount")))
        if (!plan.isNull("transfer") && plan.optString("transfer").isNotBlank()) parts.add(Words[R.string.conflict_transfer_account, plan.optString("transfer")])
        plan.optJSONArray("lines")?.let { lines -> for (i in 0 until lines.length()) {
            val line = lines.getJSONObject(i)
            val destination = if (!line.isNull("transfer")) Words[R.string.conflict_transfer_account,line.optString("transfer")] else if (!line.isNull("category")) line.optString("category") else Words[R.string.conflict_ready_to_assign]
            parts.add(destination + ": " + Money.format(line.optLong("amount")))
        } }
        if (plan.optString("status").isNotBlank()) parts.add(state(plan.optString("status")))
        if (plan.optString("transferStatus").isNotBlank()) parts.add(Words[R.string.conflict_transfer_status,state(plan.optString("transferStatus"))])
        return parts.filter { it.isNotBlank() }.joinToString("\n")
    }
    if (plan.has("reason") && plan.has("transactions")) return listOf(plan.optString("name"),
        Words[R.string.conflict_budget_counts, plan.optInt("accounts").toString(), plan.optInt("categories").toString(), plan.optInt("transactions").toString()],
        plan.optString("reason")).joinToString("\n")
    if (plan.optString("type") != "monthPlan") return value.toString()
    val currency = plan.optString("currency", "SAR")
    fun amount(value: Long) = Money.format(value, currency)
    val rows = plan.optJSONArray("categories") ?: JSONArray()
    return (listOf(
        Words[R.string.conflict_month_total, amount(plan.optLong("baseTotal")), amount(plan.optLong("total"))],
        Words[R.string.conflict_ready_change, amount(plan.optLong("readyToAssignChange"))],
        Words[R.string.conflict_moves, plan.optInt("moveCount").toString()]
    ) + (0 until rows.length()).map { i ->
        val row = rows.getJSONObject(i)
        row.optString("name") + ": " + amount(row.optLong("base")) + " → " + amount(row.optLong("amount")) + " (" + Words[R.string.conflict_amount_change, amount(row.optLong("delta"))] + ")"
    }).joinToString("\n")
}