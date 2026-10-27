package app.urbudget.nativeapp

import kotlinx.coroutines.test.runTest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class BankReviewDefaultsTest {
    @Before fun english() = EnglishWords.install()
    @Before fun resetFormat() { Money.configure(JSONObject().put("currency", "SAR")) }

    @Test fun defaultsFollowTheDesktop() {
        assertEquals("skip", defaultBankChoice(duplicate = true, possibleDuplicate = true, candidates = listOf("a"), previouslyMatched = emptySet()))
        assertEquals("review", defaultBankChoice(duplicate = false, possibleDuplicate = true, candidates = emptyList(), previouslyMatched = emptySet()))
        assertEquals("new", defaultBankChoice(duplicate = false, possibleDuplicate = false, candidates = emptyList(), previouslyMatched = emptySet()))
        assertEquals("match:a", defaultBankChoice(duplicate = false, possibleDuplicate = false, candidates = listOf("a"), previouslyMatched = emptySet()))
        // A side already imported from another source matches only when someone chooses it.
        assertEquals("review", defaultBankChoice(duplicate = false, possibleDuplicate = false, candidates = listOf("a"), previouslyMatched = setOf("a")))
        assertEquals("review", defaultBankChoice(duplicate = false, possibleDuplicate = false, candidates = listOf("a", "b"), previouslyMatched = emptySet()))
    }

    @Test fun aSingleCleanMatchIsImportedAndClearedByDefault() = runTest {
        val budget = JSONObject("""{"currency":"SAR","transactions":[{"id":"t1","date":"2026-10-01","payee":"Shop","memo":"Weekly","amount":-500,"splits":[{"amount":-500,"memo":""}]}]}""")
        val rows = listOf(JSONObject("""{"id":"bank-1","date":"2026-10-02","payee":"SHOP 123","rename":"Shop","memo":"Card","amount":-500,"candidates":["t1"]}"""))
        val prepared = prepareBankReview(rows, budget, true)
        val row = prepared.single()
        assertEquals("match:t1", row.initial)
        assertEquals("Shop", row.payee)
        assertEquals("SHOP 123", row.bankText)
        assertEquals("2026-10-02 · Card", row.detail)
        assertEquals("-5.00 SAR", row.amountText)
        assertTrue(row.options.contains("match:t1" to "Match: 2026-10-01 · Shop · Weekly"))
        assertEquals("review", row.options.first().first)
        val choice = bankReviewChoices(prepared, emptyMap(), true).getJSONObject(0)
        assertEquals("match", choice.getString("action"))
        assertEquals("t1", choice.getString("matchId"))
        assertEquals(1 to 0, bankReviewCounts(prepared, emptyMap()))
    }

    @Test fun undecidedRowsBlockTheImportAndAreNeverSentAsImports() = runTest {
        val budget = JSONObject("""{"transactions":[{"id":"a","date":"2026-10-01","payee":"A","splits":[{"memo":"first"},{"memo":"second part"}]},{"id":"b","date":"2026-10-01","payee":"B"}]}""")
        val rows = listOf(
            JSONObject("""{"id":"1","amount":-1,"candidates":["a:leg:1","b"],"candidateDetails":[{"id":"a:leg:1","transactionId":"a","splitIndex":1}]}"""),
            JSONObject("""{"id":"2","amount":-1,"possibleDuplicate":true,"idless":true,"candidates":[]}"""),
            JSONObject("""{"id":"3","amount":-1,"candidates":["b"],"previouslyMatchedCandidates":["b"]}"""),
            JSONObject("""{"id":"4","amount":-1,"duplicate":true,"candidates":["b"]}"""))
        val prepared = prepareBankReview(rows, budget, false)
        assertEquals(listOf("review", "review", "review", "skip"), prepared.map { it.initial })
        assertTrue(prepared[0].options.contains("match:a:leg:1" to "Match: 2026-10-01 · A · Split 2 · second part"))
        assertEquals("May already be imported — choose", prepared[1].options.first().second)
        assertTrue(prepared[2].options.contains("match:b" to "Match: 2026-10-01 · B · Previously imported"))
        // Bank sync can't add an already-imported row again, or match it.
        assertTrue(prepared[3].locked)
        assertTrue(prepared[3].options.isEmpty())
        assertEquals("skip", prepared[3].decision("new"))
        assertEquals(0 to 3, bankReviewCounts(prepared, emptyMap()))
        val choices = bankReviewChoices(prepared, emptyMap(), false)
        assertEquals(listOf("skip", "skip", "skip", "skip"), (0 until choices.length()).map { choices.getJSONObject(it).getString("action") })
        val decided = mapOf(0 to "match:a:leg:1", 1 to "new", 2 to "match:b")
        assertEquals(3 to 0, bankReviewCounts(prepared, decided))
        val sent = bankReviewChoices(prepared, decided, false)
        assertEquals(listOf("match", "new", "match", "skip"), (0 until sent.length()).map { sent.getJSONObject(it).getString("action") })
        assertEquals(JSONArray().put("b").toString(), sent.getJSONObject(2).getJSONObject("row").getJSONArray("previouslyMatchedCandidates").toString())
    }

    @Test fun fileImportsCanAddAnIdlessDuplicateSeparately() = runTest {
        val prepared = prepareBankReview(listOf(JSONObject("""{"id":"1","amount":-1,"duplicate":true,"idless":true,"candidates":["x"]}""")), JSONObject(), true)
        val row = prepared.single()
        assertFalse(row.locked)
        assertEquals(listOf("skip", "new"), row.options.map { it.first })
        assertEquals("skip", row.initial)
        assertEquals(1 to 0, bankReviewCounts(prepared, mapOf(0 to "new")))
    }

    @Test fun summaryCountsDuplicatesRenamesAndAllIncomingFiles() = runTest {
        val rows = listOf(
            JSONObject("""{"id":"1","amount":100,"duplicate":true,"rename":"Pay","payee":"PAYROLL"}"""),
            JSONObject("""{"id":"2","amount":100,"rename":"Pay","payee":"PAYROLL"}"""),
            JSONObject("""{"id":"3","amount":100,"rename":"Same","payee":"Same"}"""),
            JSONObject("""{"id":"4","amount":100,"idless":true,"possibleDuplicate":true}"""))
        val summary = summarizeBankReview(prepareBankReview(rows, JSONObject(), true))
        assertEquals(BankReviewSummary(rows = 4, duplicates = 1, renamed = 1, idless = true, allIn = true), summary)
    }
}
