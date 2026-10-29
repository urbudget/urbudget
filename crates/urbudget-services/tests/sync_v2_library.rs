//! Host-neutral lifecycle acceptance tests, complementing the real process/Worker harness.
use serde_json::{json, Value};
use urbudget_services::runtime::Runtime;
fn call(runtime: &Runtime, method: &str, params: Value) -> Value {
    urbudget_services::block_on(runtime.dispatch(method, params)).unwrap()
}
#[test]
fn deleting_the_last_budget_preserves_raw_drafts_and_survives_restart() {
    let folder = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    assert!(call(&runtime, "load_budget", json!({}))["budget"].is_null());
    let budget = urbudget_core::test_support::sample();
    let id = call(&runtime, "create_budget", json!({"budget":budget}));
    call(&runtime, "library_open", json!({"id":id}));
    call(
        &runtime,
        "editor_checkpoint",
        json!({"budgetId":id,"id":"transaction-form","epoch":"mounted-1","target":{"recordId":"new"},"baseline":null,"values":{"amount":"-.","payee":"Keep these unfinished fields"}}),
    );
    let removed = call(&runtime, "library_trash", json!({"id":id}));
    assert!(removed["active"].is_null());
    assert!(removed["library"]["activeId"].is_null());
    assert!(call(&runtime, "load_budget", json!({}))["budget"].is_null());
    drop(runtime);
    let runtime = Runtime::new(folder.path()).unwrap();
    assert!(call(&runtime, "load_budget", json!({}))["budget"].is_null());
    let catalog = call(&runtime, "library_snapshot", json!({}));
    assert_eq!(catalog["entries"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["entries"][0]["lifecycle"], "trashed");
    let recovery = call(&runtime, "editor_recovery", json!({"budgetId":id}));
    assert_eq!(recovery[0]["draft"]["amount"], "-.");
    let copied = call(&runtime, "library_recover", json!({"id":id}));
    let recovered = copied["recoveredId"].clone();
    assert_ne!(recovered, id);
    call(&runtime, "library_open", json!({"id":recovered}));
    assert_eq!(
        call(&runtime, "editor_recovery", json!({"budgetId":recovered}))[0]["draft"],
        recovery[0]["draft"]
    );
}

#[test]
fn expired_selected_account_owns_new_budgets_but_explicit_signout_does_not() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("account.json");
    std::fs::write(&path,json!({"email":"selected@example.test","accountId":"selected-account","sessionExpired":true}).to_string()).unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    {
        let mut catalog = runtime.library.lock();
        catalog.ensure_account("selected-account").unwrap();
        catalog
            .set_account_state("selected-account", &json!({"protocol":2,"enabled":true}))
            .unwrap();
    }
    let budget = urbudget_core::test_support::sample();
    let id = call(&runtime, "create_budget", json!({"budget":budget}));
    let owned = call(&runtime, "library_snapshot", json!({}));
    let entry = owned["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id)
        .unwrap();
    assert_eq!(entry["accountId"], "selected-account");
    assert_ne!(
        entry["remoteId"], id,
        "local and remote identities are independent"
    );
    drop(runtime);
    // Simulate the persisted explicit-sign-out identity without touching any OS credentials.
    std::fs::write(path, r#"{"email":null}"#).unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    let local = call(&runtime, "create_budget", json!({"budget":budget}));
    let catalog = call(&runtime, "library_snapshot", json!({}));
    let entries = catalog["entries"].as_array().unwrap();
    assert_eq!(
        entries.iter().find(|e| e["id"] == local).unwrap()["scope"],
        "local"
    );
    assert_eq!(
        entries.iter().find(|e| e["id"] == id).unwrap()["accountId"],
        "selected-account"
    );
}

#[test]
fn a_late_discard_cannot_erase_a_newer_editor_checkpoint() {
    let folder = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    let id = call(
        &runtime,
        "create_budget",
        json!({"budget":urbudget_core::test_support::sample()}),
    );
    for epoch in ["old", "new"] {
        call(
            &runtime,
            "editor_checkpoint",
            json!({"budgetId":id,"id":"form","epoch":epoch,"baseline":null,"values":{"memo":epoch}}),
        );
    }
    call(
        &runtime,
        "editor_discard",
        json!({"budgetId":id,"id":"form","epoch":"old"}),
    );
    assert_eq!(
        call(&runtime, "editor_recovery", json!({"budgetId":id}))[0]["draft"]["memo"],
        "new"
    );
}

#[test]
fn explicit_local_only_budgets_stay_private_with_account_sync_enabled() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("account.json"), json!({"email":"selected@example.test","accountId":"selected-account","sessionExpired":true}).to_string()).unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    runtime.library.lock().set_account_state("selected-account", &json!({"protocol":2,"enabled":true})).unwrap();
    for (local_only, sample, private) in [
        (json!(true), Value::Null, true),
        (json!(false), json!(true), true),
        (Value::Null, json!({"from":"original-budget"}), true),
        (json!(false), json!(false), false),
        (Value::Null, Value::Null, false),
    ] {
        let mut budget = urbudget_core::test_support::sample();
        budget.extra.insert("sample".into(), sample.clone());
        let id = call(&runtime, "create_budget", json!({"budget":budget,"localOnly":local_only}));
        let catalog = call(&runtime, "library_snapshot", json!({}));
        let entry = catalog["entries"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap();
        assert_eq!(entry["scope"], if private { "local" } else { "account" }, "scope mismatch for localOnly={local_only}, sample={sample}");
        assert_eq!(entry["accountId"].is_null(), private);
        assert_eq!(entry["remoteId"].is_null(), private);
        assert_eq!(entry["incarnation"].is_null(), private);
    }
}

#[test]
fn unrelated_library_actions_keep_the_active_draft_and_light_snapshot() {
    let folder=tempfile::tempdir().unwrap(); let runtime=Runtime::new(folder.path()).unwrap();
    let mut budget=urbudget_core::test_support::sample();
    budget.extra.insert("sourceExport".into(),json!({"large":"original"}));
    let first=call(&runtime,"create_budget",json!({"budget":budget}));
    let opened=call(&runtime,"library_open",json!({"id":first}));
    assert!(opened["budget"].get("sourceExport").is_none());
    let second=call(&runtime,"create_budget",json!({"budget":urbudget_core::test_support::sample()}));
    let mut draft=opened["budget"].clone(); draft["name"]=json!("Unsaved working name");
    call(&runtime,"replace_draft",json!({"budget":draft}));
    let outcome=call(&runtime,"library_trash",json!({"id":second}));
    assert_eq!(outcome["active"]["budget"]["name"],"Unsaved working name");
    assert_eq!(call(&runtime,"snapshot_budget",json!({}))["name"],"Unsaved working name");
    assert_eq!(call(&runtime,"snapshot_budget",json!({}))["sourceExport"]["large"],"original");
}

#[test]
fn normal_editor_unmount_discards_only_live_active_checkpoint() {
    let folder=tempfile::tempdir().unwrap();let runtime=Runtime::new(folder.path()).unwrap();
    let id=call(&runtime,"create_budget",json!({"budget":urbudget_core::test_support::sample()}));
    call(&runtime,"library_open",json!({"id":id}));
    let params=json!({"budgetId":id,"id":"editor","epoch":"one","target":{},"baseline":{},"values":{"amount":"-"}});
    call(&runtime,"editor_checkpoint",params.clone());
    urbudget_services::library_api::discard_checkpoint_if_active(&runtime.db,&runtime.library,id.as_str().unwrap(),"editor","one").unwrap();
    assert!(call(&runtime,"editor_recovery",json!({"budgetId":id})).as_array().unwrap().is_empty());
    call(&runtime,"editor_checkpoint",params);
    call(&runtime,"library_trash",json!({"id":id}));
    urbudget_services::library_api::discard_checkpoint_if_active(&runtime.db,&runtime.library,id.as_str().unwrap(),"editor","one").unwrap();
    assert_eq!(call(&runtime,"editor_recovery",json!({"budgetId":id}))[0]["draft"]["amount"],"-");
}
