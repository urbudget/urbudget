package app.urbudget.nativeapp

import android.util.Base64
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/** Exercises the real shared assistant through JNI with synthetic files and no provider credentials. */
@RunWith(AndroidJUnit4::class)
class NativeAssistantTest {
    // Files and conversations belong to the open budget, so the assistant needs one.
    @Before fun openBudget() { ServiceTestRuntime.openFixture() }

    @Test fun attachmentConversationAndImmediateAuthFailureSurviveTheNativeBoundary() {
        val budgetBefore = ServiceTestRuntime.call("snapshot_budget") as JSONObject
        ServiceTestRuntime.call("assistant_settings", JSONObject().put("change", JSONObject()
            .put("provider", "chatgpt").put("shareMemos", false).put("keepHistory", true)))
        val status = ServiceTestRuntime.call("assistant_status") as JSONObject
        assertFalse(status.getBoolean("shareMemos"))
        assertFalse("The isolated test runtime must not have real provider credentials", status.getBoolean("ready"))
        val csv = "Date,Payee,Amount\n2026-10-21,Synthetic store,-12.50\n"
        val file = ServiceTestRuntime.call("assistant_attach", JSONObject().put("name", "synthetic-statement.csv")
            .put("data", Base64.encodeToString(csv.toByteArray(), Base64.NO_WRAP))) as JSONObject
        assertEquals("csv", file.getString("kind"))
        assertEquals(csv.toByteArray().size, file.getInt("size"))
        val thread = ServiceTestRuntime.call("assistant_send", JSONObject().put("text", "Review this synthetic statement")
            .put("attachments", JSONArray().put(file.getString("id")))
            .put("screen", JSONObject().put("page", "assistant").put("month", "2026-10").put("today", "2026-10-21").put("selected", JSONArray()))) as String
        try {
            var view = ServiceTestRuntime.call("assistant_thread", JSONObject().put("id", thread)) as JSONObject
            val deadline = System.nanoTime() + 5_000_000_000L
            while (view.getBoolean("running") && System.nanoTime() < deadline) {
                Thread.sleep(20)
                view = ServiceTestRuntime.call("assistant_thread", JSONObject().put("id", thread)) as JSONObject
            }
            assertFalse("Immediate credential failure must clear running", view.getBoolean("running"))
            val entries = view.getJSONArray("entries")
            assertEquals("user", entries.getJSONObject(0).getString("kind"))
            assertEquals(file.getString("id"), entries.getJSONObject(0).getJSONArray("attachments").getJSONObject(0).getString("id"))
            assertEquals("signIn", entries.getJSONObject(entries.length() - 1).getString("code"))
            val threads = ServiceTestRuntime.call("assistant_threads") as JSONArray
            assertTrue((0 until threads.length()).any { threads.getJSONObject(it).getString("id") == thread })
            assertEquals("Assistant turns do not edit budget data", budgetBefore.toString(), (ServiceTestRuntime.call("snapshot_budget") as JSONObject).toString())
        } finally {
            ServiceTestRuntime.call("assistant_delete_thread", JSONObject().put("id", thread))
            ServiceTestRuntime.call("assistant_settings", JSONObject().put("change", JSONObject().put("shareMemos", true)))
        }
        val threads = ServiceTestRuntime.call("assistant_threads") as JSONArray
        assertFalse((0 until threads.length()).any { threads.getJSONObject(it).getString("id") == thread })
    }

    @Test fun unsupportedAttachmentsAndUnsafeConversationPathsAreRejected() {
        val badFile = runCatching { ServiceTestRuntime.call("assistant_attach", JSONObject().put("name", "unsupported.exe")
            .put("data", Base64.encodeToString("synthetic".toByteArray(), Base64.NO_WRAP))) }
        assertTrue(badFile.isFailure)
        assertTrue(badFile.exceptionOrNull()?.message.orEmpty().contains("Attach an image"))
        val badPath = runCatching { ServiceTestRuntime.call("assistant_thread", JSONObject().put("id", "../budget")) }
        assertTrue(badPath.isFailure)
        assertTrue(badPath.exceptionOrNull()?.message.orEmpty().contains("conversation"))
    }
}
