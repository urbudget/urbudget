package app.urbudget.nativeapp

import app.urbudget.nativeapp.ui.smsEditable
import app.urbudget.nativeapp.ui.smsEdited
import app.urbudget.nativeapp.ui.rows
import app.urbudget.nativeapp.ui.smsBankIds
import app.urbudget.nativeapp.ui.smsSameTransaction
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
    }
}
