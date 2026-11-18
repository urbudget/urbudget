//! Deterministic synthetic timing and output parity. No user budgets or machine-specific paths.
//! Run: cargo test --test performance -- --ignored --nocapture
use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, hint::black_box, time::Instant};
use urbudget_lib::{domain::*, engine, reports, schedules};

fn fixture(transactions: usize, months: u32, categories: usize, scheduled: usize) -> (Budget, String, String) {
    let end = NaiveDate::from_ymd_opt(2026, 12, 1).unwrap();
    let start = end.checked_sub_months(Months::new(months - 1)).unwrap();
    let month = |n| start.checked_add_months(Months::new(n)).unwrap().format("%Y-%m").to_string();
    let mut b = Budget::default();
    b.extra.insert("calculationAsOfMonth".into(), json!("2026-12"));
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
            extra: json!({"source":{"id":format!("source-{n}"),"memo":"Representative imported metadata","approved":true}})
                .as_object().unwrap().clone().into() };
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
    (b, month(0), format!("{}-{:02}", end.year(), end.month()))
}

fn measure<T: Serialize>(scenario: &str, operation: &str, mut run: impl FnMut() -> T) -> String {
    black_box(run());
    let mut elapsed = vec![];
    let mut digest = String::new();
    for _ in 0..3 {
        let start = Instant::now();
        let result = black_box(run());
        elapsed.push(start.elapsed().as_secs_f64() * 1000.0);
        // Serialization and hashing are outside the timed calculation.
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&result).unwrap()));
        if !digest.is_empty() { assert_eq!(hash, digest, "nondeterministic {scenario}/{operation}"); }
        digest = hash;
    }
    elapsed.sort_by(f64::total_cmp);
    println!("{scenario}/{operation}: median {:.3} ms; sha256 {digest}", elapsed[1]);
    digest
}

#[test]
#[ignore = "deterministic synthetic engine/report benchmark; run explicitly with --ignored --nocapture"]
fn synthetic_engine_and_report_performance() {
    let mut hashes = BTreeMap::new();
    for (name, count, months, categories, scheduled) in [
        ("representative", 10_000, 60, 60, 0),
        ("long-history", 30_000, 120, 100, 0),
        ("schedule-heavy", 1000, 24, 80, 48),
    ] {
        let (b, from, to) = fixture(count, months, categories, scheduled);
        println!("fixture {name}: {count} transactions, {months} months, {categories} ordinary categories, {scheduled} schedules");
        hashes.insert(format!("{name}/selected"), measure(name, "selected", || engine::calculate_at(black_box(&b), &to, &to).unwrap()));
        hashes.insert(format!("{name}/history"), measure(name, "history", || engine::calculate_history(black_box(&b), &to).unwrap()));
        let mut filter = reports::Filter { from, to, account_ids: vec![], category_ids: vec![] };
        hashes.insert(format!("{name}/reports"), measure(name, "reports", || reports::calculate(black_box(&b), &filter).unwrap()));
        filter.account_ids = vec!["cash".into(), "card".into()];
        filter.category_ids = (0..10).map(|n| format!("c{n}")).chain(["__uncategorized".into()]).collect();
        hashes.insert(format!("{name}/filtered-reports"), measure(name, "filtered-reports", || reports::calculate(black_box(&b), &filter).unwrap()));
    }
    println!("output hashes: {}", serde_json::to_string(&hashes).unwrap());
    // Engine fingerprints retain the pre-optimization baseline. Report fingerprints
    // include the intentional Stage 4 correction: Age of Money counts original saved
    // cash outflows, netting each parent's splits/refunds instead of counting its legs.
    // The original report hashes remain in reports/engine-stages-20261020/
    // stage3-native-performance.json; all twelve matched throughout Stage 3.
    // The six report fingerprints were recorded again when each month gained `market`
    // (what markets did to net worth).
    let expected: BTreeMap<String, String> = serde_json::from_value(json!({
        "long-history/filtered-reports":"9188bd4b04a47760512d945b67c6dfa52898802f7953b9e3817bc3237a3121cb",
        "long-history/history":"c40bdc57f23a6090ef5c34809db5d06198659dbc2b498e18d478178cacec63b8",
        "long-history/reports":"dfab6ffa500f5bd6fc8cb112dbad79b4d606e65e352704ca9355b45f9310f819",
        "long-history/selected":"4d65c651a0e0146bf537befcbabd391fc9b25df9eb971c7ab4707cb0903f22bd",
        "representative/filtered-reports":"0abd449f333761b9d4c1b547ad623beb8491e97731fef162b9ce2b01d6096004",
        "representative/history":"d7c020c7f7eb28b8832e8a87b979946b8518c8b61e04d727ce676e7f096a658b",
        "representative/reports":"e61c3cc87b4f7b203010f53d8876439353ae160bf2a50f6fc47e7e64728d1316",
        "representative/selected":"46d79f4f78db53bb61b263a014b0ec3cfba18591739b2fdec8a22322f3162ba1",
        "schedule-heavy/filtered-reports":"d673d82b84c83aa949f45cfd17f5f2d05e53e8e6aa898c5a1ed203395e483b98",
        "schedule-heavy/history":"9c488f8faf4bfc7d66cc0e113403a95e46f89b2db2a0e12e9e8ebd5be4a536a8",
        "schedule-heavy/reports":"5b14b547a6d9eece5ee9c59077ccac66814eb9b8038e7add1008674f75e0231e",
        "schedule-heavy/selected":"a426d193d91c5a5c1fb9e6e56c89e5e15d3ba62088584e14416a044c51f47216"
    })).unwrap();
    assert_eq!(hashes, expected, "synthetic accounting outputs changed");
}
