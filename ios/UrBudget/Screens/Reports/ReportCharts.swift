import SwiftUI
import Charts

/*
 * Swift Charts for the reports, after Android's canvas charts (ui/Charts.kt) and the desktop's SVG
 * ones: bars at most 24pt wide, 2pt lines with an end dot, hairline grids on round ticks that always
 * include zero, money-formatted axis labels. A tap picks a column or point and the readout above
 * the chart lists its figures, where the desktop shows a hover tooltip. Every label arrives
 * formatted in the model.
 */

/// The desktop's report palette (reports.css) in each appearance, and the roles charts use.
enum ReportPalette {
    private static let light: [UInt32] = [0x2A78D6, 0xEB6834, 0x1BAF7A, 0xEDA100, 0xE87BA4, 0x008300, 0x6250D6, 0xE34948]
    private static let dark: [UInt32] = [0x3987E5, 0xD95926, 0x199E70, 0xC98500, 0xD55181, 0x008300, 0x9085E9, 0xE66767]
    private static let surfaceLight: UInt32 = 0xFFFFFF
    private static let surfaceDark: UInt32 = 0x1C1C1E

    static let series: [Color] = zip(light, dark).map { Color(light: $0, dark: $1) }
    static let other = Color(light: 0xA3A8B0, dark: 0x5D636C)
    /// Heat levels 0 (nothing spent) to 5: series-1 mixed into the card.
    static let heat: [Color] = [Color.insetBackground] + [0.14, 0.30, 0.48, 0.70].map(mix) + [series[0]]
    /// Treemap tiles: levels 0 and 1 share the lightest.
    static let tiles: [Color] = [0.10, 0.10, 0.20, 0.30, 0.42, 0.56].map(mix)

    static func color(_ code: Int?) -> Color {
        guard let code else { return .secondary }
        switch code {
        case 0...7: return series[code]
        case ReportColors.ink: return .primary
        case ReportColors.positive: return .urPositive
        case ReportColors.negative: return .urNegative
        default: return other
        }
    }

    private static func mix(_ amount: Double) -> Color {
        Color(light: blend(surfaceLight, light[0], amount), dark: blend(surfaceDark, dark[0], amount))
    }

    private static func blend(_ a: UInt32, _ b: UInt32, _ t: Double) -> UInt32 {
        func channel(_ shift: UInt32) -> UInt32 {
            let x = Double((a >> shift) & 0xFF)
            let y = Double((b >> shift) & 0xFF)
            return UInt32((x + (y - x) * t).rounded()) << shift
        }
        return channel(16) | channel(8) | channel(0)
    }
}

/// Wraps its children onto as many lines as they need, for legends and chips.
struct ReportFlowLayout: Layout {
    var spacing: CGFloat = 14
    var lineSpacing: CGFloat = 6

    private func sizes(_ subviews: Subviews, width: CGFloat) -> [CGSize] {
        subviews.map { view in
            let ideal = view.sizeThatFits(.unspecified)
            return ideal.width > width ? view.sizeThatFits(ProposedViewSize(width: width, height: nil)) : ideal
        }
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0, widest: CGFloat = 0
        for size in sizes(subviews, width: width) {
            if x > 0 && x + size.width > width {
                y += line + lineSpacing
                x = 0
                line = 0
            }
            x += size.width + spacing
            line = max(line, size.height)
            widest = max(widest, x - spacing)
        }
        return CGSize(width: proposal.width ?? widest, height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0
        for (view, size) in zip(subviews, sizes(subviews, width: bounds.width)) {
            if x > 0 && x + size.width > bounds.width {
                y += line + lineSpacing
                x = 0
                line = 0
            }
            view.place(at: CGPoint(x: bounds.minX + x, y: bounds.minY + y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            line = max(line, size.height)
        }
    }
}

/// A swatch matching the mark: a square for bars, a short line for lines (dashed for dashed lines).
struct ReportLegendKey: View {
    var color: Color
    var line = false
    var dashed = false

    var body: some View {
        if line {
            Path { path in
                path.move(to: CGPoint(x: 1, y: 2))
                path.addLine(to: CGPoint(x: 15, y: 2))
            }
            .stroke(color, style: StrokeStyle(lineWidth: 2, lineCap: .round, dash: dashed ? [3, 3] : []))
            .frame(width: 16, height: 4)
        } else {
            RoundedRectangle(cornerRadius: 2, style: .continuous).fill(color).frame(width: 10, height: 10)
        }
    }
}

struct ReportLegendView: View {
    var items: [ReportLegendItem]

    var body: some View {
        ReportFlowLayout(spacing: 16, lineSpacing: 4) {
            ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                HStack(spacing: 6) {
                    ReportLegendKey(color: ReportPalette.color(item.color), line: item.line, dashed: item.dashed)
                    Text(item.label).font(.caption).foregroundStyle(Color.secondary).lineLimit(1)
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(spoken)
    }

    private var spoken: String {
        let labels = items.map(\.label).joined(separator: ", ")
        return String(localized: "Legend: \(labels)")
    }
}

/// The figures of the chosen column or point, where the desktop shows its tooltip.
struct ReportReadoutView: View {
    var title: String
    var rows: [ReportReadout]

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title).font(.caption).foregroundStyle(Color.secondary).lineLimit(1)
            ReportFlowLayout(spacing: 14, lineSpacing: 2) {
                if rows.isEmpty { Text("Nothing").font(.subheadline).foregroundStyle(Color.secondary) }
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    HStack(spacing: 5) {
                        if let color = row.color { ReportLegendKey(color: ReportPalette.color(color), line: row.line, dashed: row.dashed) }
                        Text(row.value).font(.subheadline.weight(.semibold)).monospacedDigit()
                        Text(row.label).font(.caption).foregroundStyle(Color.secondary).lineLimit(1)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, minHeight: 38, alignment: .topLeading)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(title)
        .accessibilityValue(rows.isEmpty ? String(localized: "Nothing") : rows.map { "\($0.label) \($0.value)" }.joined(separator: ", "))
    }
}

/// The currency over a money axis, when amounts show one.
private struct ReportUnitLabel: ViewModifier {
    var unit: String?

    func body(content: Content) -> some View {
        if let unit {
            content.chartYAxisLabel(unit, position: .top, alignment: .trailing)
        } else {
            content
        }
    }
}

enum ReportAxis {
    /// X labels that never crowd: each needs `spacing` points from the last one drawn; the year shows when it changes.
    static func xLabels(_ ticks: [ReportXTick?], keys: [String], band: CGFloat, spacing: CGFloat) -> [String: String] {
        var out: [String: String] = [:]
        var last = -CGFloat.infinity
        var lastYear: Int?
        for (index, tick) in ticks.enumerated() {
            guard let tick, index < keys.count else { continue }
            let at = band * (CGFloat(index) + 0.5)
            if at - last < spacing { continue }
            out[keys[index]] = lastYear == nil || lastYear != tick.year ? tick.long : tick.short
            last = at
            lastYear = tick.year
        }
        return out
    }

    static func tickLabel(_ value: Double?, ticks: [Double], labels: [String]) -> String {
        guard let value, let index = ticks.firstIndex(where: { abs($0 - value) <= 1e-9 * max(1, abs(value)) }), index < labels.count else { return "" }
        return labels[index]
    }

    /// The column under a tap, from where the chart's plot area is.
    static func index(at location: CGPoint, proxy: ChartProxy, geometry: GeometryProxy, count: Int) -> Int? {
        guard count > 0, let anchor = proxy.plotFrame else { return nil }
        let plot = geometry[anchor]
        guard plot.width > 0, location.x >= plot.minX, location.x <= plot.maxX else { return nil }
        return min(count - 1, max(0, Int(((location.x - plot.minX) / (plot.width / CGFloat(count))).rounded(.down))))
    }
}

private struct ReportBarPiece: Identifiable {
    var id: Int
    var key: String
    var series: String
    var label: String
    var start: Double
    var end: Double
    var color: Int
    var text: String
    var title: String
}

private struct ReportLinePoint: Identifiable {
    var id: Int
    var key: String
    var value: Double
    var series: String
    var color: Int
    var dashed: Bool
    var label: String
    var text: String
    var title: String
}

/// Columns by month (or any keys): one series, stacked series (positives up, negatives down) or
/// grouped series side by side, with an optional overlay line and average rule.
struct ReportColumnChart: View {
    var model: ReportColumnModel
    var height: CGFloat = 220
    @State private var selected: Int?
    @ScaledMetric(relativeTo: .caption2) private var labelSpacing: CGFloat = 44
    @Environment(\.layoutDirection) private var direction

    var body: some View {
        let count = model.keys.count
        let shown = selected.flatMap { $0 < count ? $0 : nil } ?? count - 1
        VStack(alignment: .leading, spacing: 8) {
            if shown >= 0 && shown < model.readouts.count { ReportReadoutView(title: model.titles[shown], rows: model.readouts[shown]) }
            // Months run left to right in every language, so a tap's column is counted from the left.
            GeometryReader { geometry in plot(width: geometry.size.width) }
                .environment(\.layoutDirection, .leftToRight)
                .frame(height: height)
        }
        .onChange(of: model.keys) { selected = nil }
    }

    private func value(_ series: ReportColumnSeries, _ index: Int) -> Double { index < series.values.count ? series.values[index] : 0 }

    private func pieces() -> [ReportBarPiece] {
        var out: [ReportBarPiece] = []
        for index in model.keys.indices {
            var top = 0.0
            var bottom = 0.0
            for (k, series) in model.series.enumerated() {
                let v = value(series, index)
                let text = k < model.texts.count && index < model.texts[k].count ? model.texts[k][index] : ""
                func piece(_ start: Double, _ end: Double) -> ReportBarPiece {
                    ReportBarPiece(id: out.count, key: model.keys[index], series: series.key, label: series.label, start: start, end: end,
                                   color: series.color, text: text, title: model.titles[index])
                }
                if model.grouped {
                    out.append(piece(0, v))
                } else if v > 0 {
                    out.append(piece(top, top + v))
                    top += v
                } else if v < 0 {
                    out.append(piece(bottom, bottom + v))
                    bottom += v
                }
            }
        }
        return out
    }

    private func linePoints(_ line: ReportLineOverlay) -> [ReportLinePoint] {
        var out: [ReportLinePoint] = []
        var segment = 0
        for (index, value) in line.values.enumerated() where index < model.keys.count {
            guard let value else {
                segment += 1
                continue
            }
            out.append(ReportLinePoint(id: out.count, key: model.keys[index], value: value, series: "line-\(segment)", color: line.color, dashed: false,
                                       label: line.label, text: index < model.lineTexts.count ? model.lineTexts[index] ?? "" : "", title: model.titles[index]))
        }
        return out
    }

    private func plot(width: CGFloat) -> some View {
        let count = max(1, model.keys.count)
        let band = max(1, width - 52) / CGFloat(count)
        let barWidth = min(24, max(2, band * 0.62))
        let seriesCount = max(1, model.series.count)
        let groupWidth = min(band * 0.8, CGFloat(seriesCount) * 24 + CGFloat(seriesCount - 1) * 2)
        let each = max(2, (groupWidth - CGFloat(seriesCount - 1) * 2) / CGFloat(seriesCount))
        let labels = ReportAxis.xLabels(model.xTicks, keys: model.keys, band: band, spacing: labelSpacing)
        let bars = pieces()
        let points = model.line.map(linePoints) ?? []
        let low = model.ticks.first ?? 0
        let high = model.ticks.last ?? 1
        return Chart {
            if let selected, selected < model.keys.count {
                RectangleMark(x: .value("Month", model.keys[selected]), yStart: .value("Amount", low), yEnd: .value("Amount", high), width: .ratio(1))
                    .foregroundStyle(Color.primary.opacity(0.07))
                    .accessibilityHidden(true)
            }
            RuleMark(y: .value("Zero", 0.0))
                .foregroundStyle(Color.secondary.opacity(0.6))
                .lineStyle(StrokeStyle(lineWidth: 1))
                .accessibilityHidden(true)
            ForEach(bars) { bar in
                if model.grouped {
                    BarMark(x: .value("Month", bar.key), yStart: .value("Amount", bar.start), yEnd: .value("Amount", bar.end), width: .fixed(each))
                        .position(by: .value("Series", bar.series), axis: .horizontal, span: .fixed(groupWidth))
                        .foregroundStyle(ReportPalette.color(bar.color))
                        .cornerRadius(3)
                        .accessibilityLabel("\(bar.title), \(bar.label)")
                        .accessibilityValue(bar.text)
                } else {
                    BarMark(x: .value("Month", bar.key), yStart: .value("Amount", bar.start), yEnd: .value("Amount", bar.end), width: .fixed(barWidth))
                        .foregroundStyle(ReportPalette.color(bar.color))
                        .cornerRadius(3)
                        .accessibilityLabel(model.series.count > 1 ? "\(bar.title), \(bar.label)" : bar.title)
                        .accessibilityValue(bar.text)
                }
            }
            if let average = model.average {
                RuleMark(y: .value("Average", average))
                    .foregroundStyle(Color.secondary)
                    .lineStyle(StrokeStyle(lineWidth: 1, dash: [4, 3]))
                    .annotation(position: .top, alignment: .trailing, spacing: 2) {
                        Text(model.averageText ?? "").font(.caption2).monospacedDigit().foregroundStyle(Color.secondary)
                            .padding(.horizontal, 3)
                            .background(Color.cardBackground.opacity(0.85), in: RoundedRectangle(cornerRadius: 3))
                            .environment(\.layoutDirection, direction)
                    }
                    .accessibilityLabel(model.averageText ?? String(localized: "Average"))
            }
            ForEach(points) { point in
                LineMark(x: .value("Month", point.key), y: .value("Amount", point.value), series: .value("Line", point.series))
                    .foregroundStyle(ReportPalette.color(point.color))
                    .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
                    .accessibilityLabel("\(point.title), \(point.label)")
                    .accessibilityValue(point.text)
            }
            if let last = points.last {
                PointMark(x: .value("Month", last.key), y: .value("Amount", last.value))
                    .foregroundStyle(ReportPalette.color(last.color))
                    .symbolSize(40)
                    .accessibilityHidden(true)
            }
        }
        .chartXScale(domain: model.keys)
        .chartYScale(domain: low...high)
        .chartXAxis {
            AxisMarks(values: model.keys.filter { labels[$0] != nil }) { value in
                AxisValueLabel(labels[value.as(String.self) ?? ""] ?? "", collisionResolution: .disabled)
            }
        }
        .chartYAxis {
            AxisMarks(position: .trailing, values: model.ticks) { value in
                AxisGridLine()
                AxisValueLabel(ReportAxis.tickLabel(value.as(Double.self), ticks: model.ticks, labels: model.tickLabels))
            }
        }
        .modifier(ReportUnitLabel(unit: model.unit))
        .chartLegend(.hidden)
        .chartOverlay { proxy in
            GeometryReader { geometry in
                Rectangle().fill(.clear).contentShape(Rectangle())
                    .onTapGesture { location in
                        let index = ReportAxis.index(at: location, proxy: proxy, geometry: geometry, count: model.keys.count)
                        selected = index == nil || index == selected ? nil : index
                    }
                    // Dragging across reads each month in turn; an up-and-down drag still scrolls the page.
                    .simultaneousGesture(DragGesture(minimumDistance: 10).onChanged { drag in
                        guard abs(drag.translation.width) > abs(drag.translation.height),
                              let index = ReportAxis.index(at: drag.location, proxy: proxy, geometry: geometry, count: model.keys.count) else { return }
                        if index != selected { selected = index }
                    })
                    .sensoryFeedback(.selection, trigger: selected)
                    .accessibilityHidden(true)
            }
        }
        .accessibilityLabel(model.label)
    }
}

/// Lines over months or days on one axis that includes zero; a single line gets a light wash.
struct ReportLinesChart: View {
    var model: ReportLinesModel
    var height: CGFloat = 220
    @State private var selected: Int?
    @ScaledMetric(relativeTo: .caption2) private var labelSpacing: CGFloat = 44

    var body: some View {
        let count = model.keys.count
        let shown = selected.flatMap { $0 < count ? $0 : nil } ?? count - 1
        VStack(alignment: .leading, spacing: 8) {
            if shown >= 0 && shown < model.readouts.count { ReportReadoutView(title: model.titles[shown], rows: model.readouts[shown]) }
            // Time runs left to right in every language, so a tap's point is counted from the left.
            GeometryReader { geometry in plot(width: geometry.size.width) }
                .environment(\.layoutDirection, .leftToRight)
                .frame(height: height)
        }
        .onChange(of: model.keys) { selected = nil }
    }

    private func points() -> [ReportLinePoint] {
        var out: [ReportLinePoint] = []
        for (k, series) in model.series.enumerated() {
            var segment = 0
            for (index, value) in series.values.enumerated() where index < model.keys.count {
                guard let value else {
                    segment += 1
                    continue
                }
                let text = k < model.texts.count && index < model.texts[k].count ? model.texts[k][index] ?? "" : ""
                out.append(ReportLinePoint(id: out.count, key: model.keys[index], value: value, series: "\(series.key)#\(segment)", color: series.color,
                                           dashed: series.dashed, label: series.label, text: text, title: model.titles[index]))
            }
        }
        return out
    }

    private func plot(width: CGFloat) -> some View {
        let count = max(1, model.keys.count)
        let band = max(1, width - 52) / CGFloat(count)
        let labels = ReportAxis.xLabels(model.xTicks, keys: model.keys, band: band, spacing: labelSpacing)
        let all = points()
        let ends = model.series.indices.compactMap { k in all.last { $0.series.hasPrefix("\(model.series[k].key)#") } }
        let low = model.ticks.first ?? 0
        let high = model.ticks.last ?? 1
        let pointMarks = count <= 1
        let chosen: [ReportLinePoint] = selected.flatMap { index in index < model.keys.count ? all.filter { $0.key == model.keys[index] } : nil } ?? []
        return Chart {
            RuleMark(y: .value("Zero", 0.0))
                .foregroundStyle(Color.secondary.opacity(0.6))
                .lineStyle(StrokeStyle(lineWidth: 1))
                .accessibilityHidden(true)
            if model.wash {
                ForEach(all) { point in
                    AreaMark(x: .value("Day", point.key), yStart: .value("Base", 0.0), yEnd: .value("Amount", point.value), series: .value("Series", point.series))
                        .foregroundStyle(ReportPalette.color(point.color).opacity(0.12))
                        .accessibilityHidden(true)
                }
            }
            ForEach(all) { point in
                LineMark(x: .value("Day", point.key), y: .value("Amount", point.value), series: .value("Series", point.series))
                    .foregroundStyle(ReportPalette.color(point.color))
                    .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round, dash: point.dashed ? [6, 4] : []))
                    .accessibilityLabel(model.series.count > 1 ? "\(point.title), \(point.label)" : point.title)
                    .accessibilityValue(point.text)
                if pointMarks {
                    PointMark(x: .value("Day", point.key), y: .value("Amount", point.value))
                        .foregroundStyle(ReportPalette.color(point.color))
                        .accessibilityHidden(true)
                }
            }
            if let selected, selected < model.keys.count {
                RuleMark(x: .value("Day", model.keys[selected]))
                    .foregroundStyle(Color.secondary)
                    .lineStyle(StrokeStyle(lineWidth: 1))
                    .accessibilityHidden(true)
            }
            ForEach(selected == nil ? ends : chosen) { point in
                PointMark(x: .value("Day", point.key), y: .value("Amount", point.value))
                    .foregroundStyle(ReportPalette.color(point.color))
                    .symbolSize(40)
                    .accessibilityHidden(true)
            }
        }
        .chartXScale(domain: model.keys)
        .chartYScale(domain: low...high)
        .chartXAxis {
            AxisMarks(values: model.keys.filter { labels[$0] != nil }) { value in
                AxisValueLabel(labels[value.as(String.self) ?? ""] ?? "", collisionResolution: .disabled)
            }
        }
        .chartYAxis {
            AxisMarks(position: .trailing, values: model.ticks) { value in
                AxisGridLine()
                AxisValueLabel(ReportAxis.tickLabel(value.as(Double.self), ticks: model.ticks, labels: model.tickLabels))
            }
        }
        .modifier(ReportUnitLabel(unit: model.unit))
        .chartLegend(.hidden)
        .chartOverlay { proxy in
            GeometryReader { geometry in
                Rectangle().fill(.clear).contentShape(Rectangle())
                    .onTapGesture { location in
                        let index = ReportAxis.index(at: location, proxy: proxy, geometry: geometry, count: model.keys.count)
                        selected = index == nil || index == selected ? nil : index
                    }
                    // Dragging across reads each month in turn; an up-and-down drag still scrolls the page.
                    .simultaneousGesture(DragGesture(minimumDistance: 10).onChanged { drag in
                        guard abs(drag.translation.width) > abs(drag.translation.height),
                              let index = ReportAxis.index(at: drag.location, proxy: proxy, geometry: geometry, count: model.keys.count) else { return }
                        if index != selected { selected = index }
                    })
                    .sensoryFeedback(.selection, trigger: selected)
                    .accessibilityHidden(true)
            }
        }
        .accessibilityLabel(model.label)
    }
}

/// Parts of a whole as a ring, largest first, the total in the middle. A tap on a slice selects it.
struct ReportDonut: View {
    var slices: [ReportSlice]
    var centerValue: String
    var centerLabel: String
    var onSelect: ((String) -> Void)? = nil
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 200

    var body: some View {
        let shown = slices.filter { $0.value > 0 }
        let side = min(size, 280)
        ZStack {
            if shown.isEmpty {
                Circle().stroke(Color.insetBackground, lineWidth: side * 0.19).padding(side * 0.095)
            } else {
                Chart(shown) { slice in
                    SectorMark(angle: .value("Amount", Double(slice.value)), innerRadius: .ratio(0.62), angularInset: shown.count > 1 ? 1 : 0)
                        .foregroundStyle(ReportPalette.color(slice.color))
                        .cornerRadius(2)
                        .accessibilityLabel(slice.label)
                        .accessibilityValue(slice.valueText)
                }
                .chartLegend(.hidden)
                .chartOverlay { proxy in
                    GeometryReader { geometry in
                        if let onSelect {
                            Rectangle().fill(.clear).contentShape(Rectangle())
                                .onTapGesture { location in
                                    if let key = sliceKey(at: location, in: geometry.size, shown: shown) { onSelect(key) }
                                }
                                .accessibilityHidden(true)
                        }
                    }
                }
                // Clockwise from the top in every language, as `sliceKey` reads a tap.
                .environment(\.layoutDirection, .leftToRight)
            }
            VStack(spacing: 1) {
                Text(centerValue).font(.headline).monospacedDigit().lineLimit(1).minimumScaleFactor(0.5)
                Text(centerLabel).font(.caption).foregroundStyle(Color.secondary).lineLimit(1)
            }
            .frame(width: side * 0.54)
            .allowsHitTesting(false)
            .accessibilityElement(children: .combine)
        }
        .frame(width: side, height: side)
        .frame(maxWidth: .infinity)
    }

    private func sliceKey(at location: CGPoint, in size: CGSize, shown: [ReportSlice]) -> String? {
        let center = CGPoint(x: size.width / 2, y: size.height / 2)
        let outer = min(size.width, size.height) / 2
        let dx = location.x - center.x
        let dy = location.y - center.y
        let radius = (dx * dx + dy * dy).squareRoot()
        guard radius >= outer * 0.6, radius <= outer else { return nil }
        var degrees = atan2(dx, -dy) * 180 / .pi
        if degrees < 0 { degrees += 360 }
        let total = Double(shown.map(\.value).reportSum)
        guard total > 0 else { return nil }
        var start = 0.0
        for slice in shown {
            let end = start + Double(slice.value) / total * 360
            if degrees >= start && degrees < end { return slice.key }
            start = end
        }
        return nil
    }
}

/// A ranked bar under its name, amount and share.
struct ReportBarRowView: View {
    var row: ReportBarRow
    var defaultColor = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                if let color = row.color { ReportLegendKey(color: ReportPalette.color(color)) }
                Text(row.label).lineLimit(1)
                Spacer(minLength: 8)
                Text(row.valueText).fontWeight(.semibold).monospacedDigit().lineLimit(1)
                if let detail = row.detail {
                    Text(detail).font(.footnote).foregroundStyle(Color.secondary).monospacedDigit().lineLimit(1).frame(minWidth: 46, alignment: .trailing)
                }
            }
            .font(.subheadline)
            GeometryReader { geometry in
                RoundedRectangle(cornerRadius: 3, style: .continuous)
                    .fill(ReportPalette.color(row.color ?? defaultColor))
                    .frame(width: max(0, geometry.size.width * min(1, max(0, row.fraction))))
            }
            .frame(height: 8)
        }
        .padding(.vertical, 3)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(row.label)
        .accessibilityValue([row.valueText, row.detail].compactMap { $0 }.joined(separator: ", "))
    }
}

/// A change either side of a centre line: less towards the start (left, or right in Arabic), more towards the end.
struct ReportDivergingRowView: View {
    var row: ReportDivRow
    @Environment(\.layoutDirection) private var direction

    var body: some View {
        let color = ReportPalette.color(row.color)
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(row.label).lineLimit(1)
                Spacer(minLength: 8)
                Text(row.valueText).fontWeight(.semibold).monospacedDigit().lineLimit(1)
                Text(row.detail).font(.footnote).foregroundStyle(Color.secondary).monospacedDigit().lineLimit(1)
            }
            .font(.subheadline)
            bar(color)
        }
        .padding(.vertical, 3)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(row.label)
        .accessibilityValue("\(row.valueText), \(row.detail)")
    }

    /// Placed by offsets from the left edge, so it's laid out left to right and mirrored by hand in Arabic.
    private func bar(_ color: Color) -> some View {
        let drawsLeft = (row.fraction < 0) == (direction == .leftToRight)
        return GeometryReader { geometry in
            let half = (geometry.size.width - 1) / 2
            let length = half * CGFloat(min(1, abs(row.fraction)))
            ZStack(alignment: .topLeading) {
                Rectangle().fill(Color.secondary.opacity(0.5)).frame(width: 1, height: 12).offset(x: half)
                RoundedRectangle(cornerRadius: 3, style: .continuous).fill(color)
                    .frame(width: length, height: 12)
                    .offset(x: drawsLeft ? half - length : half + 1)
            }
        }
        .environment(\.layoutDirection, .leftToRight)
        .frame(height: 12)
    }
}

/// Days as a grid of weeks (columns) by weekday (rows), shaded in five steps by how much was spent.
/// Wider ranges scroll sideways, opening on the latest weeks.
struct ReportHeatCalendar: View {
    var model: ReportHeatModel
    var selected: Int
    var onDay: (Int) -> Void
    @State private var width: CGFloat = 340

    var body: some View {
        // Arabic weekday names need a little more room than "Mon".
        let left: CGFloat = AppLanguage.isRightToLeft ? 40 : 30
        let top: CGFloat = 18
        let cell = min(18, max(9, ((width - left) / CGFloat(max(1, model.weeks))).rounded(.down) - 3))
        let pitch = cell + 3
        let contentWidth = left + pitch * CGFloat(model.weeks)
        let contentHeight = top + pitch * 7
        ScrollView(.horizontal, showsIndicators: false) {
            Canvas { context, _ in
                func origin(_ index: Int) -> CGPoint {
                    CGPoint(x: left + CGFloat((index + model.lead) / 7) * pitch, y: top + CGFloat((index + model.lead) % 7) * pitch)
                }
                var previous = -CGFloat.infinity
                for (k, month) in model.months.enumerated() {
                    let x = origin(month.index).x
                    if k == 0 || x - previous >= 28 {
                        context.draw(Text(month.text).font(.caption2).foregroundStyle(Color.secondary), at: CGPoint(x: x, y: 0), anchor: .topLeading)
                    }
                    previous = x
                }
                for (k, label) in model.weekdayLabels.enumerated() {
                    let y = top + CGFloat(1 + 2 * k) * pitch + cell / 2
                    context.draw(Text(label).font(.caption2).foregroundStyle(Color.secondary), at: CGPoint(x: 0, y: y), anchor: .leading)
                }
                for (index, level) in model.levels.enumerated() {
                    let rect = CGRect(origin: origin(index), size: CGSize(width: cell, height: cell))
                    let shape = Path(roundedRect: rect, cornerRadius: 2)
                    context.fill(shape, with: .color(ReportPalette.heat[min(5, max(0, level))]))
                    if level == 0 { context.stroke(shape, with: .color(Color.primary.opacity(0.12)), lineWidth: 1) }
                    if index == selected { context.stroke(shape, with: .color(.primary), lineWidth: 1.5) }
                }
            }
            .frame(width: contentWidth, height: contentHeight)
            .contentShape(Rectangle())
            .onTapGesture { location in
                let column = Int(((location.x - left) / pitch).rounded(.down))
                let row = Int(((location.y - top) / pitch).rounded(.down))
                let index = column * 7 + row - model.lead
                if column >= 0 && (0...6).contains(row) && model.days.indices.contains(index) { onDay(index) }
            }
        }
        .defaultScrollAnchor(.trailing)
        .frame(height: contentHeight)
        .background {
            GeometryReader { geometry in
                Color.clear
                    .onAppear { width = geometry.size.width }
                    .onChange(of: geometry.size.width) { _, next in width = next }
            }
        }
        // Drawn at fixed positions: weeks run left to right, the latest at the right, in every language.
        .environment(\.layoutDirection, .leftToRight)
    }
}

/// The heat calendar's key: an empty day, then less to more.
struct ReportHeatLegend: View {
    var body: some View {
        HStack(spacing: 3) {
            swatch(0)
            Text("No spending").font(.caption).foregroundStyle(Color.secondary).padding(.leading, 3)
            Spacer(minLength: 8)
            Text("Less").font(.caption).foregroundStyle(Color.secondary).padding(.trailing, 3)
            ForEach(1...5, id: \.self) { swatch($0) }
            Text(String(localized: "reports.legend.more", defaultValue: "More", comment: "End of a scale from less to more spending."))
                .font(.caption).foregroundStyle(Color.secondary).padding(.leading, 3)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Darker days had more spending")
    }

    private func swatch(_ level: Int) -> some View {
        RoundedRectangle(cornerRadius: 2).fill(ReportPalette.heat[level])
            .overlay(RoundedRectangle(cornerRadius: 2).strokeBorder(level == 0 ? Color.primary.opacity(0.12) : .clear))
            .frame(width: 11, height: 11)
    }
}

/// Tiles with areas in proportion to their values, shaded by magnitude; labels where they fit.
struct ReportTreemap: View {
    var items: [ReportTreemapItem]
    var onSelect: (ReportTreemapItem) -> Void
    var height: CGFloat = 280
    @Environment(\.layoutDirection) private var direction

    /// Tiles are placed by offsets from the left edge, so they're laid out left to right; in Arabic the
    /// largest starts at the right, mirrored by hand, and each tile's text keeps the app's direction.
    var body: some View {
        GeometryReader { geometry in
            let sorted = items.filter { $0.value > 0 }.reportSorted(descending: { $0.value })
            let boxes = ReportMath.squarify(sorted, value: { Double($0.value) }, width: geometry.size.width, height: geometry.size.height)
            ZStack(alignment: .topLeading) {
                ForEach(boxes.indices, id: \.self) { index in tile(boxes[index], width: geometry.size.width) }
            }
        }
        .environment(\.layoutDirection, .leftToRight)
        .frame(height: height)
    }

    private func tile(_ box: ReportTileBox<ReportTreemapItem>, width: CGFloat) -> some View {
        let item = box.item
        let w = max(0, box.w - 2)
        let h = max(0, box.h - 2)
        let x: Double = direction == .rightToLeft ? Double(width) - box.x - box.w : box.x
        return Button { onSelect(item) } label: {
            VStack(alignment: .leading, spacing: 2) {
                if w > 44 && h > 22 { Text(item.label).font(.caption.weight(.semibold)).lineLimit(1) }
                if w > 76 && h > 40 { Text(item.valueText).font(.caption).monospacedDigit().lineLimit(1) }
            }
            .padding(.horizontal, 7)
            .padding(.vertical, 5)
            .frame(width: w, height: h, alignment: .topLeading)
            .background(ReportPalette.tiles[min(5, max(0, item.level))], in: RoundedRectangle(cornerRadius: 3, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 3, style: .continuous).strokeBorder(Color.primary.opacity(0.1)))
            .clipped()
            .foregroundStyle(Color.primary)
            .environment(\.layoutDirection, direction)
        }
        .buttonStyle(.plain)
        .offset(x: x + 1, y: box.y + 1)
        .accessibilityLabel(item.label)
        .accessibilityValue("\(item.valueText), \(item.detail), \(item.more)")
        .accessibilityHint("Shows this payee's transactions")
    }
}

/// Small columns, one group a month, scaled to the largest value; negatives hang below the line in red.
struct ReportMiniBars: View {
    var model: ReportMiniBarsModel

    var body: some View {
        let ids = model.labels.indices.map(String.init)
        Chart {
            ForEach(model.series, id: \.label) { (series: ReportMiniSeries) in
                ForEach(Array(series.values.enumerated()), id: \.offset) { pair in
                    bar(series, index: pair.offset, value: pair.element)
                }
            }
            RuleMark(y: .value("Zero", 0.0)).foregroundStyle(Color.secondary.opacity(0.6)).lineStyle(StrokeStyle(lineWidth: 1)).accessibilityHidden(true)
        }
        .chartXScale(domain: ids)
        .chartXAxis {
            AxisMarks(values: ids) { value in
                AxisValueLabel(model.labels[Int(value.as(String.self) ?? "") ?? 0])
            }
        }
        .chartYAxis(.hidden)
        .chartLegend(.hidden)
        // Months run left to right, as on the reports' other charts.
        .environment(\.layoutDirection, .leftToRight)
        .frame(height: 130)
        .accessibilityLabel(spoken)
    }

    /// "Income and Spending by month".
    private var spoken: String {
        let labels = model.series.map(\.label)
        if labels.count == 2 { return String(localized: "\(labels[0]) and \(labels[1]) by month") }
        let joined = labels.joined(separator: " and ")
        return String(localized: "\(joined) by month")
    }

    /// One column. Its parts are worked out first: as one expression, Xcode 26 can't type-check it in time.
    private func bar(_ series: ReportMiniSeries, index: Int, value: Int64) -> some ChartContent {
        let month: String = index < model.labels.count ? model.labels[index] : ""
        let spoken: String = index < series.texts.count ? series.texts[index] : ""
        let label: String = "\(month), \(series.label)"
        let color: Color = value < 0 ? Color.urNegative : ReportPalette.color(series.color)
        let width: MarkDimension = .ratio(model.series.count > 1 ? 0.8 : 0.55)
        return BarMark(x: .value("Month", String(index)), y: .value("Amount", Double(value)), width: width)
            .position(by: .value("Series", series.label))
            .foregroundStyle(color)
            .cornerRadius(3)
            .accessibilityLabel(label)
            .accessibilityValue(spoken)
    }
}

/// The month's spending as one bar, split by category in the list's colours.
struct ReportSegmentBar: View {
    var rows: [ReportSpendRow]

    var body: some View {
        GeometryReader { geometry in
            let weights = rows.map { max(1, Double($0.weight)) }
            let total = max(1, weights.reduce(0, +))
            let room = max(0, geometry.size.width - CGFloat(max(0, rows.count - 1)) * 2)
            HStack(spacing: 2) {
                ForEach(rows.indices, id: \.self) { index in
                    Rectangle().fill(ReportPalette.color(rows[index].color)).frame(width: room * CGFloat(weights[index] / total))
                }
            }
        }
        .frame(height: 24)
        .clipShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
        .accessibilityHidden(true)
    }
}

/// A report's headline figure.
struct ReportTileCard: View {
    var tile: ReportTile

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(tile.label).font(.caption).foregroundStyle(Color.secondary).lineLimit(2)
            Text(tile.value).font(.title3.weight(.semibold)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.6)
                .foregroundStyle(tile.tone == .negative ? Color.urNegative : tile.tone == .positive ? Color.urPositive : Color.primary)
            if let note = tile.note {
                Text(note).font(.caption).foregroundStyle(Color.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .accessibilityElement(children: .combine)
    }
}

/// The headline figures, two to a row (one at the largest text sizes).
struct ReportTilesView: View {
    var tiles: [ReportTile]
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let columns = typeSize.isAccessibilitySize ? 1 : 2
        let rows = stride(from: 0, to: tiles.count, by: columns).map { Array(tiles[$0..<min($0 + columns, tiles.count)]) }
        Grid(horizontalSpacing: 10, verticalSpacing: 10) {
            ForEach(rows.indices, id: \.self) { index in
                GridRow {
                    ForEach(Array(rows[index].enumerated()), id: \.offset) { _, tile in ReportTileCard(tile: tile) }
                    if rows[index].count < columns { Color.clear.gridCellUnsizedAxes([.horizontal, .vertical]) }
                }
            }
        }
    }
}

/// A row of a report's list: its title over a detail line, and its figure.
struct ReportFigureRowView: View {
    var row: ReportFigureRow

    var body: some View {
        let strong = row.style == .total || row.style == .net || row.style == .group
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(row.title).fontWeight(strong ? .semibold : .regular).foregroundStyle(row.style == .muted ? .secondary : .primary).lineLimit(2)
                if let subtitle = row.subtitle {
                    Text(subtitle).font(.footnote).monospacedDigit().foregroundStyle(Color.secondary).lineLimit(3)
                }
            }
            Spacer(minLength: 8)
            if !row.value.isEmpty {
                Text(row.value).fontWeight(strong ? .bold : .medium).monospacedDigit().lineLimit(1)
                    .foregroundStyle(row.tone == .negative ? Color.urNegative : row.tone == .positive ? Color.urPositive : Color.primary)
            }
        }
        .padding(.leading, row.style == .child ? 14 : 0)
        .accessibilityElement(children: .combine)
    }
}
