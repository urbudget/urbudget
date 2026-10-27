import SwiftUI
import Observation
import UniformTypeIdentifiers

/// A report: its page name, its group, its symbol, what it shows, and whether the Categories filter applies.
struct ReportInfo: Identifiable, Hashable {
    var id: String
    var label: String
    /// "Spending", "Income" or "Wealth": an identifier; `groupName` is what people see.
    var group: String
    var icon: String
    var blurb: String
    var categories = false

    static let all: [ReportInfo] = [
        ReportInfo(id: "categories", label: String(localized: "Spending by category"), group: "Spending", icon: "chart.pie",
                   blurb: String(localized: "Where your money went, by group and category, and how last month compares."), categories: true),
        ReportInfo(id: "payees", label: String(localized: "Spending by payee"), group: "Spending", icon: "storefront",
                   blurb: String(localized: "Who you pay most, and how often."), categories: true),
        ReportInfo(id: "trends", label: String(localized: "Spending over time"), group: "Spending", icon: "chart.bar",
                   blurb: String(localized: "Spending month by month, by group or in total."), categories: true),
        ReportInfo(id: "calendar", label: String(localized: "Spending calendar"), group: "Spending", icon: "calendar",
                   blurb: String(localized: "Spending day by day: busy days, quiet days and weekdays."), categories: true),
        ReportInfo(id: "incomeExpense", label: String(localized: "Income vs. expense"), group: "Income", icon: "arrow.left.arrow.right",
                   blurb: String(localized: "Income, spending and the surplus left in your budget."), categories: true),
        ReportInfo(id: "income", label: String(localized: "Income breakdown"), group: "Income", icon: "banknote",
                   blurb: String(localized: "Where your income comes from, and how steady it is.")),
        ReportInfo(id: "cashflow", label: String(localized: "Money in and out"), group: "Income", icon: "arrow.up.arrow.down",
                   blurb: String(localized: "Account movements across your budget boundary, including credit purchases."), categories: true),
        ReportInfo(id: "networth", label: String(localized: "Net worth"), group: "Wealth", icon: "chart.line.uptrend.xyaxis",
                   blurb: String(localized: "What you own minus what you owe, month by month.")),
        ReportInfo(id: "balance", label: String(localized: "Balance sheet"), group: "Wealth", icon: "building.columns",
                   blurb: String(localized: "Assets and liabilities by type, with the ratios that matter.")),
        ReportInfo(id: "balances", label: String(localized: "Balance over time"), group: "Wealth", icon: "chart.xyaxis.line",
                   blurb: String(localized: "Each account's balance at the end of every month.")),
        ReportInfo(id: "forecast", label: String(localized: "Forecast"), group: "Wealth", icon: "chart.line.flattrend.xyaxis",
                   blurb: String(localized: "Where your cash and card balances are heading, from what's scheduled.")),
        ReportInfo(id: "age", label: String(localized: "Cash age"), group: "Wealth", icon: "hourglass",
                   blurb: String(localized: "How long money waits in your accounts before you spend it.")),
    ]
    static let groups = ["Spending", "Income", "Wealth"]

    static func find(_ id: String) -> ReportInfo { all.first { $0.id == id } ?? all[0] }

    /// A group's name, as the title menu lists it.
    static func groupName(_ group: String) -> String {
        switch group {
        case "Spending": return String(localized: "Spending")
        case "Income": return String(localized: "Income")
        default: return String(localized: "Wealth")
        }
    }

    /// A group's heading in the list of reports.
    static func groupHeader(_ group: String) -> String {
        switch group {
        case "Spending": return String(localized: "Spending reports")
        case "Income": return String(localized: "Income reports")
        default: return String(localized: "Wealth reports")
        }
    }
}

/// Where the reports' navigation goes: a report (or one group of Spending by category), or the transactions behind a figure.
enum ReportNav: Hashable {
    case report(String, group: String?)
    case drill(ReportDrillRequest)
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

/// Reports: cards for this month's spending, income against spending, net worth and cash age over
/// six months, then every report by group, as YNAB's phone app and Android show them. A report has
/// its range and filters, charts and tables worked out by the shared engine, and opens the
/// transactions behind a figure.
struct ReportsView: View {
    @Environment(BudgetStore.self) private var store
    @State private var path: [ReportNav] = []
    @State private var filters = ReportFilters()
    @State private var cache = ReportBudgetCache()

    init() {}

    var body: some View {
        NavigationStack(path: $path) {
            ReportsOverview(cache: cache)
                .navigationTitle("Reports")
                .toolbar { ToolbarItem(placement: .trailingBar) { BudgetActionsMenu() } }
                .navigationDestination(for: ReportNav.self) { nav in
                    switch nav {
                    case .report(let id, let group):
                        ReportPage(id: id, group: group, filters: filters, cache: cache) { path.append($0) }
                    case .drill(let request):
                        ReportDrillView(request: request, cache: cache)
                    }
                }
        }
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

private struct ReportsOverview: View {
    @Environment(BudgetStore.self) private var store
    var cache: ReportBudgetCache
    @State private var attempt = 0
    @State private var result: ReportScoped<ReportOverviewKey, ReportOverviewModel?>?
    @State private var failure: ReportScoped<ReportOverviewKey, String>?
    @State private var pickingMonth = false

    var body: some View {
        let key = ReportOverviewKey(version: store.version, month: store.month, attempt: attempt)
        let failed = failure?.forScope(key)
        let current = result?.scope == key
        List {
            monthSection
            if let failed {
                Section {
                    EmptyState(title: String(localized: "Reports unavailable"), detail: failed, symbol: "exclamationmark.triangle",
                               action: String(localized: "Retry")) { attempt += 1 }
                }
            } else if let result {
                if let model = result.value {
                    Group { cards(model) }
                        .disabled(!current)
                        .opacity(current ? 1 : 0.55)
                }
            } else {
                Section { ProgressView().frame(maxWidth: .infinity).padding(.vertical, 32) }
            }
            allReports
        }
        .groupedList()
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
        let filter: JSON = ["from": .string(Dates.addMonths(key.month, -5)), "to": .string(key.month), "accountIds": [], "categoryIds": []]
        do {
            let json = try await store.query("reports", filter)
            let cache = cache
            let model = await Task.detached(priority: .userInitiated) {
                ReportMath.overviewModel(ReportMath.parseReport(json), cache.get(version: version, budget: budget))
            }.value
            guard !Task.isCancelled else { return }
            result = ReportScoped(scope: key, value: model)
        } catch {
            guard !Task.isCancelled else { return }
            failure = ReportScoped(scope: key, value: BudgetStore.readable(error))
        }
    }

    private var monthSection: some View {
        Section {
            HStack {
                Button { store.selectMonth(Dates.addMonths(store.month, -1)) } label: {
                    Image(systemName: "chevron.backward").font(.body.weight(.semibold))
                }
                .accessibilityLabel("Previous month")
                Spacer()
                Button { pickingMonth = true } label: {
                    Text(Dates.monthLabel(store.month)).font(.headline).foregroundStyle(Color.primary)
                }
                .accessibilityHint("Chooses another month")
                Spacer()
                Button { store.selectMonth(Dates.addMonths(store.month, 1)) } label: {
                    Image(systemName: "chevron.forward").font(.body.weight(.semibold))
                }
                .accessibilityLabel("Next month")
            }
            .buttonStyle(.borderless)
        } footer: {
            Text("The cards cover the six months ending here.")
        }
    }

    private func header(_ title: LocalizedStringKey, icon: String, id: String) -> some View {
        NavigationLink(value: ReportNav.report(id, group: nil)) {
            Label(title, systemImage: icon).font(.headline).foregroundStyle(Color.urAccent)
        }
        .accessibilityHint("Opens the full report")
    }

    private func bigFigure(_ text: String, negative: Bool) -> some View {
        Text(text).font(.largeTitle.weight(.bold)).monospacedDigit().lineLimit(1).minimumScaleFactor(0.5)
            .foregroundStyle(negative ? Color.urNegative : Color.primary)
    }

    @ViewBuilder private func spendRow(_ row: ReportSpendRow, month: String, dot: Bool) -> some View {
        let content = HStack(spacing: 10) {
            if dot {
                RoundedRectangle(cornerRadius: 3).fill(ReportPalette.color(row.color)).frame(width: 6, height: 18).accessibilityHidden(true)
            }
            Text(row.label).lineLimit(1)
            Spacer(minLength: 8)
            Text(row.valueText).monospacedDigit().lineLimit(1)
        }
        .accessibilityElement(children: .combine)
        if row.key.hasPrefix("__") && row.key != ReportMath.uncategorized {
            content
        } else {
            NavigationLink(value: ReportNav.drill(ReportDrillRequest(from: month, to: month, accounts: [], categories: [],
                                                                     target: ReportDrillTarget(title: row.label, dimension: "category", key: row.key)))) {
                content
            }
        }
    }

    @ViewBuilder private func cards(_ model: ReportOverviewModel) -> some View {
        Section {
            header("Spending breakdown", icon: "chart.pie", id: "categories")
            VStack(alignment: .leading, spacing: 4) {
                Text(model.monthName).font(.subheadline).foregroundStyle(Color.secondary)
                bigFigure(model.spendingText, negative: false)
                if !model.rows.isEmpty {
                    ReportSegmentBar(rows: model.rows).padding(.vertical, 6)
                } else if model.nothingSpent {
                    Text("Nothing spent yet this month.").font(.headline)
                }
            }
            .padding(.vertical, 4)
            .accessibilityElement(children: .combine)
            if !model.rows.isEmpty {
                HStack {
                    Text("Top categories")
                    Spacer()
                    Text("Spent")
                }
                .font(.footnote)
                .foregroundStyle(Color.secondary)
                .accessibilityHidden(true)
                ForEach(model.rows) { spendRow($0, month: model.month, dot: true) }
            }
            if let lead = model.refundLead {
                Text(lead).font(.subheadline)
                ForEach(model.refunds) { spendRow($0, month: model.month, dot: false) }
            }
        }
        Section {
            header("Income vs. spending", icon: "arrow.left.arrow.right", id: "incomeExpense")
            VStack(alignment: .leading, spacing: 10) {
                Text(model.incomeLead).font(.title3.weight(.semibold))
                ReportMiniBars(model: model.incomeBars)
                ReportLegendView(items: [ReportLegendItem(label: String(localized: "Income"), color: ReportColors.positive),
                                         ReportLegendItem(label: String(localized: "Spending"), color: 0)])
            }
            .padding(.vertical, 4)
        }
        Section {
            header("Net worth", icon: "chart.line.uptrend.xyaxis", id: "networth")
            VStack(alignment: .leading, spacing: 8) {
                bigFigure(model.netWorthText, negative: model.netWorthNegative)
                ValueRow(title: String(localized: "Assets"), value: model.assetsText)
                ValueRow(title: String(localized: "Debts"), value: model.debtsText, color: model.debtsNegative ? .urNegative : .primary)
                ReportMiniBars(model: model.netWorthBars)
            }
            .padding(.vertical, 4)
        }
        Section {
            header("Cash age", icon: "hourglass", id: "age")
            VStack(alignment: .leading, spacing: 8) {
                bigFigure(model.ageText, negative: false)
                ReportMiniBars(model: model.ageBars)
            }
            .padding(.vertical, 4)
        }
    }

    private var allReports: some View {
        ForEach(ReportInfo.groups, id: \.self) { group in
            Section {
                ForEach(ReportInfo.all.filter { $0.group == group }) { report in
                    NavigationLink(value: ReportNav.report(report.id, group: nil)) {
                        Label {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(report.label)
                                Text(report.blurb).font(.footnote).foregroundStyle(Color.secondary)
                            }
                        } icon: {
                            Image(systemName: report.icon).foregroundStyle(Color.urAccent)
                        }
                    }
                }
            } header: {
                Text(ReportInfo.groupHeader(group))
            }
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

/// The page title opens the other reports, as Android's group and chips do.
private struct ReportTitleMenu: ViewModifier {
    var enabled: Bool
    var current: String
    var choose: (String) -> Void

    func body(content: Content) -> some View {
        if enabled {
            content.toolbarTitleMenu {
                ForEach(ReportInfo.groups, id: \.self) { group in
                    Section(ReportInfo.groupName(group)) {
                        ForEach(ReportInfo.all.filter { $0.group == group }) { report in
                            Button { choose(report.id) } label: {
                                Label(report.label, systemImage: report.id == current ? "checkmark" : report.icon)
                            }
                        }
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

/// One report: its filters, its figures for exactly those filters and this snapshot, and the way
/// to the transactions behind them. While new figures load, the last ones stay on screen, dimmed.
private struct ReportPage: View {
    @Environment(BudgetStore.self) private var store
    var group: String?
    var filters: ReportFilters
    var cache: ReportBudgetCache
    var navigate: (ReportNav) -> Void

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
    @State private var choosingRange = false
    @State private var choosingAccounts = false
    @State private var choosingCategories = false
    @State private var editingEveryday = false
    @State private var exportFile: TextFile?
    @State private var exportName = "urbudget-report.csv"
    @State private var exporting = false
    @State private var exportError: String?

    init(id: String, group: String?, filters: ReportFilters, cache: ReportBudgetCache, navigate: @escaping (ReportNav) -> Void) {
        _id = State(initialValue: id)
        self.group = group
        self.filters = filters
        self.cache = cache
        self.navigate = navigate
    }

    private var info: ReportInfo { ReportInfo.find(id) }
    private var isForecast: Bool { id == "forecast" }
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

    private var accountGroups: [ReportFilterGroup] { context.map { ReportMath.accountFilterGroups($0, budgetOnly: isForecast) } ?? [] }
    private var categoryGroups: [ReportFilterGroup] { context.map(ReportMath.categoryFilterGroups) ?? [] }

    var body: some View {
        let key = currentKey()
        let viewKey = ReportViewKey(data: key, id: id, options: options, scope: scope(key))
        let forecastViewKey = ReportForecastViewKey(data: forecastKey, everyday: everyday)
        ScrollViewReader { proxy in
            content(key: key, viewKey: viewKey, proxy: proxy)
        }
        .navigationTitle(group ?? info.label)
        .inlineTitle()
        .modifier(ReportTitleMenu(enabled: group == nil, current: id) { id = $0 })
        .toolbar { toolbar(key: key, viewKey: viewKey) }
        .task(id: store.version) { await loadContext() }
        .task(id: ReportTaskKey(key: key, active: !isForecast)) { await loadData(key) }
        .task(id: ReportTaskKey(key: viewKey, active: data?.scope == key)) { await buildView(viewKey) }
        .task(id: ReportTaskKey(key: forecastKey, active: isForecast)) { await loadForecast(forecastKey) }
        .task(id: ReportTaskKey(key: forecastViewKey, active: isForecast && forecastData?.scope == forecastKey)) { await buildForecast(forecastViewKey) }
        .onChange(of: id) {
            // A report's own choices start afresh when another report opens, as the desktop's do.
            byGroup = true
            chartAccounts = nil
            payeeQuery = ""
            ahead = 6
            everyday = nil
            exportError = nil
        }
        .sheet(isPresented: $choosingRange) {
            ReportRangeSheet(range: range, earliest: context?.earliestMonth ?? Dates.currentMonth) { filters.range = $0 }
        }
        .sheet(isPresented: $choosingAccounts) {
            ReportFilterSheet(title: String(localized: "Accounts"), groups: accountGroups,
                              selected: Binding(get: { filters.accounts }, set: { filters.accounts = $0 }))
        }
        .sheet(isPresented: $choosingCategories) {
            ReportFilterSheet(title: String(localized: "Categories"), groups: categoryGroups,
                              selected: Binding(get: { filters.categories }, set: { filters.categories = $0 }))
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
        let failed = failure?.forScope(key)
        List {
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
        let loading = isForecast ? forecastFailure?.forScope(forecastKey) == nil && !forecastReady : failure?.forScope(key) == nil && !ready
        ToolbarItemGroup(placement: .trailingBar) {
            if loading { ProgressView().accessibilityLabel("Calculating") }
            if !isForecast {
                Button { export(key) } label: { Image(systemName: "square.and.arrow.up") }
                    .accessibilityLabel("Export CSV")
            }
            BudgetActionsMenu()
        }
    }

    private var filterSection: some View {
        let accountLabel = ReportMath.filterSummary(accountGroups, filters.accounts,
                                                    allLabel: isForecast ? String(localized: "All budget accounts") : String(localized: "All accounts"),
                                                    several: { String(localized: "\($0) accounts", locale: .app) })
        let categoryLabel = ReportMath.filterSummary(categoryGroups, filters.categories, allLabel: String(localized: "All categories"),
                                                     several: { String(localized: "\($0) categories", locale: .app) })
        return Section {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    if !isForecast { rangeMenu }
                    Button { choosingAccounts = true } label: {
                        ReportChip(title: String(localized: "Accounts"), text: accountLabel, icon: "wallet.pass", active: !filters.accounts.isEmpty)
                    }
                    .buttonStyle(.plain)
                    if info.categories {
                        Button { choosingCategories = true } label: {
                            ReportChip(title: String(localized: "Categories"), text: categoryLabel, icon: "square.grid.2x2", active: !filters.categories.isEmpty)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.vertical, 2)
            }
            .listRowInsets(EdgeInsets())
            .listRowBackground(Color.clear)
        } footer: {
            Text(info.blurb)
        }
    }

    private var rangeMenu: some View {
        let current = range
        let earliest = context?.earliestMonth ?? Dates.currentMonth
        return Menu {
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
            ReportChip(title: String(localized: "Months"), text: ReportMath.describeRange(current), icon: "calendar", active: false)
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
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
    private func export(_ key: ReportKey) {
        exportError = nil
        Task {
            do {
                let csv = try await store.query("reportCsv", key.filter)
                guard key == currentKey() else { return }
                exportFile = TextFile(data: Data(csv.string.utf8))
                exportName = "urbudget-report-\(key.from)-\(key.to).csv"
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
        guard !isForecast, data?.scope != key, key.version == store.version, store.budget != nil else { return }
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

    private func buildView(_ viewKey: ReportViewKey) async {
        guard let figures = data?.forScope(viewKey.data), shown?.scope != viewKey, viewKey.data.version == store.version else { return }
        let budget = store.budget
        let cache = cache
        let built = await Task.detached(priority: .userInitiated) {
            ReportMath.reportView(viewKey.id, figures, cache.get(version: viewKey.data.version, budget: budget), viewKey.scope, viewKey.options)
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

/// A custom range of whole months; the presets are in the range menu.
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
            Form {
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
        .presentationDetents([.medium])
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
            List {
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
            .searchable(text: $query, prompt: "Search \(title.lowercased())")
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
            Form {
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
        .presentationDetents([.medium])
    }
}

// MARK: The transactions behind a figure

/// The transactions behind a report figure, newest first; one opens in the editor.
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
        List {
            Section {
                if let drill { Text(drill.range).font(.subheadline).foregroundStyle(Color.secondary) }
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top) { figures(drill) }
                    VStack(alignment: .leading, spacing: 10) { figures(drill) }
                }
                .padding(.vertical, 4)
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
