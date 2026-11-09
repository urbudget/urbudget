package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.text
import org.json.JSONObject
import java.math.BigDecimal
import java.util.Locale

/**
 * A card or account the review's texts name that the budget doesn't know: the texts about money on
 * it ([items]) and the transfers that name it as their other side ([via]). [ref] is null for a
 * sender whose texts name no card (a mobile wallet), when the budget has nowhere to put them.
 * [refs] are all its numbers: several for an account the bank numbers afresh for each transfer
 * (a term deposit), [ref] the first.
 */
internal data class SmsSetupCandidate(val key: String, val sender: String, val ref: String?, val items: List<SmsItem>, val via: List<SmsItem>,
                                      val refs: List<String> = listOfNotNull(ref)) {
    val texts: Int get() = (items + via).distinctBy { it.index }.size
}

/** A text with its numbers taken out: a bank's texts of one kind share it, whatever their amounts, dates and numbers. */
internal fun smsShape(body: String): String = body.map { c -> if (Character.digit(c, 10) >= 0) '#' else c }.joinToString("")
    .replace(Regex("#+(?:[.,٫٬:/-]#+)*"), "#").replace(Regex("\\s+"), " ").trim()

/** What a sender's texts that name no card are remembered by, as a card's digits are. */
internal const val SmsAnyCard = "*"

/**
 * Every card and account the review needs set up: those its texts are on that no account matches,
 * and those transfers name as their other side that none does, each once for its sender and digits.
 * Numbers only transfers name are one account when the words by them name the same account but
 * for the first ("to your deposit account no." and "from your deposit account no."), or, with too
 * few words to tell, their texts are of one shape: a bank that numbers each placement into a term
 * deposit afresh would otherwise make an account of every placement. The most texts first.
 */
internal fun smsSetupCandidates(batch: SmsBatch, items: List<SmsItem>): List<SmsSetupCandidate> {
    val live = items.filter { it.transaction != null && it.disposition in setOf("transaction", "pending") && it.index !in batch.dismissed }
    val own = live.filter { it.needs == "account" }.groupBy { smsCardKey(it.sender, it.ref ?: SmsAnyCard) }
    val via = live.filter { item -> item.transaction!!.let { it.text("kind") == "transfer" && it.text("transferAccountId").isBlank() && it.text("otherRef").isNotBlank() } }
        .groupBy { smsCardKey(it.sender, it.transaction!!.text("otherRef")) }
    val found = (own.keys + via.keys).map { key ->
        val mine = own[key]
        val first = (mine ?: via.getValue(key)).first()
        SmsSetupCandidate(key, first.sender, if (mine != null) first.ref else first.transaction!!.text("otherRef"), mine.orEmpty(), via[key].orEmpty())
    }
    val (lone, rest) = found.partition { it.items.isEmpty() && it.ref != null }
    fun alike(c: SmsSetupCandidate): String {
        val body = c.via.first().sources.first().body
        val words = smsWordsBy(body, c.ref!!).first.trim().split(Regex("\\s+")).drop(1).filter { w -> w.any(Char::isLetter) }
        return smsSenderKey(c.sender) + "\u0000" + if (words.size >= 2) "words:" + words.joinToString(" ") else "shape:" + smsShape(body)
    }
    val grouped = lone.groupBy(::alike).values.map { group ->
        if (group.size < 2) group.single() else {
            val refs = group.mapNotNull { it.ref }.sortedBy(::smsRefKey)
            SmsSetupCandidate("\u0001" + smsCardKey(group.first().sender, refs.first()), group.first().sender, refs.first(), emptyList(),
                group.flatMap { it.via }.distinctBy { it.index }, refs)
        }
    }
    return (rest + grouped).sortedWith(compareByDescending<SmsSetupCandidate> { it.texts }.thenBy { it.key })
}

/** Words in any of the languages the reading knows; Latin ones match whole words ("mada" isn't in "Ramadan"). */
private class SmsCue(vararg words: String) {
    private val latin = words.filter { w -> w.all { it.code < 0x250 } }
    private val pattern = Regex("(?<!\\p{L})(?:" + latin.joinToString("|") { Regex.escape(it) } + ")(?!\\p{L})")
    private val other = words.filterNot { it in latin }
    fun at(text: String) = (latin.isNotEmpty() && pattern.containsMatchIn(text)) || other.any { it in text }
}

private val CreditCue = SmsCue("credit card", "credit crd", "cr card", "cr. card", "creditcard", "tarjeta de crédito", "tarjeta de credito", "carte de crédit", "carte de credit",
    "cartão de crédito", "cartao de credito", "carta di credito", "kreditkarte", "kartu kredit", "kredi kartı", "ائتمان", "クレジット", "信用卡")
private val SavingsCue = SmsCue("savings", "saving a/c", "sav a/c", "term deposit", "fixed deposit", "time deposit", "deposit account", "investment", "murabaha", "tagesgeld",
    "sparkonto", "festgeld", "épargne", "epargne", "livret", "ahorro", "ahorros", "inversión", "poupança", "poupanca", "investimento", "tabungan", "deposito", "ادخار", "توفير",
    "وديعة", "استثمار", "مرابحة", "储蓄", "定期")
/** The user's own: "your", in the reading's languages, by an account's digits. */
private val OwnCue = SmsCue("your", "yours", "votre", "vos", "tu", "su", "ihr", "ihre", "ihrem", "dein", "deine", "seu", "sua", "anda", "حسابك", "لحسابك", "بطاقتك", "您的", "あなたの")
private val WalletCue = SmsCue("m-pesa", "mpesa", "wallet", "e-wallet", "ewallet", "mobile money", "gcash", "paytm", "stc pay", "محفظة")
private val DebitCue = SmsCue("mada", "debit", "debit card", "atm card", "girocard", "ec-karte", "débito", "debito", "rupay", "maestro", "interac", "مدى")
private val AccountCue = SmsCue("a/c", "acct", "account", "current", "checking", "chequing", "cheq", "konto", "girokonto", "cuenta", "compte", "conta", "rekening",
    "hesap", "حساب", "账户", "帐户", "口座")
private val CardCue = SmsCue("card", "crd", "carte", "karte", "tarjeta", "cartão", "cartao", "carta", "kartu", "kart", "بطاقة", "カード", "卡")
private val PaymentCue = SmsCue("payment", "repayment", "paid", "pago", "paiement", "zahlung", "pagamento", "pembayaran", "سداد", "还款", "返済")
private val Brands = listOf("visa" to "Visa", "mastercard" to "Mastercard", "master card" to "Mastercard", "amex" to "Amex", "american express" to "Amex",
    "discover" to "Discover", "jcb" to "JCB", "unionpay" to "UnionPay", "mada" to "mada", "مدى" to "mada", "rupay" to "RuPay", "maestro" to "Maestro", "interac" to "Interac")
    .map { (cue, name) -> SmsCue(cue) to name }
private val CreditBrands = setOf("Visa", "Mastercard", "Amex", "Discover", "JCB", "UnionPay")

/**
 * The words a text puts before a card's digits, back to the line's start or the number before them
 * ("VISA Card:" in "Amount:SAR 5\nVISA Card:1234"), and the few after, up to the next number, for
 * languages that name it after ("尾号1234的信用卡"). Lowercase; empty when the digits aren't there.
 */
internal fun smsWordsBy(body: String, ref: String): Pair<String, String> {
    val text = body.map { c -> Character.digit(c, 10).takeIf { it >= 0 }?.let { '0' + it } ?: c }.joinToString("")
    val digits = smsDigits(ref)
    val found = if (digits.length >= 3) (Regex("(?<![0-9])" + digits + "(?![0-9])").find(text) ?: Regex("(?<![0-9])[0-9]*" + digits.takeLast(4) + "(?![0-9])").find(text))
        else ref.trim().takeIf { it.isNotEmpty() }?.let { Regex(Regex.escape(it), RegexOption.IGNORE_CASE).find(text) }
    val range = found?.range ?: return "" to ""
    val number = Regex("[0-9]{2,}")
    fun tail(s: String) = s.substringAfterLast('\n').let { line -> number.findAll(line).lastOrNull()?.let { line.substring(it.range.last + 1) } ?: line }.takeLast(48)
    val prefix = text.substring(0, range.first)
    var before = tail(prefix)
    // A card's name on the line above its digits.
    if (before.count { it.isLetter() } < 3 && '\n' in prefix && number.find(prefix.substringAfterLast('\n')) == null) before = tail(prefix.substringBeforeLast('\n')) + " " + before
    val after = text.substring(range.last + 1).substringBefore('\n').let { s -> number.find(s)?.let { s.substring(0, it.range.first) } ?: s }.take(24)
    return before.lowercase(Locale.ROOT) to after.lowercase(Locale.ROOT)
}

/** What a candidate is: its account type, whether it's a card rather than an account, and the card's network. */
internal data class SmsSetupKind(val type: String, val card: Boolean, val brand: String?)

/**
 * What a candidate's texts say it is, from the words by its digits: a credit card, savings, a
 * wallet, a debit card or an account. A card that says nothing more of itself is a credit card:
 * a debit card's texts say so (debit, mada, ATM) or name its account, and only a debit card is
 * suggested as part of an account, since putting a credit card in one would be wrong for both.
 * Anything that isn't a card is a current account.
 */
internal fun smsSetupKind(candidate: SmsSetupCandidate): SmsSetupKind {
    fun brand(words: String) = Brands.firstOrNull { it.first.at(words) }?.second
    fun kind(words: String): SmsSetupKind? = when {
        CreditCue.at(words) -> SmsSetupKind("creditCard", true, brand(words))
        SavingsCue.at(words) -> SmsSetupKind("savings", false, null)
        WalletCue.at(words) -> SmsSetupKind("cash", false, null)
        DebitCue.at(words) -> SmsSetupKind("checking", true, brand(words))
        AccountCue.at(words) && !CardCue.at(words) -> SmsSetupKind("checking", false, null)
        else -> null
    }
    // A sender whose texts name no card: what the sender and its texts call it.
    if (candidate.ref == null) {
        val all = (listOf(candidate.sender) + candidate.items.flatMap { item -> item.sources.map { it.body } }).joinToString("\n").lowercase(Locale.ROOT)
        return SmsSetupKind(if (WalletCue.at(all)) "cash" else "checking", false, null)
    }
    val near = smsNear(candidate)
    val before = near.joinToString("\n") { it.first }
    val after = near.joinToString("\n") { it.second }
    // Each text says what it is; most of them decide, a credit card, savings, a wallet, a debit
    // card then an account first on a tie. One mention of another account by the digits doesn't.
    val order = listOf("creditCard" to true, "savings" to false, "cash" to false, "checking" to true, "checking" to false)
    near.mapNotNull { kind(it.first) }.groupBy { it.type to it.card }.entries
        .sortedWith(compareByDescending<Map.Entry<Pair<String, Boolean>, List<SmsSetupKind>>> { it.value.size }.thenBy { order.indexOf(it.key) })
        .firstOrNull()?.let { (_, votes) -> return votes.first().copy(brand = votes.firstNotNullOfOrNull { it.brand }) }
    val paid = candidate.items.any { item -> item.transaction?.text("kind") == "transfer" && (item.amount ?: 0L) > 0L &&
        (PaymentCue.at(item.note.lowercase(Locale.ROOT)) || item.sources.any { PaymentCue.at(it.body.lowercase(Locale.ROOT)) }) }
    if (paid) return SmsSetupKind("creditCard", true, brand(before))
    brand(before)?.let { return SmsSetupKind(if (it in CreditBrands) "creditCard" else "checking", true, it) }
    if (CardCue.at(before)) return SmsSetupKind("creditCard", true, null)
    kind(after)?.let { return it }
    // An account only transfers name may be said a line away from its digits ("Transfer to your
    // investment account … To: 0055"): what its texts call savings, or a credit card, is that.
    if (candidate.items.isEmpty()) {
        val whole = candidate.via.flatMap { item -> item.sources.map { it.body } }.joinToString("\n").lowercase(Locale.ROOT)
        if (SavingsCue.at(whole)) return SmsSetupKind("savings", false, null)
        if (CreditCue.at(whole)) return SmsSetupKind("creditCard", true, brand(whole))
    }
    return if (CardCue.at(after)) SmsSetupKind("creditCard", true, brand(after)) else SmsSetupKind("checking", false, null)
}

/** The words by a candidate's digits in each of its texts ([smsWordsBy]). */
private fun smsNear(candidate: SmsSetupCandidate): List<Pair<String, String>> =
    candidate.items.flatMap { item -> item.sources.map { smsWordsBy(it.body, item.ref.orEmpty()) } } +
        candidate.via.flatMap { item -> item.sources.map { smsWordsBy(it.body, item.transaction!!.text("otherRef")) } }

/**
 * Whether an account only transfers name is surely the user's: its texts call it theirs ("your
 * savings"), or say what kind of account it is beyond a plain number. A bare number ("To: 6902***")
 * is more often someone else's, where the money went is spending.
 */
internal fun smsSetupOwned(candidate: SmsSetupCandidate, kind: SmsSetupKind): Boolean =
    candidate.items.isNotEmpty() || candidate.ref == null || kind.type != "checking" || smsNear(candidate).any { OwnCue.at(it.first) }

/** A sender's name for an account's: "VM-HDFCBK-S", as Indian senders are written, is "HDFCBK". */
internal fun smsSenderName(sender: String): String =
    sender.trim().replace(Regex("^[A-Za-z]{2}-(?=[A-Za-z0-9]{4,})"), "").replace(Regex("(?<=[A-Za-z0-9]{4})-[STPGstpg]$"), "").uppercase(Locale.ROOT)

/**
 * A new account's name: the sender, the card's network or the type when it's savings or a card, and
 * the last digits ("ANB Visa 4512"); none for an account numbered afresh for each transfer.
 */
internal fun smsSetupName(candidate: SmsSetupCandidate, kind: SmsSetupKind, typeName: (String) -> String): String {
    if (candidate.refs.size > 1) return listOf(smsSenderName(candidate.sender), typeName(kind.type)).filter { it.isNotBlank() }.joinToString(" ")
    val digits = candidate.ref?.let(::smsDigits)?.takeIf { it.length >= 3 }?.takeLast(4)
    if (candidate.ref != null && digits == null) return listOf(smsSenderName(candidate.sender), candidate.ref.trim()).filter { it.isNotBlank() }.joinToString(" ")
    val product = kind.brand?.takeIf { it != "mada" } ?: kind.type.takeIf { it == "creditCard" || it == "savings" }?.let(typeName)
    return listOfNotNull(smsSenderName(candidate.sender), product, digits).filter { it.isNotBlank() }.joinToString(" ")
}

/**
 * What to do with a candidate: make an account ([Create]) with this name, type and balance before
 * the review's texts ([opening], worked out from them when null), put its texts in another account
 * ([Join], [joins] an open account's id or another candidate's key), or not add them ([Skip]).
 */
internal data class SmsSetupPlan(val action: String, val name: String = "", val type: String = "checking", val opening: Long? = null, val joins: String? = null,
                                  /** A card's credit limit: with it, the credit left its texts give says what's owed. */
                                  val limit: Long? = null) {
    fun json(): JSONObject = JSONObject().put("action", action).put("name", name).put("type", type).put("opening", opening ?: JSONObject.NULL).put("joins", joins ?: JSONObject.NULL)
        .put("limit", limit ?: JSONObject.NULL)

    companion object {
        const val Create = "create"
        const val Join = "join"
        const val Skip = "skip"
        fun of(json: JSONObject): SmsSetupPlan? = runCatching {
            SmsSetupPlan(json.getString("action").also { require(it in setOf(Create, Join, Skip)) }, json.text("name"), json.text("type").ifBlank { "checking" },
                if (json.isNull("opening")) null else json.getLong("opening"), json.text("joins").ifBlank { null },
                json.optLong("limit").takeIf { it > 0 })
        }.getOrNull()
    }
}

/**
 * What the setup suggests for each candidate: an account of the type and name its texts suggest.
 * A debit card is suggested as part of its sender's current account, being set up (one whose own
 * texts are on it, the main one when one has three times the texts of any other) or already in
 * the budget under the sender's name: the card and the account are the same money, and two
 * accounts would count it twice. An account only transfers name that isn't surely the user's
 * ([smsSetupOwned]) isn't tracked: what moved to or from it is spending or income.
 */
internal fun smsSetupSuggestions(candidates: List<SmsSetupCandidate>, budget: JSONObject, typeName: (String) -> String): Map<String, SmsSetupPlan> {
    val kinds = candidates.associate { it.key to smsSetupKind(it) }
    val open = budget.rows("accounts").filterNot { it.optBoolean("archived") }
    return candidates.associate { c ->
        val kind = kinds.getValue(c.key)
        val plan = SmsSetupPlan(SmsSetupPlan.Create, smsSetupName(c, kind, typeName), kind.type)
        val sender = smsSenderKey(c.sender)
        val accounts = candidates.filter { smsSenderKey(it.sender) == sender && it.key != c.key && it.items.isNotEmpty() && kinds.getValue(it.key).let { k -> k.type == "checking" && !k.card } }
            .sortedByDescending { it.items.size }
        val named = smsSenderName(c.sender).takeIf { it.length >= 2 }
        val joins = if (!kind.card || kind.type != "checking") null else when {
            accounts.size == 1 || (accounts.size > 1 && accounts[0].items.size >= 3 * accounts[1].items.size) -> accounts[0].key
            accounts.isEmpty() -> named?.let { n -> open.filter { a -> AccountTypes.infer(a) == "checking" && a.text("name").uppercase(Locale.ROOT).contains(n) }.singleOrNull()?.text("id") }
            else -> null
        }
        c.key to when {
            joins != null -> plan.copy(action = SmsSetupPlan.Join, joins = joins)
            !smsSetupOwned(c, kind) -> plan.copy(action = SmsSetupPlan.Skip)
            else -> plan
        }
    }.let { plans -> smsUniqueNames(plans, candidates.associateBy { it.key }, open.map { it.text("name") }) }
}

/**
 * Accounts to make, each under its own name: two named alike ("ANB savings" for two sets of
 * deposits) read as one. A name already in the budget or suggested before takes the candidate's
 * digits, else a number.
 */
internal fun smsUniqueNames(plans: Map<String, SmsSetupPlan>, candidates: Map<String, SmsSetupCandidate>, existing: List<String>): Map<String, SmsSetupPlan> {
    val taken = existing.mapTo(HashSet()) { it.trim().lowercase(Locale.ROOT) }
    return plans.mapValues { (key, plan) ->
        if (plan.action != SmsSetupPlan.Create) return@mapValues plan
        val digits = candidates[key]?.ref?.let(::smsDigits)?.takeIf { it.length >= 2 }?.takeLast(4)
        val name = sequenceOf(plan.name) + listOfNotNull(digits?.takeUnless { plan.name.endsWith(it) }?.let { "${plan.name} $it" }).asSequence() + generateSequence(2) { it + 1 }.map { "${plan.name} $it" }
        val chosen = name.first { it.trim().lowercase(Locale.ROOT) !in taken }
        taken += chosen.trim().lowercase(Locale.ROOT)
        if (chosen == plan.name) plan else plan.copy(name = chosen)
    }
}

/**
 * Each candidate's plan: the user's answer, else the suggestion. One that joins a candidate not
 * being made, or an account that's gone, is made itself.
 */
internal fun smsSetupPlans(candidates: List<SmsSetupCandidate>, answers: Map<String, SmsSetupPlan>, suggested: Map<String, SmsSetupPlan>, open: Set<String>): Map<String, SmsSetupPlan> {
    val chosen = candidates.associate { it.key to (answers[it.key] ?: suggested.getValue(it.key)) }
    return chosen.mapValues { (key, plan) ->
        val fine = plan.action != SmsSetupPlan.Join || plan.joins in open || (plan.joins != key && chosen[plan.joins]?.action == SmsSetupPlan.Create)
        if (fine) plan else plan.copy(action = SmsSetupPlan.Create, joins = null)
    }
}

/** A balance a text gives, in cents: zero and overdrawn ones too. */
internal fun smsBalanceCents(text: String): Long? = runCatching {
    require(text.matches(Regex("-?[0-9]+(?:\\.[0-9]{1,2}0*)?")))
    BigDecimal(text).movePointRight(2).longValueExact().also { require(it in -1_000_000_000_000L..1_000_000_000_000L) }
}.getOrNull()

/** An account's balance before the review's texts, worked out from the balance [given] by the latest text that gives one, received on [date]. */
internal data class SmsOpening(val balance: Long, val given: Long, val date: String)

/**
 * The balance before the review's texts of an account made for [members] (a card and the cards that
 * are part of it): the balance the latest text about it gives after its money moved, less what the
 * review [adds] to the account up to and including that text, transfers with it included. Its
 * balance after adding is then the bank's as of that text, even with earlier texts missing. Null
 * when no text gives a balance.
 */
internal fun smsOpening(members: List<SmsSetupCandidate>, adds: (SmsItem) -> Boolean, limit: Long? = null): SmsOpening? {
    val own = members.flatMap { it.items }.distinctBy { it.index }
    val via = members.flatMap { it.via }.distinctBy { it.index }
    fun at(item: SmsItem) = item.sources.minOf { it.received }
    // A card with a limit: the credit left less the limit, the balance as the budget writes it (negative when owed).
    // A card's "balance" is its credit left, as most banks text it ([smsBankFigure]).
    fun given(t: JSONObject): Long? = if (limit != null) (smsBalanceCents(t.text("available")) ?: smsBalanceCents(t.text("balance")))?.let { it - limit }
        else smsBalanceCents(t.text("balance"))
    val (alert, given) = own.filter { it.disposition == "transaction" }.mapNotNull { item -> item.transaction?.let(::given)?.let { item to it } }
        .maxWithOrNull(compareBy<Pair<SmsItem, Long>> { at(it.first) }.thenBy { it.first.index }) ?: return null
    val time = at(alert)
    val net = own.filter { adds(it) && at(it) <= time }.sumOf { it.amount ?: 0L } - via.filter { adds(it) && at(it) <= time }.sumOf { it.amount ?: 0L }
    return SmsOpening(given - net, given, alert.sources.first().receivedDate)
}

/**
 * The balance an account to make starts with: what the user entered, else what its texts say. A
 * card's texts give the credit left, so without its limit a card starts with nothing owed.
 */
internal fun smsSetupBalance(plan: SmsSetupPlan, opening: SmsOpening?): Long =
    plan.opening ?: if (AccountTypes.isLiability(plan.type) && smsPlanLimit(plan) == null) 0L else opening?.balance ?: 0L

/** A plan's credit limit, when it makes a credit account: [smsOpening] reads the credit left against it. */
internal fun smsPlanLimit(plan: SmsSetupPlan): Long? = plan.limit?.takeIf { it > 0 && AccountTypes.kindFor(plan.type) == "credit" }

/** The candidates that are part of [key]'s account, itself first. */
internal fun smsSetupMembers(key: String, candidates: List<SmsSetupCandidate>, plans: Map<String, SmsSetupPlan>): List<SmsSetupCandidate> =
    candidates.filter { it.key == key || plans[it.key]?.let { p -> p.action == SmsSetupPlan.Join && p.joins == key } == true }.sortedByDescending { it.key == key }

/** An account remembered for a sender's card or account by its digits, or with [kind] for a format ([smsRememberCard]). */
internal data class SmsRemembered(val sender: String, val ref: String, val account: String, val kind: String = "")

/**
 * The setup as changes: the accounts to make (records for the budget), what to remember for each
 * candidate's sender and digits (an account's id, or [SmsSkipCard]), and for one numbered afresh
 * the formats of its texts, and the transfers to an account the user doesn't track, which are
 * then spending or income. A credit card starts at nothing owed unless the user said otherwise or
 * gave its limit: a card's texts usually give the credit left, which says what's owed only beside it.
 */
internal data class SmsSetupChanges(val accounts: List<JSONObject>, val remember: List<SmsRemembered>, val spending: List<Int>)
internal fun smsSetupChanges(candidates: List<SmsSetupCandidate>, plans: Map<String, SmsSetupPlan>, adds: (SmsItem) -> Boolean, newId: () -> String): SmsSetupChanges {
    val made = candidates.filter { plans[it.key]?.action == SmsSetupPlan.Create }.associate { it.key to newId() }
    val accounts = candidates.mapNotNull { c ->
        val plan = plans.getValue(c.key).takeIf { it.action == SmsSetupPlan.Create } ?: return@mapNotNull null
        val opening = smsSetupBalance(plan, smsOpening(smsSetupMembers(c.key, candidates, plans), adds, smsPlanLimit(plan)))
        JSONObject().put("id", made.getValue(c.key)).put("name", plan.name.trim().ifBlank { smsSenderName(c.sender) }).put("kind", AccountTypes.kindFor(plan.type))
            .put("openingBalance", opening).put("accountType", plan.type).apply { smsPlanLimit(plan)?.let { put("creditLimit", it) } }
    }
    val remember = candidates.flatMap { c ->
        val plan = plans.getValue(c.key)
        val account = when (plan.action) {
            SmsSetupPlan.Create -> made.getValue(c.key)
            SmsSetupPlan.Join -> made[plan.joins] ?: plan.joins!!
            else -> SmsSkipCard
        }
        val formats = if (c.refs.size > 1 && account != SmsSkipCard) c.via.mapNotNull { it.template }.distinct().map { SmsRemembered(c.sender, it, account, SmsForFormat) } else emptyList()
        c.refs.ifEmpty { listOf(SmsAnyCard) }.map { SmsRemembered(c.sender, it, account) } + formats
    }
    val spending = candidates.filter { plans[it.key]?.action == SmsSetupPlan.Skip }.flatMap { c -> c.via.map { it.index } }
    return SmsSetupChanges(accounts, remember, spending)
}

/** A transfer item to an account the user doesn't track, made the spending or income it is for theirs. */
internal fun smsUntracked(item: JSONObject) {
    val t = item.getJSONObject("transaction")
    t.put("kind", if ((smsCents(t.text("amount")) ?: 0L) < 0L) "expense" else "income").put("otherRef", JSONObject.NULL).put("otherAccount", JSONObject.NULL)
}
