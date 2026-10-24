package app.urbudget.nativeapp.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import app.urbudget.nativeapp.ProposalChoices
import app.urbudget.nativeapp.sameJson
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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

    /** The card as the assistant screen keeps it: each revision made by `marking`, the card it gives shown, and a row's editor over it. */
    private fun show(start: List<JSONObject>) {
        val proposal = mutableStateOf(draft(start))
        val changing = mutableStateOf<JSONObject?>(null)
        val revise = { revision: JSONObject -> revisions += revision; proposal.value = marking(revision, proposal.value, start) }
        compose.setContent {
            MaterialTheme {
                AssistantProposalCard(proposal.value, "SAR", busy = false, canUndo = false, apply = { applied++ }, undo = {}, discard = {}, refine = {},
                    reviewImport = {}, saveImport = {}, report = {}, spreadsheet = {}, revise = revise, change = { changing.value = it })
                changing.value?.let { row ->
                    ProposalRowEditor(row, budget, null, "2026-10", { change ->
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
        .put("status", "proposed").put("card", JSONObject().put("kicker", "Proposal · Transactions").put("rows", JSONArray(rows)).put("stats", JSONArray()).put("more", 0))
    private fun row(label: String, amount: Long, key: String, detail: String?, edit: JSONObject) =
        JSONObject().put("label", label).put("amount", amount).put("key", key).put("edit", edit).apply { if (detail != null) put("detail", detail) }
    private fun category(id: String, name: String) = JSONObject().put("id", id).put("name", name).put("group", "Everyday")
    private fun assertJson(expected: String, actual: JSONObject) = assertTrue("$actual", sameJson(JSONObject(expected), actual))
}
