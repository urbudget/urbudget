package app.urbudget.nativeapp

import kotlinx.coroutines.test.runTest
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.IOException

/** What a save may and may not repeat: the same bytes in, one object and one calculation out. */
class BudgetPersistTest {
    @org.junit.Before fun english() = EnglishWords.install()

    private class Storage(var saved: String?) : BudgetStorage {
        override val identity = "budget"
        var fail = false
        var writes = 0
        override fun read() = saved
        override fun write(json: String) { if (fail) throw IOException("Disk full"); saved = json; writes++ }
    }

    private val methods = mutableListOf<String>()
    private val engine = BudgetEngine { text ->
        val request = JSONObject(text)
        methods += request.getString("method")
        if (request.getJSONObject("budget").optBoolean("invalid")) JSONObject().put("ok", false).put("error", "Invalid test budget").toString()
        else JSONObject().put("ok", true).put("value", JSONObject()).toString()
    }

    @Test fun anEditPublishesItsOwnObjectAndCalculatesOnce() = runTest {
        val storage = Storage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        methods.clear()
        val edited = repository.edit { it.put("name", "Edited") }
        assertNotSame(opened.budget, edited.budget)
        assertEquals("Edited", edited.budget!!.getString("name"))
        assertEquals(listOf("calculate"), methods)
        assertEquals("Original", opened.budget!!.getString("name"))
        // Nothing to parse or calculate again: the next load finds what was just published.
        val loaded = repository.load()
        assertSame(edited.budget, loaded.budget)
        assertSame(edited.summary, loaded.summary)
        assertEquals(listOf("calculate"), methods)
    }

    @Test fun anEditThatChangesNothingKeepsThePublishedObjects() = runTest {
        val storage = Storage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        methods.clear()
        val same = repository.edit { }
        assertSame(opened.budget, same.budget)
        assertSame(opened.summary, same.summary)
        assertTrue(methods.isEmpty())
    }

    @Test fun anUnchangedEditKeepsTheSelectedMonthsTotals() = runTest {
        val storage = Storage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        val selected = repository.selectMonth("2026-11")
        methods.clear()
        val same = repository.edit { }
        assertSame(opened.budget, same.budget)
        assertSame(selected.summary, same.summary)
        assertTrue(methods.isEmpty())
        assertEquals("2026-11", same.month)
    }

    @Test fun anEditCalculatesTheSelectedMonth() = runTest {
        val months = mutableListOf<String>()
        val counted = BudgetEngine { text -> months += JSONObject(text).getJSONObject("params").getString("month"); engine.invoke(text) }
        val repository = BudgetRepository(Storage(BUDGET), counted, "2026-10")
        repository.load()
        repository.selectMonth("2026-11")
        months.clear()
        assertEquals("2026-11", repository.edit { it.put("name", "Edited") }.month)
        assertEquals(listOf("2026-11"), months)
    }

    @Test fun aFailedWriteChangesNothingCached() = runTest {
        val storage = Storage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        storage.fail = true
        try { repository.edit { it.put("name", "Unsaved") }; fail("Expected write failure") } catch (_: IOException) {}
        methods.clear()
        val after = repository.selectMonth("2026-10")
        assertSame(opened.budget, after.budget)
        assertSame(opened.summary, after.summary)
        assertTrue(methods.isEmpty())
        assertEquals("Original", opened.budget!!.getString("name"))
    }

    @Test fun aRefusedCalculationWritesNothingAndChangesNothingCached() = runTest {
        val storage = Storage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        try { repository.edit { it.put("invalid", true) }; fail("Expected refusal") } catch (_: IllegalArgumentException) {}
        assertEquals(0, storage.writes)
        assertSame(opened.budget, repository.selectMonth("2026-10").budget)
    }

    companion object {
        /** As this JSON library writes it: "unchanged" means the same text, so its key order counts here. */
        val BUDGET: String = JSONObject("""{"schemaVersion":2,"name":"Original","currency":"USD","accounts":[],"categories":[],"assignments":[],"transactions":[]}""").toString()
    }
}
