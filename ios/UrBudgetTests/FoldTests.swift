import XCTest
@testable import UrBudget

/// The iPhone Duo's fold (UI/Fold.swift), after Android's foldables: what iOS reports, and what
/// the layout makes of it.
final class FoldTests: XCTestCase {
    /// The Duo's inner screen, upright and on its side, in points (2007 × 2853 pixels at 3×).
    private let book = CGSize(width: 951, height: 669)
    private let table = CGSize(width: 669, height: 951)

    private func crease(_ window: CGSize, across: Bool = false, width: CGFloat = 24, active: Bool = true) -> FoldRegion {
        across ? FoldRegion(frame: CGRect(x: 0, y: (window.height - width) / 2, width: window.width, height: width), active: active)
               : FoldRegion(frame: CGRect(x: (window.width - width) / 2, y: 0, width: width, height: window.height), active: active)
    }

    func testAFoldSplitsTheWindowOnlyWhileHalfOpen() {
        // Half open like a book: the fold runs top to bottom, and the window has two sides.
        let half = FoldState.resolve(window: book, regions: [crease(book)], posture: .halfOpen)
        XCTAssertTrue(half.sideBySide)
        XCTAssertFalse(half.stacked)
        XCTAssertEqual(half.hinge, HingeSpan(start: 463.5, width: 24))
        XCTAssertFalse(half.docksTabs, "A real hinge keeps the system's tab bar, which iOS stands clear of the fold")
        var pretended = half
        pretended.pretended = true
        XCTAssertTrue(pretended.docksTabs)
        XCTAssertEqual(half.hinge?.end, 487.5)
        XCTAssertNil(half.tableTop)

        // Open flat it's one wide window, whatever region is kept for the crease; shut, the cover screen.
        XCTAssertNil(FoldState.resolve(window: book, regions: [crease(book)], posture: .open).division)
        XCTAssertNil(FoldState.resolve(window: CGSize(width: 466, height: 678), regions: [crease(book)], posture: .closed).division)
        XCTAssertEqual(FoldState.resolve(window: book, regions: [crease(book)], posture: .open).posture, .open)
        // A region that isn't active isn't splitting anything; nor is there a fold with no region at all.
        XCTAssertNil(FoldState.resolve(window: book, regions: [crease(book, active: false)], posture: .halfOpen).division)
        XCTAssertNil(FoldState.resolve(window: book, regions: [], posture: .halfOpen).division)
        // An ordinary iPhone or iPad says nothing of a hinge and has no region.
        XCTAssertEqual(FoldState.resolve(window: CGSize(width: 402, height: 874), regions: [], posture: .none), FoldState(window: CGSize(width: 402, height: 874)))
        // Before the window is measured there's nothing to split.
        XCTAssertNil(FoldState.resolve(window: .zero, regions: [crease(book)], posture: .halfOpen).division)
    }

    func testAFoldRunningAcrossIsATable() {
        let standing = FoldState.resolve(window: table, regions: [crease(table, across: true)], posture: .halfOpen)
        XCTAssertTrue(standing.stacked)
        XCTAssertFalse(standing.sideBySide)
        XCTAssertNil(standing.hinge, "Nothing sits side by side across this fold")
        XCTAssertEqual(standing.tableTop, 487.5)
        XCTAssertEqual(standing.flatHalf, 463.5)
        // A sheet stands on the flat half, clear of the fold, above the home indicator.
        XCTAssertEqual(FoldState.sheetHeight(flatHalf: standing.flatHalf, bottomInset: 21), 434.5)
        XCTAssertNil(FoldState.sheetHeight(flatHalf: nil, bottomInset: 21), "No table, a sheet's own heights")
        XCTAssertNil(FoldState.sheetHeight(flatHalf: 180, bottomInset: 21), "Too short a half to hold a sheet")
    }

    func testARegionThatDoesntCrossTheWindowIsNoFold() {
        // Too near an edge to leave a side worth having.
        let edge = FoldRegion(frame: CGRect(x: 120, y: 0, width: 24, height: book.height), active: true)
        XCTAssertNil(FoldState.resolve(window: book, regions: [edge], posture: .halfOpen).division)
        // Too short to cross the window (a cutout, not a crease).
        let stub = FoldRegion(frame: CGRect(x: 463, y: 0, width: 24, height: 200), active: true)
        XCTAssertNil(FoldState.resolve(window: book, regions: [stub], posture: .halfOpen).division)
        // The first that does cross is the fold.
        XCTAssertEqual(FoldState.resolve(window: book, regions: [edge, stub, crease(book)], posture: .halfOpen).hinge?.start, 463.5)
    }

    func testAListEndsAtTheFoldAndWhatsOpenedTakesTheFarSide() {
        let half = FoldState.resolve(window: book, regions: [crease(book)], posture: .halfOpen)
        for pane in [WideLayout.Pane.register, .reports, .budget] {
            XCTAssertEqual(WideLayout.split(pane, span: WindowSpan(width: book.width), fold: half), WideLayout.Split(list: 463.5, gap: 24, hinge: true))
        }
        // Open flat the same window splits by its width, as a wide window does.
        // (No crease is told here: with one, a list's edge near it moves onto it, as the Duo's test has.)
        let flat = FoldState(window: book, posture: .open)
        XCTAssertEqual(WideLayout.split(.register, span: WindowSpan(width: book.width), fold: flat), WideLayout.Split(list: WideLayout.registerWidth(951)))
        XCTAssertEqual(WideLayout.split(.reports, span: WindowSpan(width: book.width), fold: flat), WideLayout.Split(list: WideLayout.reportsListWidth(951)))
        XCTAssertEqual(WideLayout.split(.budget, span: WindowSpan(width: book.width), fold: flat)?.list, 951 - WideLayout.categoryWidth(951))
        XCTAssertEqual(WideLayout.split(.register, span: WindowSpan(width: book.width), fold: flat)?.hinge, false)
        // On a table the window is a phone's width: one thing at a time.
        let standing = FoldState.resolve(window: table, regions: [crease(table, across: true)], posture: .halfOpen)
        XCTAssertNil(WideLayout.split(.register, span: WindowSpan(width: table.width), fold: standing))
        // The cover screen, and any phone, shows one thing at a time.
        XCTAssertNil(WideLayout.split(.register, span: WindowSpan(width: 466), fold: FoldState(window: CGSize(width: 466, height: 678), posture: .closed)))
        XCTAssertNil(WideLayout.split(.budget, span: WindowSpan(width: 402), fold: FoldState()))
        // A screen the fold doesn't cross (the app beside the assistant) doesn't split at it.
        XCTAssertNil(WideLayout.split(.register, span: WindowSpan(width: 463.5), fold: half))
        XCTAssertNil(WideLayout.split(.register, span: WindowSpan(start: 500, width: 451), fold: half))
    }

    /// What the iPhone Duo Simulator reports half open (iOS 27.1, 11 November 2026): a region 40
    /// points wide down the middle of a 951 × 669 window, with the app's screens starting 62 points
    /// in from each end.
    func testTheDuoAsItsSimulatorReportsIt() {
        let window = CGSize(width: 951, height: 669)
        let reported = FoldRegion(frame: CGRect(x: 455, y: 0, width: 40, height: 669), active: true)
        let fold = FoldState.resolve(window: window, regions: [reported], posture: .halfOpen)
        XCTAssertEqual(fold.hinge, HingeSpan(start: 455, width: 40))
        // A screen inside the margins: its list runs from its own start to the fold.
        let screen = WindowSpan(start: 62, width: 827)
        XCTAssertEqual(WideLayout.split(.budget, span: screen, fold: fold), WideLayout.Split(list: 393, gap: 40, hinge: true))
        XCTAssertEqual(WideLayout.hinge(across: screen, fold: fold)?.after, 394)
        // The assistant takes what's past the fold.
        XCTAssertEqual(WideLayout.assistantSide(span: screen, fold: fold)?.width, 394)
        XCTAssertEqual(WideLayout.assistantSide(span: screen, fold: fold)?.gap, 40)
        // Lying flat the same region is reported, idle: one wide window, creased down its middle.
        let idle = FoldRegion(frame: reported.frame, active: false)
        let flat = FoldState.resolve(window: window, regions: [idle], posture: .open)
        XCTAssertNil(flat.division)
        XCTAssertEqual(flat.flatCrease, 475)
        XCTAssertNil(fold.flatCrease, "Half open it's a fold, not a crease to line up with")
        // As seen there: the app's screens are 868 points wide from the window's start.
        let wide = WindowSpan(start: 0, width: 868)
        XCTAssertEqual(WideLayout.registerWidth(868), 434)
        XCTAssertEqual(WideLayout.split(.register, span: wide, fold: flat), WideLayout.Split(list: 475), "The register's edge moves onto the crease")
        XCTAssertEqual(WideLayout.split(.budget, span: wide, fold: flat), WideLayout.Split(list: 475))
        XCTAssertEqual(WideLayout.split(.reports, span: wide, fold: flat), WideLayout.Split(list: WideLayout.reportsListWidth(868)), "The reports' list is nowhere near it")
        // The assistant takes the far half, where the window alone would be too narrow for both.
        XCTAssertNil(WideLayout.assistantWidth(868))
        XCTAssertEqual(WideLayout.assistantSide(span: wide, fold: flat)?.width, 393)
        XCTAssertEqual(WideLayout.assistantSide(span: wide, fold: flat)?.gap, 0)
        // Folded shut there's no crease on the cover screen.
        XCTAssertNil(FoldState.resolve(window: CGSize(width: 466, height: 678), regions: [idle], posture: .closed).crease)
        // A wide window with no crease keeps its own widths.
        XCTAssertEqual(WideLayout.creased(434, span: wide, fold: FoldState()), 434)
    }

    func testTheAssistantTakesTheFarSideOfAFold() {
        let half = FoldState.resolve(window: book, regions: [crease(book)], posture: .halfOpen)
        let side = WideLayout.assistantSide(span: WindowSpan(width: book.width), fold: half)
        XCTAssertEqual(side?.width, 463.5)
        XCTAssertEqual(side?.gap, 24)
        // Flat, by the window's width: the Duo's inner screen keeps the app its 600 points.
        let flat = WideLayout.assistantSide(span: WindowSpan(width: book.width), fold: FoldState(window: book, posture: .open))
        XCTAssertEqual(flat?.width, 351)
        XCTAssertEqual(flat?.gap, 0)
        // Folded, or on a table, there's no side for it: it's a sheet.
        XCTAssertNil(WideLayout.assistantSide(span: WindowSpan(width: 466), fold: FoldState(window: CGSize(width: 466, height: 678), posture: .closed)))
        XCTAssertNil(WideLayout.assistantSide(span: WindowSpan(width: table.width), fold: FoldState.resolve(window: table, regions: [crease(table, across: true)], posture: .halfOpen)))
    }

    /// `-fold book` and `-fold table` pretend a fold on any simulator; without them nothing is pretended.
    func testNothingIsPretendedUnlessAsked() {
        XCTAssertNil(FoldDebug.state(window: book))
        XCTAssertFalse(FoldDebug.shown)
    }

    @MainActor func testAnIPhoneStaysUprightUntilItsUnfolded() {
        guard UIDevice.current.userInterfaceIdiom == .phone else { return }
        FoldOrientation.follow(.none)
        XCTAssertEqual(FoldOrientation.mask, .portrait)
        FoldOrientation.follow(.closed)
        XCTAssertEqual(FoldOrientation.mask, .portrait, "The cover screen is a phone")
        FoldOrientation.follow(.halfOpen)
        XCTAssertEqual(FoldOrientation.mask, .all, "Half open it can stand on a table")
        FoldOrientation.follow(.open)
        XCTAssertEqual(FoldOrientation.mask, .all)
        FoldOrientation.follow(.closed)
        XCTAssertEqual(FoldOrientation.mask, .portrait)
    }
}
