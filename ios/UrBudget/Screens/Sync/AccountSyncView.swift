import SwiftUI

/// Account & sync, as Android's CloudScreen: sign in with an emailed code, and end-to-end
/// encrypted sync of budgets between this iPhone, desktop and Android.
struct AccountSyncView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    @Environment(AppStoreSubscription.self) private var appStore
    @AppStorage(SyncCoordinator.automaticKey) private var automatic = true

    @State private var overview: JSON?
    @State private var busy = false
    @State private var error: String?
    @State private var notice: String?
    @State private var email = ""
    @State private var code = ""
    @State private var sent = false
    @State private var passphrase: PassphraseRequest?
    @State private var conflict: IdentifiedJSON?
    @State private var removing: JSON?
    @State private var confirmSignOut = false
    @State private var deletingAccount = false
    @FocusState private var typingCode: Bool

    private var status: JSON { sync.status ?? [:] }
    private var signedIn: String { status.text("email") }

    var body: some View {
        Form {
            Section {
                Text("Your budgets stay on this device. Sync is optional, and budgets are encrypted before they leave it.")
                    .font(.subheadline).foregroundStyle(Color.secondary)
                    .listRowBackground(Color.clear)
            }
            if let notice { Notice(text: notice, tone: "info").listRowInsets(EdgeInsets()).listRowBackground(Color.clear) }
            if let error { FormErrorView(message: error) }
            accountSection
            if !signedIn.isEmpty {
                SyncSubscriptionSection(overview: overview, reload: loadOverview)
                syncSection
                budgetsSection
                Section {
                    Button("Reset forgotten sync passphrase") { passphrase = PassphraseRequest(kind: .reset) }
                    Button("Delete account and cloud data", role: .destructive) { deletingAccount = true }
                }
                .disabled(busy)
            }
            Section {
                Button("Privacy & your choices") { run { _ = try await store.service("account_open", ["next": "/privacy/app/"]) } }
                    .disabled(busy)
            }
        }
        .navigationTitle("Account & sync")
        .overlay(alignment: .top) { if busy { ProgressView().padding(8) } }
        .task {
            // Status follows sign-ins and sync attempts while the screen is open.
            while !Task.isCancelled {
                await sync.refreshStatus()
                try? await Task.sleep(for: .seconds(5))
            }
        }
        .task(id: signedIn) {
            guard !signedIn.isEmpty else { overview = nil; return }
            // An App Store purchase made while signed out or offline reaches the account now.
            await appStore.deliverUnfinished()
            await loadOverview()
        }
        // A renewal, refund or approved purchase arrived while the screen is open.
        .onChange(of: appStore.deliveries) { Task { await loadOverview() } }
        .sheet(item: $passphrase) { request in
            // A wrong passphrase stays in the sheet; the first sync after it shows here.
            PassphraseSheet(request: request) { phrase in
                _ = try await store.service(request.kind == .reset ? "sync_reset" : "sync_setup", ["passphrase": .string(phrase)])
                run { try await sync.syncNow() }
            }
        }
        .sheet(item: $conflict) { entry in
            SyncConflictSheet(entry: entry.value) { keep, choices in
                _ = try await store.service("sync_resolve", ["id": .string(entry.value.text("id")), "keep": .string(keep), "choices": choices, "review": entry.value["review"]])
                store.reload()
                await sync.refreshStatus()
            }
        }
        .sheet(isPresented: $deletingAccount) { DeleteAccountSheet(email: signedIn, appStore: overview?["subscription"].text("provider") == "apple") { confirm in
            _ = try await store.service("account_delete", ["confirm": .string(confirm)])
            await sync.refreshStatus()
        } }
        .confirmationDialog("Sign out of UrBudget?", isPresented: $confirmSignOut, titleVisibility: .visible) {
            Button("Sign out", role: .destructive) { run { _ = try await store.service("account_sign_out") } }
        } message: {
            Text("Sync stops on this iPhone and it forgets the sync key. Your budgets here stay as they are.")
        }
        .confirmationDialog(removeTitle, isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Remove cloud copy", role: .destructive) {
                guard let id = removing?.text("id") else { return }
                run {
                    _ = try await store.service("sync_remove", ["id": .string(id)])
                    try await sync.syncNow()
                }
            }
        } message: {
            Text("This stops syncing it and deletes its cloud copy. Copies on your devices stay.")
        }
    }

    private var removeTitle: Text {
        guard let removing else { return Text("Remove the cloud copy of this budget?") }
        return Text("Remove the cloud copy of \(removing.text("name"))?")
    }

    // MARK: Sections

    @ViewBuilder private var accountSection: some View {
        Section("Account") {
            if signedIn.isEmpty {
                Text("Sign in with your email to sync your budgets. No password needed.")
                    .font(.subheadline).foregroundStyle(Color.secondary)
                TextField("Email address", text: $email)
                    .textContentType(.emailAddress)
                    .emailKeyboard()
                    .autocorrectionDisabled()
                    .onChange(of: email) { sent = false; code = "" }
                if sent {
                    TextField("Code from the email", text: $code)
                        .textContentType(.oneTimeCode)
                        .numberKeyboard()
                        .focused($typingCode)
                    Button("Sign in") {
                        run {
                            _ = try await store.service("account_verify", ["email": .string(email.trimmingCharacters(in: .whitespaces)),
                                                                           "code": .string(code.trimmingCharacters(in: .whitespaces))])
                            code = ""
                            await sync.syncAutomatically()
                        }
                    }
                    .fontWeight(.semibold)
                    .disabled(busy || code.trimmingCharacters(in: .whitespaces).isEmpty)
                    Button("Send a new code", action: sendCode).disabled(busy || !email.contains("@"))
                } else {
                    Button("Send sign-in code", action: sendCode).fontWeight(.semibold).disabled(busy || !email.contains("@"))
                }
            } else {
                Label {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(signedIn)
                        Text(status.text("credentialStore").isEmpty ? "Session stored securely on this device" : "Session stored in \(status.text("credentialStore"))")
                            .font(.footnote).foregroundStyle(Color.secondary)
                    }
                } icon: {
                    Image(systemName: "person.crop.circle.fill").foregroundStyle(Color.urAccent).font(.title2)
                }
                Button("Sign out on this device", role: .destructive) { confirmSignOut = true }
                    .disabled(busy)
            }
        }
    }

    /// Optional subscription details: a slow request mustn't hold up the local controls.
    private func loadOverview() async {
        guard !signedIn.isEmpty else { return }
        overview = try? await store.service("account_overview")
    }

    private func sendCode() {
        run {
            _ = try await store.service("account_start", ["email": .string(email.trimmingCharacters(in: .whitespaces))])
            sent = true
            notice = String(localized: "Check your email for a sign-in code. It may land in your Junk folder.")
            typingCode = true
        }
    }

    @ViewBuilder private var syncSection: some View {
        let report = sync.lastReport
        Section {
            Toggle(isOn: $automatic) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Sync automatically")
                    Text("After changes and every minute while the app is open").font(.footnote).foregroundStyle(Color.secondary)
                }
            }
            if sync.unlocked {
                Button {
                    run { try await sync.syncNow() }
                } label: {
                    HStack {
                        Label("Sync now", systemImage: "arrow.triangle.2.circlepath")
                        if sync.syncing { Spacer(); ProgressView() }
                    }
                }
                .disabled(busy || sync.syncing)
                Button { run { _ = try await store.service("sync_lock") } } label: { Label("Lock sync on this device", systemImage: "lock") }
                    .disabled(busy)
            } else {
                Button {
                    run {
                        let exists = try await store.service("sync_has_key").bool
                        passphrase = PassphraseRequest(kind: exists ? .unlock : .setup)
                    }
                } label: { Label("Set up or unlock sync", systemImage: "key") }
                .disabled(busy)
            }
        } header: {
            Text("Encrypted sync")
        } footer: {
            if let report, !(report.text("message").isEmpty && report.text("at").isEmpty) {
                Text(reportLine(report))
            } else {
                Text("Only your sync passphrase opens your budgets. UrBudget can't read or recover them.")
            }
        }
    }

    @ViewBuilder private var budgetsSection: some View {
        let budgets = sync.lastReport?.rows("budgets") ?? []
        if !budgets.isEmpty {
            Section("Budgets") {
                NavigationLink("Budget library") { LibraryView() }
                if sync.status?["sync"]["protocol"].int != 2 {
                    Text("Automatic budget discovery is not enabled for this account yet. Existing budgets keep syncing; use the controls below to add or download budgets.").font(.footnote).foregroundStyle(.secondary)
                }
                ForEach(budgets, id: \.idText) { entry in
                    let state = entry.text("state")
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Text(entry.text("name")).fontWeight(.semibold)
                            Spacer()
                            Text(syncStateLabel(state))
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(state == "conflict" ? Color.urWarning : state == "error" ? Color.urNegative : .secondary)
                                .padding(.horizontal, 8).padding(.vertical, 3)
                                .background((state == "conflict" ? Color.urWarning : Color.secondary).opacity(0.14), in: Capsule())
                        }
                        if !entry.text("message").isEmpty { Text(entry.text("message")).font(.subheadline).foregroundStyle(Color.secondary) }
                        ForEach(entry["notes"].array.map(\.string), id: \.self) { Text($0).font(.footnote).foregroundStyle(Color.secondary) }
                        HStack(spacing: 10) {
                            switch state {
                            case "cloud_only" where sync.status?["sync"]["protocol"].int != 2:
                                Button("Download and open") {
                                    run {
                                        let id = try await store.service("sync_download", ["id": .string(entry.text("id"))]).string
                                        store.openBudget(id: id)
                                    }
                                }
                                .buttonStyle(.bordered)
                            case "local_only" where sync.status?["sync"]["protocol"].int != 2:
                                Button("Start syncing") {
                                    run {
                                        _ = try await store.service("sync_track", ["ids": JSON([entry.text("id")])])
                                        try await sync.syncNow()
                                    }
                                }
                                .buttonStyle(.bordered)
                            case "conflict":
                                Button("Review differences") { conflict = IdentifiedJSON(value: entry) }
                                    .buttonStyle(.borderedProminent).tint(.urWarning)
                            default:
                                EmptyView()
                            }
                            Spacer()
                            if sync.status?["sync"]["protocol"].int != 2 && state != "local_only" {
                                Button("Remove cloud copy", role: .destructive) { removing = entry }.buttonStyle(.borderless).font(.subheadline)
                            }
                        }
                        .disabled(busy)
                    }
                    .padding(.vertical, 4)
                }
            }
        }
    }

    private func syncStateLabel(_ state: String) -> String {
        switch state {
        case "cloud_only": return String(localized: "In the cloud")
        case "local_only": return String(localized: "This device only")
        case "conflict": return String(localized: "Needs review")
        case "synced": return String(localized: "Synced")
        case "uploaded": return String(localized: "Uploaded")
        case "downloaded": return String(localized: "Downloaded")
        case "merged": return String(localized: "Merged")
        case "waiting": return String(localized: "Waiting")
        case "deletion_conflict": return String(localized: "Newer changes need review before deletion")
        case "update_app": return String(localized: "Update the app")
        case "error": return String(localized: "Error")
        case "": return String(localized: "Unknown")
        default: return state.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }

    /// The last sync's message (the engine's words) and when it was tried.
    private func reportLine(_ report: JSON) -> String {
        var parts = [report.text("message")].filter { !$0.isEmpty }
        if !report.text("at").isEmpty {
            let when = lastAttempt(report.text("at"))
            parts.append(String(localized: "Last attempt: \(when)"))
        }
        return ImportsText.line(parts)
    }

    private func lastAttempt(_ at: String) -> String {
        let parser = ISO8601DateFormatter()
        parser.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        let date = parser.date(from: at) ?? ISO8601DateFormatter().date(from: at)
        return date.map { $0.formatted(Date.FormatStyle(date: .abbreviated, time: .shortened).locale(.app)) } ?? at
    }

    /// Runs one account or sync action: one at a time, with its error shown on the screen.
    private func run(_ action: @escaping @MainActor () async throws -> Void) {
        guard !busy else { return }
        busy = true
        error = nil
        notice = nil
        Task {
            defer { busy = false }
            do { try await action() } catch { self.error = BudgetStore.readable(error) }
            await sync.refreshStatus()
        }
    }
}

private struct PassphraseRequest: Identifiable {
    enum Kind { case setup, unlock, reset }
    let id = UUID()
    var kind: Kind
}

/// Creating, entering or resetting the sync passphrase.
private struct PassphraseSheet: View {
    var request: PassphraseRequest
    var submit: (String) async throws -> Void
    @State private var phrase = ""
    @State private var repeated = ""

    var body: some View {
        let needsRepeat = request.kind != .unlock
        let valid = phrase.count >= 8 && (!needsRepeat || phrase == repeated)
        FormSheet(title: title, confirm: request.kind == .reset ? String(localized: "Reset") : String(localized: "Continue"),
                  enabled: valid, destructive: request.kind == .reset) {
            try await submit(phrase)
        } content: {
            Section {
                SecureField("Sync passphrase", text: $phrase).textContentType(request.kind == .unlock ? .password : .newPassword)
                if needsRepeat { SecureField("Repeat passphrase", text: $repeated).textContentType(.newPassword) }
            } footer: {
                Text(request.kind == .reset
                     ? "This deletes cloud copies encrypted with the old passphrase. Budgets on your devices stay, and this device uploads its copies again."
                     : request.kind == .setup
                        ? "At least 8 characters. Keep it somewhere safe: UrBudget can't recover it for you."
                        : "The passphrase you chose when you set up sync on another device.")
            }
            if needsRepeat && !repeated.isEmpty && phrase != repeated { FormErrorView(message: String(localized: "The passphrases don't match.")) }
        }
        .presentationDetents([.medium])
    }

    private var title: String {
        switch request.kind {
        case .reset: return String(localized: "Reset passphrase")
        case .setup: return String(localized: "Create passphrase")
        case .unlock: return String(localized: "Unlock sync")
        }
    }
}

/// Choosing what to keep when both devices changed the same thing.
private struct SyncConflictSheet: View {
    var entry: JSON
    var resolve: (String, JSON) async throws -> Void
    @State private var choices: [String: String] = [:]
    @State private var whole = ""

    var body: some View {
        let conflicts = entry.rows("conflicts")
        let ready = conflicts.isEmpty ? !whole.isEmpty : conflicts.allSatisfy { choices[$0.text("key")] != nil }
        FormSheet(title: String(localized: "Resolve \(entry.text("name"))"), confirm: String(localized: "Apply"), enabled: ready) {
            var picked: JSON = [:]
            for (key, value) in choices { picked[key] = .string(value) }
            try await resolve(conflicts.isEmpty ? whole : "merge", picked)
        } content: {
            Section {
                Text("Review both versions. Nothing changes until you apply your choices.").foregroundStyle(Color.secondary)
            }
            if conflicts.isEmpty {
                Picker("Keep", selection: $whole) {
                    Text("This device replaces the cloud copy").tag("local")
                    Text("The cloud copy replaces this device").tag("cloud")
                }
                .pickerStyle(.inline)
            }
            ForEach(conflicts, id: \.self.conflictKey) { difference in
                Section {
                    if difference["about"].flag("wholeBudget") {
                        Text("This choice replaces all budget changes with the selected version.").foregroundStyle(.secondary)
                    }
                    LabeledContent("This device", value: describe(difference["local"]))
                    LabeledContent("Cloud", value: describe(difference["remote"]))
                    Picker("Keep", selection: Binding(get: { choices[difference.text("key")] ?? "" },
                                                      set: { choices[difference.text("key")] = $0 })) {
                        Text("This device").tag("local")
                        Text("Cloud").tag("cloud")
                    }
                    .pickerStyle(.segmented)
                } header: {
                    Text(ImportsText.line([difference.text("label"), difference.text("field") == "budgetVersion" ? String(localized: "Whole budget version") : difference.text("field")].filter { !$0.isEmpty }))
                }
            }
        }
    }

    private func describe(_ value: JSON) -> String {
        if value.has("amount") && (value.has("lines") || value.has("transfer")) {
            func state(_ value: String) -> String {
                switch value { case "cleared": return String(localized: "Cleared"); case "reconciled": return String(localized: "Reconciled"); case "uncleared": return String(localized: "Uncleared"); default: return value }
            }
            var parts = [value.text("account"), Money.format(value.long("amount"))]
            if !value.text("transfer").isEmpty { let account = value.text("transfer"); parts.append(String(localized: "Transfer: \(account)")) }
            for line in value.rows("lines") {
                let destination: String
                if !line.text("transfer").isEmpty { let account = line.text("transfer"); destination = String(localized: "Transfer: \(account)") }
                else { destination = line.text("category", String(localized: "Ready to Assign")) }
                parts.append(destination + ": " + Money.format(line.long("amount")))
            }
            if !value.text("status").isEmpty { parts.append(state(value.text("status"))) }
            if !value.text("transferStatus").isEmpty { let status = state(value.text("transferStatus")); parts.append(String(localized: "Transfer status: \(status)")) }
            return parts.filter { !$0.isEmpty }.joined(separator: "\n")
        }
        if value.has("reason") && value.has("transactions") {
            let accounts = String(value.long("accounts")), categories = String(value.long("categories")), transactions = String(value.long("transactions"))
            return [value.text("name"), String(localized: "Accounts: \(accounts); Categories: \(categories); Transactions: \(transactions)"), value.text("reason")].joined(separator: "\n")
        }
        if value.text("type") == "monthPlan" {
            let currency = value.text("currency", "SAR")
            let before = Money.format(value.long("baseTotal"), currency: currency)
            let after = Money.format(value.long("total"), currency: currency)
            let change = Money.format(value.long("readyToAssignChange"), currency: currency)
            let moves = String(value.long("moveCount"))
            let summary = [String(localized: "Month total: \(before) → \(after)"),
                           String(localized: "Ready to Assign change: \(change)"), String(localized: "Moves: \(moves)")]
            let rows = value.rows("categories").map { row in
                let delta = Money.format(row.long("delta"), currency: currency)
                let detail = String(localized: "Change: \(delta)")
                return "\(row.text("name")): \(Money.format(row.long("base"), currency: currency)) → \(Money.format(row.long("amount"), currency: currency)) (\(detail))"
            }
            return (summary + rows).joined(separator: "\n")
        }
        switch value {
        case .null: return String(localized: "Deleted")
        case .string(let text): return text.isEmpty ? String(localized: "Empty") : text
        case .int, .double, .bool: return value.string
        default: return value.serialized()
        }
    }
}

private extension JSON {
    var conflictKey: String { text("key") }
}

/// Deleting the account needs its email typed, as on Android.
private struct DeleteAccountSheet: View {
    var email: String
    /// Sync is an App Store subscription, which only its subscriber can cancel.
    var appStore: Bool
    var delete: (String) async throws -> Void
    @State private var confirmation = ""

    var body: some View {
        FormSheet(title: String(localized: "Delete account"), confirm: String(localized: "Delete"),
                  enabled: confirmation.trimmingCharacters(in: .whitespaces).caseInsensitiveCompare(email) == .orderedSame, destructive: true) {
            try await delete(confirmation)
        } content: {
            Section {
                TextField("Type \(email) to confirm", text: $confirmation).emailKeyboard().autocorrectionDisabled()
            } footer: {
                Text(appStore
                     ? "This deletes your account and cloud budgets. Budgets on your devices stay. Your App Store subscription keeps renewing until you cancel it in your Apple Account settings (Manage subscription, above). Purchase records may be kept as the privacy policy describes."
                     : "This deletes your account and cloud budgets and stops subscription renewals. Budgets on your devices stay. Purchase records may be kept as the privacy policy describes.")
            }
        }
        .presentationDetents([.medium])
    }
}
