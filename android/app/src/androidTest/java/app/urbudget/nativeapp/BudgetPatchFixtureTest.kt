package app.urbudget.nativeapp

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** The shared patch cases on Android's own `org.json`, a different class from the unit tests'. */
@RunWith(AndroidJUnit4::class)
class BudgetPatchFixtureTest {
    private val cases = JSONArray(InstrumentationRegistry.getInstrumentation().context.assets.open("patch-cases.json").bufferedReader().use { it.readText() })

    @Test fun theCasesEveryClientShares() {
        assertTrue(cases.length() > 10)
        for (i in 0 until cases.length()) {
            val case = cases.getJSONObject(i)
            val before = case.getJSONObject("before")
            val text = before.toString()
            val after = applyBudgetPatch(before, case.getJSONObject("patch"))
            assertSameJson(case.getString("name"), case.getJSONObject("after"), after)
            assertEquals(case.getString("name"), text, before.toString())
        }
    }

    @Test fun untouchedRecordsAreTheSameObjects() {
        val case = cases.getJSONObject(1)
        val before = case.getJSONObject("before")
        val after = applyBudgetPatch(before, case.getJSONObject("patch"))
        assertSame(before.getJSONArray("transactions").get(0), after.getJSONArray("transactions").get(0))
        assertNotSame(before.getJSONArray("transactions").get(1), after.getJSONArray("transactions").get(1))
        assertSame(before.getJSONArray("accounts"), after.getJSONArray("accounts"))
        val empty = JSONObject()
        assertSame(before, applyBudgetPatch(before, empty))
    }

    @Test fun sameBudgetIgnoresTheOrderOfAssignmentsOnly() {
        val before = cases.getJSONObject(0).getJSONObject("before")
        val rows = before.getJSONArray("assignments")
        val reversed = JSONArray((rows.length() - 1 downTo 0).map { rows.getJSONObject(it) })
        assertTrue(sameBudget(before, before.shallowCopy().put("assignments", reversed)))
        val transactions = before.getJSONArray("transactions")
        val moved = JSONArray((transactions.length() - 1 downTo 0).map { transactions.getJSONObject(it) })
        assertFalse(sameBudget(before, before.shallowCopy().put("transactions", moved)))
    }
}
