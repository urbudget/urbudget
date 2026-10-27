use crate::{i18n::tr, t};
use crate::{domain::*, ledger, schedules};
use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub account_ids: Vec<String>,
    #[serde(default)]
    pub category_ids: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Month {
    pub month: String,
    pub income: i64,
    pub spending: i64,
    pub net_income: i64,
    pub assets: i64,
    pub debt: i64,
    pub net_worth: i64,
    /// Money into budget accounts from outside them (income, refunds, transfers from tracking).
    pub inflow: i64,
    /// Money out of budget accounts (spending, transfers to tracking), as a positive amount.
    pub outflow: i64,
    pub categories: BTreeMap<String, i64>,
    pub income_sources: BTreeMap<String, i64>,
    pub accounts: BTreeMap<String, i64>,
}
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PayeeTotal {
    pub amount: i64,
    /// Transactions with spending at this payee.
    pub count: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub months: Vec<Month>,
    pub categories: BTreeMap<String, i64>,
    pub income_sources: BTreeMap<String, i64>,
    /// Spending by payee over the range.
    pub payees: BTreeMap<String, PayeeTotal>,
    /// Spending on each day of the range that had any.
    pub days: BTreeMap<String, i64>,
    pub income: i64,
    pub spending: i64,
    /// Distinct saved transactions with selected expense parts (including refunds).
    pub transaction_count: usize,
    pub age_of_money: Option<i64>,
    /// Each account's balance just before the range starts (for a balance sheet's first column).
    pub opening_accounts: BTreeMap<String, i64>,
    /// Age of money at the end of each month in the range (None until there is an outflow to age).
    pub age_of_money_by_month: BTreeMap<String, Option<i64>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contribution {
    /// The original saved record to open, even when this row is an expanded transfer leg.
    pub transaction_id: String,
    pub date: String,
    /// The budget side of this activity, matching the report's account filter.
    pub account_id: String,
    pub category_id: String,
    pub payee: String,
    pub memo: String,
    /// Spending is positive and refunds negative, exactly as in the report totals.
    pub amount: i64,
}
/// Drilldowns visit the same accounting-filtered parts as totals, including split
/// transfers, refunds, payment exclusions and the original saved record identity.
/// Ordinary reports avoid allocating these rows.
pub fn details(b: &Budget, f: &Filter, dimension: &str, key: &str) -> Result<Vec<Contribution>, String> {
    if !matches!(dimension, "category" | "payee" | "day") {
        return Err(t!("Choose a category, payee or day for the report detail."));
    }
    let mut rows = vec![];
    calculate_with(b, f, |t, saved_id, account_id, part, category_id, amount| {
        let payee = ledger::split_payee(t, part);
        let matches = match dimension { "category" => category_id == key, "payee" => payee == key, _ => t.date == key };
        if matches && amount != 0 {
            rows.push(Contribution {
                transaction_id: saved_id.into(), date: t.date.clone(), account_id: account_id.into(),
                category_id: category_id.into(), payee: payee.into(),
                memo: part.extra.get("memo").and_then(serde_json::Value::as_str).unwrap_or(&t.memo).into(), amount,
            });
        }
    })?;
    Ok(rows)
}
pub fn calculate(b: &Budget, f: &Filter) -> Result<Report, String> {
    calculate_with(b, f, |_, _, _, _, _, _| {})
}
fn calculate_with(b: &Budget, f: &Filter, mut contribution: impl FnMut(&Transaction, &str, &str, &Split, &str, i64)) -> Result<Report, String> {
    validate(b)?;
    let date = |s: &str| -> Result<NaiveDate, &str> {
        if s.len() != 7 { return Err(tr("Choose valid months.")); }
        let date = NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d")
            .map_err(|_| tr("Choose valid months."))?;
        if date.format("%Y-%m").to_string() != s { return Err(tr("Choose valid months.")); }
        Ok(date)
    };
    let mut current = date(&f.from)?;
    let end = date(&f.to)?;
    if current > end || (end - current).num_days() > 366 * 100 {
        return Err(t!("Choose a range of up to 100 years."));
    }
    let selected: HashSet<_> = f.account_ids.iter().map(String::as_str).collect();
    let cats: HashSet<_> = f.category_ids.iter().map(String::as_str).collect();
    let include = |id: &str| selected.is_empty() || selected.contains(id);
    let month_count = ((end.year() - current.year()) * 12 + end.month() as i32 - current.month() as i32 + 1) as usize;
    let category_count = b.categories.iter().filter(|c| cats.is_empty() || cats.contains(c.id.as_str())).count()
        + usize::from(cats.is_empty() || cats.contains(ledger::UNCATEGORIZED_CATEGORY));
    validate_history_workload(month_count, category_count, b.accounts.iter().filter(|a| include(&a.id)).count(), 0)?;
    let accounts: HashMap<_, _> = b.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let payment_categories: HashSet<_> = b.categories.iter().filter(|c| c.payment_account().is_some())
        .map(|c| c.id.as_str()).collect();
    let mut entries = ledger::transactions(b);
    // Share the chronological ledger with cash aging, and visit each entry only once
    // for period totals and running balances instead of rescanning history each month.
    entries.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
    let mut pending = entries.iter().peekable();
    let mut months = vec![];
    let mut total = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut payees: BTreeMap<String, PayeeTotal> = BTreeMap::new();
    let mut payee_transactions: HashMap<String, HashSet<String>> = HashMap::new();
    let mut spending_transactions = HashSet::new();
    let mut days = BTreeMap::new();
    let mut income = 0;
    let mut spending = 0;
    let mut opening_accounts: BTreeMap<_, _> = b
        .accounts
        .iter()
        .filter(|a| include(&a.id))
        .map(|a| (a.id.clone(), a.opening_balance))
        .collect();
    let first = current.format("%Y-%m").to_string();
    while let Some(t) = pending.next_if(|t| t.date[..7] < first[..]) {
        if let Some(n) = opening_accounts.get_mut(&t.account_id) {
            *n += t.amount;
        }
        if let Some(other) = t.transfer_account_id.as_ref() {
            if let Some(n) = opening_accounts.get_mut(other) {
                *n -= t.amount;
            }
        }
    }
    let mut balances = opening_accounts.clone();
    while current <= end {
        let month = current.format("%Y-%m").to_string();
        let mut row = Month {
            month: month.clone(),
            income: 0,
            spending: 0,
            net_income: 0,
            assets: 0,
            debt: 0,
            net_worth: 0,
            inflow: 0,
            outflow: 0,
            categories: BTreeMap::new(),
            income_sources: BTreeMap::new(),
            accounts: BTreeMap::new(),
        };
        while let Some(t) = pending.next_if(|t| t.date[..7] <= month[..]) {
            if let Some(n) = balances.get_mut(&t.account_id) {
                *n += t.amount;
            }
            if let Some(other) = t.transfer_account_id.as_ref() {
                if let Some(n) = balances.get_mut(other) {
                    *n -= t.amount;
                }
            }
            let a = accounts[t.account_id.as_str()];
            let mut direction = 1;
            let budget_id = if let Some(peer) = t.transfer_account_id.as_ref() {
                let other = accounts[peer.as_str()];
                if a.on_budget() == other.on_budget() {
                    continue;
                }
                if a.on_budget() {
                    &t.account_id
                } else {
                    direction = -1;
                    peer
                }
            } else {
                if !a.on_budget() {
                    continue;
                }
                &t.account_id
            };
            if !include(budget_id) {
                continue;
            }
            // With a category filter, cash flow covers the selected expense parts only.
            // Starting balances initialize the ledger rather than period cash flow.
            let flow: i64 = if ledger::starting_balance(t) { 0 } else if cats.is_empty() {
                t.amount * direction
            } else {
                t.splits
                    .iter()
                    .filter(|p| match ledger::classify(t, p, p.amount * direction) {
                        ledger::Flow::Income => false,
                        ledger::Flow::Expense(c) => cats.contains(c),
                        ledger::Flow::Uncategorized => cats.contains(ledger::UNCATEGORIZED_CATEGORY),
                        ledger::Flow::Opening => false,
                    })
                    .map(|p| p.amount * direction)
                    .sum()
            };
            if flow > 0 {
                row.inflow += flow;
            } else {
                row.outflow -= flow;
            }
            for part in &t.splits {
                let amount = part.amount * direction;
                let payee_name = ledger::split_payee(t, part);
                let category = match ledger::classify(t, part, amount) {
                    ledger::Flow::Income => {
                        row.income += amount;
                        *row.income_sources.entry(payee_name.to_owned()).or_default() += amount;
                        continue;
                    }
                    ledger::Flow::Opening => continue,
                    ledger::Flow::Uncategorized => ledger::UNCATEGORIZED_CATEGORY,
                    ledger::Flow::Expense(category) => category,
                };
                if !cats.is_empty() && !cats.contains(category) { continue; }
                if payment_categories.contains(category) { continue; }
                contribution(t, ledger::saved_id(t), budget_id, part, category, -amount);
                *row.categories.entry(category.to_owned()).or_default() -= amount;
                row.spending -= amount;
                if amount != 0 {
                    payees.entry(payee_name.to_owned()).or_default().amount -= amount;
                    let parent = ledger::saved_id(t).to_owned();
                    payee_transactions.entry(payee_name.to_owned()).or_default().insert(parent.clone());
                    spending_transactions.insert(parent);
                    *days.entry(t.date.clone()).or_default() -= amount;
                }
            }
        }
        // Accounts opted out of net worth still report their balances, but not as assets or debt.
        let counted = || {
            balances
                .iter()
                .filter(|(id, _)| accounts.get(id.as_str()).is_some_and(|a| a.in_net_worth()))
                .map(|(_, n)| *n)
        };
        row.assets = counted().filter(|n| *n > 0).sum();
        row.debt = -counted().filter(|n| *n < 0).sum::<i64>();
        row.net_worth = row.assets - row.debt;
        row.accounts = balances.clone();
        row.net_income = row.income - row.spending;
        for (k, v) in &row.categories {
            *total.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &row.income_sources {
            *sources.entry(k.clone()).or_default() += v;
        }
        income += row.income;
        spending += row.spending;
        months.push(row);
        current = current
            .checked_add_months(Months::new(1))
            .ok_or_else(|| t!("Invalid date"))?;
    }
    let range: Vec<String> = months.iter().map(|m| m.month.clone()).collect();
    let (age_of_money, age_of_money_by_month) = cash_age_history(b, &entries, &accounts, &range, &f.to);
    for (payee, total) in &mut payees {
        total.count = payee_transactions.get(payee).map_or(0, |ids| ids.len() as i64);
    }
    Ok(Report {
        months,
        categories: total,
        income_sources: sources,
        payees,
        days,
        income,
        spending,
        transaction_count: spending_transactions.len(),
        age_of_money,
        opening_accounts,
        age_of_money_by_month,
    })
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastEvent {
    pub date: String,
    /// In (+) or out (-) of `account_id`, a budget account.
    pub amount: i64,
    pub payee: String,
    pub account_id: String,
    /// The other budget account, for a transfer between two of them.
    pub peer_id: Option<String>,
    pub category_id: Option<String>,
    /// The schedule it comes from; none for a transaction already entered for a later date.
    pub schedule_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Forecast {
    /// Each budget account's balance at the end of today.
    pub balances: BTreeMap<String, i64>,
    /// Balances after every forecast event, calculated once for all native clients.
    pub closing_balances: BTreeMap<String, i64>,
    /// Money in and out of budget accounts after today, through the last day asked for:
    /// transactions already entered for later dates, and scheduled ones. In date order.
    pub events: Vec<ForecastEvent>,
    /// Average monthly spending in the three full months before this one.
    pub recent_spending: i64,
}
#[derive(Default)]
struct ForecastWork {
    occurrences: usize,
    events: usize,
    absolute_total: i128,
}
impl ForecastWork {
    fn occurrence(&mut self) -> Result<(), String> {
        if self.occurrences >= 1_000_000 { return Err(t!("The forecast exceeds 1,000,000 scheduled occurrences.")); }
        self.occurrences += 1;
        Ok(())
    }
    fn event(&mut self, amount: i64) -> Result<(), String> {
        if self.events >= 500_000 { return Err(t!("The forecast exceeds 500,000 events. Choose a shorter range.")); }
        let total = self.absolute_total + i128::from(amount).abs();
        if total > i128::from(MAX_SAFE_TOTAL) { return Err(t!("Forecast totals exceed the safe numeric range.")); }
        self.events += 1;
        self.absolute_total = total;
        Ok(())
    }
}
/// What's known about the months ahead, for a cash forecast.
pub fn forecast(b: &Budget, today: &str, through: &str) -> Result<Forecast, String> {
    forecast_for_accounts(b, today, through, &[])
}
pub fn forecast_for_accounts(b: &Budget, today: &str, through: &str, account_ids: &[String]) -> Result<Forecast, String> {
    validate(b)?;
    let day = |s: &str| {
        if !valid_date(s) { return Err(t!("Choose valid dates.")); }
        NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| t!("Choose valid dates."))
    };
    let (start, end) = (day(today)?, day(through)?);
    if end < start || (end - start).num_days() > 366 * 5 {
        return Err(t!("Choose up to five years ahead."));
    }
    let accounts: HashMap<_, _> = b.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let on_budget = |id: &str| accounts.get(id).is_some_and(|a| a.on_budget());
    let mut balances: BTreeMap<String, i64> = b
        .accounts
        .iter()
        .filter(|a| a.on_budget())
        .map(|a| (a.id.clone(), a.opening_balance))
        .collect();
    let entries = ledger::transactions(b);
    for t in entries.iter().filter(|t| t.date.as_str() <= today) {
        if let Some(n) = balances.get_mut(&t.account_id) { *n += t.amount; }
        if let Some(n) = t.transfer_account_id.as_ref().and_then(|p| balances.get_mut(p)) { *n -= t.amount; }
    }
    let mut work = ForecastWork { absolute_total: balances.values().map(|n| i128::from(*n).abs()).sum(), ..Default::default() };
    if work.absolute_total > i128::from(MAX_SAFE_TOTAL) { return Err(t!("Forecast balances exceed the safe numeric range.")); }
    // One event for each budget account the money moves in or out of.
    let push =
        |events: &mut Vec<ForecastEvent>, work: &mut ForecastWork, t: &Transaction, date: &str, schedule: Option<&str>| -> Result<(), String> {
            let category = t.splits.iter().find_map(|s| s.category_id.clone());
            let sides = [
                (
                    Some(&t.account_id),
                    t.amount,
                    t.transfer_account_id.as_ref(),
                ),
                (
                    t.transfer_account_id.as_ref(),
                    -t.amount,
                    Some(&t.account_id),
                ),
            ];
            for (account, amount, peer) in sides {
                let Some(account) = account.filter(|a| on_budget(a)) else {
                    continue;
                };
                if amount == 0 {
                    continue;
                }
                work.event(amount)?;
                events.push(ForecastEvent {
                    date: date.into(),
                    amount,
                    payee: t.payee.clone(),
                    account_id: account.clone(),
                    peer_id: peer.filter(|p| on_budget(p)).cloned(),
                    category_id: category.clone(),
                    schedule_id: schedule.map(String::from),
                });
            }
            Ok(())
        };
    let mut events = vec![];
    for t in entries.iter().filter(|t| t.date.as_str() > today && t.date.as_str() <= through) {
        push(&mut events, &mut work, t, &t.date, None)?;
    }
    for s in schedules::read(b)?.into_iter().filter(|s| !s.paused) {
        // A scheduled split with transfers moves money in legs, like an entered one.
        let one = Budget {
            accounts: b.accounts.clone(),
            transactions: vec![s.transaction.clone()],
            ..Budget::default()
        };
        let legs: Vec<Transaction> = ledger::transactions(&one)
            .into_iter()
            .map(Cow::into_owned)
            .collect();
        let mut next_date = s.next_date.clone();
        let mut count = 0;
        while let Some(due) = next_date {
            if due.as_str() > through {
                break;
            }
            count += 1;
            work.occurrence()?;
            if count > 10_000 {
                return Err(t!("A schedule repeats too often to forecast."));
            }
            if due.as_str() > today {
                for leg in &legs {
                    push(&mut events, &mut work, leg, &due, Some(&s.id))?;
                }
            }
            next_date = schedules::next(&due, &s.anchor_date, &s.frequency)?;
        }
    }
    events.sort_by(|a, b| a.date.cmp(&b.date).then(b.amount.cmp(&a.amount)));
    let month = start.with_day(1).ok_or_else(|| t!("Invalid date"))?;
    let shift = |n: u32| {
        month
            .checked_sub_months(Months::new(n))
            .map(|d| d.format("%Y-%m").to_string())
            .ok_or_else(|| t!("Invalid date"))
    };
    let recent = calculate(
        b,
        &Filter {
            from: shift(3)?,
            to: shift(1)?,
            account_ids: account_ids.to_vec(),
            category_ids: vec![],
        },
    )?;
    let mut closing_balances = balances.clone();
    for event in &events {
        // Each transfer already has an event for each account side.
        if let Some(balance) = closing_balances.get_mut(&event.account_id) {
            *balance = balance.checked_add(event.amount).ok_or_else(|| t!("Forecast balance exceeds the supported range."))?;
        }
    }
    Ok(Forecast {
        balances,
        closing_balances,
        events,
        recent_spending: recent.spending / 3,
    })
}
#[cfg(test)]
fn cash_age(b: &Budget, through: &str) -> Option<i64> {
    let mut entries = ledger::transactions(b);
    entries.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
    let accounts = b.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    cash_age_history(b, &entries, &accounts, &[], through).0
}
// Cash FIFO, average age of the last ten original transactions with a net cash outflow.
// Split expenses, refunds, and transfer legs are one cash movement for their parent.
// As in YNAB, each outflow keeps its exact age and the ages are summed and divided by ten,
// even before there are ten, then rounded to the nearest day.
// It is an estimate
// when the ages of opening balances are unknown; use the first recorded date.
// Returns the value through `through` and the value at the end of each of `months`,
// all from one pass over the transactions.
fn cash_age_history(
    b: &Budget,
    tx: &[Cow<'_, Transaction>],
    accounts: &HashMap<&str, &Account>,
    months: &[String],
    through: &str,
) -> (Option<i64>, BTreeMap<String, Option<i64>>) {
    let mut history: BTreeMap<String, Option<i64>> =
        months.iter().map(|m| (m.clone(), None)).collect();
    let mut pending = months.iter().peekable();
    let current =
        |ages: &VecDeque<f64>| (!ages.is_empty()).then(|| (ages.iter().sum::<f64>() / 10.0).round() as i64);
    let cash = |id: &str| {
        accounts
            .get(id)
            .is_some_and(|a| a.on_budget() && a.kind == AccountKind::Cash)
    };
    let Some(first) = tx
        .first()
        .and_then(|t| NaiveDate::parse_from_str(&t.date, "%Y-%m-%d").ok())
    else {
        return (None, history);
    };
    let mut lots: VecDeque<(NaiveDate, i64)> = VecDeque::new();
    let opening: i64 = b
        .accounts
        .iter()
        .filter(|a| cash(&a.id))
        .map(|a| a.opening_balance.max(0))
        .sum();
    if opening > 0 {
        lots.push_back((first, opening));
    }
    let mut ages: VecDeque<f64> = VecDeque::new();
    let mut deficit = 0i64;
    let mut events = BTreeMap::<(&str, &str), i64>::new();
    for t in tx.iter().filter(|t| &t.date[..7] <= through) {
        let mut amount = if cash(&t.account_id) { t.amount } else { 0 };
        if let Some(peer) = &t.transfer_account_id {
            if cash(peer) { amount -= t.amount; }
        }
        if amount == 0 { continue; }
        let parent = ledger::saved_id(t);
        *events.entry((t.date.as_str(), parent)).or_default() += amount;
    }
    for ((date, _), n) in events {
        // Months that ended before this transaction keep the value reached so far.
        while let Some(m) = pending.next_if(|m| m.as_str() < &date[..7]) {
            history.insert(m.clone(), current(&ages));
        }
        let Ok(d) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            return (None, history);
        };
        if n > 0 {
            let repayment = n.min(deficit);
            deficit -= repayment;
            if n > repayment {
                lots.push_back((d, n - repayment));
            }
        }
        if n < 0 {
            let mut remaining = -n;
            let mut weight = 0i128;
            while remaining > 0 {
                let Some((date, available)) = lots.front_mut() else {
                    break;
                };
                let take = remaining.min(*available);
                weight += take as i128 * (d - *date).num_days() as i128;
                *available -= take;
                remaining -= take;
                if *available == 0 {
                    lots.pop_front();
                }
            }
            if remaining > 0 {
                deficit += remaining;
                ages.clear();
                continue;
            }
            ages.push_back(weight as f64 / (-n) as f64);
            if ages.len() > 10 {
                ages.pop_front();
            }
        }
    }
    let last = current(&ages);
    for m in pending {
        history.insert(m.clone(), last);
    }
    (last, history)
}
pub fn csv(budget: &Budget, report: &Report) -> Result<String, String> {
    // Two blocks with different columns in one file.
    let mut w = csv::WriterBuilder::new().flexible(true).from_writer(vec![]);
    w.write_record([
        "Month",
        "Income",
        "Spending",
        "Net income",
        "Assets",
        "Debt",
        "Net worth",
    ])
    .map_err(|e| e.to_string())?;
    let money = |n: i64| {
        format!(
            "{}{}.{:02}",
            if n < 0 { "-" } else { "" },
            n.abs() / 100,
            n.abs() % 100
        )
    };
    for m in &report.months {
        w.write_record([
            m.month.clone(),
            money(m.income),
            money(m.spending),
            money(m.net_income),
            money(m.assets),
            money(m.debt),
            money(m.net_worth),
        ])
        .map_err(|e| e.to_string())?;
    }
    // Second block, long format: one row per month and category or income source.
    let names: HashMap<&str, &str> = budget
        .categories
        .iter()
        .map(|c| (c.id.as_str(), c.name.as_str()))
        .collect();
    w.write_record([""; 4]).map_err(|e| e.to_string())?;
    w.write_record(["Month", "Type", "Name", "Amount"])
        .map_err(|e| e.to_string())?;
    for m in &report.months {
        for (id, n) in &m.categories {
            let name = if id == ledger::UNCATEGORIZED_CATEGORY { "Needs a category" }
                else { names.get(id.as_str()).copied().unwrap_or(id) };
            w.write_record([m.month.as_str(), "Spending", name, &money(*n)])
                .map_err(|e| e.to_string())?;
        }
        for (payee, n) in &m.income_sources {
            let name = if payee.is_empty() { "No payee" } else { payee };
            w.write_record([m.month.as_str(), "Income", name, &money(*n)])
                .map_err(|e| e.to_string())?;
        }
    }
    String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tests::*;
    fn october(categories: &[&str]) -> Filter {
        Filter {
            from: "2026-10".into(), to: "2026-10".into(), account_ids: vec![],
            category_ids: categories.iter().map(|c| (*c).into()).collect(),
        }
    }
    #[test]
    fn reports_require_canonical_months_and_bound_the_selected_history_size() {
        for invalid in ["2026-1", "2026-13", " 2026-01", "2026-01-01", "é2026-1", "+10000-01"] {
            for from in [false, true] {
                let mut filter = october(&[]);
                if from { filter.from = invalid.into(); } else { filter.to = invalid.into(); }
                assert!(calculate(&sample(), &filter).is_err(), "{invalid}");
            }
        }
        let mut b = sample();
        for n in 0..1700 {
            b.categories.push(Category { id: format!("extra-{n}"), name: format!("Extra {n}"), group: "Extra".into(),
                target: 0, extra: Default::default() });
        }
        let mut filter = Filter { from: "1927-01".into(), to: "2026-12".into(), account_ids: vec![], category_ids: vec![] };
        assert!(calculate(&b, &filter).err().unwrap().contains("supported history size"));
        filter.category_ids = vec!["food".into()];
        filter.account_ids = vec!["cash".into()];
        assert_eq!(calculate(&b, &filter).unwrap().months.len(), 1200, "selected counts bound actual report rows");
    }
    #[test]
    fn forecast_limits_are_global_and_checked_before_the_next_output_event() {
        let mut work = ForecastWork { occurrences: 999_999, events: 499_999, absolute_total: 0 };
        work.occurrence().unwrap();
        assert!(work.occurrence().unwrap_err().contains("scheduled occurrences"));
        work.event(1).unwrap();
        assert!(work.event(1).unwrap_err().contains("500,000 events"));
        assert_eq!((work.events, work.absolute_total), (500_000, 1));
        let mut work = ForecastWork { absolute_total: i128::from(MAX_SAFE_TOTAL) - 1, ..Default::default() };
        work.event(-1).unwrap();
        assert!(work.event(1).unwrap_err().contains("safe numeric range"));
        assert_eq!(work.events, 1, "the rejected event is not included");
    }
    #[test]
    fn forecasts_reject_noncanonical_dates_and_cumulative_scheduled_amount_overflow() {
        for invalid in ["1-1-1", "2026-1-1", "+10000-01-01", "2026-02-30", "é2026-01"] {
            assert!(forecast(&sample(), invalid, "2027-01-01").is_err());
            assert!(forecast(&sample(), "2026-01-01", invalid).is_err());
        }
        let mut b = sample();
        let mut plans = vec![];
        for n in 0..6 {
            let mut t = purchase();
            t.account_id = "cash".into(); t.amount = -1_000_000_000_000; t.splits[0].amount = t.amount;
            plans.push(schedules::Schedule { extra: Default::default(), id: format!("large-{n}"), next_date: Some("2026-01-02".into()),
                anchor_date: "2026-01-02".into(), frequency: "daily".into(), paused: false, transaction: t });
        }
        b.extra.insert("schedules".into(), serde_json::json!(plans));
        validate(&b).unwrap();
        assert!(forecast(&b, "2026-01-01", "2030-12-31").err().unwrap().contains("safe numeric range"));
    }
    #[test]
    fn missing_categories_are_expenses_but_intentional_rta_is_signed_income() {
        for account in ["cash", "card"] {
            let mut b = sample();
            let mut missing = purchase();
            missing.account_id = account.into();
            missing.splits[0].category_id = None;
            b.transactions.push(missing.clone());
            let mut income = missing.clone();
            income.id = "salary".into(); income.payee = "Employer".into();
            income.amount = 10_000; income.splits[0].amount = 10_000;
            b.transactions.push(income);
            let mut correction = missing.clone();
            correction.id = "correction".into(); correction.payee = "Correction".into();
            correction.amount = -2000; correction.splits[0].amount = -2000;
            correction.splits[0].extra.insert("unassigned".into(), serde_json::json!(true));
            b.transactions.push(correction);
            let mut refund = purchase();
            refund.id = "refund".into(); refund.account_id = account.into();
            refund.amount = 500; refund.splits[0].amount = 500;
            b.transactions.push(refund);
            let r = calculate(&b, &october(&[])).unwrap();
            assert_eq!((r.income, r.spending, r.transaction_count), (8000, 4500, 2));
            assert_eq!(r.categories[ledger::UNCATEGORIZED_CATEGORY], 5000);
            assert_eq!(r.categories["food"], -500);
            assert_eq!(r.income_sources["Correction"], -2000);
            assert_eq!(crate::domain::calculate(&b, "2026-10").unwrap().income, 8000);
            let filtered = calculate(&b, &october(&["food"])).unwrap();
            assert_eq!((filtered.income, filtered.spending, filtered.transaction_count), (8000, -500, 1));
            assert_eq!((filtered.months[0].inflow, filtered.months[0].outflow), (500, 0), "category cash flow retains its selected-expense scope");
            let missing_only = calculate(&b, &october(&[ledger::UNCATEGORIZED_CATEGORY])).unwrap();
            assert_eq!((missing_only.income, missing_only.spending, missing_only.transaction_count), (8000, 5000, 1));
            assert_eq!(missing_only.months[0].outflow, 5000);
            assert!(csv(&b, &missing_only).unwrap().contains("2026-10,Spending,Needs a category,50.00"));
        }
    }
    #[test]
    fn imported_starting_balances_only_initialize_balances_until_edited() {
        for (account, amount) in [("cash", 5000), ("card", -5000)] {
            let mut b = sample();
            let mut t = purchase(); t.account_id = account.into(); t.amount = amount;
            t.splits[0].amount = amount; t.splits[0].category_id = None;
            t.splits[0].extra.insert("unassigned".into(), serde_json::json!(true));
            t.payee = "Starting Balance".into();
            t.extra.insert("startingBalance".into(), serde_json::json!(true));
            b.transactions.push(t);
            let opening = calculate(&b, &october(&[])).unwrap();
            assert_eq!((opening.income, opening.spending, opening.months[0].inflow, opening.months[0].outflow), (0, 0, 0, 0));
            assert_eq!(opening.months[0].accounts[account], b.accounts.iter().find(|a| a.id == account).unwrap().opening_balance + amount);
            assert_eq!(crate::domain::calculate(&b, "2026-10").unwrap().income, 0);
            b.transactions[0].payee = "Edited payee".into();
            assert_eq!(calculate(&b, &october(&[])).unwrap().income, amount);
            b.transactions[0].payee = "Starting Balance".into();
            b.transactions[0].splits[0].extra.insert("unassigned".into(), serde_json::json!(false));
            let cleared = calculate(&b, &october(&[])).unwrap();
            assert_eq!((cleared.income, cleared.spending), (amount.max(0), (-amount).max(0)));
            b.transactions[0].splits[0].category_id = Some("food".into());
            let categorized = calculate(&b, &october(&[])).unwrap();
            assert_eq!((categorized.income, categorized.spending), (0, -amount));
        }
    }
    #[test]
    fn split_payees_count_saved_transactions_once_across_parts_and_transfer_legs() {
        let mut b = sample();
        let mut tracking = b.accounts[0].clone();
        tracking.id = "invest".into(); tracking.kind = AccountKind::Tracking;
        b.accounts.push(tracking);
        let mut ordinary = purchase(); ordinary.account_id = "cash".into();
        ordinary.payee = "Combined".into();
        ordinary.splits = serde_json::from_value(serde_json::json!([
            {"categoryId":"food","amount":-1000,"payee":"Shop"},
            {"categoryId":"food","amount":-2000,"payee":"Shop"},
            {"categoryId":"food","amount":-2000,"payee":"Other"}
        ])).unwrap();
        b.transactions.push(ordinary.clone());
        let mut mixed = ordinary; mixed.id = "mixed".into();
        mixed.splits = serde_json::from_value(serde_json::json!([
            {"categoryId":"food","amount":-2000,"payee":"Shop"},
            {"categoryId":"food","amount":-3000,"payee":"Shop","transferAccountId":"invest"}
        ])).unwrap();
        b.transactions.push(mixed);
        let r = calculate(&b, &october(&[])).unwrap();
        assert_eq!((r.spending, r.transaction_count), (10_000, 2));
        assert_eq!((r.payees["Shop"].amount, r.payees["Shop"].count), (8000, 2));
        assert_eq!((r.payees["Other"].amount, r.payees["Other"].count), (2000, 1));
        assert!(!r.payees.contains_key("Combined"));
        assert_eq!(r.days["2026-10-15"], 10_000);
        assert_eq!(serde_json::to_value(&r).unwrap()["transactionCount"], 2);
        // Retained provider metadata cannot make an ordinary saved transaction
        // impersonate another saved parent in counts or payee averages.
        b.transactions[0].extra.insert("parentId".into(), serde_json::json!("mixed"));
        let with_metadata = calculate(&b, &october(&[])).unwrap();
        assert_eq!(serde_json::to_value(&with_metadata).unwrap(), serde_json::to_value(&r).unwrap());
    }
    #[test]
    fn drilldowns_sum_to_visible_totals_and_open_original_records_across_split_transfers() {
        let mut b = sample();
        let mut tracking = b.accounts[0].clone();
        tracking.id = "invest".into(); tracking.kind = AccountKind::Tracking;
        b.accounts.push(tracking);
        let mut mixed = purchase(); mixed.account_id = "cash".into(); mixed.id = "mixed".into();
        mixed.splits = serde_json::from_value(serde_json::json!([
            {"categoryId":"food","amount":-2000,"payee":"Shop","memo":"Groceries"},
            {"categoryId":"food","amount":-1000,"payee":"Shop","transferAccountId":"invest"},
            {"categoryId":null,"amount":-2000,"transferAccountId":"card"}
        ])).unwrap();
        b.transactions.push(mixed);
        let mut refund = purchase(); refund.id = "refund".into(); refund.account_id = "cash".into();
        refund.amount = 500; refund.splits[0].amount = 500; refund.payee = "Shop".into();
        // Provider metadata must not redirect a saved record's drilldown identity.
        refund.extra.insert("parentId".into(), serde_json::json!("different-record"));
        b.transactions.push(refund);
        let mut rta = purchase(); rta.id = "correction".into(); rta.account_id = "cash".into();
        rta.splits[0].category_id = None; rta.splits[0].extra.insert("unassigned".into(), serde_json::json!(true));
        b.transactions.push(rta);
        let mut f = october(&["food"]); f.account_ids = vec!["cash".into()];
        let totals = calculate(&b, &f).unwrap();
        for (dimension, key, expected) in [("category", "food", totals.categories["food"]),
            ("payee", "Shop", totals.payees["Shop"].amount), ("day", "2026-10-15", totals.days["2026-10-15"])] {
            let rows = details(&b, &f, dimension, key).unwrap();
            assert_eq!(rows.iter().map(|r| r.amount).sum::<i64>(), expected);
            assert_eq!(rows.len(), 3, "internal card payment and RTA correction do not become spending rows");
            assert!(rows.iter().all(|r| r.account_id == "cash"));
            assert_eq!(rows.iter().filter(|r| r.transaction_id == "mixed").count(), 2);
            assert_eq!(rows.iter().find(|r| r.transaction_id == "refund").unwrap().amount, -500);
            assert!(rows.iter().any(|r| r.memo == "Groceries"));
        }
        f.account_ids = vec!["card".into()];
        assert!(details(&b, &f, "category", "food").unwrap().is_empty());
        assert!(details(&b, &f, "unsupported", "food").is_err());
    }
    #[test]
    fn drilldown_tracking_origin_uses_budget_side_amount_and_account() {
        let mut b = sample(); let mut tracking = b.accounts[0].clone();
        tracking.id = "invest".into(); tracking.kind = AccountKind::Tracking;
        b.accounts.push(tracking);
        let mut t = purchase(); t.account_id = "invest".into(); t.amount = 5000; t.splits[0].amount = 5000;
        t.transfer_account_id = Some("cash".into()); t.transfer_status = Some(Cleared::Cleared);
        b.transactions.push(t);
        let rows = details(&b, &october(&[]), "category", "food").unwrap();
        assert_eq!((rows[0].account_id.as_str(), rows[0].amount), ("cash", 5000));
    }
    #[test]
    fn ordinary_split_income_uses_each_parts_payee_even_with_an_expense_filter() {
        let mut b = sample(); let mut t = purchase();
        t.amount = 5000; t.payee = "Combined".into();
        t.splits = serde_json::from_value(serde_json::json!([
            {"categoryId":null,"amount":2000,"payee":"Employer A"},
            {"categoryId":null,"amount":3000,"payee":"Employer B"}
        ])).unwrap();
        b.transactions.push(t);
        let r = calculate(&b, &october(&["food"])).unwrap();
        assert_eq!((r.income, r.spending, r.transaction_count), (5000, 0, 0));
        assert_eq!(r.income_sources["Employer A"], 2000);
        assert_eq!(r.income_sources["Employer B"], 3000);
        assert!(!r.income_sources.contains_key("Combined"));
    }
    #[test]
    fn tracking_transfers_classify_the_amount_from_the_budget_side() {
        let mut b = sample(); let mut tracking = b.accounts[0].clone();
        tracking.id = "invest".into(); tracking.kind = AccountKind::Tracking;
        b.accounts.push(tracking);
        let mut t = purchase(); t.account_id = "invest".into(); t.amount = 5000;
        t.transfer_account_id = Some("cash".into()); t.transfer_status = Some(Cleared::Cleared);
        t.splits[0].category_id = None;
        t.splits[0].amount = 5000;
        b.transactions.push(t);
        let unknown = calculate(&b, &october(&[])).unwrap();
        assert_eq!((unknown.income, unknown.spending), (0, 5000));
        assert_eq!(unknown.categories[ledger::UNCATEGORIZED_CATEGORY], 5000);
        assert_eq!(crate::domain::calculate(&b, "2026-10").unwrap().income, 0);
        b.transactions[0].splits[0].extra.insert("unassigned".into(), serde_json::json!(true));
        let intentional = calculate(&b, &october(&[])).unwrap();
        assert_eq!((intentional.income, intentional.spending), (-5000, 0));
        assert_eq!(crate::domain::calculate(&b, "2026-10").unwrap().income, -5000);
    }
    #[test]
    fn reports_count_card_purchase_once_and_use_month_end_balances() {
        let mut b = sample();
        b.transactions.push(purchase());
        let mut pay = purchase();
        pay.id = "pay".into();
        pay.account_id = "cash".into();
        pay.date = "2026-11-01".into();
        pay.transfer_account_id = Some("card".into());
        pay.transfer_status = Some(Cleared::Cleared);
        pay.splits.clear();
        b.transactions.push(pay);
        let r = calculate(
            &b,
            &Filter {
                from: "2026-10".into(),
                to: "2026-11".into(),
                account_ids: vec![],
                category_ids: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.spending, 5000);
        assert_eq!(r.months[0].accounts["cash"], 100000);
        assert_eq!(r.months[1].accounts["cash"], 95000);
        assert_eq!(r.months[0].net_worth, r.months[1].net_worth);
    }
    #[test]
    fn report_range_carries_opening_transfers_through_empty_months_and_excludes_later_entries() {
        let mut b = sample();
        for (id, date, account, amount, peer) in [
            ("old-expense", "2026-08-01", "cash", -2000, None),
            ("old-payment", "2026-09-01", "cash", -3000, Some("card")),
            ("purchase", "2026-10-01", "card", -5000, None),
            ("refund", "2026-12-01", "cash", 1000, None),
            ("later", "2027-01-01", "cash", -700, None),
        ] {
            let mut t = purchase();
            t.id = id.into(); t.date = date.into(); t.account_id = account.into(); t.amount = amount;
            t.splits[0].amount = amount;
            if let Some(peer) = peer {
                t.transfer_account_id = Some(peer.into());
                t.transfer_status = Some(Cleared::Cleared);
                t.splits.clear();
            }
            b.transactions.push(t);
        }
        b.transactions.reverse();
        let mut filter = Filter { from: "2026-10".into(), to: "2026-12".into(), account_ids: vec![], category_ids: vec![] };
        let report = calculate(&b, &filter).unwrap();
        assert_eq!((report.opening_accounts["cash"], report.opening_accounts["card"]), (95000, -7000));
        assert_eq!(report.months.iter().map(|m| (m.accounts["cash"], m.accounts["card"], m.spending)).collect::<Vec<_>>(),
            [(95000, -12000, 5000), (95000, -12000, 0), (96000, -12000, -1000)]);
        assert_eq!((report.spending, report.transaction_count), (4000, 2));
        filter.account_ids = vec!["cash".into()];
        filter.category_ids = vec!["food".into()];
        let filtered = calculate(&b, &filter).unwrap();
        assert_eq!((filtered.spending, filtered.transaction_count), (-1000, 1));
        assert_eq!(filtered.opening_accounts, BTreeMap::from([("cash".into(), 95000)]));
        assert!(filtered.months.iter().all(|m| m.accounts.len() == 1));
        assert_eq!(filtered.months[2].inflow, 1000);
    }
    #[test]
    fn refunds_net_against_spending_and_tracking_is_excluded() {
        let mut b = sample();
        b.transactions.push(purchase());
        let mut refund = purchase();
        refund.id = "refund".into();
        refund.amount = 1000;
        refund.splits[0].amount = 1000;
        b.transactions.push(refund);
        let r = calculate(
            &b,
            &Filter {
                from: "2026-10".into(),
                to: "2026-10".into(),
                account_ids: vec![],
                category_ids: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.spending, 4000);
        assert_eq!(r.income, 0);
    }
    #[test]
    fn cash_age_recovers_after_a_historical_cash_deficit() {
        let mut b = sample();
        b.accounts[0].opening_balance = 0;
        let mut early = purchase();
        early.account_id = "cash".into();
        early.date = "2026-10-01".into();
        early.amount = -100;
        early.splits[0].amount = -100;
        b.transactions.push(early.clone());
        let mut income = early.clone();
        income.id = "income".into();
        income.date = "2026-10-02".into();
        income.amount = 2100;
        income.splits[0].amount = 2100;
        income.splits[0].category_id = None;
        b.transactions.push(income);
        for i in 0..10 {
            let mut expense = early.clone();
            expense.id = format!("expense-{i}");
            expense.date = "2026-10-12".into();
            b.transactions.push(expense);
        }
        assert_eq!(cash_age(&b, "2026-10"), Some(10));
    }
    #[test]
    fn cash_age_counts_saved_outflows_and_nets_split_refunds_and_internal_cash_moves() {
        let mut b = sample();
        b.accounts[0].opening_balance = 0;
        let mut savings = b.accounts[0].clone();
        savings.id = "savings".into();
        b.accounts.push(savings);
        let entry = |id: &str, date: &str, amount: i64, category: Option<&str>| {
            let mut t = purchase();
            t.id = id.into(); t.date = date.into(); t.account_id = "cash".into(); t.amount = amount;
            t.splits[0].amount = amount; t.splits[0].category_id = category.map(str::to_owned);
            t
        };
        b.transactions.push(entry("income", "2026-10-01", 3000, None));
        b.transactions.push(entry("later-income", "2026-10-11", 1000, None));
        let mut internal = entry("internal", "2026-10-20", -300, None);
        internal.transfer_account_id = Some("savings".into());
        internal.transfer_status = Some(Cleared::Cleared);
        internal.splits.clear();
        b.transactions.push(internal);
        let mut mixed = entry("mixed", "2026-10-21", -1000, Some("food"));
        mixed.splits = vec![Split { category_id: Some("food".into()), amount: -100, extra: Default::default() }; 10];
        mixed.splits.push(Split { category_id: Some("food".into()), amount: 200, extra: Default::default() });
        mixed.splits.push(Split { category_id: None, amount: -200,
            extra: serde_json::json!({"transferAccountId":"savings"}).as_object().unwrap().clone().into() });
        b.transactions.push(mixed.clone());
        // A split refund that cancels its purchase, plus an internal move, is not an outflow.
        let mut neutral = entry("neutral", "2026-10-22", -100, Some("food"));
        neutral.splits = serde_json::from_value(serde_json::json!([
            {"categoryId":"food", "amount":-100}, {"categoryId":"food", "amount":100},
            {"categoryId":null, "amount":-100, "transferAccountId":"savings"}
        ])).unwrap();
        b.transactions.push(neutral);
        // One outflow aged 20 days, over ten.
        assert_eq!(calculate(&b, &october(&[])).unwrap().age_of_money, Some(2), "ten expense splits still represent one cash outflow");
        for n in 0..8 {
            let mut t = entry(&format!("outflow-{n}"), "2026-10-22", -100, Some("food"));
            t.extra.insert("parentId".into(), serde_json::json!("unrelated-import-metadata"));
            b.transactions.push(t);
        }
        assert_eq!(cash_age(&b, "2026-10"), Some(19), "nine saved outflows still divide by ten: 188 / 10");
        b.transactions.push(entry("tenth", "2026-10-31", -2000, Some("food")));
        // First outflow: 20 days; next eight: 21; last: 1,400 cents aged 30 days
        // plus 600 aged 20 = 27 days. The ten-event average, 215 / 10, rounds up.
        assert_eq!(calculate(&b, &october(&[])).unwrap().age_of_money, Some(22));
        let mut simple = b.clone();
        let parent = simple.transactions.iter_mut().find(|t| t.id == "mixed").unwrap();
        *parent = entry("mixed", "2026-10-21", -800, Some("food"));
        simple.transactions.retain(|t| t.id != "neutral" && t.id != "internal");
        assert_eq!(cash_age(&simple, "2026-10"), cash_age(&b, "2026-10"), "cash-equivalent splits must have the same age");
        b.transactions.reverse();
        assert_eq!(cash_age(&b, "2026-10"), Some(22));
    }
    #[test]
    fn age_of_money_history_records_each_month_end_and_matches_the_final_value() {
        let mut b = sample();
        b.accounts[0].opening_balance = 0;
        let mut income = purchase();
        income.id = "income".into();
        income.account_id = "cash".into();
        income.date = "2026-09-01".into();
        income.amount = 100_000;
        income.splits[0].amount = 100_000;
        income.splits[0].category_id = None;
        b.transactions.push(income);
        for i in 0..10 {
            let mut expense = purchase();
            expense.id = format!("e{i}");
            expense.account_id = "cash".into();
            expense.date = format!("2026-10-{:02}", 11 + i);
            expense.amount = -100;
            expense.splits[0].amount = -100;
            b.transactions.push(expense);
        }
        let r = calculate(
            &b,
            &Filter {
                from: "2026-08".into(),
                to: "2026-11".into(),
                account_ids: vec![],
                category_ids: vec![],
            },
        )
        .unwrap();
        let h = &r.age_of_money_by_month;
        assert_eq!(h.len(), 4);
        assert_eq!(h["2026-08"], None);
        assert_eq!(h["2026-09"], None, "no outflows so far");
        // Outflows on Oct 11..20 from cash received Sep 1: ages 40..49, average 44.5 rounds up.
        assert_eq!(h["2026-10"], Some(45));
        assert_eq!(h["2026-11"], r.age_of_money);
        let csv = csv(&b, &r).unwrap();
        assert!(csv.contains("2026-10,Spending,Food,10.00"));
    }
    fn cash_entry(id: &str, date: &str, amount: i64) -> Transaction {
        let mut t = purchase();
        t.id = id.into(); t.date = date.into(); t.account_id = "cash".into(); t.amount = amount;
        t.splits[0].amount = amount;
        if amount > 0 { t.splits[0].category_id = None; }
        t
    }
    #[test]
    fn cash_age_divides_by_ten_before_there_are_ten_outflows_like_ynab() {
        let mut b = sample();
        b.accounts[0].opening_balance = 0;
        b.transactions.push(cash_entry("income", "2024-10-01", 10_000));
        assert_eq!(cash_age(&b, "2024-10"), None, "no outflows yet");
        b.transactions.push(cash_entry("first", "2024-10-05", -100));
        assert_eq!(cash_age(&b, "2024-10"), Some(0), "4 / 10 rounds down");
        // YNAB shows 6 days for these nine ages in Oct 2024: 59 / 10, to the nearest day.
        for (n, day) in [5, 6, 7, 8, 8, 9, 10, 10].into_iter().enumerate() {
            b.transactions.push(cash_entry(&format!("outflow-{n}"), &format!("2024-10-{day:02}"), -100));
        }
        assert_eq!(cash_age(&b, "2024-10"), Some(6));
    }
    #[test]
    fn cash_age_keeps_each_outflows_exact_age_and_rounds_half_up() {
        let mut b = sample();
        b.accounts[0].opening_balance = 0;
        for day in 1..=20 {
            b.transactions.push(cash_entry(&format!("income-{day}"), &format!("2026-10-{day:02}"), 50));
        }
        // Each outflow takes two days' income, one day apart: ages 23.5, 21.5, ..., 5.5.
        for n in 0..10 {
            b.transactions.push(cash_entry(&format!("outflow-{n}"), "2026-10-25", -100));
        }
        assert_eq!(cash_age(&b, "2026-10"), Some(15), "145 / 10 rounds up; flooring each age gave 14");
        // The oldest drops out for one aged 3 days: 124.5 / 10 rounds down.
        b.transactions.push(cash_entry("late-income", "2026-10-23", 100));
        b.transactions.push(cash_entry("eleventh", "2026-10-26", -100));
        assert_eq!(cash_age(&b, "2026-10"), Some(12));
    }
    #[test]
    fn cash_age_starts_over_after_spending_more_cash_than_there_was() {
        let mut b = sample();
        b.accounts[0].opening_balance = 0;
        b.transactions.push(cash_entry("income", "2026-08-01", 1000));
        b.transactions.push(cash_entry("august", "2026-08-11", -500));
        b.transactions.push(cash_entry("short", "2026-09-01", -600));
        b.transactions.push(cash_entry("repaid", "2026-10-02", 2100));
        b.transactions.push(cash_entry("october", "2026-10-12", -100));
        for n in 0..9 {
            b.transactions.push(cash_entry(&format!("november-{n}"), "2026-11-01", -100));
        }
        let r = calculate(
            &b,
            &Filter { from: "2026-08".into(), to: "2026-11".into(), account_ids: vec![], category_ids: vec![] },
        )
        .unwrap();
        let h = &r.age_of_money_by_month;
        assert_eq!(h["2026-08"], Some(1), "one outflow aged 10 days, over ten");
        assert_eq!(h["2026-09"], None, "spending 100 more than there was clears the outflows");
        assert_eq!(h["2026-10"], Some(1), "August's outflow no longer counts");
        // October's outflow aged 10 days and nine aged 30, from cash received after the shortfall.
        assert_eq!((h["2026-11"], r.age_of_money), (Some(28), Some(28)));
    }
    #[test]
    fn accounts_left_out_of_net_worth_keep_balances_but_not_assets_and_opening_is_before_the_range()
    {
        let mut b = sample();
        b.accounts.push(Account {
            extra: serde_json::json!({"netWorth": false, "accountType": "receivable"})
                .as_object()
                .unwrap()
                .clone(),
            id: "owed".into(),
            name: "Loan to a friend".into(),
            kind: AccountKind::Tracking,
            opening_balance: 50_000,
            archived: false,
        });
        let mut september = purchase();
        september.id = "sep".into();
        september.account_id = "cash".into();
        september.date = "2026-09-10".into();
        b.transactions.push(september);
        b.transactions.push(purchase());
        let filter = Filter {
            from: "2026-10".into(),
            to: "2026-10".into(),
            account_ids: vec![],
            category_ids: vec![],
        };
        let r = calculate(&b, &filter).unwrap();
        let october = &r.months[0];
        assert_eq!(october.accounts["owed"], 50_000);
        // Cash 100,000 - 5,000 (Sept) = 95,000; card -10,000 - 5,000 = -15,000.
        assert_eq!(october.assets, 95_000);
        assert_eq!(october.debt, 15_000);
        assert_eq!(october.net_worth, 80_000);
        assert_eq!(r.opening_accounts["cash"], 95_000);
        assert_eq!(r.opening_accounts["card"], -10_000);
        assert_eq!(r.opening_accounts["owed"], 50_000);
        let s = crate::domain::calculate(&b, "2026-10").unwrap();
        assert_eq!(s.net_worth, 80_000);
        assert_eq!(
            s.accounts.iter().find(|a| a.id == "owed").unwrap().balance,
            50_000
        );
    }
    #[test]
    fn payees_days_and_money_in_and_out_cover_refunds_and_transfers_to_tracking() {
        let mut b = sample();
        b.accounts.push(Account {
            extra: serde_json::Map::new(),
            id: "invest".into(),
            name: "Invest".into(),
            kind: AccountKind::Tracking,
            opening_balance: 0,
            archived: false,
        });
        b.transactions.push(purchase());
        let mut refund = purchase();
        refund.id = "refund".into();
        refund.date = "2026-10-20".into();
        refund.amount = 1_000;
        refund.splits[0].amount = 1_000;
        b.transactions.push(refund);
        let mut salary = purchase();
        salary.id = "salary".into();
        salary.account_id = "cash".into();
        salary.payee = "Employer".into();
        salary.date = "2026-10-01".into();
        salary.amount = 300_000;
        salary.splits[0].amount = 300_000;
        salary.splits[0].category_id = None;
        b.transactions.push(salary);
        let mut save = purchase();
        save.id = "save".into();
        save.account_id = "cash".into();
        save.payee = "Transfer".into();
        save.amount = -20_000;
        save.transfer_account_id = Some("invest".into());
        save.transfer_status = Some(Cleared::Cleared);
        save.splits[0].amount = save.amount;
        b.transactions.push(save);
        let r = calculate(
            &b,
            &Filter {
                from: "2026-10".into(),
                to: "2026-10".into(),
                account_ids: vec![],
                category_ids: vec![],
            },
        )
        .unwrap();
        assert_eq!(r.payees["Shop"].amount, 4_000);
        assert_eq!(r.payees["Shop"].count, 2);
        assert!(!r.payees.contains_key("Employer"));
        assert_eq!(r.days["2026-10-15"], 25_000);
        assert_eq!(r.payees["Transfer"].amount, 20_000);
        assert_eq!(r.days["2026-10-20"], -1_000);
        // In: salary and the refund. Out: the purchase and the move to a tracking account.
        assert_eq!(r.months[0].inflow, 301_000);
        assert_eq!(r.months[0].outflow, 25_000);
    }
    #[test]
    fn forecast_starts_from_todays_balances_and_lists_later_and_scheduled_money() {
        let mut b = sample();
        b.transactions.push(purchase());
        let mut later = purchase();
        later.id = "later".into();
        later.date = "2026-10-20".into();
        later.account_id = "cash".into();
        b.transactions.push(later);
        let mut rent = purchase();
        rent.id = "rent".into();
        rent.account_id = "cash".into();
        rent.payee = "Landlord".into();
        rent.amount = -50_000;
        rent.splits[0].amount = -50_000;
        let mut payment = purchase();
        payment.id = "pay".into();
        payment.account_id = "cash".into();
        payment.amount = -15_000;
        payment.splits.clear();
        payment.transfer_account_id = Some("card".into());
        payment.transfer_status = Some(Cleared::Uncleared);
        let schedule =
            |id: &str, next: &str, frequency: &str, transaction: Transaction| schedules::Schedule { extra: Default::default(),
                id: id.into(),
                next_date: Some(next.into()),
                anchor_date: next.into(),
                frequency: frequency.into(),
                paused: false,
                transaction,
            };
        let mut gym = schedule("gym", "2026-11-02", "monthly", rent.clone());
        gym.paused = true;
        let list = vec![
            schedule("rent", "2026-11-01", "monthly", rent),
            schedule("card", "2026-11-05", "never", payment),
            gym,
        ];
        b.extra
            .insert("schedules".into(), serde_json::to_value(list).unwrap());
        let f = forecast(&b, "2026-10-16", "2026-12-31").unwrap();
        assert_eq!(f.balances["cash"], 100_000);
        assert_eq!(f.balances["card"], -15_000);
        assert_eq!(f.closing_balances["cash"], -20_000);
        assert_eq!(f.closing_balances["card"], 0);
        let events: Vec<_> = f
            .events
            .iter()
            .map(|e| {
                (
                    e.date.as_str(),
                    e.account_id.as_str(),
                    e.amount,
                    e.schedule_id.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            events,
            vec![
                ("2026-10-20", "cash", -5_000, None),
                ("2026-11-01", "cash", -50_000, Some("rent")),
                ("2026-11-05", "card", 15_000, Some("card")),
                ("2026-11-05", "cash", -15_000, Some("card")),
                ("2026-12-01", "cash", -50_000, Some("rent")),
            ]
        );
        assert_eq!(f.events[2].peer_id.as_deref(), Some("cash"));
        assert_eq!(f.events[1].category_id.as_deref(), Some("food"));
        assert_eq!(f.recent_spending, 0);
        assert!(forecast(&b, "2026-10-16", "2026-10-01").is_err());
    }
    #[test]
    fn account_forecast_spending_uses_only_selected_history() {
        let mut b=sample();
        let mut savings=b.accounts[0].clone(); savings.id="savings".into(); b.accounts.push(savings);
        for (i,date) in ["2026-07-15","2026-08-15","2026-09-15"].iter().enumerate() {
            let mut t=purchase(); t.id=format!("spend{i}"); t.account_id="cash".into(); t.date=(*date).into();
            b.transactions.push(t);
        }
        assert_eq!(forecast(&b,"2026-10-20","2026-12-31").unwrap().recent_spending,5000);
        assert_eq!(forecast_for_accounts(&b,"2026-10-20","2026-12-31",&["savings".into()]).unwrap().recent_spending,0);
        assert_eq!(forecast_for_accounts(&b,"2026-10-20","2026-12-31",&["cash".into()]).unwrap().recent_spending,5000);
    }
}
