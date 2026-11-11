#if DEBUG
import Foundation

/// Debug builds only, and only against a development server on this Mac (`wrangler dev`, named
/// by URBUDGET_API): signs a synthetic account in and sets sync up from launch arguments, as
/// Android's `SyncV2RuntimeTest` does through the same services, so sync can be seen working on
/// a Simulator without typing. `-syncQa <email> <passphrase>`; `-syncQaBudget` first makes a
/// budget where there's none.
@MainActor enum SyncQA {
    private static var ran = false

    static func run(store: BudgetStore, sync: SyncCoordinator) async {
        let arguments = ProcessInfo.processInfo.arguments
        guard !ran, let index = arguments.firstIndex(of: "-syncQa"), arguments.indices.contains(index + 2),
              let server = ProcessInfo.processInfo.environment["URBUDGET_API"],
              server.hasPrefix("http://127.0.0.1:") || server.hasPrefix("http://localhost:") else { return }
        ran = true
        let email = arguments[index + 1], passphrase = arguments[index + 2]
        do {
            // A sample budget stays on its device by design, so the one made here is an ordinary one.
            if arguments.contains("-syncQaBudget"), store.budget == nil {
                store.createBudget(name: "Sync QA \(Int(Date().timeIntervalSince1970) % 100_000)", currency: "SAR")
                for _ in 0..<100 where store.budget == nil { try await Task.sleep(for: .milliseconds(100)) }
            }
            await sync.refreshStatus()
            if !sync.signedIn {
                let start = try await store.service("account_start", ["email": .string(email)])
                _ = try await store.service("account_verify", ["email": .string(email), "code": .string(start.text("devCode"))])
            }
            await sync.refreshStatus()
            if !sync.unlocked { _ = try await store.service("sync_setup", ["passphrase": .string(passphrase)]) }
            var report = try await sync.syncNow()
            try await Task.sleep(for: .seconds(3))
            report = try await sync.syncNow()
            await sync.refreshStatus()
            let library = try await store.service("library_snapshot")
            // A device with nothing open opens what sync brought it.
            if store.budget == nil, let first = library.rows("entries").first(where: { ["", "active"].contains($0.text("lifecycle")) }) {
                store.openBudget(id: first.text("id"))
            } else {
                store.reload()
            }
            NSLog("SYNCQA done protocol=%@ state=%@ report=%@ library=%@", String(describing: sync.status?["sync"]["protocol"].int), report.text("state"),
                  report.rows("budgets").map { "\($0.text("name")):\($0.text("state"))" }.joined(separator: ", "),
                  library.rows("entries").map { "\($0.text("name")):\($0.text("scope")):\($0.flag("pending") ? "pending" : "sent")" }.joined(separator: ", "))
        } catch {
            NSLog("SYNCQA failed %@", BudgetStore.readable(error))
        }
    }
}
#endif
