package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class EditorCheckpointTest {
    @Test fun openingBaselineKeepsOnlyRelevantRecordsAndNoTransactionHistory() {
        val budget = JSONObject().put("name", "QA").put("currency", "SAR")
            .put("accounts", JSONArray().put(JSONObject().put("id", "cash")).put(JSONObject().put("id", "other")))
            .put("categories", JSONArray().put(JSONObject().put("id", "food")))
            .put("transactions", JSONArray((0 until 5000).map { JSONObject().put("id", "tx-$it").put("accountId", "cash") }))
        val baseline = EditorCheckpoint.editorBaseline(budget, JSONObject().put("account", "cash").put("category", "food").put("amount", "12 +"))
        assertFalse(baseline.has("transactions"))
        assertEquals(1, baseline.getJSONArray("accounts").length())
        assertEquals("cash", baseline.getJSONArray("accounts").getJSONObject(0).getString("id"))
        assertTrue(baseline.toString().length < 1024)
    }
}
