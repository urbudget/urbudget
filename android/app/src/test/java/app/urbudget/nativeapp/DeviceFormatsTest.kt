package app.urbudget.nativeapp

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test
import java.util.Locale

class DeviceFormatsTest {
    @Test fun aDatesOrderFollowsTheDevice() {
        assertEquals("mdy", DeviceFormats.dateFormat(Locale.US))
        assertEquals("dmy", DeviceFormats.dateFormat(Locale.UK))
        assertEquals("dmy", DeviceFormats.dateFormat(Locale.GERMANY))
        assertEquals("dmy", DeviceFormats.dateFormat(Locale.forLanguageTag("ar-SA")))
        assertEquals("iso", DeviceFormats.dateFormat(Locale.forLanguageTag("sv-SE")))
        assertEquals("iso", DeviceFormats.dateFormat(Locale.JAPAN))
    }

    @Test fun separatorsFollowTheDeviceWhereTheBudgetCanShowThem() {
        assertEquals("." to ",", DeviceFormats.separators(Locale.US))
        assertEquals("," to ".", DeviceFormats.separators(Locale.GERMANY))
        assertEquals("," to " ", DeviceFormats.separators(Locale.FRANCE))
        assertEquals("." to "'", DeviceFormats.separators(Locale.forLanguageTag("de-CH")))
        // Arabic-Indic marks aren't ones the budget writes: it falls back to the plain pair.
        assertEquals("." to ",", DeviceFormats.separators(Locale.forLanguageTag("ar-SA-u-nu-arab")))
    }

    @Test fun onlyABudgetWithoutFormatsGetsTheDevices() {
        val fresh = JSONObject().put("currency", "EUR")
        DeviceFormats.applyTo(fresh, Locale.GERMANY)
        assertEquals("dmy", fresh.getString("dateFormat"))
        assertEquals(",", fresh.getJSONObject("numberFormat").getString("decimal"))
        assertEquals(".", fresh.getJSONObject("numberFormat").getString("group"))
        val own = JSONObject().put("dateFormat", "iso").put("numberFormat", JSONObject().put("decimal", "."))
        DeviceFormats.applyTo(own, Locale.GERMANY)
        assertEquals("iso", own.getString("dateFormat"))
        assertEquals(".", own.getJSONObject("numberFormat").getString("decimal"))
    }
}
