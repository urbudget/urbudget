package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SampleBudgetTest {
    private fun rows(vararg ids: String) = JSONArray(ids.map { JSONObject().put("id", it) })
    private fun marked(from: String? = null) = JSONObject().put("sample", JSONObject().apply { if (from != null) put("from", from) })

    @Test fun samplesAreKnownByTheirMarkOrTheEngineSamplesRecords() {
        assertTrue(isSampleBudget(marked()))
        assertTrue(isSampleBudget(marked("b1")))
        // One made before samples were marked.
        assertTrue(isSampleBudget(JSONObject().put("accounts", rows("sample-checking")).put("transactions", rows("sample-market"))))
        // A fresh start from it keeps the account but not the cleared purchase.
        assertFalse(isSampleBudget(JSONObject().put("accounts", rows("sample-checking")).put("transactions", rows())))
        assertFalse(isSampleBudget(JSONObject().put("name", "My budget").put("accounts", rows("checking"))))
    }

    @Test fun leavingASampleGoesToTheBudgetItCameFromElseTheLastOpened() {
        val library = listOf(
            JSONObject().put("id", "sample").put("current", true),
            JSONObject().put("id", "missing").put("missing", true),
            JSONObject().put("id", "recent"),
            JSONObject().put("id", "home"),
        )
        assertEquals("home", sampleExit(marked("home"), library)?.optString("id"))
        assertEquals("recent", sampleExit(marked(), library)?.optString("id"))
        assertEquals("recent", sampleExit(marked("gone"), library)?.optString("id"))
        assertNull(sampleExit(marked(), listOf(JSONObject().put("id", "sample").put("current", true))))
    }
}
