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

const DATE_PARTS: [&str; 4] = ["dd", "mm", "yy", "yyyy"];

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
        let parts: Vec<&str> = order.split(['/', '-', '.', ' ']).collect();
        let mut sorted: Vec<&str> = parts.iter().map(|p| if *p == "yyyy" { "yy" } else { p }).collect();
        sorted.sort();
        if parts.len() != 3 || !parts.iter().all(|p| DATE_PARTS.contains(p)) || sorted != ["dd", "mm", "yy"] {
            return Err(format!("Write a date's order with dd, mm and yy or yyyy, not {order}."));
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
    for s in [Slot::Amount, Slot::Total, Slot::Currency, Slot::Date, Slot::Ref, Slot::Other, Slot::Payee] {
        if slots.iter().filter(|x| **x == s).count() > 1 {
            return Err("Use each of amount, total, currency, date, ref, other and payee once.".into());
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
fn number(s: &[char], at: usize, grouping: bool) -> Option<usize> {
    let mut i = at;
    while i < s.len() && (s[i].is_ascii_digit() || (grouping && s[i] == ',' && i > at && i + 1 < s.len() && s[i + 1].is_ascii_digit())) { i += 1 }
    if i == at { return None; }
    if grouping && i + 1 < s.len() && s[i] == '.' && s[i + 1].is_ascii_digit() {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() { i += 1 }
    }
    Some(i)
}
const CURRENCY_WORDS: [&str; 4] = ["ريال", "ر.س", "رس", "SR"];
fn typed(s: &[char], at: usize, kind: Slot) -> Option<usize> {
    let i = at;
    match kind {
        Slot::Amount | Slot::Total | Slot::Balance => number(s, i, true),
        Slot::Code | Slot::Number => number(s, i, false),
        Slot::Currency => {
            for word in CURRENCY_WORDS {
                let w: Vec<char> = word.chars().collect();
                if s.len() >= i + w.len() && s[i..i + w.len()] == w[..] { return Some(i + w.len()); }
            }
            (s.len() >= i + 3 && s[i..i + 3].iter().all(|c| c.is_ascii_alphabetic()) && s.get(i + 3).is_none_or(|c| !c.is_ascii_alphabetic())).then_some(i + 3)
        }
        Slot::Date => {
            let mut j = i;
            while j < s.len() && (s[j].is_ascii_digit() || "/-.".contains(s[j])) { j += 1 }
            (j - i >= 6).then_some(j)
        }
        Slot::Time => {
            let mut j = i;
            while j < s.len() && (s[j].is_ascii_digit() || s[j] == ':') { j += 1 }
            let t: String = s[i..j].iter().collect();
            (t.contains(':') && t.split(':').all(|p| (1..=2).contains(&p.len()))).then_some(j)
        }
        Slot::Ref | Slot::Other => {
            let mut j = i;
            while j < s.len() && (s[j].is_ascii_digit() || "*xX#".contains(s[j])) { j += 1 }
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
                let end = typed(s, at, *kind)?;
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
        for line in &self.lines {
            match text.get(at).and_then(|s| match_line(s, &line.tokens)) {
                Some(found) => {
                    for (kind, arg, value) in found {
                        match kind {
                            Slot::Amount => reading.amount = Some(value),
                            Slot::Total => reading.total = Some(value),
                            Slot::Balance => reading.balance = Some(value),
                            Slot::Currency => reading.currency = Some(value),
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

/// A date written in `order` ("dd/mm/yy"), as YYYY-MM-DD.
fn date(value: &str, order: &str) -> Option<String> {
    let parts: Vec<&str> = value.split(['/', '-', '.']).collect();
    let names: Vec<&str> = order.split(['/', '-', '.', ' ']).collect();
    if parts.len() != 3 || names.len() != 3 { return None; }
    let (mut d, mut m, mut y) = (0u32, 0u32, 0i32);
    for (p, n) in parts.iter().zip(&names) {
        let v: i32 = p.parse().ok()?;
        match *n { "dd" => d = v as u32, "mm" => m = v as u32, "yy" if p.len() == 2 => y = 2000 + v, "yyyy" if p.len() == 4 => y = v, _ => return None }
    }
    chrono::NaiveDate::from_ymd_opt(y, m, d).map(|day| day.format("%Y-%m-%d").to_string())
}
fn currency_code(written: &str) -> String {
    match written { "ريال" | "ر.س" | "رس" | "SR" => "SAR".into(), w => w.to_uppercase() }
}
fn plain(number: &str) -> String { number.replace(',', "") }

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
        let signed = |n: &str| format!("{}{}", if negative { "-" } else { "" }, plain(n));
        let written = reading.amount.as_ref().or(reading.total.as_ref())?;
        let currency = reading.currency.as_deref().map(currency_code).unwrap_or_else(|| b.currency.clone());
        let amount = if currency == b.currency { Some(signed(written)) } else { reading.total.as_deref().map(signed) };
        // The day the text was written for, or the day it came when the format gives none.
        let day = match &reading.date { Some((value, order)) => date(value, order)?, None => m.received_date.clone() };
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
                date: day, amount, currency: if reading.amount.is_some() { currency } else { b.currency.clone() }, written_amount: plain(written), payee,
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
const CODE_PHRASES: [&str; 12] = ["otp", "one-time password", "one time password", "verification code", "do not share", "don't share",
    "لا تشارك الرمز", "رمز التحقق", "رمز المرور", "كلمة المرور لمرة واحدة", "كلمة مرور صالحة لمرة واحدة", "رمز التفعيل"];
/// Words a code follows ("OTP: 4821", "رمز : 482193").
const CODE_WORDS: [&str; 6] = ["otp", "code", "password", "pin", "رمز", "كلمة المرور"];

/// A one-time code before any template knows the format: a code phrase, and a 4–8 digit number
/// shortly after a code word, so a purchase that ends "never share your OTP" isn't one.
/// Conservative on purpose: a miss only means the assistant reads the text.
pub fn looks_like_code(body: &str) -> bool {
    let lower: Vec<char> = body.to_lowercase().chars().collect();
    let text: String = lower.iter().collect();
    let words: Vec<String> = text.split(|c: char| !c.is_alphanumeric()).map(str::to_owned).collect();
    let phrase = CODE_PHRASES.iter().any(|p| if p.is_ascii() && !p.contains(' ') { words.iter().any(|w| w == p) } else { text.contains(p) });
    if !phrase { return false; }
    // Where each code word ends, in characters.
    let mut ends = vec![];
    for word in CODE_WORDS {
        let w: Vec<char> = word.chars().collect();
        for i in 0..(lower.len() + 1).saturating_sub(w.len()) {
            if lower[i..i + w.len()] == w[..] { ends.push(i + w.len()) }
        }
    }
    let mut i = 0;
    while i < lower.len() {
        if lower[i].is_ascii_digit() {
            let start = i;
            while i < lower.len() && lower[i].is_ascii_digit() { i += 1 }
            if (4..=8).contains(&(i - start)) && ends.iter().any(|&e| e <= start && start - e <= 60) { return true; }
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
