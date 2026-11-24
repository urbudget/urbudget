package app.urbudget.nativeapp.ui

import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Budget's list on a budget that isn't set up yet: with no accounts it leads with "Add your first
 * account", as the Accounts tab's empty state has it, ahead of "Start your plan". The engine isn't
 * started here.
 */
@RunWith(AndroidJUnit4::class)
class BudgetEmptyPromptsUiTest {
    @get:Rule val compose = createComposeRule()
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private var accounts = 0
    private var categories = 0

    private fun show(noAccounts: Boolean, noCategories: Boolean) = compose.setContent {
        MaterialTheme { LazyColumn { budgetEmptyPrompts(noAccounts, noCategories, addAccount = { accounts++ }) { categories++ } } }
    }

    @Test fun aNewBudgetLeadsWithTheAccountThenThePlan() {
        show(noAccounts = true, noCategories = true)
        val account = compose.onNodeWithText(context.getString(R.string.accounts_empty_title))
        val plan = compose.onNodeWithText(context.getString(R.string.budget_empty_title))
        account.assertIsDisplayed()
        plan.assertIsDisplayed()
        assertTrue("The account comes first", account.fetchSemanticsNode().boundsInRoot.top < plan.fetchSemanticsNode().boundsInRoot.top)
        compose.onNodeWithText(context.getString(R.string.account_add)).performClick()
        assertEquals(1, accounts)
        compose.onNodeWithText(context.getString(R.string.category_add)).performClick()
        assertEquals(1, categories)
    }

    @Test fun withAnAccountOnlyThePlanIsAskedFor() {
        show(noAccounts = false, noCategories = true)
        compose.onNodeWithText(context.getString(R.string.accounts_empty_title)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.budget_empty_title)).assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.category_add)).performClick()
        assertEquals(1, categories)
    }

    @Test fun categoriesWithoutAnAccountStillAskForOne() {
        show(noAccounts = true, noCategories = false)
        compose.onNodeWithText(context.getString(R.string.accounts_empty_title)).assertIsDisplayed()
        compose.onNodeWithText(context.getString(R.string.budget_empty_title)).assertDoesNotExist()
        compose.onNodeWithText(context.getString(R.string.account_add)).performClick()
        assertEquals(1, accounts)
    }
}
