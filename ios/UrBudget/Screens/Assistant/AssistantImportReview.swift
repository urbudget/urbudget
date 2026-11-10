import SwiftUI

/// The statement the assistant converted, reviewed row by row before one atomic budget change:
/// add each row, match it to a transaction you already entered, or skip it. The rows, their
/// choices and what's said about them are the bank review's own (`BankReview`), as a bank
/// file's import and a bank download use them: a transfer that's only suggested waits for a
/// choice, a refund names its purchase, and the import is refused when the budget changed
/// since the rows were checked.
struct AssistantImportReview: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var request: AssistantImportRequest
    var onImported: () -> Void

    @State private var account = ""
    @State private var flipSigns = false
    @State private var rows: [BankReviewRow]?
    @State private var summary: BankReviewSummary?
    @State private var decisions: [Int: String] = [:]
    /// The budget revision the rows were checked against.
    @State private var previewRevision: String?
    @State private var loading = false
    @State private var saving = false
    @State private var error: String?
    @State private var started = false

    private var fileName: String { request.file.text("name").isEmpty ? String(localized: "Statement") : request.file.text("name") }

    var body: some View {
        let accounts = (store.budget?.rows("accounts") ?? []).filter { !$0.flag("archived") }
        let accountName = accounts.first { $0.text("id") == account }?.text("name") ?? ""
        let counts = BankReview.counts(rows ?? [], decisions)
        NavigationStack {
            List {
                Section {
                    Picker("Into account", selection: $account) {
                        if account.isEmpty { Text("Choose an account").tag("") }
                        ForEach(accounts, id: \.idText) { Text($0.text("name")).tag($0.text("id")) }
                    }
                    .disabled(loading || saving)
                } footer: {
                    Text("Bring in the transactions your bank exported. You check every row, and how it matches what you already entered, before anything is saved.")
                }
                if loading {
                    HStack { Spacer(); ProgressView(); Spacer() }
                }
                if let error { FormErrorView(message: error) }
                if let rows, let summary {
                    Section {
                        VStack(alignment: .leading, spacing: 3) {
                            Text("\(fileName) into \(accountName)").font(.subheadline.weight(.semibold))
                            Text(BankReview.countsText(summary)).font(.footnote).foregroundStyle(Color.secondary)
                        }
                        if summary.allIn && !flipSigns {
                            Notice(text: String(localized: "Every row came out as money in. If these are purchases, the file writes them as positive numbers."),
                                   tone: "attention", icon: "arrow.up.arrow.down", action: String(localized: "Reverse signs")) { flipSigns = true }
                        }
                    }
                    BankReviewNotes(summary: summary, undecided: counts.undecided)
                    Section {
                        BankReviewRows(rows: rows, decisions: $decisions, enabled: !loading && !saving)
                    } header: {
                        BankReviewHeader(rows: rows, decisions: $decisions, undecided: counts.undecided, enabled: !loading && !saving)
                    } footer: {
                        Text(BankReview.matchingNote)
                    }
                }
            }
            .groupedList()
            .navigationTitle("Import bank file")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(saving) }
                ToolbarItem(placement: .confirmationAction) {
                    Button(saving ? "Importing…" : "Import \(counts.importing)") { importRows() }
                        .fontWeight(.semibold)
                        .disabled(loading || saving || rows == nil || counts.undecided > 0 || counts.importing == 0)
                }
            }
            .interactiveDismissDisabled(saving)
            .onAppear {
                guard !started else { return }
                started = true
                let suggested = request.file.text("account")
                account = accounts.contains { $0.text("id") == suggested } ? suggested : accounts.first?.text("id") ?? ""
            }
            .task(id: "\(account)|\(flipSigns)") { await preview() }
        }
    }

    private func preview() async {
        guard !account.isEmpty, let budget = store.budget else { return }
        loading = true
        error = nil
        rows = nil
        summary = nil
        decisions = [:]
        let revision = store.revision
        let params: JSON = ["accountId": .string(account), "content": .string(request.file.text("csv")), "format": "csv",
                            "dateFormat": "ymd", "flipSigns": .bool(flipSigns)]
        do {
            let result = try await store.query("bankPreview", params)
            try Task.checkCancellation()
            let prepared = try await Task.detached(priority: .userInitiated) { () throws -> [BankReviewRow] in
                try BankReview.prepare(rows: result.rows("items"), budget: budget, allowIdlessDuplicate: true, candidateSets: result["candidateSets"].array)
            }.value
            try Task.checkCancellation()
            guard store.revision == revision else { throw AppError(String(localized: "Your budget changed. Preview this statement again.")) }
            rows = prepared
            summary = BankReview.summarize(prepared)
            previewRevision = revision
            if prepared.isEmpty { error = String(localized: "The file has no transactions to import.") }
        } catch is CancellationError {
            return
        } catch {
            self.error = BudgetStore.readable(error)
        }
        loading = false
    }

    private func importRows() {
        let stale = String(localized: "Your budget changed. Preview this statement again.")
        guard let rows, let revision = previewRevision, revision == store.revision else {
            error = stale
            Task { await preview() }
            return
        }
        saving = true
        error = nil
        let chosen = decisions
        let accountId = account
        let importing = BankReview.counts(rows, chosen).importing
        Task {
            do {
                let choices = await Task.detached(priority: .userInitiated) {
                    BankReview.choices(rows, chosen, allowIdlessDuplicate: true)
                }.value
                // Refused when the budget changed since the rows were checked: nothing slips in between.
                try await store.importsCommand("bankApply", ["accountId": .string(accountId), "choices": choices], since: revision, stale: stale)
                // Said in the assistant, which is still open over the app's own messages, with Undo.
                AssistantController.shared.sayImported(
                    String(localized: "Imported \(importing) transactions. They're waiting for your approval.", locale: .app), revision: store.revision)
                onImported()
                dismiss()
            } catch {
                self.error = BudgetStore.readable(error)
                saving = false
                // Checked again against the budget as it is now.
                if store.revision != revision { await preview() }
            }
        }
    }
}
