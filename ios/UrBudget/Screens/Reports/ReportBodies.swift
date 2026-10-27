import SwiftUI

/// A card's title over the list section, with an optional line under it.
struct ReportSectionHeader: View {
    var title: String
    var aside: String? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.headline).foregroundStyle(Color.primary)
            if let aside { Text(aside).font(.footnote).foregroundStyle(Color.secondary) }
        }
        .textCase(nil)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// The headline figures as their own section, on the grouped background.
struct ReportTilesSection: View {
    var tiles: [ReportTile]

    var body: some View {
        if !tiles.isEmpty {
            Section {
                ReportTilesView(tiles: tiles)
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
            }
        }
    }
}

/// What a report body can ask of its page.
struct ReportActions {
    /// The navigation value for the transactions behind a figure, in the page's range and filters.
    var drill: (ReportDrillTarget) -> ReportNav
    var navigate: (ReportNav) -> Void
    var toggleChart: (String) -> Void
    var scrollTo: (String) -> Void
}

private struct ReportRowSection: Identifiable {
    var id: String
    var title: String?
    var rows: [ReportFigureRow]
}

/// A report table's rows as list sections: a section row starts one, and the net rows at the end get their own.
private func reportRowSections(_ rows: [ReportFigureRow], key: String) -> [ReportRowSection] {
    var out: [ReportRowSection] = []
    var current = ReportRowSection(id: "\(key):start", title: nil, rows: [])
    for row in rows {
        if row.style == .section {
            if !current.rows.isEmpty || current.title != nil { out.append(current) }
            current = ReportRowSection(id: "\(key):\(row.key)", title: row.title, rows: [])
        } else {
            if row.style == .net, let last = current.rows.last, last.style != .net {
                out.append(current)
                current = ReportRowSection(id: "\(key):\(row.key)", title: nil, rows: [])
            }
            current.rows.append(row)
        }
    }
    if !current.rows.isEmpty || current.title != nil { out.append(current) }
    return out
}

/// A row that opens what's behind it, or just the row.
private struct ReportLinkRow<Label: View>: View {
    var value: ReportNav?
    @ViewBuilder var label: () -> Label

    var body: some View {
        if let value {
            NavigationLink(value: value) { label() }
        } else {
            label()
        }
    }
}

/// Lays out a report's body as list sections.
struct ReportBodyView: View {
    var report: ReportBody
    var actions: ReportActions
    @Binding var byGroup: Bool
    var payees: [ReportPayeeRow]
    var payeeQuery: String

    var body: some View {
        switch report {
        case .categories(let model): ReportCategoryBody(model: model, actions: actions)
        case .payees(let model): ReportPayeeBody(model: model, actions: actions, rows: payees, query: payeeQuery)
        case .trends(let model): ReportOverTimeBody(model: model, byGroup: $byGroup)
        case .calendar(let model): ReportCalendarBody(model: model, actions: actions)
        case .incomeExpense(let model): ReportIncomeExpenseBody(model: model)
        case .income(let model): ReportIncomeBody(model: model, actions: actions)
        case .cashflow(let model): ReportCashFlowBody(model: model)
        case .networth(let model): ReportNetWorthBody(model: model)
        case .balance(let model): ReportBalanceSheetBody(model: model)
        case .balances(let model): ReportBalancesBody(model: model, actions: actions)
        case .age(let model): ReportAgeBody(model: model)
        }
    }
}

private struct ReportCategoryBody: View {
    var model: ReportCategoryModel
    var actions: ReportActions

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            if model.rows.isEmpty {
                ContentUnavailableView("No spending", systemImage: "chart.pie", description: Text("No spending in these months."))
            } else {
                ReportDonut(slices: model.slices, centerValue: model.centerValue, centerLabel: model.centerLabel) { key in
                    if let value = link(key) { actions.navigate(value) }
                }
                .padding(.vertical, 8)
                ForEach(model.rows) { row in
                    ReportLinkRow(value: link(row.key)) { ReportBarRowView(row: row) }
                }
            }
        } header: {
            ReportSectionHeader(title: model.group ?? String(localized: "Spending by group"))
        } footer: {
            Text(model.help)
        }
        if let changes = model.changes {
            Section {
                if changes.rows.isEmpty {
                    Text("Spending matched the average.").foregroundStyle(Color.secondary)
                } else {
                    ForEach(changes.rows) { ReportDivergingRowView(row: $0) }
                }
            } header: {
                ReportSectionHeader(title: changes.title, aside: changes.aside)
            } footer: {
                if !changes.rows.isEmpty { ReportLegendView(items: changes.legend).padding(.top, 2) }
            }
        }
    }

    /// A group opens its categories; a category opens its transactions.
    private func link(_ key: String) -> ReportNav? {
        guard key != "__other" else { return nil }
        guard model.group != nil else { return .report("categories", group: key) }
        let label = model.rows.first { $0.key == key }?.label ?? key
        return actions.drill(ReportDrillTarget(title: label, dimension: "category", key: key))
    }
}

private struct ReportPayeeBody: View {
    var model: ReportPayeeModel
    var actions: ReportActions
    var rows: [ReportPayeeRow]
    var query: String

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            if model.map.isEmpty {
                ContentUnavailableView("No payees", systemImage: "storefront", description: Text("No payees with net spending in these months."))
            } else {
                ReportTreemap(items: model.map) { item in
                    actions.navigate(actions.drill(ReportDrillTarget(title: item.label, dimension: "payee", key: item.key)))
                }
                .padding(.vertical, 6)
                .listRowInsets(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
            }
            if let more = model.more {
                Button { actions.scrollTo("payees:all") } label: {
                    Label(more, systemImage: "arrow.down").font(.subheadline.weight(.semibold))
                }
            }
        } header: {
            ReportSectionHeader(title: model.title, aside: model.aside)
        } footer: {
            Text(model.help)
        }
        Section {
            Text(countText)
                .font(.footnote).foregroundStyle(Color.secondary)
                .id("payees:all")
            if rows.isEmpty {
                ContentUnavailableView.search(text: query)
            }
            ForEach(rows) { payee in
                NavigationLink(value: actions.drill(ReportDrillTarget(title: payee.label, dimension: "payee", key: payee.name))) {
                    HStack(spacing: 10) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(payee.label).lineLimit(1)
                            Text(payee.detail).font(.footnote).monospacedDigit().foregroundStyle(Color.secondary).lineLimit(2)
                        }
                        Spacer(minLength: 8)
                        Text(payee.amountText).fontWeight(.medium).monospacedDigit().lineLimit(1)
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        } header: {
            ReportSectionHeader(title: String(localized: "All payees"))
        }
    }

    /// "12 payees", or "3 of 12 payees" while searching.
    private var countText: String {
        query.isEmpty ? String(localized: "\(model.rows.count) payees", locale: .app)
            : String(localized: "\(rows.count) of \(model.rows.count) payees", locale: .app)
    }
}

private struct ReportOverTimeBody: View {
    var model: ReportOverTimeModel
    @Binding var byGroup: Bool

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            Picker("Show", selection: $byGroup) {
                Text("By group").tag(true)
                Text("Total").tag(false)
            }
            .pickerStyle(.segmented)
            VStack(alignment: .leading, spacing: 10) {
                ReportColumnChart(model: model.chart)
                if !model.legend.isEmpty { ReportLegendView(items: model.legend) }
            }
            .padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Spending by month"))
        }
        Section {
            ForEach(model.rows) { ReportFigureRowView(row: $0) }
        } header: {
            ReportSectionHeader(title: String(localized: "Month by month"))
        }
    }
}

private struct ReportHeatPanel: View {
    var heat: ReportHeatModel
    var onDrill: (Int) -> Void
    @State private var day = -1

    var body: some View {
        let chosen = heat.days.indices.contains(day)
        VStack(alignment: .leading, spacing: 10) {
            Text(caption)
                .font(.footnote)
                .foregroundStyle(chosen ? .primary : .secondary)
            ReportHeatCalendar(model: heat, selected: day) { index in
                day = index
                if heat.spent[index] { onDrill(index) }
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Spending by day")
            .accessibilityValue("\(heat.spent.filter { $0 }.count) of \(heat.days.count) days with spending. The Biggest days list opens each day's transactions.")
            ReportHeatLegend()
        }
        .padding(.vertical, 6)
        .onChange(of: heat.days) { day = -1 }
    }

    /// The chosen day ("Thursday 2026-10-01 · 12.00 spent"), or how to choose one.
    private var caption: String {
        guard heat.days.indices.contains(day) else { return String(localized: "Tap a day for its spending. Darker means more.") }
        let amount = heat.amounts[day]
        let spent = heat.spent[day] ? String(localized: "\(amount) spent") : String(localized: "\(amount) nothing spent")
        return "\(heat.titles[day]) · \(spent)"
    }
}

private struct ReportCalendarBody: View {
    var model: ReportCalendarModel
    var actions: ReportActions

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            if let heat = model.heat {
                ReportHeatPanel(heat: heat) { index in
                    let day = heat.days[index]
                    actions.navigate(actions.drill(ReportDrillTarget(title: heat.titles[index], dimension: "day", key: day, from: day, to: day)))
                }
            } else {
                ContentUnavailableView("Not started yet", systemImage: "calendar", description: Text("These months haven't started yet."))
            }
        } header: {
            ReportSectionHeader(title: String(localized: "Spending by day"))
        }
        Section {
            ReportColumnChart(model: model.weekdays, height: 200).padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "By day of the week"), aside: String(localized: "Average spent"))
        }
        Section {
            if model.biggest.isEmpty { Text("No spending in these months.").foregroundStyle(Color.secondary) }
            ForEach(model.biggest) { row in
                ReportLinkRow(value: row.drill.map(actions.drill)) { ReportFigureRowView(row: row) }
            }
        } header: {
            ReportSectionHeader(title: String(localized: "Biggest days"))
        }
    }
}

/// Rows split into sections, a group's categories under a disclosure.
private struct ReportTableSections: View {
    var key: String
    var rows: [ReportFigureRow]
    var children: [String: [ReportFigureRow]] = [:]
    var header: String? = nil
    var aside: String? = nil
    var footer: String? = nil

    var body: some View {
        let sections = reportRowSections(rows, key: key)
        ForEach(Array(sections.enumerated()), id: \.element.id) { index, section in
            Section {
                ForEach(section.rows) { row in
                    if let expand = row.expand, let kids = children[expand], !kids.isEmpty {
                        DisclosureGroup {
                            ForEach(kids) { ReportFigureRowView(row: $0) }
                        } label: {
                            ReportFigureRowView(row: row)
                        }
                    } else {
                        ReportFigureRowView(row: row)
                    }
                }
            } header: {
                if index == 0, let header {
                    VStack(alignment: .leading, spacing: 8) {
                        ReportSectionHeader(title: header, aside: aside)
                        if let title = section.title { Text(title) }
                    }
                } else if let title = section.title {
                    Text(title)
                }
            } footer: {
                if index == sections.count - 1, let footer { Text(footer) }
            }
        }
    }
}

private struct ReportIncomeExpenseBody: View {
    var model: ReportIncomeExpenseModel

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            VStack(alignment: .leading, spacing: 10) {
                ReportColumnChart(model: model.chart)
                ReportLegendView(items: model.legend)
            }
            .padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Income and spending by month"))
        } footer: {
            Text(model.help)
        }
        Section {
            ReportLinesChart(model: model.rate, height: 200).padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Budget surplus rate"), aside: String(localized: "Surplus as a share of income"))
        }
        Section {
            ReportLinesChart(model: model.cumulative, height: 200).padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Cumulative budget surplus"), aside: String(localized: "Surplus added up"))
        }
        ReportTableSections(key: "table", rows: model.table, children: model.children, header: String(localized: "Income and spending"),
                            aside: String(localized: "Open a group to see its categories"))
        Section {
            ForEach(model.months) { ReportFigureRowView(row: $0) }
        } header: {
            ReportSectionHeader(title: String(localized: "By month"), aside: String(localized: "Budget surplus each month"))
        }
    }
}

private struct ReportIncomeBody: View {
    var model: ReportIncomeModel
    var actions: ReportActions

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            if model.bars.isEmpty {
                ContentUnavailableView("No income", systemImage: "banknote", description: Text("No income in these months."))
            } else {
                ReportDonut(slices: model.slices, centerValue: model.centerValue, centerLabel: String(localized: "Positive sources")).padding(.vertical, 8)
                ForEach(model.bars) { ReportBarRowView(row: $0) }
            }
        } header: {
            ReportSectionHeader(title: String(localized: "Where your income comes from"))
        } footer: {
            if let help = model.chartHelp { Text(help) }
        }
        if let monthly = model.monthly {
            Section {
                VStack(alignment: .leading, spacing: 10) {
                    ReportColumnChart(model: monthly)
                    ReportLegendView(items: model.legend)
                }
                .padding(.vertical, 6)
            } header: {
                ReportSectionHeader(title: String(localized: "Income by month"))
            }
        }
        Section {
            if model.sources.isEmpty { Text("No income sources in these months.").foregroundStyle(Color.secondary) }
            ForEach(model.sources) { row in
                ReportLinkRow(value: row.drill.map(actions.drill)) { ReportFigureRowView(row: row) }
            }
        } header: {
            ReportSectionHeader(title: String(localized: "Sources"))
        } footer: {
            if let help = model.sourcesHelp { Text(help) }
        }
    }
}

private struct ReportCashFlowBody: View {
    var model: ReportCashFlowModel

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            VStack(alignment: .leading, spacing: 10) {
                ReportColumnChart(model: model.chart)
                ReportLegendView(items: model.legend)
            }
            .padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Money in and out by month"))
        } footer: {
            Text("Account movements across your budget boundary, including credit-card purchases and transfers to tracking accounts. Card payments, other transfers between budget accounts and starting balances are excluded. Split inflows and outflows may be netted; these are account movements rather than gross category splits.")
        }
        Section {
            ReportLinesChart(model: model.running, height: 200).padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Running total"), aside: String(localized: "Net flow added up"))
        }
        Section {
            ForEach(model.months) { ReportFigureRowView(row: $0) }
        } header: {
            ReportSectionHeader(title: String(localized: "By month"), aside: String(localized: "Net flow each month"))
        }
    }
}

private struct ReportNetWorthBody: View {
    var model: ReportNetWorthModel

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            VStack(alignment: .leading, spacing: 10) {
                ReportColumnChart(model: model.chart)
                ReportLegendView(items: model.legend)
            }
            .padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Assets, debts and net worth"))
        }
        Section {
            VStack(alignment: .leading, spacing: 10) {
                ReportColumnChart(model: model.change, height: 200)
                ReportLegendView(items: model.changeLegend)
            }
            .padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Change each month"))
        }
        Section {
            ForEach(model.accounts) { ReportFigureRowView(row: $0) }
        } header: {
            ReportSectionHeader(title: String(localized: "By account"), aside: model.dates)
        }
    }
}

private struct ReportBalanceSheetBody: View {
    var model: ReportBalanceSheetModel

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        side(String(localized: "What you own"), total: model.ownTotal, rows: model.own, empty: String(localized: "No assets."), color: 0)
        side(String(localized: "What you owe"), total: model.oweTotal, rows: model.owe, empty: String(localized: "Nothing owed."), color: 1)
        ReportTableSections(key: "statement", rows: model.statement, header: String(localized: "reports.section.statement", defaultValue: "Statement", comment: "Balance sheet section listing each account."), aside: "\(model.statementAside)\n\(model.dates)", footer: model.help)
    }

    private func side(_ title: String, total: String, rows: [ReportBarRow], empty: String, color: Int) -> some View {
        Section {
            if rows.isEmpty { Text(empty).foregroundStyle(Color.secondary) }
            ForEach(rows) { ReportBarRowView(row: $0, defaultColor: color) }
        } header: {
            HStack(alignment: .firstTextBaseline) {
                ReportSectionHeader(title: title)
                Spacer()
                Text(total).font(.headline).monospacedDigit().foregroundStyle(Color.primary)
            }
            .accessibilityElement(children: .combine)
        }
    }
}

private struct ReportBalancesBody: View {
    var model: ReportBalancesModel
    var actions: ReportActions

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            ReportFlowLayout(spacing: 8, lineSpacing: 8) {
                ForEach(model.chips) { chip in chipView(chip) }
            }
            .padding(.vertical, 4)
            if let chart = model.chart {
                VStack(alignment: .leading, spacing: 10) {
                    ReportLinesChart(model: chart, height: 260)
                    ReportLegendView(items: model.legend)
                }
                .padding(.vertical, 6)
            } else {
                Text("Choose accounts above to chart them.").foregroundStyle(Color.secondary)
            }
        } header: {
            ReportSectionHeader(title: String(localized: "Balances at the end of each month"))
        }
        Section {
            ForEach(model.rows) { ReportFigureRowView(row: $0) }
        } header: {
            ReportSectionHeader(title: String(localized: "All accounts"))
        } footer: {
            Text("Month-end balances; lowest and highest are among those.")
        }
    }

    private func chipView(_ chip: ReportAccountChip) -> some View {
        let on = chip.slot != nil
        return Button { actions.toggleChart(chip.id) } label: {
            HStack(spacing: 7) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(on ? ReportPalette.color(chip.slot) : Color.clear)
                    .overlay(RoundedRectangle(cornerRadius: 2).strokeBorder(on ? Color.clear : Color.secondary))
                    .frame(width: 10, height: 10)
                Text(chip.name).font(.subheadline.weight(.semibold)).lineLimit(1)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .foregroundStyle(chip.enabled ? Color.primary : Color.secondary)
            .background(on ? Color.urAccent.opacity(0.14) : Color.insetBackground, in: Capsule())
        }
        .buttonStyle(.plain)
        .disabled(!chip.enabled)
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityHint(on ? "Takes it off the chart" : "Adds it to the chart")
    }
}

private struct ReportAgeBody: View {
    var model: ReportAgeModel

    var body: some View {
        ReportTilesSection(tiles: model.tiles)
        Section {
            ReportLinesChart(model: model.chart).padding(.vertical, 6)
        } header: {
            ReportSectionHeader(title: String(localized: "Cash age at the end of each month"))
        } footer: {
            Text("How long money sits in your cash accounts before you spend it: the average age of the last ten cash outflows, first in, first out, to the nearest day. Until there are ten, the missing ones count as zero days. It is an estimate when opening balances have no known dates.")
        }
        Section {
            ForEach(model.rows) { ReportFigureRowView(row: $0) }
        } header: {
            ReportSectionHeader(title: String(localized: "By month"))
        }
    }
}

/// The forecast's body: what's known ahead, the line with everyday spending, and what's coming up.
struct ReportForecastBody: View {
    var model: ReportForecastModel?
    var stale: Bool
    var error: String?
    @Binding var ahead: Int
    var retry: () -> Void
    var editEveryday: () -> Void
    var useSuggested: () -> Void

    var body: some View {
        if let model { ReportTilesSection(tiles: model.tiles).disabled(stale).opacity(stale ? 0.55 : 1) }
        Section {
            Picker("Months ahead", selection: $ahead) {
                Text("3 months").tag(3)
                Text("6 months").tag(6)
                Text("12 months").tag(12)
            }
            .pickerStyle(.segmented)
            if let model {
                Group {
                    VStack(alignment: .leading, spacing: 10) {
                        ReportLinesChart(model: model.chart, height: 260)
                        ReportLegendView(items: model.legend)
                    }
                    .padding(.vertical, 6)
                    Button(action: editEveryday) {
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Everyday spending a month").font(.footnote).foregroundStyle(Color.secondary)
                                Text(model.everydayText).font(.headline).monospacedDigit().foregroundStyle(Color.primary)
                            }
                            Spacer()
                            Text("Change").fontWeight(.semibold)
                        }
                    }
                    .accessibilityLabel("Everyday spending a month, \(model.everydayText)")
                    .accessibilityHint("Changes the spending the forecast assumes")
                    if let suggestion = model.suggestion { Button(suggestion, action: useSuggested) }
                }
                .disabled(stale)
                .opacity(stale ? 0.55 : 1)
            } else if let error {
                FormErrorView(message: error)
                Button("Retry forecast", action: retry)
            } else {
                ProgressView().frame(maxWidth: .infinity).padding(.vertical, 24)
            }
        } header: {
            ReportSectionHeader(title: String(localized: "Balance ahead"))
        } footer: {
            if let model { Text(model.help) }
        }
        if let model {
            Section {
                if model.rows.isEmpty {
                    ContentUnavailableView("Nothing coming up", systemImage: "calendar.badge.clock",
                                           description: Text("Nothing is scheduled or entered for later dates. Schedule your income and bills to make this forecast more exact."))
                }
                ForEach(model.rows) { row in
                    HStack(spacing: 10) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(row.title).lineLimit(1)
                            Text(row.detail).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2)
                        }
                        Spacer(minLength: 8)
                        VStack(alignment: .trailing, spacing: 2) {
                            Text(row.amount).fontWeight(.medium).monospacedDigit().foregroundStyle(row.amountPositive ? Color.urPositive : Color.primary)
                            Text(row.balance).font(.footnote).monospacedDigit().foregroundStyle(row.balanceNegative ? Color.urNegative : Color.secondary)
                        }
                        .lineLimit(1)
                    }
                    .accessibilityElement(children: .combine)
                }
            } header: {
                ReportSectionHeader(title: String(localized: "Coming up"), aside: model.count)
            } footer: {
                Text("Balances here count known transactions only.")
            }
            .disabled(stale)
            .opacity(stale ? 0.55 : 1)
        }
    }
}
