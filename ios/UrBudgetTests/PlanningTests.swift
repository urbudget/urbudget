import XCTest
@testable import UrBudget

final class ScheduleMathTests: XCTestCase {
    func testNextDatesFollowTheEngineRules() {
        // Monthly keeps the anchor's day, clamped to short months.
        XCTAssertEqual(ScheduleMath.next("2026-01-31", anchor: "2026-01-31", frequency: "monthly"), "2026-02-28")
        XCTAssertEqual(ScheduleMath.next("2026-02-28", anchor: "2026-01-31", frequency: "monthly"), "2026-03-31")
        XCTAssertEqual(ScheduleMath.next("2024-02-29", anchor: "2024-02-29", frequency: "yearly"), "2025-02-28")
        XCTAssertEqual(ScheduleMath.next("2026-11-15", anchor: "2026-11-15", frequency: "every3Months"), "2027-02-15")
        XCTAssertEqual(ScheduleMath.next("2026-10-21", anchor: "2026-10-21", frequency: "everyOtherYear"), "2028-10-21")
        // Twice a month: the anchor's day within the first half, and fifteen days on.
        XCTAssertEqual(ScheduleMath.next("2026-01-05", anchor: "2026-01-05", frequency: "twiceAMonth"), "2026-01-20")
        XCTAssertEqual(ScheduleMath.next("2026-01-20", anchor: "2026-01-20", frequency: "twiceAMonth"), "2026-02-05")
        XCTAssertEqual(ScheduleMath.next("2026-01-31", anchor: "2026-01-31", frequency: "twiceAMonth"), "2026-02-01")
        XCTAssertEqual(ScheduleMath.next("2026-02-15", anchor: "2026-01-15", frequency: "twiceAMonth"), "2026-02-28")
        XCTAssertEqual(ScheduleMath.next("2026-02-28", anchor: "2026-01-15", frequency: "twiceAMonth"), "2026-03-15")
        // Days.
        XCTAssertEqual(ScheduleMath.next("2026-12-31", anchor: "2026-12-31", frequency: "daily"), "2027-01-01")
        XCTAssertEqual(ScheduleMath.next("2026-10-21", anchor: "2026-10-21", frequency: "weekly"), "2026-10-28")
        XCTAssertEqual(ScheduleMath.next("2026-10-21", anchor: "2026-10-21", frequency: "every4Weeks"), "2026-11-18")
        // Once, unknown frequencies and bad dates have nothing after.
        XCTAssertNil(ScheduleMath.next("2026-10-21", anchor: "2026-10-21", frequency: "never"))
        XCTAssertNil(ScheduleMath.next("2026-10-21", anchor: "2026-10-21", frequency: "fortnightly"))
        XCTAssertNil(ScheduleMath.next("2026-02-30", anchor: "2026-02-01", frequency: "monthly"))
        XCTAssertEqual(ScheduleMath.dates(from: "2026-01-31", anchor: "2026-01-31", frequency: "monthly", count: 4),
                       ["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30"])
        XCTAssertEqual(ScheduleMath.dates(from: "2026-01-31", anchor: "2026-01-31", frequency: "never", count: 4), ["2026-01-31"])
    }

    func testUpcomingAndSummaryCountTheNext30Days() {
        let schedules: [JSON] = [
            ["id": "rent", "nextDate": "2026-11-01", "paused": false, "transaction": ["amount": -150_000]],
            ["id": "salary", "nextDate": "2026-10-25", "paused": false, "transaction": ["amount": 400_000]],
            ["id": "gym", "nextDate": "2026-10-22", "paused": true, "transaction": ["amount": -5_000]],
            ["id": "later", "nextDate": "2027-01-01", "paused": false, "transaction": ["amount": -1_000]],
            ["id": "done", "nextDate": .null, "paused": false, "transaction": ["amount": -1]],
        ]
        let upcoming = ScheduleMath.upcoming(schedules)
        XCTAssertEqual(upcoming.map { $0.text("id") }, ["salary", "rent", "later", "gym"])
        let summary = ScheduleMath.summary(upcoming, today: "2026-10-21")
        XCTAssertEqual(summary, ScheduleSummary(active: 3, paused: 1, dueSoon: 2, goingOut: 150_000, comingIn: 400_000))
        XCTAssertEqual(ScheduleFrequency.label("every4Weeks"), "Every 4 weeks")
        XCTAssertEqual(ScheduleFrequency.label(""), "Once")
        XCTAssertFalse(ScheduleFrequency.repeats("never"))
    }
}

final class LoanMathTests: XCTestCase {
    override func setUp() {
        Money.configure(budget: ["currency": "USD"])
    }

    func testRatesReadAndWriteAsTyped() throws {
        XCTAssertEqual(LoanMath.rateText(5125), "5.125")
        XCTAssertEqual(LoanMath.rateText(5000), "5")
        XCTAssertEqual(LoanMath.rateText(5250), "5.25")
        XCTAssertEqual(LoanMath.rateText(0), "0")
        XCTAssertEqual(LoanMath.rateText(5125, decimal: ","), "5,125")
        XCTAssertEqual(try LoanMath.parseRateChange("5.125"), 5125)
        XCTAssertEqual(try LoanMath.parseRateChange("  4.5 % "), 4500)
        XCTAssertEqual(try LoanMath.parseRateChange("5,5", decimal: ","), 5500)
        XCTAssertEqual(try LoanMath.parseRateChange("1000"), 1_000_000)
        for bad in ["1000.001", "abc", "5.1234", "-1", "", "99999999999999999999"] {
            XCTAssertThrowsError(try LoanMath.parseRateChange(bad), bad)
        }
        XCTAssertEqual(try LoanMath.parseRate("5.25"), 525)
        XCTAssertEqual(try LoanMath.parseRate("6 %"), 600)
        XCTAssertEqual(try LoanMath.parseRate(""), 0)
        XCTAssertThrowsError(try LoanMath.parseRate("1001"))
        XCTAssertThrowsError(try LoanMath.parseRate("five"))
        let terms: JSON = ["annualRateBasisPoints": 500, "rateHistory": [["month": "2027-01", "annualRateMilliPercent": 4250], ["month": "2026-06", "annualRateMilliPercent": 5500]]]
        XCTAssertEqual(LoanMath.rateIn(terms, month: "2026-05"), 5000)
        XCTAssertEqual(LoanMath.rateIn(terms, month: "2026-10"), 5500)
        XCTAssertEqual(LoanMath.rateIn(terms, month: "2027-03"), 4250)
    }

    func testSpansAndPlannerAmounts() {
        XCTAssertEqual(LoanMath.span(0), "0 months")
        XCTAssertEqual(LoanMath.span(1), "1 month")
        XCTAssertEqual(LoanMath.span(12), "1 year")
        XCTAssertEqual(LoanMath.span(27), "2 years 3 months")
        XCTAssertEqual(LoanMath.plannerAmount(""), 0)
        XCTAssertEqual(LoanMath.plannerAmount("25.50"), 2550)
        XCTAssertEqual(LoanMath.plannerAmount("-5"), 0)
        XCTAssertNil(LoanMath.plannerAmount("lots"))
    }

    func testDraftsRoundTripAndKeepUnknownFields() throws {
        let saved: JSON = ["accountId": "loan", "categoryId": "c", "annualRateBasisPoints": 525, "monthlyPayment": 50_000, "monthlyFees": 0,
                           "rateHistory": [["month": "2027-01", "annualRateMilliPercent": 4875, "note": "kept"]], "futureField": 1]
        let draft = LoanDraft(saved: saved)
        XCTAssertEqual(draft.rate, "5.25")
        XCTAssertEqual(draft.payment, "500.00")
        XCTAssertEqual(draft.fees, "")
        XCTAssertEqual(draft.rates.map(\.rate), ["4.875"])
        let terms = try draft.terms(accountId: "loan", saved: saved, requireCategory: true)
        XCTAssertEqual(terms, saved.merging(["monthlyFees": 0]))

        var changed = draft
        changed.rates.append(LoanRateDraft(month: "2027-01", rate: "4"))
        XCTAssertThrowsError(try changed.terms(accountId: "loan", saved: saved, requireCategory: false))
        changed = draft
        changed.payment = ""
        XCTAssertThrowsError(try changed.terms(accountId: "loan", saved: saved, requireCategory: false))
        changed = draft
        changed.category = ""
        XCTAssertNoThrow(try changed.terms(accountId: "loan", saved: saved, requireCategory: false))
        XCTAssertThrowsError(try changed.terms(accountId: "loan", saved: saved, requireCategory: true))
        XCTAssertNotEqual(changed, draft)
    }

    func testLoanAccountsPaymentsAndSources() {
        let car: JSON = ["id": "car", "kind": "tracking", "accountType": "autoLoan", "openingBalance": -100_000]
        let house: JSON = ["id": "house", "kind": "tracking", "openingBalance": 300_000]
        let card: JSON = ["id": "card", "kind": "credit", "openingBalance": -5_000]
        XCTAssertTrue(LoanMath.hasLoan(car, balance: -100_000, saved: []))
        XCTAssertFalse(LoanMath.hasLoan(house, balance: 300_000, saved: []))
        XCTAssertFalse(LoanMath.hasLoan(card, balance: -5_000, saved: []))
        XCTAssertTrue(LoanMath.hasLoan(card, balance: -5_000, saved: ["card"]))

        let payment: JSON = ["id": "p", "date": "2026-10-03", "accountId": "checking", "amount": -20_000, "transferAccountId": "car", "splits": []]
        let budget: JSON = ["transactions": [payment, ["id": "x", "date": "2026-10-04", "accountId": "checking", "amount": -100, "splits": []]]]
        XCTAssertTrue(LoanMath.paysInto(payment, "car"))
        XCTAssertEqual(LoanMath.nextPayment(budget, accountId: "car", today: "2026-10-21"), "2026-11-01")
        XCTAssertEqual(LoanMath.nextPayment(budget, accountId: "car", today: "2026-10-02"), "2026-10-02")
        XCTAssertEqual(LoanMath.paymentsWithoutInterest(budget, accountId: "car", today: "2026-10-21").map { $0.text("id") }, ["p"])
        var charged = budget
        charged["transactions"] = .array(budget["transactions"].array + [["id": "loan-interest:car:2026-10", "date": "2026-10-03", "accountId": "car", "amount": -1_000]])
        XCTAssertTrue(LoanMath.paymentsWithoutInterest(charged, accountId: "car", today: "2026-10-21").isEmpty)

        let checking: JSON = ["id": "checking", "kind": "cash", "accountType": "checking"]
        let savings: JSON = ["id": "savings", "kind": "cash", "accountType": "savings", "name": "Savings"]
        let spending: JSON = ["id": "spending", "kind": "cash"]
        XCTAssertEqual(LoanMath.defaultSource(budget, loanId: "car", sources: [savings, checking], balances: [:]), "checking")
        // An account without a type reads as checking, so it comes before savings with more in it.
        XCTAssertEqual(LoanMath.defaultSource(["transactions": []], loanId: "car", sources: [savings, spending], balances: ["savings": 9, "spending": 5]), "spending")
        XCTAssertEqual(LoanMath.defaultSource(["transactions": []], loanId: "car", sources: [savings, savings.with("id", "more")], balances: ["savings": 9, "more": 50]), "more")
        XCTAssertEqual(PlanningCardMath.defaultSource(["transactions": []], cardId: "card", sources: [savings, spending], balances: ["savings": 9, "spending": 5]), "spending")
        let paidCard: JSON = ["transactions": [["date": "2026-09-01", "accountId": "savings", "amount": -10, "transferAccountId": "card"]]]
        XCTAssertEqual(PlanningCardMath.defaultSource(paidCard, cardId: "card", sources: [checking, savings], balances: [:]), "savings")
    }
}

final class PayeePlanningTests: XCTestCase {
    func testPayeeDetailUsageAndDifferences() {
        let budget: JSON = [
            "transactions": [
                ["id": "a", "date": "2026-10-01", "payee": "Grocer", "accountId": "checking", "amount": -1_000, "splits": [["categoryId": "food", "amount": -1_000]]],
                ["id": "b", "date": "2026-10-05", "payee": "Grocer", "accountId": "checking", "amount": -500, "splits": [["categoryId": "food", "amount": -250], ["categoryId": "home", "amount": -250]]],
                ["id": "c", "date": "2026-10-03", "payee": "Other", "accountId": "checking", "amount": -1, "splits": [["payee": "Grocer", "amount": -1]]],
            ],
            "schedules": [["id": "s", "transaction": ["payee": "Grocer", "splits": []]]],
            "payeeRules": [["id": "r", "match": "GROC", "payee": "Grocer"]],
        ]
        let detail = PayeeDetail(budget: budget, name: "Grocer")
        XCTAssertEqual(detail.count, 2)
        XCTAssertEqual(detail.total, -1_500)
        XCTAssertEqual(detail.recent.map(\.id), ["b", "a"])
        XCTAssertEqual(detail.recent.first?.splits, 2)
        let usage = PayeeUsage(budget: budget, name: "Grocer")
        XCTAssertEqual([usage.transactions, usage.schedules, usage.rules], [3, 1, 1])
        XCTAssertEqual(usage.description, "3 transactions, 1 schedule, 1 rule")
        let before: [JSON] = [["id": "a", "payee": "AMZN", "splits": [["categoryId": .null]]], ["id": "b", "payee": "Cafe", "splits": []]]
        let after: [JSON] = [["id": "a", "payee": "Amazon", "splits": [["categoryId": .null]]], ["id": "b", "payee": "Cafe", "splits": []]]
        XCTAssertEqual(PayeeRulePreview.differences(before: before, after: after), 1)
        XCTAssertEqual(PayeeRulePreview.differences(before: before, after: Array(after.reversed())), 1)
    }
}

/// The planning screens' engine requests, through the real Rust engine.
final class PlanningEngineTests: XCTestCase {
    private func budget() throws -> JSON {
        Money.configure(budget: ["currency": "USD"])
        var budget = try NativeCore.engine("sampleBudget", params: ["name": "Planning", "currency": "USD", "month": .string(Dates.currentMonth)])
        try budget.upsertRecord("accounts", ["id": "loan", "name": "Car loan", "kind": "tracking", "accountType": "autoLoan",
                                             "openingBalance": -1_000_000, "archived": false])
        return budget
    }

    func testProjectionsAcceptDraftTermsAndThePayoffSolverReturnsAnAmount() throws {
        var draft = LoanDraft()
        draft.rate = "12"
        draft.payment = "100"
        let terms = try draft.terms(accountId: "loan", saved: nil, requireCategory: false)
        let projection = try NativeCore.engine("loanProjection", params: ["balance": 100_000, "terms": terms, "start": "2026-01-31"])
        XCTAssertEqual(projection.rows("schedule").first?.long("interest"), 1_000)
        XCTAssertEqual(projection.rows("schedule").last?.long("balance"), 0)
        XCTAssertEqual(projection.text("payoffDate"), projection.rows("schedule").last?.text("date"))
        let faster = try NativeCore.engine("loanProjection", params: ["balance": 100_000, "terms": terms, "start": "2026-01-31", "extra": 5_000, "lump": 0])
        XCTAssertLessThan(faster.long("totalInterest"), projection.long("totalInterest"))
        draft.rate = "0"
        let free = try draft.terms(accountId: "loan", saved: nil, requireCategory: false)
        let needed = try NativeCore.engine("loanPaymentByDate", params: ["balance": 100_000, "terms": free, "start": "2026-01-01", "due": "2026-10-01", "lump": 0])
        XCTAssertEqual(needed.isObject ? needed.long("items") : needed.int, 10_000)
    }

    func testInterestIsAddedForAPaymentEnteredBeforeTheLoanHadDetails() throws {
        var budget = try budget()
        let today = Dates.today
        let typed: JSON = ["id": "typed", "date": .string(today), "accountId": "sample-checking", "payee": "", "memo": "", "amount": -5_000,
                           "status": "uncleared", "transferAccountId": "loan", "transferStatus": "uncleared",
                           "splits": [["categoryId": "sample-transport", "amount": -5_000]]]
        budget = try NativeCore.engine("saveTransaction", budget: budget, params: ["transaction": typed])
        try budget.upsertRecord("loans", ["accountId": "loan", "categoryId": "sample-transport", "annualRateBasisPoints": 1200,
                                          "monthlyPayment": 20_000, "monthlyFees": 0, "rateHistory": []])
        XCTAssertEqual(LoanMath.paymentsWithoutInterest(budget, accountId: "loan", today: today).map { $0.text("id") }, ["typed"])
        budget = try NativeCore.engine("applyLoanPayments", budget: budget, params: ["ids": ["typed"]])
        XCTAssertTrue(LoanMath.paymentsWithoutInterest(budget, accountId: "loan", today: today).isEmpty)
        XCTAssertEqual(budget.row("transactions", id: "loan-interest:loan:\(today.prefix(7))")?.long("amount"), -10_000)

        // Recording the next payment adds no second interest charge for the month.
        let terms = try XCTUnwrap(budget.rows("loans").first)
        budget = try NativeCore.engine("loanRecord", budget: budget, params: ["terms": terms, "from": "sample-checking", "amount": 20_000,
                                                                               "date": .string(today), "id": "recorded"])
        XCTAssertNotNil(budget.row("transactions", id: "recorded"))
        XCTAssertEqual(budget.rows("transactions").count { $0.text("id").hasPrefix("loan-interest:") }, 1)
    }

    func testRulePreviewsCountWhatReapplyingChanges() throws {
        var budget = try budget()
        for (id, payee) in [("t1", "AMZN Mktp US*2K4"), ("t2", "Corner Cafe")] {
            budget = try NativeCore.engine("saveTransaction", budget: budget, params: ["transaction": [
                "id": .string(id), "date": .string(Dates.today), "accountId": "sample-checking", "payee": .string(payee), "memo": "", "amount": -1_000,
                "status": "uncleared", "transferAccountId": .null, "transferStatus": .null, "splits": [["categoryId": .null, "amount": -1_000]]]])
        }
        let rule: JSON = ["id": "r", "match": "amzn", "payee": "Amazon", "categoryId": "sample-groceries"]
        XCTAssertEqual(try PayeeRulePreview.changes(budget: budget), 0)
        XCTAssertEqual(try PayeeRulePreview.changes(budget: budget, rules: [rule]), 1)
        XCTAssertEqual(try PayeeRulePreview.changes(budget: budget, rules: [rule], only: ["other"]), 0)
        budget["payeeRules"] = [rule]
        let applied = try NativeCore.engine("workflow", budget: budget, params: ["action": ["type": "reapplyPayeeRules"]])
        XCTAssertEqual(applied.row("transactions", id: "t1")?.text("payee"), "Amazon")
        XCTAssertEqual(try PayeeRulePreview.changes(budget: applied), 0)
    }
}

private extension JSON {
    func merging(_ fields: JSON) -> JSON {
        var copy = self
        for (key, value) in fields.object { copy[key] = value }
        return copy
    }

    func with(_ key: String, _ value: JSON) -> JSON {
        var copy = self
        copy[key] = value
        return copy
    }
}

final class LoanPaymentTests: XCTestCase {
    func testAPaymentsCategoryBecomesTheLoansWhenItHadNone() {
        let budget: JSON = ["categories": [["id": "debt", "name": "Car loan"]]]
        XCTAssertTrue(LoanPayment.remembersCategory(terms: ["accountId": "loan", "categoryId": ""], budget: budget))
        XCTAssertTrue(LoanPayment.remembersCategory(terms: ["accountId": "loan"], budget: budget))
        XCTAssertTrue(LoanPayment.remembersCategory(terms: ["accountId": "loan", "categoryId": "deleted"], budget: budget), "Its category is gone")
        XCTAssertFalse(LoanPayment.remembersCategory(terms: ["accountId": "loan", "categoryId": "debt"], budget: budget))
    }

    func testAPaymentStillToComeSaysItsDay() {
        XCTAssertEqual(LoanPayment.recorded("500.00", to: "Car loan", on: "2026-11-11", today: "2026-11-11"), "Recorded a payment of 500.00 to Car loan.")
        XCTAssertEqual(LoanPayment.recorded("500.00", to: "Car loan", on: "2026-10-01", today: "2026-11-11"), "Recorded a payment of 500.00 to Car loan.")
        XCTAssertEqual(LoanPayment.recorded("500.00", to: "Car loan", on: "2026-12-01", today: "2026-11-11"),
                       "Recorded a payment of 500.00 to Car loan for \(Money.date("2026-12-01")).")
    }

    /// A first payment keeps its category on the loan, in the one change the payment is.
    func testAFirstPaymentKeepsItsCategoryInOneChange() async throws {
        let repository = try await TestLibrary.repository()
        var snapshot = try await repository.create(name: "Loan payment", currency: "USD", sample: true)
        let category = try XCTUnwrap(snapshot.budget?.rows("categories").first { $0.text("name") == "Groceries" }).text("id")
        let terms: JSON = ["accountId": "car", "categoryId": "", "annualRateBasisPoints": 500, "monthlyPayment": 50_000, "monthlyFees": 0, "rateHistory": []]
        snapshot = try await repository.edit { budget in
            try budget.upsertRecord("accounts", ["id": "car", "name": "Car loan", "kind": "tracking", "accountType": "autoLoan", "openingBalance": -1_000_000, "archived": false])
            try budget.upsertRecord("loans", terms)
        }
        XCTAssertTrue(LoanPayment.remembersCategory(terms: terms, budget: try XCTUnwrap(snapshot.budget)))
        var chosen = terms
        chosen["categoryId"] = .string(category)
        let before = snapshot
        snapshot = try await repository.edit({ try $0.upsertRecord("loans", ["accountId": "car", "categoryId": .string(category)]) },
                                             then: [("loanRecord", ["terms": chosen, "from": "sample-checking", "amount": 50_000, "date": .string(Dates.today), "id": "first-payment"])])
        XCTAssertEqual(snapshot.budget?.rows("loans").first { $0.text("accountId") == "car" }?.text("categoryId"), category)
        XCTAssertNotNil(snapshot.budget?.row("transactions", id: "first-payment"))
        snapshot = try await repository.undo()
        XCTAssertNil(snapshot.budget?.row("transactions", id: "first-payment"), "One undo takes the payment back")
        XCTAssertEqual(snapshot.budget?.rows("loans").first { $0.text("accountId") == "car" }?.text("categoryId"), "", "and the category with it")
        XCTAssertTrue(BudgetPatch.same(try XCTUnwrap(snapshot.budget), try XCTUnwrap(before.budget)))
    }
}
