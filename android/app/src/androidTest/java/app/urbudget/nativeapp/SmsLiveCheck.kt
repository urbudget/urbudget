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
 * A live check, not a test: made-up texts in the formats of banks in the US, the UK, Germany,
 * France, Spain and India go through the phone's own assistant connection, each country's into a
 * budget in its currency, and what comes back is compared with what each text says. A second round
 * sends new texts in the same formats with the templates the first one learned, so the phone reads
 * them before the assistant reviews them.
 *
 * It runs only when asked, on a phone whose assistant is connected:
 *
 *   adb shell am instrument -w -e live 1 -e class app.urbudget.nativeapp.SmsLiveCheck app.urbudget.nativeapp.test/androidx.test.runner.AndroidJUnitRunner
 *
 * Its budgets are local only (never synced), the budget that was open is opened again, and they go
 * to the Trash. The report is in the app's external files folder, sms-live-check.txt.
 */
@RunWith(AndroidJUnit4::class)
class SmsLiveCheck {
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val store = ViewModelStore()

    /** What a text says, as the review should have it. [what] is a kind, "pending", or "skip:" and a reason. */
    private data class Want(val what: String, val amount: String? = null, val currency: String = "", val date: String? = null, val ref: String? = null,
                            val payee: String? = null, val other: String? = null)
    private data class Text(val sender: String, val at: String, val body: String, val want: Want)
    private data class Bank(val name: String, val currency: String, val accounts: List<Pair<String, String>>, val first: List<Text>, val second: List<Text>)

    private fun spend(amount: String, currency: String, date: String?, ref: String, payee: String? = null) = Want("expense", "-$amount", currency, date, ref, payee)

    private val banks = listOf(
        Bank("USD", "USD", listOf("Checking 3301" to "checking", "Savings 7714" to "savings", "Visa 9876" to "creditCard"), listOf(
            Text("BANK-US", "2026-11-03 18:22", "You made a \$45.00 transaction with SQ *BLUE BOTTLE on your card ending 9876 on 11/03.", spend("45.00", "USD", "2026-11-03", "9876", "blue bottle")),
            Text("BANK-US", "2026-11-02 12:05", "You made a \$1,204.17 transaction with AMAZON MKTPLACE PMTS on your card ending 9876 on 11/02.", spend("1204.17", "USD", "2026-11-02", "9876", "amazon")),
            Text("BANK-US", "2026-10-31 07:00", "A \$2,450.00 direct deposit from ACME PAYROLL posted to checking ***3301 on 10/31.", Want("income", "2450.00", "USD", "2026-10-31", "3301")),
            Text("BANK-US", "2026-11-01 09:30", "You sent \$60.00 to Jordan Lee with Zelle from checking ***3301 on 11/01.", spend("60.00", "USD", "2026-11-01", "3301", "jordan")),
            Text("BANK-US", "2026-11-01 10:00", "Payment of \$1,500.00 from checking ***3301 to your credit card ending 9876 posted 11/01/26", Want("transfer", "-1500.00", "USD", "2026-11-01", "3301", other = "9876")),
            Text("BANK-US", "2026-11-02 08:15", "Transfer of \$300.00 from checking ***3301 to savings ***7714 completed on 11/02.", Want("transfer", "-300.00", "USD", "2026-11-02", "3301", other = "7714")),
            Text("BANK-US", "2026-11-02 08:40", "Your one-time code is 482913. It expires in 10 minutes. Never share it.", Want("skip:code")),
            Text("BANK-US", "2026-10-31 09:12", "Card ending 9876: purchase of EUR 42.00 at CAFE LUMIERE PARIS on 10/31/26", Want("expense", null, "EUR", "2026-10-31", "9876", "lumiere")),
            Text("BANK-US", "2026-11-02 21:00", "A \$150.00 hold was placed by MARRIOTT HOTELS on your card ending 9876 on 11/02.", Want("pending", "-150.00", "USD", "2026-11-02", "9876", "marriott")),
            Text("BANK-US", "2026-11-02 22:10", "Your transaction of \$230.00 at BEST BUY on card ending 9876 was declined.", Want("skip:declined")),
            Text("BANK-US", "2026-11-01 16:45", "Refund of \$19.99 from AMAZON MKTPLACE posted to card ending 9876 on 11/01.", Want("refund", "19.99", "USD", "2026-11-01", "9876", "amazon")),
        ), listOf(
            Text("BANK-US", "2026-11-04 08:05", "You made a \$7.25 transaction with STARBUCKS STORE 0412 on your card ending 9876 on 11/04.", spend("7.25", "USD", "2026-11-04", "9876", "starbucks")),
            // Received on New Year's Day about the day before: last year.
            Text("BANK-US", "2027-01-01 10:00", "You made a \$88.00 transaction with SHELL OIL 5774 on your card ending 9876 on 12/31.", spend("88.00", "USD", "2026-12-31", "9876", "shell")),
            Text("BANK-US", "2026-11-14 07:00", "A \$2,450.00 direct deposit from ACME PAYROLL posted to checking ***3301 on 11/14.", Want("income", "2450.00", "USD", "2026-11-14", "3301")),
            Text("BANK-US", "2026-11-05 10:00", "Payment of \$820.40 from checking ***3301 to your credit card ending 9876 posted 11/05/26", Want("transfer", "-820.40", "USD", "2026-11-05", "3301", other = "9876")),
        )),
        Bank("GBP", "GBP", listOf("Current Account 2210" to "checking", "Savings 8890" to "savings", "Credit Card 4512" to "creditCard"), listOf(
            Text("BANK-UK", "2026-10-28 15:01", "You've spent £12.50 at TESCO STORES 3041 with your card ending 4512.", spend("12.50", "GBP", "2026-10-28", "4512", "tesco")),
            Text("BANK-UK", "2026-10-30 13:20", "You've spent £1,089.00 at JOHN LEWIS with your card ending 4512.", spend("1089.00", "GBP", "2026-10-30", "4512", "john lewis")),
            Text("BANK-UK", "2026-10-31 06:10", "£2,150.00 has been paid into your account ending 2210 from ACME LTD. Balance £3,402.17", Want("income", "2150.00", "GBP", "2026-10-31", "2210", "acme")),
            Text("BANK-UK", "2026-11-01 05:00", "£45.00 Direct Debit to THAMES WATER from your account ending 2210", spend("45.00", "GBP", "2026-11-01", "2210", "thames")),
            Text("BANK-UK", "2026-11-01 18:00", "You've moved £200.00 from your account ending 2210 to your savings ending 8890.", Want("transfer", "-200.00", "GBP", "2026-11-01", "2210", other = "8890")),
            Text("BANK-UK", "2026-11-01 14:00", "A pre-authorisation of £150.00 was placed by HILTON LONDON on card 4512", Want("pending", "-150.00", "GBP", "2026-11-01", "4512", "hilton")),
            Text("BANK-UK", "2026-11-02 09:00", "Your security code is 739201. We'll never call you to ask for it.", Want("skip:code")),
            // Spent in euros, with what it cost in pounds.
            Text("BANK-UK", "2026-11-02 12:30", "You've spent €30.00 (£26.12) at CAFE DE FLORE with your card ending 4512.", Want("expense", "-26.12", "EUR", "2026-11-02", "4512", "flore")),
        ), listOf(
            Text("BANK-UK", "2026-11-03 08:12", "You've spent £3.40 at PRET A MANGER with your card ending 4512.", spend("3.40", "GBP", "2026-11-03", "4512", "pret")),
            Text("BANK-UK", "2026-11-30 06:10", "£2,150.00 has been paid into your account ending 2210 from ACME LTD. Balance £4,950.00", Want("income", "2150.00", "GBP", "2026-11-30", "2210", "acme")),
            Text("BANK-UK", "2026-11-04 18:00", "You've moved £1,000.00 from your account ending 2210 to your savings ending 8890.", Want("transfer", "-1000.00", "GBP", "2026-11-04", "2210", other = "8890")),
        )),
        Bank("EUR", "EUR", listOf("Girokonto 5521" to "checking", "Tagesgeld 9902" to "savings", "Kreditkarte 4421" to "creditCard"), listOf(
            Text("BANK-DE", "2026-10-31 09:12", "Kartenzahlung 1.234,56 EUR bei MEDIAMARKT BERLIN, Karte ...4421, 31.10.2026", spend("1234.56", "EUR", "2026-10-31", "4421", "mediamarkt")),
            Text("BANK-DE", "2026-11-02 17:40", "Kartenzahlung 12,50 EUR bei REWE MARKT, Karte ...4421, 02.11.2026", spend("12.50", "EUR", "2026-11-02", "4421", "rewe")),
            Text("BANK-DE", "2026-10-30 08:00", "Gutschrift 2.850,00 EUR auf Konto ...5521 von ACME GMBH (Gehalt Oktober), 30.10.2026", Want("income", "2850.00", "EUR", "2026-10-30", "5521")),
            Text("BANK-DE", "2026-11-01 07:00", "Lastschrift 49,99 EUR von VODAFONE GMBH, Konto ...5521, 01.11.2026", spend("49.99", "EUR", "2026-11-01", "5521", "vodafone")),
            Text("BANK-DE", "2026-11-01 12:00", "Überweisung 500,00 EUR von Konto ...5521 auf Tagesgeld ...9902, 01.11.2026", Want("transfer", "-500.00", "EUR", "2026-11-01", "5521", other = "9902")),
            Text("BANK-DE", "2026-11-02 09:00", "Ihr Bestätigungscode lautet 482193. Geben Sie ihn nicht weiter.", Want("skip:code")),
            Text("BANK-FR", "2026-11-02 13:05", "Paiement CB de 23,40 € chez CARREFOUR CITY le 02/11 carte ****4421", spend("23.40", "EUR", "2026-11-02", "4421", "carrefour")),
            Text("BANK-ES", "2026-11-03 11:00", "Compra con tarjeta 4421 por 15,90 EUR en MERCADONA el 03/11/2026", spend("15.90", "EUR", "2026-11-03", "4421", "mercadona")),
        ), listOf(
            Text("BANK-DE", "2026-11-05 12:00", "Kartenzahlung 7,80 EUR bei BAECKEREI SCHMIDT, Karte ...4421, 05.11.2026", spend("7.80", "EUR", "2026-11-05", "4421", "schmidt")),
            // Grouped with a space, as French banks write.
            Text("BANK-FR", "2026-11-06 19:30", "Paiement CB de 1 249,00 € chez FNAC le 06/11 carte ****4421", spend("1249.00", "EUR", "2026-11-06", "4421", "fnac")),
            Text("BANK-DE", "2026-11-30 08:00", "Gutschrift 2.850,00 EUR auf Konto ...5521 von ACME GMBH (Gehalt November), 30.11.2026", Want("income", "2850.00", "EUR", "2026-11-30", "5521")),
        )),
        Bank("INR", "INR", listOf("Savings A/c 1234" to "savings", "Credit Card 5678" to "creditCard"), listOf(
            Text("BANK-IN", "2026-10-29 10:12", "Rs.2,500.00 debited from A/c XX1234 on 29-Oct-26 to VPA swiggy@icici (UPI Ref No 630123456789). Not you? Call 18002586161", spend("2500.00", "INR", "2026-10-29", "1234", "swiggy")),
            Text("BANK-IN", "2026-10-31 09:00", "INR 1,23,456.00 credited to A/c XX1234 on 31-Oct-26 by NEFT from ACME TECHNOLOGIES PVT LTD. Avl Bal INR 2,45,000.50", Want("income", "123456.00", "INR", "2026-10-31", "1234", "acme")),
            Text("BANK-IN", "2026-11-02 14:20", "482193 is your OTP for txn of INR 4,999.00 at FLIPKART on card XX5678. Valid for 10 mins. Do not share.", Want("skip:code")),
            Text("BANK-IN", "2026-11-02 14:22", "Rs.4,999.00 spent on card XX5678 at FLIPKART on 02-Nov-26.", spend("4999.00", "INR", "2026-11-02", "5678", "flipkart")),
            Text("BANK-IN", "2026-11-01 11:00", "Rs.10,000.00 transferred from A/c XX1234 to your Credit Card XX5678 on 01-Nov-26", Want("transfer", "-10000.00", "INR", "2026-11-01", "1234", other = "5678")),
            Text("BANK-IN", "2026-11-01 06:00", "₹ 349.00 debited from A/c XX1234 for NETFLIX autopay on 01-11-2026", spend("349.00", "INR", "2026-11-01", "1234", "netflix")),
            Text("BANK-IN", "2026-11-03 09:15", "Rs.150.00 debited from A/c XX1234 on 03-Nov-26 to VPA 9876543210@ybl (UPI Ref No 630199887766). Not you? Call 18002586161", spend("150.00", "INR", "2026-11-03", "1234")),
        ), listOf(
            Text("BANK-IN", "2026-11-05 20:12", "Rs.640.00 debited from A/c XX1234 on 05-Nov-26 to VPA zomato@hdfcbank (UPI Ref No 630155443322). Not you? Call 18002586161", spend("640.00", "INR", "2026-11-05", "1234", "zomato")),
            Text("BANK-IN", "2026-11-30 09:00", "INR 1,23,456.00 credited to A/c XX1234 on 30-Nov-26 by NEFT from ACME TECHNOLOGIES PVT LTD. Avl Bal INR 3,10,000.00", Want("income", "123456.00", "INR", "2026-11-30", "1234", "acme")),
            Text("BANK-IN", "2026-11-06 14:22", "Rs.1,299.00 spent on card XX5678 at MYNTRA on 06-Nov-26.", spend("1299.00", "INR", "2026-11-06", "5678", "myntra")),
        )),
    )

    private fun waitUntil(seconds: Int = 60, what: String, done: () -> Boolean) {
        val until = System.currentTimeMillis() + seconds * 1000L
        while (!done()) {
            check(System.currentTimeMillis() < until) { "Timed out waiting for $what" }
            Thread.sleep(200)
        }
    }
    private fun library(): List<JSONObject> = (NativeServices.call("library_snapshot") as JSONObject).getJSONArray("entries").let { a -> (0 until a.length()).map { a.getJSONObject(it) } }
    private fun open(): String? = library().firstOrNull { it.optBoolean("current") }?.text("id")
    private fun sms(text: Text): BankSms {
        val received = LocalDateTime.parse(text.at.replace(' ', 'T')).atZone(ZoneId.systemDefault()).toInstant().toEpochMilli()
        return BankSms(smsId(text.sender, received, text.body), text.sender, received, text.body)
    }

    /** What's wrong with an item for a text, or nothing. */
    private fun problems(want: Want, item: JSONObject?): List<String> {
        if (item == null) return listOf("no item")
        if (want.what.startsWith("skip:")) {
            val skip = want.what.removePrefix("skip:")
            return if (item.text("disposition") == "ignore" && item.text("skip") == skip) emptyList() else listOf("${item.text("disposition")}/${item.text("skip")} for $skip")
        }
        val t = item.optJSONObject("transaction") ?: return listOf("${item.text("disposition")} with no transaction")
        val out = mutableListOf<String>()
        val disposition = if (want.what == "pending") "pending" else "transaction"
        val kind = if (want.what == "pending") "expense" else want.what
        if (item.text("disposition") != disposition) out += "disposition ${item.text("disposition")}"
        fun digits(s: String) = s.filter(Char::isDigit)
        fun same(a: String, b: String?) = b == null || (a.isNotEmpty() && digits(a).endsWith(b))
        fun money(a: String?, b: String?) = if (a == null || b == null) a == b else runCatching { BigDecimal(a).compareTo(BigDecimal(b)) == 0 }.getOrDefault(false)
        val amount = if (t.isNull("amount")) null else t.text("amount")
        // A transfer may be read from either of its accounts' side.
        val flipped = kind == "transfer" && same(t.text("accountRef"), want.other) && same(t.text("otherRef"), want.ref) && money(amount, want.amount?.let { BigDecimal(it).negate().toPlainString() })
        if (t.text("kind") != kind) out += "kind ${t.text("kind")}"
        if (!flipped) {
            if (!money(amount, want.amount)) out += "amount $amount for ${want.amount}"
            if (!same(t.text("accountRef"), want.ref)) out += "account ${t.text("accountRef")} for ${want.ref}"
            if (want.other != null && !same(t.text("otherRef"), want.other)) out += "other ${t.text("otherRef")} for ${want.other}"
        }
        if (want.currency.isNotEmpty() && t.text("currency") != want.currency) out += "currency ${t.text("currency")}"
        if (want.date != null && t.text("date") != want.date) out += "date ${t.text("date")} for ${want.date}"
        if (want.payee != null && !t.text("payee").lowercase().contains(want.payee)) out += "payee ${t.text("payee")}"
        return out
    }

    @Test fun banksElsewhereAreReadAsTheirTextsSay() = runBlocking<Unit> {
        assumeTrue("Run only when asked: -e live 1", InstrumentationRegistry.getArguments().getString("live") == "1")
        Words.source = ResourceWords(context.resources)
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put("live", vm) }
        waitUntil(what = "the open budget") { vm.state.value.budget != null }
        // The services finish starting a moment after the budget shows.
        var connection = JSONObject()
        waitUntil(what = "the assistant's services") { runCatching { connection = NativeServices.call("assistant_status") as JSONObject }.isSuccess }
        assumeTrue("The assistant is connected", connection.optBoolean("ready"))
        val home = open()
        val made = mutableListOf<String>()
        val report = StringBuilder("Model: ${connection.text("provider")} ${connection.optString("model")}\n")
        val raw = JSONArray()
        var right = 0
        var total = 0
        try {
            for (bank in banks) {
                val name = "SMS check ${bank.name} ${System.currentTimeMillis() % 100000}"
                instrumentation.runOnMainSync { vm.createBudget(name, bank.currency, localOnly = true) }
                waitUntil(what = "budget $name") { vm.state.value.budget?.optString("name") == name }
                open()?.let(made::add)
                bank.accounts.forEach { (account, type) -> instrumentation.runOnMainSync { vm.saveAccount(null, account, AccountTypes.kindFor(type), 0, type) } }
                waitUntil(what = "accounts") { vm.state.value.budget!!.rows("accounts").size >= bank.accounts.size }
                val identity = vm.state.value.budgetIdentity!!
                var templates = JSONArray()
                for ((round, texts) in listOf(bank.first, bank.second).withIndex()) {
                    val messages = texts.map(::sms)
                    val request = JSONObject().put("budgetId", identity).put("provider", connection.getString("provider")).put("model", connection.opt("model") ?: JSONObject.NULL)
                        .put("messages", JSONArray(messages.map { it.json() })).put("note", "").put("templates", templates).put("payees", JSONArray()).put("trusted", JSONArray())
                    val began = System.currentTimeMillis()
                    val analysis = NativeSmsAnalyzer.analyze(request) {}
                    val seconds = (System.currentTimeMillis() - began) / 1000
                    val items = analysis.rows("items")
                    val byText = messages.associate { m -> m.id to items.firstOrNull { item -> (0 until item.getJSONArray("sourceIds").length()).any { item.getJSONArray("sourceIds").getString(it) == m.id } } }
                    var fromTemplates = 0
                    var roundRight = 0
                    val lines = mutableListOf<String>()
                    texts.zip(messages).forEach { (sent, m) ->
                        val item = byText[m.id]
                        val wrong = problems(sent.want, item)
                        if (item?.text("template")?.let { it.isNotBlank() && it != "filter:code" } == true) fromTemplates++
                        if (wrong.isEmpty()) roundRight++ else lines += "  x ${sent.body.take(70)}\n      ${wrong.joinToString("; ")}" +
                            (item?.optJSONObject("transaction")?.let { "\n      read: ${it.text("kind")} ${it.opt("amount")} ${it.text("currency")} ${it.text("date")} ${it.text("payee")} ref ${it.text("accountRef")} other ${it.text("otherRef")}" } ?: "")
                    }
                    right += roundRight; total += texts.size
                    val learned = analysis.optJSONObject("learning")?.optJSONArray("templates") ?: JSONArray()
                    report.append("${bank.name} round ${round + 1}: $roundRight of ${texts.size} right, $fromTemplates read by a template on the phone, ${learned.length()} formats learned, ${seconds}s\n")
                    lines.forEach { report.append(it).append('\n') }
                    for (i in 0 until learned.length()) report.append("  format: ${learned.getJSONObject(i).getJSONObject("template").getJSONArray("lines").join(" / ")}\n")
                    raw.put(JSONObject().put("bank", bank.name).put("round", round + 1).put("analysis", analysis))
                    if (round == 0) templates = JSONArray((0 until learned.length()).map { learned.getJSONObject(it).getJSONObject("template") })
                }
            }
            report.append("\nAll: $right of $total right\n")
        } catch (failure: Throwable) {
            report.append("\nStopped: ${failure.message}\n")
            throw failure
        } finally {
            // The budget that was open comes back, and the checks' budgets go to the Trash.
            if (home != null) {
                instrumentation.runOnMainSync { vm.service("library_open", JSONObject().put("id", home)) }
                runCatching { waitUntil(what = "the budget that was open") { open() == home && vm.state.value.budget != null } }
            }
            made.filter { it != home }.forEach { id ->
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
