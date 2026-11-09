package app.urbudget.nativeapp.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.absoluteOffset
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.outlined.KeyboardArrowRight
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.AbsoluteAlignment
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Fill
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.hapticfeedback.HapticFeedback
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.urbudget.nativeapp.BarRowModel
import app.urbudget.nativeapp.ColumnModel
import app.urbudget.nativeapp.DivRowModel
import app.urbudget.nativeapp.HeatModel
import app.urbudget.nativeapp.LegendItem
import app.urbudget.nativeapp.LinesModel
import app.urbudget.nativeapp.MiniBarsModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.Readout
import app.urbudget.nativeapp.ReportColors
import app.urbudget.nativeapp.SliceModel
import app.urbudget.nativeapp.SpendRow
import app.urbudget.nativeapp.Tile
import app.urbudget.nativeapp.Tone
import app.urbudget.nativeapp.TreemapItem
import app.urbudget.nativeapp.XTick
import app.urbudget.nativeapp.jsWeekday
import app.urbudget.nativeapp.reportMonthName
import app.urbudget.nativeapp.squarify
import app.urbudget.nativeapp.weekdayName
import app.urbudget.nativeapp.without
import kotlinx.coroutines.launch
import java.time.LocalDate
import java.time.YearMonth
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sin

/*
 * Canvas charts for the reports, after the desktop's hand-written SVG charts (charts/Charts.tsx):
 * bars at most 24dp wide with a 4dp rounded data end, 2dp gaps between stacked parts and slices,
 * 2dp lines, end dots with a surface ring, hairline grids, labels in the muted text colour.
 *
 * Every chart answers a finger the same way: press and drag to move from month to month, or tap
 * one. The month under the finger lifts, the rest fade, a guide snaps to it and a small bubble over
 * it says its figures, with a light tick at each new month; the readout above holds them all.
 * Charts open on the last complete month; the month still under way is drawn lighter, its line
 * dashed. Bars grow and lines rise as a chart first shows, and move to new figures rather than
 * jumping. A legend's names hide and show their series.
 */

private val LightSeries = listOf(Color(0xFF2A78D6), Color(0xFFEB6834), Color(0xFF1BAF7A), Color(0xFFEDA100), Color(0xFFE87BA4), Color(0xFF008300), Color(0xFF6250D6), Color(0xFFE34948))
private val DarkSeries = listOf(Color(0xFF3987E5), Color(0xFFD95926), Color(0xFF199E70), Color(0xFFC98500), Color(0xFFD55181), Color(0xFF008300), Color(0xFF9085E9), Color(0xFFE66767))

/** The desktop's report palette (reports.css) for the theme in use, and the theme's own roles. */
@Immutable
internal class ReportPalette(val colors: UrColors) {
    val series = if (colors.dark) DarkSeries else LightSeries
    val other = if (colors.dark) Color(0xFF5D636C) else Color(0xFFA3A8B0)
    /**
     * Heat levels 0 (nothing spent) to 5: the reference blue ramp's steps 250-650 on light (more is
     * darker) and 600-200 on dark (more is brighter). Checked with the dataviz palette validator
     * (--ordinal): one hue, steps at least 0.06 apart in lightness, the faintest 2:1 on the card.
     */
    val heat = listOf(colors.surface2) + (if (colors.dark) listOf(0xFF184F95, 0xFF256ABF, 0xFF3987E5, 0xFF6DA7EC, 0xFF9EC5F4)
        else listOf(0xFF86B6EF, 0xFF5598E7, 0xFF2A78D6, 0xFF1C5CAB, 0xFF104281)).map { Color(it) }
    /** A date's ink on a heat level: light on the deep steps, dark on the bright ones. */
    fun heatInk(level: Int): Color = when {
        level == 0 -> colors.text2
        colors.dark -> if (level >= 4) colors.bg else colors.text
        else -> if (level >= 3) Color.White else colors.text
    }
    /** A payee tile: its group's colour softened, so its name reads on it in the theme's text colour. */
    fun tile(color: Color): Color = lerp(colors.surface, color, if (colors.dark) .46f else .30f)
    val hover = colors.text.copy(alpha = if (colors.dark) .08f else .05f)
    val axis = TextStyle(color = colors.muted, fontSize = 11.sp, fontFeatureSettings = "tnum")
    val axisPicked = TextStyle(color = colors.text, fontSize = 11.sp, fontWeight = FontWeight.Bold, fontFeatureSettings = "tnum")
    val note = TextStyle(color = colors.text2, fontSize = 11.sp, fontFeatureSettings = "tnum")
    val value = TextStyle(color = colors.text2, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, fontFeatureSettings = "tnum")

    fun color(code: Int?): Color = when {
        code == null -> colors.text2
        code in 0..7 -> series[code]
        code == ReportColors.INK -> colors.text
        code == ReportColors.POSITIVE -> colors.positive
        code == ReportColors.NEGATIVE -> colors.negative
        else -> other
    }
}

@Composable
internal fun reportPalette(): ReportPalette {
    val colors = ur
    return remember(colors) { ReportPalette(colors) }
}

/** How far the other marks fade while one is picked, and how light the month still under way is. */
private const val Faded = .32f
private const val SoFar = .45f

/** A swatch matching the mark: a box for bars, a short line for lines (dashed for dashed lines). */
@Composable
internal fun LegendKey(color: Color, line: Boolean = false, dashed: Boolean = false) {
    if (!line) Box(Modifier.size(10.dp).clip(RoundedCornerShape(2.dp)).background(color))
    else Canvas(Modifier.size(14.dp, 3.dp)) {
        drawLine(color, Offset(0f, size.height / 2), Offset(size.width, size.height / 2), 2.dp.toPx(), StrokeCap.Round,
            if (dashed) PathEffect.dashPathEffect(floatArrayOf(3.dp.toPx(), 2.5.dp.toPx())) else null)
    }
}

/**
 * Which colour is which. With [onToggle], each name hides and shows its series: a hidden one keeps
 * an empty key and is struck through. With [tiles], keys wear the softer fill treemap tiles use.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ChartLegend(items: List<LegendItem>, modifier: Modifier = Modifier, hidden: Set<String> = emptySet(), onToggle: ((String) -> Unit)? = null, tiles: Boolean = false) {
    val palette = reportPalette()
    FlowRow(modifier.fillMaxWidth().padding(top = if (onToggle != null) 2.dp else 8.dp),
        horizontalArrangement = Arrangement.spacedBy(if (onToggle != null) 4.dp else 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        items.forEach { item ->
            val off = item.label in hidden
            val chip = if (onToggle == null) Modifier else Modifier.clip(RoundedCornerShape(16.dp))
                .toggleable(!off, role = Role.Checkbox) { onToggle(item.label) }.heightIn(min = 34.dp).padding(horizontal = 8.dp)
            Row(chip, verticalAlignment = Alignment.CenterVertically) {
                if (off) Box(Modifier.size(10.dp).border(1.dp, ur.borderStrong, RoundedCornerShape(2.dp)))
                else LegendKey(palette.color(item.color).let { if (tiles) palette.tile(it) else it }, item.line, item.dashed)
                Spacer(Modifier.width(6.dp))
                Text(Money.own(item.label), color = if (off) ur.muted else ur.text2, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    textDecoration = if (off) TextDecoration.LineThrough else null)
            }
        }
    }
}

/** The figures of the month a chart shows: the one picked, or the last complete one. [unit] is the axis' currency. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ChartReadout(title: String, rows: List<Readout>, modifier: Modifier = Modifier, unit: String? = null) {
    val palette = reportPalette()
    Column(modifier.fillMaxWidth().heightIn(min = 40.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(title, Modifier.weight(1f), color = ur.muted, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            unit?.let { Text(it, Modifier.padding(start = 8.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall, maxLines = 1) }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            if (rows.isEmpty()) Text(stringResource(R.string.chart_nothing), color = ur.muted, style = MaterialTheme.typography.bodyMedium)
            rows.forEach { row ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (row.color != null) { LegendKey(palette.color(row.color), row.line, row.dashed); Spacer(Modifier.width(6.dp)) }
                    Text(row.value, color = ur.text, style = MaterialTheme.typography.bodyMedium.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1)
                    Text(" ${row.label}", color = ur.text2, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

/**
 * A small dark tag for the picked month: its name and first figures. It sits at the chart's top over
 * [x]; when the month's highest mark ([markY]) reaches up there, it moves beside the month (whose
 * marks span [half] either side of [x]) so all of them stay in view, and under the mark only when
 * neither side has room between the plot's [plotLeft] and the chart's [width].
 */
@Composable
private fun BoxScope.ChartBubble(title: String, rows: List<Readout>, x: Float, half: Float, plotLeft: Float, width: Int, markY: Float) {
    val palette = reportPalette()
    var own by remember { mutableStateOf(IntSize.Zero) }
    val gap = with(LocalDensity.current) { 8.dp.toPx() }
    val centered = (x - own.width / 2f).coerceIn(0f, max(0f, (width - own.width).toFloat()))
    val before = x - half - gap - own.width
    val after = x + half + gap
    val (left, top) = when {
        markY >= own.height + gap -> centered to 0f
        before >= plotLeft -> before to 0f
        after + own.width <= width -> after to 0f
        else -> centered to markY + gap
    }
    Column(Modifier.align(AbsoluteAlignment.TopLeft).absoluteOffset { IntOffset(left.roundToInt(), top.roundToInt()) }.onSizeChanged { own = it }
        .widthIn(max = 220.dp).clip(RoundedCornerShape(10.dp)).background(ur.text).padding(horizontal = 9.dp, vertical = 5.dp)) {
        Text(title, color = ur.surface.copy(alpha = .72f), fontSize = 11.sp, lineHeight = 14.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        rows.take(2).forEach { r ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (r.color != null) { LegendKey(palette.color(r.color), r.line, r.dashed); Spacer(Modifier.width(5.dp)) }
                Text(r.value, color = ur.surface, fontSize = 13.sp, lineHeight = 17.sp, fontWeight = FontWeight.SemiBold, style = Tabular, maxLines = 1)
            }
        }
    }
}

/** The tick labels of a y axis, measured once per model. */
private class AxisLabels(val ticks: List<TextLayoutResult>) {
    val width = ticks.maxOfOrNull { it.size.width } ?: 0
}

@Composable
private fun rememberAxis(labels: List<String>, measurer: TextMeasurer, style: TextStyle): AxisLabels =
    remember(labels, style) { AxisLabels(labels.map { measurer.measure(it, style) }) }

/** The plot area in pixels and its value scale. */
private class Frame(val left: Float, val top: Float, val right: Float, val bottom: Float, val lo: Double, val hi: Double) {
    val width get() = right - left
    fun y(v: Double): Float = if (hi == lo) bottom else (bottom - (v - lo) / (hi - lo) * (bottom - top)).toFloat()
}

private fun Density.plotLeft(axis: AxisLabels) = axis.width + 8.dp.toPx()

/** Where [value] sits in a chart [height] tall on the scale [ticks], as [plotFrame] places it. */
private fun Density.markY(height: Dp, ticks: List<Double>, value: Double): Float {
    val top = 12.dp.toPx()
    val bottom = height.toPx() - 22.dp.toPx()
    val lo = ticks.first()
    val hi = ticks.last()
    return if (hi == lo) bottom else (bottom - (value - lo) / (hi - lo) * (bottom - top)).toFloat()
}

private fun Density.plotFrame(size: Size, axis: AxisLabels, lo: Double, hi: Double) = Frame(
    left = plotLeft(axis), top = 12.dp.toPx(), right = size.width - 6.dp.toPx(), bottom = size.height - 22.dp.toPx(), lo = lo, hi = hi,
)

/** Gridlines and their labels; while a scale moves, the ones outside the plot wait. */
private fun DrawScope.drawAxes(frame: Frame, ticks: List<Double>, axis: AxisLabels, palette: ReportPalette) {
    val gap = 6.dp.toPx()
    val slack = 2.dp.toPx()
    ticks.forEachIndexed { i, t ->
        val y = frame.y(t)
        if (y < frame.top - slack || y > frame.bottom + slack) return@forEachIndexed
        drawLine(if (t == 0.0) palette.colors.borderStrong else palette.colors.border, Offset(frame.left, y), Offset(frame.right, y), 1f)
        val label = axis.ticks.getOrNull(i) ?: return@forEachIndexed
        drawText(label, topLeft = Offset(frame.left - gap - label.size.width, y - label.size.height / 2f))
    }
}

/**
 * X labels that never crowd, at an even step (every month, every 2nd, 3rd...) counted back from the
 * last month, so the latest always shows. The year shows in full on the first label and where it
 * changes ("Dec 2025", "Feb", "Apr"). The picked month's label is always drawn, in bold; labels it
 * would cover step aside, and the step itself doesn't change while picking.
 */
private fun Density.xLabels(ticks: List<XTick?>, x: (Int) -> Float, measurer: TextMeasurer, palette: ReportPalette, picked: Int): List<Pair<Float, TextLayoutResult>> {
    val gap = 8.dp.toPx()
    val last = ticks.indexOfLast { it != null }
    if (last < 0) return emptyList()
    // Labels every [step] months; null when two would touch and [strict].
    fun place(step: Int, picked: Int, strict: Boolean): List<Pair<Float, TextLayoutResult>>? {
        val out = ArrayList<Pair<Float, TextLayoutResult>>()
        var lastRight = Float.NEGATIVE_INFINITY
        var lastYear: Int? = null
        // The picked label says its year when the month before it is in another.
        val pickedLayout = ticks.getOrNull(picked)?.let { tick ->
            val before = (picked - 1 downTo 0).firstNotNullOfOrNull { ticks[it] }?.year
            measurer.measure(if (before == null || before != tick.year) tick.long else tick.short, palette.axisPicked)
        }
        val pickedLeft = pickedLayout?.let { x(picked) - it.size.width / 2f - gap }
        val pickedRight = pickedLayout?.let { x(picked) + it.size.width / 2f + gap }
        ticks.forEachIndexed { i, tick ->
            if (tick == null) return@forEachIndexed
            if (i == picked) { lastRight = (pickedRight ?: 0f) - gap; lastYear = tick.year; return@forEachIndexed }
            if ((last - i) % step != 0) return@forEachIndexed
            val layout = measurer.measure(if (lastYear == null || lastYear != tick.year) tick.long else tick.short, palette.axis)
            val left = x(i) - layout.size.width / 2f
            val right = x(i) + layout.size.width / 2f
            if (pickedLeft != null && pickedRight != null && right > pickedLeft && left < pickedRight) return@forEachIndexed
            if (left < lastRight + gap) { if (strict) return null else return@forEachIndexed }
            out.add(x(i) to layout)
            lastRight = right
            lastYear = tick.year
        }
        if (pickedLayout != null) out.add(x(picked) to pickedLayout)
        return out
    }
    val step = LabelSteps.firstOrNull { place(it, -1, strict = true) != null } ?: Int.MAX_VALUE
    return place(step, picked, strict = false).orEmpty()
}

/** How many months apart x labels may be: whole quarters, halves and years once months crowd. */
private val LabelSteps = listOf(1, 2, 3, 4, 6, 12, 24, 36, 60, 120)

private fun DrawScope.drawXLabels(labels: List<Pair<Float, TextLayoutResult>>, frame: Frame) {
    labels.forEach { (x, layout) ->
        val left = (x - layout.size.width / 2f).coerceIn(0f, max(0f, size.width - layout.size.width))
        drawText(layout, topLeft = Offset(left, frame.bottom + 5.dp.toPx()))
    }
}

/** Text with a halo in the surface colour, readable where it crosses marks. */
private fun DrawScope.haloText(layout: TextLayoutResult, topLeft: Offset, halo: Color) {
    drawText(layout, color = halo, topLeft = topLeft, drawStyle = Stroke(width = 4.dp.toPx(), join = StrokeJoin.Round))
    // A layout keeps the last draw style, so the fill has to be asked for again.
    drawText(layout, topLeft = topLeft, drawStyle = Fill)
}

/** A bar with a rounded data end and a square base. */
private fun barPath(x: Float, y: Float, w: Float, h: Float, up: Boolean, rounded: Boolean, radius: Float): Path? {
    if (h <= 0f || w <= 0f) return null
    val r = if (rounded) minOf(radius, w / 2, h) else 0f
    val corner = CornerRadius(r, r)
    return Path().apply {
        addRoundRect(if (up) RoundRect(x, y, x + w, y + h, topLeftCornerRadius = corner, topRightCornerRadius = corner)
        else RoundRect(x, y, x + w, y + h, bottomRightCornerRadius = corner, bottomLeftCornerRadius = corner))
    }
}

/** A line through [values], from [from] to [until] (exclusive), lifting the pen over gaps. */
private fun linePath(values: (Int) -> Double?, count: Int, x: (Int) -> Float, frame: Frame, from: Int = 0, until: Int = count): Path = Path().apply {
    var pen = false
    for (i in from until until) {
        val v = values(i)
        if (v == null) pen = false
        else { val px = x(i); val py = frame.y(v); if (pen) lineTo(px, py) else moveTo(px, py); pen = true }
    }
}

private class Bar(val path: Path, val color: Color, val index: Int)

private fun DrawScope.endDot(at: Offset, color: Color, surface: Color, radius: Dp = 4.dp) {
    drawCircle(surface, (radius + 1.dp).toPx(), at)
    drawCircle(color, radius.toPx(), at)
}

/** The dash the month still under way is outlined and lined with. */
private fun Density.soFarDash() = PathEffect.dashPathEffect(floatArrayOf(3.dp.toPx(), 2.5.dp.toPx()))

/**
 * What a chart draws while it moves, from what was on screen to its model: each series' values
 * (by key) and the scale. A first showing rises from zero; new figures over the same months (a
 * series hidden, by group or in total) move there; other months start again from zero.
 */
@Stable
private class Morph(private var keys: List<String>, values: Map<String, List<Double?>>, lo: Double, hi: Double, shown: Boolean) {
    private var from: Map<String, List<Double?>> = if (shown) values else values.zeroed()
    private var to: Map<String, List<Double?>> = values
    private var fromLo = lo
    private var fromHi = hi
    private var toLo = lo
    private var toHi = hi
    val progress = Animatable(if (shown) 1f else 0f)

    private fun at(t: Float, key: String, i: Int): Double? {
        val b = to[key]?.getOrNull(i) ?: return null
        val a = from[key]?.getOrNull(i) ?: 0.0
        return a + (b - a) * t
    }

    /** Reads the animation, so a drawing that asks is redrawn as it moves. */
    fun value(key: String, i: Int): Double? = at(progress.value, key, i)
    val lo: Double get() = fromLo + (toLo - fromLo) * progress.value
    val hi: Double get() = fromHi + (toHi - fromHi) * progress.value

    suspend fun go(keys: List<String>, values: Map<String, List<Double?>>, lo: Double, hi: Double) {
        val t = progress.value
        val same = keys == this.keys
        if (same) {
            from = to.mapValues { (k, v) -> v.indices.map { at(t, k, it) } }
            fromLo = fromLo + (toLo - fromLo) * t
            fromHi = fromHi + (toHi - fromHi) * t
        } else {
            from = values.zeroed()
            fromLo = lo
            fromHi = hi
        }
        this.keys = keys
        to = values
        toLo = lo
        toHi = hi
        progress.snapTo(0f)
        progress.animateTo(1f, tween(if (same) 380 else 620, easing = FastOutSlowInEasing))
    }

    private companion object {
        fun Map<String, List<Double?>>.zeroed() = mapValues { (_, v) -> v.map { if (it == null) null else 0.0 } }
    }
}

/**
 * The chart's moving figures, kept across scrolling: a chart shows its rise once, not each time it
 * comes back into view.
 */
@Composable
private fun rememberMorph(label: String, keys: List<String>, values: Map<String, List<Double?>>, lo: Double, hi: Double): Morph {
    val seen = rememberSaveable(label) { mutableStateOf(false) }
    val morph = remember { Morph(keys, values, lo, hi, seen.value) }
    LaunchedEffect(keys, values, lo, hi) {
        seen.value = true
        morph.go(keys, values, lo, hi)
    }
    return morph
}

/** A light tick under the finger at each new month. */
private fun HapticFeedback.tick() = performHapticFeedback(HapticFeedbackType.SegmentTick)

/** A legend name hidden or shown; the last one showing stays. */
private fun toggled(hidden: Set<String>, label: String, legend: List<LegendItem>): Set<String> =
    if (label in hidden) hidden - label else if (legend.count { it.label !in hidden && it.label != label } == 0) hidden else hidden + label

/**
 * Columns by month (or any keys): one series, stacked series (positives up, negatives down) or
 * grouped series side by side, with an optional overlay line and average rule. With a [legend], its
 * names hide and show their series and the scale follows.
 */
@Composable
internal fun ColumnChart(model: ColumnModel, legend: List<LegendItem> = emptyList(), modifier: Modifier = Modifier, height: Dp = 220.dp) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    var hidden by remember(model.label) { mutableStateOf(emptySet<String>()) }
    val m = remember(model, hidden) { model.without(hidden) }
    val axis = rememberAxis(m.tickLabels, measurer, palette.axis)
    val count = m.keys.size
    var selected by remember(m.label, m.keys) { mutableIntStateOf(-1) }
    val shown = if (selected in 0 until count) selected else m.opening
    val values = remember(m) { buildMap { m.series.forEach { put(it.key, it.values) }; m.line?.let { put("__line", it.values) } } }
    val morph = rememberMorph(m.label, m.keys, values, m.ticks.first(), m.ticks.last())
    val haptics = LocalHapticFeedback.current
    var width by remember { mutableIntStateOf(0) }
    Column(modifier.fillMaxWidth()) {
        if (shown >= 0) ChartReadout(m.titles[shown], m.readouts[shown], unit = m.unit)
        // Months run left to right in either language, as the chart is drawn.
        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
        Box(Modifier.fillMaxWidth().height(height).onSizeChanged { width = it.width }.semantics { contentDescription = m.label }
            .pointerInput(count, axis.width) {
                fun at(x: Float): Int {
                    val left = plotLeft(axis)
                    val band = (size.width - left - 6.dp.toPx()) / max(1, count)
                    return floor((x - left) / band).toInt().coerceIn(0, max(0, count - 1))
                }
                detectTapGestures { o -> val i = at(o.x); selected = if (i == selected) -1 else i.also { haptics.tick() } }
            }
            .pointerInput(count, axis.width) {
                fun at(x: Float): Int {
                    val left = plotLeft(axis)
                    val band = (size.width - left - 6.dp.toPx()) / max(1, count)
                    return floor((x - left) / band).toInt().coerceIn(0, max(0, count - 1))
                }
                fun pick(i: Int) { if (i != selected) { selected = i; haptics.tick() } }
                detectHorizontalDragGestures(onDragStart = { pick(at(it.x)) }) { change, _ -> change.consume(); pick(at(change.position.x)) }
            }
            .drawWithCache {
                val frame = plotFrame(size, axis, morph.lo, morph.hi)
                val n = max(1, count)
                val band = frame.width / n
                val x = { i: Int -> frame.left + band * (i + 0.5f) }
                val barMax = 24.dp.toPx()
                val gap = 2.dp.toPx()
                val radius = 4.dp.toPx()
                val y0 = frame.y(0.0)
                val bars = ArrayList<Bar>()
                fun v(key: String, i: Int) = morph.value(key, i) ?: 0.0
                if (m.grouped) {
                    val ns = max(1, m.series.size)
                    val groupWidth = min(band * 0.8f, ns * barMax + (ns - 1) * gap)
                    val each = max(2.dp.toPx(), (groupWidth - (ns - 1) * gap) / ns)
                    for (i in 0 until count) m.series.forEachIndexed { k, s ->
                        val value = v(s.key, i)
                        if (value == 0.0) return@forEachIndexed
                        val left = x(i) - groupWidth / 2 + k * (each + gap)
                        val h = abs(frame.y(value) - y0)
                        (if (value > 0) barPath(left, frame.y(value), each, h, true, true, radius) else barPath(left, y0, each, h, false, true, radius))
                            ?.let { bars.add(Bar(it, palette.color(s.color), i)) }
                    }
                } else {
                    val bw = min(barMax, max(2.dp.toPx(), band * 0.62f))
                    for (i in 0 until count) {
                        var top = y0
                        var bottom = y0
                        val stacked = m.series.filter { v(it.key, i) != 0.0 }
                        val lastUp = stacked.lastOrNull { v(it.key, i) > 0 }
                        val lastDown = stacked.lastOrNull { v(it.key, i) < 0 }
                        stacked.forEach { s ->
                            val value = v(s.key, i)
                            val h = abs(frame.y(value) - y0)
                            if (value > 0) {
                                val yTop = top - h
                                barPath(x(i) - bw / 2, yTop, bw, h - (if (top != y0) gap else 0f), true, s === lastUp, radius)?.let { bars.add(Bar(it, palette.color(s.color), i)) }
                                top = yTop
                            } else {
                                val offset = if (bottom != y0) gap else 0f
                                barPath(x(i) - bw / 2, bottom + offset, bw, h - offset, false, s === lastDown, radius)?.let { bars.add(Bar(it, palette.color(s.color), i)) }
                                bottom += h
                            }
                        }
                    }
                }
                val picked = selected.takeIf { it in 0 until count } ?: -1
                val line = m.line?.let { linePath({ i -> morph.value("__line", i) }, count, x, frame) }
                val lastLine = m.line?.values?.indexOfLast { it != null } ?: -1
                val average = m.averageText?.let { measurer.measure(it, palette.note) }
                val labels = xLabels(m.xTicks, x, measurer, palette, picked)
                val dash = soFarDash()
                onDrawBehind {
                    drawAxes(frame, m.ticks, axis, palette)
                    if (picked >= 0) {
                        drawRect(palette.hover, Offset(x(picked) - band / 2, frame.top), Size(band, frame.bottom - frame.top))
                        drawLine(palette.colors.borderStrong, Offset(x(picked), 0f), Offset(x(picked), frame.top), 1f)
                    }
                    bars.forEach { b ->
                        val alpha = when {
                            picked >= 0 && b.index != picked -> Faded
                            b.index == m.partial -> SoFar
                            else -> 1f
                        }
                        drawPath(b.path, b.color, alpha)
                        if (b.index == m.partial) drawPath(b.path, b.color, alpha = if (picked >= 0 && b.index != picked) Faded else 1f, style = Stroke(1.2.dp.toPx(), pathEffect = dash))
                    }
                    if (m.average != null) {
                        val ay = frame.y(m.average)
                        drawLine(palette.colors.text2, Offset(frame.left, ay), Offset(frame.right, ay), 1f)
                        average?.let { haloText(it, Offset(frame.right - it.size.width, ay - 4.dp.toPx() - it.size.height), palette.colors.surface) }
                    }
                    m.line?.let { overlay ->
                        val color = palette.color(overlay.color)
                        line?.let {
                            // A halo keeps the line clear of the bars it crosses.
                            drawPath(it, palette.colors.surface, style = Stroke(5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                            drawPath(it, color, style = Stroke(2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                        }
                        val dot = if (picked >= 0) picked else lastLine
                        if (dot >= 0) morph.value("__line", dot)?.let { endDot(Offset(x(dot), frame.y(it)), color, palette.colors.surface) }
                    }
                    drawXLabels(labels, frame)
                }
            }) {
            if (selected in 0 until count && width > 0) {
                val density = LocalDensity.current
                val left = with(density) { plotLeft(axis) }
                val band = (width - left - with(density) { 6.dp.toPx() }) / max(1, count)
                fun at(s: app.urbudget.nativeapp.ColumnSeries) = s.values.getOrElse(selected) { 0.0 }
                val highest = maxOf(0.0, if (m.grouped) m.series.maxOfOrNull(::at) ?: 0.0 else m.series.sumOf { max(0.0, at(it)) },
                    m.line?.values?.getOrNull(selected) ?: 0.0)
                ChartBubble(m.titles[selected], m.readouts[selected], left + band * (selected + 0.5f), band / 2f, left, width, with(density) { markY(height, m.ticks, highest) })
            }
        }
        }
        if (legend.isNotEmpty()) ChartLegend(legend, hidden = hidden, onToggle = if (legend.size > 1) ({ label -> hidden = toggled(hidden, label, legend) }) else null)
    }
}

/** Lines over months or days on one axis that includes zero; a single line gets a 10% wash. */
@Composable
internal fun LinesChart(model: LinesModel, legend: List<LegendItem> = emptyList(), modifier: Modifier = Modifier, height: Dp = 220.dp) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    var hidden by remember(model.label) { mutableStateOf(emptySet<String>()) }
    val m = remember(model, hidden) { model.without(hidden) }
    val axis = rememberAxis(m.tickLabels, measurer, palette.axis)
    val count = m.keys.size
    var selected by remember(m.label, m.keys) { mutableIntStateOf(-1) }
    val shown = if (selected in 0 until count) selected else m.opening
    val values = remember(m) { m.series.associate { it.key to it.values } }
    val morph = rememberMorph(m.label, m.keys, values, m.ticks.first(), m.ticks.last())
    val haptics = LocalHapticFeedback.current
    var width by remember { mutableIntStateOf(0) }
    fun Density.at(x: Float, width: Int): Int {
        val left = plotLeft(axis)
        val step = (width - left - 6.dp.toPx()) / max(1, count - 1)
        return if (count <= 1) 0 else ((x - left) / max(step, 1f)).roundToInt().coerceIn(0, count - 1)
    }
    Column(modifier.fillMaxWidth()) {
        if (shown >= 0) ChartReadout(m.titles[shown], m.readouts[shown], unit = m.unit)
        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
        Box(Modifier.fillMaxWidth().height(height).onSizeChanged { width = it.width }.semantics { contentDescription = m.label }
            .pointerInput(count, axis.width) { detectTapGestures { o -> val i = at(o.x, size.width); selected = if (i == selected) -1 else i.also { haptics.tick() } } }
            .pointerInput(count, axis.width) {
                fun pick(i: Int) { if (i != selected) { selected = i; haptics.tick() } }
                detectHorizontalDragGestures(onDragStart = { pick(at(it.x, size.width)) }) { change, _ -> change.consume(); pick(at(change.position.x, size.width)) }
            }
            .drawWithCache {
                val frame = plotFrame(size, axis, morph.lo, morph.hi)
                val step = frame.width / max(1, count - 1)
                val x = { i: Int -> frame.left + if (count == 1) frame.width / 2 else step * i }
                val y0 = frame.y(0.0)
                val dash = PathEffect.dashPathEffect(floatArrayOf(6.dp.toPx(), 4.dp.toPx()))
                val soFar = soFarDash()
                val partial = m.partial
                val split = if (partial > 0) partial else count
                val strokes = m.series.map { s -> linePath({ morph.value(s.key, it) }, count, x, frame, 0, split) }
                // The month still under way: its stretch of line dashed.
                val tails = m.series.map { s -> if (partial > 0) linePath({ morph.value(s.key, it) }, count, x, frame, partial - 1, partial + 1) else null }
                val washes = if (!m.wash) emptyList() else m.series.map { s ->
                    Path().apply {
                        var start = -1
                        var end = -1
                        val path = this
                        fun finish() { if (start >= 0) { path.lineTo(x(end), y0); path.lineTo(x(start), y0); path.close() } }
                        for (i in 0 until count) {
                            val v = morph.value(s.key, i)
                            if (v == null) { finish(); start = -1 }
                            else { if (start < 0) { start = i; moveTo(x(i), frame.y(v)) } else lineTo(x(i), frame.y(v)); end = i }
                        }
                        finish()
                    }
                }
                val ends = m.series.mapIndexedNotNull { k, s ->
                    val i = s.values.indexOfLast { it != null }
                    if (i < 0) null else Triple(k, i, frame.y(morph.value(s.key, i) ?: 0.0))
                }
                val picked = selected.takeIf { it in 0 until count } ?: -1
                val endLabels = if (picked >= 0) emptyList() else ends.mapNotNull { (k, i, py) -> m.endLabels.getOrNull(k)?.let { Triple(measurer.measure(it, palette.note), x(i), py - 10.dp.toPx()) } }
                    .sortedBy { it.third }.toMutableList().also { list -> for (k in 1 until list.size) list[k] = list[k].copy(third = max(list[k].third, list[k - 1].third + 14.dp.toPx())) }
                val labels = xLabels(m.xTicks, x, measurer, palette, picked)
                onDrawBehind {
                    drawAxes(frame, m.ticks, axis, palette)
                    if (picked >= 0) drawLine(palette.colors.borderStrong, Offset(x(picked), 0f), Offset(x(picked), frame.bottom), 1f)
                    m.series.forEachIndexed { k, s ->
                        val color = palette.color(s.color)
                        washes.getOrNull(k)?.let { drawPath(it, color.copy(alpha = .1f)) }
                        val style = Stroke(2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round, pathEffect = if (s.dashed) dash else null)
                        drawPath(strokes[k], color, style = style)
                        tails[k]?.let { drawPath(it, color, style = Stroke(2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round, pathEffect = soFar)) }
                    }
                    if (picked >= 0) m.series.forEach { s -> morph.value(s.key, picked)?.let { endDot(Offset(x(picked), frame.y(it)), palette.color(s.color), palette.colors.surface, 5.dp) } }
                    else ends.forEach { (k, i, py) -> endDot(Offset(x(i), py), palette.color(m.series[k].color), palette.colors.surface) }
                    endLabels.forEach { (layout, px, top) -> haloText(layout, Offset(px - 8.dp.toPx() - layout.size.width, top - layout.size.height / 2f), palette.colors.surface) }
                    drawXLabels(labels, frame)
                }
            }) {
            if (selected in 0 until count && width > 0) {
                val density = LocalDensity.current
                val left = with(density) { plotLeft(axis) }
                val step = (width - left - with(density) { 6.dp.toPx() }) / max(1, count - 1)
                val highest = m.series.mapNotNull { it.values.getOrNull(selected) }.maxOrNull() ?: 0.0
                ChartBubble(m.titles[selected], m.readouts[selected], if (count == 1) left + (width - left) / 2 else left + step * selected,
                    with(density) { 6.dp.toPx() }, left, width, with(density) { markY(height, m.ticks, highest) })
            }
        }
        }
        if (legend.isNotEmpty()) ChartLegend(legend, hidden = hidden, onToggle = if (legend.size > 1) ({ label -> hidden = toggled(hidden, label, legend) }) else null)
    }
}

/**
 * Parts of a whole as a ring, largest first, the total in the middle. A tap picks a slice: it pulls
 * out, the others fade and the middle says its amount and share; a second tap on it, or on the
 * middle, opens it ([onOpen]). A tap outside the ring lets go.
 */
@Composable
internal fun Donut(slices: List<SliceModel>, centerValue: String, centerLabel: String, picked: String?, onPick: (String?) -> Unit, onOpen: ((String) -> Unit)?,
                   modifier: Modifier = Modifier, size: Dp = 200.dp, openLabel: String = stringResource(R.string.chart_see_transactions)) {
    val palette = reportPalette()
    val haptics = LocalHapticFeedback.current
    val shown = remember(slices) { slices.filter { it.value > 0 } }
    val angles = remember(shown) {
        val total = shown.sumOf { it.value }.toDouble()
        var a = 0.0
        shown.map { s -> val a0 = a; a += s.value / total * 360.0; a0 to a }
    }
    val chosen = shown.indexOfFirst { it.key == picked }
    val pull = remember(picked) { Animatable(0f) }
    LaunchedEffect(picked) { pull.animateTo(1f, spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMediumLow)) }
    val grow = rememberSaveable { mutableStateOf(false) }
    val sweep = remember { Animatable(if (grow.value) 1f else 0f) }
    LaunchedEffect(Unit) { grow.value = true; sweep.animateTo(1f, tween(700, easing = FastOutSlowInEasing)) }
    Box(modifier.size(size)
        .pointerInput(shown, picked, onOpen) {
            detectTapGestures { o ->
                val c = this.size.width / 2f
                val outer = c - 8.dp.toPx()
                val r = hypot(o.x - c, o.y - c)
                if (r < outer * 0.62f) { if (picked != null && picked == shown.getOrNull(chosen)?.key) onOpen?.invoke(picked); return@detectTapGestures }
                if (r > outer + 8.dp.toPx()) { onPick(null); return@detectTapGestures }
                var deg = Math.toDegrees(atan2((o.x - c).toDouble(), (c - o.y).toDouble()))
                if (deg < 0) deg += 360.0
                val i = angles.indexOfFirst { deg >= it.first && deg < it.second }
                if (i < 0) return@detectTapGestures
                val key = shown[i].key
                if (key == picked) { if (onOpen != null) onOpen(key) else onPick(null) } else { haptics.tick(); onPick(key) }
            }
        }
        .drawWithCache {
            val c = this.size.minDimension / 2
            val middle = Offset(this.size.width / 2, this.size.height / 2)
            val outer = c - 8.dp.toPx()
            val inner = outer * 0.62f
            val mid = (outer + inner) / 2
            val ring = Stroke(outer - inner)
            val topLeft = Offset(middle.x - mid, middle.y - mid)
            val arc = Size(mid * 2, mid * 2)
            // A 2dp surface gap between slices, as an angle at the ring's middle.
            val gapDeg = if (shown.size > 1) Math.toDegrees((2.dp.toPx() / mid).toDouble()).toFloat() else 0f
            val t = sweep.value
            val out = 6.dp.toPx() * pull.value
            onDrawBehind {
                if (shown.isEmpty()) drawCircle(palette.colors.surface2, mid, middle, style = ring)
                shown.forEachIndexed { i, s ->
                    val (a0, a1) = angles[i]
                    val start = (a0 * t).toFloat()
                    val span = ((a1 - a0) * t).toFloat() - gapDeg
                    if (span <= 0f) return@forEachIndexed
                    val color = palette.color(s.color)
                    if (i == chosen) {
                        val rad = Math.toRadians((a0 + a1) / 2 * t)
                        translate(sin(rad).toFloat() * out, -cos(rad).toFloat() * out) {
                            drawArc(color, start - 90 + gapDeg / 2, span, false, topLeft, arc, style = Stroke(outer - inner + 4.dp.toPx() * pull.value))
                        }
                    } else drawArc(color, start - 90 + gapDeg / 2, span, false, topLeft, arc, alpha = if (chosen >= 0) Faded else 1f, style = ring)
                }
            }
        }, contentAlignment = Alignment.Center) {
        val slice = shown.getOrNull(chosen)
        // The total, or the picked slice, shrinks to fit inside the ring.
        Column(Modifier.widthIn(max = size * 0.52f), horizontalAlignment = Alignment.CenterHorizontally) {
            BasicText(slice?.valueText?.ifEmpty { null } ?: centerValue, style = Tabular.merge(TextStyle(color = ur.text, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center)),
                maxLines = 1, autoSize = TextAutoSize.StepBased(minFontSize = 10.sp, maxFontSize = 17.sp, stepSize = 0.5.sp))
            Text(slice?.let { Money.own(it.label) } ?: centerLabel, color = if (slice != null) ur.text2 else ur.muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.Center)
            if (slice != null && slice.share.isNotEmpty()) Text(slice.share, color = ur.muted, fontSize = 12.sp, maxLines = 1, style = Tabular)
            if (slice != null && onOpen != null && slice.key != "__other") Text(openLabel, Modifier.padding(top = 2.dp), color = ur.accent, fontSize = 12.sp,
                fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
        }
    }
}

/**
 * A ranked bar under its name, amount and share. While a slice or row is picked ([picked] is
 * false for the others), the others fade and the picked one is marked.
 */
@Composable
internal fun BarRowView(row: BarRowModel, defaultColor: Int, onClick: (() -> Unit)?, picked: Boolean? = null) {
    val palette = reportPalette()
    Column(Modifier.fillMaxWidth().background(if (picked == true) ur.surface2 else Color.Transparent)
        .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier).padding(horizontal = 16.dp, vertical = 9.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp)) {
        val fade = if (picked == false) .45f else 1f
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (row.color != null) { Box(Modifier.size(10.dp).clip(RoundedCornerShape(2.dp)).background(palette.color(row.color).copy(alpha = fade))); Spacer(Modifier.width(8.dp)) }
            Text(Money.own(row.label), Modifier.weight(1f), color = ur.text.copy(alpha = fade), style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(row.valueText, Modifier.padding(start = 8.dp), color = ur.text.copy(alpha = fade), style = MaterialTheme.typography.bodyMedium.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1)
            row.detail?.let { Text(it, Modifier.widthIn(min = 52.dp), color = ur.muted.copy(alpha = fade), style = MaterialTheme.typography.bodySmall.merge(Tabular), textAlign = TextAlign.End, maxLines = 1) }
            if (picked == true && onClick != null) Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, stringResource(R.string.chart_see_transactions), Modifier.padding(start = 2.dp).size(18.dp), tint = ur.accent)
        }
        Box(Modifier.fillMaxWidth().height(10.dp)) {
            if (row.fraction > 0f) Box(Modifier.fillMaxHeight().fillMaxWidth(row.fraction.coerceIn(0f, 1f))
                .clip(RoundedCornerShape(topEnd = 4.dp, bottomEnd = 4.dp)).background(palette.color(row.color ?: defaultColor).copy(alpha = fade)))
        }
    }
}

/** A change either side of a centre line: less to the left, more to the right. */
@Composable
internal fun DivergingRowView(row: DivRowModel) {
    val palette = reportPalette()
    val color = palette.color(row.color)
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 9.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(Money.own(row.label), Modifier.weight(1f), color = ur.text, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(row.valueText, Modifier.padding(start = 8.dp), color = ur.text, style = MaterialTheme.typography.bodyMedium.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1)
            Text(row.detail, Modifier.padding(start = 8.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall.merge(Tabular), maxLines = 1)
        }
        // A number line: less to the left and more to the right in either language, as the charts draw.
        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) { Row(Modifier.fillMaxWidth().height(14.dp)) {
            Box(Modifier.weight(1f).fillMaxHeight(), contentAlignment = Alignment.CenterEnd) {
                if (row.fraction < 0f) Box(Modifier.fillMaxHeight().fillMaxWidth(-row.fraction).clip(RoundedCornerShape(topStart = 4.dp, bottomStart = 4.dp)).background(color))
            }
            Box(Modifier.width(1.dp).fillMaxHeight().background(ur.borderStrong))
            Box(Modifier.weight(1f).fillMaxHeight(), contentAlignment = Alignment.CenterStart) {
                if (row.fraction > 0f) Box(Modifier.fillMaxHeight().fillMaxWidth(row.fraction).clip(RoundedCornerShape(topEnd = 4.dp, bottomEnd = 4.dp)).background(color))
            }
        } }
    }
}

/**
 * Days a month at a time, as a calendar: each day shaded in five steps by how much was spent, its
 * date in the corner. Swipe or use the arrows for other months; it opens on the latest. Days
 * outside the report's range are left plain.
 */
@Composable
internal fun MonthCalendar(model: HeatModel, selected: Int, onDay: (Int) -> Unit, modifier: Modifier = Modifier) {
    val palette = reportPalette()
    val months = remember(model) { model.days.map { it.take(7) }.distinct() }
    val index = remember(model) { model.days.withIndex().associate { (i, d) -> d to i } }
    // The week starts where the report's does: the first day's weekday less its place in the week.
    val weekStart = remember(model) { model.days.firstOrNull()?.let { (jsWeekday(LocalDate.parse(it)) - model.lead + 7) % 7 } ?: 0 }
    val pager = rememberPagerState(initialPage = max(0, months.lastIndex)) { months.size }
    val scope = rememberCoroutineScope()
    LaunchedEffect(selected) {
        model.days.getOrNull(selected)?.take(7)?.let { months.indexOf(it) }?.takeIf { it >= 0 && it != pager.currentPage }?.let { pager.animateScrollToPage(it) }
    }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton({ scope.launch { pager.animateScrollToPage(pager.currentPage - 1) } }, enabled = pager.currentPage > 0) {
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowLeft, stringResource(R.string.month_previous), tint = if (pager.currentPage > 0) ur.accent else ur.border)
            }
            Text(months.getOrNull(pager.currentPage)?.let { reportMonthName(it) }.orEmpty(), Modifier.weight(1f), color = ur.text,
                style = MaterialTheme.typography.titleSmall, textAlign = TextAlign.Center, maxLines = 1)
            IconButton({ scope.launch { pager.animateScrollToPage(pager.currentPage + 1) } }, enabled = pager.currentPage < months.lastIndex) {
                Icon(Icons.AutoMirrored.Outlined.KeyboardArrowRight, stringResource(R.string.month_next), tint = if (pager.currentPage < months.lastIndex) ur.accent else ur.border)
            }
        }
        Row(Modifier.fillMaxWidth()) {
            (0 until 7).forEach { k -> Text(weekdayName((k + weekStart) % 7, short = true), Modifier.weight(1f), color = ur.muted, fontSize = 11.sp, textAlign = TextAlign.Center, maxLines = 1) }
        }
        HorizontalPager(pager, Modifier.fillMaxWidth(), verticalAlignment = Alignment.Top, beyondViewportPageCount = 1) { page ->
            val month = YearMonth.parse(months[page])
            val lead = (jsWeekday(month.atDay(1)) - weekStart + 7) % 7
            val cells = List(lead) { null } + (1..month.lengthOfMonth()).map { month.atDay(it).toString() }
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                cells.chunked(7).forEach { week ->
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        (0 until 7).forEach { k ->
                            val day = week.getOrNull(k)
                            val i = day?.let { index[it] }
                            Box(Modifier.weight(1f).aspectRatio(1.15f)) {
                                if (day != null) DayCell(day.takeLast(2).trimStart('0'), i?.let { model.levels[it] }, i != null && i == selected, palette,
                                    if (i != null) ({ onDay(i) }) else null)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DayCell(date: String, level: Int?, picked: Boolean, palette: ReportPalette, onClick: (() -> Unit)?) {
    val shape = RoundedCornerShape(8.dp)
    val fill = if (level == null) Color.Transparent else palette.heat[level.coerceIn(0, 5)]
    val ink = if (level == null) ur.muted.copy(alpha = .55f) else palette.heatInk(level.coerceIn(0, 5))
    Box(Modifier.fillMaxSize().clip(shape).background(fill)
        .then(if (level == 0) Modifier.border(1.dp, ur.border, shape) else Modifier)
        .then(if (picked) Modifier.border(2.dp, ur.text, shape) else Modifier)
        .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
        .padding(horizontal = 5.dp, vertical = 3.dp)) {
        Text(date, color = ink, fontSize = 11.sp, fontWeight = if (picked) FontWeight.Bold else FontWeight.Medium, style = Tabular)
    }
}

/** The calendar's key: an empty day, then less to more. */
@Composable
internal fun HeatLegend() {
    val palette = reportPalette()
    Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        HeatSwatch(palette, 0)
        Text(stringResource(R.string.chart_no_spending), Modifier.padding(start = 4.dp), color = ur.text2, fontSize = 12.sp)
        Spacer(Modifier.weight(1f))
        Text(stringResource(R.string.chart_less), Modifier.padding(end = 4.dp), color = ur.text2, fontSize = 12.sp)
        (1..5).forEach { HeatSwatch(palette, it); Spacer(Modifier.width(3.dp)) }
        Text(stringResource(R.string.chart_more), color = ur.text2, fontSize = 12.sp)
    }
}

@Composable
private fun HeatSwatch(palette: ReportPalette, level: Int) {
    Box(Modifier.size(11.dp).clip(RoundedCornerShape(2.dp)).background(palette.heat[level])
        .then(if (level == 0) Modifier.border(1.dp, ur.border, RoundedCornerShape(2.dp)) else Modifier))
}

/**
 * Tiles with areas in proportion to their values, each in its group's colour; labels where they fit.
 * A tap picks a tile (the others fade); a second tap opens it.
 */
@Composable
internal fun Treemap(items: List<TreemapItem>, picked: String?, onPick: (TreemapItem?) -> Unit, onOpen: (TreemapItem) -> Unit, modifier: Modifier = Modifier,
                     height: Dp = 280.dp, label: String = stringResource(R.string.chart_spending_by_payee)) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    val haptics = LocalHapticFeedback.current
    var box by remember { mutableStateOf(IntSize.Zero) }
    val sorted = remember(items) { items.filter { it.value > 0 }.sortedByDescending { it.value } }
    val tiles = remember(sorted, box) { squarify(sorted, { it.value.toDouble() }, box.width.toDouble(), box.height.toDouble()) }
    val labelStyle = remember(palette) { TextStyle(color = palette.colors.text, fontSize = 12.5.sp, fontWeight = FontWeight.SemiBold) }
    val valueStyle = remember(palette) { TextStyle(color = palette.colors.text, fontSize = 12.5.sp, fontFeatureSettings = "tnum") }
    CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
    Box(modifier.fillMaxWidth().height(height).onSizeChanged { box = it }.semantics { contentDescription = label }
        .pointerInput(tiles, picked) {
            detectTapGestures { o ->
                val hit = tiles.firstOrNull { o.x >= it.x && o.x < it.x + it.w && o.y >= it.y && o.y < it.y + it.h }?.item
                when {
                    hit == null -> onPick(null)
                    hit.key == picked -> onOpen(hit)
                    else -> { haptics.tick(); onPick(hit) }
                }
            }
        }
        .drawWithCache {
            val padX = 8.dp.toPx()
            val padY = 6.dp.toPx()
            val texts = tiles.map { t ->
                val labelled = t.w > 44.dp.toPx() && t.h > 22.dp.toPx()
                val roomy = t.w > 76.dp.toPx() && t.h > 40.dp.toPx()
                val width = Constraints(maxWidth = (t.w - 2 * padX).toInt().coerceAtLeast(1))
                (if (labelled) measurer.measure(Money.own(t.item.label), labelStyle, TextOverflow.Ellipsis, maxLines = 1, constraints = width) else null) to
                    (if (roomy) measurer.measure(t.item.valueText, valueStyle, TextOverflow.Ellipsis, maxLines = 1, constraints = width) else null)
            }
            val corner = CornerRadius(4.dp.toPx())
            onDrawBehind {
                tiles.forEachIndexed { i, t ->
                    // A 2dp surface gap between tiles.
                    val origin = Offset(t.x.toFloat() + 1.dp.toPx(), t.y.toFloat() + 1.dp.toPx())
                    val area = Size(max(0f, t.w.toFloat() - 2.dp.toPx()), max(0f, t.h.toFloat() - 2.dp.toPx()))
                    val faded = picked != null && t.item.key != picked
                    drawRoundRect(palette.tile(palette.color(t.item.color)), origin, area, corner, alpha = if (faded) .35f else 1f)
                    if (t.item.key == picked) drawRoundRect(palette.colors.text, origin, area, corner, style = Stroke(2.dp.toPx()))
                    val (name, value) = texts[i]
                    if (name != null || value != null) clipRect(origin.x, origin.y, origin.x + area.width, origin.y + area.height) {
                        name?.let { drawText(it, topLeft = Offset(origin.x + padX - 1, origin.y + padY - 1), alpha = if (faded) .5f else 1f) }
                        value?.let { drawText(it, topLeft = Offset(origin.x + padX - 1, origin.y + padY - 1 + (name?.size?.height ?: 0) + 2.dp.toPx()), alpha = if (faded) .5f else 1f) }
                    }
                }
            }
        })
    }
}

/**
 * Small columns, one group a month, scaled to the largest value; negatives hang below the line in
 * red. The highest and latest columns say their figure (with [MiniBarsModel.short]). With
 * [onSelect], a finger picks a month (pressing, dragging or tapping) and the others fade.
 */
@Composable
internal fun MiniBars(model: MiniBarsModel, modifier: Modifier = Modifier, selected: Int = -1, onSelect: ((Int) -> Unit)? = null) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    val haptics = LocalHapticFeedback.current
    val legend = remember(model) { if (model.series.size > 1) model.series.map { LegendItem(it.label, it.color) } else emptyList() }
    val both = stringResource(R.string.chart_and)
    val description = stringResource(R.string.chart_by_month, model.series.map { it.label }.reduceOrNull { a, b -> both.format(a, b) }.orEmpty())
    val n = model.labels.size
    val seen = rememberSaveable { mutableStateOf(false) }
    val grow = remember { Animatable(if (seen.value) 1f else 0f) }
    LaunchedEffect(Unit) { seen.value = true; grow.animateTo(1f, tween(600, easing = FastOutSlowInEasing)) }
    val labelled = model.series.size == 1 && model.short.isNotEmpty()
    val current by rememberUpdatedState(selected)
    Column(modifier.fillMaxWidth()) {
    // Months run left to right in either language, so each label stays under its columns.
    CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
    Column(Modifier.fillMaxWidth().height(if (labelled) 150.dp else 136.dp).padding(top = 6.dp)) {
        Box(Modifier.fillMaxWidth().weight(1f).semantics { contentDescription = description }
            .then(if (onSelect == null) Modifier else Modifier
                .pointerInput(n) {
                    fun at(x: Float) = floor(x / (size.width / max(1, n).toFloat())).toInt().coerceIn(0, max(0, n - 1))
                    detectTapGestures { o -> val i = at(o.x); onSelect(if (i == current) -1 else i.also { haptics.tick() }) }
                }
                .pointerInput(n) {
                    fun at(x: Float) = floor(x / (size.width / max(1, n).toFloat())).toInt().coerceIn(0, max(0, n - 1))
                    var last = -1
                    detectHorizontalDragGestures(onDragStart = { last = current; val i = at(it.x); if (i != last) { last = i; haptics.tick(); onSelect(i) } }) { change, _ ->
                        change.consume()
                        val i = at(change.position.x)
                        if (i != last) { last = i; haptics.tick(); onSelect(i) }
                    }
                })
            .drawWithCache {
                val count = max(1, n)
                val colGap = 6.dp.toPx()
                val colWidth = (size.width - colGap * (count - 1)) / count
                val headroom = if (labelled) 16.dp.toPx() else 0f
                val range = (model.top - model.bottom).takeIf { it != 0L } ?: 1L
                val plot = size.height - headroom
                val upHeight = plot * model.top / range
                val downHeight = plot * -model.bottom / range
                val base = headroom + upHeight
                val ns = max(1, model.series.size)
                val barGap = 3.dp.toPx()
                val barWidth = min(22.dp.toPx(), (colWidth - barGap * (ns - 1)) / ns)
                val radius = 4.dp.toPx()
                val g = grow.value
                val bars = ArrayList<Bar>()
                val tops = FloatArray(n) { base }
                for (i in 0 until n) {
                    val colLeft = i * (colWidth + colGap)
                    val groupWidth = barWidth * ns + barGap * (ns - 1)
                    model.series.forEachIndexed { k, s ->
                        val v = s.values.getOrElse(i) { 0L }
                        val left = colLeft + (colWidth - groupWidth) / 2 + k * (barWidth + barGap)
                        if (v > 0 && model.top > 0) {
                            val h = upHeight * v / model.top * g
                            barPath(left, base - h, barWidth, h, true, true, radius)?.let { bars.add(Bar(it, palette.color(s.color), i)) }
                            tops[i] = min(tops[i], base - h)
                        } else if (v < 0 && model.bottom < 0) {
                            val h = downHeight * v / model.bottom * g
                            barPath(left, base, barWidth, h, false, true, radius)?.let { bars.add(Bar(it, palette.colors.negative, i)) }
                        }
                    }
                }
                // The highest column's figure and the latest one's, over them.
                val marks = if (!labelled || n == 0) emptyList() else {
                    val values = model.series[0].values
                    val high = values.indices.maxByOrNull { values[it] } ?: -1
                    val latest = n - 1
                    listOf(high, latest).distinct().filter { it in 0 until n && model.short[0].getOrNull(it).orEmpty().isNotEmpty() && values[it] > 0 }
                        .map { i -> Triple(i, measurer.measure(model.short[0][i], palette.value), tops[i]) }
                }
                val picked = selected.takeIf { it in 0 until n } ?: -1
                val dash = soFarDash()
                onDrawBehind {
                    bars.forEach { b ->
                        val alpha = when {
                            picked >= 0 && b.index != picked -> Faded
                            b.index == model.partial -> SoFar
                            else -> 1f
                        }
                        drawPath(b.path, b.color, alpha)
                        if (b.index == model.partial) drawPath(b.path, b.color, alpha = if (picked >= 0 && b.index != picked) Faded else 1f, style = Stroke(1.2.dp.toPx(), pathEffect = dash))
                    }
                    for (i in 0 until n) {
                        val colLeft = i * (colWidth + colGap)
                        drawLine(palette.colors.borderStrong, Offset(colLeft, base), Offset(colLeft + colWidth, base), 1f)
                    }
                    if (picked < 0) marks.forEach { (i, layout, top) ->
                        val cx = i * (colWidth + colGap) + colWidth / 2
                        drawText(layout, topLeft = Offset((cx - layout.size.width / 2f).coerceIn(0f, max(0f, size.width - layout.size.width)), top - layout.size.height - 2.dp.toPx()), alpha = g)
                    }
                }
            })
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            model.labels.forEachIndexed { i, label ->
                Text(label, Modifier.weight(1f), color = if (i == selected) ur.text else ur.text2, fontSize = 12.5.sp,
                    fontWeight = if (i == selected) FontWeight.Bold else FontWeight.Normal, textAlign = TextAlign.Center, maxLines = 1)
            }
        }
    }
    }
    // Two or more series say which colour is which.
    if (legend.isNotEmpty()) ChartLegend(legend)
    }
}

/** The month's spending as one bar, split by category in the list's colours. */
@Composable
internal fun SegmentBar(rows: List<SpendRow>, modifier: Modifier = Modifier) {
    val palette = reportPalette()
    Row(modifier.fillMaxWidth().padding(vertical = 6.dp).height(28.dp).clip(RoundedCornerShape(8.dp)), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        rows.forEach { row -> Box(Modifier.weight(max(1f, row.weight.toFloat())).fillMaxHeight().background(palette.color(row.color))) }
    }
}

/** A report's headline figure. */
@Composable
internal fun TileCard(tile: Tile, modifier: Modifier = Modifier) {
    val shape = RoundedCornerShape(16.dp)
    Column(modifier.clip(shape).background(ur.surface).then(if (ur.dark) Modifier.border(1.dp, ur.border, shape) else Modifier)
        .padding(horizontal = 14.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(tile.label, color = ur.muted, fontSize = 12.sp, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Text(tile.value, color = when (tile.tone) { Tone.Negative -> ur.negative; Tone.Positive -> ur.positive; Tone.None -> ur.text },
            fontSize = 18.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        tile.note?.let { Text(it, color = ur.muted, fontSize = 12.sp, lineHeight = 16.sp) }
    }
}

/** Two tiles side by side, as tall as the taller. */
@Composable
internal fun TilePair(tiles: List<Tile>, modifier: Modifier = Modifier) {
    Row(modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        tiles.forEach { TileCard(it, Modifier.weight(1f).fillMaxHeight()) }
        if (tiles.size == 1) Spacer(Modifier.weight(1f))
    }
}
