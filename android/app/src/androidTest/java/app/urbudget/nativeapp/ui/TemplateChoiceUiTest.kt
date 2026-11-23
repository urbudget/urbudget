package app.urbudget.nativeapp.ui

import android.app.Application
import androidx.compose.foundation.layout.Column
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import app.urbudget.nativeapp.*
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

/** The category templates a new budget can start with: offered from the engine, chosen, and made. */
@RunWith(AndroidJUnit4::class)
class TemplateChoiceUiTest {
    @get:Rule val compose = createComposeRule()
    private val instrumentation get() = InstrumentationRegistry.getInstrumentation()
    private val context get() = instrumentation.targetContext
    private val store = ViewModelStore()

    @Before fun setup() {
        requireEmulator()
        Words.source = ResourceWords(context.resources)
    }
    @After fun clean() { instrumentation.runOnMainSync { store.clear() } }

    @Test fun choosingATemplateShowsWhatItHoldsAndMakesItsCategories() {
        lateinit var vm: BudgetViewModel
        instrumentation.runOnMainSync { vm = BudgetViewModel(context.applicationContext as Application); store.put("templates", vm) }
        var chosen by mutableStateOf(DEFAULT_TEMPLATE)
        compose.setContent {
            MaterialTheme {
                Column {
                    TemplateChoice(chosen) { chosen = it }
                    Text("chosen:$chosen")
                }
            }
        }
        compose.waitUntil(30_000) { compose.onAllNodesWithText("Bills, Needs, Wants, Savings · 16 categories").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Everyday basics").assertExists()

        compose.onNodeWithTag("choice-open:Categories").performClick()
        compose.onNodeWithText("Student").performClick()
        compose.onNodeWithText("chosen:student").assertExists()
        compose.onNodeWithText("Tuition, living costs and a little fun on a tight budget.").assertExists()
        compose.onNodeWithText("School, Living, Fun, Savings · 11 categories").assertExists()

        val name = "Templates ${UUID.randomUUID()}"
        instrumentation.runOnMainSync { vm.createBudget(name, "USD", template = chosen) }
        compose.waitUntil(30_000) { vm.state.value.budget?.optString("name") == name || vm.state.value.error != null }
        assertNull(vm.state.value.error)
        val budget = vm.state.value.budget!!
        assertEquals(11, budget.getJSONArray("categories").length())
        assertEquals("""["School","Living","Fun","Savings"]""", budget.getJSONArray("groupOrder").toString())
    }
}
