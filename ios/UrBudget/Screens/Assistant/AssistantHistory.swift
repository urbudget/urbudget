import SwiftUI

/// Saved conversations: search them by title, open one, rename it, or delete it from this device.
struct AssistantHistorySheet: View {
    @Environment(\.dismiss) private var dismiss
    private let controller = AssistantController.shared
    @State private var deleting: String?
    @State private var search = ""
    /// The conversation being renamed, and the name typed for it.
    @State private var renaming: String?
    @State private var newTitle = ""

    var body: some View {
        let shown = AssistantHistory.matching(controller.threads, search)
        NavigationStack {
            List {
                if let error = controller.error { FormErrorView(message: error) }
                if controller.threads.isEmpty {
                    Text("No saved conversations yet").foregroundStyle(Color.secondary)
                } else if shown.isEmpty {
                    Text("No conversation matches.").foregroundStyle(Color.secondary)
                }
                // By its id: a row keeps its place and its swipe when the list is searched or reloaded.
                ForEach(shown, id: \.idText) { thread in
                    row(thread)
                }
            }
            .groupedList()
            .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search conversations")
            .navigationTitle("Conversations")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
            .task { controller.reloadHistory() }
            .alert("Delete this conversation?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } })) {
                Button("Delete", role: .destructive) {
                    if let deleting { controller.deleteThread(deleting) }
                    deleting = nil
                }
                Button("Cancel", role: .cancel) { deleting = nil }
            } message: {
                Text("This removes its saved history from this device.")
            }
            .alert("Rename conversation", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
                TextField("Conversation name", text: $newTitle)
                Button("Save") {
                    let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
                    if let renaming, !title.isEmpty { controller.renameThread(renaming, to: title) }
                    renaming = nil
                }
                Button("Cancel", role: .cancel) { renaming = nil }
            }
        }
        .presentationDetents([.medium, .large])
    }

    /// A conversation: its title and when it was last added to, and for the open one, the
    /// tokens it has used.
    private func row(_ thread: JSON) -> some View {
        let id = thread.text("id")
        let isOpen = id == controller.threadId
        let usage = isOpen ? AssistantUsage.line(controller.thread?["usage"] ?? .null) : nil
        return Button {
            if !isOpen { controller.openThread(id) }
            dismiss()
        } label: {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(thread.text("title").isEmpty ? String(localized: "New conversation") : thread.text("title"))
                        .foregroundStyle(Color.primary)
                        .lineLimit(2)
                    Text(Money.date(String(thread.text("updated").prefix(10))))
                        .font(.footnote)
                        .foregroundStyle(Color.secondary)
                    if let usage {
                        Text(usage).font(.caption).foregroundStyle(Color.secondary)
                    }
                }
                Spacer(minLength: 8)
                if isOpen {
                    Image(systemName: "checkmark").foregroundStyle(Color.urAccent)
                        .accessibilityLabel(String(localized: "state.open", defaultValue: "Open", comment: "Something that is open now (a state, not a button)."))
                }
            }
        }
        .swipeActions {
            Button("Delete", systemImage: "trash", role: .destructive) { deleting = id }
            Button("Rename conversation", systemImage: "pencil") { rename(thread) }.tint(Color.urAccent)
        }
        .contextMenu {
            Button("Rename conversation", systemImage: "pencil") { rename(thread) }
            Button("Delete conversation", systemImage: "trash", role: .destructive) { deleting = id }
        }
    }

    private func rename(_ thread: JSON) {
        newTitle = thread.text("title")
        renaming = thread.text("id")
    }
}

/// Transactions to discuss in the next message: up to 200, newest first, with search.
struct AssistantContextPicker: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @Binding var selected: [String]
    @State private var rows: [AssistantContextRow]?
    @State private var search = ""

    static let limit = AssistantSelection.limit

    var body: some View {
        let chosen = Set(selected)
        let query = search.trimmingCharacters(in: .whitespaces).lowercased()
        let shown = (rows ?? []).filter { query.isEmpty || $0.search.contains(query) }
        NavigationStack {
            List {
                Section {
                    if rows == nil {
                        ProgressView().frame(maxWidth: .infinity)
                    } else if shown.isEmpty {
                        Text(query.isEmpty ? "No transactions yet" : "No matching transactions").foregroundStyle(Color.secondary)
                    }
                    ForEach(shown) { row in
                        let on = chosen.contains(row.id)
                        Button {
                            if on { selected.removeAll { $0 == row.id } } else if selected.count < Self.limit { selected.append(row.id) }
                        } label: {
                            HStack(spacing: 12) {
                                Image(systemName: on ? "checkmark.circle.fill" : "circle")
                                    .font(.title3)
                                    .foregroundStyle(on ? Color.urAccent : .secondary)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(row.payee).foregroundStyle(Color.primary).lineLimit(1)
                                    Text(row.detail).font(.footnote).foregroundStyle(Color.secondary).lineLimit(1)
                                }
                                Spacer(minLength: 8)
                                Text(row.amount).monospacedDigit().foregroundStyle(Color.primary)
                            }
                        }
                        .disabled(!on && selected.count >= Self.limit)
                        .accessibilityElement(children: .combine)
                        .accessibilityAddTraits(on ? .isSelected : [])
                    }
                } header: {
                    Text("Select up to 200 transactions for your next message.")
                }
            }
            .groupedList()
            .searchable(text: $search, placement: .navigationBarDrawer(displayMode: .always), prompt: "Payee, memo, account or amount")
            .navigationTitle("Transactions to discuss")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Clear") { selected = [] }.disabled(selected.isEmpty)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done (\(selected.count))") { dismiss() }.fontWeight(.semibold)
                }
            }
            .task(id: store.version) {
                let budget = store.budget ?? [:]
                rows = await Task.detached(priority: .userInitiated) { AssistantContextRow.rows(budget) }.value
            }
        }
    }
}

/// A transaction as the context picker lists it, prepared off the main thread.
struct AssistantContextRow: Identifiable, Sendable {
    var id: String
    var payee: String
    var detail: String
    var amount: String
    var search: String

    static func rows(_ budget: JSON) -> [AssistantContextRow] {
        let currency = budget.text("currency")
        let accounts = Dictionary(budget.rows("accounts").map { ($0.text("id"), $0.text("name")) }, uniquingKeysWith: { first, _ in first })
        return budget.rows("transactions")
            .sorted { $0.text("date") > $1.text("date") }
            .map { transaction in
                let account = accounts[transaction.text("accountId")] ?? ""
                let payee = transaction.text("payee").isEmpty ? String(localized: "No payee") : transaction.text("payee")
                let amount = money(transaction.long("amount"), currency)
                let detail = [Money.date(transaction.text("date")), account, transaction.text("memo")].filter { !$0.isEmpty }.joined(separator: " · ")
                return AssistantContextRow(id: transaction.text("id"), payee: payee, detail: detail, amount: amount,
                                           search: [payee, account, transaction.text("memo"), amount, transaction.text("date")].joined(separator: " ").lowercased())
            }
    }
}
