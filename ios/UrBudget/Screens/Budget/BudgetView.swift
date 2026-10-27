import SwiftUI

/// Where a budget figure leads in the register: a category or an account, optionally for a month.
struct RegisterTarget: Hashable {
    var category = ""
    var account = ""
    var from = ""
    var through = ""
}

/// A request to move money: the category it starts from, and whether money goes to or comes from another.
struct MoveRequest: Hashable {
    var category: String
    var direction: String
}

enum BudgetSheet: Identifiable {
    case category(String, assign: Bool)
    case categoryPage(String, CategoryRoute)
    case move(MoveRequest)
    case unassigned
    case planning
    case newCategory(group: String?)
    case newGroup
    case renameGroup(String)
    case deleteGroup(String)
    case month
    case addAccount
    case moveToGroup(String)
    case quickAssign(group: String)
    case reorderGroups

    var id: String {
        switch self {
        case .category(let id, let assign): return "category:\(id):\(assign)"
        case .categoryPage(let id, let route): return "categoryPage:\(id):\(route)"
        case .move(let request): return "move:\(request.category):\(request.direction)"
        case .unassigned: return "unassigned"
        case .planning: return "planning"
        case .newCategory(let group): return "new:\(group ?? "")"
        case .newGroup: return "newGroup"
        case .renameGroup(let group): return "rename:\(group)"
        case .deleteGroup(let group): return "delete:\(group)"
        case .month: return "month"
        case .addAccount: return "addAccount"
        case .moveToGroup(let id): return "moveToGroup:\(id)"
        case .quickAssign(let group): return "quickAssign:\(group)"
        case .reorderGroups: return "reorderGroups"
        }
    }
}

/// The budget, as YNAB's app lays it out on a phone: Unassigned as a solid banner, then each group a
/// band with its Assigned and Available totals, and each category a row with both, Available as a
/// solid pill. A tap on a category opens its details; on Assigned, the keypad; on the pill, Move money.
struct BudgetView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    @State private var filter = "all"
    @State private var showHidden = false
    // Hidden categories start folded, as on the desktop; folds are kept per budget.
    @State private var collapsed: Set<String> = [HiddenGroup]
    @State private var foldsLoaded = false
    /// The getting-started steps, for a budget set up from the welcome screen until they're hidden.
    @State private var checklist = false
    @State private var sheet: BudgetSheet?
    @State private var path: [RegisterTarget] = []
    @State private var model: BudgetModel?
    @State private var launchEditor: TransactionEditorRequest?
    @State private var search = ""
    @State private var reordering = false
    @State private var addCompact = false

    private struct ModelKey: Equatable { var version: Int; var month: String; var ready: Bool }

    var body: some View {
        NavigationStack(path: $path) {
            content
                .navigationTitle(Dates.shortMonthLabel(store.month))
                .inlineTitle()
                .toolbar {
                    ToolbarItem(placement: .leadingBar) {
                        if reordering {
                            Button("Done") { withAnimation { reordering = false } }.fontWeight(.semibold)
                        } else {
                            viewsMenu
                        }
                    }
                    ToolbarItem(placement: .principal) { monthSwitcher }
                    ToolbarItem(placement: .trailingBar) {
                        Menu {
                            Button { sheet = .newCategory(group: nil) } label: { Label("New category", systemImage: "tag") }
                            Button { sheet = .newGroup } label: { Label("New category group", systemImage: "folder.badge.plus") }
                            Divider()
                            Button { sheet = .addAccount } label: { Label("New account", systemImage: "building.columns") }
                        } label: {
                            Image(systemName: "plus")
                        }
                        .accessibilityLabel("Add a category or group")
                    }
                    ToolbarItem(placement: .trailingBar) { BudgetActionsMenu() }
                }
                .navigationDestination(for: RegisterTarget.self) { TransactionsList(target: $0) }
                .overlay(alignment: .bottomTrailing) { AddTransactionButton(compact: addCompact) }
        }
        .task(id: ModelKey(version: store.version, month: store.month, ready: store.summary != nil)) { await rebuild() }
        .sheet(item: $sheet) { sheetContent($0) }
        .cover(item: $launchEditor) { TransactionEditor(request: $0) }
        .onAppear(perform: loadFolds)
        .onChange(of: collapsed) { _, folds in
            guard foldsLoaded, let key = store.budgetKey else { return }
            UserDefaults.standard.set(Array(folds), forKey: "folds.\(key)")
        }
        .onAppear {
            // Debug builds can open a sheet at launch (`-open unassigned`), for device screenshots.
            switch LaunchRoute.open {
            case "unassigned": sheet = .unassigned
            case "transaction": launchEditor = TransactionEditorRequest()
            case "category":
                if let first = store.budget?.rows("categories").first { sheet = .category(first.text("id"), assign: false) }
            default: return
            }
            LaunchRoute.open = nil
        }
    }

    private func loadFolds() {
        guard !foldsLoaded else { return }
        foldsLoaded = true
        guard let key = store.budgetKey else { return }
        if let saved = UserDefaults.standard.stringArray(forKey: "folds.\(key)") { collapsed = Set(saved) }
        checklist = StarterBudget.checklistShown(store)
    }

    private func hideChecklist() {
        if let key = store.budgetKey { UserDefaults.standard.set("off", forKey: "setup.\(key)") }
        withAnimation { checklist = false }
    }

    /// The list's model for this snapshot, built off the main thread; the last one stays meanwhile.
    private func rebuild() async {
        guard let budget = store.budget, let summary = store.summary else { return }
        let month = store.month
        let built = await Task.detached(priority: .userInitiated) { buildBudgetModel(budget: budget, summary: summary, month: month) }.value
        if !Task.isCancelled { model = built }
    }

    @ViewBuilder private var content: some View {
        if let error = store.calculationError {
            EmptyState(title: String(localized: "Budget calculation unavailable"),
                       detail: String(localized: "\(error) Your saved data remains available to export in Settings & data."), symbol: "exclamationmark.triangle")
        } else if let model {
            list(model)
        } else {
            ProgressView()
        }
    }

    private func visibleGroups(_ model: BudgetModel) -> [(group: GroupModel, rows: [CategoryRowModel])] {
        let query = search.trimmingCharacters(in: .whitespaces)
        return model.groups.filter { showHidden || $0.name != HiddenGroup || !query.isEmpty }
            .map { group in
                (group, group.rows.filter { $0.inView(filter) && (query.isEmpty || $0.name.localizedCaseInsensitiveContains(query)
                                                                  || group.name.localizedCaseInsensitiveContains(query)) })
            }
            .filter { !$0.rows.isEmpty || filter == "all" && query.isEmpty && $0.group.name != HiddenGroup }
    }

    private func counts(_ model: BudgetModel) -> [String: Int] {
        let rows = model.groups.filter { showHidden || $0.name != HiddenGroup }.flatMap(\.rows)
        var counts: [String: Int] = [:]
        for view in budgetViews { counts[view.id] = rows.count { $0.inView(view.id) } }
        return counts
    }

    private func list(_ model: BudgetModel) -> some View {
        let currency = store.currency
        let visible = visibleGroups(model)
        let overspent = counts(model)["overspent"] ?? 0
        // Columns as wide as the widest figure: with tabular digits only the longest strings matter.
        let assignedWidth = max(76, figureWidth(model.widestAssigned, size: 15, bold: false) + 8)
        let availableWidth = max(88, figureWidth(model.widestAvailable, size: 15, bold: true) + 26)
        let current = Dates.currentMonth
        return List {
            Section {
                if store.month != current {
                    Button { store.selectMonth(current) } label: {
                        Label("Back to \(Dates.shortMonthLabel(current))", systemImage: "calendar")
                            .font(.subheadline.weight(.semibold))
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)
                    .buttonBorderShape(.capsule)
                }
                UnassignedBanner(amount: model.readyToAssign, currency: currency) { sheet = .unassigned }
                if checklist, let budget = store.budget, budget["sample"].isNull, budget["importedFrom"].isNull, budget["ynabPlanId"].isNull {
                    GettingStartedCard(budget: budget, addAccount: { sheet = .addAccount }, assign: { sheet = .unassigned },
                                       addTransaction: { launchEditor = TransactionEditorRequest() },
                                       setTarget: {
                                           let categories = budget.rows("categories").filter { !$0.flag("hidden") && $0.text("paymentAccountId").isEmpty }
                                           if let first = categories.first(where: { $0.long("target") == 0 && !$0["goal"].isObject }) ?? categories.first {
                                               sheet = .categoryPage(first.text("id"), .target)
                                           } else {
                                               sheet = .newCategory(group: nil)
                                           }
                                       },
                                       hide: hideChecklist)
                }
                // The getting-started steps say this too, with their own button.
                if store.budget?.rows("accounts").isEmpty == true && !checklist {
                    Notice(text: String(localized: "Add the account your money is in. Its balance becomes money to assign."), tone: "info",
                           icon: "building.columns.fill", action: String(localized: "Add account")) { sheet = .addAccount }
                }
                if model.futureAssigned != 0 || !model.shortfalls.isEmpty {
                    Notice(text: futureText(model, currency), tone: model.shortfalls.isEmpty ? "info" : "negative",
                           icon: model.shortfalls.isEmpty ? "info.circle.fill" : "exclamationmark.triangle.fill")
                }
                if model.needsCategory.total > 0 {
                    Notice(text: needsCategoryText(model.needsCategory.total, money(model.needsCategory.amount, currency)),
                           tone: "attention", icon: "tag.fill", action: String(localized: "Review")) { path.append(RegisterTarget(category: NeedsCategory)) }
                }
                if overspent > 0 && filter == "all" {
                    Notice(text: String(localized: "\(overspent) overspent categories", locale: .app), tone: "negative",
                           icon: "exclamationmark.circle.fill", action: String(localized: "Show")) { filter = "overspent" }
                }
                if filter != "all" {
                    HStack {
                        showingText(budgetViews.first { $0.id == filter }?.label ?? "")
                        Spacer()
                        Button("Show all") { filter = "all" }.buttonStyle(.borderless)
                    }
                    .font(.subheadline)
                }
                if let note = model.monthNote, filter == "all" {
                    Button { sheet = .planning } label: {
                        Label { Text(note).lineLimit(4).foregroundStyle(Color.secondary) } icon: { Image(systemName: "note.text").foregroundStyle(Color.secondary) }
                            .font(.subheadline)
                    }
                }
            }
            .listRowSeparator(.hidden)
            .listRowInsets(EdgeInsets(top: 5, leading: 16, bottom: 5, trailing: 16))

            if model.uncategorized < 0 && (filter == "all" || filter == "overspent") {
                Section {
                    UncategorizedLine(activity: model.uncategorized, credit: model.uncategorizedCredit > 0, currency: currency,
                                      assignedWidth: assignedWidth, availableWidth: availableWidth) {
                        path.append(RegisterTarget(category: NeedsCategory))
                    }
                }
            }
            if visible.isEmpty {
                Section {
                    if filter == "all" {
                        EmptyState(title: String(localized: "Start your plan"),
                                   detail: String(localized: "Create categories for bills, everyday spending, and the things you’re saving for."),
                                   action: String(localized: "Add category")) { sheet = .newCategory(group: nil) }
                    } else {
                        EmptyState(title: String(localized: "Nothing to show"), detail: String(localized: "No categories match this view this month."),
                                   action: String(localized: "Show all")) { filter = "all" }
                    }
                }
                .listRowSeparator(.hidden)
            }
            ForEach(visible, id: \.group.name) { group, rows in
                let open = !collapsed.contains(group.name)
                Section {
                    if open {
                        ForEach(rows) { row in
                            CategoryLine(row: row, assignedWidth: assignedWidth, availableWidth: availableWidth,
                                         open: { sheet = .category(row.id, assign: false) },
                                         assign: { sheet = .category(row.id, assign: true) },
                                         move: { sheet = .move(MoveRequest(category: row.id, direction: row.available < 0 ? "from" : "to")) },
                                         hide: { setHidden(row, !row.hidden) },
                                         regroup: { sheet = .moveToGroup(row.id) },
                                         page: { sheet = .categoryPage(row.id, $0) })
                        }
                        .onMove { from, to in reorder(rows, in: group.name, from: from, to: to) }
                        .moveDisabled(!reordering || group.name == HiddenGroup)
                        if rows.isEmpty && group.name != HiddenGroup {
                            Button { sheet = .newCategory(group: group.name) } label: { Label("Add a category", systemImage: "plus") }
                        }
                    }
                } header: {
                    GroupHeader(name: group.name, open: open, rows: rows, currency: currency, assignedWidth: assignedWidth, availableWidth: availableWidth,
                                toggle: { withAnimation { if open { collapsed.insert(group.name) } else { collapsed.remove(group.name) } } },
                                action: group.name == HiddenGroup ? nil : { groupAction($0, group) })
                }
            }
            // Room for the last row to scroll clear of the add button.
            if filter == "all" && search.isEmpty && !reordering {
                Section {
                    HStack(spacing: 12) {
                        Button { sheet = .newCategory(group: nil) } label: {
                            Label(String(localized: "budget.addCategory", defaultValue: "Category", comment: "Button that adds a category."),
                                  systemImage: "plus").frame(maxWidth: .infinity)
                        }
                        Button { sheet = .newGroup } label: {
                            Label("Category group", systemImage: "folder.badge.plus").frame(maxWidth: .infinity)
                        }
                    }
                    .buttonStyle(.bordered)
                    .font(.subheadline.weight(.semibold))
                    .padding(.vertical, 8)
                }
                .listRowSeparator(.hidden)
            }
            Color.clear.frame(height: 64).listRowSeparator(.hidden).listRowBackground(Color.clear)
        }
        .listStyle(.plain)
        .readableWidth()
        .shrinksAddButton($addCompact)
        .searchable(text: $search, placement: .automatic, prompt: "Find a category")
        .reorderMode(reordering)
        .refreshable {
            store.foreground(force: true)
            await sync.syncAutomatically()
        }
    }

    /// The sentences about money assigned in later months, one after another.
    private func futureText(_ model: BudgetModel, _ currency: String) -> String {
        var sentences: [String] = []
        if model.futureAssigned != 0 {
            sentences.append(String(localized: "Assigned in future months: \(money(model.futureAssigned, currency))."))
        }
        if !model.shortfalls.isEmpty {
            for shortfall in model.shortfalls {
                sentences.append(String(localized: "\(Dates.monthLabel(shortfall.month)) is short by \(money(shortfall.amount, currency))."))
            }
            sentences.append(String(localized: "Move funds back from a future month."))
        } else if model.futureAssigned > 0 && model.futureReadyToAssign + model.futureAssigned >= 0 {
            sentences.append(String(localized: "It is held back from this month's unassigned money."))
        }
        return sentences.joined(separator: " ")
    }

    /// "3 transactions need a category. 45.00 counts as overspending until they have one."
    private func needsCategoryText(_ count: Int, _ amount: String) -> String {
        let first = String(localized: "\(count) transactions need a category.", locale: .app)
        let second = count == 1 ? String(localized: "\(amount) counts as overspending until it has one.")
            : String(localized: "\(amount) counts as overspending until they have one.")
        return first + " " + second
    }

    /// "Showing Overspent", the view's name in bold.
    private func showingText(_ view: String) -> Text {
        Text("Showing \(Text(view).bold().foregroundStyle(Color.primary))").foregroundStyle(Color.secondary)
    }

    private var monthSwitcher: some View {
        HStack(spacing: 2) {
            Button { store.selectMonth(Dates.addMonths(store.month, -1)) } label: { Image(systemName: "chevron.backward") }
                .accessibilityLabel("Previous month")
            Button { sheet = .month } label: {
                HStack(spacing: 3) {
                    Text(Dates.shortMonthLabel(store.month)).font(.headline).foregroundStyle(Color.primary)
                    Image(systemName: "chevron.down").font(.caption.weight(.bold)).foregroundStyle(Color.secondary)
                }
            }
            .accessibilityLabel("Choose month, \(Dates.monthLabel(store.month))")
            Button { store.selectMonth(Dates.addMonths(store.month, 1)) } label: { Image(systemName: "chevron.forward") }
                .accessibilityLabel("Next month")
        }
    }

    private var viewsMenu: some View {
        let counts = model.map(counts) ?? [:]
        let names = model.map(visibleGroups)?.map(\.group.name) ?? []
        let allCollapsed = !names.isEmpty && names.allSatisfy(collapsed.contains)
        return Menu {
            Picker("View", selection: $filter) {
                ForEach(budgetViews, id: \.id) { view in
                    Text(view.id == "all" ? view.label : "\(view.label) (\(counts[view.id] ?? 0))").tag(view.id)
                }
            }
            Divider()
            Toggle(isOn: $showHidden) { Label("Show hidden categories", systemImage: "eye") }
            Button {
                // Expanding keeps Hidden folded; folding folds it too.
                withAnimation { collapsed = allCollapsed ? [HiddenGroup] : Set(names + [HiddenGroup]) }
            } label: {
                Label(allCollapsed ? "Expand all" : "Collapse all", systemImage: allCollapsed ? "arrow.up.left.and.arrow.down.right" : "arrow.down.right.and.arrow.up.left")
            }
            Divider()
            Button { withAnimation { reordering = true } } label: { Label("Reorder categories", systemImage: "arrow.up.arrow.down") }
            Button { sheet = .reorderGroups } label: { Label("Reorder groups", systemImage: "list.bullet.indent") }
            Button { sheet = .planning } label: { Label("Month note & income", systemImage: "note.text") }
        } label: {
            Image(systemName: filter == "all" ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
        }
        .accessibilityLabel("Views")
    }

    private func reorder(_ rows: [CategoryRowModel], in group: String, from: IndexSet, to: Int) {
        guard let index = from.first, rows.indices.contains(index) else { return }
        var order = rows
        order.move(fromOffsets: from, toOffset: to)
        guard let position = order.firstIndex(where: { $0.id == rows[index].id }) else { return }
        let before = order.indices.contains(position + 1) ? JSON.string(order[position + 1].id) : JSON.null
        store.workflow(["type": "organizeCategory", "id": .string(rows[index].id), "group": .string(group), "beforeId": before])
    }

    private func setHidden(_ row: CategoryRowModel, _ hidden: Bool) {
        store.upsert("categories", ["id": .string(row.id), "hidden": .bool(hidden)]) {
            store.showToast(hidden ? String(localized: "Hid \(row.name). Find it under Hidden categories.")
                                   : String(localized: "\(row.name) is shown again."), undo: true)
        }
    }

    private func groupAction(_ action: String, _ group: GroupModel) {
        switch action {
        case "add": sheet = .newCategory(group: group.name)
        case "rename": sheet = .renameGroup(group.name)
        case "delete": sheet = .deleteGroup(group.name)
        case "quickAssign": sheet = .quickAssign(group: group.name)
        case "hide":
            store.upsertMany("categories", group.rows.map { ["id": .string($0.id), "hidden": true] }) {
                store.showToast(String(localized: "Hid the categories in \(group.name). Find them under Hidden categories."), undo: true)
            }
        case "up", "down": moveGroup(group.name, up: action == "up")
        default: break
        }
    }

    private func moveGroup(_ group: String, up: Bool) {
        guard let budget = store.budget else { return }
        let groups = orderedGroups(budget) { !$0.flag("hidden") }
        guard let index = groups.firstIndex(where: { $0.name == group }) else { return }
        let target = up ? index - 1 : index + 1
        guard groups.indices.contains(target), let first = groups[index].categories.first else { return }
        // groupBefore places the group before another; after the last group is the end.
        let before = up ? groups[target].name : (groups.indices.contains(target + 1) ? groups[target + 1].name : nil)
        let next = groups[index].categories.count > 1 ? JSON.string(groups[index].categories[1].text("id")) : JSON.null
        store.workflow(["type": "organizeCategory", "id": .string(first.text("id")), "group": .string(group),
                        "beforeId": next, "groupBefore": JSON.text(orNull: before)])
    }

    @ViewBuilder private func sheetContent(_ request: BudgetSheet) -> some View {
        switch request {
        case .category(let id, let assign):
            CategorySheet(categoryId: id, startAssigning: assign,
                          move: { sheet = .move(MoveRequest(category: $0, direction: $1)) },
                          transactions: { target in sheet = nil; path.append(target) })
        case .categoryPage(let id, let page):
            CategorySheet(categoryId: id, startAssigning: false,
                          move: { sheet = .move(MoveRequest(category: $0, direction: $1)) },
                          transactions: { target in sheet = nil; path.append(target) }, startRoute: page)
        case .move(let move):
            MoveMoneySheet(request: move)
        case .unassigned:
            UnassignedSheet(assignTo: {
                sheet = .move(MoveRequest(category: Unassigned, direction: (store.summary?.long("readyToAssign") ?? 0) < 0 ? "from" : "to"))
            }, editIncome: { sheet = .planning })
        case .planning:
            BudgetPlanningForm(month: store.month)
        case .newCategory(let group):
            NewCategoryForm(presetGroup: group)
        case .newGroup:
            NewGroupForm()
        case .renameGroup(let group):
            RenameGroupForm(group: group)
        case .deleteGroup(let group):
            DeleteGroupForm(group: group)
        case .month:
            MonthPickerSheet(month: store.month) { store.selectMonth($0); sheet = nil }
        case .addAccount:
            AccountForm(request: AccountFormRequest())
        case .moveToGroup(let id):
            MoveToGroupForm(categoryId: id)
        case .quickAssign(let group):
            GroupQuickAssignSheet(group: group)
        case .reorderGroups:
            ReorderGroupsSheet()
        }
    }
}

/// Unassigned as a solid banner: green with money to give jobs to, red when more is assigned than there is.
struct UnassignedBanner: View {
    var amount: Int64
    var currency: String
    var action: () -> Void

    var body: some View {
        let fill: Color = amount > 0 ? .urPositive : amount < 0 ? .urNegative : .urZero
        let ink: Color = amount == 0 ? .primary : .urOnSolid
        Button(action: action) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(money(amount, currency)).font(.system(size: 28, weight: .heavy, design: .rounded)).monospacedDigit()
                        .lineLimit(1).minimumScaleFactor(0.6)
                    Text(amount > 0 ? "Tap to assign" : amount < 0 ? "More is assigned than you have" : "All of it is assigned")
                        .font(.footnote).opacity(0.85)
                }
                Spacer()
                Text("Unassigned").font(.headline)
                Image(systemName: "chevron.forward").font(.subheadline.weight(.bold))
            }
            .foregroundStyle(ink)
            .padding(.horizontal, 18)
            .padding(.vertical, 12)
            .background(fill, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Unassigned \(money(amount, currency)). Assign money")
    }
}

/// A group as a band across the list, with its Assigned and Available totals for the rows shown.
private struct GroupHeader: View {
    var name: String
    var open: Bool
    var rows: [CategoryRowModel]
    var currency: String
    var assignedWidth: CGFloat
    var availableWidth: CGFloat
    var toggle: () -> Void
    var action: ((String) -> Void)?
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let assigned = rows.reduce(0) { $0 + $1.assigned }
        let available = rows.reduce(0) { $0 + $1.available }
        let stacked = VStack(alignment: .leading, spacing: 4) {
            title(wraps: true)
            HStack {
                column("Assigned", money(assigned, currency), .primary, nil)
                Spacer(minLength: 8)
                column("Available", money(available, currency), available < 0 ? .urNegative : .primary, nil)
            }
        }
        Group {
            // At the largest text sizes, or when the name doesn't fit on one line beside the totals
            // (a narrow phone, large figures), the totals go under the name.
            if typeSize.isAccessibilitySize {
                stacked
            } else {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 6) {
                        title(wraps: false)
                        Spacer(minLength: 4)
                        column("Assigned", money(assigned, currency), .primary, assignedWidth)
                        column("Available", money(available, currency), available < 0 ? .urNegative : .primary, availableWidth)
                    }
                    stacked
                }
            }
        }
        .textCase(nil)
        .padding(.vertical, 4)
    }

    /// The group's name; the hidden categories' own group is named in the app's language.
    private var displayName: String { name == HiddenGroup ? String(localized: "Hidden categories") : name }

    private func title(wraps: Bool) -> some View {
        let displayName = displayName
        return HStack(spacing: 6) {
            Button(action: toggle) {
                HStack(spacing: 6) {
                    // Points the way text reads while folded (mirrored right to left), down when open.
                    Image(systemName: "chevron.right")
                        .font(.footnote.weight(.bold))
                        .rotationEffect(.degrees(open ? 90 : 0))
                        .flipsForRightToLeftLayoutDirection(true)
                        .foregroundStyle(Color.secondary)
                    Text(displayName).font(.headline).foregroundStyle(Color.primary).lineLimit(wraps ? 2 : 1).multilineTextAlignment(.leading)
                        .fixedSize(horizontal: !wraps, vertical: false)
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel(open ? "Collapse \(displayName)" : "Expand \(displayName)")
            if let action {
                Menu {
                    Button { action("add") } label: { Label("Add category", systemImage: "plus") }
                    if !rows.isEmpty {
                        Button { action("quickAssign") } label: { Label("Quick assign…", systemImage: "wand.and.stars") }
                    }
                    Button { action("rename") } label: { Label("Rename group", systemImage: "pencil") }
                    if !rows.isEmpty {
                        Button { action("up") } label: { Label("Move up", systemImage: "arrow.up") }
                        Button { action("down") } label: { Label("Move down", systemImage: "arrow.down") }
                        Button { action("hide") } label: { Label("Hide all categories", systemImage: "eye.slash") }
                    }
                    Divider()
                    Button(role: .destructive) { action("delete") } label: { Label("Delete group…", systemImage: "trash") }
                } label: {
                    Image(systemName: "ellipsis").font(.footnote.weight(.bold)).foregroundStyle(Color.secondary).frame(width: 28, height: 28)
                }
                .accessibilityLabel("Actions for group \(name)")
            }
        }
    }

    private func column(_ label: LocalizedStringKey, _ value: String, _ color: Color, _ width: CGFloat?) -> some View {
        VStack(alignment: .trailing, spacing: 1) {
            Text(label).font(.caption2.weight(.medium)).foregroundStyle(Color.secondary)
            Text(value).figure(14, weight: .bold).foregroundStyle(color).lineLimit(1).minimumScaleFactor(0.7)
        }
        .frame(width: width, alignment: .trailing)
    }
}

/// A category: its name over what it needs (only when there's something to do), Assigned, and the pill.
private struct CategoryLine: View {
    var row: CategoryRowModel
    var assignedWidth: CGFloat
    var availableWidth: CGFloat
    var open: () -> Void
    var assign: () -> Void
    var move: () -> Void
    var hide: () -> Void
    var regroup: () -> Void
    var page: (CategoryRoute) -> Void
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        Group {
            // At the largest text sizes the figures go under the name, which keeps its width.
            if typeSize.isAccessibilitySize {
                VStack(alignment: .leading, spacing: 8) {
                    names
                    HStack {
                        assignedButton(width: nil)
                        Spacer(minLength: 8)
                        pillButton(width: nil)
                    }
                }
            } else {
                HStack(spacing: 6) {
                    names.frame(maxWidth: .infinity, alignment: .leading)
                    assignedButton(width: assignedWidth)
                    pillButton(width: availableWidth)
                }
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: open)
        .accessibilityAction(named: "Open \(row.name)", open)
        .swipeActions(edge: .leading) {
            Button(action: assign) { Label("Assign", systemImage: "plus.forwardslash.minus") }.tint(.urAccent)
        }
        .swipeActions(edge: .trailing) {
            Button(action: move) { Label(row.available < 0 ? "Cover" : "Move", systemImage: "arrow.left.arrow.right") }.tint(.urPositive)
        }
        .contextMenu {
            Button(action: open) { Label("Details", systemImage: "info.circle") }
            Button(action: assign) { Label("Assign", systemImage: "plus.forwardslash.minus") }
            Button(action: move) { Label(row.available < 0 ? "Cover overspending" : "Move money", systemImage: "arrow.left.arrow.right") }
            Button { page(.target) } label: { Label("Target…", systemImage: "target") }
            if !row.payment {
                Button { page(.details) } label: { Label("Name, group & note…", systemImage: "pencil") }
                Button(action: regroup) { Label("Move to another group", systemImage: "folder") }
                Button(action: hide) { Label(row.hidden ? "Show category" : "Hide category", systemImage: row.hidden ? "eye" : "eye.slash") }
                Divider()
                Button(role: .destructive) { page(.delete) } label: { Label("Delete category…", systemImage: "trash") }
            }
        }
    }

    private var names: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 5) {
                Text(row.name).font(.body.weight(.medium)).lineLimit(2)
                if !row.note.isEmpty {
                    Image(systemName: "note.text").font(.caption).foregroundStyle(Color.secondary)
                        .accessibilityLabel("Note: \(row.note)")
                }
            }
            if let detail = row.detail {
                HStack(spacing: 3) {
                    if row.snoozed && row.status != "negative" { Image(systemName: "moon.zzz.fill") }
                    Text(detail).lineLimit(2)
                }
                .font(.footnote.weight(.semibold))
                .foregroundStyle(row.tone.color)
            }
            if let progress = row.progress, !row.snoozed {
                TargetBar(progress: progress, status: row.status).padding(.top, 3)
            }
        }
    }

    private func assignedButton(width: CGFloat?) -> some View {
        Button(action: assign) {
            HStack(spacing: 4) {
                if row.moved { Circle().fill(Color.urAccent).frame(width: 6, height: 6) }
                if width == nil { Text("Assigned").font(.footnote).foregroundStyle(Color.secondary) }
                Text(row.assignedText).figure(15).foregroundStyle(row.assigned == 0 ? .secondary : .primary)
                    .lineLimit(1).minimumScaleFactor(0.7)
            }
            .frame(width: width, alignment: .trailing)
            .padding(.vertical, 8)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Assign to \(row.name), \(row.assignedText)")
    }

    private func pillButton(width: CGFloat?) -> some View {
        Button(action: move) { AvailablePill(text: row.availableText, status: row.status) }
            .buttonStyle(.plain)
            .frame(width: width, alignment: .trailing)
            .accessibilityLabel("Move money for \(row.name), \(row.availableText) available")
    }
}

/// YNAB's Uncategorized row: this month's spending with no category.
private struct UncategorizedLine: View {
    var activity: Int64
    var credit: Bool
    var currency: String
    var assignedWidth: CGFloat
    var availableWidth: CGFloat
    var review: () -> Void

    var body: some View {
        Button(action: review) {
            HStack(spacing: 6) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Uncategorized").font(.body.weight(.medium)).foregroundStyle(Color.primary)
                    Text("Needs a category").font(.footnote.weight(.semibold)).foregroundStyle(Color.urNegative)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Text(money(0, currency)).figure(15).foregroundStyle(Color.secondary).frame(width: assignedWidth, alignment: .trailing)
                AvailablePill(text: money(activity, currency), status: credit ? "credit" : "negative").frame(width: availableWidth, alignment: .trailing)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Uncategorized spending, \(money(activity, currency)). Show transactions")
    }
}

/// The groups in order, dragged into a new one; it saves as the budget's group order.
private struct ReorderGroupsSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var groups: [String] = []
    @State private var loaded = false
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(groups, id: \.self) { Text($0) }
                        .onMove { groups.move(fromOffsets: $0, toOffset: $1) }
                } footer: {
                    Text("Drag the handles. Hidden categories stay at the end.")
                }
                if let error { FormErrorView(message: error) }
            }
            .groupedList()
            .reorderMode(true)
            .disabled(saving)
            .navigationTitle("Reorder groups")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    SaveButton(saving: saving) {
                        let order = groups
                        attempt($error, saving: $saving) {
                            try await store.editing { budget in
                                // Groups the list doesn't show (empty ones waiting for a category) keep their place after these.
                                let rest = budget["groupOrder"].array.filter { !order.contains($0.string) }
                                budget["groupOrder"] = .array(order.map(JSON.string) + rest)
                            }
                            store.showToast(String(localized: "Reordered the groups."), undo: true)
                            dismiss()
                        }
                    }
                }
            }
        }
        .presentationDetents([.large])
        .onAppear {
            guard !loaded, let budget = store.budget else { return }
            loaded = true
            groups = groupNames(budget)
        }
    }
}
