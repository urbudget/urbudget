package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SmsInboxTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private val inbox get() = SmsInbox.get(context)
    private val first = "sms-test-one"
    private val second = "sms-test-two"
    @Before fun reset() { inbox.clear(first); inbox.clear(second); Words.source = ResourceWords(context.resources) }
    @After fun clean() { inbox.clear(first); inbox.clear(second) }
    private fun message(sender: String, time: Long = 2_000, body: String = "Purchase SAR 12.34 at Synthetic Shop") = BankSms(smsId(sender, time, body), sender, time, body)

    @Test fun multipleSendersAreExplicitAndBudgetScopedAndCaptureDoesNotInterpretMessages() {
        inbox.configure(first, setOf("BankA", "BankB"), 1_000)
        inbox.configure(second, setOf("BankC"), 1_000)
        listOf(message("BankA"), message("BankB"), message("BankC"), message("Personal"), message("BankA", 999),
            message("BankA", 2_001, "OTP 123456 for purchase SAR 12.34 at Shop")).forEach(inbox::capture)
        assertEquals(setOf("BankA", "BankB"), inbox.pending(first).map { it.sender }.toSet())
        assertEquals(3, inbox.pendingCount(first)) // Raw preview lets the user exclude OTPs before any upload.
        assertEquals(listOf("BankC"), inbox.pending(second).map { it.sender })
    }

    @Test fun repeatedCaptureAndDismissalDoNotRequeueAndRawTextIsErased() {
        inbox.configure(first, setOf("BankA"), 0)
        val sms = message("BankA")
        repeat(3) { inbox.capture(sms) }
        assertEquals(1, inbox.pendingCount(first))
        inbox.finish(first, listOf(sms.id))
        inbox.capture(sms)
        assertTrue(inbox.pending(first).isEmpty())
        inbox.readableDatabase.rawQuery("SELECT body,done FROM messages WHERE budget=? AND id=?", arrayOf(first, sms.id)).use {
            assertTrue(it.moveToFirst()); assertEquals("", it.getString(0)); assertEquals(1, it.getInt(1))
        }
    }

    @Test fun removingSenderErasesOnlyItsPendingMessagesAndStoppingPreventsCapture() {
        inbox.configure(first, setOf("BankA", "BankB"), 0)
        inbox.capture(message("BankA")); inbox.capture(message("BankB"))
        inbox.configure(first, setOf("BankB"), 3_000)
        assertEquals(listOf("BankB"), inbox.pending(first).map { it.sender })
        assertEquals(0L, inbox.sources(first).single().since)
        inbox.capture(message("BankA", 4_000))
        assertEquals(1, inbox.pendingCount(first))
        inbox.clear(first); inbox.capture(message("BankB", 5_000))
        assertTrue(inbox.pending(first).isEmpty())
    }

    @Test fun providerStyleAndBroadcastStyleCapturesUseOneFingerprint() {
        inbox.configure(first, setOf("BankA"), 0)
        val live = message("BankA")
        inbox.capture(live)
        inbox.capture(live.copy(sender = "BANKA", received = 2_050))
        assertEquals(1, inbox.pendingCount(first))
        inbox.capture(message("BankA", 3_000))
        assertEquals(2, inbox.pendingCount(first))
    }

    private fun call(method: String, budget: JSONObject, params: JSONObject): JSONObject {
        val reply = JSONObject(NativeBridge.invoke(JSONObject().put("version", 1).put("method", method).put("budget", budget).put("params", params).toString()))
        assertTrue(reply.optString("error"), reply.getBoolean("ok"))
        return reply.optJSONObject("value") ?: JSONObject().put("items", reply.getJSONArray("value"))
    }

    @Test fun assistantProposalGoesThroughReviewAndJniAndIsRecognizedOnRetry() = runBlocking {
        val budget = JSONObject(InstrumentationRegistry.getInstrumentation().context.assets.open("native-roundtrip.json").bufferedReader().use { it.readText() })
        val sms = message("BankA", 1_792_800_000_000, "Purchase USD 12.34 at Synthetic Shop on 2026-10-24")
        val analysis = syntheticSmsAnalysis(sms)
        val base = { SmsBatch(listOf(sms), analysis).params() }
        val preview = call("smsBatchPreview", budget, base())
        val rows = preview.getJSONArray("items")
        assertEquals(1, rows.length())
        assertEquals(-1234L, rows.getJSONObject(0).getJSONObject("row").getLong("amount"))
        val updated = call("smsBatchApply", budget, base().put("decisions", JSONArray().put(JSONObject().put("index", 0).put("action", "new").put("matchId", JSONObject.NULL))))
        val txs = updated.getJSONArray("transactions")
        assertEquals(budget.getJSONArray("transactions").length() + 1, txs.length())
        assertEquals("uncleared", txs.getJSONObject(txs.length() - 1).getString("status"))
        assertTrue(call("smsBatchPreview", updated, base()).getJSONArray("items").getJSONObject(0).getJSONObject("row").getBoolean("duplicate"))
    }

    @Test fun draftAndMappingsAreBudgetScopedAndClearingRemovesBoth() {
        inbox.saveState(first, "draft", "private draft"); inbox.saveState(first, "mappings", "private mapping")
        assertNull(inbox.state(second, "draft")); assertEquals("private draft", inbox.state(first, "draft"))
        inbox.clear(first); assertNull(inbox.state(first, "draft")); assertNull(inbox.state(first, "mappings"))
    }
}

internal fun syntheticSmsAnalysis(sms: BankSms): JSONObject = JSONObject().put("items", JSONArray().put(JSONObject()
    .put("sourceIds", JSONArray().put(sms.id)).put("disposition", "transaction").put("reason", "Synthetic purchase")
    .put("evidence", JSONArray().put(JSONObject().put("sourceId", sms.id).put("quote", "USD 12.34")))
    .put("question", JSONObject.NULL).put("accountReference", JSONObject.NULL)
    .put("transaction", JSONObject().put("accountId", "checking").put("date", "2026-10-24").put("amount", "-12.34").put("currency", "USD")
        .put("payee", "Synthetic Shop").put("memo", "SMS").put("kind", "expense").put("categoryId", JSONObject.NULL).put("transferAccountId", JSONObject.NULL))))
