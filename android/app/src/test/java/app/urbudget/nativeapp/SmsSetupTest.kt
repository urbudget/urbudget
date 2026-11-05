package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.rows
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SmsSetupTest {
    private var clock = 1_793_000_000_000L
    private fun sms(body: String, sender: String = "Chase"): BankSms { clock += 600_000; return BankSms(smsId(sender, clock, body), sender, clock, body) }
    private fun row(sms: BankSms, amount: String, ref: String?, kind: String = "expense", other: String? = null, balance: String? = null, payee: String = "Shop") = sms to JSONObject()
        .put("sourceIds", JSONArray().put(sms.id)).put("disposition", "transaction").put("skip", JSONObject.NULL).put("note", payee).put("evidence", JSONArray()).put("unsure", JSONArray())
        .put("transaction", JSONObject().put("date", sms.receivedDate).put("amount", amount).put("currency", "USD").put("writtenAmount", amount.removePrefix("-"))
            .put("payee", payee).put("memo", "").put("kind", kind).put("accountRef", ref ?: JSONObject.NULL).put("otherRef", other ?: JSONObject.NULL)
            .put("categoryId", JSONObject.NULL).put("balance", balance ?: JSONObject.NULL))
    private fun review(vararg rows: Pair<BankSms, JSONObject>) = SmsBatch(rows.map { it.first }, JSONObject().put("items", JSONArray(rows.map { it.second })))
    private fun budget(vararg accounts: Triple<String, String, String>) = JSONObject().put("currency", "USD").put("transactions", JSONArray())
        .put("accounts", JSONArray(accounts.map { (id, name, type) -> JSONObject().put("id", id).put("name", name).put("kind", AccountTypes.kindFor(type)).put("accountType", type).put("archived", false) }))
    private val types = mapOf("creditCard" to "Credit card", "savings" to "Savings")
    private fun candidates(batch: SmsBatch, budget: JSONObject = budget(), mappings: JSONArray = JSONArray()) =
        smsSetupCandidates(batch, batch.items(SmsAccounts.of(budget, mappings, batch.starts(budget))))
    private fun suggest(batch: SmsBatch, budget: JSONObject = budget()) = candidates(batch, budget).let { it to smsSetupSuggestions(it, budget) { t -> types[t] ?: t } }
    /** What one text's card reads as. */
    private fun kind(body: String, ref: String, kind: String = "expense", amount: String = "-12.00", sender: String = "Bank") =
        smsSetupKind(candidates(review(row(sms(body, sender), amount, ref, kind))).single())

    /** A US bank's month: a checking account, its debit card, a credit card it pays and savings it moves money to. */
    private fun month(): SmsBatch {
        val texts = listOf(
            row(sms("Chase: debit card ending 7788 was used for $45.00 at TARGET on 10/02."), "-45.00", "7788", payee = "Target"),
            row(sms("Chase: $1,200.00 direct deposit to checking account ending 3301. Available balance $2,455.00"), "1200.00", "3301", "income", balance = "2455.00", payee = "Acme"),
            row(sms("Chase: You made a $25.62 transaction with UBER on your Sapphire Visa credit card ending 4512."), "-25.62", "4512", payee = "Uber"),
            row(sms("Chase: Payment of $300.00 to your credit card ending 4512 from checking ***3301 posted."), "-300.00", "3301", "transfer", "4512", payee = "Card payment"),
            row(sms("Chase: debit card ending 7788 was used for $15.00 at STARBUCKS. Balance $2,140.00"), "-15.00", "7788", balance = "2140.00", payee = "Starbucks"),
            row(sms("Chase: $500.00 transferred from checking ***3301 to savings ***9900."), "-500.00", "3301", "transfer", "9900", payee = "Savings"),
            row(sms("Chase: debit card ending 7788 was used for $10.00 at CVS."), "-10.00", "7788", payee = "CVS"),
        )
        return review(*texts.toTypedArray())
    }

    @Test fun anEmptyBudgetsTextsAreSetUpAsTheAccountsTheyNameWithADebitCardInItsAccount() {
        val batch = month()
        val (found, suggested) = suggest(batch)
        assertEquals(listOf("3301", "7788", "4512", "9900"), found.map { it.ref })
        assertEquals(listOf(3, 3, 2, 1), found.map { it.texts })
        // The credit card is named by the payment that went to it; the savings only by the transfer.
        assertEquals(listOf(2), found[2].items.map { it.index })
        assertEquals(listOf(3), found[2].via.map { it.index })
        assertEquals(listOf(5), found[3].via.map { it.index })
        val key = { ref: String -> smsCardKey("Chase", ref) }
        assertEquals(SmsSetupPlan(SmsSetupPlan.Create, "CHASE 3301", "checking"), suggested[key("3301")])
        assertEquals(SmsSetupPlan(SmsSetupPlan.Join, "CHASE 7788", "checking", joins = key("3301")), suggested[key("7788")])
        assertEquals(SmsSetupPlan(SmsSetupPlan.Create, "CHASE Visa 4512", "creditCard"), suggested[key("4512")])
        assertEquals(SmsSetupPlan(SmsSetupPlan.Create, "CHASE Savings 9900", "savings"), suggested[key("9900")])

        // The checking account's balance before the texts: the card's alert gave 2,140.00 after 45 + 15
        // spent, 1,200 in and 300 out; the 500 after it doesn't count.
        val adds = { item: SmsItem -> item.tickedIn(batch) }
        val plans = smsSetupPlans(found, emptyMap(), suggested, emptySet())
        val opening = smsOpening(smsSetupMembers(key("3301"), found, plans), adds)!!
        assertEquals(SmsOpening(130_000L, 214_000L, batch.messages[4].receivedDate), opening)
        assertEquals(listOf(key("3301"), key("7788")), smsSetupMembers(key("3301"), found, plans).map { it.key })

        var next = 0
        val changes = smsSetupChanges(found, plans, adds) { "n${++next}" }
        assertEquals(listOf("n1" to 130_000L, "n2" to 0L, "n3" to 0L), changes.accounts.map { it.getString("id") to it.getLong("openingBalance") })
        assertEquals(listOf("cash", "credit", "cash"), changes.accounts.map { it.getString("kind") })
        assertEquals(listOf("3301" to "n1", "7788" to "n1", "4512" to "n2", "9900" to "n3"), changes.remember.map { it.ref to it.account })
        assertTrue(changes.spending.isEmpty())

        // Once made and remembered, every text has its account and transfer, and the checking
        // account ends where the bank's alert puts it, less what came after it.
        val made = budget().put("accounts", JSONArray(changes.accounts))
        val mappings = changes.remember.fold(JSONArray()) { kept, r -> smsRememberCard(kept, r.sender, r.ref, r.account, r.kind) }
        val after = batch.copy(fresh = changes.accounts.map { it.getString("id") }.toSet()).items(SmsAccounts.of(made, mappings, emptyMap()))
        assertTrue(after.all { it.needs == null })
        assertEquals("n2", after[3].transaction!!.getString("transferAccountId")); assertEquals("n3", after[5].transaction!!.getString("transferAccountId"))
        val moved = smsBalanceChanges(after, after.map { it.index to "new" })
        assertEquals(214_000L - 50_000L - 1_000L, 130_000L + moved.getValue("n1"))
        assertEquals(-2_562L + 30_000L, moved.getValue("n2"))
        assertTrue(candidates(batch, made, mappings).isEmpty())
    }

    @Test fun theUsersAnswersWinAndOneJoiningAnAccountNotMadeIsMadeItself() {
        val batch = month()
        val (found, suggested) = suggest(batch)
        val key = { ref: String -> smsCardKey("Chase", ref) }
        val skipChecking = mapOf(key("3301") to suggested.getValue(key("3301")).copy(action = SmsSetupPlan.Skip))
        val plans = smsSetupPlans(found, skipChecking, suggested, emptySet())
        assertEquals(SmsSetupPlan.Create, plans.getValue(key("7788")).action)
        // Savings not tracked: the transfer to it is spending from checking.
        val untracked = smsSetupPlans(found, mapOf(key("9900") to suggested.getValue(key("9900")).copy(action = SmsSetupPlan.Skip)), suggested, emptySet())
        val changes = smsSetupChanges(found, untracked, { true }) { "id" }
        assertEquals(listOf(5), changes.spending)
        assertEquals(SmsSkipCard, changes.remember.single { it.ref == "9900" }.account)
        val edited = batch.edit(5, ::smsUntracked).analysis.rows("items")[5].getJSONObject("transaction")
        assertEquals("expense", edited.getString("kind")); assertTrue(edited.isNull("otherRef"))
        // A balance the user entered stands; a credit card starts at nothing owed.
        val entered = smsSetupPlans(found, mapOf(key("3301") to suggested.getValue(key("3301")).copy(opening = 99_00L)), suggested, emptySet())
        assertEquals(99_00L, smsSetupChanges(found, entered, { true }) { "id" }.accounts.first().getLong("openingBalance"))
        assertEquals(0L, smsSetupBalance(SmsSetupPlan(SmsSetupPlan.Create, type = "creditCard"), SmsOpening(5_000_00L, 5_000_00L, "2026-11-01")))
        // The answers are kept with the review's draft.
        val answered = batch.copy(setup = entered)
        val restored = SmsBatch.restore(answered.saved()) { ids -> batch.messages.filter { it.id in ids } }!!
        assertEquals(entered, restored.setup)
    }

    @Test fun aDebitCardJoinsItsSendersOneAccountOrTheOneInTheBudgetNamedForItButAPlainCardDoesnt() {
        val two = review(row(sms("Chase: debit card ending 7788 was used for $45.00 at TARGET."), "-45.00", "7788"),
            row(sms("Chase: $10.00 deposited to checking account ending 3301."), "10.00", "3301", "income"),
            row(sms("Chase: $20.00 deposited to checking account ending 3302."), "20.00", "3302", "income"))
        assertEquals(SmsSetupPlan.Create, suggest(two).second.getValue(smsCardKey("Chase", "7788")).action)
        val alone = review(row(sms("Chase: debit card ending 7788 was used for $45.00 at TARGET."), "-45.00", "7788"),
            row(sms("Chase: debit card ending 7788 was used for $5.00 at CVS."), "-5.00", "7788"))
        val budget = budget(Triple("main", "Chase Checking", "checking"), Triple("visa", "Chase Visa", "creditCard"))
        assertEquals(SmsSetupPlan(SmsSetupPlan.Join, "CHASE 7788", "checking", joins = "main"), suggest(alone, budget).second.getValue(smsCardKey("Chase", "7788")))
        // A card that doesn't say it's a debit card is one of its own, beside the account.
        val plain = review(row(sms("Rs.2,400.00 spent on card XX5678 at SWIGGY.", "BANK-IN"), "-2400.00", "XX5678"),
            row(sms("Rs.120.00 debited from A/c XX1234 to VPA zomato@hdfcbank.", "BANK-IN"), "-120.00", "XX1234"))
        val (found, suggested) = suggest(plain)
        assertEquals(listOf(SmsSetupPlan(SmsSetupPlan.Create, "BANK-IN Credit card 5678", "creditCard"), SmsSetupPlan(SmsSetupPlan.Create, "BANK-IN 1234", "checking")),
            found.sortedBy { it.ref }.reversed().map { suggested.getValue(it.key) })
    }

    @Test fun whatACardIsComesFromTheWordsByItsDigitsInManyLanguages() {
        assertEquals(SmsSetupKind("creditCard", true, null), kind("Kartenzahlung 12,50 EUR bei REWE, Kreditkarte ...4421", "4421"))
        assertEquals(SmsSetupKind("checking", true, null), kind("Compra con tarjeta de débito *5566 por 25,00 EUR en MERCADONA", "5566"))
        assertEquals(SmsSetupKind("checking", true, "mada"), kind("شراء عبر بطاقة مدى رقم 3875 بمبلغ 45 ريال", "3875"))
        assertEquals(SmsSetupKind("savings", false, null), kind("تحويل إلى حساب التوفير 7714", "7714", "transfer"))
        assertEquals(SmsSetupKind("checking", false, null), kind("Rs.2,500.00 debited from A/c XX1234 on 29-Oct-26 to VPA swiggy@icici", "XX1234", sender = "VM-HDFCBK-S"))
        assertEquals(SmsSetupKind("creditCard", true, null), kind("您尾号1234的信用卡11月02日消费人民币128.50元", "1234"))
        assertEquals(SmsSetupKind("creditCard", true, "Mastercard"), kind("Mastercard 7001 spent $12.00 at SHELL", "7001"))
        // A card that receives payments is a credit card, and so is one that says nothing more; "Ramadan" isn't mada.
        assertEquals("creditCard", kind("Payment received on card 4512: SAR 3,232.33. Thank you", "4512", "transfer", "3232.33").type)
        assertEquals(SmsSetupKind("creditCard", true, null), kind("Rs.2,400.00 spent on card XX5678 at SWIGGY on 03-Nov-26.", "XX5678"))
        assertEquals(SmsSetupKind("creditCard", true, null), kind("Ramadan offer on card 9090: SAR 20 off", "9090"))
        assertEquals(SmsSetupKind("checking", false, null), kind("Your account 4455 was charged 3,000 for rent", "4455"))
        assertEquals("visa card:" to "", smsWordsBy("Online Purchase\nApple Pay\nVISA Card:1234\nAmount:SAR 5", "1234"))
        assertEquals("您尾号" to "的信用卡", smsWordsBy("您尾号١٢٣٤的信用卡11月02日", "1234"))
        assertEquals("HDFCBK", smsSenderName("VM-HDFCBK-S")); assertEquals("ICICIT", smsSenderName("JD-ICICIT")); assertEquals("M-PESA", smsSenderName("M-Pesa"))
    }

    @Test fun depositsTheBankNumbersAfreshAreOneAccountAndADebitCardJoinsTheMainAccount() {
        fun placement(n: Int) = row(sms("Transfer to your investment account\nFrom: 0026\nAmount: 500,000.00 SAR\nTo: 00${55 + n}\nOn: 15/09/26"), "-500000.00", "0026", "transfer", "00${55 + n}", payee = "Investment")
        val batch = review(*(0 until 5).map(::placement).toTypedArray(),
            row(sms("Salary\nTo: 0026\nAmount: 35,000.00 SAR"), "35000.00", "0026", "income"),
            row(sms("Transfer to account 0033\nAmount: 10.00 SAR"), "10.00", "0033", "income"),
            row(sms("PoS Purchase\nAmount: SAR 42.30\nmada: 6875\nAt: JASMINE F"), "-42.30", "6875"),
            row(sms("PoS Purchase\nAmount: SAR 6.00\nmada: 6875\nAt: MAGHSALAT"), "-6.00", "6875"),
            *(0 until 4).map { row(sms("Payment from account 0026 for rent $it\nAmount: SAR ${100 + it}.00"), "-${100 + it}.00", "0026", payee = "Rent") }.toTypedArray())
        val (found, suggested) = suggest(batch)
        val deposits = found.single { it.refs.size > 1 }
        assertEquals((0 until 5).map { "00${55 + it}" }, deposits.refs)
        assertTrue(smsSetupOwned(deposits, smsSetupKind(deposits)))
        assertEquals(SmsSetupPlan(SmsSetupPlan.Create, "CHASE Savings", "savings"), suggested.getValue(deposits.key))
        // The card's account: the one with most of the sender's texts on it, not the deposits'.
        assertEquals(smsCardKey("Chase", "0026"), suggested.getValue(smsCardKey("Chase", "6875")).joins)
        val changes = smsSetupChanges(found, smsSetupPlans(found, emptyMap(), suggested, emptySet()), { true }) { java.util.UUID.randomUUID().toString() }
        assertEquals(5, changes.remember.count { r -> r.ref in deposits.refs })
        assertEquals("Transfer to your investment account From: # Amount: # SAR To: # On: #", smsShape("Transfer to your investment account\nFrom: 0026\nAmount: 500,000.00 SAR\nTo: 0055\nOn: 15/09/26"))
    }

    @Test fun twoTextsAboutOneTransferAreAddedOnce() {
        val budget = budget(Triple("checking", "Checking 3301", "checking"), Triple("visa", "Visa 4512", "creditCard"))
        val batch = review(row(sms("Payment of $300.00 to your credit card ending 4512 from checking ***3301"), "-300.00", "3301", "transfer", "4512"),
            row(sms("Payment received on card 4512: $300.00"), "300.00", "4512", "transfer", "3301"),
            row(sms("Payment of $300.00 to your credit card ending 4512 from checking ***3301"), "-300.00", "3301", "transfer", "4512"))
        val items = batch.items(SmsAccounts.of(budget, JSONArray(), emptyMap()))
        // The card's text is the first payment's other side; the second payment is a payment of its own.
        assertEquals(listOf("transaction", "ignore", "transaction"), items.map { it.disposition })
        assertEquals("pair", items[1].skip)
        assertEquals(mapOf("checking" to -60_000L, "visa" to 60_000L), smsBalanceChanges(items, items.filter { it.tickedIn(batch) }.map { it.index to "new" }))
    }

    @Test fun aDepositPlacedAndOneMaturedAreOneAccountAndABareNumberIsntTracked() {
        val batch = review(
            row(sms("Transfer of SAR 500,000.00 to your term deposit account no. 0060 from 0026"), "-500000.00", "0026", "transfer", "0060"),
            row(sms("Transfer of SAR 501,695.40 from your term deposit account no. 0055 to 0026"), "501695.40", "0026", "transfer", "0055"),
            row(sms("Transfer of SAR 501,695.40 from your term deposit account no. 0056 to 0026"), "501695.40", "0026", "transfer", "0056"),
            row(sms("Local transfer\nTo: 6902***\nAmount: SAR 100,000.00\nFrom: 0026"), "-100000.00", "0026", "transfer", "6902***"),
            // Most of an account's texts call it an account; one mention of investments by its digits doesn't make it savings.
            row(sms("Salary to account 0026: SAR 35,000.00"), "35000.00", "0026", "income"),
            row(sms("Rent from account 0026: SAR 5,000.00"), "-5000.00", "0026"),
            row(sms("From your investment portfolio to 0026: SAR 10.00"), "10.00", "0026", "income"))
        val (found, suggested) = suggest(batch)
        val deposits = found.single { it.refs.size > 1 }
        assertEquals(listOf("0055", "0056", "0060"), deposits.refs)
        assertEquals(SmsSetupPlan(SmsSetupPlan.Create, "CHASE Savings", "savings"), suggested.getValue(deposits.key))
        assertEquals(SmsSetupPlan.Skip, suggested.getValue(smsCardKey("Chase", "6902***")).action)
        assertEquals("checking", suggested.getValue(smsCardKey("Chase", "0026")).type)
    }

    @Test fun aWalletsTextsThatNameNoCardAreOneAccountRememberedForTheSender() {
        val batch = review(row(sms("QK12AB34CD Confirmed. Ksh500.00 sent to JOHN DOE 0712345678 on 3/11/26 at 2:15 PM. New M-PESA balance is Ksh1,234.00.", "MPESA"), "-500.00", null, balance = "1234.00"),
            row(sms("QK12AB34CE Confirmed. You have received Ksh2,000.00 from JANE DOE on 4/11/26. New M-PESA balance is Ksh3,234.00.", "MPESA"), "2000.00", null, "income", balance = "3234.00"))
        val (found, suggested) = suggest(batch)
        val wallet = found.single()
        assertNull(wallet.ref); assertEquals(smsCardKey("MPESA", SmsAnyCard), wallet.key)
        assertEquals(SmsSetupPlan(SmsSetupPlan.Create, "MPESA", "cash"), suggested.getValue(wallet.key))
        val changes = smsSetupChanges(found, suggested, { true }) { "w" }
        assertEquals(1_734_00L, changes.accounts.single().getLong("openingBalance"))
        // Remembered for the sender's texts that name no card, whatever else the budget has.
        val mappings = smsRememberCard(JSONArray(), "MPESA", SmsAnyCard, "w")
        val accounts = SmsAccounts(listOf("w" to "MPESA", "bank" to "Bank"), mappings)
        assertEquals("w" to SmsAccountSource.Remembered, accounts.find("mpesa", null))
        assertNull(accounts.find("other", null))
    }

    @Test fun twoCardsTheBudgetDoesntKnowAreSetUpRatherThanPutInItsOneAccount() {
        val one = budget(Triple("main", "Everyday", "checking"))
        val single = review(row(sms("Card 1111 spent $5.00 at CVS"), "-5.00", "1111"))
        assertEquals(SmsAccountSource.Only, single.items(SmsAccounts.of(one, JSONArray(), emptyMap())).single().accountSource)
        val both = review(row(sms("Card 1111 spent $5.00 at CVS"), "-5.00", "1111"), row(sms("Card 2222 spent $7.00 at CVS"), "-7.00", "2222"))
        assertTrue(both.items(SmsAccounts.of(one, JSONArray(), emptyMap())).all { it.needs == "account" })
        assertEquals(listOf("1111", "2222"), candidates(both, one).map { it.ref })
        // A card the user said not to add isn't one of them.
        val skipped = smsRememberCard(JSONArray(), "Chase", "2222", SmsSkipCard)
        assertEquals(SmsAccountSource.Only, both.items(SmsAccounts.of(one, skipped, emptyMap())).first().accountSource)
    }
}
