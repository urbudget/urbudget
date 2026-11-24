use super::*;

fn call(function: unsafe extern "C" fn(*const c_char) -> *mut c_char, request: &str) -> Value {
    let input = CString::new(request).unwrap();
    unsafe {
        let output = function(input.as_ptr());
        let text = CStr::from_ptr(output).to_str().unwrap().to_owned();
        urbudget_free(output);
        serde_json::from_str(&text).unwrap()
    }
}

fn service(method: &str, params: Value) -> Value {
    call(urbudget_service, &json!({"version":1,"method":method,"params":params}).to_string())
}

#[test]
fn engine_requests_match_the_android_transport() {
    let request = json!({"version":1,"method":"sampleBudget","params":{"month":"2026-10","currency":"USD"}}).to_string();
    let reply = call(urbudget_invoke, &request);
    assert_eq!(reply["ok"], true, "{reply}");
    let android: Value = serde_json::from_str(&urbudget_android::invoke_json(&request)).unwrap();
    assert_eq!(reply, android, "iOS must not reinterpret engine replies");
    let summary = call(urbudget_invoke, &json!({"version":1,"method":"calculate","budget":reply["value"],
        "params":{"month":"2026-10","asOfMonth":"2026-10"}}).to_string());
    assert_eq!(summary["ok"], true, "{summary}");
    assert!(summary["value"]["categories"].as_array().is_some_and(|rows| rows.len() == 10));
    // A template's categories get new ids each time, so the list is what's compared.
    let request = json!({"version":1,"method":"budgetTemplates","params":{}}).to_string();
    let reply = call(urbudget_invoke, &request);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply, serde_json::from_str::<Value>(&urbudget_android::invoke_json(&request)).unwrap());
    // The setup walkthrough's budget: new ids each time too, so what it holds is compared.
    let request = json!({"version":1,"method":"setupBudget","params":{"answers":{
        "currency":"USD","template":"simple","accounts":[{"name":"Visa","type":"creditCard","balance":42000}],
        "amounts":[{"category":"0.0","amount":120000,"period":"year"}]}}}).to_string();
    let reply = call(urbudget_invoke, &request);
    assert_eq!(reply["ok"], true, "{reply}");
    let budget = &reply["value"];
    assert_eq!(budget["accounts"][0]["openingBalance"], -42000);
    assert_eq!(budget["categories"][0]["target"], 10000);
    assert_eq!(budget["categories"].as_array().map(Vec::len), Some(8 + 1));
    assert_eq!(budget["categories"][8]["paymentAccountId"], budget["accounts"][0]["id"]);
}

#[test]
fn malformed_and_missing_requests_return_errors() {
    for request in ["not json", "{\"version\":2,\"method\":\"validate\"}"] {
        let reply = call(urbudget_invoke, request);
        assert_eq!(reply["ok"], false, "{reply}");
        assert!(reply["error"].as_str().is_some_and(|e| !e.is_empty()));
    }
    let reply: Value = unsafe {
        let output = urbudget_invoke(std::ptr::null());
        let text = CStr::from_ptr(output).to_str().unwrap().to_owned();
        urbudget_free(output);
        serde_json::from_str(&text).unwrap()
    };
    assert_eq!(reply["error"], "Missing request");
    unsafe { urbudget_free(std::ptr::null_mut()) };
}

// The runtime is one per process, so its whole life is one test.
#[test]
fn services_store_budgets_with_revision_checks_and_undo() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("library");
    let early = service("load_budget", json!({}));
    assert_eq!(early["ok"], false);
    let opened = call(urbudget_initialize, path.to_str().unwrap());
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(call(urbudget_initialize, path.to_str().unwrap())["ok"], true, "reopening the same folder is harmless");
    assert_eq!(call(urbudget_initialize, folder.path().to_str().unwrap())["ok"], false, "the folder cannot change");

    let initial = service("load_budget", json!({}));
    assert_eq!(initial["value"]["isNew"], true, "{initial}");
    let budget = call(urbudget_invoke, &json!({"version":1,"method":"sampleBudget","params":{"month":"2026-10"}}).to_string())["value"].clone();
    let saved = service("save_budget", json!({"budget":budget,"expectedToken":initial["value"]["token"],"ackOnly":true}));
    assert_eq!(saved["ok"], true, "{saved}");
    let stale = service("save_budget", json!({"budget":budget,"expectedToken":initial["value"]["token"]}));
    assert!(stale["error"].as_str().unwrap().contains("CONFLICT"), "{stale}");

    let mut renamed = budget.clone();
    renamed["name"] = json!("Renamed on iOS");
    let second = service("save_budget", json!({"budget":renamed,"expectedToken":saved["value"]["token"],"ackOnly":true}));
    assert_eq!(second["value"]["undoAvailable"], true, "{second}");
    let undone = service("undo_budget", json!({"expectedToken":second["value"]["token"]}));
    assert_eq!(undone["value"]["budget"]["name"], "Sample budget", "{undone}");
    assert_eq!(service("list_budgets", json!({}))["value"].as_array().unwrap().len(), 1);
}

#[test]
fn a_service_call_has_room_however_small_the_callers_stack() {
    // What a sync pass needs in an unoptimized build is far past the 512 KB a Swift concurrency
    // thread has; on that stack this would end the process, not fail the test.
    #[inline(never)]
    fn deep(levels: usize) -> usize {
        let frame = [0u8; 16 * 1024];
        std::hint::black_box(&frame);
        if levels == 0 { 0 } else { 1 + deep(levels - 1) }
    }
    let caller = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| super::on_service_stack(|| Ok(deep(128))))
        .unwrap();
    assert_eq!(caller.join().unwrap(), Ok(128));
    // A panic on that thread is an answer, as it is from `urbudget_service`, not a crash.
    let failed: Result<(), String> = super::on_service_stack(|| panic!("inside the service"));
    assert!(failed.is_err());
}
