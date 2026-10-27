import SwiftUI

/// The statement the assistant converted, reviewed row by row before one atomic budget change:
/// add each row, match it to a transaction you already entered, or skip it.
struct AssistantImportReview: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var request: AssistantImportRequest
    var onImported: () -> Void

    @State private var account = ""
    @State private var flipSigns = false
    @State private var rows: [AssistantBankRow]?
    @State private var summary: AssistantBankSummary?
    @State private var decisions: [Int: String] = [:]
    @State private var previewVersion: Int?
    @State private var loading = false
    @State private var saving = false
    @State private var error: String?
    @State private var started = false

    private var fileName: String { request.file.text("name").isEmpty ? String(localized: "Statement") : request.file.text("name") }

    var body: some View {
        let accounts = (store.budget?.rows("accounts") ?? []).filter { !$0.flag("archived") }
        let accountName = accounts.first { $0.text("id") == account }?.text("name") ?? ""
        let counts = AssistantBankReview.counts(rows ?? [], decisions)
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
                            Text(summary.text).font(.footnote).foregroundStyle(Color.secondary)
                        }
                        if summary.allIn && !flipSigns {
                            Notice(text: String(localized: "Every row came out as money in. If these are purchases, the file writes them as positive numbers."),
                                   tone: "attention", icon: "arrow.up.arrow.down", action: String(localized: "Reverse signs")) { flipSigns = true }
                        }
                        if summary.idless {
                            Notice(text: String(localized: "This file has no bank transaction IDs. A row may already be imported; compare its memo and amount. Skip it if it's the same payment, or add it separately if it's a different one."),
                                   tone: "info", icon: "info.circle.fill")
                        }
                        if counts.undecided > 0 {
                            Notice(text: String(localized: "\(counts.undecided) rows need a choice before importing.", locale: .app),
                                   icon: "exclamationmark.triangle.fill")
                        }
                    } footer: {
                        Text("Matching keeps your category and memo and marks the transaction cleared. Imported transactions wait for your approval.")
                    }
                    Section {
                        ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                            rowView(row, index: index)
                        }
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

    private func rowView(_ row: AssistantBankRow, index: Int) -> some View {
        let decision = row.decision(decisions[index])
        return VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text(row.payee).font(.body.weight(.semibold)).foregroundStyle(row.locked ? .secondary : .primary).lineLimit(2)
                Spacer(minLength: 10)
                Text(row.amountText).font(.body.weight(.semibold)).monospacedDigit()
                    .foregroundStyle(row.locked ? Color.secondary : row.amount > 0 ? Color.urPositive : Color.primary)
                    .lineLimit(1)
            }
            if let bank = row.bankText {
                Text("Bank: \(bank)").font(.footnote).foregroundStyle(Color.secondary).lineLimit(1)
            }
            if !row.detail.isEmpty {
                Text(row.detail).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2)
            }
            if row.locked {
                Text("Already imported · skipped").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary)
            } else {
                if decision == "review" {
                    Text(row.possibleDuplicate ? "May already be imported: compare the memo and amount, then choose" : "Choose the matching transaction, or add it as new")
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(Color.urWarning)
                }
                Picker("What to do", selection: Binding(get: { row.decision(decisions[index]) }, set: { decisions[index] = $0 })) {
                    ForEach(row.options, id: \.value) { Text($0.label).tag($0.value) }
                }
                .pickerStyle(.menu)
                .disabled(loading || saving)
            }
        }
        .padding(.vertical, 2)
    }

    private func preview() async {
        guard !account.isEmpty, let budget = store.budget else { return }
        loading = true
        error = nil
        rows = nil
        summary = nil
        decisions = [:]
        let version = store.version
        let params: JSON = ["accountId": .string(account), "content": .string(request.file.text("csv")), "format": "csv",
                            "dateFormat": "ymd", "flipSigns": .bool(flipSigns)]
        do {
            let result = try await store.query("bankPreview", params)
            try Task.checkCancellation()
            let prepared = try await Task.detached(priority: .userInitiated) { () throws -> ([AssistantBankRow], AssistantBankSummary) in
                let rows = try AssistantBankReview.prepare(result.rows("items"), budget: budget, candidateSets: result.rows("candidateSets"))
                return (rows, AssistantBankReview.summary(rows))
            }.value
            try Task.checkCancellation()
            guard store.version == version else { throw AppError(String(localized: "Your budget changed. Preview this statement again.")) }
            rows = prepared.0
            summary = prepared.1
            previewVersion = version
            if prepared.0.isEmpty { error = String(localized: "The file has no transactions to import.") }
        } catch is CancellationError {
            return
        } catch {
            self.error = BudgetStore.readable(error)
        }
        loading = false
    }

    private func importRows() {
        guard let rows, previewVersion == store.version else {
            error = String(localized: "Your budget changed. Preview this statement again.")
            Task { await preview() }
            return
        }
        saving = true
        error = nil
        let choices = AssistantBankReview.choices(rows, decisions)
        let params: JSON = ["accountId": .string(account), "choices": .array(choices)]
        Task {
            do {
                try await store.perform { repository in ((), try await repository.command("bankApply", params)) }
                onImported()
                dismiss()
            } catch {
                self.error = BudgetStore.readable(error)
                saving = false
            }
        }
    }
}
