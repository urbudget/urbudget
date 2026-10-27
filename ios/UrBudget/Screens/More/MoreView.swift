import SwiftUI

/// The pages More leads to.
enum MoreRoute: Hashable {
    case library, sync, schedules, loans, payees, banks, settings, assistantSettings
}

/// More: the budget and its library, then everything the tabs don't cover, as Android's More drawer
/// lists it: planning, bringing data in, account and sync, the assistant, and settings.
struct MoreView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    @State private var path: [MoreRoute] = []
    @State private var sheet: MoreSheet?

    enum MoreSheet: String, Identifiable {
        case assistant, bankImport, ynabImport
        var id: String { rawValue }
    }

    var body: some View {
        NavigationStack(path: $path) {
            // One small function per section keeps the list quick to type-check.
            List {
                budgetSection(store.budget ?? [:])
                accountSection
                planningSection
                importSection
                Section {
                    NavigationLink(value: MoreRoute.settings) {
                        MoreRow(title: String(localized: "Settings & data"), detail: String(localized: "Backups, imports, formats and appearance"),
                                symbol: "gearshape", tint: .secondary)
                    }
                }
            }
            .groupedList()
            .navigationTitle("More")
            .navigationDestination(for: MoreRoute.self) { route in
                switch route {
                case .library: LibraryView()
                case .sync: AccountSyncView()
                case .schedules: SchedulesView()
                case .loans: LoanPlannerView()
                case .payees: PayeesView()
                case .banks: BankConnectionsView()
                case .settings: SettingsView()
                case .assistantSettings: AssistantSettingsView()
                }
            }
            .navigationDestination(for: RegisterTarget.self) { TransactionsList(target: $0) }
            .sheet(item: $sheet) { sheet in
                switch sheet {
                case .assistant: AssistantView()
                case .bankImport: BankFileImportView()
                case .ynabImport: YnabImportView()
                }
            }
            .onAppear(perform: openLaunchRoute)
        }
    }

    // MARK: Sections

    private func budgetSection(_ budget: JSON) -> some View {
        Section {
            NavigationLink(value: MoreRoute.library) {
                HStack(spacing: 12) {
                    Text(initials(budget.text("name")))
                        .font(.headline).foregroundStyle(.white)
                        .frame(width: 44, height: 44)
                        .background(Color.urAccent, in: RoundedRectangle(cornerRadius: 11, style: .continuous))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(budget.text("name")).font(.headline)
                        Text("\(budget.text("currency")) · Switch or manage budgets").font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
                .padding(.vertical, 4)
            }
        }
    }

    private var accountSection: some View {
        Section {
            NavigationLink(value: MoreRoute.sync) {
                MoreRow(title: String(localized: "Account & sync"), detail: syncDetail, symbol: "arrow.triangle.2.circlepath.icloud", tint: .urAccent,
                        badge: sync.conflicts)
            }
            Button { sheet = .assistant } label: {
                MoreRow(title: String(localized: "Assistant"), detail: String(localized: "Ask about your budget and review its changes"),
                        symbol: "sparkles", tint: .urBrand)
            }
            .foregroundStyle(Color.primary)
        }
    }

    private var planningSection: some View {
        Section("Planning") {
            NavigationLink(value: MoreRoute.schedules) {
                MoreRow(title: String(localized: "Scheduled"), detail: String(localized: "Bills, income and repeating transactions"),
                        symbol: "calendar.badge.clock", tint: .urAccent)
            }
            NavigationLink(value: MoreRoute.loans) {
                MoreRow(title: String(localized: "Loan planner"), detail: String(localized: "Payoff dates and extra payments"), symbol: "percent", tint: .urWarning)
            }
            NavigationLink(value: MoreRoute.payees) {
                MoreRow(title: String(localized: "Payees & rules"), detail: String(localized: "Default categories and import rules"), symbol: "person.2", tint: .urPositive)
            }
            NavigationLink(value: RegisterTarget(category: NeedsCategory)) {
                MoreRow(title: String(localized: "Needs a category"), detail: String(localized: "Spending still waiting for a category"), symbol: "tag", tint: .urWarning)
            }
        }
    }

    private var importSection: some View {
        Section("Bring in transactions") {
            NavigationLink(value: MoreRoute.banks) {
                MoreRow(title: String(localized: "Bank connections"), detail: String(localized: "Link accounts and review downloads"),
                        symbol: "building.columns", tint: .urAccent)
            }
            Button { sheet = .bankImport } label: {
                MoreRow(title: String(localized: "Import bank file"), detail: String(localized: "CSV, OFX, QFX or QIF, reviewed first"),
                        symbol: "doc.badge.arrow.up", tint: .urAccent)
            }
            .foregroundStyle(Color.primary)
            Button { sheet = .ynabImport } label: {
                MoreRow(title: String(localized: "Import from YNAB"), detail: String(localized: "From YNAB's API or its export files"),
                        symbol: "square.and.arrow.down.on.square", tint: .urAccent)
            }
            .foregroundStyle(Color.primary)
        }
    }

    private var syncDetail: String {
        guard sync.signedIn else { return String(localized: "Sign in to sync with your other devices") }
        if sync.conflicts > 0 { return String(localized: "\(sync.conflicts) budgets need review", locale: .app) }
        return sync.unlocked ? String(localized: "Encrypted sync is on") : String(localized: "Signed in · sync is locked")
    }

    private func initials(_ name: String) -> String {
        let letters = name.filter { $0.isLetter || $0.isNumber }.prefix(2).uppercased()
        return letters.isEmpty ? "UB" : letters
    }

    /// Debug builds can open a page at launch (`-open sync`), for screenshots on a device.
    private func openLaunchRoute() {
        guard path.isEmpty, sheet == nil, let route = LaunchRoute.open else { return }
        LaunchRoute.open = nil
        switch route {
        case "library": path = [.library]
        case "sync": path = [.sync]
        case "schedules": path = [.schedules]
        case "loans": path = [.loans]
        case "payees": path = [.payees]
        case "banks": path = [.banks]
        case "settings": path = [.settings]
        case "assistantSettings": path = [.assistantSettings]
        case "assistant": sheet = .assistant
        case "bankImport": sheet = .bankImport
        case "ynab": sheet = .ynabImport
        default: LaunchRoute.open = route
        }
    }
}

/// A row of More: a tinted symbol, the page's name, what it's for, and a count when something needs attention.
struct MoreRow: View {
    var title: String
    var detail: String
    var symbol: String
    var tint: Color
    var badge = 0

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: symbol)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 32, height: 32)
                .background(tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                Text(detail).font(.footnote).foregroundStyle(Color.secondary)
            }
            Spacer(minLength: 0)
            if badge > 0 {
                Text("\(badge)").font(.caption.bold()).foregroundStyle(.white)
                    .padding(.horizontal, 7).frame(minHeight: 20)
                    .background(Color.urWarning, in: Capsule())
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// Where a debug build opens at launch: `-tab accounts`, `-open sync`. Release builds ignore it.
enum LaunchRoute {
    static let morePages: Set<String> = ["library", "sync", "schedules", "loans", "payees", "banks", "settings",
                                         "assistantSettings", "assistant", "bankImport", "ynab"]
    static let accountPages: Set<String> = ["register", "networth"]
    static var initialTab: AppTab {
        tab ?? (open.map(morePages.contains) == true ? .more : open.map(accountPages.contains) == true ? .accounts : .budget)
    }

    #if DEBUG
    nonisolated(unsafe) static var tab: AppTab? = {
        switch value(after: "-tab") {
        case "budget": return .budget
        case "transactions": return .transactions
        case "accounts": return .accounts
        case "reports": return .reports
        case "more": return .more
        default: return nil
        }
    }()
    nonisolated(unsafe) static var open: String? = value(after: "-open")
    /// A file in the app's Documents folder, opened as if another app had shared it (`-openFile statement.csv`).
    nonisolated(unsafe) static var file: String? = value(after: "-openFile")
    #else
    static var tab: AppTab? { nil }
    nonisolated(unsafe) static var open: String? = nil
    nonisolated(unsafe) static var file: String? = nil
    #endif

    private static func value(after flag: String) -> String? {
        let arguments = ProcessInfo.processInfo.arguments
        guard let index = arguments.firstIndex(of: flag), arguments.indices.contains(index + 1) else { return nil }
        return arguments[index + 1]
    }
}
