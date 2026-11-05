package app.urbudget.nativeapp

import android.app.Application
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ui.accountTypes
import app.urbudget.nativeapp.ui.row
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.smsDecisions
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.math.BigDecimal
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.ZoneId

/**
 * A live check, not a test: made-up texts in the ways banks in many countries write go through the
 * phone's own assistant connection, each country's into a budget in its currency, and what comes back
 * is compared with what each text says. A second round sends new texts in the same formats with the
 * templates the first one learned, so the phone reads them before the assistant reviews them.
 *
 * The texts are shared/fixtures/sms-live-cases.json (written by androidTest/cases/make_sms_live_cases.py);
 * a copy pushed to the app's external files folder is used instead, so they can change without a
 * rebuild. It runs only when asked, on a phone whose assistant is connected:
 *
 *   adb shell am instrument -w -e live 1 [-e part banks|months|onboarding|primary|all] [-e banks USD,EUR] -e class app.urbudget.nativeapp.SmsLiveCheck app.urbudget.nativeapp.test/androidx.test.runner.AndroidJUnitRunner
 *
 * "onboarding" reads each month profile into an empty budget and sets it up as suggested;
 * "primary" does the same with the user's own bank texts since -e from (two months back by
 * default), beside Primary, which it only reads, for androidTest/cases/compare_onboarding.py.
 *
 * Its budgets ("SMS check" and the currency) are local only, never synced, and kept in the Trash
 * between runs: a run takes them out, reopens the budget that was open, and puts them back. The
 * report is sms-live-check.txt in the app's external files folder (sms-live-months.txt for the
 * months), with every reading in the .json beside it.
 */
@RunWith(AndroidJUnit4::class)
class SmsLiveCheck {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val store = ViewModelStore()

    private fun waitUntil(seconds: Int = 60, what: String, done: () -> Boolean) {
        val until = System.currentTimeMillis() + seconds * 1000L
        while (!done()) {
            check(System.currentTimeMillis() < until) { "Timed out waiting for $what" }
            Thread.sleep(200)
        }
    }
    private fun library(): List<JSONObject> = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries").let { a -> (0 until a.length()).map { a.getJSONObject(it) } }
    private fun open(): String? = library().firstOrNull { it.optBoolean("current") }?.text("id")
    private fun cases(): JSONObject {
        val pushed = File(context.getExternalFilesDir(null), "sms-live-cases.json")
        return JSONObject(if (pushed.isFile) pushed.readText() else instrumentation.context.assets.open("sms-live-cases.json").bufferedReader().use { it.readText() })
    }
    private fun sms(text: JSONObject): BankSms {
        val received = LocalDateTime.parse(text.text("at").replace(' ', 'T')).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
        return BankSms(smsId(text.text("sender"), received, text.text("body")), text.text("sender"), received, text.text("body"))
    }
    private fun JSONObject.nullable(key: String): String? = if (isNull(key)) null else optString(key).ifEmpty { null }

    /** What's wrong with an item for a text, or nothing. [others] are the ids of the texts it should share its item with. */
    private fun problems(want: JSONObject, item: JSONObject?, others: List<String>): List<String> {
        if (item == null) return listOf("no item")
        val ids = item.getJSONArray("sourceIds").let { a -> (0 until a.length()).map { a.getString(it) } }
        val grouping = if (others.all { it in ids }) emptyList() else listOf("not grouped with its other text")
        val choices = want.text("what").split('|')
        if (item.text("disposition") == "ignore") {
            return if ("skip:" + item.text("skip") in choices) grouping else grouping + "skipped as ${item.text("skip")} for ${want.text("what")}"
        }
        // A text that can't be one transaction (several in one, a cheque returned with a fee) is left for the user.
        if (item.text("disposition") == "unclear") return if ("unclear" in choices) grouping else grouping + "unclear for ${want.text("what")}: ${item.text("note")}"
        if ("unclear" in choices && choices.size == 1) return grouping + "read as ${item.text("disposition")} for unclear"
        val t = item.optJSONObject("transaction") ?: return grouping + "${item.text("disposition")} with no transaction for ${want.text("what")}"
        val kind = if (item.text("disposition") == "pending") "pending" else t.text("kind")
        val out = grouping.toMutableList()
        if (kind !in choices) out += "read as $kind for ${want.text("what")}"
        fun digits(s: String) = s.mapNotNull { c -> Character.digit(c, 10).takeIf { it >= 0 } }.joinToString("")
        fun same(a: String, b: String?) = b == null || (a.isNotEmpty() && digits(a).endsWith(b))
        fun money(a: String?, b: String?) = if (a == null || b == null) a == b else runCatching { BigDecimal(a).compareTo(BigDecimal(b)) == 0 }.getOrDefault(false)
        val amount = t.nullable("amount")
        val wantAmount = want.nullable("amount")
        // A transfer may be read from either of its accounts' side, and a refund or reversal either way.
        val flipped = (kind == "transfer" && same(t.text("accountRef"), want.nullable("other")) && same(t.text("otherRef"), want.nullable("ref"))
            && money(amount, wantAmount?.let { BigDecimal(it).negate().toPlainString() }))
        if (!flipped) {
            if (wantAmount != null && !money(amount, wantAmount)) out += "amount $amount for $wantAmount"
            if (wantAmount == null && amount != null && want.text("currency").isNotEmpty() && t.text("currency") == want.text("currency") && kind in setOf("expense", "income")) out += "amount $amount for none"
            if (!same(t.text("accountRef"), want.nullable("ref"))) out += "account ${t.text("accountRef")} for ${want.nullable("ref")}"
            if (want.nullable("other") != null && !same(t.text("otherRef"), want.nullable("other"))) out += "other ${t.text("otherRef")} for ${want.nullable("other")}"
        }
        if (want.text("currency").isNotEmpty() && t.text("currency") != want.text("currency")) out += "currency ${t.text("currency")} for ${want.text("currency")}"
        want.nullable("date")?.let { if (t.text("date") != it) out += "date ${t.text("date")} for $it" }
        val payees = want.optJSONArray("payee")?.let { a -> (0 until a.length()).map { a.getString(it) } }.orEmpty()
        if (payees.isNotEmpty() && payees.none { t.text("payee").lowercase().contains(it) }) out += "payee ${t.text("payee")}"
        return out
    }

    /** The check's budget for [name] with [accounts], taken out of the Trash or made, and opened. */
    private fun budget(vm: BudgetViewModel, name: String, currency: String, accounts: List<Pair<String, String>>): String {
        val title = "SMS check $name"
        val found = library().filter { it.text("name") == title && it.text("lifecycle") != "purged" }.sortedBy { it.text("lifecycle") == "trashed" }.firstOrNull()
        if (found == null) {
            instrumentation.runOnMainSync { vm.createBudget(title, currency, localOnly = true) }
        } else {
            if (found.text("lifecycle") == "trashed") {
                var restored = false
                instrumentation.runOnMainSync { vm.service("library_restore", JSONObject().put("id", found.text("id"))) { restored = true } }
                waitUntil(what = "restoring $title") { restored }
            }
            instrumentation.runOnMainSync { vm.service("library_open", JSONObject().put("id", found.text("id"))) }
        }
        waitUntil(what = title) { vm.state.value.budget?.optString("name") == title && open() != null && library().any { it.optBoolean("current") && it.text("name") == title } }
        check(vm.state.value.budget!!.text("currency") == currency) { "$title isn't in $currency" }
        val have = vm.state.value.budget!!.rows("accounts").map { it.text("name") }.toSet()
        accounts.filter { it.first !in have }.forEach { (account, type) -> instrumentation.runOnMainSync { vm.saveAccount(null, account, AccountTypes.kindFor(type), 0, type) } }
        waitUntil(what = "accounts of $title") { vm.state.value.budget!!.rows("accounts").map { it.text("name") }.containsAll(accounts.map { it.first }) }
        return open()!!
    }

    /** A payee written another way, or a text left unclear for the user to enter, does no harm; anything else would put wrong money in the budget. */
    private fun harmful(wrong: List<String>) = wrong.any { !it.startsWith("payee ") && !it.startsWith("unclear for ") }
    private fun describe(sent: JSONObject, item: JSONObject?, wrong: List<String>) = "  ${if (harmful(wrong)) "x" else "~"} ${sent.text("body").replace('\n', ' ').take(90)}\n      ${wrong.joinToString("; ")}" +
        (item?.optJSONObject("transaction")?.let { "\n      read: ${item.text("disposition")} ${it.text("kind")} ${it.opt("amount")} ${it.text("currency")} ${it.text("writtenAmount")} ${it.text("date")} \"${it.text("payee")}\" ref ${it.text("accountRef")} other ${it.text("otherRef")}" } ?: "")
    private fun itemOf(items: List<JSONObject>, id: String) = items.firstOrNull { item -> item.getJSONArray("sourceIds").let { a -> (0 until a.length()).any { a.getString(it) == id } } }
    /** Template readings in a request, and how many of them the assistant corrected. */
    private fun corrections(learning: JSONObject): Pair<Int, Int> {
        val stats = learning.optJSONObject("stats") ?: return 0 to 0
        return stats.keys().asSequence().map { stats.getJSONObject(it) }.fold(0 to 0) { (filled, corrected), s -> (filled + s.optInt("filled")) to (corrected + s.optInt("filled") - s.optInt("confirmed")) }
    }

    private class Session(val vm: BudgetViewModel, val connection: JSONObject, val report: StringBuilder, val raw: JSONArray, val used: MutableList<String>)
    /**
     * Runs [body] on the phone's own assistant connection when asked for [part], then reopens the
     * budget that was open, puts the check's budgets back in the Trash, and writes [name].txt and .json.
     */
    private fun session(part: String, name: String, body: suspend (Session) -> Unit) = runBlocking<Unit> {
        val arguments = InstrumentationRegistry.getArguments()
        assumeTrue("Run only when asked: -e live 1", arguments.getString("live") == "1")
        assumeTrue("Asked for ${arguments.getString("part")}", (arguments.getString("part") ?: "banks") in setOf(part, "all"))
        Words.source = ResourceWords(context.resources)
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put("live", vm) }
        waitUntil(what = "the open budget") { vm.state.value.budget != null }
        // The services finish starting a moment after the budget shows.
        var connection = JSONObject()
        waitUntil(what = "the assistant's services") { runCatching { connection = NativeServices.call("assistant_status") as JSONObject }.isSuccess }
        assumeTrue("The assistant is connected", connection.optBoolean("ready"))
        // The budget to come back to: the one open, unless a stopped run left one of its own open; then -e home <id>.
        val home = arguments.getString("home") ?: open()?.takeIf { id -> library().none { it.text("id") == id && it.text("name").startsWith("SMS check") } }
        checkNotNull(home) { "A check budget is open: say which to come back to with -e home <id>" }
        val session = Session(vm, connection, StringBuilder("Model: ${connection.text("provider")} ${connection.optString("model")}\n"), JSONArray(), mutableListOf())
        try {
            body(session)
        } catch (failure: Throwable) {
            session.report.append("\nStopped: ${failure.message}\n")
            throw failure
        } finally {
            instrumentation.runOnMainSync { vm.service("library_open", JSONObject().put("id", home)) }
            runCatching { waitUntil(what = "the budget that was open") { open() == home && vm.state.value.budget != null } }
            // Every check budget out of the Trash goes back, a stopped run's too.
            val strays = runCatching { library().filter { it.text("name").startsWith("SMS check") && it.text("lifecycle") !in setOf("trashed", "purged") }.map { it.text("id") } }.getOrDefault(emptyList())
            (session.used + strays).distinct().filter { it != home }.forEach { id ->
                var done = false
                instrumentation.runOnMainSync { vm.service("library_trash", JSONObject().put("id", id)) { done = true } }
                runCatching { waitUntil(what = "trashing $id") { done } }
            }
            val folder = context.getExternalFilesDir(null)!!
            File(folder, "$name.txt").writeText(session.report.toString())
            File(folder, "$name.json").writeText(session.raw.toString(1))
            android.util.Log.i("SmsLiveCheck", session.report.toString())
            instrumentation.runOnMainSync { store.clear() }
        }
    }
    private fun accounts(set: JSONObject) = set.getJSONArray("accounts").let { a -> (0 until a.length()).map { a.getJSONArray(it).getString(0) to a.getJSONArray(it).getString(1) } }

    /** Each set's texts in one request, then a second round through the formats the first learned. */
    @Test fun banksEverywhereAreReadAsTheirTextsSay() = session("banks", "sms-live-check") { s ->
        val only = InstrumentationRegistry.getArguments().getString("banks")?.split(',')?.map { it.trim() }?.toSet()
        var right = 0; var harm = 0; var total = 0; var templated = 0; var second = 0; var filledAll = 0; var correctedAll = 0
        for (bank in cases().rows("banks").filter { only == null || it.text("name") in only }) {
            s.used += budget(s.vm, bank.text("name"), bank.text("currency"), accounts(bank))
            val identity = s.vm.state.value.budgetIdentity!!
            var templates = JSONArray()
            for ((round, key) in listOf("first", "second").withIndex()) {
                val texts = bank.rows(key)
                if (texts.isEmpty()) continue
                val messages = texts.map(::sms)
                val request = JSONObject().put("budgetId", identity).put("provider", s.connection.getString("provider")).put("model", s.connection.opt("model") ?: JSONObject.NULL)
                    .put("messages", JSONArray(messages.map { it.json() })).put("note", "").put("templates", templates).put("payees", JSONArray()).put("trusted", JSONArray())
                val began = System.currentTimeMillis()
                val analysis = try { NativeSmsAnalyzer.analyze(request) {} } catch (failure: Exception) {
                    s.report.append("${bank.text("name")} round ${round + 1}: failed: ${failure.message}\n"); total += texts.size; harm += texts.size; continue
                }
                val seconds = (System.currentTimeMillis() - began) / 1000
                val items = analysis.rows("items")
                var fromTemplates = 0
                var roundRight = 0
                val lines = mutableListOf<String>()
                texts.zip(messages).forEach { (sent, m) ->
                    val want = sent.getJSONObject("want")
                    val others = want.optJSONArray("with")?.let { a -> (0 until a.length()).map { messages[a.getInt(it)].id } }.orEmpty()
                    val item = itemOf(items, m.id)
                    val wrong = problems(want, item, others)
                    if (item?.text("template")?.let { it.isNotBlank() && it != "filter:code" } == true) fromTemplates++
                    if (wrong.isEmpty()) roundRight++ else { lines += describe(sent, item, wrong); if (harmful(wrong)) harm++ }
                }
                right += roundRight; total += texts.size
                val learning = analysis.optJSONObject("learning") ?: JSONObject()
                val (filled, corrected) = corrections(learning)
                if (round == 1) { templated += fromTemplates; second += texts.size; filledAll += filled; correctedAll += corrected }
                val learned = learning.optJSONArray("templates") ?: JSONArray()
                s.report.append("${bank.text("name")} round ${round + 1}: $roundRight of ${texts.size} right, $fromTemplates read by a template on the phone" +
                    (if (filled > 0) " ($corrected corrected by the assistant)" else "") + ", ${learned.length()} formats learned, ${learning.optInt("dropped")} dropped, ${seconds}s\n")
                lines.forEach { s.report.append(it).append('\n') }
                learning.optJSONArray("rejected")?.let { a -> (0 until a.length()).map { a.getJSONObject(it) } }?.forEach { r ->
                    s.report.append("  dropped: ${r.getJSONArray("lines").join(" / ").take(140)}\n      ${r.text("reason")}\n")
                }
                if (round == 1 && fromTemplates < texts.size) texts.zip(messages).filter { (_, m) -> itemOf(items, m.id)?.text("template").isNullOrBlank() }
                    .forEach { (sent, _) -> s.report.append("  not by a template: ${sent.text("body").replace('\n', ' ').take(90)}\n") }
                s.raw.put(JSONObject().put("bank", bank.text("name")).put("round", round + 1).put("texts", JSONArray(texts)).put("analysis", analysis))
                if (round == 0) templates = JSONArray((0 until learned.length()).map { learned.getJSONObject(it).getJSONObject("template") })
            }
        }
        s.report.append("\nAll: $right of $total right, $harm harmful; second rounds: $templated of $second read by a learned format, $correctedAll of $filledAll of those corrected by the assistant\n")
    }

    /** Records what each request of a reading carried, how long it took, and what the phone did with its texts. */
    private class Recorder(private val inner: SmsAnalyzer) : SmsAnalyzer {
        val requests = mutableListOf<String>()
        var seconds = 0L; var read = 0; var reviewed = 0; var junk = 0; var filled = 0; var corrected = 0
        val rejected = mutableListOf<String>()
        override suspend fun status() = inner.status()
        override suspend fun lanes(request: JSONObject) = inner.lanes(request)
        override suspend fun analyze(request: JSONObject, live: (SmsLive) -> Unit): JSONObject {
            val began = System.currentTimeMillis()
            val result = try { inner.analyze(request, live) } catch (failure: Exception) {
                requests += "${request.getJSONArray("messages").length()} texts failed after ${(System.currentTimeMillis() - began) / 1000}s: ${failure.message}"
                throw failure
            }
            val took = (System.currentTimeMillis() - began) / 1000
            val learning = result.optJSONObject("learning") ?: JSONObject()
            seconds += took; read += learning.optInt("read"); reviewed += learning.optInt("reviewed"); junk += learning.optInt("junk")
            val stats = learning.optJSONObject("stats")
            stats?.keys()?.forEach { id -> stats.getJSONObject(id).let { filled += it.optInt("filled"); corrected += it.optInt("filled") - it.optInt("confirmed") } }
            learning.optJSONArray("rejected")?.let { a -> (0 until a.length()).forEach { rejected += a.getJSONObject(it).getJSONArray("lines").join(" / ").take(100) + ": " + a.getJSONObject(it).text("reason") } }
            requests += "${request.getJSONArray("messages").length()} texts (${learning.optInt("read")} read in full, ${learning.optInt("reviewed")} by formats, ${learning.optInt("junk")} kept on the phone) in ${took}s"
            return result
        }
    }

    /** Months of texts, as a phone gets them, read the way the app reads them: requests sized by lane, formats learned as it goes. */
    @Test fun monthsOfTextsAreReadAsThePhoneReadsThem() = session("months", "sms-live-months") { s ->
        val inbox = SmsInbox.get(context)
        for (profile in cases().rows("months")) {
            s.used += budget(s.vm, profile.text("name"), profile.text("currency"), accounts(profile))
            val identity = s.vm.state.value.budgetIdentity!!
            inbox.clear(identity)
            val months = profile.getJSONArray("months")
            try {
                for (n in 0 until months.length()) {
                    val texts = months.getJSONArray(n).let { a -> (0 until a.length()).map { a.getJSONObject(it) } }
                    val messages = texts.map(::sms)
                    val recorder = Recorder(NativeSmsAnalyzer)
                    val batch = SmsReadings.read(context, recorder, identity, s.connection, messages, "", null) { true } ?: error("Another reading is under way")
                    val items = batch.analysis.rows("items")
                    var right = 0; var harm = 0
                    val lines = mutableListOf<String>()
                    texts.zip(messages).forEach { (sent, m) ->
                        val item = itemOf(items, m.id)
                        val wrong = problems(sent.getJSONObject("want"), item, emptyList())
                        if (wrong.isEmpty()) right++ else { lines += describe(sent, item, wrong); if (harmful(wrong)) harm++ }
                    }
                    val phone = recorder.junk + recorder.reviewed
                    s.report.append("${profile.text("name")} month ${n + 1}: ${texts.size} texts, $right right, $harm harmful; ${recorder.requests.size} requests, ${recorder.seconds}s; " +
                        "${recorder.junk} kept on the phone, ${recorder.reviewed} read by learned formats (${recorder.corrected} corrected), ${recorder.read} read in full: " +
                        "${if (texts.isEmpty()) 0 else phone * 100 / texts.size}% handled by the phone\n")
                    recorder.requests.forEach { s.report.append("  request: $it\n") }
                    recorder.rejected.forEach { s.report.append("  dropped: $it\n") }
                    lines.forEach { s.report.append(it).append('\n') }
                    s.raw.put(JSONObject().put("profile", profile.text("name")).put("month", n + 1).put("analysis", batch.analysis))
                }
            } finally {
                inbox.clear(identity)
            }
        }
    }

    /** Empties a check budget kept from an earlier run: no accounts, transactions, or record of texts added. */
    private fun empty(vm: BudgetViewModel) {
        val budget = JSONObject(vm.exportBudget() ?: error("The budget can't be read"))
        if (budget.rows("accounts").isEmpty() && budget.rows("transactions").isEmpty()) return
        budget.put("accounts", JSONArray()).put("transactions", JSONArray())
            .put("categories", JSONArray(budget.rows("categories").filter { it.text("paymentAccountId").isBlank() }))
        budget.keys().asSequence().toList().filter { Regex("(?i)sms|bankImport|tombstone").containsMatchIn(it) }.forEach { budget.remove(it) }
        instrumentation.runOnMainSync { vm.importBudget(budget.toString()) }
        waitUntil(what = "an empty budget") { vm.state.value.budget!!.let { it.rows("accounts").isEmpty() && it.rows("transactions").isEmpty() } }
    }

    /** What [onboard] did: the review, the accounts made, what's remembered, and the rows added and left waiting. */
    private class Onboarded(val batch: SmsBatch, val items: List<SmsItem>, val candidates: List<SmsSetupCandidate>, val plans: Map<String, SmsSetupPlan>,
                            val made: List<JSONObject>, val mappings: JSONArray, val added: Int, val waiting: Int)

    /**
     * Reads [messages] into the open, empty budget the way the app does, takes every suggestion of
     * the accounts to set up, and adds every row then ready: Set up, then Add all, with the same
     * functions the review page uses.
     */
    private suspend fun onboard(s: Session, identity: String, messages: List<BankSms>, recorder: Recorder): Onboarded {
        val read = SmsReadings.read(context, recorder, identity, s.connection, messages, "", null) { true } ?: error("Another reading is under way")
        val empty = s.vm.state.value.budget!!
        val candidates = smsSetupCandidates(read, read.items(SmsAccounts.of(empty, JSONArray(), read.starts(empty))))
        val names = accountTypes().toMap()
        val plans = smsSetupPlans(candidates, emptyMap(), smsSetupSuggestions(candidates, empty) { names[it].orEmpty() }, emptySet())
        val changes = smsSetupChanges(candidates, plans, { it.tickedIn(read) }) { java.util.UUID.randomUUID().toString() }
        val ids = changes.accounts.map { it.text("id") }
        if (ids.isNotEmpty()) instrumentation.runOnMainSync { s.vm.saveAccounts(changes.accounts) }
        waitUntil(what = "the accounts set up") { s.vm.state.value.budget!!.rows("accounts").map { it.text("id") }.containsAll(ids) }
        val mappings = changes.remember.fold(JSONArray()) { kept, r -> smsRememberCard(kept, r.sender, r.ref, r.account, r.kind) }
        SmsInbox.get(context).saveState(identity, "mappings", mappings.toString())
        val batch = changes.spending.fold(read.copy(fresh = ids.toSet())) { b, index -> b.edit(index, ::smsUntracked) }
        val budget = s.vm.state.value.budget!!
        val items = batch.items(SmsAccounts.of(budget, mappings, batch.starts(budget)))
        val adding = items.filter { it.needs == null && it.tickedIn(batch) && it.index !in batch.dismissed }.map { it.index to "new" }
        val before = budget.rows("transactions").size
        if (adding.isNotEmpty()) {
            var done = false
            instrumentation.runOnMainSync { s.vm.runCommand("smsBatchApply", batch.params(batch.resolved(items)).put("decisions", smsDecisions(adding))) { done = true } }
            waitUntil(300, "adding ${adding.size} rows") { done || s.vm.state.value.error != null }
            check(done) { "Adding failed: ${s.vm.state.value.error}" }
        }
        val waiting = items.count { it.needs != null && it.index !in batch.dismissed && it.disposition in setOf("transaction", "pending") }
        return Onboarded(batch, items, candidates, plans, changes.accounts, mappings, s.vm.state.value.budget!!.rows("transactions").size - before, waiting)
    }

    /** The account the setup remembered a card's digits with. */
    private fun accountFor(mappings: JSONArray, ref: String): String? {
        val digits = smsDigits(ref).takeLast(4)
        return (0 until mappings.length()).map { mappings.getJSONObject(it) }.firstOrNull { m ->
            m.text("for").isBlank() && smsDigits(m.text("reference")).let { it.length >= 3 && (it.endsWith(digits) || digits.endsWith(it)) }
        }?.text("accountId")?.takeIf { it != SmsSkipCard }
    }

    /** What the setup made and where the texts went: each candidate's plan, each account made and its balance. */
    private fun setupReport(s: Session, name: String, texts: Int, done: Onboarded, recorder: Recorder) {
        val budget = s.vm.state.value.budget!!
        val balances = s.vm.state.value.summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty()
        fun named(id: String?) = id?.let { budget.row("accounts", it)?.text("name") } ?: "none"
        s.report.append("$name: $texts texts in ${recorder.requests.size} requests, ${recorder.seconds}s (${recorder.read} read in full, ${recorder.reviewed} by formats, " +
            "${recorder.junk} kept on the phone); ${done.made.size} accounts made, ${done.added} transactions added, ${done.waiting} still waiting\n")
        done.candidates.forEach { c ->
            val plan = done.plans.getValue(c.key)
            val what = when (plan.action) {
                SmsSetupPlan.Join -> "part of " + (done.plans[plan.joins]?.name ?: named(plan.joins))
                SmsSetupPlan.Create -> "made ${plan.name} (${plan.type})"
                else -> "not tracked"
            }
            s.report.append("  ${c.sender} ${c.refs.joinToString(", ").ifEmpty { "(no card)" }}: ${c.texts} texts, $what\n")
            // What the words by its digits say it is, from its first texts.
            val near = c.items.take(2).map { i -> smsWordsBy(i.sources.first().body, i.ref.orEmpty()) } +
                c.via.take(2).map { i -> smsWordsBy(i.sources.first().body, i.transaction!!.text("otherRef")) }
            s.report.append("      ${smsSetupKind(c)}; by its digits: ${near.joinToString(" | ") { (before, after) -> "\"${before.trim()}\" … \"${after.trim()}\"" }}\n")
        }
        done.made.forEach { a ->
            s.report.append("  ${a.text("name")}: balance before ${BigDecimal.valueOf(a.optLong("openingBalance"), 2).toPlainString()}, " +
                "now ${BigDecimal.valueOf(balances[a.text("id")] ?: 0L, 2).toPlainString()}\n")
        }
    }

    /** Each month profile's two months into an empty budget, set up as suggested and added: the accounts made, where each text went, and the balances. */
    @Test fun anEmptyBudgetIsSetUpFromMonthsOfTexts() = session("onboarding", "sms-live-onboarding") { s ->
        val inbox = SmsInbox.get(context)
        for (profile in cases().rows("months")) {
            s.used += budget(s.vm, profile.text("name") + " onboarding", profile.text("currency"), emptyList())
            empty(s.vm)
            val identity = s.vm.state.value.budgetIdentity!!
            inbox.clear(identity)
            val months = profile.getJSONArray("months")
            val texts = (0 until months.length()).flatMap { n -> months.getJSONArray(n).let { a -> (0 until a.length()).map { a.getJSONObject(it) } } }
            val messages = texts.map(::sms)
            val recorder = Recorder(NativeSmsAnalyzer)
            try {
                val done = onboard(s, identity, messages, recorder)
                setupReport(s, profile.text("name"), texts.size, done, recorder)
                val budget = s.vm.state.value.budget!!
                fun named(id: String?) = id?.let { budget.row("accounts", it)?.text("name") } ?: "none"
                // The profile's accounts, found by their digits.
                accounts(profile).forEach { (account, type) ->
                    val got = done.made.firstOrNull { it.text("id") == accountFor(done.mappings, account) }
                    val mark = when { got == null -> "x missing"; got.text("accountType") == type -> "ok"; else -> "~ type" }
                    s.report.append("  $mark $account ($type): ${got?.let { it.text("name") + " (" + it.text("accountType") + ")" } ?: "none"}\n")
                }
                // Each money text in the account its digits name, and a transfer's other side too.
                var right = 0; var money = 0
                texts.zip(messages).forEach { (sent, m) ->
                    val want = sent.getJSONObject("want")
                    val ref = want.nullable("ref")?.takeIf { !want.text("what").startsWith("skip") } ?: return@forEach
                    money++
                    val t = done.items.firstOrNull { i -> i.sources.any { it.id == m.id } }?.transaction
                    val account = accountFor(done.mappings, ref)
                    val other = want.nullable("other")?.let { accountFor(done.mappings, it) }
                    if (t != null && t.text("accountId") == account && (other == null || t.text("transferAccountId") == other)) right++
                    else s.report.append("  x ${sent.text("body").replace('\n', ' ').take(90)}\n      in ${named(t?.text("accountId"))}" +
                        "${t?.text("transferAccountId")?.ifBlank { null }?.let { " -> " + named(it) }.orEmpty()}, wanted ${named(account)}${other?.let { " -> " + named(it) }.orEmpty()}\n")
                }
                s.report.append("  $right of $money money texts in the right accounts\n")
                // Where a text gives an account's balance, its balance now is that one plus what came after.
                val balances = s.vm.state.value.summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty()
                val added = done.items.filter { it.needs == null && it.tickedIn(done.batch) && it.index !in done.batch.dismissed }
                fun at(item: SmsItem) = item.sources.minOf { it.received }
                done.made.filterNot { AccountTypes.isLiability(it.text("accountType")) }.forEach { a ->
                    val id = a.text("id")
                    val (alert, given) = added.filter { it.transaction?.text("accountId") == id }
                        .mapNotNull { i -> i.transaction?.text("balance")?.let(::smsBalanceCents)?.let { i to it } }.maxByOrNull { at(it.first) } ?: return@forEach
                    val since = added.filter { at(it) > at(alert) }.sumOf { i ->
                        val t = i.transaction!!
                        (if (t.text("accountId") == id) i.amount ?: 0L else 0L) - (if (t.text("transferAccountId") == id) i.amount ?: 0L else 0L)
                    }
                    val now = balances[id] ?: 0L
                    s.report.append("  ${if (now == given + since) "ok" else "x"} ${a.text("name")} is ${BigDecimal.valueOf(now, 2).toPlainString()}: the bank's " +
                        "${BigDecimal.valueOf(given, 2).toPlainString()} on ${alert.sources.first().receivedDate}, and ${BigDecimal.valueOf(since, 2).toPlainString()} since\n")
                }
                s.raw.put(JSONObject().put("profile", profile.text("name")).put("analysis", done.batch.analysis).put("mappings", done.mappings))
            } finally {
                inbox.clear(identity)
            }
        }
    }

    /** The parts of a budget the comparison reads: accounts, and every transaction with its parts' categories. */
    private fun slice(budget: JSONObject): JSONObject {
        val categories = budget.rows("categories").associate { it.text("id") to it.text("name") }
        return JSONObject().put("currency", budget.text("currency"))
            .put("accounts", JSONArray(budget.rows("accounts").map { a ->
                JSONObject().put("id", a.text("id")).put("name", a.text("name")).put("type", AccountTypes.infer(a)).put("archived", a.optBoolean("archived")).put("opening", a.optLong("openingBalance"))
            }))
            .put("transactions", JSONArray(budget.rows("transactions").map { t ->
                JSONObject().put("date", t.text("date")).put("account", t.text("accountId")).put("other", t.text("transferAccountId")).put("amount", t.optLong("amount"))
                    .put("payee", t.text("payee")).put("bank", (t.optJSONArray("bankImportIds")?.length() ?: 0) > 0)
                    .put("parts", JSONArray(t.rows("splits").map { p -> JSONObject().put("amount", p.optLong("amount")).put("category", categories[p.text("categoryId")].orEmpty()).put("other", p.text("transferAccountId")) }))
            }))
    }

    /**
     * The user's own bank texts, from the senders Primary collects from and since -e from (two
     * months back by default), into an empty budget set up as suggested and added, as a new user
     * would; then both budgets' accounts and transactions to sms-live-primary-data.json, for
     * compare_onboarding.py. Primary is only read.
     */
    @Test fun theUsersTextsSetUpFromNothingBesidePrimary() = session("primary", "sms-live-primary") { s ->
        val args = InstrumentationRegistry.getArguments()
        val inbox = SmsInbox.get(context)
        check(args.getString("home") != null && open() == args.getString("home")) { "Open Primary and pass its id with -e home" }
        val primary = JSONObject(s.vm.exportBudget() ?: error("Primary can't be read"))
        val senders = inbox.sources(s.vm.state.value.budgetIdentity!!).map { it.sender }.toSet()
        check(senders.isNotEmpty()) { "Primary collects no bank texts" }
        val to = LocalDate.now()
        val from = args.getString("from")?.let(LocalDate::parse) ?: to.minusMonths(2)
        s.used += budget(s.vm, "Onboarding test", primary.text("currency"), emptyList())
        empty(s.vm)
        val identity = s.vm.state.value.budgetIdentity!!
        inbox.clear(identity)
        try {
            inbox.configure(identity, senders, from.atStartOfDay(ZoneId.systemDefault()).toInstant().toEpochMilli())
            SmsReader.refresh(context, identity)
            val messages = inbox.pending(identity)
            val recorder = Recorder(NativeSmsAnalyzer)
            s.report.append("${messages.size} texts since $from from ${senders.size} senders\n")
            val done = try { onboard(s, identity, messages, recorder) } finally { recorder.requests.forEach { s.report.append("  request: $it\n") } }
            setupReport(s, "Your texts since $from", messages.size, done, recorder)
            recorder.rejected.forEach { s.report.append("  dropped: $it\n") }
            val made = JSONObject(s.vm.exportBudget() ?: error("The test budget can't be read"))
            File(context.getExternalFilesDir(null), "sms-live-primary-data.json").writeText(JSONObject().put("from", from.toString()).put("to", to.toString())
                .put("primary", slice(primary)).put("onboarding", slice(made))
                .put("setup", JSONArray(done.candidates.map { c -> JSONObject().put("sender", c.sender).put("ref", c.ref ?: JSONObject.NULL).put("texts", c.texts).put("plan", done.plans.getValue(c.key).json()) }))
                .put("unclear", done.items.count { it.disposition == "unclear" && it.index !in done.batch.dismissed }).toString(1))
        } finally {
            inbox.clear(identity)
        }
    }
}
