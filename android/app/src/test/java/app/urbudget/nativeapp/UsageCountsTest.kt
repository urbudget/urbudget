package app.urbudget.nativeapp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** When the app asks the engine to count it as in use, and what it says it is. */
class UsageCountsTest {
    private fun request(enabled: Boolean = true, debugBuild: Boolean = false, debugEngine: Boolean = false, applicationId: String = "app.urbudget") =
        UsageCounts.request(enabled, debugBuild, debugEngine, applicationId, "1.0.3")

    @Test fun aCopyFromGooglePlaySaysSo() {
        val sent = request()!!
        assertEquals(setOf("platform", "channel", "version"), sent.keys().asSequence().toSet())
        assertEquals("android", sent.getString("platform"))
        assertEquals("play", sent.getString("channel"))
        assertEquals("1.0.3", sent.getString("version"))
    }

    @Test fun aPreviewApkCountsAsAPreview() {
        assertEquals("preview", request(applicationId = "app.urbudget.nativeapp")!!.getString("channel"))
    }

    @Test fun theSwitchOffSendsNothing() {
        assertNull(request(enabled = false))
        assertNull(request(enabled = false, applicationId = "app.urbudget.nativeapp"))
    }

    @Test fun aDebugBuildCountsOnlyWithItsDebugEngine() {
        // A release engine would count on urbudget.app; a debug one only on URBUDGET_API's local server.
        assertNull(request(debugBuild = true, applicationId = "app.urbudget.nativeapp"))
        assertEquals("preview", request(debugBuild = true, debugEngine = true, applicationId = "app.urbudget.nativeapp")!!.getString("channel"))
        assertNull(request(enabled = false, debugBuild = true, debugEngine = true))
    }
}
