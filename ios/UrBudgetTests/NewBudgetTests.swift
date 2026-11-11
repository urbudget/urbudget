import XCTest
@testable import UrBudget

final class NewBudgetTests: XCTestCase {
    func testLocalFormatsFollowTheDevicesRegion() {
        let us = StarterBudget.localFormats(currency: "USD", locale: Locale(identifier: "en_US"))
        XCTAssertEqual([us.number.decimal, us.number.group, us.number.symbol, us.date], [".", ",", "before", "mdy"])
        let germany = StarterBudget.localFormats(currency: "EUR", locale: Locale(identifier: "de_DE"))
        XCTAssertEqual([germany.number.decimal, germany.number.group, germany.date], [",", ".", "dmy"])
        // A no-break space groups as a plain space; Swedish dates are written year first.
        let sweden = StarterBudget.localFormats(currency: "SEK", locale: Locale(identifier: "sv_SE"))
        XCTAssertEqual([sweden.number.decimal, sweden.number.group, sweden.date], [",", " ", "iso"])
        XCTAssertEqual(StarterBudget.localFormats(currency: "SAR", locale: Locale(identifier: "en_US")).number.symbol, "after")
    }

    func testStarterCategoriesMatchTheDesktopsAndKeepTheirGroupOrder() {
        var budget: JSON = ["categories": [["id": "existing", "name": "Kept", "group": "Mine"]]]
        StarterBudget.addCategories(to: &budget)
        let categories = budget.rows("categories")
        XCTAssertEqual(categories.count, 17)
        XCTAssertEqual(categories.first?.text("name"), "Kept")
        XCTAssertEqual(budget["groupOrder"].array.map(\.string), ["Bills", "Needs", "Wants", "Savings"])
        XCTAssertEqual(Set(categories.map { $0.text("id") }).count, 17)
        XCTAssertTrue(categories.dropFirst().allSatisfy { $0.long("target") == 0 })
    }

    func testSamplesAreKnownByTheirMarkOrTheEngineSamplesRecords() {
        XCTAssertTrue(StarterBudget.isSample(["sample": [:]]))
        XCTAssertTrue(StarterBudget.isSample(["sample": ["from": "b1"]]))
        // One made before samples were marked.
        XCTAssertTrue(StarterBudget.isSample(["accounts": [["id": "sample-checking"]], "transactions": [["id": "sample-market"]]]))
        // A fresh start from it keeps the account but not the cleared purchase.
        XCTAssertFalse(StarterBudget.isSample(["accounts": [["id": "sample-checking"]], "transactions": []]))
        XCTAssertFalse(StarterBudget.isSample(["name": "My budget", "accounts": [["id": "checking"]]]))
    }

    func testLeavingASampleGoesToTheBudgetItCameFromElseTheLastOpened() {
        let library: [JSON] = [
            ["id": "sample", "current": true],
            ["id": "missing", "missing": true],
            // The library lists these too; neither is a budget to go back to.
            ["id": "binned", "lifecycle": "trashed"],
            ["id": "kept", "lifecycle": "recovery"],
            ["id": "recent", "lifecycle": "active"],
            ["id": "home"],
        ]
        XCTAssertEqual(StarterBudget.sampleExit(from: ["sample": ["from": "home"]], library: library)?.text("id"), "home")
        XCTAssertEqual(StarterBudget.sampleExit(from: ["sample": [:]], library: library)?.text("id"), "recent")
        XCTAssertEqual(StarterBudget.sampleExit(from: ["sample": ["from": "gone"]], library: library)?.text("id"), "recent")
        XCTAssertNil(StarterBudget.sampleExit(from: ["sample": [:]], library: [["id": "sample", "current": true]]))
    }

    func testFilesFromOtherAppsOpenByWhatTheyHold() {
        func kind(_ name: String) -> OpenedFile.Kind? { OpenedFile(URL(fileURLWithPath: "/tmp/\(name)"))?.kind }
        XCTAssertEqual(kind("backup.json"), .backup)
        XCTAssertEqual(kind("YNAB Export.ZIP"), .ynab)
        XCTAssertEqual(kind("statement.csv"), .statement)
        XCTAssertEqual(kind("bank.OFX"), .statement)
        XCTAssertEqual(kind("bank.qif"), .statement)
        XCTAssertNil(kind("receipt.pdf"))
        XCTAssertNil(OpenedFile(URL(string: "https://urbudget.app/backup.json")!))
    }
}

/// The bank-name rules, checked against desktop/src/bankNames.test.ts.
final class BankNamesTests: XCTestCase {
    private func rules(_ list: [(String, String)]) -> [JSON] {
        list.enumerated().map { index, rule in ["id": .string(String(UnicodeScalar(97 + index)!)), "match": .string(rule.0), "payee": .string(rule.1), "categoryId": .null] }
    }

    func testSuggestionsDropProcessorPrefixesAndStoreNumbers() {
        XCTAssertEqual(BankNames.suggestMatch("SQ *BLUE BOTTLE COFFEE OAKLAND CA"), "blue bottle coffee")
        XCTAssertEqual(BankNames.suggestMatch("PURCHASE AUTHORIZED ON 09/14 STARBUCKS STORE 12345 SEATTLE WA"), "starbucks store")
        XCTAssertEqual(BankNames.suggestMatch("AMZN Mktp US*2K4AB1"), "amzn mktp us")
        XCTAssertEqual(BankNames.suggestMatch("7-ELEVEN 33452"), "7 eleven")
        XCTAssertEqual(BankNames.suggestMatch("TST* DIG INN 0042"), "dig inn")
        XCTAssertEqual(BankNames.suggestMatch("PAYPAL *NETFLIX"), "netflix")
        XCTAssertEqual(BankNames.suggestMatch("PAYPAL TRANSFER 123456", payee: "PayPal"), "paypal transfer")
        XCTAssertEqual(BankNames.suggestMatch("12345678"), "12345678")
    }

    func testARuleIsOfferedOnlyWhenItWouldWin() {
        let existing = rules([("amzn mktp us", "Amazon"), ("sq", "Square")])
        XCTAssertNil(BankNames.ruleFor(existing, raw: "AMZN Mktp US*2K4AB", payee: "Amazon"))
        // The suggestion is taken by another payee's rule, so the whole bank text is used.
        XCTAssertEqual(BankNames.ruleFor(existing, raw: "AMZN Mktp US*2K4AB", payee: "Audible")?.text("match"), "amzn mktp us 2k4ab")
        let blue = BankNames.ruleFor(existing, raw: "SQ *BLUE BOTTLE 0042", payee: " Blue Bottle ")
        XCTAssertEqual(blue?.text("match"), "blue bottle")
        XCTAssertEqual(blue?.text("payee"), "Blue Bottle")
        XCTAssertEqual(blue?.text("id"), "")
        XCTAssertNil(BankNames.ruleFor(existing, raw: "  ", payee: "Blue Bottle"))
    }

    func testLearningNeedsAnImportedTransactionGivenANewName() {
        let imported: JSON = ["id": "1", "payee": "SQ *BLUE BOTTLE 0042", "importedPayee": "SQ *BLUE BOTTLE 0042"]
        XCTAssertEqual(BankNames.learn(original: imported, payee: "Blue Bottle", transfer: false, rules: [])?.text("match"), "blue bottle")
        XCTAssertNil(BankNames.learn(original: imported, payee: "SQ *BLUE BOTTLE 0042", transfer: false, rules: []))
        XCTAssertNil(BankNames.learn(original: ["id": "2", "payee": "Cafe"], payee: "Blue Bottle", transfer: false, rules: []))
        XCTAssertNil(BankNames.learn(original: nil, payee: "Blue Bottle", transfer: false, rules: []))
        XCTAssertNil(BankNames.learn(original: imported, payee: "Blue Bottle", transfer: true, rules: []))
    }
}
