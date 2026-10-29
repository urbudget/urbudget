//! A bank file as lines, each with what became of it: read, needing attention (a cell that doesn't
//! read, with why), or left out (pending, another account, a balance… with why). Nothing a line
//! holds stops the others from reading; only problems with the whole file (no date column, an
//! unreadable file) are errors.
//!
//! An import session (`Session`) reads its file one way (`Structure`) with what someone changed
//! about its lines (`Edits`). Changes come as `Fixes` made against one revision of one session,
//! and apply only where they still fit it, so neither someone's edits nor the assistant's land on
//! a line that changed since, or on another file.
use crate::bank_csv::{self, Columns, FileSettings, Options};
use crate::bank_import::{self, BankTransaction, CompactReview, Counter, DateReading, Identity};
use crate::bank_statements::{self, cents, LineOptions};
use crate::domain::{validate, Budget};
use crate::t;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// A part of a transaction a line gives.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    Date,
    Payee,
    Memo,
    Amount,
    TransferAccountId,
}

/// Why a line isn't imported unless someone includes it (where that makes sense).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// Not booked yet: it can come back booked on another day or for another sum.
    Pending,
    /// Declined, failed, reversed or cancelled: the money never moved.
    Declined,
    /// Another account's, in a file with several.
    OtherAccount,
    /// In another currency than the budget's.
    OtherCurrency,
    /// Money moved between the holder's own balances (Alipay's 不计收支, WeChat's "/").
    OwnBalances,
    /// An opening or closing balance, or a total, written as a row.
    Balance,
    /// Above the header row: an account summary or a title.
    AboveHeader,
    /// No date where the date goes: a note, or a pending row without its date yet.
    NoDate,
    /// No amount: a row that states something rather than moving money.
    NoAmount,
    /// Nothing moved: a card check, a statement closing.
    Zero,
    /// A correction removed it (OFX), or it corrects another.
    Corrected,
    /// The header row again, as some banks write before each account's rows.
    RepeatedHeader,
    /// One of a batch's payments, imported as the batch's one entry.
    InWholeBatch,
    /// It didn't read without help, and was imported once helped: this is the same file again.
    Imported,
}
impl Reason {
    /// Whether "Include anyway" makes sense: the line then reads like any other, and needs fixing
    /// where it still lacks something (a date, an amount in the budget's currency).
    pub fn includable(self) -> bool {
        matches!(self, Reason::Pending | Reason::Declined | Reason::OtherCurrency | Reason::OwnBalances | Reason::Balance | Reason::NoDate | Reason::NoAmount)
    }
}

/// What became of a line.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Status {
    Ready,
    /// A part that doesn't read, in which column where the file has columns, and why.
    #[serde(rename_all = "camelCase")]
    Attention { field: Field, column: Option<usize>, reason: String },
    #[serde(rename_all = "camelCase")]
    LeftOut { reason: Reason, detail: String },
}

/// Words a state column uses for a payment that never went through, rather than one not booked
/// yet (after `plain`).
pub(crate) const DECLINED: &[&str] = &[
    "reverted", "declined", "failed", "cancelled", "canceled", "rejected", "void", "voided", "storniert", "abgelehnt",
    "geweigerd", "交易关闭",
];

/// A line's fields as they read now: its date (YYYY-MM-DD) and amount ("-12.40") where they read,
/// as the file writes them (or as given) where they don't.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Fields {
    pub date: String,
    pub payee: String,
    pub memo: String,
    pub amount: String,
    /// The other account's ID, never a payee label. Empty means an ordinary transaction.
    #[serde(default)]
    pub transfer_account_id: String,
}
impl Fields {
    pub fn get(&self, field: Field) -> &str {
        match field {
            Field::Date => &self.date,
            Field::Payee => &self.payee,
            Field::Memo => &self.memo,
            Field::Amount => &self.amount,
            Field::TransferAccountId => &self.transfer_account_id,
        }
    }
}

/// Which import something is for: one session of one file, into one account of one budget.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Scope {
    pub session: String,
    pub budget: String,
    pub account: String,
    /// The file's fingerprint (see `fingerprint`).
    pub file: String,
}
/// How a file writes its dates.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct DateOrder {
    /// "ymd", "dmy" or "mdy"; empty for the account's, or else year first.
    pub order: String,
    /// Someone said it's right, for dates that don't settle it themselves.
    pub confirmed: bool,
}
/// Everything that changes how a file's cells read: changing any of it rereads every line, and a
/// fix made while it was otherwise no longer applies.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Structure {
    /// "csv", or the format the file is in.
    pub format: String,
    /// A workbook's sheet, by its name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// The account to read in a file with several, by its label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Where a CSV's header row starts, as someone chose it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_line: Option<usize>,
    /// A CSV's columns, as someone chose them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Columns>,
    pub date_order: DateOrder,
    /// A CSV's purchases are positive.
    pub flip_signs: bool,
    /// An OFX whose amounts are all positive: money out is told by the transaction types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signs_by_type: Option<bool>,
    /// CAMT batches imported as one entry each, by their entry's line.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub whole_batches: Vec<String>,
}
/// What someone changed about lines, by line.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Edits {
    /// A CSV's cells, by column.
    pub cells: BTreeMap<String, BTreeMap<usize, String>>,
    pub fields: BTreeMap<String, BTreeMap<Field, String>>,
    /// Lines included though they're left out.
    pub include: BTreeSet<String>,
}
/// An import in progress: which, how its file reads, what's been changed about its lines, and
/// how often anything has (each change makes a new revision).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub scope: Scope,
    pub revision: u64,
    pub structure: Structure,
    pub edits: Edits,
    /// How many times something about each line changed (none: 0).
    pub versions: BTreeMap<String, u64>,
}
/// A file's fingerprint: its contents' SHA-256.
pub fn fingerprint(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}
impl Session {
    /// A new import of a file into an account.
    pub fn new(id: &str, budget: &str, account: &str, content: &str, structure: Structure) -> Session {
        let scope = Scope { session: id.into(), budget: budget.into(), account: account.into(), file: fingerprint(content) };
        Session { scope, structure, ..Session::default() }
    }
}
/// A new import of a file into an account, read as the account's last file was (its sheet,
/// account, date order and signs), else as the file suggests: the date order its dates fit, and a
/// card's purchases written as positive when most amounts are, unless a line's amount needs
/// fixing (it could be the one that says otherwise).
pub fn start(b: &Budget, account: &str, content: &str, id: &str, budget: &str, sheet: Option<String>) -> Result<Session, String> {
    let kept = bank_csv::settings(b, account);
    let layout = bank_csv::layout(b, account, content, None)?;
    let structure = Structure {
        format: layout.format.clone(),
        sheet,
        source: kept.source.clone(),
        date_order: DateOrder {
            order: layout.date_format.clone().unwrap_or_default(),
            confirmed: kept.date_format.is_some() && kept.date_confirmed == Some(true),
        },
        ..Structure::default()
    };
    let mut session = Session::new(id, budget, account, content, structure);
    if layout.format == "csv" && kept.flip_signs.is_none() && layout.flip_signs {
        let doubt = statement(b, account, content, &session)?.lines.iter().any(|l| matches!(l.status, Status::Attention { field: Field::Amount, .. }));
        session.structure.flip_signs = !doubt;
    } else {
        session.structure.flip_signs = layout.flip_signs;
    }
    Ok(session)
}
/// A new import of a file the assistant prepared, written the standard way (Date, Payee, Memo,
/// Amount; ISO dates; money out negative): read so, whatever the account's last file was like.
pub fn start_prepared(content: &str, account: &str, id: &str, budget: &str) -> Session {
    let transfer = csv::Reader::from_reader(content.as_bytes()).headers().ok()
        .is_some_and(|headers| headers.iter().any(|h| h == "TransferAccountId"));
    let columns = Columns { date: Some("Date".into()), payee: Some("Payee".into()), memo: Some("Memo".into()), amount: Some("Amount".into()),
        transfer_account_id: transfer.then(|| "TransferAccountId".into()), ..Columns::default() };
    let structure = Structure {
        format: "csv".into(),
        columns: Some(columns),
        date_order: DateOrder { order: "ymd".into(), confirmed: true },
        ..Structure::default()
    };
    Session::new(id, budget, account, content, structure)
}

/// Changes to an import, made against one revision of it: applied together where they still
/// fit it, never to another import.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Fixes {
    pub scope: Scope,
    pub base_revision: u64,
    /// How the file read when these were made.
    pub base_structure: Structure,
    /// A new way to read it. With it, every fix here applies, or none does.
    pub structure: Option<Structure>,
    pub cells: Vec<CellFix>,
    pub fields: Vec<FieldFix>,
    pub include: Vec<IncludeFix>,
    /// Lines to put back as the file has them.
    pub revert: Vec<LineRef>,
}
/// A cell's new value, and the one it had when the fix was made. A line's version is how many
/// times something about it had changed then.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CellFix {
    pub line: String,
    pub line_version: u64,
    pub column: usize,
    pub from: String,
    pub to: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldFix {
    pub line: String,
    #[serde(default)]
    pub line_version: u64,
    pub field: Field,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct IncludeFix {
    pub line: String,
    pub line_version: u64,
    pub include: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LineRef {
    pub line: String,
    pub line_version: u64,
}
/// What became of one fix: applied, or why not.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FixResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// "structure", "cell", "field", "include" or "revert".
    pub fix: &'static str,
    pub applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub session: Session,
    pub results: Vec<FixResult>,
}

/// A bank file as lines, each with what became of it, as an import session reads it.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Statement {
    pub scope: Scope,
    pub revision: u64,
    pub structure: Structure,
    /// The format it read as.
    pub format: String,
    /// The accounts in a file with several, and which one is read.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<usize>,
    /// A CSV's header row; a statement's fields (Date, Payee, Memo, Amount).
    pub headers: Vec<String>,
    /// A CSV's columns as read, and where its header row starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Columns>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_line: Option<usize>,
    /// The columns are the account's saved choice.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub saved: bool,
    /// The order its dates read in.
    pub order: String,
    pub lines: Vec<Line>,
    pub counts: Counts,
    /// Dates two orders read differently, which the file doesn't settle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_order: Option<DateQuestion>,
    /// How an OFX whose amounts are all positive gets its signs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signs: Option<SignQuestion>,
    /// The account read is in another currency than the budget's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub batches: Vec<BatchInfo>,
    /// What the account keeps for its next file, once this one is imported.
    pub remember: FileSettings,
    #[serde(skip)]
    memo_column: Option<usize>,
    #[serde(skip)]
    id_column: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub lines: usize,
}
/// A line of the file.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// Where it starts in the file (a CSV), or which record it is (`t1`, `r1`, `e1`, `e1.2`).
    pub id: String,
    /// How many times something about it changed.
    pub version: u64,
    /// Its cells with any fixes: a CSV's, or a statement's fields.
    pub cells: Vec<String>,
    /// Its cells as the file writes them, when some were fixed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub original: Vec<String>,
    pub fields: Fields,
    pub status: Status,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub included: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<usize>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub ready: usize,
    pub attention: usize,
    #[serde(default)]
    pub left_out: BTreeMap<Reason, usize>,
}
impl Counts {
    fn of(lines: &[Line]) -> Counts {
        let mut counts = Counts::default();
        for l in lines {
            match &l.status {
                Status::Ready => counts.ready += 1,
                Status::Attention { .. } => counts.attention += 1,
                Status::LeftOut { reason, .. } => *counts.left_out.entry(*reason).or_default() += 1,
            }
        }
        counts
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DateQuestion {
    pub order: String,
    /// The first date the orders read differently.
    pub example: String,
    /// Not confirmed yet: an import asks before going on.
    pub unconfirmed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SignQuestion {
    pub by_type: bool,
    pub unconfirmed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchInfo {
    pub entry: String,
    /// The entry's own line when it's imported as one, and its payments' lines (indexes in `lines`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub payments: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adds_up: Option<bool>,
    pub as_one: bool,
}
/// A statement and the review of the lines that read: `lines[i]` is the line `review.items[i]`
/// comes from.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementPreview {
    pub statement: Statement,
    pub review: CompactReview,
    pub lines: Vec<String>,
}

/// What the review needs of a line besides what's shown.
struct Extra {
    transaction: Option<BankTransaction>,
    original: Option<BankTransaction>,
    reserves: bool,
    bank_id: String,
    /// A statement's line as the file writes it, with its bank ID; empty for a CSV's, which is
    /// its line's cells as written.
    written: Vec<String>,
    /// The transaction 1.0.8 read a CSV's line as (see `bank_csv::CsvLine::then`).
    then: Option<BankTransaction>,
}
struct Reading {
    statement: Statement,
    extra: Vec<Extra>,
    batches: Vec<bank_statements::Batch>,
}

/// A file as an import session reads it.
fn reading(b: &Budget, account: &str, content: &str, session: &Session) -> Result<Reading, String> {
    validate(b)?;
    if !b.accounts.iter().any(|a| a.id == account) {
        return Err(t!("Choose an account."));
    }
    if content.len() > 20_000_000 {
        return Err(t!("Import a file smaller than 20 MB."));
    }
    if session.scope.account != account || session.scope.file != fingerprint(content) {
        return Err(t!("This import is for another file. Open the file again."));
    }
    let s = &session.structure;
    let edits = &session.edits;
    let version = |id: &str| session.versions.get(id).copied().unwrap_or(0);
    let kept = bank_csv::settings(b, account);
    let order = Some(s.date_order.order.as_str()).filter(|o| !o.is_empty()).or(kept.date_format.as_deref()).unwrap_or("ymd");
    let format = bank_statements::sniff(content).unwrap_or(if s.format.is_empty() { "csv" } else { &s.format });
    let mut statement = Statement {
        scope: session.scope.clone(),
        revision: session.revision,
        structure: s.clone(),
        format: format.into(),
        sources: vec![],
        source: None,
        headers: vec![],
        columns: None,
        header_line: None,
        saved: false,
        order: order.into(),
        lines: vec![],
        counts: Counts::default(),
        date_order: None,
        signs: None,
        currency: None,
        batches: vec![],
        remember: FileSettings::default(),
        memo_column: None,
        id_column: None,
    };
    let question = |d: &DateReading| d.example.clone().map(|example| DateQuestion { order: d.order.into(), example, unconfirmed: d.unconfirmed });
    let (mut extra, mut batches) = (vec![], vec![]);
    if format == "csv" {
        let offset = |id: &String| id.parse::<usize>().ok();
        let o = Options {
            columns: s.columns.as_ref(),
            header_at: s.header_line,
            flip_signs: s.flip_signs,
            source: s.source.as_deref(),
            include: edits.include.iter().filter_map(offset).collect(),
            cells: edits.cells.iter().filter_map(|(id, c)| Some((offset(id)?, c.iter().map(|(k, v)| (*k, v.clone())).collect()))).collect(),
            fields: edits.fields.iter().filter_map(|(id, f)| Some((offset(id)?, f.iter().map(|(k, v)| (*k, v.clone())).collect()))).collect(),
            shown: true,
        };
        let read = match bank_csv::read_lines(b, account, content, &o, order, s.date_order.confirmed) {
            Ok(read) => read,
            // Columns that aren't found: every line, for someone to say which holds what.
            Err(why) => {
                let unread = bank_csv::unread(content, &b.currency, s.header_line).map_err(|_| why.clone())?;
                // No date or amount column found (the messages `bank_csv::read_lines` gives for it).
                let columns = why == t!("CSV needs Date, Payee/Description, and Amount (or Outflow/Inflow) headers.") || why == t!("CSV needs Amount or Outflow/Inflow columns.");
                let reason = if columns { t!("Choose the columns with the date and the amount (or money out and in).") } else { why };
                for (offset, cells, above) in unread.lines {
                    let id = offset.to_string();
                    let status = if above {
                        Status::LeftOut { reason: Reason::AboveHeader, detail: String::new() }
                    } else {
                        Status::Attention { field: Field::Date, column: None, reason: reason.clone() }
                    };
                    extra.push(Extra { transaction: None, original: None, reserves: false, bank_id: String::new(), written: vec![], then: None });
                    statement.lines.push(Line {
                        version: version(&id),
                        included: false,
                        fixed: false,
                        id,
                        cells,
                        original: vec![],
                        fields: Fields::default(),
                        status,
                        batch: None,
                    });
                }
                statement.headers = unread.headers;
                statement.columns = s.columns.clone();
                statement.header_line = Some(unread.start);
                statement.counts = Counts::of(&statement.lines);
                statement.remember = remembered(&statement);
                return Ok(Reading { statement, extra, batches });
            }
        };
        let r = &read.records;
        let order = read.dates.order;
        let cell = |cells: &[String], i: Option<usize>| i.and_then(|i| cells.get(i)).cloned().unwrap_or_default();
        for l in read.lines {
            let id = l.offset.to_string();
            let given = |field: Field| edits.fields.get(&id).and_then(|f| f.get(&field)).cloned();
            let fields = match &l.transaction {
                Some(t) => Fields { date: t.date.clone(), payee: t.payee.clone(), memo: t.memo.clone(), amount: cents(t.amount), transfer_account_id: t.transfer_account_id.clone().unwrap_or_default() },
                None => Fields {
                    // A date that reads, as it reads, though another part of the line doesn't.
                    date: given(Field::Date).unwrap_or_else(|| {
                        let written = cell(&l.cells, Some(r.date_column));
                        crate::bank_import::date(&written, order).unwrap_or(written)
                    }),
                    payee: given(Field::Payee).unwrap_or_else(|| cell(&l.cells, r.payee_column)),
                    memo: given(Field::Memo).unwrap_or_else(|| cell(&l.cells, r.memo_column)),
                    amount: l.parsed_amount.map(cents).or_else(|| given(Field::Amount))
                        .unwrap_or_else(|| r.amount_columns.iter().map(|&i| cell(&l.cells, i)).find(|c| !c.trim().is_empty()).unwrap_or_default()),
                    transfer_account_id: given(Field::TransferAccountId).unwrap_or_else(|| cell(&l.cells, r.transfer_column)),
                },
            };
            extra.push(Extra { transaction: l.transaction, original: l.original, reserves: l.reserves, bank_id: l.bank_id, written: vec![], then: l.then });
            statement.lines.push(Line {
                version: version(&id),
                included: edits.include.contains(&id),
                fixed: edits.cells.contains_key(&id) || edits.fields.contains_key(&id),
                id,
                cells: l.cells,
                original: l.written,
                fields,
                status: l.status,
                batch: None,
            });
        }
        let r = read.records;
        statement.sources = r.sources.iter().map(|(label, lines)| SourceInfo { label: label.clone(), currency: None, lines: *lines }).collect();
        statement.source = (!r.sources.is_empty()).then_some(r.source);
        statement.headers = r.headers;
        statement.columns = Some(r.columns);
        statement.header_line = Some(r.start);
        statement.saved = r.saved;
        statement.order = read.dates.order.into();
        statement.date_order = question(&read.dates);
        statement.memo_column = r.memo_column;
        statement.id_column = r.id_column;
    } else {
        let o = LineOptions {
            source: s.source.as_deref(),
            legacy: false,
            include: edits.include.iter().cloned().collect(),
            fields: edits.fields.iter().map(|(id, f)| (id.clone(), f.iter().map(|(k, v)| (*k, v.clone())).collect())).collect(),
            whole_batches: s.whole_batches.iter().cloned().collect(),
            date_order: order,
            confirmed: s.date_order.confirmed,
            signs_by_type: s.signs_by_type,
        };
        let read = bank_statements::lines(format, content, &b.currency, &o)?;
        for l in read.lines {
            let fixed = edits.fields.contains_key(&l.key);
            let f = &l.fields;
            let w = l.written;
            let written = vec![w.date, w.payee, w.memo, w.amount];
            statement.lines.push(Line {
                version: version(&l.key),
                included: edits.include.contains(&l.key),
                fixed,
                cells: vec![f.date.clone(), f.payee.clone(), f.memo.clone(), f.amount.clone()],
                original: if fixed { written.clone() } else { vec![] },
                fields: l.fields,
                status: l.status,
                batch: l.batch,
                id: l.key,
            });
            let mut written = written;
            written.push(l.bank_id.clone());
            extra.push(Extra { transaction: l.transaction, original: l.original, reserves: l.reserves, bank_id: l.bank_id, written, then: None });
        }
        statement.sources = read.sources.iter().map(|s| SourceInfo { label: s.label.clone(), currency: s.currency.clone(), lines: s.lines }).collect();
        statement.source = (!read.sources.is_empty()).then_some(read.source);
        statement.currency = read.sources.get(read.source).and_then(|s| s.currency.clone()).filter(|c| !c.eq_ignore_ascii_case(&b.currency));
        statement.headers = ["Date", "Payee", "Memo", "Amount"].map(String::from).to_vec();
        if let Some(dates) = &read.dates {
            statement.order = dates.order.into();
            statement.date_order = question(dates);
        }
        statement.signs = read.signs.map(|s| SignQuestion { by_type: s.by_type, unconfirmed: s.unconfirmed });
        statement.memo_column = Some(2);
        statement.batches = read.batches.iter().map(|b| BatchInfo {
            entry: b.entry.clone(),
            line: b.line,
            payments: b.payments.clone(),
            total: b.total,
            adds_up: b.adds_up,
            as_one: b.as_one,
        }).collect();
        batches = read.batches;
    }
    // Non-CSV readers have no budget context; validate their explicit account choices here.
    for (line, detail) in statement.lines.iter_mut().zip(&mut extra) {
        if line.status == Status::Ready {
            if let Err(reason) = bank_import::validate_transfer(b, account, &line.fields.transfer_account_id) {
                line.status = Status::Attention { field: Field::TransferAccountId, column: None, reason };
                detail.transaction = None;
            }
        }
    }
    statement.counts = Counts::of(&statement.lines);
    statement.remember = remembered(&statement);
    Ok(Reading { statement, extra, batches })
}

/// What an account keeps of how its file read: the date order used, the columns chosen (when
/// each names one column), the sheet and the account read; never the header row's place, which
/// another file has elsewhere.
fn remembered(statement: &Statement) -> FileSettings {
    let s = &statement.structure;
    let once = |name: &Option<String>| {
        name.as_ref().is_none_or(|n| !n.trim().is_empty() && statement.headers.iter().filter(|h| h.trim() == n.trim()).count() == 1)
    };
    let table = statement.format == "csv";
    FileSettings {
        columns: s.columns.clone().filter(|c| table && c.names().into_iter().all(once)),
        date_format: Some(statement.order.clone()),
        date_confirmed: Some(s.date_order.confirmed),
        flip_signs: table.then_some(s.flip_signs),
        sheet: s.sheet.clone(),
        source: statement.source.filter(|_| statement.sources.len() > 1).and_then(|i| statement.sources.get(i)).map(|s| s.label.clone()),
    }
}
/// The aliases of the lines `wanted`: each line as the file writes it, and how many identical
/// lines came before.
fn aliases(account: &str, lines: &[Line], extra: &[Extra], wanted: impl Fn(usize) -> bool) -> Vec<Option<String>> {
    let mut seen: HashMap<&[String], usize> = HashMap::new();
    let written = |i: usize| match (&extra[i].written, &lines[i]) {
        (w, _) if !w.is_empty() => w.as_slice(),
        (_, l) if !l.original.is_empty() => l.original.as_slice(),
        (_, l) => l.cells.as_slice(),
    };
    (0..extra.len())
        .map(|i| {
            let n = seen.entry(written(i)).or_default();
            *n += 1;
            wanted(i).then(|| bank_import::bank_id_of(account, &format!("line:v1:{}:{n}", serde_json::to_string(written(i)).unwrap_or_default())))
        })
        .collect()
}
/// Lines that don't read without help but were imported once helped: the same file again finds
/// them imported.
fn marked(b: &Budget, account: &str, mut read: Reading) -> Reading {
    let open = |line: &Line| match &line.status {
        Status::Ready => false,
        Status::Attention { .. } => true,
        Status::LeftOut { reason, .. } => reason.includable(),
    };
    let lines = &read.statement.lines;
    if !lines.iter().any(open) {
        return read;
    }
    let aliases = aliases(account, lines, &read.extra, |i| open(&lines[i]));
    let imported = bank_import::import_ids(b);
    for (line, alias) in read.statement.lines.iter_mut().zip(aliases) {
        if alias.is_some_and(|a| imported.contains(&a)) {
            line.status = Status::LeftOut { reason: Reason::Imported, detail: String::new() };
        }
    }
    read.statement.counts = Counts::of(&read.statement.lines);
    read
}
/// Each line's identity, and each batch's ID. Lines that read without help keep the identities
/// today's import gives them; a damaged line keeps its place among lines sharing its bank ID;
/// lines that read only with help come after those. A batch is known by its whole entry's ID,
/// as today's import counts it, or would if it read it whole.
fn identities(account: &str, lines: &[Line], extra: &[Extra], batches: &[bank_statements::Batch]) -> (Vec<Option<Identity>>, Vec<Option<String>>) {
    let mut counter = Counter::default();
    let mut who: Vec<Option<Identity>> = vec![None; extra.len()];
    let mut named: Vec<Option<String>> = vec![None; batches.len()];
    let starts: HashMap<usize, usize> =
        batches.iter().enumerate().filter_map(|(k, b)| b.line.or(b.payments.first().copied()).map(|l| (l, k))).collect();
    let mut name = |counter: &mut Counter, i: usize, idless: bool| {
        let Some(&k) = starts.get(&i) else { return };
        // Another account's batch in the same file counts toward none of this one's.
        if matches!(lines[i].status, Status::LeftOut { reason: Reason::OtherAccount, .. }) {
            return;
        }
        let b = &batches[k];
        if let Some(whole) = b.whole.as_ref().filter(|w| !b.as_one && w.fit.is_empty() == idless) {
            named[k] = Some(bank_import::bank_id_of(account, &counter.identity(whole, !b.splits_today).signature));
        }
    };
    // With the bank's ID: lines today's import reads, or would but for damage, in order…
    for (i, e) in extra.iter().enumerate() {
        name(&mut counter, i, false);
        if !e.reserves {
            continue;
        }
        let known = BankTransaction { fit: e.bank_id.clone(), date: String::new(), payee: String::new(), memo: String::new(), amount: 0, transfer_account_id: None };
        let t = e.original.as_ref().or(e.transaction.as_ref()).unwrap_or(&known);
        if !t.fit.is_empty() {
            let id = counter.identity(t, true);
            who[i] = e.transaction.as_ref().map(|_| id);
        }
    }
    // …then lines included though they're left out.
    for (i, e) in extra.iter().enumerate() {
        if let Some(t) = e.transaction.as_ref().filter(|t| !e.reserves && !t.fit.is_empty()) {
            who[i] = Some(counter.identity(t, true));
        }
    }
    // Without one: lines that read without help, by what they read as…
    for (i, e) in extra.iter().enumerate() {
        name(&mut counter, i, true);
        if let Some(o) = e.original.as_ref().filter(|o| o.fit.is_empty()) {
            let id = counter.identity(o, true);
            who[i] = e.transaction.as_ref().map(|_| id);
        }
    }
    // …then lines that read only with help.
    for (i, e) in extra.iter().enumerate() {
        if let Some(t) = e.transaction.as_ref().filter(|t| e.original.is_none() && t.fit.is_empty()) {
            who[i] = Some(counter.identity(t, true));
        }
    }
    for (k, b) in batches.iter().enumerate() {
        if let Some(line) = b.line.filter(|_| b.as_one) {
            named[k] = who[line].as_ref().map(|w| bank_import::bank_id_of(account, &w.signature));
        }
        for &l in b.payments.iter().chain(&b.line) {
            if let Some(w) = who[l].as_mut() {
                w.batch.clone_from(&named[k]);
            }
        }
    }
    (who, named)
}

/// A file as lines, each with what became of it, as an import session reads it: nothing a line
/// holds stops the others from reading. No matching against the budget's transactions; see
/// `preview_statement`.
pub fn statement(b: &Budget, account: &str, content: &str, session: &Session) -> Result<Statement, String> {
    Ok(marked(b, account, reading(b, account, content, session)?).statement)
}
/// A statement, and the review of the lines that read: each with its identity, whether it's
/// imported already, and the transactions it could match.
pub fn preview_statement(b: &Budget, account: &str, content: &str, session: &Session) -> Result<StatementPreview, String> {
    let read = marked(b, account, reading(b, account, content, session)?);
    let (who, _) = identities(account, &read.statement.lines, &read.extra, &read.batches);
    let all = &read.statement.lines;
    // Lines that read only with help are known by the line as written too.
    let helped = |i: usize| read.extra[i].original.is_none() && all[i].status == Status::Ready;
    let mut aliases = if (0..all.len()).any(helped) { aliases(account, all, &read.extra, helped) } else { vec![None; all.len()] };
    // And by the ID 1.0.8 gave the line, where it read it otherwise (another column for the
    // payee, a fee taken off, spaces tidied): a file it imported is found imported still.
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut earlier: Vec<Option<String>> = read
        .extra
        .iter()
        .map(|e| {
            let t = e.then.as_ref()?;
            let signature = if t.fit.is_empty() {
                let key = serde_json::json!([t.date, t.payee, t.memo, t.amount]).to_string();
                let n = seen.entry(key.clone()).or_default();
                *n += 1;
                format!("idless:v2:{key}:{n}")
            // The same bank ID is the ID today's import gives its first line already.
            } else if t.fit != e.bank_id {
                format!("fit:{}", t.fit)
            } else {
                return None;
            };
            Some(bank_import::bank_id_of(account, &signature))
        })
        .collect();
    // Never another line's own ID: 1.0.8 gave lines sharing a bank ID one ID between them, and a
    // line it read untidied can read as another does now.
    let own: HashSet<String> = who.iter().flatten().map(|w| bank_import::bank_id_of(account, &w.signature)).collect();
    let (mut rows, mut lines) = (vec![], vec![]);
    for (i, (e, who)) in read.extra.into_iter().zip(who).enumerate() {
        if let (Status::Ready, Some(t), Some(mut who)) = (&all[i].status, e.transaction, who) {
            who.aliases.extend(aliases[i].take());
            if let Some(then) = earlier[i].take().filter(|then| !own.contains(then) && !who.aliases.contains(then)) {
                who.aliases.push(then);
            }
            rows.push((t, who));
            lines.push(all[i].id.clone());
        }
    }
    if rows.len() > 10_000 {
        return Err(t!("Choose a file containing 1–10,000 transactions."));
    }
    let review = bank_import::review_identified(b, account, rows)?;
    Ok(StatementPreview { statement: read.statement, review, lines })
}

/// Applies changes to an import session where they still fit it (see `Fixes`): the session
/// after them, and what became of each.
pub fn apply_fixes(b: &Budget, account: &str, content: &str, session: &Session, fixes: &Fixes) -> Result<Applied, String> {
    if fixes.scope != session.scope {
        return Err(t!("This fix is for another import."));
    }
    if fixes.structure.is_some() && fixes.base_structure != session.structure {
        return Err(t!("The import changed since this fix was made."));
    }
    let read = reading(b, account, content, session)?;
    let now = &read.statement;
    let lines: HashMap<&str, &Line> = now.lines.iter().map(|l| (l.id.as_str(), l)).collect();
    let same = fixes.base_structure == session.structure;
    // A line as it was when a fix for it was made, or why the fix no longer fits it.
    let line = |id: &str, version: u64| -> Result<&Line, String> {
        if !same {
            return Err(t!("The file reads another way since this fix was made."));
        }
        let line = *lines.get(id).ok_or_else(|| t!("This file has no such line."))?;
        if session.versions.get(id).copied().unwrap_or(0) != version {
            return Err(t!("This line changed since this fix was made."));
        }
        Ok(line)
    };
    let (mut next, mut results, mut touched) = (session.clone(), vec![], BTreeSet::new());
    let mut done = |results: &mut Vec<FixResult>, id: &str, fix: &'static str, outcome: Result<(), String>| {
        if outcome.is_ok() {
            touched.insert(id.to_string());
        }
        results.push(FixResult { line: Some(id.into()), fix, applied: outcome.is_ok(), problem: outcome.err() });
    };
    let table = now.format == "csv";
    // The part of a transaction a CSV column gives, by the columns read.
    let field_of = |column: usize| -> Option<Field> {
        let c = now.columns.as_ref()?;
        let is = |n: &Option<String>| n.as_deref().and_then(|n| bank_csv::position(&now.headers, n)) == Some(column);
        if is(&c.date) {
            Some(Field::Date)
        } else if is(&c.payee) {
            Some(Field::Payee)
        } else if is(&c.memo) {
            Some(Field::Memo)
        } else if is(&c.amount) || is(&c.outflow) || is(&c.inflow) {
            Some(Field::Amount)
        } else if is(&c.transfer_account_id) {
            Some(Field::TransferAccountId)
        } else {
            None
        }
    };
    let empty = String::new();
    let check_cell = |f: &CellFix| -> Result<&Line, String> {
        line(&f.line, f.line_version).and_then(|l| {
            // A line cut short has its missing cells, empty, up to the header's.
            let value = match l.cells.get(f.column) {
                Some(v) => v,
                None if table && f.column < now.headers.len() => &empty,
                None => return Err(t!("This line has no such column.")),
            };
            if table && now.id_column == Some(f.column) {
                return Err(t!("The bank's ID for a line can't be changed."));
            }
            if *value != f.from {
                return Err(t!("This cell changed since this fix was made."));
            }
            if field_of(f.column) == Some(Field::TransferAccountId) {
                bank_import::validate_transfer(b, account, f.to.trim())?;
            }
            Ok(l)
        })
    };
    let check_field = |f: &FieldFix| -> Result<&Line, String> {
        line(&f.line, f.line_version).and_then(|l| {
            if l.fields.get(f.field) != f.from {
                return Err(t!("This value changed since this fix was made."));
            }
            if f.to.trim() == f.from.trim() {
                return Err(t!("The line already has that value."));
            }
            if f.field == Field::TransferAccountId {
                bank_import::validate_transfer(b, account, f.to.trim())?;
            }
            Ok(l)
        })
    };
    // An editor saves normalized cells together with their final semantic values. The cells
    // and fields of such a line form one change: a stale field must not leave its raw cell
    // applied after clearing a newer field override (which could reverse a money amount).
    let field_lines: HashSet<&str> = fixes.fields.iter().map(|f| f.line.as_str()).collect();
    let coupled: HashSet<&str> = fixes.cells.iter().map(|f| f.line.as_str()).filter(|id| field_lines.contains(id)).collect();
    let mut refused: HashMap<&str, String> = HashMap::new();
    for f in fixes.cells.iter().filter(|f| coupled.contains(f.line.as_str())) {
        if let Err(problem) = check_cell(f) { refused.entry(f.line.as_str()).or_insert(problem); }
    }
    for f in fixes.fields.iter().filter(|f| coupled.contains(f.line.as_str())) {
        if let Err(problem) = check_field(f) { refused.entry(f.line.as_str()).or_insert(problem); }
    }
    for f in &fixes.cells {
        let checked = match refused.get(f.line.as_str()) {
            Some(problem) => Err(problem.clone()),
            None => check_cell(f),
        };
        let outcome = checked.map(|l| {
            if table {
                let written = if l.original.is_empty() { &l.cells } else { &l.original };
                let cells = next.edits.cells.entry(f.line.clone()).or_default();
                if written.get(f.column).map(|w| w.trim()) == Some(f.to.trim()) {
                    cells.remove(&f.column);
                } else {
                    cells.insert(f.column, f.to.trim().to_string());
                }
                if cells.is_empty() {
                    next.edits.cells.remove(&f.line);
                }
                // The latest change wins: a value given for that part of the line before gives way.
                if let (Some(field), Some(given)) = (field_of(f.column), next.edits.fields.get_mut(&f.line)) {
                    given.remove(&field);
                    if given.is_empty() {
                        next.edits.fields.remove(&f.line);
                    }
                }
            } else {
                let field = [Field::Date, Field::Payee, Field::Memo, Field::Amount][f.column];
                next.edits.fields.entry(f.line.clone()).or_default().insert(field, f.to.trim().to_string());
            }
        });
        done(&mut results, &f.line, "cell", outcome);
    }
    for f in &fixes.fields {
        let checked = match refused.get(f.line.as_str()) {
            Some(problem) => Err(problem.clone()),
            None => check_field(f),
        };
        let outcome = checked.map(|_| {
            next.edits.fields.entry(f.line.clone()).or_default().insert(f.field, f.to.trim().to_string());
        });
        done(&mut results, &f.line, "field", outcome);
    }
    for f in &fixes.include {
        let outcome = line(&f.line, f.line_version).and_then(|l| {
            if let (true, Status::LeftOut { reason, .. }) = (f.include, &l.status) {
                if !reason.includable() {
                    return Err(t!("This line can't be included."));
                }
            }
            if f.include {
                next.edits.include.insert(f.line.clone());
            } else {
                next.edits.include.remove(&f.line);
            }
            Ok(())
        });
        done(&mut results, &f.line, "include", outcome);
    }
    for f in &fixes.revert {
        let outcome = line(&f.line, f.line_version).map(|_| {
            next.edits.cells.remove(&f.line);
            next.edits.fields.remove(&f.line);
            next.edits.include.remove(&f.line);
        });
        done(&mut results, &f.line, "revert", outcome);
    }
    if let Some(structure) = &fixes.structure {
        // A new way to read the file comes with the fixes made for it, or not at all.
        let problem = if results.iter().any(|r| !r.applied) {
            Some(t!("Nothing was changed: a fix that comes with it no longer fits."))
        } else {
            split_already(b, account, &read, structure)
        };
        if let Some(problem) = problem {
            let mut results: Vec<FixResult> = results.into_iter().map(|r| FixResult { applied: false, problem: r.problem.or(Some(problem.clone())), ..r }).collect();
            results.insert(0, FixResult { line: None, fix: "structure", applied: false, problem: Some(problem) });
            return Ok(Applied { session: session.clone(), results });
        }
        next.structure = structure.clone();
        results.insert(0, FixResult { line: None, fix: "structure", applied: true, problem: None });
    }
    for id in touched {
        *next.versions.entry(id).or_default() += 1;
    }
    if results.iter().any(|r| r.applied) {
        next.revision += 1;
    }
    Ok(Applied { session: next, results })
}
/// What would become of changes to an import session, without making them.
pub fn check_fixes(b: &Budget, account: &str, content: &str, session: &Session, fixes: &Fixes) -> Result<Vec<FixResult>, String> {
    apply_fixes(b, account, content, session, fixes).map(|a| a.results)
}
/// A batch to be imported as one entry whose payments were imported one by one already.
fn split_already(b: &Budget, account: &str, read: &Reading, structure: &Structure) -> Option<String> {
    let before: HashSet<&String> = read.statement.structure.whole_batches.iter().collect();
    let added: Vec<&String> = structure.whole_batches.iter().filter(|e| !before.contains(e)).collect();
    if added.is_empty() {
        return None;
    }
    let imported = bank_import::import_ids(b);
    let (who, named) = identities(account, &read.statement.lines, &read.extra, &read.batches);
    read.batches.iter().zip(&named).find_map(|(batch, name)| {
        let taken = added.contains(&&batch.entry)
            && (name.as_ref().is_some_and(|n| imported.contains(&format!("camt-batch:{n}")))
                || batch.payments.iter().any(|&l| who[l].as_ref().is_some_and(|w| imported.contains(&bank_import::bank_id_of(account, &w.signature)))));
        taken.then(|| t!("These payments were imported one by one already: they can't be imported as one entry."))
    })
}

/// What the assistant is told about an import: enough to fix its lines, never more of the file
/// than that.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub scope: Scope,
    pub revision: u64,
    pub structure: Structure,
    pub format: String,
    pub headers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<usize>,
    pub counts: Counts,
    /// Lines that need attention, the first 30 of them.
    pub lines: Vec<BriefLine>,
    /// How many more need attention.
    pub more: usize,
    /// A few lines that read, to show what the file is like.
    pub samples: Vec<BriefLine>,
    /// Lines left out that someone could include (pending, declined, another currency…), the
    /// first 20 of them: the user may ask for one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<BriefLine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_order: Option<DateQuestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signs: Option<SignQuestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Batches whose payments don't add up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub batches: Vec<BatchInfo>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BriefLine {
    pub id: String,
    pub version: u64,
    /// Cut to 80 characters.
    pub cells: Vec<String>,
    pub fields: Fields,
    pub status: Status,
}
impl Statement {
    /// The lines that don't read, and those someone changed (to undo), for a phone that shows only
    /// those: a batch keeps the lines of its that are left.
    pub fn problems(mut self) -> Statement {
        let shown = |l: &Line| l.status != Status::Ready || l.fixed || l.included;
        let kept: Vec<usize> = (0..self.lines.len()).filter(|&i| shown(&self.lines[i])).collect();
        let at: HashMap<usize, usize> = kept.iter().enumerate().map(|(new, &old)| (old, new)).collect();
        for b in &mut self.batches {
            b.line = b.line.and_then(|l| at.get(&l).copied());
            b.payments = b.payments.iter().filter_map(|l| at.get(l).copied()).collect();
        }
        let mut i = 0;
        self.lines.retain(|_| {
            i += 1;
            at.contains_key(&(i - 1))
        });
        self
    }
    /// What the assistant is told: memos only when they're shared with it.
    pub fn brief(&self, share_memos: bool) -> Brief {
        let cut = |c: &str| if c.chars().count() > 80 { c.chars().take(79).chain(['…']).collect() } else { c.to_string() };
        let line = |l: &Line| BriefLine {
            id: l.id.clone(),
            version: l.version,
            cells: l.cells.iter().enumerate().map(|(i, c)| if !share_memos && Some(i) == self.memo_column { String::new() } else { cut(c) }).collect(),
            fields: Fields { memo: if share_memos { l.fields.memo.clone() } else { String::new() }, ..l.fields.clone() },
            status: l.status.clone(),
        };
        let attention: Vec<&Line> = self.lines.iter().filter(|l| matches!(l.status, Status::Attention { .. })).collect();
        Brief {
            scope: self.scope.clone(),
            revision: self.revision,
            structure: self.structure.clone(),
            format: self.format.clone(),
            headers: self.headers.clone(),
            sources: self.sources.clone(),
            source: self.source,
            counts: self.counts.clone(),
            lines: attention.iter().take(30).map(|l| line(l)).collect(),
            more: attention.len().saturating_sub(30),
            samples: self.lines.iter().filter(|l| l.status == Status::Ready).take(5).map(line).collect(),
            left_out: self.lines.iter().filter(|l| matches!(l.status, Status::LeftOut { reason, .. } if reason.includable())).take(20).map(line).collect(),
            date_order: self.date_order.clone(),
            signs: self.signs.clone(),
            currency: self.currency.clone(),
            batches: self.batches.iter().filter(|b| b.adds_up == Some(false)).cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank_import::{apply, bank_id_of, preview, Choice};
    use crate::domain::tests::*;

    fn eur() -> Budget {
        let mut b = sample();
        b.currency = "EUR".into();
        b
    }
    fn csv() -> Structure {
        Structure { format: "csv".into(), date_order: DateOrder { order: "ymd".into(), confirmed: false }, ..Structure::default() }
    }
    fn open(content: &str, structure: Structure) -> Session {
        Session::new("s1", "budget", "cash", content, structure)
    }
    fn fixes(s: &Session, change: impl FnOnce(&mut Fixes)) -> Fixes {
        let mut f = Fixes { scope: s.scope.clone(), base_revision: s.revision, base_structure: s.structure.clone(), ..Fixes::default() };
        change(&mut f);
        f
    }
    fn cell(line: &str, version: u64, column: usize, from: &str, to: &str) -> CellFix {
        CellFix { line: line.into(), line_version: version, column, from: from.into(), to: to.into() }
    }
    fn field(line: &str, version: u64, field: Field, from: &str, to: &str) -> FieldFix {
        FieldFix { line: line.into(), line_version: version, field, from: from.into(), to: to.into() }
    }
    fn applied(b: &Budget, content: &str, s: &Session, f: &Fixes) -> Session {
        let a = apply_fixes(b, "cash", content, s, f).unwrap();
        assert!(a.results.iter().all(|r| r.applied), "{:?}", a.results);
        a.session
    }
    /// Each row: its ID, date, amount, payee, and whether it's imported already.
    fn rows(b: &Budget, content: &str, s: &Session) -> Vec<(String, String, i64, String, bool)> {
        let p = preview_statement(b, "cash", content, s).unwrap();
        p.review.items.into_iter().map(|i| (i.row.id, i.row.date, i.row.amount, i.row.payee, i.row.duplicate)).collect()
    }
    fn import(b: Budget, content: &str, s: &Session) -> Budget {
        let p = preview_statement(&b, "cash", content, s).unwrap();
        let choices = p.review.items.into_iter().map(|i| Choice { row: i.row, action: "new".into(), match_id: None }).collect();
        apply(b, "cash", choices).unwrap()
    }
    fn today(b: &Budget, content: &str, format: &str) -> Vec<String> {
        preview(b, "cash", content, format, "ymd", false).unwrap().into_iter().map(|r| r.id).collect()
    }
    const CAMT: &str = r#"<Document><BkToCstmrStmt><Stmt><Acct><Id><IBAN>DE00123</IBAN></Id><Ccy>EUR</Ccy></Acct>
<Ntry><Amt Ccy="EUR">64.20</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>BOOK</Sts><BookgDt><Dt>2026-10-01</Dt></BookgDt><AcctSvcrRef>REF-1</AcctSvcrRef>
 <NtryDtls><TxDtls><RltdPties><Cdtr><Nm>Stadtwerke</Nm></Cdtr></RltdPties></TxDtls></NtryDtls></Ntry>
<Ntry><Amt Ccy="EUR">300.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts><BookgDt><Dt>2026-10-03</Dt></BookgDt><AcctSvcrRef>BATCH</AcctSvcrRef>
 <NtryDtls>
  <TxDtls><Refs><EndToEndId>A</EndToEndId></Refs><AmtDtls><TxAmt><Amt Ccy="EUR">100.00</Amt></TxAmt></AmtDtls><RltdPties><Dbtr><Nm>Anna</Nm></Dbtr></RltdPties></TxDtls>
  <TxDtls><Refs><EndToEndId>B</EndToEndId></Refs><AmtDtls><TxAmt><Amt Ccy="EUR">200.00</Amt></TxAmt></AmtDtls><RltdPties><Dbtr><Nm>Ben</Nm></Dbtr></RltdPties></TxDtls>
 </NtryDtls></Ntry>
</Stmt></BkToCstmrStmt></Document>"#;

    #[test]
    fn fixes_land_only_on_the_import_and_lines_they_were_made_for() {
        let b = eur();
        let file = "Date,Payee,Amount\n2026-10-01,Bakery,-3.20\n2026-10-02,Grocer,n/a\n2026-10-03,Lunch,-8.50\n";
        let s = open(file, csv());
        let lines = statement(&b, "cash", file, &s).unwrap().lines;
        let grocer = lines[1].id.clone();
        assert!(matches!(lines[1].status, Status::Attention { field: Field::Amount, column: Some(2), .. }));
        // Another import's fix is refused whole.
        let repair = fixes(&s, |f| f.cells.push(cell(&grocer, 0, 2, "n/a", "-12.40")));
        let mut elsewhere = repair.clone();
        elsewhere.scope.session = "s2".into();
        assert_eq!(apply_fixes(&b, "cash", file, &s, &elsewhere).unwrap_err(), "This fix is for another import.");
        // As is an import of another file.
        assert!(statement(&b, "cash", &file.replace("Lunch", "Dinner"), &s).is_err());
        // A cell that's no longer what the fix was made from is left alone.
        let stale = fixes(&s, |f| f.cells.push(cell(&grocer, 0, 2, "12", "-12.40")));
        let a = apply_fixes(&b, "cash", file, &s, &stale).unwrap();
        assert_eq!((a.results[0].applied, a.session.revision), (false, 0));
        assert_eq!(a.results[0].problem.as_deref(), Some("This cell changed since this fix was made."));
        let fixed = applied(&b, file, &s, &repair);
        assert_eq!((fixed.revision, fixed.versions[&grocer]), (1, 1));
        let after = statement(&b, "cash", file, &fixed).unwrap();
        assert_eq!((after.lines[1].status.clone(), after.lines[1].fields.amount.as_str()), (Status::Ready, "-12.40"));
        assert_eq!(after.lines[1].original, ["2026-10-02", "Grocer", "n/a"]);
        // Made again, it finds the line changed since.
        let again = apply_fixes(&b, "cash", file, &fixed, &repair).unwrap();
        assert_eq!(again.results[0].problem.as_deref(), Some("This line changed since this fix was made."));
        // The bank's ID for a line stays as the file has it.
        let ids = "Date,Payee,Amount,ID\n2026-10-01,Bakery,-3.20,A1\n";
        let with_ids = open(ids, csv());
        let id = statement(&b, "cash", ids, &with_ids).unwrap().lines[0].id.clone();
        let a = apply_fixes(&b, "cash", ids, &with_ids, &fixes(&with_ids, |f| f.cells.push(cell(&id, 0, 3, "A1", "A2")))).unwrap();
        assert_eq!(a.results[0].problem.as_deref(), Some("The bank's ID for a line can't be changed."));

        // An old proposal to include a line, after someone included it and left it out again.
        let pending = "Date,Payee,Amount,Status\n2026-10-01,Bakery,-3.20,Booked\n2026-10-02,Cinema,-12.00,Pending\n";
        let s = open(pending, csv());
        let cinema = statement(&b, "cash", pending, &s).unwrap().lines[1].id.clone();
        let proposal = fixes(&s, |f| f.include.push(IncludeFix { line: cinema.clone(), line_version: 0, include: true }));
        let s1 = applied(&b, pending, &s, &proposal);
        assert_eq!(statement(&b, "cash", pending, &s1).unwrap().lines[1].status, Status::Ready);
        let s2 = applied(&b, pending, &s1, &fixes(&s1, |f| f.include.push(IncludeFix { line: cinema.clone(), line_version: 1, include: false })));
        let late = apply_fixes(&b, "cash", pending, &s2, &proposal).unwrap();
        assert!(!late.results[0].applied);
        assert!(matches!(statement(&b, "cash", pending, &late.session).unwrap().lines[1].status, Status::LeftOut { reason: Reason::Pending, .. }));

        // A new way to read the file, from one that's no longer how it reads, is refused whole;
        // a cell fix made while it read that way is left alone.
        let s = open(file, csv());
        let flip = fixes(&s, |f| f.structure = Some(Structure { flip_signs: true, ..s.structure.clone() }));
        let flipped = applied(&b, file, &s, &flip);
        let old = fixes(&s, |f| f.structure = Some(Structure { date_order: DateOrder { order: "dmy".into(), confirmed: true }, ..s.structure.clone() }));
        assert_eq!(apply_fixes(&b, "cash", file, &flipped, &old).unwrap_err(), "The import changed since this fix was made.");
        let a = apply_fixes(&b, "cash", file, &flipped, &repair).unwrap();
        assert_eq!(a.results[0].problem.as_deref(), Some("The file reads another way since this fix was made."));
        // A new way to read it with fixes made for it: all of them or nothing.
        let both = fixes(&flipped, |f| {
            f.structure = Some(Structure { flip_signs: false, ..flipped.structure.clone() });
            f.cells.push(cell(&grocer, 0, 2, "n/a", "12.40"));
            f.cells.push(cell(&grocer, 0, 0, "wrong", "2026-10-02"));
        });
        let a = apply_fixes(&b, "cash", file, &flipped, &both).unwrap();
        assert_eq!(a.session, flipped);
        assert!(a.results.iter().all(|r| !r.applied));
        let both = fixes(&flipped, |f| {
            f.structure = Some(Structure { flip_signs: false, ..flipped.structure.clone() });
            f.cells.push(cell(&grocer, 0, 2, "n/a", "-12.40"));
        });
        let s = applied(&b, file, &flipped, &both);
        assert_eq!(statement(&b, "cash", file, &s).unwrap().lines.iter().map(|l| l.fields.amount.as_str()).collect::<Vec<_>>(), ["-3.20", "-12.40", "-8.50"]);
    }

    #[test]
    fn purchases_written_positive_and_amounts_given() {
        let b = eur();
        let file = "Date,Payee,Amount\n2026-10-01,Bakery,3.20\n2026-10-02,Grocer,n/a\n2026-10-03,Refund,-8.50\n";
        let s = open(file, Structure { flip_signs: true, ..csv() });
        let grocer = statement(&b, "cash", file, &s).unwrap().lines[1].id.clone();
        // An amount given is what moved, whatever the file's signs.
        let s = applied(&b, file, &s, &fixes(&s, |f| f.fields.push(field(&grocer, 0, Field::Amount, "n/a", "-12.40"))));
        let amounts: Vec<i64> = rows(&b, file, &s).into_iter().map(|r| r.2).collect();
        assert_eq!(amounts, [-320, -1240, 850]);
        // Another currency, included, needs the amount in the budget's.
        let mixed = "Date,Payee,Amount,Currency\n2026-10-01,Bakery,-3.20,EUR\n2026-10-02,London,-9.99,GBP\n";
        let s = open(mixed, csv());
        let london = statement(&b, "cash", mixed, &s).unwrap().lines[1].id.clone();
        let s = applied(&b, mixed, &s, &fixes(&s, |f| f.include.push(IncludeFix { line: london.clone(), line_version: 0, include: true })));
        assert!(matches!(statement(&b, "cash", mixed, &s).unwrap().lines[1].status, Status::Attention { field: Field::Amount, .. }));
        let s = applied(&b, mixed, &s, &fixes(&s, |f| f.fields.push(field(&london, 1, Field::Amount, "-9.99", "-11.60"))));
        assert_eq!(rows(&b, mixed, &s)[1].2, -1160);
        // Nothing that reads, and lines to fix: no error, an empty review.
        let none = "Date,Payee,Amount\n2026-10-01,Bakery,n/a\n";
        let p = preview_statement(&b, "cash", none, &open(none, csv())).unwrap();
        assert_eq!((p.review.items.len(), p.statement.counts.attention), (0, 1));
    }

    #[test]
    fn lines_that_read_keep_the_ids_todays_import_gives_them() {
        let b = eur();
        // A card check, and a correction, both with the purchase's FITID, before it: it's fit:X
        // as today.
        let ofx = "<OFX><CURDEF>EUR<BANKTRANLIST>\
<STMTTRN><DTPOSTED>20261001<TRNAMT>0.00<FITID>X<NAME>Card check</STMTTRN>\
<STMTTRN><DTPOSTED>20261001<TRNAMT>-9.00<FITID>X<CORRECTFITID>Y<CORRECTACTION>DELETE</STMTTRN>\
<STMTTRN><DTPOSTED>20261002<TRNAMT>-5.00<FITID>X<NAME>Purchase</STMTTRN></BANKTRANLIST></OFX>";
        let s = open(ofx, Structure { format: "ofx".into(), ..Structure::default() });
        let ids: Vec<String> = rows(&b, ofx, &s).into_iter().map(|r| r.0).collect();
        assert_eq!(ids, today(&b, ofx, "ofx"));
        assert_eq!(ids, [bank_id_of("cash", "fit:X")]);
        // A row with no date yet and a pending one, both with Café's ID, before it: Café is fit:X
        // as today; included and given a date, they take the places after it.
        let file = "Date,Payee,Amount,ID,Status\nn/a,Card hold,-4.00,X,Booked\n2026-10-02,Cinema,-12.00,X,Pending\n2026-10-03,Café,-3.50,X,Booked\n";
        let s = open(file, csv());
        assert_eq!(rows(&b, file, &s).into_iter().map(|r| r.0).collect::<Vec<_>>(), today(&b, file, "csv"));
        let lines = statement(&b, "cash", file, &s).unwrap().lines;
        let (hold, cinema) = (lines[0].id.clone(), lines[1].id.clone());
        let s = applied(&b, file, &s, &fixes(&s, |f| {
            f.include.push(IncludeFix { line: hold.clone(), line_version: 0, include: true });
            f.include.push(IncludeFix { line: cinema.clone(), line_version: 0, include: true });
            f.fields.push(field(&hold, 0, Field::Date, "n/a", "2026-10-01"));
        }));
        let got: Vec<(String, String)> = rows(&b, file, &s).into_iter().map(|r| (r.3, r.0)).collect();
        assert_eq!(got, [
            ("Card hold".to_string(), bank_id_of("cash", "fit:X:2")),
            ("Cinema".to_string(), bank_id_of("cash", "fit:X:3")),
            ("Café".to_string(), bank_id_of("cash", "fit:X")),
        ]);
    }

    #[test]
    fn a_repaired_file_imports_as_the_clean_one_does() {
        // Two transactions with one bank ID (Rent, then Café), Rent's amount damaged.
        let clean = "Date,Payee,Amount,ID\n2026-10-01,Rent,-500.00,X\n2026-10-02,Café,-3.50,X\n2026-10-03,Lunch,-8.50,Y\n";
        let broken = clean.replace("-500.00", "-5OO.00");
        let b = eur();
        let before = b.transactions.len();
        let want = rows(&b, clean, &open(clean, csv()));
        // Partly imported: Café keeps its place as the second X.
        let s = open(&broken, csv());
        assert_eq!(rows(&b, &broken, &s), want[1..]);
        let b = import(b, &broken, &s);
        // Repaired, Rent imports as itself; the others are found imported.
        let rent = statement(&b, "cash", &broken, &s).unwrap().lines[0].id.clone();
        let repaired = applied(&b, &broken, &s, &fixes(&s, |f| f.cells.push(cell(&rent, 0, 2, "-5OO.00", "-500.00"))));
        let got = rows(&b, &broken, &repaired);
        assert_eq!(got.iter().map(|r| (&r.0, r.4)).collect::<Vec<_>>(), [(&want[0].0, false), (&want[1].0, true), (&want[2].0, true)]);
        let b = import(b, &broken, &repaired);
        // The broken file again finds Rent imported, as does the clean one.
        let again = open(&broken, csv());
        let st = statement(&b, "cash", &broken, &again).unwrap();
        assert!(matches!(st.lines[0].status, Status::LeftOut { reason: Reason::Imported, .. }));
        assert!(rows(&b, &broken, &again).iter().all(|r| r.4));
        assert!(rows(&b, clean, &open(clean, csv())).iter().all(|r| r.4));
        assert!(preview(&b, "cash", clean, "csv", "ymd", false).unwrap().iter().all(|r| r.duplicate));
        assert_eq!(b.transactions.len(), before + 3);

        // Identical rows without the bank's ID, the first damaged: they're interchangeable, so
        // the repaired file gives the clean file's IDs.
        let clean = "Date,Payee,Amount\n2026-10-01,Coffee,-3.00\n2026-10-01,Coffee,-3.00\n2026-10-02,Bread,-2.00\n";
        let broken = clean.replacen("-3.00", "-3.0O", 1);
        let b = eur();
        let want: BTreeSet<String> = rows(&b, clean, &open(clean, csv())).into_iter().map(|r| r.0).collect();
        let s = open(&broken, csv());
        let b = import(b, &broken, &s);
        let coffee = statement(&b, "cash", &broken, &s).unwrap().lines[0].id.clone();
        let repaired = applied(&b, &broken, &s, &fixes(&s, |f| f.fields.push(field(&coffee, 0, Field::Amount, "-3.0O", "-3.00"))));
        let got: BTreeSet<String> = rows(&b, &broken, &repaired).into_iter().map(|r| r.0).collect();
        assert_eq!(got, want);
        let b = import(b, &broken, &repaired);
        assert!(rows(&b, clean, &open(clean, csv())).iter().all(|r| r.4));
    }

    #[test]
    fn a_batch_is_never_imported_both_whole_and_as_its_payments() {
        let camt = Structure { format: "camt053".into(), ..Structure::default() };
        let whole = bank_id_of("cash", "fit:2026-10-03/30000/BATCH");
        // Payments that don't add up: today's import reads the entry whole.
        let off = CAMT.replace(">200.00<", ">250.00<");
        let b = eur();
        let rows_today = preview(&b, "cash", &off, "camt053", "ymd", false).unwrap();
        assert_eq!(rows_today[1].id, whole);
        let b = apply(b, "cash", rows_today.into_iter().map(|row| Choice { row, action: "new".into(), match_id: None }).collect()).unwrap();
        // Its payments, once they add up, are found imported as the one entry, and aren't saved.
        let s = open(&off, camt.clone());
        let ben = statement(&b, "cash", &off, &s).unwrap().lines.iter().find(|l| l.id == "e2.2").unwrap().id.clone();
        let s = applied(&b, &off, &s, &fixes(&s, |f| f.fields.push(field(&ben, 0, Field::Amount, "250.00", "200.00"))));
        let got = rows(&b, &off, &s);
        assert_eq!(got.iter().map(|r| (r.2, r.4)).collect::<Vec<_>>(), [(-6420, true), (10000, true), (20000, true)]);
        let count = b.transactions.len();
        assert_eq!(import(b, &off, &s).transactions.len(), count);

        // Payments imported one by one: the entry can't be imported as one after that.
        let b = eur();
        let s = open(CAMT, camt.clone());
        let b = import(b, CAMT, &s);
        let as_one = fixes(&s, |f| f.structure = Some(Structure { whole_batches: vec!["e2".into()], ..camt.clone() }));
        let a = apply_fixes(&b, "cash", CAMT, &s, &as_one).unwrap();
        assert_eq!(a.results[0].problem.as_deref(), Some("These payments were imported one by one already: they can't be imported as one entry."));
        // Nor saved as one, from a review made before they were.
        let fresh = eur();
        let one = applied(&fresh, CAMT, &s, &as_one);
        let p = preview_statement(&fresh, "cash", CAMT, &one).unwrap();
        let row = p.review.items.into_iter().map(|i| i.row).find(|r| r.amount == 30000).unwrap();
        assert_eq!((row.id.as_str(), row.batch.as_deref()), (whole.as_str(), Some(whole.as_str())));
        let count = b.transactions.len();
        assert_eq!(apply(b, "cash", vec![Choice { row, action: "new".into(), match_id: None }]).unwrap().transactions.len(), count);
    }

    #[test]
    fn an_import_starts_as_the_accounts_last_file_read_and_keeps_what_it_used() {
        let mut b = eur();
        let file = "Date,Payee,Amount\n01/02/2026,Shop,-3.00\n03/04/2026,Cafe,-2.00\n";
        let s = start(&b, "cash", file, "s1", "budget", None).unwrap();
        // Nothing kept, and dates that don't settle their order: it's asked.
        assert!(statement(&b, "cash", file, &s).unwrap().date_order.unwrap().unconfirmed);
        let day_first = Structure { date_order: DateOrder { order: "dmy".into(), confirmed: true }, ..s.structure.clone() };
        let s = applied(&b, file, &s, &fixes(&s, |f| f.structure = Some(day_first)));
        let st = statement(&b, "cash", file, &s).unwrap();
        assert_eq!((st.remember.date_format.as_deref(), st.remember.date_confirmed), (Some("dmy"), Some(true)));
        bank_csv::remember(&mut b, "cash", st.remember).unwrap();
        // The next file starts with it, and doesn't ask.
        let next = file.replace("Shop", "Bakery");
        let s = start(&b, "cash", &next, "s2", "budget", None).unwrap();
        assert_eq!(s.structure.date_order, DateOrder { order: "dmy".into(), confirmed: true });
        assert!(!statement(&b, "cash", &next, &s).unwrap().date_order.unwrap().unconfirmed);

        // Columns chosen by a name two columns share aren't kept by that name; others are.
        let twice = "Date,Text,Text,Amount\n2026-10-01,Shop,Note,-3.00\n";
        let columns = Columns { date: Some("Date".into()), payee: Some("Text".into()), amount: Some("Amount".into()), ..Columns::default() };
        let st = statement(&b, "cash", twice, &open(twice, Structure { columns: Some(columns.clone()), ..csv() })).unwrap();
        assert_eq!(st.remember.columns, None);
        let once = "Date,Text,Note,Amount\n2026-10-01,Shop,Note,-3.00\n";
        let st = statement(&b, "cash", once, &open(once, Structure { columns: Some(columns.clone()), ..csv() })).unwrap();
        assert_eq!(st.remember.columns, Some(columns));
        assert_eq!(st.header_line, Some(0));

        // A file with two accounts: the one read is kept for the next.
        let both = "Account,Date,Payee,Amount\nA1,2026-10-01,Shop,-3.00\nA1,2026-10-02,Cafe,-2.00\nB2,2026-10-03,Rent,-500.00\nB2,2026-10-04,Pay,900.00\n";
        let s = start(&b, "cash", both, "s3", "budget", None).unwrap();
        let st = statement(&b, "cash", both, &s).unwrap();
        assert_eq!(st.sources.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["A1", "B2"]);
        let second = Structure { source: Some("B2".into()), ..s.structure.clone() };
        let s = applied(&b, both, &s, &fixes(&s, |f| f.structure = Some(second)));
        let st = statement(&b, "cash", both, &s).unwrap();
        assert_eq!(st.remember.source.as_deref(), Some("B2"));
        bank_csv::remember(&mut b, "cash", st.remember).unwrap();
        let s = start(&b, "cash", both, "s4", "budget", None).unwrap();
        assert_eq!(statement(&b, "cash", both, &s).unwrap().source, Some(1));
    }

    #[test]
    fn a_cards_purchases_written_positive_are_turned_unless_an_amount_is_in_doubt() {
        let b = eur();
        let file = "Date,Payee,Amount\n2026-10-01,Shop,3.00\n2026-10-02,Cafe,2.50\n2026-10-03,Bakery,4.20\n2026-10-04,Payment,-50.00\n";
        let s = start(&b, "card", file, "s1", "budget", None).unwrap();
        assert!(s.structure.flip_signs);
        let damaged = file.replace("2.50", "2.5O");
        assert!(!start(&b, "card", &damaged, "s2", "budget", None).unwrap().structure.flip_signs);
        // Not for an account that isn't a card.
        assert!(!start(&b, "cash", file, "s3", "budget", None).unwrap().structure.flip_signs);
    }

    #[test]
    fn a_file_the_assistant_prepared_reads_the_standard_way_whatever_the_account_kept() {
        let mut b = eur();
        let kept = FileSettings { date_format: Some("dmy".into()), date_confirmed: Some(true), flip_signs: Some(true), ..FileSettings::default() };
        bank_csv::remember(&mut b, "card", kept).unwrap();
        let file = "Date,Payee,Memo,Amount\n2026-10-01,Shop,,-3.00\n2026-10-02,Refund,,2.50\n";
        // As the account's own file, its signs would be turned.
        assert!(start(&b, "card", file, "s1", "budget", None).unwrap().structure.flip_signs);
        let s = start_prepared(file, "card", "s2", "budget");
        let st = statement(&b, "card", file, &s).unwrap();
        assert_eq!(st.lines.iter().map(|l| l.fields.amount.as_str()).collect::<Vec<_>>(), ["-3.00", "2.50"]);
        assert!(st.date_order.is_none_or(|d| !d.unconfirmed) && st.signs.is_none());
    }

    #[test]
    fn a_file_whose_columns_arent_found_shows_its_lines_to_choose_them() {
        let b = eur();
        // Two columns of unsigned amounts: which is the transaction's can't be told.
        let file = "Konto 4711\nDatum;Text;A;B\n15.10.2026;Shop;3,20;100,00\n16.10.2026;Kino;12,00;88,00\n";
        let s = open(file, Structure { date_order: DateOrder { order: "dmy".into(), confirmed: false }, ..csv() });
        let st = statement(&b, "cash", file, &s).unwrap();
        assert_eq!(st.headers, ["Datum", "Text", "A", "B"]);
        assert_eq!(st.lines.iter().map(|l| l.cells.len()).collect::<Vec<_>>(), [1, 4, 4]);
        assert!(matches!(st.lines[0].status, Status::LeftOut { reason: Reason::AboveHeader, .. }));
        assert!(matches!(&st.lines[1].status, Status::Attention { reason, .. } if reason.contains("Choose the columns")), "{:?}", st.lines[1].status);
        // Chosen, the columns read.
        let columns = Columns { date: Some("Datum".into()), payee: Some("Text".into()), outflow: Some("A".into()), ..Columns::default() };
        let s = applied(&b, file, &s, &fixes(&s, |f| f.structure = Some(Structure { columns: Some(columns), ..s.structure.clone() })));
        let st = statement(&b, "cash", file, &s).unwrap();
        assert_eq!(st.counts.ready, 2);
        assert_eq!(st.lines[2].fields.amount, "-12.00");
        assert_eq!(st.remember.columns.as_ref().and_then(|c| c.outflow.as_deref()), Some("A"));
    }

    #[test]
    fn what_the_file_says_of_a_line_holds_after_it_is_fixed() {
        let b = eur();
        // A line of another account's, its date fixed, is still that account's.
        let file = "Account,Date,Payee,Amount\nA,2026-10-01,Shop A,-1\nA,2026-10-02,Shop A2,-2\nB,2026-10-03,Shop B,-3\nB,2026-10-04,Shop B2,-4\nB,n/a,Missed B,-5\n";
        let s = open(file, csv());
        let missed = statement(&b, "cash", file, &s).unwrap().lines[4].clone();
        assert!(matches!(missed.status, Status::LeftOut { reason: Reason::OtherAccount, .. }));
        let s = applied(&b, file, &s, &fixes(&s, |f| f.cells = vec![cell(&missed.id, 0, 1, "n/a", "2026-10-05")]));
        assert_eq!(rows(&b, file, &s).len(), 2);

        // A fee in the other currency doesn't come off an amount written over in the budget's.
        let file = "Date,Payee,Amount,Fee,Currency\n2026-10-01,Bakery,-3.20,0,EUR\n2026-10-02,London,-9.99,1.00,GBP\n";
        let s = open(file, csv());
        let london = statement(&b, "cash", file, &s).unwrap().lines[1].id.clone();
        let s = applied(&b, file, &s, &fixes(&s, |f| f.include = vec![IncludeFix { line: london.clone(), line_version: 0, include: true }]));
        let s = applied(&b, file, &s, &fixes(&s, |f| f.cells = vec![cell(&london, 1, 2, "-9.99", "-11.60")]));
        assert_eq!(rows(&b, file, &s)[1].2, -1160);

        // Some columns chosen (an amount column, say): the others as the file names them.
        let file = "Date,Payee,Amount,Correct Amount\n2026-10-01,Shop,-10.00,-12.00\n";
        let s = open(file, csv());
        let only = Columns { amount: Some("Correct Amount".into()), ..Columns::default() };
        let s = applied(&b, file, &s, &fixes(&s, |f| f.structure = Some(Structure { columns: Some(only), ..s.structure.clone() })));
        assert_eq!(rows(&b, file, &s)[0].2, -1200);

        // Two columns with one name: the one chosen is known by its place.
        let file = "Date,Payee,Amount,Amount\n2026-10-01,Shop,-10.00,-12.00\n";
        let s = open(file, csv());
        let st = statement(&b, "cash", file, &s).unwrap();
        assert_eq!(st.columns.as_ref().unwrap().amount.as_deref(), Some("#3"));
        let second = Columns { amount: Some("#4".into()), ..st.columns.clone().unwrap() };
        let s = applied(&b, file, &s, &fixes(&s, |f| f.structure = Some(Structure { columns: Some(second), ..s.structure.clone() })));
        assert_eq!(rows(&b, file, &s)[0].2, -1200);
        // A column known only by its place isn't kept for the next file.
        assert_eq!(statement(&b, "cash", file, &s).unwrap().remember.columns, None);
        // A column named like a place doesn't take another's choice.
        let file = "Date,#4,Amount,Amount
2026-10-01,Shop,-10.00,-12.00
";
        let s = open(file, csv());
        let second = Columns { date: Some("Date".into()), payee: Some("#4".into()), amount: Some("##4".into()), ..Columns::default() };
        let s = applied(&b, file, &s, &fixes(&s, |f| f.structure = Some(Structure { columns: Some(second), ..s.structure.clone() })));
        assert_eq!((rows(&b, file, &s)[0].2, rows(&b, file, &s)[0].3.as_str()), (-1200, "Shop"));
    }

    #[test]
    fn a_batch_is_known_the_same_whatever_other_accounts_the_file_holds() {
        let b = eur();
        // A batch whose payments don't add up, imported as its one entry.
        let stmt = |account: &str| {
            let body = CAMT.split("<Stmt>").nth(1).unwrap().split("</Stmt>").next().unwrap();
            body.replace("DE00123", account).replace(">200.00<", ">250.00<")
        };
        let alone = format!("<Document><BkToCstmrStmt><Stmt>{}</Stmt></BkToCstmrStmt></Document>", stmt("B"));
        let both = format!("<Document><BkToCstmrStmt><Stmt>{}</Stmt><Stmt>{}</Stmt></BkToCstmrStmt></Document>", stmt("A"), stmt("B"));
        let whole = |entry: &str| Structure { format: "camt053".into(), source: Some("B".into()), whole_batches: vec![entry.into()], ..Structure::default() };
        let imported = import(b, &alone, &open(&alone, whole("e2")));
        // The same account's statement again, in a file whose other account has the same batch.
        let again = rows(&imported, &both, &open(&both, whole("e4")));
        assert_eq!(again.len(), 2);
        assert!(again.iter().all(|r| r.4), "{again:?}");
    }

    #[test]
    fn a_cell_typed_in_the_grid_is_what_the_line_reads() {
        let b = eur();
        let at = |s: &Session, file: &str, i: usize| statement(&b, "cash", file, s).unwrap().lines[i].clone();
        let cell = |l: &Line, column: usize, to: &str, s: &Session| CellFix {
            line: l.id.clone(),
            line_version: s.versions.get(&l.id).copied().unwrap_or(0),
            column,
            from: l.cells.get(column).cloned().unwrap_or_default(),
            to: to.into(),
        };
        // A line in another currency, included: its amount typed in is in the budget's.
        let file = "Date,Payee,Amount,Currency\n2026-10-01,Bakery,-3.20,EUR\n2026-10-02,London,-9.99,GBP\n";
        let s = open(file, csv());
        let london = at(&s, file, 1);
        let s = applied(&b, file, &s, &fixes(&s, |f| f.include = vec![IncludeFix { line: london.id.clone(), line_version: 0, include: true }]));
        assert!(matches!(at(&s, file, 1).status, Status::Attention { field: Field::Amount, .. }));
        let s = applied(&b, file, &s, &fixes(&s, |f| f.cells = vec![cell(&at(&s, file, 1), 2, "-11.60", &s)]));
        assert_eq!((at(&s, file, 1).status, at(&s, file, 1).fields.amount), (Status::Ready, "-11.60".to_string()));

        // The latest change wins: an amount given before gives way to the cell typed after it.
        let file = "Date,Payee,Amount\n2026-10-02,Grocer,n/a\n";
        let s = open(file, csv());
        let grocer = at(&s, file, 0);
        let given = FieldFix { line: grocer.id.clone(), line_version: 0, field: Field::Amount, from: "n/a".into(), to: "-12.40".into() };
        let s = applied(&b, file, &s, &fixes(&s, |f| f.fields = vec![given]));
        let s = applied(&b, file, &s, &fixes(&s, |f| f.cells = vec![cell(&at(&s, file, 0), 2, "-13.40", &s)]));
        assert_eq!(preview_statement(&b, "cash", file, &s).unwrap().review.items[0].row.amount, -1340);

        // A line cut short has its missing cells to fill in.
        let file = "Date,Payee,Amount\n2026-10-01,Bakery,-3.20\n2026-10-02,Grocer\n";
        let s = open(file, csv());
        let short = at(&s, file, 1);
        let s = applied(&b, file, &s, &fixes(&s, |f| f.include = vec![IncludeFix { line: short.id.clone(), line_version: 0, include: true }]));
        let s = applied(&b, file, &s, &fixes(&s, |f| f.cells = vec![cell(&at(&s, file, 1), 2, "-13.40", &s)]));
        assert_eq!(at(&s, file, 1).status, Status::Ready);
        // Past the header's columns, there's no cell to fill.
        let past = apply_fixes(&b, "cash", file, &s, &fixes(&s, |f| f.cells = vec![cell(&at(&s, file, 1), 7, "x", &s)])).unwrap();
        assert_eq!(past.results[0].problem.as_deref(), Some("This line has no such column."));
    }

    #[test]
    fn a_phone_is_shown_the_lines_that_dont_read() {
        let b = eur();
        let off = CAMT.replace(">200.00<", ">250.00<");
        let st = statement(&b, "cash", &off, &open(&off, Structure { format: "camt053".into(), ..Structure::default() })).unwrap();
        assert_eq!(st.lines.len(), 3);
        let problems = st.problems();
        assert_eq!(problems.lines.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["e2.1", "e2.2"]);
        assert_eq!(problems.batches[0].payments, [0, 1]);
        assert_eq!(problems.counts.attention, 2);
        // A line fixed since still shows, to be undone.
        let file = "Date,Payee,Amount\n2026-10-01,Shop,-3.00\n2026-10-02,Cafe,n/a\n";
        let s = open(file, csv());
        let cafe = statement(&b, "cash", file, &s).unwrap().lines[1].id.clone();
        let fix = FieldFix { line: cafe.clone(), line_version: 0, field: Field::Amount, from: "n/a".into(), to: "-2.00".into() };
        let s = applied(&b, file, &s, &fixes(&s, |f| f.fields = vec![fix]));
        // Giving a line the value it has changes nothing.
        let st = statement(&b, "cash", file, &s).unwrap();
        let shop = st.lines[0].clone();
        let same = FieldFix { line: shop.id.clone(), line_version: 0, field: Field::Amount, from: "-3.00".into(), to: " -3.00".into() };
        let refused = apply_fixes(&b, "cash", file, &s, &fixes(&s, |f| f.fields = vec![same])).unwrap();
        assert_eq!(refused.results[0].problem.as_deref(), Some("The line already has that value."));
        assert_eq!(refused.session, s);
        // A cell typed back as the file writes it, spaces and all, is the file's again.
        let cell = |to: &str, s: &Session| CellFix { line: shop.id.clone(), line_version: s.versions.get(&shop.id).copied().unwrap_or(0), column: 2, from: statement(&b, "cash", file, s).unwrap().lines[0].cells[2].clone(), to: to.into() };
        let changed = applied(&b, file, &s, &fixes(&s, |f| f.cells = vec![cell("-4.00", &s)]));
        let back = applied(&b, file, &changed, &fixes(&changed, |f| f.cells = vec![cell("-3.00 ", &changed)]));
        assert!(!statement(&b, "cash", file, &back).unwrap().lines[0].fixed);
        let problems = statement(&b, "cash", file, &s).unwrap().problems();
        assert_eq!(problems.lines.iter().map(|l| (l.id.as_str(), l.fixed)).collect::<Vec<_>>(), [(cafe.as_str(), true)]);
        assert_eq!((problems.counts.ready, problems.counts.attention), (2, 0));
        // A line whose amount doesn't read gives its date as it reads, to fix the rest in the same form.
        let file = "Date,Payee,Amount\n31/10/2026,Shop,-3.00\n01/10/2026,Cafe,n/a\n";
        let st = statement(&b, "cash", file, &open(file, csv())).unwrap();
        assert_eq!((st.lines[1].fields.date.as_str(), st.lines[1].fields.amount.as_str()), ("2026-10-01", "n/a"));
    }

    #[test]
    fn the_assistant_is_told_enough_to_fix_lines_and_no_more() {
        let b = eur();
        let long = "x".repeat(120);
        let mut file = String::from("Date,Payee,Memo,Amount\n");
        for i in 0..40 {
            file += &format!("2026-10-{:02},{long},Private note {i},n/a\n", i % 28 + 1);
        }
        file += "2026-10-01,Bakery,Croissants,-3.20\n";
        let st = statement(&b, "cash", &file, &open(&file, csv())).unwrap();
        let brief = st.brief(false);
        assert_eq!((brief.lines.len(), brief.more, brief.samples.len()), (30, 10, 1));
        assert_eq!(brief.lines[0].cells[1].chars().count(), 80);
        assert_eq!((brief.lines[0].cells[2].as_str(), brief.samples[0].fields.memo.as_str()), ("", ""));
        let shared = st.brief(true);
        assert_eq!((shared.lines[0].cells[2].as_str(), shared.samples[0].fields.memo.as_str()), ("Private note 0", "Croissants"));
        // Serialized, it says what it needs to.
        let json = serde_json::to_value(&brief).unwrap();
        assert_eq!(json["lines"][0]["status"]["kind"], "attention");
        assert_eq!(json["counts"]["attention"], 40);
    }
}
