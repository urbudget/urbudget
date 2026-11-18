package app.urbudget.nativeapp.ui

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import app.urbudget.nativeapp.NativePreferences

/**
 * The app preferences screens read, as state: changing one in Settings shows at once wherever it's
 * used. Without a store (previews, tests) they keep the desktop's defaults in memory.
 */
@Stable
internal class UiPreferences(private val store: NativePreferences?) {
    private var cleared by mutableStateOf(store?.newCleared ?: false)
    private var needed by mutableStateOf(store?.showNeeded ?: true)
    private var upcoming by mutableStateOf(store?.showUpcoming ?: true)
    private var start by mutableStateOf(store?.startPage ?: "budget")
    private var schedules by mutableStateOf(store?.automaticSchedules ?: true)
    private var brokerDaily by mutableStateOf(store?.ibkrDaily ?: true)
    private var brokerPromptHidden by mutableStateOf(store?.ibkrPromptHidden ?: false)
    private var brokerDailyAt = store?.ibkrDailyAt ?: ""
    private var typed by mutableStateOf(store?.typedValues ?: false)

    val newCleared get() = cleared
    val showNeeded get() = needed
    val showUpcoming get() = upcoming
    val startPage get() = start
    val automaticSchedules get() = schedules
    /** Sync with Interactive Brokers once a day when the app opens. */
    val ibkrDaily get() = brokerDaily
    /** The day that sync was last tried ("2026-11-17"), or empty. */
    var ibkrDailyAt: String
        get() = brokerDailyAt
        set(value) { brokerDailyAt = value; store?.ibkrDailyAt = value }
    /** "Not now" was answered to the offer to set an investment account up. */
    val ibkrPromptHidden get() = brokerPromptHidden
    /** Advanced: typing an investment account's value is offered. */
    val typedValues get() = typed

    fun setNewCleared(value: Boolean) { cleared = value; store?.newCleared = value }
    fun setShowNeeded(value: Boolean) { needed = value; store?.showNeeded = value }
    fun setShowUpcoming(value: Boolean) { upcoming = value; store?.showUpcoming = value }
    fun setStartPage(value: String) { start = value; store?.startPage = value }
    fun setAutomaticSchedules(value: Boolean) { schedules = value; store?.automaticSchedules = value }
    fun setIbkrDaily(value: Boolean) { brokerDaily = value; store?.ibkrDaily = value }
    fun setIbkrPromptHidden(value: Boolean) { brokerPromptHidden = value; store?.ibkrPromptHidden = value }
    fun setTypedValues(value: Boolean) { typed = value; store?.typedValues = value }
}

internal val LocalPreferences = staticCompositionLocalOf { UiPreferences(null) }
