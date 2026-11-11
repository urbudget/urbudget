import XCTest
@testable import UrBudget

final class LibraryTests: XCTestCase {
    func testNumberFormatResolvesAsMoneyReadsIt() {
        XCTAssertEqual(LibraryNumberFormat(budget: ["currency": "USD"]), LibraryNumberFormat(decimal: ".", group: ",", symbol: "before"))
        // A currency whose English symbol is its code goes after the amount.
        XCTAssertEqual(LibraryNumberFormat(budget: ["currency": "CHF"]), LibraryNumberFormat(decimal: ".", group: ",", symbol: "after"))
        let saved: JSON = ["currency": "EUR", "numberFormat": ["decimal": ",", "group": "", "symbol": "none"]]
        XCTAssertEqual(LibraryNumberFormat(budget: saved), LibraryNumberFormat(decimal: ",", group: "", symbol: "none"))
        let invalid: JSON = ["currency": "USD", "numberFormat": ["decimal": "x", "group": "-", "symbol": "left"]]
        XCTAssertEqual(LibraryNumberFormat(budget: invalid), LibraryNumberFormat(decimal: ".", group: ",", symbol: "before"))
    }

    func testExampleMatchesTheAppsMoneyFormat() {
        let budgets: [JSON] = [
            ["currency": "USD"],
            ["currency": "SAR"],
            ["currency": "EUR", "numberFormat": ["decimal": ",", "group": ".", "symbol": "after"]],
            ["currency": "CHF", "numberFormat": ["decimal": ".", "group": "'", "symbol": "before"]],
            ["currency": "SAR", "numberFormat": ["decimal": ",", "group": " ", "symbol": "none"]],
        ]
        for budget in budgets {
            Money.configure(budget: budget)
            let format = LibraryNumberFormat(budget: budget)
            let currency = budget.text("currency")
            for amount: Int64 in [0, 5, -5, 99, 123_456, -123_456_789, 100_000_000_000] {
                XCTAssertEqual(format.format(amount, currency: currency), money(amount, currency), "\(budget.serialized()) \(amount)")
            }
        }
        Money.configure(budget: ["currency": "SAR"])
    }

    func testDecimalMarkMovesAClashingThousandsSeparator() {
        let format = LibraryNumberFormat(decimal: ".", group: ",", symbol: "after")
        XCTAssertEqual(format.with(decimal: ","), LibraryNumberFormat(decimal: ",", group: ".", symbol: "after"))
        XCTAssertEqual(LibraryNumberFormat(decimal: ".", group: " ").with(decimal: ",").group, " ")
        XCTAssertFalse(format.with(decimal: ",").groups.contains { $0.id == "," })
    }

    func testWritingAFormatKeepsUnknownFields() {
        var budget: JSON = ["currency": "USD", "numberFormat": ["decimal": ".", "futureField": "kept"]]
        LibraryNumberFormat(decimal: ",", group: ".", symbol: "after").write(into: &budget)
        XCTAssertEqual(budget["numberFormat"], ["decimal": ",", "group": ".", "symbol": "after", "futureField": "kept"])
    }

    func testCurrencyChangeMovesASymbolLeftWhereTheOldCurrencyPutsIt() {
        var moved: JSON = ["currency": "USD", "numberFormat": ["decimal": ".", "group": ",", "symbol": "before"]]
        LibrarySettings.change(currency: "CHF", in: &moved)
        XCTAssertEqual(moved.text("currency"), "CHF")
        XCTAssertEqual(moved["numberFormat"].text("symbol"), "after")

        var chosen: JSON = ["currency": "USD", "numberFormat": ["symbol": "none"]]
        LibrarySettings.change(currency: "SAR", in: &chosen)
        XCTAssertEqual(chosen["numberFormat"].text("symbol"), "none")

        var unsaved: JSON = ["currency": "USD"]
        LibrarySettings.change(currency: "EUR", in: &unsaved)
        XCTAssertFalse(unsaved.has("numberFormat"))
    }

    func testFlagNamesAreTrimmedAndEmptyOnesRemoved() {
        var budget: JSON = ["flagNames": ["red": "Old", "teal": "Desktop colour"]]
        LibrarySettings.set(flagNames: ["red": "  Reimburse ", "blue": String(repeating: "x", count: 60), "green": "   "], in: &budget)
        XCTAssertEqual(budget["flagNames"].text("red"), "Reimburse")
        XCTAssertEqual(budget["flagNames"].text("blue").count, 40)
        XCTAssertFalse(budget["flagNames"].has("green"))
        XCTAssertEqual(budget["flagNames"].text("teal"), "Desktop colour")

        var cleared: JSON = ["flagNames": ["red": "Tax"]]
        LibrarySettings.set(flagNames: [:], in: &cleared)
        XCTAssertFalse(cleared.has("flagNames"))
    }

    func testBudgetNames() {
        XCTAssertTrue(LibrarySettings.validName("Home"))
        XCTAssertFalse(LibrarySettings.validName("   "))
        XCTAssertFalse(LibrarySettings.validName(String(repeating: "é", count: 101)))
    }

    func testDateExamples() {
        XCTAssertEqual(LibrarySettings.dateExample("iso", day: "2026-10-05"), "2026-10-05")
        XCTAssertEqual(LibrarySettings.dateExample("dmy", day: "2026-10-05"), "05/10/2026")
        XCTAssertEqual(LibrarySettings.dateExample("mdy", day: "2026-10-05"), "10/05/2026")
        XCTAssertEqual(LibrarySettings.dateExample("dmmmy", day: "2026-10-05"), "5 Oct 2026")
    }

    func testLastOpenedTimes() {
        let utc = Date(timeIntervalSince1970: 1_792_593_005) // 2026-10-21T14:30:05Z
        XCTAssertEqual(LibraryDates.instant("2026-10-21T14:30:05.123456789+00:00"), utc)
        XCTAssertEqual(LibraryDates.instant("2026-10-21T14:30:05Z"), utc)
        XCTAssertEqual(LibraryDates.instant("2026-10-21T17:30:05+03:00"), utc)
        XCTAssertEqual(LibraryDates.instant("2026-10-21T09:30:05.5-05:00"), utc)
        XCTAssertNil(LibraryDates.instant(""))
        XCTAssertNil(LibraryDates.instant("2026-10-21T14:30:05+0300"))
        XCTAssertNil(LibraryDates.instant("yesterday"))
    }

    func testBackupNames() {
        let backup = LibraryBackup(["name": "budget-20261021-143005-123456789.sqlite3", "path": "/x/backups/budget-20261021-143005-123456789.sqlite3", "bytes": 2048])
        XCTAssertEqual(backup.date, Date(timeIntervalSince1970: 1_792_593_005))
        XCTAssertFalse(backup.portable)
        XCTAssertTrue(backup.detail.hasPrefix("Safety copy · "))
        XCTAssertTrue(LibraryBackup(["name": "budget-20261021-143005-000000001.json"]).portable)
        XCTAssertNil(LibraryDates.backupDate("budget-20261321-143005.json"))
        XCTAssertNil(LibraryDates.backupDate("notes.json"))
        XCTAssertEqual(LibraryBackup(["name": "budget-old.json"]).title, "budget-old.json")
    }

    func testBackupsToRemove() {
        XCTAssertEqual(LibraryBackup.excess(12, keep: 0), 0)
        XCTAssertEqual(LibraryBackup.excess(12, keep: 10), 2)
        XCTAssertEqual(LibraryBackup.excess(3, keep: 5), 0)
    }

    func testCurrencyList() {
        XCTAssertTrue(LibraryCurrencies.all.contains { $0.code == "SAR" })
        XCTAssertTrue(LibraryCurrencies.all.allSatisfy { LibraryCurrencies.valid($0.code) })
        XCTAssertEqual(LibraryCurrencies.entry("USD").symbol, "$")
        XCTAssertEqual(LibraryCurrencies.entry("CHF").symbol, "")
        XCTAssertFalse(LibraryCurrencies.valid("usd"))
        XCTAssertFalse(LibraryCurrencies.valid("US"))
    }
}

final class LibraryTrashTests: XCTestCase {
    func testTrashIsListedApartAndOnlyThisDevicesCanBeDeletedForGood() {
        let open: JSON = ["id": "a", "lifecycle": "active", "scope": "local"]
        let binned: JSON = ["id": "b", "lifecycle": "trashed", "scope": "local"]
        let synced: JSON = ["id": "c", "lifecycle": "trashed", "scope": "account"]
        let purged: JSON = ["id": "d", "lifecycle": "purged", "scope": "account"]
        let kept: JSON = ["id": "e", "lifecycle": "recovery"]
        XCTAssertEqual([open, binned, synced, purged, kept].filter(LibraryTrash.holds).map { $0.text("id") }, ["b", "c", "d"])
        // An account's budget stays until its deletion has synced; one being purged is already going.
        XCTAssertEqual([open, binned, synced, purged, kept].filter(LibraryTrash.onDeviceOnly).map { $0.text("id") }, ["b"])
    }

    /// Through the real library: a budget in Trash is deleted for good, and Trash is emptied.
    func testABudgetInTrashIsDeletedForGoodAndTrashEmptied() async throws {
        let repository = try await TestLibrary.repository()
        func entries() async throws -> [JSON] { try await repository.listBudgets() }
        func id(_ name: String) async throws -> String {
            let listed = try await entries()
            return try XCTUnwrap(listed.first { $0.text("name") == name }).text("id")
        }
        for name in ["Trash one", "Trash two", "Trash three", "Stays open"] {
            _ = try await repository.create(name: name, currency: "USD", sample: false, localOnly: true)
        }
        // The open budget can't be purged, and neither can one that isn't in Trash.
        let first = try await id("Trash one")
        XCTAssertThrowsError(try NativeCore.service("library_purge", ["id": .string(first)]))
        for name in ["Trash one", "Trash two", "Trash three"] { _ = try await repository.deleteBudget(id: try await id(name)) }
        var listed = try await entries()
        XCTAssertEqual(Set(listed.filter(LibraryTrash.onDeviceOnly).map { $0.text("name") }).intersection(["Trash one", "Trash two", "Trash three"]).count, 3)

        _ = try NativeCore.service("library_purge", ["id": .string(first)])
        listed = try await entries()
        XCTAssertNil(listed.first { $0.text("name") == "Trash one" })
        XCTAssertNotNil(listed.first { $0.text("name") == "Trash two" })

        let emptied = try NativeCore.service("library_empty_trash")
        XCTAssertGreaterThanOrEqual(emptied.long("deleted"), 2)
        XCTAssertTrue(emptied.rows("failed").isEmpty)
        listed = try await entries()
        XCTAssertFalse(listed.contains(where: LibraryTrash.onDeviceOnly))
        XCTAssertNotNil(listed.first { $0.text("name") == "Stays open" && $0.flag("current") })
    }
}
