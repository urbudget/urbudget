package app.urbudget.nativeapp

import kotlinx.coroutines.test.runTest
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class AccountTypesTest {
    private fun account(json: String) = JSONObject(json)

    @Test fun inferenceFollowsChosenThenImportedThenKindAndBalance() {
        assertEquals("savings", AccountTypes.infer(account("""{"kind":"cash","accountType":"savings"}""")))
        // An unknown chosen type falls through to the import and then to the kind.
        assertEquals("mortgage", AccountTypes.infer(account("""{"kind":"tracking","accountType":"weird","source":{"type":"mortgage"}}""")))
        assertEquals("otherDebt", AccountTypes.infer(account("""{"kind":"tracking","source":{"type":"otherLiability"}}""")))
        assertEquals("creditCard", AccountTypes.infer(account("""{"kind":"credit"}""")))
        assertEquals("checking", AccountTypes.infer(account("""{"kind":"cash","accountType":null}""")))
        // Untyped tracking accounts read as a debt or an asset by the sign of their balance.
        assertEquals("otherDebt", AccountTypes.infer(account("""{"kind":"tracking","openingBalance":-5000}""")))
        assertEquals("otherAsset", AccountTypes.infer(account("""{"kind":"tracking","openingBalance":-5000}"""), 100))
        assertEquals("otherAsset", AccountTypes.infer(account("""{"kind":"tracking","openingBalance":0}""")))
        // A non-string imported type is ignored.
        assertEquals("otherDebt", AccountTypes.infer(account("""{"kind":"tracking","source":{"type":7}}"""), -1))
    }

    @Test fun editingNeverMovesAnAccountToAnotherKind() {
        // A tracked card imported as a credit card stays a tracking account when edited.
        val trackedCard = account("""{"kind":"tracking","openingBalance":-2000,"source":{"type":"creditCard"}}""")
        assertEquals("otherDebt", AccountTypes.forEditing(trackedCard, -2000))
        assertEquals("tracking", AccountTypes.kindFor(AccountTypes.forEditing(trackedCard, -2000)))
        // An imported debt keeps its loan type rather than becoming an asset.
        assertEquals("autoLoan", AccountTypes.forEditing(account("""{"kind":"tracking","source":{"type":"autoLoan"}}"""), 500))
        assertEquals("lineOfCredit", AccountTypes.forEditing(account("""{"kind":"credit","accountType":"lineOfCredit"}"""), 0))
    }

    @Test fun loanTypesNetWorthAndBudgetMembership() {
        assertTrue(AccountTypes.isLoanType("mortgage"))
        assertTrue(AccountTypes.isLoanType("otherDebt"))
        assertFalse("Credit cards are liabilities in the budget, not loans", AccountTypes.isLoanType("creditCard"))
        assertFalse(AccountTypes.isLoanType("vehicle"))
        assertTrue(AccountTypes.inNetWorth(account("""{"kind":"tracking"}""")))
        assertTrue(AccountTypes.inNetWorth(account("""{"kind":"tracking","netWorth":null}""")))
        assertFalse(AccountTypes.inNetWorth(account("""{"kind":"tracking","netWorth":false}""")))
        assertTrue(AccountTypes.onBudget(account("""{"kind":"cash"}""")))
        assertFalse(AccountTypes.onBudget(account("""{"kind":"cash","onBudget":false}""")))
        assertFalse(AccountTypes.onBudget(account("""{"kind":"tracking"}""")))
        val asset = account("""{"id":"car","kind":"tracking","accountType":"vehicle"}""")
        assertFalse(AccountTypes.hasLoan(asset, 0, emptySet()))
        assertTrue("Saved loan details always count", AccountTypes.hasLoan(asset, 0, setOf("car")))
        assertTrue(AccountTypes.hasLoan(account("""{"id":"x","kind":"tracking"}"""), -100, emptySet()))
        assertFalse(AccountTypes.hasLoan(account("""{"id":"x","kind":"credit"}"""), -100, emptySet()))
    }

    @Test fun movingAnAccountKeepsEveryOtherInPlace() {
        val accounts = JSONArray("""[{"id":"a"},{"id":"b"},{"id":"c"},{"id":"d"}]""")
        fun ids(array: JSONArray) = (0 until array.length()).map { array.getJSONObject(it).getString("id") }
        assertEquals(listOf("a", "c", "b", "d"), ids(AccountTypes.moveAccount(accounts, "b", "c", after = true)))
        assertEquals(listOf("a", "c", "b", "d"), ids(AccountTypes.moveAccount(accounts, "c", "b", after = false)))
        // Neighbours in a section can be apart in the whole list.
        assertEquals(listOf("b", "c", "d", "a"), ids(AccountTypes.moveAccount(accounts, "a", "d", after = true)))
        assertEquals(listOf("a", "b", "c", "d"), ids(AccountTypes.moveAccount(accounts, "a", "missing", after = true)))
        assertEquals(4, accounts.length())
    }

    @Test fun badgesCountApprovalsAndSpendingThatNeedsACategory() = runTest {
        val budget = JSONObject("""{
          "accounts":[{"id":"cash","kind":"cash"},{"id":"card","kind":"credit"},{"id":"loan","kind":"tracking"},{"id":"off","kind":"cash","onBudget":false}],
          "categories":[{"id":"food","name":"Food"},{"id":"unc","name":"Uncategorized"}],
          "transactions":[
            {"id":"1","date":"2026-10-01","accountId":"cash","amount":-500,"approved":false,"splits":[{"categoryId":null,"amount":-500}]},
            {"id":"2","date":"2026-10-02","accountId":"cash","amount":-500,"splits":[{"categoryId":"food","amount":-500}]},
            {"id":"3","date":"2026-10-03","accountId":"card","amount":-200,"approved":false,"splits":[{"categoryId":"unc","amount":-200}]},
            {"id":"4","date":"2026-10-04","accountId":"cash","amount":900,"splits":[{"categoryId":null,"amount":900}]},
            {"id":"5","date":"2026-12-01","accountId":"cash","amount":-100,"splits":[{"categoryId":null,"amount":-100}]},
            {"id":"6","date":"2026-10-05","accountId":"loan","amount":300,"transferAccountId":"cash","splits":[{"categoryId":null,"amount":300}]},
            {"id":"7","date":"2026-10-05","accountId":"cash","amount":-50,"transferAccountId":"card","splits":[{"categoryId":null,"amount":-50}]},
            {"id":"8","date":"2026-10-06","accountId":"cash","amount":-40,"payee":"Reconciliation adjustment","splits":[{"categoryId":null,"amount":-40}]},
            {"id":"9","date":"2026-10-06","accountId":"cash","amount":-30,"splits":[{"categoryId":null,"unassigned":true,"amount":-30}]}
          ]}""")
        val badges = accountBadges(budget, "2026-10-21")
        assertEquals(mapOf("cash" to 1, "card" to 1), badges.approvals)
        // 1: no category; 3: the import placeholder; 6: a tracking account's money taken from cash.
        // 2 is categorized, 4 is income, 5 is in the future, 7 moves money within the budget, 8 and 9 are left unassigned.
        assertEquals(mapOf("cash" to 2, "card" to 1), badges.uncategorized)
    }

    /** The store writes `unassigned` from `source` on every read, so a copy without `source` answers the same. */
    @Test fun badgesOfALightCopyAreThoseOfTheWholeOne() = runTest {
        fun budget(inflow: String, spent: String) = JSONObject("""{
          "accounts":[{"id":"cash","kind":"cash"}],
          "categories":[{"id":"food","name":"Food"}],
          "transactions":[
            {"id":"1","date":"2026-10-01","accountId":"cash","amount":-500,$inflow,
            {"id":"2","date":"2026-10-02","accountId":"cash","amount":-300,$spent
          ]}""")
        val whole = budget(""""source":{"category_name":"Inflow: Ready to Assign"},"splits":[{"categoryId":null,"amount":-500}]}""",
            """"source":{"category_name":"Food"},"splits":[{"categoryId":null,"amount":-300}]}""")
        val light = budget(""""hasImportSource":true,"splits":[{"categoryId":null,"unassigned":true,"amount":-500}]}""",
            """"hasImportSource":true,"splits":[{"categoryId":null,"unassigned":false,"amount":-300}]}""")
        assertEquals(mapOf("cash" to 1), accountBadges(whole, "2026-10-21").uncategorized)
        assertEquals(accountBadges(whole, "2026-10-21"), accountBadges(light, "2026-10-21"))
    }
}
