package app.urbudget.nativeapp

import android.system.Os
import androidx.lifecycle.Lifecycle
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.work.WorkInfo
import androidx.work.WorkManager
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.Assume.assumeTrue
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.TimeUnit

/** Run only on the disposable CodexSyncV2Qa emulator against wrangler dev. No production account. */
@RunWith(AndroidJUnit4::class)
class SyncV2RuntimeTest {
    @Test fun automaticLibraryRetriesAndDeletionSurviveActivityBackgrounding() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val args = InstrumentationRegistry.getArguments()
        val origin = args.getString("syncQaOrigin")
        assumeTrue("Opt-in local Worker test", origin in setOf("http://10.0.2.2:8788", "http://127.0.0.1:8788"))
        val context = instrumentation.targetContext
        require(context.packageName.endsWith(".nativeapp") && BuildConfig.DEBUG)
        Os.setenv("URBUDGET_API", origin!!, true)
        NativeServices.initialize(context)
        assertEquals(origin, (NativeServices.call("cloud_status") as JSONObject).getString("site"))
        val email = "sync-v2-android-20261028@example.test"
        val login = NativeServices.call("account_start", JSONObject().put("email", email)) as JSONObject
        val code = login.getString("devCode")
        NativeServices.call("account_verify", JSONObject().put("email", email).put("code", code))
        NativeServices.call("sync_setup", JSONObject().put("passphrase", "synthetic sync v2 qa only"))
        val fixture = JSONObject(instrumentation.context.assets.open("native-roundtrip.json").bufferedReader().use { it.readText() })
            .put("name", "Android lifecycle ${UUID.randomUUID()}")
        val id = NativeServices.call("create_budget", JSONObject().put("budget", fixture)) as String
        var snapshot = NativeServices.call("library_open", JSONObject().put("id", id)) as JSONObject
        val first = NativeServices.call("sync_now") as JSONObject
        assertEquals(first.toString(), "ok", first.getString("state"))

        // Locked v2 still consumes the catalog through the actual scheduled worker. It must
        // retain this local edit in the outgoing queue until the passphrase is unlocked again.
        NativeServices.call("sync_lock")
        snapshot = NativeServices.call("load_budget") as JSONObject
        val lockedDraft = snapshot.getJSONObject("budget").put("name", "Locked pending edit ${UUID.randomUUID()}")
        NativeServices.call("save_budget", JSONObject().put("budget", lockedDraft).put("expectedToken", snapshot.getString("token")))
        val lockedDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(45)
        var lockedReport = NativeServices.syncStatus.value?.optJSONObject("sync")?.optJSONObject("last")
        fun lockedAttemptCompleted() = lockedReport?.let { it.optString("state") == "locked" && it.has("changedBudgets") && it.optString("at") != first.optString("at") } == true
        while (!lockedAttemptCompleted() && System.nanoTime() < lockedDeadline) {
            Thread.sleep(200)
            lockedReport = NativeServices.syncStatus.value?.optJSONObject("sync")?.optJSONObject("last")
        }
        assertTrue("Locked v2 must run a new scheduled catalog reconciliation: $lockedReport", lockedAttemptCompleted())
        assertTrue("Locked report must include v2 invalidations", lockedReport?.has("changedBudgets") == true)
        val lockedEntries = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries")
        assertTrue("Locked worker must not upload pending financial edits", (0 until lockedEntries.length()).map(lockedEntries::getJSONObject).first { it.getString("id") == id }.optBoolean("pending"))
        NativeServices.call("sync_setup", JSONObject().put("passphrase", "synthetic sync v2 qa only"))
        NativeServices.call("sync_now")

        ActivityScenario.launch(MainActivity::class.java).use { activity ->
            instrumentation.waitForIdleSync()
            snapshot = NativeServices.call("load_budget") as JSONObject
            val changed = snapshot.getJSONObject("budget").put("name", "Android background save ${UUID.randomUUID()}")
            snapshot = NativeServices.call("save_budget", JSONObject().put("budget", changed).put("expectedToken", snapshot.getString("token"))) as JSONObject
            activity.moveToState(Lifecycle.State.CREATED)
            // The save/onStop hooks must enqueue this unique request themselves.
            val manager = WorkManager.getInstance(context)
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(90)
            var scheduled = manager.getWorkInfosForUniqueWork("account-sync").get(10, TimeUnit.SECONDS)
            while (scheduled.none { it.state == WorkInfo.State.SUCCEEDED } && System.nanoTime() < deadline) {
                Thread.sleep(200)
                scheduled = manager.getWorkInfosForUniqueWork("account-sync").get(10, TimeUnit.SECONDS)
            }
            assertTrue("App lifecycle must schedule and complete sync: $scheduled", scheduled.any { it.state == WorkInfo.State.SUCCEEDED })
            val uploadDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(30)
            var pending = true
            while (pending && System.nanoTime() < uploadDeadline) {
                val entries = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries")
                pending = (0 until entries.length()).map(entries::getJSONObject).first { it.getString("id") == id }.optBoolean("pending")
                if (pending) Thread.sleep(200)
            }
            assertFalse("Saved work must leave the durable outgoing queue", pending)
            activity.moveToState(Lifecycle.State.RESUMED)
        }
        val status = NativeServices.call("cloud_status") as JSONObject
        assertEquals(status.toString(), "ok", status.getJSONObject("sync").getJSONObject("last").getString("state"))
        NativeServices.call("editor_checkpoint", JSONObject().put("budgetId", id).put("id", "qa-editor").put("epoch", "2")
            .put("target", "transaction").put("baseline", JSONObject()).put("values", JSONObject().put("amount", "12 +")))
        NativeServices.call("editor_discard", JSONObject().put("budgetId", id).put("id", "qa-editor").put("epoch", "1"))
        val drafts = NativeServices.call("editor_recovery", JSONObject().put("budgetId", id)) as JSONArray
        assertEquals(2L, drafts.getJSONObject(0).getLong("epoch"))
        assertEquals("12 +", drafts.getJSONObject(0).getJSONObject("draft").getString("amount"))
        NativeServices.call("editor_discard", JSONObject().put("budgetId", id).put("id", "qa-editor").put("epoch", "2"))
        NativeServices.call("library_trash", JSONObject().put("id", id))
        assertTrue((NativeServices.call("load_budget") as JSONObject).isNull("budget"))
        assertNull(SqliteBudgetStorage(context).read())
        assertThrows(Exception::class.java) {
            NativeServices.call("save_budget", JSONObject().put("budget", fixture).put("expectedToken", snapshot.getString("token")))
        }
        NativeServices.call("sync_now")
        NativeServices.call("library_restore", JSONObject().put("id", id))
        NativeServices.call("sync_now")
        NativeServices.call("library_open", JSONObject().put("id", id))
        NativeServices.call("library_evict", JSONObject().put("id", id))
        assertTrue((NativeServices.call("load_budget") as JSONObject).isNull("budget"))
        val restored = NativeServices.call("library_open", JSONObject().put("id", id)) as JSONObject
        assertTrue(restored.getJSONObject("budget").getString("name").startsWith("Android background save"))
        NativeServices.call("account_sign_out")
        val privateId = NativeServices.call("create_budget", JSONObject().put("budget", fixture)) as String
        val rows = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries")
        val privateEntry = (0 until rows.length()).map(rows::getJSONObject).first { it.getString("id") == privateId }
        assertEquals("local", privateEntry.getString("scope"))
    }
}
