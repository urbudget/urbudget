package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class BankTextsTest {
    private var clock = 1_793_000_000_000L
    private fun sms(body: String = "text"): BankSms { clock += 600_000; return BankSms(smsId("Bank", clock, body), "Bank", clock, body) }
    private fun item(index: Int, account: String?, amount: String?, disposition: String = "transaction", balance: String? = null, available: String? = null,
                     transfer: String? = null, skip: String = "", sources: List<BankSms> = listOf(sms())) =
        SmsItem(index, disposition, "", skip, emptySet(), sources,
            if (amount == null) null else JSONObject().put("date", sources.first().receivedDate).put("amount", amount).put("payee", "Shop")
                .put("accountId", account ?: JSONObject.NULL).put("transferAccountId", transfer ?: JSONObject.NULL)
                .put("balance", balance ?: JSONObject.NULL).put("available", available ?: JSONObject.NULL), null, null)
    private fun budget(vararg transactions: JSONObject) = JSONObject().put("currency", "USD").put("transactions", JSONArray(transactions.toList()))
        .put("accounts", JSONArray().put(JSONObject().put("id", "checking").put("name", "Checking").put("kind", "cash"))
            .put(JSONObject().put("id", "card").put("name", "Visa").put("kind", "credit").put("creditLimit", 100_000L))
            .put(JSONObject().put("id", "plain").put("name", "Store card").put("kind", "credit")))

    @Test fun anAmountIsFoundHoweverTheBankWritesIt() {
        assertTrue(123_450L in smsWrittenAmounts("Purchase SAR 1,234.50 at NOON"))
        assertTrue(123_450L in smsWrittenAmounts("Kartenzahlung 1.234,50 EUR bei REWE"))
        assertTrue(123_450L in smsWrittenAmounts("Spent 1234.5 at shop"))
        assertTrue(1_250L in smsWrittenAmounts("KWD 12.500 debited"))
        assertTrue(50_000L in smsWrittenAmounts("SAR 500 transferred"))
        assertTrue(123_450L in smsWrittenAmounts("شراء بمبلغ ١٬٢٣٤٫٥٠ ريال"))
        assertTrue(123_400L in smsWrittenAmounts("New M-PESA balance is Ksh1,234.00."))
        assertFalse(12_345L in smsWrittenAmounts("SAR 1,234.50"))
        // Words in any case, all of them, and the amount when given.
        assertTrue(smsTextMatches("Purchase SAR 45.00 at Careem RIDE", "careem ride", 4_500L))
        assertFalse(smsTextMatches("Purchase SAR 45.00 at Careem", "careem uber", null))
        assertFalse(smsTextMatches("Purchase SAR 45.00 at Careem", "", 4_600L))
        assertTrue(smsTextMatches("Anything", "  ", null))
    }

    @Test fun chatSearchesTextsOnlyForTheAssistantBankMessagesWasAllowedToSendTo() {
        val connection = JSONObject().put("provider", "anthropic").put("model", "claude-x").put("ready", true)
        val allowed = smsConsent(connection)
        assertTrue(bankTextsAllowed(canRead = true, senders = 2, consent = allowed, connection = connection))
        // Another service, another model, or its default model: each asks in Bank messages first.
        assertFalse(bankTextsAllowed(true, 2, allowed, JSONObject().put("provider", "openai").put("model", "claude-x")))
        assertFalse(bankTextsAllowed(true, 2, allowed, JSONObject().put("provider", "anthropic").put("model", "claude-y")))
        assertFalse(bankTextsAllowed(true, 2, allowed, JSONObject().put("provider", "anthropic").put("model", JSONObject.NULL)))
        // Any stored permission used to do: one from before version 2, or one that doesn't read, doesn't.
        assertFalse(bankTextsAllowed(true, 2, """{"provider":"anthropic","model":"claude-x"}""", connection))
        assertFalse(bankTextsAllowed(true, 2, "yes", connection))
        assertFalse(bankTextsAllowed(true, 2, null, connection))
        // No assistant known, no senders chosen, or SMS access off.
        assertFalse(bankTextsAllowed(true, 2, allowed, null))
        assertFalse(bankTextsAllowed(true, 0, allowed, connection))
        assertFalse(bankTextsAllowed(false, 2, allowed, connection))
    }

    @Test fun anAddCardNamesItsTextsAndTheirWordsAreReadAgainWhenItsApplied() {
        val searched = sms("Purchase SAR 45.00 at Careem")
        val early = sms("Purchase SAR 12.00 at Bakery")
        val late = sms("Purchase SAR 80.00 at Pharmacy")
        val deleted = sms("Purchase SAR 5.00 at Kiosk")
        val other = BankSms(smsId("Card", clock, "Refund SAR 9.00"), "Card", clock, "Refund SAR 9.00")
        fun named(text: BankSms) = JSONObject().put("id", text.id).put("sender", text.sender).put("received", text.received)
        val card = JSONObject().put("action", "add").put("texts", JSONArray(listOf(searched, early, late, deleted, other).map(::named)))
        val reads = mutableListOf<Triple<String, Long, Long>>()
        val (texts, missing) = bankTextsOfCard(card, cached = { id -> searched.takeIf { it.id == id } }) { sender, from, until ->
            reads += Triple(sender, from, until)
            // The phone holds more from each sender than the card names, and no longer the deleted one.
            if (sender == "Bank") listOf(late, sms("Balance SAR 900.00"), early) else listOf(other)
        }
        assertEquals(listOf(searched, early, late, other), texts)
        assertEquals(1, missing)
        // One reading for each sender, from just before its earliest text to just after its latest; what a search found isn't read again.
        assertEquals(listOf("Bank", "Card"), reads.map { it.first })
        assertTrue(reads[0].second < early.received && reads[0].third > deleted.received && reads[0].second > early.received - 600_000)
        // A card made when cards kept the words still adds them, with nothing read.
        val old = JSONObject().put("action", "add").put("texts", JSONArray().put(named(deleted).put("body", deleted.body)))
        assertEquals(listOf(deleted) to 0, bankTextsOfCard(old, { null }) { _, _, _ -> error("An old card's texts aren't read again.") })
        // Nothing left of a card's texts: all of them are missing.
        assertEquals(emptyList<BankSms>() to 2, bankTextsOfCard(JSONObject().put("texts", JSONArray(listOf(early, late).map(::named))), { null }) { _, _, _ -> emptyList() })
    }

    @Test fun aReadCardCountsOnlyTheTextsOutsideTheOpenReview() {
        // The open review's texts are read already and wait for the user's ticks: Bank messages reads the rest.
        assertEquals(2, bankTextsToRead(listOf("a", "b", "c", "d"), listOf("b", "d")))
        assertEquals(0, bankTextsToRead(listOf("a", "b"), listOf("a", "b", "gone")))
        assertEquals(3, bankTextsToRead(listOf("a", "b", "c"), emptyList()))
        assertEquals(0, bankTextsToRead(emptyList(), emptyList()))
    }

    @Test fun aCardIsCheckedAgainAgainstTheBudgetAsItsApplied() {
        val budget = budget().apply { getJSONArray("accounts").put(JSONObject().put("id", "closed").put("name", "Old card").put("kind", "credit").put("archived", true)) }
        fun limit(account: String) = JSONObject().put("action", "limit").put("accountId", account).put("limit", 250_000L)
        fun remember(account: String) = JSONObject().put("action", "remember").put("sender", "Bank").put("ref", "1234").put("accountId", account)
        assertNull(bankTextsCardProblem(limit("card"), budget))
        assertNull(bankTextsCardProblem(remember("checking"), budget))
        // The account went since the card was made, or is no longer a card: nothing is done, and that's said.
        assertEquals(R.string.bank_texts_unknown_account, bankTextsCardProblem(limit("deleted"), budget))
        assertEquals(R.string.bank_texts_not_card, bankTextsCardProblem(limit("checking"), budget))
        assertEquals(R.string.bank_texts_unknown_account, bankTextsCardProblem(remember("deleted"), budget))
        assertEquals(R.string.bank_texts_unknown_account, bankTextsCardProblem(remember("closed"), budget))
        assertEquals(R.string.bank_texts_unknown_account, bankTextsCardProblem(limit("card"), null))
        // Reading and adding texts name no account.
        assertNull(bankTextsCardProblem(JSONObject().put("action", "read").put("note", ""), null))
        assertNull(bankTextsCardProblem(JSONObject().put("action", "add"), budget))
    }

    @Test fun eachSettledTextKeepsWhatBecameOfIt() {
        val pair = listOf(sms("one"), sms("two"))
        val added = item(0, "checking", "-12.00", balance = "88.00", sources = pair)
        val matched = item(1, "checking", "-5.00")
        val code = item(2, null, null, disposition = "ignore", skip = "code")
        val dismissed = item(3, null, null, disposition = "unclear")
        val unticked = item(4, "card", "-7.00")
        val outcomes = smsOutcomes(listOf(added, matched, code, dismissed, unticked), listOf(0 to "new", 1 to "match:t9"), setOf(3), at = 1L)
        // A text read with another is the same transaction.
        assertEquals(setOf(pair[0].id, pair[1].id), pair.map { it.id }.filter { outcomes[it]?.getString("outcome") == SmsOutcome.Added }.toSet())
        val first = outcomes.getValue(pair[0].id)
        assertEquals(listOf("checking", "-1200", "8800"), listOf(first.getString("accountId"), first.getLong("amount").toString(), first.getLong("balance").toString()))
        assertEquals(pair[1].received, first.getLong("received"))
        assertEquals("t9", outcomes.getValue(matched.sources[0].id).getString("transactionId"))
        assertEquals("code", outcomes.getValue(code.sources[0].id).getString("reason"))
        assertEquals(SmsOutcome.Dismissed, outcomes.getValue(dismissed.sources[0].id).getString("outcome"))
        assertEquals(SmsOutcome.Cleared, outcomes.getValue(unticked.sources[0].id).getString("outcome"))
        assertEquals(SmsOutcome.Discarded, smsPlainOutcomes(listOf("a"), SmsOutcome.Discarded).getValue("a").getString("outcome"))
    }

    @Test fun theBanksLatestFigureIsKeptAndAnOlderOneDoesntReplaceIt() {
        fun outcome(outcome: String, account: String, received: Long, balance: Long? = null, available: Long? = null) = JSONObject().put("outcome", outcome)
            .put("accountId", account).put("received", received).put("date", "2026-11-0${received % 9}").apply { balance?.let { put("balance", it) }; available?.let { put("available", it) } }
        val first = smsKeepBalances(null, listOf(outcome(SmsOutcome.Added, "checking", 5, balance = 8_800), outcome(SmsOutcome.Matched, "card", 5, balance = 1, available = 95_500)))!!
        val kept = JSONObject(first)
        assertEquals(8_800L, kept.getJSONObject("checking").getLong("cents"))
        // A card's credit left is kept over a "balance" whose sign texts don't settle.
        assertEquals("available", kept.getJSONObject("card").getString("kind"))
        // Older, or not added, changes nothing.
        assertSame(first, smsKeepBalances(first, listOf(outcome(SmsOutcome.Added, "checking", 4, balance = 1), outcome(SmsOutcome.Cleared, "checking", 9, balance = 2))))
        assertEquals(7_000L, JSONObject(smsKeepBalances(first, listOf(outcome(SmsOutcome.Added, "checking", 6, balance = 7_000)))!!).getJSONObject("checking").getLong("cents"))
    }

    @Test fun aCardsFigureIsItsCreditLeftLessItsLimit() {
        val b = budget()
        fun account(id: String) = b.getJSONArray("accounts").let { a -> (0 until a.length()).map { a.getJSONObject(it) }.first { it.getString("id") == id } }
        assertEquals(8_800L, smsBankFigure(account("checking"), "balance", 8_800))
        assertEquals(-4_500L, smsBankFigure(account("card"), "available", 95_500))
        // A card's "balance" is its credit left, as ANB texts it, unless its texts show it's what's owed.
        assertEquals(-95_500L, smsBankFigure(account("card"), "balance", 4_500))
        assertEquals(-4_500L, smsBankFigure(account("card"), "balance", 4_500, owes = true))
        assertEquals(-4_500L, smsBankFigure(account("plain"), "balance", 4_500, owes = true))
        assertNull(smsBankFigure(account("plain"), "available", 95_500))
        assertNull(smsBankFigure(account("plain"), "balance", 95_500))
        assertEquals(100_000L, smsCreditLimit(account("card")))
        assertNull(smsCreditLimit(account("checking").put("creditLimit", 5L)))
    }

    @Test fun aCardsTextsShowWhetherTheirBalanceIsTheCreditLeftOrWhatsOwed() {
        // The credit left falls with each purchase; what's owed rises.
        val left = listOf(item(0, "card", "-12.00", balance = "988.00"), item(1, "card", "-8.00", balance = "980.00"), item(2, "card", "-5.00", balance = "975.00"))
        val owed = listOf(item(3, "card", "-12.00", balance = "12.00"), item(4, "card", "-8.00", balance = "20.00"), item(5, "card", "-5.00", balance = "25.00"))
        assertFalse(smsCardOwes(left, "card"))
        assertTrue(smsCardOwes(owed, "card"))
        // With nothing to tell by, the credit left, as most banks text it.
        assertFalse(smsCardOwes(listOf(item(6, "card", "-12.00", balance = "12.00")), "card"))
        // A card whose texts give what's owed needs no limit: 25.00 owed is -25.00 in the budget, as the review adds.
        val store = listOf(item(7, "plain", "-12.00", balance = "12.00"), item(8, "plain", "-8.00", balance = "20.00"), item(9, "plain", "-5.00", balance = "25.00"))
        val check = smsBankCheck("plain", budget(), 0L, store, store.map { it.index to "new" }) { true }!!
        assertEquals(-2_500L, check.bank)
        assertEquals(0L, check.difference)
    }

    @Test fun theBudgetAtADayLeavesOutWhatsDatedAfterIt() {
        val b = budget(
            JSONObject().put("id", "a").put("date", "2026-11-01").put("accountId", "checking").put("amount", -1_000L).put("transferAccountId", JSONObject.NULL),
            JSONObject().put("id", "b").put("date", "2026-11-05").put("accountId", "checking").put("amount", -2_000L).put("transferAccountId", JSONObject.NULL),
            // A card payment after the day: money out of checking, into the card.
            JSONObject().put("id", "c").put("date", "2026-11-06").put("accountId", "checking").put("amount", -30_000L).put("transferAccountId", "card"))
        assertEquals(10_000L + 2_000L + 30_000L, smsBudgetBalanceAt(b, 10_000L, "checking", "2026-11-04"))
        assertEquals(-15_000L - 30_000L, smsBudgetBalanceAt(b, -15_000L, "card", "2026-11-05"))
    }

    @Test fun theReviewSaysWhetherTheBankAgreesOrWhatsHeld() {
        val b = budget()
        // 100.00 in checking now; the review adds 12.00 spent, and the bank says 88.00 after it.
        val spent = item(0, "checking", "-12.00", balance = "88.00")
        val agrees = smsBankCheck("checking", b, 10_000L, listOf(spent), listOf(0 to "new")) { true }!!
        assertEquals(0L, agrees.difference)
        // Not added, the budget is 12.00 above the bank.
        assertEquals(1_200L, -smsBankCheck("checking", b, 10_000L, listOf(spent), emptyList()) { false }!!.difference)
        // A card hold left unticked: the 955.00 of 1,000.00 left already counts its 45.00.
        val hold = item(1, "card", "-45.00", disposition = "pending", available = "955.00")
        val held = smsBankCheck("card", b, 0L, listOf(hold), emptyList()) { false }!!
        assertEquals(-4_500L, held.bank)
        assertEquals(0L, held.budget)
        assertEquals(-4_500L, held.held)
        assertEquals(held.held, held.difference)
        // ANB's card texts give the credit left as "Balance": read so against the limit, they agree.
        val coffee = item(4, "card", "-12.00", balance = "988.00")
        val pastry = item(5, "card", "-8.00", balance = "980.00")
        val card = smsBankCheck("card", b, 0L, listOf(coffee, pastry), listOf(4 to "new", 5 to "new")) { true }!!
        assertEquals(-2_000L, card.bank)
        assertEquals(0L, card.difference)
        // A card with no limit, or no text giving a figure, has nothing to check.
        assertNull(smsBankCheck("plain", b, 0L, listOf(item(2, "plain", "-5.00", available = "100.00")), emptyList()) { true })
        assertNull(smsBankCheck("checking", b, 0L, listOf(item(3, "checking", "-5.00")), emptyList()) { true })
    }

    private fun asked(text: BankSms, amount: Double, kind: String = "expense", account: String = "checking", payee: String = "Careem", match: String? = null, transfer: String? = null) =
        BankTextEntry.asked(JSONObject().put("textIds", JSONArray().put(text.id)).put("accountId", account).put("date", text.receivedDate).put("amount", amount)
            .put("kind", kind).put("payee", payee).put("match", match ?: JSONObject.NULL).put("transferAccountId", transfer ?: JSONObject.NULL))

    @Test fun aCardEntersATransactionOnlyAsItsTextSaysIt() {
        val ride = sms("Purchase SAR 45.00 at Careem\nCard 3358")
        val pay = sms("Salary SAR 9,000.00 credited")
        val moved = sms("Transfer of SAR 500.00 to card 4417")
        val texts = listOf(ride, pay, moved).associateBy { it.id }
        val b = budget()
        // Money out is negative and money in positive, whichever way the assistant wrote it; a transfer keeps its sign.
        val entries = listOf(asked(ride, 45.0), asked(pay, -9000.0, kind = "income", payee = "Employer"), asked(moved, -500.0, kind = "transfer", payee = "Visa", transfer = "card"))
        assertEquals(listOf(-4_500L, 900_000L, -50_000L), entries.map { it.cents })
        assertNull(bankTextEntriesProblem(entries, texts, b))
        // An amount the text doesn't give was made up; a text is entered once, into an open account, and a transfer names its other account.
        assertNotNull(bankTextEntriesProblem(listOf(asked(ride, 54.0)), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(asked(ride, 45.0), asked(ride, 45.0)), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(asked(ride, 45.0, account = "closed")), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(asked(moved, -500.0, kind = "transfer")), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(asked(ride, 45.0, transfer = "card")), texts, b))
        assertNotNull(bankTextEntriesProblem(emptyList(), texts, b))
        // The engine gets them as a review's reading: each text read once, its money as written, backed by its own first line.
        val reading = bankTextsReading(entries, texts, "SAR")
        assertEquals(listOf(ride, pay, moved), reading.messages)
        val first = reading.analysis.getJSONArray("items").getJSONObject(0)
        assertEquals("transaction", first.getString("disposition"))
        assertEquals(ride.id, first.getJSONArray("sourceIds").getString(0))
        assertEquals("Purchase SAR 45.00 at Careem", first.getJSONArray("evidence").getJSONObject(0).getString("quote"))
        val t = first.getJSONObject("transaction")
        assertEquals(listOf("-45.00", "45.00", "SAR", "checking"), listOf(t.getString("amount"), t.getString("writtenAmount"), t.getString("currency"), t.getString("accountId")))
        assertTrue(t.isNull("transferAccountId") && t.isNull("categoryId") && t.isNull("accountRef"))
        assertEquals("card", reading.analysis.getJSONArray("items").getJSONObject(2).getJSONObject("transaction").getString("transferAccountId"))
        // A quote is cut inside its line, never through a character written as two.
        assertTrue(ride.body.contains(bankTextQuote(ride.body)) && bankTextQuote("\n  \n Paid 5.00 \n").equals("Paid 5.00"))
        assertEquals(79, bankTextQuote("a".repeat(79) + "😀 more").length)
        // The card keeps what was asked, and reads it back the same.
        val kept = entries[2].copy(match = "t9", categoryId = null)
        assertEquals(kept, BankTextEntry.carded(JSONObject(kept.card().toString())))
    }

    @Test fun anEnteredTextIsAddedMatchedOrLeftOutByWhatTheBudgetHas() {
        val ride = sms("Purchase SAR 45.00 at Careem")
        val day = ride.receivedDate
        fun saved(id: String, date: String) = JSONObject().put("id", id).put("date", date).put("payee", "Careem").put("amount", -4_500L).put("accountId", "checking")
        val b = budget(saved("t1", day), saved("t2", "2026-01-01"))
        fun row(vararg candidates: String, duplicate: Boolean = false, before: List<String> = emptyList()) = JSONObject().put("duplicate", duplicate)
            .put("candidates", JSONArray(candidates.toList())).put("previouslyMatchedCandidates", JSONArray(before))
        fun decide(entry: BankTextEntry, row: JSONObject) = bankTextsDecide(listOf(entry), mapOf(0 to row), b)
        val plain = asked(ride, 45.0)
        // Nothing it could be: added. The one transaction it could be, the same day: matched, as the review does unasked.
        assertNull(decide(plain, row()).entries.single().match)
        assertEquals("t1", decide(plain, row("t1")).entries.single().match)
        // One on another day, one a text matched before, or several: the assistant says which, or that it's new.
        assertTrue(runCatching { decide(plain, row("t2")) }.exceptionOrNull()!!.message!!.contains("t2 (2026-01-01, Careem, -45.00, Checking)"))
        assertTrue(runCatching { decide(plain, row("t1", before = listOf("t1"))) }.isFailure)
        assertTrue(runCatching { decide(plain, row("t1", "t2")) }.isFailure)
        assertEquals("t2", decide(asked(ride, 45.0, match = "t2"), row("t1", "t2")).entries.single().match)
        assertNull(decide(asked(ride, 45.0, match = "new"), row("t1", "t2")).entries.single().match)
        assertTrue(runCatching { decide(asked(ride, 45.0, match = "t7"), row("t1")) }.isFailure)
        // A split's part is offered by its own id, and named by the transaction it's part of.
        val part = row("t1:1").put("candidateDetails", JSONArray().put(JSONObject().put("id", "t1:1").put("transactionId", "t1")))
        assertEquals("t1:1", decide(asked(ride, 45.0, match = "t1"), part).entries.single().match)
        // A text the budget already took in isn't entered again.
        val there = decide(plain, row(duplicate = true))
        assertEquals(emptyList<BankTextEntry>() to listOf(ride.id), there.entries to there.already)
    }

    @Test fun enteredTextsAreFinishedAndLeaveTheOpenReview() {
        val ride = sms("Purchase SAR 45.00 at Careem")
        val lunch = sms("Purchase SAR 30.00 at Diner")
        val gone = sms("Purchase SAR 12.00 at Bakery")
        val texts = listOf(ride, lunch, gone).associateBy { it.id }
        val entries = listOf(asked(ride, 45.0), asked(lunch, 30.0, payee = "Diner"), asked(gone, 12.0, payee = "Bakery"))
        // Added, matched to a transaction, or in the budget already: each text keeps what became of it, and when it came.
        val outcomes = bankTextOutcomes(entries, texts, mapOf(0 to "new", 1 to "match:t4"), at = 5L)
        assertEquals(listOf(SmsOutcome.Added, SmsOutcome.Matched, SmsOutcome.Matched), listOf(ride, lunch, gone).map { outcomes.getValue(it.id).getString("outcome") })
        assertEquals("t4", outcomes.getValue(lunch.id).getString("transactionId"))
        assertFalse(outcomes.getValue(ride.id).has("transactionId") || outcomes.getValue(gone.id).has("transactionId"))
        assertEquals(listOf(ride.received, -4_500L, "checking"), outcomes.getValue(ride.id).let { listOf(it.getLong("received"), it.getLong("amount"), it.getString("accountId")) })
        // The open review loses what it read from an entered text and keeps the rest, with its ticks.
        fun read(text: BankSms) = JSONObject().put("sourceIds", JSONArray().put(text.id)).put("disposition", "transaction")
        val review = SmsBatch(listOf(ride, lunch, gone), JSONObject().put("items", JSONArray(listOf(ride, lunch, gone).map(::read))), ticked = mapOf(2 to false))
        val left = review.without(setOf(ride.id, lunch.id))!!
        assertEquals(listOf(gone), left.messages)
        assertEquals(mapOf(0 to false), left.ticked)
        assertNull(review.without(texts.keys))
        assertEquals(review.messages, review.without(setOf("sms:other"))!!.messages)
    }

    /** A deposit's account beside the usual ones, and the bank's one text for the deposit paid back with its profit. */
    private fun withDeposit() = budget().apply { getJSONArray("accounts").put(JSONObject().put("id", "deposit").put("name", "Term deposit").put("kind", "tracking")) }
    private fun part(amount: Double, transfer: String? = null, category: String? = null, memo: String = "") = JSONObject().put("amount", amount).put("memo", memo)
        .put("transferAccountId", transfer ?: JSONObject.NULL).put("categoryId", category ?: JSONObject.NULL)

    @Test fun aTextThatIsMoneyOfTwoKindsIsEnteredAsOneTransactionInParts() {
        val matured = sms("Your deposit has matured. SAR 501,900.00 was credited to account 3301")
        val texts = mapOf(matured.id to matured)
        val b = withDeposit()
        fun entered(vararg parts: JSONObject, kind: String = "income") = BankTextEntry.asked(JSONObject().put("textIds", JSONArray().put(matured.id)).put("accountId", "checking")
            .put("date", matured.receivedDate).put("amount", 501_900.0).put("kind", kind).put("payee", "Term deposit").put("parts", JSONArray(parts.toList())))
        // The money back is a transfer from the deposit's account; what's beyond it is income, not a transfer.
        val entry = entered(part(500_000.0, transfer = "deposit", memo = "Principal"), part(1_900.0, memo = "Profit"))
        assertEquals(listOf(50_000_000L, 190_000L), entry.parts.map { it.cents })
        assertNull(bankTextEntriesProblem(listOf(entry), texts, b))
        val t = bankTextsReading(listOf(entry), texts, "SAR").analysis.getJSONArray("items").getJSONObject(0).getJSONObject("transaction")
        // The whole is still the amount the text gives; only the parts are worked out.
        assertEquals(listOf("501900.00", "income"), listOf(t.getString("amount"), t.getString("kind")))
        assertTrue(t.isNull("transferAccountId") && t.isNull("categoryId"))
        val lines = t.getJSONArray("splits").let { a -> (0 until a.length()).map { a.getJSONObject(it) } }
        assertEquals(listOf("500000.00", "1900.00"), lines.map { it.getString("amount") })
        assertEquals("deposit", lines[0].getString("transferAccountId"))
        assertTrue(!lines[0].getBoolean("unassigned") && !lines[1].has("transferAccountId") && lines[1].getBoolean("unassigned"))
        // Money out in parts is negative in each, however it was written.
        assertEquals(listOf(-90_000L, -10_000L), bankTextParts(listOf(BankTextPart(90_000L), BankTextPart(-10_000L)), -100_000L).map { it.cents })
        // What can't be: parts that don't add up, one part, a plain transfer in parts, a transfer with its own account or one that isn't open, every part a transfer.
        assertNotNull(bankTextEntriesProblem(listOf(entered(part(500_000.0, transfer = "deposit"), part(1_000.0))), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(entered(part(501_900.0, transfer = "deposit"))), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(entered(part(500_000.0, transfer = "deposit"), part(1_900.0), kind = "transfer")), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(entered(part(500_000.0, transfer = "checking"), part(1_900.0))), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(entered(part(500_000.0, transfer = "nowhere"), part(1_900.0))), texts, b))
        assertNotNull(bankTextEntriesProblem(listOf(entered(part(500_000.0, transfer = "deposit"), part(1_900.0, transfer = "card"))), texts, b))
        // The card keeps the parts, and checks their accounts again as it's applied.
        assertEquals(entry, BankTextEntry.carded(JSONObject(entry.card().toString())))
        val card = JSONObject().put("action", "enter").put("entries", JSONArray().put(entry.card()))
        assertNull(bankTextsCardProblem(card, b))
        assertEquals(R.string.bank_texts_unknown_account, bankTextsCardProblem(card, budget()))
    }

    @Test fun chatListsTheReviewsRowsAndChangesOneToATransferWithItsProfit() {
        val matured = sms("Your deposit has matured. SAR 501,900.00 was credited to account 3301")
        val ride = sms("Purchase SAR 45.00 at Careem")
        val b = withDeposit()
        fun read(text: BankSms, amount: String, kind: String, payee: String, other: String? = null) = JSONObject().put("sourceIds", JSONArray().put(text.id)).put("disposition", "transaction")
            .put("skip", JSONObject.NULL).put("note", payee).put("unsure", JSONArray().put("kind"))
            .put("transaction", JSONObject().put("date", text.receivedDate).put("amount", amount).put("currency", "USD").put("writtenAmount", amount.trimStart('-')).put("payee", payee)
                .put("memo", "").put("kind", kind).put("accountRef", JSONObject.NULL).put("otherRef", JSONObject.NULL).put("categoryId", JSONObject.NULL)
                .put("accountId", "checking").put("transferAccountId", other ?: JSONObject.NULL))
        // The reading took the whole credit for a transfer from the deposit.
        val review = SmsBatch(listOf(matured, ride), JSONObject().put("items", JSONArray().put(read(matured, "501900.00", "transfer", "Term deposit", other = "deposit")).put(read(ride, "-45.00", "expense", "Careem"))),
            ticked = mapOf(0 to true, 1 to false))
        fun items(batch: SmsBatch) = batch.items(SmsAccounts.of(b, JSONArray(), batch.starts(b)))
        val listed = bankTextsReviewRows(review, app.urbudget.nativeapp.ui.smsSort(review, items(review), emptyMap()), emptySet(), b)
        assertEquals(listOf(matured.id, ride.id), listed.map { it.getString("textId") })
        assertEquals(listOf("501900.00", "transfer", "Term deposit", true), listed[0].let { listOf(it.getString("amount"), it.getString("kind"), it.getJSONObject("otherAccount").getString("name"), it.getBoolean("tickedToAdd")) })
        assertTrue(listed[0].getString("text").startsWith("Your deposit has matured") && !listed[1].getBoolean("tickedToAdd"))

        // Told what it was: income for the text's whole amount, the money back a transfer, the rest profit; and the ride is ticked.
        val changes = listOf(BankTextChange.of(JSONObject().put("textId", matured.id).put("kind", "income").put("parts", JSONArray().put(part(500_000.0, transfer = "deposit", memo = "Principal")).put(part(1_900.0, memo = "Profit")))),
            BankTextChange.of(JSONObject().put("textId", ride.id).put("payee", "Careem ride").put("add", true)))
        val (edited, touched) = bankTextsChange(review, b, changes)
        assertEquals(listOf(0, 1), touched)
        val after = items(edited)
        val t = after[0].transaction!!
        assertEquals(listOf("income", "501900.00", ""), listOf(t.getString("kind"), t.getString("amount"), t.optString("transferAccountId").takeUnless { t.isNull("transferAccountId") }.orEmpty()))
        assertEquals(listOf("500000.00" to "deposit", "1900.00" to ""), after[0].splits.map { it.getString("amount") to it.optString("transferAccountId") })
        assertTrue(after[0].unsure.isEmpty() && after[0].needs == null)
        assertEquals("Careem ride", after[1].transaction!!.getString("payee"))
        assertEquals(mapOf(0 to true, 1 to true), edited.ticked)
        // Added, the account takes the whole credit and the deposit gives up only what came back.
        assertEquals(mapOf("checking" to 50_190_000L, "deposit" to -50_000_000L), smsBalanceChanges(after, listOf(0 to "new")))
        // The review itself is untouched until the card is applied, and the card reads its rows back the same.
        assertEquals("transfer", review.analysis.getJSONArray("items").getJSONObject(0).getJSONObject("transaction").getString("kind"))
        assertEquals(changes, changes.map { BankTextChange.of(JSONObject(it.json().toString())) })

        // What can't be: a text with no row, a row twice, money in called an expense, a transfer in parts, an account that isn't open.
        fun refused(vararg change: JSONObject) = runCatching { bankTextsChange(review, b, change.map(BankTextChange::of)) }.exceptionOrNull() is IllegalArgumentException
        assertTrue(refused(JSONObject().put("textId", "sms:nowhere").put("payee", "X")))
        assertTrue(refused(JSONObject().put("textId", ride.id).put("payee", "X"), JSONObject().put("textId", ride.id).put("memo", "Y")))
        assertTrue(refused(JSONObject().put("textId", matured.id).put("kind", "expense")))
        assertTrue(refused(JSONObject().put("textId", matured.id).put("parts", JSONArray().put(part(500_000.0, transfer = "deposit")).put(part(1_900.0)))))
        assertTrue(refused(JSONObject().put("textId", ride.id).put("accountId", "nowhere")))
        assertTrue(refused(JSONObject().put("textId", ride.id).put("kind", "transfer").put("transferAccountId", "checking")))
        // A purchase becomes a transfer with another account, and loses the category it was read with.
        val paid = bankTextsChange(review, b, listOf(BankTextChange.of(JSONObject().put("textId", ride.id).put("kind", "transfer").put("transferAccountId", "card")))).first
        assertEquals("card", items(paid)[1].transaction!!.getString("transferAccountId"))
        // A card's accounts are checked again as it's applied.
        val card = JSONObject().put("action", "change").put("rows", JSONArray(changes.map { it.json() }))
        assertNull(bankTextsCardProblem(card, b))
        assertEquals(R.string.bank_texts_unknown_account, bankTextsCardProblem(card, budget()))
    }
}
