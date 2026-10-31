mod common;

use urbudget_core::{bank_import::{self, Choice, Row}, bank_statement::{self, Field, FieldFix, Fixes, LineRef, Session, Status}, domain::{AccountKind, Budget, Cleared}, engine};

fn budget() -> Budget {
    let mut b = common::budget("EUR");
    for (id, kind) in [("savings", AccountKind::Cash), ("other", AccountKind::Cash), ("tracking", AccountKind::Tracking)] {
        let mut account = b.accounts[0].clone();
        account.id = id.into();
        account.name = id.into();
        account.kind = kind;
        b.accounts.push(account);
    }
    b
}

fn csv(target: &str, amount: &str) -> String {
    format!("Date,Payee,Memo,Amount,TransferAccountId\n2026-10-24,Bank wording,Original memo,{amount},{target}\n")
}

fn rows(b: &Budget, account: &str, file: &str) -> Vec<Row> {
    bank_import::preview(b, account, file, "csv", "ymd", false).unwrap()
}

fn save(b: Budget, account: &str, row: Row, matched: Option<String>) -> Result<Budget, String> {
    bank_import::apply(b, account, vec![Choice { row, action: if matched.is_some() { "match" } else { "new" }.into(), match_id: matched }])
}

fn fix(s: &Session, line: &str, version: u64, from: &str, to: &str) -> Fixes {
    Fixes {
        scope: s.scope.clone(), base_revision: s.revision, base_structure: s.structure.clone(),
        fields: vec![FieldFix { line: line.into(), line_version: version, field: Field::TransferAccountId, from: from.into(), to: to.into() }],
        ..Fixes::default()
    }
}

#[test]
fn prepared_transfer_updates_both_accounts_and_later_matches_its_other_side() {
    let b = budget();
    let file = csv("savings", "-100.00");
    let session = bank_statement::start_prepared(&file, "main", "s", "b");
    let preview = bank_statement::preview_statement(&b, "main", &file, &session).unwrap();
    let row = preview.review.items[0].row.clone();
    assert_eq!(row.transfer_account_id.as_deref(), Some("savings"));
    assert_eq!(row.payee, "Bank wording");
    let b = save(b, "main", row, None).unwrap();
    let t = &b.transactions[0];
    assert_eq!(t.transfer_account_id.as_deref(), Some("savings"));
    assert!(t.payee.is_empty() && t.splits.is_empty());
    assert_eq!(t.extra["importedPayee"], "Bank wording");
    assert_eq!((t.status, t.transfer_status), (Cleared::Cleared, Some(Cleared::Uncleared)));
    let summary = engine::calculate(&b, "2026-10").unwrap();
    assert_eq!(summary.accounts.iter().find(|a| a.id == "main").unwrap().balance, -10000);
    assert_eq!(summary.accounts.iter().find(|a| a.id == "savings").unwrap().balance, 10000);
    assert!(rows(&b, "main", &file)[0].duplicate);

    let other_file = csv("main", "100.00");
    let row = rows(&b, "savings", &other_file).remove(0);
    assert_eq!(row.candidates, [t.id.clone()]);
    let id = row.candidates[0].clone();
    let b = save(b, "savings", row, Some(id)).unwrap();
    assert_eq!(b.transactions.len(), 1);
    assert_eq!(b.transactions[0].transfer_status, Some(Cleared::Cleared));
    assert!(rows(&b, "savings", &other_file)[0].duplicate);
}

#[test]
fn an_explicit_target_pairs_only_the_named_account_even_when_bank_wording_disagrees() {
    let b = budget();
    let file = "Date,Payee,Amount\n2026-10-24,other,100.00\n";
    let row = rows(&b, "savings", file).remove(0);
    let b = save(b, "savings", row, None).unwrap();
    let file = csv("savings", "-100.00");
    let row = rows(&b, "main", &file).remove(0);
    assert_eq!(row.transfer_candidates, [b.transactions[0].id.clone()]);
    assert!(row.transfer_sure);
    let id = row.candidates[0].clone();
    let b = save(b, "main", row, Some(id)).unwrap();
    assert_eq!(b.transactions.len(), 1);
    assert_eq!(b.transactions[0].transfer_account_id.as_deref(), Some("main"));
}

#[test]
fn invalid_targets_are_rejected_in_the_editor_and_again_when_saving() {
    for target in ["main", "missing"] {
        let b = budget();
        let file = csv(target, "-100.00");
        let session = bank_statement::start_prepared(&file, "main", "s", "b");
        let statement = bank_statement::statement(&b, "main", &file, &session).unwrap();
        assert!(matches!(statement.lines[0].status, Status::Attention { field: Field::TransferAccountId, .. }));
        assert_eq!(statement.counts.ready, 0);
    }
    let mut b = budget();
    let row = rows(&b, "main", &csv("savings", "-100.00")).remove(0);
    b.accounts.retain(|a| a.id != "savings");
    assert!(save(b, "main", row, None).unwrap_err().contains("no longer exists"));
}

#[test]
fn transfer_overrides_can_be_cleared_reverted_and_reject_stale_edits() {
    let b = budget();
    let file = csv("", "-100.00");
    let s = bank_statement::start_prepared(&file, "main", "s", "b");
    let statement = bank_statement::statement(&b, "main", &file, &s).unwrap();
    let line = statement.lines[0].id.clone();
    let a = bank_statement::apply_fixes(&b, "main", &file, &s, &fix(&s, &line, 0, "", "savings")).unwrap();
    assert!(a.results[0].applied);
    let current = bank_statement::statement(&b, "main", &file, &a.session).unwrap();
    assert_eq!(current.lines[0].fields.transfer_account_id, "savings");
    let stale = bank_statement::apply_fixes(&b, "main", &file, &a.session, &fix(&s, &line, 0, "", "other")).unwrap();
    assert!(!stale.results[0].applied);
    let bad = bank_statement::apply_fixes(&b, "main", &file, &a.session, &fix(&a.session, &line, 1, "savings", "main")).unwrap();
    assert!(!bad.results[0].applied);
    let cleared = bank_statement::apply_fixes(&b, "main", &file, &a.session, &fix(&a.session, &line, 1, "savings", "")).unwrap();
    assert!(cleared.results[0].applied);
    assert!(bank_statement::preview_statement(&b, "main", &file, &cleared.session).unwrap().review.items[0].row.transfer_account_id.is_none());
    let revert = Fixes { scope: a.session.scope.clone(), base_revision: 1, base_structure: a.session.structure.clone(), revert: vec![LineRef { line, line_version: 1 }], ..Fixes::default() };
    let reverted = bank_statement::apply_fixes(&b, "main", &file, &a.session, &revert).unwrap();
    assert!(reverted.results[0].applied);
    assert!(reverted.session.edits.fields.is_empty());
}

#[test]
fn matching_an_existing_expense_converts_it_but_an_existing_other_transfer_is_rejected() {
    let b = budget();
    let initial = rows(&b, "main", &csv("", "-100.00")).remove(0);
    let mut b = save(b, "main", initial, None).unwrap();
    // A manual transaction has no bank identity yet, so it is offered for this statement.
    b.transactions[0].id = "manual".into();
    b.transactions[0].extra.remove("bankImportIds");
    b.transactions[0].extra.remove("bankImportMatches");
    let row = rows(&b, "main", &csv("savings", "-100.00")).remove(0);
    assert_eq!(row.candidates, ["manual"]);
    let b = save(b, "main", row.clone(), Some("manual".into())).unwrap();
    assert_eq!(b.transactions.len(), 1);
    assert_eq!(b.transactions[0].transfer_account_id.as_deref(), Some("savings"));
    assert!(b.transactions[0].splits.is_empty());
    let mut conflict = b;
    conflict.transactions[0].transfer_account_id = Some("other".into());
    conflict.transactions[0].extra.remove("bankImportIds");
    conflict.transactions[0].extra.remove("bankImportMatches");
    assert!(rows(&conflict, "main", &csv("savings", "-100.00"))[0].candidates.is_empty());
    assert!(save(conflict, "main", row, Some("manual".into())).unwrap_err().contains("different transfer account"));
}

#[test]
fn crossing_the_budget_boundary_keeps_a_category_split() {
    let b = budget();
    let row = rows(&b, "main", &csv("tracking", "-100.00")).remove(0);
    let b = save(b, "main", row, None).unwrap();
    assert_eq!(b.transactions[0].splits.len(), 1);
    assert_eq!(b.transactions[0].splits[0].amount, -10000);
    assert_eq!(b.transactions[0].splits[0].category_id.as_deref(), Some("local-uncategorized"));
    assert_eq!(b.transactions[0].transfer_account_id.as_deref(), Some("tracking"));
    let b = budget();
    let row = rows(&b, "tracking", &csv("main", "100.00")).remove(0);
    let b = save(b, "tracking", row, None).unwrap();
    assert_eq!(b.transactions[0].splits[0].amount, 10000);
    assert_eq!(b.transactions[0].splits[0].category_id.as_deref(), Some("local-uncategorized"));
}

#[test]
fn compact_candidates_do_not_mix_explicit_destinations_on_equal_rows() {
    let mut b = budget();
    for target in ["savings", "other"] {
        let mut row = rows(&b, target, "Date,Payee,Amount\n2026-10-24,Unrelated,100.00\n").remove(0);
        row.id = format!("existing-{target}");
        b = save(b, target, row, None).unwrap();
    }
    let file = "Date,Payee,Memo,Amount,TransferAccountId\n2026-10-24,Move one,,-100.00,savings\n2026-10-24,Move two,,-100.00,other\n";
    let session = bank_statement::start_prepared(file, "main", "s", "b");
    let p = bank_statement::preview_statement(&b, "main", file, &session).unwrap();
    assert_eq!(p.review.items.len(), 2);
    for (index, target) in ["savings", "other"].iter().enumerate() {
        assert_eq!(p.review.candidate_sets[p.review.items[index].candidate_set].candidates, [format!("existing-{target}")]);
    }
}

#[test]
fn non_csv_transfer_overrides_and_legacy_prepared_files_work() {
    let b = budget();
    let file = "!Type:Bank\nD2026-10-24\nT-100.00\nPTransfer: savings\n^\n";
    let s = bank_statement::start(&b, "main", file, "s", "b", None).unwrap();
    let line = bank_statement::statement(&b, "main", file, &s).unwrap().lines[0].id.clone();
    let applied = bank_statement::apply_fixes(&b, "main", file, &s, &fix(&s, &line, 0, "", "savings")).unwrap();
    assert!(applied.results[0].applied);
    let row = bank_statement::preview_statement(&b, "main", file, &applied.session).unwrap().review.items[0].row.clone();
    assert_eq!(row.transfer_account_id.as_deref(), Some("savings"));
    let file = "Date,Payee,Memo,Amount\n2026-10-24,Transfer: savings,,-100.00\n";
    let s = bank_statement::start_prepared(file, "main", "s", "b");
    assert!(s.structure.columns.as_ref().unwrap().transfer_account_id.is_none());
    assert!(bank_statement::preview_statement(&b, "main", file, &s).unwrap().review.items[0].row.transfer_account_id.is_none());
}
