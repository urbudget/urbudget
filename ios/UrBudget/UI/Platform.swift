import SwiftUI
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

// The app is iOS-only; these few wrappers keep iOS-specific modifiers in one place, so the shared
// code also type-checks against the macOS SDK on machines without Xcode's iOS platform.

extension Color {
    /// A colour for each appearance, from sRGB hex.
    init(light: UInt32, dark: UInt32) {
        #if canImport(UIKit)
        self.init(uiColor: UIColor { $0.userInterfaceStyle == .dark ? UIColor(hex: dark) : UIColor(hex: light) })
        #else
        self.init(nsColor: NSColor(name: nil) { $0.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? NSColor(hex: dark) : NSColor(hex: light) })
        #endif
    }

    /// The page behind grouped lists.
    static var groupedBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .systemGroupedBackground)
        #else
        Color(nsColor: .windowBackgroundColor)
        #endif
    }

    /// A card or row on the grouped page.
    static var cardBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .secondarySystemGroupedBackground)
        #else
        Color(nsColor: .controlBackgroundColor)
        #endif
    }

    /// A field or band one step in from a card.
    static var insetBackground: Color {
        #if canImport(UIKit)
        Color(uiColor: .tertiarySystemGroupedBackground)
        #else
        Color(nsColor: .underPageBackgroundColor)
        #endif
    }
}

#if canImport(UIKit)
extension UIColor {
    convenience init(hex: UInt32) {
        self.init(red: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}
#else
extension NSColor {
    convenience init(hex: UInt32) {
        self.init(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: 1)
    }
}
#endif

/// Opens UrBudget's page in the Settings app, where iOS offers the app's own language.
@MainActor func openAppSettings() {
    #if canImport(UIKit)
    if let url = URL(string: UIApplication.openSettingsURLString) { UIApplication.shared.open(url) }
    #endif
}

/// How wide a money figure draws, so a list's columns line up with its widest figure.
func figureWidth(_ text: String, size: CGFloat, bold: Bool) -> CGFloat {
    #if canImport(UIKit)
    let font = UIFont.monospacedDigitSystemFont(ofSize: UIFontMetrics.default.scaledValue(for: size), weight: bold ? .bold : .regular)
    #else
    let font = NSFont.monospacedDigitSystemFont(ofSize: size, weight: bold ? .bold : .regular)
    #endif
    return ceil((text as NSString).size(withAttributes: [.font: font]).width)
}

extension ToolbarItemPlacement {
    static var leadingBar: ToolbarItemPlacement {
        #if os(iOS)
        .topBarLeading
        #else
        .navigation
        #endif
    }

    static var trailingBar: ToolbarItemPlacement {
        #if os(iOS)
        .topBarTrailing
        #else
        .primaryAction
        #endif
    }

    static var bottomBarItems: ToolbarItemPlacement {
        #if os(iOS)
        .bottomBar
        #else
        .automatic
        #endif
    }
}

extension View {
    func inlineTitle() -> some View {
        #if os(iOS)
        navigationBarTitleDisplayMode(.inline)
        #else
        self
        #endif
    }

    /// A page's title large and on the bar's own row, beside its buttons, as Android's top bar
    /// has it: the row a large title takes under the bar goes to the page.
    /// A page reached from another keeps the bar's small title, beside its back button.
    func barTitle(_ large: Bool = true) -> some View {
        #if os(iOS)
        toolbarTitleDisplayMode(large ? .inlineLarge : .inline)
        #else
        self
        #endif
    }

    /// Rows as close together as Android's: about 56 points for a name over a line of detail.
    func compactRows() -> some View { listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16)) }

    /// A tab's first card starts just under its bar, where a grouped list leaves a line's room.
    func tightTop() -> some View { contentMargins(.top, 4, for: .scrollContent) }

    /// Search as a button on the bar until it's wanted, where iOS offers that (26 and later), so
    /// a list starts at the top of its page as on Android.
    @ViewBuilder func searchMinimized() -> some View {
        #if os(iOS)
        if #available(iOS 26.0, *) { searchToolbarBehavior(.minimize) } else { self }
        #else
        self
        #endif
    }

    /// A plain list on the app's grey page, as Android has the budget and the register: what
    /// stands above the rows (a banner, a card, a notice) is on the page, and the rows are white.
    /// On a window wider than a page reads well at, the list keeps to a column down its middle:
    /// its bands and its rows together, with the page either side.
    func pageGround(maximum: CGFloat = WideLayout.readable) -> some View {
        // No room between sections either: a band follows the rows above it directly.
        #if os(iOS)
        scrollContentBackground(.hidden).listSectionSpacing(0).scrollIndicators(.hidden)
            .modifier(EndsAtSideBar()).readableWidth(maximum).awayFromSideBar()
        #else
        scrollContentBackground(.hidden).background(Color.groupedBackground)
        #endif
    }

    /// A little more room beside a bar iOS stands at the window's side (an unfolded iPhone Duo's
    /// tabs and buttons): see `ClearOfSideBar`.
    func clearOfSideBar() -> some View { modifier(ClearOfSideBar()).modifier(AwayFromSideBar()) }

    /// Gives a page that isn't beside the window's side bar the room kept for it: see `AwayFromSideBar`.
    func awayFromSideBar() -> some View { modifier(AwayFromSideBar()) }

    /// A line under the bar's title, where iOS has one (26 and later): what a selection adds up to.
    @ViewBuilder func barSubtitle(_ text: String) -> some View {
        #if os(iOS)
        if #available(iOS 26.0, *) { navigationSubtitle(Text(text)) } else { self }
        #else
        self
        #endif
    }

    /// A plain list's row with the page's own margins. They're said outright: on an unfolded
    /// iPhone Duo's inner screen a plain list gives its rows no margins of its own, and a row
    /// that relied on them lay against the pane's edge.
    func plainRow(vertical: CGFloat = 8) -> some View { listRowInsets(EdgeInsets(top: vertical, leading: 16, bottom: vertical, trailing: 16)) }

    func largeTitle() -> some View {
        #if os(iOS)
        navigationBarTitleDisplayMode(.large)
        #else
        self
        #endif
    }

    func decimalKeyboard() -> some View {
        #if os(iOS)
        keyboardType(.numbersAndPunctuation)
        #else
        self
        #endif
    }

    func numberKeyboard() -> some View {
        #if os(iOS)
        keyboardType(.numberPad)
        #else
        self
        #endif
    }

    func emailKeyboard() -> some View {
        #if os(iOS)
        keyboardType(.emailAddress).textInputAutocapitalization(.never)
        #else
        self
        #endif
    }

    /// Codes, keys and identifiers: no capitals or corrections added while typing.
    func plainTextEntry() -> some View {
        #if os(iOS)
        textInputAutocapitalization(.never).autocorrectionDisabled()
        #else
        autocorrectionDisabled()
        #endif
    }

    func wordsCapitalized() -> some View {
        #if os(iOS)
        textInputAutocapitalization(.words)
        #else
        self
        #endif
    }

    /// Cards of rows on the grouped page, set as closely as Android's: sections a little apart,
    /// and the first card just under the bar.
    func groupedList() -> some View {
        #if os(iOS)
        // No scroll bar, as on Android. It's said here as well as on the window: a sheet doesn't take it from there.
        listStyle(.insetGrouped).listSectionSpacing(.compact).contentMargins(.top, 8, for: .scrollContent).clearOfSideBar()
            .scrollIndicators(.hidden)
        #else
        listStyle(.inset)
        #endif
    }

    /// A screen of its own over the app (a full-screen cover on iPhone).
    /// A form over the screen: the whole of a phone's, and on a wide window (an iPad, an unfolded
    /// iPhone Duo) a panel with the page still in view behind it, as Android opens a form there.
    func cover<Item: Identifiable, Content: View>(item: Binding<Item?>, @ViewBuilder content: @escaping (Item) -> Content) -> some View {
        #if os(iOS)
        modifier(FormCover(item: item, form: content))
        #else
        sheet(item: item, content: content)
        #endif
    }

    /// Hides the tab bar while a screen needs its foot for something else (a selection's
    /// actions). Where the app's own dock stands in for the tab bar (a pretended fold, which the
    /// system's bar would lie across), the tab bar is always hidden and the dock goes with it.
    func tabBarHidden(_ hidden: Bool) -> some View { modifier(TabBarHidden(hidden: hidden)) }

    /// For every tab's screen: no system tab bar where the app's dock stands in for it.
    func tabBarOffFold() -> some View { modifier(TabBarOffFold()) }
}

private struct TabBarOffFold: ViewModifier {
    @Environment(\.fold) private var fold

    func body(content: Content) -> some View {
        // A rule out here wins over a screen's own, so without a fold it must say nothing of the
        // tab bar (it would bring it back over a selection's actions). It can't be left off
        // either: the screens would be made again each time the phone half opens. So off a fold
        // it names the bottom bar, and leaves that as it is.
        content.toolbar(fold.docksTabs ? .hidden : .automatic, for: fold.docksTabs ? .tabBar : .bottomBar)
    }
}

/// Whether a screen wants the tabs out of the way, for the dock that stands in for the tab bar on a fold.
struct TabsHiddenKey: PreferenceKey {
    static let defaultValue = false
    static func reduce(value: inout Bool, nextValue: () -> Bool) { value = value || nextValue() }
}

private struct TabBarHidden: ViewModifier {
    var hidden: Bool
    @Environment(\.fold) private var fold

    func body(content: Content) -> some View {
        content
            .toolbar(hidden || fold.docksTabs ? .hidden : .visible, for: .tabBar)
            .preference(key: TabsHiddenKey.self, value: hidden)
    }
}

extension View {
    /// Shows the drag handles of a list's movable rows (iOS's edit mode).
    func reorderMode(_ active: Bool) -> some View {
        #if os(iOS)
        environment(\.editMode, .constant(active ? .active : .inactive))
        #else
        self
        #endif
    }
}

/// Keeps a full-width list to a readable column on a wide screen (iPad, or a phone in landscape),
/// centred, as Mail and Settings do; on a phone in portrait it changes nothing.
/// Where iOS stands its tab bar at the window's side (an unfolded iPhone Duo), it keeps a page's
/// cards at the very edge of the room it leaves for the bar, nine points from the bar's glass.
/// This gives a page a little more air on that side. A side inset this wide is a bar: a rounded
/// corner's or a notch's room is narrower, and is left as it is. A page that isn't beside the bar
/// has no such room by the time this sees it (`AwayFromSideBar`), and gets none.
private struct ClearOfSideBar: ViewModifier {
    private struct Sides: Equatable, Sendable { var leading: CGFloat = 0; var trailing: CGFloat = 0 }

    static let least: CGFloat = 40
    static let air: CGFloat = 8

    @State private var sides = Sides()

    func body(content: Content) -> some View {
        content
            .safeAreaPadding(.leading, sides.leading >= Self.least ? Self.air : 0)
            .safeAreaPadding(.trailing, sides.trailing >= Self.least ? Self.air : 0)
            .onGeometryChange(for: Sides.self) { Sides(leading: $0.safeAreaInsets.leading, trailing: $0.safeAreaInsets.trailing) } action: { sides = $0 }
    }
}

/// Inside a navigation stack a page is given the room iOS keeps for the bar at the window's side
/// even when the page isn't beside it: the side of a fold away from the bar, or the half of the
/// window the assistant leaves the app. That's 84 points of nothing at the page's edge (and in
/// Arabic, where the bar stays at the right, it falls on the far side's edge by the fold). This
/// gives that room back to a page that doesn't reach the window's side. It has to be said inside
/// the stack, on the page: from outside it does nothing.
private struct AwayFromSideBar: ViewModifier {
    private struct Place: Equatable, Sendable { var leading: CGFloat = 0, trailing: CGFloat = 0, minX: CGFloat = 0, maxX: CGFloat = 0 }

    @Environment(\.layoutDirection) private var direction
    @State private var place = Place()

    func body(content: Content) -> some View {
        #if canImport(UIKit)
        let phantom = WideLayout.phantomInsets(minX: place.minX, maxX: place.maxX, leading: place.leading, trailing: place.trailing,
                                               window: FoldWindow.width, rightToLeft: direction == .rightToLeft)
        #else
        let phantom = (leading: false, trailing: false)
        #endif
        var edges: Edge.Set { var set: Edge.Set = []; if phantom.leading { set.insert(.leading) }; if phantom.trailing { set.insert(.trailing) }; return set }
        content
            .ignoresSafeArea(.container, edges: edges)
            .onGeometryChange(for: Place.self) { proxy in
                let frame = proxy.frame(in: .global)
                return Place(leading: proxy.safeAreaInsets.leading, trailing: proxy.safeAreaInsets.trailing, minX: frame.minX, maxX: frame.maxX)
            } action: { place = $0 }
    }
}

/// A plain list beside the bar at the window's side: its rows and bands end where the bar's room
/// begins. (The list's own view runs on under the bar, whatever room iOS keeps, and showed
/// through it.) Above and below it's left to run under the bars as iOS has it.
private struct EndsAtSideBar: ViewModifier {
    private struct Sides: Shape {
        var clips: Bool
        func path(in rect: CGRect) -> Path { Path(rect.insetBy(dx: clips ? 0 : -4000, dy: -4000)) }
    }

    @State private var beside = false

    func body(content: Content) -> some View {
        content
            .clipShape(Sides(clips: beside))
            .onGeometryChange(for: Bool.self) { max($0.safeAreaInsets.leading, $0.safeAreaInsets.trailing) >= ClearOfSideBar.least } action: { beside = $0 }
    }
}

/// A list no wider than a page reads well at, down the middle of a wide window with the page's
/// ground either side. The list's own frame is narrowed (margins set on a list take the place of
/// the room iOS keeps for a bar at the window's side, and its cards slid under the bar), so its
/// rows, its bands and its scroll bar keep to the column together.
#if os(iOS)
private struct FormCover<Item: Identifiable, Form: View>: ViewModifier {
    @Binding var item: Item?
    @ViewBuilder var form: (Item) -> Form
    /// Decided when the form opens, and kept while it's open: a window that narrows under an open
    /// form doesn't close it to open it again another way.
    @State private var panel = false

    func body(content: Content) -> some View {
        content
            .onChange(of: item?.id == nil, initial: true) { _, closed in
                if !closed { panel = WideLayout.formIsPanel(window: FoldWindow.width) }
            }
            .fullScreenCover(item: Binding(get: { panel ? nil : item }, set: { item = $0 })) { form($0).scrollIndicators(.hidden) }
            .sheet(item: Binding(get: { panel ? item : nil }, set: { item = $0 })) { opened in
                // What's typed isn't lost to a stray swipe: the form's own Cancel closes it.
                form(opened).interactiveDismissDisabled().scrollIndicators(.hidden)
            }
    }
}
#endif

private struct ReadableWidth: ViewModifier {
    var maximum: CGFloat

    func body(content: Content) -> some View {
        content
            .frame(maxWidth: maximum)
            .frame(maxWidth: .infinity)
            .background(Color.groupedBackground)
    }
}

extension View {
    func readableWidth(_ maximum: CGFloat = WideLayout.readable) -> some View { modifier(ReadableWidth(maximum: maximum)) }
}
