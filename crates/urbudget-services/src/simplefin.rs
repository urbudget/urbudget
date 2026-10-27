//! Bank sync through SimpleFIN Bridge (https://www.simplefin.org/protocol.html), a service the user
//! pays for that reads US and Canadian banks. The bank login stays with SimpleFIN: the user pastes a
//! setup token, which is claimed once for an Access URL. That URL is a password, so it lives with
//! the app's other secrets (vault.rs) and never in the budget, which encrypted sync uploads. A
//! budget account is linked to one SimpleFIN account in its extra (`simplefin`), with the date it
//! was first synced from and the date it was last synced.
//!
//! SimpleFIN allows about 24 requests a day, so the app asks only when the user presses Sync, and
//! one request brings every account (a long first sync takes one per 45 days). The transactions
//! go to the same review as a bank file: nothing reaches the budget until the user imports it,
//! and one that comes again is recognised by its SimpleFIN id.
use crate::t;
use crate::{
    bank_import::{self, BankTransaction, Row},
    domain::{stamp, Account, Budget},
    patch::Patch,
    vault, Database,
};
use base64::Engine;
use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};


const SERVICE: &str = "com.localbudget.desktop.simplefin";
/// Where users make a setup token.
const CREATE: &str = "https://bridge.simplefin.org/simplefin/create";
/// Days per /accounts request. SimpleFIN allows 90, but its server warns above 45.
const WINDOW_DAYS: i64 = 45;
/// How far back a first sync can reach.
const HISTORY_DAYS: i64 = 365;
/// A sync starts this many days before the last one, for transactions that posted late.
const OVERLAP_DAYS: i64 = 7;
/// First syncs start this many days back unless the link says otherwise.
const DEFAULT_DAYS: i64 = 30;
/// Requests a day this app makes at most, under SimpleFIN's 24.
const DAILY_LIMIT: u32 = 20;

fn bad_token() -> String {
    t!("That isn't a SimpleFIN setup token. Copy the whole token from SimpleFIN Bridge and paste it again.")
}
fn token_used() -> String {
    t!("That setup token was already used. If you didn't use it yourself, someone else may have it: disable it at SimpleFIN Bridge, then make a new one.")
}
fn offline() -> String {
    t!("Couldn't reach SimpleFIN. Check your connection and try again.")
}
fn unreadable() -> String {
    t!("SimpleFIN sent an answer UrBudget couldn't read. Nothing was imported.")
}
fn not_connected() -> String {
    t!("Connect SimpleFIN first, in Settings › Import.")
}

/// The credential's name. A QA run (debug builds on a scratch data folder) keeps its own, so a
/// demo connection never replaces a real one.
fn service() -> String {
    if cfg!(debug_assertions) && std::env::var_os("LOCAL_BUDGET_DATA_DIR").is_some() {
        format!("{SERVICE}.qa")
    } else {
        SERVICE.into()
    }
}

// What SimpleFIN sends. Ids and amounts are strings in the protocol; numbers are read too.
#[derive(Clone, Default, Deserialize)]
pub struct AccountSet {
    #[serde(default)]
    errlist: Vec<Problem>,
    /// Version 1's messages, where the Bridge also puts quota warnings.
    #[serde(default)]
    errors: Vec<Value>,
    #[serde(default)]
    connections: Vec<Connection>,
    #[serde(default)]
    accounts: Vec<SfAccount>,
}
#[derive(Clone, Deserialize)]
struct Problem {
    #[serde(default)]
    msg: String,
    conn_id: Option<String>,
    account_id: Option<String>,
}
#[derive(Clone, Deserialize)]
struct Connection {
    conn_id: String,
    #[serde(default)]
    name: String,
    org_name: Option<String>,
}
#[derive(Clone, Deserialize)]
struct SfAccount {
    #[serde(deserialize_with = "text")]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    conn_id: String,
    #[serde(default)]
    currency: String,
    #[serde(default, deserialize_with = "text")]
    balance: String,
    #[serde(rename = "balance-date", default)]
    balance_date: i64,
    #[serde(default)]
    transactions: Vec<SfTransaction>,
}
#[derive(Clone, Deserialize)]
struct SfTransaction {
    #[serde(deserialize_with = "text")]
    id: String,
    #[serde(default)]
    posted: i64,
    #[serde(deserialize_with = "text")]
    amount: String,
    #[serde(default)]
    description: String,
    payee: Option<String>,
    memo: Option<String>,
    transacted_at: Option<i64>,
    #[serde(default)]
    pending: bool,
}
fn text<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

/// Text from SimpleFIN made safe to show and keep: no markup, control or direction-changing
/// characters, on one line and at most 300 characters.
pub fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' && chars.peek().is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!')) {
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
            }
            out.push(' ');
        } else if c.is_control()
            || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
        {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    let line = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 300 {
        line.chars().take(299).collect::<String>() + "…"
    } else {
        line
    }
}

/// A SimpleFIN amount ("-12.34", "1500", "+0.5") in cents. Digits past the cents are refused
/// unless they are zeros, so nothing is rounded.
fn cents(s: &str) -> Option<i64> {
    let s = s.trim();
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && frac.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || !frac.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let frac = frac.trim_end_matches('0');
    if frac.len() > 2 || whole.len() > 12 {
        return None;
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let n = whole * 100 + format!("{frac:0<2}").parse::<i64>().ok()?;
    (n <= 1_000_000_000_000).then_some(if negative { -n } else { n })
}

/// The calendar day of a moment, where the user is.
fn day<Tz: TimeZone>(epoch: i64, tz: &Tz) -> NaiveDate {
    DateTime::from_timestamp(epoch, 0).unwrap_or_default().with_timezone(tz).date_naive()
}
/// The moment a day starts, where the user is.
fn midnight<Tz: TimeZone>(d: NaiveDate, tz: &Tz) -> i64 {
    let start = d.and_hms_opt(0, 0, 0).unwrap();
    tz.from_local_datetime(&start).earliest().map_or(start.and_utc().timestamp(), |t| t.timestamp())
}

/// An account's link to SimpleFIN, as its extra keeps it.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Link {
    /// The SimpleFIN account.
    id: String,
    /// The first day a first sync brings in.
    #[serde(default)]
    from: String,
    last_sync: Option<String>,
    /// For a bank that sends a card's purchases as deposits.
    #[serde(default)]
    flip_signs: bool,
}
fn link(a: &Account) -> Option<Link> {
    serde_json::from_value(a.extra.get("simplefin")?.clone()).ok()
}
/// The first day this sync needs for an account: a week before its last sync, or the day it
/// was linked from.
fn since(link: &Link, today: NaiveDate) -> NaiveDate {
    let date = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let start = match link.last_sync.as_deref().and_then(date) {
        Some(last) => last - Duration::days(OVERLAP_DAYS),
        None => date(&link.from).unwrap_or(today - Duration::days(DEFAULT_DAYS)),
    };
    start.clamp(today - Duration::days(HISTORY_DAYS), today)
}
/// The requests (start and end, as epochs) that cover `start` to `end`, oldest first, none
/// longer than WINDOW_DAYS.
fn windows(start: i64, end: i64) -> Vec<(i64, i64)> {
    let span = WINDOW_DAYS * 86_400;
    let mut out = vec![];
    let mut to = end;
    while to > start {
        let from = (to - span).max(start);
        out.push((from, to));
        to = from;
    }
    out.reverse();
    out
}
/// What a sync asks for: the requests that cover every linked account since it needs.
fn plan<Tz: TimeZone>(b: &Budget, now: i64, tz: &Tz) -> Result<Vec<(i64, i64)>, String> {
    let today = day(now, tz);
    let start = b
        .accounts
        .iter()
        .filter(|a| !a.archived)
        .filter_map(link)
        .map(|l| since(&l, today))
        .min()
        .ok_or_else(|| t!("Link an account to SimpleFIN first, in Settings › Import."))?;
    Ok(windows(midnight(start, tz), now))
}

/// The Access URL, split into the address and its Basic credentials.
pub struct Access {
    base: Url,
    user: String,
    password: String,
}
/// HTTPS only, except to this computer in tests and debug builds (the fake server, a local proxy).
fn allowed(url: &Url) -> bool {
    url.scheme() == "https"
        || cfg!(any(test, debug_assertions))
            && url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
}
fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        let hex = |j: usize| s.get(j..j + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex(i + 1)) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
impl Access {
    pub fn parse(text: &str) -> Result<Access, String> {
        let bad = || t!("SimpleFIN sent an access address UrBudget can't use. Make a new setup token and connect again.");
        let mut base = Url::parse(text.trim()).map_err(|_| bad())?;
        if !allowed(&base) || base.username().is_empty() {
            return Err(bad());
        }
        let user = unescape(base.username());
        let password = unescape(base.password().unwrap_or_default());
        base.set_username("").map_err(|_| bad())?;
        base.set_password(None).map_err(|_| bad())?;
        Ok(Access { base, user, password })
    }
    fn accounts(&self) -> String {
        format!("{}/accounts", self.base.as_str().trim_end_matches('/'))
    }
}
/// The claim address inside a setup token (Base64 of a URL).
pub fn claim_url(token: &str) -> Result<Url, String> {
    let compact: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    let engines = [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ];
    let decoded = engines.iter().find_map(|e| e.decode(&compact).ok()).ok_or_else(bad_token)?;
    let url = Url::parse(String::from_utf8(decoded).map_err(|_| bad_token())?.trim()).map_err(|_| bad_token())?;
    if !allowed(&url) || url.host_str().is_none() {
        return Err(bad_token());
    }
    Ok(url)
}
pub fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // A redirect could take the credentials somewhere else.
        .redirect(reqwest::redirect::Policy::none())
        .https_only(!cfg!(any(test, debug_assertions)))
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|_| t!("Couldn't start bank sync."))
}
/// Trades a setup token's claim address for the Access URL. It works once.
pub async fn claim(http: &reqwest::Client, url: Url) -> Result<String, String> {
    let response = http
        .post(url)
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .send()
        .await
        .map_err(|_| offline())?;
    match response.status().as_u16() {
        200..=299 => {}
        403 => return Err(token_used()),
        n => return Err(t!("SimpleFIN couldn't accept the setup token (HTTP {n}). Try again later.", n)),
    }
    let access = response.text().await.map_err(|_| offline())?.trim().to_string();
    Access::parse(&access)?;
    Ok(access)
}
/// One /accounts request.
async fn fetch(http: &reqwest::Client, access: &Access, query: &[(&str, String)]) -> Result<AccountSet, String> {
    let response = http
        .get(access.accounts())
        .basic_auth(&access.user, Some(&access.password))
        .query(&[("version", "2")])
        .query(query)
        .send()
        .await
        .map_err(|_| offline())?;
    match response.status().as_u16() {
        200..=299 => response.json().await.map_err(|_| unreadable()),
        402 => Err(t!("SimpleFIN needs a payment before it sends more. Check your subscription at SimpleFIN Bridge, then sync again.")),
        403 => Err(t!("SimpleFIN turned down this connection. It may have been revoked. Remove it here, then connect again with a new setup token.")),
        n => Err(t!("SimpleFIN couldn't send your transactions (HTTP {n}). Try again later.", n)),
    }
}
/// Every account's transactions over the windows, as one answer: each transaction once, and the
/// balances from the newest window.
pub async fn download(http: &reqwest::Client, access: &Access, windows: &[(i64, i64)]) -> Result<AccountSet, String> {
    let mut all = AccountSet::default();
    for (start, end) in windows {
        let set = fetch(http, access, &[("start-date", start.to_string()), ("end-date", end.to_string())]).await?;
        for account in set.accounts {
            match all.accounts.iter_mut().find(|a| a.id == account.id) {
                Some(known) => {
                    let mut transactions = std::mem::take(&mut known.transactions);
                    for t in &account.transactions {
                        transactions.retain(|old| old.id != t.id);
                    }
                    transactions.extend(account.transactions.iter().cloned());
                    *known = SfAccount { transactions, ..account };
                }
                None => all.accounts.push(account),
            }
        }
        for c in set.connections {
            all.connections.retain(|known| known.conn_id != c.conn_id);
            all.connections.push(c);
        }
        all.errlist.extend(set.errlist);
        all.errors.extend(set.errors);
    }
    Ok(all)
}
/// SimpleFIN's messages for the user, each once, saying which bank or account it's about.
fn messages(set: &AccountSet) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let about = |p: &Problem| {
        p.account_id
            .as_ref()
            .and_then(|id| set.accounts.iter().find(|a| &a.id == id).map(|a| clean(&a.name)))
            .or_else(|| {
                let c = set.connections.iter().find(|c| Some(&c.conn_id) == p.conn_id.as_ref())?;
                Some(clean(c.org_name.as_deref().unwrap_or(&c.name)))
            })
            .filter(|s| !s.is_empty())
    };
    let errlist = set.errlist.iter().map(|p| match about(p) {
        Some(who) => format!("{who}: {}", clean(&p.msg)),
        None => clean(&p.msg),
    });
    let errors = set.errors.iter().map(|e| match e {
        Value::String(s) => clean(s),
        other => clean(&other.to_string()),
    });
    for m in errlist.chain(errors) {
        if !m.is_empty() && !out.contains(&m) && !out.iter().any(|o| o.ends_with(&format!(": {m}"))) {
            out.push(m);
        }
    }
    out
}

/// A SimpleFIN account, for choosing which budget account it fills.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub name: String,
    /// The bank.
    pub org: String,
    /// Its ISO code, or None for a currency of its own.
    pub currency: Option<String>,
    pub balance: Option<i64>,
    pub balance_date: Option<String>,
}
fn is_code(c: &str) -> bool {
    c.len() == 3 && c.bytes().all(|b| b.is_ascii_alphabetic())
}
fn listed<Tz: TimeZone>(set: &AccountSet, tz: &Tz) -> Vec<Listed> {
    set.accounts
        .iter()
        .map(|a| Listed {
            id: a.id.clone(),
            name: clean(&a.name),
            org: set
                .connections
                .iter()
                .find(|c| c.conn_id == a.conn_id)
                .map(|c| clean(c.org_name.as_deref().unwrap_or(&c.name)))
                .unwrap_or_default(),
            currency: is_code(a.currency.trim()).then(|| a.currency.trim().to_ascii_uppercase()),
            balance: cents(&a.balance),
            balance_date: (a.balance_date > 0).then(|| day(a.balance_date, tz).to_string()),
        })
        .collect()
}

/// One linked account's part of a sync, for its review.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountReview<R = Row> {
    pub account_id: String,
    /// The SimpleFIN account's name.
    pub name: String,
    pub rows: Vec<R>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_sets: Option<Vec<bank_import::CandidateSet>>,
    /// The bank's balance, to compare with the account's.
    pub balance: Option<i64>,
    pub balance_date: Option<String>,
    /// Why this account can't be reviewed.
    pub error: Option<String>,
}
/// Review rows for one linked account: its transactions posted from `start` until the download
/// at `fetched`, dated where the user is, in cents with deposits positive (a card's purchases
/// negative, as in the budget).
fn account_rows<Tz: TimeZone>(
    b: &Budget,
    account: &Account,
    link: &Link,
    sf: &SfAccount,
    (start, fetched): (NaiveDate, i64),
    tz: &Tz,
) -> Result<Vec<Row>, String> {
    let (raw, times) = account_transactions(b, link, sf, (start, fetched), tz)?;
    let mut rows = bank_import::review(b, &account.id, raw)?;
    for (row, at) in rows.iter_mut().zip(times) {
        row.at = DateTime::from_timestamp(at, 0).map(stamp);
    }
    Ok(rows)
}

fn account_transactions<Tz: TimeZone>(
    b: &Budget,
    link: &Link,
    sf: &SfAccount,
    (start, fetched): (NaiveDate, i64),
    tz: &Tz,
) -> Result<(Vec<BankTransaction>, Vec<i64>), String> {
    let currency = sf.currency.trim();
    if !currency.eq_ignore_ascii_case(&b.currency) {
        let name = clean(&sf.name);
        return Err(if is_code(currency) {
            t!("{name} is in {currency}, but this budget uses {budget}.", name, currency = currency.to_ascii_uppercase(), budget = b.currency)
        } else {
            t!("{name} uses a currency of its own, but this budget uses {budget}.", name, budget = b.currency)
        });
    }
    let mut raw = vec![];
    for t in &sf.transactions {
        // Pending transactions aren't asked for; skip any that come anyway, and their ids,
        // which can change when they post. One posted after the download (SimpleFIN's demo
        // sends tomorrow's) comes with the next sync.
        if t.pending || t.posted <= 0 || t.posted >= fetched || day(t.posted, tz) < start {
            continue;
        }
        let amount = cents(&t.amount).ok_or_else(|| {
            t!("SimpleFIN sent an amount UrBudget can't read for {account}: {amount}", account = clean(&sf.name), amount = clean(&t.amount))
        })?;
        // The Bridge sends a tidied payee beside the bank's description and memo; the memo
        // keeps whichever of those says something more.
        let description = clean(&t.description);
        let payee = t.payee.as_deref().map(clean).filter(|p| !p.is_empty()).unwrap_or(description.clone());
        let memo = [Some(description), t.memo.as_deref().map(clean)]
            .into_iter()
            .flatten()
            .find(|m| !m.is_empty() && !m.eq_ignore_ascii_case(&payee))
            .unwrap_or_default();
        let at = t.transacted_at.filter(|at| *at > 0).unwrap_or(t.posted);
        raw.push((
            BankTransaction {
                // Ids are unique only within an account (the demo's repeat across accounts), so the
                // account's id comes with it in case the budget account is linked to another later.
                fit: format!("simplefin:{}:{}", sf.id, t.id),
                date: day(at, tz).to_string(),
                payee,
                memo,
                amount: if link.flip_signs { -amount } else { amount },
            },
            at,
        ));
    }
    raw.sort_by(|a, b| a.0.date.cmp(&b.0.date));
    Ok(raw.into_iter().unzip())
}
/// The reviews for every linked account from one download made at `fetched`.
fn reviews<Tz: TimeZone>(b: &Budget, set: &AccountSet, fetched: i64, tz: &Tz) -> Vec<AccountReview> {
    reviews_with(b, set, fetched, tz, |b, account, link, sf, window, tz| {
        account_rows(b, account, link, sf, window, tz).map(|rows| (rows, None))
    })
}

fn reviews_compact<Tz: TimeZone>(b: &Budget, set: &AccountSet, fetched: i64, tz: &Tz) -> Vec<AccountReview<bank_import::CompactRow>> {
    reviews_with(b, set, fetched, tz, |b, account, link, sf, window, tz| {
        let (raw, times) = account_transactions(b, link, sf, window, tz)?;
        let mut review = bank_import::review_compact(b, &account.id, raw)?;
        // Keep the bank's timestamps on the same ordered rows in both transports.
        for (row, at) in review.items.iter_mut().zip(times) {
            row.row.at = DateTime::from_timestamp(at, 0).map(stamp);
        }
        Ok((review.items, Some(review.candidate_sets)))
    })
}

fn reviews_with<Tz: TimeZone, R>(
    b: &Budget,
    set: &AccountSet,
    fetched: i64,
    tz: &Tz,
    render: impl Fn(&Budget, &Account, &Link, &SfAccount, (NaiveDate, i64), &Tz) -> Result<(Vec<R>, Option<Vec<bank_import::CandidateSet>>), String>,
) -> Vec<AccountReview<R>> {
    let today = day(fetched, tz);
    b.accounts
        .iter()
        .filter(|a| !a.archived)
        .filter_map(|a| Some((a, link(a)?)))
        .map(|(a, l)| {
            let sf = set.accounts.iter().find(|s| s.id == l.id);
            let rows = match sf {
                Some(sf) => render(b, a, &l, sf, (since(&l, today), fetched), tz),
                None => Err(t!(
                    "SimpleFIN didn't send {account} this time. Its bank may need you to sign in again at SimpleFIN Bridge.",
                    account = a.name
                )),
            };
            let (rows, candidate_sets, error) = match rows {
                Ok((rows, sets)) => (rows, sets, None),
                Err(e) => (vec![], None, Some(e)),
            };
            AccountReview {
                account_id: a.id.clone(),
                name: sf.map(|s| clean(&s.name)).unwrap_or_default(),
                rows,
                candidate_sets,
                balance: sf.and_then(|s| cents(&s.balance)).map(|n| if l.flip_signs { -n } else { n }),
                balance_date: sf.filter(|s| s.balance_date > 0).map(|s| day(s.balance_date, tz).to_string()),
                error,
            }
        })
        .collect()
}

/// Requests made today, kept next to the budgets so a restart doesn't forget them.
#[derive(Default, Deserialize, Serialize)]
struct Usage {
    day: String,
    requests: u32,
}
struct Downloaded {
    set: AccountSet,
    at: i64,
}
pub struct SimpleFin {
    usage: PathBuf,
    /// The last download, so a review can be checked again without another request.
    last: Mutex<Option<Downloaded>>,
    /// The accounts SimpleFIN last listed, and its messages then.
    listing: Mutex<(Vec<Listed>, Vec<String>)>,
}
impl SimpleFin {
    pub fn new(directory: &Path) -> Self {
        SimpleFin {
            usage: directory.join("simplefin-usage.json"),
            last: Mutex::new(None),
            listing: Mutex::new((vec![], vec![])),
        }
    }
    fn used(&self) -> Usage {
        let today = Local::now().date_naive().to_string();
        std::fs::read(&self.usage)
            .ok()
            .and_then(|b| serde_json::from_slice::<Usage>(&b).ok())
            .filter(|u| u.day == today)
            .unwrap_or(Usage { day: today, requests: 0 })
    }
    /// Counts `n` requests against today's allowance, or says to wait for tomorrow.
    fn spend(&self, n: u32) -> Result<(), String> {
        let mut usage = self.used();
        if usage.requests + n > DAILY_LIMIT {
            return Err(if n > 1 && usage.requests < DAILY_LIMIT {
                t!(
                    "This sync needs {n} requests to SimpleFIN, and only {left} are left today. Sync again tomorrow, or link the account from a later date.",
                    n, left = DAILY_LIMIT - usage.requests
                )
            } else {
                t!("UrBudget has asked SimpleFIN for data {count} times today. SimpleFIN allows about 24 requests a day and can switch off a connection that goes over, so sync again tomorrow.", count = usage.requests)
            });
        }
        usage.requests += n;
        let _ = std::fs::write(&self.usage, serde_json::to_vec(&usage).unwrap_or_default());
        Ok(())
    }
    fn status(&self) -> Status {
        let (accounts, messages) = self.listing.lock().map(|l| l.clone()).unwrap_or_default();
        Status {
            store: vault::STORE,
            connected: vault::read(&service()).is_some(),
            accounts,
            messages,
            requests_today: self.used().requests,
            daily_limit: DAILY_LIMIT,
        }
    }
    /// What the assistant may know about bank sync: whether it's connected, today's requests,
    /// and what SimpleFIN listed this session (accounts, banks, balances, messages). Never the
    /// Access URL.
    pub fn snapshot(&self) -> Value {
        let s = self.status();
        json!({
            "available": s.store.is_some(),
            "connected": s.connected,
            "requestsToday": s.requests_today,
            "dailyLimit": s.daily_limit,
            "accounts": s.accounts,
            "messages": s.messages,
        })
    }
    fn remember(&self, set: &AccountSet) {
        if let Ok(mut listing) = self.listing.lock() {
            *listing = (listed(set, &Local), messages(set));
        }
    }
}
fn access() -> Result<Access, String> {
    Access::parse(&vault::read(&service()).ok_or_else(not_connected)?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// What protects the connection (`vault::STORE`), or None where bank sync isn't offered.
    pub store: Option<&'static str>,
    pub connected: bool,
    pub accounts: Vec<Listed>,
    pub messages: Vec<String>,
    pub requests_today: u32,
    pub daily_limit: u32,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Synced<R = Row> {
    pub reviews: Vec<AccountReview<R>>,
    pub messages: Vec<String>,
    /// The day the download was made, which becomes each imported account's last sync.
    pub through: String,
    pub requests_today: u32,
    pub daily_limit: u32,
}

pub fn simplefin_status(state: &SimpleFin) -> Status {
    state.status()
}
/// Lists the accounts again (one request, without transactions).
async fn list(state: &SimpleFin, access: &Access) -> Result<Status, String> {
    state.spend(1)?;
    let set = fetch(&http()?, access, &[("balances-only", "1".into())]).await?;
    state.remember(&set);
    Ok(state.status())
}
/// Claims a setup token, keeps the Access URL in the credential store, and lists the accounts.
pub async fn simplefin_connect(state: &SimpleFin, token: String) -> Result<Status, String> {
    vault::STORE.ok_or_else(|| t!("Bank sync needs a system credential store, which this computer doesn't have."))?;
    let url = claim_url(&token)?;
    let access = claim(&http()?, url).await?;
    vault::save(&service(), &access).map_err(|_| {
        t!("{store} couldn't keep the connection. Make a new setup token at SimpleFIN Bridge and connect again.", store = vault::store_name())
    })?;
    if let Ok(mut last) = state.last.lock() {
        *last = None;
    }
    list(&state, &Access::parse(&access)?).await
}
pub async fn simplefin_accounts(state: &SimpleFin) -> Result<Status, String> {
    list(&state, &access()?).await
}
/// Forgets the Access URL. SimpleFIN keeps the bank connection until it's removed there too.
pub fn simplefin_disconnect(state: &SimpleFin) -> Result<Status, String> {
    vault::delete(&service())?;
    if let Ok(mut last) = state.last.lock() {
        *last = None;
    }
    if let Ok(mut listing) = state.listing.lock() {
        *listing = (vec![], vec![]);
    }
    Ok(state.status())
}
/// Downloads every linked account's new transactions (unless `refresh` is false and a download
/// is at hand) and returns each account's review rows. The budget doesn't change.
pub async fn simplefin_sync(
    db: &Database,
    state: &SimpleFin,
    patch: Option<Patch>,
    refresh: bool,
) -> Result<Synced, String> {
    simplefin_sync_with(db, state, patch, refresh, |b, set, at| reviews(b, set, at, &Local)).await
}

/// Native review transport shares candidate arrays before serialization or JNI allocation.
pub async fn simplefin_sync_compact(
    db: &Database,
    state: &SimpleFin,
    patch: Option<Patch>,
    refresh: bool,
) -> Result<Synced<bank_import::CompactRow>, String> {
    simplefin_sync_with(db, state, patch, refresh, |b, set, at| reviews_compact(b, set, at, &Local)).await
}

async fn simplefin_sync_with<R>(
    db: &Database,
    state: &SimpleFin,
    patch: Option<Patch>,
    refresh: bool,
    render: impl Fn(&Budget, &AccountSet, i64) -> Vec<AccountReview<R>>,
) -> Result<Synced<R>, String> {
    let budget = db.snapshot(patch)?;
    let have = state.last.lock().map(|l| l.is_some()).unwrap_or(false);
    if refresh || !have {
        let now = Utc::now().timestamp();
        let windows = plan(&budget, now, &Local)?;
        let access = access()?;
        state.spend(windows.len() as u32)?;
        let set = download(&http()?, &access, &windows).await?;
        state.remember(&set);
        *state.last.lock().map_err(|_| t!("Bank sync stopped. Try again."))? = Some(Downloaded { set, at: now });
    }
    let last = state.last.lock().map_err(|_| t!("Bank sync stopped. Try again."))?;
    let last = last.as_ref().ok_or_else(not_connected)?;
    let usage = state.used();
    Ok(Synced {
        reviews: render(&budget, &last.set, last.at),
        messages: messages(&last.set),
        through: day(last.at, &Local).to_string(),
        requests_today: usage.requests,
        daily_limit: DAILY_LIMIT,
    })
}
/// Imports the reviewed rows like a bank file, and records the account as synced through
/// `through`, so the next sync starts from there.
pub fn simplefin_import(
    db: &Database,
    patch: Option<Patch>,
    account: String,
    choices: Vec<bank_import::Choice>,
    through: String,
) -> Result<Patch, String> {
    db.change(patch, |b| mark_synced(bank_import::apply(b, &account, choices)?, &account, &through))
}
fn mark_synced(mut b: Budget, account: &str, through: &str) -> Result<Budget, String> {
    NaiveDate::parse_from_str(through, "%Y-%m-%d").map_err(|_| t!("Invalid sync date"))?;
    let link = b
        .accounts
        .iter_mut()
        .find(|a| a.id == account)
        .and_then(|a| a.extra.get_mut("simplefin"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| t!("This account isn't linked to SimpleFIN anymore."))?;
    link.insert("lastSync".into(), json!(through));
    Ok(b)
}
/// Opens SimpleFIN Bridge's page for making a setup token.
pub fn simplefin_open() -> Result<(), String> {
    crate::host::open_browser(CREATE)
}

/// Platform-independent command surface. Network credentials never cross this boundary.
pub async fn dispatch(method: &str, p: Value, db: &Database, state: &SimpleFin) -> Result<Value, String> {
    let encode = |value| serde_json::to_value(value).map_err(|e| e.to_string());
    match method {
        "simplefin_status" => encode(simplefin_status(state)),
        "simplefin_connect" => encode(simplefin_connect(state, p["token"].as_str().unwrap_or_default().into()).await?),
        "simplefin_accounts" => encode(simplefin_accounts(state).await?),
        "simplefin_disconnect" => encode(simplefin_disconnect(state)?),
        "simplefin_open" => { simplefin_open()?; Ok(Value::Null) },
        "simplefin_sync" => serde_json::to_value(simplefin_sync(db, state, None, p["refresh"].as_bool().unwrap_or(true)).await?).map_err(|e| e.to_string()),
        "simplefin_import" => {
            let choices = serde_json::from_value(p["choices"].clone()).map_err(|e| format!("Invalid bank import choices: {e}"))?;
            let patch = simplefin_import(db, None, p["account"].as_str().unwrap_or_default().into(), choices, p["through"].as_str().unwrap_or_default().into())?;
            serde_json::to_value(patch).map_err(|e| e.to_string())
        },
        _ => Err(format!("Unknown bank connection command: {method}")),
    }
}

#[cfg(test)]
mod tests;
