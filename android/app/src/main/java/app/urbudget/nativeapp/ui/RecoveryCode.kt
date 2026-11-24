package app.urbudget.nativeapp.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.PersistableBundle
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.DialogProperties
import app.urbudget.nativeapp.R

// The recovery code (docs/plans/2026-11-24-sync-device-approval.md): what unlocks sync on a new
// device when no other device is at hand. Turning sync on or resetting it makes the key from one,
// and a lost one is replaced from a device that still syncs. The engine makes the codes and keeps
// one on this device until the person says it's saved; this is where they see it and say so.

/**
 * A recovery code the person hasn't said they've saved. Back and a tap outside do nothing, so a
 * slip can't put away what they were meant to keep: the one way out is the button that says it's
 * saved ([saved]). Until then the engine still holds the code, and Account & sync shows it again.
 */
@Composable
internal fun RecoveryCodeDialog(code: String, busy: Boolean, saved: () -> Unit) {
    val context = LocalContext.current
    var copied by remember(code) { mutableStateOf(false) }
    AlertDialog(onDismissRequest = {}, properties = DialogProperties(dismissOnBackPress = false, dismissOnClickOutside = false),
        title = { Text(stringResource(R.string.recovery_title)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Text(stringResource(R.string.recovery_message), Modifier.fillMaxWidth(), color = ur.text2)
                CodeText(code, largest = 26.sp)
                SecondaryButton(stringResource(if (copied) R.string.recovery_copied else R.string.recovery_copy), {
                    val clip = ClipData.newPlainText(context.getString(R.string.recovery_clip_label), code)
                    // Keeps the code out of the clipboard preview Android 13 and later show.
                    clip.description.extras = PersistableBundle().apply { putBoolean("android.content.extra.IS_SENSITIVE", true) }
                    (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
                    copied = true
                }, icon = Icons.Outlined.ContentCopy)
                Text(stringResource(R.string.recovery_keep), Modifier.fillMaxWidth(), color = ur.text2)
            }
        },
        confirmButton = { PrimaryButton(stringResource(R.string.recovery_saved), saved, Modifier.fillMaxWidth(), enabled = !busy) })
}
