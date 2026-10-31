package app.urbudget.nativeapp

import android.content.Intent
import android.net.Uri
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SignInReturnActivityTest {
    @Test fun browserReturnReusesTheExistingActivity() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        ActivityScenario.launch(MainActivity::class.java).use { scenario ->
            lateinit var original: MainActivity
            scenario.onActivity { original = it }
            val intent = Intent(Intent.ACTION_VIEW, Uri.parse("${context.packageName}://assistant/sign-in"))
                .setPackage(context.packageName)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            context.startActivity(intent)
            instrumentation.waitForIdleSync()
            scenario.onActivity { assertSame(original, it) }
            instrumentation.runOnMainSync {
                val resumed = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED)
                assertEquals(listOf(original), resumed.toList())
            }
        }
    }
}
