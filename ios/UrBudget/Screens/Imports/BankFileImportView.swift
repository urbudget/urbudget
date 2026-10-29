import SwiftUI
import UniformTypeIdentifiers

private let changedAgain = String(localized: "Your budget changed, so these rows were checked again. Look them over, then import.")

private enum BankFileRoute: Hashable { case account }

/// Where an import is: the file's lines to fix, or the review of those that read.
private enum BankFileStage { case lines, review }

/// The review of the lines that read: its rows, the budget revision they were checked against,
/// the statement as it read then (what the account keeps of it), and the import it was made of.
private struct BankFilePreview {
    var rows: [BankReviewRow]
    var summary: BankReviewSummary
    var revision: String
    var statement: BankStatement
    var request: BankReviewRequest
}

/// Imports a file a bank exported (the desktop's Import page, Android's BankImportDialog): choose
/// the file, fix or include the lines that need it (this opens by itself when something does),
/// then decide what each line that reads does (add it, match an existing transaction, or skip it),
/// then one budget change that Undo takes back.
struct BankFileImportView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var account = ""
    @State private var fileName = ""
    /// The file chosen, to read another of its sheets.
    @State private var file: URL?
    /// The file's text: a workbook's sheet, as CSV.
    @State private var content: String?
    @State private var sheets: [String] = []
    @State private var sheet: String?
    /// The import (bank_statement.rs `Session`), passed back as it came.
    @State private var session: JSON?
    @State private var statement: BankStatement?
    @State private var stage: BankFileStage?
    @State private var preview: BankFilePreview?
    @State private var decisions: [Int: String] = [:]
    /// Lines the assistant changed.
    @State private var assisted: Set<String> = []
    @State private var fixing: StatementLine?
    @State private var error: String?
    @State private var notice: String?
    @State private var loading = false
    @State private var saving = false
    @State private var picking = false
    @State private var confirmDiscard = false
    /// A new file (or none) since: replies for the one before are dropped.
    @State private var generation = 0
    /// Reviews asked for: only the latest one shows.
    @State private var reviews = 0
    @State private var route: [BankFileRoute] = []
    @State private var started = false
    /// A PDF chosen, which the assistant reads.
    @State private var pdf: URL?
    @State private var asking: AssistantRequest?
    /// A statement opened from another app, and the account it goes into (from an account's register).
    private let opened: URL?
    private let presetAccount: String

    init(file: URL? = nil, account: String = "") {
        opened = file
        presetAccount = account
    }

    private static let types: [UTType] = [.commaSeparatedText, .text, .xml, .spreadsheet, .zip, .pdf]
        + ["xlsx", "xls", "ods", "ofx", "qfx", "qif", "sta", "940", "mt940"].compactMap { UTType(filenameExtension: $0) } + [.data]

    private var budget: JSON { store.budget ?? [:] }
    private var accounts: [JSON] { budget.rows("accounts").filter { !$0.flag("archived") } }
    private var accountName: String { budget.row("accounts", id: account)?.text("name") ?? "" }
    private var sessionId: String { session?["scope"].text("session") ?? "" }
    /// The statement read for the session as it is now: until it is, a line's version is the one before.
    private var current: BankStatement? {
        guard let statement, let session, statement.session == sessionId, statement.revision == session.long("revision") else { return nil }
        return statement
    }
    /// The import as it is now, as a review of it would be asked for.
    private var request: BankReviewRequest {
        BankReviewRequest(number: reviews, generation: generation, session: sessionId, revision: session?.long("revision") ?? -1, account: account)
    }
    /// The review of the import as it is now: one of another file, an earlier reading of this one
    /// or another account is never shown or imported.
    private var shownPreview: BankFilePreview? {
        guard let preview, session != nil, preview.request.sameImport(as: request) else { return nil }
        return preview
    }
    private var busy: Bool { loading || saving || (session != nil && current == nil) }
    /// Most changes take a moment: the screen only looks busy (greyed, a spinner) for one that
    /// lasts, so a quick one doesn't flash it.
    @State private var slow = false

    var body: some View {
        NavigationStack(path: $route) {
            Group {
                if session == nil || statement == nil {
                    setup
                } else if stage == .review, let preview = shownPreview {
                    review(preview)
                } else if stage == .review {
                    List {
                        HStack(spacing: 10) { ProgressView(); Text("Checking the lines against your budget…").foregroundStyle(Color.secondary) }
                        if let error { FormErrorView(message: error) }
                    }
                    .groupedList()
                } else {
                    lines
                }
            }
            .navigationTitle(stage == .review ? "Review import" : "Import bank file")
            .inlineTitle()
            .toolbar { toolbar }
            .navigationDestination(for: BankFileRoute.self) { _ in
                AccountPicker(selected: account) { id in
                    account = id
                    route.removeAll()
                }
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: Self.types) { read($0) }
        .sheet(item: $asking) { AssistantView(request: $0) }
        .sheet(item: $fixing) { line in
            LineFixSheet(line: line) { values in
                fixing = nil
                if let session, let fixes = BankFixes.fields(session, line, values) { fix(fixes) }
            }
        }
        .interactiveDismissDisabled(session != nil || saving)
        .confirmationDialog("Discard this import?", isPresented: $confirmDiscard, titleVisibility: .visible) {
            Button("Discard", role: .destructive) { dismiss() }
        } message: {
            Text("Nothing has been imported yet. The changes and choices you made are lost.")
        }
        .retainingEditorDraft("bankImport", values: ["account": .string(account), "fileName": .string(fileName),
            "sheet": sheet.map(JSON.string) ?? .null, "session": session ?? .null,
            "decisions": .object(Dictionary(uniqueKeysWithValues: decisions.map { (String($0.key), JSON.string($0.value)) }))])
        .onAppear(perform: begin)
        .task(id: busy) {
            guard busy else { slow = false; return }
            try? await Task.sleep(for: .milliseconds(300))
            if !Task.isCancelled { slow = busy }
        }
        .onChange(of: store.revision) { _, now in
            // Sync, Undo or another screen changed the budget: read the file and check the rows again against it.
            guard session != nil, !saving else { return }
            refresh(notice: shownPreview != nil && shownPreview?.revision != now ? changedAgain : nil)
        }
        .onChange(of: account) {
            // An import is into one account: the file is read again into another.
            if session != nil, let file { open(file, sheet: sheet) }
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button("Cancel") {
                if (session?.long("revision") ?? 0) > 0 || !decisions.isEmpty { confirmDiscard = true } else { dismiss() }
            }
            .disabled(saving)
        }
        ToolbarItem(placement: .confirmationAction) {
            if saving || (slow && session != nil) {
                ProgressView()
            } else if stage == .review, let preview = shownPreview {
                let counts = BankReview.counts(preview.rows, decisions)
                Button(counts.importing == 1 ? "Import 1" : "Import \(counts.importing)") { importRows() }
                    .fontWeight(.semibold)
                    // Checked against the budget as it was: checked again before anything imports.
                    .disabled(counts.undecided > 0 || counts.importing == 0 || preview.revision != store.revision)
            } else if let current {
                // Nothing goes on to the review on a guess the file leaves open.
                Button("Continue") { stage = .review; runPreview() }
                    .fontWeight(.semibold)
                    .disabled(!current.canContinue)
            }
        }
    }

    // MARK: Choosing the file

    private var setup: some View {
        Form {
            Section {
                Text("Bring in the transactions your bank exported. You see the lines that don't read and fix them, then check every transaction before anything is saved.")
                    .font(.subheadline).foregroundStyle(Color.secondary)
                    .listRowBackground(Color.clear)
            }
            if accounts.isEmpty {
                EmptyState(title: String(localized: "Add an account first"),
                           detail: String(localized: "Imported transactions go into one of your accounts. Add it in Accounts, then import its statement."),
                           symbol: "building.columns")
            } else {
                fileSection
                if let pdf {
                    Section {
                        Notice(text: String(localized: "PDF statements need the assistant. It reads the PDF, finds the transactions, checks them against the statement's totals and brings them to a review like this one, where you decide on each row."),
                               tone: "info", icon: "sparkles", action: String(localized: "Read it with the assistant")) {
                            readWithAssistant(pdf)
                        }
                    }
                    .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                }
            }
            if loading {
                HStack(spacing: 10) {
                    ProgressView()
                    Text("Reading the file…").foregroundStyle(Color.secondary)
                }
            }
            if let error { FormErrorView(message: error) }
        }
    }

    private var fileSection: some View {
        Section {
            NavigationLink(value: BankFileRoute.account) {
                LabeledContent("Into account", value: accountName.isEmpty ? String(localized: "Choose") : accountName)
            }
            .disabled(loading)
            Button { picking = true } label: {
                HStack(spacing: 12) {
                    Image(systemName: fileName.isEmpty ? "doc.badge.plus" : "doc.text")
                        .font(.title3).foregroundStyle(Color.urAccent).frame(width: 30)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(fileName.isEmpty ? String(localized: "Choose a file") : fileName).foregroundStyle(Color.primary).lineLimit(1)
                        Text(pdf != nil ? "PDF · tap to choose another" : "CSV, Excel, OFX, QIF, CAMT.053 or MT940; PDF through the assistant")
                            .font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
            }
            .disabled(loading || account.isEmpty)
            .accessibilityHint("Opens your files")
        } footer: {
            Text("Lines that don't read, or that the import leaves out (pending payments, another account's, balances), are shown for you to fix or include. The account remembers how its file read; on the desktop you can also choose which column holds what.")
        }
    }

    // MARK: The file's lines

    private var lines: some View {
        let shown = statement!
        let structure = session?["structure"] ?? [:]
        let attention = shown.attentionLines
        let out = shown.leftOutLines
        let changed = shown.changedLines
        return List {
            Section { heading(BankStatementText.counts(shown)) }
            if sheets.count > 1 || shown.sources.count > 1 || shown.format == "csv" || answeredDates(shown) != nil || answeredSigns(shown) != nil { Section {
                if sheets.count > 1 {
                    Picker("Sheet", selection: Binding(get: { sheet ?? sheets[0] }, set: { chosen in if chosen != sheet, let file { open(file, sheet: chosen) } })) {
                        ForEach(sheets, id: \.self) { Text($0).tag($0) }
                    }
                }
                if shown.sources.count > 1 {
                    Picker("Account in the file", selection: Binding(get: { shown.sources[min(shown.source ?? 0, shown.sources.count - 1)].label },
                                                                    set: { label in restructure { $0["source"] = .string(label) } })) {
                        ForEach(shown.sources, id: \.label) { source in
                            Text(BankStatementText.source(source)).tag(source.label)
                        }
                    }
                }
                // Answered once, the date order and the signs stay one choice away.
                if let example = answeredDates(shown) {
                    let named = BankStatementText.readings(example)
                    Picker("Dates", selection: Binding(get: { shown.dateOrder ?? "dmy" },
                                                       set: { order in restructure { $0["dateOrder"] = ["order": .string(order), "confirmed": true] } })) {
                        Text(BankStatementText.order("dmy", example: example, reading: named?.dmy)).tag("dmy")
                        Text(BankStatementText.order("mdy", example: example, reading: named?.mdy)).tag("mdy")
                    }
                }
                if let byType = answeredSigns(shown) {
                    Picker("Money out", selection: Binding(get: { byType }, set: { on in restructure { $0["signsByType"] = .bool(on) } })) {
                        Text("Told by the transaction types").tag(true)
                        Text("As the amounts are written").tag(false)
                    }
                }
                if shown.format == "csv" {
                    // Some card issuers write a purchase as a positive number and a payment as a negative one.
                    Picker("Purchases in the file", selection: Binding(get: { structure.flag("flipSigns") },
                                                                       set: { on in restructure { $0["flipSigns"] = .bool(on) } })) {
                        Text("Negative, as bank accounts write them").tag(false)
                        Text("Positive, as many card statements write them").tag(true)
                    }
                }
            }
            .disabled(slow) }
            notices(shown)
            if !attention.isEmpty {
                Section {
                    Notice(text: String(localized: "The assistant can fix these lines from what the file says, and asks you about anything unclear. You see each fix before it's applied."),
                           tone: "info", icon: "sparkles", action: String(localized: "Fix with the assistant")) { fixWithAssistant() }
                        .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                        .disabled(slow)
                    ForEach(attention) { line in
                        lineRow(line, state: line.problem, tone: .urWarning, actions: [batchAction(line, shown), undo(line)].compactMap { $0 }) { fixing = line }
                    }
                } header: {
                    Text("Needs attention")
                } footer: {
                    Text("Not imported until fixed: tap a line to give what doesn't read.")
                }
            }
            if !out.isEmpty {
                Section {
                    ForEach(out) { line in
                        let include: (String, () -> Void)? = line.includable && !line.included ? (String(localized: "Include"), { fix(BankFixes.include(session ?? [:], line, true)) }) : nil
                        lineRow(line, state: BankStatementText.leftOut(line), tone: .secondary, actions: [include, batchAction(line, shown), undo(line)].compactMap { $0 })
                    }
                } header: {
                    Text("Left out")
                } footer: {
                    Text("Not imported. Include a line if it belongs in the account.")
                }
            }
            if !changed.isEmpty {
                Section("Changed") {
                    ForEach(changed) { line in
                        lineRow(line, state: changedState(line), tone: .urAccent, actions: [undo(line)].compactMap { $0 }) { fixing = line }
                    }
                }
            }
        }
        .groupedList()
    }

    /// The ambiguous date the date order was answered for, once it has been.
    private func answeredDates(_ shown: BankStatement) -> String? { shown.dateUnconfirmed ? nil : shown.dateExample }
    /// How money out was said to be told, once it has been.
    private func answeredSigns(_ shown: BankStatement) -> Bool? { shown.signsUnconfirmed ? nil : shown.signsByType }

    /// What became of a line that reads now: the assistant fixed it, it was included, or it was changed here.
    private func changedState(_ line: StatementLine) -> String {
        if assisted.contains(line.id) { return String(localized: "Fixed by the assistant") }
        return line.included ? String(localized: "Included") : String(localized: "Changed from the file")
    }

    private func heading(_ detail: String) -> some View {
        let source = fileName.isEmpty ? String(localized: "Statement") : fileName
        return HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text("\(source) into \(accountName)").font(.headline)
                Text(detail).font(.footnote).foregroundStyle(Color.secondary)
            }
            Spacer(minLength: 8)
            Button("Change file") { startOver() }.buttonStyle(.borderless).disabled(saving)
        }
    }

    /// What the file leaves open, and what became of the last change.
    @ViewBuilder private func notices(_ shown: BankStatement) -> some View {
        if let notice {
            Notice(text: notice, tone: "info", icon: "sparkles").listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
        if let error { FormErrorView(message: error) }
        if let other = shown.elsewhere {
            Notice(text: String(localized: "The file's amounts are in \(other). Include a line to enter its amount in your budget's currency, or import the file into a budget in \(other)."),
                   icon: "exclamationmark.triangle.fill")
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
        if let other = shown.currency {
            Notice(text: String(localized: "This account's amounts are in \(other). Choose another account in the file, or include its lines with their amounts in your budget's currency."),
                   icon: "exclamationmark.triangle.fill")
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
        }
        if shown.dateUnconfirmed, let example = shown.dateExample {
            let named = BankStatementText.readings(example)
            let order = { (which: String) in restructure { $0["dateOrder"] = ["order": .string(which), "confirmed": true] } }
            let dayFirst: String = named?.dmy ?? String(localized: "Day first")
            let monthFirst: String = named?.mdy ?? String(localized: "Month first")
            // The order the file reads in for now is the main answer.
            let first = shown.dateOrder == "mdy" ? ("mdy", monthFirst) : ("dmy", dayFirst)
            let second = shown.dateOrder == "mdy" ? ("dmy", dayFirst) : ("mdy", monthFirst)
            Notice(text: String(localized: "The dates don't say whether the day or the month comes first: is \(example)"), icon: "exclamationmark.triangle.fill",
                   action: first.1, secondary: .init(label: second.1) { order(second.0) }) { order(first.0) }
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                .disabled(slow)
        }
        if shown.signsUnconfirmed {
            Notice(text: String(localized: "Every amount that reads is positive. Is money out told by the transaction types (DEBIT, POS…)?"), icon: "exclamationmark.triangle.fill",
                   action: String(localized: "Yes, use the types"),
                   secondary: .init(label: String(localized: "No, as written")) { restructure { $0["signsByType"] = false } }) {
                restructure { $0["signsByType"] = true }
            }
            .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            .disabled(slow)
        }
    }

    /// A line of the file: what it says, what became of it, and what can be done.
    private func lineRow(_ line: StatementLine, state: String, tone: Color, actions: [(String, () -> Void)], edit: (() -> Void)? = nil) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Button { edit?() } label: {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(line.title).font(.body.weight(.semibold)).foregroundStyle(Color.primary).lineLimit(2)
                        let detail = ImportsText.line([line.fields["date"] ?? "", line.fields["memo"] ?? ""].filter { !$0.isEmpty })
                        if !detail.isEmpty { Text(detail).font(.footnote).foregroundStyle(Color.secondary).lineLimit(1) }
                    }
                    Spacer(minLength: 8)
                    // The amount as the file writes it, kept left to right ("-12.00", never "12.00-").
                    Text(AppLanguage.leftToRight(line.fields["amount"] ?? "")).font(.body.weight(.semibold)).monospacedDigit().foregroundStyle(Color.primary).lineLimit(1)
                    if edit != nil { Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary) }
                }
            }
            .buttonStyle(.borderless)
            .disabled(edit == nil || slow)
            Text(state).font(.footnote.weight(.semibold)).foregroundStyle(tone).lineLimit(3)
            if !actions.isEmpty {
                HStack(spacing: 18) {
                    ForEach(actions.indices, id: \.self) { index in
                        Button(actions[index].0, action: actions[index].1).font(.subheadline.weight(.semibold)).buttonStyle(.borderless).disabled(slow)
                    }
                }
            }
        }
        .padding(.vertical, 2)
    }

    /// A batch's payments that don't add up to its entry: imported as the entry instead, or each again.
    private func batchAction(_ line: StatementLine, _ shown: BankStatement) -> (String, () -> Void)? {
        guard let index = line.batch, shown.batches.indices.contains(index), let session else { return nil }
        let batch = shown.batches[index]
        let place = shown.places[line.id]
        if batch.addsUp == false && !batch.asOne && batch.payments.first == place {
            return (String(localized: "Import as one entry"), { fix(BankFixes.wholeBatch(session, batch.entry, true)) })
        }
        if batch.asOne && batch.line == place { return (String(localized: "Import its payments"), { fix(BankFixes.wholeBatch(session, batch.entry, false)) }) }
        return nil
    }

    private func undo(_ line: StatementLine) -> (String, () -> Void)? {
        guard line.fixed || line.included, let session else { return nil }
        let label = assisted.contains(line.id) ? String(localized: "Undo the assistant's fix") : String(localized: "Undo")
        return (label, { fix(BankFixes.revert(session, line)) })
    }

    // MARK: Reviewing the rows

    private func review(_ preview: BankFilePreview) -> some View {
        let counts = BankReview.counts(preview.rows, decisions)
        let shown = statement ?? preview.statement
        return List {
            Section { heading(BankReview.countsText(preview.summary)) }
            if shown.attention > 0 || shown.leftOutCount > 0 {
                Notice(text: BankStatementText.notImported(shown), tone: "info", icon: "tablecells", action: String(localized: "Check the file")) { stage = .lines }
                    .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            if let notice {
                Notice(text: notice, tone: "info").listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            if let error { FormErrorView(message: error) }
            if preview.summary.allIn && shown.format == "csv" && !(session?["structure"].flag("flipSigns") ?? false) {
                Notice(text: String(localized: "Every row came out as money in. If these are purchases, the file writes them as positive numbers."),
                       icon: "arrow.up.arrow.down", action: String(localized: "Reverse signs")) {
                    restructure { $0["flipSigns"] = true }
                }
                .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
            }
            BankReviewNotes(summary: preview.summary, undecided: counts.undecided)
            Section {
                BankReviewRows(rows: preview.rows, decisions: $decisions, enabled: !slow && !saving)
            } header: {
                BankReviewHeader(rows: preview.rows, decisions: $decisions, undecided: counts.undecided, enabled: !slow && !saving)
            } footer: {
                Text(BankReview.matchingNote)
            }
        }
        .groupedList()
    }

    // MARK: Actions

    private func begin() {
        guard !started else { return }
        started = true
        account = accounts.contains { $0.text("id") == presetAccount } ? presetAccount : accounts.first?.text("id") ?? ""
        if let opened { read(.success(opened)) }
    }

    /// Back to choosing the file, into the same account.
    private func startOver() {
        // Replies on their way, reviews too, are for the import that's gone.
        generation += 1
        reviews += 1
        fileName = ""; file = nil; content = nil; sheets = []; sheet = nil; pdf = nil
        session = nil; statement = nil; stage = nil; preview = nil; decisions = [:]; assisted = []
        error = nil; notice = nil; loading = false
    }

    private func read(_ result: Result<URL, Error>) {
        switch result {
        case .success(let picked):
            // A workbook opens at the sheet the account's last one was read from.
            let kept = budget.row("accounts", id: account)?["fileImport"].text("sheet") ?? ""
            open(picked, sheet: kept.isEmpty ? nil : kept)
        case .failure(let failure): error = BudgetStore.readable(failure)
        }
    }

    /// Reads a file (at `sheet` for a workbook), then starts an import of it.
    private func open(_ url: URL, sheet wanted: String?) {
        startOver()
        let current = generation
        loading = true
        Task {
            do {
                let picked = try await Task.detached(priority: .userInitiated) {
                    try ImportFiles.read(url, limit: 8 * 1024 * 1024, tooLarge: String(localized: "Choose a statement smaller than 8 MB."))
                }.value
                guard current == generation else { return }
                fileName = picked.name
                file = url
                // A PDF is the assistant's to read.
                if picked.name.lowercased().hasSuffix(".pdf") || picked.data.starts(with: [0x25, 0x50, 0x44, 0x46]) {
                    pdf = url
                    loading = false
                    return
                }
                let decoded = try await Task.detached(priority: .userInitiated) { try ImportFiles.statement(picked.data, sheet: wanted) }.value
                guard current == generation else { return }
                content = decoded.text
                sheets = decoded.sheets
                sheet = decoded.sheet
                let fresh = try await store.query("bankSession", ["accountId": .string(account), "content": .string(decoded.text),
                                                                  "sessionId": .string(UUID().uuidString.lowercased()), "budgetId": "",
                                                                  "sheet": JSON.text(orNull: decoded.sheet)])
                guard current == generation else { return }
                session = fresh
                refresh()
            } catch {
                guard current == generation else { return }
                self.error = BudgetStore.readable(error)
            }
            loading = false
        }
    }

    /// Reads the file again as the session says: the lines that need attention (or were changed),
    /// and the review when that's where it is.
    private func refresh(notice next: String? = nil) {
        guard let session, let content else { return }
        let current = generation
        let params: JSON = ["accountId": .string(account), "content": .string(content), "session": session, "problemsOnly": true]
        Task {
            do {
                let read = try await store.query("bankStatement", params)
                let parsed = await Task.detached(priority: .userInitiated) { BankStatement(read) }.value
                // A reply for the session as it was before a later change is dropped.
                guard current == generation, parsed.session == sessionId, parsed.revision == self.session?.long("revision") else { return }
                statement = parsed
                if stage == nil { stage = parsed.needsChecking ? .lines : .review }
                if stage == .review { runPreview(notice: next) }
            } catch {
                guard current == generation else { return }
                self.error = BudgetStore.readable(error)
                // A file that never read goes back to choosing one, with why.
                if statement == nil { self.session = nil }
            }
        }
    }

    /// The review of the lines that read, through the engine's `bankImportPreview`, against the published budget.
    private func runPreview(notice next: String? = nil) {
        guard let session, let content else { return }
        reviews += 1
        let asked = request
        let params: JSON = ["accountId": .string(asked.account), "content": .string(content), "session": session, "problemsOnly": true]
        error = nil
        Task {
            let revision = store.revision
            let budget = store.budget ?? [:]
            do {
                let result = try await store.query("bankImportPreview", params)
                let review = result["review"]
                let prepared = try await Task.detached(priority: .userInitiated) { () throws -> ([BankReviewRow], BankStatement) in
                    let rows = try BankReview.prepare(rows: review.rows("items"), budget: budget, allowIdlessDuplicate: true, candidateSets: review["candidateSets"].array)
                    return (rows, BankStatement(result["statement"]))
                }.value
                // A reply for another file, a reading of this one before a change, another
                // account or an earlier request is dropped.
                guard asked == request else { return }
                guard store.revision == revision else { return runPreview(notice: changedAgain) }
                preview = BankFilePreview(rows: prepared.0, summary: BankReview.summarize(prepared.0), revision: revision, statement: prepared.1, request: asked)
                decisions = [:]
                notice = next
            } catch {
                // Nor does such a reply's error show for the import that replaced it.
                guard asked == request else { return }
                self.error = BudgetStore.readable(error)
            }
        }
    }

    private func restructure(_ change: (inout JSON) -> Void) {
        guard let session else { return }
        fix(BankFixes.restructured(session, change))
    }

    /// Applies changes to the import where they still fit it, then reads the file again.
    private func fix(_ fixes: JSON) {
        // While a change applies, the lines' versions are the ones before it.
        guard !busy, let session, let content else { return }
        loading = true
        error = nil
        notice = nil
        let was = generation
        Task {
            do {
                let applied = try await store.query("bankApplyFixes", ["accountId": .string(account), "content": .string(content), "session": session, "fixes": fixes])
                // Another file chosen meanwhile is reading on its own: this change is for the one before.
                guard generation == was else { return }
                self.session = applied["session"]
                if let refused = applied.rows("results").first(where: { !$0.flag("applied") }) {
                    error = refused.text("problem").isEmpty ? String(localized: "That change no longer fits the file.") : refused.text("problem")
                }
                preview = nil
                refresh()
            } catch {
                guard generation == was else { return }
                self.error = BudgetStore.readable(error)
            }
            loading = false
        }
    }

    /// The assistant, asked to read a PDF statement into the account, with the PDF attached.
    private func readWithAssistant(_ pdf: URL) {
        let name = accountName
        asking = AssistantRequest(text: String(localized: "Import this statement into \(name). Find the transactions, ask me about anything unclear, and prepare them for the import review."),
                                  files: [pdf])
    }

    /// The assistant, opened over the import with the request ready: it's told the file's lines,
    /// and its fixes come back here.
    private func fixWithAssistant() {
        guard let session, let content else { return }
        let importing: JSON = ["account": .string(account), "content": .string(content), "session": session]
        asking = AssistantRequest(text: BankStatementText.fixImport, importing: importing) { thread, proposal in applyFix(thread, proposal) }
    }

    /// Fixes the assistant proposed: applied to this import, each only where its line hasn't changed.
    private func applyFix(_ thread: String, _ proposal: JSON) {
        let fixes = proposal["fixes"]
        guard fixes.isObject, let session, let content else { return }
        guard fixes["scope"].text("session") == sessionId else {
            AssistantController.shared.notice = String(localized: "These fixes are for another import.")
            return
        }
        let was = sessionId
        Task {
            do {
                let applied = try await store.query("bankApplyFixes", ["accountId": .string(account), "content": .string(content), "session": session, "fixes": fixes])
                guard sessionId == was else { return }
                let outcome = BankStatementText.outcome(applied)
                self.session = applied["session"]
                assisted.formUnion(outcome.lines)
                stage = .lines
                preview = nil
                if outcome.applied > 0 { AssistantController.shared.imported(thread: thread, proposal: proposal.text("id")) }
                // Back to the import, to see them.
                asking = nil
                refresh()
                notice = outcome.notice
            } catch {
                AssistantController.shared.notice = BudgetStore.readable(error)
            }
        }
    }

    private func importRows() {
        // Only the review of this import, checked against the budget as it is.
        guard let preview = shownPreview, preview.revision == store.revision else { return }
        let selected = decisions
        let accountId = account
        let importing = BankReview.counts(preview.rows, selected).importing
        // The account keeps how this file read for its next one.
        let remember = preview.statement.remember
        saving = true
        error = nil
        Task {
            do {
                let rows = preview.rows
                let choices = await Task.detached(priority: .userInitiated) {
                    BankReview.choices(rows, selected, allowIdlessDuplicate: true)
                }.value
                try await store.importsCommand("bankApply", ["accountId": .string(accountId), "choices": choices, "remember": remember],
                                               since: preview.revision, stale: changedAgain)
                store.showToast(String(localized: "Imported \(importing) transactions. They're waiting for your approval.", locale: .app), undo: true)
                dismiss()
            } catch {
                saving = false
                if store.revision != preview.revision {
                    runPreview(notice: changedAgain)
                } else {
                    self.error = BudgetStore.readable(error)
                }
            }
        }
    }
}

/// A line's date, payee, memo and amount, to give what doesn't read; the file's own cells shown for reference.
private struct LineFixSheet: View {
    var line: StatementLine
    var save: ([String: String]) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var values: [String: String] = [:]

    /// The line's cells as the file writes them.
    private var written: String { ImportsText.line(line.cells.filter { !$0.isEmpty }) }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("In the file: \(written)").font(.footnote).foregroundStyle(Color.secondary)
                    if !line.problem.isEmpty {
                        Label(line.problem, systemImage: "exclamationmark.triangle.fill").font(.footnote).foregroundStyle(Color.urWarning)
                    }
                }
                Section {
                    ForEach(BankStatementText.fields, id: \.id) { field in
                        TextField(field.label, text: Binding(get: { values[field.id] ?? "" }, set: { values[field.id] = $0 }))
                            .keyboardType(field.id == "amount" ? .numbersAndPunctuation : .default)
                            .textInputAutocapitalization(field.id == "payee" || field.id == "memo" ? .sentences : .never)
                            .autocorrectionDisabled(field.id != "memo")
                    }
                } footer: {
                    Text("Dates as YYYY-MM-DD; amounts negative for money out.")
                }
            }
            .navigationTitle("Fix line")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("Done") { save(values) }.fontWeight(.semibold) }
            }
        }
        .onAppear { for (id, _) in BankStatementText.fields { values[id] = line.fields[id] ?? "" } }
        .retainingEditorDraft("importLine", values: .object(values.mapValues(JSON.string)))
    }
}
