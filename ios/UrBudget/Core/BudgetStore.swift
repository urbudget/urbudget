import Foundation
import Observation

/// What the screens draw: one committed budget, its month's figures, and the history flags.
struct BudgetSnapshot: Sendable {
    var budget: JSON?
    var summary: JSON?
    var calculationError: String?
    var month: String
    var version: Int
    var identity: String?
    var revision: String
    var undoAvailable: Bool
    var redoAvailable: Bool
}

/// The serial boundary around the shared engine and the SQLite store, as Android's BudgetRepository
/// with SqliteBudgetStorage. A snapshot is published only after validation, calculation and a
/// durable, revision-checked commit have all succeeded.
actor BudgetRepository {
    /// Sync or another window committed first. The latest budget is in `snapshot`.
    struct Conflict: Error { var snapshot: BudgetSnapshot }

    private var current: JSON?
    private var token = ""
    private var isNew = true
    private var identity: String?
    private var undoAvailable = false
    private var redoAvailable = false
    private var month = Dates.currentMonth
    private var version = 0
    private var figures: (version: Int, month: String, asOf: String, value: JSON)?
    private var started = false
    private let folder: URL?

    /// `folder` replaces the app's library folder, for tests.
    init(folder: URL? = nil) { self.folder = folder }

    static func directory() throws -> URL {
        let base = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        return base.appendingPathComponent("UrBudget", isDirectory: true)
    }

    func start() throws -> BudgetSnapshot {
        if !started {
            try NativeCore.initialize(directory: folder ?? Self.directory())
            started = true
            AppLanguage.tellEngine()
        }
        return try load()
    }

    func load() throws -> BudgetSnapshot {
        let reply = try NativeCore.service("load_budget")
        adopt(reply, budget: reply["budget"])
        return snapshot()
    }

    /// A cheap probe when the app returns: reloads only when the revision changed (sync, schedules).
    func refresh() throws -> BudgetSnapshot? {
        guard started else { return nil }
        let status = try NativeCore.service("native_budget_status")
        if status.text("token") == token { return nil }
        return try load()
    }

    /// Enters scheduled transactions that have come due, as desktop does when it opens.
    func applyDueSchedules() throws -> BudgetSnapshot? {
        guard started, !isNew else { return nil }
        let before = token
        let reply = try NativeCore.service("apply_due_schedules", ["through": .string(Dates.today)])
        guard reply.text("token") != before else { return nil }
        adopt(reply, budget: reply["budget"])
        return snapshot()
    }

    func selectMonth(_ value: String) throws -> BudgetSnapshot {
        guard Dates.isMonth(value) else { throw AppError(String(localized: "Choose a valid month.")) }
        month = value
        return snapshot()
    }

    func create(name: String, currency: String, sample: Bool, localOnly: Bool = false, adjust: (@Sendable (inout JSON) -> Void)? = nil) throws -> BudgetSnapshot {
        let params: JSON = ["name": .string(name.trimmingCharacters(in: .whitespaces)), "currency": .string(currency.uppercased()), "month": .string(month)]
        var budget = try NativeCore.engine(sample ? "sampleBudget" : "defaultBudget", params: params)
        adjust?(&budget)
        let id = try NativeCore.service("create_budget", ["budget": budget, "localOnly": .bool(sample || localOnly)]).string
        return try open(id: id)
    }

    func importBudget(_ data: Data) throws -> BudgetSnapshot {
        try persist(Self.backup(data))
    }

    /// A JSON backup as a budget of its own in the library, which then opens (or fills a new, empty library).
    func importAsNew(_ data: Data) throws -> BudgetSnapshot {
        let budget = try Self.backup(data)
        let id = try NativeCore.service("create_budget", ["budget": budget]).string
        return try open(id: id)
    }

    private static func backup(_ data: Data) throws -> JSON {
        guard data.count <= 32 * 1024 * 1024 else { throw AppError(String(localized: "Choose a JSON backup smaller than 32 MB.")) }
        // Desktop JSON backups are bare budgets; also accept the documented versioned export envelope.
        let bom = Data([0xEF, 0xBB, 0xBF])
        let root = try JSON.parse(data.starts(with: bom) ? data.dropFirst(3) : data)
        let value = root.has("schemaVersion") ? root : root["budget"]
        guard value.has("schemaVersion") else { throw AppError(String(localized: "This file is not a portable UrBudget JSON backup.")) }
        return value
    }

    func edit(_ transform: @Sendable (inout JSON) throws -> Void) throws -> BudgetSnapshot {
        try saving { budget in
            var budget = budget
            try transform(&budget)
            return budget
        }
    }

    /// An edit, then engine commands each on the last one's result, saved as one change (one undo).
    func edit(_ transform: @Sendable (inout JSON) throws -> Void, then commands: [(String, JSON)]) throws -> BudgetSnapshot {
        try saving { budget in
            var budget = budget
            try transform(&budget)
            for (method, params) in commands { budget = try Self.changed(by: method, params, budget) }
            return budget
        }
    }

    /// With `original`, refuses when that record changed since an editor opened it.
    func command(_ method: String, _ params: JSON, original: (collection: String, record: JSON)? = nil) throws -> BudgetSnapshot {
        try saving { budget in
            if let original { try Self.verify(budget, original.collection, original.record) }
            return try Self.changed(by: method, params, budget)
        }
    }

    /// Makes a change on the open budget and saves it. When the budget changed underneath just
    /// before (sync saving what it downloaded), the change is made again on that version, once:
    /// an edit or a command means the same there, and an editor's `original` check still refuses
    /// a record that changed since it opened. Only a second refusal reaches the person.
    private func saving(_ change: (JSON) throws -> JSON) throws -> BudgetSnapshot {
        guard let budget = current else { throw AppError(String(localized: "Create or import a budget first.")) }
        do {
            return try persist(change(budget))
        } catch is Conflict {
            guard let fresh = current else { throw AppError(String(localized: "Create or import a budget first.")) }
            return try persist(change(fresh))
        }
    }

    /// An editor's record must still be the one it opened, so it never saves over a change from sync.
    static func verify(_ budget: JSON, _ collection: String, _ original: JSON) throws {
        let identity = collection == "loans" ? "accountId" : "id"
        let current = budget.rows(collection).first { $0.text(identity) == original.text(identity) }
        if current != original {
            throw AppError(String(localized: "Your budget changed while this editor was open. Close it and reopen to review the latest values."))
        }
    }

    private static func changed(by method: String, _ params: JSON, _ budget: JSON) throws -> JSON {
        let value = try NativeCore.engine(method, budget: budget, params: params)
        let changed = value["budget"].isObject ? value["budget"] : value
        guard changed.has("schemaVersion") else { throw AppError(String(localized: "This command did not return an editable budget.")) }
        return changed
    }

    func undo() throws -> BudgetSnapshot { try history("undo_budget") }
    func redo() throws -> BudgetSnapshot { try history("redo_budget") }

    private func history(_ method: String) throws -> BudgetSnapshot {
        do {
            let reply = try NativeCore.service(method, ["expectedToken": .string(token)])
            adopt(reply, budget: reply["budget"])
            return snapshot()
        } catch let error as AppError where error.message.contains("CONFLICT") {
            throw Conflict(snapshot: try load())
        }
    }

    /// A service that commits to the open budget (bank and YNAB imports, sync, assistant changes):
    /// it carries the revision this repository holds, then the committed budget is reloaded.
    func commitService(_ method: String, _ params: JSON) throws -> (JSON, BudgetSnapshot) {
        var request = params
        if !request.has("expectedToken") && !token.isEmpty { request["expectedToken"] = .string(token) }
        do {
            let value = try NativeCore.service(method, request)
            return (value, try load())
        } catch let error as AppError where error.message.contains("CONFLICT") {
            throw Conflict(snapshot: try load())
        }
    }

    /// The revision of the open budget, for services that check it themselves.
    var revision: String { token }

    func listBudgets() throws -> [JSON] { try NativeCore.service("library_snapshot")["entries"].array }

    func open(id: String) throws -> BudgetSnapshot {
        let reply = try NativeCore.service("library_open", ["id": .string(id)])
        adopt(reply, budget: reply["budget"])
        return snapshot()
    }

    func deleteBudget(id: String) throws -> [JSON] { try NativeCore.service("library_trash", ["id": .string(id)])["library"]["entries"].array }

    /// The open budget as a portable JSON backup.
    func exportData() -> Data? {
        guard let current else { return nil }
        return try? JSONSerialization.data(withJSONObject: current.foundation, options: [.prettyPrinted, .sortedKeys])
    }

    private func persist(_ budget: JSON) throws -> BudgetSnapshot {
        // calculate validates through the shared engine before anything is written.
        let asOf = Dates.currentMonth
        let value = try NativeCore.engine("calculate", budget: budget, params: ["month": .string(month), "asOfMonth": .string(asOf)])
        let reply: JSON
        do {
            reply = try NativeCore.service("save_budget", ["budget": budget, "expectedToken": .string(token), "ackOnly": true])
        } catch let error as AppError where error.message.contains("CONFLICT") {
            throw Conflict(snapshot: try load())
        }
        token = reply.text("token")
        isNew = reply["isNew"].bool
        identity = reply.text("databasePath").isEmpty ? identity : reply.text("databasePath")
        undoAvailable = reply["undoAvailable"].bool
        redoAvailable = reply["redoAvailable"].bool
        current = budget
        version += 1
        figures = (version, month, asOf, value)
        Money.configure(budget: budget)
        return snapshot()
    }

    private func adopt(_ reply: JSON, budget: JSON) {
        token = reply.text("token")
        isNew = reply["isNew"].bool
        let path = reply.text("databasePath")
        identity = path.isEmpty ? nil : path
        undoAvailable = reply["undoAvailable"].bool
        redoAvailable = reply["redoAvailable"].bool
        current = isNew || !budget.isObject ? nil : budget
        version += 1
        if let current { Money.configure(budget: current) }
    }

    private func snapshot() -> BudgetSnapshot {
        var snapshot = BudgetSnapshot(budget: current, summary: nil, calculationError: nil, month: month, version: version,
                                      identity: identity, revision: token, undoAvailable: undoAvailable, redoAvailable: redoAvailable)
        guard let current else { return snapshot }
        let asOf = Dates.currentMonth
        if let figures, figures.version == version, figures.month == month, figures.asOf == asOf {
            snapshot.summary = figures.value
            return snapshot
        }
        do {
            // calculate validates through the shared engine before producing any totals.
            let value = try NativeCore.engine("calculate", budget: current, params: ["month": .string(month), "asOfMonth": .string(asOf)])
            figures = (version, month, asOf, value)
            snapshot.summary = value
        } catch {
            snapshot.calculationError = error.localizedDescription
        }
        return snapshot
    }
}

/// A short message above the tab bar, with Undo for the last change when it offers it, or
/// another action (adding a payee rule) in its place.
struct Toast: Identifiable, Equatable {
    let id = UUID()
    var message: String
    var undo: Bool
    var action: ToastAction?

    static func == (a: Toast, b: Toast) -> Bool { a.id == b.id }
}

struct ToastAction {
    var label: String
    var run: @MainActor () -> Void
}

/// The app's one budget, published to the screens on the main thread. Every change goes through the
/// repository in the order it was made; reads and previews run beside it on the published snapshot.
@MainActor @Observable
final class BudgetStore {
    private(set) var loading = true
    private(set) var busy = false
    private(set) var budget: JSON?
    private(set) var summary: JSON?
    private(set) var calculationError: String?
    private(set) var month = Dates.currentMonth
    /// Changes whenever the budget does, so screens can key derived models on it cheaply.
    private(set) var version = 0
    private(set) var identity: String?
    /// The published budget's storage revision. A review screen (bank or YNAB import) passes the
    /// revision it showed as `expectedToken`, so the commit is refused if the budget changed since.
    private(set) var revision = ""
    private(set) var undoAvailable = false
    private(set) var redoAvailable = false
    var error: String?
    var toast: Toast?

    /// Set by the welcome screen as it creates a budget; Budget shows that budget the getting-started steps.
    @ObservationIgnored var setUpFromWelcome = false

    @ObservationIgnored let repository = BudgetRepository()
    @ObservationIgnored private var tail: Task<Void, Never>?
    @ObservationIgnored private var pending = 0
    @ObservationIgnored private var lastForeground = Date.distantPast

    var currency: String { budget?.text("currency") ?? "SAR" }
    /// The open budget's file name, for settings kept per budget on this device (the container's path can move).
    var budgetKey: String? { identity.map { URL(fileURLWithPath: $0).lastPathComponent } }
    var monthFirst: Bool { budget?.text("dateFormat") == "mdy" }

    func start() { enqueue(showBusy: false) { try await $0.start() } }

    /// Returning to the app (or pulling to refresh): enter schedules that came due and pick up changes from sync.
    func foreground(force: Bool = false) {
        guard budget != nil, force || Date().timeIntervalSince(lastForeground) > 30 else { return }
        lastForeground = Date()
        // Android's "Enter scheduled transactions automatically", on by default.
        let enterSchedules = UserDefaults.standard.object(forKey: "automaticSchedules") as? Bool ?? true
        enqueue(showBusy: false, quiet: true) { repository in
            if enterSchedules, let entered = try await repository.applyDueSchedules() { return entered }
            return try await repository.refresh()
        }
    }

    /// Runs `operation` after every change queued before it, then publishes what it returns.
    func enqueue(showBusy: Bool = true, quiet: Bool = false,
                 _ operation: @escaping @Sendable (BudgetRepository) async throws -> BudgetSnapshot?,
                 then: (@MainActor () -> Void)? = nil) {
        Task { @MainActor in
            do {
                try await perform(showBusy: showBusy) { repository in ((), try await operation(repository)) }
                then?()
            } catch {
                if !quiet { self.error = Self.readable(error) }
            }
        }
    }

    static let conflictMessage = String(localized: "Your budget changed while saving, from sync or another device. The latest version is shown; make the change again if it's still needed.")

    /// Like `enqueue`, but returns the operation's result (or throws its error) to the caller.
    /// The snapshot it returns, if any, is published first.
    @discardableResult
    func perform<T: Sendable>(showBusy: Bool = true,
                              _ operation: @escaping @Sendable (BudgetRepository) async throws -> (T, BudgetSnapshot?)) async throws -> T {
        let previous = tail
        pending += 1
        if showBusy { busy = true }
        let repository = repository
        let task = Task { @MainActor () async throws -> T in
            await previous?.value
            defer {
                pending -= 1
                if pending == 0 { busy = false }
                loading = false
            }
            do {
                let before = revision
                let (value, snapshot) = try await operation(repository)
                if let snapshot {
                    publish(snapshot)
                    // Only a new saved revision is a change worth syncing; reloads and month changes aren't.
                    if snapshot.revision != before { changed() }
                }
                return value
            } catch let conflict as BudgetRepository.Conflict {
                publish(conflict.snapshot)
                throw AppError(Self.conflictMessage)
            }
        }
        // Later changes wait for this one whether it succeeds or not.
        tail = Task { _ = try? await task.value }
        return try await task.value
    }

    /// Sheet teardown waits for its already queued save before clearing recovery.
    func afterPending(_ action: @escaping @MainActor () -> Void) {
        enqueue(showBusy: false, quiet: true, { _ in nil }, then: action)
    }

    /// Reloads the open budget from storage, after a change made outside the edit queue.
    func reload() { enqueue(showBusy: false, quiet: true) { try await $0.load() } }

    /// A storage, account, sync, bank or assistant service (`urbudget-services`), off the main thread.
    /// With `changesBudget`, it runs in the edit queue with the open budget's revision as
    /// `expectedToken`, and the budget reloads afterwards, so it never commits over a newer change.
    func service(_ method: String, _ params: JSON = [:], changesBudget: Bool = false) async throws -> JSON {
        guard changesBudget else {
            return try await Task.detached(priority: .userInitiated) { try NativeCore.service(method, params) }.value
        }
        return try await perform { repository in try await repository.commitService(method, params) }
    }

    /// Called after every change this app commits; sync hooks in here.
    @ObservationIgnored var onChange: (@MainActor () -> Void)?
    private func changed() { onChange?() }

    /// Service errors start with a code ("CONFLICT: …"); people see the sentence after it.
    nonisolated static func readable(_ error: Error) -> String {
        let message = error.localizedDescription
        if let range = message.range(of: #"^[A-Z_]{3,}: "#, options: .regularExpression) {
            return String(message[range.upperBound...])
        }
        return message
    }

    private func publish(_ snapshot: BudgetSnapshot) {
        if snapshot.version != version { budget = snapshot.budget }
        summary = snapshot.summary
        calculationError = snapshot.calculationError
        month = snapshot.month
        version = snapshot.version
        identity = snapshot.identity
        revision = snapshot.revision
        undoAvailable = snapshot.undoAvailable
        redoAvailable = snapshot.redoAvailable
        loading = false
    }

    func showToast(_ message: String, undo: Bool = false, action: ToastAction? = nil) {
        toast = Toast(message: message, undo: undo, action: action)
    }

    // MARK: Library and history

    func createBudget(name: String, currency: String, sample: Bool = false,
                      adjust: (@Sendable (inout JSON) -> Void)? = nil, then: (@MainActor () -> Void)? = nil) {
        enqueue({ try await $0.create(name: name, currency: currency, sample: sample, adjust: adjust) }, then: then)
    }

    func importBudget(_ data: Data) { enqueue { try await $0.importBudget(data) } }
    func importAsNewBudget(_ data: Data, then: (@MainActor () -> Void)? = nil) { enqueue({ try await $0.importAsNew(data) }, then: then) }
    func selectMonth(_ month: String) { enqueue(showBusy: false) { try await $0.selectMonth(month) } }
    func undo() { enqueue { try await $0.undo() } }
    func redo() { enqueue { try await $0.redo() } }
    func openBudget(id: String) { enqueue { try await $0.open(id: id) } }
    func listBudgets() async throws -> [JSON] { try await repository.listBudgets() }
    func deleteBudget(id: String) async throws -> [JSON] {
        try await perform { repository in
            let entries = try await repository.deleteBudget(id: id)
            return (entries, try await repository.load())
        }
    }
    func exportData() async -> Data? { await repository.exportData() }

    // MARK: Changes

    func edit(then: (@MainActor () -> Void)? = nil, _ transform: @escaping @Sendable (inout JSON) throws -> Void) {
        enqueue({ try await $0.edit(transform) }, then: then)
    }

    func runCommand(_ method: String, _ params: JSON, then: (@MainActor () -> Void)? = nil) {
        enqueue({ try await $0.command(method, params) }, then: then)
    }

    /// A budget workflow (assign, move money, quick assign, bulk edits…) in the shared engine.
    func workflow(_ action: JSON, then: (@MainActor () -> Void)? = nil) {
        runCommand("workflow", ["action": action], then: then)
    }

    /// Merges edited fields into a record. With `original`, refuses when the record changed since an editor opened it.
    func upsert(_ collection: String, _ record: JSON, original: JSON? = nil, then: (@MainActor () -> Void)? = nil) {
        edit(then: then) { budget in
            if let original { try BudgetRepository.verify(budget, collection, original) }
            try budget.upsertRecord(collection, record)
        }
    }

    func upsertMany(_ collection: String, _ records: [JSON], then: (@MainActor () -> Void)? = nil) {
        edit(then: then) { budget in for record in records { try budget.upsertRecord(collection, record) } }
    }

    // MARK: Awaited changes
    // Sheets await these, so they close and confirm only once the change is saved; if it fails
    // they stay open with the person's input and the error.

    func editing(_ transform: @escaping @Sendable (inout JSON) throws -> Void) async throws {
        try await perform { ((), try await $0.edit(transform)) }
    }

    func committing(_ method: String, _ params: JSON, original: (collection: String, record: JSON)? = nil) async throws {
        try await perform { ((), try await $0.command(method, params, original: original)) }
    }

    func committingWorkflow(_ action: JSON) async throws {
        try await committing("workflow", ["action": action])
    }

    func upserting(_ collection: String, _ record: JSON, original: JSON? = nil) async throws {
        try await editing { budget in
            if let original { try BudgetRepository.verify(budget, collection, original) }
            try budget.upsertRecord(collection, record)
        }
    }

    func removing(_ collection: String, id: String) async throws {
        try await editing { try $0.removeRecord(collection, id: id) }
    }

    func remove(_ collection: String, id: String, then: (@MainActor () -> Void)? = nil) {
        edit(then: then) { try $0.removeRecord(collection, id: id) }
    }

    func assign(category: String, amount: Int64, then: (@MainActor () -> Void)? = nil) {
        workflow(["type": "assignAmount", "month": .string(month), "categoryId": .string(category), "amount": .int(amount)], then: then)
    }

    /// Saves a new or edited transaction. An edit is refused if sync changed it since `original` was opened.
    func saveTransaction(_ record: JSON, original: JSON? = nil) async throws {
        var patch = record
        if patch.text("id").isEmpty { patch["id"] = .string(UUID().uuidString.lowercased()) }
        for (key, fallback) in [("memo", JSON.string("")), ("status", "uncleared"), ("transferAccountId", .null), ("transferStatus", .null), ("splits", .array([]))]
            where !patch.has(key) { patch[key] = fallback }
        let params: JSON = ["transaction": patch, "through": .string(Dates.today)]
        let guarded = original.map { (collection: "transactions", record: $0) }
        try await committing("saveTransaction", params, original: guarded)
    }

    /// A new transaction that repeats, as the desktop saves one: its schedule from the transaction's
    /// date and, when that date has come, the transaction itself entered with the schedule moved on.
    func saveRepeating(schedule: JSON, entered: JSON?) async throws {
        let id = schedule.text("id")
        let commands: [(String, JSON)] = entered.map { entered in
            [("saveTransaction", ["transaction": entered, "through": .string(Dates.today)]),
             ("applySchedules", ["through": .string(entered.text("date")), "selected": .string(id), "skip": true])]
        } ?? []
        try await perform { ((), try await $0.edit({ try $0.upsertRecord("schedules", schedule) }, then: commands)) }
    }

    func saveAccount(id: String? = nil, name: String, kind: String, openingBalance: Int64, accountType: String?, note: String = "", netWorth: Bool = true) async throws {
        let recordId = id ?? UUID().uuidString.lowercased()
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        try await editing { budget in
            let existing = budget.row("accounts", id: recordId)
            var record: JSON = ["id": .string(recordId), "name": .string(trimmed), "kind": .string(kind),
                                "openingBalance": .int(openingBalance), "archived": .bool(existing?.flag("archived") ?? false)]
            if let accountType { record["accountType"] = .string(accountType) }
            if !note.isEmpty || existing?.has("note") == true { record["note"] = .string(note) }
            if !netWorth || existing?.has("netWorth") == true { record["netWorth"] = .bool(netWorth) }
            try budget.upsertRecord("accounts", record)
            // A credit card gets its payment category, as on the desktop.
            if kind == "credit" && !budget.rows("categories").contains(where: { $0.text("paymentAccountId") == recordId }) {
                try budget.upsertRecord("categories", ["id": .string(UUID().uuidString.lowercased()), "name": .string(trimmed),
                                                       "group": "Credit Card Payments", "target": 0, "paymentAccountId": .string(recordId)])
            }
        }
    }

    /// Adds a category at the end of its group and returns its id.
    @discardableResult
    func saveCategory(name: String, group: String) async throws -> String {
        let id = UUID().uuidString.lowercased()
        try await upserting("categories", ["id": .string(id), "name": .string(name.trimmingCharacters(in: .whitespaces)),
                                           "group": .string(group.trimmingCharacters(in: .whitespaces)), "target": 0])
        return id
    }

    func saveTarget(category: String, goal: JSON?, amount: Int64) async throws {
        try await editing { budget in
            var rows = budget["categories"].array
            guard let index = rows.firstIndex(where: { $0.text("id") == category }) else { throw AppError(String(localized: "Category not found.")) }
            if let goal { rows[index]["goal"] = goal; rows[index]["target"] = .int(amount) }
            else { rows[index].remove("goal"); rows[index]["target"] = 0 }
            budget["categories"] = .array(rows)
        }
    }

    // MARK: Reads

    /// An engine request against the published budget, beside the edit queue (previews, reports).
    func query(_ method: String, _ params: JSON = [:]) async throws -> JSON {
        let budget = budget
        return try await Task.detached(priority: .userInitiated) {
            try NativeCore.engine(method, budget: budget, params: params)
        }.value
    }
}
