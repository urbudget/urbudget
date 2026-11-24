package app.urbudget.nativeapp.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class AppLinksTest {
    @Test
    fun feedbackStartsAnEmailToSupportThatSaysOnlyTheAppAndVersion() {
        assertEquals("mailto:support@urbudget.app?subject=UrBudget%20feedback%20%28Android%201.0.3%29", AppLinks.feedbackMail("1.0.3"))
    }

    @Test
    fun ratingOpensTheReleasedAppsPage() {
        assertEquals("market://details?id=app.urbudget", AppLinks.PLAY_APP)
        assertEquals("https://play.google.com/store/apps/details?id=app.urbudget", AppLinks.PLAY_WEB)
    }
}
