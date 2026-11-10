package app.urbudget.nativeapp

import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.test.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class AssistantControllerTest {
    @Test fun aStoppedReplyKeepsItsTextOnceAndTokensThatArriveLateDontShowItAgain() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val events = MutableSharedFlow<JSONObject>(extraBufferCapacity = 64)
            var running = false
            var title = "Spending"
            val entries = mutableListOf(JSONObject().put("kind", "user").put("text", "How much did I spend?"))
            val controller = controller(store, events) { method, _ ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_send" -> { running = true; "t1" }
                    "assistant_thread" -> JSONObject().put("id", "t1").put("title", title).put("running", running)
                        .put("entries", JSONArray(entries.map { JSONObject(it.toString()) }))
                    // Stop keeps what was written so far, then the mark.
                    "assistant_stop" -> {
                        running = false
                        entries += JSONObject().put("kind", "assistant").put("text", "You've spent 1,240.00 of")
                        entries += JSONObject().put("kind", "stopped")
                        null
                    }
                    else -> JSONArray()
                }
            }
            fun event(type: String, text: String? = null) = JSONObject().put("event", "assistant://event")
                .put("payload", JSONObject().put("threadId", "t1").put("type", type).apply { text?.let { put("text", it) } })
            fun kinds() = controller.state.value.entries.map { it.optString("kind") }
            runCurrent()
            controller.send("How much did I spend?", "2026-11") {}
            settle { controller.state.value.running && kinds() == listOf("user") }
            events.emit(event("started")); events.emit(event("delta", "You've spent 1,240.00 of")); runCurrent()
            advanceTimeBy(65); runCurrent()
            assertEquals("You've spent 1,240.00 of", controller.state.value.streaming)

            controller.stop()
            settle { !controller.state.value.running }
            assertEquals(listOf("user", "assistant", "stopped"), kinds())
            assertEquals("", controller.state.value.streaming)

            // Events are polled: the next poll brings what was written just before Stop, then the end.
            title = "Spending in November"
            events.emit(event("delta", " your 2,000.00")); events.emit(event("thinking", "Adding up"))
            events.emit(event("done")); runCurrent()
            // Past when the live text is next published, before the conversation is read again.
            advanceTimeBy(70); runCurrent()
            assertEquals("" to "", controller.state.value.let { it.streaming to it.thinking })
            advanceTimeBy(200)
            settle { controller.state.value.thread?.optString("title") == "Spending in November" }
            assertEquals(listOf("user", "assistant", "stopped"), kinds())
            assertEquals("" to "", controller.state.value.let { it.streaming to it.thinking })
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun readingTheNoticeIsKeptForTheServiceBeforeWhatWaitedOnItGoes() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            var told = false
            val changes = mutableListOf<String>()
            val controller = controller(store) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "anthropic").put("ready", true).put("noticeDue", !told)
                    "assistant_settings" -> { changes += params.toString(); told = params.getJSONObject("change").optBoolean("told"); JSONObject() }
                    else -> JSONArray()
                }
            }
            runCurrent()
            assertTrue(controller.state.value.status!!.getBoolean("noticeDue"))
            var dueWhenSent: Boolean? = null
            controller.told { dueWhenSent = controller.state.value.status!!.getBoolean("noticeDue") }; runCurrent()
            assertEquals(listOf("""{"change":{"told":true}}"""), changes)
            assertEquals(false, dueWhenSent)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun signingInAgainAfterAFailedReplyAsksForItOnlyOnceSignedIn() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            var ready = false
            var cancelled = true
            val controller = controller(store) { method, _ ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", ready).put("signingIn", false)
                    "assistant_sign_in" -> { if (cancelled) error("Sign-in was cancelled."); ready = true; JSONObject() }
                    else -> JSONArray()
                }
            }
            runCurrent()
            var asked = 0
            controller.signIn { asked++ }; runCurrent()
            assertEquals(0, asked)
            assertEquals("Sign-in was cancelled.", controller.state.value.error)
            cancelled = false
            controller.signIn { asked++ }; runCurrent()
            assertEquals(1, asked)
            assertTrue(controller.state.value.status!!.getBoolean("ready"))
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    /** Runs what's ready until [done]: a conversation that was read is compared on a worker thread, which takes a moment of real time. */
    private fun TestScope.settle(done: () -> Boolean) {
        repeat(600) { runCurrent(); if (done()) return; Thread.sleep(5) }
        fail("The controller didn't get there.")
    }

    private fun controller(store: ViewModelStore, events: Flow<JSONObject> = emptyFlow(), service: suspend (String, JSONObject) -> Any?): AssistantController =
        ViewModelProvider(store, object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : androidx.lifecycle.ViewModel> create(modelClass: Class<T>): T = AssistantController(service, events) as T
        })[AssistantController::class.java]
}
