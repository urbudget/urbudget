package app.urbudget.nativeapp

import kotlinx.coroutines.test.runTest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.io.IOException

/** `command`, undo and every way a store edit can end, with the engine and the store scripted. */
class BudgetRepositoryStoreEditTest {
    /** The store's side of `editsInStore`: one budget, one token, and refusals on request. */
    private class FakeEditStorage(var stored: JSONObject?, val answer: (String, JSONObject) -> Any?) : BudgetStorage {
        override var identity: String? = "budget-1"
        override val managesHistory = true
        override var editsInStore = true
        override var canUndo = false
        override var canRedo = false
        /** What it last read or wrote, as the real one's is: another writer doesn't move it. */
        override val revision get() = "token-$known"
        var token = 1
        private var known = 1
        private var seen: Int? = null
        private var budgets = 1
        var recovery: String? = null

        var refuseEdits = 0
        var refuseWrites = 0
        var failEdit: String? = null
        var failOpens = 0
        /** Opens that fail from the next write on, so the load that follows a durable write fails and any before it don't. */
        var failOpensAfterWrite = 0
        var failComplete = false
        var completeIsNull = false
        var rawBytes: String? = null
        /** Replies with this and not what the edit did, as a patch the app can't apply. */
        var replyPatch: JSONObject? = null
        var replySummary: JSONObject? = JSONObject().put("from", "edit")
        var undoPatch: JSONObject? = null
        var undoSummary: JSONObject? = JSONObject().put("from", "undo")
        /** Something that happens just as the edit reaches the store. */
        var racing: (() -> Unit)? = null
        /** The same, for a question, after its revision was read. */
        var racingQuery: (() -> Unit)? = null

        val edits = mutableListOf<JSONArray>()
        /** Method, params and revision of each question, refused or not. */
        val queries = mutableListOf<Triple<String, JSONObject, String?>>()
        var undos = 0
        var opens = 0
        var writes = 0
        var completes = 0

        private fun conflict(): Nothing = throw IllegalStateException("CONFLICT: The budget changed. Reload it before saving your edit.")
        private fun commit(budget: JSONObject) { stored = budget; token++; seen = token; known = token }
        /** What was written is what's stored now, whatever could be read before. */
        private fun written(budget: JSONObject) { writes++; commit(budget); rawBytes = null; failOpens += failOpensAfterWrite; failOpensAfterWrite = 0 }

        override fun read() = stored?.toString()
        override fun open(): StoredBudget {
            opens++
            if (failOpens > 0) { failOpens--; throw IOException("Can't read") }
            known = token
            rawBytes?.let { seen = token; return StoredBudget.Text(it) }
            if (seen == token) return StoredBudget.Unchanged
            seen = token
            return stored?.let { StoredBudget.Parsed(JSONObject(it.toString())) } ?: StoredBudget.Text(null)
        }
        override fun invalidate() { seen = null }
        override fun complete(): String? {
            completes++
            if (failComplete) throw IOException("Can't read the store")
            return if (completeIsNull) null else stored?.toString()
        }
        override fun write(json: String) {
            if (refuseWrites > 0) { refuseWrites--; conflict() }
            if (seen != token) conflict()
            written(JSONObject(json))
        }
        override fun create(json: String) { identity = "budget-${++budgets}"; written(JSONObject(json)) }
        override fun retainRecovery(json: String) { recovery = json }
        override fun readRecovery() = recovery
        override fun hasRecovery() = recovery != null

        override fun edit(steps: JSONArray, month: String, asOfMonth: String): BudgetEdit {
            edits += steps
            racing?.invoke()
            racing = null
            if (refuseEdits > 0) { refuseEdits--; conflict() }
            if (seen != token) conflict()
            failEdit?.let { throw IllegalStateException(it) }
            var budget = stored!!
            var changed = JSONObject()
            for (i in 0 until steps.length()) {
                steps.getJSONObject(i).optJSONObject("patch")?.let { changed = it; budget = applyBudgetPatch(budget, it) }
                val step = steps.getJSONObject(i).optJSONObject("params") ?: continue
                changed = JSONObject()
                if (step.has("name")) changed.put("name", step.getString("name"))
                if (step.has("row")) changed.put("transactions", JSONObject().put("delete", JSONArray()).put("upsert", JSONArray().put(step.getJSONObject("row"))))
                budget = applyBudgetPatch(budget, changed)
            }
            if (changed.length() > 0) { commit(budget); canUndo = true }
            return BudgetEdit(replyPatch ?: changed, replySummary)
        }
        override fun undoEdit(month: String, asOfMonth: String): BudgetEdit {
            undos++
            if (seen != token) conflict()
            val patch = undoPatch ?: JSONObject()
            if (patch.length() > 0) commit(applyBudgetPatch(stored!!, patch))
            canRedo = true
            return BudgetEdit(patch, undoSummary)
        }

        override fun query(method: String, params: JSONObject, revision: String?): Any? {
            queries += Triple(method, params, revision)
            racingQuery?.invoke()
            if (revision != null && revision != "token-$token") throw StaleBudget("Moved on")
            return answer(method, params)
        }

        /** Another writer: sync, or a second connection. */
        fun external(budget: JSONObject?) { stored = budget; token++ }
        /** A write that moves the token and changes nothing. */
        fun touched() { stored = JSONObject(stored!!.toString()); token++ }
    }

    private val methods = mutableListOf<String>()
    /** The methods that were sent a whole budget. */
    private val budgets = mutableListOf<String>()
    private var failCalculate = false
    private val engine = BudgetEngine { text ->
        val request = JSONObject(text)
        val method = request.getString("method")
        methods += method
        if (request.has("budget")) budgets += method
        if (method == "calculate" && failCalculate) return@BudgetEngine JSONObject().put("ok", false).put("error", "Totals failed").toString()
        val value = when (method) {
            "defaultBudget", "sampleBudget" -> JSONObject(BUDGET).put("name", request.getJSONObject("params").getString("name"))
            else -> JSONObject().put("from", "calculate")
        }
        JSONObject().put("ok", true).put("value", value).toString()
    }

    @Before fun english() = EnglishWords.install()

    private val rename = JSONObject().put("name", "Renamed")
    private fun row(id: String, memo: String = "") = JSONObject().put("id", id).put("memo", memo)
    private fun addRow(id: String, memo: String = "") = JSONObject().put("row", row(id, memo))
    private fun budget(vararg rows: JSONObject) = JSONObject(BUDGET).put("transactions", JSONArray(rows.toList()))
    private fun BudgetSnapshot.ids() = budget!!.records("transactions").map { it.getString("id") }
    private fun BudgetSnapshot.name() = budget!!.getString("name")

    private fun BudgetSnapshot.memo() = budget!!.records("transactions")[0].getString("memo")
    private fun FakeEditStorage.totals() = queries.filter { it.first == "calculate" }

    private fun opened(initial: JSONObject = budget(row("t1"))): Pair<FakeEditStorage, BudgetRepository> {
        // The store's totals, counted with the stateless ones: a test asks how often, not how.
        val storage = FakeEditStorage(initial) { method, _ ->
            methods += method
            if (method == "calculate" && failCalculate) throw IllegalStateException("Totals failed")
            JSONObject().put("from", "calculate")
        }
        return storage to BudgetRepository(storage, engine, "2026-10")
    }

    @Test fun thePatchReachesThePublishedRootAndTheTokenMoves() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val edited = repository.command("rename", rename)
        assertEquals("Renamed", edited.name())
        assertEquals("Original", first.name())
        assertNotSame(first.budget, edited.budget)
        assertNotEquals(first.revision, edited.revision)
        assertEquals(storage.revision, edited.revision)
        assertEquals("edit", edited.summary!!.getString("from"))
        assertTrue(edited.undoAvailable)
        assertEquals(listOf("calculate"), methods)
    }

    @Test fun aLoadAfterAnEditReturnsTheEditedRoot() = runTest {
        val (_, repository) = opened()
        repository.load()
        val edited = repository.command("add", addRow("t2"))
        val loaded = repository.load()
        assertSame(edited.budget, loaded.budget)
        assertSame(edited.summary, loaded.summary)
        assertEquals(listOf("t1", "t2"), loaded.ids())
    }

    @Test fun identitySurvivesAnEditReply() = runTest {
        val (_, repository) = opened()
        val first = repository.load()
        assertEquals(first.identity, repository.command("rename", rename).identity)
        assertEquals(first.identity, repository.load().identity)
    }

    @Test fun anEmptyPatchKeepsTheRootAndSummary() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val same = repository.command("nothing", JSONObject())
        assertSame(first.budget, same.budget)
        assertSame(first.summary, same.summary)
        assertEquals(first.revision, same.revision)
        assertEquals(1, storage.edits.size)
    }

    @Test fun anEmptyPatchTakesTheEditsTotalsForAMonthItHadNone() = runTest {
        val (_, repository) = opened()
        val first = repository.load()
        val selected = repository.selectMonth("2026-11")
        assertSame(first.budget, selected.budget)
        methods.clear()
        val same = repository.command("nothing", JSONObject())
        assertSame(first.budget, same.budget)
        assertSame(selected.summary, same.summary)
        assertTrue(methods.isEmpty())
    }

    @Test fun aFailedEditPublishesNothingAndKeepsTheRoot() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        storage.failEdit = "Refused by the engine"
        try { repository.command("rename", rename); fail("Expected the engine's refusal") }
        catch (refused: IllegalStateException) {
            assertFalse(refused is BudgetConflict)
            assertEquals("Refused by the engine", refused.message)
        }
        val after = repository.load()
        assertSame(first.budget, after.budget)
        assertSame(first.summary, after.summary)
        assertEquals(1, storage.edits.size)
    }

    @Test fun aConflictReloadsVerifiesAndTriesOnceMore() = runTest {
        val (storage, repository) = opened(budget(row("t1"), row("t2")))
        val first = repository.load()
        val guard = BudgetEditGuard.record(first.budget, first.identity, "transactions", "t1")
        storage.external(budget(row("t1"), row("t2", "From sync")))
        val edited = repository.command("rename", rename, guard)
        assertEquals("Renamed", edited.name())
        assertEquals("From sync", edited.budget!!.records("transactions")[1].getString("memo"))
        assertEquals(2, storage.edits.size)
        assertEquals("Renamed", storage.stored!!.getString("name"))
        assertEquals(storage.revision, edited.revision)
    }

    @Test fun aSecondRefusalCarriesTheFreshBudget() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.external(budget(row("t1", "From sync")))
        storage.refuseEdits = 2
        try { repository.command("rename", rename); fail("Expected a conflict") } catch (conflict: BudgetConflict) {
            assertEquals("From sync", conflict.snapshot.budget!!.records("transactions")[0].getString("memo"))
            assertEquals("Original", conflict.snapshot.name())
            assertTrue(conflict.message!!.startsWith("CONFLICT:"))
            assertEquals(storage.revision, conflict.snapshot.revision)
        }
        assertEquals(2, storage.edits.size)
        assertEquals("Original", storage.stored!!.getString("name"))
    }

    @Test fun aStaleGuardAfterAReloadIsRefusedAndNothingIsResent() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val guard = BudgetEditGuard.record(first.budget, first.identity, "transactions", "t1")
        storage.external(budget(row("t1", "Changed by sync")))
        try { repository.command("rename", rename, guard); fail("Expected a stale guard") } catch (conflict: BudgetConflict) {
            assertEquals(Words[R.string.guard_values_changed], conflict.message)
            assertEquals("Changed by sync", conflict.snapshot.budget!!.records("transactions")[0].getString("memo"))
        }
        assertEquals("The first try, refused, and no second", 1, storage.edits.size)
    }

    @Test fun anIdentityMovedByTheReloadIsRefusedWithoutSendingAnything() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val guard = BudgetEditGuard.record(first.budget, first.identity, "transactions", "t1")
        storage.racing = { storage.external(budget(row("t1"))); storage.identity = "budget-2" }
        try { repository.command("rename", rename, guard); fail("Expected a switched budget") } catch (conflict: BudgetConflict) {
            assertEquals(Words[R.string.guard_budget_switched], conflict.message)
            assertEquals("budget-2", conflict.snapshot.identity)
        }
        assertEquals(1, storage.edits.size)
    }

    @Test fun nothingOpenAfterTheReloadIsAConflictThatShowsIt() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.external(null)
        try { repository.command("rename", rename); fail("Expected a conflict") } catch (conflict: BudgetConflict) {
            assertNull(conflict.snapshot.budget)
        }
    }

    @Test fun aPatchFailureAfterACommitReloadsAndDoesNotThrow() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.replyPatch = BrokenPatch()
        val shown = repository.command("rename", rename)
        assertEquals("Renamed", shown.name())
        assertEquals(storage.revision, shown.revision)
        assertEquals(1, storage.edits.size)
    }

    @Test fun aPatchFailureAndAFailedReloadAreSavedNotShownWithNoSnapshot() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        storage.replyPatch = BrokenPatch()
        storage.failOpens = 1
        try { repository.command("rename", rename); fail("Expected SavedNotShown") } catch (saved: SavedNotShown) { assertNull(saved.snapshot) }
        assertEquals(1, storage.edits.size)
        // The next load heals it, with what was saved.
        assertEquals("Renamed", repository.load().name())
        assertEquals("Original", first.name())
    }

    @Test fun aNullSummaryInAnUndoReplyRecalculates() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        repository.command("rename", rename)
        storage.undoPatch = JSONObject().put("name", "Original")
        storage.undoSummary = null
        methods.clear()
        val undone = repository.undo()
        assertEquals("Original", undone.name())
        assertEquals(listOf("calculate"), methods)
        assertEquals("calculate", undone.summary!!.getString("from"))
        assertNotSame(first.budget, undone.budget)
    }

    @Test fun totalsThatFailAfterAnUndoAreSavedNotShownWithTheNewRoot() = runTest {
        val (storage, repository) = opened()
        repository.load()
        val edited = repository.command("rename", rename)
        storage.undoPatch = JSONObject().put("name", "Original")
        storage.undoSummary = null
        failCalculate = true
        try { repository.undo(); fail("Expected SavedNotShown") } catch (saved: SavedNotShown) {
            assertEquals("Totals failed", saved.message)
            assertEquals("Original", saved.snapshot!!.name())
            assertNull("Never a summary from another revision", saved.snapshot!!.summary)
            assertEquals(storage.revision, saved.snapshot!!.revision)
        }
        assertEquals(1, storage.undos)
        failCalculate = false
        val healed = repository.load()
        assertEquals("Original", healed.name())
        assertNotNull(healed.summary)
        assertNotSame(edited.budget, healed.budget)
        assertEquals(1, storage.undos)
    }

    @Test fun aCommitOnTheSecondTryThatCantBeShownStaysSavedNotShown() = runTest {
        val (storage, _) = opened()
        storage.refuseEdits = 1
        storage.replyPatch = BrokenPatch()
        var opens = 0
        val failing = object : BudgetStorage by storage {
            override fun open(): StoredBudget { if (++opens > 1) throw IOException("Can't read"); return storage.open() }
        }
        val repository = BudgetRepository(failing, engine, "2026-10")
        repository.load()
        opens = 0
        // The refusal's reload is the first open, the one after the broken patch the second.
        val shown = runCatching { repository.command("rename", rename) }
        opens = 0
        assertEquals(SavedNotShown::class, shown.exceptionOrNull()!!::class)
        assertNull((shown.exceptionOrNull() as SavedNotShown).snapshot)
        assertEquals(2, storage.edits.size)
        assertEquals("Renamed", storage.stored!!.getString("name"))
    }

    @Test fun aTokenThatMovesWithNoChangeKeepsTheRoot() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        storage.touched()
        val second = repository.load()
        assertSame(first.budget, second.budget)
        assertSame(first.summary, second.summary)
        assertEquals(storage.revision, second.revision)
        assertNotEquals(first.revision, second.revision)
    }

    @Test fun anExportComesFromTheStore() = runTest {
        val (storage, repository) = opened()
        assertFalse(repository.hasExport())
        assertNull(repository.exportBudget())
        repository.load()
        repository.command("rename", rename)
        assertEquals(0, storage.completes)
        assertTrue(repository.hasExport())
        assertEquals("Renamed", JSONObject(repository.exportBudget()!!).getString("name"))
        assertEquals(1, storage.completes)
    }

    @Test fun bytesThatWontOpenSurviveAFailedOpen() = runTest {
        val (storage, repository) = opened()
        storage.rawBytes = "{bad json"
        try { repository.load(); fail("Expected a failed load") } catch (_: Exception) {}
        assertTrue(repository.hasExport())
        assertEquals("{bad json", repository.exportBudget())
        assertEquals(0, storage.completes)
        repository.importBudget(BUDGET, repository.replacementGuard(null, null))
        assertEquals("{bad json", storage.recovery)
        assertEquals("Original", JSONObject(storage.stored.toString()).getString("name"))
    }

    @Test fun textNeverOutlivesItsRootAfterAnImport() = runTest {
        val (storage, repository) = opened()
        repository.load()
        val x = budget(row("x1")).toString()
        val imported = repository.importBudget(x)
        val edited = repository.command("add", addRow("x2"))
        assertEquals(listOf("x1", "x2"), edited.ids())
        methods.clear()
        val again = repository.importBudget(x)
        assertEquals(listOf("x1"), again.ids())
        assertNotSame(edited.budget, again.budget)
        // One checks what's about to be written, one is the totals of what's stored after it.
        assertEquals("The edit's totals don't stand for this budget", listOf("calculate", "calculate"), methods)
        assertEquals(listOf("x1"), storage.stored!!.records("transactions").map { it.getString("id") })
        assertEquals(listOf("x1", "x3"), repository.command("add", addRow("x3")).ids())
        assertEquals(listOf("x1"), imported.ids())
    }

    @Test fun textNeverOutlivesItsRootAfterACreate() = runTest {
        val (storage, repository) = opened()
        repository.load()
        val first = repository.create("Same", "EUR", false)
        val edited = repository.command("add", addRow("t1"))
        val second = repository.create("Same", "EUR", false)
        assertEquals(emptyList<String>(), second.ids())
        assertNotEquals(first.identity, second.identity)
        assertEquals(storage.identity, second.identity)
        assertNotSame(edited.budget, second.budget)
        assertEquals(second.budget!!.toString(), repository.load().budget!!.toString())
    }

    @Test fun textNeverOutlivesItsRootAfterAReload() = runTest {
        val (storage, repository) = opened()
        repository.load()
        val j = budget(row("j1")).toString()
        repository.importBudget(j)
        storage.external(budget(row("k1")))
        assertEquals(listOf("k1"), repository.load().ids())
        methods.clear()
        val again = repository.importBudget(j)
        assertEquals(listOf("j1"), again.ids())
        assertEquals(listOf("j1"), storage.stored!!.records("transactions").map { it.getString("id") })
        assertEquals(listOf("calculate", "calculate"), methods)
    }

    @Test fun anEditBlockThatTakesBackACommandReachesRootAndStore() = runTest {
        val (storage, repository) = opened()
        repository.load()
        val text = repository.edit { it.put("name", "Whole") }
        val added = repository.command("add", addRow("t2"))
        assertEquals(listOf("t1", "t2"), added.ids())
        val removed = repository.edit { budget ->
            budget.put("transactions", JSONArray(budget.records("transactions").filter { it.getString("id") != "t2" }))
        }
        assertEquals(listOf("t1"), removed.ids())
        assertEquals(listOf("t1"), storage.stored!!.records("transactions").map { it.getString("id") })
        assertEquals(text.budget!!.toString(), removed.budget!!.toString())
    }

    @Test fun aNoOpEditBlockStraightAfterACommandKeepsTheRoot() = runTest {
        val (storage, repository) = opened()
        repository.load()
        val edited = repository.command("rename", rename)
        val writes = storage.writes
        methods.clear()
        val same = repository.edit { }
        assertSame(edited.budget, same.budget)
        assertSame(edited.summary, same.summary)
        assertTrue(methods.isEmpty())
        assertEquals(writes, storage.writes)
    }

    @Test fun anImportRefusedOnceIsWrittenAgainOnWhatsStored() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val guard = repository.replacementGuard(first.budget, first.identity)
        storage.touched()
        val imported = repository.importBudget(budget(row("x1")).toString(), guard)
        assertEquals(listOf("x1"), imported.ids())
        assertEquals(1, storage.writes)
    }

    @Test fun anImportRefusedTwiceShowsWhatsStored() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val guard = repository.replacementGuard(first.budget, first.identity)
        storage.refuseWrites = 2
        try { repository.importBudget(budget(row("x1")).toString(), guard); fail("Expected a conflict") } catch (conflict: BudgetConflict) {
            assertEquals(listOf("t1"), conflict.snapshot.ids())
        }
        assertEquals(0, storage.writes)
        assertEquals(listOf("t1"), storage.stored!!.records("transactions").map { it.getString("id") })
    }

    @Test fun anImportWhoseGuardNoLongerHoldsWritesNothing() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val guard = repository.replacementGuard(first.budget, first.identity)
        storage.external(budget(row("t1", "Changed by sync")))
        try { repository.importBudget(budget(row("x1")).toString(), guard); fail("Expected a conflict") } catch (conflict: BudgetConflict) {
            assertEquals(Words[R.string.guard_replacement_changed], conflict.message)
            assertEquals("Changed by sync", conflict.snapshot.budget!!.records("transactions")[0].getString("memo"))
        }
        assertEquals(0, storage.writes)
    }

    @Test fun aRestoreRefusedOnceIsWrittenAgain() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        storage.recovery = budget(row("r1")).toString()
        val guard = repository.replacementGuard(first.budget, first.identity)
        storage.touched()
        assertEquals(listOf("r1"), repository.restoreRecovery(guard).ids())
        assertEquals(1, storage.writes)
    }

    @Test fun aRestoreRefusedTwiceOrWithAStaleGuardWritesNothing() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        storage.recovery = budget(row("r1")).toString()
        val guard = repository.replacementGuard(first.budget, first.identity)
        storage.refuseWrites = 2
        try { repository.restoreRecovery(guard); fail("Expected a conflict") } catch (conflict: BudgetConflict) { assertEquals(listOf("t1"), conflict.snapshot.ids()) }
        storage.external(budget(row("t1", "Changed by sync")))
        try { repository.restoreRecovery(guard); fail("Expected a conflict") } catch (conflict: BudgetConflict) {
            assertEquals(Words[R.string.guard_replacement_changed], conflict.message)
        }
        assertEquals(0, storage.writes)
    }

    @Test fun anEditBlockRefusedTwiceShowsTheFreshBudget() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.external(budget(row("t1", "From sync")))
        storage.refuseEdits = 2
        try { repository.edit { it.put("name", "Stale") }; fail("Expected a conflict") } catch (conflict: BudgetConflict) {
            assertEquals("From sync", conflict.snapshot.budget!!.records("transactions")[0].getString("memo"))
            assertEquals("Original", conflict.snapshot.name())
        }
        assertEquals(0, storage.writes)
    }

    @Test fun anUndoRefusedWithAConflictShowsTheFreshBudget() = runTest {
        val (storage, repository) = opened()
        repository.load()
        repository.command("rename", rename)
        storage.external(budget(row("t1", "From sync")))
        try { repository.undo(); fail("Expected a conflict") } catch (conflict: BudgetConflict) {
            assertEquals("From sync", conflict.snapshot.budget!!.records("transactions")[0].getString("memo"))
        }
        assertEquals(1, storage.undos)
    }

    @Test fun aClosedStoreLeavesNothingToKeepAndCreateGoesAhead() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.completeIsNull = true
        val created = repository.create("Next", "EUR", false)
        assertEquals("Next", created.name())
        assertNull(storage.recovery)
        assertEquals(1, storage.writes)
    }

    @Test fun aCopyThatFailsStopsTheReplacement() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        storage.failComplete = true
        try { repository.create("Next", "EUR", false); fail("Expected the read to fail") } catch (_: IOException) {}
        assertEquals(0, storage.writes)
        assertEquals("budget-1", storage.identity)
        assertSame(first.budget, repository.load().budget)
    }

    @Test fun switchingBudgetsLeavesNoRootBehind() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val guard = BudgetEditGuard.record(first.budget, first.identity, "transactions", "t1")
        storage.external(JSONObject(storage.stored!!.toString()))
        storage.identity = "budget-2"
        val other = repository.load()
        assertNotSame(first.budget, other.budget)
        assertEquals("budget-2", other.identity)
        try { repository.command("rename", rename, guard); fail("Expected a switched budget") }
        catch (expected: IllegalArgumentException) { assertEquals(Words[R.string.guard_budget_switched], expected.message) }
        assertEquals(0, storage.edits.size)
    }

    @Test fun selectingAMonthKeepsTheRootAndAsksForItsTotals() = runTest {
        val (_, repository) = opened()
        val first = repository.load()
        methods.clear()
        val next = repository.selectMonth("2026-11")
        assertSame(first.budget, next.budget)
        assertEquals("2026-11", next.month)
        assertEquals(listOf("calculate"), methods)
    }

    /** Whatever `replace` does, the write landed and the load after it failed, so it's saved and not shown. */
    private fun savedNotShown(storage: FakeEditStorage, replace: suspend () -> BudgetSnapshot) = runTest {
        val writes = storage.writes
        storage.failOpensAfterWrite = 1
        try { replace(); fail("Expected SavedNotShown") } catch (saved: SavedNotShown) { assertNull(saved.snapshot) }
        assertEquals("Written once", writes + 1, storage.writes)
    }

    @Test fun aLoadThatFailsAfterACreateIsSavedAndDoesntCreateAgain() = runTest {
        val (storage, repository) = opened()
        repository.load()
        savedNotShown(storage) { repository.create("Next", "EUR", false) }
        assertEquals("budget-2", storage.identity)
        assertEquals("Next", repository.load().name())
        assertEquals("A load doesn't make it again", 1, storage.writes)
        assertEquals("budget-2", storage.identity)
    }

    @Test fun aLoadThatFailsAfterAnImportIsSaved() = runTest {
        val (storage, repository) = opened()
        repository.load()
        savedNotShown(storage) { repository.importBudget(budget(row("x1")).toString()) }
        assertEquals(listOf("x1"), repository.load().ids())
    }

    @Test fun aLoadThatFailsAfterARestoreIsSaved() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.recovery = budget(row("r1")).toString()
        savedNotShown(storage) { repository.restoreRecovery() }
        assertEquals(listOf("r1"), repository.load().ids())
    }

    @Test fun aRetriedImportWhoseLoadFailsIsStillSavedNotAConflict() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.refuseWrites = 1
        savedNotShown(storage) { repository.importBudget(budget(row("x1")).toString()) }
        assertEquals(listOf("x1"), repository.load().ids())
    }

    @Test fun selectingAMonthWithTheSwitchOffKeepsWhatALoadPublished() = runTest {
        val (storage, repository) = opened()
        storage.editsInStore = false
        val first = repository.load()
        val next = repository.selectMonth("2026-11")
        assertSame(first.budget, next.budget)
        assertEquals("2026-11", next.month)
    }

    @Test fun nothingIsHeldAsTextWhileABudgetIsOpen() = runTest {
        val (_, repository) = opened()
        fun current() = BudgetRepository::class.java.getDeclaredField("current").apply { isAccessible = true }.get(repository)
        repository.load()
        assertNull(current())
        repository.command("rename", rename)
        assertNull(current())
        repository.edit { it.put("name", "Whole") }
        assertNull(current())
        repository.importBudget(budget(row("x1")).toString())
        assertNull(current())
        repository.undo()
        assertNull(current())
    }

    @Test fun aNoOpTransformMakesNoStorageCall() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val opens = storage.opens
        methods.clear()
        val same = repository.edit { }
        // A rebuilt array with the same rows is no change either.
        val rebuilt = repository.edit { it.put("transactions", JSONArray(it.records("transactions"))) }
        assertSame(first.budget, same.budget)
        assertSame(first.budget, rebuilt.budget)
        assertSame(first.summary, rebuilt.summary)
        assertEquals(first.revision, rebuilt.revision)
        assertEquals(0, storage.edits.size)
        assertEquals(opens, storage.opens)
        assertEquals(0, storage.writes)
        assertTrue(methods.isEmpty())
    }

    @Test fun anEditBlockGoesToTheStoreAsAPatchOfWhatChanged() = runTest {
        val (storage, repository) = opened(budget(row("t1"), row("t2")))
        val first = repository.load()
        val edited = repository.edit { it.upsertRecord("transactions", row("t2", "Edited")) }
        assertEquals(0, storage.writes)
        val patch = storage.edits.single().getJSONObject(0).getJSONObject("patch")
        val upsert = patch.getJSONObject("transactions").getJSONArray("upsert")
        assertEquals(1, upsert.length())
        assertEquals("t2", upsert.getJSONObject(0).getString("id"))
        assertSame("A row the edit left alone stays the same object", first.budget!!.records("transactions")[0], edited.budget!!.records("transactions")[0])
        assertEquals("Edited", edited.budget!!.records("transactions")[1].getString("memo"))
        assertEquals("", first.budget!!.records("transactions")[1].getString("memo"))
    }

    @Test fun aRefusedEditBlockIsRebuiltFromWhatsStored() = runTest {
        fun schedules(vararg ids: String) = JSONArray(ids.map { JSONObject().put("id", it) })
        fun JSONObject.scheduleIds() = records("schedules").map { it.getString("id") }
        val (storage, repository) = opened(budget(row("t1")).put("schedules", schedules("mine")))
        repository.load()
        storage.external(budget(row("t1")).put("schedules", schedules("mine", "from sync")))
        val edited = repository.edit { it.upsertRecord("schedules", JSONObject().put("id", "added")) }
        assertEquals(listOf("mine", "from sync", "added"), edited.budget!!.scheduleIds())
        assertEquals(listOf("mine", "from sync", "added"), storage.stored!!.scheduleIds())
        assertEquals(2, storage.edits.size)
        val sent = storage.edits.map { it.getJSONObject(0).getJSONObject("patch").getJSONObject("extras").getJSONObject("set").records("schedules").size }
        assertEquals("Never the first patch again: it would drop sync's schedule", listOf(2, 3), sent)
    }

    @Test fun aTransformThatFailsOnTheFreshBudgetIsAConflict() = runTest {
        val (storage, repository) = opened(budget(row("t1")))
        repository.load()
        storage.external(budget(row("t1", "From sync")))
        var runs = 0
        try {
            repository.edit { if (++runs == 2) error("Not on this budget") else it.put("name", "Stale") }
            fail("Expected a conflict")
        } catch (conflict: BudgetConflict) {
            assertEquals("Not on this budget", conflict.message)
            assertEquals("From sync", conflict.snapshot.memo())
        }
        assertEquals(1, storage.edits.size)
    }

    @Test fun anEditAndCommandsAreOneEditOfSteps() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val saved = repository.editThenCommands(null, { it.put("name", "Edited") }, listOf("add" to addRow("t2")))
        val steps = storage.edits.single()
        assertEquals(2, steps.length())
        assertEquals("Edited", steps.getJSONObject(0).getJSONObject("patch").getString("name"))
        assertEquals("add", steps.getJSONObject(1).getString("method"))
        assertEquals(0, storage.writes)
        assertNotSame(first.budget, saved.budget)
        // Nothing for the transform to say: only the commands go.
        repository.editThenCommands(null, { }, listOf("add" to addRow("t3")))
        assertEquals(1, storage.edits.last().length())
        assertEquals("add", storage.edits.last().getJSONObject(0).getString("method"))
        // Nothing at all: nothing goes.
        val same = repository.editThenCommands(null, { }, emptyList())
        assertEquals(2, storage.edits.size)
        assertEquals(storage.revision, same.revision)
    }

    @Test fun aQuestionGoesToTheStoreWithNoBudget() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        methods.clear()
        val params = JSONObject().put("fromMonth", "2026-01")
        assertEquals("calculate", (repository.querySnapshot("reports", params, first.budget, first.revision) as JSONObject).getString("from"))
        val (method, sent, revision) = storage.queries.last()
        assertEquals("reports", method)
        assertSame(params, sent)
        assertEquals(first.revision, revision)
        assertEquals("Only the store was asked", listOf("reports"), methods)
    }

    @Test fun aMethodThatNeedsNoBudgetStaysStateless() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        val before = storage.queries.size
        for (method in listOf("loanProjection", "loanPaymentByDate", "importYnabFiles")) repository.querySnapshot(method, JSONObject(), first.budget, first.revision)
        assertEquals(before, storage.queries.size)
        assertEquals(listOf("loanProjection", "loanPaymentByDate", "importYnabFiles"), methods.takeLast(3))
    }

    @Test fun anOutOfDateQuestionIsRefusedAndAnAnyRevisionOneIsNot() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        storage.external(budget(row("t1", "From sync")))
        try { repository.querySnapshot("reports", JSONObject(), first.budget, first.revision); fail("Expected StaleBudget") } catch (_: StaleBudget) {}
        assertNotNull(repository.querySnapshot("bankApplyFixes", JSONObject(), first.budget, null))
    }

    @Test fun bytesTheEngineRefusesAreNeverAskedOfTheStore() = runTest {
        val (storage, repository) = opened()
        val bytes = JSONObject().put("schemaVersion", 2).put("name", "Won't open").toString()
        storage.rawBytes = bytes
        failCalculate = true
        try { repository.load(); fail("Expected the engine's refusal") } catch (refused: IllegalArgumentException) {
            assertEquals("Totals failed", refused.message)
        }
        assertTrue(storage.queries.isEmpty())
        assertEquals("`snapshot`'s check of what isn't in the store", listOf("calculate"), budgets)
        assertNull(BudgetRepository::class.java.getDeclaredField("root").apply { isAccessible = true }.get(repository))
        assertTrue(repository.hasExport())
        assertEquals(bytes, repository.exportBudget())
        assertEquals(0, storage.completes)
        // A month changes nothing about that.
        try { repository.selectMonth("2026-11"); fail("Expected the engine's refusal") } catch (_: IllegalArgumentException) {}
        assertTrue(storage.queries.isEmpty())
    }

    @Test fun aMonthChosenAfterTheStoreMovedPublishesThatRevision() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.external(budget(row("t1", "From sync")))
        val shown = repository.selectMonth("2026-11")
        assertEquals("From sync", shown.memo())
        assertEquals(storage.revision, shown.revision)
        assertEquals("2026-11", shown.month)
        val (_, params, revision) = storage.totals().last()
        assertEquals("2026-11", params.getString("month"))
        assertEquals(storage.revision, revision)
        assertEquals("calculate", shown.summary!!.getString("from"))
    }

    @Test fun aLoadTheStoreOutrunsGoesAgainForOnePair() = runTest {
        val (storage, repository) = opened()
        storage.racingQuery = { storage.racingQuery = null; storage.external(budget(row("t1", "From sync"))) }
        val shown = repository.load()
        assertEquals("From sync", shown.memo())
        assertEquals(storage.revision, shown.revision)
        assertEquals(storage.revision, storage.totals().last().third)
        assertEquals(2, storage.opens)
        assertEquals(2, storage.totals().size)
    }

    @Test fun aMonthWhoseTotalsFailShowsTheErrorAndStays() = runTest {
        val (_, repository) = opened()
        val first = repository.load()
        failCalculate = true
        try { repository.selectMonth("2026-11"); fail("Expected the failure") } catch (failed: IllegalStateException) { assertEquals("Totals failed", failed.message) }
        failCalculate = false
        val after = repository.load()
        assertEquals("2026-10", after.month)
        assertSame(first.budget, after.budget)
        assertSame(first.summary, after.summary)
    }

    @Test fun aMonthChosenWhileTheStoreKeepsMovingGivesBackTheOneBefore() = runTest {
        val (storage, repository) = opened()
        repository.load()
        storage.racingQuery = { storage.touched() }
        try { repository.selectMonth("2026-11"); fail("Expected StaleBudget") } catch (_: StaleBudget) {}
        storage.racingQuery = null
        assertEquals("2026-10", repository.load().month)
    }

    @Test fun aReloadGivesUpAfterThreeTries() = runTest {
        val (storage, repository) = opened()
        storage.racingQuery = { storage.touched() }
        try { repository.load(); fail("Expected StaleBudget") } catch (_: StaleBudget) {}
        assertEquals(3, storage.opens)
        assertEquals(3, storage.totals().size)
        // Nothing's wrong once it stops.
        storage.racingQuery = null
        val healed = repository.load()
        assertEquals(storage.revision, healed.revision)
    }

    @Test fun anUndoWithNoTotalsOnAStoreThatMovedShowsTheFreshBudget() = runTest {
        val (storage, repository) = opened()
        repository.load()
        repository.command("rename", rename)
        storage.undoPatch = JSONObject().put("name", "Original")
        storage.undoSummary = null
        storage.racingQuery = { storage.racingQuery = null; storage.external(budget(row("t1", "From sync"))) }
        val shown = repository.undo()
        assertEquals("From sync", shown.memo())
        assertEquals(storage.revision, shown.revision)
        assertNotNull(shown.summary)
        assertEquals(1, storage.undos)
    }

    @Test fun noBudgetCrossesExceptOneNotYetStored() = runTest {
        val (storage, repository) = opened()
        val first = repository.load()
        repository.command("rename", rename)
        repository.edit { it.put("name", "A block") }
        repository.editThenCommands(null, { it.put("name", "Both") }, listOf("add" to addRow("t2")))
        repository.selectMonth("2026-11")
        repository.querySnapshot("reports", JSONObject(), first.budget, null)
        storage.undoPatch = JSONObject().put("name", "Undone")
        storage.undoSummary = null
        repository.undo()
        repository.load()
        assertEquals(emptyList<String>(), budgets)
        repository.importBudget(budget(row("x1")).toString())
        assertEquals("`persist`'s check, before anything is written", listOf("calculate"), budgets)
    }

    /** Applying this throws, as a patch nothing in it can be read from would. */
    private class BrokenPatch : JSONObject() {
        override fun length() = 1
        override fun has(key: String): Boolean = throw IllegalStateException("Broken patch")
    }

    private companion object {
        val BUDGET: String = JSONObject("""{"schemaVersion":2,"name":"Original","currency":"USD","accounts":[],"categories":[],"assignments":[],"transactions":[]}""").toString()
    }
}
