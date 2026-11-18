import XCTest
@testable import UrBudget

/// The bank review's defaults and choices, as Android's BankReviewDefaultsTest checks them.
final class BankReviewTests: XCTestCase {
    override func setUp() {
        super.setUp()
        Money.configure(budget: ["currency": "SAR"])
    }

    func testDefaultsFollowTheDesktop() {
        XCTAssertEqual(BankReview.defaultChoice(duplicate: true, possibleDuplicate: true, candidates: ["a"], previouslyMatched: []), "skip")
        XCTAssertEqual(BankReview.defaultChoice(duplicate: false, possibleDuplicate: true, candidates: [], previouslyMatched: []), "review")
        XCTAssertEqual(BankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: [], previouslyMatched: []), "new")
        XCTAssertEqual(BankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: ["a"], previouslyMatched: []), "match:a")
        // A side already imported from another source matches only when someone chooses it.
        XCTAssertEqual(BankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: ["a"], previouslyMatched: ["a"]), "review")
        XCTAssertEqual(BankReview.defaultChoice(duplicate: false, possibleDuplicate: false, candidates: ["a", "b"], previouslyMatched: []), "review")
    }

    func testAnExplicitTransferShowsItsAccountAndKeepsItsDestination() throws {
        let budget = try JSON.parse(#"{"currency":"SAR","accounts":[{"id":"savings","name":"Savings"}],"transactions":[]}"#)
        let source = try JSON.parse(#"{"id":"bank-1","date":"2026-10-24","payee":"BANK TRANSFER","memo":"","amount":-10000,"transferAccountId":"savings","candidates":[]}"#)
        let rows = try BankReview.prepare(rows: [source], budget: budget, allowIdlessDuplicate: true)
        XCTAssertEqual(rows[0].payee, "Transfer: Savings")
        XCTAssertEqual(BankReview.choices(rows, [:], allowIdlessDuplicate: true)[0]["row"].text("transferAccountId"), "savings")
    }

    func testARowThatDoesntSayWhereItsMoneyCameFromCanBeAddedAsATransferWithAnAccount() throws {
        let budget = try JSON.parse(#"""
        {"currency":"SAR","accounts":[{"id":"cash","name":"Cash"},{"id":"broker","name":"Broker"},{"id":"old","name":"Old","archived":true},{"id":"bank","name":"Everyday"}],
         "transactions":[{"id":"1","date":"2026-09-01","accountId":"bank","transferAccountId":"broker"},{"id":"2","accountId":"old","transferAccountId":"broker"},{"id":"3","accountId":"broker","transferAccountId":null},
                         {"id":"4","date":"2026-09-20","accountId":"cash","transferAccountId":null}]}
        """#)
        // The accounts it has moved money with come first; itself and closed ones are left out.
        let others = Investments.others(budget, account: "broker")
        XCTAssertEqual(others.map(\.id), ["bank", "cash"])
        let rows = [
            try JSON.parse(#"{"id":"ibkr:1","date":"2026-10-05","payee":"Interactive Brokers","memo":"","amount":100000,"candidates":[]}"#),
            try JSON.parse(#"{"id":"ibkr:2","date":"2026-10-09","payee":"Interactive Brokers","memo":"","amount":-50000,"transferAccountId":"cash","candidates":[]}"#),
            try JSON.parse(#"{"id":"ibkr:3","date":"2026-10-01","payee":"Interactive Brokers","memo":"","amount":7000,"duplicate":true,"candidates":[]}"#),
            try JSON.parse(#"{"id":"ibkr:4","date":"2026-10-12","payee":"Interactive Brokers","memo":"","amount":-2000,"candidates":[]}"#),
        ]
        let prepared = try BankReview.prepare(rows: rows, budget: budget, allowIdlessDuplicate: false, others: others, plain: "Add with no other account")
        // Nothing goes into another account on a guess: adding it plainly stays the default.
        XCTAssertEqual(prepared.map(\.initial), ["new", "new", "skip", "new"])
        XCTAssertEqual(prepared[0].options.map(\.id), ["review", "new", "transfer:bank", "transfer:cash", "skip"])
        XCTAssertEqual(Array(prepared[0].options.map(\.label)[1...2]), ["Add with no other account", "Add as a transfer from Everyday"])
        XCTAssertEqual(prepared[3].options[2].label, "Add as a transfer to Everyday")
        // The broker named the second row's other account: it's added as that transfer, and no other.
        XCTAssertEqual(prepared[1].options.map(\.id), ["review", "new", "skip"])
        XCTAssertEqual(prepared[1].options[1].label, "Add as a transfer")
        XCTAssertTrue(prepared[2].options.isEmpty)
        let chosen = [0: "transfer:bank", 1: "transfer:bank", 2: "transfer:bank"]
        let sent = BankReview.choices(prepared, chosen, allowIdlessDuplicate: false).array
        XCTAssertEqual(sent.map { $0.text("action") }, ["new", "new", "skip", "new"])
        XCTAssertEqual(sent[0]["row"].text("transferAccountId"), "bank")
        XCTAssertTrue(sent[0]["matchId"].isNull)
        // An account a row doesn't offer is not taken for it.
        XCTAssertEqual(sent[1]["row"].text("transferAccountId"), "cash")
        XCTAssertFalse(sent[3]["row"].has("transferAccountId"))
        XCTAssertEqual(BankReview.counts(prepared, chosen).importing, 3)
        // A bank's review, with no accounts to choose from, is as it was.
        XCTAssertEqual(try BankReview.prepare(rows: [rows[0]], budget: budget, allowIdlessDuplicate: false)[0].options.map(\.label),
                       ["Choose a match or add it", "Add as a new transaction", "Skip"])
    }

    /// A deposit is offered as a transfer only with accounts whose history reaches back to its date:
    /// one added later with its balance, or with no transactions, already holds that money in its
    /// opening balance, and a transfer would take it out twice.
    func testADepositIsOfferedOnlyWithAccountsThatExistedThen() throws {
        let budget = try JSON.parse(#"""
        {"currency":"SAR","accounts":[{"id":"broker","name":"Broker"},{"id":"bank","name":"Everyday"},{"id":"new","name":"New bank"},{"id":"empty","name":"Empty"},
                                      {"id":"card","name":"Card"}],
         "transactions":[{"id":"1","date":"2026-08-01","accountId":"bank","transferAccountId":null},
                         {"id":"2","date":"2026-11-15","accountId":"new","transferAccountId":null},
                         {"id":"3","date":"2026-09-10","accountId":"bank","transferAccountId":null,"splits":[{"amount":-100,"transferAccountId":"card"}]},
                         {"id":"4","date":"2026-07-01","accountId":"broker","transferAccountId":"bank"},
                         {"id":"5","accountId":"empty","transferAccountId":null}]}
        """#)
        // Each account carries its first transaction's date; either side of a transfer, a split's included, counts.
        let others = Investments.others(budget, account: "broker")
        XCTAssertEqual(others.map { $0.id }, ["bank", "new", "empty", "card"])
        XCTAssertEqual(others.map { $0.since }, ["2026-07-01", "2026-11-15", nil, "2026-09-10"])
        func offered(_ date: String) -> [String] { BankReview.offered(others, on: date).map { $0.id } }
        XCTAssertEqual(offered("2026-10-05"), ["bank", "card"])
        // A transaction on the day itself is history enough.
        XCTAssertEqual(offered("2026-09-10"), ["bank", "card"])
        XCTAssertEqual(offered("2026-09-09"), ["bank"])
        XCTAssertEqual(offered("2026-06-30"), [])
        XCTAssertEqual(offered("2026-11-16"), ["bank", "new", "card"])
        // A row with no date is offered every account with any history.
        XCTAssertEqual(offered(""), ["bank", "new", "card"])

        let rows = [
            try JSON.parse(#"{"id":"ibkr:1","date":"2026-10-05","payee":"Interactive Brokers","memo":"","amount":100000,"candidates":[]}"#),
            try JSON.parse(#"{"id":"ibkr:2","date":"2026-06-30","payee":"Interactive Brokers","memo":"","amount":-5000,"candidates":[]}"#),
            try JSON.parse(#"{"id":"ibkr:3","date":"2026-11-16","payee":"Interactive Brokers","memo":"","amount":2000,"candidates":[]}"#),
        ]
        let prepared = try BankReview.prepare(rows: rows, budget: budget, allowIdlessDuplicate: false, others: others, plain: "Add with no other account")
        XCTAssertEqual(prepared[0].options.map(\.id), ["review", "new", "transfer:bank", "transfer:card", "skip"])
        // Before any account's history began, it can only be added plainly.
        XCTAssertEqual(prepared[1].options.map(\.id), ["review", "new", "skip"])
        XCTAssertEqual(prepared[2].options.map(\.id), ["review", "new", "transfer:bank", "transfer:new", "transfer:card", "skip"])
        // Nothing starts on a hidden account, and an account chosen for a row that doesn't offer it isn't taken.
        XCTAssertEqual(prepared.map(\.initial), ["new", "new", "new"])
        let chosen = [0: "transfer:new", 1: "transfer:bank", 2: "transfer:new"]
        XCTAssertEqual(prepared.indices.map { prepared[$0].decision(chosen[$0]) }, ["new", "new", "transfer:new"])
        let sent = BankReview.choices(prepared, chosen, allowIdlessDuplicate: false).array
        XCTAssertFalse(sent[0]["row"].has("transferAccountId"))
        XCTAssertFalse(sent[1]["row"].has("transferAccountId"))
        XCTAssertEqual(sent[2]["row"].text("transferAccountId"), "new")
        XCTAssertEqual(BankReview.choices(prepared, [0: "transfer:card"], allowIdlessDuplicate: false).array[0]["row"].text("transferAccountId"), "card")
    }

    func testASingleCleanMatchIsImportedAndClearedByDefault() throws {
        let budget = try JSON.parse(#"{"currency":"SAR","transactions":[{"id":"t1","date":"2026-10-01","payee":"Shop","memo":"Weekly","amount":-500,"splits":[{"amount":-500,"memo":""}]}]}"#)
        let rows = [try JSON.parse(#"{"id":"bank-1","date":"2026-10-02","payee":"SHOP 123","rename":"Shop","memo":"Card","amount":-500,"candidates":["t1"]}"#)]
        let prepared = try BankReview.prepare(rows: rows, budget: budget, allowIdlessDuplicate: true)
        let row = try XCTUnwrap(prepared.first)
        XCTAssertEqual(row.initial, "match:t1")
        XCTAssertEqual(row.payee, "Shop")
        XCTAssertEqual(row.bankText, "SHOP 123")
        XCTAssertEqual(row.detail, "2026-10-02 · Card")
        XCTAssertEqual(row.amountText, "-5.00 SAR")
        XCTAssertTrue(row.options.contains(BankReviewOption(id: "match:t1", label: "Match: 2026-10-01 · Shop · Weekly")))
        XCTAssertEqual(row.options.first?.id, "review")
        let choice = BankReview.choices(prepared, [:], allowIdlessDuplicate: true)[0]
        XCTAssertEqual(choice.text("action"), "match")
        XCTAssertEqual(choice.text("matchId"), "t1")
        XCTAssertFalse(choice["row"].has("candidates"))
        let counts = BankReview.counts(prepared, [:])
        XCTAssertEqual(counts.importing, 1)
        XCTAssertEqual(counts.undecided, 0)
    }

    func testUndecidedRowsBlockTheImportAndAreNeverSentAsImports() throws {
        let budget = try JSON.parse(#"{"transactions":[{"id":"a","date":"2026-10-01","payee":"A","splits":[{"memo":"first"},{"memo":"second part"}]},{"id":"b","date":"2026-10-01","payee":"B"}]}"#)
        let rows = try [
            #"{"id":"1","amount":-1,"candidates":["a:leg:1","b"],"candidateDetails":[{"id":"a:leg:1","transactionId":"a","splitIndex":1}]}"#,
            #"{"id":"2","amount":-1,"possibleDuplicate":true,"idless":true,"candidates":[]}"#,
            #"{"id":"3","amount":-1,"candidates":["b"],"previouslyMatchedCandidates":["b"]}"#,
            #"{"id":"4","amount":-1,"duplicate":true,"candidates":["b"]}"#,
        ].map { try JSON.parse($0) }
        let prepared = try BankReview.prepare(rows: rows, budget: budget, allowIdlessDuplicate: false)
        XCTAssertEqual(prepared.map(\.initial), ["review", "review", "review", "skip"])
        XCTAssertTrue(prepared[0].options.contains(BankReviewOption(id: "match:a:leg:1", label: "Match: 2026-10-01 · A · Split 2 · second part")))
        XCTAssertEqual(prepared[1].options.first?.label, "May already be imported — choose")
        XCTAssertTrue(prepared[2].options.contains(BankReviewOption(id: "match:b", label: "Match: 2026-10-01 · B · Previously imported")))
        // Bank sync can't add an already-imported row again, or match it.
        XCTAssertTrue(prepared[3].locked)
        XCTAssertTrue(prepared[3].options.isEmpty)
        XCTAssertEqual(prepared[3].decision("new"), "skip")
        XCTAssertEqual(BankReview.counts(prepared, [:]).undecided, 3)
        XCTAssertEqual(BankReview.choices(prepared, [:], allowIdlessDuplicate: false).array.map { $0.text("action") }, ["skip", "skip", "skip", "skip"])
        let decided = [0: "match:a:leg:1", 1: "new", 2: "match:b"]
        XCTAssertEqual(BankReview.counts(prepared, decided).importing, 3)
        XCTAssertEqual(BankReview.counts(prepared, decided).undecided, 0)
        let sent = BankReview.choices(prepared, decided, allowIdlessDuplicate: false).array
        XCTAssertEqual(sent.map { $0.text("action") }, ["match", "new", "match", "skip"])
        XCTAssertEqual(sent[2]["row"]["previouslyMatchedCandidates"], ["b"])
        XCTAssertTrue(sent[3]["matchId"].isNull)
    }

    func testFileImportsCanAddAnIdlessDuplicateSeparately() throws {
        let rows = [try JSON.parse(#"{"id":"1","amount":-1,"duplicate":true,"idless":true,"candidates":["x"]}"#)]
        let prepared = try BankReview.prepare(rows: rows, budget: [:], allowIdlessDuplicate: true)
        let row = try XCTUnwrap(prepared.first)
        XCTAssertFalse(row.locked)
        XCTAssertEqual(row.options.map(\.id), ["skip", "new"])
        XCTAssertEqual(row.initial, "skip")
        XCTAssertEqual(BankReview.counts(prepared, [0: "new"]).importing, 1)
        // Adding it separately gives it an import ID of its own.
        let sent = BankReview.choices(prepared, [0: "new"], allowIdlessDuplicate: true).array
        XCTAssertEqual(sent[0]["row"].text("importId"), row.manualImportId)
    }

    func testSummaryCountsDuplicatesRenamesAndAllIncomingFiles() throws {
        let rows = try [
            #"{"id":"1","amount":100,"duplicate":true,"rename":"Pay","payee":"PAYROLL"}"#,
            #"{"id":"2","amount":100,"rename":"Pay","payee":"PAYROLL"}"#,
            #"{"id":"3","amount":100,"rename":"Same","payee":"Same"}"#,
            #"{"id":"4","amount":100,"idless":true,"possibleDuplicate":true}"#,
        ].map { try JSON.parse($0) }
        let summary = BankReview.summarize(try BankReview.prepare(rows: rows, budget: [:], allowIdlessDuplicate: true))
        XCTAssertEqual(summary, BankReviewSummary(rows: 4, duplicates: 1, renamed: 1, idless: true, allIn: true))
        XCTAssertEqual(BankReview.countsText(summary), "4 rows · 1 already imported · 1 renamed by your payee rules")
    }

    func testSharedCandidatePoolsAreReadFromTheirSet() throws {
        let budget = try JSON.parse(#"{"transactions":[{"id":"t1","date":"2026-10-01","payee":"Shop"}]}"#)
        let sets = [try JSON.parse(#"{"candidates":["t1"],"previouslyMatchedCandidates":[],"candidateDetails":[]}"#)]
        let rows = try [#"{"id":"1","amount":-1,"candidateSet":0}"#, #"{"id":"2","amount":-2,"candidateSet":0}"#].map { try JSON.parse($0) }
        let prepared = try BankReview.prepare(rows: rows, budget: budget, allowIdlessDuplicate: true, candidateSets: sets)
        XCTAssertEqual(prepared.map(\.initial), ["match:t1", "match:t1"])
        XCTAssertFalse(BankReview.choices(prepared, [:], allowIdlessDuplicate: true)[0]["row"].has("candidateSet"))
        XCTAssertThrowsError(try BankReview.prepare(rows: [try JSON.parse(#"{"id":"1","amount":-1,"candidateSet":3}"#)], budget: budget,
                                                    allowIdlessDuplicate: true, candidateSets: sets))
    }

    /// A CSV previewed, applied and previewed again through the real engine: the second preview
    /// finds every row already imported.
    func testBankPreviewAndApplyThroughTheEngine() throws {
        var budget = try NativeCore.engine("defaultBudget", params: ["name": "Test", "currency": "SAR"])
        try budget.upsertRecord("accounts", ["id": "cash", "name": "Checking", "kind": "cash", "openingBalance": 10000, "archived": false])
        let params: JSON = ["accountId": "cash", "content": "Date,Payee,Amount\n2026-10-01,Shop,-12.50\n2026-10-02,Salary,1000.00\n",
                            "format": "csv", "dateFormat": "ymd", "flipSigns": false]
        let preview = try NativeCore.engine("bankPreview", budget: budget, params: params)
        let rows = try BankReview.prepare(rows: preview.rows("items"), budget: budget, allowIdlessDuplicate: true, candidateSets: preview["candidateSets"].array)
        XCTAssertEqual(rows.map(\.initial), ["new", "new"])
        XCTAssertEqual(rows.map(\.amount), [-1250, 100_000])
        let applied = try NativeCore.engine("bankApply", budget: budget, params: ["accountId": "cash", "choices": BankReview.choices(rows, [:], allowIdlessDuplicate: true)])
        let changed = applied["budget"].isObject ? applied["budget"] : applied
        XCTAssertEqual(changed.rows("transactions").count, 2)
        let again = try NativeCore.engine("bankPreview", budget: changed, params: params)
        let second = try BankReview.prepare(rows: again.rows("items"), budget: changed, allowIdlessDuplicate: true, candidateSets: again["candidateSets"].array)
        XCTAssertTrue(second.allSatisfy(\.duplicate))
        XCTAssertEqual(BankReview.counts(second, [:]).importing, 0)
    }
}

final class ImportYnabTests: XCTestCase {
    override func setUp() {
        super.setUp()
        Money.configure(budget: ["currency": "SAR"])
    }

    func testDifferencesReadAsTheDesktopWritesThem() {
        XCTAssertEqual(ImportYnabText.difference(["kind": "account", "account": "Car Loan", "field": "balance", "actual": -1_060_000, "expected": -1_083_127], currency: "SAR"),
                       "Car Loan balance: -10,600.00 SAR here, -10,831.27 SAR in YNAB")
        XCTAssertEqual(ImportYnabText.difference(["category": "Groceries", "month": "2026-10-01", "field": "balance", "actual": 500, "expected": 700], currency: "SAR"),
                       "Groceries, October 2026, available: 5.00 SAR here, 7.00 SAR in YNAB")
        XCTAssertEqual(ImportYnabText.difference(["kind": "readyToAssign", "month": "2026-10", "actual": 0, "expected": 100], currency: "SAR"),
                       "Ready to Assign, October 2026: 0.00 SAR here, 1.00 SAR in YNAB")
        let audit: JSON = ["importAudit": ["accountDifferences": [["account": "A"]], "categoryDifferences": [["category": "C"]]]]
        XCTAssertEqual(ImportYnabText.differences(audit).map { $0.text("kind") }, ["account", "category"])
    }

    func testConflictValuesInWords() {
        XCTAssertEqual(ImportYnabText.conflictValue(.null, field: "memo", text: "", currency: "SAR"), "Deleted")
        XCTAssertEqual(ImportYnabText.conflictValue("x", field: "payee", text: "Shop, 2026-10-01", currency: "SAR"), "Shop, 2026-10-01")
        XCTAssertEqual(ImportYnabText.conflictValue(-2000, field: "amount", text: "", currency: "SAR"), "-20.00 SAR")
        XCTAssertEqual(ImportYnabText.conflictValue(true, field: "approved", text: "", currency: "SAR"), "Yes")
        XCTAssertEqual(ImportYnabText.conflictValue("", field: "memo", text: "", currency: "SAR"), "(empty)")
        XCTAssertEqual(ImportYnabText.conflictField("transferStatus"), "transfer's cleared")
    }

    /// YNAB's own export files, written by the engine from a sample budget, make a budget again
    /// through the request ImportsEngine writes by hand.
    func testExportFilesMakeABudgetThroughTheEngine() throws {
        let budget = try NativeCore.engine("sampleBudget", params: ["name": "Household", "currency": "SAR", "month": .string(Dates.currentMonth)])
        let register = try NativeCore.engine("exportRegister", budget: budget).string
        let plan = try NativeCore.engine("exportPlan", budget: budget).string
        let rebuilt = try ImportsEngine.importYnabFiles([ImportPickedFile(name: "Household - Register.csv", data: Data(register.utf8)),
                                                         ImportPickedFile(name: "Household - Plan.csv", data: Data(plan.utf8))],
                                                        dateOrder: "", currency: "SAR")
        XCTAssertTrue(rebuilt.has("schemaVersion"))
        XCTAssertEqual(rebuilt.text("currency"), "SAR")
        XCTAssertEqual(rebuilt.rows("accounts").count, budget.rows("accounts").count)
        XCTAssertThrowsError(try ImportsEngine.importYnabFiles([ImportPickedFile(name: "notes.csv", data: Data("a,b\n1,2".utf8))], dateOrder: "mdy", currency: "SAR"))
    }

    func testStatementTextFallsBackToWindowsLatin() throws {
        XCTAssertEqual(try ImportFiles.text(Data([0xEF, 0xBB, 0xBF]) + Data("Date".utf8)), "Date")
        XCTAssertEqual(try ImportFiles.text(Data([0x43, 0x61, 0x66, 0xE9])), "Café")
        // A PDF is the assistant's to read.
        XCTAssertThrowsError(try ImportFiles.text(Data("%PDF-1.7".utf8)))
        // The engine tells other encodings apart: Windows-1251, as Russian banks write.
        let russian = Data([0xC4, 0xE0, 0xF2, 0xE0, 0x3B, 0xD1, 0xF3, 0xEC, 0xEC, 0xE0, 0x3B, 0xCE, 0xEF, 0xE8, 0xF1, 0xE0, 0xED, 0xE8, 0xE5, 0x0A,
                            0x32, 0x31, 0x2E, 0x31, 0x30, 0x2E, 0x32, 0x30, 0x32, 0x36, 0x3B, 0x2D, 0x31, 0x35, 0x33, 0x34, 0x2C, 0x39, 0x30, 0x3B,
                            0xD1, 0xF3, 0xEF, 0xE5, 0xF0, 0xEC, 0xE0, 0xF0, 0xEA, 0xE5, 0xF2])
        XCTAssertEqual(try ImportFiles.text(russian), "Дата;Сумма;Описание\n21.10.2026;-1534,90;Супермаркет")
    }
}

/// A bank file's lines as a phone shows and fixes them (Android's BankStatementViewTest).
final class BankStatementTests: XCTestCase {
    override func setUp() {
        super.setUp()
        Money.configure(budget: ["currency": "EUR"])
    }

    private let session: JSON = ["scope": ["session": "s1", "budget": "b", "account": "checking", "file": "f"], "revision": 3,
                                 "structure": ["format": "csv", "dateOrder": ["order": "dmy", "confirmed": false], "flipSigns": false, "wholeBatches": ["e1"]],
                                 "edits": ["cells": [:], "fields": [:], "include": []], "versions": [:]]

    private func statement() throws -> JSON {
        try JSON.parse(#"""
        {"scope":{"session":"s1"},"revision":3,"format":"csv","order":"dmy",
         "sources":[{"label":"A1","currency":"EUR","lines":3},{"label":"B2","lines":1}],"source":0,
         "lines":[
          {"id":"30","version":2,"cells":["02/10/2026","Grocer","n/a"],"fields":{"date":"2026-10-02","payee":"Grocer","memo":"","amount":"n/a"},
           "status":{"kind":"attention","field":"amount","column":2,"reason":"Invalid amount: n/a"}},
          {"id":"60","version":0,"cells":["03/10/2026","Cinema","-12.00"],"fields":{"date":"2026-10-03","payee":"Cinema","memo":"","amount":"-12.00"},
           "status":{"kind":"leftOut","reason":"pending","detail":"Pending"}},
          {"id":"90","version":1,"cells":["04/10/2026","","-3.00"],"fields":{"date":"2026-10-04","payee":"","memo":"","amount":"-3.00"},
           "status":{"kind":"ready"},"fixed":true,"batch":0}],
         "counts":{"ready":2,"attention":1,"leftOut":{"pending":1}},
         "dateOrder":{"order":"dmy","example":"01/02/2026","unconfirmed":true},
         "batches":[{"entry":"e1","line":null,"payments":[2],"addsUp":false,"asOne":false}],
         "remember":{"dateFormat":"dmy","dateConfirmed":false,"flipSigns":false}}
        """#)
    }

    func testAStatementReadsAsThePhoneShowsIt() throws {
        let st = BankStatement(try statement())
        XCTAssertEqual(st.lines.map(\.kind), ["attention", "leftOut", "ready"])
        XCTAssertEqual(st.lines[0].problem, "Invalid amount: n/a")
        XCTAssertTrue(st.lines[1].includable)
        XCTAssertFalse(st.lines[0].includable)
        XCTAssertEqual(BankStatementText.leftOut(st.lines[1]), "Not booked yet (Pending)")
        // A line without a payee goes by what the file writes on it.
        XCTAssertEqual(st.lines[2].title, "04/10/2026 · -3.00")
        XCTAssertEqual(st.lines[2].batch, 0)
        XCTAssertEqual(st.sources, [StatementSource(label: "A1", currency: "EUR", lines: 3), StatementSource(label: "B2", currency: nil, lines: 1)])
        XCTAssertTrue(st.dateUnconfirmed)
        XCTAssertTrue(st.needsChecking)
        // A question still open: nothing goes on to the review yet.
        XCTAssertFalse(st.canContinue)
        XCTAssertNil(st.signsByType)
        XCTAssertEqual(st.attentionLines.map(\.id), ["30"])
        XCTAssertEqual(st.leftOutLines.map(\.id), ["60"])
        XCTAssertEqual(st.changedLines.map(\.id), ["90"])
        XCTAssertEqual(st.places["90"], 2)
        XCTAssertEqual(BankStatementText.counts(st), "2 lines read · 1 needs attention and won't be imported · 1 left out")
        XCTAssertEqual(st.batches, [StatementBatch(entry: "e1", line: nil, payments: [2], addsUp: false, asOne: false)])
        XCTAssertNil(st.elsewhere)
    }

    func testFixesAreMadeAgainstTheSessionAndEachLineAsItIs() throws {
        let line = BankStatement(try statement()).lines[0]
        // Only the parts changed, each against the value it has now.
        let fixes = try XCTUnwrap(BankFixes.fields(session, line, ["date": "2026-10-02", "payee": " Grocer ", "memo": "", "amount": " -12.40 "]))
        XCTAssertEqual(fixes.long("baseRevision"), 3)
        XCTAssertEqual(fixes["scope"].text("session"), "s1")
        XCTAssertEqual(fixes["baseStructure"], session["structure"])
        XCTAssertEqual(fixes["fields"], [["line": "30", "lineVersion": 2, "field": "amount", "from": "n/a", "to": "-12.40"]])
        XCTAssertNil(BankFixes.fields(session, line, ["amount": "n/a"]))
        XCTAssertTrue(BankFixes.include(session, line, true)["include"][0].flag("include"))
        XCTAssertEqual(BankFixes.revert(session, line)["revert"][0].long("lineVersion"), 2)
        // Reading the file another way changes a copy of how it reads now.
        let order = BankFixes.restructured(session) { $0["dateOrder"] = ["order": "mdy", "confirmed": true] }
        XCTAssertEqual(order["structure"]["dateOrder"].text("order"), "mdy")
        XCTAssertEqual(session["structure"]["dateOrder"].text("order"), "dmy")
        XCTAssertEqual(BankFixes.wholeBatch(session, "e2", true)["structure"]["wholeBatches"], ["e1", "e2"])
        XCTAssertEqual(BankFixes.wholeBatch(session, "e1", false)["structure"]["wholeBatches"], [])
    }

    func testAReviewIsOnlyEverOfTheImportItWasAskedFor() {
        let asked = BankReviewRequest(number: 1, generation: 1, session: "a", revision: 0, account: "checking")
        XCTAssertEqual(asked, asked)
        // Asked again (the budget changed): the earlier review shows until the new one lands.
        XCTAssertTrue(asked.sameImport(as: BankReviewRequest(number: 2, generation: 1, session: "a", revision: 0, account: "checking")))
        // Another file, though its session is at revision 0 too; a later change; another account: never.
        XCTAssertFalse(asked.sameImport(as: BankReviewRequest(number: 2, generation: 2, session: "b", revision: 0, account: "checking")))
        XCTAssertFalse(asked.sameImport(as: BankReviewRequest(number: 2, generation: 1, session: "a", revision: 1, account: "checking")))
        XCTAssertFalse(asked.sameImport(as: BankReviewRequest(number: 2, generation: 1, session: "a", revision: 0, account: "savings")))
    }

    func testADateTheOrdersReadDifferentlyIsNamedBothWays() {
        let named = BankStatementText.readings("01/02/2026")
        XCTAssertEqual(named?.dmy, "1 February 2026")
        XCTAssertEqual(named?.mdy, "2 January 2026")
        XCTAssertEqual(BankStatementText.readings("3.4.26")?.dmy, "3 April 2026")
        XCTAssertNil(BankStatementText.readings("2026-10-01"))
    }

    func testMoneyBackFromAShopSaysWhatItMayBeARefundOf() throws {
        let one = try JSON.parse(#"{"id":"t1","date":"2026-10-07","payee":"Amazon EU","amount":-2399,"categoryId":"shopping","purchases":1}"#)
        let text = BankStatementText.refund(one, categories: ["shopping": "Shopping"], currency: "EUR")
        XCTAssertTrue(text.hasPrefix("Refund of Amazon EU · ") && text.hasSuffix(" · Shopping"), text)
        let many = try JSON.parse(#"{"id":"t1","date":"2026-10-07","payee":"Amazon EU","amount":-2399,"purchases":2}"#)
        XCTAssertEqual(BankStatementText.refund(many, categories: [:], currency: "EUR"), "Possible refund · 2 purchases from Amazon EU match")
    }

    func testTheAssistantsFixesSayWhichAppliedAndWhyOthersDidnt() throws {
        let some = BankStatementText.outcome(try JSON.parse(#"""
        {"session":{},"results":[{"line":"30","fix":"field","applied":true},
         {"line":"60","fix":"field","applied":false,"problem":"This line changed since this fix was made."}]}
        """#))
        XCTAssertEqual(some.lines, ["30"])
        XCTAssertEqual(some.applied, 1)
        XCTAssertEqual(some.notice, "Applied 1 of 2 fixes. One no longer fits: This line changed since this fix was made.")
        let all = BankStatementText.outcome(try JSON.parse(#"{"results":[{"line":"30","applied":true},{"fix":"structure","applied":true}]}"#))
        XCTAssertEqual(all.notice, "Applied the assistant's 2 fixes.")
    }

    func testAWorkbookIsReadAtTheSheetAskedForAndSaysItsSheets() throws {
        var asked: String?
        let read = try ImportFiles.statement(Data([0x50, 0x4B, 0x03, 0x04, 1, 2, 3]), sheet: "Resumen") { _, sheet in
            asked = sheet
            return ["text": "Date,Amount\n", "sheet": "Resumen", "sheets": ["Movimientos", "Resumen"]]
        }
        XCTAssertEqual(asked, "Resumen")
        XCTAssertEqual(read, DecodedStatement(text: "Date,Amount\n", sheet: "Resumen", sheets: ["Movimientos", "Resumen"]))
        XCTAssertEqual(try ImportFiles.statement(Data("Date,Amount\n".utf8), sheet: nil), DecodedStatement(text: "Date,Amount\n"))
    }

    /// A line that doesn't read, fixed, imported with the rest through the real engine; the same
    /// broken file again finds every line imported, the fixed one by the line as the file writes it.
    func testALineThatDoesntReadIsFixedAndImportedThroughTheEngine() throws {
        var budget = try NativeCore.engine("defaultBudget", params: ["name": "Test", "currency": "EUR"])
        try budget.upsertRecord("accounts", ["id": "cash", "name": "Checking", "kind": "cash", "openingBalance": 10000, "archived": false])
        let content = "Date,Payee,Amount\n2026-10-14,Bakery,-3.20\n2026-10-15,Grocer,n/a\n"
        func params(_ more: JSON) -> JSON {
            var all: JSON = ["accountId": "cash", "content": .string(content)]
            for (key, value) in more.object { all[key] = value }
            return all
        }
        let session = try NativeCore.engine("bankSession", budget: budget, params: params(["sessionId": "s1", "budgetId": ""]))
        let shown = BankStatement(try NativeCore.engine("bankStatement", budget: budget, params: params(["session": session, "problemsOnly": true])))
        XCTAssertEqual(shown.lines.map(\.kind), ["attention"])
        let fixes = try XCTUnwrap(BankFixes.fields(session, shown.lines[0], ["amount": "-12.40"]))
        let applied = try NativeCore.engine("bankApplyFixes", budget: budget, params: params(["session": session, "fixes": fixes]))
        let preview = try NativeCore.engine("bankImportPreview", budget: budget, params: params(["session": applied["session"], "problemsOnly": true]))
        let rows = try BankReview.prepare(rows: preview["review"].rows("items"), budget: budget, allowIdlessDuplicate: true,
                                          candidateSets: preview["review"]["candidateSets"].array)
        XCTAssertEqual(rows.map(\.amount), [-320, -1240])
        let choices = BankReview.choices(rows, [:], allowIdlessDuplicate: true)
        let done = try NativeCore.engine("bankApply", budget: budget, params: ["accountId": "cash", "choices": choices,
                                                                                "remember": BankStatement(preview["statement"]).remember])
        let changed = done["budget"].isObject ? done["budget"] : done
        XCTAssertEqual(changed.rows("transactions").count, 2)
        let again = try NativeCore.engine("bankSession", budget: changed, params: params(["sessionId": "s2", "budgetId": ""]))
        let second = try NativeCore.engine("bankImportPreview", budget: changed, params: params(["session": again, "problemsOnly": true]))
        XCTAssertTrue(second["review"].rows("items").allSatisfy { $0.flag("duplicate") })
        XCTAssertEqual(BankStatement(second["statement"]).lines.map(\.reason), ["imported"])
    }
}

/// Investment accounts a broker fills (Holdings.swift), as the desktop's Investments.test.tsx
/// checks them: the gain is the market changes added up, the broker's text is shown as written,
/// and nothing the app edits drops the holdings.
final class InvestmentsTests: XCTestCase {
    override func setUp() {
        super.setUp()
        Money.configure(budget: ["currency": "SAR", "dateFormat": "iso"])
    }

    private func sample() throws -> JSON {
        try JSON.parse(#"""
        {"schemaVersion":2,"name":"Test","currency":"SAR",
         "accounts":[
          {"id":"cash","name":"Checking","kind":"cash","openingBalance":0,"archived":false},
          {"id":"broker","name":"Brokerage","kind":"tracking","accountType":"investment","openingBalance":0,"archived":false},
          {"id":"pension","name":"Pension","kind":"tracking","accountType":"retirement","openingBalance":0,"archived":false},
          {"id":"loan","name":"Car loan","kind":"tracking","accountType":"autoLoan","openingBalance":-100,"archived":false},
          {"id":"old","name":"Old fund","kind":"tracking","accountType":"investment","openingBalance":0,"archived":true}],
         "categories":[],"assignments":[],
         "transactions":[
          {"id":"investment-start:broker","date":"2026-09-30","accountId":"broker","payee":"Starting value","amount":2422000,"investment":"start"},
          {"id":"investment-change:broker:2026-10","date":"2026-10-30","accountId":"broker","payee":"Market change","amount":128000,"investment":"change"},
          {"id":"investment-change:broker:2026-11","date":"2026-11-13","accountId":"broker","payee":"Market change","amount":-8000,"investment":"change"},
          {"id":"d1","date":"2026-10-05","accountId":"broker","payee":"Interactive Brokers","amount":100000}],
         "investments":[{"accountId":"broker","source":"ibkr","sourceAccount":"U1234567","asOf":"2026-11-13","currency":"USD","rate":"3.75","total":2642000,
           "positions":[{"symbol":"VWRA","name":"VANG FTSE AW USDA","kind":"STK","currency":"USD","quantity":"120.5","price":"141.9087137","value":1710000,"cost":1500000,"gain":210000}],
           "cash":[{"currency":"USD","amount":"820","value":82000}],"other":1250,
           "values":[["2026-09-30",2422000],["2026-10-30",2550000],["2026-11-13",2642000]],"syncedAt":"2026-11-14T06:00:00.000Z"}]}
        """#)
    }

    func testTheGainIsTheMarketChangesAndPricesAgeAfterFourDays() throws {
        let budget = try sample()
        let held = try XCTUnwrap(Investments.holdings(budget, account: "broker"))
        XCTAssertNil(Investments.holdings(budget, account: "cash"))
        XCTAssertNil(Investments.holdings(budget, account: ""))
        XCTAssertNil(Investments.holdings(nil, account: "broker"))
        // The starting value and a deposit aren't gains; the two months' changes are.
        XCTAssertEqual(Investments.gain(budget, account: "broker"), 120_000)
        XCTAssertNil(Investments.gain(budget, account: "pension"))
        XCTAssertEqual(Investments.trackedSince(held), "2026-09-30")
        XCTAssertNil(Investments.trackedSince(["values": []]))
        XCTAssertFalse(Investments.stale(held, today: "2026-11-16"))
        XCTAssertFalse(Investments.stale(held, today: "2026-11-17"))
        XCTAssertTrue(Investments.stale(held, today: "2026-11-18"))
    }

    /// The page the engine gives for the fixture of its own tests (investment_stats.rs), cut to what's read here.
    private func holdingsPage() throws -> JSON {
        try JSON.parse(#"""
        {"days":["2025-12-31","2026-09-30","2026-10-30","2026-11-12","2026-11-13"],
         "values":[2000000,2422000,2550000,2690000,2701250],
         "gains":[0,422000,450000,550000,556250],
         "periods":[{"period":"1m","first":1,"from":"2026-09-30","to":"2026-11-13","gain":134250,"rate":0.0534},
                    {"period":"all","first":0,"from":"2025-12-31","to":"2026-11-13","gain":556250,"rate":null}]}
        """#)
    }

    func testAnInvestmentPagesFiguresArePickedApartForTheScreen() throws {
        XCTAssertEqual([0.633, 0.004, 0, 1].map { HoldingsPage.percent($0) }, ["63%", "<1%", "0%", "100%"])
        XCTAssertEqual([0.0552, -0.1, 0, -0.0001].map { HoldingsPage.percent($0, signed: true) }, ["+5.5%", "-10.0%", "0.0%", "0.0%"])
        // Income and costs are small beside the account: to a tenth, and never "0.0%" for something.
        XCTAssertEqual([0.0031, -0.0005, 0.0003, 0, -0.12].map { HoldingsPage.tenths($0) }, ["0.3%", "0.1%", "<0.1%", "0.0%", "12.0%"])
        // A period's days come out of the whole run, with its gains starting at nothing.
        let page = try holdingsPage()
        let month = HoldingsPage.series(page, try XCTUnwrap(HoldingsPage.period(page, "1m")))
        XCTAssertEqual(month.days, ["2026-09-30", "2026-10-30", "2026-11-12", "2026-11-13"])
        XCTAssertEqual(month.values, [2_422_000, 2_550_000, 2_690_000, 2_701_250])
        XCTAssertEqual(month.gains, [0, 28_000, 128_000, 134_250])
        XCTAssertEqual(HoldingsPage.series(page, try XCTUnwrap(HoldingsPage.period(page, "all"))).days.count, 5)
        XCTAssertNil(HoldingsPage.period(page, "3m"))
        // What it would be worth with no gain: the value less the gain so far; nothing where more was taken out than put in.
        XCTAssertEqual(HoldingsPage.before(month), [2_422_000, 2_522_000, 2_562_000, 2_567_000])
        XCTAssertEqual(HoldingsPage.before(HoldingsPage.Series(days: ["a", "b"], values: [1000, 200], gains: [0, 900])), [1000, nil])
        // A rate the engine leaves out is none, not zero.
        XCTAssertNil(HoldingsPage.number(try XCTUnwrap(HoldingsPage.period(page, "all")), "rate"))
        XCTAssertEqual(try XCTUnwrap(HoldingsPage.number(try XCTUnwrap(HoldingsPage.period(page, "1m")), "rate")), 0.0534, accuracy: 1e-12)
    }

    func testTheChartsDatesAndAxisFitWhatItDraws() {
        XCTAssertEqual(HoldingsPage.epochDay("1970-01-01"), 0)
        XCTAssertEqual(HoldingsPage.epochDay("2026-11-02"), 20_759)
        // Under a month: the first day of each new week (weeks turn on Monday).
        XCTAssertEqual(HoldingsPage.ticks(["2026-11-02", "2026-11-03", "2026-11-06", "2026-11-09", "2026-11-10", "2026-11-16"]), [nil, nil, nil, .day, nil, .day])
        // Over four months: the first valued day of each month, the year where it turns.
        XCTAssertEqual(HoldingsPage.ticks(["2025-10-31", "2025-11-28", "2025-12-31", "2026-01-30", "2026-02-27", "2026-02-28"]), [nil, .month, .month, .year, .month, nil])
        XCTAssertTrue(HoldingsPage.ticks([]).isEmpty)
        // The value's axis fits it and doesn't go down to zero, which would flatten the line.
        XCTAssertEqual(HoldingsPage.fittedTicks(2_422_000, 2_701_250), [2_400_000, 2_500_000, 2_600_000, 2_700_000, 2_800_000])
        XCTAssertEqual(HoldingsPage.fittedTicks(0, 101_000), [0, 50_000, 100_000, 150_000])
        XCTAssertEqual(HoldingsPage.fittedTicks(5, 5), ReportMath.niceTicks(5, 5))
    }

    func testHoldingsAreOrderedAndWrittenForTheirRows() throws {
        let rows = [
            try JSON.parse(#"{"symbol":"VWRA","name":"Fund","value":1710000,"gain":210000,"rate":0.14}"#),
            try JSON.parse(#"{"symbol":"2222","name":"oil","value":288000,"gain":-32000,"rate":-0.1}"#),
            try JSON.parse(#"{"symbol":"T","name":"Note","value":492500,"gain":null,"rate":null}"#),
        ]
        func order(_ key: String) -> [String] { HoldingsPage.sorted(rows, by: key).map { $0.text("symbol") } }
        XCTAssertEqual(order("value"), ["VWRA", "T", "2222"])
        // One the broker gives no cost for has no gain or return, and comes last.
        XCTAssertEqual(order("gain"), ["VWRA", "2222", "T"])
        XCTAssertEqual(order("rate"), ["VWRA", "2222", "T"])
        XCTAssertEqual(order("name"), ["VWRA", "T", "2222"])
        // A bond's name is its symbol, and an option's code only spells its name out again.
        XCTAssertEqual(HoldingsPage.line(try JSON.parse(#"{"symbol":"T 4 1/4","name":"T 4 1/4","quantity":"5000","price":"98.5","currency":"USD"}"#)), "5,000 × 98.5 USD")
        XCTAssertEqual(HoldingsPage.line(try JSON.parse(#"{"symbol":"AAPL  261218C00250000","name":"AAPL 18DEC26 250 C","quantity":"1","price":"2.5","currency":"USD"}"#)), "1 × 2.5 USD")
        // The order is given as each holding's place, which its row keeps as its id in any order.
        XCTAssertEqual(HoldingsPage.order(rows, by: "value"), [0, 2, 1])
        // A tile carries a symbol's first word, or a name's when there's no symbol.
        XCTAssertEqual(HoldingsPage.mark(symbol: "SPY 18DEC26 710 P", name: "SPY 18DEC26 710 P"), "SPY")
        XCTAssertEqual(HoldingsPage.mark(symbol: "ARCC 5 1/4 04/12/31", name: "ARCC 5 1/4 04/12/31"), "ARCC")
        XCTAssertEqual(HoldingsPage.mark(symbol: "GBLATL 3 1/8", name: "x"), "GBLATL")
        XCTAssertEqual(HoldingsPage.mark(symbol: " ", name: "vanguard fund"), "VANGUA")
        XCTAssertEqual(HoldingsPage.mark(symbol: "BRK.B", name: "Berkshire"), "BRK")
    }

    func testADayChartsDatesStepByTheirMarksNotByTheirDays() {
        // A year by the day marks each month's first day: every other mark is every other month,
        // counted back from the last, wherever the days fall.
        var ticks: [ReportXTick?] = []
        var keys: [String] = []
        for month in 1...12 {
            for day in 1...30 {
                keys.append(String(format: "2026-%02d-%02d", month, day))
                ticks.append(day == 1 ? ReportMath.plainTick("M\(month)") : nil)
            }
        }
        let roomy = ReportAxis.xLabels(ticks, keys: keys, band: 2, spacing: 52)
        XCTAssertEqual(roomy.count, 12)
        let tight = ReportAxis.xLabels(ticks, keys: keys, band: 1, spacing: 52)
        XCTAssertEqual(Set(tight.keys), Set(stride(from: 2, through: 12, by: 2).map { String(format: "2026-%02d-01", $0) }))
        // A chart by the month is as it was: a label for each month that has room.
        let months = (1...12).map { String(format: "2026-%02d", $0) }
        XCTAssertEqual(ReportAxis.xLabels(months.map { ReportMath.plainTick($0) }, keys: months, band: 30, spacing: 52).count, 6)
    }

    func testTheDailySyncThePromptAndALoginsNameAreReadFromTheBudget() throws {
        let budget = try sample()
        XCTAssertTrue(Investments.filled(budget))
        XCTAssertFalse(Investments.filled(nil))
        XCTAssertFalse(Investments.filled(["investments": []]))
        // Once a day for a budget Interactive Brokers fills: not when turned off, and not twice in a day, even after a failure.
        XCTAssertTrue(Investments.dailyDue(filled: true, on: true, triedOn: "2026-11-16", today: "2026-11-17"))
        XCTAssertTrue(Investments.dailyDue(filled: true, on: true, triedOn: "", today: "2026-11-17"))
        XCTAssertFalse(Investments.dailyDue(filled: true, on: true, triedOn: "2026-11-17", today: "2026-11-17"))
        XCTAssertFalse(Investments.dailyDue(filled: true, on: false, triedOn: "", today: "2026-11-17"))
        XCTAssertFalse(Investments.dailyDue(filled: false, on: true, triedOn: "", today: "2026-11-17"))
        XCTAssertEqual(Investments.newestPrices(budget), Investments.holdings(budget, account: "broker")?.text("asOf"))
        XCTAssertEqual(Investments.newestPrices(nil), "")
        // Only an open investment account no broker fills is asked about.
        let open = try JSON.parse(#"{"id":"brokerage","name":"Brokerage","kind":"tracking","accountType":"investment"}"#)
        XCTAssertTrue(Investments.promptable(open, budget: budget))
        XCTAssertFalse(Investments.promptable(try JSON.parse(#"{"id":"broker","kind":"tracking","accountType":"investment"}"#), budget: budget))
        XCTAssertFalse(Investments.promptable(try JSON.parse(#"{"id":"old","kind":"tracking","accountType":"investment","archived":true}"#), budget: budget))
        XCTAssertFalse(Investments.promptable(try JSON.parse(#"{"id":"pension","kind":"tracking","accountType":"retirement"}"#), budget: budget))
        XCTAssertFalse(Investments.promptable(try JSON.parse(#"{"id":"cash","kind":"cash","accountType":"checking"}"#), budget: budget))
        // A login is called by the accounts it brings: an alias from the statement at hand, else the id.
        let accounts = [try JSON.parse(#"{"id":"U1234567","alias":"Long term"}"#), try JSON.parse(#"{"id":"U7654321","alias":""}"#)]
        XCTAssertEqual(Investments.loginName(try JSON.parse(#"{"query":"987654","accounts":["U1234567","U7654321","U9999999"]}"#), accounts: accounts),
                       "Long term, U7654321, U9999999")
        XCTAssertNil(Investments.loginName(try JSON.parse(#"{"query":"111222","accounts":[]}"#), accounts: accounts))
        XCTAssertLessThanOrEqual(Investments.flexRequest.count, 200)
    }

    func testThePageListsWhatCanHoldInvestmentsAndAFileOrATypedValueGoesToTheEngineAsRead() throws {
        var budget = try sample()
        var accounts = budget["accounts"].array
        accounts.append(try JSON.parse(#"{"id":"untyped","name":"Other","kind":"tracking","openingBalance":0,"archived":false}"#))
        accounts.append(try JSON.parse(#"{"id":"savings","name":"Savings","kind":"cash","accountType":"savings","openingBalance":0,"archived":false}"#))
        budget["accounts"] = .array(accounts)
        // A cash account something fills is listed too, so it can be unlinked.
        var held = budget["investments"].array
        held.append(try JSON.parse(#"{"accountId":"savings","source":"manual","asOf":"2026-11-13","total":500000,"positions":[],"cash":[],"values":[["2026-11-13",500000]]}"#))
        budget["investments"] = .array(held)
        XCTAssertEqual(Investments.rows(budget).map { $0.text("id") }, ["broker", "pension", "untyped", "savings"])
        XCTAssertEqual(Investments.rows(nil).count, 0)
        XCTAssertEqual(Investments.sourceName(held[0]), "Interactive Brokers")
        XCTAssertEqual(["simplefin", "file", "manual"].map { Investments.sourceName(["source": .string($0)]) }, ["Bank connection", "Positions file", "Value you type in"])

        let read = try JSON.parse(#"""
        {"format":"csv","asOf":"2026-11-14","currency":"USD","cash":"820.5","value":"20820.5","skipped":0,
         "positions":[{"symbol":"VWRA","name":"Vanguard FTSE All-World","kind":"FUND","currency":"USD","quantity":"120.5","price":"165.975","value":"20000","cost":"18000"}]}
        """#)
        // In the budget's own currency no rate goes along, whatever was typed for one.
        let same = Investments.fileSnapshot(read, fileName: "positions.csv", asOf: "2026-11-14", currency: " usd ", budgetCurrency: "USD", rate: "3.75")
        XCTAssertEqual(["source", "sourceAccount", "asOf", "currency", "rate", "cash"].map { same.text($0) }, ["file", "positions.csv", "2026-11-14", "", "", "820.5"])
        XCTAssertEqual(same["positions"].array.first?.text("value"), "20000")
        // In another, the currency goes with what one unit of it is worth.
        let other = Investments.fileSnapshot(read, fileName: "positions.csv", asOf: "2026-11-14", currency: "usd", budgetCurrency: "SAR", rate: "3.75")
        XCTAssertEqual([other.text("currency"), other.text("rate")], ["USD", "3.75"])
        XCTAssertEqual(Investments.foreign("usd", budgetCurrency: "SAR"), "USD")
        XCTAssertNil(Investments.foreign("", budgetCurrency: "SAR"))
        // The rate given before is offered again for the same currency only.
        let before = try JSON.parse(#"{"currency":"USD","manualRate":"3.75"}"#)
        XCTAssertEqual(Investments.rememberedRate(before, code: "USD"), "3.75")
        XCTAssertEqual(Investments.rememberedRate(before, code: "EUR"), "")
        XCTAssertEqual(Investments.rememberedRate(nil, code: "USD"), "")

        let typed = Investments.typedSnapshot(cents: 3_100_050, asOf: "2026-11-17")
        XCTAssertEqual(["source", "asOf", "total"].map { typed.text($0) }, ["manual", "2026-11-17", "31000.50"])
        XCTAssertEqual(Investments.cents("20820.5"), 2_082_050)
        XCTAssertEqual(Investments.cents(""), 0)
        XCTAssertEqual(Investments.decimal(5), "0.05")
        XCTAssertEqual(Investments.decimal(-1_250), "-12.50")
    }

    func testQuantitiesAndPricesAreTheBrokersTextTrimmedForReading() throws {
        XCTAssertEqual(Investments.plain("141.9087137"), "141.9087")
        XCTAssertEqual(Investments.plain("120.5"), "120.5")
        XCTAssertEqual(Investments.plain("1234567"), "1,234,567")
        XCTAssertEqual(Investments.plain("820", places: 2), "820")
        XCTAssertEqual(Investments.plain("-0.126", places: 2), "-0.13")
        XCTAssertEqual(Investments.plain("-0.001", places: 2), "0")
        XCTAssertEqual(Investments.plain("n/a"), "n/a")
        XCTAssertEqual(Investments.plain(""), "")
        let held = try XCTUnwrap(Investments.holdings(try sample(), account: "broker"))
        XCTAssertEqual(HoldingsPage.line(held.rows("positions")[0]), "VWRA · 120.5 × 141.9087 USD")
        XCTAssertEqual(Investments.cashLine(held.rows("cash")[0]), "820 USD")
        XCTAssertEqual(Investments.signed(120_000, "SAR"), "+1,200.00 SAR")
        XCTAssertEqual(Investments.signed(-8_000, "SAR"), "-80.00 SAR")
        XCTAssertEqual(Investments.signed(0, "SAR"), "0.00 SAR")
        // A budget that writes numbers the European way reads them that way here too.
        Money.configure(budget: ["currency": "EUR", "numberFormat": ["decimal": ",", "group": "."]])
        XCTAssertEqual(Investments.plain("1234.5"), "1.234,5")
    }

    func testAnExchangeRateReadsHoweverItWasTyped() {
        XCTAssertEqual(Investments.rate("3.75"), "3.75")
        XCTAssertEqual(Investments.rate(" 150,5 "), "150.5")
        XCTAssertEqual(Investments.rate("٣٫٧٥"), "3.75")
        for bad in ["", "0", "0.0", "-1", "abc", "1e5", "1.2.3"] { XCTAssertNil(Investments.rate(bad), bad) }
        XCTAssertNil(Investments.rate(nil))
    }

    func testAStatementsAccountsGoWhereTheyAreToldAndNewOnesByDefault() throws {
        let budget = try sample()
        // The broker's own account is filled already, a loan isn't an investment, and a closed account takes nothing.
        let targets = Investments.targets(budget)
        XCTAssertEqual(targets.map { $0.text("id") }, ["pension"])
        let accounts = try JSON.parse(#"""
        [{"id":"U1","alias":"","accountId":"broker","needsRate":null,"rows":[{"id":"a","duplicate":false},{"id":"b","duplicate":true}]},
         {"id":"U2","alias":"Long term","accountId":null,"needsRate":["USD","JPY"],"rows":[]},
         {"id":"U3","alias":"","accountId":null,"needsRate":null,"rows":[]}]
        """#).array
        XCTAssertEqual(Investments.fresh(accounts[0]), 1)
        XCTAssertEqual(Investments.fresh(nil), 0)
        XCTAssertNil(Investments.needsRate(accounts[0]))
        XCTAssertEqual(Investments.needsRate(accounts[1])?.from, "USD")
        XCTAssertEqual(Investments.needsRate(accounts[1])?.to, "JPY")
        XCTAssertEqual(Investments.adding(accounts, into: [:], targets: targets).map { $0.text("id") }, ["U2", "U3"])
        // The one in another currency waits for its rate, unless it's left out.
        XCTAssertFalse(Investments.ready(accounts, into: [:], rates: [:], targets: targets))
        XCTAssertTrue(Investments.ready(accounts, into: ["U2": ""], rates: [:], targets: targets))
        XCTAssertTrue(Investments.ready(accounts, into: [:], rates: ["U2": "150,5"], targets: targets))
        var ids = ["n1", "n2"].makeIterator()
        let links = Investments.links(accounts, into: ["U3": "pension"], rates: ["U2": "150,5"], targets: targets) { ids.next() ?? "none" }
        XCTAssertEqual(links, [["source": "U2", "account": "n1", "create": true, "rate": "150.5"],
                               ["source": "U3", "account": "pension", "create": false]])
        XCTAssertEqual(Investments.links(accounts, into: ["U2": "", "U3": ""], rates: [:], targets: targets), [])
        // A choice naming an account that has since gone falls back to a new one.
        XCTAssertEqual(Investments.target(accounts[2], into: ["U3": "gone"], targets: targets), Investments.newAccount)
        // A linked account that was waiting for its rate is told it.
        let waiting = try JSON.parse(#"[{"id":"U9","accountId":"broker","needsRate":["USD","JPY"],"rows":[]}]"#).array
        XCTAssertEqual(Investments.links(waiting, into: [:], rates: ["U9": "150"], targets: targets), [["source": "U9", "account": "broker", "rate": "150"]])
        XCTAssertEqual(Investments.links(waiting, into: [:], rates: [:], targets: targets), [])
    }

    /// The holdings are an extra the app doesn't model: an edit elsewhere must send none of it,
    /// and unlinking is the one edit that takes an entry out.
    func testEditsKeepTheHoldingsAndUnlinkingTakesOneOut() throws {
        let budget = try sample()
        var renamed = budget
        var accounts = renamed["accounts"].array
        accounts[0]["name"] = "Everyday"
        renamed["accounts"] = .array(accounts)
        let patch = BudgetPatch.diff(from: budget, to: renamed)
        XCTAssertFalse(patch.has("extras"))
        XCTAssertFalse(patch.has("transactions"))
        let after = BudgetPatch.apply(patch, to: budget)
        XCTAssertEqual(after["investments"], budget["investments"])
        XCTAssertEqual(after.rows("transactions").count { $0.text("investment") == "change" }, 2)

        var unlinked = budget
        Investments.unlink(&unlinked, account: "broker")
        XCTAssertFalse(unlinked.has("investments"))
        // Its history stays, so its balance does.
        XCTAssertEqual(unlinked["transactions"], budget["transactions"])
        let removal = BudgetPatch.diff(from: budget, to: unlinked)
        XCTAssertEqual(removal["extras"]["delete"].array.map(\.string), ["investments"])
        XCTAssertNil(Investments.holdings(BudgetPatch.apply(removal, to: budget), account: "broker"))

        // With another account filled too, the list is sent without the one unlinked.
        var two = budget
        var list = two["investments"].array
        var second = list[0]
        second["accountId"] = "pension"
        list.append(second)
        two["investments"] = .array(list)
        var one = two
        Investments.unlink(&one, account: "broker")
        XCTAssertEqual(one.rows("investments").map { $0.text("accountId") }, ["pension"])
        XCTAssertEqual(BudgetPatch.diff(from: two, to: one)["extras"]["set"]["investments"], one["investments"])
        // What a sync on another device brought arrives as a patch and is kept whole.
        let arrived = BudgetPatch.apply(["extras": ["set": ["investments": two["investments"]], "delete": []]], to: unlinked)
        XCTAssertEqual(arrived["investments"], two["investments"])
    }
}
