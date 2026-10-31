mod common;

use urbudget_core::{
    bank_import::{self, Choice, Row},
    domain::{Budget, Cleared},
    sms_import,
};

fn file(id: &str, amount: &str) -> String {
    format!("Date,Payee,Memo,Amount,Id\n2026-10-24,Example shop,SMS,{amount},{id}\n")
}
fn row(b: &Budget, account: &str, csv: &str) -> Row {
    bank_import::preview(b, account, csv, "csv", "ymd", false)
        .unwrap()
        .remove(0)
}
fn choice(row: Row, matched: Option<String>) -> Choice {
    Choice {
        row,
        action: if matched.is_some() { "match" } else { "new" }.into(),
        match_id: matched,
    }
}

#[test]
fn sms_stays_uncleared_and_repeat_imports_keep_their_identity_after_corrections() {
    let b = common::budget("SAR");
    let csv = file("sms:example", "-12.34");
    let r = row(&b, "main", &csv);
    let b = sms_import::apply(b, "main", vec![choice(r, None)]).unwrap();
    assert_eq!(b.transactions.len(), 1);
    assert_eq!(b.transactions[0].status, Cleared::Uncleared);
    assert_eq!(b.transactions[0].amount, -1234);
    assert_eq!(b.transactions[0].extra["approved"], false);
    assert!(row(&b, "main", &csv).duplicate);
    assert!(row(&b, "main", &file("sms:example", "-13.00")).duplicate);
    assert!(!row(&b, "main", &file("sms:other-time", "-12.34")).duplicate);
}

#[test]
fn sms_matches_preserve_uncleared_cleared_and_reconciled_status() {
    for status in [Cleared::Uncleared, Cleared::Cleared, Cleared::Reconciled] {
        let b = common::budget("SAR");
        let r = row(&b, "main", &file("bank:existing", "-12.34"));
        let mut b = bank_import::apply(b, "main", vec![choice(r, None)]).unwrap();
        b.transactions[0].status = status;
        let r = row(&b, "main", &file("sms:new", "-12.34"));
        let id = b.transactions[0].id.clone();
        let b = sms_import::apply(b, "main", vec![choice(r, Some(id))]).unwrap();
        assert_eq!(b.transactions.len(), 1);
        assert_eq!(b.transactions[0].status, status);
        assert!(row(&b, "main", &file("sms:new", "-12.34")).duplicate);
    }
}

#[test]
fn sms_transfer_and_its_matched_other_side_stay_uncleared() {
    let mut b = common::budget("SAR");
    let mut savings = b.accounts[0].clone();
    savings.id = "savings".into();
    savings.name = "Savings".into();
    b.accounts.push(savings);
    let csv = "Date,Payee,Memo,Amount,Id,TransferAccountId\n2026-10-24,Transfer,SMS,-100,sms:out,savings\n";
    let r = row(&b, "main", csv);
    let b = sms_import::apply(b, "main", vec![choice(r, None)]).unwrap();
    assert_eq!(b.transactions[0].status, Cleared::Uncleared);
    assert_eq!(b.transactions[0].transfer_status, Some(Cleared::Uncleared));
    let r = row(&b, "savings", &file("sms:in", "100"));
    let id = b.transactions[0].id.clone();
    let b = sms_import::apply(b, "savings", vec![choice(r, Some(id))]).unwrap();
    assert_eq!(b.transactions.len(), 1);
    assert_eq!(b.transactions[0].transfer_status, Some(Cleared::Uncleared));
}

#[test]
fn an_invalid_later_choice_returns_no_partial_budget() {
    let b = common::budget("SAR");
    let good = row(&b, "main", &file("sms:good", "-1"));
    let bad = row(&b, "main", &file("sms:bad", "-2"));
    assert!(sms_import::apply(
        b,
        "main",
        vec![choice(good, None), choice(bad, Some("missing".into()))]
    )
    .is_err());
}
