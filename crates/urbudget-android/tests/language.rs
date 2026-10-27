//! The engine calls the phone apps make, in Arabic. Switching the language is process-wide, so
//! this has a test binary of its own.
use serde_json::{json, Value};
use urbudget_core::i18n;

fn invoke(method: &str, params: Value) -> Value {
    serde_json::from_str(&urbudget_android::invoke_json(&json!({ "version": 1, "method": method, "params": params }).to_string())).unwrap()
}

#[test]
fn a_new_sample_budget_and_the_engines_errors_follow_the_language() {
    let english = invoke("sampleBudget", json!({ "month": "2026-10" }));
    assert_eq!(english["value"]["name"], "Sample budget");
    assert_eq!(english["value"]["categories"][0]["name"], "Groceries");

    i18n::set_language("ar");
    let arabic = invoke("sampleBudget", json!({ "month": "2026-10" }))["value"].clone();
    // The sample's names become the new budget's own data, made in the language in use; its
    // ids and amounts are the same in every language.
    assert_eq!(arabic["name"], "ميزانية تجريبية");
    assert_eq!(arabic["accounts"][0]["name"], "الحساب الجاري");
    assert_eq!(arabic["categories"][0]["name"], "البقالة");
    assert_eq!(arabic["categories"][0]["group"], "المصروفات اليومية");
    assert_eq!(arabic["transactions"][0]["payee"], "بقالة الحي");
    assert_eq!(arabic["categories"][0]["id"], english["value"]["categories"][0]["id"]);
    assert_eq!(arabic["assignments"], english["value"]["assignments"]);
    // A name the app chose stays the app's.
    assert_eq!(invoke("sampleBudget", json!({ "month": "2026-10", "name": "Explore" }))["value"]["name"], "Explore");
    assert_eq!(invoke("sampleBudget", json!({ "month": "October" }))["error"], "شهر الميزانية التجريبية غير صالح");
    i18n::set_language("en");
    assert_eq!(invoke("sampleBudget", json!({ "month": "October" }))["error"], "Invalid sample budget month");
}
