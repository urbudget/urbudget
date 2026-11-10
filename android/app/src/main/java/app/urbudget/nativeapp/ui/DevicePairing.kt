package app.urbudget.nativeapp.ui

import android.content.Context
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.NativeServices
import app.urbudget.nativeapp.R
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

// Adding a device without typing the sync passphrase (docs/plans/2026-11-10-sync-device-approval.md).
// A one-time code goes from one screen to the other by camera or keyboard, and the shared engine
// uses it to hand the sync key over; the server never sees the code. The phone is the one with a
// camera, so it scans: a new phone scans the code a device that syncs shows, and a phone that
// syncs scans the code a new computer shows, then approves it.

/** How long a code is good for, as the engine and the server keep it. */
private const val CODE_LIFETIME_MS = 5 * 60_000L

/** Opens Google's code scanner, which shows its own camera screen and needs no camera permission
 *  in this app. `found` gets the QR code's text; closing the scanner calls neither. */
private fun scanCode(context: Context, found: (String) -> Unit, failed: () -> Unit) {
    val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
    try {
        GmsBarcodeScanning.getClient(context, options).startScan()
            .addOnSuccessListener { code -> code.rawValue?.let(found) ?: failed() }
            .addOnFailureListener { failed() }
    } catch (_: Exception) {
        // No Play services on this phone: the code can still be typed.
        failed()
    }
}

/** The engine's QR code: `modules` is `size`×`size` characters, row by row, `1` for dark. Always
 *  dark on white with the four-square margin scanners expect, whatever the app's theme. */
@Composable
private fun QrCode(qr: JSONObject, description: String) {
    val size = qr.optInt("size")
    val modules = qr.optString("modules")
    if (size <= 0 || modules.length != size * size) return
    Canvas(Modifier.size(220.dp).semantics { contentDescription = description }) {
        drawRect(Color.White)
        val cell = this.size.minDimension / (size + 8)
        for (y in 0 until size) for (x in 0 until size) if (modules[y * size + x] == '1') {
            // A hair wider than a cell, so neighbours leave no light seam between them.
            drawRect(Color.Black, Offset((x + 4) * cell, (y + 4) * cell), Size(cell + 0.5f, cell + 0.5f))
        }
    }
}

/** A code's 24 characters, to read or type: left to right in any language. */
@Composable
internal fun CodeText(code: String) {
    SelectionContainer {
        Text(code, style = MaterialTheme.typography.titleMedium.copy(fontFamily = FontFamily.Monospace, textDirection = TextDirection.Ltr),
            fontWeight = FontWeight.SemiBold, color = ur.text, textAlign = TextAlign.Center)
    }
}

/** A code as people type it has its 24 characters, with or without the hyphens. */
internal fun codeComplete(code: String) = code.count { !it.isWhitespace() && it != '-' } >= 24

/** On a phone that syncs: shows a code for a new phone to scan (or to type on a computer), and
 *  scans the code a new computer shows. The shown code is taken back when this closes. */
@Composable
internal fun AddDeviceDialog(close: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var offer by remember { mutableStateOf<JSONObject?>(null) }
    var expired by remember { mutableStateOf(false) }
    var attempt by remember { mutableIntStateOf(0) }
    var error by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    var approving by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    LaunchedEffect(attempt) {
        offer = null; expired = false
        try {
            val made = withContext(Dispatchers.IO) { NativeServices.call("sync_pair_offer") as JSONObject }
            offer = made
            delay(CODE_LIFETIME_MS)
            expired = true
        } catch (cancel: CancellationException) { throw cancel }
        catch (failure: Exception) { error = failure.message ?: context.getString(R.string.common_try_again) }
    }
    // Withdrawn outside this screen's scope, which is gone by the time it closes.
    val shown = offer?.optString("id").orEmpty()
    DisposableEffect(shown) {
        onDispose {
            if (shown.isNotBlank()) CoroutineScope(Dispatchers.IO).launch {
                runCatching { NativeServices.call("sync_pair_cancel", JSONObject().put("id", shown)) }
            }
        }
    }
    FormDialog(stringResource(R.string.pair_add_device), close) {
        Text(stringResource(R.string.pair_add_intro))
        FormError(error)
        notice?.let { Notice(it, tone = "positive") }
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
            val made = offer
            when {
                expired -> {
                    Text(stringResource(R.string.pair_code_expired), color = ur.text2)
                    SecondaryButton(stringResource(R.string.pair_new_code), { error = null; attempt++ })
                }
                made == null -> if (error == null) CircularProgressIndicator(Modifier.size(28.dp), color = ur.accent)
                    else SecondaryButton(stringResource(R.string.pair_new_code), { error = null; attempt++ })
                else -> {
                    QrCode(made.optJSONObject("qr") ?: JSONObject(), stringResource(R.string.pair_qr_description))
                    CodeText(made.optString("code"))
                    Text(stringResource(R.string.pair_code_warning), color = ur.muted, style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Center)
                }
            }
        }
        // A computer can't scan, so it shows a code of its own and the phone scans that. If the
        // scanner can't open, the computer takes this phone's code typed, as a phone would.
        SecondaryButton(stringResource(R.string.pair_scan_new_device), {
            error = null; notice = null
            scanCode(context, { approving = it }, { error = context.getString(R.string.pair_scanner_unavailable_computer) })
        }, Modifier.fillMaxWidth(), enabled = !busy, icon = Icons.Outlined.QrCodeScanner)
    }
    // The key goes to whoever shows this code, so the person says yes first.
    approving?.let { code ->
        AlertDialog(onDismissRequest = { if (!busy) approving = null },
            title = { Text(stringResource(R.string.pair_approve_title)) },
            text = { Text(stringResource(R.string.pair_approve_message)) },
            confirmButton = { TextButton({
                busy = true
                scope.launch {
                    try {
                        withContext(Dispatchers.IO) { NativeServices.call("sync_pair_approve", JSONObject().put("code", code)) }
                        notice = context.getString(R.string.pair_approved)
                    } catch (cancel: CancellationException) { throw cancel }
                    catch (failure: Exception) { error = failure.message ?: context.getString(R.string.common_try_again) }
                    finally { busy = false; approving = null }
                }
            }, enabled = !busy) { Text(stringResource(R.string.pair_approve)) } },
            dismissButton = { TextButton({ approving = null }, enabled = !busy) { Text(stringResource(R.string.common_cancel)) } })
    }
}

/** On a phone that doesn't sync yet: unlocks sync with a code. Scanning takes the one a device
 *  that syncs shows; the box takes that one typed or, when the account has one (`recovery`),
 *  the recovery code. The engine works out which it was given. */
@Composable
internal fun UnlockWithCodeDialog(recovery: Boolean, close: () -> Unit, unlocked: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var typed by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    fun accept(code: String) {
        if (busy) return
        busy = true; error = null
        scope.launch {
            try {
                withContext(Dispatchers.IO) { NativeServices.call("sync_unlock_code", JSONObject().put("code", code)) }
                unlocked()
            } catch (cancel: CancellationException) { throw cancel }
            catch (failure: Exception) { error = failure.message ?: context.getString(R.string.common_try_again) }
            finally { busy = false }
        }
    }
    // Closing always closes: a sheet that swallowed the swipe would stay, hidden, over the screen.
    FormDialog(stringResource(R.string.pair_unlock_title), close, confirm = stringResource(R.string.pair_unlock_confirm),
        onConfirm = { accept(typed) }, enabled = !busy && codeComplete(typed)) {
        Text(stringResource(R.string.pair_unlock_intro))
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth(), color = ur.accent)
        FormError(error)
        PrimaryButton(stringResource(R.string.pair_scan_code), {
            error = null
            scanCode(context, ::accept) { error = context.getString(R.string.pair_scanner_unavailable) }
        }, Modifier.fillMaxWidth(), enabled = !busy, icon = Icons.Outlined.QrCodeScanner)
        Field(typed, { typed = it }, stringResource(if (recovery) R.string.pair_code_field_recovery else R.string.pair_code_field), ltr = true)
    }
}
