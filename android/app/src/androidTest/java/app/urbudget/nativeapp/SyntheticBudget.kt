package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.YearMonth
import java.util.Collections
import java.util.Locale
import java.util.Random

/**
 * A made-up budget of a real person's size and untidiness: the same for a seed and a day, shaped on
 * desktop/src-tauri/tests/performance.rs, with what `native-roundtrip.json` leaves out (schedules,
 * `source`, every shape of transaction) and text in more scripts than one bank's.
 */
object SyntheticBudget {
    private val groups = listOf("Bills", "Everyday", "Transport", "Home", "Health", "Fun", "Family", "Giving", "Goals", "Debts")
    private val payees = listOf(
        "Carrefour", "Tesco 🛒", "Uber 🚗", "Amazon", "Spotify", "Shell", "IKEA", "Zara", "Pharmacy", "Gym",
        "بقالة الحي", "مطعم البيك", "شركة الكهرباء", "صيدلية النهدي", "بنزين",
        "セブン-イレブン", "すき家", "東京ガス", "ユニクロ", "美团外卖", "星巴克",
        "Café Müller", "Café Noir", "São Paulo Açaí", "Żabka", "Ñandú ☕", "Ÿ̈ market", "🍕 Pizza",
    )
    private val memos = listOf(
        "", "", "Weekly shop", "Shared with Sam", "Paid in cash", "Bought online",
        "غداء مع الفريق", "فاتورة الشهر", "交通费", "月末の精算", "Café with Zoë", "Birthday 🎂",
    )
    private val flags = listOf("red", "orange", "yellow", "green", "blue", "purple")
    private val frequencies = listOf("monthly", "weekly", "everyOtherMonth", "yearly")

    private fun JSONObject.orNull(key: String, value: Any?) = put(key, value ?: JSONObject.NULL)
    private fun Random.of(items: List<String>) = items[nextInt(items.size)]
    private fun uuid(kind: Int, n: Int) = "%08x-5b7d-4c3a-9e6f-%012x".format(Locale.ROOT, 0x4f2c9a1e + kind, n)
    /** Western digits, whatever the phone's language: these end up in ids and times. */
    private fun padded(n: Int, width: Int) = n.toString().padStart(width, '0')

    private fun account(id: String, name: String, kind: String, opening: Long, archived: Boolean = false) = JSONObject()
        .put("id", id).put("name", name).put("kind", kind).put("openingBalance", opening).put("archived", archived)
        .put("note", "").put("futureAccountField", "keep:$id")

    private fun category(id: String, name: String, group: String, target: Long) = JSONObject()
        .put("id", id).put("name", name).put("group", group).put("target", target).put("futureCategoryField", JSONObject().put("keep", id))

    /** What YNAB's export says about a transaction, most of an imported one's size. */
    private fun source(n: Int, date: LocalDate, amount: Long, account: String, payee: String, category: String?, parts: Int) = JSONObject()
        .put("id", uuid(1, n)).put("plan_id", uuid(6, 1)).put("date", date.toString()).put("amount", amount * 10)
        .put("memo", "Representative imported metadata").put("cleared", "cleared").put("approved", true)
        .orNull("flag_color", null).orNull("flag_name", null)
        .put("account_id", uuid(2, account.hashCode() and 0xffff)).put("account_name", account).put("account_type", "checking")
        .put("payee_id", uuid(3, payee.hashCode() and 0xffff)).put("payee_name", payee)
        .put("category_id", uuid(4, (category ?: "").hashCode() and 0xffff)).orNull("category_name", category)
        .put("category_group_id", uuid(7, (category ?: "").length)).put("category_group_name", "Everyday")
        .orNull("transfer_account_id", null).orNull("transfer_transaction_id", null).orNull("matched_transaction_id", null)
        .put("import_id", "YNAB:${amount * 10}:$date:1").put("import_payee_name", payee).put("import_payee_name_original", payee)
        .orNull("debt_transaction_type", null).put("deleted", false).put("server_knowledge", 100_000 + n).put("updated_at", "${date}T09:12:00.000Z")
        .put("subtransactions", JSONArray((0 until parts).map { JSONObject().put("id", uuid(5, n * 8 + it)).put("deleted", false) }))

    private fun part(category: String?, amount: Long) = JSONObject().orNull("categoryId", category).put("amount", amount)

    /** [amount] in [count] parts, every one of them more than nothing. */
    private fun parts(amount: Long, count: Int): List<Long> {
        val each = amount / count
        return List(count) { if (it == 0) amount - each * (count - 1) else each }
    }

    private fun schedule(n: Int, random: Random, next: LocalDate?, anchor: LocalDate, frequency: String, paused: Boolean, category: String, unit: Long): JSONObject {
        val amount = -(1000L + random.nextInt(90_000)) * unit
        val transaction = JSONObject().put("id", "template-$n").put("date", anchor.toString()).put("accountId", "checking").put("payee", random.of(payees)).put("memo", "")
            .put("amount", amount).put("status", "uncleared").put("splits", JSONArray().put(part(category, amount).put("customSplit", "keep")))
            .orNull("transferAccountId", null).orNull("transferStatus", null).put("customTransaction", "keep")
        return JSONObject().put("id", "schedule-$n").orNull("nextDate", next?.toString()).put("anchorDate", anchor.toString()).put("frequency", frequency)
            .put("paused", paused).put("customSchedule", JSONObject().put("origin", "desktop")).put("transaction", transaction)
    }

    /** Throws if the engine wouldn't take the budget, or it's far from a real one's size. */
    fun build(transactions: Int = 3000, seed: Long = 1, today: LocalDate = LocalDate.now(), currency: String = "EUR"): JSONObject {
        val random = Random(seed)
        val unit = if (currency == "JPY") 100L else 1L // Whole yen, in hundreds a person would see.
        val budget = JSONObject().put("schemaVersion", 2).put("name", "Synthetic budget").put("currency", currency)
            .put("futureBudgetField", JSONObject().put("keep", JSONArray(listOf(1, 2, 3))))
        budget.put("accounts", JSONArray(listOf(
            account("checking", "Everyday الحساب", "cash", 90_000_000 * unit),
            account("wallet", "Wallet 財布", "cash", 200_000 * unit),
            account("savings", "Savings", "cash", 100_000_000 * unit),
            account("visa", "Visa", "credit", -120_000 * unit),
            account("amex", "Amex", "credit", -60_000 * unit),
            account("brokerage", "Brokerage", "tracking", 5_000_000 * unit),
            account("car-loan", "Car loan", "tracking", -9_000_000 * unit),
            account("old-bank", "Closed bank", "cash", 0, archived = true),
        )))

        val categories = mutableListOf<JSONObject>()
        groups.forEachIndexed { g, group ->
            repeat(6) { n ->
                val id = "c-${group.lowercase()}-$n"
                val c = category(id, "$group ${n + 1}", group, if (n % 3 == 0) (5_000L + 2_500 * n) * unit else 0)
                if ((g + n) % 13 == 12) c.put("hidden", true)
                if (n == 1) c.put("note", "A note on $group, ملاحظة")
                when ((g + n) % 7) {
                    1 -> c.put("goal", JSONObject().put("goal_type", "NEED").put("goal_cadence", 1).put("goal_cadence_frequency", 1).put("goal_day", 1 + g))
                    3 -> c.put("goal", JSONObject().put("goal_type", "TB").put("goal_target_month", today.plusMonths(6L + g).toString()))
                    5 -> c.put("goal", JSONObject().put("goal_type", "MF").put("goal_snoozed_at", JSONObject.NULL))
                }
                categories += c
            }
        }
        val spending = categories.map { it.getString("id") }.filter { it != "c-debts-0" && it != "c-goals-0" }
        categories += category("pay-visa", "Visa", "Credit Card Payments", 0).put("paymentAccountId", "visa")
        categories += category("pay-amex", "Amex", "Credit Card Payments", 0).put("paymentAccountId", "amex")
        budget.put("categories", JSONArray(categories))
        budget.put("categoryOrder", JSONArray(categories.map { it.getString("id") }))
        budget.put("groupOrder", JSONArray(groups + "Credit Card Payments"))

        val thisMonth = YearMonth.from(today)
        val months = (23 downTo 0).map { thisMonth.minusMonths(it.toLong()) }
        val assignments = JSONArray()
        months.forEachIndexed { m, month ->
            categories.forEachIndexed { c, category ->
                val row = JSONObject().put("month", month.toString()).put("categoryId", category.getString("id")).put("amount", (2_000L + (m * 37 + c * 53) % 40_000) * unit)
                if (c % 7 == 0) row.put("futureAssignmentField", "keep")
                assignments.put(row)
            }
        }
        budget.put("assignments", assignments)

        val rows = ArrayList<JSONObject>(transactions)
        for (i in 0 until transactions) {
            // About two years back, and now and then a row dated ahead.
            val date = if (i % 97 == 96) today.plusDays(1L + i % 45) else today.minusDays(i.toLong() * 730 / transactions + random.nextInt(3))
            val account = when (i % 5) { 0, 1 -> "checking"; 2 -> "wallet"; 3 -> "visa"; else -> "amex" }
            val payee = random.of(payees)
            val category = random.of(spending)
            val status = when {
                i % 97 == 96 -> "uncleared"
                date < today.minusDays(540) -> "reconciled"
                i % 11 == 0 -> "uncleared"
                else -> "cleared"
            }
            var amount = -(100L + random.nextInt(49_900)) * unit
            val t = JSONObject().put("id", "t-${padded(i, 5)}").put("date", date.toString()).put("accountId", account).put("payee", payee)
                .put("memo", random.of(memos)).put("status", status).orNull("transferAccountId", null).orNull("transferStatus", null)
            var named: String? = category
            var pieces = 0
            val splits = JSONArray()
            fun transfer(to: String) = t.put("transferAccountId", to).put("transferStatus", status)
            when (i % 40) {
                0, 20 -> { // Pay, into the main account.
                    amount = (300_000L + random.nextInt(500_000)) * unit
                    t.put("accountId", "checking").put("payee", "Employer")
                    named = "Inflow: Ready to Assign"
                    splits.put(part(null, amount).put("unassigned", true))
                }
                1, 21 -> { t.put("accountId", "checking"); transfer("savings") } // Between cash accounts: no categories.
                2 -> { t.put("accountId", "checking"); transfer("visa") }
                22 -> { t.put("accountId", "checking"); transfer("amex") }
                3 -> { // Off budget, so it needs a category.
                    t.put("accountId", "checking"); transfer("brokerage")
                    splits.put(part("c-goals-0", amount))
                }
                23 -> { t.put("accountId", "checking"); transfer("car-loan"); splits.put(part("c-debts-0", amount)) }
                4, 14, 24, 34 -> { // Two to five parts.
                    pieces = 2 + i / 4 % 4
                    amount = amount.coerceAtMost(-1000L * unit)
                    parts(amount, pieces).forEachIndexed { p, money ->
                        val leg = part(random.of(spending), money)
                        if (p == 0) leg.put("futureSplitField", "keep")
                        if (i % 8 == 4) leg.put("source", JSONObject().put("id", uuid(5, i * 8 + p)).put("memo", "A part of an import").put("amount", money * 10))
                        splits.put(leg)
                    }
                }
                5, 25 -> { // A part of it moved to another account.
                    t.put("accountId", "checking")
                    val (kept, moved) = parts(amount.coerceAtMost(-1000L * unit), 2)
                    amount = kept + moved
                    splits.put(part(category, kept))
                    splits.put(part(null, moved).put("transferAccountId", "savings").put("transferStatus", status))
                }
                6 -> { amount = -amount; splits.put(part(category, amount)) } // A refund.
                7 -> { // Came in as income, and someone cleared its category.
                    amount = -amount
                    named = "Inflow: Ready to Assign"
                    splits.put(part(null, amount))
                }
                8 -> splits.put(part(null, amount).put("unassigned", false)) // Not categorized yet.
                else -> {
                    val leg = part(category, amount)
                    if (i % 9 == 0) leg.put("futureSplitField", "keep")
                    splits.put(leg)
                }
            }
            t.put("amount", amount).put("splits", splits)
            if (i % 83 != 0) t.put("createdAt", "${date}T09:${padded(i % 60, 2)}:00.000Z") // The oldest kind of row has no time.
            if (i % 31 == 0) t.put("approved", false)
            if (i % 19 == 0) t.put("flag", flags[i % flags.size])
            if (i % 5 == 0 && i % 97 != 96) t.put("bankImportIds", JSONArray().put("bank:${padded(i, 6)}"))
            if (i % 5 == 0) t.put("futureTransactionField", JSONArray().put("keep").put(i))
            // A row dated ahead has to be able to become a schedule, which an import can't.
            if (i % 10 != 9 && i % 97 != 96) t.put("source", source(i, date, amount, t.getString("accountId"), t.getString("payee"), named, pieces))
            rows += t
        }
        // Rows are added in any order, not by date.
        Collections.shuffle(rows, random)
        budget.put("transactions", JSONArray(rows))

        val bills = "c-bills-1"
        budget.put("schedules", JSONArray((0 until 12).map { n ->
            val frequency = frequencies[n % frequencies.size]
            when (n % 6) {
                0, 1 -> today.minusDays(n + 1L).let { schedule(n, random, it, it, frequency, false, bills, unit) } // Due.
                2, 3 -> today.plusDays(n * 9L).let { schedule(n, random, it, it, frequency, false, bills, unit) }
                4 -> today.plusDays(3L + n).let { schedule(n, random, it, it, frequency, true, bills, unit) } // Paused.
                else -> schedule(n, random, null, today.minusDays(90L), "never", false, bills, unit) // Finished.
            }
        }))
        budget.put("loans", JSONArray().put(JSONObject().put("accountId", "car-loan").put("categoryId", "c-debts-0").put("annualRateBasisPoints", 690)
            .put("monthlyPayment", 45_000 * unit).put("monthlyFees", 500 * unit).put("rateHistory", JSONArray()).put("futureLoanField", "keep")))
        budget.put("payees", JSONArray(payees.mapIndexed { n, name ->
            JSONObject().put("id", "payee-$n").put("name", name).orNull("categoryId", spending.getOrNull(n)).put("futurePayeeField", "keep")
        }))
        budget.put("payeeRules", JSONArray((0 until 8).map { n ->
            JSONObject().put("id", "rule-$n").put("match", payees[n]).put("payee", payees[n]).put("categoryId", spending[n]).put("futureRuleField", "keep")
        }))
        budget.put("ynabMonths", JSONArray(months.mapIndexed { m, month -> JSONObject().put("month", "$month-01").put("note", if (m % 4 == 0) "A note for $month" else "") }))
        budget.put("moneyMoves", JSONArray((0 until 40).map { n ->
            val month = months[n % months.size]
            JSONObject().put("at", "$month-10T08:${padded(n, 2)}:00+00:00").put("month", month.toString()).put("description", "Updated assigned amount for ${spending[n]}")
                .put("kind", "assign").put("changes", JSONArray().put(JSONObject().put("categoryId", spending[n]).put("amount", 100 * unit)))
        }))
        budget.put("expectedIncome", 700_000 * unit)
        budget.put("flagNames", JSONObject().put("red", "Urgent").put("blue", "Reimbursable").put("green", "مراجعة"))
        budget.put("smsImportedMessageIds", JSONArray((0 until 50).map { "sms:%08x".format(Locale.ROOT, random.nextInt()) }))
        budget.put("sourceExport", JSONObject().put("data", JSONObject().put("server_knowledge", 7).put("plan", JSONObject().put("categories", JSONArray()))))

        val reply = JSONObject(NativeBridge.invoke(JSONObject().put("version", 1).put("method", "validate").put("budget", budget).put("params", JSONObject()).toString()))
        check(reply.optBoolean("ok")) { reply.optString("error") }
        // Only a real-sized budget: in a small one, the months and categories outweigh the ledger.
        val bytes = budget.toString().toByteArray().size
        if (transactions >= 2_000) check(bytes in transactions * 1_000L..transactions * 1_270L) { "$bytes bytes for $transactions transactions" }
        return budget
    }
}
