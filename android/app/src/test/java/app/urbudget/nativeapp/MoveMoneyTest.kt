package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.coverAmount
import org.junit.Assert.assertEquals
import org.junit.Test

/** Covering an overspent category takes what the source can give of the overspending. */
class MoveMoneyTest {
    @Test fun aSourceWithEnoughCoversAllOfTheOverspending() {
        assertEquals(12_000L, coverAmount(12_000, 50_000))
        assertEquals(12_000L, coverAmount(12_000, 12_000))
    }

    @Test fun aSourceShortOfItGivesEverythingItHas() {
        assertEquals(5_000L, coverAmount(12_000, 5_000))
        assertEquals(1L, coverAmount(12_000, 1))
    }

    @Test fun withNothingToGiveTheAmountStillSaysWhatsNeeded() {
        assertEquals(12_000L, coverAmount(12_000, 0))
        assertEquals(12_000L, coverAmount(12_000, -3_000))
        assertEquals(12_000L, coverAmount(12_000, null))
    }
}
