//! Bank-text templates: a format the assistant wrote while reading, read on the phone next time.
//! A template is lines of literal words and typed slots, compiled here rather than handed to a
//! regular-expression engine, so a template can neither hang nor match everything. A text a
//! template reads still goes to the assistant, which reviews the reading.
use crate::{
    domain::Budget,
    sms_import::{self, Analysis, Evidence, Item, Message, Transaction},
    workflows::KnownPayees,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What a format means: money moving (a transaction or a hold, its kind and sign), or a text to
/// skip and why.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Outcome {
    #[serde(default)]
    pub disposition: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    /// "-" for money out, "+" for money in.
    #[serde(default)]
    pub sign: Option<String>,
    #[serde(default)]
    pub skip: Option<String>,
    #[serde(default)]
    pub memo: Option<String>,
}

/// A sender's format, as the assistant wrote it and the phone keeps it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Template {
    pub id: String,
    /// The sender's key (`smsSenderKey` on the phone: a name lowercased, a number's digits).
    pub sender: String,
    pub lines: Vec<String>,
    pub outcome: Outcome,
    /// The review note in the app's language; "{payee}" is filled in.
    #[serde(default)]
    pub note: String,
    /// The payee when the format names none (a salary, a deposit).
    #[serde(default)]
    pub payee: Option<String>,
}

/// A payee as the bank writes it, to the payee and category the user settled on.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Payee {
    pub sender: String,
    pub raw: String,
    pub payee: String,
    #[serde(default)]
    pub category_id: Option<String>,
}

/// The phone's sender key: a name lowercased, a number's punctuation dropped.
pub fn sender_key(sender: &str) -> String {
    let t = sender.trim();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || "+ ()-".contains(c)) {
        t.chars().filter(|c| !" ()-".contains(*c)).collect()
    } else {
        t.to_lowercase()
    }
}
fn payee_key(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot { Amount, Total, Balance, Currency, Date, Time, Ref, Other, Payee, Code, Number, Any }
#[derive(Clone, Debug)]
enum Token { Literal(Vec<char>), Slot(Slot, Option<String>) }
#[derive(Clone, Debug)]
struct Line { optional: bool, tokens: Vec<Token> }

/// A template ready to match: its lines parsed, and how specific it is (its literal characters).
#[derive(Clone, Debug)]
pub struct Compiled {
    pub template: Template,
    lines: Vec<Line>,
    specific: usize,
}

const DATE_PARTS: [&str; 5] = ["dd", "mm", "mmm", "yy", "yyyy"];

fn slot(name: &str) -> Result<(Slot, Option<String>), String> {
    let (name, arg) = name.split_once(':').map_or((name, None), |(n, a)| (n, Some(a.to_string())));
    let kind = match name {
        "amount" => Slot::Amount, "total" => Slot::Total, "balance" => Slot::Balance, "currency" => Slot::Currency,
        "date" => Slot::Date, "time" => Slot::Time, "ref" => Slot::Ref, "other" => Slot::Other, "payee" => Slot::Payee,
        "code" => Slot::Code, "n" => Slot::Number, "any" => Slot::Any,
        _ => return Err(format!("Unknown slot {{{name}}}.")),
    };
    if kind == Slot::Date {
        let order = arg.as_deref().ok_or("A date slot names its order, like {date:dd/mm/yy}.")?;
        let parts = date_parts(order);
        let mut sorted: Vec<&str> = parts.iter().map(|p| match *p { "yyyy" => "yy", "mmm" => "mm", p => p }).collect();
        sorted.sort();
        if !parts.iter().all(|p| DATE_PARTS.contains(p)) || (sorted != ["dd", "mm", "yy"] && sorted != ["dd", "mm"]) {
            return Err(format!("Write a date's order with dd, mm or mmm, and yy or yyyy when it has a year, not {order}."));
        }
    } else if arg.is_some() {
        return Err(format!("Only a date slot takes an order, not {{{name}}}."));
    }
    Ok((kind, arg))
}

fn parse_line(text: &str) -> Result<Line, String> {
    let (optional, text) = text.strip_prefix('?').map_or((false, text), |t| (true, t));
    let mut tokens = vec![];
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('{') {
            Some(0) => {
                let end = rest.find('}').ok_or("Close every slot with }.")?;
                let (kind, arg) = slot(&rest[1..end])?;
                tokens.push(Token::Slot(kind, arg));
                rest = &rest[end + 1..];
            }
            Some(at) => { tokens.push(Token::Literal(rest[..at].chars().collect())); rest = &rest[at..]; }
            None => { tokens.push(Token::Literal(rest.chars().collect())); rest = ""; }
        }
    }
    tokens.retain(|t| !matches!(t, Token::Literal(l) if l.iter().all(|c| c.is_whitespace())));
    if tokens.is_empty() {
        return Err("A template line needs words or a slot.".into());
    }
    // A payee or free text runs to the next literal words, so one can't sit right before another slot.
    for pair in tokens.windows(2) {
        if let [Token::Slot(Slot::Payee | Slot::Any, _), Token::Slot(..)] = pair {
            return Err("Put words between {payee} or {any} and the next slot.".into());
        }
    }
    Ok(Line { optional, tokens })
}

/// Checks and parses a template. A money format needs an amount; a skip format a known reason.
pub fn compile(t: &Template) -> Result<Compiled, String> {
    if t.lines.is_empty() || t.lines.len() > 30 || t.lines.iter().any(|l| l.chars().count() > 300) {
        return Err("A template has 1–30 lines of at most 300 characters.".into());
    }
    let lines = t.lines.iter().map(|l| parse_line(l)).collect::<Result<Vec<_>, _>>()?;
    let specific: usize = lines.iter().flat_map(|l| &l.tokens).map(|t| match t {
        Token::Literal(l) => l.iter().filter(|c| !c.is_whitespace()).count(),
        Token::Slot(..) => 0,
    }).sum();
    if specific < 4 {
        return Err("A template needs some of the format's own words, not only slots.".into());
    }
    if lines.iter().all(|l| l.optional) {
        return Err("A template needs at least one line that's always there.".into());
    }
    let slots: Vec<Slot> = lines.iter().flat_map(|l| &l.tokens).filter_map(|t| match t { Token::Slot(s, _) => Some(*s), _ => None }).collect();
    let o = &t.outcome;
    match (&o.skip, &o.disposition) {
        (Some(skip), None) if sms_import::SKIPS.contains(&skip.as_str()) => {}
        (None, Some(d)) if d == "transaction" || d == "pending" => {
            if !matches!(o.kind.as_deref(), Some("expense" | "income" | "refund" | "transfer")) || !matches!(o.sign.as_deref(), Some("-" | "+")) {
                return Err("A money template names its kind and sign.".into());
            }
            if !slots.iter().any(|s| matches!(s, Slot::Amount | Slot::Total)) {
                return Err("A money template has an {amount} or {total}.".into());
            }
            if !slots.contains(&Slot::Payee) && t.payee.as_deref().is_none_or(|p| p.trim().is_empty()) {
                return Err("A money template has a {payee}, or names the payee.".into());
            }
        }
        _ => return Err("Give a template either a skip reason or a disposition of transaction or pending.".into()),
    }
    // A currency may come again beside a balance; the amount's is the one read.
    for s in [Slot::Amount, Slot::Total, Slot::Date, Slot::Ref, Slot::Other, Slot::Payee] {
        if slots.iter().filter(|x| **x == s).count() > 1 {
            return Err("Use each of amount, total, date, ref, other and payee once.".into());
        }
    }
    Ok(Compiled { template: t.clone(), lines, specific })
}

/// A text as templates read it: direction marks gone, spaces collapsed, blank lines dropped.
fn normalised(body: &str) -> Vec<Vec<char>> {
    body.replace("\r\n", "\n").replace('\r', "\n").lines().map(|line| {
        let kept: String = line.chars().filter(|c| !matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')).collect();
        kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect::<Vec<char>>()
    }).filter(|l| !l.is_empty()).collect()
}

fn skip_space(s: &[char], mut at: usize) -> usize {
    while at < s.len() && s[at].is_whitespace() { at += 1 }
    at
}
/// Matches literal words at `at`, spaces optional on either side; returns where it ends.
fn literal(s: &[char], at: usize, lit: &[char]) -> Option<usize> {
    let mut i = at;
    for c in lit.iter().filter(|c| !c.is_whitespace()) {
        i = skip_space(s, i);
        if i >= s.len() || !s[i].to_lowercase().eq(c.to_lowercase()) { return None; }
        i += 1;
    }
    Some(i)
}
/// Separators a bank groups an amount's digits with, or puts before its decimals: "1,234.56",
/// "1.234,56", "1'234.50".
const SEPARATORS: [char; 3] = [',', '.', '\''];
/// Where a number starting at `at` ends: digits, and with `grouping` the separators between them,
/// a space among them only between groups of three ("1 249,00" as French banks write it).
fn number(s: &[char], at: usize, grouping: bool) -> Option<usize> {
    let mut i = at;
    let mut group = 0;
    while i < s.len() {
        let three = || (1..=3).all(|k| s.get(i + k).is_some_and(char::is_ascii_digit)) && s.get(i + 4).is_none_or(|c| !c.is_ascii_digit());
        if s[i].is_ascii_digit() {
            group += 1;
        } else if grouping && i > at && SEPARATORS.contains(&s[i]) && s.get(i + 1).is_some_and(char::is_ascii_digit) {
            group = 0;
        } else if grouping && i > at && s[i] == ' ' && (1..=3).contains(&group) && three() {
            group = 0;
        } else {
            break;
        }
        i += 1;
    }
    (i > at).then_some(i)
}

/// An amount as a bank writes it, as a plain decimal with a point: the separator before the last
/// one or two digits is the decimal point and the others group ("1.234,56" and "1,234.56" are
/// "1234.56", "12,50" is "12.50"). A comma before three digits groups; a point before three digits
/// is a decimal point unless another point groups ("1.250" stays "1.250", for a currency with
/// three decimals; "1.250.000" is "1250000").
pub fn decimal(number: &str) -> String {
    let chars: Vec<char> = number.chars().filter(|c| *c != ' ').collect();
    let number: String = chars.iter().collect();
    let Some(last) = chars.iter().rposition(|c| SEPARATORS.contains(c)) else { return number };
    let after = chars.len() - last - 1;
    let sep = chars[last];
    let point = sep != '\'' && match after {
        1 | 2 => true,
        3 => sep == '.' && chars[..last].iter().all(|c| *c != '.'),
        _ => false,
    };
    let digits = |part: &[char]| part.iter().filter(|c| c.is_ascii_digit()).collect::<String>();
    if point { format!("{}.{}", digits(&chars[..last]), digits(&chars[last + 1..])) } else { digits(&chars) }
}

/// Currency marks banks write beside an amount, and the currencies each can mean: a mark several
/// share is the budget's currency when it's one of them, else the first. Where one mark begins
/// another, the longer comes first.
const MARKS: &[(&str, &[&str])] = &[
    ("US$", &["USD"]), ("CA$", &["CAD"]), ("C$", &["CAD"]), ("AU$", &["AUD"]), ("A$", &["AUD"]), ("NZ$", &["NZD"]), ("S$", &["SGD"]),
    ("HK$", &["HKD"]), ("NT$", &["TWD"]), ("R$", &["BRL"]), ("MX$", &["MXN"]),
    ("$", &["USD", "CAD", "AUD", "NZD", "SGD", "HKD", "MXN", "TWD", "ARS", "CLP", "COP"]),
    ("£", &["GBP", "EGP", "LBP"]), ("€", &["EUR"]), ("₹", &["INR"]), ("Rs.", &["INR", "PKR", "LKR", "NPR", "MUR"]), ("Rs", &["INR", "PKR", "LKR", "NPR", "MUR"]),
    ("¥", &["JPY", "CNY"]), ("元", &["CNY"]), ("₩", &["KRW"]), ("₺", &["TRY"]), ("TL", &["TRY"]), ("₦", &["NGN"]), ("₱", &["PHP"]), ("₫", &["VND"]),
    ("₪", &["ILS"]), ("฿", &["THB"]), ("₴", &["UAH"]), ("₽", &["RUB"]), ("₸", &["KZT"]), ("zł", &["PLN"]), ("Kč", &["CZK"]), ("Ft", &["HUF"]),
    ("kr.", &["DKK", "SEK", "NOK", "ISK"]), ("kr", &["SEK", "NOK", "DKK", "ISK"]), ("RM", &["MYR"]), ("Rp", &["IDR"]), ("KSh", &["KES"]),
    ("ر.س", &["SAR"]), ("ر.ق", &["QAR"]), ("ر.ع", &["OMR"]), ("ريال", &["SAR", "QAR", "OMR", "YER"]), ("رس", &["SAR"]), ("SR", &["SAR"]),
    ("د.إ", &["AED"]), ("درهم", &["AED", "MAD"]), ("د.ك", &["KWD"]), ("د.ب", &["BHD"]), ("دينار", &["KWD", "BHD", "JOD", "IQD", "LYD", "TND", "DZD"]),
    ("ج.م", &["EGP"]),
];
/// Currency codes a text may give an amount in, so "OTP 482193" isn't one.
const CODES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "ARS", "AUD", "AZN", "BAM", "BDT", "BGN", "BHD", "BRL", "BYN", "CAD", "CHF", "CLP", "CNY", "COP", "CRC", "CZK",
    "DKK", "DOP", "DZD", "EGP", "ETB", "EUR", "GBP", "GEL", "GHS", "GTQ", "HKD", "HUF", "IDR", "ILS", "INR", "IQD", "IRR", "ISK", "JMD", "JOD",
    "JPY", "KES", "KRW", "KWD", "KZT", "LBP", "LKR", "LYD", "MAD", "MXN", "MYR", "NGN", "NOK", "NPR", "NZD", "OMR", "PEN", "PHP", "PKR", "PLN",
    "QAR", "RON", "RSD", "RUB", "SAR", "SEK", "SGD", "SYP", "THB", "TND", "TRY", "TWD", "TZS", "UAH", "UGX", "USD", "UYU", "UZS", "VND", "XAF",
    "XOF", "YER", "ZAR", "RMB",
];
/// The mark or code a text writes starting at `at`, and where it ends.
fn currency_mark(s: &[char], at: usize) -> Option<usize> {
    for (mark, _) in MARKS {
        let m: Vec<char> = mark.chars().collect();
        // A mark of Latin letters stands alone ("RM 12", not "RMB"); a symbol, or an Arabic word, can touch what follows.
        let alone = !m[m.len() - 1].is_ascii_alphabetic() || s.get(at + m.len()).is_none_or(|c| !c.is_ascii_alphabetic());
        if s.len() >= at + m.len() && s[at..at + m.len()] == m[..] && alone { return Some(at + m.len()); }
    }
    (s.len() >= at + 3 && s[at..at + 3].iter().all(|c| c.is_ascii_alphabetic()) && s.get(at + 3).is_none_or(|c| !c.is_ascii_alphabetic())).then_some(at + 3)
}
/// Whether a currency stands right before or after the number at `start..end`: "SAR 1,250",
/// "بـ:SAR 23.50", "$45.00", "1.234,56 €", "120 ريال", "Rs.2,500".
pub fn beside_currency(chars: &[char], start: usize, end: usize) -> bool {
    let known = |at: usize| currency_mark(chars, at).filter(|&e| {
        let word: String = chars[at..e].iter().collect();
        MARKS.iter().any(|(m, _)| *m == word) || CODES.contains(&word.to_uppercase().as_str())
    });
    let mut after = end;
    while after < chars.len() && chars[after].is_whitespace() { after += 1 }
    if known(after).is_some() { return true; }
    let mut before = start;
    while before > 0 && (chars[before - 1].is_whitespace() || chars[before - 1] == ':') { before -= 1 }
    // A mark ending where the number's spaces begin, its first letter not inside a word.
    (1..=5.min(before)).any(|len| {
        let at = before - len;
        known(at) == Some(before) && (at == 0 || !chars[at - 1].is_alphabetic() || !chars[at].is_alphabetic())
    })
}
/// Where each number in a text starts and ends, with its separators: "1,250.00" is one.
pub fn numbers(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && chars[i - 1].is_ascii_digit()) {
            i += 1;
            continue;
        }
        let end = number(chars, i, true).unwrap_or(i + 1);
        out.push((i, end));
        i = end;
    }
    out
}
/// The amounts a text gives beside a currency, as plain decimals without trailing zeros:
/// "SAR 1,250.00", "1.250,00 €" and "$1,250" are each "1250".
pub fn amounts(body: &str) -> std::collections::HashSet<String> {
    let chars: Vec<char> = body.chars().collect();
    numbers(&chars).into_iter().filter(|&(s, e)| beside_currency(&chars, s, e)).map(|(s, e)| {
        let n = decimal(&chars[s..e].iter().collect::<String>());
        if n.contains('.') { n.trim_end_matches('0').trim_end_matches('.').to_owned() } else { n }
    }).collect()
}

/// A date's parts in the order a template writes it ("dd/mm/yy", "mmm dd, yyyy", "dd-mmm").
fn date_parts(order: &str) -> Vec<&str> {
    order.split(|c: char| !c.is_ascii_alphabetic()).filter(|p| !p.is_empty()).collect()
}
const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
/// A month's name or its first three letters or more ("Nov", "Sept", "October"), from 1.
fn month(name: &str) -> Option<u32> {
    let name = name.to_lowercase();
    (name.chars().count() >= 3).then_some(())?;
    MONTHS.iter().position(|m| m.starts_with(&name)).map(|i| i as u32 + 1)
}
/// A date written in `order` from `at`: where it ends, and its day, month and year if it gives one.
/// Between numbers any of / - . stands for the order's; spaces and a comma are optional.
fn read_date(s: &[char], at: usize, order: &str) -> Option<(usize, u32, u32, Option<i32>)> {
    let (mut d, mut m, mut y) = (None, None, None);
    let mut i = at;
    let mut first = true;
    for part in date_parts(order) {
        if !first {
            while i < s.len() && (s[i].is_whitespace() || s[i] == ',') { i += 1 }
            if i < s.len() && "/-.".contains(s[i]) { i += 1 }
            while i < s.len() && s[i].is_whitespace() { i += 1 }
        }
        first = false;
        let start = i;
        if part == "mmm" {
            while i < s.len() && s[i].is_alphabetic() { i += 1 }
            m = Some(month(&s[start..i].iter().collect::<String>())?);
            // "Nov." as some write it.
            if i < s.len() && s[i] == '.' && s.get(i + 1).is_none_or(|c| !c.is_ascii_digit()) { i += 1 }
            continue;
        }
        while i < s.len() && s[i].is_ascii_digit() && i - start < 4 { i += 1 }
        let digits: String = s[start..i].iter().collect();
        let value: i32 = digits.parse().ok()?;
        match (part, digits.len()) {
            ("dd", 1 | 2) => d = Some(value as u32),
            ("mm", 1 | 2) => m = Some(value as u32),
            ("yy", 2) => y = Some(2000 + value),
            ("yyyy", 4) => y = Some(value),
            _ => return None,
        }
    }
    Some((i, d?, m?, y))
}
fn typed(s: &[char], at: usize, kind: Slot, arg: Option<&str>) -> Option<usize> {
    let i = at;
    match kind {
        Slot::Amount | Slot::Total | Slot::Balance => number(s, i, true),
        Slot::Code | Slot::Number => number(s, i, false),
        Slot::Currency => currency_mark(s, i),
        Slot::Date => read_date(s, i, arg?).map(|(end, ..)| end),
        Slot::Time => {
            let mut j = i;
            while j < s.len() && (s[j].is_ascii_digit() || s[j] == ':') { j += 1 }
            let t: String = s[i..j].iter().collect();
            if !(t.contains(':') && t.split(':').all(|p| (1..=2).contains(&p.len()))) { return None; }
            // "1:05 PM", "1:05pm", "1:05 p.m."
            let mut k = j;
            while k < s.len() && s[k].is_whitespace() { k += 1 }
            let rest: String = s[k..s.len().min(k + 4)].iter().collect::<String>().to_lowercase();
            for suffix in ["a.m.", "p.m.", "am", "pm"] {
                if rest.starts_with(suffix) && s.get(k + suffix.len()).is_none_or(|c| !c.is_alphanumeric()) { return Some(k + suffix.len()); }
            }
            Some(j)
        }
        Slot::Ref | Slot::Other => {
            // A mask may lead with points ("...4421"); after the digits a point ends the sentence.
            let mut j = i;
            while j < s.len() && ".*xX#•…".contains(s[j]) { j += 1 }
            while j < s.len() && (s[j].is_ascii_digit() || "*xX#•…".contains(s[j])) { j += 1 }
            (s[i..j].iter().filter(|c| c.is_ascii_digit()).count() >= 2).then_some(j)
        }
        Slot::Payee | Slot::Any => None,
    }
}

/// One line against a template line: the slots' text, or None.
fn match_line(s: &[char], tokens: &[Token]) -> Option<Vec<(Slot, Option<String>, String)>> {
    let mut at = 0;
    let mut found = vec![];
    for (n, token) in tokens.iter().enumerate() {
        at = skip_space(s, at);
        match token {
            Token::Literal(l) => at = literal(s, at, l)?,
            Token::Slot(kind @ (Slot::Payee | Slot::Any), arg) => {
                // Free text runs to the next literal words, or the end of the line.
                let end = match tokens.get(n + 1) {
                    Some(Token::Literal(l)) => (at..=s.len()).find(|&e| e > at && literal(s, e, l).is_some())?,
                    _ => s.len(),
                };
                let value: String = s[at..end].iter().collect::<String>().trim().to_string();
                if value.is_empty() { return None; }
                found.push((*kind, arg.clone(), value));
                at = end;
            }
            Token::Slot(kind, arg) => {
                let end = typed(s, at, *kind, arg.as_deref())?;
                found.push((*kind, arg.clone(), s[at..end].iter().collect()));
                at = end;
            }
        }
    }
    (skip_space(s, at) == s.len()).then_some(found)
}

/// What a template read from a text: each slot's text as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub amount: Option<String>,
    pub total: Option<String>,
    pub balance: Option<String>,
    pub currency: Option<String>,
    pub date: Option<(String, String)>,
    pub time: Option<String>,
    pub reference: Option<String>,
    pub other: Option<String>,
    pub payee: Option<String>,
}

impl Compiled {
    /// The text read by this template, every line accounted for, or None.
    pub fn read(&self, body: &str) -> Option<Reading> {
        let text = normalised(body);
        let mut reading = Reading::default();
        let mut at = 0;
        // A currency written again beside a balance isn't the amount's: the one beside the amount wins.
        let mut beside_amount = false;
        for line in &self.lines {
            match text.get(at).and_then(|s| match_line(s, &line.tokens)) {
                Some(found) => {
                    let kinds: Vec<Slot> = found.iter().map(|(kind, ..)| *kind).collect();
                    for (n, (kind, arg, value)) in found.into_iter().enumerate() {
                        match kind {
                            Slot::Amount => reading.amount = Some(value),
                            Slot::Total => reading.total = Some(value),
                            Slot::Balance => reading.balance = Some(value),
                            Slot::Currency => {
                                let beside = [n.checked_sub(1), Some(n + 1)].into_iter().flatten()
                                    .any(|k| matches!(kinds.get(k), Some(Slot::Amount | Slot::Total)));
                                if reading.currency.is_none() || (beside && !beside_amount) {
                                    reading.currency = Some(value);
                                    beside_amount = beside;
                                }
                            }
                            Slot::Date => reading.date = Some((value, arg.unwrap_or_default())),
                            Slot::Time => reading.time = Some(value),
                            Slot::Ref => reading.reference = Some(value),
                            Slot::Other => reading.other = Some(value),
                            Slot::Payee => reading.payee = Some(value),
                            Slot::Code | Slot::Number | Slot::Any => {}
                        }
                    }
                    at += 1;
                }
                None if line.optional => {}
                None => return None,
            }
        }
        (at == text.len()).then_some(reading)
    }
}

/// A date written in `order` ("dd/mm/yy", "mmm dd"), as YYYY-MM-DD. One with no year is in the year
/// that puts it on or before the day the text came ([received]), give or take a day.
fn date(value: &str, order: &str, received: &str) -> Option<String> {
    let chars: Vec<char> = value.chars().collect();
    let (end, d, m, y) = read_date(&chars, 0, order)?;
    if end != chars.len() { return None; }
    let came = chrono::NaiveDate::parse_from_str(received, "%Y-%m-%d").ok();
    let y = match (y, came) {
        (Some(y), _) => y,
        (None, Some(came)) => {
            use chrono::Datelike;
            let this = chrono::NaiveDate::from_ymd_opt(came.year(), m, d)?;
            if this > came + chrono::Duration::days(1) { came.year() - 1 } else { came.year() }
        }
        (None, None) => return None,
    };
    chrono::NaiveDate::from_ymd_opt(y, m, d).map(|day| day.format("%Y-%m-%d").to_string())
}
/// The three-letter code for a currency as a text writes it ([MARKS]); a mark several currencies
/// share is the [budget]'s when it's one of them.
fn currency_code(written: &str, budget: &str) -> String {
    if let Some((_, codes)) = MARKS.iter().find(|(m, _)| *m == written) {
        return if codes.contains(&budget) { budget.to_string() } else { codes[0].to_string() };
    }
    match written.to_uppercase().as_str() { "RMB" => "CNY".into(), w => w.into() }
}

/// The item a template reads from a text, as the assistant would write it; checked as the
/// assistant's would be. None when the reading doesn't make a valid item.
/// The item a template's reading of a text makes. Its payee is the one learned for the bank's name,
/// else the user's payee the name surely is ([known]: "MASHAR E" is their "Mashar"), else the
/// bank's own words; the category comes with it.
pub fn fill(b: &Budget, c: &Compiled, m: &Message, reading: &Reading, payees: &HashMap<(String, String), Payee>, known: Option<&KnownPayees>) -> Option<Item> {
    let t = &c.template;
    let o = &t.outcome;
    let item = if let Some(skip) = &o.skip {
        Item { source_ids: vec![m.id.clone()], disposition: "ignore".into(), skip: Some(skip.clone()), note: String::new(), evidence: vec![],
            transaction: None, unsure: vec![], template: Some(t.id.clone()), raw: None, kept: false }
    } else {
        let negative = o.sign.as_deref() == Some("-");
        let signed = |n: &str| format!("{}{}", if negative { "-" } else { "" }, decimal(n));
        let written = reading.amount.as_ref().or(reading.total.as_ref())?;
        let currency = reading.currency.as_deref().map(|w| currency_code(w, &b.currency)).unwrap_or_else(|| b.currency.clone());
        let amount = if currency == b.currency { Some(signed(written)) } else { reading.total.as_deref().map(signed) };
        // The day the text was written for, or the day it came when the format gives none.
        let day = match &reading.date { Some((value, order)) => date(value, order, &m.received_date)?, None => m.received_date.clone() };
        let raw = reading.payee.clone();
        let learned = raw.as_deref().and_then(|r| payees.get(&(t.sender.clone(), payee_key(r))));
        let mine = if learned.is_none() { raw.as_deref().zip(known).and_then(|(r, k)| k.same(r)) } else { None };
        let payee = learned.map(|p| p.payee.clone()).or_else(|| mine.map(|p| p.name.clone())).or_else(|| raw.clone()).or_else(|| t.payee.clone())?;
        let kind = o.kind.clone()?;
        let category = learned.map(|p| p.category_id.clone()).unwrap_or_else(|| mine.and_then(|p| p.category_id.clone()))
            .filter(|id| kind != "transfer" && b.categories.iter().any(|c| c.id == *id && c.payment_account().is_none()));
        Item {
            source_ids: vec![m.id.clone()], disposition: o.disposition.clone()?, skip: None,
            note: t.note.replace("{payee}", &payee).trim().to_string().chars().take(200).collect::<String>().trim().to_string(),
            evidence: vec![Evidence { source_id: m.id.clone(), quote: written.clone() }],
            transaction: Some(Transaction {
                date: day, amount, currency: if reading.amount.is_some() { currency } else { b.currency.clone() }, written_amount: decimal(written), payee,
                memo: o.memo.clone().unwrap_or_default(), kind, account_ref: reading.reference.clone(), other_ref: reading.other.clone(),
                other_account: None, category_id: category, account_id: None, transfer_account_id: None,
            }),
            unsure: vec![], template: Some(t.id.clone()), raw, kept: false,
        }
    };
    let mut item = item;
    if item.disposition != "ignore" && item.note.is_empty() { item.note = item.transaction.as_ref()?.payee.clone(); }
    sms_import::validate_analysis(b, std::slice::from_ref(m), &Analysis { items: vec![item.clone()] }).ok().map(|_| item)
}

/// The texts split for the assistant: those a template read (with the item it filled, for the
/// assistant to review), those it reads in full, and junk settled on the phone: texts a trusted
/// skip template read, and codes the code filter recognised.
#[derive(Debug, Default)]
pub struct Lanes {
    pub review: Vec<(usize, Item)>,
    pub read: Vec<usize>,
    pub junk: Vec<(usize, Item)>,
    /// Texts two equally specific templates both read: read in full, and the clash counted.
    pub conflicts: usize,
}

impl Lanes {
    /// Each text's lane, in order: "junk", "review" or "read".
    pub fn of(&self, count: usize) -> Vec<&'static str> {
        let mut lanes = vec!["read"; count];
        for (i, _) in &self.review { lanes[*i] = "review" }
        for (i, _) in &self.junk { lanes[*i] = "junk" }
        lanes
    }
}

/// What a code the filter recognised says read it, in place of a template's id.
pub const CODE_FILTER: &str = "filter:code";

/// Phrases only a one-time code text uses, in the languages banks write in.
const CODE_PHRASES: &[&str] = &["otp", "one-time password", "one time password", "one-time pin", "one time pin", "verification code", "security code",
    "passcode", "do not share", "don't share", "never share",
    "لا تشارك الرمز", "رمز التحقق", "رمز المرور", "كلمة المرور لمرة واحدة", "كلمة مرور صالحة لمرة واحدة", "رمز التفعيل",
    "código de verificación", "código de seguridad", "no compartas", "code de vérification", "ne partagez", "bestätigungscode", "nicht weitergeben",
    "codice di verifica", "código de verificação", "doğrulama kodu", "kode verifikasi"];
/// Words a code follows ("OTP: 4821", "رمز : 482193", "código: 4821").
const CODE_WORDS: &[&str] = &["otp", "code", "password", "pin", "passcode", "رمز", "كلمة المرور", "código", "codice", "kode", "kodu"];

/// A one-time code before any template knows the format: a code phrase, and a 4–8 digit number
/// shortly after a code word, so a purchase that ends "never share your OTP" isn't one.
/// Conservative on purpose: a miss only means the assistant reads the text.
pub fn looks_like_code(body: &str) -> bool {
    let lower: Vec<char> = body.to_lowercase().chars().collect();
    let text: String = lower.iter().collect();
    let words: Vec<String> = text.split(|c: char| !c.is_alphanumeric()).map(str::to_owned).collect();
    let phrase = CODE_PHRASES.iter().any(|p| if p.is_ascii() && !p.contains(' ') { words.iter().any(|w| w == p) } else { text.contains(p) });
    if !phrase { return false; }
    // Where each code word starts and ends, in characters.
    let mut found = vec![];
    for word in CODE_WORDS {
        let w: Vec<char> = word.chars().collect();
        for i in 0..(lower.len() + 1).saturating_sub(w.len()) {
            if lower[i..i + w.len()] == w[..] { found.push((i, i + w.len())) }
        }
    }
    let mut i = 0;
    while i < lower.len() {
        if lower[i].is_ascii_digit() {
            let start = i;
            while i < lower.len() && lower[i].is_ascii_digit() { i += 1 }
            let digits = (4..=8).contains(&(i - start));
            // After a code word ("OTP: 4821"), or first in the text with one soon after ("482193 is your OTP").
            let after = found.iter().any(|&(_, e)| e <= start && start - e <= 60);
            let first = lower[..start].iter().all(|c| !c.is_alphanumeric()) && found.iter().any(|&(s, _)| s >= i && s - i <= 40);
            if digits && (after || first) { return true; }
        } else { i += 1 }
    }
    false
}

/// One in fifty texts a trusted skip template reads still goes to review, so a template that
/// started skipping money would be caught. Codes never do: there's nothing in them to check.
fn audited(m: &Message, skip: &str) -> bool {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(m.id.as_bytes());
    skip != "code" && (u16::from(d[0]) << 8 | u16::from(d[1])) % 50 == 0
}

/// Sorts texts into lanes by the sender's templates, the most specific first. A skip template in
/// [trusted] settles its texts on the phone; every other template's texts are reviewed.
pub fn lanes(b: &Budget, messages: &[Message], templates: &[Template], payees: &[Payee], trusted: &std::collections::HashSet<String>) -> Lanes {
    let mut by_sender: HashMap<String, Vec<Compiled>> = HashMap::new();
    for t in templates {
        if let Ok(c) = compile(t) { by_sender.entry(t.sender.clone()).or_default().push(c) }
    }
    for list in by_sender.values_mut() { list.sort_by(|a, b| b.specific.cmp(&a.specific)) }
    let payees = payee_map(payees);
    let known = KnownPayees::new(b);
    let mut lanes = Lanes::default();
    for (i, m) in messages.iter().enumerate() {
        let candidates = by_sender.get(&sender_key(&m.sender)).map(Vec::as_slice).unwrap_or_default();
        let mut matched = candidates.iter().filter_map(|c| c.read(&m.body).map(|r| (c, r)));
        let first = matched.next();
        let tie = first.as_ref().is_some_and(|(c, _)| matched.any(|(other, _)| other.specific == c.specific));
        match first {
            Some(_) if tie => { lanes.conflicts += 1; lanes.read.push(i) }
            Some((c, reading)) => match fill(b, c, m, &reading, &payees, Some(&known)) {
                Some(item) if item.skip.as_deref().is_some_and(|skip| trusted.contains(&c.template.id) && !audited(m, skip)) => {
                    lanes.junk.push((i, Item { kept: true, ..item }))
                }
                Some(item) => lanes.review.push((i, item)),
                None => lanes.read.push(i),
            },
            None if looks_like_code(&m.body) => lanes.junk.push((i, Item {
                source_ids: vec![m.id.clone()], disposition: "ignore".into(), skip: Some("code".into()), note: String::new(), evidence: vec![],
                transaction: None, unsure: vec![], template: Some(CODE_FILTER.into()), raw: None, kept: true,
            })),
            None => lanes.read.push(i),
        }
    }
    lanes
}

pub fn payee_map(payees: &[Payee]) -> HashMap<(String, String), Payee> {
    payees.iter().map(|p| ((p.sender.clone(), payee_key(&p.raw)), p.clone())).collect()
}

/// How a template's replay on a batch went: texts it read the way the assistant did, and payees
/// that differ (the format was right; the payee is learned apart).
#[derive(Debug, Default, PartialEq)]
pub struct Replay {
    pub agreed: usize,
    pub payees: Vec<(String, String, Option<String>)>,
}

/// Keeps a template only when it reads its example texts, and every text of the batch it reads,
/// the way the assistant's final items do: the same outcome, kind, amount, currency, date and
/// references. Texts grouped with others (both sides of a transfer) aren't compared.
pub fn replay(b: &Budget, t: &Template, messages: &[Message], items: &[Item], examples: &[usize]) -> Result<Replay, String> {
    let c = compile(t)?;
    let by_source: HashMap<&str, &Item> = items.iter().flat_map(|i| i.source_ids.iter().map(move |s| (s.as_str(), i))).collect();
    let mut replay = Replay::default();
    for &e in examples {
        let m = messages.get(e).ok_or("A template's example isn't one of these texts.")?;
        if sender_key(&m.sender) != t.sender { return Err("A template's examples come from its sender.".into()); }
        if c.read(&m.body).is_none() { return Err(format!("The template doesn't read text {}.", e + 1)); }
    }
    for (i, m) in messages.iter().enumerate() {
        if sender_key(&m.sender) != t.sender { continue; }
        let Some(reading) = c.read(&m.body) else { continue };
        let Some(model) = by_source.get(m.id.as_str()) else { continue };
        if model.source_ids.len() > 1 { continue; }
        let filled = fill(b, &c, m, &reading, &HashMap::new(), None).ok_or_else(|| format!("The template doesn't make a valid item from text {}.", i + 1))?;
        let same = filled.disposition == model.disposition && filled.skip == model.skip && match (&filled.transaction, &model.transaction) {
            (None, None) => true,
            (Some(a), Some(b)) => a.kind == b.kind && same_number(a.amount.as_deref(), b.amount.as_deref()) && a.currency == b.currency
                && same_number(Some(&a.written_amount), Some(&b.written_amount)) && a.date == b.date
                && same_ref(a.account_ref.as_deref(), b.account_ref.as_deref()) && same_ref(a.other_ref.as_deref(), b.other_ref.as_deref()),
            _ => false,
        };
        if !same { return Err(format!("The template reads text {} differently from your item for it.", i + 1)); }
        if let (Some(raw), Some(a), Some(b)) = (&filled.raw, &filled.transaction, &model.transaction) {
            if payee_key(&a.payee) != payee_key(&b.payee) || a.category_id != b.category_id {
                replay.payees.push((raw.clone(), b.payee.clone(), b.category_id.clone()));
            }
        }
        replay.agreed += 1;
    }
    Ok(replay)
}

/// The same money however it's written: "-12.30", "-12.3", "-1,012.30" and "-1012.30" agree.
fn same_number(a: Option<&str>, b: Option<&str>) -> bool {
    fn canonical(n: &str) -> String {
        let n = n.replace(',', "");
        let (sign, n) = n.strip_prefix('-').map_or(("", n.as_str()), |r| ("-", r));
        let (whole, fraction) = n.split_once('.').unwrap_or((n, ""));
        format!("{sign}{}.{}", whole.trim_start_matches('0'), fraction.trim_end_matches('0'))
    }
    match (a, b) { (Some(a), Some(b)) => canonical(a) == canonical(b), (None, None) => true, _ => false }
}
/// The same card or account by its digits: "***0091", "0091" and "Card:0091" agree.
fn same_ref(a: Option<&str>, b: Option<&str>) -> bool {
    let digits = |r: &str| r.chars().filter(char::is_ascii_digit).collect::<String>();
    match (a, b) { (Some(a), Some(b)) => digits(a) == digits(b), (None, None) => true, _ => false }
}

/// A template's id: its sender and lines, so the same format written twice is one template.
pub fn template_id(sender: &str, lines: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{sender}\u{0}{}", lines.join("\n")).as_bytes());
    format!("t{}", digest.iter().take(6).map(|b| format!("{b:02x}")).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Account, AccountKind, Category};

    fn budget() -> Budget {
        let mut b = Budget { currency: "SAR".into(), ..Budget::default() };
        b.accounts.push(Account { extra: Default::default(), id: "main".into(), name: "Main".into(), kind: AccountKind::Cash, opening_balance: 0, archived: false });
        b.categories.push(Category { extra: Default::default(), id: "food".into(), name: "Food".into(), group: "Everyday".into(), target: 0 });
        b
    }
    fn text(id: &str, body: &str) -> Message {
        Message { id: id.into(), sender: "ANB".into(), received_date: "2026-11-01".into(), received_time: "13:05".into(), body: body.into() }
    }
    fn purchase() -> Template {
        Template {
            id: "t1".into(), sender: "anb".into(),
            lines: ["Online Purchase", "?Apple Pay", "VISA Card : {ref}", "Amount : {currency} {amount}", "?Balance : SAR {balance}", "At : {payee}", "On : {date:dd/mm/yy} {time}"]
                .map(String::from).to_vec(),
            outcome: Outcome { disposition: Some("transaction".into()), kind: Some("expense".into()), sign: Some("-".into()), memo: Some("Online".into()), ..Outcome::default() },
            note: "Purchase at {payee}".into(), payee: None,
        }
    }
    const BODY: &str = "Online Purchase\r\nApple Pay\r\nVISA Card:1234\r\nAmount:SAR 1,065.97\r\nBalance:SAR 9,120.40\r\nAt:NOON E\r\nOn:01/11/26 13:05";

    #[test]
    fn an_amount_is_read_however_a_bank_groups_it_and_marks_its_decimals() {
        for (written, plain) in [("1,234.56", "1234.56"), ("1.234,56", "1234.56"), ("12,50", "12.50"), ("12,5", "12.5"), ("1,250", "1250"),
            ("1'234.50", "1234.50"), ("1'234", "1234"), ("1,23,456.78", "123456.78"), ("1.250.000", "1250000"), ("1.250", "1.250"), ("35000", "35000"), ("0.71", "0.71"),
            ("1 249,00", "1249.00"), ("12 345 678", "12345678")] {
            assert_eq!(decimal(written), plain, "{written}");
        }
        let set = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<std::collections::HashSet<_>>();
        assert_eq!(amounts("Amount:SAR 1,250.00 Balance:SAR 9,120.40 Card:4512 On:28/10/26"), set(&["1250", "9120.4"]));
        assert_eq!(amounts("حوالة 12000 ريال وبـ:SAR 23.50 و 7 USD"), set(&["12000", "23.5", "7"]));
        assert_eq!(amounts("You spent £12.50 at TESCO; $45.00, €1.234,56 and 9,99 € and Rs.2,500.00 and ₹ 300 and 1500 kr"), set(&["12.5", "45", "1234.56", "9.99", "2500", "300", "1500"]));
        // Grouped with spaces, but only in threes: "45 1234" is two numbers.
        assert_eq!(amounts("Paiement de 1 249,00 € et 45 1234 EUR"), set(&["1249", "1234"]));
        // Not a currency: a code, a card, a word ending in a code's letters, a mark inside a word.
        assert_eq!(amounts("OTP 482193 card 4512 BASAR 99 FORMS 12 on 03/11/26"), set(&[]));
    }
    #[test]
    fn a_currency_mark_is_the_budgets_when_it_could_be_and_the_usual_one_otherwise() {
        for (mark, budget, code) in [("$", "USD", "USD"), ("$", "CAD", "CAD"), ("$", "EUR", "USD"), ("C$", "USD", "CAD"), ("£", "GBP", "GBP"), ("€", "SAR", "EUR"),
            ("Rs.", "PKR", "PKR"), ("Rs", "USD", "INR"), ("₹", "INR", "INR"), ("¥", "CNY", "CNY"), ("¥", "USD", "JPY"), ("kr", "NOK", "NOK"),
            ("ريال", "QAR", "QAR"), ("ريال", "USD", "SAR"), ("SR", "SAR", "SAR"), ("usd", "SAR", "USD"), ("RMB", "USD", "CNY")] {
            assert_eq!(currency_code(mark, budget), code, "{mark} in {budget}");
        }
    }
    #[test]
    fn a_date_may_name_its_month_or_leave_out_its_year() {
        let at = |v: &str, o: &str, r: &str| date(v, o, r);
        assert_eq!(at("28/10/26", "dd/mm/yy", "2026-10-28").as_deref(), Some("2026-10-28"));
        assert_eq!(at("28-10-2026", "dd/mm/yyyy", "2026-10-28").as_deref(), Some("2026-10-28"));
        assert_eq!(at("29-Oct-26", "dd-mmm-yy", "2026-10-29").as_deref(), Some("2026-10-29"));
        assert_eq!(at("Nov 3, 2026", "mmm dd, yyyy", "2026-11-03").as_deref(), Some("2026-11-03"));
        assert_eq!(at("03 Sept. 2026", "dd mmm yyyy", "2026-09-03").as_deref(), Some("2026-09-03"));
        // No year: the one that puts it on or before the day the text came.
        assert_eq!(at("11/03", "mm/dd", "2026-11-03").as_deref(), Some("2026-11-03"));
        assert_eq!(at("12/31", "mm/dd", "2027-01-01").as_deref(), Some("2026-12-31"));
        assert_eq!(at("02 Jan", "dd mmm", "2027-01-01").as_deref(), Some("2027-01-02"));
        assert_eq!(at("31/02/26", "dd/mm/yy", "2026-03-01"), None);
        assert_eq!(at("Foo 3", "mmm dd", "2026-11-03"), None);
        for order in ["dd/mm", "mmm dd, yyyy", "dd-mmm-yy"] { assert!(slot(&format!("date:{order}")).is_ok(), "{order}"); }
        for order in ["dd/yy", "mm/yy", "dd/mm/mmm", "dd"] { assert!(slot(&format!("date:{order}")).is_err(), "{order}"); }
    }
    #[test]
    fn banks_elsewhere_are_read_in_their_own_formats() {
        let with = |currency: &str| { let mut b = budget(); b.currency = currency.into(); b };
        let template = |sender: &str, lines: &[&str], sign: &str| Template {
            id: template_id(sender, &lines.iter().map(|l| l.to_string()).collect::<Vec<_>>()), sender: sender.into(), lines: lines.iter().map(|l| l.to_string()).collect(),
            outcome: Outcome { disposition: Some("transaction".into()), kind: Some(if sign == "-" { "expense" } else { "income" }.into()), sign: Some(sign.into()), ..Outcome::default() },
            note: "{payee}".into(), payee: None,
        };
        let read = |b: &Budget, t: &Template, sender: &str, received: &str, body: &str| {
            let c = compile(t).unwrap();
            let m = Message { id: "sms:x".into(), sender: sender.into(), received_date: received.into(), received_time: "18:22".into(), body: body.into() };
            let item = fill(b, &c, &m, &c.read(body).unwrap_or_else(|| panic!("reads {body}")), &HashMap::new(), None).unwrap();
            let t = item.transaction.unwrap();
            (t.date, t.amount, t.currency, t.written_amount, t.payee, t.account_ref)
        };
        let s = |v: &str| v.to_string();
        // A US card: a dollar sign, the month first and no year.
        let us = template("chase", &["Chase: You made a {currency}{amount} transaction with {payee} on your card ending {ref} on {date:mm/dd}."], "-");
        assert_eq!(read(&with("USD"), &us, "Chase", "2026-11-03", "Chase: You made a $1,045.00 transaction with AMAZON MKTPLACE on your card ending 9876 on 11/03."),
            (s("2026-11-03"), Some(s("-1045.00")), s("USD"), s("1045.00"), s("AMAZON MKTPLACE"), Some(s("9876"))));
        // A UK bank: the pound sign, no date in the text.
        let uk = template("bank", &["You've spent {currency}{amount} at {payee} with your card ending {ref}."], "-");
        assert_eq!(read(&with("GBP"), &uk, "Bank", "2026-10-28", "You've spent £12.50 at TESCO STORES 3041 with your card ending 4512."),
            (s("2026-10-28"), Some(s("-12.50")), s("GBP"), s("12.50"), s("TESCO STORES 3041"), Some(s("4512"))));
        // An Indian bank: rupees as Rs., a month's name, a masked account.
        let india = template("hdfcbk", &["{currency}{amount} debited from A/c {ref} on {date:dd-mmm-yy} to VPA {payee} (UPI Ref No {n}). Not you? Call {n}"], "-");
        assert_eq!(read(&with("INR"), &india, "HDFCBK", "2026-10-29", "Rs.2,500.00 debited from A/c XX1234 on 29-Oct-26 to VPA swiggy@icici (UPI Ref No 630123456789). Not you? Call 18002586161"),
            (s("2026-10-29"), Some(s("-2500.00")), s("INR"), s("2500.00"), s("swiggy@icici"), Some(s("XX1234"))));
        // A German bank: comma decimals, the currency after, a full date with points.
        let de = template("bank", &["Kartenzahlung {amount} {currency} bei {payee}, Karte {ref}, {date:dd.mm.yyyy}"], "-");
        assert_eq!(read(&with("EUR"), &de, "Bank", "2026-10-31", "Kartenzahlung 1.234,56 EUR bei MEDIAMARKT BERLIN, Karte ...4421, 31.10.2026"),
            (s("2026-10-31"), Some(s("-1234.56")), s("EUR"), s("1234.56"), s("MEDIAMARKT BERLIN"), Some(s("...4421"))));
        // The same German text in a dollar budget: its own currency, the amount left for the user.
        assert_eq!(read(&with("USD"), &de, "Bank", "2026-10-31", "Kartenzahlung 12,50 € bei CAFE, Karte ...4421, 31.10.2026").1, None);
        // A French bank: thousands grouped with a space, a date with no year.
        let fr = template("bank", &["Paiement CB de {amount} {currency} chez {payee} le {date:dd/mm} carte {ref}"], "-");
        assert_eq!(read(&with("EUR"), &fr, "Bank", "2026-11-06", "Paiement CB de 1 249,00 € chez FNAC le 06/11 carte ****4421"),
            (s("2026-11-06"), Some(s("-1249.00")), s("EUR"), s("1249.00"), s("FNAC"), Some(s("****4421"))));
        // The currency given again beside the balance: the amount's is read, whichever comes first.
        let neft = template("bank", &["{currency} {amount} credited to A/c {ref} on {date:dd-mmm-yy} by NEFT from {payee}. Avl Bal {currency} {balance}"], "+");
        assert_eq!(read(&with("INR"), &neft, "Bank", "2026-10-31", "INR 1,23,456.00 credited to A/c XX1234 on 31-Oct-26 by NEFT from ACME TECHNOLOGIES PVT LTD. Avl Bal INR 2,45,000.50"),
            (s("2026-10-31"), Some(s("123456.00")), s("INR"), s("123456.00"), s("ACME TECHNOLOGIES PVT LTD"), Some(s("XX1234"))));
        let balance_first = template("bank", &["Bal {currency} {balance}. Spent {currency} {amount} at {payee} card {ref}"], "-");
        assert_eq!(read(&with("INR"), &balance_first, "Bank", "2026-10-31", "Bal INR 9,000.00. Spent USD 45.00 at AMAZON card 1234").2, s("USD"));
        // A time with AM or PM.
        let pm = template("bank", &["Card {ref} spent {currency} {amount} at {payee} on {date:dd/mm/yyyy} {time}"], "-");
        assert_eq!(read(&with("AED"), &pm, "Bank", "2026-11-02", "Card 1234 spent AED 85.00 at CARREFOUR on 02/11/2026 7:45 PM").1, Some(s("-85.00")));
    }

    #[test]
    fn a_template_reads_its_format_whatever_the_spacing_and_skips_its_optional_lines() {
        let c = compile(&purchase()).unwrap();
        let r = c.read(BODY).unwrap();
        assert_eq!(r.amount.as_deref(), Some("1,065.97")); assert_eq!(r.currency.as_deref(), Some("SAR"));
        assert_eq!(r.reference.as_deref(), Some("1234")); assert_eq!(r.payee.as_deref(), Some("NOON E"));
        assert_eq!(r.date, Some(("01/11/26".into(), "dd/mm/yy".into())));
        // Without Apple Pay and the balance, and spaced differently.
        assert!(c.read("Online Purchase\nVISA Card : 1234\nAmount : SAR 5\nAt : SHOP\nOn : 02/11/26 07:47").is_some());
        // A different format, or an extra line, isn't this one.
        assert!(c.read("PoS Purchase\nVISA Card:1234\nAmount:SAR 5\nAt:SHOP\nOn:02/11/26 07:47").is_none());
        assert!(c.read(&format!("{BODY}\nThank you")).is_none());
    }
    #[test]
    fn a_reading_becomes_the_item_the_assistant_would_write() {
        let b = budget();
        let c = compile(&purchase()).unwrap();
        let m = text("sms:1", BODY);
        let payees = payee_map(&[Payee { sender: "anb".into(), raw: "noon  e".into(), payee: "Noon".into(), category_id: Some("food".into()) }]);
        let item = fill(&b, &c, &m, &c.read(BODY).unwrap(), &payees, None).unwrap();
        let t = item.transaction.as_ref().unwrap();
        assert_eq!((t.date.as_str(), t.amount.as_deref(), t.written_amount.as_str(), t.payee.as_str()), ("2026-11-01", Some("-1065.97"), "1065.97", "Noon"));
        assert_eq!((t.account_ref.as_deref(), t.category_id.as_deref(), t.memo.as_str()), (Some("1234"), Some("food"), "Online"));
        assert_eq!(item.note, "Purchase at Noon"); assert_eq!(item.evidence[0].quote, "1,065.97");
        assert_eq!(item.template.as_deref(), Some("t1")); assert_eq!(item.raw.as_deref(), Some("NOON E"));
        // Without a learned payee the bank's own words stand.
        assert_eq!(fill(&b, &c, &m, &c.read(BODY).unwrap(), &HashMap::new(), None).unwrap().transaction.unwrap().payee, "NOON E");
    }
    #[test]
    fn a_banks_name_for_the_users_payee_is_read_as_theirs_with_its_category() {
        let mut b = budget();
        let mut bought = crate::test_support::purchase();
        bought.payee = "Noon".into();
        b.transactions.push(bought);
        let read = lanes(&b, &[text("sms:1", BODY)], &[purchase()], &[], &Default::default());
        let item = &read.review[0].1;
        let t = item.transaction.as_ref().unwrap();
        assert_eq!((t.payee.as_str(), t.category_id.as_deref(), item.note.as_str()), ("Noon", Some("food"), "Purchase at Noon"));
        assert_eq!(item.raw.as_deref(), Some("NOON E"));
        // What the user settled on for the bank's name comes first.
        let learned = [Payee { sender: "anb".into(), raw: "NOON E".into(), payee: "Noon Minutes".into(), category_id: None }];
        let read = lanes(&b, &[text("sms:1", BODY)], &[purchase()], &learned, &Default::default());
        let t = read.review[0].1.transaction.as_ref().unwrap();
        assert_eq!((t.payee.as_str(), t.category_id.as_deref()), ("Noon Minutes", None));
    }
    #[test]
    fn a_foreign_amount_waits_unless_the_format_gives_the_total_and_dates_follow_their_order() {
        let b = budget();
        let mut t = purchase();
        t.lines = ["International Purchase", "Amount : {currency} {amount}", "?Total amount : SAR {total}", "At : {payee}", "On : {date:yy-mm-dd}"].map(String::from).to_vec();
        let c = compile(&t).unwrap();
        let waits = text("sms:2", "International Purchase\nAmount: USD 42.00\nAt: CAFE\nOn: 26-10-31");
        let item = fill(&b, &c, &waits, &c.read(&waits.body).unwrap(), &HashMap::new(), None).unwrap();
        let tx = item.transaction.unwrap();
        assert_eq!((tx.amount, tx.currency.as_str(), tx.date.as_str()), (None, "USD", "2026-10-31"));
        let total = text("sms:3", "International Purchase\nAmount: USD 42.00\nTotal amount: SAR 160.50\nAt: CAFE\nOn: 26-10-31");
        assert_eq!(fill(&b, &c, &total, &c.read(&total.body).unwrap(), &HashMap::new(), None).unwrap().transaction.unwrap().amount.as_deref(), Some("-160.50"));
    }
    #[test]
    fn a_template_that_could_hang_match_everything_or_mean_nothing_is_refused() {
        let mut t = purchase();
        t.lines = vec!["{payee}".into()];
        assert!(compile(&t).is_err());
        t.lines = vec!["At : {payee}{amount}".into()];
        assert!(compile(&t).is_err());
        t.lines = vec!["Amount : {amount} on {date:mm/yy}".into()];
        assert!(compile(&t).is_err());
        t = purchase(); t.outcome.sign = None;
        assert!(compile(&t).is_err());
        t = purchase(); t.outcome = Outcome { skip: Some("code".into()), ..Outcome::default() };
        assert!(compile(&t).is_ok());
    }
    #[test]
    fn texts_go_to_review_when_a_template_reads_them_and_to_the_assistant_otherwise() {
        let b = budget();
        let code = Template { id: "t2".into(), sender: "anb".into(), lines: vec!["رمز : {code}".into(), "لا تشارك الرمز".into()],
            outcome: Outcome { skip: Some("code".into()), ..Outcome::default() }, note: String::new(), payee: None };
        let messages = [text("sms:1", BODY), text("sms:2", "رمز : 4821\nلا تشارك الرمز"), text("sms:3", "Something new SAR 5")];
        let lanes = lanes(&b, &messages, &[purchase(), code], &[], &Default::default());
        assert_eq!(lanes.review.iter().map(|(i, it)| (*i, it.disposition.as_str())).collect::<Vec<_>>(), vec![(0, "transaction"), (1, "ignore")]);
        assert_eq!(lanes.read, vec![2]);
        // Another sender's templates don't read this sender's texts.
        let other = Message { sender: "OtherBank".into(), ..messages[0].clone() };
        assert_eq!(super::lanes(&b, &[other], &[purchase()], &[], &Default::default()).read, vec![0]);
    }
    #[test]
    fn junk_a_trusted_skip_template_reads_and_codes_stay_on_the_phone() {
        let b = budget();
        let miles = Template { id: "t3".into(), sender: "anb".into(), lines: vec!["Earned miles : {n}".into()],
            outcome: Outcome { skip: Some("rewards".into()), ..Outcome::default() }, note: String::new(), payee: None };
        let texts: Vec<Message> = (0..200).map(|i| text(&format!("sms:{i}"), &format!("Earned miles : {i}"))).collect();
        // Not trusted yet: every text is reviewed.
        assert_eq!(lanes(&b, &texts, &[miles.clone()], &[], &Default::default()).review.len(), 200);
        let trusted = std::collections::HashSet::from(["t3".to_string()]);
        let l = lanes(&b, &texts, &[miles], &[], &trusted);
        // Trusted: settled on the phone, but about one in fifty still goes to review as an audit.
        assert!(l.junk.len() > 180 && !l.review.is_empty() && l.junk.len() + l.review.len() == 200, "{} {}", l.junk.len(), l.review.len());
        // What stays on the phone says so; what's audited is reviewed like any other reading.
        assert!(l.junk.iter().all(|(_, item)| item.kept) && l.review.iter().all(|(_, item)| !item.kept));
        // A code no template knows yet is recognised by its words and its number; money never is.
        let codes = [text("c1", "لـ : شراء
لا تشارك الرمز
رمز : 482193
مبلغ : SAR 120"), text("c2", "Your OTP is 4821 for SAR 120 at AMAZON"),
            text("c3", "Purchase SAR 120 at AMAZON card 4821"), text("c4", "Purchase SAR 50.00 at SHOP card 4512. Never share your OTP."),
            text("c5", "رمز المرور صالح للاستخدام لمرة واحدة: 739201")];
        let l = lanes(&b, &codes, &[], &[], &Default::default());
        assert_eq!((l.junk.iter().map(|(i, _)| *i).collect::<Vec<_>>(), l.read.clone()), (vec![0, 1, 4], vec![2, 3]));
        assert_eq!(l.of(5), vec!["junk", "junk", "read", "read", "junk"]);
        // In other languages too, and still never a purchase that mentions one.
        for code in ["Tu código de verificación es 482193. No lo compartas.", "Votre code de vérification : 739201", "Ihr Bestätigungscode: 4821",
            "482193 is your one-time PIN for a purchase of $45.00 at BESTBUY", "Doğrulama kodu: 739201"] {
            assert!(looks_like_code(code), "{code}");
        }
        for money in ["Compra de 45,00 € en MERCADONA con tarjeta 4512. Nunca compartas tu código.", "You spent £12.50 at TESCO with card 4512. We'll never ask for your passcode."] {
            assert!(!looks_like_code(money), "{money}");
        }
    }
    #[test]
    fn a_template_is_kept_only_when_it_reads_the_batch_as_the_assistant_did() {
        let b = budget();
        let messages = [text("sms:1", BODY), text("sms:2", "Online Purchase\nVISA Card:1234\nAmount:SAR 8.00\nAt:CAFE\nOn:01/11/26 09:00")];
        let c = compile(&purchase()).unwrap();
        let items: Vec<Item> = messages.iter().map(|m| fill(&b, &c, m, &c.read(&m.body).unwrap(), &HashMap::new(), None).unwrap()).collect();
        let mut model = items.clone();
        model[0].transaction.as_mut().unwrap().payee = "Noon".into();
        let kept = replay(&b, &purchase(), &messages, &model, &[0]).unwrap();
        assert_eq!(kept.agreed, 2);
        assert_eq!(kept.payees, vec![("NOON E".into(), "Noon".into(), None)]);
        // The assistant read one of them as income: the template is wrong for this batch.
        model[1].transaction.as_mut().unwrap().kind = "income".into();
        model[1].transaction.as_mut().unwrap().amount = Some("8.00".into());
        assert!(replay(&b, &purchase(), &messages, &model, &[0]).is_err());
        // A template must read its own example.
        assert!(replay(&b, &purchase(), &[text("sms:9", "Hello")], &[], &[0]).is_err());
    }
    #[test]
    fn the_same_format_written_twice_is_one_template() {
        let lines = purchase().lines;
        assert_eq!(template_id("anb", &lines), template_id("anb", &lines));
        assert_ne!(template_id("anb", &lines), template_id("other", &lines));
        assert_eq!(sender_key("ANB"), "anb"); assert_eq!(sender_key("+966 (50) 123-4567"), "+966501234567");
    }
}
