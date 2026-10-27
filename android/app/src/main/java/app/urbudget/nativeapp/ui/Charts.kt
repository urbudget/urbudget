package app.urbudget.nativeapp.ui

import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
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
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.LayoutDirection
import app.urbudget.nativeapp.BarRowModel
import app.urbudget.nativeapp.Money
import app.urbudget.nativeapp.R
import app.urbudget.nativeapp.ColumnModel
import app.urbudget.nativeapp.DivRowModel
import app.urbudget.nativeapp.HeatModel
import app.urbudget.nativeapp.LegendItem
import app.urbudget.nativeapp.LinesModel
import app.urbudget.nativeapp.MiniBarsModel
import app.urbudget.nativeapp.Readout
import app.urbudget.nativeapp.ReportColors
import app.urbudget.nativeapp.SliceModel
import app.urbudget.nativeapp.SpendRow
import app.urbudget.nativeapp.Tile
import app.urbudget.nativeapp.Tone
import app.urbudget.nativeapp.TreemapItem
import app.urbudget.nativeapp.XTick
import app.urbudget.nativeapp.squarify
import kotlinx.coroutines.flow.first
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
 * 2dp lines, end dots with a surface ring, hairline grids, labels in the muted text colour. A tap
 * picks a column or point and a readout above the chart lists its figures, where the desktop
 * shows a hover tooltip. Every label arrives formatted in the model; geometry is cached by size.
 */

private val LightSeries = listOf(Color(0xFF2A78D6), Color(0xFFEB6834), Color(0xFF1BAF7A), Color(0xFFEDA100), Color(0xFFE87BA4), Color(0xFF008300), Color(0xFF6250D6), Color(0xFFE34948))
private val DarkSeries = listOf(Color(0xFF3987E5), Color(0xFFD95926), Color(0xFF199E70), Color(0xFFC98500), Color(0xFFD55181), Color(0xFF008300), Color(0xFF9085E9), Color(0xFFE66767))

/** The desktop's report palette (reports.css) for the theme in use, and the theme's own roles. */
@Immutable
internal class ReportPalette(val colors: UrColors) {
    val series = if (colors.dark) DarkSeries else LightSeries
    val other = if (colors.dark) Color(0xFF5D636C) else Color(0xFFA3A8B0)
    /** Heat levels 0 (nothing spent) to 5: series-1 mixed into the surface, as color-mix in oklab does. */
    val heat = listOf(colors.surface2) + listOf(.14f, .30f, .48f, .70f).map { lerp(colors.surface, series[0], it) } + series[0]
    /** Treemap tiles: levels 0 and 1 share the lightest. */
    val tiles = listOf(.10f, .10f, .20f, .30f, .42f, .56f).map { lerp(colors.surface, series[0], it) }
    val hover = colors.text.copy(alpha = if (colors.dark) .08f else .05f)
    val axis = TextStyle(color = colors.muted, fontSize = 11.sp, fontFeatureSettings = "tnum")
    val note = TextStyle(color = colors.text2, fontSize = 11.sp, fontFeatureSettings = "tnum")

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

/** A swatch matching the mark: a box for bars, a short line for lines (dashed for dashed lines). */
@Composable
internal fun LegendKey(color: Color, line: Boolean = false, dashed: Boolean = false) {
    if (!line) Box(Modifier.size(10.dp).clip(RoundedCornerShape(2.dp)).background(color))
    else Canvas(Modifier.size(14.dp, 3.dp)) {
        drawLine(color, Offset(0f, size.height / 2), Offset(size.width, size.height / 2), 2.dp.toPx(), StrokeCap.Round,
            if (dashed) PathEffect.dashPathEffect(floatArrayOf(3.dp.toPx(), 2.5.dp.toPx())) else null)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ChartLegend(items: List<LegendItem>, modifier: Modifier = Modifier) {
    val palette = reportPalette()
    FlowRow(modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        items.forEach { item ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                LegendKey(palette.color(item.color), item.line, item.dashed)
                Spacer(Modifier.width(6.dp))
                Text(Money.own(item.label), color = ur.text2, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** The figures of the chosen column or point, where the desktop shows its tooltip. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ChartReadout(title: String, rows: List<Readout>, modifier: Modifier = Modifier) {
    val palette = reportPalette()
    Column(modifier.fillMaxWidth().heightIn(min = 40.dp)) {
        Text(title, color = ur.muted, style = MaterialTheme.typography.bodySmall, maxLines = 1)
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

/** The tick labels of a y axis, measured once per model. */
private class AxisLabels(val ticks: List<TextLayoutResult>, val unit: TextLayoutResult?) {
    val width = max(ticks.maxOfOrNull { it.size.width } ?: 0, unit?.size?.width ?: 0)
}

@Composable
private fun rememberAxis(labels: List<String>, unit: String?, measurer: TextMeasurer, style: TextStyle): AxisLabels =
    remember(labels, unit, style) { AxisLabels(labels.map { measurer.measure(it, style) }, unit?.let { measurer.measure(it, style) }) }

/** The plot area in pixels and its value scale. */
private class Frame(val left: Float, val top: Float, val right: Float, val bottom: Float, val lo: Double, val hi: Double) {
    val width get() = right - left
    fun y(v: Double): Float = if (hi == lo) bottom else (bottom - (v - lo) / (hi - lo) * (bottom - top)).toFloat()
}

private fun Density.plotFrame(size: Size, axis: AxisLabels, ticks: List<Double>, unit: Boolean) = Frame(
    left = axis.width + 8.dp.toPx(), top = (if (unit) 18.dp else 10.dp).toPx(), right = size.width - 6.dp.toPx(),
    bottom = size.height - 22.dp.toPx(), lo = ticks.first(), hi = ticks.last(),
)

private fun DrawScope.drawAxes(frame: Frame, ticks: List<Double>, axis: AxisLabels, palette: ReportPalette) {
    val gap = 6.dp.toPx()
    axis.unit?.let { drawText(it, topLeft = Offset(frame.left - gap - it.size.width, 0f)) }
    ticks.forEachIndexed { i, t ->
        val y = frame.y(t)
        drawLine(if (t == 0.0) palette.colors.borderStrong else palette.colors.border, Offset(frame.left, y), Offset(frame.right, y), 1f)
        val label = axis.ticks.getOrNull(i) ?: return@forEachIndexed
        drawText(label, topLeft = Offset(frame.left - gap - label.size.width, y - label.size.height / 2f))
    }
}

/** X labels that never crowd: each needs 44dp from the last one drawn; the year shows when it changes. */
private fun Density.xLabels(ticks: List<XTick?>, x: (Int) -> Float, measurer: TextMeasurer, style: TextStyle): List<Pair<Float, TextLayoutResult>> {
    val out = ArrayList<Pair<Float, TextLayoutResult>>()
    var last = Float.NEGATIVE_INFINITY
    var lastYear: Int? = null
    val spacing = 44.dp.toPx()
    ticks.forEachIndexed { i, tick ->
        if (tick == null) return@forEachIndexed
        val at = x(i)
        if (at - last < spacing) return@forEachIndexed
        out.add(at to measurer.measure(if (lastYear == null || lastYear != tick.year) tick.long else tick.short, style))
        last = at
        lastYear = tick.year
    }
    return out
}

private fun DrawScope.drawXLabels(labels: List<Pair<Float, TextLayoutResult>>, frame: Frame) {
    labels.forEach { (x, layout) -> drawText(layout, topLeft = Offset(x - layout.size.width / 2f, frame.bottom + 5.dp.toPx())) }
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

private fun linePath(values: List<Double?>, x: (Int) -> Float, frame: Frame): Path = Path().apply {
    var pen = false
    values.forEachIndexed { i, v ->
        if (v == null) pen = false
        else { val px = x(i); val py = frame.y(v); if (pen) lineTo(px, py) else moveTo(px, py); pen = true }
    }
}

private class Bar(val path: Path, val color: Color)

private fun DrawScope.endDot(at: Offset, color: Color, surface: Color) {
    drawCircle(surface, 5.dp.toPx(), at)
    drawCircle(color, 4.dp.toPx(), at)
}

/**
 * Columns by month (or any keys): one series, stacked series (positives up, negatives down) or
 * grouped series side by side, with an optional overlay line and average rule.
 */
@Composable
internal fun ColumnChart(model: ColumnModel, modifier: Modifier = Modifier, height: Dp = 220.dp) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    val axis = rememberAxis(model.tickLabels, model.unit, measurer, palette.axis)
    var selected by remember(model) { mutableIntStateOf(-1) }
    val count = model.keys.size
    val shown = if (selected in 0 until count) selected else count - 1
    Column(modifier.fillMaxWidth()) {
        if (shown >= 0) ChartReadout(model.titles[shown], model.readouts[shown])
        Box(Modifier.fillMaxWidth().height(height).semantics { contentDescription = model.label }
            .pointerInput(model, axis.width) {
                detectTapGestures { o ->
                    val left = axis.width + 8.dp.toPx()
                    val band = (size.width - left - 6.dp.toPx()) / max(1, count)
                    val i = floor((o.x - left) / band).toInt()
                    selected = if (i !in 0 until count || i == selected) -1 else i
                }
            }
            .drawWithCache {
                val frame = plotFrame(size, axis, model.ticks, model.unit != null)
                val n = max(1, count)
                val band = frame.width / n
                val x = { i: Int -> frame.left + band * (i + 0.5f) }
                val barMax = 24.dp.toPx()
                val gap = 2.dp.toPx()
                val radius = 4.dp.toPx()
                val y0 = frame.y(0.0)
                val bars = ArrayList<Bar>()
                if (model.grouped) {
                    val ns = max(1, model.series.size)
                    val groupWidth = min(band * 0.8f, ns * barMax + (ns - 1) * gap)
                    val each = max(2.dp.toPx(), (groupWidth - (ns - 1) * gap) / ns)
                    for (i in 0 until count) model.series.forEachIndexed { k, s ->
                        val v = s.values.getOrElse(i) { 0.0 }
                        if (v == 0.0) return@forEachIndexed
                        val left = x(i) - groupWidth / 2 + k * (each + gap)
                        val h = abs(frame.y(v) - y0)
                        (if (v > 0) barPath(left, frame.y(v), each, h, true, true, radius) else barPath(left, y0, each, h, false, true, radius))
                            ?.let { bars.add(Bar(it, palette.color(s.color))) }
                    }
                } else {
                    val bw = min(barMax, max(2.dp.toPx(), band * 0.62f))
                    for (i in 0 until count) {
                        var top = y0
                        var bottom = y0
                        val stacked = model.series.filter { it.values.getOrElse(i) { 0.0 } != 0.0 }
                        val lastUp = stacked.lastOrNull { it.values[i] > 0 }
                        val lastDown = stacked.lastOrNull { it.values[i] < 0 }
                        stacked.forEach { s ->
                            val v = s.values[i]
                            val h = abs(frame.y(v) - y0)
                            if (v > 0) {
                                val yTop = top - h
                                barPath(x(i) - bw / 2, yTop, bw, h - (if (top != y0) gap else 0f), true, s === lastUp, radius)?.let { bars.add(Bar(it, palette.color(s.color))) }
                                top = yTop
                            } else {
                                val offset = if (bottom != y0) gap else 0f
                                barPath(x(i) - bw / 2, bottom + offset, bw, h - offset, false, s === lastDown, radius)?.let { bars.add(Bar(it, palette.color(s.color))) }
                                bottom += h
                            }
                        }
                    }
                }
                val line = model.line?.let { linePath(it.values, x, frame) }
                val lastLine = model.line?.values?.indexOfLast { it != null } ?: -1
                val average = model.averageText?.let { measurer.measure(it, palette.note) }
                val labels = xLabels(model.xTicks, x, measurer, palette.axis)
                onDrawBehind {
                    drawAxes(frame, model.ticks, axis, palette)
                    if (selected in 0 until count) drawRect(palette.hover, Offset(x(selected) - band / 2, frame.top), Size(band, frame.bottom - frame.top))
                    bars.forEach { drawPath(it.path, it.color) }
                    if (model.average != null) {
                        val ay = frame.y(model.average)
                        drawLine(palette.colors.text2, Offset(frame.left, ay), Offset(frame.right, ay), 1f)
                        average?.let { haloText(it, Offset(frame.right - it.size.width, ay - 4.dp.toPx() - it.size.height), palette.colors.surface) }
                    }
                    model.line?.let { overlay ->
                        val color = palette.color(overlay.color)
                        line?.let { drawPath(it, color, style = Stroke(2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round)) }
                        if (lastLine >= 0) endDot(Offset(x(lastLine), frame.y(overlay.values[lastLine] ?: 0.0)), color, palette.colors.surface)
                    }
                    drawXLabels(labels, frame)
                }
            })
    }
}

/** Lines over months or days on one axis that includes zero; a single line gets a 10% wash. */
@Composable
internal fun LinesChart(model: LinesModel, modifier: Modifier = Modifier, height: Dp = 220.dp) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    val axis = rememberAxis(model.tickLabels, model.unit, measurer, palette.axis)
    var selected by remember(model) { mutableIntStateOf(-1) }
    val count = model.keys.size
    val shown = if (selected in 0 until count) selected else count - 1
    fun pick(x: Float, width: Int, density: Density) {
        with(density) {
            val left = axis.width + 8.dp.toPx()
            val step = (width - left - 6.dp.toPx()) / max(1, count - 1)
            selected = if (count <= 1) 0 else ((x - left) / max(step, 1f)).roundToInt().coerceIn(0, count - 1)
        }
    }
    Column(modifier.fillMaxWidth()) {
        if (shown >= 0) ChartReadout(model.titles[shown], model.readouts[shown])
        Box(Modifier.fillMaxWidth().height(height).semantics { contentDescription = model.label }
            .pointerInput(model, axis.width) { detectTapGestures { o -> pick(o.x, size.width, this) } }
            .pointerInput(model, axis.width) { detectHorizontalDragGestures { change, _ -> pick(change.position.x, size.width, this) } }
            .drawWithCache {
                val frame = plotFrame(size, axis, model.ticks, model.unit != null)
                val step = frame.width / max(1, count - 1)
                val x = { i: Int -> frame.left + if (count == 1) frame.width / 2 else step * i }
                val y0 = frame.y(0.0)
                val dash = PathEffect.dashPathEffect(floatArrayOf(6.dp.toPx(), 4.dp.toPx()))
                val strokes = model.series.map { s -> linePath(s.values, x, frame) }
                val washes = if (!model.wash) emptyList() else model.series.map { s ->
                    Path().apply {
                        var start = -1
                        var end = -1
                        val path = this
                        fun finish() { if (start >= 0) { path.lineTo(x(end), y0); path.lineTo(x(start), y0); path.close() } }
                        s.values.forEachIndexed { i, v ->
                            if (v == null) { finish(); start = -1 }
                            else { if (start < 0) { start = i; moveTo(x(i), frame.y(v)) } else lineTo(x(i), frame.y(v)); end = i }
                        }
                        finish()
                    }
                }
                val ends = model.series.mapIndexedNotNull { k, s ->
                    val i = s.values.indexOfLast { it != null }
                    if (i < 0) null else Triple(k, i, frame.y(s.values[i] ?: 0.0))
                }
                val endLabels = ends.mapNotNull { (k, i, py) -> model.endLabels.getOrNull(k)?.let { Triple(measurer.measure(it, palette.note), x(i), py - 10.dp.toPx()) } }
                    .sortedBy { it.third }.toMutableList()
                for (k in 1 until endLabels.size) endLabels[k] = endLabels[k].copy(third = max(endLabels[k].third, endLabels[k - 1].third + 14.dp.toPx()))
                val labels = xLabels(model.xTicks, x, measurer, palette.axis)
                onDrawBehind {
                    drawAxes(frame, model.ticks, axis, palette)
                    model.series.forEachIndexed { k, s ->
                        val color = palette.color(s.color)
                        washes.getOrNull(k)?.let { drawPath(it, color.copy(alpha = .1f)) }
                        drawPath(strokes[k], color, style = Stroke(2.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round, pathEffect = if (s.dashed) dash else null))
                    }
                    if (selected in 0 until count) {
                        drawLine(palette.colors.borderStrong, Offset(x(selected), frame.top), Offset(x(selected), frame.bottom), 1f)
                        model.series.forEach { s -> s.values.getOrNull(selected)?.let { endDot(Offset(x(selected), frame.y(it)), palette.color(s.color), palette.colors.surface) } }
                    }
                    ends.forEach { (k, i, py) -> endDot(Offset(x(i), py), palette.color(model.series[k].color), palette.colors.surface) }
                    endLabels.forEach { (layout, px, top) -> haloText(layout, Offset(px - 8.dp.toPx() - layout.size.width, top - layout.size.height / 2f), palette.colors.surface) }
                    drawXLabels(labels, frame)
                }
            })
    }
}

/** Parts of a whole as a ring, largest first, the total in the middle. A tap on a slice selects it. */
@Composable
internal fun Donut(slices: List<SliceModel>, centerValue: String, centerLabel: String, onSelect: ((String) -> Unit)?, modifier: Modifier = Modifier, size: Dp = 200.dp) {
    val palette = reportPalette()
    val shown = remember(slices) { slices.filter { it.value > 0 } }
    val angles = remember(shown) {
        val total = shown.sumOf { it.value }.toDouble()
        var a = 0.0
        shown.map { s -> val a0 = a; a += s.value / total * 360.0; a0 to a }
    }
    Box(modifier.size(size)
        .then(if (onSelect == null) Modifier else Modifier.pointerInput(shown) {
            detectTapGestures { o ->
                val c = this.size.width / 2f
                val outer = c - 2.dp.toPx()
                val r = hypot(o.x - c, o.y - c)
                if (r < outer * 0.62f || r > outer) return@detectTapGestures
                var deg = Math.toDegrees(atan2((o.x - c).toDouble(), (c - o.y).toDouble()))
                if (deg < 0) deg += 360.0
                val i = angles.indexOfFirst { deg >= it.first && deg < it.second }
                if (i >= 0) onSelect(shown[i].key)
            }
        })
        .drawWithCache {
            val c = this.size.minDimension / 2
            val middle = Offset(this.size.width / 2, this.size.height / 2)
            val outer = c - 2.dp.toPx()
            val inner = outer * 0.62f
            val mid = (outer + inner) / 2
            val ring = Stroke(outer - inner)
            val topLeft = Offset(middle.x - mid, middle.y - mid)
            val arc = Size(mid * 2, mid * 2)
            onDrawBehind {
                if (shown.isEmpty()) drawCircle(palette.colors.surface2, mid, middle, style = ring)
                shown.forEachIndexed { i, s ->
                    val (a0, a1) = angles[i]
                    drawArc(palette.color(s.color), (a0 - 90).toFloat(), (a1 - a0).toFloat(), false, topLeft, arc, style = ring)
                }
                if (shown.size > 1) angles.forEach { (a0, _) ->
                    val rad = Math.toRadians(a0)
                    val dx = sin(rad).toFloat()
                    val dy = -cos(rad).toFloat()
                    drawLine(palette.colors.surface, Offset(middle.x + inner * dx, middle.y + inner * dy), Offset(middle.x + (outer + 1) * dx, middle.y + (outer + 1) * dy), 2.dp.toPx())
                }
            }
        }, contentAlignment = Alignment.Center) {
        // The total shrinks to fit inside the ring.
        Column(Modifier.widthIn(max = size * 0.54f), horizontalAlignment = Alignment.CenterHorizontally) {
            BasicText(centerValue, style = Tabular.merge(TextStyle(color = ur.text, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center)), maxLines = 1,
                autoSize = TextAutoSize.StepBased(minFontSize = 10.sp, maxFontSize = 17.sp, stepSize = 0.5.sp))
            Text(centerLabel, color = ur.muted, fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

/** A ranked bar under its name, amount and share. */
@Composable
internal fun BarRowView(row: BarRowModel, defaultColor: Int, onClick: (() -> Unit)?) {
    val palette = reportPalette()
    Column(Modifier.fillMaxWidth().then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier).padding(horizontal = 16.dp, vertical = 9.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (row.color != null) { LegendKey(palette.color(row.color)); Spacer(Modifier.width(8.dp)) }
            Text(Money.own(row.label), Modifier.weight(1f), color = ur.text, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(row.valueText, Modifier.padding(start = 8.dp), color = ur.text, style = MaterialTheme.typography.bodyMedium.merge(Tabular), fontWeight = FontWeight.SemiBold, maxLines = 1)
            row.detail?.let { Text(it, Modifier.widthIn(min = 52.dp), color = ur.muted, style = MaterialTheme.typography.bodySmall.merge(Tabular), textAlign = TextAlign.End, maxLines = 1) }
        }
        Box(Modifier.fillMaxWidth().height(10.dp)) {
            if (row.fraction > 0f) Box(Modifier.fillMaxHeight().fillMaxWidth(row.fraction.coerceIn(0f, 1f))
                .clip(RoundedCornerShape(topEnd = 4.dp, bottomEnd = 4.dp)).background(palette.color(row.color ?: defaultColor)))
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
 * Days as a grid of weeks (columns) by weekday (rows), shaded in five steps by how much was spent.
 * Wider ranges scroll sideways, opening on the latest weeks.
 */
@Composable
internal fun HeatCalendar(model: HeatModel, selected: Int, onDay: (Int) -> Unit, modifier: Modifier = Modifier) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    val scroll = rememberScrollState()
    LaunchedEffect(model) {
        val end = snapshotFlow { scroll.maxValue }.first { it in 1 until Int.MAX_VALUE }
        scroll.scrollTo(end)
    }
    // Weeks run left to right in either language, as the grid is drawn, and it opens on the latest.
    CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) { BoxWithConstraints(modifier.fillMaxWidth()) {
        val left = 30.dp
        val top = 18.dp
        val cell = (floor((maxWidth - left).value / max(1, model.weeks)) - 3f).coerceIn(9f, 18f).dp
        val pitch = cell + 3.dp
        Box(Modifier.horizontalScroll(scroll)) {
            Box(Modifier.width(left + pitch * model.weeks).height(top + pitch * 7)
                .pointerInput(model, cell) {
                    detectTapGestures { o ->
                        val col = floor((o.x - left.toPx()) / pitch.toPx()).toInt()
                        val row = floor((o.y - top.toPx()) / pitch.toPx()).toInt()
                        val i = col * 7 + row - model.lead
                        if (col >= 0 && row in 0..6 && i in model.days.indices) onDay(i)
                    }
                }
                .drawWithCache {
                    val l = left.toPx()
                    val t = top.toPx()
                    val p = pitch.toPx()
                    val c = cell.toPx()
                    fun at(i: Int) = Offset(l + ((i + model.lead) / 7) * p, t + ((i + model.lead) % 7) * p)
                    val months = ArrayList<Pair<Float, TextLayoutResult>>()
                    var previous = Float.NEGATIVE_INFINITY
                    model.months.forEachIndexed { k, m ->
                        val x = at(m.index).x
                        if (k == 0 || x - previous >= 28.dp.toPx()) months.add(x to measurer.measure(m.text, palette.axis))
                        previous = x
                    }
                    val weekdays = model.weekdayLabels.map { measurer.measure(it, palette.axis) }
                    val corner = CornerRadius(2.dp.toPx())
                    val cellSize = Size(c, c)
                    onDrawBehind {
                        months.forEach { (x, layout) -> drawText(layout, topLeft = Offset(x, 0f)) }
                        weekdays.forEachIndexed { k, layout -> drawText(layout, topLeft = Offset(0f, t + (1 + 2 * k) * p + c / 2 - layout.size.height / 2f)) }
                        model.levels.forEachIndexed { i, level ->
                            val o = at(i)
                            drawRoundRect(palette.heat[level.coerceIn(0, 5)], o, cellSize, corner)
                            if (level == 0) drawRoundRect(palette.colors.border, o, cellSize, corner, style = Stroke(1f))
                            if (i == selected) drawRoundRect(palette.colors.text, o, cellSize, corner, style = Stroke(1.5.dp.toPx()))
                        }
                    }
                })
        }
    } }
}

/** The heat calendar's key: an empty day, then less to more. */
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

/** Tiles with areas in proportion to their values, shaded by magnitude; labels where they fit. */
@Composable
internal fun Treemap(items: List<TreemapItem>, onSelect: (TreemapItem) -> Unit, modifier: Modifier = Modifier, height: Dp = 280.dp, label: String = stringResource(R.string.chart_spending_by_payee)) {
    val palette = reportPalette()
    val measurer = rememberTextMeasurer()
    var box by remember { mutableStateOf(IntSize.Zero) }
    val sorted = remember(items) { items.filter { it.value > 0 }.sortedByDescending { it.value } }
    val tiles = remember(sorted, box) { squarify(sorted, { it.value.toDouble() }, box.width.toDouble(), box.height.toDouble()) }
    val labelStyle = remember(palette) { TextStyle(color = palette.colors.text, fontSize = 12.5.sp, fontWeight = FontWeight.SemiBold) }
    val valueStyle = remember(palette) { TextStyle(color = palette.colors.text, fontSize = 12.5.sp, fontFeatureSettings = "tnum") }
    Box(modifier.fillMaxWidth().height(height).onSizeChanged { box = it }.semantics { contentDescription = label }
        .pointerInput(tiles) {
            detectTapGestures { o -> tiles.firstOrNull { o.x >= it.x && o.x < it.x + it.w && o.y >= it.y && o.y < it.y + it.h }?.let { onSelect(it.item) } }
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
            val corner = CornerRadius(3.dp.toPx())
            onDrawBehind {
                tiles.forEachIndexed { i, t ->
                    val origin = Offset(t.x.toFloat() + 1, t.y.toFloat() + 1)
                    val area = Size(max(0f, t.w.toFloat() - 2), max(0f, t.h.toFloat() - 2))
                    drawRoundRect(palette.tiles[t.item.level.coerceIn(0, 5)], origin, area, corner)
                    drawRoundRect(palette.colors.border, origin, area, corner, style = Stroke(1f))
                    val (label, value) = texts[i]
                    if (label != null || value != null) clipRect(origin.x, origin.y, origin.x + area.width, origin.y + area.height) {
                        label?.let { drawText(it, topLeft = Offset(origin.x + padX - 1, origin.y + padY - 1)) }
                        value?.let { drawText(it, topLeft = Offset(origin.x + padX - 1, origin.y + padY - 1 + (label?.size?.height ?: 0) + 2.dp.toPx())) }
                    }
                }
            }
        })
}

/** Small columns, one group a month, scaled to the largest value; negatives hang below the line in red. */
@Composable
internal fun MiniBars(model: MiniBarsModel, modifier: Modifier = Modifier) {
    val palette = reportPalette()
    val legend = remember(model) { if (model.series.size > 1) model.series.map { LegendItem(it.label, it.color) } else emptyList() }
    val both = stringResource(R.string.chart_and)
    val description = stringResource(R.string.chart_by_month, model.series.map { it.label }.reduceOrNull { a, b -> both.format(a, b) }.orEmpty())
    Column(modifier.fillMaxWidth()) {
    // Months run left to right in either language, so each label stays under its columns.
    CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
    Column(Modifier.fillMaxWidth().height(136.dp).padding(top = 6.dp)) {
        Box(Modifier.fillMaxWidth().weight(1f).semantics { contentDescription = description }
            .drawWithCache {
                val n = max(1, model.labels.size)
                val colGap = 6.dp.toPx()
                val colWidth = (size.width - colGap * (n - 1)) / n
                val range = (model.top - model.bottom).takeIf { it != 0L } ?: 1L
                val upHeight = size.height * model.top / range
                val downHeight = size.height * -model.bottom / range
                val ns = max(1, model.series.size)
                val barGap = 3.dp.toPx()
                val barWidth = min(22.dp.toPx(), (colWidth - barGap * (ns - 1)) / ns)
                val radius = 4.dp.toPx()
                val bars = ArrayList<Bar>()
                for (i in 0 until model.labels.size) {
                    val colLeft = i * (colWidth + colGap)
                    val groupWidth = barWidth * ns + barGap * (ns - 1)
                    model.series.forEachIndexed { k, s ->
                        val v = s.values.getOrElse(i) { 0L }
                        val left = colLeft + (colWidth - groupWidth) / 2 + k * (barWidth + barGap)
                        if (v > 0 && model.top > 0) {
                            val h = upHeight * v / model.top
                            barPath(left, upHeight - h, barWidth, h, true, true, radius)?.let { bars.add(Bar(it, palette.color(s.color))) }
                        } else if (v < 0 && model.bottom < 0) {
                            val h = downHeight * v / model.bottom
                            barPath(left, upHeight, barWidth, h, false, true, radius)?.let { bars.add(Bar(it, palette.colors.negative)) }
                        }
                    }
                }
                onDrawBehind {
                    bars.forEach { drawPath(it.path, it.color) }
                    for (i in 0 until model.labels.size) {
                        val colLeft = i * (colWidth + colGap)
                        drawLine(palette.colors.borderStrong, Offset(colLeft, upHeight), Offset(colLeft + colWidth, upHeight), 1f)
                    }
                }
            })
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            model.labels.forEach { Text(it, Modifier.weight(1f), color = ur.text2, fontSize = 12.5.sp, textAlign = TextAlign.Center, maxLines = 1) }
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
            fontSize = 18.sp, fontWeight = FontWeight.SemiBold, style = Tabular, maxLines = 1, overflow = TextOverflow.Ellipsis)
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
