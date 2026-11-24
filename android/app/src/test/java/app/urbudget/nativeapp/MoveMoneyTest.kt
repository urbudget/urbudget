package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.coverAmount
import app.urbudget.nativeapp.ui.moveStartAmount
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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

    @Test fun assigningFromUnassignedOpensWithNoAmount() {
        // Not all of Unassigned for one category.
        assertNull(moveStartAmount(fromUnassigned = true, start = 370_000, cover = null))
        assertNull(moveStartAmount(fromUnassigned = true, start = 0, cover = null))
        // More assigned than there is: taking it back still opens with what's short.
        assertEquals(2_500L, moveStartAmount(fromUnassigned = true, start = -2_500, cover = null))
    }

    @Test fun movingFromACategoryStillOpensWithWhatItHas() {
        assertEquals(8_000L, moveStartAmount(fromUnassigned = false, start = 8_000, cover = null))
        assertEquals(0L, moveStartAmount(fromUnassigned = false, start = 0, cover = null))
        assertEquals(5_000L, moveStartAmount(fromUnassigned = false, start = -12_000, cover = 5_000))
    }
}
