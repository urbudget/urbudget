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

    func groupedList() -> some View {
        #if os(iOS)
        listStyle(.insetGrouped)
        #else
        listStyle(.inset)
        #endif
    }

    /// A screen of its own over the app (a full-screen cover on iPhone).
    func cover<Item: Identifiable, Content: View>(item: Binding<Item?>, @ViewBuilder content: @escaping (Item) -> Content) -> some View {
        #if os(iOS)
        fullScreenCover(item: item, content: content)
        #else
        sheet(item: item, content: content)
        #endif
    }

    func tabBarHidden(_ hidden: Bool) -> some View {
        #if os(iOS)
        toolbar(hidden ? .hidden : .visible, for: .tabBar)
        #else
        self
        #endif
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
private struct ReadableWidth: ViewModifier {
    var maximum: CGFloat
    @State private var width: CGFloat = 0

    func body(content: Content) -> some View {
        Group {
            // Only on a wide screen: a narrow one keeps the list's own margins (a grouped list's inset).
            if width > maximum + 40 {
                content.contentMargins(.horizontal, (width - maximum) / 2, for: .scrollContent)
            } else {
                content
            }
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { width = $0 }
    }
}

extension View {
    func readableWidth(_ maximum: CGFloat = 760) -> some View { modifier(ReadableWidth(maximum: maximum)) }
}
