package app.urbudget.nativeapp

import android.content.Context
import android.content.res.Configuration
import android.view.View
import androidx.appcompat.app.AppCompatDelegate
import androidx.core.os.LocaleListCompat
import java.util.Locale

/**
 * The app's language (docs/LOCALIZATION.md): System, English or Arabic, kept as Android's per-app
 * language so the choice also shows in the system's app-language settings from Android 13. System
 * is the empty locale list; below Android 13 AppCompat stores the choice itself.
 */
object AppLanguage {
    const val SYSTEM = "system"

    /** What Settings shows as chosen: "system", "en" or "ar". */
    fun choice(): String {
        val chosen = AppCompatDelegate.getApplicationLocales()
        if (chosen.isEmpty) return SYSTEM
        return if (chosen[0]?.language == "ar") "ar" else "en"
    }

    /** Switches the app to "system", "en" or "ar". The activity updates in place (it handles locale changes itself). */
    fun choose(choice: String) {
        AppCompatDelegate.setApplicationLocales(if (choice == SYSTEM) LocaleListCompat.getEmptyLocaleList() else LocaleListCompat.forLanguageTags(choice))
    }

    /**
     * Brings the code below the screens up to [configuration], the activity's: the words reports and
     * messages use, month names, and amounts kept left to right inside right-to-left text. Returns
     * the language the interface resolved to, "en" or "ar", from the strings actually in use.
     */
    fun apply(application: Context, configuration: Configuration): String {
        val resources = application.createConfigurationContext(configuration).resources
        Words.source = ResourceWords(resources)
        val code = resources.getString(R.string.language_code)
        Money.useInterface(Locale.forLanguageTag(code), configuration.layoutDirection == View.LAYOUT_DIRECTION_RTL)
        keepWesternDigits()
        return code
    }

    /**
     * Arabic shows the same Western digits as the budget (docs/LOCALIZATION.md), including where
     * Android and Material format numbers themselves (the date picker's days). Android resets the
     * default locale on every configuration change, so this runs after each one.
     */
    fun keepWesternDigits() {
        val format = Locale.getDefault(Locale.Category.FORMAT)
        if (format.language != "ar" || format.getUnicodeLocaleType("nu") == "latn") return
        Locale.setDefault(Locale.Category.FORMAT, Locale.Builder().setLocale(format).setUnicodeLocaleKeyword("nu", "latn").build())
    }
}
