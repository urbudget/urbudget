import SwiftUI

/// The pages a category's sheet leads to; a long press on a category can open one directly.
enum CategoryRoute: Hashable { case target, details, delete, history }

/// The engine request for a Quick assign option over the given (default: every visible) categories.
func quickAssignParams(_ budget: JSON, month: String, mode: String, categories: [String]? = nil, limit: Bool = true) -> JSON {
    let reference = Dates.currentMonth
    let ids = categories ?? budget.rows("categories").filter { !$0.flag("hidden") }.map { $0.text("id") }
    return ["action": ["type": "autoAssign", "month": .string(month), "mode": .string(mode), "categories": JSON(ids),
                       "limitToAvailable": .bool(limit), "expectedAsOfMonth": .string(reference)],
            "asOfMonth": .string(reference)]
}

/// A category's details, as a sheet: what's available and why, Assigned edited on the keypad (with
/// YNAB's per-category shortcuts), this month's moves, the note, and its other editors a step in.
struct CategorySheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    /// The category shown; the arrows step to the one before or after it on Budget.
    @State private var categoryId: String
    var startAssigning: Bool
    /// Moves money for a category, "to" or "from" it.
    var move: (_ category: String, _ direction: String) -> Void
    var transactions: (RegisterTarget) -> Void
    var startRoute: CategoryRoute?

    init(categoryId: String, startAssigning: Bool, move: @escaping (_ category: String, _ direction: String) -> Void,
         transactions: @escaping (RegisterTarget) -> Void, startRoute: CategoryRoute? = nil) {
        _categoryId = State(initialValue: categoryId)
        self.startAssigning = startAssigning
        self.move = move
        self.transactions = transactions
        self.startRoute = startRoute
    }

    @State private var asking: AssistantRequest?
    @State private var editingAssigned = false
    @State private var pad = PadState(nil)
    @State private var suggestions: [String: Int64] = [:]
    @State private var error: String?
    @State private var route: [CategoryRoute] = []
    @State private var noting = false
    @State private var started = false
    @State private var paying: String?
    @State private var loanFor: String?

    private var category: JSON? { store.budget?.row("categories", id: categoryId) }
    private var total: JSON? { store.summary?.rows("categories").first { $0.text("id") == categoryId } }

    var body: some View {
        NavigationStack(path: $route) {
            Group {
                if let category { details(category) } else { ContentUnavailableView("This category was deleted", systemImage: "trash") }
            }
            .navigationDestination(for: CategoryRoute.self) { route in
                switch route {
                case .target: TargetEditor(categoryId: categoryId) { dismiss() }
                case .details: CategoryDetailsEditor(categoryId: categoryId) { dismiss() }
                case .delete: DeleteCategoryView(categoryId: categoryId) { dismiss() }
                case .history: MovesView(categoryId: categoryId)
                }
            }
        }
        .presentationDetents(editingAssigned ? [.large] : [.medium, .large])
        .presentationDragIndicator(.visible)
        .onAppear {
            guard !started else { return }
            started = true
            if startAssigning { beginAssigning() }
            if let startRoute { route = [startRoute] }
        }
        .task(id: editingAssigned) { await loadSuggestions() }
        .sheet(isPresented: $noting) { NoteEditor(categoryId: categoryId) }
        .sheet(item: Binding(get: { paying.map { IdentifiedText(value: $0) } }, set: { paying = $0?.value })) { CardPaymentSheet(accountId: $0.value) }
        .sheet(item: Binding(get: { loanFor.map { IdentifiedText(value: $0) } }, set: { loanFor = $0?.value })) { LoanEditor(accountId: $0.value) }
        .sheet(item: $asking) { AssistantView(request: $0) }
        .sensoryFeedback(.selection, trigger: categoryId)
    }

    /// The categories before and after this one, in Budget's order.
    private var neighbours: (previous: String?, next: String?) {
        guard let budget = store.budget else { return (nil, nil) }
        let ids = orderedGroups(budget) { !$0.flag("hidden") || $0.text("id") == categoryId }.flatMap(\.categories).map { $0.text("id") }
        guard let index = ids.firstIndex(of: categoryId) else { return (nil, nil) }
        return (index > 0 ? ids[index - 1] : nil, index + 1 < ids.count ? ids[index + 1] : nil)
    }

    private func show(_ id: String) {
        editingAssigned = false
        error = nil
        suggestions = [:]
        withAnimation(.snappy) { categoryId = id }
    }

    /// The loans this category pays, with each loan's account, as the desktop's inspector lists them.
    private var loans: [(terms: JSON, account: JSON)] {
        guard let budget = store.budget else { return [] }
        return budget.rows("loans").filter { $0.text("categoryId") == categoryId }.compactMap { terms in
            budget.row("accounts", id: terms.text("accountId")).map { (terms, $0) }
        }
    }

    @ViewBuilder private func loanSection(_ terms: JSON, _ account: JSON, _ category: JSON, _ currency: String) -> some View {
        let id = account.text("id")
        let balance = store.summary?.rows("accounts").first { $0.text("id") == id }?.long("balance") ?? account.long("openingBalance")
        let owed = max(0, -balance)
        let payment = terms.long("monthlyPayment")
        let targeted = category.long("target") == payment && category["goal"].text("goal_type") == "DEBT"
        Section {
            ValueRow(title: String(localized: "Still owed"), value: money(owed, currency))
            ValueRow(title: String(localized: "Monthly payment"), value: money(payment, currency))
            Button { loanFor = id } label: { Label("Loan details & payoff", systemImage: "percent") }
            if !targeted && payment > 0 {
                Button {
                    let name = category.text("name")
                    let categoryId = categoryId
                    let toast = String(localized: "Set \(name)'s target to the \(money(payment, currency)) \(account.text("name")) payment.")
                    // A monthly debt payment target, as the target editor saves one.
                    store.edit(then: { store.showToast(toast, undo: true) }) { budget in
                        var rows = budget["categories"].array
                        guard let index = rows.firstIndex(where: { $0.text("id") == categoryId }) else { throw AppError(String(localized: "Category not found.")) }
                        var goal = rows[index]["goal"].text("goal_type") == "DEBT" ? rows[index]["goal"] : ["goal_creation_month": .string("\(Dates.currentMonth)-01")]
                        goal["goal_type"] = "DEBT"
                        goal["goal_needs_whole_amount"] = .null
                        goal["goal_target"] = .int(payment * 10)
                        rows[index]["goal"] = goal
                        rows[index]["target"] = .int(payment)
                        budget["categories"] = .array(rows)
                    }
                } label: { Label("Use this payment as the target", systemImage: "target") }
            }
        } header: {
            Text("Pays \(account.text("name"))")
        }
    }

    private func beginAssigning() {
        pad.set(total?.long("assigned") ?? 0)
        editingAssigned = true
    }

    /// What the per-category Quick assign options would set Assigned to, while the keypad is open.
    private func loadSuggestions() async {
        guard editingAssigned, let budget = store.budget else { return }
        let month = store.month
        for mode in ["assignedLastMonth", "spentLastMonth", "averageAssigned", "averageSpent"] {
            var params = quickAssignParams(budget, month: month, mode: mode, categories: [categoryId], limit: false)
            params["previewMonth"] = .string(month)
            guard let result = try? await store.query("workflow", params), !Task.isCancelled else { continue }
            if let row = result["summary"].rows("categories").first(where: { $0.text("id") == categoryId }) {
                suggestions[mode] = row.long("assigned")
            }
        }
    }

    private func details(_ category: JSON) -> some View {
        let currency = store.currency
        let month = store.month
        let state = CategoryStatus(category: category, summary: total, month: month)
        let words = state.targetWords(category: category, summary: total, monthFirst: store.monthFirst)
        let assigned = total?.long("assigned") ?? 0
        let available = total?.long("available") ?? 0
        let payment = !category.text("paymentAccountId").isEmpty
        let target = category.long("target")
        let moves = store.budget.map { categoryMoves($0, category: categoryId, month: month, assigned: assigned) } ?? []
        let colors = StatusColors(state.status)
        let (from, through) = Dates.monthBounds(month)
        return List {
            Section {
                VStack(spacing: 4) {
                    Text(money(available, currency))
                        .font(.system(size: 38, weight: .heavy, design: .rounded)).monospacedDigit()
                        .foregroundStyle(state.status == "zero" ? .primary : colors.ink)
                        .lineLimit(1).minimumScaleFactor(0.5)
                    Text(state.status == "negative" ? "Overspent" : state.status == "credit" ? "Overspent on a card" : payment ? "Set aside for the card" : "Available")
                        .foregroundStyle(Color.secondary)
                    if let words {
                        Text(words.amount.map { "\(money($0, currency)) \(words.words)" } ?? words.words)
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(state.status == "underfunded" ? Color.urWarning : state.eventually > 0 ? .secondary : Color.urPositive)
                            .padding(.top, 4)
                    }
                    if let progress = state.progress, !state.snoozed {
                        TargetBar(progress: progress, status: state.status).frame(maxWidth: 220).padding(.top, 8)
                    }
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }
            Section {
                ValueRow(title: String(localized: "Carried from last month"), value: money(total?.long("carried") ?? 0, currency))
                Button {
                    if !editingAssigned { beginAssigning() }
                } label: {
                    HStack {
                        Text("Assigned in \(Dates.shortMonthLabel(month))").foregroundStyle(Color.primary)
                        Spacer()
                        if editingAssigned {
                            Text(pad.isSum ? AppLanguage.leftToRight(pad.display) : money(pad.cents ?? 0, currency))
                                .font(.title3.weight(.heavy)).monospacedDigit().foregroundStyle(Color.urAccent)
                        } else {
                            Text(money(assigned, currency)).monospacedDigit().foregroundStyle(Color.urAccent)
                            Image(systemName: "pencil").foregroundStyle(Color.urAccent)
                        }
                    }
                }
                .listRowBackground(editingAssigned ? Color.urAccent.opacity(0.12) : nil)
                // A card payment's activity is the card's own register for the month.
                Button {
                    transactions(payment ? RegisterTarget(account: category.text("paymentAccountId"), from: from, through: through)
                                         : RegisterTarget(category: categoryId, from: from, through: through))
                } label: {
                    HStack {
                        ValueRow(title: String(localized: "Activity"), value: money(total?.long("activity") ?? 0, currency))
                        Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.urTertiary)
                    }
                    .foregroundStyle(Color.primary)
                }
                if let upcoming = total?.long("upcoming"), upcoming > 0 {
                    ValueRow(title: String(localized: "Upcoming"), value: money(upcoming, currency), subtitle: String(localized: "Scheduled this month"))
                }
                if target > 0 {
                    NavigationLink(value: CategoryRoute.target) {
                        ValueRow(title: String(localized: "Target"), value: money(target, currency), subtitle: targetDescription(category))
                    }
                }
            }
            if let error { FormErrorView(message: error) }
            if editingAssigned {
                Section {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            if state.needed > 0 && !payment {
                                chip(String(localized: "Needed \(plus(state.needed, currency))")) { pad.set(assigned + state.needed) }
                            }
                            if available < 0 { chip(String(localized: "Cover \(plus(-available, currency))")) { pad.set(assigned - available) } }
                            ForEach(["assignedLastMonth", "spentLastMonth", "averageAssigned", "averageSpent"], id: \.self) { mode in
                                if let value = suggestions[mode], value != assigned {
                                    chip(suggestion(mode, money(value, currency))) { pad.set(value) }
                                }
                            }
                            Button("Cancel") { editingAssigned = false; error = nil }.buttonStyle(.borderless).foregroundStyle(Color.secondary)
                        }
                    }
                    .listRowInsets(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
                }
            } else {
                Section {
                    if payment {
                        Button { paying = category.text("paymentAccountId") } label: { Label("Make a payment", systemImage: "creditcard") }
                            .fontWeight(.semibold)
                    }
                    Button { move(categoryId, available < 0 ? "from" : "to") } label: {
                        Label(available < 0 ? "Cover overspending" : "Move money", systemImage: "arrow.left.arrow.right")
                    }
                    NavigationLink(value: CategoryRoute.target) {
                        Label(target > 0 || category["goal"].isObject ? "Edit target" : "Create target", systemImage: "target")
                    }
                }
                ForEach(loans, id: \.account.idText) { loan in loanSection(loan.terms, loan.account, category, currency) }
                Section("Note") {
                    Button { noting = true } label: {
                        Text(category.text("note").isEmpty ? String(localized: "Add a note") : category.text("note"))
                            .foregroundStyle(category.text("note").isEmpty ? Color.urAccent : .primary)
                            .lineLimit(4)
                    }
                }
                if !moves.isEmpty {
                    Section("Moves this month") {
                        ForEach(moves.suffix(5)) { line in moveRow(line, currency) }
                        if moves.count > 5 { NavigationLink("Show all \(moves.count)", value: CategoryRoute.history) }
                    }
                }
            }
        }
        .groupedList()
        .navigationTitle(category.text("name"))
        .inlineTitle()
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } }
            ToolbarItemGroup(placement: .primaryAction) {
                let neighbours = neighbours
                Button { neighbours.previous.map(show) } label: { Image(systemName: "chevron.up") }
                    .disabled(neighbours.previous == nil)
                    .keyboardShortcut(.upArrow, modifiers: .command)
                    .accessibilityLabel("Previous category")
                Button { neighbours.next.map(show) } label: { Image(systemName: "chevron.down") }
                    .disabled(neighbours.next == nil)
                    .keyboardShortcut(.downArrow, modifiers: .command)
                    .accessibilityLabel("Next category")
                menu(category, payment: payment)
            }
        }
        .safeAreaInset(edge: .bottom) {
            if editingAssigned {
                AmountKeypad(pad: pad, doneLabel: String(localized: "Assign")) {
                    guard let cents = pad.cents else { error = String(localized: "Finish the sum before saving."); return }
                    // The sheet stays open on the new figures, as the desktop's inspector does.
                    store.assign(category: categoryId, amount: cents) { editingAssigned = false; error = nil }
                }
                .transition(.move(edge: .bottom))
            }
        }
        .animation(.snappy(duration: 0.25), value: editingAssigned)
    }

    /// "+45.00", kept whole in right-to-left text.
    private func plus(_ cents: Int64, _ currency: String) -> String {
        AppLanguage.leftToRight("+" + money(cents, currency))
    }

    /// A keypad shortcut's label: what a Quick assign option would set Assigned to.
    private func suggestion(_ mode: String, _ amount: String) -> String {
        switch mode {
        case "assignedLastMonth": return String(localized: "Last month \(amount)")
        case "spentLastMonth": return String(localized: "Spent last month \(amount)")
        case "averageAssigned": return String(localized: "Avg assigned \(amount)")
        default: return String(localized: "Avg spent \(amount)")
        }
    }

    private func chip(_ text: String, _ action: @escaping () -> Void) -> some View {
        Button(text, action: action)
            .font(.subheadline.weight(.semibold))
            .buttonStyle(.bordered)
            .buttonBorderShape(.capsule)
            .tint(.urAccent)
    }

    private func menu(_ category: JSON, payment: Bool) -> some View {
        Menu {
            if !payment { Button { route.append(.details) } label: { Label("Name, group & note", systemImage: "pencil") } }
            Button { route.append(.history) } label: { Label("Money moves this month", systemImage: "clock.arrow.circlepath") }
            Button { asking = AssistantRequest(text: String(localized: "Help me with \(category.text("name")) this month.")) } label: {
                Label("Ask the assistant", systemImage: "sparkles")
            }
            let hidden = category.flag("hidden")
            Button {
                let name = category.text("name")
                store.upsert("categories", ["id": .string(categoryId), "hidden": .bool(!hidden)]) {
                    store.showToast(hidden ? String(localized: "\(name) is shown again.")
                                           : String(localized: "Hid \(name). Find it under Hidden categories."), undo: true)
                }
                dismiss()
            } label: { Label(hidden ? "Show category" : "Hide category", systemImage: hidden ? "eye" : "eye.slash") }
            if !payment {
                Divider()
                Button(role: .destructive) { route.append(.delete) } label: { Label("Delete category", systemImage: "trash") }
            }
        } label: {
            Image(systemName: "ellipsis.circle")
        }
        .accessibilityLabel("Category actions")
    }
}

func moveRow(_ line: MoveLine, _ currency: String) -> some View {
    ValueRow(title: line.what, value: AppLanguage.leftToRight((line.amount > 0 ? "+" : "") + money(line.amount, currency)),
             subtitle: line.detail ?? Dates.dayAndMonth(line.at), color: line.amount < 0 ? .urNegative : .primary)
}

/// A target's kind and how often it repeats, as the desktop's inspector names them.
func targetDescription(_ category: JSON) -> String {
    let goal = category["goal"]
    guard goal.isObject else { return String(localized: "Set aside") }
    let date = Money.date(goal.text("goal_target_date"))
    switch goal.text("goal_type") {
    case "NEED": break
    case "TB": return String(localized: "Balance target")
    case "TBD": return String(localized: "Balance target by \(date)")
    case "DEBT": return String(localized: "Loan payment target")
    case "MF": return String(localized: "Monthly funding")
    default: return String(localized: "Set aside")
    }
    let every = Int(goal.long("goal_cadence_frequency") == 0 ? 1 : goal.long("goal_cadence_frequency"))
    let cadence = goal.has("goal_cadence") ? Int(goal.long("goal_cadence")) : 1
    return goal.flag("goal_needs_whole_amount", true) ? setAsideDescription(cadence, every: every, date: date)
        : refillDescription(cadence, every: every, date: date)
}

/// "Set aside each month", "Set aside every 3 weeks", "Set aside once, by 2026-12-01"…
private func setAsideDescription(_ cadence: Int, every: Int, date: String) -> String {
    switch cadence {
    case 0: return String(localized: "Set aside once, by \(date)")
    case 1: return every > 1 ? String(localized: "Set aside every \(every) months", locale: .app) : String(localized: "Set aside each month")
    case 2: return every > 1 ? String(localized: "Set aside every \(every) weeks", locale: .app) : String(localized: "Set aside each week")
    case 13: return every > 1 ? String(localized: "Set aside every \(every) years", locale: .app) : String(localized: "Set aside each year")
    case 14: return String(localized: "Set aside every two years")
    default: return String(localized: "Set aside every \(cadence - 1) months", locale: .app)
    }
}

/// "Refill up to each month", "Refill up to every 3 weeks"…
private func refillDescription(_ cadence: Int, every: Int, date: String) -> String {
    switch cadence {
    case 0: return String(localized: "Refill up to once, by \(date)")
    case 1: return every > 1 ? String(localized: "Refill up to every \(every) months", locale: .app) : String(localized: "Refill up to each month")
    case 2: return every > 1 ? String(localized: "Refill up to every \(every) weeks", locale: .app) : String(localized: "Refill up to each week")
    case 13: return every > 1 ? String(localized: "Refill up to every \(every) years", locale: .app) : String(localized: "Refill up to each year")
    case 14: return String(localized: "Refill up to every two years")
    default: return String(localized: "Refill up to every \(cadence - 1) months", locale: .app)
    }
}

/// All of a category's moves this month.
private struct MovesView: View {
    @Environment(BudgetStore.self) private var store
    var categoryId: String

    var body: some View {
        let assigned = store.summary?.rows("categories").first { $0.text("id") == categoryId }?.long("assigned") ?? 0
        let moves = store.budget.map { categoryMoves($0, category: categoryId, month: store.month, assigned: assigned) } ?? []
        List {
            if moves.isEmpty {
                Text("No money moved for this category this month.").foregroundStyle(Color.secondary)
            } else {
                Section {
                    ForEach(moves) { moveRow($0, store.currency) }
                } footer: {
                    Text("Moves add up to what's assigned in \(Dates.monthLabel(store.month)).")
                }
            }
        }
        .groupedList()
        .navigationTitle("Money moves")
        .inlineTitle()
    }
}

private struct NoteEditor: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var categoryId: String
    @State private var note = ""
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                TextField("Note", text: $note, axis: .vertical).lineLimit(5...12)
                if let error { FormErrorView(message: error) }
            }
            .disabled(saving)
            .navigationTitle(store.budget?.row("categories", id: categoryId)?.text("name") ?? String(localized: "Note"))
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    SaveButton(saving: saving) {
                        attempt($error, saving: $saving) {
                            try await store.upserting("categories", ["id": .string(categoryId), "note": .string(note)])
                            dismiss()
                        }
                    }
                }
            }
            .onAppear { note = store.budget?.row("categories", id: categoryId)?.text("note") ?? "" }
        }
        .presentationDetents([.medium])
    }
}

/// The target kinds the desktop offers, in its words. Imported kinds stay available only to targets that use them.
private let targetBehaviours: [(id: String, label: String, hint: String)] = { () -> [(id: String, label: String, hint: String)] in
    var behaviours: [(id: String, label: String, hint: String)] = []
    behaviours.append(("set", String(localized: "Set aside each period"), String(localized: "Assign the whole amount every period, whatever is left over.")))
    behaviours.append(("refill", String(localized: "Refill up to each period"), String(localized: "Top up to the amount; money carried forward counts.")))
    behaviours.append(("TB", String(localized: "Maintain a category balance"), String(localized: "Keep at least this much available.")))
    behaviours.append(("TBD", String(localized: "Reach a balance by a date"), String(localized: "Save up to the amount by a chosen date.")))
    return behaviours
}()

/// The days a weekly target can fall on, Sunday first, as the budget numbers them.
private let weekdayNames: [LocalizedStringKey] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]

/// Creating or changing a target, as the desktop's editor does: the four behaviours, the amount on
/// the keypad, how often it repeats and from when, and snoozing for this month.
private struct TargetEditor: View {
    @Environment(BudgetStore.self) private var store
    var categoryId: String
    var close: () -> Void

    @State private var loaded = false
    @State private var behaviour = "set"
    @State private var pad = PadState(nil)
    @State private var keypad = false
    @State private var due = ""
    @State private var cadence = 1
    @State private var interval = 1
    @State private var weekday = 0
    @State private var start = Dates.currentMonth
    @State private var snoozed = false
    @State private var error: String?
    @State private var saving = false

    private var category: JSON? { store.budget?.row("categories", id: categoryId) }

    var body: some View {
        let category = category ?? [:]
        let existing = category["goal"]
        let existingType = existing.text("goal_type")
        let need = behaviour == "set" || behaviour == "refill"
        let payment = !category.text("paymentAccountId").isEmpty
        let hasTarget = existing.isObject || category.long("target") > 0
        var options = targetBehaviours
        if existingType == "MF" {
            options.append(("MF", String(localized: "Monthly funding"), String(localized: "An imported target: fund this amount each month.")))
        }
        if existingType == "DEBT" {
            options.append(("DEBT", String(localized: "Monthly debt payment"), String(localized: "An imported target for a loan payment.")))
        }
        return Form {
            Section {
                Button {
                    keypad = true
                    pad.fresh = true
                } label: {
                    VStack(spacing: 4) {
                        PadAmount(pad: pad, currency: store.currency, color: keypad ? .urAccent : .primary)
                        Text(behaviour == "TB" || behaviour == "TBD" ? "Balance to reach" : "Amount each period").font(.footnote).foregroundStyle(Color.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 6)
                }
                .listRowBackground(keypad ? Color.urAccent.opacity(0.12) : nil)
            }
            Section("Target") {
                Picker("Target", selection: $behaviour) {
                    ForEach(options, id: \.id) { option in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(option.label)
                            Text(option.hint).font(.footnote).foregroundStyle(Color.secondary)
                        }
                        .tag(option.id)
                    }
                }
                .pickerStyle(.inline)
                .labelsHidden()
            }
            Section {
                if need {
                    Picker("Repeats", selection: $cadence) {
                        Text("Once by a date").tag(0)
                        Text("Monthly").tag(1)
                        Text("Weekly").tag(2)
                        Text("Yearly").tag(13)
                        ForEach(3...12, id: \.self) { Text("Every \($0 - 1) months").tag($0) }
                        Text("Every two years").tag(14)
                    }
                    if [1, 2, 13].contains(cadence) {
                        Stepper(value: $interval, in: 1...100) { intervalLabel }
                    }
                    if cadence == 2 {
                        Picker("Day of week", selection: $weekday) {
                            ForEach(Array(weekdayNames.enumerated()), id: \.offset) { Text($0.element).tag($0.offset) }
                        }
                    } else if cadence != 1 || interval != 1 {
                        DayPicker(title: cadence == 0 ? String(localized: "Needed by") : String(localized: "First due date"), iso: $due)
                    }
                }
                if behaviour == "TBD" { DayPicker(title: String(localized: "Reach it by"), iso: $due) }
                if behaviour != "TB" { MonthRow(title: String(localized: "Starts"), month: $start) }
                if store.month == Dates.currentMonth && !payment && hasTarget {
                    Toggle(isOn: $snoozed) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Snooze this month")
                            Text("It asks for nothing until next month").font(.footnote).foregroundStyle(Color.secondary)
                        }
                    }
                }
            }
            if let error { FormErrorView(message: error) }
            if hasTarget {
                Section {
                    Button("Remove target", role: .destructive) {
                        attempt($error, saving: $saving) {
                            try await store.saveTarget(category: categoryId, goal: nil, amount: 0)
                            store.showToast(String(localized: "Removed the \(category.text("name")) target."), undo: true)
                            close()
                        }
                    }
                }
            }
        }
        .disabled(saving)
        .navigationTitle(hasTarget ? "Edit target" : "Create target")
        .inlineTitle()
        .toolbar { ToolbarItem(placement: .confirmationAction) { SaveButton(saving: saving, action: save) } }
        .safeAreaInset(edge: .bottom) {
            if keypad { AmountKeypad(pad: pad) { keypad = false }.transition(.move(edge: .bottom)) }
        }
        .animation(.snappy(duration: 0.25), value: keypad)
        .onAppear(perform: load)
    }

    /// "Every 2 weeks": the stepper's label, in the unit the target repeats in.
    private var intervalLabel: Text {
        switch cadence {
        case 2: return Text("Every \(interval) weeks")
        case 13: return Text("Every \(interval) years")
        default: return Text("Every \(interval) months")
        }
    }

    private func load() {
        guard !loaded, let category else { return }
        loaded = true
        let existing = category["goal"]
        let type = existing.text("goal_type")
        if !existing.isObject { behaviour = "set" }
        else if type == "NEED" { behaviour = existing.flag("goal_needs_whole_amount", true) ? "set" : "refill" }
        else { behaviour = type.isEmpty ? "set" : type }
        let target = category.long("target")
        pad = PadState(target > 0 ? target : nil)
        keypad = target <= 0
        due = existing.text("goal_target_date").isEmpty ? "\(store.month)-28" : existing.text("goal_target_date")
        cadence = existing.has("goal_cadence") ? Int(existing.long("goal_cadence")) : 1
        interval = max(1, Int(existing.long("goal_cadence_frequency")))
        weekday = min(6, max(0, Int(existing.long("goal_day"))))
        let created = String(existing.text("goal_creation_month").prefix(7))
        start = Dates.isMonth(created) ? created : store.month
        snoozed = existing.text("goal_snoozed_at").hasPrefix(store.month)
    }

    private func save() {
        attempt($error, saving: $saving) {
            guard let category else { throw AppError(String(localized: "This category was deleted.")) }
            guard let amount = pad.cents else { throw AppError(String(localized: "Finish the sum first.")) }
            guard amount > 0 else { throw AppError(String(localized: "Enter a target amount, or remove the target.")) }
            let existing = category["goal"]
            let need = behaviour == "set" || behaviour == "refill"
            let type = need ? "NEED" : behaviour
            let payment = !category.text("paymentAccountId").isEmpty
            var goal: JSON = existing.text("goal_type") == type ? existing : ["goal_creation_month": .string("\(Dates.currentMonth)-01")]
            goal["goal_type"] = .string(type)
            let (scaled, overflow) = amount.multipliedReportingOverflow(by: 10)
            guard !overflow else { throw AppError(String(localized: "The amount is too large.")) }
            goal["goal_target"] = .int(scaled)
            if type != "TB" { goal["goal_creation_month"] = .string("\(start)-01") }
            if store.month == Dates.currentMonth && !payment {
                goal["goal_snoozed_at"] = snoozed ? .string("\(store.month)-01") : .null
            }
            if type == "NEED" {
                goal["goal_cadence"] = .int(Int64(cadence))
                goal["goal_cadence_frequency"] = .int(Int64(interval))
                goal["goal_needs_whole_amount"] = .bool(behaviour == "set")
                if cadence == 2 {
                    goal["goal_day"] = .int(Int64(weekday))
                    goal.remove("goal_target_date"); goal.remove("goal_target_month")
                } else if cadence != 1 || interval > 1 {
                    guard Dates.day(due) != nil else { throw AppError(String(localized: "Choose a due date.")) }
                    goal["goal_target_date"] = .string(due)
                    goal.remove("goal_target_month"); goal.remove("goal_day")
                } else {
                    goal.remove("goal_target_date"); goal.remove("goal_target_month"); goal.remove("goal_day")
                }
            } else if type == "TBD" {
                guard Dates.day(due) != nil else { throw AppError(String(localized: "Choose a date to reach it by.")) }
                goal["goal_target_date"] = .string(due)
            }
            try await store.saveTarget(category: categoryId, goal: goal, amount: amount)
            store.showToast(String(localized: "Set the \(category.text("name")) target to \(money(amount, store.currency))."), undo: true)
            close()
        }
    }
}

/// A category's name, group, note and whether it's hidden.
private struct CategoryDetailsEditor: View {
    @Environment(BudgetStore.self) private var store
    var categoryId: String
    var close: () -> Void
    @State private var name = ""
    @State private var group = ""
    @State private var note = ""
    @State private var hidden = false
    @State private var loaded = false
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        let groups = store.budget.map(groupNames) ?? []
        Form {
            TextField("Name", text: $name)
            Picker("Group", selection: $group) {
                ForEach(groups.contains(group) ? groups : groups + [group], id: \.self) { Text($0).tag($0) }
            }
            TextField("Note", text: $note, axis: .vertical).lineLimit(3...8)
            Toggle(isOn: $hidden) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Hidden")
                    Text("Hidden categories keep their money and history").font(.footnote).foregroundStyle(Color.secondary)
                }
            }
            if let error { FormErrorView(message: error) }
        }
        .disabled(saving)
        .navigationTitle("Name, group & note")
        .inlineTitle()
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                SaveButton(saving: saving) {
                    attempt($error, saving: $saving) {
                        guard let original = store.budget?.row("categories", id: categoryId) else {
                            throw AppError(String(localized: "This category was deleted."))
                        }
                        try await store.upserting("categories", ["id": .string(categoryId), "name": .string(name.trimmingCharacters(in: .whitespaces)),
                                                                 "group": .string(group), "note": .string(note), "hidden": .bool(hidden)], original: original)
                        close()
                    }
                }
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || group.isEmpty)
            }
        }
        .onAppear {
            guard !loaded, let category = store.budget?.row("categories", id: categoryId) else { return }
            loaded = true
            name = category.text("name")
            group = category.text("group")
            note = category.text("note")
            hidden = category.flag("hidden")
        }
    }
}

/// Deleting a category moves its history into another one, or Unassigned.
private struct DeleteCategoryView: View {
    @Environment(BudgetStore.self) private var store
    var categoryId: String
    var close: () -> Void
    @State private var replacement = Unassigned
    @State private var confirmed = false
    @State private var picking = false
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        let budget = store.budget ?? [:]
        let name = budget.row("categories", id: categoryId)?.text("name") ?? String(localized: "category")
        // Whether anything refers to the category: an unused one is deleted without choosing where its history goes.
        let used = budget.rows("assignments").contains { $0.text("categoryId") == categoryId && $0.long("amount") != 0 }
            || budget.rows("transactions").contains { $0.rows("splits").contains { $0.text("categoryId") == categoryId } }
        let destination = destinationName(budget)
        Form {
            if used {
                Button { picking = true } label: {
                    HStack {
                        Text("Move its history into").foregroundStyle(Color.primary)
                        Spacer()
                        Text(destination).foregroundStyle(Color.secondary)
                        Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.urTertiary)
                    }
                }
                Section {
                    Toggle("Delete this category and move its history", isOn: $confirmed)
                } footer: {
                    Text("Transactions, schedules, defaults and loan categories move to the destination. Monthly assignments combine with its assignments; Unassigned takes them back.")
                }
            } else {
                Text("It has no transactions or assigned money.").foregroundStyle(Color.secondary)
            }
            if let error { FormErrorView(message: error) }
            Section {
                Button(role: .destructive) {
                    attempt($error, saving: $saving) {
                        try await store.committingWorkflow(["type": "deleteCategory", "id": .string(categoryId),
                                                            "replacement": .string(used ? replacement : Unassigned)])
                        store.showToast(used ? String(localized: "Deleted \(name). Its history moved to \(destination).")
                                             : String(localized: "Deleted \(name)."), undo: true)
                        close()
                    }
                } label: {
                    HStack {
                        Text("Delete \(name)")
                        if saving { Spacer(); ProgressView() }
                    }
                }
                .disabled(used && !confirmed || saving)
            }
        }
        .navigationTitle("Delete category")
        .inlineTitle()
        .sheet(isPresented: $picking) {
            NavigationStack {
                CategoryPicker(title: String(localized: "Move history into"), month: store.month, selected: replacement, includeUnassigned: true,
                               includePayment: false, excluding: categoryId) { id in
                    replacement = id
                    picking = false
                }
                .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { picking = false } } }
            }
        }
    }

    /// Where the history goes: Unassigned, or the chosen category's name.
    private func destinationName(_ budget: JSON) -> String {
        guard replacement != Unassigned, let category = budget.row("categories", id: replacement) else { return String(localized: "Unassigned") }
        return category.text("name")
    }
}
