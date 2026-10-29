package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.describeSyncConflict
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SyncConflictSummaryTest {
    @Test fun monthlyConflictNamesAmountsAndCategoryImpact() {
        EnglishWords.install()
        val plan = JSONObject("""{"type":"monthPlan","currency":"USD","baseTotal":10000,"total":12500,"readyToAssignChange":-2500,"moveCount":2,"categories":[{"name":"Food","base":10000,"amount":12500,"delta":2500}]}""")
        val text = describeSyncConflict(plan)
        assertTrue(text.contains("Month total: " + Money.format(10000, "USD") + " → " + Money.format(12500,"USD")))
        assertTrue(text.contains("Ready to Assign change: " + Money.format(-2500,"USD")))
        assertTrue(text.contains("Moves: 2"))
        assertTrue(text.contains("Food: "))
        assertTrue(text.contains("Change: " + Money.format(2500,"USD")))
    }
    @Test fun groupedMoneyShowsSourceAccountAndBothReconciliationStates() {
        EnglishWords.install()
        val value = JSONObject("""{"account":"Checking","amount":-1200,"lines":[],"status":"reconciled","transfer":"Savings","transferStatus":"cleared"}""")
        val text = describeSyncConflict(value)
        assertTrue(text.contains("Checking"))
        assertTrue(text.contains("Reconciled"))
        assertTrue(text.contains("Transfer: Savings"))
        assertTrue(text.contains("Transfer status: Cleared"))
    }
}
