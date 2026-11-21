package app.urbudget.nativeapp.ui

import androidx.compose.runtime.AbstractApplier
import androidx.compose.runtime.BroadcastFrameClock
import androidx.compose.runtime.Composition
import androidx.compose.runtime.Recomposer
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import app.urbudget.nativeapp.BankReviewRow
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

/** The counts under a review's confirm button ("Import 2", "Mark checked"), recomposed as the sheet is. */
@OptIn(ExperimentalCoroutinesApi::class)
class BankReviewCountsTest {
    /** A composition of state alone: nothing is drawn. */
    private object NoNodes : AbstractApplier<Unit>(Unit) {
        override fun insertTopDown(index: Int, instance: Unit) = Unit
        override fun insertBottomUp(index: Int, instance: Unit) = Unit
        override fun remove(index: Int, count: Int) = Unit
        override fun move(from: Int, to: Int, count: Int) = Unit
        override fun onClear() = Unit
    }

    private fun row(initial: String, locked: Boolean = false) =
        BankReviewRow(JSONObject(), emptyList(), initial, "bank:manual:test", emptySet(), locked = locked)

    @Test fun countsFollowRowsThatArriveAfterTheSheetOpensAndTheChoicesMadeSince() = runTest {
        val clock = BroadcastFrameClock()
        val recomposer = Recomposer(coroutineContext + clock)
        val running = launch(clock) { recomposer.runRecomposeAndApplyChanges() }
        val composition = Composition(NoNodes, recomposer)
        // What a change on screen does: the state's observers hear of it, and the next frame recomposes.
        fun TestScope.frame() { Snapshot.sendApplyNotifications(); runCurrent(); clock.sendFrame(0L); runCurrent() }

        // The sheet opens before its rows are prepared, as the SimpleFIN review does.
        var rows by mutableStateOf<List<BankReviewRow>?>(null)
        val decisions = mutableStateMapOf<Int, String>()
        var shown = -1 to -1
        composition.setContent {
            val counts by rememberBankReviewCounts(rows, decisions)
            shown = counts
        }
        frame()
        assertEquals(0 to 0, shown)

        // The rows arrive: one to add, one to match, one to decide, one already imported.
        rows = listOf(row("new"), row("match:t1"), row("review"), row("skip", locked = true))
        frame()
        assertEquals(2 to 1, shown)

        // A choice for the undecided row, then one taken back out of the import.
        decisions[2] = "new"
        frame()
        assertEquals(3 to 0, shown)
        decisions[0] = "skip"
        frame()
        assertEquals(2 to 0, shown)

        // Another download's rows replace these, with the choices cleared.
        decisions.clear()
        rows = listOf(row("skip", locked = true))
        frame()
        assertEquals(0 to 0, shown)

        composition.dispose()
        recomposer.cancel()
        running.cancel()
    }
}
