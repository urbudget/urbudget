import Foundation
import Observation
import UIKit
import BackgroundTasks
import Network

/// Encrypted sync while the app is open, as Android's MainActivity runs it: shortly after each
/// change, when the app comes to the foreground, and every thirty seconds. One attempt at a time; only
/// when signed in (locked v2 accounts still reconcile the catalog); targeted revisions refresh the open budget.
@MainActor @Observable
final class SyncCoordinator {
    /// The latest account and sync status (`cloud_status`), for badges and the sync screen.
    private(set) var status: JSON?
    private(set) var syncing = false
    private(set) var pending = false

    @ObservationIgnored private weak var store: BudgetStore?
    @ObservationIgnored private var soon: Task<Void, Never>?
    @ObservationIgnored private var periodic: Task<Void, Never>?
    @ObservationIgnored private var monitor: NWPathMonitor?
    @ObservationIgnored private var background: Task<Void, Never>?
    @ObservationIgnored private var backgroundID: UIBackgroundTaskIdentifier = .invalid
    @ObservationIgnored private var batchStarted: Date?
    @ObservationIgnored private var consumedReport: JSON?
    @ObservationIgnored private var lastAttempt = Date.distantPast
    private var isV2: Bool { status?["sync"]["protocol"].int == 2 }

    static let automaticKey = "automaticSync"
    static var automatic: Bool { UserDefaults.standard.object(forKey: automaticKey) as? Bool ?? true }

    func attach(_ store: BudgetStore) {
        self.store = store
        store.onChange = { [weak self] in self?.syncSoon() }
    }

    var signedIn: Bool { !(status?.text("email").isEmpty ?? true) }
    var unlocked: Bool { status?["sync"]["unlocked"].bool ?? false }
    /// The last sync attempt's report, if any.
    var lastReport: JSON? { status?["sync"]["last"].isObject == true ? status?["sync"]["last"] : nil }
    var conflicts: Int { lastReport?.rows("budgets").count { ["conflict", "error", "update_app", "recovery", "subscription_required", "deletion_conflict"].contains($0.text("state")) } ?? 0 }
    var statusLabel: String {
        if !unlocked && (status?["sync"]["enabled"].bool == true || lastReport != nil) { return String(localized: "Sync paused") }
        if syncing { return String(localized: "Syncing…") }
        if conflicts > 0 { return String(localized: "Sync needs you") }
        guard let report = lastReport else { return String(localized: "Sync") }
        if report.text("state") == "offline" { return String(localized: "Offline") }
        let states = report.rows("budgets").map { $0.text("state") }
        if report.text("state") == "waiting" || states.contains("queued") { return String(localized: "Waiting to sync") }
        if states.contains(where: { ["paused", "locked", "signed_out"].contains($0) }) { return String(localized: "Sync paused") }
        if report.text("state") != "ok" { return String(localized: "Sync needs you") }
        if pending || states.contains("waiting") { return String(localized: "Waiting to sync") }
        if states.isEmpty || states.contains("local_only") { return String(localized: "Saved on device") }
        return String(localized: "Synced")
    }

    func refreshStatus() async {
        if let next = try? await store?.service("cloud_status") { status = next }
        if let library = try? await store?.service("library_snapshot") {
            pending = library.rows("entries").contains { $0.flag("pending") }
            // A headless BGTask may have completed before the foreground coordinator started.
            if let store, let report = lastReport, report != consumedReport {
                consumedReport = report
                if syncInvalidatesActive(report, library: library, identity: store.identity, revision: store.revision) { store.reload() }
            }
        }
    }

    /// The app is in the foreground: sync now and then every minute until it leaves.
    func start() {
        guard periodic == nil else { return }
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [weak self] path in
            if path.status == .satisfied { Task { @MainActor in
                guard let self, Date().timeIntervalSince(self.lastAttempt) >= (self.isV2 ? 5 : 60) else { return }
                await self.syncAutomatically()
            } }
        }
        monitor.start(queue: DispatchQueue(label: "app.urbudget.sync-connectivity"))
        self.monitor = monitor
        periodic = Task { [weak self] in
            while !Task.isCancelled {
                await self?.syncAutomatically()
                try? await Task.sleep(for: .seconds(self?.isV2 == true ? 30 : 300))
            }
        }
    }

    func stop() {
        periodic?.cancel()
        periodic = nil
        soon?.cancel()
        soon = nil
        batchStarted = nil
        monitor?.cancel()
        monitor = nil
    }

    /// The OS may end this grant at any time. Pending native work survives the process.
    func enterBackground() {
        stop()
        SyncBackground.schedule(eligible: syncBackgroundEligible(status ?? [:]))
        guard Self.automatic, background == nil else { return }
        backgroundID = UIApplication.shared.beginBackgroundTask(withName: "Finish budget sync") { [weak self] in
            self?.finishBackground()
        }
        background = Task { [weak self] in
            await self?.syncAutomatically()
            self?.finishBackground()
        }
    }

    private func finishBackground() {
        background?.cancel()
        background = nil
        if backgroundID != .invalid {
            UIApplication.shared.endBackgroundTask(backgroundID)
            backgroundID = .invalid
        }
    }

    /// A change was saved: sync a few seconds later, once edits settle.
    func syncSoon() {
        pending = true
        SyncBackground.schedule(eligible: syncBackgroundEligible(status ?? [:]))
        if batchStarted == nil { batchStarted = Date() }
        let wait = max(0, min(isV2 ? 2 : 8, 15 - Date().timeIntervalSince(batchStarted!)))
        soon?.cancel()
        soon = Task { [weak self] in
            try? await Task.sleep(for: .seconds(wait))
            if !Task.isCancelled {
                self?.batchStarted = nil
                await self?.syncAutomatically()
            }
        }
    }

    func syncAutomatically() async {
        guard Self.automatic else { await refreshStatus(); return }
        await refreshStatus()
        guard syncCanReconcile(status ?? [:]) else { return }
        _ = try? await syncNow()
    }

    /// One sync of every tracked budget. Returns the report; the open budget reloads if it changed.
    @discardableResult
    func syncNow() async throws -> JSON {
        guard let store, !syncing else { return lastReport ?? [:] }
        syncing = true
        lastAttempt = Date()
        defer { syncing = false }
        let report: JSON
        do { report = try await store.service("sync_now") }
        catch { await refreshStatus(); throw error }
        let library = try await store.service("library_snapshot")
        consumedReport = report
        if syncInvalidatesActive(report, library: library, identity: store.identity, revision: store.revision) { store.reload() }
        await refreshStatus()
        return report
    }
}


/// Registered at launch, so a system wake can reconcile the account without a visible editor.
@MainActor
enum SyncBackground {
    static let identifier = "app.urbudget.account-sync"

    static func register() {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: identifier, using: nil) { task in
            Task { @MainActor in handle(task) }
        }
    }

    static func schedule(eligible: Bool) {
        guard SyncCoordinator.automatic, eligible else { BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: identifier); return }
        let request = BGAppRefreshTaskRequest(identifier: identifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        // A request may already be queued. The native outbox remains the source of pending work.
        try? BGTaskScheduler.shared.submit(request)
    }

    private static func handle(_ task: BGTask) {
        let completion = SyncBackgroundCompletion(task)
        let work = Task {
            guard SyncCoordinator.automatic else { completion.finish(true); return }
            guard UIApplication.shared.isProtectedDataAvailable else { completion.finish(false); return }
            let result = await Task.detached(priority: .utility) { () -> (success: Bool, eligible: Bool) in
                do {
                    try NativeCore.initialize(directory: BudgetRepository.directory())
                    let status = try NativeCore.service("cloud_status")
                    guard syncBackgroundEligible(status) else { return (true, false) }
                    let report = try NativeCore.service("sync_now")
                    return (report.text("state") == "ok", true)
                } catch { return (false, true) }
            }.value
            if !Task.isCancelled { schedule(eligible: result.eligible) }
            completion.finish(result.success)
        }
        task.expirationHandler = {
            work.cancel()
            completion.finish(false)
        }
    }
}

/// Expiration must complete synchronously, even if the main actor is busy finishing a UI save.
private final class SyncBackgroundCompletion: @unchecked Sendable {
    private let task: BGTask
    private let lock = NSLock()
    private var completed = false
    init(_ task: BGTask) { self.task = task }
    func finish(_ success: Bool) {
        lock.lock()
        defer { lock.unlock() }
        guard !completed else { return }
        completed = true
        task.setTaskCompleted(success: success)
    }
}

/// The UI may have switched budgets while awaiting the network. Resolve its identity on completion.
func syncInvalidatesActive(_ report: JSON, library: JSON, identity: String?, revision: String) -> Bool {
    let rows = library.rows("entries")
    let active = rows.first { $0.text("id") == library.text("activeId") }
    let activePath = active.map { $0.text("path") }.flatMap { $0.isEmpty ? nil : $0 }
    if activePath != identity { return true }
    guard report.has("changedBudgets") else { return report.flag("openChanged") }
    guard let id = rows.first(where: { $0.text("path") == identity })?.text("id") else { return false }
    return report.rows("changedBudgets").contains { $0.text("id") == id && ($0.text("revision").isEmpty || $0.text("revision") != revision) }
}

func syncCanReconcile(_ status: JSON) -> Bool {
    !status.text("email").isEmpty && (status["sync"]["protocol"].int == 2 || status["sync"]["unlocked"].bool)
}

func syncBackgroundEligible(_ status: JSON) -> Bool {
    syncCanReconcile(status) && status["sync"]["protocol"].int == 2 && status["sync"].flag("enabled")
}
