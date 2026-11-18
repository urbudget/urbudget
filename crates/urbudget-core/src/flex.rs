//! Interactive Brokers' Flex statements: the XML an Activity Flex Query produces, whether the
//! Flex Web Service sent it or the user saved it from Client Portal. A statement says what an
//! account holds and what it is worth, in IBKR's own figures: closing prices, cost, cash in each
//! currency, the account's value on each day, and the day's exchange rates. This reads those
//! figures as they are and works nothing out again (see `investments`).
//!
//! The reader takes whatever the query was set up to send. Sections the user didn't tick are
//! listed as missing rather than refused, dates read in any of the formats a query can use, and
//! a query that breaks the period out by day (one statement per day) reads as one account.
use crate::bank_statements::{xml, Node};
use crate::t;
use chrono::NaiveDate;

/// A decimal number from a statement, kept to eight places: quantities, prices and rates carry
/// more digits than money does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dec(i128);
const SCALE: i128 = 100_000_000;
fn div_round(n: i128, d: i128) -> i128 {
    let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
    if n >= 0 { (n + d / 2) / d } else { -((-n + d / 2) / d) }
}
impl Dec {
    pub const ZERO: Dec = Dec(0);
    pub const ONE: Dec = Dec(SCALE);
    /// "1234.5", "-0.25", "1,234.50" or "1.2E-4". None for anything else.
    pub fn parse(s: &str) -> Option<Dec> {
        let s = s.trim();
        if s.contains(['e', 'E']) {
            let f: f64 = s.parse().ok()?;
            return (f.is_finite() && f.abs() < 1e18).then(|| Dec((f * SCALE as f64).round() as i128));
        }
        let (negative, digits) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
        let whole: String = whole.chars().filter(|c| *c != ',').collect();
        if whole.is_empty() && frac.is_empty()
            || whole.len() > 18
            || !whole.bytes().all(|b| b.is_ascii_digit())
            || !frac.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut n: i128 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
        n *= SCALE;
        let mut place = SCALE / 10;
        for (i, b) in frac.bytes().enumerate() {
            let digit = i128::from(b - b'0');
            if i < 8 {
                n += digit * place;
                place /= 10;
            } else {
                if digit >= 5 {
                    n += 1;
                }
                break;
            }
        }
        Some(Dec(if negative { -n } else { n }))
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
    pub fn is_negative(self) -> bool {
        self.0 < 0
    }
    pub fn abs(self) -> Dec {
        Dec(self.0.abs())
    }
    pub fn negated(self) -> Dec {
        Dec(-self.0)
    }
    pub fn plus(self, other: Dec) -> Dec {
        Dec(self.0.saturating_add(other.0))
    }
    pub fn times(self, other: Dec) -> Dec {
        match self.0.checked_mul(other.0) {
            Some(n) => Dec(div_round(n, SCALE)),
            None => Dec(((self.0 as f64 / SCALE as f64) * other.0 as f64).round() as i128),
        }
    }
    /// None when dividing by zero.
    pub fn over(self, other: Dec) -> Option<Dec> {
        if other.0 == 0 {
            return None;
        }
        Some(match self.0.checked_mul(SCALE) {
            Some(n) => Dec(div_round(n, other.0)),
            None => Dec((self.0 as f64 / other.0 as f64 * SCALE as f64).round() as i128),
        })
    }
    /// As money in hundredths, rounded half away from zero.
    pub fn cents(self) -> i64 {
        div_round(self.0, SCALE / 100).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }
    /// With exactly `places` decimals (at most eight), rounded half away from zero: "61476.00".
    pub fn fixed(self, places: usize) -> String {
        let places = places.min(8);
        let step = 10_i128.pow(8 - places as u32);
        let n = div_round(self.0, step).unsigned_abs();
        let scale = 10_u128.pow(places as u32);
        let sign = if self.0 < 0 && n != 0 { "-" } else { "" };
        match places {
            0 => format!("{sign}{n}"),
            _ => format!("{sign}{}.{:0width$}", n / scale, n % scale, width = places),
        }
    }
    /// As written, without trailing zeros: "25", "0.125", "-3.5".
    pub fn text(self) -> String {
        let n = self.0.unsigned_abs();
        let scale = SCALE as u128;
        let (whole, frac) = (n / scale, n % scale);
        let mut out = format!("{}{whole}", if self.0 < 0 { "-" } else { "" });
        if frac != 0 {
            out.push('.');
            out.push_str(format!("{frac:08}").trim_end_matches('0'));
        }
        out
    }
}

/// A decimal as a statement or file wrote it, plainly: without grouping or a plus sign, and
/// with the decimals it was written with ("1,234.50" is "1234.50"). None when it doesn't read.
pub fn plain(written: &str) -> Option<String> {
    let value = Dec::parse(written)?;
    let places = written.trim().split_once('.').map_or(0, |(_, decimals)| decimals.chars().take_while(char::is_ascii_digit).count());
    Some(value.fixed(places))
}
/// Two plain decimals added up, with as many decimals as the longer of them; None when
/// either doesn't read. Money in one currency is added this way, never through floating point.
pub fn add_plain(a: &str, b: &str) -> Option<String> {
    let places = |s: &str| s.split_once('.').map_or(0, |(_, decimals)| decimals.len());
    Some(Dec::parse(a)?.plus(Dec::parse(b)?).fixed(places(a).max(places(b))))
}

/// Something the account holds: a stock, fund, bond, option or other position.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub symbol: String,
    pub name: String,
    /// IBKR's asset category: STK, FUND, BOND, OPT, FUT, CASH (a currency position) and so on.
    pub kind: String,
    /// IBKR's finer category, as written (COMMON, ETF, REIT, ADR, PREFERRED), or empty.
    pub sub_kind: String,
    /// "C" or "P" for an option, as written; else empty.
    pub put_call: String,
    pub currency: String,
    /// Negative when the position is short.
    pub quantity: Dec,
    /// The closing price on the statement's last day, in the position's currency.
    pub price: Dec,
    /// What the position is worth, in its currency.
    pub value: Dec,
    /// The same, as the statement wrote it (`plain`), or worked out to the cent when it gives
    /// none.
    pub amount: String,
    /// The position's currency in the account's base currency, when the query includes the
    /// field. Never assumed: a holding in another currency would be valued as if it weren't.
    pub fx: Option<Dec>,
    /// What it cost, in its currency, when the statement says.
    pub cost: Option<Dec>,
    /// The gain not yet realised, in its currency, when the statement says.
    pub gain: Option<Dec>,
    pub conid: String,
    pub isin: String,
    pub figi: String,
    /// The exchange it is listed on, as IBKR names it (NASDAQ, LSE, TADAWUL).
    pub exchange: String,
}
/// Cash the account holds in one currency.
#[derive(Clone, Debug, PartialEq)]
pub struct Cash {
    pub currency: String,
    pub amount: Dec,
}
/// Money put into the account or taken out of it: a deposit or withdrawal, or cash and
/// holdings transferred from or to another account, at what they were worth.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    /// IBKR's transaction id, or empty when the statement gives summaries only.
    pub id: String,
    pub date: String,
    pub currency: String,
    /// Positive for a deposit.
    pub amount: Dec,
    /// The flow's currency in the account's base currency, when the query includes the field.
    pub fx: Option<Dec>,
    pub description: String,
    /// It moved from or to another account (the Transfers section), not the user's bank.
    pub transfer: bool,
    /// The account on the other side of a transfer between the user's own accounts at IBKR
    /// (U7654321), when the statement says; else empty. Never an account at another firm.
    pub counter: String,
}
/// What a cash movement that isn't a deposit, a withdrawal or a trade was for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncomeKind {
    Dividends,
    Interest,
    /// Tax withheld, mostly on dividends.
    Tax,
    /// Fees the broker charged apart from trades: market data, advisers, adjustments.
    Fees,
    Other,
}
/// The holding a row is about, as far as the row says: any of these may be empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Security {
    pub symbol: String,
    pub conid: String,
    pub isin: String,
    /// The row's own description. On a dividend it is the payment's wording, not the
    /// holding's name.
    pub description: String,
}
impl Security {
    /// Whether the row names a holding at all.
    pub fn named(&self) -> bool {
        !(self.symbol.is_empty() && self.conid.is_empty() && self.isin.is_empty())
    }
}
/// Money the account earned or was charged: a dividend, interest, tax withheld, a fee.
#[derive(Clone, Debug, PartialEq)]
pub struct Income {
    pub date: String,
    pub kind: IncomeKind,
    pub currency: String,
    /// Positive when the account received it.
    pub amount: Dec,
    /// Its currency in the account's base currency, when the query includes the field.
    pub fx: Option<Dec>,
    /// The holding it was paid on or charged for, when the row names one.
    pub security: Security,
}
/// One trade, from the row the statement gives it at the level that is counted (see `trades`).
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    /// IBKR's transaction id, or empty.
    pub id: String,
    pub date: String,
    pub currency: String,
    /// The trade's currency in the account's base currency, when the query includes the field.
    pub fx: Option<Dec>,
    /// What closing a position gained or lost, in the trade's currency, when the row says.
    pub realised: Option<Dec>,
    /// The commission, negative when paid, when the row says; and the currency it was
    /// charged in, or empty for the trade's own.
    pub commission: Option<Dec>,
    pub commission_currency: String,
}
/// One unit of `from` in `to` on a day.
#[derive(Clone, Debug, PartialEq)]
pub struct Rate {
    pub date: String,
    pub from: String,
    pub to: String,
    pub rate: Dec,
}
/// One IBKR account in a statement.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    /// IBKR's account id (U1234567).
    pub id: String,
    /// The alias the user gave it at IBKR, if any. Never the holder's name.
    pub alias: String,
    /// The account's base currency, or empty when the statement doesn't say.
    pub base: String,
    /// The statement's first and last day (YYYY-MM-DD), or empty.
    pub from: String,
    pub to: String,
    /// When the broker made the statement, on its own clock ("2026-11-16 13:45"), or empty. It
    /// can serve the same statement for a while, so a change to the query shows only in the
    /// next one it makes.
    pub generated: String,
    pub positions: Vec<Position>,
    pub cash: Vec<Cash>,
    /// All cash together in the base currency, when the statement says.
    pub cash_base: Option<Dec>,
    /// The account's value in its base currency at the end of each day, oldest first.
    pub values: Vec<(String, Dec)>,
    pub flows: Vec<Flow>,
    /// Dividends, interest, tax withheld and fees, each as it was paid.
    pub income: Vec<Income>,
    /// Commissions on trades over the statement's period, in the base currency and signed as
    /// the broker signs them (negative when paid), when the Cash Report says.
    pub commissions: Option<Dec>,
    /// Each trade in the period, when the query includes the Trades section; without it the
    /// section is listed as missing, as the others are.
    pub trades: Option<Vec<Trade>>,
    pub rates: Vec<Rate>,
    /// The sections the query didn't include, by the name Client Portal shows.
    pub missing: Vec<&'static str>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Statement {
    pub accounts: Vec<Account>,
}

/// Whether this text is a Flex statement in XML.
pub fn sniff(content: &str) -> bool {
    let head: String = content.trim_start_matches('\u{feff}').chars().take(2048).collect();
    head.contains("<FlexQueryResponse") || head.contains("<FlexStatements")
}

/// Text from a statement made safe to show and keep: one line, no control or direction-changing
/// characters, at most 120 characters.
pub fn clean(s: &str) -> String {
    let safe: String = s
        .chars()
        .map(|c| {
            let hidden = matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}');
            if c.is_control() || hidden { ' ' } else { c }
        })
        .collect();
    let line = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 120 {
        line.chars().take(119).collect::<String>() + "…"
    } else {
        line
    }
}

fn attr<'a>(n: &'a Node, name: &str) -> &'a str {
    n.attr(name).unwrap_or("").trim()
}
fn number(n: &Node, name: &str) -> Option<Dec> {
    Dec::parse(attr(n, name))
}
fn code(s: &str) -> String {
    let s = s.trim().to_ascii_uppercase();
    if s.len() == 3 && s.bytes().all(|b| b.is_ascii_alphabetic()) { s } else { String::new() }
}

/// A date in any format a Flex Query can be set to write, alone or at the start of a date and
/// time: 20261113, 2026-11-13, 11/13/2026, 13/11/26, 13-Nov-26. `day_first` settles 03/04/2026.
fn day(s: &str, day_first: bool) -> Option<String> {
    let s = s.trim();
    let head = s.split([';', ' ', ',', 'T']).next()?;
    let date = if head.len() >= 8 && head.bytes().take(8).all(|b| b.is_ascii_digit()) {
        NaiveDate::parse_from_str(&head[..8], "%Y%m%d").ok()?
    } else if head.contains('/') {
        let parts: Vec<u32> = head.split('/').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        let [a, b, year] = parts[..] else { return None };
        let year = if year < 100 { 2000 + year } else { year };
        let (month, day) = if day_first { (b, a) } else { (a, b) };
        NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)?
    } else if head.as_bytes().get(4) == Some(&b'-') {
        NaiveDate::parse_from_str(head.get(..10)?, "%Y-%m-%d").ok()?
    } else {
        NaiveDate::parse_from_str(head, "%d-%b-%y").or_else(|_| NaiveDate::parse_from_str(head, "%d-%b-%Y")).ok()?
    };
    Some(date.format("%Y-%m-%d").to_string())
}
/// A statement's `whenGenerated` ("20261116;134519") as a day and a time to the minute, or the
/// day alone when the time doesn't read, or nothing.
fn made(s: &str, day_first: bool) -> String {
    let Some(date) = day(s, day_first) else { return String::new() };
    let time: String = s.trim().split([';', ' ', ',', 'T']).nth(1).unwrap_or("").chars().filter(char::is_ascii_digit).collect();
    match time.get(..4) {
        Some(hm) if s.trim().split([';', ' ', ',', 'T']).count() >= 2 => format!("{date} {}:{}", &hm[..2], &hm[2..]),
        _ => date,
    }
}
/// Whether the statement writes days before months: true once any slashed date starts above 12.
fn day_first(root: &Node) -> bool {
    fn scan(n: &Node, found: &mut Option<bool>) {
        for (_, v) in &n.attrs {
            let head = v.split([';', ' ', ',']).next().unwrap_or("");
            let mut parts = head.split('/');
            if let (Some(a), Some(b), Some(_), None) = (parts.next(), parts.next(), parts.next(), parts.next()) {
                match (a.parse::<u32>(), b.parse::<u32>()) {
                    (Ok(a), Ok(_)) if a > 12 => *found = Some(true),
                    (Ok(_), Ok(b)) if b > 12 && found.is_none() => *found = Some(false),
                    _ => {}
                }
            }
        }
        for c in &n.children {
            if *found != Some(true) {
                scan(c, found);
            }
        }
    }
    let mut found = None;
    scan(root, &mut found);
    found.unwrap_or(false)
}

/// The rows of a section, or None when the query didn't include it.
fn section<'a>(statement: &'a Node, name: &str, row: &'a str) -> Option<Vec<&'a Node>> {
    statement.child(name).map(|s| s.all(row).collect())
}
fn summary(n: &Node) -> bool {
    attr(n, "levelOfDetail").eq_ignore_ascii_case("SUMMARY")
}

fn positions(rows: &[&Node]) -> Vec<Position> {
    // A query can send each position whole, lot by lot, or both: the whole ones when there are
    // any, else the lots added up.
    let whole = rows.iter().any(|r| summary(r));
    let mut out: Vec<Position> = vec![];
    for r in rows.iter().filter(|r| !whole || summary(r)) {
        let quantity = number(r, "position").unwrap_or_default();
        let price = number(r, "markPrice").unwrap_or_default();
        let multiplier = number(r, "multiplier").filter(|m| !m.is_zero()).unwrap_or(Dec::ONE);
        let written = number(r, "positionValue").and_then(|value| Some((value, plain(attr(r, "positionValue"))?)));
        let (value, amount) = written.unwrap_or_else(|| {
            let worked_out = quantity.times(price).times(multiplier);
            (worked_out, worked_out.fixed(2))
        });
        let p = Position {
            symbol: clean(attr(r, "symbol")),
            name: clean(attr(r, "description")),
            kind: clean(attr(r, "assetCategory")).to_ascii_uppercase(),
            sub_kind: clean(attr(r, "subCategory")),
            put_call: clean(attr(r, "putCall")).to_ascii_uppercase(),
            currency: code(attr(r, "currency")),
            quantity,
            price,
            value,
            amount,
            fx: number(r, "fxRateToBase").filter(|f| !f.is_zero()),
            cost: number(r, "costBasisMoney"),
            gain: number(r, "fifoPnlUnrealized"),
            conid: clean(attr(r, "conid")),
            isin: clean(attr(r, "isin")),
            figi: clean(attr(r, "figi")),
            exchange: clean(attr(r, "listingExchange")),
        };
        if p.quantity.is_zero() && p.value.is_zero() {
            continue;
        }
        let same = |o: &Position| if p.conid.is_empty() { o.symbol == p.symbol && o.currency == p.currency } else { o.conid == p.conid };
        match out.iter_mut().find(|o| !whole && same(o)) {
            Some(known) => {
                known.quantity = known.quantity.plus(p.quantity);
                known.value = known.value.plus(p.value);
                known.amount = add_plain(&known.amount, &p.amount).unwrap_or_else(|| known.value.fixed(2));
                known.cost = known.cost.zip(p.cost).map(|(a, b)| a.plus(b));
                known.gain = known.gain.zip(p.gain).map(|(a, b)| a.plus(b));
            }
            None => out.push(p),
        }
    }
    out
}

fn flows(rows: &[&Node], day_first: bool) -> Vec<Flow> {
    let deposits: Vec<&&Node> = rows.iter().filter(|r| attr(r, "type").to_ascii_lowercase().contains("deposit")).collect();
    // Each one when the query sends them, else its summaries.
    let detailed = deposits.iter().any(|r| !summary(r));
    deposits
        .into_iter()
        .filter(|r| !detailed || !summary(r))
        .filter_map(|r| {
            let date = ["dateTime", "settleDate", "reportDate"].iter().find_map(|k| day(attr(r, k), day_first))?;
            let amount = number(r, "amount").filter(|a| !a.is_zero())?;
            Some(Flow {
                id: clean(attr(r, "transactionID")),
                date,
                currency: code(attr(r, "currency")),
                amount,
                fx: number(r, "fxRateToBase").filter(|f| !f.is_zero()),
                description: clean(attr(r, "description")),
                transfer: false,
                counter: String::new(),
            })
        })
        .collect()
}

/// Cash and holdings moved from or to another account (the Transfers section), each at what it
/// was worth: they change the account's value without being a gain or a loss.
fn transfers(rows: &[&Node], base: &str, day_first: bool) -> Vec<Flow> {
    let detailed = rows.iter().any(|r| !summary(r));
    rows.iter()
        .filter(|r| !detailed || !summary(r))
        .filter_map(|r| {
            let date = ["date", "dateTime", "settleDate", "reportDate"].iter().find_map(|k| day(attr(r, k), day_first))?;
            let position = number(r, "positionAmount").unwrap_or_default();
            let cash = number(r, "cashTransfer").unwrap_or_default();
            // In the row's own currency when it says; else only the base-currency figure.
            let (worth, currency, fx) = if position.is_zero() && cash.is_zero() {
                (number(r, "positionAmountInBase")?, base.to_owned(), Some(Dec::ONE))
            } else {
                (position.abs().plus(cash.abs()), code(attr(r, "currency")), number(r, "fxRateToBase").filter(|f| !f.is_zero()))
            };
            // The direction says which way it went; without one, the amounts' own sign does.
            let out = match attr(r, "direction").to_ascii_uppercase().as_str() {
                "IN" => false,
                "OUT" => true,
                _ => position.plus(cash).is_negative() || (position.is_zero() && cash.is_zero() && worth.is_negative()),
            };
            let amount = if out { worth.abs().negated() } else { worth.abs() };
            let what = [attr(r, "symbol"), attr(r, "description")].iter().map(|s| clean(s)).find(|s| !s.is_empty() && s != "--").unwrap_or_default();
            (!amount.is_zero() && !currency.is_empty()).then(|| Flow {
                id: clean(attr(r, "transactionID")),
                date,
                currency,
                amount,
                fx,
                description: if what.is_empty() { "Transfer".to_owned() } else { format!("Transfer: {what}") },
                transfer: true,
                // Between two IBKR accounts the row names the other one. An account number at
                // another firm (an ACATS transfer) isn't kept.
                counter: Some(clean(attr(r, "account")))
                    .filter(|other| attr(r, "type").eq_ignore_ascii_case("INTERNAL") && !other.is_empty() && *other != clean(attr(r, "accountId")))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn income(rows: &[&Node], day_first: bool) -> Vec<Income> {
    let others: Vec<&&Node> = rows.iter().filter(|r| !attr(r, "type").to_ascii_lowercase().contains("deposit")).collect();
    let detailed = others.iter().any(|r| !summary(r));
    others
        .into_iter()
        .filter(|r| !detailed || !summary(r))
        .filter_map(|r| {
            let kind = attr(r, "type").to_ascii_lowercase();
            // "Withholding Tax" before "Dividends": tax on a dividend names both.
            let kind = if kind.contains("withholding") {
                IncomeKind::Tax
            } else if kind.contains("dividend") {
                IncomeKind::Dividends
            } else if kind.contains("interest") {
                IncomeKind::Interest
            } else if kind.contains("fee") || kind.contains("commission") {
                IncomeKind::Fees
            } else {
                IncomeKind::Other
            };
            Some(Income {
                date: ["dateTime", "settleDate", "reportDate"].iter().find_map(|k| day(attr(r, k), day_first))?,
                kind,
                currency: code(attr(r, "currency")),
                amount: number(r, "amount").filter(|a| !a.is_zero())?,
                fx: number(r, "fxRateToBase").filter(|f| !f.is_zero()),
                security: Security {
                    symbol: Some(clean(attr(r, "symbol"))).filter(|s| s != "--").unwrap_or_default(),
                    conid: clean(attr(r, "conid")),
                    isin: clean(attr(r, "isin")),
                    description: clean(attr(r, "description")),
                },
            })
        })
        .collect()
}

/// The trades of the Trades section, each once. A query can send the executions, the orders
/// they filled, the lots they closed and summaries by symbol, all of the same trades: the
/// executions are counted where there are any, else the orders, else rows that give no
/// level. Closed lots and summaries never are. A row without a date isn't a trade that can
/// be placed in a month, and a figure that doesn't read is left unknown.
fn trades(section: &Node, day_first: bool) -> Vec<Trade> {
    // The level a row says it is; else what its element is called.
    let level = |r: &Node| match (attr(r, "levelOfDetail").to_ascii_uppercase(), r.name.as_str()) {
        (said, _) if !said.is_empty() => said,
        (_, "Trade") => String::new(),
        (_, "Order") => "ORDER".to_owned(),
        (_, other) => other.to_ascii_uppercase(),
    };
    let rows: Vec<(&Node, String)> = section.children.iter().map(|r| (r, level(r))).collect();
    let Some(counted) = ["EXECUTION", "ORDER", ""].into_iter().find(|wanted| rows.iter().any(|(_, level)| level.as_str() == *wanted)) else { return vec![] };
    rows.iter()
        .filter(|(_, level)| level.as_str() == counted)
        .filter_map(|(r, _)| {
            Some(Trade {
                id: clean(attr(r, "transactionID")),
                date: ["tradeDate", "dateTime", "reportDate"].iter().find_map(|k| day(attr(r, k), day_first))?,
                currency: code(attr(r, "currency")),
                fx: number(r, "fxRateToBase").filter(|f| !f.is_zero()),
                realised: number(r, "fifoPnlRealized"),
                commission: number(r, "ibCommission"),
                commission_currency: code(attr(r, "ibCommissionCurrency")),
            })
        })
        .collect()
}

fn account(s: &Node, day_first: bool) -> Account {
    let mut a = Account {
        id: clean(attr(s, "accountId")),
        from: day(attr(s, "fromDate"), day_first).unwrap_or_default(),
        to: day(attr(s, "toDate"), day_first).unwrap_or_default(),
        generated: made(attr(s, "whenGenerated"), day_first),
        ..Account::default()
    };
    match s.child("AccountInformation") {
        Some(info) => {
            a.base = code(attr(info, "currency"));
            a.alias = clean(attr(info, "acctAlias"));
            if a.id.is_empty() {
                a.id = clean(attr(info, "accountId"));
            }
        }
        None => a.missing.push("Account Information"),
    }
    match section(s, "OpenPositions", "OpenPosition") {
        Some(rows) => a.positions = positions(&rows),
        None => a.missing.push("Open Positions"),
    }
    match section(s, "CashReport", "CashReportCurrency") {
        Some(rows) => {
            for r in rows {
                let Some(amount) = number(r, "endingCash") else { continue };
                let currency = attr(r, "currency");
                if currency.eq_ignore_ascii_case("BASE_SUMMARY") || attr(r, "levelOfDetail").eq_ignore_ascii_case("BaseCurrency") {
                    a.cash_base = Some(amount);
                    a.commissions = number(r, "commissions");
                } else if !code(currency).is_empty() && !amount.is_zero() {
                    a.cash.push(Cash { currency: code(currency), amount });
                }
            }
        }
        None => a.missing.push("Cash Report"),
    }
    match section(s, "EquitySummaryInBase", "EquitySummaryByReportDateInBase") {
        Some(rows) => {
            for r in rows {
                if let (Some(date), Some(total)) = (day(attr(r, "reportDate"), day_first), number(r, "total")) {
                    a.values.retain(|(d, _)| *d != date);
                    a.values.push((date, total));
                }
            }
            a.values.sort();
        }
        None => a.missing.push("Net Asset Value (NAV) in Base"),
    }
    match section(s, "CashTransactions", "CashTransaction") {
        Some(rows) => {
            a.flows = flows(&rows, day_first);
            a.income = income(&rows, day_first);
        }
        None => a.missing.push("Cash Transactions"),
    }
    match section(s, "Transfers", "Transfer") {
        Some(rows) => {
            for moved in transfers(&rows, &a.base, day_first) {
                // A cash transfer some statements also list as a deposit is counted once, and
                // keeps what the transfer says of where it went.
                match a.flows.iter_mut().find(|f| !f.transfer && f.date == moved.date && f.currency == moved.currency && f.amount == moved.amount) {
                    Some(listed) => {
                        listed.transfer = true;
                        listed.counter = moved.counter;
                    }
                    None => a.flows.push(moved),
                }
            }
        }
        None => a.missing.push("Transfers"),
    }
    a.trades = s.child("Trades").map(|section| trades(section, day_first));
    if a.trades.is_none() {
        a.missing.push("Trades");
    }
    for r in section(s, "ConversionRates", "ConversionRate").unwrap_or_default() {
        let (from, to) = (code(attr(r, "fromCurrency")), code(attr(r, "toCurrency")));
        if let (Some(date), Some(rate), false, false) = (day(attr(r, "reportDate"), day_first), number(r, "rate"), from.is_empty(), to.is_empty()) {
            if !rate.is_zero() {
                a.rates.push(Rate { date, from, to, rate });
            }
        }
    }
    if a.base.is_empty() {
        // Every rate converts into the base currency, and a holding in it converts at exactly 1.
        a.base = a.rates.first().map(|r| r.to.clone())
            .or_else(|| a.positions.iter().find(|p| p.fx == Some(Dec::ONE) && !p.currency.is_empty()).map(|p| p.currency.clone()))
            .or_else(|| (a.cash.len() == 1).then(|| a.cash[0].currency.clone()))
            .unwrap_or_default();
    }
    if a.to.is_empty() {
        a.to = a.values.last().map(|(d, _)| d.clone()).unwrap_or_default();
    }
    a
}

/// Reads a Flex statement. A statement broken out by day comes as one statement per day: those
/// join into one account, with the holdings of the last day and the values of every day.
pub fn parse(content: &str) -> Result<Statement, String> {
    let content = content.trim_start_matches('\u{feff}');
    let not_flex = || t!("This isn't an Interactive Brokers Flex statement in XML. In the Flex Query's Delivery Configuration, set Format to XML, then try again.");
    if !content.trim_start().starts_with('<') {
        return Err(not_flex());
    }
    let root = xml(content).map_err(|_| not_flex())?;
    let mut statements = vec![];
    root.find_all("FlexStatement", &mut statements);
    if statements.is_empty() {
        return Err(not_flex());
    }
    let day_first = day_first(&root);
    let mut accounts: Vec<Account> = vec![];
    for s in statements {
        let next = account(s, day_first);
        if next.id.is_empty() {
            continue;
        }
        match accounts.iter_mut().find(|a| a.id == next.id) {
            Some(known) => {
                let newer = next.to >= known.to;
                let mut values = std::mem::take(&mut known.values);
                values.retain(|(d, _)| !next.values.iter().any(|(n, _)| n == d));
                values.extend(next.values.iter().cloned());
                values.sort();
                let mut flows = std::mem::take(&mut known.flows);
                for f in &next.flows {
                    if !flows.contains(f) {
                        flows.push(f.clone());
                    }
                }
                let mut income = std::mem::take(&mut known.income);
                for i in &next.income {
                    if !income.contains(i) {
                        income.push(i.clone());
                    }
                }
                // Each day's statement carries that day's commissions.
                let commissions = match (known.commissions, next.commissions) {
                    (Some(a), Some(b)) => Some(a.plus(b)),
                    (a, b) => a.or(b),
                };
                // Each day's statement lists that day's trades; one with an id is never taken twice.
                let trades = match (known.trades.take(), &next.trades) {
                    (Some(mut all), Some(more)) => {
                        for t in more {
                            if t.id.is_empty() || !all.contains(t) {
                                all.push(t.clone());
                            }
                        }
                        Some(all)
                    }
                    (all, more) => all.or_else(|| more.clone()),
                };
                let mut rates = std::mem::take(&mut known.rates);
                rates.extend(next.rates.iter().cloned());
                let from = if known.from.is_empty() || (!next.from.is_empty() && next.from < known.from) { next.from.clone() } else { known.from.clone() };
                let mut missing = known.missing.clone();
                missing.retain(|m| next.missing.contains(m));
                if newer {
                    *known = next;
                }
                known.values = values;
                known.flows = flows;
                known.income = income;
                known.commissions = commissions;
                known.trades = trades;
                known.rates = rates;
                known.from = from;
                known.missing = missing;
            }
            None => accounts.push(next),
        }
    }
    if accounts.is_empty() {
        return Err(t!("This Flex statement doesn't name an account. Add your account under Accounts in the Flex Query, then try again."));
    }
    Ok(Statement { accounts })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) use crate::test_support::FLEX_SAMPLE as SAMPLE;

    #[test]
    fn decimals_keep_eight_places_and_round_money_half_away_from_zero() {
        let d = |s: &str| Dec::parse(s).unwrap();
        assert_eq!(d("1,234.50").text(), "1234.5");
        assert_eq!(d("-0.125").text(), "-0.125");
        assert_eq!(d("0.123456789").text(), "0.12345679");
        assert_eq!(d("1.2E-4").text(), "0.00012");
        assert_eq!(d("25").text(), "25");
        assert_eq!(d("10.005").cents(), 1001);
        assert_eq!(d("-10.005").cents(), -1001);
        assert_eq!(d("10800").times(d("0.26666667")).cents(), 288000);
        assert_eq!(d("1").over(d("0.26666667")).unwrap().text(), "3.74999995");
        assert!(d("1").over(Dec::ZERO).is_none());
        for bad in ["", "abc", "1.2.3", "--1", "1e400"] {
            assert!(Dec::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn dates_read_in_every_format_a_query_can_write() {
        for (text, day_first) in [
            ("20261113", false), ("20261113;202000", false), ("20261113202000", false), ("2026-11-13", false),
            ("2026-11-13 20:20:00", false), ("11/13/2026", false), ("11/13/26", false), ("13/11/2026", true),
            ("13/11/26;20:20", true), ("13-Nov-26", false), ("13-Nov-2026", false),
        ] {
            assert_eq!(day(text, day_first).as_deref(), Some("2026-11-13"), "{text}");
        }
        assert!(day("", false).is_none());
        assert!(day("20261341", false).is_none());
    }

    #[test]
    fn reads_what_the_account_holds_and_is_worth() {
        let s = parse(SAMPLE).unwrap();
        assert_eq!(s.accounts.len(), 1);
        let a = &s.accounts[0];
        assert_eq!((a.id.as_str(), a.alias.as_str(), a.base.as_str()), ("U1234567", "Long term", "USD"));
        assert_eq!((a.from.as_str(), a.to.as_str()), ("2026-09-01", "2026-11-13"));
        assert_eq!(a.generated, "2026-11-14 06:30");
        assert_eq!([made("2026-11-14 18:05:09", false), made("20261114", false), made("", false)], ["2026-11-14 18:05", "2026-11-14", ""]);
        assert_eq!(a.missing, vec!["Trades"]);
        // The lot row repeats the whole position, so it isn't counted twice.
        let held: Vec<(&str, String, i64)> = a.positions.iter().map(|p| (p.symbol.as_str(), p.quantity.text(), p.value.cents())).collect();
        assert_eq!(held, vec![("AAPL", "25".to_string(), 580000), ("VWRA", "120.5".to_string(), 1710000), ("2222", "400".to_string(), 1080000)]);
        assert_eq!(a.positions[2].exchange, "TADAWUL");
        assert_eq!(a.positions[2].gain.unwrap().cents(), -120000);
        assert_eq!(a.cash, vec![Cash { currency: "USD".into(), amount: Dec::parse("820").unwrap() }, Cash { currency: "SAR".into(), amount: Dec::parse("1500").unwrap() }]);
        assert_eq!(a.cash_base.unwrap().cents(), 122000);
        assert_eq!(a.values.len(), 4);
        assert_eq!(a.values.last().unwrap(), &("2026-11-13".to_string(), Dec::parse("27012.5").unwrap()));
        // Deposits only: the dividend is the account's own return.
        let flows: Vec<(&str, &str, i64)> = a.flows.iter().map(|f| (f.id.as_str(), f.date.as_str(), f.amount.cents())).collect();
        assert_eq!(flows, vec![("9000001", "2026-10-05", 100000), ("9000002", "2026-11-02", 150000)]);
        // What it earned and was charged, each by what it was for, and the period's commissions.
        let income: Vec<(IncomeKind, &str, i64)> = a.income.iter().map(|i| (i.kind, i.date.as_str(), i.amount.cents())).collect();
        assert_eq!(income, vec![
            (IncomeKind::Dividends, "2026-11-06", 650),
            (IncomeKind::Tax, "2026-11-06", -98),
            (IncomeKind::Interest, "2026-10-04", 120),
            (IncomeKind::Fees, "2026-10-03", -1000),
        ]);
        assert_eq!(a.commissions.unwrap().cents(), -350);
        assert_eq!(a.rates.len(), 4);
        // The holder's name is in the file and is never read.
        assert!(!format!("{a:?}").contains("Someone"));
    }

    #[test]
    fn lots_add_up_when_the_query_sends_no_whole_positions() {
        let lots = SAMPLE
            .lines()
            .filter(|l| !(l.contains("<OpenPosition ") && l.contains(r#"levelOfDetail="SUMMARY""#)))
            .collect::<Vec<_>>()
            .join("\n")
            .replace(
                r#"levelOfDetail="LOT" openDateTime="20250110;093000" />"#,
                r#"levelOfDetail="LOT" openDateTime="20250110;093000" />
<OpenPosition accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="STK" symbol="AAPL" description="APPLE INC" conid="265598" multiplier="1" position="5" markPrice="232" positionValue="1160" costBasisMoney="1000" fifoPnlUnrealized="160" levelOfDetail="LOT" />"#,
            );
        let a = &parse(&lots).unwrap().accounts[0];
        assert_eq!(a.positions.len(), 1);
        let p = &a.positions[0];
        assert_eq!((p.quantity.text().as_str(), p.value.cents(), p.cost.unwrap().cents(), p.gain.unwrap().cents()), ("30", 696000, 550000, 146000));
        // What they are worth in their own currency is added up too, with the decimals written.
        assert_eq!(p.amount, "6960");
        let finer = lots.replace(r#"positionValue="1160""#, r#"positionValue="1160.25""#);
        assert_eq!(parse(&finer).unwrap().accounts[0].positions[0].amount, "6960.25");
    }

    #[test]
    fn a_positions_finer_category_side_and_own_currency_value_are_kept_as_written() {
        let finer = SAMPLE
            .replace(r#"symbol="VWRA" description="VANG FTSE AW USDA""#, r#"symbol="VWRA" description="VANG FTSE AW USDA" subCategory=" etf ""#)
            .replace(r#"position="400" markPrice="27" positionValue="10800""#, r#"position="400" markPrice="27" positionValue="10800.00""#)
            .replace(
                "</OpenPositions>",
                r#"<OpenPosition accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="OPT" symbol="SPY 261218P00500000" description="SPY 18DEC26 500 P" conid="700123456" putCall="p" multiplier="100" position="-1" markPrice="4.255" levelOfDetail="SUMMARY" />
</OpenPositions>"#,
            );
        let a = &parse(&finer).unwrap().accounts[0];
        let rows: Vec<(&str, &str, &str, &str, i64)> = a.positions.iter().map(|p| (p.symbol.as_str(), p.sub_kind.as_str(), p.put_call.as_str(), p.amount.as_str(), p.value.cents())).collect();
        assert_eq!(rows, vec![
            ("AAPL", "", "", "5800", 580_000),
            ("VWRA", "etf", "", "17100", 1_710_000),
            // In riyals, as written: two decimals stay two.
            ("2222", "", "", "10800.00", 1_080_000),
            // No value written: quantity times price times multiplier, to the cent.
            ("SPY 261218P00500000", "", "P", "-425.50", -42_550),
        ]);
        // A statement without them reads as before.
        let plain_sample = &parse(SAMPLE).unwrap().accounts[0];
        assert!(plain_sample.positions.iter().all(|p| p.sub_kind.is_empty() && p.put_call.is_empty()));
        // Written decimals, plainly, and added up without floating point.
        assert_eq!((plain("1,234.50"), plain("+7"), plain("0.1"), plain("abc")), (Some("1234.50".into()), Some("7".into()), Some("0.1".into()), None));
        assert_eq!((add_plain("0.1", "0.2").as_deref(), add_plain("100", "0.25").as_deref(), add_plain("-500.00", "500").as_deref(), add_plain("x", "1")), (Some("0.3"), Some("100.25"), Some("0.00"), None));
        assert_eq!((Dec::parse("-0.004").unwrap().fixed(2), Dec::parse("2.5").unwrap().fixed(0), Dec::parse("1.005").unwrap().fixed(2)), ("0.00".to_string(), "3".to_string(), "1.01".to_string()));
    }

    #[test]
    fn a_rate_the_query_leaves_out_is_unknown_not_one() {
        let narrow = SAMPLE.replace(r#" fxRateToBase="0.26666667""#, "").replace(r#" fxRateToBase="1""#, "");
        let a = &parse(&narrow).unwrap().accounts[0];
        assert!(a.positions.iter().all(|p| p.fx.is_none()));
        assert!(a.flows.iter().all(|f| f.fx.is_none()));
        // The account's currency still comes from Account Information.
        assert_eq!(a.base, "USD");
    }

    #[test]
    fn transfers_from_and_to_other_accounts_are_money_moved_at_what_it_was_worth() {
        let moved = SAMPLE.replace("<Transfers />", r#"<Transfers>
<Transfer accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="STK" symbol="AAPL" description="APPLE INC" date="20261009" type="INTERNAL" direction="IN" quantity="10" transferPrice="230" positionAmount="2300" positionAmountInBase="2300" cashTransfer="0" transactionID="7000001" levelOfDetail="DETAIL" />
<Transfer accountId="U1234567" currency="SAR" fxRateToBase="0.26666667" assetCategory="CASH" symbol="--" description="" date="20261020" type="INTERNAL" direction="OUT" account="U7654321" accountName="A Name" quantity="0" positionAmount="0" cashTransfer="-3750" transactionID="7000002" levelOfDetail="DETAIL" />
<Transfer accountId="U1234567" currency="USD" assetCategory="STK" symbol="VWRA" date="20261021" type="ACATS" direction="IN" account="998877" positionAmountInBase="1419" transactionID="7000003" levelOfDetail="DETAIL" />
<Transfer accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="CASH" symbol="--" date="20261005" type="INTERNAL" direction="IN" account="U7654321" positionAmount="0" cashTransfer="1000" levelOfDetail="DETAIL" />
</Transfers>"#);
        let a = &parse(&moved).unwrap().accounts[0];
        assert_eq!(a.missing, vec!["Trades"]);
        let flows: Vec<(&str, &str, &str, i64)> = a.flows.iter().map(|f| (f.date.as_str(), f.currency.as_str(), f.description.as_str(), f.amount.cents())).collect();
        assert_eq!(flows, vec![
            ("2026-10-05", "USD", "CASH RECEIPTS / ELECTRONIC FUND TRANSFERS", 100_000),
            ("2026-11-02", "SAR", "CASH RECEIPTS / ELECTRONIC FUND TRANSFERS", 150_000),
            // Shares in at what they were worth, cash out, and one the statement only values in dollars.
            ("2026-10-09", "USD", "Transfer: AAPL", 230_000),
            ("2026-10-20", "SAR", "Transfer", -375_000),
            ("2026-10-21", "USD", "Transfer: VWRA", 141_900),
            // The 1,000 that is also listed as a deposit on the same day isn't counted twice.
        ]);
        // A transfer between two of the user's accounts there names the other one; an account
        // at another firm isn't kept, and neither is anyone's name.
        let others: Vec<(bool, &str)> = a.flows.iter().map(|f| (f.transfer, f.counter.as_str())).collect();
        assert_eq!(others, vec![(true, "U7654321"), (false, ""), (true, ""), (true, "U7654321"), (true, "")]);
        assert!(!format!("{a:?}").contains("A Name") && !format!("{a:?}").contains("998877"));
    }

    /// The sample with a Trades section the way a query can send one: two executions of one
    /// order with the order, the lot they closed and the symbol's summary; a purchase in
    /// riyals with its order; a row whose figures don't read; and one with no date. Its cash
    /// transactions gain a dividend and its tax on the Saudi stock, named by the broker's id,
    /// and a payment in lieu on a stock no longer held.
    pub(crate) fn with_trades() -> String {
        let sold = |level: &str, id: &str, quantity: &str, commission: &str, realised: &str| format!(
            r#"<Trade accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="STK" symbol="AAPL" description="APPLE INC" conid="265598" isin="US0378331005" tradeDate="20261008" dateTime="20261008;103000" buySell="SELL" quantity="{quantity}" tradePrice="230" proceeds="1150" ibCommission="{commission}" ibCommissionCurrency="USD" fifoPnlRealized="{realised}" levelOfDetail="{level}" transactionID="{id}" />"#
        );
        let bought = |element: &str, level: &str, id: &str| format!(
            r#"<{element} accountId="U1234567" currency="SAR" fxRateToBase="0.26666667" assetCategory="STK" symbol="2222" description="SAUDI ARABIAN OIL CO" conid="400000001" tradeDate="20261103" dateTime="20261103;111500" buySell="BUY" quantity="100" tradePrice="27" proceeds="-2700" ibCommission="-7.5" ibCommissionCurrency="SAR" fifoPnlRealized="0" {level} transactionID="{id}" />"#
        );
        let trades = [
            sold("EXECUTION", "8000001", "-5", "-1.25", "248.75"),
            sold("EXECUTION", "8000002", "-5", "-1.25", "250"),
            sold("ORDER", "", "-10", "-2.5", "498.75"),
            r#"<Lot accountId="U1234567" currency="USD" fxRateToBase="1" symbol="AAPL" tradeDate="20261008" quantity="-10" fifoPnlRealized="498.75" levelOfDetail="CLOSED_LOT" />"#.to_owned(),
            r#"<SymbolSummary accountId="U1234567" currency="USD" fxRateToBase="1" symbol="AAPL" tradeDate="20261008" quantity="-10" ibCommission="-2.5" fifoPnlRealized="498.75" levelOfDetail="SYMBOL_SUMMARY" />"#.to_owned(),
            bought("Trade", r#"levelOfDetail="EXECUTION""#, "8000003"),
            bought("Order", "", ""),
            r#"<Trade accountId="U1234567" currency="USD" fxRateToBase="1" symbol="VWRA" tradeDate="20260915" quantity="1" ibCommission="n/a" fifoPnlRealized="" levelOfDetail="EXECUTION" transactionID="8000004" />"#.to_owned(),
            r#"<Trade accountId="U1234567" currency="USD" fxRateToBase="1" symbol="VWRA" quantity="1" ibCommission="-1" fifoPnlRealized="5" levelOfDetail="EXECUTION" transactionID="8000005" />"#.to_owned(),
        ];
        let paid = [
            r#"<CashTransaction accountId="U1234567" currency="SAR" fxRateToBase="0.26666667" symbol="2222" conid="400000001" description="2222 CASH DIVIDEND SAR 0.25 PER SHARE" dateTime="20261020;202000" amount="100" type="Dividends" transactionID="9000007" levelOfDetail="DETAIL" />"#,
            r#"<CashTransaction accountId="U1234567" currency="SAR" fxRateToBase="0.26666667" symbol="2222" conid="400000001" description="2222 CASH DIVIDEND SAR 0.25 PER SHARE - SA TAX" dateTime="20261020;202000" amount="-5" type="Withholding Tax" transactionID="9000008" levelOfDetail="DETAIL" />"#,
            r#"<CashTransaction accountId="U1234567" currency="USD" fxRateToBase="1" symbol="MSFT" isin="US5949181045" description="MSFT(US5949181045) PAYMENT IN LIEU OF DIVIDEND" dateTime="20261022;202000" amount="3" type="Payment In Lieu Of Dividends" transactionID="9000009" levelOfDetail="DETAIL" />"#,
        ];
        SAMPLE
            .replace("<Transfers />", &format!("<Trades>\n{}\n</Trades>\n<Transfers />", trades.join("\n")))
            .replace("</CashTransactions>", &format!("{}\n</CashTransactions>", paid.join("\n")))
    }

    #[test]
    fn each_trade_is_read_once_whatever_levels_the_query_sends() {
        let rows = |a: &Account| a.trades.as_ref().unwrap().iter().map(|t| (t.id.clone(), t.date.clone(), t.currency.clone(), t.realised.map(Dec::cents), t.commission.map(Dec::cents))).collect::<Vec<_>>();
        let row = |id: &str, date: &str, currency: &str, realised: Option<i64>, commission: Option<i64>| (id.to_string(), date.to_string(), currency.to_string(), realised, commission);
        let all = with_trades();
        let a = &parse(&all).unwrap().accounts[0];
        // The executions, and nothing of the order, lot and summary that repeat them. Figures
        // that don't read are unknown, and a row with no date is no trade to place.
        assert_eq!(rows(a), vec![
            row("8000001", "2026-10-08", "USD", Some(24_875), Some(-125)),
            row("8000002", "2026-10-08", "USD", Some(25_000), Some(-125)),
            row("8000003", "2026-11-03", "SAR", Some(0), Some(-750)),
            row("8000004", "2026-09-15", "USD", None, None),
        ]);
        assert_eq!((a.trades.as_ref().unwrap()[2].fx.map(Dec::text).as_deref(), a.trades.as_ref().unwrap()[2].commission_currency.as_str()), (Some("0.26666667"), "SAR"));
        assert!(a.missing.is_empty());
        // A statement without the section says so, as it does for the others.
        let plain = &parse(SAMPLE).unwrap().accounts[0];
        assert!(plain.trades.is_none() && plain.missing == vec!["Trades"]);
        // Without executions the orders are the trades; without either, rows that give no level.
        let orders = all.lines().filter(|l| !l.contains(r#"levelOfDetail="EXECUTION""#)).collect::<Vec<_>>().join("\n");
        assert_eq!(rows(&parse(&orders).unwrap().accounts[0]), vec![row("", "2026-10-08", "USD", Some(49_875), Some(-250)), row("", "2026-11-03", "SAR", Some(0), Some(-750))]);
        let bare = orders.lines().filter(|l| !l.contains(r#"levelOfDetail="ORDER""#) && !l.starts_with("<Order ")).collect::<Vec<_>>().join("\n").replace(
            "<Trades>",
            r#"<Trades>
<Trade currency="USD" symbol="AAPL" dateTime="2026-10-08 10:30:00" ibCommission="-2.5" fifoPnlRealized="498.75" />"#,
        );
        assert_eq!(rows(&parse(&bare).unwrap().accounts[0]), vec![row("", "2026-10-08", "USD", Some(49_875), Some(-250))]);
        // A section with nothing in it is still a statement that lists its trades.
        assert_eq!(parse(&SAMPLE.replace("<Transfers />", "<Trades />\n<Transfers />")).unwrap().accounts[0].trades, Some(vec![]));
        // A dividend keeps the holding its row names; a deposit names none.
        let named: Vec<(&str, &str, &str, bool)> = a.income.iter().map(|i| (i.security.symbol.as_str(), i.security.conid.as_str(), i.security.isin.as_str(), i.security.named())).collect();
        assert_eq!(named, vec![("AAPL", "", "", true), ("AAPL", "", "", true), ("", "", "", false), ("", "", "", false), ("2222", "400000001", "", true), ("2222", "400000001", "", true), ("MSFT", "", "US5949181045", true)]);
        assert_eq!(a.income[6].kind, IncomeKind::Dividends);
        // A statement a day keeps every day's trades, each once.
        let day = |date: &str, id: &str| format!(
            r#"<FlexStatement accountId="U9" fromDate="{date}" toDate="{date}"><AccountInformation currency="USD"/>
<Trades><Trade currency="USD" symbol="SPY" tradeDate="{date}" ibCommission="-1" fifoPnlRealized="10" levelOfDetail="EXECUTION" transactionID="{id}"/></Trades></FlexStatement>"#
        );
        let doc = format!("<FlexQueryResponse><FlexStatements>{}{}{}</FlexStatements></FlexQueryResponse>", day("20261112", "1"), day("20261113", "2"), day("20261113", "2"));
        assert_eq!(rows(&parse(&doc).unwrap().accounts[0]).len(), 2);
    }

    #[test]
    fn says_which_sections_the_query_left_out() {
        let bare = r#"<FlexQueryResponse><FlexStatements><FlexStatement accountId="U7" fromDate="13/11/2026" toDate="13/11/2026">
<OpenPositions><OpenPosition currency="EUR" fxRateToBase="1" symbol="SAP" position="2" markPrice="200" levelOfDetail="SUMMARY"/></OpenPositions>
</FlexStatement></FlexStatements></FlexQueryResponse>"#;
        let a = &parse(bare).unwrap().accounts[0];
        assert_eq!(a.missing, vec!["Account Information", "Cash Report", "Net Asset Value (NAV) in Base", "Cash Transactions", "Transfers", "Trades"]);
        // No Account Information: a holding that converts at exactly 1 is in the base currency.
        assert_eq!(a.base, "EUR");
        assert_eq!(a.to, "2026-11-13");
        // No positionValue either: quantity times price.
        assert_eq!(a.positions[0].value.cents(), 40000);
    }

    #[test]
    fn a_statement_broken_out_by_day_reads_as_one_account() {
        let day = |date: &str, total: &str, shares: &str| format!(
            r#"<FlexStatement accountId="U9" fromDate="{date}" toDate="{date}"><AccountInformation currency="USD"/>
<EquitySummaryInBase><EquitySummaryByReportDateInBase reportDate="{date}" total="{total}"/></EquitySummaryInBase>
<OpenPositions><OpenPosition currency="USD" symbol="SPY" conid="1" position="{shares}" markPrice="500" positionValue="{total}" levelOfDetail="SUMMARY"/></OpenPositions>
<CashReport/><CashTransactions><CashTransaction type="Deposits/Withdrawals" currency="USD" amount="500" transactionID="1" dateTime="20261110"/></CashTransactions><Transfers/></FlexStatement>"#
        );
        let doc = format!("<FlexQueryResponse><FlexStatements>{}{}</FlexStatements></FlexQueryResponse>", day("20261112", "1000", "2"), day("20261113", "1500", "3"));
        let s = parse(&doc).unwrap();
        assert_eq!(s.accounts.len(), 1);
        let a = &s.accounts[0];
        assert_eq!((a.from.as_str(), a.to.as_str()), ("2026-11-12", "2026-11-13"));
        assert_eq!(a.values.iter().map(|(d, v)| (d.as_str(), v.cents())).collect::<Vec<_>>(), vec![("2026-11-12", 100000), ("2026-11-13", 150000)]);
        assert_eq!(a.positions[0].quantity.text(), "3");
        assert_eq!(a.flows.len(), 1);
        assert_eq!(a.missing, vec!["Trades"]);
    }

    #[test]
    fn refuses_what_isnt_a_flex_statement() {
        for other in ["", "ClientAccountID,Symbol\nU1,AAPL", "<html><body>Sign in</body></html>", "<FlexStatementResponse><Status>Fail</Status></FlexStatementResponse>"] {
            assert!(parse(other).unwrap_err().contains("Flex"), "{other}");
        }
        assert!(sniff(SAMPLE));
        assert!(!sniff("<OFX>"));
    }
}
