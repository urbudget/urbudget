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
