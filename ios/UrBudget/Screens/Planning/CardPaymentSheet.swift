import SwiftUI

private enum CardPaymentRoute: Hashable { case from }

/// Paying a credit card, as the desktop's payment form does: what's set aside in its payment
/// category, the whole balance, or another amount on the keypad, from the cash account that paid it
/// last. It's a transfer, so it isn't spending: it uses up what's set aside and lowers what's owed.
struct CardPaymentSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    private let accountId: String

    init(accountId: String) { self.accountId = accountId }

    @State private var loaded = false
    // Fixed when the sheet opens, so the choices don't move while it's open.
    @State private var owed: Int64 = 0
    @State private var aside: Int64 = 0
    @State private var paymentId = UUID().uuidString.lowercased()
    @State private var from = ""
    @State private var choice = "other"
    @State private var pad = PadState(nil)
    @State private var keypad = false
    @State private var date = Dates.today
    @State private var memo = ""
    @State private var error: String?
    @State private var saving = false
    @State private var route: [CardPaymentRoute] = []

    private var card: JSON? { store.budget?.row("accounts", id: accountId) }
    private var category: JSON? { store.budget?.rows("categories").first { $0.text("paymentAccountId") == accountId } }
    private var sources: [JSON] { (store.budget?.rows("accounts") ?? []).filter { $0.text("kind") == "cash" && !$0.flag("archived") } }
    private var amount: Int64 {
        switch choice {
        case "aside": return aside
        case "balance": return owed
        default: return pad.cents ?? 0
        }
    }

    var body: some View {
        NavigationStack(path: $route) {
            Group {
                if let card { form(card) } else { ContentUnavailableView("This account was deleted", systemImage: "creditcard") }
            }
            .navigationDestination(for: CardPaymentRoute.self) { _ in sourcePicker }
        }
        .presentationDetents([.large])
        .interactiveDismissDisabled(saving)
        .retainingEditorDraft("cardPayment", values: ["accountId": .string(accountId), "from": .string(from), "choice": .string(choice), "amountExpression": .string(pad.text), "date": .string(date), "memo": .string(memo)])
        .onAppear(perform: load)
    }

    private func load() {
        guard !loaded, let budget = store.budget else { return }
        loaded = true
        let balances = store.planningBalances
        owed = max(0, -(balances[accountId] ?? 0))
        if let category {
            let available = store.summary?.rows("categories").first { $0.text("id") == category.text("id") }?.long("available") ?? 0
            aside = max(0, available)
        }
        from = PlanningCardMath.defaultSource(budget, cardId: accountId, sources: sources, balances: balances)
        choice = aside > 0 ? "aside" : owed > 0 ? "balance" : "other"
        keypad = choice == "other"
    }

    private func form(_ card: JSON) -> some View {
        let currency = store.currency
        let source = sources.first { $0.text("id") == from }
        return Form {
            Section {
                HStack(alignment: .top) {
                    Figure(label: String(localized: "Owed on the card"), value: money(owed, currency), color: owed > 0 ? .urNegative : .primary)
                    Spacer()
                    Figure(label: String(localized: "Set aside to pay it"), value: category == nil ? "—" : money(aside, currency),
                           color: category != nil && aside >= owed && owed > 0 ? .urPositive : .primary, alignment: .trailing)
                }
                .accessibilityElement(children: .combine)
                if category != nil && owed > aside {
                    Text("\(money(owed - aside, currency)) of the balance isn't set aside yet.").font(.footnote).foregroundStyle(Color.urWarning)
                }
            }
            Section("Amount") {
                if let category {
                    let name = category.text("name")
                    option("aside", String(localized: "What's set aside"), String(localized: "In the \(name) payment category"), aside, currency)
                }
                option("balance", String(localized: "The whole balance"), String(localized: "Pays the card off"), owed, currency)
                Button {
                    choice = "other"
                    keypad = true
                    pad.fresh = true
                } label: {
                    HStack(spacing: 12) {
                        mark(choice == "other")
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Another amount").foregroundStyle(Color.primary)
                            Text(otherAmount(currency))
                                .font(.footnote).monospacedDigit().foregroundStyle(choice == "other" ? Color.urAccent : .secondary)
                        }
                        Spacer()
                    }
                }
                .listRowBackground(keypad ? Color.urAccent.opacity(0.12) : nil)
                .accessibilityAddTraits(choice == "other" ? .isSelected : [])
            }
            Section {
                if sources.isEmpty {
                    Notice(text: String(localized: "Add a cash account in your budget to pay the card from."), tone: "info")
                } else {
                    NavigationLink(value: CardPaymentRoute.from) {
                        PlanningPickerRow(icon: "building.columns", label: String(localized: "From"), value: source?.text("name"),
                                          placeholder: String(localized: "Choose account"))
                    }
                }
                DatePicker(selection: Binding(get: { Dates.pickerDate(date) }, set: { date = Dates.fromPicker($0) }), displayedComponents: .date) {
                    Label("Date", systemImage: "calendar")
                }
                TextField("Memo (optional)", text: $memo)
            } footer: {
                footer(card, source)
            }
            if let error { FormErrorView(message: error) }
        }
        .navigationTitle("Pay \(card.text("name"))")
        .inlineTitle()
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                if saving {
                    ProgressView()
                } else {
                    Button("Pay", action: pay).fontWeight(.semibold).disabled(amount <= 0 || sources.isEmpty || keypad && pad.cents == nil)
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            if keypad {
                AmountKeypad(pad: pad) { keypad = false }
            } else if amount > 0 && !sources.isEmpty {
                Button(action: pay) {
                    Label("Pay \(money(amount, currency))", systemImage: "creditcard")
                        .font(.headline)
                        .frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.borderedProminent)
                .buttonBorderShape(.capsule)
                .padding(.horizontal)
                .padding(.bottom, 8)
                .disabled(saving)
            }
        }
    }

    /// Another amount's line: the sum or amount on the keypad once chosen, else how to enter one.
    private func otherAmount(_ currency: String) -> String {
        guard choice == "other" else { return String(localized: "Type it on the keypad") }
        return pad.isSum ? AppLanguage.leftToRight(pad.display) : money(pad.cents ?? 0, currency)
    }

    /// What the payment is recorded as, and when it waits for its date.
    private func footer(_ card: JSON, _ source: JSON?) -> Text {
        let from = source?.text("name") ?? String(localized: "the account you choose")
        let to = card.text("name")
        if date > Dates.today {
            return Text("Recorded as a transfer from \(from) to \(to), so it isn't spending: it uses up what's set aside and lowers what the card owes. Dated ahead, it waits in Scheduled until then.")
        }
        return Text("Recorded as a transfer from \(from) to \(to), so it isn't spending: it uses up what's set aside and lowers what the card owes.")
    }

    private func option(_ id: String, _ title: String, _ detail: String, _ figure: Int64, _ currency: String) -> some View {
        Button {
            choice = id
            keypad = false
        } label: {
            HStack(spacing: 12) {
                mark(choice == id)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).foregroundStyle(Color.primary)
                    Text(detail).font(.footnote).foregroundStyle(Color.secondary)
                }
                Spacer()
                Text(money(figure, currency)).monospacedDigit().fontWeight(.semibold).foregroundStyle(Color.primary)
            }
        }
        .accessibilityAddTraits(choice == id ? .isSelected : [])
    }

    private func mark(_ on: Bool) -> some View {
        Image(systemName: on ? "checkmark.circle.fill" : "circle")
            .font(.title3)
            .foregroundStyle(on ? Color.urAccent : .secondary)
            .accessibilityHidden(true)
    }

    private var sourcePicker: some View {
        let currency = store.currency
        let balances = store.planningBalances
        return List(sources, id: \.idText) { account in
            let id = account.text("id")
            Button {
                from = id
                route = []
            } label: {
                HStack(spacing: 12) {
                    AccountIcon(account: account, balance: balances[id] ?? 0)
                    Text(account.text("name")).foregroundStyle(Color.primary)
                    Spacer()
                    Text(money(balances[id] ?? 0, currency)).monospacedDigit()
                        .foregroundStyle((balances[id] ?? 0) < 0 ? Color.urNegative : .primary)
                    if id == from { Image(systemName: "checkmark").foregroundStyle(Color.urAccent).fontWeight(.semibold) }
                }
            }
        }
        .groupedList()
        .navigationTitle("Pay from")
        .inlineTitle()
    }

    private func pay() {
        attempt($error) {
            guard let card else { throw AppError(String(localized: "This account was deleted.")) }
            guard sources.contains(where: { $0.text("id") == from }) else { throw AppError(String(localized: "Choose the account the payment comes from.")) }
            guard choice != "other" || pad.cents != nil else { throw AppError(String(localized: "Finish the sum first.")) }
            let value = amount
            guard value > 0 else { throw AppError(String(localized: "Enter an amount to pay.")) }
            guard Dates.day(date) != nil else { throw AppError(String(localized: "Choose a date.")) }
            let record: JSON = [
                "id": .string(paymentId), "accountId": .string(from), "date": .string(date), "payee": "", "memo": .string(memo),
                "amount": .int(-value), "status": "uncleared", "approved": true, "flag": .null,
                "transferAccountId": .string(accountId), "transferStatus": "uncleared",
                "splits": [["categoryId": .null, "unassigned": false, "amount": .int(-value)]],
            ]
            let name = card.text("name")
            let currency = store.currency
            let ahead = date > Dates.today
            let when = date
            saving = true
            Task {
                defer { saving = false }
                do {
                    try await store.planningCommand("saveTransaction", ["transaction": record, "through": .string(Dates.today)])
                    let paid = money(value, currency)
                    store.showToast(ahead ? String(localized: "Scheduled \(paid) to \(name) for \(Money.date(when)).")
                                    : String(localized: "Paid \(paid) to \(name)."), undo: true)
                    dismiss()
                } catch {
                    self.error = BudgetStore.readable(error)
                    // The same payment id stays, so a retry saves one payment, not two.
                }
            }
        }
    }
}
