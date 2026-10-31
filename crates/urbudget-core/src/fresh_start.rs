//! Small pieces of settled history needed after a fresh start, without its transactions.
use crate::t;
use crate::domain::{validate, Budget, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const KEY: &str = "freshStartHistory";

/// Creates the fresh-start budget used by native clients. Settled history is capitalized into
/// opening balances; pending/future transactions (including both sides of transfers) stay
/// editable. Record-level source metadata is reconciled by `prepare` before persistence.
pub fn create(source: &Budget, name: &str, as_of: &str) -> Result<Budget, String> {
    use crate::domain::{Cleared, MAX_SAFE_TOTAL};
    validate(source)?;
    if !crate::domain::valid_date(as_of) { return Err(t!("Choose a valid fresh-start date.")); }
    let keep = |t: &Transaction| {
        t.date.as_str() > as_of || t.status == Cleared::Uncleared || t.extra.get("approved") == Some(&Value::Bool(false))
            || (t.transfer_account_id.is_some() && t.transfer_status.unwrap_or(Cleared::Uncleared) == Cleared::Uncleared)
            || t.splits.iter().any(|s| s.extra.get("transferAccountId").is_some_and(Value::is_string)
                && s.extra.get("transferStatus").and_then(Value::as_str).unwrap_or("uncleared") == "uncleared")
    };
    let mut balances: HashMap<String, i128> = source.accounts.iter().map(|a| (a.id.clone(), a.opening_balance as i128)).collect();
    let mut add = |account: &str, amount: i128| -> Result<(), String> {
        let balance = balances.get_mut(account).ok_or_else(|| t!("A transaction references a missing account."))?;
        *balance += amount;
        if balance.abs() > MAX_SAFE_TOTAL as i128 { return Err(t!("Fresh-start balances exceed the supported amount.")); }
        Ok(())
    };
    for t in source.transactions.iter().filter(|t| !keep(t)) {
        add(&t.account_id, t.amount as i128)?;
        if let Some(peer) = t.transfer_account_id.as_deref() { add(peer, -(t.amount as i128))?; }
        else { for split in &t.splits { if let Some(peer) = split.extra.get("transferAccountId").and_then(Value::as_str) { add(peer, -(split.amount as i128))?; } } }
    }
    let mut next = Budget { name: name.trim().into(), currency: source.currency.clone(), ..Budget::default() };
    for field in ["numberFormat", "dateFormat", "categoryOrder", "groupOrder", "payeeRules", "payees", "loans", "smsImportedMessageIds"] {
        if let Some(v) = source.extra.get(field) { next.extra.insert(field.into(), v.clone()); }
    }
    if let Some(schedules) = source.extra.get("schedules").and_then(Value::as_array) {
        next.extra.insert("schedules".into(), Value::Array(schedules.iter().filter(|s| s["nextDate"].is_string()).cloned().collect()));
    }
    next.accounts = source.accounts.iter().cloned().map(|mut a| {
        a.extra.remove("source"); a.extra.remove("simplefin"); a.opening_balance = balances[&a.id] as i64; a
    }).collect();
    next.categories = source.categories.iter().cloned().map(|mut c| { c.extra.remove("source"); c }).collect();
    next.transactions = source.transactions.iter().filter(|t| keep(t)).cloned().map(|mut t| {
        for split in &mut t.splits { split.extra.remove("source"); }
        t
    }).collect();
    prepare(next, source)
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct History {
    #[serde(default)]
    bank_import_ids: BTreeSet<String>,
    #[serde(default)]
    loan_interest_months: BTreeMap<String, BTreeSet<String>>,
}

/// Opaque bank IDs remain duplicates even after the corresponding transactions are capitalized.
pub fn bank_import_ids(b: &Budget) -> impl Iterator<Item = &str> {
    b.extra.get(KEY)
        .and_then(|h| h.get("bankImportIds"))
        .and_then(Value::as_array)
        .into_iter().flatten().filter_map(Value::as_str)
}

pub fn interest_was_charged(b: &Budget, account: &str, month: &str) -> bool {
    b.extra.get(KEY)
        .and_then(|h| h.get("loanInterestMonths"))
        .and_then(|h| h.get(account))
        .and_then(Value::as_array)
        .is_some_and(|months| months.iter().any(|m| m.as_str() == Some(month)))
}

pub fn is_interest(t: &Transaction) -> bool {
    t.id == format!("loan-interest:{}:{}", t.account_id, &t.date[..7])
        || t.extra.get("source").is_some_and(|s| s["debt_transaction_type"] == "interest")
}

/// Enriches the frontend's fresh-start snapshot using the verified source draft, including
/// source metadata that is intentionally absent from the window. Retained transactions keep
/// their ordinary import identity; only removed transactions become history markers.
pub fn prepare(mut next: Budget, source: &Budget) -> Result<Budget, String> {
    validate(source)?;
    validate(&next)?;
    if next.currency != source.currency {
        return Err(t!("A fresh start must keep the source budget's currency."));
    }
    let mut history: History = serde_json::from_value(
        source.extra.get(KEY).cloned().unwrap_or(json!({})),
    ).map_err(|_| t!("The fresh-start history could not be read."))?;
    let mut kept: HashMap<_, _> = next.transactions.iter_mut().map(|t| (t.id.clone(), t)).collect();
    for t in &source.transactions {
        if let Some(retained) = kept.get_mut(&t.id) {
            if let Some(original) = t.extra.get("source") {
                retained.extra.insert("source".into(), original.clone());
            }
            continue;
        }
        if t.id.starts_with("bank:") {
            history.bank_import_ids.insert(t.id.clone());
        }
        history.bank_import_ids.extend(t.extra.get("bankImportIds")
            .and_then(Value::as_array).into_iter().flatten()
            .filter_map(Value::as_str).map(str::to_owned));
        if is_interest(t) {
            history.loan_interest_months.entry(t.account_id.clone())
                .or_default().insert(t.date[..7].to_owned());
        }
    }
    if !history.bank_import_ids.is_empty() || !history.loan_interest_months.is_empty() {
        next.extra.insert(KEY.into(), json!(history));
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bank_import::{self, BankTransaction, Choice}, domain::*, loans, patch::Patch, store::Store};

    #[test]
    fn native_fresh_start_keeps_pending_and_future_rows_and_capitalizes_only_settled_history() {
        let mut source = crate::domain::tests::sample();
        let mut settled = crate::domain::tests::purchase();
        settled.id = "settled".into(); settled.account_id = "cash".into(); settled.status = Cleared::Cleared;
        settled.date = "2026-10-01".into();
        let mut pending = settled.clone(); pending.id = "pending".into(); pending.status = Cleared::Uncleared;
        let mut future = settled.clone(); future.id = "future".into(); future.date = "2026-11-01".into();
        source.transactions = vec![settled.clone(), pending.clone(), future.clone()];
        source.accounts[0].extra.insert("simplefin".into(), json!({"id":"checking"}));
        let before = calculate(&source, "2026-11").unwrap();
        let next = create(&source, " Fresh start ", "2026-10-21").unwrap();
        assert_eq!(next.name, "Fresh start");
        assert_eq!(next.transactions, [pending, future]);
        assert!(next.assignments.is_empty());
        assert!(next.accounts[0].extra.get("simplefin").is_none());
        assert_eq!(next.accounts[0].opening_balance, source.accounts[0].opening_balance + settled.amount);
        let after = calculate(&next, "2026-11").unwrap();
        for (a,b) in before.accounts.iter().zip(after.accounts) { assert_eq!(a.balance,b.balance); }
        assert!(create(&source, "Fresh start", "2026-99-99").is_err());
    }

    fn capitalized(source: &Budget) -> Budget {
        let summary = calculate(source, "2026-10").unwrap();
        let mut next = source.clone();
        next.name = "Fresh start".into();
        next.transactions.clear();
        next.assignments.clear();
        next.extra.remove(KEY);
        for (account, balance) in next.accounts.iter_mut().zip(summary.accounts) {
            account.opening_balance = balance.balance;
        }
        next
    }

    fn rows(b: &Budget, fit: &str) -> Vec<bank_import::Row> {
        bank_import::review(b, "cash", vec![BankTransaction {
            fit: format!("simplefin:checking:{fit}"), date: "2026-10-15".into(),
            payee: "Shop".into(), memo: "".into(), amount: -5000,
            transfer_account_id: None,
        }]).unwrap()
    }

    fn import(b: Budget, fit: &str) -> Budget {
        let row = rows(&b, fit).remove(0);
        bank_import::apply(b, "cash", vec![Choice { row, action: "new".into(), match_id: None }]).unwrap()
    }

    #[test]
    fn bank_overlap_cannot_replay_capitalized_history_even_after_repeated_fresh_starts() {
        let source = import(crate::domain::tests::sample(), "old");
        let next = prepare(capitalized(&source), &source).unwrap();
        assert!(rows(&next, "old")[0].duplicate);
        assert_eq!(import(next.clone(), "old"), next, "Apply independently enforces duplicate IDs");
        assert!(!rows(&next, "different")[0].duplicate);
        let once_more = import(next, "different");
        let again = prepare(capitalized(&once_more), &once_more).unwrap();
        assert!(rows(&again, "old")[0].duplicate);
        assert!(rows(&again, "different")[0].duplicate);
        assert_eq!(again.accounts[0].opening_balance, source.accounts[0].opening_balance - 10000);
    }

    #[test]
    fn matched_bank_ids_and_history_survive_window_patches_and_reopening() {
        let mut source = crate::domain::tests::sample();
        let mut manual = crate::domain::tests::purchase();
        manual.account_id = "cash".into();
        manual.extra.insert("bankImportIds".into(), json!([rows(&source, "matched")[0].id]));
        source.transactions.push(manual);
        let next = prepare(capitalized(&source), &source).unwrap();
        let window: Budget = serde_json::from_value(serde_json::to_value(crate::store::for_window(&next)).unwrap()).unwrap();
        let mut edited = window.clone();
        edited.name = "Renamed".into();
        let mut patched = next;
        Patch::diff(&window, &edited).apply(&mut patched);
        assert!(rows(&patched, "matched")[0].duplicate);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.db");
        let mut store = Store::open(&path).unwrap();
        let token = store.status().unwrap().token;
        store.save(&patched, &token).unwrap();
        drop(store);
        let reopened = Store::open(&path).unwrap().load().unwrap().budget;
        assert_eq!(reopened, patched);
        assert!(rows(&reopened, "matched")[0].duplicate);
    }

    fn loan() -> (Budget, loans::Terms) {
        let mut b = crate::domain::tests::sample();
        b.accounts.push(Account {
            id: "loan".into(), name: "Loan".into(), kind: AccountKind::Tracking,
            opening_balance: -100000, archived: false, extra: Default::default(),
        });
        let terms = loans::Terms {
            account_id: "loan".into(), category_id: "food".into(),
            annual_rate_basis_points: 1200, monthly_payment: 10000, monthly_fees: 0,
            rate_history: vec![],
        };
        b.extra.insert("loans".into(), json!([terms]));
        (b, terms)
    }

    #[test]
    fn fresh_start_never_charges_the_same_months_loan_interest_twice() {
        let (mut source, terms) = loan();
        loans::record(&mut source, &terms, "cash", 5000, "2026-10-01", "first").unwrap();
        for scheduled in [false, true] {
            let mut next = prepare(capitalized(&source), &source).unwrap();
            let pay = |b: &mut Budget, date: &str, id: &str| {
                if scheduled {
                    let mut payment = source.transactions.iter().find(|t| t.id == "first").unwrap().clone();
                    payment.id = id.into();
                    payment.date = date.into();
                    b.transactions.push(payment);
                    loans::apply_new_payments(b, &std::collections::HashSet::from([id.to_owned()])).unwrap();
                } else {
                    loans::record(b, &terms, "cash", 5000, date, id).unwrap();
                }
            };
            pay(&mut next, "2026-10-20", "second");
            assert_eq!(next.transactions.len(), 1);
            assert!(!next.transactions.iter().any(is_interest));
            pay(&mut next, "2026-11-01", "third");
            assert_eq!(next.transactions.iter().find(|t| t.id == "loan-interest:loan:2026-11").unwrap().amount, -910);
        }
    }

    #[test]
    fn imported_interest_metadata_is_kept_for_both_capitalized_and_pending_entries() {
        let (mut source, terms) = loan();
        let mut charge = crate::domain::tests::purchase();
        charge.id = "ynab-interest".into();
        charge.account_id = "loan".into();
        charge.amount = -1000;
        charge.splits[0].amount = -1000;
        charge.splits[0].category_id = None;
        charge.extra.insert("source".into(), json!({"debt_transaction_type": "interest"}));
        source.transactions.push(charge);

        let mut next = prepare(capitalized(&source), &source).unwrap();
        loans::record(&mut next, &terms, "cash", 5000, "2026-10-20", "payment").unwrap();
        assert_eq!(next.transactions.len(), 1);

        source.transactions[0].status = Cleared::Uncleared;
        let window = crate::store::for_window(&source);
        assert!(window.transactions[0].extra.get("source").is_none());
        let mut next = prepare(window, &source).unwrap();
        assert!(next.transactions[0].extra.contains_key("source"));
        loans::record(&mut next, &terms, "cash", 5000, "2026-10-20", "payment").unwrap();
        assert_eq!(next.transactions.len(), 2, "Retained imported interest also prevents a duplicate charge");
    }
}
