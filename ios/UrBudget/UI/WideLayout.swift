import SwiftUI

/// A wide window (an iPad, mostly) shows a list and what's opened from it side by side, where a
/// phone shows one over the other: a report beside the reports' list, a transaction beside the
/// register, a category beside the budget. The sizes follow Android's large screens (Adaptive.kt).
/// It goes by the room a screen really has, not the size class: an iPad's sidebar or a narrow Split
/// View window takes room away while the class stays regular.
enum WideLayout {
    /// A screen at least this wide shows both side by side.
    static let split: CGFloat = 720
    /// Beyond this a split page stops growing; the rest is margin.
    static let page: CGFloat = 1100
    /// The room the iPad's floating tab bar takes at the top of the window.
    static let topTabs: CGFloat = 54

    static func isWide(_ width: CGFloat) -> Bool { width >= split }

    private static func clamp(_ value: CGFloat, _ low: CGFloat, _ high: CGFloat) -> CGFloat { min(max(value, low), max(low, high)) }

    /// The register beside an open transaction: half the page, less on a roomy one, and always
    /// leaving the form 360 points.
    static func registerWidth(_ width: CGFloat) -> CGFloat {
        let page = min(width, self.page)
        return clamp(page * (page >= 960 ? 0.42 : 0.5), 300, min(480, page - 360))
    }

    /// The list of reports beside the one that's open.
    static func reportsListWidth(_ width: CGFloat) -> CGFloat { clamp(min(width, page) * 0.36, 272, 340) }

    /// A category beside the budget.
    static func categoryWidth(_ width: CGFloat) -> CGFloat { clamp(min(width, page) * 0.42, 330, 400) }

    /// How a screen shares its width between its list and what's opened from it: the list's
    /// width, and the gap before the detail, which is a fold's own width where one runs between.
    struct Split: Equatable, Sendable {
        var list: CGFloat
        var gap: CGFloat = 0
        /// The two sides are the sides of a fold: the list ends where it runs.
        var hinge = false
    }

    /// The lists that show what's opened from them alongside.
    enum Pane { case register, reports, budget }

    /// The least a screen keeps on each side of a fold for the fold to split it. (An iPhone Duo's
    /// sides are each about 390 points once the camera's and the corners' margins are taken off.)
    static let leastFoldSide: CGFloat = 240

    /// The fold where it crosses a screen lying at `span` in the window, or nil where it doesn't
    /// (the app beside the assistant, a screen beside an open sidebar): what's left of the screen
    /// before it, and its own width.
    static func hinge(across span: WindowSpan, fold: FoldState) -> (before: CGFloat, width: CGFloat, after: CGFloat)? {
        guard let hinge = fold.hinge else { return nil }
        let before = hinge.start - span.start, after = span.end - hinge.end
        guard before >= leastFoldSide, after >= leastFoldSide else { return nil }
        return (before, hinge.width, after)
    }

    /// The split for a screen lying at `span` in the window, or nil while it shows one thing at a
    /// time. A fold that crosses the screen decides it, whatever the width: the list ends at the
    /// fold and what's opened takes the far side. The fold is in the window's own measure and a
    /// screen often starts in from the window's edge (the Duo's inner screen keeps a margin at each
    /// end), so the list's width is what the screen has before the fold.
    static func split(_ pane: Pane, span: WindowSpan, fold: FoldState) -> Split? {
        if let hinge = hinge(across: span, fold: fold) { return Split(list: hinge.before, gap: hinge.width, hinge: true) }
        let width = span.width
        guard isWide(width) else { return nil }
        switch pane {
        case .register: return Split(list: creased(registerWidth(width), span: span, fold: fold))
        case .reports: return Split(list: reportsListWidth(width))
        case .budget: return Split(list: creased(width - categoryWidth(width), span: span, fold: fold))
        }
    }

    /// How near a list's edge has to come to the crease of a phone lying flat to be moved onto it.
    /// Which of a page's side insets are room kept for a bar the page isn't beside. `minX` and
    /// `maxX` are where the page lies in the window, left to right whatever the language; a side
    /// inset under 40 points is a corner's or a notch's, and stays.
    static func phantomInsets(minX: CGFloat, maxX: CGFloat, leading: CGFloat, trailing: CGFloat, window: CGFloat,
                              rightToLeft: Bool) -> (leading: Bool, trailing: Bool) {
        guard window > 0, maxX > minX else { return (false, false) }
        let left = rightToLeft ? trailing : leading, right = rightToLeft ? leading : trailing
        let leftPhantom = left >= 40 && minX - left > 1
        let rightPhantom = right >= 40 && maxX + right < window - 1
        return rightToLeft ? (leading: rightPhantom, trailing: leftPhantom) : (leading: leftPhantom, trailing: rightPhantom)
    }

    /// As wide as a list is let to grow on a wide window, where it becomes a column down the
    /// middle: Android's `ReadableWidth`, seen side by side on a Galaxy Z Fold 8 open flat.
    static let readable: CGFloat = 680

    /// A form opens as a panel over a window wide enough to show the page beside it; narrower,
    /// it takes the whole screen.
    static func formIsPanel(window: CGFloat) -> Bool { window >= split }

    static let creaseReach: CGFloat = 72

    /// A list's width on a phone lying flat: the line between it and its detail lands on the
    /// crease when it would land near it anyway, so the screen's own line and the app's are one.
    static func creased(_ list: CGFloat, span: WindowSpan, fold: FoldState) -> CGFloat {
        guard let crease = fold.flatCrease else { return list }
        let onCrease = crease - span.start
        return abs(onCrease - list) <= creaseReach && span.end - crease >= leastFoldSide ? onCrease : list
    }

    /// The assistant and the app as the two sides of a fold, or by the width they share: the
    /// assistant's width and the gap before it, or nil where there's no room for both.
    static func assistantSide(span: WindowSpan, fold: FoldState) -> (width: CGFloat, gap: CGFloat)? {
        if let hinge = hinge(across: span, fold: fold) { return (hinge.after, hinge.width) }
        // Flat, an unfolded phone's two halves: the app on one as a phone, the assistant on the other.
        if let crease = fold.flatCrease, crease - span.start >= leastFoldSide, span.end - crease >= leastFoldSide {
            return (span.end - crease, 0)
        }
        return assistantWidth(span.width).map { ($0, 0) }
    }

    /// The least the app keeps beside the assistant: a full phone's width and its tab bar.
    static let appBesideAssistant: CGFloat = 600

    /// The assistant's width beside the app, or nil where the window has no room for both (it
    /// opens as a sheet there): 46% of the window, between 340 and 520, never squeezing the app.
    static func assistantWidth(_ window: CGFloat) -> CGFloat? {
        let room = window - appBesideAssistant
        guard room >= 340 else { return nil }
        return min(clamp(window * 0.46, 340, 520), room)
    }
}

/// Where a view lies across the window: how far in from the window's start edge (the right in
/// Arabic) it begins, and how wide it is. A fold is told in the window's measure, so a screen
/// needs this to know where the fold crosses it.
struct WindowSpan: Equatable, Sendable {
    var start: CGFloat = 0
    var width: CGFloat = 0
    var end: CGFloat { start + width }

    /// Where a view lies from the window's start edge: its left, or right to left its right. (A
    /// screen that's given no fold, the app beside the assistant, isn't told the window's width
    /// and needs no start.)
    static func of(_ frame: CGRect, window: CGFloat, rightToLeft: Bool) -> WindowSpan {
        WindowSpan(start: rightToLeft && window > 0 ? window - frame.maxX : frame.minX, width: frame.width)
    }
}

private struct MeasuringSpan: ViewModifier {
    @Binding var span: WindowSpan
    @Environment(\.layoutDirection) private var direction
    @Environment(\.fold) private var fold

    @State private var frame = CGRect.zero

    func body(content: Content) -> some View {
        content
            .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frame = $0; measure() }
            // Right to left the start is counted from the window's right, so it's worked out again
            // when the window's width becomes known or changes, not only when this view moves.
            .onChange(of: fold.window.width) { measure() }
            .onChange(of: direction) { measure() }
    }

    private func measure() {
        span = WindowSpan.of(frame, window: fold.window.width, rightToLeft: direction == .rightToLeft)
    }
}

/// The transaction open beside a register on a wide window. A row tapped while a form with
/// unsaved changes is open waits for an answer, so nothing typed is lost without a word.
@MainActor @Observable
final class EditorPane {
    private(set) var request: TransactionEditorRequest?
    /// The open form has changes that aren't saved yet; the form says so as they're made.
    var dirty = false
    /// What was asked for while the open form had unsaved changes.
    var waiting: TransactionEditorRequest?

    /// The transaction the open form is editing, for the register to mark its row.
    var openId: String? { request?.transaction?.text("id") }

    func open(_ next: TransactionEditorRequest) {
        // The row that's already open stays as it is.
        if let id = next.transaction?.text("id"), id == openId { return }
        if request != nil && dirty { waiting = next } else { show(next) }
    }

    /// Lets go of the open form's changes and opens what was waiting.
    func discard() {
        if let waiting { show(waiting) }
    }

    func close() {
        request = nil
        dirty = false
        waiting = nil
    }

    private func show(_ next: TransactionEditorRequest) {
        request = next
        dirty = false
        waiting = nil
    }
}

private struct EditorPaneKey: EnvironmentKey {
    static let defaultValue: EditorPane? = nil
}

extension EnvironmentValues {
    /// Set where a transaction opens beside its list; nil where it opens over the screen.
    var editorPane: EditorPane? {
        get { self[EditorPaneKey.self] }
        set { self[EditorPaneKey.self] = newValue }
    }
}

private struct DockRoomKey: EnvironmentKey {
    static let defaultValue: CGFloat = 0
}

extension EnvironmentValues {
    /// The room the fold's dock takes at the foot of this screen (none where the tab bar is the system's).
    var dockRoom: CGFloat {
        get { self[DockRoomKey.self] }
        set { self[DockRoomKey.self] = newValue }
    }
}

private struct StartPaneKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// Whether this screen is the list of a split, with its detail between it and the window's end.
    var startPane: Bool {
        get { self[StartPaneKey.self] }
        set { self[StartPaneKey.self] = newValue }
    }
}

/// A list on the start side of a split keeps no room for what stands at the window's far edge.
/// On an iPhone Duo's inner screen iOS stands the tab bar there and keeps every screen in the tab
/// clear of it by its width, the list of a split included, though the list ends well short of it
/// (seen on the Simulator: 83 points left empty before the fold). The list's own screen gives the
/// room back.
private struct StartPaneContent: ViewModifier {
    @Environment(\.startPane) private var startPane

    func body(content: Content) -> some View {
        content.ignoresSafeArea(.container, edges: startPane ? .trailing : [])
    }
}

private struct WindowWidthKey: EnvironmentKey {
    static let defaultValue: CGFloat = 0
}

extension EnvironmentValues {
    /// The width of the whole app, tabs and sidebar included, for a screen to tell whether the
    /// sidebar is taking some of it.
    var windowWidth: CGFloat {
        get { self[WindowWidthKey.self] }
        set { self[WindowWidthKey.self] = newValue }
    }
}

/// On iPad (iOS 18 and later) the tab bar floats over the middle of the top bar. One screen's bar
/// leaves it room, but the bars of two panes side by side meet under it, where their buttons would
/// be covered. So a split screen starts below it. Where iOS has already kept the screen clear of
/// the tab bar (its top inset is more than the status bar's), or the tabs are open as a sidebar or
/// sit at the bottom, there's nothing to add.
private struct BelowTopTabs: ViewModifier {
    var active: Bool
    @Environment(\.windowWidth) private var window
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var width: CGFloat = 0
    @State private var inset: CGFloat = 0

    func body(content: Content) -> some View {
        content
            .padding(.top, room)
            .measuringWidth($width)
            .onGeometryChange(for: CGFloat.self) { $0.safeAreaInsets.top } action: { inset = $0 }
    }

    private var room: CGFloat {
        guard active, sizeClass == .regular, UIDevice.current.userInterfaceIdiom == .pad, window > 0, width >= window - 1,
              inset < WideLayout.topTabs else { return 0 }
        if #available(iOS 18.0, *) { return WideLayout.topTabs }
        return 0
    }
}

extension View {
    /// Keeps `width` at this view's own width.
    func measuringWidth(_ width: Binding<CGFloat>) -> some View {
        onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width.wrappedValue = $0 }
    }

    /// For the screen at the root of a list's navigation: on the start side of a split it runs to
    /// the split's edge (see `StartPaneContent`).
    func startPaneContent() -> some View { modifier(StartPaneContent()) }

    /// Keeps `span` at where this view lies across the window.
    func measuringSpan(_ span: Binding<WindowSpan>) -> some View { modifier(MeasuringSpan(span: span)) }

    /// Starts a split screen below the iPad's floating tab bar (see `BelowTopTabs`).
    func belowTopTabs(_ active: Bool) -> some View { modifier(BelowTopTabs(active: active)) }
}

/// What a pane says while nothing is open in it.
struct PaneHint: View {
    var title: LocalizedStringKey
    var detail: LocalizedStringKey
    var symbol: String

    var body: some View {
        // As Android's empty pane: the symbol in a soft disc of the accent, the words under it.
        VStack(spacing: 6) {
            Image(systemName: symbol).font(.system(size: 22, weight: .semibold)).foregroundStyle(Color.urAccent)
                .frame(width: 56, height: 56)
                .background(Color.urAccent.opacity(0.14), in: Circle())
                .padding(.bottom, 8)
                .accessibilityHidden(true)
            Text(title).font(.headline).multilineTextAlignment(.center)
            Text(detail).font(.subheadline).foregroundStyle(Color.secondary).multilineTextAlignment(.center)
        }
        .padding(.horizontal, 24)
        .frame(maxWidth: 360)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.groupedBackground)
        .accessibilityElement(children: .combine)
    }
}

/// The assistant, asked for from a screen: beside the app where the window has room for both, so
/// the screen it was asked from stays in use, else as a sheet over that screen.
private struct AssistantPresenter: ViewModifier {
    @Binding var request: AssistantRequest?

    func body(content: Content) -> some View {
        content
            .onChange(of: request?.id) { _, id in
                guard id != nil, let asked = request, AppActions.shared.openBeside(asked) else { return }
                request = nil
            }
            .sheet(item: Binding(get: { AppActions.shared.besideFits ? nil : request }, set: { request = $0 })) { AssistantView(request: $0).scrollIndicators(.hidden) }
    }
}

extension View {
    func assistant(_ request: Binding<AssistantRequest?>) -> some View { modifier(AssistantPresenter(request: request)) }
}

/// A list and what's opened from it. Split, they share the screen with a line between them, or
/// with a fold's own width where one runs between (nothing is drawn across it); otherwise the
/// list has the screen and what's opened shows over it, as on a phone. On the start side of a
/// fold the list keeps clear of the dock the tabs stand in there.
struct ListDetail<List: View, Detail: View>: View {
    var split: WideLayout.Split?
    @ViewBuilder var list: List
    @ViewBuilder var detail: Detail

    @Environment(\.fold) private var fold
    private var dock: CGFloat { split?.hinge == true && fold.docksTabs ? FoldDock.room : 0 }

    var body: some View {
        HStack(spacing: 0) {
            list
                // On the start side of a fold, lists end above the dock and the add button floats over it.
                .contentMargins(.bottom, dock, for: .scrollContent)
                .environment(\.dockRoom, dock)
                .environment(\.startPane, split != nil)
                .frame(width: split?.list)
            if let split {
                if split.hinge { FoldGap(width: split.gap) } else { Divider().ignoresSafeArea() }
                detail.frame(maxWidth: .infinity)
            }
        }
    }
}

/// What the far side of a split shows for a screen: the transaction form opened from it, else
/// whatever the screen keeps there.
struct FarSide<Resting: View>: View {
    var pane: EditorPane
    @ViewBuilder var resting: Resting

    var body: some View {
        Group {
            if let request = pane.request {
                TransactionEditor(request: request).id(request.id)
            } else {
                resting
            }
        }
        .environment(\.editorPane, pane)
    }
}

private struct EditorPaneGuard: ViewModifier {
    var pane: EditorPane
    /// Whether the pane is on screen; with none, a form it was showing carries on over the screen.
    var shown: Bool

    func body(content: Content) -> some View {
        content
            .cover(item: Binding(get: { shown ? nil : pane.request }, set: { if $0 == nil { pane.close() } })) { TransactionEditor(request: $0) }
            .confirmationDialog("Discard your changes?", isPresented: Binding(get: { pane.waiting != nil }, set: { if !$0 { pane.waiting = nil } }),
                                titleVisibility: .visible) {
                Button("Discard", role: .destructive) { pane.discard() }
                Button("Keep editing", role: .cancel) { pane.waiting = nil }
            } message: {
                Text("This transaction has changes that aren't saved.")
            }
    }
}

extension View {
    /// Asks before a form open in `pane` loses what's typed, and keeps the form over the screen
    /// when the pane goes (the window narrowed, the phone folded).
    func guarding(_ pane: EditorPane, shown: Bool) -> some View { modifier(EditorPaneGuard(pane: pane, shown: shown)) }
}

/// A screen with no detail of its own (Accounts, More) on a fold: it keeps to the start side, and
/// the far side waits for what's opened from it. A new transaction opens there.
struct FoldPage<Content: View>: View {
    @ViewBuilder var content: Content
    @Environment(\.fold) private var fold
    @State private var span = WindowSpan()
    @State private var pane = EditorPane()

    var body: some View {
        let onFold = WideLayout.split(.register, span: span, fold: fold)
        let split = onFold?.hinge == true ? onFold : nil
        ListDetail(split: split) {
            content.environment(\.editorPane, split == nil ? nil : pane)
        } detail: {
            FarSide(pane: pane) { FoldSideHint() }
        }
        .measuringSpan($span)
        .guarding(pane, shown: split != nil)
    }
}

/// The tabs on the start side of a pretended fold, where the system's tab bar would lie across it:
/// a dock at that side's foot, as Android's. (On an iPhone Duo iOS stands its own tab bar at the
/// window's far edge, clear of the fold.)
struct FoldDock: View {
    /// The room the dock takes at the foot of the start side.
    static let room: CGFloat = 68

    var tabs: [(tab: AppTab, title: String, symbol: String)]
    @Binding var selection: AppTab

    var body: some View {
        HStack(spacing: 0) {
            ForEach(tabs, id: \.tab) { item in
                let chosen = item.tab == selection
                Button { selection = item.tab } label: {
                    VStack(spacing: 3) {
                        Image(systemName: item.symbol).font(.system(size: 19, weight: .medium)).frame(height: 24)
                        Text(item.title).font(.caption2.weight(chosen ? .semibold : .regular)).lineLimit(1).minimumScaleFactor(0.7)
                    }
                    .foregroundStyle(chosen ? Color.urAccent : Color.primary)
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(item.title)
                .accessibilityAddTraits(chosen ? [.isSelected, .isButton] : .isButton)
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 3)
        .modifier(DockGlass())
        .padding(.horizontal, 12)
        .padding(.bottom, 6)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Tabs")
    }
}

/// The dock's glass. It holds buttons of its own, so the glass itself takes no touches (glass
/// that answers a finger, as the add button's does, swallowed every other tap on a tab).
private struct DockGlass: ViewModifier {
    func body(content: Content) -> some View {
        if #available(iOS 26.0, *) {
            content.glassEffect(.regular, in: Capsule())
        } else {
            content.background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(Color.white.opacity(0.22), lineWidth: 0.5))
                .shadow(color: .black.opacity(0.14), radius: 14, y: 5)
        }
    }
}

/// The strip a fold takes between the two sides: nothing is drawn or touched there, and it wears
/// the far side's ground, so the near side's list is seen to end at the fold.
struct FoldGap: View {
    var width: CGFloat

    var body: some View {
        Color.groupedBackground.frame(width: width).ignoresSafeArea().accessibilityHidden(true)
    }
}
