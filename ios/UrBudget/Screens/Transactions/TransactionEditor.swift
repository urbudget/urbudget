import SwiftUI

private enum EditorRoute: Hashable { case payee, category, account, splitCategory(Int) }

/// One split line being edited: its share of the total, entered without a sign, as on the desktop.
private struct SplitDraft: Identifiable {
    let id = UUID()
    var original: JSON = [:]
    var category = ""
    var amount = ""
    var memo = ""
    var transfer = ""
    var payee = ""
    var transferStatus = "uncleared"
}

/// How often a new transaction repeats; "never" saves it once.
private let frequencies: [(id: String, label: String)] = [
    ("never", String(localized: "Never")), ("daily", String(localized: "Daily")), ("weekly", String(localized: "Weekly")),
    ("everyOtherWeek", String(localized: "Every other week")), ("twiceAMonth", String(localized: "Twice a month")),
    ("every4Weeks", String(localized: "Every 4 weeks")), ("monthly", String(localized: "Monthly")),
    ("everyOtherMonth", String(localized: "Every other month")), ("every3Months", String(localized: "Every 3 months")),
    ("every4Months", String(localized: "Every 4 months")), ("twiceAYear", String(localized: "Twice a year")),
    ("yearly", String(localized: "Yearly")), ("everyOtherYear", String(localized: "Every other year")),
]

/// Adding or editing a transaction takes the whole screen, as YNAB's app does: Outflow or Inflow, the
/// amount large on the keypad (digits fill in as cents and it does sums), then the payee, category,
/// account and date, and the rest below.
struct TransactionEditor: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var request: TransactionEditorRequest

    @State private var loaded = false
    @State private var account = ""
    @State private var payee = ""
    @State private var date = Dates.today
    @State private var memo = ""
    @State private var pad = PadState(nil)
    @State private var keypad = false
    @FocusState private var typing: Bool
    @State private var direction = "out"
    @State private var status = "uncleared"
    @State private var transfer = ""
    @State private var peerStatus = "uncleared"
    @State private var flag = ""
    @State private var approved = true
    @State private var splitMode = false
    @State private var category = ""
    @State private var splits: [SplitDraft] = []
    @State private var repeatEvery = "never"
    @State private var error: String?
    @State private var confirmDelete = false
    @State private var route: [EditorRoute] = []
    @State private var payees: [PayeeInfo] = []
    @State private var saving = false
    /// What Save & add another just saved, shown over the next one.
    @State private var lastSaved: String?
    @State private var addedAnother = 0
    /// Whether the cleared switch was set by hand, so a date change no longer sets it.
    @State private var statusTouched = false
    /// The desktop's "New transactions start cleared" (Settings).
    @AppStorage("newCleared") private var newCleared = false

    private var original: JSON? { request.transaction }
    private var budget: JSON { store.budget ?? [:] }
    private var today: String { Dates.today }
    private var repeatable: Bool { original == nil }
    /// A future, uncleared transaction is a schedule entered on its day.
    private var scheduled: Bool { date > today && status == "uncleared" && (transfer.isEmpty || peerStatus == "uncleared") }
    /// A transfer between two budget accounts (or two tracking ones) moves money without a category.
    private var needsCategory: Bool {
        transfer.isEmpty || (budget.row("accounts", id: account)?.text("kind") == "tracking") != (budget.row("accounts", id: transfer)?.text("kind") == "tracking")
    }
    /// A new transaction's status: cleared when Settings says so, unless it's in the future.
    private var newStatus: String { date <= today && newCleared ? "cleared" : "uncleared" }
    private var accounts: [JSON] {
        let used = Set(original?.rows("splits").map { $0.text("transferAccountId") } ?? [])
        return budget.rows("accounts").filter {
            !$0.flag("archived") || $0.text("id") == original?.text("accountId") || $0.text("id") == original?.text("transferAccountId") || used.contains($0.text("id"))
        }
    }

    private func accountName(_ id: String) -> String? { budget.row("accounts", id: id)?.text("name") }
    private func categoryName(_ id: String) -> String? {
        switch id {
        case "": return nil
        case Unassigned: return String(localized: "Unassigned")
        default: return budget.row("categories", id: id)?.text("name")
        }
    }

    /// The payee row's value: the payee, or the account a transfer goes to or comes from.
    private var payeeValue: String? {
        guard !transfer.isEmpty else { return payee.isEmpty ? nil : payee }
        let name = accountName(transfer) ?? String(localized: "account")
        return direction == "out" ? String(localized: "Transfer to \(name)") : String(localized: "Transfer from \(name)")
    }

    /// A repeat choice's name; "never" reads "Once" for a transaction dated ahead.
    private func frequencyLabel(_ choice: (id: String, label: String)) -> String {
        guard choice.id == "never" else { return choice.label }
        return date > today ? String(localized: "Once") : String(localized: "Never")
    }

    var body: some View {
        let currency = store.currency
        NavigationStack(path: $route) {
            ScrollViewReader { scroll in
            Form {
                if let lastSaved {
                    Label(lastSaved, systemImage: "checkmark.circle.fill")
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(Color.urPositive)
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets(top: 0, leading: 4, bottom: 0, trailing: 4))
                        .id("saved")
                }
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
                        typing = false
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
                    Notice(text: String(localized: "Create an account in Accounts before adding your first transaction."), tone: "info")
                }
                if original?.text("status") == "reconciled" || original?.text("transferStatus") == "reconciled" {
                    Notice(text: String(localized: "This transaction is reconciled. Changing its amount, date, or account changes previously reconciled history."),
                           icon: "lock.fill")
                }
                Section {
                    NavigationLink(value: EditorRoute.payee) {
                        formRow("storefront", String(localized: "Payee"), payeeValue, placeholder: String(localized: "Choose payee"))
                    }
                    if !splitMode && needsCategory {
                        NavigationLink(value: EditorRoute.category) {
                            formRow("tag", String(localized: "Category"), categoryName(category), placeholder: String(localized: "Choose category"),
                                    attention: category.isEmpty)
                        }
                    }
                    NavigationLink(value: EditorRoute.account) {
                        formRow("building.columns", String(localized: "Account"), accountName(account), placeholder: String(localized: "Choose account"))
                    }
                    DatePicker(selection: Binding(get: { Dates.pickerDate(date) }, set: { date = Dates.fromPicker($0) }), displayedComponents: .date) {
                        Label { Text(scheduled ? "Date · Scheduled" : date == today ? "Date · Today" : "Date") } icon: { Image(systemName: "calendar") }
                    }
                    if repeatable {
                        Picker(selection: $repeatEvery) {
                            ForEach(frequencies, id: \.id) { Text(frequencyLabel($0)).tag($0.id) }
                        } label: {
                            Label("Repeat", systemImage: "repeat")
                        }
                    }
                }
                if splitMode && needsCategory { splitSection(currency) }
                Section {
                    TextField("Memo", text: $memo, axis: .vertical).lineLimit(1...4).focused($typing)
                    clearedRow(transfer.isEmpty ? nil : accountName(account) ?? String(localized: "this account"),
                               Binding(get: { status }, set: { status = $0; statusTouched = true }))
                    if !transfer.isEmpty { clearedRow(accountName(transfer) ?? String(localized: "the other account"), $peerStatus) }
                    Picker(selection: $flag) {
                        Text("No flag").tag("")
                        ForEach(flagChoices(budget), id: \.id) { choice in
                            Label { Text(choice.name) } icon: { Image(systemName: "flag.fill").foregroundStyle(Color.flag(choice.id)) }.tag(choice.id)
                        }
                    } label: {
                        Label { Text("Flag") } icon: { Image(systemName: flag.isEmpty ? "flag" : "flag.fill").foregroundStyle(flag.isEmpty ? Color.secondary : Color.flag(flag)) }
                    }
                    if !approved {
                        HStack {
                            Label("Imported, not yet approved", systemImage: "sparkle")
                            Spacer()
                            Button("Approve") { approved = true }.fontWeight(.semibold).buttonStyle(.borderless)
                        }
                    }
                    if !splitMode && needsCategory {
                        Button { startSplit() } label: { Label("Split between categories", systemImage: "arrow.triangle.branch") }
                    }
                }
                if let error { FormErrorView(message: error) }
                if original == nil && repeatEvery == "never" {
                    Section {
                        Button { save(another: true) } label: {
                            Label(scheduled ? "Schedule & add another" : "Save & add another", systemImage: "plus.circle").frame(maxWidth: .infinity)
                        }
                        .disabled(accounts.isEmpty)
                        .keyboardShortcut(.return, modifiers: [.command, .shift])
                    }
                }
                if original != nil {
                    Section {
                        Button("Delete transaction", role: .destructive) { confirmDelete = true }.frame(maxWidth: .infinity)
                    }
                }
            }
            // The next transaction after Save & add another starts back at the top, on its amount.
            .onChange(of: addedAnother) { _, _ in withAnimation { scroll.scrollTo("saved", anchor: .top) } }
            }
            .disabled(saving)
            .interactiveDismissDisabled(saving)
            .navigationTitle(original == nil ? "New transaction" : "Edit transaction")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(saving) }
                ToolbarItem(placement: .confirmationAction) {
                    SaveButton(title: scheduled || repeatable && repeatEvery != "never" && date > today ? String(localized: "Schedule") : String(localized: "Save"),
                               saving: saving) { save() }
                        .disabled(accounts.isEmpty)
                        .keyboardShortcut(.return, modifiers: .command)
                }
            }
            .navigationDestination(for: EditorRoute.self) { destination($0) }
            .safeAreaInset(edge: .bottom) {
                if keypad {
                    AmountKeypad(pad: pad) {
                        keypad = false
                        // A new transaction goes on to its payee, as YNAB's keypad does.
                        if original == nil && payee.isEmpty && transfer.isEmpty { route = [.payee] }
                    }
                    .transition(.move(edge: .bottom))
                }
            }
            .animation(.snappy(duration: 0.25), value: keypad)
            // The keypad steps aside for the keyboard and for the pickers.
            .onChange(of: typing) { _, typing in if typing { keypad = false } }
            .onChange(of: route) { _, route in if !route.isEmpty { keypad = false } }
            // A new transaction dated in the future starts uncleared (it's scheduled), as on the desktop.
            .onChange(of: date) { _, _ in if original == nil && !statusTouched { status = newStatus } }
            .confirmationDialog("Delete transaction?", isPresented: $confirmDelete, titleVisibility: .visible) {
                Button("Delete", role: .destructive) {
                    guard let id = original?.text("id") else { return }
                    attempt($error, saving: $saving) {
                        try await store.removing("transactions", id: id)
                        store.showToast(String(localized: "Deleted the transaction."), undo: true)
                        dismiss()
                    }
                }
            } message: {
                Text("Both sides of a transfer will be removed. You can undo the change.")
            }
        }
        .onAppear(perform: load)
        .task(id: store.version) {
            let budget = budget
            payees = await Task.detached(priority: .userInitiated) { payeeIndex(budget) }.value
        }
    }

    private func load() {
        guard !loaded else { return }
        loaded = true
        let defaultAccount = request.account.isEmpty
            ? (accounts.first { $0.text("kind") != "tracking" } ?? accounts.first)?.text("id") ?? ""
            : request.account
        guard let original else {
            account = defaultAccount
            status = newStatus
            keypad = true
            return
        }
        account = original.text("accountId")
        payee = original.text("payee")
        date = original.text("date", Dates.today)
        memo = original.text("memo")
        pad = PadState(Int64(original.long("amount").magnitude))
        direction = original.long("amount") >= 0 ? "in" : "out"
        status = original.text("status", "uncleared")
        transfer = original.text("transferAccountId")
        peerStatus = original.text("transferStatus", "uncleared")
        flag = original.text("flag")
        approved = original.flag("approved", true)
        let parts = original.rows("splits")
        splitMode = parts.count > 1
        if let first = parts.first { category = first.flag("unassigned") ? Unassigned : first.text("categoryId") }
        let sign: Int64 = original.long("amount") < 0 ? -1 : 1
        splits = parts.map { part in
            SplitDraft(original: part, category: part.flag("unassigned") ? Unassigned : part.text("categoryId"),
                       amount: Money.editable(part.long("amount") * sign), memo: part.text("memo"),
                       transfer: part.text("transferAccountId"), payee: part.text("payee"), transferStatus: part.text("transferStatus", "uncleared"))
        }
    }

    @ViewBuilder private func destination(_ route: EditorRoute) -> some View {
        switch route {
        case .payee:
            PayeePicker(payees: payees, account: account, payee: payee, transfer: transfer, onPayee: { name in
                payee = name
                transfer = ""
                // The payee's usual category fills in when none is chosen yet; its usual "none" is Unassigned only for income.
                if let usual = payees.first(where: { $0.name == name })?.category, !splitMode, category.isEmpty {
                    if usual == Unassigned || usual.isEmpty { if direction == "in" { category = Unassigned } }
                    else { category = usual }
                }
                self.route = category.isEmpty && !splitMode ? [.category] : []
            }, onTransfer: { id in
                transfer = id
                self.route = []
            })
        case .category:
            CategoryPicker(title: String(localized: "Category"), month: String(date.prefix(7)), selected: category, includeUnassigned: true,
                           includeUncategorized: true, includePayment: false, allowsCreate: true, onSplit: { self.route = []; startSplit() }) { id in
                category = id
                self.route = []
            }
        case .account:
            AccountPicker(selected: account) { id in
                account = id
                if transfer == id { transfer = "" }
                self.route = []
            }
        case .splitCategory(let index):
            CategoryPicker(title: String(localized: "Split category"), month: String(date.prefix(7)), selected: splits.indices.contains(index) ? splits[index].category : "",
                           includeUnassigned: true, includeUncategorized: true, includePayment: false, allowsCreate: true) { id in
                if splits.indices.contains(index) { splits[index].category = id }
                self.route = []
            }
        }
    }

    private func formRow(_ icon: String, _ label: String, _ value: String?, placeholder: String, attention: Bool = false) -> some View {
        Label {
            VStack(alignment: .leading, spacing: 1) {
                if value != nil { Text(label).font(.caption).foregroundStyle(Color.secondary) }
                Text(value ?? placeholder).foregroundStyle(value != nil ? .primary : attention ? Color.urWarning : .secondary).lineLimit(1)
            }
        } icon: {
            Image(systemName: icon).foregroundStyle(attention ? Color.urWarning : .secondary)
        }
    }

    /// Cleared as a switch, or locked once reconciled. `account` names the side of a transfer it's for.
    @ViewBuilder private func clearedRow(_ account: String?, _ value: Binding<String>) -> some View {
        if value.wrappedValue == "reconciled" {
            Label { Text(account.map { String(localized: "Reconciled in \($0)") } ?? String(localized: "Reconciled")) } icon: { Image(systemName: "lock.fill") }
                .foregroundStyle(Color.secondary)
        } else {
            Toggle(isOn: Binding(get: { value.wrappedValue == "cleared" }, set: { value.wrappedValue = $0 ? "cleared" : "uncleared" })) {
                Label { Text(account.map { String(localized: "Cleared in \($0)") } ?? String(localized: "Cleared")).lineLimit(1) } icon: {
                    ClearedMark(status: value.wrappedValue).frame(width: 22, height: 22)
                }
            }
            .tint(.urPositive)
        }
    }

    /// A split line's heading: Transfer, its category, or the prompt to choose one.
    private func splitName(_ part: SplitDraft) -> String {
        if !part.transfer.isEmpty { return String(localized: "Transfer") }
        return categoryName(part.category) ?? String(localized: "Choose category")
    }

    private func startSplit() {
        splitMode = true
        if splits.count < 2 {
            splits = [SplitDraft(category: category, amount: Money.editable(pad.cents ?? 0)), SplitDraft(amount: Money.editable(0))]
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
                                Text(splitName(part))
                                    .fontWeight(.semibold)
                                    .foregroundStyle(part.category.isEmpty && part.transfer.isEmpty ? Color.urWarning : .primary)
                                Image(systemName: "chevron.forward").font(.caption.weight(.bold)).foregroundStyle(Color.urTertiary)
                            }
                        }
                        .buttonStyle(.borderless)
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
            Button { splits.append(SplitDraft(amount: Money.editable(0))) } label: { Label("Add line", systemImage: "plus") }
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
                Text(left == 0 ? "Fully split" : (total < 0) == (left < 0) ? "\(money(Int64(left.magnitude), currency)) left to split" : "\(money(Int64(left.magnitude), currency)) over the total")
                    .textCase(nil)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(left == 0 ? Color.urPositive : Color.urWarning)
            }
        } footer: {
            Text("Each line is its share of the total.")
        }
    }

    private func save(another: Bool = false) {
        typing = false
        attempt($error, saving: $saving) {
            guard !account.isEmpty else { throw AppError(String(localized: "Choose an account.")) }
            guard Dates.day(date) != nil else { throw AppError(String(localized: "Choose a valid date.")) }
            guard let parsed = pad.cents else { throw AppError(String(localized: "Finish the sum first.")) }
            guard parsed >= 0 else { throw AppError(String(localized: "Use Outflow or Inflow to choose the direction.")) }
            guard transfer != account else { throw AppError(String(localized: "Choose a different transfer account.")) }
            let sign: Int64 = direction == "out" ? -1 : 1
            let cents = parsed * sign
            var record = original ?? ["id": .string(UUID().uuidString.lowercased())]
            record["date"] = .string(date)
            record["accountId"] = .string(account)
            record["payee"] = .string(payee)
            record["memo"] = .string(memo)
            record["amount"] = .int(cents)
            record["status"] = .string(status)
            record["approved"] = .bool(approved)
            record["flag"] = JSON.text(orNull: flag)
            record["transferAccountId"] = JSON.text(orNull: transfer)
            record["transferStatus"] = transfer.isEmpty ? .null : .string(peerStatus)
            let single = original?.rows("splits").count == 1 ? original!.rows("splits")[0] : JSON.object([:])
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
                    part["transferStatus"] = draft.transfer.isEmpty ? .null : .string(draft.transferStatus)
                    return part
                }
                guard parts.reduce(Int64(0), { $0 + $1.long("amount") }) == cents else {
                    throw AppError(String(localized: "The split amounts must equal the transaction total."))
                }
            } else {
                var part = single
                part["categoryId"] = category.isEmpty || category == Unassigned ? .null : .string(category)
                part["unassigned"] = .bool(category == Unassigned)
                part["amount"] = .int(cents)
                parts = [part]
            }
            record["splits"] = .array(parts)
            if repeatable && repeatEvery != "never" {
                // One schedule from this date: entered now when it's due, and repeating from there.
                let schedule: JSON = ["id": record["id"], "transaction": record, "nextDate": .string(date), "anchorDate": .string(date),
                                      "frequency": .string(repeatEvery), "paused": false]
                try await store.saveRepeating(schedule: schedule, entered: date <= today ? record : nil)
                let often = frequencies.first { $0.id == repeatEvery }?.label.lowercased() ?? repeatEvery
                store.showToast(date <= today ? String(localized: "Saved. It repeats \(often).")
                                : String(localized: "Scheduled \(often) from \(Money.date(date))."))
            } else {
                let rules = budget.rows("payeeRules")
                try await store.saveTransaction(record, original: original)
                // A bank payee renamed by hand: offer to name that bank text the same way from now on.
                if let rule = BankNames.learn(original: original, payee: payee, transfer: !transfer.isEmpty, rules: rules) {
                    store.offerPayeeRule(rule)
                }
                if another {
                    let name = transfer.isEmpty ? (payee.isEmpty ? String(localized: "the transaction") : payee) : String(localized: "the transfer")
                    startAnother(saved: String(localized: "Saved \(name), \(money(cents, store.currency)). Add the next one."))
                    return
                }
                if scheduled { store.showToast(String(localized: "Scheduled for \(Money.date(date)). It's entered on that day.")) }
            }
            dismiss()
        }
    }

    /// After Save & add another: a fresh transaction in the same account on the same date, as the desktop's.
    private func startAnother(saved: String) {
        payee = ""; memo = ""; category = ""; transfer = ""; flag = ""
        pad = PadState(nil)
        splits = []; splitMode = false
        approved = true; peerStatus = "uncleared"
        statusTouched = false
        status = newStatus
        lastSaved = saved
        addedAnother += 1
        route = []
        keypad = true
    }
}
