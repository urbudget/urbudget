import SwiftUI

/// What a proposal card's buttons do.
struct AssistantProposalActions {
    var apply: () -> Void
    var undo: () -> Void
    var discard: () -> Void
    var refine: () -> Void
    var reviewImport: () -> Void
    var saveImport: () -> Void
    var report: () -> Void
    /// `true` opens the spreadsheet, `false` exports it.
    var spreadsheet: (Bool) -> Void
    /// Fixes for a bank file being imported go to that import, when the assistant is open over it.
    var applyFix: (() -> Void)? = nil
    /// The card made again with the person's choices on it (`ProposalChoices`).
    var revise: ((JSON) -> Void)? = nil
    /// Opens a row's editor, to give it another category or amount.
    var change: ((JSON) -> Void)? = nil
}

/// A proposed change, shown in full before anything is applied: what changes, by how much, and
/// what each balance becomes. Its buttons follow Android's: a budget change is applied only from
/// here, after the service checks it again, and can be undone while it's the latest change. While
/// it waits, each part of it can be left out (its check) or given another category or amount (its
/// pencil), and Apply makes what's left (the desktop's ProposalCard).
struct AssistantProposalCard: View {
    var proposal: JSON
    var currency: String
    var busy: Bool
    var canUndo: Bool
    var actions: AssistantProposalActions
    @State private var page = 0

    private static let pageSize = 20

    /// Its parts can be left out or changed: it waits to be applied, and its rows have keys.
    private var choosing: Bool {
        actions.revise != nil && proposal.text("status") == "proposed" && proposal["card"].rows("rows").contains { !$0.text("key").isEmpty }
    }

    var body: some View {
        let status = proposal.text("status")
        if status == "applied" || status == "discarded" {
            settled(status)
        } else {
            card(status)
        }
    }

    /// An applied or discarded proposal folds to one line, with Undo while it's still possible.
    private func settled(_ status: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Label {
                Text("\(status == "applied" ? "Applied" : "Discarded"): \(proposal.text("title"))").font(.subheadline.weight(.semibold))
            } icon: {
                Image(systemName: status == "applied" ? "checkmark.circle.fill" : "xmark.circle")
                    .foregroundStyle(status == "applied" ? Color.urPositive : .secondary)
            }
            if status == "applied" && canUndo {
                Button("Undo this proposal", systemImage: "arrow.uturn.backward", action: actions.undo)
                    .font(.subheadline.weight(.semibold))
                    .disabled(busy)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    private func card(_ status: String) -> some View {
        let card = proposal["card"]
        let rows = card.rows("rows")
        let pages = max(1, (rows.count + Self.pageSize - 1) / Self.pageSize)
        let current = min(page, pages - 1)
        let shown = Array(rows[min(rows.count, current * Self.pageSize)..<min(rows.count, (current + 1) * Self.pageSize)])
        let kind = proposal.text("kind")
        let choosing = self.choosing
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Label(card.text("kicker").isEmpty ? "Proposal" : card.text("kicker"), systemImage: Self.symbol(kind))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.urAccent)
                    .textCase(.uppercase)
                Spacer(minLength: 8)
                if status == "undone" { badge("Undone", .secondary) }
                if status == "failed" { badge("Could not apply", .urNegative) }
            }
            Text(proposal.text("title")).font(.headline).fixedSize(horizontal: false, vertical: true)
            let stats = card.rows("stats")
            if !stats.isEmpty {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 130), spacing: 12, alignment: .topLeading)], alignment: .leading, spacing: 10) {
                    ForEach(Array(stats.enumerated()), id: \.offset) { _, stat in
                        Figure(label: stat.text("label"),
                               value: stat["amount"].isNull ? stat.text("text") : money(stat.long("amount"), currency))
                            .accessibilityElement(children: .combine)
                    }
                }
            }
            if !shown.isEmpty || card["total"].isObject {
                VStack(spacing: 0) {
                    ForEach(Array(shown.enumerated()), id: \.offset) { index, row in
                        if index > 0 { Divider().padding(.leading, 12) }
                        rowView(row, total: false, choosing: choosing)
                    }
                    if card["total"].isObject {
                        Divider()
                        rowView(card["total"], total: true)
                    }
                }
                .background(Color.insetBackground, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            if pages > 1 {
                AssistantPager(page: current, pages: pages, previous: "Previous rows", next: "Next rows") { page = $0 }
            }
            if card.long("more") > 0 {
                Text("\(card.long("more")) more rows").font(.caption).foregroundStyle(Color.secondary)
            }
            let check = card["check"]
            if !check.array.isEmpty {
                let passed = check[0].bool
                Label {
                    Text(passed ? check[1].string : "Check: \(check[1].string)")
                } icon: {
                    Image(systemName: passed ? "checkmark.seal.fill" : "exclamationmark.triangle.fill")
                }
                .font(.footnote)
                .foregroundStyle(passed ? Color.urPositive : Color.urNegative)
            }
            if !card.text("warning").isEmpty {
                Notice(text: card.text("warning"), tone: "attention", icon: "exclamationmark.triangle.fill")
            }
            if !card.text("note").isEmpty {
                Text(card.text("note")).font(.footnote).foregroundStyle(Color.secondary)
            }
            if !proposal.text("problem").isEmpty && proposal.text("kind") != "fix" {
                Label(proposal.text("problem"), systemImage: "exclamationmark.octagon.fill")
                    .font(.footnote)
                    .foregroundStyle(Color.urNegative)
            }
            buttons(status, kind)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(Color.urAccent.opacity(0.22)))
    }

    /// A row. While the card waits (`choosing`), its check leaves it out or puts it back, and its
    /// pencil gives it another category or amount. A row left out stays, dimmed and struck through;
    /// an amount the person gave shows in the accent colour.
    private func rowView(_ row: JSON, total: Bool, choosing: Bool = false) -> some View {
        let key = row.text("key")
        let label = row.text("label")
        let skipped = row.flag("skipped")
        let edit = row["edit"]
        let pickable = choosing && !key.isEmpty
        let editable = pickable && !skipped && (edit.flag("category") || edit.flag("amount")) && actions.change != nil
        return HStack(spacing: 4) {
            if pickable {
                Button {
                    actions.revise?(ProposalChoices.toggle(proposal["choices"], key: key, skipped: skipped))
                } label: {
                    Image(systemName: skipped ? "circle" : "checkmark.circle.fill")
                        .font(.title3)
                        .foregroundStyle(skipped ? Color.secondary : Color.urAccent)
                        .frame(minWidth: 32, minHeight: 32)
                }
                .buttonStyle(.borderless)
                .disabled(busy)
                .accessibilityLabel("Apply: \(label)")
                .accessibilityAddTraits(skipped ? [] : .isSelected)
            }
            rowText(row, total: total)
            if editable {
                Button { actions.change?(row) } label: {
                    Image(systemName: "pencil").frame(minWidth: 32, minHeight: 32)
                }
                .buttonStyle(.borderless)
                .disabled(busy)
                .accessibilityLabel("Change \(label)")
            }
        }
        .padding(.leading, pickable ? 4 : 0)
        .padding(.trailing, editable ? 4 : 0)
        .opacity(skipped ? 0.5 : 1)
    }

    private func rowText(_ row: JSON, total: Bool) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            VStack(alignment: .leading, spacing: 3) {
                Text(row.text("label"))
                    .font(total ? .subheadline.weight(.semibold) : .subheadline)
                    .strikethrough(row.flag("skipped"))
                    .fixedSize(horizontal: false, vertical: true)
                if !row.text("detail").isEmpty {
                    Text(row.text("detail")).font(.footnote).foregroundStyle(Color.secondary).fixedSize(horizontal: false, vertical: true)
                }
                if row.has("before") || row.has("after") {
                    let after = row.long("after")
                    (Text(money(row.long("before"), currency)) + Text(" → ") + Text(money(after, currency)).foregroundStyle(after < 0 ? Color.urNegative : Color.primary))
                        .font(.footnote)
                        .monospacedDigit()
                        .foregroundStyle(Color.secondary)
                        .accessibilityLabel("From \(money(row.long("before"), currency)) to \(money(after, currency))")
                }
                if !row.text("badge").isEmpty {
                    let tone: Color = row.text("tone") == "warn" ? .urWarning : .urAccent
                    Text(row.text("badge"))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(tone)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 2)
                        .background(tone.opacity(0.12), in: Capsule())
                }
            }
            Spacer(minLength: 8)
            if row.has("amount") {
                Text(money(row.long("amount"), currency))
                    .font(total ? .subheadline.weight(.bold) : .subheadline.weight(.semibold))
                    .monospacedDigit()
                    .lineLimit(1)
                    .minimumScaleFactor(0.75)
                    .foregroundStyle(row.flag("edited") ? Color.urAccent : Color.primary)
                    .accessibilityValue(row.flag("edited") ? "Changed by you" : "")
            }
        }
        .padding(.leading, row.flag("indent") ? 28 : 12)
        .padding(.trailing, 12)
        .padding(.vertical, 9)
        .accessibilityElement(children: .combine)
    }

    @ViewBuilder
    private func buttons(_ status: String, _ kind: String) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if status == "failed" {
                Button("Dismiss", action: actions.discard).disabled(busy)
            } else {
                switch kind {
                case "report":
                    primary("Read report · Print / PDF", "doc.text.magnifyingglass", actions.report)
                case "import":
                    primary("Review \(proposal.long("rows")) import rows", "tray.and.arrow.down", actions.reviewImport)
                    Button("Export converted CSV", systemImage: "square.and.arrow.up", action: actions.saveImport)
                case "spreadsheet":
                    primary("Open spreadsheet", "tablecells", { actions.spreadsheet(true) })
                    Button("Export Excel file", systemImage: "square.and.arrow.up") { actions.spreadsheet(false) }
                case "fix":
                    // The import applies them, each only to a line that hasn't changed.
                    if status == "proposed" || status == "undone" {
                        if let applyFix = actions.applyFix {
                            primary(partial.map { "\($0) to the import" } ?? "Apply to the import", "checkmark", applyFix, enabled: !nothingChosen)
                            Text("Each applies only to a line that hasn't changed.").font(.footnote).foregroundStyle(Color.secondary)
                        } else {
                            Text("Open the import these fixes are for, and apply them from there.").font(.footnote).foregroundStyle(Color.secondary)
                        }
                        HStack(spacing: 20) {
                            Button("Ask for changes", systemImage: "text.bubble", action: actions.refine).disabled(busy)
                            Button("Discard", role: .destructive, action: actions.discard).disabled(busy)
                        }
                        original
                    } else if status == "applied" {
                        Label("Applied to the import", systemImage: "checkmark.circle.fill").foregroundStyle(Color.urPositive)
                    }
                default:
                    switch status {
                    case "proposed", "undone":
                        primary(partial ?? "Apply reviewed changes", "checkmark", actions.apply, enabled: !nothingChosen)
                        HStack(spacing: 20) {
                            Button("Ask for changes", systemImage: "text.bubble", action: actions.refine).disabled(busy)
                            Button("Discard", role: .destructive, action: actions.discard).disabled(busy)
                        }
                        original
                    case "applied":
                        Label("Applied", systemImage: "checkmark.circle.fill").foregroundStyle(Color.urPositive)
                        if canUndo { Button("Undo this proposal", systemImage: "arrow.uturn.backward", action: actions.undo).disabled(busy) }
                    case "discarded":
                        Text("Discarded").font(.footnote).foregroundStyle(Color.secondary)
                    default:
                        EmptyView()
                    }
                }
            }
            if kind == "report" && proposal.long("sheets") > 0 {
                Button("Export report tables to Excel", systemImage: "tablecells") { actions.spreadsheet(false) }
            }
            if status != "failed" && ["report", "spreadsheet", "import"].contains(kind) {
                Button("Discard", role: .destructive, action: actions.discard).disabled(busy)
            }
        }
        .font(.subheadline.weight(.semibold))
        .buttonStyle(.borderless)
    }

    private func primary(_ title: String, _ symbol: String, _ action: @escaping () -> Void, enabled: Bool = true) -> some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if busy { ProgressView().controlSize(.small).tint(.white) } else { Image(systemName: symbol) }
                Text(title)
            }
            .font(.subheadline.weight(.semibold))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
        }
        .buttonStyle(.borderedProminent)
        .disabled(busy || !enabled)
    }

    /// "Apply 2 of 3" while some of the card's rows are left out.
    private var partial: String? { ProposalChoices.partial(proposal["card"].rows("rows"), choosing: choosing) }

    /// Every row is left out: nothing to apply.
    private var nothingChosen: Bool {
        choosing && !proposal["card"].rows("rows").contains { !$0.text("key").isEmpty && !$0.flag("skipped") }
    }

    /// Back to the card as the assistant made it, once the person has changed it.
    @ViewBuilder private var original: some View {
        if let revise = actions.revise, proposal.has("choices") && !proposal["choices"].isNull {
            Button("Use the assistant's version", systemImage: "arrow.uturn.backward") { revise(ProposalChoices.original) }.disabled(busy)
        }
    }

    private func badge(_ text: String, _ color: Color) -> some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(color.opacity(0.12), in: Capsule())
    }

    private static func symbol(_ kind: String) -> String {
        switch kind {
        case "report": return "doc.text"
        case "import": return "tray.and.arrow.down"
        case "spreadsheet": return "tablecells"
        case "fix": return "wrench.and.screwdriver"
        case "transactions": return "list.bullet.rectangle"
        case "payees": return "person.text.rectangle"
        default: return "arrow.left.arrow.right"
        }
    }
}

/// A card row to change: the proposal it's on, and the row.
struct AssistantRowChange {
    var proposal: JSON
    var row: JSON
}

/// A card row's category and/or amount, as its `edit` allows (the desktop's RowEditor). The
/// category starts at the row's (none is Unassigned, Ready to Assign), the amount at the row's
/// signed amount; one typed without a sign keeps the row's. Saving with nothing changed just closes.
struct AssistantRowEditor: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var row: JSON
    var onSave: (JSON) -> Void
    @State private var category: String
    @State private var amount: String
    @State private var error: String?
    @State private var route: [Route] = []

    private enum Route: Hashable { case category }

    init(row: JSON, onSave: @escaping (JSON) -> Void) {
        self.row = row
        self.onSave = onSave
        let had = row["edit"].text("categoryId")
        _category = State(initialValue: had.isEmpty ? Unassigned : had)
        _amount = State(initialValue: row["amount"].isNull ? "" : Money.editable(row.long("amount")))
    }

    var body: some View {
        let edit = row["edit"]
        let label = row.text("label")
        NavigationStack(path: $route) {
            Form {
                Section {
                    if edit.flag("category") {
                        NavigationLink(value: Route.category) {
                            PlanningPickerRow(icon: "tag", label: "Category", value: planningCategoryName(store.budget, category), placeholder: "Choose a category")
                        }
                    }
                    if edit.flag("amount") {
                        TextField("Amount", text: $amount)
                            .decimalKeyboard()
                            .accessibilityLabel("Amount for \(label)")
                            .onChange(of: amount) { _, _ in error = nil }
                    }
                } footer: {
                    if !row.text("detail").isEmpty { Text(row.text("detail")) }
                }
                if let error { FormErrorView(message: error) }
            }
            .navigationTitle("Change \(label)")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("Save", action: save).fontWeight(.semibold) }
            }
            .navigationDestination(for: Route.self) { _ in
                CategoryPicker(title: "Category for \(label)", month: store.month, selected: category, includeUnassigned: true, includePayment: false) { picked in
                    category = picked
                    route = []
                }
            }
        }
    }

    private func save() {
        do {
            if let change = try ProposalChoices.edit(row, category: category == Unassigned ? nil : category, amount: amount) { onSave(change) }
            dismiss()
        } catch {
            self.error = BudgetStore.readable(error)
        }
    }
}
