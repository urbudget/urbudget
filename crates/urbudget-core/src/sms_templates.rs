//! Bank-text templates: a format the assistant wrote while reading, read on the phone next time.
//! A template is lines of literal words and typed slots, compiled here rather than handed to a
//! regular-expression engine, so a template can neither hang nor match everything. A text a
//! template reads still goes to the assistant, which reviews the reading, until the format has
//! earned the budget's trust: then junk, and money whose payee the user has already named, are
//! read on the phone alone, with some still sent as a check.
use crate::{
    domain::Budget,
    sms_import::{self, Analysis, Evidence, Item, Message, Transaction},
    workflows::{bank_key, payee_rule, BankNames, KnownPayees},
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

/// A payee as the bank writes it, to the payee and category settled on for it: by the user in the
/// review ([mine]), or by the assistant's reading.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Payee {
    pub sender: String,
    pub raw: String,
    pub payee: String,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mine: bool,
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

/// What the user calls the payee a bank names, from what they've said: a payee rule whose text the
/// bank's name holds; else what they called the name before (the budget's transactions that keep
/// the bank's words, renamed in the review or the register since); else the name this sender's
/// texts were learned under on the phone (a review the user changed, or the assistant's reading);
/// else their payee the name surely is ("MASHAR E" is their "Mashar"). The category comes from
/// the same place, else the one they usually give that payee. Where they file a name under more
/// than one category, the transaction's amount decides (`workflows::category_for`): the price
/// that's always their subscription gets its category, and what's bought there another.
pub struct Names<'a> {
    b: &'a Budget,
    history: BankNames,
    learned: HashMap<(String, String), Payee>,
    loose: HashMap<(String, String), Payee>,
    known: KnownPayees,
}
/// A payee and category for a bank's name, and whether the user said so (a rule, their history, a
/// name learned on the phone) rather than it being found among their payees.
#[derive(Debug, PartialEq)]
pub struct Named {
    pub payee: String,
    pub category_id: Option<String>,
    pub said: bool,
    /// The category is worth a look: the user files this payee under more than one, and neither
    /// this amount nor their latest choices settle which.
    pub check_category: bool,
}
impl<'a> Names<'a> {
    pub fn new(b: &'a Budget, learned: &[Payee]) -> Self {
        let loose = learned.iter().map(|p| ((p.sender.clone(), bank_key(&p.raw)), p.clone())).collect();
        Self { b, history: BankNames::new(b), learned: payee_map(learned), loose, known: KnownPayees::new(b) }
    }
    /// The user's payee and category for [raw], the name a text from [sender] (its key) gives, in
    /// a transaction of [amount] (cents, either sign) when it's known.
    pub fn of(&self, sender: &str, raw: &str, amount: Option<i64>) -> Option<Named> {
        if raw.trim().is_empty() {
            return None;
        }
        let rule = payee_rule(self.b, raw);
        let rule_name = rule.and_then(|r| r["payee"].as_str()).map(str::trim).filter(|n| !n.is_empty());
        let rule_category = rule.and_then(|r| r["categoryId"].as_str()).filter(|c| !c.is_empty());
        let learned = self.learned.get(&(sender.to_string(), payee_key(raw))).or_else(|| self.loose.get(&(sender.to_string(), bank_key(raw))));
        let (payee, category, said) = if let Some(name) = rule_name {
            (name.to_string(), None, true)
        } else if let Some((payee, _)) = self.history.get(raw) {
            (payee.to_string(), None, true)
        } else if let Some(p) = learned {
            (p.payee.clone(), p.category_id.clone(), p.mine)
        } else if let Some(p) = self.known.same(raw) {
            (p.name.clone(), None, false)
        } else if rule_category.is_some() {
            // A rule that only sets the category keeps the bank's name.
            (raw.trim().to_string(), None, true)
        } else {
            return None;
        };
        // A rule's category first; then what the user's transactions under the bank's name say for
        // this amount, what the phone learned for the name, and their payee's own transactions.
        let (category, check) = match (rule_category, self.history.category(raw, amount), category) {
            (Some(rule), ..) => (Some(rule.to_string()), false),
            (None, Some((id, settled)), _) => (Some(id), !settled),
            (None, None, Some(learned)) => (Some(learned), false),
            (None, None, None) => match self.known.named(&payee).and_then(|p| crate::workflows::category_for(&p.filed, amount)) {
                Some((id, settled)) => (Some(id), !settled),
                None => (None, false),
            },
        };
        Some(Named { payee, category_id: category, said, check_category: check })
    }
    /// The category if it's one an item can have: an existing spending category, never for a transfer.
    fn spending(&self, kind: &str, category: Option<String>) -> Option<String> {
        category.filter(|id| kind != "transfer" && self.b.categories.iter().any(|c| c.id == *id && c.payment_account().is_none()))
    }
}

/// Gives the money items whose bank's name is known the user's payee for it, and its category
/// when they have one for it: the user's names come before any reading, the phone's or the
/// assistant's, so one name is always read the same way. A transfer gets no category.
pub fn own_names(names: &Names, messages: &[Message], items: &mut [Item]) {
    let senders: HashMap<&str, &str> = messages.iter().map(|m| (m.id.as_str(), m.sender.as_str())).collect();
    for item in items.iter_mut() {
        let Some(sender) = item.source_ids.first().and_then(|id| senders.get(id.as_str())) else { continue };
        let Some(t) = item.transaction.as_mut() else { continue };
        // The bank's words, else the reading's name for them when none were copied.
        let raw = item.raw.clone().unwrap_or_else(|| t.payee.clone());
        let amount = t.amount.as_deref().and_then(|a| sms_import::cents(a).ok());
        let Some(named) = names.of(&sender_key(sender), &raw, amount) else { continue };
        if named.payee != t.payee {
            // The note names the payee as the reading had it ("Purchase at SALON A").
            if t.payee.chars().count() >= 3 && item.note.contains(t.payee.as_str()) { item.note = item.note.replace(t.payee.as_str(), &named.payee); }
            t.payee = named.payee;
        }
        // A category the reading chose stands unless the user said what this name is, and which
        // of their categories this amount is.
        let settled = named.said && !named.check_category;
        if let Some(category) = names.spending(&t.kind, named.category_id).filter(|_| settled || t.category_id.is_none()) { t.category_id = Some(category); }
        // The user said what this name is: nothing about the payee to check, nor its category when that's settled.
        if named.said { item.unsure.retain(|f| f != "payee" && (f != "category" || t.category_id.is_none() || named.check_category)); }
        if named.check_category && t.category_id.is_some() && t.kind != "transfer" && !item.unsure.iter().any(|f| f == "category") { item.unsure.push("category".into()); }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot { Amount, Total, Balance, Available, Currency, Date, Time, Ref, Other, Payee, Code, Number, Any }
#[derive(Clone, Debug)]
/// A template line's parts: words, a slot, or the space between free text and the slot after it.
enum Token { Literal(Vec<char>), Slot(Slot, Option<String>), Gap }
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
        "amount" => Slot::Amount, "total" => Slot::Total, "balance" => Slot::Balance, "available" => Slot::Available, "currency" => Slot::Currency,
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
    // A space alone between free text and a slot marks where one ends ("{payee} {amount}"); elsewhere it's nothing.
    let mut kept: Vec<Token> = vec![];
    let count = tokens.len();
    for (n, token) in tokens.into_iter().enumerate() {
        let blank = matches!(&token, Token::Literal(l) if l.iter().all(|c| c.is_whitespace()));
        if !blank { kept.push(token); continue; }
        let free = matches!(kept.last(), Some(Token::Slot(Slot::Payee | Slot::Any, _)));
        if free && n + 1 < count { kept.push(Token::Gap) }
    }
    let mut tokens: Vec<Token> = kept;
    if matches!(tokens.last(), Some(Token::Gap)) { tokens.pop(); }
    if tokens.is_empty() {
        return Err("A template line needs words or a slot.".into());
    }
    // Free text runs to the next words, or to a space before the next slot; it can't touch a slot.
    for pair in tokens.windows(2) {
        if let [Token::Slot(Slot::Payee | Slot::Any, _), Token::Slot(..)] | [Token::Gap, Token::Gap | Token::Literal(_)] = pair {
            return Err("Put words, or a space, between {payee} or {any} and the next slot.".into());
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
        Token::Slot(..) | Token::Gap => 0,
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
        // Direction marks, and the zero-width space, joiner and byte-order mark some gateways leave in.
        let kept: String = line.chars().filter(|c| !matches!(c, '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}' | '\u{2060}' | '\u{FEFF}')).collect();
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
/// A decimal digit's value in any script banks write in: 0–9, Arabic-Indic ٠–٩, Persian ۰–۹,
/// Devanagari ०–९, Bengali ০–৯, Thai ๐–๙ and full-width ０–９.
pub fn digit(c: char) -> Option<u32> {
    let zero = match c { '0'..='9' => '0', '٠'..='٩' => '٠', '۰'..='۹' => '۰', '०'..='९' => '०', '০'..='৯' => '০', '๐'..='๙' => '๐', '０'..='９' => '０', _ => return None };
    Some(c as u32 - zero as u32)
}
fn is_digit(c: &char) -> bool { digit(*c).is_some() }
/// A number's digits as 0–9, whatever script it's written in.
fn ascii_digits(chars: &[char]) -> String {
    chars.iter().filter_map(|c| digit(*c)).map(|d| char::from_digit(d, 10).unwrap()).collect()
}
/// Separators a bank groups an amount's digits with, or puts before its decimals: "1,234.56",
/// "1.234,56", "1'234.50", the Arabic "١٢٠٫٥٠" and "٥٬٠٠٠", full-width "１，５００".
const SEPARATORS: [char; 6] = [',', '.', '\'', '٫', '٬', '，'];
/// Where a number starting at `at` ends: digits, and with `grouping` the separators between them,
/// a space among them only between groups of three ("1 249,00" as French banks write it).
fn number(s: &[char], at: usize, grouping: bool) -> Option<usize> {
    let mut i = at;
    let mut group = 0;
    while i < s.len() {
        let three = || (1..=3).all(|k| s.get(i + k).is_some_and(is_digit)) && s.get(i + 4).is_none_or(|c| !is_digit(c));
        if is_digit(&s[i]) {
            group += 1;
        } else if grouping && i > at && SEPARATORS.contains(&s[i]) && s.get(i + 1).is_some_and(is_digit) {
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

/// Currencies with three decimals, where "1.250" is one and a quarter.
const THREE_PLACES: [&str; 7] = ["KWD", "BHD", "OMR", "JOD", "TND", "LYD", "IQD"];
/// Of those, the ones whose banks write the decimals after a comma, as French does: "45,500 TND" is
/// forty-five and a half. Elsewhere a comma before three digits groups ("KWD 45,500").
const THREE_PLACES_COMMA: [&str; 1] = ["TND"];
/// An amount as a bank writes it in [currency], as a plain decimal with a point and digits 0–9: the
/// separator before the last one or two digits is the decimal point and the others group
/// ("1.234,56" and "1,234.56" are "1234.56", "12,50" is "12.50", "١٢٠٫٥٠" is "120.50"). Before
/// three digits a separator groups ("Rp 150.000" is 150000, "1.250 EUR" is 1250), except a point in
/// a currency with three decimals ("KWD 1.250" is 1.250), or a comma in one that writes them so
/// ("45,500 TND" is 45.500).
pub fn decimal(number: &str, currency: &str) -> String {
    let chars: Vec<char> = number.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(last) = chars.iter().rposition(|c| SEPARATORS.contains(c)) else { return ascii_digits(&chars) };
    let after = chars.len() - last - 1;
    let sep = chars[last];
    let point = sep == '٫' || (!matches!(sep, '\'' | '٬') && match after {
        1 | 2 => true,
        3 => (sep == '.' && THREE_PLACES.contains(&currency) && chars[..last].iter().all(|c| *c != '.'))
            || (sep == ',' && THREE_PLACES_COMMA.contains(&currency) && chars[..last].iter().all(|c| !SEPARATORS.contains(c))),
        _ => false,
    });
    if point { format!("{}.{}", ascii_digits(&chars[..last]), ascii_digits(&chars[last + 1..])) } else { ascii_digits(&chars) }
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
    ("ج.م", &["EGP"]), ("جنيه", &["EGP", "SDG", "SSP"]), ("د.م.", &["MAD"]), ("円", &["JPY"]), ("원", &["KRW"]), ("人民币", &["CNY"]),
    ("บาท", &["THB"]), ("đ", &["VND"]), ("Ksh", &["KES"]), ("GH₵", &["GHS"]), ("₵", &["GHS"]), ("₡", &["CRC"]), ("₼", &["AZN"]), ("₾", &["GEL"]),
    ("₲", &["PYG"]), ("лв", &["BGN"]), ("S/", &["PEN"]),
    // Last: a single letter, after the marks it begins.
    ("R", &["ZAR"]),
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
        if !is_digit(&chars[i]) || (i > 0 && is_digit(&chars[i - 1])) {
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
        let n = decimal(&chars[s..e].iter().collect::<String>(), "");
        if n.contains('.') { n.trim_end_matches('0').trim_end_matches('.').to_owned() } else { n }
    }).collect()
}

/// A date's parts in the order a template writes it ("dd/mm/yy", "mmm dd, yyyy", "dd-mmm", "ddmmm"):
/// each run of one letter.
fn date_parts(order: &str) -> Vec<&str> {
    let mut parts = vec![];
    let mut start = None;
    for (i, c) in order.char_indices() {
        if let Some(s) = start {
            if order[s..].starts_with(c) { continue; }
            parts.push(&order[s..i]);
            start = None;
        }
        if c.is_ascii_alphabetic() { start = Some(i) }
    }
    if let Some(s) = start { parts.push(&order[s..]) }
    parts
}
/// Each month's names in the languages banks write dates in: English, Spanish, French, German,
/// Italian, Portuguese, Turkish, Indonesian and Arabic, with and without their accents.
const MONTHS: [&[&str]; 12] = [
    &["january", "enero", "janvier", "januar", "gennaio", "janeiro", "ocak", "يناير"],
    &["february", "febrero", "février", "fevrier", "februar", "febbraio", "fevereiro", "şubat", "subat", "فبراير"],
    &["march", "marzo", "mars", "märz", "marz", "mrz", "março", "marco", "mart", "maret", "مارس"],
    &["april", "abril", "avril", "aprile", "nisan", "أبريل", "ابريل"],
    &["may", "mayo", "mai", "maggio", "maio", "mayıs", "mayis", "mei", "مايو"],
    &["june", "junio", "juin", "juni", "giugno", "junho", "haziran", "يونيو"],
    &["july", "julio", "juillet", "juli", "luglio", "julho", "temmuz", "يوليو"],
    &["august", "agosto", "août", "aout", "ağustos", "agustos", "agustus", "أغسطس", "اغسطس"],
    &["september", "septiembre", "setiembre", "septembre", "settembre", "setembro", "eylül", "eylul", "سبتمبر"],
    &["october", "octubre", "octobre", "oktober", "ottobre", "outubro", "ekim", "أكتوبر", "اكتوبر"],
    &["november", "noviembre", "novembre", "novembro", "kasım", "kasim", "نوفمبر"],
    &["december", "diciembre", "décembre", "decembre", "dezember", "dicembre", "dezembro", "aralık", "aralik", "desember", "ديسمبر"],
];
/// A month's name or its first three letters or more ("Nov", "Sept", "October", "ago", "déc"),
/// from 1. Letters that begin two months' names ("jui": juin, juillet) name neither.
fn month(name: &str) -> Option<u32> {
    let name = name.to_lowercase();
    (name.chars().count() >= 3).then_some(())?;
    let mut found = MONTHS.iter().enumerate().filter(|(_, names)| names.iter().any(|m| m.starts_with(&name))).map(|(i, _)| i as u32 + 1);
    let first = found.next()?;
    found.next().is_none().then_some(first)
}
/// A date written in `order` from `at`: where it ends, and its day, month and year if it gives one.
/// Between numbers any of / - . stands for the order's, and so do the characters some scripts write
/// (11月02日); spaces and a comma are optional. What the order ends with ("日") is read too.
fn read_date(s: &[char], at: usize, order: &str) -> Option<(usize, u32, u32, Option<i32>)> {
    let (mut d, mut m, mut y) = (None, None, None);
    let mut i = at;
    let mut first = true;
    // Between parts: up to three characters that are neither digits nor Latin letters, nor before a
    // month's name any letters at all ("02 نوفمبر").
    let between = |mut i: usize, name: bool| {
        let start = i;
        while i < s.len() && i - start < 3 && !is_digit(&s[i]) && !s[i].is_ascii_alphabetic() && !(name && s[i].is_alphabetic()) { i += 1 }
        i
    };
    for part in date_parts(order) {
        if !first { i = between(i, part == "mmm") }
        first = false;
        let start = i;
        if part == "mmm" {
            while i < s.len() && s[i].is_alphabetic() { i += 1 }
            m = Some(month(&s[start..i].iter().collect::<String>())?);
            // "Nov." as some write it.
            if i < s.len() && s[i] == '.' && s.get(i + 1).is_none_or(|c| !c.is_ascii_digit()) { i += 1 }
            continue;
        }
        while i < s.len() && is_digit(&s[i]) && i - start < 4 { i += 1 }
        let digits = ascii_digits(&s[start..i]);
        let value: i32 = digits.parse().ok()?;
        match (part, digits.len()) {
            ("dd", 1 | 2) => d = Some(value as u32),
            ("mm", 1 | 2) => m = Some(value as u32),
            ("yy", 2) => y = Some(2000 + value),
            ("yyyy", 4) => y = Some(value),
            _ => return None,
        }
    }
    let tail: Vec<char> = order.chars().rev().take_while(|c| !c.is_ascii_alphabetic()).collect::<Vec<_>>().into_iter().rev().filter(|c| !c.is_whitespace()).collect();
    if !tail.is_empty() && s.get(i..i + tail.len()).is_some_and(|t| t == &tail[..]) { i += tail.len() }
    Some((i, d?, m?, y))
}
fn typed(s: &[char], at: usize, kind: Slot, arg: Option<&str>) -> Option<usize> {
    let i = at;
    match kind {
        Slot::Amount | Slot::Total | Slot::Balance | Slot::Available => number(s, i, true),
        // Any other number may be money too ("Transaction cost, Ksh7.00"); a code is only digits.
        Slot::Number => number(s, i, true),
        Slot::Code => number(s, i, false),
        Slot::Currency => currency_mark(s, i),
        Slot::Date => read_date(s, i, arg?).map(|(end, ..)| end),
        Slot::Time => {
            let mut j = i;
            while j < s.len() && (is_digit(&s[j]) || s[j] == ':') { j += 1 }
            let t: String = s[i..j].iter().collect();
            // Counted in characters: digits in another script are more than a byte each.
            if !(t.contains(':') && t.split(':').all(|p| (1..=2).contains(&p.chars().count()))) { return None; }
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
            while j < s.len() && (is_digit(&s[j]) || "*xX#•…".contains(s[j])) { j += 1 }
            (s[i..j].iter().filter(|c| is_digit(c)).count() >= 2).then_some(j)
        }
        Slot::Payee | Slot::Any => None,
    }
}

/// How many places a line may try for free text that ends at a space before a slot: plenty for a
/// bank's line, and a template still can't make reading hang.
const TRIES: usize = 2_000;

/// One line against a template line: the slots' text, or None.
fn match_line(s: &[char], tokens: &[Token]) -> Option<Vec<(Slot, Option<String>, String)>> {
    let mut tries = TRIES;
    match_from(s, 0, tokens, &mut tries)
}
/// The rest of a line from `at` against the rest of a template line. Free text runs to the first
/// place the next words are, or, before a space and a slot, to the last space where the rest still
/// reads ("セブンイレブン １，５００円").
fn match_from(s: &[char], from: usize, tokens: &[Token], tries: &mut usize) -> Option<Vec<(Slot, Option<String>, String)>> {
    let Some((token, rest)) = tokens.split_first() else { return (skip_space(s, from) == s.len()).then(Vec::new) };
    *tries = tries.checked_sub(1)?;
    let at = skip_space(s, from);
    let then = |kind: Slot, arg: &Option<String>, value: String, end: usize, tries: &mut usize| {
        let mut found = match_from(s, end, rest, tries)?;
        found.insert(0, (kind, arg.clone(), value));
        Some(found)
    };
    match token {
        Token::Literal(l) => match_from(s, literal(s, at, l)?, rest, tries),
        Token::Gap => (at > from || at == s.len()).then(|| match_from(s, at, rest, tries)).flatten(),
        Token::Slot(kind @ (Slot::Payee | Slot::Any), arg) => {
            let value = |end: usize| Some(s[at..end].iter().collect::<String>().trim().to_string()).filter(|v| !v.is_empty());
            match rest.first() {
                // Where the next words are: whole words when they start with a space (" on " isn't in
                // "LONDON"), and a later place when the rest doesn't read from the first ("$100.00.").
                Some(Token::Literal(l)) => {
                    let word = l.first().is_some_and(|c| c.is_whitespace());
                    (at + 1..=s.len()).filter(|&e| (!word || e == s.len() || s[e].is_whitespace()) && literal(s, e, l).is_some())
                        .find_map(|e| then(*kind, arg, value(e)?, e, tries))
                }
                Some(Token::Gap) => (at + 1..s.len()).rev().filter(|&e| s[e].is_whitespace())
                    .find_map(|e| then(*kind, arg, value(e)?, e, tries)),
                _ => then(*kind, arg, value(s.len())?, s.len(), tries),
            }
        }
        Token::Slot(kind, arg) => {
            let end = typed(s, at, *kind, arg.as_deref())?;
            then(*kind, arg, s[at..end].iter().collect(), end, tries)
        }
    }
}

/// What a template read from a text: each slot's text as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub amount: Option<String>,
    pub total: Option<String>,
    pub balance: Option<String>,
    /// A card's credit left.
    pub available: Option<String>,
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
                            Slot::Available => reading.available = Some(value),
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

/// How far from the day its text came a template's date may be. A bank's alert is written for
/// that day or one near it; a format that reads the day and month the wrong way round ("03/11" as
/// 11 March) lands a month or more away, and is left to the assistant.
const DATE_DAYS: i64 = 20;

/// A date written in `order` ("dd/mm/yy", "mmm dd"), as YYYY-MM-DD. One with no year is in the year
/// that puts it on or before the day the text came ([received]), give or take a day. None when
/// it's more than [DATE_DAYS] from that day.
fn date(value: &str, order: &str, received: &str) -> Option<String> {
    let chars: Vec<char> = value.chars().collect();
    let (end, d, m, y) = read_date(&chars, 0, order)?;
    if end != chars.len() { return None; }
    let came = chrono::NaiveDate::parse_from_str(received, "%Y-%m-%d").ok();
    let y = match (y, came) {
        // A year far from the day the text came is another calendar's: a Thai year is 543 ahead
        // (2569, or 69 read as 2069). Any other is left to the assistant.
        (Some(y), Some(came)) => {
            use chrono::Datelike;
            let near = |y: i32| (y - came.year()).abs() <= 1;
            [y, y - 543, 2500 + (y % 100) - 543].into_iter().find(|&y| near(y))?
        }
        (Some(y), None) => y,
        (None, Some(came)) => {
            use chrono::Datelike;
            let this = chrono::NaiveDate::from_ymd_opt(came.year(), m, d)?;
            if this > came + chrono::Duration::days(1) { came.year() - 1 } else { came.year() }
        }
        (None, None) => return None,
    };
    let day = chrono::NaiveDate::from_ymd_opt(y, m, d)?;
    if came.is_some_and(|came| (day - came).num_days().abs() > DATE_DAYS) { return None; }
    Some(day.format("%Y-%m-%d").to_string())
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
/// The item a template's reading of a text makes. Its payee is the user's for the bank's name
/// ([names]), else the bank's own words; the category comes with it. Without [names], the format's
/// reading alone.
pub fn fill(b: &Budget, c: &Compiled, m: &Message, reading: &Reading, names: Option<&Names>) -> Option<Item> {
    let t = &c.template;
    let o = &t.outcome;
    let item = if let Some(skip) = &o.skip {
        Item { source_ids: vec![m.id.clone()], disposition: "ignore".into(), skip: Some(skip.clone()), note: String::new(), evidence: vec![],
            transaction: None, unsure: vec![], template: Some(t.id.clone()), raw: None, kept: false }
    } else {
        let negative = o.sign.as_deref() == Some("-");
        // In the budget's currency, which keeps two decimals: a third that's a zero ("KWD 12.500") goes.
        let signed = |n: &str| {
            let plain = decimal(n, &b.currency);
            let plain = match plain.split_once('.') { Some((_, f)) if f.len() > 2 && f[2..].bytes().all(|c| c == b'0') => plain[..plain.len() - (f.len() - 2)].to_string(), _ => plain };
            format!("{}{plain}", if negative { "-" } else { "" })
        };
        let written = reading.amount.as_ref().or(reading.total.as_ref())?;
        let currency = reading.currency.as_deref().map(|w| currency_code(w, &b.currency)).unwrap_or_else(|| b.currency.clone());
        let amount = if currency == b.currency { Some(signed(written)) } else { reading.total.as_deref().map(signed) };
        // The day the text was written for, or the day it came when the format gives none.
        let day = match &reading.date { Some((value, order)) => date(value, order, &m.received_date)?, None => m.received_date.clone() };
        let raw = reading.payee.clone();
        let kind = o.kind.clone()?;
        let cents = amount.as_deref().and_then(|a| sms_import::cents(a).ok());
        let named = raw.as_deref().zip(names).and_then(|(r, n)| n.of(&t.sender, r, cents));
        let category = named.as_ref().and_then(|n| names?.spending(&kind, n.category_id.clone()));
        // Filed under more than one category, with nothing to say which this is: worth a look.
        let unsure = if category.is_some() && named.as_ref().is_some_and(|n| n.check_category) { vec!["category".to_string()] } else { vec![] };
        let payee = named.map(|n| n.payee).or_else(|| raw.clone()).or_else(|| t.payee.clone())?;
        Item {
            source_ids: vec![m.id.clone()], disposition: o.disposition.clone()?, skip: None,
            note: t.note.replace("{payee}", &payee).trim().to_string().chars().take(200).collect::<String>().trim().to_string(),
            evidence: vec![Evidence { source_id: m.id.clone(), quote: written.clone() }],
            transaction: Some(Transaction {
                written_amount: decimal(written, if reading.amount.is_some() { &currency } else { &b.currency }),
                date: day, amount, currency: if reading.amount.is_some() { currency } else { b.currency.clone() }, payee,
                memo: o.memo.clone().unwrap_or_default(), kind, account_ref: reading.reference.clone(), other_ref: reading.other.clone(),
                other_account: None, category_id: category, account_id: None, transfer_account_id: None,
                balance: reading.balance.as_deref().map(|n| decimal(n, &b.currency)),
                available: reading.available.as_deref().map(|n| decimal(n, &b.currency)),
            }),
            unsure, template: Some(t.id.clone()), raw, kept: false,
        }
    };
    let mut item = item;
    if item.disposition != "ignore" && item.note.is_empty() { item.note = item.transaction.as_ref()?.payee.clone(); }
    sms_import::validate_analysis(b, std::slice::from_ref(m), &Analysis { items: vec![item.clone()] }).ok().map(|_| item)
}

/// The texts split for the assistant: those a template read (with the item it filled, for the
/// assistant to review), those it reads in full, and those read on the phone alone and never sent
/// (`junk`, each `kept`): junk a trusted skip template read, codes the code filter recognised, and
/// money a trusted format read whose payee and category the user has already said ([own_money]).
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

/// Code words that stand alone: "pin" isn't in "SHOPPING" or "SPINNEYS".
const CODE_WORDS_WHOLE: &[&str] = &["otp", "pin"];
/// Code words that end a word, alone or in a compound ("Bestätigungscode"), but don't begin a longer one ("codes").
const CODE_WORDS_ENDING: &[&str] = &["code", "password", "passcode"];

/// A one-time code before any template knows the format: a code phrase, and a 4–8 digit number
/// shortly after a code word, so a purchase that ends "never share your OTP" isn't one. The number
/// stands by itself: not an amount ("2500.00", "SAR 2500"), a card's digits ("*4512"), or part of a
/// date ("02/11/2026"). A code word is a word, or in languages that add endings to it, starts one.
/// Conservative on purpose: a miss only means the assistant reads the text, while a purchase taken
/// for a code would never be seen.
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
        let latin = w[0].is_ascii_alphabetic();
        let (whole, ending) = (CODE_WORDS_WHOLE.contains(word), CODE_WORDS_ENDING.contains(word));
        for i in 0..(lower.len() + 1).saturating_sub(w.len()) {
            if lower[i..i + w.len()] != w[..] { continue; }
            // An Arabic word takes letters on either side ("الرمز", "رمزك"); a Latin one starts a word
            // ("kodunuz"), unless it ends a compound.
            if latin && !ending && i > 0 && lower[i - 1].is_alphanumeric() { continue; }
            if (whole || ending) && lower.get(i + w.len()).is_some_and(|c| c.is_alphanumeric()) { continue; }
            found.push((i, i + w.len()));
        }
    }
    for (start, end) in numbers(&lower) {
        // A number of digits only: one with separators is an amount.
        if !lower[start..end].iter().all(is_digit) || !(4..=8).contains(&(end - start)) { continue; }
        let before = start.checked_sub(1).map(|i| lower[i]);
        let after_it = lower.get(end).copied();
        // A card's masked digits, a decimal's or a date's part, or an amount beside its currency.
        let masked = before.is_some_and(|c| "*x#•…".contains(c));
        let joined = |c: Option<char>, next: Option<&char>| c.is_some_and(|c| "/-.,:".contains(c)) && next.is_some_and(is_digit);
        let part = joined(before, start.checked_sub(2).and_then(|i| lower.get(i))) || joined(after_it, lower.get(end + 1));
        if masked || part || beside_currency(&lower, start, end) { continue; }
        // After a code word ("OTP: 4821"), or first in the text with one soon after ("482193 is your OTP").
        let after = found.iter().any(|&(_, e)| e <= start && start - e <= 60);
        let first = lower[..start].iter().all(|c| !c.is_alphanumeric()) && found.iter().any(|&(s, _)| s >= end && s - end <= 40);
        if after || first { return true; }
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

/// One in twenty texts a trusted money template reads still goes to review: a bank that changed
/// what a line means without changing its words is caught within weeks, not never.
fn audited_money(m: &Message) -> bool {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(m.id.as_bytes());
    (u16::from(d[2]) << 8 | u16::from(d[3])) % 20 == 0
}

/// Whether a trusted money format's reading needs nothing from the assistant: a purchase, income
/// or refund in the budget's currency (a transfer's other account, or a foreign amount, may need
/// it), from a name the user has already called something (a rule, their transactions, a review
/// they changed), with a category that's settled for this amount when it's spending, and not
/// this text's turn to be checked.
fn own_money(names: &Names, c: &Compiled, m: &Message, item: &Item) -> bool {
    let Some(t) = &item.transaction else { return false };
    let Some(raw) = &item.raw else { return false };
    if t.kind == "transfer" || !item.unsure.is_empty() || audited_money(m) { return false; }
    let Some(cents) = t.amount.as_deref().and_then(|a| sms_import::cents(a).ok()) else { return false };
    names.of(&c.template.sender, raw, Some(cents)).is_some_and(|n| n.said && !n.check_category) && (t.kind != "expense" || t.category_id.is_some())
}

/// Sorts texts into lanes by the sender's templates, the most specific first. A template in
/// [trusted] settles on the phone the texts that need nothing more: a skip template's junk, and a
/// money template's readings the user's own names complete ([own_money]). Every other reading is
/// reviewed.
pub fn lanes(b: &Budget, messages: &[Message], templates: &[Template], payees: &[Payee], trusted: &std::collections::HashSet<String>) -> Lanes {
    let mut by_sender: HashMap<String, Vec<Compiled>> = HashMap::new();
    for t in templates {
        if let Ok(c) = compile(t) { by_sender.entry(t.sender.clone()).or_default().push(c) }
    }
    for list in by_sender.values_mut() { list.sort_by(|a, b| b.specific.cmp(&a.specific)) }
    let names = Names::new(b, payees);
    let mut lanes = Lanes::default();
    for (i, m) in messages.iter().enumerate() {
        let candidates = by_sender.get(&sender_key(&m.sender)).map(Vec::as_slice).unwrap_or_default();
        let mut matched = candidates.iter().filter_map(|c| c.read(&m.body).map(|r| (c, r)));
        let first = matched.next();
        let tie = first.as_ref().is_some_and(|(c, _)| matched.any(|(other, _)| other.specific == c.specific));
        match first {
            Some(_) if tie => { lanes.conflicts += 1; lanes.read.push(i) }
            Some((c, reading)) => match fill(b, c, m, &reading, Some(&names)) {
                Some(item) if trusted.contains(&c.template.id) && match item.skip.as_deref() {
                    Some(skip) => !audited(m, skip),
                    None => own_money(&names, c, m, &item),
                } => lanes.junk.push((i, Item { kept: true, ..item })),
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
        let filled = fill(b, &c, m, &reading, None).ok_or_else(|| format!("The template doesn't make a valid item from text {}.", i + 1))?;
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
    let digits = |r: &str| ascii_digits(&r.chars().collect::<Vec<_>>());
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
            ("1'234.50", "1234.50"), ("1'234", "1234"), ("1,23,456.78", "123456.78"), ("1.250.000", "1250000"), ("1.250", "1250"), ("35000", "35000"), ("0.71", "0.71"),
            ("1 249,00", "1249.00"), ("12 345 678", "12345678"), ("150.000", "150000"), ("١٢٠٫٥٠", "120.50"), ("٥٬٠٠٠", "5000"), ("١٢٠٠٠", "12000"),
            ("１，５００", "1500"), ("२,५००.००", "2500.00")] {
            assert_eq!(decimal(written, "EUR"), plain, "{written}");
        }
        // Three decimals only where a currency has them.
        assert_eq!(decimal("1.250", "KWD"), "1.250"); assert_eq!(decimal("12,500.250", "BHD"), "12500.250"); assert_eq!(decimal("1,250", "KWD"), "1250");
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
        // A Thai bank's Buddhist-era year, written whole or as two digits; any other far year is left to the assistant.
        assert_eq!(at("02/11/69", "dd/mm/yy", "2026-11-02").as_deref(), Some("2026-11-02"));
        assert_eq!(at("02/11/2569", "dd/mm/yyyy", "2026-11-02").as_deref(), Some("2026-11-02"));
        assert_eq!(at("02/11/2031", "dd/mm/yyyy", "2026-11-02"), None);
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
            let item = fill(b, &c, &m, &c.read(body).unwrap_or_else(|| panic!("reads {body}")), None).unwrap();
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
        // Arabic-Indic digits, the Arabic decimal mark, pounds as جنيه.
        let eg = template("bank", &["تم خصم {amount} {currency} من حسابكم رقم {ref} لدى {payee} بتاريخ {date:dd/mm/yyyy}"], "-");
        assert_eq!(read(&with("EGP"), &eg, "Bank", "2026-11-02", "تم خصم ١٢٠٫٥٠ جنيه من حسابكم رقم ****٤٥٦٧ لدى كارفور بتاريخ ٠٢/١١/٢٠٢٦"),
            (s("2026-11-02"), Some(s("-120.50")), s("EGP"), s("120.50"), s("كارفور"), Some(s("****٤٥٦٧"))));
        // Yen after the amount, in full-width digits too; a month and day in characters.
        let jp = template("bank", &["【カード利用のお知らせ】カード下4桁{ref} {date:mm/dd} {payee} {amount}{currency}"], "-");
        assert_eq!(read(&with("JPY"), &jp, "Bank", "2026-11-02", "【カード利用のお知らせ】カード下4桁7890 11/02 セブンイレブン １，５００円").1, Some(s("-1500")));
        let cn = template("bank", &["您尾号{ref}的账户{date:mm月dd日}{time}消费人民币{amount}{currency}，商户：{payee}。"], "-");
        assert_eq!(read(&with("CNY"), &cn, "Bank", "2026-11-02", "您尾号1234的账户11月02日15:30消费人民币128.50元，商户：美团。"),
            (s("2026-11-02"), Some(s("-128.50")), s("CNY"), s("128.50"), s("美团"), Some(s("1234"))));
        // Rupiah with points between thousands; rand as R and a date with no separator; dinars' three decimals.
        let id = template("bank", &["Transaksi {currency} {amount} di {payee} kartu {ref} berhasil pada {date:dd/mm/yyyy}."], "-");
        assert_eq!(read(&with("IDR"), &id, "Bank", "2026-11-02", "Transaksi Rp 150.000 di INDOMARET kartu ****8899 berhasil pada 02/11/2026.").1, Some(s("-150000")));
        let za = template("bank", &["FNB: {currency}{amount} paid from Cheq a/c {ref} @ {payee}. Avail R{balance}. {date:ddmmm} {time}"], "-");
        assert_eq!(read(&with("ZAR"), &za, "Bank", "2026-11-02", "FNB: R1,250.00 paid from Cheq a/c ..4455 @ CHECKERS SANDTON. Avail R8,900.12. 02Nov 15:30"),
            (s("2026-11-02"), Some(s("-1250.00")), s("ZAR"), s("1250.00"), s("CHECKERS SANDTON"), Some(s("..4455"))));
        let kw = template("bank", &["Purchase of {currency} {amount} at {payee} with card ending {ref} on {date:dd/mm/yyyy}"], "-");
        assert_eq!(read(&with("KWD"), &kw, "Bank", "2026-11-02", "Purchase of KWD 12.500 at THE AVENUES with card ending 3456 on 02/11/2026").1, Some(s("-12.50")));
        // Short words after a payee are whole words (" on " isn't the end of "LONDON"); a point that
        // ends a sentence isn't the first one in it ("$100.00.").
        let fx = template("bank", &["Card ending {ref}: purchase of {currency} {amount} at {payee} on {date:mm/dd/yy}. Amount in USD: ${total} incl. foreign transaction fee ${n}."], "-");
        assert_eq!(read(&with("USD"), &fx, "Bank", "2026-11-04", "Card ending 9876: purchase of GBP 20.00 at PRET A MANGER LONDON on 11/04/26. Amount in USD: $25.62 incl. foreign transaction fee $0.75."),
            (s("2026-11-04"), Some(s("-25.62")), s("GBP"), s("20.00"), s("PRET A MANGER LONDON"), Some(s("9876"))));
        let at = template("bank", &["Purchase of {currency}{amount} at {payee}."], "-");
        assert_eq!(read(&with("USD"), &at, "Bank", "2026-11-04", "Purchase of $12.00 at AMAZON.COM.").4, s("AMAZON.COM"));
        let mut plan = template("bank", &["Your purchase of {currency}{amount} at {payee} has been converted to {n} monthly installments of {any}."], "-");
        plan.outcome = Outcome { skip: Some("other".into()), ..Outcome::default() };
        assert!(compile(&plan).unwrap().read("Your purchase of $1,200.00 at APPLE STORE has been converted to 12 monthly installments of $100.00.").is_some());
        // M-Pesa: a confirmation code, a name then a phone number, a time with PM, and a fee in a plain number.
        let mpesa = template("mpesa", &["{any} Confirmed. {currency}{amount} sent to {payee} {n} on {date:dd/mm/yy} at {time}. New M-PESA balance is {currency}{balance}. Transaction cost, {currency}{n}."], "-");
        assert_eq!(read(&with("KES"), &mpesa, "MPESA", "2026-11-03", "QK12AB34CD Confirmed. Ksh500.00 sent to JOHN DOE 0712345678 on 3/11/26 at 2:15 PM. New M-PESA balance is Ksh1,234.00. Transaction cost, Ksh7.00."),
            (s("2026-11-03"), Some(s("-500.00")), s("KES"), s("500.00"), s("JOHN DOE"), None));
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
        let learned = [Payee { sender: "anb".into(), raw: "noon  e".into(), payee: "Noon".into(), category_id: Some("food".into()), mine: true }];
        let item = fill(&b, &c, &m, &c.read(BODY).unwrap(), Some(&Names::new(&b, &learned))).unwrap();
        let t = item.transaction.as_ref().unwrap();
        assert_eq!((t.date.as_str(), t.amount.as_deref(), t.written_amount.as_str(), t.payee.as_str()), ("2026-11-01", Some("-1065.97"), "1065.97", "Noon"));
        assert_eq!((t.account_ref.as_deref(), t.category_id.as_deref(), t.memo.as_str()), (Some("1234"), Some("food"), "Online"));
        assert_eq!(item.note, "Purchase at Noon"); assert_eq!(item.evidence[0].quote, "1,065.97");
        assert_eq!(item.template.as_deref(), Some("t1")); assert_eq!(item.raw.as_deref(), Some("NOON E"));
        // The balance the text gives after it, for a new account's start; none when the line is left out.
        assert_eq!(t.balance.as_deref(), Some("9120.40"));
        let short = "Online Purchase
VISA Card:1234
Amount:SAR 5
At:SHOP
On:02/11/26 07:47";
        assert_eq!(fill(&b, &c, &text("sms:2", short), &c.read(short).unwrap(), None).unwrap().transaction.unwrap().balance, None);
        // Without a learned payee the bank's own words stand.
        assert_eq!(fill(&b, &c, &m, &c.read(BODY).unwrap(), None).unwrap().transaction.unwrap().payee, "NOON E");
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
        let learned = [Payee { sender: "anb".into(), raw: "NOON E".into(), payee: "Noon Minutes".into(), category_id: None, mine: true }];
        let read = lanes(&b, &[text("sms:1", BODY)], &[purchase()], &learned, &Default::default());
        let t = read.review[0].1.transaction.as_ref().unwrap();
        assert_eq!((t.payee.as_str(), t.category_id.as_deref()), ("Noon Minutes", None));
    }
    /// A budget where the user renamed the bank's "SALON A" to their barber's name when they added it.
    fn renamed() -> Budget {
        let mut b = budget();
        let mut cut = crate::test_support::purchase();
        (cut.id, cut.date, cut.payee, cut.account_id) = ("cut".into(), "2026-10-01".into(), "Blade and Fade".into(), "main".into());
        cut.extra.insert("importedPayee".into(), serde_json::json!("SALON A 0412"));
        b.transactions.push(cut);
        b
    }
    const SALON: &str = "Online Purchase
VISA Card:1234
Amount:SAR 45.00
At:SALON A
On:01/11/26 13:05";
    #[test]
    fn a_name_the_user_renamed_before_is_theirs_whoever_reads_the_text() {
        let b = renamed();
        // A format the phone knows reads it as theirs, with the category they gave it.
        let read = lanes(&b, &[text("sms:1", SALON)], &[purchase()], &[], &Default::default());
        let (item, t) = (&read.review[0].1, read.review[0].1.transaction.as_ref().unwrap());
        assert_eq!((t.payee.as_str(), t.category_id.as_deref(), item.note.as_str(), item.raw.as_deref()), ("Blade and Fade", Some("food"), "Purchase at Blade and Fade", Some("SALON A")));
        // So does the assistant's reading of it: the user's name comes first, and there's nothing to check.
        let mut items = vec![Item { raw: Some("SALON A".into()), template: None, note: "Haircut at Salon A".into(), unsure: vec!["payee".into(), "category".into()], ..item.clone() }];
        if let Some(t) = items[0].transaction.as_mut() { (t.payee, t.category_id) = ("Salon A".into(), None); }
        own_names(&Names::new(&b, &[]), &[text("sms:1", SALON)], &mut items);
        let t = items[0].transaction.as_ref().unwrap();
        assert_eq!((t.payee.as_str(), t.category_id.as_deref(), items[0].note.as_str()), ("Blade and Fade", Some("food"), "Haircut at Blade and Fade"));
        assert!(items[0].unsure.is_empty());
    }
    #[test]
    fn a_rule_comes_first_then_what_the_user_did_then_what_was_read() {
        let mut b = renamed();
        let learned = |payee: &str, mine: bool| [Payee { sender: "anb".into(), raw: "SALON A".into(), payee: payee.into(), category_id: None, mine }];
        let named = |b: &Budget, learned: &[Payee]| Names::new(b, learned).of("anb", "SALON A", None).map(|n| (n.payee, n.said));
        // Their history beats a name the phone learned, theirs or the assistant's.
        assert_eq!(named(&b, &learned("Salon A", true)), Some(("Blade and Fade".into(), true)));
        // A rule beats both, and gives its category.
        b.extra.insert("payeeRules".into(), serde_json::json!([{"id":"r","match":"salon a","payee":"Barber","categoryId":null}]));
        assert_eq!(named(&b, &[]), Some(("Barber".into(), true)));
        // Without either, the phone's: the user's own is said, the assistant's reading isn't.
        let plain = budget();
        assert_eq!(named(&plain, &learned("Salon A", true)), Some(("Salon A".into(), true)));
        assert_eq!(named(&plain, &learned("Salon A", false)), Some(("Salon A".into(), false)));
        assert_eq!(named(&plain, &[]), None);
        // A branch number or punctuation doesn't make another name; another script's digits are numbers too.
        assert_eq!(crate::workflows::bank_key("Salon-A  0533"), crate::workflows::bank_key("SALON A"));
        assert_eq!(crate::workflows::bank_key("صالون ١٢٣"), "صالون");
        // A transfer gets no category from a name.
        let mut items = vec![fill(&renamed(), &compile(&purchase()).unwrap(), &text("sms:1", SALON), &compile(&purchase()).unwrap().read(SALON).unwrap(), None).unwrap()];
        if let Some(t) = items[0].transaction.as_mut() { t.kind = "transfer".into(); t.amount = Some("-45.00".into()); }
        own_names(&Names::new(&renamed(), &[]), &[text("sms:1", SALON)], &mut items);
        assert_eq!(items[0].transaction.as_ref().unwrap().category_id, None);
    }
    #[test]
    fn a_foreign_amount_waits_unless_the_format_gives_the_total_and_dates_follow_their_order() {
        let b = budget();
        let mut t = purchase();
        t.lines = ["International Purchase", "Amount : {currency} {amount}", "?Total amount : SAR {total}", "At : {payee}", "On : {date:yy-mm-dd}"].map(String::from).to_vec();
        let c = compile(&t).unwrap();
        let waits = text("sms:2", "International Purchase\nAmount: USD 42.00\nAt: CAFE\nOn: 26-10-31");
        let item = fill(&b, &c, &waits, &c.read(&waits.body).unwrap(), None).unwrap();
        let tx = item.transaction.unwrap();
        assert_eq!((tx.amount, tx.currency.as_str(), tx.date.as_str()), (None, "USD", "2026-10-31"));
        let total = text("sms:3", "International Purchase\nAmount: USD 42.00\nTotal amount: SAR 160.50\nAt: CAFE\nOn: 26-10-31");
        assert_eq!(fill(&b, &c, &total, &c.read(&total.body).unwrap(), None).unwrap().transaction.unwrap().amount.as_deref(), Some("-160.50"));
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
        // Free text before a space and a slot is tried at each space, but only so many times:
        // a line of nothing but spaces and words gives up quickly.
        t = purchase();
        t.lines = vec!["Paid {any} {any} {any} {any} {payee} {amount} {currency}".into()];
        let c = compile(&t).unwrap();
        let long = format!("Paid {}", "word ".repeat(60));
        let began = std::time::Instant::now();
        assert!(c.read(&long).is_none());
        assert!(began.elapsed() < std::time::Duration::from_millis(500), "{:?}", began.elapsed());
        assert!(c.read("Paid a b c d SHOP 12.50 USD").is_some());
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
        // A trusted money format reads on the phone alone the texts whose payee the user has named,
        // one in twenty still reviewed; a name they haven't is the assistant's to read.
        let money: Vec<Message> = (0..200).map(|i| text(&format!("sms:m{i}"), BODY)).collect();
        let trusted = std::collections::HashSet::from(["t1".to_string()]);
        let named = [Payee { sender: "anb".into(), raw: "NOON E".into(), payee: "Noon".into(), category_id: Some("food".into()), mine: true }];
        let l = lanes(&b, &money, &[purchase()], &named, &trusted);
        assert!(l.junk.len() > 170 && !l.review.is_empty() && l.junk.len() + l.review.len() == 200, "{} {}", l.junk.len(), l.review.len());
        assert!(l.junk.iter().all(|(_, item)| item.kept && item.disposition == "transaction" && item.transaction.as_ref().is_some_and(|t| t.payee == "Noon" && t.category_id.as_deref() == Some("food"))));
        assert_eq!(lanes(&b, &money, &[purchase()], &[], &trusted).review.len(), 200);
        assert_eq!(lanes(&b, &money, &[purchase()], &named, &Default::default()).review.len(), 200);
        // The assistant's own guess at the name isn't the user's say-so.
        let guessed = [Payee { mine: false, ..named[0].clone() }];
        assert_eq!(lanes(&b, &money, &[purchase()], &guessed, &trusted).review.len(), 200);
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
        for money in ["Compra de 45,00 € en MERCADONA con tarjeta 4512. Nunca compartas tu código.", "You spent £12.50 at TESCO with card 4512. We'll never ask for your passcode.",
            // A code word inside another word isn't one, and neither an amount, a card's digits nor a year is a code.
            "Purchase SAR 50.00 at SHOPPING MALL card 4512. Never share your OTP.",
            "Your card ending 1234 was used for SAR 50.00 at SPINNEYS on 2026-11-02. Never share your OTP.",
            "PIN purchase of SAR 2500 at STORE with card *4512 on 02/11/2026. Never share your PIN.",
            "PIN purchase of 2500.00 at STORE. Do not share your OTP with anyone.",
            "Compra con código promocional por 1500 € en TIENDA. No compartas tu código."] {
            assert!(!looks_like_code(money), "{money}");
        }
        // A code beside a purchase's amount and card is still a code.
        for code in ["Your verification code for a purchase of SAR 299.00 at ZAIN on card *4512 is 482193. Do not share it.", "OTP-4821 for your login. Never share it."] {
            assert!(looks_like_code(code), "{code}");
        }
    }
    #[test]
    fn a_template_is_kept_only_when_it_reads_the_batch_as_the_assistant_did() {
        let b = budget();
        let messages = [text("sms:1", BODY), text("sms:2", "Online Purchase\nVISA Card:1234\nAmount:SAR 8.00\nAt:CAFE\nOn:01/11/26 09:00")];
        let c = compile(&purchase()).unwrap();
        let items: Vec<Item> = messages.iter().map(|m| fill(&b, &c, m, &c.read(&m.body).unwrap(), None).unwrap()).collect();
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
    fn a_payee_filed_two_ways_gets_the_category_its_amount_has() {
        // Amazon: a 16.00 subscription every month, and purchases of all sizes.
        let mut b = budget();
        b.categories.push(Category { extra: Default::default(), id: "subs".into(), name: "Online subscriptions".into(), group: "Bills".into(), target: 0 });
        for (id, date, amount, category) in [("a1", "2026-08-03", -1600, "subs"), ("a2", "2026-08-19", -24350, "food"), ("a3", "2026-09-03", -1600, "subs"),
            ("a4", "2026-09-21", -8999, "food"), ("a5", "2026-10-03", -1600, "subs")] {
            let mut extra = crate::domain::Extra::default();
            extra.insert("importedPayee".into(), serde_json::json!("AMAZON SA"));
            b.transactions.push(crate::domain::Transaction { extra,
                id: id.into(), account_id: "main".into(), date: date.into(), payee: "Amazon".into(), memo: String::new(), amount,
                status: crate::domain::Cleared::Uncleared, transfer_account_id: None, transfer_status: None,
                splits: vec![crate::domain::Split { extra: Default::default(), category_id: Some(category.into()), amount }] });
        }
        let names = Names::new(&b, &[]);
        let of = |amount: Option<i64>| names.of("anb", "AMAZON SA", amount).map(|n| (n.category_id, n.check_category));
        // The subscription's price is the subscription, whichever was entered last.
        assert_eq!(of(Some(-1600)), Some((Some("subs".into()), false)));
        // A little off, as a price in another currency moves: still the subscription.
        assert_eq!(of(Some(-1625)), Some((Some("subs".into()), false)));
        // Any other amount is what they buy there, not the subscription that came last.
        assert_eq!(of(Some(-5400)), Some((Some("food".into()), false)));
        // With no amount to go by, the purchases' category, settled since the latest two agree.
        assert_eq!(of(None), Some((Some("food".into()), false)));
        // A template's reading of the text says the same, and so does a reading the assistant categorized otherwise.
        let c = compile(&purchase()).unwrap();
        let m = text("sms:7", &BODY.replace("NOON E", "AMAZON SA").replace("SAR 1,065.97", "SAR 16.00"));
        let item = fill(&b, &c, &m, &c.read(&m.body).unwrap(), Some(&names)).unwrap();
        assert_eq!(item.transaction.as_ref().unwrap().category_id.as_deref(), Some("subs"));
        let mut items = vec![fill(&b, &c, &m, &c.read(&m.body).unwrap(), None).unwrap()];
        items[0].transaction.as_mut().unwrap().category_id = Some("food".into());
        own_names(&names, std::slice::from_ref(&m), &mut items);
        assert_eq!((items[0].transaction.as_ref().unwrap().payee.as_str(), items[0].transaction.as_ref().unwrap().category_id.as_deref()), ("Amazon", Some("subs")));

        // Two categories with nothing to tell them apart: the latest, and worth a look until two in a row agree.
        let filed = |rows: &[(&str, i64, &str)]| rows.iter().map(|(d, a, c)| crate::workflows::Filed { date: d.to_string(), amount: *a, category_id: c.to_string() }).collect::<Vec<_>>();
        let mixed = filed(&[("2026-09-01", 1200, "food"), ("2026-09-08", 4500, "food"), ("2026-10-02", 3000, "fun")]);
        assert_eq!(crate::workflows::category_for(&mixed, Some(999)), Some(("fun".into(), false)));
        let again = filed(&[("2026-09-01", 1200, "food"), ("2026-10-02", 3000, "fun"), ("2026-10-09", 2100, "fun")]);
        assert_eq!(crate::workflows::category_for(&again, Some(999)), Some(("fun".into(), true)));
        // An amount seen before goes where it went, the latest when it went both ways.
        assert_eq!(crate::workflows::category_for(&mixed, Some(-1200)), Some(("food".into(), true)));
        // One category is always it; none says nothing.
        assert_eq!(crate::workflows::category_for(&filed(&[("2026-09-01", 1200, "food")]), Some(5)), Some(("food".into(), true)));
        assert_eq!(crate::workflows::category_for(&[], Some(5)), None);
    }
    #[test]
    fn dates_and_amounts_as_more_banks_write_them() {
        // Month names beyond English, and none that could be two months.
        for (name, n) in [("ago", 8), ("déc", 12), ("Dez", 12), ("ene", 1), ("mag", 5), ("Eki", 10), ("نوفمبر", 11), ("juin", 6), ("juil", 7), ("Sept", 9)] {
            assert_eq!(month(name), Some(n), "{name}");
        }
        assert_eq!(month("jui"), None);
        assert_eq!(date("02 نوفمبر 2026", "dd mmm yyyy", "2026-11-02").as_deref(), Some("2026-11-02"));
        assert_eq!(date("3 ago 2026", "dd mmm yyyy", "2026-08-03").as_deref(), Some("2026-08-03"));
        // A day and month read the wrong way round land far from the day the text came: left to the assistant.
        assert_eq!(date("03/11/26", "dd/mm/yy", "2026-11-03").as_deref(), Some("2026-11-03"));
        assert_eq!(date("03/11/26", "mm/dd/yy", "2026-11-03"), None);
        assert_eq!(date("28/10/26", "dd/mm/yy", "2026-11-03").as_deref(), Some("2026-10-28"));
        // A dinar's millimes after a comma, where the currency writes them so; elsewhere a comma groups.
        assert_eq!(decimal("45,500", "TND"), "45.500");
        assert_eq!(decimal("45,500", "KWD"), "45500");
        assert_eq!(decimal("1.250", "KWD"), "1.250");
        assert_eq!(decimal("1,245,500", "TND"), "1245500");
        // A time in another script's digits is still a time.
        let t = Template { id: "t9".into(), sender: "anb".into(), lines: vec!["شراء {amount} {currency} من {payee} الساعة {time}".into()],
            outcome: Outcome { disposition: Some("transaction".into()), kind: Some("expense".into()), sign: Some("-".into()), ..Outcome::default() }, note: String::new(), payee: None };
        assert!(compile(&t).unwrap().read("شراء ١٢٠٫٥٠ SAR من متجر الساعة ١٤:٣٥").is_some());
        // Marks no one sees don't keep a format from reading its text.
        assert!(compile(&purchase()).unwrap().read(&format!("\u{FEFF}{}", BODY.replace("Amount:", "Amount:\u{200B}"))).is_some());
    }
    #[test]
    fn the_same_format_written_twice_is_one_template() {
        let lines = purchase().lines;
        assert_eq!(template_id("anb", &lines), template_id("anb", &lines));
        assert_ne!(template_id("anb", &lines), template_id("other", &lines));
        assert_eq!(sender_key("ANB"), "anb"); assert_eq!(sender_key("+966 (50) 123-4567"), "+966501234567");
    }
}
