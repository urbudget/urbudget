package app.urbudget.nativeapp.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.coerceIn
import androidx.compose.ui.unit.dp

/*
 * Large screens: an unfolded Fold, a tablet, a phone on its side. From Material's medium width class
 * the tabs stand in a rail at the window's start, and pages keep a readable width, centred in what
 * the rail leaves. With more room still, the register and Reports show their list and what's chosen
 * from it side by side. Below the medium width the phone layout is as it always was.
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

/** The window's width. It's known once the window is measured; until then the configuration says the same. */
@Composable
internal fun windowWidth(): Dp {
    val size = LocalWindowInfo.current.containerSize
    return if (size.width > 0) with(LocalDensity.current) { size.width.toDp() } else LocalConfiguration.current.screenWidthDp.dp
}

/** Whether the window is at least the medium width class, where the rail takes the dock's place. */
@Composable
internal fun wideWindow(): Boolean = windowWidth() >= MediumWidth

/** The bars and cutout the rail stays clear of: not the keyboard, which mustn't move it. */
internal val RailInsets: WindowInsets
    @Composable get() = WindowInsets.systemBars.union(WindowInsets.displayCutout)

/** The width pages have: the window's, less the rail and any bar or cutout on its side. */
@Composable
internal fun pageRoom(rail: Boolean): Dp {
    if (!rail) return windowWidth()
    val start = RailInsets.only(WindowInsetsSides.Start).asPaddingValues().calculateStartPadding(LocalLayoutDirection.current)
    return windowWidth() - RailRoom - start
}

/**
 * From this much room a page with a list shows what's chosen from it alongside: an unfolded Fold
 * beside the rail (about 728dp), a tablet, a phone on its side. Each pane is then about a phone wide.
 */
internal val SplitWidth = 720.dp

/** How wide a page split in two grows: the list, and a readable detail beside it. */
internal val SplitPageWidth = 1100.dp

/** The list's width beside its detail on a page [room] wide: about 40%, a phone's width or so. */
internal fun splitListWidth(room: Dp): Dp = (minOf(room, SplitPageWidth) * .4f).coerceIn(320.dp, 400.dp)

/** The list's width while the page shows its list and detail side by side; null while it shows one at a time. */
internal val LocalSplitList = staticCompositionLocalOf<Dp?> { null }

/**
 * A page that can show its list and the item chosen from it side by side. Split (the app decides,
 * through [LocalSplitList]), [list] and [detail] share the page with a hairline between; otherwise
 * [single] is the page as a phone shows it, one thing at a time.
 */
@Composable
internal fun ListDetail(list: @Composable () -> Unit, detail: @Composable () -> Unit, single: @Composable () -> Unit = list) {
    val listWidth = LocalSplitList.current
    if (listWidth == null) single()
    else Row(Modifier.fillMaxSize()) {
        Box(Modifier.width(listWidth).fillMaxHeight()) { list() }
        VerticalDivider(thickness = SplitDivider, color = ur.border)
        Box(Modifier.weight(1f).fillMaxHeight()) { detail() }
    }
}

/** The hairline between a split page's list and detail. */
internal val SplitDivider = 0.5.dp

/**
 * The transaction form open beside the register: whether it holds changes, and how to drop its
 * draft when the person moves on (another row, another tab), rather than keeping it for recovery.
 */
internal class PaneForm {
    var dirty = false
    var discard: () -> Unit = {}
}
