package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject

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

    private fun revision(skip: List<String>, set: List<JSONObject>) = JSONObject().put("skip", JSONArray(skip)).put("set", JSONArray(set))
    private fun JSONObject?.strings(key: String): List<String> =
        this?.optJSONArray(key)?.let { list -> (0 until list.length()).map(list::optString) }.orEmpty()
}
