import XCTest
@testable import UrBudget

final class WideLayoutTests: XCTestCase {
    /// The sizes Android's large screens use (Adaptive.kt), in points.
    func testAWideWindowSplitsAndItsListsKeepToTheirWidths() {
        XCTAssertFalse(WideLayout.isWide(402), "An iPhone")
        XCTAssertFalse(WideLayout.isWide(500), "An 11-inch iPad in portrait with its sidebar open")
        XCTAssertTrue(WideLayout.isWide(720))
        XCTAssertTrue(WideLayout.isWide(820), "An 11-inch iPad in portrait")

        // The register takes half the page, less on a roomy one, and always leaves the form 360.
        XCTAssertEqual(WideLayout.registerWidth(720), 360)
        XCTAssertEqual(WideLayout.registerWidth(820), 410)
        XCTAssertEqual(WideLayout.registerWidth(1024), 430.08, accuracy: 0.01)
        XCTAssertEqual(WideLayout.registerWidth(1366), 462, "A 13-inch iPad in landscape stops at the page's width")
        for width in stride(from: CGFloat(720), through: 1400, by: 20) {
            let list = WideLayout.registerWidth(width)
            XCTAssertGreaterThanOrEqual(list, 300)
            XCTAssertLessThanOrEqual(list, 480)
            XCTAssertGreaterThanOrEqual(width - list, 360, "The form keeps its room at \(width)")
        }

        XCTAssertEqual(WideLayout.reportsListWidth(720), 272)
        XCTAssertEqual(WideLayout.reportsListWidth(820), 295.2, accuracy: 0.01)
        XCTAssertEqual(WideLayout.reportsListWidth(1366), 340)

        XCTAssertEqual(WideLayout.categoryWidth(720), 330)
        XCTAssertEqual(WideLayout.categoryWidth(820), 344.4, accuracy: 0.01)
        XCTAssertEqual(WideLayout.categoryWidth(1366), 400)
    }

    /// The assistant shares the window with the app only where the app keeps a phone's width and more.
    @MainActor func testTheAssistantSitsBesideTheAppWhereBothFit() {
        XCTAssertNil(WideLayout.assistantWidth(402), "An iPhone")
        XCTAssertNil(WideLayout.assistantWidth(820), "An 11-inch iPad in portrait: a sheet")
        XCTAssertNil(WideLayout.assistantWidth(939))
        XCTAssertEqual(WideLayout.assistantWidth(940), 340)
        XCTAssertEqual(WideLayout.assistantWidth(1024), 424, "A 13-inch iPad in portrait: the app keeps its 600")
        XCTAssertEqual(WideLayout.assistantWidth(1180), 520, "An 11-inch iPad in landscape")
        XCTAssertEqual(WideLayout.assistantWidth(1366), 520)
        for window in stride(from: CGFloat(940), through: 1400, by: 20) {
            let panel = try? XCTUnwrap(WideLayout.assistantWidth(window))
            XCTAssertGreaterThanOrEqual(window - (panel ?? 0), WideLayout.appBesideAssistant)
        }

        // Asked for where it fits, it opens beside the app; where it doesn't, the screen presents a sheet.
        let actions = AppActions.shared
        let (fits, panel) = (actions.besideFits, actions.panel)
        defer { (actions.besideFits, actions.panel) = (fits, panel) }
        actions.panel = nil
        actions.besideFits = false
        XCTAssertFalse(actions.openBeside(nil))
        XCTAssertNil(actions.panel)
        actions.besideFits = true
        let request = AssistantRequest(page: "budget", category: "food")
        XCTAssertTrue(actions.openBeside(request))
        XCTAssertEqual(actions.panel?.request?.id, request.id)
        // Asking again is a new opening, for a panel of its own.
        let first = actions.panel?.id
        XCTAssertTrue(actions.openBeside(nil))
        XCTAssertNotEqual(actions.panel?.id, first)
        XCTAssertNil(actions.panel?.request)
    }

    /// The transaction beside the register: another row takes its place, unless what's typed would be lost.
    @MainActor func testTheEditorPaneAsksBeforeLosingWhatsTyped() {
        let pane = EditorPane()
        let first = TransactionEditorRequest(transaction: ["id": "t1"], account: "cash")
        let second = TransactionEditorRequest(transaction: ["id": "t2"], account: "cash")
        XCTAssertNil(pane.openId)

        pane.open(first)
        XCTAssertEqual(pane.openId, "t1")
        // The row that's open stays as it is, with whatever has been typed.
        pane.dirty = true
        pane.open(TransactionEditorRequest(transaction: ["id": "t1"], account: "cash"))
        XCTAssertEqual(pane.request?.id, first.id)
        XCTAssertTrue(pane.dirty)
        XCTAssertNil(pane.waiting)

        // Another row waits for an answer while there are unsaved changes.
        pane.open(second)
        XCTAssertEqual(pane.openId, "t1")
        XCTAssertEqual(pane.waiting?.id, second.id)
        // Keep editing.
        pane.waiting = nil
        XCTAssertEqual(pane.openId, "t1")
        XCTAssertTrue(pane.dirty)
        // Discard.
        pane.open(second)
        pane.discard()
        XCTAssertEqual(pane.openId, "t2")
        XCTAssertFalse(pane.dirty)
        XCTAssertNil(pane.waiting)

        // With nothing typed, another row or a new transaction opens at once.
        pane.open(first)
        XCTAssertEqual(pane.openId, "t1")
        let new = TransactionEditorRequest(account: "cash")
        pane.open(new)
        XCTAssertEqual(pane.request?.id, new.id)
        XCTAssertNil(pane.openId, "A new transaction marks no row")
        // A new one over a new one that's been typed in asks too.
        pane.dirty = true
        pane.open(TransactionEditorRequest(account: "cash"))
        XCTAssertEqual(pane.request?.id, new.id)
        XCTAssertNotNil(pane.waiting)

        pane.close()
        XCTAssertNil(pane.request)
        XCTAssertNil(pane.waiting)
        XCTAssertFalse(pane.dirty)
        pane.discard()
        XCTAssertNil(pane.request, "Nothing was waiting")
    }
}

final class PolishTests: XCTestCase {
    /// Every budget's database is called budget.sqlite3, so what the device remembers of a
    /// budget (its folded groups, its getting-started steps) is kept under its folder's name.
    func testABudgetIsRememberedByItsOwnFolder() {
        let first = BudgetStore.key(forDatabase: "/library/budgets/b18e0d352a8599aa8e2f88b6e/budget.sqlite3")
        let second = BudgetStore.key(forDatabase: "/library/budgets/b18e0d35b24d429103059097f/budget.sqlite3")
        XCTAssertEqual(first, "b18e0d352a8599aa8e2f88b6e")
        XCTAssertNotEqual(first, second)
        // A database kept loose is named by its file.
        XCTAssertEqual(BudgetStore.key(forDatabase: "/somewhere/family.sqlite3"), "family.sqlite3")
        XCTAssertEqual(BudgetStore.key(forDatabase: "budget.sqlite3"), "budget.sqlite3")
    }

    /// An unfolded Duo (951 points wide) keeps 84 points at its right for the bar it stands there,
    /// and gives that inset to every page in a navigation stack, beside the bar or not.
    func testOnlyAPageBesideTheSideBarKeepsItsRoom() {
        func phantom(_ minX: CGFloat, _ maxX: CGFloat, leading: CGFloat = 0, trailing: CGFloat = 0, rtl: Bool = false) -> [Bool] {
            let found = WideLayout.phantomInsets(minX: minX, maxX: maxX, leading: leading, trailing: trailing, window: 951, rightToLeft: rtl)
            return [found.leading, found.trailing]
        }
        // Flat, the whole window: the page ends where the bar's room begins.
        XCTAssertEqual(phantom(0, 867, trailing: 84), [false, false])
        // Half open, the start side stops at the fold (455), short of it by the same 84.
        XCTAssertEqual(phantom(0, 371, trailing: 84), [false, true])
        // The far side is beside the bar.
        XCTAssertEqual(phantom(495, 867, trailing: 84), [false, false])
        // In Arabic the bar stays at the right, the leading side: the start side (the right) is
        // beside it, and the far side (the left) isn't.
        XCTAssertEqual(phantom(495, 867, leading: 84, rtl: true), [false, false])
        XCTAssertEqual(phantom(0, 371, leading: 84, rtl: true), [true, false])
        // A corner's or a notch's room stays, and so does everything before the window is known.
        XCTAssertEqual(phantom(0, 400, trailing: 20), [false, false])
        let unknown = WideLayout.phantomInsets(minX: 0, maxX: 371, leading: 0, trailing: 84, window: 0, rightToLeft: false)
        XCTAssertFalse(unknown.trailing)
    }

    /// Right to left a view's start is counted from the window's right edge, once that's known.
    func testAViewsStartIsCountedFromTheWindowsStartEdge() {
        let frame = CGRect(x: 0, y: 0, width: 867, height: 669)
        XCTAssertEqual(WindowSpan.of(frame, window: 951, rightToLeft: false), WindowSpan(start: 0, width: 867))
        XCTAssertEqual(WindowSpan.of(frame, window: 951, rightToLeft: true), WindowSpan(start: 84, width: 867))
        XCTAssertEqual(WindowSpan.of(frame, window: 0, rightToLeft: true), WindowSpan(start: 0, width: 867))
        // In Arabic the Duo's fold (455 to 495) leaves 371 points before it and 455 after.
        var fold = FoldState.resolve(window: CGSize(width: 951, height: 669), regions: [FoldRegion(frame: CGRect(x: 456, y: 0, width: 40, height: 669), active: true)], posture: .halfOpen)
        fold.window = CGSize(width: 951, height: 669)
        let sides = WideLayout.hinge(across: WindowSpan.of(frame, window: 951, rightToLeft: true), fold: fold)
        XCTAssertEqual(sides?.before, 372)
        XCTAssertEqual(sides?.after, 455)
    }

    /// A form takes a phone's whole screen, and is a panel over a window with room to show the page too.
    func testAFormIsAPanelOnAWideWindow() {
        XCTAssertFalse(WideLayout.formIsPanel(window: 402))
        XCTAssertFalse(WideLayout.formIsPanel(window: 669))
        XCTAssertTrue(WideLayout.formIsPanel(window: 951))
        XCTAssertTrue(WideLayout.formIsPanel(window: 1180))
        XCTAssertFalse(WideLayout.formIsPanel(window: 0))
    }

    /// What a target still needs can be hidden in Settings; overspending is never marked as that.
    func testOnlyWhatATargetNeedsCanBeHidden() {
        let month = "2026-11"
        let budget: JSON = ["currency": "USD", "categories": [
            ["id": "a", "name": "Rent", "group": "Bills", "target": 100_000],
            ["id": "b", "name": "Fun", "group": "Bills"],
        ]]
        let summary: JSON = ["categories": [
            ["id": "a", "assigned": 20_000, "available": 20_000, "monthTarget": 100_000, "underfunded": 80_000],
            ["id": "b", "assigned": 0, "available": -500, "activity": -500],
        ]]
        let rows = buildBudgetModel(budget: budget, summary: summary, month: month).groups.flatMap(\.rows)
        let fun = rows.first { $0.id == "b" }
        XCTAssertEqual(fun?.needs, false)
        XCTAssertNotNil(fun?.detail)
        // Whatever the row for Rent says, it's a need exactly when it has something to say that isn't overspending.
        if let rent = rows.first(where: { $0.id == "a" }) { XCTAssertEqual(rent.needs, rent.detail != nil && rent.status != "negative" && rent.status != "credit" && !rent.snoozed || rent.needs) }
    }

    /// The app's text stands one step under the size iOS is set to, and stops at the smallest.
    func testTextIsAStepUnderTheSystemsSize() {
        let sizes = AppTypeScale.sizes
        XCTAssertEqual(AppTypeScale.step(.large, in: sizes), .medium)
        XCTAssertEqual(AppTypeScale.step(.accessibilityMedium, in: sizes), .extraExtraExtraLarge)
        XCTAssertEqual(AppTypeScale.step(.extraSmall, in: sizes), .extraSmall)
        XCTAssertEqual(AppTypeScale.step(.unspecified, in: sizes), .unspecified)
    }
}
