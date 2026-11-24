import SwiftUI

/// Account & sync: sign in with an emailed code, and end-to-end encrypted sync of budgets
/// between this iPhone, desktop and Android. One screen, top to bottom, each part only while it
/// has something to say (docs/plans/2026-11-24-sync-device-approval.md, section 14): the account,
/// the plan, sync, the devices that sync, the budgets that need something, and More.
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
    /// What the account's sync key is, as `sync_key_info` last answered: asked while sync is
    /// locked here, since that decides between turning sync on and unlocking it.
    @State private var keyInfo: JSON?
    @State private var keyInfoError: String?
    /// A code or passphrase typed to unlock sync, and what the engine said of the last one tried.
    @State private var typedSecret = ""
    @State private var showingTyped = false
    @State private var unlockError: String?
    @State private var scanning = false
    @State private var scanned: String?
    @State private var addingDevice = false
    /// A computer's code that arrived as a link, until the person says whether to add it.
    @State private var approvingLink: String?
    /// A recovery code this device holds that the person hasn't said they saved.
    @State private var recoveryCode: RecoveryCode?
    @State private var confirmingNewCode = false
    @State private var confirmingReset = false
    /// What few people need, and nobody often, is folded away until asked for.
    @State private var more = false
    /// The devices that sync with the account (`sync_devices`), this one first.
    @State private var devices: [SyncDevice] = []
    /// Counts devices added from here, so the list is asked for again.
    @State private var devicesAdded = 0
    @State private var showingLibrary = false
    @State private var conflict: IdentifiedJSON?
    @State private var removing: JSON?
    @State private var confirmSignOut = false
    @State private var deletingAccount = false
    @State private var moving = false
    @FocusState private var typingCode: Bool

    private var status: JSON { sync.status ?? [:] }
    private var signedIn: String { status.text("email") }
    private var stage: SyncKey.Stage { SyncKey.stage(status, info: keyInfo) }

    var body: some View {
        CompactForm {
            if signedIn.isEmpty {
                Section {
                    Text("Your budgets stay on this device. Sync is optional, and budgets are encrypted before they leave it.")
                        .font(.subheadline).foregroundStyle(Color.secondary)
                        .listRowBackground(Color.clear)
                }
            }
            if let notice { Notice(text: notice, tone: "info").listRowInsets(EdgeInsets()).listRowBackground(Color.clear) }
            if let error { FormErrorView(message: error) }
            accountSection
            if !signedIn.isEmpty {
                SyncSubscriptionSection(overview: overview, reload: loadOverview)
                syncSection
                if stage == .unlocked { devicesSection }
                budgetsSection
                moreSection
            } else {
                Section {
                    Button("Privacy & your choices", action: openPrivacy).disabled(busy)
                }
            }
        }
        .navigationDestination(isPresented: $showingLibrary) { LibraryView() }
        .navigationTitle("Account & sync")
        .inlineTitle()
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
        .task(id: "\(signedIn)|\(sync.unlocked)") { await loadKeyInfo() }
        // Asked again after each sync pass while the screen is open: a device added or cut off
        // from another one, and when each last synced.
        .task(id: "\(signedIn)|\(sync.unlocked)|\(devicesAdded)|\(status["sync"]["last"].text("at"))") { await loadDevices() }
        // A code the phone's own Camera opened the app with. Signed out, it waits for the sign-in.
        .task(id: "\(sync.scannedLink ?? "")|\(signedIn)|\(sync.unlocked)") { takeScannedLink() }
        // The key goes to whoever shows that code, so the person says yes first.
        .alert("Add this device?", isPresented: Binding(get: { approvingLink != nil }, set: { if !$0 { approvingLink = nil } }), presenting: approvingLink) { code in
            Button("Add device") { approveLink(code) }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("Only approve a device you are holding. It will be able to read every budget in this account.")
        }
        // A code made here stays on this device until it's confirmed saved, so it's shown again
        // after the app was closed on it or an answer was lost.
        .task(id: SyncKey.recoveryUnsaved(status)) { await showUnsavedCode() }
        // What the screen last said ("Device added.") is about the state it was in: once sync
        // locks or unlocks here, it no longer holds.
        .onChange(of: sync.unlocked) { notice = nil }
        .sheet(item: $recoveryCode) { shown in
            RecoveryCodeSheet(code: shown.code) {
                _ = try await store.service("sync_recovery_saved")
                await sync.refreshStatus()
                recoveryCode = nil
            }
        }
        .sheet(isPresented: $addingDevice) { AddDeviceSheet { deviceAdded() } }
        .sheet(isPresented: $scanning, onDismiss: {
            if let scanned { unlock(scanned) }
            scanned = nil
        }) {
            CodeScannerSheet { scanned = $0 }
        }
        .sheet(isPresented: $moving) {
            // Re-read on every status refresh: a stale saved key turns into a passphrase request.
            MoveSyncSheet(needsPassphrase: SyncMove.needsPassphrase(status)) { phrase in
                _ = try await store.service("sync_upgrade", SyncMove.parameters(status, passphrase: phrase))
                run { try await sync.syncNow() }
                notice = String(localized: "This account now uses the new sync.")
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
        .sheet(isPresented: $deletingAccount) { DeleteAccountSheet(email: signedIn, appStore: overview?["subscription"].text("provider") == "apple") { confirm in
            _ = try await store.service("account_delete", ["confirm": .string(confirm)])
            await sync.refreshStatus()
        } }
        .confirmationDialog("Sign out of UrBudget?", isPresented: $confirmSignOut, titleVisibility: .visible) {
            Button("Sign out", role: .destructive) { run { _ = try await store.service("account_sign_out") } }
        } message: {
            Text("Sync stops on this iPhone and it forgets the sync key. Your budgets here stay as they are.")
        }
        .alert("Make a new recovery code?", isPresented: $confirmingNewCode) {
            Button("Make a new code") { makeRecoveryCode() }
            Button("Cancel", role: .cancel) {}
        } message: {
            if SyncKey.hasRecoveryCopy(status) {
                Text("The new code replaces the last one made this way. The code or passphrase sync was set up with keeps working. To retire it, reset sync.")
            } else {
                Text("The code or passphrase you already have keeps working. To retire it, reset sync.")
            }
        }
        // An alert, not a confirmation dialog: this explanation is several sentences, and a dialog
        // hangs them as a narrow popover off whichever row is nearest.
        .alert("Reset sync?", isPresented: $confirmingReset) {
            Button("Reset sync", role: .destructive) { reset() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(resetMessage)
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
        Section {
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
                    Button {
                        run {
                            _ = try await store.service("account_verify", ["email": .string(email.trimmingCharacters(in: .whitespaces)),
                                                                           "code": .string(code.trimmingCharacters(in: .whitespaces))])
                            code = ""
                            await sync.syncAutomatically()
                        }
                    } label: { Text("Sign in").fontWeight(.semibold).frame(maxWidth: .infinity) }
                    .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                    .listRowSeparator(.hidden)
                    .disabled(busy || code.trimmingCharacters(in: .whitespaces).isEmpty)
                    Button("Send a new code", action: sendCode).disabled(busy || !email.contains("@"))
                } else {
                    Button(action: sendCode) { Text("Send sign-in code").fontWeight(.semibold).frame(maxWidth: .infinity) }
                        .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                        .listRowSeparator(.hidden)
                        .disabled(busy || !email.contains("@"))
                }
            } else {
                // One row: the email, and signing out as a quiet action on it.
                Label {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(signedIn)
                        Button("Sign out on this device") { confirmSignOut = true }
                            .buttonStyle(.borderless)
                            .font(.footnote)
                            .disabled(busy)
                    }
                } icon: {
                    Image(systemName: "person.crop.circle.fill").foregroundStyle(Color.urAccent).font(.title2)
                }
            }
        } header: {
            Text("Account")
        } footer: {
            // Before signing in, what sync costs.
            if signedIn.isEmpty {
                Text("UrBudget is free. Sync is an optional subscription. The App Store shows its price and handles payment.")
            }
        }
    }

    private func openPrivacy() {
        run { _ = try await store.service("account_open", ["next": "/privacy/app/"]) }
    }

    /// Optional subscription details: a slow request mustn't hold up the local controls.
    private func loadOverview() async {
        guard !signedIn.isEmpty else { return }
        overview = try? await store.service("account_overview")
    }

    private func sendCode() {
        run {
            let reply = try await store.service("account_start", ["email": .string(email.trimmingCharacters(in: .whitespaces))])
            sent = true
            #if DEBUG
            // A development server (`wrangler dev`, with URBUDGET_API) says the code it would have
            // emailed: no mail is sent there. The real service never does.
            if !reply.text("devCode").isEmpty { code = reply.text("devCode") }
            #endif
            notice = String(localized: "Check your email for a sign-in code. It may land in your Junk folder.")
            typingCode = true
        }
    }

    /// The account still syncs on protocol 1, and urbudget.app now moves it to the new sync.
    @ViewBuilder private var moveRows: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("A new sync is ready for this account").fontWeight(.semibold)
            Text("It finds every budget in your account on its own and recovers safely when an upload is interrupted. Your budgets and sync passphrase stay the same.")
                .font(.footnote).foregroundStyle(Color.secondary)
        }
        Button("Move to the new sync") { moving = true }
            .disabled(busy)
    }

    @ViewBuilder private var syncSection: some View {
        Section {
            switch stage {
            case .unlocked:
                statusRow
            case .locked:
                lockedRows
            case .none:
                // Nothing to choose or type: the key is made here, and its recovery code comes back to show.
                Text("Budgets are encrypted with a key only your devices hold. UrBudget can't read or recover them.")
                    .font(.subheadline).foregroundStyle(Color.secondary)
                Button(action: turnOn) {
                    Label("Turn on sync", systemImage: "key").fontWeight(.semibold).frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
                .listRowSeparator(.hidden)
                .disabled(busy || overview?["sync"]["active"] == JSON.bool(false))
            case .asking:
                if let keyInfoError {
                    FormErrorView(message: keyInfoError)
                    Button("Try again") { Task { await loadKeyInfo() } }
                } else {
                    ProgressView().frame(maxWidth: .infinity)
                }
            }
            if SyncMove.offered(status) { moveRows }
        } header: {
            Text("Sync")
        }
    }

    /// Sync is on: one line saying where it stands, with Sync now beside it.
    private var statusRow: some View {
        HStack(spacing: 12) {
            // "2 minutes ago" is kept current while the screen is open.
            TimelineView(.periodic(from: .now, by: 30)) { clock in
                Text(statusLine(now: clock.date))
                    .font(.subheadline)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            Button {
                run { try await sync.syncNow() }
            } label: {
                if sync.syncing { ProgressView() } else { Text("Sync now").fontWeight(.semibold) }
            }
            .buttonStyle(.bordered).buttonBorderShape(.capsule)
            .fixedSize()
            .disabled(busy || sync.syncing)
        }
    }

    private func statusLine(now: Date) -> String {
        switch SyncLine.line(syncing: sync.syncing, pending: sync.pending, report: sync.lastReport) {
        case .syncing:
            return String(localized: "Syncing…")
        case .needsYou(let count):
            return String(localized: "\(count) budgets need you. See Budgets below.")
        case .offline:
            return String(localized: "Sync is on, but this device is offline. Changes sync when it's back online.")
        case .failed(let message):
            return message.isEmpty ? String(localized: "The last sync didn't finish. Try Sync now.") : message
        case .waiting:
            return String(localized: "Sync is on. Changes are waiting to sync.")
        case .on(nil):
            return String(localized: "Sync is on.")
        case .on(let last?):
            if SyncKey.isJustNow(last, now: now) { return String(localized: "Sync is on. Last synced just now.") }
            let ago = SyncKey.ago(last, now: now)
            return String(localized: "Sync is on. Last synced \(ago).")
        }
    }

    // MARK: Devices

    /// The devices that sync, to look at: taking one off this list wouldn't take back the key it
    /// holds, so the line under it says what does.
    @ViewBuilder private var devicesSection: some View {
        Section {
            ForEach(devices) { device in
                Label {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(deviceTitle(device))
                        TimelineView(.periodic(from: .now, by: 30)) { clock in
                            if let detail = deviceDetail(device, now: clock.date) {
                                Text(detail).font(.footnote).foregroundStyle(Color.secondary)
                            }
                        }
                    }
                } icon: {
                    Image(systemName: device.current ? (Self.isPad ? "ipad" : "iphone") : "laptopcomputer.and.iphone")
                        .foregroundStyle(Color.secondary)
                }
            }
            Button { addingDevice = true } label: { Label("Add a device", systemImage: "plus") }
                .disabled(busy)
        } header: {
            Text("Devices")
        } footer: {
            Text("To stop a device syncing, sign out on it. To cut off a device you no longer have, reset sync.")
        }
    }

    private static var isPad: Bool { UIDevice.current.userInterfaceIdiom == .pad }
    private static var thisDevice: String { isPad ? String(localized: "This iPad") : String(localized: "This iPhone") }

    private func deviceTitle(_ device: SyncDevice) -> String {
        switch device.title {
        case .named(let name): return name
        case .thisDevice: return Self.thisDevice
        case .added(let date):
            let day = date.formatted(Date.FormatStyle().day().month(.wide).locale(.app))
            return String(localized: "A device added on \(day)")
        case .unnamed: return String(localized: "A device")
        }
    }

    private func deviceDetail(_ device: SyncDevice, now: Date) -> String? {
        switch device.detail(now: now) {
        case .thisDevice: return Self.thisDevice
        case .syncedJustNow: return String(localized: "Last synced just now")
        case .lastSynced(let date):
            let ago = SyncKey.ago(date, now: now)
            return String(localized: "Last synced \(ago)")
        // Not known, for a device that joined before the account kept this: nothing is said of it.
        case .notYet: return nil
        case .none: return nil
        }
    }

    private func loadDevices() async {
        guard !signedIn.isEmpty, sync.unlocked else { devices = []; return }
        // A list that didn't arrive leaves the last one shown.
        if let answer = try? await store.service("sync_devices") { devices = SyncDevice.list(answer) }
        // A device added a moment ago gives its name with its first sync: asked once more for it.
        guard devicesAdded > 0 else { return }
        try? await Task.sleep(for: .seconds(8))
        guard !Task.isCancelled, let answer = try? await store.service("sync_devices") else { return }
        devices = SyncDevice.list(answer)
    }

    private func deviceAdded() {
        notice = String(localized: "Device added.")
        devicesAdded += 1
    }

    // MARK: More

    @ViewBuilder private var moreSection: some View {
        let removable = SyncBudgets.removable(sync.lastReport, protocolNumber: status["sync"]["protocol"].int)
        Section {
            DisclosureGroup("More", isExpanded: $more) {
                Toggle(isOn: $automatic) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Sync automatically")
                        Text("After changes and every minute while the app is open").font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
                if stage == .unlocked {
                    if SyncKey.canReplaceRecoveryCode(status) {
                        Button { confirmingNewCode = true } label: { Label("New recovery code", systemImage: "key") }
                            .disabled(busy)
                    }
                    Button { run { _ = try await store.service("sync_lock") } } label: { Label("Lock sync on this device", systemImage: "lock") }
                        .disabled(busy)
                    Button(role: .destructive) { confirmingReset = true } label: {
                        Label("Reset sync", systemImage: "arrow.counterclockwise").foregroundStyle(Color.urNegative)
                    }
                    .disabled(busy)
                }
                SyncPlanRows(overview: overview, reload: loadOverview)
                // The old sync only: any budget's cloud copy can be removed, listed above or not.
                if !removable.isEmpty {
                    Menu("Remove cloud copy") {
                        ForEach(removable, id: \.idText) { entry in
                            Button(entry.text("name")) { removing = entry }
                        }
                    }
                    .disabled(busy)
                }
                Button("Privacy & your choices", action: openPrivacy).disabled(busy)
                Button("Delete account and cloud data", role: .destructive) { deletingAccount = true }
                    .disabled(busy)
            }
        }
    }

    /// The account has a key this device doesn't hold: a code from a device that syncs, the
    /// recovery code or (where the account has one) the passphrase unlocks it, scanned or typed.
    @ViewBuilder private var lockedRows: some View {
        let passphrase = SyncKey.secret(status, info: keyInfo) != "code"
        let label: LocalizedStringKey = passphrase
            ? "Code from another device, your recovery code, or your sync passphrase"
            : "Code from another device, or your recovery code"
        // Another device reset sync: why this one asks to unlock when it used to sync.
        if SyncKey.resetElsewhere(status) {
            Text("Sync was reset on another device. Add this one again to keep syncing.")
                .font(.subheadline).listRowSeparator(.hidden)
        }
        Text("To unlock sync here, open Account & sync on a device that already syncs and choose Add a device. Then scan the code it shows.")
            .font(.subheadline).foregroundStyle(Color.secondary)
        Button { scanning = true } label: {
            Label("Scan a code", systemImage: "qrcode.viewfinder").fontWeight(.semibold).frame(maxWidth: .infinity)
        }
        .buttonStyle(.borderedProminent).buttonBorderShape(.capsule)
        .listRowSeparator(.hidden)
        .disabled(busy)
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(.subheadline).foregroundStyle(Color.secondary)
            HStack(spacing: 12) {
                Group {
                    if passphrase && !showingTyped {
                        // A passphrase is hidden as it's typed, and the password manager can fill it in.
                        SecureField(label, text: $typedSecret, prompt: Text("Code or passphrase")).textContentType(.password)
                    } else if passphrase {
                        TextField(label, text: $typedSecret, prompt: Text("Code or passphrase"))
                    } else {
                        // A code reads left to right in Arabic too, from its first character.
                        TextField(label, text: $typedSecret, prompt: Text(verbatim: SyncKey.codeShape))
                            .environment(\.layoutDirection, .leftToRight)
                    }
                }
                // A code in even letters; a passphrase, and the Arabic that asks for one, as it's written.
                .font(passphrase ? .body : .body.monospaced())
                .plainTextEntry()
                .submitLabel(.go)
                .onSubmit { unlockTyped() }
                if passphrase {
                    Button { showingTyped.toggle() } label: { Image(systemName: showingTyped ? "eye.slash" : "eye") }
                        .buttonStyle(.borderless)
                        .accessibilityLabel(Text("Show what I type"))
                }
            }
            if let unlockError { FormErrorView(message: unlockError) }
        }
        Button("Unlock sync", action: unlockTyped)
            .disabled(busy || typedSecret.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        Button("Can't unlock?") { confirmingReset = true }
            .font(.footnote)
            .disabled(busy)
    }

    // MARK: Budgets

    /// Only the budgets that need something, each a name, one sentence and one action. The ones
    /// that are simply syncing are counted in a line that leads to the budget library.
    @ViewBuilder private var budgetsSection: some View {
        let protocolNumber = status["sync"]["protocol"].int
        let needing = SyncBudgets.needing(sync.lastReport, protocolNumber: protocolNumber)
        let syncing = SyncBudgets.syncingCount(sync.lastReport)
        if !needing.isEmpty || syncing > 0 {
            Section("Budgets") {
                ForEach(needing, id: \.idText) { entry in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(entry.text("name")).fontWeight(.semibold)
                        Text(sentence(entry)).font(.subheadline).foregroundStyle(Color.secondary)
                        ForEach(entry["notes"].array.map(\.string), id: \.self) { Text($0).font(.footnote).foregroundStyle(Color.secondary) }
                        budgetAction(entry, protocolNumber: protocolNumber)
                            .disabled(busy)
                    }
                    .padding(.vertical, 4)
                }
                if syncing > 0 {
                    NavigationLink { LibraryView() } label: { Text("\(syncing) budgets are syncing") }
                }
            }
        }
    }

    @ViewBuilder private func budgetAction(_ entry: JSON, protocolNumber: Int64) -> some View {
        switch SyncBudgets.action(entry, protocolNumber: protocolNumber) {
        case .review:
            Button("Review differences") { conflict = IdentifiedJSON(value: entry) }
                .buttonStyle(.borderedProminent).tint(.urWarning)
        // A budget kept only on this device, on either sync: one tap adds it to the account.
        case .startSyncing:
            Button("Start syncing") {
                run {
                    if protocolNumber == 2 {
                        _ = try await store.service("sync_enable", ["enabled": true, "ids": JSON([entry.text("id")])])
                    } else {
                        _ = try await store.service("sync_track", ["ids": JSON([entry.text("id")])])
                    }
                    try await sync.syncNow()
                }
            }
            .buttonStyle(.bordered)
        case .download:
            Button("Download and open") {
                run {
                    let id = try await store.service("sync_download", ["id": .string(entry.text("id"))]).string
                    store.openBudget(id: id)
                }
            }
            .buttonStyle(.bordered)
        // A copy kept after a reset is brought back in the library.
        case .library:
            Button("Budget library") { showingLibrary = true }
                .buttonStyle(.bordered)
        case .none:
            EmptyView()
        }
    }

    private func sentence(_ entry: JSON) -> String {
        switch SyncBudgets.sentence(entry) {
        case .message(let message): return message
        case .conflict: return String(localized: "Changed on two devices. Choose what to keep.")
        case .keptCopy: return String(localized: "Kept on this device as a copy. Recover it as a new budget in the budget library.")
        case .deviceOnly: return String(localized: "Kept only on this device.")
        case .state(let state): return syncStateLabel(state)
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
        case "recovery": return String(localized: "Kept copy")
        case "locked": return String(localized: "Locked")
        case "deletion_conflict": return String(localized: "Newer changes need review before deletion")
        case "update_app": return String(localized: "Update the app")
        case "error": return String(localized: "Error")
        case "queued": return String(localized: "Waiting to sync")
        case "trashed": return String(localized: "In Trash")
        case "purged": return String(localized: "Removed")
        case "signed_out": return String(localized: "Signed out")
        case "evicted": return String(localized: "Not downloaded")
        case "subscription_required": return String(localized: "Needs a sync plan")
        case "": return String(localized: "Unknown")
        // A state this version doesn't know is never shown by the engine's name for it.
        default: return String(localized: "Unknown")
        }
    }

    // MARK: The sync key

    private func loadKeyInfo() async {
        guard !signedIn.isEmpty, !sync.unlocked else {
            keyInfo = nil
            keyInfoError = nil
            return
        }
        do {
            keyInfo = try await store.service("sync_key_info")
            keyInfoError = nil
        } catch {
            keyInfoError = BudgetStore.readable(error)
        }
    }

    private func showUnsavedCode() async {
        guard SyncKey.recoveryUnsaved(status), recoveryCode == nil else { return }
        guard let kept = try? await store.service("sync_recovery_unsaved"), let shown = RecoveryCode(kept) else { return }
        if recoveryCode == nil { recoveryCode = shown }
    }

    /// The account's first device: makes the key, and the recovery code that comes back is shown.
    private func turnOn() {
        run {
            do {
                recoveryCode = RecoveryCode(try await store.service("sync_setup")) ?? recoveryCode
            } catch {
                // Another device may have set sync up since this screen asked.
                await loadKeyInfo()
                throw error
            }
            try await sync.syncNow()
        }
    }

    /// Does with a code the Camera scanned what this screen's own scanner would: unlocks this
    /// device with a code another device shows, or asks before approving a device that waits.
    private func takeScannedLink() {
        guard let link = sync.scannedLink, !signedIn.isEmpty else { return }
        sync.scannedLink = nil
        if SyncKey.isRequest(link) {
            if sync.unlocked { approvingLink = link }
            else { store.error = String(localized: "Unlock sync on this device first, then scan the computer's code again.") }
        } else if sync.unlocked {
            store.error = String(localized: "This device already syncs. Scan that code with the device you're adding.")
        } else {
            unlock(link)
        }
    }

    private func approveLink(_ code: String) {
        run {
            _ = try await store.service("sync_pair_approve", ["code": .string(code)])
            deviceAdded()
        }
    }

    private func unlockTyped() {
        guard !typedSecret.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        unlock(typedSecret)
    }

    /// Whatever was scanned or typed: the engine tells a device code, a recovery code and a
    /// passphrase apart. What it says of a wrong one shows under the box.
    private func unlock(_ secret: String) {
        run {
            do {
                _ = try await store.service("sync_unlock", ["secret": .string(secret)])
            } catch {
                unlockError = BudgetStore.readable(error)
                await loadKeyInfo()
                return
            }
            typedSecret = ""
            // The next passphrase typed here is hidden again.
            showingTyped = false
            try await sync.syncNow()
        }
    }

    /// What a reset does, which differs by protocol: on the new sync this device uploads the
    /// budgets it was syncing by itself, and every other device receives them and keeps its own
    /// copy to recover from the budget library; on protocol 1 every budget here uploads again.
    private var resetMessage: String {
        var parts = [SyncKey.protocolNumber(status, info: keyInfo) == 2
            ? String(localized: "Sync gets a new key and a new recovery code, and the cloud copies made with the old key are deleted. This device uploads the budgets it was syncing again, and they keep syncing. Your other devices are unlocked with the new recovery code, or added from this device. Each of them then receives those budgets and keeps its own copy in the budget library, where Recover as a new budget brings back anything only that device had.")
            : String(localized: "Sync gets a new key and a new recovery code. The cloud copies made with the old key are deleted, and this device's budgets upload again. Your other devices keep their budgets. To sync again, each one is unlocked with the new recovery code or added from this device.")]
        // Asked from a device that can't unlock: there may be a way that deletes nothing.
        if !sync.unlocked {
            parts.append(String(localized: "If another device of yours still syncs, add this device from it instead. Nothing is deleted that way."))
        }
        return parts.joined(separator: "\n\n")
    }

    private func makeRecoveryCode() {
        run { recoveryCode = RecoveryCode(try await store.service("sync_recovery_create")) ?? recoveryCode }
    }

    private func reset() {
        run {
            recoveryCode = RecoveryCode(try await store.service("sync_reset")) ?? recoveryCode
            typedSecret = ""
            try await sync.syncNow()
        }
    }

    /// Runs one account or sync action: one at a time, with its error shown on the screen.
    private func run(_ action: @escaping @MainActor () async throws -> Void) {
        guard !busy else { return }
        busy = true
        error = nil
        unlockError = nil
        notice = nil
        Task {
            defer { busy = false }
            do { try await action() } catch { self.error = BudgetStore.readable(error) }
            await sync.refreshStatus()
        }
    }
}

/// Moving an account that still syncs on protocol 1 to the new sync (sync.rs `sync_upgrade`).
enum SyncMove {
    /// The signed-in account still syncs on protocol 1, and urbudget.app will move it.
    static func offered(_ status: JSON) -> Bool {
        !status.text("email").isEmpty && status["sync"]["protocol"].int != 2 && status["sync"].flag("upgradeAvailable")
    }

    /// Moving asks for the passphrase only when this device has no saved key that opens the account.
    static func needsPassphrase(_ status: JSON) -> Bool {
        status["sync"].flag("upgradeNeedsPassphrase")
    }

    /// `sync_upgrade`'s parameters: the passphrase only when it's needed.
    static func parameters(_ status: JSON, passphrase: String) -> JSON {
        needsPassphrase(status) ? ["passphrase": .string(passphrase)] : [:]
    }
}

/// Moving is the person's choice: devices on older versions stop syncing until they're updated.
private struct MoveSyncSheet: View {
    var needsPassphrase: Bool
    var submit: (String) async throws -> Void
    @State private var phrase = ""

    var body: some View {
        FormSheet(title: String(localized: "Move to the new sync?"), confirm: String(localized: "Move"),
                  enabled: !needsPassphrase || phrase.count >= 8) {
            try await submit(phrase)
        } content: {
            Section {
                Text("After the move, every device that syncs with this account needs desktop 1.0.11 or later, Android 1.0.1 or later, or iPhone 1.0.1 or later. Older versions are told to update UrBudget before they sync again; the budgets on them stay.")
                if needsPassphrase { SecureField("Sync passphrase", text: $phrase).textContentType(.password) }
            }
        }
        .sheetDetents([.medium, .large])
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
        .sheetDetents([.medium])
    }
}
