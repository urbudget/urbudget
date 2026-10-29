//! A bank's CSV export: where its header row is, and which column holds what, found by the
//! names banks give their columns or taken from the columns someone chose for the account. The
//! account keeps that choice, with the file's date order and signs, for its next file.
use crate::bank_import::{dates, money, read_dates, BankTransaction, DateReading};
use crate::bank_statement::{Field, Reason, Status, DECLINED};
use crate::domain::Budget;
use crate::i18n::tr;
use crate::t;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// Which of a CSV's columns hold what, by their header text, as someone chose them. A column
/// left out isn't read.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Columns {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    /// One column for money in and out, negative for out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outflow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inflow: Option<String>,
    /// The bank's own id for each transaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// An explicit local destination account ID, supplied by an import proposal or mapping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer_account_id: Option<String>,
}
impl Columns {
    pub(crate) fn names(&self) -> [&Option<String>; 8] {
        [&self.date, &self.payee, &self.memo, &self.amount, &self.outflow, &self.inflow, &self.id, &self.transfer_account_id]
    }
}

/// How an account's bank files read, kept on the account (`fileImport`) when it imports one so
/// the next file from the same bank needs no choices.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileSettings {
    /// Without them, the columns are found by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Columns>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_format: Option<String>,
    /// Someone confirmed that date order, for files whose dates don't settle it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_confirmed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_signs: Option<bool>,
    /// The sheet of a workbook, by its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// The account in files with several, by its label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub fn settings(b: &Budget, account: &str) -> FileSettings {
    b.accounts.iter().find(|a| a.id == account)
        .and_then(|a| a.extra.get("fileImport"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Keeps what an import used for the account's next file. A setting left out stays as it was;
/// columns without a date go back to being found by name. What can't be kept (a date order that
/// isn't one, a column without a name) stays as it was too: it never stops an import.
pub fn remember(b: &mut Budget, account: &str, chosen: FileSettings) -> Result<(), String> {
    let bad = |n: &Option<String>| n.as_ref().is_some_and(|n| n.trim().is_empty() || n.len() > 200);
    let mut kept = settings(b, account);
    if let Some(columns) = chosen.columns.filter(|c| !c.names().into_iter().any(bad)) {
        kept.columns = columns.date.is_some().then_some(columns);
    }
    if let Some(order) = chosen.date_format.filter(|f| matches!(f.as_str(), "ymd" | "dmy" | "mdy")) {
        kept.date_format = Some(order);
        kept.date_confirmed = chosen.date_confirmed.filter(|c| *c);
    }
    if chosen.flip_signs.is_some() {
        kept.flip_signs = chosen.flip_signs;
    }
    let named = |n: Option<String>| n.filter(|n| !n.trim().is_empty() && n.len() <= 200);
    if let Some(sheet) = named(chosen.sheet) {
        kept.sheet = Some(sheet);
    }
    if let Some(source) = named(chosen.source) {
        kept.source = Some(source);
    }
    let a = b.accounts.iter_mut().find(|a| a.id == account).ok_or_else(|| t!("Account not found"))?;
    a.extra.insert("fileImport".into(), serde_json::to_value(kept).map_err(|e| e.to_string())?);
    Ok(())
}

/// A statement's text and the encoding it was in ("spreadsheet" for a workbook's sheet, as CSV).
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Decoded {
    pub text: String,
    pub encoding: &'static str,
    /// The sheet read, of a workbook's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// A workbook's sheets, by name, to read another.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sheets: Vec<String>,
}

/// What a PDF statement needs.
pub fn pdf() -> &'static str {
    tr("A PDF statement can't be read directly. The assistant can read it, check it against the statement's totals and bring its rows to this review.")
}

/// A statement file's text, whatever it came as: an Excel or OpenDocument workbook (its sheet that
/// looks most like a statement, as CSV), a ZIP of statements (CAMT.053 files a day, MT940), or text
/// in any encoding (`decode`). A PDF is the assistant's to read.
pub fn read_statement(bytes: &[u8]) -> Result<Decoded, String> {
    read_sheet(bytes, None)
}
/// The same, reading a workbook's sheet by its name, when it has one by that name.
pub fn read_sheet(bytes: &[u8], sheet: Option<&str>) -> Result<Decoded, String> {
    if bytes.starts_with(b"%PDF") {
        return Err(pdf().into());
    }
    let zip = bytes.starts_with(b"PK\x03\x04");
    if zip || bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return match workbook(bytes, sheet) {
            Ok(read) => Ok(read),
            Err(_) if zip => zipped(bytes),
            Err(e) => Err(e),
        };
    }
    Ok(decode(bytes))
}

/// A workbook's sheet with the most rows holding a date and a number (or the one `chosen`), as
/// CSV: dates as YYYY-MM-DD, numbers as written without grouping.
fn workbook(bytes: &[u8], chosen: Option<&str>) -> Result<Decoded, String> {
    use calamine::{Data, Reader};
    let mut book = calamine::open_workbook_auto_from_rs(std::io::Cursor::new(bytes))
        .map_err(|e| t!("The spreadsheet couldn't be opened: {e}", e))?;
    let text = |d: &Data| match d {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) => s.trim().to_owned(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        // Cents as cents: a sum Excel worked out can come out as 12.299999999999999.
        Data::Float(f) if ((f * 100.0).round() - f * 100.0).abs() < 1e-6 => format!("{:.2}", f),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.as_datetime().map_or_else(|| dt.as_f64().to_string(), |t| t.format("%Y-%m-%d %H:%M:%S").to_string().trim_end_matches(" 00:00:00").to_owned()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
    };
    let mut best: Option<(usize, String, Vec<Vec<String>>)> = None;
    let sheets = book.sheet_names();
    let picked = chosen.filter(|c| sheets.iter().any(|s| s == c));
    for name in sheets.iter().filter(|s| picked.is_none_or(|p| p == s.as_str())).cloned() {
        let Ok(range) = book.worksheet_range(&name) else { continue };
        let left = range.start().map_or(0, |(_, c)| c as usize);
        let mut score = 0;
        let rows: Vec<Vec<String>> = range.rows().map(|row| {
            let dated = row.iter().any(|d| matches!(d, Data::DateTime(_) | Data::DateTimeIso(_)) || matches!(d, Data::String(s) if is_date(s)));
            let numbered = row.iter().any(|d| matches!(d, Data::Float(_) | Data::Int(_)) || matches!(d, Data::String(s) if s.contains(['.', ',']) && s.bytes().any(|b| b.is_ascii_digit())));
            score += (dated && numbered) as usize;
            std::iter::repeat_n(String::new(), left).chain(row.iter().map(text)).collect()
        }).collect();
        if best.as_ref().is_none_or(|(s, _, _)| score > *s) {
            best = Some((score, name, rows));
        }
    }
    let (_, sheet, rows) = best.ok_or_else(|| t!("The spreadsheet has no sheets."))?;
    let mut out = csv::WriterBuilder::new().flexible(true).from_writer(vec![]);
    for row in &rows {
        let end = row.iter().rposition(|c| !c.is_empty()).map_or(0, |i| i + 1);
        out.write_record(&row[..end]).map_err(|e| e.to_string())?;
    }
    let text = String::from_utf8(out.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(Decoded { text, encoding: "spreadsheet", sheet: Some(sheet), sheets })
}

/// A ZIP of statements, as some banks hand out a day's CAMT.053 files or MT940 together: those,
/// read as one (each statement's entries are its own); anything else, its first file.
fn zipped(bytes: &[u8]) -> Result<Decoded, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| t!("This file is a ZIP archive that doesn't hold a statement."))?;
    let mut files = vec![];
    for i in 0..archive.len().min(400) {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() || entry.size() > 20_000_000 {
            continue;
        }
        let mut data = vec![];
        std::io::Read::read_to_end(&mut std::io::Read::take(&mut entry, 20_000_001), &mut data).map_err(|e| e.to_string())?;
        let name = entry.name().map(|n| n.to_string()).unwrap_or_default();
        files.push((name, decode(&data).text));
    }
    files.sort();
    let kinds: Vec<Option<&str>> = files.iter().map(|(_, t)| crate::bank_statements::sniff(t)).collect();
    let first = kinds.first().copied().flatten();
    let text = match first {
        Some(kind) if ["camt053", "mt940"].contains(&kind) && kinds.iter().all(|k| *k == Some(kind)) => {
            files.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n")
        }
        _ => files.into_iter().next().map(|(_, t)| t).ok_or_else(|| t!("This ZIP archive is empty."))?,
    };
    if text.len() > 20_000_000 {
        return Err(t!("Import a file smaller than 20 MB."));
    }
    Ok(Decoded { text, encoding: "UTF-8", ..Decoded::default() })
}

/// A statement's text from its bytes: as its byte-order mark says, else UTF-8 when it is, else the
/// legacy encoding the bytes fit best (Windows-1250 or -1252 in Europe, Shift_JIS in Japan, GBK in
/// China, Windows-1251 for Cyrillic, -1255 for Hebrew, -1256 for Arabic…), as banks still write.
pub fn decode(bytes: &[u8]) -> Decoded {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        let text = encoding.decode_without_bom_handling(&bytes[bom..]).0.into_owned();
        return Decoded { text, encoding: encoding.name(), ..Decoded::default() };
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Decoded { text: text.to_string(), encoding: "UTF-8", ..Decoded::default() };
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let mut encoding = detector.guess(None, true);
    // A British file that's ASCII but for £ signs reads as Central European, whose Ł is £'s byte:
    // only letters of those languages inside words make it so.
    if [encoding_rs::WINDOWS_1250, encoding_rs::ISO_8859_2].contains(&encoding) && !central_european(bytes) {
        encoding = encoding_rs::WINDOWS_1252;
    }
    Decoded { text: encoding.decode_without_bom_handling(bytes).0.into_owned(), encoding: encoding.name(), ..Decoded::default() }
}
/// Whether bytes that are letters in Windows-1250 (ł, ą, ż, Ł, Ż…) but symbols in Windows-1252
/// (³, ¹, ¿, £, ¯…) come up inside words, as in Polish and Czech text.
fn central_european(bytes: &[u8]) -> bool {
    const LETTERS: [u8; 11] = [0x8D, 0x8F, 0x9D, 0x9F, 0xA3, 0xA5, 0xAF, 0xB3, 0xB9, 0xBE, 0xBF];
    let letter = |i: Option<usize>| i.and_then(|i| bytes.get(i)).is_some_and(|b| b.is_ascii_alphabetic() || *b >= 0xC0);
    (0..bytes.len()).filter(|&i| LETTERS.contains(&bytes[i]) && (letter(i.checked_sub(1)) || letter(Some(i + 1)))).count() >= 2
}
/// `read_statement` for a file the apps read as ISO-8859-1, one character for each byte, which
/// crosses the apps' text boundary unchanged.
pub fn decode_latin1(latin1: &str, sheet: Option<&str>) -> Result<Decoded, String> {
    let bytes = latin1.chars().map(|c| u8::try_from(c as u32)).collect::<Result<Vec<u8>, _>>()
        .map_err(|_| t!("The statement's bytes didn't come through as they were."))?;
    read_sheet(&bytes, sheet)
}

/// A CSV's columns, for choosing which holds what.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// "csv", or the format the file turned out to be in (an MT940 statement saved as .txt), with
    /// no columns to choose.
    pub format: String,
    /// The header row, as the file writes it.
    pub headers: Vec<String>,
    /// The first rows under it.
    pub samples: Vec<Vec<String>>,
    /// The columns an import reads now: the account's saved choice when this file has those
    /// columns, or else the ones found by name.
    pub columns: Columns,
    /// The columns are the account's saved choice.
    pub saved: bool,
    /// The account's date order, or else the only one every date in the file fits.
    pub date_format: Option<String>,
    pub flip_signs: bool,
}

/// With `columns` just chosen, the date order is the one their date column fits before the
/// account's.
pub fn layout(b: &Budget, account: &str, content: &str, columns: Option<&Columns>) -> Result<Layout, String> {
    if !b.accounts.iter().any(|a| a.id == account) {
        return Err(t!("Choose an account."));
    }
    if content.len() > 20_000_000 {
        return Err(t!("Import a file smaller than 20 MB."));
    }
    if let Some(format) = crate::bank_statements::sniff(content) {
        let none = Columns::default();
        return Ok(Layout { format: format.into(), headers: vec![], samples: vec![], columns: none, saved: false, date_format: None, flip_signs: false });
    }
    let kept = settings(b, account);
    let chosen = columns.filter(|c| c.date.is_some());
    let sheet = Sheet::read(content, &b.currency, chosen.or(kept.columns.as_ref()), None)?;
    let visible = sheet.visible();
    let days: Vec<&str> = sheet.picked.date.into_iter()
        .flat_map(|d| visible.iter().filter_map(move |r| r.get(d)))
        .filter(|s| !s.is_empty()).collect();
    let found = crate::bank_import::date_order(&days).map(String::from);
    // A card's statement that's mostly money in writes purchases as positive numbers.
    let card = b.accounts.iter().any(|a| a.id == account && a.kind == crate::domain::AccountKind::Credit);
    let signs: Vec<i64> = sheet.picked.amount.filter(|_| sheet.picked.direction.is_none()).into_iter()
        .flat_map(|i| visible.iter().filter_map(move |r| r.get(i)))
        .filter_map(|c| money(c, &b.currency).ok()).filter(|n| *n != 0).map(i64::signum).collect();
    let positive = signs.len() >= 3 && signs.iter().filter(|s| **s > 0).count() * 2 > signs.len();
    Ok(Layout {
        format: "csv".into(),
        columns: sheet.columns(),
        saved: sheet.saved && chosen.is_none(),
        samples: visible.iter().take(3).map(|r| r.iter().map(String::from).collect()).collect(),
        date_format: if chosen.is_some() { found.or(kept.date_format) } else { kept.date_format.or(found) },
        flip_signs: kept.flip_signs.unwrap_or(card && positive),
        headers: sheet.headers,
    })
}

/// How to read a CSV's records beyond what its names say.
#[derive(Default, Clone)]
pub(crate) struct Options<'a> {
    /// The columns chosen for this file (without a date: find them by name); None for the
    /// account's saved choice when the file has those columns.
    pub columns: Option<&'a Columns>,
    /// Where the header row starts, as someone chose it.
    pub header_at: Option<usize>,
    pub flip_signs: bool,
    /// Which account's rows, by its label, in a file with several.
    pub source: Option<&'a str>,
    /// Records someone included though they're left out, by where they start.
    pub include: HashSet<usize>,
    /// Cells someone fixed, by where their record starts: (column, value).
    pub cells: HashMap<usize, Vec<(usize, String)>>,
    /// Values someone gave a record's fields, whatever its columns say, by where it starts: a
    /// date (YYYY-MM-DD), an amount (positive for money in), a payee, a memo.
    pub fields: HashMap<usize, Vec<(Field, String)>>,
    /// Keep each record's cells, to show them (today's import doesn't need them).
    pub shown: bool,
}

/// What became of a CSV record, read up to its date (dates are read together, once their order
/// is known).
#[derive(Clone, Debug)]
pub(crate) struct Record {
    /// Where the record starts in the file as written: its line's id.
    pub offset: usize,
    /// Its cells, with any fixes.
    pub cells: Vec<String>,
    /// Its cells as the file writes them, when some were fixed.
    pub written: Vec<String>,
    /// The bank's ID for it as the file writes it, or "".
    pub bank_id: String,
    /// What it read as without help (its date as written, and its fate), when it's read again with
    /// fixes or included.
    pub first: Option<(Option<String>, Fate)>,
    /// Its date as written (with any fix), where it got that far.
    pub day: Option<String>,
    /// Its date as the file writes it, where today's reader would read it: what the file's dates
    /// say about their order comes from these alone, never from fixes.
    pub evidence: Option<String>,
    pub fate: Fate,
    /// Today's reader reads it (it passes every check that leaves rows out first, then reads or
    /// fails to read its amount): it holds its place among rows with a repeated bank id.
    pub reserves: bool,
    /// Today's reader never saw it (see `Sheet::hidden`).
    pub hidden: bool,
    /// How 1.0.8 read it (its bank ID, payee, memo and amount), where it could: the transactions
    /// it imported are known by the IDs it gave them.
    pub then: Option<(String, String, String, i64)>,
}
#[derive(Clone, Debug)]
pub(crate) enum Fate {
    /// Read, all but its date.
    Read { fit: String, payee: String, memo: String, amount: i64, transfer_account_id: Option<String> },
    Attention { field: Field, column: Option<usize>, reason: String },
    LeftOut { reason: Reason, detail: String },
}

/// A CSV's records and what was read from it.
pub(crate) struct Records {
    pub headers: Vec<String>,
    pub columns: Columns,
    /// The columns are the account's saved choice.
    pub saved: bool,
    /// Where the header row (or, without one, the first record) starts.
    pub start: usize,
    pub separator: u8,
    pub date_column: usize,
    /// Where the payee, memo and amounts are read from (amount, money out, money in), and the
    /// bank's ID.
    pub payee_column: Option<usize>,
    pub memo_column: Option<usize>,
    pub amount_columns: [Option<usize>; 3],
    pub id_column: Option<usize>,
    pub transfer_column: Option<usize>,
    /// The accounts in a file with several (labels and how many dated rows each has), and which
    /// is read.
    pub sources: Vec<(String, usize)>,
    pub source: usize,
    pub records: Vec<Record>,
    /// Another currency some of the read account's rows are in.
    pub foreign: Option<String>,
}

/// A CSV's records, each with what became of it: nothing a row holds stops the others from
/// reading. Only a file without a date or amount column, or one whose chosen columns are gone,
/// is an error. What the file says about itself (its sections, its account column, who holds
/// the account, which records today's reader would read) comes from the file as written; fixes
/// and inclusions change only their own lines.
pub(crate) fn records(b: &Budget, account: &str, content: &str, o: &Options) -> Result<Records, String> {
    let saved = settings(b, account).columns;
    let merged;
    let chosen = match o.columns {
        Some(c) if c.date.is_some() => Some(c),
        // Some roles chosen (the assistant's amount column, say): the others as the file names
        // them, with those chosen laid over them.
        Some(c) => {
            let found = Sheet::read(content, &b.currency, saved.as_ref(), o.header_at)?.columns();
            let pick = |mine: &Option<String>, theirs: &Option<String>| mine.clone().or_else(|| theirs.clone());
            merged = Columns {
                date: found.date.clone(),
                payee: pick(&c.payee, &found.payee),
                memo: pick(&c.memo, &found.memo),
                // One amount column, or money out and in apart: what's chosen decides which.
                amount: if c.outflow.is_some() || c.inflow.is_some() { None } else { pick(&c.amount, &found.amount) },
                outflow: if c.amount.is_some() { None } else { pick(&c.outflow, &found.outflow) },
                inflow: if c.amount.is_some() { None } else { pick(&c.inflow, &found.inflow) },
                id: pick(&c.id, &found.id),
                transfer_account_id: pick(&c.transfer_account_id, &found.transfer_account_id),
            };
            Some(&merged).filter(|m| m.date.is_some())
        }
        None => saved.as_ref(),
    };
    let mut sheet = Sheet::read(content, &b.currency, chosen, o.header_at)?;
    // Rows under another account's header row have that row's columns: none is mended to fit these.
    let other = (0..sheet.records.len()).find(|&k| !sheet.hidden[k] && sheet.other_header(&sheet.records[k]));
    sheet.mend(&b.currency, other.unwrap_or(sheet.records.len()));
    if let Some(c) = chosen.filter(|c| o.columns.is_some() && c.date.is_some() && !sheet.saved) {
        let missing = c.names().into_iter().flatten().find(|n| position(&sheet.headers, n).is_none());
        return Err(match missing {
            Some(n) => t!("This file has no column named ‘{name}’. Choose its columns again.", name = n),
            None => t!("Choose the file's columns again."),
        });
    }
    let p = &sheet.picked;
    let d = p.date.ok_or_else(|| t!("CSV needs Date, Payee/Description, and Amount (or Outflow/Inflow) headers."))?;
    if p.amount.is_none() && p.outflow.is_none() && p.inflow.is_none() {
        let named_code = sheet.headers.iter().map(|s| header(s)).find_map(|(s, code)| {
            [AMOUNTS, OUTFLOWS, INFLOWS].iter().any(|n| named(&s, n, &b.currency)).then_some(code).flatten()
        });
        return Err(match named_code {
            Some(code) => t!("The file's amounts are in {code}, but this budget uses {currency}.", code, currency = b.currency),
            None => t!("CSV needs Amount or Outflow/Inflow columns."),
        });
    }
    fn cell(r: &csv::StringRecord, i: Option<usize>) -> &str {
        i.and_then(|i| r.get(i)).unwrap_or("")
    }
    let dated = |r: &csv::StringRecord| cell(r, Some(d)).bytes().any(|c| c.is_ascii_digit());
    // An empty cell, or a dash for one (Monobank's "—").
    let blank = |s: &str| s.chars().all(|c| matches!(c, '-' | '—' | '–'));
    let read = |s: &str| if blank(s) { Ok(0) } else { money(s, &b.currency) };
    let shown: Vec<&csv::StringRecord> = sheet.records.iter().zip(&sheet.hidden).filter(|(_, h)| !**h).map(|(r, _)| r).collect();
    // With a column for each party, the account holder is the name that comes up most in them.
    let holder = p.parties.map(|(a, b)| {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for r in &shown {
            for i in [a, b] {
                let name = cell(r, Some(i));
                if !name.is_empty() {
                    *seen.entry(name).or_default() += 1;
                }
            }
        }
        seen.into_iter().max_by_key(|&(name, n)| (n, std::cmp::Reverse(name))).map(|(name, _)| name).unwrap_or("")
    });
    // A file with every account's transactions, each row saying whose. That column names an
    // account on every row, a few accounts in turn, the first more than once; the other party's
    // account changes from row to row, and is often blank.
    let with_dates: Vec<&&csv::StringRecord> = shown.iter().filter(|r| dated(r)).collect();
    let own = p.accounts.iter().find_map(|&i| {
        let values: Vec<&str> = with_dates.iter().map(|r| cell(r, Some(i))).collect();
        let mut blocks: Vec<&str> = values.clone();
        blocks.dedup();
        let distinct: HashSet<&&str> = blocks.iter().collect();
        let first = *values.first()?;
        let ok = !values.contains(&"") && distinct.len() == blocks.len() && blocks.len() >= 2 && blocks.len() * 2 <= values.len()
            && values.iter().filter(|v| **v == first).count() >= 2;
        ok.then(|| (i, blocks.iter().map(|b| (b.to_string(), values.iter().filter(|v| *v == b).count())).collect::<Vec<_>>()))
    });
    // A record as it reads, someone having included it or not, with any values given for its
    // fields, and whether someone wrote its amount in (in the budget's currency, then).
    let classify = |r: &csv::StringRecord, included: bool, given: &[(Field, String)], amount_written: bool| -> (Option<String>, Fate) {
        let lift = |reason: Reason| included && reason.includable();
        let left = |reason: Reason, detail: &str| Fate::LeftOut { reason, detail: detail.to_string() };
        let attention = |field: Field, column: Option<usize>, reason: String| Fate::Attention { field, column, reason };
        let value = |field: Field| given.iter().rev().find(|(f, _)| *f == field).map(|(_, v)| v.trim());
        // A note, a total, or a pending row without its date yet: no digits where the date goes.
        let day = cell(r, Some(d));
        // (Included, with a date given, it reads.)
        if !(dated(r) || lift(Reason::NoDate) && value(Field::Date).is_some()) {
            return (None, if lift(Reason::NoDate) { attention(Field::Date, Some(d), t!("This line has no date.")) } else { left(Reason::NoDate, day) });
        }
        let day = Some(value(Field::Date).unwrap_or(day).to_string());
        let given_amount = value(Field::Amount);
        // An amount in another currency, written over in the budget's.
        let mut converted = false;
        // An opening or closing balance, or a turnover total, written as a row.
        if let Some(c) = r.iter().find(|c| BALANCES.contains(&plain(c).as_str())) {
            if !lift(Reason::Balance) {
                return (day, left(Reason::Balance, c));
            }
        }
        // A payment the bank reversed, declined or never made, or hasn't booked yet (with a
        // booking date still to come, too): a pending row can come back booked on another day or
        // for another sum, so it's read once booked.
        if let Some(state) = p.states.iter().map(|&i| cell(r, Some(i))).find(|s| SKIPPED.contains(&plain(s).as_str())) {
            let reason = if DECLINED.contains(&plain(state).as_str()) { Reason::Declined } else { Reason::Pending };
            if !lift(reason) {
                return (day, left(reason, state));
            }
        }
        if p.posted.is_some_and(|i| cell(r, Some(i)).is_empty()) && !lift(Reason::Pending) {
            return (day, left(Reason::Pending, ""));
        }
        // Money in another currency, in a file that mixes them (a conversion's other leg).
        if let Some(code) = p.currencies.iter().map(|&i| cell(r, Some(i))).find(|c| {
            c.len() == 3 && c.bytes().all(|b| b.is_ascii_alphabetic()) && !c.eq_ignore_ascii_case(&b.currency)
        }) {
            let code = code.to_ascii_uppercase();
            if !lift(Reason::OtherCurrency) {
                return (day, left(Reason::OtherCurrency, &code));
            }
            converted = amount_written;
            if given_amount.is_none() && !amount_written {
                return (day, attention(Field::Amount, p.amount.or(p.outflow).or(p.inflow), t!("This line is in {code}: enter the amount in {currency}.", code, currency = b.currency)));
            }
        }
        // Money moved between the holder's own balances, by the file's own account.
        if let Some(way) = p.direction.map(|i| cell(r, Some(i))).filter(|w| *w == "/" || NEUTRAL.contains(&plain(w).as_str())) {
            if !lift(Reason::OwnBalances) {
                return (day, left(Reason::OwnBalances, way));
            }
        }
        // A row whose amount is empty states something (a balance) rather than moving money.
        if [p.amount, p.outflow, p.inflow].iter().all(|&i| blank(cell(r, i))) {
            if !lift(Reason::NoAmount) {
                return (day, left(Reason::NoAmount, ""));
            }
            if given_amount.is_none() {
                return (day, attention(Field::Amount, p.amount.or(p.outflow).or(p.inflow), t!("This line has no amount.")));
            }
        }
        // An amount given for the line is what it moved, whatever its columns and signs say.
        let amount = if let Some(given) = given_amount {
            match money(given, &b.currency) {
                Err(e) => return (day, attention(Field::Amount, None, e)),
                Ok(0) => return (day, attention(Field::Amount, None, t!("Enter an amount other than zero."))),
                Ok(n) => n,
            }
        // An empty amount, with money out and in in columns of their own, is read from those.
        } else if p.amount.is_some() && !(blank(cell(r, p.amount)) && (p.outflow.is_some() || p.inflow.is_some())) {
            match money(cell(r, p.amount), &b.currency) {
                Err(e) => return (day, attention(Field::Amount, p.amount, e)),
                Ok(n) => match p.direction.map(|i| plain(cell(r, Some(i)))) {
                    Some(w) if OUT.contains(&w.as_str()) => -n.abs(),
                    Some(w) if IN.contains(&w.as_str()) => n.abs(),
                    // Some card issuers write purchases as positive numbers in one Amount column.
                    _ if o.flip_signs => -n,
                    _ => n,
                },
            }
        } else {
            // Money out and money in, which some banks write with a minus and some without.
            match (read(cell(r, p.inflow)), read(cell(r, p.outflow))) {
                (Err(e), _) => return (day, attention(Field::Amount, p.inflow, e)),
                (_, Err(e)) => return (day, attention(Field::Amount, p.outflow, e)),
                (Ok(i), Ok(out)) => i.abs() - out.abs(),
            }
        };
        let amount = match read(cell(r, p.fee)) {
            _ if given_amount.is_some() || converted => amount,
            Err(e) => return (day, attention(Field::Amount, p.fee, e)),
            Ok(fee) => amount - fee.abs(),
        };
        // A notice of nothing moving (a statement closing, a card check).
        if amount == 0 {
            return (day, left(Reason::Zero, ""));
        }
        // The party that isn't the account holder (the holder, for money moved between their own
        // accounts), or else a column that only names the other party, or else the payee's.
        let party = p.parties.and_then(|(a, b)| {
            let named = [a, b].map(|i| cell(r, Some(i)));
            named.into_iter().find(|n| !n.is_empty() && Some(*n) != holder).or(named.into_iter().find(|n| !n.is_empty()))
        });
        let name = p.names.iter().map(|&i| cell(r, Some(i))).find(|n| !n.trim().is_empty());
        let (mut payee, mut memo) = (tidy(party.or(name).unwrap_or(cell(r, p.payee))), tidy(cell(r, p.memo)));
        // A payee from a column that only names them keeps the bank's description as the memo,
        // where the file has no other.
        let described = tidy(cell(r, p.payee));
        if party.is_none() && name.is_some() && memo.is_empty() && described != payee {
            memo = described;
        }
        // A card payment often names the shop only in the description; a top-up or a conversion
        // may say only what kind of payment it is.
        if payee.is_empty() {
            payee = std::mem::take(&mut memo);
        }
        if payee.is_empty() {
            payee = tidy(cell(r, p.kind));
        }
        if let Some(given) = value(Field::Payee) {
            payee = given.to_string();
        }
        if let Some(given) = value(Field::Memo) {
            memo = given.to_string();
        }
        let target = value(Field::TransferAccountId).unwrap_or_else(|| cell(r, p.transfer_account_id)).trim();
        if let Err(reason) = crate::bank_import::validate_transfer(b, account, target) {
            return (day, attention(Field::TransferAccountId, p.transfer_account_id, reason));
        }
        let transfer_account_id = (!target.is_empty()).then(|| target.to_string());
        (day, Fate::Read { fit: cell(r, p.id).into(), payee, memo, amount, transfer_account_id })
    };
    // First the file as written: each record's section (a header row after something other than
    // a transaction starts another account's rows, as comdirect exports: this header again, or
    // one of its own) and what today's reader makes of it.
    let mut originals: Vec<Option<(Option<String>, Fate)>> = Vec::with_capacity(sheet.records.len());
    let (mut sections, mut labels) = (vec![], vec![String::new()]);
    // Each section's own header row, where it has other columns than this one.
    let mut headed: Vec<Option<usize>> = vec![None];
    let (mut section, mut between, mut read_here) = (0, false, 0);
    // The section's name: its heading ("Umsätze Tagesgeld PLUS-Konto"), not the balance under it,
    // which changes from file to file; else the first cell of the last line before its header.
    let (mut heading, mut note): (Option<String>, String) = (None, String::new());
    for (k, (r, hidden)) in sheet.records.iter().zip(&sheet.hidden).enumerate() {
        if *hidden {
            originals.push(None);
            sections.push(section);
            continue;
        }
        let other = sheet.other_header(r);
        if other || r.len() == sheet.headers.len() && r.iter().zip(&sheet.headers).all(|(c, h)| plain(c) == plain(h)) {
            if between && read_here > 0 {
                section += 1;
                read_here = 0;
                labels.push(heading.take().unwrap_or_else(|| std::mem::take(&mut note)));
                headed.push(None);
            }
            if other {
                headed[section] = Some(k);
            }
            originals.push(None);
            sections.push(section);
            continue;
        }
        if !dated(r) {
            between = true;
            note = tidy(r.iter().find(|c| !c.is_empty()).unwrap_or(""));
            let balance = r.iter().filter(|c| !c.is_empty()).skip(1).any(|c| money(c, &b.currency).is_ok());
            if heading.is_none() && !balance && !note.is_empty() {
                heading = Some(note.clone());
            }
        }
        let (day, fate) = classify(r, false, &[], false);
        if matches!(fate, Fate::Read { .. }) {
            between = false;
            read_here += 1;
            heading = None;
        }
        originals.push(Some((day, fate)));
        sections.push(section);
    }
    // The accounts: by the account column where there is one, else by section.
    let (sources, by_column): (Vec<(String, usize)>, bool) = match &own {
        Some((_, blocks)) => (blocks.clone(), true),
        None if section > 0 => {
            let counts: Vec<usize> = (0..=section).map(|s| sections.iter().zip(&originals).filter(|(x, o)| **x == s && o.is_some()).count()).collect();
            let named = labels.iter().enumerate().map(|(i, l)| if l.is_empty() { format!("Part {}", i + 1) } else { l.clone() });
            (named.zip(counts).collect(), false)
        }
        None => (vec![], false),
    };
    let source = o.source.and_then(|s| sources.iter().position(|(l, _)| l == s)).unwrap_or(0);
    // The account chosen has a header row of its own: its rows are read by its columns, from it
    // (the others' rows above it are then shown as lines above the header).
    if let Some(k) = headed.get(source).copied().flatten().filter(|_| own.is_none() && o.header_at.is_none()) {
        // (A record's place takes in the blank lines before it.)
        let text = &content.trim_start_matches('\u{feff}')[sheet.offsets[k]..];
        let at = sheet.offsets[k] + text.len() - text.trim_start_matches(['\r', '\n']).len();
        let mut read = records(b, account, content, &Options { header_at: Some(at), source: None, ..o.clone() })?;
        (read.sources, read.source) = (sources, source);
        return Ok(read);
    }
    // A file 1.0.8 read (its header the first line) the way it read it, where its date is in the
    // same column: its columns by its names, cells only trimmed, no fee taken off.
    let then = Then::columns(&sheet.headers, &b.currency).filter(|t| sheet.start == 0 && t.date == d);
    let then_of = |r: &csv::StringRecord| -> Option<(String, String, String, i64)> {
        let t = then.as_ref()?;
        let val = |i: Option<usize>| cell(r, i).trim();
        let amount = if t.amount.is_some() {
            let n = money(val(t.amount), &b.currency).ok()?;
            if o.flip_signs { -n } else { n }
        } else {
            let n = |s: &str| if s.is_empty() { Ok(0) } else { money(s, &b.currency) };
            n(val(t.inflow)).ok()? - n(val(t.outflow)).ok()?
        };
        Some((val(t.fit).to_string(), val(t.payee).to_string(), val(t.memo).to_string(), amount))
    };
    let mut out = Vec::with_capacity(sheet.records.len());
    let mut foreign = None;
    for (k, original) in sheet.records.iter().enumerate() {
        let offset = sheet.offsets[k];
        let included = o.include.contains(&offset);
        let fixed = o.cells.get(&offset).map(|fixes| {
            let mut cells: Vec<String> = original.iter().map(String::from).collect();
            for (column, value) in fixes {
                if cells.len() <= *column {
                    cells.resize(column + 1, String::new());
                }
                cells[*column] = value.trim().to_string();
            }
            csv::StringRecord::from(cells)
        });
        let r = fixed.as_ref().unwrap_or(original);
        let given = o.fields.get(&offset).map(Vec::as_slice);
        let amount_written = o.cells.get(&offset).is_some_and(|fixes| fixes.iter().any(|(c, _)| [p.amount, p.outflow, p.inflow].contains(&Some(*c))));
        let cells: Vec<String> = if o.shown { r.iter().map(String::from).collect() } else { vec![] };
        let written: Vec<String> = if o.shown && fixed.is_some() { original.iter().map(String::from).collect() } else { vec![] };
        let mut record = Record {
            offset,
            cells,
            written,
            bank_id: cell(original, p.id).to_string(),
            first: None,
            day: None,
            evidence: None,
            fate: Fate::LeftOut { reason: Reason::RepeatedHeader, detail: String::new() },
            reserves: false,
            hidden: sheet.hidden[k],
            then: None,
        };
        // A line of a statement in aligned columns that today's reader dropped for having no date.
        if record.hidden {
            let (day, fate) = classify(r, included, given.unwrap_or(&[]), amount_written);
            record.fate = fate;
            record.day = day;
            out.push(record);
            continue;
        }
        let Some((day0, fate0)) = &originals[k] else {
            out.push(record);
            continue;
        };
        // Another account's rows: by its column (a dated row naming another), or its section.
        let elsewhere = match &own {
            Some((i, _)) if by_column => {
                let whose = cell(original, Some(*i));
                // Undated too: a line of another account's, its date fixed, is still that account's.
                !whose.is_empty() && whose != sources[source].0
            }
            _ => !sources.is_empty() && sections[k] != source,
        };
        if elsewhere {
            let label = match &own {
                Some((i, _)) => cell(original, Some(*i)).to_string(),
                None => sources.get(sections[k]).map(|(l, _)| l.clone()).unwrap_or_default(),
            };
            record.fate = Fate::LeftOut { reason: Reason::OtherAccount, detail: label };
            out.push(record);
            continue;
        }
        record.reserves = matches!(fate0, Fate::Read { .. } | Fate::Attention { .. });
        record.evidence = day0.clone().filter(|_| record.reserves);
        record.then = then_of(original);
        let (day, fate) = if fixed.is_some() || included || given.is_some() {
            record.first = Some((day0.clone(), fate0.clone()));
            classify(r, included, given.unwrap_or(&[]), amount_written)
        } else {
            (day0.clone(), fate0.clone())
        };
        if let Fate::LeftOut { reason: Reason::OtherCurrency, detail } = fate0 {
            foreign.get_or_insert(detail.clone());
        }
        record.day = day;
        record.fate = fate;
        out.push(record);
    }
    Ok(Records {
        columns: sheet.columns(),
        saved: sheet.saved,
        start: sheet.start,
        separator: sheet.separator,
        date_column: d,
        payee_column: p.payee,
        memo_column: p.memo,
        amount_columns: [p.amount, p.outflow, p.inflow],
        id_column: p.id,
        transfer_column: p.transfer_account_id,
        sources,
        source,
        records: out,
        foreign,
        headers: sheet.headers,
    })
}
/// The columns 1.0.8 read a CSV by: for each, the first in the file named one of its names
/// (without case, spaces or punctuation), a money column naming another currency left out.
struct Then {
    date: usize,
    payee: Option<usize>,
    memo: Option<usize>,
    amount: Option<usize>,
    outflow: Option<usize>,
    inflow: Option<usize>,
    fit: Option<usize>,
}
impl Then {
    fn columns(headers: &[String], currency: &str) -> Option<Self> {
        let h: Vec<(String, Option<&str>)> = headers
            .iter()
            .map(|s| {
                let s = s.trim();
                match s.strip_suffix(')').and_then(|s| s.rsplit_once('(')) {
                    Some((name, code)) if code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()) => (name, Some(code)),
                    _ => (s, None),
                }
            })
            .map(|(name, code)| (name.to_lowercase().replace([' ', '_', '-', '.', '(', ')'], ""), code))
            .collect();
        let at = |names: &[&str]| h.iter().position(|(s, _)| names.contains(&s.as_str()));
        let money = |names: &[&str]| h.iter().position(|(s, code)| names.contains(&s.as_str()) && code.is_none_or(|c| c == currency));
        let then = Self {
            date: at(&["date", "transactiondate", "posteddate", "transdate"]).or(at(&["postingdate", "postdate"]))?,
            payee: at(&["payee", "name", "merchant", "description", "transactiondescription"]),
            memo: at(&["memo", "notes"]),
            amount: money(&["amount", "transactionamount"]),
            outflow: money(&["outflow", "debit", "withdrawal"]),
            inflow: money(&["inflow", "credit", "deposit"]),
            fit: at(&["fitid", "transactionid", "id"]),
        };
        (then.amount.is_some() || then.outflow.is_some() || then.inflow.is_some()).then_some(then)
    }
}

/// Bank text loses its line breaks, tabs and padding, and the marks that set the direction of
/// right-to-left text (an Israeli card's merchants).
fn tidy(s: &str) -> String {
    let s: String = s.chars().filter(|c| !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A CSV's transactions as today's import reads them: every record must read, or the first that
/// doesn't is the error. `columns` are the ones chosen for this file (without a date: find them
/// by name); without any, the account's saved choice when the file has those columns.
pub(crate) fn read(
    b: &Budget,
    account: &str,
    content: &str,
    date_format: &str,
    flip_signs: bool,
    columns: Option<&Columns>,
) -> Result<Vec<BankTransaction>, String> {
    let read = records(b, account, content, &Options { columns, flip_signs, ..Options::default() })?;
    let shown = || read.records.iter().filter(|r| !r.hidden);
    if let Some(reason) = shown().find_map(|r| match &r.fate {
        Fate::Attention { reason, .. } => Some(reason.clone()),
        _ => None,
    }) {
        return Err(reason);
    }
    let (mut raw, mut days) = (vec![], vec![]);
    for r in shown() {
        if let Fate::Read { fit, payee, memo, amount, transfer_account_id } = &r.fate {
            raw.push(BankTransaction { fit: fit.clone(), date: String::new(), payee: payee.clone(), memo: memo.clone(), amount: *amount, transfer_account_id: transfer_account_id.clone() });
            days.push(r.day.clone().unwrap_or_default());
        }
    }
    if let (true, Some(code)) = (raw.is_empty(), &read.foreign) {
        return Err(t!("The file's amounts are in {code}, but this budget uses {currency}.", code, currency = b.currency));
    }
    for (t, day) in raw.iter_mut().zip(dates(&days, date_format)?) {
        t.date = day;
    }
    Ok(raw)
}

/// A line of a CSV, as an import shows it.
pub(crate) struct CsvLine {
    pub offset: usize,
    pub cells: Vec<String>,
    /// Its cells as the file writes them, when some were fixed.
    pub written: Vec<String>,
    pub status: Status,
    /// The transaction it gives, when it's ready.
    pub transaction: Option<BankTransaction>,
    /// The net amount already read, even when the date prevents a complete transaction.
    pub parsed_amount: Option<i64>,
    /// The transaction it gives without help, when it does: its identity is made from this.
    pub original: Option<BankTransaction>,
    /// The bank's ID for it as the file writes it, or "".
    pub bank_id: String,
    pub reserves: bool,
    /// The transaction 1.0.8 read it as, where it read without help: it's known by its ID too.
    pub then: Option<BankTransaction>,
}
/// A CSV as lines: its records with their dates read in the order the file's own dates settle,
/// and the lines above its header.
pub(crate) struct CsvLines {
    pub records: Records,
    pub dates: DateReading,
    pub lines: Vec<CsvLine>,
}
pub(crate) fn read_lines(b: &Budget, account: &str, content: &str, o: &Options, date_format: &str, confirmed: bool) -> Result<CsvLines, String> {
    let mut records = records(b, account, content, &Options { shown: true, ..o.clone() })?;
    // What the file's own dates say about their order, from the lines today's reader reads.
    let evidence: Vec<&str> = records.records.iter().filter_map(|r| r.evidence.as_deref()).collect();
    let dates = read_dates(&evidence, date_format, confirmed);
    let column = Some(records.date_column);
    let content = content.trim_start_matches('\u{feff}');
    // Account details or a title above the header: shown, not read.
    let mut lines: Vec<CsvLine> = above(content, records.start, records.separator)
        .into_iter()
        .map(|(offset, cells)| CsvLine {
            offset,
            cells,
            written: vec![],
            status: Status::LeftOut { reason: Reason::AboveHeader, detail: String::new() },
            transaction: None,
            parsed_amount: None,
            original: None,
            bank_id: String::new(),
            reserves: false,
            then: None,
        })
        .collect();
    // A record that read, all but its date, with its date read in the file's order (each date
    // once: a day has several transactions).
    let read: RefCell<HashMap<String, Result<String, String>>> = RefCell::default();
    let dated = |day: &Option<String>, fit: &str, payee: &str, memo: &str, amount: i64, transfer_account_id: &Option<String>| -> Result<BankTransaction, String> {
        let day = day.as_deref().unwrap_or("");
        let date = read.borrow_mut().entry(day.to_string())
            .or_insert_with(|| crate::bank_import::date(day, dates.order).map_err(|_| t!("Can't read the date ‘{date}’.", date = day)))
            .clone()?;
        Ok(BankTransaction { fit: fit.into(), date, payee: payee.into(), memo: memo.into(), amount, transfer_account_id: transfer_account_id.clone() })
    };
    for r in std::mem::take(&mut records.records) {
        let parsed_amount = match &r.fate { Fate::Read { amount, .. } => Some(*amount), _ => None };
        let (status, transaction) = match &r.fate {
            Fate::Read { fit, payee, memo, amount, transfer_account_id } => match dated(&r.day, fit, payee, memo, *amount, transfer_account_id) {
                Ok(t) => (Status::Ready, Some(t)),
                Err(reason) => (Status::Attention { field: Field::Date, column, reason }, None),
            },
            Fate::Attention { field, column, reason } => (Status::Attention { field: *field, column: *column, reason: reason.clone() }, None),
            Fate::LeftOut { reason, detail } => (Status::LeftOut { reason: *reason, detail: detail.clone() }, None),
        };
        let original = match &r.first {
            Some((day, Fate::Read { fit, payee, memo, amount, transfer_account_id })) => dated(day, fit, payee, memo, *amount, transfer_account_id).ok(),
            Some(_) => None,
            None => transaction.clone(),
        };
        let then = original.as_ref().zip(r.then).map(|(o, (fit, payee, memo, amount))| BankTransaction { fit, date: o.date.clone(), payee, memo, amount, transfer_account_id: None });
        lines.push(CsvLine { offset: r.offset, cells: r.cells, written: r.written, status, transaction, parsed_amount, original, bank_id: r.bank_id, reserves: r.reserves, then });
    }
    Ok(CsvLines { records, dates, lines })
}
/// The lines above the header row with something in them (not only separators), as cells.
fn above(content: &str, start: usize, separator: u8) -> Vec<(usize, Vec<String>)> {
    all_lines(&content[..start.min(content.len())])
        .into_iter()
        .map(|(offset, line)| (offset, fields(line, separator)))
        .filter(|(_, cells)| cells.iter().any(|c| !c.is_empty()))
        .collect()
}
/// A CSV whose columns aren't found (or no longer there): its header row (or its widest line, or
/// "Column 1"… for one without), and every line as cells, those above the header marked, for
/// someone to say which column holds what.
pub(crate) struct Unread {
    pub headers: Vec<String>,
    pub start: usize,
    pub lines: Vec<(usize, Vec<String>, bool)>,
}
pub(crate) fn unread(content: &str, currency: &str, header_at: Option<usize>) -> Result<Unread, String> {
    let content = content.trim_start_matches('\u{feff}');
    let sheet = Sheet::read(content, currency, None, header_at)?;
    let mut lines: Vec<(usize, Vec<String>, bool)> = above(content, sheet.start, sheet.separator).into_iter().map(|(o, c)| (o, c, true)).collect();
    lines.extend(sheet.records.iter().zip(&sheet.offsets).map(|(r, &offset)| (offset, r.iter().map(String::from).collect(), false)));
    Ok(Unread { headers: sheet.headers, start: sheet.start, lines })
}
/// Every line with something on it, and where it starts; lines end in CR LF, LF or a lone CR.
fn all_lines(content: &str) -> Vec<(usize, &str)> {
    let (mut out, mut start, mut i) = (vec![], 0, 0);
    let bytes = content.as_bytes();
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'\n' || bytes[i] == b'\r' {
            let line = &content[start..i];
            if !line.trim().is_empty() {
                out.push((start, line));
            }
            i += if bytes.get(i) == Some(&b'\r') && bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
            start = i;
        } else {
            i += 1;
        }
    }
    out
}

/// Header names banks give each column, after `plain`. A column from an earlier list wins; within
/// a list, the file's first.
const DATES: &[&[&str]] = &[
    // When it happened, or the only date.
    &[
        "date", "transactiondate", "transdate", "starteddate", "dateop", "dateoperation", "datedoperation",
        "datedeloperation", "datum", "fecha", "fechaoperacion", "data", "dataoperazione", "dataoperacji",
        "datamov", "datamovimento", "dato", "transaktionsdatum", "transaktionsdag", "belegdatum", "abschlussdatum",
        "datatransakcji", "tradedate", "uitvoeringsdatum", "datetime", "dateandtime", "交易时间", "交易日期", "日付",
        "取引日", "年月日", "датаоперации", "датаiчасоперації", "датаічасоперації", "дата", "תאריך", "תאריךרכישה",
        "תאריךעסקה", "التاريخ", "تاريخ", "تاريخالعملية", "تاريخالحركة",
    ],
    // When the bank booked it.
    &[
        "postingdate", "postdate", "posteddate", "completeddate", "bogfort", "bogforingsdato", "bookingdate", "buchungstag", "buchungsdatum", "buchung",
        "boekdatum", "boekingsdatum", "bokforingsdag", "bokforingsdatum", "kirjauspaiva", "dataksiegowania", "datarozliczenia", "датаплатежа",
    ],
    // When it counted for interest.
    &[
        "valuedate", "valutadatum", "wertstellung", "valuta", "dateval", "datevaleur", "datedevaleur",
        "fechavalor", "datavaluta", "datavalor", "rentedatum", "rentedato", "interestdate", "יוםערך",
    ],
];
const PAYEES: &[&[&str]] = &[
    &[
        "payee", "name", "naam", "merchant", "description", "description1", "transactionref1", "descrizionecompleta", "naturedeloperation", "dettagli", "transactiondescription",
        "payeename", "merchantname", "partnername", "counterpartyname", "naamtegenpartij", "naamomschrijving", "omschrijving",
        "nadawcaodbiorca", "odbiorcazleceniodawca", "avisierungstext", "beschreibung1", "beschreibung", "gavejas", "namn",
        "beguenstigterzahlungspflichtiger", "begunstigterzahlungspflichtiger", "auftraggeberbegunstigter",
        "begunstigterauftraggeber", "beguenstigterauftraggeber",
        "auftraggeberbeguenstigter", "auftraggeberempfanger", "namezahlungsbeteiligter", "zahlungsempfanger",
        "empfanger", "beneficiary", "beneficiaire", "libelle", "libelleoperation", "label", "concepto",
        "descripcion", "descrizione", "descricao", "opis", "tekst", "text", "beskrivning", "forklaring", "saajamaksaja",
        "narration", "transactionremarks", "remarks", "описание", "описоперации", "описоперації", "деталіоперації", "摘要",
        "内容", "入出金内容", "תיאורהתנועה", "תיאור", "الوصف", "البيان", "التفاصيل", "وصفالعملية",
    ],
    // What some banks write about a transaction when they don't name the other party.
    &[
        "transactie", "counterparty", "details", "transactiondetails", "narrative", "particulars", "reference", "libelleinterbancaire",
        "paymentreference", "verwendungszweck", "buchungstext", "mededelingen", "opisoperacji", "tytul", "opistransakcji",
        "rubrik", "vorgangverwendungszweck",
    ],
];
/// Columns for each party to a payment, one of which is the account holder.
const PARTIES: &[&str] = &[
    "zahlungspflichtiger", "zahlungspflichtige", "zahlungsempfangerin", "zahlungsempfanger", "auftraggeberzahlungsempfanger",
    "empfangerzahlungspflichtiger", "payer", "payername", "recipient", "recipientname", "sender", "sendername", "payeename", "from", "to",
    "sourcename", "targetname",
];
/// What a row that only states a balance or a total says.
const BALANCES: &[&str] = &[
    "anfangssaldo", "endsaldo", "startsaldo", "schlusssaldo", "kontostand", "openingbalance", "closingbalance",
    "startbalance", "endbalance", "saldoiniziale", "saldofinale", "soldeinitial", "soldefinal", "saldoinicial",
    "saldofinal", "beginsaldo", "eindsaldo", "likutispradziai", "likutispabaigai", "apyvarta", "saldopoczatkowe",
    "saldokoncowe", "openingsaldo", "afsluitingsaldo", "broughtforward", "carriedforward",
];
const MEMOS: &[&[&str]] = &[&[
    "memo", "notes", "note", "notesandtags", "transactionref2", "vrijemededeling", "mededeling", "商品说明", "商品", "メモ",
    "אסמכתה", "reference", "paymentreference", "verwendungszweck",
    "mededelingen", "omschrijving1", "communication", "communicatie", "referencia", "causale", "message", "tytul",
    "opis", "beschreibung2", "paaiskinimai", "rubrik", "vorgangverwendungszweck", "viesti",
]];
const AMOUNTS: &[&str] = &[
    "amount", "transactionamount", "euro", "euros", "eur", "suma", "gross", "betrag", "bedrag", "montant", "importe", "importo", "valor", "kwota",
    "belopp", "belob", "belop", "maara", "umsatz", "amounttotal", "cardcurrencyamount", "sourceamountafterfees", "суммаплатежа", "сумма",
    "сумаввалютікартки", "сума", "金额", "入出金", "זכותחובה", "סכוםחיוב", "المبلغ",
];
const OUTFLOWS: &[&str] = &[
    "outflow", "debit", "withdrawal", "withdrawals", "debits", "moneyout", "paidout", "out", "soll", "debet", "af",
    "debito", "cargo", "cargos", "addebiti", "uscite", "dare", "uttag", "utfrakonto", "debitamount", "lastschrift",
    "belastung", "obciazenia", "utavkonto", "withdrawalamount", "支払い金額", "出金金額", "お引出し", "出金", "חובה", "مدين",
    "المدين", "سحب", "مبلغمدين",
];
const INFLOWS: &[&str] = &[
    "inflow", "credit", "deposit", "deposits", "credits", "moneyin", "paidin", "in", "haben", "bij", "credito",
    "abono", "abonos", "accrediti", "entrate", "avere", "insattning", "innpakonto", "creditamount", "gutschrift",
    "uznania", "depositamount", "預かり金額", "入金金額", "お預入れ", "入金", "זכות", "دائن", "الدائن", "إيداع", "ايداع", "مبلغدائن",
];
const IDS: &[&str] = &["fitid", "transactionid", "id", "transferwiseid", "transaktionsnr", "irasonr", "交易订单号", "交易单号"];
/// What the bank charged on top, as Revolut writes it beside each amount.
const FEES: &[&str] = &["fee", "fees", "conversioncharge", "commission", "sourcefeeamount", "сумакомісій", "комиссия", "עמלה"];
/// Columns saying whether a row went through: its state, its kind, its effect on the balance.
const STATES: &[&str] = &[
    "state", "status", "transactionstate", "transactionstatus", "contabilizzazione", "type", "balanceimpact",
    "bewegungstyp", "info", "stato", "交易状态", "当前状态", "статус",
];
/// What such a column says of a row that didn't go through, or hasn't been booked yet.
const SKIPPED: &[&str] = &[
    "reverted", "declined", "failed", "cancelled", "canceled", "rejected", "void", "voided", "storniert",
    "abgelehnt", "pending", "authorised", "authorized", "vorgemerkt", "reserviert", "reserverat", "noncontabilizzato",
    "inbehandeling", "pendiente", "enattente", "vormerkung", "umsatzvorgemerkt", "venter", "temporaryhold",
    "authorisation", "authorization", "order", "memo", "geweigerd", "autorizzato", "held", "交易关闭", "等待确认收货", "等待付款",
];
/// Columns saying what kind of payment a row is.
const KINDS: &[&str] = &[
    "type", "transactiontype", "tapahtuma", "umsatzart", "vorgang", "rodzajtransakcji", "typtransakcji", "交易类型", "交易分类",
];
/// Columns that only name the other party, cleaner than a bank's description of the payment.
const NAMES: &[&str] = &[
    "supplierfound", "libellesuggere", "merchant", "merchantname", "payeename", "counterpartyname", "description2", "subdescription",
    "naamtegenpartij", "naamtegenpartijbevat", "naamvandetegenpartij", "otherpartyname", "nameofsenderreceiver", "destination",
    "交易对方", "摘要内容", "שםביתעסק",
];
/// A column that may name the account each row is in, in a file with several (RBC's, a
/// register's), or else the other party's account (N26's, DKB's).
const ACCOUNTS: &[&str] = &[
    "accountnumber", "accountno", "acctnumber", "account", "accountname", "kontonummer", "auftragskonto", "rekening",
    "rekeningnummer",
];
const CURRENCIES: &[&str] = &["currency", "waehrung", "wahrung", "devise", "divisa", "moneda", "munt", "waluta", "valiuta"];
/// A column saying which way an unsigned amount went ("Af"/"Bij", "D"/"C", "S"/"H", "IN"/"OUT").
const DIRECTIONS: &[&str] = &[
    "afbij", "debitcredit", "creditdebit", "debitcreditindicator", "creditdebitindicator", "dc", "cd",
    "sollhaben", "sh", "direction", "dk", "收支",
];
const OUT: &[&str] = &["af", "d", "db", "dr", "dbit", "debit", "debet", "s", "soll", "out", "支出"];
const IN: &[&str] = &["bij", "c", "cr", "crdt", "credit", "h", "haben", "in", "k", "收入"];
/// What a direction column says of a move that's neither, between the holder's own balances
/// (Alipay's 不计收支, WeChat's "/").
const NEUTRAL: &[&str] = &["不计收支", "中性交易"];

/// A header's name without case, accents, spaces or punctuation ("Trans. Date" → "transdate",
/// "Libellé" → "libelle"), and the currency it names in brackets at the end ("Amount (USD)").
fn header(s: &str) -> (String, Option<String>) {
    match s.trim().strip_suffix(')').and_then(|s| s.rsplit_once('(')) {
        Some((name, c)) if c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()) => (plain(name), Some(c.into())),
        _ => (plain(s), None),
    }
}
pub(crate) fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.to_lowercase().chars() {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ą' | 'ā' | 'ă' => out.push('a'),
            'æ' => out.push_str("ae"),
            'ç' | 'ć' | 'č' | 'ĉ' => out.push('c'),
            'ď' | 'đ' | 'ð' => out.push('d'),
            'è' | 'é' | 'ê' | 'ë' | 'ę' | 'ė' | 'ē' | 'ě' => out.push('e'),
            'ğ' | 'ģ' => out.push('g'),
            'ì' | 'í' | 'î' | 'ï' | 'į' | 'ī' | 'ı' => out.push('i'),
            'ķ' => out.push('k'),
            'ł' | 'ľ' | 'ĺ' | 'ļ' => out.push('l'),
            'ñ' | 'ń' | 'ň' | 'ņ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ő' | 'ō' => out.push('o'),
            'œ' => out.push_str("oe"),
            'ŕ' | 'ř' => out.push('r'),
            'ś' | 'š' | 'ş' | 'ș' => out.push('s'),
            'ß' => out.push_str("ss"),
            'ť' | 'ţ' | 'ț' => out.push('t'),
            'þ' => out.push_str("th"),
            'ù' | 'ú' | 'û' | 'ü' | 'ů' | 'ű' | 'ū' | 'ų' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            c if c.is_alphabetic() || c.is_ascii_digit() => out.push(c),
            _ => {}
        }
    }
    out
}
/// Whether a plain header is one of `names`, also when the budget's currency follows it
/// ("Betrag EUR", "Débit euros", "Umsatz in EUR").
fn named(name: &str, names: &[&str], currency: &str) -> bool {
    let code = currency.to_ascii_lowercase();
    names.contains(&name)
        || [code.as_str(), "euros", "euroa", "euro", "円", "元"].iter().any(|s| {
            name.strip_suffix(s).is_some_and(|n| names.contains(&n) || n.strip_suffix("in").is_some_and(|n| names.contains(&n)))
        })
}
/// A chosen column among a header row's: the same text, or failing that the same plain name.
pub(crate) fn position(headers: &[String], wanted: &str) -> Option<usize> {
    headers
        .iter()
        .position(|h| h.trim() == wanted.trim())
        // A column chosen by its place ("#4"), where its name can't tell it apart: blank, or
        // another column's too.
        .or_else(|| {
            let n: usize = wanted.trim().strip_prefix('#')?.trim_start_matches('#').parse().ok()?;
            (1..=headers.len()).contains(&n).then(|| n - 1)
        })
        .or_else(|| {
            let wanted = plain(wanted);
            headers.iter().position(|h| plain(h) == wanted)
        })
}

/// Where each part of a transaction is in a row.
#[derive(Debug, Default)]
struct Picked {
    date: Option<usize>,
    payee: Option<usize>,
    memo: Option<usize>,
    amount: Option<usize>,
    outflow: Option<usize>,
    inflow: Option<usize>,
    id: Option<usize>,
    transfer_account_id: Option<usize>,
    fee: Option<usize>,
    states: Vec<usize>,
    direction: Option<usize>,
    currencies: Vec<usize>,
    /// When the bank booked it, beside when it happened: empty until it's booked.
    posted: Option<usize>,
    /// Columns that only name the other party (a shop, a payee), read before the payee's text.
    names: Vec<usize>,
    /// What kind of payment it is, for a row that names no one.
    kind: Option<usize>,
    /// A column for each party to a payment, read for whichever isn't the account holder.
    parties: Option<(usize, usize)>,
    /// Columns that may say which account each row is in.
    accounts: Vec<usize>,
}
impl Picked {
    /// Found by their names.
    fn detect(headers: &[String], currency: &str) -> Self {
        let h: Vec<(String, Option<String>)> = headers.iter().map(|s| header(s)).collect();
        // The likeliest name the file has, whatever order its columns are in.
        let find = |lists: &[&[&str]]| {
            lists.iter().flat_map(|names| names.iter()).find_map(|name| h.iter().position(|(s, _)| named(s, &[name], currency)))
        };
        // A money column that names a currency is read only in the budget's.
        let money = |names: &[&str]| {
            names.iter().find_map(|name| {
                h.iter().position(|(s, code)| named(s, &[name], currency) && code.as_deref().is_none_or(|c| c == currency))
            })
        };
        // Columns that only name the other party are read row by row before it (`names`).
        let payee = PAYEES.iter().flat_map(|names| names.iter()).filter(|n| !NAMES.contains(n))
            .find_map(|name| h.iter().position(|(s, _)| named(s, &[name], currency)));
        let tier = |names: &[&str]| names.iter().find_map(|name| h.iter().position(|(s, _)| named(s, &[name], currency)));
        let parties = match h.iter().enumerate().filter(|(_, (s, _))| PARTIES.contains(&s.as_str())).map(|(i, _)| i).collect::<Vec<_>>()[..] {
            [a, b] => Some((a, b)),
            _ => None,
        };
        Self {
            date: find(DATES),
            posted: tier(DATES[0]).and(tier(DATES[1])),
            names: NAMES.iter()
                .filter_map(|n| h.iter().position(|(s, _)| s == n))
                .filter(|i| parties.is_none_or(|(a, b)| *i != a && *i != b))
                // RBC's Description 2 names the other party, after Description 1's kind of payment;
                // UBS's, before a Description 3, is the kind.
                .filter(|&i| h[i].0 != "description2" || !h.iter().any(|(s, _)| s == "description3"))
                .collect(),
            payee,
            memo: find(MEMOS).filter(|&m| Some(m) != payee),
            amount: money(AMOUNTS),
            outflow: money(OUTFLOWS),
            inflow: money(INFLOWS),
            id: find(&[IDS]),
            transfer_account_id: headers.iter().position(|h| h.trim() == "TransferAccountId"),
            fee: money(FEES),
            states: (0..h.len()).filter(|&i| STATES.contains(&h[i].0.as_str())).collect(),
            kind: find(&[KINDS]),
            direction: find(&[DIRECTIONS]),
            currencies: (0..h.len()).filter(|&i| named(&h[i].0, CURRENCIES, currency)).collect(),
            parties,
            accounts: (0..h.len()).filter(|&i| ACCOUNTS.contains(&h[i].0.as_str())).collect(),
        }
    }
    /// What the names got wrong, put right by the rows: a payee column that says the same on
    /// every row (an account's name) gives way to the next, a payee by content where no name
    /// gave one, and signs from a column of D/C or S/H letters whose header is blank.
    fn refine(&mut self, headers: &[String], records: &[csv::StringRecord], currency: &str) {
        let sample: Vec<&csv::StringRecord> = records.iter().filter(|r| r.len() >= headers.len()).take(50).collect();
        let values = |i: usize| sample.iter().filter_map(move |r| r.get(i)).filter(|c| !c.is_empty());
        let constant = |i: usize| values(i).count() >= 3 && values(i).all(|c| Some(c) == values(i).next());
        if self.payee.is_some_and(constant) {
            let h: Vec<(String, Option<String>)> = headers.iter().map(|s| header(s)).collect();
            self.payee = PAYEES.iter().flat_map(|names| names.iter()).filter(|n| !NAMES.contains(n)).find_map(|name| {
                (0..h.len()).find(|&i| named(&h[i].0, &[name], currency) && !constant(i) && Some(i) != self.memo)
            });
        }
        if self.payee.is_none() && self.parties.is_none() {
            self.payee = Self::by_content(headers, records, currency).payee.filter(|&i| Some(i) != self.memo);
        }
        let unsigned = self.amount.is_some_and(|i| values(i).all(|c| !c.starts_with(['-', '−', '('])));
        if self.direction.is_none() && unsigned {
            let picked = [self.date, self.payee, self.memo, self.amount, self.id];
            self.direction = (0..headers.len()).find(|&i| {
                !picked.contains(&Some(i)) && values(i).count() >= 2 && values(i).all(|c| {
                    let w = plain(c);
                    w.len() <= 6 && (OUT.contains(&w.as_str()) || IN.contains(&w.as_str()))
                })
            });
        }
    }
    /// As chosen, when the header row has every chosen column. Fees, states, currencies and
    /// directions are still found by name.
    fn chosen(headers: &[String], columns: &Columns, currency: &str) -> Option<Self> {
        let at = |n: &Option<String>| match n {
            Some(n) => position(headers, n).map(Some),
            None => Some(None),
        };
        Some(Self {
            date: Some(position(headers, columns.date.as_deref()?)?),
            payee: at(&columns.payee)?,
            memo: at(&columns.memo)?,
            amount: at(&columns.amount)?,
            outflow: at(&columns.outflow)?,
            inflow: at(&columns.inflow)?,
            id: at(&columns.id)?,
            transfer_account_id: at(&columns.transfer_account_id)?,
            ..Self::detect(headers, currency)
        })
    }
    fn complete(&self) -> bool {
        self.date.is_some() && (self.amount.is_some() || self.outflow.is_some() || self.inflow.is_some())
    }
    /// What the names don't find, found by what the first rows hold: the first column of dates,
    /// the one column of amounts (or the one of several with money both in and out, as a balance
    /// rarely has), and the longest text for the payee. Anything less sure is left to choose, and
    /// a column named for another currency is never read.
    fn by_content(headers: &[String], records: &[csv::StringRecord], currency: &str) -> Self {
        let named = Self::detect(headers, currency);
        let foreign = |i: usize| header(&headers[i]).1.is_some_and(|c| c != currency);
        // Rows shaped like the transactions: a note or a total under them is narrower.
        let sample: Vec<&csv::StringRecord> = records.iter().filter(|r| r.len() >= headers.len()).take(50).collect();
        let cells = |i: usize| sample.iter().filter_map(move |r| r.get(i)).filter(|c| !c.is_empty());
        // Four in five of a column's cells, or all but one (a damaged cell in a short file).
        let mostly = |i: usize, test: &dyn Fn(&str) -> bool| {
            let (all, yes) = cells(i).fold((0, 0), |(all, yes), c| (all + 1, yes + test(c) as usize));
            all > 0 && (yes * 5 >= all * 4 || yes >= 1 && yes + 1 == all)
        };
        let date = named.date.or_else(|| (0..headers.len()).find(|&i| mostly(i, &is_date)));
        let money: Vec<usize> = (0..headers.len())
            .filter(|&i| Some(i) != date && !foreign(i) && mostly(i, &|c| amount_like(c, currency)))
            .collect();
        let both_ways = |i: &usize| cells(*i).any(|c| c.starts_with(['-', '−', '('])) && cells(*i).any(|c| !c.starts_with(['-', '−', '(']));
        let amount = match money.as_slice() {
            [one] => Some(*one),
            _ => Some(money.iter().filter(|i| both_ways(i)).collect::<Vec<_>>()).filter(|m| m.len() == 1).map(|m| *m[0]),
        };
        // A running balance beside the amount, both signed (CommBank's file without a header): the
        // balance goes up and down by the amount, row after row, in whichever order they come.
        let value = |r: &csv::StringRecord, i: usize| r.get(i).filter(|c| !c.is_empty()).and_then(|c| crate::bank_import::money(c, currency).ok());
        let running = |a: usize, b: usize| {
            // A column that doesn't move (an exchange rate of 1.0) is no balance.
            let steps: Vec<bool> = sample.windows(2)
                .filter_map(|w| Some((value(w[0], a)?, value(w[0], b)?, value(w[1], a)?, value(w[1], b)?)))
                .filter(|(_, b0, _, b1)| b0 != b1)
                .map(|(a0, b0, a1, b1)| b1 - b0 == a1 || b0 - b1 == a0)
                .collect();
            steps.len() >= 2 && steps.iter().filter(|s| **s).count() * 5 >= steps.len() * 4
        };
        let amount = amount.or_else(|| money.iter().copied().find(|&a| money.iter().any(|&b| b != a && running(a, b))));
        // Two columns side by side with each row's money in one or the other: money out, then in,
        // as banks order them (TD's and CIBC's headerless files).
        let split = money.windows(2).map(|w| (w[0], w[1])).find(|&(a, b)| {
            b == a + 1 && sample.iter().all(|r| {
                let filled = |i: usize| r.get(i).is_some_and(|c| !c.is_empty());
                filled(a) != filled(b)
            })
        });
        let length = |i: usize| {
            let text: Vec<&str> = cells(i).filter(|c| c.chars().any(char::is_alphabetic)).collect();
            if text.is_empty() || text.len() * 2 < sample.len() { 0 } else { text.iter().map(|c| c.chars().count()).sum::<usize>() / text.len() }
        };
        let payee = named.payee.or_else(|| {
            (0..headers.len())
                .filter(|&i| Some(i) != date && !money.contains(&i))
                .map(|i| (length(i), std::cmp::Reverse(i)))
                .filter(|&(n, _)| n > 2)
                .max()
                .map(|(_, std::cmp::Reverse(i))| i)
        });
        if named.amount.is_some() || named.outflow.is_some() || named.inflow.is_some() {
            return Self { date, payee, ..named };
        }
        match (amount, split) {
            (None, Some((outflow, inflow))) => Self { date, payee, outflow: Some(outflow), inflow: Some(inflow), ..named },
            _ => Self { date, payee, amount, ..named },
        }
    }
}
fn is_date(cell: &str) -> bool {
    // A five-digit number reads as a spreadsheet's day number, but in a file of unnamed columns
    // it's more likely a code.
    let serial = cell.trim().len() == 5 && cell.trim().bytes().all(|b| b.is_ascii_digit());
    !serial && ["dmy", "mdy", "ymd"].iter().any(|f| crate::bank_import::date(cell, f).is_ok())
}
/// An amount as banks write them, with cents: a code or a count ("05", "00000000") isn't one.
fn amount_like(cell: &str, currency: &str) -> bool {
    cell.contains(['.', ',']) && money(cell, currency).is_ok()
}
/// A statement in columns lined up with spaces rather than separated, as some banks' text exports
/// are (BBVA México's): the header row's names mark where each column is, and each run of text in
/// a transaction's line goes to the name it lines up with. As tab-separated text, or None for a
/// file that isn't laid out this way.
fn aligned(content: &str) -> Option<(String, Vec<(usize, bool)>)> {
    // Runs of text between two or more spaces, with where they start and end (in characters).
    fn runs(line: &str) -> Vec<(usize, usize, String)> {
        let chars: Vec<char> = line.trim_end().chars().collect();
        let (mut out, mut i) = (vec![], 0);
        while i < chars.len() {
            if chars[i] == ' ' {
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && !(chars[i] == ' ' && chars.get(i + 1).is_none_or(|c| *c == ' ')) {
                i += 1;
            }
            out.push((start, i, chars[start..i].iter().collect()));
        }
        out
    }
    let lines: Vec<(usize, &str)> = all_lines(content);
    let dated = |line: &str| runs(line).first().is_some_and(|(_, _, first)| is_date(first));
    // Separators, or commas between fields rather than in amounts, make it a CSV.
    let separated = |line: &str| line.contains(['\t', ';', '|']) || line.matches(',').count() >= 2;
    // The column names: no date or number among them, over dated lines (or a line whose date is
    // damaged, then dated ones).
    let numeric = |t: &str| t.bytes().any(|b| b.is_ascii_digit()) && t.chars().all(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-' | '+'));
    let wide = |k: usize| lines.get(k).is_some_and(|l| runs(l.1).len() >= 3);
    let header = (0..lines.len().min(60)).find(|&h| {
        let data = |k: usize| wide(k) && dated(lines[k].1);
        wide(h) && !separated(lines[h].1) && !dated(lines[h].1) && !runs(lines[h].1).iter().any(|r| numeric(&r.2))
            && (data(h + 1) || wide(h + 1) && data(h + 2))
    })?;
    let names = runs(lines[header].1);
    let mut out = vec![names.iter().map(|n| n.2.as_str()).collect::<Vec<_>>().join("\t")];
    // Each line of the aligned text: where it starts in the file, and whether today's reader
    // read it (it read only dated lines; the others are kept for showing and fixing).
    let mut origin = vec![(lines[header].0, true)];
    for &(offset, line) in &lines[header + 1..] {
        origin.push((offset, dated(line)));
        let mut cells = vec![String::new(); names.len()];
        for (start, end, text) in runs(line) {
            // The name it overlaps most, as an amount lines up under its column's name; text that
            // runs on past its own name stays in the column it started in.
            let overlap = |&(s, e, _): &(usize, usize, String)| end.min(e) as i64 - start.max(s) as i64;
            let column = (0..names.len()).filter(|&j| overlap(&names[j]) > 0).max_by_key(|&j| overlap(&names[j]))
                .or_else(|| (0..names.len()).rev().find(|&j| names[j].0 <= start))
                .unwrap_or(0);
            if !cells[column].is_empty() {
                cells[column].push(' ');
            }
            cells[column].push_str(&text);
        }
        out.push(cells.join("\t"));
    }
    Some((out.join("\n"), origin))
}
/// Where the transactions start in a file whose header row doesn't name its columns, or that has
/// none: the first line with a date and an amount among those with the most common number of
/// fields, and the header row above it, if that line has as many fields and no dates.
fn data_start(lines: &[(usize, &str)], currency: &str) -> Option<(usize, u8, Option<usize>)> {
    SEPARATORS.into_iter().find_map(|d| {
        let rows: Vec<Vec<String>> = lines.iter().map(|(_, l)| fields(l, d)).collect();
        let mut counts: HashMap<usize, usize> = HashMap::new();
        for r in rows.iter().filter(|r| r.len() >= 3) {
            *counts.entry(r.len()).or_default() += 1;
        }
        let (&n, _) = counts.iter().max_by_key(|&(n, k)| (*k, *n))?;
        let data = |r: &[String]| r.len() == n && r.iter().any(|c| is_date(c)) && r.iter().any(|c| amount_like(c, currency));
        let mut first = rows.iter().position(|r| data(r))?;
        // A row as wide just above, with a date or an amount, is data with the other damaged, not
        // the header.
        let damaged = |r: &[String]| r.len() == n && r.iter().any(|c| is_date(c) || amount_like(c, currency));
        while first > 0 && damaged(&rows[first - 1]) {
            first -= 1;
        }
        let header = first.checked_sub(1).filter(|&h| rows[h].len() == n && !rows[h].iter().any(|c| is_date(c)));
        Some((lines[first].0, d, header.map(|h| lines[h].0)))
    })
}

/// A CSV's header row, the records under it, and the columns read.
struct Sheet {
    headers: Vec<String>,
    records: Vec<csv::StringRecord>,
    /// Where each record starts in the file as written: its line's id.
    offsets: Vec<usize>,
    /// Records today's reader never saw: lines of a statement in aligned columns without a date
    /// (it kept only dated lines). Shown and fixable, never part of what it reads.
    hidden: Vec<bool>,
    /// Where the header row starts, or the first record where there's none.
    start: usize,
    picked: Picked,
    /// The columns are the ones chosen.
    saved: bool,
    separator: u8,
}
impl Sheet {
    /// Finds the header row among the first lines, since some banks write an account summary
    /// above it: the first with every chosen column, or else the first whose names give a date
    /// and an amount. Without one, the columns are found by what's in them, under the header row
    /// if there is one ("Column 1", "Column 2"… if not, as some banks export). Failing that, the
    /// widest line, so its columns can be chosen by hand. `header_at` is a header row someone
    /// chose (it doesn't apply to a statement in aligned columns, whose header is its layout).
    fn read(content: &str, currency: &str, chosen: Option<&Columns>, header_at: Option<usize>) -> Result<Self, String> {
        let content = content.trim_start_matches('\u{feff}');
        match aligned(content) {
            Some((text, origin)) => Self::find(&text, currency, chosen, None, Some(&origin)),
            None => Self::find(content, currency, chosen, header_at, None),
        }
    }
    fn find(content: &str, currency: &str, chosen: Option<&Columns>, header_at: Option<usize>, origin: Option<&[(usize, bool)]>) -> Result<Self, String> {
        // A header row someone chose: its names say what they can, what's under them the rest.
        if let Some(at) = header_at.filter(|&at| at < content.len() && content.is_char_boundary(at)) {
            let line = content[at..].lines().next().unwrap_or("");
            let separator = separators(line).fold((b',', 0), |best, d| {
                let n = fields(line, d).len();
                if n > best.1 { (d, n) } else { best }
            }).0;
            let mut sheet = Self::parse(content, at, separator, false, Picked::default(), false, origin)?;
            match chosen.and_then(|c| Picked::chosen(&sheet.headers, c, currency)) {
                Some(picked) => (sheet.picked, sheet.saved) = (picked, true),
                None => {
                    let visible = sheet.visible();
                    let named = Picked::detect(&sheet.headers, currency);
                    sheet.picked = if named.complete() {
                        let mut named = named;
                        named.refine(&sheet.headers, &visible, currency);
                        named
                    } else {
                        Picked::by_content(&sheet.headers, &visible, currency)
                    };
                }
            }
            return Ok(sheet);
        }
        let lines = first_lines(content);
        let locate = |pick: &dyn Fn(&[String]) -> Option<Picked>| {
            lines.iter().find_map(|&(at, line)| {
                separators(line).find_map(|d| pick(&fields(line, d)).map(|p| (at, d, p)))
            })
        };
        if let Some((at, separator, picked)) = chosen.and_then(|c| locate(&|h| Picked::chosen(h, c, currency))) {
            return Self::parse(content, at, separator, false, picked, true, origin);
        }
        if let Some((at, separator, picked)) = locate(&|h| Some(Picked::detect(h, currency)).filter(Picked::complete)) {
            let mut sheet = Self::parse(content, at, separator, false, picked, false, origin)?;
            let mut picked = std::mem::take(&mut sheet.picked);
            picked.refine(&sheet.headers, &sheet.visible(), currency);
            sheet.picked = picked;
            return Ok(sheet);
        }
        if let Some((first, separator, header)) = data_start(&lines, currency) {
            let mut sheet = Self::parse(content, header.unwrap_or(first), separator, header.is_none(), Picked::default(), false, origin)?;
            match chosen.and_then(|c| Picked::chosen(&sheet.headers, c, currency)) {
                Some(picked) => (sheet.picked, sheet.saved) = (picked, true),
                None => {
                    let visible = sheet.visible();
                    sheet.picked = Picked::by_content(&sheet.headers, &visible, currency);
                }
            }
            return Ok(sheet);
        }
        let (at, separator) = widest(&lines);
        let line = lines.iter().find(|l| l.0 == at).map_or("", |l| l.1);
        Self::parse(content, at, separator, false, Picked::detect(&fields(line, separator), currency), false, origin)
    }
    /// The records from `at` on: the first is the header row, unless `numbered`. `origin` maps a
    /// statement in aligned columns back to the file's lines (see `aligned`).
    fn parse(content: &str, at: usize, separator: u8, numbered: bool, picked: Picked, saved: bool, origin: Option<&[(usize, bool)]>) -> Result<Self, String> {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(separator)
            .trim(csv::Trim::All)
            .flexible(true)
            .has_headers(false)
            .from_reader(content[at..].as_bytes());
        // Where a record starts in the file as written, and whether today's reader saw it.
        let first_line = content[..at].matches('\n').count();
        let place = |position: Option<&csv::Position>| -> (usize, bool) {
            let position = position.cloned().unwrap_or_else(csv::Position::new);
            match origin {
                Some(origin) => origin.get(first_line + position.line().max(1) as usize - 1).copied().unwrap_or((at, true)),
                None => (at + position.byte() as usize, true),
            }
        };
        let start = match origin {
            Some(origin) => origin.get(first_line).map_or(at, |o| o.0),
            None => at,
        };
        let (mut headers, mut records, mut offsets, mut hidden, mut first) = (vec![], vec![], vec![], vec![], true);
        for record in reader.records() {
            let record = record.map_err(|e| e.to_string())?;
            if first && !numbered {
                first = false;
                headers = record.iter().map(String::from).collect();
                continue;
            }
            first = false;
            if record.iter().all(|f| f.is_empty()) {
                continue;
            }
            let (offset, seen) = place(record.position());
            records.push(record);
            offsets.push(offset);
            hidden.push(!seen);
        }
        if first && !numbered {
            return Err(t!("Empty file"));
        }
        let headers = if numbered {
            // As wide as most rows: a total under them can run wider.
            let mut counts: HashMap<usize, usize> = HashMap::new();
            for (r, h) in records.iter().zip(&hidden) {
                if !h {
                    *counts.entry(r.len()).or_default() += 1;
                }
            }
            let width = counts.into_iter().max_by_key(|&(n, k)| (k, n)).map_or(0, |(n, _)| n);
            (1..=width).map(|i| format!("Column {i}")).collect()
        } else {
            headers
        };
        Ok(Self { headers, records, offsets, hidden, start, picked, saved, separator })
    }
    /// The records today's reader saw, which say what the columns hold.
    fn visible(&self) -> std::borrow::Cow<'_, [csv::StringRecord]> {
        if self.hidden.iter().any(|h| *h) {
            self.records.iter().zip(&self.hidden).filter(|(_, h)| !**h).map(|(r, _)| r.clone()).collect::<Vec<_>>().into()
        } else {
            self.records.as_slice().into()
        }
    }
    /// A row with more fields than the header row, whose amounts don't read, has an unquoted
    /// separator in its text: the fields it split go back together in the payee's column, if the
    /// amounts read then.
    fn mend(&mut self, currency: &str, until: usize) {
        let (Some(text), width) = (self.picked.payee, self.headers.len()) else { return };
        let p = &self.picked;
        let money_cells = [p.amount, p.outflow, p.inflow];
        let reads = |r: &csv::StringRecord| {
            money_cells.iter().flatten().all(|&i| r.get(i).is_none_or(|c| c.is_empty() || money(c, currency).is_ok()))
        };
        let separator = if self.separator == b'\t' { " ".to_string() } else { format!("{} ", self.separator as char) };
        for r in self.records.iter_mut().take(until).filter(|r| r.len() > width && width > text) {
            if reads(r) {
                continue;
            }
            let extra = r.len() - width;
            let fields: Vec<&str> = r.iter().collect();
            let joined = fields[text..=text + extra].join(&separator);
            let mut fixed: Vec<&str> = fields[..text].to_vec();
            fixed.push(&joined);
            fixed.extend(&fields[text + extra + 1..]);
            let fixed = csv::StringRecord::from(fixed);
            if reads(&fixed) {
                *r = fixed;
            }
        }
    }
    /// Another account's header row, with columns of its own (comdirect's card and depot sections
    /// under its current account's): not this header, but naming its date's and amount's columns.
    fn other_header(&self, r: &csv::StringRecord) -> bool {
        let p = &self.picked;
        let name = |i: Option<usize>| i.and_then(|i| self.headers.get(i)).map(|h| plain(h)).filter(|h| !h.is_empty());
        let Some(date) = name(p.date) else { return false };
        let cells: Vec<String> = r.iter().map(plain).collect();
        let same = r.len() == self.headers.len() && r.iter().zip(&self.headers).all(|(c, h)| plain(c) == plain(h));
        !same && cells.contains(&date) && [p.amount, p.outflow, p.inflow].into_iter().filter_map(name).any(|a| cells.contains(&a))
    }
    fn columns(&self) -> Columns {
        // A column by its name, or by its place where its name doesn't say which (see `position`).
        let name = |i: Option<usize>| {
            let i = i?;
            let h = self.headers.get(i)?;
            let unique = !h.trim().is_empty() && self.headers.iter().filter(|o| o.trim() == h.trim()).count() == 1;
            if unique {
                return Some(h.clone());
            }
            // Written so no column's own name reads as it ("##4" where a column is named "#4").
            let mut place = format!("#{}", i + 1);
            while self.headers.iter().any(|o| o.trim() == place) {
                place.insert(0, '#');
            }
            Some(place)
        };
        let p = &self.picked;
        Columns {
            date: name(p.date),
            payee: name(p.payee),
            memo: name(p.memo),
            amount: name(p.amount),
            outflow: name(p.outflow),
            inflow: name(p.inflow),
            id: name(p.id),
            transfer_account_id: name(p.transfer_account_id),
        }
    }
}
/// The first lines with anything on them, and where each starts.
/// Lines end in CR LF, LF or, from old Mac software, a lone CR.
fn first_lines(content: &str) -> Vec<(usize, &str)> {
    let mut out = vec![];
    let mut start = 0;
    let bytes = content.as_bytes();
    let mut i = 0;
    while i <= bytes.len() && out.len() < 60 {
        if i == bytes.len() || bytes[i] == b'\n' || bytes[i] == b'\r' {
            let line = &content[start..i];
            if !line.trim().is_empty() {
                out.push((start, line));
            }
            i += if bytes.get(i) == Some(&b'\r') && bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
            start = i;
        } else {
            i += 1;
        }
    }
    out
}
/// The separators a line could use, tabs first: a semicolon file's amounts have commas in them.
const SEPARATORS: [u8; 4] = [b'\t', b';', b'|', b','];
fn separators(line: &str) -> impl Iterator<Item = u8> + '_ {
    let any = SEPARATORS.into_iter().any(|d| line.contains(d as char));
    SEPARATORS.into_iter().filter(move |&d| !any && d == b',' || line.contains(d as char))
}
fn fields(line: &str, separator: u8) -> Vec<String> {
    csv::ReaderBuilder::new()
        .delimiter(separator)
        .trim(csv::Trim::All)
        .flexible(true)
        .has_headers(false)
        .from_reader(line.as_bytes())
        .records()
        .next()
        .and_then(Result::ok)
        .map(|r| r.iter().map(String::from).collect())
        .unwrap_or_default()
}
/// The separator that splits some line into the most fields, and the first line it splits that
/// much.
fn widest(lines: &[(usize, &str)]) -> (usize, u8) {
    let mut best = (0, b',', 0);
    for d in SEPARATORS {
        for &(at, line) in lines {
            let n = fields(line, d).len();
            if n > best.2 {
                best = (at, d, n);
            }
        }
    }
    (best.0, best.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank_import::{apply, preview, preview_mapped, Choice};
    use crate::domain::tests::*;

    fn eur() -> Budget {
        let mut b = sample();
        b.currency = "EUR".into();
        b
    }

    type Shown = Vec<(String, Option<(String, i64, String)>)>;
    fn status(s: &Status) -> String {
        match s {
            Status::Ready => "ready".into(),
            Status::Attention { field, .. } => format!("attention {field:?}"),
            Status::LeftOut { reason, .. } => format!("{reason:?}"),
        }
    }
    fn shown(b: &Budget, csv: &str, o: &Options, order: &str) -> Shown {
        let read = read_lines(b, "cash", csv, o, order, false).unwrap();
        read.lines.iter().map(|l| (status(&l.status), l.transaction.as_ref().map(|t| (t.date.clone(), t.amount, t.payee.clone())))).collect()
    }
    fn fixture(file: &str) -> String {
        read_statement(&std::fs::read(format!("{}/../../shared/fixtures/bank-files/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap().text
    }

    #[test]
    fn every_line_says_what_became_of_it() {
        let b = eur();
        let csv = "Account statement 4711\n\
                   Date,Payee,Amount,Currency,Status\n\
                   2026-10-01,Bakery,-3.20,EUR,Booked\n\
                   2026-10-02,Cinema,-12.00,EUR,Pending\n\
                   2026-10-03,Shop,-5.00,EUR,Declined\n\
                   2026-10-04,Closing balance,100.00,EUR,\n\
                   Total,,-20.20,,\n\
                   2026-10-05,Card check,0.00,EUR,Booked\n\
                   2026-10-06,Note,,EUR,Booked\n\
                   2026-10-07,London shop,-9.99,GBP,Booked\n\
                   2026-10-08,Grocer,n/a,EUR,Booked\n\
                   2026-10-09,Lunch,-8.50,EUR,Booked\n";
        let read = read_lines(&b, "cash", csv, &Options::default(), "ymd", false).unwrap();
        let got: Vec<(String, bool)> = read.lines.iter().map(|l| (status(&l.status), l.reserves)).collect();
        let want = [
            ("AboveHeader", false), ("ready", true), ("Pending", false), ("Declined", false), ("Balance", false), ("NoDate", false),
            ("Zero", false), ("NoAmount", false), ("OtherCurrency", false), ("attention Amount", true), ("ready", true),
        ];
        assert_eq!(got, want.map(|(s, r)| (s.to_string(), r)));
        // The amount that doesn't read says why, in its column.
        assert_eq!(read.lines[9].status, Status::Attention { field: Field::Amount, column: Some(2), reason: "Invalid amount: n/a".into() });
        // Every line keeps where it starts in the file, and its cells.
        assert_eq!(read.lines[0].offset, 0);
        assert_eq!(&csv[read.lines[1].offset..read.lines[1].offset + 10], "2026-10-01");
        assert_eq!(read.lines[9].cells, ["2026-10-08", "Grocer", "n/a", "EUR", "Booked"]);
        // Today's import stops at that amount; the other lines read as it reads them.
        assert_eq!(preview(&b, "cash", csv, "csv", "ymd", false).err().unwrap(), "Invalid amount: n/a");
        let clean = csv.replace("2026-10-08,Grocer,n/a,EUR,Booked\n", "");
        let today: Vec<(String, i64, String)> = preview(&b, "cash", &clean, "csv", "ymd", false).unwrap().into_iter().map(|r| (r.date, r.amount, r.payee)).collect();
        let now: Vec<(String, i64, String)> = read.lines.iter().filter_map(|l| l.transaction.as_ref()).map(|t| (t.date.clone(), t.amount, t.payee.clone())).collect();
        assert_eq!(now, today);
    }

    #[test]
    fn a_corrupt_cell_changes_only_its_own_line() {
        let b = eur();
        let clean = fixture("de-dkb-2023.csv");
        let o = Options::default();
        let before = shown(&b, &clean, &o, "dmy");
        // Each line with a date and an amount, broken in turn: that line needs attention, and
        // every other line reads exactly as it did.
        let ready: Vec<usize> = (0..before.len()).filter(|&i| before[i].0 == "ready").collect();
        assert!(ready.len() >= 6);
        let read = read_lines(&b, "cash", &clean, &o, "dmy", false).unwrap();
        for &i in &ready {
            let line = &read.lines[i];
            let amount = read.records.columns.amount.as_ref().map(|n| position(&read.records.headers, n).unwrap()).unwrap();
            let (from, to) = (line.offset, line.offset + clean[line.offset..].find(['\r', '\n']).unwrap_or(clean.len() - line.offset));
            let original = &clean[from..to];
            let broken_cell = &line.cells[amount];
            for (what, broken) in [("amount", original.replacen(broken_cell.as_str(), "x1", 1)), ("date", original.replacen(&line.cells[read.records.date_column], "3x.10.2026", 1))] {
                let text = format!("{}{}{}", &clean[..from], broken, &clean[to..]);
                let after = shown(&b, &text, &o, "dmy");
                assert_eq!(after.len(), before.len(), "{what} {i}");
                for k in 0..after.len() {
                    if k == i {
                        assert!(after[k].0.starts_with("attention"), "{what} {i}: {:?}", after[k]);
                    } else {
                        assert_eq!(after[k], before[k], "{what} {i} broke line {k}");
                    }
                }
            }
        }
    }

    #[test]
    fn dates_that_no_longer_settle_their_order_ask_instead_of_changing() {
        let b = eur();
        let csv = "Date;Payee;Amount\n31/10/2026;Rent;-500,00\n01/02/2026;Shop;-3,00\n";
        let read = read_lines(&b, "cash", csv, &Options::default(), "mdy", false).unwrap();
        assert_eq!((read.dates.order, read.dates.unconfirmed), ("dmy", false));
        assert_eq!(read.lines[1].transaction.as_ref().unwrap().date, "2026-02-01");
        // The only date that settled it broken: nothing re-dated silently, the order needs confirming.
        let broken = csv.replace("31/10/2026", "3l/10/2026");
        let read = read_lines(&b, "cash", &broken, &Options::default(), "mdy", false).unwrap();
        assert!(read.dates.unconfirmed);
        assert_eq!(read.dates.example.as_deref(), Some("01/02/2026"));
        // Confirmed day first, it reads as before.
        let read = read_lines(&b, "cash", &broken, &Options::default(), "dmy", true).unwrap();
        assert!(!read.dates.unconfirmed);
        assert_eq!(read.lines[1].transaction.as_ref().unwrap().date, "2026-02-01");
        // A fixed date doesn't count toward the order: only the file's own dates do.
        let evidence: Vec<_> = records(&b, "cash", &broken, &Options::default()).unwrap().records.iter().map(|r| r.evidence.clone()).collect();
        assert_eq!(evidence, [Some("3l/10/2026".to_string()), Some("01/02/2026".to_string())]);
    }

    #[test]
    fn a_file_with_several_accounts_reads_the_one_chosen() {
        let mut b = eur();
        b.currency = "CAD".into();
        let rbc = fixture("ca-rbc.csv");
        let read = read_lines(&b, "cash", &rbc, &Options::default(), "mdy", false).unwrap();
        let sources: Vec<&str> = read.records.sources.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(sources, ["01234-5000123", "4510000000001234"]);
        let ready = |read: &CsvLines| read.lines.iter().filter(|l| l.status == Status::Ready).count();
        assert_eq!(ready(&read), 5);
        let visa = Options { source: Some("4510000000001234"), ..Options::default() };
        let read = read_lines(&b, "cash", &rbc, &visa, "mdy", false).unwrap();
        assert_eq!(ready(&read), 2);
        assert_eq!(read.lines.iter().filter(|l| matches!(l.status, Status::LeftOut { reason: Reason::OtherAccount, .. })).count(), 5);
        // comdirect writes each account's rows under a heading of their own.
        let comdirect = fixture("de-comdirect.csv");
        let read = read_lines(&eur(), "cash", &comdirect, &Options::default(), "dmy", false).unwrap();
        assert_eq!(read.records.sources.len(), 2);
        let second = Options { source: Some(&read.records.sources[1].0), ..Options::default() };
        let other = read_lines(&eur(), "cash", &comdirect, &second, "dmy", false).unwrap();
        assert!(ready(&other) > 0);
        assert_eq!(ready(&read), 6);
    }

    #[test]
    fn included_lines_read_and_need_what_they_still_lack() {
        let b = eur();
        let csv = "Date,Payee,Amount,Currency,Status\n\
                   2026-10-01,Bakery,-3.20,EUR,Booked\n\
                   2026-10-02,Cinema,-12.00,EUR,Pending\n\
                   2026-10-03,London shop,-9.99,GBP,Booked\n\
                   ,Pending card,-4.00,EUR,Booked\n\
                   2026-10-05,Card check,0.00,EUR,Booked\n";
        let read = read_lines(&b, "cash", csv, &Options::default(), "ymd", false).unwrap();
        let include: HashSet<usize> = read.lines.iter().map(|l| l.offset).collect();
        let all = read_lines(&b, "cash", csv, &Options { include, ..Options::default() }, "ymd", false).unwrap();
        let got: Vec<(String, bool)> = all.lines.iter().map(|l| (status(&l.status), l.reserves)).collect();
        // Pending reads; another currency needs the amount in the budget's; no date needs one;
        // nothing moving can't be included. Included lines hold no place among repeated ids.
        let want = [("ready", true), ("ready", false), ("attention Amount", false), ("attention Date", false), ("Zero", false)];
        assert_eq!(got, want.map(|(s, r)| (s.to_string(), r)));
        assert_eq!(all.lines[1].transaction.as_ref().map(|t| t.amount), Some(-1200));
    }

    #[test]
    fn a_fixed_cell_changes_only_its_line_and_keeps_its_place() {
        let b = eur();
        let csv = "Date,Payee,Amount\n2026-10-01,Bakery,-3.20\n2026-10-02,Grocer,n/a\n2026-10-03,Lunch,-8.50\n";
        let read = read_lines(&b, "cash", csv, &Options::default(), "ymd", false).unwrap();
        let grocer = read.lines[1].offset;
        let fixed = Options { cells: HashMap::from([(grocer, vec![(2, "-12.40".to_string())])]), ..Options::default() };
        let after = read_lines(&b, "cash", csv, &fixed, "ymd", false).unwrap();
        assert_eq!(after.lines[1].status, Status::Ready);
        assert_eq!(after.lines[1].cells[2], "-12.40");
        assert!(after.lines[1].reserves);
        assert_eq!(after.lines[1].transaction.as_ref().map(|t| t.amount), Some(-1240));
        for k in [0, 2] {
            assert_eq!(after.lines[k].transaction.as_ref().map(|t| (t.date.clone(), t.amount)), read.lines[k].transaction.as_ref().map(|t| (t.date.clone(), t.amount)));
        }
    }

    #[test]
    fn an_aligned_statement_shows_the_lines_todays_reader_left_out() {
        let mut b = eur();
        b.currency = "MXN".into();
        let text = fixture("mx-bbva.txt");
        // A date the bank's text export mangled: today's reader drops the line without a word.
        let broken = text.replacen("29-09-2026", "29-O9-2026", 1);
        assert_eq!(preview(&b, "cash", &broken, "csv", "dmy", false).unwrap().len(), 6);
        let read = read_lines(&b, "cash", &broken, &Options::default(), "dmy", false).unwrap();
        let line = read.lines.iter().find(|l| l.cells.first().is_some_and(|c| c == "29-O9-2026")).unwrap();
        assert!(matches!(line.status, Status::Attention { field: Field::Date, .. }));
        assert!(!line.reserves);
        assert_eq!(&broken[line.offset..line.offset + 10], "29-O9-2026");
        assert_eq!(read.lines.iter().filter(|l| l.status == Status::Ready).count(), 6);
    }

    #[test]
    fn decodes_statements_in_the_encodings_banks_write() {
        let cases: [(&encoding_rs::Encoding, &str); 8] = [
            (encoding_rs::WINDOWS_1252, "Buchungstag;Empfänger;Verwendungszweck;Betrag\n14.10.2026;Bäckerei Müller;Brötchen für Café;-3,20\n15.10.2026;Größte Straße;Überweisung;-12,00"),
            (encoding_rs::WINDOWS_1250, "Data operacji;Opis operacji;Tytuł;Kwota\n14.10.2026;ŻABKA Z1234;Zakup przy użyciu karty, Łódź;-12,50\n15.10.2026;Przelew wychodzący;Opłata za mieszkanie;-1200,00"),
            (encoding_rs::SHIFT_JIS, "日付,摘要,摘要内容,支払い金額,預かり金額\n2026/9/25,振込１,カ）サンプルシヨウジ,,285000\n2026/9/28,口座振替３,サンプルデンキ,8640,"),
            (encoding_rs::GBK, "交易时间,交易分类,交易对方,商品说明,收/支,金额\n2026-10-21 18:42:10,餐饮美食,示例餐厅,晚餐套餐,支出,38.50\n2026-10-18 09:15:33,转账红包,李四,转账,收入,200.00"),
            (encoding_rs::WINDOWS_1251, "Дата операции;Статус;Сумма платежа;Категория;Описание\n21.10.2026;OK;-1534,90;Супермаркеты;Супермаркет Пример\n15.10.2026;OK;85000,00;Пополнения;Зарплата ООО Пример"),
            (encoding_rs::WINDOWS_1255, "תאריך,יום ערך,תיאור התנועה,זכות/חובה,יתרה\n22/10/2026,22/10/2026,משיכת מזומן כספומט,-400.00,20293.95\n14/10/2026,14/10/2026,העברה מאת ישראל ישראלי,250.00,20700.85"),
            (encoding_rs::WINDOWS_1256, "التاريخ,الوصف,المبلغ,الرصيد\n2026/10/21,شراء من سوبرماركت المثال,-153.90,4520.10\n2026/10/15,راتب شهر أكتوبر,8500.00,4674.00"),
            (encoding_rs::UTF_8, "Date,Payee,Amount\n2026-10-14,Café,-3.20"),
        ];
        for (encoding, text) in cases {
            let (bytes, _, lossy) = encoding.encode(text);
            assert!(!lossy, "{}", encoding.name());
            let read = decode(&bytes);
            assert_eq!((read.encoding, read.text.as_str()), (encoding.name(), text));
            // As the apps send it: a character for each byte.
            let latin1: String = bytes.iter().map(|&b| b as char).collect();
            assert_eq!(decode_latin1(&latin1, None).unwrap(), read);
        }
        // £ signs in an otherwise English file aren't Polish Ł.
        let pounds = encoding_rs::WINDOWS_1252.encode("Date,Description,Amount\n14/10/2026,TESCO STORES,£12.50\n15/10/2026,SALARY,£2,150.00").0;
        assert_eq!(decode(&pounds).text, "Date,Description,Amount\n14/10/2026,TESCO STORES,£12.50\n15/10/2026,SALARY,£2,150.00");
        // A byte-order mark says which.
        assert_eq!(decode(b"\xFF\xFED\x00\xE4\x00").text, "Dä");
        assert_eq!(decode(b"\xEF\xBB\xBFB\xC3\xA4").text, "Bä");
        assert!(decode_latin1("not bytes: ł", None).is_err());
        // A workbook's other sheet, by its name.
        let book = std::fs::read(format!("{}/../../shared/fixtures/bank-files/es-bbva.xlsx", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let found = read_statement(&book).unwrap();
        assert_eq!((found.sheet.as_deref(), found.sheets.as_slice()), (Some("Movimientos"), &["Resumen".to_string(), "Movimientos".to_string()][..]));
        let other = read_sheet(&book, Some("Resumen")).unwrap();
        assert_eq!((other.sheet.as_deref(), other.sheets.len()), (Some("Resumen"), 2));
        assert_ne!(other.text, found.text);
        assert_eq!(read_sheet(&book, Some("Gone")).unwrap(), found);
        // A PDF is the assistant's to read; a ZIP or an Excel file that isn't one says so.
        assert_eq!(read_statement(b"%PDF-1.7\n1 0 obj").err().as_deref(), Some(pdf()));
        assert!(read_statement(b"PK\x03\x04 not really a zip").is_err());
        assert!(read_statement(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0]).is_err());
    }

    #[test]
    fn reads_a_file_without_a_header_whose_total_runs_wider() {
        let b = eur();
        let text = "25/09/2026;PIX TRANSF BELTRANO;850,00\n29/09/2026;PADARIA EXEMPLO;-23,50\n01/10/2026;CIA ENERGIA;-187,34\n\
                    03/10/2026;APL APLIC AUT;-500,00\n06/10/2026;PIX TRANSF FULANO;-120,00\nTotal;;;19,16\n";
        let read = rows(&b, text, "dmy");
        assert_eq!(read.iter().map(|r| r.3).collect::<Vec<_>>(), [85000, -2350, -18734, -50000, -12000]);
    }

    #[test]
    fn reads_a_statement_laid_out_in_aligned_columns() {
        let b = eur();
        let line = |d: &str, text: &str, out: &str, inn: &str, left: &str| format!("{d:<12}{text:<40}{out:>10}{inn:>12}{left:>12}");
        let text = [
            "Movimientos de la cuenta".to_string(),
            String::new(),
            line("Día", "Concepto", "Cargo", "Abono", "Saldo"),
            // Text with two spaces in it runs on past its column's name, and stays in that column.
            line("25-09-2026", "DEPOSITO DE NOMINA  ACME CORP SA DE CV", "", "18,500.00", "30,845.67"),
            line("29-09-2026", "PAGO SERVICIO LUZ", "486.00", "", "30,359.67"),
            "Saldo final                                                              30,359.67".to_string(),
        ]
        .join("\r\n");
        let found = layout(&b, "cash", &text, None).unwrap();
        assert_eq!(found.headers, ["Día", "Concepto", "Cargo", "Abono", "Saldo"]);
        let read = rows(&b, &text, "dmy");
        assert_eq!(
            read.iter().map(|r| (r.0.as_str(), r.1.as_str(), r.3)).collect::<Vec<_>>(),
            [("2026-09-25", "DEPOSITO DE NOMINA ACME CORP SA DE CV", 1_850_000), ("2026-09-29", "PAGO SERVICIO LUZ", -48_600)]
        );
    }
    fn rows(b: &Budget, csv: &str, order: &str) -> Vec<(String, String, String, i64)> {
        preview(b, "cash", csv, "csv", order, false)
            .unwrap()
            .into_iter()
            .map(|r| (r.date, r.payee, r.memo, r.amount))
            .collect()
    }
    fn row(date: &str, payee: &str, memo: &str, amount: i64) -> (String, String, String, i64) {
        (date.into(), payee.into(), memo.into(), amount)
    }

    #[test]
    fn reads_revolut_skipping_what_never_went_through_or_is_pending_and_taking_off_fees() {
        let csv = "Type,Product,Started Date,Completed Date,Description,Amount,Fee,Currency,State,Balance
TOPUP,Current,2026-10-01 09:12:44,2026-10-01 09:12:50,Top-Up by *1234,200.00,0.00,EUR,COMPLETED,200.00
CARD_PAYMENT,Current,2026-10-02 18:03:10,2026-10-03 07:00:01,Café Central,-12.50,0.25,EUR,COMPLETED,187.25
CARD_PAYMENT,Current,2026-10-04 11:00:00,,Bakery,-3.20,0.00,EUR,PENDING,184.05
CARD_PAYMENT,Current,2026-10-05 12:00:00,,Hotel,-90.00,0.00,EUR,DECLINED,184.05
TRANSFER,Current,2026-10-06 08:00:00,2026-10-06 08:00:01,To Joint,-50.00,0.00,EUR,REVERTED,184.05";
        // The pending one comes in once it's booked.
        assert_eq!(
            rows(&eur(), csv, "ymd"),
            [row("2026-10-01", "Top-Up by *1234", "", 20000), row("2026-10-02", "Café Central", "", -1275)]
        );
        // A pocket in another currency isn't read as this budget's.
        let usd = csv.replace(",EUR,", ",USD,");
        let wrong = preview(&eur(), "cash", &usd, "csv", "ymd", false).err().unwrap();
        assert!(wrong.contains("USD") && wrong.contains("EUR"), "{wrong}");
    }

    #[test]
    fn finds_the_header_under_an_account_summary_in_european_files() {
        // A savings bank's export: the account above the header, dates with dots, decimal commas.
        let dkb = "\u{feff}\"Kontonummer:\";\"DE12 3456 7890 1234 5678 90\";
\"Von:\";\"01.10.2026\";

\"Buchungstag\";\"Wertstellung\";\"Buchungstext\";\"Auftraggeber / Begünstigter\";\"Verwendungszweck\";\"Betrag (EUR)\";
\"02.10.2026\";\"02.10.2026\";\"Lastschrift\";\"Stadtwerke\";\"Strom Oktober\";\"-64,20\";
\"03.10.26\";\"04.10.26\";\"Gutschrift\";\"Arbeitgeber GmbH\";\"Gehalt\";\"2.450,00\";
";
        assert_eq!(
            rows(&eur(), dkb, "ymd"),
            [
                row("2026-10-02", "Stadtwerke", "Strom Oktober", -6420),
                row("2026-10-03", "Arbeitgeber GmbH", "Gehalt", 245000),
            ]
        );
        // A Dutch bank writes every amount without a sign, and which way it went beside it.
        let ing = "\"Datum\",\"Naam / Omschrijving\",\"Rekening\",\"Tegenrekening\",\"Code\",\"Af Bij\",\"Bedrag (EUR)\",\"Mutatiesoort\",\"Mededelingen\"
\"20261002\",\"Albert Heijn\",\"NL01INGB\",\"\",\"BA\",\"Af\",\"23,45\",\"Betaalautomaat\",\"Pasvolgnr 001\"
\"20261003\",\"Werkgever BV\",\"NL01INGB\",\"NL02RABO\",\"GT\",\"Bij\",\"1500,00\",\"Online bankieren\",\"Salaris\"";
        assert_eq!(
            rows(&eur(), ing, "ymd"),
            [
                row("2026-10-02", "Albert Heijn", "Pasvolgnr 001", -2345),
                row("2026-10-03", "Werkgever BV", "Salaris", 150000),
            ]
        );
        // Spanish names, debit written with a minus in its own column, a total under the rows.
        let es = "Fecha Operación;Fecha Valor;Concepto;Cargo;Abono;Saldo
15/10/2026;15/10/2026;Mercadona;-41,90;;1.000,00
16/10/2026;16/10/2026;Bizum de Ana;;20,00;1.020,00
;;Saldo final;;;1.020,00";
        assert_eq!(
            rows(&eur(), es, "dmy"),
            [row("2026-10-15", "Mercadona", "", -4190), row("2026-10-16", "Bizum de Ana", "", 2000)]
        );
    }

    #[test]
    fn reads_dates_with_times_dots_and_month_names_and_whichever_order_fits() {
        let b = eur();
        let one = |csv: &str, order: &str| rows(&b, csv, order)[0].0.clone();
        assert_eq!(one("Date,Payee,Amount\n2026-10-15T14:03:22Z,Shop,-1", "ymd"), "2026-10-15");
        assert_eq!(one("Date,Payee,Amount\n15.10.2026 14:03,Shop,-1", "dmy"), "2026-10-15");
        assert_eq!(one("Date,Payee,Amount\n15 Oct 2026,Shop,-1", "dmy"), "2026-10-15");
        assert_eq!(one("Date,Payee,Amount\n15-OCT-26 10:00,Shop,-1", "dmy"), "2026-10-15");
        assert_eq!(one("Date,Payee,Amount\n\"Oct 15, 2026\",Shop,-1", "mdy"), "2026-10-15");
        // Day first, whatever was chosen, when a day past the twelfth says so.
        let csv = "Date,Payee,Amount\n03/10/2026,Shop,-1\n15/10/2026,Shop,-1";
        assert_eq!(rows(&b, csv, "mdy")[0].0, "2026-10-03");
        assert_eq!(rows(&b, csv, "ymd")[0].0, "2026-10-03");
        // Either way round fits every date here, so the choice decides.
        let both = "Date,Payee,Amount\n03/10/2026,Shop,-1";
        assert_eq!(rows(&b, both, "mdy")[0].0, "2026-03-10");
        assert!(preview(&b, "cash", both, "csv", "ymd", false).is_err());
        assert_eq!(layout(&b, "cash", csv, None).unwrap().date_format.as_deref(), Some("dmy"));
        assert_eq!(layout(&b, "cash", both, None).unwrap().date_format, None);
    }

    #[test]
    fn reads_the_columns_chosen_and_the_account_keeps_them_for_its_next_file() {
        let mut b = eur();
        let csv = "Wann;Wer;Wieviel;Notiz\n15.10.2026;Bäckerei;-3,20;Brötchen";
        // Names it doesn't know: the columns are found by what's in them.
        let found = layout(&b, "cash", csv, None).unwrap();
        assert_eq!(found.headers, ["Wann", "Wer", "Wieviel", "Notiz"]);
        assert_eq!(found.samples, [["15.10.2026", "Bäckerei", "-3,20", "Brötchen"]]);
        let by_content = Columns { date: Some("Wann".into()), payee: Some("Wer".into()), amount: Some("Wieviel".into()), ..Columns::default() };
        assert_eq!((found.columns.clone(), found.saved, found.date_format.as_deref()), (by_content, false, Some("dmy")));
        // Two money columns with each row's in one or the other: money out, then money in, as TD
        // and CIBC write them without a header row.
        let split = "Wann;Wer;Aus;Ein\n15.10.2026;Bäckerei;3,20;\n16.10.2026;Lohn;;2.000,00";
        let amounts: Vec<i64> = preview(&b, "cash", split, "csv", "dmy", false).unwrap().iter().map(|r| r.amount).collect();
        assert_eq!(amounts, [-320, 200000]);
        // Money in both on a row: which is which can't be told, so they're left to choose.
        let unsure = "Wann;Wer;Aus;Ein\n15.10.2026;Bäckerei;3,20;1,00\n16.10.2026;Lohn;;2.000,00";
        assert_eq!(
            preview(&b, "cash", unsure, "csv", "dmy", false).err().unwrap(),
            "CSV needs Amount or Outflow/Inflow columns."
        );
        let columns = Columns {
            date: Some("Wann".into()),
            payee: Some("Wer".into()),
            amount: Some("Wieviel".into()),
            memo: Some("Notiz".into()),
            ..Columns::default()
        };
        // Once a date column is chosen, its dates say which order they're in.
        assert_eq!(layout(&b, "cash", csv, Some(&columns)).unwrap().date_format.as_deref(), Some("dmy"));
        let read = preview_mapped(&b, "cash", csv, "csv", "dmy", false, Some(&columns)).unwrap();
        assert_eq!((read[0].date.as_str(), read[0].payee.as_str(), read[0].amount), ("2026-10-15", "Bäckerei", -320));
        let wrong = Columns { amount: Some("Betrag".into()), ..columns.clone() };
        let error = preview_mapped(&b, "cash", csv, "csv", "dmy", false, Some(&wrong)).err().unwrap();
        assert!(error.contains("‘Betrag’"), "{error}");

        let rows = preview_mapped(&b, "cash", csv, "csv", "dmy", false, Some(&columns)).unwrap();
        b = apply(b, "cash", rows.into_iter().map(|row| Choice { row, action: "new".into(), match_id: None }).collect()).unwrap();
        remember(&mut b, "cash", FileSettings { columns: Some(columns.clone()), date_format: Some("dmy".into()), flip_signs: Some(false), ..FileSettings::default() }).unwrap();
        // The next file from the bank, with its columns in another order and an extra one.
        let next = "Notiz;Wieviel;Wann;Wer;Saldo\nMiete;-800,00;01.11.2026;Vermieter;100,00";
        let read = preview(&b, "cash", next, "csv", "dmy", false).unwrap();
        assert_eq!((read[0].payee.as_str(), read[0].memo.as_str(), read[0].amount), ("Vermieter", "Miete", -80000));
        let found = layout(&b, "cash", next, None).unwrap();
        assert_eq!((found.columns, found.saved, found.date_format.as_deref()), (columns.clone(), true, Some("dmy")));
        // A file without those columns is read by its names, and an empty choice forgets them.
        assert_eq!(rows_of(&b, "Date,Payee,Amount\n2026-11-02,Shop,-1"), 1);
        // What can't be kept stays as it was, and never stops an import.
        remember(&mut b, "cash", FileSettings { date_format: Some("ydm".into()), ..FileSettings::default() }).unwrap();
        let blank = Columns { payee: Some(" ".into()), ..columns.clone() };
        remember(&mut b, "cash", FileSettings { columns: Some(blank), ..FileSettings::default() }).unwrap();
        assert_eq!(settings(&b, "cash").columns, Some(columns.clone()));
        remember(&mut b, "cash", FileSettings { columns: Some(Columns::default()), ..FileSettings::default() }).unwrap();
        let kept = FileSettings { columns: None, date_format: Some("dmy".into()), flip_signs: Some(false), ..FileSettings::default() };
        assert_eq!(settings(&b, "cash"), kept);
    }
    fn rows_of(b: &Budget, csv: &str) -> usize {
        preview(b, "cash", csv, "csv", "ymd", false).unwrap().len()
    }

    #[test]
    fn reads_late_headers_pipes_split_dutch_columns_and_leaves_out_totals() {
        let b = eur();
        let one = row("2026-10-01", "Shop", "", -100);
        let late = format!("{}Date,Payee,Amount\n2026-10-01,Shop,-1.00\n", "Account summary line\n".repeat(45));
        assert_eq!(rows(&b, &late, "ymd"), [one.clone()]);
        assert_eq!(rows(&b, "Date|Payee|Amount\n2026-10-01|Shop|-1.00", "ymd"), [one.clone()]);
        assert_eq!(rows(&b, "Date,Payee,Amount\n2026-10-01,Shop,-1.00\nTotal,,-1.00\nSumme,,", "ymd"), [one]);
        assert_eq!(
            rows(&b, "Datum;Naam;Af;Bij\n01-10-2026;Shop;1,50;\n02-10-2026;Loon;;2.000,00", "dmy"),
            [row("2026-10-01", "Shop", "", -150), row("2026-10-02", "Loon", "", 200000)]
        );
        assert_eq!(rows(&b, "Date,Payee,Amount\n2026-10-01,Shop,12.34 DR\n2026-10-02,Refund,12.34 CR", "ymd")[0].3, -1234);
        // A description with the separator in it, unquoted, goes back together.
        assert_eq!(
            rows(&b, "Data;Valuta;Descrizione;Euro;Caus.\n05/10/2026;05/10/2026;BONIFICO PER QUOTA; GRAZIE;85.00;048", "dmy"),
            [row("2026-10-05", "BONIFICO PER QUOTA; GRAZIE", "", 8500)]
        );
        // No header row at all: an account line, then the transactions, as BNP Paribas exports them.
        let bnp = "Compte de chèques;M DUPONT;****1234;16/10/2026;;2 134.35\n15/10/2026;CB;FACTURE CARTE;DU 141026 BOULANGERIE EXEMPLE;-4.60\n05/10/2026;SCT;VIR SEPA RECU;/DE M JEAN DUPONT /MOTIF LOYER;85.00";
        assert_eq!(
            rows(&b, bnp, "dmy"),
            [row("2026-10-15", "DU 141026 BOULANGERIE EXEMPLE", "", -460), row("2026-10-05", "/DE M JEAN DUPONT /MOTIF LOYER", "", 8500)]
        );
        assert_eq!(layout(&b, "cash", bnp, None).unwrap().headers, ["Column 1", "Column 2", "Column 3", "Column 4", "Column 5"]);
        // A statement in another format, saved as .txt and chosen as CSV, says what it is.
        let sta = ":20:STARTUMS\n:25:12345678/0012345678\n:60F:C261001EUR0,00\n:61:2610011001DR1,00NMSCNONREF\n:86:Fee\n";
        assert_eq!(layout(&b, "cash", sta, None).unwrap().format, "mt940");
        assert_eq!(layout(&b, "cash", bnp, None).unwrap().format, "csv");
    }

    #[test]
    fn names_ignore_case_accents_punctuation_and_a_currency_after_them() {
        assert_eq!(plain("Libellé de l'opération"), "libelledeloperation");
        assert_eq!(plain("#Data operacji"), "dataoperacji");
        assert_eq!(header("Betrag (EUR)"), ("betrag".into(), Some("EUR".into())));
        assert_eq!(header("Betrag (€)"), ("betrag".into(), None));
        assert!(named("debiteuros", OUTFLOWS, "EUR") && named("betrageur", AMOUNTS, "EUR"));
        assert!(!named("datedevaleur", AMOUNTS, "EUR"));
        let csv = "Date;Libellé;Débit euros;Crédit euros\n15/10/2026;Boulangerie;3,20;";
        assert_eq!(rows(&eur(), csv, "dmy"), [row("2026-10-15", "Boulangerie", "", -320)]);
    }
}
