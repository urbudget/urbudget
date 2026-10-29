import SwiftUI

// MARK: Payees & rules

/// A payee row, prepared off the main thread.
private struct PayeeLine: Identifiable {
    var name: String
    var subtitle: String
    var id: String { name }
}

/// An import rule row.
private struct PayeeRuleLine: Identifiable {
    var id: String
    var rule: JSON
    var title: String
    var subtitle: String
}

private func payeeLines(_ budget: JSON) -> [PayeeLine] {
    var categories: [String: String] = [:]
    for c in budget.rows("categories") { categories[c.text("id")] = c.text("name") }
    return payeeIndex(budget).map { info in
        let usage = info.count > 0 ? "\(transactionCount(info.count)) · \(Money.date(info.last))" : String(localized: "Not used in transactions yet")
        let fixed = info.defaultCategory.map { id -> String in
            let name = id == Unassigned ? String(localized: "Unassigned") : categories[id] ?? String(localized: "Deleted category")
            return String(localized: "Default: \(name)")
        }
        return PayeeLine(name: info.name, subtitle: [usage, fixed].compactMap { $0 }.joined(separator: " · "))
    }
}

private func ruleLines(_ budget: JSON) -> [PayeeRuleLine] {
    budget.rows("payeeRules").enumerated().map { index, rule in
        let rename = rule.text("payee").trimmingCharacters(in: .whitespaces)
        let category = rule.text("categoryId").isEmpty ? nil : planningCategoryName(budget, rule.text("categoryId"))
        let subtitle = [rename.isEmpty ? nil : String(localized: "Rename to \(rename)"), category].compactMap { $0 }.joined(separator: " · ")
        let match = rule.text("match")
        return PayeeRuleLine(id: rule.text("id").isEmpty ? "rule:\(index)" : rule.text("id"), rule: rule, title: String(localized: "Contains “\(match)”"),
                             subtitle: subtitle.isEmpty ? String(localized: "Keep payee name") : subtitle)
    }
}

/// What the payee editor opens with: a payee's name, or nil to add one.
private struct PayeeEditorRequest: Identifiable {
    let id = UUID()
    var name: String?
}

/// What the rule editor opens with: a rule, or nil (with a name to rename to) to add one.
private struct PayeeRuleRequest: Identifiable {
    let id = UUID()
    var rule: JSON?
    var payee = ""
}

/// Payees & rules: the payees with how often they're used and their default category, each opening
/// for its details, renaming or merging; then the import rules that rename and categorize bank
/// transactions, with how many existing transactions they'd change (PayeeScreen.kt, PayeesPage.tsx).
struct PayeesView: View {
    @Environment(BudgetStore.self) private var store
    @State private var tab = "payees"
    @State private var search = ""
    @State private var lines: [PayeeLine]?
    @State private var pending: Int?
    @State private var editingPayee: PayeeEditorRequest?
    @State private var editingRule: PayeeRuleRequest?
    @State private var deletingRule: PayeeRuleLine?

    init() {}

    var body: some View {
        let rules = store.budget.map(ruleLines) ?? []
        List {
            Section {
                Picker("Show", selection: $tab) {
                    Text(lines.map { String(localized: "Payees (\($0.count))", locale: .app) } ?? String(localized: "Payees")).tag("payees")
                    Text("Import rules (\(rules.count))").tag("rules")
                }
                .pickerStyle(.segmented)
            }
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets())
            if tab == "payees" { payeeSections } else { ruleSections(rules) }
        }
        .groupedList()
        .navigationTitle("Payees & rules")
        .searchable(text: $search, prompt: tab == "payees" ? "Search payees" : "Search rules")
        .toolbar {
            ToolbarItem(placement: .trailingBar) {
                Button {
                    if tab == "payees" { editingPayee = PayeeEditorRequest() } else { editingRule = PayeeRuleRequest() }
                } label: {
                    Image(systemName: "plus")
                }
                .accessibilityLabel(tab == "payees" ? "Add payee" : "Add import rule")
            }
        }
        .task(id: store.version) {
            guard let budget = store.budget else { return }
            let prepared = await Task.detached(priority: .userInitiated) { payeeLines(budget) }.value
            guard !Task.isCancelled else { return }
            lines = prepared
            let count = await Task.detached(priority: .utility) { try? PayeeRulePreview.changes(budget: budget) }.value
            if !Task.isCancelled { pending = count }
        }
        .sheet(item: $editingPayee) { PayeeEditor(name: $0.name) }
        .sheet(item: $editingRule) { PayeeRuleEditor(rule: $0.rule, payee: $0.payee) }
        .confirmationDialog("Delete this rule?", isPresented: Binding(get: { deletingRule != nil }, set: { if !$0 { deletingRule = nil } }),
                            titleVisibility: .visible, presenting: deletingRule) { line in
            Button("Delete", role: .destructive) {
                store.remove("payeeRules", id: line.rule.text("id")) { store.showToast(String(localized: "Deleted the rule."), undo: true) }
            }
        } message: { line in
            Text("\(line.title) won't rename or categorize imported transactions any more. Transactions it already changed stay as they are.")
        }
    }

    @ViewBuilder private var payeeSections: some View {
        let query = search.trimmingCharacters(in: .whitespaces)
        if let lines {
            let shown = query.isEmpty ? lines : lines.filter { $0.name.localizedCaseInsensitiveContains(query) }
            if lines.isEmpty {
                EmptyState(title: String(localized: "No payees yet"), detail: String(localized: "Payees appear here as you enter transactions."),
                           symbol: "person.2", action: String(localized: "Add payee")) {
                    editingPayee = PayeeEditorRequest()
                }
                .listRowBackground(Color.clear)
            } else if shown.isEmpty {
                ContentUnavailableView.search(text: query).listRowBackground(Color.clear)
            } else {
                Section {
                    ForEach(shown) { line in
                        Button { editingPayee = PayeeEditorRequest(name: line.name) } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(line.name).foregroundStyle(Color.primary)
                                Text(line.subtitle).font(.footnote).foregroundStyle(Color.secondary).lineLimit(2)
                            }
                        }
                        .contextMenu {
                            Button { editingPayee = PayeeEditorRequest(name: line.name) } label: { Label("Edit payee", systemImage: "pencil") }
                            Button { editingRule = PayeeRuleRequest(payee: line.name) } label: { Label("Add an import rule for it", systemImage: "wand.and.stars") }
                        }
                    }
                } footer: {
                    Text("Tidy names, and give payees a default category for new transactions.")
                }
            }
        } else {
            Section { ProgressView().frame(maxWidth: .infinity) }
        }
    }

    @ViewBuilder private func ruleSections(_ all: [PayeeRuleLine]) -> some View {
        let query = search.trimmingCharacters(in: .whitespaces)
        let shown = query.isEmpty ? all : all.filter { "\($0.rule.text("match")) \($0.rule.text("payee"))".localizedCaseInsensitiveContains(query) }
        if let pending, pending > 0 {
            Section {
                Notice(text: String(localized: "The rules would rename or categorize \(transactionCount(pending)) you already have."), tone: "info",
                       icon: "wand.and.stars", action: String(localized: "Apply")) { reapply(pending) }
            }
            .listRowInsets(EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12))
            .listRowBackground(Color.clear)
        }
        if all.isEmpty {
            EmptyState(title: String(localized: "No import rules"),
                       detail: String(localized: "Rules rename and categorize transactions as they come in from bank files and bank sync."),
                       symbol: "wand.and.stars", action: String(localized: "Add import rule")) { editingRule = PayeeRuleRequest() }
                .listRowBackground(Color.clear)
        } else if shown.isEmpty {
            ContentUnavailableView.search(text: query).listRowBackground(Color.clear)
        } else {
            Section {
                ForEach(shown) { line in
                    let index = all.firstIndex { $0.id == line.id } ?? 0
                    Button { editingRule = PayeeRuleRequest(rule: line.rule) } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(line.title).foregroundStyle(Color.primary)
                            Text(line.subtitle).font(.footnote).foregroundStyle(Color.secondary)
                        }
                    }
                    .swipeActions {
                        Button { deletingRule = line } label: { Label("Delete", systemImage: "trash") }.tint(.urNegative)
                    }
                    .contextMenu {
                        Button { editingRule = PayeeRuleRequest(rule: line.rule) } label: { Label("Edit rule", systemImage: "pencil") }
                        Button { move(line, "up") } label: { Label("Move earlier", systemImage: "arrow.up") }.disabled(index == 0)
                        Button { move(line, "down") } label: { Label("Move later", systemImage: "arrow.down") }.disabled(index == all.count - 1)
                        Button { apply(line) } label: { Label("Apply to existing transactions", systemImage: "wand.and.stars") }
                        Divider()
                        Button(role: .destructive) { deletingRule = line } label: { Label("Delete rule", systemImage: "trash") }
                    }
                }
            } footer: {
                Text("A rule matches when the bank's payee contains its text, ignoring case and punctuation. The longest pattern wins; list order breaks ties.")
            }
            Section {
                Button { reapply(pending) } label: { Label("Apply rules to existing transactions", systemImage: "wand.and.stars") }
                    .disabled(store.busy)
            }
        }
    }

    private func reapply(_ count: Int?) {
        store.workflow(["type": "reapplyPayeeRules"]) {
            store.showToast(count.map { String(localized: "Updated \(transactionCount($0)).") } ?? String(localized: "Applied the rules."), undo: true)
        }
    }

    private func apply(_ line: PayeeRuleLine) {
        store.workflow(["type": "reapplyPayeeRules", "ruleIds": JSON([line.rule.text("id")])]) {
            store.showToast(String(localized: "Applied \(line.title) to existing transactions."), undo: true)
        }
    }

    private func move(_ line: PayeeRuleLine, _ direction: String) {
        store.workflow(["type": "reorderPayeeRule", "id": .string(line.rule.text("id")), "direction": .string(direction)])
    }
}

// MARK: Payee

private enum PayeeRoute: Hashable { case category, merge }

/// A payee: how often it's used, its latest transactions, its default category (or the one used
/// last), and renaming or merging it into another payee (PayeeScreen.kt's PayeeDialog). Saving runs
/// the engine's savePayee, which updates transactions, split payees, schedules and rules.
private struct PayeeEditor: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var original: String?

    init(name: String?) { original = name }

    @State private var loaded = false
    @State private var name = ""
    @State private var category = ""
    @State private var info: PayeeInfo?
    @State private var names: [String] = []
    @State private var detail: PayeeDetail?
    @State private var usage: PayeeUsage?
    @State private var error: String?
    @State private var saving = false
    @State private var route: [PayeeRoute] = []
    @State private var editing: TransactionEditorRequest?

    private var trimmed: String { name.trimmingCharacters(in: .whitespaces) }
    private var merging: Bool { !trimmed.isEmpty && trimmed != original && names.contains(trimmed) }
    private var stored: JSON? { original.flatMap { name in store.budget.flatMap { planningPayeeRecord($0, name: name) } } }

    var body: some View {
        let currency = store.currency
        NavigationStack(path: $route) {
            Form {
                if let original {
                    Section {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(original).font(.title2.bold())
                            Text(usageLine(currency))
                                .font(.subheadline).foregroundStyle(Color.secondary).monospacedDigit()
                        }
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets(top: 4, leading: 4, bottom: 4, trailing: 4))
                        .accessibilityElement(children: .combine)
                    }
                }
                Section {
                    TextField(original == nil ? "Payee name" : "Name", text: $name).wordsCapitalized()
                } footer: {
                    if merging {
                        mergeNote.foregroundStyle(Color.urWarning)
                    } else if original != nil {
                        Text("Renaming or merging updates saved transactions, split payees, schedules and import rules.")
                    }
                }
                Section {
                    NavigationLink(value: PayeeRoute.category) {
                        PlanningPickerRow(icon: "tag", label: String(localized: "Default category"), value: planningCategoryName(store.budget, category),
                                          placeholder: placeholder)
                    }
                    if !category.isEmpty {
                        Button("Clear default category") { category = "" }
                    }
                } footer: {
                    Text("Filled in when you enter a transaction for this payee. Without one, the category you used last time is suggested.")
                }
                if let original { PayeeBankNames(payee: original) }
                if original != nil && names.count > 1 {
                    Section {
                        NavigationLink(value: PayeeRoute.merge) { Label("Merge into another payee", systemImage: "arrow.triangle.merge") }
                    }
                }
                if let detail, !detail.recent.isEmpty {
                    Section("Recent transactions") {
                        ForEach(detail.recent) { line in
                            Button { editing = TransactionEditorRequest(transaction: line.transaction, account: line.accountId) } label: { recentRow(line, currency) }
                                .accessibilityHint("Opens the transaction")
                        }
                    }
                }
                if let error { FormErrorView(message: error) }
            }
            .navigationTitle(original == nil ? "Add payee" : "Payee")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button(merging ? "Merge" : "Save", action: save).fontWeight(.semibold).disabled(trimmed.isEmpty)
                    }
                }
            }
            .navigationDestination(for: PayeeRoute.self) { destination($0) }
        }
        .interactiveDismissDisabled(saving)
        .retainingEditorDraft("payee", values: ["name": .string(name), "category": .string(category)])
        .onAppear(perform: load)
        .task(id: store.version) {
            guard let budget = store.budget else { return }
            let original = original
            let prepared = await Task.detached(priority: .userInitiated) { () -> ([PayeeInfo], PayeeDetail?, PayeeUsage?) in
                (payeeIndex(budget), original.map { PayeeDetail(budget: budget, name: $0) }, original.map { PayeeUsage(budget: budget, name: $0) })
            }.value
            guard !Task.isCancelled else { return }
            names = prepared.0.map(\.name)
            info = prepared.0.first { $0.name == original }
            detail = prepared.1
            usage = prepared.2
        }
        .cover(item: $editing) { TransactionEditor(request: $0) }
    }

    private var placeholder: String {
        guard let lastUsed = info?.lastUsed else { return String(localized: "No default") }
        let name = (lastUsed.isEmpty ? nil : planningCategoryName(store.budget, lastUsed)) ?? String(localized: "no category")
        return String(localized: "Last used: \(name)")
    }

    /// "12 transactions · total -480.00 SAR", or that there are none yet.
    private func usageLine(_ currency: String) -> String {
        guard let detail else { return " " }
        if detail.count == 0 { return String(localized: "No transactions yet") }
        return String(localized: "\(transactionCount(detail.count)) · total \(money(detail.total, currency))")
    }

    /// Typing another payee's name merges into it: says so, with what that payee has.
    private var mergeNote: Text {
        if let described = usage?.description {
            return Text("\(trimmed) already exists, so saving merges the two (\(described)).")
        }
        return Text("\(trimmed) already exists, so saving merges the two.")
    }

    private func load() {
        guard !loaded else { return }
        loaded = true
        name = original ?? ""
        category = stored?.text("categoryId") ?? ""
    }

    private func recentRow(_ line: PayeeRecent, _ currency: String) -> some View {
        let account = store.budget?.row("accounts", id: line.accountId)?.text("name")
        let category: String? = line.splits > 1 ? String(localized: "Split (\(line.splits))", locale: .app)
            : line.unassigned ? String(localized: "Unassigned")
            : line.categoryId.isEmpty ? String(localized: "No category") : planningCategoryName(store.budget, line.categoryId)
        return HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(Money.date(line.date)).foregroundStyle(Color.primary)
                Text([account, category].compactMap { $0 }.joined(separator: " · ")).font(.footnote).foregroundStyle(Color.secondary).lineLimit(1)
            }
            Spacer()
            Text(money(line.amount, currency)).monospacedDigit().foregroundStyle(line.amount > 0 ? Color.urPositive : .primary)
        }
    }

    @ViewBuilder private func destination(_ route: PayeeRoute) -> some View {
        switch route {
        case .category:
            CategoryPicker(title: String(localized: "Default category"), month: Dates.currentMonth, selected: category, includeUnassigned: true,
                           includePayment: false) { picked in
                category = picked
                self.route = []
            }
        case .merge:
            PayeeNameList(title: String(localized: "Merge into"), names: names.filter { $0 != original }, selected: merging ? trimmed : "") { picked in
                name = picked
                self.route = []
            }
        }
    }

    private func save() {
        guard !trimmed.isEmpty else { return }
        var record = stored ?? ["id": .string(UUID().uuidString.lowercased())]
        record["name"] = .string(trimmed)
        record["categoryId"] = JSON.text(orNull: category)
        let action: JSON = ["type": "savePayee", "from": JSON.text(orNull: original), "record": record]
        let guarded = stored.map { (collection: "payees", record: $0) }
        let (from, to, merged) = (original, trimmed, merging)
        let described = usage?.description
        saving = true
        error = nil
        Task {
            defer { saving = false }
            do {
                try await store.planningCommand("workflow", ["action": action], original: guarded)
                if let from, merged {
                    store.showToast(described.map { String(localized: "Merged \(from) into \(to) (\($0)).") } ?? String(localized: "Merged \(from) into \(to)."),
                                    undo: true)
                } else if let from, from != to {
                    store.showToast(String(localized: "Renamed \(from) to \(to)."), undo: true)
                } else {
                    store.showToast(from == nil ? String(localized: "Added \(to).") : String(localized: "Saved \(to)."), undo: true)
                }
                dismiss()
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }
}

/// Payee names to choose one from, with search.
/// The bank names a payee goes by, as the desktop's payee page lists them: its rules, text its bank
/// files used that the rules would name otherwise, and Apply for earlier transactions. Changes save
/// at once, with Undo.
private struct PayeeBankNames: View {
    @Environment(BudgetStore.self) private var store
    var payee: String
    @State private var text = ""
    @State private var error: String?
    @State private var pending = 0

    private struct Seen: Hashable { var raw: String; var match: String }

    var body: some View {
        let budget = store.budget ?? [:]
        let rules = budget.rows("payeeRules").filter { $0.text("payee") == payee }
        Section {
            ForEach(Array(rules.enumerated()), id: \.offset) { _, rule in
                HStack {
                    Text("“\(rule.text("match"))”")
                    Spacer()
                    if !rule.text("categoryId").isEmpty {
                        Text(planningCategoryName(store.budget, rule.text("categoryId")) ?? String(localized: "Deleted category"))
                            .font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
                .swipeActions { Button("Remove", role: .destructive) { remove(rule) } }
                .contextMenu { Button("Remove", systemImage: "trash", role: .destructive) { remove(rule) } }
            }
            HStack {
                TextField("Text the bank uses, e.g. AMZN MKTP", text: $text)
                    .autocorrectionDisabled()
                    .plainTextEntry()
                    .submitLabel(.done)
                    .onSubmit { add(text) }
                Button("Add") { add(text) }
                    .buttonStyle(.borderless)
                    .disabled(text.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            if let error { FormErrorView(message: error) }
            ForEach(seen(budget), id: \.self) { item in
                Button { add(item.match) } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Add “\(item.match)”")
                        Text(item.raw).font(.footnote).foregroundStyle(Color.secondary)
                    }
                }
                .accessibilityHint("Seen in a bank file")
            }
            if pending > 0 {
                Button("Apply to \(pending) earlier transactions") { apply(rules) }
            }
        } header: {
            Text("Bank names")
        } footer: {
            Text("Bank files name the same place in different ways. A bank payee that contains one of these, ignoring case and punctuation, is named \(payee).")
        }
        .task(id: store.version) {
            let ids = rules.map { $0.text("id") }
            guard !ids.isEmpty, let budget = store.budget else { pending = 0; return }
            pending = await Task.detached(priority: .userInitiated) { (try? PayeeRulePreview.changes(budget: budget, only: ids)) ?? 0 }.value
        }
    }

    /// Bank text on this payee's transactions that the rules would name something else, up to five.
    private func seen(_ budget: JSON) -> [Seen] {
        let rules = budget.rows("payeeRules")
        var found: [String: Seen] = [:]
        var order: [String] = []
        for t in budget.rows("transactions") where t.text("payee") == payee && order.count < 5 {
            let raw = t.text("importedPayee").trimmingCharacters(in: .whitespaces)
            let key = BankNames.normalize(raw)
            guard !raw.isEmpty, found[key] == nil, let rule = BankNames.ruleFor(rules, raw: raw, payee: payee) else { continue }
            found[key] = Seen(raw: raw, match: rule.text("match"))
            order.append(key)
        }
        return order.compactMap { found[$0] }
    }

    private func add(_ typed: String) {
        let match = typed.trimmingCharacters(in: .whitespaces)
        guard !match.isEmpty else { return }
        let rules = store.budget?.rows("payeeRules") ?? []
        if let taken = rules.first(where: { BankNames.normalize($0.text("match")) == BankNames.normalize(match) }) {
            error = takenMessage(match, owner: taken.text("payee"))
            return
        }
        let rule: JSON = ["id": .string(UUID().uuidString.lowercased()), "match": .string(match), "payee": .string(payee), "categoryId": .null]
        let name = payee
        store.edit(then: { store.showToast(String(localized: "Bank payees with “\(match)” are now named \(name)."), undo: true) }) { budget in
            var list = budget["payeeRules"].array
            list.append(rule)
            budget["payeeRules"] = .array(list)
        }
        text = ""
        error = nil
    }

    /// Why a bank name can't be added: it's already this payee's, or another rule has it.
    private func takenMessage(_ match: String, owner: String) -> String {
        if owner == payee { return String(localized: "“\(match)” is already one of \(payee)'s bank names.") }
        if owner.isEmpty { return String(localized: "“\(match)” is already a rule. Change it on the Import rules tab.") }
        return String(localized: "“\(match)” is already a rule for \(owner). Change it on the Import rules tab.")
    }

    private func remove(_ rule: JSON) {
        let match = rule.text("match")
        let id = rule.text("id")
        let name = payee
        store.edit(then: { store.showToast(String(localized: "Removed “\(match)” from \(name)."), undo: true) }) { budget in
            budget["payeeRules"] = .array(budget["payeeRules"].array.filter { $0.text("id") != id || $0.text("match") != match })
        }
    }

    private func apply(_ rules: [JSON]) {
        let count = pending
        store.workflow(["type": "reapplyPayeeRules", "ruleIds": JSON(rules.map { $0.text("id") })]) {
            store.showToast(String(localized: "Updated \(transactionCount(count))."), undo: true)
        }
    }
}

private struct PayeeNameList: View {
    var title: String
    var names: [String]
    var selected: String
    var onPick: (String) -> Void
    @State private var search = ""

    var body: some View {
        let query = search.trimmingCharacters(in: .whitespaces)
        let shown = query.isEmpty ? names : names.filter { $0.localizedCaseInsensitiveContains(query) }
        List {
            ForEach(shown, id: \.self) { name in
                Button { onPick(name) } label: {
                    HStack {
                        Text(name).foregroundStyle(Color.primary)
                        Spacer()
                        if name == selected { Image(systemName: "checkmark").foregroundStyle(Color.urAccent).fontWeight(.semibold) }
                    }
                }
            }
            if shown.isEmpty && !query.isEmpty { ContentUnavailableView.search(text: query) }
        }
        .groupedList()
        .searchable(text: $search, prompt: "Search payees")
        .navigationTitle(title)
        .inlineTitle()
    }
}

// MARK: Import rule

private enum PayeeRuleRoute: Hashable { case category, payee }

/// What a rule's preview was worked out from.
private struct PayeeRuleKey: Hashable {
    var match: String
    var payee: String
    var category: String
    var version: Int
}

/// An import rule: the text to look for in the bank's payee, the name to rename it to and the
/// category to fill in, with how many transactions you already have it would change, worked out
/// by the engine (PayeeScreen.kt's RuleDialog).
private struct PayeeRuleEditor: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var original: JSON?
    var preset: String

    init(rule: JSON?, payee: String = "") {
        original = rule
        preset = payee
    }

    @State private var loaded = false
    @State private var id = UUID().uuidString.lowercased()
    @State private var match = ""
    @State private var payee = ""
    @State private var category = ""
    /// The preview for the rule as typed; a nil count when the engine couldn't work it out.
    @State private var changes: (key: PayeeRuleKey, count: Int?)?
    @State private var names: [String] = []
    @State private var error: String?
    @State private var saving = false
    @State private var confirmDelete = false
    @State private var route: [PayeeRuleRoute] = []

    private var ready: Bool { !match.trimmingCharacters(in: .whitespaces).isEmpty && (!payee.trimmingCharacters(in: .whitespaces).isEmpty || !category.isEmpty) }
    private var key: PayeeRuleKey { PayeeRuleKey(match: match, payee: payee, category: category, version: store.version) }
    private var previewed: Bool { changes?.key == key }
    private var count: Int? { previewed ? changes?.count : nil }
    private var rules: [JSON] { store.budget?.rows("payeeRules") ?? [] }
    private var index: Int? { rules.firstIndex { $0.text("id") == id } }

    var body: some View {
        NavigationStack(path: $route) {
            Form {
                Section {
                    TextField("Bank payee contains", text: $match)
                        .autocorrectionDisabled()
                    HStack {
                        TextField("Rename to", text: $payee).wordsCapitalized()
                        if !names.isEmpty {
                            Button { route.append(.payee) } label: { Image(systemName: "list.bullet") }
                                .buttonStyle(.borderless)
                                .accessibilityLabel("Choose a payee")
                        }
                    }
                    NavigationLink(value: PayeeRuleRoute.category) {
                        PlanningPickerRow(icon: "tag", label: String(localized: "Category"), value: planningCategoryName(store.budget, category),
                                          placeholder: String(localized: "Keep category"))
                    }
                    if !category.isEmpty {
                        Button("Keep the category instead") { category = "" }
                    }
                } footer: {
                    status
                }
                if ready, let count, count > 0 {
                    Section {
                        Button(action: { save(apply: true) }) {
                            Label("Save & apply to \(count) transactions", systemImage: "wand.and.stars")
                        }
                        .disabled(saving)
                    } footer: {
                        Text("Renames and categorizes the transactions you already have that this rule matches, as one change you can undo.")
                    }
                }
                if original != nil, let index {
                    Section {
                        Button { reorder("up") } label: { Label("Move earlier", systemImage: "arrow.up") }.disabled(index == 0 || saving)
                        Button { reorder("down") } label: { Label("Move later", systemImage: "arrow.down") }.disabled(index == rules.count - 1 || saving)
                    } footer: {
                        Text("The longest pattern wins; list order breaks ties.")
                    }
                    Section {
                        Button("Delete rule", role: .destructive) { confirmDelete = true }.frame(maxWidth: .infinity)
                    }
                }
                if let error { FormErrorView(message: error) }
            }
            .navigationTitle(original == nil ? "Add import rule" : "Import rule")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button("Save") { save(apply: false) }.fontWeight(.semibold).disabled(!ready)
                    }
                }
            }
            .navigationDestination(for: PayeeRuleRoute.self) { destination($0) }
            .confirmationDialog("Delete this rule?", isPresented: $confirmDelete, titleVisibility: .visible) {
                Button("Delete", role: .destructive, action: delete)
            } message: {
                Text("It won't rename or categorize imported transactions any more. Transactions it already changed stay as they are.")
            }
        }
        .presentationDetents([.medium, .large])
        .interactiveDismissDisabled(saving)
        .retainingEditorDraft("payeeRule", values: ["match": .string(match), "payee": .string(payee), "category": .string(category)])
        .onAppear(perform: load)
        .task(id: key) { await preview() }
        .task(id: store.version) {
            guard let budget = store.budget else { return }
            let prepared = await Task.detached(priority: .utility) { payeeIndex(budget).map(\.name) }.value
            if !Task.isCancelled { names = prepared }
        }
    }

    @ViewBuilder private var status: some View {
        VStack(alignment: .leading, spacing: 6) {
            if match.trimmingCharacters(in: .whitespaces).isEmpty {
                Text("Enter the text to look for in the bank's payee, e.g. CARREFOUR.")
            } else if !ready {
                Text("Choose a new name, a category, or both.").foregroundStyle(Color.urWarning)
            } else if previewed {
                if let count, count > 0 {
                    Text("Applied to existing transactions, it would rename or categorize \(count) transactions.")
                } else if count != nil {
                    Text("It wouldn't change any transactions you already have.")
                }
            } else {
                HStack(spacing: 6) {
                    ProgressView().controlSize(.mini)
                    Text("Checking your transactions…")
                }
            }
            Text("A rule matches when the bank's payee contains its text from the start of a word, ignoring case and punctuation; the longest match wins. It renames the payee and fills in the category if there's none.")
        }
    }

    private func load() {
        guard !loaded else { return }
        loaded = true
        guard let original else {
            payee = preset
            return
        }
        if !original.text("id").isEmpty { id = original.text("id") }
        match = original.text("match")
        payee = original.text("payee")
        category = original.text("categoryId")
    }

    /// The rule as typed, keeping any fields this editor doesn't show.
    private var record: JSON {
        var rule = original ?? [:]
        rule["id"] = .string(id)
        rule["match"] = .string(match.trimmingCharacters(in: .whitespaces))
        rule["payee"] = .string(payee.trimmingCharacters(in: .whitespaces))
        rule["categoryId"] = JSON.text(orNull: category)
        return rule
    }

    private func preview() async {
        let key = key
        guard ready, let budget = store.budget else { return }
        try? await Task.sleep(for: .milliseconds(300))
        if Task.isCancelled { return }
        let draft = record
        let id = id
        var list = budget.rows("payeeRules")
        if let at = list.firstIndex(where: { $0.text("id") == id }) { list[at] = draft } else { list.append(draft) }
        let rules = list
        let found = await Task.detached(priority: .userInitiated) { try? PayeeRulePreview.changes(budget: budget, rules: rules, only: [id]) }.value
        if !Task.isCancelled { changes = (key, found) }
    }

    @ViewBuilder private func destination(_ route: PayeeRuleRoute) -> some View {
        switch route {
        case .category:
            CategoryPicker(title: String(localized: "Category"), month: Dates.currentMonth, selected: category, includePayment: false) { picked in
                category = picked
                self.route = []
            }
        case .payee:
            PayeeNameList(title: String(localized: "Rename to"), names: names, selected: payee.trimmingCharacters(in: .whitespaces)) { picked in
                payee = picked
                self.route = []
            }
        }
    }

    private func save(apply: Bool) {
        guard ready else { return }
        let rule = record
        let guarded = original.map { (collection: "payeeRules", record: $0) }
        let id = id
        let count = count
        let commands: [(String, JSON)] = apply ? [("workflow", ["action": ["type": "reapplyPayeeRules", "ruleIds": JSON([id])]])] : []
        saving = true
        error = nil
        Task {
            defer { saving = false }
            do {
                try await store.planningEdit(original: guarded, then: commands) { try $0.upsertRecord("payeeRules", rule) }
                if apply {
                    store.showToast(String(localized: "Saved the rule and updated \(transactionCount(count ?? 0))."), undo: true)
                } else {
                    store.showToast(guarded == nil ? String(localized: "Added the rule.") : String(localized: "Saved the rule."), undo: true)
                }
                dismiss()
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }

    private func reorder(_ direction: String) {
        let id = id
        saving = true
        error = nil
        Task {
            defer { saving = false }
            do {
                try await store.planningCommand("workflow", ["action": ["type": "reorderPayeeRule", "id": .string(id), "direction": .string(direction)]])
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }

    private func delete() {
        guard let original else { return }
        let id = id
        saving = true
        Task {
            defer { saving = false }
            do {
                try await store.planningEdit(original: (collection: "payeeRules", record: original)) { try $0.removeRecord("payeeRules", id: id) }
                store.showToast(String(localized: "Deleted the rule."), undo: true)
                dismiss()
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }
}
