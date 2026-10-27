import SwiftUI
import UniformTypeIdentifiers

/// YNAB's words for the totals an import checks, and the conflict fields, as the desktop shows them.
enum ImportYnabText {
    private static let fields = ["balance": String(localized: "balance"), "cleared_balance": String(localized: "cleared balance"),
                                 "uncleared_balance": String(localized: "uncleared balance"), "budgeted": String(localized: "assigned"),
                                 "activity": String(localized: "activity")]
    private static let conflictFields = ["splits": String(localized: "split lines"), "status": String(localized: "cleared"),
                                         "approved": String(localized: "approval"), "transferStatus": String(localized: "transfer's cleared"),
                                         "accountId": String(localized: "account")]

    /// A total that differs from YNAB's: "Car Loan balance: −10,600.00 here, −10,831.27 in YNAB".
    static func difference(_ d: JSON, currency: String) -> String {
        let actual = money(d.long("actual"), currency)
        let expected = money(d.long("expected"), currency)
        let amounts = String(localized: "\(actual) here, \(expected) in YNAB")
        let month = d.text("month").isEmpty ? "" : Dates.monthLabel(String(d.text("month").prefix(7)))
        let field = d.text("field")
        var kind = d.text("kind")
        if kind.isEmpty { kind = !d.text("account").isEmpty ? "account" : !d.text("category").isEmpty ? "category" : "readyToAssign" }
        switch kind {
        case "account":
            return accountDifference(d.text("account"), field: field, amounts: amounts)
        case "category":
            return categoryDifference([d.text("category"), month].filter { !$0.isEmpty }, field: field, amounts: amounts)
        default:
            if month.isEmpty { return String(localized: "Ready to Assign: \(amounts)") }
            return String(localized: "Ready to Assign, \(month): \(amounts)")
        }
    }

    /// "Car Loan balance: …", a whole sentence for each total so every language can order it.
    private static func accountDifference(_ account: String, field: String, amounts: String) -> String {
        switch field {
        case "", "balance": return String(localized: "\(account) balance: \(amounts)")
        case "cleared_balance": return String(localized: "\(account) cleared balance: \(amounts)")
        case "uncleared_balance": return String(localized: "\(account) uncleared balance: \(amounts)")
        case "budgeted": return String(localized: "\(account) assigned: \(amounts)")
        case "activity": return String(localized: "\(account) activity: \(amounts)")
        default: return "\(account) \(field.replacingOccurrences(of: "_", with: " ")): \(amounts)"
        }
    }

    /// "Groceries, October 2026, available: …"; `about` is the category and the month, either of which may be missing.
    private static func categoryDifference(_ about: [String], field: String, amounts: String) -> String {
        switch (field, about.count) {
        case ("balance", 2): return String(localized: "\(about[0]), \(about[1]), available: \(amounts)")
        case ("balance", 1): return String(localized: "\(about[0]), available: \(amounts)")
        case ("budgeted", 2): return String(localized: "\(about[0]), \(about[1]), assigned: \(amounts)")
        case ("budgeted", 1): return String(localized: "\(about[0]), assigned: \(amounts)")
        case ("activity", 2): return String(localized: "\(about[0]), \(about[1]), activity: \(amounts)")
        case ("activity", 1): return String(localized: "\(about[0]), activity: \(amounts)")
        default:
            let name = field == "balance" ? String(localized: "available") : fields[field] ?? field
            return (about + [name]).filter { !$0.isEmpty }.joined(separator: ", ") + ": \(amounts)"
        }
    }

    /// Every difference in an imported budget's audit, each tagged with what it's about.
    static func differences(_ budget: JSON) -> [JSON] {
        let audit = budget["importAudit"]
        return [("accountDifferences", "account"), ("categoryDifferences", "category"), ("readyToAssignDifferences", "readyToAssign")]
            .flatMap { key, kind in
                audit[key].array.filter(\.isObject).map { row -> JSON in
                    var row = row
                    if row.text("kind").isEmpty { row["kind"] = .string(kind) }
                    return row
                }
            }
    }

    static func conflictField(_ field: String) -> String { conflictFields[field] ?? field }

    /// A value in the conflict review: the side in words, plain text, an amount as money, or the record.
    static func conflictValue(_ value: JSON, field: String, text: String, currency: String) -> String {
        if value.isNull { return String(localized: "Deleted") }
        if !text.isEmpty { return text }
        switch value {
        case .string(let string): return string.isEmpty ? String(localized: "(empty)") : string
        case .int(let number): return field == "amount" ? money(number, currency) : String(number)
        case .double(let number): return String(number)
        case .bool(let flag): return flag ? String(localized: "Yes") : String(localized: "No")
        default:
            let data = try? JSONSerialization.data(withJSONObject: value.foundation, options: [.prettyPrinted, .sortedKeys])
            return data.map { String(decoding: $0, as: UTF8.self) } ?? value.serialized()
        }
    }

    /// The seconds an import took, as "1.24".
    static func seconds(_ milliseconds: Int64) -> String { String(format: "%.2f", Double(milliseconds) / 1000) }
}

private enum YnabRoute: Hashable { case tokens }

/// Imports from YNAB (YnabImport.tsx and YnabFileImport.tsx, Android's YnabImportDialog): from the
/// YNAB account with a personal access token (saved tokens, plans, changes only after the first
/// import, a review of competing changes and of totals that differ), or from YNAB's export files.
struct YnabImportView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var mode = "api"
    @State private var route: [YnabRoute] = []

    // From the YNAB account
    @State private var tokens: [JSON] = []
    @State private var tokenStore = true
    @State private var savedId: String?
    @State private var choseToken = false
    @State private var token = ""
    @State private var remember = false
    @State private var plans: [JSON]?
    @State private var planId = ""
    @State private var plansError: String?
    @State private var loadingPlans = false
    @State private var replace = false
    @State private var busy = false
    @State private var progress: String?
    @State private var message: String?
    @State private var failure: String?
    /// An import waiting on its differences from YNAB, or on a choice for each competing change.
    @State private var differences: JSON?
    @State private var conflicts: JSON?
    @State private var choices: [String: String] = [:]
    @State private var reviewRevision = ""
    @State private var planName = ""

    // From export files
    @State private var picking = false
    @State private var files: [ImportPickedFile] = []
    @State private var dateOrder = ""
    @State private var preview: JSON?
    @State private var previewRevision = ""
    @State private var fileError: String?
    @State private var reading = false
    @State private var confirmReplace = false
    @State private var generation = 0
    /// Export files opened from another app (Files, Mail, AirDrop), read when the screen appears.
    private let opened: [URL]

    init(files: [URL] = []) { opened = files }

    private var budget: JSON { store.budget ?? [:] }
    /// The budget has nothing in it yet, so there's nothing to merge with or replace.
    private var blank: Bool { budget.rows("accounts").isEmpty && budget.rows("transactions").isEmpty }
    private var saved: JSON? { tokens.first { $0.text("id") == savedId } }
    private var secret: String { token.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var reviewing: Bool { differences != nil || conflicts != nil }
    private var credentialKey: String { "\(savedId ?? "")|\(secret)" }

    var body: some View {
        NavigationStack(path: $route) {
            Form {
                if !reviewing {
                    Section {
                        Picker("Import from", selection: $mode) {
                            Text("YNAB account").tag("api")
                            Text("Export files").tag("files")
                        }
                        .pickerStyle(.segmented)
                        .disabled(busy || reading)
                    }
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
                }
                statusContent
                if mode == "api" { apiContent } else { fileContent }
            }
            .navigationTitle("Import from YNAB")
            .inlineTitle()
            .toolbar { toolbar }
            .navigationDestination(for: YnabRoute.self) { _ in
                YnabTokenList(tokens: tokens, current: savedId, store: String(localized: "Keychain"),
                              use: { id in
                                  choseToken = true
                                  savedId = id
                                  route.removeAll()
                              },
                              paste: {
                                  choseToken = true
                                  savedId = nil
                                  plans = nil
                                  planId = ""
                                  route.removeAll()
                              },
                              forget: forget)
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.zip, .commaSeparatedText, .tabSeparatedText, .json, .text, .data],
                      allowsMultipleSelection: true) { pick($0) }
        .onAppear {
            guard !opened.isEmpty, files.isEmpty, !reading else { return }
            mode = "files"
            pick(.success(opened))
        }
        .interactiveDismissDisabled(busy || reviewing || reading)
        .confirmationDialog("Replace this budget?", isPresented: $confirmReplace, titleVisibility: .visible) {
            Button("Replace with “\(preview?.text("name") ?? "YNAB")”", role: .destructive) { replaceBudget() }
        } message: {
            Text("Everything in this budget is replaced by the YNAB export. You can undo it.")
        }
        .task { await loadTokens(initial: true) }
        .task(id: credentialKey) { await loadPlans() }
        .onReceive(NotificationCenter.default.publisher(for: .urbudgetHostEvent)) { note in
            if let event = note.hostEvent, event.name == "ynab-import-progress", busy { progress = event.payload.string }
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button(message != nil && !reviewing ? "Done" : "Cancel") { dismiss() }.disabled(busy)
        }
        ToolbarItem(placement: .confirmationAction) {
            if busy {
                ProgressView()
            } else if let differences {
                Button("Import anyway") { accept(differences) }.fontWeight(.semibold).disabled(store.revision != reviewRevision)
            } else if let conflicts {
                let all = conflicts.rows("conflicts").allSatisfy { ["local", "remote"].contains(choices[$0.text("path")] ?? "") }
                Button("Apply") { resolve(conflicts) }.fontWeight(.semibold).disabled(!all || store.revision != reviewRevision)
            } else if mode == "api" {
                Button("Import") { runImport() }
                    .fontWeight(.semibold)
                    .disabled(loadingPlans || planId.isEmpty || (savedId == nil && secret.isEmpty))
            } else if let preview {
                Button(blank ? "Import" : "Create") {
                    if blank { replaceBudget() } else { createBudget(preview) }
                }
                    .fontWeight(.semibold).disabled(reading)
            }
        }
    }

    // MARK: From the YNAB account

    /// What the last request did: where an import is up to, or why it stopped.
    @ViewBuilder private var statusContent: some View {
        if let failure {
            Section {
                Notice(text: String(localized: "The import stopped. Nothing was changed. \(failure)"), tone: "negative", icon: "exclamationmark.triangle.fill",
                       action: String(localized: "Dismiss")) { self.failure = nil }
            }
            .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
        if busy {
            Section {
                HStack(spacing: 10) {
                    ProgressView()
                    Text(progress ?? String(localized: "Importing…")).foregroundStyle(Color.secondary)
                }
            }
        }
    }

    @ViewBuilder private var apiContent: some View {
        if let differences {
            differencesContent(differences)
        } else if let conflicts {
            conflictsContent(conflicts)
        } else {
            if let message {
                Section {
                    Notice(text: message, tone: "positive", icon: "checkmark.circle.fill")
                }
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            Section {
                Text("Brings over your accounts, categories, transactions, targets, notes and scheduled transactions, and checks every total against YNAB. This budget is backed up first, and UrBudget never changes anything in YNAB.")
                    .font(.subheadline).foregroundStyle(Color.secondary)
            }
            .listRowBackground(Color.clear)
            tokenSection
            planSection
            if !blank { modeSection }
        }
    }

    private var tokenSection: some View {
        Section {
            if let saved {
                HStack(spacing: 12) {
                    Image(systemName: "key.fill").foregroundStyle(Color.urAccent).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(saved.text("label"))
                        Text("••••\(saved.text("hint"))").font(.footnote).monospaced().foregroundStyle(Color.secondary)
                    }
                }
                NavigationLink(value: YnabRoute.tokens) { Text("Change token") }.disabled(busy)
            } else {
                SecureField("Paste your personal access token", text: Binding(get: { token }, set: { choseToken = true; token = $0 }))
                    .autocorrectionDisabled()
                    .disabled(busy)
                if tokenStore { Toggle("Remember on this device", isOn: $remember).disabled(busy) }
                if !tokens.isEmpty {
                    NavigationLink(value: YnabRoute.tokens) { Text("Use a saved token") }.disabled(busy)
                }
            }
        } header: {
            Text("YNAB token")
        } footer: {
            if saved == nil { tokenHelp }
        }
    }

    /// Where a YNAB token comes from, and whether it's kept.
    private var tokenHelp: Text {
        let kept = tokenStore ? Text("A remembered token is kept in Keychain; otherwise it's used only for this import.")
            : Text("It's used only for this import and never stored.")
        let start = Text("In YNAB on the web, open Account Settings, then Developer Settings, and choose New Token. Copy it right away: YNAB shows it only once.")
        return start + Text(verbatim: " ") + kept
    }

    private var planSection: some View {
        Section {
            if let plans {
                Picker("Plan", selection: $planId) {
                    ForEach(plans, id: \.idText) { plan in Text(shownName(plan, in: plans)).tag(plan.text("id")) }
                }
                .disabled(busy || loadingPlans)
            } else {
                LabeledContent("Plan") {
                    if loadingPlans {
                        ProgressView()
                    } else {
                        Text(plansError != nil ? "No plans to show" : "Paste your token to choose a plan").foregroundStyle(Color.secondary)
                    }
                }
            }
        } footer: {
            if let plansError { Text(plansError) }
        }
    }

    private var modeSection: some View {
        Section {
            Picker("If this budget already has data", selection: $replace) {
                choice("Merge with this budget", "Brings in YNAB's changes and keeps edits made here. Competing changes are shown for review.").tag(false)
                choice("Replace this budget", "Starts over from YNAB's version. The current budget stays in a backup.").tag(true)
            }
            .pickerStyle(.inline)
            .labelsHidden()
            .disabled(busy)
        } header: {
            Text("If this budget already has data")
        }
    }

    private func choice(_ title: LocalizedStringKey, _ detail: LocalizedStringKey) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
            Text(detail).font(.footnote).foregroundStyle(Color.secondary)
        }
    }

    /// A plan's name, with when it was edited when two plans share it.
    private func shownName(_ plan: JSON, in plans: [JSON]) -> String {
        let name = plan.text("name")
        let edited = plan.text("lastModifiedOn")
        guard !edited.isEmpty, plans.count(where: { $0.text("name") == name }) > 1 else { return name }
        let date = Money.date(String(edited.prefix(10)))
        return String(localized: "\(name) · edited \(date)")
    }

    @ViewBuilder private func differencesContent(_ result: JSON) -> some View {
        let rows = result.rows("differences")
        let currency = store.currency
        if store.revision != reviewRevision {
            stalenessNotice
        }
        Section {
            Notice(text: rebuiltNotice, icon: "exclamationmark.triangle.fill")
        }
        .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        Section {
            ForEach(Array(rows.enumerated()), id: \.offset) { _, difference in
                Text(ImportYnabText.difference(difference, currency: currency)).font(.subheadline)
            }
        } header: {
            Text("\(rows.count) totals don't match YNAB")
        } footer: {
            Text("Importing anyway brings everything over as YNAB has it, and keeps these differences with the budget. A later import asks again only if they change.")
        }
        Section {
            Button("Cancel review", role: .cancel) { differences = nil }.disabled(busy)
        }
    }

    private var rebuiltNotice: String {
        if planName.isEmpty {
            return String(localized: "Nothing has been saved yet. UrBudget rebuilt your plan from YNAB's transactions, and these came out different from what YNAB shows.")
        }
        return String(localized: "Nothing has been saved yet. UrBudget rebuilt \(planName) from YNAB's transactions, and these came out different from what YNAB shows.")
    }

    @ViewBuilder private func conflictsContent(_ result: JSON) -> some View {
        let rows = result.rows("conflicts")
        let currency = store.currency
        if store.revision != reviewRevision {
            stalenessNotice
        }
        Section {
            Text("Independent edits are merged automatically. Only these competing changes need a choice. Nothing has been overwritten.")
                .font(.subheadline)
            Menu {
                Button("Keep all of this budget's") { for row in rows { choices[row.text("path")] = "local" } }
                Button("Take all of YNAB's") { for row in rows { choices[row.text("path")] = "remote" } }
            } label: {
                Label("Choose for all", systemImage: "checklist")
            }
            .disabled(busy)
        } header: {
            Text("Choose which changes to keep")
        }
        ForEach(Array(rows.enumerated()), id: \.offset) { _, conflict in
            let path = conflict.text("path")
            let field = conflict.text("field")
            Section {
                ValueBlock(title: "This budget", value: ImportYnabText.conflictValue(conflict["local"], field: field, text: conflict.text("localText"), currency: currency))
                ValueBlock(title: "In YNAB", value: ImportYnabText.conflictValue(conflict["remote"], field: field, text: conflict.text("remoteText"), currency: currency))
                Picker("Keep", selection: Binding(get: { choices[path] ?? "" }, set: { choices[path] = $0 })) {
                    Text("This budget's").tag("local")
                    Text("YNAB's").tag("remote")
                }
                .pickerStyle(.segmented)
                .disabled(busy)
                .accessibilityLabel("Keep for \(conflict.text("about").isEmpty ? path : conflict.text("about"))")
            } header: {
                Text(ImportsText.line([conflict.text("about").isEmpty ? path : conflict.text("about"), field.isEmpty ? nil : ImportYnabText.conflictField(field)]
                    .compactMap { $0 }))
                    .textCase(nil)
            }
        }
        Section {
            Button("Cancel review", role: .cancel) { conflicts = nil; choices = [:] }.disabled(busy)
        }
    }

    private var stalenessNotice: some View {
        Section {
            Notice(text: String(localized: "Your budget changed since this review. Cancel it and import again to review the latest changes."), tone: "negative",
                   icon: "exclamationmark.triangle.fill")
        }
        .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
    }

    // MARK: From export files

    @ViewBuilder private var fileContent: some View {
        let json = files.count == 1 && files[0].name.lowercased().hasSuffix(".json")
        Section {
            exportHelp.font(.subheadline).foregroundStyle(Color.secondary)
        }
        .listRowBackground(Color.clear)
        Section {
            Button { picking = true } label: {
                Label(files.isEmpty ? "Choose export file…" : "Choose other files…", systemImage: "doc.zipper")
            }
            .disabled(reading || busy)
            if !files.isEmpty {
                Text(files.map(\.name).joined(separator: ", ")).font(.footnote).foregroundStyle(Color.secondary)
            }
            if !json {
                Picker("Date order in the file", selection: $dateOrder) {
                    Text("Detect automatically").tag("")
                    Text("Day first (31/12/2026)").tag("dmy")
                    Text("Month first (12/31/2026)").tag("mdy")
                    Text("Year first (2026-12-31)").tag("ymd")
                }
                .disabled(reading || busy)
                .onChange(of: dateOrder) { readFiles() }
            }
        } footer: {
            Text("A full YNAB API export (.json) can be imported too.")
        }
        if reading {
            HStack(spacing: 10) { ProgressView(); Text("Reading the export…").foregroundStyle(Color.secondary) }
        }
        if let fileError { FormErrorView(message: fileError) }
        if let preview { previewContent(preview, fromFiles: !json) }
    }

    /// How to get YNAB's export, and what importing it does to this budget.
    private var exportHelp: Text {
        let effect = blank ? Text("It fills this empty budget.") : Text("This makes a new budget; the open one isn't changed.")
        let start = Text("In YNAB on the web, open your plan's menu and export it. Choose the .zip it downloads, or the Register and Plan files inside.")
        return start + Text(verbatim: " ") + effect + Text(verbatim: " ") + Text("No token or connection is needed.")
    }

    /// "4 budget, 1 credit, 2 tracking".
    private func accountKinds(cash: Int, credit: Int, tracking: Int) -> String {
        String(localized: "\(cash) budget, \(credit) credit, \(tracking) tracking", locale: .app)
    }

    private func matchesText(checks: Int64) -> String {
        if checks > 0 {
            return String(localized: "Checked \(checks) category months against YNAB's plan: every amount matches.", locale: .app)
        }
        return String(localized: "Every total matches YNAB.")
    }

    /// "Checked 120 category months against YNAB's plan: 2 amounts differ.", or "3 totals differ from YNAB."
    private func differsText(checks: Int64, found: Int) -> String {
        if checks > 0 {
            return String(localized: "Checked \(checks) category months against YNAB's plan: \(found) amounts differ.", locale: .app)
        }
        return String(localized: "\(found) totals differ from YNAB.", locale: .app)
    }

    @ViewBuilder private func previewContent(_ preview: JSON, fromFiles: Bool) -> some View {
        let accounts = preview.rows("accounts")
        let kinds = { (kind: String) in accounts.filter { $0.text("kind") == kind } }
        let tracking = kinds("tracking")
        let months = Set(preview.rows("assignments").map { $0.text("month") }).count
        let currency = preview.text("currency").isEmpty ? store.currency : preview.text("currency")
        let found = ImportYnabText.differences(preview)
        let checks = preview["importAudit"].long("categoryMonthChecks")
        Section {
            ValueRow(title: String(localized: "Accounts"), value: "\(accounts.count)",
                     subtitle: accountKinds(cash: kinds("cash").count, credit: kinds("credit").count, tracking: tracking.count))
            ValueRow(title: String(localized: "Categories"), value: "\(preview.rows("categories").count)")
            ValueRow(title: String(localized: "Transactions"), value: "\(preview.rows("transactions").count)")
            ValueRow(title: String(localized: "Months of assignments"), value: "\(months)")
            ValueRow(title: String(localized: "Currency"), value: currency)
        } header: {
            Text(preview.text("name")).textCase(nil)
        } footer: {
            if !tracking.isEmpty {
                Text("Tracking accounts: \(tracking.map { $0.text("name") }.joined(separator: ", ")). An account is treated as tracking when none of its rows has a category; change it afterwards if that's wrong.")
            }
        }
        Section {
            if found.isEmpty {
                Label(matchesText(checks: checks), systemImage: "checkmark.circle.fill")
                    .foregroundStyle(Color.urPositive)
            } else {
                Label(differsText(checks: checks, found: found.count), systemImage: "exclamationmark.triangle.fill")
                    .foregroundStyle(Color.urWarning)
                ForEach(Array(found.enumerated()), id: \.offset) { _, difference in
                    Text(ImportYnabText.difference(difference, currency: currency)).font(.subheadline)
                }
            }
        } footer: {
            if fromFiles {
                Text("YNAB's export leaves out targets, notes, split memos and which accounts are closed. It has only the next date of each scheduled transaction, so those aren't brought in.")
            }
        }
        Section {
            if blank {
                Button("Import “\(preview.text("name"))”") { replaceBudget() }.fontWeight(.semibold)
            } else {
                Button("Create “\(preview.text("name"))” and open it") { createBudget(preview) }.fontWeight(.semibold)
                Button("Replace this budget…", role: .destructive) { confirmReplace = true }
            }
        }
        .disabled(reading || busy)
    }

    // MARK: Actions

    /// Runs one YNAB request at a time; a failure is shown as the import having stopped.
    private func run(_ work: @escaping @MainActor () async throws -> Void) {
        guard !busy else { return }
        busy = true
        failure = nil
        message = nil
        progress = nil
        Task {
            do { try await work() } catch { failure = BudgetStore.readable(error) }
            busy = false
            progress = nil
        }
    }

    private func loadTokens(pick: String? = nil, initial: Bool = false) async {
        do {
            let found = try await store.service("ynab_tokens").array
            tokens = found
            tokenStore = true
            if let pick {
                savedId = pick
            } else if let id = savedId, !found.contains(where: { $0.text("id") == id }) {
                savedId = nil
            }
            // The most recently used saved token, unless a token's already being pasted.
            if initial && !choseToken && savedId == nil, let latest = found.first { savedId = latest.text("id") }
        } catch {
            tokens = []
            tokenStore = false
        }
    }

    /// Lists the token's plans shortly after it's pasted (at once for a saved one), so the plan is
    /// picked by name, keeping the one already picked when the token reads it.
    private func loadPlans() async {
        let pasted = secret
        let saved = savedId
        guard saved != nil || !pasted.isEmpty else {
            loadingPlans = false
            return
        }
        loadingPlans = true
        if saved == nil {
            try? await Task.sleep(for: .milliseconds(400))
            guard !Task.isCancelled else { return }
        }
        var params: JSON = ["token": .string(saved == nil ? pasted : "")]
        if let saved { params["savedToken"] = .string(saved) }
        do {
            let found = try await store.service("ynab_plans", params).array
            guard !Task.isCancelled else { return }
            plans = found
            plansError = nil
            if !found.contains(where: { $0.text("id") == planId }) { planId = found.first?.text("id") ?? "" }
        } catch {
            guard !Task.isCancelled else { return }
            plans = nil
            planId = ""
            plansError = BudgetStore.readable(error)
        }
        loadingPlans = false
    }

    private func forget(_ id: String) async throws {
        let wasInUse = id == savedId
        _ = try await store.service("forget_ynab_token", ["id": .string(id)])
        await loadTokens()
        if wasInUse {
            savedId = tokens.first?.text("id")
            if tokens.isEmpty { plans = nil; planId = "" }
        }
    }

    /// A saved token is named for the plans it reads.
    private var tokenLabel: String {
        let names = (plans ?? []).map { $0.text("name") }
        return names.count > 3 ? "\(names.prefix(3).joined(separator: ", ")) +\(names.count - 3) more" : names.joined(separator: ", ")
    }

    private func runImport() {
        let label = tokenLabel
        var params: JSON = ["planId": .string(planId), "replace": .bool(replace && !blank)]
        if let savedId {
            params["token"] = ""
            params["savedToken"] = .string(savedId)
            if !label.isEmpty { params["tokenLabel"] = .string(label) }
        } else {
            params["token"] = .string(secret)
            if remember && tokenStore {
                params["rememberToken"] = true
                params["tokenLabel"] = .string(label)
            }
        }
        planName = plans?.first { $0.text("id") == planId }?.text("name") ?? ""
        // The pasted token isn't kept on screen once it's been used.
        token = ""
        run {
            let result = try await store.service("sync_ynab", params, changesBudget: true)
            if !result.text("savedToken").isEmpty {
                remember = false
                await loadTokens(pick: result.text("savedToken"))
            } else if savedId != nil {
                await loadTokens()
            }
            settle(result)
        }
    }

    /// An import waits on its differences from YNAB, or on a choice for each competing change.
    private func settle(_ result: JSON) {
        differences = nil
        conflicts = nil
        if !result.text("reviewId").isEmpty {
            // The review is checked against the budget as it is now; a later change refuses it.
            reviewRevision = store.revision
            if !result.rows("differences").isEmpty {
                differences = result
            } else {
                conflicts = result
                choices = [:]
            }
            return
        }
        let snapshot = result["snapshot"]["budget"]
        let time = ImportYnabText.seconds(result.long("elapsedMs"))
        if result.flag("unchanged") {
            message = String(localized: "Already up to date · \(time) seconds.")
            return
        }
        let name = snapshot.text("name")
        let accounts = snapshot.rows("accounts").count
        let transactions = snapshot.rows("transactions").count
        let done = String(localized: "Imported and saved \(name) · \(accounts) accounts · \(transactions) transactions · \(time) seconds.", locale: .app)
        let backup = String(localized: "A backup was saved automatically.")
        let changesOnly: [String] = result.flag("incremental") ? [String(localized: "Downloaded changes only.")] : []
        message = ([done] + changesOnly + [backup]).joined(separator: " ")
    }

    private func accept(_ pending: JSON) {
        let params: JSON = ["reviewId": .string(pending.text("reviewId")), "expectedToken": .string(reviewRevision)]
        run { settle(try await store.service("accept_ynab_import", params, changesBudget: true)) }
    }

    private func resolve(_ pending: JSON) {
        let params: JSON = ["reviewId": .string(pending.text("reviewId")), "choices": .object(choices.mapValues(JSON.string)),
                            "expectedToken": .string(reviewRevision)]
        run {
            _ = try await store.service("resolve_ynab", params, changesBudget: true)
            conflicts = nil
            choices = [:]
            message = String(localized: "Reviewed changes imported and saved. A backup was created.")
        }
    }

    private func pick(_ result: Result<[URL], Error>) {
        let urls: [URL]
        switch result {
        case .success(let picked): urls = picked
        case .failure(let failure): fileError = BudgetStore.readable(failure); return
        }
        guard !urls.isEmpty else { return }
        reading = true
        fileError = nil
        preview = nil
        Task {
            do {
                files = try await Task.detached(priority: .userInitiated) { try ImportFiles.readYnab(urls) }.value
                readFiles()
            } catch {
                fileError = BudgetStore.readable(error)
                reading = false
            }
        }
    }

    /// Builds the budget the export makes, through the engine's `importYnabFiles` (or `importYnab`
    /// for a full API export), without changing anything yet.
    private func readFiles() {
        guard !files.isEmpty else { return }
        generation += 1
        let current = generation
        let chosen = files
        let order = dateOrder
        let currency = store.currency
        reading = true
        fileError = nil
        preview = nil
        Task {
            do {
                let value: JSON
                if chosen.count == 1 && chosen[0].name.lowercased().hasSuffix(".json") {
                    let export = try await Task.detached(priority: .userInitiated) { try JSON.parse(chosen[0].data) }.value
                    value = try await store.query("importYnab", ["export": export])
                } else {
                    value = try await Task.detached(priority: .userInitiated) {
                        try ImportsEngine.importYnabFiles(chosen, dateOrder: order, currency: currency)
                    }.value
                }
                guard current == generation else { return }
                guard value.has("schemaVersion") else { throw AppError(String(localized: "These files didn't make a budget UrBudget can open.")) }
                preview = value
                previewRevision = store.revision
            } catch {
                guard current == generation else { return }
                fileError = BudgetStore.readable(error)
            }
            reading = false
        }
    }

    /// The open budget's date and number formats carry over, as on the desktop.
    private func styled(_ preview: JSON) -> JSON {
        var imported = preview
        if budget.has("dateFormat") { imported["dateFormat"] = budget["dateFormat"] }
        if imported.text("currency") == budget.text("currency") && budget["numberFormat"].isObject { imported["numberFormat"] = budget["numberFormat"] }
        return imported
    }

    private func createBudget(_ preview: JSON) {
        let imported = styled(preview)
        let name = preview.text("name")
        run {
            let id = try await store.service("create_budget", ["budget": imported]).string
            store.showToast(String(localized: "Created “\(name)” from YNAB's export and opened it."))
            dismiss()
            store.openBudget(id: id)
        }
    }

    private func replaceBudget() {
        guard let preview else { return }
        let imported = styled(preview)
        let name = preview.text("name")
        let revision = previewRevision
        let empty = blank
        run {
            let data = await Task.detached(priority: .userInitiated) { imported.data() }.value
            do {
                try await store.importsReplace(with: data, since: revision,
                                               stale: String(localized: "Your budget changed since this preview. Choose Replace again to replace it as it is now."))
            } catch {
                // Once told, a second Replace replaces the budget as it is now.
                if store.revision != revision { previewRevision = store.revision }
                throw error
            }
            store.showToast(empty ? String(localized: "Imported “\(name)” from YNAB's export.")
                                  : String(localized: "Replaced this budget with “\(name)” from YNAB's export."), undo: true)
            dismiss()
        }
    }
}

/// A labelled value that can run to several lines, for the conflict review.
private struct ValueBlock: View {
    var title: LocalizedStringKey
    var value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.footnote).foregroundStyle(Color.secondary)
            Text(value).font(.callout).lineLimit(12).textSelection(.enabled)
        }
        .accessibilityElement(children: .combine)
    }
}

/// The YNAB tokens kept on this device: use one, or delete it.
private struct YnabTokenList: View {
    var tokens: [JSON]
    var current: String?
    var store: String
    var use: (String) -> Void
    var paste: () -> Void
    var forget: (String) async throws -> Void
    @State private var confirming: JSON?
    @State private var error: String?

    var body: some View {
        List {
            if tokens.isEmpty {
                EmptyState(title: String(localized: "No saved tokens"), detail: String(localized: "Paste one and choose Remember on this device."), symbol: "key")
            } else {
                Section {
                    ForEach(tokens, id: \.idText) { token in row(token) }
                } footer: {
                    Text("Kept in \(store) on this device. Deleting one here doesn't switch it off in YNAB; to do that, delete it under Developer Settings in YNAB.")
                }
            }
            Section {
                Button("Paste a different token", action: paste)
            }
            if let error { FormErrorView(message: error) }
        }
        .groupedList()
        .navigationTitle("Saved YNAB tokens")
        .inlineTitle()
        .confirmationDialog(deleteTitle, isPresented: Binding(get: { confirming != nil }, set: { if !$0 { confirming = nil } }), titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                guard let id = confirming?.text("id") else { return }
                Task {
                    do { try await forget(id); error = nil } catch { self.error = BudgetStore.readable(error) }
                }
            }
        } message: {
            Text("UrBudget forgets this token on this device. It keeps working in YNAB until you delete it there.")
        }
    }

    private var deleteTitle: Text {
        guard let confirming else { return Text("Delete this token?") }
        return Text("Delete \(confirming.text("label")) (••••\(confirming.text("hint")))?")
    }

    /// "••••abcd · saved 2026-10-01 · last used 2026-10-20".
    private func tokenDetail(_ token: JSON) -> String {
        let hint = token.text("hint")
        let saved = Money.date(String(token.text("savedAt").prefix(10)))
        let lastUsed = token.text("lastUsed")
        guard !lastUsed.isEmpty else { return String(localized: "••••\(hint) · saved \(saved)") }
        let used = Money.date(String(lastUsed.prefix(10)))
        return String(localized: "••••\(hint) · saved \(saved) · last used \(used)")
    }

    private func row(_ token: JSON) -> some View {
        let id = token.text("id")
        let detail = tokenDetail(token)
        return Button { use(id) } label: {
            HStack(spacing: 12) {
                Image(systemName: "key.fill").foregroundStyle(Color.urAccent).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(token.text("label")).foregroundStyle(Color.primary)
                    Text(detail).font(.footnote).foregroundStyle(Color.secondary)
                }
                Spacer()
                if id == current {
                    Text("In use").font(.footnote.weight(.semibold)).foregroundStyle(Color.urAccent)
                }
            }
        }
        .swipeActions(edge: .trailing) {
            Button(role: .destructive) { confirming = token } label: { Label("Delete", systemImage: "trash") }
        }
        .contextMenu {
            Button { use(id) } label: { Label("Use", systemImage: "checkmark") }
            Button(role: .destructive) { confirming = token } label: { Label("Delete", systemImage: "trash") }
        }
    }
}
