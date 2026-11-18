package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.smsQuery
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

/**
 * A card of the assistant's that enters bank texts straight into the budget, through the app's own
 * view model and the real engine: the transactions go in as a review's do, and Bank messages
 * finishes their texts, so its last text handled moves with them.
 */
@RunWith(AndroidJUnit4::class)
class BankTextsEnterTest {
    private lateinit var session: EditSession
    private val inbox get() = SmsInbox.get(session.context)

    @Before fun setup() { requireEmulator(); session = EditSession() }
    @After fun close() { session.close() }

    private fun sms(time: Long, body: String) = BankSms(smsId("BankA", time, body), "BankA", time, body)
    private fun read(text: BankSms) = JSONObject().put("sourceIds", JSONArray().put(text.id)).put("disposition", "unclear").put("skip", JSONObject.NULL)
        .put("note", "Waiting").put("evidence", JSONArray()).put("unsure", JSONArray())

    @Test fun aCardEntersItsTextsAndBankMessagesCountsThemHandled() = runBlocking {
        val fixture = JSONObject(InstrumentationRegistry.getInstrumentation().context.assets.open("native-roundtrip.json").bufferedReader().use { it.readText() })
        session.make(fixture.put("name", "Bank texts entered"))
        val vm = session.model()
        val identity = vm.state.value.budgetIdentity!!
        val budget = vm.state.value.budget!!
        inbox.clear(identity)
        try {
            // One text from before collecting starts (never collected), one waiting in the open review, and one waiting beside it.
            val early = sms(1_792_800_000_000, "Purchase USD 12.34 at Synthetic Shop on 2026-10-24\nCard 3358")
            val waiting = sms(1_792_900_000_000, "Purchase USD 30.00 at Diner on 2026-10-25")
            val other = sms(1_792_950_000_000, "Purchase USD 8.00 at Kiosk on 2026-10-26")
            inbox.configure(identity, setOf("BankA"), 1_792_850_000_000)
            inbox.captureAll(listOf(early, waiting, other))
            assertEquals(listOf(waiting, other), inbox.pending(identity))
            assertNull(inbox.handledThrough(identity))
            inbox.saveState(identity, "draft", SmsBatch(listOf(waiting, other), JSONObject().put("items", JSONArray(listOf(waiting, other).map(::read)))).saved())

            // As the assistant asks: the engine checks the reading, and nothing in the budget could be either.
            val texts = listOf(early, waiting).associateBy { it.id }
            val asked = listOf(
                BankTextEntry(listOf(early.id), "checking", "2026-10-24", -1_234L, "Synthetic Shop", "expense"),
                BankTextEntry(listOf(waiting.id), "card", "2026-10-25", -3_000L, "Diner", "expense"))
            assertNull(bankTextEntriesProblem(asked, texts, budget))
            val checked = vm.smsQuery("smsBatchPreview", bankTextsReading(asked, texts, "USD").params()).rows("items").associate { it.getInt("index") to it.getJSONObject("row") }
            val decided = bankTextsDecide(asked.map { it.copy(match = "new") }, checked, budget)
            assertEquals(2, decided.entries.size)
            val card = JSONObject().put("action", "enter").put("entries", JSONArray(decided.entries.map { it.card() }))
                .put("texts", JSONArray(texts.values.map { JSONObject().put("id", it.id).put("sender", it.sender).put("received", it.received).put("body", it.body) }))

            val before = storedBudget().rows("transactions").size
            val told = BankTextsAssistant.entered.value
            val done = BankTextsAssistant.apply(session.context, vm, card)
            assertNull(done.page); assertNull(done.notice)
            assertNull(vm.state.value.error)

            // Both are in the budget, in their accounts, marked as from these texts.
            val saved = storedBudget()
            val added = saved.rows("transactions").filter { it.text("payee") in setOf("Synthetic Shop", "Diner") }
            assertEquals(before + 2, saved.rows("transactions").size)
            assertEquals(setOf("checking" to -1_234L, "card" to -3_000L), added.map { it.text("accountId") to it.getLong("amount") }.toSet())
            val imported = saved.getJSONArray("smsImportedMessageIds").let { a -> (0 until a.length()).map { a.getString(it) } }
            assertTrue(imported.containsAll(listOf(early.id, waiting.id)))

            // Bank messages finished them: nothing of theirs waits, their words are gone, and the last text handled is the newer one.
            assertEquals(listOf(other), inbox.pending(identity))
            assertEquals(waiting.received, inbox.handledThrough(identity))
            val records = inbox.records(identity, listOf(early.id, waiting.id, other.id))
            assertTrue(records.getValue(early.id).done && records.getValue(waiting.id).done && !records.getValue(other.id).done)
            assertEquals(SmsOutcome.Added, records.getValue(early.id).outcome!!.getString("outcome"))
            inbox.captureAll(listOf(early, waiting))
            assertEquals(1, inbox.pendingCount(identity))
            // The open review keeps only what it read from the text still waiting, and an open page is told.
            val review = SmsBatch.restore(inbox.state(identity, "draft")) { ids -> inbox.waiting(identity, ids) }!!
            assertEquals(listOf(other), review.messages)
            assertEquals(told + 1, BankTextsAssistant.entered.value)

            // The same card again adds nothing: the budget has them.
            BankTextsAssistant.apply(session.context, vm, card)
            assertEquals(before + 2, storedBudget().rows("transactions").size)
            assertEquals(waiting.received, inbox.handledThrough(identity))
        } finally { inbox.clear(identity) }
    }
}
