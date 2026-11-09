package app.urbudget.nativeapp.ui

import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.DraggableState
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.layout.offset
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.nestedscroll.NestedScrollConnection
import androidx.compose.ui.input.nestedscroll.NestedScrollSource
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.Velocity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/**
 * A panel rising from the bottom that a drag down closes, as a sheet does: from its top
 * ([dragArea]: the handle and its header) or from a list in it that's already at its start. Let
 * go past a quarter of its height, or flung down, it closes; otherwise it settles back.
 */
internal class PanelDrag(private val scope: CoroutineScope, private val close: () -> Unit) {
    var offset by mutableFloatStateOf(0f)
        private set
    internal var height = 0

    private fun by(delta: Float): Float {
        val before = offset
        offset = (offset + delta).coerceAtLeast(0f)
        return offset - before
    }

    fun settle(velocity: Float) {
        if (offset <= 0f) return
        if (offset > height * .25f || velocity > 1_500f) close()
        else scope.launch { animate(offset, 0f, animationSpec = spring()) { value, _ -> offset = value } }
    }

    fun reset() { offset = 0f }

    private val drag = DraggableState { by(it) }
    val dragArea: Modifier = Modifier.draggable(drag, Orientation.Vertical, onDragStopped = { settle(it) })

    /** A list in the panel scrolls first; only what it can't use, pulling down at its start, moves the panel. */
    val connection = object : NestedScrollConnection {
        override fun onPreScroll(available: Offset, source: NestedScrollSource): Offset =
            if (source == NestedScrollSource.UserInput && available.y < 0f && offset > 0f) Offset(0f, by(available.y)) else Offset.Zero

        override fun onPostScroll(consumed: Offset, available: Offset, source: NestedScrollSource): Offset =
            if (source == NestedScrollSource.UserInput && available.y > 0f) Offset(0f, by(available.y)) else Offset.Zero

        override suspend fun onPreFling(available: Velocity): Velocity =
            if (offset > 0f) { settle(available.y); available } else Velocity.Zero
    }
}

/** A [PanelDrag] for a panel shown while [open]: back in place each time it opens. */
@Composable
internal fun rememberPanelDrag(open: Boolean, close: () -> Unit): PanelDrag {
    val scope = rememberCoroutineScope()
    val latest by rememberUpdatedState(close)
    val drag = remember(scope) { PanelDrag(scope) { latest() } }
    LaunchedEffect(open) { if (open) drag.reset() }
    return drag
}

/** Moves the panel with the drag and lets a list in it hand over what it can't scroll. */
internal fun Modifier.panelDrag(drag: PanelDrag): Modifier =
    offset { IntOffset(0, drag.offset.roundToInt()) }.onSizeChanged { drag.height = it.height }.nestedScroll(drag.connection)
