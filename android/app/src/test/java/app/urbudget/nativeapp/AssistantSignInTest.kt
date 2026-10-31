package app.urbudget.nativeapp

import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class AssistantSignInTest {
    @Test fun signInUsesTheInstalledPackageAndPublishesCompletion() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            var status = JSONObject().put("provider", "chatgpt").put("ready", false)
            val finish = CompletableDeferred<Unit>()
            var returnedPackage: String? = null
            val controller = controller(store) { method, params ->
                when (method) {
                    "assistant_status" -> status
                    "assistant_sign_in" -> {
                        returnedPackage = params.getString("returnPackage")
                        finish.await()
                        status = JSONObject().put("provider", "chatgpt").put("ready", true).put("signingIn", false)
                        status
                    }
                    else -> JSONArray()
                }
            }
            runCurrent()
            controller.signIn(); runCurrent()
            assertEquals(BuildConfig.APPLICATION_ID, returnedPackage)
            assertTrue("signIn" in controller.state.value.busy)
            finish.complete(Unit); runCurrent()
            assertTrue(controller.state.value.status!!.getBoolean("ready"))
            assertFalse("signIn" in controller.state.value.busy)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun recreatedControllerKeepsCheckingAnAttemptStartedBeforeItExisted() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            var status = JSONObject().put("provider", "chatgpt").put("ready", false).put("signingIn", true)
            val controller = controller(store) { method, _ -> if (method == "assistant_status") status else JSONArray() }
            runCurrent()
            assertTrue(controller.state.value.status!!.getBoolean("signingIn"))
            status = JSONObject().put("provider", "chatgpt").put("ready", true).put("signingIn", false)
            advanceTimeBy(1201); runCurrent()
            assertTrue(controller.state.value.status!!.getBoolean("ready"))
            assertFalse(controller.state.value.status!!.getBoolean("signingIn"))
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun returningFromTheBrowserRefreshesStatusWithoutHidingAnExchangeFailure() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val controller = controller(store) { method, _ ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", false).put("signingIn", false)
                    "assistant_sign_in" -> error("Synthetic token exchange failed")
                    else -> JSONArray()
                }
            }
            runCurrent()
            controller.signIn(); runCurrent()
            controller.refresh(); runCurrent() // AssistantScreen's ON_RESUME callback.
            controller.refreshSignInStatus(); runCurrent()
            assertEquals("Synthetic token exchange failed", controller.state.value.error)
            assertFalse("signIn" in controller.state.value.busy)
            assertFalse(controller.state.value.status!!.getBoolean("signingIn"))
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    private fun controller(store: ViewModelStore, service: suspend (String, JSONObject) -> Any?): AssistantController =
        ViewModelProvider(store, object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : androidx.lifecycle.ViewModel> create(modelClass: Class<T>): T = AssistantController(service) as T
        })[AssistantController::class.java]
}
