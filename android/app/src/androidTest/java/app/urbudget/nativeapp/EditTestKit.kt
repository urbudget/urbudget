package app.urbudget.nativeapp

import android.app.Application
import android.database.sqlite.SQLiteDatabase
import android.os.Build
import androidx.lifecycle.ViewModelStore
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.time.YearMonth
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

private val strictRun get() = InstrumentationRegistry.getArguments().getString("strict") == "1"
/** `-e quick 1`: the same checks on small budgets, for a run during development. */
private val quickRun get() = InstrumentationRegistry.getArguments().getString("quick") == "1"

/** A budget's transaction count, cut down under `-e quick 1`. */
fun sized(transactions: Int) = if (quickRun) minOf(transactions, 300) else transactions

/** Only on an isolated emulator: these tests make budgets and open them. Under `-e strict 1` a phone is a failure, not a skip. */
fun requireEmulator() {
    val emulator = Build.HARDWARE.contains("ranchu") || Build.HARDWARE.contains("goldfish")
    if (strictRun) assertTrue("These checks run only on an emulator", emulator)
    else assumeTrue("Use an isolated emulator", emulator)
}

/** After everything queued so far. */
fun BudgetViewModel.settle() {
    val done = CountDownLatch(1)
    afterPending { done.countDown() }
    assertTrue("The queue didn't finish in time", done.await(60, TimeUnit.SECONDS))
}

/** `vm.query`, waited for. Fails on `onError`. */
fun BudgetViewModel.asked(method: String, params: JSONObject = JSONObject(), anyRevision: Boolean = false): JSONObject {
    val done = CountDownLatch(1)
    var reply: JSONObject? = null
    var failure: String? = null
    query(method, params, onError = { failure = it; done.countDown() }, anyRevision = anyRevision) { reply = it; done.countDown() }
    assertTrue("$method didn't answer in time", done.await(60, TimeUnit.SECONDS))
    assertNull("$method: $failure", failure)
    return reply!!
}

/** A method on a whole budget, the way the app called it before `edit_budget`. */
fun stateless(method: String, budget: JSONObject?, params: JSONObject = JSONObject()): JSONObject {
    val request = JSONObject().put("version", 1).put("method", method).put("params", params)
    if (budget != null) request.put("budget", budget)
    val response = JSONObject(NativeBridge.invoke(request.toString()))
    assertTrue(response.optString("error"), response.getBoolean("ok"))
    return response.getJSONObject("value")
}

/** The budget a changing method's reply holds. */
fun changedBudget(reply: JSONObject): JSONObject = reply.optJSONObject("budget") ?: reply

object Canon {
    /** Assignments as the store lists them. Nothing else may differ in order. */
    fun storeOrder(b: JSONObject): JSONObject {
        val copy = b.shallowCopy()
        val rows = b.rows("assignments").sortedWith(compareBy({ it.text("month") }, { it.text("categoryId") }))
        return copy.put("assignments", JSONArray(rows))
    }

    /**
     * What's the same when a command ran twice at different times. Only for comparing with the
     * stateless path: the app's copy and the store must match exactly, times and all.
     */
    fun forStateless(b: JSONObject): JSONObject {
        val copy = JSONObject(storeOrder(b).toString())
        copy.optJSONArray("moneyMoves")?.let { moves -> (0 until moves.length()).forEach { moves.getJSONObject(it).remove("at") } }
        for (t in copy.rows("transactions")) {
            t.remove("createdAt")
            t.put("id", COPY.replace(t.text("id"), "copy-$1"))
        }
        for (s in copy.rows("schedules")) s.optJSONObject("transaction")?.remove("createdAt")
        return copy
    }
    private val COPY = Regex("^copy-\\d+-(\\d+)$")

    /** What the app holds of one transaction, as `store::hide` makes it. */
    fun windowRow(t: JSONObject): JSONObject {
        val row = t.shallowCopy()
        val imported = t.has("source")
        row.remove("hasImportSource")
        row.remove("source")
        return if (imported) row.put("hasImportSource", true) else row
    }

    /** What the app holds of a whole budget, as `store::for_window` makes it. Only for a whole one: on a light copy it would drop the hint. */
    fun windowOf(full: JSONObject): JSONObject {
        val copy = full.shallowCopy()
        copy.remove("sourceExport")
        return copy.put("transactions", JSONArray(full.rows("transactions").map(::windowRow)))
    }

    /** A row of the app's copy as the store takes it: `source` back from `whole`, the hint gone. Only for comparing with a stateless call. */
    fun restored(record: JSONObject, whole: JSONObject): JSONObject {
        val row = JSONObject(record.toString())
        row.remove("hasImportSource")
        whole.rows("transactions").firstOrNull { it.text("id") == row.text("id") }?.opt("source")?.let { row.put("source", it) }
        return row
    }

    /** Sorted keys, so two copies of a budget read the same whatever built them. */
    fun text(value: Any?): String = when (value) {
        is JSONObject -> value.keys().asSequence().toList().sorted().joinToString(prefix = "{", postfix = "}") { JSONObject.quote(it) + ":" + text(value.get(it)) }
        is JSONArray -> (0 until value.length()).joinToString(prefix = "[", postfix = "]") { text(value.get(it)) }
        is String -> JSONObject.quote(value)
        else -> value.toString()
    }

    fun digest(b: JSONObject): String =
        MessageDigest.getInstance("SHA-256").digest(text(storeOrder(b)).toByteArray()).joinToString("") { "%02x".format(it) }
}

/** The first place two values differ, as a path like `transactions[id=t-01234].splits[0].amount`. */
fun firstDifference(a: Any?, b: Any?, path: String = "budget"): String? = when {
    a is JSONObject && b is JSONObject -> (a.keys().asSequence() + b.keys().asSequence()).distinct().firstNotNullOfOrNull { key ->
        if (!a.has(key)) "$path.$key: only on the right, ${b.get(key).toString().take(200)}"
        else if (!b.has(key)) "$path.$key: only on the left, ${a.get(key).toString().take(200)}"
        else firstDifference(a.get(key), b.get(key), "$path.$key")
    }
    a is JSONArray && b is JSONArray -> (0 until minOf(a.length(), b.length())).firstNotNullOfOrNull { i ->
        val id = (a.opt(i) as? JSONObject)?.optString("id").orEmpty()
        firstDifference(a.get(i), b.get(i), if (id.isEmpty()) "$path[$i]" else "$path[id=$id]")
    } ?: if (a.length() != b.length()) "$path: ${a.length()} and ${b.length()} items" else null
    Canon.text(a) == Canon.text(b) -> null
    else -> "$path: $a and $b"
}

fun assertSameJson(what: String, expected: Any?, actual: Any?) {
    firstDifference(expected, actual)?.let { throw AssertionError("$what: $it") }
}

/** Budgets the tests make, all local-only and all sent to the Trash afterwards, and the view models that show them. */
class EditSession(services: Boolean = true) {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    val context: android.content.Context get() = instrumentation.targetContext
    private val store = ViewModelStore()
    private val budgets = mutableListOf<String>()
    private var models = 0

    init {
        // Before anything else: on a phone, this app's files are someone's real budget.
        requireEmulator()
        Words.source = ResourceWords(context.resources)
        if (services) {
            NativeServices.initialize(context)
            // A copy left by an earlier run would be offered as this one's.
            File(context.filesDir, "budget-before-replacement.json").delete()
        }
    }

    /** Made through the service, so the app's first copy is what the store really holds. */
    fun make(budget: JSONObject, open: Boolean = true): String {
        val id = NativeServices.call("create_budget", JSONObject().put("budget", budget).put("localOnly", true)) as String
        budgets += id
        if (open) NativeServices.call("library_open", JSONObject().put("id", id))
        return id
    }
    fun track(id: String) { budgets += id }

    fun model(): BudgetViewModel {
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync {
            vm = BudgetViewModel(context.applicationContext as Application)
            store.put("model-${models++}", vm)
        }
        vm.settle()
        return vm
    }
    fun scripted(repository: BudgetRepository): BudgetViewModel {
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync {
            vm = BudgetViewModel(context.applicationContext as Application, repository)
            store.put("model-${models++}", vm)
        }
        vm.settle()
        return vm
    }
    fun onMain(action: () -> Unit) = instrumentation.runOnMainSync(action)

    fun close() {
        instrumentation.runOnMainSync { store.clear() }
        for (id in budgets) runCatching { NativeServices.call("library_trash", JSONObject().put("id", id)) }
    }
}

/**
 * One edit at a time, with everything that should hold after it checked. Runs `strict`, which reads
 * the budget back through `load_budget` after each step, and quiet, which doesn't: `load_budget` and
 * `vm.refresh()` both repair the drift this is here to find. `repository.load()` is no oracle either:
 * on a matching token it returns its own copy.
 */
class EditDriver(private val vm: BudgetViewModel, val strict: Boolean, private val keep: Int = 1) {
    /** What's known to have happened, which decides how much of check 8 can be asked. */
    enum class Change {
        /** A patch on the app's copy, so what it didn't touch is the same object. */
        Edit,
        /** Saved, with every row new: a whole budget was written, or read back on the way. */
        Rebuilt,
        /** An undo or a redo. What's left of either is the test's to say. */
        History,
        /** Another budget, or a write from outside the app. Only that the budget is a new one. */
        Replaced,
        /** Saved, but only what the app's copy doesn't hold, a heavy extra: the token moves and the root stays. */
        Hidden,
        /** Same root, same token, same undo. */
        Nothing,
    }

    /** What the checks compare with, taken before the work. */
    class Mark(val root: JSONObject, val text: String, val token: String, val epoch: Long, val saved: Int, val undo: Boolean)

    private val events = AtomicInteger()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    /** A digest for every step and a full copy only of the last: the test shares the app's 192 MB heap. */
    val digests = mutableListOf<Pair<String, String>>()
    val copies = ArrayDeque<Pair<String, JSONObject>>()

    init { scope.launch(start = CoroutineStart.UNDISPATCHED) { NativeServices.saved.collect { events.incrementAndGet() } } }
    fun close() = scope.cancel()

    /** Check 10 for a quiet script, as its last look. Not sooner: a full read repairs what's being looked for. */
    fun end(name: String = "the end") = try { loadsBack(name) } finally { close() }

    private fun status() = NativeServices.call("native_budget_status") as JSONObject
    private fun budget() = vm.state.value.budget!!

    /** `method` through `vm.runCommand`, checked against the same method on the whole budget. */
    fun command(name: String, method: String, params: JSONObject, changes: Boolean = true) {
        val expected = changedBudget(stateless(method, storedBudget(), params))
        step(name, expected, changes) { vm.runCommand(method, params) }
    }

    fun workflow(name: String, action: JSONObject, changes: Boolean = true) = command(name, "workflow", JSONObject().put("action", action), changes)

    /**
     * Runs `run`, waits for the queue and checks. `expected`: the whole budget the stateless path
     * makes, null where there's no such thing.
     */
    fun step(name: String, expected: JSONObject?, changes: Boolean = true, run: () -> Unit) {
        val before = mark()
        run()
        vm.settle()
        check(name, before, expected, if (changes) Change.Edit else Change.Nothing)
    }

    fun mark(): Mark {
        val root = vm.state.value.budget!!
        return Mark(root, root.toString(), status().getString("token"), vm.operationFailureEpoch, events.get(), vm.state.value.undoAvailable)
    }

    /** Everything that should hold now, for work done since `before`. The queue must be idle. */
    fun check(name: String, before: Mark, expected: JSONObject? = null, change: Change = Change.Edit) {
        val state = vm.state.value
        // 1
        assertNull("$name: error", state.error)
        assertEquals("$name: a failure was recorded", before.epoch, vm.operationFailureEpoch)
        // 2
        assertEquals("$name: the published snapshot before it changed", before.text, before.root.toString())
        // 3, 4 and 6
        val stored = verify(name)
        // 5
        if (expected != null) assertSameJson("$name: stateless", Canon.forStateless(expected), Canon.forStateless(stored))
        // 7 is in verify. 8
        val token = status().getString("token")
        if (change == Change.Nothing) {
            assertEquals("$name: token", before.token, token)
            assertSame("$name: root", before.root, state.budget)
            assertEquals("$name: undo", before.undo, state.undoAvailable)
        } else {
            assertNotEquals("$name: token", before.token, token)
            if (change == Change.Hidden) assertSame("$name: root", before.root, state.budget)
            else assertNotSame("$name: root", before.root, state.budget)
            if (change == Change.Edit || change == Change.History) sampledRows(name, before.root, state.budget!!)
            if (change == Change.Edit || change == Change.Rebuilt) {
                assertTrue("$name: undo", state.undoAvailable)
                assertFalse("$name: redo", state.redoAvailable)
            }
            if (change != Change.Replaced) waitForSaved(name, before.saved)
        }
        // 9
        assertFalse("$name: recovery.json", File(File(state.budgetIdentity!!).parentFile, "recovery.json").exists())
        // 10
        if (strict) loadsBack(name)
        digests += name to Canon.digest(stored)
        copies.addLast(name to stored)
        while (copies.size > keep) copies.removeFirst()
    }

    /** A full read says what the app's copy says, undo and redo included. */
    fun loadsBack(name: String) {
        val state = vm.state.value
        val loaded = NativeServices.call("load_budget", JSONObject().put("light", true)) as JSONObject
        assertSameJson("$name: loaded", Canon.storeOrder(state.budget!!), Canon.storeOrder(loaded.getJSONObject("budget")))
        assertEquals("$name: undo after a load", state.undoAvailable, loaded.getBoolean("undoAvailable"))
        assertEquals("$name: redo after a load", state.redoAvailable, loaded.getBoolean("redoAvailable"))
    }

    /** Checks 3 and 7: the app's copy is the draft as a window sees it, at the store's revision. Returns the whole draft. */
    fun onTheDraft(name: String): JSONObject {
        val state = vm.state.value
        val draft = NativeServices.call("snapshot_budget") as JSONObject
        assertSameJson("$name: app and draft", Canon.storeOrder(state.budget!!), Canon.storeOrder(Canon.windowOf(draft)))
        assertEquals("$name: revision", status().getString("token"), state.budgetRevision)
        return draft
    }

    /** App, draft and rows agree, and the totals and revision are theirs. Returns the stored budget. */
    fun verify(name: String): JSONObject {
        val state = vm.state.value
        val draft = onTheDraft(name)
        val answer = NativeServices.call("saved_budget") as JSONObject
        val stored = answer.getJSONObject("budget")
        assertSameJson("$name: draft and rows", Canon.storeOrder(draft), Canon.storeOrder(stored))
        assertFalse("$name: unsaved draft", answer.getBoolean("unsavedDraft"))
        val month = state.month
        val totals = stateless("calculate", stored, JSONObject().put("month", month).put("asOfMonth", YearMonth.now().toString()))
        assertSameJson("$name: totals", totals, state.summary)
        return stored
    }

    /** A patch mustn't rebuild what it doesn't touch: screens compare rows by identity to skip work. */
    private fun sampledRows(name: String, before: JSONObject, after: JSONObject) {
        val rows = after.rows("transactions").associateBy { it.text("id") }
        val kept = before.rows("transactions").filterIndexed { i, _ -> i % 37 == 0 }
        for (row in kept) {
            val now = rows[row.text("id")] ?: continue
            if (now.toString() == row.toString()) assertSame("$name: row ${row.text("id")} was rebuilt", row, now)
        }
    }

    private fun waitForSaved(name: String, saved: Int) {
        val until = System.currentTimeMillis() + 5_000
        while (events.get() == saved) {
            assertTrue("$name: NativeServices.saved didn't fire", System.currentTimeMillis() < until)
            Thread.sleep(10)
        }
    }
}

/**
 * A storage with `editsInStore`, for what can't be arranged through the real one. It holds a budget,
 * counts calls, and any of `edit`, `undoEdit`, `redoEdit`, `open`, `complete` and `query:<method>` can
 * be told to refuse or fail a given number of times. Needs no `NativeServices`.
 */
class ScriptedEditStorage(var stored: JSONObject) : BudgetStorage {
    enum class Fault { Conflict, Stale, Failure }
    private val faults = HashMap<String, ArrayDeque<Fault>>()
    private val counts = HashMap<String, Int>()
    private var token = 1
    private var known = 1
    private var seen: Int? = null

    override var identity: String? = "scripted"
    /** What it last read or wrote, as the real one's is: another writer doesn't move it. */
    override val revision get() = "scripted:$known"
    override val managesHistory = true
    override val editsInStore = true
    override var canUndo = false
    override var canRedo = false

    /** What an edit changes. */
    var editPatch: (JSONArray) -> JSONObject = { JSONObject() }
    /** What an undo changes, and the totals it replies with. */
    var undoPatch: JSONObject = JSONObject()
    var undoSummary: JSONObject? = JSONObject()
    /** Replied in place of what the edit really changed, as a patch the app can't apply. */
    var brokenReply: JSONObject? = null
    /** The store's answer to a question. */
    var answer: (String, JSONObject) -> Any? = { _, _ -> JSONObject().put("readyToAssign", 0) }
    /** Holds `edit` and `undoEdit` back. This store is so quick that the queue can finish one inside the call that sent it. */
    @Volatile var gate: CountDownLatch? = null

    /** Calls run on the view model's queue and its query workers at once. */
    @Synchronized fun count(method: String) = counts[method] ?: 0
    @Synchronized fun fail(method: String, fault: Fault, times: Int = 1) { faults.getOrPut(method) { ArrayDeque() }.apply { repeat(times) { addLast(fault) } } }
    /** Another writer. */
    @Synchronized fun external(budget: JSONObject) { stored = budget; token++ }

    @Synchronized private fun called(method: String) {
        counts[method] = count(method) + 1
        when (faults[method]?.removeFirstOrNull()) {
            Fault.Conflict -> throw IllegalStateException("CONFLICT: The budget changed. Reload it before saving your edit.")
            Fault.Stale -> throw StaleBudget(Words[R.string.model_changed_preview])
            Fault.Failure -> throw IOException("Scripted failure of $method")
            null -> {}
        }
    }
    private fun commit(patch: JSONObject) { if (patch.length() > 0) { stored = applyBudgetPatch(stored, patch); token++; seen = token; known = token } }

    override fun read() = stored.toString()
    override fun write(json: String) { stored = JSONObject(json); token++; seen = token; known = token }
    override fun open(): StoredBudget {
        called("open")
        known = token
        if (seen == token) return StoredBudget.Unchanged
        seen = token
        return StoredBudget.Parsed(JSONObject(stored.toString()))
    }
    override fun invalidate() { seen = null }
    override fun complete(): String? { called("complete"); return stored.toString() }
    override fun edit(steps: JSONArray, month: String, asOfMonth: String): BudgetEdit {
        gate?.await()
        called("edit")
        if (seen != token) throw IllegalStateException("CONFLICT: The budget changed. Reload it before saving your edit.")
        val patch = editPatch(steps)
        commit(patch)
        canUndo = true
        return BudgetEdit(brokenReply ?: patch, JSONObject())
    }
    override fun undoEdit(month: String, asOfMonth: String): BudgetEdit {
        gate?.await()
        called("undoEdit")
        commit(undoPatch)
        canRedo = true
        return BudgetEdit(undoPatch, undoSummary)
    }
    override fun redoEdit(month: String, asOfMonth: String): BudgetEdit {
        called("redoEdit")
        return BudgetEdit(JSONObject(), JSONObject())
    }
    override fun query(method: String, params: JSONObject, revision: String?): Any? {
        called("query:$method")
        if (revision != null && revision != "scripted:$token") throw StaleBudget(Words[R.string.model_changed_preview])
        return answer(method, params)
    }
}

/** Actions the checks send, as the screens build them. */
object Edits {
    fun bulk(ids: List<String>, operation: String, value: Any? = null) = JSONObject().put("action", JSONObject().put("type", "bulk").put("ids", JSONArray(ids))
        .put("operation", operation).put("value", value ?: JSONObject.NULL).put("accountId", "all"))
    fun assign(month: String, category: String, amount: Long) = JSONObject().put("action", JSONObject().put("type", "assignAmount").put("month", month).put("categoryId", category).put("amount", amount))
    fun transaction(id: String, date: String, amount: Long = -1_250, category: String? = "c-fun-1", payee: String = "Added", account: String = "checking") = JSONObject()
        .put("id", id).put("date", date).put("accountId", account).put("payee", payee).put("memo", "").put("amount", amount).put("status", "uncleared")
        .put("splits", JSONArray().put(JSONObject().put("categoryId", category ?: JSONObject.NULL).put("amount", amount)))
        .put("transferAccountId", JSONObject.NULL).put("transferStatus", JSONObject.NULL)
    /** Rows a tap can reach: dated, with one category, outside any transfer, not closed. */
    fun plain(row: JSONObject) = row.isNull("transferAccountId") && row.text("status") == "cleared" && row.rows("splits").size == 1
        && row.rows("splits")[0].isNull("transferAccountId") && !row.rows("splits")[0].isNull("categoryId") && row.getLong("amount") < 0
        && row.text("date") <= java.time.LocalDate.now().toString()
}

/** The stored budget, read without touching the draft. */
fun storedBudget(): JSONObject = (NativeServices.call("saved_budget") as JSONObject).getJSONObject("budget")
fun storedRow(id: String): JSONObject? = storedBudget().rows("transactions").firstOrNull { it.text("id") == id }

/** `save_budget` from outside the app's copy, with `change` applied to what's stored. The app isn't told. */
fun saveFromElsewhere(change: (JSONObject) -> Unit) {
    val answer = NativeServices.call("saved_budget") as JSONObject
    val budget = answer.getJSONObject("budget")
    change(budget)
    NativeServices.call("save_budget", JSONObject().put("budget", budget).put("expectedToken", answer.getString("token")).put("ackOnly", true))
}

private val secondConnections = HashMap<String, SQLiteDatabase>()

/**
 * Another connection to the open budget, as sync's background store is. Whatever it commits moves
 * the token. The budget's tables only take writes from the engine (`urbudget_storage_protocol`), so
 * from here that means schema: a trigger, or a table made and dropped to move nothing but the token.
 *
 * This is Android's SQLite and the engine has its own, two copies in one process, whose locks can't
 * see each other. So it asks for WAL, or opening would change the file's journal mode, and it's left
 * open, because closing the last connection it knows of checkpoints and deletes the WAL under the
 * engine. Only use it while the queue is idle.
 */
fun secondConnection(write: (SQLiteDatabase) -> Unit) {
    val path = (NativeServices.call("native_budget_status") as JSONObject).getString("databasePath")
    write(secondConnections.getOrPut(path) {
        SQLiteDatabase.openDatabase(path, null, SQLiteDatabase.OPEN_READWRITE or SQLiteDatabase.NO_LOCALIZED_COLLATORS or SQLiteDatabase.ENABLE_WRITE_AHEAD_LOGGING)
    })
}

/** A write on another connection that changes nothing in the budget. */
fun moveTheToken() = secondConnection {
    it.execSQL("CREATE TABLE edit_check(x)")
    it.execSQL("DROP TABLE edit_check")
}
