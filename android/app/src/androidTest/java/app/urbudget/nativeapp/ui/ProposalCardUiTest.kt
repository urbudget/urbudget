package app.urbudget.nativeapp.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ProposalAction
import app.urbudget.nativeapp.ProposalChoices
import app.urbudget.nativeapp.ResourceWords
import app.urbudget.nativeapp.Words
import app.urbudget.nativeapp.sameJson
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * An assistant proposal changed on its card before it's applied (desktop ProposalCard.test.tsx).
 * Synthetic: the engine's revise is stood in for, with no budget runtime, credentials or network.
 */
@RunWith(AndroidJUnit4::class)
class ProposalCardUiTest {
    @get:Rule val compose = createComposeRule()

    /** The app's words, as its activity loads them: an amount that doesn't read says why in them. */
    @Before fun words() { Words.source = ResourceWords(InstrumentationRegistry.getInstrumentation().targetContext.resources) }

    private val rows = listOf(
        row("Careem · Oct 3", -3800, "t:a", "Uncategorized → Transport", JSONObject().put("category", true).put("categoryId", "transport").put("amount", false)),
        row("Extra Electronics · Oct 5", -89900, "t:b", "Uncategorized → Gifts", JSONObject().put("category", true).put("categoryId", "gifts").put("amount", false))
            .put("badge", "Check this").put("tone", "warn"),
        JSONObject().put("label", "New rule: payees containing “careem”").put("detail", "Into Transport").put("badge", "rule").put("key", "r:r1"),
    )
    private val budget = JSONObject().put("currency", "SAR").put("categories", JSONArray(listOf(
        category("transport", "Transport"), category("gifts", "Gifts"), category("groceries", "Groceries"), category("rent", "Rent"))))
    private val revisions = mutableListOf<JSONObject>()
    private var applied = 0

    @Test fun leavesOutAPartWithItsCheckboxAndAppliesTheRest() {
        show(rows)
        compose.onNodeWithText("Apply reviewed changes").assertIsEnabled()
        compose.onNodeWithContentDescription("Apply: Extra Electronics · Oct 5").assertIsOn().performClick()
        compose.runOnIdle { assertJson("""{"skip":["t:b"],"set":[]}""", revisions.last()) }
        compose.onNodeWithContentDescription("Apply: Extra Electronics · Oct 5").assertIsOff()
        // What's applied says how much of it.
        compose.onNodeWithText("Apply 2 of 3").performClick()
        compose.runOnIdle { assertEquals(1, applied) }
        // Nothing chosen: nothing to apply.
        compose.onNodeWithContentDescription("Apply: Careem · Oct 3").performClick()
        compose.onNodeWithContentDescription("Apply: New rule: payees containing “careem”").performClick()
        compose.runOnIdle { assertJson("""{"skip":["t:b","t:a","r:r1"],"set":[]}""", revisions.last()) }
        compose.onNodeWithText("Apply 0 of 3").assertIsNotEnabled()
        // A part left out can't be changed until it's back.
        compose.onNodeWithContentDescription("Change Careem · Oct 3").assertDoesNotExist()
        // And all of it again, as the assistant made it.
        compose.onNodeWithText("Use the assistant's version").performClick()
        compose.runOnIdle { assertJson("""{"skip":[],"set":[]}""", revisions.last()) }
        compose.onNodeWithText("Apply reviewed changes").assertIsEnabled()
        compose.onNodeWithText("Use the assistant's version").assertDoesNotExist()
    }

    @Test fun givesAPartAnotherCategoryOrUnassigned() {
        show(rows)
        // Only what can be changed has a pencil.
        compose.onNodeWithContentDescription("Change New rule: payees containing “careem”").assertDoesNotExist()
        compose.onNodeWithContentDescription("Change Extra Electronics · Oct 5").performClick()
        compose.onNodeWithText("Gifts").performClick()
        compose.onNodeWithText("Groceries").performClick()
        compose.onNodeWithText("Save").performClick()
        compose.runOnIdle { assertJson("""{"skip":[],"set":[{"key":"t:b","category":"groceries"}]}""", revisions.last()) }
        compose.onNodeWithText("Change Extra Electronics · Oct 5").assertDoesNotExist()
        // Unassigned (Ready to Assign) is a category to give too.
        compose.onNodeWithContentDescription("Change Careem · Oct 3").performClick()
        compose.onNodeWithText("Transport").performClick()
        compose.onNodeWithText("Unassigned").performClick()
        compose.onNodeWithText("Save").performClick()
        compose.runOnIdle { assertJson("""{"skip":[],"set":[{"key":"t:b","category":"groceries"},{"key":"t:a","category":null}]}""", revisions.last()) }
    }

    @Test fun givesAPartAnotherAmountKeepingItsSign() {
        val both = { id: String -> JSONObject().put("category", true).put("categoryId", id).put("amount", true) }
        show(listOf(row("Home", -45900, "s:0", null, both("rent")), row("Food", -2400, "s:1", null, both("groceries"))))
        compose.onNodeWithContentDescription("Change Home").performClick()
        compose.onNode(hasSetTextAction()).assert(hasText("-459.00")).performTextReplacement("300")
        compose.onNodeWithText("Save").performClick()
        compose.runOnIdle { assertJson("""{"skip":[],"set":[{"key":"s:0","amount":-30000}]}""", revisions.last()) }
        compose.onNodeWithText(money(-30000, "SAR")).assertExists()
        // Cancel: nothing changes.
        compose.onNodeWithContentDescription("Change Food").performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("+5")
        compose.onNodeWithText("Cancel").performClick()
        compose.runOnIdle { assertEquals(1, revisions.size) }
        // An amount that doesn't read says so.
        compose.onNodeWithContentDescription("Change Food").performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("lots")
        compose.onNodeWithText("Save").performClick()
        compose.onNodeWithText("Enter an amount", substring = true).assertExists()
        compose.runOnIdle { assertEquals(1, revisions.size) }
    }

    @Test fun aBankTextCardSaysWhatItDoesAndIsDoneOnThePhone() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        var done = 0
        val proposal = mutableStateOf(JSONObject().put("id", "p1").put("kind", "bankTexts").put("status", "proposed").put("title", "Set a credit limit")
            .put("actions", JSONArray(listOf("applyBankTexts", "askChanges", "discard")))
            .put("bankTexts", JSONObject().put("action", "limit").put("accountId", "card").put("limit", 250_050L))
            .put("card", JSONObject().put("kicker", "Bank messages").put("rows", JSONArray().put(JSONObject().put("label", "Credit limit for Visa").put("detail", "None set yet").put("amount", 250_050L)))))
        compose.setContent {
            MaterialTheme { AssistantProposalCard(proposal.value, "SAR", busy = false, act = { if (it == ProposalAction.ApplyBankTexts) done++ }) }
        }
        compose.onNodeWithText("Credit limit for Visa").assertExists()
        compose.onNodeWithText(context.getString(app.urbudget.nativeapp.R.string.bank_texts_apply_limit)).performClick()
        compose.runOnIdle { assertEquals(1, done) }
        // Applied, it folds to a line that says so, with nothing more to do; the line opens on what it set.
        proposal.value = JSONObject(proposal.value.toString()).put("status", "applied").put("actions", JSONArray())
        compose.onNodeWithText("Applied: Set a credit limit").assertExists()
        compose.onNodeWithText(context.getString(app.urbudget.nativeapp.R.string.bank_texts_apply_limit)).assertDoesNotExist()
        compose.onNodeWithText("Credit limit for Visa").assertDoesNotExist()
        compose.onNodeWithTag("assistant-card-line").performClick()
        compose.onNodeWithText("Credit limit for Visa").assertExists()
    }

    @Test fun aCardDrawsOnlyWhatTheCoreOffersAndAKindItDoesntKnowPlainly() {
        val acts = mutableListOf<ProposalAction>()
        val proposal = mutableStateOf(draft(rows).put("kind", "somethingNewer").put("actions", JSONArray(listOf("apply", "somethingNewer", "discard")))
            .put("message", JSONObject().put("tone", "info").put("text", "Your budget changed.")))
        compose.setContent { MaterialTheme { AssistantProposalCard(proposal.value, "SAR", busy = false, act = { acts += it }, choose = {}, change = {}) } }
        // Its rows to read, what it says, and the buttons this app knows: nothing to tick or change.
        compose.onNodeWithTag("assistant-card-update").assertExists()
        compose.onNodeWithTag("assistant-card-message:info").assertExists()
        compose.onNodeWithText("Careem · Oct 3").assertExists()
        compose.onNodeWithContentDescription("Apply: Careem · Oct 3").assertDoesNotExist()
        compose.onNodeWithText("Apply reviewed changes").performClick()
        compose.onNodeWithText("Discard").performClick()
        compose.runOnIdle { assertEquals(listOf(ProposalAction.Apply, ProposalAction.Discard), acts) }
        // A failed card says why in its own strip, and offers a new version.
        proposal.value = draft(rows).put("status", "failed").put("actions", JSONArray(listOf("askAgain", "discard")))
            .put("message", JSONObject().put("tone", "error").put("text", "This can't be applied any more."))
        compose.onNodeWithTag("assistant-card-message:error").assertExists()
        compose.onNodeWithText("This can't be applied any more.").assertExists()
        compose.onNodeWithText("Ask for a new version").performClick()
        compose.runOnIdle { assertEquals(ProposalAction.AskAgain, acts.last()) }
        compose.onNodeWithContentDescription("Apply: Careem · Oct 3").assertDoesNotExist()
    }

    @Test fun aFinishedCardFoldsToALineThatKeepsUndoAndOpensOnItsRows() {
        val acts = mutableListOf<ProposalAction>()
        val applied = draft(rows).put("status", "applied").put("actions", JSONArray(listOf("undo", "show")))
        val proposal = mutableStateOf(applied)
        val undoable = mutableStateOf<String?>("p1")
        compose.setContent { MaterialTheme { AssistantProposalCard(proposal.value, "SAR", busy = false, act = { acts += it }, undoable = undoable.value, choose = {}, change = {}) } }
        compose.onNodeWithText("Applied: Categorize 2 transactions").assertExists()
        compose.onNodeWithText("Careem · Oct 3").assertDoesNotExist()
        compose.onNodeWithText("Undo this proposal").performClick()
        compose.onNodeWithText("Show in the budget").performClick()
        compose.runOnIdle { assertEquals(listOf(ProposalAction.Undo, ProposalAction.Show), acts) }
        // Opened, its rows are there to read, as they were applied.
        compose.onNodeWithTag("assistant-card-line").performClick()
        compose.onNodeWithText("Careem · Oct 3").assertExists()
        compose.onNodeWithContentDescription("Apply: Careem · Oct 3").assertDoesNotExist()
        // Undo only while the budget's own Undo would take this card's change back.
        undoable.value = null
        compose.onNodeWithText("Undo this proposal").assertDoesNotExist()
        compose.onNodeWithText("Show in the budget").assertExists()
        // Undone, it can be applied again from inside; set aside, it can be put back.
        proposal.value = draft(rows).put("status", "undone").put("actions", JSONArray(listOf("apply", "askChanges", "discard")))
        compose.onNodeWithText("Undone: Categorize 2 transactions").assertExists()
        compose.onNodeWithText("Apply reviewed changes").performClick()
        proposal.value = draft(rows).put("status", "discarded").put("actions", JSONArray(listOf("restore")))
        compose.onNodeWithText("Discarded: Categorize 2 transactions").assertExists()
        compose.onNodeWithText("Restore").performClick()
        compose.runOnIdle { assertEquals(listOf(ProposalAction.Apply, ProposalAction.Restore), acts.takeLast(2)) }
    }

    @Test fun aThingTheAssistantMadeHasItsOwnButtonsAndRemoveInItsMenu() {
        val acts = mutableListOf<ProposalAction>()
        val report = JSONObject().put("id", "p1").put("kind", "report").put("status", "proposed").put("title", "October in review").put("made", true)
            .put("actions", JSONArray(listOf("preview", "print", "excel", "remove"))).put("card", JSONObject().put("kicker", "Report").put("rows", JSONArray()))
        val proposal = mutableStateOf(report)
        compose.setContent { MaterialTheme { AssistantProposalCard(proposal.value, "SAR", busy = false, act = { acts += it }) } }
        compose.onNodeWithText("Preview").performClick()
        compose.onNodeWithText("Discard").assertDoesNotExist()
        compose.onNodeWithText("Remove").assertDoesNotExist()
        compose.onNodeWithTag("assistant-card-menu").performClick()
        compose.onNodeWithText("Remove").performClick()
        compose.runOnIdle { assertEquals(listOf(ProposalAction.Preview, ProposalAction.Remove), acts) }
        // Removed, it folds to a line with Restore.
        proposal.value = JSONObject(report.toString()).put("status", "discarded").put("actions", JSONArray(listOf("restore")))
        compose.onNodeWithText("Removed: October in review").assertExists()
        compose.onNodeWithText("Restore").assertExists()
    }

    @Test fun aQuestionsAnswersAreButtonsAndFiguresHaveBarsAndADeletionSaysHowMany() {
        val answers = mutableListOf<String>()
        val question = JSONObject().put("id", "p1").put("kind", "question").put("status", "proposed").put("title", "Which account paid for it?")
            .put("actions", JSONArray(listOf("answer"))).put("card", JSONObject().put("kicker", "Question").put("rows", JSONArray(listOf(
                JSONObject().put("label", "Checking").put("key", "o:0"), JSONObject().put("label", "Visa").put("key", "o:1")))))
        val proposal = mutableStateOf(question)
        compose.setContent { MaterialTheme { AssistantProposalCard(proposal.value, "SAR", busy = false, act = {}, choose = {}, answer = { answers += it }) } }
        // Each answer is a button, never a row to tick; pressing one sends its words.
        compose.onNodeWithContentDescription("Apply: Visa").assertDoesNotExist()
        compose.onNodeWithText("Visa").performClick()
        compose.runOnIdle { assertEquals(listOf("Visa"), answers) }
        // Answered, it folds to what was asked, and its answers are no longer buttons.
        proposal.value = JSONObject(question.toString()).put("status", "applied").put("actions", JSONArray())
        compose.onNodeWithText("Asked: Which account paid for it?").assertExists()
        compose.onNodeWithTag("assistant-card-line").performClick()
        compose.onAllNodesWithTag("assistant-answer").assertCountEquals(0)
        compose.onNodeWithText("Visa").assertExists()

        // Figures: a bar for each row, and Remove in the menu with Show as the button.
        proposal.value = JSONObject().put("id", "p2").put("kind", "figures").put("status", "proposed").put("title", "Spending in October").put("made", true)
            .put("actions", JSONArray(listOf("remove", "show"))).put("card", JSONObject().put("kicker", "Figures").put("rows", JSONArray(listOf(
                JSONObject().put("label", "Groceries").put("amount", -120_000L), JSONObject().put("label", "Dining out").put("amount", -60_000L))))
                .put("total", JSONObject().put("label", "Total").put("amount", -180_000L)))
        compose.onAllNodesWithTag("assistant-figure-bar").assertCountEquals(2)
        compose.onNodeWithText("Total").assertExists()
        compose.onNodeWithText("Show in the budget").assertExists()

        // A deletion's button says how many it deletes, and fewer once some are left out.
        val deleting = listOf(row("Careem · Oct 3", -3800, "t:a", null, JSONObject()), row("Extra Electronics · Oct 5", -89900, "t:b", null, JSONObject()))
        val delete = { skipped: Boolean -> JSONObject().put("id", "p3").put("kind", "delete").put("status", "proposed").put("title", "Delete 2 transactions")
            .put("actions", JSONArray(listOf("apply", "askChanges", "discard")))
            .put("card", JSONObject().put("kicker", "Proposal · Delete").put("rows", JSONArray(deleting.map { JSONObject(it.toString()).put("skipped", skipped && it.optString("key") == "t:b") }))) }
        proposal.value = delete(false)
        compose.onNodeWithTag("assistant-card-main").assertTextEquals("Delete 2 transactions")
        proposal.value = delete(true)
        compose.onNodeWithTag("assistant-card-main").assertTextEquals("Delete 1 transaction")
    }

    /** The card as the assistant screen keeps it: each revision made by `marking`, the card it gives shown, and a row's editor over it. */
    private fun show(start: List<JSONObject>) {
        val proposal = mutableStateOf(draft(start))
        val changing = mutableStateOf<JSONObject?>(null)
        val revise = { revision: JSONObject -> revisions += revision; proposal.value = marking(revision, proposal.value, start) }
        compose.setContent {
            MaterialTheme {
                AssistantProposalCard(proposal.value, "SAR", busy = false, act = { action ->
                    when (action) {
                        ProposalAction.Apply -> applied++
                        ProposalAction.UseOriginal -> revise(ProposalChoices.original())
                        else -> {}
                    }
                }, choose = revise, change = { changing.value = it })
                changing.value?.let { row ->
                    ProposalRowEditor(row, budget, null, null, "2026-10", { change ->
                        revise(ProposalChoices.change(proposal.value.optJSONObject("choices"), row.text("key"), change)); changing.value = null
                    }) { changing.value = null }
                }
            }
        }
    }

    /** What the engine does with a revision, as far as the card shows it. */
    private fun marking(revision: JSONObject, proposal: JSONObject, start: List<JSONObject>): JSONObject {
        val skip = revision.getJSONArray("skip").let { list -> (0 until list.length()).map(list::getString) }
        val set = revision.getJSONArray("set").let { list -> (0 until list.length()).map(list::getJSONObject) }
        val next = JSONObject(proposal.toString())
        if (skip.isEmpty() && set.isEmpty()) next.remove("choices") else next.put("choices", revision)
        // The assistant's own version is on offer once the user has chosen something.
        next.put("actions", JSONArray(if (next.has("choices")) listOf("apply", "askChanges", "useOriginal", "discard") else listOf("apply", "askChanges", "discard")))
        next.getJSONObject("card").put("rows", JSONArray(start.map { row ->
            val given = set.firstOrNull { it.getString("key") == row.optString("key") }
            JSONObject(row.toString()).put("skipped", row.optString("key") in skip).put("edited", given != null).apply {
                if (given?.has("amount") == true) put("amount", given.getLong("amount"))
                if (given?.has("category") == true) getJSONObject("edit").put("categoryId", given.opt("category"))
            }
        }))
        return next
    }

    private fun draft(rows: List<JSONObject>) = JSONObject().put("id", "p1").put("title", "Categorize 2 transactions").put("kind", "transactions")
        .put("status", "proposed").put("made", false).put("actions", JSONArray(listOf("apply", "askChanges", "discard")))
        .put("card", JSONObject().put("kicker", "Proposal · Transactions").put("rows", JSONArray(rows)).put("stats", JSONArray()).put("more", 0))
    private fun row(label: String, amount: Long, key: String, detail: String?, edit: JSONObject) =
        JSONObject().put("label", label).put("amount", amount).put("key", key).put("edit", edit).apply { if (detail != null) put("detail", detail) }
    private fun category(id: String, name: String) = JSONObject().put("id", id).put("name", name).put("group", "Everyday")
    private fun assertJson(expected: String, actual: JSONObject) = assertTrue("$actual", sameJson(JSONObject(expected), actual))
}
