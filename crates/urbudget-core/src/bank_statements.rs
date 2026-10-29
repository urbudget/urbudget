//! Statements in the banks' own exchange formats: OFX and QFX, QIF, ISO 20022 CAMT.053 (XML) and
//! SWIFT MT940, and which format a file is in, whatever it was chosen as. Each reads as lines, one
//! for each transaction record, saying what became of it (see `bank_statement`).
use crate::bank_import::{dates, money, read_dates, BankTransaction, DateReading};
use crate::bank_statement::{Field, Fields, Reason, Status};
use crate::t;
use chrono::NaiveDate;
use std::collections::{HashMap, HashSet};

/// The format a statement is in, when its content says so: OFX, CAMT.053, MT940 or QIF.
pub fn sniff(content: &str) -> Option<&'static str> {
    let content = content.trim_start_matches('\u{feff}').trim_start();
    let head: String = content.chars().take(4096).collect::<String>().to_ascii_uppercase();
    if head.starts_with("OFXHEADER") || head.contains("<OFX>") {
        return Some("ofx");
    }
    // An account report (camt.052) reads the same way as a statement.
    if ["CAMT.053", "BKTOCSTMRSTMT>", "CAMT.052", "BKTOCSTMRACCTRPT>"].iter().any(|s| head.contains(s)) {
        return Some("camt053");
    }
    let lines = || content.lines().map(str::trim_start).take(400);
    if lines().any(|l| l.starts_with(":20:")) && lines().any(|l| l.starts_with(":61:")) {
        return Some("mt940");
    }
    let first = content.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_ascii_lowercase();
    if ["!type:", "!account", "!option:", "!clear:"].iter().any(|p| first.starts_with(p)) {
        return Some("qif");
    }
    // A QIF file some banks write without its !Type line: dated records, each ended by ^.
    let amount = |l: &str| l.len() > 1 && (l.starts_with('T') || l.starts_with('U')) && l[1..].trim_start().starts_with(|c: char| c.is_ascii_digit() || "-+.,".contains(c));
    let dated = first.starts_with('d') && first[1..].trim_start().starts_with(|c: char| c.is_ascii_digit());
    if dated && lines().any(|l| l.trim() == "^") && lines().any(|l| amount(l.trim())) {
        return Some("qif");
    }
    None
}

/// An element of an XML document, by its local name (namespace prefixes dropped).
#[derive(Default, Debug)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Node>,
}
impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// The element at a path of child names.
    fn at(&self, path: &[&str]) -> Option<&Node> {
        path.iter().try_fold(self, |n, name| n.child(name))
    }
    fn text_at(&self, path: &[&str]) -> Option<&str> {
        self.at(path).map(|n| n.text.trim()).filter(|t| !t.is_empty())
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    /// The text of this element and everything in it.
    fn deep_text(&self) -> String {
        let mut out = self.text.trim().to_string();
        for c in &self.children {
            out.push_str(&c.deep_text());
        }
        out
    }
    /// Every element below with this name, at any depth.
    fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            } else {
                c.find_all(name, out);
            }
        }
    }
}
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or(entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_string()
}
/// A forgiving reader for the XML banks write: elements, attributes, text and CDATA; comments,
/// declarations and processing instructions are passed over, and unclosed elements close at the
/// end.
fn xml(s: &str) -> Result<Node, String> {
    let mut stack = vec![Node::default()];
    let mut rest = s;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            stack.last_mut().unwrap().text.push_str(&unescape(rest));
            break;
        };
        stack.last_mut().unwrap().text.push_str(&unescape(&rest[..lt]));
        rest = &rest[lt..];
        if let Some(cdata) = rest.strip_prefix("<![CDATA[") {
            let end = cdata.find("]]>").ok_or_else(|| t!("The XML file ends inside a CDATA section."))?;
            stack.last_mut().unwrap().text.push_str(&cdata[..end]);
            rest = &cdata[end + 3..];
            continue;
        }
        if rest.starts_with("<!--") {
            rest = rest.find("-->").map_or("", |e| &rest[e + 3..]);
            continue;
        }
        let gt = rest.find('>').ok_or_else(|| t!("The XML file ends inside a tag."))?;
        let inner = &rest[1..gt];
        rest = &rest[gt + 1..];
        if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            let name = local(name.trim());
            // Close back to the matching element, tolerating ones never closed.
            if let Some(depth) = stack.iter().rposition(|n| n.name == name).filter(|&d| d > 0) {
                while stack.len() > depth {
                    let node = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(node);
                }
            }
            continue;
        }
        let empty = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let (name, mut attrs_text) = inner.split_once(|c: char| c.is_whitespace()).unwrap_or((inner, ""));
        let mut node = Node { name: local(name), ..Node::default() };
        while let Some(eq) = attrs_text.find('=') {
            let key = local(attrs_text[..eq].trim());
            let after = attrs_text[eq + 1..].trim_start();
            let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
            let Some(close) = after[1..].find(quote) else { break };
            node.attrs.push((key, unescape(&after[1..1 + close])));
            attrs_text = &after[close + 2..];
        }
        if empty {
            stack.last_mut().unwrap().children.push(node);
        } else if stack.len() > 64 {
            // Statements nest a dozen deep; this deep is a file that isn't one.
            return Err(t!("This XML file is nested too deeply to be a bank statement."));
        } else {
            stack.push(node);
        }
    }
    while stack.len() > 1 {
        let node = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(node);
    }
    Ok(stack.pop().unwrap())
}

fn iso_date(s: &str) -> Option<String> {
    let day = s.trim().get(..10)?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok().map(|d| d.format("%Y-%m-%d").to_string())
}
fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What reading a statement's lines takes besides the file: which account, and what the editor
/// was told about its lines.
#[derive(Default, Clone)]
pub(crate) struct LineOptions<'a> {
    /// The account to read in a file with several, by its label (see `Source`); else the one
    /// today's import reads, or the first in the budget's currency when that one isn't.
    pub source: Option<&'a str>,
    /// Read as today's import does: its account, and a batch whose payments don't add up to its
    /// entry, or can't all be read, as the one entry.
    pub legacy: bool,
    /// Lines to import though they're left out (where that makes sense), by key.
    pub include: HashSet<String>,
    /// Values given for lines' fields, by key: a date (YYYY-MM-DD), an amount (positive for money
    /// in, in the budget's currency), a payee, a memo.
    pub fields: HashMap<String, Vec<(Field, String)>>,
    /// Batches to import as one entry each, by their entry's key.
    pub whole_batches: HashSet<String>,
    /// A QIF's date order ("mdy" when empty), and whether it's been confirmed.
    pub date_order: &'a str,
    pub confirmed: bool,
    /// For an OFX whose amounts are all positive: whether money out is told by the transaction
    /// types (DEBIT, POS…). None leaves it to the file when the file settles it.
    pub signs_by_type: Option<bool>,
}

/// A transaction record of a statement as a line: where it is, what it says, and what became of it.
#[derive(Debug, Clone)]
pub(crate) struct RecordLine {
    /// Where it is among the file's records: `t1` (the first OFX transaction, or MT940 `:61:`),
    /// `r1` (QIF record), `e1` (CAMT entry), `e1.2` (that entry's second payment).
    pub key: String,
    pub written: Written,
    /// Its fields as they read now, with any values given for them; as written where they don't read.
    pub fields: Fields,
    /// The bank's ID for it, when the file gives one apart from its date and amount (OFX), so it's
    /// known even when the line doesn't read.
    pub bank_id: String,
    pub status: Status,
    /// What it imports as, with any values given for its fields, when it does.
    pub transaction: Option<BankTransaction>,
    /// What it reads as without help, when it does: its identity is made from this.
    pub original: Option<BankTransaction>,
    /// It holds its place among records sharing a bank ID, as today's import counts them: it
    /// reads, or would but for a damaged part (see `bank_csv::CsvLine::reserves`).
    pub reserves: bool,
    /// The CAMT batch it belongs to (see `StatementLines::batches`).
    pub batch: Option<usize>,
}
/// A record's date and amount as the file writes them, and its payee and memo as read.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Written {
    pub date: String,
    pub payee: String,
    pub memo: String,
    pub amount: String,
}
/// An account in a statement file; a bank's download can hold several.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Source {
    /// Its number, IBAN or name as the file gives it, or its place when the file gives none.
    pub label: String,
    /// The currency the file says it's in.
    pub currency: Option<String>,
    pub lines: usize,
}
/// A CAMT entry booking several payments at once.
#[derive(Debug, Clone)]
pub(crate) struct Batch {
    /// The entry's key; its payments' are this with `.1`, `.2`….
    pub entry: String,
    /// The entry as one transaction, as today's import reads it when it doesn't split it: its bank
    /// ID names the batch.
    pub whole: Option<BankTransaction>,
    /// The entry's total, when it reads.
    pub total: Option<i64>,
    /// The payments' lines.
    pub payments: Vec<usize>,
    /// The entry's own line, when it's imported as one.
    pub line: Option<usize>,
    /// Whether the payments add up to the entry, when they and it all read.
    pub adds_up: Option<bool>,
    pub as_one: bool,
    /// Today's import reads it as its payments (they read and add up as the file has them);
    /// otherwise it reads it as the one entry.
    pub splits_today: bool,
}
/// How an OFX whose amounts are all positive gets its signs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Signs {
    /// Money out is told by the transaction types (DEBIT, POS…).
    pub by_type: bool,
    /// An amount that doesn't read could be the negative one that says the types aren't needed,
    /// and no one has said: an import asks.
    pub unconfirmed: bool,
}
#[derive(Debug, Clone)]
pub(crate) struct StatementLines {
    pub sources: Vec<Source>,
    /// The account read; the others' lines are left out.
    pub source: usize,
    pub lines: Vec<RecordLine>,
    pub batches: Vec<Batch>,
    /// How a QIF's dates read.
    pub dates: Option<DateReading>,
    pub signs: Option<Signs>,
    /// What today's import stops at, for the account it reads.
    pub stop: Option<String>,
}

/// A record's parts as read, each read or not, before what the editor was told.
struct Parts {
    key: String,
    source: usize,
    written: Written,
    date: Result<String, String>,
    amount: Result<i64, String>,
    payee: String,
    memo: String,
    fit: Fit,
    left_out: Option<(Reason, String)>,
    batch: Option<usize>,
}
/// How a record's bank ID is made: as the file gives it (OFX), or its reference with its date and
/// amount (CAMT, MT940; see `dated_fit`).
enum Fit {
    Plain(String),
    Dated(String),
}
struct Parsed {
    sources: Vec<Source>,
    source: usize,
    parts: Vec<Parts>,
    batches: Vec<Batch>,
    dates: Option<DateReading>,
    signs: Option<Signs>,
    stop: Option<String>,
    /// The date order a date given for a line is read in.
    order: String,
}

/// A statement in an exchange format as lines: every transaction record, with what became of it
/// whatever the other records hold. Only a file that isn't a statement at all is an error.
pub(crate) fn lines(format: &str, content: &str, currency: &str, o: &LineOptions) -> Result<StatementLines, String> {
    let content = content.trim_start_matches('\u{feff}');
    let mut read = match format {
        "ofx" | "qfx" => ofx_lines(content, currency, o),
        "qif" => qif_lines(content, currency, o),
        "camt053" => camt_lines(content, currency, o)?,
        "mt940" => mt940_lines(content, currency, o),
        _ => return Err(t!("Choose CSV, OFX, QFX, QIF, CAMT.053 or MT940.")),
    };
    for p in &read.parts {
        if let Some(s) = read.sources.get_mut(p.source) {
            s.lines += 1;
        }
    }
    let order = read.order.clone();
    let mut lines: Vec<RecordLine> = read.parts.into_iter().map(|p| finish(p, o, currency, &order)).collect();
    for batch in &mut read.batches {
        settle(batch, &mut lines);
    }
    Ok(StatementLines {
        sources: read.sources,
        source: read.source,
        lines,
        batches: read.batches,
        dates: read.dates,
        signs: read.signs,
        stop: read.stop,
    })
}

/// A statement read as today's import reads it: its transactions, or what stops it.
pub(crate) fn transactions(format: &str, content: &str, currency: &str, date_format: &str) -> Result<Vec<BankTransaction>, String> {
    if matches!(format, "ofx" | "qfx") {
        if let Some(code) = tag(content, "CURDEF").filter(|c| !c.is_empty()) {
            if !code.eq_ignore_ascii_case(currency) {
                return Err(t!("File currency {code} differs from {currency}.", code, currency));
            }
        }
    }
    let o = LineOptions { legacy: true, date_order: date_format, ..LineOptions::default() };
    let read = lines(format, content, currency, &o)?;
    if let Some(stop) = read.stop {
        return Err(stop);
    }
    Ok(read.lines.into_iter().filter(|l| l.status == Status::Ready).filter_map(|l| l.transaction).collect())
}

/// A line with what the editor was told: values given for its fields, and whether to include it.
fn finish(p: Parts, o: &LineOptions, currency: &str, order: &str) -> RecordLine {
    let Parts { key, written, date, amount, payee, memo, fit, left_out, batch, .. } = p;
    let make = |date: &str, amount: i64, payee: &str, memo: &str| BankTransaction {
        fit: match &fit {
            Fit::Plain(f) => f.clone(),
            Fit::Dated(reference) => dated_fit(reference, date, amount),
        },
        date: date.to_string(),
        payee: payee.to_string(),
        memo: memo.to_string(),
        amount,
        transfer_account_id: None,
    };
    let original = match (&left_out, &date, &amount) {
        (None, Ok(d), Ok(a)) => Some(make(d, *a, &payee, &memo)),
        _ => None,
    };
    let (mut date, mut amount, mut payee, mut memo, mut given) = (date, amount, payee, memo, false);
    for (field, value) in o.fields.get(&key).into_iter().flatten() {
        match field {
            Field::Date => {
                date = crate::bank_import::date(value, order).map_err(|_| t!("Can't read the date ‘{date}’.", date = value.trim()));
            }
            Field::Amount => {
                given = true;
                amount = money(value, currency).and_then(|a| if a == 0 { Err(t!("Enter an amount other than zero.")) } else { Ok(a) });
            }
            Field::Payee => payee = value.trim().to_string(),
            Field::Memo => memo = value.trim().to_string(),
            Field::TransferAccountId => {},
        }
    }
    let typed = |field: Field| o.fields.get(&key).and_then(|f| f.iter().rev().find(|(g, _)| *g == field)).map(|(_, v)| v.trim().to_string());
    let fields = Fields {
        date: date.clone().unwrap_or_else(|_| typed(Field::Date).unwrap_or_else(|| written.date.clone())),
        payee: payee.clone(),
        memo: memo.clone(),
        amount: amount.as_ref().map_or_else(|_| typed(Field::Amount).unwrap_or_else(|| written.amount.clone()), |a| cents(*a)),
        transfer_account_id: typed(Field::TransferAccountId).unwrap_or_default(),
    };
    let bank_id = match &fit {
        Fit::Plain(f) => f.clone(),
        Fit::Dated(_) => String::new(),
    };
    let attention = |field, reason: &str| Status::Attention { field, column: None, reason: reason.to_string() };
    let status = match &left_out {
        Some((reason, detail)) if !(o.include.contains(&key) && reason.includable()) => {
            Status::LeftOut { reason: *reason, detail: detail.clone() }
        }
        Some((Reason::OtherCurrency, code)) if !given => {
            attention(Field::Amount, &t!("This line is in {code}: enter the amount in {currency}.", code, currency))
        }
        _ => match (&date, &amount) {
            (Err(e), _) => attention(Field::Date, e),
            (_, Err(e)) => attention(Field::Amount, e),
            _ => Status::Ready,
        },
    };
    let transaction = match (&status, &date, &amount) {
        (Status::Ready, Ok(d), Ok(a)) => {
            let mut transaction = make(d, *a, &payee, &memo);
            transaction.transfer_account_id = (!fields.transfer_account_id.is_empty()).then(|| fields.transfer_account_id.clone());
            Some(transaction)
        },
        _ => None,
    };
    RecordLine { key, written, fields, bank_id, status, transaction, original, reserves: left_out.is_none(), batch }
}

/// A batch's payments that don't add up to its entry, as the file has them or with a value given
/// for one, all need attention: which of them is wrong can't be told.
fn settle(batch: &mut Batch, lines: &mut [RecordLine]) {
    let Some(total) = batch.total.filter(|_| !batch.as_one) else { return };
    let zero = |l: &RecordLine| matches!(l.status, Status::LeftOut { reason: Reason::Zero, .. });
    let sum = |amount: &dyn Fn(&RecordLine) -> Option<i64>| -> Option<i64> {
        batch.payments.iter().map(|&i| if zero(&lines[i]) { Some(0) } else { amount(&lines[i]) }).sum()
    };
    let original = sum(&|l: &RecordLine| l.original.as_ref().map(|t| t.amount));
    let now = sum(&|l: &RecordLine| l.transaction.as_ref().map(|t| t.amount));
    if original.is_some_and(|s| s != total) {
        for &i in &batch.payments {
            lines[i].original = None;
        }
    }
    batch.adds_up = now.map(|s| s == total);
    if let Some(s) = now.filter(|s| *s != total) {
        let reason = t!("These payments add up to {sum}, not the entry's {total}.", sum = cents(s), total = cents(total));
        for &i in &batch.payments {
            if lines[i].status == Status::Ready {
                lines[i].status = Status::Attention { field: Field::Amount, column: None, reason: reason.clone() };
                lines[i].transaction = None;
            }
        }
    }
}
/// An amount in cents as a number with a decimal point: -1240 is "-12.40".
pub(crate) fn cents(n: i64) -> String {
    format!("{}{}.{:02}", if n < 0 { "-" } else { "" }, n.abs() / 100, n.abs() % 100)
}

/// The account to read: the one asked for, else the one today's import reads, unless it's in
/// another currency than the budget's and another account isn't.
fn choose(sources: &mut [Source], o: &LineOptions, today: usize, currency: &str) -> usize {
    // Several accounts each get a label to choose them by: the file's, else their place.
    if sources.len() > 1 {
        for i in 0..sources.len() {
            let base = if sources[i].label.trim().is_empty() { format!("Account {}", i + 1) } else { sources[i].label.clone() };
            let (mut label, mut n) = (base.clone(), 1);
            while sources[..i].iter().any(|s| s.label == label) {
                n += 1;
                label = format!("{base} ({n})");
            }
            sources[i].label = label;
        }
    }
    if let Some(i) = o.source.and_then(|l| sources.iter().position(|s| s.label == l)) {
        return i;
    }
    let fits = |s: &Source| s.currency.as_ref().is_none_or(|c| c.eq_ignore_ascii_case(currency));
    if o.legacy || sources.get(today).is_none_or(fits) {
        return today;
    }
    sources.iter().position(fits).unwrap_or(today)
}
/// The currency an account is in, when it isn't the budget's.
fn foreign(s: &Source, currency: &str) -> Option<String> {
    s.currency.as_ref().filter(|c| !c.eq_ignore_ascii_case(currency)).map(|c| c.to_ascii_uppercase())
}

/// The text of an OFX element (`<NAME>…`, closed or not), or None when there's none.
pub(crate) fn tag(s: &str, name: &str) -> Option<String> {
    tag_in(s, &s.to_ascii_uppercase(), name)
}
/// The same, with the text in capitals already: its tags are found whatever their case.
fn tag_in(s: &str, upper: &str, name: &str) -> Option<String> {
    let needle = format!("<{name}>");
    let start = upper.find(&needle)? + needle.len();
    let rest = &s[start..];
    let text = match rest.trim_start().strip_prefix("<![CDATA[") {
        Some(data) => data.split("]]>").next()?.trim(),
        None => rest.split('<').next()?.trim(),
    };
    if !text.contains('&') {
        return Some(text.to_string());
    }
    Some(text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\""))
}

/// An OFX or QFX statement: each account's transactions (a bank's every account can come in one
/// download), less those a correction deletes or replaces.
fn ofx_lines(content: &str, currency: &str, o: &LineOptions) -> Parsed {
    let upper = content.to_ascii_uppercase();
    let lists: Vec<usize> = upper.match_indices("<BANKTRANLIST>").map(|(i, _)| i).collect();
    // Each account's header (its currency and number) comes before its list of transactions.
    let bounds = |k: usize| -> ((usize, usize), (usize, usize)) {
        if lists.is_empty() {
            return ((0, content.len()), (0, content.len()));
        }
        let head = (if k == 0 { 0 } else { lists[k - 1] }, lists[k]);
        (head, (if k == 0 { 0 } else { lists[k] }, lists.get(k + 1).copied().unwrap_or(content.len())))
    };
    let mut sources: Vec<Source> = (0..lists.len().max(1))
        .map(|k| {
            let ((from, to), _) = bounds(k);
            let head = &content[from..to];
            let at = ["<BANKACCTFROM>", "<CCACCTFROM>"].iter().filter_map(|t| upper[from..to].rfind(t)).max();
            Source {
                label: at.and_then(|i| tag(&head[i..], "ACCTID")).unwrap_or_default(),
                currency: tag(head, "CURDEF").filter(|c| !c.is_empty()).map(|c| c.to_ascii_uppercase()),
                lines: 0,
            }
        })
        .collect();
    let source = choose(&mut sources, o, 0, currency);
    let (mut parts, mut kinds, mut stop) = (vec![], vec![], None);
    for (k, account) in sources.iter().enumerate() {
        let (_, (start, end)) = bounds(k);
        let starts: Vec<usize> = upper[start..end].match_indices("<STMTTRN>").map(|(i, _)| start + i).collect();
        let blocks: Vec<(&str, &str, String)> = starts
            .iter()
            .enumerate()
            .map(|(i, &s)| {
                let next = starts.get(i + 1).copied().unwrap_or(end);
                let close = upper[s..next].find("</STMTTRN>").map_or(next, |j| s + j);
                let action = tag_in(&content[s..close], &upper[s..close], "CORRECTACTION").unwrap_or_default().to_ascii_uppercase();
                (&content[s..close], &upper[s..close], action)
            })
            .collect();
        // Earlier lines a correction deletes or replaces.
        let corrected: HashSet<String> = blocks
            .iter()
            .filter(|(.., action)| matches!(action.as_str(), "DELETE" | "REPLACE"))
            .filter_map(|(block, upper, _)| tag_in(block, upper, "CORRECTFITID"))
            .collect();
        for (block, upper, action) in blocks {
            let tag = |name: &str| tag_in(block, upper, name);
            let deletes = action == "DELETE";
            let posted = tag("DTPOSTED").or_else(|| tag("DTUSER"));
            // YYYYMMDD with a time after it, or now and then a date written another way.
            let date = match &posted {
                None => Err(t!("Missing OFX posted date")),
                Some(d) => d.get(..8).and_then(|day| NaiveDate::parse_from_str(day, "%Y%m%d").ok())
                    .map(|day| day.format("%Y-%m-%d").to_string())
                    .map_or_else(|| crate::bank_import::date(d, "ymd").map_err(|_| t!("Invalid OFX posted date ‘{date}’", date = d)), Ok),
            };
            let written = tag("TRNAMT");
            let amount = written.as_deref().ok_or_else(|| t!("Missing OFX amount")).and_then(|a| money(a, currency));
            if k == source && !deletes && stop.is_none() {
                stop = date.as_ref().err().or(amount.as_ref().err()).cloned();
            }
            let (payee, memo) = (tag("NAME").unwrap_or_default(), tag("MEMO").unwrap_or_default());
            let (payee, memo) = if payee.is_empty() { (memo, String::new()) } else { (payee, memo) };
            let fit = tag("FITID").unwrap_or_default();
            let left_out = if k != source {
                Some((Reason::OtherAccount, account.label.clone()))
            } else if deletes {
                Some((Reason::Corrected, tag("CORRECTFITID").unwrap_or_default()))
            } else if let Some(code) = foreign(account, currency) {
                Some((Reason::OtherCurrency, code))
            } else if !fit.is_empty() && corrected.contains(&fit) {
                Some((Reason::Corrected, fit.clone()))
            } else if amount.as_ref().is_ok_and(|a| *a == 0) {
                Some((Reason::Zero, String::new()))
            } else {
                None
            };
            kinds.push(tag("TRNTYPE").unwrap_or_default().to_ascii_uppercase());
            parts.push(Parts {
                key: format!("t{}", parts.len() + 1),
                source: k,
                written: Written { date: posted.unwrap_or_default(), payee: payee.clone(), memo: memo.clone(), amount: written.unwrap_or_default() },
                date,
                amount,
                payee,
                memo,
                fit: Fit::Plain(fit),
                left_out,
                batch: None,
            });
        }
    }
    // A bank that writes every amount as positive and says which way it went in the type. Only
    // the account's own amounts that read decide it, whatever else their lines lack; while one
    // doesn't read, it could be the negative one that says otherwise, so it's asked.
    const OUT: [&str; 9] = ["DEBIT", "PAYMENT", "CHECK", "FEE", "SRVCHG", "POS", "ATM", "DIRECTDEBIT", "CASH"];
    let counted: Vec<usize> = (0..parts.len()).filter(|&i| parts[i].source == source && parts[i].left_out.is_none()).collect();
    let read: Vec<i64> = counted.iter().filter_map(|&i| parts[i].amount.as_ref().ok().copied()).collect();
    let typed = counted.iter().any(|&i| OUT.contains(&kinds[i].as_str()));
    let signs = (!read.is_empty() && read.iter().all(|a| *a > 0) && typed).then(|| {
        let settled = read.len() == counted.len();
        Signs { by_type: o.signs_by_type.unwrap_or(settled), unconfirmed: !settled && o.signs_by_type.is_none() }
    });
    if signs.is_some_and(|s| s.by_type) {
        for (p, kind) in parts.iter_mut().zip(&kinds) {
            if let (true, Ok(a)) = (p.source == source && OUT.contains(&kind.as_str()), &mut p.amount) {
                *a = -a.abs();
            }
        }
    }
    Parsed { sources, source, parts, batches: vec![], dates: None, signs, stop, order: "ymd".into() }
}

/// A QIF file: the records of each account's transactions (Quicken's every-account export holds
/// several, with lists of categories and the like, which aren't transactions).
fn qif_lines(content: &str, currency: &str, o: &LineOptions) -> Parsed {
    struct Record {
        account: usize,
        fields: HashMap<char, String>,
    }
    // Accounts by `!Account`, named by their description's N; records before any are the first's.
    let (mut records, mut names) = (vec![], vec![String::new()]);
    let mut fields: HashMap<char, String> = HashMap::new();
    // An account's own description (`!Account`) until its transactions' `!Type:` line, and lists
    // that aren't transactions (categories, classes, memorized payees).
    let (mut describing, mut other, mut unsupported) = (false, false, false);
    // A record with something in it, at its ^, or cut short by a header or the file's end.
    let keep = |fields: &mut HashMap<char, String>, records: &mut Vec<Record>, account: usize| {
        if ['D', 'T', 'U'].iter().any(|c| fields.get(c).is_some_and(|v| !v.is_empty())) {
            records.push(Record { account, fields: std::mem::take(fields) });
        }
        fields.clear();
    };
    for line in content.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let account = names.len() - 1;
        if let Some(header) = line.strip_prefix('!') {
            if !describing && !other {
                keep(&mut fields, &mut records, account);
            }
            fields.clear();
            let header = header.to_ascii_lowercase();
            if header == "account" {
                names.push(String::new());
                describing = true;
            } else if let Some(kind) = header.strip_prefix("type:") {
                describing = false;
                other = !matches!(kind.trim(), "bank" | "ccard" | "cash" | "oth a" | "oth l");
                unsupported |= other && matches!(kind.trim(), "invst" | "port");
            }
            continue;
        }
        if line == "^" {
            if describing || other {
                fields.clear();
            } else {
                keep(&mut fields, &mut records, account);
            }
        } else if describing {
            if let Some(name) = line.strip_prefix('N') {
                names[account] = name.trim().to_string();
            }
        } else if !other {
            let key = line.chars().next().unwrap();
            if ['D', 'P', 'M', 'T', 'U'].contains(&key) {
                fields.insert(key, line[1..].to_string());
            }
        }
    }
    // A last record the file didn't close with ^ reads when it has its date and amount; without
    // them, today's import stops at it.
    let last = names.len() - 1;
    let tail = (!describing && !other && !fields.is_empty()).then(|| {
        let whole = fields.contains_key(&'D') && (fields.contains_key(&'T') || fields.contains_key(&'U'));
        let kept = ['D', 'T', 'U'].iter().any(|c| fields.get(c).is_some_and(|v| !v.is_empty()));
        keep(&mut fields, &mut records, last);
        (!whole, kept)
    });
    let mut index: HashMap<usize, usize> = HashMap::new();
    let mut sources: Vec<Source> = vec![];
    for r in &records {
        index.entry(r.account).or_insert_with(|| {
            sources.push(Source { label: names[r.account].clone(), currency: None, lines: 0 });
            sources.len() - 1
        });
    }
    let get = |r: &Record, c: char| r.fields.get(&c).cloned().unwrap_or_default();
    // Newer Quicken writes the amount as U too, and some files only as U.
    let written: Vec<String> = records.iter().map(|r| if get(r, 'T').is_empty() { get(r, 'U') } else { get(r, 'T') }).collect();
    let amounts: Vec<Result<i64, String>> = written.iter().map(|a| money(a, currency)).collect();
    let moves = |i: usize| amounts[i].as_ref().map_or(true, |a| *a != 0);
    // Today's import reads the first account with a transaction in it.
    let today = (0..records.len()).find(|&i| moves(i)).map_or(0, |i| index[&records[i].account]);
    let source = choose(&mut sources, o, today, currency);
    let ours: Vec<usize> = (0..records.len()).filter(|&i| index[&records[i].account] == source).collect();
    let counted: Vec<usize> = ours.iter().copied().filter(|&i| moves(i)).collect();
    let (cut_short, tail_kept) = tail.unwrap_or((false, false));
    let tail_ours = tail_kept && ours.last() == Some(&(records.len() - 1));
    let mut stop = ours.iter().filter(|&&i| !(cut_short && tail_ours && i == records.len() - 1)).find_map(|&i| amounts[i].clone().err());
    let tail_account_ours = ours.first().is_some_and(|&i| records[i].account == last);
    if stop.is_none() && cut_short && (tail_account_ours || counted.is_empty()) {
        stop = Some(t!("Incomplete QIF record (missing ^ terminator)."));
    }
    if stop.is_none() && counted.is_empty() && unsupported {
        stop = Some(t!("Only bank, cash, credit-card and other asset or liability QIF files are supported."));
    }
    let chosen = if o.date_order.is_empty() { "mdy" } else { o.date_order };
    let raw: Vec<String> = records.iter().map(|r| get(r, 'D')).collect();
    let (dated, reading, order): (Vec<Result<String, String>>, Option<DateReading>, String) = if o.legacy {
        // Today's import reads the dates in the chosen order, else the only one that reads them all.
        let mut dated: Vec<Result<String, String>> = raw.iter().map(|d| crate::bank_import::date(d, chosen)).collect();
        let days: Vec<String> = counted.iter().map(|&i| raw[i].clone()).collect();
        match dates(&days, chosen) {
            Ok(read) => counted.iter().zip(read).for_each(|(&i, d)| dated[i] = Ok(d)),
            Err(e) => {
                counted.iter().for_each(|&i| dated[i] = Err(e.clone()));
                stop.get_or_insert(e);
            }
        }
        (dated, None, chosen.to_string())
    } else {
        let evidence: Vec<&str> = counted.iter().map(|&i| raw[i].as_str()).filter(|d| !d.trim().is_empty()).collect();
        let reading = read_dates(&evidence, chosen, o.confirmed);
        let dated = raw
            .iter()
            .map(|d| match d.trim() {
                "" => Err(t!("This record has no date.")),
                d => crate::bank_import::date(d, reading.order).map_err(|_| t!("Can't read the date ‘{date}’.", date = d)),
            })
            .collect();
        let order = reading.order.to_string();
        (dated, Some(reading), order)
    };
    let parts = records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let (payee, memo) = (get(r, 'P'), get(r, 'M'));
            let (payee, memo) = if payee.is_empty() { (memo, String::new()) } else { (payee, memo) };
            let s = index[&r.account];
            let left_out = if s != source {
                Some((Reason::OtherAccount, sources[s].label.clone()))
            } else if !moves(i) {
                Some((Reason::Zero, String::new()))
            } else {
                None
            };
            Parts {
                key: format!("r{}", i + 1),
                source: s,
                written: Written { date: raw[i].clone(), payee: payee.clone(), memo: memo.clone(), amount: written[i].clone() },
                date: dated[i].clone(),
                amount: amounts[i].clone(),
                payee,
                memo,
                fit: Fit::Plain(String::new()),
                left_out,
                batch: None,
            }
        })
        .collect();
    Parsed { sources, source, parts, batches: vec![], dates: reading, signs: None, stop, order }
}

/// A CAMT.053 bank-to-customer statement (or a camt.052 account report): each account's entries,
/// a pending one left out until it's booked. An entry that books several payments at once (a
/// batch) is its payments, each a line, when each has its amount in the budget's currency. The
/// payee is the other party: the creditor of money out, the debtor of money in.
fn camt_lines(content: &str, currency: &str, o: &LineOptions) -> Result<Parsed, String> {
    let root = xml(content)?;
    let mut statements = vec![];
    root.find_all("Stmt", &mut statements);
    if statements.is_empty() {
        root.find_all("Rpt", &mut statements);
    }
    if statements.is_empty() {
        return Err(t!("This XML file has no CAMT.053 statement in it."));
    }
    // Several statements for the one account (a day each) are all its.
    let account = |s: &Node| s.at(&["Acct", "Id"]).map(Node::deep_text);
    let (mut ids, mut sources): (Vec<Option<String>>, Vec<Source>) = (vec![], vec![]);
    for s in &statements {
        let id = account(s);
        if !ids.contains(&id) {
            let code = s.text_at(&["Acct", "Ccy"]).or_else(|| s.all("Ntry").find_map(|e| e.child("Amt")?.attr("Ccy")));
            sources.push(Source { label: id.clone().unwrap_or_default(), currency: code.map(str::to_ascii_uppercase), lines: 0 });
            ids.push(id);
        }
    }
    let source = choose(&mut sources, o, 0, currency);
    let (mut parts, mut batches, mut stop): (Vec<Parts>, Vec<Batch>, Option<String>) = (vec![], vec![], None);
    let mut number = 0;
    for s in &statements {
        let k = ids.iter().position(|id| *id == account(s)).unwrap_or(0);
        for entry in s.all("Ntry") {
            number += 1;
            let key = format!("e{number}");
            let status = entry.text_at(&["Sts", "Cd"]).or(entry.text_at(&["Sts"])).unwrap_or("BOOK");
            let booked = status.eq_ignore_ascii_case("BOOK");
            let posted = entry.text_at(&["BookgDt", "Dt"]).or(entry.text_at(&["BookgDt", "DtTm"]))
                .or(entry.text_at(&["ValDt", "Dt"])).or(entry.text_at(&["ValDt", "DtTm"]));
            let date = posted.and_then(iso_date).ok_or_else(|| t!("A CAMT.053 entry has no booking date."));
            let out_of = entry.text_at(&["CdtDbtInd"]).is_some_and(|i| i.eq_ignore_ascii_case("DBIT"));
            // A reversal keeps the parties of what it reverses: a direct debit coming back names its
            // creditor still.
            let reversal = entry.text_at(&["RvslInd"]).is_some_and(|r| r.eq_ignore_ascii_case("true"));
            let other_is_creditor = out_of != reversal;
            // An amount in the budget's currency, or why not, and the currency it's in instead.
            let amount = |amt: &Node| -> (Result<i64, String>, Option<String>) {
                if let Some(code) = amt.attr("Ccy").filter(|c| !c.eq_ignore_ascii_case(currency)) {
                    let why = t!("The file's amounts are in {code}, but this budget uses {currency}.", code, currency);
                    return (Err(why), Some(code.to_ascii_uppercase()));
                }
                (money(amt.text.trim(), currency).map(|n| if out_of { -n.abs() } else { n.abs() }), None)
            };
            let shown = |amt: Option<&Node>| amt.map(|a| format!("{}{}", if out_of { "-" } else { "" }, a.text.trim())).unwrap_or_default();
            let (total, foreign) = match entry.child("Amt") {
                Some(a) => amount(a),
                None => (Err(t!("A CAMT.053 entry has no amount.")), None),
            };
            if k == source && booked && stop.is_none() {
                stop = date.as_ref().err().or(total.as_ref().err()).cloned();
            }
            let details: Vec<&Node> = entry.all("NtryDtls").flat_map(|d| d.all("TxDtls")).collect();
            let party = |tx: &Node| {
                let side = if other_is_creditor { "Cdtr" } else { "Dbtr" };
                let base = tx.at(&["RltdPties", side]);
                base.and_then(|p| p.text_at(&["Nm"]).or(p.text_at(&["Pty", "Nm"])).map(String::from))
                    .or_else(|| tx.text_at(&["RltdPties", if other_is_creditor { "UltmtCdtr" } else { "UltmtDbtr" }, "Nm"]).map(String::from))
            };
            let purpose = |tx: &Node| {
                let lines: Vec<String> = tx.child("RmtInf").map(|r| r.all("Ustrd").map(|u| u.text.clone()).collect()).unwrap_or_default();
                let text = tidy(&lines.join(" "));
                if text.is_empty() { tx.text_at(&["AddtlTxInf"]).map(tidy).unwrap_or_default() } else { text }
            };
            let reference = [&["AcctSvcrRef"][..], &["Refs", "AcctSvcrRef"], &["NtryRef"]].iter()
                .filter_map(|p| entry.text_at(p)).find(|r| known(r)).unwrap_or("").to_string();
            let general = entry.text_at(&["AddtlNtryInf"]).map(tidy).unwrap_or_default();
            let left_out = if k != source {
                Some((Reason::OtherAccount, sources[k].label.clone()))
            } else if !booked {
                Some((Reason::Pending, status.to_string()))
            } else {
                None
            };
            let zero = |a: &Result<i64, String>| a.as_ref().is_ok_and(|a| *a == 0).then(|| (Reason::Zero, String::new()));
            // The entry as one transaction: its first payment's parties.
            let first = details.first().copied();
            let (payee, memo) = parties(
                first.and_then(party).unwrap_or_default(),
                first.map(purpose).filter(|m| !m.is_empty()).unwrap_or_else(|| general.clone()),
                &general,
            );
            let whole = |batch: Option<usize>| Parts {
                key: key.clone(),
                source: k,
                written: Written { date: posted.unwrap_or_default().into(), payee: payee.clone(), memo: memo.clone(), amount: shown(entry.child("Amt")) },
                date: date.clone(),
                amount: total.clone(),
                payee: payee.clone(),
                memo: memo.clone(),
                fit: Fit::Dated(reference.clone()),
                left_out: left_out.clone().or_else(|| foreign.clone().map(|c| (Reason::OtherCurrency, c))).or_else(|| zero(&total)),
                batch,
            };
            let amounts: Vec<Option<&Node>> = details.iter().map(|tx| tx.at(&["AmtDtls", "TxAmt", "Amt"]).or(tx.child("Amt"))).collect();
            let in_ours = |a: &Option<&Node>| a.is_some_and(|a| a.attr("Ccy").is_none_or(|c| c.eq_ignore_ascii_case(currency)));
            if details.len() < 2 || !amounts.iter().all(in_ours) {
                parts.push(whole(None));
                continue;
            }
            let read: Vec<Result<i64, String>> = amounts.iter().map(|a| amount(a.unwrap()).0).collect();
            let sum: Option<i64> = read.iter().map(|a| a.as_ref().ok().copied()).sum();
            let adds_up = total.as_ref().ok().zip(sum).map(|(t, s)| s == *t);
            let as_one = o.whole_batches.contains(&key) || (o.legacy && adds_up != Some(true));
            let b = batches.len();
            let mut batch = Batch {
                entry: key.clone(),
                whole: match (&date, &total) {
                    (Ok(d), Ok(t)) => Some(BankTransaction { fit: dated_fit(&reference, d, *t), date: d.clone(), payee: payee.clone(), memo: memo.clone(), amount: *t, transfer_account_id: None }),
                    _ => None,
                },
                total: total.as_ref().ok().copied(),
                payments: vec![],
                line: None,
                adds_up,
                as_one,
                splits_today: adds_up == Some(true),
            };
            if as_one {
                batch.line = Some(parts.len());
                parts.push(whole(Some(b)));
            }
            for (n, (tx, amount)) in details.iter().zip(read).enumerate() {
                let (payee, memo) = parties(party(tx).unwrap_or_default(), purpose(tx), &general);
                let tx_ref = tx.text_at(&["Refs", "AcctSvcrRef"]).or(tx.text_at(&["Refs", "EndToEndId"])).filter(|r| known(r));
                let left = if k != source {
                    left_out.clone()
                } else if as_one {
                    Some((Reason::InWholeBatch, key.clone()))
                } else {
                    left_out.clone().or_else(|| zero(&amount))
                };
                batch.payments.push(parts.len());
                parts.push(Parts {
                    key: format!("{key}.{}", n + 1),
                    source: k,
                    written: Written { date: posted.unwrap_or_default().into(), payee: payee.clone(), memo: memo.clone(), amount: shown(amounts[n]) },
                    date: date.clone(),
                    amount,
                    payee,
                    memo,
                    fit: Fit::Dated(tx_ref.map(|r| format!("{reference}/{r}")).unwrap_or_default()),
                    left_out: left,
                    batch: Some(b),
                });
            }
            batches.push(batch);
        }
    }
    Ok(Parsed { sources, source, parts, batches, dates: None, signs: None, stop, order: "ymd".into() })
}
/// A reference that tells transactions apart: not a placeholder for none.
fn known(r: &str) -> bool {
    !r.is_empty() && !["NONREF", "NOTPROVIDED", "NONE", "N/A"].contains(&r.to_ascii_uppercase().as_str())
}
/// The bank's reference as an ID with its date and amount: a bank that uses one reference for a
/// mandate or a day's entries doesn't make next month's charge a duplicate.
fn dated_fit(reference: &str, date: &str, amount: i64) -> String {
    if reference.is_empty() { String::new() } else { format!("{date}/{amount}/{reference}") }
}
/// The payee and memo, with the memo (or the entry's text) as the payee when there's no other party.
fn parties(payee: String, memo: String, general: &str) -> (String, String) {
    if payee.trim().is_empty() {
        (if memo.is_empty() { general.to_string() } else { memo }, String::new())
    } else {
        (tidy(&payee), memo)
    }
}

/// An MT940 statement: every `:61:` line, with its `:86:` text, of each account's statements (a
/// file often holds one for each day). German banks structure that text in `?nn` fields: `?00` the
/// kind of payment, `?20`–`?29` and `?60`–`?63` the purpose (SEPA keys such as `SVWZ+`), `?32`/`?33`
/// the other party's name.
fn mt940_lines(content: &str, currency: &str, o: &LineOptions) -> Parsed {
    // Fields start with :tag: at the start of a line; their text runs on over the next lines.
    let mut fields: Vec<(String, String)> = vec![];
    for line in content.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.starts_with('{') || trimmed == "-" || trimmed == "-}" {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(':') {
            if let Some((tag, value)) = rest.split_once(':').filter(|(t, _)| t.len() <= 3 && t.chars().all(|c| c.is_ascii_alphanumeric())) {
                fields.push((tag.to_ascii_uppercase(), value.to_string()));
                continue;
            }
        }
        if let Some((_, value)) = fields.last_mut() {
            value.push('\n');
            value.push_str(line);
        }
    }
    // Accounts by `:25:`; lines before the first are its. Each one's currency is its opening
    // balance's (`:60F:` C or D, the date, then the currency).
    let code = |value: &str| value.trim().chars().skip(7).take(3).collect::<String>();
    let first = fields.iter().find(|(t, _)| t == "25").map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let (mut ids, mut sources) = (vec![first.clone()], vec![Source { label: first, currency: None, lines: 0 }]);
    let mut current = 0;
    for (tag, value) in &fields {
        match tag.as_str() {
            "25" => {
                let id = value.trim();
                current = ids.iter().position(|i| i == id).unwrap_or_else(|| {
                    ids.push(id.to_string());
                    sources.push(Source { label: id.to_string(), currency: None, lines: 0 });
                    ids.len() - 1
                });
            }
            "60F" | "60M" => {
                let code = code(value);
                if code.len() == 3 && sources[current].currency.is_none() {
                    sources[current].currency = Some(code.to_ascii_uppercase());
                }
            }
            _ => {}
        }
    }
    let source = choose(&mut sources, o, 0, currency);
    let (mut parts, mut stop, mut pending): (Vec<Parts>, Option<String>, Option<usize>) = (vec![], None, None);
    current = 0;
    for (tag, value) in &fields {
        // An :86: belongs to the :61: just before it; one after the closing balance sums the day.
        if tag != "86" {
            pending = None;
        }
        match tag.as_str() {
            "25" => current = ids.iter().position(|i| i == value.trim()).unwrap_or(0),
            "60F" | "60M" => {
                // Today's import stops at any statement in another currency.
                let code = code(value);
                if code.len() == 3 && !code.eq_ignore_ascii_case(currency) && stop.is_none() {
                    stop = Some(t!("The file's amounts are in {code}, but this budget uses {currency}.", code, currency));
                }
            }
            "61" => {
                let line = statement_line(value, currency);
                if current == source && stop.is_none() {
                    stop = line.date.as_ref().err().or(line.amount.as_ref().err()).cloned();
                }
                let left_out = if current != source {
                    Some((Reason::OtherAccount, sources[current].label.clone()))
                } else if let Some(code) = foreign(&sources[current], currency) {
                    Some((Reason::OtherCurrency, code))
                } else if line.amount.as_ref().is_ok_and(|a| *a == 0) {
                    Some((Reason::Zero, String::new()))
                } else {
                    None
                };
                pending = Some(parts.len());
                parts.push(Parts {
                    key: format!("t{}", parts.len() + 1),
                    source: current,
                    written: Written { date: line.written_date, amount: line.written_amount, ..Written::default() },
                    date: line.date,
                    amount: line.amount,
                    payee: String::new(),
                    memo: String::new(),
                    fit: Fit::Dated(line.reference),
                    left_out,
                    batch: None,
                });
            }
            "86" => {
                if let Some(p) = pending.map(|i| &mut parts[i]) {
                    let (payee, memo) = details(value);
                    p.written.payee.clone_from(&payee);
                    p.written.memo.clone_from(&memo);
                    (p.payee, p.memo) = (payee, memo);
                }
            }
            _ => {}
        }
    }
    Parsed { sources, source, parts, batches: vec![], dates: None, signs: None, stop, order: "ymd".into() }
}
/// A `:61:` line's parts, each read or not.
struct StatementLine {
    date: Result<String, String>,
    amount: Result<i64, String>,
    reference: String,
    written_date: String,
    written_amount: String,
}
/// A `:61:` line: value date YYMMDD, booking date MMDD, C/D (RC/RD reverse them), a funds code
/// letter, the amount with a decimal comma, then the type and references.
fn statement_line(value: &str, currency: &str) -> StatementLine {
    let text: String = value.lines().next().unwrap_or("").trim().to_string();
    let bad = || t!("Cannot read the MT940 line ‘:61:{text}’.", text);
    let six = text.char_indices().nth(6).map_or(text.len(), |(i, _)| i);
    let value_date = NaiveDate::parse_from_str(&text[..six], "%y%m%d").ok();
    let mut written_date = text[..six].to_string();
    let mut rest = &text[six..];
    let mut date = value_date.ok_or_else(bad);
    if let Some(day) = rest.get(..4).filter(|s| s.bytes().all(|b| b.is_ascii_digit())) {
        let (m, d) = (day[..2].parse::<u32>().unwrap_or(0), day[2..].parse::<u32>().unwrap_or(0));
        written_date.push_str(day);
        // The booking date has no year: the value date's, or the next or last one across New Year.
        date = date.and_then(|value_date| {
            let year = chrono::Datelike::year(&value_date);
            [year, year + 1, year - 1].iter()
                .filter_map(|y| NaiveDate::from_ymd_opt(*y, m, d))
                .min_by_key(|b| (*b - value_date).num_days().abs())
                .ok_or_else(bad)
        });
        rest = &rest[4..];
    }
    let (sign, after) = if let Some(r) = rest.strip_prefix("RC") {
        (Some(-1), r)
    } else if let Some(r) = rest.strip_prefix("RD") {
        (Some(1), r)
    } else if let Some(r) = rest.strip_prefix('C') {
        (Some(1), r)
    } else if let Some(r) = rest.strip_prefix('D') {
        (Some(-1), r)
    } else {
        (None, rest)
    };
    let after = after.strip_prefix(|c: char| c.is_ascii_alphabetic()).unwrap_or(after);
    let number: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
    // The type (N, F or S and three letters or digits, "NBA " at ING) follows the amount, then the
    // references. Anything else straight after it means the amount is damaged ("6x,20"), not 6.00.
    let typed = |i: usize| after[i..].starts_with(['N', 'F', 'S']) && after[i + 1..].starts_with(|c: char| c.is_ascii_alphanumeric());
    let follows = &after[number.len()..];
    let whole = follows.is_empty() || typed(number.len()) || follows.starts_with(|c: char| c == '/' || c.is_whitespace());
    let amount = match sign {
        Some(sign) if whole => money(&number.replace('.', ","), currency).map(|n| n.abs() * sign),
        _ => Err(bad()),
    };
    // Found by its shape when the amount is damaged, so a repaired line keeps its ID.
    let at = if amount.is_ok() {
        number.len()
    } else {
        after.char_indices().map(|(i, _)| i).filter(|&i| i >= number.len()).find(|&i| typed(i)).unwrap_or(number.len())
    };
    let reference = after.get(at..).and_then(|r| r.get(4..)).unwrap_or("");
    // The bank's reference after //, else the customer's before it.
    let mut parts = reference.splitn(2, "//");
    let (customer, bank) = (parts.next().unwrap_or("").trim(), parts.next().unwrap_or("").trim());
    let reference = [bank, customer].into_iter().find(|r| known(r)).unwrap_or("").to_string();
    let written_amount = rest[..rest.len() - after.len() + number.len()].to_string();
    let date = date.map(|d| d.format("%Y-%m-%d").to_string());
    StatementLine { date, amount, reference, written_date, written_amount }
}
/// The payee and memo in a `:86:` field: numbered subfields (`?nn` in Germany and Austria, `~nn`
/// or `^nn` in Poland), Dutch `/NAME/…/REMI/…` keys, or free text as the payee.
fn details(value: &str) -> (String, String) {
    let joined: String = value.lines().collect();
    // Free text broken over lines: the breaks were spaces.
    let spaced = value.lines().map(str::trim).collect::<Vec<_>>().join(" ");
    let b = joined.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let marks = |i: usize| b.get(i).copied().filter(|c| b"?~^<>|".contains(c) && digits(i + 1..i + 3));
    // The subfields start after the transaction code (`105?00…`), or at once (`?00…`).
    let Some(sep) = marks(3).filter(|_| digits(0..3)).or(marks(0)) else {
        return keyed(&joined).unwrap_or_else(|| (tidy(&spaced), String::new()));
    };
    let (mut kind, mut purpose, mut name) = (String::new(), String::new(), String::new());
    for part in joined.split(sep as char).skip(1) {
        let (code, text) = (part.get(..2).unwrap_or(""), part.get(2..).unwrap_or(""));
        match code {
            "00" => kind.push_str(text),
            "20" | "21" | "22" | "23" | "24" | "25" | "26" | "27" | "28" | "29" | "60" | "61" | "62" | "63" => purpose.push_str(text),
            "32" | "33" => name.push_str(text),
            _ => {}
        }
    }
    // SEPA keys in the purpose: the remittance text (SVWZ+) is what people write.
    let memo = purpose.find("SVWZ+").map(|i| &purpose[i + 5..]).map(|s| {
        let end = ["ABWA+", "ABWE+", "IBAN+", "BIC+", "EREF+", "KREF+", "MREF+", "CRED+"].iter()
            .filter_map(|k| s.find(k)).min().unwrap_or(s.len());
        s[..end].to_string()
    }).unwrap_or(purpose);
    let payee = if name.trim().is_empty() { kind } else { name };
    (tidy(&payee), tidy(&memo))
}
/// Dutch banks' `:86:` keys: `/TRTP/SEPA OVERBOEKING/IBAN/…/NAME/J Jansen/REMI/USTD//Factuur 12/`,
/// or ING's `/CNTP/account/BIC/name/city/`.
fn keyed(text: &str) -> Option<(String, String)> {
    const KEYS: [&str; 24] = [
        "TRTP", "IBAN", "BIC", "NAME", "REMI", "EREF", "MARF", "CSID", "ORDP", "BENM", "ULTC", "ULTD", "PURP", "RTRN",
        "ADDR", "CNTP", "ISDT", "PREF", "KREF", "SVCL", "OCMT", "CHGS", "EXCH", "BUSP",
    ];
    let text = text.trim();
    if !text.starts_with('/') {
        return None;
    }
    let mut at: Vec<(usize, usize, &str)> = KEYS.iter()
        .flat_map(|k| text.match_indices(&format!("/{k}/")).map(move |(i, m)| (i, i + m.len(), *k)).collect::<Vec<_>>())
        .collect();
    at.sort();
    if !at.iter().any(|(_, _, k)| ["NAME", "REMI", "TRTP", "CNTP"].contains(k)) {
        return None;
    }
    let value = |key: &str| {
        let i = at.iter().position(|(_, _, k)| *k == key)?;
        let (end, next) = (at[i].1, at.get(i + 1).map_or(text.len(), |n| n.0));
        Some(text.get(end..next.max(end)).unwrap_or("").trim_matches('/').to_string())
    };
    let name = value("NAME").filter(|n| !n.trim().is_empty())
        .or_else(|| value("CNTP").and_then(|c| c.split('/').nth(2).map(String::from)))
        .filter(|n| !n.trim().is_empty())
        .or_else(|| value("TRTP"))
        .unwrap_or_default();
    let remi = value("REMI").unwrap_or_default();
    let remi = remi.strip_prefix("USTD//").or(remi.strip_prefix("USTD/")).unwrap_or(&remi);
    let remi = remi.strip_prefix("STRD/CUR/").or(remi.strip_prefix("STRD/")).unwrap_or(remi);
    Some((tidy(&name), tidy(remi.trim_matches('/'))))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAMT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
 <BkToCstmrStmt><Stmt><Id>1</Id>
  <Acct><Id><IBAN>DE00123456780000000000</IBAN></Id><Ccy>EUR</Ccy></Acct>
  <Ntry><Amt Ccy="EUR">64.20</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>BOOK</Sts>
   <BookgDt><Dt>2026-10-01</Dt></BookgDt><ValDt><Dt>2026-10-01</Dt></ValDt><AcctSvcrRef>REF-1</AcctSvcrRef>
   <NtryDtls><TxDtls><RltdPties><Dbtr><Nm>Max Mustermann</Nm></Dbtr><Cdtr><Nm>Stadtwerke &amp; Co</Nm></Cdtr></RltdPties>
    <RmtInf><Ustrd>Abschlag Strom</Ustrd><Ustrd>Oktober</Ustrd></RmtInf></TxDtls></NtryDtls></Ntry>
  <Ntry><Amt Ccy="EUR">12.90</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts>PDNG</Sts><BookgDt><Dt>2026-10-02</Dt></BookgDt></Ntry>
  <Ntry><Amt Ccy="EUR">300.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts><BookgDt><DtTm>2026-10-03T10:00:00</DtTm></BookgDt>
   <AcctSvcrRef>BATCH</AcctSvcrRef>
   <NtryDtls>
    <TxDtls><Refs><EndToEndId>A</EndToEndId></Refs><AmtDtls><TxAmt><Amt Ccy="EUR">100.00</Amt></TxAmt></AmtDtls><RltdPties><Dbtr><Pty><Nm>Anna</Nm></Pty></Dbtr></RltdPties></TxDtls>
    <TxDtls><Refs><EndToEndId>B</EndToEndId></Refs><AmtDtls><TxAmt><Amt Ccy="EUR">200.00</Amt></TxAmt></AmtDtls><RltdPties><Dbtr><Pty><Nm>Ben</Nm></Pty></Dbtr></RltdPties></TxDtls>
   </NtryDtls></Ntry>
 </Stmt></BkToCstmrStmt></Document>"#;
    const STA: &str = ":20:STARTUMS\r\n:25:12345678/0012345678\r\n:28C:00001/001\r\n:60F:C261001EUR1000,00\r\n\
:61:2610011001DR64,20NDDTNONREF//4711\r\n:86:105?00BASISLASTSCHRIFT?20EREF+4711-2026-10?21SVWZ+Abschlag Strom Ok\r\ntober?22MREF+M-1?30BYLADEM1001?31DE00987654320000000011?32Stadtwerke Beispiel?33stadt\r\n\
:62F:C261001EUR935,80\r\n-\r\n:20:STARTUMS\r\n:25:12345678/0012345678\r\n:28C:00002/001\r\n:60F:C261001EUR935,80\r\n\
:61:2612311231CR2450,00NTRFNONREF\r\n:86:Gehalt Dezember Arbeitgeber GmbH\r\n:61:2701020102RD12,90NMSCNONREF\r\n:86:166?00GUTSCHRIFT\r\n:62F:C270102EUR3398,70\r\n-";

    fn status(s: &Status) -> String {
        match s {
            Status::Ready => "ready".into(),
            Status::Attention { field, .. } => format!("attention {field:?}"),
            Status::LeftOut { reason, .. } => format!("{reason:?}"),
        }
    }
    fn statuses(read: &StatementLines) -> Vec<(String, String, bool)> {
        read.lines.iter().map(|l| (l.key.clone(), status(&l.status), l.reserves)).collect()
    }
    fn want(rows: &[(&str, &str, bool)]) -> Vec<(String, String, bool)> {
        rows.iter().map(|(k, s, r)| (k.to_string(), s.to_string(), *r)).collect()
    }
    fn amounts(read: &StatementLines) -> Vec<Option<i64>> {
        read.lines.iter().map(|l| l.transaction.as_ref().map(|t| t.amount)).collect()
    }
    fn read(format: &str, content: &str, currency: &str) -> StatementLines {
        lines(format, content, currency, &LineOptions::default()).unwrap()
    }
    fn fixture(file: &str) -> String {
        let bytes = std::fs::read(format!("{}/../../shared/fixtures/bank-files/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        crate::bank_csv::read_statement(&bytes).unwrap().text
    }
    /// An account's number, its currency, and its transactions' type, date, amount and name.
    type Account<'a> = (&'a str, &'a str, &'a [(&'a str, &'a str, &'a str, &'a str)]);
    fn ofx(statements: &[Account]) -> String {
        let mut out = String::from("OFXHEADER:100\nDATA:OFXSGML\n<OFX><BANKMSGSRSV1>");
        for (account, currency, rows) in statements {
            out += &format!("<STMTTRNRS><STMTRS><CURDEF>{currency}<BANKACCTFROM><ACCTID>{account}</BANKACCTFROM><BANKTRANLIST>\n");
            for (i, (kind, date, amount, name)) in rows.iter().enumerate() {
                out += &format!("<STMTTRN><TRNTYPE>{kind}<DTPOSTED>{date}<TRNAMT>{amount}<FITID>{account}-{i}<NAME>{name}</STMTTRN>\n");
            }
            out += "</BANKTRANLIST></STMTRS></STMTTRNRS>";
        }
        out + "</BANKMSGSRSV1></OFX>"
    }

    #[test]
    fn every_record_says_what_became_of_it() {
        let file = "<OFX><CURDEF>EUR<BANKTRANLIST>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261001<TRNAMT>-3.20<FITID>a<NAME>Bakery</STMTTRN>\
<STMTTRN><TRNTYPE>OTHER<DTPOSTED>20261002<TRNAMT>0.00<FITID>b<NAME>Card check</STMTTRN>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261003<TRNAMT>-9.00<FITID>c<NAME>Wrong</STMTTRN>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261003<TRNAMT>-9.00<FITID>d<CORRECTFITID>c<CORRECTACTION>DELETE</STMTTRN>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>2026-1O-04<TRNAMT>-5.00<FITID>e<NAME>Shop</STMTTRN>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261005<TRNAMT>n/a<FITID>f<NAME>Grocer</STMTTRN>\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261006<FITID>g<NAME>Lunch</STMTTRN>\
</BANKTRANLIST></OFX>";
        let got = read("ofx", file, "EUR");
        assert_eq!(statuses(&got), want(&[
            ("t1", "ready", true), ("t2", "Zero", false), ("t3", "Corrected", false), ("t4", "Corrected", false),
            ("t5", "attention Date", true), ("t6", "attention Amount", true), ("t7", "attention Amount", true),
        ]));
        assert_eq!(got.lines[4].status, Status::Attention { field: Field::Date, column: None, reason: "Invalid OFX posted date ‘2026-1O-04’".into() });
        assert_eq!(got.lines[4].written.date, "2026-1O-04");
        // Today's import stops at the first of them.
        assert_eq!(got.stop.as_deref(), Some("Invalid OFX posted date ‘2026-1O-04’"));
        assert_eq!(transactions("ofx", file, "EUR", "ymd").unwrap_err(), "Invalid OFX posted date ‘2026-1O-04’");
        let qif = "!Type:Bank\nD10/01/2026\nT-3.20\nPBakery\n^\nD10/02/2026\nT0.00\nPCheck\n^\nD10/03/2026\nTn/a\nPGrocer\n^\nT-4.00\nPNo date\n^\n";
        assert_eq!(statuses(&read("qif", qif, "EUR")), want(&[
            ("r1", "ready", true), ("r2", "Zero", false), ("r3", "attention Amount", true), ("r4", "attention Date", true),
        ]));
        let sta = ":20:X\n:25:A\n:60F:C261001EUR0,00\n:61:2610011001DR3,20NMSCNONREF\n:86:Bakery\n:61:2610021002D0,00NMSCNONREF\n\
:61:2610031003X9,00NMSCNONREF\n:61:26100X1004D9,00NMSCNONREF\n";
        assert_eq!(statuses(&read("mt940", sta, "EUR")), want(&[
            ("t1", "ready", true), ("t2", "Zero", false), ("t3", "attention Amount", true), ("t4", "attention Date", true),
        ]));
        let doc = CAMT.replace("<Amt Ccy=\"EUR\">64.20", "<Amt Ccy=\"USD\">64.20");
        assert_eq!(statuses(&read("camt053", &doc, "EUR")), want(&[
            ("e1", "OtherCurrency", false), ("e2", "Pending", false), ("e3.1", "ready", true), ("e3.2", "ready", true),
        ]));
    }

    #[test]
    fn a_file_with_several_accounts_reads_the_one_chosen_in_its_own_currency() {
        let file = ofx(&[
            ("111", "USD", &[("DEBIT", "20261001", "-4.50", "Cafe")]),
            ("222", "EUR", &[("CREDIT", "20261002", "100.00", "Interest"), ("DEBIT", "20261003", "-9.00", "Fee")]),
        ]);
        let usd = read("ofx", &file, "USD");
        let labels: Vec<&str> = usd.sources.iter().map(|s| s.label.as_str()).collect();
        assert_eq!((labels, usd.source), (vec!["111", "222"], 0));
        assert_eq!(statuses(&usd), want(&[("t1", "ready", true), ("t2", "OtherAccount", false), ("t3", "OtherAccount", false)]));
        // The savings account chosen in a USD budget: its lines are in EUR.
        let savings = lines("ofx", &file, "USD", &LineOptions { source: Some("222"), ..LineOptions::default() }).unwrap();
        assert_eq!(statuses(&savings), want(&[("t1", "OtherAccount", false), ("t2", "OtherCurrency", false), ("t3", "OtherCurrency", false)]));
        // A EUR budget reads the EUR account, where today's import stops at the file's first currency.
        let eur = read("ofx", &file, "EUR");
        assert_eq!((eur.source, amounts(&eur)), (1, vec![None, Some(10000), Some(-900)]));
        assert_eq!(transactions("ofx", &file, "EUR", "ymd").unwrap_err(), "File currency USD differs from EUR.");
        assert_eq!(transactions("ofx", &file, "USD", "ymd").unwrap().len(), 1);

        // Quicken's every-account export: today's import reads the first.
        let qif = fixture("us-quicken-all-accounts.qif");
        let all = read("qif", &qif, "USD");
        assert!(all.sources.len() >= 2, "{:?}", all.sources);
        let today: Vec<_> = transactions("qif", &qif, "USD", "mdy").unwrap().into_iter().map(|t| (t.date, t.amount)).collect();
        let first: Vec<_> = all.lines.iter().filter_map(|l| l.transaction.as_ref()).map(|t| (t.date.clone(), t.amount)).collect();
        assert_eq!(first, today);
        let second = lines("qif", &qif, "USD", &LineOptions { source: Some(&all.sources[1].label), ..LineOptions::default() }).unwrap();
        assert_eq!(second.source, 1);
        assert!(second.lines.iter().any(|l| l.status == Status::Ready));
        let elsewhere = second.lines.iter().filter(|l| matches!(l.status, Status::LeftOut { reason: Reason::OtherAccount, .. })).count();
        assert_eq!(elsewhere, second.sources.iter().enumerate().filter(|(i, _)| *i != 1).map(|(_, s)| s.lines).sum::<usize>());

        // CAMT: one statement a day for one account, another account's in USD.
        let usd_account = CAMT.replace("DE00123456780000000000", "US00999").replace("EUR", "USD");
        let doc = CAMT.replace("</BkToCstmrStmt>", &format!("{}</BkToCstmrStmt>", &usd_account[usd_account.find("<Stmt>").unwrap()..usd_account.find("</BkToCstmrStmt>").unwrap()]));
        let both = read("camt053", &doc, "USD");
        assert_eq!(both.sources.iter().map(|s| (s.label.as_str(), s.currency.as_deref())).collect::<Vec<_>>(), [("DE00123456780000000000", Some("EUR")), ("US00999", Some("USD"))]);
        assert_eq!(both.source, 1);
        assert_eq!(both.lines.iter().filter(|l| l.status == Status::Ready).count(), 3);
        assert!(transactions("camt053", &doc, "USD", "ymd").unwrap_err().contains("EUR"));
        assert_eq!(transactions("camt053", &doc, "EUR", "ymd").unwrap().len(), 3);

        // MT940: the second account's statements are in USD; today's import stops at them.
        let sta = ":20:X\n:25:EUR-1\n:60F:C261001EUR0,00\n:61:2610011001DR3,20NMSCNONREF\n:86:Bakery\n-\n\
:20:Y\n:25:USD-2\n:60F:C261001USD0,00\n:61:2610021002CR50,00NMSCNONREF\n:86:Refund\n-";
        assert!(transactions("mt940", sta, "EUR", "ymd").unwrap_err().contains("USD"));
        let usd = read("mt940", sta, "USD");
        assert_eq!((usd.source, statuses(&usd)), (1, want(&[("t1", "OtherAccount", false), ("t2", "ready", true)])));
        let eur = read("mt940", sta, "EUR");
        assert_eq!((eur.source, statuses(&eur)), (0, want(&[("t1", "ready", true), ("t2", "OtherAccount", false)])));
    }

    #[test]
    fn an_ofx_gets_its_signs_from_the_amounts_that_read() {
        let file = |first: &str, date: &str| ofx(&[("1", "EUR", &[("DEBIT", date, first, "Cafe"), ("POS", "20261002", "3.00", "Shop"), ("CREDIT", "20261003", "100.00", "Pay")])]);
        // Every amount positive: the types say which way the money went.
        let clean = read("ofx", &file("4.50", "20261001"), "EUR");
        assert_eq!((clean.signs, amounts(&clean)), (Some(Signs { by_type: true, unconfirmed: false }), vec![Some(-450), Some(-300), Some(10000)]));
        // The only negative amount, on a line whose date is damaged, still says the signs are written.
        let damaged = read("ofx", &file("-4.50", "2026x001"), "EUR");
        assert_eq!((damaged.signs, amounts(&damaged)), (None, vec![None, Some(300), Some(10000)]));
        // The only negative amount doesn't read: nothing flipped, it's asked.
        let unread = read("ofx", &file("-4,5x", "20261001"), "EUR");
        assert_eq!((unread.signs, amounts(&unread)), (Some(Signs { by_type: false, unconfirmed: true }), vec![None, Some(300), Some(10000)]));
        let told = |by_type| LineOptions { signs_by_type: Some(by_type), ..LineOptions::default() };
        let yes = lines("ofx", &file("-4,5x", "20261001"), "EUR", &told(true)).unwrap();
        assert_eq!((yes.signs, amounts(&yes)[1]), (Some(Signs { by_type: true, unconfirmed: false }), Some(-300)));
        // An amount given for it isn't evidence: still asked.
        let given = LineOptions { fields: HashMap::from([("t1".to_string(), vec![(Field::Amount, "-4.50".to_string())])]), ..LineOptions::default() };
        let fixed = lines("ofx", &file("-4,5x", "20261001"), "EUR", &given).unwrap();
        assert_eq!((fixed.signs.unwrap().unconfirmed, amounts(&fixed)), (true, vec![Some(-450), Some(300), Some(10000)]));
        // Another account's negative amount doesn't decide this one's.
        let two = ofx(&[
            ("1", "EUR", &[("DEBIT", "20261001", "-4.50", "Cafe")]),
            ("2", "EUR", &[("POS", "20261002", "3.00", "Shop"), ("CREDIT", "20261003", "100.00", "Pay")]),
        ]);
        let second = lines("ofx", &two, "EUR", &LineOptions { source: Some("2"), ..LineOptions::default() }).unwrap();
        assert_eq!((second.signs.map(|s| s.by_type), amounts(&second)), (Some(true), vec![None, Some(-300), Some(10000)]));
        assert_eq!(read("ofx", &two, "EUR").signs, None);
    }

    #[test]
    fn a_damaged_payment_in_a_batch_changes_only_its_own_line() {
        let clean = read("camt053", CAMT, "EUR");
        let ids = |read: &StatementLines| -> Vec<(String, Option<BankTransaction>)> {
            read.lines.iter().map(|l| (l.key.clone(), l.transaction.clone())).collect()
        };
        assert_eq!(clean.lines[2].transaction.as_ref().unwrap().fit, "2026-10-03/10000/BATCH/A");
        let batch = &clean.batches[0];
        assert_eq!((batch.entry.as_str(), batch.payments.as_slice(), batch.adds_up), ("e3", &[2, 3][..], Some(true)));
        // The batch is named by the ID today's import gives the whole entry.
        assert_eq!(batch.whole.as_ref().unwrap().fit, "2026-10-03/30000/BATCH");
        let broken = CAMT.replace(">100.00<", ">1OO.00<");
        let damaged = read("camt053", &broken, "EUR");
        assert_eq!(statuses(&damaged)[2..], want(&[("e3.1", "attention Amount", true), ("e3.2", "ready", true)]));
        assert_eq!(ids(&damaged)[3], ids(&clean)[3]);
        assert_eq!(damaged.lines[3].original, clean.lines[3].original);
        assert_eq!(damaged.batches[0].whole, clean.batches[0].whole);
        // Today's import reads such an entry whole, as it still does.
        let today: Vec<String> = transactions("camt053", &broken, "EUR", "ymd").unwrap().into_iter().map(|t| t.fit).collect();
        assert_eq!(today, ["2026-10-01/-6420/REF-1", "2026-10-03/30000/BATCH"]);
        // Repaired, it reads as the clean file does.
        let given = |amount: &str| LineOptions { fields: HashMap::from([("e3.1".to_string(), vec![(Field::Amount, amount.to_string())])]), ..LineOptions::default() };
        let repaired = lines("camt053", &broken, "EUR", &given("100.00")).unwrap();
        assert_eq!(ids(&repaired), ids(&clean));
        // A wrong amount for it: the payments no longer add up, and which is wrong can't be told.
        let wrong = lines("camt053", &broken, "EUR", &given("150.00")).unwrap();
        assert_eq!(statuses(&wrong)[2..], want(&[("e3.1", "attention Amount", true), ("e3.2", "attention Amount", true)]));
        assert_eq!(wrong.lines[3].status, Status::Attention { field: Field::Amount, column: None, reason: "These payments add up to 350.00, not the entry's 300.00.".into() });
        assert_eq!(wrong.batches[0].adds_up, Some(false));
        // Payments that don't add up in the file: imported as one entry when asked, or by today's import.
        let off = CAMT.replace(">200.00<", ">250.00<");
        assert_eq!(statuses(&read("camt053", &off, "EUR"))[2..], want(&[("e3.1", "attention Amount", true), ("e3.2", "attention Amount", true)]));
        assert!(read("camt053", &off, "EUR").lines[2].original.is_none());
        let whole = lines("camt053", &off, "EUR", &LineOptions { whole_batches: HashSet::from(["e3".to_string()]), ..LineOptions::default() }).unwrap();
        assert_eq!(statuses(&whole)[2..], want(&[("e3", "ready", true), ("e3.1", "InWholeBatch", false), ("e3.2", "InWholeBatch", false)]));
        assert_eq!(whole.lines[2].transaction.as_ref().unwrap().fit, "2026-10-03/30000/BATCH");
        assert_eq!(whole.batches[0].line, Some(2));
        assert_eq!(transactions("camt053", &off, "EUR", "ymd").unwrap()[1].fit, "2026-10-03/30000/BATCH");
    }

    #[test]
    fn a_repaired_mt940_line_keeps_its_reference() {
        let clean = read("mt940", STA, "EUR");
        let broken = STA.replace("DR64,20NDDT", "DR6x,20NDDT");
        let damaged = read("mt940", &broken, "EUR");
        assert!(matches!(damaged.lines[0].status, Status::Attention { field: Field::Amount, .. }));
        assert_eq!(damaged.lines[0].written.payee, "Stadtwerke Beispielstadt");
        let given = LineOptions { fields: HashMap::from([("t1".to_string(), vec![(Field::Amount, "-64.20".to_string())])]), ..LineOptions::default() };
        let repaired = lines("mt940", &broken, "EUR", &given).unwrap();
        assert_eq!(repaired.lines[0].transaction, clean.lines[0].transaction);
        assert_eq!(repaired.lines[0].transaction.as_ref().unwrap().fit, "2026-10-01/-6420/4711");
    }

    #[test]
    fn included_lines_read_and_need_what_they_still_lack() {
        let include = |keys: &[&str]| LineOptions { include: keys.iter().map(|k| k.to_string()).collect(), ..LineOptions::default() };
        // A pending entry reads; one in another currency needs the amount in the budget's.
        let doc = CAMT.replace("<Amt Ccy=\"EUR\">64.20", "<Amt Ccy=\"USD\">64.20");
        let all = lines("camt053", &doc, "EUR", &include(&["e1", "e2", "e3.1"])).unwrap();
        assert_eq!(statuses(&all)[..2], want(&[("e1", "attention Amount", false), ("e2", "ready", false)]));
        assert_eq!(all.lines[0].status, Status::Attention { field: Field::Amount, column: None, reason: "This line is in USD: enter the amount in EUR.".into() });
        assert_eq!(all.lines[1].transaction.as_ref().map(|t| t.amount), Some(-1290));
        let mut given = include(&["e1"]);
        given.fields.insert("e1".into(), vec![(Field::Amount, "-59.90".into())]);
        assert_eq!(lines("camt053", &doc, "EUR", &given).unwrap().lines[0].transaction.as_ref().map(|t| t.amount), Some(-5990));
        // Nothing moving, and a correction, can't be included.
        let file = "<OFX><STMTTRN><DTPOSTED>20261002<TRNAMT>0.00<FITID>b</STMTTRN>\
<STMTTRN><DTPOSTED>20261003<TRNAMT>-9.00<FITID>c</STMTTRN><STMTTRN><DTPOSTED>20261003<TRNAMT>-9.00<FITID>d<CORRECTFITID>c<CORRECTACTION>DELETE</STMTTRN></OFX>";
        assert_eq!(statuses(&lines("ofx", file, "EUR", &include(&["t1", "t2", "t3"])).unwrap()), want(&[
            ("t1", "Zero", false), ("t2", "Corrected", false), ("t3", "Corrected", false),
        ]));
        // A date given for a line without one.
        let qif = "!Type:Bank\nD10/01/2026\nT-3.20\nPBakery\n^\nT-4.00\nPNo date\n^\n";
        let mut dated = LineOptions::default();
        dated.fields.insert("r2".into(), vec![(Field::Date, "2026-10-02".into()), (Field::Payee, "Kiosk".into())]);
        let read = lines("qif", qif, "EUR", &dated).unwrap();
        let t = read.lines[1].transaction.as_ref().unwrap();
        assert_eq!((t.date.as_str(), t.payee.as_str(), t.amount), ("2026-10-02", "Kiosk", -400));
    }

    #[test]
    fn qif_dates_that_dont_settle_their_order_ask() {
        let qif = |first: &str| format!("!Type:Bank\nD{first}\nT-1.00\nPA\n^\nD01/02/2026\nT-2.00\nPB\n^\n");
        let settled = read("qif", &qif("31/10/2026"), "EUR");
        let reading = settled.dates.as_ref().unwrap();
        assert_eq!((reading.order, reading.unconfirmed), ("dmy", false));
        assert_eq!(settled.lines[1].transaction.as_ref().unwrap().date, "2026-02-01");
        // The deciding date damaged: that line needs its date, and the order is asked, not changed.
        let open = read("qif", &qif("31/1O/2026"), "EUR");
        assert!(open.dates.as_ref().unwrap().unconfirmed);
        assert!(matches!(open.lines[0].status, Status::Attention { field: Field::Date, .. }));
        let confirmed = lines("qif", &qif("31/1O/2026"), "EUR", &LineOptions { date_order: "dmy", confirmed: true, ..LineOptions::default() }).unwrap();
        assert_eq!((confirmed.dates.unwrap().unconfirmed, confirmed.lines[1].transaction.as_ref().unwrap().date.as_str()), (false, "2026-02-01"));
    }

    #[test]
    fn a_qif_file_without_its_type_line_is_known_by_its_records() {
        let records = "D10/31/2026\nT-12.50\nPShop\n^\nD11/01/2026\nU1,000.00\nPPay\n^\n";
        assert_eq!(sniff(records), Some("qif"));
        assert_eq!(read("qif", records, "EUR").lines.len(), 2);
        // Not a CSV that merely starts with a D.
        assert_eq!(sniff("Date,Amount\n2026-10-01,-1\n"), None);
        assert_eq!(sniff("D1,T2\n^,x\n"), None);
    }

    #[test]
    fn damaged_statements_never_panic() {
        // FUZZ_ITERATIONS=20000 for the full run.
        let runs: usize = std::env::var("FUZZ_ITERATIONS").ok().and_then(|n| n.parse().ok()).unwrap_or(400);
        let mut files: Vec<(&str, String)> = vec![("camt053", CAMT.into()), ("mt940", STA.into())];
        for (file, format) in [
            ("de-sparkasse.camt053.xml", "camt053"), ("camt052-intraday.xml", "camt053"), ("camt053-pending.xml", "camt053"),
            ("de-postbank.mt940.sta", "mt940"), ("nl-ing.mt940.sta", "mt940"), ("us-chase.qfx", "ofx"), ("ofx2-xml.ofx", "ofx"),
            ("qif-iso-dates.qif", "qif"), ("us-quicken-all-accounts.qif", "qif"),
        ] {
            files.push((format, fixture(file)));
        }
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as usize
        };
        const BITS: &[u8] = b"<>/:^-+0123456789,.CDRNFSTOX \n\r\xc3\xa9";
        for run in 0..runs {
            let (format, text) = &files[run % files.len()];
            let mut bytes = text.as_bytes().to_vec();
            for _ in 0..1 + next() % 6 {
                let at = next() % bytes.len().max(1);
                match next() % 5 {
                    0 if at < bytes.len() => bytes[at] = BITS[next() % BITS.len()],
                    1 if at < bytes.len() => {
                        bytes.remove(at);
                    }
                    2 => bytes.insert(at.min(bytes.len()), BITS[next() % BITS.len()]),
                    3 => bytes.truncate(at),
                    _ => {
                        let len = 1 + next() % 40;
                        let from = next() % bytes.len().max(1);
                        let piece: Vec<u8> = bytes.iter().skip(from).take(len).copied().collect();
                        let at = at.min(bytes.len());
                        bytes.splice(at..at, piece);
                    }
                }
            }
            let text = String::from_utf8_lossy(&bytes);
            let _ = transactions(format, &text, "EUR", "dmy");
            let Ok(first) = lines(format, &text, "EUR", &LineOptions::default()) else { continue };
            // Everything included, every field given, every batch whole, each account in turn.
            let mut o = LineOptions { signs_by_type: Some(run % 2 == 0), confirmed: true, date_order: "mdy", ..LineOptions::default() };
            for (i, l) in first.lines.iter().enumerate() {
                o.include.insert(l.key.clone());
                if i % 2 == 0 {
                    o.fields.insert(l.key.clone(), vec![(Field::Date, "2026-10-01".into()), (Field::Amount, format!("-{i}.5"))]);
                }
            }
            o.whole_batches = first.batches.iter().filter(|_| run % 3 == 0).map(|b| b.entry.clone()).collect();
            for s in &first.sources {
                let read = lines(format, &text, "EUR", &LineOptions { source: Some(&s.label), ..o.clone() }).unwrap();
                for l in &read.lines {
                    assert_eq!(l.status == Status::Ready, l.transaction.is_some(), "{format} {}", l.key);
                }
            }
        }
    }

    #[test]
    fn reads_camt053_booked_entries_batches_and_parties() {
        let doc = CAMT;
        assert_eq!(sniff(doc), Some("camt053"));
        let rows = transactions("camt053", doc, "EUR", "ymd").unwrap();
        let read: Vec<_> = rows.iter().map(|t| (t.date.as_str(), t.payee.as_str(), t.memo.as_str(), t.amount)).collect();
        assert_eq!(read, [
            ("2026-10-01", "Stadtwerke & Co", "Abschlag Strom Oktober", -6420),
            ("2026-10-03", "Anna", "", 10000),
            ("2026-10-03", "Ben", "", 20000),
        ]);
        assert_eq!((rows[0].fit.as_str(), rows[1].fit.as_str()), ("2026-10-01/-6420/REF-1", "2026-10-03/10000/BATCH/A"));
        assert!(transactions("camt053", &doc.replace("Ccy=\"EUR\">64.20", "Ccy=\"USD\">64.20"), "EUR", "ymd").err().unwrap().contains("USD"));
    }

    #[test]
    fn reads_mt940_with_german_structured_details_across_days() {
        let sta = STA;
        assert_eq!(sniff(sta), Some("mt940"));
        let rows = transactions("mt940", sta, "EUR", "ymd").unwrap();
        let read: Vec<_> = rows.iter().map(|t| (t.date.as_str(), t.payee.as_str(), t.memo.as_str(), t.amount)).collect();
        assert_eq!(read, [
            ("2026-10-01", "Stadtwerke Beispielstadt", "Abschlag Strom Oktober", -6420),
            ("2026-12-31", "Gehalt Dezember Arbeitgeber GmbH", "", 245000),
            ("2027-01-02", "GUTSCHRIFT", "", 1290),
        ]);
        assert_eq!(rows[0].fit, "2026-10-01/-6420/4711");
        // A booking date in January for a value date in December belongs to the next year.
        let wrap = ":20:X\n:25:A\n:60F:C261231EUR0,00\n:61:2612310102D5,00NMSCNONREF\n:86:Fee\n";
        assert_eq!(transactions("mt940", wrap, "EUR", "ymd").unwrap()[0].date, "2027-01-02");
    }

    #[test]
    fn reads_the_details_dutch_and_polish_banks_write() {
        // ING and ABN AMRO: keys and values between slashes.
        let ing = "/TRTP/SEPA OVERBOEKING/IBAN/NL00INGB0001234567/BIC/INGBNL2A/NAME/J. Jansen/REMI/USTD//Huur oktober/EREF/NOTPROVIDED";
        assert_eq!(details(ing), ("J. Jansen".into(), "Huur oktober".into()));
        let rabo = "/ORDP//NAME/Albert Heijn 1234/ADDR/Amsterdam/REMI/STRD/CUR/1234567890/";
        assert_eq!(details(rabo), ("Albert Heijn 1234".into(), "1234567890".into()));
        let cntp = "/CNTP/NL00RABO0123456789/RABONL2U/Stichting Voorbeeld/UTRECHT/REMI/USTD//Gift/";
        assert_eq!(details(cntp), ("Stichting Voorbeeld".into(), "Gift".into()));
        // Polish banks number the subfields after ~ or ^.
        assert_eq!(details("020~00VE02~20Faktura 12/2026~21za paliwo~32ORLEN SA~33Stacja 4"), ("ORLEN SAStacja 4".into(), "Faktura 12/2026za paliwo".into()));
        assert_eq!(details("^00PRZELEW^20Czynsz^32Spółdzielnia"), ("Spółdzielnia".into(), "Czynsz".into()));
        // Free text, slashes and all.
        assert_eq!(details("BEA   NR:1234   01.10.26/12.34 ALBERT HEIJN"), ("BEA NR:1234 01.10.26/12.34 ALBERT HEIJN".into(), String::new()));
    }

    #[test]
    fn sniffs_formats_and_reads_forgiving_xml() {
        assert_eq!(sniff("OFXHEADER:100\nDATA:OFXSGML"), Some("ofx"));
        assert_eq!(sniff("\u{feff}  !Type:Bank\nD10/14/2026"), Some("qif"));
        assert_eq!(sniff("Date,Payee,Amount\n2026-10-14,Shop,-1.00"), None);
        let n = xml("<a x='1'><b>one &lt;two&gt; &#x20AC;</b><c/><d><e>unclosed</d><![CDATA[<raw>]]></a>").unwrap();
        let a = n.child("a").unwrap();
        assert_eq!((a.attr("x"), a.text_at(&["b"])), (Some("1"), Some("one <two> €")));
        assert_eq!(a.text_at(&["d", "e"]), Some("unclosed"));
        assert!(a.text.contains("<raw>"));
    }
}
