use crate::t;
use crate::domain::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_SCHEDULES: usize = 2_000;
const MAX_OCCURRENCES: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upcoming {
    pub amount: i64,
    pub first_due: String,
}

pub fn upcoming(b: &Budget, month: &str) -> Result<HashMap<String, i64>, String> {
    Ok(UpcomingIndex::new(b, month, month)?.amounts(month))
}

/// Remaining scheduled outflows by category, including cash payments to on-budget cards.
/// Due dates describe positive obligations only; scheduled inflows do not fund them early.
pub fn upcoming_details(b: &Budget, month: &str) -> Result<HashMap<String, Upcoming>, String> {
    Ok(UpcomingIndex::new(b, month, month)?
        .months
        .remove(month)
        .unwrap_or_default())
}

/// A recurrence is traversed once for a whole calculation range. Account and payment
/// category lookup, and the category impact of its template, are also resolved once.
pub struct UpcomingIndex {
    months: BTreeMap<String, HashMap<String, Upcoming>>,
}

impl UpcomingIndex {
    pub fn new(b: &Budget, from: &str, through: &str) -> Result<Self, String> {
        if !valid_month(from) || !valid_month(through) || from > through {
            return Err(t!("Choose a valid scheduled month range."));
        }
        let accounts: HashMap<&str, &Account> =
            b.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
        let payment_categories: HashMap<&str, &str> = b
            .categories
            .iter()
            .filter_map(|c| c.payment_account().map(|id| (id, c.id.as_str())))
            .collect();
        let mut months = BTreeMap::<String, HashMap<String, Upcoming>>::new();
        let mut month_totals = HashMap::<String, i128>::new();
        let mut occurrences = 0usize;
        for s in read(b)?.into_iter().filter(|s| !s.paused) {
            let mut next_date = s.next_date.clone();
            let mut count = 0;
            let mut obligations = None;
            while let Some(due) = next_date {
                let month = due.get(..7).ok_or_else(|| t!("Invalid schedule date"))?;
                if month > through {
                    break;
                }
                count += 1;
                occurrences += 1;
                if occurrences > MAX_OCCURRENCES {
                    return Err(t!("Schedule history exceeds 1,000,000 occurrences in this calculation. Adjust the schedules' next dates first."));
                }
                if count > 10000 {
                    return Err(t!("Schedule history exceeds 10,000 occurrences."));
                }
                if month >= from {
                    if obligations.is_none() {
                        let mut per_occurrence = HashMap::<String, i64>::new();
                        let account = *accounts
                            .get(s.transaction.account_id.as_str())
                            .ok_or_else(|| t!("Missing scheduled account"))?;
                        let on_budget = account.on_budget();
                        let direction = if on_budget { 1 } else { -1 };
                        let mut record = |peer: Option<&str>,
                                          category: Option<&str>,
                                          amount: i64|
                         -> Result<(), String> {
                            let mut amount = amount
                                .checked_mul(-direction)
                                .ok_or_else(|| t!("Scheduled totals exceed the supported range."))?;
                            let mut payment_category = None;
                            if let Some(other) = peer {
                                let other =
                                    *accounts.get(other).ok_or_else(|| t!("Missing scheduled account"))?;
                                if other.on_budget() == on_budget {
                                    // A cash payment consumes the receiving card's payment envelope.
                                    // Cash moves, balance transfers, and cash advances reserve no money.
                                    let card = if on_budget
                                        && account.kind == AccountKind::Cash
                                        && other.kind == AccountKind::Credit
                                        && amount > 0
                                    {
                                        Some(other)
                                    } else if on_budget
                                        && account.kind == AccountKind::Credit
                                        && other.kind == AccountKind::Cash
                                        && amount < 0
                                    {
                                        amount = amount.checked_neg().ok_or_else(|| t!("Scheduled totals exceed the supported range."))?;
                                        Some(account)
                                    } else {
                                        None
                                    };
                                    let Some(card) = card else {
                                        return Ok(());
                                    };
                                    payment_category = Some(
                                        *payment_categories
                                            .get(card.id.as_str())
                                            .ok_or_else(|| t!("Missing credit-card payment category"))?,
                                    );
                                }
                            } else if !on_budget {
                                return Ok(());
                            }
                            if let Some(c) = payment_category.or(category).filter(|_| amount > 0) {
                                let total = per_occurrence.entry(c.to_owned()).or_default();
                                *total = total
                                    .checked_add(amount)
                                    .ok_or_else(|| t!("Scheduled totals exceed the supported range."))?;
                            }
                            Ok(())
                        };
                        let internal_transfer = s
                            .transaction
                            .transfer_account_id
                            .as_deref()
                            .is_some_and(|peer| {
                                accounts
                                    .get(peer)
                                    .is_some_and(|a| a.on_budget() == on_budget)
                            });
                        if internal_transfer {
                            // The parent already describes the whole transfer. Legacy splits must not
                            // charge ordinary categories or count the same payment a second time.
                            record(
                                s.transaction.transfer_account_id.as_deref(),
                                None,
                                s.transaction.amount,
                            )?;
                        } else {
                            for part in &s.transaction.splits {
                                record(
                                    s.transaction
                                        .transfer_account_id
                                        .as_deref()
                                        .or_else(|| crate::ledger::transfer(part)),
                                    part.category_id.as_deref(),
                                    part.amount,
                                )?;
                            }
                        }
                        obligations = Some(per_occurrence);
                    }
                    let out = months.entry(month.to_owned()).or_default();
                    for (category, amount) in obligations.as_ref().unwrap() {
                        let month_total = month_totals.entry(month.to_owned()).or_default();
                        *month_total += i128::from(*amount);
                        if *month_total > i128::from(MAX_SAFE_TOTAL) {
                            return Err(t!("Scheduled monthly totals exceed the safe numeric range."));
                        }
                        let total = out.entry(category.clone()).or_insert_with(|| Upcoming {
                            amount: 0,
                            first_due: due.clone(),
                        });
                        total.amount = total
                            .amount
                            .checked_add(*amount)
                            .ok_or_else(|| t!("Scheduled totals exceed the supported range."))?;
                        if due < total.first_due {
                            total.first_due = due.clone();
                        }
                    }
                }
                next_date = next(&due, &s.anchor_date, &s.frequency)?;
            }
        }
        Ok(Self { months })
    }

    pub fn amounts(&self, month: &str) -> HashMap<String, i64> {
        self.months
            .get(month)
            .into_iter()
            .flat_map(|items| items.iter())
            .map(|(id, item)| (id.clone(), item.amount))
            .collect()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    #[serde(default, flatten)]
    pub extra: serde_json::Map<String, Value>,
    pub id: String,
    pub next_date: Option<String>,
    pub anchor_date: String,
    pub frequency: String,
    #[serde(default)]
    pub paused: bool,
    pub transaction: Transaction,
}
pub fn read(b: &Budget) -> Result<Vec<Schedule>, String> {
    let Some(value) = b.extra.get("schedules") else { return Ok(vec![]); };
    if value.as_array().is_some_and(|rows| rows.len() > MAX_SCHEDULES) {
        return Err(t!("This budget exceeds the supported limit of 2,000 schedules."));
    }
    if value.as_array().is_some_and(|rows| rows.iter().any(|row|
        row.pointer("/transaction/splits").and_then(Value::as_array).is_some_and(|splits| splits.len() > 100))) {
        return Err(t!("A scheduled transaction exceeds the supported limit of 100 splits."));
    }
    // Deserialize from the borrowed JSON: do not clone the full template tree first.
    Vec::<Schedule>::deserialize(value).map_err(|e| t!("Invalid schedules: {error}", error = e))
}
fn date(s: &str) -> Result<NaiveDate, String> {
    if s.len() != 10 {
        return Err(t!("Invalid scheduled date."));
    }
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| t!("Invalid scheduled date."))?;
    if d.to_string() != s {
        return Err(t!("Invalid scheduled date."));
    }
    Ok(d)
}
pub fn next(current: &str, anchor: &str, frequency: &str) -> Result<Option<String>, String> {
    let d = date(current)?;
    let anchor = date(anchor)?;
    let months = match frequency {
        "never" => return Ok(None),
        "monthly" => 1,
        "everyOtherMonth" => 2,
        "every3Months" => 3,
        "every4Months" => 4,
        "twiceAYear" => 6,
        "yearly" => 12,
        "everyOtherYear" => 24,
        _ => 0,
    };
    let result = if months > 0 {
        let first = d
            .with_day(1)
            .unwrap()
            .checked_add_months(Months::new(months))
            .ok_or_else(|| t!("Schedule exceeds date range"))?;
        let last = first
            .checked_add_months(Months::new(1))
            .ok_or_else(|| t!("Schedule exceeds date range"))?
            - Duration::days(1);
        first.with_day(anchor.day().min(last.day())).unwrap()
    } else if frequency == "twiceAMonth" {
        let base = (anchor.day() - 1) % 15 + 1;
        let mut candidate = d.checked_add_signed(Duration::days(1)).ok_or_else(|| t!("Schedule exceeds date range"))?;
        loop {
            let month_end = candidate
                .with_day(1)
                .unwrap()
                .checked_add_months(Months::new(1))
                .ok_or_else(|| t!("Schedule exceeds date range"))?
                - Duration::days(1);
            if candidate.day() == base || candidate.day() == (base + 15).min(month_end.day()) {
                break candidate;
            }
            candidate = candidate.checked_add_signed(Duration::days(1)).ok_or_else(|| t!("Schedule exceeds date range"))?;
        }
    } else {
        d.checked_add_signed(Duration::days(match frequency {
            "daily" => 1,
            "weekly" => 7,
            "everyOtherWeek" => 14,
            "every4Weeks" => 28,
            _ => return Err(t!("Unknown repeat frequency.")),
        })).ok_or_else(|| t!("Schedule exceeds date range"))?
    };
    let result = result.to_string();
    date(&result)?;
    Ok(Some(result))
}
pub fn validate_schedules(b: &Budget) -> Result<(), String> {
    let mut ids = HashSet::new();
    let accounts: HashMap<_, _> = b.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let categories: HashSet<_> = b.categories.iter().filter(|c| c.payment_account().is_none()).map(|c| c.id.as_str()).collect();
    for s in read(b)? {
        if s.id.len() > 64 || s.id.is_empty() || !ids.insert(s.id.clone()) {
            return Err(t!("Invalid or duplicate schedule ID."));
        }
        next(
            s.next_date.as_deref().unwrap_or(&s.anchor_date),
            &s.anchor_date,
            &s.frequency,
        )?;
        let t = &s.transaction;
        if !accounts.contains_key(t.account_id.as_str())
            || t.transfer_account_id
                .as_ref()
                .is_some_and(|id| id == &t.account_id || !accounts.contains_key(id.as_str()))
        {
            return Err(t!("Schedule references a missing account."));
        }
        if t.splits.is_empty() && t.transfer_account_id.as_ref().is_some_and(|peer| {
            accounts[t.account_id.as_str()].on_budget() != accounts[peer.as_str()].on_budget()
        }) {
            return Err(t!("A scheduled transfer crossing the budget boundary needs a category or Unassigned split."));
        }
        if !(-1_000_000_000_000..=1_000_000_000_000).contains(&t.amount)
            || t.memo.len() > 5000
            || t.payee.len() > 200
            || t.splits.len() > 100
            || (t.transfer_account_id.is_none() && t.splits.is_empty())
            || (!t.splits.is_empty()
                && t.splits.iter().map(|x| x.amount as i128).sum::<i128>() != t.amount as i128)
            || t.splits.iter().any(|s| {
                !(-1_000_000_000_000..=1_000_000_000_000).contains(&s.amount)
                    || s.extra.get("transferAccountId").filter(|v| !v.is_null()).is_some_and(|v| !v.is_string())
                    || crate::ledger::transfer(s).is_some_and(|id| {
                        id == t.account_id
                            || t.transfer_account_id.is_some()
                            || !accounts.contains_key(id)
                    })
                    || s.category_id.as_ref()
                        .is_some_and(|id| !categories.contains(id.as_str()))
            })
        {
            return Err(t!("Invalid scheduled transaction details."));
        }
    }
    Ok(())
}
/// The transaction a schedule posts for the occurrence due on `due`, dated `date`.
pub fn occurrence(s: &Schedule, due: &str, date: &str) -> Transaction {
    let mut t = s.transaction.clone();
    // A new occurrence is a new payment, even when its template came from an imported one.
    t.extra.remove("bankImportIds");
    t.extra.remove("bankImportMatches");
    t.id = format!("scheduled:{}:{due}", s.id);
    t.date = date.to_string();
    t.status = Cleared::Uncleared;
    t.transfer_status = t.transfer_account_id.as_ref().map(|_| Cleared::Uncleared);
    for part in &mut t.splits {
        part.extra.remove("bankImportIds");
        if crate::ledger::transfer(part).is_some() {
            part.extra
                .insert("transferStatus".into(), json!("uncleared"));
        }
    }
    t.extra.insert("approved".into(), json!(false));
    t.extra.insert("scheduleId".into(), json!(s.id));
    t.extra.insert("scheduledDate".into(), json!(due));
    // It's added when it's entered (`apply`), not when the schedule, or the transaction moved
    // ahead into it, was.
    t.extra.remove("createdAt");
    t
}
pub fn apply(
    b: &mut Budget,
    through: &str,
    selected: Option<&str>,
    skip: bool,
) -> Result<usize, String> {
    date(through)?;
    validate_schedules(b)?;
    let mut schedules = read(b)?;
    let mut posted = vec![];
    let existing: HashSet<_> = b.transactions.iter().map(|t| t.id.as_str()).collect();
    let mut split_count: usize = b.transactions.iter().map(|t| t.splits.len()).sum();
    let mut ledger_entries: usize = b.transactions.iter().map(|t|
        if t.splits.iter().any(|s| crate::ledger::transfer(s).is_some()) { t.splits.len() } else { 1 }).sum();
    let mut occurrences = 0usize;
    let now = stamp_now();
    for s in &mut schedules {
        if selected.is_some_and(|id| id != s.id) || (selected.is_none() && s.paused) {
            continue;
        }
        let mut iterations = 0;
        while let Some(due) = s.next_date.clone() {
            if selected.is_none() && due.as_str() > through {
                break;
            }
            iterations += 1;
            occurrences += 1;
            if occurrences > MAX_OCCURRENCES {
                return Err(t!("More than 1,000,000 scheduled occurrences are due. Adjust the schedules' next dates first."));
            }
            if iterations > 10000 {
                return Err(t!("More than 10,000 occurrences are due. Adjust the schedule’s next date first."));
            }
            if !skip {
                let mut t = occurrence(s, &due, if selected.is_some() { through } else { &due });
                t.set_created_at(&now);
                if !existing.contains(t.id.as_str()) {
                    split_count = split_count.saturating_add(t.splits.len());
                    ledger_entries = ledger_entries.saturating_add(if t.splits.iter().any(|s| crate::ledger::transfer(s).is_some()) { t.splits.len() } else { 1 });
                    if b.transactions.len() + posted.len() >= 200_000
                        || split_count > MAX_TRANSACTION_SPLITS || ledger_entries > MAX_LEDGER_ENTRIES {
                        return Err(t!("Posting these schedules would exceed the supported transaction size. Adjust the schedules' next dates first."));
                    }
                    posted.push(t);
                }
            }
            s.next_date = next(&due, &s.anchor_date, &s.frequency)?;
            if selected.is_some() {
                break;
            }
        }
    }
    let count = posted.len();
    let ids = posted.iter().map(|t| t.id.clone()).collect();
    b.transactions.extend(posted);
    crate::loans::apply_new_payments(b, &ids)?;
    // A one-time schedule is done once it's entered or skipped: it leaves nothing to schedule.
    schedules.retain(|s| s.next_date.is_some());
    b.extra.insert("schedules".into(), json!(schedules));
    validate(b)?;
    Ok(count)
}

/// Keep long transaction IDs within the existing schedule limit without truncating their
/// identity. The counter only disambiguates an ID already used elsewhere in this budget.
fn future_schedule_id(transaction_id: &str, attempt: u32) -> String {
    let mut digest = Sha256::new();
    digest.update(b"urbudget:future-schedule:v1\0");
    digest.update(transaction_id.as_bytes());
    let base = format!("future:{}", URL_SAFE_NO_PAD.encode(digest.finalize()));
    // The visible suffix lets the UI recognize a prior allocation even if an earlier
    // collision was deleted. u32 keeps every candidate within 61 UTF-8 bytes.
    if attempt == 0 { base } else { format!("{base}:{attempt}") }
}

/// A transaction dated after `through` is a scheduled transaction: each one entered by hand
/// becomes a one-time schedule, entered on its date like any other, and leaves the ledger until
/// then. Imported ones (a bank's or YNAB's), ones already cleared, loan interest and entered
/// occurrences stay as they are. Returns how many moved.
pub fn schedule_future(b: &mut Budget, through: &str) -> Result<usize, String> {
    date(through)?;
    let uncleared = |t: &Transaction| {
        t.status == Cleared::Uncleared
            && t.transfer_status.is_none_or(|s| s == Cleared::Uncleared)
            && t.splits.iter().all(|s| {
                crate::ledger::transfer(s).is_none()
                    || crate::ledger::status(s) == Cleared::Uncleared
            })
    };
    let mut schedules = read(b)?;
    let mut scheduled_ids: HashSet<_> = schedules.iter().map(|s| s.id.clone()).collect();
    let mut templates: HashSet<_> = schedules.iter().map(|s| s.transaction.id.clone()).collect();
    let movable = |t: &Transaction, ids: &HashSet<String>, templates: &HashSet<String>| {
        t.date.as_str() > through && uncleared(t)
            && !t.extra.contains_key("bankImportIds") && !t.extra.contains_key("source")
            && !t.id.starts_with("scheduled:") && !crate::fresh_start::is_interest(t)
            && !ids.contains(&t.id) && (t.id.len() <= 64 || !templates.contains(&t.id))
    };
    // Preflight before taking any ledger records: a rejected conversion leaves the
    // caller's budget intact and never builds/serializes an oversized schedule tree.
    let additions = b.transactions.iter().filter(|t| movable(t, &scheduled_ids, &templates))
        .take(MAX_SCHEDULES - schedules.len() + 1).count();
    if schedules.len() + additions > MAX_SCHEDULES {
        return Err(t!("Converting future transactions would exceed the supported limit of 2,000 schedules."));
    }
    // Reserve ledger IDs too: a long ID processed first must not take the ID of a short
    // transaction that will be converted later. Existing short schedule IDs stay unchanged.
    let mut reserved: HashSet<_> = schedules.iter().map(|s| s.id.clone()).collect();
    reserved.extend(b.transactions.iter().map(|t| t.id.clone()));
    let mut moved = 0;
    let mut kept = Vec::with_capacity(b.transactions.len());
    for t in std::mem::take(&mut b.transactions) {
        if !movable(&t, &scheduled_ids, &templates) {
            kept.push(t);
            continue;
        }
        let id = if t.id.len() <= 64 {
            t.id.clone()
        } else {
            let mut attempt = 0;
            loop {
                let candidate = future_schedule_id(&t.id, attempt);
                if !reserved.contains(&candidate) {
                    break candidate;
                }
                attempt = attempt.checked_add(1).ok_or_else(|| t!("Too many schedule ID collisions."))?;
            }
        };
        reserved.insert(id.clone());
        scheduled_ids.insert(id.clone());
        templates.insert(t.id.clone());
        moved += 1;
        schedules.push(Schedule {
            extra: Default::default(),
            id,
            next_date: Some(t.date.clone()),
            anchor_date: t.date.clone(),
            frequency: "never".into(),
            paused: false,
            transaction: t,
        });
    }
    b.transactions = kept;
    if moved > 0 {
        b.extra.insert("schedules".into(), json!(schedules));
        validate_schedules(b)?;
        validate(b)?;
    }
    Ok(moved)
}

pub fn from_ynab(source: &Value, b: &Budget) -> Result<Vec<Schedule>, String> {
    let p = source
        .pointer("/data/plan")
        .or_else(|| source.pointer("/data/budget"))
        .ok_or_else(|| t!("Missing plan"))?;
    let accounts: HashMap<_, _> = b.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let mut scheduled_ids = HashSet::new();
    for row in p["scheduled_transactions"].as_array().into_iter().flatten().filter(|row| row["deleted"] != true) {
        let id = row["id"].as_str().ok_or_else(|| t!("Missing schedule ID"))?;
        if id.is_empty() || id.len() > 64 || !scheduled_ids.insert(id) {
            return Err(t!("Invalid or duplicate schedule ID."));
        }
        if scheduled_ids.len() > MAX_SCHEDULES {
            return Err(t!("This budget exceeds the supported limit of 2,000 schedules."));
        }
    }
    let mut subtransactions = HashMap::<&str, Vec<&Value>>::new();
    for sub in p["scheduled_subtransactions"].as_array().into_iter().flatten().filter(|row| row["deleted"] != true) {
        let parent = sub["scheduled_transaction_id"].as_str().filter(|id| scheduled_ids.contains(id))
            .ok_or_else(|| t!("A scheduled split references a missing parent transaction."))?;
        let parts = subtransactions.entry(parent).or_default();
        if parts.len() >= 100 {
            return Err(t!("A scheduled transaction exceeds the supported limit of 100 splits."));
        }
        parts.push(sub);
    }
    let payee = |id: &Value| {
        p["payees"]
            .as_array()
            .and_then(|ps| ps.iter().find(|v| v["id"] == *id))
            .and_then(|v| v["name"].as_str())
            .unwrap_or("")
            .to_string()
    };
    let categories: HashSet<&str> = b.categories.iter().map(|c| c.id.as_str()).collect();
    let internal_categories: HashSet<&str> = p["categories"].as_array().into_iter().flatten()
        .filter(|c| c["internal"] == true).filter_map(|c| c["id"].as_str()).collect();
    let category = |value: &Value| -> Result<Option<String>, String> {
        if value.is_null() { return Ok(None); }
        let id = value.as_str().ok_or_else(|| t!("Scheduled transaction has an invalid category reference."))?;
        if categories.contains(id) { return Ok(Some(id.to_owned())); }
        // Ready to Assign and other provider-internal categories have no envelope.
        // Retained source metadata restores their explicit role after import.
        if internal_categories.contains(id) { return Ok(None); }
        Err(t!("YNAB scheduled transaction references unknown category {id}.", id))
    };
    let money = |v: &Value| -> Result<i64, String> {
        let n = v.as_i64().ok_or_else(|| t!("Invalid scheduled amount"))?;
        if n % 10 != 0 {
            return Err(t!("Scheduled amount has sub-cent precision."));
        }
        let amount = n / 10;
        if !valid_money(amount) { return Err(t!("Scheduled amount exceeds the supported range.")); }
        Ok(amount)
    };
    let mut out = vec![];
    for s in p["scheduled_transactions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["deleted"] != true)
    {
        if out.len() >= MAX_SCHEDULES {
            return Err(t!("This budget exceeds the supported limit of 2,000 schedules."));
        }
        let id = s["id"].as_str().ok_or_else(|| t!("Missing schedule ID"))?.to_owned();
        if !s["transfer_account_id"].is_null() && !s["transfer_account_id"].is_string() {
            return Err(t!("Scheduled transfer has an invalid account reference."));
        }
        let transfer = s["transfer_account_id"].as_str().map(String::from);
        let account = s["account_id"].as_str().ok_or_else(|| t!("Missing schedule account"))?.to_owned();
        let source_account = accounts.get(account.as_str()).ok_or_else(|| t!("Schedule references a missing account."))?;
        if transfer.as_ref().is_some_and(|other| other == &account || !accounts.contains_key(other.as_str())) {
            return Err(t!("Scheduled transfer references a missing or identical account."));
        }
        let amount = money(&s["amount"])?;
        let mut splits = vec![];
        for sub in subtransactions.get(id.as_str()).into_iter().flatten() {
            if !sub["transfer_account_id"].is_null() && !sub["transfer_account_id"].is_string() {
                return Err(t!("Scheduled split transfer has an invalid account reference."));
            }
            if sub["transfer_account_id"].as_str().is_some_and(|other| other == account || !accounts.contains_key(other)) {
                return Err(t!("Scheduled split transfer references a missing or identical account."));
            }
            splits.push(Split {
                category_id: category(&sub["category_id"])?,
                amount: money(&sub["amount"])?,
                extra: serde_json::from_value(
                    json!({"id":sub["id"],"memo":sub["memo"],"payee":payee(&sub["payee_id"]),"transferAccountId":sub["transfer_account_id"],"transferStatus":"uncleared"}),
                )
                .unwrap(),
            });
        }
        let categorized = transfer.as_ref().is_none_or(|other| {
            source_account.on_budget() != accounts[other.as_str()].on_budget()
        });
        if splits.is_empty() && categorized {
            splits.push(Split {
                category_id: category(&s["category_id"])?,
                amount,
                extra: Default::default(),
            });
        }
        let first = s["date_first"]
            .as_str()
            .ok_or_else(|| t!("Missing first scheduled date"))?
            .to_owned();
        let next = s["date_next"]
            .as_str()
            .ok_or_else(|| t!("Missing next scheduled date"))?
            .to_owned();
        date(&first)?;
        date(&next)?;
        out.push(Schedule { extra: Default::default(),
            id: id.clone(),
            anchor_date: first,
            next_date: Some(next.clone()),
            frequency: s["frequency"]
                .as_str()
                .ok_or_else(|| t!("Missing frequency"))?
                .to_owned(),
            paused: false,
            transaction: Transaction {
                id,
                date: next,
                account_id: account,
                payee: payee(&s["payee_id"]),
                memo: s["memo"].as_str().unwrap_or("").into(),
                amount,
                status: Cleared::Uncleared,
                splits,
                transfer_status: transfer.as_ref().map(|_| Cleared::Uncleared),
                transfer_account_id: transfer,
                extra: serde_json::from_value(json!({"flag":s["flag_color"],"approved":false}))
                    .unwrap(),
            },
        });
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduled_monthly_requirements_cannot_exceed_safe_integer_totals() {
        let mut b = crate::domain::tests::sample();
        let mut transaction = crate::domain::tests::purchase();
        transaction.account_id = "cash".into();
        transaction.amount = 0;
        transaction.splits = (0..100).map(|i| Split { category_id: Some("food".into()),
            amount: if i < 50 { -1_000_000_000_000 } else { 1_000_000_000_000 }, extra: Default::default() }).collect();
        let schedules: Vec<_> = (0..6).map(|i| Schedule { extra: Default::default(), id: format!("large-{i}"),
            next_date: Some("2026-10-01".into()), anchor_date: "2026-10-01".into(),
            frequency: "daily".into(), paused: false, transaction: transaction.clone() }).collect();
        b.extra.insert("schedules".into(), json!(schedules));
        // Every template and split is valid; the derived monthly obligation is not.
        validate(&b).unwrap();
        assert!(upcoming(&b, "2026-10").unwrap_err().contains("safe numeric range"));
        assert!(calculate(&b, "2026-10").unwrap_err().contains("safe numeric range"));
    }

    #[test]
    fn schedule_count_and_range_are_checked_before_expanding_templates() {
        let mut b = crate::domain::tests::sample();
        b.extra.insert("schedules".into(), json!(vec![Value::Null; MAX_SCHEDULES + 1]));
        assert!(read(&b).unwrap_err().contains("2,000 schedules"));
        b.extra.insert("schedules".into(), json!([{"transaction":{"splits":vec![Value::Null;101]}}]));
        assert!(read(&b).unwrap_err().contains("100 splits"));
        b.extra.remove("schedules");
        assert!(UpcomingIndex::new(&b, "2026-1", "2026-10").is_err());
        assert!(UpcomingIndex::new(&b, "2026-10", "2026-09").is_err());
    }

    #[test]
    fn converting_too_many_future_transactions_preserves_the_original_ledger() {
        let mut b = crate::domain::tests::sample();
        b.transactions = (0..=MAX_SCHEDULES).map(|i| {
            let mut t = crate::domain::tests::purchase();
            t.id = format!("future-{i}"); t.date = "2026-11-01".into(); t.status = Cleared::Uncleared; t
        }).collect();
        let original = b.clone();
        assert!(schedule_future(&mut b, "2026-10-01").unwrap_err().contains("2,000 schedules"));
        assert_eq!(b, original);
    }

    #[test]
    fn imported_schedules_reject_bad_dates_accounts_and_amounts_at_the_boundary() {
        let b = crate::domain::tests::sample();
        let row = json!({"id":"schedule", "date_first":"2026-10-01", "date_next":"2026-10-02",
            "frequency":"monthly", "account_id":"cash", "amount":-50000, "category_id":"food"});
        for (field, value) in [("date_first", json!("ééééé")), ("date_next", json!("+262142-12-31")),
            ("account_id", json!("missing")), ("transfer_account_id", json!("missing")), ("transfer_account_id", json!(42)),
            ("amount", json!(i64::MIN))] {
            let mut invalid = row.clone(); invalid[field] = value;
            let source = json!({"data":{"plan":{"scheduled_transactions":[invalid]}}});
            assert!(from_ynab(&source, &b).is_err(), "{field}");
        }
        let source = json!({"data":{"plan":{"scheduled_transactions":[row]}}});
        assert_eq!(from_ynab(&source, &b).unwrap()[0].next_date.as_deref(), Some("2026-10-02"));
        let mut too_many_parts = source.clone();
        too_many_parts["data"]["plan"]["scheduled_subtransactions"] = json!(vec![json!({"scheduled_transaction_id":"schedule"});101]);
        assert!(from_ynab(&too_many_parts, &b).unwrap_err().contains("100 splits"));
        let mut missing_parent = source.clone();
        missing_parent["data"]["plan"]["scheduled_subtransactions"] = json!([{"scheduled_transaction_id":"missing"}]);
        assert!(from_ynab(&missing_parent, &b).unwrap_err().contains("missing parent"));
    }

    #[test]
    fn excessive_scheduled_transfer_expansion_fails_before_changing_the_budget() {
        let mut transaction = crate::domain::tests::purchase();
        transaction.amount = 0;
        transaction.splits = (0..100).map(|_| Split { category_id: None, amount: 0,
            extra: json!({"transferAccountId":"cash"}).as_object().unwrap().clone().into() }).collect();
        let mut b = scheduled(transaction);
        let mut schedules = read(&b).unwrap();
        schedules[0].next_date = Some("2000-01-01".into());
        schedules[0].anchor_date = "2000-01-01".into();
        schedules[0].frequency = "daily".into();
        b.extra.insert("schedules".into(), json!(schedules));
        let original = b.clone();
        assert!(apply(&mut b, "2015-01-01", None, false).unwrap_err().contains("supported transaction size"));
        assert_eq!(b, original);
    }

    fn scheduled(transaction: Transaction) -> Budget {
        let mut b = crate::domain::tests::sample();
        b.extra.insert("schedules".into(), json!([Schedule { extra: Default::default(),
            id: "test".into(),
            next_date: Some("2026-10-01".into()),
            anchor_date: "2026-10-01".into(),
            frequency: "never".into(),
            paused: false,
            transaction,
        }]));
        b
    }

    #[test]
    fn schedules_cannot_post_payment_category_expenses_or_unclassified_boundary_transfers() {
        let mut t = crate::domain::tests::purchase();
        t.splits[0].category_id = Some("payment".into());
        assert!(validate(&scheduled(t)).is_err());
        for reverse in [false, true] {
            let mut t = crate::domain::tests::purchase();
            t.account_id = if reverse { "tracking" } else { "cash" }.into();
            t.transfer_account_id = Some(if reverse { "cash" } else { "tracking" }.into());
            t.transfer_status = Some(Cleared::Uncleared);
            t.splits.clear();
            let mut b = scheduled(t);
            let mut tracking = b.accounts[0].clone();
            tracking.id = "tracking".into();
            tracking.kind = AccountKind::Tracking;
            b.accounts.push(tracking);
            assert!(validate(&b).is_err());
        }
    }
    #[test]
    fn invalid_schedule_amounts_and_splits_are_rejected_before_calculation() {
        let mut t = crate::domain::tests::purchase();
        t.amount = i64::MIN;
        t.splits[0].amount = i64::MIN;
        assert!(validate(&scheduled(t)).is_err());

        let mut t = crate::domain::tests::purchase();
        t.amount = 0;
        t.splits[0].amount = i64::MAX;
        t.splits.push(Split { amount: -i64::MAX, ..t.splits[0].clone() });
        assert!(validate(&scheduled(t)).is_err());

        let mut t = crate::domain::tests::purchase();
        t.amount = 0;
        t.splits[0].amount = 0;
        t.splits = vec![t.splits[0].clone(); 101];
        assert!(validate(&scheduled(t)).is_err());
    }

    #[test]
    fn schedule_dates_remain_canonical_and_within_the_transaction_date_range() {
        for current in ["2026-1-1", "+262142-12-31", "9999-12-31"] {
            assert!(next(current, current, "daily").is_err(), "{current}");
        }
        assert!(next("9999-12-01", "9999-12-01", "monthly").is_err());
        assert_eq!(next("9999-12-31", "9999-12-31", "never").unwrap(), None);
    }

    #[test]
    fn a_scheduled_transaction_is_added_when_it_is_entered() {
        // The template may carry the time of a transaction moved ahead into the schedule.
        let mut t = crate::domain::tests::purchase();
        t.set_created_at("2026-09-01T08:00:00.000Z");
        let mut b = scheduled(t);
        let before = stamp_now();
        apply(&mut b, "2026-10-01", None, false).unwrap();
        let entered = b.transactions.iter().find(|t| t.id == "scheduled:test:2026-10-01").unwrap();
        assert!(entered.created_at().is_some_and(|at| at >= before.as_str()), "{:?}", entered.created_at());
    }
    #[test]
    fn scheduled_occurrences_drop_imported_match_metadata_on_both_sides() {
        let mut t = crate::domain::tests::purchase();
        t.extra.insert("bankImportIds".into(), json!(["old-source", "old-destination"]));
        t.extra.insert("bankImportMatches".into(), json!({"card":"old-source"}));
        t.splits[0].category_id = None;
        t.splits[0].extra.insert("transferAccountId".into(), json!("cash"));
        t.splits[0].extra.insert("bankImportIds".into(), json!(["old-destination"]));
        let mut b = scheduled(t);
        apply(&mut b, "2026-10-01", None, false).unwrap();
        let entered = &b.transactions[0];
        assert!(!entered.extra.contains_key("bankImportIds") && !entered.extra.contains_key("bankImportMatches"));
        assert!(!entered.splits[0].extra.contains_key("bankImportIds"));
        let incoming = |amount| crate::bank_import::BankTransaction {
            fit: "new-payment".into(), date: "2026-10-01".into(), payee: "Transfer".into(), memo: String::new(), amount,
            transfer_account_id: None,
        };
        assert_eq!(crate::bank_import::review(&b, "card", vec![incoming(-5000)]).unwrap()[0].candidates, [entered.id.clone()]);
        let destination = crate::bank_import::review(&b, "cash", vec![incoming(5000)]).unwrap();
        assert_eq!(destination[0].candidate_details[0].transaction_id, entered.id);
    }
    #[test]
    fn upcoming_counts_only_transfers_that_spend_budget_money() {
        let mut t = crate::domain::tests::purchase();
        t.account_id = "tracking".into();
        t.amount = 5000;
        t.splits[0].amount = 5000;
        t.splits[0].extra.insert("transferAccountId".into(), json!("cash"));
        let mut b = scheduled(t);
        let mut tracking = b.accounts[0].clone();
        tracking.id = "tracking".into();
        tracking.kind = AccountKind::Tracking;
        b.accounts.push(tracking);
        validate(&b).unwrap();
        assert_eq!(upcoming(&b, "2026-10").unwrap().get("food"), Some(&5000));
        apply(&mut b, "2026-10-01", None, false).unwrap();
        assert_eq!(calculate(&b, "2026-10").unwrap().categories[0].activity, -5000);

        let mut t = crate::domain::tests::purchase();
        t.account_id = "cash".into();
        t.transfer_account_id = Some("card".into());
        t.transfer_status = Some(Cleared::Uncleared);
        let b = scheduled(t);
        validate(&b).unwrap();
        assert_eq!(upcoming(&b, "2026-10").unwrap().get("payment"), Some(&5000));
    }
    #[test]
    fn scheduled_card_payments_reduce_payment_funding_from_either_register() {
        for reversed in [false, true] {
            for representation in 0..3 {
                let mut t = crate::domain::tests::purchase();
                t.account_id = if reversed { "card" } else { "cash" }.into();
                let peer = if reversed { "cash" } else { "card" };
                let sign = if reversed { 1 } else { -1 };
                t.amount = sign * 20000;
                t.transfer_account_id = Some(peer.into());
                t.transfer_status = Some(Cleared::Uncleared);
                t.splits.clear();
                if representation == 1 {
                    // Legacy top-level transfers can retain a redundant ordinary-category split.
                    t.splits.push(Split { category_id: Some("food".into()), amount: t.amount, extra: Default::default() });
                } else if representation == 2 {
                    t.transfer_account_id = None;
                    t.transfer_status = None;
                    t.splits = [8000, 12000].into_iter().map(|amount| Split {
                        category_id: None,
                        amount: sign * amount,
                        extra: json!({"transferAccountId": peer}).as_object().unwrap().clone().into(),
                    }).collect();
                }
                let mut b = scheduled(t);
                b.assignments.push(Assignment { extra: Default::default(), month: "2026-10".into(), category_id: "payment".into(), amount: 5000 });
                validate(&b).unwrap();
                assert_eq!(upcoming(&b, "2026-10").unwrap(), std::collections::HashMap::from([("payment".into(), 20000)]),
                    "reversed={reversed}, representation={representation}");
                assert_eq!(upcoming_details(&b, "2026-10").unwrap()["payment"].first_due, "2026-10-01");
                let summary = calculate(&b, "2026-10").unwrap();
                let payment = summary.categories.iter().find(|c| c.id == "payment").unwrap();
                assert_eq!((payment.available, payment.upcoming, payment.needed), (5000, 20000, 15000));
            }
        }
    }
    #[test]
    fn mixed_scheduled_splits_count_each_expense_and_payment_once() {
        let mut t = crate::domain::tests::purchase();
        t.account_id = "cash".into();
        t.amount = -25000;
        t.splits = serde_json::from_value(json!([
            {"categoryId":"food", "amount":-3000},
            {"categoryId":null, "amount":-20000, "transferAccountId":"card"},
            {"categoryId":null, "amount":-2000, "transferAccountId":"savings"}
        ])).unwrap();
        let mut b = scheduled(t);
        let mut savings = b.accounts[0].clone();
        savings.id = "savings".into();
        b.accounts.push(savings);
        validate(&b).unwrap();
        assert_eq!(upcoming(&b, "2026-10").unwrap(), std::collections::HashMap::from([
            ("food".into(), 3000), ("payment".into(), 20000)
        ]));
    }
    #[test]
    fn cash_transfers_card_balance_transfers_and_cash_advances_are_not_scheduled_payments() {
        for (source, peer, amount) in [("cash", "savings", -5000), ("card", "other-card", -5000), ("card", "cash", -5000)] {
            let mut t = crate::domain::tests::purchase();
            t.account_id = source.into();
            t.amount = amount;
            t.splits.clear();
            t.transfer_account_id = Some(peer.into());
            t.transfer_status = Some(Cleared::Uncleared);
            let mut b = scheduled(t);
            let mut savings = b.accounts[0].clone();
            savings.id = "savings".into();
            b.accounts.push(savings);
            let mut card = b.accounts[1].clone();
            card.id = "other-card".into();
            b.accounts.push(card);
            let mut category = b.categories[1].clone();
            category.id = "other-payment".into();
            category.extra.insert("paymentAccountId".into(), json!("other-card"));
            b.categories.push(category);
            validate(&b).unwrap();
            assert!(upcoming(&b, "2026-10").unwrap().is_empty(), "{source}->{peer}");
        }
    }
    #[test]
    fn upcoming_metadata_uses_the_earliest_positive_obligation_and_all_remaining_occurrences() {
        let mut b = crate::domain::tests::sample();
        let mut schedules = vec![];
        for (id, due, amount, frequency, paused) in [
            ("weekly", "2026-10-03", -1000, "weekly", false),
            ("early", "2026-10-02", -2000, "never", false),
            ("inflow", "2026-10-01", 3000, "never", false),
            ("paused", "2026-10-01", -5000, "never", true),
            ("old", "2026-09-30", -5000, "never", false),
            ("future", "2026-11-01", -5000, "never", false),
        ] {
            let mut transaction = crate::domain::tests::purchase();
            transaction.account_id = "cash".into();
            transaction.amount = amount;
            transaction.splits[0].amount = amount;
            schedules.push(Schedule { extra: Default::default(),
                id: id.into(), next_date: Some(due.into()), anchor_date: due.into(),
                frequency: frequency.into(), paused, transaction,
            });
        }
        for reverse in [false, true] {
            if reverse { schedules.reverse(); }
            b.extra.insert("schedules".into(), json!(schedules));
            validate(&b).unwrap();
            assert_eq!(upcoming_details(&b, "2026-10").unwrap(), std::collections::HashMap::from([
                ("food".into(), Upcoming { amount: 7000, first_due: "2026-10-02".into() })
            ]));
        }
    }
    #[test]
    fn range_index_preserves_recurrence_phase_month_end_and_earliest_due() {
        let mut b = crate::domain::tests::sample();
        let schedules: Vec<_> = [
            ("weekly", "2024-01-31", "weekly", 100, false),
            ("monthly", "2024-01-31", "monthly", 2000, false),
            ("once", "2024-03-02", "never", 300, false),
            ("paused", "2024-02-01", "daily", 999, true),
        ].into_iter().map(|(id, due, frequency, amount, paused)| {
            let mut transaction = crate::domain::tests::purchase();
            transaction.account_id = "cash".into();
            transaction.amount = -amount;
            transaction.splits[0].amount = -amount;
            Schedule { extra: Default::default(), id: id.into(), next_date: Some(due.into()), anchor_date: due.into(),
                frequency: frequency.into(), paused, transaction }
        }).collect();
        b.extra.insert("schedules".into(), json!(schedules));
        let index = UpcomingIndex::new(&b, "2024-02", "2024-03").unwrap();
        assert_eq!(index.amounts("2024-02")["food"], 2400);
        assert_eq!(index.amounts("2024-03")["food"], 2700);
        assert_eq!(index.months["2024-02"]["food"].first_due, "2024-02-07");
        assert_eq!(index.months["2024-03"]["food"].first_due, "2024-03-02");
        assert!(index.amounts("2024-01").is_empty());
        assert!(index.amounts("2024-04").is_empty());
        for month in ["2024-02", "2024-03"] {
            assert_eq!(index.amounts(month), upcoming(&b, month).unwrap());
            assert_eq!(index.months[month], upcoming_details(&b, month).unwrap());
        }
    }

    #[test]
    fn month_end_anchor_survives_february_and_leap_years() {
        assert_eq!(
            next("2024-01-31", "2024-01-31", "monthly")
                .unwrap()
                .unwrap(),
            "2024-02-29"
        );
        assert_eq!(
            next("2024-02-29", "2024-01-31", "monthly")
                .unwrap()
                .unwrap(),
            "2024-03-31"
        );
        assert_eq!(
            next("2026-02-20", "2026-01-05", "twiceAMonth")
                .unwrap()
                .unwrap(),
            "2026-03-05"
        );
    }
    #[test]
    fn catch_up_is_idempotent_and_early_entry_keeps_anchor() {
        let mut b = Budget::default();
        b.accounts.push(Account {
            id: "a".into(),
            name: "Cash".into(),
            kind: AccountKind::Cash,
            opening_balance: 10000,
            archived: false,
            extra: Default::default(),
        });
        let t = Transaction {
            id: "s".into(),
            date: "2026-01-31".into(),
            account_id: "a".into(),
            payee: "Rent".into(),
            memo: "".into(),
            amount: -100,
            status: Cleared::Cleared,
            splits: vec![Split {
                category_id: None,
                amount: -100,
                extra: Default::default(),
            }],
            transfer_account_id: None,
            transfer_status: None,
            extra: Default::default(),
        };
        b.extra.insert(
            "schedules".into(),
            json!([Schedule { extra: Default::default(),
                id: "s".into(),
                next_date: Some("2026-01-31".into()),
                anchor_date: "2026-01-31".into(),
                frequency: "monthly".into(),
                paused: false,
                transaction: t
            }]),
        );
        assert_eq!(apply(&mut b, "2026-03-01", None, false).unwrap(), 2);
        assert_eq!(apply(&mut b, "2026-03-01", None, false).unwrap(), 0);
        assert_eq!(apply(&mut b, "2026-03-15", Some("s"), false).unwrap(), 1);
        assert_eq!(
            read(&b).unwrap()[0].next_date.as_deref(),
            Some("2026-04-30")
        );
        assert_eq!(b.transactions.last().unwrap().status, Cleared::Uncleared);
    }

    fn cash_budget(transactions: Vec<Transaction>) -> Budget {
        let mut b = Budget::default();
        b.accounts.push(Account {
            id: "a".into(),
            name: "Cash".into(),
            kind: AccountKind::Cash,
            opening_balance: 10000,
            archived: false,
            extra: Default::default(),
        });
        b.transactions = transactions;
        b
    }
    fn spend(id: &str, date: &str) -> Transaction {
        Transaction {
            id: id.into(),
            date: date.into(),
            account_id: "a".into(),
            payee: "Rent".into(),
            memo: "October".into(),
            amount: -100,
            status: Cleared::Uncleared,
            splits: vec![Split { category_id: None, amount: -100, extra: Default::default() }],
            transfer_account_id: None,
            transfer_status: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn future_transactions_become_one_time_schedules_entered_on_their_date() {
        let mut imported = spend("bank", "2026-11-02");
        imported.extra.insert("bankImportIds".into(), json!(["fit-1"]));
        let mut cleared = spend("cleared", "2026-11-03");
        cleared.status = Cleared::Cleared;
        let mut b = cash_budget(vec![
            spend("past", "2026-10-19"),
            spend("ahead", "2026-10-24"),
            imported,
            cleared,
            spend("loan-interest:a:2026-10", "2026-10-31"),
        ]);
        assert_eq!(schedule_future(&mut b, "2026-10-19").unwrap(), 1);
        let ids: Vec<_> = b.transactions.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["past", "bank", "cleared", "loan-interest:a:2026-10"]);
        let s = &read(&b).unwrap()[0];
        assert_eq!((s.id.as_str(), s.frequency.as_str(), s.next_date.as_deref()), ("ahead", "never", Some("2026-10-24")));
        assert_eq!(s.transaction.memo, "October");
        // Nothing more to move; on its date it's entered, and the one-time schedule is done.
        assert_eq!(schedule_future(&mut b, "2026-10-19").unwrap(), 0);
        assert_eq!(apply(&mut b, "2026-10-23", None, false).unwrap(), 0);
        assert_eq!(apply(&mut b, "2026-10-24", None, false).unwrap(), 1);
        assert!(read(&b).unwrap().is_empty());
        let entered = b.transactions.last().unwrap();
        assert_eq!((entered.id.as_str(), entered.date.as_str()), ("scheduled:ahead:2026-10-24", "2026-10-24"));
    }

    #[test]
    fn skipping_a_one_time_schedule_removes_it() {
        let mut b = cash_budget(vec![spend("ahead", "2026-10-24")]);
        schedule_future(&mut b, "2026-10-19").unwrap();
        assert_eq!(apply(&mut b, "2026-10-19", Some("ahead"), true).unwrap(), 0);
        assert!(read(&b).unwrap().is_empty());
        assert!(b.transactions.is_empty());
    }

    #[test]
    fn long_future_transaction_ids_do_not_block_due_posting_or_repeat_after_reopen() {
        let ids = ["a".repeat(65), "a".repeat(100), "é".repeat(50)];
        let mut b = cash_budget(ids.iter().map(|id| spend(id, "2026-10-24")).collect());
        b.extra.insert("schedules".into(), json!([Schedule { extra: Default::default(),
            id: "due".into(),
            next_date: Some("2026-10-19".into()),
            anchor_date: "2026-10-19".into(),
            frequency: "never".into(),
            paused: false,
            transaction: spend("due", "2026-10-19"),
        }]));
        validate(&b).unwrap(); // All three original transaction IDs are valid.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = crate::store::Store::open(&path).unwrap();
        let token = store.load().unwrap().token;
        let saved = store.save(&b, &token).unwrap();

        let moved = store.run_schedules(&saved.token, "2026-10-19").unwrap();
        assert_eq!(moved.budget.transactions.len(), 1);
        assert_eq!(moved.budget.transactions[0].id, "scheduled:due:2026-10-19");
        let schedules = read(&moved.budget).unwrap();
        assert_eq!(schedules.len(), ids.len());
        assert!(schedules.iter().all(|s| s.id.len() <= 64));
        // Shared UTF-8/SHA-256 vectors keep native and frontend allocation compatible.
        assert_eq!(schedules[0].id, "future:awdQEhj_lDM6uJk9a71eziPA7a5EZVn-g4SnZwGW1h0");
        assert_eq!(schedules[2].id, "future:bYms6xZpBuJ5kqY4kWL2cOFPOhsXuHRmePyJnZTjakY");
        let original_ids: Vec<_> = schedules.iter().map(|s| s.transaction.id.clone()).collect();
        assert_eq!(original_ids, ids);
        assert_eq!(schedules.iter().map(|s| &s.id).collect::<HashSet<_>>().len(), ids.len());
        let repeated = store.run_schedules(&moved.token, "2026-10-19").unwrap();
        assert_eq!(repeated.token, moved.token);
        assert_eq!(repeated.budget, moved.budget);

        drop(store);
        let mut store = crate::store::Store::open(&path).unwrap();
        let reopened = store.load().unwrap();
        assert_eq!(reopened.budget, moved.budget);
        let posted = store.run_schedules(&reopened.token, "2026-10-24").unwrap();
        assert!(read(&posted.budget).unwrap().is_empty());
        assert_eq!(posted.budget.transactions.len(), ids.len() + 1);
        assert!(posted.budget.transactions.iter().all(|t| t.id.len() <= 100));
        let repeated = store.run_schedules(&posted.token, "2026-10-24").unwrap();
        assert_eq!(repeated.token, posted.token);
        assert_eq!(repeated.budget, posted.budget);
    }

    #[test]
    fn long_future_ids_preserve_colliding_schedules_and_short_transactions() {
        let long_id = "x".repeat(100);
        assert!(future_schedule_id(&long_id, u32::MAX).len() <= 64);
        let occupied_schedule = future_schedule_id(&long_id, 0);
        let short_transaction = future_schedule_id(&long_id, 1);
        let mut b = cash_budget(vec![
            spend(&long_id, "2026-10-24"),
            spend(&short_transaction, "2026-10-24"),
        ]);
        b.extra.insert("schedules".into(), json!([Schedule { extra: Default::default(),
            id: occupied_schedule.clone(),
            next_date: Some("2026-10-24".into()),
            anchor_date: "2026-10-24".into(),
            frequency: "never".into(),
            paused: false,
            transaction: spend("existing", "2026-10-24"),
        }]));
        let mut reversed = b.clone();
        reversed.transactions.reverse();
        for budget in [&mut b, &mut reversed] {
            assert_eq!(schedule_future(budget, "2026-10-19").unwrap(), 2);
            let schedules = read(budget).unwrap();
            assert_eq!(schedules.len(), 3);
            let existing = schedules.iter().find(|s| s.id == occupied_schedule).unwrap();
            assert_eq!(existing.transaction.id, "existing");
            let short = schedules.iter().find(|s| s.id == short_transaction).unwrap();
            assert_eq!(short.transaction.id, short_transaction);
            let long = schedules.iter().find(|s| s.transaction.id == long_id).unwrap();
            assert_ne!(long.id, occupied_schedule);
            assert_ne!(long.id, short_transaction);
            assert!(long.id.len() <= 64);
            assert_eq!(schedule_future(budget, "2026-10-19").unwrap(), 0);
        }
        // Reordering the ledger cannot change the identity of the mapped schedule.
        let mapped_id = |budget: &Budget| read(budget).unwrap().into_iter()
            .find(|s| s.transaction.id == long_id).unwrap().id;
        assert_eq!(mapped_id(&b), mapped_id(&reversed));
        assert_eq!(apply(&mut b, "2026-10-24", None, false).unwrap(), 3);
        assert_eq!(b.transactions.len(), 3);
        assert_eq!(b.transactions.iter().map(|t| &t.id).collect::<HashSet<_>>().len(), 3);
        assert_eq!(apply(&mut b, "2026-10-24", None, false).unwrap(), 0);
    }
}
