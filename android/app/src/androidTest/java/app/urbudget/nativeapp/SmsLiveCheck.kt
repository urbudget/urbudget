package app.urbudget.nativeapp

import android.app.Application
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.runBlocking
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.math.BigDecimal
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
 *   adb shell am instrument -w -e live 1 [-e banks USD,EUR] -e class app.urbudget.nativeapp.SmsLiveCheck app.urbudget.nativeapp.test/androidx.test.runner.AndroidJUnitRunner
 *
 * Its budgets ("SMS check" and the currency) are local only, never synced, and kept in the Trash
 * between runs: a run takes them out, reopens the budget that was open, and puts them back. The
 * report is sms-live-check.txt in the app's external files folder, with every reading in
 * sms-live-check.json.
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

    @Test fun banksEverywhereAreReadAsTheirTextsSay() = runBlocking<Unit> {
        val arguments = InstrumentationRegistry.getArguments()
        assumeTrue("Run only when asked: -e live 1", arguments.getString("live") == "1")
        val only = arguments.getString("banks")?.split(',')?.map { it.trim() }?.toSet()
        Words.source = ResourceWords(context.resources)
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put("live", vm) }
        waitUntil(what = "the open budget") { vm.state.value.budget != null }
        // The services finish starting a moment after the budget shows.
        var connection = JSONObject()
        waitUntil(what = "the assistant's services") { runCatching { connection = NativeServices.call("assistant_status") as JSONObject }.isSuccess }
        assumeTrue("The assistant is connected", connection.optBoolean("ready"))
        val home = open()
        val used = mutableListOf<String>()
        val report = StringBuilder("Model: ${connection.text("provider")} ${connection.optString("model")}\n")
        val raw = JSONArray()
        var right = 0
        var total = 0
        var templated = 0
        var second = 0
        try {
            for (bank in cases().rows("banks").filter { only == null || it.text("name") in only }) {
                val accounts = bank.getJSONArray("accounts").let { a -> (0 until a.length()).map { a.getJSONArray(it).getString(0) to a.getJSONArray(it).getString(1) } }
                used += budget(vm, bank.text("name"), bank.text("currency"), accounts)
                val identity = vm.state.value.budgetIdentity!!
                var templates = JSONArray()
                for ((round, key) in listOf("first", "second").withIndex()) {
                    val texts = bank.rows(key)
                    if (texts.isEmpty()) continue
                    val messages = texts.map(::sms)
                    val request = JSONObject().put("budgetId", identity).put("provider", connection.getString("provider")).put("model", connection.opt("model") ?: JSONObject.NULL)
                        .put("messages", JSONArray(messages.map { it.json() })).put("note", "").put("templates", templates).put("payees", JSONArray()).put("trusted", JSONArray())
                    val began = System.currentTimeMillis()
                    val analysis = try { NativeSmsAnalyzer.analyze(request) {} } catch (failure: Exception) {
                        report.append("${bank.text("name")} round ${round + 1}: failed: ${failure.message}\n"); total += texts.size; continue
                    }
                    val seconds = (System.currentTimeMillis() - began) / 1000
                    val items = analysis.rows("items")
                    fun itemOf(id: String) = items.firstOrNull { item -> item.getJSONArray("sourceIds").let { a -> (0 until a.length()).any { a.getString(it) == id } } }
                    var fromTemplates = 0
                    var roundRight = 0
                    val lines = mutableListOf<String>()
                    texts.zip(messages).forEach { (sent, m) ->
                        val want = sent.getJSONObject("want")
                        val others = want.optJSONArray("with")?.let { a -> (0 until a.length()).map { messages[a.getInt(it)].id } }.orEmpty()
                        val item = itemOf(m.id)
                        val wrong = problems(want, item, others)
                        if (item?.text("template")?.let { it.isNotBlank() && it != "filter:code" } == true) fromTemplates++
                        if (wrong.isEmpty()) roundRight++ else lines += "  x ${sent.text("body").take(90)}\n      ${wrong.joinToString("; ")}" +
                            (item?.optJSONObject("transaction")?.let { "\n      read: ${item.text("disposition")} ${it.text("kind")} ${it.opt("amount")} ${it.text("currency")} ${it.text("writtenAmount")} ${it.text("date")} \"${it.text("payee")}\" ref ${it.text("accountRef")} other ${it.text("otherRef")}" } ?: "")
                    }
                    right += roundRight; total += texts.size
                    if (round == 1) { templated += fromTemplates; second += texts.size }
                    val learning = analysis.optJSONObject("learning") ?: JSONObject()
                    val learned = learning.optJSONArray("templates") ?: JSONArray()
                    report.append("${bank.text("name")} round ${round + 1}: $roundRight of ${texts.size} right, $fromTemplates read by a template on the phone, " +
                        "${learned.length()} formats learned, ${learning.optInt("dropped")} dropped, ${seconds}s\n")
                    lines.forEach { report.append(it).append('\n') }
                    learning.optJSONArray("rejected")?.let { a -> (0 until a.length()).map { a.getJSONObject(it) } }?.forEach { r ->
                        report.append("  dropped: ${r.getJSONArray("lines").join(" / ").take(140)}\n      ${r.text("reason")}\n")
                    }
                    if (round == 1 && fromTemplates < texts.size) texts.zip(messages).filter { (_, m) -> itemOf(m.id)?.text("template").isNullOrBlank() }
                        .forEach { (sent, _) -> report.append("  not by a template: ${sent.text("body").take(90)}\n") }
                    raw.put(JSONObject().put("bank", bank.text("name")).put("round", round + 1).put("texts", JSONArray(texts)).put("analysis", analysis))
                    if (round == 0) templates = JSONArray((0 until learned.length()).map { learned.getJSONObject(it).getJSONObject("template") })
                }
            }
            report.append("\nAll: $right of $total right; second rounds: $templated of $second read by a learned format\n")
        } catch (failure: Throwable) {
            report.append("\nStopped: ${failure.message}\n")
            throw failure
        } finally {
            // The budget that was open comes back, and the check's budgets go back to the Trash.
            if (home != null) {
                instrumentation.runOnMainSync { vm.service("library_open", JSONObject().put("id", home)) }
                runCatching { waitUntil(what = "the budget that was open") { open() == home && vm.state.value.budget != null } }
            }
            used.distinct().filter { it != home }.forEach { id ->
                var done = false
                instrumentation.runOnMainSync { vm.service("library_trash", JSONObject().put("id", id)) { done = true } }
                runCatching { waitUntil(what = "trashing $id") { done } }
            }
            val folder = context.getExternalFilesDir(null)!!
            File(folder, "sms-live-check.txt").writeText(report.toString())
            File(folder, "sms-live-check.json").writeText(raw.toString(1))
            android.util.Log.i("SmsLiveCheck", report.toString())
            instrumentation.runOnMainSync { store.clear() }
        }
    }
}
