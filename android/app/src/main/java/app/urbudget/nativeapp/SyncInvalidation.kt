package app.urbudget.nativeapp

import org.json.JSONObject

/** Resolve the current displayed path at completion; never reuse the identity from before sync. */
internal fun syncInvalidatesActive(report: JSONObject, library: JSONObject, identity: String?, revision: String?): Boolean {
    val entries = library.getJSONArray("entries")
    val rows = (0 until entries.length()).map(entries::getJSONObject)
    val active = rows.firstOrNull { it.optString("id") == library.optString("activeId") }
    val activePath = active?.optString("path")?.ifBlank { null }
    if (activePath != identity) return true
    if (!report.has("changedBudgets")) return report.optBoolean("openChanged")
    val id = rows.firstOrNull { it.optString("path") == identity }?.optString("id") ?: return false
    val changes = report.optJSONArray("changedBudgets") ?: return false
    return (0 until changes.length()).map(changes::getJSONObject).any {
        it.optString("id") == id && (it.optString("revision").isBlank() || it.optString("revision") != revision)
    }
}

/** The key gates financial transfer, not v2 catalog/tombstone reconciliation. */
internal fun syncCanReconcile(status: JSONObject): Boolean {
    val sync = status.optJSONObject("sync") ?: return false
    return !status.isNull("email") && status.optString("email").isNotBlank() &&
        (sync.optInt("protocol", 1) == 2 || sync.optBoolean("unlocked"))
}

/** v1 retains its foreground-only cadence; signed-out/private users need no persistent worker. */
internal fun syncBackgroundEligible(status: JSONObject): Boolean = syncCanReconcile(status) &&
    status.optJSONObject("sync")?.let { it.optInt("protocol") == 2 && it.optBoolean("enabled") } == true

/** A v1 account urbudget.app will move to the new sync (sync.rs `Status::upgrade_available`). */
internal fun syncMoveOffered(status: JSONObject?): Boolean {
    val signedIn = status ?: return false
    val sync = signedIn.optJSONObject("sync") ?: return false
    return !signedIn.isNull("email") && signedIn.optString("email").isNotBlank() &&
        sync.optInt("protocol", 1) != 2 && sync.optBoolean("upgradeAvailable")
}

/** Moving asks for the passphrase only when this device has no saved key that opens the account. */
internal fun syncMoveNeedsPassphrase(status: JSONObject?): Boolean =
    status?.optJSONObject("sync")?.optBoolean("upgradeNeedsPassphrase") == true

/** `sync_upgrade`'s parameters: the passphrase only when it's needed. */
internal fun syncMoveParams(status: JSONObject?, passphrase: String): JSONObject =
    JSONObject().also { if (syncMoveNeedsPassphrase(status)) it.put("passphrase", passphrase) }
