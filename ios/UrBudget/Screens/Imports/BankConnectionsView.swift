import SwiftUI

private let checkedAgain = String(localized: "Your budget changed, so these rows were checked again. Look them over, then import.")

/// Nothing to import: every row is already in the budget (BankSync.tsx's nothingNew).
private func nothingNew(_ review: JSON) -> Bool {
    review.text("error").isEmpty && review.rows("rows").allSatisfy { $0.flag("duplicate") }
}

/// One linked account's download, as the screen and its review show it.
private struct SyncEntry: Identifiable {
    var id: String { accountId }
    var accountId: String
    var review: JSON
    var name: String
    var bank: String
    var fresh: Int
    var quiet: Bool
    var error: String
    var bankBalance: String?
    var bankDate: String?
    var credit: Bool

    static func all(_ download: JSON, budget: JSON) -> [SyncEntry] {
        let currency = budget.text("currency")
        var accounts: [String: JSON] = [:]
        for account in budget.rows("accounts") { accounts[account.text("id")] = account }
        return download.rows("reviews").map { review in
            let id = review.text("accountId")
            let account = accounts[id]
            return SyncEntry(accountId: id, review: review, name: account?.text("name") ?? review.text("name"), bank: review.text("name"),
                             fresh: review.rows("rows").count { !$0.flag("duplicate") }, quiet: nothingNew(review), error: review.text("error"),
                             bankBalance: review["balance"].isNull ? nil : money(review.long("balance"), currency),
                             bankDate: review.text("balanceDate").isEmpty ? nil : Money.date(review.text("balanceDate")),
                             credit: account?.text("kind") == "credit")
        }
    }
}

private struct LinkRequest: Identifiable {
    var bank: JSON
    var id: String { bank.text("id") }
}

/// Bank connections through SimpleFIN Bridge (Android's BankConnectionsScreen, BankSync.tsx):
/// connect with a setup token, link each bank account to a budget account, then download activity
/// and review it one account at a time. Nothing is added until it's reviewed.
struct BankConnectionsView: View {
    @Environment(BudgetStore.self) private var store
    @State private var status: JSON?
    @State private var token = ""
    @State private var download: JSON?
    @State private var entries: [SyncEntry] = []
    /// The budget, and its revision, that the downloaded rows were checked against.
    @State private var reviewBudget: JSON = [:]
    @State private var reviewRevision = ""
    /// The revision the last download or check asked about, so a failed check isn't retried forever.
    @State private var attempted = ""
    @State private var busy = false
    @State private var importing = false
    @State private var error: String?
    @State private var notice: String?
    @State private var linking: LinkRequest?
    @State private var unlinking: JSON?
    @State private var confirmDisconnect = false
    // A sync reviews one account at a time; skipped and imported ones are left for later.
    @State private var reviewing = false
    @State private var skipped: Set<String> = []
    @State private var imported: Set<String> = []
    @State private var first: String?

    init() {}

    private var connected: Bool { status?.flag("connected") ?? false }
    private var credentialStore: String { status?.text("store") ?? "" }
    private var bankAccounts: [JSON] { status?.rows("accounts") ?? [] }
    private var through: String { download?.text("through") ?? "" }

    /// Budget accounts by the SimpleFIN account each is linked to.
    private var linkedAccounts: [String: JSON] {
        var linked: [String: JSON] = [:]
        for account in store.budget?.rows("accounts") ?? [] {
            let id = account["simplefin"].text("id")
            if !id.isEmpty { linked[id] = account }
        }
        return linked
    }

    private var pending: [SyncEntry] {
        let open = entries.filter { !$0.quiet && $0.error.isEmpty && !skipped.contains($0.accountId) && !imported.contains($0.accountId) }
        return open.filter { $0.accountId == first } + open.filter { $0.accountId != first }
    }

    var body: some View {
        List {
            Section {
                Text("SimpleFIN Bridge · Nothing is added until you review it.")
                    .font(.subheadline).foregroundStyle(Color.secondary)
                    .listRowBackground(Color.clear)
            }
            if let notice {
                Notice(text: notice, tone: "info").listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            if let error { FormErrorView(message: error) }
            if status == nil {
                if busy {
                    HStack(spacing: 10) { ProgressView(); Text("Checking the connection…").foregroundStyle(Color.secondary) }
                } else {
                    Button("Try again") { loadStatus() }
                }
            } else if !connected {
                connectSection
            } else {
                connectionSection
                accountsSection
            }
            ForEach(messages, id: \.self) { message in
                Notice(text: message, icon: "exclamationmark.triangle.fill").listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            if download != nil { downloadSection }
            if connected {
                Section {
                    Button("Disconnect this device", role: .destructive) { confirmDisconnect = true }.disabled(busy)
                }
            }
        }
        .groupedList()
        .navigationTitle("Bank connections")
        .inlineTitle()
        .overlay(alignment: .top) { if busy && status != nil { ProgressView().padding(8) } }
        .task { if status == nil { loadStatus() } }
        .sheet(item: $linking) { request in
            BankLinkSheet(bank: request.bank) {
                // A changed link changes what a download brings in.
                download = nil
                entries = []
                reviewing = false
            }
        }
        .sheet(isPresented: $reviewing) { reviewSheet }
        .confirmationDialog("Disconnect SimpleFIN on this device?", isPresented: $confirmDisconnect, titleVisibility: .visible) {
            Button("Disconnect", role: .destructive) { disconnect() }
        } message: {
            Text("UrBudget forgets the connection. Linked accounts keep their transactions. SimpleFIN Bridge keeps the bank connection until you remove it there too.")
        }
        .confirmationDialog(unlinkTitle, isPresented: Binding(get: { unlinking != nil }, set: { if !$0 { unlinking = nil } }),
                            titleVisibility: .visible) {
            Button("Unlink", role: .destructive) {
                if let bank = unlinking { unlink(bank) }
                unlinking = nil
            }
        } message: {
            Text("Its budget account keeps the transactions already imported. Downloads stop bringing in this bank account.")
        }
        .onChange(of: store.revision) { _, now in
            // Sync, Undo or another screen changed the budget: check the downloaded rows again.
            guard download != nil, !busy, now != attempted else { return }
            recheck()
        }
    }

    private var unlinkTitle: Text {
        guard let unlinking else { return Text("Unlink this account?") }
        return Text("Unlink \(unlinking.text("name"))?")
    }

    private var messages: [String] {
        var seen: [String] = []
        for message in (status?["messages"].array ?? []) + (download?["messages"].array ?? []) where !message.string.isEmpty {
            if !seen.contains(message.string) { seen.append(message.string) }
        }
        return seen
    }

    // MARK: Sections

    private var connectSection: some View {
        let available = !(status?["store"].isNull ?? true)
        let keptIn = credentialStore.isEmpty ? String(localized: "this device's credential store") : credentialStore
        return Section {
            if !available {
                Notice(text: String(localized: "Bank sync needs a system credential store, which this device doesn't have."), tone: "negative")
                    .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            SecureField("SimpleFIN setup token", text: $token)
                .autocorrectionDisabled()
            Button { connect() } label: { Label("Connect", systemImage: "link") }
                .disabled(!available || busy || token.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            Button { run { _ = try await store.service("simplefin_open") } } label: { Label("Get a setup token", systemImage: "safari") }
                .disabled(busy)
        } header: {
            Text("Connect your bank")
        } footer: {
            Text("Make a setup token at SimpleFIN Bridge and paste it here. It works once. Your bank login stays with SimpleFIN. The connection is kept in \(keptIn), out of budget backups.")
        }
    }

    private var connectionSection: some View {
        let used = status?.long("requestsToday") ?? 0
        let limit = status?.long("dailyLimit") ?? 0
        let requests = String(localized: "\(used) of \(limit) requests used today", locale: .app)
        let linked = !linkedAccounts.isEmpty
        return Section {
            HStack(spacing: 12) {
                BankRowIcon(symbol: "link", tint: .urPositive)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Connected")
                    Text(requests).font(.footnote).foregroundStyle(Color.secondary)
                }
            }
            Button { downloadActivity() } label: {
                BankActionRow(title: String(localized: "Download activity"),
                              detail: linked ? String(localized: "From every linked account, then review one account at a time") : String(localized: "Link a budget account first"),
                              symbol: "icloud.and.arrow.down")
            }
            .disabled(busy || !linked)
            Button { run { status = try await store.service("simplefin_accounts") } } label: {
                BankActionRow(title: String(localized: "Refresh accounts"), detail: String(localized: "Lists your bank accounts and balances again. Uses one request."),
                              symbol: "arrow.clockwise")
            }
            .disabled(busy)
        } footer: {
            Text("Each download uses one request for every 45 days of history it brings in.")
        }
    }

    @ViewBuilder private var accountsSection: some View {
        let linked = linkedAccounts
        if bankAccounts.isEmpty {
            Section {
                Text("SimpleFIN hasn't listed your accounts yet.").foregroundStyle(Color.secondary)
                Button("List my accounts") { run { status = try await store.service("simplefin_accounts") } }.disabled(busy)
            } header: {
                Text("Bank accounts")
            }
        } else {
            Section {
                ForEach(bankAccounts, id: \.idText) { bank in
                    let account = linked[bank.text("id")]
                    Button { linking = LinkRequest(bank: bank) } label: { bankRow(bank, linked: account) }
                        .disabled(busy)
                        .swipeActions(edge: .trailing) {
                            if account != nil {
                                Button("Unlink", role: .destructive) { unlinking = bank }
                            }
                        }
                        .contextMenu {
                            Button { linking = LinkRequest(bank: bank) } label: {
                                Label(account == nil ? "Link to a budget account" : "Change link", systemImage: "link")
                            }
                            if account != nil {
                                Button(role: .destructive) { unlinking = bank } label: { Label("Unlink", systemImage: "minus.circle") }
                            }
                        }
                }
            } header: {
                Text("Bank accounts")
            } footer: {
                Text("Tap one to link it to a budget account.")
            }
        }
    }

    private func bankRow(_ bank: JSON, linked: JSON?) -> some View {
        let subtitle = ImportsText.line([bank.text("org").isEmpty ? nil : bank.text("org"), linkText(linked)].compactMap { $0 })
        return HStack(spacing: 12) {
            BankRowIcon(symbol: "building.columns", tint: linked == nil ? .secondary : .urAccent)
            VStack(alignment: .leading, spacing: 2) {
                Text(bank.text("name")).foregroundStyle(Color.primary)
                Text(subtitle).font(.footnote).foregroundStyle(Color.secondary)
            }
            Spacer(minLength: 8)
            if !bank["balance"].isNull {
                Text(money(bank.long("balance"), bank.text("currency").isEmpty ? store.currency : bank.text("currency")))
                    .monospacedDigit().foregroundStyle(bank.long("balance") < 0 ? Color.urNegative : .primary)
            }
        }
    }

    /// "Linked to Checking · Last sync 2026-10-01", or "Not linked".
    private func linkText(_ linked: JSON?) -> String {
        guard let linked else { return String(localized: "Not linked") }
        let name = linked.text("name")
        let lastSync = linked["simplefin"].text("lastSync")
        guard !lastSync.isEmpty else { return String(localized: "Linked to \(name) · Last sync never") }
        let date = Money.date(lastSync)
        return String(localized: "Linked to \(name) · Last sync \(date)")
    }

    private var downloadSection: some View {
        Section {
            ForEach(entries) { entry in
                let open = !entry.quiet && entry.error.isEmpty && !imported.contains(entry.accountId)
                if open {
                    Button {
                        skipped.remove(entry.accountId)
                        first = entry.accountId
                        notice = nil
                        reviewing = true
                    } label: { entryRow(entry, open: true) }
                    .disabled(busy)
                } else {
                    entryRow(entry, open: false)
                }
            }
            if !pending.isEmpty && !reviewing {
                Button {
                    first = nil
                    reviewing = true
                } label: {
                    Text("Review \(pending.count) accounts")
                        .fontWeight(.semibold).frame(maxWidth: .infinity)
                }
                .disabled(busy)
            }
        } header: {
            Text("Downloaded activity")
        } footer: {
            Text("Through \(Money.date(through)) · \(download?.long("requestsToday") ?? 0) of \(download?.long("dailyLimit") ?? 0) requests today")
        }
    }

    private func entryRow(_ entry: SyncEntry, open: Bool) -> some View {
        let tint: Color = !entry.error.isEmpty ? .urNegative : open ? .urAccent : .urPositive
        return HStack(spacing: 12) {
            BankRowIcon(symbol: !entry.error.isEmpty ? "exclamationmark.triangle" : open ? "checklist" : "checkmark", tint: tint)
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.name).foregroundStyle(Color.primary)
                Text(entryDetail(entry)).font(.footnote).foregroundStyle(Color.secondary)
            }
            Spacer(minLength: 0)
            if open { Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.urTertiary) }
        }
    }

    /// Where a downloaded account is up to, and the bank's balance: "3 new to review · Bank balance 120.00 SAR on 2026-10-21".
    private func entryDetail(_ entry: SyncEntry) -> String {
        let state: String
        let fresh = entry.fresh
        if !entry.error.isEmpty { state = entry.error }
        else if imported.contains(entry.accountId) { state = String(localized: "Reviewed") }
        else if entry.quiet { state = String(localized: "Up to date") }
        else if skipped.contains(entry.accountId) { state = String(localized: "Skipped for now · \(fresh) new", locale: .app) }
        else { state = String(localized: "\(fresh) new to review", locale: .app) }
        guard let amount = entry.bankBalance else { return state }
        let balance: String
        if let date = entry.bankDate { balance = String(localized: "Bank balance \(amount) on \(date)") }
        else { balance = String(localized: "Bank balance \(amount)") }
        return ImportsText.line([state, balance])
    }

    @ViewBuilder private var reviewSheet: some View {
        NavigationStack {
            if let current = pending.first, !busy || importing {
                BankSyncReview(entry: current, through: through, budget: reviewBudget, after: pending.count - 1,
                               ours: ourBalance(current.accountId), importing: importing, error: error, notice: notice,
                               onCancel: { reviewing = false },
                               onSkip: {
                                   skipped.insert(current.accountId)
                                   if first == current.accountId { first = nil }
                                   if pending.isEmpty { reviewing = false }
                               },
                               onImport: { choices, count in importAccount(current, choices: choices, count: count) })
                    .id("\(current.accountId):\(reviewRevision)")
            } else {
                Group {
                    if busy {
                        VStack(spacing: 12) {
                            ProgressView()
                            Text("Checking matches…").foregroundStyle(Color.secondary)
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    } else {
                        EmptyState(title: String(localized: "Nothing left to review"),
                                   detail: error ?? String(localized: "Every downloaded account is reviewed or skipped."), symbol: "checkmark.circle")
                    }
                }
                .navigationTitle("Review bank activity")
                .inlineTitle()
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { reviewing = false } } }
            }
        }
        .interactiveDismissDisabled(busy)
    }

    private func ourBalance(_ id: String) -> String {
        let summary = store.summary?.rows("accounts").first { $0.text("id") == id }
        return money(summary?.long("balance") ?? store.budget?.row("accounts", id: id)?.long("openingBalance") ?? 0, store.currency)
    }

    // MARK: Actions

    /// Runs one connection action at a time, with its error on the screen. Afterwards, if the
    /// budget changed meanwhile (sync, Undo), the downloaded rows are checked again.
    private func run(importing: Bool = false, recheckAfter: Bool = true, _ work: @escaping @MainActor () async throws -> Void) {
        guard !busy else { return }
        busy = true
        self.importing = importing
        error = nil
        Task {
            do { try await work() } catch { self.error = BudgetStore.readable(error) }
            busy = false
            self.importing = false
            if recheckAfter, download != nil, store.revision != attempted { recheck() }
        }
    }

    private func recheck() {
        run(recheckAfter: false) {
            try await sync(refresh: false)
            notice = checkedAgain
            if pending.isEmpty { reviewing = false }
        }
    }

    private func loadStatus() {
        run { status = try await store.service("simplefin_status") }
    }

    private func connect() {
        let pasted = token.trimmingCharacters(in: .whitespacesAndNewlines)
        token = ""
        run { status = try await store.service("simplefin_connect", ["token": .string(pasted)]) }
    }

    private func disconnect() {
        run {
            status = try await store.service("simplefin_disconnect")
            download = nil
            entries = []
            reviewing = false
        }
    }

    private func unlink(_ bank: JSON) {
        let bankId = bank.text("id")
        download = nil
        entries = []
        store.edit { budget in
            var rows = budget["accounts"].array
            for index in rows.indices where rows[index]["simplefin"].text("id") == bankId { rows[index].remove("simplefin") }
            budget["accounts"] = .array(rows)
        }
    }

    /// Downloads the activity of every linked account (one request per 45 days needed) and checks
    /// it against the budget. `refresh: false` checks the last download again, with no request.
    private func sync(refresh: Bool) async throws {
        let revision = store.revision
        let budget = store.budget ?? [:]
        attempted = revision
        let result = try await store.service("simplefin_sync", ["refresh": .bool(refresh), "expectedToken": .string(revision)])
        guard store.revision == revision else {
            throw AppError(String(localized: "The budget changed while downloading. Refresh the bank review before importing."))
        }
        download = result
        reviewBudget = budget
        reviewRevision = revision
        entries = SyncEntry.all(result, budget: budget)
        // The download also lists the accounts again, with today's request count.
        if let fresh = try? await store.service("simplefin_status") { status = fresh }
    }

    private func downloadActivity() {
        skipped = []
        imported = []
        first = nil
        reviewing = false
        notice = nil
        run {
            try await sync(refresh: true)
            guard let download else { return }
            let through = download.text("through")
            // Accounts with nothing new are recorded as checked through the download, one import each
            // with no rows; ones already marked are left alone, so a quiet sync adds no empty undo step.
            let quiet = download.rows("reviews").filter {
                nothingNew($0) && store.budget?.row("accounts", id: $0.text("accountId"))?["simplefin"].text("lastSync") != through
            }.map { $0.text("accountId") }
            if !quiet.isEmpty {
                for id in quiet {
                    _ = try await store.service("simplefin_import", ["account": .string(id), "through": .string(through), "choices": [],
                                                                     "expectedToken": .string(store.revision)], changesBudget: true)
                }
                try await sync(refresh: false)
            }
            if !pending.isEmpty {
                reviewing = true
            } else if entries.allSatisfy({ $0.error.isEmpty }) {
                notice = String(localized: "Nothing new to import.")
            }
        }
    }

    private func importAccount(_ entry: SyncEntry, choices: JSON, count: Int) {
        let through = through
        let revision = reviewRevision
        run(importing: true) {
            _ = try await store.service("simplefin_import", ["account": .string(entry.accountId), "through": .string(through), "choices": choices,
                                                             "expectedToken": .string(revision)], changesBudget: true)
            imported.insert(entry.accountId)
            if first == entry.accountId { first = nil }
            let name = entry.name
            let date = Money.date(through)
            store.showToast(count > 0 ? String(localized: "Imported \(count) transactions into \(name). They're waiting for your approval.", locale: .app)
                                      : String(localized: "\(name) is checked through \(date)."), undo: true)
            importing = false
            // The other accounts' rows are checked against the budget with this import in it.
            try await sync(refresh: false)
            if pending.isEmpty { reviewing = false }
        }
    }
}

// MARK: Rows

private struct BankRowIcon: View {
    var symbol: String
    var tint: Color

    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(tint)
            .frame(width: 34, height: 34)
            .background(tint.opacity(0.14), in: Circle())
            .accessibilityHidden(true)
    }
}

private struct BankActionRow: View {
    var title: String
    var detail: String
    var symbol: String

    var body: some View {
        HStack(spacing: 12) {
            BankRowIcon(symbol: symbol, tint: .urAccent)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                Text(detail).font(.footnote).foregroundStyle(Color.secondary)
            }
        }
    }
}

// MARK: Linking

private enum LinkRoute: Hashable { case account }

/// Which budget account a SimpleFIN account fills, from which day, and whether its signs are reversed.
private struct BankLinkSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var bank: JSON
    var onChange: () -> Void
    @State private var account = ""
    @State private var from = Dates.addDays(Dates.today, -30)
    @State private var flip = false
    @State private var error: String?
    @State private var loaded = false
    @State private var confirmUnlink = false
    @State private var route: [LinkRoute] = []

    private var bankId: String { bank.text("id") }
    private var existing: JSON? { store.budget?.rows("accounts").first { $0["simplefin"].text("id") == bankId } }

    var body: some View {
        let budget = store.budget ?? [:]
        let chosen = budget.row("accounts", id: account)
        let otherLink = chosen.map { $0["simplefin"].text("id") }.flatMap { $0.isEmpty || $0 == bankId ? nil : $0 }
        let lastSync = chosen?.text("id") == existing?.text("id") ? existing?["simplefin"].text("lastSync") ?? "" : ""
        let today = Dates.today
        NavigationStack(path: $route) {
            Form {
                Section {
                    LabeledContent("Bank", value: bank.text("org").isEmpty ? "SimpleFIN" : bank.text("org"))
                    if !bank["balance"].isNull {
                        LabeledContent("Balance", value: money(bank.long("balance"), bank.text("currency").isEmpty ? store.currency : bank.text("currency")))
                    }
                }
                Section {
                    NavigationLink(value: LinkRoute.account) {
                        LabeledContent("Budget account", value: chosen?.text("name") ?? String(localized: "Choose"))
                    }
                    DatePicker("First import from", selection: Binding(get: { Dates.pickerDate(from) }, set: { from = Dates.fromPicker($0) }),
                               in: Dates.pickerDate(Dates.addDays(today, -365))...Dates.pickerDate(today), displayedComponents: .date)
                    Toggle("Reverse bank amount signs", isOn: $flip)
                } footer: {
                    Text(footer(otherLink: otherLink != nil, chosen: chosen?.text("name") ?? "", lastSync: lastSync))
                }
                if existing != nil {
                    Section {
                        Button("Unlink account", role: .destructive) { confirmUnlink = true }
                    }
                }
                if let error { FormErrorView(message: error) }
            }
            .navigationTitle("Link \(bank.text("name"))")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { save() }.fontWeight(.semibold).disabled(account.isEmpty)
                }
            }
            .navigationDestination(for: LinkRoute.self) { _ in
                AccountPicker(selected: account) { id in
                    account = id
                    route.removeAll()
                }
            }
            .confirmationDialog("Unlink \(bank.text("name"))?", isPresented: $confirmUnlink, titleVisibility: .visible) {
                Button("Unlink", role: .destructive) { unlink() }
            } message: {
                Text("Its budget account keeps the transactions already imported. Downloads stop bringing in this bank account.")
            }
        }
        .presentationDetents([.medium, .large])
        .onAppear {
            guard !loaded else { return }
            loaded = true
            guard let existing else { return }
            account = existing.text("id")
            let link = existing["simplefin"]
            if Dates.day(link.text("from")) != nil { from = link.text("from") }
            flip = link.flag("flipSigns")
        }
    }

    private func footer(otherLink: Bool, chosen: String, lastSync: String) -> String {
        var parts = [String(localized: "Up to one year of history, downloaded in 45-day windows. Each window uses one request."),
                     String(localized: "Reverse the signs for a card whose purchases arrive as money in.")]
        if !lastSync.isEmpty {
            let date = Money.date(lastSync)
            parts.append(String(localized: "Synced through \(date); the next download starts a week before that."))
        }
        if otherLink { parts.append(String(localized: "\(chosen) is linked to another bank account now. Linking it here replaces that link.")) }
        return parts.joined(separator: " ")
    }

    private func save() {
        attempt($error) {
            guard Dates.day(from) != nil, from <= Dates.today else { throw AppError(String(localized: "Choose today or an earlier date.")) }
            let currency = bank.text("currency")
            guard currency.isEmpty || currency == store.currency else { throw AppError(String(localized: "This bank account uses a different currency.")) }
            let bankId = bankId
            let accountId = account
            let from = from
            let flip = flip
            store.edit { budget in
                var rows = budget["accounts"].array
                guard let index = rows.firstIndex(where: { $0.text("id") == accountId }) else { throw AppError(String(localized: "Choose an account.")) }
                // The same account staying linked keeps its last sync, so the next download continues from there.
                var link: JSON = rows[index]["simplefin"].text("id") == bankId ? rows[index]["simplefin"] : [:]
                link["id"] = .string(bankId)
                link["from"] = .string(from)
                link["flipSigns"] = .bool(flip)
                for other in rows.indices where other != index && rows[other]["simplefin"].text("id") == bankId { rows[other].remove("simplefin") }
                rows[index]["simplefin"] = link
                budget["accounts"] = .array(rows)
            }
            onChange()
            dismiss()
        }
    }

    private func unlink() {
        let bankId = bankId
        store.edit { budget in
            var rows = budget["accounts"].array
            for index in rows.indices where rows[index]["simplefin"].text("id") == bankId { rows[index].remove("simplefin") }
            budget["accounts"] = .array(rows)
        }
        onChange()
        dismiss()
    }
}

// MARK: Review

/// One account's downloaded rows (BankSync.tsx): what each does, the bank's balance beside
/// UrBudget's, and Skip this account to come back to it later.
private struct BankSyncReview: View {
    var entry: SyncEntry
    var through: String
    var budget: JSON
    var after: Int
    var ours: String
    var importing: Bool
    var error: String?
    var notice: String?
    var onCancel: () -> Void
    var onSkip: () -> Void
    var onImport: (JSON, Int) -> Void
    @State private var rows: [BankReviewRow]?
    @State private var summary: BankReviewSummary?
    @State private var decisions: [Int: String] = [:]
    @State private var problem: String?

    var body: some View {
        let counts = rows.map { BankReview.counts($0, decisions) } ?? (importing: 0, undecided: 0)
        List {
            headerSection
            if let notice {
                Notice(text: notice, tone: "info").listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            if let message = error ?? problem { FormErrorView(message: message) }
            if let summary {
                if summary.allIn && entry.credit {
                    Notice(text: String(localized: "Every new row from this card came in as money in. If these are purchases, the bank sends signs the other way round: turn on Reverse bank amount signs for its link."),
                           icon: "arrow.up.arrow.down")
                        .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                }
                BankReviewNotes(summary: summary, undecided: counts.undecided)
            }
            if let rows {
                if rows.isEmpty {
                    Section {
                        Text("No new transactions. Confirm to record the account as checked through \(Money.date(through)).").foregroundStyle(Color.secondary)
                    }
                } else {
                    Section {
                        BankReviewRows(rows: rows, decisions: $decisions, enabled: !importing)
                    } header: {
                        BankReviewHeader(rows: rows, decisions: $decisions, undecided: counts.undecided, enabled: !importing)
                    } footer: {
                        Text(BankReview.matchingNote)
                    }
                }
            } else if problem == nil {
                HStack(spacing: 10) { ProgressView(); Text("Checking matches…").foregroundStyle(Color.secondary) }
            }
        }
        .groupedList()
        .navigationTitle("Review bank activity")
        .inlineTitle()
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel", action: onCancel).disabled(importing) }
            ToolbarItem(placement: .confirmationAction) {
                if importing {
                    ProgressView()
                } else {
                    Button(counts.importing > 0 ? "Import \(counts.importing)" : "Mark checked") { submit(count: counts.importing) }
                        .fontWeight(.semibold)
                        .disabled(rows == nil || counts.undecided > 0 || !entry.error.isEmpty)
                }
            }
            ToolbarItem(placement: .bottomBarItems) {
                Button("Skip this account", action: onSkip).disabled(importing)
            }
        }
        .task { await prepare() }
    }

    private var headerSection: some View {
        let parts = [summary.map(BankReview.countsText), after > 0 ? String(localized: "\(after) more after this", locale: .app) : nil].compactMap { $0 }
        let details = parts.isEmpty ? String(localized: "Checking matches…") : ImportsText.line(parts)
        return Section {
            VStack(alignment: .leading, spacing: 2) {
                Text("\(entry.bank.isEmpty ? "SimpleFIN" : entry.bank) into \(entry.name)").font(.headline)
                Text(details).font(.footnote).foregroundStyle(Color.secondary)
            }
            HStack(alignment: .top) {
                Figure(label: bankLabel, value: entry.bankBalance ?? String(localized: "Not sent"))
                Spacer(minLength: 12)
                Figure(label: String(localized: "UrBudget, before this import"), value: ours, alignment: .trailing)
            }
        }
    }

    private var bankLabel: String {
        guard let date = entry.bankDate else { return String(localized: "At the bank") }
        return String(localized: "At the bank on \(date)")
    }

    private func prepare() async {
        let review = entry.review
        let budget = budget
        do {
            let prepared = try await Task.detached(priority: .userInitiated) {
                try BankReview.prepare(rows: review.rows("rows"), budget: budget, allowIdlessDuplicate: false, candidateSets: review["candidateSets"].array)
            }.value
            rows = prepared
            summary = BankReview.summarize(prepared)
        } catch is CancellationError {
        } catch {
            problem = BudgetStore.readable(error)
        }
    }

    private func submit(count: Int) {
        guard let rows else { return }
        let selected = decisions
        Task {
            let choices = await Task.detached(priority: .userInitiated) { BankReview.choices(rows, selected, allowIdlessDuplicate: false) }.value
            onImport(choices, count)
        }
    }
}
