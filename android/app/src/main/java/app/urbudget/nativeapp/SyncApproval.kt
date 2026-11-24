package app.urbudget.nativeapp

import androidx.annotation.StringRes
import org.json.JSONObject
import java.time.Instant
import java.time.OffsetDateTime

// Sync with no passphrase to type (docs/plans/2026-11-24-sync-device-approval.md): what Account &
// sync works out from the engine's answers before it draws anything. The engine makes, reads and
// checks every code; nothing here does.

/** Where this device stands with the account's sync key. */
internal enum class SyncKey {
    /** Nothing says yet: the engine hasn't answered, and no earlier sync left word. */
    Unknown,
    /** The account has no key: sync was never turned on. */
    None,
    /** The account has a key and this device doesn't hold it. */
    Locked,
    /** This device holds the key. */
    Unlocked,
}

private fun JSONObject.secretKind(): String? = if (isNull("secret")) null else optString("secret").ifBlank { null }

/**
 * `status` is `cloud_status`, `keyInfo` the last fresh `sync_key_info` (null until it answers, or
 * offline). Until it does, the last sync's word that the account has a key still stands, so a
 * locked device doesn't offer to turn sync on while it waits.
 */
internal fun syncKey(status: JSONObject?, keyInfo: JSONObject?): SyncKey {
    val sync = status?.optJSONObject("sync")
    return when {
        sync?.optBoolean("unlocked") == true -> SyncKey.Unlocked
        keyInfo != null -> if (keyInfo.optBoolean("exists")) SyncKey.Locked else SyncKey.None
        sync?.secretKind() != null -> SyncKey.Locked
        else -> SyncKey.Unknown
    }
}

/** What the account's key is made from, `"code"` or `"passphrase"`: the fresh answer, else what the last sync saved. */
internal fun syncSecret(status: JSONObject?, keyInfo: JSONObject?): String? =
    keyInfo?.secretKind() ?: status?.optJSONObject("sync")?.secretKind()

/**
 * The sync protocol the account is on, 1 or 2: the fresh answer, else what the last sync saved.
 * A reset leaves different things behind on each, so its confirmation reads differently.
 */
internal fun syncProtocol(status: JSONObject?, keyInfo: JSONObject?): Int =
    if (keyInfo?.optInt("protocol") == 2 || status?.optJSONObject("sync")?.optInt("protocol") == 2) 2 else 1

/** The label of the box that unlocks sync: an account with a passphrase can still type it there. */
@StringRes
internal fun syncUnlockLabel(secret: String?): Int =
    if (secret == "passphrase") R.string.cloud_unlock_field_passphrase else R.string.cloud_unlock_field_code

/** The recovery code an answer carries (`sync_setup`, `sync_unlock`, `sync_reset`, `sync_recovery_create`, `sync_recovery_unsaved`). */
internal fun recoveryCodeIn(answer: Any?): String? =
    (answer as? JSONObject)?.takeUnless { it.isNull("recoveryCode") }?.optString("recoveryCode")?.ifBlank { null }

/**
 * A code's six groups of four on two lines of three, as it is shown. The first line keeps its
 * hyphen: the text read across both lines is the code exactly, which is what an app from before
 * recovery codes needs typed into its passphrase box.
 */
internal fun codeLines(code: String): String {
    val groups = code.trim().split('-')
    if (groups.size != 6) return code.trim()
    return groups.take(3).joinToString("-") + "-\n" + groups.drop(3).joinToString("-")
}

/** The engine's QR code: `size` squares a side. */
internal class QrGrid(val size: Int, private val squares: BooleanArray) {
    fun dark(x: Int, y: Int): Boolean = squares[y * size + x]
}

/**
 * Reads `qr: {size, modules}`, where `modules` is `size`×`size` characters row by row, `1` for a
 * dark square. Null for anything that isn't a whole square of 0s and 1s, so a damaged answer
 * draws nothing rather than a code that scans as something else.
 */
internal fun qrGrid(qr: JSONObject?): QrGrid? {
    val size = qr?.optInt("size") ?: return null
    val modules = qr.optString("modules")
    // The smallest QR code is 21 squares a side, the largest 177.
    if (size !in 21..177 || modules.length != size * size || modules.any { it != '0' && it != '1' }) return null
    return QrGrid(size, BooleanArray(modules.length) { modules[it] == '1' })
}

/** How long a device code lasts, on the server and in the engine. */
internal const val PAIR_LIFETIME_MS = 5 * 60_000L

/** How often the screen showing a code asks whether it's still waiting. */
internal const val PAIR_POLL_MS = 3_000L

/**
 * How long a code that was just made counts down for, in milliseconds: until `expiresAt` by this
 * device's clock. Only the count on the screen follows it; whether the code was used or ran out
 * is the server's to say (`pairState`). A code that has run out as it arrives, or outlasts the time codes last, means
 * the clock here is wrong, so it gets the time codes last.
 */
internal fun pairingLifetime(expiresAt: String?, now: Instant): Long {
    val left = runCatching { OffsetDateTime.parse(expiresAt).toInstant().toEpochMilli() - now.toEpochMilli() }.getOrNull()
    return if (left == null || left <= 0 || left > PAIR_LIFETIME_MS) PAIR_LIFETIME_MS else left
}

/** What became of a code this device is showing (`sync_pair_state`). */
internal enum class PairState {
    /** Nobody has used it yet. */
    Waiting,
    /** The new device has the key. */
    Collected,
    /** It ran out or was taken back: only a new code will do. */
    Gone,
}

/**
 * Reads `{waiting, collected}`. Only the server's word that the key was collected means a
 * device was added: a code that is no longer waiting has otherwise run out. No answer (offline,
 * say) leaves it waiting, to be asked again.
 */
internal fun pairState(answer: Any?): PairState {
    val state = answer as? JSONObject ?: return PairState.Waiting
    return when {
        state.optBoolean("collected") -> PairState.Collected
        state.optBoolean("waiting") -> PairState.Waiting
        else -> PairState.Gone
    }
}

/** The time a code has left, as minutes and seconds: `4:07`. */
internal fun pairingCountdown(left: Long): String {
    val seconds = (left.coerceAtLeast(0) + 999) / 1000
    return "${seconds / 60}:${(seconds % 60).toString().padStart(2, '0')}"
}

/** What every QR code of UrBudget's starts with (crates/urbudget-services/src/cloud/codes.rs). */
private const val CODE_PREFIX = "URBUDGET-"
private const val LINK_PREFIX = "urbudget://"

/** Whether scanned text is one of UrBudget's codes: a link of its own kind, or as codes were written before they were links. */
internal fun scannableSyncCode(text: String): Boolean = text.trim().let { it.startsWith(LINK_PREFIX, ignoreCase = true) || it.startsWith(CODE_PREFIX) }

/** Whether a scanned code is one a device waiting to be added shows, which a device that syncs approves. */
internal fun syncCodeIsRequest(text: String): Boolean = text.trim().let { it.startsWith(LINK_PREFIX + "ask/", ignoreCase = true) || it.startsWith(CODE_PREFIX + "ASK:") }

/**
 * The UrBudget code in a camera frame, given the frame's brightness row by row, or null when it
 * shows none. Some other QR code in view is passed over, so the camera keeps looking. Read here
 * on the phone, with ZXing: the frame goes nowhere.
 */
internal fun decodeSyncCode(brightness: ByteArray, width: Int, height: Int): String? = try {
    val source = com.google.zxing.PlanarYUVLuminanceSource(brightness, width, height, 0, 0, width, height, false)
    com.google.zxing.qrcode.QRCodeReader()
        .decode(com.google.zxing.BinaryBitmap(com.google.zxing.common.HybridBinarizer(source)))
        .text?.takeIf(::scannableSyncCode)
} catch (_: com.google.zxing.ReaderException) {
    null
}
