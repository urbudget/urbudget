//! Transfer intent survives the assistant proposal, CSV review and real import application.
#[path = "../../urbudget-core/tests/common/mod.rs"]
mod common;
use serde_json::{json, Value};
use urbudget_core::{bank_import, bank_statement, domain::Budget, engine};
use urbudget_services::assistant::{attachments::{Files, ImportRow}, proposals::ImportFile, tools::{call, Context}};

fn context<'a>(budget: &'a Budget, files: &'a Files) -> Context<'a> {
    Context { budget, today: "2026-10-28", month: "2026-10", share_memos: true, files, bank_sync: &Value::Null, import: None }
}

fn import(budget: Budget, file: &str, session: &bank_statement::Session) -> Budget {
    let preview = bank_statement::preview_statement(&budget, "main", file, session).unwrap();
    assert_eq!(preview.statement.counts.attention, 0);
    let choices = preview.review.items.into_iter().map(|item| bank_import::Choice {
        row: item.row, action: "new".into(), match_id: None,
    }).collect();
    bank_import::apply(budget, "main", choices).unwrap()
}

#[test]
fn prepared_transfer_updates_both_accounts_without_a_counterpart() {
    let b = common::budget("SAR");
    let files = Files::empty();
    let response = call("prepare_import", &json!({
        "accountId": "main", "rows": [{ "date": "2026-10-24", "payee": "Card payment", "amount": -100, "transferAccountId": "card" }]
    }), &context(&b, &files));
    let proposal = response.proposal.unwrap_or_else(|| panic!("{}", response.output));
    let file = proposal.import.unwrap().csv().unwrap();
    assert!(file.contains("TransferAccountId"));
    let session = bank_statement::start_prepared(&file, "main", "s", "b");
    let after = import(b, &file, &session);
    let saved = &after.transactions[0];
    assert_eq!(saved.transfer_account_id.as_deref(), Some("card"));
    assert!(saved.payee.is_empty() && saved.splits.is_empty());
    let calculated = engine::calculate(&after, "2026-10").unwrap();
    let balance = |id: &str| calculated.accounts.iter().find(|a| a.id == id).unwrap().balance;
    assert_eq!((balance("main"), balance("card")), (-10000, 10000));
    let again = bank_statement::preview_statement(&after, "main", &file, &session).unwrap();
    assert!(again.review.items[0].row.duplicate);
}

#[test]
fn assistant_can_set_and_clear_an_import_transfer_without_changing_bank_text() {
    let b = common::budget("SAR");
    let files = Files::empty();
    let file = "Date,Payee,Amount\n2026-10-24,Card payment,-100\n";
    let mut session = bank_statement::start(&b, "main", file, "s", "b", None).unwrap();
    for (from, to) in [("", "card"), ("card", ""), ("", "card")] {
        let statement = bank_statement::statement(&b, "main", file, &session).unwrap();
        let brief = statement.brief(true);
        let ctx = Context { import: Some(&brief), ..context(&b, &files) };
        let response = call("fix_import", &json!({ "fields": [{
            "line": statement.lines[0].id, "field": "transferAccountId", "from": from, "to": to,
            "reason": "The user identified the destination account"
        }] }), &ctx);
        let proposal = response.proposal.unwrap_or_else(|| panic!("{}", response.output));
        let applied = bank_statement::apply_fixes(&b, "main", file, &session, proposal.fixes.as_ref().unwrap()).unwrap();
        assert!(applied.results.iter().all(|r| r.applied));
        session = applied.session;
        let preview = bank_statement::preview_statement(&b, "main", file, &session).unwrap();
        assert_eq!(preview.review.items[0].row.transfer_account_id.as_deref().unwrap_or_default(), to);
        assert_eq!(preview.review.items[0].row.payee, "Card payment");
    }
    assert_eq!(import(b, file, &session).transactions[0].transfer_account_id.as_deref(), Some("card"));
}

#[test]
fn invalid_transfer_targets_are_not_silently_turned_into_payees() {
    let b = common::budget("SAR");
    let files = Files::empty();
    for target in [json!("main"), json!("missing"), json!("Transfer: card"), json!(123)] {
        let response = call("prepare_import", &json!({
            "accountId": "main", "rows": [{ "date": "2026-10-24", "payee": "Card payment", "amount": -100, "transferAccountId": target }]
        }), &context(&b, &files));
        assert!(response.proposal.is_none(), "{target}");
        assert!(response.output["error"].is_string());
    }
    let response = call("prepare_import", &json!({
        "accountId": "main", "rows": [{ "date": "2026-10-24", "payee": "Card payment", "amount": -100, "transferAccount": "card" }]
    }), &context(&b, &files));
    assert!(response.proposal.is_none());
    assert!(response.output["error"].as_str().unwrap().contains("Unsupported import row field"));
    assert!(serde_json::from_value::<urbudget_services::assistant::attachments::Mapping>(json!({
        "headerRow": 1, "date": "A", "payee": "B", "amount": "C", "dateOrder": "ymd", "transferAccount": "D"
    })).is_err());
}

#[test]
fn saved_import_proposals_without_transfers_remain_compatible() {
    let row: ImportRow = serde_json::from_value(json!({ "date": "2026-10-24", "payee": "Shop", "memo": "", "amount": -100 })).unwrap();
    assert_eq!(row.transfer_account_id, None);
    let csv = ImportFile { account: "main".into(), source: "statement".into(), rows: vec![row] }.csv().unwrap();
    assert_eq!(csv, "Date,Payee,Memo,Amount\n2026-10-24,Shop,,-1.00\n");
}
