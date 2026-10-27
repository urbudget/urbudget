package app.urbudget.nativeapp

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

class CategoryStatusTest {
    @Before fun english() = EnglishWords.install()
    private val month = "2026-10"
    private fun category(target: Long = 0, goal: JSONObject? = null) = JSONObject().put("id", "c").put("name", "Rent").put("target", target).apply { if (goal != null) put("goal", goal) }
    private fun summary(available: Long, assigned: Long = 0, activity: Long = 0, needed: Long = 0, required: Long = 0, funded: Long = 0) =
        JSONObject().put("id", "c").put("available", available).put("assigned", assigned).put("activity", activity).put("needed", needed)
            .put("targetFundingRequired", required).put("targetFundingFunded", funded).put("carried", 0)

    @Test fun statusFollowsTheDesktopsPillRules() {
        assertEquals("positive", CategoryStatuses.of(category(), summary(500), month).status)
        assertEquals("zero", CategoryStatuses.of(category(), summary(0), month).status)
        assertEquals("negative", CategoryStatuses.of(category(), summary(-100), month).status)
        assertEquals("underfunded", CategoryStatuses.of(category(10000), summary(2000, assigned = 2000, needed = 8000, required = 10000, funded = 2000), month).status)
        assertEquals("credit", CategoryStatuses.of(category(), summary(-300).put("creditOverspending", 300), month).status)
    }

    @Test fun progressComparesFundingWithWhatTheTargetRequires() {
        assertEquals(25, CategoryStatuses.of(category(8000), summary(2000, assigned = 2000, needed = 6000, required = 8000, funded = 2000), month).progress)
        assertEquals(100, CategoryStatuses.of(category(8000), summary(8000, assigned = 8000, required = 8000, funded = 8000), month).progress)
        assertNull(CategoryStatuses.of(category(), summary(10), month).progress)
    }

    @Test fun targetWordsMatchYnab() {
        val c = category(8000)
        val short = summary(2000, assigned = 2000, needed = 6000, required = 8000, funded = 2000).put("targetDue", JSONObject().put("date", "2026-10-31").put("monthly", true))
        val words = CategoryStatuses.targetWords(c, short, CategoryStatuses.of(c, short, month), false)!!
        assertEquals(6000L, words.amount)
        assertEquals("more needed by the 31st", words.words)
        val dated = summary(0, needed = 6000, required = 6000).put("targetDue", JSONObject().put("date", "2026-11-01").put("monthly", false))
        assertEquals("needed by November 1st", CategoryStatuses.targetWords(c, dated, CategoryStatuses.of(c, dated, month), true)!!.words)
        val funded = summary(8000, assigned = 8000, required = 8000, funded = 8000)
        assertEquals("Funded", CategoryStatuses.targetWords(c, funded, CategoryStatuses.of(c, funded, month), false)!!.words)
        val spent = summary(0, assigned = 8000, activity = -8000, required = 8000, funded = 8000)
        assertEquals("Fully spent", CategoryStatuses.targetWords(c, spent, CategoryStatuses.of(c, spent, month), false)!!.words)
    }

    @Test fun snoozedTargetsAskOnlyForCommitments() {
        val c = category(8000, JSONObject().put("goal_type", "NEED").put("goal_snoozed_at", "2026-10-01"))
        val state = CategoryStatuses.of(c, summary(0, needed = 8000, required = 8000), month)
        assertEquals(true, state.snoozed)
        assertEquals("zero", state.status)
    }
}
