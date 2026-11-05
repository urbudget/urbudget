package app.urbudget.nativeapp

import android.app.Activity
import android.os.Bundle
import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.Locale

/**
 * What one clear tap costs, from the tap to the published budget, at two sizes. Not a test, and it
 * runs only when asked:
 *
 *   adb -s emulator-5584 shell am instrument -w -e perf 1 -e class app.urbudget.nativeapp.EngineEditPerformanceCheck app.urbudget.nativeapp.test/androidx.test.runner.AndroidJUnitRunner
 *
 * The result is edit-perf.txt in the app's external files folder, and in `am instrument`'s output.
 */
@RunWith(AndroidJUnit4::class)
class EngineEditPerformanceCheck {
    private lateinit var session: EditSession
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()

    @Before fun setup() {
        assumeTrue("Run only when asked: -e perf 1", InstrumentationRegistry.getArguments().getString("perf") == "1")
        requireEmulator()
        session = EditSession()
    }
    @After fun clean() { if (::session.isInitialized) session.close() }

    private class Run(val transactions: Int, val took: List<Double>) {
        val median get() = took.sorted()[took.size / 2]
        val worst get() = took.max()
        override fun toString() = "%d transactions: median %.1f ms, worst %.1f ms".format(Locale.ROOT, transactions, median, worst)
    }

    private fun toggles(transactions: Int): Run {
        session.make(SyntheticBudget.build(transactions = transactions, seed = 51).put("name", "Performance"))
        val vm = session.model()
        val id = vm.state.value.budget!!.rows("transactions").first(Edits::plain).text("id")
        fun tap(status: String) { vm.runCommand("workflow", Edits.bulk(listOf(id), "status", status)); vm.settle() }
        // The first tap reads rows no later one will.
        tap("uncleared")
        tap("cleared")
        val took = (1..TAPS).map { tap ->
            val status = if (tap % 2 == 1) "uncleared" else "cleared"
            val began = System.nanoTime()
            tap(status)
            (System.nanoTime() - began) / 1e6
        }
        assertNull(vm.state.value.error)
        assertEquals(if (TAPS % 2 == 1) "uncleared" else "cleared", storedRow(id)!!.text("status"))
        return Run(transactions, took)
    }

    private fun report(lines: List<String>) {
        val text = lines.joinToString("\n", postfix = "\n")
        Log.i("EngineEditPerformance", text)
        File(session.context.getExternalFilesDir(null), "edit-perf.txt").writeText(text)
        instrumentation.sendStatus(Activity.RESULT_FIRST_USER, Bundle().apply { putString("edit-perf", text) })
    }

    @Test fun clearTogglesAtTwoSizes() {
        assumeTrue("Not with -e trace 1", InstrumentationRegistry.getArguments().getString("trace") == null)
        val small = toggles(SMALL)
        val large = toggles(LARGE)
        val ratio = large.median / small.median
        report(listOf(small, large).map(Run::toString) + "%d to %d transactions: %.2fx".format(Locale.ROOT, SMALL, LARGE, ratio))
        assertTrue("Size barely matters: %.2fx".format(Locale.ROOT, ratio), ratio < 1.5)
    }

    /**
     * A budget of a real size, left open for a trace: it's made here because the preview build has no
     * test hooks, and that build installs over this one and keeps its data. Run with `-e trace 1`.
     */
    @Test fun leaveABudgetOpenForATrace() {
        assumeTrue("Run only when asked: -e trace 1", InstrumentationRegistry.getArguments().getString("trace") == "1")
        val id = NativeServices.call("create_budget", JSONObject().put("budget", SyntheticBudget.build(transactions = 2_742, seed = 52).put("name", "Trace")).put("localOnly", true)) as String
        NativeServices.call("library_open", JSONObject().put("id", id))
        NativePreferences(session.context).run { automaticSync = false; automaticSchedules = false }
        report(listOf("Open: $id"))
    }

    private companion object {
        const val SMALL = 2_700
        const val LARGE = 10_000
        const val TAPS = 30
    }
}
