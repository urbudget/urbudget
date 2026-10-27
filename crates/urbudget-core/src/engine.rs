//! Integer-money envelope accounting. Cash overspending has priority over credit;
//! funded card spending reserves cash in the corresponding payment category.
use crate::t;
use crate::domain::*;
use chrono::{Datelike, Months, NaiveDate};
use std::collections::{BTreeMap, HashMap};

pub fn calculate(b: &Budget, month: &str) -> Result<Summary, String> {
    calculate_at(b, month, &calculation_month(b))
}
pub fn calculate_at(b: &Budget, month: &str, as_of: &str) -> Result<Summary, String> {
    let summary = calculate_range_at(b, month, as_of, false)?
        .remove(month)
        .ok_or_else(|| t!("Could not calculate budget."))?;
    hold_back_future(b, month, as_of, summary)
}
/// `month`'s summary as the budget screen shows it, from the one `calculate_history` keeps.
/// Like YNAB's budget screen, every month (past ones too) holds back what later months assign.
pub(crate) fn hold_back_future(b: &Budget, month: &str, as_of: &str, mut summary: Summary) -> Result<Summary, String> {
    let mut future_months = BTreeMap::<&str, i64>::new();
    for a in b.assignments.iter().filter(|a| a.month.as_str() > month) {
        *future_months.entry(&a.month).or_default() += a.amount;
    }
    let mut future = 0;
    for (m, assigned) in future_months {
        future += assigned;
        // A later negative assignment must not hide an earlier month's shortage.
        // Future transactions are deliberately excluded: that money is not here yet.
        // Before the calculation month, later months' own income already funds them.
        let shortage = future - summary.ready_to_assign;
        if month >= as_of && shortage > 0 {
            summary.future_shortfalls.push(FutureShortfall { month: m.into(), amount: shortage });
        }
    }
    summary.future_assigned = future;
    summary.future_ready_to_assign = summary.ready_to_assign - future;
    summary.ready_to_assign = net_of_future(summary.ready_to_assign, future);
    summary.validate_numeric_range()?;
    Ok(summary)
}
/// Ready to Assign once what later months already have assigned (`future`) is held back from
/// `ready` (before that). `calculate` reports both: `future_ready_to_assign + future_assigned`
/// is `ready`.
pub fn net_of_future(ready: i64, future: i64) -> i64 {
    if future > 0 && ready >= 0 {
        (ready - future).max(0)
    } else {
        ready
    }
}

/// Global assignment must leave every future month's existing assignments funded,
/// even if a later month returns some of that money. Historical edits are limited to
/// the Ready to Assign the month shows, net of what later months assign.
pub fn assignment_capacity(b: &Budget, month: &str, summary: &Summary) -> i64 {
    if month < calculation_month(b).as_str() { return summary.ready_to_assign.max(0); }
    let mut months = BTreeMap::<&str, i64>::new();
    for a in b.assignments.iter().filter(|a| a.month.as_str() > month) {
        *months.entry(&a.month).or_default() += a.amount;
    }
    let mut assigned = 0;
    let mut peak = 0;
    for amount in months.values() { assigned += amount; peak = peak.max(assigned); }
    (summary.future_ready_to_assign + summary.future_assigned - peak).max(0)
}

pub fn calculate_history(b: &Budget, month: &str) -> Result<BTreeMap<String, Summary>, String> {
    calculate_history_at(b, month, &calculation_month(b))
}

pub(crate) fn calculation_month(b: &Budget) -> String {
    b.extra.get("calculationAsOfMonth")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .unwrap_or_else(current_month)
}

/// The device calendar month, without any imported or preview budget context.
pub fn current_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

fn calculate_history_at(b: &Budget, month: &str, as_of: &str) -> Result<BTreeMap<String, Summary>, String> {
    calculate_range_at(b, month, as_of, true)
}

/// Both modes roll the same accounting state forward. The selected-month path only
/// builds the requested summary; targets retain compact prior-month availability.
fn calculate_range_at(b: &Budget, month: &str, as_of: &str, retain_history: bool) -> Result<BTreeMap<String, Summary>, String> {
    validate(b)?;
    let end = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| t!("Invalid budget month."))?;
    if !valid_month(month) || !valid_month(as_of) {
        return Err(t!("Invalid budget month."));
    }
    let na = b.accounts.len();
    let nc = b.categories.len();
    let credit_accounts: Vec<usize> = b.accounts.iter().enumerate()
        .filter(|(_, a)| a.kind == AccountKind::Credit).map(|(i, _)| i).collect();
    let nk = credit_accounts.len();
    let mut first = month.to_string();
    for t in &b.transactions {
        if t.date[..7] < first[..] { first = t.date[..7].into(); }
    }
    for a in &b.assignments {
        if a.month < first { first = a.month.clone(); }
    }
    let mut date = NaiveDate::parse_from_str(&format!("{first}-01"), "%Y-%m-%d")
        .map_err(|_| t!("Invalid start month"))?;
    if end.year() - date.year() > 200 {
        return Err(t!("Budget history exceeds 200 years."));
    }
    let months = ((end.year() - date.year()) * 12 + end.month() as i32 - date.month() as i32 + 1) as usize;
    validate_history_workload(months, nc, na, nk)?;
    let ledger = crate::ledger::transactions(b);
    let ai: HashMap<&str, usize> = b
        .accounts
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id.as_str(), i))
        .collect();
    let ci: HashMap<&str, usize> = b
        .categories
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.as_str(), i))
        .collect();
    let mut credit_column = vec![None; na];
    for (column, account) in credit_accounts.iter().enumerate() { credit_column[*account] = Some(column); }
    let mut payment = vec![None; nk];
    let mut payment_account = vec![None; nc];
    for (category, c) in b.categories.iter().enumerate() {
        if let Some(account) = c.payment_account().and_then(|id| ai.get(id)).copied() {
            if let Some(column) = credit_column[account] {
                payment[column] = Some(category);
                payment_account[category] = Some(account);
            }
        }
    }
    let mut working: Vec<i64> = b.accounts.iter().map(|a| a.opening_balance).collect();
    let mut cleared = working.clone();
    for t in &ledger {
        let i = ai[t.account_id.as_str()];
        working[i] += t.amount;
        if t.status != Cleared::Uncleared {
            cleared[i] += t.amount
        }
        if let Some(other) = &t.transfer_account_id {
            let j = ai[other.as_str()];
            working[j] -= t.amount;
            if t.transfer_status != Some(Cleared::Uncleared) {
                cleared[j] -= t.amount
            }
        }
    }
    let account_summaries: Vec<AccountSummary> = b.accounts.iter().enumerate()
        .map(|(i, a)| AccountSummary { id: a.id.clone(), balance: working[i],
            cleared: cleared[i], uncleared: working[i] - cleared[i] }).collect();
    let net_worth = working.iter().zip(&b.accounts).filter(|(_, a)| a.in_net_worth())
        .map(|(n, _)| n).sum();
    let upcoming_index = if retain_history {
        Some(crate::schedules::UpcomingIndex::new(b, &first, month)?)
    } else { None };
    let mut balance: Vec<i64> = b.accounts.iter().map(|a| a.opening_balance).collect();
    let mut carry = vec![0i64; nc];
    let assignment_index = crate::targets::AssignmentIndex::new(b);
    let mut assignments_by_month: HashMap<&str, Vec<&Assignment>> = HashMap::new();
    for a in &b.assignments { assignments_by_month.entry(&a.month).or_default().push(a); }
    // Read imported/native creation metadata once, not again for every tied-date
    // comparison during the ledger sort.
    let mut ordered: Vec<_> = ledger.iter().enumerate().map(|(i, t)| (i, t, t.created_at())).collect();
    // Native entries retain their creation order even if the stored array is reordered.
    // Legacy/API entries have no time: preserve their existing newest-first fallback.
    ordered.sort_by(|(ix, x, created_x), (iy, y, created_y)| x.date.cmp(&y.date).then_with(|| {
        match (created_x, created_y) {
            (None, None) => iy.cmp(ix),
            (a, b) => a.cmp(b).then_with(|| ix.cmp(iy)),
        }
    }));
    let mut result = BTreeMap::new();
    let mut available_history = HashMap::<String, Vec<i64>>::new();
    let retain_target_history = b.categories.iter().any(|c| c.extra.get("goal")
        .is_some_and(|g| g["goal_needs_whole_amount"].as_bool() == Some(false)));
    let mut cursor = 0;
    let mut assigned = vec![0i64; nc];
    let mut cash = vec![0i64; nc];
    // Cash and tracking accounts never need a credit funding column. Reuse the
    // monthly work buffers, including each category's purchase allocation storage.
    let mut credit = vec![0i64; nc * nk];
    let mut credit_refunds = vec![0i64; nc * nk];
    let mut credit_purchases: Vec<Vec<(usize, i64)>> = vec![vec![]; nc];
    let mut activity = vec![0i64; nc];
    let mut overspent = vec![0i64; nc];
    let mut available = vec![0i64; nc];
    let mut positive = vec![0i64; nk];
    while date <= end {
        let m = date.format("%Y-%m").to_string();
        assigned.fill(0);
        cash.fill(0);
        credit.fill(0);
        credit_refunds.fill(0);
        for purchases in &mut credit_purchases { purchases.clear(); }
        activity.fill(0);
        overspent.fill(0);
        let mut income = 0i64;
        // Spending this month with no category (negative): YNAB's Uncategorized row. Of it,
        // what cards spent beyond their own cash (positive), which is debt rather than cash.
        let mut uncategorized = 0i64;
        let mut uncategorized_credit = 0i64;
        for a in assignments_by_month.get(m.as_str()).into_iter().flatten() {
                assigned[ci[a.category_id.as_str()]] += a.amount
        }
        let mut day = "";
        let start = cursor;
        while cursor < ordered.len() && ordered[cursor].1.date[..7] == m { cursor += 1; }
        for (_, t, _) in &ordered[start..cursor] {
            if day != t.date {
                // API dates have no intraday posting order. Refresh spendable
                // card cash from the previous day's closing balance, so a
                // same-day payment/refund cannot create a temporary cash pool.
                for (column, i) in credit_accounts.iter().enumerate() {
                    positive[column] = balance[*i].max(0);
                }
                day = &t.date;
            }
            let i = ai[t.account_id.as_str()];
            let a = &b.accounts[i];
            balance[i] += t.amount;
            let (budget_account, direction) = if let Some(other) = &t.transfer_account_id {
                let j = ai[other.as_str()];
                let o = &b.accounts[j];
                balance[j] -= t.amount;
                if a.on_budget() && o.on_budget() {
                    for (idx, amount) in [(i, t.amount), (j, -t.amount)] {
                        if let Some(column) = credit_column[idx] {
                            if amount < 0 {
                                // Transferring a positive card balance spends that cash too.
                                positive[column] = (positive[column] + amount).max(0);
                            } else if a.kind != AccountKind::Credit || o.kind != AccountKind::Credit {
                                // Card-to-card balance transfers move debt; their payment
                                // envelopes are reassigned manually, as in YNAB.
                                if let Some(c) = payment[column] {
                                    activity[c] -= amount
                                }
                            }
                        }
                    }
                    continue;
                }
                if a.on_budget() == o.on_budget() {
                    continue;
                }
                // Categorization is stored in the source side's sign, even when
                // the source is a tracking account. Only the budget side counts.
                if a.on_budget() { (i, 1) } else { (j, -1) }
            } else {
                if !a.on_budget() {
                    continue;
                }
                (i, 1)
            };
            let i = budget_account;
            let column = credit_column[i];
            for s in &t.splits {
                let amount = s.amount * direction;
                // Consume positive card cash for every outflow, including uncategorized
                // ones, so a later purchase cannot spend the same cash a second time.
                let mut cash_part = 0;
                if amount < 0 {
                    if let Some(column) = column {
                        cash_part = -positive[column].min(-amount);
                        positive[column] += cash_part;
                    }
                }
                if let Some(cat) = s.category_id.as_deref() {
                    let c = ci[cat];
                    if let Some(column) = column {
                        let cell = c * nk + column;
                        cash[c] += cash_part;
                        credit[cell] += amount - cash_part;
                        if amount - cash_part < 0 {
                            credit_purchases[c].push((column, -(amount - cash_part)));
                        } else {
                            credit_refunds[cell] += amount - cash_part;
                        }
                    } else {
                        cash[c] += amount
                    }
                } else {
                    match crate::ledger::classify(t, s, amount) {
                        crate::ledger::Flow::Income => income += amount,
                        // Spending with no category is overspending, as YNAB shows it: its cash
                        // comes out of Ready to Assign the month after, like any overspent category.
                        // On a card only the card's own positive cash is spent; the rest is debt.
                        crate::ledger::Flow::Uncategorized => {
                            uncategorized += amount;
                            if column.is_some() { uncategorized_credit -= amount - cash_part }
                        }
                        _ => {}
                    }
                }
            }
        }
        for c in 0..nc {
            if b.categories[c].payment_account().is_some() {
                continue;
            }
            let base = c * nk;
            let credit_row = &credit[base..base + nk];
            let refunds = &mut credit_refunds[base..base + nk];
            activity[c] = cash[c] + credit_row.iter().sum::<i64>();
            let mut funded = carry[c] + assigned[c] + cash[c];
            // Net refunds release their card's reserve before funding new purchases. Purchases
            // then consume the remaining cash in ledger order, never in sidebar account order.
            for (a, v) in credit_row.iter().enumerate().filter(|(_, v)| **v > 0) {
                if let Some(p) = payment[a] {
                    activity[p] -= v
                }
                funded += v;
            }
            for (a, amount) in &credit_purchases[c] {
                // A refund cancels the earliest purchases on its card first. Merely capping
                // total spending would reserve cash for refunded purchases ahead of another
                // card's still-outstanding purchases.
                let refunded = (*amount).min(refunds[*a]);
                refunds[*a] -= refunded;
                let spent = *amount - refunded;
                let reserved = funded.max(0).min(spent);
                if let Some(p) = payment[*a] { activity[p] += reserved; }
                funded -= reserved;
            }
            let available = carry[c] + assigned[c] + activity[c];
            let all_overspend = (-available).max(0);
            let cash_overspend = (-(carry[c] + assigned[c] + cash[c]))
                .max(0)
                .min(all_overspend);
            overspent[c] = all_overspend - cash_overspend;
        }
        // Excess payment reserves remain assigned until the user moves them. A positive
        // card balance is not a transaction and must not manufacture monthly activity.
        for c in 0..nc { available[c] = carry[c] + assigned[c] + activity[c]; }
        let total_available = available.iter().sum::<i64>();
        let cash_on_hand: i64 = b.accounts.iter().enumerate().filter(|(_, a)| a.on_budget())
            .map(|(i, a)| if a.kind == AccountKind::Credit { balance[i].max(0) } else { balance[i] }).sum();
        // Cash overspending, uncategorized spending's included, is held until next month.
        let ready = cash_on_hand - total_available - (uncategorized + uncategorized_credit) - overspent.iter().sum::<i64>();
        if retain_history || date == end {
            let upcoming = match &upcoming_index {
                Some(index) => index.amounts(&m),
                None => crate::schedules::upcoming(b, &m)?,
            };
            let categories: Vec<_> = b
            .categories
            .iter()
            .enumerate()
            .map(|(c, cat)| {
                let available = available[c];
                let previous_available = |previous: &str| available_history.get(previous).map(|values| values[c]).unwrap_or(0);
                // Without an explicit target, YNAB's Underfunded pays off the uncovered
                // card debt. Use this month's balance, excluding later posted entries.
                let payment_balance = if cat.target == 0
                    && cat.extra.get("goal").is_none_or(serde_json::Value::is_null)
                {
                    payment_account[c].map(|i| (-balance[i]).max(0))
                } else { None };
                let balance_target = payment_balance.or_else(||
                    cat.extra.get("goal").is_some_and(|g| g["goal_type"] == "TB").then_some(cat.target));
                let target_needed = if let Some(required) = payment_balance {
                    (required - available).max(0)
                } else if crate::targets::undated_balance(cat) {
                    // A balance with no date asks for nothing this month, as YNAB's underfunded
                    // amount says. What's left of it is target_funding_required - _funded.
                    0
                } else {
                    // Explicit targets still obey their snooze/creation rules.
                    crate::targets::needed_with_available(cat, &m, assigned[c], available, carry[c], previous_available, &assignment_index, as_of)
                };
                let target_funding_required = if let Some(required) = balance_target { required } else {
                    crate::targets::needed_with_available(cat, &m, 0, available - assigned[c], carry[c], previous_available, &assignment_index, as_of)
                };
                let target_funding_funded = if let Some(required) = balance_target { available.max(0).min(required) } else {
                    (target_funding_required - target_needed).clamp(0, target_funding_required)
                };
                // The month's cost, so neither carryover nor a card's balance counts: an undated
                // balance has no monthly amount, and a refill asks for all of it.
                let month_target = if crate::targets::undated_balance(cat) { 0 } else {
                    crate::targets::needed_with_available(cat, &m, 0, available - assigned[c], 0, |_| 0, &assignment_index, as_of)
                };
                CategorySummary {
                    id: cat.id.clone(),
                    assigned: assigned[c],
                    activity: activity[c],
                    available,
                    carried: carry[c],
                    credit_overspending: overspent[c],
                    upcoming: *upcoming.get(&cat.id).unwrap_or(&0),
                    target_funding_required,
                    target_funding_funded,
                    month_target,
                    target_due: crate::targets::target_due(cat, &m),
                    needed: target_needed
                    .max((upcoming.get(&cat.id).copied().unwrap_or(0) - available).max(0)),
                }
            })
            .collect();
            let summary = Summary {
                    ready_to_assign: ready,
                    future_assigned: 0,
                    future_ready_to_assign: ready,
                    future_shortfalls: vec![],
                    assigned: assigned.iter().sum(),
                    activity: activity.iter().sum(),
                    available: total_available,
                    income,
                    uncategorized,
                    uncategorized_credit,
                    net_worth,
                    accounts: account_summaries.clone(),
                    categories,
                    imported_snapshot: false,
                };
            summary.validate_numeric_range()?;
            result.insert(m.clone(), summary);
        }
        for c in 0..nc { carry[c] = available[c].max(0); }
        if retain_target_history { available_history.insert(m, carry.clone()); }
        date = date
            .checked_add_months(Months::new(1))
            .ok_or_else(|| t!("Invalid month"))?;
    }
    Ok(result)
}

#[cfg(test)]
mod review_regressions {
    use super::*;

    #[test]
    fn later_negative_assignments_do_not_hide_intermediate_future_shortfalls() {
        for returned in [-50_000, -150_000] {
            let mut b = crate::domain::tests::sample();
            b.extra.insert("calculationAsOfMonth".into(), serde_json::json!("2026-10"));
            b.assignments = vec![
                Assignment { extra: Default::default(), month: "2026-11".into(), category_id: "food".into(), amount: 150_000 },
                Assignment { extra: Default::default(), month: "2026-12".into(), category_id: "food".into(), amount: returned },
            ];
            // Recorded future income must not justify assigning money we do not hold yet.
            let mut income = crate::domain::tests::purchase();
            income.account_id = "cash".into(); income.date = "2026-11-01".into();
            income.amount = 100_000; income.splits[0].amount = income.amount;
            income.splits[0].category_id = None;
            b.transactions.push(income);
            let s = calculate_at(&b, "2026-10", "2026-10").unwrap();
            assert_eq!(s.future_assigned, 150_000 + returned);
            assert_eq!(s.future_shortfalls.len(), 1);
            assert_eq!((s.future_shortfalls[0].month.as_str(), s.future_shortfalls[0].amount), ("2026-11", 50_000));
            assert_eq!(assignment_capacity(&b, "2026-10", &s), 0);
            b.accounts[0].opening_balance += 50_000;
            let funded = calculate_at(&b, "2026-10", "2026-10").unwrap();
            assert!(funded.future_shortfalls.is_empty());
            assert_eq!(assignment_capacity(&b, "2026-10", &funded), 0);
            b.accounts[0].opening_balance += 10_000;
            assert_eq!(assignment_capacity(&b, "2026-10", &calculate_at(&b, "2026-10", "2026-10").unwrap()), 10_000);
        }
    }

    #[test]
    fn past_months_hold_back_later_assignments_like_ynab() {
        // YNAB's budget screen nets every month, not only the calculation month and later. The
        // history stays un-netted, as the import audit compares it with YNAB's API figure.
        let mut b = sample();
        b.extra.insert("calculationAsOfMonth".into(), json!("2026-10"));
        b.assignments = vec![
            Assignment { extra: Default::default(), month: "2026-08".into(), category_id: "food".into(), amount: 30_000 },
            Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "food".into(), amount: -50_000 },
            Assignment { extra: Default::default(), month: "2026-10".into(), category_id: "food".into(), amount: 15_000 },
        ];
        let history = calculate_history_at(&b, "2026-10", "2026-10").unwrap();
        let ready = |month: &str| history[month].ready_to_assign;
        assert!(ready("2026-09") > 15_000);
        let september = calculate_at(&b, "2026-09", "2026-10").unwrap();
        assert_eq!((september.ready_to_assign, september.future_assigned), (ready("2026-09") - 15_000, 15_000));
        assert_eq!(september.future_ready_to_assign + september.future_assigned, ready("2026-09"));
        assert_eq!(assignment_capacity(&b, "2026-09", &september), september.ready_to_assign);
        // Later months that return more than they assign hold nothing back.
        let august = calculate_at(&b, "2026-08", "2026-10").unwrap();
        assert_eq!((august.ready_to_assign, august.future_assigned), (ready("2026-08"), -35_000));
        // Assigning more later than a past month held shows nothing left, but no shortfall:
        // income after that month funds the later months.
        b.assignments[2].amount = ready("2026-09") + 1_000;
        let september = calculate_at(&b, "2026-09", "2026-10").unwrap();
        assert_eq!((september.ready_to_assign, september.future_ready_to_assign), (0, -1_000));
        assert!(september.future_shortfalls.is_empty());
        assert_eq!(assignment_capacity(&b, "2026-09", &september), 0);
    }
    use crate::domain::tests::{sample, purchase};
    use serde_json::json;

    #[test]
    fn selected_month_matches_every_history_field_across_targets_and_account_kinds() {
        let mut b = two_cards();
        let mut tracking = b.accounts[0].clone();
        tracking.id = "tracking".into(); tracking.kind = AccountKind::Tracking;
        let mut savings = b.accounts[0].clone(); savings.id = "savings".into();
        b.accounts.insert(0, tracking);
        b.accounts.insert(2, savings);
        b.accounts.iter_mut().find(|a| a.id == "card2").unwrap().opening_balance = 3000;
        b.assignments.clear();
        for (id, target, goal) in [
            ("annual", 120000, json!({"goal_type":"NEED", "goal_cadence":13,
                "goal_creation_month":"2025-01-01", "goal_target_date":"2025-12-31", "goal_needs_whole_amount":false})),
            ("weekly", 1000, json!({"goal_type":"NEED", "goal_cadence":2,
                "goal_creation_month":"2025-01-01", "goal_day":0, "goal_needs_whole_amount":false})),
            ("dated", 25000, json!({"goal_type":"TBD", "goal_creation_month":"2025-01-01", "goal_target_date":"2025-09-30"})),
            ("balance", 50000, json!({"goal_type":"TB"})),
        ] {
            let mut category = b.categories[0].clone();
            category.id = id.into(); category.target = target;
            category.extra.insert("goal".into(), goal); b.categories.push(category);
        }
        b.categories[0].extra.insert("goal".into(), json!({"goal_type":"NEED",
            "goal_cadence":1, "goal_needs_whole_amount":false, "goal_creation_month":"2025-01-01"}));
        let first = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let months: Vec<_> = (0..24).map(|n| first.checked_add_months(Months::new(n)).unwrap().format("%Y-%m").to_string()).collect();
        for (n, month) in months.iter().enumerate() {
            for (category, amount) in [("food", 18000), ("annual", 9000), ("weekly", 2000)] {
                b.assignments.push(Assignment { extra: Default::default(), month: month.clone(), category_id: category.into(), amount });
            }
            for (part, account, category, amount) in [
                (0, "cash", "food", -12000), (1, "card", "food", -8000),
                (2, "card2", "food", -5000), (3, "card", "food", 2000),
                (4, "cash", "annual", if n % 12 == 11 { -100000 } else { 0 }),
            ] {
                let mut t = purchase(); t.id = format!("{n}-{part}"); t.account_id = account.into();
                t.date = format!("{month}-{:02}", part + 1); t.amount = amount;
                t.splits[0].category_id = Some(category.into()); t.splits[0].amount = amount;
                t.status = if part % 2 == 0 { Cleared::Uncleared } else { Cleared::Reconciled };
                b.transactions.push(t);
            }
        }
        let mut bill = purchase(); bill.account_id = "cash".into(); bill.date = "2025-01-07".into();
        let mut card_payment = bill.clone(); card_payment.transfer_account_id = Some("card".into());
        card_payment.splits.clear();
        b.extra.insert("schedules".into(), json!([
            {"id":"bill", "nextDate":"2025-01-07", "anchorDate":"2025-01-07", "frequency":"weekly", "transaction":bill},
            {"id":"card-payment", "nextDate":"2025-01-15", "anchorDate":"2025-01-15", "frequency":"monthly", "transaction":card_payment},
        ]));
        for as_of in ["2025-06", "2026-01", "2027-01"] {
            let history = calculate_history_at(&b, "2026-12", as_of).unwrap();
            for month in &months {
                let mut selected = calculate_range_at(&b, month, as_of, false).unwrap();
                assert_eq!(selected.len(), 1);
                assert_eq!(serde_json::to_value(selected.remove(month).unwrap()).unwrap(),
                    serde_json::to_value(&history[month]).unwrap(), "{month}, as of {as_of}");
            }
        }
        // Empty and cash-only budgets exercise the zero-width credit matrix too.
        let mut cash_only = sample(); cash_only.accounts.remove(1); cash_only.categories.remove(1);
        for budget in [Budget::default(), cash_only] {
            assert_eq!(serde_json::to_value(calculate_range_at(&budget, "2026-10", "2026-10", false).unwrap()).unwrap(),
                serde_json::to_value(calculate_history_at(&budget, "2026-10", "2026-10").unwrap()).unwrap());
        }
    }

    #[test]
    fn selected_recurring_refill_uses_the_previous_cycles_closing_available() {
        let mut b = sample();
        b.accounts[0].opening_balance = 1_000_000;
        b.categories[0].target = 120000;
        b.categories[0].extra.insert("goal".into(), json!({"goal_type":"NEED", "goal_cadence":13,
            "goal_creation_month":"2025-01-01", "goal_target_date":"2025-12-31", "goal_needs_whole_amount":false}));
        b.assignments = vec![
            Assignment { extra: Default::default(), month:"2025-01".into(), category_id:"food".into(), amount:120000 },
            Assignment { extra: Default::default(), month:"2026-01".into(), category_id:"food".into(), amount:8334 },
        ];
        let mut spent = purchase(); spent.account_id = "cash".into(); spent.date = "2025-12-15".into();
        spent.amount = -100000; spent.splits[0].amount = spent.amount; b.transactions.push(spent);
        let history = calculate_history_at(&b, "2026-02", "2026-02").unwrap();
        assert_eq!(history["2025-12"].categories[0].available, 20000);
        let selected = calculate_at(&b, "2026-02", "2026-02").unwrap();
        let row = &selected.categories[0];
        // Prior-cycle 20,000 plus January's 8,334 leaves 91,666 over eleven months.
        assert_eq!((row.carried, row.available, row.needed, row.target_funding_required), (28334, 28334, 8334, 8334));
        assert_eq!(crate::targets::needed_indexed_at(&b.categories[0], "2026-02", row.assigned,
            row.available, row.carried, &history, &crate::targets::AssignmentIndex::new(&b), "2026-02"), row.needed);
        // A future refill cannot rely on that prior-cycle money until the month arrives.
        assert_eq!(calculate_at(&b, "2026-02", "2026-01").unwrap().categories[0].needed, 10152);
    }

    #[test]
    fn tracking_transfers_use_the_budget_accounts_cash_or_credit_rules() {
        for kind in [AccountKind::Cash, AccountKind::Credit] {
            for amount in [-25000, -5000, 5000] {
                for reversed in [false, true] {
                    let mut ordinary = sample();
                    let mut tracking = ordinary.accounts[0].clone();
                    tracking.id = "tracking".into();
                    tracking.kind = AccountKind::Tracking;
                    tracking.opening_balance = 0;
                    ordinary.accounts.push(tracking);
                    let account = if kind == AccountKind::Credit { "card" } else { "cash" };
                    let mut t = purchase();
                    t.account_id = account.into();
                    t.amount = amount;
                    t.splits[0].amount = amount;
                    ordinary.transactions.push(t.clone());
                    let mut transferred = ordinary.clone();
                    t.transfer_account_id = Some("tracking".into());
                    t.transfer_status = Some(Cleared::Cleared);
                    if reversed {
                        t.account_id = "tracking".into();
                        t.transfer_account_id = Some(account.into());
                        t.amount = -amount;
                        t.splits[0].amount = -amount;
                    }
                    transferred.transactions[0] = t;
                    for month in ["2026-10", "2026-11"] {
                        let expected = calculate(&ordinary, month).unwrap();
                        let actual = calculate(&transferred, month).unwrap();
                        assert_eq!(actual.ready_to_assign, expected.ready_to_assign,
                            "{kind:?}, {amount}, reversed={reversed}, {month}");
                        assert_eq!(serde_json::to_value(actual.categories).unwrap(),
                            serde_json::to_value(expected.categories).unwrap());
                    }
                }
            }
        }
    }

    fn two_cards() -> Budget {
        let mut b = sample();
        let mut second = b.accounts[1].clone();
        second.id = "card2".into();
        b.accounts.push(second);
        let mut category = b.categories[1].clone();
        category.id = "payment2".into();
        category.extra.insert("paymentAccountId".into(), json!("card2"));
        b.categories.push(category);
        b
    }

    #[test]
    fn card_balance_transfers_leave_payment_envelopes_for_manual_reassignment() {
        for reversed in [false, true] {
            let mut b = two_cards();
            b.assignments.push(Assignment { extra: Default::default(),
                month: "2026-10".into(), category_id: "payment".into(), amount: 10000,
            });
            let mut t = purchase();
            t.account_id = if reversed { "card" } else { "card2" }.into();
            t.transfer_account_id = Some(if reversed { "card2" } else { "card" }.into());
            t.transfer_status = Some(Cleared::Cleared);
            t.amount = if reversed { 10000 } else { -10000 };
            t.splits.clear();
            b.transactions.push(t);
            for month in ["2026-10", "2026-11"] {
                let s = calculate(&b, month).unwrap();
                assert_eq!(s.categories[1].available, 10000);
                assert_eq!(s.categories[2].available, 0);
                assert_eq!(s.ready_to_assign, 70000);
            }
        }
    }

    #[test]
    fn idle_positive_card_does_not_erase_assigned_payment_money_each_month() {
        let mut b = sample();
        b.accounts[1].opening_balance = 5000;
        b.assignments.push(Assignment { extra: Default::default(),
            month: "2026-10".into(), category_id: "payment".into(), amount: 20000,
        });
        let history = calculate_history(&b, "2027-02").unwrap();
        for s in history.values() {
            assert_eq!(s.categories[1].available, 20000);
            assert_eq!(s.categories[1].activity, 0);
            assert_eq!(s.ready_to_assign, 65000);
        }
    }

    #[test]
    fn same_day_card_funding_uses_recorded_creation_order() {
        let mut b = two_cards();
        b.assignments[0].amount = 5000;
        for (account, stamp) in [("card", "2026-10-15T09:00:00.000Z"),
                                 ("card2", "2026-10-15T10:00:00.000Z")] {
            let mut t = purchase();
            t.id = account.into();
            t.account_id = account.into();
            t.set_created_at(stamp);
            b.transactions.push(t);
        }
        for _ in 0..2 {
            let s = calculate(&b, "2026-10").unwrap();
            assert_eq!(s.categories[1].available, 5000);
            assert_eq!(s.categories[2].available, 0);
            b.transactions.reverse();
        }
    }

    #[test]
    fn outgoing_transfers_and_uncategorized_spending_consume_positive_card_cash() {
        for transfer in [true, false] {
            let mut b = sample();
            b.accounts[1].opening_balance = 10000;
            b.assignments[0].amount = 5000;
            let mut out = purchase();
            out.id = "out".into();
            out.amount = -10000;
            out.set_created_at("2026-10-15T09:00:00.000Z");
            if transfer {
                out.splits.clear();
                out.transfer_account_id = Some("cash".into());
                out.transfer_status = Some(Cleared::Cleared);
            } else {
                out.splits[0].category_id = None;
                out.splits[0].amount = -10000;
            }
            let mut spend = purchase();
            spend.set_created_at("2026-10-15T10:00:00.000Z");
            b.transactions = vec![spend, out];
            let s = calculate(&b, "2026-10").unwrap();
            assert_eq!(s.accounts[1].balance, -5000);
            assert_eq!(s.categories[1].available, 5000);
            // Uncategorized, the card's cash is overspent, so it comes out the month after.
            assert_eq!(s.ready_to_assign, 105000);
            assert_eq!(s.uncategorized_credit, 0);
            let next = calculate(&b, "2026-11").unwrap();
            assert_eq!(next.ready_to_assign, if transfer { 105000 } else { 95000 });
        }
    }

    #[test]
    fn explicit_as_of_month_controls_future_refill_targets() {
        let mut b = sample();
        b.assignments[0].month = "2026-09".into();
        b.assignments[0].amount = 5000;
        b.categories[0].target = 10000;
        b.categories[0].extra.insert("goal".into(), json!({
            "goal_type": "NEED", "goal_cadence": 1, "goal_needs_whole_amount": false,
        }));
        // Deliberately disagree with the explicit clock to prove no ambient clock leaks in.
        b.extra.insert("calculationAsOfMonth".into(), json!("2026-10"));
        let future = calculate_at(&b, "2026-10", "2026-09").unwrap();
        assert_eq!(future.categories[0].needed, 10000);
        let current = calculate_at(&b, "2026-10", "2026-10").unwrap();
        assert_eq!(current.categories[0].needed, 5000);
    }

    #[test]
    fn underfunded_without_a_card_target_covers_existing_debt_after_funded_spending() {
        let mut b = sample();
        b.transactions.push(purchase());
        let mut future = purchase();
        future.id = "future".into();
        future.date = "2026-12-15".into();
        b.transactions.push(future);
        let before = calculate(&b, "2026-10").unwrap();
        assert_eq!(before.categories[1].available, 5000);
        assert_eq!(before.categories[1].needed, 10000,
            "only October debt not covered by payment cash needs funding");
        let funded = crate::workflows::apply(b, &json!({
            "type": "autoAssign", "month": "2026-10", "mode": "underfunded",
            "categories": ["payment"], "limitToAvailable": true,
        })).unwrap();
        let after = calculate(&funded, "2026-10").unwrap();
        assert_eq!(after.categories[1].assigned, 10000);
        assert_eq!(after.categories[1].available, 15000);
        assert_eq!(after.categories[1].needed, 0);
        assert_eq!(after.ready_to_assign, 70000);
    }
    #[test]
    fn missing_categories_are_overspending_that_comes_out_of_next_month_as_in_ynab() {
        for account in ["cash", "card"] {
            let mut b = sample();
            let mut t = purchase(); t.account_id = account.into(); t.splits[0].category_id = None;
            b.transactions.push(t);
            let missing = calculate(&b, "2026-10").unwrap();
            assert_eq!(missing.income, 0);
            assert_eq!(missing.uncategorized, -5000);
            // Spent on a card beyond its cash, it's debt: credit overspending.
            assert_eq!(missing.uncategorized_credit, if account == "cash" { 0 } else { 5000 });
            // Held this month like any overspending; its cash comes out of next month's.
            assert_eq!(missing.ready_to_assign, 80000);
            assert_eq!(missing.categories[1].available, 0);
            let next = calculate(&b, "2026-11").unwrap();
            assert_eq!(next.uncategorized, 0);
            assert_eq!(next.ready_to_assign, if account == "cash" { 75000 } else { 80000 });
            // Taken from Unassigned on purpose, it isn't overspending and comes out at once.
            b.transactions[0].splits[0].extra.insert("unassigned".into(), json!(true));
            let intentional = calculate(&b, "2026-10").unwrap();
            assert_eq!(intentional.income, -5000);
            assert_eq!(intentional.uncategorized, 0);
            assert_eq!(intentional.ready_to_assign, next.ready_to_assign);
            assert_eq!(intentional.categories[1].available, missing.categories[1].available);
        }
    }
    #[test]
    fn imported_starting_balance_changes_cash_but_not_period_income() {
        let mut b = sample();
        let mut t = purchase(); t.account_id = "cash".into(); t.payee = "Starting Balance".into();
        t.amount = 5000; t.splits[0].amount = 5000; t.splits[0].category_id = None;
        t.extra.insert("startingBalance".into(), json!(true)); b.transactions.push(t);
        let opening = calculate(&b, "2026-10").unwrap();
        assert_eq!((opening.income, opening.ready_to_assign, opening.net_worth), (0,85000,95000));
        b.transactions[0].payee = "Employer".into();
        let renamed = calculate(&b, "2026-10").unwrap();
        assert_eq!((renamed.income,renamed.ready_to_assign), (5000,85000));
    }
    #[test]
    fn card_funding_follows_purchases_and_survives_account_reordering() {
        let mut b = sample();
        b.assignments[0].amount = 15000;
        let mut second = b.accounts[1].clone(); second.id = "card2".into();
        b.accounts.push(second);
        let mut category = b.categories[1].clone(); category.id = "payment2".into();
        category.extra.insert("paymentAccountId".into(), json!("card2")); b.categories.push(category);
        for (id, account, date) in [("a", "card", "2026-10-01"), ("b", "card2", "2026-10-02"), ("c", "card", "2026-10-03")] {
            let mut t = purchase(); t.id = id.into(); t.account_id = account.into(); t.date = date.into();
            t.amount = -10000; t.splits[0].amount = t.amount; b.transactions.push(t);
        }
        let amounts = |s: Summary| s.categories.into_iter().map(|c| (c.id, c.available)).collect::<BTreeMap<_,_>>();
        let first = amounts(calculate(&b, "2026-10").unwrap());
        assert_eq!(first["payment"], 10000); assert_eq!(first["payment2"], 5000);
        b.accounts.swap(1, 2);
        assert_eq!(first, amounts(calculate(&b, "2026-10").unwrap()));
        // Cash spending retains priority over both cards.
        let mut cash = purchase(); cash.id = "cash-spend".into(); cash.account_id = "cash".into();
        b.transactions.push(cash);
        let after = amounts(calculate(&b, "2026-10").unwrap());
        assert_eq!(after["payment"], 10000); assert_eq!(after["payment2"], 0);
    }
    #[test]
    fn target_progress_has_monthly_units_for_weekly_and_yearly_goals() {
        let mut b = sample(); b.categories[0].target = 10000; b.assignments[0].amount = 10000;
        b.categories[0].extra.insert("goal".into(), json!({"goal_type":"NEED", "goal_cadence":2, "goal_day":0}));
        let row = calculate(&b, "2026-10").unwrap().categories.remove(0);
        assert_eq!((row.target_funding_required,row.target_funding_funded,row.needed), (40000,10000,30000));
        b.categories[0].target = 120000; b.assignments[0].amount = 0;
        b.categories[0].extra.insert("goal".into(),json!({"goal_type":"NEED","goal_cadence":13,"goal_target_date":"2027-09-30","goal_creation_month":"2026-10-01"}));
        let row = calculate(&b,"2026-10").unwrap().categories.remove(0);
        assert_eq!((row.target_funding_required,row.target_funding_funded,row.needed),(10000,0,10000));
    }
    #[test]
    fn an_undated_balance_needs_nothing_this_month_and_reports_what_is_left_to_save() {
        // Emergency fund: 15,000.00 with no date and 13,000.00 saved. YNAB's underfunded amount
        // is 0 every month, and its row says "2,000.00 more needed eventually".
        let mut b = sample();
        b.accounts[0].opening_balance = 2_000_000;
        b.categories[0].target = 1_500_000;
        b.categories[0].extra.insert("goal".into(), json!({"goal_type":"TB", "goal_creation_month":"2026-10-01"}));
        b.assignments = vec![Assignment { extra: Default::default(), month: "2026-07".into(), category_id: "food".into(), amount: 300_000 },
            Assignment { extra: Default::default(), month: "2026-08".into(), category_id: "food".into(), amount: 1_000_000 }];
        for month in ["2026-10", "2026-11"] {
            let row = calculate(&b, month).unwrap().categories.remove(0);
            assert_eq!((row.available, row.needed, row.target_funding_required, row.target_funding_funded, row.month_target),
                (1_300_000, 0, 1_500_000, 1_300_000, 0), "{month}");
        }
    }
    #[test]
    fn month_targets_count_each_targets_monthly_cost_and_due_dates_reach_the_summary() {
        let mut b = sample();
        b.extra.insert("calculationAsOfMonth".into(), json!("2026-10"));
        // Food refills to 200.00 with 50.00 carried from September; the card owes 100.00.
        b.categories[0].extra.insert("goal".into(), json!({"goal_type":"NEED", "goal_cadence":1,
            "goal_needs_whole_amount":false, "goal_creation_month":"2026-09-01"}));
        b.assignments = vec![Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "food".into(), amount: 5000 },
            Assignment { extra: Default::default(), month: "2026-10".into(), category_id: "food".into(), amount: 1000 }];
        for (id, target, goal) in [
            ("annual", 120000, json!({"goal_type":"NEED", "goal_cadence":13, "goal_target_date":"2027-09-30", "goal_creation_month":"2026-10-01"})),
            ("trip", 30000, json!({"goal_type":"TBD", "goal_target_date":"2026-10-20", "goal_creation_month":"2026-10-01"})),
            ("weekly", 1000, json!({"goal_type":"NEED", "goal_cadence":2, "goal_day":0})),
            ("balance", 50000, json!({"goal_type":"TB"})),
            ("snoozed", 7000, json!({"goal_type":"NEED", "goal_cadence":1, "goal_snoozed_at":"2026-10-01"})),
        ] {
            let mut category = b.categories[0].clone();
            category.id = id.into(); category.target = target;
            category.extra.insert("goal".into(), goal); b.categories.push(category);
        }
        let s = calculate(&b, "2026-10").unwrap();
        let row = |id: &str| s.categories.iter().find(|c| c.id == id).unwrap();
        let month_targets: BTreeMap<_, _> = s.categories.iter().map(|c| (c.id.as_str(), c.month_target)).collect();
        assert_eq!(month_targets, BTreeMap::from([("food", 20000), ("payment", 0), ("annual", 10000),
            ("trip", 30000), ("weekly", 4000), ("balance", 0), ("snoozed", 0)]));
        // Funding still deducts the refill's carryover and this month's assignment, and the card's balance.
        assert_eq!((row("food").target_funding_required, row("food").needed), (15000, 14000));
        assert_eq!(row("payment").target_funding_required, 10000);
        let due = |id: &str| row(id).target_due.clone().map(|d| (d.date, d.monthly));
        assert_eq!(due("food"), Some(("2026-10-31".into(), true)));
        assert_eq!(due("trip"), Some(("2026-10-20".into(), false)));
        for id in ["payment", "annual", "weekly", "balance"] { assert_eq!(due(id), None, "{id}"); }
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["categories"][0]["monthTarget"], 20000);
        assert_eq!(json["categories"][0]["targetDue"], json!({"date":"2026-10-31", "monthly":true}));
        assert!(json["categories"][1].get("targetDue").is_none(), "absent rather than null");
        // A month before the target existed asks for nothing.
        assert_eq!(calculate(&b, "2026-08").unwrap().categories[0].month_target, 0);
    }
    #[test]
    fn refunded_purchases_do_not_take_priority_over_another_cards_outstanding_spending() {
        let mut b = sample(); b.assignments[0].amount = 10000;
        let mut second = b.accounts[1].clone(); second.id = "card2".into(); b.accounts.push(second);
        let mut category = b.categories[1].clone(); category.id = "payment2".into();
        category.extra.insert("paymentAccountId".into(), json!("card2")); b.categories.push(category);
        for (id, account, day, amount) in [("a", "card", 1, -10000), ("b", "card2", 2, -10000), ("refund", "card", 3, 10000), ("c", "card", 4, -10000)] {
            let mut t = purchase(); t.id = id.into(); t.account_id = account.into();
            t.date = format!("2026-10-{day:02}"); t.amount = amount; t.splits[0].amount = amount; b.transactions.push(t);
        }
        let summary = calculate(&b, "2026-10").unwrap();
        let amount = |id| summary.categories.iter().find(|c| c.id == id).unwrap().available;
        assert_eq!((amount("payment"), amount("payment2")), (0, 10000));
    }
}
