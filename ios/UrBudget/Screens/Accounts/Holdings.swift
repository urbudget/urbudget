import SwiftUI

/// Investment accounts that mirror a broker (crates/urbudget-core investments.rs; the desktop's
/// Investments.tsx). What an account holds lives in the budget's `investments`, one entry per
/// account id, so it syncs like everything else and shows here without a connection on this
/// device. The account's balance stays the sum of its transactions: the engine writes a "Starting
/// value" and one "Market change" a month (`investment: "start" | "change"`) to keep it equal to
/// the broker's value. Nothing here is worked out by the app but the gain, which is those
/// changes added up.
enum Investments {
    /// The budget extra that holds every account's holdings.
    static let key = "investments"
    /// Where a new Interactive Brokers account goes unless the person says otherwise.
    static let newAccount = "new"
    /// What to ask Client Portal's "Configure Query with AI" for, in its own words: the sections
    /// the reader uses, each with all of its fields (the desktop's FLEX_REQUEST). It takes at
    /// most 200 characters, and is English whatever the interface.
    static let flexRequest = "Activity Flex Query, XML, Last 365 Calendar Days, Currency Rates Yes. All fields of: Account Information, Cash Report, Cash Transactions, NAV in Base, Open Positions (Summary), Trades, Transfers."

    /// Where the choices about Interactive Brokers are kept on this device: whether to sync once
    /// a day when the app opens, the day that was last tried, and "Not now" to the question on
    /// an investment account.
    static let dailyKey = "ibkrDaily"
    static let dailyAtKey = "ibkrDailyAt"
    static let promptHiddenKey = "ibkrPromptHidden"

    /// Where an account's figures come from, for the line under them: Interactive Brokers'
    /// account there, a bank connection, a positions file, or a value typed in (HoldingsSetup.swift
    /// brings the last two in; a bank sync brings the bank's).
    static func sourceNote(_ held: JSON) -> String {
        let date = Money.date(held.text("asOf"))
        switch held.text("source", "ibkr") {
        case "ibkr":
            let id = held.text("sourceAccount")
            return String(localized: "From Interactive Brokers account \(id). Each month's change in value is added to the register as Market change.")
        case "simplefin":
            return String(localized: "From your bank connection, as of \(date). Each month's change in value is listed under Transactions as Market change.")
        case "file":
            return String(localized: "From a positions file, with prices from \(date). Each month's change in value is listed under Transactions as Market change.")
        default:
            return String(localized: "From the value you typed for \(date). Each month's change in value is listed under Transactions as Market change.")
        }
    }

    /// Whether Interactive Brokers fills any account of the budget.
    static func filled(_ budget: JSON?) -> Bool {
        budget?.rows(key).contains { $0.text("source") == "ibkr" } ?? false
    }

    /// Whether to sync by itself now: the budget has an account Interactive Brokers fills, the
    /// person hasn't turned it off, and it wasn't tried yet today. The broker makes one
    /// statement a day, so once is enough, and a failure isn't tried again until tomorrow.
    static func dailyDue(filled: Bool, on: Bool, triedOn: String, today: String) -> Bool {
        filled && on && triedOn != today
    }

    /// The day the newest prices among the budget's Interactive Brokers accounts are from, or empty.
    static func newestPrices(_ budget: JSON?) -> String {
        (budget?.rows(key) ?? []).filter { $0.text("source") == "ibkr" }.map { $0.text("asOf") }.max() ?? ""
    }

    /// An open investment account nothing fills yet: the one that's offered a way to see what it holds.
    static func promptable(_ account: JSON, budget: JSON?) -> Bool {
        account.text("kind") == "tracking" && !account.flag("archived") && AccountTypes.infer(account) == "investment"
            && holdings(budget, account: account.text("id")) == nil
    }

    /// What a login is called in the list of logins: the accounts its last statement named, by
    /// their alias in the statement at hand, else their id. Nil before it has named any, when
    /// its row is called by its query.
    static func loginName(_ login: JSON, accounts: [JSON]) -> String? {
        let ids = login["accounts"].array.map(\.string)
        if ids.isEmpty { return nil }
        let names = ids.map { id -> String in
            let alias = accounts.first { $0.text("id") == id }?.text("alias") ?? ""
            return alias.isEmpty ? id : alias
        }
        return names.joined(separator: ", ")
    }

    /// Syncs the budget's Interactive Brokers accounts once a day, a few seconds after the app
    /// opens, when this device has the token (the desktop's useDailyIbkrSync). A failure stays
    /// quiet (the account's card says when prices are old) and isn't tried again until tomorrow.
    @MainActor static func dailySync(_ store: BudgetStore, defaults: UserDefaults = .standard) async {
        func due() -> Bool {
            dailyDue(filled: filled(store.budget), on: defaults.object(forKey: dailyKey) as? Bool ?? true,
                     triedOn: defaults.string(forKey: dailyAtKey) ?? "", today: Dates.today)
        }
        guard due() else { return }
        try? await Task.sleep(nanoseconds: 4_000_000_000)
        guard !Task.isCancelled, due() else { return }
        guard let status = try? await store.service("ibkr_status"), status.flag("connected") else { return }
        defaults.set(Dates.today, forKey: dailyAtKey)
        let had = newestPrices(store.budget)
        // Offline, or the statement isn't ready: tomorrow's try, or Sync, brings it.
        guard (try? await store.service("ibkr_fetch")) != nil,
              (try? await store.service("ibkr_apply", ["links": []], changesBudget: true)) != nil else { return }
        let has = newestPrices(store.budget)
        if has > had { store.showToast(String(localized: "Interactive Brokers: prices from \(Money.date(has)).")) }
    }

    /// An account's holdings, when a broker fills it.
    static func holdings(_ budget: JSON?, account: String) -> JSON? {
        guard !account.isEmpty else { return nil }
        return budget?.rows(key).first { $0.text("accountId") == account }
    }

    /// What the account gained or lost beyond the money put in or taken out: its market changes
    /// added up. Nil until there is one, so a day-old account doesn't say it gained nothing.
    static func gain(_ budget: JSON, account: String) -> Int64? {
        var total: Int64 = 0
        var any = false
        for transaction in budget.rows("transactions") where transaction.text("investment") == "change" && transaction.text("accountId") == account {
            total += transaction.long("amount")
            any = true
        }
        return any ? total : nil
    }

    /// Holdings whose prices are more than four days old: past a weekend, the broker has newer ones.
    static func stale(_ holdings: JSON, today: String = Dates.today) -> Bool {
        let asOf = holdings.text("asOf")
        return !asOf.isEmpty && asOf < Dates.addDays(today, -4)
    }

    /// The first day the broker valued the account, which the gain is counted from.
    static func trackedSince(_ holdings: JSON) -> String? {
        let first = holdings["values"].array.first?[0].string ?? ""
        return first.isEmpty ? nil : first
    }

    /// A quantity or price as the broker wrote it ("141.9087137"), to at most `places` decimals in
    /// the budget's number format. Text that isn't a plain number is shown as it came.
    static func plain(_ text: String, places: Int = 4) -> String {
        guard text.range(of: #"^-?[0-9]+(\.[0-9]+)?$"#, options: .regularExpression) != nil,
              var value = Decimal(string: text, locale: Locale(identifier: "en_US_POSIX")) else { return text }
        var rounded = Decimal()
        NSDecimalRound(&rounded, &value, places, .plain)
        let negative = rounded < 0
        let digits = NSDecimalNumber(decimal: negative ? -rounded : rounded).stringValue
        let parts = digits.split(separator: ".", maxSplits: 1).map(String.init)
        let whole = Array(parts.first ?? "0")
        let style = Money.format
        var grouped = ""
        for (index, digit) in whole.enumerated() {
            if index > 0 && (whole.count - index) % 3 == 0 { grouped += style.group }
            grouped.append(digit)
        }
        let fraction = parts.count > 1 ? parts[1] : ""
        return (negative ? "-" : "") + grouped + (fraction.isEmpty ? "" : style.decimal + fraction)
    }

    /// "820 USD": cash in its own currency.
    static func cashLine(_ cash: JSON) -> String {
        AppLanguage.leftToRight("\(plain(cash.text("amount"), places: 2)) \(cash.text("currency"))")
    }

    /// A gain or loss with its sign: "+1,300.00 SAR", "-320.00 SAR".
    static func signed(_ amount: Int64, _ currency: String) -> String {
        AppLanguage.leftToRight((amount > 0 ? "+" : "") + Money.plain(amount, currency: currency))
    }

    // MARK: The connection

    /// The budget's other open accounts, the ones this account has moved money with first: where
    /// a deposit the broker lists could have come from. Each comes with the date of its first
    /// transaction (either side of a transfer counts), or nil when it has none: a deposit from
    /// before then is already in its opening balance (`BankReview.offered`).
    static func others(_ budget: JSON, account: String) -> [(id: String, name: String, since: String?)] {
        var moved: [String: Int] = [:]
        var since: [String: String] = [:]
        func seen(_ id: String, _ date: String) {
            guard !id.isEmpty, !date.isEmpty else { return }
            if let earlier = since[id], earlier <= date { return }
            since[id] = date
        }
        for transaction in budget.rows("transactions") {
            let from = transaction.text("accountId")
            let to = transaction.text("transferAccountId")
            let other = from == account ? to : to == account ? from : ""
            if !other.isEmpty { moved[other, default: 0] += 1 }
            let date = transaction.text("date")
            seen(from, date)
            seen(to, date)
            for split in transaction.rows("splits") { seen(split.text("transferAccountId"), date) }
        }
        let open = budget.rows("accounts").filter { $0.text("id") != account && !$0.flag("archived") }
        let ranked = open.enumerated().sorted { first, second in
            let a = moved[first.element.text("id")] ?? 0
            let b = moved[second.element.text("id")] ?? 0
            return a != b ? a > b : first.offset < second.offset
        }
        return ranked.map { (id: $0.element.text("id"), name: $0.element.text("name"), since: since[$0.element.text("id")]) }
    }

    /// How many of an account's deposits, withdrawals and transfers aren't in the budget yet.
    static func fresh(_ review: JSON?) -> Int {
        review?.rows("rows").count { !$0.flag("duplicate") } ?? 0
    }

    /// The currencies an exchange rate is needed between, when the statement can't give one.
    static func needsRate(_ account: JSON) -> (from: String, to: String)? {
        let pair = account["needsRate"].array
        guard pair.count == 2, !pair[0].string.isEmpty, !pair[1].string.isEmpty else { return nil }
        return (pair[0].string, pair[1].string)
    }

    /// A typed exchange rate ("3.75", "3,75", Arabic-Indic digits) as the engine reads one, or nil
    /// when it isn't a number above zero.
    static func rate(_ text: String?) -> String? {
        let western = String((text ?? "").map { character -> Character in
            if let value = character.wholeNumberValue, character.isNumber, value < 10, !character.isASCII { return Character(String(value)) }
            return character
        })
        let tidy = western.trimmingCharacters(in: .whitespaces)
            .replacingOccurrences(of: "\u{066B}", with: ".").replacingOccurrences(of: ",", with: ".")
        guard tidy.range(of: #"^[0-9]+(\.[0-9]+)?$"#, options: .regularExpression) != nil,
              let value = Decimal(string: tidy, locale: Locale(identifier: "en_US_POSIX")), value > 0 else { return nil }
        return tidy
    }

    /// The tracking accounts an Interactive Brokers account could fill: open, not a loan, and not
    /// already filled by a broker.
    static func targets(_ budget: JSON, balances: [String: Int64] = [:]) -> [JSON] {
        let filled = Set(budget.rows(key).map { $0.text("accountId") })
        return budget.rows("accounts").filter { account in
            let id = account.text("id")
            return account.text("kind") == "tracking" && !account.flag("archived") && !filled.contains(id)
                && !AccountTypes.isLiability(AccountTypes.infer(account, balance: balances[id]))
        }
    }

    /// Where a statement's account that fills nothing yet goes: `newAccount`, a tracking account's
    /// id, or "" for nowhere. A choice that no longer names an open account falls back to a new one.
    static func target(_ account: JSON, into: [String: String], targets: [JSON]) -> String {
        guard let chosen = into[account.text("id")] else { return newAccount }
        if chosen.isEmpty || chosen == newAccount || targets.contains(where: { $0.text("id") == chosen }) { return chosen }
        return newAccount
    }

    /// The statement's accounts that would be added: ones that fill nothing yet and aren't set to "Don't add".
    static func adding(_ accounts: [JSON], into: [String: String], targets: [JSON]) -> [JSON] {
        accounts.filter { $0.text("accountId").isEmpty && !target($0, into: into, targets: targets).isEmpty }
    }

    /// Whether every account about to be added has the exchange rate it needs.
    static func ready(_ accounts: [JSON], into: [String: String], rates: [String: String], targets: [JSON]) -> Bool {
        adding(accounts, into: into, targets: targets).allSatisfy { needsRate($0) == nil || rate(rates[$0.text("id")]) != nil }
    }

    /// What `ibkr_apply` is told: each new account and where it goes, and a linked account that
    /// was waiting for its exchange rate. Accounts already linked need nothing said.
    static func links(_ accounts: [JSON], into: [String: String], rates: [String: String], targets: [JSON],
                      newId: () -> String = { UUID().uuidString.lowercased() }) -> [JSON] {
        var links: [JSON] = []
        for account in adding(accounts, into: into, targets: targets) {
            let chosen = target(account, into: into, targets: targets)
            var link: JSON = ["source": .string(account.text("id")), "account": .string(chosen == newAccount ? newId() : chosen),
                              "create": .bool(chosen == newAccount)]
            if needsRate(account) != nil, let given = rate(rates[account.text("id")]) { link["rate"] = .string(given) }
            links.append(link)
        }
        for account in accounts where !account.text("accountId").isEmpty && needsRate(account) != nil {
            guard let given = rate(rates[account.text("id")]) else { continue }
            links.append(["source": .string(account.text("id")), "account": .string(account.text("accountId")), "rate": .string(given)])
        }
        return links
    }

    /// Stops an account mirroring its broker. Its transactions stay, so its balance and history do.
    static func unlink(_ budget: inout JSON, account: String) {
        let rest = budget[key].array.filter { $0.text("accountId") != account }
        if rest.isEmpty { budget.remove(key) } else { budget[key] = .array(rest) }
    }
}

/// An account's deposits, withdrawals and transfers to review, as its sheet is asked for: the account, the
/// statement's review of it, and the budget revision the rows were checked against.
struct DepositsRequest: Identifiable {
    var account: JSON
    var review: JSON
    var revision: String
    var id: String { account.text("id") }
}

/// On an open investment account nothing fills: the offer to see what it holds (the desktop's
/// prompt over such an account's register). "Set up" offers the ways to keep it up to date;
/// "Not now" puts the question away on every account.
struct BrokerPrompt: View {
    var setUp: () -> Void
    var dismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("See what this account holds and how it's doing").font(.headline)
            Text("Connect Interactive Brokers, or bring in the positions file your broker lets you download.").font(.subheadline).foregroundStyle(Color.secondary)
            HStack(spacing: 12) {
                Spacer(minLength: 0)
                Button("Not now", action: dismiss).buttonStyle(.borderless).foregroundStyle(Color.secondary)
                Button(action: setUp) { Label("Set up", systemImage: "chart.xyaxis.line") }.buttonStyle(.bordered)
            }
            .padding(.top, 6)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
}

/// An investment account's card, over its transactions: its value as the broker reported it,
/// the gain beyond the money put in, its deposits to review and Sync. The whole picture (the
/// period's return and chart, each holding, income and costs) is a page of its own (HoldingsPage.swift)
/// that `open` pushes over the register.
struct HoldingsCard: View {
    @Environment(BudgetStore.self) private var store
    var account: JSON
    var holdings: JSON
    /// Opens the Interactive Brokers screen, where this device connects.
    var connect: () -> Void
    /// Opens the review of the deposits a statement lists.
    var review: (DepositsRequest) -> Void
    /// For an account kept from a positions file or a typed value: brings the next one.
    var fromFile: () -> Void = {}
    var typeValue: () -> Void = {}
    /// Opens the account's page of performance and holdings. The register holds where it leads,
    /// outside its list, where a lazy row's own destination would be lost.
    var open: () -> Void = {}

    @State private var status: JSON?
    /// This account in the statement at hand this session, with its deposits to review.
    @State private var reviewed: JSON?
    @State private var reviewedRevision = ""
    @State private var gain: Int64?
    @State private var busy = false
    @State private var message: String?

    private struct Key: Equatable {
        var account: String
        var version: Int
    }

    private var accountId: String { account.text("id") }
    /// Only Interactive Brokers has a connection to ask about and deposits to review: an account
    /// kept from a file, a bank connection or a typed value shows its figures and nothing to sync.
    private var byBroker: Bool { holdings.text("source", "ibkr") == "ibkr" }

    var body: some View {
        let currency = store.currency
        let count = holdings.rows("positions").count
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Holdings").font(.headline)
                Text(pricesLine).font(.footnote)
                    .foregroundStyle(Investments.stale(holdings) ? Color.urWarning : Color.secondary)
            }
            HStack(alignment: .top, spacing: 24) {
                Figure(label: String(localized: "Value"), value: money(holdings.long("total"), currency))
                if let gain, let since = Investments.trackedSince(holdings) {
                    Figure(label: String(localized: "Gain since \(Money.date(since))"), value: Investments.signed(gain, currency),
                           color: gain < 0 ? .urNegative : gain > 0 ? .urPositive : .primary)
                }
            }
            if let message { FormErrorView(message: message) }
            actions
            Hairline()
            Button(action: open) {
                HStack(spacing: 12) {
                    Image(systemName: "chart.xyaxis.line").foregroundStyle(Color.urAccent).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Performance and holdings").foregroundStyle(Color.primary)
                        if count > 0 { Text("\(count) holdings").font(.footnote).foregroundStyle(Color.secondary) }
                    }
                    Spacer(minLength: 8)
                    Image(systemName: "chevron.forward").font(.footnote.weight(.semibold)).foregroundStyle(Color.secondary).accessibilityHidden(true)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            Hairline()
            Text(footnote).font(.footnote).foregroundStyle(Color.secondary)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.cardBackground, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        .task(id: Key(account: accountId, version: store.version)) { await refresh() }
    }

    /// "Prices from 2026-11-13", and that they're a few days old when they are.
    private var pricesLine: String {
        let from = String(localized: "Prices from \(Money.date(holdings.text("asOf")))")
        return Investments.stale(holdings) ? from + " · " + String(localized: "These prices are a few days old.") : from
    }

    /// Where the figures come from, and how far the balance has moved from them since.
    private var footnote: String {
        var parts = [Investments.sourceNote(holdings)]
        if byBroker, let balance = store.summary?.rows("accounts").first(where: { $0.text("id") == accountId })?.long("balance") {
            let apart = balance - holdings.long("total")
            if apart != 0 {
                let amount = money(Int64(apart.magnitude), store.currency)
                parts.append(String(localized: "The balance is \(amount) away from Interactive Brokers' value. A sync brings them back together."))
            }
        }
        return parts.joined(separator: " ")
    }

    @ViewBuilder private var actions: some View {
        let fresh = Investments.fresh(reviewed)
        let source = holdings.text("source", "ibkr")
        // Kept from a file or a typed value: the next one comes the same way.
        if source == "file" {
            Button(action: fromFile) {
                Label("Update from a file", systemImage: "doc.badge.arrow.up").frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
        } else if source == "manual" {
            Button(action: typeValue) {
                Label("Update value", systemImage: "pencil").frame(maxWidth: .infinity)
            }
            .buttonStyle(.bordered)
        }
        if fresh > 0 || status != nil {
            HStack(spacing: 8) {
                if fresh > 0, let reviewed {
                    Button {
                        review(DepositsRequest(account: account, review: reviewed, revision: reviewedRevision))
                    } label: {
                        Text("Review \(fresh) deposits").frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(busy)
                }
                if status?.flag("connected") == true {
                    Button { sync() } label: {
                        Label(busy ? String(localized: "Syncing…") : String(localized: "Sync"), systemImage: "arrow.triangle.2.circlepath")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)
                    .disabled(busy)
                } else if status != nil {
                    Button(action: connect) {
                        Label("Connect on this device", systemImage: "link").frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)
                }
            }
        }
    }

    /// The gain, off the main thread, the connection as it stands, and the deposits still to
    /// review: the budget keeps what the broker listed, so they show with or without a statement
    /// this session.
    private func refresh() async {
        guard let budget = store.budget else { return }
        let id = accountId
        let total = await Task.detached(priority: .userInitiated) { Investments.gain(budget, account: id) }.value
        guard !Task.isCancelled else { return }
        gain = total
        guard byBroker, !busy, let current = try? await store.service("ibkr_status"), !Task.isCancelled else { return }
        status = current
        do { try await check() } catch { reviewed = nil }
    }

    /// This account in the statement at hand, against the budget as it is now.
    @discardableResult
    private func check() async throws -> JSON? {
        let revision = store.revision
        let all = try await store.service("ibkr_review_compact")
        let mine = all.rows("accounts").first { $0.text("accountId") == accountId }
        reviewed = mine
        reviewedRevision = revision
        return mine
    }

    private func sync() {
        guard !busy else { return }
        busy = true
        message = nil
        let name = account.text("name")
        Task {
            do {
                status = try await store.service("ibkr_fetch")
                _ = try await store.service("ibkr_apply", ["links": []], changesBudget: true)
                let mine = try await check()
                if let problem = mine?.text("error"), !problem.isEmpty {
                    message = problem
                } else if let mine, Investments.needsRate(mine) != nil {
                    message = String(localized: "This account needs an exchange rate. Give it in More › Investments › Interactive Brokers.")
                } else {
                    store.showToast(String(localized: "\(name) is up to date."))
                }
            } catch {
                message = BudgetStore.readable(error)
            }
            busy = false
        }
    }
}
