import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

/// UrBudget's colours (desktop/src/styles.css, as Android shows them) on iOS's own grouped
/// backgrounds. The words and status colours stay UrBudget's; surfaces and type follow iOS.
extension Color {
    static let urAccent = Color(light: 0x0068DA, dark: 0x5B93FF)
    static let urPositive = Color(light: 0x197A35, dark: 0x56C67F)
    static let urNegative = Color(light: 0xCC2229, dark: 0xFF8077)
    static let urWarning = Color(light: 0xAD4F00, dark: 0xE2B44A)
    /// Yellow for spending that still needs a category.
    static let urAttention = Color(light: 0xFFCC00, dark: 0xE2B44A)
    static let urOnAttention = Color(light: 0x1D1D1F, dark: 0x14161A)
    /// The grey of zero amounts and inactive chips.
    static let urZero = Color(light: 0xE9E9EE, dark: 0x2E2E33)
    /// Text on a solid status fill.
    static let urOnSolid = Color(light: 0xFFFFFF, dark: 0x0C0E18)
    /// A band across a plain list: a group in the budget, a day in the register.
    static let urBand = Color(light: 0xF5F5F7, dark: 0x1C1C1E)
    static let urHairline = Color(light: 0xD9D9DE, dark: 0x38383A)
    static let urBrand = Color(light: 0x6D4AE8, dark: 0x6D4AE8)
    /// Chevrons and other faint marks (the system's tertiary label). Fixed colours, unlike
    /// `.primary`/`.secondary`/`.tertiary`, which iOS 26 tints inside buttons.
    static let urTertiary = Color(light: 0xB4B4B9, dark: 0x5A5A5F)

    static func flag(_ name: String) -> Color {
        switch name {
        case "red": return Color(light: 0xFF3B30, dark: 0xE5484D)
        case "orange": return Color(light: 0xFF9500, dark: 0xF76B15)
        case "yellow": return Color(light: 0xFFCC00, dark: 0xE2A336)
        case "green": return Color(light: 0x34C759, dark: 0x30A46C)
        case "blue": return Color(light: 0x007AFF, dark: 0x3E63DD)
        case "purple": return Color(light: 0xAF52DE, dark: 0x8E4EC6)
        default: return .secondary
        }
    }
}

/// What a category's state looks like: the pill's fill and text, and the colour for words about it.
struct StatusColors {
    var fill: Color
    var content: Color
    var ink: Color

    init(_ status: String) {
        switch status {
        case "positive": (fill, content, ink) = (.urPositive, .urOnSolid, .urPositive)
        case "underfunded", "credit": (fill, content, ink) = (.urWarning, .urOnSolid, .urWarning)
        case "negative": (fill, content, ink) = (.urNegative, .urOnSolid, .urNegative)
        default: (fill, content, ink) = (.urZero, .secondary, .secondary)
        }
    }
}

extension Tone {
    var color: Color {
        switch self {
        case .negative: return .urNegative
        case .warning: return .urWarning
        case .positive: return .urPositive
        case .muted: return .secondary
        }
    }
}

/// Money in lists: tabular figures, so amounts line up, at the text style the size stands for,
/// so they grow with Dynamic Type.
extension Text {
    func figure(_ size: CGFloat = 17, weight: Font.Weight = .regular) -> Text {
        font(.system(Font.TextStyle.forSize(size), design: .default, weight: weight)).monospacedDigit()
    }
}

extension Font {
    /// The system font at exactly `size` points at the default text size, growing with Dynamic
    /// Type: for figures that sit between two text styles (14 points, in the budget's columns).
    static func sized(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        #if canImport(UIKit)
        .system(size: UIFontMetrics.default.scaledValue(for: size), weight: weight)
        #else
        .system(size: size, weight: weight)
        #endif
    }
}

extension Font.TextStyle {
    /// The text style whose default size is nearest `size` points.
    static func forSize(_ size: CGFloat) -> Font.TextStyle {
        switch size {
        case ..<12.5: return .caption
        case ..<14: return .footnote
        case ..<16: return .subheadline
        case ..<18.5: return .body
        case ..<21: return .title3
        case ..<25: return .title2
        case ..<31: return .title
        default: return .largeTitle
        }
    }
}

/// The app's text one step under the size iOS is set to: body at 16 points where iOS has 17, a
/// footnote at 12, the scale Android's screens are drawn to, so the same rows hold the same
/// words on both. It moves with the person's own text size, a step behind it. It's set on the
/// window, so sheets and the bars follow it too (a sheet doesn't take a size set in SwiftUI).
@MainActor enum AppTypeScale {
    #if canImport(UIKit)
    static let sizes: [UIContentSizeCategory] = [
        .extraSmall, .small, .medium, .large, .extraLarge, .extraExtraLarge, .extraExtraExtraLarge,
        .accessibilityMedium, .accessibilityLarge, .accessibilityExtraLarge, .accessibilityExtraExtraLarge,
        .accessibilityExtraExtraExtraLarge,
    ]

    nonisolated static func step(_ size: UIContentSizeCategory, in sizes: [UIContentSizeCategory]) -> UIContentSizeCategory {
        guard let index = sizes.firstIndex(of: size), index > 0 else { return size }
        return sizes[index - 1]
    }

    /// Sets every window of the app a step under the system's size; called when the app comes
    /// forward and when the size is changed in Settings.
    static func apply() {
        let stepped = step(UIApplication.shared.preferredContentSizeCategory, in: sizes)
        for scene in UIApplication.shared.connectedScenes {
            for window in (scene as? UIWindowScene)?.windows ?? [] {
                // Asking for an override that isn't set is an error, so that's asked first.
                let set = window.traitOverrides.contains(UITraitPreferredContentSizeCategory.self)
                if !set || window.traitOverrides.preferredContentSizeCategory != stepped {
                    window.traitOverrides.preferredContentSizeCategory = stepped
                }
            }
        }
    }
    #else
    static func apply() {}
    #endif
}
