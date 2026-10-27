package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class AssistantPresentationTest {
    @Before fun english() = EnglishWords.install()

    @Test fun tokenBurstsPreserveExactTextAcrossBatchedPublications() {
        val buffer = AssistantStreamBuffer()
        val expected = StringBuilder()
        repeat(20_000) { index ->
            val token = "$index · "
            expected.append(token)
            buffer.append(token)
            if (index % 64 == 0) assertEquals(expected.toString(), buffer.drain()!!.first)
        }
        assertEquals(expected.toString(), buffer.drain()!!.first)
        assertNull(buffer.drain())
    }

    @Test fun completingMessageAndSwitchingConversationDoNotLeakBufferedText() {
        val buffer = AssistantStreamBuffer()
        buffer.append("private reply")
        buffer.append("reasoning", thinking = true)
        buffer.clearReply()
        assertEquals("" to "reasoning", buffer.drain())
        buffer.append("late chunk")
        buffer.clear()
        assertNull(buffer.drain())
        buffer.append("new conversation")
        assertEquals("new conversation" to "", buffer.drain())
    }

    @Test fun largeUnchangedPollKeepsTheEntirePublishedPresentation() {
        val thread = conversation("one", 5_000)
        val first = assistantThreadPresentation(thread, null)
        val refreshed = assistantThreadPresentation(JSONObject(thread.toString()), first)
        assertSame(first, refreshed)
        assertSame(first.entries, refreshed.entries)
    }

    @Test fun appendingReplyOrUpdatingProposalOnlyInvalidatesChangedEntries() {
        val original = conversation("one", 500)
        val first = assistantThreadPresentation(original, null)
        val next = JSONObject(original.toString())
        next.getJSONArray("entries").getJSONObject(300).put("text", "Rechecked proposal")
        next.getJSONArray("entries").put(JSONObject().put("kind", "assistant").put("text", "Final reply"))
        val refreshed = assistantThreadPresentation(next, first)
        assertEquals(501, refreshed.entries.size)
        repeat(500) { if (it != 300) assertSame(first.entries[it], refreshed.entries[it]) }
        assertNotSame(first.entries[300], refreshed.entries[300])
        assertEquals("Message 300", first.entries[300].getString("text"))
        assertEquals("Rechecked proposal", refreshed.entries[300].getString("text"))
    }

    @Test fun anotherConversationNeverReusesOldEntryReferences() {
        val first = assistantThreadPresentation(conversation("one", 20), null)
        val second = assistantThreadPresentation(conversation("two", 20), first)
        repeat(20) { assertNotSame(first.entries[it], second.entries[it]) }
    }

    @Test fun leavingARowOutAndPuttingItBackChangesOnlyItsKey() {
        assertJson("""{"skip":[],"set":[]}""", ProposalChoices.original())
        val one = ProposalChoices.toggle(null, "t:b", skipped = false)
        assertJson("""{"skip":["t:b"],"set":[]}""", one)
        assertEquals(listOf("t:b", "t:a"), keys(ProposalChoices.toggle(one, "t:a", skipped = false), "skip"))
        // Put back: out of skip, and an amount of nothing given to it goes too; other choices stay.
        val choices = JSONObject().put("skip", JSONArray(listOf("s:0", "s:1"))).put("set", JSONArray(listOf(
            JSONObject().put("key", "s:0").put("amount", 0), JSONObject().put("key", "s:1").put("category", JSONObject.NULL),
            JSONObject().put("key", "s:2").put("amount", 0))))
        val back = ProposalChoices.toggle(choices, "s:0", skipped = true)
        assertEquals(listOf("s:1"), keys(back, "skip"))
        assertEquals(listOf("s:1", "s:2"), back.getJSONArray("set").let { set -> (0 until set.length()).map { set.getJSONObject(it).getString("key") } })
        // A category given stays when its row is put back, Ready to Assign (null) included.
        val again = ProposalChoices.toggle(back, "s:1", skipped = true)
        assertEquals(emptyList<String>(), keys(again, "skip"))
        assertTrue(again.getJSONArray("set").getJSONObject(0).let { it.has("category") && it.isNull("category") })
    }

    @Test fun anEditReplacesItsRowsEntryOverWhatItWasGiven() {
        val one = ProposalChoices.change(JSONObject().put("skip", JSONArray(listOf("r:r1"))).put("set", JSONArray()), "t:b", JSONObject().put("category", "groceries"))
        assertJson("""{"skip":["r:r1"],"set":[{"category":"groceries","key":"t:b"}]}""", one)
        val two = ProposalChoices.change(one, "t:a", JSONObject().put("category", JSONObject.NULL))
        assertJson("""{"skip":["r:r1"],"set":[{"category":"groceries","key":"t:b"},{"category":null,"key":"t:a"}]}""", two)
        // Another amount for t:b keeps the category it was given.
        val three = ProposalChoices.change(two, "t:b", JSONObject().put("amount", -30000))
        assertJson("""{"skip":["r:r1"],"set":[{"category":null,"key":"t:a"},{"category":"groceries","amount":-30000,"key":"t:b"}]}""", three)
    }

    @Test fun savingARowsEditorGivesWhatChangedKeepingTheRowsSign() {
        Money.configure(JSONObject().put("currency", "SAR"))
        val home = JSONObject().put("label", "Home").put("amount", -45900).put("key", "s:0")
            .put("edit", JSONObject().put("category", true).put("categoryId", "rent").put("amount", true))
        assertJson("""{"amount":-30000}""", ProposalChoices.edit(home, "rent", "300"))
        assertJson("""{"amount":500}""", ProposalChoices.edit(home, "rent", "+5"))
        assertNull(ProposalChoices.edit(home, "rent", "-459.00"))
        assertNull(ProposalChoices.edit(home, "rent", " 459 "))
        assertJson("""{"category":null}""", ProposalChoices.edit(home, null, "-459.00"))
        assertJson("""{"category":"groceries","amount":-1250}""", ProposalChoices.edit(home, "groceries", "12.5"))
        assertThrows(IllegalArgumentException::class.java) { ProposalChoices.edit(home, "rent", "lots") }
        // A category only: the amount isn't read. Ready to Assign is a row with no categoryId.
        val careem = JSONObject().put("label", "Careem").put("amount", -3800).put("key", "t:a")
            .put("edit", JSONObject().put("category", true).put("amount", false))
        assertNull(ProposalChoices.edit(careem, null, "lots"))
        assertJson("""{"category":"transport"}""", ProposalChoices.edit(careem, "transport", ""))
        // A positive row stays positive.
        val salary = JSONObject().put("label", "Salary").put("amount", 2000).put("key", "t:s").put("edit", JSONObject().put("category", false).put("amount", true))
        assertJson("""{"amount":1500}""", ProposalChoices.edit(salary, null, "15"))
        assertNull(ProposalChoices.edit(JSONObject().put("label", "New rule"), null, ""))
    }

    @Test fun applySaysHowMuchOfTheCardItMakesWhenSomeIsLeftOut() {
        val rows = listOf(JSONObject().put("key", "t:a"), JSONObject().put("key", "t:b").put("skipped", true), JSONObject().put("key", "r:r1"), JSONObject().put("label", "unkeyed"))
        assertEquals(ProposalChoices.Partial(2, 3), ProposalChoices.partial(rows, choosing = true))
        assertNull(ProposalChoices.partial(rows, choosing = false))
        assertNull(ProposalChoices.partial(rows.map { JSONObject(it.toString()).put("skipped", false) }, choosing = true))
        assertEquals(ProposalChoices.Partial(0, 3), ProposalChoices.partial(rows.map { JSONObject(it.toString()).put("skipped", true) }, choosing = true))
    }

    /** Key order is the JSON library's, not the revision's. */
    private fun assertJson(expected: String, actual: JSONObject?) = assertTrue("$actual", actual != null && sameJson(JSONObject(expected), actual))
    private fun keys(revision: JSONObject, name: String) = revision.getJSONArray(name).let { list -> (0 until list.length()).map(list::getString) }

    private fun conversation(id: String, count: Int) = JSONObject().put("id", id).put("running", false)
        .put("entries", JSONArray((0 until count).map { JSONObject().put("kind", "assistant").put("text", "Message $it") }))
}
