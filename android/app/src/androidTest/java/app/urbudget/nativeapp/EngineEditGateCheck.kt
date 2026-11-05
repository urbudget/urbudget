package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.time.YearMonth
import java.util.Locale

/**
 * The speed gate before the apps use `edit_budget`: one status tap on a real-sized budget, through
 * the real service. Not a test, and it runs only when asked:
 *
 *   adb -s <device> shell am instrument -w -e perf 1 [-e home <library id>] -e class app.urbudget.nativeapp.EngineEditGateCheck app.urbudget.nativeapp.test/androidx.test.runner.AndroidJUnitRunner
 *
 * Its budget ("SMS check gate") is made for the run, local only and never synced. Like SmsLiveCheck's
 * it goes to the Trash afterwards, and the budget that was open is reopened. The result is
 * edit-gate.txt in the app's external files folder.
 */
@RunWith(AndroidJUnit4::class)
class EngineEditGateCheck {
    private val tag = "EngineEditGateCheck"
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext

    private fun library(): List<JSONObject> = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries").let { a -> (0 until a.length()).map { a.getJSONObject(it) } }
    private fun open(): String? = library().firstOrNull { it.optBoolean("current") }?.text("id")

    @Test fun oneStatusTapThroughEditBudget() {
        val arguments = InstrumentationRegistry.getArguments()
        assumeTrue("Run only when asked: -e perf 1", arguments.getString("perf") == "1")
        Words.source = ResourceWords(context.resources)
        NativeServices.initialize(context)
        // The budget to come back to: the one open, unless a stopped run left one of its own open; then -e home <id>.
        val current = open()
        val home = arguments.getString("home") ?: current?.takeIf { id -> library().none { it.text("id") == id && it.text("name").startsWith("SMS check") } }
        check(home != null || current == null) { "A check budget is open: say which to come back to with -e home <id>" }
        val month = YearMonth.now().toString()
        val made = SyntheticBudget.build(transactions = TRANSACTIONS).put("name", "SMS check gate")
        val row = made.rows("transactions").first { it.text("status") == "cleared" && it.isNull("transferAccountId") && it.has("source") }
        val id = row.text("id")
        val bytes = made.toString().toByteArray().size
        var own: String? = null
        try {
            own = NativeServices.call("create_budget", JSONObject().put("budget", made).put("localOnly", true)) as String
            var token = (NativeServices.call("open_budget", JSONObject().put("id", own)) as JSONObject).getString("token")
            val took = (1..TAPS).map { tap ->
                val status = if (tap % 2 == 1) "uncleared" else "cleared"
                val action = JSONObject().put("type", "bulk").put("ids", JSONArray().put(id)).put("operation", "status").put("value", status).put("accountId", "all")
                val step = JSONObject().put("method", "workflow").put("params", JSONObject().put("action", action))
                val request = JSONObject().put("steps", JSONArray().put(step)).put("expectedToken", token).put("month", month).put("asOfMonth", month)
                val began = System.nanoTime()
                val reply = NativeServices.call("edit_budget", request) as JSONObject
                val elapsed = (System.nanoTime() - began) / 1e6
                token = reply.getString("token")
                elapsed
            }
            // Timings of work that didn't land would mean nothing.
            val saved = NativeServices.call("saved_budget") as JSONObject
            assertEquals(token, saved.getString("token"))
            assertEquals(false, saved.getBoolean("unsavedDraft"))
            val stored = saved.getJSONObject("budget").rows("transactions").first { it.text("id") == id }
            assertEquals(if (TAPS % 2 == 1) "uncleared" else "cleared", stored.text("status"))
            assertEquals(row.getJSONObject("source").getString("import_id"), stored.getJSONObject("source").getString("import_id"))

            val sorted = took.sorted()
            val report = "Device ${android.os.Build.MODEL}, ${made.getJSONArray("transactions").length()} transactions, %.2f MB\n".format(Locale.ROOT, bytes / 1_000_000.0) +
                "$TAPS status taps through edit_budget: median %.1f ms, worst %.1f ms\n".format(Locale.ROOT, sorted[sorted.size / 2], sorted.last()) +
                took.joinToString(" ", postfix = "\n") { "%.1f".format(Locale.ROOT, it) }
            android.util.Log.i(tag, report)
            File(context.getExternalFilesDir(null), "edit-gate.txt").writeText(report)
        } finally {
            runCatching { home?.let { NativeServices.call("library_open", JSONObject().put("id", it)) } }
            // Every check budget out of the Trash goes back, a stopped run's too.
            val strays = runCatching { library().filter { it.text("name").startsWith("SMS check") && it.text("lifecycle") !in setOf("trashed", "purged") }.map { it.text("id") } }.getOrDefault(emptyList())
            (listOfNotNull(own) + strays).distinct().filter { it != home }.forEach { budget ->
                runCatching { NativeServices.call("library_trash", JSONObject().put("id", budget)) }
            }
        }
    }

    private companion object {
        const val TRANSACTIONS = 2_742
        const val TAPS = 30
    }
}
