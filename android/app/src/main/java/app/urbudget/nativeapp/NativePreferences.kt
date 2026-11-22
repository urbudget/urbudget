package app.urbudget.nativeapp

import android.content.Context

class NativePreferences(context: Context) {
    private val preferences = context.getSharedPreferences("native-preferences", Context.MODE_PRIVATE)
    var automaticSync: Boolean
        get() = preferences.getBoolean("automaticSync", true)
        set(value) { preferences.edit().putBoolean("automaticSync", value).apply() }
    var automaticSchedules: Boolean
        get() = preferences.getBoolean("automaticSchedules", true)
        set(value) { preferences.edit().putBoolean("automaticSchedules", value).apply() }
    var theme: String
        get() = preferences.getString("theme", "system") ?: "system"
        set(value) { preferences.edit().putString("theme", value).apply() }
    /** The desktop's display preferences that mean something on a phone (SettingsPage.tsx). */
    var newCleared: Boolean
        get() = preferences.getBoolean("newCleared", false)
        set(value) { preferences.edit().putBoolean("newCleared", value).apply() }
    var showNeeded: Boolean
        get() = preferences.getBoolean("showNeeded", true)
        set(value) { preferences.edit().putBoolean("showNeeded", value).apply() }
    var showUpcoming: Boolean
        get() = preferences.getBoolean("showUpcoming", true)
        set(value) { preferences.edit().putBoolean("showUpcoming", value).apply() }
    var startPage: String
        get() = preferences.getString("startPage", "budget") ?: "budget"
        set(value) { preferences.edit().putString("startPage", value).apply() }
    /** Sync with Interactive Brokers once a day when the app opens, and the day it last tried. */
    var ibkrDaily: Boolean
        get() = preferences.getBoolean("ibkrDaily", true)
        set(value) { preferences.edit().putBoolean("ibkrDaily", value).apply() }
    var ibkrDailyAt: String
        get() = preferences.getString("ibkrDailyAt", "") ?: ""
        set(value) { preferences.edit().putString("ibkrDailyAt", value).apply() }
    /** "Not now" was answered to the offer to set an investment account up. */
    var ibkrPromptHidden: Boolean
        get() = preferences.getBoolean("ibkrPromptHidden", false)
        set(value) { preferences.edit().putBoolean("ibkrPromptHidden", value).apply() }
    /** Advanced: an investment account's value can be typed in, for a broker with nothing to download. */
    var typedValues: Boolean
        get() = preferences.getBoolean("typedValues", false)
        set(value) { preferences.edit().putBoolean("typedValues", value).apply() }
    /** Count this copy as in use on urbudget.app, anonymously, once a day (UsageCounts). */
    var usageCounts: Boolean
        get() = preferences.getBoolean("usageCounts", true)
        set(value) { preferences.edit().putBoolean("usageCounts", value).apply() }
}
