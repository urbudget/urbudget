import SwiftUI

@main
struct UrBudgetApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var store = BudgetStore()
    @State private var sync = SyncCoordinator()
    @State private var appStore = AppStoreSubscription()
    @AppStorage("theme") private var theme = "system"
    /// Unit tests open their own temporary library; the app's must stay closed.
    private let testing = ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(store)
                .environment(sync)
                .environment(appStore)
                // The app's language, with Western digits in counts and other numbers, as budgets write them.
                .environment(\.locale, .app)
                .tint(.urAccent)
                .preferredColorScheme(theme == "dark" ? .dark : theme == "light" ? .light : nil)
                .task {
                    guard !testing else { return }
                    sync.attach(store)
                    appStore.attach(store)
                    store.start()
                }
        }
    }
}

/// The welcome screen until there is a budget, then the tabs. Errors and toasts show over both.
struct RootView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    @Environment(AppStoreSubscription.self) private var appStore
    @Environment(\.scenePhase) private var phase
    @Environment(\.undoManager) private var undoManager

    private struct History: Equatable { var revision: String; var undo: Bool }
    @State private var opened: OpenedFile?
    @State private var backup: OpenedFile?
    /// Debug builds show the welcome screen over a budget with `-open welcome`, for screenshots.
    @State private var welcomePreview = LaunchRoute.open == "welcome"
    @State private var newTransaction: TransactionEditorRequest?

    var body: some View {
        Group {
            if store.loading {
                ProgressView().controlSize(.large)
            } else if store.budget == nil {
                WelcomeView()
            } else {
                // Every form, picker and report belongs to one library entry.
                MainTabs().id(store.identity ?? "")
            }
        }
        .alert("Couldn’t complete that", isPresented: Binding(get: { store.error != nil }, set: { if !$0 { store.error = nil } })) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(store.error ?? "")
        }
        .overlay(alignment: .bottom) { ToastView() }
        .onChange(of: History(revision: store.revision, undo: store.undoAvailable)) { _, _ in offerUndo() }
        .onChange(of: phase) { _, phase in
            switch phase {
            case .active:
                store.foreground()
                startForegroundWork()
            case .background:
                HostEvents.shared.stop()
                sync.stop()
            default:
                break
            }
        }
        // Background services start once the library is open.
        .onChange(of: store.loading) { _, loading in
            if !loading && phase == .active { startForegroundWork() }
        }
        .onReceive(NotificationCenter.default.publisher(for: .urbudgetHostEvent)) { notification in
            guard let event = notification.hostEvent else { return }
            switch event.name {
            case "host://open-url":
                if let url = URL(string: event.payload.text("url")), ["https", "http"].contains(url.scheme?.lowercased() ?? "") {
                    WebPresenter.open(url)
                }
            case "host://open-file":
                let path = event.payload.text("path")
                if !path.isEmpty { WebPresenter.share(URL(fileURLWithPath: path)) }
            default:
                break
            }
        }
        // Files shared to UrBudget from Files, Mail or AirDrop: statements and YNAB exports open their
        // imports; a backup asks whether it's a budget of its own or replaces this one.
        .onOpenURL(perform: open)
        .onChange(of: store.loading) { _, loading in
            guard !loading, let name = LaunchRoute.file,
                  let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first else { return }
            LaunchRoute.file = nil
            open(documents.appendingPathComponent(name))
        }
        .sheet(isPresented: $welcomePreview) { WelcomeView() }
        // The quick action and the Add a transaction shortcut, once a budget is open.
        .onChange(of: AppActions.shared.pending, initial: true) { _, _ in takeAction() }
        .onChange(of: store.budget == nil) { _, _ in takeAction() }
        .cover(item: $newTransaction) { TransactionEditor(request: $0) }
        .sheet(item: $opened) { file in
            if file.kind == .ynab { YnabImportView(files: [file.url]) } else { BankFileImportView(file: file.url) }
        }
        .confirmationDialog(backupTitle,
                            isPresented: Binding(get: { backup != nil }, set: { if !$0 { backup = nil } }), titleVisibility: .visible) {
            Button("Add as a new budget") { openBackup(replace: false) }
            if store.budget != nil { Button("Replace this budget", role: .destructive) { openBackup(replace: true) } }
        } message: {
            Text("A new budget leaves this one as it is. Replacing keeps a copy of this budget with its backups, and you can undo it.")
        }
        // A finished ChatGPT sign-in closes its callback page; closing the page early stops the sign-in.
        .onReceive(NotificationCenter.default.publisher(for: .assistantSignInFinished)) { _ in WebPresenter.closePage() }
        .onReceive(NotificationCenter.default.publisher(for: .urbudgetWebPageClosed)) { _ in
            if AssistantController.shared.signingIn { AssistantController.shared.cancelSignIn() }
        }
    }

    /// Shaking the phone undoes the last budget change, as it undoes elsewhere in iOS: the system's
    /// undo manager holds one action standing for the budget's history, renewed whenever it changes.
    /// (Typing in a field registers its own undo, which a shake offers first.)
    private func offerUndo() {
        guard let undoManager else { return }
        undoManager.removeAllActions(withTarget: store)
        guard store.undoAvailable else { return }
        undoManager.registerUndo(withTarget: store) { store in
            MainActor.assumeIsolated {
                store.toast = nil
                store.undo()
            }
        }
    }

    /// "Open UrBudget-2026-10-21.json?"
    private var backupTitle: Text {
        if let name = backup?.url.lastPathComponent { return Text("Open \(name)?") }
        return Text("Open this backup?")
    }

    private func takeAction() {
        guard AppActions.shared.pending == .newTransaction, !store.loading, store.budget != nil else { return }
        AppActions.shared.pending = nil
        newTransaction = TransactionEditorRequest()
    }

    private func open(_ url: URL) {
        guard let file = OpenedFile(url) else {
            if url.isFileURL { store.error = String(localized: "UrBudget opens JSON backups, bank statements (CSV, OFX, QFX, QIF or MT940) and YNAB exports.") }
            return
        }
        let name = url.lastPathComponent
        if file.kind == .backup { backup = file }
        else if store.budget == nil { store.error = String(localized: "Create or open a budget first, then import \(name).") }
        else { opened = file }
    }

    private func openBackup(replace: Bool) {
        guard let file = backup else { return }
        backup = nil
        Task {
            do {
                let data = try await Task.detached(priority: .userInitiated) {
                    try ImportFiles.read(file.url, limit: 32 * 1024 * 1024, tooLarge: String(localized: "Choose a JSON backup smaller than 32 MB.")).data
                }.value
                if replace { store.importBudget(data) }
                else { store.importAsNewBudget(data) { store.showToast(openedAsNew(store.budget?.text("name"))) } }
            } catch {
                store.error = BudgetStore.readable(error)
            }
        }
    }

    private func openedAsNew(_ name: String?) -> String {
        guard let name else { return String(localized: "Opened the backup as a new budget.") }
        return String(localized: "Opened \(name) as a new budget.")
    }

    private func startForegroundWork() {
        guard !store.loading else { return }
        // The engine writes its messages in the app's language; it's told again on each return.
        Task.detached(priority: .utility) { AppLanguage.tellEngine() }
        HostEvents.shared.start()
        sync.start()
        appStore.start()
    }
}

/// A file another app opened in UrBudget, by what it holds.
struct OpenedFile: Identifiable {
    enum Kind { case backup, ynab, statement }
    let id = UUID()
    var url: URL
    var kind: Kind

    init?(_ url: URL) {
        guard url.isFileURL else { return nil }
        switch url.pathExtension.lowercased() {
        case "json": kind = .backup
        case "zip", "tsv": kind = .ynab
        case "csv", "ofx", "qfx", "qif", "sta", "940", "mt940", "txt": kind = .statement
        default: return nil
        }
        self.url = url
    }
}

enum AppTab: Hashable { case budget, transactions, accounts, reports, more }

/// Budget, Transactions, Accounts and Reports are tabs; More holds everything else.
struct MainTabs: View {
    @State private var tab = LaunchRoute.initialTab

    private static let tabs: [(tab: AppTab, title: String, symbol: String)] = [
        (.budget, String(localized: "Budget"), "square.grid.2x2"),
        (.transactions, String(localized: "Transactions"), "arrow.left.arrow.right"),
        (.accounts, String(localized: "Accounts"), "building.columns"),
        (.reports, String(localized: "Reports"), "chart.bar.xaxis"),
        (.more, String(localized: "More"), "ellipsis.circle"),
    ]

    var body: some View {
        tabView
            .background {
            // Hidden buttons carry ⌘1–⌘5, as the desktop's sidebar shortcuts do.
                ForEach(Array(Self.tabs.enumerated()), id: \.offset) { index, destination in
                    Button { tab = destination.tab } label: { Text(verbatim: "") }
                        .keyboardShortcut(KeyEquivalent(Character(String(index + 1))), modifiers: .command)
                        .hidden()
                }
            }
    }

    /// On iPad (iOS 18 and later) the tabs can also open as a sidebar; on iPhone they're the tab bar.
    @ViewBuilder private var tabView: some View {
        if #available(iOS 18.0, *) {
            TabView(selection: $tab) {
                ForEach(Self.tabs, id: \.tab) { item in
                    Tab(item.title, systemImage: item.symbol, value: item.tab) { screen(item.tab) }
                }
            }
            .tabViewStyle(.sidebarAdaptable)
        } else {
            TabView(selection: $tab) {
                ForEach(Self.tabs, id: \.tab) { item in
                    screen(item.tab).tabItem { Label(item.title, systemImage: item.symbol) }.tag(item.tab)
                }
            }
        }
    }

    @ViewBuilder private func screen(_ tab: AppTab) -> some View {
        switch tab {
        case .budget: BudgetView()
        case .transactions: NavigationStack { TransactionsList(target: RegisterTarget()) }
        case .accounts: AccountsView()
        case .reports: ReportsView()
        case .more: MoreView()
        }
    }
}

/// A short message above the tab bar, with Undo for the change it describes.
struct ToastView: View {
    @Environment(BudgetStore.self) private var store

    var body: some View {
        ZStack {
            if let toast = store.toast {
                HStack(spacing: 12) {
                    Text(toast.message).font(.subheadline).foregroundStyle(Color.primary).frame(maxWidth: .infinity, alignment: .leading)
                    if let action = toast.action {
                        Button(action.label) {
                            store.toast = nil
                            action.run()
                        }
                        .font(.subheadline.bold())
                        .foregroundStyle(Color.urAccent)
                    } else if toast.undo {
                        Button("Undo") {
                            store.toast = nil
                            store.undo()
                        }
                        .font(.subheadline.bold())
                        .foregroundStyle(Color.urAccent)
                    }
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 14)
                .glassCard(cornerRadius: 22)
                .padding(.horizontal, 12)
                .padding(.bottom, 120)
                .transition(.move(edge: .bottom).combined(with: .opacity))
                // Swiping it down puts it away early.
                .gesture(DragGesture(minimumDistance: 12).onEnded { if $0.translation.height > 12 { store.toast = nil } })
                .task(id: toast.id) {
                    // VoiceOver reads it out; one with Undo stays long enough to reach the button.
                    let offer = toast.action?.label ?? (toast.undo ? String(localized: "Undo") : nil)
                    AccessibilityNotification.Announcement(announcement(toast.message, offer: offer)).post()
                    // An offer (Add rule, Apply) waits longest, as the desktop's does.
                    let seconds = toast.action != nil ? 10 : toast.undo ? 8 : 4
                    try? await Task.sleep(for: .seconds(UIAccessibility.isVoiceOverRunning && offer != nil ? 14 : seconds))
                    if store.toast?.id == toast.id { store.toast = nil }
                }
            }
        }
        .animation(.spring(duration: 0.3), value: store.toast)
        .sensoryFeedback(.success, trigger: store.toast?.id) { _, new in new != nil }
    }

    /// What VoiceOver reads: the message, then the button it offers ("Rule added. Undo available.").
    private func announcement(_ message: String, offer: String?) -> String {
        guard let offer else { return message }
        return String(localized: "\(message) \(offer) available.")
    }
}

/// What the transaction editor opens with: a transaction to edit, or the account a new one goes in.
struct TransactionEditorRequest: Identifiable {
    let id = UUID()
    var transaction: JSON?
    var account = ""
}

/// "+ Transaction" floats above the tab bar on Liquid Glass, as Android's floats on its frosted
/// glass, and shrinks to its plus while a list scrolls down (see `shrinksAddButton`).
struct AddTransactionButton: View {
    var account = ""
    var compact = false
    @State private var editing: TransactionEditorRequest?

    var body: some View {
        Button {
            editing = TransactionEditorRequest(account: account)
        } label: {
            HStack(spacing: 10) {
                Image(systemName: "plus")
                    .font(.system(size: 17, weight: .bold))
                    .foregroundStyle(.white)
                    .frame(width: 34, height: 34)
                    .background(Color.urAccent.gradient, in: Circle())
                if !compact {
                    Text("Transaction")
                        .font(.headline)
                        .foregroundStyle(Color.primary)
                        .fixedSize()
                        .transition(.opacity.combined(with: .scale(scale: 0.85, anchor: .leading)))
                }
            }
            .padding(.leading, 7)
            .padding(.trailing, compact ? 7 : 18)
            .frame(height: 48)
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .glassCapsule()
        .animation(.snappy(duration: 0.28), value: compact)
        .padding(.trailing, 16)
        .padding(.bottom, 12)
        .accessibilityLabel("Add transaction")
        .keyboardShortcut("n", modifiers: .command)
        .sensoryFeedback(.impact(weight: .light), trigger: editing != nil)
        .cover(item: $editing) { TransactionEditor(request: $0) }
    }
}

extension View {
    /// Liquid Glass on iOS 26, a frosted capsule before it.
    @ViewBuilder func glassCapsule() -> some View {
        if #available(iOS 26.0, *) {
            glassEffect(.regular.interactive(), in: Capsule())
        } else {
            background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(Color.white.opacity(0.22), lineWidth: 0.5))
                .shadow(color: .black.opacity(0.14), radius: 14, y: 5)
        }
    }

    /// Glass for a floating message card, tinted with the background so its text reads over a busy list.
    @ViewBuilder func glassCard(cornerRadius: CGFloat) -> some View {
        if #available(iOS 26.0, *) {
            glassEffect(.regular.tint(Color.groupedBackground.opacity(0.7)), in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
        } else {
            background(.regularMaterial, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
                .shadow(color: .black.opacity(0.14), radius: 14, y: 5)
        }
    }

    /// Sets `compact` while the list scrolls down and clears it when it scrolls back up, as
    /// Android's add button shrinks. iOS 17 keeps the button full size.
    @ViewBuilder func shrinksAddButton(_ compact: Binding<Bool>) -> some View {
        if #available(iOS 18.0, *) {
            onScrollGeometryChange(for: CGFloat.self, of: { $0.contentOffset.y + $0.contentInsets.top }) { old, new in
                if new < 24 { compact.wrappedValue = false }
                else if new > old + 6 { compact.wrappedValue = true }
                else if new < old - 6 { compact.wrappedValue = false }
            }
        } else {
            self
        }
    }
}

/// The ⋯ menu: undo, redo and Settings.
struct BudgetActionsMenu: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    @State private var page: Page?

    enum Page: String, Identifiable {
        case settings, sync, assistant
        var id: String { rawValue }
    }

    var body: some View {
        Menu {
            Button { store.undo() } label: { Label("Undo", systemImage: "arrow.uturn.backward") }
                .keyboardShortcut("z", modifiers: .command)
                .disabled(!store.undoAvailable || store.busy)
            Button { store.redo() } label: { Label("Redo", systemImage: "arrow.uturn.forward") }
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .disabled(!store.redoAvailable || store.busy)
            Divider()
            Button { page = .assistant } label: { Label("Assistant", systemImage: "sparkles") }
            if sync.signedIn && sync.unlocked {
                Button { Task { try? await sync.syncNow() } } label: { Label("Sync now", systemImage: "arrow.triangle.2.circlepath") }
                    .disabled(sync.syncing)
            }
            Button { page = .sync } label: { Label("Account & sync", systemImage: "icloud") }
            Button { page = .settings } label: { Label("Settings & data", systemImage: "gearshape") }
        } label: {
            Image(systemName: "ellipsis.circle")
                .overlay(alignment: .topTrailing) {
                    // A budget waiting for a sync decision shows here on every tab.
                    if sync.conflicts > 0 { Circle().fill(Color.urWarning).frame(width: 8, height: 8).offset(x: 2, y: -2) }
                }
        }
        .accessibilityLabel(sync.conflicts > 0 ? Text("More actions, a synced budget needs review") : Text("More actions"))
        .sheet(item: $page) { page in
            switch page {
            case .assistant:
                AssistantView()
            case .settings, .sync:
                NavigationStack {
                    Group {
                        if page == .settings { SettingsView() } else { AccountSyncView() }
                    }
                    .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { self.page = nil } } }
                }
            }
        }
    }
}
