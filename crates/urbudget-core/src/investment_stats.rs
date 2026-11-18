//! Figures for an investment account's page, worked out from what its broker reported
//! (`investments`): how it did over a period, its ups and downs, each month's gain, what it
//! holds and what it earned and was charged. Nothing here uses prices of its own.
//!
//! All three apps read them through the engine's `investmentPage` (the desktop's own copy of
//! these figures, `investmentStats.ts`, was retired on 18 November 2026; the tests here began
//! as that file's cases).
//!
//! `portfolio` is the accounts together, for Reports: their value, money put in and gain over
//! a range of months, taken from the ledger so they agree with Net worth; what they hold
//! today; and what they earned and were charged. Every app reads it (`investmentsReport`).

use crate::domain::{AccountKind, Budget};
use crate::investments::{self, Commissions, Holding, Holdings};
use crate::{ledger, reports};
use chrono::{Datelike, Months, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// How far back a figure looks: a month, three, since January, a year, or since tracking began.
pub const PERIODS: [&str; 5] = ["1m", "3m", "ytd", "1y", "all"];

fn date(iso: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(iso, "%Y-%m-%d").ok()
}
fn days(iso: &str) -> i64 {
    date(iso).map_or(0, |d| i64::from(d.num_days_from_ce()))
}
/// The same day `months` earlier, or the month's last day when it has no such day.
fn months_before(iso: &str, months: u32) -> String {
    date(iso).and_then(|d| d.checked_sub_months(Months::new(months))).map_or_else(|| iso.to_string(), |d| d.to_string())
}

/// Money put into the account (positive) or taken out, by day, oldest first. The broker's own
/// record when the holdings carry it; for holdings brought in before they did, what the budget
/// has in the account apart from the lines `investments` writes.
pub fn flows_of(b: &Budget, held: &Holdings) -> Vec<(String, i64)> {
    if let Some(flows) = &held.flows {
        return flows.clone();
    }
    let mut by_day = std::collections::BTreeMap::<&str, i64>::new();
    for t in &b.transactions {
        if investments::generated(t) || t.extra.get("investment").is_some_and(|v| !v.is_null()) {
            continue;
        }
        if t.account_id == held.account_id {
            *by_day.entry(&t.date).or_default() += t.amount;
        } else if t.transfer_account_id.as_deref() == Some(&held.account_id) {
            *by_day.entry(&t.date).or_default() -= t.amount;
        } else {
            for s in &t.splits {
                if s.extra.get("transferAccountId").and_then(|v| v.as_str()) == Some(&held.account_id) {
                    *by_day.entry(&t.date).or_default() -= s.amount;
                }
            }
        }
    }
    by_day.into_iter().filter(|(_, amount)| *amount != 0).map(|(day, amount)| (day.to_string(), amount)).collect()
}

/// How the account did over a period, ending on the last day the broker valued it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Performance {
    /// Where the period starts among the account's valued days.
    pub first: usize,
    /// The day the period is measured from, and its last day.
    pub from: String,
    pub to: String,
    /// The account's value on those two days.
    pub start: i64,
    pub end: i64,
    /// Money put in, less money taken out, after `from`.
    pub put: i64,
    /// What the value changed by beyond the money put in.
    pub gain: i64,
    /// The gain as a share of the money at work (the Modified Dietz return): the starting value
    /// plus each deposit weighted by how much of the period it was there for. None when there
    /// was no money at work to measure against.
    pub rate: Option<f64>,
    /// The values from `from` on, and the gain so far on each of those days: it ends at `gain`.
    #[serde(skip)]
    pub values: Vec<(String, i64)>,
    #[serde(skip)]
    pub gains: Vec<i64>,
}
/// The day the return is measured from: the period's first, unless the money at work then was
/// under a twentieth of the most there was in the period, and then the day it first reached
/// that. An account that held next to nothing until money arrived late in the period would
/// otherwise have its gain set against that money spread over the whole period: an account
/// empty until two deposits in its last weeks read a loss of under 5% as about -40% over a year.
fn at_work_from<'a>(start: i64, flows: &[(&'a str, i64)], from: &'a str) -> &'a str {
    let mut sorted = flows.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut capital = start;
    let mut peak = start.max(0);
    for (_, amount) in &sorted {
        capital += amount;
        peak = peak.max(capital);
    }
    if start.saturating_mul(20) >= peak {
        return from;
    }
    let mut capital = start;
    for (day, amount) in &sorted {
        capital += amount;
        if capital.saturating_mul(20) >= peak {
            return (*day).max(from);
        }
    }
    from
}
/// The gain as a share of the money at work between two days (the Modified Dietz return): the
/// value at the start, plus each of `flows` for the share of the days it was there, from the
/// day `at_work_from` says. None when there were no days, or no money at work, to measure against.
fn dietz<'a>(start: i64, gain: i64, flows: impl Iterator<Item = (&'a str, i64)>, from: &'a str, to: &str) -> Option<f64> {
    let flows: Vec<(&str, i64)> = flows.collect();
    let from = at_work_from(start, &flows, from);
    let span = days(to) - days(from);
    if span <= 0 {
        return None;
    }
    let mut weighted = 0.0;
    for (day, amount) in &flows {
        weighted += (*amount as f64) * ((days(to) - days(day)).clamp(0, span) as f64) / (span as f64);
    }
    let at_work = start as f64 + weighted;
    (at_work > 0.0).then(|| gain as f64 / at_work)
}
/// From an account's values day by day, oldest first, and the money put in or taken out.
pub fn performance(values: &[(String, i64)], flows: &[(String, i64)], period: &str) -> Option<Performance> {
    let (to, end) = values.last()?.clone();
    let wanted = match period {
        "all" => values[0].0.clone(),
        "ytd" => format!("{}-12-31", date(&to).map_or(0, |d| d.year()) - 1),
        "1m" => months_before(&to, 1),
        "3m" => months_before(&to, 3),
        _ => months_before(&to, 12),
    };
    // The last valued day on or before the period's start; tracking that began later starts there.
    let first = values.iter().take_while(|(day, _)| *day <= wanted).count().saturating_sub(1);
    let (from, start) = values[first].clone();
    let within = || flows.iter().filter(|(day, _)| *day > from && *day <= to).map(|(day, amount)| (day.as_str(), *amount));
    let put: i64 = within().map(|(_, amount)| amount).sum();
    let gain = end - start - put;
    let rate = dietz(start, gain, within(), &from, &to);
    // Day by day: what the value has moved by, less what had been put in by then.
    let within = values[first..].to_vec();
    let mut at = 0;
    let mut so_far = 0;
    let gains = within
        .iter()
        .map(|(day, value)| {
            while at < flows.len() && flows[at].0 <= *day {
                if flows[at].0 > from {
                    so_far += flows[at].1;
                }
                at += 1;
            }
            value - start - so_far
        })
        .collect();
    Some(Performance { first, from, to, start, end, put, gain, rate, values: within, gains })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Day {
    pub date: String,
    pub amount: i64,
}
/// The furthest the gain fell back from a high: by how much, between which days, and as a
/// share of what the account was worth at that high.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Drop {
    pub amount: i64,
    pub from: String,
    pub to: String,
    pub rate: Option<f64>,
}
/// A period's ups and downs, day by day.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Swings {
    /// The day the account gained most and the day it lost most, beyond money moved.
    pub best: Option<Day>,
    pub worst: Option<Day>,
    /// The days it ended up, among the days it moved at all.
    pub up: usize,
    pub moved: usize,
    pub drop: Option<Drop>,
}
/// From a period's running gain: money moved is already out of it.
pub fn swings(did: &Performance) -> Swings {
    let mut out = Swings::default();
    let mut peak = 0;
    for i in 1..did.gains.len() {
        let day = &did.values[i].0;
        let amount = did.gains[i] - did.gains[i - 1];
        if amount != 0 {
            out.moved += 1;
        }
        if amount > 0 {
            out.up += 1;
        }
        if amount > 0 && amount > out.best.as_ref().map_or(0, |d| d.amount) {
            out.best = Some(Day { date: day.clone(), amount });
        }
        if amount < 0 && amount < out.worst.as_ref().map_or(0, |d| d.amount) {
            out.worst = Some(Day { date: day.clone(), amount });
        }
        if did.gains[i] >= did.gains[peak] {
            peak = i;
        }
        let fall = did.gains[i] - did.gains[peak];
        if fall < out.drop.as_ref().map_or(0, |d| d.amount) {
            let high = did.values[peak].1;
            out.drop = Some(Drop { amount: fall, from: did.values[peak].0.clone(), to: day.clone(), rate: (high > 0).then(|| fall as f64 / high as f64) });
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Month {
    pub month: String,
    pub gain: i64,
}
/// The gain in each month the broker valued the account: what its value moved by from the last
/// valued day before the month to the last one in it, less the money put in between them. The
/// first month runs from the first valued day.
pub fn monthly(held: &Holdings, flows: &[(String, i64)]) -> Vec<Month> {
    let mut out = vec![];
    let Some((mut since, mut before)) = held.values.first().cloned() else { return out };
    for (i, (day, value)) in held.values.iter().enumerate() {
        let month = day.get(..7).unwrap_or(day);
        // Only a month's last valued day; and a month whose only valued day is the very first
        // has nothing to be measured from.
        if held.values.get(i + 1).is_some_and(|next| next.0.get(..7) == Some(month)) || i == 0 {
            continue;
        }
        let put: i64 = flows.iter().filter(|(d, _)| *d > since && d <= day).map(|(_, amount)| amount).sum();
        out.push(Month { month: month.to_string(), gain: value - before - put });
        (since, before) = (day.clone(), *value);
    }
    out
}

/// The last valued day's change beyond money put in that day, and the day it's measured from.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DayChange {
    pub amount: i64,
    pub since: String,
}
pub fn day_change(held: &Holdings, flows: &[(String, i64)]) -> Option<DayChange> {
    let [.., (since, before), (day, value)] = held.values.as_slice() else { return None };
    let put: i64 = flows.iter().filter(|(d, _)| d == day).map(|(_, amount)| amount).sum();
    Some(DayChange { amount: value - before - put, since: since.clone() })
}

/// What a broker's asset category is called, for a group of holdings and for one: English
/// keys the apps translate.
pub fn kind(code: &str) -> (&'static str, &'static str) {
    match code {
        "STK" => ("Stocks", "Stock"),
        "FUND" => ("Funds", "Fund"),
        "BOND" | "BILL" => ("Bonds", "Bond"),
        "OPT" | "FOP" | "IOPT" | "WAR" => ("Options", "Option"),
        "FUT" => ("Futures", "Future"),
        "CASH" => ("Currencies", "Currency"),
        "CRYPTO" => ("Crypto", "Crypto"),
        "CMDTY" => ("Commodities", "Commodity"),
        "CFD" => ("CFDs", "CFD"),
        // A source that doesn't say what a holding is: it is a holding, and nothing more is claimed.
        "" => ("Holdings", ""),
        _ => ("Other holdings", "Other"),
    }
}

/// What one holding is, more finely than its kind, from the broker's category, finer category
/// and an option's side: an English key the apps translate ("ETF", "Call option"). A finer
/// category is matched whatever its case and spaces; without one, it is the kind's singular.
/// Currencies written without decimals, and with three; every other has two.
const NO_DECIMALS: [&str; 16] = ["JPY", "KRW", "VND", "CLP", "ISK", "UGX", "PYG", "XOF", "XAF", "XPF", "KMF", "GNF", "RWF", "VUV", "BIF", "DJF"];
const THREE_DECIMALS: [&str; 7] = ["KWD", "BHD", "OMR", "JOD", "TND", "LYD", "IQD"];
/// An amount in `currency` with that currency's usual decimals ("17098.9999" USD is "17099.00",
/// "4000" is "4000.00", "1.25" KWD is "1.250"), rounded half away from zero; as it came when it
/// doesn't read. The apps show these amounts as they get them.
pub fn in_minor_units(amount: &str, currency: &str) -> String {
    let places = if NO_DECIMALS.contains(&currency) { 0 } else if THREE_DECIMALS.contains(&currency) { 3 } else { 2 };
    crate::flex::Dec::parse(amount).map_or_else(|| amount.to_string(), |value| value.fixed(places))
}
pub fn instrument(code: &str, sub_kind: &str, put_call: &str) -> &'static str {
    let sub = sub_kind.trim().to_ascii_uppercase();
    let side = put_call.trim().to_ascii_uppercase();
    match code {
        "STK" => match sub.as_str() {
            "ETF" => "ETF",
            "ETN" => "Exchange-traded note",
            "REIT" => "REIT",
            "ADR" | "GDR" => "Depositary receipt",
            "PREFERRED" => "Preferred share",
            "RIGHT" => "Right",
            "CLOSED-END FUND" => "Closed-end fund",
            _ => "Stock",
        },
        "BILL" => "Treasury bill",
        "BOND" if sub.contains("GOVT") => "Government bond",
        "BOND" if sub.contains("CORP") => "Corporate bond",
        "OPT" | "FOP" | "IOPT" | "WAR" if side == "C" || side == "CALL" => "Call option",
        "OPT" | "FOP" | "IOPT" | "WAR" if side == "P" || side == "PUT" => "Put option",
        _ => kind(code).1,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Part {
    pub key: String,
    pub value: i64,
    pub share: f64,
    /// By currency only: what the part is in that currency, as a plain decimal, or null when
    /// some of it isn't known in that currency. Left out of the other lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Option<String>>,
}
/// Largest first, each with its share of the positive total.
fn parts(sums: Vec<(String, i64)>) -> Vec<Part> {
    let total: i64 = sums.iter().map(|(_, value)| *value).filter(|value| *value > 0).sum();
    let mut out: Vec<Part> = sums
        .into_iter()
        .filter(|(_, value)| *value != 0)
        .map(|(key, value)| Part { key, value, share: if total > 0 { value as f64 / total as f64 } else { 0.0 }, amount: None })
        .collect();
    out.sort_by(|a, b| b.value.cmp(&a.value));
    out
}
fn add(sums: &mut Vec<(String, i64)>, key: &str, value: i64) {
    match sums.iter_mut().find(|(k, _)| k == key) {
        Some((_, sum)) => *sum += value,
        None => sums.push((key.to_string(), value)),
    }
}
/// Holdings' values added up by kind, under the group names of `kind`, in the order first met.
pub fn kinds(positions: &[Holding]) -> Vec<(String, i64)> {
    let mut sums = vec![];
    for p in positions {
        add(&mut sums, kind(&p.kind).0, p.value);
    }
    sums
}
/// What tells one holding from another across accounts: its ISIN, else the broker's id for
/// it, else its symbol (or name) with its currency, so one ticker in two currencies is two.
pub fn holding_key(isin: &str, conid: &str, symbol: &str, name: &str, currency: &str) -> String {
    if !isin.is_empty() {
        isin.to_string()
    } else if !conid.is_empty() {
        conid.to_string()
    } else {
        format!("{} {currency}", if symbol.is_empty() { name } else { symbol })
    }
}
/// Value that is neither a position nor cash, when it's enough to show.
fn accrued(held: &Holdings) -> i64 {
    if held.other.abs() >= 100 { held.other } else { 0 }
}

/// What the account holds, added up.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    /// What the positions cost and have gained, over the ones the broker gives a cost for,
    /// and that gain as a share of the cost.
    pub cost: Option<i64>,
    pub unrealised: Option<i64>,
    pub unrealised_rate: Option<f64>,
    pub cash: i64,
    /// The account by kind of holding (the group keys of `kind`, "Cash", "Accrued") and by currency.
    pub by_kind: Vec<Part>,
    pub by_currency: Vec<Part>,
}
pub fn totals(held: &Holdings) -> Totals {
    let priced: Vec<(i64, i64)> = held.positions.iter().filter_map(|p| Some((p.cost?, p.gain?))).collect();
    let cost: i64 = priced.iter().map(|(cost, _)| cost).sum();
    let unrealised: i64 = priced.iter().map(|(_, gain)| gain).sum();
    let (mut by_kind, mut by_currency) = (kinds(&held.positions), vec![]);
    for p in &held.positions {
        add(&mut by_currency, &p.currency, p.value);
    }
    for c in &held.cash {
        add(&mut by_kind, "Cash", c.value);
        add(&mut by_currency, &c.currency, c.value);
    }
    if accrued(held) != 0 {
        add(&mut by_kind, "Accrued", held.other);
    }
    Totals {
        cost: (!priced.is_empty()).then_some(cost),
        unrealised: (!priced.is_empty()).then_some(unrealised),
        unrealised_rate: (!priced.is_empty() && cost > 0).then(|| unrealised as f64 / cost as f64),
        cash: held.cash.iter().map(|c| c.value).sum(),
        by_kind: parts(by_kind),
        by_currency: parts(by_currency),
    }
}

/// What the account earned and was charged over the twelve months ending with the month its
/// prices are from: dividends and interest (positive when received), tax withheld and fees
/// (negative when paid).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Earned {
    /// The months added up, first and last.
    pub from: String,
    pub to: String,
    pub dividends: i64,
    pub interest: i64,
    pub tax: i64,
    pub fees: i64,
    pub other: i64,
    /// Commissions on trades, over the period the broker's statement gave them for.
    pub commissions: Option<Commissions>,
}
/// None when the broker's statement didn't say.
pub fn earned(held: &Holdings) -> Option<Earned> {
    if held.income.is_empty() && held.commissions.is_none() {
        return None;
    }
    let to = held.as_of.get(..7).unwrap_or(&held.as_of).to_string();
    let from = months_before(&format!("{to}-01"), 11).get(..7).unwrap_or(&to).to_string();
    let mut sum = Earned { from, to, dividends: 0, interest: 0, tax: 0, fees: 0, other: 0, commissions: held.commissions.clone() };
    for m in held.income.iter().filter(|m| m.month >= sum.from && m.month <= sum.to) {
        sum.dividends += m.dividends;
        sum.interest += m.interest;
        sum.tax += m.tax;
        sum.fees += m.fees;
        sum.other += m.other;
    }
    Some(sum)
}

/// One period of the page: how the account did, and its ups and downs when there are days enough.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Period {
    pub period: &'static str,
    #[serde(flatten)]
    pub did: Performance,
    pub swings: Option<Swings>,
}
/// A holding as the page lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub symbol: String,
    pub name: String,
    /// What a group of its kind is called, and one of them ("Stocks", "Stock").
    pub group: &'static str,
    pub one: &'static str,
    /// What it is, more finely (`instrument`): "ETF", "Call option", else as `one`.
    pub instrument: &'static str,
    pub currency: String,
    pub quantity: String,
    pub price: String,
    pub value: i64,
    /// What it is worth in its own currency, as a plain decimal; None when the source didn't say.
    pub amount: Option<String>,
    pub gain: Option<i64>,
    /// Its gain as a share of what it cost, and its value as a share of the account.
    pub rate: Option<f64>,
    pub share: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CashRow {
    pub currency: String,
    pub amount: String,
    pub value: i64,
    pub share: Option<f64>,
}
/// A holding named for a fact about it, with the share or return that makes it one.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Named {
    pub name: String,
    pub rate: f64,
}
/// Everything an investment account's page shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub account_id: String,
    /// Where the figures come from: "ibkr", or one of `investments::SOURCES`.
    pub source: String,
    pub source_account: String,
    pub as_of: String,
    pub total: i64,
    /// Every valued day, the value on it, and the gain since the first of them: a period's
    /// running gain is this from its `first` day on, less the figure on that day.
    pub days: Vec<String>,
    pub values: Vec<i64>,
    pub gains: Vec<i64>,
    /// In the order of `PERIODS`; empty before the account was ever valued.
    pub periods: Vec<Period>,
    pub day_change: Option<DayChange>,
    /// The last twelve months the account was valued in, each with its gain.
    pub monthly: Vec<Month>,
    pub totals: Totals,
    pub holdings: Vec<Row>,
    pub cash: Vec<CashRow>,
    /// Dividends and interest earned but not yet paid, when enough to show; else 0.
    pub accrued: i64,
    /// The largest single line among holdings, cash and accrued: what the share bars are drawn against.
    pub largest: i64,
    pub biggest: Option<Named>,
    /// The holdings that gained most and least against their cost, when two or more have one.
    pub best: Option<Named>,
    pub worst: Option<Named>,
    pub earned: Option<Earned>,
    /// The days counted as money moved on an estimate, with what was counted.
    pub estimated: Vec<Day>,
    /// Sections the broker's statement left out, by the name the broker shows.
    pub missing: Vec<String>,
}
/// The page for `account`, or None when no broker fills it.
pub fn page(b: &Budget, account: &str) -> Option<Page> {
    let held = investments::find(b, account)?;
    let flows = flows_of(b, &held);
    let periods: Vec<Period> = PERIODS
        .iter()
        .filter_map(|period| {
            let did = performance(&held.values, &flows, period)?;
            let swings = (did.values.len() > 2).then(|| swings(&did));
            Some(Period { period, did, swings })
        })
        .collect();
    let gains = periods.last().map(|all| all.did.gains.clone()).unwrap_or_default();
    let share = |value: i64| (held.total > 0 && value > 0).then(|| value as f64 / held.total as f64);
    let rate = |gain: Option<i64>, cost: Option<i64>| Some(gain? as f64 / cost.filter(|c| *c != 0)?.abs() as f64);
    let name = |p: &investments::Holding| if p.name.is_empty() { p.symbol.clone() } else { p.name.clone() };
    let holdings: Vec<Row> = held
        .positions
        .iter()
        .map(|p| Row {
            symbol: p.symbol.clone(),
            name: name(p),
            group: kind(&p.kind).0,
            one: kind(&p.kind).1,
            instrument: instrument(&p.kind, &p.sub_kind, &p.put_call),
            currency: p.currency.clone(),
            quantity: p.quantity.clone(),
            price: p.price.clone(),
            value: p.value,
            amount: (!p.amount.is_empty()).then(|| in_minor_units(&p.amount, &p.currency)),
            gain: p.gain,
            rate: rate(p.gain, p.cost),
            share: share(p.value),
        })
        .collect();
    let biggest = held.positions.iter().rev().max_by_key(|p| p.value).and_then(|p| Some(Named { name: name(p), rate: share(p.value)? }));
    let mut ranked: Vec<Named> = held.positions.iter().filter_map(|p| Some(Named { name: name(p), rate: rate(p.gain, p.cost)? })).collect();
    ranked.sort_by(|a, b| b.rate.total_cmp(&a.rate));
    let (best, worst) = if ranked.len() > 1 { (ranked.first().cloned(), ranked.last().cloned()) } else { (None, None) };
    let largest = held.positions.iter().map(|p| p.value).chain(held.cash.iter().map(|c| c.value)).chain([accrued(&held), 1]).max().unwrap_or(1);
    let mut monthly = monthly(&held, &flows);
    monthly.drain(..monthly.len().saturating_sub(12));
    Some(Page {
        days: held.values.iter().map(|(day, _)| day.clone()).collect(),
        values: held.values.iter().map(|(_, value)| *value).collect(),
        gains,
        periods,
        day_change: day_change(&held, &flows),
        monthly,
        totals: totals(&held),
        holdings,
        cash: held.cash.iter().map(|c| CashRow { currency: c.currency.clone(), amount: in_minor_units(&c.amount, &c.currency), value: c.value, share: share(c.value) }).collect(),
        accrued: accrued(&held),
        largest,
        biggest,
        best,
        worst,
        earned: earned(&held),
        estimated: held.estimated.iter().map(|day| Day { date: day.clone(), amount: flows.iter().find(|(d, _)| d == day).map_or(0, |(_, amount)| *amount) }).collect(),
        missing: held.missing.clone(),
        account_id: held.account_id,
        source: held.source,
        source_account: held.source_account,
        as_of: held.as_of,
        total: held.total,
    })
}

/// How the investments did over a range of months: all of them together, or one account.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Span {
    /// The value at the end of the month before the range, and at the range's end.
    pub start: i64,
    pub end: i64,
    /// Money put in, less money taken out; and what the value changed by beyond it.
    pub put: i64,
    pub gain: i64,
    /// The gain as a share of the money at work (see `dietz`), and the same as a yearly rate
    /// when it is measured over more than a year.
    pub rate: Option<f64>,
    pub yearly: Option<f64>,
    /// The days the return is measured over.
    pub days: i64,
    /// What trades realised in the range: a part of the gain, not beside it. None where no
    /// statement listed the trades of any of its months.
    pub realised: Option<i64>,
}
/// One month of the range: the accounts' balances at its end, as Net worth has them.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthValue {
    pub month: String,
    pub value: i64,
    /// The accounts' market changes in the month, and the rest of what the value moved by.
    pub gain: i64,
    pub put: i64,
    /// What trades realised in the month, where a statement listed them.
    pub realised: Option<i64>,
    /// Each account's balance, from the month it joins.
    pub by_account: BTreeMap<String, i64>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Slice {
    pub key: String,
    pub value: i64,
}
/// What an account's balance at a month's end was made of.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parts {
    pub month: String,
    /// The day the split is from.
    pub as_of: String,
    /// Its holdings by kind (the group names of `kind`), largest first.
    pub by_kind: Vec<Slice>,
    pub cash: i64,
    pub accrued: i64,
    /// The balance beyond those: what it has moved by since the split's day, or all of an
    /// account nothing breaks down.
    pub rest: i64,
}
/// One account over the range.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSpan {
    pub account_id: String,
    pub source: String,
    pub source_account: String,
    pub as_of: String,
    /// The first day it was valued: it counts from then, and its value that day as money put in.
    pub joined: String,
    /// As `Span`; the start is 0 for an account that joins inside the range.
    pub start: i64,
    pub end: i64,
    pub put: i64,
    pub gain: i64,
    pub rate: Option<f64>,
    /// What its trades realised in the range, where its statements listed them.
    pub realised: Option<i64>,
    /// How many days of the range were counted as money moved on an estimate.
    pub estimated_days: usize,
    pub missing: Vec<String>,
    /// Valued only when brought in, so a gain shows in the month of the next value.
    pub sparse: bool,
    /// The balance at the end of the month before the range, and at the range's end, split by
    /// what it was in; None where what it held then isn't known.
    pub start_parts: Option<Parts>,
    pub end_parts: Option<Parts>,
    /// Today's cash, currency by currency: the amount in that currency, and its value.
    pub cash: Vec<investments::CashHolding>,
}
/// What the accounts hold today, added up.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Allocation {
    /// The oldest day any account's prices are from.
    pub as_of: String,
    /// The positive parts by kind added up: what the holdings' shares are of.
    pub total: i64,
    /// How many holdings, once the same one in two accounts is one.
    pub count: usize,
    pub cash: i64,
    pub accrued: i64,
    pub cost: Option<i64>,
    pub unrealised: Option<i64>,
    pub unrealised_rate: Option<f64>,
    pub largest_share: Option<f64>,
    pub top_five_share: Option<f64>,
    /// By kind of holding (the group names of `kind`, "Cash", "Accrued"), by account id and
    /// by currency: each largest first, with its share of that list's positive parts.
    pub by_kind: Vec<Part>,
    pub by_account: Vec<Part>,
    pub by_currency: Vec<Part>,
}
/// A holding across the accounts that have it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Merged {
    /// Its ISIN, else the broker's id for it, else its symbol and currency.
    pub key: String,
    pub symbol: String,
    pub name: String,
    pub group: &'static str,
    pub one: &'static str,
    /// What it is, more finely (`instrument`).
    pub instrument: &'static str,
    pub currency: String,
    pub value: i64,
    /// What it is worth in `currency`, added up across the accounts as decimals; None unless
    /// every account holding it gave one in that currency.
    pub amount: Option<String>,
    pub cost: Option<i64>,
    pub gain: Option<i64>,
    /// Its gain as a share of what it cost, and its value as a share of `Allocation::total`.
    pub rate: Option<f64>,
    pub share: f64,
    pub accounts: Vec<String>,
}
/// Commissions on an account's trades, over the period its statement gave them for.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paid {
    pub account_id: String,
    pub from: String,
    pub to: String,
    pub amount: i64,
}
/// What one account earned and was charged over the range.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIncome {
    pub account_id: String,
    pub dividends: i64,
    pub interest: i64,
    pub tax: i64,
    pub fees: i64,
    pub other: i64,
    /// Commissions on its trades in the range (negative): the months' own where its
    /// statements listed the trades, else its statement's one figure when that statement's
    /// period meets the range, else None.
    pub commissions: Option<i64>,
}
/// What the accounts earned and were charged in a month.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct EarnedMonth {
    pub month: String,
    pub dividends: i64,
    pub interest: i64,
    pub tax: i64,
    pub fees: i64,
    pub other: i64,
    /// Commissions on the month's trades (negative), where a statement listed them.
    pub commissions: Option<i64>,
}
/// What one holding paid over the range, across the accounts that had it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HoldingEarned {
    /// As `holding_key` names it, so it is the holding's key among `Portfolio::holdings`.
    pub key: String,
    pub symbol: String,
    pub name: String,
    pub dividends: i64,
    pub tax: i64,
    /// What it is worth today where it is still held, and the dividends as a share of that.
    pub value: Option<i64>,
    #[serde(rename = "yield")]
    pub yielded: Option<f64>,
}
/// What the accounts earned and were charged over the range.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Income {
    /// Every month of the range.
    pub months: Vec<EarnedMonth>,
    /// Dividends by holding, largest first, from the accounts whose statements name them.
    pub by_holding: Vec<HoldingEarned>,
    /// The same by account: one row for each of `reporting`, in its order.
    pub by_account: Vec<AccountIncome>,
    pub dividends: i64,
    pub interest: i64,
    pub tax: i64,
    pub fees: i64,
    pub other: i64,
    /// Dividends and interest over the average month-end value of the accounts that report them.
    #[serde(rename = "yield")]
    pub yielded: Option<f64>,
    /// Each statement's commissions as one figure, for the accounts whose trades aren't
    /// known by month.
    pub commissions: Vec<Paid>,
    /// The accounts whose source reports income and costs, and the ones it doesn't.
    pub reporting: Vec<String>,
    pub silent: Vec<String>,
}
/// The investment accounts together, for Reports.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Portfolio {
    pub from: String,
    pub to: String,
    pub total: Span,
    pub months: Vec<MonthValue>,
    /// The accounts with holdings, in the budget's order.
    pub accounts: Vec<AccountSpan>,
    pub allocation: Allocation,
    /// Today's holdings, largest first.
    pub holdings: Vec<Merged>,
    pub income: Income,
    /// Open investment and retirement accounts that nothing fills.
    pub unfilled: Vec<String>,
}

/// An account's balance at a month's end by what it was in: the snapshot kept for that month,
/// or what it holds now for a month from its prices' on with no snapshot after it. None for a
/// month whose holdings weren't kept.
fn made_of(held: &Holdings, month: &str, balance: i64) -> Option<Parts> {
    let split = |as_of: &str, kinds: Vec<(String, i64)>, cash: i64, accrued: i64| {
        let known = kinds.iter().map(|(_, value)| value).sum::<i64>() + cash + accrued;
        let mut by_kind: Vec<Slice> = kinds.into_iter().filter(|(_, value)| *value != 0).map(|(key, value)| Slice { key, value }).collect();
        by_kind.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.key.cmp(&b.key)));
        Parts { month: month.to_string(), as_of: as_of.to_string(), by_kind, cash, accrued, rest: balance - known }
    };
    if let Some(kept) = held.snapshots.iter().find(|s| s.month == month) {
        return Some(split(&kept.as_of, kept.by_kind.iter().map(|(key, value)| (key.clone(), *value)).collect(), kept.cash, kept.other));
    }
    let newest = held.snapshots.iter().map(|s| s.month.as_str()).max().unwrap_or("");
    let priced = held.as_of.get(..7).unwrap_or(&held.as_of);
    (month > newest && month >= priced).then(|| split(&held.as_of, kinds(&held.positions), held.cash.iter().map(|c| c.value).sum(), held.other))
}

/// An amount in the budget's currency (`value`, in hundredths) in the account's own currency,
/// at the rate the account's figures were turned into the budget's with, to that currency's
/// minor units. None when the account has no rate that reads.
fn accrued_in(held: &Holdings, value: i64) -> Option<String> {
    use crate::flex::Dec;
    let rate = Dec::parse(&held.rate).filter(|rate| !rate.is_zero() && !rate.is_negative())?;
    let own = Dec::parse(&plain(value))?.over(rate)?;
    Some(in_minor_units(&own.text(), &held.currency))
}

/// The investment accounts a report's filter includes, over its months.
/// - Each month's value is the accounts' balances at its end and its gain their market
///   changes, so the figures are the ones Net worth has.
/// - An account counts from the first day it was valued, with its value that day as money
///   put in: starting to track an account that already existed is never a gain.
/// - What they hold, and the holdings themselves, are today's whatever the range.
pub fn portfolio(b: &Budget, f: &reports::Filter) -> Result<Portfolio, String> {
    crate::domain::validate(b)?;
    let months = reports::months_in(f)?;
    let (first, last) = (months[0].as_str(), months[months.len() - 1].as_str());
    let all = investments::read(b);
    let wanted = |id: &str| f.account_ids.is_empty() || f.account_ids.iter().any(|a| a == id);
    let held: Vec<&Holdings> =
        b.accounts.iter().filter(|a| wanted(&a.id)).filter_map(|a| all.iter().find(|h| h.account_id == a.id && !h.values.is_empty())).collect();
    // The range runs from the last day of the month before it to its own last day.
    let before = date(&format!("{first}-01")).and_then(|d| d.pred_opt()).map(|d| d.to_string()).unwrap_or_default();
    let through = date(&format!("{last}-01")).and_then(|d| d.checked_add_months(Months::new(1))).and_then(|d| d.pred_opt()).map(|d| d.to_string()).unwrap_or_default();
    let (before, through) = (before.as_str(), through.as_str());

    // From the ledger: each account's balance going into the range, then what each month
    // moved it by and how much of that was the market's.
    let place: HashMap<&str, usize> = held.iter().enumerate().map(|(i, h)| (h.account_id.as_str(), i)).collect();
    let slot: HashMap<&str, usize> = months.iter().enumerate().map(|(i, m)| (m.as_str(), i)).collect();
    let mut balance: Vec<i64> = held.iter().map(|h| b.accounts.iter().find(|a| a.id == h.account_id).map_or(0, |a| a.opening_balance)).collect();
    let mut moved = vec![vec![0_i64; held.len()]; months.len()];
    let mut market = moved.clone();
    for t in ledger::transactions(b) {
        let month = t.date.get(..7).unwrap_or("");
        if month > last {
            continue;
        }
        let at = slot.get(month).copied();
        for (account, amount) in [(Some(t.account_id.as_str()), t.amount), (t.transfer_account_id.as_deref(), -t.amount)] {
            let Some(i) = account.and_then(|id| place.get(id)).copied() else { continue };
            match at {
                Some(m) => moved[m][i] += amount,
                None => balance[i] += amount,
            }
        }
        if let (Some(m), Some(i)) = (at, place.get(t.account_id.as_str()).copied()) {
            if investments::market_change(&t) {
                market[m][i] += t.amount;
            }
        }
    }

    // What trades realised and cost, in the months a statement listed them for.
    let mut realised_in: Vec<Option<i64>> = vec![None; months.len()];
    let mut commissions_in = realised_in.clone();
    let mut realised_by: Vec<Option<i64>> = vec![None; held.len()];
    for (i, h) in held.iter().enumerate() {
        for row in &h.trades {
            let Some(m) = slot.get(row.month.as_str()).copied() else { continue };
            realised_in[m] = Some(realised_in[m].unwrap_or(0) + row.realised);
            commissions_in[m] = Some(commissions_in[m].unwrap_or(0) + row.commissions);
            realised_by[i] = Some(realised_by[i].unwrap_or(0) + row.realised);
        }
    }
    let joined: Vec<&str> = held.iter().map(|h| h.values[0].0.as_str()).collect();
    let mut running = balance.clone();
    let mut gains = vec![0_i64; held.len()];
    let start: i64 = (0..held.len()).filter(|i| joined[*i] <= before).map(|i| balance[i]).sum();
    let mut last_value = start;
    let mut by_month = vec![];
    for (m, month) in months.iter().enumerate() {
        let mut row = MonthValue { month: month.clone(), value: 0, gain: 0, put: 0, realised: realised_in[m], by_account: BTreeMap::new() };
        for (i, h) in held.iter().enumerate() {
            running[i] += moved[m][i];
            if joined[i].get(..7).is_none_or(|began| began > month.as_str()) {
                continue;
            }
            row.value += running[i];
            row.gain += market[m][i];
            gains[i] += market[m][i];
            row.by_account.insert(h.account_id.clone(), running[i]);
        }
        row.put = row.value - last_value - row.gain;
        last_value = row.value;
        by_month.push(row);
    }

    // Each account, and the money that went into all of them, for the return.
    let mut accounts = vec![];
    let mut put_in: Vec<(String, i64)> = vec![];
    let (mut began, mut ended): (Option<&str>, Option<&str>) = (None, None);
    for (i, h) in held.iter().enumerate() {
        // Tracked before the range, and by its end.
        let (earlier, tracked) = (joined[i] <= before, joined[i] <= through);
        let on_or_before = |day: &str| h.values.iter().rev().find(|(d, _)| d.as_str() <= day).map(|(d, _)| d.as_str());
        // Measured from its last valued day before the range, or the day it joined, to its last in the range.
        let since = if earlier { on_or_before(before).unwrap_or(joined[i]) } else { joined[i] };
        let until = on_or_before(through);
        let flows: Vec<(String, i64)> = if tracked { flows_of(b, h).into_iter().filter(|(day, _)| day.as_str() > since && day.as_str() <= through).collect() } else { vec![] };
        let (start, end, gain) = (if earlier { balance[i] } else { 0 }, if tracked { running[i] } else { 0 }, gains[i]);
        let opened = if earlier { start } else { h.values[0].1 };
        let rate = until.and_then(|until| dietz(opened, gain, flows.iter().map(|(day, amount)| (day.as_str(), *amount)), since, until));
        if tracked {
            if !earlier {
                put_in.push((joined[i].to_string(), h.values[0].1));
            }
            let from = if earlier { before } else { joined[i] };
            began = Some(began.map_or(from, |day| day.min(from)));
            ended = ended.max(until);
        }
        accounts.push(AccountSpan {
            account_id: h.account_id.clone(),
            source: h.source.clone(),
            source_account: h.source_account.clone(),
            as_of: h.as_of.clone(),
            joined: joined[i].to_string(),
            start,
            end,
            put: end - start - gain,
            gain,
            rate,
            realised: realised_by[i],
            estimated_days: h.estimated.iter().filter(|day| day.as_str() > since && day.as_str() <= through).count(),
            missing: h.missing.clone(),
            sparse: h.source != "ibkr",
            start_parts: if earlier { made_of(h, before.get(..7).unwrap_or(""), balance[i]) } else { None },
            end_parts: if tracked { made_of(h, last, running[i]) } else { None },
            cash: h.cash.iter().map(|c| investments::CashHolding { amount: in_minor_units(&c.amount, &c.currency), ..c.clone() }).collect(),
        });
        put_in.extend(flows);
    }
    let (end, gain): (i64, i64) = (accounts.iter().map(|a| a.end).sum(), gains.iter().sum());
    let rate = began.zip(ended).and_then(|(from, to)| dietz(start, gain, put_in.iter().map(|(day, amount)| (day.as_str(), *amount)), from, to));
    // A yearly rate is over the days the return is measured on.
    let measured: Vec<(&str, i64)> = put_in.iter().map(|(day, amount)| (day.as_str(), *amount)).collect();
    let length = began.zip(ended).map_or(0, |(from, to)| (days(to) - days(at_work_from(start, &measured, from))).max(0));
    let total = Span {
        start,
        end,
        put: end - start - gain,
        gain,
        rate,
        yearly: rate.filter(|rate| length > 366 && *rate > -1.0).map(|rate| (1.0 + rate).powf(365.0 / length as f64) - 1.0),
        days: length,
        realised: realised_by.iter().flatten().copied().reduce(|sum, realised| sum + realised),
    };

    // What they hold today. An account's value that nothing breaks down is a holding of no
    // known kind, in the account's currency, so every list adds up to the accounts' values.
    let (mut by_kind, mut by_currency, mut by_account) = (vec![], vec![], vec![]);
    let (mut cash, mut accrued) = (0, 0);
    let (mut cost, mut unrealised): (Option<i64>, Option<i64>) = (None, None);
    let mut merged: Vec<Merged> = vec![];
    // What each currency's part is in that currency, while every piece of it is known: a
    // holding's or cash's own amount, or, in the budget's currency, its value.
    let mut in_currency: Vec<(String, Option<String>)> = vec![];
    let mut add_in = |currency: &str, written: &str, value: i64| {
        let own = if !written.is_empty() { Some(written.to_string()) } else { (currency == b.currency).then(|| plain(value)) };
        match in_currency.iter_mut().find(|(key, _)| key == currency) {
            Some((_, sum)) => *sum = sum.as_deref().zip(own.as_deref()).and_then(|(a, b)| crate::flex::add_plain(a, b)),
            None => in_currency.push((currency.to_string(), own)),
        }
    };
    for h in &held {
        let sums = totals(h);
        let invested: i64 = h.positions.iter().map(|p| p.value).sum();
        for (key, value) in kinds(&h.positions) {
            add(&mut by_kind, &key, value);
        }
        for p in &h.positions {
            add(&mut by_currency, &p.currency, p.value);
            add_in(&p.currency, &p.amount, p.value);
        }
        for c in &h.cash {
            add(&mut by_kind, "Cash", c.value);
            add(&mut by_currency, &c.currency, c.value);
            add_in(&c.currency, &c.amount, c.value);
        }
        let rest = h.total - invested - sums.cash - h.other;
        for (key, value) in [("Accrued", h.other), (kind("").0, rest)] {
            if value != 0 {
                add(&mut by_kind, key, value);
                add(&mut by_currency, &h.currency, value);
                // Accrued income is in the account's own currency: its value turned back with
                // the rate it was turned into the budget's with, when there is one.
                let own = if key == "Accrued" { accrued_in(h, value).unwrap_or_default() } else { String::new() };
                add_in(&h.currency, &own, value);
            }
        }
        add(&mut by_account, &h.account_id, h.total);
        cash += sums.cash;
        accrued += h.other;
        if let (Some(paid), Some(gained)) = (sums.cost, sums.unrealised) {
            cost = Some(cost.unwrap_or(0) + paid);
            unrealised = Some(unrealised.unwrap_or(0) + gained);
        }
        for p in &h.positions {
            let key = holding_key(&p.isin, &p.conid, &p.symbol, &p.name, &p.currency);
            let both = |a: Option<i64>, b: Option<i64>| if a.is_none() && b.is_none() { None } else { Some(a.unwrap_or(0) + b.unwrap_or(0)) };
            let amount = (!p.amount.is_empty()).then(|| p.amount.clone());
            match merged.iter_mut().find(|row| row.key == key) {
                Some(row) => {
                    // Added up only while every account's is known and in the same currency.
                    row.amount = row.amount.as_deref().zip(amount.as_deref()).filter(|_| row.currency == p.currency).and_then(|(a, b)| crate::flex::add_plain(a, b));
                    row.value += p.value;
                    row.cost = both(row.cost, p.cost);
                    row.gain = both(row.gain, p.gain);
                    if !row.accounts.contains(&h.account_id) {
                        row.accounts.push(h.account_id.clone());
                    }
                }
                None => merged.push(Merged {
                    key,
                    symbol: p.symbol.clone(),
                    name: if p.name.is_empty() { p.symbol.clone() } else { p.name.clone() },
                    group: kind(&p.kind).0,
                    one: kind(&p.kind).1,
                    instrument: instrument(&p.kind, &p.sub_kind, &p.put_call),
                    currency: p.currency.clone(),
                    value: p.value,
                    amount,
                    cost: p.cost,
                    gain: p.gain,
                    rate: None,
                    share: 0.0,
                    accounts: vec![h.account_id.clone()],
                }),
            }
        }
    }
    // Largest first, each a share of the list's positive parts; what is owed is listed and is no share.
    let shares = |sums: Vec<(String, i64)>| parts(sums).into_iter().map(|part| Part { share: part.share.max(0.0), ..part }).collect::<Vec<_>>();
    let whole: i64 = by_kind.iter().map(|(_, value)| *value).filter(|value| *value > 0).sum();
    let share = |value: i64| if whole > 0 && value > 0 { value as f64 / whole as f64 } else { 0.0 };
    merged.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.key.cmp(&b.key)));
    for row in &mut merged {
        row.rate = row.gain.zip(row.cost.filter(|cost| *cost != 0)).map(|(gain, cost)| gain as f64 / cost.abs() as f64);
        row.share = share(row.value);
        row.amount = row.amount.as_deref().map(|amount| in_minor_units(amount, &row.currency));
    }
    let allocation = Allocation {
        as_of: held.iter().map(|h| h.as_of.as_str()).min().unwrap_or("").to_string(),
        total: whole,
        count: merged.len(),
        cash,
        accrued,
        cost,
        unrealised,
        unrealised_rate: unrealised.zip(cost.filter(|cost| *cost > 0)).map(|(gained, paid)| gained as f64 / paid as f64),
        largest_share: merged.first().filter(|_| whole > 0).map(|row| share(row.value)),
        top_five_share: (!merged.is_empty() && whole > 0).then(|| merged.iter().take(5).map(|row| share(row.value)).sum()),
        by_kind: shares(by_kind),
        by_account: shares(by_account),
        by_currency: shares(by_currency)
            .into_iter()
            .map(|part| Part { amount: Some(in_currency.iter().find(|(key, _)| *key == part.key).and_then(|(_, sum)| sum.as_deref().map(|sum| in_minor_units(sum, &part.key)))), ..part })
            .collect(),
    };

    // What they earned and were charged in the range's months.
    let mut earned: Vec<EarnedMonth> = months.iter().zip(&commissions_in).map(|(month, commissions)| EarnedMonth { month: month.clone(), commissions: *commissions, ..EarnedMonth::default() }).collect();
    // A statement's commissions as one figure, for an account whose trades aren't known by
    // month, when the statement's period meets the range.
    let one_figure = |h: &Holdings| h.commissions.clone().filter(|paid| h.trades.is_empty() && paid.from.as_str() <= through && paid.to.as_str() > before);
    let mut by_account: Vec<AccountIncome> = vec![];
    for h in held.iter().filter(|h| !h.income.is_empty()) {
        let by_month = h.trades.iter().filter(|row| slot.contains_key(row.month.as_str())).map(|row| row.commissions).reduce(|sum, paid| sum + paid);
        let commissions = by_month.or_else(|| one_figure(h).map(|paid| paid.amount));
        let mut own = AccountIncome { account_id: h.account_id.clone(), commissions, ..AccountIncome::default() };
        for row in &h.income {
            let Some(sum) = slot.get(row.month.as_str()).map(|m| &mut earned[*m]) else { continue };
            sum.dividends += row.dividends;
            sum.interest += row.interest;
            sum.tax += row.tax;
            sum.fees += row.fees;
            sum.other += row.other;
            own.dividends += row.dividends;
            own.interest += row.interest;
            own.tax += row.tax;
            own.fees += row.fees;
            own.other += row.other;
        }
        by_account.push(own);
    }
    // By holding, with what each is worth today where it is still among the holdings.
    let mut by_holding: Vec<HoldingEarned> = vec![];
    for row in held.iter().flat_map(|h| &h.income_by).filter(|row| slot.contains_key(row.month.as_str())) {
        match by_holding.iter_mut().find(|sum| sum.key == row.key) {
            Some(sum) => {
                sum.dividends += row.dividends;
                sum.tax += row.tax;
            }
            None => {
                let value = merged.iter().find(|h| h.key == row.key).map(|h| h.value);
                by_holding.push(HoldingEarned { key: row.key.clone(), symbol: row.symbol.clone(), name: row.name.clone(), dividends: row.dividends, tax: row.tax, value, yielded: None });
            }
        }
    }
    for sum in &mut by_holding {
        sum.yielded = sum.value.filter(|value| *value > 0).map(|value| sum.dividends as f64 / value as f64);
    }
    by_holding.sort_by(|a, b| b.dividends.cmp(&a.dividends).then_with(|| a.key.cmp(&b.key)));
    let sum = |of: fn(&EarnedMonth) -> i64| earned.iter().map(of).sum::<i64>();
    let (dividends, interest) = (sum(|m| m.dividends), sum(|m| m.interest));
    let reports_income = |h: &Holdings| !h.income.is_empty();
    // The average over the months any of the accounts that report income was in.
    let valued: Vec<i64> = by_month
        .iter()
        .filter_map(|row| {
            let values: Vec<i64> = held.iter().filter(|h| reports_income(h)).filter_map(|h| row.by_account.get(&h.account_id).copied()).collect();
            (!values.is_empty()).then(|| values.iter().sum())
        })
        .collect();
    let average = if valued.is_empty() { 0.0 } else { valued.iter().sum::<i64>() as f64 / valued.len() as f64 };
    let ids = |keep: bool| held.iter().filter(|h| reports_income(h) == keep).map(|h| h.account_id.clone()).collect::<Vec<_>>();
    let income = Income {
        dividends,
        interest,
        tax: sum(|m| m.tax),
        fees: sum(|m| m.fees),
        other: sum(|m| m.other),
        yielded: (average > 0.0).then(|| (dividends + interest) as f64 / average),
        commissions: held.iter().filter_map(|h| one_figure(h).map(|paid| Paid { account_id: h.account_id.clone(), from: paid.from, to: paid.to, amount: paid.amount })).collect(),
        reporting: ids(true),
        silent: ids(false),
        months: earned,
        by_account,
        by_holding,
    };
    let unfilled = b
        .accounts
        .iter()
        .filter(|a| !a.archived && a.kind == AccountKind::Tracking && !all.iter().any(|h| h.account_id == a.id))
        .filter(|a| a.extra.get("accountType").and_then(|v| v.as_str()).is_some_and(|kind| kind == "investment" || kind == "retirement"))
        .map(|a| a.id.clone())
        .collect();
    Ok(Portfolio { from: f.from.clone(), to: f.to.clone(), total, months: by_month, accounts, allocation, holdings: merged, income, unfilled })
}

/// An amount in hundredths as a plain decimal, as the reports' own file writes them.
fn plain(amount: i64) -> String {
    format!("{}{}.{:02}", if amount < 0 { "-" } else { "" }, amount.unsigned_abs() / 100, amount.unsigned_abs() % 100)
}
/// The investments report as a CSV file: its months with each account's value, then today's
/// holdings, then what the accounts earned and were charged by month, by account and by holding.
pub fn csv(b: &Budget, report: &Portfolio) -> Result<String, String> {
    // Blocks with different columns in one file, as the reports' own.
    let mut w = csv::WriterBuilder::new().flexible(true).from_writer(vec![]);
    let name = |id: &String| b.accounts.iter().find(|a| a.id == *id).map_or(id.clone(), |a| a.name.clone());
    // Columns for what trades realised and cost only where some statement listed its trades.
    let (realised, commissions) = (report.total.realised.is_some(), report.income.months.iter().any(|m| m.commissions.is_some()));
    let known = |amount: Option<i64>| amount.map(plain).unwrap_or_default();
    let mut header: Vec<String> = ["Month", "Value", "Money put in", "Gain"].map(String::from).to_vec();
    header.extend(realised.then(|| "Realised gain".to_string()));
    header.extend(report.accounts.iter().map(|a| name(&a.account_id)));
    w.write_record(&header).map_err(|e| e.to_string())?;
    for m in &report.months {
        let mut row = vec![m.month.clone(), plain(m.value), plain(m.put), plain(m.gain)];
        row.extend(realised.then(|| known(m.realised)));
        // Empty until the account joins.
        row.extend(report.accounts.iter().map(|a| m.by_account.get(&a.account_id).map(|value| plain(*value)).unwrap_or_default()));
        w.write_record(&row).map_err(|e| e.to_string())?;
    }
    if !report.holdings.is_empty() {
        w.write_record([""; 4]).map_err(|e| e.to_string())?;
        w.write_record(["Holding", "Symbol", "Kind", "Currency", "Value", "Cost", "Unrealised gain", "Share", "Accounts"]).map_err(|e| e.to_string())?;
        for h in &report.holdings {
            w.write_record([
                h.name.clone(),
                h.symbol.clone(),
                h.group.to_string(),
                h.currency.clone(),
                plain(h.value),
                h.cost.map(plain).unwrap_or_default(),
                h.gain.map(plain).unwrap_or_default(),
                format!("{:.1}%", h.share * 100.0),
                h.accounts.iter().map(name).collect::<Vec<_>>().join(", "),
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    if !report.income.reporting.is_empty() {
        w.write_record([""; 4]).map_err(|e| e.to_string())?;
        let mut header = vec!["Month", "Dividends", "Interest", "Tax", "Fees", "Other"];
        header.extend(commissions.then_some("Commissions"));
        w.write_record(&header).map_err(|e| e.to_string())?;
        for m in &report.income.months {
            let mut row = vec![m.month.clone(), plain(m.dividends), plain(m.interest), plain(m.tax), plain(m.fees), plain(m.other)];
            row.extend(commissions.then(|| known(m.commissions)));
            w.write_record(&row).map_err(|e| e.to_string())?;
        }
        w.write_record([""; 4]).map_err(|e| e.to_string())?;
        // Each account's commissions where any is known, by month or as its statement's one figure.
        let paid = report.income.by_account.iter().any(|a| a.commissions.is_some());
        let mut header = vec!["Account", "Dividends", "Interest", "Tax", "Fees", "Other"];
        header.extend(paid.then_some("Commissions"));
        w.write_record(&header).map_err(|e| e.to_string())?;
        for a in &report.income.by_account {
            let mut row = vec![name(&a.account_id), plain(a.dividends), plain(a.interest), plain(a.tax), plain(a.fees), plain(a.other)];
            row.extend(paid.then(|| known(a.commissions)));
            w.write_record(&row).map_err(|e| e.to_string())?;
        }
    }
    if !report.income.by_holding.is_empty() {
        w.write_record([""; 4]).map_err(|e| e.to_string())?;
        w.write_record(["Holding", "Symbol", "Dividends", "Tax", "Value", "Yield"]).map_err(|e| e.to_string())?;
        for h in &report.income.by_holding {
            let earned = h.yielded.map(|rate| format!("{:.2}%", rate * 100.0)).unwrap_or_default();
            w.write_record([h.name.clone(), h.symbol.clone(), plain(h.dividends), plain(h.tax), known(h.value), earned]).map_err(|e| e.to_string())?;
        }
    }
    String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::investments::{CashHolding, Holding, MonthIncome};
    use crate::test_support::sample;
    use serde_json::json;

    fn position(symbol: &str, name: &str, kind: &str, currency: &str, value: i64, cost: Option<i64>, gain: Option<i64>) -> Holding {
        Holding { symbol: symbol.into(), name: name.into(), kind: kind.into(), currency: currency.into(), quantity: "1".into(), price: "1".into(), value, cost, gain, ..Default::default() }
    }
    fn series(rows: &[(&str, i64)]) -> Vec<(String, i64)> {
        rows.iter().map(|(day, amount)| (day.to_string(), *amount)).collect()
    }
    /// The fixture the desktop's retired investmentStats.test.ts used.
    fn held() -> Holdings {
        Holdings {
            account_id: "broker".into(),
            source: "ibkr".into(),
            source_account: "U1".into(),
            as_of: "2026-11-13".into(),
            currency: "USD".into(),
            rate: "1".into(),
            total: 2_701_250,
            positions: vec![
                position("VWRA", "Fund", "STK", "USD", 1_710_000, Some(1_500_000), Some(210_000)),
                position("2222", "Oil", "STK", "SAR", 288_000, Some(320_000), Some(-32_000)),
                position("T", "Note", "BOND", "USD", 492_500, None, None),
            ],
            cash: vec![
                CashHolding { currency: "USD".into(), amount: "820".into(), value: 82_000 },
                CashHolding { currency: "SAR".into(), amount: "1500".into(), value: 40_000 },
            ],
            other: 1_250,
            values: series(&[("2025-12-31", 2_000_000), ("2026-09-30", 2_422_000), ("2026-10-30", 2_550_000), ("2026-11-12", 2_690_000), ("2026-11-13", 2_701_250)]),
            flows: Some(series(&[("2026-10-05", 100_000), ("2026-11-02", 40_000), ("2026-11-13", 5_000)])),
            income: vec![
                MonthIncome { month: "2025-10".into(), dividends: 99_999, ..Default::default() },
                MonthIncome { month: "2025-12".into(), dividends: 300, ..Default::default() },
                MonthIncome { month: "2026-10".into(), interest: 120, fees: -1_000, ..Default::default() },
                MonthIncome { month: "2026-11".into(), dividends: 650, tax: -98, ..Default::default() },
            ],
            commissions: Some(Commissions { from: "2025-11-14".into(), to: "2026-11-13".into(), amount: -350 }),
            ..Default::default()
        }
    }
    fn flows() -> Vec<(String, i64)> {
        held().flows.unwrap()
    }
    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-10)
    }

    #[test]
    fn a_periods_gain_is_measured_against_the_money_put_in_and_its_return_against_the_money_at_work() {
        // One month back lands on 13 October, so the period starts from the last valued day before it.
        let month = performance(&held().values,&flows(), "1m").unwrap();
        assert_eq!((month.from.as_str(), month.to.as_str(), month.start, month.end, month.first), ("2026-09-30", "2026-11-13", 2_422_000, 2_701_250, 1));
        // Three deposits since: 1,450.00 in all, so the gain is what's left of the 2,792.50 rise.
        assert_eq!((month.put, month.gain), (145_000, 134_250));
        // 44 days: the deposits count for the 39, 11 and 0 days they were there.
        assert!(close(month.rate, 134_250.0 / (2_422_000.0 + 100_000.0 * 39.0 / 44.0 + 40_000.0 * 11.0 / 44.0)));
        assert_eq!(month.gains, vec![0, 2_550_000 - 2_422_000 - 100_000, 2_690_000 - 2_422_000 - 140_000, 134_250]);
        // This year starts from the last value of last year; a year and all start from the first one.
        let year = performance(&held().values,&flows(), "ytd").unwrap();
        assert_eq!((year.from.as_str(), year.start, year.put, year.gain), ("2025-12-31", 2_000_000, 145_000, 556_250));
        assert_eq!(performance(&held().values,&flows(), "1y").unwrap().from, "2025-12-31");
        assert_eq!(performance(&held().values,&flows(), "all").unwrap().values.len(), 5);
        // An account that opened empty: all of its value was put in, and the return is on that
        // money from the day it arrived, not spread over the day it sat empty.
        let fresh = series(&[("2026-11-01", 0), ("2026-11-13", 101_000)]);
        let opened = performance(&fresh, &series(&[("2026-11-02", 100_000)]), "all").unwrap();
        assert_eq!((opened.gain, opened.put), (1_000, 100_000));
        assert!(close(opened.rate, 1_000.0 / 100_000.0));
        // Next to nothing for most of a year, then two deposits in its last weeks: a loss of 9.00
        // on the 200.00 put in reads as about -4.9%, not about -40%.
        let late = series(&[("2025-11-17", 19), ("2026-02-03", 263), ("2026-10-05", 10_263), ("2026-10-12", 20_000), ("2026-11-17", 19_219)]);
        let deposits = series(&[("2026-02-03", 244), ("2026-10-05", 10_000), ("2026-10-12", 10_000)]);
        let year = performance(&late, &deposits, "1y").unwrap();
        assert_eq!((year.put, year.gain), (20_244, 19_219 - 19 - 20_244));
        // From 5 October, when a twentieth of the money had arrived: the first deposit and what was
        // there before it for all 43 days, the second for its 36.
        assert!(close(year.rate, -1_044.0 / (19.0 + 244.0 + 10_000.0 + 10_000.0 * 36.0 / 43.0)));
        assert!(year.rate.unwrap() > -0.06);
        // An account that held money all along is measured from the period's start, as before.
        assert_eq!(at_work_from(2_000_000, &[("2026-10-05", 100_000)], "2025-12-31"), "2025-12-31");
        // Nothing at work to measure against, or nothing valued yet.
        assert_eq!(performance(&series(&[("2026-11-13", 0)]), &[], "all").unwrap().rate, None);
        assert_eq!(performance(&[], &[], "all"), None);
        // A month back from a 31st that the month before doesn't have is its last day.
        assert_eq!(months_before("2026-03-31", 1), "2026-02-28");
    }

    #[test]
    fn the_days_deposit_is_taken_out_of_the_days_change() {
        assert_eq!(day_change(&held(), &flows()), Some(DayChange { amount: 2_701_250 - 2_690_000 - 5_000, since: "2026-11-12".into() }));
        assert_eq!(day_change(&Holdings { values: series(&[("2026-11-13", 1)]), ..held() }, &[]), None);
    }

    #[test]
    fn the_brokers_record_of_deposits_is_used_or_the_budgets_for_holdings_from_before_it_was_kept() {
        let mut b = sample();
        let tx = |over: serde_json::Value| -> crate::domain::Transaction {
            let mut row = json!({"id": "x", "date": "2026-10-05", "accountId": "broker", "payee": "", "memo": "", "amount": 0, "status": "cleared",
                "splits": [], "transferAccountId": null, "transferStatus": null});
            row.as_object_mut().unwrap().extend(over.as_object().unwrap().clone());
            serde_json::from_value(row).unwrap()
        };
        b.transactions = vec![
            tx(json!({"amount": 100_000})),
            tx(json!({"accountId": "bank", "transferAccountId": "broker", "amount": -40_000, "date": "2026-11-02"})),
            tx(json!({"accountId": "bank", "amount": -7_000, "date": "2026-11-03", "splits": [{"categoryId": null, "amount": -7_000, "transferAccountId": "broker"}]})),
            // The lines written for the account's value aren't money put in, and other accounts' transactions aren't this one's.
            tx(json!({"amount": 999_999, "investment": "change"})),
            tx(json!({"accountId": "bank", "amount": -5})),
        ];
        assert_eq!(flows_of(&b, &held()), flows());
        assert_eq!(flows_of(&b, &Holdings { flows: None, ..held() }), series(&[("2026-10-05", 100_000), ("2026-11-02", 40_000), ("2026-11-03", 7_000)]));
    }

    #[test]
    fn what_it_holds_is_added_up_by_kind_and_by_currency() {
        let sums = totals(&held());
        // Cost and gain only over the holdings the broker gives a cost for.
        assert_eq!((sums.cost, sums.unrealised, sums.cash), (Some(1_820_000), Some(178_000), 122_000));
        assert!(close(sums.unrealised_rate, 178_000.0 / 1_820_000.0));
        let pairs = |parts: &[Part]| parts.iter().map(|p| (p.key.clone(), p.value)).collect::<Vec<_>>();
        assert_eq!(pairs(&sums.by_kind), vec![("Stocks".to_string(), 1_998_000), ("Bonds".into(), 492_500), ("Cash".into(), 122_000), ("Accrued".into(), 1_250)]);
        assert_eq!(pairs(&sums.by_currency), vec![("USD".to_string(), 2_284_500), ("SAR".into(), 328_000)]);
        assert!(close(Some(sums.by_kind.iter().map(|p| p.share).sum()), 1.0));
        assert_eq!(totals(&Holdings { positions: vec![held().positions[2].clone()], ..held() }).cost, None);
        assert_eq!((kind("OPT"), kind("???"), kind("FUND").1), (("Options", "Option"), ("Other holdings", "Other"), "Fund"));
    }

    #[test]
    fn twelve_months_of_income_and_costs_end_with_the_month_the_prices_are_from() {
        let paid = earned(&held()).unwrap();
        // December 2025 to November 2026: October 2025's dividend is too old to count.
        assert_eq!((paid.from.as_str(), paid.to.as_str()), ("2025-12", "2026-11"));
        assert_eq!((paid.dividends, paid.interest, paid.tax, paid.fees, paid.other), (950, 120, -98, -1_000, 0));
        assert_eq!(paid.commissions.map(|c| c.amount), Some(-350));
        assert_eq!(earned(&Holdings { income: vec![], commissions: None, ..held() }), None);
    }

    #[test]
    fn a_periods_best_and_worst_days_how_often_it_rose_and_its_furthest_fall_from_a_high() {
        // The fixture only ever rises: 4,220.00 by the end of September is its best stretch.
        let all = swings(&performance(&held().values,&flows(), "all").unwrap());
        assert_eq!(all.best, Some(Day { date: "2026-09-30".into(), amount: 422_000 }));
        assert_eq!((all.worst, all.drop, all.up, all.moved), (None, None, 4, 4));
        // One that rises and falls: the fall counted is the deepest from a high, not the first.
        let gains = vec![0, 500, 200, 800, 300, 300];
        let days = ["2026-11-02", "2026-11-03", "2026-11-04", "2026-11-05", "2026-11-06", "2026-11-09"];
        let did = Performance {
            values: days.iter().zip(&gains).map(|(day, gain)| (day.to_string(), 10_000 + gain)).collect(),
            gains,
            ..performance(&held().values,&flows(), "all").unwrap()
        };
        assert_eq!(swings(&did), Swings {
            best: Some(Day { date: "2026-11-05".into(), amount: 600 }),
            worst: Some(Day { date: "2026-11-06".into(), amount: -500 }),
            up: 2,
            moved: 4,
            drop: Some(Drop { amount: -500, from: "2026-11-05".into(), to: "2026-11-06".into(), rate: Some(-500.0 / 10_800.0) }),
        });
    }

    #[test]
    fn the_gain_is_split_by_month_leaving_the_money_put_in_out_of_each() {
        let months = monthly(&held(), &flows());
        assert_eq!(months.iter().map(|m| (m.month.as_str(), m.gain)).collect::<Vec<_>>(), vec![("2026-09", 422_000), ("2026-10", 28_000), ("2026-11", 106_250)]);
        // Together they are the gain since the first valued day.
        assert_eq!(months.iter().map(|m| m.gain).sum::<i64>(), performance(&held().values,&flows(), "all").unwrap().gain);
        assert!(monthly(&Holdings { values: vec![], ..held() }, &[]).is_empty());
    }

    #[test]
    fn the_page_has_every_period_from_one_run_of_gains_and_each_holdings_share_and_return() {
        let mut b = sample();
        b.extra.insert("investments".into(), json!([held()]));
        let page = page(&b, "broker").unwrap();
        assert_eq!(page.periods.iter().map(|p| p.period).collect::<Vec<_>>(), PERIODS);
        assert_eq!((page.days.len(), page.values.len(), page.gains.len()), (5, 5, 5));
        // A period's running gain is the whole run from its first day, less the figure there.
        for p in &page.periods {
            let from_all: Vec<i64> = page.gains[p.did.first..].iter().map(|gain| gain - page.gains[p.did.first]).collect();
            assert_eq!(from_all, p.did.gains, "{}", p.period);
            assert_eq!(p.swings.is_some(), p.did.values.len() > 2);
        }
        let fund = &page.holdings[0];
        assert_eq!((fund.name.as_str(), fund.group, fund.one, fund.value, fund.gain), ("Fund", "Stocks", "Stock", 1_710_000, Some(210_000)));
        assert!(close(fund.rate, 0.14) && close(fund.share, 1_710_000.0 / 2_701_250.0));
        // No cost from the broker, no return.
        assert_eq!(page.holdings[2].rate, None);
        assert_eq!((page.accrued, page.largest, page.cash.len()), (1_250, 1_710_000, 2));
        assert_eq!(page.biggest.as_ref().map(|n| n.name.as_str()), Some("Fund"));
        assert_eq!((page.best.as_ref().map(|n| n.name.as_str()), page.worst.as_ref().map(|n| n.name.as_str())), (Some("Fund"), Some("Oil")));
        assert_eq!(page.monthly.len(), 3);
        // As the phones read it.
        let read = crate::commands::read(&b, "investmentPage", &json!({"accountId": "broker"})).unwrap();
        assert_eq!(read["periods"][0]["period"], "1m");
        assert_eq!((read["periods"][0]["from"].as_str(), read["periods"][0]["gain"].as_i64()), (Some("2026-09-30"), Some(134_250)));
        assert_eq!(read["totals"]["byKind"][0]["key"], "Stocks");
        assert!(read["periods"][0].get("values").is_none());
        // An account no broker fills has no page.
        assert!(crate::commands::read(&b, "investmentPage", &json!({"accountId": "cash"})).unwrap().is_null());
    }

    /// The budget of `shared/fixtures/investments-report.json`, in dollars:
    /// - Long term: filled the way Interactive Brokers fills one, since November 2025, with the
    ///   broker's own record of money moved (one deposit the budget doesn't have yet).
    /// - Pension: a value typed in three times.
    /// - Trading: filled like Long term, from August 2026, so it joins inside the range. It
    ///   holds the same world fund as Long term.
    /// - Euro funds: a positions file in euros, brought in twice.
    /// - Old workplace plan: a retirement account nothing fills.
    /// Long term sends Pension 2,000.00 in April.
    fn fixture() -> Budget {
        let transfer = |id: &str, date: &str, from: &str, to: &str, amount: i64| {
            json!({"id": id, "date": date, "accountId": from, "payee": "", "memo": "", "amount": -amount, "status": "cleared",
                "splits": [{"categoryId": null, "amount": -amount, "unassigned": true}], "transferAccountId": to, "transferStatus": "cleared"})
        };
        let tracking = |id: &str, name: &str, kind: &str, opening: i64| json!({"id": id, "name": name, "kind": "tracking", "openingBalance": opening, "archived": false, "accountType": kind});
        let mut b: Budget = serde_json::from_value(json!({
            "schemaVersion": 2, "name": "Investments fixture", "currency": "USD",
            "fixtureNotice": "Synthetic data only; no real account or person.",
            "accounts": [
                {"id": "checking", "name": "Everyday", "kind": "cash", "openingBalance": 6_000_000, "archived": false},
                tracking("broker", "Long term", "investment", 0),
                tracking("pension", "Pension", "retirement", 0),
                tracking("trading", "Trading", "investment", 0),
                tracking("euro", "Euro funds", "investment", 0),
                tracking("old", "Old workplace plan", "retirement", 800_000),
            ],
            "categories": [{"id": "food", "name": "Food", "group": "Living", "target": 0}],
            "transactions": [
                transfer("to-broker", "2026-02-10", "checking", "broker", 500_000),
                transfer("broker-to-pension", "2026-04-15", "broker", "pension", 200_000),
                transfer("to-euro", "2026-05-05", "checking", "euro", 110_000),
                transfer("to-trading", "2026-08-03", "checking", "trading", 1_000_000),
                {"id": "groceries", "date": "2026-10-12", "accountId": "checking", "payee": "Grocer", "memo": "", "amount": -30_000, "status": "cleared",
                    "splits": [{"categoryId": "food", "amount": -30_000}], "transferAccountId": null, "transferStatus": null},
                transfer("more-to-trading", "2026-11-03", "checking", "trading", 150_000),
                transfer("more-to-euro", "2026-11-05", "checking", "euro", 20_000),
            ],
            "assignments": [],
        }))
        .unwrap();
        for (day, value) in [("2025-12-31", "20000"), ("2026-06-30", "21500"), ("2026-11-10", "23000")] {
            b = investments::record(b, "pension", investments::Snapshot { source: "manual".into(), as_of: day.into(), total: value.into(), ..Default::default() }).unwrap();
        }
        let listed = |symbol: &str, name: &str, kind: &str, value: &str, cost: &str| investments::Position {
            symbol: symbol.into(),
            name: name.into(),
            kind: kind.into(),
            quantity: "100".into(),
            price: "1".into(),
            value: value.into(),
            cost: cost.into(),
            ..Default::default()
        };
        let file = |day: &str, fund: (&str, &str), bond: &str, cash: &str| investments::Snapshot {
            source: "file".into(),
            source_account: "positions.csv".into(),
            as_of: day.into(),
            currency: "EUR".into(),
            rate: "1.1".into(),
            positions: vec![listed("EUNL", "Core world fund", "FUND", fund.0, fund.1), listed("BUND", "Federal bond 2034", "BOND", bond, "")],
            cash: cash.into(),
            ..Default::default()
        };
        b = investments::record(b, "euro", file("2026-02-27", ("9000", "8000"), "3000", "500")).unwrap();
        b = investments::record(b, "euro", file("2026-10-30", ("10400", "9000"), "3050", "250")).unwrap();
        let identified = |isin: &str, p: Holding| Holding { isin: isin.into(), ..p };
        let finer = |sub_kind: &str, put_call: &str, amount: &str, p: Holding| Holding { sub_kind: sub_kind.into(), put_call: put_call.into(), amount: amount.into(), ..p };
        let world = "IE00BK5BQT80";
        let long_term = Holdings {
            account_id: "broker".into(),
            source: "ibkr".into(),
            source_account: "U1111111".into(),
            as_of: "2026-11-17".into(),
            currency: "USD".into(),
            rate: "1".into(),
            total: 5_300_000,
            positions: vec![
                // The broker's finer category and what each is worth in its own currency, as a
                // statement read since they were kept gives them.
                finer("ETF", "", "32000.00", identified(world, position("VWRA", "World fund", "STK", "USD", 3_200_000, Some(2_700_000), Some(500_000)))),
                finer("COMMON", "", "9000.00", identified("US0378331005", position("AAPL", "Apple", "STK", "USD", 900_000, Some(600_000), Some(300_000)))),
                finer("US-T GOVT", "", "8000.00", identified("US91282CJZ59", position("T 4 02/15/34", "Treasury note", "BOND", "USD", 800_000, None, None))),
                // Sold, not bought: it takes from the account's value.
                finer("", "P", "-500.00", Holding { conid: "700123456".into(), ..position("SPY 260116P00500000", "SPY put", "OPT", "USD", -50_000, Some(-40_000), Some(-10_000)) }),
            ],
            cash: vec![
                CashHolding { currency: "USD".into(), amount: "4000".into(), value: 400_000 },
                CashHolding { currency: "SAR".into(), amount: "1406.25".into(), value: 37_500 },
            ],
            other: 12_500,
            values: series(&[
                ("2025-11-28", 4_000_000), ("2025-12-31", 4_100_000), ("2026-01-30", 4_180_000), ("2026-02-27", 4_750_000), ("2026-03-31", 4_700_000),
                ("2026-04-30", 4_560_000), ("2026-05-29", 4_640_000), ("2026-06-30", 4_700_000), ("2026-07-31", 4_820_000), ("2026-08-31", 4_790_000),
                ("2026-09-30", 4_950_000), ("2026-10-30", 5_120_000), ("2026-11-16", 5_280_000), ("2026-11-17", 5_300_000),
            ]),
            flows: Some(series(&[("2026-02-10", 500_000), ("2026-04-15", -200_000), ("2026-09-08", 100_000)])),
            estimated: vec!["2026-09-08".into()],
            income: vec![
                MonthIncome { month: "2025-12".into(), dividends: 3_000, ..Default::default() },
                MonthIncome { month: "2026-03".into(), dividends: 4_000, tax: -600, ..Default::default() },
                MonthIncome { month: "2026-06".into(), dividends: 4_500, tax: -675, ..Default::default() },
                MonthIncome { month: "2026-09".into(), interest: 800, fees: -1_000, ..Default::default() },
            ],
            commissions: Some(Commissions { from: "2025-11-18".into(), to: "2026-11-17".into(), amount: -2_350 }),
            synced_at: String::new(),
            missing: vec!["Transfers".into()],
            // What it held at the end of 2025 was kept; nothing in between was.
            snapshots: vec![
                investments::MonthEnd {
                    month: "2025-12".into(),
                    as_of: "2025-12-31".into(),
                    total: 4_100_000,
                    by_kind: [("Stocks".to_string(), 3_250_000), ("Bonds".to_string(), 500_000)].into(),
                    cash: 340_000,
                    other: 10_000,
                    cost: Some(2_900_000),
                    unrealised: Some(350_000),
                },
                investments::MonthEnd {
                    month: "2026-11".into(),
                    as_of: "2026-11-17".into(),
                    total: 5_300_000,
                    by_kind: [("Stocks".to_string(), 4_100_000), ("Bonds".to_string(), 800_000), ("Options".to_string(), -50_000)].into(),
                    cash: 437_500,
                    other: 12_500,
                    cost: Some(3_260_000),
                    unrealised: Some(790_000),
                },
            ],
            ..Default::default()
        };
        // Brought in by a build from before snapshots were kept: it has none.
        let trading = Holdings {
            account_id: "trading".into(),
            source: "ibkr".into(),
            source_account: "U2222222".into(),
            as_of: "2026-11-17".into(),
            currency: "USD".into(),
            rate: "1".into(),
            total: 1_250_000,
            positions: vec![
                finer("ETF", "", "7000.00", identified(world, position("VWRA", "World fund", "STK", "USD", 700_000, Some(650_000), Some(50_000)))),
                // Its statement gave no value in its own currency for this one.
                identified("US5949181045", position("MSFT", "Microsoft", "STK", "USD", 430_000, Some(400_000), Some(30_000))),
            ],
            cash: vec![CashHolding { currency: "USD".into(), amount: "1200".into(), value: 120_000 }],
            values: series(&[("2026-08-03", 1_000_000), ("2026-08-31", 1_030_000), ("2026-09-30", 1_010_000), ("2026-10-30", 1_060_000), ("2026-11-17", 1_250_000)]),
            flows: Some(series(&[("2026-08-03", 1_000_000), ("2026-11-03", 150_000)])),
            income: vec![MonthIncome { month: "2026-10".into(), dividends: 700, tax: -105, ..Default::default() }],
            commissions: Some(Commissions { from: "2026-08-03".into(), to: "2026-11-17".into(), amount: -420 }),
            // Its statements list its trades and name the holding each dividend was paid on;
            // Long term's do neither. One of the three has been sold since.
            trades: [("2026-08", 0, -120, 2), ("2026-09", -8_000, -60, 1), ("2026-10", 25_000, -150, 3), ("2026-11", 0, -90, 1)]
                .map(|(month, realised, commissions, count)| investments::MonthTrades { month: month.into(), realised, commissions, count })
                .to_vec(),
            income_by: [("IE00BK5BQT80", "VWRA", "World fund", 400, -60), ("US5949181045", "MSFT", "Microsoft", 200, -30), ("US67066G1040", "NVDA", "Nvidia", 100, -15)]
                .map(|(key, symbol, name, dividends, tax)| investments::HoldingIncome { month: "2026-10".into(), key: key.into(), symbol: symbol.into(), name: name.into(), dividends, tax })
                .to_vec(),
            ..Default::default()
        };
        let entries = b.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap();
        entries.extend([json!(long_term), json!(trading)]);
        for account in ["broker", "trading"] {
            investments::revalue(&mut b, account).unwrap();
        }
        // The moment each was brought in is the only thing that differs from one run to the next.
        let then = "2026-11-18T05:00:00.000Z";
        for entry in b.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap() {
            entry["syncedAt"] = json!(then);
        }
        for t in b.transactions.iter_mut().filter(|t| investments::generated(t)) {
            t.extra.insert("createdAt".into(), json!(then));
        }
        crate::domain::validate(&b).unwrap();
        b
    }
    fn range(from: &str, to: &str, accounts: &[&str]) -> reports::Filter {
        reports::Filter { from: from.into(), to: to.into(), account_ids: accounts.iter().map(|id| id.to_string()).collect(), category_ids: vec![] }
    }
    /// The filter the shared fixture's expected report is for: January to November 2026, every account.
    fn fixture_range() -> reports::Filter {
        range("2026-01", "2026-11", &[])
    }
    const INVESTED: [&str; 4] = ["broker", "pension", "trading", "euro"];
    /// The market changes written for `account` in `month`.
    fn changes(b: &Budget, account: &str, month: &str) -> i64 {
        b.transactions.iter().filter(|t| t.account_id == account && t.date.starts_with(month) && investments::market_change(t)).map(|t| t.amount).sum()
    }

    #[test]
    fn the_portfolios_months_are_the_accounts_balances_and_market_changes_as_net_worth_has_them() {
        let b = fixture();
        let report = portfolio(&b, &fixture_range()).unwrap();
        let ledger = reports::calculate(&b, &fixture_range()).unwrap();
        assert_eq!(report.months.len(), 11);
        let mut before = report.total.start;
        for (row, month) in report.months.iter().zip(&ledger.months) {
            assert_eq!(row.month, month.month);
            // Each account's figure is its balance, and the month's value their sum.
            for (account, value) in &row.by_account {
                assert_eq!(*value, month.accounts[account], "{account} in {}", row.month);
            }
            assert_eq!(row.value, row.by_account.values().sum::<i64>(), "{}", row.month);
            // The gain is the market changes of the accounts in it; the rest of the move is money put in.
            assert_eq!(row.gain, row.by_account.keys().map(|account| changes(&b, account, &row.month)).sum::<i64>(), "{}", row.month);
            assert_eq!(row.put, row.value - before - row.gain, "{}", row.month);
            before = row.value;
        }
        let rows: Vec<(&str, i64, i64, i64)> = report.months.iter().map(|m| (m.month.as_str(), m.value, m.gain, m.put)).collect();
        assert_eq!(rows, vec![
            ("2026-01", 6_180_000, 80_000, 0),
            // The euro account joins with 13,750.00, beside the 5,000.00 sent to Long term.
            ("2026-02", 8_125_000, 70_000, 1_875_000),
            ("2026-03", 8_075_000, -50_000, 0),
            // 2,000.00 went from Long term to Pension: nothing was put into the two together.
            ("2026-04", 8_135_000, 60_000, 0),
            ("2026-05", 8_325_000, 80_000, 110_000),
            ("2026-06", 8_335_000, 10_000, 0),
            ("2026-07", 8_455_000, 120_000, 0),
            // Trading joins with the 10,000.00 it opened with: money put in, and only what it made since is a gain.
            ("2026-08", 9_455_000, 0, 1_000_000),
            ("2026-09", 9_595_000, 40_000, 100_000),
            ("2026-10", 9_837_000, 242_000, 0),
            ("2026-11", 10_377_000, 370_000, 170_000),
        ]);
        assert_eq!(report.months[3].by_account["broker"] - report.months[2].by_account["broker"], -140_000);
        assert_eq!(report.months[3].by_account["pension"] - report.months[2].by_account["pension"], 200_000);
        // An account is listed from the month it joins.
        assert_eq!(report.months[0].by_account.keys().collect::<Vec<_>>(), ["broker", "pension"]);
        assert!(report.months[6].by_account.get("trading").is_none() && report.months[7].by_account["trading"] == 1_030_000);
        // The range as a whole, from the end of December.
        let total = &report.total;
        assert_eq!((total.start, total.end, total.gain, total.put), (6_100_000, 10_377_000, 1_022_000, 3_255_000));
        assert_eq!(total.end, ledger.months[10].accounts.iter().filter(|(id, _)| INVESTED.contains(&id.as_str())).map(|(_, n)| *n).sum::<i64>());
        assert_eq!(total.put, report.months.iter().map(|m| m.put).sum::<i64>());
        assert_eq!(total.gain, report.months.iter().map(|m| m.gain).sum::<i64>());
    }

    #[test]
    fn each_account_counts_from_the_day_it_joined_and_its_value_then_is_money_put_in() {
        let b = fixture();
        let report = portfolio(&b, &fixture_range()).unwrap();
        // In the budget's order, with what fills each.
        let rows: Vec<(&str, &str, &str, bool, i64, i64, i64, i64)> =
            report.accounts.iter().map(|a| (a.account_id.as_str(), a.source.as_str(), a.joined.as_str(), a.sparse, a.start, a.end, a.put, a.gain)).collect();
        assert_eq!(rows, vec![
            ("broker", "ibkr", "2025-11-28", false, 4_100_000, 5_300_000, 400_000, 800_000),
            ("pension", "manual", "2025-12-31", true, 2_000_000, 2_300_000, 200_000, 100_000),
            // Joined inside the range: they start from nothing, and what they were worth that day was put in.
            ("trading", "ibkr", "2026-08-03", false, 0, 1_250_000, 1_150_000, 100_000),
            ("euro", "file", "2026-02-27", true, 0, 1_527_000, 1_505_000, 22_000),
        ]);
        let (broker, pension, trading, euro) = (&report.accounts[0], &report.accounts[1], &report.accounts[2], &report.accounts[3]);
        assert_eq!((broker.estimated_days, broker.missing.as_slice(), broker.as_of.as_str(), broker.source_account.as_str()), (1, &["Transfers".to_string()][..], "2026-11-17", "U1111111"));
        // An account's own return is its page's over the same days.
        let page = page(&b, "trading").unwrap();
        let all = &page.periods.last().unwrap().did;
        assert_eq!((all.gain, all.put + all.start), (trading.gain, trading.put));
        assert!(close(trading.rate, all.rate.unwrap()));
        // 321 days from the end of December to Long term's last prices; 314 to Pension's last value.
        assert!(close(broker.rate, 800_000.0 / (4_100_000.0 + 500_000.0 * 280.0 / 321.0 - 200_000.0 * 216.0 / 321.0 + 100_000.0 * 70.0 / 321.0)));
        assert!(close(pension.rate, 100_000.0 / (2_000_000.0 + 200_000.0 * 209.0 / 314.0)));
        // From the day it joined to its last prices; the money sent after those is there for none of them.
        assert!(close(euro.rate, 22_000.0 / (1_375_000.0 + 110_000.0 * 178.0 / 245.0)));
        // Together: the two that join bring their first value in on the day they join, and the
        // 2,000.00 between Long term and Pension leaves one as it enters the other.
        let at_work = 6_100_000.0 + 500_000.0 * 280.0 / 321.0 + 100_000.0 * 70.0 / 321.0
            + 1_375_000.0 * 263.0 / 321.0 + 110_000.0 * 196.0 / 321.0 + 20_000.0 * 12.0 / 321.0
            + 1_000_000.0 * 106.0 / 321.0 + 150_000.0 * 14.0 / 321.0;
        assert!(close(report.total.rate, 1_022_000.0 / at_work), "{:?}", report.total.rate);
        // Under a year: no yearly rate.
        assert_eq!((report.total.days, report.total.yearly), (321, None));
        // One account alone: the transfer is money that left it.
        let alone = portfolio(&b, &range("2026-01", "2026-11", &["broker"])).unwrap();
        assert_eq!((alone.accounts.len(), alone.total.put, alone.months[3].put, alone.total.rate), (1, 400_000, -200_000, broker.rate));
        // A range that ends before an account joins has it at nothing, and one after its prices at what the ledger says.
        let early = portfolio(&b, &range("2026-01", "2026-03", &[])).unwrap();
        assert_eq!(early.accounts.iter().map(|a| (a.end, a.rate.is_some(), a.end_parts.is_some())).collect::<Vec<_>>()[2], (0, false, false));
        assert_eq!(early.total.end, 8_075_000);
        // Over more than a year, the return is also given as a yearly rate.
        let mut slow = crate::test_support::sample();
        slow.accounts.push(serde_json::from_value(json!({"id": "fund", "name": "Fund", "kind": "tracking", "openingBalance": 0, "archived": false})).unwrap());
        for (day, value) in [("2024-12-31", "10000"), ("2026-12-31", "12100")] {
            slow = investments::record(slow, "fund", investments::Snapshot { source: "manual".into(), as_of: day.into(), total: value.into(), ..Default::default() }).unwrap();
        }
        let two_years = portfolio(&slow, &range("2025-01", "2026-12", &[])).unwrap().total;
        assert_eq!((two_years.start, two_years.gain, two_years.put, two_years.days), (1_000_000, 210_000, 0, 730));
        assert!(close(two_years.rate, 0.21) && close(two_years.yearly, 0.1));
    }

    #[test]
    fn what_a_holding_is_is_named_from_its_kind_finer_category_and_side() {
        for (code, sub, side, named) in [
            ("STK", "ETF", "", "ETF"),
            ("STK", " etf ", "", "ETF"),
            ("STK", "ETN", "", "Exchange-traded note"),
            ("STK", "Reit", "", "REIT"),
            ("STK", "ADR", "", "Depositary receipt"),
            ("STK", "gdr", "", "Depositary receipt"),
            ("STK", "PREFERRED", "", "Preferred share"),
            ("STK", "RIGHT", "", "Right"),
            ("STK", "Closed-End Fund", "", "Closed-end fund"),
            ("STK", "COMMON", "", "Stock"),
            ("STK", "", "", "Stock"),
            ("BILL", "", "", "Treasury bill"),
            ("BOND", "US-T GOVT", "", "Government bond"),
            ("BOND", "corp", "", "Corporate bond"),
            ("BOND", "MUNI", "", "Bond"),
            ("BOND", "", "", "Bond"),
            ("OPT", "", "C", "Call option"),
            ("OPT", "", " p ", "Put option"),
            ("FOP", "", "P", "Put option"),
            ("WAR", "", "c", "Call option"),
            ("OPT", "", "", "Option"),
            ("FUND", "ETF", "", "Fund"),
            ("FUT", "", "", "Future"),
            ("CASH", "", "", "Currency"),
            ("CRYPTO", "", "", "Crypto"),
            ("CMDTY", "", "", "Commodity"),
            ("CFD", "", "", "CFD"),
            ("WHAT", "", "", "Other"),
            ("", "ETF", "", ""),
        ] {
            assert_eq!(instrument(code, sub, side), named, "{code} {sub:?} {side:?}");
        }
    }

    #[test]
    fn own_currency_amounts_have_their_currencys_usual_decimals() {
        assert_eq!(in_minor_units("17098.9999", "USD"), "17099.00");
        assert_eq!(in_minor_units("4000", "EUR"), "4000.00");
        assert_eq!(in_minor_units("-500.005", "USD"), "-500.01");
        assert_eq!(in_minor_units("1.25", "KWD"), "1.250");
        assert_eq!(in_minor_units("150000.4", "JPY"), "150000");
        assert_eq!(in_minor_units("n/a", "USD"), "n/a");
    }
    #[test]
    fn accrued_income_counts_in_the_accounts_own_currency() {
        // A dollar account at the broker in a riyal budget, with income accrued in dollars.
        let mut b = crate::test_support::sample();
        b.currency = "SAR".into();
        b.accounts.push(serde_json::from_value(json!({"id": "broker", "name": "Broker", "kind": "tracking", "openingBalance": 0, "archived": false})).unwrap());
        let statement = crate::flex::parse(crate::test_support::FLEX_SAMPLE).unwrap().accounts.remove(0);
        let b = investments::apply(b, "broker", &statement, None).unwrap();
        let held = investments::find(&b, "broker").unwrap();
        assert_eq!((held.currency.as_str(), held.rate.as_str(), held.other), ("USD", "3.74999995", 4_687));
        let report = portfolio(&b, &range("2026-11", "2026-11", &[])).unwrap();
        let amounts: Vec<(&str, Option<Option<String>>)> = report.allocation.by_currency.iter().map(|p| (p.key.as_str(), p.amount.clone())).collect();
        // Dollars: the holdings' 5,800 and 17,100, the 820 in cash and the 12.50 accrued.
        // Riyals: the Saudi stock's 10,800 and 1,500 in cash.
        assert_eq!(amounts, vec![("USD", Some(Some("23732.50".to_string()))), ("SAR", Some(Some("12300.00".to_string())))]);
        let read = serde_json::to_value(&report).unwrap();
        assert_eq!(read["allocation"]["byCurrency"][0]["amount"], "23732.50");
        // Without a rate that reads, what is accrued can't be put in dollars.
        let mut unrated = b.clone();
        unrated.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap()[0]["rate"] = json!("");
        let report = portfolio(&unrated, &range("2026-11", "2026-11", &[])).unwrap();
        assert_eq!(report.allocation.by_currency.iter().find(|p| p.key == "USD").map(|p| p.amount.clone()), Some(Some(None)));
        assert_eq!(accrued_in(&held, 4_687).as_deref(), Some("12.50"));
        assert_eq!(accrued_in(&Holdings { rate: "0".into(), ..held.clone() }, 4_687), None);
    }

    #[test]
    fn each_holding_says_what_it_is_and_what_it_is_worth_in_its_own_currency() {
        let b = fixture();
        let report = portfolio(&b, &fixture_range()).unwrap();
        let read = serde_json::to_value(&report).unwrap();
        // Merged: the world fund's amounts in its two accounts, added up as decimals.
        let rows: Vec<(&str, &str, Option<&str>)> = report.holdings.iter().map(|h| (h.symbol.as_str(), h.instrument, h.amount.as_deref())).collect();
        assert_eq!(rows, vec![
            ("VWRA", "ETF", Some("39000.00")),
            ("EUNL", "Fund", Some("10400.00")),
            ("AAPL", "Stock", Some("9000.00")),
            ("T 4 02/15/34", "Government bond", Some("8000.00")),
            // No amount from its statement.
            ("MSFT", "Stock", None),
            ("BUND", "Bond", Some("3050.00")),
            ("SPY 260116P00500000", "Put option", Some("-500.00")),
        ]);
        assert_eq!(read["holdings"][0]["amount"], "39000.00");
        assert!(read["holdings"][4]["amount"].is_null());
        // By currency: holdings and cash in that currency, in it. In the budget's own currency a
        // value nothing else gives an amount for (a typed value, accrued income) is its amount.
        let by_currency: Vec<(&str, i64, Option<&str>)> = report.allocation.by_currency.iter().map(|p| (p.key.as_str(), p.value, p.amount.clone().flatten().as_deref().map(|_| ""))).collect();
        assert_eq!(by_currency.iter().map(|(key, value, _)| (*key, *value)).collect::<Vec<_>>(), vec![("USD", 8_812_500), ("EUR", 1_507_000), ("SAR", 37_500)]);
        assert_eq!(report.allocation.by_currency.iter().map(|p| p.amount.clone()).collect::<Vec<_>>(), vec![Some(Some("88125.00".to_string())), Some(Some("13700.00".to_string())), Some(Some("1406.25".to_string()))]);
        // Only on by currency.
        assert!(read["allocation"]["byKind"][0].get("amount").is_none() && read["allocation"]["byAccount"][0].get("amount").is_none());
        assert_eq!(read["allocation"]["byCurrency"][2], json!({"key": "SAR", "value": 37_500, "share": 37_500.0 / 10_357_000.0, "amount": "1406.25"}));
        // Each account's cash by currency.
        assert_eq!(report.accounts[0].cash.iter().map(|c| (c.currency.as_str(), c.amount.as_str(), c.value)).collect::<Vec<_>>(), vec![("USD", "4000.00", 400_000), ("SAR", "1406.25", 37_500)]);
        assert_eq!((report.accounts[1].cash.len(), report.accounts[3].cash[0].amount.as_str(), report.accounts[3].cash[0].currency.as_str()), (0, "250.00", "EUR"));
        assert_eq!(read["accounts"][0]["cash"][1], json!({"currency": "SAR", "amount": "1406.25", "value": 37_500}));

        // One account's amount unknown, or in another currency: the merged amount isn't known.
        let mut b2 = b.clone();
        let entries = b2.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap();
        let trading = entries.iter_mut().find(|e| e["accountId"] == "trading").unwrap();
        trading["positions"][0].as_object_mut().unwrap().remove("amount");
        let unknown = portfolio(&b2, &fixture_range()).unwrap();
        assert_eq!((unknown.holdings[0].symbol.as_str(), unknown.holdings[0].amount.as_deref(), unknown.holdings[0].value), ("VWRA", None, 3_900_000));
        let mut b3 = b.clone();
        let entries = b3.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap();
        let trading = entries.iter_mut().find(|e| e["accountId"] == "trading").unwrap();
        trading["positions"][0]["currency"] = json!("GBP");
        trading["positions"][0]["amount"] = json!("5500.00");
        let mixed = portfolio(&b3, &fixture_range()).unwrap();
        assert_eq!((mixed.holdings[0].currency.as_str(), mixed.holdings[0].amount.as_deref()), ("USD", None));
        // A currency with something of unknown amount in it has none: here pounds with a holding
        // that gave none, and accrued income at a broker whose currency isn't the budget's and
        // whose rate doesn't read.
        let entries = b3.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap();
        entries.iter_mut().find(|e| e["accountId"] == "trading").unwrap()["positions"][0].as_object_mut().unwrap().remove("amount");
        let gbp = portfolio(&b3, &fixture_range()).unwrap();
        let part = gbp.allocation.by_currency.iter().find(|p| p.key == "GBP").unwrap();
        assert_eq!((part.value, part.amount.clone()), (700_000, Some(None)));
        let mut b4 = b.clone();
        let entries = b4.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap();
        let broker = entries.iter_mut().find(|e| e["accountId"] == "broker").unwrap();
        broker["currency"] = json!("SAR");
        // With its rate, the 125.00 accrued is in riyals beside the 1,406.25 in cash.
        let sar = portfolio(&b4, &fixture_range()).unwrap();
        assert_eq!(sar.allocation.by_currency.iter().find(|p| p.key == "SAR").map(|p| p.amount.clone()), Some(Some(Some("1531.25".to_string()))));
        let entries = b4.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap();
        entries.iter_mut().find(|e| e["accountId"] == "broker").unwrap()["rate"] = json!("n/a");
        let sar = portfolio(&b4, &fixture_range()).unwrap();
        assert_eq!(sar.allocation.by_currency.iter().find(|p| p.key == "SAR").map(|p| p.amount.clone()), Some(Some(None)));

        // One account's page: each row says what it is and what it is worth in its currency.
        let page = page(&b, "broker").unwrap();
        assert_eq!(page.holdings.iter().map(|r| (r.instrument, r.amount.as_deref())).collect::<Vec<_>>(), vec![
            ("ETF", Some("32000.00")), ("Stock", Some("9000.00")), ("Government bond", Some("8000.00")), ("Put option", Some("-500.00")),
        ]);
        let row = serde_json::to_value(&page.holdings[0]).unwrap();
        assert_eq!((row["instrument"].as_str(), row["amount"].as_str(), row["one"].as_str()), (Some("ETF"), Some("32000.00"), Some("Stock")));
        // An entry stored before these were kept reads, and says what it is from its kind alone.
        let mut old = crate::test_support::sample();
        let mut entry = serde_json::to_value(held()).unwrap();
        for p in entry["positions"].as_array_mut().unwrap() {
            assert!(p.get("subKind").is_none() && p.get("putCall").is_none() && p.get("amount").is_none());
        }
        entry["positions"][2]["kind"] = json!("BOND");
        old.extra.insert("investments".into(), json!([entry.clone()]));
        let stored: Holdings = serde_json::from_value(entry.clone()).unwrap();
        assert_eq!(serde_json::to_value(&stored).unwrap(), entry);
        let old_page = crate::commands::read(&old, "investmentPage", &json!({"accountId": "broker"})).unwrap();
        assert_eq!((old_page["holdings"][0]["instrument"].as_str(), old_page["holdings"][2]["instrument"].as_str()), (Some("Stock"), Some("Bond")));
        assert!(old_page["holdings"].as_array().unwrap().iter().all(|r| r["amount"].is_null()));
    }

    #[test]
    fn what_the_accounts_hold_is_added_up_and_the_same_holding_in_two_is_one() {
        let b = fixture();
        let report = portfolio(&b, &fixture_range()).unwrap();
        let held = &report.allocation;
        let pairs = |parts: &[Part]| parts.iter().map(|p| (p.key.as_str().to_owned(), p.value)).collect::<Vec<_>>();
        let named = |rows: &[(&str, i64)]| rows.iter().map(|(key, value)| (key.to_string(), *value)).collect::<Vec<_>>();
        // A value nothing breaks down (Pension's) is holdings of no known kind.
        assert_eq!(pairs(&held.by_kind), named(&[("Stocks", 5_230_000), ("Holdings", 2_300_000), ("Funds", 1_144_000), ("Bonds", 1_135_500), ("Cash", 585_000), ("Accrued", 12_500), ("Options", -50_000)]));
        assert_eq!(pairs(&held.by_account), named(&[("broker", 5_300_000), ("pension", 2_300_000), ("euro", 1_507_000), ("trading", 1_250_000)]));
        assert_eq!(pairs(&held.by_currency), named(&[("USD", 8_812_500), ("EUR", 1_507_000), ("SAR", 37_500)]));
        // Every list adds up to the accounts' values, and its shares to the whole of its positive parts.
        for parts in [&held.by_kind, &held.by_account, &held.by_currency] {
            assert_eq!(parts.iter().map(|p| p.value).sum::<i64>(), 10_357_000);
            assert!(close(Some(parts.iter().filter(|p| p.value > 0).map(|p| p.share).sum()), 1.0));
        }
        // The sold option isn't a share of anything.
        assert_eq!((held.total, held.by_kind.last().unwrap().share), (10_407_000, 0.0));
        assert_eq!((held.count, held.cash, held.accrued, held.cost, held.unrealised), (7, 585_000, 12_500, Some(5_300_000), Some(1_024_000)));
        assert!(close(held.unrealised_rate, 1_024_000.0 / 5_300_000.0));
        // Dated by the oldest prices among them.
        assert_eq!(held.as_of, "2026-10-30");
        let rows: Vec<(&str, &str, i64, Vec<&str>)> = report.holdings.iter().map(|h| (h.key.as_str(), h.symbol.as_str(), h.value, h.accounts.iter().map(String::as_str).collect())).collect();
        assert_eq!(rows, vec![
            // The world fund is in both broker accounts, and one row by its ISIN.
            ("IE00BK5BQT80", "VWRA", 3_900_000, vec!["broker", "trading"]),
            // A file gives no ISIN: its symbol and currency stand in.
            ("EUNL EUR", "EUNL", 1_144_000, vec!["euro"]),
            ("US0378331005", "AAPL", 900_000, vec!["broker"]),
            ("US91282CJZ59", "T 4 02/15/34", 800_000, vec!["broker"]),
            ("US5949181045", "MSFT", 430_000, vec!["trading"]),
            ("BUND EUR", "BUND", 335_500, vec!["euro"]),
            ("700123456", "SPY 260116P00500000", -50_000, vec!["broker"]),
        ]);
        let fund = &report.holdings[0];
        assert_eq!((fund.name.as_str(), fund.group, fund.one, fund.currency.as_str(), fund.cost, fund.gain), ("World fund", "Stocks", "Stock", "USD", Some(3_350_000), Some(550_000)));
        assert!(close(fund.rate, 550_000.0 / 3_350_000.0) && close(Some(fund.share), 3_900_000.0 / 10_407_000.0));
        assert_eq!((report.holdings[3].cost, report.holdings[3].rate, report.holdings[6].share), (None, None, 0.0));
        assert!(close(held.largest_share, 3_900_000.0 / 10_407_000.0) && close(held.top_five_share, 7_174_000.0 / 10_407_000.0));
        // Today's, whatever the range.
        let earlier = portfolio(&b, &range("2026-01", "2026-03", &[])).unwrap();
        assert_eq!((&earlier.allocation, &earlier.holdings), (held, &report.holdings));
        // Only the accounts asked for.
        let one = portfolio(&b, &range("2026-01", "2026-11", &["trading"])).unwrap();
        assert_eq!((one.allocation.total, one.holdings.len(), one.holdings[0].accounts.len()), (1_250_000, 2, 1));
    }

    #[test]
    fn a_balance_is_split_by_what_it_was_in_only_where_that_was_kept() {
        let b = fixture();
        let report = portfolio(&b, &fixture_range()).unwrap();
        let ledger = reports::calculate(&b, &fixture_range()).unwrap();
        let sum = |parts: &Parts| parts.by_kind.iter().map(|k| k.value).sum::<i64>() + parts.cash + parts.accrued + parts.rest;
        // Whatever a split is from, it adds up to the account's balance at that month's end.
        for account in &report.accounts {
            if let Some(parts) = &account.start_parts {
                assert_eq!((parts.month.as_str(), sum(parts)), ("2025-12", ledger.opening_accounts[&account.account_id]), "{}", account.account_id);
            }
            let parts = account.end_parts.as_ref().unwrap();
            assert_eq!((parts.month.as_str(), sum(parts)), ("2026-11", ledger.months[10].accounts[&account.account_id]), "{}", account.account_id);
        }
        let kinds = |parts: &Parts| parts.by_kind.iter().map(|k| (k.key.as_str().to_owned(), k.value)).collect::<Vec<_>>();
        let (broker, pension, trading, euro) = (&report.accounts[0], &report.accounts[1], &report.accounts[2], &report.accounts[3]);
        // Long term has the snapshot kept at the end of December, and November's.
        let start = broker.start_parts.as_ref().unwrap();
        assert_eq!((start.as_of.as_str(), kinds(start), start.cash, start.accrued, start.rest), ("2025-12-31", vec![("Stocks".to_string(), 3_250_000), ("Bonds".into(), 500_000)], 340_000, 10_000, 0));
        let end = broker.end_parts.as_ref().unwrap();
        assert_eq!((end.as_of.as_str(), kinds(end), end.cash, end.accrued, end.rest), ("2026-11-17", vec![("Stocks".to_string(), 4_100_000), ("Bonds".into(), 800_000), ("Options".into(), -50_000)], 437_500, 12_500, 0));
        // A typed value breaks nothing down: all of it is the rest.
        let typed = pension.start_parts.as_ref().unwrap();
        assert_eq!((kinds(typed), typed.cash, typed.rest), (vec![], 0, 2_000_000));
        // An account that wasn't there at the start has no start to split.
        assert_eq!((&trading.start_parts, &euro.start_parts), (&None, &None));
        // No snapshot for November: what Trading holds now, from the prices in hand.
        let now = trading.end_parts.as_ref().unwrap();
        assert_eq!((now.as_of.as_str(), kinds(now), now.cash, now.rest), ("2026-11-17", vec![("Stocks".to_string(), 1_130_000)], 120_000, 0));
        // The euro account's prices are October's: what the balance moved by since is the rest.
        let since = euro.end_parts.as_ref().unwrap();
        assert_eq!((since.as_of.as_str(), kinds(since), since.cash, since.rest), ("2026-10-30", vec![("Funds".to_string(), 1_144_000), ("Bonds".into(), 335_500)], 27_500, 20_000));
        // Without the snapshot, what Long term held in December isn't known, and a range ending
        // in a month before the prices with none can't be split either.
        let mut bare = b.clone();
        let entry = bare.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap().iter_mut().find(|e| e["accountId"] == "broker").unwrap();
        entry.as_object_mut().unwrap().remove("snapshots");
        let unknown = portfolio(&bare, &fixture_range()).unwrap();
        assert_eq!((&unknown.accounts[0].start_parts, unknown.accounts[0].end_parts.is_some(), unknown.accounts[0].start), (&None, true, 4_100_000));
        let past = portfolio(&b, &range("2026-01", "2026-07", &[])).unwrap();
        assert_eq!((past.accounts[0].start_parts.is_some(), &past.accounts[0].end_parts, &past.accounts[3].end_parts), (true, &None, &None));
        // A month a snapshot was kept for is split from it, though the prices have moved on.
        let june = portfolio(&b, &range("2026-01", "2026-06", &[])).unwrap();
        assert_eq!(june.accounts[1].end_parts.as_ref().map(|p| (p.as_of.as_str(), p.rest)), Some(("2026-06-30", 2_150_000)));
        let february = portfolio(&b, &range("2026-02", "2026-02", &[])).unwrap();
        assert_eq!(february.accounts[3].end_parts.as_ref().map(|p| (p.as_of.as_str(), kinds(p), p.cash, p.rest)), Some(("2026-02-27", vec![("Funds".to_string(), 990_000), ("Bonds".into(), 330_000)], 55_000, 0)));
    }

    #[test]
    fn income_is_the_months_of_the_range_from_the_accounts_that_report_it() {
        let b = fixture();
        let report = portfolio(&b, &fixture_range()).unwrap();
        let income = &report.income;
        // December 2025's dividend is before the range.
        assert_eq!((income.dividends, income.interest, income.tax, income.fees, income.other), (9_200, 800, -1_380, -1_000, 0));
        assert_eq!(income.months.len(), 11);
        assert_eq!(income.months.iter().filter(|m| m.dividends != 0 || m.interest != 0).map(|m| (m.month.as_str(), m.dividends, m.interest, m.tax, m.fees)).collect::<Vec<_>>(), vec![
            ("2026-03", 4_000, 0, -600, 0), ("2026-06", 4_500, 0, -675, 0), ("2026-09", 0, 800, 0, -1_000), ("2026-10", 700, 0, -105, 0),
        ]);
        assert_eq!((income.reporting.as_slice(), income.silent.as_slice()), (&["broker".to_string(), "trading".into()][..], &["pension".to_string(), "euro".into()][..]));
        // A statement's commissions as one figure, for the account whose trades aren't known by month.
        assert_eq!(income.commissions.iter().map(|c| (c.account_id.as_str(), c.amount)).collect::<Vec<_>>(), vec![("broker", -2_350)]);
        // The same by account: a row for each that reports, in that order, adding up to the totals.
        assert_eq!(income.by_account.iter().map(|a| (a.account_id.as_str(), a.dividends, a.interest, a.tax, a.fees, a.other)).collect::<Vec<_>>(), vec![
            ("broker", 8_500, 800, -1_275, -1_000, 0), ("trading", 700, 0, -105, 0, 0),
        ]);
        // With each one's commissions: Long term's statement gives one figure, Trading's are its months'.
        assert_eq!(income.by_account.iter().map(|a| a.commissions).collect::<Vec<_>>(), [Some(-2_350), Some(-420)]);
        assert_eq!(income.by_account.iter().map(|a| a.account_id.clone()).collect::<Vec<_>>(), income.reporting);
        for narrowed in [fixture_range(), range("2026-04", "2026-09", &[]), range("2025-06", "2026-12", &["broker"]), range("2026-01", "2026-02", &[])] {
            let income = portfolio(&b, &narrowed).unwrap().income;
            let sum = |of: fn(&AccountIncome) -> i64| income.by_account.iter().map(of).sum::<i64>();
            assert_eq!((sum(|a| a.dividends), sum(|a| a.interest), sum(|a| a.tax), sum(|a| a.fees), sum(|a| a.other)), (income.dividends, income.interest, income.tax, income.fees, income.other));
            assert_eq!(income.by_account.len(), income.reporting.len());
            // Their commissions are the months' own and the statements' single figures together.
            let by_month: i64 = income.months.iter().filter_map(|m| m.commissions).sum();
            let listed: i64 = income.commissions.iter().map(|c| c.amount).sum();
            assert_eq!(income.by_account.iter().filter_map(|a| a.commissions).sum::<i64>(), by_month + listed);
        }
        // Against the two broker accounts' average value at the months' ends.
        let average = (4_180_000 + 4_750_000 + 4_700_000 + 4_560_000 + 4_640_000 + 4_700_000 + 4_820_000 + 5_820_000 + 5_960_000 + 6_180_000 + 6_550_000) as f64 / 11.0;
        assert!(close(income.yielded, 10_000.0 / average));
        // An account typed as an investment that nothing fills is named, and no part of the sums.
        assert_eq!(report.unfilled, vec!["old".to_string()]);
        // Accounts that report none have no yield to give.
        assert_eq!(portfolio(&b, &range("2026-01", "2026-11", &["pension", "euro"])).unwrap().income.yielded, None);
        // A budget without holdings has an empty report.
        let none = portfolio(&crate::test_support::sample(), &fixture_range()).unwrap();
        assert_eq!((none.accounts.len(), none.months.len(), none.total.end, none.total.rate, none.allocation.total, none.holdings.len()), (0, 11, 0, None, 0, 0));
        assert!(portfolio(&b, &range("2026-13", "2026-11", &[])).is_err() && portfolio(&b, &range("2026-11", "2026-01", &[])).is_err());
    }

    #[test]
    fn what_trades_realised_and_cost_and_dividends_by_holding_are_given_where_statements_list_them() {
        let b = fixture();
        let report = portfolio(&b, &fixture_range()).unwrap();
        // Only Trading's statements list its trades: the others have no figure, and the whole has Trading's.
        assert_eq!(report.accounts.iter().map(|a| a.realised).collect::<Vec<_>>(), vec![None, None, Some(17_000), None]);
        assert_eq!(report.total.realised, Some(17_000));
        // By month from the first month a statement listed them for; a month with trades that closed nothing is 0.
        let known = [None, None, None, None, None, None, None, Some(0), Some(-8_000), Some(25_000), Some(0)];
        assert_eq!(report.months.iter().map(|m| m.realised).collect::<Vec<_>>(), known);
        assert_eq!(report.income.months.iter().map(|m| m.commissions).collect::<Vec<_>>(), [None, None, None, None, None, None, None, Some(-120), Some(-60), Some(-150), Some(-90)]);
        // Realised gains are a part of the gain, which is as it was.
        assert_eq!((report.total.gain, report.months[9].gain), (1_022_000, 242_000));
        // Dividends by holding, largest first, with today's value across the accounts where
        // it is still held and the dividends as a share of it.
        let rows: Vec<(&str, &str, i64, i64, Option<i64>)> = report.income.by_holding.iter().map(|h| (h.key.as_str(), h.name.as_str(), h.dividends, h.tax, h.value)).collect();
        assert_eq!(rows, vec![
            ("IE00BK5BQT80", "World fund", 400, -60, Some(3_900_000)),
            ("US5949181045", "Microsoft", 200, -30, Some(430_000)),
            ("US67066G1040", "Nvidia", 100, -15, None),
        ]);
        assert!(close(report.income.by_holding[0].yielded, 400.0 / 3_900_000.0) && report.income.by_holding[2].yielded.is_none());
        // A holding's key is the one it has among the holdings.
        assert_eq!(report.holdings[0].key, report.income.by_holding[0].key);
        // A range outside the months listed knows nothing of trades, and has no dividends to list.
        let spring = portfolio(&b, &range("2026-01", "2026-06", &[])).unwrap();
        assert_eq!((spring.total.realised, spring.months.iter().all(|m| m.realised.is_none()), spring.income.months.iter().all(|m| m.commissions.is_none()), spring.income.by_holding.len()), (None, true, true, 0));
        // Trading's trades are still known by month, so its statement's one figure stays out
        // and it has no commissions for months before the first one listed.
        assert_eq!(spring.income.commissions.iter().map(|c| c.account_id.as_str()).collect::<Vec<_>>(), ["broker"]);
        assert_eq!(spring.income.by_account.iter().map(|a| a.commissions).collect::<Vec<_>>(), [Some(-2_350), None]);
        // A statement whose period is outside the range gives no figure for it.
        assert_eq!(portfolio(&b, &range("2025-01", "2025-10", &[])).unwrap().income.by_account.iter().map(|a| a.commissions).collect::<Vec<_>>(), [None, None]);
        // Accounts whose statements list no trades: nothing is known, and each statement's commissions are listed.
        let others = portfolio(&b, &range("2026-01", "2026-11", &["broker", "pension", "euro"])).unwrap();
        assert_eq!((others.total.realised, others.accounts.iter().all(|a| a.realised.is_none()), others.months.iter().all(|m| m.realised.is_none())), (None, true, true));
        assert!(others.income.months.iter().all(|m| m.commissions.is_none()) && others.income.by_holding.is_empty() && others.income.commissions.len() == 1);
        // On the wire: null where unknown.
        let read = serde_json::to_value(&report).unwrap();
        assert_eq!((read["accounts"][0]["realised"].is_null(), read["months"][0]["realised"].is_null(), read["months"][9]["realised"].as_i64(), read["total"]["realised"].as_i64()), (true, true, Some(25_000), Some(17_000)));
        assert_eq!((read["income"]["months"][0]["commissions"].is_null(), read["income"]["months"][9]["commissions"].as_i64()), (true, Some(-150)));
        assert_eq!((read["income"]["byHolding"][2]["value"].is_null(), read["income"]["byHolding"][2]["yield"].is_null(), read["income"]["byHolding"][0]["yield"].is_f64()), (true, true, true));
        // From a statement itself: the sample with trades, read and brought in.
        let mut own = crate::test_support::sample();
        own.currency = "USD".into();
        own.accounts.push(serde_json::from_value(json!({"id": "broker", "name": "Broker", "kind": "tracking", "openingBalance": 0, "archived": false})).unwrap());
        let traded = crate::flex::parse(&crate::flex::tests::with_trades()).unwrap().accounts.remove(0);
        let own = investments::apply(own, "broker", &traded, None).unwrap();
        let autumn = portfolio(&own, &range("2026-08", "2026-11", &[])).unwrap();
        assert_eq!(autumn.months.iter().map(|m| m.realised).collect::<Vec<_>>(), [None, Some(0), Some(49_875), Some(0)]);
        assert_eq!((autumn.total.realised, autumn.income.months[2].commissions, autumn.income.months[3].commissions, autumn.income.commissions.len()), (Some(49_875), Some(-250), Some(-200), 0));
        assert_eq!(autumn.income.by_holding.iter().map(|h| (h.symbol.as_str(), h.dividends, h.tax, h.value)).collect::<Vec<_>>(), vec![
            ("2222", 2_667, -133, Some(288_000)), ("AAPL", 650, -98, Some(580_000)), ("MSFT", 300, 0, None),
        ]);
        // The sample as it is lists no trades: nothing is known of them, and its commissions are the statement's.
        let mut plain = crate::test_support::sample();
        plain.currency = "USD".into();
        plain.accounts.push(serde_json::from_value(json!({"id": "broker", "name": "Broker", "kind": "tracking", "openingBalance": 0, "archived": false})).unwrap());
        let statement = crate::flex::parse(crate::test_support::FLEX_SAMPLE).unwrap().accounts.remove(0);
        let quiet = portfolio(&investments::apply(plain, "broker", &statement, None).unwrap(), &range("2026-08", "2026-11", &[])).unwrap();
        assert_eq!((quiet.total.realised, quiet.accounts[0].realised, quiet.income.commissions.len()), (None, None, 1));
        assert!(quiet.months.iter().all(|m| m.realised.is_none()) && quiet.income.months.iter().all(|m| m.commissions.is_none()));
    }

    #[test]
    fn net_worths_change_is_what_markets_did_and_everything_else() {
        let b = fixture();
        let report = reports::calculate(&b, &fixture_range()).unwrap();
        let mut before: i64 = report.opening_accounts.values().sum();
        for month in &report.months {
            // The market's part is the accounts' market changes; the rest is every other line
            // that isn't money moving between two accounts.
            assert_eq!(month.market, INVESTED.iter().map(|account| changes(&b, account, &month.month)).sum::<i64>(), "{}", month.month);
            let rest: i64 = b.transactions.iter().filter(|t| t.date.starts_with(&month.month) && t.transfer_account_id.is_none() && !investments::market_change(t)).map(|t| t.amount).sum();
            assert_eq!(month.net_worth - before, month.market + rest, "{}", month.month);
            before = month.net_worth;
        }
        let of = |report: &reports::Report, month: &str| report.months.iter().find(|m| m.month == month).map(|m| m.market);
        // October: the three accounts valued that month gained 2,420.00 and 300.00 went on groceries.
        assert_eq!((of(&report, "2026-10"), report.months[9].net_worth - report.months[8].net_worth), (Some(242_000), 212_000));
        // The euro account joining in February is not the market's doing.
        assert_eq!((of(&report, "2026-02"), report.months[1].net_worth - report.months[0].net_worth), (Some(70_000), 1_445_000));
        // It matches the investments report's gain month by month.
        let invested = portfolio(&b, &fixture_range()).unwrap();
        assert_eq!(report.months.iter().map(|m| m.market).collect::<Vec<_>>(), invested.months.iter().map(|m| m.gain).collect::<Vec<_>>());
        // Only the accounts the filter includes, and only ones that count in net worth.
        assert_eq!(of(&reports::calculate(&b, &range("2026-01", "2026-11", &["checking", "trading"])).unwrap(), "2026-10"), Some(50_000));
        let mut left_out = b.clone();
        left_out.accounts[1].extra.insert("netWorth".into(), json!(false));
        assert_eq!(of(&reports::calculate(&left_out, &fixture_range()).unwrap(), "2026-10"), Some(72_000));
        // A budget without investments has no market figure in any month.
        let mut plain = crate::test_support::sample();
        plain.transactions.push(crate::test_support::purchase());
        assert!(reports::calculate(&plain, &range("2026-10", "2026-10", &[])).unwrap().months.iter().all(|m| m.market == 0));
    }

    #[test]
    fn the_report_is_read_as_a_command_and_exported_with_its_holdings_and_income() {
        let b = fixture();
        let params = json!({"from": "2026-01", "to": "2026-11", "accountIds": []});
        let read = crate::commands::read(&b, "investmentsReport", &params).unwrap();
        assert_eq!(read, serde_json::to_value(portfolio(&b, &fixture_range()).unwrap()).unwrap());
        assert_eq!((read["total"]["put"].as_i64(), read["months"][1]["byAccount"]["euro"].as_i64(), read["accounts"][2]["startParts"].is_null()), (Some(3_255_000), Some(1_375_000), true));
        assert_eq!((read["accounts"][0]["endParts"]["byKind"][0]["key"].as_str(), read["accounts"][0]["estimatedDays"].as_i64(), read["income"]["yield"].is_f64()), (Some("Stocks"), Some(1), true));
        assert_eq!((read["allocation"]["topFiveShare"].is_f64(), read["holdings"][0]["one"].as_str(), read["total"]["yearly"].is_null()), (true, Some("Stock"), true));
        assert!(crate::commands::read(&b, "investmentsReport", &json!({"from": "2026-01"})).is_err());
        let file = crate::commands::read(&b, "investmentsCsv", &params).unwrap();
        let lines: Vec<&str> = file.as_str().unwrap().lines().collect();
        assert_eq!(lines[0], "Month,Value,Money put in,Gain,Realised gain,Long term,Pension,Trading,Euro funds");
        // An account's column is empty until it joins, and what trades realised where no statement listed them.
        assert_eq!((lines[1], lines[11]), ("2026-01,61800.00,0.00,800.00,,41800.00,20000.00,,", "2026-11,103770.00,1700.00,3700.00,0.00,53000.00,23000.00,12500.00,15270.00"));
        assert_eq!(&lines[12..15], [",,,", "Holding,Symbol,Kind,Currency,Value,Cost,Unrealised gain,Share,Accounts", "World fund,VWRA,Stocks,USD,39000.00,33500.00,5500.00,37.5%,\"Long term, Trading\""]);
        assert_eq!(&lines[21..24], [",,,", "Month,Dividends,Interest,Tax,Fees,Other,Commissions", "2026-01,0.00,0.00,0.00,0.00,0.00,"]);
        assert_eq!((lines[25], lines[32]), ("2026-03,40.00,0.00,-6.00,0.00,0.00,", "2026-10,7.00,0.00,-1.05,0.00,0.00,-1.50"));
        assert_eq!(&lines[34..38], [",,,", "Account,Dividends,Interest,Tax,Fees,Other,Commissions", "Long term,85.00,8.00,-12.75,-10.00,0.00,-23.50", "Trading,7.00,0.00,-1.05,0.00,0.00,-4.20"]);
        assert_eq!(&lines[38..], [",,,", "Holding,Symbol,Dividends,Tax,Value,Yield", "World fund,VWRA,4.00,-0.60,39000.00,0.01%", "Microsoft,MSFT,2.00,-0.30,4300.00,0.05%", "Nvidia,NVDA,1.00,-0.15,,"]);
        // A budget whose statements list no trades has neither column.
        let mut quiet = b.clone();
        for entry in quiet.extra.get_mut(investments::KEY).and_then(|v| v.as_array_mut()).unwrap() {
            entry.as_object_mut().unwrap().retain(|key, _| key != "trades" && key != "incomeBy");
        }
        let file = crate::commands::read(&quiet, "investmentsCsv", &params).unwrap();
        let lines: Vec<&str> = file.as_str().unwrap().lines().collect();
        assert_eq!((lines[0], lines[22], lines.len()), ("Month,Value,Money put in,Gain,Long term,Pension,Trading,Euro funds", "Month,Dividends,Interest,Tax,Fees,Other", 38));
    }

    /// The fixture the desktop's, Android's and iPhone's report tests build their view models
    /// from: the budget with the filter, and the report the engine gives for them. Run with
    /// UPDATE_INVESTMENTS_FIXTURE=1 to write both again after a change that is meant.
    #[test]
    fn the_shared_fixture_is_this_budget_and_the_engines_report_for_it() {
        let dir = format!("{}/../../shared/fixtures", env!("CARGO_MANIFEST_DIR"));
        let (given, expected) = (format!("{dir}/investments-report.json"), format!("{dir}/investments-report-expected.json"));
        let filter = json!({"from": "2026-01", "to": "2026-11", "accountIds": []});
        let report = serde_json::to_value(portfolio(&fixture(), &fixture_range()).unwrap()).unwrap();
        if std::env::var("UPDATE_INVESTMENTS_FIXTURE").is_ok() {
            let whole = json!({"fixtureNotice": "Synthetic data only; no real account or person. The engine's investmentsReport for this budget and filter is investments-report-expected.json.", "filter": filter, "budget": fixture()});
            std::fs::write(&given, format!("{}\n", serde_json::to_string_pretty(&whole).unwrap())).unwrap();
            std::fs::write(&expected, format!("{}\n", serde_json::to_string_pretty(&report).unwrap())).unwrap();
            return;
        }
        let read = |path: &str| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e} (run with UPDATE_INVESTMENTS_FIXTURE=1)"))).unwrap()
        };
        let kept = read(&given);
        assert_eq!(kept["filter"], filter);
        assert_eq!(kept["budget"], serde_json::to_value(fixture()).unwrap(), "the fixture's budget changed: run with UPDATE_INVESTMENTS_FIXTURE=1");
        // What the apps do with it: the budget as saved, through the read command.
        let budget: Budget = serde_json::from_value(kept["budget"].clone()).unwrap();
        let answered = crate::commands::read(&budget, "investmentsReport", &kept["filter"]).unwrap();
        assert_eq!(answered, report);
        assert!(same(&answered, &read(&expected)), "the report changed: run with UPDATE_INVESTMENTS_FIXTURE=1\n{answered:#}");
    }
    /// Whether two JSON values say the same, a fraction to twelve places: one read back from a
    /// file can differ from the one written in its last digit.
    fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        use serde_json::Value::{Array, Number, Object};
        match (a, b) {
            (Number(x), Number(y)) if x.is_f64() || y.is_f64() => x.as_f64().zip(y.as_f64()).is_some_and(|(x, y)| (x - y).abs() < 1e-12),
            (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
            (Object(x), Object(y)) => x.len() == y.len() && x.iter().all(|(key, x)| y.get(key).is_some_and(|y| same(x, y))),
            _ => a == b,
        }
    }
}
