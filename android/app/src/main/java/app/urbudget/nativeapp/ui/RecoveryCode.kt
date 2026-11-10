package app.urbudget.nativeapp.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.PersistableBundle
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.urbudget.nativeapp.R
import kotlinx.coroutines.launch
import org.json.JSONObject

// Sync with no passphrase to choose (docs/plans/2026-11-10-sync-device-approval.md). The account's
// first device makes the sync key and a recovery code for it, shown once for the person to keep.
// After that a device gets the key from one that syncs (DevicePairing.kt) or, with none at hand,
// from that code. The engine makes and checks the codes; this is the one showing of one.

/** The recovery code an answer carries, when the engine made a key without a passphrase. */
internal fun JSONObject.recoveryCode(): String = if (isNull("recoveryCode")) "" else optString("recoveryCode")

/** A recovery code that was just made, shown once. The phone doesn't keep it, so the sheet's one
 *  way out is the button that says it's saved. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun RecoveryCodeDialog(code: String, close: () -> Unit) {
    val context = LocalContext.current
    var copied by remember { mutableStateOf(false) }
    // A swipe down, a tap outside and Back do nothing: a slip would lose the only showing. So
    // this is UrSheet with that one rule, and a header with no Cancel.
    val stays = remember { { value: SheetValue -> value != SheetValue.Hidden } }
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true, confirmValueChange = stays)
    val scope = rememberCoroutineScope()
    ModalBottomSheet({
        // Something hid it all the same: bring it back rather than leave the code unseen.
        scope.launch { sheet.show() }
    }, Modifier.windowInsetsPadding(WindowInsets.statusBars), sheetState = sheet,
        properties = ModalBottomSheetProperties(shouldDismissOnBackPress = false),
        shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp), containerColor = ur.surface, contentColor = ur.text,
        scrimColor = ur.scrim, tonalElevation = 0.dp,
        dragHandle = { Box(Modifier.padding(top = 8.dp, bottom = 4.dp).size(36.dp, 5.dp).clip(CircleShape).background(ur.borderStrong)) }) {
        FollowLanguage { CompositionLocalProvider(LocalSheet provides true) { Column(Modifier.fillMaxWidth()) {
            SheetHeader(stringResource(R.string.recovery_save_title), null, null, null)
            Column(Modifier.fillMaxWidth().weight(1f, fill = false).verticalScroll(rememberScrollState()).padding(horizontal = 18.dp).padding(bottom = 20.dp),
                horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(14.dp)) {
                Text(stringResource(R.string.recovery_save_message))
                CodeText(code)
                SecondaryButton(stringResource(if (copied) R.string.recovery_copied else R.string.recovery_copy), {
                    val clip = ClipData.newPlainText(context.getString(R.string.recovery_row), code)
                    // Keeps the code out of the clipboard preview Android 13 and later show.
                    clip.description.extras = PersistableBundle().apply { putBoolean("android.content.extra.IS_SENSITIVE", true) }
                    (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(clip)
                    copied = true
                }, icon = Icons.Outlined.ContentCopy)
                PrimaryButton(stringResource(R.string.recovery_saved), close, Modifier.fillMaxWidth())
            }
        } } }
    }
}
