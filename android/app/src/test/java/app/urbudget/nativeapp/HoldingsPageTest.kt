package app.urbudget.nativeapp

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/** An investment account's page: the engine's figures (`investmentPage`) picked apart for the screen. */
class HoldingsPageTest {
    @Before fun plainNumbers() { Money.configure(JSONObject().put("currency", "USD")) }

    /** The page the engine gives for the fixture of its own tests (investment_stats.rs), cut to what's read here. */
    private fun page() = JSONObject("""
        {"days":["2025-12-31","2026-09-30","2026-10-30","2026-11-12","2026-11-13"],
         "values":[2000000,2422000,2550000,2690000,2701250],
         "gains":[0,422000,450000,550000,556250],
         "periods":[{"period":"1m","first":1,"from":"2026-09-30","to":"2026-11-13","gain":134250,"rate":0.0534},
                    {"period":"all","first":0,"from":"2025-12-31","to":"2026-11-13","gain":556250,"rate":null}]}""")

    @Test fun sharesAndReturnsAreWrittenAsPercentages() {
        assertEquals(listOf("63%", "<1%", "0%", "100%"), listOf(0.633, 0.004, 0.0, 1.0).map { HoldingsPage.percent(it) })
        assertEquals(listOf("+5.5%", "-10.0%", "0.0%", "0.0%"), listOf(0.0552, -0.1, 0.0, -0.0001).map { HoldingsPage.percent(it, true) })
        // Income and costs are small beside the account: to a tenth, and never "0.0%" for something.
        assertEquals(listOf("0.3%", "0.1%", "<0.1%", "0.0%", "12.0%"), listOf(0.0031, -0.0005, 0.0003, 0.0, -0.12).map { HoldingsPage.tenths(it) })
    }

    @Test fun aPeriodsDaysComeOutOfTheWholeRunWithItsGainsStartingAtNothing() {
        val page = page()
        val month = HoldingsPage.series(page, HoldingsPage.period(page, "1m")!!)
        assertEquals(listOf("2026-09-30", "2026-10-30", "2026-11-12", "2026-11-13"), month.days)
        assertEquals(listOf(2422000L, 2550000L, 2690000L, 2701250L), month.values)
        // As the engine's own test has them: the gain so far on each day, ending at the period's gain.
        assertEquals(listOf(0L, 28000L, 128000L, 134250L), month.gains)
        assertEquals(5, HoldingsPage.series(page, HoldingsPage.period(page, "all")!!).days.size)
        assertNull(HoldingsPage.period(page, "3m"))
        // What it would be worth with no gain: the value less the gain so far.
        assertEquals(listOf(2422000.0, 2522000.0, 2562000.0, 2567000.0), HoldingsPage.before(month))
        // More taken out than was put in leaves nothing to draw.
        assertEquals(listOf(1000.0, null), HoldingsPage.before(HoldingsPage.Series(listOf("a", "b"), listOf(1000L, 200L), listOf(0L, 900L))))
        // A rate or share the engine leaves out is none, not zero.
        assertNull(HoldingsPage.rate(HoldingsPage.period(page, "all")!!, "rate"))
        assertEquals(0.0534, HoldingsPage.rate(HoldingsPage.period(page, "1m")!!, "rate")!!, 1e-12)
    }

    @Test fun theChartsDatesAreMarkedByTheWeekThenByTheMonth() {
        // Under a month: the first day of each new week (weeks turn on Monday).
        val days = listOf("2026-11-02", "2026-11-03", "2026-11-06", "2026-11-09", "2026-11-10", "2026-11-16")
        assertEquals(listOf(null, null, null, HoldingsPage.Tick.Day, null, HoldingsPage.Tick.Day), HoldingsPage.ticks(days))
        // Over four months: the first valued day of each month, the year where it turns.
        val months = listOf("2025-10-31", "2025-11-28", "2025-12-31", "2026-01-30", "2026-02-27", "2026-02-28")
        assertEquals(listOf(null, HoldingsPage.Tick.Month, HoldingsPage.Tick.Month, HoldingsPage.Tick.Year, HoldingsPage.Tick.Month, null), HoldingsPage.ticks(months))
        assertTrue(HoldingsPage.ticks(emptyList()).isEmpty())
    }

    @Test fun theValuesAxisFitsThemAndDoesntGoDownToZero() {
        val ticks = HoldingsPage.fittedTicks(2_422_000.0, 2_701_250.0)
        assertTrue(ticks.first() <= 2_422_000.0 && ticks.last() >= 2_701_250.0)
        assertTrue("An axis from zero would flatten the line", ticks.first() > 2_000_000.0)
        assertTrue(ticks.size in 3..7)
        assertEquals(ticks, ticks.sorted())
        // An account that grew from nothing does start at zero; one that never moved gets the usual axis.
        assertEquals(0.0, HoldingsPage.fittedTicks(0.0, 101_000.0).first(), 0.0)
        assertEquals(niceTicks(5.0, 5.0), HoldingsPage.fittedTicks(5.0, 5.0))
    }

    @Test fun holdingsAreOrderedByValueGainReturnOrName() {
        val rows = listOf(
            JSONObject("""{"symbol":"VWRA","name":"Fund","value":1710000,"gain":210000,"rate":0.14}"""),
            JSONObject("""{"symbol":"2222","name":"oil","value":288000,"gain":-32000,"rate":-0.1}"""),
            JSONObject("""{"symbol":"T","name":"Note","value":492500,"gain":null,"rate":null}"""))
        fun order(by: String) = HoldingsPage.sorted(rows, by).map { it.optString("symbol") }
        assertEquals(listOf("VWRA", "T", "2222"), order("value"))
        // One the broker gives no cost for has no gain or return, and comes last.
        assertEquals(listOf("VWRA", "2222", "T"), order("gain"))
        assertEquals(listOf("VWRA", "2222", "T"), order("rate"))
        assertEquals(listOf("VWRA", "T", "2222"), order("name"))
    }

    @Test fun aHoldingsLineShowsItsSymbolOnlyWhenThatAddsToItsName() {
        assertEquals("VWRA · 120.5 × 141.9087 USD", HoldingsPage.line(JSONObject("""{"symbol":"VWRA","name":"Fund","quantity":"120.5","price":"141.9087137","currency":"USD"}""")))
        // A bond's name is its symbol, and an option's code only spells its name out again.
        assertEquals("5,000 × 98.5 USD", HoldingsPage.line(JSONObject("""{"symbol":"T 4 1/4","name":"T 4 1/4","quantity":"5000","price":"98.5","currency":"USD"}""")))
        assertEquals("1 × 2.5 USD", HoldingsPage.line(JSONObject("""{"symbol":"AAPL  261218C00250000","name":"AAPL 18DEC26 250 C","quantity":"1","price":"2.5","currency":"USD"}""")))
    }

    @Test fun aHoldingsTileCarriesItsSymbolsFirstWord() {
        assertEquals("SPY", HoldingsPage.mark("SPY 18DEC26 710 P", "SPY 18DEC26 710 P"))
        assertEquals("ARCC", HoldingsPage.mark("ARCC 5 1/4 04/12/31", "ARCC 5 1/4 04/12/31"))
        assertEquals("GBLATL", HoldingsPage.mark("GBLATL 3 1/8", "x"))
        // With no symbol, the name's first word, cut to six letters.
        assertEquals("VANGUA", HoldingsPage.mark(" ", "vanguard fund"))
        assertEquals("BRK", HoldingsPage.mark("BRK.B", "Berkshire"))
    }
}
