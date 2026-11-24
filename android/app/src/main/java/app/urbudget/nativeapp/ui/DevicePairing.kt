package app.urbudget.nativeapp.ui

import android.os.SystemClock
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.time.Instant
import kotlin.math.floor

// Adding a device to sync with nothing long to type (docs/plans/2026-11-24-sync-device-approval.md).
// A one-time code goes from one screen to the other by camera or keyboard, and the shared engine
// uses it to hand the sync key over; the server never sees the code. The phone has the camera, so
// it scans: a new phone scans the code a device that syncs shows, and a phone that syncs scans
// the code a new computer shows, then approves it.

/** The light margin a QR code needs around it, in squares, for a scanner to find its edges. */
private const val QuietZone = 4

/**
 * The engine's QR code, always dark on white with its margin, whatever the app's theme, and never
 * mirrored in a right-to-left language. Squares sit on whole pixels, so none blurs into the next.
 */
@Composable
private fun QrCode(grid: QrGrid, description: String) {
    Canvas(Modifier.size(232.dp).clip(RoundedCornerShape(12.dp)).semantics { contentDescription = description }) {
        drawRect(Color.White)
        val across = grid.size + 2 * QuietZone
        val square = floor(size.minDimension / across).coerceAtLeast(1f)
        val start = floor((size.minDimension - square * across) / 2) + QuietZone * square
        for (y in 0 until grid.size) for (x in 0 until grid.size) if (grid.dark(x, y)) {
            drawRect(Color.Black, Offset(start + x * square, start + y * square), Size(square, square))
        }
    }
}

/**
 * A code's 24 characters, to read, select or type: left to right in any language, three groups to
 * a line, and as large as the width allows up to [largest].
 */
@Composable
internal fun CodeText(code: String, modifier: Modifier = Modifier, largest: TextUnit = 22.sp) {
    val lines = remember(code) { codeLines(code) }
    SelectionContainer(modifier) {
        BasicText(lines, style = TextStyle(color = ur.text, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.SemiBold, textDirection = TextDirection.Ltr),
            softWrap = false, maxLines = 2, autoSize = TextAutoSize.StepBased(minFontSize = 11.sp, maxFontSize = largest, stepSize = 1.sp))
    }
}

/** Takes back a code nobody used. Its screen is gone by then, so this outlives the screen's own scope. */
private fun withdraw(id: String) {
    CoroutineScope(Dispatchers.IO).launch { runCatching { NativeServices.call("sync_pair_cancel", JSONObject().put("id", id)) } }
}

/**
 * On a device that syncs: shows a code for a new phone to scan or a new computer to have typed in,
 * and scans the code a new computer shows. It closes itself through [added] once the new device has
 * the key. The shown code is taken back when this closes.
 */
@Composable
internal fun AddDeviceDialog(close: () -> Unit, added: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val latestAdded by rememberUpdatedState(added)
    var offer by remember { mutableStateOf<JSONObject?>(null) }
    // When the shown code runs out, on the clock that keeps counting while the phone sleeps, and how long that leaves.
    var deadline by remember { mutableLongStateOf(0L) }
    var left by remember { mutableLongStateOf(0L) }
    var expired by remember { mutableStateOf(false) }
    var attempt by remember { mutableIntStateOf(0) }
    var offerError by remember { mutableStateOf<String?>(null) }
    var scanError by remember { mutableStateOf<String?>(null) }
    var scanning by remember { mutableStateOf(false) }
    // A computer's code that was scanned, until the person says whether to add it.
    var approving by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    LaunchedEffect(attempt) {
        offer = null; expired = false; offerError = null
        try {
            val made = withContext(Dispatchers.IO) {
                val made = NativeServices.call("sync_pair_offer") as JSONObject
                // The screen closed while the code was being made: nobody will see it, so it's taken back here.
                if (!isActive) runCatching { NativeServices.call("sync_pair_cancel", JSONObject().put("id", made.text("id"))) }
                made
            }
            left = pairingLifetime(made.text("expiresAt"), Instant.now())
            deadline = SystemClock.elapsedRealtime() + left
            offer = made
        } catch (cancel: CancellationException) { throw cancel }
        catch (failure: Exception) { offerError = failure.message ?: context.getString(R.string.common_try_again) }
    }
    val shown = offer?.text("id").orEmpty()
    DisposableEffect(shown) { onDispose { if (shown.isNotBlank()) withdraw(shown) } }
    // Counts the code down to when it runs out.
    LaunchedEffect(shown) {
        while (shown.isNotBlank()) {
            left = deadline - SystemClock.elapsedRealtime()
            if (left <= 0) { expired = true; break }
            delay(minOf(left, 1_000L))
        }
    }
    // Asks what became of the code. Only the server's word that the key was collected closes this
    // with the news; a code that ran out or was taken back offers a new one. It goes on asking past
    // this device's own count, whose clock may run ahead of the server's.
    LaunchedEffect(shown) {
        while (shown.isNotBlank()) {
            delay(PAIR_POLL_MS)
            val state = try { pairState(NativeServices.read("sync_pair_state", JSONObject().put("id", shown))) }
                catch (cancel: CancellationException) { throw cancel }
                catch (_: Exception) { PairState.Waiting } // No answer this time: it's asked again in a moment.
            when (state) {
                PairState.Waiting -> continue
                PairState.Collected -> latestAdded()
                PairState.Gone -> expired = true
            }
            break
        }
    }
    FormDialog(stringResource(R.string.pair_add_device), close) {
        Text(stringResource(R.string.pair_add_intro), color = ur.text2)
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
            val made = offer
            val grid = remember(made) { qrGrid(made?.optJSONObject("qr")) }
            when {
                expired -> {
                    Text(stringResource(R.string.pair_code_expired), color = ur.text2)
                    SecondaryButton(stringResource(R.string.pair_new_code), { attempt++ })
                }
                made == null -> if (offerError == null) CircularProgressIndicator(Modifier.padding(vertical = 24.dp).size(28.dp), color = ur.accent) else {
                    FormError(offerError)
                    SecondaryButton(stringResource(R.string.pair_new_code), { attempt++ })
                }
                else -> {
                    grid?.let { QrCode(it, stringResource(R.string.pair_qr_description)) }
                    CodeText(made.text("code"))
                    Text(stringResource(R.string.pair_code_expires, pairingCountdown(left)), color = ur.text2, style = MaterialTheme.typography.bodySmall)
                    Text(stringResource(R.string.pair_code_warning), color = ur.muted, style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Center)
                }
            }
        }
        Hairline()
        // A computer has no camera to scan with, so it shows a code of its own and the phone scans that.
        // If the scanner can't open, the computer takes this phone's code typed, as a phone would scan it.
        Text(stringResource(R.string.pair_scan_computer_intro), color = ur.text2)
        FormError(scanError)
        SecondaryButton(stringResource(R.string.pair_scan_computer), {
            scanError = null
            scanning = true
        }, Modifier.fillMaxWidth(), enabled = !busy, icon = Icons.Outlined.QrCodeScanner)
    }
    if (scanning) SyncCodeScanner(
        found = { scanning = false; approving = it },
        unavailable = { scanning = false; scanError = context.getString(R.string.pair_scanner_unavailable_computer) },
        close = { scanning = false })
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
                        latestAdded()
                    } catch (cancel: CancellationException) { throw cancel }
                    catch (failure: Exception) { scanError = failure.message ?: context.getString(R.string.common_try_again) }
                    finally { busy = false; approving = null }
                }
            }, enabled = !busy) { Text(stringResource(R.string.pair_approve)) } },
            dismissButton = { TextButton({ approving = null }, enabled = !busy) { Text(stringResource(R.string.common_cancel)) } })
    }
}
