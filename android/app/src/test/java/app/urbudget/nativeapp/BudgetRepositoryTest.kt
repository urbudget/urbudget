package app.urbudget.nativeapp

import kotlinx.coroutines.async
import kotlinx.coroutines.test.runTest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.IOException

class BudgetRepositoryTest {
    @org.junit.Before fun english() = EnglishWords.install()
    private class MemoryStorage(var saved: String? = null) : BudgetStorage {
        override var identity: String? = "first-budget"
        var fail = false
        var recovery: String? = null
        override fun read() = saved
        override fun write(json: String) { if (fail) throw IOException("Disk full"); saved = json }
        override fun retainRecovery(json: String) { recovery = json }
        override fun readRecovery() = recovery
        override fun hasRecovery() = recovery != null
    }
    // The real Rust engine is exercised by instrumentation. This deterministic fake isolates
    // persistence ordering and failure recovery, without implementing any financial arithmetic.
    private val engine = BudgetEngine { text ->
        val request = JSONObject(text)
        if (request.optJSONObject("budget")?.optBoolean("invalid") == true) {
            JSONObject().put("ok", false).put("error", "Invalid test budget").toString()
        } else {
            val value = when (request.getString("method")) {
                "defaultBudget", "sampleBudget" -> JSONObject(BUDGET)
                "validate" -> JSONObject.NULL
                else -> JSONObject()
            }
            JSONObject().put("ok", true).put("value", value).toString()
        }
    }

    @Test fun failedWriteDoesNotPublishOrChangeUndoHistory() = runTest {
        val storage = MemoryStorage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        repository.load()
        storage.fail = true
        try { repository.edit { it.put("name", "Unsaved") }; fail("Expected write failure") } catch (_: IOException) {}
        assertEquals("Original", JSONObject(repository.exportBudget()!!).getString("name"))
        assertFalse(repository.selectMonth("2026-10").undoAvailable)
        storage.fail = false
        repository.edit { it.put("name", "Saved") }
        assertEquals("Saved", JSONObject(storage.saved!!).getString("name"))
        assertEquals("Original", repository.undo().budget!!.getString("name"))
    }

    @Test fun invalidImportLeavesLastDurableBudgetUntouched() = runTest {
        val storage = MemoryStorage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        repository.load()
        storage.recovery = "earlier backup"
        try { repository.importBudget(JSONObject(BUDGET).put("invalid", true).toString()); fail("Expected validation failure") } catch (_: IllegalArgumentException) {}
        assertEquals(BUDGET, storage.saved)
        assertEquals(BUDGET, repository.exportBudget())
        assertEquals("earlier backup", storage.recovery)
    }

    @Test fun editingPreservesUnknownRootRecordAndNestedFields() = runTest {
        val original = JSONObject(BUDGET).put("futureRoot", JSONObject().put("keep", true))
            .put("accounts", JSONArray().put(JSONObject().put("id", "cash").put("name", "Before").put("futureField", JSONObject().put("nested", 7))))
        val storage = MemoryStorage(original.toString())
        val repository = BudgetRepository(storage, engine, "2026-10")
        repository.load()
        repository.edit { it.upsertRecord("accounts", JSONObject().put("id", "cash").put("name", "After")) }
        val saved = JSONObject(storage.saved!!)
        assertTrue(saved.getJSONObject("futureRoot").getBoolean("keep"))
        assertEquals(7, saved.getJSONArray("accounts").getJSONObject(0).getJSONObject("futureField").getInt("nested"))
    }

    @Test fun concurrentEditsSerializeAgainstLatestDurableSnapshot() = runTest {
        val storage = MemoryStorage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        repository.load()
        val first = async { repository.edit { it.put("first", true) } }
        val second = async { repository.edit { it.put("second", true) } }
        first.await(); second.await()
        val saved = JSONObject(storage.saved!!)
        assertTrue(saved.getBoolean("first"))
        assertTrue(saved.getBoolean("second"))
    }

    @Test fun failedLoadStillAllowsRecoveryExport() = runTest {
        val storage = MemoryStorage("{bad json")
        val repository = BudgetRepository(storage, engine, "2026-10")
        try { repository.load(); fail("Expected invalid JSON") } catch (_: Exception) {}
        assertEquals("{bad json", repository.exportBudget())
    }

    @Test fun replacingBudgetRetainsRecoverablePreviousBytes() = runTest {
        val storage = MemoryStorage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        repository.load()
        repository.importBudget(JSONObject(BUDGET).put("name", "Imported").toString())
        assertEquals(BUDGET, storage.recovery)
        assertTrue(repository.selectMonth("2026-10").undoAvailable)
        assertEquals("Original", repository.restoreRecovery().budget!!.getString("name"))
    }

    @Test fun queuedEditorCannotOverwriteARecordChangedByEarlierRefresh() = runTest {
        val initial = JSONObject(BUDGET).put("accounts", JSONArray().put(JSONObject().put("id", "cash").put("name", "Before")))
        val storage = MemoryStorage(initial.toString())
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        val guard = BudgetEditGuard.record(opened.budget, opened.identity, "accounts", "cash")
        // A sync queued ahead of the edit publishes a fresh durable record after the
        // editor has already confirmed its old values but before its command executes.
        storage.saved = initial.put("accounts", JSONArray().put(JSONObject().put("id", "cash").put("name", "Remote name"))).toString()
        repository.load()
        try {
            repository.edit(guard) { it.upsertRecord("accounts", JSONObject().put("id", "cash").put("name", "Old editor name")) }
            fail("Expected a stale editor conflict")
        } catch (expected: IllegalArgumentException) { assertTrue(expected.message!!.contains("changed")) }
        assertEquals("Remote name", JSONObject(storage.saved!!).records("accounts").single().getString("name"))
        assertFalse(repository.selectMonth("2026-10").undoAvailable)
    }

    @Test fun independentQueuedAccountLinksPreserveBothChangesAndImportedMetadata() = runTest {
        val initial = JSONObject(BUDGET).put("accounts", JSONArray()
            .put(JSONObject().put("id", "cash").put("name", "Cash").put("unknown", JSONObject().put("version", 7)))
            .put(JSONObject().put("id", "card").put("name", "Card")))
        val storage = MemoryStorage(initial.toString())
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        val cash = BudgetEditGuard.record(opened.budget, opened.identity, "accounts", "cash")
        val card = BudgetEditGuard.record(opened.budget, opened.identity, "accounts", "card")
        repository.edit(cash) { it.upsertRecord("accounts", JSONObject().put("id", "cash").put("simplefinId", "bank-cash")) }
        repository.edit(card) { it.upsertRecord("accounts", JSONObject().put("id", "card").put("simplefinId", "bank-card")) }
        val accounts = JSONObject(storage.saved!!).records("accounts")
        assertEquals(listOf("bank-cash", "bank-card"), accounts.map { it.getString("simplefinId") })
        assertEquals(7, accounts.first().getJSONObject("unknown").getInt("version"))
    }

    @Test fun recordGuardAllowsUnrelatedRefreshButRejectsIdenticalOtherBudget() = runTest {
        val original = JSONObject(BUDGET).put("accounts", JSONArray().put(JSONObject().put("id", "cash").put("name", "Cash")))
        val storage = MemoryStorage(original.toString())
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        val guard = BudgetEditGuard.record(opened.budget, opened.identity, "accounts", "cash")
        storage.saved = original.put("remoteField", true).toString()
        repository.load()
        repository.edit(guard) { it.upsertRecord("accounts", JSONObject().put("id", "cash").put("name", "Updated")) }
        assertTrue(JSONObject(storage.saved!!).getBoolean("remoteField"))
        val next = repository.load()
        val nextGuard = BudgetEditGuard.record(next.budget, next.identity, "accounts", "cash")
        storage.identity = "second-budget"
        repository.load()
        try { repository.edit(nextGuard) { it.put("name", "Wrong budget") }; fail("Expected switched budget conflict") }
        catch (expected: IllegalArgumentException) { assertTrue(expected.message!!.contains("open budget")) }
        assertEquals("Original", JSONObject(storage.saved!!).getString("name"))
    }

    @Test fun movingBankLinkRejectsChangedDestinationBeforeUnlinkingSource() = runTest {
        val original = JSONObject(BUDGET).put("accounts", JSONArray()
            .put(JSONObject().put("id", "source").put("simplefin", JSONObject().put("id", "bank")))
            .put(JSONObject().put("id", "destination").put("name", "Before")))
        val storage = MemoryStorage(original.toString())
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        val guard = BudgetEditGuard.recordSet(opened.budget, opened.identity, "accounts", listOf("source", "destination"))
        storage.saved = original.also { it.records("accounts").last().put("name", "Changed by sync") }.toString()
        repository.load()
        try {
            repository.edit(guard) { budget ->
                budget.upsertRecord("accounts", JSONObject().put("id", "source").put("simplefin", JSONObject.NULL))
                budget.upsertRecord("accounts", JSONObject().put("id", "destination").put("simplefin", JSONObject().put("id", "bank")))
            }
            fail("Expected a stale link destination conflict")
        } catch (_: IllegalArgumentException) {}
        assertEquals("bank", JSONObject(storage.saved!!).records("accounts").first().getJSONObject("simplefin").getString("id"))
        assertFalse(JSONObject(storage.saved!!).records("accounts").last().has("simplefin"))
    }

    @Test fun queuedImportAndRecoveryCannotReplaceNewlyOpenedIdenticalBudget() = runTest {
        val storage = MemoryStorage(BUDGET)
        storage.recovery = JSONObject(BUDGET).put("name", "Recovery").toString()
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        val guard = repository.replacementGuard(opened.budget, opened.identity)
        storage.identity = "second-budget"
        repository.load()
        try { repository.importBudget(JSONObject(BUDGET).put("name", "Imported").toString(), guard); fail("Expected import identity conflict") }
        catch (_: IllegalArgumentException) {}
        try { repository.restoreRecovery(guard); fail("Expected recovery identity conflict") }
        catch (_: IllegalArgumentException) {}
        assertEquals(BUDGET, storage.saved)
        assertEquals("Recovery", JSONObject(storage.recovery!!).getString("name"))
    }

    @Test fun replacementGuardPreservesPristineImportAndInvalidFileRecovery() = runTest {
        for (source in listOf(null, "{unreadable", JSONObject(BUDGET).put("invalid", true).toString())) {
            val storage = MemoryStorage(source)
            val repository = BudgetRepository(storage, engine, "2026-10")
            try { repository.load() } catch (_: Exception) {}
            val guard = repository.replacementGuard(null, null)
            repository.importBudget(BUDGET, guard)
            assertEquals("Original", JSONObject(storage.saved!!).getString("name"))
            assertEquals(source, storage.recovery)
        }
    }

    @Test fun replacementRejectsRefreshedBudgetEvenWhenCurrentIdentityIsUnchanged() = runTest {
        val storage = MemoryStorage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        val opened = repository.load()
        val guard = repository.replacementGuard(opened.budget, opened.identity)
        storage.saved = JSONObject(BUDGET).put("name", "Synced").toString()
        repository.load()
        try { repository.importBudget(BUDGET, guard); fail("Expected replacement revision conflict") } catch (_: IllegalArgumentException) {}
        assertEquals("Synced", JSONObject(storage.saved!!).getString("name"))
        assertNull(storage.recovery)
    }

    @Test fun guardHandlesReorderedJsonButRejectsNewRecordCollisionAndMonthNoteChanges() = runTest {
        val a = JSONObject(BUDGET).put("accounts", JSONArray("""[{"id":"cash","name":"Cash","value":1}]"""))
        val guard = BudgetEditGuard.record(a, null, "accounts", "cash")
        val reordered = JSONObject(a.toString()).put("accounts", JSONArray("""[{"value":1.0,"name":"Cash","id":"cash"}]"""))
        guard.verify(reordered, null)
        val absent = BudgetEditGuard.record(a, null, "accounts", "new")
        reordered.getJSONArray("accounts").put(JSONObject().put("id", "new"))
        try { absent.verify(reordered, null); fail("Expected ID collision conflict") } catch (_: IllegalArgumentException) {}
        a.put("ynabMonths", JSONArray().put(JSONObject().put("month", "2026-10").put("note", "Before")))
        val planning = BudgetEditGuard.command(a, null, "workflow", JSONObject().put("action", JSONObject().put("type", "setBudgetPlanning").put("month", "2026-10")))
        val later = JSONObject(a.toString())
        later.getJSONArray("ynabMonths").getJSONObject(0).put("note", "Remote")
        try { planning.verify(later, null); fail("Expected month note conflict") } catch (_: IllegalArgumentException) {}
    }

    @Test fun unchangedRefreshReusesPublishedObjectsAndDoesNotRecalculateReports() = runTest {
        val methods = mutableListOf<String>()
        val counted = BudgetEngine { request -> methods += JSONObject(request).getString("method"); engine.invoke(request) }
        val storage = MemoryStorage(BUDGET)
        val repository = BudgetRepository(storage, counted, "2026-10")
        val first = repository.load()
        repeat(5) {
            val refreshed = repository.load()
            assertSame(first.budget, refreshed.budget)
            assertSame(first.summary, refreshed.summary)
        }
        assertEquals(listOf("calculate"), methods)
        assertNull(first.reports)
        val nextMonth = repository.selectMonth("2026-11")
        assertSame(first.budget, nextMonth.budget)
        assertEquals(listOf("calculate", "calculate"), methods)
        repository.querySnapshot("reports", JSONObject().put("from", "2026-11").put("to", "2026-11"), nextMonth.budget)
        assertEquals("reports", methods.last())
    }

    @Test fun revisionAndLibraryChangesInvalidateCacheWithoutMutatingPriorSnapshot() = runTest {
        val storage = MemoryStorage(BUDGET)
        val repository = BudgetRepository(storage, engine, "2026-10")
        val first = repository.load()
        storage.saved = JSONObject(BUDGET).put("name", "Remote").toString()
        val remote = repository.load()
        assertNotSame(first.budget, remote.budget)
        assertEquals("Original", first.budget!!.getString("name"))
        assertEquals("Remote", remote.budget!!.getString("name"))
        storage.identity = "other-budget"
        val other = repository.load()
        assertNotSame(remote.budget, other.budget)
        assertEquals("other-budget", other.identity)
    }

    companion object {
        const val BUDGET = """{"schemaVersion":2,"name":"Original","currency":"USD","accounts":[],"categories":[],"assignments":[],"transactions":[]}"""
    }
}
