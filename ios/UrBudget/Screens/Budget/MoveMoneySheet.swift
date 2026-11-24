import SwiftUI

private enum MoveEnd: Hashable { case from, to }

/// What covering an overspending of `overspent` takes from a source that has `has`: all of it when
/// the source has enough, otherwise everything the source has. With nothing to give (or no source
/// chosen yet) it's the whole overspending, so the amount still says what's needed.
func coverAmount(_ overspent: Int64, has: Int64?) -> Int64 {
    if let has, has > 0, has < overspent { return has }
    return overspent
}

/// The amount the Move money sheet opens with, for a category (or Unassigned) that has `start`:
/// `cover` when it's covering overspending; what's short when it's below zero; nothing (an empty
/// keypad) when assigning from Unassigned, since all of it rarely belongs in one category; otherwise
/// what the category has to move.
func moveStartAmount(fromUnassigned: Bool, start: Int64, cover: Int64?) -> Int64? {
    if let cover { return cover }
    if start < 0 { return -start }
    return fromUnassigned ? nil : start
}

/// Moving money, as a sheet: from one category (or Unassigned) to another, the amount on the keypad.
/// Assigning from Unassigned opens with no amount (`moveStartAmount`) and the keypad up.
/// Covering overspending takes from Unassigned when it has money, and opens with as much of the
/// overspending as the source can give (`coverAmount`); choosing another source sets the amount
/// again, until one has been typed.
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
    /// The overspending this sheet covers, or 0 when it isn't covering one.
    @State private var overspent: Int64 = 0
    /// The amount the sheet last put in: while it's still there, a new source puts in its own.
    @State private var suggested: Int64?

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
            CompactForm {
                Section {
                    Button {
                        keypad = true
                        pad.fresh = true
                    } label: {
                        PadAmount(pad: pad, currency: currency, color: keypad ? .urAccent : .primary)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 8)
                    }
                    .listRowBackground(keypad ? Color.urAccent.opacity(0.12) : Color.cardBackground)
                }
                Section {
                    NavigationLink(value: MoveEnd.from) { end("From", from, currency) }
                    Button {
                        (from, to) = (to, from)
                    } label: {
                        Label("Swap", systemImage: "arrow.up.arrow.down").font(.subheadline.weight(.semibold)).frame(maxWidth: .infinity)
                    }
                    .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 2, trailing: 16))
                    NavigationLink(value: MoveEnd.to) { end("To", to, currency) }
                }
                if let error { FormErrorView(message: error) }
                if !keypad {
                    Section {
                        Button(action: save) {
                            HStack(spacing: 8) {
                                if saving { ProgressView().tint(.white) }
                                Text(from == Unassigned ? "Assign" : "Move").fontWeight(.semibold)
                            }
                            .frame(maxWidth: .infinity)
                        }
                        .primaryAction()
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
        .sheetDetents([.large])
        .retainingEditorDraft("moveMoney", values: ["from": .string(from), "to": .string(to), "amountExpression": .string(pad.text), "month": .string(store.month)])
        .onAppear(perform: start)
        .onChange(of: from) { _, source in
            guard overspent > 0, pad.cents == suggested else { return }
            let next = covered(source)
            if next != pad.cents {
                pad.set(next)
                suggested = next
            }
        }
    }

    /// What covering takes from `source`, as much of the overspending as it can give.
    private func covered(_ source: String) -> Int64 {
        coverAmount(overspent, has: source.isEmpty ? nil : available(source))
    }

    private func start() {
        guard !started else { return }
        started = true
        let start = available(request.category)
        let rta = store.summary?.long("readyToAssign") ?? 0
        let source = request.direction == "to" ? request.category : start < 0 && rta > 0 && request.category != Unassigned ? Unassigned : ""
        from = source
        if request.direction == "from" { to = request.category }
        // Covering this category's overspending: the amount is what the source can give of it.
        let over: Int64 = start < 0 && request.direction != "to" && request.category != Unassigned ? -start : 0
        overspent = over
        let opening = moveStartAmount(fromUnassigned: request.category == Unassigned, start: start,
                                      cover: over > 0 ? coverAmount(over, has: source.isEmpty ? nil : available(source)) : nil)
        pad = PadState(opening)
        suggested = opening
        // With no amount to start from, the keypad is up and waiting for one.
        keypad = opening == nil
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
