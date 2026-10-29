import SwiftUI

private enum MoveEnd: Hashable { case from, to }

/// Moving money, as a sheet: from one category (or Unassigned) to another, the amount on the keypad.
/// Covering overspending opens with the overspent amount, taken from Unassigned when it has money.
struct MoveMoneySheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var request: MoveRequest

    @State private var from = ""
    @State private var to = ""
    @State private var pad = PadState(nil)
    @State private var keypad = false
    @State private var route: [MoveEnd] = []
    @State private var error: String?
    @State private var title = String(localized: "Move money")
    @State private var started = false
    @State private var saving = false

    private func available(_ id: String) -> Int64 {
        if id == Unassigned { return store.summary?.long("readyToAssign") ?? 0 }
        return store.summary?.rows("categories").first { $0.text("id") == id }?.long("available") ?? 0
    }

    private func name(_ id: String) -> String {
        if id == Unassigned { return String(localized: "Unassigned") }
        return store.budget?.row("categories", id: id)?.text("name") ?? String(localized: "Choose a category")
    }

    var body: some View {
        let currency = store.currency
        NavigationStack(path: $route) {
            Form {
                Section {
                    Button {
                        keypad = true
                        pad.fresh = true
                    } label: {
                        PadAmount(pad: pad, currency: currency, color: keypad ? .urAccent : .primary)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 8)
                    }
                    .listRowBackground(keypad ? Color.urAccent.opacity(0.12) : nil)
                }
                Section {
                    NavigationLink(value: MoveEnd.from) { end("From", from, currency) }
                    Button {
                        (from, to) = (to, from)
                    } label: {
                        Label("Swap", systemImage: "arrow.up.arrow.down").frame(maxWidth: .infinity)
                    }
                    NavigationLink(value: MoveEnd.to) { end("To", to, currency) }
                }
                if let error { FormErrorView(message: error) }
                if !keypad {
                    Section {
                        Button(action: save) {
                            HStack(spacing: 8) {
                                if saving { ProgressView() }
                                Text(from == Unassigned ? "Assign" : "Move").fontWeight(.semibold)
                            }
                            .frame(maxWidth: .infinity)
                        }
                        .disabled(saving)
                    }
                }
            }
            .interactiveDismissDisabled(saving)
            .navigationTitle(title)
            .inlineTitle()
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } } }
            .navigationDestination(for: MoveEnd.self) { end in
                CategoryPicker(title: end == .from ? String(localized: "Move from") : String(localized: "Move to"), month: store.month,
                               selected: end == .from ? from : to,
                               includeUnassigned: true, includePayment: false) { id in
                    if end == .from { from = id } else { to = id }
                    route.removeLast()
                }
            }
            .safeAreaInset(edge: .bottom) {
                if keypad { AmountKeypad(pad: pad) { keypad = false }.transition(.move(edge: .bottom)) }
            }
            .animation(.snappy(duration: 0.25), value: keypad)
        }
        .presentationDetents([.large])
        .retainingEditorDraft("moveMoney", values: ["from": .string(from), "to": .string(to), "amountExpression": .string(pad.text), "month": .string(store.month)])
        .onAppear(perform: start)
    }

    private func start() {
        guard !started else { return }
        started = true
        let start = available(request.category)
        let rta = store.summary?.long("readyToAssign") ?? 0
        if request.direction == "to" { from = request.category }
        else if start < 0 && rta > 0 && request.category != Unassigned { from = Unassigned }
        if request.direction == "from" { to = request.category }
        pad = PadState(start < 0 ? -start : max(0, start))
        if request.category == Unassigned && start >= 0 { title = String(localized: "Assign money") }
        else if start < 0 && request.category != Unassigned { title = String(localized: "Cover overspending") }
    }

    private func end(_ label: LocalizedStringKey, _ id: String, _ currency: String) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(label).font(.footnote).foregroundStyle(Color.secondary)
                Text(id.isEmpty ? String(localized: "Choose a category") : name(id)).fontWeight(.semibold)
                    .foregroundStyle(id.isEmpty ? Color.urAccent : .primary).lineLimit(1)
            }
            Spacer()
            if !id.isEmpty {
                let amount = available(id)
                AvailablePill(text: money(amount, currency), status: amount < 0 ? "negative" : amount > 0 ? "positive" : "zero")
            }
        }
    }

    private func save() {
        attempt($error, saving: $saving) {
            guard !from.isEmpty, !to.isEmpty else { throw AppError(String(localized: "Choose where the money comes from and where it goes.")) }
            guard from != to else { throw AppError(String(localized: "Choose two different places.")) }
            guard let amount = pad.cents else { throw AppError(String(localized: "Finish the sum first.")) }
            guard amount > 0 else { throw AppError(String(localized: "Enter an amount to move.")) }
            try await store.committingWorkflow(["type": "moveMoney", "month": .string(store.month), "from": .string(from), "to": .string(to), "amount": .int(amount)])
            store.showToast(String(localized: "Moved \(money(amount, store.currency)) from \(name(from)) to \(name(to))."), undo: true)
            dismiss()
        }
    }
}
