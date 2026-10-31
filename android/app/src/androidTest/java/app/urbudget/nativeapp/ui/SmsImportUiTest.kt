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
import org.json.JSONObject
import org.json.JSONArray
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

/** Synthetic emulator-only checks of the actual selection and commit boundary. */
@RunWith(AndroidJUnit4::class)
class SmsImportUiTest {
    @get:Rule val compose = createComposeRule()
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val store = ViewModelStore()
    private var identity: String? = null

    @Before fun setup() {
        assumeTrue("Use an isolated emulator", Build.HARDWARE.contains("ranchu") || Build.HARDWARE.contains("goldfish"))
        Words.source = ResourceWords(context.resources)
    }
    @After fun clean() {
        identity?.let { SmsInbox.get(context).clear(it) }
        instrumentation.runOnMainSync { store.clear() }
    }

    @Test fun multipleSendersCanBeSelectedAndSearchKeepsEarlierChoices() {
        var saved: Set<String>? = null
        compose.setContent {
            MaterialTheme {
                SmsSenderPicker(emptyList(), {}, save = { senders, _ -> saved = senders }, load = { listOf("Example Bank A", "Example Bank B", "Personal") })
            }
        }
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Example Bank A").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Example Bank A").performClick()
        compose.onNodeWithText("Example Bank B").performClick()
        compose.onNodeWithText(context.getString(R.string.common_search)).performTextInput("Personal")
        compose.onNodeWithText(context.getString(R.string.common_save)).performClick()
        compose.runOnIdle { assertEquals(setOf("Example Bank A", "Example Bank B"), saved) }
    }

    @Test fun cancellingReviewKeepsInboxAndBudgetAndOnlyConfirmImports() {
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.READ_SMS)
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECEIVE_SMS)
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put("sms", vm) }
        val name = "SMS test ${UUID.randomUUID()}"
        instrumentation.runOnMainSync { vm.createBudget(name, "USD") }
        compose.waitUntil(15_000) { vm.state.value.budget?.optString("name") == name }
        val fixture = JSONObject(instrumentation.context.assets.open("native-roundtrip.json").bufferedReader().use { it.readText() }).put("name", "$name fixture")
        instrumentation.runOnMainSync { vm.importBudget(fixture.toString()) }
        compose.waitUntil(15_000) { vm.state.value.budget?.optString("name") == "$name fixture" }
        identity = vm.state.value.budgetIdentity!!
        val inbox = SmsInbox.get(context)
        inbox.configure(identity!!, setOf("SyntheticBank"), 0)
        val body = "Purchase USD 12.34 at Synthetic Shop on 2026-10-24"
        val sms = BankSms(smsId("SyntheticBank", 1000, body), "SyntheticBank", 1000, body)
        inbox.capture(sms)
        val before = vm.state.value.budget!!.getJSONArray("transactions").length()
        var analyses = 0
        val analyzer = object : SmsAnalyzer {
            override suspend fun status() = JSONObject().put("ready", true).put("provider", "openai").put("model", "synthetic-test")
            override suspend fun analyze(request: JSONObject): JSONObject {
                analyses++
                assertEquals(1, request.getJSONArray("messages").length())
                assertEquals(sms.id, request.getJSONArray("messages").getJSONObject(0).getString("id"))
                return syntheticSmsAnalysis(sms)
            }
        }
        compose.setContent {
            val state by vm.state.collectAsState()
            MaterialTheme { state.budget?.let { SmsImportScreen(it, vm, analyzer) } }
        }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-analyze").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("sms-analyze").assertIsNotEnabled()
        compose.onNodeWithTag("sms-inbox").performScrollToNode(hasTestTag("sms-message:${sms.id}"))
        compose.onNodeWithTag("sms-select:${sms.id}").assertIsOff().performClick()
        compose.onNodeWithTag("sms-inbox").performScrollToNode(hasTestTag("sms-analyze"))
        compose.onNodeWithTag("sms-analyze").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-consent").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, analyses)
        compose.onNodeWithText(context.getString(R.string.common_cancel)).performClick()
        assertEquals(0, analyses)
        compose.onNodeWithTag("sms-analyze").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-consent").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("sms-consent").performClick()
        val importLabel = context.getString(R.string.sms_confirm_review)
        compose.waitUntil(10_000) { compose.onAllNodesWithText(importLabel).fetchSemanticsNodes().isNotEmpty() }
        assertEquals(before, vm.state.value.budget!!.getJSONArray("transactions").length())
        compose.onNodeWithText(context.getString(R.string.common_cancel)).performClick()
        compose.runOnIdle { assertEquals(1, inbox.pendingCount(identity!!)) }
        assertNotNull(inbox.state(identity!!, "draft"))
        assertEquals(1, analyses)
        compose.onNodeWithTag("sms-review").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText(importLabel).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText(importLabel).performClick()
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

    @Test fun oneGroupedAccountAnswerIsRememberedOnlyExplicitlyAndRefinesBothMessages() {
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.READ_SMS)
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECEIVE_SMS)
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put("sms-group", vm) }
        val name = "SMS grouped ${UUID.randomUUID()}"
        instrumentation.runOnMainSync { vm.createBudget(name, "USD") }
        compose.waitUntil(15_000) { vm.state.value.budget?.optString("name") == name }
        val fixture = JSONObject(instrumentation.context.assets.open("native-roundtrip.json").bufferedReader().use { it.readText() }).put("name", "$name fixture")
        instrumentation.runOnMainSync { vm.importBudget(fixture.toString()) }
        compose.waitUntil(15_000) { vm.state.value.budget?.optString("name") == "$name fixture" }
        identity = vm.state.value.budgetIdentity!!
        val inbox = SmsInbox.get(context); inbox.configure(identity!!, setOf("SyntheticBank"), 0)
        val messages = (1L..2L).map { at -> val body = "Purchase USD 12.34 at Synthetic Shop card *1234 on 2026-10-24"; BankSms(smsId("SyntheticBank", at, body), "SyntheticBank", at, body) }
        messages.forEach(inbox::capture)
        var calls = 0
        val analyzer = object : SmsAnalyzer {
            override suspend fun status() = JSONObject().put("ready", true).put("provider", "anthropic").put("model", "synthetic-test")
            override suspend fun analyze(request: JSONObject): JSONObject {
                calls++
                assertEquals(2, request.getJSONArray("messages").length())
                if (calls == 1) return JSONObject().put("items", JSONArray().put(JSONObject()
                    .put("sourceIds", JSONArray(messages.map { it.id })).put("disposition", "question").put("reason", "Both messages use the same card")
                    .put("evidence", JSONArray().put(JSONObject().put("sourceId", messages[0].id).put("quote", "*1234")))
                    .put("transaction", JSONObject.NULL).put("question", "Which account is card *1234?").put("accountReference", "*1234")))
                assertTrue(request.getString("instruction").contains("checking"))
                assertEquals("checking", request.getJSONArray("mappings").getJSONObject(0).getString("accountId"))
                assertFalse(request.isNull("previous"))
                return JSONObject().put("items", JSONArray(messages.map { syntheticSmsAnalysis(it).getJSONArray("items").getJSONObject(0) }))
            }
        }
        compose.setContent { val state by vm.state.collectAsState(); MaterialTheme { state.budget?.let { SmsImportScreen(it, vm, analyzer) } } }
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-analyze").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText(context.getString(R.string.sms_select_batch)).performClick()
        compose.onNodeWithTag("sms-analyze").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-consent").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("sms-consent").performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithText("Which account is card *1234?").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText(context.getString(R.string.sms_confirm_review)).assertIsNotEnabled()
        assertNull(inbox.state(identity!!, "mappings"))
        val scroller = compose.onNodeWithTag("lazy-form:${context.getString(R.string.sms_review_title)}")
        scroller.performScrollToNode(hasTestTag("choice-open:${context.getString(R.string.bank_into_account)}"))
        compose.onNodeWithTag("choice-open:${context.getString(R.string.bank_into_account)}").performClick()
        compose.onNodeWithText("Everyday").performClick()
        assertNull(inbox.state(identity!!, "mappings"))
        scroller.performScrollToNode(hasText(context.getString(R.string.sms_remember_mapping)))
        compose.onNodeWithText(context.getString(R.string.sms_remember_mapping)).performClick()
        compose.waitUntil(10_000) { inbox.state(identity!!, "mappings") != null }
        scroller.performScrollToNode(hasText(context.getString(R.string.sms_refine)))
        compose.onNodeWithText(context.getString(R.string.sms_refine)).performClick()
        compose.waitUntil(10_000) { compose.onAllNodesWithTag("sms-consent").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(1, calls)
        compose.onNodeWithTag("sms-consent").performClick()
        compose.waitUntil(10_000) { calls == 2 && compose.onAllNodesWithText(context.getString(R.string.sms_confirm_review)).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText(context.getString(R.string.sms_confirm_review)).assertIsEnabled().performClick()
        compose.waitUntil(10_000) { inbox.pendingCount(identity!!) == 0 }
    }
}
