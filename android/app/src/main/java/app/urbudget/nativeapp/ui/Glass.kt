package app.urbudget.nativeapp.ui

import android.os.Build
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.ScrollState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.BlurEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.TileMode
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** What a glass top shares with the page under it: the room the top takes, and how far the page has scrolled. */
@Stable
internal class GlassPage {
    /** The top's height once measured; until then [guess], a bar under the status bar. */
    internal var measured by mutableStateOf<Dp?>(null)
    internal var guess = 0.dp
    val room: Dp get() = measured ?: guess
    /** How far the page's content has scrolled; read while drawing, so every step of a scroll draws the glass again. */
    internal var scroll: () -> Int by mutableStateOf(NoScroll)
}

private val NoScroll: () -> Int = { 0 }

/** The glass top a page sits under, if it has one. */
internal val LocalGlassPage = staticCompositionLocalOf<GlassPage?> { null }

/** Whether this is a glass top's own content: a bar there has no fill of its own. */
internal val LocalGlassBar = staticCompositionLocalOf { false }

/**
 * A page's list under a glass top: a LazyColumn that keeps the top's room above its first item, on
 * top of its own padding, and tells the top when it scrolls. A plain LazyColumn where there's no glass top.
 */
@Composable
internal fun PageList(modifier: Modifier = Modifier, state: LazyListState = rememberLazyListState(), contentPadding: PaddingValues = PaddingValues(),
                      verticalArrangement: Arrangement.Vertical = Arrangement.Top, content: LazyListScope.() -> Unit) {
    val room = glassRoom(state)
    val direction = LocalLayoutDirection.current
    LazyColumn(modifier, state, PaddingValues(start = contentPadding.calculateStartPadding(direction), end = contentPadding.calculateEndPadding(direction),
        top = contentPadding.calculateTopPadding() + room, bottom = contentPadding.calculateBottomPadding()), verticalArrangement = verticalArrangement, content = content)
}

/**
 * For a list under a glass top: the room to keep above its first item (as content padding), so it
 * starts below the top and scrolls beneath it. Calling this is also what tells the top when the
 * list has scrolled. Nothing, on a page with no glass top.
 */
@Composable
internal fun glassRoom(list: LazyListState): Dp =
    glassRoom(list) { list.firstVisibleItemScrollOffset + if (list.firstVisibleItemIndex > 0) 1_000_000 else 0 }

/** The same for a column that scrolls as a whole. */
@Composable
internal fun glassRoom(scroll: ScrollState): Dp = glassRoom(scroll) { scroll.value }

@Composable
private fun glassRoom(key: Any, scroll: () -> Int): Dp {
    val page = LocalGlassPage.current ?: return 0.dp
    DisposableEffect(page, key) {
        page.scroll = scroll
        onDispose { if (page.scroll === scroll) page.scroll = NoScroll }
    }
    return page.room
}

/**
 * A page whose top part (its bar, and whatever stands under it, such as a search field) floats over
 * its content: clear while the content is at its start, frosted glass once it has scrolled under
 * the top. The content's list asks for [glassRoom] and keeps it as content padding. Content that
 * doesn't is [padded]: kept below the top, as on a page with a plain bar.
 *
 * The content is drawn into a layer of its own; the top draws that layer again behind itself,
 * blurred and tinted with the page's colour. Before Android 12 nothing can blur, so the tint is all
 * but solid.
 */
@Composable
internal fun GlassTop(modifier: Modifier = Modifier, padded: Boolean = false, bar: Dp = 64.dp, tint: Color = ur.bg,
                      top: @Composable ColumnScope.() -> Unit, content: @Composable () -> Unit) {
    val density = LocalDensity.current
    val page = remember { GlassPage() }
    page.guess = WindowInsets.statusBars.asPaddingValues().calculateTopPadding() + bar
    val under = rememberGraphicsLayer()
    val frost = rememberGraphicsLayer()
    val scrolled by remember(page) { derivedStateOf { page.scroll() > 0 } }
    val amount by animateFloatAsState(if (scrolled) 1f else 0f, tween(180), label = "glass")
    val edge = ur.border
    val blurs = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    val radius = with(density) { 18.dp.toPx() }
    Box(modifier) {
        Box(Modifier.fillMaxSize().then(if (padded) Modifier.padding(top = page.room) else Modifier).drawWithContent {
            under.record { this@drawWithContent.drawContent() }
            drawLayer(under)
        }) { CompositionLocalProvider(LocalGlassPage provides if (padded) null else page, content = content) }
        // The empty pointer input keeps a tap on the top's own gaps from reaching what is under it.
        Column(Modifier.fillMaxWidth().onSizeChanged { page.measured = with(density) { it.height.toDp() } }.pointerInput(Unit) {}.drawWithContent {
            if (amount > 0f) {
                if (blurs) {
                    page.scroll()
                    frost.renderEffect = BlurEffect(radius, radius, TileMode.Clamp)
                    frost.clip = true
                    frost.alpha = amount
                    // Over the page's own colour: words that stand straight on the page blur into it, rather than
                    // showing sharp through a blur that has nothing solid in it.
                    frost.record { drawRect(tint); drawLayer(under) }
                    drawLayer(frost)
                }
                drawRect(tint, alpha = amount * if (blurs) .72f else .96f)
                val line = 1.dp.toPx()
                drawLine(edge, Offset(0f, size.height - line / 2), Offset(size.width, size.height - line / 2), line, alpha = amount)
            }
            drawContent()
        }) { CompositionLocalProvider(LocalGlassBar provides true) { top() } }
    }
}
