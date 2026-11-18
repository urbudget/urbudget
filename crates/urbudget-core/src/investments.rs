//! Investment accounts that mirror what a broker reports.
//!
//! A tracking account can carry holdings: what it holds, what each holding is worth, and the
//! account's value day by day, taken from a broker's statement (`flex`) as the broker worked
//! them out. They live in the budget's `investments` list, one entry per account id, so they
//! sync and back up with everything else.
//!
//! The account's balance is still the sum of its transactions, like every other account's, so
//! net worth and the reports need nothing new on any platform. Two kinds of transaction this
//! module writes keep that sum equal to the broker's value:
//! - one "starting value" on the first day the broker valued the account, and
//! - one "market change" a month, dated the month's last valued day: what the value changed by
//!   beyond the money put in or taken out.
//! Money put in or taken out stays an ordinary transaction (usually a transfer from a bank
//! account), so the market changes add up to the account's gain. Both kinds are rewritten from
//! the stored values on every sync and after deposits are imported, and are never edited by
//! hand.
//!
//! The deposits, withdrawals and transfers the broker lists are kept too (`Movement`), so any
//! device that has the budget can review them (`pending`, `import`) without the statement:
//! each is matched to the transaction already in the budget, joined to its other side in
//! another account, or added. A transfer between two broker accounts the budget holds is known
//! from both statements and becomes one transfer between them.
use crate::bank_import::{self, BankTransaction, Choice};
use crate::domain::{stamp_now, valid_date, valid_money, validate, Account, AccountKind, Budget, Cleared, Split, Transaction};
use crate::flex::{self, Dec};
use crate::t;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The budget extra that holds every account's holdings.
pub const KEY: &str = "investments";
/// Daily values are kept this many days back; before that, each month's last.
const DAILY_DAYS: i64 = 400;
/// A day's change in value this much of the account, with no deposit or transfer near it to
/// explain it, is taken for money moved rather than a gain: a twentieth, and at least 100.00.
/// A whole account rarely moves that much in a day; a transfer in or out usually does.
const JUMP_SHARE: i64 = 20;
const JUMP_FLOOR: i64 = 10_000;
/// Only between valued days this close together: over a longer gap a change that size is ordinary.
const JUMP_DAYS: i64 = 4;
/// How far from the day the value shows it a listed deposit or transfer may be dated and still
/// explain the jump: a bank wire is often dated when sent and counted when it lands, days later
/// and over a weekend (a riyal wire dated the 30th showed in the value on the 5th).
const NEAR_DAYS: i64 = 7;
/// Dividends by holding are kept for this many months.
const INCOME_BY_MONTHS: i32 = 36;
/// How many days apart the two accounts' statements may date one transfer between them.
const PAIR_DAYS: i64 = 3;
/// Money that arrived in another currency is worth what the broker's rate says, and the bank
/// took what its own rate and fees came to: the two are the same money when within this share
/// of each other (3 in 100).
const NEAR_PARTS: (i64, i64) = (3, 100);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holding {
    pub symbol: String,
    pub name: String,
    /// The broker's asset category (STK, FUND, BOND, OPT).
    pub kind: String,
    /// The broker's finer category, as written (COMMON, ETF, REIT, ADR, PREFERRED); empty when
    /// it doesn't say. `investment_stats::instrument` names what it is from these.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sub_kind: String,
    /// "C" or "P" for an option; else empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub put_call: String,
    pub currency: String,
    /// As the broker wrote them: shown, never calculated with.
    pub quantity: String,
    pub price: String,
    /// In the budget's currency, in hundredths.
    pub value: i64,
    /// What it is worth in its own currency (`currency`), as a plain decimal ("61476.00"); empty
    /// when the source gives no value in that currency.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub amount: String,
    pub cost: Option<i64>,
    pub gain: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub figi: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub exchange: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashHolding {
    pub currency: String,
    pub amount: String,
    pub value: i64,
}
/// What an account earned and was charged in a month, in the budget's currency, signed as the
/// broker signs them: dividends and interest received are positive, tax and fees negative.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthIncome {
    pub month: String,
    #[serde(default)]
    pub dividends: i64,
    #[serde(default)]
    pub interest: i64,
    /// Tax withheld.
    #[serde(default)]
    pub tax: i64,
    /// Fees apart from trades' commissions.
    #[serde(default)]
    pub fees: i64,
    #[serde(default)]
    pub other: i64,
}
/// A month's trades, in the budget's currency: what closing positions gained or lost, the
/// commissions paid (negative), and how many trades there were. A month with none is kept
/// too: it says the month is known.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MonthTrades {
    pub month: String,
    pub realised: i64,
    pub commissions: i64,
    pub count: u32,
}
/// What one holding paid in a month and the tax withheld on it, in the budget's currency.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HoldingIncome {
    pub month: String,
    /// The holding, as `investment_stats::holding_key` names it.
    pub key: String,
    pub symbol: String,
    pub name: String,
    pub dividends: i64,
    pub tax: i64,
}
/// Commissions on trades over a statement's period, as its Cash Report adds them up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Commissions {
    pub from: String,
    pub to: String,
    pub amount: i64,
}
/// One deposit, withdrawal or transfer as the broker listed it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Movement {
    /// The broker's id for it, or empty when its statement gives none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub date: String,
    pub currency: String,
    /// In its own currency as the broker wrote it, positive for money in.
    pub amount: String,
    /// In the budget's currency, in hundredths.
    pub value: i64,
    /// What the broker calls it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub memo: String,
    /// It moved from or to another account, not the user's bank.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transfer: bool,
    /// The account at the broker on the other side of a transfer between the user's own
    /// accounts there, when the statement says.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub counter: String,
    /// The day its money counts on in the account's flows, when that isn't its own date: the
    /// day the value showed it (see `holdings`). Empty for its own date, and on an entry from
    /// before it was kept.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub counted: String,
}
impl Movement {
    /// The day its money counts on.
    fn counted_on(&self) -> &str {
        if self.counted.is_empty() { &self.date } else { &self.counted }
    }
}
/// One thing an account holds, as a source other than a broker's own statement gives it: its
/// figures as plain decimals ("1234.56"), in the snapshot's currency.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Position {
    pub symbol: String,
    pub name: String,
    /// STK, FUND, BOND, OPT or CRYPTO; empty when the source doesn't say.
    pub kind: String,
    /// Its own currency, when the source names one: shown, never converted with.
    pub currency: String,
    pub quantity: String,
    pub price: String,
    /// What it's worth, and what it cost in all (empty when the source doesn't say).
    pub value: String,
    pub cost: String,
}
/// What an account holds on a day, from a bank connection, a positions file (read by
/// `positions_file` or by the assistant) or a value typed in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Snapshot {
    /// One of `SOURCES`.
    pub source: String,
    /// The account at the source, or the file's name; may be empty.
    pub source_account: String,
    pub as_of: String,
    /// The currency the figures are in; empty for the budget's.
    pub currency: String,
    /// One unit of that currency in the budget's, when it isn't the budget's. One given before
    /// is remembered.
    pub rate: String,
    pub positions: Vec<Position>,
    /// Cash not invested; empty when the source doesn't say.
    pub cash: String,
    /// The account's whole value, when the source says it: what it counts beyond the
    /// positions is cash. Empty for the positions and cash added up.
    pub total: String,
}
/// Where holdings other than Interactive Brokers' come from: a bank connection, a file, or a
/// value typed in.
pub const SOURCES: [&str; 3] = ["simplefin", "file", "manual"];

/// What an account was made of when it was last brought in during a month: the one record of
/// what it held then, since the holdings themselves are only kept as they are now.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MonthEnd {
    /// The month (2026-11), and the day in it the figures are from.
    pub month: String,
    pub as_of: String,
    /// The account's value, in the budget's currency like the rest.
    pub total: i64,
    /// Its holdings added up by kind, under the group names of `investment_stats::kind`.
    pub by_kind: BTreeMap<String, i64>,
    pub cash: i64,
    /// Value that is neither a holding nor cash (see `Holdings::other`).
    pub other: i64,
    /// What the holdings with a known cost cost, and have gained.
    pub cost: Option<i64>,
    pub unrealised: Option<i64>,
}

/// One account's holdings, as its broker last reported them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Holdings {
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
    pub account_id: String,
    /// Where they come from: "ibkr".
    pub source: String,
    /// The account at the broker (U1234567).
    pub source_account: String,
    /// The day the holdings and prices are from.
    pub as_of: String,
    /// The broker account's own currency.
    pub currency: String,
    /// One unit of that currency in the budget's, on `as_of`.
    pub rate: String,
    /// The rate the user gave, for a statement that carries none for the budget's currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_rate: Option<String>,
    /// The account's value in the budget's currency.
    pub total: i64,
    pub positions: Vec<Holding>,
    pub cash: Vec<CashHolding>,
    /// Value that is neither a position nor cash: dividends and interest accrued but not paid.
    #[serde(default)]
    pub other: i64,
    /// The account's value at the end of each day, oldest first.
    pub values: Vec<(String, i64)>,
    /// Money put in (positive) or taken out on each day it happened, as the broker recorded
    /// it: what the account's gain and return are measured against, whether or not those
    /// deposits are in the budget yet. Transfers from and to other accounts are among them, at
    /// what they were worth. None on an entry from before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flows: Option<Vec<(String, i64)>>,
    /// The days among `flows` that are an estimate: the value jumped with nothing in the
    /// statement to explain it, so the jump was counted as money moved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub estimated: Vec<String>,
    /// Each deposit, withdrawal and transfer the broker listed, oldest first: what the review
    /// offers until it's in the budget. Empty on an entry from before they were kept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub movements: Vec<Movement>,
    /// For an entry that had money in and out by day before its movements were kept: the first
    /// day of the statement that brought them. Its flows and estimates before that day have no
    /// movements to be worked out again from, so they stay as they are. Empty otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub movements_from: String,
    /// What it earned and was charged, month by month.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub income: Vec<MonthIncome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commissions: Option<Commissions>,
    /// What its trades realised and cost, month by month, from the statements that list
    /// them. Empty when none has.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trades: Vec<MonthTrades>,
    /// Dividends and the tax withheld on them by holding, month by month, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub income_by: Vec<HoldingIncome>,
    /// When this was last brought in (see `domain::stamp`).
    pub synced_at: String,
    /// Sections the broker's statement left out, by the name the broker shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// What it was made of in each month it was brought in, oldest first: one a month, the
    /// month's latest. Empty on an entry from before they were kept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<MonthEnd>,
}

/// Every account's holdings. Entries that don't read are passed over, never an error: a newer
/// app may write what this one doesn't know.
pub fn read(b: &Budget) -> Vec<Holdings> {
    b.extra
        .get(KEY)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}
/// The account types the apps know (accounts.ts).
const ACCOUNT_TYPES: [&str; 18] = [
    "checking", "savings", "cash", "creditCard", "lineOfCredit", "investment", "retirement", "property", "vehicle", "receivable",
    "otherAsset", "mortgage", "autoLoan", "studentLoan", "personalLoan", "medicalDebt", "otherDebt", "otherLiability",
];
/// The type an account was given, as the apps read it: its own, else the one the budget it was
/// imported from gave it (YNAB calls every tracked asset "otherAsset"). None when neither says.
fn stated_type(a: &Account) -> Option<&str> {
    let known = |t: &&str| ACCOUNT_TYPES.contains(t);
    a.extra
        .get("accountType")
        .and_then(Value::as_str)
        .filter(known)
        .or_else(|| a.extra.get("source").and_then(|s| s.get("type")).and_then(Value::as_str).filter(known))
}
/// The accounts off the budget that Reports counts as investments: the ones whose holdings
/// have been valued (those the Investments report covers), and the ones typed as an
/// investment, retirement or other asset. Money moved between them and the budget is neither
/// spending nor income in Reports. Property, vehicles, receivables, debts, and accounts with
/// no type at all are left as they are: money into them is spending.
pub fn invested_accounts(b: &Budget) -> HashSet<&str> {
    let valued: HashSet<String> = read(b).into_iter().filter(|h| !h.values.is_empty()).map(|h| h.account_id).collect();
    b.accounts
        .iter()
        .filter(|a| !a.on_budget())
        .filter(|a| valued.contains(&a.id) || matches!(stated_type(a), Some("investment" | "retirement" | "otherAsset")))
        .map(|a| a.id.as_str())
        .collect()
}
pub fn find(b: &Budget, account: &str) -> Option<Holdings> {
    read(b).into_iter().find(|h| h.account_id == account)
}
fn store(b: &mut Budget, holdings: Holdings) {
    let list = b.extra.entry(KEY).or_insert_with(|| json!([]));
    if !list.is_array() {
        *list = json!([]);
    }
    let list = list.as_array_mut().expect("just made an array");
    list.retain(|v| v.get("accountId").and_then(Value::as_str) != Some(holdings.account_id.as_str()));
    list.push(serde_json::to_value(holdings).expect("holdings serialize"));
}
/// A broker account that now fills another of the budget's accounts (`to`): the one it filled
/// stops mirroring it, and the lines this module wrote there go, so the broker's value isn't
/// counted in both. What its review of the broker's deposits brought in goes with the broker:
/// - a row it added with no other account is taken out, and `to`'s review offers it again;
/// - one joined to, or added as, a transfer with another account moves its broker side to `to`,
///   known by the id `to`'s review gives the same row, so that review has it as in the budget.
/// The user's own transactions there stay.
pub fn release(b: &mut Budget, account: &str, to: &str) {
    // The ids the account's review gives its rows, and the ones `to`'s gives the same rows: an
    // id is the account's and the row's together.
    let raw = pending(b, account);
    let renamed: HashMap<String, String> = row_ids(account, &raw).into_iter().zip(row_ids(to, &raw)).collect();
    unlink(b, account);
    let added = |t: &Transaction| {
        t.account_id == account && t.transfer_account_id.is_none() && renamed.contains_key(&t.id) && !t.splits.iter().any(|s| crate::ledger::transfer(s).is_some())
    };
    b.transactions.retain(|t| !(generated(t) && t.account_id == account) && !added(t));
    let rename = |ids: &mut Value| {
        for id in ids.as_array_mut().into_iter().flatten() {
            if let Some(new) = id.as_str().and_then(|old| renamed.get(old)) {
                *id = json!(new);
            }
        }
    };
    for t in &mut b.transactions {
        let matched = t.extra.get("bankImportMatches").and_then(|m| m.get(account)).and_then(Value::as_str).and_then(|id| renamed.get(id)).cloned();
        if let Some(new) = matched {
            if t.account_id == account {
                t.account_id = to.to_owned();
            } else if t.transfer_account_id.as_deref() == Some(account) {
                t.transfer_account_id = Some(to.to_owned());
            }
            if let Some(sides) = t.extra.get_mut("bankImportMatches").and_then(Value::as_object_mut) {
                sides.remove(account);
                sides.insert(to.to_owned(), json!(new));
            }
            if let Some(ids) = t.extra.get_mut("bankImportIds") {
                rename(ids);
            }
        }
        // A split's part matched to a row as a transfer with the account.
        for part in &mut t.splits {
            let ours = crate::ledger::transfer(part) == Some(account)
                && part.extra.get("bankImportIds").and_then(Value::as_array).is_some_and(|ids| ids.iter().any(|id| id.as_str().is_some_and(|id| renamed.contains_key(id))));
            if ours {
                part.extra.insert("transferAccountId".into(), json!(to));
                if let Some(ids) = part.extra.get_mut("bankImportIds") {
                    rename(ids);
                }
                if let Some(ids) = t.extra.get_mut("bankImportIds") {
                    rename(ids);
                }
            }
        }
    }
}
/// Stops an account mirroring its broker. Its transactions stay, so its balance and history
/// stay as they are.
pub fn unlink(b: &mut Budget, account: &str) {
    if let Some(list) = b.extra.get_mut(KEY).and_then(Value::as_array_mut) {
        list.retain(|v| v.get("accountId").and_then(Value::as_str) != Some(account));
        if list.is_empty() {
            b.extra.remove(KEY);
        }
    }
}

/// How a statement's base currency turns into the budget's.
pub struct Converter<'a> {
    statement: &'a flex::Account,
    budget: &'a str,
    manual: Option<Dec>,
}
impl<'a> Converter<'a> {
    pub fn new(statement: &'a flex::Account, budget: &'a str, manual: Option<&str>) -> Self {
        Converter { statement, budget, manual: manual.and_then(Dec::parse).filter(|r| r > &Dec::ZERO) }
    }
    /// One unit of `from` in `to` on `date`, from the statement's rates: the latest on or
    /// before the day, else the earliest after it.
    fn listed(&self, from: &str, to: &str, date: &str) -> Option<Dec> {
        let rates = || self.statement.rates.iter().filter(|r| r.from == from && r.to == to);
        rates()
            .filter(|r| r.date.as_str() <= date)
            .max_by(|a, b| a.date.cmp(&b.date))
            .or_else(|| rates().min_by(|a, b| a.date.cmp(&b.date)))
            .map(|r| r.rate)
    }
    /// One unit of `currency` in the base currency on `date`, when the statement says.
    fn to_base(&self, currency: &str, date: &str) -> Option<Dec> {
        let s = self.statement;
        if currency == s.base {
            return Some(Dec::ONE);
        }
        self.listed(currency, &s.base, date)
            .or_else(|| s.positions.iter().filter(|p| p.currency == currency).find_map(|p| p.fx))
            .or_else(|| s.flows.iter().filter(|f| f.currency == currency && f.fx.is_some()).min_by_key(|f| distance(&f.date, date)).and_then(|f| f.fx))
    }
    /// One unit of the base currency in the budget's on `date`: 1 when they're the same, else
    /// from the statement, else the rate the user gave. None when nothing says.
    pub fn rate(&self, date: &str) -> Option<Dec> {
        let s = self.statement;
        if s.base == self.budget {
            return Some(Dec::ONE);
        }
        self.listed(&s.base, self.budget, date)
            .or_else(|| self.to_base(self.budget, date).and_then(|r| Dec::ONE.over(r)))
            .or(self.manual)
    }
    /// An amount in `currency`, in the budget's currency in hundredths.
    fn money(&self, amount: Dec, currency: &str, fx: Option<Dec>, date: &str) -> Option<i64> {
        if currency == self.budget {
            return Some(amount.cents());
        }
        let in_base = amount.times(fx.or_else(|| self.to_base(currency, date))?);
        Some(in_base.times(self.rate(date)?).cents())
    }
}
fn distance(a: &str, b: &str) -> i64 {
    let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    match (day(a), day(b)) {
        (Some(a), Some(b)) => (a - b).num_days().abs(),
        _ => i64::MAX,
    }
}

/// The currencies a rate is needed between (the statement's and the budget's), when the
/// statement can't be turned into the budget's currency without one from the user.
pub fn needs_rate(statement: &flex::Account, budget: &str, manual: Option<&str>) -> Option<(String, String)> {
    let day = as_of(statement);
    Converter::new(statement, budget, manual).rate(&day).is_none().then(|| (statement.base.clone(), budget.to_owned()))
}
/// The day a statement's holdings are from.
pub fn as_of(statement: &flex::Account) -> String {
    statement.values.last().map(|(d, _)| d.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| statement.to.clone())
}
/// A statement's value in its own currency: the broker's figure, else what it holds added up.
pub fn value_in_base(statement: &flex::Account) -> Dec {
    statement.values.last().map(|(_, v)| *v).unwrap_or_else(|| {
        let positions = statement.positions.iter().fold(Dec::ZERO, |sum, p| sum.plus(p.value.times(p.fx.unwrap_or(Dec::ONE))));
        positions.plus(statement.cash_base.unwrap_or(Dec::ZERO))
    })
}

fn unreadable() -> String {
    t!("This statement has amounts too large for UrBudget to keep.")
}
/// The statement holds something in a currency it gives no rate for: its Flex Query leaves out
/// both the currency rates and the rows' own rate.
fn no_rate(currency: &str, base: &str) -> String {
    t!("The statement doesn't say what {currency} is worth in {base}. In the Flex Query, set Include Currency Rates to Yes, then sync again.", currency, base)
}

/// Builds an account's holdings from a statement, keeping the values already stored for days
/// the statement doesn't cover.
fn holdings(b: &Budget, account: &str, s: &flex::Account, manual: Option<&str>, now: &str) -> Result<Holdings, String> {
    let known = find(b, account);
    let manual = manual.map(str::to_owned).or_else(|| known.as_ref().and_then(|k| k.manual_rate.clone()));
    let known_values = known.as_ref().map(|k| k.values.clone());
    let convert = Converter::new(s, &b.currency, manual.as_deref());
    let day = as_of(s);
    if s.base.is_empty() || !valid_date(&day) {
        return Err(t!("This statement doesn't say the account's currency or dates. Add the Account Information section to the Flex Query, then sync again."));
    }
    let need = || t!("UrBudget needs the exchange rate from {from} to {to} for this account.", from = s.base, to = b.currency);
    let rate = convert.rate(&day).ok_or_else(need)?;
    let money = |amount: Dec, currency: &str, fx: Option<Dec>| {
        let converted = convert.money(amount, currency, fx, &day).ok_or_else(|| no_rate(currency, &s.base))?;
        if valid_money(converted) { Ok(converted) } else { Err(unreadable()) }
    };
    let mut positions = vec![];
    for p in &s.positions {
        let value = money(p.value, &p.currency, p.fx)?;
        let cost = p.cost.map(|c| money(c, &p.currency, p.fx)).transpose()?;
        let gain = match p.gain {
            Some(g) => Some(money(g, &p.currency, p.fx)?),
            None => cost.map(|c| value - c),
        };
        positions.push(Holding {
            symbol: p.symbol.clone(),
            name: p.name.clone(),
            kind: p.kind.clone(),
            sub_kind: p.sub_kind.clone(),
            put_call: p.put_call.clone(),
            currency: p.currency.clone(),
            quantity: p.quantity.text(),
            price: p.price.text(),
            value,
            amount: p.amount.clone(),
            cost,
            gain,
            conid: p.conid.clone(),
            isin: p.isin.clone(),
            figi: p.figi.clone(),
            exchange: p.exchange.clone(),
        });
    }
    positions.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.symbol.cmp(&b.symbol)));
    // Cash in each currency when every one converts; else all of it as one amount.
    let each: Option<Vec<CashHolding>> = s
        .cash
        .iter()
        .map(|c| Some(CashHolding { currency: c.currency.clone(), amount: c.amount.text(), value: convert.money(c.amount, &c.currency, None, &day)? }))
        .collect();
    let mut cash = match (each, s.cash_base) {
        (Some(each), _) if !each.is_empty() || s.cash_base.is_none() => each,
        (_, Some(all)) if !all.is_zero() => vec![CashHolding { currency: s.base.clone(), amount: all.text(), value: all.times(rate).cents() }],
        _ => vec![],
    };
    cash.sort_by(|a, b| b.value.cmp(&a.value));
    let held: i64 = positions.iter().map(|p| p.value).sum::<i64>() + cash.iter().map(|c| c.value).sum::<i64>();
    let total = match s.values.last() {
        Some((_, v)) => v.times(rate).cents(),
        None => held,
    };
    if !valid_money(total) {
        return Err(unreadable());
    }
    let mut values: BTreeMap<String, i64> = known_values.map(|v| v.into_iter().collect()).unwrap_or_default();
    if s.values.is_empty() {
        values.insert(day.clone(), total);
    }
    for (date, value) in &s.values {
        let value = value.times(convert.rate(date).ok_or_else(need)?).cents();
        if !valid_money(value) {
            return Err(unreadable());
        }
        values.insert(date.clone(), value);
    }
    // The statement is the record for the days it covers; what was kept from before stays.
    let (from, to) = (if s.from.is_empty() { "0000-00-00" } else { s.from.as_str() }, if s.to.is_empty() { day.as_str() } else { s.to.as_str() });
    let covered = |date: &str| date >= from && date <= to;
    // A query without one of these sections lists none of that kind of money moved: what was
    // kept of that kind stays, and what the value did can't be checked against it.
    let lacks = |section: &str| s.missing.iter().any(|m| *m == section);
    let (no_deposits, no_transfers) = (lacks("Cash Transactions"), lacks("Transfers"));
    let mut movements: Vec<Movement> = vec![];
    for f in &s.flows {
        let amount = convert.money(f.amount, &f.currency, f.fx, &f.date).ok_or_else(|| no_rate(&f.currency, &s.base))?;
        if !valid_money(amount) {
            return Err(unreadable());
        }
        if amount != 0 {
            movements.push(Movement {
                id: f.id.clone(),
                date: f.date.clone(),
                currency: f.currency.clone(),
                amount: f.amount.text(),
                value: amount,
                memo: f.description.clone(),
                transfer: f.transfer,
                counter: f.counter.clone(),
                counted: String::new(),
            });
        }
    }
    // Ones from before stay, unless it lists them again; within its days, ones of a kind its
    // query doesn't send stay too.
    let earlier: Vec<Movement> = known
        .as_ref()
        .map(|k| {
            k.movements
                .iter()
                .filter(|m| {
                    let again = !m.id.is_empty() && movements.iter().any(|new| new.id == m.id);
                    let unsent = if m.transfer { no_transfers } else { no_deposits };
                    !again && (!covered(&m.date) || unsent)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    movements.extend(earlier);
    movements.sort_by(|a, b| a.date.cmp(&b.date));
    // Money in and out by day is worked out again on every sync, from every movement kept and
    // every value kept, over the days valued daily: the same movements and values always give
    // the same days, whatever days the statement covers. Before those days only each month's
    // last value is kept, which can't show a jump, so what was worked out then stays as it is,
    // and so do the movements counted on those days.
    // An entry from before movements were kept has nothing to work its days out from but the
    // statement, so outside the statement's days it keeps what it had. It is known by money in
    // or out on a day that wasn't an estimate, with no movement kept to say why; once a
    // statement brings it movements, the days before that statement's stay as they were.
    let legacy = known.as_ref().is_some_and(|k| {
        k.movements.is_empty() && k.flows.as_ref().is_some_and(|f| f.iter().any(|(d, amount)| *amount != 0 && !k.estimated.contains(d)))
    });
    let movements_from = match known.as_ref() {
        Some(k) if !k.movements_from.is_empty() => k.movements_from.clone(),
        Some(k) if k.movements.is_empty() && k.flows.is_some() && !movements.is_empty() => from.to_owned(),
        _ => String::new(),
    };
    // The newest day the account knows, this statement's or a later one already kept: an older
    // statement (a saved file) mustn't widen the days worked out again or the days kept daily.
    let newest = known.as_ref().map_or(day.clone(), |k| if k.as_of > day { k.as_of.clone() } else { day.clone() });
    let daily = daily_from(&newest);
    let mut live_from = if legacy && from > daily.as_str() { from.to_owned() } else { daily };
    if movements_from > live_from {
        live_from = movements_from.clone();
    }
    let frozen = |date: &str| date < live_from.as_str() || (legacy && date > to);
    let mut flows: BTreeMap<String, i64> =
        known.as_ref().and_then(|k| k.flows.as_ref()).map(|f| f.iter().filter(|(d, _)| frozen(d)).cloned().collect()).unwrap_or_default();
    let mut estimated: Vec<String> = known.as_ref().map(|k| k.estimated.iter().filter(|d| frozen(d)).cloned().collect()).unwrap_or_default();
    let was_estimated = |date: &str| known.as_ref().is_some_and(|k| k.estimated.iter().any(|d| d == date));
    // The movements worked out again: those counted on a day still worked out. Each starts on
    // its own date, which may be just before those days, and moves to the day of the jump it
    // explains. A movement counted on a day that stays as it is has its money there already.
    let live: Vec<usize> = (0..movements.len()).filter(|&i| !frozen(movements[i].counted_on())).collect();
    // Their money by day, with the movements on each day.
    let mut listed: BTreeMap<String, (i64, Vec<usize>)> = BTreeMap::new();
    for &i in &live {
        let entry = listed.entry(movements[i].date.clone()).or_default();
        entry.0 += movements[i].value;
        entry.1.push(i);
    }
    let mut counted: Vec<String> = movements.iter().map(|m| m.date.clone()).collect();
    // A day the value jumped by a twentieth or more: money or holdings moved, unless the market
    // did it. Listed deposits and transfers explain jumps near them; one that nothing listed
    // explains is taken for money moved only where the statement couldn't list it (a query
    // without the Transfers or Cash Transactions section, or an entry from before movements were
    // kept). Outside the statement's days, a day taken for one before stays one. Counting the
    // jump as money moved keeps it out of the gain; the days are remembered as estimates so the
    // app can say so.
    let may_guess = |date: &str| if covered(date) { no_deposits || no_transfers || legacy } else { was_estimated(date) };
    // The days worked out again, after the last value before them, so a jump on their first day
    // is judged too.
    let seed = values.range(..live_from.clone()).next_back();
    let series: Vec<(&String, i64)> = seed.into_iter().chain(values.iter().filter(|(d, _)| !frozen(d))).map(|(d, v)| (d, *v)).collect();
    let mut guessed: Vec<(String, i64)> = vec![];
    // The listed days' flows not yet taken to explain a jump: each explains one only, so a
    // deposit can't also cover a transfer of the same size a few days later that wasn't listed.
    let mut unused: Vec<(String, i64, Vec<usize>)> = listed.iter().map(|(d, (amount, at))| (d.clone(), *amount, at.clone())).collect();
    // A flow that explains a jump on another day counts on the jump's day, when the value shows
    // it: else the days between read as a loss and a gain of the whole deposit.
    let mut redated: Vec<(String, String, i64)> = vec![];
    for pair in series.windows(2) {
        let ((earlier, before), (date, after)) = (pair[0], pair[1]);
        if distance(earlier, date) > JUMP_DAYS {
            continue;
        }
        let moved = after - before;
        let unexplained = moved - listed.get(date).map_or(0, |(amount, _)| *amount);
        // The jump itself has to be large: a deposit listed a day before the value shows it
        // leaves a small day with a flow, which is timing, not money unaccounted for.
        let large = moved.abs() >= JUMP_FLOOR && moved.abs() * JUMP_SHARE >= before.abs().max(after.abs());
        if !large {
            continue;
        }
        // What explains it: one flow dated within a week of it, the nearest first, or those
        // days' flows together.
        let close = |amount: i64| (amount - moved).abs() * 4 <= moved.abs();
        let near: Vec<usize> = (0..unused.len()).filter(|&i| distance(&unused[i].0, date) <= NEAR_DAYS).collect();
        let one = near.iter().copied().filter(|&i| close(unused[i].1)).min_by_key(|&i| distance(&unused[i].0, date));
        let taken: Vec<usize> = match one {
            Some(i) => vec![i],
            None if !near.is_empty() && close(near.iter().map(|&i| unused[i].1).sum()) => near,
            None => vec![],
        };
        if !taken.is_empty() {
            for i in taken.into_iter().rev() {
                let (day, amount, at) = unused.remove(i);
                if day != *date {
                    redated.push((day, date.clone(), amount));
                    for m in at {
                        counted[m] = date.clone();
                    }
                }
            }
        } else if may_guess(date) {
            guessed.push((date.clone(), unexplained));
        }
    }
    let mut listed: BTreeMap<String, i64> = listed.into_iter().map(|(d, (amount, _))| (d, amount)).collect();
    for (from, to, amount) in redated {
        *listed.entry(from).or_default() -= amount;
        *listed.entry(to).or_default() += amount;
    }
    for (date, amount) in listed {
        *flows.entry(date).or_default() += amount;
    }
    // Each worked-out movement remembers the day it counts on, for the next sync.
    for &i in &live {
        movements[i].counted = if counted[i] == movements[i].date { String::new() } else { counted[i].clone() };
    }
    for (date, amount) in guessed {
        *flows.entry(date.clone()).or_default() += amount;
        estimated.push(date);
    }
    estimated.sort();
    estimated.dedup();
    // A first statement that lists no money moved of either kind leaves the account's money in
    // and out to the budget's own transfers, as for an entry from before flows were kept.
    let unlisted = no_deposits && no_transfers && known.as_ref().map_or(true, |k| k.flows.is_none());
    if unlisted {
        flows.clear();
        estimated.clear();
    }
    // A month the statement starts partway through keeps the fuller figures already stored.
    let whole_from = if from.ends_with("-01") { from[..7].to_owned() } else { format!("{}~", &from[..7]) };
    // Without the Cash Transactions section the statement says nothing of it: what was kept stays.
    let outside = |month: &str| month < whole_from.as_str() || month > &to[..7];
    let mut income: BTreeMap<String, MonthIncome> = known
        .as_ref()
        .map(|k| k.income.iter().filter(|m| no_deposits || outside(&m.month)).map(|m| (m.month.clone(), m.clone())).collect())
        .unwrap_or_default();
    // The same months' dividends and tax withheld by the holding they were paid on.
    let mut income_by: Vec<HoldingIncome> = known.as_ref().map(|k| k.income_by.iter().filter(|row| no_deposits || outside(&row.month)).cloned().collect()).unwrap_or_default();
    for i in &s.income {
        let month = i.date[..7].to_owned();
        if month.as_str() < whole_from.as_str() && known.as_ref().is_some_and(|k| k.income.iter().any(|m| m.month == month)) {
            continue;
        }
        let amount = convert.money(i.amount, &i.currency, i.fx, &i.date).ok_or_else(|| no_rate(&i.currency, &s.base))?;
        let row = income.entry(month.clone()).or_insert_with(|| MonthIncome { month: month.clone(), ..MonthIncome::default() });
        match i.kind {
            flex::IncomeKind::Dividends => row.dividends += amount,
            flex::IncomeKind::Interest => row.interest += amount,
            flex::IncomeKind::Tax => row.tax += amount,
            flex::IncomeKind::Fees => row.fees += amount,
            flex::IncomeKind::Other => row.other += amount,
        }
        if !i.security.named() || !matches!(i.kind, flex::IncomeKind::Dividends | flex::IncomeKind::Tax) {
            continue;
        }
        // The holding as the statement's own positions identify it, so it has the key it has
        // among the holdings; one since sold has what the row says of it.
        let paid_on = &i.security;
        let held = s.positions.iter().find(|p| {
            if !paid_on.conid.is_empty() || !paid_on.isin.is_empty() {
                (!paid_on.conid.is_empty() && p.conid == paid_on.conid) || (!paid_on.isin.is_empty() && p.isin == paid_on.isin)
            } else {
                p.symbol == paid_on.symbol && p.currency == i.currency
            }
        });
        let either = |held: Option<&String>, said: &String| held.filter(|text| !text.is_empty()).unwrap_or(said).clone();
        let (isin, conid, symbol) = (either(held.map(|p| &p.isin), &paid_on.isin), either(held.map(|p| &p.conid), &paid_on.conid), either(held.map(|p| &p.symbol), &paid_on.symbol));
        let currency = held.map_or(&i.currency, |p| &p.currency);
        let key = crate::investment_stats::holding_key(&isin, &conid, &symbol, &paid_on.description, currency);
        let at = match income_by.iter().position(|row| row.month == month && row.key == key) {
            Some(at) => at,
            None => {
                // Its name among the holdings, else the one kept before, else its symbol.
                let name = held
                    .map(|p| p.name.clone())
                    .or_else(|| known.as_ref().and_then(|k| k.income_by.iter().find(|row| row.key == key)).map(|row| row.name.clone()))
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| if symbol.is_empty() { paid_on.description.clone() } else { symbol.clone() });
                income_by.push(HoldingIncome { month: month.clone(), key, symbol, name, dividends: 0, tax: 0 });
                income_by.len() - 1
            }
        };
        match i.kind {
            flex::IncomeKind::Tax => income_by[at].tax += amount,
            _ => income_by[at].dividends += amount,
        }
    }
    income_by.sort_by(|a, b| a.month.cmp(&b.month).then_with(|| a.key.cmp(&b.key)));
    if let Some(oldest) = income_by.last().and_then(|row| shifted(&row.month, 1 - INCOME_BY_MONTHS)) {
        income_by.retain(|row| row.month >= oldest);
    }
    // What its trades realised and cost by month, when the statement lists them. Every month
    // of its period is then known, with a trade in it or not; a statement that doesn't list
    // them leaves what was kept.
    let trades: Vec<MonthTrades> = match &s.trades {
        None => known.as_ref().map(|k| k.trades.clone()).unwrap_or_default(),
        Some(listed) => {
            let had = |month: &str| known.as_ref().is_some_and(|k| k.trades.iter().any(|row| row.month == month));
            let mut by_month: BTreeMap<String, MonthTrades> =
                known.as_ref().map(|k| k.trades.iter().filter(|row| outside(&row.month)).map(|row| (row.month.clone(), row.clone())).collect()).unwrap_or_default();
            let mut month = if valid_date(from) { Some(from[..7].to_owned()) } else { None };
            while let Some(current) = month.filter(|current| current.as_str() <= &to[..7]) {
                if !(current.as_str() < whole_from.as_str() && had(&current)) {
                    by_month.entry(current.clone()).or_insert_with(|| MonthTrades { month: current.clone(), ..MonthTrades::default() });
                }
                month = shifted(&current, 1);
            }
            for t in listed {
                let month = t.date[..7].to_owned();
                if month.as_str() < whole_from.as_str() && had(&month) {
                    continue;
                }
                let row = by_month.entry(month.clone()).or_insert_with(|| MonthTrades { month, ..MonthTrades::default() });
                row.count += 1;
                // A figure with no rate to bring it into the budget's currency is left out.
                if let Some(realised) = t.realised.and_then(|amount| convert.money(amount, &t.currency, t.fx, &t.date)) {
                    row.realised = row.realised.saturating_add(realised);
                }
                let charged_in = if t.commission_currency.is_empty() { &t.currency } else { &t.commission_currency };
                let fx = t.fx.filter(|_| *charged_in == t.currency);
                if let Some(paid) = t.commission.and_then(|amount| convert.money(amount, charged_in, fx, &t.date)) {
                    row.commissions = row.commissions.saturating_add(paid);
                }
            }
            by_month.into_values().collect()
        }
    };
    let commissions = match s.commissions {
        Some(paid) => Some(Commissions { from: s.from.clone(), to: s.to.clone(), amount: paid.times(rate).cents() }),
        None => known.as_ref().and_then(|k| k.commissions.clone()),
    };
    let mut next = Holdings {
        // What a newer app stored in the entry stays.
        extra: known.as_ref().map(|k| k.extra.clone()).unwrap_or_default(),
        account_id: account.to_owned(),
        source: "ibkr".into(),
        source_account: s.id.clone(),
        as_of: day.clone(),
        currency: s.base.clone(),
        rate: rate.text(),
        manual_rate: manual.filter(|_| s.base != b.currency),
        total,
        positions,
        cash,
        other: total - held,
        values: thin(values, &newest),
        flows: (!unlisted).then(|| flows.into_iter().filter(|(_, amount)| *amount != 0).collect()),
        estimated,
        movements,
        movements_from,
        income: income.into_values().collect(),
        commissions,
        trades,
        income_by,
        synced_at: now.to_owned(),
        missing: s.missing.iter().map(|m| (*m).to_owned()).collect(),
        snapshots: known.map(|k| k.snapshots).unwrap_or_default(),
    };
    let made_of = month_end(&next);
    keep_month(&mut next.snapshots, made_of);
    Ok(next)
}
/// What the holdings are made of on the day they're from, for that month's snapshot.
fn month_end(held: &Holdings) -> MonthEnd {
    let sums = crate::investment_stats::totals(held);
    MonthEnd {
        month: held.as_of.get(..7).unwrap_or(&held.as_of).to_owned(),
        as_of: held.as_of.clone(),
        total: held.total,
        by_kind: crate::investment_stats::kinds(&held.positions).into_iter().collect(),
        cash: sums.cash,
        other: held.other,
        cost: sums.cost,
        unrealised: sums.unrealised,
    }
}
/// Keeps a month's snapshot in place of the one already there for that month, unless that one
/// is from a later day: figures brought in for an earlier day never replace newer ones.
fn keep_month(snapshots: &mut Vec<MonthEnd>, made_of: MonthEnd) {
    match snapshots.iter_mut().find(|s| s.month == made_of.month) {
        Some(known) if known.as_of > made_of.as_of => {}
        Some(known) => *known = made_of,
        None => snapshots.push(made_of),
    }
    snapshots.sort_by(|a, b| a.month.cmp(&b.month));
}
/// The month (2026-11) so many months after, or before when negative.
fn shifted(month: &str, by: i32) -> Option<String> {
    let first = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok()?;
    let moved = if by < 0 { first.checked_sub_months(chrono::Months::new(by.unsigned_abs())) } else { first.checked_add_months(chrono::Months::new(by.unsigned_abs())) };
    moved.map(|d| d.format("%Y-%m").to_string())
}
/// The day so many days after `date`, or before when negative; `date` itself when it doesn't read.
#[cfg(test)]
fn shift_days(date: &str, by: i64) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or_else(|_| date.to_owned(), |d| (d + chrono::Duration::days(by)).to_string())
}
/// The first day a value is kept for every day, `DAILY_DAYS` before the latest.
fn daily_from(latest: &str) -> String {
    chrono::NaiveDate::parse_from_str(latest, "%Y-%m-%d").map(|d| (d - chrono::Duration::days(DAILY_DAYS)).to_string()).unwrap_or_default()
}
/// Every day's value for the last `DAILY_DAYS`, and each earlier month's last, and always the
/// first valued day: the starting value and the first month's change are measured from it.
fn thin(values: BTreeMap<String, i64>, latest: &str) -> Vec<(String, i64)> {
    let cutoff = daily_from(latest);
    let mut out: Vec<(String, i64)> = vec![];
    for (date, value) in values {
        if !valid_date(&date) {
            continue;
        }
        if date < cutoff && out.len() > 1 && out.last().is_some_and(|(d, _)| d[..7] == date[..7]) {
            out.pop();
        }
        out.push((date, value));
    }
    out
}

/// The id part for an account in the transactions this module writes: the account's id, or a
/// digest of one too long to fit beside the rest.
fn tag(account: &str) -> String {
    if account.len() <= 60 {
        account.to_owned()
    } else {
        Sha256::digest(account.as_bytes()).iter().take(16).map(|b| format!("{b:02x}")).collect()
    }
}
fn start_id(account: &str) -> String {
    format!("investment-start:{}", tag(account))
}
fn change_id(account: &str, month: &str) -> String {
    format!("investment-change:{}:{month}", tag(account))
}
fn moved_id(account: &str, month: &str) -> String {
    format!("investment-moved:{}:{month}", tag(account))
}
/// Whether this module wrote the transaction: a starting value, a month's market change, or a
/// month's money moved that the budget doesn't have yet.
pub fn generated(t: &Transaction) -> bool {
    t.extra.get("investment").is_some_and(|v| v == "start" || v == "change" || v == "moved")
}
/// Whether the transaction is a month's market change: what the account gained or lost.
pub fn market_change(t: &Transaction) -> bool {
    t.extra.get("investment").is_some_and(|v| v == "change")
}

/// Rewrites the transactions this module writes for an account from its stored values, so its
/// balance on each valued month end, and now, is what the broker said:
/// - **Starting value**, on the first valued day.
/// - **Market change**, a month: what the value changed by beyond the money the broker says
///   was put in or taken out. It is the account's gain whatever the budget has recorded.
/// - **Money moved in or out**, a month: the broker's deposits, withdrawals and transfers that
///   the budget doesn't have yet. It shrinks to nothing as those are entered or imported
///   (everything else in the account counts as money put in or taken out).
/// Holdings from before the broker's flows were kept have only the first two, and their market
/// change is whatever the budget's own records leave. Without holdings there is nothing to do.
pub fn revalue(b: &mut Budget, account: &str) -> Result<(), String> {
    let Some(holdings) = find(b, account) else { return Ok(()) };
    let Some(opening) = b.accounts.iter().find(|a| a.id == account).map(|a| a.opening_balance) else { return Ok(()) };
    let Some((first_day, first_value)) = holdings.values.first().cloned() else { return Ok(()) };
    // What the account holds apart from this module's own transactions, by day, and whether
    // each is money moved with the broker's own record of it: a transfer with another of the
    // budget's accounts, or one this account's review of the broker's deposits brought in
    // (matched or added without another account).
    let reviewed: HashSet<String> = row_ids(account, &pending(b, account)).into_iter().collect();
    let from_review = |t: &Transaction| t.extra.get("bankImportMatches").and_then(|m| m.get(account)).and_then(Value::as_str).is_some_and(|id| reviewed.contains(id));
    let mut moves: Vec<(String, i64, bool)> = crate::ledger::transactions(b)
        .iter()
        .filter(|t| !generated(t))
        .filter_map(|t| {
            if t.account_id == account {
                Some((t.date.clone(), t.amount, t.transfer_account_id.is_some() || from_review(t)))
            } else if t.transfer_account_id.as_deref() == Some(account) {
                Some((t.date.clone(), -t.amount, true))
            } else {
                None
            }
        })
        .collect();
    // Money moved that the broker counted on another day within a week, for nearly the same
    // amount (a wire in transit, a little lost to the exchange rate), counts on the broker's
    // day: else a wire sent on the 30th and counted on the 5th has a line taking it out of the
    // month it left the bank and another putting it back in the next. Nearly the same is what
    // the review takes for the same money (`NEAR_PARTS`).
    if let Some(flows) = &holdings.flows {
        let mut free: Vec<(String, i64)> = flows.iter().filter(|(_, amount)| *amount != 0).cloned().collect();
        moves.sort();
        for (day, amount, _) in moves.iter_mut().filter(|m| m.2) {
            let close = |counted: i64| {
                counted.signum() == amount.signum() && (counted - *amount).abs().saturating_mul(NEAR_PARTS.1) <= amount.abs().saturating_mul(NEAR_PARTS.0)
            };
            let nearest = (0..free.len()).filter(|&i| distance(&free[i].0, day) <= NEAR_DAYS && close(free[i].1)).min_by_key(|&i| distance(&free[i].0, day));
            if let Some(i) = nearest {
                *day = free.remove(i).0;
            }
        }
    }
    let mut outside: Vec<(String, i64)> = moves.into_iter().map(|(day, amount, _)| (day, amount)).collect();
    outside.sort();
    let through = |day: &str| opening + outside.iter().take_while(|(d, _)| d.as_str() <= day).map(|(_, n)| n).sum::<i64>();
    let mut wanted: Vec<(String, &str, String, i64, String)> = vec![];
    let start = first_value - through(&first_day);
    let mut written = start;
    if start != 0 {
        wanted.push((start_id(account), "start", first_day.clone(), start, "Value when tracking began".into()));
    }
    let mut month_ends: BTreeMap<&str, (&str, i64)> = BTreeMap::new();
    for (date, value) in &holdings.values {
        month_ends.insert(&date[..7], (date.as_str(), *value));
    }
    let (mut last_day, mut last_value) = (first_day.as_str(), first_value);
    for (month, (date, value)) in month_ends {
        // What it takes to bring the balance to the broker's value at the month's end.
        let needed = value - through(date) - written;
        let (change, moved) = match &holdings.flows {
            Some(flows) => {
                let put: i64 = flows.iter().filter(|(d, _)| d.as_str() > last_day && d.as_str() <= date).map(|(_, amount)| *amount).sum();
                let change = value - last_value - put;
                (change, needed - change)
            }
            None => (needed, 0),
        };
        written += needed;
        if change != 0 {
            wanted.push((change_id(account, month), "change", date.to_owned(), change, format!("Change in value in {month} beyond money put in or taken out")));
        }
        if moved != 0 {
            wanted.push((moved_id(account, month), "moved", date.to_owned(), moved, format!("What the broker counted as money moved in {month} beyond what the budget has: deposits, withdrawals and transfers not reviewed yet, and differences in exchange rates")));
        }
        (last_day, last_value) = (date, value);
    }
    // Checked before anything is written, so a refusal leaves the budget as it was.
    if wanted.iter().any(|w| !valid_money(w.3)) {
        return Err(unreadable());
    }
    let now = stamp_now();
    b.transactions.retain(|t| !(generated(t) && t.account_id == account) || wanted.iter().any(|w| w.0 == t.id));
    for (id, kind, date, amount, memo) in wanted {
        let split = Split { extra: Map::from_iter([("unassigned".to_string(), Value::Bool(true))]).into(), category_id: None, amount };
        match b.transactions.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                // Leave an unchanged month exactly as it was, so a sync that brings nothing new
                // changes nothing. One still worded as this module words it (an earlier build's
                // wording, which starts the same) takes this build's; a note the user wrote stays.
                let ours = t.memo.trim().is_empty() || t.memo.split_whitespace().take(3).eq(memo.split_whitespace().take(3));
                if t.amount == amount && t.date == date && t.account_id == account {
                    if ours && t.memo != memo {
                        t.memo = memo;
                    }
                } else {
                    t.amount = amount;
                    t.date = date;
                    t.account_id = account.to_owned();
                    t.splits = vec![split];
                    t.memo = memo;
                }
            }
            None => b.transactions.push(Transaction {
                extra: Map::from_iter([("investment".to_string(), json!(kind)), ("createdAt".to_string(), json!(now))]).into(),
                id,
                date,
                account_id: account.to_owned(),
                payee: match kind {
                    "start" => "Starting value",
                    "moved" => "Money moved in or out",
                    _ => "Market change",
                }
                .into(),
                memo,
                amount,
                status: Cleared::Cleared,
                splits: vec![split],
                transfer_account_id: None,
                transfer_status: None,
            }),
        }
    }
    Ok(())
}

/// An account for a broker account that has none yet: a tracking account of the Investment
/// type, named after the alias the user gave it at the broker or the broker and the account's
/// last digits.
pub fn new_account(b: &Budget, id: &str, s: &flex::Account, name: Option<&str>) -> Account {
    let tail: String = s.id.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let base = name
        .map(flex::clean)
        .filter(|n| !n.is_empty())
        .or_else(|| (!s.alias.is_empty()).then(|| s.alias.clone()))
        .unwrap_or_else(|| format!("Interactive Brokers …{tail}"));
    let mut name = base.clone();
    let mut n = 2;
    while b.accounts.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
        name = format!("{base} {n}");
        n += 1;
    }
    Account {
        extra: Map::from_iter([("accountType".to_string(), json!("investment"))]),
        id: id.to_owned(),
        name,
        kind: AccountKind::Tracking,
        opening_balance: 0,
        archived: false,
    }
}

/// Brings a statement into an account: stores what it holds and rewrites its value history.
/// `manual` is the exchange rate from the statement's currency to the budget's, for a
/// statement that carries none; one given before is remembered.
pub fn apply(mut b: Budget, account: &str, s: &flex::Account, manual: Option<&str>) -> Result<Budget, String> {
    let target = b.accounts.iter().find(|a| a.id == account).ok_or_else(|| t!("Account not found."))?;
    if target.kind != AccountKind::Tracking {
        return Err(t!("Holdings go into a tracking account. Choose an Investment account, or let UrBudget add one."));
    }
    if let Some(other) = read(&b).into_iter().find(|h| h.source_account == s.id && h.account_id != account) {
        let name = b.accounts.iter().find(|a| a.id == other.account_id).map(|a| a.name.clone()).unwrap_or_default();
        return Err(t!("{broker} already fills {account}.", broker = s.id, account = name));
    }
    let mut next = holdings(&b, account, s, manual, &stamp_now())?;
    // An older statement (a saved file) adds to the history and leaves what the account holds
    // now, and what its latest statement said, as they are.
    if let Some(known) = find(&b, account).filter(|k| next.as_of < k.as_of) {
        next = Holdings {
            values: next.values,
            flows: next.flows,
            estimated: next.estimated,
            movements: next.movements,
            movements_from: next.movements_from,
            income: next.income,
            trades: next.trades,
            income_by: next.income_by,
            snapshots: next.snapshots,
            synced_at: next.synced_at,
            ..known
        };
    }
    // A statement that says what the last one did leaves the budget exactly as it was, so a
    // sync that brings nothing new isn't a change to save, undo or upload.
    if let Some(known) = find(&b, account) {
        if (Holdings { synced_at: known.synced_at.clone(), extra: known.extra.clone(), ..next.clone() }) == known {
            next = known;
        }
    }
    store(&mut b, next);
    revalue(&mut b, account)?;
    validate(&b)?;
    Ok(b)
}

/// An amount of the budget's currency in hundredths, as a decimal.
fn decimal(cents: i64) -> Dec {
    Dec::parse(&format!("{}{}.{:02}", if cents < 0 { "-" } else { "" }, cents.unsigned_abs() / 100, cents.unsigned_abs() % 100)).unwrap_or(Dec::ZERO)
}

/// Records what an account holds on a day from a source other than Interactive Brokers (see
/// `Snapshot`), and rewrites its value history so its balance is that value: what it changed
/// by beyond the money the budget has going in and out is its Market change.
/// - The newest day's figures replace what the account holds; an earlier day's only add to
///   its history, so past values can be filled in.
/// - A value alone (nothing listed) is kept as it is. With holdings listed, what the source
///   counts beyond them is cash.
/// - Figures in another currency need a rate, once: the last one given is used until another is.
pub fn record(mut b: Budget, account: &str, snap: Snapshot) -> Result<Budget, String> {
    let target = b.accounts.iter().find(|a| a.id == account).ok_or_else(|| t!("Account not found."))?;
    if target.kind != AccountKind::Tracking {
        return Err(t!("Holdings go into a tracking account. Choose an Investment account, or let UrBudget add one."));
    }
    if !SOURCES.contains(&snap.source.as_str()) {
        return Err(format!("Unknown holdings source: {}", snap.source));
    }
    let known = find(&b, account);
    if known.as_ref().is_some_and(|k| k.source == "ibkr") {
        return Err(t!("Interactive Brokers fills {account}. Unlink it there before keeping it another way.", account = target.name));
    }
    if !valid_date(&snap.as_of) {
        return Err(t!("Give the day these figures are from."));
    }
    let currency = if snap.currency.trim().is_empty() { b.currency.clone() } else { snap.currency.trim().to_ascii_uppercase() };
    let foreign = currency != b.currency;
    let rate_text = if foreign {
        Some(snap.rate.trim().replace(',', "."))
            .filter(|r| !r.is_empty())
            .or_else(|| known.as_ref().filter(|k| k.currency == currency).and_then(|k| k.manual_rate.clone()))
            .ok_or_else(|| t!("These figures are in {currency}, but this budget uses {budget}. Give what 1 {currency} is worth in {budget}.", currency, budget = b.currency))?
    } else {
        "1".to_string()
    };
    let rate = Dec::parse(&rate_text).filter(|r| !r.is_zero() && !r.is_negative()).ok_or_else(|| t!("Give the exchange rate as a number above zero."))?;
    // A figure of the source's, in the budget's currency, in hundredths; None for one left empty.
    let amount = |text: &str, of: &str| -> Result<Option<i64>, String> {
        if text.trim().is_empty() {
            return Ok(None);
        }
        let value = Dec::parse(text).ok_or_else(|| t!("UrBudget can't read {value} as an amount for {name}.", value = text.trim(), name = of))?;
        let cents = value.times(rate).cents();
        if valid_money(cents) { Ok(Some(cents)) } else { Err(unreadable()) }
    };
    let mut positions = vec![];
    for p in &snap.positions {
        let (symbol, name) = (flex::clean(&p.symbol), flex::clean(&p.name));
        let label = if name.is_empty() { symbol.as_str() } else { name.as_str() };
        if label.is_empty() {
            continue;
        }
        let value = amount(&p.value, label)?.ok_or_else(|| t!("UrBudget can't read {value} as an amount for {name}.", value = "", name = label))?;
        let cost = amount(&p.cost, label)?;
        let own = p.currency.trim().to_ascii_uppercase();
        let own = if own.is_empty() { currency.clone() } else { own };
        positions.push(Holding {
            symbol,
            name,
            kind: p.kind.trim().to_ascii_uppercase(),
            // The value as written is in its own currency only when that is the figures' currency.
            amount: if own == currency { flex::plain(&p.value).unwrap_or_default() } else { String::new() },
            currency: own,
            quantity: flex::clean(&p.quantity),
            price: flex::clean(&p.price),
            value,
            cost,
            gain: cost.map(|c| value - c),
            ..Holding::default()
        });
    }
    let invested: i64 = positions.iter().map(|p| p.value).sum();
    let said = amount(&snap.total, &target.name)?;
    let listed_cash = amount(&snap.cash, &target.name)?;
    let nothing_listed = positions.is_empty() && listed_cash.is_none();
    if nothing_listed && said.is_none() {
        return Err(t!("Give the account's value, or at least one holding with its value."));
    }
    // With holdings listed, what the source counts in the account beyond them is cash.
    let cash_value = if nothing_listed { 0 } else { said.map_or(listed_cash.unwrap_or(0), |total| total - invested) };
    let total = said.unwrap_or(invested + cash_value);
    if !valid_money(total) {
        return Err(unreadable());
    }
    let cash = if cash_value == 0 {
        vec![]
    } else {
        vec![CashHolding { currency: currency.clone(), amount: decimal(cash_value).over(rate).unwrap_or(Dec::ZERO).text(), value: cash_value }]
    };
    let mut values: BTreeMap<String, i64> = known.as_ref().map(|k| k.values.iter().cloned().collect()).unwrap_or_default();
    values.insert(snap.as_of.clone(), total);
    let latest = values.keys().next_back().cloned().unwrap_or_default();
    let values = thin(values, &latest);
    let now = stamp_now();
    let brought = Holdings {
        extra: known.as_ref().map(|k| k.extra.clone()).unwrap_or_default(),
        account_id: account.to_owned(),
        source: snap.source.clone(),
        source_account: flex::clean(&snap.source_account),
        as_of: snap.as_of.clone(),
        currency: currency.clone(),
        rate: rate.text(),
        manual_rate: foreign.then(|| rate_text.clone()),
        total,
        positions,
        cash,
        other: 0,
        values: vec![],
        flows: None,
        estimated: vec![],
        movements: vec![],
        movements_from: String::new(),
        income: vec![],
        commissions: None,
        trades: vec![],
        income_by: vec![],
        synced_at: now.clone(),
        missing: vec![],
        snapshots: vec![],
    };
    // Its month keeps what these figures say it was made of, whichever day they are from.
    let made_of = month_end(&brought);
    let mut next = match &known {
        // An earlier day's figures add to the history and leave what it holds now as it is.
        Some(k) if snap.as_of < latest => Holdings { values, synced_at: now, ..k.clone() },
        _ => Holdings { values, snapshots: known.as_ref().map(|k| k.snapshots.clone()).unwrap_or_default(), ..brought },
    };
    keep_month(&mut next.snapshots, made_of);
    // Figures that say what the last ones did leave the budget exactly as it was, so a sync
    // that brings nothing new isn't a change to save, undo or upload.
    if let Some(k) = known {
        if (Holdings { synced_at: k.synced_at.clone(), ..next.clone() }) == k {
            next = k;
        }
    }
    store(&mut b, next);
    revalue(&mut b, account)?;
    validate(&b)?;
    Ok(b)
}

/// The row a movement is in the review: the broker's id tells one already brought in, and
/// `other` is the budget account on its other side when that is known.
fn row(b: &Budget, held: &Holdings, m: &Movement, other: Option<String>) -> (BankTransaction, i64) {
    let fit = if m.id.is_empty() {
        format!("ibkr:{}:{}:{}:{}", held.source_account, m.date, m.amount, m.currency)
    } else {
        format!("ibkr:{}:{}", held.source_account, m.id)
    };
    let foreign = m.currency != b.currency;
    let worth = foreign.then(|| format!("{} {}", m.amount, m.currency));
    // The other account at the broker, by its number, when the budget doesn't hold it.
    let elsewhere = (other.is_none() && !m.counter.is_empty()).then(|| m.counter.clone());
    let memo = [Some(m.memo.clone()), worth, elsewhere].into_iter().flatten().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ");
    let near = if foreign { (m.value.abs() / NEAR_PARTS.1 * NEAR_PARTS.0).max(100) } else { 0 };
    (BankTransaction { fit, date: m.date.clone(), payee: "Interactive Brokers".into(), memo, amount: m.value, transfer_account_id: other }, near)
}
/// Which movements are the two sides of one transfer between broker accounts the budget holds,
/// each by its place (the holdings' and the movement's) and both ways round: the same amount
/// in the same currency going opposite ways within a few days, where neither statement names
/// a different account. Ones whose statements name each other pair first, then the nearest.
fn partners(all: &[Holdings]) -> HashMap<(usize, usize), (usize, usize)> {
    // How many sides don't name the other, days apart, then the places.
    let mut found: Vec<(u8, i64, usize, usize, usize, usize)> = vec![];
    for (a, from) in all.iter().enumerate() {
        for (i, out) in from.movements.iter().enumerate().filter(|(_, m)| m.transfer && m.value < 0) {
            let Some(sent) = Dec::parse(&out.amount) else { continue };
            for (b, to) in all.iter().enumerate().filter(|(b, to)| *b != a && to.source == from.source) {
                if !(out.counter.is_empty() || out.counter == to.source_account) {
                    continue;
                }
                for (j, came) in to.movements.iter().enumerate().filter(|(_, m)| m.transfer && m.value > 0 && m.currency == out.currency) {
                    let apart = distance(&out.date, &came.date);
                    if apart <= PAIR_DAYS && (came.counter.is_empty() || came.counter == from.source_account) && Dec::parse(&came.amount) == Some(sent.negated()) {
                        found.push((out.counter.is_empty() as u8 + came.counter.is_empty() as u8, apart, a, i, b, j));
                    }
                }
            }
        }
    }
    found.sort();
    let mut pairs = HashMap::new();
    for (_, _, a, i, b, j) in found {
        if !pairs.contains_key(&(a, i)) && !pairs.contains_key(&(b, j)) {
            pairs.insert((a, i), (b, j));
            pairs.insert((b, j), (a, i));
        }
    }
    pairs
}
/// The rows of the account whose holdings are at `at`, one for each of its movements in order.
fn rows(b: &Budget, all: &[Holdings], pairs: &HashMap<(usize, usize), (usize, usize)>, at: usize) -> Vec<(BankTransaction, i64)> {
    let held = &all[at];
    let exists = |id: &String| b.accounts.iter().any(|a| a.id == *id);
    held.movements
        .iter()
        .enumerate()
        .map(|(i, m)| {
            // Its other side: the account its partner is in, else the one its statement names.
            let other = pairs
                .get(&(at, i))
                .map(|(other, _)| all[*other].account_id.clone())
                .or_else(|| all.iter().find(|h| !m.counter.is_empty() && h.source == held.source && h.source_account == m.counter && h.account_id != held.account_id).map(|h| h.account_id.clone()))
                .filter(exists);
            row(b, held, m, other)
        })
        .collect()
}
/// The deposits, withdrawals and transfers the broker listed for an account, as rows for its
/// review (`bank_import::review_near`), in the budget's currency: one already brought in is
/// known by the broker's id, a transfer with another broker account the budget holds names
/// that account, and one made in another currency says how far the bank's figure may be from
/// the broker's. Nothing for an account no broker fills.
pub fn pending(b: &Budget, account: &str) -> Vec<(BankTransaction, i64)> {
    let all = read(b);
    match all.iter().position(|h| h.account_id == account) {
        Some(at) => rows(b, &all, &partners(&all), at),
        None => vec![],
    }
}
/// The ids the review gives an account's rows, in order (see `bank_import::Counter`).
fn row_ids(account: &str, raw: &[(BankTransaction, i64)]) -> Vec<String> {
    let mut counter = bank_import::Counter::default();
    raw.iter().map(|(t, _)| bank_import::bank_id_of(account, &counter.identity(t, true).signature)).collect()
}

/// Brings reviewed rows into the budget like a bank's, then works every broker account's lines
/// out again, since money now in the budget is no longer stood in for. A choice says which row
/// (by its id) and what to do with it, and for a row added may name the account the money came
/// from or went to; the rest of the row is the broker's own, read again here. A transfer
/// between two broker accounts is brought in once: its row in the other account is marked as
/// this same transaction, so that review doesn't offer it again.
pub fn import(b: Budget, account: &str, choices: Vec<Choice>) -> Result<Budget, String> {
    let all = read(&b);
    let at = all.iter().position(|h| h.account_id == account).ok_or_else(|| t!("Account not found."))?;
    let pairs = partners(&all);
    let raw = rows(&b, &all, &pairs, at);
    let ids = row_ids(account, &raw);
    let current = bank_import::review_near(&b, account, raw)?;
    let changed = || t!("These deposits changed since you looked. Review them again.");
    let mut chosen = vec![];
    // The transaction a paired row becomes, with its other side's account and row.
    let mut joined: Vec<(String, String, String)> = vec![];
    for c in choices {
        if c.action == "skip" {
            continue;
        }
        let place = ids.iter().position(|id| *id == c.row.id).ok_or_else(changed)?;
        let mut row = current.iter().find(|r| r.id == c.row.id).cloned().ok_or_else(changed)?;
        if c.action == "new" {
            if row.transfer_account_id.is_none() {
                row.transfer_account_id = c.row.transfer_account_id.clone().filter(|other| !other.is_empty());
            }
            row.import_id = c.row.import_id.clone();
        }
        if let Some((other, theirs)) = pairs.get(&(at, place)) {
            let other_rows = rows(&b, &all, &pairs, *other);
            let their_id = row_ids(&all[*other].account_id, &other_rows).swap_remove(*theirs);
            let transaction = if c.action == "match" { c.match_id.clone() } else { Some(row.import_id.clone().unwrap_or_else(|| row.id.clone())) };
            if let Some(id) = transaction {
                joined.push((id, all[*other].account_id.clone(), their_id));
            }
        }
        chosen.push(Choice { row, action: c.action, match_id: c.match_id });
    }
    let mut b = bank_import::apply(b, account, chosen)?;
    for (id, other, their_id) in joined {
        let Some(t) = b.transactions.iter_mut().find(|t| t.id == id) else { continue };
        let ours = t.account_id == account && t.transfer_account_id.as_deref() == Some(other.as_str());
        let theirs = t.account_id == other && t.transfer_account_id.as_deref() == Some(account);
        if !ours && !theirs {
            continue;
        }
        if let Some(known) = t.extra.entry("bankImportIds").or_insert(json!([])).as_array_mut() {
            if !known.iter().any(|v| v.as_str() == Some(their_id.as_str())) {
                known.push(json!(their_id));
            }
        }
        if let Some(sides) = t.extra.entry("bankImportMatches").or_insert(json!({})).as_object_mut() {
            sides.entry(other.clone()).or_insert(json!(their_id));
        }
        // The other side is the broker's own record as well.
        if theirs {
            if t.status != Cleared::Reconciled {
                t.status = Cleared::Cleared;
            }
        } else if t.transfer_status != Some(Cleared::Reconciled) {
            t.transfer_status = Some(Cleared::Cleared);
        }
    }
    for held in read(&b) {
        revalue(&mut b, &held.account_id)?;
    }
    validate(&b)?;
    Ok(b)
}
/// Works every broker account's lines out again, for a budget that just changed some other way
/// (a transfer typed in, a transaction deleted): the account's balance stays the broker's
/// value. An account whose lines can't be written is left as it was.
pub fn settle(b: &mut Budget) {
    for held in read(b) {
        let _ = revalue(b, &held.account_id);
    }
}

/// What a fresh start keeps of the holdings: everything they hold now, and the values and
/// month snapshots from the last value on or before the fresh-start day, which the new opening
/// balance already equals.
pub fn after_fresh_start(list: &Value, as_of: &str) -> Value {
    let mut list = list.clone();
    for entry in list.as_array_mut().into_iter().flatten() {
        let Some(values) = entry.get_mut("values").and_then(Value::as_array_mut) else { continue };
        let day = |v: &Value| v.get(0).and_then(Value::as_str).unwrap_or("").to_owned();
        let anchor = values.iter().rposition(|v| day(v).as_str() <= as_of).unwrap_or(0);
        values.drain(..anchor);
        // What it was made of in the months before the one it now starts in goes with those values.
        let start = values.first().map(day).unwrap_or_default();
        let month = start.get(..7).unwrap_or("");
        if let Some(snapshots) = entry.get_mut("snapshots").and_then(Value::as_array_mut) {
            snapshots.retain(|s| s.get("month").and_then(Value::as_str).unwrap_or("") >= month);
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flex::tests::SAMPLE;

    fn budget(currency: &str) -> Budget {
        let account = |id: &str, kind| Account { extra: Map::new(), id: id.into(), name: id.into(), kind, opening_balance: 0, archived: false };
        let mut b = Budget { currency: currency.into(), ..Budget::default() };
        b.accounts = vec![account("bank", AccountKind::Cash), account("broker", AccountKind::Tracking)];
        b.accounts[1].extra.insert("accountType".into(), json!("investment"));
        b
    }
    fn statement() -> flex::Account {
        flex::parse(SAMPLE).unwrap().accounts.remove(0)
    }
    fn balance(b: &Budget, account: &str, through: &str) -> i64 {
        crate::ledger::transactions(b)
            .iter()
            .filter(|t| t.date.as_str() <= through)
            .map(|t| if t.account_id == account { t.amount } else if t.transfer_account_id.as_deref() == Some(account) { -t.amount } else { 0 })
            .sum()
    }
    fn written(b: &Budget) -> Vec<(String, String, i64)> {
        let mut out: Vec<_> = b.transactions.iter().filter(|t| generated(t)).map(|t| (t.id.clone(), t.date.clone(), t.amount)).collect();
        out.sort();
        out
    }
    fn deposit(b: &mut Budget, id: &str, date: &str, amount: i64) {
        b.transactions.push(Transaction {
            extra: Default::default(),
            id: id.into(),
            date: date.into(),
            account_id: "bank".into(),
            payee: String::new(),
            memo: String::new(),
            amount: -amount,
            status: Cleared::Cleared,
            splits: vec![Split { extra: Map::from_iter([("unassigned".to_string(), json!(true))]).into(), category_id: None, amount: -amount }],
            transfer_account_id: Some("broker".into()),
            transfer_status: Some(Cleared::Cleared),
        });
    }

    #[test]
    fn a_statement_in_the_budgets_currency_fills_the_account() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let h = find(&b, "broker").unwrap();
        assert_eq!((h.source.as_str(), h.source_account.as_str(), h.as_of.as_str(), h.currency.as_str(), h.rate.as_str()), ("ibkr", "U1234567", "2026-11-13", "USD", "1"));
        assert_eq!(h.total, 2_701_250);
        // Largest first; the Saudi stock is converted, the rest are already in dollars.
        let rows: Vec<(&str, &str, &str, i64, Option<i64>)> = h.positions.iter().map(|p| (p.symbol.as_str(), p.quantity.as_str(), p.price.as_str(), p.value, p.gain)).collect();
        assert_eq!(rows, vec![
            ("VWRA", "120.5", "141.9087137", 1_710_000, Some(210_000)),
            ("AAPL", "25", "232", 580_000, Some(130_000)),
            ("2222", "400", "27", 288_000, Some(-32_000)),
        ]);
        assert_eq!(h.cash.iter().map(|c| (c.currency.as_str(), c.amount.as_str(), c.value)).collect::<Vec<_>>(), vec![("USD", "820", 82_000), ("SAR", "1500", 40_000)]);
        // The accrued dividend is in the value and in neither a position nor cash.
        assert_eq!(h.other, 1_250);
        assert_eq!(h.manual_rate, None);
        assert_eq!(balance(&b, "broker", "9999-12-31"), h.total);
        assert_eq!(b.extra[KEY].as_array().unwrap().len(), 1);
    }

    #[test]
    fn what_it_earned_and_was_charged_is_kept_by_month_with_the_brokers_deposits() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let h = find(&b, "broker").unwrap();
        // The broker's own record of money put in, whether or not the budget has it yet.
        assert_eq!(h.flows, Some(vec![("2026-10-05".to_string(), 100_000), ("2026-11-02".to_string(), 40_000)]));
        assert!(h.estimated.is_empty());
        assert_eq!(h.income, vec![
            MonthIncome { month: "2026-10".into(), interest: 120, fees: -1_000, ..MonthIncome::default() },
            MonthIncome { month: "2026-11".into(), dividends: 650, tax: -98, ..MonthIncome::default() },
        ]);
        assert_eq!(h.commissions, Some(Commissions { from: "2026-09-01".into(), to: "2026-11-13".into(), amount: -350 }));
        // A later statement that starts partway through November keeps November as it was,
        // adds December, and leaves the earlier deposit alone.
        let mut later = statement();
        later.from = "2026-11-10".into();
        later.to = "2026-12-31".into();
        later.values = vec![("2026-12-31".into(), Dec::parse("28000").unwrap())];
        later.flows.retain(|f| f.date.as_str() >= "2026-11-10");
        later.income = vec![flex::Income { date: "2026-12-04".into(), kind: flex::IncomeKind::Dividends, currency: "USD".into(), amount: Dec::parse("7").unwrap(), fx: None, security: Default::default() }];
        later.commissions = None;
        let h = find(&apply(b, "broker", &later, None).unwrap(), "broker").unwrap();
        assert_eq!(h.flows, Some(vec![("2026-10-05".to_string(), 100_000), ("2026-11-02".to_string(), 40_000)]));
        assert_eq!(h.income.iter().map(|m| (m.month.as_str(), m.dividends, m.tax, m.fees)).collect::<Vec<_>>(),
            vec![("2026-10", 0, 0, -1_000), ("2026-11", 650, -98, 0), ("2026-12", 700, 0, 0)]);
        assert_eq!(h.commissions.unwrap().amount, -350);
        // An entry written before these were kept still reads.
        let old: Holdings = serde_json::from_value(json!({"accountId":"a","source":"ibkr","sourceAccount":"U1","asOf":"2026-11-13","currency":"USD","rate":"1","total":1,"positions":[],"cash":[],"values":[],"syncedAt":""})).unwrap();
        assert!(old.flows.is_none() && old.income.is_empty() && old.commissions.is_none());
    }

    #[test]
    fn the_balance_follows_the_brokers_value_month_by_month() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        // The first valued day starts the account. Each month then has its gain, and beside it
        // the money the broker says was put in that the budget doesn't have yet.
        assert_eq!(written(&b), vec![
            ("investment-change:broker:2026-10".to_string(), "2026-10-30".to_string(), 28_000),
            ("investment-change:broker:2026-11".to_string(), "2026-11-13".to_string(), 111_250),
            ("investment-moved:broker:2026-10".to_string(), "2026-10-30".to_string(), 100_000),
            ("investment-moved:broker:2026-11".to_string(), "2026-11-13".to_string(), 40_000),
            ("investment-start:broker".to_string(), "2026-09-30".to_string(), 2_422_000),
        ]);
        let payees: Vec<&str> = b.transactions.iter().filter(|t| generated(t)).map(|t| t.payee.as_str()).collect();
        assert!(payees.contains(&"Money moved in or out") && payees.contains(&"Market change") && payees.contains(&"Starting value"));
        for (day, value) in [("2026-09-30", 2_422_000), ("2026-10-30", 2_550_000), ("2026-11-13", 2_701_250)] {
            assert_eq!(balance(&b, "broker", day), value, "{day}");
        }
        // A sync that brings the same statement changes nothing at all.
        let again = apply(b.clone(), "broker", &statement(), None).unwrap();
        assert_eq!(again, b);
        assert!(crate::patch::Patch::diff(&b, &again).is_empty());
    }

    #[test]
    fn money_put_in_is_not_a_gain() {
        let mut b = budget("USD");
        deposit(&mut b, "d1", "2026-10-05", 100_000);
        deposit(&mut b, "d2", "2026-11-02", 40_000);
        let b = apply(b, "broker", &statement(), None).unwrap();
        // October rose 1,280 of which 1,000 was a deposit; November 1,512.50 of which 400. With
        // both deposits in the budget, nothing is left to stand in for them.
        let lines = |b: &Budget, kind: &str| written(b).iter().filter(|w| w.0.contains(kind)).map(|w| w.2).collect::<Vec<i64>>();
        assert_eq!(lines(&b, "change"), vec![28_000, 111_250]);
        assert!(lines(&b, "moved").is_empty());
        assert_eq!(balance(&b, "broker", "9999-12-31"), 2_701_250);
        // One taken out of the budget again is stood in for, and the gain doesn't move.
        let mut later = b.clone();
        later.transactions.retain(|t| t.id != "d2");
        revalue(&mut later, "broker").unwrap();
        assert_eq!(lines(&later, "change"), vec![28_000, 111_250]);
        assert_eq!(lines(&later, "moved"), vec![40_000]);
        assert_eq!(balance(&later, "broker", "9999-12-31"), 2_701_250);
        // One entered for a different amount than the broker's leaves the difference standing.
        deposit(&mut later, "d3", "2026-11-03", 39_000);
        revalue(&mut later, "broker").unwrap();
        assert_eq!(lines(&later, "moved"), vec![1_000]);
        assert_eq!(balance(&later, "broker", "9999-12-31"), 2_701_250);
        // Holdings from before the broker's flows were kept: the change is what the budget leaves.
        let mut old = b.clone();
        old.transactions.retain(|t| t.id != "d1");
        old.extra[KEY][0].as_object_mut().unwrap().remove("flows");
        revalue(&mut old, "broker").unwrap();
        assert_eq!(lines(&old, "change"), vec![128_000, 111_250]);
        assert!(lines(&old, "moved").is_empty());
    }

    #[test]
    fn an_account_with_a_balance_already_is_corrected_not_doubled() {
        let mut b = budget("USD");
        b.accounts[1].opening_balance = 2_000_000;
        let b = apply(b, "broker", &statement(), None).unwrap();
        assert_eq!(written(&b).iter().find(|w| w.0.starts_with("investment-start")).unwrap().2, 422_000);
        assert_eq!(b.accounts[1].opening_balance + balance(&b, "broker", "9999-12-31"), 2_701_250);
    }

    #[test]
    fn a_budget_in_another_currency_uses_the_statements_own_rates() {
        let b = apply(budget("SAR"), "broker", &statement(), None).unwrap();
        let h = find(&b, "broker").unwrap();
        // 27,012.50 dollars at 1 / 0.26666667.
        assert_eq!(h.rate, "3.74999995");
        assert_eq!(h.total, 10_129_687);
        // The Saudi stock and the riyal cash are already in the budget's currency: exact.
        let aramco = h.positions.iter().find(|p| p.symbol == "2222").unwrap();
        assert_eq!((aramco.value, aramco.cost, aramco.gain), (1_080_000, Some(1_200_000), Some(-120_000)));
        assert_eq!(h.cash.iter().find(|c| c.currency == "SAR").unwrap().value, 150_000);
        assert_eq!(h.manual_rate, None);
        assert!(needs_rate(&statement(), "SAR", None).is_none());
        // A riyal deposit keeps its amount to the halala, so it matches the bank's side.
        // The dollar one is worth what the broker's rate says, so the bank's side may be a little off.
        let rows = pending(&b, "broker");
        assert_eq!(rows.iter().map(|(r, near)| (r.fit.as_str(), r.date.as_str(), r.amount, *near)).collect::<Vec<_>>(), vec![
            ("ibkr:U1234567:9000001", "2026-10-05", 375_000, 11_250),
            ("ibkr:U1234567:9000002", "2026-11-02", 150_000, 0),
        ]);
        assert_eq!(rows[0].0.memo, "CASH RECEIPTS / ELECTRONIC FUND TRANSFERS · 1000 USD");
        assert_eq!(rows[1].0.memo, "CASH RECEIPTS / ELECTRONIC FUND TRANSFERS");
    }

    #[test]
    fn a_query_without_the_rows_own_rates_uses_the_statements_or_says_what_to_tick() {
        // The FX Rate to Base field left out of positions and cash transactions.
        let narrow = SAMPLE.replace(r#" fxRateToBase="0.26666667""#, "").replace(r#" fxRateToBase="1""#, "");
        let s = flex::parse(&narrow).unwrap().accounts.remove(0);
        let b = apply(budget("USD"), "broker", &s, None).unwrap();
        // The riyal stock converts with the statement's currency rates, not at one to one.
        assert_eq!(find(&b, "broker").unwrap().positions.iter().find(|p| p.symbol == "2222").unwrap().value, 288_000);
        assert_eq!(pending(&b, "broker").iter().map(|(r, _)| r.amount).collect::<Vec<_>>(), vec![100_000, 40_000]);
        // With no currency rates either, nothing is guessed: the message says what to change.
        let bare: String = narrow.lines().filter(|l| !l.contains("<ConversionRate ")).collect::<Vec<_>>().join("\n");
        let s = flex::parse(&bare).unwrap().accounts.remove(0);
        let refused = apply(budget("USD"), "broker", &s, None).unwrap_err();
        assert!(refused.contains("what SAR is worth in USD") && refused.contains("Include Currency Rates"), "{refused}");
        // Holdings all in the account's own currency need no rate at all.
        let mut plain = s.clone();
        plain.positions.retain(|p| p.currency == "USD");
        plain.cash.retain(|c| c.currency == "USD");
        plain.flows.retain(|f| f.currency == "USD");
        let b = apply(budget("USD"), "broker", &plain, None).unwrap();
        assert_eq!(find(&b, "broker").unwrap().positions.len(), 2);
        assert_eq!(pending(&b, "broker").len(), 1);
    }

    #[test]
    fn a_jump_nothing_explains_is_money_moved_not_a_gain() {
        // The account is emptied, sits at nothing, and comes back larger, as a transfer to and
        // from another account looks in a statement whose query has no Transfers section.
        let mut s = statement();
        s.flows.clear();
        s.missing = vec!["Transfers"];
        s.values = [("2026-09-30", "24000"), ("2026-10-01", "0"), ("2026-10-02", "0"), ("2026-10-05", "30000"), ("2026-10-06", "30600"), ("2026-11-13", "31000")]
            .into_iter()
            .map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap()))
            .collect();
        let b = apply(budget("USD"), "broker", &s, None).unwrap();
        let h = find(&b, "broker").unwrap();
        assert_eq!(h.flows, Some(vec![("2026-10-01".to_string(), -2_400_000), ("2026-10-05".to_string(), 3_000_000)]));
        assert_eq!(h.estimated, vec!["2026-10-01", "2026-10-05"]);
        // Two transfers three days apart each explain their own day: neither is a guess, and the
        // flows are the transfers' own amounts, not the days' moves.
        let mut pair = statement();
        pair.flows.retain(|f| f.date == "2026-10-05");
        let second = flex::Flow { date: "2026-10-08".into(), amount: Dec::parse("9000").unwrap(), ..pair.flows[0].clone() };
        pair.flows[0].amount = Dec::parse("6000").unwrap();
        pair.flows.push(second);
        pair.values = [("2026-10-02", "24000"), ("2026-10-05", "30010"), ("2026-10-06", "30050"), ("2026-10-07", "30000"), ("2026-10-08", "39020"), ("2026-11-13", "39500")]
            .into_iter()
            .map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap()))
            .collect();
        let both = find(&apply(budget("USD"), "broker", &pair, None).unwrap(), "broker").unwrap();
        assert!(both.estimated.is_empty());
        assert_eq!(both.flows, Some(vec![("2026-10-05".to_string(), 600_000), ("2026-10-08".to_string(), 900_000)]));
        // A twentieth of the account in a day is enough to count, and a smaller move isn't.
        let mut small = statement();
        small.flows.clear();
        small.missing = vec!["Transfers"];
        small.values = [("2026-11-10", "20000"), ("2026-11-11", "21100"), ("2026-11-12", "21900"), ("2026-11-13", "20700")]
            .into_iter()
            .map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap()))
            .collect();
        let judged = find(&apply(budget("USD"), "broker", &small, None).unwrap(), "broker").unwrap();
        assert_eq!(judged.estimated, vec!["2026-11-11", "2026-11-13"]);
        // Only the ordinary days' moves are gains: 600 in October, 400 in November.
        let lines = |kind: &str| written(&b).iter().filter(|w| w.0.contains(kind)).map(|w| w.2).collect::<Vec<i64>>();
        assert_eq!(lines("change"), vec![60_000, 40_000]);
        assert_eq!(lines("moved"), vec![600_000]);
        assert_eq!(balance(&b, "broker", "9999-12-31"), 3_100_000);
        // A deposit listed a day before the value shows it explains the jump: nothing is guessed.
        let mut listed = statement();
        listed.flows.retain(|f| f.date == "2026-10-05");
        listed.flows[0].amount = Dec::parse("6000").unwrap();
        listed.values = [("2026-09-30", "24000"), ("2026-10-05", "24100"), ("2026-10-06", "30200"), ("2026-11-13", "30500")]
            .into_iter()
            .map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap()))
            .collect();
        let h = find(&apply(budget("USD"), "broker", &listed, None).unwrap(), "broker").unwrap();
        assert!(h.estimated.is_empty());
        // It counts on the day the value shows it, so neither day reads as a loss or a gain of it.
        assert_eq!(h.flows, Some(vec![("2026-10-06".to_string(), 600_000)]));
        // A wire dated when it was sent and counted days later, over a weekend, explains the jump
        // it makes: the deposit is counted once, not again as a guess (a riyal wire dated the 30th
        // that showed in the value on the 5th was once counted twice).
        let mut wired = statement();
        wired.flows.retain(|f| f.date == "2026-10-05");
        wired.flows[0].date = "2026-09-30".into();
        wired.flows[0].amount = Dec::parse("6000").unwrap();
        let second = flex::Flow { date: "2026-10-12".into(), ..wired.flows[0].clone() };
        wired.flows.push(second);
        wired.values = [("2026-09-29", "24000"), ("2026-09-30", "24050"), ("2026-10-02", "24000"), ("2026-10-05", "29950"), ("2026-10-09", "30000"), ("2026-10-12", "36000"), ("2026-11-13", "36100")]
            .into_iter()
            .map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap()))
            .collect();
        let h = find(&apply(budget("USD"), "broker", &wired, None).unwrap(), "broker").unwrap();
        assert!(h.estimated.is_empty(), "{:?}", h.estimated);
        assert_eq!(h.flows, Some(vec![("2026-10-05".to_string(), 600_000), ("2026-10-12".to_string(), 600_000)]));
        // The movement itself keeps the broker's date, for the review.
        assert!(h.movements.iter().any(|m| m.date == "2026-09-30"));
        // One deposit explains one jump: a second jump of the same size days later, with nothing
        // listed for it, is still money moved.
        let mut twice = statement();
        twice.missing = vec!["Transfers"];
        twice.flows.retain(|f| f.date == "2026-10-05");
        twice.flows[0].amount = Dec::parse("6000").unwrap();
        twice.values = [("2026-10-02", "24000"), ("2026-10-05", "30000"), ("2026-10-06", "30000"), ("2026-10-07", "36000"), ("2026-11-13", "36100")]
            .into_iter()
            .map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap()))
            .collect();
        let h = find(&apply(budget("USD"), "broker", &twice, None).unwrap(), "broker").unwrap();
        assert_eq!(h.estimated, vec!["2026-10-07"]);
        // A statement that covers those days again replaces the estimates with what it says.
        let again = find(&apply(b, "broker", &listed, None).unwrap(), "broker").unwrap();
        assert!(again.estimated.is_empty());
        assert_eq!(again.flows, Some(vec![("2026-10-06".to_string(), 600_000)]));
    }

    /// A transaction in the bank account as a bank file leaves it.
    fn entry(b: &mut Budget, id: &str, date: &str, payee: &str, amount: i64, category: Option<&str>) {
        b.transactions.push(Transaction {
            extra: Default::default(),
            id: id.into(),
            date: date.into(),
            account_id: "bank".into(),
            payee: payee.into(),
            memo: String::new(),
            amount,
            status: Cleared::Cleared,
            splits: vec![Split { extra: Default::default(), category_id: category.map(String::from), amount }],
            transfer_account_id: None,
            transfer_status: None,
        });
    }
    fn reviewed(b: &Budget, account: &str) -> Vec<bank_import::Row> {
        bank_import::review_near(b, account, pending(b, account)).unwrap()
    }
    fn choose(row: &bank_import::Row, action: &str, match_id: Option<&String>) -> Choice {
        Choice { row: row.clone(), action: action.into(), match_id: match_id.cloned() }
    }
    fn lines(b: &Budget, account: &str, kind: &str) -> Vec<i64> {
        written(b).iter().filter(|w| w.0.contains(kind) && w.0.contains(&format!(":{account}"))).map(|w| w.2).collect()
    }
    fn transfer(id: &str, date: &str, amount: &str, counter: &str) -> flex::Flow {
        flex::Flow { id: id.into(), date: date.into(), currency: "USD".into(), amount: Dec::parse(amount).unwrap(), fx: None, description: "Transfer".into(), transfer: true, counter: counter.into() }
    }

    #[test]
    fn what_the_broker_listed_is_kept_so_the_review_needs_only_the_budget() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let kept = |b: &Budget| find(b, "broker").unwrap().movements.iter().map(|m| (m.id.clone(), m.date.clone(), m.currency.clone(), m.amount.clone(), m.value, m.transfer)).collect::<Vec<_>>();
        let both = vec![
            ("9000001".to_string(), "2026-10-05".to_string(), "USD".to_string(), "1000".to_string(), 100_000, false),
            ("9000002".to_string(), "2026-11-02".to_string(), "SAR".to_string(), "1500".to_string(), 40_000, false),
        ];
        assert_eq!(kept(&b), both);
        // The riyal one is worth what the broker's rate says, so its other side may be a little off.
        let rows = reviewed(&b, "broker");
        assert_eq!(rows.iter().map(|r| (r.date.as_str(), r.amount, r.near, r.duplicate)).collect::<Vec<_>>(), vec![("2026-10-05", 100_000, 0, false), ("2026-11-02", 40_000, 1_200, false)]);
        assert_eq!(rows[0].payee, "Interactive Brokers");
        // A later statement that starts after them leaves them to review; one that covers their
        // days and no longer lists them takes them away.
        let mut later = statement();
        later.from = "2026-11-10".into();
        later.to = "2026-12-31".into();
        later.values = vec![("2026-12-31".into(), Dec::parse("28000").unwrap())];
        later.flows.clear();
        assert_eq!(kept(&apply(b.clone(), "broker", &later, None).unwrap()), both);
        let mut none = statement();
        none.flows.clear();
        assert!(kept(&apply(b.clone(), "broker", &none, None).unwrap()).is_empty());
        // Holdings from before they were kept read, with nothing to review until the next sync.
        let mut old = b.clone();
        old.extra[KEY][0].as_object_mut().unwrap().remove("movements");
        assert!(pending(&old, "broker").is_empty() && pending(&b, "bank").is_empty());
    }

    #[test]
    fn a_wire_the_broker_counts_days_later_is_in_transit_not_money_moved() {
        // The bank sends 1,000.00 on 30 September; the broker counts it on 5 October.
        let sent = |counted: &str, values: [(&str, &str); 6]| {
            let mut b = budget("USD");
            entry(&mut b, "wire", "2026-09-30", "", -100_000, None);
            let wire = b.transactions.iter_mut().find(|t| t.id == "wire").unwrap();
            wire.transfer_account_id = Some("broker".into());
            wire.transfer_status = Some(Cleared::Cleared);
            let mut s = statement();
            s.flows.retain(|f| f.date == "2026-10-05");
            s.flows[0].amount = Dec::parse(counted).unwrap();
            s.values = values.into_iter().map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap())).collect();
            apply(b, "broker", &s, None).unwrap()
        };
        let b = sent("1000", [("2026-09-29", "50"), ("2026-09-30", "50"), ("2026-10-02", "50"), ("2026-10-05", "1050"), ("2026-10-30", "1060"), ("2026-11-13", "1070")]);
        // Nothing takes it out of September and puts it back in October: the months' lines are
        // the market's alone, and October ends at the broker's value.
        assert!(lines(&b, "broker", "moved").is_empty(), "{:?}", written(&b));
        assert_eq!(lines(&b, "broker", "change"), vec![1_000, 1_000]);
        assert_eq!(balance(&b, "broker", "2026-10-30"), 106_000);
        // At the month's end it was on its way, so it is in the account's balance then.
        assert_eq!(balance(&b, "broker", "2026-09-30"), 105_000);
        // A little lost to the exchange rate on the way is the one line left, in the month it arrived.
        let b = sent("995", [("2026-09-29", "50"), ("2026-09-30", "50"), ("2026-10-02", "50"), ("2026-10-05", "1045"), ("2026-10-30", "1055"), ("2026-11-13", "1065")]);
        assert_eq!(lines(&b, "broker", "moved"), vec![-500]);
        assert_eq!(lines(&b, "broker", "change"), vec![1_000, 1_000]);
    }

    #[test]
    fn a_line_this_module_writes_is_never_offered_as_the_match() {
        // One deposit in the month, four days before its end: the line that stands in for it is
        // the same amount within the days a match may be apart.
        let mut s = statement();
        s.flows.retain(|f| f.date == "2026-10-05");
        s.flows[0].date = "2026-10-26".into();
        let b = apply(budget("USD"), "broker", &s, None).unwrap();
        assert!(written(&b).contains(&("investment-moved:broker:2026-10".to_string(), "2026-10-30".to_string(), 100_000)));
        let rows = reviewed(&b, "broker");
        assert_eq!((rows.len(), rows[0].candidates.len(), rows[0].duplicate), (1, 0, false));
    }

    #[test]
    fn the_banks_side_is_found_and_joined_as_a_transfer() {
        let mut b = budget("USD");
        b.categories.push(crate::domain::Category { id: "invest".into(), name: "Investing".into(), group: "Savings".into(), target: 0, extra: Default::default() });
        // The bank's text names the broker, whatever the account is called here.
        entry(&mut b, "wire", "2026-10-06", "INTERACTIVE BROKERS LLC", -100_000, None);
        // One a payee rule already filed, named in the bank's text; and rent of the same amount.
        entry(&mut b, "filed", "2026-11-03", "IBKR deposit", -40_000, Some("invest"));
        entry(&mut b, "rent", "2026-11-02", "Landlord", -40_000, Some("invest"));
        // The same amount with nothing in the text to say so is only suggested.
        entry(&mut b, "plain", "2026-10-04", "Wire out", -100_000, None);
        let b = apply(b, "broker", &statement(), None).unwrap();
        let rows = reviewed(&b, "broker");
        assert_eq!((rows[0].transfer_candidates.clone(), rows[0].transfer_sure), (vec!["wire".to_string()], true));
        assert_eq!((rows[1].transfer_candidates.clone(), rows[1].transfer_sure), (vec!["filed".to_string()], true));
        let mut without = b.clone();
        without.transactions.retain(|t| t.id != "wire");
        let unsure = reviewed(&without, "broker");
        assert_eq!((unsure[0].transfer_candidates.clone(), unsure[0].transfer_sure), (vec!["plain".to_string()], false));
        // Joined, each is one transfer: the bank's side keeps its category across the budget's
        // edge, nothing stands in for the money any more, and the balance is still the broker's.
        let b = import(b.clone(), "broker", vec![choose(&rows[0], "match", Some(&"wire".to_string())), choose(&rows[1], "match", Some(&"filed".to_string()))]).unwrap();
        let wire = b.transactions.iter().find(|t| t.id == "wire").unwrap();
        assert_eq!((wire.transfer_account_id.as_deref(), wire.amount, wire.payee.as_str()), (Some("broker"), -100_000, ""));
        let filed = b.transactions.iter().find(|t| t.id == "filed").unwrap();
        assert_eq!((filed.transfer_account_id.as_deref(), filed.splits[0].category_id.as_deref()), (Some("broker"), Some("invest")));
        assert!(lines(&b, "broker", "moved").is_empty());
        assert_eq!(lines(&b, "broker", "change"), vec![28_000, 111_250]);
        assert_eq!(balance(&b, "broker", "9999-12-31"), 2_701_250);
        assert!(reviewed(&b, "broker").iter().all(|r| r.duplicate));
    }

    #[test]
    fn money_sent_in_another_currency_matches_what_the_bank_took() {
        // A riyal budget: the broker's 1,000 dollars are worth 3,750.00 at its rate, and the
        // bank took 3,765.00 for them.
        let mut b = budget("SAR");
        entry(&mut b, "wire", "2026-10-05", "IBKR", -376_500, None);
        entry(&mut b, "far", "2026-10-05", "IBKR", -400_000, None);
        let b = apply(b, "broker", &statement(), None).unwrap();
        let rows = reviewed(&b, "broker");
        assert_eq!((rows[0].amount, rows[0].near, rows[0].transfer_candidates.clone(), rows[0].transfer_sure), (375_000, 11_250, vec!["wire".to_string()], true));
        // The riyal deposit is exact: nothing a little off is offered for it.
        assert_eq!((rows[1].near, rows[1].candidates.len()), (0, 0));
        let total = find(&b, "broker").unwrap().total;
        let b = import(b, "broker", vec![choose(&rows[0], "match", Some(&"wire".to_string()))]).unwrap();
        // The budget keeps what the bank took; the difference stands in the account, and the
        // gain is the broker's whatever the bank charged.
        let wire = b.transactions.iter().find(|t| t.id == "wire").unwrap();
        assert_eq!((wire.transfer_account_id.as_deref(), wire.amount), (Some("broker"), -376_500));
        assert_eq!(lines(&b, "broker", "moved"), vec![-1_500, 150_000]);
        assert_eq!(balance(&b, "broker", "9999-12-31"), total);
        // A transfer typed in for the bank's figure is the match in the account itself.
        let mut typed = apply(budget("SAR"), "broker", &statement(), None).unwrap();
        deposit(&mut typed, "typed", "2026-10-06", 376_500);
        let rows = reviewed(&typed, "broker");
        assert_eq!(rows[0].candidates, vec!["typed".to_string()]);
        let typed = import(typed, "broker", vec![choose(&rows[0], "match", Some(&"typed".to_string()))]).unwrap();
        assert!(reviewed(&typed, "broker")[0].duplicate);
    }

    #[test]
    fn a_transfer_between_two_broker_accounts_is_one_transfer() {
        let mut b = budget("USD");
        b.accounts.push(Account { id: "second".into(), name: "Trading".into(), ..b.accounts[1].clone() });
        let (mut ours, mut theirs) = (statement(), statement());
        ours.flows = vec![transfer("7001", "2026-10-20", "-500", "U7654321")];
        theirs.id = "U7654321".into();
        theirs.flows = vec![transfer("8001", "2026-10-21", "500", "U1234567")];
        let b = apply(apply(b, "broker", &ours, None).unwrap(), "second", &theirs, None).unwrap();
        // Each review names the other account, and until it's brought in each stands in for it.
        let (out, came) = (reviewed(&b, "broker"), reviewed(&b, "second"));
        assert_eq!((out[0].transfer_account_id.as_deref(), out[0].amount), (Some("second"), -50_000));
        assert_eq!((came[0].transfer_account_id.as_deref(), came[0].amount), (Some("broker"), 50_000));
        assert_eq!((lines(&b, "broker", "moved"), lines(&b, "second", "moved")), (vec![-50_000], vec![50_000]));
        // Brought in from either side it is one transfer, cleared on both, and the other
        // account's review no longer offers it.
        let b = import(b, "broker", vec![choose(&out[0], "new", None)]).unwrap();
        let moved: Vec<&Transaction> = b.transactions.iter().filter(|t| !generated(t)).collect();
        assert_eq!(moved.len(), 1);
        assert_eq!((moved[0].account_id.as_str(), moved[0].transfer_account_id.as_deref(), moved[0].amount), ("broker", Some("second"), -50_000));
        assert_eq!((moved[0].status, moved[0].transfer_status), (Cleared::Cleared, Some(Cleared::Cleared)));
        assert!(reviewed(&b, "second")[0].duplicate && reviewed(&b, "broker")[0].duplicate);
        assert!(lines(&b, "broker", "moved").is_empty() && lines(&b, "second", "moved").is_empty());
        for account in ["broker", "second"] {
            assert_eq!(balance(&b, account, "9999-12-31"), 2_701_250, "{account}");
        }
        // Statements that name no account still pair on the amount, currency and days.
        let mut unnamed = budget("USD");
        unnamed.accounts.push(Account { id: "second".into(), name: "Trading".into(), ..unnamed.accounts[1].clone() });
        ours.flows[0].counter.clear();
        theirs.flows[0].counter.clear();
        let unnamed = apply(apply(unnamed, "broker", &ours, None).unwrap(), "second", &theirs, None).unwrap();
        assert_eq!(pending(&unnamed, "second")[0].0.transfer_account_id.as_deref(), Some("broker"));
        // One that names an account the budget doesn't hold pairs with nothing, and says which.
        let mut elsewhere = budget("USD");
        elsewhere.accounts.push(Account { id: "second".into(), name: "Trading".into(), ..elsewhere.accounts[1].clone() });
        ours.flows[0].counter = "U9999999".into();
        let elsewhere = apply(apply(elsewhere, "broker", &ours, None).unwrap(), "second", &theirs, None).unwrap();
        let row = &pending(&elsewhere, "broker")[0].0;
        assert_eq!((row.transfer_account_id.as_deref(), row.memo.as_str()), (None, "Transfer · U9999999"));
        assert_eq!(pending(&elsewhere, "second")[0].0.transfer_account_id, None);
    }

    #[test]
    fn a_row_can_be_added_as_a_transfer_from_the_account_it_came_from() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let rows = reviewed(&b, "broker");
        let mut from_bank = choose(&rows[0], "new", None);
        from_bank.row.transfer_account_id = Some("bank".into());
        // Only the row's id and the account are taken from the choice: the amount is the broker's.
        from_bank.row.amount = 1;
        let b = import(b, "broker", vec![from_bank, choose(&rows[1], "skip", None)]).unwrap();
        let added: Vec<&Transaction> = b.transactions.iter().filter(|t| !generated(t)).collect();
        assert_eq!(added.len(), 1);
        assert_eq!((added[0].account_id.as_str(), added[0].transfer_account_id.as_deref(), added[0].amount, added[0].date.as_str()), ("broker", Some("bank"), 100_000, "2026-10-05"));
        assert_eq!(balance(&b, "bank", "9999-12-31"), -100_000);
        assert_eq!(lines(&b, "broker", "moved"), vec![40_000]);
        assert_eq!(balance(&b, "broker", "9999-12-31"), 2_701_250);
        // A row the budget no longer lists is refused, not guessed at.
        let mut gone = choose(&rows[1], "new", None);
        gone.row.id = "bank:not-a-row".into();
        assert!(import(b.clone(), "broker", vec![gone]).unwrap_err().contains("Review them again"));
        assert!(import(b, "bank", vec![]).is_err());
    }

    #[test]
    fn a_transfer_typed_in_takes_the_place_of_the_line_that_stood_in_for_it() {
        let mut b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        deposit(&mut b, "typed", "2026-10-05", 100_000);
        assert_eq!(balance(&b, "broker", "9999-12-31"), 2_801_250);
        settle(&mut b);
        assert_eq!(lines(&b, "broker", "moved"), vec![40_000]);
        assert_eq!(balance(&b, "broker", "9999-12-31"), 2_701_250);
        // Nothing changes when nothing needs to.
        let before = b.clone();
        settle(&mut b);
        assert_eq!(b, before);
    }

    #[test]
    fn a_currency_the_statement_has_no_rate_for_asks_once() {
        let s = statement();
        assert_eq!(needs_rate(&s, "JPY", None), Some(("USD".into(), "JPY".into())));
        assert!(apply(budget("JPY"), "broker", &s, None).unwrap_err().contains("USD to JPY"));
        let b = apply(budget("JPY"), "broker", &s, Some("150")).unwrap();
        let h = find(&b, "broker").unwrap();
        assert_eq!((h.rate.as_str(), h.manual_rate.as_deref(), h.total), ("150", Some("150"), 405_187_500));
        // The rate is remembered for the next sync, and a new one replaces it.
        assert_eq!(find(&apply(b.clone(), "broker", &s, None).unwrap(), "broker").unwrap().total, 405_187_500);
        assert_eq!(find(&apply(b, "broker", &s, Some("151")).unwrap(), "broker").unwrap().manual_rate.as_deref(), Some("151"));
        for bad in ["0", "-3", "abc"] {
            assert!(needs_rate(&s, "JPY", Some(bad)).is_some(), "{bad}");
        }
    }

    #[test]
    fn a_statement_without_values_or_cash_still_shows_what_it_holds() {
        let mut s = statement();
        s.values.clear();
        s.cash.clear();
        s.cash_base = None;
        let b = apply(budget("USD"), "broker", &s, None).unwrap();
        let h = find(&b, "broker").unwrap();
        assert_eq!((h.as_of.as_str(), h.total, h.other), ("2026-11-13", 2_578_000, 0));
        assert_eq!(h.values, vec![("2026-11-13".to_string(), 2_578_000)]);
        assert_eq!(written(&b), vec![("investment-start:broker".to_string(), "2026-11-13".to_string(), 2_578_000)]);
    }

    #[test]
    fn later_statements_add_days_and_keep_the_history() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let mut next = statement();
        next.values = vec![("2026-11-13".into(), Dec::parse("27012.5").unwrap()), ("2026-12-31".into(), Dec::parse("28000").unwrap())];
        next.to = "2026-12-31".into();
        let b = apply(b, "broker", &next, None).unwrap();
        let h = find(&b, "broker").unwrap();
        assert_eq!(h.values.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>(), vec!["2026-09-30", "2026-10-30", "2026-11-12", "2026-11-13", "2026-12-31"]);
        assert_eq!(written(&b).len(), 6);
        assert_eq!(balance(&b, "broker", "9999-12-31"), 2_800_000);
        // Older than the daily window, only each month's last value stays, and the first valued
        // day: the starting value and the first month's change are measured from it.
        let old: BTreeMap<String, i64> = [("2024-01-10", 1), ("2024-01-20", 6), ("2024-01-31", 2), ("2024-02-01", 3), ("2024-02-09", 7), ("2026-11-12", 4), ("2026-11-13", 5)]
            .into_iter()
            .map(|(d, v)| (d.to_string(), v))
            .collect();
        assert_eq!(thin(old, "2026-11-13").iter().map(|(_, v)| *v).collect::<Vec<_>>(), vec![1, 2, 7, 4, 5]);
    }

    #[test]
    fn an_older_statement_adds_to_the_history_and_leaves_what_is_held_now() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let now = find(&b, "broker").unwrap();
        // A statement saved in early October, brought in after the November sync.
        let mut old = statement();
        old.to = "2026-10-03".into();
        old.values = vec![("2026-10-01".into(), Dec::parse("24100").unwrap()), ("2026-10-02".into(), Dec::parse("24150").unwrap())];
        old.positions.truncate(1);
        let b = apply(b, "broker", &old, None).unwrap();
        let h = find(&b, "broker").unwrap();
        // What it holds now, and as of when, are November's; October's days join the history.
        assert_eq!((h.as_of.as_str(), h.total, h.positions.len(), &h.cash), (now.as_of.as_str(), now.total, now.positions.len(), &now.cash));
        assert!(h.values.iter().any(|(d, _)| d == "2026-10-02") && h.values.last() == now.values.last());
        assert_eq!(balance(&b, "broker", "9999-12-31"), now.total);
    }

    #[test]
    fn only_a_tracking_account_and_only_one_takes_a_broker_account() {
        let s = statement();
        assert!(apply(budget("USD"), "bank", &s, None).unwrap_err().contains("tracking account"));
        assert!(apply(budget("USD"), "gone", &s, None).is_err());
        let mut b = apply(budget("USD"), "broker", &s, None).unwrap();
        let second = new_account(&b, "second", &s, None);
        assert_eq!((second.name.as_str(), second.kind, second.extra["accountType"].as_str()), ("Long term", AccountKind::Tracking, Some("investment")));
        b.accounts.push(second);
        assert!(apply(b.clone(), "second", &s, None).unwrap_err().contains("already fills broker"));
        // Unlinking keeps the history; the same broker account can then fill another.
        unlink(&mut b, "broker");
        assert!(b.extra.get(KEY).is_none());
        assert_eq!(written(&b).len(), 5);
        assert!(apply(b, "second", &s, None).is_ok());
        let mut plain = s.clone();
        plain.alias.clear();
        let named = budget("USD");
        assert_eq!(new_account(&named, "x", &plain, None).name, "Interactive Brokers …4567");
        assert_eq!(new_account(&named, "x", &plain, Some("broker")).name, "broker 2");
    }

    #[test]
    fn a_fresh_start_keeps_the_value_it_starts_from() {
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let kept = after_fresh_start(&b.extra[KEY], "2026-11-01");
        let days: Vec<&str> = kept[0]["values"].as_array().unwrap().iter().map(|v| v[0].as_str().unwrap()).collect();
        assert_eq!(days, vec!["2026-10-30", "2026-11-12", "2026-11-13"]);
        // The new budget opens at that value, so only what came after is a change.
        let mut fresh = budget("USD");
        fresh.accounts[1].opening_balance = 2_550_000;
        fresh.extra.insert(KEY.into(), kept);
        revalue(&mut fresh, "broker").unwrap();
        assert_eq!(written(&fresh), vec![
            ("investment-change:broker:2026-11".to_string(), "2026-11-13".to_string(), 111_250),
            ("investment-moved:broker:2026-11".to_string(), "2026-11-13".to_string(), 40_000),
        ]);
    }

    fn typed(date: &str, value: &str) -> Snapshot {
        Snapshot { source: "manual".into(), as_of: date.into(), total: value.into(), ..Snapshot::default() }
    }
    fn held(symbol: &str, name: &str, kind: &str, value: &str, cost: &str) -> Position {
        Position { symbol: symbol.into(), name: name.into(), kind: kind.into(), quantity: "10".into(), price: "1".into(), value: value.into(), cost: cost.into(), ..Position::default() }
    }

    #[test]
    fn a_value_typed_in_keeps_the_balance_there_and_the_rest_is_the_market() {
        let mut b = budget("USD");
        deposit(&mut b, "d1", "2026-10-05", 900_000);
        // The first value: what the budget hadn't recorded of it is where tracking starts.
        let b = record(b, "broker", typed("2026-10-31", "10,000.00")).unwrap();
        assert_eq!(balance(&b, "broker", "2026-10-31"), 1_000_000);
        assert_eq!(written(&b), vec![("investment-start:broker".to_string(), "2026-10-31".to_string(), 100_000)]);
        // A month on, with more put in: what the value moved by beyond that is the market's.
        let mut b = b;
        deposit(&mut b, "d2", "2026-11-10", 30_000);
        let b = record(b, "broker", typed("2026-11-30", "10500")).unwrap();
        assert_eq!(balance(&b, "broker", "2026-11-30"), 1_050_000);
        assert_eq!(lines(&b, "broker", "change"), vec![20_000]);
        let kept = find(&b, "broker").unwrap();
        assert_eq!((kept.source.as_str(), kept.as_of.as_str(), kept.total, kept.positions.len(), kept.cash.len(), kept.other), ("manual", "2026-11-30", 1_050_000, 0, 0, 0));
        assert_eq!(kept.values, vec![("2026-10-31".to_string(), 1_000_000), ("2026-11-30".to_string(), 1_050_000)]);
        // An earlier month filled in afterwards adds to the history and leaves today's figures.
        let b = record(b, "broker", typed("2026-09-30", "950")).unwrap();
        let kept = find(&b, "broker").unwrap();
        assert_eq!((kept.as_of.as_str(), kept.total, kept.values.len()), ("2026-11-30", 1_050_000, 3));
        assert_eq!(balance(&b, "broker", "2026-09-30"), 95_000);
        assert_eq!(balance(&b, "broker", "2026-11-30"), 1_050_000);
        // October now runs from September's value: the 9,000.00 put in explains all but 50.00 of its rise.
        assert_eq!(written(&b).iter().map(|w| w.2).collect::<Vec<_>>(), vec![5_000, 20_000, 95_000]);
        // The same value again is no change at all, and a transaction typed later moves the market's share.
        assert_eq!(record(b.clone(), "broker", typed("2026-11-30", "10500.00")).unwrap(), b);
        let mut b = b;
        deposit(&mut b, "d3", "2026-11-20", 5_000);
        settle(&mut b);
        assert_eq!(balance(&b, "broker", "2026-11-30"), 1_050_000);
        assert_eq!(written(&b).iter().map(|w| w.2).collect::<Vec<_>>(), vec![5_000, 15_000, 95_000]);
        // The page reads it as any other account's: the gain is what the deposits don't explain.
        let page = crate::investment_stats::page(&b, "broker").unwrap();
        assert_eq!((page.source.as_str(), page.total, page.holdings.len()), ("manual", 1_050_000, 0));
        assert_eq!(page.periods.last().map(|p| (p.did.put, p.did.gain)), Some((935_000, 20_000)));
    }

    #[test]
    fn a_files_holdings_are_kept_with_their_cost_and_what_is_left_is_cash() {
        let snap = Snapshot {
            source: "file".into(),
            source_account: "positions.csv".into(),
            as_of: "2026-11-13".into(),
            positions: vec![held("VWRA", "World fund", "fund", "17100", "15000"), held("T", "", "BOND", "4,925.00", ""), held("", "", "", "5", "")],
            cash: "820".into(),
            ..Snapshot::default()
        };
        let b = record(budget("USD"), "broker", snap.clone()).unwrap();
        let kept = find(&b, "broker").unwrap();
        assert_eq!(kept.positions.iter().map(|p| (p.symbol.as_str(), p.kind.as_str(), p.value, p.cost, p.gain)).collect::<Vec<_>>(), vec![
            ("VWRA", "FUND", 1_710_000, Some(1_500_000), Some(210_000)),
            // No cost from the file is no gain, not a gain of its whole value; and one with no name is passed over.
            ("T", "BOND", 492_500, None, None),
        ]);
        assert_eq!((kept.total, kept.cash[0].value, kept.cash[0].amount.as_str(), kept.flows.is_none()), (2_284_500, 82_000, "820", true));
        assert_eq!(balance(&b, "broker", "2026-11-13"), 2_284_500);
        // A whole value the source gives is the account's: what it counts beyond the holdings is cash.
        let b = record(b, "broker", Snapshot { as_of: "2026-11-14".into(), cash: String::new(), total: "23,000".into(), ..snap.clone() }).unwrap();
        let kept = find(&b, "broker").unwrap();
        assert_eq!((kept.total, kept.cash[0].value), (2_300_000, 2_300_000 - 1_710_000 - 492_500));
        // Figures in another currency need its rate once; it is remembered after.
        let foreign = Snapshot { currency: "eur".into(), ..snap.clone() };
        assert!(record(budget("USD"), "broker", foreign.clone()).unwrap_err().contains("Give what 1 EUR is worth in USD"));
        let b = record(budget("USD"), "broker", Snapshot { rate: "1,1".into(), ..foreign.clone() }).unwrap();
        let kept = find(&b, "broker").unwrap();
        assert_eq!((kept.currency.as_str(), kept.manual_rate.as_deref(), kept.positions[0].value, kept.cash[0].value, kept.cash[0].amount.as_str()), ("EUR", Some("1.1"), 1_881_000, 90_200, "820"));
        let b = record(b, "broker", Snapshot { as_of: "2026-11-20".into(), ..foreign }).unwrap();
        assert_eq!(find(&b, "broker").unwrap().total, 2_512_950);
    }

    #[test]
    fn what_an_account_was_made_of_is_kept_once_a_month_as_it_was_last_brought_in() {
        let months = |b: &Budget| find(b, "broker").unwrap().snapshots.iter().map(|s| (s.month.clone(), s.as_of.clone(), s.total)).collect::<Vec<_>>();
        let kept = |rows: &[(&str, &str, i64)]| rows.iter().map(|(month, day, total)| (month.to_string(), day.to_string(), *total)).collect::<Vec<_>>();
        // A broker's statement keeps the month its prices are from: the holdings by kind, cash,
        // what is accrued, and what the holdings cost and have gained.
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        assert_eq!(find(&b, "broker").unwrap().snapshots, vec![MonthEnd {
            month: "2026-11".into(),
            as_of: "2026-11-13".into(),
            total: 2_701_250,
            by_kind: [("Stocks".to_string(), 2_578_000)].into(),
            cash: 122_000,
            other: 1_250,
            cost: Some(2_270_000),
            unrealised: Some(308_000),
        }]);
        // A later day in the same month takes its place; the next month is added beside it.
        let mut later = statement();
        later.values.push(("2026-11-16".into(), Dec::parse("28000").unwrap()));
        let b = apply(b, "broker", &later, None).unwrap();
        assert_eq!(months(&b), kept(&[("2026-11", "2026-11-16", 2_800_000)]));
        later.to = "2026-12-31".into();
        later.values.push(("2026-12-31".into(), Dec::parse("29000").unwrap()));
        let b = apply(b, "broker", &later, None).unwrap();
        assert_eq!(months(&b), kept(&[("2026-11", "2026-11-16", 2_800_000), ("2026-12", "2026-12-31", 2_900_000)]));
        // The same statement again changes nothing, and an older one doesn't take a newer month's place.
        assert_eq!(apply(b.clone(), "broker", &later, None).unwrap(), b);
        assert_eq!(months(&apply(b, "broker", &statement(), None).unwrap())[1], kept(&[("2026-12", "2026-12-31", 2_900_000)])[0]);

        // A positions file keeps its month the same way.
        let file = |day: &str, fund: &str| Snapshot {
            source: "file".into(),
            as_of: day.into(),
            positions: vec![held("VWRA", "World fund", "FUND", fund, "15000"), held("T", "", "BOND", "4925", "")],
            cash: "820".into(),
            ..Snapshot::default()
        };
        let b = record(budget("USD"), "broker", file("2026-11-13", "17100")).unwrap();
        let first = find(&b, "broker").unwrap().snapshots;
        assert_eq!(first, vec![MonthEnd {
            month: "2026-11".into(),
            as_of: "2026-11-13".into(),
            total: 2_284_500,
            by_kind: [("Funds".to_string(), 1_710_000), ("Bonds".to_string(), 492_500)].into(),
            cash: 82_000,
            other: 0,
            cost: Some(1_500_000),
            unrealised: Some(210_000),
        }]);
        // Figures for an earlier day fill in a month nothing was kept for, and leave alone a
        // month already kept from a later day.
        let b = record(b, "broker", file("2026-11-05", "16000")).unwrap();
        assert_eq!(find(&b, "broker").unwrap().snapshots, first);
        let b = record(b, "broker", file("2026-10-30", "16500")).unwrap();
        assert_eq!(months(&b), kept(&[("2026-10", "2026-10-30", 2_224_500), ("2026-11", "2026-11-13", 2_284_500)]));
        assert_eq!(find(&b, "broker").unwrap().positions[0].value, 1_710_000);
        // A later day replaces its month; a value alone is kept as nothing broken down.
        let b = record(b, "broker", file("2026-11-20", "17500")).unwrap();
        assert_eq!(months(&b), kept(&[("2026-10", "2026-10-30", 2_224_500), ("2026-11", "2026-11-20", 2_324_500)]));
        let b = record(b, "broker", typed("2026-12-31", "24000")).unwrap();
        let last = find(&b, "broker").unwrap().snapshots.pop().unwrap();
        assert_eq!((last.month.as_str(), last.total, last.by_kind.len(), last.cash, last.cost), ("2026-12", 2_400_000, 0, 0, None));
        // An entry reads whether it has none, or one a newer app wrote more into.
        let mut entry = b.extra[KEY][0].clone();
        entry["snapshots"] = json!([{"month": "2027-01", "asOf": "2027-01-29", "total": 5, "byKind": {"Stocks": 5}, "bySector": {"Tech": 5}}]);
        let read: Holdings = serde_json::from_value(entry.clone()).unwrap();
        assert_eq!((read.snapshots[0].by_kind["Stocks"], read.snapshots[0].cash, read.snapshots[0].cost), (5, 0, None));
        entry.as_object_mut().unwrap().remove("snapshots");
        assert!(serde_json::from_value::<Holdings>(entry).unwrap().snapshots.is_empty());

        // A fresh start keeps the snapshots from the month of the value it starts from.
        let b = apply(apply(budget("USD"), "broker", &statement(), None).unwrap(), "broker", &later, None).unwrap();
        let after = |day: &str| after_fresh_start(&b.extra[KEY], day)[0]["snapshots"].as_array().unwrap().iter().map(|s| s["month"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
        assert_eq!(after("2026-12-15"), vec!["2026-11", "2026-12"]);
        assert_eq!(after("2026-12-31"), vec!["2026-12"]);
    }

    #[test]
    fn trades_and_dividends_by_holding_are_kept_by_month_from_the_statements_that_list_them() {
        let traded = flex::parse(&flex::tests::with_trades()).unwrap().accounts.remove(0);
        let b = apply(budget("USD"), "broker", &traded, None).unwrap();
        let h = find(&b, "broker").unwrap();
        let month = |month: &str, realised: i64, commissions: i64, count: u32| MonthTrades { month: month.into(), realised, commissions, count };
        // Every month of the statement's period, each trade once: two sales of one order in
        // October, a purchase in riyals in November at the row's own rate, and in September a
        // trade whose figures didn't read.
        assert_eq!(h.trades, vec![month("2026-09", 0, 0, 1), month("2026-10", 49_875, -250, 2), month("2026-11", 0, -200, 1)]);
        // The statement's own total is kept beside them, and Trades is asked of no one.
        assert_eq!(h.commissions.as_ref().map(|c| c.amount), Some(-350));
        assert!(h.missing.is_empty());
        // Dividends and their tax by holding, each under the key it has among the holdings:
        // the row's symbol or broker's id finds the position, and one since sold keeps its own.
        let paid = |month: &str, key: &str, symbol: &str, name: &str, dividends: i64, tax: i64| HoldingIncome { month: month.into(), key: key.into(), symbol: symbol.into(), name: name.into(), dividends, tax };
        assert_eq!(h.income_by, vec![
            paid("2026-10", "SA14TG012N13", "2222", "SAUDI ARABIAN OIL CO", 2_667, -133),
            paid("2026-10", "US5949181045", "MSFT", "MSFT", 300, 0),
            paid("2026-11", "US0378331005", "AAPL", "APPLE INC", 650, -98),
        ]);
        // They are the months' own dividends and tax, holding by holding.
        for row in &h.income {
            let of = |pick: fn(&HoldingIncome) -> i64| h.income_by.iter().filter(|by| by.month == row.month).map(pick).sum::<i64>();
            assert_eq!((of(|by| by.dividends), of(|by| by.tax)), (row.dividends, row.tax), "{}", row.month);
        }
        // The same statement again changes nothing.
        assert_eq!(apply(b.clone(), "broker", &traded, None).unwrap(), b);

        // A later statement is the record for its own months: November is replaced, December
        // added, and the months before its period stay.
        let trade = |date: &str, realised: &str, commission: &str| flex::Trade {
            id: String::new(),
            date: date.into(),
            currency: "USD".into(),
            fx: Some(Dec::ONE),
            realised: Dec::parse(realised),
            commission: Dec::parse(commission),
            commission_currency: String::new(),
        };
        let mut later = traded.clone();
        later.from = "2026-11-01".into();
        later.to = "2026-12-31".into();
        later.values.push(("2026-12-31".into(), Dec::parse("28000").unwrap()));
        later.flows.retain(|f| f.date.as_str() >= "2026-11-01");
        later.income.retain(|i| i.date.as_str() >= "2026-11-01");
        later.income.push(flex::Income {
            date: "2026-12-15".into(),
            kind: flex::IncomeKind::Dividends,
            currency: "USD".into(),
            amount: Dec::parse("12").unwrap(),
            fx: None,
            security: flex::Security { isin: "IE00BK5BQT80".into(), ..Default::default() },
        });
        later.trades = Some(vec![trade("2026-11-20", "100", "-1"), trade("2026-12-02", "-40", "-1.5"), trade("2026-12-02", "", "-1.5")]);
        let after = apply(b.clone(), "broker", &later, None).unwrap();
        let h = find(&after, "broker").unwrap();
        assert_eq!(h.trades, vec![month("2026-09", 0, 0, 1), month("2026-10", 49_875, -250, 2), month("2026-11", 10_000, -100, 1), month("2026-12", -4_000, -300, 2)]);
        assert_eq!(h.income_by.iter().map(|by| (by.month.as_str(), by.symbol.as_str(), by.dividends)).collect::<Vec<_>>(), vec![
            ("2026-10", "2222", 2_667), ("2026-10", "MSFT", 300), ("2026-11", "AAPL", 650), ("2026-12", "VWRA", 1_200),
        ]);
        assert_eq!(h.income_by[3].name, "VANG FTSE AW USDA");
        // One that starts partway through a month leaves that month as it was kept, and one
        // without the section leaves every month.
        let mut partway = later.clone();
        partway.from = "2026-11-10".into();
        partway.trades = Some(vec![trade("2026-11-20", "1", "-9"), trade("2026-12-30", "5", "-0.5")]);
        let h = find(&apply(after.clone(), "broker", &partway, None).unwrap(), "broker").unwrap();
        assert_eq!(h.trades[2..], [month("2026-11", 10_000, -100, 1), month("2026-12", 500, -50, 1)]);
        let mut silent = later.clone();
        silent.trades = None;
        silent.values.push(("2027-01-04".into(), Dec::parse("28100").unwrap()));
        assert_eq!(find(&apply(after.clone(), "broker", &silent, None).unwrap(), "broker").unwrap().trades.len(), 4);
        // A trade with no rate for its currency is counted and its figures left out.
        let mut unpriced = later.clone();
        unpriced.trades = Some(vec![flex::Trade { currency: "JPY".into(), fx: None, ..trade("2026-12-02", "900", "-90") }]);
        assert_eq!(find(&apply(after.clone(), "broker", &unpriced, None).unwrap(), "broker").unwrap().trades[3], month("2026-12", 0, 0, 1));

        // Dividends by holding go back 36 months from the newest.
        let mut old = after.clone();
        let rows = old.extra[KEY][0]["incomeBy"].as_array_mut().unwrap();
        rows.insert(0, json!({"month": "2024-01", "key": "X", "symbol": "X", "name": "X", "dividends": 1, "tax": 0}));
        rows.insert(0, json!({"month": "2023-12", "key": "X", "symbol": "X", "name": "X", "dividends": 1, "tax": 0}));
        let mut next = later.clone();
        next.values.push(("2027-01-04".into(), Dec::parse("28100").unwrap()));
        let kept = find(&apply(old, "broker", &next, None).unwrap(), "broker").unwrap().income_by;
        assert_eq!((kept[0].month.as_str(), kept.len()), ("2024-01", 5));

        // A statement that lists no trades and names no holdings keeps neither, and an entry
        // from before they were kept reads and is written as it was.
        let plain = find(&apply(budget("USD"), "broker", &statement(), None).unwrap(), "broker").unwrap();
        assert!(plain.trades.is_empty());
        let mut nameless = statement();
        for i in &mut nameless.income {
            i.security = Default::default();
        }
        let kept = apply(budget("USD"), "broker", &nameless, None).unwrap();
        let entry = &kept.extra[KEY][0];
        assert!(entry.get("trades").is_none() && entry.get("incomeBy").is_none());
        // A fresh start keeps them as it keeps the months' income.
        let fresh = after_fresh_start(&after.extra[KEY], "2026-12-15");
        assert_eq!((fresh[0]["trades"].as_array().unwrap().len(), fresh[0]["incomeBy"].as_array().unwrap().len(), fresh[0]["income"].as_array().unwrap().len()), (4, 4, 3));
    }

    #[test]
    fn a_holdings_finer_category_side_and_own_currency_value_are_kept_where_the_source_gives_them() {
        // From a broker's statement: the value in its own currency as written, the riyal stock's in riyals.
        let b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        let held = find(&b, "broker").unwrap();
        assert_eq!(held.positions.iter().map(|p| (p.symbol.as_str(), p.currency.as_str(), p.amount.as_str(), p.value)).collect::<Vec<_>>(), vec![
            ("VWRA", "USD", "17100", 1_710_000), ("AAPL", "USD", "5800", 580_000), ("2222", "SAR", "10800", 288_000),
        ]);
        let finer = flex::parse(&SAMPLE.replace(r#"symbol="VWRA" description="VANG FTSE AW USDA""#, r#"symbol="VWRA" description="VANG FTSE AW USDA" subCategory="ETF""#)).unwrap().accounts.remove(0);
        let b = apply(b, "broker", &finer, None).unwrap();
        let entry = &b.extra[KEY][0]["positions"];
        assert_eq!((entry[0]["subKind"].as_str(), entry[1].get("subKind"), entry[0].get("putCall")), (Some("ETF"), None, None));
        // From a file: the value is in the holding's own currency only when that is the file's.
        let snap = Snapshot {
            source: "file".into(),
            as_of: "2026-11-13".into(),
            currency: "EUR".into(),
            rate: "1.1".into(),
            positions: vec![
                held_in("EUNL", "10,400.50", ""),
                held_in("SAP", "2000", "eur"),
                // Listed as a dollar holding, but valued in the file's euros.
                held_in("AAPL", "900", "USD"),
            ],
            ..Snapshot::default()
        };
        let kept = find(&record(budget("USD"), "broker", snap).unwrap(), "broker").unwrap();
        let rows: Vec<(&str, &str, &str)> = kept.positions.iter().map(|p| (p.symbol.as_str(), p.currency.as_str(), p.amount.as_str())).collect();
        assert_eq!(rows, vec![("EUNL", "EUR", "10400.50"), ("SAP", "EUR", "2000"), ("AAPL", "USD", "")]);
        // A value typed in alone has no holdings to give one to.
        assert!(find(&record(budget("USD"), "broker", typed("2026-11-13", "500")).unwrap(), "broker").unwrap().positions.is_empty());
    }
    fn held_in(symbol: &str, value: &str, currency: &str) -> Position {
        Position { currency: currency.into(), ..held(symbol, symbol, "STK", value, "") }
    }

    fn valued(rows: &[(&str, &str)]) -> Vec<(String, Dec)> {
        rows.iter().map(|(d, v)| (d.to_string(), Dec::parse(v).unwrap())).collect()
    }
    /// The same statement covering days from `from` on, as the next sync's statement does when
    /// its year has moved on: what it lists before then is left out.
    fn from_day(s: &flex::Account, from: &str) -> flex::Account {
        let mut next = s.clone();
        next.from = from.into();
        next.values.retain(|(d, _)| d.as_str() >= from);
        next.flows.retain(|f| f.date.as_str() >= from);
        next.income.retain(|i| i.date.as_str() >= from);
        next
    }

    #[test]
    fn the_days_money_moved_are_the_same_whatever_days_the_statement_covers() {
        // A wire dated 30 September that the value shows on 5 October.
        let mut wired = statement();
        wired.flows.retain(|f| f.date == "2026-10-05");
        wired.flows[0].date = "2026-09-30".into();
        wired.flows[0].amount = Dec::parse("6000").unwrap();
        wired.values = valued(&[("2026-09-29", "24000"), ("2026-09-30", "24050"), ("2026-10-02", "24000"), ("2026-10-05", "29950"), ("2026-10-09", "30000"), ("2026-10-30", "30100"), ("2026-11-13", "30200")]);
        // A withdrawal the value shows on 2 October, listed on the 5th.
        let mut withdrawn = statement();
        withdrawn.flows.retain(|f| f.date == "2026-10-05");
        withdrawn.flows[0].amount = Dec::parse("-6000").unwrap();
        withdrawn.values = valued(&[("2026-09-30", "30000"), ("2026-10-01", "30000"), ("2026-10-02", "24000"), ("2026-10-05", "24010"), ("2026-10-30", "24100"), ("2026-11-13", "24200")]);
        // A jump nothing listed explains, in a statement whose query has no Transfers section.
        let mut guessed = statement();
        guessed.flows.clear();
        guessed.missing = vec!["Transfers"];
        guessed.values = valued(&[("2026-10-01", "24000"), ("2026-10-02", "24000"), ("2026-10-05", "30000"), ("2026-10-30", "30100"), ("2026-11-13", "30200")]);
        let cases: [(&str, flex::Account, Vec<(String, i64)>, Vec<&str>, [&str; 2]); 3] = [
            ("wire", wired, vec![("2026-10-05".into(), 600_000)], vec![], ["2026-10-01", "2026-10-06"]),
            ("withdrawal", withdrawn, vec![("2026-10-02".into(), -600_000)], vec![], ["2026-10-03", "2026-10-06"]),
            ("estimate", guessed, vec![("2026-10-05".into(), 600_000)], vec!["2026-10-05"], ["2026-10-03", "2026-10-06"]),
        ];
        for (what, s, flows, estimated, later) in cases {
            let b = apply(budget("USD"), "broker", &s, None).unwrap();
            let h = find(&b, "broker").unwrap();
            assert_eq!((h.flows.clone(), h.estimated.clone()), (Some(flows.clone()), estimated.iter().map(|d| d.to_string()).collect::<Vec<_>>()), "{what}");
            // The next statements' years start after the money's own date, and after the day the
            // value showed it: one flow, on the same day, and the same lines in the register.
            for from in later {
                let next = from_day(&s, from);
                let after = apply(b.clone(), "broker", &next, None).unwrap();
                let kept = find(&after, "broker").unwrap();
                assert_eq!((kept.flows.clone(), kept.estimated.clone()), (h.flows.clone(), h.estimated.clone()), "{what} from {from}");
                assert_eq!(written(&after), written(&b), "{what} from {from}");
                assert_eq!(kept.movements, h.movements, "{what} from {from}");
                // And again, from what that kept.
                let again = apply(after.clone(), "broker", &next, None).unwrap();
                assert_eq!(written(&again), written(&b), "{what} from {from}, twice");
            }
            // Once its year has moved past the day, a statement with every section, which can't
            // say anything of that day, leaves the estimate as it was.
            let mut full = from_day(&s, "2026-10-06");
            full.missing = vec![];
            let kept = find(&apply(b.clone(), "broker", &full, None).unwrap(), "broker").unwrap();
            assert_eq!((kept.flows, kept.estimated), (h.flows.clone(), h.estimated.clone()), "{what} after a full statement");
        }
    }

    #[test]
    fn a_jump_a_full_statement_lists_nothing_for_is_the_market() {
        // A fall of 6% in a day, with every section sent and nothing listed: the market did it.
        let mut s = statement();
        s.flows.clear();
        s.values = valued(&[("2026-09-30", "30000"), ("2026-10-01", "30000"), ("2026-10-02", "28200"), ("2026-10-30", "28300"), ("2026-11-13", "28400")]);
        let b = apply(budget("USD"), "broker", &s, None).unwrap();
        let h = find(&b, "broker").unwrap();
        assert_eq!((h.flows.clone(), h.estimated.clone()), (Some(vec![]), vec![]));
        assert_eq!((lines(&b, "broker", "change"), lines(&b, "broker", "moved")), (vec![-170_000, 10_000], vec![]));
        // A query without the Transfers section can't say, so the same fall is taken for money moved.
        s.missing = vec!["Transfers"];
        let h = find(&apply(budget("USD"), "broker", &s, None).unwrap(), "broker").unwrap();
        assert_eq!((h.flows, h.estimated), (Some(vec![("2026-10-02".to_string(), -180_000)]), vec!["2026-10-02".to_string()]));
    }

    #[test]
    fn a_statement_without_a_section_leaves_what_an_earlier_one_kept() {
        let full = SAMPLE.replace(
            "<Transfers />",
            r#"<Transfers>
<Transfer accountId="U1234567" currency="USD" fxRateToBase="1" assetCategory="STK" symbol="AAPL" description="APPLE INC" date="20261009" type="INTERNAL" direction="IN" quantity="10" transferPrice="230" positionAmount="2300" positionAmountInBase="2300" cashTransfer="0" transactionID="7000001" levelOfDetail="DETAIL" />
</Transfers>"#,
        );
        let without = |from: &str, to: &str| {
            let (start, end) = (full.find(from).unwrap(), full.find(to).unwrap() + to.len());
            flex::parse(&format!("{}{}", &full[..start], &full[end..])).unwrap().accounts.remove(0)
        };
        let b = apply(budget("USD"), "broker", &flex::parse(&full).unwrap().accounts.remove(0), None).unwrap();
        let h = find(&b, "broker").unwrap();
        assert_eq!(h.movements.len(), 3);
        let review = |b: &Budget| reviewed(b, "broker").iter().map(|r| (r.id.clone(), r.date.clone(), r.amount)).collect::<Vec<_>>();
        for (section, s) in [("Cash Transactions", without("<CashTransactions>", "</CashTransactions>")), ("Transfers", without("<Transfers>", "</Transfers>"))] {
            assert!(s.missing.contains(&section), "{section}");
            let after = apply(b.clone(), "broker", &s, None).unwrap();
            let kept = find(&after, "broker").unwrap();
            assert_eq!((&kept.movements, &kept.flows, &kept.estimated), (&h.movements, &h.flows, &h.estimated), "{section}");
            assert_eq!((&kept.income, &kept.income_by), (&h.income, &h.income_by), "{section}");
            assert_eq!(written(&after), written(&b), "{section}");
            assert_eq!(review(&after), review(&b), "{section}");
        }
        // A first statement that lists neither leaves money in and out to the budget's own
        // transfers, as for holdings from before the broker's record was kept.
        let neither = flex::parse(&{
            let (a, b2) = (SAMPLE.find("<CashTransactions>").unwrap(), SAMPLE.find("</CashTransactions>").unwrap() + "</CashTransactions>".len());
            format!("{}{}", &SAMPLE[..a], &SAMPLE[b2..]).replace("<Transfers />", "")
        })
        .unwrap()
        .accounts
        .remove(0);
        let mut own = budget("USD");
        deposit(&mut own, "d1", "2026-10-05", 100_000);
        let own = apply(own, "broker", &neither, None).unwrap();
        let h = find(&own, "broker").unwrap();
        assert!(h.flows.is_none() && h.movements.is_empty() && h.estimated.is_empty());
        assert!(lines(&own, "broker", "moved").is_empty());
        assert_eq!(balance(&own, "broker", "9999-12-31"), 2_701_250);
    }

    #[test]
    fn re_pointing_a_broker_account_takes_its_lines_and_reviewed_rows_from_the_account_it_filled() {
        // Three movements: one joined to the bank's own entry, one added with no other account,
        // and one added as a transfer from the bank.
        let mut s = statement();
        let third = flex::Flow { id: "9000003".into(), date: "2026-10-20".into(), amount: Dec::parse("500").unwrap(), ..s.flows[0].clone() };
        s.flows.push(third);
        let mut b = budget("USD");
        entry(&mut b, "wire", "2026-10-05", "IBKR", -100_000, None);
        let b = apply(b, "broker", &s, None).unwrap();
        let rows = reviewed(&b, "broker");
        assert_eq!(rows.iter().map(|r| (r.date.as_str(), r.amount)).collect::<Vec<_>>(), vec![("2026-10-05", 100_000), ("2026-10-20", 50_000), ("2026-11-02", 40_000)]);
        let mut from_bank = choose(&rows[2], "new", None);
        from_bank.row.transfer_account_id = Some("bank".into());
        let b = import(b, "broker", vec![choose(&rows[0], "match", Some(&"wire".to_string())), choose(&rows[1], "new", None), from_bank]).unwrap();
        let added_plain = rows[1].id.clone();
        let added_transfer = rows[2].id.clone();
        assert!(b.transactions.iter().any(|t| t.id == added_plain) && b.transactions.iter().any(|t| t.id == added_transfer));
        assert!(reviewed(&b, "broker").iter().all(|r| r.duplicate));
        let bank_before = balance(&b, "bank", "9999-12-31");

        // The broker account now fills another account.
        let mut moved = b.clone();
        moved.accounts.push(Account { id: "broker2".into(), name: "broker2".into(), ..moved.accounts[1].clone() });
        release(&mut moved, "broker", "broker2");
        let moved = apply(moved, "broker2", &s, None).unwrap();
        // The row added with no other account is gone, and the new account's review offers it again.
        assert!(!moved.transactions.iter().any(|t| t.id == added_plain));
        let review: Vec<(String, bool)> = reviewed(&moved, "broker2").iter().map(|r| (r.date.clone(), r.duplicate)).collect();
        assert_eq!(review, vec![("2026-10-05".to_string(), true), ("2026-10-20".to_string(), false), ("2026-11-02".to_string(), true)]);
        // The bank's side joined to it is now a transfer with the new account; the one added as a
        // transfer from the bank is in the new account.
        let wire = moved.transactions.iter().find(|t| t.id == "wire").unwrap();
        assert_eq!((wire.account_id.as_str(), wire.transfer_account_id.as_deref()), ("bank", Some("broker2")));
        assert!(wire.extra["bankImportMatches"].get("broker").is_none() && wire.extra["bankImportMatches"].get("broker2").is_some());
        let added = moved.transactions.iter().find(|t| t.id == added_transfer).unwrap();
        assert_eq!((added.account_id.as_str(), added.transfer_account_id.as_deref()), ("broker2", Some("bank")));
        // The old account holds nothing of the broker's any more, the bank is as it was, and the
        // broker is counted once, at its value.
        assert!(!moved.transactions.iter().any(|t| generated(t) && t.account_id == "broker"));
        assert_eq!(balance(&moved, "broker", "9999-12-31"), 0);
        assert_eq!(balance(&moved, "bank", "9999-12-31"), bank_before);
        assert_eq!(balance(&moved, "broker2", "9999-12-31"), find(&moved, "broker2").unwrap().total);
        // Bringing in the row offered again puts the money where the broker is now.
        let again = reviewed(&moved, "broker2");
        let moved = import(moved, "broker2", vec![choose(&again[1], "new", None)]).unwrap();
        assert!(reviewed(&moved, "broker2").iter().all(|r| r.duplicate));
        assert_eq!(balance(&moved, "broker2", "9999-12-31"), find(&moved, "broker2").unwrap().total);

        // Stopping tracking keeps the lines and the rows, so the account keeps its balance and history.
        let mut unlinked = b.clone();
        unlink(&mut unlinked, "broker");
        assert_eq!(written(&unlinked), written(&b));
        assert_eq!(unlinked.transactions, b.transactions);
    }

    /// The account's register lines and flows for September and October 2026, and its estimates.
    fn autumn(b: &Budget) -> (Vec<(String, String, i64)>, Option<Vec<(String, i64)>>, Vec<String>) {
        let h = find(b, "broker").unwrap();
        let lines = written(b).into_iter().filter(|w| w.0.starts_with("investment-start") || w.0.ends_with(":2026-09") || w.0.ends_with(":2026-10")).collect();
        let flows = h.flows.map(|f| f.into_iter().filter(|(d, _)| d.as_str() <= "2026-10-31").collect());
        (lines, flows, h.estimated.into_iter().filter(|d| d.as_str() <= "2026-10-31").collect())
    }
    /// A sync on `day`: its statement's year ends then, and lists the first one's money moved
    /// within that year, as the broker would; its query is the first one's.
    fn sync_on(b: Budget, first: &flex::Account, day: &str) -> Budget {
        let mut s = first.clone();
        s.from = shift_days(day, -364);
        s.to = day.into();
        s.values = valued(&[(day, "30200")]);
        s.flows.retain(|f| f.date >= s.from);
        s.income.clear();
        // The same query: a section it left out it still leaves out.
        apply(b, "broker", &s, None).unwrap()
    }

    #[test]
    fn what_was_worked_out_stays_as_its_days_leave_the_daily_values() {
        let mut wired = statement();
        wired.flows.retain(|f| f.date == "2026-10-05");
        wired.flows[0].date = "2026-09-30".into();
        wired.flows[0].amount = Dec::parse("6000").unwrap();
        wired.values = valued(&[("2026-09-29", "24000"), ("2026-09-30", "24050"), ("2026-10-02", "24000"), ("2026-10-05", "29950"), ("2026-10-09", "30000"), ("2026-10-30", "30100"), ("2026-11-13", "30200")]);
        let mut withdrawn = statement();
        withdrawn.flows.retain(|f| f.date == "2026-10-05");
        withdrawn.flows[0].amount = Dec::parse("-6000").unwrap();
        withdrawn.values = valued(&[("2026-09-30", "30000"), ("2026-10-01", "30000"), ("2026-10-02", "24000"), ("2026-10-05", "24010"), ("2026-10-30", "24100"), ("2026-11-13", "24200")]);
        let mut guessed = statement();
        guessed.flows.clear();
        guessed.missing = vec!["Transfers"];
        guessed.values = valued(&[("2026-09-30", "24000"), ("2026-10-01", "24000"), ("2026-10-02", "24000"), ("2026-10-05", "30000"), ("2026-10-30", "30100"), ("2026-11-13", "30200")]);
        // Every day across the edge of the daily values (which starts 400 days before the
        // statement's last day: 2 October 2026 for one ending 6 November 2027), and days skipped.
        let daily: Vec<String> = (0..20).map(|n| shift_days("2027-10-27", n)).collect();
        let skipping: [&[&str]; 3] = [&["2027-11-06", "2027-11-10"], &["2027-11-05", "2027-11-07"], &["2027-11-04", "2027-11-09", "2027-11-15"]];
        for (what, first) in [("wire", wired), ("withdrawal", withdrawn), ("estimate", guessed)] {
            let start = apply(budget("USD"), "broker", &first, None).unwrap();
            let expected = autumn(&start);
            assert!(expected.1.as_ref().is_some_and(|f| f.len() == 1), "{what}: {expected:?}");
            // Kept up to a year later, with a value a month.
            let mut kept = start;
            for month in ["2027-01-31", "2027-04-30", "2027-07-31", "2027-10-26"] {
                kept = sync_on(kept, &first, month);
                assert_eq!(autumn(&kept), expected, "{what} on {month}");
            }
            let days: Vec<&[String]> = vec![&daily];
            for run in days.into_iter().map(|d| d.iter().map(String::as_str).collect::<Vec<_>>()).chain(skipping.iter().map(|d| d.to_vec())) {
                let mut b = kept.clone();
                for day in &run {
                    b = sync_on(b, &first, day);
                    assert_eq!(autumn(&b), expected, "{what} on {day} of {run:?}");
                }
            }
        }
    }

    #[test]
    fn days_before_an_entrys_first_movements_stay_as_they_were() {
        // An entry from before movements were kept: a deposit on 20 October 2025, counted then.
        let mut first = statement();
        first.from = "2025-10-01".into();
        first.flows.retain(|f| f.date == "2026-10-05");
        first.flows[0].id = "8000001".into();
        first.flows[0].date = "2025-10-20".into();
        first.flows[0].amount = Dec::parse("6000").unwrap();
        first.values = valued(&[("2025-10-17", "20000"), ("2025-10-20", "26000"), ("2025-10-31", "26100"), ("2026-09-30", "26200"), ("2026-10-30", "27300"), ("2026-11-13", "27400")]);
        let mut b = apply(budget("USD"), "broker", &first, None).unwrap();
        let entry = b.extra.get_mut(KEY).unwrap()[0].as_object_mut().unwrap();
        entry.remove("movements");
        assert_eq!(find(&b, "broker").unwrap().flows, Some(vec![("2025-10-20".to_string(), 600_000)]));
        let october = |b: &Budget| written(b).into_iter().filter(|w| w.0.ends_with(":2025-10")).collect::<Vec<_>>();
        let before = october(&b);
        assert_eq!(before.iter().map(|w| w.2).collect::<Vec<_>>(), vec![10_000, 600_000]);
        // Its next statements start in January and list a deposit of their own.
        let mut later = statement();
        later.from = "2026-01-01".into();
        later.values = valued(&[("2026-09-30", "26200"), ("2026-10-30", "27300"), ("2026-11-13", "27400")]);
        for sync in 1..=3 {
            b = apply(b, "broker", &later, None).unwrap();
            let h = find(&b, "broker").unwrap();
            assert_eq!((h.movements_from.as_str(), h.movements.len()), ("2026-01-01", 2), "sync {sync}");
            assert_eq!(h.flows.as_ref().unwrap()[0], ("2025-10-20".to_string(), 600_000), "sync {sync}");
            assert_eq!(october(&b), before, "sync {sync}");
        }
        // An entry that kept its movements from the start has nothing extra kept as it was.
        let fresh = find(&apply(budget("USD"), "broker", &first, None).unwrap(), "broker").unwrap();
        assert!(fresh.movements_from.is_empty());
        // And one that moved on reads as counted on each movement's own day, written only when it isn't.
        let wire = |b: &Budget| b.extra[KEY][0]["movements"].as_array().unwrap().iter().map(|m| m.get("counted").cloned()).collect::<Vec<_>>();
        assert_eq!(wire(&apply(budget("USD"), "broker", &first, None).unwrap()), vec![None]);
    }

    #[test]
    fn money_the_review_adds_with_no_other_account_is_in_transit_too() {
        // The wire is dated 30 September; the value shows it on 5 October.
        let mut s = statement();
        s.flows.retain(|f| f.date == "2026-10-05");
        s.flows[0].date = "2026-09-30".into();
        s.values = valued(&[("2026-09-29", "50"), ("2026-09-30", "50"), ("2026-10-02", "50"), ("2026-10-05", "1050"), ("2026-10-30", "1060"), ("2026-11-13", "1070")]);
        let b = apply(budget("USD"), "broker", &s, None).unwrap();
        assert_eq!(lines(&b, "broker", "moved"), vec![100_000]);
        let rows = reviewed(&b, "broker");
        let b = import(b, "broker", vec![choose(&rows[0], "new", None)]).unwrap();
        let added = b.transactions.iter().find(|t| !generated(t)).unwrap();
        assert_eq!((added.date.as_str(), added.amount, added.transfer_account_id.as_deref()), ("2026-09-30", 100_000, None));
        // Added on the broker's date, it counts on the day the broker counted it: no line takes
        // it out of September and puts it back in October.
        assert!(lines(&b, "broker", "moved").is_empty(), "{:?}", written(&b));
        assert_eq!(balance(&b, "broker", "2026-10-30"), 106_000);
        // Money typed into the account by hand isn't the broker's record of it, and stays on its day.
        let mut typed = apply(budget("USD"), "broker", &s, None).unwrap();
        typed.transactions.push(Transaction { id: "typed".into(), account_id: "broker".into(), ..added.clone() });
        typed.transactions.last_mut().unwrap().extra = Default::default();
        revalue(&mut typed, "broker").unwrap();
        assert_eq!(lines(&typed, "broker", "moved"), vec![-100_000, 100_000]);
    }

    #[test]
    fn the_review_and_the_in_transit_rule_take_the_same_money_for_nearly_the_same_amount() {
        // A riyal budget: the bank sent 3,840.00 on 29 September; the broker counted 1,000 dollars,
        // 3,750.00 at its rate, on 2 October.
        let mut b = budget("SAR");
        entry(&mut b, "wire", "2026-09-29", "IBKR", -384_000, None);
        let mut s = statement();
        s.flows.retain(|f| f.date == "2026-10-05");
        s.flows[0].date = "2026-10-02".into();
        s.values = valued(&[("2026-09-26", "5000"), ("2026-09-29", "5000"), ("2026-10-02", "6000"), ("2026-10-30", "6010"), ("2026-11-13", "6020")]);
        let b = apply(b, "broker", &s, None).unwrap();
        let rows = reviewed(&b, "broker");
        assert_eq!((rows[0].amount, rows[0].transfer_candidates.clone()), (375_000, vec!["wire".to_string()]));
        let b = import(b, "broker", vec![choose(&rows[0], "match", Some(&"wire".to_string()))]).unwrap();
        // The review took them for the same money, and so does the account's balance: no line
        // takes it out of September; what the bank's rate cost is October's one line.
        assert_eq!(lines(&b, "broker", "moved"), vec![-9_000]);
    }

    #[test]
    fn the_first_valued_day_stays_when_its_year_is_long_past() {
        let mut first = statement();
        first.flows.clear();
        first.income.clear();
        first.from = "2025-06-01".into();
        first.to = "2025-06-30".into();
        first.values = valued(&[("2025-06-14", "20000"), ("2025-06-20", "20100"), ("2025-06-30", "20200")]);
        let b = apply(budget("USD"), "broker", &first, None).unwrap();
        let start = |b: &Budget| written(b).into_iter().filter(|w| w.0.starts_with("investment-start") || w.0.ends_with(":2025-06")).collect::<Vec<_>>();
        let before = start(&b);
        assert_eq!(before, vec![("investment-change:broker:2025-06".to_string(), "2025-06-30".to_string(), 20_000), ("investment-start:broker".to_string(), "2025-06-14".to_string(), 2_000_000)]);
        // More than a year on, the first day is still the one tracking began on.
        let mut later = first.clone();
        later.from = "2026-07-01".into();
        later.to = "2026-08-31".into();
        later.values = valued(&[("2026-08-31", "25000")]);
        let b = apply(b, "broker", &later, None).unwrap();
        assert_eq!(find(&b, "broker").unwrap().values.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>(), vec!["2025-06-14", "2025-06-30", "2026-08-31"]);
        assert_eq!(start(&b), before);
    }

    #[test]
    fn a_sync_keeps_what_a_newer_app_stored_and_the_lines_take_new_wording() {
        let mut b = apply(budget("USD"), "broker", &statement(), None).unwrap();
        b.extra.get_mut(KEY).unwrap()[0]["fromANewerApp"] = json!({"kept": true});
        let mut later = statement();
        later.values.push(("2026-11-16".into(), Dec::parse("28000").unwrap()));
        let mut b = apply(b, "broker", &later, None).unwrap();
        assert_eq!(b.extra[KEY][0]["fromANewerApp"], json!({"kept": true}));
        assert_eq!(find(&b, "broker").unwrap().total, 2_800_000);
        // A line whose amount and day are as they were takes the wording this build gives it,
        // when it is still worded as this module words it; a note the user wrote stays.
        let memo = |b: &Budget, id: &str| b.transactions.iter().find(|t| t.id == id).unwrap().memo.clone();
        let wording = memo(&b, "investment-change:broker:2026-10");
        let set = |b: &mut Budget, id: &str, text: &str| b.transactions.iter_mut().find(|t| t.id == id).unwrap().memo = text.into();
        set(&mut b, "investment-change:broker:2026-10", "Change in value in 2026-10, as an earlier build worded it");
        set(&mut b, "investment-change:broker:2026-11", "Sold the fund this month");
        set(&mut b, "investment-start:broker", "");
        revalue(&mut b, "broker").unwrap();
        assert_eq!(memo(&b, "investment-change:broker:2026-10"), wording);
        assert_eq!(memo(&b, "investment-change:broker:2026-11"), "Sold the fund this month");
        assert_eq!(memo(&b, "investment-start:broker"), "Value when tracking began");
    }

    #[test]
    fn only_what_reads_goes_in_and_a_brokers_own_account_is_left_alone() {
        let b = budget("USD");
        assert!(record(b.clone(), "bank", typed("2026-11-13", "1")).unwrap_err().contains("tracking account"));
        assert!(record(b.clone(), "broker", typed("13/11/2026", "1")).unwrap_err().contains("day these figures are from"));
        assert!(record(b.clone(), "broker", typed("2026-11-13", "")).unwrap_err().contains("Give the account's value"));
        assert!(record(b.clone(), "broker", typed("2026-11-13", "a lot")).unwrap_err().contains("can't read a lot"));
        assert!(record(b.clone(), "broker", Snapshot { source: "elsewhere".into(), ..typed("2026-11-13", "1") }).is_err());
        let bad = Snapshot { source: "file".into(), as_of: "2026-11-13".into(), positions: vec![held("X", "Thing", "", "n/a", "")], ..Snapshot::default() };
        assert!(record(b.clone(), "broker", bad).unwrap_err().contains("for Thing"));
        // An account Interactive Brokers fills keeps the broker's own record of its money.
        let filled = apply(b, "broker", &statement(), None).unwrap();
        assert!(record(filled.clone(), "broker", typed("2026-11-30", "1")).unwrap_err().contains("Interactive Brokers fills broker"));
        // Unlinked, it can be kept another way, from the history it has.
        let mut freed = filled;
        unlink(&mut freed, "broker");
        assert!(record(freed, "broker", typed("2026-11-30", "30000")).is_ok());
    }
}
