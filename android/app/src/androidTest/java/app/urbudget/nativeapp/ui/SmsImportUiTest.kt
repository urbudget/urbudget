package app.urbudget.nativeapp.ui

import android.Manifest
import android.app.Application
import android.os.Looper
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.LaunchedEffect
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
        requireEmulator()
        Words.source = ResourceWords(context.resources)
    }
    @After fun clean() {
        identity?.let { SmsInbox.get(context).clear(it) }
        instrumentation.runOnMainSync { store.clear() }
    }

    /** A budget with the round-trip fixture's accounts, or [empty]: a new one with none. */
    private fun fixture(tag: String, empty: Boolean = false): BudgetViewModel {
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.READ_SMS)
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECEIVE_SMS)
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put(tag, vm) }
        val name = "SMS $tag ${UUID.randomUUID()}"
        instrumentation.runOnMainSync { vm.createBudget(name, "USD") }
        compose.waitUntil(30_000) { vm.state.value.budget?.optString("name") == name || vm.state.value.error != null }
        assertNull(vm.state.value.error)
        if (empty) {
            identity = vm.state.value.budgetIdentity!!
            SmsInbox.get(context).configure(identity!!, setOf("SyntheticBank"), 0)
            return vm
        }
        val fixture = sized(JSONObject(instrumentation.context.assets.open("native-roundtrip.json").bufferedReader().use { it.readText() })).put("name", "$name fixture")
        instrumentation.runOnMainSync { vm.importBudget(fixture.toString()) }
        compose.waitUntil(30_000) { vm.state.value.budget?.optString("name") == "$name fixture" }
        identity = vm.state.value.budgetIdentity!!
        SmsInbox.get(context).configure(identity!!, setOf("SyntheticBank"), 0)
        return vm
    }
    /**
     * With `-e large 1`, the same budget with a ledger of a real size behind it. The rows go after the
     * fixture's own, in years past and for amounts no text here mentions, so no scenario can match one.
     */
    private fun sized(budget: JSONObject): JSONObject {
        if (InstrumentationRegistry.getArguments().getString("large") != "1") return budget
        val category = budget.rows("categories").first { it.text("paymentAccountId").isBlank() }.text("id")
        val rows = budget.rows("transactions").toMutableList()
        val start = java.time.LocalDate.now().minusYears(3)
        repeat(3_000) { i ->
            val amount = -(700_000L + i)
            rows += JSONObject().put("id", "large-$i").put("date", start.minusDays(i % 700L).toString()).put("accountId", "checking").put("payee", "Payee ${i % 40}").put("memo", "")
                .put("amount", amount).put("status", "cleared").put("transferAccountId", JSONObject.NULL).put("transferStatus", JSONObject.NULL)
                .put("splits", JSONArray().put(JSONObject().put("categoryId", category).put("amount", amount)))
                .put("source", JSONObject().put("id", "imported-$i").put("memo", "Representative imported metadata").put("import_id", "YNAB:$amount:$i"))
        }
        return budget.put("transactions", JSONArray(rows))
    }
    private fun show(vm: BudgetViewModel, analyzer: SmsAnalyzer) = compose.setContent {
        val state by vm.state.collectAsState()
        MaterialTheme { state.budget?.let { SmsImportScreen(it, vm, analyzer = analyzer) } }
    }
    private fun waitFor(tag: String) = compose.waitUntil(10_000) { compose.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty() }
    /** Scrolls the review to a row it hasn't drawn yet, below the cards and accounts to set up. */
    private fun reach(tag: String) = compose.onNodeWithTag("sms-review-list").performScrollToNode(hasTestTag(tag))

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
        override suspend fun analyze(request: JSONObject, live: (SmsLive) -> Unit): JSONObject = reply(request)
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
        waitFor("sms-setup:1234")
        // Nothing to add until the card has its account: the page's button sets it up.
        compose.onNodeWithTag("sms-add").assertDoesNotExist()
        compose.onNodeWithTag("choice-open:${context.getString(R.string.sms_setup_action)}").performClick()
        compose.onNodeWithText(context.getString(R.string.sms_setup_part_of, "Everyday")).performClick()
        compose.onNodeWithTag("sms-setup-apply").assertTextEquals(context.getString(R.string.sms_setup_apply_none)).performClick()
        compose.waitUntil(10_000) { inbox.state(identity!!, "mappings") != null }
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-add").assertIsEnabled() }.isSuccess }
        compose.onNodeWithTag("sms-add").performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before + 2 }
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 }
        assertEquals(1, calls)
        assertEquals("checking", JSONArray(inbox.state(identity!!, "mappings")).getJSONObject(0).getString("accountId"))
    }

    @Test fun anEmptyBudgetIsSetUpFromItsTextsAndItsAccountEndsAtTheBanksLatestBalance() {
        val vm = fixture("onboarding", empty = true)
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        fun text(at: Long, body: String, quote: String, amount: String, ref: String, kind: String = "expense", balance: String? = null): Pair<BankSms, JSONObject> {
            val sms = BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body)
            return sms to syntheticSmsAnalysis(sms, accountId = null, ref = ref).getJSONArray("items").getJSONObject(0).apply {
                getJSONArray("evidence").getJSONObject(0).put("quote", quote)
                getJSONObject("transaction").put("amount", amount).put("writtenAmount", amount.removePrefix("-")).put("kind", kind).put("balance", balance ?: JSONObject.NULL)
            }
        }
        // A checking account, the debit card that draws on it, and a credit card.
        val texts = listOf(
            text(1, "Debit card ending 7788 was used for USD 45.00 at TARGET", "USD 45.00", "-45.00", "7788"),
            text(2, "USD 1,200.00 deposited to checking account ending 3301. Available balance USD 2,455.00", "USD 1,200.00", "1200.00", "3301", "income", "2455.00"),
            text(3, "Credit card ending 4512 was charged USD 25.62 at UBER", "USD 25.62", "-25.62", "4512"),
            text(4, "Debit card ending 7788 was used for USD 15.00 at STARBUCKS. Balance USD 2,140.00", "USD 15.00", "-15.00", "7788", balance = "2140.00"),
        )
        inbox.captureAll(texts.map { it.first })
        val analysis = JSONObject().put("items", JSONArray(texts.map { it.second }))
        inbox.saveState(identity!!, "draft", SmsBatch(texts.map { it.first }, analysis).saved())
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        show(vm, analyzer { analysis })
        waitFor("sms-review")
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-setup:3301")
        compose.onNodeWithText(context.getString(R.string.sms_setup_title)).assertExists()
        // The balance before the texts: the card's alert gave 2,140.00 after 45 + 15 spent and 1,200 in.
        compose.onNodeWithTag("sms-setup-opening:3301", useUnmergedTree = true).assertTextContains(money(100_000, "USD"), substring = true)
        // Nothing is ready before the accounts are there: setting them up is the page's button.
        compose.onNodeWithTag("sms-add").assertDoesNotExist()
        compose.onNodeWithTag("sms-setup-apply").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_setup_apply, 2, "2")).performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.rows("accounts").size == 2 }
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-add").assertIsEnabled() }.isSuccess }
        compose.onNodeWithTag("sms-add").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_add, 4, "4")).performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before + 4 }
        val accounts = vm.state.value.budget!!.rows("accounts")
        val checking = accounts.single { it.text("accountType") == "checking" }
        val card = accounts.single { it.text("accountType") == "creditCard" }
        assertEquals(100_000L, checking.getLong("openingBalance")); assertEquals(0L, card.getLong("openingBalance"))
        compose.waitUntil(10_000) { vm.state.value.summary!!.rows("accounts").any { it.text("id") == checking.text("id") && it.optLong("balance") == 214_000L } }
        assertEquals(-2_562L, vm.state.value.summary!!.rows("accounts").first { it.text("id") == card.text("id") }.optLong("balance"))
        // The debit card is remembered as part of the checking account.
        val mappings = JSONArray(inbox.state(identity!!, "mappings"))
        assertEquals(mapOf("3301" to checking.text("id"), "7788" to checking.text("id"), "4512" to card.text("id")),
            (0 until mappings.length()).map { mappings.getJSONObject(it) }.associate { it.getString("reference") to it.getString("accountId") })
        // Every text is settled.
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 }
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

    @Test fun aCardOrARowTheUserWontAddIsClearedAndTheCardIsRememberedSo() {
        val vm = fixture("skip")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*1234", "checking").toString())
        // Two purchases on a card the budget has no account for, one in euros waiting for its amount, and one ready.
        val texts = listOf("*9999", "*9999", "*1234", "*1234").mapIndexed { i, card ->
            val body = "Purchase ${if (i == 2) "EUR" else "USD"} 12.34 at Synthetic Shop card $card #$i"
            BankSms(smsId("SyntheticBank", i + 1L, body), "SyntheticBank", i + 1L, body) to card
        }
        inbox.captureAll(texts.map { it.first })
        val analysis = JSONObject().put("items", JSONArray(texts.mapIndexed { i, (sms, card) ->
            syntheticSmsAnalysis(sms, accountId = null, ref = card).getJSONArray("items").getJSONObject(0).apply {
                if (i == 2) {
                    getJSONObject("transaction").put("amount", JSONObject.NULL).put("currency", "EUR")
                    getJSONArray("evidence").getJSONObject(0).put("quote", "EUR 12.34")
                }
            }
        }))
        inbox.saveState(identity!!, "draft", SmsBatch(texts.map { it.first }, analysis).saved())
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        show(vm, analyzer { analysis })
        waitFor("sms-review")
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-setup:9999")
        compose.waitUntil(10_000) { runCatching { reach("sms-tick:3"); compose.onNodeWithTag("sms-tick:3").assertIsEnabled() }.isSuccess }
        // Unticked, a row says it won't be added.
        compose.onNodeWithTag("sms-wont-add:3", useUnmergedTree = true).assertDoesNotExist()
        compose.onNodeWithTag("sms-tick:3").performScrollTo().performClick()
        compose.onNodeWithTag("sms-wont-add:3", useUnmergedTree = true).assertExists()
        compose.onNodeWithTag("sms-tick:3").performClick()
        // The card's texts aren't added, and it's remembered so its next texts don't ask.
        compose.onNodeWithTag("choice-open:${context.getString(R.string.sms_setup_action)}").performScrollTo().performClick()
        compose.onNodeWithText(context.getString(R.string.sms_setup_skip)).performClick()
        compose.onNodeWithTag("sms-setup-apply").performScrollTo().performClick()
        fun mapping(card: String) = JSONArray(inbox.state(identity!!, "mappings")).let { a -> (0 until a.length()).map { a.getJSONObject(it) } }
            .firstOrNull { it.getString("reference") == card }?.getString("accountId")
        compose.waitUntil(10_000) { mapping("*9999") == SmsSkipCard }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-setup:9999").fetchSemanticsNodes().isEmpty() }
        // Nor the row waiting for its amount.
        compose.onNodeWithTag("sms-dont-add:2", useUnmergedTree = true).performScrollTo().performClick()
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-add").assertIsEnabled() }.isSuccess }
        compose.onNodeWithTag("sms-add").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_add, 1, "1"))
        compose.onNodeWithTag("sms-add").performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before + 1 }
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 }
        assertEquals("checking", mapping("*1234"))
    }

    @Test fun whileTheAssistantAnswersTheCardShowsWhatItsDoingItsLatestThoughtAndTheTextsItsAnswerCovers() {
        val vm = fixture("live")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.captureAll((1L..40L).map { at -> val body = "Your code is ${1000 + at}"; BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body) })
        val thought = kotlinx.coroutines.CompletableDeferred<Unit>()
        val wrote = kotlinx.coroutines.CompletableDeferred<Unit>()
        val formats = kotlinx.coroutines.CompletableDeferred<Unit>()
        show(vm, object : SmsAnalyzer {
            override suspend fun status() = connection
            override suspend fun analyze(request: JSONObject, live: (SmsLive) -> Unit): JSONObject {
                live(SmsLive("thinking", "Reading the codes", 0, 40, 0))
                thought.await()
                live(SmsLive("writing", "Reading the codes", 25, 40, 0, "skipped"))
                wrote.await()
                live(SmsLive("writing", "Reading the codes", 40, 40, 0, "templates"))
                formats.await()
                return codes(request.getJSONArray("messages"))
            }
        })
        waitFor("sms-thought")
        compose.onNodeWithTag("sms-live").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_live_thinking, 40, "40"))
        compose.onNodeWithTag("sms-thought").assertTextEquals("Reading the codes")
        // One request: no count of requests, and stopping cancels it.
        compose.onNodeWithTag("sms-progress").assertDoesNotExist()
        compose.onNodeWithTag("sms-stop").assertTextEquals(context.getString(R.string.common_cancel))
        thought.complete(Unit)
        val writing = context.resources.getQuantityString(R.plurals.sms_live_writing, 40, "40", "25")
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-live").assertTextEquals(writing) }.isSuccess }
        wrote.complete(Unit)
        // Once the texts are written up, the formats it writes for next time say so.
        val writingFormats = context.getString(R.string.sms_live_formats)
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-live").assertTextEquals(writingFormats) }.isSuccess }
        formats.complete(Unit)
        compose.waitUntil(15_000) { inbox.pendingCount(identity!!) == 0 }
        waitFor("sms-up-to-date")
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
        waitFor("sms-setup:9999")
        compose.waitUntil(10_000) { runCatching { reach("sms-tick:0"); compose.onNodeWithTag("sms-tick:0").assertIsEnabled() }.isSuccess }
        // Each untick stays: a row drawn before the last tick doesn't bring it back.
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").performScrollTo().performClick() }
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").assertIsOff() }
        compose.onNodeWithText(context.resources.getQuantityString(R.plurals.sms_clear, 3, "3")).assertExists()
        compose.onNodeWithTag("sms-tick-all:ready").performClick()
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").assertIsOn() }
        compose.onNodeWithTag("sms-tick-all:ready").performClick()
        (0..2).forEach { compose.onNodeWithTag("sms-tick:$it").assertIsOff() }
        compose.onNodeWithTag("sms-balance:checking").assertDoesNotExist()
        compose.onNodeWithTag("sms-tick:1").performScrollTo().performClick()
        compose.onNodeWithTag("sms-tick:2").performScrollTo().performClick()
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

    @Test fun aTransfersOtherAccountIsMadeFromTheReviewAndRowsGoUnderTheirDay() {
        val vm = fixture("made")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*1234", "checking").toString())
        val body = "Transfer USD 12.34 from card *1234 to your deposit 4321"
        val sms = BankSms(smsId("SyntheticBank", 1, body), "SyntheticBank", 1, body)
        inbox.capture(sms)
        // A placement into a deposit the budget doesn't have yet, read on an earlier visit.
        val analysis = JSONObject().put("items", JSONArray().put(syntheticSmsAnalysis(sms, accountId = null, ref = "*1234").getJSONArray("items").getJSONObject(0).also {
            it.getJSONObject("transaction").put("kind", "transfer").put("otherRef", "4321").put("payee", "Deposit")
        }))
        inbox.saveState(identity!!, "draft", SmsBatch(listOf(sms), analysis).saved())
        var calls = 0
        show(vm, analyzer { calls++; analysis })
        waitFor("sms-review")
        compose.onNodeWithTag("sms-review").performClick()
        // The deposit the transfer names is set up as an account, named for the sender and its digits.
        waitFor("sms-setup:4321")
        compose.onNodeWithTag("sms-setup-apply").assertTextEquals(context.resources.getQuantityString(R.plurals.sms_setup_apply, 1, "1")).performClick()
        compose.waitUntil(10_000) { vm.state.value.budget!!.rows("accounts").any { it.text("name") == "SYNTHETICBANK 4321" } }
        // Made for this review, its balance is from before the transfer: ready to add, not held back.
        waitFor("sms-tick:0")
        compose.onNodeWithTag("sms-tick:0").assertIsOn()
        compose.onNodeWithTag("sms-day:2026-10-24").assertExists()
        // Its digits are remembered for the sender's next texts.
        val made = vm.state.value.budget!!.rows("accounts").first { it.text("name") == "SYNTHETICBANK 4321" }.text("id")
        compose.waitUntil(10_000) { JSONArray(inbox.state(identity!!, "mappings") ?: "[]").let { m -> (0 until m.length()).any { m.getJSONObject(it).getString("accountId") == made } } }
        assertEquals(0, calls)
    }

    @Test fun aCardPaymentsOtherAccountIsChosenOnceAndRememberedForTheCard() {
        val vm = fixture("payment")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*5555", "card").toString())
        val body = "Card Payment card *5555 USD 12.34"
        val sms = BankSms(smsId("SyntheticBank", 1, body), "SyntheticBank", 1, body)
        inbox.capture(sms)
        // A payment into the card naming no other account, read on an earlier visit.
        val analysis = JSONObject().put("items", JSONArray().put(syntheticSmsAnalysis(sms, accountId = null, ref = "*5555").getJSONArray("items").getJSONObject(0).also {
            it.put("note", "Card payment").getJSONObject("transaction").put("kind", "transfer").put("amount", "12.34").put("payee", "Card payment")
        }))
        inbox.saveState(identity!!, "draft", SmsBatch(listOf(sms), analysis).saved())
        show(vm, analyzer { analysis })
        waitFor("sms-review")
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-transfer:0")
        compose.onNodeWithTag("choice-open:${context.getString(R.string.sms_which_other)}").performScrollTo().performClick()
        compose.onNodeWithText("Everyday").performClick()
        // Ready to add, from the account it came from into the card; and remembered for the card's next payments.
        waitFor("sms-tick:0")
        compose.onNodeWithTag("sms-tick:0").assertIsOn()
        compose.onNodeWithTag("sms-accounts:0", useUnmergedTree = true).assertTextContains("Everyday → Credit card", substring = true)
        compose.waitUntil(10_000) {
            JSONArray(inbox.state(identity!!, "mappings") ?: "[]").let { m -> (0 until m.length()).map { m.getJSONObject(it) }
                .any { it.optString("for") == SmsForTransfer && it.getString("reference") == "*5555" && it.getString("accountId") == "checking" } }
        }
    }

    @Test fun whatAReadingLearnsIsKeptApartFromTheReviewAndEveryRequestCarriesTheTemplates() {
        val vm = fixture("learn")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        val body = "Your code is 1234"
        inbox.capture(BankSms(smsId("SyntheticBank", 1, body), "SyntheticBank", 1, body))
        val template = JSONObject().put("id", "t1").put("sender", "syntheticbank").put("lines", JSONArray().put("Your code is {code}"))
            .put("outcome", JSONObject().put("disposition", JSONObject.NULL).put("kind", JSONObject.NULL).put("sign", JSONObject.NULL).put("skip", "code").put("memo", JSONObject.NULL))
            .put("note", "").put("payee", JSONObject.NULL)
        val requests = java.util.Collections.synchronizedList(mutableListOf<JSONObject>())
        show(vm, analyzer { request ->
            requests += request
            codes(request.getJSONArray("messages")).put("learning", JSONObject().put("reviewed", 0).put("read", 1)
                .put("templates", JSONArray().put(JSONObject().put("template", template).put("example", body).put("agreed", 1))))
        })
        compose.waitUntil(15_000) { inbox.templates(identity!!).length() == 1 }
        waitFor("sms-up-to-date")
        assertEquals(SmsTemplateSummary(1, 0, 0), inbox.templateSummary(identity!!))
        // The request carried the budget's templates and payees (none yet), and the review never held the learning.
        assertEquals(0, requests.single().getJSONArray("templates").length()); assertEquals(0, requests.single().getJSONArray("payees").length())
        assertNull(inbox.state(identity!!, "draft"))
    }

    @Test fun aRequestIsSizedByWhatTheAssistantReadsAndALearnedFormatIsListedAndForgotten() {
        val vm = fixture("formats")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.captureAll((1L..600L).map { at -> val body = "Your code is ${1000 + at}"; BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body) })
        val template = JSONObject().put("id", "t1").put("sender", "syntheticbank").put("lines", JSONArray().put("Your code is {code}"))
            .put("outcome", JSONObject().put("skip", "code")).put("note", "").put("payee", JSONObject.NULL)
        val sizes = java.util.Collections.synchronizedList(mutableListOf<Int>())
        show(vm, object : SmsAnalyzer {
            override suspend fun status() = connection
            // Most are junk the phone settles, sent along with the texts read in full: in a budget's
            // first request, before it knows any format, 60 of the hundred.
            override suspend fun lanes(request: JSONObject) = List(request.getJSONArray("messages").length()) { if (it < 500) "junk" else "read" }
            override suspend fun analyze(request: JSONObject, live: (SmsLive) -> Unit): JSONObject {
                sizes += request.getJSONArray("messages").length()
                assertTrue(request.has("trusted"))
                return codes(request.getJSONArray("messages")).put("learning", JSONObject().put("read", 100).put("junk", 500)
                    .put("templates", JSONArray().put(JSONObject().put("template", template).put("example", "Your code is 1001").put("agreed", 1))))
            }
        })
        compose.waitUntil(15_000) { inbox.pendingCount(identity!!) == 0 }
        waitFor("sms-up-to-date")
        assertEquals(listOf(500 + SmsFirstRead, 100 - SmsFirstRead), sizes.toList())
        // The format is listed with a text it read, and forgetting it has its texts read in full again.
        compose.onNodeWithTag("sms-inbox").performScrollToNode(hasTestTag("sms-formats"))
        compose.onNodeWithTag("sms-formats").performClick()
        waitFor("sms-format:t1")
        compose.onNodeWithText("Your code is 1001").assertExists()
        compose.onNodeWithTag("sms-format-forget:t1").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-format:t1").fetchSemanticsNodes().isEmpty() }
        assertEquals(0, inbox.templates(identity!!).length())
    }

    @Test fun aTapThatClearsManyTextsForGoodAsksFirst() {
        val vm = fixture("clear")
        val inbox = SmsInbox.get(context)
        inbox.saveState(identity!!, "consent", smsConsent(connection))
        inbox.saveState(identity!!, "mappings", smsRememberCard(JSONArray(), "SyntheticBank", "*1234", "checking").toString())
        val body = "Purchase USD 12.34 at Synthetic Shop card *1234 on 2026-10-24"
        val sms = BankSms(smsId("SyntheticBank", 1000, body), "SyntheticBank", 1000, body)
        val codes = (1L..60L).map { at -> val text = "Your code is ${1000 + at}"; BankSms(smsId("SyntheticBank", at, text), "SyntheticBank", at, text) }
        inbox.captureAll(codes + sms)
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        show(vm, analyzer { request ->
            val sent = request.getJSONArray("messages").let { a -> (0 until a.length()).map { a.getJSONObject(it) } }
            codes(JSONArray(sent.filter { it.getString("id") != sms.id })).also { reply ->
                if (sent.any { it.getString("id") == sms.id }) reply.getJSONArray("items").put(syntheticSmsAnalysis(sms, accountId = null, ref = "*1234").getJSONArray("items").get(0))
            }
        })
        waitFor("sms-review")
        compose.onNodeWithTag("sms-review").performClick()
        waitFor("sms-add")
        compose.waitUntil(10_000) { runCatching { compose.onNodeWithTag("sms-add").assertIsEnabled() }.isSuccess }
        // Adding the one purchase would clear sixty codes for good: it asks, and Cancel changes nothing.
        compose.onNodeWithTag("sms-add").performClick()
        waitFor("sms-clear-detail")
        compose.onNodeWithText(context.getString(R.string.common_cancel)).performClick()
        compose.runOnIdle { assertEquals(61, inbox.pendingCount(identity!!)) }
        assertEquals(before, vm.state.value.budget!!.getJSONArray("transactions").length())
        compose.onNodeWithTag("sms-add").performClick()
        waitFor("sms-clear-confirm")
        compose.onNodeWithTag("sms-clear-confirm").performClick()
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 }
        compose.waitUntil(10_000) { vm.state.value.budget!!.getJSONArray("transactions").length() == before + 1 }
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

    // Compose's test clock runs a composition's coroutine on the thread that resumes it, and frames with it. A preview
    // coming back on a worker drew beside the main thread: aTapThatClearsManyTextsForGoodAsksFirst failed one run in twenty.
    @Test fun aPreviewComesBackOnTheMainThread() {
        val vm = fixture("thread")
        val body = "Purchase USD 12.34 at Synthetic Shop card *1234 on 2026-10-24"
        val sms = BankSms(smsId("SyntheticBank", 1000, body), "SyntheticBank", 1000, body)
        val batch = SmsBatch(listOf(sms), syntheticSmsAnalysis(sms))
        var thread: Thread? = null
        compose.setContent { LaunchedEffect(Unit) { vm.smsPreview(vm.state.value.budget!!, batch, batch.analysis); thread = Thread.currentThread() } }
        compose.waitUntil(10_000) { thread != null }
        assertSame(Looper.getMainLooper().thread, thread)
    }
}
