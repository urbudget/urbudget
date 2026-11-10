package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * Append tokens once; publish at a bounded cadence instead of copying a reply for every token.
 * What's published is the live reply's text and the latest line of the model's thinking.
 */
internal class AssistantStreamBuffer {
    /** Text whose round has ended, kept on screen until the conversation is read back with it. */
    private val settled = StringBuilder()
    private val reply = StringBuilder()
    private val reasoning = StringBuilder()
    private var changed = false

    fun append(text: String, thinking: Boolean = false) {
        if (text.isEmpty()) return
        if (thinking) {
            reasoning.append(text)
            // Only its latest line is shown, so only its tail is kept.
            if (reasoning.length > 4_000) reasoning.delete(0, reasoning.length - 2_000)
        } else {
            // Words are arriving: the thinking that led to them is over.
            reply.append(text); reasoning.clear()
        }
        changed = true
    }

    fun clearReply() { reply.clear(); changed = true }

    /** The thinking is over: the round moved on to a lookup, or ended. */
    fun rested() { if (reasoning.isNotEmpty()) { reasoning.clear(); changed = true } }

    /** The round's text is whole. It stays as the live reply until [shown] says its entry is on screen, so the two never swap through an empty frame. */
    fun settle() {
        if (reply.isNotEmpty()) { if (settled.isNotEmpty()) settled.append("\n\n"); settled.append(reply); reply.clear() }
        reasoning.clear(); changed = true
    }

    /** The conversation now shows what [settle] kept. */
    fun shown() { if (settled.isNotEmpty()) { settled.clear(); changed = true } }

    fun clear() { settled.clear(); reply.clear(); reasoning.clear(); changed = false }
    fun drain(): Pair<String, String>? = if (!changed) null else {
        changed = false
        val text = if (settled.isEmpty()) reply.toString() else if (reply.isEmpty()) settled.toString() else "$settled\n\n$reply"
        text to assistantThinkingLine(reasoning)
    }
}

/** The latest line of the model's thinking, without the Markdown marks around a heading: the panel shows one line, not all of it. */
internal fun assistantThinkingLine(thinking: CharSequence): String =
    thinking.lineSequence().lastOrNull { it.isNotBlank() }?.trim()?.trim('*', '#', '_', '`', ' ').orEmpty()

/** Whether the conversation's latest reply is [text]: the entry a `message` event announced has been read back. */
internal fun assistantShows(entries: List<JSONObject>, text: String): Boolean =
    entries.lastOrNull { it.optString("kind") == "assistant" }?.optString("text")?.trim() == text.trim()

/** How long to wait between reads of the core's events: short while a reply runs, when the phone's answers to it must not wait. */
internal fun assistantPollDelay(replying: Boolean): Long = if (replying) 100 else 300

/** The kinds of entry a conversation has. One this app doesn't know is not drawn: never as an empty reply. */
internal enum class AssistantEntryKind {
    User, Assistant, Step, Proposal, Stopped, Error, Cut;

    companion object {
        fun of(entry: JSONObject): AssistantEntryKind? = when (entry.optString("kind")) {
            "user" -> User
            "assistant" -> Assistant
            "step" -> Step
            "proposal" -> Proposal
            "stopped" -> Stopped
            "error" -> Error
            "cut" -> Cut
            else -> null
        }
    }
}

/** A part of what goes with a message. Each can be taken off the next one; a new message starts with them all again. */
enum class AssistantPart { Place, Transactions, Import }

/** Whether a page shows a month, so the month is part of where the user is. */
internal fun assistantPageShowsMonth(page: String): Boolean = page == "budget" || page == "reports"

/** The words that name a page, from the key a message was sent with (`AssistantPlace.page`): the page's own title in the app. None for a key this app doesn't send. */
internal fun assistantPageName(page: String): Int? = when (page) {
    "budget" -> R.string.tab_budget
    "transactions" -> R.string.tab_transactions
    "accounts" -> R.string.tab_accounts
    "reports" -> R.string.tab_reports
    "scheduled" -> R.string.page_scheduled
    "payees" -> R.string.page_payees
    "settings" -> R.string.page_settings
    "bankMessages" -> R.string.sms_title
    "bankConnections" -> R.string.page_banks
    "import" -> R.string.import_bank_file
    "Account & sync" -> R.string.page_account_sync
    "Budget library" -> R.string.page_library
    "assistant" -> R.string.assistant_title
    else -> null
}

/**
 * What goes with the next message, as "With this message" lists it and as `screen` sends it:
 * where the user is (none once taken off), the month when that page shows one, the selected
 * transactions and the bank file being imported.
 */
data class AssistantSending(val place: AssistantPlace? = null, val month: String? = null, val selected: List<String> = emptyList(), val import: JSONObject? = null)

/**
 * [place] is where the app is and [importing] the bank file open under the assistant; [category]
 * is one handed over with "Ask the assistant", which then counts as the one in view. Parts in
 * [without] don't go.
 */
internal fun assistantSending(place: AssistantPlace, month: String, selected: List<String>, importing: JSONObject?, without: Set<AssistantPart> = emptySet(),
                              category: String? = null): AssistantSending {
    val here = place.copy(page = if (importing != null) "import" else place.page, categoryId = category ?: place.categoryId)
        .takeUnless { AssistantPart.Place in without }
    return AssistantSending(here, month.takeIf { here != null && it.isNotEmpty() && assistantPageShowsMonth(here.page) },
        if (AssistantPart.Transactions in without) emptyList() else selected, importing.takeUnless { AssistantPart.Import in without })
}

/** The `screen` of `assistant_send` and `assistant_retry`. No page means the user took where they are off the message: then no month, account or category goes either. */
internal fun assistantScreen(sending: AssistantSending, today: String, bankTexts: Boolean): JSONObject {
    val screen = JSONObject().put("platform", "android").put("page", sending.place?.page.orEmpty())
    sending.place?.let { place ->
        sending.month?.let { screen.put("month", it) }
        place.accountId?.let { screen.put("accountId", it) }
        place.categoryId?.let { screen.put("categoryId", it) }
    }
    return screen.put("today", today).put("selected", JSONArray(sending.selected)).put("import", sending.import ?: JSONObject.NULL).put("bankTexts", bankTexts)
}

/** At most this many selected transactions go with a message (services `assistant_send`). */
internal const val AssistantSelectionLimit = 200

/** A selection as it's handed to the assistant: its first 200, and how many there were when that left some out. */
internal fun assistantSelection(ids: List<String>): Pair<List<String>, Int?> =
    if (ids.size > AssistantSelectionLimit) ids.take(AssistantSelectionLimit) to ids.size else ids to null

/** The composer's text with [addition] put in: on a new line after what's already typed, never over it, and not a second time. */
internal fun assistantDraftWith(draft: String, addition: String): String = when {
    draft.isBlank() -> addition
    draft.trimEnd().endsWith(addition.trimEnd()) -> draft
    else -> draft.trimEnd() + "\n" + addition
}

/** One look at the conversation's list: where it's scrolled to, whether more lies below, and whether a scroll is under way. */
internal data class AssistantListFrame(val index: Int, val offset: Int, val more: Boolean, val scrolling: Boolean)

/**
 * Whether new text is followed to the end after [now]. At the end it is, however the list got
 * there. A scroll that moved the list and left it short of the end stops it: the user is reading
 * further up. Text arriving under a still list changes nothing, so the list follows only when it
 * was at the end before the text came.
 */
internal fun assistantFollows(following: Boolean, before: AssistantListFrame?, now: AssistantListFrame): Boolean = when {
    !now.more -> true
    now.scrolling && before != null && (before.index != now.index || before.offset != now.offset) -> false
    else -> following
}

/**
 * What stands under a reply for copying, reporting and rating it. The latest reply has its buttons
 * out; an earlier one keeps them behind one quiet button, so a long conversation isn't a column of
 * them. A reply of the turn still being answered has none yet: they'd come in under it while the
 * rest of the answer is written below, and push that down.
 */
internal enum class AssistantReplyTools { Shown, Folded, Waiting }

/** [index] is the reply's place among [entries], and [running] whether the conversation's reply is still being made. */
internal fun assistantReplyTools(index: Int, entries: List<JSONObject>, running: Boolean): AssistantReplyTools = when {
    running && index > entries.indexOfLast { it.optString("kind") == "user" } -> AssistantReplyTools.Waiting
    index == entries.indexOfLast { it.optString("kind") == "assistant" } -> AssistantReplyTools.Shown
    else -> AssistantReplyTools.Folded
}

/** The conversations whose title has [query] in it, whatever the case; all of them for no query. */
internal fun assistantThreadsMatching(threads: List<JSONObject>, query: String): List<JSONObject> {
    val wanted = query.trim()
    return if (wanted.isEmpty()) threads else threads.filter { it.optString("title").contains(wanted, ignoreCase = true) }
}

/** The tokens a conversation's requests used: sent, of those from cache, and received. None when it has used none. */
internal data class AssistantUsage(val sent: Long, val cached: Long, val received: Long) {
    companion object {
        fun of(thread: JSONObject?): AssistantUsage? {
            val usage = thread?.optJSONObject("usage") ?: return null
            return AssistantUsage(usage.optLong("input"), usage.optLong("cached"), usage.optLong("output")).takeIf { it.sent != 0L || it.cached != 0L || it.received != 0L }
        }
    }
}

/**
 * The line of details a report of a reply starts with, for the user to edit or clear. It isn't
 * translated: it's read by whoever handles the report.
 */
internal fun assistantReportDetails(version: String, provider: String, model: String?, tools: List<String>, language: String): String {
    val service = when (provider) { "anthropic" -> "Anthropic"; "openai" -> "OpenAI"; else -> "ChatGPT" }
    return "App: UrBudget Android $version · Service: $service · Model: ${model?.takeIf(String::isNotBlank) ?: "default"} · " +
        "Tools: ${tools.filter(String::isNotBlank).joinToString(", ").ifEmpty { "none" }} · Language: $language"
}

/** A report's reason can be this long, counted as the service counts it (services `report_payload`). */
internal const val AssistantReportReasonLimit = 1_000

/** The reason as it's sent: the user's words, then the details after a blank line. Only the details are cut to fit. */
internal fun assistantReportReason(reason: String, details: String): String {
    val words = reason.trim().cut(AssistantReportReasonLimit)
    val line = details.trim()
    return when {
        line.isEmpty() -> words
        words.isEmpty() -> line.cut(AssistantReportReasonLimit)
        else -> (AssistantReportReasonLimit - words.length - 2).let { room -> if (room <= 0) words else (words + "\n\n" + line.cut(room)).trimEnd() }
    }
}

/** The first [length] characters, never ending between the two halves of one. */
private fun String.cut(length: Int): String =
    if (this.length <= length) this else take(if (length > 0 && Character.isHighSurrogate(this[length - 1])) length - 1 else length)

/** What a conversation's panel last showed, for telling what's new in it. */
internal data class AssistantHeard(val conversation: String, val running: Boolean, val cards: Int, val last: AssistantEntryKind?) {
    companion object {
        fun of(conversation: String, running: Boolean, entries: List<JSONObject>) = AssistantHeard(conversation, running,
            entries.count { it.optString("kind") == "proposal" }, entries.lastOrNull()?.let(AssistantEntryKind::of))
    }
}

/** What's read aloud: a reply once it has finished, and a card when it arrives. Never the text as it's written. */
internal enum class AssistantAnnouncement(val words: Int) {
    Replied(R.string.assistant_replied), Proposal(R.string.assistant_proposal_ready);

    companion object {
        /** What happened between two looks at the same conversation. Opening one announces nothing. */
        fun between(before: AssistantHeard?, now: AssistantHeard): List<AssistantAnnouncement> {
            if (before == null || before.conversation != now.conversation) return emptyList()
            return listOfNotNull(
                Proposal.takeIf { now.cards > before.cards && (before.running || now.running) },
                // A reply the user stopped, or one that failed, says so on screen instead.
                Replied.takeIf { before.running && !now.running && (now.last == AssistantEntryKind.Assistant || now.last == AssistantEntryKind.Cut) })
        }
    }
}

/** All comparisons run on a worker. Unchanged messages retain their Compose/render caches. */
internal data class AssistantThreadPresentation(val thread: JSONObject, val entries: List<JSONObject>)

internal fun assistantThreadPresentation(thread: JSONObject, previous: AssistantThreadPresentation?): AssistantThreadPresentation {
    val sameThread = previous?.thread?.optString("id") == thread.optString("id")
    if (sameThread && sameJson(previous?.thread, thread)) return previous!!
    val entries = thread.records("entries").mapIndexed { index, entry ->
        val old = if (sameThread) previous?.entries?.getOrNull(index) else null
        if (old != null && sameJson(old, entry)) old else entry
    }
    return AssistantThreadPresentation(thread, entries)
}

/**
 * What the button under an error does, with its label. The core says which in the entry's `action`
 * (services `history::error_action`), and only for the conversation's last entry: an earlier error
 * has none, and neither has one that asking again wouldn't help ("none"), so those show no button.
 */
internal enum class AssistantErrorAction(val label: Int) {
    /** Sign in with ChatGPT again, then ask for the reply again. */
    SignIn(R.string.assistant_continue_chatgpt),
    /** The key, the model or the service needs changing: the assistant's settings. */
    Settings(R.string.assistant_settings),
    /** The plan's limit was reached: ChatGPT's usage page. */
    Usage(R.string.assistant_plan_usage),
    /** The conversation no longer fits the model: carry it on in a new one. */
    NewConversation(R.string.assistant_continue_new),
    Retry(R.string.assistant_try_again);

    companion object {
        fun of(entry: JSONObject): AssistantErrorAction? = when (entry.optString("action")) {
            "signIn" -> SignIn
            "settings" -> Settings
            "usage" -> Usage
            "newConversation" -> NewConversation
            "retry" -> Retry
            else -> null
        }
    }
}

/** Whose service the messages go to, as the notice of what's sent names it: a ChatGPT plan and an OpenAI key are both OpenAI's. */
internal fun assistantProviderCompany(provider: String): String = if (provider == "anthropic") "Anthropic" else "OpenAI"

/**
 * What a proposal card can offer, as the core names it in the card's `actions`. The card draws a
 * button for each one this app knows, in the order given, and leaves out any other.
 */
internal enum class ProposalAction(val key: String) {
    Apply("apply"), ApplyToImport("applyToImport"), ApplyBankTexts("applyBankTexts"), AskChanges("askChanges"), UseOriginal("useOriginal"),
    Discard("discard"), Remove("remove"), Restore("restore"), Undo("undo"), AskAgain("askAgain"), Show("show"),
    Preview("preview"), Print("print"), Excel("excel"), Open("open"), Save("save"), ReviewImport("reviewImport"), SaveCsv("saveCsv"), Answer("answer");

    companion object {
        private val known = values().associateBy { it.key }
        fun of(key: String): ProposalAction? = known[key]
    }
}

/** Where a card's "Show in the budget" leads: the register with [transactions] selected, or else the Budget tab on [month] with [categories] marked. */
internal data class AssistantShow(val month: String?, val categories: List<String>, val transactions: List<String>)

/**
 * When a card was applied, as the app writes a day and a time ("21/10/2026, 14:30"), and no longer
 * than it needs to be under the card's title: the time alone on [today]. None when [at] (RFC 3339)
 * doesn't read.
 */
internal fun assistantMoment(at: String, zone: ZoneId = ZoneId.systemDefault(), time: DateTimeFormatter = DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT),
                             today: LocalDate = LocalDate.now(zone)): String? =
    runCatching { OffsetDateTime.parse(at).atZoneSameInstant(zone) }.getOrNull()?.let {
        val clock = Money.ltr(it.toLocalTime().format(time))
        if (it.toLocalDate() == today) clock else Words[R.string.sms_moment, Money.date(it.toLocalDate().toString()), clock]
    }

/**
 * A proposal card as the core's view of it says to draw it (its `kind`, `status`, `made`,
 * `actions`, `message` and `appliedAt`). Nothing here works out from kind and status what a card
 * offers: that is the core's `actions`.
 */
internal object ProposalCards {
    private val kinds = setOf("budget", "transactions", "payees", "notes", "categories", "schedules", "delete", "fix", "bankTexts",
        "report", "spreadsheet", "import", "figures", "question")
    private val onLine = setOf(ProposalAction.Undo, ProposalAction.Restore, ProposalAction.Show)

    /** Whether this app draws the card's kind in full. Any other is drawn plainly, with a line saying an update is needed. */
    fun known(proposal: JSONObject): Boolean = proposal.optString("kind") in kinds

    /**
     * The actions the card offers that this app knows, in the core's order. Undo is one only
     * while the budget's own Undo would take this card's change back: [undoable] is the id of
     * the card it would (the conversation's `undoable`).
     */
    fun actions(proposal: JSONObject, undoable: String?): List<ProposalAction> = proposal.strings("actions").mapNotNull(ProposalAction::of)
        .filter { it != ProposalAction.Undo || (!undoable.isNullOrEmpty() && undoable == proposal.optString("id")) }

    /** The card's buttons, the first being the main one. Remove is in the card's menu and a question's answers are its rows, so neither is a button. */
    fun buttons(proposal: JSONObject, undoable: String?): List<ProposalAction> =
        actions(proposal, undoable).filter { it != ProposalAction.Remove && it != ProposalAction.Answer }

    /** A card that's done with: applied, set aside, undone or replaced. It folds to one line. */
    fun finished(proposal: JSONObject): Boolean = proposal.optString("status") in setOf("applied", "discarded", "undone", "replaced")

    /** The buttons a finished card keeps beside its line: Undo, Restore and Show. */
    fun lineButtons(proposal: JSONObject, undoable: String?): List<ProposalAction> = buttons(proposal, undoable).filter { it in onLine }

    /** The buttons a finished card shows once it's opened: whatever else it still offers. */
    fun openedButtons(proposal: JSONObject, undoable: String?): List<ProposalAction> = buttons(proposal, undoable).filter { it !in onLine }

    /** A question that was answered: its line says what was asked, and it has no state to show. */
    private fun asked(proposal: JSONObject): Boolean = proposal.optString("kind") == "question" && proposal.optString("status") == "applied"

    /** A finished card's line: its title, with the line to itself. A question that was answered reads "Asked: …". */
    fun finishedTitle(proposal: JSONObject): String =
        proposal.optString("title").let { title -> if (asked(proposal)) Words[R.string.assistant_asked, title] else title }

    /**
     * Where a finished card stands, under its title: "Applied 21/10/2026, 14:30" (the time alone
     * today, plain "Applied" when the core kept no time), "Undone", "Replaced by a newer
     * version", "Discarded", or "Removed" for a thing the assistant made. None for a question
     * that was answered: its title says it.
     */
    fun finishedState(proposal: JSONObject, moment: (String) -> String? = ::assistantMoment): String? = when (proposal.optString("status")) {
        "applied" -> if (asked(proposal)) null
            else moment(proposal.optString("appliedAt"))?.let { Words[R.string.assistant_applied_at, it] } ?: Words[R.string.assistant_applied]
        "undone" -> Words[R.string.assistant_undone]
        "replaced" -> Words[R.string.assistant_replaced]
        else -> Words[if (proposal.optBoolean("made")) R.string.assistant_removed else R.string.assistant_discarded]
    }

    /** What the card has to say about itself: its tone ("info", "warn" or "error") and its words. None when it says nothing. */
    fun message(proposal: JSONObject): Pair<String, String>? =
        proposal.optJSONObject("message")?.let { said -> said.optString("text").takeIf(String::isNotEmpty)?.let { said.optString("tone") to it } }

    /** What "Ask for a new version" sends: the card's title, and why it couldn't be applied. */
    fun askAgain(proposal: JSONObject): String =
        Words[R.string.assistant_ask_again_prompt, proposal.optString("title"), message(proposal)?.second.orEmpty()]

    /**
     * A change the budget's own changes move: one that waits is made again on the budget each
     * time the conversation is read, and the budget's Undo and Redo take an applied one back and
     * put it back. A conversation with one is read again when the budget changes.
     */
    fun followsBudget(proposal: JSONObject): Boolean = !proposal.optBoolean("made") && proposal.optString("kind") !in setOf("question", "fix", "bankTexts") &&
        proposal.optString("status") in setOf("proposed", "undone", "applied")

    /**
     * How long each row's bar is on a figures card: the row's share of the largest amount on the
     * card, by absolute value, from 0 to 1. A card with no amounts has bars of no length.
     */
    fun barShares(rows: List<JSONObject>): List<Float> {
        val sizes = rows.map { Math.abs(it.optLong("amount")).toDouble() }
        val largest = sizes.maxOrNull()?.takeIf { it > 0 } ?: return rows.map { 0f }
        return sizes.map { (it / largest).toFloat() }
    }

    /**
     * How many transactions a delete card deletes: its rows still chosen, or every one of them
     * ([more] counts those left off the card) when none can be left out.
     */
    fun deleting(rows: List<JSONObject>, more: Int): Int {
        val keyed = rows.filter { it.optString("key").isNotEmpty() }
        return if (keyed.isEmpty()) rows.size + more else keyed.count { !it.optBoolean("skipped") }
    }

    /** Where the card's Show leads, from its `highlight`: its transactions when it names any, else its month and categories. None when it names neither. */
    fun show(proposal: JSONObject): AssistantShow? {
        val highlight = proposal.optJSONObject("card")?.optJSONObject("highlight") ?: return null
        val transactions = highlight.strings("transactions")
        val categories = highlight.strings("categories")
        return when {
            transactions.isNotEmpty() -> AssistantShow(null, emptyList(), transactions)
            categories.isNotEmpty() -> AssistantShow(highlight.optString("month").ifEmpty { null }, categories, emptyList())
            else -> null
        }
    }

    /**
     * What a `fix` card is marked with when its import took only some of its fixes: "Fixes
     * applied: 1 of 3. Not applied: …", naming the rows that didn't take, up to five then "…".
     * None when every one took. [applied] is what the import says became of each fix
     * (`bankApplyFixes`), in the order of the card's `fixes`.
     */
    fun fixNote(proposal: JSONObject, applied: JSONObject): String? {
        val results = applied.records("results")
        val took = results.count { it.optBoolean("applied") }
        if (took == results.size) return null
        val rows = proposal.optJSONObject("card")?.records("rows").orEmpty()
        val fields = proposal.optJSONObject("fixes")?.records("fields").orEmpty()
        val places = HashMap<String, Int>()
        val labels = LinkedHashSet<String>()
        for (result in results) {
            val kind = result.optString("fix")
            val place = places[kind] ?: 0
            places[kind] = place + 1
            if (result.optBoolean("applied")) continue
            val line = if (result.isNull("line")) "" else result.optString("line")
            // The row's key on the card, as the core makes it for each kind of fix.
            val key = when (kind) {
                "structure" -> "x"
                "include" -> "i:$line"
                "field" -> fields.getOrNull(place)?.let { "f:$line:${it.optString("field")}" }
                else -> null
            }
            val label = rows.firstOrNull { key != null && it.optString("key") == key }?.optString("label").orEmpty()
                .ifEmpty { if (line.isEmpty()) "" else Words[R.string.bank_line_number, line] }
            if (label.isNotEmpty()) labels += label
        }
        val comma = Words[R.string.list_separator]
        val list = labels.take(5).joinToString(comma) + if (labels.size > 5) "$comma…" else ""
        return Words[R.string.assistant_fix_note, Money.count(took.toLong()), Money.count(results.size.toLong()), list]
    }
}

/** How long a card waits after the user's last choice on it before the choices go to the core, in one call. */
internal const val ProposalChoiceDelay = 600L

/**
 * The notes the assistant keeps for the open budget (its `assistantNotes`), as the assistant's
 * settings list and save them: always the whole list.
 */
internal object AssistantNotes {
    /** A note's length, in the characters the core counts. */
    const val Length = 200
    /** How many notes a budget keeps. */
    const val Limit = 30

    /** The budget's notes; none when it has no `assistantNotes`. */
    fun list(budget: JSONObject?): List<String> = budget.strings("assistantNotes").filter(String::isNotEmpty)

    /** [note] as the core keeps one: on one line with single spaces, and at most 200 characters, whole ones only. */
    fun cleaned(note: String): String {
        val line = note.split(Regex("\\s+")).filter(String::isNotEmpty).joinToString(" ")
        return line.substring(0, line.offsetByCodePoints(0, minOf(Length, line.codePointCount(0, line.length)))).trim()
    }

    /** The list with [note] at its end; none when there's nothing to add, or it's there already. */
    fun adding(note: String, notes: List<String>): List<String>? = cleaned(note).takeIf { it.isNotEmpty() && it !in notes }?.let { notes + it }

    /** The list without the note at [index]. */
    fun removing(index: Int, notes: List<String>): List<String> = notes.filterIndexed { at, _ -> at != index }
}

/**
 * Starting points other pages offer: the assistant opened with a request in its message field
 * for the user to send. Nothing is sent by itself.
 */
internal object AssistantStart {
    /** What an import added: how many transactions, and of those the ones that still have no category, with the names of the accounts they're in. */
    data class Imported(val added: Int = 0, val ids: List<String> = emptyList(), val accounts: List<String> = emptyList())

    /**
     * The transactions in [after] that weren't in [before], and of them those that still have no
     * category: none at all, or the Uncategorized placeholder (the desktop's uncategorizedImports).
     * Payee rules may have categorized the rest. Tracking accounts use no categories, and
     * transfers need none.
     */
    fun uncategorized(before: JSONObject, after: JSONObject): Imported {
        val known = before.records("transactions").mapTo(HashSet()) { it.optString("id") }
        val budgeted = after.records("accounts").filter { it.optString("kind") != "tracking" && it.optBoolean("onBudget", true) }
            .associate { it.optString("id") to it.optString("name") }
        val placeholders = after.records("categories").filter { it.optString("name") in setOf("Uncategorized", "Uncategorized Transactions") }
            .mapTo(hashSetOf("local-uncategorized")) { it.optString("id") }
        fun JSONObject.none(key: String) = isNull(key) || optString(key).isEmpty()
        var added = 0
        val ids = mutableListOf<String>()
        val accounts = LinkedHashSet<String>()
        for (transaction in after.records("transactions")) {
            if (transaction.optString("id") in known) continue
            added++
            val name = budgeted[transaction.optString("accountId")] ?: continue
            if (!transaction.none("transferAccountId")) continue
            val open = transaction.records("splits").all { part -> part.none("transferAccountId") && (part.none("categoryId") || part.optString("categoryId") in placeholders) }
            if (!open) continue
            ids += transaction.optString("id")
            accounts += name
        }
        return Imported(added, ids, accounts.toList())
    }

    /**
     * What the assistant is asked about them, ready to send with them attached (the desktop's
     * categorizeText). A message takes 200 of them: over that, it says these are the first.
     */
    fun categorize(imported: Imported): String {
        val where = joined(imported.accounts)
        val count = imported.ids.size
        return when {
            count > AssistantSelectionLimit -> Words.count(R.plurals.assistant_categorize_first, count.toLong(), Money.count(AssistantSelectionLimit.toLong()), where)
            count == 1 -> Words[R.string.assistant_categorize_one, where]
            else -> Words.count(R.plurals.assistant_categorize_new, count.toLong(), where)
        }
    }

    /** "Checking", "Checking and Savings", "Checking, Savings and Cash". */
    fun joined(names: List<String>): String =
        if (names.size < 2) names.firstOrNull().orEmpty() else Words[R.string.chart_and, names.dropLast(1).joinToString(Words[R.string.list_separator]), names.last()]
}

private fun JSONObject?.strings(key: String): List<String> =
    this?.optJSONArray(key)?.let { list -> (0 until list.length()).map(list::optString) }.orEmpty()

/**
 * What the user chose on a proposal card, as `assistant_revise` takes it (desktop ProposalCard):
 * `skip` lists the keys of rows left out, `set` the rows given another category (null: Ready to
 * Assign) or amount. Each choice is made from the card's current `choices`.
 */
internal object ProposalChoices {
    /** The assistant's own version: nothing left out or changed. */
    fun original(): JSONObject = revision(emptyList(), emptyList())

    /** `choices` with the row `key` left out, or put back when it was; one put back loses an amount of nothing given to it. */
    fun toggle(choices: JSONObject?, key: String, skipped: Boolean): JSONObject {
        val skip = choices.strings("skip")
        val set = choices?.records("set").orEmpty()
        return if (skipped) revision(skip.filter { it != key }, set.filterNot { it.optString("key") == key && it.has("amount") && !it.isNull("amount") && it.optLong("amount") == 0L })
        else revision(skip + key, set)
    }

    /** `choices` with `change` (a category and/or an amount) given to the row `key`, over what it was given before. */
    fun change(choices: JSONObject?, key: String, change: JSONObject): JSONObject {
        val set = choices?.records("set").orEmpty()
        val given = JSONObject(set.firstOrNull { it.optString("key") == key }?.toString() ?: "{}")
        change.keys().forEach { given.put(it, change.get(it)) }
        return revision(choices.strings("skip"), set.filterNot { it.optString("key") == key } + given.put("key", key))
    }

    /**
     * What saving a row's editor gives it: `category` (null: Ready to Assign) when it isn't the one
     * the row has, and the typed amount when it isn't the row's; null when nothing changed. An
     * amount typed without a sign keeps the row's sign. Throws when the amount doesn't read.
     */
    fun edit(row: JSONObject, category: String?, amount: String): JSONObject? {
        val edit = row.optJSONObject("edit") ?: return null
        val change = JSONObject()
        val had = edit.optString("categoryId").takeUnless { edit.isNull("categoryId") || it.isEmpty() }
        if (edit.optBoolean("category") && category != had) change.put("category", category ?: JSONObject.NULL)
        if (edit.optBoolean("amount")) {
            val was = if (row.has("amount") && !row.isNull("amount")) row.optLong("amount") else null
            val typed = amount.trim()
            var cents = Money.parse(typed)
            if (!typed.startsWith("-") && !typed.startsWith("+") && (was ?: 0L) < 0) cents = -Math.abs(cents)
            if (cents != was) change.put("amount", cents)
        }
        return change.takeIf { it.length() > 0 }
    }

    /** How much of a card Apply makes: `chosen` of its `total` parts ("Apply 2 of 3"). */
    data class Partial(val chosen: Int, val total: Int)

    /** How much of the card Apply makes while the user is choosing and some of its rows are left out; null otherwise. The card words it. */
    fun partial(rows: List<JSONObject>, choosing: Boolean): Partial? {
        val keyed = rows.filter { it.optString("key").isNotEmpty() }
        val chosen = keyed.count { !it.optBoolean("skipped") }
        return if (choosing && chosen < keyed.size) Partial(chosen, keyed.size) else null
    }

    /**
     * `choices` with every row of [rows] that can be left out in or out, as [keep] says of each:
     * all of them, none, or only the ones marked to check. Categories and amounts given stay,
     * but for an amount of nothing on a row that's in again.
     */
    fun select(choices: JSONObject?, rows: List<JSONObject>, keep: (JSONObject) -> Boolean): JSONObject {
        val keyed = rows.filter { it.optString("key").isNotEmpty() }
        val kept = keyed.filter(keep).mapTo(HashSet()) { it.optString("key") }
        return revision(keyed.map { it.optString("key") }.filter { it !in kept },
            choices?.records("set").orEmpty().filterNot { it.optString("key") in kept && it.has("amount") && !it.isNull("amount") && it.optLong("amount") == 0L })
    }

    /** Whether a row is marked to check: "Only the ones to check" keeps those. */
    fun toCheck(row: JSONObject): Boolean = row.optString("tone") == "warn"

    /**
     * A card's rows as choices that haven't reached the core yet leave them ([pending], the
     * revision on its way): rows left out or put back, an amount given, and a category given,
     * whose name ([category]; null is Ready to Assign) the row shows as `given` until the core's
     * card says it in its own words.
     */
    fun shown(rows: List<JSONObject>, pending: JSONObject?, category: (String?) -> String): List<JSONObject> {
        if (pending == null) return rows
        val skip = pending.strings("skip").toSet()
        val set = pending.records("set").associateBy { it.optString("key") }
        return rows.map { row ->
            val key = row.optString("key")
            val given = set[key]
            if (key.isEmpty() || (given == null && (key in skip) == row.optBoolean("skipped"))) return@map row
            JSONObject(row.toString()).put("skipped", key in skip).also { shown ->
                if (given?.has("amount") == true && !given.isNull("amount") && given.optLong("amount") != row.optLong("amount")) shown.put("amount", given.optLong("amount")).put("edited", true)
                if (given?.has("category") == true) {
                    val to = if (given.isNull("category")) null else given.optString("category")
                    val edit = shown.optJSONObject("edit")
                    val had = edit?.optString("categoryId")?.takeUnless { edit.isNull("categoryId") || it.isEmpty() }
                    if (to != had) { edit?.put("categoryId", to ?: JSONObject.NULL); shown.put("edited", true).put("given", category(to)) }
                }
            }
        }
    }

    private fun revision(skip: List<String>, set: List<JSONObject>) = JSONObject().put("skip", JSONArray(skip)).put("set", JSONArray(set))
}
