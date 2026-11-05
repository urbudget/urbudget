//! What one edit costs a phone, by the old path and by `edit_budget`, on synthetic budgets. No user
//! budgets or machine-specific paths.
//! Run: cargo test -p urbudget-android --release --test edit_cost -- --ignored --nocapture
use serde_json::{json, Value};
use std::time::Instant;
use urbudget_core::{domain::*, schedules};
use urbudget_services::{block_on, runtime::Runtime};

/// Timed clear taps per budget, after one that warms the caches.
const TAPS: usize = 21;

/// The fixture of `desktop/src-tauri/tests/performance.rs`, with `source` in the shape YNAB gives it.
fn fixture(transactions: usize, months: u32, categories: usize, scheduled: usize) -> (Budget, String) {
    // December 2026, in months since year 0, so the fixture needs no date library here.
    let end = 2026 * 12 + 11;
    let month = |n: u32| {
        let index = end + 1 - months + n;
        format!("{}-{:02}", index / 12, index % 12 + 1)
    };
    let mut b = Budget::default();
    for (id, kind, opening_balance) in [
        ("cash", AccountKind::Cash, 100_000_000),
        ("savings", AccountKind::Cash, 100_000_000),
        ("card", AccountKind::Credit, -100_000),
        ("card2", AccountKind::Credit, -50_000),
        ("tracking", AccountKind::Tracking, 500_000),
    ] {
        b.accounts.push(Account { id: id.into(), name: id.into(), kind, opening_balance,
            archived: false, extra: Default::default() });
    }
    for n in 0..categories {
        b.categories.push(Category { id: format!("c{n}"), name: format!("Category {n}"), group: format!("Group {}", n / 10),
            target: 10_000, extra: Default::default() });
    }
    for card in ["card", "card2"] {
        b.categories.push(Category { id: format!("pay-{card}"), name: format!("Pay {card}"), group: "Payments".into(),
            target: 0, extra: json!({"paymentAccountId": card}).as_object().unwrap().clone() });
    }
    for n in 0..months {
        for c in &b.categories {
            b.assignments.push(Assignment { extra: Default::default(), month: month(n), category_id: c.id.clone(), amount: 10_000 });
        }
    }
    for n in 0..transactions {
        let date = format!("{}-{:02}", month((n as u32) % months), 1 + (n / months as usize) % 28);
        let mut t = Transaction { id: format!("tx-{n:06}"), date, account_id: "cash".into(),
            payee: format!("Payee {}", n % 29), memo: "Synthetic benchmark transaction".into(), amount: -100 - (n as i64 % 500),
            status: Cleared::Cleared, splits: vec![], transfer_account_id: None, transfer_status: None,
            extra: Default::default() };
        let category = format!("c{}", n % categories);
        let mut split = Split { category_id: Some(category.clone()), amount: t.amount, extra: Default::default() };
        match n % 16 {
            0 => { t.amount = 100_000; split.category_id = None; }
            2 | 12 | 14 => { t.account_id = "card".into(); }
            3 => { t.amount = 70; }
            4 => { t.amount = -2000; t.transfer_account_id = Some("card".into()); }
            5 => { t.amount = -300; t.transfer_account_id = Some("tracking".into()); }
            6 => { t.account_id = "tracking".into(); t.amount = 300; t.transfer_account_id = Some("cash".into()); }
            7 => {
                t.amount = -900;
                t.splits = serde_json::from_value(json!([
                    {"categoryId":category,"amount":-400,"payee":"Split purchase"},
                    {"categoryId":null,"amount":-300,"transferAccountId":"card"},
                    {"categoryId":null,"amount":-200,"transferAccountId":"savings"}
                ])).unwrap();
            }
            8 => { t.account_id = "card".into(); t.amount = -200; t.transfer_account_id = Some("card2".into()); }
            9 => { split.category_id = None; split.extra.insert("unassigned".into(), json!(false)); }
            10 => { t.amount = -50; split.category_id = None; split.extra.insert("unassigned".into(), json!(true)); }
            _ => {}
        }
        if t.transfer_account_id.is_some() { t.transfer_status = Some(Cleared::Cleared); }
        if t.splits.is_empty() && !matches!(n % 16, 4 | 8) {
            split.amount = t.amount;
            t.splits.push(split);
        }
        t.set_created_at(&format!("{}T09:{:02}:00.000Z", t.date, n % 60));
        // Most of an imported transaction's size, and the phones carry it.
        let source = json!({"id":format!("4f2c9a1e-5b7d-4c3a-9e6f-{n:012x}"),"date":t.date,"amount":t.amount * 10,
            "memo":"Representative imported metadata","cleared":"cleared","approved":true,"flag_color":null,"flag_name":null,
            "account_id":format!("7d1e3b2a-90c4-4e58-b1a6-{:012x}", n % 5),"account_name":t.account_id,
            "payee_id":format!("a93c5e10-2f6b-4d17-8c42-{:012x}", n % 29),"payee_name":t.payee,
            "category_id":format!("1be7f4c6-3a58-4b09-96d2-{:012x}", n % categories),"category_name":format!("Category {}", n % categories),
            "transfer_account_id":null,"transfer_transaction_id":null,"matched_transaction_id":null,
            "import_id":format!("YNAB:{}:{}:1", t.amount * 10, t.date),"import_payee_name":t.payee,"import_payee_name_original":t.payee,
            "debt_transaction_type":null,"deleted":false,"subtransactions":[]});
        t.extra.insert("source".into(), source);
        b.transactions.push(t);
    }
    let mut planned = vec![];
    for n in 0..scheduled {
        let mut transaction = b.transactions[if n % 4 == 0 { 4 } else if n % 6 == 0 { 7 } else { 1 }].clone();
        transaction.id = format!("template-{n}");
        if transaction.transfer_account_id.is_none() && transaction.splits.len() == 1 {
            transaction.splits[0].category_id = Some(format!("c{}", n % categories));
        }
        let due = format!("{}-{:02}", month(0), 1 + n % 28);
        planned.push(schedules::Schedule { extra: Default::default(), id: format!("schedule-{n}"), next_date: Some(due.clone()), anchor_date: due,
            frequency: if n % 3 == 0 { "weekly" } else { "monthly" }.into(), paused: n % 17 == 0, transaction });
    }
    b.extra.insert("schedules".into(), json!(planned));
    // Stored order must not be required to be chronological.
    b.transactions.reverse();
    validate(&b).unwrap();
    (b, month(months - 1))
}

/// A stateless engine call with the budget as text, as the app sends it. Returns the reply's text.
fn stateless(method: &str, budget: &str, params: &Value) -> String {
    urbudget_android::invoke_json(&format!(r#"{{"version":1,"method":"{method}","params":{params},"budget":{budget}}}"#))
}

fn value_of(reply: &str) -> Value {
    let mut reply: Value = serde_json::from_str(reply).unwrap();
    assert_eq!(reply["ok"], true, "{}", reply["error"]);
    reply["value"].take()
}

fn median(mut elapsed: Vec<f64>) -> f64 {
    elapsed.sort_by(f64::total_cmp);
    elapsed[elapsed.len() / 2]
}

/// One `edit_budget` as the service's JNI entry runs it: the request's text parsed, the reply's
/// written. Returns the reply, how long that took in ms, and the reply's size. Reading it is Kotlin's.
fn timed_edit(runtime: &Runtime, step: Value, token: &str, month: &str) -> (Value, f64, usize) {
    let request = json!({"steps":[step],"expectedToken":token,"month":month,"asOfMonth":month}).to_string();
    let start = Instant::now();
    let reply = block_on(runtime.dispatch("edit_budget", serde_json::from_str(&request).unwrap())).unwrap();
    let text = reply.to_string();
    let took = start.elapsed().as_secs_f64() * 1000.0;
    (reply, took, text.len())
}

/// Clear taps through the old path: the app's whole budget to the engine and back, again for the
/// totals, then to the store. Reading the replies is left out, since on a phone Kotlin does that.
/// The same taps through `edit_budget` follow. Returns their median, in ms.
fn measure(scenario: &str, budget: &Budget, month: &str) -> f64 {
    let folder = std::env::temp_dir().join(format!("urbudget-edit-cost-{}-{scenario}", std::process::id()));
    let runtime = Runtime::new(&folder).unwrap();
    let call = |method: &str, p: Value| block_on(runtime.dispatch(method, p)).unwrap();
    call("save_budget", json!({"budget":budget,"expectedToken":"","ackOnly":true}));
    let opened = call("load_budget", json!({}));
    let mut app = opened["budget"].to_string();
    let mut token = opened["token"].as_str().unwrap().to_owned();
    println!("fixture {scenario}: {} transactions, {:.2} MB", budget.transactions.len(), app.len() as f64 / 1_000_000.0);
    let id = "tx-000001"; // A plain expense from the cash account.
    let totals = json!({"month":month,"asOfMonth":month});
    let (mut applying, mut calculating, mut saving) = (vec![], vec![], vec![]);
    let mut status = "";
    for tap in 0..=TAPS {
        status = if tap % 2 == 0 { "uncleared" } else { "cleared" };
        let edit = json!({"action":{"type":"bulk","ids":[id],"operation":"status","value":status,"accountId":"all"}});
        let start = Instant::now();
        let changed = stateless("workflow", &app, &edit);
        let applied = start.elapsed();
        let changed = value_of(&changed).to_string();

        let start = Instant::now();
        let summary = stateless("calculate", &changed, &totals);
        let calculated = start.elapsed();
        value_of(&summary);

        let request = format!(r#"{{"budget":{changed},"expectedToken":"{token}","ackOnly":true}}"#);
        let start = Instant::now();
        // The service's JNI entry parses its request before dispatching it.
        let saved = block_on(runtime.dispatch("save_budget", serde_json::from_str(&request).unwrap())).unwrap();
        let stored = start.elapsed();

        token = saved["token"].as_str().unwrap().to_owned();
        app = changed;
        if tap > 0 {
            applying.push(applied.as_secs_f64() * 1000.0);
            calculating.push(calculated.as_secs_f64() * 1000.0);
            saving.push(stored.as_secs_f64() * 1000.0);
        }
    }
    let (mut editing, mut replies) = (vec![], vec![]);
    for tap in 0..=TAPS {
        status = if tap % 2 == 0 { "uncleared" } else { "cleared" };
        let step = json!({"method":"workflow","params":{"action":{"type":"bulk","ids":[id],"operation":"status","value":status,"accountId":"all"}}});
        let (reply, took, bytes) = timed_edit(&runtime, step, &token, month);
        token = reply["token"].as_str().unwrap().to_owned();
        if tap > 0 {
            editing.push(took);
            replies.push(bytes as f64);
        }
    }
    // Timings of work that didn't land would mean nothing.
    let saved = call("load_budget", json!({}));
    let rows = saved["budget"]["transactions"].as_array().unwrap();
    assert_eq!(rows.len(), budget.transactions.len());
    let row = rows.iter().find(|t| t["id"] == id).unwrap();
    assert_eq!(row["status"], status);
    assert!(row.get("source").is_some_and(Value::is_object), "the edit lost the row's source");

    let sum = |rows: &[&Vec<f64>]| median((0..TAPS).map(|tap| rows.iter().map(|r| r[tap]).sum()).collect());
    println!("{scenario}/stateless apply: median {:.3} ms", median(applying.clone()));
    println!("{scenario}/stateless calculate: median {:.3} ms", median(calculating.clone()));
    println!("{scenario}/stateless apply plus calculate: median {:.3} ms", sum(&[&applying, &calculating]));
    println!("{scenario}/save_budget: median {:.3} ms", median(saving.clone()));
    println!("{scenario}/whole edit: median {:.3} ms", sum(&[&applying, &calculating, &saving]));
    let edit = median(editing.clone());
    println!("{scenario}/edit_budget: median {edit:.3} ms, worst {:.3} ms, reply {:.0} bytes", editing.iter().copied().fold(0.0, f64::max), median(replies));
    drop(runtime);
    let _ = std::fs::remove_dir_all(&folder);
    edit
}

/// `diff_extras` sends the whole of any changed extra, so an assign's reply carries every
/// `moneyMoves` row there is, and that list only grows.
fn measure_money_moves(budget: &Budget, month: &str) {
    for rows in [0, 1_000, 5_000] {
        let mut budget = budget.clone();
        let moves: Vec<Value> = (0..rows).map(|n| json!({"at":"2026-10-01T08:00:00+00:00","month":month,"description":format!("Updated assigned amount for Category {}", n % 60),
            "kind":"assign","changes":[{"categoryId":format!("c{}", n % 60),"amount":100}]})).collect();
        budget.extra.insert("moneyMoves".into(), json!(moves));
        let folder = std::env::temp_dir().join(format!("urbudget-edit-cost-{}-moves-{rows}", std::process::id()));
        let runtime = Runtime::new(&folder).unwrap();
        let call = |method: &str, p: Value| block_on(runtime.dispatch(method, p)).unwrap();
        call("save_budget", json!({"budget":budget,"expectedToken":"","ackOnly":true}));
        let mut token = call("load_budget", json!({}))["token"].as_str().unwrap().to_owned();
        let (mut editing, mut replies) = (vec![], vec![]);
        for tap in 0..=TAPS {
            let step = json!({"method":"workflow","params":{"action":{"type":"assignAmount","month":month,"categoryId":"c1","amount":20_000 + tap as i64}}});
            let (reply, took, bytes) = timed_edit(&runtime, step, &token, month);
            token = reply["token"].as_str().unwrap().to_owned();
            if tap > 0 {
                editing.push(took);
                replies.push(bytes as f64);
            }
        }
        println!("{rows} money moves/edit_budget assign: median {:.3} ms, reply {:.0} bytes", median(editing), median(replies));
        drop(runtime);
        let _ = std::fs::remove_dir_all(&folder);
    }
}

#[test]
#[ignore = "synthetic edit timing; run explicitly with --ignored --nocapture"]
fn one_edit_at_two_budget_sizes() {
    // The same shape but for the ledger, so the difference is the size of the history.
    let (fold, month) = fixture(2_742, 36, 60, 12);
    let (long, _) = fixture(10_000, 36, 60, 12);
    let at_fold = measure("fold-sized", &fold, &month);
    let at_long = measure("long-history", &long, &month);
    println!("edit_budget at 10,000 transactions costs {:.2}x its cost at 2,742", at_long / at_fold);
    measure_money_moves(&fold, &month);
}
