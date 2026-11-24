import SwiftUI

/// A set of categories a new budget can start with, as the engine writes them in the language in
/// use (budget_templates.rs): the same list on desktop and Android. Its names are the new budget's
/// own afterwards, to rename or delete like any other.
struct BudgetTemplate: Identifiable, Equatable {
    var id: String
    var name: String
    var description: String
    /// The groups and how many categories, or the description when there are none.
    var summary: String
    var hasCategories: Bool

    /// The one a new budget starts on, the engine's first.
    static let defaultID = "everyday"

    /// Every template, in the order the engine lists them; none if it can't say.
    static func all() -> [BudgetTemplate] {
        guard let reply = try? NativeCore.engine("budgetTemplates") else { return [] }
        return reply["templates"].array.map {
            BudgetTemplate(id: $0.text("id"), name: $0.text("name"), description: $0.text("description"),
                           summary: $0.text("summary"), hasCategories: !$0["groups"].array.isEmpty)
        }
    }
}

/// Which template a new budget starts with: one row showing the chosen one and what it holds,
/// opening the list to choose another.
struct TemplatePicker: View {
    @Binding var selection: String
    @Environment(BudgetStore.self) private var store
    @State private var templates: [BudgetTemplate] = []

    var body: some View {
        let chosen = templates.first { $0.id == selection }
        NavigationLink {
            TemplateList(templates: templates, selection: $selection)
        } label: {
            VStack(alignment: .leading, spacing: 2) {
                LabeledContent("Categories") { Text(verbatim: chosen?.name ?? "") }
                if let chosen {
                    Text(verbatim: chosen.summary).font(.footnote).foregroundStyle(Color.secondary)
                }
            }
        }
        .disabled(templates.isEmpty)
        // Asked once the store has started, which is when the engine learns the app's language: the
        // welcome screen debug builds open over the app (`-open welcome`) appears before that.
        .task(id: store.loading) { if !store.loading { templates = BudgetTemplate.all() } }
    }
}

/// The templates with what each is for, the chosen one ticked. Choosing one goes back. On the app's
/// page and cards, as the currency list beside it (navy in dark, not iOS's grey).
private struct TemplateList: View {
    var templates: [BudgetTemplate]
    @Binding var selection: String
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        AppList(templates, id: \.id) { template in
            Button {
                selection = template.id
                dismiss()
            } label: {
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(verbatim: template.name).foregroundStyle(Color.primary)
                        Text(verbatim: template.description).font(.footnote).foregroundStyle(Color.secondary)
                        if template.hasCategories {
                            Text(verbatim: template.summary).font(.footnote).foregroundStyle(Color.secondary)
                        }
                    }
                    Spacer(minLength: 0)
                    if template.id == selection {
                        Image(systemName: "checkmark").fontWeight(.semibold).foregroundStyle(Color.urAccent)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(template.id == selection ? .isSelected : [])
        }
        .groupedList()
        .navigationTitle("Categories")
        .inlineTitle()
    }
}

/// How a new budget starts, as the desktop's welcome sets one up (newBudget.ts, Welcome.tsx).
enum StarterBudget {
    /// The device's way of writing numbers and dates, as a new budget's starting formats.
    static func localFormats(currency: String, locale: Locale = .current) -> (number: LibraryNumberFormat, date: String) {
        let decimal = locale.decimalSeparator == "," ? "," : "."
        // Spaces of any width group as a space; a typographic apostrophe as an apostrophe.
        let separators: [String: String] = [",": ",", ".": ".", " ": " ", "\u{00A0}": " ", "\u{202F}": " ", "'": "'", "\u{2019}": "'"]
        var group = separators[locale.groupingSeparator ?? ","] ?? ","
        if group == decimal { group = decimal == "." ? "," : "." }
        let pattern = DateFormatter.dateFormat(fromTemplate: "yMd", options: 0, locale: locale) ?? ""
        let date = pattern.hasPrefix("M") ? "mdy" : pattern.hasPrefix("d") ? "dmy" : "iso"
        return (LibraryNumberFormat(decimal: decimal, group: group, symbol: LibraryNumberFormat.defaultSymbol(currency)), date)
    }

    /// Whether a budget is a sample to try things on. Samples made before they were marked are known
    /// by the engine sample's account and purchase (a fresh start from one doesn't carry the purchase).
    static func isSample(_ budget: JSON) -> Bool {
        budget["sample"].isObject
            || (budget.row("accounts", id: "sample-checking") != nil && budget.row("transactions", id: "sample-market") != nil)
    }

    /// The budget leaving a sample goes to: the one it was opened from, else the one opened most recently.
    static func sampleExit(from sample: JSON, library: [JSON]) -> JSON? {
        // The library lists budgets in Trash and ones kept for recovery too: neither can be opened.
        let others = library.filter { !$0.flag("current") && !$0.flag("missing") && ["", "active"].contains($0.text("lifecycle")) }
        return others.first { $0.text("id") == sample["sample"].text("from") } ?? others.first
    }

    /// Whether Budget shows a budget the getting-started steps: on for one set up from the welcome
    /// screen, until it's hidden.
    @MainActor static func checklistShown(_ store: BudgetStore) -> Bool {
        guard let key = store.budgetKey else { return false }
        if store.setUpFromWelcome {
            store.setUpFromWelcome = false
            UserDefaults.standard.set("on", forKey: "setup.\(key)")
        }
        return UserDefaults.standard.string(forKey: "setup.\(key)") == "on"
    }
}

/// Five steps that teach the method by doing it, each ticked off by what's in the budget, as the
/// desktop's getting-started checklist. Shown on Budget for budgets set up from the welcome screen
/// until it's hidden.
struct GettingStartedCard: View {
    var budget: JSON
    /// The month's Unassigned.
    var unassigned: Int64
    var addAccount: () -> Void
    var assign: () -> Void
    var addTransaction: () -> Void
    var setTarget: () -> Void
    var hide: () -> Void
    @State private var chosen: Int?

    private struct Step {
        var title: String
        var text: String
        var done: Bool
        var action: (label: String, run: () -> Void)?
    }

    private var steps: [Step] {
        var steps: [Step] = []
        steps.append(Step(title: String(localized: "Add your accounts"),
                          text: String(localized: "Add each bank account and credit card with today's balance. That balance is the money you have to budget."),
                          done: !budget.rows("accounts").isEmpty, action: (String(localized: "Add an account"), addAccount)))
        steps.append(Step(title: String(localized: "Assign your money"),
                          text: String(localized: "Give all of it a job: tap a category's Assigned amount and type on the keypad until Unassigned reaches zero. Quick assign, in the Unassigned sheet, can fill in targets for you."),
                          // Done when all of it has a job, not when the first amount is typed.
                          done: unassigned == 0 && budget.rows("assignments").contains { $0.long("amount") != 0 }, action: (String(localized: "Assign money"), assign)))
        steps.append(Step(title: String(localized: "Record your spending"),
                          text: String(localized: "Add transactions as you spend, or import the statement file your bank lets you download (CSV, Excel, OFX, QIF and more) from an account's page. Each one comes out of its category."),
                          done: !budget.rows("transactions").isEmpty, action: (String(localized: "Add a transaction"), addTransaction)))
        steps.append(Step(title: String(localized: "Plan for a bigger bill"),
                          text: String(localized: "Pick something that doesn't come every month, like car insurance or gifts, and give it a target. UrBudget works out what to set aside each month so it's ready when it's due."),
                          done: budget.rows("categories").contains { $0.long("target") > 0 || $0["goal"].isObject }, action: (String(localized: "Set a target"), setTarget)))
        steps.append(Step(title: String(localized: "Cover overspending"),
                          text: String(localized: "When a category's Available turns red, tap it and move money over from a category that has some left. Changing the plan is part of the plan."),
                          done: budget.rows("moneyMoves").contains { $0.text("kind") == "move" && $0.rows("changes").count > 1 }))
        return steps
    }

    var body: some View {
        let steps = steps
        let doneCount = steps.count { $0.done }
        let allDone = doneCount == steps.count
        let open = chosen ?? steps.firstIndex { !$0.done } ?? steps.count - 1
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text("Getting started").font(.headline)
                Spacer()
                Text("\(doneCount) of \(steps.count) done").font(.subheadline).foregroundStyle(Color.secondary)
                Button(action: hide) {
                    Image(systemName: "xmark").font(.footnote.weight(.bold)).foregroundStyle(Color.secondary).frame(width: 28, height: 28)
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Hide getting started")
            }
            ProgressView(value: Double(doneCount), total: Double(steps.count)).tint(.urPositive)
            VStack(alignment: .leading, spacing: 2) {
                ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                    Button { withAnimation(.snappy) { chosen = index } } label: {
                        HStack(spacing: 10) {
                            Image(systemName: step.done ? "checkmark.circle.fill" : index == open && !allDone ? "circle.inset.filled" : "circle")
                                .foregroundStyle(step.done ? Color.urPositive : index == open ? Color.urAccent : Color.secondary)
                            Text(step.title).foregroundStyle(step.done ? Color.secondary : Color.primary)
                                .fontWeight(index == open && !allDone ? .semibold : .regular)
                            Spacer()
                        }
                        .padding(.vertical, 5)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityValue(step.done ? "Done" : "Not done")
                }
            }
            if allDone {
                Text("You're set up. Keep giving every part of your money a job as it comes in.")
                    .font(.subheadline).foregroundStyle(Color.secondary)
                Button("Hide getting started", action: hide).font(.subheadline.weight(.semibold)).buttonStyle(.borderless)
            } else {
                let step = steps[open]
                Text(step.text).font(.subheadline).foregroundStyle(Color.secondary).fixedSize(horizontal: false, vertical: true)
                if let action = step.action, !step.done {
                    Button(action.label, action: action.run).buttonStyle(.borderedProminent).controlSize(.small)
                }
            }
        }
        .padding(16)
        .background(Color.urAccent.opacity(0.08), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        // When a step gets done, move on to the next one.
        .onChange(of: doneCount) { _, _ in chosen = nil }
    }
}

/// Leaving the sample budget with no budget of your own yet: the sample's library id, and whether to
/// delete it once the new budget opens.
struct SampleExit: Identifiable {
    var id: String
    var delete: Bool
}

/// Over a sample budget, as the desktop's banner: back to your own budget, deleting the sample on the
/// way if you like. With no budget of your own yet, both start one from the welcome screen.
struct SampleBanner: View {
    @Environment(BudgetStore.self) private var store
    @State private var library: [JSON] = []
    @State private var exit: SampleExit?
    @State private var working = false

    var body: some View {
        let own = StarterBudget.sampleExit(from: store.budget ?? [:], library: library) != nil
        VStack(alignment: .leading, spacing: 12) {
            Label {
                (Text("This is a sample budget.").bold() + Text(verbatim: " ") + Text("Try anything here; your own budget is separate."))
                    .fixedSize(horizontal: false, vertical: true)
            } icon: {
                Image(systemName: "sparkles").foregroundStyle(Color.urAccent)
            }
            .font(.subheadline)
            HStack(spacing: 10) {
                Button(own ? "Go to my budget" : "Create my budget") { leave(delete: false) }
                    .buttonStyle(.borderedProminent)
                Button("Delete sample") { leave(delete: true) }
                    .buttonStyle(.bordered)
            }
            .controlSize(.small)
            .disabled(working || store.busy)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.urAccent.opacity(0.08), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .task { library = (try? await store.listBudgets()) ?? [] }
        .sheet(item: $exit) { WelcomeView(leaving: $0) }
    }

    /// Opens the budget to go back to and deletes the sample once it's closed; with none, starts one.
    private func leave(delete: Bool) {
        guard !working else { return }
        working = true
        let sample = store.budget ?? [:]
        Task {
            defer { working = false }
            do {
                library = try await store.listBudgets()
                guard let id = library.first(where: { $0.flag("current") })?.text("id") else { return }
                guard let target = StarterBudget.sampleExit(from: sample, library: library) else {
                    exit = SampleExit(id: id, delete: delete)
                    return
                }
                let targetId = target.text("id")
                try await store.perform { repository in ((), try await repository.open(id: targetId)) }
                if delete {
                    _ = try await store.deleteBudget(id: id)
                    store.showToast(String(localized: "Deleted the sample budget."))
                } else {
                    store.showToast(String(localized: "Opened \(target.text("name"))"))
                }
            } catch {
                store.error = BudgetStore.readable(error)
            }
        }
    }
}
