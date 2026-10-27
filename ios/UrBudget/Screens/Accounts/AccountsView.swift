import SwiftUI

/// One account as its row shows it.
private struct AccountLine: Identifiable {
    var id: String
    var account: JSON
    var name: String
    var subtitle: String
    var balance: Int64
    var balanceText: String
    var approvals: Int
    var needsCategory: Int
    var inNetWorth: Bool
    var archived: Bool
    var loan: Bool
}

private struct AccountSection: Identifiable {
    var id: String
    var name: String
    var lines: [AccountLine]
    var total: Int64
}

private func accountSections(budget: JSON, summary: JSON?) -> [AccountSection] {
    let currency = budget.text("currency")
    var balances: [String: Int64]?
    if let summary {
        var values: [String: Int64] = [:]
        for row in summary.rows("accounts") { values[row.text("id")] = row.long("balance") }
        balances = values
    }
    let badges = AccountBadges(budget: budget)
    let loans = Set(budget.rows("loans").map { $0.text("accountId") })
    let all = budget.rows("accounts")
    func line(_ account: JSON) -> AccountLine {
        let id = account.text("id")
        let balance = balances?[id] ?? account.long("openingBalance")
        let type = AccountTypes.infer(account, balance: balance)
        let worth = AccountTypes.inNetWorth(account)
        return AccountLine(id: id, account: account, name: account.text("name"),
                           subtitle: AccountTypes.label(type) + (worth ? "" : " · " + String(localized: "Not in net worth")),
                           balance: balance, balanceText: balances == nil ? "—" : money(balance, currency),
                           approvals: badges.approvals[id] ?? 0, needsCategory: badges.uncategorized[id] ?? 0,
                           inNetWorth: worth, archived: account.flag("archived"),
                           loan: loans.contains(id) || account.text("kind") == "tracking" && AccountTypes.isLiability(type))
    }
    let kinds = [("cash", String(localized: "Cash")), ("credit", String(localized: "Credit")),
                 ("tracking", String(localized: "Tracking")), ("closed", String(localized: "Closed"))]
    return kinds.compactMap { key, name in
        let lines = all.filter { key == "closed" ? $0.flag("archived") : !$0.flag("archived") && $0.text("kind") == key }.map(line)
        // Accounts left out of net worth are left out of the total too, as in the sidebar.
        let total = lines.reduce(Int64(0)) { $0 + ($1.inNetWorth ? $1.balance : 0) }
        return lines.isEmpty ? nil : AccountSection(id: key, name: name, lines: lines, total: total)
    }
}

private enum AccountsPage: Hashable { case netWorth }

/// What the account form opens with: an account to edit, or nil for a new one.
struct AccountFormRequest: Identifiable {
    let id = UUID()
    var account: JSON?
}

/// Accounts: cash, credit, tracking and closed accounts, each kind a section with its total.
/// A tap opens the account's transactions; a long press, its actions.
struct AccountsView: View {
    @Environment(BudgetStore.self) private var store
    @Environment(SyncCoordinator.self) private var sync
    @State private var folded: Set<String> = ["closed"]
    @State private var form: AccountFormRequest?
    @State private var reconciling: JSON?
    @State private var paying: String?
    @State private var loanFor: String?
    @State private var addCompact = false
    @State private var launchPage: AccountsLaunch?

    /// Debug builds open the first account's register (`-open register`) or net worth (`-open networth`) at launch.
    private enum AccountsLaunch: Hashable { case register(String), netWorth }

    var body: some View {
        let currency = store.currency
        let sections = store.budget.map { accountSections(budget: $0, summary: store.summary) } ?? []
        NavigationStack {
            List {
                if let summary = store.summary {
                    Section {
                        // Opens the Net worth report, month by month.
                        NavigationLink(value: AccountsPage.netWorth) {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text("Net worth").font(.subheadline).foregroundStyle(Color.secondary)
                                    Text(money(summary.long("netWorth"), currency))
                                        .font(.system(size: 28, weight: .heavy, design: .rounded)).monospacedDigit()
                                        .foregroundStyle(summary.long("netWorth") < 0 ? Color.urNegative : .primary)
                                }
                                Spacer()
                                Image(systemName: "chart.line.uptrend.xyaxis").font(.title2).foregroundStyle(Color.urAccent)
                            }
                            .padding(.vertical, 4)
                        }
                        .accessibilityHint("Shows net worth month by month")
                    }
                }
                if sections.isEmpty {
                    EmptyState(title: String(localized: "Add your first account"),
                               detail: String(localized: "Start with your checking or spending account and its current balance."),
                               symbol: "building.columns", action: String(localized: "Add account")) { form = AccountFormRequest() }
                }
                ForEach(sections) { section in
                    let open = !folded.contains(section.id)
                    Section {
                        if open {
                            ForEach(section.lines) { line in
                                NavigationLink(value: RegisterTarget(account: line.id)) { row(line) }
                                    .contextMenu { menu(line, section) }
                            }
                        }
                    } header: {
                        sectionHeader(section, open: open, currency: currency)
                    }
                }
                Color.clear.frame(height: 56).listRowBackground(Color.clear)
            }
            .groupedList()
            .readableWidth()
            .shrinksAddButton($addCompact)
            .refreshable {
                store.foreground(force: true)
                await sync.syncAutomatically()
            }
            .navigationTitle("Accounts")
            .toolbar {
                ToolbarItem(placement: .trailingBar) {
                    Button { form = AccountFormRequest() } label: { Image(systemName: "plus") }.accessibilityLabel("Add account")
                }
                ToolbarItem(placement: .trailingBar) { BudgetActionsMenu() }
            }
            .navigationDestination(for: RegisterTarget.self) { TransactionsList(target: $0) }
            .navigationDestination(for: AccountsPage.self) { _ in StandaloneReport(id: "networth") }
            .navigationDestination(item: $launchPage) { page in
                switch page {
                case .register(let id): TransactionsList(target: RegisterTarget(account: id))
                case .netWorth: StandaloneReport(id: "networth")
                }
            }
            .onAppear {
                switch LaunchRoute.open {
                case "register": launchPage = (sections.first?.lines.first?.id).map { .register($0) }
                case "networth": launchPage = .netWorth
                default: return
                }
                LaunchRoute.open = nil
            }
            .overlay(alignment: .bottomTrailing) { AddTransactionButton(compact: addCompact) }
            .sheet(item: $form) { AccountForm(request: $0) }
            .sheet(item: Binding(get: { paying.map { IdentifiedText(value: $0) } }, set: { paying = $0?.value })) { CardPaymentSheet(accountId: $0.value) }
            .sheet(item: Binding(get: { loanFor.map { IdentifiedText(value: $0) } }, set: { loanFor = $0?.value })) { LoanEditor(accountId: $0.value) }
            .sheet(item: Binding(get: { reconciling.map { IdentifiedJSON(value: $0) } }, set: { reconciling = $0?.value })) { ReconcileSheet(account: $0.value) }
        }
    }

    /// A kind's header: its name and total, folding its accounts away on a tap.
    private func sectionHeader(_ section: AccountSection, open: Bool, currency: String) -> some View {
        let total = money(section.total, currency)
        return Button {
            withAnimation { if open { folded.insert(section.id) } else { folded.remove(section.id) } }
        } label: {
            HStack {
                // Points to where the text goes (left in Arabic), and down once open.
                Image(systemName: "chevron.right").font(.caption.weight(.bold)).rotationEffect(.degrees(open ? 90 : 0))
                    .flipsForRightToLeftLayoutDirection(true)
                Text(section.name)
                Spacer()
                Text(total).monospacedDigit()
                    .foregroundStyle(section.id != "closed" && section.total < 0 ? Color.urNegative : .primary)
            }
            .font(.headline)
            .foregroundStyle(Color.primary)
            .textCase(nil)
        }
        .buttonStyle(.plain)
        .accessibilityLabel(open ? "\(section.name), \(total). Collapse" : "\(section.name), \(total). Expand")
    }

    private func row(_ line: AccountLine) -> some View {
        HStack(spacing: 12) {
            AccountIcon(account: line.account, balance: line.balance)
            VStack(alignment: .leading, spacing: 2) {
                Text(line.name).foregroundStyle(line.inNetWorth ? .primary : .secondary)
                // "Checking" under an account called Checking says nothing new.
                if line.subtitle.caseInsensitiveCompare(line.name) != .orderedSame {
                    Text(line.subtitle).font(.footnote).foregroundStyle(Color.secondary)
                }
            }
            Spacer()
            if line.needsCategory > 0 {
                Circle().fill(Color.urAttention).frame(width: 9, height: 9).accessibilityLabel("Needs a category")
            }
            if line.approvals > 0 {
                Text(line.approvals > 99 ? "99+" : "\(line.approvals)")
                    .font(.caption.bold()).foregroundStyle(.white)
                    .padding(.horizontal, 6).frame(minWidth: 20, minHeight: 20)
                    .background(Color.urAccent, in: Capsule())
                    .accessibilityLabel("\(line.approvals) to approve")
            }
            Text(line.balanceText).monospacedDigit().fontWeight(.semibold)
                .foregroundStyle(!line.inNetWorth ? .secondary : line.balance < 0 ? Color.urNegative : .primary)
        }
    }

    @ViewBuilder private func menu(_ line: AccountLine, _ section: AccountSection) -> some View {
        Button { form = AccountFormRequest(account: line.account) } label: { Label("Edit account", systemImage: "pencil") }
        Button { reconciling = line.account } label: { Label("Reconcile", systemImage: "lock") }.disabled(line.archived)
        if line.account.text("kind") == "credit" && !line.archived {
            Button { paying = line.id } label: { Label("Make a payment", systemImage: "creditcard") }
        }
        if line.loan {
            Button { loanFor = line.id } label: { Label("Loan details & payoff", systemImage: "percent") }
        }
        Divider()
        let index = section.lines.firstIndex { $0.id == line.id } ?? 0
        Button { move(line, section, -1) } label: { Label("Move up", systemImage: "arrow.up") }.disabled(index == 0)
        Button { move(line, section, 1) } label: { Label("Move down", systemImage: "arrow.down") }.disabled(index == section.lines.count - 1)
        Divider()
        Button {
            let archived = !line.archived
            store.upsert("accounts", ["id": .string(line.id), "archived": .bool(archived)]) {
                store.showToast(closingMessage(line, archived: archived), undo: true)
            }
        } label: {
            Label(line.archived ? "Reopen account" : "Close account", systemImage: line.archived ? "tray.and.arrow.up" : "archivebox")
        }
    }

    /// "Closed Visa." (with the balance it still has, if any) or "Reopened Visa."
    private func closingMessage(_ line: AccountLine, archived: Bool) -> String {
        guard archived else { return String(localized: "Reopened \(line.name).") }
        guard line.balance != 0 else { return String(localized: "Closed \(line.name).") }
        return String(localized: "Closed \(line.name). It still has a balance of \(line.balanceText).")
    }

    private func move(_ line: AccountLine, _ section: AccountSection, _ delta: Int) {
        guard let index = section.lines.firstIndex(where: { $0.id == line.id }), section.lines.indices.contains(index + delta) else { return }
        let neighbour = section.lines[index + delta].id
        store.edit { budget in
            budget["accounts"] = .array(AccountTypes.move(budget["accounts"].array, id: line.id, neighbour: neighbour, after: delta > 0))
        }
    }
}

/// Adding or editing an account: its name, type, balance, note and whether it counts in net worth.
struct AccountForm: View {
    @Environment(BudgetStore.self) private var store
    var request: AccountFormRequest
    @State private var name = ""
    @State private var type = "checking"
    /// Empty for a new account (the field shows 0.00), so typing doesn't start by deleting it.
    @State private var balance = ""
    @State private var note = ""
    @State private var archived = false
    @State private var netWorth = true
    @State private var loaded = false

    var body: some View {
        let original = request.account
        let kind = AccountTypes.kind(for: type)
        let existingKind = original?.text("kind")
        let choices = AccountTypes.all.filter { existingKind == nil || existingKind == "" || AccountTypes.kind(for: $0.id) == existingKind }
        FormSheet(title: original == nil ? String(localized: "Add account") : String(localized: "Edit account"),
                  confirm: original == nil ? String(localized: "Add") : String(localized: "Save"),
                  enabled: !name.trimmingCharacters(in: .whitespaces).isEmpty) {
            let opening = balance.trimmingCharacters(in: .whitespaces).isEmpty ? 0 : try Money.parse(balance)
            // Only tracking accounts can stay out of net worth.
            let counted = kind != "tracking" || netWorth
            if let original {
                try await store.upserting("accounts", ["id": .string(original.text("id")), "name": .string(name.trimmingCharacters(in: .whitespaces)),
                                                       "accountType": .string(type), "note": .string(note), "openingBalance": .int(opening),
                                                       "archived": .bool(archived), "netWorth": .bool(counted)], original: original)
            } else {
                try await store.saveAccount(name: name, kind: kind, openingBalance: opening, accountType: type, note: note, netWorth: counted)
                let added = name.trimmingCharacters(in: .whitespaces)
                store.showToast(String(localized: "Added \(added)."))
            }
        } content: {
            Section {
                TextField("Account name", text: $name).wordsCapitalized()
                Picker("Account type", selection: $type) {
                    ForEach(choices, id: \.id) { Text($0.label).tag($0.id) }
                }
            } footer: {
                Text(kind == "cash" ? "Its money is in your budget, ready to assign." : kind == "credit" ? "A credit card payment category is created for it." : "Tracked outside the budget, in net worth.")
            }
            Section {
                AmountField(title: balanceTitle(new: original == nil), text: $balance)
            } footer: {
                Text(balanceFooter(new: original == nil))
            }
            Section {
                TextField("Note", text: $note, axis: .vertical).lineLimit(2...6)
            }
            if kind == "tracking" || original != nil {
                Section {
                    if kind == "tracking" {
                        Toggle("Count in net worth", isOn: $netWorth)
                    }
                    if original != nil {
                        Toggle("Account closed", isOn: $archived)
                    }
                } footer: {
                    Text(original != nil ? "A closed account keeps its history and balance in the budget." : "")
                }
            }
        }
        .onAppear {
            guard !loaded else { return }
            loaded = true
            guard let original else { return }
            // An untyped account is read as the accounts list reads it: an imported debt stays a debt.
            let current = store.summary?.rows("accounts").first { $0.text("id") == original.text("id") }?.long("balance") ?? original.long("openingBalance")
            name = original.text("name")
            type = AccountTypes.forEditing(original, balance: current)
            balance = Money.editable(original.long("openingBalance"))
            note = original.text("note")
            archived = original.flag("archived")
            netWorth = AccountTypes.inNetWorth(original)
        }
    }

    private func balanceTitle(new: Bool) -> String {
        let currency = store.currency
        return new ? String(localized: "Current balance (\(currency))") : String(localized: "Opening balance (\(currency))")
    }

    /// How to enter a debt, and for an existing account what changing its balance or kind means.
    private func balanceFooter(new: Bool) -> String {
        let debt: String? = AccountTypes.isLiability(type) ? String(localized: "Enter what you owe as a negative amount.")
            : new ? String(localized: "For debt, enter a negative balance.") : nil
        let history: String? = new ? nil : String(localized: "Changing an opening balance changes the account’s entire history.")
        let kinds: String? = new ? nil
            : String(localized: "To switch between cash, credit and tracking, create a new account so past transactions keep their budget treatment.")
        return [debt, history, kinds].compactMap { $0 }.joined(separator: " ")
    }
}

/// Reconciling, as YNAB asks it: is the cleared balance what the bank says? If not, enter the bank's.
struct ReconcileSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var account: JSON
    @State private var differs = false
    @State private var balance = ""
    @State private var date = Dates.today
    @State private var preview: Int64?
    @State private var loading = false
    @State private var error: String?
    @State private var adjustmentId = UUID().uuidString.lowercased()

    var body: some View {
        let currency = store.currency
        let total = store.summary?.rows("accounts").first { $0.text("id") == account.text("id") }
        let cleared = total?.long("cleared") ?? 0
        NavigationStack {
            Form {
                Section {
                    VStack(spacing: 4) {
                        Text("Is your cleared balance").foregroundStyle(Color.secondary)
                        Text(money(cleared, currency)).font(.system(size: 34, weight: .heavy, design: .rounded)).monospacedDigit()
                        Text("what your bank shows?").foregroundStyle(Color.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
                }
                if !differs && preview == nil {
                    Section {
                        HStack(spacing: 12) {
                            Button { differs = true } label: { Text("No").frame(maxWidth: .infinity) }.buttonStyle(.bordered)
                            Button {
                                balance = Money.editable(cleared)
                                review()
                            } label: { Text("Yes").frame(maxWidth: .infinity) }
                            .buttonStyle(.borderedProminent).tint(.urPositive)
                            .disabled(total == nil || loading)
                        }
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets())
                    }
                }
                if differs {
                    Section {
                        AmountField(title: String(localized: "Bank's balance"), text: $balance).onChange(of: balance) { preview = nil }
                        DayPicker(title: String(localized: "Statement date"), iso: $date).onChange(of: date) { preview = nil }
                    } footer: {
                        Text("Mark the transactions on your statement as cleared first; any difference left becomes an adjustment.")
                    }
                }
                if loading { ProgressView().frame(maxWidth: .infinity) }
                if let preview {
                    if preview == 0 {
                        Notice(text: String(localized: "Balances match. Cleared transactions will be locked as reconciled."), tone: "positive", icon: "lock.fill")
                    } else {
                        Notice(text: String(localized: "An adjustment of \(money(preview, currency)) will be recorded. Look for missing or incorrect transactions first."),
                               tone: "attention", icon: "exclamationmark.triangle.fill")
                    }
                }
                if let error { FormErrorView(message: error) }
            }
            .navigationTitle("Reconcile \(account.text("name"))")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button(preview == nil ? "Review" : preview == 0 ? "Reconcile" : "Adjust & finish") {
                        if preview == nil { review() } else { finish() }
                    }
                    .fontWeight(.semibold)
                    .disabled(total == nil || loading || !(differs || preview != nil))
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func params() throws -> JSON {
        ["accountId": .string(account.text("id")), "balance": .int(try Money.parse(balance)), "date": .string(date), "adjustmentId": .string(adjustmentId)]
    }

    private func review() {
        attempt($error) {
            let params = try params()
            loading = true
            Task {
                defer { loading = false }
                do {
                    let candidate = try await store.query("reconcile", params)
                    preview = candidate.row("transactions", id: adjustmentId)?.long("amount") ?? 0
                } catch {
                    self.error = BudgetStore.readable(error)
                }
            }
        }
    }

    private func finish() {
        attempt($error) {
            store.runCommand("reconcile", try params())
            dismiss()
        }
    }
}
