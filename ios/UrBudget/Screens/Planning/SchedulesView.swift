import SwiftUI

/// A schedule as its row shows it.
private struct ScheduleItem: Identifiable {
    var id: String
    var schedule: JSON
    var title: String
    var subtitle: String
    var amount: Int64
    var due: Bool
    var paused: Bool
    var once: Bool
    var next: String
}

/// The Scheduled page's rows and figures.
private struct ScheduleListModel {
    var due: [ScheduleItem]
    var coming: [ScheduleItem]
    var paused: [ScheduleItem]
    var finished: [ScheduleItem]
    var summary: ScheduleSummary

    var isEmpty: Bool { due.isEmpty && coming.isEmpty && paused.isEmpty && finished.isEmpty }

    init(budget: JSON, today: String) {
        var accounts: [String: String] = [:]
        for a in budget.rows("accounts") { accounts[a.text("id")] = a.text("name") }
        var categories: [String: String] = [:]
        for c in budget.rows("categories") { categories[c.text("id")] = c.text("name") }
        func item(_ schedule: JSON) -> ScheduleItem {
            let t = schedule["transaction"]
            let next = schedule.text("nextDate")
            let paused = schedule.flag("paused")
            let due = !paused && !next.isEmpty && next <= today
            let splits = t.rows("splits")
            let category: String?
            if !t.text("transferAccountId").isEmpty && splits.allSatisfy({ $0.text("categoryId").isEmpty && !$0.flag("unassigned") }) {
                category = nil
            } else if splits.count > 1 {
                category = String(localized: "category.split", defaultValue: "Split", comment: "In place of a category: the transaction is split.")
            } else if splits.first?.flag("unassigned") == true {
                category = String(localized: "Unassigned")
            } else {
                category = splits.first.flatMap { categories[$0.text("categoryId")] }
            }
            let frequency = schedule.text("frequency")
            let date = Money.date(next)
            let when = next.isEmpty ? String(localized: "Finished") : due ? String(localized: "Due \(date)") : date
            let title = t.text("payee").isEmpty
                ? accounts[t.text("transferAccountId")].map { String(localized: "Transfer: \($0)") } ?? String(localized: "Scheduled transaction")
                : t.text("payee")
            return ScheduleItem(id: schedule.text("id"), schedule: schedule, title: title,
                                subtitle: planningJoined([when, ScheduleFrequency.label(frequency), accounts[t.text("accountId")], category].compactMap { $0 }, separator: " · "),
                                amount: t.long("amount"), due: due, paused: paused, once: !ScheduleFrequency.repeats(frequency), next: next)
        }
        let all = budget.rows("schedules")
        let upcoming = ScheduleMath.upcoming(all)
        let items = upcoming.map(item)
        due = items.filter(\.due)
        coming = items.filter { !$0.due && !$0.paused }
        paused = items.filter(\.paused)
        finished = all.filter { $0.text("nextDate").isEmpty }.map(item)
        summary = ScheduleMath.summary(upcoming, today: today)
    }
}

/// What the schedule editor opens with: a schedule to edit, or nil for a new one.
private struct ScheduleEditorRequest: Identifiable {
    let id = UUID()
    var schedule: JSON?
}

/// Scheduled: what's due in the next 30 days, then every schedule soonest first with paused ones
/// after (SchedulesModal.tsx). Each opens in the editor; swipes and its menu enter, skip, pause or
/// delete it.
struct SchedulesView: View {
    @Environment(BudgetStore.self) private var store
    @State private var editing: ScheduleEditorRequest?
    @State private var deleting: ScheduleItem?

    init() {}

    var body: some View {
        let currency = store.currency
        let model = ScheduleListModel(budget: store.budget ?? [:], today: Dates.today)
        List {
            if model.isEmpty {
                EmptyState(title: String(localized: "Nothing scheduled"),
                           detail: String(localized: "Add a transaction with a later date, or choose how often it repeats, and it waits here."),
                           symbol: "calendar.badge.clock", action: String(localized: "Add schedule")) { editing = ScheduleEditorRequest() }
                    .listRowBackground(Color.clear)
            } else {
                Section {
                    summary(model.summary, currency)
                } footer: {
                    Text("A transaction dated ahead waits here and is entered on its date, uncleared and waiting for approval; a repeating one carries on from there.")
                }
            }
            if !model.due.isEmpty {
                Section {
                    ForEach(model.due) { row($0, currency) }
                    Button {
                        let due = model.due
                        store.runCommand("applySchedules", ["through": .string(Dates.today)]) {
                            store.showToast(due.count == 1 ? String(localized: "Entered \(due[0].title).")
                                            : String(localized: "Entered the \(due.count) schedules that were due.", locale: .app), undo: true)
                        }
                    } label: {
                        Label(model.due.count == 1 ? "Enter the one that's due" : "Enter all \(model.due.count) due", systemImage: "arrow.down.to.line")
                    }
                    .disabled(store.busy)
                } header: {
                    Text("Due")
                }
            }
            if !model.coming.isEmpty {
                Section("Coming up") { ForEach(model.coming) { row($0, currency) } }
            }
            if !model.paused.isEmpty {
                Section("Paused") { ForEach(model.paused) { row($0, currency) } }
            }
            if !model.finished.isEmpty {
                Section {
                    ForEach(model.finished) { row($0, currency) }
                } header: {
                    Text("Finished")
                } footer: {
                    Text("These have nothing left to enter. Delete them, or edit one to give it a new date.")
                }
            }
        }
        .groupedList()
        .navigationTitle("Scheduled")
        .toolbar {
            ToolbarItem(placement: .trailingBar) {
                Button { editing = ScheduleEditorRequest() } label: { Image(systemName: "plus") }
                    .accessibilityLabel("Add schedule")
            }
        }
        .sheet(item: $editing) { ScheduleEditor(schedule: $0.schedule) }
        .confirmationDialog("Delete this schedule?", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } }),
                            titleVisibility: .visible, presenting: deleting) { item in
            Button("Delete", role: .destructive) {
                store.remove("schedules", id: item.id) { store.showToast(String(localized: "Deleted the schedule for \(item.title)."), undo: true) }
            }
        } message: { item in
            Text("\(item.title) won't be entered again. Transactions it already entered stay. You can undo this.")
        }
    }

    private func summary(_ summary: ScheduleSummary, _ currency: String) -> some View {
        Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 12) {
            GridRow {
                Figure(label: String(localized: "Scheduled"), value: scheduledCount(summary))
                    .frame(maxWidth: .infinity, alignment: .leading)
                Figure(label: String(localized: "Due in 30 days"), value: "\(summary.dueSoon)").frame(maxWidth: .infinity, alignment: .leading)
            }
            GridRow {
                Figure(label: String(localized: "Going out"), value: money(summary.goingOut, currency)).frame(maxWidth: .infinity, alignment: .leading)
                Figure(label: String(localized: "Coming in"), value: money(summary.comingIn, currency), color: summary.comingIn > 0 ? .urPositive : .primary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }

    /// "4", or "4 · 1 paused" with paused ones.
    private func scheduledCount(_ summary: ScheduleSummary) -> String {
        guard summary.paused > 0 else { return "\(summary.active)" }
        return String(localized: "\(summary.active) · \(summary.paused) paused", locale: .app)
    }

    private func row(_ item: ScheduleItem, _ currency: String) -> some View {
        Button { editing = ScheduleEditorRequest(schedule: item.schedule) } label: {
            HStack(spacing: 12) {
                PlanningRowIcon(symbol: item.paused ? "pause.fill" : item.once ? "calendar" : "repeat",
                                tint: item.due ? .urWarning : item.paused || item.next.isEmpty ? .secondary : .urAccent)
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.title).foregroundStyle(Color.primary).lineLimit(1)
                    Text(item.subtitle).font(.footnote).foregroundStyle(item.due ? Color.urWarning : .secondary).lineLimit(2)
                }
                Spacer(minLength: 8)
                Text(AppLanguage.leftToRight((item.amount > 0 ? "+" : "") + money(item.amount, currency))).monospacedDigit().fontWeight(.semibold)
                    .foregroundStyle(item.amount > 0 ? Color.urPositive : .primary)
            }
            .opacity(item.paused || item.next.isEmpty ? 0.6 : 1)
        }
        .accessibilityHint("Opens the schedule")
        .swipeActions(edge: .leading, allowsFullSwipe: true) {
            if !item.next.isEmpty {
                Button { enter(item) } label: { Label("Enter", systemImage: "arrow.down.to.line") }.tint(.urAccent)
                Button { skip(item) } label: { Label("Skip", systemImage: "forward.end") }.tint(.gray)
            }
        }
        .swipeActions(edge: .trailing) {
            Button { deleting = item } label: { Label("Delete", systemImage: "trash") }.tint(.urNegative)
            if !item.next.isEmpty {
                Button { pause(item) } label: { Label(item.paused ? "Resume" : "Pause", systemImage: item.paused ? "play" : "pause") }
                    .tint(.urWarning)
            }
        }
        .contextMenu {
            if !item.next.isEmpty {
                Button { enter(item) } label: { Label("Enter now", systemImage: "arrow.down.to.line") }
                Button { skip(item) } label: { Label(item.once ? "Skip it" : "Skip \(Money.date(item.next))", systemImage: "forward.end") }
                Button { pause(item) } label: { Label(item.paused ? "Resume" : "Pause", systemImage: item.paused ? "play" : "pause") }
            }
            Button { editing = ScheduleEditorRequest(schedule: item.schedule) } label: { Label("Edit", systemImage: "pencil") }
            Divider()
            Button(role: .destructive) { deleting = item } label: { Label("Delete", systemImage: "trash") }
        }
    }

    private func enter(_ item: ScheduleItem) {
        store.runCommand("applySchedules", ["through": .string(Dates.today), "selected": .string(item.id)]) {
            store.showToast(String(localized: "Entered \(item.title)."), undo: true)
        }
    }

    private func skip(_ item: ScheduleItem) {
        let date = Money.date(item.next)
        store.runCommand("applySchedules", ["through": .string(Dates.today), "selected": .string(item.id), "skip": true]) {
            store.showToast(item.once ? String(localized: "Skipped \(item.title). It won't be entered.")
                            : String(localized: "Skipped \(item.title) on \(date)."), undo: true)
        }
    }

    private func pause(_ item: ScheduleItem) {
        store.upsert("schedules", ["id": .string(item.id), "paused": .bool(!item.paused)]) {
            store.showToast(item.paused ? String(localized: "Resumed \(item.title).")
                            : String(localized: "Paused \(item.title). It won't be entered until you resume it."), undo: true)
        }
    }
}
