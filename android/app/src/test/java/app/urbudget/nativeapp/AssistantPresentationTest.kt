package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

class AssistantPresentationTest {
    @Before fun english() = EnglishWords.install()

    @Test fun tokenBurstsPreserveExactTextAcrossBatchedPublications() {
        val buffer = AssistantStreamBuffer()
        val expected = StringBuilder()
        repeat(20_000) { index ->
            val token = "$index · "
            expected.append(token)
            buffer.append(token)
            if (index % 64 == 0) assertEquals(expected.toString(), buffer.drain()!!.first)
        }
        assertEquals(expected.toString(), buffer.drain()!!.first)
        assertNull(buffer.drain())
    }

    @Test fun completingMessageAndSwitchingConversationDoNotLeakBufferedText() {
        val buffer = AssistantStreamBuffer()
        buffer.append("private reply")
        buffer.append("reasoning", thinking = true)
        buffer.clearReply()
        assertEquals("" to "reasoning", buffer.drain())
        buffer.append("late chunk")
        buffer.clear()
        assertNull(buffer.drain())
        buffer.append("new conversation")
        assertEquals("new conversation" to "", buffer.drain())
    }

    @Test fun largeUnchangedPollKeepsTheEntirePublishedPresentation() {
        val thread = conversation("one", 5_000)
        val first = assistantThreadPresentation(thread, null)
        val refreshed = assistantThreadPresentation(JSONObject(thread.toString()), first)
        assertSame(first, refreshed)
        assertSame(first.entries, refreshed.entries)
    }

    @Test fun appendingReplyOrUpdatingProposalOnlyInvalidatesChangedEntries() {
        val original = conversation("one", 500)
        val first = assistantThreadPresentation(original, null)
        val next = JSONObject(original.toString())
        next.getJSONArray("entries").getJSONObject(300).put("text", "Rechecked proposal")
        next.getJSONArray("entries").put(JSONObject().put("kind", "assistant").put("text", "Final reply"))
        val refreshed = assistantThreadPresentation(next, first)
        assertEquals(501, refreshed.entries.size)
        repeat(500) { if (it != 300) assertSame(first.entries[it], refreshed.entries[it]) }
        assertNotSame(first.entries[300], refreshed.entries[300])
        assertEquals("Message 300", first.entries[300].getString("text"))
        assertEquals("Rechecked proposal", refreshed.entries[300].getString("text"))
    }

    @Test fun anotherConversationNeverReusesOldEntryReferences() {
        val first = assistantThreadPresentation(conversation("one", 20), null)
        val second = assistantThreadPresentation(conversation("two", 20), first)
        repeat(20) { assertNotSame(first.entries[it], second.entries[it]) }
    }

    @Test fun anErrorsButtonIsTheOneTheCoreNamesAndOnlyTheLatestErrorHasOne() {
        fun error(action: Any?) = JSONObject().put("kind", "error").put("text", "It failed.").apply { action?.let { put("action", it) } }
        assertEquals(AssistantErrorAction.Retry, AssistantErrorAction.of(error("retry")))
        assertEquals(AssistantErrorAction.SignIn, AssistantErrorAction.of(error("signIn")))
        assertEquals(AssistantErrorAction.Settings, AssistantErrorAction.of(error("settings")))
        assertEquals(AssistantErrorAction.Usage, AssistantErrorAction.of(error("usage")))
        assertEquals(AssistantErrorAction.NewConversation, AssistantErrorAction.of(error("newConversation")))
        // An earlier error carries no action; "none" is one asking again wouldn't help. Neither has a button, nor has an action this app doesn't know.
        assertNull(AssistantErrorAction.of(error(null)))
        assertNull(AssistantErrorAction.of(error("none")))
        assertNull(AssistantErrorAction.of(error(JSONObject.NULL)))
        assertNull(AssistantErrorAction.of(error("somethingNewer")))
        assertEquals(listOf("Continue with ChatGPT", "Assistant settings", "Plan usage", "Continue in a new conversation", "Try again"), AssistantErrorAction.values().map { Words[it.label] })
    }

    @Test fun theNoticeOfWhatsSentNamesTheCompanyBehindTheService() {
        assertEquals(listOf("OpenAI", "OpenAI", "Anthropic"), listOf("chatgpt", "openai", "anthropic").map(::assistantProviderCompany))
    }

    @Test fun leavingARowOutAndPuttingItBackChangesOnlyItsKey() {
        assertJson("""{"skip":[],"set":[]}""", ProposalChoices.original())
        val one = ProposalChoices.toggle(null, "t:b", skipped = false)
        assertJson("""{"skip":["t:b"],"set":[]}""", one)
        assertEquals(listOf("t:b", "t:a"), keys(ProposalChoices.toggle(one, "t:a", skipped = false), "skip"))
        // Put back: out of skip, and an amount of nothing given to it goes too; other choices stay.
        val choices = JSONObject().put("skip", JSONArray(listOf("s:0", "s:1"))).put("set", JSONArray(listOf(
            JSONObject().put("key", "s:0").put("amount", 0), JSONObject().put("key", "s:1").put("category", JSONObject.NULL),
            JSONObject().put("key", "s:2").put("amount", 0))))
        val back = ProposalChoices.toggle(choices, "s:0", skipped = true)
        assertEquals(listOf("s:1"), keys(back, "skip"))
        assertEquals(listOf("s:1", "s:2"), back.getJSONArray("set").let { set -> (0 until set.length()).map { set.getJSONObject(it).getString("key") } })
        // A category given stays when its row is put back, Ready to Assign (null) included.
        val again = ProposalChoices.toggle(back, "s:1", skipped = true)
        assertEquals(emptyList<String>(), keys(again, "skip"))
        assertTrue(again.getJSONArray("set").getJSONObject(0).let { it.has("category") && it.isNull("category") })
    }

    @Test fun anEditReplacesItsRowsEntryOverWhatItWasGiven() {
        val one = ProposalChoices.change(JSONObject().put("skip", JSONArray(listOf("r:r1"))).put("set", JSONArray()), "t:b", JSONObject().put("category", "groceries"))
        assertJson("""{"skip":["r:r1"],"set":[{"category":"groceries","key":"t:b"}]}""", one)
        val two = ProposalChoices.change(one, "t:a", JSONObject().put("category", JSONObject.NULL))
        assertJson("""{"skip":["r:r1"],"set":[{"category":"groceries","key":"t:b"},{"category":null,"key":"t:a"}]}""", two)
        // Another amount for t:b keeps the category it was given.
        val three = ProposalChoices.change(two, "t:b", JSONObject().put("amount", -30000))
        assertJson("""{"skip":["r:r1"],"set":[{"category":null,"key":"t:a"},{"category":"groceries","amount":-30000,"key":"t:b"}]}""", three)
    }

    @Test fun savingARowsEditorGivesWhatChangedKeepingTheRowsSign() {
        Money.configure(JSONObject().put("currency", "SAR"))
        val home = JSONObject().put("label", "Home").put("amount", -45900).put("key", "s:0")
            .put("edit", JSONObject().put("category", true).put("categoryId", "rent").put("amount", true))
        assertJson("""{"amount":-30000}""", ProposalChoices.edit(home, "rent", "300"))
        assertJson("""{"amount":500}""", ProposalChoices.edit(home, "rent", "+5"))
        assertNull(ProposalChoices.edit(home, "rent", "-459.00"))
        assertNull(ProposalChoices.edit(home, "rent", " 459 "))
        assertJson("""{"category":null}""", ProposalChoices.edit(home, null, "-459.00"))
        assertJson("""{"category":"groceries","amount":-1250}""", ProposalChoices.edit(home, "groceries", "12.5"))
        assertThrows(IllegalArgumentException::class.java) { ProposalChoices.edit(home, "rent", "lots") }
        // A category only: the amount isn't read. Ready to Assign is a row with no categoryId.
        val careem = JSONObject().put("label", "Careem").put("amount", -3800).put("key", "t:a")
            .put("edit", JSONObject().put("category", true).put("amount", false))
        assertNull(ProposalChoices.edit(careem, null, "lots"))
        assertJson("""{"category":"transport"}""", ProposalChoices.edit(careem, "transport", ""))
        // A positive row stays positive.
        val salary = JSONObject().put("label", "Salary").put("amount", 2000).put("key", "t:s").put("edit", JSONObject().put("category", false).put("amount", true))
        assertJson("""{"amount":1500}""", ProposalChoices.edit(salary, null, "15"))
        assertNull(ProposalChoices.edit(JSONObject().put("label", "New rule"), null, ""))
    }

    @Test fun applySaysHowMuchOfTheCardItMakesWhenSomeIsLeftOut() {
        val rows = listOf(JSONObject().put("key", "t:a"), JSONObject().put("key", "t:b").put("skipped", true), JSONObject().put("key", "r:r1"), JSONObject().put("label", "unkeyed"))
        assertEquals(ProposalChoices.Partial(2, 3), ProposalChoices.partial(rows, choosing = true))
        assertNull(ProposalChoices.partial(rows, choosing = false))
        assertNull(ProposalChoices.partial(rows.map { JSONObject(it.toString()).put("skipped", false) }, choosing = true))
        assertEquals(ProposalChoices.Partial(0, 3), ProposalChoices.partial(rows.map { JSONObject(it.toString()).put("skipped", true) }, choosing = true))
    }

    @Test fun textThatWasJustWrittenStaysLiveUntilItsEntryIsShown() {
        val buffer = AssistantStreamBuffer()
        buffer.append("Weighing the months", thinking = true)
        assertEquals("" to "Weighing the months", buffer.drain())
        // Words arriving end the thinking that led to them.
        buffer.append("You spent ")
        buffer.append("1,240.00.")
        assertEquals("You spent 1,240.00." to "", buffer.drain())
        // The round ends: its text is still what's live, until the conversation shows it.
        buffer.settle()
        assertEquals("You spent 1,240.00." to "", buffer.drain())
        buffer.append("Next, ")
        assertEquals("You spent 1,240.00.\n\nNext, " to "", buffer.drain())
        buffer.shown()
        assertEquals("Next, " to "", buffer.drain())
        buffer.shown()
        assertNull(buffer.drain())
    }

    @Test fun onlyTheLatestLineOfThinkingIsKeptToShow() {
        assertEquals("", assistantThinkingLine(""))
        assertEquals("Comparing October with September", assistantThinkingLine("**Reading the budget**\n\nLooking at groceries.\nComparing October with September\n\n"))
        assertEquals("Reading the budget", assistantThinkingLine("## **Reading the budget**"))
        val buffer = AssistantStreamBuffer()
        repeat(2_000) { buffer.append("Line $it of a long think\n", thinking = true) }
        assertEquals("Line 1999 of a long think", buffer.drain()!!.second)
    }

    @Test fun aReplyIsShownOnceTheConversationsLatestReplyIsIt() {
        val entries = listOf(JSONObject().put("kind", "user").put("text", "Hi"), JSONObject().put("kind", "assistant").put("text", "Hello.\n"), JSONObject().put("kind", "step").put("text", "Looked up"))
        assertTrue(assistantShows(entries, "Hello."))
        assertFalse(assistantShows(entries, "Hello again."))
        assertFalse(assistantShows(emptyList(), "Hello."))
    }

    @Test fun eventsAreReadFasterWhileAReplyRuns() {
        assertEquals(100L, assistantPollDelay(replying = true))
        assertEquals(300L, assistantPollDelay(replying = false))
    }

    @Test fun anEntryOfAKindThisAppDoesntKnowHasNoKindSoItIsNeverDrawnAsAReply() {
        fun kind(name: String) = AssistantEntryKind.of(JSONObject().put("kind", name))
        assertEquals(listOf(AssistantEntryKind.User, AssistantEntryKind.Assistant, AssistantEntryKind.Step, AssistantEntryKind.Proposal, AssistantEntryKind.Stopped,
            AssistantEntryKind.Error, AssistantEntryKind.Cut), listOf("user", "assistant", "step", "proposal", "stopped", "error", "cut").map(::kind))
        assertNull(kind("somethingNewer"))
        assertNull(AssistantEntryKind.of(JSONObject()))
    }

    @Test fun whatGoesWithAMessageIsItsPartsLessTheOnesTakenOff() {
        val budget = AssistantPlace("budget")
        val register = AssistantPlace("transactions", accountId = "a1", categoryId = "c1")
        val file = JSONObject().put("account", "a1").put("content", "Date,Amount").put("session", JSONObject())
        // The month goes only from a page that shows one.
        assertEquals(AssistantSending(budget, "2026-11"), assistantSending(budget, "2026-11", emptyList(), null))
        assertEquals(AssistantSending(register, null, listOf("t1", "t2")), assistantSending(register, "2026-11", listOf("t1", "t2"), null))
        // Over an import the page is the import's, and its file goes too.
        val importing = assistantSending(budget, "2026-11", emptyList(), file)
        assertEquals("import" to null, importing.place?.page to importing.month)
        assertSame(file, importing.import)
        // A category handed over with "Ask the assistant" is the one in view.
        assertEquals(AssistantPlace("budget", categoryId = "c9"), assistantSending(budget, "2026-11", emptyList(), null, category = "c9").place)
        // Each part comes off on its own.
        assertEquals(AssistantSending(null, null, listOf("t1"), file), assistantSending(register, "2026-11", listOf("t1"), file, setOf(AssistantPart.Place), category = "c9"))
        assertEquals(AssistantSending(register.copy(page = "import"), null, listOf("t1")), assistantSending(register, "2026-11", listOf("t1"), file, setOf(AssistantPart.Import)))
        assertEquals(AssistantSending(budget, "2026-11"), assistantSending(budget, "2026-11", listOf("t1"), null, setOf(AssistantPart.Transactions)))
    }

    @Test fun theScreenSentLeavesOutMonthAccountAndCategoryOnceThePlaceIsTakenOff() {
        val here = assistantScreen(AssistantSending(AssistantPlace("budget", categoryId = "c9"), "2026-11", listOf("t1")), "2026-11-10", bankTexts = true)
        assertJson("""{"platform":"android","page":"budget","month":"2026-11","categoryId":"c9","today":"2026-11-10","selected":["t1"],"import":null,"bankTexts":true}""", here)
        val register = assistantScreen(AssistantSending(AssistantPlace("transactions", accountId = "a1")), "2026-11-10", bankTexts = false)
        assertJson("""{"platform":"android","page":"transactions","accountId":"a1","today":"2026-11-10","selected":[],"import":null,"bankTexts":false}""", register)
        // No page says the user took where they are off the message.
        val file = JSONObject().put("account", "a1")
        val nowhere = assistantScreen(AssistantSending(null, null, listOf("t1"), file), "2026-11-10", bankTexts = false)
        assertJson("""{"platform":"android","page":"","today":"2026-11-10","selected":["t1"],"import":{"account":"a1"},"bankTexts":false}""", nowhere)
        assertFalse(nowhere.has("month") || nowhere.has("accountId") || nowhere.has("categoryId"))
    }

    @Test fun aPagesKeyNamesThePageAsTheAppDoes() {
        assertEquals(listOf("Budget", "Transactions", "Accounts", "Reports", "Scheduled", "Payees & rules", "Settings & data", "Bank messages", "Bank connections", "Import bank file"),
            listOf("budget", "transactions", "accounts", "reports", "scheduled", "payees", "settings", "bankMessages", "bankConnections", "import").map { Words[assistantPageName(it)!!] })
        assertNull(assistantPageName("somewhereNew"))
        assertEquals(listOf(true, true, false, false), listOf("budget", "reports", "transactions", "import").map(::assistantPageShowsMonth))
    }

    @Test fun aSelectionOver200IsCutToItsFirst200AndSaysHowManyThereWere() {
        val few = (1..200).map { "t$it" }
        assertEquals(few to null, assistantSelection(few))
        val many = (1..201).map { "t$it" }
        assertEquals(few to 201, assistantSelection(many))
        assertEquals(emptyList<String>() to null, assistantSelection(emptyList()))
        assertEquals("Only the first 200 of 1,250 selected transactions are attached.", Words.count(R.plurals.assistant_only_first, 1250))
    }

    @Test fun somethingPutInTheComposerGoesAfterWhatsTypedNeverOverIt() {
        assertEquals("Fix these lines.", assistantDraftWith("", "Fix these lines."))
        assertEquals("Fix these lines.", assistantDraftWith("  \n", "Fix these lines."))
        assertEquals("About rent\nRevise the proposal ‘Cover rent’: ", assistantDraftWith("About rent  ", "Revise the proposal ‘Cover rent’: "))
        // Put in a second time, it's there already.
        assertEquals("About rent\nFix these lines.", assistantDraftWith("About rent\nFix these lines.", "Fix these lines."))
    }

    @Test fun newTextIsFollowedOnlyFromTheEnd() {
        fun frame(index: Int, offset: Int, more: Boolean, scrolling: Boolean = false) = AssistantListFrame(index, offset, more, scrolling)
        // At the end it follows, however it got there.
        assertTrue(assistantFollows(false, frame(3, 0, more = true, scrolling = true), frame(9, 40, more = false, scrolling = true)))
        assertTrue(assistantFollows(false, null, frame(0, 0, more = false)))
        // Text arriving under a still list leaves it as it was: followed if it was at the end before.
        assertTrue(assistantFollows(true, frame(9, 40, more = false), frame(9, 40, more = true)))
        assertFalse(assistantFollows(false, frame(4, 10, more = true), frame(4, 10, more = true)))
        // A finger resting on the list at its end doesn't stop it either.
        assertTrue(assistantFollows(true, frame(9, 40, more = false, scrolling = true), frame(9, 40, more = true, scrolling = true)))
        // Scrolling away from the end stops it.
        assertFalse(assistantFollows(true, frame(9, 40, more = false, scrolling = true), frame(9, 12, more = true, scrolling = true)))
        assertFalse(assistantFollows(true, frame(9, 40, more = true, scrolling = true), frame(8, 300, more = true, scrolling = true)))
        // The list moving with no scroll (the conversation was read again) doesn't.
        assertTrue(assistantFollows(true, frame(9, 40, more = true), frame(8, 40, more = true)))
        assertTrue(assistantFollows(true, null, frame(0, 0, more = true)))
    }

    @Test fun conversationsAreSearchedByTitleWhateverTheCase() {
        val threads = listOf("October groceries", "Rent in 2026", "خطة أكتوبر").map { JSONObject().put("id", it).put("title", it) }
        fun found(query: String) = assistantThreadsMatching(threads, query).map { it.getString("title") }
        assertEquals(listOf("October groceries", "Rent in 2026", "خطة أكتوبر"), found("  "))
        assertEquals(listOf("October groceries"), found("OCTOBER"))
        assertEquals(listOf("Rent in 2026"), found(" rent "))
        assertEquals(listOf("خطة أكتوبر"), found("أكتوبر"))
        assertEquals(emptyList<String>(), found("holiday"))
    }

    @Test fun aConversationsTokensAreSaidOnlyWhenItUsedSome() {
        fun thread(input: Long, output: Long, cached: Long) = JSONObject().put("usage", JSONObject().put("input", input).put("output", output).put("cached", cached))
        assertEquals(AssistantUsage(sent = 2400, cached = 800, received = 60), AssistantUsage.of(thread(2400, 60, 800)))
        assertNull(AssistantUsage.of(thread(0, 0, 0)))
        assertNull(AssistantUsage.of(JSONObject()))
        assertNull(AssistantUsage.of(null))
        assertEquals("Tokens: 12,400 sent (8,000 from cache), 960 received", Words[R.string.assistant_tokens, Money.count(12400), Money.count(8000), Money.count(960)])
    }

    @Test fun aReportsDetailsNameTheAppTheServiceAndTheTools() {
        assertEquals("App: UrBudget Android 1.0.0 · Service: ChatGPT · Model: default · Tools: get_month, list_transactions · Language: en",
            assistantReportDetails("1.0.0", "chatgpt", null, listOf("get_month", "list_transactions"), "en"))
        assertEquals("App: UrBudget Android 1.0.1 · Service: Anthropic · Model: claude-haiku-5-5 · Tools: none · Language: ar",
            assistantReportDetails("1.0.1", "anthropic", "claude-haiku-5-5", emptyList(), "ar"))
        assertTrue(assistantReportDetails("1.0.0", "openai", " ", listOf(""), "en").contains("Service: OpenAI · Model: default · Tools: none"))
    }

    @Test fun aReportsDetailsFollowTheReasonAndOnlyTheyAreCutToFit() {
        val details = "App: UrBudget Android 1.0.0 · Service: ChatGPT · Model: default · Tools: none · Language: en"
        assertEquals("It added the months wrongly.\n\n$details", assistantReportReason("  It added the months wrongly. ", details))
        // Cleared by the user, nothing is added; with no reason, they go alone.
        assertEquals("It added the months wrongly.", assistantReportReason("It added the months wrongly.", "  "))
        assertEquals(details, assistantReportReason("", details))
        // The whole keeps within the reason's 1,000 characters: the details give way, the user's words don't.
        val long = "x".repeat(950)
        val sent = assistantReportReason(long, details)
        assertEquals(1000, sent.length)
        assertEquals("$long\n\n${details.take(48)}", sent)
        val full = "y".repeat(999)
        assertEquals(full, assistantReportReason(full, details))
        assertEquals("z".repeat(1000), assistantReportReason("z".repeat(1000), details))
        // Never cut between the two halves of one character.
        val emoji = "💸".repeat(40)
        assertEquals("x".repeat(990) + "\n\n" + "💸".repeat(4), assistantReportReason("x".repeat(990), emoji))
        assertEquals("x".repeat(989) + "\n\n" + "💸".repeat(4), assistantReportReason("x".repeat(989), emoji))
    }

    @Test fun aFinishedReplyAndANewCardAreAnnouncedOnceAndNothingElseIs() {
        fun heard(running: Boolean, vararg kinds: String, conversation: String = "t1") = AssistantHeard.of(conversation, running, kinds.map { JSONObject().put("kind", it) })
        val asked = heard(true, "user")
        // Opening a conversation, and text as it's written, say nothing.
        assertEquals(emptyList<AssistantAnnouncement>(), AssistantAnnouncement.between(null, heard(false, "user", "assistant")))
        assertEquals(emptyList<AssistantAnnouncement>(), AssistantAnnouncement.between(heard(false, "user", "assistant", conversation = "t0"), heard(false, "user", "assistant", "proposal")))
        assertEquals(emptyList<AssistantAnnouncement>(), AssistantAnnouncement.between(asked, heard(true, "user", "step")))
        // A card as it arrives; the reply once it has finished.
        assertEquals(listOf(AssistantAnnouncement.Proposal), AssistantAnnouncement.between(heard(true, "user", "step"), heard(true, "user", "step", "proposal")))
        assertEquals(listOf(AssistantAnnouncement.Replied), AssistantAnnouncement.between(heard(true, "user", "step", "proposal"), heard(false, "user", "step", "proposal", "assistant")))
        assertEquals(listOf(AssistantAnnouncement.Proposal, AssistantAnnouncement.Replied), AssistantAnnouncement.between(asked, heard(false, "user", "proposal", "assistant")))
        assertEquals(listOf(AssistantAnnouncement.Replied), AssistantAnnouncement.between(asked, heard(false, "user", "assistant", "cut")))
        // Stopped, or failed: the screen says so, not this.
        assertEquals(emptyList<AssistantAnnouncement>(), AssistantAnnouncement.between(asked, heard(false, "user", "assistant", "stopped")))
        assertEquals(emptyList<AssistantAnnouncement>(), AssistantAnnouncement.between(asked, heard(false, "user", "error")))
        // A card changed after the reply (applied, discarded) is the same card.
        assertEquals(emptyList<AssistantAnnouncement>(), AssistantAnnouncement.between(heard(false, "user", "proposal", "assistant"), heard(false, "user", "proposal", "assistant")))
        assertEquals(listOf("The assistant replied.", "A proposal is ready to review."), AssistantAnnouncement.values().map { Words[it.words] })
    }

    @Test fun aCardsButtonsAreTheActionsTheCoreGivesThatThisAppKnows() {
        fun card(vararg actions: String, status: String = "proposed") = JSONObject().put("id", "p1").put("kind", "budget").put("status", status).put("actions", JSONArray(actions.toList()))
        // In the core's order, the first being the main one; one this app doesn't know is left out.
        val waiting = card("apply", "somethingNewer", "askChanges", "useOriginal", "discard", "show")
        assertEquals(listOf(ProposalAction.Apply, ProposalAction.AskChanges, ProposalAction.UseOriginal, ProposalAction.Discard, ProposalAction.Show), ProposalCards.buttons(waiting, null))
        assertEquals(emptyList<ProposalAction>(), ProposalCards.buttons(card("somethingNewer"), null))
        assertEquals(emptyList<ProposalAction>(), ProposalCards.buttons(JSONObject().put("id", "p1"), null))
        // Remove is in the card's menu and a question's answers are its rows: neither is a button, and whatever follows is the main one.
        assertEquals(listOf(ProposalAction.Preview, ProposalAction.Print, ProposalAction.Excel), ProposalCards.buttons(card("preview", "print", "excel", "remove"), null))
        assertEquals(listOf(ProposalAction.Show), ProposalCards.buttons(card("remove", "show"), null))
        assertEquals(listOf(ProposalAction.Remove, ProposalAction.Show), ProposalCards.actions(card("remove", "show"), null))
        assertEquals(emptyList<ProposalAction>(), ProposalCards.buttons(card("answer"), null))
        // Undo only while the budget's own Undo would take this card's change back.
        val applied = card("undo", "show", status = "applied")
        assertEquals(listOf(ProposalAction.Show), ProposalCards.buttons(applied, null))
        assertEquals(listOf(ProposalAction.Show), ProposalCards.buttons(applied, "p2"))
        assertEquals(listOf(ProposalAction.Undo, ProposalAction.Show), ProposalCards.buttons(applied, "p1"))
        // A finished card keeps Undo, Restore and Show on its line, and shows the rest once opened.
        assertTrue(listOf("applied", "discarded", "undone", "replaced").all { ProposalCards.finished(card(status = it)) })
        assertFalse(listOf("proposed", "failed").any { ProposalCards.finished(card(status = it)) })
        val undone = card("apply", "askChanges", "discard", "show", status = "undone")
        assertEquals(listOf(ProposalAction.Show), ProposalCards.lineButtons(undone, null))
        assertEquals(listOf(ProposalAction.Apply, ProposalAction.AskChanges, ProposalAction.Discard), ProposalCards.openedButtons(undone, null))
        assertEquals(listOf(ProposalAction.Restore) to emptyList<ProposalAction>(),
            card("restore", status = "replaced").let { ProposalCards.lineButtons(it, null) to ProposalCards.openedButtons(it, null) })
        // Every action the core can give has a name here, and a kind this app doesn't know is told from those it draws.
        assertEquals(listOf("apply", "applyToImport", "applyBankTexts", "askChanges", "useOriginal", "discard", "remove", "restore", "undo", "askAgain", "show",
            "preview", "print", "excel", "open", "save", "reviewImport", "saveCsv", "answer"), ProposalAction.values().map { it.key })
        assertTrue(listOf("budget", "transactions", "payees", "notes", "categories", "schedules", "delete", "fix", "bankTexts", "report", "spreadsheet", "import", "figures", "question")
            .all { ProposalCards.known(JSONObject().put("kind", it)) })
        assertFalse(ProposalCards.known(JSONObject().put("kind", "somethingNewer")))
    }

    @Test fun aFinishedCardsLineIsItsTitleAndUnderItWhereItStands() {
        Money.configure(JSONObject().put("currency", "SAR"))
        Money.useInterface(java.util.Locale.ENGLISH, false)
        fun card(status: String, kind: String = "budget") = JSONObject().put("kind", kind).put("status", status).put("title", "Cover Dining out")
        // The day as the budget writes dates, and the time where the phone is; on the day itself, the time alone.
        val riyadh = java.time.ZoneId.of("Asia/Riyadh")
        val hours = java.time.format.DateTimeFormatter.ofPattern("HH:mm")
        val clock: (String) -> String? = { assistantMoment(it, riyadh, hours, java.time.LocalDate.parse("2026-10-23")) }
        val thatDay: (String) -> String? = { assistantMoment(it, riyadh, hours, java.time.LocalDate.parse("2026-10-22")) }
        assertEquals("Cover Dining out", ProposalCards.finishedTitle(card("applied")))
        assertEquals("Applied 2026-10-21, 13:00", ProposalCards.finishedState(card("applied").put("appliedAt", "2026-10-21T10:00:00Z"), clock))
        assertEquals("Applied 2026-10-22, 01:30", ProposalCards.finishedState(card("applied").put("appliedAt", "2026-10-21T22:30:00.123456789+00:00"), clock))
        // The day is the phone's, not the core's: half past one in Riyadh is already the 22nd.
        assertEquals("Applied 01:30", ProposalCards.finishedState(card("applied").put("appliedAt", "2026-10-21T22:30:00Z"), thatDay))
        assertEquals("Applied 2026-10-21, 13:00", ProposalCards.finishedState(card("applied").put("appliedAt", "2026-10-21T10:00:00Z"), thatDay))
        // The core kept no time, or one that doesn't read: plain "Applied".
        assertEquals("Applied", ProposalCards.finishedState(card("applied"), clock))
        assertEquals("Applied", ProposalCards.finishedState(card("applied").put("appliedAt", "yesterday"), clock))
        assertEquals("Undone", ProposalCards.finishedState(card("undone"), clock))
        assertEquals("Replaced by a newer version", ProposalCards.finishedState(card("replaced"), clock))
        assertEquals("Discarded", ProposalCards.finishedState(card("discarded"), clock))
        // A thing the assistant made is removed, not discarded.
        assertEquals("Removed", ProposalCards.finishedState(card("discarded", "report").put("made", true), clock))
        // A question that was answered says what was asked, and nothing under it; set aside unanswered, it's a card like any other.
        val answered = card("applied", "question").put("appliedAt", "2026-10-21T10:00:00Z")
        assertEquals("Asked: Cover Dining out", ProposalCards.finishedTitle(answered))
        assertNull(ProposalCards.finishedState(answered, clock))
        assertEquals("Cover Dining out" to "Discarded", card("discarded", "question").let { ProposalCards.finishedTitle(it) to ProposalCards.finishedState(it, clock) })
    }

    @Test fun aCardSaysOneThingAboutItselfAndAskingAgainSendsItsReason() {
        val failed = JSONObject().put("title", "Cover Dining out").put("message", JSONObject().put("tone", "error").put("text", "This can't be applied any more."))
        assertEquals("error" to "This can't be applied any more.", ProposalCards.message(failed))
        assertNull(ProposalCards.message(JSONObject().put("title", "Cover Dining out")))
        assertNull(ProposalCards.message(JSONObject().put("message", JSONObject().put("tone", "info").put("text", ""))))
        assertEquals("Prepare “Cover Dining out” again. It couldn't be applied: This can't be applied any more.", ProposalCards.askAgain(failed))
    }

    @Test fun aConversationIsReadAgainWhenTheBudgetChangesOnlyForCardsTheBudgetMoves() {
        fun card(kind: String, status: String, made: Boolean = false) = JSONObject().put("kind", kind).put("status", status).put("made", made)
        assertTrue(listOf("proposed", "undone", "applied").all { ProposalCards.followsBudget(card("transactions", it)) })
        assertFalse(listOf("discarded", "replaced", "failed").any { ProposalCards.followsBudget(card("transactions", it)) })
        // A thing made, a question, an import's fixes and a phone's bank-text card don't change with the budget.
        assertFalse(ProposalCards.followsBudget(card("report", "proposed", made = true)))
        assertFalse(listOf("question", "fix", "bankTexts").any { ProposalCards.followsBudget(card(it, "proposed")) })
    }

    @Test fun aFiguresBarIsItsRowsShareOfTheLargestAmount() {
        fun rows(vararg amounts: Long?) = amounts.map { amount -> JSONObject().put("label", "Row").apply { amount?.let { put("amount", it) } } }
        assertEquals(listOf(1f, .5f, .25f, 0f), ProposalCards.barShares(rows(-40000, 20000, -10000, 0)))
        // By absolute value, so money in and money out compare; a row with no amount has no bar.
        assertEquals(listOf(.5f, 1f, 0f), ProposalCards.barShares(rows(1500, -3000, null)))
        assertEquals(listOf(0f, 0f), ProposalCards.barShares(rows(0, null)))
        assertEquals(emptyList<Float>(), ProposalCards.barShares(emptyList()))
    }

    @Test fun aDeleteCardCountsTheTransactionsStillChosen() {
        val rows = listOf(JSONObject().put("key", "t:a"), JSONObject().put("key", "t:b").put("skipped", true), JSONObject().put("key", "t:c"))
        assertEquals(2, ProposalCards.deleting(rows, 0))
        // None can be left out: every one goes, those left off the card too.
        assertEquals(5, ProposalCards.deleting(listOf(JSONObject().put("label", "a"), JSONObject().put("label", "b")), 3))
        assertEquals(listOf("Delete 1 transaction", "Delete 2 transactions"), listOf(1L, 2L).map { Words.count(R.plurals.assistant_delete_transactions, it) })
    }

    @Test fun showLeadsToACardsTransactionsOrElseItsMonthAndCategories() {
        fun card(highlight: JSONObject?) = JSONObject().put("card", JSONObject().apply { highlight?.let { put("highlight", it) } })
        val both = JSONObject().put("month", "2026-10").put("categories", JSONArray(listOf("c1", "c2"))).put("transactions", JSONArray(listOf("t1")))
        assertEquals(AssistantShow(null, emptyList(), listOf("t1")), ProposalCards.show(card(both)))
        assertEquals(AssistantShow("2026-10", listOf("c1", "c2"), emptyList()), ProposalCards.show(card(JSONObject(both.toString()).put("transactions", JSONArray()))))
        assertEquals(AssistantShow(null, listOf("c1"), emptyList()), ProposalCards.show(card(JSONObject().put("categories", JSONArray(listOf("c1"))).put("transactions", JSONArray()))))
        assertNull(ProposalCards.show(card(JSONObject().put("month", "2026-10").put("categories", JSONArray()).put("transactions", JSONArray()))))
        assertNull(ProposalCards.show(card(null)))
        assertNull(ProposalCards.show(JSONObject()))
    }

    @Test fun aFixCardIsMarkedWithTheFixesItsImportDidntTake() {
        fun result(fix: String, line: String?, applied: Boolean) = JSONObject().put("fix", fix).put("applied", applied).apply { line?.let { put("line", it) } }
        fun field(line: String, field: String) = JSONObject().put("line", line).put("field", field)
        val rows = listOf(
            JSONObject().put("label", "Dates read day first").put("key", "x"),
            JSONObject().put("label", "Line 4: date").put("key", "f:4:date"), JSONObject().put("label", "Line 4: amount").put("key", "f:4:amount"),
            JSONObject().put("label", "Line 9: payee").put("key", "f:9:payee"), JSONObject().put("label", "Include line 12").put("key", "i:12"))
        val proposal = JSONObject().put("card", JSONObject().put("rows", JSONArray(rows)))
            .put("fixes", JSONObject().put("fields", JSONArray(listOf(field("4", "date"), field("4", "amount"), field("9", "payee")))))
        fun note(vararg results: JSONObject) = ProposalCards.fixNote(proposal, JSONObject().put("results", JSONArray(results.toList())))
        // Every one took: nothing to say.
        assertNull(note(result("structure", null, true), result("field", "4", true), result("include", "12", true)))
        // A field's result is told from its line's others by its place among the card's field fixes.
        assertEquals("Fixes applied: 3 of 5. Not applied: Line 4: amount, Include line 12",
            note(result("structure", null, true), result("field", "4", true), result("field", "4", false), result("field", "9", true), result("include", "12", false)))
        assertEquals("Fixes applied: 0 of 2. Not applied: Dates read day first, Line 9: payee",
            note(result("structure", null, false), result("field", "9", false).also { proposal.getJSONObject("fixes").put("fields", JSONArray(listOf(field("9", "payee")))) }))
        // A fix with no row on the card is named by its line; more than five end with "…", each said once.
        val many = (1..7).map { result("revert", "$it", false) } + result("revert", "1", false) + result("cell", "2", true)
        assertEquals("Fixes applied: 1 of 9. Not applied: Line 1, Line 2, Line 3, Line 4, Line 5, …", note(*many.toTypedArray()))
    }

    @Test fun tickingManyRowsAtOnceKeepsAllNoneOrOnlyTheOnesToCheck() {
        val rows = listOf(JSONObject().put("key", "t:a"), JSONObject().put("key", "t:b").put("tone", "warn"), JSONObject().put("key", "t:c").put("skipped", true),
            JSONObject().put("label", "New rule"), JSONObject().put("key", "r:r1").put("tone", "warn"))
        val choices = JSONObject().put("skip", JSONArray(listOf("t:c"))).put("set", JSONArray(listOf(
            JSONObject().put("key", "t:a").put("category", "groceries"), JSONObject().put("key", "t:c").put("amount", 0))))
        // All of them: an amount of nothing given to a row goes when the row is in again; a category given stays.
        assertJson("""{"skip":[],"set":[{"key":"t:a","category":"groceries"}]}""", ProposalChoices.select(choices, rows) { true })
        assertJson("""{"skip":["t:a","t:b","t:c","r:r1"],"set":[{"key":"t:a","category":"groceries"},{"key":"t:c","amount":0}]}""", ProposalChoices.select(choices, rows) { false })
        assertJson("""{"skip":["t:a","t:c"],"set":[{"key":"t:a","category":"groceries"},{"key":"t:c","amount":0}]}""", ProposalChoices.select(choices, rows, ProposalChoices::toCheck))
        assertJson("""{"skip":[],"set":[]}""", ProposalChoices.select(null, rows) { true })
    }

    @Test fun choicesOnTheirWayToTheCoreShowOnTheCardsRowsAtOnce() {
        val careem = JSONObject().put("label", "Careem").put("amount", -3800).put("key", "t:a").put("detail", "Uncategorized → Transport")
            .put("edit", JSONObject().put("category", true).put("categoryId", "transport").put("amount", true))
        val extra = JSONObject().put("label", "Extra").put("amount", -89900).put("key", "t:b").put("skipped", true)
        val rule = JSONObject().put("label", "New rule").put("key", "r:r1")
        val rows = listOf(careem, extra, rule, JSONObject().put("label", "Total"))
        val names = mapOf("groceries" to "Groceries", "transport" to "Transport")
        val name = { id: String? -> if (id == null) "Unassigned" else names.getValue(id) }
        // Nothing on its way: the rows are the core's own.
        assertSame(rows, ProposalChoices.shown(rows, null, name))
        // Several choices made one after another are one revision, and the rows show every one of them.
        val pending = ProposalChoices.toggle(ProposalChoices.change(ProposalChoices.toggle(null, "r:r1", skipped = false), "t:a", JSONObject().put("category", "groceries").put("amount", -3000)), "t:b", skipped = true)
        assertJson("""{"skip":["r:r1"],"set":[{"key":"t:a","category":"groceries","amount":-3000}]}""", pending)
        val shown = ProposalChoices.shown(rows, pending, name)
        assertEquals(listOf(false, false, true, false), shown.map { it.optBoolean("skipped") })
        assertEquals(listOf(-3000L, true, "Groceries", "groceries"), shown[0].let { listOf(it.getLong("amount"), it.getBoolean("edited"), it.getString("given"), it.getJSONObject("edit").getString("categoryId")) })
        // The core's rows are never changed themselves, and a row no choice touches is the core's own.
        assertEquals(-3800L to "transport", careem.getLong("amount") to careem.getJSONObject("edit").getString("categoryId"))
        assertSame(rows[3], shown[3])
        // Ready to Assign is a category to give too; one the row already has isn't given again.
        val unassigned = ProposalChoices.shown(rows, ProposalChoices.change(null, "t:a", JSONObject().put("category", JSONObject.NULL)), name)[0]
        assertEquals("Unassigned" to true, unassigned.getString("given") to unassigned.getJSONObject("edit").isNull("categoryId"))
        val same = ProposalChoices.shown(rows, ProposalChoices.change(null, "t:a", JSONObject().put("category", "transport").put("amount", -3800)), name)[0]
        assertEquals(listOf(false, false), listOf(same.has("given"), same.optBoolean("edited")))
    }

    @Test fun notesForTheAssistantAreAddedCleanedAndRemovedAsAWholeList() {
        assertEquals(listOf("Rent is due on the 1st", "Salary lands on the 27th"), AssistantNotes.list(JSONObject().put("assistantNotes", JSONArray(listOf("Rent is due on the 1st", "", "Salary lands on the 27th")))))
        assertEquals(emptyList<String>(), AssistantNotes.list(JSONObject()))
        assertEquals(emptyList<String>(), AssistantNotes.list(null))
        val notes = listOf("Rent is due on the 1st")
        // A note is kept on one line with single spaces; nothing, or one that's there already, adds nothing.
        assertEquals(notes + "Careem is transport", AssistantNotes.adding("  Careem \n is\ttransport ", notes))
        assertNull(AssistantNotes.adding("   ", notes))
        assertNull(AssistantNotes.adding(" Rent is  due on the 1st", notes))
        // At most 200 characters, whole ones only.
        assertEquals(200, AssistantNotes.cleaned("x".repeat(250)).length)
        assertEquals("💸".repeat(200), AssistantNotes.cleaned("💸".repeat(201)))
        assertEquals("x".repeat(199), AssistantNotes.cleaned("x".repeat(199) + "  yz"))
        assertEquals(listOf("a", "c"), AssistantNotes.removing(1, listOf("a", "b", "c")))
        assertEquals(listOf("a"), AssistantNotes.removing(4, listOf("a")))
    }

    @Test fun anImportsNewTransactionsWithNoCategoryAreOfferedToTheAssistant() {
        fun account(id: String, name: String, kind: String = "checking") = JSONObject().put("id", id).put("name", name).put("kind", kind)
        fun transaction(id: String, account: String, category: Any? = null, transfer: String? = null) = JSONObject().put("id", id).put("accountId", account)
            .apply { transfer?.let { put("transferAccountId", it) } }.put("splits", JSONArray(listOf(JSONObject().put("amount", -1000).put("categoryId", category ?: JSONObject.NULL))))
        val accounts = JSONArray(listOf(account("a1", "Checking"), account("a2", "Savings"), account("a3", "Brokerage", "tracking")))
        val categories = JSONArray(listOf(JSONObject().put("id", "c1").put("name", "Groceries"), JSONObject().put("id", "c2").put("name", "Uncategorized")))
        val before = JSONObject().put("accounts", accounts).put("categories", categories).put("transactions", JSONArray(listOf(transaction("old", "a1"))))
        val after = JSONObject(before.toString()).put("transactions", JSONArray(listOf(transaction("old", "a1"), transaction("n1", "a1"), transaction("n2", "a1", "c1"),
            transaction("n3", "a2", "c2"), transaction("n4", "a3"), transaction("n5", "a2", transfer = "a1"), transaction("n6", "a1", "local-uncategorized"))))
        // New, in a budget account, not a transfer, and with no category or only a placeholder for one. A rule may have categorized the rest.
        val imported = AssistantStart.uncategorized(before, after)
        assertEquals(AssistantStart.Imported(6, listOf("n1", "n3", "n6"), listOf("Checking", "Savings")), imported)
        assertEquals(AssistantStart.Imported(), AssistantStart.uncategorized(after, after))
        assertEquals("Categorize the 3 new transactions in Checking and Savings.", AssistantStart.categorize(imported))
        assertEquals("Categorize the new transaction in Checking.", AssistantStart.categorize(AssistantStart.Imported(1, listOf("n1"), listOf("Checking"))))
        assertEquals("Categorize these 200 of the 230 new transactions in A, B and C.", AssistantStart.categorize(AssistantStart.Imported(230, (1..230).map { "t$it" }, listOf("A", "B", "C"))))
        assertEquals(listOf("", "Checking"), listOf(emptyList(), listOf("Checking")).map(AssistantStart::joined))
        assertEquals(listOf("Imported 1 transaction for review.", "Imported 12 transactions from your bank for review."),
            listOf(Words.count(R.plurals.bank_imported_review, 1), Words.count(R.plurals.bank_sync_imported_review, 12)))
    }

    /** Key order is the JSON library's, not the revision's. */
    private fun assertJson(expected: String, actual: JSONObject?) = assertTrue("$actual", actual != null && sameJson(JSONObject(expected), actual))
    private fun keys(revision: JSONObject, name: String) = revision.getJSONArray(name).let { list -> (0 until list.length()).map(list::getString) }

    private fun conversation(id: String, count: Int) = JSONObject().put("id", id).put("running", false)
        .put("entries", JSONArray((0 until count).map { JSONObject().put("kind", "assistant").put("text", "Message $it") }))
}
