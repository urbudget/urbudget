import SwiftUI

/// Quick assign's options, grouped as the desktop groups them. `group` identifies the group (in
/// English); `quickAssignGroupName` names it.
struct QuickMode: Identifiable, Hashable {
    var id: String
    var label: String
    var group: String
    var hint: String
}

let quickAssignModes: [QuickMode] = { () -> [QuickMode] in
    var modes: [QuickMode] = []
    modes.append(QuickMode(id: "underfunded", label: String(localized: "Underfunded"), group: "Fund targets",
                           hint: String(localized: "Assign what targets still need and cover overspending.")))
    modes.append(QuickMode(id: "assignedLastMonth", label: String(localized: "Assigned last month"), group: "Repeat past months",
                           hint: String(localized: "Assign what each category got last month.")))
    modes.append(QuickMode(id: "spentLastMonth", label: String(localized: "Spent last month"), group: "Repeat past months",
                           hint: String(localized: "Assign what each category spent last month.")))
    modes.append(QuickMode(id: "averageAssigned", label: String(localized: "Average assigned"), group: "Repeat past months",
                           hint: String(localized: "Assign each category's 12-month average assignment.")))
    modes.append(QuickMode(id: "averageSpent", label: String(localized: "Average spent"), group: "Repeat past months",
                           hint: String(localized: "Assign each category's 12-month average spending.")))
    modes.append(QuickMode(id: "reduceOverfunding", label: String(localized: "Reduce overfunding"), group: "Clean up",
                           hint: String(localized: "Return what's beyond each target to Unassigned.")))
    modes.append(QuickMode(id: "resetAvailable", label: String(localized: "Reset available amounts"), group: "Clean up",
                           hint: String(localized: "Bring every Available to 0.00.")))
    modes.append(QuickMode(id: "resetAssigned", label: String(localized: "Reset assigned amounts"), group: "Clean up",
                           hint: String(localized: "Set this month's assignments to 0.00.")))
    return modes
}()

/// The groups of Quick assign options, in order.
let quickAssignGroups = ["Fund targets", "Repeat past months", "Clean up"]

/// A Quick assign group's name in the app's language.
func quickAssignGroupName(_ group: String) -> String {
    switch group {
    case "Fund targets": return String(localized: "Fund targets")
    case "Repeat past months": return String(localized: "Repeat past months")
    case "Clean up": return String(localized: "Clean up")
    default: return group
    }
}

/// Unassigned's sheet: the month in figures, how far this month's targets are funded, targets against
/// expected income, ways to give the money jobs (Quick assign with what each option would do, or one
/// category), and the month's recent changes.
struct UnassignedSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var assignTo: () -> Void
    var editIncome: () -> Void
    @State private var outcomes: [String: Int64] = [:]
    @State private var allChanges = false
    /// Cash age in days (nil while it's worked out, -1 when there isn't one yet).
    @State private var cashAge: Int64?

    var body: some View {
        let currency = store.currency
        let month = store.month
        let summary = store.summary ?? [:]
        let budget = store.budget ?? [:]
        let rta = summary.long("readyToAssign")
        var totals: [String: JSON] = [:]
        for row in summary.rows("categories") { totals[row.text("id")] = row }
        let visible = budget.rows("categories").filter { !$0.flag("hidden") }
        let needed = visible.reduce(Int64(0)) { $0 + CategoryStatus(category: $1, summary: totals[$1.text("id")], month: month).needed }
        let targets = visible.reduce(Int64(0)) { $0 + (totals[$1.text("id")]?.long("monthTarget") ?? 0) }
        let carried = visible.reduce(Int64(0)) { $0 + (totals[$1.text("id")]?.long("carried") ?? 0) }
        let income: Int64? = budget["expectedIncome"].isNull ? nil : budget.long("expectedIncome")
        let changes = Array(budget.rows("moneyMoves").filter { $0.text("month") == month }.reversed())
        let note = budget.rows("ynabMonths").first { $0.text("month").hasPrefix(month) }?.text("note") ?? ""
        return NavigationStack {
            List {
                Section {
                    VStack(spacing: 4) {
                        Text(money(rta, currency)).font(.system(size: 38, weight: .heavy, design: .rounded)).monospacedDigit()
                            .foregroundStyle(rta > 0 ? Color.urPositive : rta < 0 ? Color.urNegative : .primary)
                        Text(rta > 0 ? "Unassigned: ready for a job" : rta < 0 ? "More is assigned than you have" : "All of it is assigned")
                            .foregroundStyle(Color.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
                }
                Section {
                    if needed > 0 {
                        Notice(text: String(localized: "\(money(needed, currency)) still needed this month"), tone: "attention", icon: "target")
                    } else {
                        Notice(text: String(localized: "This month's funding is complete"), tone: "positive", icon: "checkmark.circle.fill")
                    }
                    Button(action: assignTo) {
                        Label(rta < 0 ? "Take money back" : "Assign to a category", systemImage: "arrow.left.arrow.right")
                            .fontWeight(.semibold).frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets(top: 4, leading: 0, bottom: 4, trailing: 0))
                figuresSection(summary, carried: carried, currency: currency)
                targetsSection(month, targets: targets, income: income, note: note, currency: currency)
                ForEach(quickAssignGroups, id: \.self) { group in
                    Section(groupTitle(group)) {
                        ForEach(quickAssignModes.filter { $0.group == group }) { mode in
                            QuickModeRow(mode: mode, outcome: outcomes[mode.id], currency: currency)
                        }
                    }
                }
                if !changes.isEmpty { changesSection(changes) }
            }
            .groupedList()
            .navigationTitle(Dates.monthLabel(month))
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } } }
            .navigationDestination(for: QuickMode.self) { QuickAssignPreview(mode: $0) { dismiss() } }
        }
        .presentationDetents([.large])
        .task(id: store.version) { await quickAssignOutcomes(store, into: $outcomes) }
        .task(id: "\(store.version):\(store.month)") {
            // As the desktop's month summary works it out: the reports engine for this month alone.
            let month = store.month
            let report = try? await store.query("reports", ["from": .string(month), "to": .string(month), "accountIds": [], "categoryIds": []])
            guard !Task.isCancelled else { return }
            cashAge = report.map { $0["ageOfMoney"].isNull ? -1 : $0.long("ageOfMoney") } ?? -1
        }
    }

    // MARK: Sections

    private func figuresSection(_ summary: JSON, carried: Int64, currency: String) -> some View {
        // The month's activity and available include spending that still needs a category.
        let activity = summary.long("activity") + summary.long("uncategorized")
        let available = summary.long("available") + summary.long("uncategorized")
        return Section {
            ValueRow(title: String(localized: "Carried over"), value: money(carried, currency))
            ValueRow(title: String(localized: "Income this month"), value: money(summary.long("income"), currency))
            ValueRow(title: String(localized: "Assigned"), value: money(summary.long("assigned"), currency))
            ValueRow(title: String(localized: "Activity"), value: money(activity, currency))
            ValueRow(title: String(localized: "Available"), value: money(available, currency), color: available < 0 ? .urNegative : .primary)
            ValueRow(title: String(localized: "Cash age"), value: cashAgeText, subtitle: String(localized: "How long money waits before you spend it"))
        }
    }

    private func targetsSection(_ month: String, targets: Int64, income: Int64?, note: String, currency: String) -> some View {
        let expected = income.map { money($0, currency) } ?? String(localized: "Add")
        let comparison = incomeComparison(income: income, targets: targets, currency: currency)
        return Section("Targets & income") {
            ValueRow(title: String(localized: "\(Dates.shortMonthLabel(month)) targets"), value: money(targets, currency))
            Button(action: editIncome) {
                ValueRow(title: String(localized: "Expected income"), value: expected, subtitle: comparison, color: income == nil ? .urAccent : .primary)
                    .foregroundStyle(Color.primary)
            }
            Button(action: editIncome) {
                HStack(alignment: .firstTextBaseline) {
                    Text("Month note").foregroundStyle(Color.primary)
                    Spacer(minLength: 16)
                    Text(note.isEmpty ? String(localized: "Add") : note).lineLimit(2).multilineTextAlignment(.trailing)
                        .foregroundStyle(note.isEmpty ? Color.urAccent : Color.secondary)
                }
            }
        }
    }

    private func changesSection(_ changes: [JSON]) -> some View {
        Section("Recent changes") {
            ForEach(Array((allChanges ? changes : Array(changes.prefix(4))).enumerated()), id: \.offset) { _, change in
                ValueRow(title: change.text("description").isEmpty ? String(localized: "Change") : change.text("description"), value: "",
                         subtitle: Dates.dayAndMonth(change.text("at")))
            }
            if changes.count > 4 && !allChanges { Button("Show all \(changes.count)") { allChanges = true } }
        }
    }

    // MARK: Words

    /// Cash age in days, "—" when there's none yet.
    private var cashAgeText: String {
        guard let cashAge else { return "…" }
        return cashAge < 0 ? "—" : String(localized: "\(Int(cashAge)) days", locale: .app)
    }

    /// Expected income against this month's targets.
    private func incomeComparison(income: Int64?, targets: Int64, currency: String) -> String {
        guard let income else { return String(localized: "Compare your targets with what you expect to earn") }
        if targets > income { return String(localized: "Targets are \(money(targets - income, currency)) more than expected income") }
        return String(localized: "\(money(income - targets, currency)) left after targets")
    }

    /// The first group carries the Quick assign name, as the desktop's sheet heads it.
    private func groupTitle(_ group: String) -> String {
        let name = quickAssignGroupName(group)
        return group == "Fund targets" ? String(localized: "Quick assign · \(name)") : name
    }
}

/// What each Quick assign option would change in total (for `categories`, or every visible one),
/// worked out while a sheet is open.
@MainActor func quickAssignOutcomes(_ store: BudgetStore, categories: [String]? = nil, into outcomes: Binding<[String: Int64]>) async {
    guard let budget = store.budget, let summary = store.summary else { return }
    let month = store.month
    let before = summary.long("assigned")
    for mode in quickAssignModes {
        var params = quickAssignParams(budget, month: month, mode: mode.id, categories: categories)
        params["previewMonth"] = .string(month)
        guard let result = try? await store.query("workflow", params), !Task.isCancelled else { continue }
        outcomes.wrappedValue[mode.id] = result["summary"].long("assigned") - before
    }
}

/// A Quick assign option with what it would change; it opens its preview unless it changes nothing.
private struct QuickModeRow: View {
    var mode: QuickMode
    var outcome: Int64?
    var currency: String

    var body: some View {
        let label = VStack(alignment: .leading, spacing: 2) {
            Text(mode.label).foregroundStyle(outcome == 0 ? Color.secondary : Color.primary)
            Text(mode.hint).font(.footnote).foregroundStyle(Color.secondary)
        }
        let value = Text(outcomeText)
            .monospacedDigit()
            .foregroundStyle(outcome == nil || outcome == 0 ? Color.secondary : outcome! > 0 ? Color.urPositive : Color.urNegative)
        if outcome == 0 {
            HStack { label; Spacer(); value }
        } else {
            NavigationLink(value: mode) { HStack { label; Spacer(); value } }
        }
    }

    /// "+45.00", "No change", or "…" while it's worked out.
    private var outcomeText: String {
        guard let outcome else { return "…" }
        if outcome == 0 { return String(localized: "No change") }
        return AppLanguage.leftToRight((outcome > 0 ? "+" : "") + money(outcome, currency))
    }
}

/// Quick assign for one group's categories, from the group's menu on Budget.
struct GroupQuickAssignSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var group: String
    @State private var outcomes: [String: Int64] = [:]

    private var ids: [String] {
        (store.budget?.rows("categories") ?? []).filter { !$0.flag("hidden") && $0.text("group") == group }.map { $0.text("id") }
    }

    var body: some View {
        let ids = ids
        NavigationStack {
            List {
                ForEach(quickAssignGroups, id: \.self) { section in
                    Section(quickAssignGroupName(section)) {
                        ForEach(quickAssignModes.filter { $0.group == section }) { mode in
                            QuickModeRow(mode: mode, outcome: outcomes[mode.id], currency: store.currency)
                        }
                    }
                }
            }
            .groupedList()
            .navigationTitle("Quick assign · \(group)")
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Close") { dismiss() } } }
            .navigationDestination(for: QuickMode.self) { QuickAssignPreview(mode: $0, categories: ids, scope: group) { dismiss() } }
        }
        .presentationDetents([.large])
        .task(id: store.version) { await quickAssignOutcomes(store, categories: ids, into: $outcomes) }
    }
}

/// One category's change in a Quick assign preview.
private struct QuickRow: Identifiable {
    var id: String
    var name: String
    var before: Int64
    var after: Int64
    var availableAfter: Int64
}

/// A Quick assign option explained with the budget's own numbers before applying it: what it assigns
/// and returns, Unassigned before and after, each category's change, and warnings when it would
/// over-assign or leave categories overspent.
private struct QuickAssignPreview: View {
    @Environment(BudgetStore.self) private var store
    var mode: QuickMode
    /// The categories it works on (one group's) and their name, or every visible category.
    var categories: [String]? = nil
    var scope: String? = nil
    var done: () -> Void
    @State private var preview: JSON?
    @State private var failed: String?
    @State private var saving = false

    var body: some View {
        let currency = store.currency
        let month = store.month
        let summary = store.summary ?? [:]
        let visible = categories?.count ?? (store.budget?.rows("categories") ?? []).count { !$0.flag("hidden") }
        let rows = preview.map(changedRows) ?? []
        let assigns = rows.filter { $0.after > $0.before }.reduce(Int64(0)) { $0 + $1.after - $1.before }
        let returns = rows.filter { $0.after < $0.before }.reduce(Int64(0)) { $0 + $1.before - $1.after }
        let after = preview?["summary"]
        return List {
            Section {
                Text(explain(mode.id, month)).foregroundStyle(Color.secondary)
            } footer: {
                Text(scopeText(visible: visible, month: month))
            }
            if let failed { FormErrorView(message: failed) }
            if preview == nil && failed == nil { ProgressView().frame(maxWidth: .infinity) }
            if let after {
                figuresSection(after, summary: summary, assigns: assigns, returns: returns, changed: rows.count, visible: visible, currency: currency)
                if after.long("readyToAssign") < 0 {
                    Notice(text: String(localized: "This assigns more than you have: Unassigned would be \(money(after.long("readyToAssign"), currency))."),
                           tone: "negative", icon: "exclamationmark.triangle.fill")
                }
                let overspent = after.rows("categories").count { $0.long("available") < 0 }
                if overspent > 0 {
                    Notice(text: String(localized: "\(overspent) categories stay overspent.", locale: .app), tone: "attention")
                }
                if rows.isEmpty {
                    Notice(text: String(localized: "Nothing to change for \(Dates.monthLabel(month))."), tone: "info")
                } else {
                    rowsSection(rows, currency: currency)
                }
            }
        }
        .groupedList()
        .navigationTitle(mode.label)
        .inlineTitle()
        .safeAreaInset(edge: .bottom) {
            Button(action: apply) {
                HStack(spacing: 8) {
                    if saving { ProgressView().tint(.white) }
                    applyLabel(rows.count)
                }
                .fontWeight(.semibold).frame(maxWidth: .infinity, minHeight: 34)
            }
            .buttonStyle(.borderedProminent)
            .tint(["resetAvailable", "resetAssigned"].contains(mode.id) ? .urNegative : .urAccent)
            .disabled(rows.isEmpty || saving)
            .padding()
            .background(.bar)
        }
        .task(id: store.version) {
            guard let budget = store.budget else { return }
            var params = quickAssignParams(budget, month: month, mode: mode.id, categories: categories)
            params["previewMonth"] = .string(month)
            do { preview = try await store.query("workflow", params) } catch { failed = BudgetStore.readable(error) }
        }
    }

    // MARK: Parts

    private func figuresSection(_ after: JSON, summary: JSON, assigns: Int64, returns: Int64, changed: Int, visible: Int,
                                currency: String) -> some View {
        let returning = returns > assigns
        let net = money(returning ? returns - assigns : assigns - returns, currency)
        let unassigned = AppLanguage.leftToRight("\(money(summary.long("readyToAssign"), currency)) → \(money(after.long("readyToAssign"), currency))")
        return Section {
            HStack(alignment: .top) {
                Figure(label: returning ? String(localized: "Returns") : String(localized: "Assigns"), value: net,
                       color: returning ? .urNegative : .urPositive)
                Spacer()
                Figure(label: String(localized: "Unassigned"), value: unassigned,
                       color: after.long("readyToAssign") < 0 ? .urNegative : .primary, alignment: .trailing)
            }
        } footer: {
            Text("\(changed) of \(visible) categories change.")
        }
    }

    private func rowsSection(_ rows: [QuickRow], currency: String) -> some View {
        Section {
            ForEach(rows) { row in
                let change = row.after - row.before
                ValueRow(title: row.name, value: AppLanguage.leftToRight((change > 0 ? "+" : "") + money(change, currency)),
                         subtitle: rowDetail(row, currency: currency), color: change > 0 ? .urPositive : .urNegative)
            }
        }
    }

    /// "120.00 → 150.00 · Available 30.00".
    private func rowDetail(_ row: QuickRow, currency: String) -> String {
        let change = AppLanguage.leftToRight("\(money(row.before, currency)) → \(money(row.after, currency))")
        return String(localized: "\(change) · Available \(money(row.availableAfter, currency))")
    }

    /// Which categories the option works on, under its explanation.
    private func scopeText(visible: Int, month: String) -> String {
        let label = Dates.monthLabel(month)
        guard let scope else { return String(localized: "All \(visible) visible categories in \(label).", locale: .app) }
        return String(localized: "The \(visible) categories in \(scope), for \(label).", locale: .app)
    }

    private func applyLabel(_ count: Int) -> Text {
        count == 0 ? Text("Nothing to apply") : Text("Apply to \(count) categories")
    }

    private func changedRows(_ result: JSON) -> [QuickRow] {
        guard let budget = store.budget else { return [] }
        var before: [String: JSON] = [:]
        for row in store.summary?.rows("categories") ?? [] { before[row.text("id")] = row }
        var after: [String: JSON] = [:]
        for row in result["summary"].rows("categories") { after[row.text("id")] = row }
        return orderedGroups(budget).flatMap(\.categories).compactMap { c in
            let id = c.text("id")
            let b = before[id]?.long("assigned") ?? 0
            let a = after[id]?.long("assigned") ?? b
            return a == b ? nil : QuickRow(id: id, name: c.text("name"), before: b, after: a, availableAfter: after[id]?.long("available") ?? 0)
        }
    }

    private func apply() {
        guard let budget = store.budget else { return }
        let params = quickAssignParams(budget, month: store.month, mode: mode.id, categories: categories)
        let rows = preview.map(changedRows) ?? []
        let change = rows.reduce(Int64(0)) { $0 + $1.after - $1.before }
        attempt($failed, saving: $saving) {
            try await store.committing("workflow", params)
            let amount = money(Int64(change.magnitude), store.currency)
            store.showToast(change >= 0 ? String(localized: "\(mode.label): assigned \(amount).")
                                        : String(localized: "\(mode.label): returned \(amount)."), undo: true)
            done()
        }
    }

    /// What an option does, in words, for this month (desktop QuickAssign.tsx explain).
    private func explain(_ mode: String, _ month: String) -> String {
        let now = Dates.monthName(month)
        let last = Dates.monthName(Dates.addMonths(month, -1))
        switch mode {
        case "underfunded":
            return String(localized: "Assigns what each category still needs for its \(now) target, and covers overspending. It never assigns more than Unassigned holds: overspent categories go first, then the targets due soonest, then credit card payments.")
        case "assignedLastMonth":
            return String(localized: "Replaces each category's \(now) assignment with what you assigned to it in \(last).")
        case "spentLastMonth":
            return String(localized: "Replaces each category's \(now) assignment with what it spent in \(last). Categories that spent nothing get 0.00, and credit card payments stay as they are.")
        case "averageAssigned":
            return String(localized: "Replaces each category's \(now) assignment with its average monthly assignment over the last 12 months.")
        case "averageSpent":
            return String(localized: "Replaces each category's \(now) assignment with its average monthly spending over the last 12 months. Credit card payments stay as they are.")
        case "reduceOverfunding":
            return String(localized: "Lowers the \(now) assignment of categories holding more than their target needs, and returns the extra to Unassigned.")
        case "resetAvailable":
            return String(localized: "Changes each category's \(now) assignment so its Available becomes 0.00. Money set aside, including what carried over from \(last), returns to Unassigned.")
        case "resetAssigned":
            return String(localized: "Sets each category's \(now) assignment to 0.00. Carried-over money and spending stay, so each Available drops by what was assigned.")
        default: return ""
        }
    }
}
