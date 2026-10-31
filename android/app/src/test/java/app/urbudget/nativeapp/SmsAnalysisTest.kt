package app.urbudget.nativeapp

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class SmsAnalysisTest {
    private fun message(sender: String = "Bank", body: String = "Purchase card *1234") = BankSms(smsId(sender, 1000, body), sender, 1000, body)
    @Test fun exactMoneyRejectsRoundingZeroGroupingAndOverflow() {
        listOf("0", "-0.00", "1.001", "1,000", "1e2", "NaN", "10000000000.01", "--1").forEach { assertNull(it, smsCents(it)) }
        assertEquals(-1L, smsCents("-0.01")); assertEquals(1_000_000_000_000L, smsCents("10000000000"))
    }
    @Test fun fingerprintsKeepDistinctPurchasesAndNormalizeProviderFormatting() {
        assertEquals(smsId("BANK", 1000, "a\r\nb"), smsId("bank", 1000, "a\nb"))
        assertEquals(smsSenderKey("+966 (50) 123-4567"), smsSenderKey("+966501234567"))
        assertNotEquals(smsId("bank", 1000, "a"), smsId("bank", 1001, "a"))
        assertNotEquals(smsId("bank", 1000, "a"), smsId("other", 1000, "a"))
    }
    @Test fun draftsRestoreOnlyIfEveryOriginalMessageStillExists() {
        val sms=message(); val batch=SmsBatch(listOf(sms),JSONObject("{\"items\":[]}"))
        val saved=batch.saved(); assertFalse(saved.contains(sms.body))
        assertEquals(sms,SmsBatch.restore(saved,listOf(sms))!!.messages.single())
        assertNull(SmsBatch.restore(saved,emptyList())); assertNull(SmsBatch.restore("broken",listOf(sms)))
    }
    @Test fun rememberedAccountIsScopedToSenderAndLiteralCardReference() {
        val sms=message(); val item=JSONObject().put("sourceIds",JSONArray().put(sms.id)).put("accountReference","*1234")
        var mappings=smsRememberMapping(JSONArray(),item,listOf(sms),"main")
        mappings=smsRememberMapping(mappings,item,listOf(sms),"card")
        assertEquals(1,mappings.length()); assertEquals("card",mappings.getJSONObject(0).getString("accountId"))
        val other=message("Other"); item.put("sourceIds",JSONArray().put(other.id))
        mappings=smsRememberMapping(mappings,item,listOf(other),"other")
        assertEquals(2,mappings.length())
        item.put("accountReference","*9999")
        assertThrows(IllegalArgumentException::class.java) { smsRememberMapping(mappings,item,listOf(other),"main") }
    }
    @Test fun groupedQuestionCannotRememberDifferentSendersAsOneBankAccount() {
        val a=message(); val b=message("Other")
        val item=JSONObject().put("sourceIds",JSONArray().put(a.id).put(b.id)).put("accountReference","*1234")
        assertThrows(IllegalArgumentException::class.java) { smsRememberMapping(JSONArray(),item,listOf(a,b),"main") }
    }
}
