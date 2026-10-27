package app.urbudget.nativeapp

import kotlinx.coroutines.test.runTest
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.time.LocalDate

class PlanningMathTest {
    @org.junit.Before fun english() = EnglishWords.install()

    private fun schedule(id: String, next: String?, amount: Long, paused: Boolean = false) = JSONObject().put("id", id).put("paused", paused)
        .put("nextDate", next ?: JSONObject.NULL).put("transaction", JSONObject().put("amount", amount))

    @Test fun finishedSchedulesAreSkippedAndPausedOnesGoLast() {
        val list = listOf(schedule("late", "2026-12-01", -100), schedule("done", null, -100), schedule("blank", "", -1),
            schedule("paused", "2026-10-01", -100, paused = true), schedule("soon", "2026-10-22", 500))
        assertEquals(listOf("soon", "late", "paused"), upcomingSchedules(list).map { it.getString("id") })
    }

    @Test fun summaryCountsTheNextThirtyDaysWithoutPausedSchedules() {
        val today = LocalDate.parse("2026-10-21")
        val upcoming = upcomingSchedules(listOf(schedule("rent", "2026-11-01", -150_000), schedule("pay", "2026-10-25", 400_000),
            schedule("due", "2026-10-20", -2_000), schedule("edge", "2026-11-20", -1_000), schedule("later", "2026-11-21", -9_999),
            schedule("paused", "2026-10-22", -5_000, paused = true)))
        val summary = scheduleSummary(upcoming, today)
        assertEquals(ScheduleSummary(active = 5, paused = 1, dueSoon = 4, goingOut = 153_000, comingIn = 400_000), summary)
    }

    @Test fun projectionsStartNextMonthOnceThisMonthsPaymentIsIn() = runTest {
        val unpaid = JSONObject("""{"transactions":[
            {"id":"old","date":"2026-09-30","accountId":"cash","amount":-100,"transferAccountId":"loan","splits":[]},
            {"id":"later","date":"2026-10-25","accountId":"cash","amount":-100,"transferAccountId":"loan","splits":[]}]}""")
        assertEquals("2026-10-21", nextLoanPayment(unpaid, "loan", "2026-10-21"))
        val paid = JSONObject("""{"transactions":[{"id":"p","date":"2026-10-03","accountId":"cash","amount":-100,"transferAccountId":"loan","splits":[]}]}""")
        assertEquals("2026-11-01", nextLoanPayment(paid, "loan", "2026-10-21"))
        // Recorded on the loan's side, or as a split, it counts the same.
        val fromLoan = JSONObject("""{"transactions":[{"id":"p","date":"2026-10-03","accountId":"loan","amount":100,"transferAccountId":"cash","splits":[]}]}""")
        assertEquals("2026-11-01", nextLoanPayment(fromLoan, "loan", "2026-10-21"))
        val split = JSONObject("""{"transactions":[{"id":"p","date":"2026-12-03","accountId":"cash","amount":-100,"splits":[{"transferAccountId":"loan","amount":-100}]}]}""")
        assertEquals("2027-01-01", nextLoanPayment(split, "loan", "2026-12-21"))
    }

    @Test fun paymentsComeFromTheAccountThatPaidLastElseCheckingWithTheMost() {
        val sources = listOf(JSONObject("""{"id":"save","kind":"cash","accountType":"savings"}"""), JSONObject("""{"id":"check","kind":"cash"}"""),
            JSONObject("""{"id":"check2","kind":"cash","accountType":"checking"}"""))
        val balances = mapOf("save" to 900_000L, "check" to 100L, "check2" to 5_000L)
        assertEquals("check2", defaultLoanSource(JSONObject("""{"transactions":[]}"""), "loan", sources, balances))
        val budget = JSONObject("""{"transactions":[
            {"date":"2026-08-01","accountId":"check","amount":-100,"transferAccountId":"loan"},
            {"date":"2026-09-01","accountId":"save","amount":-100,"transferAccountId":"loan"},
            {"date":"2026-10-01","accountId":"elsewhere","amount":-100,"transferAccountId":"loan"}]}""")
        assertEquals("save", defaultLoanSource(budget, "loan", sources, balances))
    }

    @Test fun spansRatesAndRateChanges() {
        assertEquals("0 months", span(0))
        assertEquals("1 month", span(1))
        assertEquals("2 years 3 months", span(27))
        assertEquals("1 year", span(12))
        val terms = JSONObject("""{"annualRateBasisPoints":525,"rateHistory":[{"month":"2026-01","annualRateMilliPercent":6000},{"month":"2027-01","annualRateMilliPercent":4125}]}""")
        assertEquals(5_250, rateIn(terms, "2025-12"))
        assertEquals(6_000, rateIn(terms, "2026-10"))
        assertEquals(4_125, rateIn(terms, "2027-03"))
        assertEquals("4.125", rateText(4_125))
        assertEquals("6", rateText(6_000))
        assertEquals("5,25", rateText(5_250, ","))
        assertEquals(5_125, parseRateChange("5.125"))
        assertEquals(5_125, parseRateChange(" 5,125 % ", ","))
        assertEquals(0, parseRateChange("0"))
        assertEquals(1_000_000, parseRateChange("1000"))
        listOf("5.1234", "-1", "abc", "", "1000.001").forEach { text ->
            assertThrows(text, IllegalArgumentException::class.java) { parseRateChange(text) }
        }
    }
}
