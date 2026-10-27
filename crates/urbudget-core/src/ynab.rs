use crate::{i18n::tr, t};
use crate::domain::*;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| t!("YNAB export is missing {key}.", key))
}
fn rows<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    v[key]
        .as_array()
        .ok_or_else(|| t!("YNAB export is missing {key}.", key))
}
fn source_date<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    let date = text(v, key)?;
    if !valid_date(date) {
        return Err(t!("Invalid YNAB {key} “{date}”: expected a date in YYYY-MM-DD format.", key, date));
    }
    Ok(date)
}
fn source_month(v: &Value) -> Result<&str, String> {
    let date = source_date(v, "month")?;
    if !date.ends_with("-01") { return Err(t!("Invalid month in YNAB export: expected the first day of the month.")); }
    Ok(&date[..7])
}
fn live(v: &&Value) -> bool {
    v["deleted"] != true
}
fn money(v: &Value) -> Result<i64, String> {
    let n = v.as_i64().ok_or_else(|| t!("Invalid YNAB money value."))?;
    if n % 10 != 0 {
        return Err(t!("This export contains sub-cent precision. The original export is preserved; import cannot round your money."));
    }
    let amount = n / 10;
    if !valid_money(amount) { return Err(t!("YNAB amount exceeds the supported money range.")); }
    Ok(amount)
}
/// A milliunit amount YNAB works out, such as a target's shortfall, rounded to the cent it
/// shows. Missing is zero.
fn shown_money(v: &Value) -> i64 {
    let n = v.as_i64().unwrap_or(0);
    n.saturating_add(n.signum() * 5) / 10
}
fn status(v: &Value) -> Result<Cleared, String> {
    serde_json::from_value(v.clone()).map_err(|_| t!("Invalid cleared status."))
}
fn meta(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn is_inflow(c: &Value) -> bool {
    c["internal"] == true && matches!(c["name"].as_str(),
        Some("Inflow: Ready to Assign" | "Inflow: To be Budgeted" | "Ready to Assign" | "To be Budgeted"))
}

/// Recover small accounting facts from retained API metadata once when a budget is loaded.
/// An explicit false records the user's later choice to clear a category and wins over import data.
pub(crate) fn normalize_import_metadata(b: &mut Budget) {
    let plan = b.extra.get("sourceExport").and_then(|v| v.pointer("/data/plan").or_else(|| v.pointer("/data/budget")));
    let source_categories = plan.and_then(|p| p["categories"].as_array());
    let inflow: HashSet<&str> = source_categories.into_iter().flatten().filter(|c| is_inflow(c))
        .filter_map(|c| c["id"].as_str()).collect();
    let known: HashSet<&str> = source_categories.into_iter().flatten()
        .filter_map(|c| c["id"].as_str()).collect();
    let payees = b.extra.get("payees").and_then(Value::as_array)
        .or_else(|| plan.and_then(|p| p["payees"].as_array()));
    let starting_payees: HashSet<&str> = payees.into_iter().flatten()
        .filter(|p| p["name"] == "Starting Balance").filter_map(|p| p["id"].as_str()).collect();
    let original_role = |v: &Value| {
        v["category_id"].as_str().filter(|id| known.contains(id)).map(|id| inflow.contains(id))
            .or_else(|| matches!(v["category_name"].as_str(), Some("Inflow: Ready to Assign" | "Inflow: To be Budgeted")).then_some(true))
    };
    for t in &mut b.transactions {
        let source = t.extra.get("source");
        let was_starting = source.is_some_and(|s| s["payee_name"] == "Starting Balance"
            || s["payee_id"].as_str().is_some_and(|id| starting_payees.contains(id)));
        let single = t.splits.len() == 1;
        let mut opening_role = !t.splits.is_empty();
        for s in &mut t.splits {
            let original = s.extra.get("source").filter(|v| !v.is_null()).or_else(|| single.then_some(source).flatten());
            let role = original.and_then(original_role);
            opening_role &= role == Some(true)
                || (s.category_id.is_none() && s.extra.get("unassigned").and_then(Value::as_bool) == Some(true));
            if s.category_id.is_some() || s.extra.get("unassigned").and_then(Value::as_bool).is_some() { continue; }
            if let Some(unassigned) = role {
                s.extra.insert("unassigned".into(), json!(unassigned));
            }
        }
        if was_starting && opening_role && !t.extra.contains_key("startingBalance") {
            t.extra.insert("startingBalance".into(), json!(true));
        }
    }
    let groups: HashMap<&str, &Value> = b.extra.get("categoryGroups").and_then(Value::as_array)
        .or_else(|| plan.and_then(|p| p["category_groups"].as_array())).into_iter().flatten()
        .filter_map(|g| g["id"].as_str().map(|id| (id, g))).collect();
    for c in &mut b.categories {
        let Some(source) = c.extra.get("source") else { continue };
        let Some(group) = source["category_group_id"].as_str().and_then(|id| groups.get(id)) else { continue };
        // Repair only the old importer's unchanged, name-only association. Native categories,
        // real card-payment groups and subsequently edited associations remain intact.
        if group["name"] != "Credit Card Payments" && c.name == source["name"] && c.group == group["name"]
            && b.accounts.iter().any(|a| a.kind == AccountKind::Credit && a.name == c.name
                && c.payment_account() == Some(a.id.as_str()))
        {
            c.extra.remove("paymentAccountId");
        }
    }
    // Older schedule templates omitted their original category. Match stable provider IDs,
    // not the current payee/amount/order, and retain an explicit subsequent user choice.
    let parents: HashMap<String, bool> = b.extra.get("scheduledTransactions").and_then(Value::as_array)
        .or_else(|| plan.and_then(|p| p["scheduled_transactions"].as_array())).into_iter().flatten()
        .filter_map(|s| Some((s["id"].as_str()?.to_owned(), s["category_id"].as_str()?)))
        .filter(|(_, category)| known.contains(category)).map(|(id, category)| (id, inflow.contains(category))).collect();
    let children: HashMap<(String, String), bool> = b.extra.get("scheduledSubtransactions").and_then(Value::as_array)
        .or_else(|| plan.and_then(|p| p["scheduled_subtransactions"].as_array())).into_iter().flatten()
        .filter_map(|s| Some(((s["scheduled_transaction_id"].as_str()?.to_owned(), s["id"].as_str()?.to_owned()), s["category_id"].as_str()?)))
        .filter(|(_, category)| known.contains(category)).map(|(id, category)| (id, inflow.contains(category))).collect();
    for t in &mut b.transactions {
        let Some(id) = t.extra.get("scheduleId").and_then(Value::as_str) else { continue };
        let single = t.splits.len() == 1;
        for s in &mut t.splits {
            if s.category_id.is_some() || s.extra.get("unassigned").and_then(Value::as_bool).is_some() { continue; }
            let role = s.extra.get("id").and_then(Value::as_str).and_then(|child| children.get(&(id.to_owned(), child.to_owned())))
                .or_else(|| single.then(|| parents.get(id)).flatten());
            if let Some(role) = role { s.extra.insert("unassigned".into(), json!(role)); }
        }
    }
    if let Some(schedules) = b.extra.get_mut("schedules").and_then(Value::as_array_mut) {
        for schedule in schedules {
            let Some(id) = schedule["id"].as_str().map(str::to_owned) else { continue };
            let Some(splits) = schedule.get_mut("transaction").and_then(|t| t.get_mut("splits")).and_then(Value::as_array_mut) else { continue };
            let single = splits.len() == 1;
            for s in splits {
                let Some(s) = s.as_object_mut() else { continue };
                if s.get("categoryId").is_some_and(|v| !v.is_null()) || s.get("unassigned").is_some_and(Value::is_boolean) { continue; }
                let role = s.get("id").and_then(Value::as_str).and_then(|child| children.get(&(id.clone(), child.to_owned())))
                    .or_else(|| single.then(|| parents.get(&id)).flatten());
                if let Some(role) = role { s.insert("unassigned".into(), json!(role)); }
            }
        }
    }
}

fn month_after(month: &str) -> String {
    let (y, m) = (month[..4].parse::<i32>().unwrap_or(0), month[5..7].parse::<u32>().unwrap_or(1));
    if m == 12 { format!("{:04}-01", y + 1) } else { format!("{y:04}-{:02}", m + 1) }
}
/// YNAB adds a loan's interest to its balance every month without a transaction: a twelfth of
/// the annual rate on what's owed as the month starts (a starting balance dated in the month
/// counts), with the current month's estimate already included. It works in milliunits (a
/// thousandth of the currency) and shows the balance to the cent, so a month's charge here is
/// what the running total of interest rounds to, less what earlier months charged. Where a loan in
/// YNAB owes more than its transactions add up to by exactly those charges, they're added as
/// uncleared monthly entries, as recording a loan payment adds them. A month that has an interest
/// transaction of its own keeps it. Any other difference is left for the account check.
fn add_loan_interest(b: &mut Budget, accounts: &[Value]) -> Result<(), String> {
    let last = month_after(&chrono::Local::now().format("%Y-%m").to_string());
    for a in accounts.iter().filter(live).filter(|a| a["on_budget"] == false) {
        let Some(rates) = a["debt_interest_rates"].as_object() else { continue };
        let mut rates: Vec<(&str, i64)> =
            rates.iter().filter_map(|(d, r)| Some((d.get(..7)?, r.as_i64()?))).collect();
        rates.sort();
        let Some(&(_, first_rate)) = rates.first() else { continue };
        let id = text(a, "id")?;
        let ledger = crate::ledger::transactions(b);
        // (date, amount for the loan, whether it's the starting balance)
        let moves: Vec<(&str, i64, bool)> = ledger.iter().filter_map(|t| {
            let amount = if t.account_id == id { t.amount }
                else if t.transfer_account_id.as_deref() == Some(id) { -t.amount }
                else { return None };
            Some((t.date.as_str(), amount, t.account_id == id && t.payee == "Starting Balance"))
        }).collect();
        let charged: HashSet<&str> = ledger.iter()
            .filter(|t| t.account_id == id && crate::fresh_start::is_interest(t))
            .map(|t| &t.date[..7]).collect();
        let owed_more = moves.iter().map(|m| m.1).sum::<i64>() - shown_money(&a["balance"]);
        let Some(mut month) = moves.iter().map(|m| m.0[..7].to_owned()).min() else { continue };
        // Interest so far in milliunits, and in the cents charged for it.
        let (mut charges, mut interest, mut total) = (vec![], 0i64, 0i64);
        while owed_more > 0 && total < owed_more && month <= last {
            let starts = format!("{month}-01");
            let owed = -(moves.iter()
                .filter(|m| m.0 < starts.as_str() || (m.2 && m.0.starts_with(month.as_str())))
                .map(|m| m.1).sum::<i64>() * 10 - interest);
            let rate = rates.iter().rev().find(|(d, _)| *d <= month.as_str()).map_or(first_rate, |r| r.1);
            if !charged.contains(month.as_str()) {
                interest += ((owed.max(0) as i128 * rate as i128 + 600_000) / 1_200_000) as i64;
            }
            let charge = (interest + 5) / 10 - total;
            if charge > 0 {
                charges.push((month.clone(), charge));
                total += charge;
            }
            month = month_after(&month);
        }
        drop(ledger);
        if owed_more <= 0 || total != owed_more { continue }
        for (month, charge) in charges {
            let named = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
                .map_or(month.clone(), |d| d.format("%B %Y").to_string());
            b.transactions.push(Transaction {
                id: format!("loan-interest:{id}:{month}"),
                date: format!("{month}-01"),
                account_id: id.into(),
                payee: "Loan interest".into(),
                memo: format!("Interest YNAB added for {named}"),
                amount: -charge,
                status: Cleared::Uncleared,
                splits: vec![Split { category_id: None, amount: -charge, extra: Default::default() }],
                transfer_account_id: None,
                transfer_status: None,
                extra: meta(json!({"loanInterest": true, "ynabInterest": true})).into(),
            });
        }
    }
    Ok(())
}
fn cents(n: i64) -> String {
    format!("{}{}.{:02}", if n < 0 { "−" } else { "" }, n.abs() / 100, n.abs() % 100)
}

/// The budget a YNAB export describes, refused when an account's totals differ from YNAB's.
pub fn import_export(export: &Value) -> Result<Budget, String> {
    let b = import_unverified(export)?;
    // Never compensate for import mismatches by inventing adjustment entries.
    if let Some(diffs) = b.extra.get("importAudit").and_then(|a| a["accountDifferences"].as_array()).filter(|a| !a.is_empty()) {
        let each: Vec<String> = diffs.iter().map(|d| {
            let field = d["field"].as_str().unwrap_or("balance").replace('_', " ");
            let field = match field.as_str() {
                "balance" => tr("balance").to_owned(),
                "cleared balance" => tr("cleared balance").to_owned(),
                "uncleared balance" => tr("uncleared balance").to_owned(),
                _ => field,
            };
            t!("{account} {field} {actual} here, {expected} in YNAB",
                account = d["account"].as_str().unwrap_or(tr("An account")), field,
                actual = cents(d["actual"].as_i64().unwrap_or(0)), expected = cents(d["expected"].as_i64().unwrap_or(0)))
        }).collect();
        return Err(t!("Account totals don't match YNAB, so nothing was imported: {differences}.", differences = each.join(crate::i18n::list_separator())));
    }
    Ok(b)
}
/// Every total in an imported budget's audit that came out different from YNAB's, each tagged
/// with what it's about: an account ("account"), a category in a month ("category") or a
/// month's Ready to Assign ("readyToAssign").
pub fn differences(b: &Budget) -> Vec<Value> {
    let Some(audit) = b.extra.get("importAudit") else { return vec![] };
    [("accountDifferences", "account"), ("categoryDifferences", "category"), ("readyToAssignDifferences", "readyToAssign")]
        .iter()
        .flat_map(|(key, kind)| {
            audit[key].as_array().into_iter().flatten().map(move |d| {
                let mut d = d.clone();
                d["kind"] = json!(kind);
                d
            })
        })
        .collect()
}
/// The budget a YNAB export describes, with its audit attached. Totals that differ from YNAB's
/// are listed there (see `differences`), not refused, so the person can decide.
pub fn import_unverified(export: &Value) -> Result<Budget, String> {
    let p = export
        .pointer("/data/plan")
        .or_else(|| export.pointer("/data/budget"))
        .ok_or_else(|| t!("Choose a full YNAB API export."))?;
    let accounts = rows(p, "accounts")?;
    let categories = rows(p, "categories")?;
    let transactions = rows(p, "transactions")?;
    let groups: HashMap<_, _> = rows(p, "category_groups")?
        .iter()
        .filter(live)
        .map(|g| (g["id"].as_str().unwrap_or(""), g))
        .collect();
    let payees: HashMap<_, _> = rows(p, "payees")?
        .iter()
        .filter(live)
        .map(|g| (g["id"].as_str().unwrap_or(""), g))
        .collect();
    let inflow: HashSet<_> = categories
        .iter()
        .filter(|c| is_inflow(c))
        .filter_map(|c| c["id"].as_str())
        .collect();
    if inflow.is_empty() {
        return Err(t!("Could not identify YNAB’s Ready to Assign category."));
    }
    let mut b = Budget {
        schema_version: SCHEMA_VERSION,
        name: text(p, "name")?.into(),
        currency: text(&p["currency_format"], "iso_code")?.into(),
        ..Budget::default()
    };
    for a in accounts.iter().filter(live) {
        let on_budget = a["on_budget"]
            .as_bool()
            .ok_or_else(|| t!("Missing account classification"))?;
        b.accounts.push(Account {
            id: text(a, "id")?.into(),
            name: text(a, "name")?.into(),
            kind: if !on_budget {
                AccountKind::Tracking
            } else if a["type"] == "creditCard" || a["type"] == "lineOfCredit" {
                AccountKind::Credit
            } else {
                AccountKind::Cash
            },
            opening_balance: 0,
            archived: a["closed"] == true,
            extra: meta(json!({"onBudget":on_budget,"note":a["note"],"source":a})),
        });
    }
    let is_budget: HashMap<_, _> = b
        .accounts
        .iter()
        .map(|a| (a.id.clone(), a.on_budget()))
        .collect();
    // Validate source relationships before transfer folding indexes either side.
    // A deleted/missing account must be reported even when its transaction would
    // otherwise be consumed as the counterpart of another row.
    for t in transactions.iter().filter(live) {
        let id = text(t, "id")?;
        source_date(t, "date")?;
        let account = text(t, "account_id")?;
        if !is_budget.contains_key(account) {
            return Err(t!("YNAB transaction {id} references missing account {account}.", id, account));
        }
        if !t["transfer_account_id"].is_null() {
            let other = text(t, "transfer_account_id")?;
            if !is_budget.contains_key(other) {
                return Err(t!("YNAB transaction {id} references missing transfer account {other}.", id, other));
            }
        }
        if !t["transfer_transaction_id"].is_null() {
            text(t, "transfer_transaction_id")?;
            if t["transfer_account_id"].is_null() {
                return Err(t!("YNAB transaction {id} has a transfer counterpart without a transfer account.", id));
            }
        }
    }
    let mut payment_categories: HashMap<&str, &str> = HashMap::new();
    for c in categories.iter().filter(live) {
        let id = text(c, "id")?;
        if inflow.contains(id) {
            continue;
        }
        let group = c["category_group_id"].as_str().and_then(|id| groups.get(id));
        let payment_group = group.is_some_and(|g| g["name"] == "Credit Card Payments");
        // Unused internal spending categories need no envelope, but payment categories
        // are required even for a new or closed card with no categorized transactions.
        if c["internal"] == true && !payment_group && !transactions.iter().any(|t| t["category_id"] == id) {
            continue;
        }
        let group = group.ok_or_else(|| t!("Missing category group"))?;
        let name = text(c, "name")?;
        // The public API has no account-to-payment-category ID. A name is usable
        // only when it identifies exactly one credit account and one category.
        let payment = if payment_group {
            let matches: Vec<_> = b.accounts.iter()
                .filter(|a| a.on_budget() && a.kind == AccountKind::Credit && a.name == name)
                .collect();
            match matches.as_slice() {
                [] => return Err(t!("YNAB payment category “{name}” ({id}) has no matching on-budget credit account. Check the account and payment category names in YNAB, then export again.", name, id)),
                [account] => {
                    if let Some(other) = payment_categories.insert(account.id.as_str(), id) {
                        return Err(t!("YNAB payment categories “{name}” ({other} and {id}) both match credit account “{account}” ({account_id}). The export does not identify which category belongs to the account.", name, other, id, account = account.name, account_id = account.id));
                    }
                    Some(*account)
                }
                _ => {
                    let ids = matches.iter().map(|a| a.id.as_str()).collect::<Vec<_>>().join(", ");
                    return Err(t!("YNAB payment category “{name}” ({id}) matches multiple on-budget credit accounts ({ids}). Give those accounts distinct names in YNAB, then export again.", name, id, ids));
                }
            }
        } else { None };
        let goal: Map<_, _> = c
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.starts_with("goal_"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        b.categories.push(Category{id:id.into(),name:name.into(),group:text(group,"name")?.into(),target:money(c.get("goal_target").filter(|v|!v.is_null()).unwrap_or(&json!(0)))?,extra:meta(json!({"hidden":c["hidden"]==true||group["hidden"]==true,"note":c["note"],"paymentAccountId":payment.map(|a|&a.id),"goal":if c["goal_type"].is_null(){Value::Null}else{Value::Object(goal)},"source":c}))});
    }
    for account in b.accounts.iter().filter(|a| a.on_budget() && a.kind == AccountKind::Credit) {
        if !payment_categories.contains_key(account.id.as_str()) {
            return Err(t!("YNAB credit account “{name}” ({id}) has no matching category in Credit Card Payments. Check the account and payment category names in YNAB, then export again.", name = account.name, id = account.id));
        }
    }
    let known: HashSet<_> = b.categories.iter().map(|c| c.id.clone()).collect();
    let category = |v: &Value| -> Result<Option<String>, String> {
        match v.as_str() {
            None => Ok(None),
            Some(id) if inflow.contains(id) => Ok(None),
            Some(id) if known.contains(id) => Ok(Some(id.into())),
            Some(id) => Err(t!("Unknown category {id}.", id)),
        }
    };
    let mut txmap = HashMap::new();
    for t in transactions.iter().filter(live) {
        let id = text(t, "id")?;
        if txmap.insert(id, t).is_some() { return Err(t!("Duplicate YNAB transaction ID {id}.", id)); }
    }
    let mut subs: HashMap<&str, Vec<&Value>> = HashMap::new();
    // Each split transfer leg's counterpart in the other account. A plan export can leave the
    // leg's transfer_transaction_id empty (seen on a live YNAB plan); the counterpart is then the
    // one transaction there on the same date for the opposite amount that names the leg, its
    // parent, or nothing.
    let mut leg_peers: HashMap<&str, &str> = HashMap::new();
    let mut used_peers = HashSet::new();
    let mut unresolved = vec![];
    let mut leg_ids = HashSet::new();
    // A deleted split's lines can outlive it in a merged delta; they go with it.
    let deleted: HashSet<&str> = transactions.iter().filter(|t| !live(t)).filter_map(|t| t["id"].as_str()).collect();
    for s in rows(p, "subtransactions")?.iter().filter(live) {
        let leg = text(s, "id")?;
        let parent = text(s, "transaction_id")?;
        if deleted.contains(parent) && !txmap.contains_key(parent) {
            continue;
        }
        if !leg_ids.insert(leg) {
            return Err(t!("Duplicate YNAB split ID {leg}.", leg));
        }
        let Some(owner) = txmap.get(parent).copied() else {
            return Err(t!("YNAB split {leg} references missing transaction {parent}.", leg, parent));
        };
        if !s["transfer_account_id"].is_null() {
            let account = text(s, "transfer_account_id")?;
            if !is_budget.contains_key(account) {
                return Err(t!("YNAB split {leg} references missing transfer account {account}.", leg, account));
            }
            if let Some(peer) = s["transfer_transaction_id"].as_str() {
                if peer == parent || !txmap.contains_key(peer) {
                    return Err(t!("YNAB split {leg} references missing or invalid transfer counterpart {peer}.", leg, peer));
                }
                if !used_peers.insert(peer) {
                    return Err(t!("YNAB split {leg} reuses transfer counterpart {peer}, already used by another split.", leg, peer));
                }
                leg_peers.insert(leg, peer);
            } else {
                unresolved.push((s, owner));
            }
        } else if !s["transfer_transaction_id"].is_null() {
            return Err(t!("YNAB split {leg} has a transfer counterpart without a transfer account.", leg));
        }
        subs.entry(parent).or_default().push(s)
    }
    // Reserve every explicit counterpart before inferring missing links, regardless of the
    // source order. Equal-valued legs must not compete for a peer another leg already names.
    for (s, owner) in unresolved {
        let (leg, parent, account) = (text(s, "id")?, text(s, "transaction_id")?, text(s, "transfer_account_id")?);
        let amount = -money(&s["amount"])?;
        let candidates: Vec<&Value> = transactions.iter().filter(live).filter(|u| {
            u["account_id"] == account
                && u["transfer_account_id"] == owner["account_id"]
                && u["date"] == owner["date"]
                && money(&u["amount"]).ok() == Some(amount)
                && (u["transfer_transaction_id"].is_null()
                    || u["transfer_transaction_id"] == leg
                    || u["transfer_transaction_id"] == parent)
                && u["id"].as_str().is_some_and(|id| id != parent && !used_peers.contains(id))
        }).collect();
        let naming: Vec<&Value> = candidates.iter().copied().filter(|u| u["transfer_transaction_id"] == leg).collect();
        let peer = match (naming.as_slice(), candidates.as_slice()) {
            ([u], _) | ([], [u]) => text(u, "id")?,
            _ => return Err(t!("YNAB split {leg} is a transfer to account {account}, but its counterpart there can't be identified. No data was written.", leg, account)),
        };
        used_peers.insert(peer);
        leg_peers.insert(leg, peer);
    }
    // Counterparts of split legs are folded into their parent regardless of API order.
    let split_peers: HashSet<&str> = leg_peers.values().copied().collect();
    let mut consumed = HashSet::new();
    let mut pairs = 0usize;
    for original in transactions.iter().filter(live) {
        if consumed.contains(text(original, "id")?) || split_peers.contains(text(original, "id")?) {
            continue;
        }
        let mut t = original;
        let mut counterpart = None;
        if let Some(otherid) = t["transfer_transaction_id"].as_str() {
            let other = *txmap
                .get(otherid)
                .ok_or_else(|| t!("A transfer is missing its counterpart."))?;
            if other["transfer_transaction_id"] != t["id"]
                || other["transfer_account_id"] != t["account_id"]
                || money(&other["amount"])? != -money(&t["amount"])?
                || other["date"] != t["date"]
            {
                return Err(t!("Inconsistent transfer pair. The source export is preserved for review."));
            }
            let budget_side = *is_budget.get(text(t, "account_id")?).ok_or_else(|| t!("Missing transfer account"))?;
            let other_budget = *is_budget.get(text(other, "account_id")?).ok_or_else(|| t!("Missing transfer account"))?;
            if (!budget_side && other_budget)
                || (budget_side == other_budget && money(&t["amount"])? > 0)
            {
                t = other;
                counterpart = Some(original)
            } else {
                counterpart = Some(other)
            }
            consumed.insert(text(counterpart.unwrap(), "id")?.to_string());
            pairs += 1;
        } else if !t["transfer_account_id"].is_null() {
            return Err(t!("Split transfers require review before import; no data was written."));
        }
        consumed.insert(text(t, "id")?.to_string());
        let amount = money(&t["amount"])?;
        let mut splits = vec![];
        if let Some(children) = subs.get(text(t, "id")?) {
            for s in children {
                let mut extra = meta(
                    json!({"id":s["id"],"memo":s["memo"],"payee":s["payee_id"].as_str().and_then(|id|payees.get(id)).map(|p|p["name"].clone()),"source":s}),
                );
                if s["category_id"].as_str().is_some_and(|id| inflow.contains(id)) {
                    extra.insert("unassigned".into(), json!(true));
                }
                if let Some(other) = s["transfer_account_id"].as_str() {
                    let peer = leg_peers
                        .get(text(s, "id")?)
                        .and_then(|id| txmap.get(id))
                        .ok_or_else(|| t!("A split transfer is missing its counterpart."))?;
                    // A counterpart found by date and amount (above) may name nothing back.
                    let names_back = !peer["transfer_transaction_id"].is_null() || !s["transfer_transaction_id"].is_null();
                    if peer["account_id"] != other
                        || peer["transfer_account_id"] != t["account_id"]
                        || (names_back
                            && peer["transfer_transaction_id"] != s["id"]
                            && peer["transfer_transaction_id"] != t["id"])
                        || peer["date"] != t["date"]
                        || money(&peer["amount"])? != -money(&s["amount"])?
                    {
                        return Err(t!("Inconsistent split transfer counterpart."));
                    }
                    extra.insert("transferAccountId".into(), json!(other));
                    extra.insert("transferStatus".into(), json!(status(&peer["cleared"])?));
                    extra.insert("transferSource".into(), (*peer).clone());
                    pairs += 1;
                }
                splits.push(Split {
                    category_id: category(&s["category_id"])?,
                    amount: money(&s["amount"])?,
                    extra: extra.into(),
                });
            }
        } else if counterpart.is_none()
            || is_budget.get(text(t, "account_id")?).ok_or_else(|| t!("Missing transaction account"))?
                != is_budget.get(text(counterpart.unwrap(), "account_id")?).ok_or_else(|| t!("Missing transfer account"))?
        {
            splits.push(Split {
                category_id: category(&t["category_id"])?,
                amount,
                extra: if t["category_id"].as_str().is_some_and(|id| inflow.contains(id)) {
                    meta(json!({"unassigned": true})).into()
                } else { Default::default() },
            })
        }
        let payee = t["payee_id"]
            .as_str()
            .and_then(|id| payees.get(id))
            .and_then(|p| p["name"].as_str())
            // Interest YNAB records on a loan (once it's edited there) has no payee.
            .unwrap_or(if t["debt_transaction_type"] == "interest" { "Loan interest" } else { "" })
            .to_string();
        b.transactions.push(Transaction{id:text(t,"id")?.into(),date:text(t,"date")?.into(),account_id:text(t,"account_id")?.into(),payee,memo:t["memo"].as_str().unwrap_or("").into(),amount,status:status(&t["cleared"])?,splits,transfer_account_id:counterpart.map(|o|text(o,"account_id").map(String::from)).transpose()?,transfer_status:counterpart.map(|o|status(&o["cleared"])).transpose()?,extra:meta(json!({"flag":t["flag_color"],"approved":t["approved"],"source":t,"transferSource":counterpart})).into()});
    }
    let mut month_records = vec![];
    for m in rows(p, "months")?.iter().filter(live) {
        let month = source_month(m)?;
        for c in rows(m, "categories")?.iter().filter(live) {
            let id = text(c, "id")?;
            if known.contains(id) {
                b.assignments.push(Assignment { extra: Default::default(),
                    month: month.into(),
                    category_id: id.into(),
                    amount: money(&c["budgeted"])?,
                })
            }
        }
        month_records.push(m.clone());
    }
    b.extra = meta(
        json!({"ynabPlanId":p["id"],"importedAt":chrono::Utc::now().to_rfc3339(),"sourceSha256":format!("{:x}",Sha256::digest(serde_json::to_vec(export).map_err(|e|e.to_string())?)),"sourceExport":export,"ynabMonths":month_records,"payees":p["payees"],"scheduledTransactions":p["scheduled_transactions"],"scheduledSubtransactions":p["scheduled_subtransactions"],"categoryGroups":p["category_groups"],"currencyFormat":p["currency_format"],"dateFormat":p["date_format"],"importCounts":{"sourceTransactions":transactions.iter().filter(live).count(),"transferPairs":pairs,"localTransactions":b.transactions.len(),"splits":rows(p,"subtransactions")?.iter().filter(live).count()}}),
    );
    let schedules = crate::schedules::from_ynab(export, &b)?;
    b.extra.insert("schedules".into(), json!(schedules));
    add_loan_interest(&mut b, accounts)?;
    normalize_import_metadata(&mut b);
    b.assignments
        .sort_by(|a, c| (&a.month, &a.category_id).cmp(&(&c.month, &c.category_id)));
    validate(&b)?;
    let audit = audit(&b)?;
    b.extra.insert("importAudit".into(), audit);
    Ok(b)
}

pub fn audit(b: &Budget) -> Result<Value, String> {
    let source = b
        .extra
        .get("sourceExport")
        .ok_or_else(|| t!("No source export attached."))?;
    let p = source
        .pointer("/data/plan")
        .or_else(|| source.pointer("/data/budget"))
        .ok_or_else(|| t!("Missing plan"))?;
    let mut account_diffs = vec![];
    let mut category_diffs = vec![];
    let mut ready_diffs = vec![];
    let mut category_checks = 0;
    // Differences from what YNAB's screens show. Unlike the checks above, these don't stop an
    // import: they include known differences in how the two apps present the same money.
    let mut shown_ready_diffs = vec![];
    let mut age_diffs = vec![];
    let mut underfunded_diffs = vec![];
    let months = rows(p, "months")?.iter().filter(live)
        .map(|m| Ok((source_month(m)?, m))).collect::<Result<Vec<_>, String>>()?;
    let first = months.iter().map(|(month, _)| *month).min().ok_or_else(|| t!("Missing plan months"))?;
    let last = months.iter().map(|(month, _)| *month).max().ok_or_else(|| t!("Missing plan months"))?;
    let history = crate::engine::calculate_history(b, last)?;
    let as_of = crate::engine::calculation_month(b);
    let ages = crate::reports::calculate(b, &crate::reports::Filter {
        from: first.into(), to: last.into(), account_ids: vec![], category_ids: vec![],
    })?.age_of_money_by_month;
    // YNAB's budget screen holds back what later months already have assigned from every month.
    let mut ynab_assigned = BTreeMap::<&str, i64>::new();
    for (month, m) in &months {
        for c in rows(m, "categories")?.iter().filter(live) {
            *ynab_assigned.entry(*month).or_default() += shown_money(&c["budgeted"]);
        }
    }
    let s = history
        .values()
        .next_back()
        .ok_or_else(|| t!("Missing calculated months"))?;
    for a in rows(p, "accounts")?.iter().filter(live) {
        let result = s
            .accounts
            .iter()
            .find(|x| Some(x.id.as_str()) == a["id"].as_str())
            .ok_or_else(|| t!("Account omitted"))?;
        for (field, got) in [
            ("balance", result.balance),
            ("cleared_balance", result.cleared),
            ("uncleared_balance", result.uncleared),
        ] {
            // A loan's balance carries YNAB's interest to a milliunit; it shows it to the cent.
            let expected = if a["debt_interest_rates"].as_object().is_some_and(|r| !r.is_empty()) {
                shown_money(&a[field])
            } else {
                money(&a[field])?
            };
            if got != expected {
                account_diffs.push(
                    json!({"account":a["name"],"field":field,"expected":expected,"actual":got}),
                )
            }
        }
    }
    for (month, m) in months {
        let summary = history.get(month).ok_or_else(|| t!("Missing calculated month"))?;
        let expected = money(&m["to_be_budgeted"])?;
        if summary.ready_to_assign != expected {
            ready_diffs
                .push(json!({"month":month,"expected":expected,"actual":summary.ready_to_assign}))
        }
        let later = ynab_assigned.iter().filter(|(m, _)| **m > month).map(|(_, n)| n).sum();
        let shown = crate::engine::net_of_future(expected, later);
        let here = crate::engine::hold_back_future(b, month, &as_of, summary.clone())?.ready_to_assign;
        if here != shown {
            shown_ready_diffs.push(json!({"month":month,"expected":shown,"actual":here}))
        }
        let (expected_age, age) = (m["age_of_money"].as_i64(), ages.get(month).copied().flatten());
        // YNAB's API has no age of money for months after the current one.
        let unreported = expected_age.is_none() && month > as_of.as_str();
        if expected_age != age && !unreported {
            age_diffs.push(json!({"month":month,"expected":expected_age,"actual":age}))
        }
        for c in rows(m, "categories")?.iter().filter(live) {
            if let Some(got) = summary
                .categories
                .iter()
                .find(|x| Some(x.id.as_str()) == c["id"].as_str())
            {
                category_checks += 1;
                for (field, value) in [
                    ("budgeted", got.assigned),
                    ("activity", got.activity),
                    ("balance", got.available),
                ] {
                    let expected = money(&c[field])?;
                    if expected != value {
                        category_diffs.push(json!({"month":month,"category":c["name"],"field":field,"expected":expected,"actual":value}))
                    }
                }
                // YNAB shows overspending in place of a target's shortfall; `needed` covers both.
                // A target snoozed for the month asks for nothing on YNAB's screen or in its
                // Auto-Assign, though its API still reports the shortfall.
                let snoozed = c["goal_snoozed_at"].as_str().is_some_and(|at| at.starts_with(month));
                let shortfall = if snoozed { 0 } else { shown_money(&c["goal_under_funded"]) };
                // Without a target, YNAB's screen and Auto-Assign still ask for the month's
                // scheduled spending the category can't cover, which its API leaves out.
                let upcoming = if c["goal_type"].is_null() { got.upcoming - money(&c["balance"])? } else { 0 };
                let underfunded = shortfall.max(-money(&c["balance"])?).max(upcoming).max(0);
                // After the current month YNAB's screen asks for a refill's whole amount, as
                // UrBudget does, while its API still takes the carryover off.
                let refill = c["goal_type"] == "NEED" && c["goal_needs_whole_amount"] == false;
                // Without a target, a card payment category asks here for the card balance its
                // cash doesn't cover, where YNAB's screen shows nothing. That's UrBudget's label.
                let card = c["goal_type"].is_null()
                    && b.categories.iter().any(|x| x.id == got.id && x.payment_account().is_some());
                if got.needed != underfunded && !(refill && month > as_of.as_str()) && !card {
                    underfunded_diffs.push(json!({"month":month,"category":c["name"],"expected":underfunded,"actual":got.needed}))
                }
            }
        }
    }
    Ok(
        json!({"accountChecks":b.accounts.len()*3,"categoryMonthChecks":category_checks,"monthChecks":rows(p,"months")?.len(),"accountDifferences":account_diffs,"categoryDifferences":category_diffs,"readyToAssignDifferences":ready_diffs,"shownReadyToAssignDifferences":shown_ready_diffs,"ageOfMoneyDifferences":age_diffs,"underfundedDifferences":underfunded_diffs}),
    )
}

/// YNAB register CSV structure, including both sides of transfers and split rows.
/// No amount, flag, memo, or cleared status is inferred during export.
pub fn export_register(b: &Budget) -> Result<String, String> {
    export_register_for(b, |_, _| true, false)
}
/// The register CSV for just the records `keep` accepts, given each record's account and its
/// transaction's id (a split transfer's leg gives its parent's id): one account's transactions,
/// or the ones a filter lists. `newest_first` orders them like the register (see
/// `sort_newest_first`); otherwise they're in the budget's order.
pub fn export_register_for(
    b: &Budget,
    keep: impl Fn(&str, &str) -> bool,
    newest_first: bool,
) -> Result<String, String> {
    validate(b)?;
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record([
        "Account",
        "Flag",
        "Date",
        "Payee",
        "Category Group/Category",
        "Category Group",
        "Category",
        "Memo",
        "Outflow",
        "Inflow",
        "Cleared",
    ])
    .map_err(|e| e.to_string())?;
    let amount = |n: i64| format!("{}.{:02}", n / 100, n.abs() % 100);
    let mut txs = crate::ledger::transactions(b);
    if newest_first {
        sort_newest_first(&mut txs);
    }
    for t in &txs {
        let id = t
            .extra
            .get("parentId")
            .and_then(Value::as_str)
            .unwrap_or(&t.id);
        let account = b.accounts.iter().find(|a| a.id == t.account_id).unwrap();
        let peer = t
            .transfer_account_id
            .as_ref()
            .and_then(|id| b.accounts.iter().find(|a| &a.id == id));
        let no_split = Split {
            category_id: None,
            amount: t.amount,
            extra: Default::default(),
        };
        let splits: Vec<_> = if t.splits.is_empty() {
            vec![&no_split]
        } else {
            t.splits.iter().collect()
        };
        for (i, s) in splits.iter().enumerate() {
            if !keep(&account.id, id) {
                break;
            }
            let c = s
                .category_id
                .as_ref()
                .and_then(|id| b.categories.iter().find(|c| &c.id == id));
            let group = c.map(|c| c.group.as_str()).unwrap_or(
                if account.on_budget() && peer.is_none_or(|p| !p.on_budget())
                    && (s.amount >= 0 || crate::ledger::explicitly_unassigned(t, s) || crate::ledger::starting_balance(t)) {
                    "Inflow"
                } else {
                    ""
                },
            );
            let cat = c.map(|c| c.name.as_str()).unwrap_or(if group == "Inflow" {
                "Ready to Assign"
            } else {
                ""
            });
            let memo = if splits.len() > 1 {
                format!(
                    "Split ({}/{}) {}",
                    i + 1,
                    splits.len(),
                    s.extra.get("memo").and_then(Value::as_str).unwrap_or("")
                )
            } else {
                t.memo.clone()
            };
            let payee = peer
                .map(|a| format!("Transfer : {}", a.name))
                .unwrap_or_else(|| {
                    s.extra
                        .get("payee")
                        .and_then(Value::as_str)
                        .filter(|p| !p.is_empty())
                        .unwrap_or(&t.payee)
                        .to_string()
                });
            let flag = t.extra.get("flag").and_then(Value::as_str).unwrap_or("");
            w.write_record([
                account.name.clone(),
                titlecase(flag),
                t.date.clone(),
                payee,
                if group.is_empty() {
                    "".into()
                } else {
                    format!("{group}: {cat}")
                },
                group.into(),
                cat.into(),
                memo,
                amount((-s.amount).max(0)),
                amount(s.amount.max(0)),
                titlecase(&format!("{:?}", t.status)),
            ])
            .map_err(|e| e.to_string())?;
        }
        if let Some(peer) = peer.filter(|p| keep(&p.id, id)) {
            let source = t.extra.get("transferSource").unwrap_or(&Value::Null);
            let memo = source["memo"].as_str().unwrap_or(&t.memo);
            w.write_record([
                peer.name.clone(),
                titlecase(source["flag_color"].as_str().unwrap_or("")),
                t.date.clone(),
                format!("Transfer : {}", account.name),
                "".into(),
                "".into(),
                "".into(),
                memo.into(),
                amount(t.amount.max(0)),
                amount((-t.amount).max(0)),
                titlecase(&format!(
                    "{:?}",
                    t.transfer_status.unwrap_or(Cleared::Uncleared)
                )),
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn titlecase(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + &c.as_str().to_lowercase(),
    }
}

pub fn export_plan(b: &Budget) -> Result<String, String> {
    validate(b)?;
    let mut months: std::collections::BTreeSet<String> = b
        .assignments
        .iter()
        .map(|a| a.month.clone())
        .chain(b.transactions.iter().map(|t| t.date[..7].into()))
        .collect();
    months.insert(chrono::Local::now().format("%Y-%m").to_string());
    let mut w = csv::Writer::from_writer(vec![]);
    w.write_record([
        "Month",
        "Category Group/Category",
        "Category Group",
        "Category",
        "Assigned",
        "Activity",
        "Available",
    ])
    .map_err(|e| e.to_string())?;
    let amount = |n: i64| {
        format!(
            "{}{}.{:02}",
            if n < 0 { "-" } else { "" },
            n.abs() / 100,
            n.abs() % 100
        )
    };
    // Every month's category values are already in this history. Recalculating each prefix
    // makes a long-history export repeat the same accounting work once per output month.
    let history = crate::engine::calculate_history(b, months.last().expect("current month included"))?;
    for month in months {
        let s = history.get(&month).ok_or_else(|| t!("Could not calculate export month"))?;
        let label = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
            .map_err(|e| e.to_string())?
            .format("%b %Y")
            .to_string();
        for c in &b.categories {
            let v = s.categories.iter().find(|v| v.id == c.id).unwrap();
            w.write_record([
                label.clone(),
                format!("{}: {}", c.group, c.name),
                c.group.clone(),
                c.name.clone(),
                amount(v.assigned),
                amount(v.activity),
                amount(v.available),
            ])
            .map_err(|e| e.to_string())?;
        }
    }
    String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_export_history_matches_each_months_independent_category_calculation() {
        let mut b = crate::domain::tests::sample();
        let mut t = crate::domain::tests::purchase();
        t.date = "2026-09-15".into();
        b.transactions.push(t);
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "food".into(), amount: 7000 });
        b.assignments.push(Assignment { extra: Default::default(), month: "2027-01".into(), category_id: "food".into(), amount: 4000 });
        let exported = export_plan(&b).unwrap();
        let mut reader = csv::Reader::from_reader(exported.as_bytes());
        let money = |n: i64| format!("{}{}.{:02}", if n < 0 {"-"} else {""}, n.abs()/100, n.abs()%100);
        for row in reader.records() {
            let row = row.unwrap();
            let day = chrono::NaiveDate::parse_from_str(&format!("01 {}", &row[0]), "%d %b %Y").unwrap();
            let month = day.format("%Y-%m").to_string();
            let expected = calculate(&b, &month).unwrap();
            let category = b.categories.iter().find(|c| c.name == row[3]).unwrap();
            let value = expected.categories.iter().find(|v| v.id == category.id).unwrap();
            assert_eq!((&row[4], &row[5], &row[6]), (money(value.assigned).as_str(), money(value.activity).as_str(), money(value.available).as_str()));
        }
    }
    fn fixture() -> Value {
        let transaction = |id: &str, amount: i64, category: Value| json!({"id":id,"account_id":"cash","date":"2026-10-01","amount":amount,"payee_id":"shop","memo":"Parent memo","cleared":"reconciled","approved":true,"flag_color":"purple","category_id":category,"transfer_account_id":null,"transfer_transaction_id":null,"deleted":false});
        json!({"data":{"plan":{"id":"plan","name":"Example","currency_format":{"iso_code":"SAR"},"date_format":{"format":"YYYY-MM-DD"},"accounts":[{"id":"cash","name":"Cash","type":"checking","on_budget":true,"closed":false,"deleted":false,"balance":80000,"cleared_balance":80000,"uncleared_balance":0}],"category_groups":[{"id":"living","name":"Living","hidden":false,"deleted":false}],"categories":[{"id":"income","name":"Inflow: Ready to Assign","internal":true,"deleted":false},{"id":"food","name":"Food","category_group_id":"living","hidden":false,"deleted":false,"goal_target":20000,"goal_type":"MF","note":"Original category note"}],"payees":[{"id":"shop","name":"Shop","deleted":false}],"transactions":[transaction("start",100000,json!("income")),transaction("split",-20000,json!("split-system-id")),transaction("zero",0,json!("income"))],"subtransactions":[{"id":"split-1","transaction_id":"split","category_id":"food","amount":-15000,"memo":"First part","payee_id":"shop","deleted":false,"transfer_account_id":null},{"id":"split-2","transaction_id":"split","category_id":"food","amount":-5000,"memo":"Second part","payee_id":null,"deleted":false,"transfer_account_id":null}],"scheduled_transactions":[],"scheduled_subtransactions":[],"months":[{"month":"2026-10-01","note":"Month note","to_be_budgeted":80000,"deleted":false,"categories":[{"id":"food","name":"Food","budgeted":20000,"activity":-20000,"balance":0,"deleted":false}]}]}}})
    }
    fn report(b: &Budget) -> crate::reports::Report {
        crate::reports::calculate(b, &crate::reports::Filter {
            from: "2026-10".into(), to: "2026-10".into(), account_ids: vec![], category_ids: vec![],
        }).unwrap()
    }
    #[test]
    fn malformed_source_months_and_transaction_dates_return_errors_without_panicking() {
        for date in ["123456é89", "2026-02-30", "+262142-12-31", "2026-1-01", "2026"] {
            let mut source = fixture();
            source["data"]["plan"]["months"][0]["month"] = json!(date);
            let result = std::panic::catch_unwind(|| import_export(&source));
            assert!(result.is_ok(), "month {date} must not panic");
            assert!(result.unwrap().unwrap_err().contains("month"));
            let mut source = fixture();
            source["data"]["plan"]["transactions"][0]["date"] = json!(date);
            assert!(std::panic::catch_unwind(|| import_export(&source)).unwrap().unwrap_err().contains("date"));
        }
        let mut source = fixture();
        source["data"]["plan"]["months"][0]["month"] = json!("2026-10-02");
        assert!(import_export(&source).unwrap_err().contains("first day"));
    }
    #[test]
    fn auditing_retained_source_metadata_validates_months_before_slicing() {
        let mut b = import_export(&fixture()).unwrap();
        for month in ["123456é89", "x", "2026-99-01"] {
            b.extra.get_mut("sourceExport").unwrap()["data"]["plan"]["months"][0]["month"] = json!(month);
            let result = std::panic::catch_unwind(|| audit(&b));
            assert!(result.is_ok());
            assert!(result.unwrap().unwrap_err().contains("month"));
        }
    }
    fn audit_at(b: &mut Budget, as_of: &str) -> Value {
        b.extra.insert("calculationAsOfMonth".into(), json!(as_of));
        audit(b).unwrap()
    }
    fn source_month_mut<'a>(b: &'a mut Budget, month: usize) -> &'a mut Value {
        &mut b.extra.get_mut("sourceExport").unwrap()["data"]["plan"]["months"][month]
    }
    fn spend(id: &str, date: &str, amount: i64) -> Transaction {
        Transaction {
            id: id.into(), date: date.into(), account_id: "cash".into(), payee: "Shop".into(),
            memo: "".into(), amount, status: Cleared::Cleared,
            splits: vec![Split { category_id: Some("food".into()), amount, extra: Default::default() }],
            transfer_account_id: None, transfer_status: None, extra: Default::default(),
        }
    }
    #[test]
    fn screen_ready_to_assign_holds_back_later_months_assignments_like_ynab() {
        let mut source = fixture();
        // November assigns 100.00 more to Food, so YNAB's screen shows October's 800.00 as 700.00.
        source["data"]["plan"]["months"].as_array_mut().unwrap().push(json!({"month":"2026-11-01","to_be_budgeted":70000,"deleted":false,
            "categories":[{"id":"food","name":"Food","budgeted":10000,"activity":0,"balance":10000,"deleted":false}]}));
        let mut b = import_export(&source).unwrap();
        let a = audit_at(&mut b, "2026-10");
        assert_eq!(a["readyToAssignDifferences"], json!([]));
        assert_eq!(a["shownReadyToAssignDifferences"], json!([]));
        // The audit compares whatever the budget screen shows, which can differ for months
        // before the one the app calculates from.
        let a = audit_at(&mut b, "2026-11");
        let here = crate::engine::calculate(&b, "2026-10").unwrap().ready_to_assign;
        assert_eq!(a["shownReadyToAssignDifferences"],
            if here == 7000 { json!([]) } else { json!([{"month":"2026-10","expected":7000,"actual":here}]) });
        // YNAB never shows less than nothing when later months take more than a month holds.
        source_month_mut(&mut b, 0)["to_be_budgeted"] = json!(5000);
        source_month_mut(&mut b, 1)["to_be_budgeted"] = json!(65000);
        let a = audit_at(&mut b, "2026-10");
        assert_eq!(a["shownReadyToAssignDifferences"], json!([
            {"month":"2026-10","expected":0,"actual":7000},
            {"month":"2026-11","expected":6500,"actual":7000},
        ]));
    }
    #[test]
    fn age_of_money_is_compared_with_ynabs_month_end_value() {
        let mut b = import_export(&fixture()).unwrap();
        assert_eq!(audit_at(&mut b, "2026-10")["ageOfMoneyDifferences"], json!([]));
        // Ten purchases ten days after the money arrived.
        for n in 0..10 {
            b.transactions.push(spend(&format!("later-{n}"), "2026-10-11", -100));
        }
        source_month_mut(&mut b, 0)["age_of_money"] = json!(10);
        assert_eq!(audit_at(&mut b, "2026-10")["ageOfMoneyDifferences"], json!([]));
        source_month_mut(&mut b, 0)["age_of_money"] = json!(11);
        assert_eq!(audit_at(&mut b, "2026-10")["ageOfMoneyDifferences"],
            json!([{"month":"2026-10","expected":11,"actual":10}]));
        source_month_mut(&mut b, 0)["age_of_money"] = Value::Null;
        assert_eq!(audit_at(&mut b, "2026-10")["ageOfMoneyDifferences"],
            json!([{"month":"2026-10","expected":null,"actual":10}]));
    }
    #[test]
    fn underfunded_amounts_compare_with_ynabs_target_shortfall_or_overspending() {
        let mut b = import_export(&fixture()).unwrap();
        assert_eq!(audit_at(&mut b, "2026-10")["underfundedDifferences"], json!([]));
        // Food's target is met here; YNAB says it's 5.00 short.
        source_month_mut(&mut b, 0)["categories"][0]["goal_under_funded"] = json!(5000);
        assert_eq!(audit_at(&mut b, "2026-10")["underfundedDifferences"],
            json!([{"month":"2026-10","category":"Food","expected":500,"actual":0}]));
        // Overspending 3.00 shows in place of a smaller shortfall in both apps.
        b.transactions.push(spend("over", "2026-10-05", -300));
        let food = &mut source_month_mut(&mut b, 0)["categories"][0];
        (food["activity"], food["balance"], food["goal_under_funded"]) = (json!(-23000), json!(-3000), json!(1000));
        assert_eq!(audit_at(&mut b, "2026-10")["underfundedDifferences"], json!([]));
        assert_eq!((shown_money(&json!(4995)), shown_money(&json!(-4995)), shown_money(&Value::Null)), (500, -500, 0));
    }
    #[test]
    fn scheduled_spending_a_category_without_a_target_cant_cover_is_expected_as_ynabs_screen_asks() {
        let mut source = fixture();
        let plan = &mut source["data"]["plan"];
        plan["categories"].as_array_mut().unwrap().push(json!({"id":"fun", "name":"Fun", "category_group_id":"living", "hidden":false, "deleted":false}));
        // 10.00 every month from November 5th. YNAB's November row says "Assign $10.00 more to
        // cover the upcoming transactions" and its Auto-Assign Underfunded offers 10.00, while its
        // API's goal_under_funded is empty, as for any category without a target.
        plan["scheduled_transactions"] = json!([{"id":"bill", "account_id":"cash", "payee_id":"shop", "category_id":"fun",
            "amount":-10000, "date_first":"2026-11-05", "date_next":"2026-11-05", "frequency":"monthly", "deleted":false}]);
        plan["months"].as_array_mut().unwrap().push(json!({"month":"2026-11-01", "to_be_budgeted":80000, "deleted":false, "categories":[
            {"id":"food", "name":"Food", "budgeted":0, "activity":0, "balance":0, "goal_type":"MF", "goal_under_funded":20000, "deleted":false},
            {"id":"fun", "name":"Fun", "budgeted":0, "activity":0, "balance":0, "goal_type":null, "goal_under_funded":null, "deleted":false}]}));
        let mut b = import_export(&source).unwrap();
        assert_eq!(audit_at(&mut b, "2026-10")["underfundedDifferences"], json!([]));
        // Once 10.00 is assigned, neither app asks for more.
        b.assignments.iter_mut().find(|a| a.month == "2026-11" && a.category_id == "fun").unwrap().amount = 1000;
        let fun = &mut source_month_mut(&mut b, 1)["categories"][1];
        (fun["budgeted"], fun["balance"]) = (json!(10000), json!(10000));
        assert_eq!(audit_at(&mut b, "2026-10")["underfundedDifferences"], json!([]));
    }
    #[test]
    fn card_payment_categories_without_a_target_keep_their_label_without_being_flagged() {
        let mut b = import_export(&fixture()).unwrap();
        let sample = crate::domain::tests::sample();
        // The card owes 100.00 and its payment category holds nothing: UrBudget says 100.00 is
        // needed, YNAB's screen and API say nothing.
        b.accounts.push(sample.accounts[1].clone());
        b.categories.push(sample.categories[1].clone());
        source_month_mut(&mut b, 0)["categories"].as_array_mut().unwrap().push(json!({"id":"payment", "name":"Card payment",
            "budgeted":0, "activity":0, "balance":0, "goal_type":null, "goal_under_funded":null, "deleted":false}));
        assert_eq!(audit_at(&mut b, "2026-10")["underfundedDifferences"], json!([]));
        // With a target in YNAB it's compared like any other category.
        source_month_mut(&mut b, 0)["categories"][1]["goal_type"] = json!("NEED");
        assert_eq!(audit_at(&mut b, "2026-10")["underfundedDifferences"],
            json!([{"month":"2026-10","category":"Card payment","expected":0,"actual":10000}]));
    }
    #[test]
    fn audit_leaves_out_what_only_ynabs_api_reports_after_the_current_month() {
        let mut b = import_export(&fixture()).unwrap();
        // Utilities refills up to 120.00. Ten October bills spend 115.20, so 4.80 carries over.
        let mut utilities = b.categories.iter().find(|c| c.id == "food").unwrap().clone();
        (utilities.id, utilities.name, utilities.target) = ("utilities".into(), "Utilities".into(), 12000);
        utilities.extra.insert("goal".into(), json!({"goal_type":"NEED", "goal_cadence":1, "goal_cadence_frequency":1,
            "goal_needs_whole_amount":false, "goal_creation_month":"2026-10-01"}));
        b.categories.push(utilities);
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-10".into(), category_id: "utilities".into(), amount: 12000 });
        let mut pay = b.transactions.iter().find(|t| t.id == "start").unwrap().clone();
        (pay.id, pay.amount) = ("pay".into(), 20000);
        pay.splits.iter_mut().for_each(|s| s.amount = 20000);
        b.transactions.push(pay);
        for n in 0..10 {
            let mut bill = spend(&format!("bill-{n}"), "2026-10-11", -1152);
            bill.splits[0].category_id = Some("utilities".into());
            b.transactions.push(bill);
        }
        let refill = |budgeted: i64, activity: i64, under_funded: i64| json!({"id":"utilities", "name":"Utilities",
            "budgeted":budgeted, "activity":activity, "balance":4800, "goal_type":"NEED", "goal_needs_whole_amount":false,
            "goal_under_funded":under_funded, "deleted":false});
        let october = source_month_mut(&mut b, 0);
        october["age_of_money"] = json!(10);
        october["categories"].as_array_mut().unwrap().push(refill(120000, -115200, 0));
        // November is after the current month: YNAB's screen asks for 120.00 as UrBudget does,
        // but its API says 115.20, and it has no age of money yet.
        let november = json!({"month":"2026-11-01", "to_be_budgeted":0, "age_of_money":null, "deleted":false, "categories":[
            {"id":"food", "name":"Food", "budgeted":0, "activity":0, "balance":0, "goal_under_funded":20000, "deleted":false},
            refill(0, 0, 115200)]});
        b.extra.get_mut("sourceExport").unwrap()["data"]["plan"]["months"].as_array_mut().unwrap().push(november);
        let a = audit_at(&mut b, "2026-10");
        assert_eq!((&a["underfundedDifferences"], &a["ageOfMoneyDifferences"]), (&json!([]), &json!([])));
        // Once November is the current month both are compared again.
        source_month_mut(&mut b, 1)["categories"][1]["goal_under_funded"] = json!(120000);
        let a = audit_at(&mut b, "2026-11");
        assert_eq!(a["underfundedDifferences"], json!([{"month":"2026-11","category":"Utilities","expected":12000,"actual":11520}]));
        assert_eq!(a["ageOfMoneyDifferences"], json!([{"month":"2026-11","expected":null,"actual":10}]));
    }
    #[test]
    fn api_snooze_timestamps_are_preserved_in_target_and_source_metadata() {
        for at in ["2026-10-15T12:34:56Z", "2026-10-15T12:34:56.123456+03:00"] {
            let mut source = fixture();
            source["data"]["plan"]["categories"][1]["goal_snoozed_at"] = json!(at);
            let b = import_export(&source).unwrap();
            let category = b.categories.iter().find(|c| c.id == "food").unwrap();
            assert_eq!(category.extra["goal"]["goal_snoozed_at"], at);
            assert_eq!(category.extra["source"]["goal_snoozed_at"], at);
            assert_eq!(b.extra["sourceExport"], source);
        }
    }
    #[test]
    fn missing_transfer_accounts_and_orphan_splits_are_rejected_before_folding() {
        for reversed in [false, true] {
            let mut source = fixture();
            let p = &mut source["data"]["plan"];
            let mut from = p["transactions"][0].clone();
            from["transfer_account_id"] = json!("missing-account");
            from["transfer_transaction_id"] = json!("peer");
            let mut peer = from.clone(); peer["id"] = json!("peer"); peer["account_id"] = json!("missing-account");
            peer["transfer_account_id"] = json!("cash"); peer["transfer_transaction_id"] = json!("start"); peer["amount"] = json!(-100000);
            p["transactions"] = json!(if reversed { vec![peer, from] } else { vec![from, peer] });
            let error = std::panic::catch_unwind(|| import_export(&source)).unwrap().unwrap_err();
            assert!(error.contains("missing-account") && error.contains("missing"), "{error}");
        }
        let mut source = fixture();
        source["data"]["plan"]["subtransactions"][0]["transaction_id"] = json!("missing-parent");
        let error = import_export(&source).unwrap_err();
        assert!(error.contains("split-1") && error.contains("missing-parent"), "{error}");
        let mut source = fixture();
        source["data"]["plan"]["subtransactions"][0]["transfer_account_id"] = json!("missing-card");
        assert!(import_export(&source).unwrap_err().contains("missing-card"));
    }
    #[test]
    fn duplicate_source_transactions_are_not_silently_consumed_but_deleted_rows_stay_ignored() {
        let mut source = fixture();
        let duplicate = source["data"]["plan"]["transactions"][0].clone();
        source["data"]["plan"]["transactions"].as_array_mut().unwrap().push(duplicate);
        assert!(import_export(&source).unwrap_err().contains("Duplicate YNAB transaction ID start"));
        let deleted = source["data"]["plan"]["transactions"].as_array_mut().unwrap().last_mut().unwrap();
        deleted["deleted"] = json!(true); deleted["account_id"] = json!("missing-account"); deleted["date"] = json!("invalid");
        import_export(&source).unwrap();
    }
    fn card_import_fixture() -> Value {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        p["accounts"].as_array_mut().unwrap().push(json!({"id":"visa","name":"Visa","type":"creditCard","on_budget":true,"balance":0,"cleared_balance":0,"uncleared_balance":0}));
        p["category_groups"].as_array_mut().unwrap().push(json!({"id":"payments","name":"Credit Card Payments","internal":true}));
        p["categories"].as_array_mut().unwrap().push(json!({"id":"visa-pay","name":"Visa","category_group_id":"payments","goal_target":0}));
        source
    }
    #[test]
    fn payment_categories_without_transactions_survive_internal_and_closed_flags() {
        for internal in [false, true] {
            for closed in [false, true] {
                let mut source = card_import_fixture();
                let p = &mut source["data"]["plan"];
                p["categories"][2]["internal"] = json!(internal);
                p["categories"][2]["hidden"] = json!(closed);
                p["accounts"][1]["closed"] = json!(closed);
                // Neither a deleted card nor an off-budget account makes the name ambiguous.
                p["accounts"].as_array_mut().unwrap().push(json!({"id":"old-visa","name":"Visa","type":"creditCard","on_budget":true,"deleted":true}));
                p["accounts"].as_array_mut().unwrap().push(json!({"id":"tracked-visa","name":"Visa","type":"creditCard","on_budget":false,"balance":0,"cleared_balance":0,"uncleared_balance":0}));
                let b = import_export(&source).unwrap();
                assert_eq!(b.categories.iter().find(|c| c.id == "visa-pay").unwrap().payment_account(), Some("visa"));
                assert_eq!(b.accounts.iter().find(|a| a.id == "visa").unwrap().archived, closed);
            }
        }
    }
    #[test]
    fn duplicate_card_names_are_rejected_instead_of_linking_the_first_account() {
        let mut source = card_import_fixture();
        let p = &mut source["data"]["plan"];
        let mut other = p["accounts"][1].clone();
        other["id"] = json!("other-visa");
        p["accounts"].as_array_mut().unwrap().push(other);
        let error = import_export(&source).err().unwrap();
        assert!(error.contains("matches multiple"), "{error}");
        for detail in ["Visa", "visa-pay", "visa", "other-visa"] { assert!(error.contains(detail), "{error}"); }
    }
    #[test]
    fn unmatched_payment_category_is_rejected_with_its_name_and_id() {
        let mut source = card_import_fixture();
        source["data"]["plan"]["categories"][2]["name"] = json!("Old Visa name");
        let error = import_export(&source).err().unwrap();
        for detail in ["Old Visa name", "visa-pay", "no matching on-budget credit account"] { assert!(error.contains(detail), "{error}"); }
    }
    #[test]
    fn duplicate_payment_categories_cannot_claim_one_card() {
        let mut source = card_import_fixture();
        let p = &mut source["data"]["plan"];
        let mut other = p["categories"][2].clone();
        other["id"] = json!("other-payment");
        p["categories"].as_array_mut().unwrap().push(other);
        let error = import_export(&source).err().unwrap();
        for detail in ["visa-pay", "other-payment", "both match", "Visa"] { assert!(error.contains(detail), "{error}"); }
    }
    #[test]
    fn card_without_a_payment_category_reports_the_unmatched_account() {
        let mut source = card_import_fixture();
        source["data"]["plan"]["categories"].as_array_mut().unwrap().pop();
        let error = import_export(&source).err().unwrap();
        for detail in ["Visa", "visa", "no matching category in Credit Card Payments"] { assert!(error.contains(detail), "{error}"); }
    }
    #[test]
    fn starting_balance_keeps_history_but_is_not_income_until_renamed_or_recategorized() {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        p["payees"].as_array_mut().unwrap().push(json!({"id":"opening","name":"Starting Balance"}));
        p["transactions"][0]["payee_id"] = json!("opening");
        let mut b = import_export(&source).unwrap();
        assert_eq!(b.transactions.len(), 3);
        assert_eq!(b.transactions[0].amount, 10000);
        assert_eq!(b.transactions[0].extra["startingBalance"], true);
        assert_eq!(b.transactions[0].splits[0].extra["unassigned"], true);
        assert_eq!((report(&b).income, report(&b).spending), (0, 2000));
        assert_eq!(calculate(&b, "2026-10").unwrap().accounts[0].balance, 8000);
        b.transactions[0].payee = "Income corrected".into();
        assert_eq!(report(&b).income, 10000);
        b.transactions[0].payee = "Starting Balance".into();
        b.transactions[0].splits[0].category_id = Some("food".into());
        assert_eq!((report(&b).income, report(&b).spending), (0, -8000));
    }
    /// A 12,000.00 car loan at 6% from July 1st, paid 350.00 a month, as a YNAB plan exports it:
    /// its balance includes the interest YNAB adds each month, which no transaction records.
    fn loan_fixture(rate: i64) -> Value {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        p["accounts"][0]["balance"] = json!(80000 - 1400000);
        p["accounts"][0]["uncleared_balance"] = json!(-1400000);
        p["accounts"].as_array_mut().unwrap().push(json!({"id":"loan","name":"Car Loan","type":"autoLoan","on_budget":false,"closed":false,"deleted":false,
            "balance":-10831270,"cleared_balance":-12000000,"uncleared_balance":1168730,"debt_interest_rates":{"2026-07-01":rate}}));
        p["payees"].as_array_mut().unwrap().push(json!({"id":"opening","name":"Starting Balance"}));
        let rows = p["transactions"].as_array_mut().unwrap();
        rows.push(json!({"id":"opened","account_id":"loan","date":"2026-07-01","amount":-12000000,"payee_id":"opening","cleared":"cleared","approved":true,"category_id":null,"transfer_account_id":null,"transfer_transaction_id":null,"deleted":false}));
        for date in ["2026-07-28", "2026-08-28", "2026-09-28", "2026-10-12"] {
            let (paid, received) = (format!("paid-{date}"), format!("received-{date}"));
            rows.push(json!({"id":paid,"account_id":"cash","date":date,"amount":-350000,"cleared":"uncleared","approved":true,"category_id":"food","transfer_account_id":"loan","transfer_transaction_id":received,"deleted":false}));
            rows.push(json!({"id":received,"account_id":"loan","date":date,"amount":350000,"cleared":"uncleared","approved":true,"category_id":null,"transfer_account_id":"cash","transfer_transaction_id":paid,"deleted":false}));
        }
        source
    }
    #[test]
    fn interest_ynab_records_without_a_payee_is_called_loan_interest() {
        let mut source = fixture();
        let rows = source["data"]["plan"]["transactions"].as_array_mut().unwrap();
        let mut interest = rows[0].clone();
        (interest["id"], interest["amount"], interest["payee_id"]) = (json!("interest"), json!(-5000), Value::Null);
        (interest["category_id"], interest["debt_transaction_type"]) = (Value::Null, json!("interest"));
        rows.push(interest.clone());
        (interest["id"], interest["debt_transaction_type"]) = (json!("fee"), json!("fee"));
        rows.push(interest);
        let b = import_unverified(&source).unwrap();
        let payee = |id: &str| b.transactions.iter().find(|t| t.id == id).unwrap().payee.as_str();
        assert_eq!((payee("interest"), payee("fee"), payee("start")), ("Loan interest", "", "Shop"));
    }
    #[test]
    fn loan_interest_ynab_adds_without_transactions_is_imported_month_by_month() {
        let b = import_export(&loan_fixture(6000)).unwrap();
        let interest: Vec<_> = b.transactions.iter().filter(|t| crate::fresh_start::is_interest(t))
            .map(|t| (t.date.as_str(), t.amount, t.status.clone())).collect();
        assert_eq!(interest, [
            ("2026-07-01", -6000, Cleared::Uncleared), ("2026-08-01", -5855, Cleared::Uncleared),
            ("2026-09-01", -5709, Cleared::Uncleared), ("2026-10-01", -5563, Cleared::Uncleared),
        ]);
        let loan = calculate(&b, "2026-10").unwrap().accounts.into_iter().find(|a| a.id == "loan").unwrap();
        assert_eq!((loan.balance, loan.cleared, loan.uncleared), (-1083127, -1200000, 116873));
        // Charges that don't come out to YNAB's balance exactly are never added.
        let error = import_export(&loan_fixture(7000)).unwrap_err();
        assert!(error.contains("Car Loan balance −10600.00 here, −10831.27 in YNAB"), "{error}");
        // Unverified, the same import is built and its differences listed for the person to decide.
        // (The fixture's months leave out the payments, so Food and Ready to Assign differ too.)
        let b = import_unverified(&loan_fixture(7000)).unwrap();
        let listed: Vec<_> = differences(&b).iter().map(|d| {
            let about = if d["account"].is_null() { d["category"].clone() } else { d["account"].clone() };
            (d["kind"].clone(), about, d["field"].clone(), d["actual"].clone(), d["expected"].clone())
        }).collect();
        assert_eq!(listed, [
            (json!("account"), json!("Car Loan"), json!("balance"), json!(-1060000), json!(-1083127)),
            (json!("account"), json!("Car Loan"), json!("uncleared_balance"), json!(140000), json!(116873)),
            (json!("category"), json!("Food"), json!("activity"), json!(-37000), json!(-2000)),
            (json!("category"), json!("Food"), json!("balance"), json!(-35000), json!(0)),
            (json!("readyToAssign"), Value::Null, Value::Null, json!(-97000), json!(8000)),
        ]);
        let verified = import_unverified(&loan_fixture(6000)).unwrap();
        assert!(differences(&verified).iter().all(|d| d["kind"] != "account"));
    }
    #[test]
    fn a_target_snoozed_for_the_month_is_not_expected_to_ask_for_anything() {
        // YNAB's API still reports a snoozed target's shortfall; its screen and Auto-Assign don't.
        let mut source = fixture();
        let food = &mut source["data"]["plan"]["months"][0]["categories"][0];
        food["goal_under_funded"] = json!(50000);
        food["goal_snoozed_at"] = json!("2026-10-05T10:00:00.427");
        let b = import_export(&source).unwrap();
        assert_eq!(b.extra["importAudit"]["underfundedDifferences"], json!([]));
        source["data"]["plan"]["months"][0]["categories"][0]["goal_snoozed_at"] = Value::Null;
        let b = import_export(&source).unwrap();
        assert_eq!(b.extra["importAudit"]["underfundedDifferences"][0]["expected"], 5000);
    }
    #[test]
    fn loan_interest_follows_ynabs_milliunits_and_balances_between_cents() {
        // At 5%, September's interest is 47.494...: YNAB keeps 47.495, so the balance it shows
        // is 11,096.25 (not 11,096.24), and October's 46.234 brings it to 10,792.479.
        for balance in [-10792479, -10792480] {
            let mut source = loan_fixture(5000);
            let loan = &mut source["data"]["plan"]["accounts"][1];
            loan["balance"] = json!(balance);
            loan["uncleared_balance"] = json!(balance + 12000000);
            let b = import_export(&source).unwrap();
            let interest: Vec<_> = b.transactions.iter().filter(|t| crate::fresh_start::is_interest(t)).map(|t| t.amount).collect();
            assert_eq!(interest, [-5000, -4875, -4750, -4623], "{balance}");
            let loan = calculate(&b, "2026-10").unwrap().accounts.into_iter().find(|a| a.id == "loan").unwrap();
            assert_eq!((loan.balance, loan.uncleared), (-1079248, 120752));
        }
    }
    #[test]
    fn card_name_collision_keeps_ordinary_envelope_and_repairs_old_import() {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        p["accounts"].as_array_mut().unwrap().push(json!({"id":"visa","name":"Visa","type":"creditCard","on_budget":true,"balance":0,"cleared_balance":0,"uncleared_balance":0}));
        p["category_groups"].as_array_mut().unwrap().push(json!({"id":"payments","name":"Credit Card Payments"}));
        p["categories"][1]["name"] = json!("Visa");
        p["categories"].as_array_mut().unwrap().push(json!({"id":"visa-pay","name":"Visa","category_group_id":"payments","goal_target":0}));
        let mut b = import_export(&source).unwrap();
        assert_eq!(b.categories[0].payment_account(), None);
        assert_eq!(b.categories[1].payment_account(), Some("visa"));
        assert_eq!(report(&b).spending, 2000);
        b.categories[0].extra.insert("paymentAccountId".into(), json!("visa"));
        normalize_import_metadata(&mut b);
        assert_eq!(b.categories[0].payment_account(), None);
        assert_eq!(b.categories[1].payment_account(), Some("visa"));
        assert_eq!(report(&b).spending, 2000);
        b.categories[0].group = "User moved this".into();
        b.categories[0].extra.insert("paymentAccountId".into(), json!("visa"));
        normalize_import_metadata(&mut b);
        assert_eq!(b.categories[0].payment_account(), Some("visa"));
    }
    #[test]
    fn negative_rta_and_split_rta_keep_explicit_role_but_unknown_and_user_cleared_do_not() {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        p["transactions"][2]["amount"] = json!(-10000);
        p["accounts"][0]["balance"] = json!(70000);
        p["accounts"][0]["cleared_balance"] = json!(70000);
        p["subtransactions"][1]["category_id"] = json!("income");
        let mut b = import_export(&source).unwrap();
        assert_eq!(b.transactions[2].splits[0].extra["unassigned"], true);
        assert_eq!(b.transactions[1].splits[1].extra["unassigned"], true);
        assert_eq!((report(&b).income, report(&b).spending), (8500, 1500));
        b.transactions[2].splits[0].extra.insert("unassigned".into(), json!(false));
        b.transactions[1].splits[0].category_id = None;
        normalize_import_metadata(&mut b);
        assert_eq!(b.transactions[2].splits[0].extra["unassigned"], false);
        assert_eq!(b.transactions[1].splits[0].extra["unassigned"], false);
        assert_eq!(report(&b).spending, 2500);
        let mut unknown = b.transactions[2].clone();
        unknown.extra = meta(json!({"source":{"id":"bank-row"}})).into();
        unknown.splits[0].extra.remove("unassigned");
        b.transactions = vec![unknown];
        normalize_import_metadata(&mut b);
        assert!(!b.transactions[0].splits[0].extra.contains_key("unassigned"));
    }
    #[test]
    fn scheduled_import_rejects_unknown_categories_and_preserves_internal_and_uncategorized_roles() {
        let scheduled = |category: Value| json!({
            "id":"scheduled","account_id":"cash","payee_id":"shop","category_id":category,"amount":-10000,
            "date_first":"2026-10-05","date_next":"2026-10-05","frequency":"never"
        });
        for category in [json!("missing-category"), json!(42)] {
            let mut source = fixture();
            source["data"]["plan"]["scheduled_transactions"] = json!([scheduled(category.clone())]);
            let error = import_export(&source).unwrap_err();
            assert!(error.contains("category"), "{error}");
            if category.is_string() { assert!(error.contains("missing-category"), "{error}"); }
            // A split parent's provider-only marker is ignored; each actual split must resolve.
            source["data"]["plan"]["scheduled_transactions"][0]["category_id"] = json!("split-system-id");
            source["data"]["plan"]["scheduled_subtransactions"] = json!([{
                "id":"part", "scheduled_transaction_id":"scheduled", "category_id":category, "amount":-10000
            }]);
            assert!(import_export(&source).unwrap_err().contains("category"));
        }
        for category in [Value::Null, json!("income"), json!("internal-uncategorized")] {
            let mut source = fixture();
            source["data"]["plan"]["categories"].as_array_mut().unwrap().push(json!({
                "id":"internal-uncategorized", "name":"Uncategorized", "internal":true
            }));
            source["data"]["plan"]["scheduled_transactions"] = json!([scheduled(category.clone())]);
            let b = import_export(&source).unwrap();
            let split = &b.extra["schedules"][0]["transaction"]["splits"][0];
            assert!(split["categoryId"].is_null());
            if category == "income" { assert_eq!(split["unassigned"], true); }
            if category == "internal-uncategorized" { assert_eq!(split["unassigned"], false); }
            assert_eq!(import_export(&source).unwrap().extra["schedules"], b.extra["schedules"]);
        }
    }
    #[test]
    fn imported_schedule_roles_survive_legacy_load_due_posting_and_explicit_clear() {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        let scheduled = |id: &str, category: &str, amount: i64| json!({
            "id":id,"account_id":"cash","payee_id":"shop","category_id":category,"amount":amount,
            "date_first":"2026-10-05","date_next":"2026-10-05","frequency":"never"
        });
        p["scheduled_transactions"] = json!([scheduled("correction", "income", -10000), scheduled("parts", "split-system-id", -20000)]);
        p["scheduled_subtransactions"] = json!([
            {"id":"scheduled-food","scheduled_transaction_id":"parts","category_id":"food","amount":-15000},
            {"id":"scheduled-income","scheduled_transaction_id":"parts","category_id":"income","amount":-5000}
        ]);
        let mut b = import_export(&source).unwrap();
        assert_eq!(b.extra["schedules"][0]["transaction"]["splits"][0]["unassigned"], true);
        assert_eq!(b.extra["schedules"][1]["transaction"]["splits"][1]["unassigned"], true);
        let mut legacy = b.clone();
        for schedule in legacy.extra.get_mut("schedules").unwrap().as_array_mut().unwrap() {
            for part in schedule["transaction"]["splits"].as_array_mut().unwrap() {
                part.as_object_mut().unwrap().remove("unassigned");
            }
        }
        normalize_import_metadata(&mut legacy);
        assert_eq!(legacy.extra["schedules"], b.extra["schedules"]);
        legacy.extra.get_mut("schedules").unwrap()[0]["transaction"]["splits"][0]["unassigned"] = json!(false);
        normalize_import_metadata(&mut legacy);
        assert_eq!(legacy.extra["schedules"][0]["transaction"]["splits"][0]["unassigned"], false);
        assert_eq!(crate::schedules::apply(&mut b, "2026-10-05", None, false).unwrap(), 2);
        assert_eq!((report(&b).income, report(&b).spending), (8500, 3500));
        // Already posted occurrences from older versions retain their provider schedule identity.
        for t in b.transactions.iter_mut().filter(|t| t.extra.contains_key("scheduleId")) {
            for part in &mut t.splits { part.extra.remove("unassigned"); }
        }
        normalize_import_metadata(&mut b);
        assert_eq!((report(&b).income, report(&b).spending), (8500, 3500));
    }
    #[test]
    fn api_import_preserves_source_ids_flags_parent_and_split_memos() {
        let source = fixture();
        let b = import_export(&source).unwrap();
        assert_eq!(b.transactions.len(), 3);
        assert_eq!(b.transactions[1].id, "split");
        assert_eq!(b.transactions[1].extra["flag"], "purple");
        assert_eq!(b.transactions[1].memo, "Parent memo");
        assert_eq!(b.transactions[1].splits[0].extra["memo"], "First part");
        assert_eq!(b.categories[0].extra["note"], "Original category note");
        assert_eq!(b.extra["sourceExport"], source);
        assert!(b.extra["importAudit"]["readyToAssignDifferences"]
            .as_array()
            .unwrap()
            .is_empty());
        let again = import_export(&source).unwrap();
        assert_eq!(b.transactions, again.transactions);
        let csv = export_register(&b).unwrap();
        let rows: Vec<_> = csv::Reader::from_reader(csv.as_bytes()).records().collect();
        assert_eq!(rows.len(), 4);
        assert!(csv.contains("Split (1/2) First part"));
        assert!(csv.contains("Purple"));
    }
    #[test]
    fn split_transfer_import_folds_counterpart_even_when_it_arrives_first() {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        p["accounts"].as_array_mut().unwrap().push(json!({"id":"savings","name":"Savings","type":"savings","on_budget":true,"closed":false,"deleted":false,"balance":5000,"cleared_balance":0,"uncleared_balance":5000}));
        p["subtransactions"][1]["category_id"] = Value::Null;
        p["subtransactions"][1]["transfer_account_id"] = json!("savings");
        p["subtransactions"][1]["transfer_transaction_id"] = json!("peer");
        p["transactions"].as_array_mut().unwrap().insert(0,json!({"id":"peer","account_id":"savings","date":"2026-10-01","amount":5000,"cleared":"uncleared","category_id":null,"transfer_account_id":"cash","transfer_transaction_id":"split-2","memo":"Other side memo","deleted":false}));
        p["months"][0]["categories"][0]["activity"] = json!(-15000);
        p["months"][0]["categories"][0]["balance"] = json!(5000);
        let b = import_export(&source).unwrap();
        assert_eq!(b.transactions.len(), 3);
        let t = b.transactions.iter().find(|t| t.id == "split").unwrap();
        assert_eq!(
            t.splits[1].extra["transferSource"]["memo"],
            "Other side memo"
        );
        assert_eq!(crate::ledger::transfer(&t.splits[1]), Some("savings"));
        assert!(b.extra["importAudit"]["categoryDifferences"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(b.extra["importAudit"]["readyToAssignDifferences"]
            .as_array()
            .unwrap()
            .is_empty());
        let csv = export_register(&b).unwrap();
        assert!(csv.contains("Other side memo"));
        let mut broken = source;
        broken["data"]["plan"]["transactions"][0]["amount"] = json!(6000);
        assert!(import_export(&broken).is_err());
    }
    #[test]
    fn split_transfer_leg_without_its_counterpart_id_is_matched_from_the_other_side() {
        // A live plan export left the leg's transfer_transaction_id empty.
        let with = |peer_names: Value| {
            let mut source = fixture();
            let p = &mut source["data"]["plan"];
            p["accounts"].as_array_mut().unwrap().push(json!({"id":"savings","name":"Savings","type":"savings","on_budget":true,"closed":false,"deleted":false,"balance":5000,"cleared_balance":0,"uncleared_balance":5000}));
            p["subtransactions"][1]["category_id"] = Value::Null;
            p["subtransactions"][1]["transfer_account_id"] = json!("savings");
            p["subtransactions"][1]["transfer_transaction_id"] = Value::Null;
            p["transactions"].as_array_mut().unwrap().push(json!({"id":"peer","account_id":"savings","date":"2026-10-01","amount":5000,"cleared":"uncleared","category_id":null,"transfer_account_id":"cash","transfer_transaction_id":peer_names,"memo":"Other side memo","deleted":false}));
            p["months"][0]["categories"][0]["activity"] = json!(-15000);
            p["months"][0]["categories"][0]["balance"] = json!(5000);
            source
        };
        for names in [json!("split-2"), Value::Null] {
            let b = import_export(&with(names.clone())).unwrap_or_else(|e| panic!("{names}: {e}"));
            assert_eq!(b.transactions.len(), 3, "{names}: the counterpart folds into its split");
            let t = b.transactions.iter().find(|t| t.id == "split").unwrap();
            assert_eq!(crate::ledger::transfer(&t.splits[1]), Some("savings"));
            assert_eq!(t.splits[1].extra["transferSource"]["memo"], "Other side memo");
        }
        // Two equally likely counterparts: nothing is guessed.
        let mut ambiguous = with(Value::Null);
        let mut twin = ambiguous["data"]["plan"]["transactions"].as_array().unwrap().last().unwrap().clone();
        twin["id"] = json!("twin");
        ambiguous["data"]["plan"]["transactions"].as_array_mut().unwrap().push(twin);
        assert!(import_export(&ambiguous).unwrap_err().contains("can't be identified"));
    }
    fn equal_split_transfers() -> Value {
        let mut source = fixture();
        let p = &mut source["data"]["plan"];
        p["accounts"].as_array_mut().unwrap().push(json!({"id":"savings","name":"Savings","type":"savings","on_budget":true,"closed":false,"deleted":false,"balance":20000,"cleared_balance":20000,"uncleared_balance":0}));
        for s in p["subtransactions"].as_array_mut().unwrap() {
            s["amount"] = json!(-10000);
            s["category_id"] = Value::Null;
            s["transfer_account_id"] = json!("savings");
            s["transfer_transaction_id"] = if s["id"] == "split-2" { json!("peer-b") } else { Value::Null };
        }
        for id in ["peer-a", "peer-b"] {
            p["transactions"].as_array_mut().unwrap().push(json!({"id":id,"account_id":"savings","date":"2026-10-01","amount":10000,"cleared":"cleared","category_id":null,"transfer_account_id":"cash","transfer_transaction_id":"split","deleted":false}));
        }
        p["months"][0]["categories"][0]["activity"] = json!(0);
        p["months"][0]["categories"][0]["balance"] = json!(20000);
        source
    }
    #[test]
    fn explicit_split_transfer_peers_are_reserved_before_missing_links_in_any_order() {
        // Both peers name the parent and have the same amount. Only split-2 explicitly
        // names its peer, leaving split-1's identity unambiguous after that reservation.
        for reverse_legs in [false, true] {
            for reverse_transactions in [false, true] {
                let mut source = equal_split_transfers();
                let p = &mut source["data"]["plan"];
                if reverse_legs { p["subtransactions"].as_array_mut().unwrap().reverse(); }
                if reverse_transactions { p["transactions"].as_array_mut().unwrap().reverse(); }
                let b = import_export(&source).unwrap();
                assert!(differences(&b).is_empty());
                assert_eq!(b.transactions.len(), 3, "counterparts fold into the split");
                let split = b.transactions.iter().find(|t| t.id == "split").unwrap();
                for (leg, peer) in [("split-1", "peer-a"), ("split-2", "peer-b")] {
                    let s = split.splits.iter().find(|s| s.extra["id"] == leg).unwrap();
                    assert_eq!(s.extra["transferSource"]["id"], peer);
                    assert_eq!(crate::ledger::transfer(s), Some("savings"));
                }
            }
        }
    }
    #[test]
    fn split_transfer_peer_reservations_reject_reuse_and_invalid_references() {
        let mut duplicate = equal_split_transfers();
        duplicate["data"]["plan"]["subtransactions"][0]["transfer_transaction_id"] = json!("peer-b");
        assert!(import_export(&duplicate).unwrap_err().contains("already used by another split"));
        for invalid in ["missing", "split"] {
            let mut source = equal_split_transfers();
            source["data"]["plan"]["subtransactions"][1]["transfer_transaction_id"] = json!(invalid);
            assert!(import_export(&source).unwrap_err().contains("missing or invalid transfer counterpart"));
        }
        let mut inconsistent = equal_split_transfers();
        inconsistent["data"]["plan"]["transactions"][4]["amount"] = json!(11000);
        assert!(import_export(&inconsistent).unwrap_err().contains("Inconsistent split transfer counterpart"));
    }
    #[test]
    fn mismatched_balances_and_subcent_precision_are_never_silently_adjusted() {
        let mut source = fixture();
        source["data"]["plan"]["accounts"][0]["balance"] = json!(80010);
        assert!(import_export(&source)
            .unwrap_err()
            .contains("Account totals"));
        let mut source = fixture();
        source["data"]["plan"]["transactions"][0]["amount"] = json!(100001);
        assert!(import_export(&source).unwrap_err().contains("sub-cent"));
    }
    #[test]
    fn incomplete_transfer_pairs_abort_instead_of_doubling_a_balance() {
        let mut source = fixture();
        source["data"]["plan"]["accounts"].as_array_mut().unwrap().push(json!({
            "id":"other", "name":"Savings", "type":"savings", "on_budget":true,
            "balance":0, "cleared_balance":0, "uncleared_balance":0,
        }));
        source["data"]["plan"]["transactions"][0]["transfer_transaction_id"] = json!("missing");
        source["data"]["plan"]["transactions"][0]["transfer_account_id"] = json!("other");
        assert!(import_export(&source).unwrap_err().contains("counterpart"));
    }
    #[test]
    fn register_export_keeps_ready_to_assign_on_budget_side_of_tracking_transfers() {
        let mut b = import_export(&fixture()).unwrap();
        b.accounts.push(Account {
            id: "brokerage".into(),
            name: "Brokerage".into(),
            kind: AccountKind::Tracking,
            opening_balance: 0,
            archived: false,
            extra: Map::new(),
        });
        for (id, amount) in [("deposit", -5000), ("withdrawal", 4990)] {
            b.transactions.push(Transaction {
                id: id.into(),
                date: "2026-10-02".into(),
                account_id: "cash".into(),
                payee: "".into(),
                memo: "".into(),
                amount,
                status: Cleared::Reconciled,
                splits: vec![Split {
                    category_id: None,
                    amount,
                    extra: meta(json!({"unassigned": true})).into(),
                }],
                transfer_account_id: Some("brokerage".into()),
                transfer_status: Some(Cleared::Cleared),
                extra: Default::default(),
            });
        }
        let csv = export_register(&b).unwrap();
        let rows: Vec<_> = csv::Reader::from_reader(csv.as_bytes())
            .records()
            .map(Result::unwrap)
            .filter(|r| &r[2] == "2026-10-02")
            .collect();
        assert_eq!(rows.len(), 4);
        for row in rows {
            if &row[0] == "Cash" {
                assert_eq!(&row[4], "Inflow: Ready to Assign");
                assert_eq!(&row[10], "Reconciled");
            } else {
                assert_eq!(&row[4], "");
                assert_eq!(&row[10], "Cleared");
            }
        }
    }
    /// The fixture as UrBudget saves it after importing, and a later YNAB export changed by `change`.
    fn merge_case(local: impl FnOnce(&mut Budget), change: impl FnOnce(&mut Value)) -> (Budget, Budget) {
        let base = fixture();
        let mut here = import_unverified(&base).unwrap();
        local(&mut here);
        let mut changed = base;
        change(&mut changed["data"]["plan"]);
        (here, import_unverified(&changed).unwrap())
    }
    fn tx<'a>(b: &'a Budget, id: &str) -> Option<&'a Transaction> {
        b.transactions.iter().find(|t| t.id == id)
    }
    #[test]
    fn an_amount_changed_on_both_sides_is_one_choice_with_its_split_lines() {
        let (here, remote) = merge_case(
            |b| {
                let t = b.transactions.iter_mut().find(|t| t.id == "start").unwrap();
                t.amount = 11000;
                t.splits[0].amount = 11000;
            },
            |p| p["transactions"][0]["amount"] = json!(120000),
        );
        let (_, conflicts) = crate::import_sync::merge_review(&here, remote.clone(), BTreeMap::new()).unwrap();
        let named: Vec<_> = conflicts.iter().map(|c| (c.path.as_str(), c.field.as_deref(), c.local_text.as_deref(), c.remote_text.as_deref())).collect();
        assert_eq!(named, [(
            "Transaction start.amount",
            Some("amount and split lines"),
            Some("110.00: Ready to Assign 110.00"),
            Some("120.00: Ready to Assign 120.00"),
        )]);
        // Choosing for the amount takes that side's lines too, so the result always adds up.
        for (choice, amount) in [("local", 11000), ("remote", 12000)] {
            let choices = BTreeMap::from([("Transaction start.amount".to_string(), choice.to_string())]);
            let (merged, left) = crate::import_sync::merge_review(&here, remote.clone(), choices).unwrap();
            assert!(left.is_empty());
            let t = tx(&merged, "start").unwrap();
            assert_eq!((t.amount, t.splits[0].amount), (amount, amount));
        }
    }
    #[test]
    fn a_category_changed_on_both_sides_shows_each_sides_lines_in_words() {
        let (here, remote) = merge_case(
            |b| {
                let s = &mut b.transactions.iter_mut().find(|t| t.id == "start").unwrap().splits[0];
                s.category_id = Some("food".into());
                s.extra.remove("unassigned");
            },
            |p| {
                p["categories"].as_array_mut().unwrap().push(json!({"id":"fun","name":"Fun","category_group_id":"living","hidden":false,"deleted":false}));
                p["transactions"][0]["category_id"] = json!("fun");
            },
        );
        let (_, conflicts) = crate::import_sync::merge_review(&here, remote, BTreeMap::new()).unwrap();
        let named: Vec<_> = conflicts.iter().map(|c| (c.path.as_str(), c.local_text.as_deref(), c.remote_text.as_deref())).collect();
        assert_eq!(named, [("Transaction start.splits", Some("Food 100.00"), Some("Fun 100.00"))]);
    }
    #[test]
    fn split_lines_removed_in_ynab_merge_unless_they_were_edited_here() {
        let remove = |p: &mut Value| {
            p["subtransactions"][1]["deleted"] = json!(true);
            p["subtransactions"][0]["amount"] = json!(-20000);
        };
        let (here, remote) = merge_case(|_| {}, remove);
        let (merged, conflicts) = crate::import_sync::merge_review(&here, remote, BTreeMap::new()).unwrap();
        assert!(conflicts.is_empty());
        assert_eq!(tx(&merged, "split").unwrap().splits.iter().map(|s| s.amount).collect::<Vec<_>>(), [-2000]);
        let (here, remote) = merge_case(
            |b| { b.transactions.iter_mut().find(|t| t.id == "split").unwrap().splits[1].extra.insert("memo".into(), json!("edited here")); },
            remove,
        );
        let (_, conflicts) = crate::import_sync::merge_review(&here, remote, BTreeMap::new()).unwrap();
        let named: Vec<_> = conflicts.iter().map(|c| (c.path.as_str(), c.local_text.as_deref(), c.remote_text.as_deref())).collect();
        assert_eq!(named, [("Transaction split.splits", Some("Food −15.00 (First part) · Food −5.00 (edited here)"), Some("Food −20.00 (First part)"))]);
    }
    #[test]
    fn a_split_deleted_in_ynab_takes_its_lines_and_asks_only_if_edited_here() {
        // A delta can mark the split deleted while its lines stay in the merged export.
        let delete = |p: &mut Value| p["transactions"][1]["deleted"] = json!(true);
        let (here, remote) = merge_case(|_| {}, delete);
        assert!(tx(&remote, "split").is_none());
        let (merged, conflicts) = crate::import_sync::merge_review(&here, remote, BTreeMap::new()).unwrap();
        assert!(conflicts.is_empty() && tx(&merged, "split").is_none());
        let (here, remote) = merge_case(|b| b.transactions.iter_mut().find(|t| t.id == "split").unwrap().memo = "edited here".into(), delete);
        let (_, conflicts) = crate::import_sync::merge_review(&here, remote, BTreeMap::new()).unwrap();
        let named: Vec<_> = conflicts.iter().map(|c| (c.path.as_str(), c.local_text.as_deref(), c.remote.is_none())).collect();
        assert_eq!(named, [("Transaction split", Some("Shop, 2026-10-01, Cash, −20.00"), true)]);
    }
    #[test]
    fn a_transaction_deleted_here_stays_deleted_unless_ynab_changed_it() {
        let (here, remote) = merge_case(|b| b.transactions.retain(|t| t.id != "split"), |_| {});
        let (merged, conflicts) = crate::import_sync::merge_review(&here, remote, BTreeMap::new()).unwrap();
        assert!(conflicts.is_empty() && tx(&merged, "split").is_none());
        let (here, remote) = merge_case(|b| b.transactions.retain(|t| t.id != "split"), |p| p["transactions"][1]["memo"] = json!("edited in YNAB"));
        let (_, conflicts) = crate::import_sync::merge_review(&here, remote, BTreeMap::new()).unwrap();
        let named: Vec<_> = conflicts.iter().map(|c| (c.path.as_str(), c.local.is_none(), c.remote_text.as_deref())).collect();
        assert_eq!(named, [("Transaction split", true, Some("Shop, 2026-10-01, Cash, −20.00"))]);
    }
    #[test]
    fn a_schedule_deleted_in_ynab_goes_unless_it_was_edited_here() {
        let scheduled = json!({"id":"rent","account_id":"cash","payee_id":"shop","category_id":"food","amount":-10000,
            "memo":null,"date_first":"2026-11-01","date_next":"2026-11-01","frequency":"monthly","deleted":false});
        let with_schedule = |p: &mut Value| p["scheduled_transactions"] = json!([scheduled.clone()]);
        let mut base = fixture();
        with_schedule(&mut base["data"]["plan"]);
        let here = import_unverified(&base).unwrap();
        assert_eq!(here.extra["schedules"].as_array().unwrap().len(), 1);
        let mut gone = base.clone();
        gone["data"]["plan"]["scheduled_transactions"][0]["deleted"] = json!(true);
        let remote = import_unverified(&gone).unwrap();
        let (merged, conflicts) = crate::import_sync::merge_review(&here, remote.clone(), BTreeMap::new()).unwrap();
        assert!(conflicts.is_empty());
        assert_eq!(merged.extra["schedules"].as_array().unwrap().len(), 0);
        let mut edited = here.clone();
        edited.extra.get_mut("schedules").unwrap()[0]["transaction"]["memo"] = json!("edited here");
        let (_, conflicts) = crate::import_sync::merge_review(&edited, remote, BTreeMap::new()).unwrap();
        let named: Vec<_> = conflicts.iter().map(|c| (c.path.as_str(), c.local_text.as_deref(), c.remote.is_none())).collect();
        assert_eq!(named, [("Schedule rent", Some("Shop, monthly, next 2026-11-01, −10.00"), true)]);
    }

    /// A YNAB export of `years` years ending in 2026: paychecks, card and checking purchases,
    /// three-way splits, card payments and a monthly transfer to a tracking account, with
    /// account balances that add up. Month records carry the assignments only.
    fn large_export(years: i32) -> Value {
        let mut transactions = vec![];
        let mut subtransactions = vec![];
        let mut months = vec![];
        let (mut checking, mut card, mut brokerage) = (0i64, 0i64, 0i64);
        let mut seed = 7u64;
        let mut amount = |lo: i64, hi: i64| { seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); lo + (seed >> 33) as i64 % (hi - lo) };
        let spend = ["food", "fun", "home", "car", "misc"];
        let mut n = 0;
        let mut id = || { n += 1; format!("t{n}") };
        for year in 2027 - years..2027 {
            for month in 1..=12 {
                let date = |day: u32| format!("{year}-{month:02}-{day:02}");
                let mut push = |t: Value| transactions.push(t);
                let tx = |id: String, account: &str, day: u32, amount: i64, category: Value, transfer: Value, peer: Value| json!({"id":id,"account_id":account,"date":date(day),"amount":amount,"payee_id":"shop","memo":null,"cleared":"cleared","approved":true,"flag_color":null,"category_id":category,"transfer_account_id":transfer,"transfer_transaction_id":peer,"deleted":false});
                for day in [1, 15] { push(tx(id(), "cash", day, 2_500_000, json!("income"), Value::Null, Value::Null)); checking += 2_500_000; }
                for k in 0..55 { let a = -amount(5_000, 90_000) * 10; push(tx(id(), "card", 1 + k % 28, a, json!(spend[k as usize % 5]), Value::Null, Value::Null)); card += a; }
                for k in 0..20 { let a = -amount(5_000, 60_000) * 10; push(tx(id(), "cash", 1 + k % 28, a, json!(spend[k as usize % 5]), Value::Null, Value::Null)); checking += a; }
                for k in 0..4 {
                    let parent = id();
                    let parts = [-amount(1_000, 20_000) * 10, -amount(1_000, 20_000) * 10, -amount(1_000, 20_000) * 10];
                    push(tx(parent.clone(), "cash", 3 + k * 6, parts.iter().sum(), json!("split-system-id"), Value::Null, Value::Null));
                    for (j, part) in parts.iter().enumerate() { subtransactions.push(json!({"id":format!("{parent}-{j}"),"transaction_id":parent,"category_id":spend[j],"amount":part,"memo":null,"payee_id":null,"transfer_account_id":null,"deleted":false})); }
                    checking += parts.iter().sum::<i64>();
                }
                // Pay the card in full and move some savings to the tracking account.
                let (pay, peer) = (id(), id());
                push(tx(pay.clone(), "cash", 27, card, Value::Null, json!("card"), json!(peer.clone())));
                push(tx(peer, "card", 27, -card, Value::Null, json!("cash"), json!(pay)));
                checking += card; card = 0;
                let (out, into) = (id(), id());
                push(tx(out.clone(), "cash", 28, -500_000, json!("misc"), json!("brokerage"), json!(into.clone())));
                push(tx(into, "brokerage", 28, 500_000, Value::Null, json!("cash"), json!(out)));
                checking -= 500_000; brokerage += 500_000;
                let budgeted: Vec<Value> = spend.iter().map(|c| json!({"id":c,"name":c,"budgeted":900_000,"activity":0,"balance":0,"deleted":false})).collect();
                months.push(json!({"month":format!("{year}-{month:02}-01"),"note":null,"to_be_budgeted":0,"age_of_money":null,"deleted":false,"categories":budgeted}));
            }
        }
        let account = |id: &str, name: &str, kind: &str, on_budget: bool, balance: i64| json!({"id":id,"name":name,"type":kind,"on_budget":on_budget,"closed":false,"deleted":false,"balance":balance,"cleared_balance":balance,"uncleared_balance":0});
        let category = |id: &str, group: &str| json!({"id":id,"name":id,"category_group_id":group,"hidden":false,"deleted":false});
        json!({"data":{"server_knowledge":1,"plan":{"id":"large","name":"Large","currency_format":{"iso_code":"USD"},"date_format":{"format":"YYYY-MM-DD"},
            "accounts":[account("cash","Checking","checking",true,checking),account("card","Card","creditCard",true,card),account("brokerage","Brokerage","otherAsset",false,brokerage)],
            "category_groups":[{"id":"living","name":"Living","hidden":false,"deleted":false},{"id":"cards","name":"Credit Card Payments","hidden":false,"deleted":false}],
            "categories":[json!({"id":"income","name":"Inflow: Ready to Assign","internal":true,"deleted":false}),category("food","living"),category("fun","living"),category("home","living"),category("car","living"),category("misc","living"),json!({"id":"card-pay","name":"Card","category_group_id":"cards","hidden":false,"deleted":false})],
            "payees":[{"id":"shop","name":"Shop","deleted":false}],
            "transactions":transactions,"subtransactions":subtransactions,"scheduled_transactions":[],"scheduled_subtransactions":[],"months":months}}})
    }
    #[test]
    #[ignore = "synthetic import timing; run explicitly with --ignored --nocapture"]
    fn benchmark_large_import() {
        use std::time::Instant;
        let export = large_export(12);
        let count = export["data"]["plan"]["transactions"].as_array().unwrap().len();
        let at = Instant::now();
        let mut local = import_unverified(&export).unwrap();
        let imported = at.elapsed();
        assert!(local.extra["importAudit"]["accountDifferences"].as_array().unwrap().is_empty());
        local.transactions[0].memo = "Edited here".into();
        let at = Instant::now();
        let incoming = import_unverified(&export).unwrap();
        let (merged, conflicts) = crate::import_sync::merge_review(&local, incoming, BTreeMap::new()).unwrap();
        let reimported = at.elapsed();
        assert!(conflicts.is_empty());
        assert_eq!(merged.transactions[0].memo, "Edited here");
        let at = Instant::now();
        calculate(&merged, "2026-12").unwrap();
        let month = at.elapsed();
        let at = Instant::now();
        crate::reports::calculate(&merged, &crate::reports::Filter { from: "2015-01".into(), to: "2026-12".into(), account_ids: vec![], category_ids: vec![] }).unwrap();
        let reports = at.elapsed();
        println!("{count} transactions: import and check {imported:?}, re-import and merge {reimported:?}, December 2026 {month:?}, 12 years of reports {reports:?}");
    }

}
#[cfg(test)]
mod export_tests {
    use super::*;
    use crate::domain::tests::{purchase, sample};
    #[test]
    fn one_account_export_includes_transfers_into_it_newest_first_and_can_keep_listed_ones() {
        let mut b = sample();
        b.transactions.push(purchase());
        let mut pay = purchase();
        pay.id = "pay".into();
        pay.account_id = "cash".into();
        pay.date = "2026-10-20".into();
        pay.transfer_account_id = Some("card".into());
        pay.transfer_status = Some(Cleared::Cleared);
        pay.splits.clear();
        b.transactions.push(pay);
        let card = export_register_for(&b, |account, _| account == "card", true).unwrap();
        let rows: Vec<&str> = card.lines().skip(1).collect();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].starts_with("Card,,2026-10-20,Transfer : Cash"), "{}", rows[0]);
        assert!(rows[1].starts_with("Card,,2026-10-15,Shop"), "{}", rows[1]);
        let listed = export_register_for(&b, |_, id| id == "tx", true).unwrap();
        assert_eq!(listed.lines().count(), 2);
    }
    #[test]
    fn a_newest_first_export_lists_a_days_transactions_like_the_register() {
        let mut b = sample();
        let mut add = |payee: &str, created: Option<&str>| {
            let mut t = purchase();
            t.id = payee.into();
            t.payee = payee.into();
            if let Some(at) = created {
                t.set_created_at(at);
            }
            b.transactions.push(t);
        };
        // Saved in this order: the later-added first, then two from before the app kept the time.
        add("Added second", Some("2026-10-15T12:00:00.200Z"));
        add("Added first", Some("2026-10-15T12:00:00.100Z"));
        add("Old one", None);
        add("Old two", None);
        let csv = export_register_for(&b, |_, _| true, true).unwrap();
        let payees: Vec<String> = csv::Reader::from_reader(csv.as_bytes())
            .records()
            .map(|r| r.unwrap()[3].to_owned())
            .collect();
        assert_eq!(payees, ["Added second", "Added first", "Old two", "Old one"]);
    }
}
