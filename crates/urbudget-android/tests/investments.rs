//! Investment holdings through the commands the phone apps send (More → Interactive Brokers):
//! a statement comes in, fills a new account, its deposits go through the phones' compact review,
//! and what the app then keeps of the budget (the light copy) still has the holdings and the
//! marks on the transactions that follow the broker's value.
use serde_json::{json, Value};
use urbudget_core::test_support::{sample, FLEX_SAMPLE};
use urbudget_services::{block_on, runtime::Runtime};

fn call(runtime: &Runtime, method: &str, p: Value) -> Result<Value, String> {
    block_on(runtime.dispatch(method, p))
}
fn ok(runtime: &Runtime, method: &str, p: Value) -> Value {
    call(runtime, method, p).unwrap_or_else(|error| panic!("{method}: {error}"))
}
fn token(runtime: &Runtime) -> String {
    ok(runtime, "native_budget_status", json!({}))["token"].as_str().unwrap().to_owned()
}
/// A runtime with a budget in the statement's own currency saved and loaded.
fn opened() -> (tempfile::TempDir, Runtime) {
    let folder = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    let mut budget = serde_json::to_value(sample()).unwrap();
    budget["currency"] = json!("USD");
    ok(&runtime, "save_budget", json!({"budget":budget,"expectedToken":""}));
    (folder, runtime)
}
fn transaction<'a>(budget: &'a Value, id: &str) -> &'a Value {
    budget["transactions"].as_array().unwrap().iter().find(|t| t["id"] == id).unwrap_or_else(|| panic!("no {id}"))
}
fn balance(budget: &Value, account: &str) -> i64 {
    budget["transactions"].as_array().unwrap().iter().filter(|t| t["accountId"] == account).map(|t| t["amount"].as_i64().unwrap()).sum()
}

#[test]
fn a_statement_fills_a_new_account_and_its_deposits_go_through_the_compact_review() {
    let (_folder, runtime) = opened();
    // Before any statement there is nothing to choose from or review.
    assert_eq!(ok(&runtime, "ibkr_status", json!({}))["accounts"], json!([]));
    assert!(call(&runtime, "ibkr_review_compact", json!({})).is_err());
    assert!(call(&runtime, "ibkr_file", json!({"content":"not a statement"})).unwrap_err().contains("Flex"));

    let status = ok(&runtime, "ibkr_file", json!({"content":FLEX_SAMPLE}));
    assert_eq!((status["accounts"][0]["id"].as_str(), status["fromFile"].as_bool(), status["connected"].as_bool()), (Some("U1234567"), Some(true), Some(false)));
    let review = ok(&runtime, "ibkr_review_compact", json!({}));
    let found = &review["accounts"][0];
    assert_eq!((found["id"].as_str(), found["value"].as_i64(), found["positions"].as_i64()), (Some("U1234567"), Some(2_701_250), Some(3)));
    assert!(found["accountId"].is_null() && found["needsRate"].is_null() && found["rows"].as_array().unwrap().is_empty());

    // A stale revision is refused before anything changes, as a bank import's is.
    let before = token(&runtime);
    let links = json!([{"source":"U1234567","account":"broker","create":true}]);
    assert!(call(&runtime, "ibkr_apply", json!({"links":links,"expectedToken":"stale"})).unwrap_err().starts_with("CONFLICT:"));
    assert_eq!(token(&runtime), before);

    // Bringing it in saves the budget and answers with it, like a load.
    let loaded = ok(&runtime, "ibkr_apply", json!({"links":links,"expectedToken":before}));
    assert_ne!(loaded["token"], json!(before));
    let budget = &loaded["budget"];
    let account = budget["accounts"].as_array().unwrap().iter().find(|a| a["id"] == "broker").unwrap();
    assert_eq!((account["name"].as_str(), account["kind"].as_str(), account["accountType"].as_str()), (Some("Long term"), Some("tracking"), Some("investment")));
    assert_eq!(budget["investments"][0]["total"], 2_701_250);
    assert_eq!(balance(budget, "broker"), 2_701_250);

    // The review now has its deposits, in the compact form the phones' bank review reads.
    let review = ok(&runtime, "ibkr_review_compact", json!({}));
    let found = &review["accounts"][0];
    assert_eq!(found["accountId"], "broker");
    let rows = found["rows"].as_array().unwrap();
    assert_eq!(rows.iter().map(|r| (r["date"].as_str().unwrap(), r["amount"].as_i64().unwrap(), r["duplicate"].as_bool().unwrap())).collect::<Vec<_>>(),
        vec![("2026-10-05", 100_000, false), ("2026-11-02", 40_000, false)]);
    let sets = found["candidateSets"].as_array().unwrap();
    assert!(rows.iter().all(|r| (r["candidateSet"].as_u64().unwrap() as usize) < sets.len()));

    // Imported as the phone sends them: each row without its matching choices, to be added.
    let choices: Vec<Value> = rows.iter().map(|r| {
        let mut row = r.clone();
        row.as_object_mut().unwrap().remove("candidateSet");
        json!({"row":row,"action":"new"})
    }).collect();
    let loaded = ok(&runtime, "ibkr_import", json!({"account":"broker","choices":choices,"expectedToken":token(&runtime)}));
    let budget = &loaded["budget"];
    // The money put in is no longer part of the months' market changes; the value stays the broker's.
    assert_eq!(transaction(budget, "investment-change:broker:2026-10")["amount"], 28_000);
    assert_eq!(transaction(budget, "investment-change:broker:2026-11")["amount"], 111_250);
    assert_eq!(balance(budget, "broker"), 2_701_250);
    assert!(ok(&runtime, "ibkr_review_compact", json!({}))["accounts"][0]["rows"].as_array().unwrap().iter().all(|r| r["duplicate"] == true));

    // A later sync with nothing to add fills the linked account again and changes nothing.
    let again = ok(&runtime, "ibkr_apply", json!({"links":[],"expectedToken":token(&runtime)}));
    assert_eq!(again["budget"]["transactions"], budget["transactions"]);
}

#[test]
fn the_light_copy_keeps_holdings_and_an_unlink_keeps_the_history() {
    let (_folder, runtime) = opened();
    ok(&runtime, "ibkr_file", json!({"content":FLEX_SAMPLE}));
    ok(&runtime, "ibkr_apply", json!({"links":[{"source":"U1234567","account":"broker","create":true}],"expectedToken":token(&runtime)}));

    // What a phone keeps of the budget: the holdings, and the marks that make the gain.
    let light = ok(&runtime, "load_budget", json!({"light":true}));
    let budget = &light["budget"];
    assert_eq!(budget["investments"][0]["accountId"], "broker");
    assert_eq!(budget["investments"][0]["positions"].as_array().unwrap().len(), 3);
    assert_eq!(transaction(budget, "investment-start:broker")["investment"], "start");
    assert_eq!(transaction(budget, "investment-change:broker:2026-11")["investment"], "change");

    // An edit to one of those transactions, sent as the phone sends its patches, keeps the mark.
    let mut noted = transaction(budget, "investment-change:broker:2026-10").clone();
    noted["memo"] = json!("Noted on the phone");
    let step = json!([{"patch":{"transactions":{"upsert":[noted],"delete":[]}}}]);
    ok(&runtime, "edit_budget", json!({"steps":step,"expectedToken":token(&runtime),"month":"2026-11","asOfMonth":"2026-11","light":true}));
    let saved = ok(&runtime, "load_budget", json!({}));
    let kept = transaction(&saved["budget"], "investment-change:broker:2026-10");
    assert_eq!((kept["investment"].as_str(), kept["memo"].as_str()), (Some("change"), Some("Noted on the phone")));

    // Unlink, as the phone's screen does it: the list goes, the account and its transactions stay.
    let unlink = json!([{"patch":{"extras":{"set":{},"delete":["investments"]}}}]);
    ok(&runtime, "edit_budget", json!({"steps":unlink,"expectedToken":token(&runtime),"month":"2026-11","asOfMonth":"2026-11","light":true}));
    let after = ok(&runtime, "load_budget", json!({}));
    assert!(after["budget"].get("investments").is_none());
    assert_eq!(balance(&after["budget"], "broker"), 2_701_250);
    // With nothing linked, the statement's account is offered again and has no deposits to review.
    let review = ok(&runtime, "ibkr_review_compact", json!({}));
    assert!(review["accounts"][0]["accountId"].is_null());
    assert!(review["accounts"][0]["rows"].as_array().unwrap().is_empty());
}
