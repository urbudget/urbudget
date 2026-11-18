import SwiftUI
import UniformTypeIdentifiers

/// More › Investments, and the ways an investment account is kept up to date besides Interactive
/// Brokers (the desktop's InvestmentsSettings and HoldingsSetup.tsx): a positions file the broker
/// lets you download, read by the engine (positions_file.rs) and looked over before it's kept,
/// or, for someone who asked for it, a value typed in. Both go into the budget through one engine
/// command, `holdingsRecord`. A brokerage account linked under Bank connections gets its holdings
/// with each bank sync (BankConnectionsView.swift).
extension Investments {
    /// Where "Advanced: let me type an account's value myself" is kept on this device.
    static let typedKey = "typedValues"

    /// How an account is kept up to date, by where its holdings come from.
    static func sourceName(_ held: JSON) -> String {
        switch held.text("source", "ibkr") {
        case "ibkr": return "Interactive Brokers"
        case "simplefin": return String(localized: "Bank connection")
        case "file": return String(localized: "Positions file")
        default: return String(localized: "Value you type in")
        }
    }

    /// The accounts the Investments page lists: the open ones that can hold investments (an
    /// investment or retirement account, or a tracking asset with no type of its own), and any
    /// other account something already fills.
    static func rows(_ budget: JSON?) -> [JSON] {
        guard let budget else { return [] }
        let filled = Set(budget.rows(key).map { $0.text("accountId") })
        return budget.rows("accounts").filter { account in
            guard !account.flag("archived") else { return false }
            if filled.contains(account.text("id")) { return true }
            return account.text("kind") == "tracking" && ["investment", "retirement", "otherAsset"].contains(AccountTypes.infer(account))
        }
    }

    /// A plain decimal the engine wrote ("1234.5") in hundredths; nothing reads as zero.
    static func cents(_ text: String) -> Int64 {
        guard var value = Decimal(string: text.trimmingCharacters(in: .whitespaces), locale: Locale(identifier: "en_US_POSIX")) else { return 0 }
        value *= 100
        var rounded = Decimal()
        NSDecimalRound(&rounded, &value, 0, .plain)
        return NSDecimalNumber(decimal: rounded).int64Value
    }

    /// Hundredths as the plain decimal the engine reads ("1234.50").
    static func decimal(_ cents: Int64) -> String {
        let magnitude = cents.magnitude
        let part = magnitude % 100
        return (cents < 0 ? "-" : "") + "\(magnitude / 100)." + (part < 10 ? "0\(part)" : "\(part)")
    }

    /// A currency code as typed, when the figures in it need a rate to the budget's.
    static func foreign(_ typed: String, budgetCurrency: String) -> String? {
        let code = typed.trimmingCharacters(in: .whitespaces).uppercased()
        return code.isEmpty || code == budgetCurrency ? nil : code
    }

    /// The rate given for this account before, when its figures were in `code`: used again unless
    /// another is typed.
    static func rememberedRate(_ held: JSON?, code: String?) -> String {
        guard let held, let code, held.text("currency") == code else { return "" }
        return held.text("manualRate")
    }

    /// What a positions file says an account holds, to record (investments.rs `Snapshot`): its
    /// holdings and cash as the engine read them, the day they're from, and, when they aren't in
    /// the budget's currency, that currency with what one unit of it is worth.
    static func fileSnapshot(_ read: JSON, fileName: String, asOf: String, currency: String, budgetCurrency: String, rate: String) -> JSON {
        let code = foreign(currency, budgetCurrency: budgetCurrency)
        return ["source": "file", "sourceAccount": .string(fileName), "asOf": .string(asOf),
                "currency": .string(code ?? ""), "rate": .string(code == nil ? "" : rate),
                "positions": .array(read["positions"].array), "cash": .string(read.text("cash"))]
    }

    /// An account's whole value on a day, typed in.
    static func typedSnapshot(cents: Int64, asOf: String) -> JSON {
        ["source": "manual", "asOf": .string(asOf), "total": .string(decimal(cents))]
    }
}

extension BudgetStore {
    /// Records what an account holds (`holdingsRecord`) in the edit queue, and throws what the
    /// engine refuses.
    func recordHoldings(account: String, snapshot: JSON) async throws {
        let params: JSON = ["accountId": .string(account), "snapshot": snapshot]
        try await perform { repository in ((), try await repository.command("holdingsRecord", params)) }
    }
}

/// A sheet of the setup: a positions file being looked over, or a value being typed.
enum HoldingsSetupRequest: Identifiable {
    case file(account: JSON, url: URL)
    case typed(account: JSON)

    var id: String {
        switch self {
        case .file(let account, let url): return "file:" + account.text("id") + ":" + url.absoluteString
        case .typed(let account): return "typed:" + account.text("id")
        }
    }
}

/// What's being set up for an account, as the view that offers it keeps it (see `holdingsSetup`).
struct HoldingsSetupState {
    /// The account whose ways to be kept up to date are offered.
    var choosing: JSON?
    /// A positions file is being chosen, and the account it's for.
    var picking = false
    var pickingFor: JSON?
    var sheet: HoldingsSetupRequest?

    /// Asks for a positions file for `account`.
    mutating func pick(_ account: JSON) {
        pickingFor = account
        picking = true
    }
}

extension View {
    /// The setup's dialogs and sheets, for a view that keeps a `HoldingsSetupState`. `broker` opens
    /// Interactive Brokers' own screen.
    func holdingsSetup(_ state: Binding<HoldingsSetupState>, broker: @escaping () -> Void) -> some View {
        modifier(HoldingsSetupModifier(state: state, broker: broker))
    }
}

private struct HoldingsSetupModifier: ViewModifier {
    @Environment(BudgetStore.self) private var store
    @Binding var state: HoldingsSetupState
    var broker: () -> Void
    @AppStorage(Investments.typedKey) private var typed = false

    private static let types: [UTType] = [.commaSeparatedText, .text, .xml, .spreadsheet, .pdf]
        + ["xlsx", "xls", "ods", "ofx", "qfx"].compactMap { UTType(filenameExtension: $0) } + [.data]

    func body(content: Content) -> some View {
        let account = state.choosing
        let held = Investments.holdings(store.budget, account: account?.text("id") ?? "")
        content
            .confirmationDialog(Text(verbatim: account?.text("name") ?? ""),
                                isPresented: Binding(get: { state.choosing != nil }, set: { if !$0 { state.choosing = nil } }), titleVisibility: .visible) {
                if let account { choices(account, held: held) }
            } message: {
                if let held, held.text("source", "ibkr") != "ibkr" {
                    Text(Investments.sourceNote(held) + " " + String(localized: "Unlinking stops the account following this value. Its transactions stay."))
                }
            }
            // On a view of its own: a second file importer on the host's own view would replace the host's.
            .background {
                Color.clear.fileImporter(isPresented: $state.picking, allowedContentTypes: Self.types) { result in
                    let account = state.pickingFor
                    state.pickingFor = nil
                    if case .success(let url) = result, let account { state.sheet = .file(account: account, url: url) }
                }
            }
            .sheet(item: $state.sheet) { request in
                switch request {
                case .file(let account, let url): PositionsFileSheet(account: account, url: url)
                case .typed(let account): TypedValueSheet(account: account)
                }
            }
    }

    /// The ways to keep an account up to date when nothing fills it yet, else what can be done
    /// with the way it is: a newer file, a newer value, or no longer following it.
    @ViewBuilder private func choices(_ account: JSON, held: JSON?) -> some View {
        let source = held?.text("source", "ibkr")
        if source == nil {
            Button("Positions file") { state.pick(account) }
            Button { broker() } label: { Text(verbatim: "Interactive Brokers") }
            if typed { Button("Type its value") { state.sheet = .typed(account: account) } }
        } else if source == "ibkr" {
            Button { broker() } label: { Text(verbatim: "Interactive Brokers") }
        } else {
            if source == "file" { Button("Update from a file") { state.pick(account) } }
            if source == "manual" { Button("Update value") { state.sheet = .typed(account: account) } }
            Button("Unlink", role: .destructive) { unlink(account.text("id")) }
        }
    }

    private func unlink(_ account: String) {
        Task {
            do { try await store.editing { budget in Investments.unlink(&budget, account: account) } }
            catch { store.showToast(BudgetStore.readable(error)) }
        }
    }
}

/// More › Investments: every account that can hold investments with how it's kept up to date,
/// the way into Interactive Brokers' own connection, and the switch that offers typing a value.
struct InvestmentsHomeView: View {
    @Environment(BudgetStore.self) private var store
    @State private var setup = HoldingsSetupState()
    @State private var showingBroker = false
    @State private var help = false
    @AppStorage(Investments.typedKey) private var typed = false

    var body: some View {
        let rows = Investments.rows(store.budget)
        List {
            Section {
                Text("See what your investment accounts hold and how they're doing. Each one is kept up to date its own way: by Interactive Brokers, by your bank connection, or from a positions file you bring in.")
                    .font(.subheadline).foregroundStyle(Color.secondary).listRowBackground(Color.clear)
            }
            Section {
                if rows.isEmpty {
                    Text("An investment account you add under Accounts shows here.").font(.subheadline).foregroundStyle(Color.secondary)
                }
                ForEach(rows, id: \.idText) { account in
                    accountRow(account)
                }
            }
            Section {
                Button { showingBroker = true } label: {
                    HStack(spacing: 12) {
                        Image(systemName: "link").foregroundStyle(Color.urAccent).frame(width: 28).accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(verbatim: "Interactive Brokers").foregroundStyle(Color.primary)
                            Text("Holdings, prices, deposits and income, with a read-only token").font(.footnote).foregroundStyle(Color.secondary)
                        }
                        Spacer(minLength: 8)
                        Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary).accessibilityHidden(true)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            Section {
                DisclosureGroup(isExpanded: $help) {
                    ForEach(Array(Self.how.enumerated()), id: \.offset) { _, line in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            Text(verbatim: "•").foregroundStyle(Color.secondary)
                            Text(line)
                        }
                        .font(.footnote)
                    }
                } label: {
                    Text("How an account is kept up to date")
                }
            }
            Section {
                Toggle("Let me type an account's value myself", isOn: $typed)
            } footer: {
                Text("Advanced: for a broker with nothing to download")
            }
        }
        .groupedList()
        .navigationTitle("Investments")
        .inlineTitle()
        .navigationDestination(isPresented: $showingBroker) { InvestmentsView() }
        .holdingsSetup($setup) { showingBroker = true }
    }

    private static let how = [
        String(localized: "Interactive Brokers syncs holdings, prices, deposits and income with a read-only token. Open Interactive Brokers on this page to set it up."),
        String(localized: "A bank connection brings an investment account's holdings with each bank sync, when its bank sends them. Link the account under Bank connections."),
        String(localized: "A positions file is what your broker lets you download: a CSV, a spreadsheet, or an OFX or QFX statement. The assistant can read other layouts."),
        String(localized: "For a broker with nothing to download, you can type the account's value yourself. Turn that on at the bottom of this page."),
    ]

    /// One account: how it's kept up to date and what it's worth. A tap offers what can be done
    /// with it; one Interactive Brokers fills opens its connection.
    private func accountRow(_ account: JSON) -> some View {
        let held = Investments.holdings(store.budget, account: account.text("id"))
        let about = held.map { ImportsText.line([Investments.sourceName($0), Money.date($0.text("asOf"))]) } ?? String(localized: "Nothing yet")
        return Button {
            if held?.text("source", "ibkr") == "ibkr" { showingBroker = true } else { setup.choosing = account }
        } label: {
            HStack(spacing: 12) {
                Image(systemName: "chart.pie").foregroundStyle(held == nil ? Color.secondary : Color.urAccent).frame(width: 28).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(account.text("name")).foregroundStyle(Color.primary)
                    Text(about).font(.footnote).foregroundStyle(Color.secondary)
                }
                Spacer(minLength: 8)
                if let held { Text(money(held.long("total"), store.currency)).monospacedDigit().lineLimit(1).layoutPriority(1).foregroundStyle(Color.primary) }
                Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary).accessibilityHidden(true)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// What a positions file says an account holds, looked over before it's kept: each holding with
/// its value and cost, the day the figures are from, and the currency they're in. A file that
/// can't be read, or that left rows out, can go to the assistant, which reads other layouts and
/// proposes the same thing.
struct PositionsFileSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var account: JSON
    var url: URL

    @State private var fileName = ""
    @State private var read: JSON?
    @State private var problem: String?
    @State private var date = Dates.today
    @State private var currency = ""
    @State private var rate = ""
    @State private var saving = false
    @State private var asking: AssistantRequest?

    private var accountId: String { account.text("id") }
    /// The currency the figures are in, when it isn't the budget's.
    private var code: String? { Investments.foreign(currency, budgetCurrency: store.currency) }
    /// A rate given for this account before is used again unless another is typed.
    private var remembered: String { Investments.rememberedRate(Investments.holdings(store.budget, account: accountId), code: code) }
    private var givenRate: String? { Investments.rate(rate) ?? (rate.trimmingCharacters(in: .whitespaces).isEmpty && !remembered.isEmpty ? remembered : nil) }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(account.text("name")).font(.headline)
                        if let read { Text(summary(read)).font(.footnote).foregroundStyle(Color.secondary) }
                    }
                }
                if let problem { FormErrorView(message: problem) }
                if let read {
                    holdingsSection(read)
                    if read.long("skipped") > 0 {
                        Notice(text: skippedNote(Int(read.long("skipped"))),
                               tone: "info", icon: "sparkles", action: String(localized: "Ask the assistant to read it")) { askAssistant() }
                            .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                    }
                    figuresSection
                } else if problem == nil {
                    HStack(spacing: 10) { ProgressView(); Text("Reading the file…").foregroundStyle(Color.secondary) }
                } else {
                    Notice(text: String(localized: "The assistant can read other layouts: it looks at the file and proposes what the account holds for you to check."),
                           tone: "info", icon: "sparkles", action: String(localized: "Ask the assistant to read it")) { askAssistant() }
                        .listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                }
            }
            .groupedList()
            .navigationTitle("Positions file")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(saving) }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button("Save holdings") { save() }
                            .fontWeight(.semibold)
                            .disabled(read == nil || (code != nil && givenRate == nil))
                    }
                }
            }
            .task { await load() }
            .sheet(item: $asking) { AssistantView(request: $0) }
        }
        .interactiveDismissDisabled(saving)
    }

    /// An amount of the file's, in the currency its figures are in.
    private func figure(_ text: String) -> String { money(Investments.cents(text), code ?? store.currency) }

    /// "3 holdings · 28,645.00 SAR", and how much of it is cash.
    private func summary(_ read: JSON) -> String {
        let count = read.rows("positions").count
        let all = ImportsText.line([String(localized: "\(count) holdings", locale: .app), figure(read.text("value"))])
        let cash = read.text("cash")
        return cash.isEmpty ? all : all + " " + String(localized: "\(figure(cash)) of it is cash.")
    }

    /// Rows that looked like holdings and had no value that could be read.
    private func skippedNote(_ count: Int) -> String {
        String(localized: "\(count) rows had no value UrBudget could read and are left out.", locale: .app)
    }

    @ViewBuilder private func holdingsSection(_ read: JSON) -> some View {
        let positions = read.rows("positions")
        if !positions.isEmpty {
            Section {
                ForEach(Array(positions.enumerated()), id: \.offset) { _, position in
                    let name = position.text("name")
                    let symbol = position.text("symbol")
                    HStack(alignment: .firstTextBaseline, spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(name.isEmpty ? symbol : name)
                            let about = detail(position)
                            if !about.isEmpty { Text(about).font(.footnote).foregroundStyle(Color.secondary) }
                        }
                        Spacer(minLength: 8)
                        Text(figure(position.text("value"))).monospacedDigit().lineLimit(1).layoutPriority(1)
                    }
                }
            }
        }
    }

    /// "VWRA · 120.5 × 165.975 · cost 18,000.00 USD": what a holding's row says under its name.
    private func detail(_ position: JSON) -> String {
        let name = position.text("name")
        let symbol = position.text("symbol")
        let quantity = position.text("quantity")
        let price = position.text("price")
        var parts: [String] = []
        if !name.isEmpty && !symbol.isEmpty && symbol.count <= 12 { parts.append(AppLanguage.leftToRight(symbol)) }
        if !quantity.isEmpty && !price.isEmpty {
            parts.append(AppLanguage.leftToRight("\(Investments.plain(quantity)) × \(Investments.plain(price))"))
        } else if !quantity.isEmpty {
            parts.append(AppLanguage.leftToRight(Investments.plain(quantity)))
        }
        let cost = position.text("cost")
        if !cost.isEmpty { parts.append(String(localized: "cost \(figure(cost))")) }
        return parts.isEmpty ? "" : ImportsText.line(parts)
    }

    /// The day the figures are from, the currency they're in, and the rate that currency needs.
    private var figuresSection: some View {
        Section {
            DatePicker(selection: Binding(get: { Dates.pickerDate(date) }, set: { date = Dates.fromPicker($0) }), displayedComponents: .date) {
                Text("Prices from")
            }
            LabeledContent {
                TextField(String(localized: "Currency"), text: $currency).plainTextEntry()
                    .multilineTextAlignment(AppLanguage.isRightToLeft ? .leading : .trailing).amountDirection().frame(maxWidth: 150)
            } label: {
                Text("Currency")
            }
            if let code {
                LabeledContent {
                    TextField(String(localized: "Exchange rate"), text: $rate, prompt: Text(verbatim: remembered.isEmpty ? "0.00" : remembered))
                        .decimalKeyboard().monospacedDigit().multilineTextAlignment(AppLanguage.isRightToLeft ? .leading : .trailing)
                        .amountDirection()
                        .frame(maxWidth: 150)
                } label: {
                    Text("1 \(code) in \(store.currency)").layoutPriority(1)
                }
            }
        } footer: {
            Text("The account's balance becomes this value. From here on, what it changes by beyond the money you put in or take out is recorded as Market change.")
        }
    }

    private func load() async {
        guard read == nil, problem == nil else { return }
        if currency.isEmpty { currency = store.currency }
        let url = url
        do {
            let picked = try await Task.detached(priority: .userInitiated) {
                try ImportFiles.read(url, limit: 8 * 1024 * 1024, tooLarge: String(localized: "Choose a statement smaller than 8 MB."))
            }.value
            fileName = picked.name
            // A PDF has no text to read here: it's the assistant's.
            if picked.name.lowercased().hasSuffix(".pdf") || picked.data.starts(with: [0x25, 0x50, 0x44, 0x46]) {
                problem = String(localized: "UrBudget can't read a PDF here.")
                return
            }
            let text = try await Task.detached(priority: .userInitiated) { try ImportFiles.statement(picked.data, sheet: nil).text }.value
            let found = try await store.query("holdingsFile", ["content": .string(text)])
            if !found.text("asOf").isEmpty { date = found.text("asOf") }
            if !found.text("currency").isEmpty { currency = found.text("currency") }
            read = found
        } catch is CancellationError {
        } catch {
            problem = BudgetStore.readable(error)
        }
    }

    private func save() {
        guard let read, !saving else { return }
        saving = true
        problem = nil
        let id = accountId
        let name = account.text("name")
        let snapshot = Investments.fileSnapshot(read, fileName: fileName, asOf: date, currency: currency, budgetCurrency: store.currency, rate: givenRate ?? "")
        Task {
            do {
                try await store.recordHoldings(account: id, snapshot: snapshot)
                store.showToast(String(localized: "\(name) is up to date."), undo: true)
                dismiss()
            } catch {
                problem = BudgetStore.readable(error)
            }
            saving = false
        }
    }

    /// The assistant, asked to read the file into the account, with the file attached.
    private func askAssistant() {
        let name = account.text("name")
        asking = AssistantRequest(page: "accounts", account: accountId,
                                  text: String(localized: "Read what \(name) holds from the attached file and record it."), files: [url])
    }
}

/// The account's value on a day, typed in: for a broker with nothing to download. Its balance
/// follows the value, and a day in the past fills its history in.
struct TypedValueSheet: View {
    @Environment(BudgetStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    var account: JSON

    @State private var value = ""
    @State private var date = Dates.today
    @State private var problem: String?
    @State private var saving = false

    var body: some View {
        let name = account.text("name")
        let known = Investments.holdings(store.budget, account: account.text("id"))
        NavigationStack {
            List {
                Section {
                    Text("Type what the account is worth on a day, as your broker shows it. Do it again whenever you like: once a month is plenty.")
                        .font(.subheadline).foregroundStyle(Color.secondary).listRowBackground(Color.clear)
                }
                if let problem { FormErrorView(message: problem) }
                Section {
                    AmountField(title: String(localized: "Value"), text: $value)
                    DatePicker(selection: Binding(get: { Dates.pickerDate(date) }, set: { date = Dates.fromPicker($0) }), displayedComponents: .date) {
                        Text("On")
                    }
                } footer: {
                    VStack(alignment: .leading, spacing: 6) {
                        if let known, !known["values"].array.isEmpty {
                            Text("Last value: \(money(known.long("total"), store.currency)) on \(Money.date(known.text("asOf"))). An earlier day adds to its history.")
                        }
                        Text("The account's balance becomes this value. From here on, what it changes by beyond the money you put in or take out is recorded as Market change.")
                    }
                }
            }
            .groupedList()
            .navigationTitle("Value of \(name)")
            .inlineTitle()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(saving) }
                ToolbarItem(placement: .confirmationAction) {
                    if saving {
                        ProgressView()
                    } else {
                        Button("Save value") { save() }
                            .fontWeight(.semibold)
                            .disabled(value.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
            }
        }
        .interactiveDismissDisabled(saving)
    }

    private func save() {
        guard !saving else { return }
        let cents: Int64
        do { cents = try Money.parse(value) } catch {
            problem = BudgetStore.readable(error)
            return
        }
        saving = true
        problem = nil
        let id = account.text("id")
        let name = account.text("name")
        let snapshot = Investments.typedSnapshot(cents: cents, asOf: date)
        Task {
            do {
                try await store.recordHoldings(account: id, snapshot: snapshot)
                store.showToast(String(localized: "\(name) is up to date."), undo: true)
                dismiss()
            } catch {
                problem = BudgetStore.readable(error)
            }
            saving = false
        }
    }
}
