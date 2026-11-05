package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class BudgetPatchTest {
    private val cases = JSONArray(javaClass.classLoader!!.getResourceAsStream("patch-cases.json")!!.bufferedReader().use { it.readText() })
    private val base: JSONObject get() = cases.getJSONObject(0).getJSONObject("before")

    @Test fun theCasesEveryClientShares() {
        assertTrue(cases.length() > 10)
        for (i in 0 until cases.length()) {
            val case = cases.getJSONObject(i)
            val before = case.getJSONObject("before")
            val text = before.toString()
            val after = applyBudgetPatch(before, case.getJSONObject("patch"))
            assertTrue(case.getString("name"), sameJson(case.getJSONObject("after"), after))
            assertEquals(case.getString("name"), text, before.toString())
        }
    }

    @Test fun anEmptyPatchReturnsTheBudget() {
        val budget = base
        assertSame(budget, applyBudgetPatch(budget, JSONObject()))
    }

    @Test fun untouchedRecordsStayTheSameObjects() {
        val budget = base
        val row = budget.records("transactions")[1]
        val patch = JSONObject().put("transactions", JSONObject().put("delete", JSONArray())
            .put("upsert", JSONArray().put(JSONObject(row.toString()).put("memo", "Edited"))))
        val after = applyBudgetPatch(budget, patch)
        assertNotSame(budget, after)
        assertSame(budget.records("transactions")[0], after.records("transactions")[0])
        assertSame(budget.records("transactions")[2], after.records("transactions")[2])
        assertNotSame(row, after.records("transactions")[1])
        // A collection the patch doesn't mention is the same array.
        assertSame(budget.getJSONArray("accounts"), after.getJSONArray("accounts"))
        assertSame(budget.getJSONArray("assignments"), after.getJSONArray("assignments"))
        assertSame(budget.get("futureBudgetField"), after.get("futureBudgetField"))
    }

    @Test fun thePreviousRootsTextDoesNotChange() {
        for (i in 0 until cases.length()) {
            val case = cases.getJSONObject(i)
            val previous = JSONObject(case.getJSONObject("before").toString())
            val text = previous.toString()
            val arrays = listOf("accounts", "categories", "transactions", "assignments").map { previous.getJSONArray(it) }
            val after = applyBudgetPatch(previous, case.getJSONObject("patch"))
            assertEquals(case.getString("name"), text, previous.toString())
            assertEquals(listOf("accounts", "categories", "transactions", "assignments").map { previous.getJSONArray(it) }, arrays)
            // Changing the new copy reaches nothing the old one holds.
            if (after !== previous) after.put("name", "Changed afterwards")
            assertEquals(case.getString("name"), text, previous.toString())
        }
    }

    /** A budget with some of everything an `edit { }` touches. Parsed fresh, as the app's root is. */
    private val full: JSONObject get() = JSONObject("""{"schemaVersion":2,"name":"Budget","currency":"EUR","groupOrder":["Bills","Fun"],"flagNames":{"red":"Tax"},
        "numberFormat":{"decimal":".","group":","},"accounts":[{"id":"a1","name":"Cash","kind":"cash","openingBalance":0,"archived":false},{"id":"a2","name":"Card","kind":"credit","openingBalance":0,"archived":false},{"id":"a3","name":"Old","kind":"cash","openingBalance":0,"archived":true}],
        "categories":[{"id":"c1","name":"Rent","group":"Bills","target":100,"goal":{"goal_amount":100}},{"id":"c2","name":"Fun","group":"Fun","target":0}],
        "assignments":[{"month":"2026-10","categoryId":"c1","amount":100},{"month":"2026-11","categoryId":"c1","amount":100}],
        "transactions":[{"id":"t1","memo":"","amount":-100,"source":{"payee_name":"Shop"}},{"id":"t2","memo":"","amount":-200},{"id":"t3","memo":"","amount":-300}],
        "schedules":[{"id":"s1","paused":false},{"id":"s2","paused":false}],"loans":[{"accountId":"a3","categoryId":"c1"}],
        "payeeRules":[{"id":"r1","match":"Shop"}],"payees":[{"id":"p1","name":"Shop"}]}""")

    private fun deleteRecord(budget: JSONObject, collection: String, identity: String, id: String) {
        budget.put(collection, JSONArray(budget.records(collection).filter { it.optString(identity) != id }))
    }

    /** Every shape of transform `BudgetViewModel` and the screens hand `edit { }`. */
    private val transforms: Map<String, (JSONObject) -> Unit> = mapOf(
        "upsert a new account" to { b -> b.upsertRecord("accounts", JSONObject().put("id", "a4").put("name", "New")) },
        "archive an account by its id alone" to { b -> b.upsertRecord("accounts", JSONObject().put("id", "a1").put("archived", true)) },
        "upsert a category" to { b -> b.upsertRecord("categories", JSONObject().put("id", "c2").put("hidden", true)) },
        "upsert a new category" to { b -> b.upsertRecord("categories", JSONObject().put("id", "c3").put("name", "New").put("group", "Fun").put("target", 0)) },
        "upsert a transaction" to { b -> b.upsertRecord("transactions", JSONObject().put("id", "t2").put("memo", "Edited")) },
        "upsert a new transaction" to { b -> b.upsertRecord("transactions", JSONObject().put("id", "t4").put("memo", "")) },
        "upsert a schedule" to { b -> b.upsertRecord("schedules", JSONObject().put("id", "s1").put("paused", true)) },
        "upsert a new schedule" to { b -> b.upsertRecord("schedules", JSONObject().put("id", "s3")) },
        "upsert a loan" to { b -> b.upsertRecord("loans", JSONObject().put("accountId", "a3").put("categoryId", "c2")) },
        "upsert a payee rule" to { b -> b.upsertRecord("payeeRules", JSONObject().put("id", "r2").put("match", "Cafe")) },
        "upsert a payee" to { b -> b.upsertRecord("payees", JSONObject().put("id", "p1").put("name", "Shop Ltd")) },
        "upsert many" to { b -> listOf("a1", "a2").forEach { b.upsertRecord("accounts", JSONObject().put("id", it).put("note", "Both")) } },
        "remove an account" to { b -> deleteRecord(b, "accounts", "id", "a2") },
        "remove a category" to { b -> deleteRecord(b, "categories", "id", "c2") },
        "remove a transaction" to { b -> deleteRecord(b, "transactions", "id", "t2") },
        "remove a schedule" to { b -> deleteRecord(b, "schedules", "id", "s1") },
        "remove a loan" to { b -> deleteRecord(b, "loans", "accountId", "a3") },
        "remove a payee rule" to { b -> deleteRecord(b, "payeeRules", "id", "r1") },
        "save a credit account with its category" to { b ->
            b.upsertRecord("accounts", JSONObject().put("id", "a5").put("name", "Card 2").put("kind", "credit"))
            b.upsertRecord("categories", JSONObject().put("id", "c4").put("name", "Card 2").put("group", "Credit Card Payments").put("target", 0).put("paymentAccountId", "a5"))
        },
        "set a target" to { b ->
            val record = b.records("categories").first { it.optString("id") == "c2" }
            val next = JSONObject(record.toString()).put("goal", JSONObject().put("goal_amount", 500)).put("target", 500)
            b.put("categories", JSONArray(b.records("categories").map { if (it === record) next else it }))
        },
        "remove a target" to { b ->
            val record = b.records("categories").first { it.optString("id") == "c1" }
            val next = JSONObject(record.toString()).put("target", 0)
            next.remove("goal")
            b.put("categories", JSONArray(b.records("categories").map { if (it === record) next else it }))
        },
        "move an account" to { b -> b.put("accounts", AccountTypes.moveAccount(b.getJSONArray("accounts"), "a1", "a2", true)) },
        "move an account nowhere" to { b -> b.put("accounts", AccountTypes.moveAccount(b.getJSONArray("accounts"), "a1", "a1", true)) },
        "budget settings" to { b ->
            b.put("name", "Renamed").put("currency", "USD").put("numberFormat", JSONObject().put("decimal", ",").put("group", ".")).put("dateFormat", "dmy")
        },
        "flag names" to { b -> b.put("flagNames", JSONObject().put("blue", "Work")) },
        "no flag names" to { b -> b.remove("flagNames") },
        "a new group" to { b ->
            val order = b.getJSONArray("groupOrder")
            b.put("groupOrder", JSONArray((0 until order.length()).map { order.optString(it) } + "New"))
        },
        "the first group order" to { b -> b.put("groupOrder", JSONArray(listOf("Bills", "Fun", "New"))) },
        "an empty group gone" to { b ->
            val order = b.getJSONArray("groupOrder")
            b.put("groupOrder", JSONArray((0 until order.length()).map { order.optString(it) }.filter { it != "Fun" }))
        },
        "a repeating transaction's schedule" to { b -> b.upsertRecord("schedules", JSONObject().put("id", "s4").put("transaction", JSONObject().put("id", "s4")).put("nextDate", "2026-12-01").put("frequency", "monthly")) },
        "an assignment changed" to { b ->
            val rows = b.records("assignments")
            b.put("assignments", JSONArray(rows.map { if (it.optString("month") == "2026-10") JSONObject(it.toString()).put("amount", 7) else it }))
        },
        "an assignment removed and one added" to { b ->
            val rows = b.records("assignments").filter { it.optString("month") != "2026-10" }
            b.put("assignments", JSONArray(rows + JSONObject().put("month", "2026-12").put("categoryId", "c2").put("amount", 5)))
        },
        "an order of its own" to { b -> b.put("transactions", JSONArray(b.records("transactions").reversed())) },
    )

    @Test fun thereIsAPatchThatMakesEachTransformsBudget() {
        for ((name, transform) in transforms) {
            val base = full
            val copy = base.shallowCopy()
            transform(copy)
            val patch = budgetPatch(base, copy)
            assertTrue(name, patch.length() > 0 || name == "move an account nowhere")
            assertTrue("$name: ${applyBudgetPatch(base, patch)}", sameJson(copy, applyBudgetPatch(base, patch)))
        }
    }

    @Test fun noTransformChangesWhatItWasGiven() {
        for ((name, transform) in transforms) {
            val base = full
            val text = base.toString()
            val copy = base.shallowCopy()
            transform(copy)
            budgetPatch(base, copy)
            assertEquals(name, text, base.toString())
        }
    }

    @Test fun aPatchHoldsOnlyWhatChanged() {
        val base = full
        val copy = base.shallowCopy()
        copy.upsertRecord("transactions", JSONObject().put("id", "t2").put("memo", "Edited"))
        val patch = budgetPatch(base, copy)
        assertEquals(setOf("transactions"), patch.keys().asSequence().toSet())
        val change = patch.getJSONObject("transactions")
        assertEquals(setOf("upsert", "delete"), change.keys().asSequence().toSet())
        assertEquals(listOf("t2"), change.getJSONArray("upsert").let { rows -> (0 until rows.length()).map { rows.getJSONObject(it).getString("id") } })
        assertEquals(0, change.getJSONArray("delete").length())
        assertEquals("Records go whole", -200, change.getJSONArray("upsert").getJSONObject(0).getInt("amount"))
    }

    @Test fun nothingChangedIsAnEmptyPatch() {
        val base = full
        assertEquals(0, budgetPatch(base, base.shallowCopy()).length())
        val rebuilt = base.shallowCopy()
        for (key in listOf("accounts", "categories", "transactions", "assignments", "schedules")) rebuilt.put(key, JSONArray(base.records(key)))
        val patch = budgetPatch(base, rebuilt)
        assertEquals("A rebuilt array of the same rows is no change, but an extra can't be told by identity", setOf("extras"), patch.keys().asSequence().toSet())
        assertTrue(sameJson(rebuilt, applyBudgetPatch(base, patch)))
    }

    @Test fun aRecordCopiedAndChangedIsFound() {
        val base = full
        val copy = base.shallowCopy()
        val rows = base.records("transactions")
        copy.put("transactions", JSONArray(rows.map { if (it.optString("id") == "t3") JSONObject(it.toString()).put("memo", "Changed") else it }))
        val upsert = budgetPatch(base, copy).getJSONObject("transactions").getJSONArray("upsert")
        assertEquals(1, upsert.length())
        assertEquals("Changed", upsert.getJSONObject(0).getString("memo"))
    }

    @Test fun anOrderIsSentOnlyWhenRecordsMoved() {
        val base = full
        val removed = base.shallowCopy()
        removed.put("transactions", JSONArray(base.records("transactions").filter { it.optString("id") != "t2" }))
        assertFalse(budgetPatch(base, removed).getJSONObject("transactions").has("order"))
        val added = base.shallowCopy()
        added.put("transactions", JSONArray(base.records("transactions") + JSONObject().put("id", "t4")))
        assertFalse(budgetPatch(base, added).getJSONObject("transactions").has("order"))
        val moved = base.shallowCopy()
        moved.put("transactions", JSONArray(base.records("transactions").reversed()))
        val change = budgetPatch(base, moved).getJSONObject("transactions")
        assertEquals("[\"t3\",\"t2\",\"t1\"]", change.getJSONArray("order").toString())
        assertEquals(0, change.getJSONArray("upsert").length())
    }

    @Test fun anAssignmentGoneIsDeletedByItsKey() {
        val base = full
        val copy = base.shallowCopy()
        copy.put("assignments", JSONArray(base.records("assignments").filter { it.optString("month") != "2026-10" }))
        val change = budgetPatch(base, copy).getJSONObject("assignments")
        assertEquals(0, change.getJSONArray("upsert").length())
        assertTrue(sameJson(JSONObject().put("month", "2026-10").put("categoryId", "c1"), change.getJSONArray("delete").getJSONObject(0)))
    }

    @Test fun anExtraRemovedIsDeletedAndOneAddedIsSet() {
        val base = full
        val copy = base.shallowCopy()
        copy.remove("flagNames")
        copy.put("expectedIncome", 5)
        val extras = budgetPatch(base, copy).getJSONObject("extras")
        assertEquals("[\"flagNames\"]", extras.getJSONArray("delete").toString())
        assertEquals(5, extras.getJSONObject("set").getInt("expectedIncome"))
        assertEquals(1, extras.getJSONObject("set").length())
    }

    @Test fun anAmountPastWhatADoubleHoldsComesThroughExactly() {
        val patch = JSONObject("""{"transactions":{"upsert":[{"id":"t1","amount":9007199254740993}],"delete":[]}}""")
        val after = applyBudgetPatch(base, patch)
        assertEquals(9_007_199_254_740_993L, after.records("transactions")[0].getLong("amount"))
    }

    @Test fun sameBudgetIgnoresTheOrderOfAssignmentsAndNothingElse() {
        val budget = base
        val rows = budget.records("assignments")
        val reversed = JSONObject(budget.toString()).put("assignments", JSONArray(rows.reversed().map { JSONObject(it.toString()) }))
        assertTrue(sameBudget(budget, reversed))
        val moved = JSONObject(budget.toString()).put("transactions", JSONArray(budget.records("transactions").reversed().map { JSONObject(it.toString()) }))
        assertFalse(sameBudget(budget, moved))
        val accounts = JSONObject(budget.toString()).put("accounts", JSONArray(budget.records("accounts").reversed().map { JSONObject(it.toString()) }))
        assertFalse(sameBudget(budget, accounts))
        val amount = JSONObject(reversed.toString())
        amount.getJSONArray("assignments").getJSONObject(0).put("amount", 1)
        assertFalse(sameBudget(budget, amount))
        assertFalse(sameBudget(budget, JSONObject(budget.toString()).put("name", "Other")))
        assertFalse(sameBudget(budget, JSONObject(budget.toString()).put("newField", true)))
    }
}
