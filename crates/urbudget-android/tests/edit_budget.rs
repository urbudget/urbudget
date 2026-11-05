//! `edit_budget` against the two paths it replaces, the stateless engine and `save_budget`. One
//! budget goes through every command, and a copy patched the way a phone patches its own has to
//! stay equal to the store at each step.
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use urbudget_core::{commands, domain::Budget, engine, patch::Patch, store::Store, sync_storage};
use urbudget_services::{block_on, runtime::Runtime};

const MONTH: &str = "2026-10";
const CONFLICT: &str = "CONFLICT: The budget changed. Reload it before saving your edit.";

fn call(runtime: &Runtime, method: &str, p: Value) -> Result<Value, String> {
    block_on(runtime.dispatch(method, p))
}
fn ok(runtime: &Runtime, method: &str, p: Value) -> Value {
    call(runtime, method, p).unwrap_or_else(|error| panic!("{method}: {error}"))
}
fn token(runtime: &Runtime) -> String {
    ok(runtime, "native_budget_status", json!({}))["token"].as_str().unwrap().to_owned()
}
fn saved(runtime: &Runtime) -> Value {
    ok(runtime, "saved_budget", json!({}))["budget"].clone()
}
/// Everything a failed edit must leave alone: the status, stored rows, draft and undo.
fn state(runtime: &Runtime) -> Value {
    json!({"status":ok(runtime, "native_budget_status", json!({})),"saved":ok(runtime, "saved_budget", json!({})),
        "draft":store_order(ok(runtime, "snapshot_budget", json!({})))})
}
fn edit_at(runtime: &Runtime, expected: &str, steps: Value) -> Result<Value, String> {
    call(runtime, "edit_budget", json!({"steps":steps,"expectedToken":expected,"month":MONTH,"asOfMonth":MONTH}))
}
fn edit(runtime: &Runtime, steps: Value) -> Result<Value, String> {
    edit_at(runtime, &token(runtime), steps)
}
fn edited(runtime: &Runtime, steps: Value) -> Value {
    edit(runtime, steps).unwrap_or_else(|error| panic!("edit_budget: {error}"))
}
fn stateless(method: &str, budget: &Value, params: &Value) -> Value {
    let reply: Value = serde_json::from_str(&urbudget_android::invoke_json(
        &json!({"version":1,"method":method,"budget":budget,"params":params}).to_string(),
    ))
    .unwrap();
    assert_eq!(reply["ok"], true, "{method}: {}", reply["error"]);
    reply["value"].clone()
}
/// The budget in a changing method's reply.
fn changed_budget(reply: Value) -> Value {
    if reply.get("count").is_some() { reply["budget"].clone() } else { reply }
}
/// A runtime with `budget` saved and loaded.
fn opened(budget: &Value) -> (tempfile::TempDir, Runtime) {
    let folder = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    ok(&runtime, "save_budget", json!({"budget":budget,"expectedToken":""}));
    (folder, runtime)
}
fn recovery_file(runtime: &Runtime) -> PathBuf {
    let status = ok(runtime, "native_budget_status", json!({}));
    Path::new(status["databasePath"].as_str().unwrap()).with_file_name("recovery.json")
}
fn row(budget: &Value, id: &str) -> Value {
    budget["transactions"].as_array().unwrap().iter().find(|t| t["id"] == id).unwrap_or_else(|| panic!("no {id}")).clone()
}
fn revision(token: &str) -> i64 {
    token.split(':').nth(1).unwrap().parse().unwrap()
}
fn status_step(id: &str, status: &str) -> Value {
    json!({"method":"workflow","params":{"action":{"type":"bulk","ids":[id],"operation":"status","value":status,"accountId":"all"}}})
}
fn memo_step(id: &str, memo: &str) -> Value {
    json!({"method":"workflow","params":{"action":{"type":"bulk","ids":[id],"operation":"memo","value":memo}}})
}

/// Assignments as the store lists them. Nothing else has an order that may differ.
fn store_order(mut budget: Value) -> Value {
    let rows = budget["assignments"].as_array_mut().unwrap();
    rows.sort_by_key(|a| (a["month"].as_str().unwrap().to_owned(), a["categoryId"].as_str().unwrap().to_owned()));
    budget
}
/// Takes out what differs because two runs of a command happened at different times. A row that
/// was already there keeps every field: only the rows a command adds have a time of their own.
fn comparable(budget: &Value, known: &HashSet<String>) -> Value {
    let mut b = store_order(budget.clone());
    if let Some(moves) = b.get_mut("moneyMoves").and_then(Value::as_array_mut) {
        moves.iter_mut().for_each(|m| { m.as_object_mut().unwrap().remove("at"); });
    }
    for t in b["transactions"].as_array_mut().unwrap() {
        let id = t["id"].as_str().unwrap().to_owned();
        if known.contains(&id) { continue; }
        t.as_object_mut().unwrap().remove("createdAt");
        if id.starts_with("copy-") { t["id"] = json!(format!("copy-{}", id.rsplit('-').next().unwrap())); }
    }
    // A new row dated ahead goes straight into a schedule, its time with it.
    for s in b.get_mut("schedules").and_then(Value::as_array_mut).into_iter().flatten() {
        if !s["transaction"]["id"].as_str().is_some_and(|id| known.contains(id)) {
            s["transaction"].as_object_mut().unwrap().remove("createdAt");
        }
    }
    b
}
/// Where two budgets first differ, as a path: a whole budget in a failure message says nothing.
fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => x.keys().chain(y.keys()).find_map(|k| match (x.get(k), y.get(k)) {
            (Some(p), Some(q)) => first_difference(p, q, &format!("{path}.{k}")),
            _ => Some(format!("{path}.{k}: {:?} and {:?}", x.get(k), y.get(k))),
        }),
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x.iter().zip(y).enumerate().find_map(|(i, (p, q))| {
            let at = p.get("id").and_then(Value::as_str).map_or(i.to_string(), |id| format!("id={id}"));
            first_difference(p, q, &format!("{path}[{at}]"))
        }),
        _ => (a != b).then(|| format!("{path}: {a} and {b}")),
    }
}
fn assert_same(a: &Value, b: &Value, what: &str) {
    if let Some(difference) = first_difference(a, b, "budget") { panic!("{what}: {difference}"); }
}
fn ids(budget: &Value) -> HashSet<String> {
    budget["transactions"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect()
}

fn terms() -> Value {
    json!({"accountId":"loan","categoryId":"loan-cost","annualRateBasisPoints":500,"monthlyPayment":10000,"monthlyFees":50})
}
/// A budget that gives every command something to do, with fields no client models on every kind
/// of record, as a newer app would leave them.
fn fixture() -> Value {
    let ordinary = |id: &str, date: &str, account: &str, payee: &str, amount: i64, category: Value| {
        json!({"id":id,"date":date,"accountId":account,"payee":payee,"memo":"","amount":amount,"status":"cleared",
            "splits":[{"categoryId":category,"amount":amount}],"transferAccountId":null,"transferStatus":null,
            "createdAt":format!("{date}T09:00:00.000Z")})
    };
    let mut imported = ordinary("t-import", "2026-10-03", "cash", "Bakery", -2500, json!("food"));
    imported["approved"] = json!(false);
    imported["bankImportIds"] = json!(["bank-0"]);
    imported["source"] = json!({"id":"ynab-1","memo":"From the import","amount":-25000,"category_id":"ynab-food","category_name":"Food"});
    imported["futureTransactionField"] = json!(["keep", 1]);
    imported["splits"][0]["futureSplitField"] = json!({"keep":true});
    let mut split = ordinary("t-split", "2026-10-05", "cash", "Split shop", -700, json!(null));
    split["splits"] = json!([{"categoryId":"food","amount":-400},
        {"categoryId":null,"amount":-300,"transferAccountId":"card","transferStatus":"cleared"}]);
    let mut transfer = ordinary("t-transfer", "2026-10-06", "cash", "Savings", -2000, json!(null));
    transfer["splits"] = json!([]);
    transfer["transferAccountId"] = json!("savings");
    transfer["transferStatus"] = json!("cleared");
    let mut reconciled = ordinary("t-reconciled", "2026-09-20", "cash", "Rent", -40000, json!("rent"));
    reconciled["status"] = json!("reconciled");
    let mut legacy = ordinary("t-legacy", "2026-09-21", "cash", "Old row", -900, json!("fun"));
    legacy.as_object_mut().unwrap().remove("createdAt");
    let mut future = ordinary("t-future", "2026-12-25", "cash", "Gift", -1500, json!("fun"));
    future["status"] = json!("uncleared");
    let mut payment = ordinary("t-loanpay", "2026-10-12", "cash", "Loan", -10000, json!("loan-cost"));
    payment["transferAccountId"] = json!("loan");
    payment["transferStatus"] = json!("cleared");
    let schedule = |id: &str, date: &str, payee: &str, amount: i64| {
        json!({"id":id,"nextDate":date,"anchorDate":date,"frequency":"monthly","paused":false,"customSchedule":{"origin":"desktop"},
            "transaction":{"id":format!("template-{id}"),"date":date,"accountId":"cash","payee":payee,"memo":"","amount":amount,"status":"uncleared",
                "splits":[{"categoryId":"rent","amount":amount,"customSplit":"keep"}],"transferAccountId":null,"transferStatus":null,"customTransaction":"keep"}})
    };
    json!({
        "schemaVersion":2,"name":"Edits","currency":"SAR","futureClientSetting":{"nested":[1,2,3]},
        "accounts":[
            {"id":"cash","name":"Cash","kind":"cash","openingBalance":2_000_000,"archived":false,"futureAccountField":"keep"},
            {"id":"savings","name":"Savings","kind":"cash","openingBalance":500_000,"archived":false},
            {"id":"card","name":"Card","kind":"credit","openingBalance":-10_000,"archived":false},
            {"id":"loan","name":"Loan","kind":"tracking","openingBalance":-500_000,"archived":false}
        ],
        "categories":[
            {"id":"food","name":"Food","group":"Living","target":20_000,"futureCategorySetting":{"color":"blue"}},
            {"id":"fun","name":"Fun","group":"Living","target":0},
            {"id":"rent","name":"Rent","group":"Home","target":40_000,"goal":{"goal_type":"NEED","goal_cadence":1,"goal_cadence_frequency":1}},
            {"id":"old","name":"Old","group":"Home","target":0},
            {"id":"spare","name":"Spare","group":"Home","target":0},
            {"id":"loan-cost","name":"Loan cost","group":"Home","target":0},
            {"id":"payment","name":"Card payment","group":"Credit Card Payments","target":0,"paymentAccountId":"card"}
        ],
        "assignments":[
            {"month":"2026-09","categoryId":"food","amount":15_000},
            {"month":MONTH,"categoryId":"food","amount":20_000,"futureAssignmentNote":"keep"},
            {"month":MONTH,"categoryId":"fun","amount":9_000},
            {"month":MONTH,"categoryId":"rent","amount":40_000},
            {"month":MONTH,"categoryId":"old","amount":700},
            {"month":MONTH,"categoryId":"spare","amount":300}
        ],
        "transactions":[
            ordinary("t-income", "2026-09-01", "cash", "Salary", 1_000_000, json!(null)),
            ordinary("t-plain", "2026-10-02", "cash", "Bakery", -5000, json!("food")),
            imported,
            ordinary("t-card", "2026-10-04", "card", "Shop", -3000, json!("food")),
            ordinary("t-old", "2026-10-04", "cash", "Old shop", -700, json!("old")),
            split, transfer, reconciled, legacy,
            ordinary("t-stall", "2026-10-08", "cash", "Market stall", -600, json!(null)),
            future, payment
        ],
        "payees":[{"id":"payee-1","name":"Bakery","categoryId":"food"}],
        "payeeRules":[
            {"id":"rule-1","match":"Nothing like this","payee":"Unused","categoryId":"fun"},
            {"id":"rule-2","match":"Market stall","payee":"The market","categoryId":"food"}
        ],
        "loans":[terms()],
        "schedules":[schedule("rent", "2026-10-01", "Landlord", -40_000), schedule("gym", "2026-10-05", "Gym", -8_000)],
        "sourceExport":{"data":{"server_knowledge":7}}
    })
}

/// Every workflow type, every bulk operation and every other changing method but `applyPatch`,
/// in an order where each still has something to do.
fn commands_table() -> Vec<(&'static str, &'static str, Value)> {
    let flow = |action: Value| json!({"action":action});
    let bulk = |ids: Value, operation: &str, value: Value| flow(json!({"type":"bulk","ids":ids,"operation":operation,"value":value,"accountId":"all"}));
    let auto = |mode: &str| flow(json!({"type":"autoAssign","month":MONTH,"mode":mode,"categories":["food","fun"],"limitToAvailable":false}));
    let new_row = |id: &str, date: &str| json!({"id":id,"date":date,"accountId":"cash","payee":"Added","memo":"","amount":-120,"status":"uncleared",
        "splits":[{"categoryId":"fun","amount":-120}],"transferAccountId":null,"transferStatus":null});
    let mut whole = fixture()["transactions"][2].clone();
    assert_eq!(whole["id"], "t-import");
    whole["memo"] = json!("Edited with its source");
    let message = "Purchase SAR 12.34 at Bakery card *1234";
    let sms = json!({"messages":[{"id":"sms:0","sender":"Bank","receivedDate":"2026-10-20","receivedTime":"","body":message}],
        "analysis":{"items":[{"sourceIds":["sms:0"],"disposition":"transaction","skip":null,"note":"Purchase","evidence":[{"sourceId":"sms:0","quote":"SAR 12.34"}],"unsure":[],
            "transaction":{"date":"2026-10-20","amount":"-12.34","currency":"SAR","writtenAmount":"12.34","payee":"Bakery","memo":"","kind":"expense","accountRef":"*1234","otherRef":null,"categoryId":null,"accountId":"cash","transferAccountId":null}}]},
        "decisions":[{"index":0,"action":"new","matchId":null}]});
    vec![
        ("assign new", "workflow", flow(json!({"type":"assignAmount","month":"2026-11","categoryId":"food","amount":12_345}))),
        ("assign existing", "workflow", flow(json!({"type":"assignAmount","month":MONTH,"categoryId":"food","amount":25_000}))),
        ("assign zero", "workflow", flow(json!({"type":"assignAmount","month":MONTH,"categoryId":"fun","amount":0}))),
        ("planning", "workflow", flow(json!({"type":"setBudgetPlanning","month":MONTH,"note":"Holiday plan","expectedIncome":500_000}))),
        ("organize", "workflow", flow(json!({"type":"organizeCategory","id":"fun","group":"Everyday","beforeId":null,"groupBefore":null}))),
        ("rename group", "workflow", flow(json!({"type":"renameGroup","from":"Everyday","to":"Day to day"}))),
        ("delete into a category", "workflow", flow(json!({"type":"deleteCategory","id":"old","replacement":"food"}))),
        ("delete into ready to assign", "workflow", flow(json!({"type":"deleteCategory","id":"spare","replacement":"__rta"}))),
        ("underfunded", "workflow", auto("underfunded")),
        ("assigned last month", "workflow", auto("assignedLastMonth")),
        ("spent last month", "workflow", auto("spentLastMonth")),
        ("average assigned", "workflow", auto("averageAssigned")),
        ("average spent", "workflow", auto("averageSpent")),
        ("reduce overfunding", "workflow", auto("reduceOverfunding")),
        ("reset available", "workflow", auto("resetAvailable")),
        ("reset assigned", "workflow", auto("resetAssigned")),
        ("move money", "workflow", flow(json!({"type":"moveMoney","month":MONTH,"from":"__rta","to":"food","amount":100}))),
        ("rename payee", "workflow", flow(json!({"type":"renamePayee","from":"Bakery","to":"The Bakery"}))),
        ("save payee", "workflow", flow(json!({"type":"savePayee","record":{"id":"payee-2","name":"Butcher","categoryId":"food","futureField":17}}))),
        ("reorder rule", "workflow", flow(json!({"type":"reorderPayeeRule","id":"rule-2","direction":"up"}))),
        ("reapply rules", "workflow", flow(json!({"type":"reapplyPayeeRules"}))),
        ("bulk status", "workflow", bulk(json!(["t-plain"]), "status", json!("uncleared"))),
        ("bulk approve", "workflow", bulk(json!(["t-import"]), "approve", json!(null))),
        ("bulk flag", "workflow", bulk(json!(["t-plain"]), "flag", json!("red"))),
        ("bulk payee", "workflow", bulk(json!(["t-plain"]), "payee", json!("Renamed row"))),
        ("bulk memo", "workflow", bulk(json!(["t-plain", "t-import"]), "memo", json!("Bulk memo"))),
        ("bulk category", "workflow", bulk(json!(["t-plain"]), "category", json!("fun"))),
        ("bulk category, unassigned on an imported row", "workflow", bulk(json!(["t-import"]), "category", json!("__rta"))),
        ("bulk duplicate", "workflow", bulk(json!(["t-import"]), "duplicate", json!(null))),
        ("bulk delete", "workflow", bulk(json!(["t-card"]), "delete", json!(null))),
        ("new transaction", "saveTransaction", json!({"transaction":new_row("t-new", "2026-10-15")})),
        ("edited transaction", "saveTransaction", json!({"transaction":whole})),
        ("reconcile", "reconcile", json!({"accountId":"savings","balance":499_000,"date":"2026-10-21","adjustmentId":"adjustment-1"})),
        ("due schedules", "applySchedules", json!({"through":"2026-10-02"})),
        ("selected schedule", "applySchedules", json!({"through":"2026-10-20","selected":"gym"})),
        ("skipped schedule", "applySchedules", json!({"through":"2026-10-20","selected":"gym","skip":true})),
        ("schedule future", "scheduleFuture", json!({"through":"2026-10-21"})),
        ("new transaction through a date", "saveTransaction", json!({"transaction":new_row("t-ahead", "2026-12-01"),"through":"2026-10-21"})),
        ("loan payments", "applyLoanPayments", json!({"ids":["t-loanpay"]})),
        ("loan record", "loanRecord", json!({"terms":terms(),"from":"cash","amount":5000,"date":"2026-10-20","id":"loan-payment-1"})),
        ("bank import that remembers", "bankApply", json!({"accountId":"cash",
            "choices":[{"row":{"id":"bank-1","date":"2026-10-19","payee":"Cafe","memo":"","amount":-700},"action":"new","matchId":null}],
            "remember":{"dateFormat":"dmy","dateConfirmed":true}})),
        ("sms batch", "smsBatchApply", sms),
    ]
}

/// The whole table on one budget, against a copy that only ever gets the replies' patches. `strict`
/// also loads each step, though that resets the store's draft and would hide drift by itself.
fn every_command(strict: bool) {
    let (_folder, runtime) = opened(&fixture());
    let first = ok(&runtime, "load_budget", json!({}));
    let mut app: Budget = serde_json::from_value(first["budget"].clone()).unwrap();
    let mut states = vec![first["budget"].clone()];
    let table = commands_table();
    assert!(table.len() < 50, "undo keeps 50 steps");
    for (label, method, params) in table {
        let before = serde_json::to_value(&app).unwrap();
        let known = ids(&before);
        let expected = changed_budget(stateless(method, &before, &params));
        let reply = edited(&runtime, json!([{"method":method,"params":params}]));
        let patch: Patch = serde_json::from_value(reply["patch"].clone()).unwrap();
        assert!(!patch.is_empty(), "{label} did nothing, so it tests nothing");
        patch.apply(&mut app);

        let answer = ok(&runtime, "saved_budget", json!({}));
        let stored = answer["budget"].clone();
        assert_eq!(answer["unsavedDraft"], false, "{label}");
        assert_eq!(answer["token"], reply["token"], "{label}");
        assert_same(&store_order(ok(&runtime, "snapshot_budget", json!({}))), &stored, &format!("{label}: draft and rows"));
        assert_same(&store_order(serde_json::to_value(&app).unwrap()), &stored, &format!("{label}: the app's copy"));
        assert_same(&comparable(&expected, &known), &comparable(&stored, &known), &format!("{label}: stateless"));
        assert_eq!(reply["summary"], stateless("calculate", &stored, &json!({"month":MONTH,"asOfMonth":MONTH})), "{label}: totals");
        assert!(!recovery_file(&runtime).exists(), "{label}");
        if strict {
            assert_same(&ok(&runtime, "load_budget", json!({}))["budget"], &stored, &format!("{label}: loaded"));
        }
        states.push(stored);
    }
    assert_eq!(ok(&runtime, "load_budget", json!({}))["budget"], *states.last().unwrap());

    // Back through every command and forward again, to the same rows each time.
    for state in states.iter().rev().skip(1) {
        let undone = ok(&runtime, "undo_budget", json!({"expectedToken":token(&runtime)}));
        assert_eq!(undone["budget"], *state);
    }
    assert_eq!(ok(&runtime, "native_budget_status", json!({}))["undoAvailable"], false);
    for state in states.iter().skip(1) {
        let redone = ok(&runtime, "redo_budget", json!({"expectedToken":token(&runtime)}));
        assert_eq!(redone["budget"], *state);
    }
}

#[test]
fn command_matches_stateless_for_every_method() {
    every_command(false);
}

#[test]
fn command_matches_stateless_for_every_method_when_each_step_is_loaded() {
    every_command(true);
}

#[test]
fn preserve_extensions_is_a_no_op_for_commands() {
    let mut budget = serde_json::to_value(serde_json::from_value::<Budget>(fixture()).unwrap()).unwrap();
    for (label, method, params) in commands_table() {
        let known = ids(&budget);
        let reply = changed_budget(stateless(method, &budget, &params));
        let parsed: Budget = serde_json::from_value(budget.clone()).unwrap();
        let direct = commands::encode(commands::apply(parsed, method, &params).unwrap().budget).unwrap();
        assert_same(&comparable(&reply, &known), &comparable(&direct, &known), label);
        budget = reply;
    }
    assert_eq!(budget["assignments"].as_array().unwrap().iter().filter(|a| a["futureAssignmentNote"] == "keep").count(), 1);
    assert_eq!(budget["schedules"][0]["customSchedule"]["origin"], "desktop");
    // The one that needs it: a patch replaces a row whole.
    let params = json!({"patch":{"assignments":{"upsert":[{"month":MONTH,"categoryId":"food","amount":1}]}}});
    let original = fixture();
    let reply = stateless("applyPatch", &original, &params);
    let bare = commands::apply(serde_json::from_value(original).unwrap(), "applyPatch", &params).unwrap().budget;
    assert_eq!(reply["assignments"][1]["futureAssignmentNote"], "keep");
    assert!(!bare.assignments[1].extra.contains_key("futureAssignmentNote"));
}

#[test]
fn edit_budget_rows_match_save_budget() {
    let (_a, by_edit) = opened(&fixture());
    let (_b, by_save) = opened(&fixture());
    let table = commands_table();
    let some = ["assign existing", "rename payee", "bulk memo", "bulk delete", "new transaction", "reconcile", "due schedules"];
    let known = ids(&fixture());
    for (label, method, params) in table.into_iter().filter(|(label, ..)| some.contains(label)) {
        let current = saved(&by_save);
        let changed = changed_budget(stateless(method, &current, &params));
        let acknowledged = ok(&by_save, "save_budget", json!({"budget":changed,"expectedToken":token(&by_save),"ackOnly":true}));
        let reply = edited(&by_edit, json!([{"method":method,"params":params}]));
        assert_same(&comparable(&saved(&by_edit), &known), &comparable(&saved(&by_save), &known), label);
        assert_eq!(reply["undoAvailable"], acknowledged["undoAvailable"], "{label}");
        assert_eq!(reply["redoAvailable"], acknowledged["redoAvailable"], "{label}");
        assert_eq!(revision(reply["token"].as_str().unwrap()), revision(acknowledged["token"].as_str().unwrap()), "{label}");
    }
    for step in 0..some.len() {
        let undone = ok(&by_edit, "undo_budget", json!({"expectedToken":token(&by_edit)}));
        let other = ok(&by_save, "undo_budget", json!({"expectedToken":token(&by_save)}));
        assert_same(&comparable(&undone["budget"], &known), &comparable(&other["budget"], &known), &format!("undo {step}"));
        assert_eq!(undone["redoAvailable"], other["redoAvailable"]);
    }
    assert_eq!(comparable(&saved(&by_edit), &known), comparable(&serde_json::to_value(serde_json::from_value::<Budget>(fixture()).unwrap()).unwrap(), &known));
    let redone = ok(&by_edit, "redo_budget", json!({"expectedToken":token(&by_edit)}));
    let other = ok(&by_save, "redo_budget", json!({"expectedToken":token(&by_save)}));
    assert_eq!(comparable(&redone["budget"], &known), comparable(&other["budget"], &known));
}

#[test]
fn extensions_survive() {
    let folder = tempfile::tempdir().unwrap();
    let runtime = Runtime::new(folder.path()).unwrap();
    let initial = runtime.load().unwrap();
    let mut budget = serde_json::to_value(urbudget_core::test_support::sample()).unwrap();
    budget["assignments"][0]["customNote"] = json!({"legacyId":"a-17"});
    budget["schedules"] = json!([{
        "id":"planned","nextDate":"2026-10-21","anchorDate":"2026-10-21","frequency":"monthly","paused":false,
        "customSchedule":{"origin":"desktop"},
        "transaction":{"id":"template","date":"2026-10-21","accountId":"cash","payee":"Rent","memo":"","amount":-100,"status":"uncleared","splits":[{"categoryId":"food","amount":-100,"customSplit":"keep"}],"customTransaction":"keep"}
    }]);
    ok(&runtime, "save_budget", json!({"budget":budget,"expectedToken":initial["token"]}));
    let reply = edited(&runtime, json!([
        {"method":"applySchedules","params":{"through":"2026-10-21"}},
        {"method":"workflow","params":{"action":{"type":"assignAmount","month":MONTH,"categoryId":"food","amount":21_000}}}
    ]));
    assert_eq!(reply["patch"]["assignments"]["upsert"][0]["customNote"]["legacyId"], "a-17");
    drop(runtime);
    let reopened = Runtime::new(folder.path()).unwrap();
    let saved = reopened.load().unwrap();
    assert_eq!(saved["budget"]["assignments"][0]["amount"], 21_000);
    assert_eq!(saved["budget"]["assignments"][0]["customNote"]["legacyId"], "a-17");
    assert_eq!(saved["budget"]["schedules"][0]["customSchedule"]["origin"], "desktop");
    assert_eq!(saved["budget"]["schedules"][0]["transaction"]["customTransaction"], "keep");
    let posted = saved["budget"]["transactions"].as_array().unwrap().iter().find(|t| t["scheduleId"] == "planned").unwrap().clone();
    assert_eq!(posted["customTransaction"], "keep");
    assert_eq!(posted["splits"][0]["customSplit"], "keep");
}

#[test]
fn steps_commit_once_with_one_undo() {
    let (_folder, runtime) = opened(&fixture());
    let before = ok(&runtime, "load_budget", json!({}));
    let start = before["token"].as_str().unwrap();
    let new_row = json!({"id":"t-new","date":"2026-10-15","accountId":"cash","payee":"Added","memo":"","amount":-120,"status":"uncleared",
        "splits":[{"categoryId":"fun","amount":-120}],"transferAccountId":null,"transferStatus":null});
    let reply = edit_at(&runtime, start, json!([
        {"patch":{"name":"Renamed in a step"}},
        {"method":"saveTransaction","params":{"transaction":new_row}},
        {"method":"applySchedules","params":{"through":"2026-10-02"}}
    ])).unwrap();
    assert_eq!(revision(reply["token"].as_str().unwrap()), revision(start) + 1, "one commit");
    assert_eq!(reply["undoAvailable"], true);
    assert_eq!(reply["patch"]["name"], "Renamed in a step");
    let upserted: HashSet<_> = reply["patch"]["transactions"]["upsert"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(upserted, HashSet::from(["t-new", "scheduled:rent:2026-10-01"]));
    let done = saved(&runtime);

    let undone = ok(&runtime, "undo_budget", json!({"expectedToken":reply["token"]}));
    assert_eq!(undone["budget"], before["budget"], "one undo takes the whole edit back");
    assert_eq!(undone["undoAvailable"], false);
    assert_eq!(undone["redoAvailable"], true);
    let redone = ok(&runtime, "redo_budget", json!({"expectedToken":undone["token"]}));
    assert_eq!(redone["budget"], done);
}

#[test]
fn no_recovery_file_after_success() {
    let (_folder, runtime) = opened(&fixture());
    edited(&runtime, json!([status_step("t-plain", "uncleared")]));
    assert!(!recovery_file(&runtime).exists());
    assert_eq!(ok(&runtime, "read_recovery", json!({})), Value::Null);
}

/// What sync does to the open budget: a real change on a connection of its own, which neither the
/// app nor the open store is told of. Android's tests can't write these rows from their own SQLite,
/// because the tables only take writes through the engine's `urbudget_storage_protocol`.
#[test]
fn a_second_connections_write_reaches_the_next_edit() {
    let (_folder, runtime) = opened(&fixture());
    let before = token(&runtime);
    let status = ok(&runtime, "native_budget_status", json!({}));
    let path = PathBuf::from(status["databasePath"].as_str().unwrap());
    let id = path.parent().unwrap().file_name().unwrap().to_str().unwrap().to_owned();
    let mut second = Store::open_budget(&path, &id).unwrap();
    let (mut budget, status) = second.saved().unwrap();
    budget.transactions.iter_mut().find(|t| t.id == "t-card").unwrap().memo = "From sync".into();
    second.save(&budget, &status.token).unwrap();
    drop(second);

    // The app's edit is refused as stale, and nothing of it lands.
    let refused = edit_at(&runtime, &before, json!([memo_step("t-plain", "Tapped")])).unwrap_err();
    assert_eq!(refused, CONFLICT);
    assert_eq!(row(&saved(&runtime), "t-plain")["memo"], "");
    // Reloaded, as the app does, the same edit lands on top of the other writer's.
    let loaded = ok(&runtime, "load_budget", json!({}));
    assert_eq!(row(&loaded["budget"], "t-card")["memo"], "From sync");
    let reply = edit_at(&runtime, loaded["token"].as_str().unwrap(), json!([memo_step("t-plain", "Tapped")])).unwrap();
    let stored = saved(&runtime);
    assert_eq!(row(&stored, "t-plain")["memo"], "Tapped");
    assert_eq!(row(&stored, "t-card")["memo"], "From sync");
    assert_eq!(reply["patch"]["transactions"]["upsert"].as_array().unwrap().len(), 1, "the patch holds the edit alone");
}

#[test]
fn recovery_and_clean_draft_on_failed_commit() {
    let (_folder, runtime) = opened(&fixture());
    edited(&runtime, json!([memo_step("t-card", "An edit to undo")]));
    let before = saved(&runtime);
    let path = ok(&runtime, "native_budget_status", json!({}))["databasePath"].as_str().unwrap().to_owned();
    let external = rusqlite::Connection::open(path).unwrap();
    external.execute_batch("CREATE TRIGGER reject_native_save BEFORE UPDATE ON metadata BEGIN SELECT RAISE(ABORT, 'Simulated disk failure'); END;").unwrap();
    let failing = json!([status_step("t-plain", "uncleared")]);
    assert!(edit(&runtime, failing.clone()).unwrap_err().contains("Simulated disk failure"));

    let answer = ok(&runtime, "saved_budget", json!({}));
    assert_eq!(answer["budget"], before);
    assert_eq!(answer["unsavedDraft"], false);
    assert_eq!(store_order(ok(&runtime, "snapshot_budget", json!({}))), before, "the draft holds no failed edit");
    let recovery = ok(&runtime, "read_recovery", json!({}));
    assert_eq!(row(&recovery["budget"], "t-plain")["status"], "uncleared", "the failed edit stays recoverable");
    assert!(recovery_file(&runtime).exists());
    assert_eq!(ok(&runtime, "native_budget_status", json!({}))["undoAvailable"], false, "the trigger moved the token, as sync would");

    external.execute_batch("DROP TRIGGER reject_native_save;").unwrap();
    let reply = edited(&runtime, failing);
    assert_eq!(row(&saved(&runtime), "t-plain")["status"], "uncleared");
    assert_eq!(reply["undoAvailable"], true);
    assert!(!recovery_file(&runtime).exists(), "a successful edit clears a failed one's file");
}

#[test]
fn failed_command_leaves_token_draft_and_rows() {
    let (_folder, runtime) = opened(&fixture());
    edited(&runtime, json!([memo_step("t-card", "An edit to undo")]));
    let before = state(&runtime);
    let bad = json!({"method":"workflow","params":{"action":{"type":"assignAmount","month":MONTH,"categoryId":"nothing","amount":1}}});
    for steps in [
        json!([bad]),
        // An earlier step that worked goes with it.
        json!([status_step("t-plain", "uncleared"), bad]),
        json!([{"patch":{"name":"Renamed"}}, {"method":"saveTransaction","params":{"transaction":{"id":"broken"}}}]),
        // Bad rows in a patch, caught by the step's own check.
        json!([{"patch":{"assignments":{"upsert":[{"month":"2026-99","categoryId":"food","amount":1}]}}}]),
        json!([{"patch":{"transactions":{"delete":[],"upsert":[{"id":"t-x","date":"2026-10-01","accountId":"nowhere","payee":"","memo":"","amount":-1,"status":"cleared","splits":[{"categoryId":null,"amount":-1}],"transferAccountId":null,"transferStatus":null}]}}}]),
    ] {
        edit(&runtime, steps.clone()).expect_err(&steps.to_string());
        assert_eq!(state(&runtime), before, "{steps}");
        assert!(!recovery_file(&runtime).exists(), "{steps}");
    }
    assert!(edit(&runtime, json!([bad])).unwrap_err().contains("Choose a valid category and month."), "the engine's own message");
}

#[test]
fn totals_failure_saves_nothing() {
    let (_folder, runtime) = opened(&fixture());
    let before = state(&runtime);
    let refused = call(&runtime, "edit_budget", json!({"steps":[status_step("t-plain", "uncleared")],"expectedToken":token(&runtime),"month":"2026-13","asOfMonth":MONTH}));
    assert!(refused.is_err());
    assert_eq!(state(&runtime), before);
    assert!(!recovery_file(&runtime).exists());
}

#[test]
fn conflict_on_stale_token() {
    let (_folder, runtime) = opened(&fixture());
    let stale = token(&runtime);
    edited(&runtime, json!([status_step("t-plain", "uncleared")]));
    let before = state(&runtime);
    assert_eq!(edit_at(&runtime, &stale, json!([memo_step("t-card", "Stale")])).unwrap_err(), CONFLICT);
    assert_eq!(state(&runtime), before);
}

#[test]
fn stale_cache_reloads_rather_than_conflicts() {
    let (_folder, runtime) = opened(&fixture());
    let start = token(&runtime);
    let path = ok(&runtime, "native_budget_status", json!({}))["databasePath"].as_str().unwrap().to_owned();
    {
        // Another connection writes, as background sync does: the token moves, the cache doesn't.
        let mut other = Store::open_budget(Path::new(&path), start.split(':').next().unwrap()).unwrap();
        let (mut budget, status) = other.saved().unwrap();
        budget.name = "Renamed elsewhere".into();
        other.save(&budget, &status.token).unwrap();
    }
    let moved = token(&runtime);
    assert_ne!(moved, start);
    assert_eq!(edit_at(&runtime, &start, json!([memo_step("t-card", "Stale")])).unwrap_err(), CONFLICT);
    edit_at(&runtime, &moved, json!([memo_step("t-card", "On the fresh rows")])).unwrap();
    let stored = saved(&runtime);
    assert_eq!(stored["name"], "Renamed elsewhere");
    assert_eq!(row(&stored, "t-card")["memo"], "On the fresh rows");
}

#[test]
fn closed_store_replies_conflict() {
    let (_folder, runtime) = opened(&fixture());
    let open = token(&runtime);
    runtime.db.replace(None);
    assert_eq!(edit_at(&runtime, &open, json!([])).unwrap_err(), CONFLICT);
    assert_eq!(edit_at(&runtime, "", json!([])).unwrap_err(), CONFLICT);
}

#[test]
fn blocked_budget_refused_with_draft_unchanged() {
    let (_folder, runtime) = opened(&fixture());
    runtime.db.store().unwrap().set_sync_blocked(true).unwrap();
    let before = state(&runtime);
    let refused = edit(&runtime, json!([status_step("t-plain", "uncleared")])).unwrap_err();
    assert_eq!(refused, "This budget was removed from its account. Recover your changes as a copy before editing.");
    assert_eq!(state(&runtime), before);
}

#[test]
fn reply_has_every_ack_field() {
    let (_folder, runtime) = opened(&fixture());
    let reply = edited(&runtime, json!([status_step("t-plain", "uncleared")]));
    let status = ok(&runtime, "native_budget_status", json!({}));
    for key in ["token", "savedAt", "databasePath", "isNew", "undoAvailable", "redoAvailable"] {
        assert!(reply.get(key).is_some(), "{key}");
        assert_eq!(reply[key], status[key], "{key}");
    }
    assert_eq!(reply["isNew"], false);
    assert_eq!(reply["undoAvailable"], true);
    assert_eq!(reply["redoAvailable"], false);
    assert_eq!(reply["patch"]["transactions"]["upsert"][0]["id"], "t-plain");
    let stored = saved(&runtime);
    assert_eq!(reply["summary"], stateless("calculate", &stored, &json!({"month":MONTH,"asOfMonth":MONTH})));
    // Without asOfMonth, as `calculate` goes without it.
    let plain = ok(&runtime, "edit_budget", json!({"steps":[],"expectedToken":reply["token"],"month":MONTH}));
    assert_eq!(plain["summary"], stateless("calculate", &stored, &json!({"month":MONTH})));
}

#[test]
fn undo_survives_native_budget_status() {
    let (_folder, runtime) = opened(&fixture());
    let before = ok(&runtime, "load_budget", json!({}));
    let reply = edited(&runtime, json!([status_step("t-plain", "uncleared")]));
    // A probe with a token the history doesn't know would throw the undo away.
    let probe = ok(&runtime, "native_budget_status", json!({}));
    assert_eq!(probe["undoAvailable"], true);
    let undone = ok(&runtime, "undo_budget", json!({"expectedToken":reply["token"]}));
    assert_eq!(undone["budget"], before["budget"]);
}

#[test]
fn empty_patch_keeps_token_and_undo() {
    let (_folder, runtime) = opened(&fixture());
    edited(&runtime, json!([status_step("t-plain", "uncleared")]));
    let second = edited(&runtime, json!([memo_step("t-plain", "Second")]));
    let undone = ok(&runtime, "undo_budget", json!({"expectedToken":second["token"]}));
    assert_eq!(undone["redoAvailable"], true);
    for steps in [json!([]), json!([{"patch":{}}]), json!([status_step("t-plain", "uncleared")])] {
        let reply = edit_at(&runtime, undone["token"].as_str().unwrap(), steps.clone()).unwrap();
        assert_eq!(reply["patch"], json!({}), "{steps}");
        assert_eq!(reply["token"], undone["token"], "{steps}");
        assert_eq!(reply["undoAvailable"], true, "{steps}");
        assert_eq!(reply["redoAvailable"], true, "{steps}");
        assert!(reply["summary"].is_object(), "{steps}");
    }
    ok(&runtime, "redo_budget", json!({"expectedToken":undone["token"]}));
    assert_eq!(row(&saved(&runtime), "t-plain")["memo"], "Second");
}

#[test]
fn source_and_source_export_survive_edit() {
    let (_folder, runtime) = opened(&fixture());
    let original = saved(&runtime);
    let imported = row(&original, "t-import");
    let source = imported["source"].clone();
    let without_source = |memo: &str| {
        let mut light = imported.clone();
        let fields = light.as_object_mut().unwrap();
        fields.remove("source");
        fields.insert("hasImportSource".into(), json!(true));
        light["memo"] = json!(memo);
        light
    };
    // A patch and a command, each from a copy without `source`.
    let reply = edited(&runtime, json!([{"patch":{"transactions":{"upsert":[without_source("In a patch")]}}}]));
    assert_eq!(reply["patch"]["transactions"]["upsert"][0]["source"], source, "the reply's rows are as stored");
    let stored = row(&saved(&runtime), "t-import");
    assert_eq!((stored["memo"].as_str(), &stored["source"]), (Some("In a patch"), &source));
    assert!(stored.get("hasImportSource").is_none());
    edited(&runtime, json!([{"method":"saveTransaction","params":{"transaction":without_source("In a command")}}]));
    let stored = row(&saved(&runtime), "t-import");
    assert_eq!((stored["memo"].as_str(), &stored["source"]), (Some("In a command"), &source));
    assert!(stored.get("hasImportSource").is_none());

    // Deleted, the row and its source come back with undo.
    edited(&runtime, json!([{"method":"workflow","params":{"action":{"type":"bulk","ids":["t-import"],"operation":"delete"}}}]));
    assert!(saved(&runtime)["transactions"].as_array().unwrap().iter().all(|t| t["id"] != "t-import"));
    ok(&runtime, "undo_budget", json!({"expectedToken":token(&runtime)}));
    assert_eq!(row(&saved(&runtime), "t-import")["source"], source);

    let stored = saved(&runtime);
    assert_eq!(stored["sourceExport"], original["sourceExport"]);
    assert_eq!(ok(&runtime, "snapshot_budget", json!({}))["sourceExport"], original["sourceExport"]);
    ok(&runtime, "undo_budget", json!({"expectedToken":token(&runtime)}));
    ok(&runtime, "undo_budget", json!({"expectedToken":token(&runtime)}));
    assert_eq!(saved(&runtime), original, "both edits undone, byte for byte");
}

#[test]
fn a_step_cant_touch_heavy_extras_or_use_apply_patch() {
    let (_folder, runtime) = opened(&fixture());
    let before = state(&runtime);
    for (steps, message) in [
        (json!([{"method":"applyPatch","params":{"patch":{"name":"Nope"}}}]), "Send a patch as a patch step."),
        (json!([{"patch":{"extras":{"set":{"sourceExport":{}}}}}]), "A step can't change heavy extras."),
        (json!([{"patch":{"extras":{"delete":["sourceExport"]}}}]), "A step can't change heavy extras."),
        (json!([{"method":"reports","params":{}}]), "Unknown engine method: reports"),
    ] {
        assert_eq!(edit(&runtime, steps.clone()).unwrap_err(), message, "{steps}");
        assert_eq!(state(&runtime), before, "{steps}");
    }
    assert!(edit(&runtime, json!([{"patch":{},"method":"workflow"}])).unwrap_err().starts_with("Invalid steps"));
}

#[test]
fn the_calendar_guard_reads_only_the_commands_own_month() {
    let (_folder, runtime) = opened(&fixture());
    // Totals asked for another month: a tap at midnight must not fail on them.
    let assign = |params: Value| json!([{"method":"workflow","params":params}]);
    let action = json!({"type":"assignAmount","month":MONTH,"categoryId":"food","amount":21_000});
    let current = engine::current_month();
    assert_ne!(current, "1999-01");
    let reply = ok(&runtime, "edit_budget", json!({"steps":assign(json!({"action":action})),"expectedToken":token(&runtime),"month":MONTH,"asOfMonth":"1999-01"}));
    assert_eq!(reply["summary"], stateless("calculate", &saved(&runtime), &json!({"month":MONTH,"asOfMonth":"1999-01"})));
    // A command that names its month is held to it.
    let stale = call(&runtime, "edit_budget", json!({"steps":assign(json!({"action":action,"asOfMonth":"1999-01"})),"expectedToken":token(&runtime),"month":MONTH}));
    assert!(stale.unwrap_err().contains("calendar month changed"));
}

#[test]
fn a_new_row_in_a_patch_gets_the_time_it_was_added() {
    let (_folder, runtime) = opened(&fixture());
    let new_row = json!({"id":"t-patched","date":"2026-10-15","accountId":"cash","payee":"Added","memo":"","amount":-120,"status":"uncleared",
        "splits":[{"categoryId":"fun","amount":-120}],"transferAccountId":null,"transferStatus":null});
    let reply = edited(&runtime, json!([{"patch":{"transactions":{"upsert":[new_row]}}}]));
    let sent = &reply["patch"]["transactions"]["upsert"][0];
    assert!(sent["createdAt"].is_string());
    assert_eq!(row(&saved(&runtime), "t-patched")["createdAt"], sent["createdAt"], "the app gets the time the store has");
}

/// `id` as a copy without hidden fields sends it, with a new memo.
fn light_row(budget: &Value, id: &str, memo: &str) -> Value {
    let mut light = row(budget, id);
    let fields = light.as_object_mut().unwrap();
    fields.remove("source");
    fields.insert("hasImportSource".into(), json!(true));
    fields.insert("memo".into(), json!(memo));
    light
}

#[test]
fn reply_totals_are_those_of_the_budget_as_stored() {
    // The card holds exactly what `t-card` spends, so whichever of two purchases on that day comes
    // first takes the cash and leaves the other as debt. A row with no time sorts first until it
    // has one, and a row added by a patch gets its time just before the commit.
    let mut budget = fixture();
    budget["accounts"][2]["openingBalance"] = json!(3000);
    let (_folder, runtime) = opened(&budget);
    let added = json!({"id":"t-added","date":"2026-10-04","accountId":"card","payee":"Added","memo":"","amount":-3000,"status":"uncleared",
        "splits":[{"categoryId":"old","amount":-3000}],"transferAccountId":null,"transferStatus":null});
    let reply = edited(&runtime, json!([{"patch":{"transactions":{"upsert":[added]}}}]));
    let stored = saved(&runtime);
    let params = json!({"month":MONTH,"asOfMonth":MONTH});
    assert_eq!(reply["summary"], stateless("calculate", &stored, &params));
    let mut unstamped = stored;
    let rows = unstamped["transactions"].as_array_mut().unwrap();
    rows.iter_mut().find(|t| t["id"] == "t-added").unwrap().as_object_mut().unwrap().remove("createdAt");
    assert_ne!(reply["summary"], stateless("calculate", &unstamped, &params), "the time has to matter here");
}

#[test]
fn a_refused_edit_leaves_no_source_behind() {
    for refused in [
        // Refused while the steps run, and before they run.
        json!({"patch":{"assignments":{"upsert":[{"month":"2026-99","categoryId":"food","amount":1}]}}}),
        json!({"patch":{"extras":{"delete":["sourceExport"]}}}),
        json!({"method":"applyPatch","params":{}}),
    ] {
        let (_folder, runtime) = opened(&fixture());
        let original = saved(&runtime);
        let source = row(&original, "t-import")["source"].clone();
        let mut changed = row(&original, "t-import");
        changed["source"] = json!({"id":"from-a-refused-edit"});
        let refuse = || {
            let before = state(&runtime);
            assert!(edit(&runtime, json!([{"patch":{"transactions":{"upsert":[changed]}}}, refused])).is_err(), "{refused}");
            assert_eq!(state(&runtime), before, "{refused}");
        };
        // Nothing reloads between these, so what the refused edit left would fill in `source`.
        refuse();
        edited(&runtime, json!([{"method":"saveTransaction","params":{"transaction":light_row(&original, "t-import", "By a command")}}]));
        assert_eq!(row(&saved(&runtime), "t-import")["source"], source, "{refused}");
        refuse();
        let mut whole = saved(&runtime);
        let copy = whole["transactions"].as_array_mut().unwrap().iter_mut().find(|t| t["id"] == "t-import").unwrap();
        *copy = light_row(&original, "t-import", "By a whole save");
        ok(&runtime, "save_budget", json!({"budget":whole,"expectedToken":token(&runtime),"ackOnly":true}));
        let stored = row(&saved(&runtime), "t-import");
        assert_eq!((stored["memo"].as_str(), &stored["source"]), (Some("By a whole save"), &source), "{refused}");
    }
}

#[test]
fn a_step_finds_the_source_an_earlier_step_sent() {
    let (_folder, runtime) = opened(&fixture());
    let original = saved(&runtime);
    let mut changed = row(&original, "t-import");
    changed["source"] = json!({"id":"sent-by-a-step"});
    let save = |memo: &str| json!({"method":"saveTransaction","params":{"transaction":light_row(&original, "t-import", memo)}});
    edited(&runtime, json!([{"patch":{"transactions":{"upsert":[changed]}}}, save("Same edit")]));
    let stored = row(&saved(&runtime), "t-import");
    assert_eq!((stored["memo"].as_str(), stored["source"]["id"].as_str()), (Some("Same edit"), Some("sent-by-a-step")));
    // Committed, it's the source later copies get back.
    edited(&runtime, json!([save("Next edit")]));
    let stored = row(&saved(&runtime), "t-import");
    assert_eq!((stored["memo"].as_str(), stored["source"]["id"].as_str()), (Some("Next edit"), Some("sent-by-a-step")));
}

#[test]
fn sync_right_after_an_edit_succeeds() {
    let (_folder, runtime) = opened(&fixture());
    let reply = edited(&runtime, json!([status_step("t-plain", "uncleared")]));
    let mut store = runtime.db.store().unwrap();
    assert!(!store.has_unsaved_draft());
    let (mut budget, status) = store.saved().unwrap();
    assert_eq!(status.token, reply["token"].as_str().unwrap());
    budget.name = "From sync".into();
    let merged = store.save_synced(&budget, &status.token, 7, &budget).unwrap();
    assert!(!store.has_unsaved_draft());
    let posted = store.run_schedules(&merged.token, "2026-10-02").unwrap();
    assert!(posted.budget.transactions.iter().any(|t| t.id == "scheduled:rent:2026-10-01"));
    assert!(!store.has_unsaved_draft());
    drop(store);
    // And neither of them holds up the next edit.
    edited(&runtime, json!([memo_step("t-card", "After sync")]));
    let stored = saved(&runtime);
    assert_eq!((stored["name"].as_str(), row(&stored, "t-card")["memo"].as_str()), (Some("From sync"), Some("After sync")));
}

#[test]
fn sync_dirty_and_revision_advance() {
    let (_folder, runtime) = opened(&fixture());
    let start = {
        let mut store = runtime.db.store().unwrap();
        let (mut base, status) = store.saved().unwrap();
        base.extra.remove("sourceExport");
        store.bind_sync(&sync_storage::Binding {
            identity: sync_storage::RemoteIdentity { account_id: "account".into(), key_generation: "1".into(), catalog_epoch: "epoch".into(),
                budget_id: "remote".into(), incarnation: "one".into() },
            remote_version: "1".into(), acknowledged_local_revision: status.revision, base,
        }).unwrap();
        assert!(!store.sync_dirty().unwrap());
        status
    };
    // Nothing changed, so there's nothing to send.
    edited(&runtime, json!([]));
    {
        let store = runtime.db.store().unwrap();
        assert!(!store.sync_dirty().unwrap());
        assert_eq!(store.revision().unwrap(), start.revision);
    }
    edited(&runtime, json!([status_step("t-plain", "uncleared")]));
    let store = runtime.db.store().unwrap();
    assert!(store.sync_dirty().unwrap());
    assert_eq!(store.revision().unwrap(), start.revision + 1);
}

fn edited_light(runtime: &Runtime, steps: Value) -> Value {
    ok(runtime, "edit_budget", json!({"steps":steps,"expectedToken":token(runtime),"month":MONTH,"asOfMonth":MONTH,"light":true}))
}
fn history_light(runtime: &Runtime, method: &str) -> Value {
    ok(runtime, method, json!({"patchOnly":true,"expectedToken":token(runtime),"month":MONTH,"asOfMonth":MONTH,"light":true}))
}
fn assert_light(row: &Value) {
    assert!(row.get("source").is_none(), "{row}");
    assert_eq!(row["hasImportSource"], true, "{row}");
}

#[test]
fn light_load_has_no_source_or_heavy_and_keeps_undo() {
    let (_folder, runtime) = opened(&fixture());
    let original = saved(&runtime);
    edited(&runtime, json!([memo_step("t-plain", "Before the load")]));
    let light = ok(&runtime, "load_budget", json!({"light":true}));
    let budget = &light["budget"];
    assert!(budget.get("sourceExport").is_none());
    assert_light(&row(budget, "t-import"));
    assert!(row(budget, "t-plain").get("hasImportSource").is_none(), "only an imported row has the hint");
    let whole = ok(&runtime, "load_budget", json!({}));
    for key in ["token", "savedAt", "databasePath", "isNew", "undoAvailable", "redoAvailable"] {
        assert_eq!(light[key], whole[key], "{key}");
    }
    assert_eq!(light["undoAvailable"], true);
    // Nothing a light load does costs the store anything it holds.
    history_light(&runtime, "undo_budget");
    assert_eq!(saved(&runtime), original);
}

#[test]
fn default_load_stays_complete() {
    let (_folder, runtime) = opened(&fixture());
    ok(&runtime, "load_budget", json!({"light":true}));
    let whole = ok(&runtime, "load_budget", json!({}));
    assert_eq!(whole["budget"]["sourceExport"], fixture()["sourceExport"]);
    let imported = row(&whole["budget"], "t-import");
    assert_eq!(imported["source"], row(&fixture(), "t-import")["source"]);
    assert!(imported.get("hasImportSource").is_none());
}

#[test]
fn light_patch_reply_hides_source_but_history_keeps_it() {
    let (_folder, runtime) = opened(&fixture());
    let original = saved(&runtime);
    let reply = edited_light(&runtime, json!([memo_step("t-import", "Changed")]));
    assert_light(&reply["patch"]["transactions"]["upsert"][0]);
    edited_light(&runtime, json!([{"method":"workflow","params":{"action":{"type":"bulk","ids":["t-import"],"operation":"delete"}}}]));

    // Back from the undo stack: hidden in the reply, whole in the store.
    let undone = history_light(&runtime, "undo_budget");
    assert_light(&undone["patch"]["transactions"]["upsert"][0]);
    assert_eq!(row(&saved(&runtime), "t-import")["source"], row(&original, "t-import")["source"]);
    let undone = history_light(&runtime, "undo_budget");
    assert_light(&undone["patch"]["transactions"]["upsert"][0]);
    assert_eq!(saved(&runtime), original);
    let redone = history_light(&runtime, "redo_budget");
    assert_light(&redone["patch"]["transactions"]["upsert"][0]);
    assert_eq!(row(&saved(&runtime), "t-import")["source"], row(&original, "t-import")["source"]);
    history_light(&runtime, "undo_budget");

    // A heavy change: a light copy holds none of it, so the reply says nothing changed.
    let mut newer = original.clone();
    newer["sourceExport"] = json!({"data":{"server_knowledge":8}});
    let at = token(&runtime);
    ok(&runtime, "save_budget", json!({"budget":newer,"expectedToken":at,"ackOnly":true}));
    let undone = history_light(&runtime, "undo_budget");
    assert_eq!(undone["patch"], json!({}));
    assert_ne!(undone["token"], json!(at));
    assert_eq!(saved(&runtime), original, "the undo still put the old export back");
}

#[test]
fn light_save_transaction_restores_source() {
    let (_folder, runtime) = opened(&fixture());
    let original = saved(&runtime);
    let sent = light_row(&original, "t-import", "Edited");
    let reply = edited_light(&runtime, json!([{"method":"saveTransaction","params":{"transaction":sent,"through":"2026-10-20"}}]));
    assert_light(&reply["patch"]["transactions"]["upsert"][0]);
    let stored = row(&saved(&runtime), "t-import");
    assert_eq!((stored["memo"].as_str(), &stored["source"]), (Some("Edited"), &row(&original, "t-import")["source"]));
    assert!(stored.get("hasImportSource").is_none());
}

#[test]
fn future_dated_import_stays_out_of_schedules() {
    let mut budget = fixture();
    let rows = budget["transactions"].as_array_mut().unwrap();
    rows.iter_mut().find(|t| t["id"] == "t-future").unwrap()["source"] = json!({"id":"ynab-2","memo":"From the import"});
    let (_folder, runtime) = opened(&budget);
    let original = saved(&runtime);
    let through = "2026-10-20";
    let sent = light_row(&original, "t-future", "Still dated ahead");
    edited_light(&runtime, json!([{"method":"saveTransaction","params":{"transaction":sent,"through":through}}]));
    let stored = saved(&runtime);
    assert_eq!(row(&stored, "t-future")["source"], row(&original, "t-future")["source"]);
    assert_eq!(stored["schedules"], original["schedules"]);

    // Without a source the same row does become a schedule, which is what `source` was keeping it from.
    let mut plain = light_row(&stored, "t-future", "Dated ahead");
    let fields = plain.as_object_mut().unwrap();
    fields.remove("hasImportSource");
    fields.insert("id".into(), json!("t-ahead"));
    edited_light(&runtime, json!([{"method":"saveTransaction","params":{"transaction":plain,"through":through}}]));
    let after = saved(&runtime);
    assert_eq!(after["schedules"].as_array().unwrap().len(), original["schedules"].as_array().unwrap().len() + 1);
}

#[test]
fn light_patch_step_keeps_source_and_stores_no_hint() {
    let (_folder, runtime) = opened(&fixture());
    let original = saved(&runtime);
    let sent = light_row(&original, "t-import", "In a patch");
    let reply = edited_light(&runtime, json!([{"patch":{"transactions":{"upsert":[sent]}}}]));
    assert_light(&reply["patch"]["transactions"]["upsert"][0]);
    let stored = row(&saved(&runtime), "t-import");
    assert_eq!((stored["memo"].as_str(), &stored["source"]), (Some("In a patch"), &row(&original, "t-import")["source"]));
    assert!(stored.get("hasImportSource").is_none());
}

#[test]
fn a_row_an_undo_took_source_off_stays_without_it() {
    let (_folder, runtime) = opened(&fixture());
    let original = saved(&runtime);
    // A backup with the same export: a row gains a `source`, and one this budget lacks comes with one.
    let mut backup = original.clone();
    let mut extra = row(&original, "t-plain");
    extra["id"] = json!("t-backup");
    extra["source"] = json!({"id":"ynab-8"});
    let rows = backup["transactions"].as_array_mut().unwrap();
    rows.push(extra);
    rows.iter_mut().find(|t| t["id"] == "t-plain").unwrap()["source"] = json!({"id":"ynab-9"});
    ok(&runtime, "save_budget", json!({"budget":backup,"expectedToken":token(&runtime),"ackOnly":true}));

    // Redone, the row has it again, and an app sending the row without it doesn't lose it.
    history_light(&runtime, "undo_budget");
    assert_eq!(saved(&runtime), original);
    history_light(&runtime, "redo_budget");
    let sent = light_row(&saved(&runtime), "t-plain", "Redone");
    edited_light(&runtime, json!([{"method":"saveTransaction","params":{"transaction":sent,"through":"2026-10-20"}}]));
    assert_eq!(row(&saved(&runtime), "t-plain")["source"], json!({"id":"ynab-9"}));

    // Undone, nothing gives it back: not the row's next edit, as a command or a patch, and not a
    // new row with the id of one the undo removed.
    history_light(&runtime, "undo_budget");
    history_light(&runtime, "undo_budget");
    assert_eq!(saved(&runtime), original);
    let mut sent = row(&original, "t-plain");
    sent["memo"] = json!("After the undo");
    let reply = edited_light(&runtime, json!([{"method":"saveTransaction","params":{"transaction":sent,"through":"2026-10-20"}}]));
    assert!(reply["patch"]["transactions"]["upsert"][0].get("hasImportSource").is_none());
    assert!(row(&saved(&runtime), "t-plain").get("source").is_none());
    sent["memo"] = json!("In a patch");
    edited_light(&runtime, json!([{"patch":{"transactions":{"upsert":[sent]}}}]));
    assert!(row(&saved(&runtime), "t-plain").get("source").is_none());
    let mut same_id = row(&backup, "t-backup");
    same_id.as_object_mut().unwrap().remove("source");
    edited_light(&runtime, json!([{"patch":{"transactions":{"upsert":[same_id]}}}]));
    assert!(row(&saved(&runtime), "t-backup").get("source").is_none());
}
