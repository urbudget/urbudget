package app.urbudget.nativeapp

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** When the page asks for permission to notify as a reading starts, so its notification can show. */
class SmsReadingNoticeTest {
    @Test fun askedOnceOnAndroidThirteenAndUpWhileNotAllowed() {
        assertTrue(smsAskToNotify(sdk = 33, allowed = false, asked = false))
        assertTrue(smsAskToNotify(sdk = 37, allowed = false, asked = false))
    }

    @Test fun neverAgainOnceAskedWhateverTheAnswer() {
        assertFalse(smsAskToNotify(sdk = 37, allowed = false, asked = true))
    }

    @Test fun notWhenAlreadyAllowed() {
        assertFalse(smsAskToNotify(sdk = 37, allowed = true, asked = false))
    }

    @Test fun notBeforeAndroidThirteenWhereNotificationsNeedNoPermission() {
        assertFalse(smsAskToNotify(sdk = 32, allowed = false, asked = false))
        assertFalse(smsAskToNotify(sdk = 26, allowed = false, asked = false))
    }
}
