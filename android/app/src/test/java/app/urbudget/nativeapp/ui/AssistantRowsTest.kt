package app.urbudget.nativeapp.ui

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class AssistantRowsTest {
    private fun entry(kind: String, text: String = kind) = JSONObject().put("kind", kind).put("text", text)

    @Test fun stepsInARowAreOneLineAndEverythingElseStandsAlone() {
        val entries = listOf(entry("user"), entry("step", "a"), entry("step", "b"), entry("step", "c"), entry("assistant"), entry("step", "d"), entry("proposal"), entry("step", "e"))
        val rows = assistantRows(entries)
        assertEquals(listOf(0, 1, 4, 5, 6, 7), rows.map { it.first })
        assertEquals(listOf("a", "b", "c"), rows[1].second.map { it.getString("text") })
        assertEquals(listOf(1, 3, 1, 1, 1, 1), rows.map { it.second.size })
        assertEquals(emptyList<Pair<Int, List<JSONObject>>>(), assistantRows(emptyList()))
    }

    @Test fun aCutReplyHasItsOwnRowAndAnEntryOfAnUnknownKindHasNone() {
        val entries = listOf(entry("user"), entry("assistant"), entry("cut"), entry("somethingNewer"), entry("step", "a"), entry("somethingNewer"), entry("step", "b"), entry("assistant"))
        val rows = assistantRows(entries)
        // Each row keeps the index of its first entry, which is what its place in the list is known by.
        assertEquals(listOf(0, 1, 2, 4, 7), rows.map { it.first })
        assertEquals(listOf("user", "assistant", "cut", "step", "assistant"), rows.map { it.second.first().getString("kind") })
        assertEquals(listOf("a", "b"), rows[3].second.map { it.getString("text") })
        assertEquals(emptyList<Pair<Int, List<JSONObject>>>(), assistantRows(listOf(entry("somethingNewer"), JSONObject())))
    }

    @Test fun whereTheUserIsReadsAsThePageItsMonthAndWhatsInView() {
        app.urbudget.nativeapp.Money.useInterface(java.util.Locale.ENGLISH, false)
        assertEquals("Budget · November 2026", assistantPlaceLine("Budget", "2026-11", null, null))
        assertEquals("Transactions · Checking · Groceries", assistantPlaceLine("Transactions", null, "Checking", "Groceries"))
        // A sent message's month and names come as text, empty when there were none.
        assertEquals("Accounts", assistantPlaceLine("Accounts", "", "", ""))
        assertEquals("Budget · October 2026 · Rent", assistantPlaceLine("Budget", "2026-10", null, "Rent"))
    }
}
