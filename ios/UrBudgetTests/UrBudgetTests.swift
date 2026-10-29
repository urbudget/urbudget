import XCTest
@testable import UrBudget

final class JSONTests: XCTestCase {
    func testRoundTripKeepsIntegersBooleansAndUnknownFields() throws {
        let parsed = try JSON.parse(#"{"a":9007199254740993,"b":true,"c":1.5,"d":null,"e":[1,"x"],"f":{"g":"h"}}"#)
        XCTAssertEqual(parsed["a"], .int(9_007_199_254_740_993))
        XCTAssertEqual(parsed["b"], .bool(true))
        XCTAssertEqual(parsed["c"], .double(1.5))
        XCTAssertTrue(parsed["d"].isNull && parsed.has("d"))
        XCTAssertEqual(try JSON.parse(parsed.serialized()), parsed)
    }

    func testUpsertMergesFieldsIntoTheRecord() throws {
        var budget: JSON = ["accounts": [["id": "a", "name": "Old", "futureField": "kept"]]]
        try budget.upsertRecord("accounts", ["id": "a", "name": "New"])
        XCTAssertEqual(budget.rows("accounts").first?.text("name"), "New")
        XCTAssertEqual(budget.rows("accounts").first?.text("futureField"), "kept")
        XCTAssertThrowsError(try budget.upsertRecord("unknown", ["id": "x"]))
    }
}

final class MoneyTests: XCTestCase {
    func testFormatsAndParsesInTheBudgetsFormat() throws {
        Money.configure(budget: ["currency": "USD"])
        XCTAssertEqual(money(123_456, "USD"), "$1,234.56")
        XCTAssertEqual(money(-5, "USD"), "-$0.05")
        Money.configure(budget: ["currency": "SAR"])
        XCTAssertEqual(money(123_456, "SAR"), "1,234.56 SAR")
        XCTAssertEqual(try Money.parse("1,234.5"), 123_450)
        XCTAssertEqual(try Money.parse("-12"), -1200)
        XCTAssertThrowsError(try Money.parse("12.345"))
        XCTAssertEqual(Money.editable(-1234), "-12.34")
        XCTAssertEqual(Money.editable(5), "0.05")
    }

    func testDecimalCommaBudgets() throws {
        Money.configure(budget: ["currency": "EUR", "numberFormat": ["decimal": ",", "group": ".", "symbol": "after"]])
        XCTAssertEqual(money(123_456, "EUR"), "1.234,56 €")
        XCTAssertEqual(try Money.parse("1.234,56"), 123_456)
        Money.configure(budget: ["currency": "SAR"])
    }

    func testDoubleZeroTypesTwoZeros() {
        XCTAssertEqual(AmountPad.press(AmountPad.press("", "5"), "00"), "5.00")
        XCTAssertEqual(AmountPad.press("", "00"), "0.00")
        XCTAssertEqual(AmountPad.press("1.00 +", "00"), "1.00 + 0.00")
        let pad = PadState(1_250)
        pad.press("00")
        XCTAssertEqual(pad.cents, 0, "A seeded amount is replaced by the first digits, as by a single zero")
    }

    func testKeypadWorksSumsInCents() {
        var text = ""
        for key in ["1", "2", "3", "4", "+", "5", "0", "0", "×", "2"] { text = AmountPad.press(text, key) }
        XCTAssertEqual(text, "12.34 + 5.00 × 0.02")
        XCTAssertEqual(AmountPad.value(text), 1244)
        XCTAssertEqual(AmountPad.press("12.34", "⌫"), "1.23")
        XCTAssertEqual(AmountPad.value("10.00 ÷ 3.00"), 333)
        XCTAssertNil(AmountPad.value("10.00 ÷ 0.00"))
        XCTAssertNil(AmountPad.value("10.00 +"))
    }

    func testDates() {
        XCTAssertEqual(Dates.addMonths("2026-01", -1), "2025-12")
        XCTAssertEqual(Dates.monthBounds("2026-02").1, "2026-02-28")
        XCTAssertEqual(Dates.dayLabel("2026-10-21", monthFirst: true), "Wednesday, October 21, 2026")
        XCTAssertEqual(Dates.dayLabel("2026-10-21", monthFirst: false), "Wednesday, 21 October 2026")
    }
}

final class CategoryStatusTests: XCTestCase {
    func testStatusesFollowTheDesktopRules() {
        let category: JSON = ["id": "food", "target": 10_000]
        XCTAssertEqual(CategoryStatus(category: category, summary: ["available": -500, "assigned": 0], month: "2026-10").status, "negative")
        let short = CategoryStatus(category: category, summary: ["available": 2000, "assigned": 2000, "needed": 8000], month: "2026-10")
        XCTAssertEqual(short.status, "underfunded")
        XCTAssertEqual(short.targetWords(category: category, summary: ["available": 2000, "assigned": 2000], monthFirst: false)?.words, "more needed this month")
        XCTAssertEqual(CategoryStatus(category: [:], summary: ["available": 100], month: "2026-10").status, "positive")
        XCTAssertEqual(CategoryStatus(category: [:], summary: ["available": -100, "creditOverspending": 100], month: "2026-10").status, "credit")
    }
}

/// The Rust services are one per process, so every storage check shares one temporary library.
final class RepositoryTests: XCTestCase {
    func testBudgetsAreCalculatedSavedUndoneAndSearchedThroughTheSharedEngine() async throws {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("urbudget-tests-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: folder) }
        let repository = BudgetRepository(folder: folder)
        var snapshot = try await repository.start()
        XCTAssertNil(snapshot.budget)

        snapshot = try await repository.create(name: "Tests", currency: "USD", sample: true)
        let budget = try XCTUnwrap(snapshot.budget)
        let summary = try XCTUnwrap(snapshot.summary)
        let model = buildBudgetModel(budget: budget, summary: summary, month: snapshot.month)
        XCTAssertEqual(model.groups.map(\.name), ["Everyday", "Savings"])
        let groceries = try XCTUnwrap(model.groups[0].rows.first { $0.name == "Groceries" })

        snapshot = try await repository.command("workflow", ["action": ["type": "assignAmount", "month": .string(snapshot.month),
                                                                        "categoryId": .string(groceries.id), "amount": 70_000]])
        XCTAssertEqual(snapshot.summary?.rows("categories").first { $0.text("id") == groceries.id }?.long("assigned"), 70_000)
        snapshot = try await repository.undo()
        XCTAssertEqual(snapshot.summary?.rows("categories").first { $0.text("id") == groceries.id }?.long("assigned"), 60_000)

        let transaction: JSON = ["id": "tx", "date": .string(Dates.today), "accountId": "sample-checking", "payee": "Corner Cafe", "memo": "",
                                 "amount": -450, "status": "uncleared", "transferAccountId": .null, "transferStatus": .null,
                                 "splits": [["categoryId": .string(groceries.id), "amount": -450]]]
        snapshot = try await repository.command("saveTransaction", ["transaction": transaction, "through": .string(Dates.today)])
        XCTAssertEqual(RegisterIndex(budget: try XCTUnwrap(snapshot.budget)).matching(.init(search: "corner")).map(\.id), ["tx"])

        // An editor can't save over a record that changed after it opened.
        let opened = try XCTUnwrap(snapshot.budget?.row("transactions", id: "tx"))
        _ = try await repository.edit { try $0.upsertRecord("transactions", ["id": "tx", "memo": "changed elsewhere"]) }
        do {
            _ = try await repository.command("saveTransaction", ["transaction": transaction, "through": .string(Dates.today)], original: ("transactions", opened))
            XCTFail("A stale editor must not save")
        } catch {}

        // A record deleted after the form opened must not be resurrected by its stale Save.
        _ = try await repository.edit { $0["transactions"] = .array($0.rows("transactions").filter { $0.text("id") != "tx" }) }
        do {
            _ = try await repository.command("saveTransaction", ["transaction": transaction, "through": .string(Dates.today)], original: ("transactions", opened))
            XCTFail("A deleted transaction must stay deleted")
        } catch {}

        snapshot = try await repository.create(name: "Second", currency: "EUR", sample: false)
        let budgets = try await repository.listBudgets()
        XCTAssertEqual(budgets.count, 2)
        snapshot = try await repository.open(id: try XCTUnwrap(budgets.first { $0.text("name") == "Tests" }).text("id"))
        XCTAssertEqual(snapshot.budget?.text("name"), "Tests")

        // A budget from the welcome screen starts in the chosen formats with the starter categories.
        // (The native runtime is one per process, so these share this test's library.)
        snapshot = try await repository.create(name: "Fresh", currency: "EUR", sample: false) { budget in
            budget["dateFormat"] = "dmy"
            StarterBudget.addCategories(to: &budget)
        }
        XCTAssertEqual(snapshot.budget?.text("dateFormat"), "dmy")
        XCTAssertEqual(snapshot.budget?.rows("categories").count, 16)

        // A backup opened from Files is added beside the open budget, not over it.
        let exported = await repository.exportData()
        snapshot = try await repository.importAsNew(try XCTUnwrap(exported))
        XCTAssertEqual(snapshot.budget?.text("name"), "Fresh")
        XCTAssertEqual(snapshot.budget?.rows("categories").count, 16)
        let library = try await repository.listBudgets()
        XCTAssertEqual(library.count, 4)
        do {
            _ = try await repository.importAsNew(Data(#"{"name":"not a backup"}"#.utf8))
            XCTFail("Only portable UrBudget backups open")
        } catch {}

        // Sync saves a downloaded change a moment before an edit: the edit is made again on that
        // version instead of failing with "Your budget changed while saving".
        var downloaded = try XCTUnwrap(snapshot.budget)
        downloaded["name"] = "Renamed on the desktop"
        _ = try NativeCore.service("save_budget", ["budget": downloaded, "expectedToken": .string(snapshot.revision), "ackOnly": true])
        snapshot = try await repository.edit { try $0.upsertRecord("categories", ["id": "after-sync", "name": "Added on the phone", "group": "Everyday", "target": 0]) }
        XCTAssertEqual(snapshot.budget?.text("name"), "Renamed on the desktop")
        XCTAssertNotNil(snapshot.budget?.row("categories", id: "after-sync"))
        // An editor that showed a record sync has since changed still refuses.
        let shown = try XCTUnwrap(snapshot.budget?.row("categories", id: "after-sync"))
        var renamed = try XCTUnwrap(snapshot.budget)
        try renamed.upsertRecord("categories", ["id": "after-sync", "name": "Renamed by sync"])
        _ = try NativeCore.service("save_budget", ["budget": renamed, "expectedToken": .string(snapshot.revision), "ackOnly": true])
        do {
            _ = try await repository.command("workflow", ["action": ["type": "assignAmount", "month": .string(snapshot.month), "categoryId": "after-sync", "amount": 100]],
                                             original: ("categories", shown))
            XCTFail("An editor's record changed by sync must not be saved over")
        } catch {}

        // Everything works signed out: the status has the account and sync, and nothing to unlock.
        let status = try NativeCore.service("cloud_status")
        XCTAssertTrue(status.text("email").isEmpty)
        XCTAssertFalse(status["sync"]["unlocked"].bool)
        XCTAssertFalse(status.has("license"))
    }
}

final class SyncInvalidationTests: XCTestCase {
    func testLockedV2StillReconcilesButLegacyRequiresKey() {
        var status: JSON = ["email": "qa@example.test", "sync": ["protocol": 2, "unlocked": false]]
        XCTAssertTrue(syncCanReconcile(status))
        status["sync"]["protocol"] = 1
        XCTAssertFalse(syncCanReconcile(status))
        status["sync"]["unlocked"] = true
        XCTAssertTrue(syncCanReconcile(status))
        status["email"] = .null
        XCTAssertFalse(syncCanReconcile(status))
    }
    private let library: JSON = ["activeId": "current", "entries": [["id": "former", "path": "/former"], ["id": "current", "path": "/current"]]]
    func testTargetedChangesRespectTheIdentityAtCompletion() {
        let report: JSON = ["openChanged": true, "changedBudgets": [["id": "former", "revision": "next"]]]
        XCTAssertFalse(syncInvalidatesActive(report, library: library, identity: "/current", revision: "old"))
    }
    func testRevisionAndDeletionInvalidations() {
        let report: JSON = ["openChanged": false, "changedBudgets": [["id": "current", "revision": "next"]]]
        XCTAssertTrue(syncInvalidatesActive(report, library: library, identity: "/current", revision: "old"))
        XCTAssertFalse(syncInvalidatesActive(report, library: library, identity: "/current", revision: "next"))
        var empty = library
        empty["activeId"] = .null
        XCTAssertTrue(syncInvalidatesActive(["openChanged": false, "changedBudgets": []], library: empty, identity: "/current", revision: "next"))
    }
    func testLegacyFlagRequiresAbsentTargetedChanges() {
        XCTAssertTrue(syncInvalidatesActive(["openChanged": true], library: library, identity: "/current", revision: "old"))
        XCTAssertFalse(syncInvalidatesActive(["openChanged": true, "changedBudgets": []], library: library, identity: "/current", revision: "old"))
    }
    func testPersistentSyncRequiresAnEnabledV2Account() {
        var status: JSON = ["email": "qa@example.test", "sync": ["protocol": 2, "enabled": true, "unlocked": false]]
        XCTAssertTrue(syncBackgroundEligible(status))
        status["sync"]["enabled"] = false
        XCTAssertFalse(syncBackgroundEligible(status))
        status["sync"]["enabled"] = true
        status["sync"]["protocol"] = 1
        status["sync"]["unlocked"] = true
        XCTAssertFalse(syncBackgroundEligible(status))
        status["sync"]["protocol"] = 2
        status["email"] = .null
        XCTAssertFalse(syncBackgroundEligible(status))
    }
}
