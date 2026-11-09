package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.smsSort
import app.urbudget.nativeapp.ui.smsKnown
import app.urbudget.nativeapp.ui.smsPreview
import android.content.Context
import app.urbudget.nativeapp.ui.row
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.smsBankIds
import app.urbudget.nativeapp.ui.text
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.withContext
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
 * sender's are ever read.
 */
internal object BankTextsAssistant {
    /** A reading a card asked for, for Bank messages to start: what's waiting, with a note. */
    data class Read(val note: String)
    val reads = MutableStateFlow<Read?>(null)
    /** Bumped when a card changed what Bank messages has stored (a card remembered), so an open page reads it again. */
    val changed = MutableStateFlow(0L)

    /** Texts searches found, by id, for a card to add to the review: the latest few hundred. */
    private val found = object : LinkedHashMap<String, BankSms>(256, .75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, BankSms>) = size > 400
    }
    private val stamp = DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm")
    private fun time(at: Long) = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()).format(stamp)
    private fun day(at: Long) = Instant.ofEpochMilli(at).atZone(ZoneId.systemDefault()).toLocalDate().toString()

    /** Whether this budget reads bank texts on this phone: senders chosen, the assistant allowed to read them, and SMS access on. */
    fun available(context: Context, identity: String?): Boolean = identity != null && SmsReader.canRead(context) &&
        SmsInbox.get(context).let { inbox -> inbox.sources(identity).isNotEmpty() && inbox.state(identity, "consent") != null }

    /** The answer to one request (`assistant://phone`): `output` and `step`, a card for a proposal, or `error`. */
    suspend fun answer(context: Context, vm: BudgetViewModel, request: JSONObject): JSONObject = withContext(Dispatchers.IO) {
        runCatching {
            val state = vm.state.value
            val budget = checkNotNull(state.budget) { Words[R.string.bank_texts_not_set_up] }
            val identity = state.budgetIdentity
            check(available(context, identity)) { Words[R.string.bank_texts_not_set_up] }
            val args = request.optJSONObject("args") ?: JSONObject()
            when (request.optString("tool")) {
                "get_bank_texts" -> status(context, vm, identity!!, budget, state.summary)
                "search_bank_texts" -> search(context, identity!!, budget, args)
                "propose_bank_texts" -> propose(context, identity!!, budget, args)
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

    private fun propose(context: Context, identity: String, budget: JSONObject, args: JSONObject): JSONObject {
        val inbox = SmsInbox.get(context)
        val currency = budget.text("currency")
        fun row(label: String, detail: String? = null, amount: Long? = null) = JSONObject().put("label", label).apply { detail?.let { put("detail", it) }; amount?.let { put("amount", it) } }
        val (title, rows, note, apply) = when (val action = args.optString("action")) {
            "read" -> {
                val waiting = inbox.pendingCount(identity)
                val open = inbox.state(identity, "draft") != null
                val written = args.optString("note").trim().take(2000)
                require(waiting > 0 || open) { Words[R.string.bank_texts_nothing_waiting] }
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
                    JSONObject().put("action", action).put("texts", JSONArray(texts.map { JSONObject().put("id", it.id).put("sender", it.sender).put("received", it.received).put("body", it.body) })))
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
            else -> error("Give an action: read, add, remember or limit.")
        }
        val card = JSONObject().put("kicker", Words[R.string.sms_title]).put("rows", JSONArray(rows)).put("note", note)
        val output = JSONObject().put("card", title).put("action", apply.optString("action")).put("rows", rows.size)
        return JSONObject().put("output", output).put("step", Words[R.string.bank_texts_step_card]).put("title", title).put("card", card).put("apply", apply)
    }
    private data class Proposed(val title: String, val rows: List<JSONObject>, val note: String, val apply: JSONObject)

    /**
     * Does what an applied card says. Returns the page to open: Bank messages, to read what's
     * waiting (with the texts the card added), or null when nothing more is to be done there.
     */
    suspend fun apply(context: Context, vm: BudgetViewModel, apply: JSONObject): String? {
        val identity = checkNotNull(vm.state.value.budgetIdentity) { Words[R.string.bank_texts_not_set_up] }
        val inbox = SmsInbox.get(context)
        return when (apply.optString("action")) {
            "read" -> { reads.value = Read(apply.optString("note")); "Bank messages" }
            "add" -> {
                val texts = apply.optJSONArray("texts").let { a -> (0 until (a?.length() ?: 0)).mapNotNull { a!!.optJSONObject(it) } }
                    .map { BankSms(it.getString("id"), it.getString("sender"), it.getLong("received"), it.getString("body")) }
                withContext(smsStorageLane) { inbox.bring(identity, texts) }
                reads.value = Read(""); "Bank messages"
            }
            "remember" -> {
                withContext(smsStorageLane) {
                    val kept = JSONArray(inbox.state(identity, "mappings") ?: "[]")
                    inbox.saveState(identity, "mappings", smsRememberCard(kept, apply.getString("sender"), apply.getString("ref"), apply.getString("accountId")).toString())
                }
                changed.value = changed.value + 1; null
            }
            "limit" -> { vm.setCreditLimit(apply.getString("accountId"), apply.getLong("limit")); null }
            else -> error("This card can't be applied here.")
        }
    }
}
