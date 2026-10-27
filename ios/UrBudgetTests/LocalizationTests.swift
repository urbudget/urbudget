import XCTest
@testable import UrBudget

/// The Arabic String Catalogs as the app ships them: compiled into ar.lproj beside English.
final class LocalizationTests: XCTestCase {
    private func bundle(_ language: String) throws -> Bundle {
        let path = try XCTUnwrap(Bundle.main.path(forResource: language, ofType: "lproj"), "No \(language).lproj in the app")
        return try XCTUnwrap(Bundle(path: path))
    }

    func testArabicStringsResolve() throws {
        let arabic = try bundle("ar")
        let expected = [
            "Budget": "الميزانية",
            "Transactions": "المعاملات",
            "Save": "حفظ",
            "Cancel": "إلغاء",
            "Settings & data": "الإعدادات والبيانات",
            "Account & sync": "الحساب والمزامنة",
            "Language": "اللغة",
            "Assigned": "المخصَّص",
            "Available": "المتاح",
            "Unassigned": "غير المخصَّص",
            "Payee": "المستفيد",
        ]
        for (key, value) in expected {
            XCTAssertEqual(arabic.localizedString(forKey: key, value: "MISSING", table: nil), value, key)
        }
        // A new budget's starter names and the sample's name use the engine's Arabic words.
        XCTAssertEqual(arabic.localizedString(forKey: "starter.group.savings", value: "MISSING", table: nil), "المدخرات")
        XCTAssertEqual(arabic.localizedString(forKey: "Groceries", value: "MISSING", table: nil), "البقالة")
        XCTAssertEqual(arabic.localizedString(forKey: "Sample budget", value: "MISSING", table: nil), "ميزانية تجريبية")
        // A named key carries its English in the catalog, and its Arabic here.
        XCTAssertEqual(arabic.localizedString(forKey: "state.open", value: "MISSING", table: nil), "مفتوحة")
        XCTAssertEqual(try bundle("en").localizedString(forKey: "state.open", value: "MISSING", table: nil), "Open")
        // Placeholders survive, isolated so names and amounts keep their own direction.
        XCTAssertEqual(arabic.localizedString(forKey: "Opened %@", value: "MISSING", table: nil), "فُتحت \u{2068}%@\u{2069}")
    }

    func testInfoPlistAndShortcutsAreTranslated() throws {
        let arabic = try bundle("ar")
        XCTAssertEqual(arabic.localizedString(forKey: "CFBundleDisplayName", value: "MISSING", table: "InfoPlist"), "UrBudget")
        XCTAssertEqual(arabic.localizedString(forKey: "New transaction", value: "MISSING", table: "InfoPlist"), "معاملة جديدة")
        XCTAssertEqual(arabic.localizedString(forKey: "Bank statement", value: "MISSING", table: "InfoPlist"), "كشف حساب")
        XCTAssertEqual(try bundle("en").localizedString(forKey: "CFBundleDisplayName", value: "MISSING", table: "InfoPlist"), "UrBudget")
        XCTAssertNotEqual(arabic.localizedString(forKey: "Add a transaction in ${applicationName}", value: "MISSING", table: "AppShortcuts"), "MISSING")
    }

    /// Every count the app shows has all six Arabic plural forms, and English keeps its one and other.
    func testPluralVariationsExist() throws {
        let arabic = try XCTUnwrap(NSDictionary(contentsOf: try XCTUnwrap(try bundle("ar").url(forResource: "Localizable", withExtension: "stringsdict"))))
        let english = try XCTUnwrap(NSDictionary(contentsOf: try XCTUnwrap(try bundle("en").url(forResource: "Localizable", withExtension: "stringsdict"))))
        let keys = ["%lld transactions", "transactions.inSentence", "Updated %lld transactions.", "%lld budgets need review",
                    "Removed %lld backups", "Remove %lld older backups?", "%lld days", "%lld months", "%lld years", "%lld rows"]
        for key in keys {
            let forms = try XCTUnwrap(pluralForms(arabic[key]), "No Arabic plural for \(key)")
            XCTAssertEqual(Set(forms), ["zero", "one", "two", "few", "many", "other"], key)
            let englishForms = try XCTUnwrap(pluralForms(english[key]), "No English plural for \(key)")
            XCTAssertTrue(Set(englishForms).isSuperset(of: ["one", "other"]), key)
        }
    }

    /// Arabic picks its plural form by number, with Western digits as budgets write them.
    func testArabicPluralFormsAndDigits() throws {
        let format = try bundle("ar").localizedString(forKey: "%lld transactions", value: nil, table: nil)
        let arabic = Locale(identifier: "ar@numbers=latn")
        XCTAssertEqual(String(format: format, locale: arabic, 1), "معاملة واحدة")
        XCTAssertEqual(String(format: format, locale: arabic, 2), "معاملتان")
        XCTAssertEqual(String(format: format, locale: arabic, 3), "3 معاملات")
        XCTAssertEqual(String(format: format, locale: arabic, 11), "11 معاملة")
        XCTAssertEqual(String(format: format, locale: arabic, 100), "100 معاملة")
    }

    /// English reads exactly as it did before the catalogs.
    func testEnglishIsUnchanged() {
        XCTAssertEqual(AppLanguage.code, "en")
        XCTAssertEqual(AppLanguage.leftToRight("-1,234.56 SAR"), "-1,234.56 SAR")
        XCTAssertEqual(transactionCount(1), "1 transaction")
        XCTAssertEqual(transactionCount(3), "3 transactions")
        XCTAssertEqual(String(localized: "Removed \(Int64(1)) backups", locale: .app), "Removed 1 backup")
        XCTAssertEqual(String(localized: "Removed \(Int64(4)) backups", locale: .app), "Removed 4 backups")
        XCTAssertEqual(StarterBudget.groups.map(\.name), ["Bills", "Needs", "Wants", "Savings"])
        XCTAssertEqual(StarterBudget.groups[1].categories, ["Groceries", "Transportation", "Medical", "Insurance"])
        XCTAssertEqual(Dates.ordinal(22), "22nd")
        XCTAssertEqual(Dates.monthLabel("2026-10"), "October 2026")
        XCTAssertEqual(Locale.app.numberingSystem.identifier, "latn")
    }

    private func pluralForms(_ entry: Any?) -> [String]? {
        guard let entry = entry as? [String: Any] else { return nil }
        for case let (key, value as [String: Any]) in entry where key != "NSStringLocalizedFormatKey" {
            if value["NSStringFormatSpecTypeKey"] as? String == "NSStringPluralRuleType" {
                return value.keys.filter { !$0.hasPrefix("NSString") }
            }
        }
        return nil
    }
}
