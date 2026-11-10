//! Files the user attaches. They are kept in the budget's folder. Images and PDFs go to the model
//! with the message; spreadsheets and CSV files are read here, a page at a time, so only the rows
//! the model asks for leave the computer, and a column mapping converts every row exactly.
use crate::t;
use super::{new_id, tools::truncate};
use base64::Engine;
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Pdf,
    Sheet,
    Csv,
    Text,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub kind: Kind,
}
impl Meta {
    pub fn is_table(&self) -> bool {
        matches!(self.kind, Kind::Sheet | Kind::Csv)
    }
}

/// Recognizes a file by its name. Anything else is refused.
pub fn kind_of(name: &str) -> Option<(Kind, &'static str)> {
    let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    Some(match ext.as_str() {
        "png" => (Kind::Image, "image/png"),
        "jpg" | "jpeg" => (Kind::Image, "image/jpeg"),
        "gif" => (Kind::Image, "image/gif"),
        "webp" => (Kind::Image, "image/webp"),
        "pdf" => (Kind::Pdf, "application/pdf"),
        "xlsx" | "xlsm" => (Kind::Sheet, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "xls" => (Kind::Sheet, "application/vnd.ms-excel"),
        "xlsb" => (Kind::Sheet, "application/vnd.ms-excel.sheet.binary.macroEnabled.12"),
        "ods" => (Kind::Sheet, "application/vnd.oasis.opendocument.spreadsheet"),
        "csv" | "tsv" => (Kind::Csv, "text/csv"),
        "txt" | "ofx" | "qfx" | "qif" | "json" | "md" => (Kind::Text, "text/plain"),
        _ => return None,
    })
}
fn limit(kind: Kind) -> u64 {
    match kind {
        Kind::Image => 10 << 20,
        Kind::Pdf => 25 << 20,
        Kind::Sheet | Kind::Csv => 20 << 20,
        Kind::Text => 2 << 20,
    }
}

/// A photo without what its camera wrote into it: where and when it was taken, the device, an
/// embedded preview. A JPEG keeps which way up it is; a PNG loses its text and Exif chunks. A
/// file that doesn't read as either goes as it is, and so do WebP and GIF.
fn scrub<'a>(mime: &str, bytes: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
    let cleaned = match mime {
        "image/jpeg" => scrub_jpeg(bytes),
        "image/png" => scrub_png(bytes),
        _ => None,
    };
    cleaned.map_or(std::borrow::Cow::Borrowed(bytes), std::borrow::Cow::Owned)
}

/// Exif's orientation (1 to 8) from an APP1 segment's contents, when it has one.
fn exif_orientation(exif: &[u8]) -> Option<u16> {
    let tiff = exif.strip_prefix(b"Exif\0\0")?;
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let short = |at: usize| tiff.get(at..at + 2).map(|b| if little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) });
    let long = |at: usize| tiff.get(at..at + 4).map(|b| if little { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) });
    let first = long(4)? as usize;
    for entry in 0..short(first)? as usize {
        let at = first.checked_add(2 + entry * 12)?;
        if short(at)? == 0x0112 {
            return short(at + 8).filter(|o| (1..=8).contains(o));
        }
    }
    None
}

fn scrub_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut kept: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut orientation = None;
    let mut removed = false;
    let mut at = 2;
    loop {
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        // Any number of fill bytes may come before a marker.
        let mut marker_at = at + 1;
        while *bytes.get(marker_at)? == 0xFF {
            marker_at += 1;
        }
        let marker = bytes[marker_at];
        // The picture itself starts here and runs to the end.
        if marker == 0xDA {
            kept.extend_from_slice(&bytes[at..]);
            break;
        }
        // Markers that stand alone.
        if marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            kept.extend_from_slice(&bytes[at..=marker_at]);
            at = marker_at + 1;
            continue;
        }
        let length = u16::from_be_bytes([*bytes.get(marker_at + 1)?, *bytes.get(marker_at + 2)?]) as usize;
        let end = marker_at.checked_add(1 + length)?;
        if length < 2 || end > bytes.len() {
            return None;
        }
        // APP1 is Exif or XMP; APP13 is Photoshop's and IPTC's.
        if marker == 0xE1 || marker == 0xED {
            removed = true;
            if marker == 0xE1 && orientation.is_none() {
                orientation = exif_orientation(&bytes[marker_at + 3..end]);
            }
        } else {
            kept.extend_from_slice(&bytes[at..end]);
        }
        at = end;
    }
    if !removed {
        return None;
    }
    let mut out = Vec::with_capacity(kept.len() + 40);
    out.extend_from_slice(&[0xFF, 0xD8]);
    // Which way up the photo is, and nothing else: a receipt on its side still reads.
    if let Some(o) = orientation.filter(|o| *o != 1) {
        out.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x22]);
        out.extend_from_slice(b"Exif\0\0II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0");
        out.extend_from_slice(&o.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    }
    out.extend_from_slice(&kept);
    Some(out)
}

fn scrub_png(bytes: &[u8]) -> Option<Vec<u8>> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIGNATURE) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(SIGNATURE);
    let mut removed = false;
    let mut at = SIGNATURE.len();
    while at < bytes.len() {
        let head = bytes.get(at..at + 8)?;
        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        // Length, type, data and its check.
        let end = at.checked_add(12)?.checked_add(length)?;
        if end > bytes.len() {
            return None;
        }
        let kind = &head[4..8];
        if matches!(kind, b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME") {
            removed = true;
        } else {
            out.extend_from_slice(&bytes[at..end]);
        }
        at = end;
        if kind == b"IEND" {
            break;
        }
    }
    removed.then_some(out)
}

/// The attachments of one budget, in `<budget folder>/assistant/files`.
pub struct Files {
    dir: Option<PathBuf>,
    memory: HashMap<String, (Meta, Vec<u8>)>,
}
impl Files {
    pub fn empty() -> Self {
        Self { dir: None, memory: HashMap::new() }
    }
    pub fn open(dir: PathBuf) -> Self {
        Self { dir: Some(dir), memory: HashMap::new() }
    }
    #[cfg(test)]
    pub fn insert(&mut self, name: &str, bytes: Vec<u8>) -> Meta {
        let (kind, mime) = kind_of(name).unwrap();
        let meta = Meta { id: new_id("file"), name: name.into(), mime: mime.into(), size: bytes.len() as u64, kind };
        self.memory.insert(meta.id.clone(), (meta.clone(), bytes));
        meta
    }
    fn valid_id(id: &str) -> bool {
        !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    }
    /// Where a file waits between being chosen and being sent: beside the kept files, so a file
    /// that's never sent is never among a conversation's.
    fn waiting(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.with_file_name("staged"))
    }
    /// Keeps a file the user chose until its message is sent (`adopt`) or it's taken off again
    /// (`discard`). A photo is kept without where and when it was taken.
    pub fn stage(&self, name: &str, bytes: &[u8]) -> Result<Meta, String> {
        let (kind, mime) = kind_of(name).ok_or_else(|| t!("Attach an image, a PDF, a spreadsheet (xlsx, xls, ods), a CSV or a text file."))?;
        if bytes.is_empty() {
            return Err(t!("{name} is empty.", name));
        }
        if bytes.len() as u64 > limit(kind) {
            return Err(t!("{name} is larger than {size} MB.", name, size = limit(kind) >> 20));
        }
        let bytes = if kind == Kind::Image { scrub(mime, bytes) } else { std::borrow::Cow::Borrowed(bytes) };
        let dir = self.waiting().ok_or_else(|| t!("No folder for attachments."))?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let meta = Meta {
            id: new_id("file"),
            name: truncate(name.rsplit(['/', '\\']).next().unwrap_or(name), 120),
            mime: mime.into(),
            size: bytes.len() as u64,
            kind,
        };
        std::fs::write(dir.join(format!("{}.bin", meta.id)), &bytes).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(format!("{}.json", meta.id)), serde_json::to_vec(&meta).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        Ok(meta)
    }
    /// Moves a waiting file among the kept ones, when its message is sent. One already kept (a
    /// message sent again after it failed) stays where it is.
    pub fn adopt(&self, id: &str) -> Result<(), String> {
        let (Some(dir), Some(waiting), true) = (self.dir.as_ref(), self.waiting(), Self::valid_id(id)) else { return Ok(()) };
        if !waiting.join(format!("{id}.json")).is_file() {
            return Ok(());
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        // The file itself first: its description is what says it's there.
        for extension in ["bin", "json"] {
            std::fs::rename(waiting.join(format!("{id}.{extension}")), dir.join(format!("{id}.{extension}"))).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    /// Forgets a waiting file the user took off their message. A file already sent stays.
    pub fn discard(&self, id: &str) {
        if let (Some(waiting), true) = (self.waiting(), Self::valid_id(id)) {
            for extension in ["bin", "json"] {
                let _ = std::fs::remove_file(waiting.join(format!("{id}.{extension}")));
            }
        }
    }
    /// Forgets every waiting file: what an earlier session chose and never sent.
    pub fn discard_waiting(&self) {
        if let Some(waiting) = self.waiting() {
            let _ = std::fs::remove_dir_all(waiting);
        }
    }
    /// The folder a file is in: among the kept files, or waiting to be sent.
    fn holding(&self, id: &str) -> Option<PathBuf> {
        if !Self::valid_id(id) {
            return None;
        }
        [self.dir.clone(), self.waiting()].into_iter().flatten().find(|d| d.join(format!("{id}.json")).is_file())
    }
    pub fn get(&self, id: &str) -> Result<Meta, String> {
        if let Some((meta, _)) = self.memory.get(id) {
            return Ok(meta.clone());
        }
        let missing = || t!("No attachment has the id {id}.", id);
        let dir = self.holding(id).ok_or_else(missing)?;
        let raw = std::fs::read(dir.join(format!("{id}.json"))).map_err(|_| missing())?;
        serde_json::from_slice(&raw).map_err(|_| missing())
    }
    pub fn bytes(&self, id: &str) -> Result<Vec<u8>, String> {
        if let Some((_, bytes)) = self.memory.get(id) {
            return Ok(bytes.clone());
        }
        let missing = || t!("The file for attachment {id} is gone.", id);
        let dir = self.holding(id).ok_or_else(missing)?;
        std::fs::read(dir.join(format!("{id}.bin"))).map_err(|_| missing())
    }
    pub fn base64(&self, id: &str) -> Result<String, String> {
        Ok(base64::engine::general_purpose::STANDARD.encode(self.bytes(id)?))
    }
}

pub struct TableRow {
    /// The row number as the file shows it, from 1.
    pub number: usize,
    pub cells: Vec<String>,
}
pub struct Table {
    pub sheets: Vec<String>,
    pub sheet: String,
    pub width: usize,
    pub rows: Vec<TableRow>,
}

fn cell_text(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_owned(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(t) if t.time() == chrono::NaiveTime::MIN => t.date().format("%Y-%m-%d").to_string(),
            Some(t) => t.format("%Y-%m-%d %H:%M").to_string(),
            None => dt.as_f64().to_string(),
        },
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(_) => "#ERROR".into(),
    }
}

fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        // Bank exports are often Windows-1252.
        Err(_) => bytes.iter().map(|&b| if b < 0x80 { b as char } else { char::from_u32(b as u32).unwrap_or('?') }).collect(),
    }
}

/// Reads a spreadsheet's sheet (the first by default) or a CSV file.
pub fn read_table(files: &Files, id: &str, sheet: Option<&str>) -> Result<Table, String> {
    let meta = files.get(id)?;
    let bytes = files.bytes(id)?;
    let mut rows = vec![];
    let mut width = 0;
    let (sheets, chosen) = match meta.kind {
        Kind::Sheet => {
            let mut book = open_workbook_auto_from_rs(std::io::Cursor::new(bytes))
                .map_err(|e| format!("{} couldn't be opened as a spreadsheet: {e}", meta.name))?;
            let names = book.sheet_names();
            let chosen = match sheet {
                Some(s) => names
                    .iter()
                    .find(|n| n.eq_ignore_ascii_case(s))
                    .cloned()
                    .ok_or_else(|| format!("{} has no sheet named {s}. Sheets: {}", meta.name, names.join(", ")))?,
                None => names.first().cloned().ok_or("The spreadsheet has no sheets.")?,
            };
            let range = book.worksheet_range(&chosen).map_err(|e| e.to_string())?;
            let (top, left) = range.start().map_or((0, 0), |(r, c)| (r as usize, c as usize));
            for (i, row) in range.rows().enumerate() {
                let mut cells: Vec<String> = vec![String::new(); left];
                cells.extend(row.iter().map(cell_text));
                while cells.last().is_some_and(String::is_empty) {
                    cells.pop();
                }
                if !cells.is_empty() {
                    width = width.max(cells.len());
                    rows.push(TableRow { number: top + i + 1, cells });
                }
            }
            (names, chosen)
        }
        Kind::Csv => {
            let text = decode(&bytes);
            let first = text.lines().next().unwrap_or_default();
            let delimiter = if first.contains('\t') {
                b'\t'
            } else if first.matches(';').count() > first.matches(',').count() {
                b';'
            } else {
                b','
            };
            let mut reader = csv::ReaderBuilder::new()
                .delimiter(delimiter)
                .has_headers(false)
                .flexible(true)
                .from_reader(text.as_bytes());
            for (i, record) in reader.records().enumerate() {
                let record = record.map_err(|e| format!("Row {} of {}: {e}", i + 1, meta.name))?;
                let mut cells: Vec<String> = record.iter().map(|c| c.trim().to_owned()).collect();
                while cells.last().is_some_and(String::is_empty) {
                    cells.pop();
                }
                if !cells.is_empty() {
                    width = width.max(cells.len());
                    rows.push(TableRow { number: i + 1, cells });
                }
            }
            (vec![meta.name.clone()], meta.name.clone())
        }
        _ => return Err(format!("{} isn't a spreadsheet or CSV file.", meta.name)),
    };
    Ok(Table { sheets, sheet: chosen, width, rows })
}

pub fn column_letters(width: usize) -> Vec<String> {
    (0..width).map(letter).collect()
}
fn letter(mut i: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (i % 26) as u8) as char);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s
}
/// "A" is 0, "AA" is 26. Also accepts a 1-based number.
pub fn column_index(column: &str) -> Option<usize> {
    let c = column.trim();
    if let Ok(n) = c.parse::<usize>() {
        return n.checked_sub(1);
    }
    if c.is_empty() || !c.chars().all(|ch| ch.is_ascii_alphabetic()) || c.len() > 3 {
        return None;
    }
    let mut n = 0usize;
    for ch in c.to_ascii_uppercase().bytes() {
        n = n * 26 + (ch - b'A' + 1) as usize;
    }
    Some(n - 1)
}

/// A preview the model gets with the message: the sheet names and the first rows.
pub fn preview(files: &Files, meta: &Meta) -> String {
    let base = format!("Attachment {} \u{201c}{}\u{201d}", meta.id, meta.name);
    match meta.kind {
        Kind::Sheet | Kind::Csv => match read_table(files, &meta.id, None) {
            Ok(t) => {
                let mut s = format!(
                    "{base}: {}{} with rows up to {}. The first rows of \u{201c}{}\u{201d} (columns A, B, …):",
                    if meta.kind == Kind::Sheet { "spreadsheet" } else { "CSV file" },
                    if t.sheets.len() > 1 { format!(" with sheets {}", t.sheets.join(", ")) } else { String::new() },
                    t.rows.last().map_or(0, |r| r.number),
                    t.sheet,
                );
                for r in t.rows.iter().take(15) {
                    s.push_str(&format!("\nRow {}: {}", r.number, r.cells.iter().map(|c| truncate(c, 40)).collect::<Vec<_>>().join(" | ")));
                }
                s.push_str("\nUse read_attachment to read more rows.");
                s
            }
            Err(e) => format!("{base}: couldn't be read ({e})."),
        },
        Kind::Text => match files.bytes(&meta.id) {
            Ok(b) => format!("{base}:\n{}", truncate(&decode(&b), 50_000)),
            Err(e) => format!("{base}: {e}"),
        },
        Kind::Image | Kind::Pdf => format!("{base} is included below."),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    pub date: String,
    pub payee: String,
    pub memo: String,
    pub amount: i64,
    /// An explicit other account in this budget, never a payee label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer_account_id: Option<String>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Expected {
    pub count: Option<usize>,
    pub outflow: Option<i64>,
    pub inflow: Option<i64>,
}

/// Which columns hold what, as the model read them off the sheet.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mapping {
    pub sheet: Option<String>,
    pub header_row: usize,
    pub last_row: Option<usize>,
    pub date: String,
    pub payee: String,
    pub memo: Option<String>,
    pub amount: Option<String>,
    pub outflow: Option<String>,
    pub inflow: Option<String>,
    pub transfer_account_id: Option<String>,
    pub date_order: String,
    #[serde(default)]
    pub negate: bool,
    #[serde(default)]
    pub skip_rows: Vec<usize>,
}

/// Converts every data row with the mapping. Rows that can't be read are listed, not guessed.
pub fn convert(files: &Files, id: &str, m: &Mapping) -> Result<(Vec<ImportRow>, Vec<String>), String> {
    let table = read_table(files, id, m.sheet.as_deref())?;
    let col = |name: &str, c: &str| column_index(c).ok_or_else(|| format!("{name} column \u{201c}{c}\u{201d} isn't a column letter."));
    let date = col("The date", &m.date)?;
    let payee = col("The payee", &m.payee)?;
    let memo = m.memo.as_deref().filter(|c| !c.is_empty()).map(|c| col("The memo", c)).transpose()?;
    let amount = m.amount.as_deref().filter(|c| !c.is_empty()).map(|c| col("The amount", c)).transpose()?;
    let outflow = m.outflow.as_deref().filter(|c| !c.is_empty()).map(|c| col("The outflow", c)).transpose()?;
    let inflow = m.inflow.as_deref().filter(|c| !c.is_empty()).map(|c| col("The inflow", c)).transpose()?;
    let transfer = m.transfer_account_id.as_deref().filter(|c| !c.is_empty()).map(|c| col("The transfer account ID", c)).transpose()?;
    if amount.is_none() && outflow.is_none() && inflow.is_none() {
        return Err("Give an amount column, or outflow and inflow columns.".into());
    }
    if !["ymd", "dmy", "mdy"].contains(&m.date_order.as_str()) {
        return Err("dateOrder must be ymd, dmy or mdy.".into());
    }
    let mut rows = vec![];
    let mut skipped = vec![];
    for r in table.rows.iter().filter(|r| r.number > m.header_row && m.last_row.is_none_or(|l| r.number <= l)) {
        if m.skip_rows.contains(&r.number) {
            continue;
        }
        let get = |i: usize| r.cells.get(i).map(String::as_str).unwrap_or_default();
        let value = |i: Option<usize>| -> Result<i64, ()> {
            match i {
                None => Ok(0),
                Some(i) if get(i).is_empty() => Ok(0),
                Some(i) => parse_amount(get(i)).ok_or(()),
            }
        };
        let money = match amount {
            Some(_) => value(amount).map(|v| if m.negate { -v } else { v }),
            None => value(inflow).and_then(|i| value(outflow).map(|o| i.abs() - o.abs())),
        };
        let parsed_date = parse_date(get(date), &m.date_order);
        match (parsed_date, money) {
            (Some(d), Ok(a)) => {
                if a == 0 && get(payee).is_empty() {
                    continue;
                }
                rows.push(ImportRow {
                    date: d,
                    payee: truncate(get(payee), 200),
                    memo: memo.map(|i| truncate(get(i), 500)).unwrap_or_default(),
                    amount: a,
                    transfer_account_id: transfer.map(|i| get(i).trim()).filter(|id| !id.is_empty()).map(str::to_owned),
                });
            }
            (None, _) if get(date).is_empty() => {
                // A note or a blank line between transactions.
                if money.is_ok_and(|a| a != 0) {
                    skipped.push(t!("row {row} has an amount but no date", row = r.number));
                }
            }
            (None, _) => skipped.push(t!("row {row}: \u{201c}{text}\u{201d} isn't a date", row = r.number, text = truncate(get(date), 20))),
            (_, Err(())) => skipped.push(t!("row {row}: the amount isn't a number", row = r.number)),
        }
    }
    Ok((rows, skipped))
}

/// Reads amounts the way banks write them: 1,234.56 · 1.234,56 · (12.00) · 12.00- · 12.00 DR · SAR 5.
pub fn parse_amount(text: &str) -> Option<i64> {
    let mut s = text.trim().to_uppercase();
    if s.is_empty() {
        return None;
    }
    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = s[1..s.len() - 1].to_string();
    }
    let trimmed = s.trim_end();
    if let Some(rest) = trimmed.strip_suffix("DR") {
        negative = !negative;
        s = rest.to_string();
    } else if let Some(rest) = trimmed.strip_suffix("CR") {
        s = rest.to_string();
    }
    let s = s.trim();
    if s.ends_with('-') {
        negative = !negative;
    }
    if s.trim_start_matches(|c: char| !c.is_ascii_digit() && c != '-').starts_with('-') {
        negative = !negative;
    }
    let digits: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
    if !digits.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let (dot, comma) = (digits.rfind('.'), digits.rfind(','));
    let decimal = match (dot, comma) {
        (Some(d), Some(c)) => Some(d.max(c)),
        (Some(d), None) => (digits.matches('.').count() == 1).then_some(d),
        (None, Some(c)) => (digits.matches(',').count() == 1 && digits.len() - c - 1 <= 2).then_some(c),
        (None, None) => None,
    };
    let (whole, fraction) = match decimal {
        Some(i) => (&digits[..i], &digits[i + 1..]),
        None => (digits.as_str(), ""),
    };
    let whole: String = whole.chars().filter(char::is_ascii_digit).collect();
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let mut thousandths: String = fraction.chars().take(3).collect();
    while thousandths.len() < 3 {
        thousandths.push('0');
    }
    let cents = (thousandths.parse::<i64>().ok()? + 5) / 10;
    let value = whole.checked_mul(100)?.checked_add(cents)?;
    Some(if negative { -value } else { value })
}

/// Reads a date: ISO always; otherwise in the order given, with numbers or month names.
pub fn parse_date(text: &str, order: &str) -> Option<String> {
    use chrono::NaiveDate;
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let head = t.split([' ', 'T']).next().unwrap_or(t);
    if let Ok(d) = NaiveDate::parse_from_str(head, "%Y-%m-%d") {
        return Some(d.to_string());
    }
    for f in ["%d %b %Y", "%d-%b-%Y", "%d/%b/%Y", "%d %B %Y", "%b %d, %Y", "%B %d, %Y", "%b %d %Y", "%d-%b-%y", "%d %b %y"] {
        if let Ok(d) = NaiveDate::parse_from_str(t, f) {
            return Some(d.to_string());
        }
    }
    let parts: Vec<&str> = head.split(['/', '.', '-']).filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 || !parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let n: Vec<i32> = parts.iter().map(|p| p.parse().unwrap_or(0)).collect();
    let (y, m, d) = if parts[0].len() == 4 {
        (n[0], n[1], n[2])
    } else {
        match order {
            "mdy" => (n[2], n[0], n[1]),
            "ymd" => (n[0], n[1], n[2]),
            _ => (n[2], n[1], n[0]),
        }
    };
    let y = if y < 100 { 2000 + y } else { y };
    NaiveDate::from_ymd_opt(y, m as u32, d as u32).map(|d| d.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_as_banks_write_them() {
        assert_eq!(parse_amount("1,234.56"), Some(123456));
        assert_eq!(parse_amount("1.234,56"), Some(123456));
        assert_eq!(parse_amount("(12.00)"), Some(-1200));
        assert_eq!(parse_amount("12.00-"), Some(-1200));
        assert_eq!(parse_amount("-7.5"), Some(-750));
        assert_eq!(parse_amount("SAR 5"), Some(500));
        assert_eq!(parse_amount("45.10 DR"), Some(-4510));
        assert_eq!(parse_amount("1,000"), Some(100000));
        assert_eq!(parse_amount("12,5"), Some(1250));
        assert_eq!(parse_amount("abc"), None);
    }

    #[test]
    fn dates_in_each_order() {
        assert_eq!(parse_date("2026-10-03", "dmy").as_deref(), Some("2026-10-03"));
        assert_eq!(parse_date("03/10/2026", "dmy").as_deref(), Some("2026-10-03"));
        assert_eq!(parse_date("10/03/2026", "mdy").as_deref(), Some("2026-10-03"));
        assert_eq!(parse_date("3 Oct 2026", "dmy").as_deref(), Some("2026-10-03"));
        assert_eq!(parse_date("03.10.26", "dmy").as_deref(), Some("2026-10-03"));
        assert_eq!(parse_date("Opening balance", "dmy"), None);
    }

    #[test]
    fn columns_by_letter() {
        assert_eq!(column_index("A"), Some(0));
        assert_eq!(column_index("z"), Some(25));
        assert_eq!(column_index("AA"), Some(26));
        assert_eq!(column_letters(28)[27], "AB");
    }

    #[test]
    fn a_csv_statement_converts_with_a_mapping_and_skips_notes() {
        let mut files = Files::empty();
        let csv = "Bank statement,,,\nDate,Description,Debit,Credit\n01/10/2026,Opening balance,,\n03/10/2026,\"Bistro, Riyadh\",1240.00,\n05/10/2026,Salary,,18000.00\n,Total,1240.00,18000.00\n";
        let meta = files.insert("statement.csv", csv.as_bytes().to_vec());
        let table = read_table(&files, &meta.id, None).unwrap();
        assert_eq!(table.rows[1].cells, vec!["Date", "Description", "Debit", "Credit"]);
        let mapping: Mapping = serde_json::from_value(serde_json::json!({
            "headerRow": 2, "date": "A", "payee": "B", "outflow": "C", "inflow": "D", "dateOrder": "dmy", "skipRows": [3]
        }))
        .unwrap();
        let (rows, skipped) = convert(&files, &meta.id, &mapping).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], ImportRow { date: "2026-10-03".into(), payee: "Bistro, Riyadh".into(), memo: String::new(), amount: -124000, transfer_account_id: None });
        assert_eq!(rows[1].amount, 1800000);
        assert_eq!(skipped, vec!["row 6 has an amount but no date".to_string()]);
    }

    #[test]
    fn unknown_files_are_refused() {
        assert!(kind_of("virus.exe").is_none());
        assert_eq!(kind_of("Receipt.JPG").unwrap().0, Kind::Image);
    }

    #[test]
    fn mapped_transfer_column_preserves_account_ids() {
        let mut files = Files::empty();
        let meta = files.insert("statement.csv", b"Date,Payee,Amount,Other account\n2026-10-24,Card payment,-100,card\n2026-10-25,Shop,-5,\n".to_vec());
        let mapping: Mapping = serde_json::from_value(serde_json::json!({
            "headerRow": 1, "date": "A", "payee": "B", "amount": "C", "transferAccountId": "D", "dateOrder": "ymd"
        })).unwrap();
        let (rows, skipped) = convert(&files, &meta.id, &mapping).unwrap();
        assert!(skipped.is_empty());
        assert_eq!(rows[0].transfer_account_id.as_deref(), Some("card"));
        assert_eq!(rows[1].transfer_account_id, None);
    }

    /// A JPEG as a camera writes it: JFIF, Exif (on its side, with a place), XMP, then the picture.
    fn camera_jpeg() -> Vec<u8> {
        let segment = |marker: u8, body: &[u8]| {
            let mut s = vec![0xFF, marker];
            s.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
            s.extend_from_slice(body);
            s
        };
        // Orientation 6, big-endian as some cameras write it, and a place after it.
        let mut exif = b"Exif\0\0MM\0*\0\0\0\x08\0\x02\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\x88\x25\0\x04\0\0\0\x01\0\0\0\x26\0\0\0\0".to_vec();
        exif.extend_from_slice(b"GPS 24.7136N 46.6753E");
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(segment(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
        jpeg.extend(segment(0xE1, &exif));
        jpeg.extend(segment(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta>Riyadh</x:xmpmeta>"));
        jpeg.extend(segment(0xDB, &[0; 65]));
        jpeg.extend(segment(0xDA, &[1, 1, 0, 0, 63, 0]));
        jpeg.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn a_photo_is_kept_without_where_it_was_taken_but_the_right_way_up() {
        let photo = camera_jpeg();
        let clean = scrub("image/jpeg", &photo);
        let found = |needle: &[u8]| clean.windows(needle.len()).any(|w| w == needle);
        assert!(!found(b"GPS") && !found(b"Riyadh") && !found(b"xmpmeta"));
        // The picture and its tables are untouched, and it still says it's on its side.
        assert!(found(b"JFIF") && clean.ends_with(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD9]));
        assert!(clean.starts_with(&[0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x22]));
        assert_eq!(exif_orientation(&clean[6..38]), Some(6));
        // Nothing to take out: the same bytes. Not a JPEG at all: the same bytes.
        let plain = scrub("image/jpeg", &clean);
        assert_eq!(exif_orientation(&plain[6..38]), Some(6));
        assert!(!plain.windows(3).any(|w| w == b"GPS"));
        assert_eq!(&*scrub("image/jpeg", b"not a picture"), b"not a picture");
        assert_eq!(&*scrub("image/jpeg", &photo[..40]), &photo[..40]);
    }

    #[test]
    fn a_png_loses_its_text_and_exif() {
        let chunk = |kind: &[u8; 4], data: &[u8]| {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            c.extend_from_slice(&[0; 4]);
            c
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"tEXt", b"Location\0Riyadh"));
        png.extend(chunk(b"eXIf", b"MM\0*GPS"));
        png.extend(chunk(b"IDAT", &[7; 20]));
        png.extend(chunk(b"IEND", &[]));
        let clean = scrub("image/png", &png);
        assert!(!clean.windows(6).any(|w| w == b"Riyadh") && !clean.windows(3).any(|w| w == b"GPS"));
        assert!(clean.windows(4).any(|w| w == b"IDAT") && clean.windows(4).any(|w| w == b"IEND"));
        assert_eq!(clean.len(), png.len() - (12 + 15) - (12 + 7));
    }

    #[test]
    fn a_chosen_file_waits_until_its_message_is_sent() {
        let folder = tempfile::tempdir().unwrap();
        let files = Files::open(folder.path().join("files"));
        let kept = files.stage("notes.txt", b"rent 1200").unwrap();
        let dropped = files.stage("other.txt", b"never sent").unwrap();
        // Chosen, not sent: nothing among the conversation's files, and it reads from where it waits.
        assert!(!folder.path().join("files").exists());
        assert_eq!(files.bytes(&kept.id).unwrap(), b"rent 1200");
        files.discard(&dropped.id);
        assert!(files.get(&dropped.id).is_err());
        files.adopt(&kept.id).unwrap();
        assert!(folder.path().join("files").join(format!("{}.bin", kept.id)).is_file());
        assert_eq!(files.get(&kept.id).unwrap(), kept);
        // Sent again, or never there: nothing to do. Taking it off now leaves what was sent.
        files.adopt(&kept.id).unwrap();
        files.adopt("file_unknown").unwrap();
        files.discard(&kept.id);
        assert_eq!(files.bytes(&kept.id).unwrap(), b"rent 1200");
        let late = files.stage("late.txt", b"x").unwrap();
        files.discard_waiting();
        assert!(files.get(&late.id).is_err() && files.get(&kept.id).is_ok());
    }
}
