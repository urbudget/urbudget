import SwiftUI

/// A small form in a sheet, with Cancel and its action in the navigation bar.
struct FormSheet<Content: View>: View {
    @Environment(\.dismiss) private var dismiss
    var title: String
    var confirm = String(localized: "Save")
    var enabled = true
    var destructive = false
    /// Runs the change and waits for it to save; the sheet closes only when it has.
    var onConfirm: () async throws -> Void
    @ViewBuilder var content: Content
    @State private var error: String?
    @State private var saving = false

    var body: some View {
        NavigationStack {
            Form {
                content
                if let error { FormErrorView(message: error) }
            }
            .disabled(saving)
            .navigationTitle(title)
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(saving) }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button(confirm, role: destructive ? .destructive : nil) {
                            saving = true
                            error = nil
                            Task {
                                do {
                                    try await onConfirm()
                                    dismiss()
                                } catch {
                                    self.error = BudgetStore.readable(error)
                                }
                                saving = false
                            }
                        }
                        .fontWeight(.semibold)
                        .disabled(!enabled)
                    }
                }
            }
            .interactiveDismissDisabled(saving)
        }
    }
}

struct NewCategoryForm: View {
    @Environment(BudgetStore.self) private var store
    var presetGroup: String?
    var presetName = ""
    /// Called with the new category's id once it's saved, as a picker that offered to create it chooses it.
    var onCreated: ((String) -> Void)?
    @State private var name = ""
    @State private var group = ""
    @State private var newGroup = false
    @State private var started = false

    var body: some View {
        let existing = store.budget.map(groupNames) ?? []
        FormSheet(title: String(localized: "New category"), confirm: String(localized: "Add"),
                  enabled: !name.trimmingCharacters(in: .whitespaces).isEmpty && !group.trimmingCharacters(in: .whitespaces).isEmpty) {
            let id = try await store.saveCategory(name: name, group: group)
            onCreated?(id)
        } content: {
            TextField("Category name", text: $name).wordsCapitalized()
            if newGroup || existing.isEmpty {
                TextField("New group name", text: $group).wordsCapitalized()
            } else {
                Picker("Group", selection: $group) {
                    ForEach(existing, id: \.self) { Text($0).tag($0) }
                    Text("New group…").tag("__new")
                }
                .onChange(of: group) { _, value in if value == "__new" { newGroup = true; group = "" } }
            }
        }
        .presentationDetents([.medium])
        .onAppear {
            guard !started else { return }
            started = true
            name = presetName
            // A budget's first group is named in the app's language, as the engine names its own.
            group = presetGroup ?? existing.first ?? String(localized: "Everyday", comment: "The group of a budget's first category.")
        }
        .retainingEditorDraft("NewCategoryForm", values: ["name": .string(name), "group": .string(group)])
    }
}

/// A new, empty group: it waits in the group order with an "Add a category" row.
struct NewGroupForm: View {
    @Environment(BudgetStore.self) private var store
    @State private var name = ""

    var body: some View {
        let existing = store.budget.map(groupNames) ?? []
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        let taken = existing.contains(trimmed)
        FormSheet(title: String(localized: "New category group"), confirm: String(localized: "Add"), enabled: !trimmed.isEmpty && !taken) {
            try await store.editing { budget in
                var order = budget["groupOrder"].array
                if order.isEmpty { order = existing.map(JSON.string) }
                order.append(.string(trimmed))
                budget["groupOrder"] = .array(order)
            }
        } content: {
            TextField("Group name", text: $name).wordsCapitalized()
            if taken { FormErrorView(message: String(localized: "There is already a group called \(trimmed).")) }
        }
        .presentationDetents([.medium])
        .retainingEditorDraft("NewGroupForm", values: ["name": .string(name)])
    }
}

struct RenameGroupForm: View {
    @Environment(BudgetStore.self) private var store
    var group: String
    @State private var name = ""

    var body: some View {
        let existing = store.budget.map(groupNames) ?? []
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        FormSheet(title: String(localized: "Rename group"), enabled: !trimmed.isEmpty && trimmed != group) {
            try await store.committingWorkflow(["type": "renameGroup", "from": .string(group), "to": .string(trimmed)])
            if existing.contains(trimmed) { store.showToast(String(localized: "Merged \(group) into \(trimmed)."), undo: true) }
        } content: {
            Section {
                TextField("Group name", text: $name).wordsCapitalized()
            } footer: {
                Text("An existing group's name merges the two groups.")
            }
        }
        .presentationDetents([.medium])
        .onAppear { if name.isEmpty { name = group } }
        .retainingEditorDraft("RenameGroupForm", values: ["group": .string(group), "name": .string(name)])
    }
}

/// Deleting a group moves its categories to another group (a merge), or drops an empty group from the order.
struct DeleteGroupForm: View {
    @Environment(BudgetStore.self) private var store
    var group: String
    @State private var target = ""

    var body: some View {
        let others = (store.budget.map(groupNames) ?? []).filter { $0 != group }
        // Hidden categories count too: they'd be left in a group that no longer shows.
        let members = (store.budget?.rows("categories") ?? []).filter { $0.text("group") == group }
        let count = members.count
        let hidden = members.count { $0.flag("hidden") }
        FormSheet(title: String(localized: "Delete \(group)"), confirm: String(localized: "Delete"), enabled: count == 0 || !target.isEmpty, destructive: true) {
            if count == 0 {
                try await store.editing { budget in
                    budget["groupOrder"] = .array(budget["groupOrder"].array.filter { $0.string != group })
                }
                store.showToast(String(localized: "Deleted \(group)."), undo: true)
            } else {
                try await store.committingWorkflow(["type": "renameGroup", "from": .string(group), "to": .string(target)])
                store.showToast(String(localized: "Deleted \(group). Its categories moved to \(target)."), undo: true)
            }
        } content: {
            if count == 0 {
                Text("The group is empty.").foregroundStyle(Color.secondary)
            } else if others.isEmpty {
                Text("This is the only group. Add another group to move its categories to first.").foregroundStyle(Color.secondary)
            } else {
                Section {
                    Picker(selection: $target) {
                        ForEach(others, id: \.self) { Text($0).tag($0) }
                    } label: {
                        Text("Move its \(count) categories to")
                    }
                } footer: {
                    if hidden > 0 { Text("Including \(hidden) hidden categories.") }
                }
            }
        }
        .presentationDetents([.medium])
        .onAppear { if target.isEmpty { target = others.first ?? "" } }
        .retainingEditorDraft("DeleteGroupForm", values: ["group": .string(group), "target": .string(target)])
    }
}

/// The month's note and the income you expect, for comparing with targets.
struct BudgetPlanningForm: View {
    @Environment(BudgetStore.self) private var store
    var month: String
    @State private var note = ""
    @State private var income = ""
    @State private var started = false

    var body: some View {
        FormSheet(title: Dates.monthLabel(month)) {
            let expected: Int64? = income.trimmingCharacters(in: .whitespaces).isEmpty ? nil : try Money.parse(income)
            if let expected, expected < 0 { throw AppError(String(localized: "Expected income must be zero or positive.")) }
            try await store.committingWorkflow(["type": "setBudgetPlanning", "month": .string(month), "note": .string(note),
                            "expectedIncome": expected.map(JSON.int) ?? .null])
        } content: {
            Section("Month note") {
                TextField("Note", text: $note, axis: .vertical).lineLimit(4...10)
            }
            Section {
                AmountField(title: String(localized: "Expected monthly income (\(store.currency))"), text: $income)
            } footer: {
                Text("Expected income helps compare your targets with your plan. It doesn’t add money to Unassigned.")
            }
        }
        .onAppear {
            guard !started, let budget = store.budget else { return }
            started = true
            note = budget.rows("ynabMonths").first { $0.text("month").hasPrefix(month) }?.text("note") ?? ""
            income = budget["expectedIncome"].isNull ? "" : Money.editable(budget.long("expectedIncome"))
        }
        .retainingEditorDraft("BudgetPlanningForm", values: ["month": .string(month), "note": .string(note), "income": .string(income)])
    }
}

/// Moves a category to another group (or a new one), at the end of it.
struct MoveToGroupForm: View {
    @Environment(BudgetStore.self) private var store
    var categoryId: String
    @State private var group = ""
    @State private var newGroup = ""

    var body: some View {
        let groups = store.budget.map(groupNames) ?? []
        let current = store.budget?.row("categories", id: categoryId)
        let target = group == "__new" ? newGroup.trimmingCharacters(in: .whitespaces) : group
        let name = current?.text("name")
        let shown = name ?? String(localized: "category")
        FormSheet(title: String(localized: "Move \(shown)"), confirm: String(localized: "Move"),
                  enabled: !target.isEmpty && target != current?.text("group")) {
            try await store.committingWorkflow(["type": "organizeCategory", "id": .string(categoryId), "group": .string(target), "beforeId": .null])
            if let name {
                store.showToast(String(localized: "Moved \(name) to \(target)."), undo: true)
            } else {
                store.showToast(String(localized: "Moved the category to \(target)."), undo: true)
            }
        } content: {
            Picker("Group", selection: $group) {
                ForEach(groups, id: \.self) { Text($0).tag($0) }
                Text("New group…").tag("__new")
            }
            .pickerStyle(.inline)
            if group == "__new" { TextField("New group name", text: $newGroup).wordsCapitalized() }
        }
        .presentationDetents([.medium, .large])
        .onAppear { if group.isEmpty { group = current?.text("group") ?? groups.first ?? "" } }
    }
}


/// Ordered durable raw fields. A forced budget switch leaves the former editor recoverable.
@MainActor
final class EditorDraftCheckpoint {
    let id = UUID().uuidString
    private let identity: String?
    private let target: String
    private let baseline: JSON
    private weak var store: BudgetStore?
    private var budgetID: String?
    private var epoch: Int64 = 0
    private var previous: JSON?
    private var checkpointFailed = false
    private var tail: Task<Void, Never>?

    init(store: BudgetStore, target: String, baseline: JSON) {
        self.store = store; self.identity = store.identity; self.target = target; self.baseline = baseline
    }
    private func owner() async throws -> String {
        if let budgetID { return budgetID }
        let library = try await Task.detached { try NativeCore.service("library_snapshot") }.value
        guard let row = library.rows("entries").first(where: { $0.text("path") == identity }) else {
            throw AppError(String(localized: "The editor's budget is no longer in the library."))
        }
        budgetID = row.text("id")
        return row.text("id")
    }
    func save(_ values: JSON) {
        guard previous != values else { return }
        previous = values
        epoch += 1
        let next = epoch, before = tail
        tail = Task {
            await before?.value
            guard next == epoch else { return }
            do {
                let params: JSON = ["budgetId": .string(try await owner()), "id": .string(id), "epoch": .string(String(next)),
                                    "target": .string(target), "baseline": baseline, "values": values]
                _ = try await Task.detached { try NativeCore.service("editor_checkpoint", params) }.value
                checkpointFailed = false
            } catch { checkpointFailed = true; store?.error = BudgetStore.readable(error) }
        }
    }
    func discard(onlyIfActive: Bool = false) {
        let savedEpoch = epoch, before = tail
        tail = Task {
            await before?.value
            do {
                let budget = try await owner()
                if onlyIfActive && checkpointFailed { return }
                let params: JSON = ["budgetId": .string(budget), "id": .string(id), "epoch": .string(String(savedEpoch)), "onlyIfActive": .bool(onlyIfActive)]
                _ = try await Task.detached { try NativeCore.service("editor_discard", params) }.value
            } catch { store?.error = BudgetStore.readable(error) }
        }
    }
}


private struct RetainedEditorDraft: ViewModifier {
    @Environment(BudgetStore.self) private var store
    var target: String
    var values: JSON
    var enabled: Bool
    @State private var checkpoint: EditorDraftCheckpoint?
    @State private var identity: String?
    func body(content: Content) -> some View {
        content.onAppear {
            guard enabled, store.identity != nil else { return }
            if checkpoint == nil {
                identity = store.identity
                checkpoint = EditorDraftCheckpoint(store: store, target: target, baseline: editorBaseline(store.budget ?? [:], values: values, version: store.version))
            }
            checkpoint?.save(values)
        }
        .onChange(of: enabled) { _, active in
            if active, store.identity != nil {
                identity = store.identity
                checkpoint = EditorDraftCheckpoint(store: store, target: target, baseline: editorBaseline(store.budget ?? [:], values: values, version: store.version))
                checkpoint?.save(values)
            } else {
                let closing = checkpoint, owner = identity
                checkpoint = nil
                store.afterPending { if owner == store.identity && store.error == nil { closing?.discard(onlyIfActive: true) } }
            }
        }
        .onChange(of: values) { _, next in if enabled { checkpoint?.save(next) } }
        .onDisappear {
            let closing = checkpoint, owner = identity
            store.afterPending { if owner == store.identity && store.error == nil { closing?.discard(onlyIfActive: true) } }
        }
    }
}
extension View {
    func retainingEditorDraft(_ target: String, values: JSON, enabled: Bool = true) -> some View {
        modifier(RetainedEditorDraft(target: target, values: values, enabled: enabled))
    }
}

/// Opening records needed to review a draft; large history remains in the retained budget database.
private func editorBaseline(_ budget: JSON, values: JSON, version: Int) -> JSON {
    var references = Set<String>()
    func collect(_ value: JSON) {
        switch value {
        case .string(let text): if !text.isEmpty { references.insert(text) }
        case .array(let rows): rows.forEach(collect)
        case .object(let fields): fields.values.forEach(collect)
        default: break
        }
    }
    collect(values)
    var result: JSON = ["name": budget["name"], "currency": budget["currency"], "version": .string(String(version))]
    for table in ["accounts", "categories", "loans", "schedules", "payees", "payeeRules", "ynabMonths", "assignments"] {
        result[table] = .array(budget.rows(table).filter { row in
            (table == "schedules" ? ["id"] : ["id", "accountId", "categoryId", "name", "month"]).contains { references.contains(row.text($0)) }
        })
    }
    return result
}
