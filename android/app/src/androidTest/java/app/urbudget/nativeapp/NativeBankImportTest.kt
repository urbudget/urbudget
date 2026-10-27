package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import kotlinx.coroutines.runBlocking

@RunWith(AndroidJUnit4::class)
class NativeBankImportTest {
    private fun asset(name: String) = InstrumentationRegistry.getInstrumentation().context.assets.open(name).bufferedReader().use { it.readText() }
    private fun call(method: String, budget: JSONObject, params: JSONObject): Any {
        val response = JSONObject(NativeBridge.invoke(JSONObject().put("version", 1).put("method", method)
            .put("budget", budget).put("params", params).toString()))
        assertTrue(response.optString("error"), response.getBoolean("ok"))
        return response.get("value")
    }

    @Test fun previewApplyAndRepeatUseDesktopDuplicateDetectionThroughJni() {
        val budget = JSONObject(asset("native-roundtrip.json"))
        val params = JSONObject().put("accountId", "checking").put("content", asset("native-bank.csv"))
            .put("format", "csv").put("dateFormat", "ymd").put("flipSigns", false)
        val rows = (call("bankPreview", budget, params) as JSONObject).getJSONArray("items")
        assertEquals(1, rows.length())
        assertEquals(-1234L, rows.getJSONObject(0).getLong("amount"))
        assertFalse(rows.getJSONObject(0).getBoolean("duplicate"))
        assertEquals(4, budget.getJSONArray("transactions").length())
        val choices = JSONArray().put(JSONObject().put("row", rows.getJSONObject(0)).put("action", "new").put("matchId", JSONObject.NULL))
        val updated = call("bankApply", budget, JSONObject().put("accountId", "checking").put("choices", choices)) as JSONObject
        assertEquals(5, updated.getJSONArray("transactions").length())
        val repeated = (call("bankPreview", updated, params) as JSONObject).getJSONArray("items")
        assertTrue(repeated.getJSONObject(0).getBoolean("duplicate"))
        val summary = call("calculate", updated, JSONObject().put("month", "2026-10").put("asOfMonth", "2026-10")) as JSONObject
        assertEquals(142766L, summary.records("accounts").first { it.getString("id") == "checking" }.getLong("balance"))
    }

    @Test fun aLineThatDoesntReadIsFixedOnThePhoneAndImportedWithTheRest() = runBlocking {
        val budget = JSONObject(asset("native-roundtrip.json"))
        val content = "Date,Payee,Amount\n2026-10-14,Bakery,-3.20\n2026-10-15,Grocer,n/a\n"
        val base = { JSONObject().put("accountId", "checking").put("content", content) }
        val session = call("bankSession", budget, base().put("sessionId", "s1").put("budgetId", "b")) as JSONObject
        val shown = parseStatement(call("bankStatement", budget, base().put("session", session).put("problemsOnly", true)) as JSONObject)
        assertEquals(listOf("attention"), shown.lines.map { it.kind })
        assertTrue(shown.needsChecking)
        val fixes = fieldFixes(session, shown.lines[0], mapOf("amount" to "-12.40"))!!
        val applied = call("bankApplyFixes", budget, base().put("session", session).put("fixes", fixes)) as JSONObject
        val fixed = applied.getJSONObject("session")
        val preview = call("bankImportPreview", budget, base().put("session", fixed).put("problemsOnly", true)) as JSONObject
        val review = preview.getJSONObject("review")
        val rows = prepareBankReview(review.records("items"), budget, true, review.optJSONArray("candidateSets"))
        assertEquals(listOf(-320L, -1240L), rows.map { it.amount })
        val choices = bankReviewChoices(rows, rows.indices.associateWith { "new" }, true)
        val remember = parseStatement(preview.getJSONObject("statement")).remember
        val updated = call("bankApply", budget, JSONObject().put("accountId", "checking").put("choices", choices).put("remember", remember)) as JSONObject
        assertEquals(budget.getJSONArray("transactions").length() + 2, updated.getJSONArray("transactions").length())
        // The same broken file again: both lines are known, the fixed one by the line as the file writes it.
        val again = call("bankSession", updated, base().put("sessionId", "s2").put("budgetId", "b")) as JSONObject
        val seen = call("bankImportPreview", updated, base().put("session", again).put("problemsOnly", true)) as JSONObject
        assertTrue(seen.getJSONObject("review").records("items").all { it.optBoolean("duplicate") })
    }

    @Test fun repeatedLargeMatchSetsStayCompactAndKeepTheLastMatchUsable() = runBlocking {
        val budget = JSONObject(asset("native-roundtrip.json"))
        val transactions = budget.getJSONArray("transactions")
        repeat(1_000) { index ->
            transactions.put(JSONObject().put("id", "candidate-$index").put("date", "2026-10-15")
                .put("accountId", "checking").put("payee", "Synthetic payee $index").put("memo", "")
                .put("amount", -1234).put("status", "uncleared")
                .put("splits", JSONArray().put(JSONObject().put("categoryId", "food").put("amount", -1234))))
        }
        val csv = buildString {
            append("Date,Payee,Memo,Amount\n")
            repeat(500) { append("2026-10-15,Import $it,Synthetic only,-12.34\n") }
        }
        val params = JSONObject().put("accountId", "checking").put("content", csv).put("format", "csv").put("dateFormat", "ymd")
        val request = JSONObject().put("version", 1).put("method", "bankPreview").put("budget", budget).put("params", params)
        val wire = NativeBridge.invoke(request.toString())
        assertTrue("Repeated candidate metadata must not grow with rows times candidates", wire.length < 1_000_000)
        val reply = JSONObject(wire)
        assertTrue(reply.optString("error"), reply.getBoolean("ok"))
        val preview = reply.getJSONObject("value")
        assertEquals(500, preview.getJSONArray("items").length())
        assertEquals(1, preview.getJSONArray("candidateSets").length())
        assertEquals(1_000, preview.getJSONArray("candidateSets").getJSONObject(0).getJSONArray("candidates").length())
        val prepared = prepareBankReview(preview.records("items"), budget, true, preview.optJSONArray("candidateSets"))
        assertSame(prepared.first().options, prepared.last().options)
        assertTrue(prepared.first().options.any { it.first == "match:candidate-999" })
        val choices = bankReviewChoices(prepared, mapOf(0 to "match:candidate-999"), true)
        assertTrue("Import commits must not resend every candidate", choices.toString().length < 1_000_000)
        val updated = call("bankApply", budget, JSONObject().put("accountId", "checking").put("choices", choices)) as JSONObject
        assertEquals("cleared", updated.records("transactions").first { it.optString("id") == "candidate-999" }.getString("status"))
        assertEquals("uncleared", updated.records("transactions").first { it.optString("id") == "candidate-0" }.getString("status"))
    }
}
