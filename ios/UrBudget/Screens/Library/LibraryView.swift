import SwiftUI
import UniformTypeIdentifiers

/// The budgets on this device and the open one's backups, as Android's budget library: open, add,
/// fresh-start or delete a budget; back up, restore and prune backups; recover unsaved changes.
struct LibraryView: View {
    @Environment(BudgetStore.self) private var store
    @State private var budgets: [JSON] = []
    @State private var backups: [LibraryBackup] = []
    @State private var keep = 0
    /// `read_recovery`'s reply, { budget, matches }, when a save didn't finish.
    @State private var recovery: JSON?
    @State private var loaded = false
    @State private var working = false
    @State private var loadError: String?
    @State private var creating = false
    @State private var freshStarting = false
    @State private var deleting: JSON?
    @State private var restoring: LibraryBackup?
    /// The revision shown when Restore was chosen: a newer change stops the restore, as on Android.
    @State private var restoreToken = ""
    @State private var pruning = false
    @State private var discarding = false
    @State private var export: LibraryExport?
    @State private var exporting = false

    init() {}

    var body: some View {
        let busy = working || store.busy
        let name = store.budget?.text("name")
        let excess = LibraryBackup.excess(backups.count, keep: keep)
        let pruneTitle = String(localized: "Remove \(excess) older backups?", locale: .app)
        // One small function per section: Xcode 26's type checker gives up on the whole list at once.
        List {
            budgetsSection(busy: busy)
            freshStartSection(busy: busy)
            if let recovery { recoverySection(recovery, busy: busy) }
            backupSettingsSection(busy: busy)
            backupsSection(busy: busy, name: name, excess: excess)
            if let loadError { Section { FormErrorView(message: loadError) } }
        }
        .groupedList()
        .navigationTitle("Budget library")
        .toolbar {
            if busy {
                ToolbarItem(placement: .trailingBar) { ProgressView().accessibilityLabel("Working") }
            }
        }
        .task(id: store.version) { await reload() }
        .refreshable { await reload() }
        .sheet(isPresented: $creating) { NewBudgetForm() }
        .sheet(isPresented: $freshStarting) {
            FreshStartForm(defaultName: String(localized: "\(name ?? "") · Fresh start")) { freshStart(name: $0, asOf: $1) }
        }
        .confirmationDialog(deleteTitle, isPresented: present($deleting), titleVisibility: .visible,
                            presenting: deleting) { entry in
            Button("Delete local budget", role: .destructive) { delete(entry) }
        } message: { _ in
            Text("The budget and its files are removed from this device. Export a backup first if you want to keep them. Cloud copies are managed separately under Account & sync.")
        }
        .confirmationDialog("Restore this backup?", isPresented: present($restoring), titleVisibility: .visible, presenting: restoring) { backup in
            Button("Restore", role: .destructive) { restore(backup) }
        } message: { backup in
            Text("This replaces the open budget with the backup from \(backup.title). The current budget is backed up before replacement. A newer change will stop this restore for review.")
        }
        .confirmationDialog(pruneTitle, isPresented: $pruning, titleVisibility: .visible) {
            Button("Remove backups", role: .destructive) { prune() }
        } message: {
            Text("Only the newest \(keep) are kept. Removed backups can't be recovered.")
        }
        .confirmationDialog("Keep the saved budget?", isPresented: $discarding, titleVisibility: .visible) {
            Button("Discard unsaved changes", role: .destructive) { discardRecovery() }
        } message: {
            Text("The changes that weren't saved are deleted. Export them first if you want to keep a copy.")
        }
        .fileExporter(isPresented: $exporting, document: export?.file, contentType: export?.type ?? .json, defaultFilename: export?.name) { result in
            switch result {
            case .success: store.showToast(export?.done ?? String(localized: "Exported"))
            case .failure(let failure): store.error = BudgetStore.readable(failure)
            }
        }
    }

    /// "Delete Household?"
    private var deleteTitle: Text {
        if let name = deleting?.text("name") { return Text("Delete \(name)?") }
        return Text("Delete budget?")
    }

    // MARK: Sections

    private func budgetsSection(busy: Bool) -> some View {
        Section {
            ForEach(budgets, id: \.idText) { (entry: JSON) in budgetRow(entry) }
            if !loaded && budgets.isEmpty { ProgressView().frame(maxWidth: .infinity) }
            Button { creating = true } label: { Label("New budget", systemImage: "plus") }
                .disabled(busy)
        } header: {
            Text("Budgets")
        } footer: {
            Text("Each has its own history, bank links, assistant chats and backups. Tap the open budget for its name, currency and formats. Swipe another to delete it.")
        }
    }

    private func freshStartSection(busy: Bool) -> some View {
        Section {
            Button { freshStarting = true } label: {
                LibraryRowLabel(title: "Make a fresh start", detail: "Carry balances, categories and targets into a new budget; this one stays in the library", symbol: "sparkles")
            }
            .disabled(busy || store.budget == nil)
        } header: {
            Text("Start a new plan")
        }
    }

    private func backupSettingsSection(busy: Bool) -> some View {
        let retention = Binding<Int>(get: { keep }, set: { setRetention($0) })
        return Section {
            Button { backUp() } label: {
                LibraryRowLabel(title: "Back up now", detail: "A complete copy to save or share", symbol: "externaldrive.badge.plus")
            }
            .disabled(busy || store.budget == nil)
            Picker("Backups to keep", selection: retention) {
                ForEach(LibrarySettings.retentionChoices, id: \.keep) { (choice: (keep: Int, label: String)) in
                    Text(choice.label).tag(choice.keep)
                }
            }
            .disabled(busy || !loaded)
        } header: {
            Text("Backups & recovery")
        } footer: {
            Text("Applies to each budget's automatic and manual backups. Older copies are removed when a new backup is made.")
        }
    }

    private func backupsSection(busy: Bool, name: String?, excess: Int) -> some View {
        let removeTitle = String(localized: "Remove \(excess) older backups", locale: .app)
        return Section {
            if loaded && backups.isEmpty {
                Text("No backups yet.").foregroundStyle(Color.secondary)
            }
            ForEach(backups) { (backup: LibraryBackup) in backupRow(backup, busy: busy) }
            if excess > 0 {
                Button(role: .destructive) { pruning = true } label: {
                    Label(removeTitle, systemImage: "trash")
                }
                .disabled(busy)
            }
        } header: {
            if let name { Text("Backups of \(name)") } else { Text("Backups of this budget") }
        } footer: {
            Text("Safety copies are made before a restore, an import or sync replaces this budget. Restoring one backs up the current budget first.")
        }
    }

    // MARK: Rows

    @ViewBuilder
    private func budgetRow(_ entry: JSON) -> some View {
        if entry.flag("current") {
            NavigationLink { SettingsView() } label: { LibraryBudgetLabel(entry: entry) }
                .accessibilityHint("Shows the budget's name, currency and formats")
        } else {
            Button { open(entry) } label: { LibraryBudgetLabel(entry: entry) }
                .swipeActions {
                    Button(role: .destructive) { deleting = entry } label: { Label("Delete", systemImage: "trash") }
                }
                .contextMenu {
                    Button { open(entry) } label: { Label("Open", systemImage: "folder") }
                        .disabled(entry.flag("missing"))
                    Button(role: .destructive) { deleting = entry } label: { Label("Delete local budget", systemImage: "trash") }
                }
                .accessibilityHint(entry.flag("missing") ? Text("Its file is missing. Swipe to delete it.") : Text("Opens this budget"))
        }
    }

    private func backupRow(_ backup: LibraryBackup, busy: Bool) -> some View {
        HStack(spacing: 12) {
            Image(systemName: backup.portable ? "doc.text" : "clock.arrow.circlepath")
                .foregroundStyle(Color.secondary)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(backup.title)
                Text(backup.detail).font(.footnote).foregroundStyle(Color.secondary)
            }
            .accessibilityElement(children: .combine)
            Spacer(minLength: 8)
            Button("Restore") { confirmRestore(backup) }
                .fontWeight(.semibold)
                .buttonStyle(.borderless)
                .disabled(busy || store.revision.isEmpty)
                .accessibilityLabel("Restore the backup from \(backup.title)")
        }
        .contextMenu {
            Button { confirmRestore(backup) } label: { Label("Restore", systemImage: "arrow.counterclockwise") }
                .disabled(busy || store.revision.isEmpty)
            if backup.portable {
                ShareLink(item: URL(fileURLWithPath: backup.path)) { Label("Share", systemImage: "square.and.arrow.up") }
            }
        }
    }

    private func recoverySection(_ recovery: JSON, busy: Bool) -> some View {
        let matches = recovery["matches"].bool
        return Section {
            Notice(text: matches
                   ? String(localized: "Some changes weren't saved. Restore them to continue where you left off, or keep the saved budget.")
                   : String(localized: "Some changes weren't saved, and the budget has changed since. Export them to compare, or keep the saved budget."),
                   tone: "attention", icon: "exclamationmark.triangle.fill")
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
            if matches {
                Button { restoreRecovery(recovery) } label: { Label("Restore changes", systemImage: "arrow.uturn.backward.circle") }
                    .disabled(busy)
            }
            Button { exportRecovery(recovery) } label: { Label("Export them", systemImage: "square.and.arrow.up") }
                .disabled(busy)
            Button(role: .destructive) { discarding = true } label: { Label("Keep saved budget", systemImage: "xmark.circle") }
                .disabled(busy)
        } header: {
            Text("Unsaved changes")
        }
    }

    // MARK: Actions

    private func reload() async {
        do {
            budgets = try await store.listBudgets()
            if store.budget != nil {
                backups = try await store.service("list_backups").array.filter(\.isObject).map(LibraryBackup.init)
            } else {
                backups = []
            }
            keep = Int(try await store.service("backup_retention").int)
            loadError = nil
        } catch {
            loadError = BudgetStore.readable(error)
        }
        // A recovery file that can't be read is left alone, as the desktop does after reporting it.
        if store.budget != nil, let value = try? await store.service("read_recovery"), value["budget"].isObject {
            recovery = value
        } else {
            recovery = nil
        }
        loaded = true
    }

    /// Runs one library action at a time, reporting failure in the app's alert, then reloads the lists.
    private func run(_ success: String? = nil, _ work: @escaping @MainActor () async throws -> Void) {
        guard !working else { return }
        working = true
        Task {
            do {
                try await work()
                if let success { store.showToast(success) }
            } catch {
                store.error = BudgetStore.readable(error)
            }
            working = false
            await reload()
        }
    }

    private func open(_ entry: JSON) {
        guard !store.busy, !entry.flag("missing") else { return }
        let id = entry.text("id"), name = entry.text("name")
        run(String(localized: "Opened \(name)")) {
            try await store.perform { repository in ((), try await repository.open(id: id)) }
        }
    }

    private func delete(_ entry: JSON) {
        let id = entry.text("id"), name = entry.text("name")
        run(String(localized: "Deleted \(name)")) { budgets = try await store.deleteBudget(id: id) }
    }

    /// The engine carries the open budget's accounts, categories and pending activity into a new one, which then opens.
    private func freshStart(name: String, asOf: String) {
        let params: JSON = ["name": .string(name.trimmingCharacters(in: .whitespaces)), "asOf": .string(asOf)]
        run(String(localized: "Fresh start created")) {
            try await store.perform { repository in
                let (id, _) = try await repository.commitService("fresh_start", params)
                return ((), try await repository.open(id: id.string))
            }
        }
    }

    private func backUp() {
        run(String(localized: "Backup created")) { _ = try await store.service("backup_database") }
    }

    private func setRetention(_ value: Int) {
        let previous = keep
        keep = value
        run {
            do {
                _ = try await store.service("set_backup_retention", ["keep": .int(Int64(value))])
            } catch {
                keep = previous
                throw error
            }
        }
    }

    private func prune() {
        let count = keep
        run {
            let removed = try await store.service("prune_backups", ["keep": .int(Int64(count))]).int
            store.showToast(String(localized: "Removed \(removed) backups", locale: .app))
        }
    }

    private func confirmRestore(_ backup: LibraryBackup) {
        restoreToken = store.revision
        restoring = backup
    }

    /// Restores a backup file in the edit queue with the revision that was showing, so a newer change stops it.
    private func restore(_ backup: LibraryBackup) {
        let params: JSON = ["path": .string(backup.path), "expectedToken": .string(restoreToken)]
        run(String(localized: "Backup restored")) { _ = try await store.service("restore_backup_file", params, changesBudget: true) }
    }

    /// Puts the unsaved changes back as the draft (Rust restores the fields it keeps out of it) and commits them.
    private func restoreRecovery(_ recovery: JSON) {
        let draft = recovery["budget"]
        let store = store
        run(String(localized: "Unsaved changes restored")) {
            do {
                try await store.perform { repository in
                    _ = try await store.service("replace_draft", ["budget": draft])
                    let (_, snapshot) = try await repository.commitService("commit_budget", [:])
                    return ((), snapshot)
                }
            } catch {
                // Never leave a half-applied draft behind the next edit.
                store.reload()
                throw error
            }
        }
    }

    private func discardRecovery() {
        run(String(localized: "Unsaved changes discarded")) {
            _ = try await store.service("discard_recovery")
            recovery = nil
        }
    }

    private func exportRecovery(_ recovery: JSON) {
        let budget = recovery["budget"]
        let data = (try? JSONSerialization.data(withJSONObject: budget.foundation, options: [.prettyPrinted, .sortedKeys])) ?? budget.data()
        export = LibraryExport(file: TextFile(data: data), type: .json, name: "UrBudget-unsaved-changes-\(Dates.today).json",
                               done: String(localized: "Unsaved changes exported"))
        exporting = true
    }

    private func present<Value>(_ value: Binding<Value?>) -> Binding<Bool> {
        Binding(get: { value.wrappedValue != nil }, set: { if !$0 { value.wrappedValue = nil } })
    }
}

/// A budget in the library: its initials, name, currency and when it was last opened.
private struct LibraryBudgetLabel: View {
    var entry: JSON
    @ScaledMetric(relativeTo: .body) private var badge: CGFloat = 40

    var body: some View {
        let current = entry.flag("current")
        let missing = entry.flag("missing")
        HStack(spacing: 12) {
            Text(initials(entry.text("name")))
                .font(.subheadline.weight(.bold))
                .foregroundStyle(current ? Color.white : Color.secondary)
                .frame(width: badge, height: badge)
                .background(current ? Color.urAccent : Color.urZero, in: RoundedRectangle(cornerRadius: badge / 4, style: .continuous))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.text("name")).foregroundStyle(Color.primary)
                Text(subtitle).font(.footnote).foregroundStyle(missing ? Color.urNegative : Color.secondary)
            }
            Spacer(minLength: 8)
            if current {
                Text(String(localized: "state.open", defaultValue: "Open", comment: "Something that is open now (a state, not a button)."))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.urAccent)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(Color.urAccent.opacity(0.14), in: Capsule())
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
    }

    private var subtitle: String {
        let status: String
        if entry.flag("current") {
            status = String(localized: "Open now")
        } else if entry.flag("missing") {
            status = String(localized: "File missing")
        } else if let opened = LibraryDates.instant(entry.text("lastOpened")) {
            let when = opened.formatted(.relative(presentation: .named).locale(.app))
            status = String(localized: "Opened \(when)", comment: "When a budget was last opened, as in “Opened 2 hours ago”.")
        } else {
            status = String(localized: "On this device")
        }
        return [entry.text("currency"), status].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    private func initials(_ name: String) -> String {
        let letters = name.filter { $0.isLetter || $0.isNumber }.prefix(2).uppercased()
        return letters.isEmpty ? "UB" : letters
    }
}

/// A row's title over a line about it, with an icon.
private struct LibraryRowLabel: View {
    var title: LocalizedStringKey
    var detail: LocalizedStringKey
    var symbol: String

    var body: some View {
        Label {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).foregroundStyle(Color.primary)
                Text(detail).font(.footnote).foregroundStyle(Color.secondary)
            }
        } icon: {
            Image(systemName: symbol)
        }
    }
}

private struct NewBudgetForm: View {
    @Environment(BudgetStore.self) private var store
    @State private var name = String(localized: "My budget")
    @State private var currency = Locale.current.currency?.identifier ?? "SAR"
    @State private var sample = false
    @State private var started = false

    var body: some View {
        FormSheet(title: String(localized: "New budget"), confirm: String(localized: "Create"),
                  enabled: LibrarySettings.validName(name) && LibraryCurrencies.valid(currency)) {
            store.createBudget(name: name, currency: currency, sample: sample)
        } content: {
            Section {
                TextField("Budget name", text: $name).wordsCapitalized()
                LibraryCurrencyPicker(selection: $currency)
                Toggle("Start with sample money", isOn: $sample)
            } footer: {
                Text("The new budget opens with its own accounts, categories and history. This one stays in the library.")
            }
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            guard !started else { return }
            started = true
            if store.budget != nil { currency = store.currency }
        }
    }
}

private struct FreshStartForm: View {
    var defaultName: String
    var onCreate: (String, String) -> Void
    @State private var name = ""
    @State private var asOf = Dates.today
    @State private var started = false

    var body: some View {
        FormSheet(title: String(localized: "Make a fresh start"), confirm: String(localized: "Create"), enabled: LibrarySettings.validName(name)) {
            onCreate(name, asOf)
        } content: {
            Section {
                TextField("New budget name", text: $name).wordsCapitalized()
                DayPicker(title: String(localized: "Start date"), iso: $asOf)
            } footer: {
                Text("The shared engine carries account balances, categories, targets and pending activity into a separate budget. Cleared transactions up to the start date become opening balances. Your original remains in this library.")
            }
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            guard !started else { return }
            started = true
            name = defaultName
        }
    }
}
