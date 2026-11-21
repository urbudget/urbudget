package app.urbudget.nativeapp

import android.system.Os
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.time.LocalDate
import java.time.ZoneOffset

/**
 * This copy counting itself as in use on a local Worker (web/src/usage.ts), never on urbudget.app,
 * as MainActivity does when it comes to the front. Opt-in: a debug build with a debug engine
 * (-PrustDebug=true), `pm clear` first, the Worker from web/scripts/sync-v2-local.mjs,
 * `adb reverse tcp:8788 tcp:8788` and `-e usageQaOrigin http://127.0.0.1:8788` (on a read-only
 * emulator copy the app couldn't reach 10.0.2.2). Afterwards the Worker's app_usage has an
 * android/preview first open, day, week and month.
 */
@RunWith(AndroidJUnit4::class)
class UsageCountsLiveCheck {
    @Test fun countsThisCopyOnTheLocalWorkerWhenItComesToTheFront() {
        val origin = InstrumentationRegistry.getArguments().getString("usageQaOrigin")
        assumeTrue("Opt-in local Worker check", origin in setOf("http://10.0.2.2:8788", "http://127.0.0.1:8788"))
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        require(context.packageName.endsWith(".nativeapp") && BuildConfig.DEBUG && BuildConfig.RUST_DEBUG) { "A debug engine counts only where URBUDGET_API points." }
        Os.setenv("URBUDGET_API", origin!!, true)
        NativePreferences(context).usageCounts = true
        val today = LocalDate.now(ZoneOffset.UTC).toString()
        val file = File(File(context.filesDir, "native"), "usage.json")
        fun usage() = if (file.exists()) JSONObject(file.readText()) else JSONObject()

        ActivityScenario.launch(MainActivity::class.java).use {
            val deadline = System.currentTimeMillis() + 30_000
            while (usage().optString("day") != today && System.currentTimeMillis() < deadline) Thread.sleep(250)
            assertEquals(origin, (NativeServices.call("cloud_status") as JSONObject).getString("site"))
            // Not counted: ask the engine again, so its refusal shows.
            if (usage().optString("day") != today) NativeServices.call("usage_report", UsageCounts.request(NativePreferences(context))!!)
        }
        assertEquals(usage().toString(), today, usage().getString("day"))
        assertFalse(usage().toString(), usage().getBoolean("first"))
    }
}
