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
                // The iPhone Duo's fold, for every screen and sheet (UI/Fold.swift).
                .modifier(FoldReader())
                // No scroll bars anywhere, as on Android. A list that's a column down a wide window,
                // or a pane beside the bar an unfolded Duo stands at its side, put its bar over
                // its own figures rather than at the window's edge.
                .scrollIndicators(.hidden)
                // Text a step under iOS's own size, the scale Android's screens use (UI/Theme.swift).
                .onAppear { AppTypeScale.apply() }
                .onReceive(NotificationCenter.default.publisher(for: UIContentSizeCategory.didChangeNotification)) { _ in AppTypeScale.apply() }
                .onReceive(NotificationCenter.default.publisher(for: UIScene.didActivateNotification)) { _ in AppTypeScale.apply() }
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
    @State private var showingLibrary = false
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
                VStack {
                    Button("Budget library") { showingLibrary = true }.padding()
                    WelcomeView()
                }
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
                AssistantController.shared.enterForeground()
            case .background:
                // A reply still being written keeps its events coming for the time iOS grants
                // it; the assistant stops them itself when the reply ends or that time is up.
                if !AssistantController.shared.enterBackground() { HostEvents.shared.stop() }
                sync.enterBackground()
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
        #if DEBUG
        .task(id: store.loading) { if !store.loading { await SyncQA.run(store: store, sync: sync) } }
        #endif
        .sheet(isPresented: $showingLibrary) { NavigationStack { LibraryView() } }
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
    @Environment(BudgetStore.self) private var store
    @State private var tab = LaunchRoute.initialTab
    /// What the Transactions tab opens on. A card of the assistant can ask to see some
    /// transactions, and the tab then starts again on those alone; `registerShown` counts the
    /// times, so asking for the same ones again shows them again.
    @State private var register = RegisterTarget()
    @State private var registerShown = 0
    /// The tabs' width, and the whole window's (which the assistant can share with them).
    @State private var width: CGFloat = 0
    @State private var window = WindowSpan()
    /// Where the tabs themselves lie, for the fold's dock to end at the fold.
    @State private var tabsSpan = WindowSpan()
    @Environment(\.fold) private var fold
    /// A screen has its foot in use (a selection's actions): the fold's dock steps aside, as the tab bar does.
    @State private var tabsHidden = false

    private static let tabs: [(tab: AppTab, title: String, symbol: String)] = [
        (.budget, String(localized: "Budget"), "square.grid.2x2"),
        (.transactions, String(localized: "Transactions"), "arrow.left.arrow.right"),
        (.accounts, String(localized: "Accounts"), "building.columns"),
        (.reports, String(localized: "Reports"), "chart.bar.xaxis"),
        (.more, String(localized: "More"), "ellipsis.circle"),
    ]

    /// The page the assistant is told the app is on while it's open beside it.
    private var place: String {
        switch tab {
        case .budget: return "budget"
        case .transactions: return "transactions"
        case .accounts: return "accounts"
        case .reports: return "reports"
        case .more: return "assistant"
        }
    }

    var body: some View {
        let actions = AppActions.shared
        // The assistant's side: the far side of a fold, or a share of a wide window.
        let side = WideLayout.assistantSide(span: window, fold: fold)
        let beside = actions.panel != nil && side != nil
        return HStack(spacing: 0) {
            tabs
                // Beside the assistant the app is a screen of its own: it sees no fold, lays out
                // by the width it has, and keeps its tab bar.
                .environment(\.fold, beside ? FoldState() : fold)
                .measuringSpan($tabsSpan)
                .overlay(alignment: .bottomLeading) {
                    // On the start side of a fold the tabs stand in a dock that keeps to that side.
                    if !beside, !tabsHidden, fold.docksTabs, let hinge = WideLayout.hinge(across: tabsSpan, fold: fold) {
                        FoldDock(tabs: Self.tabs, selection: $tab).frame(width: hinge.before).ignoresSafeArea(.keyboard)
                    }
                }
                .onPreferenceChange(TabsHiddenKey.self) { tabsHidden = $0 }
            // On a wide window, or across a fold, the assistant sits beside the app, and both stay in use.
            if let panel = actions.panel, let side {
                if side.gap > 0 { FoldGap(width: side.gap) } else { Divider().ignoresSafeArea() }
                AssistantView(request: panel.request, beside: true, place: place)
                    .id(panel.id)
                    .frame(width: side.width)
                    .transition(.move(edge: .trailing))
            }
        }
        .animation(.snappy(duration: 0.25), value: actions.panel?.id)
        .measuringSpan($window)
        .onChange(of: side != nil, initial: true) { _, fits in
            actions.besideFits = fits
            // The window narrowed (or the phone folded) with the assistant beside the app: it
            // closes, its conversation kept.
            if !fits { actions.panel = nil }
        }
    }

    private var tabs: some View {
        tabView
            .measuringWidth($width)
            .environment(\.windowWidth, width)
            .background {
            // Hidden buttons carry ⌘1–⌘5, as the desktop's sidebar shortcuts do.
                ForEach(Array(Self.tabs.enumerated()), id: \.offset) { index, destination in
                    Button { tab = destination.tab } label: { Text(verbatim: "") }
                        .keyboardShortcut(KeyEquivalent(Character(String(index + 1))), modifiers: .command)
                        .hidden()
                }
            }
            // A card of the assistant asked to see something, as its sheet closed.
            .onChange(of: AppActions.shared.destination) { _, destination in go(destination) }
            // The assistant, asked for by a message over the tabs (an import's "Categorize with
            // the assistant"), which no screen of its own could present.
            .assistant(Binding(get: { AppActions.shared.asking }, set: { AppActions.shared.asking = $0 }))
    }

    /// Goes where a card of the assistant asked: the Transactions tab showing its transactions
    /// alone, or else the Budget tab on its month.
    private func go(_ destination: AppActions.Destination?) {
        guard let destination else { return }
        AppActions.shared.destination = nil
        if !destination.transactions.isEmpty {
            register = RegisterTarget(only: destination.transactions)
            registerShown += 1
            tab = .transactions
        } else {
            if Dates.isMonth(destination.month) { store.selectMonth(destination.month) }
            tab = .budget
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
        // Accounts and More have no detail of their own: on a fold they keep to its start side.
        Group {
            switch tab {
            case .budget: BudgetView()
            case .transactions: RegisterTab(target: register).id(registerShown)
            case .accounts: FoldPage { AccountsView() }
            case .reports: ReportsView()
            case .more: FoldPage { MoreView() }
            }
        }
        .tabBarOffFold()
    }
}

/// A short message above the tab bar, with Undo for the change it describes.
struct ToastView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.fold) private var fold
    @State private var span = WindowSpan()

    var body: some View {
        ZStack {
            if let toast = store.toast {
                VStack(alignment: .leading, spacing: 10) {
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
                    // Something more the message offers, on a line of its own: its words are
                    // longer than there's room for beside Undo.
                    if let offer = toast.offer {
                        Button(offer.label) {
                            store.toast = nil
                            offer.run()
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
                    // An offer (Add rule, Apply, the assistant) waits longest, as the desktop's does.
                    let seconds = toast.action != nil || toast.offer != nil ? 10 : toast.undo ? 8 : 4
                    try? await Task.sleep(for: .seconds(UIAccessibility.isVoiceOverRunning && offer != nil ? 14 : seconds))
                    if store.toast?.id == toast.id { store.toast = nil }
                }
            }
        }
        // On a fold a message keeps to the start side, over the dock, clear of what's open on the other.
        .frame(width: WideLayout.hinge(across: span, fold: fold)?.before)
        .frame(maxWidth: .infinity, alignment: .leading)
        .measuringSpan($span)
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
    /// On a wide window's Transactions tab the new transaction opens beside the register.
    @Environment(\.editorPane) private var pane
    /// On a fold the tabs stand in a dock at this screen's foot, which the button floats over.
    @Environment(\.dockRoom) private var dockRoom

    var body: some View {
        Button {
            let request = TransactionEditorRequest(account: account)
            if let pane { pane.open(request) } else { editing = request }
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
        .padding(.bottom, 12 + dockRoom)
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

/// The ⋯ menu: undo, redo, the budget library and Settings.
/// The assistant, a tap away on every tab's bar, as on Android. It's told the page it was asked from.
struct AssistantBarButton: View {
    var page = "assistant"
    var account = ""
    @State private var asking: AssistantRequest?

    var body: some View {
        Button { asking = AssistantRequest(page: page, account: account) } label: { Image(systemName: "sparkles") }
            .accessibilityLabel("Assistant")
            .assistant($asking)
    }
}

struct BudgetActionsMenu: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    @State private var page: Page?

    enum Page: String, Identifiable {
        case library, settings, sync, assistant
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
            Button { page = .library } label: { Label("Budgets", systemImage: "books.vertical") }
            // Also a button on the bar; here too, for a bar too crowded to show it (an iPad's, with its tabs).
            Button { if !AppActions.shared.openBeside(nil) { page = .assistant } } label: { Label("Assistant", systemImage: "sparkles") }
            if sync.signedIn && sync.unlocked {
                Button { Task { try? await sync.syncNow() } } label: { Label("Sync now", systemImage: "arrow.triangle.2.circlepath") }
                    .disabled(sync.syncing)
            }
            Button { page = .sync } label: { Label(sync.signedIn ? sync.statusLabel : String(localized: "Account & sync"), systemImage: sync.statusSymbol) }
            Button { page = .settings } label: { Label("Settings & data", systemImage: "gearshape") }
        } label: {
            Label(sync.signedIn ? sync.statusLabel : String(localized: "More actions"), systemImage: "ellipsis.circle")
                .font(.caption)
                .overlay(alignment: .topTrailing) {
                    // Sync that's paused or waiting for a decision shows here on every tab.
                    if sync.needsYou { Circle().fill(Color.urWarning).frame(width: 8, height: 8).offset(x: 2, y: -2) }
                }
        }
        .accessibilityLabel(sync.conflicts > 0 ? Text("More actions, a synced budget needs review") : sync.needsYou ? Text("More actions, sync is paused") : Text("More actions"))
        .sheet(item: $page) { page in
            switch page {
            case .assistant:
                AssistantView()
            case .library, .settings, .sync:
                NavigationStack {
                    Group {
                        switch page {
                        case .library: LibraryView()
                        case .settings: SettingsView()
                        default: AccountSyncView()
                        }
                    }
                    .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { self.page = nil } } }
                }
            }
        }
    }
}
