package app.urbudget.nativeapp.ui

import android.content.res.Configuration
import android.view.View
import androidx.annotation.PluralsRes
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.unit.LayoutDirection

/** The configuration the app's screens are drawn in, so sheets and dialogs follow a language change too. */
internal val LocalAppConfiguration = staticCompositionLocalOf<Configuration?> { null }

/**
 * Draws [content] in [configuration]'s language and direction. The activity handles language
 * changes itself, so this is what redraws the open screens (strings, right to left) in place.
 */
@Composable
fun ProvideLanguage(configuration: Configuration, content: @Composable () -> Unit) {
    val direction = if (configuration.layoutDirection == View.LAYOUT_DIRECTION_RTL) LayoutDirection.Rtl else LayoutDirection.Ltr
    CompositionLocalProvider(LocalConfiguration provides configuration, LocalAppConfiguration provides configuration,
        androidx.compose.ui.platform.LocalLayoutDirection provides direction, content = content)
}

/** Keeps a sheet's or dialog's own window in the app's language as it changes. */
@Composable
internal fun FollowLanguage(content: @Composable () -> Unit) {
    val configuration = LocalAppConfiguration.current
    if (configuration == null) content() else CompositionLocalProvider(LocalConfiguration provides configuration, content = content)
}

/**
 * Changes with the interface's language. Anything remembered or prepared with words or month names
 * in it takes this as a key, so a new language rebuilds it.
 */
@Composable
@ReadOnlyComposable
internal fun languageKey(): Any? = LocalAppConfiguration.current?.locales

/** A counted phrase: the count fills `%1$s` (Western digits, as the budget writes them) and picks the plural form. */
@Composable
internal fun countText(@PluralsRes id: Int, count: Int, vararg more: Any): String = pluralStringResource(id, count, count.toString(), *more)
