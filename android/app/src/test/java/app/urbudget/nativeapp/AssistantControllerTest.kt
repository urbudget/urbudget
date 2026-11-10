package app.urbudget.nativeapp

import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.launch
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
            controller.send("How much did I spend?")
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

    @Test fun aRepliesTextStaysLiveUntilTheConversationShowsItAndThenGivesWayInOneStep() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val events = MutableSharedFlow<JSONObject>(extraBufferCapacity = 64)
            val entries = mutableListOf(JSONObject().put("kind", "user").put("text", "How much did I spend?"))
            var reads = 0
            val controller = controller(store, events) { method, _ ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_send" -> "t1"
                    "assistant_thread" -> { reads++; JSONObject().put("id", "t1").put("title", "Spending").put("running", true).put("entries", JSONArray(entries.map { JSONObject(it.toString()) })) }
                    else -> JSONArray()
                }
            }
            fun event(type: String, text: String? = null) = JSONObject().put("event", "assistant://event")
                .put("payload", JSONObject().put("threadId", "t1").put("type", type).apply { text?.let { put("text", it) } })
            // Each state the panel is given: whether the conversation shows a reply, and whether a live one is shown.
            val shown = mutableListOf<Pair<Boolean, Boolean>>()
            backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
                controller.state.collect { state -> shown += state.entries.any { it.optString("kind") == "assistant" } to state.streaming.isNotEmpty() }
            }
            runCurrent()
            controller.send("How much did I spend?")
            settle { controller.state.value.running }
            events.emit(event("started")); events.emit(event("thinking", "Adding up October")); runCurrent()
            advanceTimeBy(65); runCurrent()
            assertEquals("" to "Adding up October", controller.state.value.let { it.streaming to it.thinking })
            events.emit(event("delta", "You spent 1,240.00.")); runCurrent()
            advanceTimeBy(65); runCurrent()
            assertEquals("You spent 1,240.00." to "", controller.state.value.let { it.streaming to it.thinking })

            // The round ends. The conversation read next doesn't have the reply yet: the live text stays.
            val from = shown.size
            val before = reads
            events.emit(event("message", "You spent 1,240.00.")); runCurrent()
            advanceTimeBy(100)
            settle { reads > before }
            repeat(20) { runCurrent(); Thread.sleep(5) }
            assertEquals("You spent 1,240.00.", controller.state.value.streaming)
            // Read again with the reply in it, the two change places at once.
            entries += JSONObject().put("kind", "assistant").put("text", "You spent 1,240.00.").put("index", 1)
            events.emit(event("step", "Looked up October")); runCurrent()
            advanceTimeBy(100)
            settle { controller.state.value.entries.size == 2 }
            assertEquals("", controller.state.value.streaming)
            assertEquals(emptyList<Pair<Boolean, Boolean>>(), shown.drop(from).filter { (reply, live) -> reply == live })
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun whatsBeingWrittenStaysWithItsConversationAndComesAlongToANewOne() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val controller = controller(store) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_thread" -> JSONObject().put("id", params.getString("id")).put("title", "A conversation").put("running", false)
                        .put("entries", JSONArray(listOf(JSONObject().put("kind", "user").put("text", "Hello"))))
                    else -> JSONArray()
                }
            }
            val composer = controller.composer
            fun open(id: String) { controller.openThread(id); settle { controller.state.value.threadId == id && controller.state.value.entries.size == 1 } }
            runCurrent()
            open("t1")
            composer.text = "About rent"; composer.select(listOf("a", "b")); controller.keepScroll(AssistantScroll(3, 20))
            // Another conversation has its own unsent message; this one's waits for it.
            open("t2")
            assertEquals("" to emptyList<String>(), composer.text to composer.selected)
            composer.text = "About food"
            open("t1")
            assertEquals("About rent" to listOf("a", "b"), composer.text to composer.selected)
            // A conversation opens at its end, wherever its list was left.
            assertNull(composer.scroll)

            // A new conversation takes what's typed and picked along, and asking for one again changes nothing.
            controller.startNew()
            assertNull(controller.state.value.thread)
            assertEquals("About rent" to listOf("a", "b"), composer.text to composer.selected)
            controller.startNew()
            assertEquals("About rent", composer.text)
            open("t2")
            assertEquals("About food", composer.text)
            // A new conversation that has its own unsent message shows that one, and the other stays where it was.
            controller.startNew()
            assertEquals("About rent", composer.text)
            open("t2")
            assertEquals("About food", composer.text)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun askingTheAssistantFromAPageIsForANewConversationWithWhatsHandedOverUnsent() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            var sent = 0
            val controller = controller(store) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_send" -> { sent++; "t9" }
                    "assistant_thread" -> JSONObject().put("id", params.getString("id")).put("title", "A conversation").put("running", false)
                        .put("entries", JSONArray(listOf(JSONObject().put("kind", "user").put("text", "Hello"))))
                    else -> JSONArray()
                }
            }
            val composer = controller.composer
            runCurrent()
            // With no conversation open it's for the one about to begin, after what's typed there.
            composer.text = "Also"; composer.without = setOf(AssistantPart.Place)
            controller.handOver("Help me with Groceries this month.", categoryId = "c1")
            assertNull(controller.state.value.thread)
            assertEquals("Also\nHelp me with Groceries this month." to "c1", composer.text to composer.category)
            assertEquals(emptySet<AssistantPart>(), composer.without)

            // From a conversation with messages it's for a new one. The one left keeps its own unsent message.
            controller.openThread("t1"); settle { controller.state.value.entries.size == 1 }
            composer.text = "About rent"
            controller.handOver(selected = (1..250).map { "t$it" })
            assertNull(controller.state.value.thread)
            assertEquals("Also\nHelp me with Groceries this month.", composer.text)
            // Only the first 200 can go, and the composer knows how many there were to say so.
            assertEquals(Triple(200, "t1", "t200"), Triple(composer.selected.size, composer.selected.first(), composer.selected.last()))
            assertEquals(250, composer.selectedOf)
            assertEquals(0, sent)
            // A selection made in the assistant's own list afterwards is whole.
            composer.select(listOf("t1"))
            assertNull(composer.selectedOf)
            controller.openThread("t1"); settle { controller.state.value.entries.size == 1 }
            assertEquals("About rent", composer.text)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun aSentMessageEmptiesTheComposerAndASuggestedQuestionLeavesWhatsTypedAlone() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val events = MutableSharedFlow<JSONObject>(extraBufferCapacity = 64)
            var running = false
            val screens = mutableListOf<JSONObject>()
            val controller = controller(store, events) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_send" -> { screens += params.getJSONObject("screen"); running = true; "t1" }
                    "assistant_thread" -> JSONObject().put("id", "t1").put("title", "Spending").put("running", running)
                        .put("entries", JSONArray(listOf(JSONObject().put("kind", "user").put("text", "How much?"))))
                    else -> JSONArray()
                }
            }
            val composer = controller.composer
            runCurrent()
            assertFalse(controller.replyRunning)
            composer.text = "How much?"; composer.select(listOf("a")); composer.without = setOf(AssistantPart.Import); composer.category = "c1"
            controller.send(composer.text, assistantSending(AssistantPlace("budget"), "2026-11", composer.selected, JSONObject().put("account", "a1"), composer.without, composer.category))
            settle { controller.state.value.running && "send" !in controller.state.value.busy }
            // It went with where the user is and the transaction picked, and without the file taken off it.
            val screen = screens.single()
            assertEquals(listOf("import", "c1", "a"), listOf(screen.getString("page"), screen.getString("categoryId"), screen.getJSONArray("selected").getString(0)))
            assertTrue(screen.isNull("import") && !screen.has("month"))
            // The next message starts afresh, with every part again.
            assertEquals(listOf("", emptyList<String>(), emptySet<AssistantPart>(), null), listOf(composer.text, composer.selected, composer.without, composer.category))
            // While the reply runs the core's events are read faster, until it ends.
            assertTrue(controller.replyRunning)
            running = false
            events.emit(JSONObject().put("event", "assistant://event").put("payload", JSONObject().put("threadId", "t1").put("type", "done"))); runCurrent()
            assertFalse(controller.replyRunning)
            advanceTimeBy(100)
            settle { !controller.state.value.running }

            composer.text = "Still typing"
            controller.send("Where is my money going this month?")
            settle { controller.state.value.running }
            assertEquals("Still typing", composer.text)
            assertEquals("", screens.last().getString("page"))
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun aFailureNobodyAskedForIsSaidOnceInThePanelAndOneTheUserAskedForInADialog() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            var failing = false
            var title = "Spending"
            val controller = controller(store) { method, _ ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_send" -> "t1"
                    "assistant_thread" -> if (failing) error("The conversation couldn't be read.") else JSONObject().put("id", "t1").put("title", title).put("running", true)
                        .put("entries", JSONArray(listOf(JSONObject().put("kind", "user").put("text", "Hi"))))
                    "assistant_rate" -> error("That reply is gone.")
                    else -> JSONArray()
                }
            }
            runCurrent()
            controller.send("Hi")
            settle { controller.state.value.running }
            // While a reply runs the conversation is read again every second or so.
            failing = true
            advanceTimeBy(1300); runCurrent()
            assertEquals("The conversation couldn't be read." to null, controller.state.value.let { it.quiet to it.error })
            // Dismissed, the same failure coming again isn't said again.
            controller.dismissQuiet()
            advanceTimeBy(3000); runCurrent()
            assertEquals(null to null, controller.state.value.let { it.quiet to it.error })
            // Once it has been read, a new failure is news.
            failing = false; title = "Read again"
            advanceTimeBy(1300)
            settle { controller.state.value.thread?.optString("title") == "Read again" }
            failing = true
            advanceTimeBy(1300); runCurrent()
            assertEquals("The conversation couldn't be read.", controller.state.value.quiet)

            // What the user just asked for fails in a dialog, and leaves the line alone.
            controller.rate(1, true); runCurrent()
            assertEquals("The conversation couldn't be read." to "That reply is gone.", controller.state.value.let { it.quiet to it.error })
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun aStatusThatCantBeReadIsItsOwnStateUntilItCan() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            var down = true
            val controller = controller(store) { method, _ ->
                when (method) {
                    "assistant_status" -> if (down) error("Services are still starting.") else JSONObject().put("provider", "openai").put("ready", false)
                    else -> JSONArray()
                }
            }
            runCurrent()
            // Not "not set up": nothing is known, and nothing is raised.
            assertEquals(listOf(null, true, null, null), controller.state.value.let { listOf(it.status, it.statusFailed, it.error, it.quiet) })
            down = false
            controller.refresh(); runCurrent()
            assertEquals(false to false, controller.state.value.let { it.statusFailed to it.status!!.getBoolean("ready") })
            // With a status known, failing to read it again is a line in the panel, and what's known stays.
            down = true
            controller.refresh(); runCurrent()
            assertEquals(listOf(false, "Services are still starting.", null), controller.state.value.let { listOf(it.statusFailed, it.quiet, it.error) })
            assertNotNull(controller.state.value.status)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun carryingALongConversationOnOpensTheNewOneWithWhatsTyped() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val calls = mutableListOf<String>()
            var title = "October"
            val controller = controller(store) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_rate" -> { calls += "rate ${params.getString("threadId")} ${params.getInt("index")} ${params.get("up")}"; null }
                    "assistant_continue" -> { calls += "continue ${params.getString("threadId")}"; "t2" }
                    "assistant_rename_thread" -> { calls += "rename ${params.getString("id")}"; title = params.getString("title"); null }
                    "assistant_threads" -> JSONArray(listOf(JSONObject().put("id", "t1").put("title", title).put("updated", "2026-11-10T08:00:00Z")))
                    "assistant_thread" -> if (params.getString("id") == "t1") JSONObject().put("id", "t1").put("title", title).put("running", false).put("long", true)
                        .put("entries", JSONArray(listOf(JSONObject().put("kind", "user").put("text", "Hello"), JSONObject().put("kind", "assistant").put("text", "Hi.").put("index", 1))))
                    else JSONObject().put("id", "t2").put("title", "October (continued)").put("running", false).put("long", false).put("continuedFrom", "October").put("entries", JSONArray())
                    else -> JSONArray()
                }
            }
            runCurrent()
            controller.openThread("t1"); settle { controller.state.value.entries.size == 2 }
            assertTrue(controller.state.value.thread!!.getBoolean("long"))
            // A thumb goes by the reply's place in the conversation; taking it off sends none.
            controller.rate(1, true); settle { "rate" !in controller.state.value.busy }
            controller.rate(1, null); settle { "rate" !in controller.state.value.busy }
            controller.renameThread("t1", "October so far"); settle { controller.state.value.thread?.optString("title") == "October so far" }
            assertEquals("October so far", controller.state.value.threads.single().getString("title"))

            controller.composer.text = "And November?"
            controller.continueInNew()
            settle { controller.state.value.threadId == "t2" && controller.state.value.thread!!.has("entries") }
            assertEquals("October" to emptyList<JSONObject>(), controller.state.value.let { it.thread!!.getString("continuedFrom") to it.entries })
            assertEquals("And November?", controller.composer.text)
            assertEquals(listOf("rate t1 1 true", "rate t1 1 null", "rename t1", "continue t1"), calls)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun choicesOnACardShowAtOnceAndGoToTheCoreInOneCallAMomentAfterTheLastAndBeforeApply() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val calls = mutableListOf<String>()
            val revisions = mutableListOf<JSONObject>()
            var reads = 0
            var saves = 0
            var changed = 0
            var card = card("p1", "proposed", "apply", "askChanges", "discard")
            val controller = controller(store, saved = { saves++ }) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_thread" -> { reads++; conversation(card).apply { if (card.getString("status") == "applied") put("undoable", "p1") } }
                    "assistant_revise" -> { calls += "revise"; revisions += params.getJSONObject("revision"); card = JSONObject(card.toString()).put("choices", params.getJSONObject("revision")); card }
                    "assistant_apply_commit" -> {
                        calls += "apply"
                        card = JSONObject(card.toString()).put("status", "applied").put("actions", JSONArray(listOf("undo")))
                        JSONObject().put("patch", JSONObject()).put("proposal", card).put("undoAvailable", true)
                    }
                    else -> JSONArray()
                }
            }
            fun shown() = controller.state.value.entries.last().getJSONObject("proposal")
            runCurrent()
            controller.openThread("t1"); settle { controller.state.value.entries.size == 2 }
            val before = reads
            // Three ticks in a row: each shows at once, and nothing goes to the core while they keep coming.
            val one = ProposalChoices.toggle(null, "t:a", skipped = false)
            val two = ProposalChoices.toggle(one, "t:b", skipped = false)
            val three = ProposalChoices.toggle(two, "t:a", skipped = true)
            controller.choose("p1", one); advanceTimeBy(400); runCurrent()
            controller.choose("p1", two); advanceTimeBy(400); runCurrent()
            assertSame(two, controller.cards.choices("p1"))
            controller.choose("p1", three); advanceTimeBy(ProposalChoiceDelay - 1); runCurrent()
            assertEquals(emptyList<String>(), calls)
            // A moment after the last of them they go in one call, and the card that comes back takes its place with no conversation read.
            advanceTimeBy(2); settle { calls.size == 1 }
            assertTrue(sameJson(JSONObject("""{"skip":["t:b"],"set":[]}"""), revisions.single()))
            assertNull(controller.cards.choices("p1"))
            assertTrue(sameJson(three, shown().getJSONObject("choices")))
            assertEquals(before, reads)
            // The card is never busy for a tick: only for what's done with it.
            assertFalse(controller.state.value.cardBusy("p1"))

            // Apply right after a tick sends the tick first, then applies; a second tap while it runs does nothing.
            val four = ProposalChoices.toggle(three, "t:b", skipped = true)
            controller.choose("p1", four)
            controller.apply("p1") { changed++ }
            controller.apply("p1") { changed++ }
            assertTrue(controller.state.value.cardBusy("p1") && !controller.state.value.cardBusy("p2"))
            settle { !controller.state.value.cardBusy("p1") && "apply" in calls }
            assertEquals(listOf("revise", "revise", "apply"), calls)
            assertTrue(sameJson(JSONObject("""{"skip":[],"set":[]}"""), revisions.last()))
            assertEquals(listOf(1, 1), listOf(saves, changed))
            assertEquals("applied", shown().getString("status"))
            // Which card the budget's Undo would take back is the conversation's to say.
            assertEquals("p1", controller.state.value.undoable)
            // The tick's own moment passes with nothing more to send.
            advanceTimeBy(ProposalChoiceDelay + 100); runCurrent()
            assertEquals(3, calls.size)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun aCardIsPutInItsPlaceByWhatsDoneWithItAndFollowsTheBudget() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val events = MutableSharedFlow<JSONObject>(extraBufferCapacity = 64)
            val calls = mutableListOf<String>()
            var reads = 0
            var card = card("p1", "proposed", "applyToImport", "askChanges", "discard").put("kind", "transactions")
            var refuse: String? = null
            val controller = controller(store, events) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_thread" -> { reads++; conversation(card) }
                    "assistant_mark" -> {
                        refuse?.let { error(it) }
                        calls += "mark ${params.getString("status")}${if (params.has("note")) " " + params.getString("note") else ""}"
                        card = JSONObject(card.toString()).put("status", params.getString("status")); card
                    }
                    "database_status" -> JSONObject().put("token", "token-7")
                    "assistant_undo_commit" -> { calls += "undo ${params.getString("expectedToken")}"; card = JSONObject(card.toString()).put("status", "undone"); JSONObject().put("proposal", card) }
                    else -> JSONArray()
                }
            }
            fun shown() = controller.state.value.entries.last().getJSONObject("proposal")
            runCurrent()
            controller.openThread("t1"); settle { controller.state.value.entries.size == 2 }
            // The budget changed while a change waits: the conversation is read again, so the card is the one made on the budget as it is.
            val opened = reads
            controller.budgetChanged(); advanceTimeBy(100); settle { reads == opened + 1 }

            // A card the app carries out itself takes the card's turn: a second tap does nothing, and it's marked with what there is to say.
            val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
            var runs = 0
            controller.carryOut("p1") { runs++; gate.await(); CardDone("applied", "Fixes applied: 1 of 2. Not applied: Line 4: date") }
            runCurrent()
            controller.carryOut("p1") { runs++; null }
            runCurrent()
            assertEquals(1 to true, runs to controller.state.value.cardBusy("p1"))
            gate.complete(Unit); settle { !controller.state.value.cardBusy("p1") }
            assertEquals(listOf("mark applied Fixes applied: 1 of 2. Not applied: Line 4: date"), calls)
            assertEquals("applied", shown().getString("status"))
            // One that fails says why, and marks nothing; one that leaves the card as it is marks nothing either.
            controller.carryOut("p1") { error("That account isn’t open in this budget.") }; settle { !controller.state.value.cardBusy("p1") }
            assertEquals("That account isn’t open in this budget.", controller.state.value.error)
            controller.dismissError()
            controller.carryOut("p1") { null }; settle { !controller.state.value.cardBusy("p1") }
            assertEquals(1, calls.size)

            // Undo asks the core with the budget's token as it is now; the card that comes back is undone.
            controller.undoProposal("p1") {}; settle { !controller.state.value.cardBusy("p1") }
            assertEquals("undo token-7", calls.last())
            assertEquals("undone", shown().getString("status"))
            // Set aside and put back; a step the core refuses is said in its words.
            controller.mark("p1", "discarded"); settle { !controller.state.value.cardBusy("p1") }
            controller.mark("p1", "proposed"); settle { !controller.state.value.cardBusy("p1") }
            assertEquals(listOf("mark discarded", "mark proposed"), calls.takeLast(2))
            refuse = "This proposal was already handled."
            controller.mark("p1", "discarded"); settle { !controller.state.value.cardBusy("p1") }
            assertEquals("This proposal was already handled.", controller.state.value.error)
            refuse = null

            // A newer card made in its place: the event's view of the earlier one replaces it where it stands.
            val size = controller.state.value.entries.size
            events.emit(JSONObject().put("event", "assistant://event").put("payload", JSONObject().put("threadId", "t1").put("type", "replaced")
                .put("proposal", JSONObject(card.toString()).put("status", "replaced").put("replacedBy", "p2").put("actions", JSONArray(listOf("restore"))))))
            runCurrent()
            assertEquals(listOf("replaced", "p2", size), listOf(shown().getString("status"), shown().getString("replacedBy"), controller.state.value.entries.size))

            // A thing the assistant made doesn't change with the budget: nothing is read for it.
            card = card("p1", "proposed", "preview", "remove").put("kind", "report").put("made", true)
            controller.reloadThread(); advanceTimeBy(100); settle { shown().getString("kind") == "report" }
            val settled = reads
            controller.budgetChanged(); advanceTimeBy(200); runCurrent()
            assertEquals(settled, reads)
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    @Test fun theBudgetsNotesAreSavedAsAWholeListAndTheBudgetComesBackWithThem() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = ViewModelStore()
        try {
            val saved = mutableListOf<String>()
            var said = 0
            var full = false
            val controller = controller(store, saved = { said++ }) { method, params ->
                when (method) {
                    "assistant_status" -> JSONObject().put("provider", "chatgpt").put("ready", true)
                    "assistant_notes_commit" -> { if (full) error("A budget keeps up to 30 notes."); saved += params.getJSONArray("notes").toString(); JSONObject().put("token", "t2") }
                    else -> JSONArray()
                }
            }
            runCurrent()
            var refreshed = 0
            var done = 0
            controller.saveNotes(listOf("Rent is due on the 1st", "Careem is transport"), { refreshed++ }) { done++ }; runCurrent()
            assertEquals(listOf("""["Rent is due on the 1st","Careem is transport"]"""), saved)
            assertEquals(listOf(1, 1, 1), listOf(said, refreshed, done))
            // The core's limits are the core's to say; nothing is taken as saved.
            full = true
            controller.saveNotes(listOf("One too many"), { refreshed++ }) { done++ }; runCurrent()
            assertEquals("A budget keeps up to 30 notes.", controller.state.value.error)
            assertEquals(listOf(1, 1, 1), listOf(said, refreshed, done))
        } finally { store.clear(); Dispatchers.resetMain() }
    }

    /** A card as the conversation's view gives it, offering [actions]. */
    private fun card(id: String, status: String, vararg actions: String) = JSONObject().put("id", id).put("kind", "budget").put("status", status).put("title", "Cover Dining out")
        .put("made", false).put("actions", JSONArray(actions.toList())).put("card", JSONObject().put("kicker", "Proposal").put("rows", JSONArray()))

    /** A conversation with one card, as the core shows it. */
    private fun conversation(card: JSONObject) = JSONObject().put("id", "t1").put("title", "Dining out").put("running", false)
        .put("entries", JSONArray(listOf(JSONObject().put("kind", "user").put("text", "Cover Dining out"), JSONObject().put("kind", "proposal").put("proposal", JSONObject(card.toString())))))

    /** Runs what's ready until [done]: a conversation that was read is compared on a worker thread, which takes a moment of real time. */
    private fun TestScope.settle(done: () -> Boolean) {
        repeat(600) { runCurrent(); if (done()) return; Thread.sleep(5) }
        fail("The controller didn't get there.")
    }

    private fun controller(store: ViewModelStore, events: Flow<JSONObject> = emptyFlow(), saved: () -> Unit = {}, service: suspend (String, JSONObject) -> Any?): AssistantController =
        ViewModelProvider(store, object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : androidx.lifecycle.ViewModel> create(modelClass: Class<T>): T = AssistantController(service, events, saved) as T
        })[AssistantController::class.java]
}
