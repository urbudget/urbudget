package app.urbudget.nativeapp.ui

import android.Manifest
import android.app.Application
import android.os.Build
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.*
import app.urbudget.nativeapp.R
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

/** Synthetic emulator-only checks of preparing, the one-time consent, and the review's commit boundary. */
@RunWith(AndroidJUnit4::class)
class SmsImportUiTest {
    @get:Rule val compose = createComposeRule()
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val store = ViewModelStore()
    private var identity: String? = null
    private val connection = JSONObject().put("ready", true).put("provider", "anthropic").put("model", "synthetic-test")

    @Before fun setup() {
        assumeTrue("Use an isolated emulator", Build.HARDWARE.contains("ranchu") || Build.HARDWARE.contains("goldfish"))
        Words.source = ResourceWords(context.resources)
    }
    @After fun clean() {
        identity?.let { SmsInbox.get(context).clear(it) }
        instrumentation.runOnMainSync { store.clear() }
    }

    private fun fixture(tag: String): BudgetViewModel {
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.READ_SMS)
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECEIVE_SMS)
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put(tag, vm) }
        val name = "SMS $tag ${UUID.randomUUID()}"
        instrumentation.runOnMainSync { vm.createBudget(name, "USD") }
        compose.waitUntil(30_000) { vm.state.value.budget?.optString("name") == name || vm.state.value.error != null }
        assertNull(vm.state.value.error)
        val fixture = JSONObject(instrumentation.context.assets.open("native-roundtrip.json").bufferedReader().use { it.readText() }).put("name", "$name fixture")
        instrumentation.runOnMainSync { vm.importBudget(fixture.toString()) }
        compose.waitUntil(15_000) { vm.state.value.budget?.optString("name") == "$name fixture" }
        identity = vm.state.value.budgetIdentity!!
        SmsInbox.get(context).configure(identity!!, setOf("SyntheticBank"), 0)
        return vm
    }
    private fun show(vm: BudgetViewModel, analyzer: SmsAnalyzer) = compose.setContent {
        val state by vm.state.collectAsState()
        MaterialTheme { state.budget?.let { SmsImportScreen(it, vm, analyzer = analyzer) } }
    }
    private fun waitFor(tag: String) = compose.waitUntil(10_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }

    @Test fun banksComeFirstAndAPersonsNumberStaysHiddenUntilAskedFor() {
        var saved: Set<String>? = null
        compose.setContent {
            MaterialTheme {
                SmsSenderPicker(emptyList(), {}, save = { senders, _ -> saved = senders }, load = {
                    listOf(SmsSender("Example Bank A", 12, "Purchase USD 4.00"), SmsSender("Example Bank B", 3, null), SmsSender("+15550001999", 40, null))
                })
            }
        }
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Example Bank A").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("+15550001999").assertDoesNotExist()
        compose.onNodeWithText("Example Bank A").performClick()
        compose.onNodeWithText("Example Bank B").performClick()
        compose.onNodeWithText(context.resources.getQuantityString(R.plurals.sms_show_numbers, 1, "1")).performClick()
        compose.onNodeWithText("+15550001999").assertExists()
        compose.onNodeWithText(context.getString(R.string.common_save)).performClick()
        compose.runOnIdle { assertEquals(setOf("Example Bank A", "Example Bank B"), saved) }
    }

    private fun analyzer(reply: suspend (JSONObject) -> JSONObject) = object : SmsAnalyzer {
        override suspend fun status() = connection
        override suspend fun analyze(request: JSONObject): JSONObject = reply(request)
    }
    /** What the model returns for texts that are only codes. */
    private fun codes(sent: JSONArray) = JSONObject().put("items", JSONArray((0 until sent.length()).map { i ->
        val id = sent.getJSONObject(i).getString("id")
        JSONObject().put("sourceIds", JSONArray().put(id)).put("disposition", "ignore").put("skip", "code").put("note", "")
            .put("evidence", JSONArray()).put("transaction", JSONObject.NULL).put("unsure", JSONArray())
    }))
    private fun draftSize(inbox: SmsInbox) = runCatching { JSONObject(inbox.state(identity!!, "draft")!!).getJSONArray("ids").length() }.getOrDefault(0)

    @Test fun sendingIsAskedOnceAndOnlyTheReviewImports() {
        val vm = fixture("consent")
        val inbox = SmsInbox.get(context)
        val body = "Purchase USD 12.34 at Synthetic Shop card *1234 on 2026-10-24"
        val sms = BankSms(smsId("SyntheticBank", 1000, body), "SyntheticBank", 1000, body)
        inbox.capture(sms)
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*1234", "checking").toString())
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        var analyses = 0
        show(vm, analyzer { request ->
            analyses++
            assertEquals(sms.id, request.getJSONArray("messages").getJSONObject(0).getString("id"))
            assertFalse(request.has("mappings"))
            syntheticSmsAnalysis(sms, accountId = null, ref = "*1234")
        })
        // Without permission nothing is read by itself: the page asks.
        waitFor("sms-prepare")
        compose.onNodeWithTag("sms-prepare").performClick()
        waitFor("sms-consent")
        compose.onNodeWithText(context.getString(R.string.common_cancel)).performClick()
        assertEquals(0, analyses)
        compose.onNodeWithTag("sms-prepare").performClick()
        waitFor("sms-consent")
        compose.onNodeWithText(context.getString(R.string.sms_send_confirm)).performClick()
        waitFor("sms-review")
        assertEquals(1, analyses)
        assertEquals(before, vm.state.value.budget!!.getJSONArray("transactions").length())
        assertTrue(smsConsented(inbox.state(identity!!, "consent"), connection))
        compose.onNodeWithTag("sms-needs").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_to_review, 1, "1"))
        // Later keeps the review and the message.
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-review-page")
        compose.onNodeWithText(context.getString(R.string.sms_later_action)).performClick()
        waitFor("sms-review")
        compose.runOnIdle { assertEquals(1, inbox.pendingCount(identity!!)) }
        assertNotNull(inbox.state(identity!!, "draft"))
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-add")
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-add").assertIsEnabled() }.isSuccess }
        compose.onNodeWithTag("sms-add").performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before + 1 }
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 }
        val added = vm.state.value.budget!!.getJSONArray("transactions").getJSONObject(before)
        assertEquals("uncleared", added.getString("status"))
        assertEquals(-1234L, added.getLong("amount"))
        assertEquals("checking", added.getString("accountId"))
        assertNull(inbox.state(identity!!, "draft"))
        instrumentation.runOnMainSync { vm.undo() }
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before }
        assertFalse(vm.state.value.budget!!.has("smsImportedMessageIds"))
    }

    @Test fun oneCardChoiceAnswersEveryMessageWithoutAskingTheAssistantAgain() {
        val vm = fixture("card")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        val messages = (1L..2L).map { at -> val body = "Purchase USD 12.34 at Synthetic Shop card *1234 on 2026-10-24"; BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body) }
        messages.forEach(inbox::capture)
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        var calls = 0
        show(vm, analyzer {
            calls++
            JSONObject().put("items", JSONArray(messages.map { syntheticSmsAnalysis(it, accountId = null, ref = "*1234").getJSONArray("items").getJSONObject(0) }))
        })
        // With permission, opening the page reads what's waiting.
        waitFor("sms-review")
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-card:*1234")
        compose.onNodeWithTag("sms-add").assertIsNotEnabled()
        compose.onNodeWithTag("choice-open:${context.getString(R.string.sms_which_account)}").performClick()
        compose.onNodeWithText("Everyday").performClick()
        compose.waitUntil(10_000) { inbox.state(identity!!, "mappings") != null }
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-add").assertIsEnabled() }.isSuccess }
        compose.onNodeWithTag("sms-add").performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before + 2 }
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 }
        assertEquals(1, calls)
        assertEquals("checking", JSONArray(inbox.state(identity!!, "mappings")).getJSONObject(0).getString("accountId"))
    }

    @Test fun hundredsOfCodesAreReadTwoHundredAtATimeAndHandledWithoutAQuestion() {
        val vm = fixture("codes")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.captureAll((1L..450L).map { at -> val body = "Your code is ${1000 + at}"; BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body) })
        val sizes = java.util.Collections.synchronizedList(mutableListOf<Int>())
        show(vm, analyzer { request ->
            val sent = request.getJSONArray("messages")
            sizes += sent.length()
            // The third request is still being read when the user stops.
            if (sizes.size == 3) kotlinx.coroutines.awaitCancellation()
            codes(sent)
        })
        compose.waitUntil(15_000) { sizes.size == 3 }
        compose.onNodeWithTag("sms-progress").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_reading_progress, 450, "450", "400"))
        compose.onNodeWithTag("sms-stop").performClick()
        // What was read is only codes: handled without a question, the rest wait.
        // The summary is saved just after the texts are settled.
        compose.waitUntil(15_000) { inbox.pendingCount(identity!!) == 50 && inbox.state(identity!!, "last") != null }
        assertEquals(listOf(200, 200, 50), sizes.toList())
        assertNull(inbox.state(identity!!, "draft"))
        assertEquals(400, JSONObject(inbox.state(identity!!, "last")!!).getJSONObject("skipped").getInt("code"))
        // Stopped stays stopped; reading the rest is a tap.
        waitFor("sms-prepare")
        assertEquals(3, sizes.size)
        compose.onNodeWithTag("sms-prepare").performClick()
        compose.waitUntil(15_000) { inbox.pendingCount(identity!!) == 0 }
        waitFor("sms-up-to-date")
        assertEquals(listOf(200, 200, 50, 50), sizes.toList())
    }

    @Test fun untickingRowsOneAfterAnotherKeepsEachAndWhatStillNeedsAChoiceStaysRead() {
        val vm = fixture("untick")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*1234", "checking").toString())
        // Three purchases on a remembered card, and one on a card the phone can't match.
        val texts = (1L..4L).map { at ->
            val card = if (at == 4L) "*9999" else "*1234"
            val body = "Purchase USD 12.34 at Synthetic Shop card $card #$at"
            BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body) to card
        }
        inbox.captureAll(texts.map { it.first })
        // Read on an earlier visit: the page opens on the review, with nothing of its own to read.
        val analysis = JSONObject().put("items", JSONArray(texts.map { (sms, card) -> syntheticSmsAnalysis(sms, accountId = null, ref = card).getJSONArray("items").getJSONObject(0) }))
        inbox.saveState(identity!!, "draft", SmsBatch(texts.map { it.first }, analysis).saved())
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        var calls = 0
        show(vm, analyzer { calls++; analysis })
        waitFor("sms-review")
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-card:*9999")
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-tick:0").assertIsEnabled() }.isSuccess }
        // Each untick stays: a row drawn before the last tick doesn't bring it back.
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").performClick() }
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").assertIsOff() }
        compose.onNodeWithText(context.resources.getQuantityString(R.plurals.sms_clear, 3, "3")).assertExists()
        compose.onNodeWithTag("sms-tick-all:ready").performClick()
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").assertIsOn() }
        compose.onNodeWithTag("sms-tick-all:ready").performClick()
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").assertIsOff() }
        compose.onNodeWithTag("sms-balance:checking").assertDoesNotExist()
        compose.onNodeWithTag("sms-tick:1").performClick()
        compose.onNodeWithTag("sms-tick:2").performClick()
        // Two purchases of 12.34 move the account's balance by 24.68, shown before adding.
        val balance = vm.state.value.summary!!.rows("accounts").first { it.text("id") == "checking" }.optLong("balance")
        val currency = vm.state.value.budget!!.getString("currency")
        compose.onNodeWithTag("sms-balance:checking").assertExists()
        compose.onNodeWithText(context.getString(R.string.sms_balance_change, money(balance, currency), money(balance - 2_468, currency))).assertExists()
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-add").assertIsEnabled() }.isSuccess }
        compose.onNodeWithTag("sms-add").performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before + 2 }
        // The unmatched card waits in the review as it was read; the unticked text is cleared, and
        // nothing just settled is sent to the assistant while the page catches up.
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 1 && draftSize(inbox) == 1 }
        waitFor("sms-needs")
        compose.onNodeWithTag("sms-needs").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_to_review, 1, "1"))
        compose.waitForIdle()
        assertEquals(0, calls)
    }

    @Test fun aLaterStartTakesEarlierTextsOutOfTheReviewAndDiscardingAsksAndReadsNothingAgain() {
        val vm = fixture("start")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*1234", "checking").toString())
        // One text from long ago, one received after the start this test moves to.
        val texts = listOf(1_000L, System.currentTimeMillis() + 86_400_000L).mapIndexed { i, at ->
            val body = "Purchase USD 12.34 at Synthetic Shop card *1234 #$i"
            BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body)
        }
        inbox.captureAll(texts)
        val analysis = JSONObject().put("items", JSONArray(texts.map { syntheticSmsAnalysis(it, accountId = null, ref = "*1234").getJSONArray("items").getJSONObject(0) }))
        inbox.saveState(identity!!, "draft", SmsBatch(texts, analysis).saved())
        var calls = 0
        show(vm, analyzer { calls++; analysis })
        waitFor("sms-needs")
        compose.onNodeWithTag("sms-needs").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_to_review, 2, "2"))
        // New messages only, from now: the old text leaves the review and the inbox; the other stays read.
        compose.onNodeWithText(context.getString(R.string.sms_collect)).performClick()
        compose.onNodeWithTag("choice-open:${context.getString(R.string.sms_history)}").performClick()
        compose.onNodeWithText(context.getString(R.string.sms_new_only)).performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText(context.resources.getQuantityString(R.plurals.sms_size_takes_out, 1, "1")).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText(context.getString(R.string.common_save)).performClick()
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 1 && draftSize(inbox) == 1 }
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-needs").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_to_review, 1, "1")) }.isSuccess }
        // Discarding asks, then clears the review's text rather than leaving it to be read again.
        compose.onNodeWithTag("sms-discard").performClick()
        compose.onNodeWithTag("sms-discard-confirm").performClick()
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 && inbox.state(identity!!, "draft") == null }
        waitFor("sms-up-to-date")
        compose.waitForIdle()
        assertEquals(0, calls)
    }

    @Test fun aTextForAPurchaseAlreadyInTheBudgetIsMatchedWithoutAQuestion() {
        val vm = fixture("known")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*1234", "checking").toString())
        val body = "Card *1234: USD 40.00 at EXAMPLE MKT on 2026-10-02"
        val purchase = BankSms(smsId("SyntheticBank", 1, body), "SyntheticBank", 1, body)
        val code = BankSms(smsId("SyntheticBank", 2, "Your code is 4821"), "SyntheticBank", 2, "Your code is 4821")
        listOf(purchase, code).forEach(inbox::capture)
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        show(vm, analyzer { request ->
            val read = codes(request.getJSONArray("messages"))
            read.getJSONArray("items").put(0, JSONObject().put("sourceIds", JSONArray().put(purchase.id)).put("disposition", "transaction").put("skip", JSONObject.NULL)
                .put("note", "Purchase at Example market").put("evidence", JSONArray().put(JSONObject().put("sourceId", purchase.id).put("quote", "USD 40.00"))).put("unsure", JSONArray())
                .put("transaction", JSONObject().put("date", "2026-10-02").put("amount", "-40.00").put("currency", "USD").put("writtenAmount", "40.00")
                    .put("payee", "Example market").put("memo", "").put("kind", "expense").put("accountRef", "*1234").put("otherRef", JSONObject.NULL).put("categoryId", JSONObject.NULL)))
            read
        })
        compose.waitUntil(15_000) { inbox.pendingCount(identity!!) == 0 }
        waitFor("sms-up-to-date")
        assertEquals(before, vm.state.value.budget!!.getJSONArray("transactions").length())
        val spend = (0 until before).map { vm.state.value.budget!!.getJSONArray("transactions").getJSONObject(it) }.first { it.getString("id") == "cash-spend" }
        val ids = spend.getJSONArray("bankImportIds").let { a -> (0 until a.length()).map { a.getString(it) }.toSet() }
        assertTrue(ids.containsAll(smsBankIds("checking", listOf(purchase.id))))
        val last = JSONObject(inbox.state(identity!!, "last")!!)
        assertEquals(1, last.getInt("known")); assertEquals(1, last.getJSONObject("skipped").getInt("code"))
    }
}
