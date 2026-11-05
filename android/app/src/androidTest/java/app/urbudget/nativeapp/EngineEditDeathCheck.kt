package app.urbudget.nativeapp

import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.io.FileOutputStream
import java.time.LocalDate
import java.util.Random

/**
 * An edit killed halfway, by a real force-stop: `SERVICES` is a process-wide `OnceLock`, so nothing
 * short of a dead process starts it again. Driven by `run_edit_checks.py`, one cycle at a time:
 * `arm` edits until it's killed, and `verify` looks at what it left, in a fresh process.
 *
 *   -e death arm -e seed 3 -e class app.urbudget.nativeapp.EngineEditDeathCheck#arm
 *   -e death verify -e class app.urbudget.nativeapp.EngineEditDeathCheck#verify
 */
@RunWith(AndroidJUnit4::class)
class EngineEditDeathCheck {
    private val arguments get() = InstrumentationRegistry.getArguments()
    private lateinit var session: EditSession
    private val journal get() = File(session.context.filesDir, "edit-death-journal.txt")
    private val today = LocalDate.now().toString()

    @Before fun setup() {
        assumeTrue("Run by run_edit_checks.py", arguments.getString("death") != null)
        requireEmulator()
        session = EditSession()
    }

    private fun comparable(budget: JSONObject) = Canon.digest(Canon.forStateless(budget))
    private fun row(budget: JSONObject, n: Int, ids: List<String>): Triple<String, JSONObject, Boolean> {
        val date = today
        return when (n % 3) {
            0 -> Triple("workflow", Edits.bulk(listOf(ids[n % ids.size]), "memo", "Memo $n"), false)
            // A transfer and a split are what a half-written commit would break.
            1 -> Triple("saveTransaction", JSONObject().put("transaction", Edits.transaction("t-transfer-$n", date, amount = -2_000 - n.toLong())
                .put("splits", JSONArray()).put("transferAccountId", "savings").put("transferStatus", "cleared")), true)
            else -> Triple("saveTransaction", JSONObject().put("transaction", Edits.transaction("t-split-$n", date, amount = -700).put("splits", JSONArray()
                .put(JSONObject().put("categoryId", "c-home-1").put("amount", -400)).put(JSONObject().put("categoryId", "c-fun-1").put("amount", -300)))), true)
        }
    }

    @Test fun arm() {
        val seed = arguments.getString("seed")!!.toLong()
        session.make(SyntheticBudget.build(transactions = 300, seed = seed).put("name", "Death $seed"))
        val vm = session.model()
        val start = vm.state.value.budget!!
        val ids = start.rows("transactions").filter(Edits::plain).map { it.text("id") }
        val stream = FileOutputStream(journal, false)
        fun note(step: Int, digest: String) {
            stream.write("$step $digest\n".toByteArray())
            stream.flush()
            stream.fd.sync()
        }
        note(0, comparable(storedBudget()))
        Log.i(TAG, "armed $seed")
        var n = 1
        // Until the host stops it. This never ends by itself, and it mustn't: a run that isn't killed proves nothing.
        while (true) {
            val before = storedBudget()
            val (method, params, _) = row(before, n, ids)
            // What it will be, on the disk before the edit starts, so the kill can't outrun the record.
            note(n, comparable(changedBudget(stateless(method, before, params))))
            if (method == "workflow") vm.runCommand(method, params) else vm.saveTransaction(params.getJSONObject("transaction"))
            vm.settle()
            assertNull(vm.state.value.error)
            n++
            Thread.sleep(Random(n.toLong()).nextInt(5).toLong())
        }
    }

    @Test fun verify() {
        val lines = journal.readLines().filter { it.endsWith("\n").not() && it.contains(' ') }
        assertTrue("Nothing was journaled", lines.isNotEmpty())
        val last = lines.last().split(" ")
        val step = last[0].toInt()
        val allowed = lines.takeLast(2).map { it.split(" ")[1] }
        val answer = NativeServices.call("saved_budget") as JSONObject
        val stored = answer.getJSONObject("budget")
        assertTrue(stored.getString("name").startsWith("Death"))
        assertTrue("Step $step: the store holds neither that edit nor the one before", comparable(stored) in allowed)
        assertFalse(answer.getBoolean("unsavedDraft"))
        stateless("calculate", stored, JSONObject().put("month", today.take(7)).put("asOfMonth", today.take(7)))
        for (t in stored.rows("transactions")) {
            val parts = t.rows("splits")
            if (parts.isNotEmpty()) assertEquals("Row ${t.text("id")}: a partial split", t.getLong("amount"), parts.sumOf { it.getLong("amount") })
            if (t.text("id").startsWith("t-transfer-")) assertEquals("savings", t.text("transferAccountId"))
        }
        val status = NativeServices.call("native_budget_status") as JSONObject
        assertFalse("Undo doesn't outlive the process", status.getBoolean("undoAvailable"))
        val recovery = File(File(status.getString("databasePath")).parentFile, "recovery.json")
        if (recovery.exists()) NativeServices.call("discard_recovery")

        val vm = session.model()
        val d = EditDriver(vm, strict = true)
        d.verify("a fresh process")
        val row = stored.rows("transactions").first(Edits::plain).text("id")
        d.workflow("one more edit", Edits.bulk(listOf(row), "memo", "After the kill").getJSONObject("action"))
        d.close()
        Log.i(TAG, "verified at step $step, ${if (comparable(stored) == allowed.last()) "with" else "without"} the last edit")

        val id = (NativeServices.call("library_snapshot") as JSONObject).getString("activeId")
        session.track(id)
        session.close()
        journal.delete()
    }

    private companion object { const val TAG = "EngineEditDeath" }
}
