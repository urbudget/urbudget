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
    /// How strongly the month still under way is drawn.
    static let soFar = 0.45
    /// Heat levels 0 (nothing spent) to 5, Android's own scale: one blue, darker for more on
    /// light and brighter for more on dark, its steps far enough apart to tell at a glance.
    static let heat: [Color] = [Color.insetBackground] + zip([0x86B6EF, 0x5598E7, 0x2A78D6, 0x1C5CAB, 0x104281] as [UInt32],
                                                             [0x184F95, 0x256ABF, 0x3987E5, 0x6DA7EC, 0x9EC5F4] as [UInt32]).map { Color(light: $0, dark: $1) }
    /// A date's ink on a heat level: light on the deep steps, dark on the bright ones.
    static func heatInk(_ level: Int) -> Color {
        level == 0 ? .secondary : Color(light: level >= 3 ? 0xFFFFFF : 0x1D1D1F, dark: level >= 4 ? 0x0C0E18 : 0xFFFFFF)
    }
    /// Treemap tiles: levels 0 and 1 share the lightest.
    static let tiles: [Color] = [0.10, 0.10, 0.20, 0.30, 0.42, 0.56].map(mix)

    /// A series colour as a treemap tile wears it: mixed into the card, so its words stay readable.
    static func tile(_ code: Int) -> Color {
        guard (0...7).contains(code) else { return Color(light: blend(surfaceLight, 0xA3A8B0, 0.42), dark: blend(surfaceDark, 0x5D636C, 0.6)) }
        return Color(light: blend(surfaceLight, light[code], 0.34), dark: blend(surfaceDark, dark[code], 0.46))
    }

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

/// Which colour is which. With `onToggle`, each name hides and shows its series: a hidden one keeps
/// an empty key and is struck through.
struct ReportLegendView: View {
    var items: [ReportLegendItem]
    var hidden: Set<String> = []
    var onToggle: ((String) -> Void)? = nil
    /// Keys in the softer fill treemap tiles wear.
    var tiles = false

    var body: some View {
        if let onToggle {
            ReportFlowLayout(spacing: 4, lineSpacing: 2) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    let off = hidden.contains(item.label)
                    Button { onToggle(item.label) } label: {
                        HStack(spacing: 6) {
                            if off {
                                RoundedRectangle(cornerRadius: 2, style: .continuous).stroke(Color.secondary.opacity(0.6), lineWidth: 1).frame(width: 10, height: 10)
                            } else {
                                ReportLegendKey(color: ReportPalette.color(item.color), line: item.line, dashed: item.dashed)
                            }
                            Text(item.label).font(.caption).foregroundStyle(off ? Color.secondary.opacity(0.7) : Color.secondary).strikethrough(off).lineLimit(1)
                        }
                        .padding(.horizontal, 8)
                        .frame(minHeight: 34)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(item.label)
                    .accessibilityValue(off ? Text("Hidden") : Text("Shown"))
                    .accessibilityHint("Hides or shows this in the chart")
                }
            }
            .padding(.horizontal, -8)
        } else {
            ReportFlowLayout(spacing: 16, lineSpacing: 4) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(spacing: 6) {
                        ReportLegendKey(color: tiles ? ReportPalette.tile(item.color) : ReportPalette.color(item.color), line: item.line, dashed: item.dashed)
                        Text(item.label).font(.caption).foregroundStyle(Color.secondary).lineLimit(1)
                    }
                }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(spoken)
        }
    }

    /// A legend name hidden or shown; the last one showing stays.
    static func toggled(_ hidden: Set<String>, _ label: String, legend: [ReportLegendItem]) -> Set<String> {
        if hidden.contains(label) { return hidden.subtracting([label]) }
        if !legend.contains(where: { !hidden.contains($0.label) && $0.label != label }) { return hidden }
        return hidden.union([label])
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
    /// X labels that never crowd, at an even step (every month, every 2nd, 3rd…) counted back from
    /// the last one, so the latest always shows. Each needs `spacing` points; the year shows in full
    /// on the first label and where it changes ("Dec 2025", "Feb", "Apr").
    static func xLabels(_ ticks: [ReportXTick?], keys: [String], band: CGFloat, spacing: CGFloat) -> [String: String] {
        guard let last = ticks.lastIndex(where: { $0 != nil }) else { return [:] }
        let step = max(1, Int((spacing / max(band, 0.01)).rounded(.up)))
        var out: [String: String] = [:]
        var lastYear: Int?
        for (index, tick) in ticks.enumerated() {
            guard let tick, index < keys.count, (last - index) % step == 0 else { continue }
            out[keys[index]] = lastYear == nil || lastYear != tick.year ? tick.long : tick.short
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
    /// A point drawn a second time, to start the dashed stretch into the month under way.
    var echo = false
}

/// Columns by month (or any keys): one series, stacked series (positives up, negatives down) or
/// grouped series side by side, with an optional overlay line and average rule.
/// With a `legend`, its names hide and show their series and the scale follows. The month still
/// under way is drawn lighter, and the chart opens on the last complete one.
struct ReportColumnChart: View {
    /// Every series; `model` is what's drawn, without the hidden ones.
    private var full: ReportColumnModel
    private var legend: [ReportLegendItem]?
    private var height: CGFloat
    @State private var selected: Int?
    @State private var hidden: Set<String> = []
    @ScaledMetric(relativeTo: .caption2) private var labelSpacing: CGFloat = 52
    @Environment(\.layoutDirection) private var direction

    init(model: ReportColumnModel, legend: [ReportLegendItem]? = nil, height: CGFloat = 220) {
        full = model
        self.legend = legend
        self.height = height
    }

    private var model: ReportColumnModel { full.without(hidden) }

    var body: some View {
        let model = model
        let count = model.keys.count
        let shown = selected.flatMap { $0 < count ? $0 : nil } ?? model.opening
        VStack(alignment: .leading, spacing: 8) {
            if shown >= 0 && shown < model.readouts.count { ReportReadoutView(title: model.titles[shown], rows: model.readouts[shown]) }
            // Months run left to right in every language, so a tap's column is counted from the left.
            GeometryReader { geometry in plot(width: geometry.size.width) }
                .environment(\.layoutDirection, .leftToRight)
                .frame(height: height)
            if let legend, !legend.isEmpty {
                ReportLegendView(items: legend, hidden: hidden, onToggle: legend.count > 1 ? { label in
                    withAnimation(.easeInOut(duration: 0.25)) { hidden = ReportLegendView.toggled(hidden, label, legend: legend) }
                } : nil)
            }
        }
        .onChange(of: full.keys) { selected = nil }
        .onChange(of: legend?.map(\.label)) { hidden = [] }
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
            // The stretch into the month under way is dashed: it starts again from the month before.
            if index == model.partial, let before = out.last, before.series == "line-\(segment)" {
                segment += 1
                out.append(ReportLinePoint(id: out.count, key: before.key, value: before.value, series: "line-\(segment)", color: line.color, dashed: true,
                                           label: line.label, text: "", title: before.title, echo: true))
            }
            out.append(ReportLinePoint(id: out.count, key: model.keys[index], value: value, series: "line-\(segment)", color: line.color, dashed: index == model.partial,
                                       label: line.label, text: index < model.lineTexts.count ? model.lineTexts[index] ?? "" : "", title: model.titles[index]))
        }
        return out
    }

    private func plot(width: CGFloat) -> some View {
        let model = model
        let count = max(1, model.keys.count)
        let band = max(1, width - 52) / CGFloat(count)
        let underWay: String? = model.partial >= 0 && model.partial < model.keys.count ? model.keys[model.partial] : nil
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
                        .foregroundStyle(ReportPalette.color(bar.color).opacity(bar.key == underWay ? ReportPalette.soFar : 1))
                        .cornerRadius(3)
                        .accessibilityLabel("\(bar.title), \(bar.label)")
                        .accessibilityValue(bar.text)
                } else {
                    BarMark(x: .value("Month", bar.key), yStart: .value("Amount", bar.start), yEnd: .value("Amount", bar.end), width: .fixed(barWidth))
                        .foregroundStyle(ReportPalette.color(bar.color).opacity(bar.key == underWay ? ReportPalette.soFar : 1))
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
                    .lineStyle(StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round, dash: point.dashed ? [3, 3] : []))
                    .accessibilityLabel("\(point.title), \(point.label)")
                    .accessibilityValue(point.text)
                    .accessibilityHidden(point.echo)
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
/// With a `legend`, its names hide and show their lines and the scale follows. The stretch into the
/// month still under way is dashed, and the chart opens on the last complete one.
struct ReportLinesChart: View {
    /// Every line; `model` is what's drawn, without the hidden ones.
    private var full: ReportLinesModel
    private var legend: [ReportLegendItem]?
    private var height: CGFloat
    @State private var selected: Int?
    @State private var hidden: Set<String> = []
    @ScaledMetric(relativeTo: .caption2) private var labelSpacing: CGFloat = 52

    init(model: ReportLinesModel, legend: [ReportLegendItem]? = nil, height: CGFloat = 220) {
        full = model
        self.legend = legend
        self.height = height
    }

    private var model: ReportLinesModel { full.without(hidden) }

    var body: some View {
        let model = model
        let count = model.keys.count
        let shown = selected.flatMap { $0 < count ? $0 : nil } ?? model.opening
        VStack(alignment: .leading, spacing: 8) {
            if shown >= 0 && shown < model.readouts.count { ReportReadoutView(title: model.titles[shown], rows: model.readouts[shown]) }
            // Time runs left to right in every language, so a tap's point is counted from the left.
            GeometryReader { geometry in plot(width: geometry.size.width) }
                .environment(\.layoutDirection, .leftToRight)
                .frame(height: height)
            if let legend, !legend.isEmpty {
                ReportLegendView(items: legend, hidden: hidden, onToggle: legend.count > 1 ? { label in
                    withAnimation(.easeInOut(duration: 0.25)) { hidden = ReportLegendView.toggled(hidden, label, legend: legend) }
                } : nil)
            }
        }
        .onChange(of: full.keys) { selected = nil }
        .onChange(of: legend?.map(\.label)) { hidden = [] }
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
                // The stretch into the month under way is dashed: it starts again from the month before.
                if index == model.partial, !series.dashed, let before = out.last, before.series == "\(series.key)#\(segment)" {
                    segment += 1
                    out.append(ReportLinePoint(id: out.count, key: before.key, value: before.value, series: "\(series.key)#\(segment)", color: series.color,
                                               dashed: true, label: series.label, text: "", title: before.title, echo: true))
                }
                out.append(ReportLinePoint(id: out.count, key: model.keys[index], value: value, series: "\(series.key)#\(segment)", color: series.color,
                                           dashed: series.dashed || index == model.partial, label: series.label, text: text, title: model.titles[index]))
            }
        }
        return out
    }

    private func plot(width: CGFloat) -> some View {
        let model = model
        let count = max(1, model.keys.count)
        let band = max(1, width - 52) / CGFloat(count)
        let labels = ReportAxis.xLabels(model.xTicks, keys: model.keys, band: band, spacing: labelSpacing)
        let all = points()
        let ends = model.series.indices.compactMap { k in all.last { $0.series.hasPrefix("\(model.series[k].key)#") } }
        let low = model.ticks.first ?? 0
        let high = model.ticks.last ?? 1
        let pointMarks = count <= 1
        let chosen: [ReportLinePoint] = selected.flatMap { index in index < model.keys.count ? all.filter { $0.key == model.keys[index] && !$0.echo } : nil } ?? []
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
                    .accessibilityHidden(point.echo)
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

/// Parts of a whole as a ring, largest first, the total in the middle. A tap picks a slice: it
/// stands out, the others fade, and the middle says its amount and share. A second tap, or a tap
/// on the middle, opens it, where there's something to open.
struct ReportDonut: View {
    var slices: [ReportSlice]
    var centerValue: String
    var centerLabel: String
    var picked: String? = nil
    var onPick: ((String?) -> Void)? = nil
    var onOpen: ((String) -> Void)? = nil
    /// What opening a slice shows: "See transactions", "See categories".
    var openLabel: String = String(localized: "See transactions")
    @ScaledMetric(relativeTo: .body) private var size: CGFloat = 200

    var body: some View {
        let shown = slices.filter { $0.value > 0 }
        let side = min(size, 280)
        let total = shown.map(\.value).reportSum
        let chosen = shown.first { $0.key == picked }
        let opens = chosen.map { canOpen($0.key) } ?? false
        ZStack {
            if shown.isEmpty {
                Circle().stroke(Color.insetBackground, lineWidth: side * 0.19).padding(side * 0.095)
            } else {
                Chart(shown) { slice in
                    SectorMark(angle: .value("Amount", Double(slice.value)), innerRadius: .ratio(0.62),
                               outerRadius: .ratio(chosen == nil || slice.key == picked ? 1 : 0.95), angularInset: shown.count > 1 ? 1 : 0)
                        .foregroundStyle(ReportPalette.color(slice.color).opacity(chosen == nil || slice.key == picked ? 1 : 0.35))
                        .cornerRadius(2)
                        .accessibilityLabel(slice.label)
                        .accessibilityValue("\(slice.valueText), \(ReportMath.share(slice.value, total))")
                }
                .chartLegend(.hidden)
                .chartOverlay { _ in
                    GeometryReader { geometry in
                        if onPick != nil {
                            Rectangle().fill(.clear).contentShape(Rectangle())
                                .onTapGesture { location in tapped(sliceKey(at: location, in: geometry.size, shown: shown), inMiddle: inMiddle(location, geometry.size)) }
                                .accessibilityHidden(true)
                        }
                    }
                }
                // Clockwise from the top in every language, as `sliceKey` reads a tap.
                .environment(\.layoutDirection, .leftToRight)
                .animation(.easeInOut(duration: 0.2), value: picked)
            }
            VStack(spacing: 1) {
                if let chosen {
                    Text(chosen.valueText).font(.headline).monospacedDigit().lineLimit(1).minimumScaleFactor(0.5)
                    Text("\(chosen.label) · \(ReportMath.share(chosen.value, total))").font(.caption).foregroundStyle(Color.secondary)
                        .lineLimit(2).multilineTextAlignment(.center).minimumScaleFactor(0.8)
                    if opens { Text(openLabel).font(.caption.weight(.semibold)).foregroundStyle(Color.urAccent).padding(.top, 2) }
                } else {
                    Text(centerValue).font(.headline).monospacedDigit().lineLimit(1).minimumScaleFactor(0.5)
                    Text(centerLabel).font(.caption).foregroundStyle(Color.secondary).lineLimit(1)
                }
            }
            .frame(width: side * 0.54)
            .allowsHitTesting(false)
            .accessibilityElement(children: .combine)
        }
        .frame(width: side, height: side)
        .frame(maxWidth: .infinity)
        .sensoryFeedback(.selection, trigger: picked)
        .accessibilityAction(named: Text(openLabel)) { if let chosen, opens { onOpen?(chosen.key) } }
    }

    private func canOpen(_ key: String) -> Bool { onOpen != nil && key != "__other" }

    /// A slice picks it, the picked slice or the middle opens it, and anywhere else lets go.
    private func tapped(_ key: String?, inMiddle: Bool) {
        if let picked, key == picked || (key == nil && inMiddle) {
            if canOpen(picked) { onOpen?(picked) } else if key == nil { onPick?(nil) }
        } else {
            onPick?(key)
        }
    }

    private func inMiddle(_ location: CGPoint, _ size: CGSize) -> Bool {
        let dx = location.x - size.width / 2, dy = location.y - size.height / 2
        return (dx * dx + dy * dy).squareRoot() < min(size.width, size.height) / 2 * 0.6
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
    /// With something picked on the chart: whether it's this row. The others fade.
    var picked: Bool? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                if let color = row.color { ReportLegendKey(color: ReportPalette.color(color)) }
                // The name has what the figures leave, on two lines if it must: they're never cut.
                Text(row.label).lineLimit(2).layoutPriority(1)
                Spacer(minLength: 4)
                Text(row.valueText).fontWeight(.semibold).monospacedDigit().lineLimit(1).fixedSize().layoutPriority(2)
                if let detail = row.detail {
                    Text(detail).font(.footnote).foregroundStyle(Color.secondary).monospacedDigit().lineLimit(1).fixedSize()
                        .frame(minWidth: 40, alignment: .trailing).layoutPriority(2)
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
        .opacity(picked == false ? 0.45 : 1)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(row.label)
        .accessibilityValue([row.valueText, row.detail].compactMap { $0 }.joined(separator: ", "))
        .accessibilityAddTraits(picked == true ? .isSelected : [])
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

/// Days a month at a time, as a calendar: each day shaded in five steps by how much was spent, its
/// date in the corner. Swipe or use the arrows for other months; it opens on the latest. Days
/// outside the report's range are left plain.
struct ReportMonthCalendar: View {
    var model: ReportHeatModel
    var selected: Int
    var onDay: (Int) -> Void
    @State private var page: Int?

    var body: some View {
        let months = ReportMonthCalendar.months(model)
        let shown = min(max(0, page ?? months.count - 1), max(0, months.count - 1))
        let weekStart = ReportMonthCalendar.weekStart(model)
        VStack(spacing: 6) {
            HStack {
                step(-1, "chevron.backward", String(localized: "Previous month"), enabled: shown > 0, shown: shown)
                Text(months.indices.contains(shown) ? ReportMath.monthName(months[shown]) : "")
                    .font(.subheadline.weight(.semibold)).lineLimit(1).frame(maxWidth: .infinity)
                step(1, "chevron.forward", String(localized: "Next month"), enabled: shown < months.count - 1, shown: shown)
            }
            HStack(spacing: 4) {
                ForEach(0..<7, id: \.self) { k in
                    Text(ReportMath.weekdayName((k + weekStart) % 7, short: true)).font(.caption2).foregroundStyle(Color.secondary)
                        .lineLimit(1).minimumScaleFactor(0.7).frame(maxWidth: .infinity)
                }
            }
            .accessibilityHidden(true)
            if months.indices.contains(shown) { grid(months[shown], weekStart: weekStart) }
        }
        .gesture(DragGesture(minimumDistance: 24).onEnded { drag in
            guard abs(drag.translation.width) > abs(drag.translation.height) * 1.5 else { return }
            // A swipe towards the start of the line brings the next month, in either direction of writing.
            let forward = (drag.translation.width < 0) != AppLanguage.isRightToLeft
            let next = shown + (forward ? 1 : -1)
            if months.indices.contains(next) { withAnimation(.easeInOut(duration: 0.2)) { page = next } }
        })
        .onChange(of: model.days) { page = nil }
        .onChange(of: selected) { _, index in
            // A day picked from elsewhere (the Biggest days list) brings its month.
            guard model.days.indices.contains(index), let at = months.firstIndex(of: String(model.days[index].prefix(7))) else { return }
            if at != shown { page = at }
        }
    }

    private func step(_ by: Int, _ symbol: String, _ label: String, enabled: Bool, shown: Int) -> some View {
        Button { withAnimation(.easeInOut(duration: 0.2)) { page = shown + by } } label: {
            Image(systemName: symbol).font(.body.weight(.semibold)).frame(width: 44, height: 36).contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .disabled(!enabled)
        .accessibilityLabel(label)
    }

    private func grid(_ month: String, weekStart: Int) -> some View {
        let cells = ReportMonthCalendar.cells(month, weekStart: weekStart)
        var index: [String: Int] = [:]
        for (at, day) in model.days.enumerated() { index[day] = at }
        let weeks = stride(from: 0, to: cells.count, by: 7).map { Array(cells[$0..<min($0 + 7, cells.count)]) }
        return VStack(spacing: 4) {
            ForEach(weeks.indices, id: \.self) { w in
                HStack(spacing: 4) {
                    ForEach(0..<7, id: \.self) { k in
                        let day: String? = k < weeks[w].count ? weeks[w][k] : nil
                        Group {
                            if let day { cell(day, at: index[day]) } else { Color.clear }
                        }
                        .frame(maxWidth: .infinity)
                        .aspectRatio(1.15, contentMode: .fit)
                    }
                }
            }
        }
    }

    @ViewBuilder private func cell(_ day: String, at index: Int?) -> some View {
        let level = index.map { min(5, max(0, model.levels[$0])) }
        let picked = index != nil && index == selected
        let shape = RoundedRectangle(cornerRadius: 8, style: .continuous)
        let date = String(Int(day.suffix(2)) ?? 0)
        let face = Text(date).font(.caption2.weight(picked ? .bold : .medium)).monospacedDigit()
            .foregroundStyle(level.map(ReportPalette.heatInk) ?? Color.secondary.opacity(0.55))
            .padding(.horizontal, 5).padding(.vertical, 3)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(level.map { ReportPalette.heat[$0] } ?? Color.clear, in: shape)
            .overlay { if level == 0 { shape.strokeBorder(Color.primary.opacity(0.12)) } }
            .overlay { if picked { shape.strokeBorder(Color.primary, lineWidth: 2) } }
        if let index {
            Button { onDay(index) } label: { face }
                .buttonStyle(.plain)
                .accessibilityLabel(model.titles[index])
                .accessibilityValue(model.spent[index] ? String(localized: "\(model.amounts[index]) spent") : String(localized: "Nothing spent"))
                .accessibilityAddTraits(picked ? .isSelected : [])
        } else {
            face.accessibilityHidden(true)
        }
    }

    /// The months the report's days fall in, in order.
    static func months(_ model: ReportHeatModel) -> [String] {
        var out: [String] = []
        for day in model.days where out.last != String(day.prefix(7)) { out.append(String(day.prefix(7))) }
        return out
    }

    /// The week starts where the report's does: the first day's weekday less its place in the week.
    static func weekStart(_ model: ReportHeatModel) -> Int {
        guard let first = model.days.first else { return 0 }
        return ((ReportMath.jsWeekday(first) - model.lead) % 7 + 7) % 7
    }

    /// A month's days in calendar order, with a gap (nil) for each weekday before its first.
    static func cells(_ month: String, weekStart: Int) -> [String?] {
        let first = "\(month)-01"
        let (_, last) = Dates.monthBounds(month)
        let count = Int(last.suffix(2)) ?? 30
        let lead = ((ReportMath.jsWeekday(first) - weekStart) % 7 + 7) % 7
        return Array(repeating: nil, count: lead) + (1...max(1, count)).map { String(format: "%@-%02d", month, $0) }
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

/// Tiles with areas in proportion to their values, each in its group's colour; labels where they
/// fit. A tap picks a tile (the others fade); a second tap opens it.
struct ReportTreemap: View {
    var items: [ReportTreemapItem]
    var picked: String? = nil
    var onPick: (ReportTreemapItem?) -> Void
    var onOpen: (ReportTreemapItem) -> Void
    var height: CGFloat = 280
    @Environment(\.layoutDirection) private var direction

    /// Tiles are placed by offsets from the left edge, so they're laid out left to right; in Arabic the
    /// largest starts at the right, mirrored by hand, and each tile's text keeps the app's direction.
    var body: some View {
        GeometryReader { geometry in
            let sorted = items.filter { $0.value > 0 }.reportSorted(descending: { $0.value })
            let boxes = ReportMath.squarify(sorted, value: { Double($0.value) }, width: geometry.size.width, height: geometry.size.height)
            ZStack(alignment: .topLeading) {
                // A tap between tiles lets go of the one picked.
                Color.clear.contentShape(Rectangle()).onTapGesture { onPick(nil) }.accessibilityHidden(true)
                ForEach(boxes.indices, id: \.self) { index in tile(boxes[index], width: geometry.size.width) }
            }
        }
        .environment(\.layoutDirection, .leftToRight)
        .frame(height: height)
        .sensoryFeedback(.selection, trigger: picked)
    }

    private func tile(_ box: ReportTileBox<ReportTreemapItem>, width: CGFloat) -> some View {
        let item = box.item
        let w = max(0, box.w - 2)
        let h = max(0, box.h - 2)
        let x: Double = direction == .rightToLeft ? Double(width) - box.x - box.w : box.x
        let chosen = item.key == picked
        let faded = picked != nil && !chosen
        return Button { if chosen { onOpen(item) } else { onPick(item) } } label: {
            VStack(alignment: .leading, spacing: 2) {
                if w > 44 && h > 22 { Text(item.label).font(.caption.weight(.semibold)).lineLimit(1) }
                if w > 76 && h > 40 { Text(item.valueText).font(.caption).monospacedDigit().lineLimit(1) }
            }
            .opacity(faded ? 0.5 : 1)
            .padding(.horizontal, 7)
            .padding(.vertical, 5)
            .frame(width: w, height: h, alignment: .topLeading)
            .background(ReportPalette.tile(item.color).opacity(faded ? 0.35 : 1), in: RoundedRectangle(cornerRadius: 4, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 4, style: .continuous).strokeBorder(chosen ? Color.primary : Color.primary.opacity(0.1), lineWidth: chosen ? 2 : 1))
            .clipped()
            .foregroundStyle(Color.primary)
            .environment(\.layoutDirection, direction)
        }
        .buttonStyle(.plain)
        .offset(x: x + 1, y: box.y + 1)
        .accessibilityLabel(item.label)
        .accessibilityValue("\(item.valueText), \(item.detail), \(item.more)")
        .accessibilityHint(chosen ? Text("Shows this payee's transactions") : Text("Picks this payee"))
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }
}

/// Small columns, one group a month, scaled to the largest value; negatives hang below the line in
/// red. The highest and latest columns say their figure (with the model's `short`). With `onSelect`,
/// a finger picks a month (tapping or dragging across) and the others fade.
struct ReportMiniBars: View {
    var model: ReportMiniBarsModel
    var selected: Int = -1
    var onSelect: ((Int) -> Void)? = nil

    var body: some View {
        let ids = model.labels.indices.map(String.init)
        let labelled = ReportMiniBars.labelled(model)
        Chart {
            ForEach(model.series, id: \.label) { (series: ReportMiniSeries) in
                ForEach(Array(series.values.enumerated()), id: \.offset) { pair in
                    bar(series, index: pair.offset, value: pair.element, labelled: labelled)
                }
            }
            RuleMark(y: .value("Zero", 0.0)).foregroundStyle(Color.secondary.opacity(0.6)).lineStyle(StrokeStyle(lineWidth: 1)).accessibilityHidden(true)
        }
        .chartXScale(domain: ids)
        .chartXAxis {
            AxisMarks(values: ids) { value in
                let index = Int(value.as(String.self) ?? "") ?? 0
                AxisValueLabel { Text(model.labels[index]).fontWeight(index == selected ? .bold : .regular).foregroundStyle(index == selected ? Color.primary : Color.secondary) }
            }
        }
        .chartYAxis(.hidden)
        .chartLegend(.hidden)
        .chartOverlay { proxy in
            GeometryReader { geometry in
                if let onSelect {
                    Rectangle().fill(.clear).contentShape(Rectangle())
                        .onTapGesture { location in
                            guard let index = ReportAxis.index(at: location, proxy: proxy, geometry: geometry, count: model.labels.count) else { return }
                            onSelect(index == selected ? -1 : index)
                        }
                        // Dragging across reads each month in turn; an up-and-down drag still scrolls the page.
                        .simultaneousGesture(DragGesture(minimumDistance: 10).onChanged { drag in
                            guard abs(drag.translation.width) > abs(drag.translation.height),
                                  let index = ReportAxis.index(at: drag.location, proxy: proxy, geometry: geometry, count: model.labels.count) else { return }
                            if index != selected { onSelect(index) }
                        })
                        .accessibilityHidden(true)
                }
            }
        }
        // Months run left to right, as on the reports' other charts.
        .environment(\.layoutDirection, .leftToRight)
        .frame(height: labelled.isEmpty ? 130 : 146)
        .sensoryFeedback(.selection, trigger: selected)
        .accessibilityLabel(spoken)
    }

    /// The columns that say their figure: the highest and the latest, for a single series with short figures.
    static func labelled(_ model: ReportMiniBarsModel) -> Set<Int> {
        guard model.series.count == 1, let short = model.short.first, let values = model.series.first?.values, !values.isEmpty else { return [] }
        var out: Set<Int> = [values.count - 1]
        if let highest = values.indices.max(by: { values[$0] < values[$1] }) { out.insert(highest) }
        return out.filter { $0 < short.count && !short[$0].isEmpty }
    }

    /// "Income and Spending by month".
    private var spoken: String {
        let labels = model.series.map(\.label)
        if labels.count == 2 { return String(localized: "\(labels[0]) and \(labels[1]) by month") }
        let joined = labels.joined(separator: " and ")
        return String(localized: "\(joined) by month")
    }

    /// One column. Its parts are worked out first: as one expression, Xcode 26 can't type-check it in time.
    private func bar(_ series: ReportMiniSeries, index: Int, value: Int64, labelled: Set<Int>) -> some ChartContent {
        let month: String = index < model.labels.count ? model.labels[index] : ""
        let spoken: String = index < series.texts.count ? series.texts[index] : ""
        let label: String = "\(month), \(series.label)"
        let base: Color = value < 0 ? Color.urNegative : ReportPalette.color(series.color)
        // The month under way is lighter; with a month picked, the others fade.
        let strength: Double = selected >= 0 ? (index == selected ? 1 : 0.35) : (index == model.partial ? ReportPalette.soFar : 1)
        let width: MarkDimension = .ratio(model.series.count > 1 ? 0.8 : 0.55)
        let figure: String = labelled.contains(index) ? (model.short.first?[index] ?? "") : ""
        return BarMark(x: .value("Month", String(index)), y: .value("Amount", Double(value)), width: width)
            .position(by: .value("Series", series.label))
            .foregroundStyle(base.opacity(strength))
            .cornerRadius(3)
            .annotation(position: value < 0 ? .bottom : .top, spacing: 2) {
                if !figure.isEmpty { Text(figure).font(.caption2).monospacedDigit().foregroundStyle(Color.secondary).fixedSize() }
            }
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
            Text(tile.value).font(.title3.weight(.semibold)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.7)
                .fixedSize(horizontal: false, vertical: true)
                .foregroundStyle(tile.tone == .negative ? Color.urNegative : tile.tone == .positive ? Color.urPositive : Color.primary)
            if let note = tile.note {
                Text(note).font(.caption).foregroundStyle(Color.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(.horizontal, 12)
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
        // Each row is as tall as its taller tile at the width it really has. (A grid sized its
        // rows before a note wrapped, and the figure above a two-line note was squeezed smaller.)
        VStack(spacing: 10) {
            ForEach(rows.indices, id: \.self) { index in
                HStack(alignment: .top, spacing: 10) {
                    ForEach(Array(rows[index].enumerated()), id: \.offset) { _, tile in ReportTileCard(tile: tile) }
                    if rows[index].count < columns { Color.clear.frame(maxWidth: .infinity).frame(height: 1) }
                }
                .fixedSize(horizontal: false, vertical: true)
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
