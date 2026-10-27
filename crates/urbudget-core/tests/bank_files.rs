//! Statements in the layouts real banks export (made-up transactions), read as an import would:
//! the date order and signs the file suggests, then every row. Each file's total matches the
//! balances the bank printed in it, where it printed them, and one payee shows the right column.
//! Each file's bytes become text as the apps have the engine do it (`bank_csv::read_statement`):
//! some are in Shift_JIS, GBK or Windows-1251, as those banks write them, some are Excel workbooks
//! or a ZIP of statements. Beside the CSVs, statements in the
//! banks' exchange formats: CAMT.052/053, MT940, OFX and QIF.
mod common;

use common::*;
use urbudget_core::bank_csv;
use urbudget_core::bank_import::{self, Row};

/// A bank's CSV or workbook read as an import would: the date order and signs the file suggests.
fn read_bank(file: &str, currency: &str, account: &str) -> Vec<Row> {
    let csv = text(&csv_name(file));
    let b = budget(currency);
    let layout = bank_csv::layout(&b, account, &csv, None).unwrap();
    let order = layout.date_format.unwrap_or_else(|| "ymd".into());
    bank_import::preview(&b, account, &csv, "csv", &order, layout.flip_signs).unwrap_or_else(|e| panic!("{file}: {e}"))
}
fn read_statement(file: &str, format: &str, order: &str, currency: &str) -> Vec<Row> {
    bank_import::preview(&budget(currency), "main", &text(file), format, order, false).unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn reads_banks_exports_as_they_come() {
    for &(file, currency, account, count, total, payee) in BANKS {
        let rows = read_bank(file, currency, account);
        assert_eq!(rows.len(), count, "{file}");
        assert_eq!(rows.iter().map(|r| r.amount).sum::<i64>(), total, "{file}");
        assert!(rows.iter().any(|r| r.payee == payee), "{file}: no {payee:?} in {:?}", rows.iter().map(|r| &r.payee).collect::<Vec<_>>());
        assert!(rows.iter().all(|r| r.date.starts_with("2026-")), "{file}");
    }
}

#[test]
fn reads_banks_statement_formats() {
    for &(file, format, order, currency, count, total, payee) in STATEMENTS {
        let text = text(file);
        // Chosen as a CSV, the file says what it is.
        assert_eq!(bank_csv::layout(&budget(currency), "main", &text, None).unwrap().format, format.replace("qfx", "ofx"), "{file}");
        let rows = read_statement(file, format, order, currency);
        assert_eq!(rows.len(), count, "{file}");
        assert_eq!(rows.iter().map(|r| r.amount).sum::<i64>(), total, "{file}");
        assert!(rows.iter().any(|r| r.payee == payee), "{file}: no {payee:?} in {:?}", rows.iter().map(|r| &r.payee).collect::<Vec<_>>());
        assert!(rows.iter().all(|r| r.date.starts_with("2026-")), "{file}");
        assert!(rows.iter().all(|r| !r.duplicate), "{file}");
    }
}

/// Every row's identity, date, amount and payee, as imports have given them: a change to how
/// files are read must not give a transaction already imported a new id (it would come in twice)
/// or another row's. `UPDATE_BANK_IDS=1` writes the file anew after a deliberate change.
#[test]
fn imported_rows_keep_their_ids_and_values() {
    let mut now = serde_json::Map::new();
    let rows = |rows: Vec<Row>| serde_json::json!(rows.iter().map(|r| serde_json::json!([r.id, r.date, r.amount, r.payee])).collect::<Vec<_>>());
    for &(file, currency, account, ..) in BANKS {
        now.insert(csv_name(file), rows(read_bank(file, currency, account)));
    }
    for &(file, format, order, currency, ..) in STATEMENTS {
        now.insert(file.to_string(), rows(read_statement(file, format, order, currency)));
    }
    let path = format!("{}/../../shared/fixtures/bank-files/ids.json", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("UPDATE_BANK_IDS").is_ok() {
        // A row a line, so a change shows as the rows it touched.
        let files: Vec<String> = now.iter().map(|(file, rows)| {
            let lines: Vec<String> = rows.as_array().unwrap().iter().map(|r| format!("    {r}")).collect();
            format!("  {}: [\n{}\n  ]", serde_json::json!(file), lines.join(",\n"))
        }).collect();
        std::fs::write(&path, format!("{{\n{}\n}}\n", files.join(",\n"))).unwrap();
        return;
    }
    let kept: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e} (run with UPDATE_BANK_IDS=1)"))).unwrap();
    for (file, rows) in &now {
        assert_eq!(Some(rows), kept.get(file), "{file}: ids or values changed");
    }
    assert_eq!(now.len(), kept.len(), "files added or removed: run with UPDATE_BANK_IDS=1");
}

/// A budget holding the transactions 1.0.8 imported from a file, by the IDs it gave them.
fn imported_by_1_0_8(currency: &str, account: &str, rows: &[serde_json::Value]) -> urbudget_core::domain::Budget {
    let mut b = budget(currency);
    for r in rows {
        let t = serde_json::json!({
            "id": r[0], "date": r[1], "accountId": account, "payee": r[3], "memo": r[4], "amount": r[2],
            "status": "cleared", "splits": [{ "categoryId": null, "amount": r[2] }], "transferAccountId": null, "transferStatus": null
        });
        b.transactions.push(serde_json::from_value(t).unwrap());
    }
    b
}

/// The same file again after updating finds what 1.0.8 imported from it imported, however today's
/// import reads its lines (a payee from another column, a reference in the memo, a fee taken off).
/// `ids-1.0.8.json` is what 1.0.8's engine made of these files, with the date order and signs
/// chosen then.
#[test]
fn files_1_0_8_imported_are_found_imported() {
    use urbudget_core::bank_statement::{preview_statement, start, DateOrder};
    let path = format!("{}/../../shared/fixtures/bank-files/ids-1.0.8.json", env!("CARGO_MANIFEST_DIR"));
    let then: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for (file, kept) in &then {
        let account = kept["account"].as_str().unwrap();
        let b = imported_by_1_0_8(kept["currency"].as_str().unwrap(), account, kept["rows"].as_array().unwrap());
        let content = text(file);
        let mut session = start(&b, account, &content, "s1", "budget", None).unwrap();
        session.structure.date_order = DateOrder { order: kept["order"].as_str().unwrap().into(), confirmed: true };
        if session.structure.format == "csv" {
            session.structure.flip_signs = kept["flip"].as_bool().unwrap();
        }
        let items = preview_statement(&b, account, &content, &session).unwrap().review.items;
        assert!(!items.is_empty(), "{file}");
        let again: Vec<_> = items.iter().filter(|i| !i.row.duplicate).map(|i| (&i.row.date, i.row.amount, &i.row.payee)).collect();
        assert!(again.is_empty(), "{file}: would come in again: {again:?}");
    }
    // Spaces inside a description, which today's import tidies away.
    let content = "Transaction Date,Post Date,Description,Category,Type,Amount,Memo\n10/01/2026,10/02/2026,AMAZON MKTPL*AB12C3  Amzn.com/bill WA,Shopping,Sale,-23.99,\n";
    let row = serde_json::json!(["bank:b1c05fc2ac872ea6bacd831f8698f60a8a84e840ca169f48db2830c049e8e418", "2026-10-01", -2399, "AMAZON MKTPL*AB12C3  Amzn.com/bill WA", ""]);
    let b = imported_by_1_0_8("USD", "main", &[row]);
    let mut session = start(&b, "main", content, "s1", "budget", None).unwrap();
    session.structure.date_order = DateOrder { order: "mdy".into(), confirmed: true };
    let items = preview_statement(&b, "main", content, &session).unwrap().review.items;
    assert_eq!(items[0].row.payee, "AMAZON MKTPL*AB12C3 Amzn.com/bill WA");
    assert!(items[0].row.duplicate);
}

/// comdirect's export: each account's rows under its heading, its balance and a header row, a
/// card's with columns of its own. Each account is known by its heading, the same from one
/// month's file to the next, and read by its own columns, its rows never another's.
#[test]
fn comdirect_accounts_each_by_its_heading_and_columns() {
    use urbudget_core::bank_statement::{preview_statement, start, statement};
    let b = budget("EUR");
    let labels = |content: &str| {
        let s = start(&b, "main", content, "s1", "budget", None).unwrap();
        statement(&b, "main", content, &s).unwrap().sources.iter().map(|s| s.label.clone()).collect::<Vec<_>>()
    };
    let content = text("de-comdirect.csv");
    assert_eq!(labels(&content), ["Part 1", "Umsätze Tagesgeld PLUS-Konto"]);
    assert_eq!(labels(&content.replace("5.500,00 EUR", "6.000,00 EUR")), labels(&content));

    let content = [
        r#""Buchungstag";"Wertstellung (Valuta)";"Vorgang";"Buchungstext";"Umsatz in EUR";"#,
        r#""01.10.2026";"01.10.2026";"Lastschrift";"Stadtwerke";"-64,20";"#,
        r#""02.10.2026";"02.10.2026";"Gutschrift";"Arbeitgeber";"2.500,00";"#,
        "",
        r#""Alter Kontostand";"1.000,00 EUR";"#,
        "",
        r#""Umsätze Visa-Karte (Kreditkarte)";"Zeitraum: 30 Tage";"#,
        r#""Neuer Kontostand";"-23,99 EUR";"#,
        "",
        r#""Buchungstag";"Umsatztag";"Vorgang";"Referenz";"Buchungstext";"Umsatz in EUR";"#,
        r#""15.10.2026";"14.10.2026";"Kartenverfügung";"12345678";"AMAZON EU";"-23,99";"#,
        "",
    ]
    .join("\n");
    assert_eq!(labels(&content), ["Part 1", "Umsätze Visa-Karte (Kreditkarte)"]);
    let mut s = start(&b, "main", &content, "s1", "budget", None).unwrap();
    let rows = |s: &urbudget_core::bank_statement::Session| preview_statement(&b, "main", &content, s).unwrap().review.items.into_iter().map(|i| (i.row.date, i.row.payee, i.row.amount)).collect::<Vec<_>>();
    assert_eq!(rows(&s), [("2026-10-01".into(), "Stadtwerke".into(), -6420), ("2026-10-02".into(), "Arbeitgeber".into(), 250_000)]);
    s.structure.source = Some("Umsätze Visa-Karte (Kreditkarte)".into());
    assert_eq!(rows(&s), [("2026-10-15".into(), "AMAZON EU".into(), -2399)]);
}
