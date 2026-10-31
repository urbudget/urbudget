package app.urbudget.nativeapp

import android.content.Intent
import android.net.Uri
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.Assume.assumeTrue
import org.junit.runner.RunWith
import java.net.Socket
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** Real JNI and loopback sockets. The remote transport probe requires explicit opt-in. */
@RunWith(AndroidJUnit4::class)
class NativeSignInTest {
    /** Explicit opt-in smoke check: a made-up grant must reach OpenAI and be rejected. */
    @Test fun tokenTransportWithSyntheticGrant() {
        assumeTrue(InstrumentationRegistry.getArguments().getString("checkAuthNetwork") == "true")
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        // Initialize only the isolated cache runtime. Avoid status/credential reads so
        // this opt-in transport probe is also safe on an installed preview's device.
        ServiceTestRuntime.call("host_events")
        call("host_events")
        call("assistant_foreground", JSONObject().put("active", false))
        val executor = Executors.newSingleThreadExecutor()
        val pending = executor.submit<JSONObject> {
            rawCall("assistant_sign_in", JSONObject().put("returnPackage", context.packageName))
        }
        try {
            var authorize: Uri? = null
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
            while (authorize == null && System.nanoTime() < deadline) {
                val events = call("host_events") as JSONArray
                for (i in 0 until events.length()) {
                    val event = events.getJSONObject(i)
                    if (event.optString("event") == "host://open-url") authorize = Uri.parse(event.getJSONObject("payload").getString("url"))
                }
                if (authorize == null) Thread.sleep(50)
            }
            val start = requireNotNull(authorize)
            val callback = Uri.parse(start.getQueryParameter("redirect_uri")).buildUpon()
                .appendQueryParameter("state", start.getQueryParameter("state"))
                .appendQueryParameter("code", "urbudget-synthetic-network-test")
                .appendQueryParameter("client_id", "oaiapp_urbudget_synthetic_network_test").build()
            Socket("127.0.0.1", callback.port).use { socket ->
                socket.soTimeout = 5000
                socket.getOutputStream().write("GET ${callback.encodedPath}?${callback.encodedQuery} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n".toByteArray())
                socket.getInputStream().bufferedReader().readText()
            }
            Thread.sleep(500)
            assertFalse("Token exchange must wait for the app to return", pending.isDone)
            call("assistant_foreground", JSONObject().put("active", true))
            val result = pending.get(40, TimeUnit.SECONDS)
            assertFalse(result.getBoolean("ok"))
            val error = result.getString("error")
            assertTrue("Synthetic token transport result: $error", error.startsWith("ChatGPT didn't accept the sign-in:"))
        } finally {
            call("assistant_foreground", JSONObject().put("active", true))
            call("assistant_cancel_sign_in")
            executor.shutdownNow()
        }
    }

    @Test fun browserReturnTargetsThisInstallAndTheCallbackEndsThePendingAttempt() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        ServiceTestRuntime.call("assistant_status") // Initialize the isolated test runtime.
        call("host_events")
        val executor = Executors.newSingleThreadExecutor()
        val pending = executor.submit<JSONObject> {
            rawCall("assistant_sign_in", JSONObject().put("returnPackage", context.packageName))
        }
        try {
            var authorize: Uri? = null
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
            while (authorize == null && System.nanoTime() < deadline) {
                val events = call("host_events") as JSONArray
                for (i in 0 until events.length()) {
                    val event = events.getJSONObject(i)
                    if (event.optString("event") == "host://open-url") authorize = Uri.parse(event.getJSONObject("payload").getString("url"))
                }
                if (authorize == null) Thread.sleep(50)
            }
            val start = requireNotNull(authorize) { "Sign-in did not open its authorization URL" }
            assertTrue((call("assistant_status") as JSONObject).getBoolean("signingIn"))
            val callback = Uri.parse(start.getQueryParameter("redirect_uri")).buildUpon()
                .appendQueryParameter("state", start.getQueryParameter("state"))
                .appendQueryParameter("error", "access_denied").build()
            val page = Socket("127.0.0.1", callback.port).use { socket ->
                socket.soTimeout = 5000
                socket.getOutputStream().write("GET ${callback.encodedPath}?${callback.encodedQuery} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n".toByteArray())
                socket.getInputStream().bufferedReader().readText()
            }
            val href = Regex("href=\"([^\"]+)\"").find(page)!!.groupValues[1]
            val intent = Intent.parseUri(href, Intent.URI_INTENT_SCHEME)
            assertEquals(context.packageName, intent.`package`)
            assertEquals("${context.packageName}://assistant/sign-in", intent.data.toString())
            assertTrue(intent.flags and Intent.FLAG_ACTIVITY_CLEAR_TOP != 0)
            assertTrue(intent.flags and Intent.FLAG_ACTIVITY_SINGLE_TOP != 0)
            assertEquals("app.urbudget.nativeapp.MainActivity", context.packageManager.resolveActivity(intent, 0)?.activityInfo?.name)
            val result = pending.get(5, TimeUnit.SECONDS)
            assertFalse(result.getBoolean("ok"))
            assertTrue(result.getString("error").contains("cancelled"))
            val status = call("assistant_status") as JSONObject
            assertFalse(status.getBoolean("signingIn"))
            assertFalse(status.getBoolean("ready"))
            assertEquals("failed", status.getString("signInPhase"))
            assertEquals(result.getString("error"), status.getString("signInError"))
            // A later status refresh (including after reopening Settings) retains the reason.
            assertEquals(status.getString("signInError"), (call("assistant_status") as JSONObject).getString("signInError"))
        } finally {
            call("assistant_cancel_sign_in")
            executor.shutdownNow()
        }
    }

    private fun rawCall(method: String, params: JSONObject = JSONObject()) =
        JSONObject(NativeBridge.service(JSONObject().put("version", 1).put("method", method).put("params", params).toString()))
    private fun call(method: String, params: JSONObject = JSONObject()): Any? {
        val result = rawCall(method, params)
        check(result.getBoolean("ok")) { result.optString("error") }
        return result.opt("value")
    }
}
