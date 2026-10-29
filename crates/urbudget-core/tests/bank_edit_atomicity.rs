mod common;

use urbudget_core::{bank_statement::{self, CellFix, Field, FieldFix, Fixes, Session, Status}, domain::Budget};

fn fixes(s: &Session) -> Fixes {
    Fixes { scope: s.scope.clone(), base_revision: s.revision, base_structure: s.structure.clone(), ..Fixes::default() }
}

fn cell(line: &str, version: u64, column: usize, from: &str, to: &str) -> CellFix {
    CellFix { line: line.into(), line_version: version, column, from: from.into(), to: to.into() }
}

fn field(line: &str, version: u64, field: Field, from: &str, to: &str) -> FieldFix {
    FieldFix { line: line.into(), line_version: version, field, from: from.into(), to: to.into() }
}

fn session(_b: &Budget, file: &str) -> Session {
    bank_statement::start_prepared(file, "main", "edit-atomicity", "budget")
}

#[test]
fn a_stale_amount_override_cannot_leave_its_raw_cell_applied_with_the_opposite_sign() {
    let b = common::budget("EUR");
    let file = "Date,Payee,Memo,Amount\n2026-10-24,Shop,,10\n";
    let mut s = session(&b, file);
    s.structure.flip_signs = true;
    let line = bank_statement::statement(&b, "main", file, &s).unwrap().lines[0].id.clone();
    let mut assistant = fixes(&s);
    assistant.fields.push(field(&line, 0, Field::Amount, "-10.00", "-20.00"));
    let corrected = bank_statement::apply_fixes(&b, "main", file, &s, &assistant).unwrap().session;

    // The client refreshed lineVersion, but the draft still holds the values it opened with.
    let mut draft = fixes(&corrected);
    draft.cells.push(cell(&line, 1, 3, "10", "-12.00"));
    draft.fields.push(field(&line, 1, Field::Amount, "-10.00", "-12.00"));
    let applied = bank_statement::apply_fixes(&b, "main", file, &corrected, &draft).unwrap();
    assert!(applied.results.iter().all(|r| !r.applied));
    assert_eq!(applied.session, corrected);
    let preview = bank_statement::preview_statement(&b, "main", file, &applied.session).unwrap();
    assert_eq!(preview.review.items[0].row.amount, -2000);
    assert_eq!(preview.statement.lines[0].cells[3], "10");
}

#[test]
fn a_refused_coupled_line_does_not_block_another_coupled_line() {
    let b = common::budget("EUR");
    let file = "Date,Payee,Memo,Amount,Fee\n2026-10-24,First,,10,1\n2026-10-24,Second,,20,2\n";
    let mut s = session(&b, file);
    s.structure.flip_signs = true;
    let lines = bank_statement::statement(&b, "main", file, &s).unwrap().lines;
    let mut draft = fixes(&s);
    draft.cells.push(cell(&lines[0].id, 0, 3, "stale", "-12.00"));
    draft.fields.push(field(&lines[0].id, 0, Field::Amount, "-11.00", "-12.00"));
    draft.cells.push(cell(&lines[1].id, 0, 3, "20", "-25.00"));
    draft.fields.push(field(&lines[1].id, 0, Field::Amount, "-22.00", "-25.00"));
    let applied = bank_statement::apply_fixes(&b, "main", file, &s, &draft).unwrap();
    assert!(applied.results.iter().filter(|r| r.line.as_deref() == Some(&lines[0].id)).all(|r| !r.applied));
    assert!(applied.results.iter().filter(|r| r.line.as_deref() == Some(&lines[1].id)).all(|r| r.applied));
    let preview = bank_statement::preview_statement(&b, "main", file, &applied.session).unwrap();
    assert_eq!(preview.review.items.iter().map(|r| r.row.amount).collect::<Vec<_>>(), [-1100, -2500]);
    assert!(!applied.session.versions.contains_key(&lines[0].id));
    assert_eq!(applied.session.versions[&lines[1].id], 1);
}

#[test]
fn stale_or_invalid_transfer_fields_cannot_partially_update_the_line() {
    let b = common::budget("EUR");
    let file = "Date,Payee,Memo,Amount,TransferAccountId\n2026-10-24,Shop,,-10.00,\n";
    let s = session(&b, file);
    let line = bank_statement::statement(&b, "main", file, &s).unwrap().lines[0].id.clone();
    let mut assistant = fixes(&s);
    assistant.fields.push(field(&line, 0, Field::TransferAccountId, "", "card"));
    let corrected = bank_statement::apply_fixes(&b, "main", file, &s, &assistant).unwrap().session;
    for (from, to, version) in [("", "", 1), ("card", "main", 1), ("card", "", 0)] {
        let mut draft = fixes(&corrected);
        draft.cells.push(cell(&line, 1, 1, "Shop", "Typed payee"));
        draft.cells.push(cell(&line, 1, 4, "", "card"));
        draft.fields.push(field(&line, version, Field::TransferAccountId, from, to));
        let applied = bank_statement::apply_fixes(&b, "main", file, &corrected, &draft).unwrap();
        assert!(applied.results.iter().all(|r| !r.applied));
        assert_eq!(applied.session, corrected);
        let line = &bank_statement::statement(&b, "main", file, &applied.session).unwrap().lines[0];
        assert_eq!(line.fields.transfer_account_id, "card");
        assert_eq!(line.fields.payee, "Shop");
    }
}

#[test]
fn independent_cells_only_repairs_still_apply_where_they_fit() {
    let b = common::budget("EUR");
    let file = "Date,Payee,Memo,Amount\n2026-10-24,Shop,,-10.00\n";
    let s = session(&b, file);
    let line = bank_statement::statement(&b, "main", file, &s).unwrap().lines[0].id.clone();
    let mut draft = fixes(&s);
    draft.cells.push(cell(&line, 0, 1, "Shop", "New payee"));
    draft.cells.push(cell(&line, 0, 3, "stale", "-20.00"));
    let applied = bank_statement::apply_fixes(&b, "main", file, &s, &draft).unwrap();
    assert!(applied.results[0].applied);
    assert!(!applied.results[1].applied);
    let line = &bank_statement::statement(&b, "main", file, &applied.session).unwrap().lines[0];
    assert_eq!(line.fields.payee, "New payee");
    assert_eq!(line.fields.amount, "-10.00");
}

#[test]
fn an_unreadable_csv_date_keeps_the_net_amount_already_read_from_flows_and_fees() {
    let b = common::budget("EUR");
    for (file, flip_signs, expected) in [
        ("Date,Payee,Outflow,Inflow\n2026-10-99,Shop,10,\n", false, "-10.00"),
        ("Date,Payee,Amount,Fee\n2026-10-99,Shop,10,1\n", true, "-11.00"),
    ] {
        let mut s = bank_statement::start(&b, "main", file, "date-attention", "budget", None).unwrap();
        s.structure.flip_signs = flip_signs;
        let lines = bank_statement::statement(&b, "main", file, &s).unwrap().lines;
        assert!(matches!(lines[0].status, Status::Attention { field: Field::Date, .. }));
        assert_eq!(lines[0].fields.amount, expected);
    }
    let file = "Date,Payee,Amount\n2026-10-99,Shop,invalid\n";
    let s = bank_statement::start(&b, "main", file, "money-attention", "budget", None).unwrap();
    let lines = bank_statement::statement(&b, "main", file, &s).unwrap().lines;
    assert!(matches!(lines[0].status, Status::Attention { field: Field::Amount, .. }));
    assert_eq!(lines[0].fields.amount, "invalid");
}
