package app.urbudget.nativeapp.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.NativePreferences
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.UsageCounts
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Settings → Privacy & data: Count this copy as in use, on by default, turns the anonymous usage
 * counts off and on again, and is kept. Nothing is sent: the engine isn't started here.
 */
@RunWith(AndroidJUnit4::class)
class PrivacySettingsUiTest {
    @get:Rule val compose = createComposeRule()
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val store = NativePreferences(context)
    private val before = store.usageCounts

    @After fun restore() { store.usageCounts = before }

    @Test fun theSwitchTurnsTheCountsOffAndOnAgain() {
        store.usageCounts = true
        var policy = 0
        compose.setContent { MaterialTheme { PrivacyCard(UiPreferences(store)) { policy++ } } }
        val switch = compose.onNode(isToggleable())
        switch.assertIsOn()

        compose.onNodeWithText(context.getString(R.string.settings_usage_counts)).performClick()
        switch.assertIsOff()
        assertFalse(store.usageCounts)
        // Off: the app asks the engine for nothing, whatever kind of build it is.
        assertNull(UsageCounts.request(store.usageCounts, debugBuild = false, debugEngine = false, applicationId = "app.urbudget", version = "1.0.3"))
        assertNull(UsageCounts.request(store))

        switch.performClick()
        switch.assertIsOn()
        assertTrue(NativePreferences(context).usageCounts)
        assertNotNull(UsageCounts.request(store.usageCounts, debugBuild = false, debugEngine = false, applicationId = "app.urbudget", version = "1.0.3"))

        // The policy is still one tap away.
        compose.onNodeWithText(context.getString(R.string.settings_privacy)).performClick()
        assertEquals(1, policy)
    }
}
