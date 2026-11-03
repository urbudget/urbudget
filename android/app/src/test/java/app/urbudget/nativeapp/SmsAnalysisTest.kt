package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.smsEditable
import app.urbudget.nativeapp.ui.smsEdited
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.smsBankIds
import app.urbudget.nativeapp.ui.smsChanged
import app.urbudget.nativeapp.ui.smsOwnName
import app.urbudget.nativeapp.ui.smsSameTransaction
import app.urbudget.nativeapp.ui.smsSort
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SmsAnalysisTest {
    private fun message(sender: String = "Bank", body: String = "Purchase card *1234") = BankSms(smsId(sender, 1000, body), sender, 1000, body)
    private val accounts = listOf("visa" to "Bank Visa - 4417 (50,000)", "checking" to "ANB Checking - 0091 (SAR)", "savings" to "Savings")
    private fun item(sms: BankSms, transaction: JSONObject?, disposition: String = "transaction") = JSONObject()
        .put("sourceIds", JSONArray().put(sms.id)).put("disposition", disposition).put("skip", if (disposition == "ignore") "code" else JSONObject.NULL)
        .put("note", "Note").put("evidence", JSONArray()).put("unsure", JSONArray().put("date")).put("transaction", transaction ?: JSONObject.NULL)
    private fun purchase(ref: String?, amount: String? = "-12.34", kind: String = "expense", other: String? = null) = JSONObject()
        .put("date", "2026-10-28").put("amount", amount ?: JSONObject.NULL).put("currency", if (amount == null) "EUR" else "SAR").put("writtenAmount", "12.34")
        .put("payee", "Shop").put("memo", "").put("kind", kind).put("accountRef", ref ?: JSONObject.NULL).put("otherRef", other ?: JSONObject.NULL).put("categoryId", JSONObject.NULL)

    @Test fun exactMoneyRejectsRoundingZeroGroupingAndOverflow() {
        listOf("0", "-0.00", "1.001", "1,000", "1e2", "NaN", "10000000000.01", "--1").forEach { assertNull(it, smsCents(it)) }
        assertEquals(-1L, smsCents("-0.01")); assertEquals(1_000_000_000_000L, smsCents("10000000000"))
        assertEquals("-12.30", smsDecimal(-1230)); assertEquals("0.05", smsDecimal(5)); assertEquals(-1230L, smsCents(smsDecimal(-1230)))
    }
    @Test fun fingerprintsKeepDistinctPurchasesAndNormalizeProviderFormatting() {
        assertEquals(smsId("BANK", 1000, "a\r\nb"), smsId("bank", 1000, "a\nb"))
        assertEquals(smsSenderKey("+966 (50) 123-4567"), smsSenderKey("+966501234567"))
        assertNotEquals(smsId("bank", 1000, "a"), smsId("bank", 1001, "a"))
        assertNotEquals(smsId("bank", 1000, "a"), smsId("other", 1000, "a"))
    }
    @Test fun aPersonsNumberIsToldApartFromABanksNameOrShortCode() {
        assertTrue(smsPersonal("+966 50 123 4567")); assertFalse(smsPersonal("ANB")); assertFalse(smsPersonal("3323")); assertFalse(smsPersonal("AlRajhiBank"))
    }
    @Test fun draftsRestoreOnlyIfEveryOriginalMessageStillExistsAndKeepTheUsersChoices() {
        val sms = message(); val batch = SmsBatch(listOf(sms), JSONObject("{\"items\":[]}"), mapOf(0 to false), setOf(0))
        val saved = batch.saved(); assertFalse(saved.contains(sms.body))
        val waiting = { ids: List<String> -> listOf(sms).filter { it.id in ids } }
        val restored = SmsBatch.restore(saved, waiting)!!
        assertEquals(sms, restored.messages.single()); assertEquals(mapOf(0 to false), restored.ticked); assertEquals(setOf(0), restored.dismissed)
        assertNull(SmsBatch.restore(saved) { emptyList() }); assertNull(SmsBatch.restore("broken", waiting))
        // A draft from the earlier review has no version and isn't shown.
        assertNull(SmsBatch.restore(JSONObject().put("ids", JSONArray().put(sms.id)).put("analysis", JSONObject()).toString(), waiting))
    }
    @Test fun aReviewGrowsBatchByBatchAndEveryChoiceKeepsItsItem() {
        val first = message(); val second = BankSms(smsId("Bank", 2, "Code 1234"), "Bank", 2, "Code 1234")
        fun read(sms: BankSms) = JSONObject().put("items", JSONArray().put(JSONObject().put("sourceIds", JSONArray().put(sms.id)).put("disposition", "ignore")))
        val grown = SmsBatch(listOf(first), read(first), mapOf(0 to false), setOf(0)).plus(listOf(second), read(second))
        assertEquals(listOf(first, second), grown.messages)
        assertEquals(listOf(first.id, second.id), grown.analysis.rows("items").map { it.getJSONArray("sourceIds").getString(0) })
        assertEquals(mapOf(0 to false), grown.ticked); assertEquals(setOf(0), grown.dismissed)
    }
    @Test fun aCardIsTheRememberedAccountFirstThenTheOneAccountWhoseNameCarriesItsDigits() {
        val none = SmsAccounts(accounts, JSONArray())
        assertEquals("visa" to SmsAccountSource.Name, none.find("anb", "VISA Card:4417"))
        assertEquals("checking" to SmsAccountSource.Name, none.find("anb", "***0091"))
        assertNull(none.find("anb", "3358")); assertNull(none.find("anb", null)); assertNull(none.find("anb", "**12"))
        val remembered = SmsAccounts(accounts, smsRememberCard(JSONArray(), "anb", "3358", "savings"))
        assertEquals("savings" to SmsAccountSource.Remembered, remembered.find("ANB", "مدى أثير:3358"))
        assertNull(remembered.find("OtherBank", "3358"))
        // Two accounts carrying the digits can't be told apart; one open account is the only choice.
        assertNull(SmsAccounts(listOf("a" to "Visa 4417", "b" to "Old Visa 4417"), JSONArray()).find("anb", "4417"))
        assertNull(SmsAccounts(listOf("a" to "Visa (50,000)", "b" to "Savings"), JSONArray()).find("anb", "5000"))
        assertEquals("a" to SmsAccountSource.Only, SmsAccounts(listOf("a" to "Everyday"), JSONArray()).find("anb", null))
        // A remembered card whose account closed falls back to matching.
        assertNull(SmsAccounts(accounts.filter { it.first != "savings" }, smsRememberCard(JSONArray(), "anb", "3358", "savings")).find("anb", "3358"))
    }
    @Test fun rememberingACardReplacesItForItsSenderOnly() {
        var mappings = smsRememberCard(JSONArray(), "Bank", "*1234", "main")
        mappings = smsRememberCard(mappings, "BANK", "card 1234", "card")
        assertEquals(1, mappings.length()); assertEquals("card", mappings.getJSONObject(0).getString("accountId"))
        mappings = smsRememberCard(mappings, "Other", "*1234", "other")
        assertEquals(2, mappings.length())
        // The other side of a card's transfers is remembered beside its account, and replaced on its own.
        mappings = smsRememberCard(mappings, "Bank", "*1234", "checking", SmsForTransfer)
        mappings = smsRememberCard(mappings, "Bank", "1234", "savings", SmsForTransfer)
        assertEquals(3, mappings.length()); assertEquals("card", SmsAccounts(listOf("card" to "Visa", "savings" to "Savings"), mappings).find("Bank", "1234")?.first)
        assertThrows(IllegalArgumentException::class.java) { smsRememberCard(mappings, "Bank", " ", "main") }
    }
    @Test fun itemsResolveTheirAccountsAndSayWhatTheUserStillGives() {
        val messages = listOf("Visa 4417 SAR 12.34", "mada 3358 SAR 12.34", "Visa 4417 EUR 12.34", "***0091 to card 4417", "OTP 1234").map { message(body = it) }
        val analysis = JSONObject().put("items", JSONArray()
            .put(item(messages[0], purchase("4417"))).put(item(messages[1], purchase("3358"))).put(item(messages[2], purchase("4417", amount = null)))
            .put(item(messages[3], purchase("***0091", kind = "transfer", other = "4417"))).put(item(messages[4], null, "ignore")))
        val batch = SmsBatch(messages, analysis)
        val items = batch.items(SmsAccounts(accounts, JSONArray()))
        assertEquals(listOf(null, "account", "amount", null, null), items.map { it.needs })
        assertEquals(listOf("visa", "", "visa", "checking"), items.take(4).map { it.transaction!!.optString("accountId") })
        assertEquals("visa", items[3].transaction!!.getString("transferAccountId"))
        assertNotNull(items[1].card); assertNull(items[0].card); assertNull(items[4].transaction)
        assertEquals(setOf("date"), items[0].unsure)
        val resolved = batch.resolved(items).getJSONArray("items")
        assertEquals("visa", resolved.getJSONObject(0).getJSONObject("transaction").getString("accountId"))
        assertTrue(resolved.getJSONObject(1).getJSONObject("transaction").isNull("accountId"))
        assertTrue(resolved.getJSONObject(0).getJSONObject("transaction").isNull("transferAccountId"))
        // The draft itself keeps only what the user chose; matching is redone each time.
        assertFalse(batch.analysis.getJSONArray("items").getJSONObject(0).getJSONObject("transaction").has("accountId"))
    }
    @Test fun aCardTheUserDoesntAddIsSettledAndARowTheyWontAddWaitsForNothing() {
        val messages = listOf("mada 3358 SAR 12.34", "mada 3358 SAR 5.00", "Visa 4417 EUR 12.34", "Visa 4417 SAR 1.00").map { message(body = it) }
        val analysis = JSONObject().put("items", JSONArray().put(item(messages[0], purchase("3358"))).put(item(messages[1], purchase("3358")))
            .put(item(messages[2], purchase("4417", amount = null))).put(item(messages[3], purchase("4417"))))
        val skipping = SmsAccounts(accounts, smsRememberCard(JSONArray(), "Bank", "3358", SmsSkipCard))
        val batch = SmsBatch(messages, analysis)
        val items = batch.items(skipping)
        // The card's texts are settled like codes, with nothing left to choose.
        assertEquals(listOf("ignore", "ignore", "transaction", "transaction"), items.map { it.disposition })
        assertEquals(listOf("card", "card"), items.take(2).map { it.skip })
        assertNull(items[0].needs); assertNull(items[0].card); assertNull(skipping.find("Bank", "3358"))
        // A row still waiting for its amount that the user won't add waits for nothing.
        val sorted = smsSort(batch.copy(dismissed = setOf(2)), items, emptyMap())
        assertTrue(sorted.waiting.isEmpty())
        assertEquals(listOf(0, 1, 2), sorted.skipped.map { it.index }); assertEquals(listOf(3), sorted.ready.map { it.index })
        assertEquals(listOf(2), smsSort(batch, items, emptyMap()).waiting.map { it.index })
    }
    @Test fun theEditorsRecordComesBackAsTheUsersTransaction() {
        val sms = message(body = "Visa 4417 SAR 12.34"); val other = message(body = "Multiple purchases")
        val batch = SmsBatch(listOf(sms, other), JSONObject().put("items", JSONArray().put(item(sms, purchase("4417"))).put(item(other, null, "unclear").put("skip", JSONObject.NULL))), dismissed = setOf(1))
        val items = batch.items(SmsAccounts(accounts, JSONArray()))
        val record = smsEditable(items[0], "SAR")
        assertEquals(-1234L, record.getLong("amount")); assertEquals("visa", record.getString("accountId"))
        record.put("amount", -1500L).put("payee", "Shop & Co").getJSONArray("splits").getJSONObject(0).put("categoryId", "food")
        val budget = JSONObject().put("currency", "SAR").put("accounts", JSONArray())
        val edited = smsEdited(batch, items[0], record, budget).analysis.getJSONArray("items").getJSONObject(0)
        val t = edited.getJSONObject("transaction")
        assertEquals("-15.00", t.getString("amount")); assertEquals("15.00", t.getString("writtenAmount")); assertEquals("food", t.getString("categoryId"))
        assertEquals("visa", t.getString("accountId")); assertEquals("4417", t.getString("accountRef")); assertEquals(0, edited.getJSONArray("unsure").length())
        // Entering a message the assistant couldn't read makes it a transaction, ticked and no longer dismissed.
        val entered = smsEdited(batch, items[1], smsEditable(items[1], "SAR").put("accountId", "checking").put("amount", 5000L).put("payee", "Refund"), budget)
        val made = entered.analysis.getJSONArray("items").getJSONObject(1)
        assertEquals("transaction", made.getString("disposition")); assertTrue(made.isNull("skip"))
        assertEquals("income", made.getJSONObject("transaction").getString("kind")); assertEquals("SAR", made.getJSONObject("transaction").getString("currency"))
        assertEquals(true, entered.ticked[1]); assertFalse(1 in entered.dismissed)
    }
    @Test fun sendingIsAllowedOncePerProviderAndModel() {
        val connection = JSONObject().put("provider", "anthropic").put("model", "claude-x")
        val saved = smsConsent(connection)
        assertTrue(smsConsented(saved, connection))
        assertFalse(smsConsented(saved, JSONObject().put("provider", "anthropic").put("model", "claude-y")))
        assertFalse(smsConsented(saved, JSONObject().put("provider", "openai").put("model", "claude-x")))
        assertFalse(smsConsented(null, connection))
        // Given when reading only happened on a tap: asked again now the page reads by itself.
        assertFalse(smsConsented("""{"provider":"anthropic","model":"claude-x"}""", connection))
    }
    @Test fun anMmsIsReadOnlyWhenItIsNothingButText() {
        assertEquals("Purchase SAR 12.00", smsMmsText(listOf("application/smil" to null, "text/plain" to "Purchase ", "text/plain" to "SAR 12.00")))
        assertNull(smsMmsText(listOf("text/plain" to "Your statement", "image/jpeg" to null)))
        assertNull(smsMmsText(listOf("application/smil" to null)))
        assertNull(smsMmsText(emptyList()))
    }

    @Test fun aTextMatchesWhatsThereOnlyOnTheSameDayInTheSameAccountUnlessAnotherTextHasIt() {
        val text = JSONObject().put("date", "2026-10-29").put("accountId", "card").put("payee", "HungerStation")
        fun there(date: String = "2026-10-29", account: String = "card", ids: List<String> = listOf("bank:from-a-file")) =
            JSONObject().put("date", date).put("accountId", account).put("payee", "Adam Pharmacy").put("bankImportIds", JSONArray(ids))
        // The engine's id for the text "sms:earlier" imported into "card".
        val claimed = smsBankIds("card", listOf("sms:earlier"))
        assertEquals(setOf("bank:5df1d97bb416496bf74f6c926b501649419dea6d387197deba1c701345e464c5"), claimed)
        assertTrue(smsSameTransaction(text, there(), claimed))
        assertFalse(smsSameTransaction(text, there(date = "2026-10-31"), claimed))
        assertFalse(smsSameTransaction(text, there(account = "checking"), claimed))
        assertFalse(smsSameTransaction(text, there(ids = claimed.toList()), claimed))
        // A card payment the user entered from the current account is the same transfer as the card's text.
        val payment = JSONObject(text.toString()).put("transferAccountId", "checking")
        assertTrue(smsSameTransaction(payment, there(account = "checking").put("transferAccountId", "card"), claimed))
        assertFalse(smsSameTransaction(payment, there(account = "checking").put("transferAccountId", "savings"), claimed))
    }

    @Test fun aTransfersOtherAccountIsTheOneTheBudgetHasThenTheUsersChoiceThenTheAssistantsThenTheUsualOne() {
        fun moved(id: String, date: String, from: String, to: String, cents: Long) =
            JSONObject().put("id", id).put("date", date).put("accountId", from).put("transferAccountId", to).put("amount", cents)
        // The card paid from checking three times, the second entered on the card's side; a split part into savings.
        val sides = smsTransferSides(JSONObject().put("transactions", JSONArray().put(moved("a", "2026-08-31", "checking", "visa", -700_000))
            .put(moved("b", "2026-09-30", "visa", "checking", 955_784)).put(moved("c", "2026-10-28", "checking", "visa", -323_233))
            .put(JSONObject().put("id", "d").put("date", "2026-07-01").put("accountId", "checking").put("amount", -1000L)
                .put("splits", JSONArray().put(JSONObject().put("amount", -1000L).put("transferAccountId", "savings"))))))
        assertEquals(8, sides.size)
        assertTrue(SmsTransferSide("visa", "checking", 323_233, "2026-10-28") in sides)
        assertTrue(SmsTransferSide("savings", "checking", 1000, "2026-07-01") in sides)
        val plain = SmsAccounts(accounts, JSONArray(), transfers = sides)
        // Entered already: the other side of the transfer the budget has, a day or two apart.
        assertEquals("checking" to SmsAccountSource.Budget, plain.other("anb", "4417", null, null, "visa", 323_233, "2026-10-30"))
        // A new payment: the account that paid the card the last three times.
        assertEquals("checking" to SmsAccountSource.Usual, plain.other("anb", "4417", null, null, "visa", 50_000, "2026-11-03"))
        // Not for money the other way, nor for a text naming an account the phone can't find.
        assertNull(plain.other("anb", "4417", null, null, "visa", -50_000, "2026-11-03"))
        assertNull(plain.other("anb", "0091", "0044", null, "checking", -50_000, "2026-11-03"))
        // The assistant names it from what the text says it is; the user's choice for the card comes first.
        assertEquals("savings" to SmsAccountSource.Assistant, plain.other("anb", "0091", "0044", "SAVINGS", "checking", -50_000, "2026-11-03"))
        val chosen = SmsAccounts(accounts, smsRememberCard(JSONArray(), "anb", "4417", "savings", SmsForTransfer), transfers = sides)
        assertEquals("savings" to SmsAccountSource.Remembered, chosen.other("ANB", "VISA Card:4417", null, "ANB Checking - 0091 (SAR)", "visa", 50_000, "2026-11-03"))
        // The digits a text names still win, and the card itself is still the card.
        assertEquals("visa" to SmsAccountSource.Name, chosen.other("anb", "0091", "4417", "Savings", "checking", -50_000, "2026-11-03"))
        assertEquals("visa", chosen.find("anb", "4417")?.first)
        // A transfer the phone places goes to the ready list with what it was found from; one it can't waits under its card.
        val texts = listOf("Card Payment VISA Card:4417 SAR 500", "Murabaha 0044 from 0091 SAR 500").map { message(sender = "anb", body = it) }
        val batch = SmsBatch(texts, JSONObject().put("items", JSONArray().put(item(texts[0], purchase("4417", "500.00", "transfer"))).put(item(texts[1], purchase("0091", "-500.00", "transfer", "0044")))))
        val items = batch.items(SmsAccounts.of(JSONObject().put("accounts", JSONArray(accounts.map { (id, name) -> JSONObject().put("id", id).put("name", name) }))
            .put("transactions", JSONArray()), JSONArray(), emptyMap()))
        assertEquals(listOf("transfer", "transfer"), items.map { it.needs })
        assertNotEquals(items[0].transfer, items[1].transfer)
        val placed = batch.items(plain)
        assertEquals(listOf(null, "transfer"), placed.map { it.needs })
        assertEquals("checking", placed[0].transaction!!.getString("transferAccountId")); assertTrue(placed[0].otherGuessed)
    }

    @Test fun moneyFromANameTheUserMadeATransferIsATransferWithThatAccountNextTime() {
        val sms = message(sender = "anb", body = "حوالة واردة بـ:SAR 10129.47 من:AHMAD S HORANIEH إلى:0091")
        val income = purchase("0091", amount = "10129.47", kind = "income").put("payee", "Ahmad S Horanieh")
        val batch = SmsBatch(listOf(sms), JSONObject().put("items", JSONArray().put(item(sms, income))))
        val before = batch.items(SmsAccounts(accounts, JSONArray()))[0]
        // Made a transfer from savings in the editor: the bank's name for the account holder is what's remembered.
        val record = smsEditable(before, "SAR").put("transferAccountId", "savings").put("payee", "")
        assertEquals("Ahmad S Horanieh", smsOwnName(before, record))
        // A transfer has no payee or category for a format to learn.
        assertEquals(emptySet<String>(), smsChanged(before, record).first)
        val mappings = smsRememberCard(JSONArray(), sms.sender, "Ahmad S Horanieh", "savings", SmsForOwn)
        val after = batch.items(SmsAccounts(accounts, mappings))[0]
        assertEquals("transfer", after.transaction!!.getString("kind")); assertEquals("savings", after.transaction!!.getString("transferAccountId"))
        assertTrue(after.transaction!!.isNull("categoryId")); assertNull(after.needs); assertEquals(SmsAccountSource.Remembered, after.otherSource)
        // The name in a template's capitals is the same person; another sender's isn't; a transfer has none to remember.
        assertEquals("savings", SmsAccounts(accounts, mappings).own("ANB", "AHMAD  S HORANIEH"))
        assertNull(SmsAccounts(accounts, mappings).own("Other", "Ahmad S Horanieh"))
        assertNull(smsOwnName(after, smsEditable(after, "SAR")))
    }

    @Test fun aStartIsAWholeDayOrTheMomentNewMessagesBegan() {
        val day = SmsStart("date", "2026-10-03")
        val midnight = java.time.LocalDate.parse("2026-10-03").atStartOfDay(java.time.ZoneId.systemDefault()).toInstant().toEpochMilli()
        // A text at 01:22 on the start day is collected: "Since 3 Oct" holds all of 3 Oct.
        assertEquals(midnight, day.since()); assertEquals(day, SmsStart.of(midnight))
        assertEquals(SmsStart("now", "2026-10-03"), SmsStart.of(midnight + 70_638_000))
        assertEquals("all", SmsStart.of(0).mode); assertEquals(0L, SmsStart("all", "2026-10-03").since())
        assertEquals(1234L, SmsStart("now", "2026-10-03").since(now = 1234L))
        assertTrue(day.valid); assertFalse(SmsStart("date", java.time.LocalDate.now().plusDays(1).toString()).valid); assertFalse(SmsStart("date", "").valid)
    }
    @Test fun aNewBudgetCollectsNewMessagesAndOneWithHistoryAMonthButNotBeforeItBegins() {
        val today = java.time.LocalDate.parse("2026-11-02")
        fun budget(vararg dates: String, archived: Boolean = false) = JSONObject()
            .put("accounts", JSONArray().put(JSONObject().put("id", "a").put("archived", archived)))
            .put("transactions", JSONArray(dates.map { JSONObject().put("accountId", "a").put("date", it) }))
        // Balances entered as they are now: earlier texts are already in them.
        assertEquals(SmsStart("now", "2026-11-02"), SmsStart.suggested(budget(), today))
        assertEquals(SmsStart("now", "2026-11-02"), SmsStart.suggested(budget("2026-12-01"), today))
        assertEquals(SmsStart("now", "2026-11-02"), SmsStart.suggested(budget("2026-10-20", archived = true), today))
        assertEquals(SmsStart("date", "2026-10-20"), SmsStart.suggested(budget("2026-10-20", "2026-10-25"), today))
        assertEquals(SmsStart("date", "2026-10-03"), SmsStart.suggested(budget("2024-12-13"), today))
    }
    @Test fun aTransactionFromBeforeItsAccountsRecordsIsNotTickedToAdd() {
        val budget = JSONObject().put("accounts", JSONArray(listOf("visa", "checking", "savings").map { JSONObject().put("id", it) }))
            .put("transactions", JSONArray().put(JSONObject().put("accountId", "visa").put("date", "2026-10-20"))
                .put(JSONObject().put("accountId", "savings").put("transferAccountId", "checking").put("date", "2026-10-25")))
        val starts = smsStarts(budget, today = "2026-11-02")
        assertEquals(mapOf("visa" to "2026-10-20", "checking" to "2026-10-25", "savings" to "2026-10-25"), starts)
        // An account with nothing in it yet starts today.
        assertEquals("2026-11-02", smsStarts(budget.put("transactions", JSONArray()), today = "2026-11-02")["visa"])
        val messages = listOf("Visa 4417 SAR 12.34", "***0091 SAR 12.34", "Visa 4417 SAR 9.00").map { message(body = it) }
        fun on(date: String, ref: String) = purchase(ref).put("date", date)
        val batch = SmsBatch(messages, JSONObject().put("items", JSONArray().put(item(messages[0], on("2026-10-19", "4417")))
            .put(item(messages[1], on("2026-10-28", "***0091"))).put(item(messages[2], on("2026-10-20", "4417")))))
        val items = batch.items(SmsAccounts(accounts, JSONArray(), starts))
        assertEquals(listOf(true, false, false), items.map { it.early })
        assertEquals(listOf(false, true, true), items.map { it.tickedIn(batch) })
        // The user can still add it, and without starts nothing is held back.
        assertTrue(items[0].tickedIn(batch.copy(ticked = mapOf(0 to true))))
        assertFalse(batch.items(SmsAccounts(accounts, JSONArray())).any { it.early })
    }
    @Test fun collectingCarriesOnRightAfterTheLastTextHandled() {
        val handled = java.time.LocalDate.parse("2026-11-02").atTime(7, 47).atZone(java.time.ZoneId.systemDefault()).toInstant().toEpochMilli()
        val after = SmsStart.after(handled)
        assertEquals(SmsStart("after", "2026-11-02", handled), after)
        // Nothing handled is read again, and nothing after it is missed.
        assertEquals(handled + 1, after.since()); assertTrue(after.valid)
        assertEquals(after, SmsStart.of(handled + 1, handled))
        // Once a newer text is handled, the saved start reads back as the moment it is.
        assertEquals("now", SmsStart.of(handled + 1, handled + 60_000).mode)
    }
    @Test fun eachAccountSaysWhereItsBankDataAndReconcilingEnd() {
        fun t(account: String, date: String, status: String = "uncleared", bank: Boolean = false, transfer: String? = null, transferStatus: String? = null) = JSONObject()
            .put("accountId", account).put("date", date).put("status", status).put("bankImportIds", JSONArray(if (bank) listOf("bank:x") else emptyList()))
            .put("transferAccountId", transfer ?: JSONObject.NULL).put("transferStatus", transferStatus ?: JSONObject.NULL)
        val budget = JSONObject().put("accounts", JSONArray(listOf("anb" to "Everyday", "visa" to "Visa", "cash" to "Cash", "old" to "Old").map { (id, name) ->
            JSONObject().put("id", id).put("name", name).put("archived", id == "old") }))
            .put("transactions", JSONArray(listOf(
                t("anb", "2026-10-20", bank = true), t("anb", "2026-11-01", bank = true), t("anb", "2026-10-28", status = "reconciled"),
                t("anb", "2026-12-01", bank = true), t("cash", "2026-10-30"), t("anb", "2026-10-29", transfer = "visa", transferStatus = "reconciled"),
                t("old", "2026-10-31", bank = true))))
        val marks = smsAccountMarks(budget, today = "2026-11-02")
        // A scheduled transaction isn't bank data yet; an account with neither, or archived, isn't listed.
        assertEquals(listOf(SmsAccountMark("anb", "Everyday", "2026-11-01", "2026-10-28"), SmsAccountMark("visa", "Visa", null, "2026-10-29")), marks)
        assertEquals("2026-11-01", marks[0].day); assertEquals("2026-10-29", marks[1].day)
    }
    @Test fun readingSaysItsRequestsAndTimeOnceThereIsAPace() {
        assertEquals(SmsReadingSize(205, 2, null), smsReadingSize(205, null))
        // 700 ms a text, from the budget's last full request: 2,913 texts are 15 requests, about 34 minutes.
        assertEquals(SmsReadingSize(2_913, 15, 34), smsReadingSize(2_913, 700))
        assertEquals(1, smsReadingSize(3, 700).minutes); assertEquals(SmsReadingSize(0, 0, null), smsReadingSize(0, 700))
        // With learned formats a request carries more than it reads in full: the minutes say how long, the requests only without them.
        assertEquals(SmsReadingSize(631, 0, 4), smsReadingSize(631, 321, learned = true))
        assertEquals(SmsReadingSize(631, 4, null), smsReadingSize(631, null, learned = true))
    }
    @Test fun addingMovesEachBalanceByItsTransactionsAndATransfersOtherSide() {
        val messages = listOf("Visa 4417 SAR 12.34", "***0091 to card 4417", "mada 3358 SAR 12.34", "Visa 4417 SAR 5.00").map { message(body = it) }
        val batch = SmsBatch(messages, JSONObject().put("items", JSONArray().put(item(messages[0], purchase("4417")))
            .put(item(messages[1], purchase("***0091", kind = "transfer", other = "4417").put("amount", "-100.00")))
            .put(item(messages[2], purchase("3358"))).put(item(messages[3], purchase("4417").put("amount", "-5.00")))))
        val items = batch.items(SmsAccounts(accounts, JSONArray()))
        // A match and an item still needing an account change nothing.
        val changes = smsBalanceChanges(items, listOf(0 to "new", 1 to "new", 2 to "new", 3 to "match:t1"))
        assertEquals(mapOf("visa" to -1_234L + 10_000L, "checking" to -10_000L), changes)
        assertTrue(smsBalanceChanges(items, listOf(3 to "match:t1")).isEmpty())
    }
    @Test fun anAccountMadeForTheReviewHoldsNothingBackAndIsKeptWithIt() {
        val budget = JSONObject().put("accounts", JSONArray(listOf("checking", "cd").map { JSONObject().put("id", it) }))
            .put("transactions", JSONArray().put(JSONObject().put("accountId", "checking").put("date", "2026-10-01")))
        val sms = message(body = "***0091 to deposit 4321")
        val batch = SmsBatch(listOf(sms), JSONObject().put("items", JSONArray().put(item(sms, purchase("***0091", kind = "transfer", other = "4321").put("date", "2026-10-24")))))
        val open = listOf("checking" to "ANB 0091", "cd" to "ANB 4321")
        // A new CD has no transactions, so it starts today and the transfer into it would be held back...
        assertTrue(batch.items(SmsAccounts(open, JSONArray(), batch.starts(budget))).single().early)
        // ...unless it was made for this review, its balance from before these texts.
        val made = batch.copy(fresh = setOf("cd"))
        assertFalse(made.items(SmsAccounts(open, JSONArray(), made.starts(budget))).single().early)
        val restored = SmsBatch.restore(made.saved()) { ids -> listOf(sms).filter { it.id in ids } }!!
        assertEquals(setOf("cd"), restored.fresh); assertEquals(setOf("cd"), restored.only(listOf(0))!!.fresh)
    }
    @Test fun rowsAreNewestFirstAndANewAccountIsNamedForItsSenderAndDigits() {
        val messages = listOf("a", "b", "c").map { message(body = it) }
        val batch = SmsBatch(messages, JSONObject().put("items", JSONArray(listOf("2026-10-03", "2026-10-05", "2026-10-03").mapIndexed { i, day -> item(messages[i], purchase("4417").put("date", day)) })))
        val items = batch.items(SmsAccounts(accounts, JSONArray()))
        assertEquals(listOf(1, 2, 0), app.urbudget.nativeapp.ui.smsNewestFirst(items).map { it.index })
        assertEquals("ANB 4321", app.urbudget.nativeapp.ui.smsAccountName("anb", "***4321"))
        assertEquals("ANB Savings", app.urbudget.nativeapp.ui.smsAccountName(" anb ", "Savings"))
    }
    @Test fun aTemplatesRowsAreCountedAndWhatTheUserChangesInThemIsNamed() {
        val sms = message(body = "Visa 4417 SAR 12.34"); val code = message(body = "Code 1234")
        val batch = SmsBatch(listOf(sms, code), JSONObject().put("items", JSONArray()
            .put(item(sms, purchase("4417")).put("template", "t1").put("raw", "SHOP")).put(item(code, null, "ignore"))))
        assertEquals(1, batch.templated)
        val read = batch.items(SmsAccounts(accounts, JSONArray()))[0]
        assertEquals("t1" to "SHOP", read.template to read.raw)
        assertEquals(emptySet<String>() to null, app.urbudget.nativeapp.ui.smsChanged(read, smsEditable(read, "SAR")))
        val record = smsEditable(read, "SAR").put("payee", "Shop & Co").put("amount", -1500L)
        record.getJSONArray("splits").getJSONObject(0).put("categoryId", "food")
        assertEquals(setOf("payee", "amount", "category") to "food", app.urbudget.nativeapp.ui.smsChanged(read, record))
    }

    @Test fun aRequestTakesWhatTheAssistantCanReadAndTextsKeptOnThePhoneAreCountedApart() {
        assertEquals(SmsBatchSize, smsRequestSize(null))
        // Junk costs the assistant nothing; up to 400 reviewed and 200 read in full go in one request.
        assertEquals(1_000, smsRequestSize(List(1_000) { "junk" }))
        assertEquals(600, smsRequestSize(List(400) { "review" } + List(205) { "read" }))
        assertEquals(400, smsRequestSize(List(401) { "review" }))
        assertEquals(250, smsRequestSize(List(50) { "junk" } + List(201) { "read" }))
        val code = message(body = "Code 1234")
        val batch = SmsBatch(listOf(code), JSONObject().put("items", JSONArray().put(item(code, null, "ignore").put("template", "filter:code").put("kept", true))))
        assertEquals(0 to 1, batch.templated to batch.kept)
        assertTrue(batch.items(SmsAccounts(accounts, JSONArray()))[0].kept)
    }

    @Test fun onlySkipFormatsConfirmedOverTwoRequestsAreTrustedAndASenderDriftsWhenItsTextsTurnNew() {
        val skip = JSONObject().put("outcome", JSONObject().put("disposition", "ignore").put("skip", "rewards"))
        val money = JSONObject().put("outcome", JSONObject().put("disposition", "transaction").put("skip", JSONObject.NULL))
        val confirmed = JSONObject().put("agreed", 1).put("confirmed", 2).put("requests", 1)
        assertTrue(smsTrusted(skip, confirmed))
        assertFalse(smsTrusted(money, confirmed))
        // Written and confirmed in one request isn't enough; nor is any correction, by the assistant or the user.
        assertFalse(smsTrusted(skip, JSONObject().put("agreed", 3)))
        assertFalse(smsTrusted(skip, JSONObject(confirmed.toString()).put("corrected", JSONObject().put("skip", 1))))
        assertFalse(smsTrusted(skip, JSONObject(confirmed.toString()).put("edited", JSONObject().put("payee", 1))))
        assertEquals(66, smsAccuracy(JSONObject().put("filled", 3).put("confirmed", 2)))
        assertNull(smsAccuracy(JSONObject()))
        // Six of the last twenty new, after twenty known: the bank changed its wording.
        val changed = List(14) { true } + List(6) { false }
        assertTrue(smsDrifting(List(20) { true } + changed))
        assertFalse(smsDrifting(List(20) { true } + List(15) { true } + List(5) { false }))
        // A sender that was always partly new, or too new to tell, isn't drifting.
        assertFalse(smsDrifting(List(20) { it % 7 != 0 } + changed))
        assertFalse(smsDrifting(List(4) { true } + changed))
    }
    @Test fun aReviewKeepsWhatStillWaitsAndALaterStartOrARemovedSenderTakesTheRestOut() {
        val texts = listOf(BankSms(smsId("anb", 1, "a"), "anb", 1_000, "a"), BankSms(smsId("anb", 2, "b"), "anb", 2_000, "b"),
            BankSms(smsId("Other", 3, "c"), "Other", 3_000, "c"), BankSms(smsId("ANB", 4, "d"), "ANB", 4_000, "d"))
        fun of(vararg sms: BankSms) = JSONObject().put("sourceIds", JSONArray(sms.map { it.id })).put("disposition", "ignore")
        val batch = SmsBatch(texts, JSONObject().put("v", 2).put("items", JSONArray().put(of(texts[0])).put(of(texts[1], texts[2])).put(of(texts[3]))),
            ticked = mapOf(0 to true, 2 to false), dismissed = setOf(1))
        val rest = batch.only(listOf(2, 1))!!
        assertEquals(listOf(texts[1], texts[2], texts[3]), rest.messages)
        assertEquals(mapOf(1 to false), rest.ticked); assertEquals(setOf(0), rest.dismissed)
        assertEquals(2, rest.analysis.getJSONArray("items").length()); assertEquals(2, rest.analysis.getInt("v"))
        assertNull(batch.only(emptyList()))
        // An item stays while one of its texts is from the start on.
        assertEquals(listOf(texts[1], texts[2], texts[3]), batch.from(2_500)!!.messages)
        assertEquals(listOf(texts[3]), batch.from(3_500)!!.messages); assertNull(batch.from(5_000))
        // Removing a sender takes out every item with one of its texts.
        assertEquals(listOf(texts[0], texts[3]), batch.fromSenders(setOf("anb"))!!.messages)
        // What's kept restores from the texts still waiting.
        assertEquals(rest.messages, SmsBatch.restore(rest.saved()) { ids -> texts.filter { it.id in ids } }!!.messages)
    }
}
