use super::*;
use urbudget_core::test_support::{purchase, sample};

fn call(method: &str, budget: impl Serialize, params: Value) -> Value {
    serde_json::from_str(&invoke_json(
        &json!({"version":1,"method":method,"budget":budget,"params":params}).to_string(),
    ))
    .unwrap()
}
fn value(response: Value) -> Value {
    assert_eq!(response["ok"], true, "{response}");
    response["value"].clone()
}

#[test]
fn malformed_and_unsupported_requests_return_structured_errors() {
    for input in [
        "",
        "{",
        "null",
        r#"{"version":2,"method":"defaultBudget"}"#,
        r#"{"version":1,"method":"defaultBudget","params":[]}"#,
        r#"{"version":1,"method":"calculate"}"#,
    ] {
        let response: Value = serde_json::from_str(&invoke_json(input)).unwrap();
        assert_eq!(response["ok"], false, "{input}");
        assert!(response["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty()));
    }
    assert_eq!(
        call("calculate", sample(), json!({"month":"2026-1"}))["ok"],
        false
    );
    assert_eq!(
        call(
            "calculate",
            sample(),
            json!({"month":"2026-10","asOfMonth":"2026-13"})
        )["ok"],
        false
    );
}

#[test]
fn bridge_calculation_is_exactly_the_desktop_core_for_credit_and_transfers() {
    let mut budget = sample();
    budget.transactions.push(purchase());
    let mut transfer = purchase();
    transfer.id = "split-transfer".into();
    transfer.account_id = "cash".into();
    transfer.amount = -3000;
    transfer.splits = serde_json::from_value(json!([
        {"categoryId":null,"amount":-1000,"transferAccountId":"card","transferStatus":"uncleared"},
        {"categoryId":"food","amount":-2000}
    ]))
    .unwrap();
    budget.transactions.push(transfer);
    for month in ["2026-10", "2026-11", "2027-01"] {
        let direct = encode(engine::calculate_at(&budget, month, "2026-10").unwrap()).unwrap();
        let native = value(call(
            "calculate",
            &budget,
            json!({"month":month,"asOfMonth":"2026-10"}),
        ));
        assert_eq!(native, direct);
    }
    assert_eq!(
        value(call("ledger", &budget, json!({}))),
        encode(ledger::transactions(&budget)).unwrap()
    );
}

#[test]
fn reports_and_loan_projection_share_the_desktop_implementation() {
    let mut budget = sample();
    budget.transactions.push(purchase());
    let filter = json!({"from":"2026-10","to":"2026-12"});
    let direct =
        reports::calculate(&budget, &serde_json::from_value(filter.clone()).unwrap()).unwrap();
    assert_eq!(
        value(call("reports", &budget, filter)),
        encode(direct).unwrap()
    );
    let filter = json!({"from":"2026-10","to":"2026-12","dimension":"category","key":"food"});
    let direct = reports::details(&budget, &serde_json::from_value(filter.clone()).unwrap(), "category", "food").unwrap();
    assert_eq!(value(call("reportDetails", &budget, filter)), encode(direct).unwrap());
    let terms = loans::Terms {
        account_id: "loan".into(),
        category_id: "food".into(),
        annual_rate_basis_points: 500,
        monthly_payment: 10000,
        monthly_fees: 50,
        rate_history: vec![],
    };
    let direct = loans::project_with(100000, &terms, "2026-10-01", 500, 1000).unwrap();
    assert_eq!(
        value(call(
            "loanProjection",
            Value::Null,
            json!({"balance":100000,"terms":terms,"start":"2026-10-01","extra":500,"lump":1000})
        )),
        encode(direct).unwrap()
    );
}

#[test]
fn mutation_preserves_unknown_fields_and_reconciliation_uses_shared_ledger() {
    let mut original = encode(sample()).unwrap();
    original["futureClientSetting"] = json!({"nested":[1,2,3]});
    original["assignments"][0]["futureAssignmentNote"] = json!("keep");
    original["categories"][0]["futureCategorySetting"] = json!({"color":"blue"});
    let changed = value(call(
        "workflow",
        original.clone(),
        json!({"action":{"type":"moveMoney","month":"2026-10","from":"__rta","to":"food","amount":5000}}),
    ));
    assert_eq!(changed["assignments"][0]["amount"], 25000);
    assert_eq!(changed["assignments"][0]["futureAssignmentNote"], "keep");
    assert_eq!(
        changed["futureClientSetting"],
        original["futureClientSetting"]
    );
    assert_eq!(
        changed["categories"][0]["futureCategorySetting"],
        original["categories"][0]["futureCategorySetting"]
    );
    let reconciled = value(call(
        "reconcile",
        changed,
        json!({"accountId":"cash","balance":97500,"date":"2026-10-21","adjustmentId":"reconcile-1"}),
    ));
    let summary = value(call(
        "calculate",
        &reconciled,
        json!({"month":"2026-10","asOfMonth":"2026-10"}),
    ));
    assert_eq!(summary["accounts"][0]["cleared"], 97500);
    assert_eq!(summary["accounts"][0]["balance"], 97500);
    assert_eq!(reconciled["assignments"][0]["futureAssignmentNote"], "keep");
}

#[test]
fn schedules_preserve_extensions_and_do_not_post_twice() {
    let mut budget = encode(sample()).unwrap();
    let mut transaction = purchase();
    transaction.id = "template".into();
    budget["schedules"] = json!([{"id":"monthly","nextDate":"2026-10-15","anchorDate":"2026-10-15","frequency":"monthly","paused":false,"transaction":transaction,"futureReminder":"retain"}]);
    let first = value(call(
        "applySchedules",
        &budget,
        json!({"through":"2026-10-21"}),
    ));
    assert_eq!(first["count"], 1);
    assert_eq!(first["budget"]["schedules"][0]["futureReminder"], "retain");
    let second = value(call(
        "applySchedules",
        &first["budget"],
        json!({"through":"2026-10-21"}),
    ));
    assert_eq!(second["count"], 0);
    assert_eq!(first["budget"], second["budget"]);
}

#[test]
fn invalid_mutations_are_rejected_and_defaults_are_valid() {
    assert_eq!(
        call(
            "applyPatch",
            sample(),
            json!({"patch":{"assignments":{"upsert":[{"month":"2026-99","categoryId":"food","amount":1}]}}})
        )["ok"],
        false
    );
    assert_eq!(
        call(
            "applyPatch",
            sample(),
            json!({"patch":{"accounts":{"delete":["cash"]}}})
        )["ok"],
        true
    );
    let empty = value(call(
        "defaultBudget",
        Value::Null,
        json!({"currency":"USD","name":"Native"}),
    ));
    assert_eq!(empty["currency"], "USD");
    assert_eq!(empty["accounts"], json!([]));
    let demo = value(call(
        "sampleBudget",
        Value::Null,
        json!({"month":"2026-10"}),
    ));
    let summary = value(call(
        "calculate",
        demo,
        json!({"month":"2026-10","asOfMonth":"2026-10"}),
    ));
    assert_eq!(summary["readyToAssign"], 320000);
    assert_eq!(summary["accounts"][0]["balance"], 487500);
}

#[test]
fn saving_transactions_applies_rules_and_keeps_creation_time_on_edit() {
    let mut budget = sample();
    budget.extra.insert(
        "payeeRules".into(),
        json!([{"id":"market-rule","match":"Shop","payee":"Market","categoryId":"food"}]),
    );
    let mut transaction = purchase();
    transaction.splits[0].category_id = None;
    let first = value(call(
        "saveTransaction",
        &budget,
        json!({"transaction":transaction,"through":"2026-10-21"}),
    ));
    assert_eq!(first["transactions"][0]["payee"], "Market");
    assert_eq!(first["transactions"][0]["splits"][0]["categoryId"], "food");
    let created = first["transactions"][0]["createdAt"].clone();
    assert!(created.as_str().is_some());
    let mut edited = first["transactions"][0].clone();
    edited["createdAt"] = json!("2001-01-01T00:00:00.000Z");
    edited["memo"] = json!("Corrected");
    let second = value(call(
        "saveTransaction",
        &first,
        json!({"transaction":edited,"through":"2026-10-21"}),
    ));
    assert_eq!(second["transactions"][0]["createdAt"], created);
    assert_eq!(second["transactions"][0]["memo"], "Corrected");
    assert_eq!(second["transactions"].as_array().unwrap().len(), 1);
}

#[test]
fn workflow_reference_is_ephemeral_and_cannot_use_stale_imported_context() {
    let mut budget = sample();
    budget
        .extra
        .insert("calculationAsOfMonth".into(), json!("2025-01"));
    let params = json!({"asOfMonth":"2026-10","action":{"type":"autoAssign","month":"2026-10","mode":"underfunded","categories":["food"],"limitToAvailable":true,"expectedAsOfMonth":"2026-10"}});
    let (changed, reference) = apply_workflow(budget.clone(), &params, "2026-10").unwrap();
    assert_eq!(reference, "2026-10");
    assert_eq!(
        changed.extra["calculationAsOfMonth"], "2025-01",
        "ephemeral context never becomes a saved edit"
    );
    let mut expected = budget.clone();
    expected
        .extra
        .insert("calculationAsOfMonth".into(), json!("2026-10"));
    let direct = workflows::apply(expected, &params["action"]).unwrap();
    assert_eq!(changed.assignments, direct.assignments);
    assert!(apply_workflow(budget.clone(), &params, "2026-11")
        .unwrap_err()
        .contains("calendar month changed"));
    let mut stale = params.clone();
    stale["asOfMonth"] = json!("2026-11");
    assert!(apply_workflow(budget, &stale, "2026-11")
        .unwrap_err()
        .contains("calendar month changed"));
}

#[test]
fn workflow_preview_returns_explicit_summary_without_persisting_reference() {
    let current = engine::current_month();
    let budget = sample();
    let response = value(call(
        "workflow",
        &budget,
        json!({"asOfMonth":current,"previewMonth":"2026-10","action":{"type":"moveMoney","month":"2026-10","from":"__rta","to":"food","amount":5000}}),
    ));
    assert!(response["budget"].get("calculationAsOfMonth").is_none());
    let changed: domain::Budget = serde_json::from_value(response["budget"].clone()).unwrap();
    assert_eq!(
        response["summary"],
        encode(engine::calculate_at(&changed, "2026-10", &current).unwrap()).unwrap()
    );
}

#[test]
fn register_export_can_be_limited_to_listed_transactions_and_one_account() {
    let mut budget = sample();
    budget.transactions.push(purchase());
    let mut other = purchase();
    other.id = "other".into();
    other.payee = "Bakery".into();
    budget.transactions.push(other);
    let all = value(call("exportRegister", &budget, json!({})));
    assert_eq!(all, encode(ynab::export_register(&budget).unwrap()).unwrap());
    let listed = value(call("exportRegister", &budget, json!({"ids":["other"]})));
    let text = listed.as_str().unwrap();
    assert!(text.contains("Bakery"), "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
    let none = value(call("exportRegister", &budget, json!({"accountId":"no-such-account"})));
    assert_eq!(none.as_str().unwrap().lines().count(), 1);
}

#[test]
fn a_bank_file_import_is_a_session_the_phones_fix_and_review() {
    let content = "Date,Payee,Amount
2026-10-01,Bakery,-3.20
2026-10-02,Grocer,n/a
2026-10-03,Lunch,-8.50
";
    let base = json!({"accountId": "cash", "content": content});
    let with = |more: Value| {
        let mut params = base.clone();
        params.as_object_mut().unwrap().extend(more.as_object().unwrap().clone());
        params
    };
    let session = value(call("bankSession", sample(), with(json!({"sessionId": "s1", "budgetId": "b1"}))));
    assert_eq!(session["scope"]["account"], "cash");
    // Only the line that doesn't read, for a phone.
    let problems = value(call("bankStatement", sample(), with(json!({"session": session, "problemsOnly": true}))));
    let lines = problems["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["status"]["kind"], "attention");
    assert_eq!(problems["counts"]["ready"], 2);
    // Its amount given, it reads, and the review has all three.
    let fixes = json!({
        "scope": session["scope"], "baseRevision": 0, "baseStructure": session["structure"],
        "fields": [{"line": lines[0]["id"], "lineVersion": 0, "field": "amount", "from": "n/a", "to": "-12.40"}],
    });
    let applied = value(call("bankApplyFixes", sample(), with(json!({"session": session, "fixes": fixes}))));
    assert_eq!(applied["results"][0]["applied"], true);
    let preview = value(call("bankImportPreview", sample(), with(json!({"session": applied["session"]}))));
    let amounts: Vec<i64> = preview["review"]["items"].as_array().unwrap().iter().map(|i| i["amount"].as_i64().unwrap()).collect();
    assert_eq!(amounts, [-320, -1240, -850]);
    assert_eq!(preview["lines"].as_array().unwrap().len(), 3);
    // A phone's review carries the line it fixed (to undo), not every line.
    let short = value(call("bankImportPreview", sample(), with(json!({"session": applied["session"], "problemsOnly": true}))));
    assert_eq!(short["statement"]["lines"].as_array().unwrap().len(), 1);
    assert_eq!(short["statement"]["lines"][0]["fixed"], true);
    assert_eq!(short["review"]["items"].as_array().unwrap().len(), 3);
    // Another import's fix is refused.
    let mut elsewhere = fixes.clone();
    elsewhere["scope"]["session"] = json!("s2");
    assert_eq!(call("bankApplyFixes", sample(), with(json!({"session": session, "fixes": elsewhere})))["ok"], false);
    // A file the assistant prepared reads the standard way.
    let prepared = value(call("bankSession", sample(), with(json!({"sessionId": "s3", "prepared": true}))));
    assert_eq!(prepared["structure"]["dateOrder"], json!({"order": "ymd", "confirmed": true}));
    assert_eq!(prepared["structure"]["columns"]["amount"], "Amount");
}
