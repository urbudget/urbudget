package app.urbudget.nativeapp

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SyncInvalidationTest {
    @Test fun lockedV2StillReconcilesWhileLegacyNeedsItsKey() {
        val status = JSONObject("""{"email":"qa@example.test","sync":{"protocol":2,"unlocked":false}}""")
        assertTrue(syncCanReconcile(status))
        status.getJSONObject("sync").put("protocol", 1)
        assertFalse(syncCanReconcile(status))
        status.getJSONObject("sync").put("unlocked", true)
        assertTrue(syncCanReconcile(status))
        status.put("email", JSONObject.NULL)
        assertFalse(syncCanReconcile(status))
    }
    @Test fun onlyEnabledSignedInV2SchedulesPersistentWork() {
        val status = JSONObject("""{"email":"qa@example.test","sync":{"protocol":2,"enabled":true,"unlocked":false}}""")
        assertTrue(syncBackgroundEligible(status))
        status.getJSONObject("sync").put("enabled", false)
        assertFalse(syncBackgroundEligible(status))
        status.getJSONObject("sync").put("enabled", true).put("protocol", 1).put("unlocked", true)
        assertFalse(syncBackgroundEligible(status))
        status.getJSONObject("sync").put("protocol", 2)
        status.put("email", JSONObject.NULL)
        assertFalse(syncBackgroundEligible(status))
    }
    @Test fun onlyASignedInV1AccountTheServerWillMoveIsOffered() {
        val status = JSONObject("""{"email":"qa@example.test","sync":{"protocol":1,"unlocked":true,"upgradeAvailable":true,"upgradeNeedsPassphrase":false}}""")
        assertTrue(syncMoveOffered(status))
        assertFalse(syncMoveNeedsPassphrase(status))
        assertFalse("the saved key moves it", syncMoveParams(status, "typed").has("passphrase"))
        status.getJSONObject("sync").put("upgradeNeedsPassphrase", true)
        assertEquals("typed passphrase", syncMoveParams(status, "typed passphrase").getString("passphrase"))
        status.getJSONObject("sync").put("protocol", 2)
        assertFalse("already moved", syncMoveOffered(status))
        status.getJSONObject("sync").put("protocol", 1).remove("upgradeAvailable")
        assertFalse("an older engine or a server that refuses moves", syncMoveOffered(status))
        status.getJSONObject("sync").put("upgradeAvailable", true)
        status.put("email", JSONObject.NULL)
        assertFalse(syncMoveOffered(status))
        assertFalse(syncMoveOffered(null))
    }
    private fun library() = JSONObject("""{"activeId":"current","entries":[{"id":"former","path":"/former"},{"id":"current","path":"/current"}]}""")
    @Test fun targetedChangesDoNotRefreshAnUnrelatedSwitchedBudget() {
        val report = JSONObject("""{"openChanged":true,"changedBudgets":[{"id":"former","revision":"next"}]}""")
        assertFalse(syncInvalidatesActive(report, library(), "/current", "old"))
    }
    @Test fun newerCurrentRevisionReloadsButAnAlreadyDisplayedRevisionDoesNot() {
        val report = JSONObject("""{"openChanged":false,"changedBudgets":[{"id":"current","revision":"next"}]}""")
        assertTrue(syncInvalidatesActive(report, library(), "/current", "old"))
        assertFalse(syncInvalidatesActive(report, library(), "/current", "next"))
    }
    @Test fun deletionToEmptyReloadsEvenWithoutReadableRevision() {
        assertTrue(syncInvalidatesActive(JSONObject("""{"openChanged":false,"changedBudgets":[]}"""), library().put("activeId", JSONObject.NULL), "/current", "old"))
    }
    @Test fun legacyFlagIsOnlyUsedWhenTargetedChangesAreAbsent() {
        assertTrue(syncInvalidatesActive(JSONObject("""{"openChanged":true}"""), library(), "/current", "old"))
        assertFalse(syncInvalidatesActive(JSONObject("""{"openChanged":true,"changedBudgets":[]}"""), library(), "/current", "old"))
    }
}
