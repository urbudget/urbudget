//! The process-wide language as the apps use it: English until `set_language("ar")`, then
//! Arabic for the engine's messages. These tests switch the language for the whole process, so
//! they have a test binary of their own and take turns.
use std::sync::Mutex;
use urbudget_core::domain::{self, Budget};
use urbudget_core::i18n::{self, Language};
use urbudget_core::{bank_import, loans, store::Store};

static TURN: Mutex<()> = Mutex::new(());

/// Runs `f` with the language set to `code`, and leaves English behind.
fn speaking<T>(code: &str, f: impl FnOnce() -> T) -> T {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    i18n::set_language(code);
    let out = f();
    i18n::set_language("en");
    out
}

fn budget() -> Budget {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": 2, "name": "Household", "currency": "SAR",
        "accounts": [{ "id": "cash", "name": "Cash", "kind": "cash", "openingBalance": 100000, "archived": false }],
        "categories": [{ "id": "food", "name": "Food", "group": "Living", "target": 0 }],
        "transactions": [], "assignments": []
    }))
    .unwrap()
}
fn unnamed() -> Budget {
    let mut b = budget();
    b.name = " ".into();
    b
}
/// The error a call ends with.
fn error<T>(result: Result<T, String>) -> String {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}
fn bank_file(content: &str) -> String {
    error(bank_import::preview(&budget(), "cash", content, "csv", "ymd", false))
}
fn no_account() -> String {
    error(bank_import::preview(&budget(), "nowhere", "", "csv", "ymd", false))
}

#[test]
fn english_is_the_default_and_unchanged() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(i18n::language(), Language::English);
    assert_eq!(domain::validate(&unnamed()).unwrap_err(), "Invalid budget name or currency.");
    assert_eq!(no_account(), "Choose an account.");
    assert_eq!(bank_file("Date,Payee,Amount\n2026-10-01,Shop,abc\n"), "Invalid amount: abc");
    assert_eq!(
        bank_file("Date,Payee,Amount\n2026-10-01,Shop,5.00 USD\n"),
        "The amount ‘5.00 USD’ is in USD, but this budget uses SAR."
    );
}

#[test]
fn arabic_reaches_real_messages() {
    speaking("ar", || {
        assert_eq!(i18n::language(), Language::Arabic);
        assert_eq!(domain::validate(&unnamed()).unwrap_err(), "اسم الميزانية أو عملتها غير صالح.");
        assert_eq!(no_account(), "اختر حسابًا.");
        let terms: loans::Terms = serde_json::from_value(serde_json::json!({
            "accountId": "loan", "categoryId": "food", "annualRateBasisPoints": 1200, "monthlyPayment": 0, "monthlyFees": 0
        }))
        .unwrap();
        assert_eq!(
            error(loans::project(100_000, &terms, "2026-10-01", 0)),
            "أدخل دفعة موجبة، وسعر فائدة من 0 إلى 1,000%، ورسومًا غير سالبة."
        );
        // Words the engine shows outside a sentence follow the language too.
        let october = chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        assert_eq!(i18n::month_year(october), "أكتوبر 2026");
        assert_eq!(i18n::day_month(october), "5 أكتوبر");
    });
}

#[test]
fn values_and_codes_survive_translation() {
    speaking("ar", || {
        // A value keeps its text, isolated so it reads in its own direction, wherever Arabic puts it.
        let message = bank_file("Date,Payee,Amount\n2026-10-01,Shop,abc\n");
        assert_eq!(message, "مبلغ غير صالح: \u{2068}abc\u{2069}");
        let message = bank_file("Date,Payee,Amount\n2026-10-01,Shop,5.00 USD\n");
        assert!(message.starts_with("المبلغ «\u{2068}5.00 USD\u{2069}» بعملة \u{2068}USD\u{2069}"), "{message}");
        assert!(message.ends_with("\u{2068}SAR\u{2069}."), "{message}");

        // A code the apps read stays in English at the start; only the sentence is Arabic.
        let folder = tempfile::tempdir().unwrap();
        let mut store = Store::open(&folder.path().join("budget.sqlite3")).unwrap();
        let conflict = error(store.save(&budget(), "an old revision"));
        assert!(conflict.starts_with("CONFLICT: "), "{conflict}");
        assert!(conflict["CONFLICT: ".len()..].chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)), "{conflict}");
    });
}

#[test]
fn only_ar_selects_arabic() {
    for (code, language) in [("ar", Language::Arabic), ("en", Language::English), ("fr", Language::English), ("", Language::English), ("system", Language::English)] {
        speaking(code, || assert_eq!(i18n::language(), language, "{code:?}"));
    }
    speaking("ar", || {});
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(i18n::language(), Language::English, "switching back leaves English");
}
