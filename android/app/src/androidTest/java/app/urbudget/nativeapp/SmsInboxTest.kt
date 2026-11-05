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

    @Test fun templatesAreLearnedPerBudgetTheirReadingsCountedAndForgotten() {
        inbox.configure(first, setOf("BankA", "BankB"), 0)
        fun template(id: String, sender: String) = JSONObject().put("id", id).put("sender", sender).put("lines", JSONArray().put("Purchase {currency} {amount} at {payee}"))
            .put("outcome", JSONObject().put("disposition", "transaction").put("kind", "expense").put("sign", "-").put("skip", JSONObject.NULL).put("memo", JSONObject.NULL))
            .put("note", "Purchase at {payee}").put("payee", JSONObject.NULL)
        fun learned(t: JSONObject) = JSONObject().put("template", t).put("example", "Purchase SAR 1.00 at SHOP").put("agreed", 1)
        inbox.learn(first, JSONObject().put("templates", JSONArray().put(learned(template("t1", "banka"))).put(learned(template("t2", "bankb"))))
            .put("payees", JSONArray().put(JSONObject().put("sender", "banka").put("raw", "SHOP").put("payee", "Shop").put("categoryId", JSONObject.NULL))))
        assertEquals(listOf("t1", "t2"), (0 until 2).map { inbox.templates(first).getJSONObject(it).getString("id") })
        assertEquals(0, inbox.templates(second).length())
        // Readings reviewed over two requests, and a row the user changed before adding it.
        repeat(2) { inbox.learn(first, JSONObject().put("stats", JSONObject().put("t1", JSONObject().put("filled", 3).put("confirmed", 2).put("corrected", JSONObject().put("payee", 1))))) }
        inbox.edited(first, "t1", setOf("amount"))
        assertEquals(SmsTemplateSummary(2, 6, 4, mapOf("t1" to 66)), inbox.templateSummary(first))
        val stats = inbox.readableDatabase.rawQuery("SELECT stats FROM sms_templates WHERE budget=? AND id='t1'", arrayOf(first)).use { it.moveToFirst(); JSONObject(it.getString(0)) }
        assertEquals(2, stats.getJSONObject("corrected").getInt("payee")); assertEquals(1, stats.getJSONObject("edited").getInt("amount")); assertEquals(2, stats.getInt("requests"))
        // The user's payee for the bank's name replaces the one learned before, whatever its case.
        inbox.learnPayee(first, "banka", "shop", "Shop & Co", null)
        assertEquals(1, inbox.payees(first).length()); assertEquals("Shop & Co", inbox.payees(first).getJSONObject(0).getString("payee"))
        // A rewrite retires a template; removing a sender forgets its formats; Forget clears the rest.
        inbox.learn(first, JSONObject().put("retired", JSONArray().put("t1")))
        assertEquals(listOf("t2"), (0 until inbox.templates(first).length()).map { inbox.templates(first).getJSONObject(it).getString("id") })
        inbox.configure(first, setOf("BankA"), 0)
        assertEquals(0, inbox.templates(first).length())
        inbox.forgetTemplates(first)
        assertEquals(0, inbox.payees(first).length()); assertEquals(SmsTemplateSummary(0, 0, 0), inbox.templateSummary(first))
    }

    @Test fun skipFormatsAreTrustedOnceConfirmedAndFormatsRetireWhenUnusedOrRewrittenOften() {
        inbox.configure(first, setOf("BankA"), 0)
        fun template(id: String, skip: String?) = JSONObject().put("id", id).put("sender", "banka").put("lines", JSONArray().put("Line $id {n}"))
            .put("outcome", JSONObject().put("disposition", if (skip == null) "transaction" else "ignore").put("skip", skip ?: JSONObject.NULL)).put("note", "").put("payee", JSONObject.NULL)
        fun learned(t: JSONObject, replaces: String? = null) = JSONObject().put("template", t).put("example", "Line 1").put("agreed", 1).put("replaces", replaces ?: JSONObject.NULL)
        fun trusted() = inbox.trusted(first).let { a -> (0 until a.length()).map { a.getString(it) } }
        fun state(id: String) = inbox.templateList(first).firstOrNull { it.id == id }?.state
        val day = 86_400_000L; val start = 1_000 * day
        inbox.learn(first, JSONObject().put("templates", JSONArray().put(learned(template("s1", "rewards"))).put(learned(template("m1", null)))), start)
        assertEquals(emptyList<String>(), trusted())
        // Confirmed again in a later request, a skip format is trusted: its texts stay on the phone. A money format never is.
        val confirmed = JSONObject().put("filled", 2).put("confirmed", 2)
        inbox.learn(first, JSONObject().put("stats", JSONObject().put("s1", confirmed).put("m1", JSONObject(confirmed.toString()))), start)
        assertEquals(listOf("s1"), trusted()); assertEquals("candidate", state("m1"))
        inbox.learn(first, JSONObject().put("stats", JSONObject().put("s1", JSONObject().put("settled", 4))), start)
        assertEquals(4, inbox.templateList(first).first { it.id == "s1" }.stats.getInt("settled"))
        // One correction takes the trust away.
        inbox.learn(first, JSONObject().put("stats", JSONObject().put("s1", JSONObject().put("filled", 1).put("corrected", JSONObject().put("skip", 1)))), start)
        assertEquals(emptyList<String>(), trusted())
        // A format rewritten three times in a month retires with its last rewrite: its texts are read in full.
        var old = "m1"
        listOf("m2", "m3", "m4").forEach { id ->
            inbox.learn(first, JSONObject().put("templates", JSONArray().put(learned(template(id, null), old))).put("retired", JSONArray().put(old)), start + day)
            old = id
        }
        fun stored(id: String) = inbox.readableDatabase.rawQuery("SELECT state FROM sms_templates WHERE budget=? AND id=?", arrayOf(first, id)).use { it.moveToFirst(); it.getString(0) }
        assertEquals(List(4) { "retired" }, listOf("m1", "m2", "m3", "m4").map(::stored))
        assertEquals(listOf("s1"), inbox.templateList(first).map { it.id })
        // Formats unused for half a year retire too.
        inbox.learn(first, JSONObject(), start + 182 * day)
        assertEquals(0, inbox.templates(first).length())
    }

    @Test fun aSenderWhoseTextsTurnNewIsPointedOut() {
        inbox.configure(first, setOf("BankA", "BankB"), 0)
        fun flags(vararg known: Boolean) = JSONArray(known.toList())
        inbox.learn(first, JSONObject().put("senders", JSONObject().put("banka", flags(*BooleanArray(20) { true })).put("bankb", flags(*BooleanArray(20) { true }))))
        assertEquals(emptySet<String>(), inbox.templateSummary(first).drifting)
        inbox.learn(first, JSONObject().put("senders", JSONObject().put("banka", flags(*BooleanArray(20) { it < 12 })).put("bankb", flags(*BooleanArray(20) { it != 5 }))))
        assertEquals(setOf("banka"), inbox.templateSummary(first).drifting)
        inbox.forgetTemplates(first)
        assertEquals(emptySet<String>(), inbox.drifting(first))
    }

    @Test fun aReviewOfAPhonesHistoryIsSavedWholeAndAScanIsCollectedInOneGo() {
        // Larger than SQLite hands back in one value: a review of thousands of texts.
        val large = "x".repeat(3_000_000)
        inbox.saveState(first, "draft", large)
        assertEquals(large, inbox.state(first, "draft"))
        inbox.saveState(first, "draft", null); assertNull(inbox.state(first, "draft"))
        inbox.configure(first, setOf("BankA"), 0)
        val scan = (1L..2_000L).map { at -> message("BankA", at, "Your code is $at") }
        val started = System.nanoTime()
        assertEquals(listOf(first), inbox.captureAll(scan))
        val seconds = (System.nanoTime() - started) / 1e9
        assertTrue("Collecting 2,000 texts took $seconds s", seconds < 5)
        assertEquals(2_000, inbox.pendingCount(first))
        assertEquals(mapOf("banka" to 2_000), inbox.collected(first, 0))
        // Collecting again adds nothing; a later start removes what waits from before it.
        assertTrue(inbox.captureAll(scan).isEmpty())
        inbox.collectFrom(first, 1_001, keep = setOf(scan.first().id))
        assertEquals(1_001, inbox.pendingCount(first))
    }
}

/** What the model returns for a synthetic purchase; [accountId] stands in for the app's or the user's choice. */
internal fun syntheticSmsAnalysis(sms: BankSms, accountId: String? = "checking", ref: String? = null): JSONObject = JSONObject().put("items", JSONArray().put(JSONObject()
    .put("sourceIds", JSONArray().put(sms.id)).put("disposition", "transaction").put("skip", JSONObject.NULL).put("note", "Synthetic purchase")
    .put("evidence", JSONArray().put(JSONObject().put("sourceId", sms.id).put("quote", "USD 12.34"))).put("unsure", JSONArray())
    .put("transaction", JSONObject().put("date", "2026-10-24").put("amount", "-12.34").put("currency", "USD").put("writtenAmount", "12.34")
        .put("payee", "Synthetic Shop").put("memo", "").put("kind", "expense").put("accountRef", ref ?: JSONObject.NULL).put("otherRef", JSONObject.NULL)
        .put("categoryId", JSONObject.NULL).put("accountId", accountId ?: JSONObject.NULL).put("transferAccountId", JSONObject.NULL))))
