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

// Account & sync, simplified (section 14 of the same plan): which parts the screen shows, which
// budgets it lists and what a device's row says. Each part shows only when it has something to say.

/** A moment the engine or the server wrote, in either of the ways they write one. Null for anything else. */
internal fun syncInstant(text: String?): Instant? {
    val written = text?.trim().orEmpty()
    if (written.isEmpty() || written == "null") return null
    return runCatching { OffsetDateTime.parse(written).toInstant() }.getOrNull()
        ?: runCatching { Instant.parse(written.replace(' ', 'T').removeSuffix("Z") + "Z") }.getOrNull()
}

/**
 * "Last synced 5 minutes ago", counted in minutes up to an hour, hours up to two days, then days.
 * A clock that runs behind the server's makes it "just now" rather than a time to come. Null when
 * there is no moment to count from.
 */
internal fun lastSynced(then: Instant?, now: Instant): String? {
    val minutes = java.time.Duration.between(then ?: return null, now).toMinutes()
    return when {
        minutes < 1 -> Words[R.string.sync_last_now]
        minutes < 60 -> Words.count(R.plurals.sync_last_minutes, minutes)
        minutes < 48 * 60 -> Words.count(R.plurals.sync_last_hours, minutes / 60)
        else -> Words.count(R.plurals.sync_last_days, minutes / (24 * 60))
    }
}

/** A device that syncs with the account (`sync_devices`). */
internal class SyncDevice(val id: String, val name: String?, val added: Instant?, val lastSeen: Instant?, val current: Boolean)

/** Reads `{devices: [{id, name, addedAt, lastSeenAt, current}]}` in the engine's order: this phone first, then the most recently seen. */
internal fun syncDevices(answer: Any?): List<SyncDevice> {
    val rows = (answer as? JSONObject)?.optJSONArray("devices") ?: return emptyList()
    return (0 until rows.length()).mapNotNull { rows.optJSONObject(it) }.map { row ->
        fun words(key: String) = if (row.isNull(key)) null else row.optString(key).trim().ifEmpty { null }
        SyncDevice(row.optString("id"), words("name"), syncInstant(words("addedAt")), syncInstant(words("lastSeenAt")), row.optBoolean("current"))
    }
}

/**
 * What a device is called in the list: its name, or "This phone" for the one in hand before it has
 * one, and one that registered before names were kept by the day it was added ("A device added on
 * 3 November").
 */
internal fun deviceTitle(device: SyncDevice, zone: java.time.ZoneId = java.time.ZoneId.systemDefault()): String = when {
    device.name != null -> device.name
    device.current -> Words[R.string.sync_device_this]
    device.added != null -> Words[R.string.sync_device_added_on, Money.ltr(device.added.atZone(zone).toLocalDate().format(Money.pattern("d MMMM")))]
    else -> Words[R.string.sync_device_unnamed]
}

/** The line under a device's name: "This phone" for the one in hand, when another last synced. */
internal fun deviceDetail(device: SyncDevice, now: Instant): String? =
    if (device.current) Words[R.string.sync_device_this].takeIf { device.name != null } else lastSynced(device.lastSeen, now)

/** What the plan part of the screen has to say. A plan in good standing says nothing there. */
internal enum class PlanNotice {
    None,
    /** No plan: the price and Subscribe. */
    Subscribe,
    /** A plan that was cancelled and runs to a date. */
    Ending,
    /** The store couldn't take the last payment. */
    PaymentFailed,
}

private fun budgetStates(status: JSONObject?): List<String> {
    val rows = status?.optJSONObject("sync")?.optJSONObject("last")?.optJSONArray("budgets") ?: return emptyList()
    return (0 until rows.length()).mapNotNull { rows.optJSONObject(it)?.optString("state") }
}

/**
 * `overview` is `account_overview` (null until it answers, or offline). Without it the plan is only
 * mentioned when the last sync was turned away for want of one.
 */
internal fun planNotice(status: JSONObject?, overview: JSONObject?): PlanNotice {
    val plan = overview?.optJSONObject("subscription")
    val active = overview?.optJSONObject("sync")?.optBoolean("active")
    val turnedAway = status?.optJSONObject("sync")?.optJSONObject("last")?.optString("state") == "subscription_required" ||
        "subscription_required" in budgetStates(status)
    return when {
        plan?.optString("status") == "past_due" -> PlanNotice.PaymentFailed
        active == false || (active == null && turnedAway) -> PlanNotice.Subscribe
        planDay(overview, "endsAt") != null -> PlanNotice.Ending
        else -> PlanNotice.None
    }
}

/** The day the plan renews (`renewsAt`) or ends (`endsAt`), when the server gave one. */
internal fun planDay(overview: JSONObject?, key: String, zone: java.time.ZoneId = java.time.ZoneId.systemDefault()): java.time.LocalDate? {
    val plan = overview?.optJSONObject("subscription") ?: return null
    return syncInstant(if (plan.isNull(key)) null else plan.optString(key))?.atZone(zone)?.toLocalDate()
}

/** What the one status line says where sync is on. */
internal enum class SyncLine {
    /** Nothing needs the person: "Sync is on", with when it last synced. */
    On,
    /** Changes made here haven't gone up yet. */
    Waiting,
    /** A budget changed in two places and waits for a choice. */
    Review,
    /** A budget needs something else: a kept copy to recover, one that failed. */
    Attention,
    /** This version of the app is too old to sync a budget. */
    Update,
    NeedsPlan,
    Offline,
    /** The last sync stopped with an error, which the engine words itself. */
    Failed,
}

/** The first thing that needs the person, else how sync stands (`cloud_status`). */
internal fun syncLine(status: JSONObject?): SyncLine {
    val report = status?.optJSONObject("sync")?.optJSONObject("last")
    val states = budgetStates(status)
    val state = report?.optString("state").orEmpty()
    return when {
        "conflict" in states || "deletion_conflict" in states -> SyncLine.Review
        "update_app" in states -> SyncLine.Update
        state == "subscription_required" || "subscription_required" in states -> SyncLine.NeedsPlan
        "recovery" in states || "error" in states -> SyncLine.Attention
        state == "offline" -> SyncLine.Offline
        state == "error" || state == "attention" -> SyncLine.Failed
        state == "waiting" || "waiting" in states || "queued" in states -> SyncLine.Waiting
        else -> SyncLine.On
    }
}

/** When the last sync that finished was, for "Last synced 2 minutes ago". None while one is waiting or failed. */
internal fun lastSyncedAt(status: JSONObject?): Instant? {
    val report = status?.optJSONObject("sync")?.optJSONObject("last") ?: return null
    return if (report.optString("state") == "ok") syncInstant(report.optString("at")) else null
}

/** What a listed budget needs from the person. */
internal enum class BudgetNeed {
    /** Changed in two places: the differences are reviewed here. */
    Review,
    /** A copy this device kept when the account's budgets were replaced: recovered from the budget library. */
    KeptCopy,
    /** Kept only on this device, with a button that adds it to sync. */
    DeviceOnly,
    Failed,
    /** In the account and not on this device (the older sync): downloaded from here. */
    Download,
    /** Deleted here and changed elsewhere: looked over in the budget library. */
    ReviewDeletion,
    Update,
    NeedsPlan,
}

internal class BudgetRow(val entry: JSONObject, val need: BudgetNeed)

/** The budgets that need something, in the engine's order, and how many others are simply syncing. */
internal class SyncBudgets(val rows: List<BudgetRow>, val syncing: Int)

/**
 * Sorts the last sync's budgets (`sync.last.budgets`) into those listed and those only counted.
 * One that is neither, such as a budget in the Trash, is left to the budget library.
 */
internal fun syncBudgets(status: JSONObject?): SyncBudgets {
    val sync = status?.optJSONObject("sync")
    val entries = sync?.optJSONObject("last")?.optJSONArray("budgets")
    val newSync = sync?.optInt("protocol", 1) == 2
    val rows = mutableListOf<BudgetRow>()
    var syncing = 0
    for (i in 0 until (entries?.length() ?: 0)) {
        val entry = entries?.optJSONObject(i) ?: continue
        val need = when (entry.optString("state")) {
            "conflict" -> BudgetNeed.Review
            "recovery" -> BudgetNeed.KeptCopy
            "local_only" -> BudgetNeed.DeviceOnly
            "error" -> BudgetNeed.Failed
            "cloud_only" -> if (newSync) null else BudgetNeed.Download
            "deletion_conflict" -> BudgetNeed.ReviewDeletion
            "update_app" -> BudgetNeed.Update
            "subscription_required" -> BudgetNeed.NeedsPlan
            "synced", "uploaded", "downloaded", "merged", "waiting", "queued" -> { syncing++; null }
            else -> null
        }
        if (need != null) rows += BudgetRow(entry, need)
    }
    return SyncBudgets(rows, syncing)
}

/** The one plain sentence under a listed budget's name when the engine has none of its own. */
@StringRes
internal fun budgetNeedSentence(need: BudgetNeed): Int = when (need) {
    BudgetNeed.Review -> R.string.cloud_budget_review
    BudgetNeed.KeptCopy -> R.string.cloud_budget_kept
    BudgetNeed.DeviceOnly -> R.string.cloud_budget_device_only
    BudgetNeed.Failed -> R.string.cloud_budget_failed
    BudgetNeed.Download -> R.string.cloud_budget_download
    BudgetNeed.ReviewDeletion -> R.string.library_deletion_conflict
    BudgetNeed.Update -> R.string.cloud_budget_update
    BudgetNeed.NeedsPlan -> R.string.cloud_subscription_needed
}

/**
 * What a listed budget's row says. The engine's own sentence is used where it says what went wrong
 * or what to choose; a kept copy and a budget waiting for a plan or an update are worded here, as
 * the engine's words for those say how sync works.
 */
internal fun budgetSentence(row: BudgetRow): String {
    val own = if (row.entry.isNull("message")) "" else row.entry.optString("message").trim()
    val worded = row.need in setOf(BudgetNeed.Review, BudgetNeed.Failed, BudgetNeed.DeviceOnly, BudgetNeed.ReviewDeletion)
    return if (worded && own.isNotEmpty()) own else Words[budgetNeedSentence(row.need)]
}

/** The status line where sync is on: "Sync is on. Last synced 2 minutes ago.", or what needs the person, in plain words. */
internal fun syncLineText(status: JSONObject?, now: Instant): String = when (syncLine(status)) {
    SyncLine.On -> lastSynced(lastSyncedAt(status), now)?.let { Words[R.string.cloud_line_on_last, it] } ?: Words[R.string.cloud_line_on]
    SyncLine.Waiting -> Words[R.string.cloud_line_waiting]
    SyncLine.Review -> Words[R.string.cloud_line_review]
    SyncLine.Attention -> Words[R.string.cloud_line_attention]
    SyncLine.Update -> Words[R.string.cloud_line_update]
    SyncLine.NeedsPlan -> Words[R.string.cloud_subscription_needed]
    SyncLine.Offline -> Words[R.string.cloud_line_offline]
    SyncLine.Failed -> status?.optJSONObject("sync")?.optJSONObject("last")?.takeUnless { it.isNull("message") }?.optString("message")?.trim()?.ifEmpty { null }
        ?: Words[R.string.cloud_line_failed]
}
