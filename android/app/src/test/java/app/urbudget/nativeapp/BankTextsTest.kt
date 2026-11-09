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
}
