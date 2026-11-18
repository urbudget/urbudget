package app.urbudget.nativeapp.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.IndicationNodeFactory
import androidx.compose.foundation.background
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.foundation.interaction.FocusInteraction
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.node.CompositionLocalConsumerModifierNode
import androidx.compose.ui.node.DelegatableNode
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.currentValueOf
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/*
 * What a press looks like across the app (the theme provides it as LocalIndication, so every
 * clickable, selectable and toggleable takes it): a soft wash in the text colour with rounded
 * corners, in place of Material's ripple, which spreads to a row's square edges. A row that runs
 * the width of its card is washed inset from the card's edges, as One UI marks a pressed row; a
 * button, chip or tile is washed whole, its corners rounded (and clipped to its own shape, where
 * it has one). The wash comes up quickly and fades out once the finger lifts, and a press too
 * quick to see still shows briefly. Keyboard focus keeps a lighter wash for as long as it lasts.
 */
/**
 * A chosen row's tint (a selected transaction, the category open beside the budget, a picked
 * report row): the same rounded shape inside the row's edges as a press, over the row's own
 * [base] fill, rather than a band to its square ends.
 */
internal fun Modifier.chosenWash(chosen: Boolean, tint: Color, base: Color): Modifier =
    background(base).then(if (!chosen) Modifier else Modifier.drawBehind {
        val side = 6.dp.toPx()
        val end = 2.dp.toPx()
        drawRoundRect(tint, Offset(side, end), Size(size.width - side * 2, size.height - end * 2), CornerRadius(12.dp.toPx()))
    })

internal object SoftPress : IndicationNodeFactory {
    override fun create(interactionSource: InteractionSource): DelegatableNode = SoftPressNode(interactionSource)
    override fun equals(other: Any?): Boolean = other === this
    override fun hashCode(): Int = javaClass.hashCode()
}

private class SoftPressNode(private val source: InteractionSource) : Modifier.Node(), DrawModifierNode, CompositionLocalConsumerModifierNode {
    private val wash = Animatable(0f)
    private var focused = false
    private var running: Job? = null

    override fun onAttach() {
        coroutineScope.launch {
            val presses = mutableListOf<PressInteraction.Press>()
            source.interactions.collect { interaction ->
                when (interaction) {
                    is PressInteraction.Press -> { presses += interaction; to(1f, 90) }
                    is PressInteraction.Release -> { presses -= interaction.press; if (presses.isEmpty()) settle() }
                    is PressInteraction.Cancel -> { presses -= interaction.press; if (presses.isEmpty()) settle() }
                    is FocusInteraction.Focus -> { focused = true; if (presses.isEmpty()) to(FOCUS, 150) }
                    is FocusInteraction.Unfocus -> { focused = false; if (presses.isEmpty()) to(0f, 200) }
                }
            }
        }
    }

    private fun to(target: Float, millis: Int) {
        running?.cancel()
        running = coroutineScope.launch { wash.animateTo(target, tween(millis)) { invalidateDraw() } }
    }

    /** Lifted: the wash finishes coming up if a quick tap cut it short, then fades. */
    private fun settle() {
        running?.cancel()
        running = coroutineScope.launch {
            if (wash.value < .7f) wash.animateTo(1f, tween(70)) { invalidateDraw() }
            wash.animateTo(if (focused) FOCUS else 0f, tween(260)) { invalidateDraw() }
        }
    }

    override fun ContentDrawScope.draw() {
        drawContent()
        val amount = wash.value
        if (amount <= 0f) return
        val colors = currentValueOf(LocalUr)
        val w = size.width
        val h = size.height
        // A row across its card: washed inside the card's edges. Anything smaller: washed whole.
        val row = h >= 40.dp.toPx() && w >= h * 3.2f
        val sideInset = if (row) 6.dp.toPx() else 0f
        val endInset = if (row) 3.dp.toPx() else 0f
        val radius = if (row) 12.dp.toPx() else minOf(12.dp.toPx(), minOf(w, h) / 2)
        drawRoundRect(colors.text, topLeft = Offset(sideInset, endInset), size = Size(w - sideInset * 2, h - endInset * 2),
            cornerRadius = CornerRadius(radius), alpha = amount * if (colors.dark) .12f else .08f)
    }

    private companion object {
        const val FOCUS = .6f
    }
}
