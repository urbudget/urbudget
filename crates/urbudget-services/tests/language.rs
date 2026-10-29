//! The mobile apps' `set_language` service call, and what it changes. These tests switch the
//! language for the whole process, so they have a test binary of their own and take turns.
use serde_json::{json, Value};
use std::sync::Mutex;
use urbudget_services::i18n::{self, Language};
use urbudget_services::runtime::Runtime;

static TURN: Mutex<()> = Mutex::new(());

fn call(runtime: &Runtime, method: &str, params: Value) -> Result<Value, String> {
    urbudget_services::block_on(runtime.dispatch(method, params))
}

#[test]
fn the_service_call_switches_the_engines_messages() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let folder = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    urbudget_services::block_on(runtime.dispatch("save_budget",serde_json::json!({"budget":urbudget_core::test_support::sample(),"expectedToken":""}))).unwrap();
    let budget = urbudget_core::test_support::sample();
    let stale = || call(&runtime, "save_budget", json!({ "budget": budget, "expectedToken": "an old revision" })).unwrap_err();

    assert_eq!(stale(), "CONFLICT: The budget changed. Reload it before saving your edit.");
    assert_eq!(call(&runtime, "set_language", json!({ "language": "ar" })).unwrap(), Value::Null);
    assert_eq!(i18n::language(), Language::Arabic);
    assert_eq!(stale(), "CONFLICT: تغيّرت الميزانية. أعد تحميلها قبل حفظ تعديلك.");
    // One from deeper in the engine.
    let fresh_start = call(&runtime, "fresh_start", json!({ "name": "Next year", "asOf": "not a date" })).unwrap_err();
    assert_eq!(fresh_start, "اختر تاريخًا صالحًا للبداية الجديدة.");

    assert_eq!(call(&runtime, "set_language", json!({ "language": "en" })).unwrap(), Value::Null);
    assert_eq!(stale(), "CONFLICT: The budget changed. Reload it before saving your edit.");
    assert!(call(&runtime, "set_language", json!({})).is_err(), "the language is required");
    assert_eq!(i18n::language(), Language::English);
}

#[test]
fn messages_from_urbudget_app_follow_the_language() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let rate_limited = "Too many attempts. Wait a few minutes and try again.";
    assert_eq!(i18n::translate(rate_limited), rate_limited);
    i18n::set_language("ar");
    assert_eq!(i18n::translate(rate_limited), "محاولات كثيرة جدًا. انتظر بضع دقائق، ثم حاول مرة أخرى.");
    // A message the table doesn't know stays as the server wrote it.
    assert_eq!(i18n::translate("Something new from the server."), "Something new from the server.");
    i18n::set_language("en");
}

#[test]
fn the_assistant_is_asked_to_reply_in_arabic() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    use urbudget_services::assistant::prompt;
    assert_eq!(prompt::in_language(prompt::INSTRUCTIONS), prompt::INSTRUCTIONS);
    i18n::set_language("ar");
    // The instructions stay English and ask for Arabic replies, unless the person writes in
    // another language.
    let instructions = prompt::in_language(prompt::INSTRUCTIONS);
    assert!(instructions.starts_with(prompt::INSTRUCTIONS) && instructions.ends_with(prompt::ARABIC));
    assert!(prompt::ARABIC.contains("Reply in Modern Standard Arabic unless the user writes to you in another language"));
    i18n::set_language("en");
}
