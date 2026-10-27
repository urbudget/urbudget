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
}

/// A proposed change, shown in full before anything is applied: what changes, by how much, and
/// what each balance becomes. Its buttons follow Android's: a budget change is applied only from
/// here, after the service checks it again, and can be undone while it's the latest change.
struct AssistantProposalCard: View {
    var proposal: JSON
    var currency: String
    var busy: Bool
    var canUndo: Bool
    var actions: AssistantProposalActions
    @State private var page = 0

    private static let pageSize = 20

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
                settledTitle(status).font(.subheadline.weight(.semibold))
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

    private func settledTitle(_ status: String) -> Text {
        let title = proposal.text("title")
        return status == "applied" ? Text("Applied: \(title)") : Text("Discarded: \(title)")
    }

    private func card(_ status: String) -> some View {
        let card = proposal["card"]
        let rows = card.rows("rows")
        let pages = max(1, (rows.count + Self.pageSize - 1) / Self.pageSize)
        let current = min(page, pages - 1)
        let shown = Array(rows[min(rows.count, current * Self.pageSize)..<min(rows.count, (current + 1) * Self.pageSize)])
        let kind = proposal.text("kind")
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Label(card.text("kicker").isEmpty ? String(localized: "Proposal") : card.text("kicker"), systemImage: Self.symbol(kind))
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
                        rowView(row, total: false)
                    }
                    if card["total"].isObject {
                        Divider()
                        rowView(card["total"], total: true)
                    }
                }
                .background(Color.insetBackground, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
            if pages > 1 {
                AssistantPager(page: current, pages: pages, previous: String(localized: "Previous rows"), next: String(localized: "Next rows")) { page = $0 }
            }
            if card.long("more") > 0 {
                Text("\(card.long("more")) more rows").font(.caption).foregroundStyle(Color.secondary)
            }
            let check = card["check"]
            if !check.array.isEmpty {
                let passed = check[0].bool
                Label {
                    Text(passed ? check[1].string : String(localized: "Check: \(check[1].string)"))
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

    private func rowView(_ row: JSON, total: Bool) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            VStack(alignment: .leading, spacing: 3) {
                Text(row.text("label"))
                    .font(total ? .subheadline.weight(.semibold) : .subheadline)
                    .fixedSize(horizontal: false, vertical: true)
                if !row.text("detail").isEmpty {
                    Text(row.text("detail")).font(.footnote).foregroundStyle(Color.secondary).fixedSize(horizontal: false, vertical: true)
                }
                if row.has("before") || row.has("after") {
                    let after = row.long("after")
                    change(row.long("before"), after)
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
            }
        }
        .padding(.leading, row.flag("indent") ? 28 : 12)
        .padding(.trailing, 12)
        .padding(.vertical, 9)
        .accessibilityElement(children: .combine)
    }

    /// "20.00 → 5.00", kept left to right as one piece in Arabic so the arrow still reads from before to after.
    private func change(_ before: Int64, _ after: Int64) -> Text {
        let open = AppLanguage.isRightToLeft ? "\u{2066}" : ""
        let close = AppLanguage.isRightToLeft ? "\u{2069}" : ""
        let ink: Color = after < 0 ? .urNegative : .primary
        let amounts: Text = Text(money(before, currency)) + Text(" → ") + Text(money(after, currency)).foregroundStyle(ink)
        return Text(verbatim: open) + amounts + Text(verbatim: close)
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
                            primary("Apply to the import", "checkmark", applyFix)
                            Text("Each applies only to a line that hasn't changed.").font(.footnote).foregroundStyle(Color.secondary)
                        } else {
                            Text("Open the import these fixes are for, and apply them from there.").font(.footnote).foregroundStyle(Color.secondary)
                        }
                        HStack(spacing: 20) {
                            Button("Ask for changes", systemImage: "text.bubble", action: actions.refine).disabled(busy)
                            Button("Discard", role: .destructive, action: actions.discard).disabled(busy)
                        }
                    } else if status == "applied" {
                        Label("Applied to the import", systemImage: "checkmark.circle.fill").foregroundStyle(Color.urPositive)
                    }
                default:
                    switch status {
                    case "proposed", "undone":
                        primary("Apply reviewed changes", "checkmark", actions.apply)
                        HStack(spacing: 20) {
                            Button("Ask for changes", systemImage: "text.bubble", action: actions.refine).disabled(busy)
                            Button("Discard", role: .destructive, action: actions.discard).disabled(busy)
                        }
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

    private func primary(_ title: LocalizedStringKey, _ symbol: String, _ action: @escaping () -> Void) -> some View {
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
        .disabled(busy)
    }

    private func badge(_ text: LocalizedStringKey, _ color: Color) -> some View {
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
