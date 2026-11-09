package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.coerceIn
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

/**
 * From this much room a page with a list shows what's chosen from it alongside: an unfolded Fold
 * beside the rail (about 728dp), a tablet, a phone on its side. Each pane is then about a phone wide.
 */
internal val SplitWidth = 720.dp

/** How wide a page split in two grows: the list, and a readable detail beside it. */
internal val SplitPageWidth = 1100.dp

/**
 * A page that can show its list and the item chosen from it side by side. With [SplitWidth] or more,
 * [list] and [detail] share the page with a hairline between; with less, [single] is the page as a
 * phone shows it, one at a time, at most [ReadableWidth] wide.
 */
@Composable
internal fun ListDetail(single: @Composable () -> Unit, list: @Composable () -> Unit, detail: @Composable () -> Unit) {
    BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        val listWidth = (maxWidth * .4f).coerceIn(320.dp, 400.dp)
        if (maxWidth < SplitWidth) Box(Modifier.widthIn(max = ReadableWidth).fillMaxSize()) { single() }
        else Row(Modifier.fillMaxSize()) {
            Box(Modifier.width(listWidth).fillMaxHeight()) { list() }
            VerticalDivider(thickness = 0.5.dp, color = ur.border)
            Box(Modifier.weight(1f).fillMaxHeight()) { detail() }
        }
    }
}
