package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.smsSort
import app.urbudget.nativeapp.ui.smsKnown
import app.urbudget.nativeapp.ui.smsPreview
import android.content.Context
import app.urbudget.nativeapp.ui.row
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.smsBankIds
import app.urbudget.nativeapp.ui.smsDecisions
import app.urbudget.nativeapp.ui.smsQuery
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import org.json.JSONArray
import org.json.JSONObject
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import kotlin.math.abs
import kotlin.math.roundToLong

/**
 * The assistant's bank-text tools, answered on the phone (services `assistant/phone.rs`): what's
 * waiting and the bank's balances, a search of the chosen senders' texts in Messages, and cards
 * the user applies here. Texts go to the assistant only when a search finds them; no other
 * sender's are ever read. A card can enter texts' transactions straight into the budget: they
 * go in as the review's do (the engine's `smsBatchApply`), and their texts are finished in Bank
 * messages, so its last text handled moves with them.
 */
internal object BankTextsAssistant {
    /** A reading a card asked for, for Bank messages to start: what's waiting, with a note. */
    data class Read(val note: String)
    val reads = MutableStateFlow<Read?>(null)
    /** Bumped when a card changed what Bank messages has stored (a card remembered), so an open page reads it again. */
    val changed = MutableStateFlow(0L)
    /** Bumped when a card entered texts into the budget and finished them here, so an open page takes its texts and review from storage again. */
    val entered = MutableStateFlow(0L)

    /** Texts searches found, by id, for a card to add to the review or enter: the latest few hundred. */
    private val found = object : LinkedHashMap<String, BankSms>(256, .75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, BankSms>) = size > 400
    }
    private val stamp = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm")
    private fun time(at: Long) = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()).format(stamp)
    private fun day(at: Long) = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()).toLocalDate().toString()

    /**
     * Whether chat may search this budget's bank texts on this phone, for the assistant as it's
     * connected now ([connection], its status): `bankTextsAllowed`. Storage that can't be read
     * counts as not allowed.
     */
    fun available(context: Context, identity: String?, connection: JSONObject?): Boolean = identity != null && runCatching {
        val inbox = SmsInbox.get(context)
        bankTextsAllowed(SmsReader.canRead(context), inbox.sources(identity).size, inbox.state(identity, "consent"), connection)
    }.getOrDefault(false)

    /** What applying a card did: the page to open, if any, and something to tell the user about it. */
    data class Applied(val page: String? = null, val notice: String? = null)

    /**
     * The answer to one request (`assistant://phone`): `output` and `step`, a card for a proposal,
     * or `error`. [connection] gives the assistant as it's connected now, and is asked for each
     * request: its service or model may have changed since the message was sent, and the
     * permission to send texts is for one of each.
     */
    suspend fun answer(context: Context, vm: BudgetViewModel, request: JSONObject, connection: suspend () -> JSONObject = { NativeSmsAnalyzer.status() }): JSONObject = withContext(Dispatchers.IO) {
        runCatching {
            val state = vm.state.value
            val budget = checkNotNull(state.budget) { Words[R.string.bank_texts_not_set_up] }
            val identity = state.budgetIdentity
            check(available(context, identity, connection())) { Words[R.string.bank_texts_not_set_up] }
            val args = request.optJSONObject("args") ?: JSONObject()
            when (request.optString("tool")) {
                "get_bank_texts" -> status(context, vm, identity!!, budget, state.summary)
                "search_bank_texts" -> search(context, identity!!, budget, args)
                "propose_bank_texts" -> propose(context, vm, identity!!, budget, args)
                else -> error("There's no bank-text tool named ${request.optString("tool")}.")
            }
        }.getOrElse { JSONObject().put("error", it.message ?: it.javaClass.simpleName) }
    }

    private suspend fun status(context: Context, vm: BudgetViewModel, identity: String, budget: JSONObject, summary: JSONObject?): JSONObject {
        val inbox = SmsInbox.get(context)
        val accounts = budget.rows("accounts")
        fun name(id: String) = accounts.firstOrNull { it.text("id") == id }?.text("name")
        val mappings = JSONArray(inbox.state(identity, "mappings") ?: "[]")
        val output = JSONObject()
            .put("senders", JSONArray(inbox.sources(identity).map { JSONObject().put("sender", it.sender).put("collectingFrom", day(it.since)) }))
            .put("readAsTheyArrive", inbox.state(identity, "auto") == "on")
        // The newest text finished with, from a review or a card that entered it.
        inbox.handledThrough(identity)?.let { output.put("lastTextHandled", time(it)) }
        // The review's texts are read and wait for the user's ticks; what waits besides them isn't read yet.
        val batch = runCatching { SmsBatch.restore(inbox.state(identity, "draft")) { ids -> inbox.waiting(identity, ids) } }.getOrNull()
        val inReview = batch?.messages?.mapTo(HashSet()) { it.id }.orEmpty()
        val unread = inbox.pending(identity).filter { it.id !in inReview }
        output.put("waitingToBeRead", JSONObject().put("count", unread.size).apply {
            if (unread.isNotEmpty()) { put("first", day(unread.minOf { it.received })); put("last", day(unread.maxOf { it.received })) }
        })
        // The open review as Bank messages shows it: checked against the budget, so what's already
        // there is matched, and a row that may already be there asks before Add.
        output.put("review", if (batch == null) JSONObject.NULL else runCatching {
            val items = batch.items(SmsAccounts.of(budget, mappings, batch.starts(budget)))
            val prepared = runCatching { vm.smsPreview(budget, batch, batch.resolved(items)) }.getOrNull()
            val known = smsKnown(prepared, items, budget)
            val sorted = smsSort(batch, items, known)
            val asks = prepared?.filterValues { it.initial == "review" }?.keys.orEmpty()
            JSONObject().put("texts", batch.messages.size).put("toReview", sorted.needs)
                .put("readyToAdd", sorted.ready.count { it.index !in asks })
                .put("mayAlreadyBeInTheBudget", sorted.ready.count { it.index in asks })
                .put("needAnAccountOrTheOtherSide", sorted.waiting.size).put("holds", sorted.holds.size)
                .put("alreadyInTheBudget", sorted.existing.size).put("notTransactions", sorted.skipped.size)
                .put("checked", prepared != null)
        }.getOrDefault(JSONObject.NULL))
        inbox.state(identity, "last")?.let { runCatching { JSONObject(it) }.getOrNull() }?.let { last ->
            output.put("lastReview", JSONObject().put("matchedWithoutAQuestion", last.optInt("known")).put("skipped", last.optJSONObject("skipped") ?: JSONObject())
                .apply { if (last.has("at")) put("on", day(last.getLong("at"))) })
        }
        output.put("cards", JSONArray((0 until mappings.length()).mapNotNull { mappings.optJSONObject(it) }.filter { it.text("for").isEmpty() }.map { m ->
            JSONObject().put("sender", m.text("sender")).put("digits", m.text("reference"))
                .put("account", if (m.text("accountId") == SmsSkipCard) "not added" else name(m.text("accountId")) ?: "a removed account")
        }))
        output.put("learnedFormats", inbox.templateSummary(identity).count)
        // The bank's latest figure for each account against the budget at the end of that day.
        val balances = runCatching { JSONObject(inbox.state(identity, "balances") ?: "{}") }.getOrDefault(JSONObject())
        val now = summary?.rows("accounts")?.associate { it.text("id") to it.optLong("balance") }.orEmpty()
        output.put("bankBalances", JSONArray(balances.keys().asSequence().mapNotNull { id ->
            val saved = balances.optJSONObject(id) ?: return@mapNotNull null
            val account = budget.row("accounts", id) ?: return@mapNotNull null
            val kind = saved.optString("kind")
            val row = JSONObject().put("account", account.text("name")).put("accountId", id).put("at", time(saved.optLong("received")))
            smsCreditLimit(account)?.let { row.put("creditLimit", smsDecimal(it)) }
            if (kind == "available") row.put("creditLeft", smsDecimal(saved.optLong("cents")))
            val figure = smsBankFigure(account, kind, saved.optLong("cents"))
            if (figure == null) row.put("note", if (account.text("kind") == "credit") "The card has no credit limit, so what's owed can't be told from its credit left." else "Not comparable.")
            else {
                val date = saved.optString("date").ifBlank { day(saved.optLong("received")) }
                val atDay = smsBudgetBalanceAt(budget, now[id] ?: 0L, id, date)
                row.put("bank", smsDecimal(figure)).put("budgetAtEndOfThatDay", smsDecimal(atDay)).put("difference", smsDecimal(figure - atDay))
            }
            row
        }.toList()))
        return JSONObject().put("output", output).put("step", Words[R.string.bank_texts_step_status])
    }

    private fun search(context: Context, identity: String, budget: JSONObject, args: JSONObject): JSONObject {
        val inbox = SmsInbox.get(context)
        val zone = ZoneId.systemDefault()
        val to = args.optString("to").ifBlank { null }?.let(LocalDate::parse) ?: LocalDate.now()
        val from = args.optString("from").ifBlank { null }?.let(LocalDate::parse) ?: to.minusDays(30)
        require(!from.isAfter(to) && ChronoUnit.DAYS.between(from, to) <= 366) { Words[R.string.bank_texts_range] }
        val sender = args.optString("sender").ifBlank { null }
        require(sender == null || inbox.sources(identity).any { smsSenderKey(it.sender) == smsSenderKey(sender) }) { Words[R.string.bank_texts_unknown_sender] }
        val amount = if (args.has("amount") && !args.isNull("amount")) (abs(args.getDouble("amount")) * 100).roundToLong() else null
        val texts = SmsReader.search(context, identity, from.atStartOfDay(zone).toInstant().toEpochMilli(), to.plusDays(1).atStartOfDay(zone).toInstant().toEpochMilli(), sender)
            .filter { smsTextMatches(it.body, args.optString("words"), amount) }
        val shown = texts.take(20)
        synchronized(found) { shown.forEach { found[it.id] = it } }
        val records = inbox.records(identity, shown.map { it.id })
        val imported = budget.optJSONArray("smsImportedMessageIds").let { ids -> (0 until (ids?.length() ?: 0)).mapTo(HashSet()) { ids!!.getString(it) } }
        val transactions = budget.rows("transactions")
        val accounts = budget.rows("accounts").map { it.text("id") }
        fun transaction(id: String, record: SmsRecord?): JSONObject? {
            val wanted = record?.outcome?.optString("transactionId")?.ifBlank { null }
            val hashes = if (wanted == null) accounts.flatMap { smsBankIds(it, listOf(id)) }.toSet() else emptySet()
            val t = transactions.firstOrNull { t ->
                if (wanted != null) t.text("id") == wanted
                else t.optJSONArray("bankImportIds")?.let { a -> (0 until a.length()).any { a.optString(it) in hashes } } == true
            } ?: return null
            return JSONObject().put("id", t.text("id")).put("date", t.text("date")).put("payee", t.text("payee")).put("amount", smsDecimal(t.optLong("amount")))
                .put("account", budget.row("accounts", t.text("accountId"))?.text("name").orEmpty())
        }
        val rows = shown.map { text ->
            val record = records[text.id]
            val outcome = when {
                record == null -> if (text.id in imported) "added or matched" else "never collected"
                !record.done -> "waiting"
                record.outcome != null -> record.outcome.optString("outcome")
                text.id in imported -> "added or matched"
                else -> "finished before outcomes were kept"
            }
            JSONObject().put("id", text.id).put("sender", text.sender).put("received", time(text.received)).put("text", text.body.take(1200)).put("outcome", outcome).apply {
                record?.outcome?.optString("reason")?.ifBlank { null }?.let { put("reason", it) }
                transaction(text.id, record)?.let { put("transaction", it) }
            }
        }
        val output = JSONObject().put("from", from.toString()).put("to", to.toString()).put("texts", JSONArray(rows)).put("more", texts.size - shown.size)
        return JSONObject().put("output", output).put("step", Words.count(R.plurals.bank_texts_step_found, texts.size.toLong()))
    }

    private suspend fun propose(context: Context, vm: BudgetViewModel, identity: String, budget: JSONObject, args: JSONObject): JSONObject {
        val inbox = SmsInbox.get(context)
        val currency = budget.text("currency")
        fun row(label: String, detail: String? = null, amount: Long? = null) = JSONObject().put("label", label).apply { detail?.let { put("detail", it) }; amount?.let { put("amount", it) } }
        val (title, rows, note, apply, said) = when (val action = args.optString("action")) {
            "read" -> {
                // What Bank messages will read: the texts waiting outside the open review, whose own are read already.
                val batch = runCatching { SmsBatch.restore(inbox.state(identity, "draft")) { ids -> inbox.waiting(identity, ids) } }.getOrNull()
                val waiting = bankTextsToRead(inbox.pending(identity).map { it.id }, batch?.messages?.map { it.id }.orEmpty())
                val open = batch != null
                val written = args.optString("note").trim().take(2000)
                require(waiting > 0 || open) { Words[R.string.bank_texts_nothing_waiting] }
                // The open review is read again only with a note to read it with: without one the card would do nothing.
                require(waiting > 0 || written.isNotEmpty()) { Words[R.string.bank_texts_nothing_new] }
                Proposed(Words[R.string.bank_texts_title_read],
                    listOfNotNull(row(if (waiting > 0) Words.count(R.plurals.bank_texts_card_read, waiting.toLong()) else Words[R.string.bank_texts_card_read_again]),
                        written.ifBlank { null }?.let { row(Words[R.string.bank_texts_card_note], it) }),
                    Words[R.string.bank_texts_card_read_note], JSONObject().put("action", action).put("note", written))
            }
            "add" -> {
                val ids = args.optJSONArray("textIds").let { a -> (0 until (a?.length() ?: 0)).map { a!!.getString(it) } }.distinct()
                require(ids.isNotEmpty() && ids.size <= 50) { Words[R.string.bank_texts_search_first] }
                val texts = synchronized(found) { ids.map { found[it] ?: error(Words[R.string.bank_texts_search_first]) } }
                val keys = inbox.sources(identity).map { smsSenderKey(it.sender) }.toSet()
                require(texts.all { smsSenderKey(it.sender) in keys }) { Words[R.string.bank_texts_unknown_sender] }
                Proposed(Words.count(R.plurals.bank_texts_card_add, texts.size.toLong()),
                    texts.map { row(Money.joined(listOf(Money.date(it.receivedDate), Money.own(it.sender))), it.body.replace(Regex("\\s+"), " ").take(90)) },
                    Words[R.string.bank_texts_card_add_note],
                    // The card is kept with the conversation, so it names each text (enough to find it
                    // on the phone again) without its words: those are read when the card is applied.
                    JSONObject().put("action", action).put("texts", JSONArray(texts.map { JSONObject().put("id", it.id).put("sender", it.sender).put("received", it.received) })))
            }
            "enter" -> {
                val asked = args.rows("transactions").map(BankTextEntry::asked)
                val texts = synchronized(found) { asked.flatMap { it.textIds }.distinct().associateWith { found[it] ?: error(Words[R.string.bank_texts_search_first]) } }
                val keys = inbox.sources(identity).map { smsSenderKey(it.sender) }.toSet()
                require(texts.values.all { smsSenderKey(it.sender) in keys }) { Words[R.string.bank_texts_unknown_sender] }
                bankTextEntriesProblem(asked, texts, budget)?.let { error(it) }
                // Checked by the engine as a review's reading is, and against what the budget already has.
                val checked = vm.smsQuery("smsBatchPreview", bankTextsReading(asked, texts, currency).params()).rows("items")
                    .associate { it.getInt("index") to it.getJSONObject("row") }
                val decided = bankTextsDecide(asked, checked, budget)
                require(decided.entries.isNotEmpty()) { "Those texts are already in the budget. Nothing to enter." }
                val names = budget.rows("accounts").associate { it.text("id") to it.text("name") }
                Proposed(Words.count(R.plurals.bank_texts_card_enter, decided.entries.size.toLong()),
                    decided.entries.map { e ->
                        row(e.payee, Money.joined(listOfNotNull(Money.date(e.date), names[e.accountId], e.match?.let { Words[R.string.bank_texts_card_enter_match] })), e.cents)
                    },
                    Words[R.string.bank_texts_card_enter_note],
                    // As an "add" card: each text by what finds it again, without its words.
                    JSONObject().put("action", action).put("entries", JSONArray(decided.entries.map { it.card() }))
                        .put("texts", JSONArray(decided.entries.flatMap { it.textIds }.map { texts.getValue(it) }.map { JSONObject().put("id", it.id).put("sender", it.sender).put("received", it.received) })),
                    JSONObject().put("transactions", JSONArray(decided.entries.map { e ->
                        JSONObject().put("textIds", JSONArray(e.textIds)).put("does", if (e.match == null) "add" else "match").apply { e.match?.let { put("matches", it) } }
                    })).put("alreadyInTheBudget", JSONArray(decided.already)))
            }
            "remember" -> {
                val sender = args.optString("sender").trim()
                val ref = args.optString("ref").trim()
                val account = budget.row("accounts", args.optString("accountId"))?.takeUnless { it.optBoolean("archived") }
                require(inbox.sources(identity).any { smsSenderKey(it.sender) == smsSenderKey(sender) }) { Words[R.string.bank_texts_unknown_sender] }
                require(ref.isNotEmpty() && ref.length <= 80) { Words[R.string.bank_texts_digits_needed] }
                requireNotNull(account) { Words[R.string.bank_texts_unknown_account] }
                Proposed(Words[R.string.bank_texts_title_remember], listOf(row(Words[R.string.bank_texts_card_remember, ref, account.text("name")], Money.own(sender))),
                    Words[R.string.bank_texts_card_remember_note],
                    JSONObject().put("action", action).put("sender", sender).put("ref", ref).put("accountId", account.text("id")))
            }
            "limit" -> {
                val account = requireNotNull(budget.row("accounts", args.optString("accountId"))) { Words[R.string.bank_texts_unknown_account] }
                require(account.text("kind") == "credit") { Words[R.string.bank_texts_not_card] }
                val limit = if (args.has("limit") && !args.isNull("limit")) (args.getDouble("limit") * 100).roundToLong() else 0L
                require(limit > 0) { Words[R.string.bank_texts_limit_positive] }
                val now = smsCreditLimit(account)
                Proposed(Words[R.string.bank_texts_title_limit],
                    listOf(row(Words[R.string.bank_texts_card_limit, account.text("name")],
                        now?.let { Words[R.string.bank_texts_card_limit_now, Money.format(it, currency)] } ?: Words[R.string.bank_texts_card_limit_none], limit)),
                    Words[R.string.bank_texts_card_limit_note], JSONObject().put("action", action).put("accountId", account.text("id")).put("limit", limit))
            }
            else -> error("Give an action: read, add, enter, remember or limit.")
        }
        val card = JSONObject().put("kicker", Words[R.string.sms_title]).put("rows", JSONArray(rows)).put("note", note)
        val output = JSONObject().put("card", title).put("action", apply.optString("action")).put("rows", rows.size)
        said?.let { more -> more.keys().forEach { output.put(it, more.get(it)) } }
        return JSONObject().put("output", output).put("step", Words[R.string.bank_texts_step_card]).put("title", title).put("card", card).put("apply", apply)
    }
    /** A card: its title, rows and note, what applying it does, and what more the assistant is told about it ([said]). */
    private data class Proposed(val title: String, val rows: List<JSONObject>, val note: String, val apply: JSONObject, val said: JSONObject? = null)

    /** Runs a budget command and waits for it: one that fails is the error, said as the app says it. */
    private suspend fun command(vm: BudgetViewModel, method: String, params: JSONObject) = withContext(Dispatchers.Main) {
        suspendCancellableCoroutine { waiting ->
            val failures = vm.operationFailureEpoch
            var saved = false
            vm.runCommand(method, params) { saved = true }
            vm.afterPending {
                if (!waiting.isActive) return@afterPending
                if (saved && vm.operationFailureEpoch == failures) waiting.resume(Unit)
                else waiting.resumeWithException(IllegalStateException(vm.state.value.error ?: Words[R.string.model_action_not_saved]))
            }
        }
    }

    /**
     * Does what an applied card says. The page it gives to open is Bank messages, to read what's
     * waiting (with the texts the card added); none when nothing more is to be done there. Texts
     * a card named that are no longer on the phone are left out and counted in its notice; with
     * none of them left, nothing is done and that's the error. A card is checked again as it's
     * applied: the account it names may have gone since it was made, and then nothing is done
     * and that's the error too, so the card never says Applied for it.
     */
    suspend fun apply(context: Context, vm: BudgetViewModel, apply: JSONObject): Applied {
        val identity = checkNotNull(vm.state.value.budgetIdentity) { Words[R.string.bank_texts_not_set_up] }
        val inbox = SmsInbox.get(context)
        bankTextsCardProblem(apply, vm.state.value.budget)?.let { error(Words[it]) }
        return when (apply.optString("action")) {
            "read" -> { reads.value = Read(apply.optString("note")); Applied("Bank messages") }
            "add" -> {
                val (texts, missing) = withContext(Dispatchers.IO) {
                    bankTextsOfCard(apply, { id -> synchronized(found) { found[id] } }) { sender, from, until -> SmsReader.search(context, identity, from, until, sender) }
                }
                val gone = if (missing > 0) Words.count(R.plurals.bank_texts_gone, missing.toLong()) else null
                if (gone != null && texts.isEmpty()) error(gone)
                withContext(smsStorageLane) { inbox.bring(identity, texts) }
                reads.value = Read(""); Applied("Bank messages", gone)
            }
            "enter" -> {
                // A reading under way saves its review when it ends, with these texts still in it.
                check(SmsReadings.state.value == null) { Words[R.string.bank_texts_reading_now] }
                val budget = checkNotNull(vm.state.value.budget) { Words[R.string.bank_texts_not_set_up] }
                val (onPhone, missing) = withContext(Dispatchers.IO) {
                    bankTextsOfCard(apply, { id -> synchronized(found) { found[id] } }) { sender, from, until -> SmsReader.search(context, identity, from, until, sender) }
                }
                val texts = onPhone.associateBy { it.id }
                // A transaction whose text is gone from the phone can't be checked against it, and isn't entered.
                val entries = apply.rows("entries").map(BankTextEntry::carded).filter { e -> e.textIds.all { it in texts } }
                val gone = if (missing > 0) Words.count(R.plurals.bank_texts_gone, missing.toLong()) else null
                if (entries.isEmpty()) error(gone ?: Words[R.string.bank_texts_search_first])
                val reading = bankTextsReading(entries, texts, budget.text("currency"))
                // What the budget took in since the card was made isn't entered again; its text is finished all the same.
                val there = vm.smsQuery("smsBatchPreview", reading.params()).rows("items").filter { it.getJSONObject("row").optBoolean("duplicate") }.mapTo(HashSet()) { it.getInt("index") }
                val decisions = entries.indices.filter { it !in there }.map { it to (entries[it].match?.let { id -> "match:$id" } ?: "new") }
                if (decisions.isNotEmpty()) command(vm, "smsBatchApply", reading.params().put("decisions", smsDecisions(decisions)))
                val finished = entries.flatMap { it.textIds }.toSet()
                withContext(smsStorageLane) {
                    // The open review loses what it read from these texts: they're in the budget now.
                    val open = SmsBatch.restore(inbox.state(identity, "draft")) { ids -> inbox.waiting(identity, ids) }
                    inbox.enter(identity, finished.map(texts::getValue), bankTextOutcomes(entries, texts, decisions.toMap()))
                    if (open != null && open.messages.any { it.id in finished }) inbox.saveState(identity, "draft", open.without(finished)?.saved())
                }
                entered.value = entered.value + 1; Applied(notice = gone)
            }
            "remember" -> {
                withContext(smsStorageLane) {
                    val kept = JSONArray(inbox.state(identity, "mappings") ?: "[]")
                    inbox.saveState(identity, "mappings", smsRememberCard(kept, apply.getString("sender"), apply.getString("ref"), apply.getString("accountId")).toString())
                }
                changed.value = changed.value + 1; Applied()
            }
            "limit" -> { vm.setCreditLimit(apply.getString("accountId"), apply.getLong("limit")); Applied() }
            else -> error("This card can't be applied here.")
        }
    }
}

/**
 * Whether chat may search a budget's bank texts: SMS access on, senders chosen, and sending
 * allowed for the assistant as it's connected now ([connection], its status; none when it isn't
 * known). That's the permission Bank messages itself reads with (`smsConsented`), so texts never
 * go to a service or model the user hasn't allowed there.
 */
internal fun bankTextsAllowed(canRead: Boolean, senders: Int, consent: String?, connection: JSONObject?): Boolean =
    canRead && senders > 0 && connection != null && smsConsented(consent, connection)

/**
 * How many texts a "read" card will have Bank messages read: those [waiting] that aren't in the
 * open review ([reviewed]), whose texts are read already and wait only for the user's ticks.
 */
internal fun bankTextsToRead(waiting: List<String>, reviewed: List<String>): Int {
    val inReview = reviewed.toSet()
    return waiting.count { it !in inReview }
}

/**
 * Why a card can't be applied to [budget] as it is now, as the words to say it with; none when
 * it can. A card that remembers digits needs its account still open, and one that sets a credit
 * limit needs its account still there and still a card: either may have gone since the card
 * was made. One that enters transactions needs every account they name still open.
 */
internal fun bankTextsCardProblem(apply: JSONObject, budget: JSONObject?): Int? {
    val account = budget?.row("accounts", apply.optString("accountId"))
    fun open(id: String) = budget?.row("accounts", id)?.optBoolean("archived") == false
    return when (apply.optString("action")) {
        "enter" -> R.string.bank_texts_unknown_account.takeIf {
            apply.rows("entries").map(BankTextEntry::carded).any { e -> !open(e.accountId) || e.transferAccountId?.let(::open) == false }
        }
        "remember" -> R.string.bank_texts_unknown_account.takeIf { account == null || account.optBoolean("archived") }
        "limit" -> when {
            account == null -> R.string.bank_texts_unknown_account
            account.text("kind") != "credit" -> R.string.bank_texts_not_card
            else -> null
        }
        else -> null
    }
}

/** How far either side of a text's time its sender's texts are read to find it again; an MMS is timed to the second. */
private const val CardTextMargin = 60_000L

/**
 * The texts an "add" card names, with their words. A card keeps only what finds a text again
 * (its id, sender and time), so the words come from what a search found ([cached], by id), or
 * else from the phone: [read] gives a sender's texts from a time until another, once for each
 * sender, and the id tells which is meant. A card made before that carries the words itself.
 * Returns the texts found, and how many are nowhere now.
 */
internal fun bankTextsOfCard(apply: JSONObject, cached: (String) -> BankSms?, read: (sender: String, from: Long, until: Long) -> List<BankSms>): Pair<List<BankSms>, Int> {
    val wanted = apply.rows("texts")
    val texts = wanted.map { named ->
        named.text("body").ifEmpty { null }?.let { BankSms(named.getString("id"), named.getString("sender"), named.getLong("received"), it) } ?: cached(named.getString("id"))
    }.toMutableList()
    wanted.indices.filter { texts[it] == null }.groupBy { smsSenderKey(wanted[it].getString("sender")) }.values.forEach { group ->
        val onPhone = read(wanted[group.first()].getString("sender"), group.minOf { wanted[it].getLong("received") } - CardTextMargin,
            group.maxOf { wanted[it].getLong("received") } + CardTextMargin).associateBy { it.id }
        group.forEach { texts[it] = onPhone[wanted[it].getString("id")] }
    }
    val found = texts.filterNotNull()
    return found to wanted.size - found.size
}

/**
 * One transaction a card enters from texts: the texts it's read from, and what the assistant read
 * of them. [cents] is signed as the budget writes amounts. [match] is the transaction already in
 * the budget it is, and none to add it, once that's settled ([bankTextsDecide]); before then it's
 * what the assistant said: a transaction's id, "new", or nothing.
 */
internal data class BankTextEntry(val textIds: List<String>, val accountId: String, val date: String, val cents: Long, val payee: String, val kind: String,
                                  val categoryId: String? = null, val transferAccountId: String? = null, val memo: String = "", val match: String? = null) {
    /** As a card keeps it. */
    fun card(): JSONObject = JSONObject().put("textIds", JSONArray(textIds)).put("accountId", accountId).put("date", date).put("cents", cents).put("payee", payee)
        .put("kind", kind).put("memo", memo).apply {
            categoryId?.let { put("categoryId", it) }; transferAccountId?.let { put("transferAccountId", it) }; match?.let { put("match", it) }
        }

    companion object {
        private fun ids(json: JSONObject) = json.optJSONArray("textIds").let { a -> (0 until (a?.length() ?: 0)).map { a!!.getString(it) } }
        private fun optional(json: JSONObject, name: String) = if (json.isNull(name)) null else json.optString(name).trim().ifEmpty { null }
        private fun words(json: JSONObject, name: String) = if (json.isNull(name)) "" else json.optString(name).trim()

        /** As the assistant asks for it: its amount a number, signed by its kind whichever way it was written (a transfer keeps its sign). */
        fun asked(json: JSONObject): BankTextEntry {
            val kind = words(json, "kind")
            val written = if (json.isNull("amount")) 0L else (json.getDouble("amount") * 100).roundToLong()
            val cents = when (kind) { "expense" -> -abs(written); "income", "refund" -> abs(written); else -> written }
            return BankTextEntry(ids(json), words(json, "accountId"), words(json, "date"), cents, words(json, "payee"), kind,
                optional(json, "categoryId"), optional(json, "transferAccountId"), words(json, "memo"), optional(json, "match"))
        }

        /** As a card kept it. */
        fun carded(json: JSONObject): BankTextEntry = BankTextEntry(ids(json), words(json, "accountId"), words(json, "date"), json.optLong("cents"), words(json, "payee"),
            words(json, "kind"), optional(json, "categoryId"), optional(json, "transferAccountId"), words(json, "memo"), optional(json, "match"))
    }
}

/**
 * Why these can't be entered, said to the assistant; none when they can go to the engine's check.
 * Each has its texts (found by a search: [texts], by id), an open account, and an amount one of its
 * texts writes: a number the text doesn't give was made up, by a reading gone wrong or a text
 * written to mislead it.
 */
internal fun bankTextEntriesProblem(entries: List<BankTextEntry>, texts: Map<String, BankSms>, budget: JSONObject): String? {
    if (entries.isEmpty() || entries.size > 20) return "Give 1 to 20 transactions."
    val all = entries.flatMap { it.textIds }
    if (entries.any { it.textIds.isEmpty() || it.textIds.size > 5 } || all.size != all.toSet().size) return "Give each transaction its text's id from search_bank_texts, and each text once."
    fun open(id: String) = budget.row("accounts", id)?.optBoolean("archived") == false
    entries.forEach { e ->
        val named = "\"${e.payee}\""
        when {
            e.payee.isBlank() || e.payee.length > 500 || e.memo.length > 2000 -> return "Give every transaction a payee."
            e.kind !in setOf("expense", "income", "refund", "transfer") -> return "$named: kind is expense, income, refund or transfer."
            runCatching { LocalDate.parse(e.date) }.isFailure -> return "$named: write the date as YYYY-MM-DD."
            !open(e.accountId) -> return "$named: accountId isn't an open account of this budget."
            e.kind == "transfer" && (e.transferAccountId == null || e.transferAccountId == e.accountId || !open(e.transferAccountId)) ->
                return "$named: a transfer needs transferAccountId, another open account."
            e.kind != "transfer" && e.transferAccountId != null -> return "$named: only a transfer has transferAccountId."
            e.cents == 0L || e.textIds.none { abs(e.cents) in smsWrittenAmounts(texts[it]?.body.orEmpty()) } ->
                return "$named: the amount isn't a number its text gives. Copy it from the text. For a purchase in another currency that the text doesn't convert, add the text to the review instead (action add)."
        }
    }
    return null
}

/** Words of a text that say it's the one meant: its first line, short, copied exactly (the engine checks that a reading quotes its text). */
internal fun bankTextQuote(body: String): String {
    val line = body.lineSequence().map { it.trim() }.firstOrNull { it.isNotEmpty() }.orEmpty().take(80)
    return if (line.lastOrNull()?.isHighSurrogate() == true) line.dropLast(1) else line
}

/**
 * The entries as a review's reading of their texts, for the engine to check and import
 * (`smsBatchPreview`, `smsBatchApply`): each a transaction with its accounts chosen, in the
 * budget's [currency], backed by a quote of its first text.
 */
internal fun bankTextsReading(entries: List<BankTextEntry>, texts: Map<String, BankSms>, currency: String): SmsBatch {
    val items = entries.map { e ->
        val first = texts.getValue(e.textIds.first())
        JSONObject().put("sourceIds", JSONArray(e.textIds)).put("disposition", "transaction").put("skip", JSONObject.NULL).put("note", e.payee)
            .put("evidence", JSONArray().put(JSONObject().put("sourceId", first.id).put("quote", bankTextQuote(first.body))))
            .put("transaction", JSONObject().put("date", e.date).put("amount", smsDecimal(e.cents)).put("currency", currency).put("writtenAmount", smsDecimal(abs(e.cents)))
                .put("payee", e.payee).put("memo", e.memo).put("kind", e.kind).put("accountRef", JSONObject.NULL).put("otherRef", JSONObject.NULL)
                .put("categoryId", e.categoryId ?: JSONObject.NULL).put("accountId", e.accountId).put("transferAccountId", e.transferAccountId ?: JSONObject.NULL))
            .put("unsure", JSONArray())
    }
    return SmsBatch(entries.flatMap { it.textIds }.map(texts::getValue), JSONObject().put("items", JSONArray(items)))
}

/** What a card enters once it's checked against the budget: [entries] with their match settled, and the texts the budget [already] has. */
internal data class BankTextsDecided(val entries: List<BankTextEntry>, val already: List<String>)

/**
 * Settles each entry against the engine's review row for it ([rows], by its place): a text the
 * budget already took in is left out; with nothing it could be, it's added; the one transaction
 * it could be, dated the same day, is matched, as the review matches it without a question.
 * Anything else it could be is the assistant's to say ([BankTextEntry.match]: a transaction's id,
 * or "new"), and the error asks for it, naming them.
 */
internal fun bankTextsDecide(entries: List<BankTextEntry>, rows: Map<Int, JSONObject>, budget: JSONObject): BankTextsDecided {
    val saved = budget.rows("transactions").associateBy { it.text("id") }
    val names = budget.rows("accounts").associate { it.text("id") to it.text("name") }
    fun list(row: JSONObject, name: String) = row.optJSONArray(name).let { a -> (0 until (a?.length() ?: 0)).map { a!!.getString(it) } }
    val already = mutableListOf<String>()
    val settled = entries.mapIndexedNotNull { index, e ->
        val row = requireNotNull(rows[index]) { "\"${e.payee}\" couldn't be checked against the budget." }
        if (row.optBoolean("duplicate")) { already += e.textIds; return@mapIndexedNotNull null }
        val candidates = list(row, "candidates")
        // A split's part is offered by its own id, and is the transaction it's part of.
        val whole = row.rows("candidateDetails").associate { it.text("id") to it.text("transactionId") }
        fun there(id: String) = saved[whole[id] ?: id]
        fun said(id: String) = there(id)?.let { t -> "$id (${t.text("date")}, ${t.text("payee")}, ${smsDecimal(t.optLong("amount"))}, ${names[t.text("accountId")].orEmpty()})" } ?: id
        val could = candidates.joinToString("; ", transform = ::said)
        val match = when (e.match) {
            "new" -> null
            null -> when {
                candidates.isEmpty() -> null
                candidates.size == 1 && candidates[0] !in list(row, "previouslyMatchedCandidates") && there(candidates[0])?.text("date") == e.date -> candidates[0]
                else -> throw IllegalArgumentException("\"${e.payee}\" may already be in the budget as: $could. Give match with that transaction's id, or \"new\" to add it as well.")
            }
            else -> candidates.firstOrNull { it == e.match || whole[it] == e.match }
                ?: throw IllegalArgumentException("\"${e.payee}\" can't be matched to ${e.match}." + if (candidates.isEmpty()) " Nothing in the budget could be it: leave match out." else " It could be: $could.")
        }
        e.copy(match = match)
    }
    return BankTextsDecided(settled, already)
}

/**
 * What became of each text a card entered, kept with it as a review's is ([SmsOutcome]): added,
 * or matched to a transaction ([decisions], by entry: "new" or "match:id"); one with no decision
 * was in the budget already, and is matched to what it's there as.
 */
internal fun bankTextOutcomes(entries: List<BankTextEntry>, texts: Map<String, BankSms>, decisions: Map<Int, String>, at: Long = System.currentTimeMillis()): Map<String, JSONObject> =
    entries.flatMapIndexed { index, e ->
        val decision = decisions[index]
        val outcome = JSONObject().put("outcome", if (decision == "new") SmsOutcome.Added else SmsOutcome.Matched).put("at", at)
            .put("received", e.textIds.maxOf { texts.getValue(it).received }).put("date", e.date).put("payee", e.payee).put("accountId", e.accountId).put("amount", e.cents)
        decision?.takeIf { it.startsWith("match:") }?.let { outcome.put("transactionId", it.removePrefix("match:")) }
        e.textIds.map { it to outcome }
    }.toMap()
