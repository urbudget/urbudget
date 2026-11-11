import SwiftUI
import UIKit

/*
 * The iPhone Duo's fold, after Android's foldables (ui/Adaptive.kt). Folded, the cover screen is a
 * phone and nothing here applies. Open flat, the inner screen is one wide window, and a list and
 * what's opened from it share it by its width (WideLayout). Half open like a book, the fold is
 * never crossed: the start side is a phone of its own, with the tabs in a dock at its foot, and
 * what's opened (a category, a transaction, a report, the assistant) takes the far side. Half open
 * on a table, with the fold running across, what's typed on (the keypad, sheets) keeps to the flat
 * half below it.
 *
 * iOS 27.1 says two things. How far the hinge is open comes from `onHingeChange` (UIKit's
 * UIHingeInteraction). Where the fold runs comes from the window's reserved regions of the kind
 * `.division`, "an area where content should split into separate regions, such as at the fold of a
 * hinge". Both are read once, at the app's root (`FoldReader`), and every screen gets the result
 * as `\.fold`.
 */

/// How far the hinge is open, as iOS says it (UIHinge.Status). `none` is a device without one.
enum HingePosture: String, Equatable, Sendable {
    case none, closed, halfOpen, open
}

/// A region the window keeps for a fold (SwiftUI's ReservedRegion of the kind `.division`): its
/// frame in the window, measured from the start edge (the right in Arabic), margins included.
struct FoldRegion: Equatable, Sendable {
    var frame: CGRect
    var active: Bool
}

/// Where a top-to-bottom fold starts, measured from the window's start edge, and how wide it is.
struct HingeSpan: Equatable, Sendable {
    var start: CGFloat
    var width: CGFloat
    var end: CGFloat { start + width }
}

/// The fold as the window has it. `division` is where it runs, and only while it splits what's
/// shown in two: a Duo half open. Flat, folded, or on a device with no hinge, there's none.
struct FoldState: Equatable, Sendable {
    var division: CGRect? = nil
    /// Where the screen creases, while the phone is unfolded at all: the same place, whether it's
    /// splitting the window (half open) or lying flat under one wide window. Flat, a list and its
    /// detail meet there when they'd meet near it anyway.
    var crease: CGRect? = nil
    var window: CGSize = .zero
    var posture: HingePosture = .none
    /// The fold is pretended on a device with no hinge (`FoldDebug`). There the system's tab bar
    /// would lie across it, so the app's own dock stands in on the start side. On an iPhone Duo
    /// iOS keeps its tab bar at the window's far edge, clear of the fold, and it's left alone.
    var pretended = false

    /// Whether the tabs stand in the app's dock on the start side, in the system tab bar's place.
    var docksTabs: Bool { pretended && hinge != nil }

    /// The least either side of a fold keeps for it to count as splitting the window.
    static let leastSide: CGFloat = 280

    /// The fold runs top to bottom (a book); otherwise across (standing half open on a table).
    private var vertical: Bool { division.map { $0.height >= $0.width } ?? true }
    /// A fold running top to bottom splits the window into a start side and an end side.
    var sideBySide: Bool { division != nil && vertical }
    /// A fold running across: the top half to look at, the bottom half (on the table) to touch.
    var stacked: Bool { division != nil && !vertical }

    /// Flat, where a crease running top to bottom lies across the window, from its start edge.
    var flatCrease: CGFloat? {
        guard division == nil, let crease, crease.height >= crease.width else { return nil }
        return crease.midX
    }

    /// The fold that splits the window into two sides, from its start edge.
    var hinge: HingeSpan? {
        guard sideBySide, let division else { return nil }
        return HingeSpan(start: division.minX, width: division.width)
    }

    /// The top of the flat half while the phone stands half open on a table: the fold's foot.
    var tableTop: CGFloat? { stacked ? division?.maxY : nil }

    /// How tall the flat half is, for what's typed on to keep to it.
    var flatHalf: CGFloat? { tableTop.map { window.height - $0 } }

    /// How tall a sheet stands on the flat half: up to the fold and no further, less the home
    /// indicator's room (a sheet's height is counted above it) and a little air. Nil where there's
    /// no table posture, or the flat half is too short to hold a sheet.
    static func sheetHeight(flatHalf: CGFloat?, bottomInset: CGFloat) -> CGFloat? {
        guard let flatHalf else { return nil }
        let height = flatHalf - bottomInset - 8
        return height >= 200 ? height : nil
    }

    /// The fold from what iOS reports. A region counts while it's active, the hinge isn't flat or
    /// shut (flat, the inner screen is one window), and it really crosses the window with room on
    /// both sides of it.
    static func resolve(window: CGSize, regions: [FoldRegion], posture: HingePosture) -> FoldState {
        var state = FoldState(window: window, posture: posture)
        guard posture != .closed, window.width > 0, window.height > 0 else { return state }
        func crosses(_ frame: CGRect) -> Bool {
            let book = frame.height >= frame.width
            return book
                ? frame.height >= window.height * 0.9 && frame.minX >= leastSide && window.width - frame.maxX >= leastSide
                : frame.width >= window.width * 0.9 && frame.minY >= leastSide && window.height - frame.maxY >= leastSide
        }
        state.crease = regions.lazy.map(\.frame).first(where: crosses)
        if posture == .halfOpen || posture == .none {
            state.division = regions.lazy.filter(\.active).map(\.frame).first(where: crosses)
        }
        return state
    }
}

private struct FoldKey: EnvironmentKey {
    static let defaultValue = FoldState()
}

extension EnvironmentValues {
    /// The fold of the window this view is in. A part of the app laid out as a screen of its own
    /// (the app beside the assistant) is given the flat default, so it sees no fold.
    var fold: FoldState {
        get { self[FoldKey.self] }
        set { self[FoldKey.self] = newValue }
    }
}

// MARK: Reading the fold

/// What the root measures: the window, and the folds iOS keeps regions for in it.
private struct FoldProbe: Equatable, Sendable {
    var window: CGSize = .zero
    var regions: [FoldRegion] = []
}

private struct FoldProbeKey: PreferenceKey {
    static let defaultValue = FoldProbe()
    static func reduce(value: inout FoldProbe, nextValue: () -> FoldProbe) { value = nextValue() }
}

/// Follows the hinge and the window's fold and gives what's inside it `\.fold`. Before iOS 27.1,
/// and on anything without a hinge, that's the flat default.
struct FoldReader: ViewModifier {
    @State private var probe = FoldProbe()
    @State private var posture = HingePosture.none

    func body(content: Content) -> some View {
        let fold = FoldDebug.state(window: probe.window) ?? FoldState.resolve(window: probe.window, regions: probe.regions, posture: posture)
        content
            .environment(\.fold, fold)
            .background { reader }
            .overlay(alignment: .top) { if FoldDebug.shown { FoldDebug.readout(fold, probe.regions) } }
            .onChange(of: fold.posture, initial: true) { _, posture in FoldOrientation.follow(posture) }
    }

    @ViewBuilder private var reader: some View {
        if #available(iOS 27.1, *) {
            GeometryReader { proxy in
                // Read again whenever this is laid out or the hinge moves (which makes this view
                // again): whether a region is active can change with no change to the window's size.
                Color.clear.preference(key: FoldProbeKey.self, value: FoldProbe(
                    window: proxy.size,
                    regions: proxy.reservedRegions(kind: .division, options: .includeInactive).map { FoldRegion(frame: $0.frame, active: $0.isActive) }))
            }
            .ignoresSafeArea()
            .onPreferenceChange(FoldProbeKey.self) { probe = $0 }
            .onHingeChange { _, context in posture = HingePosture(context.hinge) }
        } else {
            GeometryReader { proxy in Color.clear.preference(key: FoldProbeKey.self, value: FoldProbe(window: proxy.size)) }
                .ignoresSafeArea()
                .onPreferenceChange(FoldProbeKey.self) { probe = $0 }
        }
    }
}

@available(iOS 27.1, *)
extension HingePosture {
    init(_ hinge: DeviceHinge?) {
        guard let hinge else { self = .none; return }
        switch hinge.status {
        case .closed: self = .closed
        case .partiallyOpen: self = .halfOpen
        case .fullyOpen: self = .open
        default: self = .none
        }
    }
}

/// An iPhone stays upright, as the app always has. Unfolded, a Duo turns with the hand, so it can
/// stand half open on a table with the fold running across. The app delegate asks this which
/// orientations the window takes.
@MainActor enum FoldOrientation {
    private(set) static var unfolded = false

    static var mask: UIInterfaceOrientationMask {
        UIDevice.current.userInterfaceIdiom == .pad || unfolded ? .all : .portrait
    }

    static func follow(_ posture: HingePosture) {
        let next = posture == .halfOpen || posture == .open
        guard next != unfolded else { return }
        unfolded = next
        for scene in UIApplication.shared.connectedScenes {
            for window in (scene as? UIWindowScene)?.windows ?? [] { window.rootViewController?.setNeedsUpdateOfSupportedInterfaceOrientations() }
        }
    }
}

// MARK: Trying it without the phone

/// Debug builds can pretend there's a fold, on any simulator: `-fold book` puts one down the
/// window's middle, `-fold table` across it, `-fold flat` and `-fold closed` say only where the
/// hinge stands. `-foldInfo` writes what's read over the top of the app.
enum FoldDebug {
    #if DEBUG
    private static let kind: String? = {
        let arguments = ProcessInfo.processInfo.arguments
        guard let at = arguments.firstIndex(of: "-fold"), at + 1 < arguments.count else { return nil }
        return arguments[at + 1]
    }()
    static let shown = ProcessInfo.processInfo.arguments.contains("-foldInfo")
    #else
    private static let kind: String? = nil
    static let shown = false
    #endif

    /// The width a pretended fold takes, margins and all (the Duo's own region is 40 points wide).
    static let width: CGFloat = 40

    static func state(window: CGSize) -> FoldState? {
        guard let kind, window.width > 0 else { return nil }
        switch kind {
        case "book":
            let frame = CGRect(x: (window.width - width) / 2, y: 0, width: width, height: window.height)
            var state = FoldState.resolve(window: window, regions: [FoldRegion(frame: frame, active: true)], posture: .halfOpen)
            state.pretended = true
            return state
        case "table":
            let frame = CGRect(x: 0, y: (window.height - width) / 2, width: window.width, height: width)
            var state = FoldState.resolve(window: window, regions: [FoldRegion(frame: frame, active: true)], posture: .halfOpen)
            state.pretended = true
            return state
        case "flat": return FoldState(window: window, posture: .open)
        case "closed": return FoldState(window: window, posture: .closed)
        default: return nil
        }
    }

    static func readout(_ fold: FoldState, _ regions: [FoldRegion]) -> some View {
        let seen = regions.map { "\(Int($0.frame.minX)),\(Int($0.frame.minY)) \(Int($0.frame.width))×\(Int($0.frame.height)) \($0.active ? "active" : "idle")" }.joined(separator: "; ")
        let split = fold.sideBySide ? "book" : fold.stacked ? "table" : "none"
        return Text(verbatim: "window \(Int(fold.window.width))×\(Int(fold.window.height)) · hinge \(fold.posture.rawValue) · fold \(split) · regions [\(seen)]")
            .font(.caption2.monospaced()).padding(6)
            .background(.yellow.opacity(0.85), in: RoundedRectangle(cornerRadius: 6))
            .foregroundStyle(.black)
            .padding(.top, 60)
            .allowsHitTesting(false)
            .accessibilityHidden(true)
    }
}

// MARK: What screens use

/// The far side of a fold with nothing open in it yet: what's opened comes here.
struct FoldSideHint: View {
    var body: some View {
        PaneHint(title: "This side is for what you open", detail: "A category, the assistant or a new transaction opens here, clear of the fold.",
                 symbol: "rectangle.righthalf.inset.filled")
    }
}

/// A sheet's heights. Standing half open on a table, a sheet keeps to the flat half below the
/// fold, whatever heights it takes on a phone.
private struct SheetDetents: ViewModifier {
    var detents: Set<PresentationDetent>
    @Environment(\.fold) private var fold

    func body(content: Content) -> some View {
        Group {
            if let height = FoldState.sheetHeight(flatHalf: fold.flatHalf, bottomInset: FoldWindow.bottomInset) {
                content.presentationDetents([.height(height)])
            } else {
                content.presentationDetents(detents)
            }
        }
        // A sheet at part height is glass on iOS 26 and later, and the screen under it shows
        // through as smears of colour behind its figures. A sheet here is a solid page.
        .presentationBackground(Color.groupedBackground)
        // A sheet doesn't take the window's "no scroll bars": it's said for each.
        .scrollIndicators(.hidden)
    }
}

/// The keypad of a form that fills the window, while the phone stands half open on a table: it
/// takes the whole flat half, its keys centred, like a calculator lying on the desk, and the form
/// stays above the fold. (A keypad in a sheet is on the flat half already, with its sheet.)
private struct OnFlatHalf: ViewModifier {
    var enabled: Bool
    @Environment(\.fold) private var fold

    func body(content: Content) -> some View {
        if enabled, let height = FoldState.sheetHeight(flatHalf: fold.flatHalf, bottomInset: FoldWindow.bottomInset - 8) {
            content.frame(maxWidth: .infinity).frame(height: height).background(Color(light: 0xD1D3D9, dark: 0x2B2B2E))
        } else {
            content
        }
    }
}

/// The home indicator's room at the window's foot, which a sheet's or a keypad's height doesn't count.
@MainActor enum FoldWindow {
    /// The window's width, or 0 before there is one.
    static var width: CGFloat {
        for scene in UIApplication.shared.connectedScenes {
            if let window = (scene as? UIWindowScene)?.keyWindow { return window.bounds.width }
        }
        return 0
    }

    static var bottomInset: CGFloat {
        for scene in UIApplication.shared.connectedScenes {
            if let window = (scene as? UIWindowScene)?.keyWindow { return window.safeAreaInsets.bottom }
        }
        return 0
    }
}

extension View {
    /// On a table, gives a full-window form's keypad the flat half below the fold.
    func onFlatHalf(enabled: Bool = true) -> some View { modifier(OnFlatHalf(enabled: enabled)) }

    /// `presentationDetents`, kept below the fold while the phone stands half open on a table.
    func sheetDetents(_ detents: Set<PresentationDetent>) -> some View { modifier(SheetDetents(detents: detents)) }
}
