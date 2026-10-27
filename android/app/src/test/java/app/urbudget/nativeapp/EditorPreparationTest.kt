package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.filterChoices
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runTest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class EditorPreparationTest {
    @org.junit.Before fun english() = EnglishWords.install()
    @org.junit.Before fun resetFormat() { Money.configure(JSONObject().put("currency", "SAR")) }

    @Test fun bankReviewKeepsTransferLegIdsAndConservativeMatchDefaults() = runTest {
        val budget = JSONObject("""{"transactions":[{"id":"transfer","date":"2026-10-01","payee":"Savings"},{"id":"empty","date":"2026-10-02","payee":null}]}""")
        val rows = listOf(
            JSONObject("""{"candidates":["transfer:leg:2","empty"],"candidateDetails":[{"id":"transfer:leg:2","transactionId":"transfer"}]}"""),
            JSONObject("""{"candidates":[],"amount":-250}"""),
            JSONObject("""{"duplicate":true,"candidates":[]}""")
        )
        val before = budget.toString()
        val prepared = prepareBankReview(rows, budget, false)
        // Two candidates need a choice; a duplicate is locked as already imported.
        assertEquals(listOf("review", "new", "skip"), prepared.map { it.initial })
        assertTrue(prepared[0].options.contains("match:transfer:leg:2" to "Match: 2026-10-01 · Savings · Split 1"))
        assertTrue(prepared[0].options.contains("match:empty" to "Match: 2026-10-02 · Transfer"))
        assertTrue(prepared[2].locked)
        assertFalse(prepared[2].options.any { it.first == "new" })
        val choices = bankReviewChoices(prepared, mapOf(0 to "match:transfer:leg:2"), false)
        assertEquals("match", choices.getJSONObject(0).getString("action"))
        assertEquals("transfer:leg:2", choices.getJSONObject(0).getString("matchId"))
        assertEquals("new", choices.getJSONObject(1).getString("action"))
        assertFalse(choices.getJSONObject(0).getJSONObject("row").has("candidates"))
        assertEquals(before, budget.toString())
    }

    @Test fun theOtherSideOfMoneyMovedElsewhereIsOfferedAsATransfer() = runTest {
        val budget = JSONObject("""{"accounts":[{"id":"checking","name":"Everyday"},{"id":"savings","name":"Savings"}],
            "transactions":[{"id":"move","accountId":"savings","date":"2026-10-02","payee":"TO EVERYDAY","amount":-20000}]}""")
        val sure = JSONObject("""{"candidates":["move"],"previouslyMatchedCandidates":[],"candidateDetails":[],"transferCandidates":["move"],"transferSure":true}""")
        val rows = listOf(JSONObject("""{"id":"bank:in","candidateSet":0,"amount":20000}"""), JSONObject("""{"id":"bank:two","candidateSet":1,"amount":20000}"""))
        val prepared = prepareBankReview(rows, budget, true, JSONArray().put(sure).put(JSONObject(sure.toString()).put("transferSure", false)))
        // Backed up by the bank text, it's chosen; on amount and date alone it's only suggested.
        assertEquals(listOf("match:move", "review"), prepared.map { it.initial })
        assertEquals(listOf("move", "move"), prepared.map { it.transfer })
        assertTrue(prepared[0].options.contains("match:move" to "Transfer from Savings: 2026-10-02 · TO EVERYDAY"))
        assertTrue(summarizeBankReview(prepared).transfers)
        val row = bankReviewChoices(prepared, emptyMap(), true).getJSONObject(0).getJSONObject("row")
        assertFalse(row.has("transferCandidates") || row.has("transferSure"))
    }

    @Test fun statementsAreReadAsTheFormatTheirContentSays() {
        assertEquals("mt940", statementFormat("umsaetze.txt", "{1:F01BANK}{2:I940}{4:\r\n:20:STARTUMS\r\n:25:1/2\r\n:61:2610011001DR1,00NMSC\r\n"))
        assertEquals("camt053", statementFormat("export.xml", "<Document/>"))
        assertEquals("camt053", statementFormat("x.dat", "\uFEFF<?xml version=\"1.0\"?><ns2:Document><ns2:BkToCstmrStmt>"))
        assertEquals("qfx", statementFormat("Chase.QFX", "OFXHEADER:100\nDATA:OFXSGML"))
        assertEquals("ofx", statementFormat("download.csv", "<OFX><STMTTRN>"))
        assertEquals("qif", statementFormat("export.txt", "\n!Option:AutoSwitch\n!Account"))
        assertEquals("mt940", statementFormat("konto.STA", ""))
        assertEquals("csv", statementFormat("konto.csv", "Date,Payee,Amount"))
        assertEquals(null, statementFormat("konto.txt", "Date,Payee,Amount"))
    }

    @Test fun statementsAreReadInTheEncodingsBanksWrite() {
        fun bytes(vararg values: Int) = ByteArray(values.size) { values[it].toByte() }
        // Not UTF-8: the engine tells the encoding, given a character for each byte.
        assertEquals("Tytu\u00b3;\u00afABKA", decodeStatement(bytes(0x54, 0x79, 0x74, 0x75, 0xB3, 0x3B, 0xAF, 0x41, 0x42, 0x4B, 0x41)) { it })
        // A workbook (a ZIP inside) is the engine's to read whole.
        assertEquals("sheet", decodeStatement(bytes(0x50, 0x4B, 0x03, 0x04, 0x14, 0x00), binary = { "sheet" }))
        // Without the engine (as here), Windows-1252.
        assertEquals("Bäck;-3", decodeStatement(bytes(0x42, 0xE4, 0x63, 0x6B, 0x3B, 0x2D, 0x33)))
        assertEquals("Bä", decodeStatement(bytes(0xEF, 0xBB, 0xBF, 0x42, 0xC3, 0xA4)))
        assertEquals("Dä", decodeStatement(bytes(0xFF, 0xFE, 0x44, 0x00, 0xE4, 0x00)))
        assertEquals("Dä", decodeStatement(bytes(0xFE, 0xFF, 0x00, 0x44, 0x00, 0xE4)))
        assertEquals("Date", decodeStatement(bytes(0x44, 0x00, 0x61, 0x00, 0x74, 0x00, 0x65, 0x00)))
    }

    @Test fun intentionalIdlessDuplicateGetsStableRetryIdWithoutMutatingSource() = runTest {
        val row = JSONObject("""{"duplicate":true,"idless":true,"importId":"original","candidates":[]}""")
        val before = row.toString()
        val prepared = prepareBankReview(listOf(row), JSONObject(), true)
        assertEquals("skip", prepared.single().initial)
        assertTrue(prepared.single().options.any { it.first == "new" })
        val first = bankReviewChoices(prepared, mapOf(0 to "new"), true).getJSONObject(0).getJSONObject("row")
        val retry = bankReviewChoices(prepared, mapOf(0 to "new"), true).getJSONObject(0).getJSONObject("row")
        assertTrue(first.getString("importId").startsWith("bank:manual:"))
        assertEquals(first.getString("importId"), retry.getString("importId"))
        assertNotEquals("original", first.getString("importId"))
        assertEquals(before, row.toString())
        assertEquals("original", bankReviewChoices(prepared, emptyMap(), true).getJSONObject(0).getJSONObject("row").getString("importId"))
    }

    @Test fun largePayeePreparationDeduplicatesAndPreservesSavedMetadataAcrossAllSources() = runTest {
        val saved = JSONObject("""{"id":"saved","name":"Shop","categoryId":"food","notes":"Keep metadata"}""")
        val transactions = JSONArray()
        repeat(10_000) { transactions.put(JSONObject().put("payee", if (it % 2 == 0) "Shop" else "Derived")) }
        transactions.put(JSONObject("""{"payee":null,"splits":[{"payee":"Split payee"},{"payee":null}]}"""))
        val budget = JSONObject().put("transactions", transactions).put("payees", JSONArray().put(saved))
            .put("schedules", JSONArray("""[{"transaction":{"payee":"Scheduled"}}]"""))
            .put("payeeRules", JSONArray("""[{"payee":"Rule payee"}]"""))
        val rows = preparePayees(budget)
        assertEquals(listOf("Derived", "Rule payee", "Scheduled", "Shop", "Split payee"), rows.map { it.getString("name") })
        assertSame(saved, rows.first { it.getString("name") == "Shop" })
        assertEquals("Keep metadata", saved.getString("notes"))
        assertEquals(rows.map { it.getString("id") }, preparePayees(budget).map { it.getString("id") })
        assertEquals(1, budget.getJSONArray("payees").length())
    }

    @Test fun fileByteEncodingPreservesUnsignedProtocolExactly() = runTest {
        val source = ByteArray(20_000) { it.toByte() }
        val encoded = encodeFileBytes(source)
        assertEquals(source.size, encoded.length())
        source.indices.forEach { assertEquals(source[it].toInt() and 255, encoded.getInt(it)) }
    }

    @Test fun pooledBankCandidatesStaySharedAndEveryMatchRemainsReachableWithSmallCommit() = runTest {
        val candidates = JSONArray(); val prior = JSONArray(); val transactions = JSONArray()
        repeat(2_000) {
            candidates.put("candidate-$it"); prior.put("candidate-$it")
            transactions.put(JSONObject().put("id", "candidate-$it").put("date", "2026-10-01").put("payee", "Payee $it"))
        }
        val pool = JSONArray().put(JSONObject().put("candidates", candidates).put("previouslyMatchedCandidates", prior))
        val rows = List(2_000) { JSONObject().put("id", "bank-$it").put("candidateSet", 0).put("amount", -100).put("memo", "Preserved") }
        val prepared = prepareBankReview(rows, JSONObject().put("transactions", transactions), false, pool)
        assertSame(prepared.first().options, prepared.last().options)
        assertSame(prepared.first().previouslyMatched, prepared.last().previouslyMatched)
        assertEquals(2_003, prepared.last().options.size)
        assertEquals("match:candidate-1999", prepared.last().options.last().first)
        assertEquals("Match: 2026-10-01 · Payee 1999 · Previously imported", prepared.last().options.last().second)
        assertEquals("review", prepared.last().initial)
        val commits = bankReviewChoices(prepared, mapOf(1_999 to "match:candidate-1999"), false)
        val selected = commits.getJSONObject(1_999).getJSONObject("row")
        assertEquals(listOf("candidate-1999"), selected.getJSONArray("previouslyMatchedCandidates").let { values -> (0 until values.length()).map(values::getString) })
        assertEquals("Preserved", selected.getString("memo"))
        assertFalse(selected.has("candidateSet")); assertFalse(selected.has("candidates")); assertFalse(selected.has("candidateDetails"))
        assertTrue("Commit must scale with bank rows, not rows multiplied by candidates", commits.toString().length < 300_000)
        assertEquals(2_000, candidates.length())
        assertTrue(rows.first().has("candidateSet"))
        assertFalse(rows.last().has("previouslyMatchedCandidates"))
    }

    @Test fun legacyPriorMatchEvidenceIsReducedToOnlyTheChosenId() = runTest {
        val source = JSONObject("""{"id":"row","candidates":["a","b"],"previouslyMatchedCandidates":["a","b"],"candidateDetails":[]}""")
        val prepared = prepareBankReview(listOf(source), JSONObject(), false)
        val committed = bankReviewChoices(prepared, mapOf(0 to "match:b"), false).getJSONObject(0).getJSONObject("row")
        assertEquals("[\"b\"]", committed.getJSONArray("previouslyMatchedCandidates").toString())
        assertFalse(committed.has("candidates"))
        assertEquals(2, source.getJSONArray("previouslyMatchedCandidates").length())
    }

    @Test fun largeChoiceSearchRetainsOrderAndExactOptionIds() = runTest {
        val choices = (0 until 10_000).map { "id-$it" to "Account $it" }
        assertSame(choices, filterChoices(choices, ""))
        assertEquals(listOf("id-9999" to "Account 9999"), filterChoices(choices, "ACCOUNT 9999"))
        assertEquals(listOf("id-12", "id-120", "id-121"), filterChoices(choices, "Account 12").take(3).map { it.first })
        assertTrue(filterChoices(choices, "absent").isEmpty())
    }

    @Test fun cancelledPreparationStopsBeforePublishingEvenForEmptyInputs() = runTest {
        var stopped = 0
        val task = launch {
            currentCoroutineContext().cancel()
            try { preparePayees(JSONObject()); fail("Payee work must be cancelled") } catch (_: CancellationException) { stopped++ }
            try { prepareBankReview(emptyList(), JSONObject(), false); fail("Bank work must be cancelled") } catch (_: CancellationException) { stopped++ }
            try { bankReviewChoices(emptyList(), emptyMap(), false); fail("Import work must be cancelled") } catch (_: CancellationException) { stopped++ }
            try { encodeFileBytes(byteArrayOf()); fail("File encoding must be cancelled") } catch (_: CancellationException) { stopped++ }
            try { filterChoices(emptyList(), ""); fail("Search must be cancelled") } catch (_: CancellationException) { stopped++ }
        }
        task.join()
        assertEquals(5, stopped)
    }
}
