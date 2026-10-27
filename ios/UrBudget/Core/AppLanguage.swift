import Foundation

/// The language the app runs in: English or Arabic, as iOS resolved it from the person's preferred
/// languages, or from the app's own language (Settings → UrBudget → Language). System is iOS's default.
/// Changing it there restarts the app, so it holds for the life of the process.
enum AppLanguage {
    /// "ar" or "en", never "system": the resolved language, as the engine is told it.
    static let code: String = Bundle.main.preferredLocalizations.first?.hasPrefix("ar") == true ? "ar" : "en"

    static var isRightToLeft: Bool { code == "ar" }

    /// The language's name in its own language, as language lists show it whatever the app's language.
    static var name: String { code == "ar" ? "العربية" : "English" }

    /// Left-to-right text (an amount, a sum) kept whole inside right-to-left text, so "-1,234.56 SAR"
    /// never comes out as "1,234.56 SAR-". English text is returned as it is.
    static func leftToRight(_ text: String) -> String {
        isRightToLeft && !text.isEmpty ? "\u{2066}" + text + "\u{2069}" : text
    }

    /// Tells the engine the language for the messages it writes (errors, sync and import status).
    /// Older engines don't know the request; the app carries on in English messages then.
    static func tellEngine() {
        _ = try? NativeCore.service("set_language", ["language": .string(code)])
    }
}

extension Locale {
    /// The app's language and the person's region, with Western digits (0–9) as budgets write them,
    /// for counts and other numbers inside the app's text.
    static var app: Locale {
        var components = Locale.Components(locale: .current)
        components.numberingSystem = Locale.NumberingSystem("latn")
        return Locale(components: components)
    }
}
