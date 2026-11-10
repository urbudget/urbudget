import AVFoundation
import SwiftUI
import VisionKit

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
    @State private var addingDevice = false
    @State private var unlockingWithCode = false
    /// What the account's sync key is, as the engine last answered `sync_key_info`: whether one
    /// exists, whether a passphrase or a recovery code opens it, and how setup and reset go.
    @State private var keyInfo: JSON = [:]
    /// A recovery code just made, shown until the person says they've saved it.
    @State private var newCode: ShownRecoveryCode?
    @State private var confirmingNewCode = false
    @State private var confirmingReset = false
    /// Sync's less used rows (recovery code, reset, lock) are folded away until asked for.
    @State private var more = false
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
        .task(id: "\(signedIn)|\(sync.unlocked)") {
            // While sync is locked here, the ways to unlock it depend on the account's key.
            guard !signedIn.isEmpty else { keyInfo = [:]; return }
            guard !sync.unlocked else { return }
            if let info = try? await store.service("sync_key_info") { keyInfo = info }
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
            let currency = entry.value.text("currency", entry.value["metadata"].text("currency", store.currency))
            SyncConflictSheet(entry: entry.value, currency: currency) { keep, choices in
                _ = try await store.service("sync_resolve", ["id": .string(entry.value.text("id")), "keep": .string(keep), "choices": choices, "review": entry.value["review"]])
                store.reload()
                await sync.refreshStatus()
            }
        }
        .sheet(isPresented: $addingDevice) { AddDeviceSheet() }
        .sheet(isPresented: $unlockingWithCode) {
            // A wrong or used code stays in the sheet; the first sync after it shows here.
            UnlockWithCodeSheet(recovery: keyInfo["recovery"].bool) { run { try await sync.syncNow() } }
        }
        .sheet(item: $newCode) { shown in RecoveryCodeSheet(code: shown.code) }
        // The current code can't be shown: this iPhone never kept it. So the one thing to do is
        // make a new one.
        .alert("Recovery code", isPresented: $confirmingNewCode) {
            Button("Make a new code") { makeKey("sync_recovery_create", thenSync: false) }
            Button("Cancel", role: .cancel) {}
        } message: {
            if status["sync"]["recovery"].bool {
                Text("Your account has a recovery code, for unlocking sync when you have no other device at hand. It was shown when it was made and can't be shown again. If you've lost it, make a new one: the old code stops working and nothing else changes.")
            } else {
                Text("Your account has no recovery code yet. One unlocks sync on a new device when you have no other device at hand. Your sync passphrase keeps working.")
            }
        }
        .confirmationDialog("Reset sync?", isPresented: $confirmingReset, titleVisibility: .visible) {
            Button("Reset", role: .destructive) { makeKey("sync_reset", thenSync: true) }
        } message: {
            if sync.unlocked {
                Text("Resetting makes a new sync key and a new recovery code, and deletes the cloud copies made with the old key. Budgets stay on the devices that have them, as recovered copies in the budget library to bring back and add to sync again. Your other devices need to be added again.")
            } else {
                // Asked from an iPhone that can't unlock: there may be a way that deletes nothing.
                Text("Resetting makes a new sync key and a new recovery code, and deletes the cloud copies made with the old key. Budgets stay on the devices that have them, as recovered copies in the budget library to bring back and add to sync again. If another device of yours still syncs, add this iPhone from it instead: nothing is deleted that way.")
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
                Button { addingDevice = true } label: { Label("Add a device", systemImage: "laptopcomputer.and.iphone") }
                    .disabled(busy)
                // What few people need, and nobody often, folds away.
                DisclosureGroup("More", isExpanded: $more) {
                    // Sync that makes its key in the app has a recovery code to replace.
                    if status["sync"]["protocol"].int == 2 {
                        Button("Recovery code") { confirmingNewCode = true }
                    }
                    Button("Reset sync") { askReset() }
                    Button("Lock sync on this device") { run { _ = try await store.service("sync_lock") } }
                }
                .disabled(busy)
            } else if keyInfo["exists"].bool {
                // The account has a key: its passphrase where it has one, and a code either way.
                if keyInfo["passphrase"].bool {
                    Button { passphrase = PassphraseRequest(kind: .unlock) } label: { Label("Unlock with your sync passphrase", systemImage: "key") }
                    Button { unlockingWithCode = true } label: { Label("Unlock with a code", systemImage: "qrcode.viewfinder") }
                } else {
                    Button { unlockingWithCode = true } label: { Label("Unlock sync", systemImage: "key") }
                }
                Button("Can't unlock? Reset sync") { askReset() }
                    .font(.footnote)
            } else {
                Button {
                    run {
                        // If another device set sync up since this screen opened, the rows above
                        // take over. Otherwise this is the account's first device.
                        keyInfo = try await store.service("sync_key_info")
                        if keyInfo["exists"].bool { return }
                        if keyInfo.text("setup") == "code" {
                            // Nothing to choose: the key is made here, and its code comes back to show.
                            let made = try await store.service("sync_setup")
                            newCode = ShownRecoveryCode(made.text("recoveryCode"))
                            try await sync.syncNow()
                        } else {
                            // Sync that still makes its key from a passphrase asks for one.
                            passphrase = PassphraseRequest(kind: .setup)
                        }
                    }
                } label: { Label("Turn on sync", systemImage: "key") }
            }
        } header: {
            Text("Encrypted sync")
        } footer: {
            if let report, !(report.text("message").isEmpty && report.text("at").isEmpty) {
                Text(reportLine(report))
            } else {
                Text("Only your devices can open your budgets. UrBudget can't read or recover them.")
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

    /// A reset makes a key here with a recovery code, or asks for a new passphrase where sync
    /// still makes its key from one.
    private func askReset() {
        run {
            keyInfo = try await store.service("sync_key_info")
            if keyInfo.text("reset") == "code" { confirmingReset = true } else { passphrase = PassphraseRequest(kind: .reset) }
        }
    }

    /// Makes a new recovery code, alone or with the new key of a reset, and shows it once.
    private func makeKey(_ method: String, thenSync: Bool) {
        run {
            let made = try await store.service(method)
            newCode = ShownRecoveryCode(made.text("recoveryCode"))
            if thenSync { try await sync.syncNow() }
        }
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
    var currency: String
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
            var parts = [value.text("account"), Money.format(value.long("amount"), currency: currency)]
            if !value.text("transfer").isEmpty { let account = value.text("transfer"); parts.append(String(localized: "Transfer: \(account)")) }
            for line in value.rows("lines") {
                let destination: String
                if !line.text("transfer").isEmpty { let account = line.text("transfer"); destination = String(localized: "Transfer: \(account)") }
                else { destination = line.text("category", String(localized: "Ready to Assign")) }
                parts.append(destination + ": " + Money.format(line.long("amount"), currency: currency))
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

// MARK: The recovery code

// Sync with no passphrase to choose (docs/plans/2026-11-10-sync-device-approval.md). The account's
// first device makes the sync key and a recovery code for it, shown once for the person to keep.
// After that a device gets the key from one that syncs (below) or, with none at hand, from that
// code. The engine makes and checks the codes; these are its screens.

/// A recovery code to show, once. `nil` when the engine made none.
private struct ShownRecoveryCode: Identifiable {
    let id = UUID()
    var code: String

    init?(_ code: String) {
        guard !code.isEmpty else { return nil }
        self.code = code
    }
}

/// A recovery code that was just made. The iPhone doesn't keep it, so the sheet's one way out is
/// the button that says it's saved.
private struct RecoveryCodeSheet: View {
    @Environment(\.dismiss) private var dismiss
    var code: String
    @State private var copied = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(code)
                        .font(.title3.monospaced().weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .multilineTextAlignment(.center)
                        .textSelection(.enabled)
                        .environment(\.layoutDirection, .leftToRight)
                    Button {
                        #if canImport(UIKit)
                        UIPasteboard.general.string = code
                        #endif
                        copied = true
                    } label: {
                        if copied { Label("Copied", systemImage: "checkmark") } else { Label("Copy", systemImage: "doc.on.doc") }
                    }
                } footer: {
                    Text("If you ever have no device that syncs, this code gets your budgets back. Keep it somewhere safe: we can't show it again.")
                }
                Section {
                    Button("I've saved it") { dismiss() }
                        .frame(maxWidth: .infinity)
                        .fontWeight(.semibold)
                }
            }
            .navigationTitle("Save your recovery code")
            .inlineTitle()
        }
        // A swipe down does nothing: a slip would lose the only showing.
        .interactiveDismissDisabled()
    }
}

// MARK: Adding a device with a code

// Adding a device without typing the sync passphrase (docs/plans/2026-11-10-sync-device-approval.md).
// A one-time code goes from one screen to the other by camera or keyboard, and the shared engine
// uses it to hand the sync key over; the server never sees the code. The phone is the one with a
// camera, so it scans: a new phone scans the code a device that syncs shows, and a phone that
// syncs scans the code a new computer shows, then approves it.

/// Whether enough of a code is typed to try it: 24 characters, however they are spaced.
private func codeComplete(_ code: String) -> Bool {
    code.filter { !$0.isWhitespace && $0 != "-" }.count >= 24
}

/// The engine's QR code: `modules` is `size`×`size` characters, row by row, `1` for dark. Always
/// dark on white with the four-square margin scanners expect, and never mirrored for Arabic.
private struct PairingCodeImage: View {
    var qr: JSON

    var body: some View {
        let size = Int(qr["size"].int)
        let modules = Array(qr.text("modules").utf8)
        Canvas { context, canvas in
            context.fill(Path(CGRect(origin: .zero, size: canvas)), with: .color(.white))
            guard size > 0, modules.count == size * size else { return }
            let cell = min(canvas.width, canvas.height) / CGFloat(size + 8)
            var dark = Path()
            for y in 0..<size {
                for x in 0..<size where modules[y * size + x] == UInt8(ascii: "1") {
                    // A hair wider than a cell, so neighbours leave no light seam between them.
                    dark.addRect(CGRect(x: CGFloat(x + 4) * cell, y: CGFloat(y + 4) * cell, width: cell + 0.5, height: cell + 0.5))
                }
            }
            context.fill(dark, with: .color(.black))
        }
        .frame(width: 220, height: 220)
        .environment(\.layoutDirection, .leftToRight)
        .accessibilityLabel(Text("QR code for adding a device"))
    }
}

#if canImport(UIKit)
/// The camera, reading one QR code.
private struct PairingCodeCamera: UIViewControllerRepresentable {
    var found: (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(found: found) }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(recognizedDataTypes: [.barcode(symbologies: [.qr])], qualityLevel: .balanced,
                                                recognizesMultipleItems: false, isHighFrameRateTrackingEnabled: false, isHighlightingEnabled: true)
        scanner.delegate = context.coordinator
        try? scanner.startScanning()
        return scanner
    }

    func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {}

    static func dismantleUIViewController(_ scanner: DataScannerViewController, coordinator: Coordinator) {
        scanner.stopScanning()
    }

    @MainActor final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        private let found: (String) -> Void
        private var done = false

        init(found: @escaping (String) -> Void) { self.found = found }

        func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) {
            guard !done else { return }
            for item in addedItems {
                if case .barcode(let code) = item, let text = code.payloadStringValue {
                    done = true
                    found(text)
                    return
                }
            }
        }
    }
}
#endif

/// The scanner as a sheet. It asks for the camera the first time; without it the code is typed.
private struct PairingCodeScanner: View {
    var found: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var allowed: Bool?

    var body: some View {
        NavigationStack {
            Group {
                if allowed == nil {
                    ProgressView()
                } else if allowed == true, cameraSupported {
                    #if canImport(UIKit)
                    PairingCodeCamera(found: found).ignoresSafeArea(edges: .bottom)
                    #endif
                } else {
                    ContentUnavailableView("The camera isn’t available", systemImage: "camera",
                                           description: Text("Allow UrBudget to use the camera in Settings, or type the code instead."))
                }
            }
            .navigationTitle("Scan the code")
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
        }
        .task { allowed = await AVCaptureDevice.requestAccess(for: .video) }
    }

    private var cameraSupported: Bool {
        #if canImport(UIKit)
        DataScannerViewController.isSupported
        #else
        false
        #endif
    }
}

/// On an iPhone that syncs: shows a code for a new phone to scan (or to type on a computer), and
/// scans the code a new computer shows. The shown code is taken back when this closes.
private struct AddDeviceSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var offer: JSON?
    @State private var expired = false
    @State private var attempt = 0
    @State private var error: String?
    @State private var notice: String?
    @State private var scanning = false
    @State private var approving: String?
    @State private var busy = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    VStack(spacing: 10) {
                        if expired {
                            Text("This code has expired.").foregroundStyle(Color.secondary)
                            Button("Make a new code") { error = nil; attempt += 1 }
                        } else if let offer {
                            PairingCodeImage(qr: offer["qr"])
                            Text(offer.text("code"))
                                .font(.body.monospaced().weight(.semibold))
                                .environment(\.layoutDirection, .leftToRight)
                                .textSelection(.enabled)
                        } else if error == nil {
                            ProgressView()
                        } else {
                            Button("Make a new code") { error = nil; attempt += 1 }
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .multilineTextAlignment(.center)
                } header: {
                    Text("Sign in to UrBudget on the new device, then scan this code there or type it.")
                } footer: {
                    if offer != nil && !expired {
                        Text("It works once, for 5 minutes. Only use it on a device of your own: anyone who has it can read your budgets, and UrBudget will never ask you for it.")
                    }
                }
                // A computer can't scan, so it shows a code of its own and the iPhone scans that.
                // Without the camera, the computer takes this iPhone's code typed, as a phone would.
                Section {
                    Button { error = nil; notice = nil; scanning = true } label: { Label("Adding a computer? Scan its code", systemImage: "qrcode.viewfinder") }
                }
                .disabled(busy)
                if let notice { Notice(text: notice, tone: "positive").listRowInsets(EdgeInsets()).listRowBackground(Color.clear) }
                if let error { FormErrorView(message: error) }
            }
            .navigationTitle("Add a device")
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .task(id: attempt) { await makeOffer() }
        .onDisappear { withdraw() }
        .sheet(isPresented: $scanning) {
            PairingCodeScanner { text in
                scanning = false
                approving = text
            }
        }
        // The key goes to whoever shows this code, so the person says yes first.
        .alert("Add this device to sync?", isPresented: Binding(get: { approving != nil }, set: { if !$0 { approving = nil } }), presenting: approving) { code in
            Button("Add this device") { approve(code) }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("It will be able to read every budget in your account. Only continue if the code is on a device of your own.")
        }
    }

    private func makeOffer() async {
        withdraw()
        offer = nil
        expired = false
        do {
            offer = try await store.service("sync_pair_offer")
            try await Task.sleep(for: .seconds(300))
            expired = true
        } catch is CancellationError {
        } catch {
            self.error = BudgetStore.readable(error)
        }
    }

    /// Takes the shown code back, so a late scan of it finds nothing.
    private func withdraw() {
        guard let id = offer?.text("id"), !id.isEmpty else { return }
        let store = store
        Task { _ = try? await store.service("sync_pair_cancel", ["id": .string(id)]) }
    }

    private func approve(_ code: String) {
        guard !busy else { return }
        busy = true
        Task {
            defer { busy = false }
            do {
                _ = try await store.service("sync_pair_approve", ["code": .string(code)])
                notice = String(localized: "Done. The other device unlocks in a few seconds.")
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }
}

/// On an iPhone that doesn't sync yet: unlocks sync with a code. Scanning takes the one a device
/// that syncs shows; the box takes that one typed or, when the account has one (`recovery`), the
/// recovery code. The engine works out which it was given.
private struct UnlockWithCodeSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var recovery: Bool
    var unlocked: () -> Void
    @State private var typed = ""
    @State private var scanning = false
    @State private var error: String?
    @State private var busy = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Button { error = nil; scanning = true } label: { Label("Scan the code", systemImage: "qrcode.viewfinder") }
                    TextField("Code", text: $typed)
                        .font(.body.monospaced())
                        .autocorrectionDisabled()
                        .environment(\.layoutDirection, .leftToRight)
                } footer: {
                    if recovery {
                        Text("On a device where UrBudget already syncs, open Account & sync and choose Add a device. Then scan the code it shows, or type it here. No device at hand? Type your recovery code.")
                    } else {
                        Text("On a device where UrBudget already syncs, open Account & sync and choose Add a device. Then scan the code it shows, or type it here.")
                    }
                }
                if let error { FormErrorView(message: error) }
            }
            .disabled(busy)
            .navigationTitle("Unlock with a code")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(busy) }
                ToolbarItem(placement: .confirmationAction) {
                    if busy { ProgressView() } else { Button("Unlock sync") { accept(typed) }.disabled(!codeComplete(typed)) }
                }
            }
        }
        .sheet(isPresented: $scanning) {
            PairingCodeScanner { text in
                scanning = false
                accept(text)
            }
        }
        .presentationDetents([.medium])
    }

    private func accept(_ code: String) {
        guard !busy else { return }
        busy = true
        error = nil
        Task {
            defer { busy = false }
            do {
                _ = try await store.service("sync_unlock_code", ["code": .string(code)])
                dismiss()
                unlocked()
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }
}
