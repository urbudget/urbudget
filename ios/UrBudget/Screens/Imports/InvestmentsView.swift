import SwiftUI
import UniformTypeIdentifiers

/// Interactive Brokers, through its own Flex Web Service (crates/urbudget-services ibkr.rs; the
/// desktop's IbkrSettings): three steps make a Flex Query and a read-only token and paste them
/// here, and each account
/// there fills an Investment account here with what it holds and what it's worth. The token is
/// kept on this device, never in the budget; what a sync brings is in the budget, so every
/// device shows it. A statement saved from Client Portal as a file reads the same way.
struct InvestmentsView: View {
    @Environment(BudgetStore.self) private var store
    @State private var status: JSON?
    /// The last statement's accounts against the budget (`ibkr_review_compact`), and the budget
    /// revision they were checked against.
    @State private var review: JSON?
    @State private var reviewRevision = ""
    @State private var query = ""
    @State private var token = ""
    /// Where each account that fills nothing yet goes: `Investments.newAccount`, a tracking
    /// account's id, or "" for nowhere.
    @State private var into: [String: String] = [:]
    @State private var rates: [String: String] = [:]
    /// What's running: "status", "connect", "file", "sync", "unlink" or "remove".
    @State private var working: String?
    @State private var error: String?
    @State private var help = false
    @State private var picking = false
    @State private var confirmRemove = false
    @State private var unlinking: IdentifiedText?
    @State private var deposits: DepositsRequest?
    /// The fields for a second login at Interactive Brokers are showing.
    @State private var another = false
    /// Someone who brought in a file is pasting a token after all.
    @State private var withToken = false
    /// The login being asked about before it goes, by its query id.
    @State private var removingLogin: IdentifiedText?
    /// The request for Client Portal's query builder was copied.
    @State private var copied = false
    /// Sync once a day when the app opens (Investments.dailySync).
    @AppStorage(Investments.dailyKey) private var daily = true

    init() {}

    private var busy: Bool { working != nil }
    private var connected: Bool { status?.flag("connected") ?? false }
    private var hasStore: Bool { !(status?["store"].isNull ?? true) }
    /// The statement's accounts, each with the budget account it fills, if any. One the budget
    /// keeps from another device's login (`kept`) is on its own page, not here.
    private var accounts: [JSON] { (review?.rows("accounts") ?? []).filter { !$0.flag("kept") } }
    /// Accounts a broker fills, as the budget keeps them: all there is to show without a statement.
    private var filled: [JSON] { (store.budget?.rows(Investments.key) ?? []).filter { $0.text("source") == "ibkr" } }

    private var targets: [JSON] {
        var balances: [String: Int64] = [:]
        for row in store.summary?.rows("accounts") ?? [] { balances[row.text("id")] = row.long("balance") }
        return Investments.targets(store.budget ?? [:], balances: balances)
    }

    var body: some View {
        let targets = targets
        let adding = Investments.adding(accounts, into: into, targets: targets)
        AppList {
            if let error { FormErrorView(message: error) }
            if status == nil {
                if busy {
                    HStack(spacing: 10) { ProgressView(); Text("Checking the connection…").foregroundStyle(Color.secondary) }
                } else {
                    Button("Try again") { loadStatus() }
                }
            } else if accounts.isEmpty {
                if connected {
                    lead(String(localized: "Connected. Interactive Brokers makes each day's statement overnight, so a sync brings holdings and prices as of the last close."))
                    refusedLogins
                    filledSection
                    syncSection(adding: [])
                    loginsSection
                    dailySection
                    anotherLogin
                } else {
                    lead(filled.isEmpty
                         ? String(localized: "See what your Interactive Brokers account holds and what it's worth, as part of your net worth. UrBudget reads your statements straight from Interactive Brokers with a token that can only read. It can't trade or move money.")
                         : String(localized: "These accounts were filled from Interactive Brokers, but this device has no token for it. Connect here to sync them, or bring in a statement file."))
                    filledSection
                    setupSections
                }
                moreSection
            } else {
                lead(connected
                     ? String(localized: "Connected. Interactive Brokers makes each day's statement overnight, so a sync brings holdings and prices as of the last close.")
                     : String(localized: "From a statement file. Connect with a token to update without choosing a file each time."))
                refusedLogins
                ForEach(accounts, id: \.idText) { account in
                    accountSection(account, targets: targets)
                }
                notices
                syncSection(adding: adding)
                if connected {
                    loginsSection
                    dailySection
                    anotherLogin
                } else {
                    tokenAfterFile
                }
                moreSection
            }
        }
        .groupedList()
        .navigationTitle("Interactive Brokers")
        .inlineTitle()
        .task { if status == nil { loadStatus() } }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.xml, .text, .data]) { read($0) }
        .sheet(item: $deposits) { request in
            DepositsReview(request: request) { recheck() }
        }
        .confirmationDialog("Remove the connection?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button("Remove connection", role: .destructive) { disconnect() }
        } message: {
            Text("UrBudget forgets the token on this device. Your accounts keep what they hold and stop updating. To switch the token off, do that in Client Portal too.")
        }
        .confirmationDialog("Remove this login?", isPresented: Binding(get: { removingLogin != nil }, set: { if !$0 { removingLogin = nil } }), titleVisibility: .visible) {
            Button("Remove login", role: .destructive) {
                if let login = removingLogin?.value { disconnect(login: login) }
                removingLogin = nil
            }
        } message: {
            Text("UrBudget forgets its token on this device. Its accounts keep what they hold and stop updating. To switch the token off, do that in Client Portal too.")
        }
        .confirmationDialog(unlinkTitle, isPresented: Binding(get: { unlinking != nil }, set: { if !$0 { unlinking = nil } }), titleVisibility: .visible) {
            Button("Unlink", role: .destructive) {
                if let account = unlinking?.value { unlink(account) }
                unlinking = nil
            }
        } message: {
            Text("The account keeps its transactions and its balance. It stops following Interactive Brokers.")
        }
        .onChange(of: store.revision) { _, now in
            // Sync, Undo or another screen changed the budget: check the statement against it again.
            guard review != nil, !busy, now != reviewRevision else { return }
            recheck()
        }
    }

    private var unlinkTitle: Text {
        let name = store.budget?.row("accounts", id: unlinking?.value ?? "")?.text("name") ?? ""
        return name.isEmpty ? Text("Unlink this account?") : Text("Unlink \(name)?")
    }

    // MARK: Sections

    private func lead(_ text: String) -> some View {
        Section {
            Text(text).font(.subheadline).foregroundStyle(Color.secondary).listRowBackground(Color.clear)
        }
    }

    /// What to do in Client Portal, step by step, with the two things to paste at the step they
    /// come from.
    @ViewBuilder private var setupSections: some View {
        let keptIn = status?.text("store") ?? ""
        let pasted = !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !token.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        Section {
            Text("In Client Portal, open Performance & Reports › Flex Queries and add an Activity Flex Query. If the page offers Configure Query with AI, paste this request and save what it builds:")
                .font(.subheadline)
            // The request is written to Client Portal in English, left to right whatever the interface.
            VStack(alignment: .leading, spacing: 8) {
                Text(verbatim: Investments.flexRequest).font(.footnote).foregroundStyle(Color.secondary)
                    .multilineTextAlignment(.leading).frame(maxWidth: .infinity, alignment: .leading)
                    .environment(\.layoutDirection, .leftToRight)
                Button(copied ? String(localized: "Copied") : String(localized: "Copy"), systemImage: copied ? "checkmark" : "doc.on.doc") {
                    UIPasteboard.general.string = Investments.flexRequest
                    copied = true
                }
                .font(.subheadline.weight(.semibold)).buttonStyle(.borderless)
            }
            DisclosureGroup("Or set it up by hand") {
                VStack(alignment: .leading, spacing: 6) {
                    GuideLine(marker: "1.", text: Text("Give the query a name, such as UrBudget."), small: true)
                    GuideLine(marker: "2.", text: Text("Under Sections, tick each of these and choose Select All in it:"), small: true)
                    // Client Portal's own names, in English whatever the interface.
                    VStack(alignment: .leading, spacing: 3) {
                        GuideLine(marker: "•", text: Text(verbatim: "Account Information"), small: true)
                        GuideLine(marker: "•", text: Text(verbatim: "Cash Report"), small: true)
                        GuideLine(marker: "•", text: Text(verbatim: "Cash Transactions"), small: true)
                        GuideLine(marker: "•", text: Text(verbatim: "Net Asset Value (NAV) in Base"), small: true)
                        GuideLine(marker: "•", text: Text("Open Positions, choosing Summary"), small: true)
                        GuideLine(marker: "•", text: Text("Trades, choosing Executions"), small: true)
                        GuideLine(marker: "•", text: Text(verbatim: "Transfers"), small: true)
                    }
                    .padding(.leading, 20)
                    GuideLine(marker: "3.", text: Text("Under Delivery Configuration, set Format to XML and Period to Last 365 Calendar Days."), small: true)
                    GuideLine(marker: "4.", text: Text("Under General Configuration, set Include Currency Rates to Yes."), small: true)
                    GuideLine(marker: "5.", text: Text("Save it with Continue, then Create."), small: true)
                }
            }
        } header: {
            SetupStepHeader(number: 1, title: String(localized: "Make a Flex Query"))
        }
        Section {
            VStack(alignment: .leading, spacing: 6) {
                GuideLine(marker: "•", text: Text("On the same page, open Flex Web Service Configuration and turn the service on."))
                GuideLine(marker: "•", text: Text("Generate a token. Choose one year so it lasts, and leave the IP address restriction empty."))
            }
        } header: {
            SetupStepHeader(number: 2, title: String(localized: "Make a token"))
        }
        Section {
            VStack(alignment: .leading, spacing: 6) {
                GuideLine(marker: "•", text: Text("Query ID: on the Flex Queries page, the info (i) icon beside your query opens a box with the Query ID at the top."))
                GuideLine(marker: "•", text: Text("Token: the one you just generated."))
            }
            TextField("Query ID", text: $query).numberKeyboard().plainTextEntry()
            SecureField("Token", text: $token).autocorrectionDisabled()
            Button { connect() } label: {
                Label(working == "connect" ? String(localized: "Connecting…") : String(localized: "Connect"), systemImage: "link")
            }
            .disabled(!hasStore || busy || !pasted)
            if working == "connect" {
                HStack(spacing: 10) {
                    ProgressView()
                    Text("Interactive Brokers is preparing your statement. This can take a minute.").font(.footnote).foregroundStyle(Color.secondary)
                }
            }
        } header: {
            SetupStepHeader(number: 3, title: String(localized: "Paste both here"))
        } footer: {
            Text(hasStore ? String(localized: "UrBudget keeps the token in \(keptIn), never in the budget file.")
                          : String(localized: "This device has no credential store to keep a token in. You can still bring in a statement file."))
        }
    }

    /// A statement file brought these accounts in: a token keeps them up to date without one.
    @ViewBuilder private var tokenAfterFile: some View {
        Section {
            Button(withToken ? String(localized: "Cancel") : String(localized: "Connect with a token")) { withToken.toggle() }
                .disabled(busy)
        }
        if withToken { setupSections }
    }

    /// The logins, once there is more than one: each is removed on its own.
    @ViewBuilder private var loginsSection: some View {
        let logins = status?.rows("logins") ?? []
        if logins.count > 1 {
            Section {
                ForEach(Array(logins.enumerated()), id: \.offset) { _, login in
                    let id = login.text("query")
                    let name = Investments.loginName(login, accounts: accounts)
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(name ?? String(localized: "Query \(id)"))
                            if name != nil { Text("Query \(id)").font(.footnote).foregroundStyle(Color.secondary) }
                        }
                        Spacer(minLength: 8)
                        Button("Remove", role: .destructive) { removingLogin = IdentifiedText(value: id) }
                            .buttonStyle(.borderless).disabled(busy)
                    }
                }
            } header: {
                Text("Logins")
            }
        }
    }

    /// Whether the app syncs by itself, once a day, when it opens.
    private var dailySection: some View {
        Section {
            Toggle("Sync once a day when UrBudget opens", isOn: $daily)
        }
    }

    /// Someone with a second login at Interactive Brokers pastes its query and token too.
    @ViewBuilder private var anotherLogin: some View {
        Section {
            Button(another ? String(localized: "Cancel") : String(localized: "Add another login")) { another.toggle() }
                .disabled(busy)
        } footer: {
            if another {
                Text("For a second login at Interactive Brokers, sign in to Client Portal with it and make a query and a token there too. Both logins then sync together.")
            }
        }
        if another { setupSections }
    }

    /// Logins that couldn't be fetched the last time they were asked, each with why.
    @ViewBuilder private var refusedLogins: some View {
        let refused = (status?.rows("logins") ?? []).filter { !$0.text("error").isEmpty }
        ForEach(Array(refused.enumerated()), id: \.offset) { _, login in
            Notice(text: String(localized: "Query \(login.text("query")): \(login.text("error"))"))
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
    }

    /// Everything else, folded away: what the token can do and when the app syncs, reading a
    /// file instead, and forgetting the connection.
    private var moreSection: some View {
        Section {
            DisclosureGroup(isExpanded: $help) {
                ForEach(Array(Self.how.enumerated()), id: \.offset) { _, line in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(verbatim: "•").foregroundStyle(Color.secondary)
                        Text(line)
                    }
                    .font(.footnote)
                }
                Button { run("guide") { _ = try await store.service("ibkr_open") } } label: {
                    Label("Interactive Brokers' guide", systemImage: "safari")
                }
                .font(.footnote)
            } label: {
                Text("How it works")
            }
            Button { picking = true } label: { Label("Use a statement file", systemImage: "doc.text") }
                .disabled(busy)
            if connected {
                // With several logins each is removed on its own line above.
                if (status?.rows("logins").count ?? 0) < 2 {
                    Button("Remove connection", role: .destructive) { confirmRemove = true }.disabled(busy)
                }
            } else if status?.rows("accounts").isEmpty == false {
                Button("Start over") { disconnect() }.disabled(busy)
            }
        }
    }

    private static let how = [
        String(localized: "The token can only read statements. It can't trade or move money, and it stays on this device."),
        String(localized: "A sync asks Interactive Brokers for your query's statement. It makes each day's statement overnight, so prices are as of the last close."),
        String(localized: "UrBudget syncs when you tap Sync, and once a day when it opens."),
        String(localized: "The account's balance follows Interactive Brokers' value, so your net worth and reports include it. Each month's change is added to the account as Market change."),
    ]

    /// The accounts a broker fills, from the budget alone: shown until a statement says more.
    @ViewBuilder private var filledSection: some View {
        if !filled.isEmpty {
            Section {
                ForEach(Array(filled.enumerated()), id: \.offset) { _, held in
                    let id = held.text("accountId")
                    let name = store.budget?.row("accounts", id: id)?.text("name") ?? held.text("sourceAccount")
                    let count = held.rows("positions").count
                    HStack(spacing: 12) {
                        BrokerIcon(tint: .urAccent)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(name)
                            Text(ImportsText.line([held.text("sourceAccount"), String(localized: "\(count) holdings", locale: .app), Money.date(held.text("asOf"))]))
                                .font(.footnote).foregroundStyle(Color.secondary)
                        }
                        Spacer(minLength: 8)
                        Text(money(held.long("total"), store.currency)).monospacedDigit().lineLimit(1).layoutPriority(1)
                        unlinkMenu(id)
                    }
                }
            }
        }
    }

    /// Unlinking, folded behind the row's own menu: it's rare, and it isn't the screen's action.
    private func unlinkMenu(_ account: String) -> some View {
        Menu {
            Button(role: .destructive) { unlinking = IdentifiedText(value: account) } label: { Label("Unlink", systemImage: "minus.circle") }
        } label: {
            Image(systemName: "ellipsis.circle").foregroundStyle(Color.secondary)
        }
        .accessibilityLabel("More")
        .disabled(busy)
    }

    /// One account in the statement: what it is, where it goes, and the rate it needs if any.
    private func accountSection(_ account: JSON, targets: [JSON]) -> some View {
        let id = account.text("id")
        let alias = account.text("alias")
        let linked = account.text("accountId")
        let chosen = Investments.target(account, into: into, targets: targets)
        let fresh = Investments.fresh(account)
        let positions = Int(account.long("positions"))
        let about = ImportsText.line([alias.isEmpty ? nil : id, String(localized: "\(positions) holdings", locale: .app),
                                     account.text("asOf").isEmpty ? nil : Money.date(account.text("asOf"))].compactMap { $0 })
        let currency = account.text("currency").isEmpty ? store.currency : account.text("currency")
        return Section {
            HStack(spacing: 12) {
                BrokerIcon(tint: linked.isEmpty ? .secondary : .urAccent)
                VStack(alignment: .leading, spacing: 2) {
                    Text(alias.isEmpty ? id : alias)
                    Text(about).font(.footnote).foregroundStyle(Color.secondary)
                }
                Spacer(minLength: 8)
                Text(money(account.long("value"), currency)).monospacedDigit().lineLimit(1).layoutPriority(1)
            }
            if linked.isEmpty {
                Picker("Goes into", selection: Binding(get: { chosen }, set: { into[id] = $0 })) {
                    Text("A new investment account").tag(Investments.newAccount)
                    ForEach(targets, id: \.idText) { target in
                        Text(target.text("name")).tag(target.text("id"))
                    }
                    Text("Don't add").tag("")
                }
                .disabled(busy)
            } else {
                HStack(spacing: 12) {
                    Text("Goes into")
                    Spacer(minLength: 8)
                    Text(store.budget?.row("accounts", id: linked)?.text("name") ?? "").foregroundStyle(Color.secondary).lineLimit(1)
                    unlinkMenu(linked)
                }
                if fresh > 0 {
                    Button {
                        if let budgetAccount = store.budget?.row("accounts", id: linked) {
                            deposits = DepositsRequest(account: budgetAccount, review: account, revision: reviewRevision)
                        }
                    } label: {
                        Label { Text("Review \(fresh) deposits") } icon: { Image(systemName: "checklist") }
                    }
                    .disabled(busy)
                }
            }
            if let pair = Investments.needsRate(account), !linked.isEmpty || !chosen.isEmpty {
                LabeledContent {
                    TextField(String(localized: "Exchange rate"), text: Binding(get: { rates[id] ?? "" }, set: { rates[id] = $0 }), prompt: Text(verbatim: "0.00"))
                        .decimalKeyboard().monospacedDigit().multilineTextAlignment(AppLanguage.isRightToLeft ? .leading : .trailing)
                        .amountDirection()
                        .frame(maxWidth: 150)
                } label: {
                    Text("1 \(pair.from) in \(pair.to)").layoutPriority(1)
                }
            }
            if !account.text("error").isEmpty { FormErrorView(message: account.text("error")) }
        }
    }

    /// What needs saying about the statement: a rate the person gives, sections the query leaves out.
    @ViewBuilder private var notices: some View {
        if accounts.contains(where: { Investments.needsRate($0) != nil }) {
            Notice(text: String(localized: "The statement has no exchange rate for this budget's currency, so UrBudget uses the one you give for every day. Change it here when it moves."),
                   tone: "info")
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
        if let missing = accounts.map({ $0["missing"].array.map(\.string) }).first(where: { !$0.isEmpty }) {
            let sections = missing.joined(separator: ", ")
            Notice(text: String(localized: "Your Flex Query leaves out \(sections). UrBudget shows what it can; tick them in Client Portal for the whole picture."))
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
    }

    /// The screen's one action once there's a statement or a token: add what's new, else sync.
    private func syncSection(adding: [JSON]) -> some View {
        let ready = Investments.ready(accounts, into: into, rates: rates, targets: targets)
        return Section {
            Button {
                bring(fetch: adding.isEmpty && connected)
            } label: {
                HStack(spacing: 8) {
                    if working == "sync" { ProgressView() }
                    Text(adding.isEmpty ? (working == "sync" ? String(localized: "Syncing…") : String(localized: "Sync now"))
                                        : (working == "sync" ? String(localized: "Adding…") : String(localized: "Add to my budget")))
                        .fontWeight(.semibold)
                }
                .frame(maxWidth: .infinity)
            }
            .primaryAction()
            // Without a token there is nothing to fetch: the next statement comes from a file.
            .disabled(busy || !ready || (adding.isEmpty && !connected))
        } footer: {
            if working == "sync" && adding.isEmpty && connected {
                Text("Interactive Brokers is preparing your statement. This can take a minute.")
            }
        }
    }

    // MARK: Actions

    /// Runs one action at a time, with its error on the screen.
    private func run(_ what: String, _ work: @escaping @MainActor () async throws -> Void) {
        guard !busy else { return }
        working = what
        error = nil
        Task {
            do { try await work() } catch { self.error = BudgetStore.readable(error) }
            working = nil
        }
    }

    /// Takes a status in, and the statement it holds against the budget.
    private func show(_ next: JSON) async throws {
        status = next
        if next.rows("accounts").isEmpty { review = nil } else { _ = try await check() }
    }

    @discardableResult
    private func check() async throws -> JSON {
        let revision = store.revision
        let next = try await store.service("ibkr_review_compact")
        review = next
        reviewRevision = revision
        return next
    }

    private func recheck() {
        run("check") { _ = try await check() }
    }

    private func loadStatus() {
        run("status") { try await show(try await store.service("ibkr_status")) }
    }

    private func connect() {
        let pastedToken = token
        let pastedQuery = query
        run("connect") {
            try await show(try await store.service("ibkr_connect", ["token": .string(pastedToken), "query": .string(pastedQuery)]))
            token = ""
            query = ""
            another = false
            withToken = false
        }
    }

    /// Forgets one login, by its query id, or the whole connection.
    private func disconnect(login: String? = nil) {
        run("remove") {
            let params: JSON = login.map { JSON.object(["query": .string($0)]) } ?? [:]
            try await show(try await store.service("ibkr_disconnect", params))
            if login == nil {
                into = [:]
                rates = [:]
            }
        }
    }

    private func read(_ result: Result<URL, Error>) {
        guard case .success(let url) = result else { return }
        run("file") {
            let content = try await Task.detached(priority: .userInitiated) {
                let file = try ImportFiles.read(url, limit: 32 * 1024 * 1024,
                                                tooLarge: String(localized: "Choose a statement file smaller than 32 MB."))
                return try ImportFiles.text(file.data)
            }.value
            try await show(try await store.service("ibkr_file", ["content": .string(content)]))
        }
    }

    /// Fetches the statement (when there's a token and nothing new to add) and brings it into the
    /// budget: every linked account, and the new ones where they were told to go.
    private func bring(fetch: Bool) {
        let targets = targets
        let links = Investments.links(accounts, into: into, rates: rates, targets: targets)
        let count = Investments.adding(accounts, into: into, targets: targets).count
        run("sync") {
            if fetch { status = try await store.service("ibkr_fetch") }
            _ = try await store.service("ibkr_apply", ["links": .array(links)], changesBudget: true)
            let next = try await check()
            status = try await store.service("ibkr_status")
            if let failed = next.rows("accounts").first(where: { !$0.text("accountId").isEmpty && !$0.text("error").isEmpty }) {
                error = failed.text("error")
            } else if count > 0 {
                store.showToast(String(localized: "Added \(count) accounts from Interactive Brokers.", locale: .app), undo: true)
            } else {
                store.showToast(String(localized: "Your Interactive Brokers accounts are up to date."))
            }
        }
    }

    private func unlink(_ account: String) {
        run("unlink") {
            try await store.editing { budget in Investments.unlink(&budget, account: account) }
            if review != nil { _ = try await check() }
        }
    }
}

/// A step of the setup, over what it asks for: its number and what to do.
/// One part of a setup step, behind its marker: "1." where the order matters, "•" where it
/// doesn't. Markers are kept left to right, so "1." doesn't read ".1" in Arabic.
private struct GuideLine: View {
    var marker: String
    var text: Text
    var small = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(verbatim: AppLanguage.leftToRight(marker)).foregroundStyle(Color.secondary)
                .frame(minWidth: 14, alignment: .leading)
            text.foregroundStyle(small ? Color.secondary : Color.primary)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .font(small ? .footnote : .subheadline)
    }
}

private struct SetupStepHeader: View {
    var number: Int
    var title: String

    var body: some View {
        HStack(spacing: 8) {
            Text(verbatim: "\(number)").font(.footnote.weight(.semibold)).monospacedDigit().foregroundStyle(Color.urAccent)
                .frame(width: 22, height: 22).background(Color.urAccent.opacity(0.14), in: Circle())
            Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(Color.primary)
        }
        .textCase(nil)
        .accessibilityElement(children: .combine)
    }
}

private struct BrokerIcon: View {
    var tint: Color

    var body: some View {
        Image(systemName: "chart.line.uptrend.xyaxis")
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(tint)
            .frame(width: 34, height: 34)
            .background(tint.opacity(0.14), in: Circle())
            .accessibilityHidden(true)
    }
}

/// The deposits, withdrawals and transfers the broker listed for one account, reviewed like a
/// bank's rows (the desktop's Deposits): each is matched to the transfer already in the budget,
/// joined to its other side, or added (as a transfer with the account chosen for it, or with
/// none), so it counts as money put in and not as a gain.
struct DepositsReview: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var request: DepositsRequest
    /// Called once the rows are in the budget, before the sheet closes.
    var onDone: () -> Void
    @State private var rows: [BankReviewRow]?
    @State private var summary: BankReviewSummary?
    @State private var decisions: [Int: String] = [:]
    @State private var problem: String?
    @State private var importing = false

    var body: some View {
        let counts = rows.map { BankReview.counts($0, decisions) } ?? (importing: 0, undecided: 0)
        NavigationStack {
            AppList {
                Section {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(request.account.text("name")).font(.headline)
                        Text("Money you moved into or out of Interactive Brokers. Each one is matched to the transfer already in your budget, joined to its other side in the account it came from, or added as a transfer from the account you choose. Either way it counts as money you put in, not as a gain.")
                            .font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
                if let problem { FormErrorView(message: problem) }
                if let summary { BankReviewNotes(summary: summary, undecided: counts.undecided) }
                if let rows {
                    Section {
                        BankReviewRows(rows: rows, decisions: $decisions, enabled: !importing)
                    } header: {
                        BankReviewHeader(rows: rows, decisions: $decisions, undecided: counts.undecided, enabled: !importing)
                    } footer: {
                        Text(BankReview.matchingNote)
                    }
                } else if problem == nil {
                    HStack(spacing: 10) { ProgressView(); Text("Checking matches…").foregroundStyle(Color.secondary) }
                }
            }
            .groupedList()
            .navigationTitle("Deposits and transfers")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(importing) }
                ToolbarItem(placement: .confirmationAction) {
                    if importing {
                        ProgressView()
                    } else {
                        Button("Import \(counts.importing)") { submit(count: counts.importing) }
                            .fontWeight(.semibold)
                            .disabled(rows == nil || counts.undecided > 0 || counts.importing == 0)
                    }
                }
            }
            .task { await prepare() }
        }
        .interactiveDismissDisabled(importing)
    }

    private func prepare() async {
        let review = request.review
        let budget = store.budget ?? [:]
        let account = request.account.text("id")
        // The broker says money moved, not where from: any other account can be named as its other side.
        let plain = String(localized: "Add with no other account")
        do {
            let prepared = try await Task.detached(priority: .userInitiated) {
                try BankReview.prepare(rows: review.rows("rows"), budget: budget, allowIdlessDuplicate: false, candidateSets: review["candidateSets"].array,
                                       others: Investments.others(budget, account: account), plain: plain)
            }.value
            rows = prepared
            summary = BankReview.summarize(prepared)
        } catch is CancellationError {
        } catch {
            problem = BudgetStore.readable(error)
        }
    }

    private func submit(count: Int) {
        guard let rows, !importing else { return }
        importing = true
        problem = nil
        let selected = decisions
        let account = request.account.text("id")
        let revision = request.revision
        Task {
            let choices = await Task.detached(priority: .userInitiated) { BankReview.choices(rows, selected, allowIdlessDuplicate: false) }.value
            do {
                _ = try await store.service("ibkr_import", ["account": .string(account), "choices": choices, "expectedToken": .string(revision)],
                                            changesBudget: true)
                store.showToast(String(localized: "\(count) deposits are in your budget.", locale: .app), undo: true)
                onDone()
                dismiss()
            } catch {
                problem = BudgetStore.readable(error)
            }
            importing = false
        }
    }
}
