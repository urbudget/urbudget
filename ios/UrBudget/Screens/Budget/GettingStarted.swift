import SwiftUI

/// How a new budget starts, as the desktop's welcome sets one up (newBudget.ts, Welcome.tsx).
enum StarterBudget {
    /// Bills, needs, wants and savings categories to rename or delete later, named in the app's
    /// language when the budget is created; from then on they're the person's own names.
    static let groups: [(name: String, categories: [String])] = { () -> [(name: String, categories: [String])] in
        let bills: [String] = [String(localized: "Rent or mortgage"), String(localized: "Electricity"), String(localized: "Water"),
                               String(localized: "Internet"), String(localized: "Phone")]
        let needs: [String] = [String(localized: "Groceries"), String(localized: "Transportation"), String(localized: "Medical"),
                               String(localized: "Insurance")]
        let wants: [String] = [String(localized: "Dining out"), String(localized: "Entertainment"), String(localized: "Shopping"),
                               String(localized: "Subscriptions")]
        let savings: [String] = [String(localized: "Emergency fund"), String(localized: "Vacation"), String(localized: "Gifts")]
        return [(String(localized: "Bills"), bills), (String(localized: "Needs"), needs),
                (String(localized: "Wants"), wants),
                (String(localized: "starter.group.savings", defaultValue: "Savings", comment: "A starter category group: money put aside."), savings)]
    }()

    static func addCategories(to budget: inout JSON) {
        var categories = budget["categories"].array
        for group in groups {
            for name in group.categories {
                categories.append(["id": .string(UUID().uuidString.lowercased()), "name": .string(name), "group": .string(group.name), "target": 0])
            }
        }
        budget["categories"] = .array(categories)
        budget["groupOrder"] = .array(groups.map { .string($0.name) })
    }

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
                          done: budget.rows("assignments").contains { $0.long("amount") != 0 }, action: (String(localized: "Assign money"), assign)))
        steps.append(Step(title: String(localized: "Record your spending"),
                          text: String(localized: "Add transactions as you spend, or import the statement file your bank lets you download (CSV, OFX, QFX or QIF) from an account's page. Each one comes out of its category."),
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
