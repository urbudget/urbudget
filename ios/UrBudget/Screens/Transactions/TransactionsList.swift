import SwiftUI
import UniformTypeIdentifiers

/// One transaction as the register draws it: everything formatted once, off the main thread.
private struct TransactionLine: Identifiable, Sendable {
    struct Chip: Sendable, Hashable { var text: String; var attention: Bool }
    var id: String
    var date: String
    var payee: String
    var transfer: Bool
    var card: Bool
    var otherAccount: String
    var amount: Int64
    var amountText: String
    var chips: [Chip]
    var accountName: String
    var memo: String
    var cleared: String
    var future: Bool
    var flag: String
    var approved: Bool
}

private struct DaySection: Identifiable, Sendable {
    var id: String
    var label: String
    var lines: [TransactionLine]
}

/// Turns the matching entries into lines, in day sections when sorted by date.
private func registerSections(_ entries: [RegisterIndex.Entry], order: String, account: String, budget: JSON, today: String) -> [DaySection] {
    let currency = budget.text("currency")
    let monthFirst = budget.text("dateFormat") == "mdy"
    var accounts: [String: JSON] = [:]
    for a in budget.rows("accounts") { accounts[a.text("id")] = a }
    var categories: [String: String] = [:]
    for c in budget.rows("categories") { categories[c.text("id")] = c.text("name") }
    func splitName(_ part: JSON) -> String {
        if part.flag("unassigned") { return String(localized: "Unassigned") }
        if !part.text("transferAccountId").isEmpty { return String(localized: "Transfer") }
        return categories[part.text("categoryId")] ?? String(localized: "Uncategorized")
    }
    let lines = entries.map { entry -> TransactionLine in
        let t = entry.transaction
        let amount = entry.amount(for: account)
        let transferId = t.text("transferAccountId")
        // Seen from the receiving account, a transfer shows its own side's memo and flag.
        let reverse = !account.isEmpty && transferId == account
        let source = reverse ? t["transferSource"] : JSON.null
        let other = reverse ? t.text("accountId") : transferId
        let otherAccount = accounts[other]
        let splits = t.rows("splits")
        var chips: [TransactionLine.Chip] = []
        if splits.count > 1 {
            for (i, part) in splits.prefix(2).enumerated() {
                let needs = entry.needsCategory.contains(i)
                chips.append(.init(text: "\(needs ? String(localized: "Needs a category") : splitName(part)) \(money(part.long("amount"), currency))", attention: needs))
            }
            if splits.count > 2 { chips.append(.init(text: AppLanguage.leftToRight("+\(splits.count - 2)"), attention: false)) }
        } else if entry.needsCategory.contains(0) {
            chips = [.init(text: String(localized: "Needs a category"), attention: true)]
        } else if let first = splits.first {
            if first.flag("unassigned") { chips = [.init(text: String(localized: "Unassigned"), attention: false)] }
            else if !first.text("categoryId").isEmpty { chips = [.init(text: categories[first.text("categoryId")] ?? String(localized: "Category"), attention: false)] }
        }
        let statuses = account.isEmpty ? [entry.status] : entry.accountStatuses[account] ?? [entry.status]
        return TransactionLine(
            id: entry.id, date: entry.date,
            payee: !transferId.isEmpty ? transferLabel(amount: amount, otherAccount?.text("name")) : (t.text("payee").isEmpty ? String(localized: "No payee") : t.text("payee")),
            transfer: !transferId.isEmpty, card: !transferId.isEmpty && otherAccount?.text("kind") == "credit", otherAccount: other,
            amount: amount, amountText: AppLanguage.leftToRight((amount > 0 ? "+" : "") + money(amount, currency)), chips: chips,
            accountName: account.isEmpty ? accounts[t.text("accountId")]?.text("name") ?? "" : "",
            memo: source.text("memo").isEmpty ? t.text("memo") : source.text("memo"),
            cleared: statuses.first ?? entry.status, future: entry.date > today,
            flag: reverse ? source.text("flag_color") : entry.flag, approved: entry.approved)
    }
    guard order == "newest" || order == "oldest" else { return [DaySection(id: "all", label: "", lines: lines)] }
    var sections: [DaySection] = []
    for line in lines {
        if sections.last?.id == line.date { sections[sections.count - 1].lines.append(line) }
        else { sections.append(DaySection(id: line.date, label: Dates.dayLabel(line.date, monthFirst: monthFirst), lines: [line])) }
    }
    return sections
}

/// A transfer's payee line: "To: Savings" for money going out, "From: Savings" for money coming in.
private func transferLabel(amount: Int64, _ account: String?) -> String {
    let name = account ?? String(localized: "Account")
    return amount < 0 ? String(localized: "To: \(name)") : String(localized: "From: \(name)")
}

/// Date range presets, as the desktop's register offers them.
private let datePresets: [(id: String, label: String)] = [
    ("thisMonth", String(localized: "This month")), ("lastMonth", String(localized: "Last month")),
    ("last3", String(localized: "Last 3 months")), ("last6", String(localized: "Last 6 months")),
    ("last12", String(localized: "Last 12 months")), ("ytd", String(localized: "Year to date")), ("lastYear", String(localized: "Last year")),
]

private func datePreset(_ id: String) -> (String, String) {
    let now = Dates.currentMonth
    func months(_ back: Int) -> (String, String) { (Dates.monthBounds(Dates.addMonths(now, -back)).0, Dates.monthBounds(now).1) }
    switch id {
    case "thisMonth": return months(0)
    case "lastMonth": return Dates.monthBounds(Dates.addMonths(now, -1))
    case "last3": return months(2)
    case "last6": return months(5)
    case "last12": return months(11)
    case "ytd": return ("\(now.prefix(4))-01-01", Dates.monthBounds(now).1)
    case "lastYear":
        let year = Dates.year(now) - 1
        return ("\(year)-01-01", "\(year)-12-31")
    default: return ("", "")
    }
}

private func dateRangeLabel(_ from: String, _ through: String) -> String {
    switch (from.isEmpty, through.isEmpty) {
    case (false, false): return "\(Dates.shortDay(from)) – \(Dates.shortDay(through))"
    case (false, true): return String(localized: "From \(Dates.shortDay(from))")
    case (true, false): return String(localized: "Through \(Dates.shortDay(through))")
    default: return String(localized: "Any date")
    }
}

private func bulkAction(_ ids: Set<String>, _ operation: String, _ value: String, _ account: String) -> JSON {
    ["type": "bulk", "ids": JSON(Array(ids)), "operation": .string(operation), "value": .string(value), "accountId": .string(account.isEmpty ? "all" : account)]
}

/// "1 transaction", "3 transactions".
func transactionCount(_ count: Int) -> String {
    String(localized: "transactions.inSentence", defaultValue: "\(count) transactions", locale: .app,
           comment: "A number of transactions inside a sentence (“Delete 3 transactions?”).")
}

/// A CSV file for the share sheet and Files.
struct TextFile: FileDocument {
    static var readableContentTypes: [UTType] { [.commaSeparatedText, .json, .plainText] }
    var data: Data
    init(data: Data) { self.data = data }
    init(configuration: ReadConfiguration) throws { data = configuration.file.regularFileContents ?? Data() }
    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper { FileWrapper(regularFileWithContents: data) }
}

/// Transactions: the days as sections with the date written out, each transaction its payee and
/// amount over its categories as chips, the account and the memo. An account's transactions lead
/// with its balances. Search and filters are in the navigation bar; Select edits many at once.
struct TransactionsList: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    var target: RegisterTarget

    @State private var search = ""
    @State private var query = ""
    @State private var status = ""
    @State private var flag = ""
    @State private var category: String
    @State private var from: String
    @State private var through: String
    @State private var order = "newest"
    @State private var index: RegisterIndex?
    @State private var indexVersion = -1
    @State private var sections: [DaySection]?
    @State private var selecting = false
    @State private var selected: Set<String> = []
    @State private var editing: TransactionEditorRequest?
    @State private var accountForm: AccountFormRequest?
    @State private var reconciling: JSON?
    @State private var bulkCategory = false
    @State private var categorizing: String?
    @State private var bulkText: BulkTextRequest?
    @State private var moreFilters = false
    @State private var addCompact = false
    @State private var pickingCategory = false
    @State private var minimum = ""
    @State private var maximum = ""
    @State private var confirmDelete = false
    @State private var exportFile: TextFile?
    @State private var exporting = false
    @State private var showAllScheduled = false
    @State private var paying: String?
    @State private var loanFor: String?
    @State private var editingSchedule: IdentifiedJSON?
    @State private var jump: RegisterTarget?
    @State private var importing = false
    @State private var asking: AssistantRequest?

    init(target: RegisterTarget) {
        self.target = target
        _category = State(initialValue: target.category)
        _from = State(initialValue: target.from)
        _through = State(initialValue: target.through)
    }

    private struct LinesKey: Equatable {
        var version: Int
        var filter: RegisterIndex.Filter
    }

    private var filter: RegisterIndex.Filter {
        let low = minimum.isEmpty ? nil : try? Money.parse(minimum)
        let high = maximum.isEmpty ? nil : try? Money.parse(maximum)
        return RegisterIndex.Filter(account: target.account, status: status, category: category, flag: flag, from: from, through: through,
                                    minimum: low, maximum: high,
                                    invalidAmount: !minimum.isEmpty && low == nil || !maximum.isEmpty && high == nil,
                                    search: query, order: order)
    }

    private var account: JSON? { target.account.isEmpty ? nil : store.budget?.row("accounts", id: target.account) }

    private var title: String {
        if let account { return account.text("name") }
        switch category {
        case NeedsCategory: return String(localized: "Uncategorized")
        case Unassigned: return String(localized: "Unassigned")
        case "": return String(localized: "Transactions")
        default: return store.budget?.row("categories", id: category)?.text("name") ?? String(localized: "Category")
        }
    }

    /// The export's file name, in English whatever the app's language, as file names stay.
    private var exportName: String {
        if let account { return "UrBudget \(account.text("name")).csv" }
        switch category {
        case NeedsCategory: return "UrBudget Uncategorized.csv"
        case Unassigned: return "UrBudget Unassigned.csv"
        case "": return "UrBudget Transactions.csv"
        default: return "UrBudget \(store.budget?.row("categories", id: category)?.text("name") ?? "Category").csv"
        }
    }

    /// The title, or how many are selected while selecting.
    private var screenTitle: String {
        selecting ? String(localized: "\(selected.count) selected", locale: .app) : title
    }

    var body: some View {
        list
            .navigationTitle(screenTitle)
            .searchable(text: $search, prompt: "Payee, category, memo, amount")
            .toolbar { toolbar }
            .tabBarHidden(selecting)
            .overlay(alignment: .bottomTrailing) { if !selecting { AddTransactionButton(account: target.account, compact: addCompact) } }
            .task(id: search) {
                // Typing settles for a moment before the list recomputes.
                if !search.isEmpty { try? await Task.sleep(for: .milliseconds(150)) }
                if !Task.isCancelled { query = search }
            }
            .task(id: LinesKey(version: store.version, filter: filter)) { await recompute() }
            .cover(item: $editing) { TransactionEditor(request: $0) }
            .navigationDestination(item: $jump) { TransactionsList(target: $0) }
            .sheet(item: Binding(get: { paying.map { IdentifiedText(value: $0) } }, set: { paying = $0?.value })) { CardPaymentSheet(accountId: $0.value) }
            .sheet(item: Binding(get: { loanFor.map { IdentifiedText(value: $0) } }, set: { loanFor = $0?.value })) { LoanEditor(accountId: $0.value) }
            .sheet(item: $editingSchedule) { ScheduleEditor(schedule: $0.value) }
            .sheet(isPresented: $importing) { BankFileImportView(account: target.account) }
            .sheet(item: $asking) { AssistantView(request: $0) }
            .sheet(item: $accountForm) { AccountForm(request: $0) }
            .sheet(item: Binding(get: { reconciling.map { IdentifiedJSON(value: $0) } }, set: { reconciling = $0?.value })) { ReconcileSheet(account: $0.value) }
            .sheet(isPresented: $bulkCategory) {
                NavigationStack {
                    CategoryPicker(title: String(localized: "Categorize \(selected.count)", locale: .app), month: Dates.currentMonth, selected: "",
                                   includeUnassigned: true, includePayment: false) { id in
                        bulkCategory = false
                        onSelection("category", id) { String(localized: "Categorized \($0) as \(categoryName(id)).") }
                    }
                    .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { bulkCategory = false } } }
                }
            }
            .sheet(item: Binding(get: { categorizing.map { IdentifiedText(value: $0) } }, set: { categorizing = $0?.value })) { item in
                NavigationStack {
                    CategoryPicker(title: String(localized: "Category"), month: Dates.currentMonth, selected: "", includeUnassigned: true, includePayment: false) { id in
                        categorizing = nil
                        bulk([item.value], "category", id, done: String(localized: "Categorized \(payee(item.value)) as \(categoryName(id))."))
                    }
                    .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { categorizing = nil } } }
                }
            }
            .sheet(item: $bulkText) { request in
                BulkTextSheet(request: request, count: selected.count) { value in
                    let ids = selected
                    try await store.committingWorkflow(bulkAction(ids, request.operation, value, target.account))
                    let changed = transactionCount(ids.count)
                    store.showToast(request.operation == "payee" ? String(localized: "Changed the payee of \(changed).")
                                    : String(localized: "Changed the memo of \(changed)."), undo: true)
                    endSelection()
                }
            }
            .sheet(isPresented: $pickingCategory) {
                NavigationStack {
                    CategoryPicker(title: String(localized: "register.showCategory", defaultValue: "Show category", comment: "Shows one category's transactions."), month: Dates.currentMonth, selected: category, includeUnassigned: true) { id in
                        category = id
                        pickingCategory = false
                    }
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) { Button("Cancel") { pickingCategory = false } }
                        if !category.isEmpty {
                            ToolbarItem(placement: .confirmationAction) { Button("All categories") { category = ""; pickingCategory = false } }
                        }
                    }
                }
            }
            .sheet(isPresented: $moreFilters) {
                RegisterFiltersSheet(minimum: $minimum, maximum: $maximum, from: $from, through: $through)
            }
            .confirmationDialog("Delete \(transactionCount(selected.count))?", isPresented: $confirmDelete, titleVisibility: .visible) {
                Button("Delete", role: .destructive) { onSelection("delete") { String(localized: "Deleted \($0).") } }
            } message: {
                Text("Both sides of transfers are removed too. You can undo this.")
            }
            .fileExporter(isPresented: $exporting, document: exportFile, contentType: .commaSeparatedText, defaultFilename: exportName) { _ in }
    }

    /// A bulk workflow on `ids`; once it's saved, `done` (if any) says what changed, with Undo.
    private func bulk(_ ids: Set<String>, _ operation: String, _ value: String = "", done: String? = nil) {
        store.workflow(bulkAction(ids, operation, value, target.account)) {
            if let done { store.showToast(done, undo: true) }
        }
    }

    /// A bulk workflow on the selection, which ends; `done` words the message from the count.
    private func onSelection(_ operation: String, _ value: String = "", done: (String) -> String) {
        let ids = selected
        endSelection()
        bulk(ids, operation, value, done: done(transactionCount(ids.count)))
    }

    private func payee(_ id: String) -> String {
        sections?.lazy.flatMap(\.lines).first { $0.id == id }?.payee ?? String(localized: "the transaction")
    }

    private func approveAll() {
        guard let ids = index?.awaitingApprovalIds(target.account), !ids.isEmpty else { return }
        bulk(ids, "approve", done: String(localized: "Approved \(transactionCount(ids.count))."))
    }

    private func recompute() async {
        guard let budget = store.budget else { return }
        let version = store.version
        let filter = filter
        let account = target.account
        let today = Dates.today
        // The index for this snapshot is kept while only the filter changes.
        let prepared = index != nil && indexVersion == version ? index! : await Task.detached(priority: .userInitiated) { RegisterIndex(budget: budget) }.value
        guard !Task.isCancelled else { return }
        index = prepared
        indexVersion = version
        let built = await Task.detached(priority: .userInitiated) {
            registerSections(prepared.matching(filter), order: filter.order, account: account, budget: budget, today: today)
        }.value
        if !Task.isCancelled { sections = built }
    }

    private var schedules: [JSON] {
        guard category.isEmpty, let budget = store.budget else { return [] }
        return budget.rows("schedules").filter { schedule in
            // A schedule with no next date has finished.
            let t = schedule["transaction"]
            return !schedule.text("nextDate").isEmpty && t.isObject
                && (target.account.isEmpty || t.text("accountId") == target.account || t.text("transferAccountId") == target.account)
        }
        .sorted { ($0.flag("paused") ? 1 : 0, $0.text("nextDate")) < ($1.flag("paused") ? 1 : 0, $1.text("nextDate")) }
    }

    private var activeFilters: Int {
        [status, flag, minimum, maximum].count { !$0.isEmpty } + (order != "newest" ? 1 : 0) + (from.isEmpty && through.isEmpty ? 0 : 1)
            + (category.isEmpty || category == target.category ? 0 : 1)
    }

    private var list: some View {
        let currency = store.currency
        let toApprove = index?.awaitingApproval(target.account) ?? 0
        let schedules = schedules
        return List {
            if let account, !selecting {
                Section {
                    AccountSummary(account: account, reconcile: { reconciling = account }, edit: { accountForm = AccountFormRequest(account: account) },
                                   pay: { paying = account.text("id") }, loan: { loanFor = account.text("id") }, importFile: { importing = true })
                }
            }
            if !from.isEmpty || !through.isEmpty || activeFilters > 0 {
                Section {
                    HStack {
                        Image(systemName: "line.3.horizontal.decrease.circle.fill").foregroundStyle(Color.urAccent)
                        Text(filterSummary).font(.subheadline).lineLimit(2)
                        Spacer()
                        Button("Clear") { clearFilters() }.font(.subheadline.weight(.semibold)).buttonStyle(.borderless)
                    }
                }
            }
            if toApprove > 0 && !selecting {
                Section {
                    Notice(text: String(localized: "\(toApprove) transactions to approve", locale: .app), tone: "info", icon: "sparkle",
                           action: status == "approval" ? nil : String(localized: "Review"),
                           secondary: .init(label: String(localized: "Approve all"), action: approveAll)) { status = "approval" }
                }
                .listRowInsets(EdgeInsets(top: 4, leading: 12, bottom: 4, trailing: 12))
                .listRowBackground(Color.clear)
            }
            if !schedules.isEmpty && query.isEmpty && !selecting {
                Section {
                    ForEach(showAllScheduled ? schedules : Array(schedules.prefix(3)), id: \.idText) { schedule in
                        ScheduleLine(schedule: schedule, account: target.account, currency: currency) { editingSchedule = IdentifiedJSON(value: schedule) }
                    }
                    if schedules.count > 3 {
                        Button(showAllScheduled ? "Show fewer" : "Show \(schedules.count - 3) more") { withAnimation { showAllScheduled.toggle() } }
                    }
                } header: {
                    Text("Scheduled").textCase(nil).font(.headline).foregroundStyle(Color.primary)
                }
            }
            if let sections {
                if sections.isEmpty || sections.allSatisfy(\.lines.isEmpty) {
                    Section {
                        if !query.isEmpty || activeFilters > 0 {
                            EmptyState(title: String(localized: "No matches"), detail: String(localized: "Nothing matches this search and these filters."),
                                       symbol: "magnifyingglass", action: String(localized: "Clear filters")) { search = ""; clearFilters() }
                        } else {
                            EmptyState(title: String(localized: "No transactions here"),
                                       detail: String(localized: "Add a purchase, income, or a transfer to see your money in motion."),
                                       symbol: "arrow.left.arrow.right",
                                       action: account?.flag("archived") == true ? nil : String(localized: "Add a transaction")) {
                                editing = TransactionEditorRequest(account: target.account)
                            }
                        }
                    }
                }
                ForEach(sections) { section in
                    Section {
                        ForEach(section.lines) { line in row(line) }
                    } header: {
                        if !section.label.isEmpty { Text(section.label).textCase(nil).font(.subheadline.weight(.semibold)).foregroundStyle(Color.primary) }
                    }
                }
            } else {
                ProgressView().frame(maxWidth: .infinity)
            }
            Color.clear.frame(height: 64).listRowSeparator(.hidden).listRowBackground(Color.clear)
        }
        .listStyle(.plain)
        .readableWidth()
        .shrinksAddButton($addCompact)
        .refreshable {
            store.foreground(force: true)
            await sync.syncAutomatically()
        }
    }

    private var filterSummary: String {
        var parts: [String] = []
        if !from.isEmpty || !through.isEmpty { parts.append(dateRangeLabel(from, through)) }
        switch status {
        case "approval": parts.append(String(localized: "Needs approval"))
        case "uncleared": parts.append(String(localized: "Uncleared"))
        case "cleared": parts.append(String(localized: "Cleared"))
        case "reconciled": parts.append(String(localized: "Reconciled"))
        case "notReconciled": parts.append(String(localized: "Not reconciled"))
        default: break
        }
        if flag == "__any" { parts.append(String(localized: "Flagged")) }
        else if !flag.isEmpty { parts.append(String(localized: "Flag: \(flagColorName(flag))")) }
        if category == NeedsCategory && target.category != NeedsCategory { parts.append(String(localized: "Needs a category")) }
        else if !category.isEmpty && category != target.category { parts.append(categoryName(category)) }
        if order != "newest" { parts.append(orderLabel) }
        if !minimum.isEmpty || !maximum.isEmpty { parts.append(amountRange) }
        return parts.joined(separator: " · ")
    }

    private var orderLabel: String {
        switch order {
        case "oldest": return String(localized: "Oldest first")
        case "amount": return String(localized: "By amount")
        case "payee": return String(localized: "By payee")
        default: return order
        }
    }

    private var amountRange: String {
        if minimum.isEmpty { return String(localized: "Up to \(maximum)") }
        if maximum.isEmpty { return String(localized: "At least \(minimum)") }
        return String(localized: "\(minimum) to \(maximum)")
    }

    private func categoryName(_ id: String) -> String {
        id == Unassigned ? String(localized: "Unassigned") : store.budget?.row("categories", id: id)?.text("name") ?? String(localized: "Category")
    }

    private func clearFilters() {
        status = ""; flag = ""; order = "newest"; from = ""; through = ""; category = target.category; minimum = ""; maximum = ""
    }

    private func row(_ line: TransactionLine) -> some View {
        let isSelected = selected.contains(line.id)
        return TransactionRow(line: line, selecting: selecting, isSelected: isSelected)
            .contentShape(Rectangle())
            .onTapGesture {
                if selecting {
                    if isSelected { selected.remove(line.id) } else { selected.insert(line.id) }
                } else if let transaction = store.budget?.row("transactions", id: line.id) {
                    editing = TransactionEditorRequest(transaction: transaction, account: target.account)
                }
            }
            .listRowBackground(isSelected ? Color.urAccent.opacity(0.12) : nil)
            .swipeActions(edge: .leading, allowsFullSwipe: true) {
                if line.cleared != "reconciled" && !selecting {
                    Button { toggleCleared(line) } label: {
                        Label(line.cleared == "cleared" ? "Uncleared" : "Cleared", systemImage: line.cleared == "cleared" ? "circle.dashed" : "checkmark.circle")
                    }
                    .tint(.urPositive)
                }
                if !line.approved && !selecting {
                    Button { bulk([line.id], "approve") } label: { Label("Approve", systemImage: "hand.thumbsup") }
                        .tint(.urAccent)
                }
                if line.chips.contains(where: \.attention) && !selecting {
                    Button { categorizing = line.id } label: { Label("Categorize", systemImage: "tag") }.tint(.urWarning)
                }
            }
            .swipeActions(edge: .trailing) {
                if !selecting {
                    Button(role: .destructive) { bulk([line.id], "delete", done: String(localized: "Deleted \(line.payee).")) } label: { Label("Delete", systemImage: "trash") }
                }
            }
            .contextMenu {
                if !selecting {
                    Button {
                        if let transaction = store.budget?.row("transactions", id: line.id) {
                            editing = TransactionEditorRequest(transaction: transaction, account: target.account)
                        }
                    } label: { Label("Edit", systemImage: "pencil") }
                    if line.cleared != "reconciled" {
                        Button { toggleCleared(line) } label: { Label(line.cleared == "cleared" ? "Mark uncleared" : "Mark cleared", systemImage: "checkmark.circle") }
                    }
                    Menu {
                        Button("No flag") { bulk([line.id], "flag") }
                        ForEach(flagChoices(store.budget ?? [:]), id: \.id) { choice in
                            Button(choice.name) { bulk([line.id], "flag", choice.id) }
                        }
                    } label: { Label("Flag", systemImage: "flag") }
                    if !line.transfer {
                        Button { categorizing = line.id } label: { Label("Categorize…", systemImage: "tag") }
                    }
                    if line.transfer, line.otherAccount != target.account, let other = store.budget?.row("accounts", id: line.otherAccount) {
                        Button { jump = RegisterTarget(account: line.otherAccount) } label: {
                            Label("Go to \(other.text("name"))", systemImage: "arrow.forward.circle")
                        }
                    }
                    Button { bulk([line.id], "duplicate", done: String(localized: "Duplicated \(line.payee).")) } label: {
                        Label("Duplicate", systemImage: "plus.square.on.square")
                    }
                    Button { asking = AssistantRequest(selected: [line.id]) } label: { Label("Ask the assistant", systemImage: "sparkles") }
                    Button {
                        selecting = true
                        selected = [line.id]
                    } label: { Label("Select", systemImage: "checkmark.circle") }
                    Divider()
                    Button(role: .destructive) { bulk([line.id], "delete", done: String(localized: "Deleted \(line.payee).")) } label: { Label("Delete", systemImage: "trash") }
                }
            }
    }

    private func toggleCleared(_ line: TransactionLine) {
        bulk([line.id], "status", line.cleared == "cleared" ? "uncleared" : "cleared")
    }

    private func endSelection() {
        selected = []
        selecting = false
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        if selecting {
            ToolbarItem(placement: .cancellationAction) { Button("Done") { endSelection() } }
            ToolbarItem(placement: .trailingBar) {
                let ids = Set(sections?.flatMap { $0.lines.map(\.id) } ?? [])
                Button(selected == ids ? "Select none" : "Select all") { selected = selected == ids ? [] : ids }
            }
            ToolbarItemGroup(placement: .bottomBarItems) {
                let lines = sections?.flatMap(\.lines).filter { selected.contains($0.id) } ?? []
                if lines.contains(where: { !$0.approved }) {
                    Button { onSelection("approve") { String(localized: "Approved \($0).") } } label: { Label("Approve", systemImage: "hand.thumbsup") }
                }
                Button { bulkCategory = true } label: { Label("Category", systemImage: "tag") }.disabled(selected.isEmpty)
                Spacer()
                Button { onSelection("status", "cleared") { String(localized: "Marked \($0) cleared.") } } label: { Label("Cleared", systemImage: "checkmark.circle") }
                    .disabled(selected.isEmpty)
                Spacer()
                Menu {
                    Button("No flag") { onSelection("flag") { String(localized: "Removed the flag from \($0).") } }
                    ForEach(flagChoices(store.budget ?? [:]), id: \.id) { choice in
                        Button(choice.name) { onSelection("flag", choice.id) { String(localized: "Flagged \($0) \(choice.name).") } }
                    }
                } label: { Label("Flag", systemImage: "flag") }
                .disabled(selected.isEmpty)
                Spacer()
                Menu {
                    Button { bulkText = BulkTextRequest(operation: "payee") } label: { Label("Change payee…", systemImage: "storefront") }
                    Button { bulkText = BulkTextRequest(operation: "memo") } label: { Label("Change memo…", systemImage: "text.alignleft") }
                    Button { onSelection("status", "uncleared") { String(localized: "Marked \($0) uncleared.") } } label: {
                        Label("Mark uncleared", systemImage: "circle.dashed")
                    }
                    Button { onSelection("duplicate") { String(localized: "Duplicated \($0).") } } label: {
                        Label("Duplicate", systemImage: "plus.square.on.square")
                    }
                    Button {
                        asking = AssistantRequest(selected: Array(selected))
                        endSelection()
                    } label: { Label("Ask the assistant", systemImage: "sparkles") }
                    Divider()
                    Button(role: .destructive) { confirmDelete = true } label: { Label("Delete", systemImage: "trash") }
                } label: { Label("More", systemImage: "ellipsis.circle") }
                .disabled(selected.isEmpty)
            }
        } else {
            ToolbarItem(placement: .trailingBar) {
                Button("Select") { selecting = true }.disabled(sections?.allSatisfy(\.lines.isEmpty) ?? true)
            }
            ToolbarItemGroup(placement: .trailingBar) {
                filterMenu
                BudgetActionsMenu()
            }
        }
    }

    private var filterMenu: some View {
        Menu {
            Picker("Status", selection: $status) {
                Text("Any status").tag("")
                Text("Needs approval").tag("approval")
                Text("Uncleared").tag("uncleared")
                Text("Cleared").tag("cleared")
                Text("Reconciled").tag("reconciled")
                Text("Not reconciled").tag("notReconciled")
            }
            .pickerStyle(.menu)
            Picker("Flag", selection: $flag) {
                Text("Any or no flag").tag("")
                Text("Flagged").tag("__any")
                Text("Not flagged").tag("__none")
                ForEach(flagChoices(store.budget ?? [:]), id: \.id) { Text($0.name).tag($0.id) }
            }
            .pickerStyle(.menu)
            Button { moreFilters = true } label: { Label("Amount and custom dates…", systemImage: "slider.horizontal.3") }
            Menu("Dates") {
                Button("Any date") { from = ""; through = "" }
                ForEach(datePresets, id: \.id) { preset in
                    Button(preset.label) { (from, through) = datePreset(preset.id) }
                }
            }
            if target.category.isEmpty {
                Button { pickingCategory = true } label: {
                    Label(category.isEmpty || category == NeedsCategory ? "Category…" : "Category: \(categoryName(category))", systemImage: "tag")
                }
                if target.account.isEmpty {
                    Toggle("Needs a category", isOn: Binding(get: { category == NeedsCategory }, set: { category = $0 ? NeedsCategory : "" }))
                }
            }
            Picker("Sort", selection: $order) {
                Text("Newest first").tag("newest")
                Text("Oldest first").tag("oldest")
                Text("Amount").tag("amount")
                Text("Payee").tag("payee")
            }
            .pickerStyle(.menu)
            Divider()
            Button { export() } label: {
                Label(filtered ? "Export these transactions" : "Export all transactions", systemImage: "square.and.arrow.up")
            }
            .disabled(sections?.allSatisfy(\.lines.isEmpty) ?? true)
        } label: {
            Image(systemName: activeFilters > 0 ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle")
        }
        .accessibilityLabel("Filters and actions")
    }

    /// Whether the list shows less than the whole register, so an export takes just what it shows.
    private var filtered: Bool { !target.account.isEmpty || !category.isEmpty || !query.isEmpty || activeFilters > 0 }

    private func export() {
        var params: JSON = [:]
        if filtered { params["ids"] = JSON(Array(Set(sections?.flatMap { $0.lines.map(\.id) } ?? []))) }
        if !target.account.isEmpty { params["accountId"] = .string(target.account) }
        Task {
            do {
                let csv = try await store.query("exportRegister", params)
                exportFile = TextFile(data: Data(csv.string.utf8))
                exporting = true
            } catch {
                store.error = BudgetStore.readable(error)
            }
        }
    }
}

/// A JSON record that sheets can present by its id.
struct IdentifiedJSON: Identifiable {
    var value: JSON
    var id: String { value.text("id") }
}

private struct TransactionRow: View {
    var line: TransactionLine
    var selecting: Bool
    var isSelected: Bool

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            if selecting {
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .foregroundStyle(isSelected ? Color.urAccent : .secondary)
            }
            VStack(alignment: .leading, spacing: 5) {
                HStack(spacing: 6) {
                    if line.transfer {
                        Image(systemName: line.card ? "creditcard" : "arrow.left.arrow.right")
                            .font(.caption.weight(.bold)).foregroundStyle(Color.urAccent)
                            .frame(width: 22, height: 22)
                            .background(Color.urAccent.opacity(0.14), in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                    }
                    if !line.flag.isEmpty { FlagDot(color: line.flag) }
                    Text(line.payee).font(.body.weight(.semibold)).lineLimit(1)
                        .foregroundStyle(line.approved ? .primary : Color.urAccent)
                    Spacer(minLength: 8)
                    Text(line.amountText).figure(17, weight: .semibold).lineLimit(1)
                        .foregroundStyle(line.amount > 0 ? Color.urPositive : .primary)
                }
                HStack(spacing: 6) {
                    ForEach(line.chips, id: \.self) { TagChip(text: $0.text, attention: $0.attention) }
                    Spacer(minLength: 4)
                    if !line.accountName.isEmpty {
                        Text(line.accountName).font(.subheadline).foregroundStyle(Color.secondary).lineLimit(1)
                    }
                    if !line.approved && !selecting {
                        Image(systemName: "sparkle").font(.caption.weight(.bold)).foregroundStyle(Color.urAccent).accessibilityLabel("To approve")
                    }
                }
                if !line.memo.isEmpty { Text(line.memo).font(.subheadline).foregroundStyle(Color.secondary).lineLimit(1) }
            }
            ClearedMark(status: line.cleared)
        }
        .padding(.vertical, 3)
        .opacity(line.future ? 0.6 : 1)
        .accessibilityElement(children: .combine)
    }
}

/// A scheduled transaction: payee and amount, category and next date, Enter and Skip.
private struct ScheduleLine: View {
    @Environment(BudgetStore.self) private var store
    var schedule: JSON
    var account: String
    var currency: String
    var edit: () -> Void
    @State private var confirmDelete = false

    var body: some View {
        let t = schedule["transaction"]
        let amount = !account.isEmpty && t.text("transferAccountId") == account ? -t.long("amount") : t.long("amount")
        let paused = schedule.flag("paused")
        let today = Dates.today
        let next = schedule.text("nextDate")
        let due = !paused && next <= today
        let categoryName = categoryLabel(t.rows("splits"))
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Image(systemName: "repeat").foregroundStyle(Color.secondary)
                Text(title(t))
                    .font(.body.weight(.semibold)).foregroundStyle(Color.secondary).lineLimit(1)
                Spacer()
                Text(AppLanguage.leftToRight((amount > 0 ? "+" : "") + money(amount, currency))).figure(17, weight: .semibold)
                    .foregroundStyle(amount > 0 ? Color.urPositive : .secondary)
            }
            HStack(spacing: 8) {
                if let categoryName { TagChip(text: categoryName) }
                Text(paused ? String(localized: "Paused") : when(next, today: today, due: due))
                    .font(.subheadline).foregroundStyle(due ? Color.urWarning : .secondary)
                Spacer()
                if !paused {
                    Button("Enter") {
                        store.runCommand("applySchedules", ["through": .string(today), "selected": .string(schedule.text("id"))]) {
                            store.showToast(String(localized: "Entered \(name)."), undo: true)
                        }
                    }
                    .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small).tint(.urAccent)
                    Button("Skip") {
                        store.runCommand("applySchedules", ["through": .string(today), "selected": .string(schedule.text("id")), "skip": true]) {
                            store.showToast(String(localized: "Skipped \(name) on \(Money.date(next))."), undo: true)
                        }
                    }
                    .buttonStyle(.bordered).buttonBorderShape(.capsule).controlSize(.small)
                }
            }
        }
        .opacity(paused ? 0.55 : 1)
        .contentShape(Rectangle())
        .onTapGesture(perform: edit)
        .contextMenu {
            Button(action: edit) { Label("Edit", systemImage: "pencil") }
            Button { store.upsert("schedules", ["id": .string(schedule.text("id")), "paused": .bool(!paused)]) } label: {
                Label(paused ? "Resume" : "Pause", systemImage: paused ? "play" : "pause")
            }
            Button(role: .destructive) { confirmDelete = true } label: { Label("Delete", systemImage: "trash") }
        }
        .confirmationDialog("Delete the \(name) schedule?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete schedule", role: .destructive) {
                store.remove("schedules", id: schedule.text("id")) { store.showToast(String(localized: "Deleted the \(name) schedule."), undo: true) }
            }
        } message: {
            Text("Transactions it already entered stay.")
        }
    }

    /// The payee, else the account a transfer goes to, else "Scheduled".
    private func title(_ t: JSON) -> String {
        if !t.text("payee").isEmpty { return t.text("payee") }
        guard let transferName = store.budget?.row("accounts", id: t.text("transferAccountId"))?.text("name") else { return String(localized: "Scheduled") }
        return String(localized: "Transfer: \(transferName)")
    }

    /// "Split", "To Unassigned" or the category's name, for the chip.
    private func categoryLabel(_ splits: [JSON]) -> String? {
        if splits.count > 1 { return String(localized: "category.split", defaultValue: "Split", comment: "In place of a category: the transaction is split.") }
        if splits.first?.flag("unassigned") == true { return String(localized: "To Unassigned") }
        return splits.first.flatMap { store.budget?.row("categories", id: $0.text("categoryId"))?.text("name") }
    }

    /// "Due today", "Tomorrow", or the date, as a reminders list words it.
    private func when(_ next: String, today: String, due: Bool) -> String {
        if next == today { return String(localized: "Due today") }
        if due { return String(localized: "Due \(Money.date(next))") }
        return next == Dates.addDays(today, 1) ? String(localized: "Tomorrow") : Money.date(next)
    }

    private var name: String {
        let t = schedule["transaction"]
        if !t.text("payee").isEmpty { return t.text("payee") }
        guard let account = store.budget?.row("accounts", id: t.text("transferAccountId"))?.text("name") else {
            return String(localized: "scheduled transaction")
        }
        return String(localized: "Transfer: \(account)")
    }
}

/// An account's balances above its transactions: working balance, cleared and uncleared, and its actions.
private struct AccountSummary: View {
    @Environment(BudgetStore.self) private var store
    var account: JSON
    var reconcile: () -> Void
    var edit: () -> Void
    var pay: () -> Void
    var loan: () -> Void
    var importFile: () -> Void

    var body: some View {
        let currency = store.currency
        let total = store.summary?.rows("accounts").first { $0.text("id") == account.text("id") }
        let balance = total?.long("balance") ?? 0
        VStack(alignment: .leading, spacing: 12) {
            Text(AccountTypes.typeLabel(account)).font(.subheadline).foregroundStyle(Color.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text("Working balance").font(.footnote).foregroundStyle(Color.secondary)
                Text(money(balance, currency)).font(.system(size: 30, weight: .heavy, design: .rounded)).monospacedDigit()
                    .foregroundStyle(balance < 0 ? Color.urNegative : .primary)
            }
            HStack(spacing: 24) {
                Figure(label: String(localized: "Cleared"), value: money(total?.long("cleared") ?? 0, currency))
                Figure(label: String(localized: "Uncleared"), value: money(total?.long("uncleared") ?? 0, currency))
            }
            if account.text("kind") == "credit" && !account.flag("archived") {
                Button(action: pay) { Label("Make a payment", systemImage: "creditcard").frame(maxWidth: .infinity) }
                    .buttonStyle(.borderedProminent)
            }
            if hasLoan(balance) {
                Button(action: loan) { Label("Loan details & payoff", systemImage: "percent").frame(maxWidth: .infinity) }
                    .buttonStyle(.bordered)
            }
            HStack(spacing: 8) {
                Button(action: reconcile) { Label("Reconcile", systemImage: "lock").frame(maxWidth: .infinity) }
                    .disabled(total == nil || account.flag("archived"))
                Button(action: importFile) { Label("Import", systemImage: "square.and.arrow.down").frame(maxWidth: .infinity) }
                    .disabled(account.flag("archived"))
                Button(action: edit) { Label("Edit", systemImage: "pencil").frame(maxWidth: .infinity) }
                    .accessibilityLabel("Edit account")
            }
            .labelStyle(StackedLabelStyle())
            .buttonStyle(.bordered)
        }
        .padding(.vertical, 6)
    }

    /// A tracking debt, or any account with loan details saved (Planning.tsx's hasLoan).
    private func hasLoan(_ balance: Int64) -> Bool {
        let loans = Set(store.budget?.rows("loans").map { $0.text("accountId") } ?? [])
        let type = AccountTypes.infer(account, balance: balance)
        return loans.contains(account.text("id")) || account.text("kind") == "tracking" && AccountTypes.isLiability(type)
    }
}

/// A payee or memo for every selected transaction.
private struct BulkTextRequest: Identifiable {
    var operation: String
    var id: String { operation }
}

private struct BulkTextSheet: View {
    var request: BulkTextRequest
    var count: Int
    var apply: (String) async throws -> Void
    @State private var value = ""

    var body: some View {
        FormSheet(title: request.operation == "payee" ? String(localized: "Change payee") : String(localized: "Change memo"),
                  confirm: String(localized: "Apply"),
                  enabled: request.operation == "memo" || !value.trimmingCharacters(in: .whitespaces).isEmpty) {
            try await apply(value.trimmingCharacters(in: .whitespaces))
        } content: {
            Section {
                TextField(request.operation == "payee" ? "Payee" : "Memo", text: $value)
            } footer: {
                // One selected transaction reads "Applies to the selected transaction. …" (the singular form).
                Text("Applies to all \(count) selected transactions. You can undo it.")
            }
        }
        .presentationDetents([.medium])
    }
}

/// Filters a menu can't hold: an amount range (outflows and inflows alike) and a custom date range.
private struct RegisterFiltersSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Binding var minimum: String
    @Binding var maximum: String
    @Binding var from: String
    @Binding var through: String
    @State private var custom = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    AmountField(title: String(localized: "At least"), text: $minimum)
                    AmountField(title: String(localized: "At most"), text: $maximum)
                } header: {
                    Text("Amount")
                } footer: {
                    Text("Amounts match outflows and inflows alike.")
                }
                Section("Dates") {
                    Toggle("Custom range", isOn: $custom.animation())
                    if custom {
                        DayPicker(title: String(localized: "From"), iso: Binding(get: { from.isEmpty ? Dates.addDays(Dates.today, -30) : from }, set: { from = $0 }))
                        DayPicker(title: String(localized: "Through"), iso: Binding(get: { through.isEmpty ? Dates.today : through }, set: { through = $0 }))
                    }
                }
                Section {
                    Button("Clear these filters", role: .destructive) {
                        minimum = ""; maximum = ""; from = ""; through = ""; custom = false
                    }
                }
            }
            .navigationTitle("Filters")
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { finish() } } }
        }
        .presentationDetents([.medium, .large])
        .onAppear { custom = !from.isEmpty || !through.isEmpty }
    }

    private func finish() {
        if custom {
            if from.isEmpty { from = Dates.addDays(Dates.today, -30) }
            if through.isEmpty { through = Dates.today }
            if from > through { (from, through) = (through, from) }
        }
        dismiss()
    }
}

/// An icon over a short title, for a row of equal buttons.
struct StackedLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        VStack(spacing: 3) {
            configuration.icon.font(.body)
            configuration.title.font(.caption.weight(.semibold)).lineLimit(1).minimumScaleFactor(0.8)
        }
        .padding(.vertical, 2)
    }
}
