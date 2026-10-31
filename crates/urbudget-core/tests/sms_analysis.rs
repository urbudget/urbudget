mod common;
use serde_json::{json, Value};
use urbudget_core::{domain::Cleared, sms_import::*};

fn messages() -> Vec<Message> {
    [
        "Purchase SAR 12.34 at Shop card *1234",
        "Refund SAR 3.50 at Shop card *5678",
    ]
    .iter()
    .enumerate()
    .map(|(i, body)| Message {
        id: format!("sms:{i}"),
        sender: "Bank".into(),
        received_date: "2026-10-31".into(),
        body: body.to_string(),
    })
    .collect()
}
fn value() -> Value {
    json!({"items":[
        {"sourceIds":["sms:0"],"disposition":"transaction","reason":"Purchase","evidence":[{"sourceId":"sms:0","quote":"SAR 12.34"}],"transaction":{"accountId":"main","date":"2026-10-31","amount":"-12.34","currency":"SAR","payee":"Shop","memo":"SMS","kind":"expense","categoryId":null,"transferAccountId":null},"question":null,"accountReference":"*1234"},
        {"sourceIds":["sms:1"],"disposition":"transaction","reason":"Refund","evidence":[{"sourceId":"sms:1","quote":"Refund SAR 3.50"}],"transaction":{"accountId":"card","date":"2026-10-31","amount":"3.50","currency":"SAR","payee":"Shop","memo":"SMS","kind":"refund","categoryId":null,"transferAccountId":null},"question":null,"accountReference":"*5678"}
    ]})
}
fn analysis(v: Value) -> Analysis {
    serde_json::from_value(v).unwrap()
}
fn decisions() -> Vec<Decision> {
    (0..2)
        .map(|index| Decision {
            index,
            action: "new".into(),
            match_id: None,
        })
        .collect()
}

#[test]
fn two_accounts_commit_together_and_sources_survive_changed_amounts_and_accounts() {
    let b = common::budget("SAR");
    let a = analysis(value());
    let saved = apply_batch(b, &messages(), &a, decisions()).unwrap();
    assert_eq!(saved.transactions.len(), 2);
    assert!(saved
        .transactions
        .iter()
        .all(|t| t.status == Cleared::Uncleared));
    assert_eq!(saved.transactions[0].account_id, "main");
    assert_eq!(saved.transactions[1].account_id, "card");
    let mut retry = value();
    retry["items"][0]["transaction"]["accountId"] = json!("card");
    retry["items"][0]["transaction"]["amount"] = json!("-99.00");
    let retry = analysis(retry);
    assert!(preview(&saved, &messages(), &retry)
        .unwrap()
        .iter()
        .all(|r| r.row.duplicate));
    assert_eq!(
        apply_batch(saved, &messages(), &retry, decisions())
            .unwrap()
            .transactions
            .len(),
        2
    );
}
#[test]
fn omitted_repeated_and_fabricated_source_ids_are_rejected() {
    let b = common::budget("SAR");
    let mut v = value();
    v["items"].as_array_mut().unwrap().pop();
    assert!(validate_analysis(&b, &messages(), &analysis(v)).is_err());
    for id in ["sms:0", "sms:invented"] {
        let mut v = value();
        v["items"][1]["sourceIds"] = json!([id]);
        assert!(validate_analysis(&b, &messages(), &analysis(v)).is_err());
    }
}
#[test]
fn fabricated_evidence_and_mappings_and_invalid_money_cannot_reach_import() {
    let b = common::budget("SAR");
    for (key, bad) in [
        ("amount", "-1.001"),
        ("amount", "-1e3"),
        ("amount", "0"),
        ("amount", "99999999999999999999999999"),
        ("currency", "USD"),
        ("date", "2026-02-30"),
        ("accountId", "invented"),
        ("categoryId", "invented"),
        ("kind", "income"),
    ] {
        let mut v = value();
        v["items"][0]["transaction"][key] = json!(bad);
        assert!(
            apply_batch(b.clone(), &messages(), &analysis(v), decisions()).is_err(),
            "{key}={bad}"
        );
    }
    let mut v = value();
    v["items"][0]["evidence"][0]["quote"] = json!("SAR 999");
    assert!(validate_analysis(&b, &messages(), &analysis(v)).is_err());
    let mut v = value();
    v["items"][0]["accountReference"] = json!("*9999");
    assert!(validate_analysis(&b, &messages(), &analysis(v)).is_err());
}
#[test]
fn every_nontransaction_has_a_disposition_and_cannot_smuggle_a_transaction() {
    let b = common::budget("SAR");
    for disposition in ["question", "pending", "ignore"] {
        let mut v = value();
        v["items"][1]["disposition"] = json!(disposition);
        v["items"][1]["question"] = json!("Which card account?");
        assert!(validate_analysis(&b, &messages(), &analysis(v.clone())).is_err());
        v["items"][1]["transaction"] = Value::Null;
        let a = analysis(v);
        assert!(validate_analysis(&b, &messages(), &a).is_ok());
        let saved = apply_batch(
            b.clone(),
            &messages(),
            &a,
            vec![Decision {
                index: 0,
                action: "new".into(),
                match_id: None,
            }],
        )
        .unwrap();
        assert_eq!(saved.transactions.len(), 1);
        assert_eq!(saved.extra["smsImportedMessageIds"], json!(["sms:0"]));
    }
}
#[test]
fn an_invalid_last_account_decision_has_no_partial_result_and_no_source_acknowledgement() {
    let b = common::budget("SAR");
    let before = b.clone();
    let mut d = decisions();
    d[1].action = "match".into();
    d[1].match_id = Some("missing".into());
    assert!(apply_batch(b.clone(), &messages(), &analysis(value()), d).is_err());
    assert_eq!(b, before);
}
#[test]
fn grouping_two_transfer_alerts_uses_all_sources_and_one_ledger_transaction() {
    let b = common::budget("SAR");
    let mut m = messages();
    m[0].body = "Transfer out SAR 12.34".into();
    m[1].body = "Transfer in SAR 12.34".into();
    let mut v = value();
    v["items"].as_array_mut().unwrap().pop();
    v["items"][0]["sourceIds"] = json!(["sms:1", "sms:0"]);
    v["items"][0]["accountReference"] = Value::Null;
    v["items"][0]["transaction"]["kind"] = json!("transfer");
    v["items"][0]["transaction"]["transferAccountId"] = json!("card");
    let saved = apply_batch(
        b,
        &m,
        &analysis(v),
        vec![Decision {
            index: 0,
            action: "new".into(),
            match_id: None,
        }],
    )
    .unwrap();
    assert_eq!(saved.transactions.len(), 1);
    assert_eq!(
        saved.extra["smsImportedMessageIds"],
        json!(["sms:0", "sms:1"])
    );
    assert_eq!(
        saved.transactions[0].transfer_status,
        Some(Cleared::Uncleared)
    );
}
#[test]
fn exact_amount_boundaries_never_round() {
    assert_eq!(cents("-0.01").unwrap(), -1);
    assert_eq!(cents("10000000000.00").unwrap(), 1_000_000_000_000);
    for invalid in [
        "1.001",
        "1.",
        "+1",
        "-0",
        "1,234",
        "1e2",
        "NaN",
        "10000000000.01",
    ] {
        assert!(cents(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn reciprocal_transfer_proposals_require_grouping_or_skipping_one() {
    let b = common::budget("SAR");
    let mut v = value();
    v["items"][0]["transaction"]["kind"] = json!("transfer");
    v["items"][0]["transaction"]["transferAccountId"] = json!("card");
    v["items"][1]["transaction"]["kind"] = json!("transfer");
    v["items"][1]["transaction"]["transferAccountId"] = json!("main");
    v["items"][1]["transaction"]["amount"] = json!("12.34");
    assert!(apply_batch(b.clone(), &messages(), &analysis(v.clone()), decisions()).is_err());
    let mut d = decisions();
    d[1].action = "skip".into();
    assert_eq!(
        apply_batch(b, &messages(), &analysis(v), d)
            .unwrap()
            .transactions
            .len(),
        1
    );
}

#[test]
fn explicit_category_and_source_tombstones_survive_import_and_fresh_start() {
    let mut b = common::budget("SAR");
    b.categories.push(urbudget_core::domain::Category {
        id: "shopping".into(),
        name: "Shopping".into(),
        group: "Spending".into(),
        target: 0,
        extra: Default::default(),
    });
    let mut v = value();
    v["items"][0]["transaction"]["categoryId"] = json!("shopping");
    v["items"][1]["transaction"]["categoryId"] = json!("shopping");
    let saved = apply_batch(b, &messages(), &analysis(v), decisions()).unwrap();
    assert!(saved
        .transactions
        .iter()
        .all(|t| t.splits[0].category_id.as_deref() == Some("shopping")));
    let fresh = urbudget_core::fresh_start::create(&saved, "Restart", "2026-10-31").unwrap();
    assert_eq!(
        fresh.extra["smsImportedMessageIds"],
        json!(["sms:0", "sms:1"])
    );
}

#[test]
fn reviewed_payee_and_uncategorized_choice_win_over_bank_defaults() {
    let mut b = common::budget("SAR");
    b.categories.push(urbudget_core::domain::Category {
        id: "shopping".into(),
        name: "Shopping".into(),
        group: "Spending".into(),
        target: 0,
        extra: Default::default(),
    });
    let rules = json!([{"id":"r","match":"Shop","payee":"Renamed Shop","categoryId":"shopping"}]);
    let payees = json!([{"id":"p","name":"Shop","categoryId":"shopping"}]);
    b.extra.insert("payeeRules".into(), rules.clone());
    b.extra.insert("payees".into(), payees.clone());
    let a = analysis(value());
    assert!(preview(&b, &messages(), &a)
        .unwrap()
        .iter()
        .all(|r| r.row.rename.is_none() && r.row.category_id.is_none()));
    let saved = apply_batch(b, &messages(), &a, decisions()).unwrap();
    assert_eq!(saved.extra["payeeRules"], rules);
    assert_eq!(saved.extra["payees"], payees);
    assert!(saved.transactions.iter().all(|t| t.payee == "Shop"));
    assert_eq!(
        saved.transactions[0].splits[0].category_id.as_deref(),
        Some("local-uncategorized")
    );
    assert_eq!(saved.transactions[1].splits[0].category_id, None);
}
