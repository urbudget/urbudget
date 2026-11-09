package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.dp

/*
 * Large screens: an unfolded Fold, a tablet, a phone on its side. From Material's medium width class
 * the tabs stand in a rail at the window's start, and pages keep a readable width, centred in what
 * the rail leaves. Below it the phone layout is as it always was.
 */

/** Material's medium width class starts here. */
internal val MediumWidth = 600.dp

/** How wide a page's content grows on a large screen. Sheets stop at Material's 640dp of their own. */
internal val ReadableWidth = 680.dp

/** The assistant's panel, as wide as the sheets. */
internal val PanelWidth = 640.dp

/** The rail, and the room it keeps at the window's start: the rail and its margin. */
internal val RailWidth = 92.dp
internal val RailMargin = 12.dp
internal val RailRoom = RailWidth + RailMargin

/** Whether the window is at least the medium width class, where the rail takes the dock's place. */
@Composable
internal fun wideWindow(): Boolean {
    val size = LocalWindowInfo.current.containerSize
    // The window's size is known once it's measured; until then the configuration says the same.
    val width = if (size.width > 0) with(LocalDensity.current) { size.width.toDp() } else LocalConfiguration.current.screenWidthDp.dp
    return width >= MediumWidth
}

/** The bars and cutout the rail stays clear of: not the keyboard, which mustn't move it. */
internal val RailInsets: WindowInsets
    @Composable get() = WindowInsets.systemBars.union(WindowInsets.displayCutout)
