//! Faults in real banks' files, one at a time: a damaged amount or date makes its own line need
//! attention and changes nothing else (no other line's ID, date, amount or payee), unless it took
//! the only evidence of the file's date order or signs, and then the import asks. Repaired by a
//! value for the field, or by the cell as the file had it, the file imports as the clean one does.
mod common;

use common::*;
use std::collections::BTreeMap;
use urbudget_core::bank_csv;
use urbudget_core::bank_statement::{
    apply_fixes, preview_statement, CellFix, DateOrder, Field, FieldFix, Fixes, Line, Session, Status, Structure, StatementPreview,
};
use urbudget_core::domain::Budget;

/// Lines sampled from each file.
const SAMPLED: usize = 6;

/// A row: its ID, date, amount and payee.
type Row = (String, String, i64, String);
/// Each line that reads, by its place among the file's lines.
fn rows(p: &StatementPreview) -> BTreeMap<usize, Row> {
    let at: BTreeMap<&str, usize> = p.statement.lines.iter().enumerate().map(|(i, l)| (l.id.as_str(), i)).collect();
    p.lines.iter().zip(&p.review.items).map(|(id, i)| (at[id.as_str()], (i.row.id.clone(), i.row.date.clone(), i.row.amount, i.row.payee.clone()))).collect()
}
fn sorted(p: &StatementPreview) -> Vec<Row> {
    let mut all: Vec<Row> = rows(p).into_values().collect();
    all.sort();
    all
}
fn kind(l: &Line) -> String {
    match &l.status {
        Status::Ready => "ready".into(),
        Status::Attention { field, .. } => format!("attention {field:?}"),
        Status::LeftOut { reason, .. } => format!("{reason:?}"),
    }
}
fn asks(p: &StatementPreview) -> bool {
    p.statement.date_order.as_ref().is_some_and(|q| q.unconfirmed) || p.statement.signs.as_ref().is_some_and(|q| q.unconfirmed)
}

/// The file with one line's amount or date damaged so it can't be read, or None when the line's
/// text can't be found to damage.
fn damage(format: &str, content: &str, clean: &StatementPreview, i: usize, field: Field) -> Option<String> {
    let line = &clean.statement.lines[i];
    let nth = |tag: &str, n: usize| content.match_indices(tag).nth(n).map(|(at, _)| at);
    let number = |id: &str| id.trim_start_matches(|c: char| c.is_ascii_alphabetic()).split('.').next()?.parse::<usize>().ok();
    let replace = |from: usize, to: usize, text: &str| Some(format!("{}{text}{}", &content[..from], &content[to..]));
    // The text after a tag, up to the next '<'.
    let value = |at: usize, tag: &str| {
        let start = at + content[at..].find(tag)? + tag.len();
        let end = start + content[start..].find('<').unwrap_or(content.len() - start);
        Some((start, end))
    };
    match format {
        "csv" => {
            let start: usize = line.id.parse().ok()?;
            let end = start + content[start..].find(['\r', '\n']).unwrap_or(content.len() - start);
            let text = &content[start..end];
            let columns = clean.statement.columns.as_ref()?;
            let headers = &clean.statement.headers;
            let names = match field {
                Field::Date => vec![columns.date.as_ref()],
                _ => vec![columns.amount.as_ref(), columns.outflow.as_ref(), columns.inflow.as_ref()],
            };
            let cell = names.into_iter().flatten().filter_map(|n| headers.iter().position(|h| h == n)).map(|c| &line.cells[c]).find(|c| !c.trim().is_empty())?;
            let at = start + text.find(cell.as_str())?;
            // A character misread, as the cell's width stays (a statement in aligned columns).
            let digit = at + cell.find(|c: char| c.is_ascii_digit())?;
            replace(digit, digit + 1, "x")
        }
        "ofx" | "qfx" => {
            let block = nth("<STMTTRN>", number(&line.id)? - 1)?;
            let (from, to) = value(block, if field == Field::Date { "<DTPOSTED>" } else { "<TRNAMT>" })?;
            if to == from {
                return None;
            }
            replace(from, if field == Field::Date { from + 1 } else { from }, "x")
        }
        "mt940" => {
            let at = nth(":61:", number(&line.id)? - 1)? + 4;
            let end = at + content[at..].find(['\r', '\n']).unwrap_or(content.len() - at);
            match field {
                Field::Date => replace(at, at + 1, "x"),
                _ => content[at..end].find(',').and_then(|c| replace(at + c, at + c + 1, "x")),
            }
        }
        "camt053" => {
            let entry = nth("<Ntry>", number(&line.id)? - 1)?;
            let payment = line.id.split('.').nth(1).and_then(|n| n.parse::<usize>().ok());
            match (field, payment) {
                // A batch's payments share their entry's date: damaging it damages them all.
                (Field::Date, Some(_)) => None,
                (Field::Date, None) => {
                    let booked = entry + content[entry..].find("<BookgDt>")?;
                    let (from, _) = value(booked, "<Dt>").or_else(|| value(booked, "<DtTm>"))?;
                    replace(from, from + 1, "x")
                }
                (_, payment) => {
                    let at = match payment {
                        Some(n) => content[entry..].match_indices("<TxDtls>").nth(n - 1).map(|(at, _)| entry + at)?,
                        None => entry,
                    };
                    let amt = at + content[at..].find("<Amt ").or_else(|| content[at..].find("<Amt>"))?;
                    let from = amt + content[amt..].find('>')? + 1;
                    replace(from, from, "x")
                }
            }
        }
        _ => None,
    }
}

/// A fix set for a session: its fixes, and the answers the clean file gave.
fn repair(session: &Session, clean: &StatementPreview, change: impl FnOnce(&mut Fixes)) -> Fixes {
    let structure = Structure {
        date_order: DateOrder { order: clean.statement.order.clone(), confirmed: true },
        signs_by_type: clean.statement.signs.as_ref().map(|s| s.by_type),
        ..session.structure.clone()
    };
    let mut fixes = Fixes { scope: session.scope.clone(), base_structure: session.structure.clone(), structure: Some(structure), ..Fixes::default() };
    change(&mut fixes);
    fixes
}

fn check(name: &str, format: &str, b: &Budget, account: &str, content: &str, structure: Structure) -> (usize, usize) {
    let open = |content: &str| Session::new("faults", "budget", account, content, structure.clone());
    let preview = |content: &str, s: &Session| preview_statement(b, account, content, s).unwrap_or_else(|e| panic!("{name}: {e}"));
    let clean = preview(content, &open(content));
    let ready: Vec<usize> = (0..clean.statement.lines.len()).filter(|&i| clean.statement.lines[i].status == Status::Ready).collect();
    let picked: Vec<usize> = (0..SAMPLED.min(ready.len())).map(|k| ready[k * ready.len() / SAMPLED.min(ready.len())]).collect();
    let (mut tried, mut landed) = (0, 0);
    for &i in &picked {
        for field in [Field::Amount, Field::Date] {
            let Some(broken) = damage(format, content, &clean, i, field) else { continue };
            tried += 1;
            let what = format!("{name} line {i} {field:?}");
            let s = open(&broken);
            let damaged = preview_statement(b, account, &broken, &s).unwrap_or_else(|e| panic!("{what}: {e}"));
            let lines = &damaged.statement.lines;
            assert_eq!(lines.len(), clean.statement.lines.len(), "{what}");
            // The damage may have hit a cell that only looks like the amount or date.
            if !matches!(&lines[i].status, Status::Attention { field: f, .. } if *f == field) {
                if std::env::var("FAULTS_MISSED").is_ok() {
                    eprintln!("missed: {what}: {:?} {:?}", lines[i].status, lines[i].cells);
                }
                continue;
            }
            landed += 1;
            let (before, after) = (rows(&clean), rows(&damaged));
            if !asks(&damaged) {
                for k in (0..lines.len()).filter(|&k| k != i) {
                    assert_eq!(kind(&lines[k]), kind(&clean.statement.lines[k]), "{what} changed line {k}");
                    assert_eq!(after.get(&k), before.get(&k), "{what} changed line {k}");
                }
            }
            // Repaired with the value the clean file has.
            let want = &before[&i];
            let to = match field {
                Field::Date => want.1.clone(),
                _ => format!("{}{}.{:02}", if want.2 < 0 { "-" } else { "" }, want.2.abs() / 100, want.2.abs() % 100),
            };
            let from = lines[i].fields.get(field).to_string();
            let fixes = repair(&s, &clean, |f| f.fields.push(FieldFix { line: lines[i].id.clone(), line_version: 0, field, from, to }));
            let fixed = apply_fixes(b, account, &broken, &s, &fixes).unwrap();
            assert!(fixed.results.iter().all(|r| r.applied), "{what}: {:?}", fixed.results);
            assert_eq!(sorted(&preview(&broken, &fixed.session)), sorted(&clean), "{what} repaired by its value");
            // Repaired by the cell as the file had it.
            if let Status::Attention { column: Some(c), .. } = lines[i].status {
                let cell = CellFix { line: lines[i].id.clone(), line_version: 0, column: c, from: lines[i].cells[c].clone(), to: clean.statement.lines[i].cells[c].clone() };
                let fixes = repair(&s, &clean, |f| f.cells.push(cell));
                let fixed = apply_fixes(b, account, &broken, &s, &fixes).unwrap();
                assert!(fixed.results.iter().all(|r| r.applied), "{what}: {:?}", fixed.results);
                assert_eq!(sorted(&preview(&broken, &fixed.session)), sorted(&clean), "{what} repaired by its cell");
            }
        }
    }
    (tried, landed)
}

#[test]
fn a_damaged_line_changes_only_itself_and_repairs_to_the_clean_file() {
    let (mut tried, mut landed) = (0, 0);
    for &(file, currency, account, ..) in BANKS {
        let name = csv_name(file);
        let content = text(&name);
        let b = budget(currency);
        let layout = bank_csv::layout(&b, account, &content, None).unwrap();
        let order = layout.date_format.unwrap_or_else(|| "ymd".into());
        let structure = Structure { format: "csv".into(), date_order: DateOrder { order, confirmed: false }, flip_signs: layout.flip_signs, ..Structure::default() };
        let (t, l) = check(&name, "csv", &b, account, &content, structure);
        tried += t;
        landed += l;
    }
    for &(file, format, order, currency, ..) in STATEMENTS {
        let structure = Structure { format: format.into(), date_order: DateOrder { order: order.into(), confirmed: false }, ..Structure::default() };
        let (t, l) = check(file, format.replace("qfx", "ofx").as_str(), &budget(currency), "main", &text(file), structure);
        tried += t;
        landed += l;
    }
    eprintln!("{landed} of {tried} faults landed on their line");
    // Nearly every fault lands on the cell it was meant for.
    assert!(landed * 10 >= tried * 9, "{landed} of {tried} faults landed");
}
