import SwiftUI
import Observation
import UniformTypeIdentifiers

/// A view of a report: one of the report bodies, by the id it has always had, with its label on
/// the report's switch, the report it is a view of, what it shows, and whether the Categories
/// filter applies.
struct ReportInfo: Identifiable, Hashable {
    var id: String
    var label: String
    /// The report it is a view of: a `ReportTopic` id.
    var report: String
    var blurb: String
    var categories = false

    /// Every view, each report's in the order of its switch: a report opens on its first.
    static let all: [ReportInfo] = [
        ReportInfo(id: "categories", label: String(localized: "Category"), report: "spending",
                   blurb: String(localized: "Where your money went, by group and category, and how last month compares."), categories: true),
        ReportInfo(id: "payees", label: String(localized: "Payee"), report: "spending",
                   blurb: String(localized: "Who you pay most, and how often."), categories: true),
        ReportInfo(id: "trends", label: String(localized: "Over time"), report: "spending",
                   blurb: String(localized: "Spending month by month, by group or in total."), categories: true),
        ReportInfo(id: "calendar", label: String(localized: "Calendar"), report: "spending",
                   blurb: String(localized: "Spending day by day: busy days, quiet days and weekdays."), categories: true),
        ReportInfo(id: "incomeExpense", label: String(localized: "Vs. spending"), report: "income",
                   blurb: String(localized: "Income, spending and the surplus left in your budget."), categories: true),
        ReportInfo(id: "income", label: String(localized: "Sources"), report: "income",
                   blurb: String(localized: "Where your income comes from, and how steady it is.")),
        ReportInfo(id: "cashflow", label: String(localized: "In and out"), report: "income",
                   blurb: String(localized: "Account movements across your budget boundary, including credit purchases."), categories: true),
        ReportInfo(id: "networth", label: String(localized: "Over time"), report: "networth",
                   blurb: String(localized: "What you own minus what you owe, month by month.")),
        ReportInfo(id: "balances", label: String(localized: "By account"), report: "networth",
                   blurb: String(localized: "Each account's balance at the end of every month.")),
        ReportInfo(id: "balance", label: String(localized: "Balance sheet"), report: "balance",
                   blurb: String(localized: "Assets and liabilities by type, with the ratios that matter.")),
        ReportInfo(id: "performance", label: String(localized: "Performance"), report: ReportTopic.investments,
                   blurb: String(localized: "How your investments did: their value, what you put in, and what they gained.")),
        ReportInfo(id: "allocation", label: String(localized: "Allocation"), report: ReportTopic.investments,
                   blurb: String(localized: "What your investments are in today, across all your accounts.")),
        ReportInfo(id: "investmentIncome", label: String(localized: "Income and costs"), report: ReportTopic.investments,
                   blurb: String(localized: "Dividends and interest earned, and the tax and fees paid.")),
        ReportInfo(id: "forecast", label: String(localized: "Balances"), report: "forecast",
                   blurb: String(localized: "Where your cash and card balances are heading, from what's scheduled.")),
        ReportInfo(id: "age", label: String(localized: "Cash age"), report: "forecast",
                   blurb: String(localized: "How long money waits in your accounts before you spend it.")),
    ]

    static func find(_ id: String) -> ReportInfo { all.first { $0.id == id } ?? all[0] }

    /// Whether any account of the budget has holdings: the Investments report is about them.
    static func hasHoldings(_ budget: JSON?) -> Bool {
        !(budget?.rows(Investments.key).isEmpty ?? true)
    }
}

/// One of the six reports: its name, the question it answers, its symbol, and the views inside
/// it. Investments is listed only once an account has holdings.
struct ReportTopic: Identifiable, Hashable {
    var id: String
    var label: String
    var question: String
    var icon: String

    static let investments = "investments"
    static let all: [ReportTopic] = [
        ReportTopic(id: "spending", label: String(localized: "Spending"), question: String(localized: "Where did my money go?"), icon: "chart.pie"),
        ReportTopic(id: "income", label: String(localized: "Income"), question: String(localized: "Am I spending less than I earn?"),
                    icon: "arrow.left.arrow.right"),
        ReportTopic(id: "networth", label: String(localized: "Net worth"), question: String(localized: "What am I worth, and why did it change?"),
                    icon: "chart.line.uptrend.xyaxis"),
        ReportTopic(id: "balance", label: String(localized: "Balance sheet"), question: String(localized: "What do I own and owe?"),
                    icon: "building.columns"),
        ReportTopic(id: investments, label: String(localized: "Investments"), question: String(localized: "How are my investments doing?"), icon: "percent"),
        ReportTopic(id: "forecast", label: String(localized: "Forecast"), question: String(localized: "What's coming up?"),
                    icon: "chart.line.flattrend.xyaxis"),
    ]

    static func find(_ id: String) -> ReportTopic { all.first { $0.id == id } ?? all[0] }

    /// The reports to list: Investments only when there are holdings.
    static func shown(holdings: Bool) -> [ReportTopic] {
        holdings ? all : all.filter { $0.id != investments }
    }

    /// Its views, in the order of its switch.
    var views: [ReportInfo] { ReportInfo.all.filter { $0.report == id } }

    /// Spending and Income leave out money moved into or out of investment accounts, and say so under their filter chip.
    var leavesOutInvested: Bool { id == "spending" || id == "income" }

    /// The view a report opens on: the one last used in it this session, else its first.
    static func view(of report: String, remembered: [String: String] = [:]) -> String {
        let views = find(report).views
        if let last = remembered[report], views.contains(where: { $0.id == last }) { return last }
        return views.first?.id ?? ReportInfo.all[0].id
    }

    /// Where an id leads. There were once fifteen reports, each called by what is now a view's
    /// id: such an id is still that view, in its report. A report's own id is the view it opens on.
    static func resolve(_ id: String, remembered: [String: String] = [:]) -> String {
        if ReportInfo.all.contains(where: { $0.id == id }) { return id }
        return view(of: id, remembered: remembered)
    }
}

/// Where the reports' navigation goes: a report at one of its views, by the view's id (or one
/// group of Spending by category), the transactions behind a figure, or an account's Holdings page.
enum ReportNav: Hashable {
    case report(String, group: String?)
    case drill(ReportDrillRequest)
    case holdings(String)
}

/// An account's Holdings page, opened from the Performance report.
private struct ReportHoldingsPage: View {
    @Environment(BudgetStore.self) private var store
    var accountId: String

    var body: some View {
        if let account = store.budget?.row("accounts", id: accountId) {
            HoldingsPageView(account: account)
        } else {
            Color.groupedBackground.ignoresSafeArea()
        }
    }
}

/// The transactions behind a report figure, in the range and filters of the report it came from.
struct ReportDrillRequest: Hashable {
    var from: String
    var to: String
    var accounts: [String]
    var categories: [String]
    var target: ReportDrillTarget

    var params: JSON {
        ["from": .string(from), "to": .string(to), "accountIds": JSON(accounts), "categoryIds": JSON(categories),
         "dimension": .string(target.dimension), "key": .string(target.key)]
    }
}

/// The range and filters every report shares, as Android keeps them across its reports.
@Observable
final class ReportFilters {
    /// Nil until someone picks a range: the last twelve months ending with the budget's month.
    var range: ReportRange?
    var accounts: Set<String> = []
    var categories: Set<String> = []
    /// The view last used in each report this session, by the report's id.
    var views: [String: String] = [:]
}

/// Reads a budget for reports once per snapshot, from whichever task asks first.
final class ReportBudgetCache: @unchecked Sendable {
    private let lock = NSLock()
    private var last: (version: Int, value: ReportBudget)?

    func get(version: Int, budget: JSON?) -> ReportBudget {
        if let hit = lock.withLock({ last }), hit.version == version { return hit.value }
        let value = ReportMath.parseBudget(budget ?? [:], today: Dates.today)
        lock.withLock { last = (version, value) }
        return value
    }
}

/// Reports: an overview of six cards, one for each report (Spending, Income, Net worth, Balance
/// sheet, Investments once an account has holdings, Forecast), each with its question, a figure
/// and a small chart over six months. A card opens its report, which has a switch between its
/// views, one filter chip, charts and tables worked out by the shared engine, and opens the
/// transactions behind a figure.
struct ReportsView: View {
    @Environment(BudgetStore.self) private var store
    @State private var path: [ReportNav] = []
    @State private var filters = ReportFilters()
    @State private var cache = ReportBudgetCache()
    @State private var span = WindowSpan()
    @Environment(\.fold) private var fold

    init() {}

    /// On a wide window the list of the six reports stays at the side and the Overview, or the
    /// report chosen, is beside it; Back from a report returns to the Overview.
    var body: some View {
        // By the window's width, or by a fold: the list ends where it runs and the report takes the far side.
        let split = WideLayout.split(.reports, span: span, fold: fold)
        let wide = split != nil
        HStack(spacing: 0) {
            if let split {
                NavigationStack {
                    ReportsIndex(open: shown) { id in
                        path = id.map { [.report(ReportTopic.view(of: $0, remembered: filters.views), group: nil)] } ?? []
                    }
                        .startPaneContent()
                        .navigationTitle("Reports")
                        .barTitle()
                }
                .environment(\.startPane, true)
                // On the start side of a fold the list keeps clear of the dock the tabs stand in.
                .contentMargins(.bottom, split.hinge && fold.docksTabs ? FoldDock.room : 0, for: .scrollContent)
                .frame(width: split.list)
                if split.hinge { FoldGap(width: split.gap) } else { Divider().ignoresSafeArea() }
            }
            NavigationStack(path: $path) {
                ReportsOverview(cache: cache, filters: filters)
                    .navigationTitle(wide ? Text("Overview") : Text("Reports"))
                    .barTitle()
                    .toolbar {
                        ToolbarItem(placement: .trailingBar) { AssistantBarButton(page: "reports") }
                        ToolbarItem(placement: .trailingBar) { BudgetActionsMenu() }
                    }
                    .navigationDestination(for: ReportNav.self) { nav in
                        switch nav {
                        case .report(let id, let group):
                            // Beside the list, another report takes this one's place, so the list's mark follows.
                            ReportPage(id: id, group: group, filters: filters, cache: cache,
                                       switchTo: wide && group == nil ? { path = [.report($0, group: nil)] } : nil) { path.append($0) }
                                .id(nav)
                        case .drill(let request):
                            ReportDrillView(request: request, cache: cache)
                        case .holdings(let account):
                            ReportHoldingsPage(accountId: account)
                        }
                    }
            }
        }
        .measuringSpan($span)
        .onAppear {
            // Debug builds can open a report at launch (`-open report:categories`), for screenshots.
            guard let route = LaunchRoute.open, route.hasPrefix("report:") else { return }
            LaunchRoute.open = nil
            path = [.report(String(route.dropFirst(7)), group: nil)]
        }
        .belowTopTabs(wide && split?.hinge != true)
    }

    /// The report the pane is on (whatever was opened from it since), or nil on the Overview.
    private var shown: String? {
        if case .report(let id, _)? = path.first { return ReportInfo.find(ReportTopic.resolve(id)).report }
        return nil
    }
}

/// The reports' list on a wide window: the Overview, then the six reports, the open one marked.
private struct ReportsIndex: View {
    @Environment(BudgetStore.self) private var store
    var open: String?
    var choose: (String?) -> Void

    var body: some View {
        AppList {
            Section { row(nil, String(localized: "Overview"), "square.grid.2x2") }
            Section {
                ForEach(ReportTopic.shown(holdings: ReportInfo.hasHoldings(store.budget))) { report in row(report.id, report.label, report.icon) }
            }
        }
        .groupedList()
    }

    private func row(_ id: String?, _ label: String, _ icon: String) -> some View {
        let selected = id == open
        return Button { choose(id) } label: {
            Label {
                Text(label).foregroundStyle(selected ? Color.urAccent : Color.primary).fontWeight(selected ? .semibold : .regular)
            } icon: {
                Image(systemName: icon).foregroundStyle(Color.urAccent)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .listRowBackground(selected ? Color.urAccent.opacity(0.12) : Color.cardBackground)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// One report pushed from another tab, as Accounts' net worth opens Net worth, with its own filters.
struct StandaloneReport: View {
    var id: String
    @State private var filters = ReportFilters()
    @State private var cache = ReportBudgetCache()

    var body: some View {
        PushedReport(nav: .report(id, group: nil), filters: filters, cache: cache)
    }
}

/// A report or drill-down on a stack with no path of its own; what it opens is pushed after it.
private struct PushedReport: View {
    var nav: ReportNav
    var filters: ReportFilters
    var cache: ReportBudgetCache
    @State private var next: ReportNav?

    var body: some View {
        Group {
            switch nav {
            case .report(let id, let group):
                ReportPage(id: id, group: group, filters: filters, cache: cache) { next = $0 }
            case .drill(let request):
                ReportDrillView(request: request, cache: cache)
            case .holdings(let account):
                ReportHoldingsPage(accountId: account)
            }
        }
        .navigationDestination(item: $next) { PushedReport(nav: $0, filters: filters, cache: cache) }
    }
}

// MARK: Overview

private struct ReportOverviewKey: Hashable {
    var version: Int
    var month: String
    var attempt: Int
}

/// The Reports tab's first page: the month the cards end at, then a card for each report with its
/// question, its headline figure and a small chart. A card's heading opens its report at the view
/// it was last on.
private struct ReportsOverview: View {
    @Environment(BudgetStore.self) private var store
    var cache: ReportBudgetCache
    /// The view each report was last on this session.
    var filters: ReportFilters
    @State private var attempt = 0
    @State private var result: ReportScoped<ReportOverviewKey, ReportOverviewModel?>?
    @State private var failure: ReportScoped<ReportOverviewKey, String>?
    @State private var pickingMonth = false
    /// The month under a finger on each card's small chart, by the card's report.
    @State private var picked: [String: Int] = [:]

    var body: some View {
        let key = ReportOverviewKey(version: store.version, month: store.month, attempt: attempt)
        let failed = failure?.forScope(key)
        let current = result?.scope == key
        AppList {
            monthSection
            if let failed {
                Section {
                    EmptyState(title: String(localized: "Reports unavailable"), detail: failed, symbol: "exclamationmark.triangle",
                               action: String(localized: "Retry")) { attempt += 1 }
                }
            } else if let result {
                if let model = result.value {
                    Group {
                        ForEach(model.cards) { card in
                            Section { cardRows(card) }
                        }
                    }
                    .disabled(!current)
                    .opacity(current ? 1 : 0.55)
                }
            } else {
                Section { ProgressView().frame(maxWidth: .infinity).padding(.vertical, 32) }
            }
        }
        .groupedList()
        .tightTop()
        .environment(\.defaultMinListRowHeight, 30)
        .toolbar {
            if failed == nil && result != nil && !current {
                ToolbarItem(placement: .leadingBar) { ProgressView().accessibilityLabel("Updating reports") }
            }
        }
        .task(id: key) { await load(key) }
        .sheet(isPresented: $pickingMonth) {
            MonthPickerSheet(month: store.month) { month in
                store.selectMonth(month)
                pickingMonth = false
            }
        }
    }

    private func load(_ key: ReportOverviewKey) async {
        let budget = store.budget
        let version = store.version
        guard budget != nil, version == key.version, result?.scope != key else { return }
        let from = Dates.addMonths(key.month, -5)
        let today = Dates.today
        let filter: JSON = ["from": .string(from), "to": .string(key.month), "accountIds": [], "categoryIds": []]
        do {
            let json = try await store.query("reports", filter)
            // The holdings' figures for the same months, and the forecast the Forecast report reads:
            // a card whose figures can't be had is left out.
            var portfolio: JSON?
            if ReportInfo.hasHoldings(budget) { portfolio = await investments(from: from, to: key.month) }
            let held = portfolio
            let ahead = await forecast(from: today)
            let cache = cache
            let model = await Task.detached(priority: .userInitiated) { () -> ReportOverviewModel? in
                let ctx = cache.get(version: version, budget: budget)
                return ReportMath.overviewModel(ReportMath.parseReport(json), ctx, portfolio: held.map { ReportMath.parsePortfolio($0) },
                                                forecast: ahead.map { ReportMath.parseForecast($0) }, today: today)
            }.value
            guard !Task.isCancelled else { return }
            result = ReportScoped(scope: key, value: model)
            // New figures start with no month picked.
            picked = [:]
        } catch {
            guard !Task.isCancelled else { return }
            failure = ReportScoped(scope: key, value: BudgetStore.readable(error))
        }
    }

    /// The engine's investments report for the cards' months, or nil when it can't be had.
    private func investments(from: String, to: String) async -> JSON? {
        try? await store.query("investmentsReport", ["from": .string(from), "to": .string(to), "accountIds": []])
    }

    /// The budget accounts' forecast for the six months ahead, as the Forecast report reads it, or nil when it can't be had.
    private func forecast(from today: String) async -> JSON? {
        try? await store.query("forecast", ["today": .string(today), "through": .string(ReportMath.addMonths(day: today, ReportMath.overviewMonthsAhead)),
                                            "accountIds": []])
    }

    private var monthSection: some View {
        // The month the cards end at, chosen as on Android: its name opens the months, and the
        // arrows step one at a time. It sits on the page, not in a card of its own.
        Section {
            HStack(spacing: 4) {
                Button { pickingMonth = true } label: {
                    HStack(spacing: 5) {
                        Text(Dates.shortMonthLabel(store.month)).font(.title3.bold())
                        Image(systemName: "chevron.down").font(.footnote.weight(.bold))
                    }
                    .foregroundStyle(Color.primary)
                }
                .accessibilityHint("Chooses another month")
                Spacer()
                Button { store.selectMonth(Dates.addMonths(store.month, -1)) } label: {
                    Image(systemName: "chevron.backward").font(.body.weight(.semibold)).frame(width: 36, height: 32)
                }
                .accessibilityLabel("Previous month")
                Button { store.selectMonth(Dates.addMonths(store.month, 1)) } label: {
                    Image(systemName: "chevron.forward").font(.body.weight(.semibold)).frame(width: 36, height: 32)
                }
                .accessibilityLabel("Next month")
            }
            .buttonStyle(.borderless)
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets(top: 0, leading: 6, bottom: 0, trailing: 0))
        }
    }

    /// A month picked on a small chart of two series: its name, then each series' figure.
    private func pickedMonth(_ bars: ReportMiniBarsModel, _ index: Int) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            monthCaption(bars, index)
            HStack(spacing: 16) {
                ForEach(bars.series, id: \.label) { series in
                    HStack(spacing: 6) {
                        ReportLegendKey(color: ReportPalette.color(series.color))
                        Text(index < series.texts.count ? series.texts[index] : "").font(.headline).monospacedDigit().lineLimit(1)
                    }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(series.label)
                    .accessibilityValue(index < series.texts.count ? series.texts[index] : "")
                }
            }
        }
    }

    private func monthCaption(_ bars: ReportMiniBarsModel, _ index: Int) -> some View {
        Text(index < bars.titles.count ? bars.titles[index] : "").font(.subheadline).foregroundStyle(Color.secondary)
    }

    private func bigFigure(_ text: String, negative: Bool) -> some View {
        Text(text).font(.title.weight(.heavy)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.5)
            .foregroundStyle(negative ? Color.urNegative : Color.primary)
    }

    /// A card: the report's name and question, which open the report, then its headline and small chart.
    @ViewBuilder private func cardRows(_ card: ReportOverviewCard) -> some View {
        let report = ReportTopic.find(card.id)
        let at = picked[card.id] ?? -1
        NavigationLink(value: ReportNav.report(ReportTopic.view(of: card.id, remembered: filters.views), group: nil)) {
            VStack(alignment: .leading, spacing: 3) {
                Label(report.label, systemImage: report.icon).font(.headline).foregroundStyle(Color.urAccent)
                Text(report.question).font(.subheadline).foregroundStyle(Color.secondary)
            }
        }
        .accessibilityHint("Opens the full report")
        .listRowSeparator(.hidden)
        VStack(alignment: .leading, spacing: 8) {
            headline(card, at: at)
            smallChart(card, at: at)
        }
        .padding(.vertical, 4)
    }

    /// The card's figure and its note; a month picked on its small chart takes their place.
    @ViewBuilder private func headline(_ card: ReportOverviewCard, at: Int) -> some View {
        if let bars = card.bars, bars.labels.indices.contains(at) {
            if bars.series.count > 1 {
                pickedMonth(bars, at)
            } else if let series = bars.series.first, series.values.indices.contains(at), series.texts.indices.contains(at) {
                bigFigure(series.texts[at], negative: series.values[at] < 0)
                monthCaption(bars, at)
            }
        } else {
            bigFigure(card.figure, negative: card.negative)
            if let note = card.note {
                Text(note).font(.subheadline)
                    .foregroundStyle(card.noteTone == .none ? Color.secondary : HoldingsPage.color(card.noteTone))
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    /// The card's small chart: columns by month, one bar split into its parts, or a line ahead.
    @ViewBuilder private func smallChart(_ card: ReportOverviewCard, at: Int) -> some View {
        if let bars = card.bars {
            ReportMiniBars(model: bars, selected: at) { picked[card.id] = $0 }
            if bars.series.count > 1 {
                ReportLegendView(items: bars.series.map { ReportLegendItem(label: $0.label, color: $0.color) })
            }
        } else if let parts = card.parts {
            ReportSegmentBar(rows: parts)
            ReportLegendView(items: parts.map { ReportLegendItem(label: "\($0.label) \($0.valueText)", color: $0.color) })
        } else if let line = card.line {
            ReportLinesChart(model: line, height: 130, readout: false)
        }
    }
}

// MARK: A report

private struct ReportKey: Hashable, Sendable {
    var version: Int
    var from: String
    var to: String
    var accounts: [String]
    var categories: [String]
    var attempt: Int

    var filter: JSON { ["from": .string(from), "to": .string(to), "accountIds": JSON(accounts), "categoryIds": JSON(categories)] }
    /// The same months and accounts, as the engine's investments report takes them.
    var portfolioFilter: JSON { ["from": .string(from), "to": .string(to), "accountIds": JSON(accounts)] }
}

private struct ReportViewKey: Hashable {
    var data: ReportKey
    var id: String
    var options: ReportOptions
    var scope: ReportScope
}

private struct ReportForecastKey: Hashable {
    var version: Int
    var today: String
    var months: Int
    var accounts: [String]
    var attempt: Int
}

private struct ReportForecastViewKey: Hashable {
    var data: ReportForecastKey
    var everyday: Int64?
}

private struct ReportTaskKey<Key: Hashable>: Hashable {
    var key: Key
    var active: Bool
}

/// A filter as a pill that opens its choices.
private struct ReportChip: View {
    var title: String
    var text: String
    var icon: String
    var active: Bool

    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: icon).imageScale(.small)
            Text(text).lineLimit(1)
            Image(systemName: "chevron.down").imageScale(.small).font(.caption.weight(.bold))
        }
        .font(.subheadline.weight(.semibold))
        .foregroundStyle(active ? Color.urAccent : Color.primary)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(active ? Color.urAccent.opacity(0.14) : Color.cardBackground, in: Capsule())
        .contentShape(Capsule())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(title): \(text)")
    }
}

/// The page title opens the other reports.
private struct ReportTitleMenu: ViewModifier {
    var enabled: Bool
    /// The report the page is on.
    var current: String
    /// Whether an account has holdings: Investments is listed only then.
    var holdings: Bool
    var choose: (String) -> Void

    func body(content: Content) -> some View {
        if enabled {
            content.toolbarTitleMenu {
                ForEach(ReportTopic.shown(holdings: holdings)) { report in
                    Button { choose(report.id) } label: {
                        Label(report.label, systemImage: report.id == current ? "checkmark" : report.icon)
                    }
                }
            }
        } else {
            content
        }
    }
}

private struct ReportSearch: ViewModifier {
    var enabled: Bool
    @Binding var text: String

    func body(content: Content) -> some View {
        if enabled {
            content.searchable(text: $text, prompt: "Find a payee")
        } else {
            content
        }
    }
}

/// One report, at one of its views: the switch between its views, its filters behind one chip,
/// its figures for exactly those filters and this snapshot, and the way to the transactions
/// behind them. While new figures load, the last ones stay on screen, dimmed.
/// The Investments reports read the engine's investments report in place of the reports' figures,
/// as the forecast reads its own; the balance sheet reads both when the budget has holdings.
private struct ReportPage: View {
    @Environment(BudgetStore.self) private var store
    var group: String?
    var filters: ReportFilters
    var cache: ReportBudgetCache
    /// Beside the reports' list: the title menu's choice of another report replaces this page (by
    /// the view that report opens on), where on a phone it changes in place.
    var switchTo: ((String) -> Void)?
    var navigate: (ReportNav) -> Void

    /// The view shown: the id of one of the report bodies.
    @State private var id: String
    @State private var attempt = 0
    @State private var context: ReportBudget?
    @State private var data: ReportScoped<ReportKey, ReportData>?
    @State private var failure: ReportScoped<ReportKey, String>?
    @State private var shown: ReportScoped<ReportViewKey, ReportBody?>?
    @State private var byGroup = true
    @State private var chartAccounts: [String: Int]?
    @State private var payeeQuery = ""
    @State private var ahead = 6
    @State private var everyday: Int64?
    @State private var forecastData: ReportScoped<ReportForecastKey, ReportForecastData>?
    @State private var forecastFailure: ReportScoped<ReportForecastKey, String>?
    @State private var forecastShown: ReportScoped<ReportForecastViewKey, ReportForecastModel>?
    @State private var portfolio: ReportScoped<ReportKey, Portfolio>?
    @State private var portfolioFailure: ReportScoped<ReportKey, String>?
    @State private var choosingFilters = false
    /// Export CSV was chosen in the filters' sheet: the file is asked for once the sheet has closed.
    @State private var exportAsked = false
    @State private var editingEveryday = false
    @State private var exportFile: TextFile?
    @State private var exportName = "urbudget-report.csv"
    @State private var exporting = false
    @State private var exportError: String?

    init(id: String, group: String?, filters: ReportFilters, cache: ReportBudgetCache, switchTo: ((String) -> Void)? = nil,
         navigate: @escaping (ReportNav) -> Void) {
        _id = State(initialValue: ReportTopic.resolve(id))
        self.group = group
        self.filters = filters
        self.cache = cache
        self.switchTo = switchTo
        self.navigate = navigate
    }

    private var info: ReportInfo { ReportInfo.find(id) }
    /// The report this view belongs to.
    private var topic: ReportTopic { ReportTopic.find(info.report) }
    private var isForecast: Bool { id == "forecast" }
    private var isInvestments: Bool { info.report == ReportTopic.investments }
    /// Allocation is today's holdings and the forecast runs from today: neither takes a range of months.
    private var takesRange: Bool { !isForecast && id != "allocation" }
    private var hasHoldings: Bool { ReportInfo.hasHoldings(store.budget) }
    /// The Investments reports are made of the engine's investments report, and the balance sheet
    /// groups what is owned with it.
    private var wantsPortfolio: Bool { isInvestments || (id == "balance" && hasHoldings) }
    private var range: ReportRange { filters.range ?? ReportMath.defaultRange(month: store.month, today: Dates.currentMonth) }
    private var options: ReportOptions { ReportOptions(group: group, byGroup: byGroup, chartAccounts: chartAccounts) }

    /// Reports that don't split by category ignore the Categories filter.
    private func currentKey() -> ReportKey {
        let thisMonth = Dates.currentMonth
        let range = range
        return ReportKey(version: store.version, from: range.from.isEmpty ? (context?.earliestMonth ?? thisMonth) : range.from,
                         to: range.to.isEmpty ? thisMonth : range.to, accounts: filters.accounts.sorted(),
                         categories: info.categories ? filters.categories.sorted() : [], attempt: attempt)
    }

    private func scope(_ key: ReportKey) -> ReportScope {
        ReportScope(from: key.from, to: key.to, accounts: Set(key.accounts), someCategories: !key.categories.isEmpty, today: Dates.today,
                    weekStart: ReportMath.localeWeekStart())
    }

    private var forecastKey: ReportForecastKey {
        ReportForecastKey(version: store.version, today: Dates.today, months: ahead, accounts: filters.accounts.sorted(), attempt: attempt)
    }

    /// The accounts to choose from: only those with holdings for the Investments reports.
    private var accountGroups: [ReportFilterGroup] {
        guard let context else { return [] }
        return isInvestments ? ReportMath.holdingsFilterGroups(context) : ReportMath.accountFilterGroups(context, budgetOnly: isForecast)
    }

    /// Why this report's figures couldn't be had, for exactly these filters.
    private func problem(_ key: ReportKey) -> String? {
        isInvestments ? portfolioFailure?.forScope(key) : failure?.forScope(key)
    }

    /// Whether everything this report is built from has arrived for these filters. The balance
    /// sheet goes ahead without the investments report when that can't be had.
    private func figuresReady(_ key: ReportKey) -> Bool {
        let held = portfolio?.scope == key
        if isInvestments { return held }
        if wantsPortfolio { return data?.scope == key && (held || portfolioFailure?.scope == key) }
        return data?.scope == key
    }
    private var categoryGroups: [ReportFilterGroup] { context.map(ReportMath.categoryFilterGroups) ?? [] }

    /// The accounts in force, in words: all of them, one's name, or how many.
    private var accountLabel: String {
        ReportMath.filterSummary(accountGroups, filters.accounts,
                                 allLabel: isForecast ? String(localized: "All budget accounts") : String(localized: "All accounts"),
                                 several: { String(localized: "\($0) accounts", locale: .app) })
    }

    private var categoryLabel: String {
        ReportMath.filterSummary(categoryGroups, filters.categories, allLabel: String(localized: "All categories"),
                                 several: { String(localized: "\($0) categories", locale: .app) })
    }

    /// Another report, from the title menu: at the view it was last on.
    private func open(_ report: String) {
        let view = ReportTopic.view(of: report, remembered: filters.views)
        if let switchTo { switchTo(view) } else { id = view }
    }

    private func exportIfAsked() {
        guard exportAsked else { return }
        exportAsked = false
        export(currentKey())
    }

    var body: some View {
        let key = currentKey()
        let viewKey = ReportViewKey(data: key, id: id, options: options, scope: scope(key))
        let forecastViewKey = ReportForecastViewKey(data: forecastKey, everyday: everyday)
        ScrollViewReader { proxy in
            content(key: key, viewKey: viewKey, proxy: proxy)
        }
        .navigationTitle(group ?? topic.label)
        .inlineTitle()
        .modifier(ReportTitleMenu(enabled: group == nil, current: topic.id, holdings: hasHoldings) { next in open(next) })
        .toolbar { toolbar(key: key, viewKey: viewKey) }
        .task(id: store.version) { await loadContext() }
        .task(id: ReportTaskKey(key: key, active: !isForecast && !isInvestments)) { await loadData(key) }
        .task(id: ReportTaskKey(key: key, active: wantsPortfolio)) { await loadPortfolio(key) }
        .task(id: ReportTaskKey(key: viewKey, active: figuresReady(key))) { await buildView(viewKey) }
        .task(id: ReportTaskKey(key: forecastKey, active: isForecast)) { await loadForecast(forecastKey) }
        .task(id: ReportTaskKey(key: forecastViewKey, active: isForecast && forecastData?.scope == forecastKey)) { await buildForecast(forecastViewKey) }
        .onChange(of: id) {
            // A view's own choices start afresh when another opens, as the desktop's do.
            byGroup = true
            chartAccounts = nil
            payeeQuery = ""
            ahead = 6
            everyday = nil
            exportError = nil
            // The report opens on this view the next time, this session.
            if group == nil { filters.views[info.report] = id }
        }
        .onAppear { if group == nil { filters.views[info.report] = id } }
        .sheet(isPresented: $choosingFilters, onDismiss: exportIfAsked) {
            ReportFiltersSheet(filters: filters, range: takesRange ? range : nil, earliest: context?.earliestMonth ?? Dates.currentMonth,
                               accountGroups: accountGroups, accountLabel: accountLabel, categoryGroups: info.categories ? categoryGroups : nil,
                               categoryLabel: categoryLabel, export: isForecast ? nil : { exportAsked = true })
        }
        .sheet(isPresented: $editingEveryday) {
            ReportEverydaySheet(current: forecastShown?.value.everyday ?? 0) { everyday = $0 }
        }
        .fileExporter(isPresented: $exporting, document: exportFile, contentType: .commaSeparatedText, defaultFilename: exportName) { result in
            if case .failure(let error) = result { exportError = error.localizedDescription }
        }
    }

    @ViewBuilder private func content(key: ReportKey, viewKey: ReportViewKey, proxy: ScrollViewProxy) -> some View {
        let report: ReportBody? = (shown?.value ?? nil).flatMap { $0.id == id ? $0 : nil }
        let stale = report != nil && shown?.scope.data != key
        let failed = problem(key)
        AppList {
            filterSection
            notice(key)
            if let failed, !isForecast {
                Section {
                    EmptyState(title: String(localized: "Report unavailable"), detail: failed, symbol: "exclamationmark.triangle",
                               action: String(localized: "Retry report")) { attempt += 1 }
                }
            }
            if let exportError {
                Section { FormErrorView(message: exportError) }
            }
            if isForecast {
                forecastContent
            } else if failed == nil {
                if let report {
                    ReportBodyView(report: report, actions: actions(key, proxy: proxy), byGroup: $byGroup, payees: payees(report), payeeQuery: payeeQuery)
                        .disabled(stale)
                        .opacity(stale ? 0.55 : 1)
                } else if shown?.scope == viewKey {
                    Section {
                        ContentUnavailableView("No figures for these months", systemImage: "chart.bar.xaxis",
                                               description: Text("Choose another range or clear the filters."))
                    }
                } else {
                    Section { ProgressView().frame(maxWidth: .infinity).padding(.vertical, 32) }
                }
            }
        }
        .groupedList()
        .modifier(ReportSearch(enabled: id == "payees", text: $payeeQuery))
    }

    @ToolbarContentBuilder private func toolbar(key: ReportKey, viewKey: ReportViewKey) -> some ToolbarContent {
        let ready = shown?.scope.data == key && shown?.scope.id == id
        let forecastReady = forecastShown?.scope.data == forecastKey
        let loading = isForecast ? forecastFailure?.forScope(forecastKey) == nil && !forecastReady : problem(key) == nil && !ready
        ToolbarItemGroup(placement: .trailingBar) {
            if loading { ProgressView().accessibilityLabel("Calculating") }
            BudgetActionsMenu()
        }
    }

    /// The switch between the report's views, then the one filter chip, which says what is in
    /// force and opens the filters and the export.
    private var filterSection: some View {
        let views = topic.views
        let narrowed = info.categories && !filters.categories.isEmpty
        let label = ReportMath.filterLabel(range: takesRange ? ReportMath.describeRange(range) : nil, accounts: accountLabel,
                                           categories: narrowed ? categoryLabel : nil)
        // What moved into or out of investments in these exact filters' figures, which Spending and Income leave out.
        let invested: Int64 = topic.leavesOutInvested ? (data?.forScope(currentKey())?.invested ?? 0) : 0
        let investedNote = ReportMath.investedNote(invested, currency: store.currency)
        return Section {
            if group == nil && views.count > 1 {
                Picker("View", selection: $id) {
                    ForEach(views) { view in Text(view.label).tag(view.id) }
                }
                .pickerStyle(.segmented)
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            }
            Button { choosingFilters = true } label: {
                ReportChip(title: String(localized: "Filters"), text: label, icon: "line.3.horizontal.decrease.circle",
                           active: !filters.accounts.isEmpty || narrowed)
            }
            .buttonStyle(.plain)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, 2)
            .listRowInsets(EdgeInsets())
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
            if let investedNote {
                Text(investedNote)
                    .font(.footnote)
                    .foregroundStyle(Color.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 2, trailing: 4))
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
        } footer: {
            Text(info.blurb)
        }
    }

    /// "12.00 of spending still needs a category…", for the notice above a report.
    private func uncategorizedText(_ amount: Int64) -> String {
        let text = money(amount, store.currency)
        return String(localized: "\(text) of spending still needs a category. It is included in these totals.")
    }

    @ViewBuilder private func notice(_ key: ReportKey) -> some View {
        let amount = info.categories ? (data?.forScope(key)?.categories[ReportMath.uncategorized] ?? 0) : 0
        if amount != 0 {
            Section {
                Notice(text: uncategorizedText(amount), icon: "exclamationmark.triangle.fill", action: String(localized: "Review")) {
                    navigate(.drill(drillRequest(key, ReportDrillTarget(title: ReportMath.needsCategory, dimension: "category", key: ReportMath.uncategorized))))
                }
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
            }
        }
    }

    private var forecastContent: some View {
        let key = forecastKey
        let failed = forecastFailure?.forScope(key)
        let stale = forecastShown.map { $0.scope.data != key } ?? false
        return ReportForecastBody(model: failed == nil ? forecastShown?.value : nil, stale: stale, error: failed, ahead: $ahead,
                                  retry: { attempt += 1 }, editEveryday: { editingEveryday = true }, useSuggested: { everyday = nil })
    }

    private func drillRequest(_ key: ReportKey, _ target: ReportDrillTarget) -> ReportDrillRequest {
        ReportDrillRequest(from: key.from, to: key.to, accounts: key.accounts, categories: key.categories, target: target)
    }

    private func actions(_ key: ReportKey, proxy: ScrollViewProxy) -> ReportActions {
        ReportActions(
            drill: { .drill(drillRequest(key, $0)) },
            navigate: navigate,
            toggleChart: { toggleChart($0, key: key) },
            scrollTo: { target in withAnimation { proxy.scrollTo(target, anchor: .top) } })
    }

    private func payees(_ report: ReportBody) -> [ReportPayeeRow] {
        guard case .payees(let model) = report else { return [] }
        let query = payeeQuery.trimmingCharacters(in: .whitespaces)
        return query.isEmpty ? model.rows : model.rows.filter { $0.label.localizedCaseInsensitiveContains(query) }
    }

    private func toggleChart(_ account: String, key: ReportKey) {
        guard case .balances(let model)? = shown?.value ?? nil, let figures = data?.forScope(key) else { return }
        let ctx = cache.get(version: key.version, budget: store.budget)
        chartAccounts = ReportMath.toggleChartAccount(model.chosen, eligible: ReportMath.balanceAccounts(figures, ctx, accounts: Set(key.accounts)), id: account)
    }

    /// The engine writes the CSV for these exact filters, then the system asks where to save it.
    /// The Investments reports have a CSV of their own.
    private func export(_ key: ReportKey) {
        exportError = nil
        let investments = isInvestments
        let method = investments ? "investmentsCsv" : "reportCsv"
        let params = investments ? key.portfolioFilter : key.filter
        Task {
            do {
                let csv = try await store.query(method, params)
                guard key == currentKey() else { return }
                exportFile = TextFile(data: Data(csv.string.utf8))
                exportName = investments ? "urbudget-investments-\(key.from)-\(key.to).csv" : "urbudget-report-\(key.from)-\(key.to).csv"
                exporting = true
            } catch {
                if key == currentKey() { exportError = BudgetStore.readable(error) }
            }
        }
    }

    private func loadContext() async {
        let version = store.version
        let budget = store.budget
        let cache = cache
        let value = await Task.detached(priority: .userInitiated) { cache.get(version: version, budget: budget) }.value
        if version == store.version { context = value }
    }

    /// A report belongs to its exact snapshot and filters; new labels must never describe old figures.
    private func loadData(_ key: ReportKey) async {
        guard !isForecast, !isInvestments, data?.scope != key, key.version == store.version, store.budget != nil else { return }
        // Filters ticked in quick succession ask the engine once, for the last of them.
        if data != nil {
            try? await Task.sleep(for: .milliseconds(150))
            guard !Task.isCancelled else { return }
        }
        do {
            let json = try await store.query("reports", key.filter)
            let parsed = await Task.detached(priority: .userInitiated) { ReportMath.parseReport(json) }.value
            guard !Task.isCancelled else { return }
            data = ReportScoped(scope: key, value: parsed)
        } catch {
            guard !Task.isCancelled else { return }
            failure = ReportScoped(scope: key, value: BudgetStore.readable(error))
        }
    }

    /// The engine's investments report for these exact filters, as the forecast has its own figures.
    private func loadPortfolio(_ key: ReportKey) async {
        guard wantsPortfolio, portfolio?.scope != key, key.version == store.version, store.budget != nil else { return }
        // Filters ticked in quick succession ask the engine once, for the last of them.
        if portfolio != nil {
            try? await Task.sleep(for: .milliseconds(150))
            guard !Task.isCancelled else { return }
        }
        do {
            let json = try await store.query("investmentsReport", key.portfolioFilter)
            let parsed = await Task.detached(priority: .userInitiated) { ReportMath.parsePortfolio(json) }.value
            guard !Task.isCancelled else { return }
            portfolio = ReportScoped(scope: key, value: parsed)
        } catch {
            guard !Task.isCancelled else { return }
            portfolioFailure = ReportScoped(scope: key, value: BudgetStore.readable(error))
        }
    }

    private func buildView(_ viewKey: ReportViewKey) async {
        guard shown?.scope != viewKey, viewKey.data.version == store.version else { return }
        let budget = store.budget
        let cache = cache
        let held = portfolio?.forScope(viewKey.data)
        // An Investments report is made of the investments report alone.
        if ReportInfo.find(viewKey.id).report == ReportTopic.investments {
            guard let held else { return }
            let built = await Task.detached(priority: .userInitiated) {
                ReportMath.investmentsView(viewKey.id, held, cache.get(version: viewKey.data.version, budget: budget), today: viewKey.scope.today)
            }.value
            guard !Task.isCancelled else { return }
            shown = ReportScoped(scope: viewKey, value: built)
            return
        }
        guard let figures = data?.forScope(viewKey.data) else { return }
        let built = await Task.detached(priority: .userInitiated) {
            ReportMath.reportView(viewKey.id, figures, cache.get(version: viewKey.data.version, budget: budget), viewKey.scope, viewKey.options, portfolio: held)
        }.value
        guard !Task.isCancelled else { return }
        shown = ReportScoped(scope: viewKey, value: built)
    }

    private func loadForecast(_ key: ReportForecastKey) async {
        guard isForecast, forecastData?.scope != key, key.version == store.version, store.budget != nil else { return }
        let params: JSON = ["today": .string(key.today), "through": .string(ReportMath.addMonths(day: key.today, key.months)), "accountIds": JSON(key.accounts)]
        do {
            let json = try await store.query("forecast", params)
            let parsed = await Task.detached(priority: .userInitiated) { ReportMath.parseForecast(json) }.value
            guard !Task.isCancelled else { return }
            forecastData = ReportScoped(scope: key, value: parsed)
        } catch {
            guard !Task.isCancelled else { return }
            forecastFailure = ReportScoped(scope: key, value: BudgetStore.readable(error))
        }
    }

    private func buildForecast(_ viewKey: ReportForecastViewKey) async {
        guard let figures = forecastData?.forScope(viewKey.data), forecastShown?.scope != viewKey, viewKey.data.version == store.version else { return }
        let budget = store.budget
        let cache = cache
        let key = viewKey.data
        let model = await Task.detached(priority: .userInitiated) {
            ReportMath.forecastView(figures, cache.get(version: key.version, budget: budget), accounts: Set(key.accounts), months: key.months,
                                    everyday: viewKey.everyday, start: key.today)
        }.value
        guard !Task.isCancelled else { return }
        forecastShown = ReportScoped(scope: viewKey, value: model)
    }
}

// MARK: Sheets

/// What a report is filtered by, a row each: the months (where the view takes any), the accounts
/// and, where they apply, the categories, each opening the choices it always had; then Export CSV.
private struct ReportFiltersSheet: View {
    var filters: ReportFilters
    /// The months in force, or nil for a view that takes none.
    var range: ReportRange?
    var earliest: String
    var accountGroups: [ReportFilterGroup]
    var accountLabel: String
    /// The categories to choose from, or nil where they don't apply.
    var categoryGroups: [ReportFilterGroup]?
    var categoryLabel: String
    /// Asks for the CSV, which is made once this sheet has closed; nil for a view with none.
    var export: (() -> Void)?
    @Environment(\.dismiss) private var dismiss
    @State private var choosingRange = false
    @State private var choosingAccounts = false
    @State private var choosingCategories = false

    var body: some View {
        NavigationStack {
            AppList {
                Section {
                    if let range { rangeRow(range) }
                    Button { choosingAccounts = true } label: {
                        row(String(localized: "Accounts"), accountLabel, "wallet.pass")
                    }
                    if categoryGroups != nil {
                        Button { choosingCategories = true } label: {
                            row(String(localized: "Categories"), categoryLabel, "square.grid.2x2")
                        }
                    }
                }
                if let export {
                    Section {
                        Button {
                            export()
                            dismiss()
                        } label: {
                            Label("Export CSV", systemImage: "square.and.arrow.up")
                        }
                    }
                }
            }
            .groupedList()
            .navigationTitle("Filters")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
        .sheetDetents([.medium])
        .sheet(isPresented: $choosingRange) {
            ReportRangeSheet(range: range ?? ReportMath.customRange(from: earliest, to: Dates.currentMonth), earliest: earliest) { filters.range = $0 }
        }
        .sheet(isPresented: $choosingAccounts) {
            ReportFilterSheet(title: String(localized: "Accounts"), groups: accountGroups,
                              selected: Binding(get: { filters.accounts }, set: { filters.accounts = $0 }))
        }
        .sheet(isPresented: $choosingCategories) {
            ReportFilterSheet(title: String(localized: "Categories"), groups: categoryGroups ?? [],
                              selected: Binding(get: { filters.categories }, set: { filters.categories = $0 }))
        }
    }

    /// The months: the presets in a menu, and the custom range's own sheet.
    private func rangeRow(_ current: ReportRange) -> some View {
        Menu {
            ForEach(ReportRangePreset.allCases.filter { $0 != .custom }, id: \.self) { preset in
                Button {
                    filters.range = ReportMath.rangeFor(preset, today: Dates.currentMonth, earliest: earliest)
                } label: {
                    if current.preset == preset { Label(preset.label, systemImage: "checkmark") } else { Text(preset.label) }
                }
            }
            Divider()
            Button { choosingRange = true } label: {
                Label("Custom range…", systemImage: current.preset == .custom ? "checkmark" : "calendar")
            }
        } label: {
            row(String(localized: "Months"), ReportMath.describeRange(current), "calendar")
        }
    }

    /// A filter's row: what it is, and what is chosen.
    private func row(_ title: String, _ value: String, _ icon: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon).foregroundStyle(Color.urAccent).frame(width: 24)
            Text(title).foregroundStyle(Color.primary)
            Spacer(minLength: 8)
            Text(value).foregroundStyle(Color.secondary).lineLimit(1)
            Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.urTertiary)
        }
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

/// A custom range of whole months; the presets are in the filters' sheet.
private struct ReportRangeSheet: View {
    var onApply: (ReportRange) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var from: String
    @State private var to: String

    init(range: ReportRange, earliest: String, onApply: @escaping (ReportRange) -> Void) {
        self.onApply = onApply
        _from = State(initialValue: range.from.isEmpty ? earliest : range.from)
        _to = State(initialValue: range.to.isEmpty ? Dates.currentMonth : range.to)
    }

    var body: some View {
        NavigationStack {
            CompactForm {
                Section {
                    MonthRow(title: String(localized: "From"), month: $from)
                    MonthRow(title: String(localized: "To"), month: $to)
                } footer: {
                    Text("Reports cover whole months, from the start of the first to the end of the last.")
                }
            }
            .navigationTitle("Custom range")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Apply") {
                        onApply(ReportMath.customRange(from: from, to: to))
                        dismiss()
                    }
                }
            }
        }
        .sheetDetents([.medium])
    }
}

/// A grouped checklist: an empty selection means all, the empty id alone means none.
private struct ReportFilterSheet: View {
    var title: String
    var groups: [ReportFilterGroup]
    @Binding var selected: Set<String>
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""

    var body: some View {
        let text = query.trimmingCharacters(in: .whitespaces)
        let shownGroups = text.isEmpty ? groups : groups.map { group in
            ReportFilterGroup(name: group.name, items: group.items.filter { $0.label.localizedCaseInsensitiveContains(text) })
        }.filter { !$0.items.isEmpty }
        NavigationStack {
            AppList {
                ForEach(shownGroups) { group in
                    let ids = group.items.map(\.id)
                    let every = ids.allSatisfy { ReportMath.filterIsOn(selected, $0) }
                    Section {
                        ForEach(group.items) { item in
                            let on = ReportMath.filterIsOn(selected, item.id)
                            Button {
                                selected = ReportMath.filterToggle(groups, selected, [item.id], on: !on)
                            } label: {
                                HStack {
                                    Text(item.label).foregroundStyle(Color.primary)
                                    Spacer()
                                    Image(systemName: on ? "checkmark.circle.fill" : "circle")
                                        .foregroundStyle(on ? Color.urAccent : Color.secondary)
                                        .imageScale(.large)
                                }
                            }
                            .accessibilityAddTraits(on ? .isSelected : [])
                        }
                    } header: {
                        HStack {
                            Text(group.name)
                            Spacer()
                            Button(every ? "Deselect all" : "Select all") {
                                selected = ReportMath.filterToggle(groups, selected, ids, on: !every)
                            }
                            .font(.footnote.weight(.semibold))
                            .accessibilityLabel(every ? "Deselect all in \(group.name)" : "Select all in \(group.name)")
                        }
                        .textCase(nil)
                    }
                }
                if shownGroups.isEmpty {
                    if text.isEmpty {
                        ContentUnavailableView("Nothing to choose", systemImage: "line.3.horizontal.decrease.circle")
                    } else {
                        ContentUnavailableView.search(text: text)
                    }
                }
            }
            .groupedList()
            .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search \(title.lowercased())")
            .navigationTitle(title)
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
                ToolbarItemGroup(placement: .bottomBarItems) {
                    Button("Select all") { selected = [] }.disabled(selected.isEmpty)
                    Spacer()
                    Button("Select none") { selected = [""] }.disabled(selected == [""])
                }
            }
        }
    }
}

/// The everyday spending a forecast assumes, typed in.
private struct ReportEverydaySheet: View {
    var onSave: (Int64) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var text: String
    @State private var error: String?

    init(current: Int64, onSave: @escaping (Int64) -> Void) {
        self.onSave = onSave
        _text = State(initialValue: Money.editable(current))
    }

    var body: some View {
        NavigationStack {
            CompactForm {
                Section {
                    AmountField(title: String(localized: "Everyday spending a month"), text: $text)
                    FormErrorView(message: error)
                } footer: {
                    Text("Spending that isn't scheduled, spread evenly over each day.")
                }
            }
            .navigationTitle("Everyday spending")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") {
                        do {
                            onSave(abs(try Money.parse(text)))
                            dismiss()
                        } catch {
                            self.error = BudgetStore.readable(error)
                        }
                    }
                }
            }
        }
        .sheetDetents([.medium])
    }
}

// MARK: The transactions behind a figure

/// The transactions behind a report figure, newest first; one opens in the editor, or all of
/// them in the register.
private struct ReportDrillView: View {
    @Environment(BudgetStore.self) private var store
    var request: ReportDrillRequest
    var cache: ReportBudgetCache
    @State private var model: ReportScoped<Int, ReportDrillModel>?
    @State private var error: String?
    @State private var editing: TransactionEditorRequest?
    @State private var search = ""

    var body: some View {
        let drill = model?.value
        let query = search.trimmingCharacters(in: .whitespaces)
        let lines = (drill?.lines ?? []).filter {
            query.isEmpty || $0.payee.localizedCaseInsensitiveContains(query) || $0.detail.localizedCaseInsensitiveContains(query)
                || $0.amount.contains(query)
        }
        AppList {
            Section {
                if let drill { Text(drill.range).font(.subheadline).foregroundStyle(Color.secondary) }
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top) { figures(drill) }
                    VStack(alignment: .leading, spacing: 10) { figures(drill) }
                }
                .padding(.vertical, 4)
                if let register = registerTarget {
                    // By its destination: a report's stack has no path for registers of its own.
                    NavigationLink { TransactionsList(target: register) } label: { Label("Open in register", systemImage: "list.bullet.rectangle") }
                }
            }
            if let error {
                Section { FormErrorView(message: error) }
            }
            if let drill {
                Section {
                    if drill.lines.isEmpty {
                        Text("No transactions.").foregroundStyle(Color.secondary)
                    } else if lines.isEmpty {
                        ContentUnavailableView.search(text: query)
                    }
                    ForEach(lines) { line in
                        Button { open(line, drill) } label: { row(line) }
                    }
                }
            } else if error == nil {
                Section { ProgressView().frame(maxWidth: .infinity).padding(.vertical, 24) }
            }
        }
        .groupedList()
        .searchable(text: $search, prompt: "Search these transactions")
        .navigationTitle(request.target.title)
        .inlineTitle()
        .cover(item: $editing) { TransactionEditor(request: $0) }
        .task(id: store.version) { await load() }
    }

    /// Where the register shows the figure's transactions (Android's registerTarget): its category
    /// or payee over the report's months, or the day, in the one account the report is filtered
    /// to. Income sources have no register filter.
    private var registerTarget: RegisterTarget? {
        let target = request.target
        let from = target.from ?? ReportMath.firstDay(request.from)
        let through = target.to ?? ReportMath.lastDay(request.to)
        let account = request.accounts.count == 1 ? request.accounts[0] : ""
        switch target.dimension {
        case "category":
            return RegisterTarget(category: target.key == ReportMath.uncategorized ? NeedsCategory : target.key, account: account, from: from, through: through)
        case "payee":
            return target.key.isEmpty ? nil : RegisterTarget(account: account, from: from, through: through, search: target.key)
        case "day":
            return RegisterTarget(account: account, from: from, through: through)
        default:
            return nil
        }
    }

    @ViewBuilder private func figures(_ drill: ReportDrillModel?) -> some View {
        let income = request.target.dimension == "income"
        Figure(label: drill?.figureLabel ?? (income ? String(localized: "Received") : String(localized: "Spent")), value: drill?.figure ?? "—")
        Spacer(minLength: 12)
        Figure(label: String(localized: "Transactions"), value: drill?.count ?? "—")
        Spacer(minLength: 12)
        Figure(label: String(localized: "Average"), value: drill?.average ?? "—", alignment: .trailing)
    }

    private func row(_ line: ReportDrillLine) -> some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(line.payee).lineLimit(1)
                Text(line.detail).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2)
            }
            Spacer(minLength: 10)
            Text(line.amount).fontWeight(.semibold).monospacedDigit().lineLimit(1)
                .foregroundStyle(line.positive ? Color.urPositive : Color.primary)
        }
        .foregroundStyle(Color.primary)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityHint("Opens the transaction")
    }

    /// The saved record as it is now, else as it was when the figures were worked out.
    private func open(_ line: ReportDrillLine, _ drill: ReportDrillModel) {
        if let record = store.budget?.row("transactions", id: line.transactionId) ?? drill.records[line.transactionId] {
            editing = TransactionEditorRequest(transaction: record)
        }
    }

    private func load() async {
        let version = store.version
        guard let budget = store.budget, model?.scope != version else { return }
        let request = request
        let cache = cache
        do {
            let built: ReportDrillModel
            if request.target.dimension == "income" {
                // The engine's details cover spending; income lines come from the same snapshot here.
                built = await Task.detached(priority: .userInitiated) {
                    ReportMath.incomeDrill(budget, cache.get(version: version, budget: budget), payee: request.target.key,
                                           fromMonth: request.from, toMonth: request.to, accounts: Set(request.accounts))
                }.value
            } else {
                let json = try await store.query("reportDetails", request.params)
                built = await Task.detached(priority: .userInitiated) {
                    ReportMath.drillFromDetails(json.array, budget, cache.get(version: version, budget: budget),
                                                from: request.target.from ?? ReportMath.firstDay(request.from),
                                                to: request.target.to ?? ReportMath.lastDay(request.to), accounts: Set(request.accounts))
                }.value
            }
            guard !Task.isCancelled else { return }
            model = ReportScoped(scope: version, value: built)
            error = nil
        } catch {
            guard !Task.isCancelled else { return }
            self.error = BudgetStore.readable(error)
        }
    }
}
