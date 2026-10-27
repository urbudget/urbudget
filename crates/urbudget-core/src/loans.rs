use crate::{i18n::tr, t};
use crate::domain::*;
use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// When a loan's interest counts as added: a millisecond before the payment that brings it, so a
/// payment lists above its month's interest however it was entered (Record payment, or a
/// transfer typed in a register).
fn just_before(at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(at)
        .map(|t| stamp(t.with_timezone(&chrono::Utc) - chrono::Duration::milliseconds(1)))
        .unwrap_or_else(|_| at.to_owned())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Terms {
    pub account_id: String,
    pub category_id: String,
    pub annual_rate_basis_points: i64,
    pub monthly_payment: i64,
    pub monthly_fees: i64,
    #[serde(default)]
    pub rate_history: Vec<Rate>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rate {
    pub month: String,
    pub annual_rate_milli_percent: i64,
}
fn rate_at(t: &Terms, date: &str) -> i64 {
    t.rate_history
        .iter()
        .filter(|r| r.month.as_str() <= &date[..7])
        .max_by_key(|r| &r.month)
        .map(|r| r.annual_rate_milli_percent)
        .unwrap_or(t.annual_rate_basis_points * 10)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payment {
    pub date: String,
    pub payment: i64,
    pub interest: i64,
    pub fees: i64,
    pub principal: i64,
    pub balance: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub months: usize,
    pub total_interest: i64,
    pub total_fees: i64,
    pub payoff_date: String,
    pub schedule: Vec<Payment>,
}
fn interest(balance: i64, rate: i64) -> i64 {
    ((balance as i128 * rate as i128 + 600000) / 1200000) as i64
}
fn valid(t: &Terms) -> Result<(), String> {
    if !(0..=100000).contains(&t.annual_rate_basis_points)
        || t.rate_history.iter().any(|r| {
            !valid_month(&r.month)
                || !(0..=1_000_000).contains(&r.annual_rate_milli_percent)
        })
        || !(0..=1_000_000_000_000).contains(&t.monthly_fees)
        || !(1..=1_000_000_000_000).contains(&t.monthly_payment)
    {
        Err(t!("Enter a positive payment, a rate from 0–1,000%, and non-negative fees."))
    } else {
        Ok(())
    }
}
pub fn project(balance: i64, terms: &Terms, start: &str, extra: i64) -> Result<Projection, String> {
    project_with(balance, terms, start, extra, 0)
}
/// `project` with a one-off payment, `lump`, made with the first payment: after that month's
/// interest and fees are added, as recording a payment adds them, so a one-off of just the
/// balance can't clear the loan.
pub fn project_with(balance: i64, terms: &Terms, start: &str, extra: i64, lump: i64) -> Result<Projection, String> {
    valid(terms)?;
    if ![balance, extra, lump].iter().all(|v| (0..=1_000_000_000_000).contains(v)) {
        return Err(t!("Invalid balance or extra payment."));
    }
    if !valid_date(start) { return Err(t!("Invalid first payment date")); }
    let anchor =
        NaiveDate::parse_from_str(start, "%Y-%m-%d").map_err(|_| t!("Invalid first payment date"))?;
    let mut balance = balance;
    let mut rows = vec![];
    let mut total_interest = 0;
    let mut total_fees = 0;
    while balance > 0 {
        if rows.len() >= 1200 {
            return Err(t!("This payment takes more than 100 years to repay the loan."));
        }
        let fee = terms.monthly_fees;
        let date = anchor
            .checked_add_months(Months::new(rows.len() as u32))
            .ok_or_else(|| t!("Date exceeds supported range"))?
            .to_string();
        if !valid_date(&date) { return Err(t!("Date exceeds supported range")); }
        let charge = interest(balance, rate_at(terms, &date));
        let once = if rows.is_empty() { lump } else { 0 };
        let payment = (terms.monthly_payment + extra + once).min(balance + charge + fee);
        if payment <= charge + fee {
            return Err(t!("Payment does not cover monthly interest and fees. Increase the payment."));
        }
        let principal = payment - charge - fee;
        balance -= principal;
        rows.push(Payment {
            date,
            payment,
            interest: charge,
            fees: fee,
            principal,
            balance,
        });
        total_interest += charge;
        total_fees += fee;
    }
    Ok(Projection {
        months: rows.len(),
        total_interest,
        total_fees,
        payoff_date: rows.last().map(|r| r.date.clone()).unwrap_or(start.into()),
        schedule: rows,
    })
}
pub fn payment_by_date(balance: i64, terms: &Terms, start: &str, due: &str) -> Result<i64, String> {
    payment_by_date_with(balance, terms, start, due, 0)
}
/// `payment_by_date` with a one-off payment made with the first payment (see `project_with`).
pub fn payment_by_date_with(balance: i64, terms: &Terms, start: &str, due: &str, lump: i64) -> Result<i64, String> {
    valid(terms)?;
    const LIMIT: i64 = 1_000_000_000_000;
    if ![balance, lump].iter().all(|n| (0..=LIMIT).contains(n)) {
        return Err(t!("Invalid loan balance or one-off payment."));
    }
    let date = |value: &str, error: &str| -> Result<NaiveDate, String> {
        if !valid_date(value) { return Err(error.into()); }
        let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| error.to_owned())?;
        Ok(parsed)
    };
    let due_date = date(due, tr("Invalid payoff date"))?;
    let start_date = date(start, tr("Invalid first payment date"))?;
    if due_date < start_date {
        return Err(t!("Payoff date must follow the first payment date."));
    }
    if balance == 0 { return Ok(0); }
    let months = ((due_date.year() - start_date.year()) * 12
        + due_date.month() as i32 - start_date.month() as i32 + 1).min(1200) as u32;
    // A bounded feasibility check treats insufficient payment as false while keeping
    // actual date/range failures visible. It uses the projection's cent rounding and
    // rejects non-amortizing months just as project_with does.
    let fits = |monthly: i64| -> Result<bool, String> {
        let mut remaining = balance;
        for step in 0..months {
            let date = start_date.checked_add_months(Months::new(step)).ok_or_else(|| t!("Date exceeds supported range"))?;
            if date > due_date { return Ok(false); }
            let charge = interest(remaining, rate_at(terms, &date.to_string())) + terms.monthly_fees;
            let payment = (monthly + if step == 0 { lump } else { 0 }).min(remaining + charge);
            if payment <= charge { return Ok(false); }
            remaining -= payment - charge;
            if remaining == 0 { return Ok(true); }
        }
        Ok(false)
    };
    let mut low = 1;
    let mut high = (balance + interest(balance, rate_at(terms, start)) + terms.monthly_fees - lump).clamp(1, LIMIT);
    if !fits(high)? {
        return Err(t!("No supported monthly payment can repay this loan by the selected date."));
    }
    while low < high {
        let mid = (low + high) / 2;
        if fits(mid)? {
            high = mid
        } else {
            low = mid + 1
        };
    }
    let mut selected = terms.clone();
    selected.monthly_payment = low;
    let projection = project_with(balance, &selected, start, 0, lump)?;
    if projection.payoff_date.as_str() > due {
        return Err(t!("No supported monthly payment can repay this loan by the selected date."));
    }
    Ok(low)
}
pub fn record(
    b: &mut Budget,
    terms: &Terms,
    from: &str,
    amount: i64,
    date: &str,
    id: &str,
) -> Result<(), String> {
    // Command draft patches are untrusted until validated, including existing ledger rows.
    validate(b)?;
    valid(terms)?;
    if !valid_date(date) { return Err(t!("Invalid payment date")); }
    if amount <= 0 || amount > 1_000_000_000_000 || b.transactions.iter().any(|t| t.id == id) {
        return Err(t!("Invalid or duplicate payment."));
    }
    let account = b
        .accounts
        .iter()
        .find(|a| a.id == terms.account_id && a.kind == AccountKind::Tracking)
        .ok_or_else(|| t!("Choose a tracking loan account"))?;
    if !b
        .accounts
        .iter()
        .any(|a| a.id == from && a.kind == AccountKind::Cash && a.on_budget())
        || !b.categories.iter().any(|c| c.id == terms.category_id)
    {
        return Err(t!("Choose a cash account and payment category."));
    }
    let balance = -(account.opening_balance
        + crate::ledger::transactions(b)
            .iter()
            .filter(|t| t.date.as_str() <= date)
            .map(|t| {
                if t.account_id == account.id {
                    t.amount
                } else if t.transfer_account_id.as_deref() == Some(&account.id) {
                    -t.amount
                } else {
                    0
                }
            })
            .sum::<i64>());
    if balance <= 0 {
        return Err(t!("This account has no loan balance to repay on the payment date."));
    }
    let month = &date[..7];
    let interest_id = format!("loan-interest:{}:{month}", account.id);
    // The interest and the payment are added together, the interest just before.
    let now = stamp_now();
    if !crate::fresh_start::interest_was_charged(b, &account.id, month) && !b.transactions.iter().any(|t| {
        t.id == interest_id
            || (t.account_id == account.id
                && t.date.starts_with(month)
                && t.extra
                    .get("source")
                    .is_some_and(|s| s["debt_transaction_type"] == "interest"))
    }) {
        let charge = interest(balance.max(0), rate_at(terms, date)) + terms.monthly_fees;
        if charge > 0 {
            b.transactions.push(Transaction {
                id: interest_id,
                date: date.into(),
                account_id: account.id.clone(),
                payee: "Loan interest & fees".into(),
                memo: format!("Monthly interest plus fees for {month}"),
                amount: -charge,
                status: Cleared::Uncleared,
                splits: vec![Split {
                    category_id: None,
                    amount: -charge,
                    extra: Default::default(),
                }],
                transfer_account_id: None,
                transfer_status: None,
                extra: serde_json::from_value(json!({"loanInterest":true, "createdAt": just_before(&now)})).unwrap(),
            });
        }
    }
    b.transactions.push(Transaction {
        id: id.into(),
        date: date.into(),
        account_id: from.into(),
        payee: "".into(),
        memo: "Loan payment".into(),
        amount: -amount,
        status: Cleared::Uncleared,
        splits: vec![Split {
            category_id: Some(terms.category_id.clone()),
            amount: -amount,
            extra: Default::default(),
        }],
        transfer_account_id: Some(terms.account_id.clone()),
        transfer_status: Some(Cleared::Uncleared),
        extra: serde_json::from_value(json!({"createdAt": now})).unwrap(),
    });
    validate(b)
}
/// Post interest only for newly entered local loan payments; imported history is never rewritten.
pub fn apply_new_payments(
    b: &mut Budget,
    ids: &std::collections::HashSet<String>,
) -> Result<(), String> {
    // New payments have already been appended by callers. Validate the complete input
    // before expanding transfers, slicing dates, or calculating interest from balances.
    validate(b)?;
    let terms: Vec<Terms> =
        serde_json::from_value(b.extra.get("loans").cloned().unwrap_or(json!([])))
            .map_err(|_| t!("Invalid loan terms"))?;
    if terms.is_empty() {
        return Ok(());
    }
    // Only generated split legs inherit an accounting parent. A saved transaction
    // can retain unrelated imported metadata named parentId without changing its ID.
    let new = |p: &std::borrow::Cow<'_, Transaction>| ids.contains(crate::ledger::saved_id(p));
    // Owned: payments below are appended to the budget while this snapshot is read. Only the
    // new transactions can be payments to post interest for.
    let ledger: Vec<Transaction> = crate::ledger::transactions(b)
        .into_iter()
        .filter(|p| new(p))
        .map(std::borrow::Cow::into_owned)
        .collect();
    // Interest is added with the payments that bring it.
    let now = stamp_now();
    for t in terms {
        valid(&t)?;
        let account = b
            .accounts
            .iter()
            .find(|a| a.id == t.account_id)
            .ok_or_else(|| t!("Missing loan account"))?
            .clone();
        // Either register may own the saved transfer. A payment increases the loan
        // balance, whether that is a receiving-side inflow or a payer-side outflow.
        let pays_loan = |p: &Transaction| p.transfer_account_id.as_deref().is_some_and(|peer| {
            (p.account_id == t.account_id && p.amount > 0)
                || (peer == t.account_id && p.amount < 0)
        });
        let mut dates: Vec<_> = ledger
            .iter()
            .filter(|p| pays_loan(p))
            .map(|p| p.date.clone())
            .collect();
        dates.sort();
        dates.dedup();
        for date in dates {
            let month = &date[..7];
            let interest_id = format!("loan-interest:{}:{month}", t.account_id);
            if crate::fresh_start::interest_was_charged(b, &t.account_id, month) || b.transactions.iter().any(|p| {
                p.id == interest_id
                    || (p.account_id == t.account_id
                        && p.date.starts_with(month)
                        && p.extra
                            .get("source")
                            .is_some_and(|s| s["debt_transaction_type"] == "interest"))
            }) {
                continue;
            }
            let balance = -(account.opening_balance
                + crate::ledger::transactions(b)
                    .iter()
                    .filter(|p| p.date <= date)
                    .map(|p| {
                        if p.date == date && new(p) {
                            0
                        } else if p.account_id == t.account_id {
                            p.amount
                        } else if p.transfer_account_id.as_deref() == Some(&t.account_id) {
                            -p.amount
                        } else {
                            0
                        }
                    })
                    .sum::<i64>());
            let charge = interest(balance.max(0), rate_at(&t, &date)) + t.monthly_fees;
            // Added just before the day's first payment to the loan, as Record payment adds it.
            let paid_at = ledger
                .iter()
                .filter(|p| p.date == date && pays_loan(p))
                .filter_map(Transaction::created_at)
                .min()
                .unwrap_or(now.as_str());
            let added = just_before(paid_at);
            if charge > 0 {
                b.transactions.push(Transaction {
                    id: interest_id,
                    date: date.clone(),
                    account_id: t.account_id.clone(),
                    payee: "Loan interest & fees".into(),
                    memo: format!("Monthly interest plus fees for {month}"),
                    amount: -charge,
                    status: Cleared::Uncleared,
                    splits: vec![Split {
                        category_id: None,
                        amount: -charge,
                        extra: Default::default(),
                    }],
                    transfer_account_id: None,
                    transfer_status: None,
                    extra: json!({"loanInterest":true, "createdAt": added}).as_object().unwrap().clone().into(),
                });
            }
        }
    }
    Ok(())
}
pub fn source_terms(b: &Budget, account_id: &str) -> Result<Terms, String> {
    let a = b
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| t!("Account not found"))?;
    let s = a
        .extra
        .get("source")
        .ok_or_else(|| t!("No imported loan terms are available"))?;
    let rates = s["debt_interest_rates"]
        .as_object()
        .ok_or_else(|| t!("This account has no imported loan rates"))?;
    let mut history: Vec<_> = rates
        .iter()
        .filter_map(|(date, rate)| {
            Some(Rate {
                month: date.get(..7)?.into(),
                annual_rate_milli_percent: rate.as_i64()?,
            })
        })
        .collect();
    history.sort_by(|a, b| a.month.cmp(&b.month));
    let month = chrono::Local::now().format("%Y-%m").to_string();
    let latest = |key: &str| {
        s[key]
            .as_object()
            .and_then(|o| {
                o.iter()
                    .filter(|(d, _)| d.get(..7).is_some_and(|d| d <= month.as_str()))
                    .max_by_key(|(d, _)| *d)
            })
            .and_then(|(_, v)| v.as_i64())
            .unwrap_or(0)
    };
    let category = crate::ledger::transactions(b)
        .iter()
        .filter(|t| t.transfer_account_id.as_deref() == Some(account_id))
        .flat_map(|t| t.splits.iter())
        .filter_map(|s| s.category_id.clone())
        .next()
        .unwrap_or_default();
    let payment = latest("debt_minimum_payments");
    let fees = latest("debt_escrow_amounts");
    if payment % 10 != 0 || fees % 10 != 0 {
        return Err(t!("Imported loan payment or fees contain sub-cent values."));
    }
    Ok(Terms {
        account_id: account_id.into(),
        category_id: category,
        annual_rate_basis_points: latest("debt_interest_rates") / 10,
        monthly_payment: payment / 10,
        monthly_fees: fees / 10,
        rate_history: history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terms() -> Terms {
        Terms {
            account_id: "loan".into(),
            category_id: "c".into(),
            annual_rate_basis_points: 1200,
            monthly_payment: 10000,
            monthly_fees: 0,
            rate_history: vec![],
        }
    }
    #[test]
    fn amortization_rounds_cents_and_final_payment_and_extra_reduces_interest() {
        let t = terms();
        let p = project(100000, &t, "2026-01-31", 0).unwrap();
        assert_eq!(p.schedule[0].interest, 1000);
        assert_eq!(p.schedule[1].date, "2026-02-28");
        assert_eq!(p.schedule[2].date, "2026-03-31");
        assert_eq!(p.schedule.last().unwrap().balance, 0);
        assert!(p.schedule.last().unwrap().payment < 10000);
        assert!(
            project(100000, &t, "2026-01-31", 5000)
                .unwrap()
                .total_interest
                < p.total_interest
        );
    }
    #[test]
    fn a_one_off_payment_comes_after_the_months_interest() {
        let t = terms();
        // 1,000.00 at 12%: the first month adds 10.00 of interest, so 1,000.00 more doesn't
        // clear it alone; the month's own payment does.
        let p = project_with(100000, &t, "2026-01-31", 0, 100000).unwrap();
        assert_eq!((p.months, p.total_interest, p.schedule[0].payment), (1, 1000, 101000));
        assert_eq!(project_with(100000, &t, "2026-01-31", 0, 101000).unwrap().schedule[0].payment, 101000);
        assert_eq!(project(100000, &t, "2026-01-31", 0).unwrap().total_interest, project_with(100000, &t, "2026-01-31", 0, 0).unwrap().total_interest);
    }
    #[test]
    fn zero_rate_payoff_and_negative_amortization() {
        let mut t = terms();
        t.annual_rate_basis_points = 0;
        assert_eq!(
            payment_by_date(100000, &t, "2026-01-01", "2026-10-01").unwrap(),
            10000
        );
        t.monthly_payment = 10;
        t.annual_rate_basis_points = 1200;
        assert!(project(100000, &t, "2026-01-01", 0).is_err());
    }
    #[test]
    fn payoff_solver_validates_one_off_and_dates_even_for_a_paid_off_loan() {
        for balance in [0, 100000] {
            for lump in [-1, 1_000_000_000_001, i64::MAX] {
                assert!(payment_by_date_with(balance, &terms(), "2026-01-01", "2026-10-01", lump).is_err());
            }
            for (start, due) in [
                ("2026-1-01", "2026-10-01"), ("2026-01-01", "2026-1-01"),
                ("not-a-date", "2026-10-01"), ("2026-01-01", "2025-12-31"),
                ("2026-02-30", "2026-10-01"), ("2026-01-01", "+10000-01-01"),
            ] {
                assert!(payment_by_date_with(balance, &terms(), start, due, 0).is_err(), "{start} -> {due}");
            }
        }
        assert_eq!(payment_by_date_with(0, &terms(), "2026-01-01", "2026-01-01", 0).unwrap(), 0);
    }
    #[test]
    fn loan_projection_and_recording_reject_noncanonical_dates_before_slicing() {
        for invalid in ["1-1-1", "2026-1-1", "+10000-01-01", "2026-02-30", "é2026-01"] {
            for balance in [0, 100000] {
                assert!(project_with(balance, &terms(), invalid, 0, 0).is_err(), "{invalid}");
            }
            let mut b = crate::domain::tests::sample();
            b.accounts[1].id = "loan".into();
            b.accounts[1].kind = AccountKind::Tracking;
            b.categories.retain(|c| c.payment_account().is_none());
            let mut t = terms();
            t.category_id = "food".into();
            let before = b.clone();
            assert!(record(&mut b, &t, "cash", 100, invalid, "payment").is_err(), "{invalid}");
            assert_eq!(b, before);
        }
        let mut malformed = terms();
        malformed.rate_history.push(Rate { month: "2026-1 ".into(), annual_rate_milli_percent: 12000 });
        assert!(project_with(100000, &malformed, "2026-01-01", 0, 0).is_err());
        assert!(project_with(100000, &terms(), "9999-12-31", 0, 0).is_err(), "a valid first date must not produce unsupported later dates");
    }
    #[test]
    fn loan_mutations_validate_existing_and_new_ledger_rows_before_arithmetic() {
        let mut original = crate::domain::tests::sample();
        original.accounts[1].id = "loan".into();
        original.accounts[1].kind = AccountKind::Tracking;
        original.accounts[1].opening_balance = -100000;
        original.categories.retain(|c| c.payment_account().is_none());
        let mut terms = terms();
        terms.category_id = "food".into();
        original.extra.insert("loans".into(), json!([terms]));
        let mut payment = crate::domain::tests::purchase();
        payment.account_id = "cash".into();
        payment.transfer_account_id = Some("loan".into());
        payment.transfer_status = Some(Cleared::Uncleared);
        for malformed_date in [true, false] {
            let mut invalid = payment.clone();
            if malformed_date { invalid.date = "x".into(); }
            else {
                invalid.amount = i64::MIN;
                invalid.splits[0].amount = i64::MIN;
            }
            let mut b = original.clone();
            b.transactions.push(invalid);
            let before = b.clone();
            assert!(apply_new_payments(&mut b, &std::collections::HashSet::from(["tx".into()])).is_err());
            assert_eq!(b, before, "an invalid new payment must not post interest");
            assert!(record(&mut b, &terms, "cash", 1000, "2026-10-20", "next").is_err());
            assert_eq!(b, before, "an invalid existing ledger must not be used for a payment balance");
        }
    }
    #[test]
    fn payoff_solver_never_returns_an_unsupported_or_infeasible_monthly_payment() {
        const LIMIT: i64 = 1_000_000_000_000;
        assert!(payment_by_date_with(LIMIT, &terms(), "2026-01-01", "2026-01-01", 0).is_err(),
            "the first month's 1.01 trillion cents exceeds the supported monthly payment");
        let mut fees = terms();
        fees.monthly_fees = LIMIT;
        assert!(payment_by_date_with(LIMIT, &fees, "2026-01-01", "2027-01-01", 0).is_err());
        for (balance, due, lump) in [(LIMIT, "2026-02-28", 0), (LIMIT, "2026-01-31", 20_000_000_000),
            (100000, "2026-10-31", 25000), (100000, "2026-01-31", 100000)] {
            let mut selected = terms();
            selected.monthly_fees = 37;
            selected.rate_history = vec![Rate { month: "2026-03".into(), annual_rate_milli_percent: 18000 }];
            let payment = payment_by_date_with(balance, &selected, "2026-01-31", due, lump).unwrap();
            assert!((1..=LIMIT).contains(&payment));
            selected.monthly_payment = payment;
            let projection = project_with(balance, &selected, "2026-01-31", 0, lump).unwrap();
            assert!(projection.payoff_date.as_str() <= due);
            assert_eq!(projection.schedule.last().unwrap().balance, 0);
            if payment > 1 {
                selected.monthly_payment -= 1;
                assert!(!project_with(balance, &selected, "2026-01-31", 0, lump)
                    .is_ok_and(|p| p.payoff_date.as_str() <= due), "one cent less must not meet the date");
            }
        }
    }
    #[test]
    fn partial_payments_charge_interest_once_and_spend_the_category_once() {
        let mut b = Budget::default();
        for (id, kind, balance) in [
            ("cash", AccountKind::Cash, 500000),
            ("loan", AccountKind::Tracking, -100000),
        ] {
            b.accounts.push(Account {
                id: id.into(),
                name: id.into(),
                kind,
                opening_balance: balance,
                archived: false,
                extra: Default::default(),
            });
        }
        b.categories.push(Category {
            id: "c".into(),
            name: "Loan payment".into(),
            group: "Bills".into(),
            target: 10000,
            extra: Default::default(),
        });
        record(&mut b, &terms(), "cash", 5000, "2026-01-02", "one").unwrap();
        record(&mut b, &terms(), "cash", 5000, "2026-01-20", "two").unwrap();
        assert_eq!(b.transactions.len(), 3);
        let added = |id: &str| b.transactions.iter().find(|t| t.id == id).unwrap().created_at().unwrap().to_owned();
        assert!(added("loan-interest:loan:2026-01") < added("one"));
        let s = calculate(&b, "2026-01").unwrap();
        assert_eq!(
            s.accounts.iter().find(|a| a.id == "loan").unwrap().balance,
            -91000
        );
        assert_eq!(s.categories[0].activity, -10000);
        record(&mut b, &terms(), "cash", 10000, "2026-02-01", "three").unwrap();
        assert_eq!(
            b.transactions
                .iter()
                .find(|t| t.id == "loan-interest:loan:2026-02")
                .unwrap()
                .amount,
            -910
        );
    }
    #[test]
    fn projections_apply_dated_rates_and_new_payments_post_interest_once() {
        let mut t = terms();
        t.rate_history = vec![Rate {
            month: "2026-02".into(),
            annual_rate_milli_percent: 6000,
        }];
        let p = project(100000, &t, "2026-01-01", 0).unwrap();
        assert_eq!(p.schedule[0].interest, 1000);
        assert_eq!(p.schedule[1].interest, 455);
        let mut b = crate::domain::tests::sample();
        b.accounts[1].id = "loan".into();
        b.accounts[1].kind = AccountKind::Tracking;
        b.accounts[1].opening_balance = -100000;
        b.categories.retain(|c| c.payment_account().is_none());
        t.category_id = "food".into();
        b.extra.insert("loans".into(), json!([t]));
        let mut payment = crate::domain::tests::purchase();
        payment.account_id = "cash".into();
        payment.date = "2026-02-01".into();
        payment.transfer_account_id = Some("loan".into());
        payment.transfer_status = Some(Cleared::Uncleared);
        b.transactions.push(payment);
        let ids = std::collections::HashSet::from(["tx".into()]);
        apply_new_payments(&mut b, &ids).unwrap();
        apply_new_payments(&mut b, &ids).unwrap();
        assert_eq!(b.transactions.len(), 2);
        assert_eq!(b.transactions[1].amount, -500);
    }
    #[test]
    fn a_payment_typed_in_a_register_lists_above_its_interest() {
        let mut b = crate::domain::tests::sample();
        b.accounts[1].id = "loan".into();
        b.accounts[1].kind = AccountKind::Tracking;
        b.accounts[1].opening_balance = -100000;
        b.categories.retain(|c| c.payment_account().is_none());
        let mut t = terms();
        t.category_id = "food".into();
        b.extra.insert("loans".into(), json!([t]));
        let mut payment = crate::domain::tests::purchase();
        payment.account_id = "cash".into();
        payment.date = "2026-02-01".into();
        payment.transfer_account_id = Some("loan".into());
        payment.transfer_status = Some(Cleared::Uncleared);
        payment.set_created_at("2026-02-01T09:00:00.000Z");
        b.transactions.push(payment);
        apply_new_payments(&mut b, &std::collections::HashSet::from(["tx".into()])).unwrap();
        let interest = b.transactions.iter().find(|t| t.id.starts_with("loan-interest:")).unwrap();
        assert_eq!(interest.created_at(), Some("2026-02-01T08:59:59.999Z"));
        let mut newest: Vec<&Transaction> = b.transactions.iter().collect();
        sort_newest_first(&mut newest);
        assert_eq!(newest[0].id, "tx");
    }

    #[test]
    fn imported_parent_metadata_does_not_change_which_loan_payment_is_new() {
        let mut b = crate::domain::tests::sample();
        b.accounts[1].id = "loan".into();
        b.accounts[1].kind = AccountKind::Tracking;
        b.accounts[1].opening_balance = -100000;
        b.categories.retain(|c| c.payment_account().is_none());
        let mut terms = terms();
        terms.category_id = "food".into();
        b.extra.insert("loans".into(), json!([terms]));
        let mut payment = crate::domain::tests::purchase();
        payment.account_id = "cash".into();
        payment.transfer_account_id = Some("loan".into());
        payment.transfer_status = Some(Cleared::Uncleared);
        payment.extra.insert("parentId".into(), json!("unrelated-import-id"));
        let mut old = payment.clone();
        old.id = "old".into();
        old.amount = -20000;
        old.splits[0].amount = old.amount;
        old.extra.insert("parentId".into(), json!("tx"));
        b.transactions.extend([old, payment]);
        apply_new_payments(&mut b, &std::collections::HashSet::from(["tx".into()])).unwrap();
        let charge = b.transactions.iter().find(|t| t.id == "loan-interest:loan:2026-10").unwrap();
        assert_eq!(charge.amount, -800, "interest uses the balance after the old payment, before the new one");
    }

    #[test]
    fn new_loan_payments_charge_the_same_interest_from_either_transfer_side() {
        for reversed in [false, true] {
            for split in [false, true] {
                let mut b = crate::domain::tests::sample();
                b.accounts[1].id = "loan".into();
                b.accounts[1].kind = AccountKind::Tracking;
                b.accounts[1].opening_balance = -100000;
                b.categories.retain(|c| c.payment_account().is_none());
                let mut t = terms();
                t.category_id = "food".into();
                b.extra.insert("loans".into(), json!([t]));
                let mut payment = crate::domain::tests::purchase();
                payment.account_id = if reversed { "loan" } else { "cash" }.into();
                let peer = if reversed { "cash" } else { "loan" };
                let sign = if reversed { 1 } else { -1 };
                payment.amount = sign * 10000;
                payment.splits[0].amount = payment.amount;
                payment.transfer_account_id = Some(peer.into());
                payment.transfer_status = Some(Cleared::Cleared);
                payment.set_created_at("2026-10-15T09:00:00.000Z");
                if split {
                    payment.transfer_account_id = None;
                    payment.transfer_status = None;
                    payment.splits = [4000, 6000].into_iter().map(|amount| Split {
                        category_id: Some("food".into()),
                        amount: sign * amount,
                        extra: json!({"transferAccountId": peer, "transferStatus": "cleared"})
                            .as_object().unwrap().clone().into(),
                    }).collect();
                }
                b.transactions.push(payment);
                validate(&b).unwrap();
                let ids = std::collections::HashSet::from(["tx".into()]);
                apply_new_payments(&mut b, &ids).unwrap();
                apply_new_payments(&mut b, &ids).unwrap();
                assert_eq!(b.transactions.len(), 2, "reversed={reversed}, split={split}");
                let charge = b.transactions.iter().find(|p| p.id == "loan-interest:loan:2026-10").unwrap();
                assert_eq!(charge.amount, -1000);
                assert_eq!(charge.created_at(), Some("2026-10-15T08:59:59.999Z"));
                let summary = calculate(&b, "2026-10").unwrap();
                assert_eq!(summary.accounts.iter().find(|a| a.id == "loan").unwrap().balance, -91000);
                assert_eq!(summary.accounts.iter().find(|a| a.id == "cash").unwrap().balance, 90000);
                assert_eq!(summary.categories[0].activity, -10000);
                let mut later = b.transactions[0].clone();
                later.id = "later-payment".into();
                later.date = "2026-10-20".into();
                later.amount /= 2;
                for part in &mut later.splits { part.amount /= 2; }
                later.set_created_at("2026-10-20T09:00:00.000Z");
                b.transactions.push(later);
                apply_new_payments(&mut b, &std::collections::HashSet::from(["later-payment".into()])).unwrap();
                assert_eq!(b.transactions.len(), 3, "a second payment must not charge interest again");
                let summary = calculate(&b, "2026-10").unwrap();
                assert_eq!(summary.accounts.iter().find(|a| a.id == "loan").unwrap().balance, -86000);
            }
        }
    }

    #[test]
    fn loan_inflows_without_a_transfer_and_loan_drawdowns_do_not_post_payment_interest() {
        for transfer in [false, true] {
            let mut b = crate::domain::tests::sample();
            b.accounts[1].id = "loan".into();
            b.accounts[1].kind = AccountKind::Tracking;
            b.accounts[1].opening_balance = -100000;
            b.categories.retain(|c| c.payment_account().is_none());
            let mut t = terms();
            t.category_id = "food".into();
            b.extra.insert("loans".into(), json!([t]));
            let mut entry = crate::domain::tests::purchase();
            entry.account_id = "loan".into();
            entry.amount = if transfer { -10000 } else { 10000 };
            entry.splits[0].amount = entry.amount;
            if transfer {
                entry.transfer_account_id = Some("cash".into());
                entry.transfer_status = Some(Cleared::Cleared);
            }
            b.transactions.push(entry);
            apply_new_payments(&mut b, &std::collections::HashSet::from(["tx".into()])).unwrap();
            assert_eq!(b.transactions.len(), 1);
        }
    }
}
