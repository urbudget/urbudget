use crate::bank_csv::{self, Columns};
use crate::t;
use crate::{domain::*, ledger, workflows};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{hash_map::DefaultHasher, BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub transaction_id: String,
    pub split_index: usize,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub date: String,
    pub payee: String,
    pub memo: String,
    pub amount: i64,
    #[serde(default)]
    pub duplicate: bool,
    #[serde(default)]
    pub candidates: Vec<String>,
    /// These account sides already have a bank identity. They may match another source,
    /// but only after an explicit choice, since a different receipt can have the same amount.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previously_matched_candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_details: Vec<Candidate>,
    /// Candidates in another account that matching makes into a transfer with this one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transfer_candidates: Vec<String>,
    /// The transfer is clear enough to choose for the row (see `transfer_offers`); otherwise it's
    /// only suggested.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transfer_sure: bool,
    /// Without a bank ID, identical rows in separate files cannot be distinguished for sure.
    #[serde(default)]
    pub idless: bool,
    /// An older import used an identity without the memo. Ask rather than discard or duplicate it.
    #[serde(default)]
    pub possible_duplicate: bool,
    /// Identity shared by indistinguishable ID-less rows, independent of their file position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Explicitly import another identical ID-less row. The window keeps this ID across retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_id: Option<String>,
    /// The payee a rule gives the row, when it differs from the bank's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename: Option<String>,
    /// The category the payee rules give it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// When the bank says it happened (RFC 3339), for bank sync: imported, it's when the
    /// transaction was added. A file gives no time; its rows are added when they're imported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// More IDs it's known by: a line that read only once fixed is known by the line as the file
    /// wrote it too, so the same file again finds it imported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The batch it belongs to, by the ID of its entry as one transaction (which is this row's
    /// own ID when it's that): a batch's payments and the whole entry are never both imported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<String>,
    /// The purchase it may be a refund of (see `refund_offers`); imported, it takes that
    /// purchase's category when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refund_of: Option<RefundOf>,
}
/// A purchase a row of money in may be a refund of.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RefundOf {
    /// The purchase (the latest, of several).
    pub id: String,
    pub date: String,
    pub payee: String,
    /// Cents, negative.
    pub amount: i64,
    /// The purchases' category, when they agree on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    /// How many purchases it may be a refund of.
    pub purchases: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub row: Row,
    pub action: String,
    pub match_id: Option<String>,
}
/// A lossless pool of matching choices for native clients. Large statements commonly have
/// hundreds of rows sharing thousands of candidates; repeating those arrays exhausts a
/// phone's JSON heap before the first review row can render.
#[derive(Debug, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSet {
    pub candidates: Vec<String>,
    pub previously_matched_candidates: Vec<String>,
    pub candidate_details: Vec<Candidate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub transfer_candidates: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub transfer_sure: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactRow {
    #[serde(flatten)]
    pub row: Row,
    pub candidate_set: usize,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactReview {
    pub items: Vec<CompactRow>,
    pub candidate_sets: Vec<CandidateSet>,
}
/// Currency signs banks write beside amounts. A sign such as "$" stands for several
/// currencies, so only a written code ("USD") is checked against the budget's.
const SIGNS: [&str; 15] = [
    "$", "€", "£", "¥", "₹", "₩", "₪", "₺", "₽", "₱", "฿", "₫", "₴", "ر.س", "د.إ",
];
/// A three-letter currency code at the start or end of an amount, and the amount without it.
fn code(t: &str) -> Option<(&str, &str)> {
    let is_code = |c: &str| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase());
    let letter = |c: char| c.is_ascii_alphabetic();
    if let (Some(c), Some(rest)) = (t.get(..3), t.get(3..)) {
        if is_code(c) && !rest.starts_with(letter) {
            return Some((c, rest));
        }
    }
    let (rest, c) = t.split_at_checked(t.len().checked_sub(3)?)?;
    (is_code(c) && !rest.ends_with(letter)).then_some((c, rest))
}
/// An amount in cents: "1,234.56", "1.234,56", "1 234,56", "(12.34)", "12.34-", "$12.34" or
/// "12.34 USD". One that could be read two ways, such as "1.234", is refused, never guessed.
pub(crate) fn money(s: &str, currency: &str) -> Result<i64, String> {
    let bad = || t!("Invalid amount: {amount}", amount = s.trim());
    let mut t = s.trim();
    let (mut negative, mut credit) = (false, false);
    if let Some(inner) = t.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        negative = true;
        t = inner;
    }
    // Peel minus and plus signs, currency signs and codes off both ends.
    loop {
        let before = t;
        t = t.trim();
        if let Some(rest) = t.strip_prefix(['-', '−']).or(t.strip_suffix(['-', '−'])) {
            negative = true;
            t = rest;
        }
        t = t.strip_prefix('+').unwrap_or(t);
        for sign in SIGNS {
            t = t.strip_prefix(sign).or(t.strip_suffix(sign)).unwrap_or(t);
        }
        // "US$", "C$", "R$": a dollar sign with its country's letters.
        let letters = t.bytes().take_while(u8::is_ascii_uppercase).count();
        if (1..=3).contains(&letters) && t[letters..].starts_with('$') {
            t = &t[letters + 1..];
        }
        if let Some((c, rest)) = code(t) {
            if c != currency {
                return Err(t!("The amount ‘{amount}’ is in {code}, but this budget uses {currency}.", amount = s.trim(), code = c, currency));
            }
            t = rest;
        }
        // "12.34 CR" is money in and "12.34 DR" money out, as some statements write them.
        if let Some(rest) = t.strip_suffix("CR").or(t.strip_suffix("Cr")) {
            credit = true;
            t = rest;
        } else if let Some(rest) = t.strip_suffix("DR").or(t.strip_suffix("Dr")) {
            negative = true;
            t = rest;
        }
        if t == before {
            break;
        }
    }
    // The last dot or comma marks the cents, unless only grouping fits: "1,234" and
    // "1.234.567" have none. A single dot before three digits could be either.
    let (whole, frac, point) = match t.rfind(['.', ',']) {
        None => (t, "", None),
        Some(i) => {
            let mark = &t[i..i + 1];
            let other = if mark == "." { "," } else { "." };
            let (left, right) = (&t[..i], &t[i + 1..]);
            if left.contains(other) || !left.contains(mark) && right.len() != 3 {
                (left, right, Some(mark))
            } else if mark == "," || left.contains(mark) {
                (t, "", None)
            } else {
                return Err(t!("Cannot tell whether the dot in ‘{amount}’ separates thousands or cents.", amount = s.trim()));
            }
        }
    };
    // Thousands may be grouped by the other mark, a space or an apostrophe, in groups of three
    // (or Indian lakhs, "1,23,456"). The mark used for cents can't appear among them.
    let groups: Vec<&str> = whole
        .split(|c: char| matches!(c, '.' | ',' | '\'' | '’') || c.is_whitespace())
        .collect();
    let grouped = groups.len() == 1
        || groups[0].len() <= 3
            && groups[1..].iter().all(|g| (2..=3).contains(&g.len()))
            && groups.last().is_some_and(|g| g.len() == 3);
    if !grouped
        || point.is_some_and(|p| whole.contains(p))
        || groups
            .iter()
            .any(|g| g.is_empty() || !g.bytes().all(|c| c.is_ascii_digit()))
        || frac.len() > 2
        || !frac.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    let sign = if negative && !credit { -1 } else { 1 };
    let whole = groups
        .concat()
        .parse::<i64>()
        .map_err(|_| t!("Amount too large"))?;
    let n = whole
        .checked_mul(100)
        .and_then(|n| n.checked_add(format!("{frac:0<2}").parse::<i64>().ok()?))
        .ok_or_else(|| t!("Amount too large"))?;
    if n > 1_000_000_000_000 {
        return Err(t!("Amount exceeds supported range"));
    }
    Ok(n * sign)
}
pub(crate) fn date(s: &str, format: &str) -> Result<String, String> {
    // A time after the date isn't needed: "2026-10-15 14:03:22", "2026-10-15T14:03:22Z",
    // "15 Oct 2026 10:00". It starts at the last space (or T between digits) before its colon.
    // FNB writes its dates in single quotes.
    let s = s.trim().trim_matches('\'').trim();
    let day = s.find(':').and_then(|colon| {
        let head = &s[..colon];
        head.char_indices().rev().find(|&(i, c)| {
            c.is_whitespace()
                || c == 'T' && head[..i].ends_with(|p: char| p.is_ascii_digit()) && head[i + 1..].starts_with(|n: char| n.is_ascii_digit())
        })
    });
    let day = match day {
        Some((i, _)) if i > 0 => &s[..i],
        _ => s,
    };
    let raw = day.trim_end_matches('.').replace('\'', "/").replace(' ', "");
    // Two-digit years first: %Y also reads "26" as the year 26, so "10/17/26" would become 0026.
    // Dots and month names before the month ("15 Oct 2026") are written day first.
    let patterns = match format {
        "dmy" => vec![
            "%d/%m/%y", "%d-%m-%y", "%d.%m.%y", "%d/%m/%Y", "%d-%m-%Y", "%d.%m.%Y", "%d%b%y", "%d-%b-%y", "%d/%b/%y",
            "%d%b%Y", "%d-%b-%Y", "%d/%b/%Y",
        ],
        "mdy" => vec!["%m/%d/%y", "%m-%d-%y", "%m/%d/%Y", "%m-%d-%Y", "%b%d,%Y", "%b%d%Y"],
        _ => vec!["%Y-%m-%d", "%Y/%m/%d", "%Y.%m.%d", "%Y%m%d"],
    };
    let parsed = std::iter::once("%Y-%m-%d").chain(patterns).filter_map(|p| NaiveDate::parse_from_str(&raw, p).ok())
        // A spreadsheet's serial day number, when a bank's file went through Excel: 46310.
        .chain((raw.len() == 5).then(|| raw.parse::<i64>().ok()).flatten().filter(|n| (20_000..=80_000).contains(n))
            .and_then(|n| NaiveDate::from_ymd_opt(1899, 12, 30)?.checked_add_signed(chrono::Duration::days(n))))
        .chain(worded(day));
    for d in parsed {
        let normalized = d.format("%Y-%m-%d").to_string();
        if d.year() >= 1900 && valid_date(&normalized) {
            return Ok(normalized);
        }
    }
    Err(t!("Cannot read date ‘{date}’. Select the matching date format.", date = s))
}
/// A date with its month as a word, in the languages banks write: "15. Okt. 2026", "15-ene-2026",
/// "2026 oct. 15", "Wed, 15 October 2026". The numbers around the month are the day and the
/// year, year first only when it has four digits. Without a year ("26 Sep", as FNB writes), it's
/// the latest such day that isn't more than a month ahead.
fn worded(s: &str) -> Option<NaiveDate> {
    let mut month = None;
    let mut numbers = vec![];
    for token in s.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
        if token.bytes().all(|b| b.is_ascii_digit()) {
            numbers.push(token);
            continue;
        }
        if !token.chars().all(char::is_alphabetic) {
            return None;
        }
        let word = bank_csv::plain(token);
        // An abbreviation by its first three letters, a longer word only as a month's whole name:
        // "Description 1" isn't a date in December.
        let found = match word.as_str() {
            // French June and July share their first three letters; Afrikaans December is "Des".
            "juin" => Some(6),
            "juil" => Some(7),
            "des" => Some(12),
            short if short.chars().count() <= 5 => {
                short.get(..3).and_then(|start| MONTHS.iter().position(|names| names.contains(&start))).map(|m| m as u32 + 1)
            }
            long => NAMED.iter().position(|names| names.contains(&long)).map(|m| m as u32 + 1),
        };
        match found {
            Some(m) if month.is_none() => month = Some(m),
            None if month.is_none() && WEEKDAYS.contains(&word.as_str()) => {}
            _ => return None,
        }
    }
    let (day, year) = match numbers[..] {
        [a, b] if a.len() == 4 => (b, a),
        [a, b] => (a, b),
        [day] if day.len() <= 2 => {
            let (today, day) = (Utc::now().date_naive(), day.parse().ok()?);
            let this = NaiveDate::from_ymd_opt(today.year(), month?, day)?;
            return if this > today + chrono::Duration::days(31) { NaiveDate::from_ymd_opt(today.year() - 1, month?, day) } else { Some(this) };
        }
        _ => return None,
    };
    let year: i32 = year.parse().ok()?;
    let year = if year < 100 { 2000 + year } else { year };
    NaiveDate::from_ymd_opt(year, month?, day.parse().ok()?)
}
/// The first three letters of each month's name: English, German, Dutch, French, Spanish,
/// Italian, Portuguese, Polish, Afrikaans and the Nordic languages.
const MONTHS: [&[&str]; 12] = [
    &["jan", "ene", "gen", "sty"],
    &["feb", "fev", "lut"],
    &["mar", "mrz", "mrt", "maa"],
    &["apr", "abr", "avr", "kwi"],
    &["may", "mai", "mei", "mag", "maj"],
    &["jun", "giu", "cze"],
    &["jul", "lug", "lip"],
    &["aug", "ago", "aou", "sie"],
    &["sep", "set", "wrz"],
    &["oct", "okt", "ott", "out", "paz"],
    &["nov", "lis"],
    &["dec", "dez", "dic", "gru"],
];
/// Months' whole names, after `plain`, in the same languages.
const NAMED: [&[&str]; 12] = [
    &["january", "januar", "januari", "janvier", "enero", "gennaio", "janeiro", "stycznia", "styczen", "januarie"],
    &["february", "februar", "februari", "fevrier", "febrero", "febbraio", "fevereiro", "lutego", "februarie"],
    &["march", "maart", "marzec"],
    &["aprile", "kwietnia", "kwiecien"],
    &["maggio"],
    &["giugno", "czerwca", "czerwiec"],
    &["juillet", "luglio", "lipiec"],
    &["august", "augustus", "augusti", "sierpnia", "sierpien"],
    &["september", "septembre", "septiembre", "setiembre", "settembre", "setembro", "wrzesnia", "wrzesien"],
    &["october", "oktober", "octobre", "octubre", "ottobre", "outubro", "pazdziernika", "pazdziernik"],
    &["november", "novembre", "noviembre", "novembro", "listopada", "listopad"],
    &["december", "dezember", "decembre", "diciembre", "dicembre", "dezembro", "grudnia", "grudzien", "desember"],
];
const WEEKDAYS: [&str; 14] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun", "mo", "di", "mi", "do", "fr", "sa", "so"];
/// A file's dates in the order chosen, or in the only order they all fit when some don't fit
/// that one: a file that writes 31/10/2026 isn't month first, whatever was chosen.
pub(crate) fn dates(raw: &[String], format: &str) -> Result<Vec<String>, String> {
    let read = |f: &str| raw.iter().map(|s| date(s, f)).collect::<Result<Vec<_>, _>>();
    let chosen = read(format);
    if chosen.is_ok() {
        return chosen;
    }
    let mut fits = ["ymd", "dmy", "mdy"].into_iter().filter(|f| *f != format).filter_map(|f| read(f).ok());
    match (fits.next(), fits.next()) {
        (Some(only), None) => Ok(only),
        _ => chosen,
    }
}
/// How a statement's dates read when some may not: each date in the order the dates themselves
/// settle, else the chosen one, and whether that order still needs confirming.
#[derive(Debug, Clone, PartialEq)]
pub struct DateReading {
    /// The order the dates are read in.
    pub order: &'static str,
    /// Two orders read these dates differently and the dates don't settle which (none reads more
    /// of them), and the order hasn't been confirmed: an import asks before going on.
    pub unconfirmed: bool,
    /// The first date those orders read differently, to show when asking.
    pub example: Option<String>,
    /// Each date as YYYY-MM-DD, or None where it doesn't read in that order.
    pub dates: Vec<Option<String>>,
}
const ORDERS: [&str; 3] = ["ymd", "dmy", "mdy"];
/// Reads a column of dates that may hold unreadable ones without giving up on the rest: the order
/// that reads the most of them (the chosen one when it's among those), asking only when orders
/// reading as many would give different dates. ISO dates, or 31/10/2026 beside 01/02/2026, settle
/// it; 01/02/2026 alone doesn't, unless `confirmed` says the chosen order is right. Unlike `dates`,
/// nothing here fails: a date that doesn't read is None.
pub fn read_dates(raw: &[&str], chosen: &str, confirmed: bool) -> DateReading {
    // A statement's dates repeat (a day has several transactions): each is read once an order.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut distinct = vec![];
    let at: Vec<usize> = raw.iter().map(|s| *seen.entry(s).or_insert_with(|| {
        distinct.push(*s);
        distinct.len() - 1
    })).collect();
    let read: Vec<Vec<Option<String>>> = ORDERS.iter().map(|f| {
        let once: Vec<Option<String>> = distinct.iter().map(|s| date(s, f).ok()).collect();
        at.iter().map(|&k| once[k].clone()).collect()
    }).collect();
    let counts: Vec<usize> = read.iter().map(|dates| dates.iter().filter(|d| d.is_some()).count()).collect();
    let most = counts.iter().copied().max().unwrap_or(0);
    let best: Vec<usize> = (0..ORDERS.len()).filter(|&i| counts[i] == most).collect();
    let chosen_at = ORDERS.iter().position(|f| *f == chosen);
    let i = chosen_at.filter(|c| best.contains(c)).unwrap_or(best[0]);
    // The first date another order reading as many would make a different day.
    let example = best.iter().filter(|&&j| j != i).find_map(|&j| {
        (0..raw.len()).find(|&k| matches!((&read[i][k], &read[j][k]), (Some(a), Some(b)) if a != b)).map(|k| raw[k].trim().to_string())
    });
    DateReading {
        order: ORDERS[i],
        unconfirmed: example.is_some() && !(confirmed && Some(i) == chosen_at),
        example,
        dates: read[i].clone(),
    }
}
/// The order every one of these dates fits, when only one does (or year first, which any order
/// reads).
pub(crate) fn date_order(raw: &[&str]) -> Option<&'static str> {
    let fits = |f| !raw.is_empty() && raw.iter().all(|s| date(s, f).is_ok());
    if fits("ymd") {
        return Some("ymd");
    }
    match (fits("dmy"), fits("mdy")) {
        (true, false) => Some("dmy"),
        (false, true) => Some("mdy"),
        _ => None,
    }
}
pub fn preview(
    b: &Budget,
    account: &str,
    content: &str,
    format: &str,
    date_format: &str,
    flip_signs: bool,
) -> Result<Vec<Row>, String> {
    preview_mapped(b, account, content, format, date_format, flip_signs, None)
}
/// A preview reading a CSV's columns as chosen (see `bank_csv::read`).
pub fn preview_mapped(
    b: &Budget,
    account: &str,
    content: &str,
    format: &str,
    date_format: &str,
    flip_signs: bool,
    columns: Option<&Columns>,
) -> Result<Vec<Row>, String> {
    review(b, account, parse_file(b, account, content, format, date_format, flip_signs, columns)?)
}

pub fn preview_compact(
    b: &Budget,
    account: &str,
    content: &str,
    format: &str,
    date_format: &str,
    flip_signs: bool,
    columns: Option<&Columns>,
) -> Result<CompactReview, String> {
    review_compact(b, account, parse_file(b, account, content, format, date_format, flip_signs, columns)?)
}

fn parse_file(
    b: &Budget,
    account: &str,
    content: &str,
    format: &str,
    date_format: &str,
    flip_signs: bool,
    columns: Option<&Columns>,
) -> Result<Vec<BankTransaction>, String> {
    validate(b)?;
    if !b.accounts.iter().any(|a| a.id == account) {
        return Err(t!("Choose an account."));
    }
    if content.len() > 20_000_000 {
        return Err(t!("Import a file smaller than 20 MB."));
    }
    // A statement whose content says what it is reads as that, whatever it was chosen as.
    let raw = match crate::bank_statements::sniff(content).unwrap_or(format) {
        "csv" => bank_csv::read(b, account, content, date_format, flip_signs, columns)?,
        format => crate::bank_statements::transactions(format, content, &b.currency, date_format)?,
    };
    if raw.is_empty() || raw.len() > 10000 {
        return Err(t!("Choose a file containing 1–10,000 transactions."));
    }
    Ok(raw)
}
/// One transaction as the bank gave it, before the review.
#[derive(Debug, Clone, PartialEq)]
pub struct BankTransaction {
    /// The bank's own id for it (an OFX FITID, a SimpleFIN transaction id), or "" when it has none.
    pub fit: String,
    /// YYYY-MM-DD.
    pub date: String,
    pub payee: String,
    pub memo: String,
    /// Cents, positive for money in.
    pub amount: i64,
}
/// The review's rows for transactions bound for `account`, however they arrived (a file, bank
/// sync, the assistant): an id that stays the same when the same transaction comes again, whether
/// it already did, the existing transactions it could match (same amount, within ten days), and
/// what the payee rules make of it.
fn bank_id(account: &str, signature: &str) -> String {
    format!("bank:{:x}", Sha256::digest(format!("{account}|{signature}").as_bytes()))
}

pub(crate) fn import_ids(b: &Budget) -> HashSet<String> {
    b.transactions.iter().flat_map(|t| {
        std::iter::once(t.id.as_str()).chain(t.extra.get("bankImportIds")
            .and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str))
    }).chain(crate::fresh_start::bank_import_ids(b)).map(str::to_owned).collect()
}

/// One account's matches, indexed once by amount and day. Source rows use the saved parent
/// total; receiving split transfers each have their own identity, never the parent's total.
struct Matches {
    days: HashMap<i64, BTreeMap<NaiveDate, Vec<String>>>,
    targets: HashMap<String, (usize, Option<usize>)>,
    previously_matched: HashSet<String>,
    /// Transactions in other accounts a row could be the other side of (see `loose`), by the
    /// amount such a row has and their day, and where each is.
    transfers: HashMap<i64, BTreeMap<NaiveDate, Vec<String>>>,
    transfer_targets: HashMap<String, usize>,
}
/// How many days apart banks book the two sides of money moved between your own accounts.
const TRANSFER_DAYS: i64 = 5;
/// How many of the nearest a row weighs, so a file of one repeated amount stays quick.
const TRANSFER_CHOICES: usize = 20;
/// Whether a transaction in another account could be the other side of a transfer with
/// `account`: one part with no category yet (none, or the Uncategorized an import gives
/// spending), as a bank file leaves money moved between your own accounts.
fn loose(b: &Budget, t: &Transaction, account: &str) -> bool {
    t.account_id != account
        && t.transfer_account_id.is_none()
        && !ledger::starting_balance(t)
        && match t.splits.as_slice() {
            [s] => ledger::transfer(s).is_none() && match s.category_id.as_deref() {
                Some(id) => workflows::placeholder(b, id),
                None => !ledger::explicitly_unassigned(t, s),
            },
            _ => false,
        }
}
impl Matches {
    fn new(b: &Budget, account: &str) -> Self {
        let mut out = Self {
            days: HashMap::new(),
            targets: HashMap::new(),
            previously_matched: HashSet::new(),
            transfers: HashMap::new(),
            transfer_targets: HashMap::new(),
        };
        let mut reserved: HashSet<String> = b.transactions.iter().map(|t| t.id.clone()).collect();
        for (i, t) in b.transactions.iter().enumerate() {
            let Ok(day) = NaiveDate::parse_from_str(&t.date, "%Y-%m-%d") else { continue };
            let side_matched = t.extra.get("bankImportMatches").and_then(|v| v.get(account)).is_some();
            let mut add = |id: String, amount: i64, split: Option<usize>, matched: bool| {
                out.days.entry(amount).or_default().entry(day).or_default().push(id.clone());
                if matched { out.previously_matched.insert(id.clone()); }
                out.targets.insert(id, (i, split));
            };
            if t.account_id == account {
                add(t.id.clone(), t.amount, None, side_matched);
            } else if t.transfer_account_id.as_deref() == Some(account) {
                add(t.id.clone(), -t.amount, None, side_matched);
            } else {
                for (j, part) in t.splits.iter().enumerate().filter(|(_, s)| ledger::transfer(s) == Some(account)) {
                    let matched = part.extra.get("bankImportIds").and_then(Value::as_array).is_some_and(|ids| !ids.is_empty());
                    // Include the part's content so a reordered/edited split cannot silently
                    // receive a choice made against an older preview. Clearing it is harmless.
                    let mut content = part.clone();
                    content.extra.remove("transferStatus");
                    content.extra.remove("bankImportIds");
                    let key = json!([t.id, j, content]).to_string();
                    let base = format!("bank-leg:{:x}", Sha256::digest(key.as_bytes()));
                    let mut id = base.clone();
                    let mut attempt = 0;
                    while !reserved.insert(id.clone()) {
                        attempt += 1;
                        id = format!("{base}:{attempt}");
                    }
                    add(id, -part.amount, Some(j), matched);
                }
                if loose(b, t, account) {
                    out.transfers.entry(-t.amount).or_default().entry(day).or_default().push(t.id.clone());
                    out.transfer_targets.insert(t.id.clone(), i);
                }
            }
        }
        out
    }
    /// The transactions elsewhere a row could be the other side of, nearest first, and how many
    /// days apart.
    fn transfers(&self, amount: i64, day: NaiveDate) -> Vec<(i64, &String)> {
        let mut out = vec![];
        let Some(days) = self.transfers.get(&amount) else { return out };
        for apart in 0..=TRANSFER_DAYS {
            let around = [day - chrono::Duration::days(apart), day + chrono::Duration::days(apart)];
            for d in &around[..if apart == 0 { 1 } else { 2 }] {
                for id in days.get(d).into_iter().flatten() {
                    if out.len() == TRANSFER_CHOICES {
                        return out;
                    }
                    out.push((apart, id));
                }
            }
        }
        out
    }
    /// Whether a transaction here that no bank row has been yet could be this row.
    fn unmatched(&self, amount: i64, day: NaiveDate) -> bool {
        self.days.get(&amount).into_iter()
            .flat_map(|days| days.range(day - chrono::Duration::days(10)..=day + chrono::Duration::days(10)))
            .any(|(_, ids)| ids.iter().any(|id| !self.previously_matched.contains(id)))
    }
    fn candidates(&self, amount: i64, day: NaiveDate) -> Vec<String> {
        let mut ids: Vec<String> = self.days.get(&amount).into_iter()
            .flat_map(|days| days.range(day - chrono::Duration::days(10)..=day + chrono::Duration::days(10)))
            .flat_map(|(_, ids)| ids.iter().cloned()).collect();
        ids.sort();
        ids
    }
    /// The row's choices: transactions here it could be, and the one elsewhere it was offered as
    /// the other side of a transfer when none here could be it but an earlier bank row.
    fn candidate_set(&self, b: &Budget, amount: i64, day: NaiveDate, transfer: Option<(&String, bool)>) -> CandidateSet {
        let mut candidates = self.candidates(amount, day);
        let transfer = transfer.filter(|_| candidates.iter().all(|c| self.previously_matched.contains(c)));
        let transfer_candidates: Vec<String> = transfer.map(|(id, _)| id.clone()).into_iter().collect();
        candidates.extend(transfer_candidates.iter().cloned());
        let previously_matched_candidates = candidates.iter()
            .filter(|id| self.previously_matched.contains(*id)).cloned().collect();
        let candidate_details = candidates.iter().filter_map(|id| {
            let &(i, split) = self.targets.get(id)?;
            Some(Candidate { id: id.clone(), transaction_id: b.transactions[i].id.clone(), split_index: split? })
        }).collect();
        CandidateSet { candidates, previously_matched_candidates, candidate_details, transfer_candidates, transfer_sure: transfer.is_some_and(|(_, sure)| sure) }
    }
}
/// Words too common in account names and bank text to say which account is meant.
const COMMON: &[&str] = &[
    "account", "accounts", "konto", "girokonto", "bank", "the", "and", "for", "from", "my", "card", "credit", "debit",
    "payment", "transfer", "online", "pay", "current", "checking",
];
/// Bank text's words, without case or accents.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(bank_csv::plain).collect()
}
/// Bank text without its numbers (references, dates, card digits), to compare one with another.
fn wording(s: &str) -> String {
    words(s).into_iter().filter(|w| !w.chars().all(|c| c.is_ascii_digit())).collect::<Vec<_>>().join(" ")
}
/// What bank text says about where money went.
struct Clues {
    /// Each account's name, as the words that pick it out ("REVOLUT**1234" names Revolut, but
    /// "shopping" doesn't name ING).
    names: Vec<(String, Vec<String>)>,
    /// The wording each account's side of a transfer had, and the accounts it went to or came from.
    seen: HashMap<(String, String), HashSet<String>>,
}
impl Clues {
    fn new(b: &Budget) -> Self {
        let names = b.accounts.iter().map(|a| {
            let words = words(&a.name).into_iter()
                .filter(|w| w.chars().count() >= 3 && !COMMON.contains(&w.as_str()) && !w.chars().all(|c| c.is_ascii_digit()));
            (a.id.clone(), words.collect())
        }).collect();
        let mut seen: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for t in &b.transactions {
            let Some(other) = &t.transfer_account_id else { continue };
            let here = t.extra.get("importedPayee").and_then(Value::as_str).unwrap_or(&t.payee);
            let there = t.extra.get("transferImportedPayee").and_then(Value::as_str).unwrap_or("");
            for (account, text, to) in [(&t.account_id, here, other), (other, there, &t.account_id)] {
                let w = wording(text);
                if !w.is_empty() {
                    seen.entry((account.clone(), w)).or_default().insert(to.clone());
                }
            }
        }
        Self { names, seen }
    }
    /// What one side's bank text (payee and memo) says: the accounts it names, and its payee's
    /// wording.
    fn read(&self, payee: &str, memo: &str) -> Said {
        let words: HashSet<String> = words(payee).into_iter().chain(words(memo)).collect();
        let named = self.names.iter().filter(|(_, name)| name.iter().any(|w| words.contains(w))).map(|(id, _)| id.clone()).collect();
        Said { named, wording: wording(payee) }
    }
    /// How much a row in account `b` and a transaction in account `a` look like the two sides of
    /// one move: None when either text names another of your accounts and not this one, else one
    /// point for each text that names the other's account and each that had this wording in an
    /// earlier transfer between them.
    fn fit(&self, b: &str, row: &Said, a: &str, theirs: &Said) -> Option<u8> {
        let elsewhere = |said: &Said, own: &str, wanted: &str| !said.named.contains(wanted) && said.named.iter().any(|n| n != own);
        if elsewhere(row, b, a) || elsewhere(theirs, a, b) {
            return None;
        }
        let seen = |account: &str, said: &Said, to: &str| self.seen.get(&(account.to_owned(), said.wording.clone())).is_some_and(|s| s.contains(to));
        Some([row.named.contains(a), theirs.named.contains(b), seen(b, row, a), seen(a, theirs, b)].into_iter().filter(|&x| x).count() as u8)
    }
}
struct Said {
    named: HashSet<String>,
    wording: String,
}
/// How many times the same two wordings, between the same two accounts, have to pair up in one
/// file for the pattern to count as the bank text agreeing: a transfer that recurs (a weekly
/// top-up) is written the same way each time on both sides, a coincidence seldom so.
const RECURRING: usize = 5;
/// Which transaction elsewhere each row is offered as the other side of a transfer, and whether
/// the bank text agrees, which makes it clear enough to choose; amount and date alone only
/// suggest it, since a card payment of a round sum can mirror a transfer. Offers where the text
/// agrees go first, then the nearest in days, then where it agrees more; no two rows are offered
/// the same one, and none where either text names another of your accounts. Rows that could be a
/// transaction already in this account (other than one an earlier bank row already is), or may
/// already be imported, aren't offered one.
fn transfer_offers<'a>(b: &Budget, account: &str, matches: &'a Matches, rows: &[(Row, NaiveDate)]) -> Vec<Option<(&'a String, bool)>> {
    let clues = Clues::new(b);
    let mut said: HashMap<&String, Said> = HashMap::new();
    // Whether the text doesn't agree, days apart, how little it agrees, row, transaction.
    let mut pairs: Vec<(bool, i64, u8, usize, &String)> = vec![];
    // Each pairing's two wordings and the other account, and the rows each such trio pairs.
    let mut wordings: HashMap<(usize, &String), (String, String, &str)> = HashMap::new();
    let mut recurring: HashMap<(String, String, &str), HashSet<usize>> = HashMap::new();
    for (i, (r, day)) in rows.iter().enumerate() {
        if r.duplicate || r.possible_duplicate || matches.unmatched(r.amount, *day) {
            continue;
        }
        let row = clues.read(&r.payee, &r.memo);
        for (apart, id) in matches.transfers(r.amount, *day) {
            let t = &b.transactions[matches.transfer_targets[id]];
            let theirs = said.entry(id).or_insert_with(|| {
                clues.read(t.extra.get("importedPayee").and_then(Value::as_str).unwrap_or(&t.payee), &t.memo)
            });
            let Some(agrees) = clues.fit(account, &row, &t.account_id, theirs) else { continue };
            if agrees == 0 && !row.wording.is_empty() && !theirs.wording.is_empty() {
                let trio = (row.wording.clone(), theirs.wording.clone(), t.account_id.as_str());
                recurring.entry(trio.clone()).or_default().insert(i);
                wordings.insert((i, id), trio);
            }
            pairs.push((agrees == 0, apart, 4 - agrees, i, id));
        }
    }
    for p in pairs.iter_mut().filter(|p| p.0) {
        if wordings.get(&(p.3, p.4)).is_some_and(|trio| recurring[trio].len() >= RECURRING) {
            p.0 = false;
        }
    }
    pairs.sort();
    let mut offers = vec![None; rows.len()];
    let mut taken = HashSet::new();
    for (unsure, _, _, i, id) in pairs {
        if offers[i].is_none() && taken.insert(id) {
            offers[i] = Some((id, !unsure));
        }
    }
    offers
}

/// How a row is known: what its ID is made from, and for a row without the bank's ID, what an
/// older import's ID and identical rows' shared one are made from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Identity {
    pub signature: String,
    pub legacy: Option<String>,
    pub group: Option<String>,
    pub idless: bool,
    /// More IDs it's known by (see `Row::aliases`).
    pub aliases: Vec<String>,
    /// Its batch's ID (see `Row::batch`).
    pub batch: Option<String>,
}
/// What a file's rows share, counted in the order they come: a bank ID given twice keeps each
/// row's place (`fit:X`, then `fit:X:2`), and identical rows without one are told apart by theirs.
#[derive(Default)]
pub(crate) struct Counter {
    fits: HashMap<String, usize>,
    keys: HashMap<String, usize>,
    legacy: HashMap<String, usize>,
}
impl Counter {
    /// The identity a transaction takes here; when not `counted`, the one it would take, leaving
    /// the next for whatever comes after.
    pub(crate) fn identity(&mut self, t: &BankTransaction, counted: bool) -> Identity {
        let next = |seen: &mut HashMap<String, usize>, key: &str| {
            let n = seen.get(key).copied().unwrap_or(0) + 1;
            if counted {
                seen.insert(key.to_string(), n);
            }
            n
        };
        if t.fit.is_empty() {
            let old = format!("{}|{}|{}", t.date, t.payee, t.amount);
            let legacy = format!("{old}|{}", next(&mut self.legacy, &old));
            // Structured encoding avoids delimiter collisions; memo distinguishes independent
            // receipts. Only truly identical rows share the per-file occurrence counter.
            let key = json!([t.date, t.payee, t.memo, t.amount]).to_string();
            let n = next(&mut self.keys, &key);
            Identity {
                signature: format!("idless:v2:{key}:{n}"),
                legacy: Some(legacy),
                group: Some(format!("idless:group:v2:{key}")),
                idless: true,
                aliases: vec![],
                batch: None,
            }
        } else {
            // A bank that gives two transactions in one file the same ID: the second keeps its
            // place in the file too, so neither is taken for the other.
            let n = next(&mut self.fits, &t.fit);
            let signature = if n == 1 { format!("fit:{}", t.fit) } else { format!("fit:{}:{n}", t.fit) };
            Identity { signature, legacy: None, group: None, idless: false, aliases: vec![], batch: None }
        }
    }
}
/// Rows as a file gives them, each known as its place among them makes it.
fn identified(raw: Vec<BankTransaction>) -> Vec<(BankTransaction, Identity)> {
    let mut counter = Counter::default();
    raw.into_iter().map(|t| {
        let who = counter.identity(&t, true);
        (t, who)
    }).collect()
}
pub(crate) fn bank_id_of(account: &str, signature: &str) -> String {
    bank_id(account, signature)
}

pub fn review(b: &Budget, account: &str, raw: Vec<BankTransaction>) -> Result<Vec<Row>, String> {
    let mut rows = Vec::with_capacity(raw.len());
    review_each(b, account, identified(raw), |mut row, matches, day, transfer| {
        let choices = matches.candidate_set(b, row.amount, day, transfer);
        row.candidates = choices.candidates;
        row.previously_matched_candidates = choices.previously_matched_candidates;
        row.candidate_details = choices.candidate_details;
        row.transfer_candidates = choices.transfer_candidates;
        row.transfer_sure = choices.transfer_sure;
        rows.push(row);
    })?;
    Ok(rows)
}

pub fn review_compact(b: &Budget, account: &str, raw: Vec<BankTransaction>) -> Result<CompactReview, String> {
    review_identified(b, account, identified(raw))
}
/// A compact review of rows already known (see `Identity`).
pub(crate) fn review_identified(b: &Budget, account: &str, raw: Vec<(BankTransaction, Identity)>) -> Result<CompactReview, String> {
    let mut review = CompactReview { items: Vec::with_capacity(raw.len()), candidate_sets: vec![] };
    let mut windows = HashMap::new();
    let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
    review_each(b, account, raw, |row, matches, day, transfer| {
        let candidate_set = *windows.entry((row.amount, day, transfer.map(|(id, sure)| (id.clone(), sure)))).or_insert_with(|| {
            let choices = matches.candidate_set(b, row.amount, day, transfer);
            let mut hash = DefaultHasher::new();
            choices.hash(&mut hash);
            let bucket = buckets.entry(hash.finish()).or_default();
            // Hashes only locate a bucket: compare every identifier/detail before sharing,
            // so a collision cannot substitute another row's matching choices.
            if let Some(&index) = bucket.iter().find(|&&index| review.candidate_sets[index] == choices) {
                index
            } else {
                let index = review.candidate_sets.len();
                review.candidate_sets.push(choices);
                bucket.push(index);
                index
            }
        });
        review.items.push(CompactRow { row, candidate_set });
    })?;
    Ok(review)
}

fn review_each(
    b: &Budget,
    account: &str,
    raw: Vec<(BankTransaction, Identity)>,
    mut emit: impl FnMut(Row, &Matches, NaiveDate, Option<(&String, bool)>),
) -> Result<(), String> {
    // File previews and bank-sync snapshots share this boundary. Validate before the
    // match index negates transfer amounts or applies its ten-day date window.
    validate(b)?;
    if !b.accounts.iter().any(|a| a.id == account) {
        return Err(t!("Choose an account."));
    }
    if raw.iter().any(|(t, _)| !valid_date(&t.date)
        || !(-1_000_000_000_000..=1_000_000_000_000).contains(&t.amount)) {
        return Err(t!("A bank transaction has an invalid date or amount."));
    }
    let imported: HashSet<&str> = b
        .transactions
        .iter()
        .flat_map(|t| {
            let ids = t.extra.get("bankImportIds").and_then(Value::as_array);
            std::iter::once(t.id.as_str()).chain(ids.into_iter().flatten().filter_map(Value::as_str))
        })
        .chain(crate::fresh_start::bank_import_ids(b))
        .collect();
    let matches = Matches::new(b, account);
    let mut rows = Vec::with_capacity(raw.len());
    for (BankTransaction { date, payee, memo, amount, .. }, who) in raw {
        let Identity { signature, legacy, group, idless, aliases, batch } = who;
        let legacy = legacy.map(|l| bank_id(account, &l));
        let group_id = group.map(|g| bank_id(account, &g));
        let id = bank_id(account, &signature);
        // A batch's payments once the whole entry is imported, or the entry once a payment is.
        let batch_taken = batch.as_ref().is_some_and(|whole| {
            if *whole == id { imported.contains(format!("camt-batch:{whole}").as_str()) } else { imported.contains(whole.as_str()) }
        });
        let duplicate = imported.contains(id.as_str()) || aliases.iter().any(|a| imported.contains(a.as_str())) || batch_taken;
        let possible_duplicate = !duplicate && (legacy.is_some_and(|id| imported.contains(id.as_str()))
            || group_id.as_ref().is_some_and(|id| imported.contains(id.as_str())));
        let d = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|_| t!("Invalid bank transaction date"))?;
        let mut row = Row {
            id,
            date,
            payee,
            memo,
            amount,
            duplicate,
            candidates: vec![],
            previously_matched_candidates: vec![],
            candidate_details: vec![],
            transfer_candidates: vec![],
            transfer_sure: false,
            idless,
            possible_duplicate,
            group_id,
            import_id: None,
            rename: None,
            category_id: None,
            at: None,
            aliases,
            batch,
            refund_of: None,
        };
        let t = new_transaction(b, account, &row);
        row.rename = Some(t.payee).filter(|p| *p != row.payee);
        row.category_id = t.splits[0].category_id.clone();
        rows.push((row, d));
    }
    let offers = transfer_offers(b, account, &matches, &rows);
    let refunds = refund_offers(b, account, &matches, &rows, &offers);
    for (((mut row, d), transfer), refund) in rows.into_iter().zip(offers).zip(refunds) {
        row.refund_of = refund;
        emit(row, &matches, d, transfer);
    }
    Ok(())
}
/// How long after a purchase its refund may come.
const REFUND_DAYS: i64 = 90;
/// A purchase a refund may be of.
struct Purchase<'a> {
    id: &'a str,
    date: NaiveDate,
    payee: &'a str,
    /// Its payee's name, and its bank text without numbers, to tell the shop by.
    name: String,
    wording: String,
    amount: i64,
    category: Option<&'a str>,
}
/// Which purchase each row may be a refund of: money in that could be nothing already here and
/// isn't offered as a transfer, into an account on the budget, after a purchase from the same
/// payee (by the name the payee rules give it, or its bank text without numbers) in that account
/// in the 90 days before, in the budget or the file. Purchases of the same amount say which, else
/// larger ones (a partial refund). One purchase gives its category; several, theirs when they
/// agree on one, else none (it's only said they may be).
fn refund_offers(b: &Budget, account: &str, matches: &Matches, rows: &[(Row, NaiveDate)], offers: &[Option<(&String, bool)>]) -> Vec<Option<RefundOf>> {
    if !b.accounts.iter().any(|a| a.id == account && a.on_budget()) || rows.iter().all(|(r, _)| r.amount <= 0) {
        return vec![None; rows.len()];
    }
    // A category, not the Uncategorized an import gives spending.
    fn category<'a>(b: &Budget, c: Option<&'a str>) -> Option<&'a str> {
        c.filter(|c| !workflows::placeholder(b, c))
    }
    let mut purchases: Vec<Purchase> = b.transactions.iter()
        .filter(|t| t.account_id == account && t.amount < 0 && t.transfer_account_id.is_none() && !ledger::starting_balance(t))
        .filter(|t| matches!(t.splits.as_slice(), [s] if ledger::transfer(s).is_none()))
        .filter_map(|t| Some(Purchase {
            id: &t.id,
            date: NaiveDate::parse_from_str(&t.date, "%Y-%m-%d").ok()?,
            payee: &t.payee,
            name: bank_csv::plain(&t.payee),
            wording: wording(t.extra.get("importedPayee").and_then(Value::as_str).unwrap_or(&t.payee)),
            amount: t.amount,
            category: category(b, t.splits[0].category_id.as_deref()),
        }))
        .collect();
    purchases.extend(rows.iter().filter(|(r, _)| r.amount < 0 && !r.duplicate).map(|(r, day)| {
        let payee = r.rename.as_deref().unwrap_or(&r.payee);
        Purchase { id: &r.id, date: *day, payee, name: bank_csv::plain(payee), wording: wording(&r.payee), amount: r.amount, category: category(b, r.category_id.as_deref()) }
    }));
    rows.iter().zip(offers).map(|((r, day), offer)| {
        if r.amount <= 0 || r.duplicate || r.possible_duplicate || offer.is_some() || !matches.candidates(r.amount, *day).is_empty() {
            return None;
        }
        let (name, said) = (bank_csv::plain(r.rename.as_deref().unwrap_or(&r.payee)), wording(&r.payee));
        let shop = |p: &&Purchase| (!name.is_empty() && p.name == name) || (!said.is_empty() && p.wording == said);
        let before: Vec<&Purchase> = purchases.iter().filter(|p| p.date <= *day && (*day - p.date).num_days() <= REFUND_DAYS).filter(shop).collect();
        let exact: Vec<&Purchase> = before.iter().copied().filter(|p| -p.amount == r.amount).collect();
        let these = if exact.is_empty() { before.into_iter().filter(|p| -p.amount > r.amount).collect() } else { exact };
        let latest = these.iter().max_by_key(|p| (p.date, p.id))?;
        let agreed = these.iter().all(|p| p.category == these[0].category);
        Some(RefundOf {
            id: latest.id.to_string(),
            date: latest.date.format("%Y-%m-%d").to_string(),
            payee: latest.payee.to_string(),
            amount: latest.amount,
            category_id: these[0].category.filter(|_| agreed).map(String::from),
            purchases: these.len(),
        })
    }).collect()
}
/// The IDs a row is kept under besides its own: identical rows' shared one, its aliases, and its
/// batch's mark on a payment.
fn more_ids(r: &Row) -> Vec<String> {
    let batch = r.batch.as_ref().filter(|whole| **whole != r.id).map(|whole| format!("camt-batch:{whole}"));
    r.group_id.iter().chain(&r.aliases).cloned().chain(batch).collect()
}
/// A new transaction for an imported row, keeping the bank's payee text, with the payee rules applied.
fn new_transaction(b: &Budget, account: &str, r: &Row) -> Transaction {
    let mut extra = json!({"approved": false, "bankImportIds": [r.id], "bankImportMatches": {account: r.id}});
    extra["bankImportIds"].as_array_mut().unwrap().extend(more_ids(r).into_iter().map(Value::from));
    if !r.payee.trim().is_empty() {
        extra["importedPayee"] = json!(r.payee);
    }
    let mut t = Transaction {
        id: r.id.clone(),
        date: r.date.clone(),
        account_id: account.into(),
        payee: r.payee.clone(),
        memo: r.memo.clone(),
        amount: r.amount,
        status: Cleared::Cleared,
        splits: vec![Split {
            category_id: None,
            amount: r.amount,
            extra: Default::default(),
        }],
        transfer_account_id: None,
        transfer_status: None,
        extra: extra.as_object().unwrap().clone().into(),
    };
    workflows::apply_payee_rules(b, &mut t);
    t
}
pub fn apply(mut b: Budget, account: &str, choices: Vec<Choice>) -> Result<Budget, String> {
    validate(&b)?;
    if !b.accounts.iter().any(|a| a.id == account) {
        return Err(t!("Account not found"));
    }
    let mut imported = import_ids(&b);
    let matches = Matches::new(&b, account);
    let mut used = HashSet::new();
    let now = Utc::now();
    // Rows without the bank's time are added now, a millisecond apart, so the register shows a
    // day's rows in the order the statement does: a newest-first file's first row is the newest.
    let count = choices.len();
    let newest_first = {
        let steps = choices.windows(2).map(|w| w[1].row.date.cmp(&w[0].row.date));
        let (back, forward) = steps.fold((0, 0), |(b, f), o| match o {
            std::cmp::Ordering::Less => (b + 1, f),
            std::cmp::Ordering::Greater => (b, f + 1),
            std::cmp::Ordering::Equal => (b, f),
        });
        back > forward
    };
    // Refunds imported, and the purchase whose category each takes.
    let mut refunds: Vec<(String, RefundOf)> = vec![];
    for (i, c) in choices.into_iter().enumerate() {
        let offset = if newest_first { count - 1 - i } else { i };
        if c.action == "skip" {
            continue;
        }
        let r = c.row;
        if let Some(id) = &r.import_id {
            if !r.idless || c.action != "new" || !id.starts_with("bank:manual:") || id.len() > 100 || id.len() < 24 {
                return Err(t!("Preview this file again before importing a separate transaction."));
            }
        }
        let identity = r.import_id.as_deref().unwrap_or(&r.id);
        if imported.contains(identity) {
            continue;
        }
        // A batch's payments and the whole entry are never both imported.
        if let Some(whole) = &r.batch {
            if if *whole == r.id { imported.contains(&format!("camt-batch:{whole}")) } else { imported.contains(whole) } {
                continue;
            }
        }
        if c.action == "match" {
            let id = c
                .match_id
                .as_ref()
                .ok_or_else(|| t!("Choose the matching transaction"))?;
            if !used.insert(id.clone()) {
                return Err(t!("Two imported rows cannot match the same local transaction. Review the selections."));
            }
            let (index, split) = match (matches.targets.get(id), matches.transfer_targets.get(id)) {
                (Some(&target), _) => target,
                (None, Some(&index)) => (index, None),
                _ => return Err(t!("Matching transaction changed or was already matched. Preview again.")),
            };
            // A default match from an older preview must not silently consume another
            // payment that was imported meanwhile. Refreshed previews allow explicit matches.
            if matches.previously_matched.contains(id) && !r.previously_matched_candidates.contains(id) {
                return Err(t!("Matching transaction was imported since this preview. Preview again."));
            }
            let on_budget = |id: &str| b.accounts.iter().any(|a| a.id == id && a.on_budget());
            let same_side = on_budget(&b.transactions[index].account_id) == on_budget(account);
            let t = &mut b.transactions[index];
            let date_a =
                NaiveDate::parse_from_str(&t.date, "%Y-%m-%d").map_err(|_| t!("Invalid date"))?;
            let date_b =
                NaiveDate::parse_from_str(&r.date, "%Y-%m-%d").map_err(|_| t!("Invalid date"))?;
            let apart = (date_a - date_b).num_days().abs();
            if apart > 10 {
                return Err(t!("Matching transaction is more than ten days apart."));
            }
            if let Some(split) = split {
                let part = &mut t.splits[split];
                if ledger::transfer(part) != Some(account) || -part.amount != r.amount {
                    return Err(t!("Matching account or amount changed. Preview again."));
                }
                if ledger::status(part) != Cleared::Reconciled {
                    part.extra.insert("transferStatus".into(), json!("cleared"));
                }
                part.extra.entry("bankImportIds").or_insert(json!([]))
                    .as_array_mut().ok_or_else(|| t!("Invalid bank identifiers"))?.push(json!(r.id));
            } else if t.account_id == account && t.amount == r.amount {
                if t.status != Cleared::Reconciled {
                    t.status = Cleared::Cleared
                }
            } else if t.transfer_account_id.as_deref() == Some(account) && -t.amount == r.amount {
                if t.transfer_status != Some(Cleared::Reconciled) {
                    t.transfer_status = Some(Cleared::Cleared)
                }
            } else if matches.transfer_targets.contains_key(id) && -t.amount == r.amount && apart <= TRANSFER_DAYS {
                // Its other side, from money moved between your own accounts: they become one
                // transfer, so the move isn't counted as spending or income. Between budget
                // accounts it needs no category; across the budget's edge it keeps its own.
                t.transfer_account_id = Some(account.into());
                t.transfer_status = Some(Cleared::Cleared);
                t.payee.clear();
                if same_side {
                    t.splits.clear();
                }
                if !r.memo.is_empty() {
                    t.extra.insert("transferSource".into(), json!({"memo": r.memo}));
                }
                // This side's bank text, which tells the next transfer between them apart.
                if !r.payee.trim().is_empty() {
                    t.extra.insert("transferImportedPayee".into(), json!(r.payee));
                }
            } else {
                return Err(t!("Matching account or amount changed. Preview again."));
            }
            if split.is_none() {
                t.extra.entry("bankImportMatches").or_insert(json!({}))
                    .as_object_mut().ok_or_else(|| t!("Invalid bank matches"))?.insert(account.into(), json!(r.id));
            }
            t.extra
                .entry("bankImportIds")
                .or_insert(json!([]))
                .as_array_mut()
                .ok_or_else(|| t!("Invalid bank identifiers"))?
                .push(json!(r.id));
            let ids = t.extra.get_mut("bankImportIds").and_then(Value::as_array_mut).expect("checked above");
            for more in more_ids(&r) {
                if !ids.iter().any(|id| id.as_str() == Some(more.as_str())) { ids.push(json!(more)); }
            }
            if t.account_id == account && !r.payee.trim().is_empty() {
                t.extra.entry("importedPayee").or_insert_with(|| json!(r.payee));
            }
            t.extra.insert("approved".into(), json!(false));
        } else if c.action == "new" {
            let mut t = new_transaction(&b, account, &r);
            t.id = identity.to_owned();
            // When the bank says it happened, or else now plus the row's offset.
            let at = r.at.as_deref().and_then(|at| DateTime::parse_from_rfc3339(at).ok());
            let at = at.map_or(now + chrono::Duration::milliseconds(offset as i64), |at| at.with_timezone(&Utc));
            t.set_created_at(&stamp(at));
            if t.amount < 0
                && t.splits[0].category_id.is_none()
                && b.accounts.iter().any(|a| a.id == account && a.on_budget())
            {
                let category = b
                    .categories
                    .iter()
                    .find(|c| c.name == "Uncategorized" || c.name == "Uncategorized Transactions")
                    .map(|c| c.id.clone())
                    .unwrap_or("local-uncategorized".into());
                if !b.categories.iter().any(|c| c.id == category) {
                    b.categories.push(Category {
                        id: category.clone(),
                        name: "Uncategorized".into(),
                        group: "Needs review".into(),
                        target: 0,
                        extra: Default::default(),
                    });
                }
                t.splits[0].category_id = Some(category);
            }
            if let Some(refund) = r.refund_of.clone().filter(|_| t.amount > 0) {
                refunds.push((t.id.clone(), refund));
            }
            b.transactions.push(t);
        } else {
            return Err(t!("Review every imported row before saving."));
        }
        imported.insert(identity.to_owned());
        imported.extend(more_ids(&r));
        imported.insert(r.id);
    }
    // A refund takes its purchase's category, when it has none and the purchase (in the budget,
    // or this file) still has that category.
    for (id, refund) in refunds {
        let Some(category) = refund.category_id else { continue };
        let still = b.transactions.iter().any(|t| {
            t.id == refund.id && t.account_id == account && t.amount < 0 && matches!(t.splits.as_slice(), [s] if s.category_id.as_deref() == Some(category.as_str()))
        });
        if let (true, Some(t)) = (still, b.transactions.iter_mut().find(|t| t.id == id)) {
            if let [s] = t.splits.as_mut_slice() {
                s.category_id.get_or_insert(category);
            }
        }
    }
    validate(&b)?;
    Ok(b)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tests::*;
    fn add_rows(b: Budget, rows: Vec<Row>, account: &str) -> Budget {
        apply(b, account, rows.into_iter().map(|row| Choice { row, action: "new".into(), match_id: None }).collect()).unwrap()
    }
    fn file(memo: &str) -> String {
        format!("Date,Payee,Memo,Amount\n2026-10-15,Shop,{memo},-50.00")
    }
    #[test]
    fn bank_reviews_reject_unsupported_dates_and_invalid_ledgers_before_matching() {
        let b = sample();
        for date in ["+262142-12-31", "+10000-01-01", "not-a-date", "2026-02-30"] {
            let csv = format!("Date,Payee,Amount\n{date},Shop,-50.00");
            assert!(preview(&b, "cash", &csv, "csv", "ymd", false).is_err(), "{date}");
            let raw = BankTransaction { fit: "bank-id".into(), date: date.into(),
                payee: "Shop".into(), memo: String::new(), amount: -5000 };
            assert!(review(&b, "cash", vec![raw]).is_err(), "{date}");
        }
        // Bank-sync reviews also receive the current, possibly invalid, UI draft.
        let mut invalid = b.clone();
        let mut transfer = purchase();
        transfer.transfer_account_id = Some("cash".into());
        transfer.transfer_status = Some(Cleared::Uncleared);
        transfer.amount = i64::MIN;
        transfer.splits[0].amount = i64::MIN;
        invalid.transactions.push(transfer);
        assert!(review(&invalid, "cash", vec![]).is_err(), "do not negate an unchecked transfer amount");
        let raw = BankTransaction { fit: "bank-id".into(), date: "2026-10-15".into(),
            payee: "Shop".into(), memo: String::new(), amount: i64::MIN };
        assert!(review(&b, "cash", vec![raw]).is_err());
        assert!(review(&b, "missing", vec![]).is_err());
        // A supported upper date remains safe when the matching window extends past year 9999.
        let csv = "Date,Payee,Amount\n9999-12-31,Shop,-50.00";
        assert_eq!(preview(&b, "cash", csv, "csv", "ymd", false).unwrap()[0].date, "9999-12-31");
    }
    #[test]
    fn idless_receipts_keep_distinct_memos_and_reimports_remain_idempotent() {
        let mut b = sample();
        for memo in ["Receipt A", "Receipt B"] {
            let rows = preview(&b, "cash", &file(memo), "csv", "ymd", false).unwrap();
            assert!(!rows[0].duplicate && !rows[0].possible_duplicate);
            assert_eq!(rows[0].candidates, rows[0].previously_matched_candidates,
                "an already imported payment must require an explicit choice before absorbing another receipt");
            b = add_rows(b, rows, "cash");
        }
        assert_eq!(b.transactions.len(), 2);
        let combined = "Date,Payee,Memo,Amount\n2026-10-15,Shop,Receipt B,-50.00\n2026-10-15,Shop,Receipt A,-50.00";
        let rows = preview(&b, "cash", combined, "csv", "ymd", false).unwrap();
        assert!(rows.iter().all(|r| r.duplicate));
        assert_eq!(add_rows(b.clone(), rows, "cash"), b);
    }
    #[test]
    fn csv_payments_can_explicitly_match_ofx_and_bank_sync_ids_without_duplicating() {
        let b = sample();
        let csv = file("Original receipt");
        let rows = preview(&b, "cash", &csv, "csv", "ymd", false).unwrap();
        let mut b = add_rows(b, rows, "cash");
        let id = b.transactions[0].id.clone();
        b.transactions[0].status = Cleared::Reconciled;
        b.transactions[0].memo = "Keep my memo".into();
        b.transactions[0].splits[0].category_id = Some("food".into());
        let ofx = "<OFX><CURDEF>SAR<STMTTRN><TRNAMT>-50.00<DTPOSTED>20261015000000<FITID>ofx-id<NAME>Shop</STMTTRN></OFX>";
        let ofx_row = preview(&b, "cash", ofx, "ofx", "ymd", false).unwrap().remove(0);
        let sync_row = review(&b, "cash", vec![incoming("simplefin-id", -5000)]).unwrap().remove(0);
        for row in [ofx_row, sync_row] {
            assert!(!row.duplicate);
            assert_eq!(row.candidates, [id.clone()]);
            assert_eq!(row.previously_matched_candidates, [id.clone()]);
            b = apply(b, "cash", vec![Choice { row: row.clone(), action: "match".into(), match_id: Some(id.clone()) }]).unwrap();
            assert_eq!(apply(b.clone(), "cash", vec![Choice { row, action: "match".into(), match_id: Some(id.clone()) }]).unwrap(), b);
        }
        assert_eq!(b.transactions.len(), 1);
        let t = &b.transactions[0];
        assert_eq!(t.status, Cleared::Reconciled);
        assert_eq!(t.memo, "Keep my memo");
        assert_eq!(t.splits[0].category_id.as_deref(), Some("food"));
        assert!(preview(&b, "cash", &csv, "csv", "ymd", false).unwrap()[0].duplicate);
        assert!(preview(&b, "cash", ofx, "ofx", "ymd", false).unwrap()[0].duplicate);
        assert!(review(&b, "cash", vec![incoming("simplefin-id", -5000)]).unwrap()[0].duplicate);
    }
    #[test]
    fn direct_transfer_sides_can_each_match_more_than_one_import_source() {
        let mut b = sample();
        let mut t = purchase();
        t.account_id = "cash".into();
        t.transfer_account_id = Some("card".into());
        t.status = Cleared::Reconciled;
        t.transfer_status = Some(Cleared::Reconciled);
        t.splits.clear();
        b.transactions.push(t);
        for source in ["file", "sync"] {
            for (account, amount) in [("cash", -5000), ("card", 5000)] {
                let row = review(&b, account, vec![incoming(source, amount)]).unwrap().remove(0);
                assert_eq!(row.candidates, ["tx"]);
                assert_eq!(row.previously_matched_candidates.is_empty(), source == "file");
                b = apply(b, account, vec![Choice { row, action: "match".into(), match_id: Some("tx".into()) }]).unwrap();
            }
        }
        assert_eq!(b.transactions.len(), 1);
        assert_eq!(b.transactions[0].status, Cleared::Reconciled);
        assert_eq!(b.transactions[0].transfer_status, Some(Cleared::Reconciled));
        for (account, amount) in [("cash", -5000), ("card", 5000)] {
            assert!(review(&b, account, vec![incoming("file", amount), incoming("sync", amount)]).unwrap().iter().all(|r| r.duplicate));
        }
    }
    #[test]
    fn matching_rejects_reusing_an_account_side_in_one_batch_and_newly_imported_stale_choices() {
        for split in [false, true] {
            let mut b = if split { split_budget(false) } else { sample() };
            if !split { b.transactions.push(purchase()); }
            let (account, amount) = if split { ("savings", 1000) } else { ("card", -5000) };
            let rows = review(&b, account, vec![incoming("first", amount), incoming("second", amount)]).unwrap();
            let choices = || rows.iter().cloned().map(|row| Choice { match_id: Some(row.candidates[0].clone()), row, action: "match".into() }).collect();
            assert!(apply(b.clone(), account, choices()).unwrap_err().contains("Two imported rows"));
            let id = rows[0].candidates[0].clone();
            b = apply(b, account, vec![Choice { row: rows[0].clone(), action: "match".into(), match_id: Some(id.clone()) }]).unwrap();
            assert!(apply(b.clone(), account, vec![Choice { row: rows[1].clone(), action: "match".into(), match_id: Some(id.clone()) }]).unwrap_err().contains("since this preview"));
            let fresh = review(&b, account, vec![incoming("second", amount)]).unwrap().remove(0);
            assert_eq!(fresh.previously_matched_candidates, [id.clone()]);
            b = apply(b, account, vec![Choice { row: fresh, action: "match".into(), match_id: Some(id) }]).unwrap();
            assert_eq!(b.transactions.len(), 1);
            assert!(review(&b, account, vec![incoming("first", amount), incoming("second", amount)]).unwrap().iter().all(|r| r.duplicate));
        }
    }
    #[test]
    fn legacy_idless_imports_require_a_choice_including_after_a_fresh_start() {
        let legacy_id = bank_id("cash", "2026-10-15|Shop|-5000|1");
        let mut b = sample();
        let mut t = purchase();
        t.account_id = "cash".into();
        t.id = legacy_id.clone();
        t.memo = "Receipt A".into();
        b.transactions.push(t);
        for memo in ["Receipt A", "Receipt B"] {
            let row = preview(&b, "cash", &file(memo), "csv", "ymd", false).unwrap().remove(0);
            assert!(!row.duplicate && row.possible_duplicate);
        }
        // A deliberate add has a new identity, and cannot disappear behind the legacy alias.
        let rows = preview(&b, "cash", &file("Receipt B"), "csv", "ymd", false).unwrap();
        let b = add_rows(b, rows, "cash");
        assert_eq!(b.transactions.len(), 2);
        assert!(preview(&b, "cash", &file("Receipt B"), "csv", "ymd", false).unwrap()[0].duplicate);
        let mut fresh = sample();
        fresh.extra.insert("freshStartHistory".into(), json!({"bankImportIds":[legacy_id]}));
        assert!(preview(&fresh, "cash", &file("Receipt A"), "csv", "ymd", false).unwrap()[0].possible_duplicate);
    }
    #[test]
    fn a_partial_import_of_identical_rows_is_ambiguous_when_exported_alone() {
        let b = sample();
        let csv = "Date,Payee,Memo,Amount\n2026-10-15,Shop,Receipt,-50.00\n2026-10-15,Shop,Receipt,-50.00";
        let rows = preview(&b, "cash", csv, "csv", "ymd", false).unwrap();
        // The first row was skipped. A later export contains only the second identical row,
        // numbered first again. It must not silently become another payment.
        let b = add_rows(b, vec![rows[1].clone()], "cash");
        let row = preview(&b, "cash", &file("Receipt"), "csv", "ymd", false).unwrap().remove(0);
        assert!(!row.duplicate && row.possible_duplicate);
        assert!(preview(&b, "cash", csv, "csv", "ymd", false).unwrap()[1].duplicate);
    }
    #[test]
    fn explicit_idless_duplicate_override_is_stable_on_retry_and_forbidden_for_bank_ids() {
        let b = sample();
        let rows = preview(&b, "cash", &file("same receipt"), "csv", "ymd", false).unwrap();
        let b = add_rows(b, rows, "cash");
        let mut row = preview(&b, "cash", &file("same receipt"), "csv", "ymd", false).unwrap().remove(0);
        assert!(row.duplicate && row.idless);
        row.import_id = Some("bank:manual:explicit-stable-retry-id".into());
        let b = add_rows(b, vec![row.clone()], "cash");
        assert_eq!(b.transactions.len(), 2);
        assert_eq!(add_rows(b.clone(), vec![row.clone()], "cash"), b);
        row.idless = false;
        assert!(apply(b, "cash", vec![Choice { row, action: "new".into(), match_id: None }]).is_err());
    }
    fn split_budget(equal: bool) -> Budget {
        let mut b = sample();
        let mut peer = b.accounts[0].clone();
        peer.id = "savings".into();
        peer.name = "Savings".into();
        b.accounts.push(peer);
        let mut t = purchase();
        t.account_id = "cash".into();
        t.status = Cleared::Uncleared;
        t.amount = if equal { -2000 } else { -3000 };
        t.splits = serde_json::from_value(json!([
            {"amount":-1000,"categoryId":null,"memo":"First leg","transferAccountId":"savings","transferStatus":"uncleared"},
            {"amount":if equal {-1000} else {-2000},"categoryId":null,"memo":"Second leg","transferAccountId":"savings","transferStatus":"uncleared"}
        ])).unwrap();
        b.transactions.push(t);
        validate(&b).unwrap();
        b
    }
    fn incoming(fit: &str, amount: i64) -> BankTransaction {
        BankTransaction { fit: fit.into(), date: "2026-10-15".into(), payee: "Transfer".into(), memo: String::new(), amount }
    }
    #[test]
    fn compact_reviews_expand_losslessly_including_split_and_previous_match_evidence() {
        let mut b = split_budget(true);
        let first = review(&b, "savings", vec![incoming("already-imported", 1000)]).unwrap().remove(0);
        let chosen = first.candidates[0].clone();
        b = apply(b, "savings", vec![Choice { row: first, action: "match".into(), match_id: Some(chosen.clone()) }]).unwrap();
        let raw = || {
            let mut next_day = incoming("another-day", 1000);
            next_day.date = "2026-10-16".into();
            vec![incoming("already-imported", 1000), incoming("new-source", 1000), next_day, incoming("other-amount", 2000)]
        };
        let full = review(&b, "savings", raw()).unwrap();
        let compact = review_compact(&b, "savings", raw()).unwrap();
        assert_eq!(compact.candidate_sets.len(), 2, "different date windows with identical choices share their pool");
        for (expected, item) in full.iter().zip(&compact.items) {
            let choices = &compact.candidate_sets[item.candidate_set];
            let mut expanded = item.row.clone();
            expanded.candidates = choices.candidates.clone();
            expanded.candidate_details = choices.candidate_details.clone();
            expanded.previously_matched_candidates = choices.previously_matched_candidates.clone();
            assert_eq!(serde_json::to_value(expected).unwrap(), serde_json::to_value(expanded).unwrap());
        }
        let mut selected = compact.items[1].row.clone();
        assert!(apply(b.clone(), "savings", vec![Choice { row: selected.clone(), action: "match".into(), match_id: Some(chosen.clone()) }]).unwrap_err().contains("since this preview"));
        // The compact submit only needs the explicitly chosen acknowledgement. Matching still
        // revalidates the account, amount, date and split identity against the current budget.
        selected.previously_matched_candidates.push(chosen.clone());
        let saved = apply(b.clone(), "savings", vec![Choice { row: selected.clone(), action: "match".into(), match_id: Some(chosen.clone()) }]).unwrap();
        assert_eq!(saved.transactions.len(), b.transactions.len());
        b.transactions[0].splits.swap(0, 1);
        assert!(apply(b, "savings", vec![Choice { row: selected, action: "match".into(), match_id: Some(chosen) }]).is_err());
    }
    #[test]
    fn compact_statement_keeps_all_ten_thousand_choices_without_repeating_them_per_row() {
        let mut b = sample();
        b.transactions = (0..10_000).map(|i| {
            let mut transaction = purchase();
            transaction.id = format!("candidate-{i:05}");
            transaction
        }).collect();
        let content = format!("Date,Payee,Amount,Id\n{}", (0..1000).map(|i| format!("2026-10-15,Shop,-50.00,receipt-{i}\n")).collect::<String>());
        let compact = preview_compact(&b, "card", &content, "csv", "ymd", false, None).unwrap();
        assert_eq!(compact.items.len(), 1000);
        assert_eq!(compact.candidate_sets.len(), 1);
        assert_eq!(compact.candidate_sets[0].candidates.len(), 10_000);
        assert!(compact.items.iter().all(|row| row.candidate_set == 0 && row.row.candidates.is_empty()));
        assert!(serde_json::to_vec(&compact).unwrap().len() < 1_000_000, "transport must scale with unique choices plus rows, not their product");
        let last = compact.candidate_sets[0].candidates.last().unwrap().clone();
        let row = compact.items.into_iter().next().unwrap().row;
        let saved = apply(b, "card", vec![Choice { row, action: "match".into(), match_id: Some(last.clone()) }]).unwrap();
        assert_eq!(saved.transactions.len(), 10_000);
        assert_eq!(saved.transactions.iter().find(|t| t.id == last).unwrap().status, Cleared::Cleared);
    }
    #[test]
    fn split_source_totals_and_each_receiving_leg_match_without_duplicate_transactions() {
        let b = split_budget(false);
        let source = review(&b, "cash", vec![incoming("source", -3000)]).unwrap().remove(0);
        assert_eq!(source.candidates, ["tx"]);
        let rows = review(&b, "savings", vec![incoming("first", 1000), incoming("second", 2000)]).unwrap();
        assert_eq!(rows[0].candidates.len(), 1);
        assert_ne!(rows[0].candidates, rows[1].candidates);
        assert_eq!(rows[1].candidate_details[0].split_index, 1);
        let choices = rows.into_iter().map(|row| Choice { match_id: Some(row.candidates[0].clone()), row, action: "match".into() }).collect();
        let b = apply(b, "savings", choices).unwrap();
        assert!(b.transactions[0].splits.iter().all(|s| ledger::status(s) == Cleared::Cleared));
        let b = apply(b, "cash", vec![Choice { row: source, action: "match".into(), match_id: Some("tx".into()) }]).unwrap();
        assert_eq!(b.transactions.len(), 1);
        assert_eq!(b.transactions[0].status, Cleared::Cleared);
        assert!(review(&b, "savings", vec![incoming("first", 1000), incoming("second", 2000)]).unwrap().iter().all(|r| r.duplicate));
    }
    #[test]
    fn equal_receiving_legs_match_separately_and_stale_leg_choices_are_rejected() {
        let mut b = split_budget(true);
        let row = review(&b, "savings", vec![incoming("first", 1000)]).unwrap().remove(0);
        assert_eq!(row.candidates.len(), 2);
        let first = row.candidate_details.iter().find(|d| d.split_index == 0).unwrap().id.clone();
        let mut reordered = b.clone();
        reordered.transactions[0].splits.swap(0, 1);
        assert!(apply(reordered, "savings", vec![Choice { row: row.clone(), action: "match".into(), match_id: Some(first.clone()) }]).is_err());
        b = apply(b, "savings", vec![Choice { row, action: "match".into(), match_id: Some(first) }]).unwrap();
        let row = review(&b, "savings", vec![incoming("second", 1000)]).unwrap().remove(0);
        assert_eq!(row.candidates.len(), 2);
        let matched = row.candidate_details.iter().find(|d| d.split_index == 1).unwrap().id.clone();
        assert!(!row.previously_matched_candidates.contains(&matched));
        assert_eq!(row.previously_matched_candidates.len(), 1);
        b = apply(b, "savings", vec![Choice { row, action: "match".into(), match_id: Some(matched) }]).unwrap();
        assert!(b.transactions[0].splits.iter().all(|s| ledger::status(s) == Cleared::Cleared));
        // The same two legs from a different bank source remain independently matchable.
        let rows = review(&b, "savings", vec![incoming("sync-first", 1000), incoming("sync-second", 1000)]).unwrap();
        let choices = rows.into_iter().enumerate().map(|(i, row)| {
            assert_eq!(row.candidates, row.previously_matched_candidates);
            let id = row.candidate_details.iter().find(|d| d.split_index == i).unwrap().id.clone();
            Choice { row, action: "match".into(), match_id: Some(id) }
        }).collect();
        b = apply(b, "savings", choices).unwrap();
        assert_eq!(b.transactions.len(), 1);
        assert!(b.transactions[0].splits.iter().all(|s| s.extra["bankImportIds"].as_array().unwrap().len() == 2));
        assert!(review(&b, "savings", vec![incoming("first", 1000), incoming("second", 1000), incoming("sync-first", 1000), incoming("sync-second", 1000)]).unwrap().iter().all(|r| r.duplicate));
    }
    #[test]
    fn a_files_rows_on_one_day_are_added_in_the_files_order() {
        let b = sample();
        let csv = "Date,Payee,Amount\n2026-10-14,Other day,-9.00\n2026-10-15,First,-1.00\n2026-10-15,Second,-2.00\n2026-10-15,Third,-3.00";
        let rows = preview(&b, "cash", csv, "csv", "ymd", false).unwrap();
        let before = stamp_now();
        let choices = rows.into_iter().map(|row| Choice { row, action: "new".into(), match_id: None }).collect();
        let b = apply(b, "cash", choices).unwrap();
        assert!(b.transactions.iter().all(|t| t.created_at().is_some_and(|at| at >= before.as_str())));
        // Newest first, the file's last row on the day is on top.
        let mut list: Vec<&Transaction> = b.transactions.iter().collect();
        sort_newest_first(&mut list);
        let payees: Vec<&str> = list.iter().map(|t| t.payee.as_str()).collect();
        assert_eq!(payees, ["Third", "Second", "First", "Other day"]);
    }
    #[test]
    fn a_newest_first_files_rows_on_one_day_show_in_the_files_order() {
        let b = sample();
        let csv = "Date,Payee,Amount\n2026-10-16,Next day,-9.00\n2026-10-15,First,-1.00\n2026-10-15,Second,-2.00\n2026-10-15,Third,-3.00\n2026-10-14,Day before,-4.00";
        let rows = preview(&b, "cash", csv, "csv", "ymd", false).unwrap();
        let choices = rows.into_iter().map(|row| Choice { row, action: "new".into(), match_id: None }).collect();
        let b = apply(b, "cash", choices).unwrap();
        let mut list: Vec<&Transaction> = b.transactions.iter().collect();
        sort_newest_first(&mut list);
        let payees: Vec<&str> = list.iter().map(|t| t.payee.as_str()).collect();
        assert_eq!(payees, ["Next day", "First", "Second", "Third", "Day before"]);
    }
    #[test]
    fn csv_matches_manual_entries_and_reimport_is_idempotent() {
        let mut b = sample();
        let mut t = purchase();
        t.account_id = "cash".into();
        t.status = Cleared::Uncleared;
        b.transactions.push(t);
        let rows = preview(
            &b,
            "cash",
            "Date,Payee,Amount\n2026-10-15,Shop,-50.00",
            "csv",
            "ymd",
            false,
        )
        .unwrap();
        assert_eq!(rows[0].candidates, vec!["tx"]);
        let b = apply(
            b,
            "cash",
            vec![Choice {
                row: rows[0].clone(),
                action: "match".into(),
                match_id: Some("tx".into()),
            }],
        )
        .unwrap();
        assert_eq!(b.transactions.len(), 1);
        assert_eq!(b.transactions[0].status, Cleared::Cleared);
        assert!(
            preview(
                &b,
                "cash",
                "Date,Payee,Amount\n2026-10-15,Shop,-50.00",
                "csv",
                "ymd",
                false
            )
            .unwrap()[0]
                .duplicate
        );
    }
    #[test]
    fn keeps_the_bank_text_and_previews_what_the_rules_do() {
        let mut b = sample();
        b.extra.insert(
            "payeeRules".into(),
            json!([{"id":"r","match":"sq blue bottle","payee":"Blue Bottle","categoryId":null}]),
        );
        b.extra.insert("payees".into(), json!([{"id":"p","name":"Blue Bottle","categoryId":"food"}]));
        let mut manual = purchase();
        manual.account_id = "cash".into();
        manual.payee = "Corner Shop".into();
        b.transactions.push(manual);
        let file = "Date,Payee,Amount
2026-10-15,SQ *BLUE BOTTLE 0042,-6.50
2026-10-15,SHOP 12,-50.00
2026-10-16,ELSEWHERE,-1.00";
        let rows = preview(&b, "cash", file, "csv", "ymd", false).unwrap();
        assert_eq!(rows[0].rename.as_deref(), Some("Blue Bottle"));
        assert_eq!(rows[0].category_id.as_deref(), Some("food"));
        assert_eq!((rows[2].rename.as_deref(), rows[2].category_id.as_deref()), (None, None));
        let choices = vec![
            Choice { row: rows[0].clone(), action: "new".into(), match_id: None },
            Choice { row: rows[1].clone(), action: "match".into(), match_id: Some("tx".into()) },
        ];
        let b = apply(b, "cash", choices).unwrap();
        let new = b.transactions.iter().find(|t| t.id == rows[0].id).unwrap();
        assert_eq!((new.payee.as_str(), new.splits[0].category_id.as_deref()), ("Blue Bottle", Some("food")));
        assert_eq!(new.extra["importedPayee"], "SQ *BLUE BOTTLE 0042");
        // A transaction matched to a bank row keeps its payee and learns the bank's text.
        let matched = b.transactions.iter().find(|t| t.id == "tx").unwrap();
        assert_eq!((matched.payee.as_str(), &matched.extra["importedPayee"]), ("Corner Shop", &json!("SHOP 12")));
    }
    #[test]
    fn reads_two_digit_years_as_this_century() {
        assert_eq!(date("10/17/26", "mdy").unwrap(), "2026-10-17");
        assert_eq!(date("17/10/26", "dmy").unwrap(), "2026-10-17");
        assert_eq!(date("10-17-26", "mdy").unwrap(), "2026-10-17");
        // QIF writes the year after an apostrophe.
        assert_eq!(date("10/17'26", "mdy").unwrap(), "2026-10-17");
        assert_eq!(date(" 1/ 5/26", "mdy").unwrap(), "2026-01-05");
        assert_eq!(date("10/17/2026", "mdy").unwrap(), "2026-10-17");
        assert_eq!(date("2026-10-17", "dmy").unwrap(), "2026-10-17");
        // A year that can only be read as the year 26 isn't one.
        assert!(date("26-10-17", "ymd").is_err());
        let b = sample();
        let r = preview(&b, "cash", "!Type:Bank\nD10/17'26\nT-1.25\nPShop\n^", "qif", "mdy", false).unwrap();
        assert_eq!(r[0].date, "2026-10-17");
        let r = preview(&b, "cash", "Date,Payee,Amount\n10/17/26,Shop,-1.25", "csv", "mdy", false).unwrap();
        assert_eq!(r[0].date, "2026-10-17");
    }
    #[test]
    fn reads_sgml_ofx_and_qif_with_explicit_date_order() {
        let b = sample();
        let r=preview(&b,"cash","<OFX><CURDEF>SAR<STMTTRN><TRNAMT>-1.25<DTPOSTED>20261015000000<FITID>abc<NAME>A &amp; B</STMTTRN></OFX>","ofx","ymd",false).unwrap();
        assert_eq!(r[0].amount, -125);
        assert_eq!(r[0].payee, "A & B");
        let r = preview(
            &b,
            "cash",
            "!Type:Bank\nD15/10/2026\nT-1.25\nPShop\n^",
            "qif",
            "dmy",
            false,
        )
        .unwrap();
        assert_eq!(r[0].date, "2026-10-15");
        assert!(money("1.001", "SAR").is_err());
    }
    #[test]
    fn reads_month_names_in_banks_languages_and_spreadsheet_day_numbers() {
        for (s, want) in [
            ("15. Okt. 2026", "2026-10-15"),
            ("15-ene-2026", "2026-01-15"),
            ("2026 oct. 15", "2026-10-15"),
            ("Wed, 15 October 2026", "2026-10-15"),
            ("15 juin 2026", "2026-06-15"),
            ("15 juil. 2026", "2026-07-15"),
            ("15 août 2026", "2026-08-15"),
            ("15 März 2026", "2026-03-15"),
            ("15 października 2026", "2026-10-15"),
            ("15-dic-26", "2026-12-15"),
            ("15 Oct 2026 10:00", "2026-10-15"),
            ("46310", "2026-10-15"),
        ] {
            assert_eq!(date(s, "dmy").as_deref(), Ok(want), "{s}");
        }
        // Words that aren't a month, or a month among other words, aren't a date.
        for s in ["Rewe 12 2026", "Sept 2026 Rent 1", "SEP12345 01 2026", "Oct", "12345", "Description 1", "Desc 2", "Augusta 12 2026"] {
            assert!(date(s, "dmy").is_err(), "{s}");
        }
        // Without a year, as FNB writes them in quotes: a day in the past eleven months is that day,
        // and one over a month ahead was last year's.
        let today = Utc::now().date_naive();
        for ahead in [-330, -200, -1, 0, 40] {
            let day = today + chrono::Duration::days(ahead);
            if (day.month(), day.day()) == (2, 29) {
                continue;
            }
            let want = if ahead > 31 { day.with_year(day.year() - 1).unwrap() } else { day };
            let read = date(&format!("'{} {}'", day.format("%d"), day.format("%b")), "dmy").unwrap();
            assert_eq!(read, want.format("%Y-%m-%d").to_string(), "{ahead}");
        }
    }
    #[test]
    fn reads_dates_in_the_order_they_settle_and_asks_when_they_dont() {
        let read = |raw: &[&str], chosen: &str, confirmed: bool| read_dates(raw, chosen, confirmed);
        // ISO dates read the same in any order: nothing to ask.
        let iso = read(&["2026-10-01", "2026-10-02"], "dmy", false);
        assert_eq!((iso.order, iso.unconfirmed), ("dmy", false));
        assert_eq!(iso.dates, [Some("2026-10-01".into()), Some("2026-10-02".into())]);
        // 31/10/2026 reads only day first, so 01/02/2026 is 1 February, whatever was chosen.
        let settled = read(&["31/10/2026", "01/02/2026"], "mdy", false);
        assert_eq!((settled.order, settled.unconfirmed), ("dmy", false));
        assert_eq!(settled.dates[1].as_deref(), Some("2026-02-01"));
        // Only 01/02/2026: either order fits, so the chosen one is used and needs confirming...
        let open = read(&["01/02/2026", "03/04/2026"], "mdy", false);
        assert_eq!((open.order, open.unconfirmed, open.example.as_deref()), ("mdy", true, Some("01/02/2026")));
        assert_eq!(open.dates[0].as_deref(), Some("2026-01-02"));
        // ...unless it was confirmed.
        assert!(!read(&["01/02/2026"], "dmy", true).unconfirmed);
        // A damaged date reads as nothing; the others still read. When it was the only one that
        // settled the order, the order needs confirming rather than changing silently.
        let damaged = read(&["31/1O/2026", "01/02/2026"], "mdy", false);
        assert_eq!(damaged.dates[0], None);
        assert!(damaged.unconfirmed);
        let kept = read(&["31/10/2026", "n/a", "01/02/2026"], "mdy", false);
        assert_eq!((kept.order, kept.unconfirmed, kept.dates[1].clone()), ("dmy", false, None));
        // A confirmed order the dates contradict gives way to them.
        assert_eq!(read(&["10/31/2026", "01/02/2026"], "dmy", true).order, "mdy");
        // Nothing to read.
        assert!(!read(&[], "ymd", false).unconfirmed);
    }

    #[test]
    fn reads_ofx_and_qif_the_way_banks_and_quicken_write_them() {
        let b = sample();
        let read = |content: &str, format: &str| -> Vec<(String, String, String, i64)> {
            preview(&b, "cash", content, format, "mdy", false).unwrap().into_iter().map(|r| (r.date, r.payee, r.memo, r.amount)).collect()
        };
        let row = |d: &str, p: &str, m: &str, a: i64| (d.to_string(), p.to_string(), m.to_string(), a);
        // Two accounts in one download: the first one's. A zero line is passed over, a payee
        // only in the memo is the payee, and two lines with one FITID are both kept.
        let ofx = "OFXHEADER:100\nDATA:OFXSGML\n<OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><CURDEF>sar<BANKTRANLIST>\n\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261014120000.000[-5:EST]<TRNAMT>-4.50<FITID>1<NAME>Cafe</STMTTRN>\n\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261014<TRNAMT>-4.50<FITID>1<NAME>Cafe</STMTTRN>\n\
<STMTTRN><TRNTYPE>OTHER<DTPOSTED>20261015<TRNAMT>0.00<FITID>2<NAME>Card check</STMTTRN>\n\
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20261016<TRNAMT>100.00<FITID>3<NAME><MEMO>Refund 12</STMTTRN>\n\
</BANKTRANLIST></STMTRS></STMTTRNRS><STMTTRNRS><STMTRS><CURDEF>SAR<BANKTRANLIST>\n\
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261017<TRNAMT>-9.00<FITID>4<NAME>Savings</STMTTRN>\n</BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>";
        assert_eq!(read(ofx, "ofx"), [
            row("2026-10-14", "Cafe", "", -450), row("2026-10-14", "Cafe", "", -450), row("2026-10-16", "Refund 12", "", 10000),
        ]);
        let ids: HashSet<_> = preview(&b, "cash", ofx, "ofx", "ymd", false).unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), 3);
        // A bank that writes every amount as positive: the type says which way it went.
        let positive = "<OFX><STMTTRN><TRNTYPE>POS<DTPOSTED>20261014<TRNAMT>4.50<FITID>a<NAME>Cafe</STMTTRN>\
<STMTTRN><TRNTYPE>DIRECTDEP<DTPOSTED>20261015<TRNAMT>900.00<FITID>b<NAME>Payroll</STMTTRN></OFX>";
        assert_eq!(read(positive, "qif"), [row("2026-10-14", "Cafe", "", -450), row("2026-10-15", "Payroll", "", 90000)]);
        // Quicken's every-account export: account list, categories, then each account's own.
        let qif = "!Option:AutoSwitch\n!Account\nNChecking\nTBank\n^\nNSavings\nTBank\n^\n!Clear:AutoSwitch\n\
!Type:Cat\nNGroceries\nE\n^\n!Account\nNChecking\nTBank\n^\n!type:bank \nD10/14'26\nU-1,234.50\nT-1,234.50\nPRent\nLHousing\nC*\n^\n\
D10/15'26\nT12.00\nMInterest\n^\n!Account\nNSavings\nTBank\n^\n!Type:Bank\nD10/16'26\nT-5.00\nPOther account\n^\n";
        assert_eq!(read(qif, "qif"), [row("2026-10-14", "Rent", "", -123450), row("2026-10-15", "Interest", "", 1200)]);
        // The last record without its ^, and an amount only as U.
        assert_eq!(read("!Type:CCard\nD10/14/2026\nU-3.00\nPShop", "qif"), [row("2026-10-14", "Shop", "", -300)]);
        let investments = preview(&b, "cash", "!Type:Invst\nD10/14/2026\nNBuy\nT-3.00\n^", "qif", "mdy", false);
        assert!(investments.err().unwrap().contains("Only bank"));
    }

    fn refunds_budget() -> Budget {
        let mut b = sample();
        b.transactions.clear();
        for (id, name) in [("shopping", "Shopping"), ("gifts", "Gifts")] {
            b.categories.push(Category { extra: Default::default(), id: id.into(), name: name.into(), group: "Living".into(), target: 0 });
        }
        b
    }
    fn bought(id: &str, date: &str, payee: &str, amount: i64, category: Option<&str>) -> Transaction {
        let mut t = purchase();
        (t.id, t.date, t.account_id, t.payee, t.amount) = (id.into(), date.into(), "cash".into(), payee.into(), amount);
        t.splits[0].amount = amount;
        t.splits[0].category_id = category.map(String::from);
        t
    }
    fn money_in(date: &str, payee: &str, amount: i64) -> BankTransaction {
        BankTransaction { fit: String::new(), date: date.into(), payee: payee.into(), memo: String::new(), amount }
    }
    fn imported(b: Budget, rows: Vec<Row>) -> Budget {
        apply(b, "cash", rows.into_iter().map(|row| Choice { row, action: "new".into(), match_id: None }).collect()).unwrap()
    }
    fn category_of(b: &Budget, payee: &str, amount: i64) -> Option<String> {
        b.transactions.iter().find(|t| t.payee == payee && t.amount == amount).and_then(|t| t.splits[0].category_id.clone())
    }

    #[test]
    fn money_back_from_a_shop_is_offered_as_a_refund_of_what_was_bought_there() {
        let mut b = refunds_budget();
        b.transactions.push(bought("p1", "2026-10-07", "Amazon EU", -2399, Some("shopping")));
        let rows = review(&b, "cash", vec![money_in("2026-10-12", "Amazon EU", 2399)]).unwrap();
        let refund = rows[0].refund_of.clone().unwrap();
        assert_eq!(refund, RefundOf { id: "p1".into(), date: "2026-10-07".into(), payee: "Amazon EU".into(), amount: -2399, category_id: Some("shopping".into()), purchases: 1 });
        let b = imported(b, rows);
        assert_eq!(category_of(&b, "Amazon EU", 2399).as_deref(), Some("shopping"));

        // Two purchases of that amount there in other categories: it's said it may be either, and
        // no category is given.
        let mut b = refunds_budget();
        b.transactions.push(bought("p1", "2026-10-02", "Amazon EU", -2399, Some("shopping")));
        b.transactions.push(bought("p2", "2026-10-07", "Amazon EU", -2399, Some("gifts")));
        let rows = review(&b, "cash", vec![money_in("2026-10-12", "Amazon EU", 2399)]).unwrap();
        let refund = rows[0].refund_of.clone().unwrap();
        assert_eq!((refund.id.as_str(), refund.category_id, refund.purchases), ("p2", None, 2));
        assert_eq!(category_of(&imported(b, rows), "Amazon EU", 2399), None);

        // Part of a larger purchase back: the larger purchases there agree on the category.
        let mut b = refunds_budget();
        b.transactions.push(bought("p1", "2026-09-20", "Zalando", -8000, Some("shopping")));
        b.transactions.push(bought("p2", "2026-10-01", "Zalando", -5000, Some("shopping")));
        b.transactions.push(bought("p3", "2026-10-03", "Zalando", -500, Some("gifts")));
        let rows = review(&b, "cash", vec![money_in("2026-10-12", "ZALANDO", 1000)]).unwrap();
        let refund = rows[0].refund_of.clone().unwrap();
        assert_eq!((refund.id.as_str(), refund.category_id.as_deref(), refund.purchases), ("p2", Some("shopping"), 2));

        // Not a split purchase, a transfer, one too long ago, another shop's, or another account's.
        let mut b = refunds_budget();
        let mut split = bought("p1", "2026-10-07", "Amazon EU", -2399, Some("shopping"));
        split.splits = vec![Split { category_id: Some("shopping".into()), amount: -1399, extra: Default::default() }, Split { category_id: Some("gifts".into()), amount: -1000, extra: Default::default() }];
        b.transactions.push(split);
        let mut moved = bought("p2", "2026-10-07", "Amazon EU", -2399, None);
        (moved.transfer_account_id, moved.transfer_status) = (Some("card".into()), Some(Cleared::Cleared));
        moved.splits.clear();
        b.transactions.push(moved);
        b.transactions.push(bought("p3", "2026-06-01", "Amazon EU", -2399, Some("shopping")));
        b.transactions.push(bought("p4", "2026-10-07", "Other Shop", -2399, Some("shopping")));
        let mut card = bought("p5", "2026-10-07", "Amazon EU", -2399, Some("shopping"));
        card.account_id = "card".into();
        b.transactions.push(card);
        assert_eq!(review(&b, "cash", vec![money_in("2026-10-12", "Amazon EU", 2399)]).unwrap()[0].refund_of, None);

        // Money in that could be a transaction already here is that, not a refund.
        let mut b = refunds_budget();
        b.transactions.push(bought("p1", "2026-10-07", "Amazon EU", -2399, Some("shopping")));
        b.transactions.push(bought("in", "2026-10-11", "Expected refund", 2399, None));
        assert_eq!(review(&b, "cash", vec![money_in("2026-10-12", "Amazon EU", 2399)]).unwrap()[0].refund_of, None);
    }

    #[test]
    fn a_refund_in_the_same_file_as_its_purchase_takes_the_category_the_rules_give_it() {
        let mut b = refunds_budget();
        b.extra.insert("payeeRules".into(), json!([{"id": "r", "match": "amazon", "payee": "Amazon", "categoryId": "shopping"}]));
        // Newest first, as many banks write it: the refund before the purchase.
        let rows = review(&b, "cash", vec![money_in("2026-10-12", "AMAZON MKTPL REFUND 4471", 2399), money_in("2026-10-07", "AMAZON MKTPL 4471", -2399)]).unwrap();
        let refund = rows[0].refund_of.clone().unwrap();
        assert_eq!((refund.payee.as_str(), refund.category_id.as_deref()), ("Amazon", Some("shopping")));
        // The rules give the refund its category already; the purchase checks out at saving.
        let b = imported(b, rows);
        assert_eq!(category_of(&b, "Amazon", 2399).as_deref(), Some("shopping"));

        // Without a rule for the refund's text, the purchase's category is checked at saving: a
        // review made before the purchase changed category gives none.
        let mut b = refunds_budget();
        b.transactions.push(bought("p1", "2026-10-07", "Amazon EU", -2399, Some("shopping")));
        let rows = review(&b, "cash", vec![money_in("2026-10-12", "Amazon EU", 2399)]).unwrap();
        b.transactions[0].splits[0].category_id = Some("gifts".into());
        assert_eq!(category_of(&imported(b, rows), "Amazon EU", 2399), None);
        // An account off the budget has no categories to give.
        let mut b = refunds_budget();
        b.accounts[0].extra.insert("onBudget".into(), json!(false));
        b.transactions.push(bought("p1", "2026-10-07", "Amazon EU", -2399, None));
        assert_eq!(review(&b, "cash", vec![money_in("2026-10-12", "Amazon EU", 2399)]).unwrap()[0].refund_of, None);
    }

    #[test]
    fn reads_decimal_commas_and_refuses_amounts_it_cannot_tell_apart() {
        // A comma before the last one or two digits, with no dot, marks the cents.
        assert_eq!(money("12,34", "SAR"), Ok(1234));
        assert_eq!(money("-12,5", "SAR"), Ok(-1250));
        assert_eq!(money("1.234,56", "SAR"), Ok(123456));
        assert_eq!(money("1\u{a0}234,56", "SAR"), Ok(123456));
        assert_eq!(money("1.234.567", "SAR"), Ok(123456700));
        // Commas before groups of three still separate thousands, Indian grouping included.
        assert_eq!(money("1,234", "SAR"), Ok(123400));
        assert_eq!(money("-1,234,567.89", "SAR"), Ok(-123456789));
        assert_eq!(money("1,23,456.78", "SAR"), Ok(12345678));
        assert_eq!(money("(12.34)", "SAR"), Ok(-1234));
        assert_eq!(money("12.34-", "SAR"), Ok(-1234));
        // Anything else is refused rather than guessed.
        for s in [
            "12,3456",
            "1.234",
            "1,234.567",
            "1.234,567",
            "1,23,45",
            "1,2,345",
            "12.34.56",
        ] {
            assert!(money(s, "SAR").is_err(), "{s}");
        }
        // Through a whole file: a European bank's CSV, and an OFX amount with a decimal comma.
        let b = sample();
        let r = preview(
            &b,
            "cash",
            "Date;Payee;Amount\n15/10/2026;Shop;-1.234,56\n16/10/2026;Cafe;-12,34",
            "csv",
            "dmy",
            false,
        )
        .unwrap();
        assert_eq!([r[0].amount, r[1].amount], [-123456, -1234]);
        let r = preview(
            &b,
            "cash",
            "<OFX><STMTTRN><TRNAMT>-1,25<DTPOSTED>20261015<FITID>a</STMTTRN></OFX>",
            "ofx",
            "ymd",
            false,
        )
        .unwrap();
        assert_eq!(r[0].amount, -125);
    }
    #[test]
    fn reads_amounts_written_with_a_currency_sign_or_code() {
        assert_eq!(money("$12.34", "USD"), Ok(1234));
        assert_eq!(money("-$1,234.56", "USD"), Ok(-123456));
        assert_eq!(money("$-5.00", "USD"), Ok(-500));
        assert_eq!(money("($5.00)", "USD"), Ok(-500));
        assert_eq!(money("US$12.34", "USD"), Ok(1234));
        assert_eq!(money("€5", "EUR"), Ok(500));
        assert_eq!(money("-12,34 €", "EUR"), Ok(-1234));
        assert_eq!(money("£1,000.00", "GBP"), Ok(100000));
        assert_eq!(money("12.34 USD", "USD"), Ok(1234));
        assert_eq!(money("SAR -7.50", "SAR"), Ok(-750));
        // A written code has to be the budget's own currency.
        let wrong = money("12.34 EUR", "USD").unwrap_err();
        assert!(wrong.contains("EUR") && wrong.contains("USD"), "{wrong}");
        assert!(money("12.34 XY", "USD").is_err());
        // Debit and credit written after the amount.
        assert_eq!((money("12.34 DR", "USD"), money("$12.34CR", "USD"), money("-1,234.56 Cr", "USD")), (Ok(-1234), Ok(1234), Ok(123456)));
        let mut b = sample();
        b.currency = "USD".into();
        let r = preview(
            &b,
            "cash",
            "Date,Payee,Amount\n2026-10-15,Shop,\"-$1,234.56\"",
            "csv",
            "ymd",
            false,
        )
        .unwrap();
        assert_eq!(r[0].amount, -123456);
    }
    #[test]
    fn finds_the_columns_under_the_names_banks_give_them() {
        let mut b = sample();
        b.currency = "USD".into();
        let read = |csv: &str| {
            let r = preview(&b, "cash", csv, "csv", "mdy", false).unwrap();
            (r[0].date.clone(), r[0].payee.clone(), r[0].amount)
        };
        let shop = || ("2026-10-14".to_string(), "Shop".to_string(), -5000);
        assert_eq!(read("Details,Posting Date,Description,Amount,Type,Balance\nDEBIT,10/14/2026,Shop,-50.00,DEBIT_CARD,100.00"), shop());
        assert_eq!(
            read("Post Date,Transaction Description,Amount\n10/14/2026,Shop,-50.00"),
            shop()
        );
        assert_eq!(
            read("Trans Date,Description,Amount\n10/14/2026,Shop,-50.00"),
            shop()
        );
        // A transaction date wins over a posting date, whichever comes first.
        assert_eq!(
            read("Post Date,Trans. Date,Description,Amount\n10/15/2026,10/14/2026,Shop,-50.00"),
            shop()
        );
        assert_eq!(
            read("Trans. Date,Post Date,Description,Amount\n10/14/2026,10/15/2026,Shop,-50.00"),
            shop()
        );
        // Apple Card's layout: the merchant's name rather than the description with its address,
        // and the amount column named for its currency.
        assert_eq!(
            read("Transaction Date,Clearing Date,Description,Merchant,Category,Type,Amount (USD),Purchased By\n10/14/2026,10/15/2026,SHOP 12 MAIN ST SEATTLE WA,Shop,Other,Purchase,-50.00,Me"),
            shop()
        );
        // The column in the budget's currency is the one read.
        assert_eq!(
            read("Date,Payee,Amount (EUR),Amount (USD)\n10/14/2026,Shop,-46.00,-50.00"),
            shop()
        );
        let wrong = preview(
            &b,
            "cash",
            "Date,Payee,Amount (EUR)\n10/14/2026,Shop,-46.00",
            "csv",
            "mdy",
            false,
        )
        .err()
        .unwrap();
        assert!(wrong.contains("EUR") && wrong.contains("USD"), "{wrong}");
    }
    #[test]
    fn flips_signs_for_cards_that_write_purchases_as_positive() {
        let b = sample();
        let csv = "Date,Description,Amount\n2026-10-15,Shop,50.00\n2026-10-16,Payment,-20.00";
        let r = preview(&b, "cash", csv, "csv", "ymd", true).unwrap();
        assert_eq!([r[0].amount, r[1].amount], [-5000, 2000]);
        let r = preview(&b, "cash", csv, "csv", "ymd", false).unwrap();
        assert_eq!([r[0].amount, r[1].amount], [5000, -2000]);
        // Outflow and Inflow columns already say which way the money went.
        let r = preview(
            &b,
            "cash",
            "Date,Payee,Outflow,Inflow\n2026-10-15,Shop,50.00,",
            "csv",
            "ymd",
            true,
        )
        .unwrap();
        assert_eq!(r[0].amount, -5000);
    }

    fn accounts() -> Budget {
        let mut b = sample();
        for (id, kind) in [("revolut", AccountKind::Cash), ("pension", AccountKind::Tracking)] {
            b.accounts.push(Account {
                extra: Default::default(),
                id: id.into(),
                name: id.into(),
                kind,
                opening_balance: 0,
                archived: false,
            });
        }
        b
    }
    fn import(b: Budget, account: &str, csv: &str) -> (Budget, Vec<Row>) {
        let rows = preview(&b, account, csv, "csv", "ymd", false).unwrap();
        let choices = rows.iter().map(|r| {
            let only = (r.candidates.len() == 1).then(|| r.candidates[0].clone());
            Choice { row: r.clone(), action: if only.is_some() { "match" } else { "new" }.into(), match_id: only }
        }).collect();
        (apply(b, account, choices).unwrap(), rows)
    }
    #[test]
    fn the_other_side_of_money_moved_between_accounts_becomes_one_transfer() {
        let main = "Date,Payee,Amount\n2026-10-01,REVOLUT**1234,-200.00\n2026-10-02,Grocer,-50.00\n2026-10-03,Salary,3000.00";
        let (b, _) = import(accounts(), "cash", main);
        assert_eq!(b.transactions.len(), 3);
        let topup = b.transactions.iter().find(|t| t.payee == "REVOLUT**1234").unwrap().id.clone();
        // A day later in the other bank, and the same amount on a day too far away.
        let revolut = "Date,Description,Amount\n2026-10-02,Top-Up by *1234,200.00\n2026-10-20,Refund,50.00";
        let rows = preview(&b, "revolut", revolut, "csv", "ymd", false).unwrap();
        assert_eq!((rows[0].candidates.clone(), rows[0].transfer_candidates.clone()), (vec![topup.clone()], vec![topup.clone()]));
        assert!(rows[1].candidates.is_empty() && rows[1].transfer_candidates.is_empty());
        let (b, _) = import(b, "revolut", revolut);
        assert_eq!(b.transactions.len(), 4, "the top-up isn't added twice");
        let t = b.transactions.iter().find(|t| t.id == topup).unwrap();
        assert_eq!((t.account_id.as_str(), t.transfer_account_id.as_deref()), ("cash", Some("revolut")));
        assert_eq!((t.payee.as_str(), t.splits.len(), t.transfer_status), ("", 0, Some(Cleared::Cleared)));
        assert_eq!(t.extra["bankImportMatches"].as_object().unwrap().len(), 2);
        assert_eq!(t.extra["importedPayee"], "REVOLUT**1234");
        let balances = crate::engine::calculate(&b, "2026-10").unwrap();
        let balance = |id: &str| balances.accounts.iter().find(|a| a.id == id).unwrap().balance;
        assert_eq!((balance("cash"), balance("revolut")), (100_000 - 20_000 - 5_000 + 300_000, 20_000 + 5_000));
        // Both files are known now, on both sides.
        assert!(preview(&b, "revolut", revolut, "csv", "ymd", false).unwrap().iter().all(|r| r.duplicate));
        assert!(preview(&b, "cash", main, "csv", "ymd", false).unwrap().iter().all(|r| r.duplicate));
    }
    #[test]
    fn each_side_is_offered_once_to_the_nearest_row_and_never_over_a_match_here_or_a_category() {
        let main = "Date,Payee,Amount\n2026-10-01,To Revolut,-100.00\n2026-10-08,To Revolut,-100.00\n2026-10-05,Shop,-30.00";
        let (mut b, _) = import(accounts(), "cash", main);
        // Spending someone categorized, and income they assigned, aren't loose.
        let shop = b.transactions.iter_mut().find(|t| t.payee == "Shop").unwrap();
        shop.splits[0].category_id = Some("food".into());
        let mut gift = purchase();
        (gift.id, gift.account_id, gift.amount, gift.date) = ("gift".into(), "cash".into(), 4_000, "2026-10-05".into());
        gift.splits = vec![Split { category_id: None, amount: 4_000, extra: json!({"unassigned": true}).as_object().unwrap().clone().into() }];
        b.transactions.push(gift);
        // A transaction already in this account wins over one elsewhere.
        let mut here = purchase();
        (here.id, here.account_id, here.amount, here.date) = ("here".into(), "revolut".into(), 10_000, "2026-10-16".into());
        here.splits[0].amount = 10_000;
        here.splits[0].category_id = None;
        let revolut = "Date,Description,Amount\n2026-10-07,Top-up,100.00\n2026-10-02,Top-up,100.00\n2026-10-05,Card,30.00\n2026-10-05,Back,-40.00";
        let rows = preview(&b, "revolut", revolut, "csv", "ymd", false).unwrap();
        let offered = |r: &Row| b.transactions.iter().find(|t| r.transfer_candidates.first() == Some(&t.id)).map(|t| t.date.clone());
        assert_eq!(rows.iter().map(offered).collect::<Vec<_>>(), [Some("2026-10-08".into()), Some("2026-10-01".into()), None, None]);
        b.transactions.push(here);
        let rows = preview(&b, "revolut", revolut, "csv", "ymd", false).unwrap();
        assert_eq!((rows[0].candidates.clone(), rows[0].transfer_candidates.len()), (vec!["here".to_string()], 0));
        assert_eq!(rows[1].transfer_candidates.len(), 1);
        // The compact review a phone reads carries the same offers.
        let compact = preview_compact(&b, "revolut", revolut, "csv", "ymd", false, None).unwrap();
        let sets: Vec<_> = compact.items.iter().map(|i| &compact.candidate_sets[i.candidate_set].transfer_candidates).collect();
        assert_eq!(sets.iter().map(|s| s.len()).collect::<Vec<_>>(), [0, 1, 0, 0]);
        // A stale choice can't make a transfer of something no longer loose, or too far away.
        let mut far = rows[1].clone();
        far.date = "2026-10-09".into();
        let id = rows[1].transfer_candidates[0].clone();
        let error = apply(b.clone(), "revolut", vec![Choice { row: far, action: "match".into(), match_id: Some(id) }]).err().unwrap();
        assert!(error.contains("Preview again"), "{error}");
    }
    #[test]
    fn a_transfer_across_the_budgets_edge_keeps_its_category() {
        let (b, _) = import(accounts(), "cash", "Date,Payee,Amount\n2026-10-01,Pension fund,-500.00");
        let (b, _) = import(b, "pension", "Date,Payee,Amount\n2026-10-03,Contribution,500.00");
        assert_eq!(b.transactions.len(), 1);
        let t = &b.transactions[0];
        assert_eq!((t.transfer_account_id.as_deref(), t.splits.len()), (Some("pension"), 1));
        assert!(t.splits[0].category_id.as_deref().is_some_and(|c| crate::workflows::placeholder(&b, c)), "still needs a category");
    }
    #[test]
    fn bank_text_decides_which_side_is_chosen_and_which_is_only_suggested() {
        let mut b = accounts();
        b.accounts.push(Account { extra: Default::default(), id: "joint".into(), name: "Joint account".into(), kind: AccountKind::Cash, opening_balance: 0, archived: false });
        let main = "Date,Payee,Amount\n2026-10-01,Grocer,-100.00\n2026-10-01,Top-up REVOLUT**1234,-100.00\n2026-10-01,To Joint,-50.00\n2026-10-08,A. Example,-70.00";
        let (b, _) = import(b, "cash", main);
        let id = |b: &Budget, payee: &str| b.transactions.iter().find(|t| t.payee == payee).unwrap().id.clone();
        let revolut = "Date,Description,Amount\n2026-10-02,Top-up,100.00\n2026-10-01,Payment received,50.00\n2026-10-09,Payment from A. Example,70.00";
        let rows = preview(&b, "revolut", revolut, "csv", "ymd", false).unwrap();
        // The side whose text names Revolut wins over the grocer's, and is chosen.
        assert_eq!((rows[0].transfer_candidates.clone(), rows[0].transfer_sure), (vec![id(&b, "Top-up REVOLUT**1234")], true));
        // Money sent "To Joint" isn't the other side of a Revolut payment, whatever the amount.
        assert!(rows[1].transfer_candidates.is_empty());
        // Amount and date alone only suggest it.
        assert_eq!((rows[2].transfer_candidates.clone(), rows[2].transfer_sure), (vec![id(&b, "A. Example")], false));
        let (b, _) = import(b, "revolut", revolut);
        // Once they've been paired, the same wording between the same accounts is chosen next time.
        let (b, _) = import(b, "cash", "Date,Payee,Amount\n2026-11-08,A. Example,-80.00");
        let rows = preview(&b, "revolut", "Date,Description,Amount\n2026-11-09,Payment from A. Example,80.00", "csv", "ymd", false).unwrap();
        assert_eq!((rows[0].transfer_candidates.len(), rows[0].transfer_sure), (1, true));
    }
    #[test]
    fn an_earlier_bank_row_of_the_same_amount_does_not_hide_a_transfer() {
        let (b, _) = import(accounts(), "revolut", "Date,Description,Amount,ID\n2026-10-01,Weekly top-up,100.00,a1");
        let (b, _) = import(b, "cash", "Date,Payee,Amount\n2026-10-07,Revolut,-100.00");
        let rows = preview(&b, "revolut", "Date,Description,Amount,ID\n2026-10-08,Weekly top-up,100.00,a2", "csv", "ymd", false).unwrap();
        // Last week's top-up, already a bank row, is still listed, so the row needs a choice; the transfer is there to choose.
        assert_eq!(rows[0].candidates.len(), 2);
        assert_eq!((rows[0].previously_matched_candidates.len(), rows[0].transfer_candidates.len()), (1, 1));
    }
    #[test]
    fn a_transfer_written_the_same_way_week_after_week_is_chosen() {
        let weeks = |n: usize, payee: &str, sign: &str, from: u32| {
            let mut csv = String::from("Date,Payee,Amount\n");
            for w in 0..n {
                csv.push_str(&format!("2026-09-{:02},{payee},{sign}50.00\n", from + 7 * w as u32));
            }
            csv
        };
        for (n, sure) in [(RECURRING, true), (RECURRING - 1, false)] {
            let (b, _) = import(accounts(), "cash", &weeks(n, "Weekly saving", "-", 1));
            let rows = preview(&b, "revolut", &weeks(n, "From A. Example", "", 2), "csv", "ymd", false).unwrap();
            assert!(rows.iter().all(|r| r.transfer_candidates.len() == 1 && r.transfer_sure == sure), "{n} weeks");
        }
    }
}
