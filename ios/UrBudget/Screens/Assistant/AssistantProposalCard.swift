import SwiftUI

/// What a proposal card's buttons do.
struct AssistantProposalActions {
    var apply: () -> Void
    var undo: () -> Void
    /// Sets the card aside; a thing the assistant made is removed the same way.
    var discard: () -> Void
    /// Puts a discarded, removed or replaced card back.
    var restore: () -> Void
    var refine: () -> Void
    /// Asks the assistant to prepare again a card that couldn't be applied.
    var askAgain: () -> Void
    /// Goes to what the card names: its transactions, or the Budget tab on its month.
    var show: () -> Void
    /// Sends one of a question's answers as the person's message.
    var answer: (String) -> Void
    var reviewImport: () -> Void
    var saveImport: () -> Void
    var report: () -> Void
    /// `true` opens the spreadsheet, `false` exports it.
    var spreadsheet: (Bool) -> Void
    /// Fixes for a bank file being imported go to that import, when the assistant is open over it.
    var applyFix: (() -> Void)? = nil
    /// The person's choices on the card (`ProposalChoices`), all of them: kept at once, and
    /// sent to the service a moment later.
    var revise: ((JSON) -> Void)? = nil
    /// Opens a row's editor, to give it another category or amount.
    var change: ((JSON) -> Void)? = nil
}

/// A card the assistant made, drawn as the service's view of it says: a change shown in full
/// before anything is applied (what changes, by how much, and what each balance becomes), a
/// thing it made (a report, a spreadsheet, a converted file, figures), or a question with
/// answers to tap. Its buttons are the actions the service offers, in its order, the first as
/// the main one. While a change waits, each part of it can be left out (its check) or given
/// another category or amount (its pencil), which shows at once, and Apply makes what's left.
/// A card that's done with folds to a line that opens.
struct AssistantProposalCard: View {
    var proposal: JSON
    var currency: String
    /// An action of this card is running: its buttons and checks wait. Other cards carry on.
    var busy: Bool
    /// The action running on this card ("apply", "report", "import", "open"…): the button that
    /// asked for it spins.
    var working: String? = nil
    /// The person's choices on this card that the service doesn't have yet, shown at once.
    var choices: JSON? = nil
    /// This card's change is the one the budget's Undo would take back.
    var canUndo: Bool
    /// A message can be sent now: a question's answers and Ask for a new version send one.
    var canSend = true
    /// Names the category the person gave a row (nil: Ready to Assign), until the card says it.
    var categoryName: (String?) -> String = { _ in "" }
    var actions: AssistantProposalActions
    @State private var page = 0
    /// A finished card's rows are open to read.
    @State private var opened = false

    private static let pageSize = 20

    /// The person's choices as they stand: those not sent yet, else the ones the card was made with.
    private var chosen: JSON { choices ?? proposal["choices"] }

    /// The card's rows as they stand: the service's, with the choices it doesn't have yet laid
    /// over them.
    private var rows: [JSON] {
        let all = proposal["card"].rows("rows")
        guard let choices else { return all }
        return ProposalChoices.shown(all, pending: choices, sent: proposal["choices"], category: categoryName)
    }

    /// Its parts can be left out or changed: it waits for this app to apply it, and its rows
    /// have keys.
    private func choosing(_ offered: [AssistantCardAction]) -> Bool {
        actions.revise != nil && AssistantCard.known(proposal) && proposal.text("status") == "proposed"
            && (offered.contains(.apply) || offered.contains(.applyToImport))
            && proposal["card"].rows("rows").contains { !$0.text("key").isEmpty }
    }

    var body: some View {
        if AssistantCard.finished(proposal) {
            finished
        } else {
            waiting
        }
    }

    /// A card that's done with, as one line: what became of it, then its title, and the actions
    /// it still offers. The chevron opens its rows to read, as they were.
    private var finished: some View {
        let status = proposal.text("status")
        let card = proposal["card"]
        let title = proposal.text("title")
        let line = AssistantCard.lineActions(proposal, canUndo: canUndo)
        let inside = AssistantCard.openedActions(proposal)
        return VStack(alignment: .leading, spacing: 10) {
            Button {
                withAnimation(.easeOut(duration: 0.2)) { opened.toggle() }
            } label: {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: Self.finishedSymbol(proposal))
                        .foregroundStyle(status == "applied" ? Color.urPositive : Color.secondary)
                    VStack(alignment: .leading, spacing: 2) {
                        if let asked = AssistantCard.asked(proposal) {
                            Text(asked).font(.subheadline.weight(.semibold)).foregroundStyle(Color.primary)
                        } else {
                            Text(AssistantCard.finishedWords(proposal)).font(.footnote).foregroundStyle(Color.secondary)
                            Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(Color.primary)
                        }
                    }
                    .multilineTextAlignment(.leading)
                    Spacer(minLength: 8)
                    // Points the way text reads while folded (mirrored right to left), down when open.
                    Image(systemName: "chevron.right")
                        .font(.footnote.weight(.bold))
                        .rotationEffect(.degrees(opened ? 90 : 0))
                        .flipsForRightToLeftLayoutDirection(true)
                        .foregroundStyle(Color.secondary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint(opened ? Text("Collapse \(title)") : Text("Expand \(title)"))
            if !line.isEmpty {
                HStack(spacing: 20) {
                    ForEach(line, id: \.self) { action in
                        button(action, rows: [], choosing: false)
                    }
                }
                .font(.subheadline.weight(.semibold))
                .buttonStyle(.borderless)
            }
            if opened {
                details(card, rows: card.rows("rows"), choosing: false)
                notes(card)
                if !inside.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(inside, id: \.self) { action in
                            button(action, rows: card.rows("rows"), choosing: false)
                        }
                    }
                    .font(.subheadline.weight(.semibold))
                    .buttonStyle(.borderless)
                }
            } else if status == "applied" {
                // How it was applied (an import that took only some of its fixes) shows without
                // opening it.
                messageStrip
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    /// A card that waits on the person: to apply it, answer it, or open what it made.
    private var waiting: some View {
        let card = proposal["card"]
        let kind = proposal.text("kind")
        let offered = AssistantCard.actions(proposal)
        let rows = self.rows
        let choosing = self.choosing(offered)
        // Deleting is drawn in the app's destructive colour.
        let accent: Color = kind == "delete" ? .urNegative : .urAccent
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Label(card.text("kicker").isEmpty ? String(localized: "Proposal") : card.text("kicker"), systemImage: Self.symbol(kind))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(accent)
                    .textCase(.uppercase)
                Spacer(minLength: 8)
                // A thing the assistant made has no state to badge.
                if proposal.text("status") == "failed" && !proposal.flag("made") { badge("Could not apply", .urNegative) }
                if offered.contains(.remove) { menu }
            }
            Text(proposal.text("title")).font(.headline).fixedSize(horizontal: false, vertical: true)
            if offered.contains(.answer) {
                answers(rows)
            } else {
                details(card, rows: rows, choosing: choosing)
            }
            notes(card)
            if !AssistantCard.known(proposal) {
                Text("Update UrBudget to use this card.").font(.footnote).foregroundStyle(Color.secondary)
            }
            buttons(rows, choosing: choosing)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(accent.opacity(0.22)))
    }

    /// What the card is of: its figures, its rows a page at a time with its total, and its check.
    @ViewBuilder
    private func details(_ card: JSON, rows: [JSON], choosing: Bool) -> some View {
        let pages = max(1, (rows.count + Self.pageSize - 1) / Self.pageSize)
        let current = min(page, pages - 1)
        let shown = Array(rows[min(rows.count, current * Self.pageSize)..<min(rows.count, (current + 1) * Self.pageSize)])
        // A figures card draws a bar on each row: its share of the card's largest amount.
        let shares = proposal.text("kind") == "figures" ? AssistantCard.barShares(rows) : []
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
        if choosing { selection(rows) }
        if !shown.isEmpty || card["total"].isObject {
            VStack(spacing: 0) {
                ForEach(Array(shown.enumerated()), id: \.offset) { index, row in
                    if index > 0 { Divider().padding(.leading, 12) }
                    rowView(row, total: false, choosing: choosing, share: Self.share(shares, current * Self.pageSize + index))
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
    }

    /// A row's share of the card's largest amount; nil for a card that draws no bars.
    private static func share(_ shares: [Double], _ place: Int) -> Double? {
        shares.indices.contains(place) ? shares[place] : nil
    }

    /// What the card says under its rows: the warning that's part of what it proposes, its
    /// note, and what it has to say about itself. That last is said nowhere else.
    @ViewBuilder
    private func notes(_ card: JSON) -> some View {
        if !card.text("warning").isEmpty {
            Notice(text: card.text("warning"), tone: "attention", icon: "exclamationmark.triangle.fill")
        }
        if !card.text("note").isEmpty {
            Text(card.text("note")).font(.footnote).foregroundStyle(Color.secondary)
        }
        messageStrip
    }

    /// What the card has to say about itself, as a strip in its tone: quiet when it was only
    /// brought up to date, the app's warning for something to check, and its error when the
    /// card can't be applied.
    @ViewBuilder
    private var messageStrip: some View {
        if let message = AssistantCard.message(proposal) {
            switch message.tone {
            case "error":
                Notice(text: message.text, tone: "negative", icon: "exclamationmark.octagon.fill")
            case "warn":
                Notice(text: message.text, tone: "attention", icon: "exclamationmark.triangle.fill")
            default:
                Label(message.text, systemImage: "info.circle")
                    .font(.footnote)
                    .foregroundStyle(Color.secondary)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.insetBackground, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
        }
    }

    /// A question's answers, each a button across the card that sends it as the person's message.
    private func answers(_ rows: [JSON]) -> some View {
        VStack(spacing: 8) {
            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                Button {
                    actions.answer(row.text("label"))
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row.text("label")).font(.subheadline.weight(.semibold))
                        if !row.text("detail").isEmpty {
                            Text(row.text("detail")).font(.footnote).foregroundStyle(Color.secondary)
                        }
                    }
                    .multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.vertical, 4)
                }
                .buttonStyle(.bordered)
                .disabled(busy || !canSend)
            }
        }
    }

    /// Shortcuts on a card with more than five rows to choose from: all of them, none, or only
    /// the ones marked to check.
    @ViewBuilder
    private func selection(_ rows: [JSON]) -> some View {
        if let revise = actions.revise, rows.filter({ !$0.text("key").isEmpty }).count > 5 {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 20) { selectionButtons(rows, revise) }
                VStack(alignment: .leading, spacing: 8) { selectionButtons(rows, revise) }
            }
            .font(.footnote.weight(.semibold))
            .buttonStyle(.borderless)
            .disabled(busy)
        }
    }

    @ViewBuilder
    private func selectionButtons(_ rows: [JSON], _ revise: @escaping (JSON) -> Void) -> some View {
        let chosen = self.chosen
        Button("Select all") { revise(ProposalChoices.select(chosen, rows: rows) { _ in true }) }
        Button("Select none") { revise(ProposalChoices.select(chosen, rows: rows) { _ in false }) }
        if rows.contains(where: { !$0.text("key").isEmpty && $0.text("tone") == "warn" }) {
            Button("Only the ones to check") { revise(ProposalChoices.select(chosen, rows: rows) { $0.text("tone") == "warn" }) }
        }
    }

    /// The card's menu: Remove, for a thing the assistant made.
    private var menu: some View {
        Menu {
            Button("Remove", systemImage: "trash", role: .destructive, action: actions.discard)
        } label: {
            Image(systemName: "ellipsis")
                .font(.footnote.weight(.bold))
                .foregroundStyle(Color.secondary)
                .frame(width: 36, height: 28)
                .contentShape(Rectangle())
        }
        .disabled(busy)
        .accessibilityLabel("More actions")
    }

    /// A row. While the card waits (`choosing`), its check leaves it out or puts it back, and its
    /// pencil gives it another category or amount: both show at once, from the choices as they
    /// stand. A row left out stays, dimmed and struck through; an amount the person gave shows
    /// in the accent colour. `share` is a figures row's bar.
    private func rowView(_ row: JSON, total: Bool, choosing: Bool = false, share: Double? = nil) -> some View {
        let key = row.text("key")
        let label = row.text("label")
        let skipped = row.flag("skipped")
        let edit = row["edit"]
        let pickable = choosing && !key.isEmpty
        let editable = pickable && !skipped && (edit.flag("category") || edit.flag("amount")) && actions.change != nil
        // The check and the pencil are 44 points across, so the text beside them needs no margin of its own.
        return HStack(spacing: 0) {
            if pickable {
                Button {
                    actions.revise?(ProposalChoices.toggle(chosen, key: key, skipped: skipped))
                } label: {
                    Image(systemName: skipped ? "circle" : "checkmark.circle.fill")
                        .font(.title3)
                        .foregroundStyle(skipped ? Color.secondary : Color.urAccent)
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .disabled(busy)
                .accessibilityLabel("Apply: \(label)")
                .accessibilityAddTraits(skipped ? [] : .isSelected)
            }
            rowText(row, total: total, leading: (pickable ? 0 : 12) + (row.flag("indent") ? 16 : 0), trailing: editable ? 0 : 12, share: share)
            if editable {
                Button { actions.change?(row) } label: {
                    Image(systemName: "pencil")
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
                .disabled(busy)
                .accessibilityLabel("Change \(label)")
            }
        }
        .opacity(skipped ? 0.5 : 1)
    }

    private func rowText(_ row: JSON, total: Bool, leading: CGFloat, trailing: CGFloat, share: Double?) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(row.text("label"))
                        .font(total ? .subheadline.weight(.semibold) : .subheadline)
                        .strikethrough(row.flag("skipped"))
                        .fixedSize(horizontal: false, vertical: true)
                    if !row.text("detail").isEmpty {
                        Text(row.text("detail")).font(.footnote).foregroundStyle(Color.secondary).fixedSize(horizontal: false, vertical: true)
                    }
                    // The category the person just gave it, until the card names it itself.
                    if !row.text("given").isEmpty {
                        Text(row.text("given")).font(.footnote.weight(.semibold)).foregroundStyle(Color.urAccent).fixedSize(horizontal: false, vertical: true)
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
                        .foregroundStyle(row.flag("edited") ? Color.urAccent : Color.primary)
                        .accessibilityValue(row.flag("edited") ? Text("Changed by you") : Text(verbatim: ""))
                }
            }
            if let share { bar(share) }
        }
        .padding(.leading, leading)
        .padding(.trailing, trailing)
        .padding(.vertical, 9)
        .accessibilityElement(children: .combine)
    }

    /// A figures row's bar, as long as the row's share of the card's largest amount. It runs
    /// left to right in every language, as the app's other bars and charts do.
    private func bar(_ share: Double) -> some View {
        Capsule().fill(Color.primary.opacity(0.08))
            .frame(height: 4)
            .overlay(alignment: .leading) {
                GeometryReader { proxy in
                    Capsule().fill(Color.urAccent).frame(width: proxy.size.width * CGFloat(min(1, max(0, share))))
                }
            }
            .environment(\.layoutDirection, .leftToRight)
            .accessibilityHidden(true)
    }

    /// "20.00 → 5.00", kept left to right as one piece in Arabic so the arrow still reads from before to after.
    private func change(_ before: Int64, _ after: Int64) -> Text {
        let open = AppLanguage.isRightToLeft ? "\u{2066}" : ""
        let close = AppLanguage.isRightToLeft ? "\u{2069}" : ""
        let ink: Color = after < 0 ? .urNegative : .primary
        let amounts: Text = Text(money(before, currency)) + Text(" → ") + Text(money(after, currency)).foregroundStyle(ink)
        return Text(verbatim: open) + amounts + Text(verbatim: close)
    }

    /// The card's buttons, as the service offers them: the first as the main one, the others
    /// under it in the service's order, side by side where they fit.
    @ViewBuilder
    private func buttons(_ rows: [JSON], choosing: Bool) -> some View {
        let main = AssistantCard.main(proposal)
        let offered = AssistantCard.buttons(proposal, canUndo: canUndo)
        let others = offered.filter { $0 != main }
        if !offered.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                if let main, offered.contains(main) { button(main, rows: rows, choosing: choosing) }
                if !others.isEmpty {
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 20) {
                            ForEach(others, id: \.self) { action in
                                button(action, rows: rows, choosing: choosing)
                            }
                        }
                        VStack(alignment: .leading, spacing: 8) {
                            ForEach(others, id: \.self) { action in
                                button(action, rows: rows, choosing: choosing)
                            }
                        }
                    }
                }
            }
            .font(.subheadline.weight(.semibold))
            .buttonStyle(.borderless)
        }
    }

    /// One action the card offers, drawn as its button. What a card is for (applying it,
    /// opening what it made, asking for it again) is a wide button; the rest are quiet ones.
    @ViewBuilder
    private func button(_ action: AssistantCardAction, rows: [JSON], choosing: Bool) -> some View {
        switch action {
        case .apply:
            applyButton(rows, choosing: choosing)
        case .applyToImport:
            importButton(rows, choosing: choosing)
        case .preview:
            primary("Read report · Print / PDF", "doc.text.magnifyingglass", actions.report, spins: "report")
        case .open:
            primary("Open spreadsheet", "tablecells", { actions.spreadsheet(true) }, spins: "open")
        case .reviewImport:
            primary("Review \(proposal.long("rows")) import rows", "tray.and.arrow.down", actions.reviewImport, spins: "import")
        case .askAgain:
            primary("Ask for a new version", "arrow.clockwise", actions.askAgain, enabled: canSend, spins: "")
        case .excel:
            Button("Export report tables to Excel", systemImage: "tablecells") { actions.spreadsheet(false) }.disabled(busy)
        case .save:
            Button("Export Excel file", systemImage: "square.and.arrow.up") { actions.spreadsheet(false) }.disabled(busy)
        case .saveCsv:
            Button("Export converted CSV", systemImage: "square.and.arrow.up", action: actions.saveImport).disabled(busy)
        case .askChanges:
            Button("Ask for changes", systemImage: "text.bubble", action: actions.refine).disabled(busy)
        case .useOriginal:
            // Back to the card as the assistant made it, once the person has changed it.
            Button("Use the assistant's version", systemImage: "arrow.uturn.backward") { actions.revise?(ProposalChoices.original) }.disabled(busy)
        case .discard:
            Button("Discard", role: .destructive, action: actions.discard).disabled(busy)
        case .restore:
            Button("Restore", systemImage: "arrow.uturn.backward", action: actions.restore).disabled(busy)
        case .undo:
            Button("Undo", systemImage: "arrow.uturn.backward", action: actions.undo).disabled(busy)
        case .show:
            Button("Show in the budget", systemImage: "eye", action: actions.show).disabled(busy)
        case .remove, .answer:
            // Remove is in the card's menu, and a question's answers are its rows.
            EmptyView()
        }
    }

    /// Apply, saying how much of the card it makes while some of its rows are left out
    /// ("Apply 2 of 3"). A delete card's button deletes, and says how many transactions.
    @ViewBuilder
    private func applyButton(_ rows: [JSON], choosing: Bool) -> some View {
        let partial = ProposalChoices.partial(rows, choosing: choosing)
        // Every row is left out: nothing to apply.
        let nothing = partial?.chosen == 0
        if proposal.text("kind") == "delete" {
            let count = AssistantCard.deleting(rows, more: Int(proposal["card"].long("more")))
            primary("Delete \(count) transactions", "trash", actions.apply, enabled: !nothing, destructive: true)
        } else if let partial {
            primary("Apply \(partial.chosen) of \(partial.total)", "checkmark", actions.apply, enabled: !nothing)
        } else {
            primary("Apply reviewed changes", "checkmark", actions.apply)
        }
    }

    /// Apply for an import's fixes. The import the assistant is open over applies them, each
    /// only to a line that hasn't changed; anywhere else the card says where to go.
    @ViewBuilder
    private func importButton(_ rows: [JSON], choosing: Bool) -> some View {
        if let applyFix = actions.applyFix {
            if let partial = ProposalChoices.partial(rows, choosing: choosing) {
                primary("Apply \(partial.chosen) of \(partial.total) to the import", "checkmark", applyFix, enabled: partial.chosen > 0)
            } else {
                primary("Apply to the import", "checkmark", applyFix)
            }
            Text("Each applies only to a line that hasn't changed.").font(.footnote).foregroundStyle(Color.secondary)
        } else {
            Text("Open the import these fixes are for, and apply them from there.").font(.footnote).foregroundStyle(Color.secondary)
        }
    }

    /// A wide button. It spins while its own action (`spins`) runs on this card, and waits, as
    /// the card's other buttons do, while any action of this card runs.
    private func primary(_ title: LocalizedStringKey, _ symbol: String, _ action: @escaping () -> Void, enabled: Bool = true, spins: String = "apply",
                         destructive: Bool = false) -> some View {
        Button(role: destructive ? .destructive : nil, action: action) {
            HStack(spacing: 8) {
                if working == spins { ProgressView().controlSize(.small).tint(.white) } else { Image(systemName: symbol) }
                Text(title)
            }
            .font(.subheadline.weight(.semibold))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 4)
        }
        .buttonStyle(.borderedProminent)
        .tint(destructive ? Color.urNegative : nil)
        .disabled(busy || working != nil || !enabled)
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
        case "notes": return "note.text"
        case "categories": return "tag"
        case "schedules": return "calendar.badge.clock"
        case "delete": return "trash"
        case "figures": return "chart.bar"
        case "question": return "questionmark.circle"
        default: return "arrow.left.arrow.right"
        }
    }

    /// The mark on a finished card's line, by what became of it.
    private static func finishedSymbol(_ proposal: JSON) -> String {
        if proposal.text("kind") == "question" { return "questionmark.circle" }
        switch proposal.text("status") {
        case "applied": return "checkmark.circle.fill"
        case "undone": return "arrow.uturn.backward.circle"
        case "replaced": return "arrow.triangle.2.circlepath"
        default: return "xmark.circle"
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
                            PlanningPickerRow(icon: "tag", label: String(localized: "Category"), value: planningCategoryName(store.budget, category),
                                              placeholder: String(localized: "Choose a category"))
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
                CategoryPicker(title: String(localized: "Category for \(label)"), month: store.month, selected: category, includeUnassigned: true, includePayment: false) { picked in
                    category = picked
                    route = []
                }
            }
        }
        .retainingEditorDraft("assistantRow", values: ["row": row, "category": .string(category), "amount": .string(amount)])
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
