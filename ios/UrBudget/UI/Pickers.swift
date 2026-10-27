import SwiftUI

/// One entry of the category picker, prepared once per budget so typing a search does no JSON work.
private struct PickerEntry: Identifiable {
    var id: String
    var name: String
    var group: String
    var available: Int64?
    var status: String
}

/// Choosing a category: search, then the groups with each category's Available pill, as YNAB's
/// picker shows them. Unassigned and Uncategorized come first when offered; hidden categories last.
struct CategoryPicker: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var title: String
    var month: String
    var selected: String
    var includeUnassigned = false
    var includeUncategorized = false
    var includePayment = true
    /// A category to leave out, such as the one being deleted.
    var excluding = ""
    /// Offers to create a category named as the search, when none matches it.
    var allowsCreate = false
    var onSplit: (() -> Void)?
    var onPick: (String) -> Void
    @State private var search = ""
    @State private var searching = false
    @State private var creating: String?
    /// The category just created, chosen once its sheet has gone (choosing pops this picker, which
    /// mustn't happen while the sheet and the search field are still being put away).
    @State private var created: String?
    /// A choice made while searching, reported once the search has closed (see `PickedAfterSearch`).
    @State private var picked: String?

    private var groups: [(name: String, entries: [PickerEntry])] {
        guard let budget = store.budget else { return [] }
        var totals: [String: JSON] = [:]
        for row in store.summary?.rows("categories") ?? [] { totals[row.text("id")] = row }
        let query = search.trimmingCharacters(in: .whitespaces).lowercased()
        return orderedGroups(budget) { (includePayment || $0.text("paymentAccountId").isEmpty) && $0.text("id") != excluding }.compactMap { group in
            let entries = group.categories.map { c -> PickerEntry in
                let id = c.text("id")
                return PickerEntry(id: id, name: c.text("name"), group: group.name, available: totals[id]?.long("available"),
                                   status: CategoryStatus(category: c, summary: totals[id], month: month).status)
            }.filter { query.isEmpty || "\($0.name) \($0.group)".lowercased().contains(query) }
            return entries.isEmpty ? nil : (group.name, entries)
        }
    }

    var body: some View {
        let currency = store.currency
        List {
            if search.isEmpty && (onSplit != nil || includeUnassigned || includeUncategorized) {
                Section {
                    if let onSplit {
                        Button { onSplit() } label: { Label("Split between categories", systemImage: "arrow.triangle.branch") }
                    }
                    if includeUnassigned {
                        row(id: Unassigned, name: String(localized: "Unassigned"), detail: String(localized: "Available to assign"),
                            available: store.summary?.long("readyToAssign"),
                            status: nil, currency: currency)
                    }
                    if includeUncategorized {
                        row(id: "", name: String(localized: "Uncategorized"), detail: nil, available: nil, status: nil, currency: currency)
                    }
                }
            }
            ForEach(groups, id: \.name) { group in
                Section(groupDisplayName(group.name)) {
                    ForEach(group.entries) { entry in
                        row(id: entry.id, name: entry.name, detail: nil, available: entry.available, status: entry.status, currency: currency)
                    }
                }
            }
            let typed = search.trimmingCharacters(in: .whitespaces)
            let exact = groups.contains { $0.entries.contains { $0.name.caseInsensitiveCompare(typed) == .orderedSame } }
            if allowsCreate && !typed.isEmpty && !exact {
                Section {
                    Button {
                        searching = false
                        creating = typed
                    } label: { Label("Create “\(typed)”", systemImage: "plus.circle.fill") }
                }
            } else if groups.isEmpty && !search.isEmpty {
                ContentUnavailableView.search(text: search)
            }
        }
        .groupedList()
        .searchable(text: $search, isPresented: $searching, prompt: allowsCreate ? Text("Search or create a category") : Text("Search categories"))
        .modifier(PickedAfterSearch(searching: $searching, picked: $picked, report: onPick))
        .navigationTitle(title)
        .inlineTitle()
        .sheet(item: Binding(get: { creating.map { IdentifiedText(value: $0) } }, set: { creating = $0?.value }), onDismiss: {
            guard let id = created else { return }
            created = nil
            onPick(id)
        }) { item in
            NewCategoryForm(presetName: item.value) { id in created = id }
        }
    }

    private func row(id: String, name: String, detail: String?, available: Int64?, status: String?, currency: String) -> some View {
        Button {
            choose(id, searching: $searching, picked: $picked, report: onPick)
        } label: {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(name).fontWeight(id == selected ? .semibold : .regular).foregroundStyle(Color.primary)
                    if let detail { Text(detail).font(.footnote).foregroundStyle(Color.secondary) }
                }
                Spacer()
                if id == selected { Image(systemName: "checkmark").foregroundStyle(Color.urAccent).fontWeight(.semibold) }
                if let available {
                    AvailablePill(text: money(available, currency), status: status ?? (available < 0 ? "negative" : available > 0 ? "positive" : "zero"))
                }
            }
        }
    }
}

/// Choosing an account, with each one's balance.
struct AccountPicker: View {
    @Environment(BudgetStore.self) private var store
    var selected: String
    var onPick: (String) -> Void

    var body: some View {
        let currency = store.currency
        var balances: [String: Int64] = [:]
        for row in store.summary?.rows("accounts") ?? [] { balances[row.text("id")] = row.long("balance") }
        let accounts = (store.budget?.rows("accounts") ?? []).filter { !$0.flag("archived") || $0.text("id") == selected }
        return List(accounts, id: \.idText) { account in
            let id = account.text("id")
            Button { onPick(id) } label: {
                HStack(spacing: 12) {
                    AccountIcon(account: account, balance: balances[id] ?? 0)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(account.text("name")).foregroundStyle(Color.primary)
                        Text(AccountTypes.typeLabel(account)).font(.footnote).foregroundStyle(Color.secondary)
                    }
                    Spacer()
                    Text(money(balances[id] ?? 0, currency)).monospacedDigit()
                        .foregroundStyle((balances[id] ?? 0) < 0 ? Color.urNegative : .primary)
                    if id == selected { Image(systemName: "checkmark").foregroundStyle(Color.urAccent).fontWeight(.semibold) }
                }
            }
        }
        .groupedList()
        .navigationTitle("Account")
        .inlineTitle()
    }
}

extension JSON {
    /// A record's id, for lists keyed by it.
    var idText: String { text("id") }
}

/// An account's type as a round icon, coloured by whether its money is in the budget.
struct AccountIcon: View {
    var account: JSON
    var balance: Int64

    var body: some View {
        let kind = account.text("kind")
        let outside = !AccountTypes.inNetWorth(account)
        let tint: Color = outside ? .secondary : kind == "cash" ? .urPositive : kind == "credit" ? .urAccent : .secondary
        Image(systemName: AccountTypes.symbol(type: AccountTypes.infer(account, balance: balance), kind: kind))
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(tint)
            .frame(width: 34, height: 34)
            .background(tint.opacity(0.14), in: Circle())
    }
}

/// Payees: type to search or name a new one; transfers to the other accounts come first, as YNAB lists them.
struct PayeePicker: View {
    @Environment(BudgetStore.self) private var store
    var payees: [PayeeInfo]
    var account: String
    var payee: String
    var transfer: String
    var onPayee: (String) -> Void
    var onTransfer: (String) -> Void
    @State private var search = ""
    /// A transaction without a payee yet opens straight on typing one.
    @State private var searching = false
    @State private var started = false
    @State private var picked: String?
    @State private var pickedTransfer: String?

    var body: some View {
        let query = search.trimmingCharacters(in: .whitespaces)
        let names = payees.map(\.name).filter { query.isEmpty || $0.localizedCaseInsensitiveContains(query) }
        let transfers = (store.budget?.rows("accounts") ?? []).filter {
            !$0.flag("archived") && $0.text("id") != account && (query.isEmpty || $0.text("name").localizedCaseInsensitiveContains(query))
        }
        List {
            if !query.isEmpty && !names.contains(where: { $0.caseInsensitiveCompare(query) == .orderedSame }) {
                Button { choose(query, searching: $searching, picked: $picked, report: onPayee) } label: { Label("Add “\(query)”", systemImage: "plus.circle.fill") }
            }
            if !transfers.isEmpty {
                Section("Transfer to or from") {
                    ForEach(transfers, id: \.idText) { other in
                        Button { choose(other.text("id"), searching: $searching, picked: $pickedTransfer, report: onTransfer) } label: {
                            HStack {
                                Label(other.text("name"), systemImage: "arrow.left.arrow.right").foregroundStyle(Color.primary)
                                Spacer()
                                if transfer == other.text("id") { Image(systemName: "checkmark").foregroundStyle(Color.urAccent) }
                            }
                        }
                    }
                }
            }
            if !names.isEmpty {
                Section("Payees") {
                    ForEach(names, id: \.self) { name in
                        Button { choose(name, searching: $searching, picked: $picked, report: onPayee) } label: {
                            HStack {
                                Text(name).foregroundStyle(Color.primary)
                                Spacer()
                                if name == payee && transfer.isEmpty { Image(systemName: "checkmark").foregroundStyle(Color.urAccent) }
                            }
                        }
                    }
                }
            } else if query.isEmpty && transfers.isEmpty {
                ContentUnavailableView("No payees yet", systemImage: "storefront",
                                       description: Text("Type who you paid, or who paid you, to add them."))
                    .listRowBackground(Color.clear)
            }
        }
        .groupedList()
        .searchable(text: $search, isPresented: $searching, prompt: "Search or add a payee")
        .onSubmit(of: .search) {
            // Return picks the payee typed, or the one match.
            guard !query.isEmpty else { return }
            choose(names.first { $0.caseInsensitiveCompare(query) == .orderedSame } ?? query, searching: $searching, picked: $picked, report: onPayee)
        }
        .modifier(PickedAfterSearch(searching: $searching, picked: $picked, report: onPayee))
        .modifier(PickedAfterSearch(searching: $searching, picked: $pickedTransfer, report: onTransfer))
        .onAppear {
            guard !started else { return }
            started = true
            if payee.isEmpty && transfer.isEmpty { searching = true }
        }
        .navigationTitle("Payee")
        .inlineTitle()
    }
}

/// The months of a year as a grid, as YNAB's month header opens them.
struct MonthPickerSheet: View {
    var month: String
    var onPick: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var year: Int

    init(month: String, onPick: @escaping (String) -> Void) {
        self.month = month
        self.onPick = onPick
        _year = State(initialValue: Dates.year(month))
    }

    var body: some View {
        let now = Dates.currentMonth
        NavigationStack {
            VStack(spacing: 18) {
                HStack {
                    Button { year -= 1 } label: { Image(systemName: "chevron.backward") }.accessibilityLabel("Previous year")
                    Text(String(year)).font(.title2.bold()).monospacedDigit().frame(maxWidth: .infinity)
                    Button { year += 1 } label: { Image(systemName: "chevron.forward") }.accessibilityLabel("Next year")
                }
                .font(.title3)
                LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 10), count: 4), spacing: 10) {
                    ForEach(1...12, id: \.self) { number in
                        let value = Dates.month(year: year, number: number)
                        let chosen = value == month
                        Button { onPick(value) } label: {
                            Text(Dates.shortMonthName(value))
                                .font(.body.weight(.semibold))
                                .frame(maxWidth: .infinity, minHeight: 46)
                                .foregroundStyle(chosen ? Color.white : value == now ? Color.urAccent : .primary)
                                .background(chosen ? Color.urAccent : value == now ? Color.urAccent.opacity(0.14) : Color.insetBackground,
                                            in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                        }
                        .buttonStyle(.plain)
                    }
                }
                if month != now {
                    Button("Go to this month") { onPick(now) }
                }
                Spacer(minLength: 0)
            }
            .padding()
            .navigationTitle("Month")
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
        }
        .presentationDetents([.height(360)])
    }
}

/// Reports a choice made in a picker. While its search field is open, the search closes first and
/// the choice follows (`PickedAfterSearch`): a navigation stack ignores going back from a page
/// whose search is still open, so the picker would otherwise stay up after a choice.
@MainActor func choose(_ value: String, searching: Binding<Bool>, picked: Binding<String?>, report: (String) -> Void) {
    if searching.wrappedValue {
        picked.wrappedValue = value
        searching.wrappedValue = false
    } else {
        report(value)
    }
}

struct PickedAfterSearch: ViewModifier {
    @Binding var searching: Bool
    @Binding var picked: String?
    var report: (String) -> Void

    func body(content: Content) -> some View {
        content.onChange(of: searching) { _, open in
            guard !open, let value = picked else { return }
            picked = nil
            // Once the search's closing animation is over: going back during it leaves the page up.
            Task { @MainActor in
                try? await Task.sleep(for: .milliseconds(450))
                report(value)
            }
        }
    }
}
