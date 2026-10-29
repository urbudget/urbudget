import SwiftUI

private enum ScheduleRoute: Hashable { case payee, category, account, splitCategory(Int) }

/// One split line of a scheduled transaction: its share of the total, without a sign.
private struct ScheduleSplitDraft: Identifiable {
    let id = UUID()
    var original: JSON = [:]
    var category = ""
    var amount = ""
    var memo = ""
    var transfer = ""
    var payee = ""
}

/// A schedule: the transaction it enters (direction and amount on the keypad, payee or transfer,
/// category or split, account, memo, flag), its next date, how often it repeats, and pausing it.
/// One that's due when saved (dated today or earlier) is entered right away, as the desktop's
/// Register.tsx saveSchedule does; `nil` makes a new schedule.
struct ScheduleEditor: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    private let original: JSON?

    init(schedule: JSON?) { original = schedule }

    @State private var loaded = false
    @State private var id = UUID().uuidString.lowercased()
    @State private var account = ""
    @State private var payee = ""
    @State private var transfer = ""
    @State private var nextDate = Dates.addDays(Dates.today, 1)
    @State private var frequency = "monthly"
    @State private var paused = false
    @State private var memo = ""
    @State private var flag = ""
    @State private var pad = PadState(nil)
    @State private var keypad = false
    @State private var direction = "out"
    @State private var category = ""
    @State private var splitMode = false
    @State private var splits: [ScheduleSplitDraft] = []
    @State private var error: String?
    @State private var saving = false
    @State private var confirmDelete = false
    @State private var route: [ScheduleRoute] = []
    @State private var payees: [PayeeInfo] = []

    private var budget: JSON { store.budget ?? [:] }
    private var today: String { Dates.today }
    private var template: JSON { original?["transaction"] ?? [:] }
    /// A transfer between two budget accounts (or two tracking ones) moves money without a category.
    private var needsCategory: Bool {
        transfer.isEmpty || (budget.row("accounts", id: account)?.text("kind") == "tracking") != (budget.row("accounts", id: transfer)?.text("kind") == "tracking")
    }
    private var accounts: [JSON] {
        budget.rows("accounts").filter { !$0.flag("archived") || $0.text("id") == template.text("accountId") || $0.text("id") == template.text("transferAccountId") }
    }
    private var due: Bool { !paused && nextDate <= today }

    private func accountName(_ id: String) -> String? { budget.row("accounts", id: id)?.text("name") }

    /// The payee row's value: the payee, or the transfer it makes.
    private var payeeValue: String? {
        guard !transfer.isEmpty else { return payee.isEmpty ? nil : payee }
        if let name = accountName(transfer) {
            return direction == "out" ? String(localized: "Transfer to \(name)") : String(localized: "Transfer from \(name)")
        }
        return direction == "out" ? String(localized: "Transfer to account") : String(localized: "Transfer from account")
    }

    var body: some View {
        let currency = store.currency
        NavigationStack(path: $route) {
            Form {
                Section {
                    Picker("Direction", selection: $direction) {
                        Text("Outflow").tag("out")
                        Text("Inflow").tag("in")
                    }
                    .pickerStyle(.segmented)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
                }
                Section {
                    Button {
                        keypad = true
                        pad.fresh = true
                    } label: {
                        VStack(spacing: 4) {
                            PadAmount(pad: pad, currency: currency, color: direction == "in" ? .urPositive : .primary, sign: direction == "in" ? "+" : "−")
                            Text(keypad ? "Type the amount" : "Tap to change").font(.footnote).foregroundStyle(Color.secondary)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                    }
                    .listRowBackground(keypad ? Color.urAccent.opacity(0.12) : nil)
                    .accessibilityLabel("Amount, \(money(pad.cents ?? 0, currency)). Edit amount")
                }
                if accounts.isEmpty {
                    Notice(text: String(localized: "Create an account in Accounts before scheduling a transaction."), tone: "info")
                }
                Section {
                    NavigationLink(value: ScheduleRoute.payee) {
                        PlanningPickerRow(icon: "storefront", label: String(localized: "Payee"), value: payeeValue,
                                          placeholder: String(localized: "Choose payee"))
                    }
                    if !splitMode && needsCategory {
                        NavigationLink(value: ScheduleRoute.category) {
                            PlanningPickerRow(icon: "tag", label: String(localized: "Category"), value: planningCategoryName(store.budget, category),
                                              placeholder: String(localized: "Choose category"), attention: category.isEmpty)
                        }
                    }
                    NavigationLink(value: ScheduleRoute.account) {
                        PlanningPickerRow(icon: "building.columns", label: String(localized: "Account"), value: accountName(account),
                                          placeholder: String(localized: "Choose account"))
                    }
                }
                if splitMode && needsCategory { splitSection(currency) }
                scheduleSection
                Section {
                    TextField("Memo", text: $memo, axis: .vertical).lineLimit(1...4)
                    Picker(selection: $flag) {
                        Text("No flag").tag("")
                        ForEach(flagChoices(budget), id: \.id) { choice in
                            Label { Text(choice.name) } icon: { Image(systemName: "flag.fill").foregroundStyle(Color.flag(choice.id)) }.tag(choice.id)
                        }
                    } label: {
                        Label { Text("Flag") } icon: {
                            Image(systemName: flag.isEmpty ? "flag" : "flag.fill").foregroundStyle(flag.isEmpty ? Color.secondary : Color.flag(flag))
                        }
                    }
                    if !splitMode && needsCategory {
                        Button { startSplit() } label: { Label("Split between categories", systemImage: "arrow.triangle.branch") }
                    }
                }
                if let error { FormErrorView(message: error) }
                if original != nil {
                    Section {
                        Button("Delete schedule", role: .destructive) { confirmDelete = true }.frame(maxWidth: .infinity)
                    }
                }
            }
            .navigationTitle(original == nil ? "New schedule" : "Edit schedule")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button(due ? "Save & enter" : "Save", action: save).fontWeight(.semibold).disabled(accounts.isEmpty)
                    }
                }
            }
            .navigationDestination(for: ScheduleRoute.self) { destination($0) }
            .safeAreaInset(edge: .bottom) {
                if keypad {
                    AmountKeypad(pad: pad) {
                        keypad = false
                        if original == nil && payee.isEmpty && transfer.isEmpty { route = [.payee] }
                    }
                }
            }
            .confirmationDialog("Delete this schedule?", isPresented: $confirmDelete, titleVisibility: .visible) {
                Button("Delete", role: .destructive, action: delete)
            } message: {
                Text("It won't be entered again. Transactions it already entered stay. You can undo this.")
            }
        }
        .interactiveDismissDisabled(saving)
        .retainingEditorDraft("schedule", values: ["account": .string(account), "payee": .string(payee), "transfer": .string(transfer), "nextDate": .string(nextDate), "frequency": .string(frequency), "paused": .bool(paused), "memo": .string(memo), "flag": .string(flag), "amountExpression": .string(pad.text), "direction": .string(direction), "category": .string(category), "splitMode": .bool(splitMode), "splits": .array(splits.map { part in ["category": .string(part.category), "amount": .string(part.amount), "memo": .string(part.memo), "transfer": .string(part.transfer), "payee": .string(part.payee)] })])
        .onAppear(perform: load)
        .task(id: store.version) {
            let budget = budget
            payees = await Task.detached(priority: .userInitiated) { payeeIndex(budget) }.value
        }
    }

    private var scheduleSection: some View {
        let following = ScheduleMath.dates(from: nextDate, anchor: anchorDate, frequency: frequency, count: 4).dropFirst()
        return Section {
            DatePicker(selection: Binding(get: { Dates.pickerDate(nextDate) }, set: { nextDate = Dates.fromPicker($0) }), displayedComponents: .date) {
                Label { Text(due ? "Next date · Due" : "Next date") } icon: { Image(systemName: "calendar") }
            }
            Picker(selection: $frequency) {
                ForEach(ScheduleFrequency.all, id: \.id) { Text($0.label).tag($0.id) }
            } label: {
                Label("Repeat", systemImage: "repeat")
            }
            Toggle(isOn: $paused) { Label("Paused", systemImage: "pause.circle") }
        } header: {
            Text("Schedule")
        } footer: {
            VStack(alignment: .leading, spacing: 4) {
                if paused {
                    Text("A paused schedule isn't entered until you resume it.")
                } else if ScheduleFrequency.repeats(frequency) && !following.isEmpty {
                    Text("Then \(planningList(following.map(Money.date))) and on.")
                } else if !ScheduleFrequency.repeats(frequency) {
                    Text("Entered once, then it's done.")
                }
                if !paused { Text("If it's dated today or earlier, it's entered as soon as you save it.") }
            }
        }
    }

    /// The date the schedule repeats from: kept while the next date stays, else the new date (as Android saves it).
    private var anchorDate: String {
        guard let original, nextDate == original.text("nextDate") else { return nextDate }
        return original.text("anchorDate", nextDate)
    }

    private func load() {
        guard !loaded else { return }
        loaded = true
        guard let original else {
            account = (accounts.first { $0.text("kind") != "tracking" } ?? accounts.first)?.text("id") ?? ""
            keypad = true
            return
        }
        let t = template
        id = original.text("id")
        account = t.text("accountId")
        payee = t.text("payee")
        transfer = t.text("transferAccountId")
        nextDate = original.text("nextDate").isEmpty ? Dates.addDays(today, 1) : original.text("nextDate")
        let saved = original.text("frequency")
        frequency = ScheduleFrequency.all.contains { $0.id == saved } ? saved : "never"
        paused = original.flag("paused")
        memo = t.text("memo")
        flag = t.text("flag")
        pad = PadState(Int64(t.long("amount").magnitude))
        direction = t.long("amount") > 0 ? "in" : "out"
        let parts = t.rows("splits")
        splitMode = parts.count > 1
        if let first = parts.first { category = first.flag("unassigned") ? Unassigned : first.text("categoryId") }
        let sign: Int64 = t.long("amount") < 0 ? -1 : 1
        splits = parts.map { part in
            ScheduleSplitDraft(original: part, category: part.flag("unassigned") ? Unassigned : part.text("categoryId"),
                               amount: Money.editable(part.long("amount") * sign), memo: part.text("memo"),
                               transfer: part.text("transferAccountId"), payee: part.text("payee"))
        }
    }

    @ViewBuilder private func destination(_ route: ScheduleRoute) -> some View {
        switch route {
        case .payee:
            PayeePicker(payees: payees, account: account, payee: payee, transfer: transfer, onPayee: { name in
                payee = name
                transfer = ""
                // The payee's usual category fills in when none is chosen yet; its usual "none" is Unassigned only for income.
                if let usual = payees.first(where: { $0.name == name })?.category, !splitMode, category.isEmpty {
                    if usual == Unassigned || usual.isEmpty { if direction == "in" { category = Unassigned } }
                    else if budget.row("categories", id: usual)?.text("paymentAccountId").isEmpty ?? false { category = usual }
                }
                self.route = category.isEmpty && !splitMode ? [.category] : []
            }, onTransfer: { other in
                transfer = other
                self.route = []
            })
        case .category:
            CategoryPicker(title: String(localized: "Category"), month: String(nextDate.prefix(7)), selected: category, includeUnassigned: true,
                           includeUncategorized: true, includePayment: false, onSplit: { self.route = []; startSplit() }) { picked in
                category = picked
                self.route = []
            }
        case .account:
            AccountPicker(selected: account) { picked in
                account = picked
                if transfer == picked { transfer = "" }
                self.route = []
            }
        case .splitCategory(let index):
            CategoryPicker(title: String(localized: "Split category"), month: String(nextDate.prefix(7)), selected: splits.indices.contains(index) ? splits[index].category : "",
                           includeUnassigned: true, includeUncategorized: true, includePayment: false) { picked in
                if splits.indices.contains(index) { splits[index].category = picked }
                self.route = []
            }
        }
    }

    private func startSplit() {
        splitMode = true
        if splits.count < 2 {
            splits = [ScheduleSplitDraft(category: category, amount: Money.editable(pad.cents ?? 0)), ScheduleSplitDraft(amount: Money.editable(0))]
        }
    }

    private func splitSection(_ currency: String) -> some View {
        let total = pad.cents ?? 0
        let assigned = splits.reduce(Int64(0)) { $0 + ((try? Money.parse($1.amount)) ?? 0) }
        let left = total - assigned
        return Section {
            ForEach($splits) { $part in
                let index = splits.firstIndex { $0.id == part.id } ?? 0
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Button { route.append(.splitCategory(index)) } label: {
                            HStack {
                                Text(splitTitle(part))
                                    .fontWeight(.semibold)
                                    .foregroundStyle(part.category.isEmpty && part.transfer.isEmpty ? Color.urWarning : .primary)
                                Image(systemName: "chevron.forward").font(.caption.weight(.bold)).foregroundStyle(Color.urTertiary)
                            }
                        }
                        .buttonStyle(.borderless)
                        .disabled(!part.transfer.isEmpty)
                        Spacer()
                        Button { splits.removeAll { $0.id == part.id } } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(Color.urTertiary) }
                            .buttonStyle(.borderless)
                            .accessibilityLabel("Remove split line \(index + 1)")
                    }
                    HStack {
                        AmountField(title: String(localized: "Amount"), text: $part.amount, labeled: false).frame(maxWidth: 130)
                        TextField("Memo", text: $part.memo)
                    }
                    .textFieldStyle(.roundedBorder)
                    if left != 0 && ((try? Money.parse(part.amount)) ?? 0) == 0 {
                        Button("Use the \(money(Int64(left.magnitude), currency)) left") { part.amount = Money.editable(left) }
                            .font(.subheadline).buttonStyle(.borderless)
                    }
                }
                .padding(.vertical, 4)
            }
            Button { splits.append(ScheduleSplitDraft(amount: Money.editable(0))) } label: { Label("Add line", systemImage: "plus") }
            if splits.count <= 1 {
                Button("Undo split") {
                    splitMode = false
                    category = splits.first?.category ?? ""
                }
            }
        } header: {
            HStack {
                Text("Split").textCase(nil).font(.headline).foregroundStyle(Color.primary)
                Spacer()
                splitStatus(left: left, total: total, currency: currency)
                    .textCase(nil)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(left == 0 ? Color.urPositive : Color.urWarning)
            }
        } footer: {
            Text("Each line is its share of the total.")
        }
    }

    /// A split line's title: its category, or Transfer.
    private func splitTitle(_ part: ScheduleSplitDraft) -> String {
        if !part.transfer.isEmpty { return String(localized: "Transfer") }
        return planningCategoryName(store.budget, part.category) ?? String(localized: "Choose category")
    }

    /// How much of the total the split lines leave, or go over it by.
    private func splitStatus(left: Int64, total: Int64, currency: String) -> Text {
        let amount = money(Int64(left.magnitude), currency)
        if left == 0 { return Text("Fully split") }
        return (total < 0) == (left < 0) ? Text("\(amount) left to split") : Text("\(amount) over the total")
    }

    /// The schedule as edited, keeping every field of the original this editor doesn't show.
    private func record() throws -> JSON {
        guard !account.isEmpty else { throw AppError(String(localized: "Choose an account.")) }
        guard Dates.day(nextDate) != nil else { throw AppError(String(localized: "Choose a valid date.")) }
        guard let parsed = pad.cents else { throw AppError(String(localized: "Finish the sum first.")) }
        guard parsed >= 0 else { throw AppError(String(localized: "Use Outflow or Inflow to choose the direction.")) }
        guard transfer != account else { throw AppError(String(localized: "Choose a different transfer account.")) }
        let sign: Int64 = direction == "out" ? -1 : 1
        let cents = parsed * sign
        var t: JSON = original == nil ? ["id": .string("schedule:\(id)"), "status": "uncleared"] : template
        t["date"] = .string(nextDate)
        t["accountId"] = .string(account)
        t["payee"] = .string(payee.trimmingCharacters(in: .whitespaces))
        t["memo"] = .string(memo)
        t["amount"] = .int(cents)
        if !t.has("status") { t["status"] = "uncleared" }
        t["flag"] = JSON.text(orNull: flag)
        t["transferAccountId"] = JSON.text(orNull: transfer)
        t["transferStatus"] = transfer.isEmpty ? .null : "uncleared"
        let single = template.rows("splits").count == 1 ? template.rows("splits")[0] : JSON.object([:])
        var parts: [JSON]
        if !needsCategory {
            var part = single
            part["categoryId"] = .null
            part["unassigned"] = false
            part["amount"] = .int(cents)
            parts = [part]
        } else if splitMode {
            guard splits.count >= 2 else { throw AppError(String(localized: "Add at least two split lines.")) }
            parts = try splits.map { draft in
                var part = draft.original
                part["categoryId"] = draft.category.isEmpty || draft.category == Unassigned ? .null : .string(draft.category)
                part["unassigned"] = .bool(draft.category == Unassigned)
                part["amount"] = .int(try Money.parse(draft.amount) * sign)
                part["memo"] = .string(draft.memo)
                part["payee"] = JSON.text(orNull: draft.payee)
                part["transferAccountId"] = JSON.text(orNull: draft.transfer)
                part["transferStatus"] = draft.transfer.isEmpty ? .null : "uncleared"
                return part
            }
            guard parts.reduce(Int64(0), { $0 + $1.long("amount") }) == cents else { throw AppError(String(localized: "The split amounts must equal the transaction total.")) }
        } else {
            var part = single
            part["categoryId"] = category.isEmpty || category == Unassigned ? .null : .string(category)
            part["unassigned"] = .bool(category == Unassigned)
            part["amount"] = .int(cents)
            parts = [part]
        }
        t["splits"] = .array(parts)
        var schedule = original ?? [:]
        schedule["id"] = .string(id)
        schedule["transaction"] = t
        schedule["nextDate"] = .string(nextDate)
        schedule["anchorDate"] = .string(anchorDate)
        schedule["frequency"] = .string(frequency)
        schedule["paused"] = .bool(paused)
        return schedule
    }

    private func save() {
        let schedule: JSON
        do {
            schedule = try record()
        } catch {
            self.error = BudgetStore.readable(error)
            return
        }
        let enter = due
        let today = today
        let isNew = original == nil
        let guarded = original.map { (collection: "schedules", record: $0) }
        let id = id
        let entered = enteredMessage(schedule["transaction"])
        let scheduled = scheduledMessage(frequency, from: Money.date(nextDate))
        saving = true
        error = nil
        Task {
            defer { saving = false }
            do {
                // A due schedule is entered with the save, as one change: every occurrence up to today, on its own date.
                try await store.planningEdit(original: guarded, then: enter ? [("applySchedules", ["through": .string(today)])] : []) { budget in
                    if !isNew && budget.row("schedules", id: id) == nil {
                        throw AppError(String(localized: "This schedule was entered or deleted since you opened it. Close the editor to see the latest."))
                    }
                    try budget.upsertRecord("schedules", schedule)
                }
                if enter {
                    store.showToast(entered, undo: true)
                } else if isNew {
                    store.showToast(scheduled, undo: true)
                }
                dismiss()
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }

    private func delete() {
        guard let original else { return }
        let guarded = (collection: "schedules", record: original)
        let id = id
        saving = true
        Task {
            defer { saving = false }
            do {
                try await store.planningEdit(original: guarded) { try $0.removeRecord("schedules", id: id) }
                store.showToast(String(localized: "Deleted the schedule."), undo: true)
                dismiss()
            } catch {
                self.error = BudgetStore.readable(error)
            }
        }
    }

    /// "Entered Rent. It repeats monthly.", once a due schedule is saved and entered.
    private func enteredMessage(_ t: JSON) -> String {
        let entered: String
        if !t.text("payee").isEmpty {
            entered = String(localized: "Entered \(t.text("payee")).")
        } else if let other = accountName(t.text("transferAccountId")) {
            entered = String(localized: "Entered the transfer to \(other).")
        } else {
            entered = String(localized: "Entered the transfer to another account.")
        }
        return Self.repeats(frequency).map { entered + " " + $0 } ?? entered
    }

    /// "It repeats monthly.", in a sentence of its own for each frequency.
    private static func repeats(_ frequency: String) -> String? {
        switch frequency {
        case "daily": return String(localized: "It repeats daily.")
        case "weekly": return String(localized: "It repeats weekly.")
        case "everyOtherWeek": return String(localized: "It repeats every other week.")
        case "every4Weeks": return String(localized: "It repeats every 4 weeks.")
        case "twiceAMonth": return String(localized: "It repeats twice a month.")
        case "monthly": return String(localized: "It repeats monthly.")
        case "everyOtherMonth": return String(localized: "It repeats every other month.")
        case "every3Months": return String(localized: "It repeats every 3 months.")
        case "every4Months": return String(localized: "It repeats every 4 months.")
        case "twiceAYear": return String(localized: "It repeats twice a year.")
        case "yearly": return String(localized: "It repeats yearly.")
        case "everyOtherYear": return String(localized: "It repeats every other year.")
        default: return nil
        }
    }

    /// "Scheduled monthly from 21/10/2026.", or a one-time schedule's date, once a new schedule is saved.
    private func scheduledMessage(_ frequency: String, from date: String) -> String {
        switch frequency {
        case "daily": return String(localized: "Scheduled daily from \(date).")
        case "weekly": return String(localized: "Scheduled weekly from \(date).")
        case "everyOtherWeek": return String(localized: "Scheduled every other week from \(date).")
        case "every4Weeks": return String(localized: "Scheduled every 4 weeks from \(date).")
        case "twiceAMonth": return String(localized: "Scheduled twice a month from \(date).")
        case "monthly": return String(localized: "Scheduled monthly from \(date).")
        case "everyOtherMonth": return String(localized: "Scheduled every other month from \(date).")
        case "every3Months": return String(localized: "Scheduled every 3 months from \(date).")
        case "every4Months": return String(localized: "Scheduled every 4 months from \(date).")
        case "twiceAYear": return String(localized: "Scheduled twice a year from \(date).")
        case "yearly": return String(localized: "Scheduled yearly from \(date).")
        case "everyOtherYear": return String(localized: "Scheduled every other year from \(date).")
        default: return String(localized: "Scheduled for \(date). It's entered on that day.")
        }
    }
}
