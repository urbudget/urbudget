package app.urbudget.nativeapp.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.DpRect
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.coerceIn
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.window.layout.FoldingFeature
import androidx.window.layout.WindowInfoTracker
import kotlinx.coroutines.flow.map

/*
 * Large screens and foldables: an unfolded Fold, a dual-screen phone, a tablet, a phone on its side.
 * From Material's medium width class the tabs stand in a rail at the window's start, and pages keep a
 * readable width beside it. With more room still, the register and Reports show their list and what's
 * chosen from it side by side. Sheets, the assistant and a new transaction float at the window's end
 * as panels a phone wide, so the page stays in view beside them. A hinge that splits the screen (a
 * Fold half open like a book, a dual-screen phone) is never crossed: the list ends at it and what's
 * opened takes the other side. Half open on a table, what's typed on (the keypad, sheets) sits on the
 * flat half below the fold. Below the medium width the phone layout is as it always was.
 */

/** Material's medium width class starts here. */
internal val MediumWidth = 600.dp

/** How wide a page's content grows on a large screen. */
internal val ReadableWidth = 680.dp

/**
 * The rail, and the room it keeps at the window's start: its margin from the edge, the rail, and a
 * gap before the page, so lists and bands don't run up against it.
 */
internal val RailWidth = 76.dp
internal val RailMargin = 8.dp
internal val RailGap = 14.dp
internal val RailRoom = RailMargin + RailWidth + RailGap

/** The window's width. It's known once the window is measured; until then the configuration says the same. */
@Composable
internal fun windowWidth(): Dp {
    val size = LocalWindowInfo.current.containerSize
    return if (size.width > 0) with(LocalDensity.current) { size.width.toDp() } else LocalConfiguration.current.screenWidthDp.dp
}

/** The window's height, as [windowWidth]. */
@Composable
internal fun windowHeight(): Dp {
    val size = LocalWindowInfo.current.containerSize
    return if (size.height > 0) with(LocalDensity.current) { size.height.toDp() } else LocalConfiguration.current.screenHeightDp.dp
}

/** Whether the window is at least the medium width class, where the rail takes the dock's place and panels float at the side. */
@Composable
internal fun wideWindow(): Boolean = windowWidth() >= MediumWidth

/** The bars and cutout the rail and panels stay clear of: not the keyboard, which mustn't move the rail. */
internal val RailInsets: WindowInsets
    @Composable get() = WindowInsets.systemBars.union(WindowInsets.displayCutout)

/** The width pages have: the window's, less the rail and any bar or cutout on its side. */
@Composable
internal fun pageRoom(rail: Boolean): Dp {
    if (!rail) return windowWidth()
    return windowWidth() - RailRoom - railStartInset()
}

/** A bar or cutout on the rail's side of the window. */
@Composable
internal fun railStartInset(): Dp = RailInsets.only(WindowInsetsSides.Start).asPaddingValues().calculateStartPadding(LocalLayoutDirection.current)

// ---------- The fold ----------

/**
 * The fold as the window has it. [hinge] is where it runs, in the window's own coordinates (left to
 * right whatever the language), and only while it splits what's shown in two: a Fold half open, or a
 * dual-screen phone's hinge, which hides a strip of the window. [vertical] when it runs top to bottom
 * (a book); otherwise it runs across, a Fold standing half open on a table or a dual-screen phone
 * turned, and either way what's touched keeps to the bottom half.
 */
internal data class FoldState(val hinge: DpRect? = null, val vertical: Boolean = true) {
    /** A hinge running top to bottom splits the window into a start side and an end side. */
    val sideBySide get() = hinge != null && vertical
    /** A hinge running across: the top half to look at, the bottom half (on the table) to touch. */
    val stacked get() = hinge != null && !vertical
}

internal val LocalFold = staticCompositionLocalOf { FoldState() }

private tailrec fun Context.activity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}

/** Follows the fold (androidx.window) and gives what's inside it [LocalFold]. Without a fold it's the flat default. */
@Composable
internal fun ProvideFold(content: @Composable () -> Unit) {
    val activity = LocalContext.current.activity()
    if (activity == null) return content()
    // The window's features, with none to begin with. (The library's own WindowLayoutInfo can't be
    // made here to begin with: its constructor is for the library alone, and lint says so.)
    val flow = remember(activity) { WindowInfoTracker.getOrCreate(activity).windowLayoutInfo(activity).map { it.displayFeatures } }
    val features by flow.collectAsStateWithLifecycle(emptyList())
    val density = LocalDensity.current
    val fold = remember(features, density) {
        val feature = features.filterIsInstance<FoldingFeature>().firstOrNull()
        if (feature == null || !feature.isSeparating) FoldState()
        else with(density) {
            val b = feature.bounds
            FoldState(DpRect(b.left.toDp(), b.top.toDp(), b.right.toDp(), b.bottom.toDp()),
                vertical = feature.orientation == FoldingFeature.Orientation.VERTICAL)
        }
    }
    CompositionLocalProvider(LocalFold provides fold, content = content)
}

/** Where a top-to-bottom hinge starts, measured from the window's start edge (the right in Arabic), and how wide it is. */
internal data class HingeSpan(val start: Dp, val width: Dp) { val end get() = start + width }

@Composable
internal fun verticalHinge(): HingeSpan? {
    val fold = LocalFold.current
    val hinge = fold.hinge?.takeIf { fold.sideBySide } ?: return null
    val start = if (LocalLayoutDirection.current == LayoutDirection.Ltr) hinge.left else windowWidth() - hinge.right
    return HingeSpan(start, hinge.right - hinge.left)
}

/** The top of the flat half while the phone stands half open on a table: the hinge's foot, from the window's top. */
@Composable
internal fun tableTop(): Dp? = LocalFold.current.let { if (it.stacked) it.hinge?.bottom else null }

// ---------- The app beside the assistant ----------

/** The app's width from the window's start edge while the assistant shares the window beside it; null while the app has it all. */
internal val LocalAppArea = staticCompositionLocalOf<Dp?> { null }

/**
 * The app as a screen of its own beside the assistant: what's inside takes [width] for the window's
 * width, so a phone-wide side lays out as a phone (the dock, sheets from its foot), sees no fold, and
 * its sheets and full-screen forms keep to that side ([LocalAppArea]). With no [width] it's the window.
 */
@Composable
internal fun ProvideAppSide(width: Dp?, content: @Composable () -> Unit) {
    if (width == null) return content()
    val real = LocalWindowInfo.current
    val px = with(LocalDensity.current) { width.roundToPx() }
    val side = remember(real, px) {
        object : androidx.compose.ui.platform.WindowInfo {
            override val isWindowFocused: Boolean get() = real.isWindowFocused
            override val containerSize: androidx.compose.ui.unit.IntSize get() = androidx.compose.ui.unit.IntSize(px, real.containerSize.height)
        }
    }
    CompositionLocalProvider(LocalWindowInfo provides side, LocalFold provides FoldState(), LocalAppArea provides width, content = content)
}

// ---------- Panels at the side ----------

/** The gap a floating panel keeps from the window's edges, as the rail does. */
internal val PanelMargin = 12.dp

/**
 * How wide a panel floating at the window's end is: a phone wide, about half the window. With a hinge
 * splitting the screen top to bottom it's the whole end side, less a bar or cutout on that edge (the
 * panel keeps clear of those too), so it never crosses the hinge. On a narrow window it's the window's
 * width, less its margins: a sheet opened on the inner screen stays a floating card when folded.
 */
@Composable
internal fun panelWidth(max: Dp = 460.dp): Dp {
    val hinge = verticalHinge()
    val window = windowWidth()
    val sides = RailInsets.only(WindowInsetsSides.Horizontal).asPaddingValues()
    val direction = LocalLayoutDirection.current
    return when {
        hinge != null -> window - hinge.end - sides.calculateEndPadding(direction) - PanelMargin * 2
        window < MediumWidth -> window - sides.calculateStartPadding(direction) - sides.calculateEndPadding(direction) - PanelMargin * 2
        else -> (window * .46f).coerceIn(340.dp, max)
    }
}

/** A panel's way in from the window's end edge, and back out (the left in Arabic). */
@Composable
internal fun panelEnter(ltr: Boolean = LocalLayoutDirection.current == LayoutDirection.Ltr) =
    androidx.compose.animation.slideInHorizontally(androidx.compose.animation.core.tween(PanelIn)) { if (ltr) it / 2 else -it / 2 } +
        androidx.compose.animation.fadeIn(androidx.compose.animation.core.tween(PanelIn))

@Composable
internal fun panelExit(ltr: Boolean = LocalLayoutDirection.current == LayoutDirection.Ltr) =
    androidx.compose.animation.slideOutHorizontally(androidx.compose.animation.core.tween(PanelOut)) { if (ltr) it / 2 else -it / 2 } +
        androidx.compose.animation.fadeOut(androidx.compose.animation.core.tween(PanelOut))

/** How long a floating panel takes to come in and to go. */
internal const val PanelIn = 260
internal const val PanelOut = 200

// ---------- List and detail ----------

/**
 * From this much room a page with a list shows what's chosen from it alongside: an unfolded Fold
 * beside the rail, a tablet, a phone on its side. Each pane is then about a phone wide.
 */
internal val SplitWidth = 720.dp

/** How wide a page split in two grows: the list, and a readable detail beside it. */
internal val SplitPageWidth = 1100.dp

/**
 * The list's width beside its detail on a page [room] wide: half of a Fold's room, a little less on a
 * tablet. Larger text widens it, so names aren't cut short, as long as the detail keeps a phone's width.
 */
internal fun splitListWidth(room: Dp, fontScale: Float = 1f): Dp {
    val page = minOf(room, SplitPageWidth)
    val share = page * (if (page < 960.dp) .5f else .42f) * fontScale.coerceIn(1f, 1.3f)
    return minOf(share, page - 360.dp).coerceIn(300.dp, 480.dp)
}

/**
 * Reports' list beside a report: the reports by name, narrow, so the report (charts, its chips and
 * figures) has most of the room.
 */
internal fun reportsListWidth(room: Dp): Dp = (minOf(room, SplitPageWidth) * .36f).coerceIn(272.dp, 340.dp)

/**
 * The budget beside an open category: the category's details take a phone's width at the end, and
 * the budget keeps the rest, so the row that was tapped (its Assigned and Available) stays in view.
 */
internal fun budgetListWidth(room: Dp): Dp {
    val page = minOf(room, SplitPageWidth)
    // The budget's two figure columns grow with the currency (SAR's are wide), so the category keeps
    // little more than a phone's narrowest width and the budget's names the rest.
    return page - (page * .42f).coerceIn(330.dp, 400.dp) - SplitGap
}

/**
 * Whether a sheet shows docked in a page's detail pane rather than floating over the page: the
 * budget's category, beside the budget. The sheet's own sheets (a note, a picker) still float.
 */
internal val LocalSheetPane = staticCompositionLocalOf { false }

/** Whether a sheet's content fills its pane to the bottom, so a keypad under it sits at the pane's foot. */
internal val LocalSheetFills = staticCompositionLocalOf { false }

/** The far side of a hinge, or a pane with nothing open in it yet: what's opened comes here. */
@Composable
internal fun FoldSideHint() {
    Box(Modifier.fillMaxSize().statusBarsPadding(), contentAlignment = androidx.compose.ui.Alignment.Center) {
        EmptyState(androidx.compose.ui.res.stringResource(app.urbudget.nativeapp.R.string.fold_side_title),
            androidx.compose.ui.res.stringResource(app.urbudget.nativeapp.R.string.fold_side_detail))
    }
}

/** The list's width while the page shows its list and detail side by side, and the gap between them (a hinge's, or a gutter). */
internal data class Split(val list: Dp, val gap: Dp = SplitGap, val hinge: Boolean = false) { val detailStart get() = list + gap }

/** The split while the page shows list and detail side by side; null while it shows one at a time. */
internal val LocalSplitList = staticCompositionLocalOf<Split?> { null }

/**
 * A page that can show its list and the item chosen from it side by side. Split (the app decides,
 * through [LocalSplitList]), [list] and [detail] share the page with a gutter between, or the hinge
 * where one runs; otherwise [single] is the page as a phone shows it, one thing at a time. The panes'
 * own cards mark where each ends, so there's no line between them.
 */
@Composable
internal fun ListDetail(list: @Composable () -> Unit, detail: @Composable () -> Unit, single: @Composable () -> Unit = list) {
    val split = LocalSplitList.current
    if (split == null) single()
    else Row(Modifier.fillMaxSize()) {
        Box(Modifier.width(split.list).fillMaxHeight()) { list() }
        Spacer(Modifier.width(split.gap))
        Box(Modifier.weight(1f).fillMaxHeight()) { detail() }
    }
}

/** The gutter between a split page's list and detail. */
internal val SplitGap = 12.dp

/** A list beside its detail is a card inset from the pane's edges: its first band rounds at the top, its last row at the foot. */
internal val PaneListShape = androidx.compose.foundation.shape.RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp)
internal val PaneListFoot = androidx.compose.foundation.shape.RoundedCornerShape(bottomStart = 20.dp, bottomEnd = 20.dp)

/**
 * The transaction form open beside the register: whether it holds changes, and how to drop its
 * draft when the person moves on (another row, another tab), rather than keeping it for recovery.
 */
internal class PaneForm {
    var dirty = false
    var discard: () -> Unit = {}
}

/** Where a transaction form shows. Left unsaid, the form picks: floating in a window of its own on a large screen, else over the screen. */
internal sealed interface FormHost {
    /** Beside the register, in the pane its list leaves; [pane] tells the app whether it holds changes. */
    class Beside(val pane: PaneForm) : FormHost
    /** Floating at the window's end over the page, as a layer of the app's own window. */
    object Side : FormHost
    /** The same in a window of its own, for a form opened from deep inside a page. */
    object SideWindow : FormHost
    /** Over the whole screen, as on a phone. */
    object Screen : FormHost
}
