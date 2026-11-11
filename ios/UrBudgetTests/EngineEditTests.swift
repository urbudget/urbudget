import XCTest
@testable import UrBudget

/// The engine's services open one library folder per process, so every test that needs the real
/// store shares this one (and each creates the budgets it works on).
enum TestLibrary {
    static let folder: URL = {
        let temporary = FileManager.default.temporaryDirectory
        // What earlier runs on this simulator left behind.
        for old in (try? FileManager.default.contentsOfDirectory(at: temporary, includingPropertiesForKeys: nil)) ?? []
            where old.lastPathComponent.hasPrefix("urbudget-tests-") { try? FileManager.default.removeItem(at: old) }
        return temporary.appendingPathComponent("urbudget-tests-\(UUID().uuidString)")
    }()

    static func repository() async throws -> BudgetRepository {
        let repository = BudgetRepository(folder: folder)
        _ = try await repository.start()
        return repository
    }
}

/// What sync does to the open budget behind the app's back: a commit the repository hasn't seen.
enum EngineEdits {
    static func elsewhere(_ patch: JSON, at snapshot: BudgetSnapshot) throws {
        _ = try NativeCore.service("edit_budget", ["steps": [["patch": patch]], "expectedToken": .string(snapshot.revision),
                                                   "month": .string(snapshot.month), "asOfMonth": .string(Dates.currentMonth)])
    }

    /// The rows as stored, heavy extras and all.
    static func stored() throws -> JSON { try NativeCore.service("saved_budget")["budget"] }
}

final class BudgetPatchTests: XCTestCase {
    private func cases() throws -> [JSON] {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "patch-cases", withExtension: "json"))
        return try JSON.parse(Data(contentsOf: url)).array
    }

    /// shared/fixtures/patch-cases.json, the cases Rust, Kotlin and TypeScript apply too.
    func testTheCasesEveryClientShares() throws {
        let cases = try cases()
        XCTAssertGreaterThan(cases.count, 10)
        for item in cases {
            XCTAssertEqual(BudgetPatch.apply(item["patch"], to: item["before"]), item["after"], item.text("name"))
        }
    }

    func testAnEmptyPatchGivesTheBudgetBack() throws {
        let before = try XCTUnwrap(cases().first)["before"]
        XCTAssertEqual(BudgetPatch.apply([:], to: before), before)
        XCTAssertEqual(BudgetPatch.diff(from: before, to: before), [:])
    }

    /// A diff made here, applied here, gives the edited budget, for every shared case.
    func testADiffAppliedIsTheEditedBudget() throws {
        for item in try cases() {
            let (before, after) = (item["before"], item["after"])
            let patch = BudgetPatch.diff(from: before, to: after)
            XCTAssertTrue(BudgetPatch.same(BudgetPatch.apply(patch, to: before), after), item.text("name"))
        }
    }

    func testADiffNamesOnlyWhatChanged() throws {
        let before: JSON = ["schemaVersion": 2, "name": "Home", "currency": "SAR",
                            "accounts": [["id": "cash", "name": "Cash"]],
                            "categories": [["id": "food", "name": "Food"], ["id": "rent", "name": "Rent"]],
                            "transactions": [["id": "t1", "memo": ""], ["id": "t2", "memo": ""], ["id": "t3", "memo": ""]],
                            "assignments": [["month": "2026-10", "categoryId": "food", "amount": 100]],
                            "payees": [["id": "p1", "name": "Shop"]], "dateFormat": "dmy", "sample": [:]]
        var after = before
        try after.upsertRecord("transactions", ["id": "t2", "memo": "Changed"])
        try after.upsertRecord("transactions", ["id": "t4", "memo": "New"])
        try after.removeRecord("transactions", id: "t1")
        after["categories"] = [["id": "rent", "name": "Rent"], ["id": "food", "name": "Food"]]
        after["assignments"] = [["month": "2026-10", "categoryId": "food", "amount": 250], ["month": "2026-11", "categoryId": "rent", "amount": 5]]
        after["dateFormat"] = "mdy"
        after.remove("sample")

        let patch = BudgetPatch.diff(from: before, to: after)
        XCTAssertEqual(Set(patch.keys), ["transactions", "categories", "assignments", "extras"], "Accounts and the budget's own fields didn't change")
        XCTAssertEqual(patch["transactions"]["upsert"].array.map { $0.text("id") }, ["t2", "t4"])
        XCTAssertEqual(patch["transactions"]["delete"], ["t1"])
        XCTAssertFalse(patch["transactions"].has("order"), "Survivors in place and a new record last need no order")
        XCTAssertEqual(patch["categories"]["upsert"], [])
        XCTAssertEqual(patch["categories"]["order"], ["rent", "food"])
        XCTAssertEqual(patch["assignments"]["upsert"].array.count, 2)
        XCTAssertEqual(patch["extras"]["set"], ["dateFormat": "mdy"], "Payees are untouched, so they don't travel")
        XCTAssertEqual(patch["extras"]["delete"], ["sample"])
        XCTAssertEqual(BudgetPatch.apply(patch, to: before), after)
    }

    func testSameIgnoresTheOrderOfAssignmentsOnly() throws {
        let before = try XCTUnwrap(cases().first)["before"]
        var shuffled = before
        shuffled["assignments"] = .array(before["assignments"].array.reversed())
        XCTAssertTrue(BudgetPatch.same(before, shuffled))
        var moved = before
        moved["transactions"] = .array(before["transactions"].array.reversed())
        XCTAssertFalse(BudgetPatch.same(before, moved))
    }
}

/// Edits run in the store (`edit_budget`), and the app patches its own light copy with the reply.
final class EngineEditTests: XCTestCase {
    private var repository: BudgetRepository!

    override func setUp() async throws { repository = try await TestLibrary.repository() }

    private var category = ""

    /// A sample budget of this test's own, opened.
    private func sample(_ name: String) async throws -> BudgetSnapshot {
        let snapshot = try await repository.create(name: name, currency: "USD", sample: true)
        category = try XCTUnwrap(snapshot.budget?.rows("categories").first { $0.text("name") == "Groceries" }).text("id")
        return snapshot
    }

    private func transaction(_ id: String, memo: String = "", amount: Int64 = -450) -> JSON {
        ["id": .string(id), "date": .string(Dates.today), "accountId": "sample-checking", "payee": "Corner Cafe", "memo": .string(memo),
         "amount": .int(amount), "status": "uncleared", "transferAccountId": .null, "transferStatus": .null,
         "splits": [["categoryId": .string(category), "amount": .int(amount)]]]
    }

    /// The app's copy after every kind of change is what a fresh load of the store gives.
    func testThePatchedCopyIsWhatTheStoreHolds() async throws {
        var snapshot = try await sample("Edits")
        let groceries = category
        let created = snapshot.revision

        // A command.
        snapshot = try await repository.command("saveTransaction", ["transaction": transaction("e1"), "through": .string(Dates.today)])
        XCTAssertNotEqual(snapshot.revision, created)
        XCTAssertNotNil(snapshot.budget?.row("transactions", id: "e1"))
        XCTAssertTrue(snapshot.undoAvailable)
        // A record edited here, an extra set, and an extra removed.
        snapshot = try await repository.edit { budget in
            try budget.upsertRecord("transactions", ["id": "e1", "memo": "Edited here"])
            budget["dateFormat"] = "mdy"
            budget.remove("sample")
        }
        XCTAssertEqual(snapshot.budget?.row("transactions", id: "e1")?.text("memo"), "Edited here")
        XCTAssertEqual(snapshot.budget?.text("dateFormat"), "mdy")
        XCTAssertEqual(snapshot.budget?.has("sample"), false)
        // An edit and commands as one change.
        let before = snapshot
        snapshot = try await repository.edit({ try $0.upsertRecord("categories", ["id": "pets", "name": "Pets", "group": "Everyday", "target": 0]) },
                                             then: [("workflow", ["action": ["type": "assignAmount", "month": .string(snapshot.month), "categoryId": "pets", "amount": 1_500]]),
                                                    ("saveTransaction", ["transaction": transaction("e2", memo: "Second"), "through": .string(Dates.today)])])
        XCTAssertEqual(snapshot.summary?.rows("categories").first { $0.text("id") == "pets" }?.long("assigned"), 1_500)
        XCTAssertNotNil(snapshot.budget?.row("transactions", id: "e2"))
        // The totals that came with the edit are the store's own.
        let asked = try NativeCore.service("query_budget", ["method": "calculate", "params": ["month": .string(snapshot.month), "asOfMonth": .string(Dates.currentMonth)]])
        XCTAssertEqual(snapshot.summary, asked)
        XCTAssertEqual(snapshot.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 60_000)

        let shown = try XCTUnwrap(snapshot.budget)
        let loaded = try await repository.load()
        XCTAssertTrue(BudgetPatch.same(shown, try XCTUnwrap(loaded.budget)), "The patched copy drifted from the store")
        XCTAssertEqual(loaded.revision, snapshot.revision, "A load is not a change")

        // One undo takes the edit and both its commands back; redo brings them again.
        snapshot = try await repository.undo()
        XCTAssertNil(snapshot.budget?.row("categories", id: "pets"))
        XCTAssertNil(snapshot.budget?.row("transactions", id: "e2"))
        XCTAssertTrue(BudgetPatch.same(try XCTUnwrap(snapshot.budget), try XCTUnwrap(before.budget)))
        XCTAssertEqual(snapshot.summary, before.summary)
        XCTAssertTrue(snapshot.redoAvailable)
        snapshot = try await repository.redo()
        XCTAssertTrue(BudgetPatch.same(try XCTUnwrap(snapshot.budget), shown))
        XCTAssertFalse(snapshot.redoAvailable)
        let again = try await repository.load()
        XCTAssertTrue(BudgetPatch.same(try XCTUnwrap(snapshot.budget), try XCTUnwrap(again.budget)))
    }

    func testAnEditThatChangesNothingSavesNothing() async throws {
        var snapshot = try await sample("Nothing")
        snapshot = try await repository.command("saveTransaction", ["transaction": transaction("n1"), "through": .string(Dates.today)])
        let (revision, version) = (snapshot.revision, snapshot.version)
        snapshot = try await repository.edit { _ in }
        XCTAssertEqual(snapshot.revision, revision)
        XCTAssertEqual(snapshot.version, version)
        // The same record written again is no change either: no new revision, and nothing more to undo.
        let memo = try XCTUnwrap(snapshot.budget?.row("transactions", id: "n1")).text("memo")
        snapshot = try await repository.edit { try $0.upsertRecord("transactions", ["id": "n1", "memo": .string(memo)]) }
        XCTAssertEqual(snapshot.revision, revision)
        snapshot = try await repository.undo()
        XCTAssertNil(snapshot.budget?.row("transactions", id: "n1"), "The one undo is the transaction itself")
        XCTAssertFalse(snapshot.undoAvailable)
    }

    /// A failed command leaves the store and the app's copy as they were.
    func testARefusedEditChangesNothing() async throws {
        var snapshot = try await sample("Refused")
        let (revision, budget) = (snapshot.revision, snapshot.budget)
        do {
            _ = try await repository.command("saveTransaction", ["transaction": ["id": "bad", "date": "not a date", "accountId": "nowhere", "amount": 1]])
            XCTFail("An invalid transaction must not save")
        } catch {}
        do {
            // The first step is fine and the second isn't: neither is saved.
            _ = try await repository.edit({ try $0.upsertRecord("categories", ["id": "half", "name": "Half", "group": "Everyday", "target": 0]) },
                                          then: [("saveTransaction", ["transaction": ["id": "bad", "date": "not a date", "accountId": "nowhere", "amount": 1]])])
            XCTFail("One bad step must stop the whole edit")
        } catch {}
        snapshot = try await repository.load()
        XCTAssertEqual(snapshot.revision, revision)
        XCTAssertEqual(snapshot.budget, budget)
        XCTAssertNil(try EngineEdits.stored().row("categories", id: "half"))
    }

    /// What an import keeps of each transaction (YNAB's own copy) and of the whole export stays in
    /// the store through edits here, and comes back in a backup.
    func testImportMetadataSurvivesEditsOnTheLightCopy() async throws {
        var snapshot = try await sample("Imported")
        let exported = await repository.exportData()
        var whole = try JSON.parse(try XCTUnwrap(exported))
        var imported = transaction("y1", memo: "From YNAB")
        imported["source"] = ["id": "ynab-1", "import_id": "YNAB:-450:2026-10-01:1", "category_name": "Groceries"]
        imported["futureField"] = "kept"
        whole["transactions"] = .array(whole["transactions"].array + [imported, transaction("y2")])
        whole["sourceExport"] = ["budget": ["id": "ynab-budget", "payees": [["id": "p", "name": "Corner Cafe"]]]]
        snapshot = try await repository.importBudget(whole.data())

        // The app's copy is the light one.
        let light = try XCTUnwrap(snapshot.budget?.row("transactions", id: "y1"))
        XCTAssertFalse(light.has("source"))
        XCTAssertTrue(light.flag("hasImportSource"))
        XCTAssertEqual(light.text("futureField"), "kept")
        XCTAssertEqual(snapshot.budget?.row("transactions", id: "y2")?.has("hasImportSource"), false)
        XCTAssertEqual(snapshot.budget?.has("sourceExport"), false)

        // Edit that very record, another one, an extra, then a command, an undo and a redo.
        snapshot = try await repository.edit { try $0.upsertRecord("transactions", ["id": "y1", "memo": "Edited on the phone"]) }
        XCTAssertFalse(try XCTUnwrap(snapshot.budget?.row("transactions", id: "y1")).has("source"))
        snapshot = try await repository.edit { budget in
            try budget.upsertRecord("transactions", ["id": "y2", "memo": "Also edited"])
            budget["dateFormat"] = "mdy"
        }
        var cleared = try XCTUnwrap(snapshot.budget?.row("transactions", id: "y1"))
        cleared["status"] = "cleared"
        snapshot = try await repository.command("saveTransaction", ["transaction": cleared, "through": .string(Dates.today)], original: ("transactions", cleared.with("status", "uncleared")))
        snapshot = try await repository.undo()
        snapshot = try await repository.redo()
        XCTAssertEqual(snapshot.budget?.row("transactions", id: "y1")?.text("status"), "cleared")

        let backup = await repository.exportData()
        for (name, budget) in [("store", try EngineEdits.stored()), ("backup", try JSON.parse(try XCTUnwrap(backup)))] {
            let row = try XCTUnwrap(budget.row("transactions", id: "y1"), name)
            XCTAssertEqual(row["source"], imported["source"], name)
            XCTAssertEqual(row.text("memo"), "Edited on the phone", name)
            XCTAssertEqual(row.text("status"), "cleared", name)
            XCTAssertEqual(row.text("futureField"), "kept", name)
            XCTAssertFalse(row.has("hasImportSource"), "The hint is the app's alone (\(name))")
            XCTAssertEqual(budget["sourceExport"], whole["sourceExport"], name)
            XCTAssertEqual(budget.text("dateFormat"), "mdy", name)
        }

        // Another budget and back: opening never brings the heavy copy into the app.
        snapshot = try await repository.create(name: "Other", currency: "EUR", sample: false)
        let entries = try await repository.listBudgets()
        snapshot = try await repository.open(id: try XCTUnwrap(entries.first { $0.text("name") == "Imported" }).text("id"))
        XCTAssertEqual(snapshot.budget?.row("transactions", id: "y1")?.has("source"), false)
        snapshot = try await repository.edit { try $0.upsertRecord("transactions", ["id": "y1", "memo": "After reopening"]) }
        XCTAssertEqual(try EngineEdits.stored().row("transactions", id: "y1")?["source"], imported["source"])
        XCTAssertEqual(try EngineEdits.stored()["sourceExport"], whole["sourceExport"])
    }

    /// Sync commits first: the edit is built again on what's stored, and a second refusal shows it.
    func testAnEditAfterSyncIsMadeAgainOnTheNewVersion() async throws {
        var snapshot = try await sample("Synced")
        snapshot = try await repository.command("saveTransaction", ["transaction": transaction("s1"), "through": .string(Dates.today)])
        var synced = try XCTUnwrap(snapshot.budget?.row("transactions", id: "s1"))
        synced["payee"] = "Renamed by sync"
        try EngineEdits.elsewhere(["transactions": ["upsert": [synced], "delete": []]], at: snapshot)

        // The patch is rebuilt on the synced record, so sync's payee isn't overwritten by the copy from before it.
        snapshot = try await repository.edit { try $0.upsertRecord("transactions", ["id": "s1", "memo": "Mine"]) }
        let row = try XCTUnwrap(snapshot.budget?.row("transactions", id: "s1"))
        XCTAssertEqual(row.text("payee"), "Renamed by sync")
        XCTAssertEqual(row.text("memo"), "Mine")
        XCTAssertEqual(try EngineEdits.stored().row("transactions", id: "s1")?.text("payee"), "Renamed by sync")

        // Undo after a sync is refused, and what's stored is shown.
        try EngineEdits.elsewhere(["name": "Renamed elsewhere"], at: snapshot)
        do {
            _ = try await repository.undo()
            XCTFail("Undo must not run over a change it hasn't seen")
        } catch let conflict as BudgetRepository.Conflict {
            XCTAssertEqual(conflict.snapshot.budget?.text("name"), "Renamed elsewhere")
            XCTAssertNil(conflict.message)
        }

        // An editor's record that sync changed: refused with the editor's own message, on the fresh budget.
        snapshot = try await repository.load()
        let opened = try XCTUnwrap(snapshot.budget?.row("transactions", id: "s1"))
        var again = opened
        again["memo"] = "Changed by sync"
        try EngineEdits.elsewhere(["transactions": ["upsert": [again], "delete": []]], at: snapshot)
        do {
            _ = try await repository.command("saveTransaction", ["transaction": opened.with("memo", "Stale save"), "through": .string(Dates.today)], original: ("transactions", opened))
            XCTFail("A stale editor must not save")
        } catch let conflict as BudgetRepository.Conflict {
            XCTAssertEqual(conflict.snapshot.budget?.row("transactions", id: "s1")?.text("memo"), "Changed by sync")
            XCTAssertEqual(conflict.message?.contains("editor was open"), true)
        }
        XCTAssertEqual(try EngineEdits.stored().row("transactions", id: "s1")?.text("memo"), "Changed by sync")
    }

    /// Totals follow the month, and catch up when the store has moved past what's shown.
    func testTotalsAreOfTheShownRevision() async throws {
        var snapshot = try await sample("Totals")
        let groceries = category
        let next = Dates.addMonths(snapshot.month, 1)
        snapshot = try await repository.selectMonth(next)
        snapshot = try await repository.command("workflow", ["action": ["type": "assignAmount", "month": .string(next), "categoryId": .string(groceries), "amount": 12_300]])
        XCTAssertEqual(snapshot.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 12_300)

        // Sync changes an assignment; a month change then asks for totals at a revision that's gone.
        try EngineEdits.elsewhere(["assignments": ["upsert": [["month": .string(next), "categoryId": .string(groceries), "amount": 99_900]], "delete": []]], at: snapshot)
        snapshot = try await repository.selectMonth(next)
        // Same month: the cached totals are still the shown revision's.
        XCTAssertEqual(snapshot.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 12_300)
        snapshot = try await repository.selectMonth(Dates.addMonths(next, 1))
        snapshot = try await repository.selectMonth(next)
        XCTAssertEqual(snapshot.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 99_900, "Rows and totals moved together")
        XCTAssertEqual(snapshot.budget?.rows("assignments").first { $0.text("month") == next && $0.text("categoryId") == groceries }?.long("amount"), 99_900)
    }

    /// A dry run replies with totals and the transactions it would change, and saves nothing.
    func testADryRunSavesNothing() async throws {
        let snapshot = try await sample("Dry")
        let groceries = category
        let action: JSON = ["type": "assignAmount", "month": .string(snapshot.month), "categoryId": .string(groceries), "amount": 77_700]
        let preview = try NativeCore.service("query_budget", ["method": "workflow", "params": ["action": action, "previewMonth": .string(snapshot.month)],
                                                               "expectedToken": .string(snapshot.revision)])
        XCTAssertEqual(preview["summary"].rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 77_700)
        let account = try XCTUnwrap(snapshot.budget?.rows("accounts").first).text("id")
        let reconcile = try NativeCore.service("query_budget", ["method": "reconcile", "params": ["accountId": .string(account), "balance": 123_456, "date": .string(Dates.today), "adjustmentId": "adjust"],
                                                                 "expectedToken": .string(snapshot.revision)])
        XCTAssertNotNil(reconcile.row("transactions", id: "adjust"), "The reconcile sheet reads its adjustment from the reply")
        let after = try await repository.load()
        XCTAssertEqual(after.revision, snapshot.revision)
        XCTAssertEqual(after.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 60_000)
    }
}

/// The published store: what the screens call.
@MainActor
final class BudgetStoreEditTests: XCTestCase {
    private func settled(_ store: BudgetStore) async { await withCheckedContinuation { done in store.afterPending { done.resume() } } }

    func testChangesPreviewsAndReadsGoThroughTheStore() async throws {
        let repository = try await TestLibrary.repository()
        let store = BudgetStore(repository: repository)
        store.createBudget(name: "Published", currency: "USD", sample: true)
        await settled(store)
        let groceries = try XCTUnwrap(store.budget?.rows("categories").first { $0.text("name") == "Groceries" }).text("id")
        var changes = 0
        store.onChange = { changes += 1 }

        // A queued change publishes the patched budget and the totals that came with it.
        let version = store.version
        store.assign(category: groceries, amount: 45_600)
        await settled(store)
        XCTAssertEqual(store.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 45_600)
        XCTAssertEqual(store.budget?.rows("assignments").first { $0.text("month") == store.month && $0.text("categoryId") == groceries }?.long("amount"), 45_600)
        XCTAssertGreaterThan(store.version, version)
        XCTAssertEqual(changes, 1, "One saved revision, one nudge to sync")
        XCTAssertTrue(store.undoAvailable)

        // An awaited one, as sheets make them.
        try await store.saveTransaction(["date": .string(Dates.today), "accountId": "sample-checking", "payee": "Bakery", "amount": -1_250,
                                         "splits": [["categoryId": .string(groceries), "amount": -1_250]]])
        let bakery = try XCTUnwrap(store.budget?.rows("transactions").first { $0.text("payee") == "Bakery" })
        XCTAssertEqual(changes, 2)

        // A preview is a dry run on the store's copy: totals come back, nothing is saved.
        let action: JSON = ["type": "assignAmount", "month": .string(store.month), "categoryId": .string(groceries), "amount": 99_900]
        let revision = store.revision
        let preview = try await store.query("workflow", ["action": action, "previewMonth": .string(store.month)])
        XCTAssertEqual(preview["summary"].rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 99_900)
        XCTAssertEqual(store.revision, revision)
        XCTAssertEqual(store.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 45_600)

        // Reads: a report and an export, of the budget that's shown.
        let (from, to) = (Dates.addMonths(store.month, -1), store.month)
        let report = try await store.query("reports", ["from": .string(from), "to": .string(to), "accountIds": [], "categoryIds": []])
        XCTAssertTrue(report.isObject)
        let csv = try await store.query("exportRegister").string
        XCTAssertTrue(csv.contains("Bakery"))
        // The engine answers these from their parameters alone, budget or no budget.
        let terms: JSON = ["accountId": "loan", "rate": .double(5.0), "payment": 50_000, "paymentDay": 1]
        _ = try? await store.query("loanProjection", ["balance": 100_000, "terms": terms, "start": "2026-01-31"])

        // Sync commits behind the app's back: a read catches up first, so its answer and the
        // shown budget are of one revision.
        var synced = bakery
        synced["payee"] = "Bakery (renamed by sync)"
        _ = try NativeCore.service("edit_budget", ["steps": [["patch": ["transactions": ["upsert": [synced], "delete": []]]]], "expectedToken": .string(store.revision),
                                                   "month": .string(store.month), "asOfMonth": .string(Dates.currentMonth)])
        let stale = store.revision
        let after = try await store.query("exportRegister").string
        XCTAssertTrue(after.contains("renamed by sync"))
        XCTAssertNotEqual(store.revision, stale)
        XCTAssertEqual(store.budget?.row("transactions", id: bakery.text("id"))?.text("payee"), "Bakery (renamed by sync)")

        // Undo and redo publish patches too.
        store.assign(category: groceries, amount: 100)
        await settled(store)
        store.undo()
        await settled(store)
        XCTAssertEqual(store.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 45_600)
        XCTAssertTrue(store.redoAvailable)
        store.redo()
        await settled(store)
        XCTAssertEqual(store.summary?.rows("categories").first { $0.text("id") == groceries }?.long("assigned"), 100)
        XCTAssertNil(store.error)

        // A backup is the whole budget from the store.
        let exported = await store.exportData()
        let backup = try JSON.parse(try XCTUnwrap(exported))
        XCTAssertEqual(backup.rows("transactions").count, store.budget?.rows("transactions").count)
        XCTAssertEqual(backup.text("name"), "Published")
    }
}

private extension JSON {
    func with(_ key: String, _ value: JSON) -> JSON {
        var copy = self
        copy[key] = value
        return copy
    }
}
