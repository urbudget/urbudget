use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use crate::t;

pub const SCHEMA_VERSION: u32 = 2;
const MAX_AMOUNT: i64 = 1_000_000_000_000;
pub(crate) const MAX_SAFE_TOTAL: i64 = 9_000_000_000_000_000;
pub(crate) const MAX_LEDGER_ENTRIES: usize = 500_000;
pub(crate) const MAX_TRANSACTION_SPLITS: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
    pub schema_version: u32,
    pub name: String,
    pub currency: String,
    pub accounts: Vec<Account>,
    pub categories: Vec<Category>,
    pub transactions: Vec<Transaction>,
    pub assignments: Vec<Assignment>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            extra: Map::new(),
            name: "My budget".into(),
            currency: "SAR".into(),
            accounts: vec![],
            categories: vec![],
            transactions: vec![],
            assignments: vec![],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
    pub id: String,
    pub name: String,
    pub kind: AccountKind,
    pub opening_balance: i64,
    pub archived: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Cash,
    Credit,
    Tracking,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
    pub id: String,
    pub name: String,
    pub group: String,
    pub target: i64,
}
/// A record's fields beyond the ones the app models (an import's source data, links), kept as
/// they arrived. Copies of a budget share them until one copy changes them: on an imported
/// budget they are most of each transaction, and every edit copies the budget at least once.
#[derive(Clone, Debug, Default)]
pub struct Extra(Arc<Map<String, Value>>);
impl Deref for Extra {
    type Target = Map<String, Value>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Extra {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}
impl PartialEq for Extra {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}
impl From<Map<String, Value>> for Extra {
    fn from(map: Map<String, Value>) -> Self {
        Self(Arc::new(map))
    }
}
impl Serialize for Extra {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Extra {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Map::deserialize(deserializer).map(Self::from)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    #[serde(default, flatten)]
    pub extra: Extra,
    pub id: String,
    pub date: String,
    pub account_id: String,
    pub payee: String,
    pub memo: String,
    pub amount: i64,
    pub status: Cleared,
    pub splits: Vec<Split>,
    pub transfer_account_id: Option<String>,
    pub transfer_status: Option<Cleared>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Cleared {
    Uncleared,
    Cleared,
    Reconciled,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Split {
    #[serde(default, flatten)]
    pub extra: Extra,
    pub category_id: Option<String>,
    pub amount: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    #[serde(default, flatten)]
    pub extra: Map<String, Value>,
    pub month: String,
    pub category_id: String,
    pub amount: i64,
}

pub(crate) fn valid_date(value: &str) -> bool {
    value.len() == 10 && NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .is_ok_and(|date| date.to_string() == value)
}
pub fn valid_month(value: &str) -> bool {
    value.len() == 7 && valid_date(&format!("{value}-01"))
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 100
}
fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 200
}
pub(crate) fn valid_money(value: i64) -> bool {
    (-MAX_AMOUNT..=MAX_AMOUNT).contains(&value)
}

/// Bound derived allocations and monthly replay work as well as saved record counts.
/// Multiplication saturates so even a malformed request cannot wrap the size check.
pub(crate) fn validate_history_workload(months: usize, categories: usize, accounts: usize, credit: usize) -> Result<(), String> {
    let rows = months.saturating_mul(categories.saturating_add(accounts).max(1));
    let matrix = categories.saturating_mul(credit);
    if rows > 2_000_000 || matrix > 2_000_000 || months.saturating_mul(matrix) > 50_000_000 {
        return Err(t!("This calculation exceeds the supported history size. Choose a shorter history or reduce the number of accounts and categories."));
    }
    Ok(())
}

pub fn validate(b: &Budget) -> Result<(), String> {
    if !(1..=SCHEMA_VERSION).contains(&b.schema_version) {
        return Err(t!("Unsupported backup format."));
    }
    let currency = b.currency.as_bytes();
    if !valid_text(&b.name) || currency.len() != 3 || !currency.iter().all(u8::is_ascii_uppercase) {
        return Err(t!("Invalid budget name or currency."));
    }
    if b.accounts.len() > 1000
        || b.categories.len() > 5000
        || b.transactions.len() > 200_000
        || b.assignments.len() > 200_000
    {
        return Err(t!("This budget exceeds the supported size."));
    }
    if b.extra.get("calculationAsOfMonth").filter(|v| !v.is_null())
        .is_some_and(|v| !v.as_str().is_some_and(valid_month))
    {
        return Err(t!("Invalid calculation reference month."));
    }
    // The monthly income someone expects, set beside the month's targets in the budget page.
    if b.extra.get("expectedIncome").filter(|v| !v.is_null())
        .is_some_and(|v| !v.as_i64().is_some_and(|n| (0..=MAX_AMOUNT).contains(&n)))
    {
        return Err(t!("Invalid expected income."));
    }
    let mut accounts = HashMap::new();
    let mut categories = HashSet::new();
    let mut payment_accounts = HashSet::new();
    let mut payment_categories = HashSet::new();
    let mut ids = HashSet::new();
    let mut assignment_ids = HashSet::new();
    let mut absolute_total: i128 = 0;
    let mut ledger_entries = 0usize;
    let mut split_count = 0usize;
    for a in &b.accounts {
        if !valid_id(&a.id)
            || !valid_text(&a.name)
            || accounts.insert(a.id.as_str(), a).is_some()
            || !valid_money(a.opening_balance)
        {
            return Err(t!("Account data is invalid or duplicated."));
        }
        absolute_total += i128::from(a.opening_balance).abs();
    }
    for c in &b.categories {
        if !valid_id(&c.id)
            || !valid_text(&c.name)
            || !valid_text(&c.group)
            || !categories.insert(&c.id)
            || !(0..=MAX_AMOUNT).contains(&c.target)
        {
            return Err(t!("Category data is invalid or duplicated."));
        }
        crate::targets::validate_goal(c)?;
        // A weekly target can request its amount five times in one month. Include
        // obligations in the same numeric allowance as the recorded budget money.
        let occurrences = if c.extra.get("goal").is_some_and(|g|
            g["goal_type"] == "NEED" && g["goal_cadence"].as_i64() == Some(2)) { 5 } else { 1 };
        absolute_total += i128::from(c.target) * occurrences;
        if let Some(link) = c.extra.get("paymentAccountId").filter(|v| !v.is_null()) {
            let id = link.as_str().ok_or_else(|| t!("Invalid credit card payment category link."))?;
            if !accounts.get(id).is_some_and(|a| a.on_budget() && a.kind == AccountKind::Credit)
                || !payment_accounts.insert(id)
            {
                return Err(t!("Payment category '{name}' ({category}) must link to a unique on-budget credit account; invalid or duplicate account '{account}'.", name = c.name, category = c.id, account = id));
            }
            payment_categories.insert(&c.id);
        }
    }
    if let Some(a) = b.accounts.iter().find(|a| a.on_budget() && a.kind == AccountKind::Credit
        && !payment_accounts.contains(a.id.as_str()))
    {
        return Err(t!("Credit account '{name}' ({account}) needs one linked payment category. Its payment category is missing from this budget.", name = a.name, account = a.id));
    }
    for t in &b.transactions {
        split_count = split_count.saturating_add(t.splits.len());
        ledger_entries = ledger_entries.saturating_add(if t.splits.iter().any(|s| crate::ledger::transfer(s).is_some()) { t.splits.len() } else { 1 });
        if ledger_entries > MAX_LEDGER_ENTRIES || split_count > MAX_TRANSACTION_SPLITS {
            return Err(t!("This budget exceeds the supported number of transaction parts or expanded transfer entries."));
        }
        if !valid_id(&t.id)
            || !ids.insert(&t.id)
            || !accounts.contains_key(t.account_id.as_str())
            || !valid_date(&t.date)
            || t.payee.len() > 200
            || t.memo.len() > 5000
            || !valid_money(t.amount)
        {
            return Err(t!("A transaction has an invalid date, amount, account, or duplicate ID."));
        }
        absolute_total += i128::from(t.amount).abs();
        if let Some(other) = &t.transfer_account_id {
            if other == &t.account_id || !accounts.contains_key(other.as_str()) || t.transfer_status.is_none() {
                return Err(t!("A transfer must connect two different accounts."));
            }
            if t.splits.is_empty() && accounts[t.account_id.as_str()].on_budget() != accounts[other.as_str()].on_budget() {
                return Err(t!("A transfer crossing the budget boundary needs a category or Unassigned split."));
            }
        }
        if t.transfer_account_id.is_none() || !t.splits.is_empty() {
            if t.splits.is_empty() || t.splits.len() > 100 {
                return Err(t!("A transaction needs at least one category, or Unassigned."));
            }
            let mut total = 0i64;
            for s in &t.splits {
                if s.extra.get("transferAccountId").filter(|v| !v.is_null()).is_some_and(|v| !v.is_string()) {
                    return Err(t!("A split transfer has an invalid account reference."));
                }
                if !valid_money(s.amount)
                    || s.category_id
                        .as_ref()
                        .is_some_and(|id| !categories.contains(id))
                {
                    return Err(t!("A transaction split has an invalid amount or category."));
                }
                if s.category_id.as_ref().is_some_and(|id| payment_categories.contains(id)) {
                    return Err(t!("Card payments must be recorded as transfers, not expense categories."));
                }
                if let Some(other) = crate::ledger::transfer(s) {
                    if t.transfer_account_id.is_some()
                        || other == t.account_id
                        || !accounts.contains_key(other)
                    {
                        return Err(t!("A split transfer must connect two different accounts."));
                    }
                    if let Some(status) = s.extra.get("transferStatus") {
                        serde_json::from_value::<Cleared>(status.clone())
                            .map_err(|_| t!("Invalid split transfer status."))?;
                    }
                }
                total += s.amount;
                absolute_total += i128::from(s.amount).abs();
            }
            if total != t.amount {
                return Err(t!("Split amounts must add up to the transaction total."));
            }
        }
    }
    for a in &b.assignments {
        if !valid_month(&a.month)
            || !categories.contains(&a.category_id)
            || !valid_money(a.amount)
            || !assignment_ids.insert((&a.month, &a.category_id))
        {
            return Err(t!("A monthly assignment is invalid or duplicated."));
        }
        absolute_total += i128::from(a.amount).abs();
    }
    if absolute_total > i128::from(MAX_SAFE_TOTAL) {
        return Err(t!("Budget totals exceed the safe numeric range."));
    }
    crate::schedules::validate_schedules(b)?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub balance: i64,
    pub cleared: i64,
    pub uncleared: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    pub id: String,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    pub carried: i64,
    #[serde(default)]
    pub credit_overspending: i64,
    #[serde(default)]
    pub needed: i64,
    #[serde(default)]
    pub upcoming: i64,
    /// Like-for-like funding amounts for the target meter (monthly funding, or TB balance).
    #[serde(default)]
    pub target_funding_required: i64,
    #[serde(default)]
    pub target_funding_funded: i64,
    /// What the target asks for in this month before carryover and this month's assignments:
    /// a refill's whole amount, a weekly target's weeks, a dated target's monthly share. The
    /// month's targets total these, for comparing with expected income.
    #[serde(default)]
    pub month_target: i64,
    /// When this month's target funding is due, for the target text ("needed by the 31st").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_due: Option<TargetDue>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TargetDue {
    pub date: String,
    /// The deadline recurs on this day of every month ("by the 31st"), rather than being a
    /// date of its own ("by October 1st").
    pub monthly: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FutureShortfall {
    pub month: String,
    pub amount: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub ready_to_assign: i64,
    #[serde(default)]
    pub future_assigned: i64,
    #[serde(default)]
    pub future_ready_to_assign: i64,
    /// Assignment shortfalls by future month, using only money held in this month.
    #[serde(default)]
    pub future_shortfalls: Vec<FutureShortfall>,
    pub assigned: i64,
    pub activity: i64,
    pub available: i64,
    pub income: i64,
    /// Spending this month with no category (negative). It's overspending, YNAB's Uncategorized
    /// row: its cash comes out of Ready to Assign the month after.
    #[serde(default)]
    pub uncategorized: i64,
    /// Of that, what cards spent beyond their own cash (positive): debt, like credit overspending.
    #[serde(default)]
    pub uncategorized_credit: i64,
    pub net_worth: i64,
    #[serde(default)]
    pub imported_snapshot: bool,
    pub accounts: Vec<AccountSummary>,
    pub categories: Vec<CategorySummary>,
}

impl Summary {
    /// Derived requirements include schedules, so validating the stored transactions
    /// alone cannot guarantee that every value sent to JavaScript is a safe integer.
    pub(crate) fn validate_numeric_range(&self) -> Result<(), String> {
        let safe = |n: i64| i128::from(n).abs() <= i128::from(MAX_SAFE_TOTAL);
        let totals_safe = [self.ready_to_assign, self.future_assigned, self.future_ready_to_assign,
            self.assigned, self.activity, self.available, self.income, self.uncategorized, self.uncategorized_credit, self.net_worth].into_iter().all(safe);
        let accounts_safe = self.accounts.iter().all(|a| [a.balance, a.cleared, a.uncleared].into_iter().all(safe));
        let categories_safe = self.categories.iter().all(|c| [c.assigned, c.activity, c.available, c.carried,
            c.credit_overspending, c.needed, c.upcoming, c.target_funding_required, c.target_funding_funded,
            c.month_target].into_iter().all(safe));
        // These columns are also summed in the inspector. Individually safe rows are
        // insufficient when several large obligations share a month.
        let columns_safe = [
            self.categories.iter().map(|c| i128::from(c.needed).abs()).sum::<i128>(),
            self.categories.iter().map(|c| i128::from(c.upcoming).abs()).sum::<i128>(),
            self.categories.iter().map(|c| i128::from(c.target_funding_required).abs()).sum::<i128>(),
            self.categories.iter().map(|c| i128::from(c.month_target).abs()).sum::<i128>(),
        ].into_iter().all(|n| n <= i128::from(MAX_SAFE_TOTAL));
        if !totals_safe || !accounts_safe || !categories_safe || !columns_safe
            || !self.future_shortfalls.iter().all(|s| safe(s.amount)) {
            return Err(t!("Calculated budget totals exceed the safe numeric range."));
        }
        Ok(())
    }
}

pub fn calculate(b: &Budget, month: &str) -> Result<Summary, String> {
    crate::engine::calculate(b, month)
}
impl Account {
    /// Whether the balance counts toward net worth. A tracking account can opt out (e.g. a
    /// receivable you don't want to count yet) with `"netWorth": false`.
    pub fn in_net_worth(&self) -> bool {
        self.extra
            .get("netWorth")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }
    pub fn on_budget(&self) -> bool {
        self.kind != AccountKind::Tracking
            && self
                .extra
                .get("onBudget")
                .and_then(Value::as_bool)
                .unwrap_or(true)
    }
}
impl Category {
    pub fn payment_account(&self) -> Option<&str> {
        self.extra.get("paymentAccountId").and_then(Value::as_str)
    }
}
impl Transaction {
    /// When the transaction was added (`"createdAt"`, ISO-8601 UTC with milliseconds), which
    /// orders transactions on the same date. Ones from before the app kept it, and YNAB's, have
    /// none.
    pub fn created_at(&self) -> Option<&str> {
        self.extra
            .get("createdAt")
            .and_then(Value::as_str)
            .filter(|at| !at.is_empty())
    }
    /// Records when the transaction was added. Whatever adds it sets it once; edits keep it.
    pub fn set_created_at(&mut self, at: &str) {
        self.extra.insert("createdAt".into(), Value::String(at.into()));
    }
}
/// `at` written the way `createdAt` keeps it: ISO-8601 UTC with milliseconds, the same length
/// every time, so the text sorts in time order.
pub fn stamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
/// Now, as a `createdAt`.
pub fn stamp_now() -> String {
    stamp(Utc::now())
}
/// The order transactions were added in, oldest first: by date, then by when each was added (one
/// without a `createdAt` counts as older). Use it with a stable sort over transactions in the
/// budget's order, so ones added at the same moment stay in the order they're saved in. The
/// register shows this order newest first; byAdded in order.ts is the window's copy.
pub fn added_order(a: &Transaction, b: &Transaction) -> Ordering {
    a.date.cmp(&b.date).then_with(|| match (a.created_at(), b.created_at()) {
        // Neither has a time: by id, the order the register always gave them, so upgrading
        // doesn't reshuffle a day.
        (None, None) => a.id.cmp(&b.id),
        (x, y) => x.cmp(&y),
    })
}
/// `list` (in the budget's order) newest first, as the register shows it: the order they were
/// added in, reversed.
pub fn sort_newest_first<T: Borrow<Transaction>>(list: &mut [T]) {
    list.sort_by(|a, b| added_order(a.borrow(), b.borrow()));
    list.reverse();
}

pub fn reconcile(
    mut budget: Budget,
    account_id: &str,
    balance: i64,
    date: &str,
    adjustment_id: &str,
) -> Result<Budget, String> {
    if !valid_money(balance)
        || !valid_date(date)
    {
        return Err(t!("Invalid reconciliation balance or date."));
    }
    let summary = calculate(&budget, &date[..7])?;
    let account = summary
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| t!("Account not found."))?;
    let difference = balance - account.cleared;
    if difference != 0 {
        budget.transactions.push(Transaction {
            extra: Map::from_iter([("createdAt".to_string(), Value::String(stamp_now()))]).into(),
            id: adjustment_id.into(),
            date: date.into(),
            account_id: account_id.into(),
            payee: "Reconciliation adjustment".into(),
            memo: format!(
                "Explicit balance adjustment: cleared balance {} → {} (minor units).",
                account.cleared, balance
            ),
            amount: difference,
            status: Cleared::Cleared,
            splits: vec![Split {
                extra: Map::from_iter([("unassigned".into(), Value::Bool(true))]).into(),
                category_id: None,
                amount: difference,
            }],
            transfer_account_id: None,
            transfer_status: None,
        });
    }
    for t in &mut budget.transactions {
        for s in &mut t.splits {
            if crate::ledger::transfer(s) == Some(account_id)
                && crate::ledger::status(s) == Cleared::Cleared
            {
                s.extra
                    .insert("transferStatus".into(), serde_json::json!("reconciled"));
            }
        }
        if t.account_id == account_id && t.status == Cleared::Cleared {
            t.status = Cleared::Reconciled;
        }
        if t.transfer_account_id.as_deref() == Some(account_id)
            && t.transfer_status == Some(Cleared::Cleared)
        {
            t.transfer_status = Some(Cleared::Reconciled);
        }
    }
    validate(&budget)?;
    Ok(budget)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn canonical_dates_and_reference_months_are_validated_before_slicing() {
        for date in ["2026-1-01", "2026-01-1", "+262142-12-31", "2026-02-30", "2026-1- 1", "ééééé"] {
            assert!(!valid_date(date), "{date}");
        }
        assert!(valid_date("0000-01-01"));
        assert!(valid_date("9999-12-31"));
        let mut b = sample();
        for month in ["2026-1", "2026-13", "2026-10-01", "+262142-12"] {
            b.extra.insert("calculationAsOfMonth".into(), serde_json::json!(month));
            assert!(validate(&b).is_err(), "{month}");
            assert!(crate::engine::calculate_at(&sample(), "2026-10", month).is_err(), "{month}");
        }
    }

    #[test]
    fn expected_income_is_an_optional_non_negative_amount() {
        let mut b = sample();
        for value in [serde_json::json!(null), serde_json::json!(0), serde_json::json!(7_291_710), serde_json::json!(MAX_AMOUNT)] {
            b.extra.insert("expectedIncome".into(), value.clone());
            validate(&b).unwrap_or_else(|e| panic!("{value}: {e}"));
        }
        for value in [serde_json::json!(-1), serde_json::json!(MAX_AMOUNT + 1), serde_json::json!(12.5), serde_json::json!("72917.10")] {
            b.extra.insert("expectedIncome".into(), value.clone());
            assert_eq!(validate(&b).unwrap_err(), "Invalid expected income.", "{value}");
        }
    }

    #[test]
    fn aggregate_weekly_targets_stay_within_javascript_integer_precision() {
        let mut b = Budget::default();
        b.categories = (0..2000).map(|i| Category {
            id: format!("goal-{i}"), name: format!("Goal {i}"), group: "Goals".into(), target: MAX_AMOUNT,
            extra: Map::from_iter([("goal".into(), serde_json::json!({"goal_type":"NEED", "goal_cadence":2}))]),
        }).collect();
        assert!(validate(&b).unwrap_err().contains("safe numeric range"));
        b.categories.truncate(1000);
        validate(&b).unwrap();
    }

    #[test]
    fn expanded_split_transfers_are_bounded_before_allocating_the_ledger() {
        let mut b = sample();
        let mut t = purchase();
        t.amount = 0;
        t.splits = (0..100).map(|_| Split { category_id: None, amount: 0,
            extra: Map::from_iter([("transferAccountId".into(), serde_json::json!("cash"))]).into(),
        }).collect();
        b.transactions = (0..=MAX_LEDGER_ENTRIES / 100).map(|i| {
            let mut t = t.clone(); t.id = format!("large-{i}"); t
        }).collect();
        assert!(validate(&b).unwrap_err().contains("expanded transfer entries"));
    }

    #[test]
    fn history_limits_bound_derived_work_without_overflowing_the_check() {
        validate_history_workload(120, 100, 10, 2).unwrap();
        assert!(validate_history_workload(2400, 1000, 10, 2).is_err());
        assert!(validate_history_workload(120, 1000, 500, 500).is_err());
        assert!(validate_history_workload(usize::MAX, usize::MAX, usize::MAX, usize::MAX).is_err());
        let mut b = sample();
        b.categories = (0..1000).map(|i| Category { id: format!("c{i}"),
            name: format!("Category {i}"), group: "Categories".into(), target: 0, extra: Default::default() }).collect();
        let mut payment = sample().categories[1].clone(); payment.id = "payment".into(); b.categories.push(payment);
        b.assignments = vec![Assignment { extra: Default::default(), month: "1826-01".into(), category_id: "c0".into(), amount: 0 }];
        assert!(calculate(&b, "2026-10").unwrap_err().contains("history size"));
    }

    #[test]
    fn derived_money_limits_cover_future_adjustments_and_summed_inspector_columns() {
        let mut summary = calculate(&sample(), "2026-10").unwrap();
        summary.categories[0].needed = MAX_SAFE_TOTAL;
        summary.categories[1].needed = 1;
        assert!(summary.validate_numeric_range().is_err());
        summary.categories[0].needed = 0;
        summary.future_ready_to_assign = -MAX_SAFE_TOTAL - 1;
        assert!(summary.validate_numeric_range().is_err());
        summary.future_ready_to_assign = -MAX_SAFE_TOTAL;
        summary.validate_numeric_range().unwrap();
    }

    pub use crate::test_support::{sample, purchase};
    #[test]
    fn transactions_on_a_date_go_in_the_order_they_were_added() {
        let at = |id: &str, date: &str, created: Option<&str>| {
            let mut t = purchase();
            t.id = id.into();
            t.date = date.into();
            if let Some(c) = created {
                t.set_created_at(c);
            }
            t
        };
        // In the budget's order: the later-added one saved first, then two from before the app
        // kept the time, which count as older and keep the order they always had, by id.
        let list = [
            at("second", "2026-10-15", Some("2026-10-15T09:00:00.500Z")),
            at("first", "2026-10-15", Some("2026-10-15T09:00:00.100Z")),
            at("legacy-b", "2026-10-15", None),
            at("legacy-a", "2026-10-15", None),
            at("earlier-day", "2026-10-14", Some("2026-10-20T00:00:00.000Z")),
        ];
        let mut sorted: Vec<&Transaction> = list.iter().collect();
        sort_newest_first(&mut sorted);
        let ids: Vec<&str> = sorted.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["second", "first", "legacy-b", "legacy-a", "earlier-day"]);
        assert_eq!(stamp(DateTime::from_timestamp(1_760_901_068, 123_000_000).unwrap()), "2025-10-19T19:11:08.123Z");
    }
    #[test]
    fn copies_share_extra_fields_until_one_changes_them() {
        let mut t = purchase();
        t.extra.insert("source".into(), serde_json::json!({"id": "ynab-1"}));
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["source"]["id"], "ynab-1", "stored beside the modelled fields");
        let mut copy = t.clone();
        assert_eq!(copy, t);
        copy.extra.insert("flag".into(), "red".into());
        copy.splits[0].extra.insert("transferStatus".into(), "cleared".into());
        assert_ne!(copy, t);
        assert!(t.extra.get("flag").is_none() && t.splits[0].extra.is_empty());
        assert_eq!(serde_json::from_value::<Transaction>(json).unwrap(), t);
    }
    #[test]
    fn any_iso_style_currency_code_is_accepted() {
        let mut b = sample();
        for code in ["JPY", "KWD", "CHF"] {
            b.currency = code.into();
            assert!(validate(&b).is_ok(), "{code}");
        }
        for code in ["usd", "US", "EURO", "€"] {
            b.currency = code.into();
            assert!(validate(&b).is_err(), "{code}");
        }
    }
    #[test]
    fn credit_spending_reduces_envelope_without_double_counting() {
        let mut b = sample();
        b.transactions.push(purchase());
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.ready_to_assign, 80_000);
        assert_eq!(s.available, 20_000);
        assert_eq!(s.net_worth, 85_000);
        assert_eq!(s.categories[1].available, 5000);
        assert_eq!(s.ready_to_assign + s.available, s.accounts[0].balance);
    }
    #[test]
    fn transfers_are_conserved_and_clear_independently() {
        let mut b = sample();
        let mut t = purchase();
        t.account_id = "cash".into();
        t.transfer_account_id = Some("card".into());
        t.transfer_status = Some(Cleared::Uncleared);
        t.splits.clear();
        b.transactions.push(t);
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.net_worth, 90_000);
        assert_eq!(s.activity, -5000);
        assert_eq!(s.accounts[0].cleared, 95_000);
        assert_eq!(s.accounts[1].cleared, -10_000);
        assert_eq!(s.accounts[1].uncleared, 5_000);
    }
    #[test]
    fn credit_overspending_resets_and_becomes_unfunded_debt() {
        let mut b = sample();
        let mut t = purchase();
        t.amount = -25_000;
        t.splits[0].amount = t.amount;
        b.transactions.push(t);
        let s = calculate(&b, "2026-11").unwrap();
        assert_eq!(s.categories[0].carried, 0);
        assert_eq!(s.categories[0].assigned, 0);
        assert_eq!(s.categories[0].available, 0);
        assert_eq!(s.categories[1].available, 20_000);
        assert_eq!(s.ready_to_assign, 80_000);
    }
    #[test]
    fn payment_categories_require_unique_on_budget_credit_accounts() {
        for link in [serde_json::json!("missing"), serde_json::json!("cash"), serde_json::json!(17)] {
            let mut b = sample();
            b.categories[1].extra.insert("paymentAccountId".into(), link);
            assert!(validate(&b).is_err());
        }
        let mut b = sample();
        b.categories[0].extra.insert("paymentAccountId".into(), serde_json::json!("card"));
        assert!(validate(&b).is_err(), "duplicate links must not depend on category order");
        b.categories.remove(0);
        assert!(validate(&b).is_err(), "assignments still require an existing category");
        b.assignments.clear();
        assert!(validate(&b).is_ok());
        b.accounts[1].extra.insert("onBudget".into(), serde_json::json!(false));
        assert!(validate(&b).is_err(), "off-budget cards have no payment envelope");
        b.categories.clear();
        assert!(validate(&b).is_ok());
    }
    #[test]
    fn unfunded_card_links_and_payment_expenses_are_rejected_before_calculation() {
        let mut b = sample();
        b.transactions.push(purchase());
        b.categories.pop();
        assert!(calculate(&b, "2026-10").is_err(), "missing reserve must never become free RTA");
        b.categories = sample().categories;
        b.transactions[0].splits[0].category_id = Some("payment".into());
        for account in ["cash", "card"] {
            b.transactions[0].account_id = account.into();
            assert!(validate(&b).is_err(), "payment-category expenses cannot disappear from activity");
        }
    }
    #[test]
    fn boundary_transfers_require_explicit_category_semantics_in_both_directions() {
        let mut b = sample();
        let mut tracking = b.accounts[0].clone();
        tracking.id = "tracking".into();
        tracking.kind = AccountKind::Tracking;
        b.accounts.push(tracking);
        for reverse in [false, true] {
            let mut t = purchase();
            t.account_id = if reverse { "tracking" } else { "cash" }.into();
            t.transfer_account_id = Some(if reverse { "cash" } else { "tracking" }.into());
            t.transfer_status = Some(Cleared::Cleared);
            if reverse { t.amount = -t.amount; t.splits[0].amount = t.amount; }
            b.transactions = vec![t];
            assert!(validate(&b).is_ok());
            b.transactions[0].splits.clear();
            assert!(validate(&b).is_err());
            let amount = b.transactions[0].amount;
            b.transactions[0].splits.push(Split { category_id: None, amount, extra: Default::default() });
            assert!(validate(&b).is_ok(), "an explicit uncategorized split is supported");
        }
    }
    #[test]
    fn rejects_unbalanced_splits_and_bad_references() {
        let mut b = sample();
        let mut t = purchase();
        t.splits[0].amount = -4999;
        b.transactions.push(t);
        assert!(validate(&b).is_err());
        b.transactions[0].splits[0].amount = -5000;
        b.transactions[0].splits[0].category_id = Some("missing".into());
        assert!(validate(&b).is_err());
    }
    #[test]
    fn future_assignments_reserve_cash_without_spending_future_income() {
        let mut b = sample();
        let mut t = purchase();
        t.date = "2026-11-01".into();
        t.amount = 10_000;
        t.splits = vec![Split {
            extra: Default::default(),
            category_id: None,
            amount: 10_000,
        }];
        b.transactions.push(t);
        b.assignments.push(Assignment { extra: Default::default(),
            month: "2026-11".into(),
            category_id: "food".into(),
            amount: 5000,
        });
        assert_eq!(
            crate::engine::calculate_at(&b, "2026-10", "2026-10")
                .unwrap()
                .ready_to_assign,
            75_000
        );
        assert_eq!(calculate(&b, "2026-11").unwrap().ready_to_assign, 75_000);
    }
    #[test]
    fn refunds_restore_their_envelope() {
        let mut b = sample();
        let mut t = purchase();
        t.amount = 5000;
        t.splits[0].amount = 5000;
        b.transactions.push(t);
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.categories[0].available, 25_000);
        assert_eq!(s.income, 0);
    }
    #[test]
    fn reconciliation_adjusts_only_posted_balance() {
        let mut b = sample();
        let mut t = purchase();
        t.status = Cleared::Uncleared;
        b.transactions.push(t);
        let result = reconcile(b, "card", -10_003, "2026-10-15", "adjustment").unwrap();
        let s = calculate(&result, "2026-10").unwrap();
        assert_eq!(s.accounts[1].cleared, -10_003);
        assert_eq!(s.accounts[1].balance, -15_003);
        assert_eq!(result.transactions[0].status, Cleared::Uncleared);
        assert_eq!(result.transactions[1].status, Cleared::Reconciled);
        assert_eq!(result.transactions[1].splits[0].extra.get("unassigned"), Some(&Value::Bool(true)));
        assert_eq!(s.income, -3, "a negative reconciliation is an intentional RTA adjustment");
    }
    #[test]
    fn cash_overspending_reduces_next_month_ready_to_assign() {
        let mut b = sample();
        let mut t = purchase();
        t.account_id = "cash".into();
        t.amount = -25_000;
        t.splits[0].amount = t.amount;
        b.transactions.push(t);
        let current = calculate(&b, "2026-10").unwrap();
        assert_eq!(current.ready_to_assign, 80_000);
        assert_eq!(current.categories[0].available, -5000);
        let next = calculate(&b, "2026-11").unwrap();
        assert_eq!(next.ready_to_assign, 75_000);
        assert_eq!(next.categories[0].available, 0);
    }
    #[test]
    fn budgeted_tracking_transfer_spends_only_once() {
        let mut b = sample();
        let mut asset = b.accounts[0].clone();
        asset.id = "asset".into();
        asset.kind = AccountKind::Tracking;
        asset.opening_balance = 900_000;
        b.accounts.push(asset);
        let mut t = purchase();
        t.account_id = "cash".into();
        t.transfer_account_id = Some("asset".into());
        t.transfer_status = Some(Cleared::Uncleared);
        b.transactions.push(t);
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.ready_to_assign, 80_000);
        assert_eq!(s.categories[0].available, 15_000);
        assert_eq!(s.accounts[2].balance, 905_000);
        assert_eq!(s.net_worth, 990_000);
    }
    #[test]
    fn cash_advance_creates_debt_without_reserving_payment_money() {
        let mut b = sample();
        let mut t = purchase();
        t.transfer_account_id = Some("cash".into());
        t.transfer_status = Some(Cleared::Cleared);
        t.splits.clear();
        b.transactions.push(t);
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.ready_to_assign, 85_000);
        assert_eq!(s.categories[1].available, 0);
    }
    #[test]
    fn positive_credit_balance_spends_as_cash() {
        let mut b = sample();
        b.accounts[1].opening_balance = 10_000;
        b.transactions.push(purchase());
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.ready_to_assign, 90_000);
        assert_eq!(s.categories[1].available, 0);
        assert_eq!(s.categories[0].available, 15_000);
    }
    #[test]
    fn covering_credit_overspending_restores_payment_reserve() {
        let mut b = sample();
        let mut t = purchase();
        t.amount = -25_000;
        t.splits[0].amount = t.amount;
        b.transactions.push(t);
        assert_eq!(
            calculate(&b, "2026-10").unwrap().categories[0].credit_overspending,
            5000
        );
        b.assignments[0].amount = 25_000;
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.categories[0].credit_overspending, 0);
        assert_eq!(s.categories[1].available, 25_000);
    }
    #[test]
    fn cash_spending_gets_priority_when_category_is_underfunded() {
        let mut b = sample();
        b.assignments[0].amount = 5000;
        let card = purchase();
        let mut cash = card.clone();
        cash.id = "cash-tx".into();
        cash.account_id = "cash".into();
        b.transactions = vec![card, cash];
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.categories[0].available, -5000);
        assert_eq!(s.categories[0].credit_overspending, 5000);
        assert_eq!(s.categories[1].available, 0);
    }
    #[test]
    fn positive_card_balance_created_mid_month_is_spent_as_cash() {
        let mut b = sample();
        let mut pay = purchase();
        pay.id = "payment".into();
        pay.date = "2026-10-01".into();
        pay.account_id = "cash".into();
        pay.amount = -20000;
        pay.splits.clear();
        pay.transfer_account_id = Some("card".into());
        pay.transfer_status = Some(Cleared::Cleared);
        b.transactions.push(pay);
        b.transactions.push(purchase());
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.accounts[1].balance, 5000);
        assert_eq!(s.categories[1].activity, -20000);
        assert_eq!(s.categories[0].activity, -5000);
    }
    #[test]
    fn same_day_offsetting_card_entries_do_not_create_cash_spending() {
        let mut b = sample();
        b.accounts[1].opening_balance = 0;
        let mut charge = purchase();
        charge.amount = -230000;
        charge.splits[0].amount = charge.amount;
        let mut refund = charge.clone();
        refund.id = "refund".into();
        refund.amount = 230000;
        refund.splits[0].amount = refund.amount;
        let mut small = purchase();
        small.id = "small".into();
        b.transactions = vec![charge, small, refund];
        let s = calculate(&b, "2026-10").unwrap();
        assert_eq!(s.accounts[1].balance, -5000);
        assert_eq!(s.categories[1].activity, 5000);
        assert_eq!(s.categories[0].activity, -5000);
    }
}
