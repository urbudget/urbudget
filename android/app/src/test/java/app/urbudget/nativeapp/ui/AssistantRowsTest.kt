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
}
