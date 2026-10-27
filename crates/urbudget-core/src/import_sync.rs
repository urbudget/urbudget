//! A delta is merged into the immutable source snapshot before validation. Local
//! edits then merge against that snapshot; competing edits never silently win.
use crate::i18n::{language, tr, Language};
use crate::{domain::*, t, ynab};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn plan(v: &Value) -> Result<&Value, String> {
    v.pointer("/data/plan")
        .or_else(|| v.pointer("/data/budget"))
        .ok_or_else(|| t!("Missing YNAB plan"))
}
pub fn merge_delta(base: &Value, delta: &Value) -> Result<Value, String> {
    let a = plan(base)?;
    let d = plan(delta)?;
    if a["id"] != d["id"] {
        return Err(t!("The response belongs to a different YNAB plan."));
    }
    fn overlay(a: &Value, d: &Value) -> Value {
        if let (Some(a), Some(d)) = (a.as_object(), d.as_object()) {
            let mut result = a.clone();
            for (k, v) in d {
                result.insert(k.clone(), overlay(result.get(k).unwrap_or(&Value::Null), v));
            }
            Value::Object(result)
        } else if let (Some(a), Some(d)) = (a.as_array(), d.as_array()) {
            let key = if a.iter().chain(d).all(|x| x.get("id").is_some()) {
                "id"
            } else {
                "month"
            };
            let mut result = a.clone();
            for v in d {
                if let Some(i) = result
                    .iter()
                    .position(|x| x.get(key).is_some() && x[key] == v[key])
                {
                    result[i] = overlay(&result[i], v)
                } else {
                    result.push(v.clone())
                }
            }
            Value::Array(result)
        } else {
            d.clone()
        }
    }
    Ok(json!({"data":{"plan":overlay(a,d),"server_knowledge":delta["data"]["server_knowledge"]}}))
}
fn editable(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.remove("source");
        if let Some(t) = o.get_mut("transferSource").and_then(Value::as_object_mut) {
            t.retain(|k, _| k == "memo" || k == "flag_color");
        }
        for child in o.values_mut() {
            *child = editable(child.take());
        }
    } else if let Some(a) = v.as_array_mut() {
        for child in a {
            *child = editable(child.take());
        }
    }
    v
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub path: String,
    pub local: Option<Value>,
    pub remote: Option<Value>,
    /// What the conflict is about, in words ("Whole Foods, 2026-10-18, Visa"), and which of its
    /// fields ("memo"), named once the merge has every record (see `describe`).
    pub about: Option<String>,
    pub field: Option<String>,
    /// Each side's value in words where the raw value is a record or a list of split lines
    /// ("Shop, 2026-10-01, Cash, −20.00"; "Food −15.00 · Food −5.00").
    pub local_text: Option<String>,
    pub remote_text: Option<String>,
}
/// A schedule's repeat for the review: the stored key, or its words in Arabic.
fn repeat(key: &str) -> &str {
    let word = match key {
        "never" => tr("never"),
        "daily" => tr("daily"),
        "weekly" => tr("weekly"),
        "everyOtherWeek" => tr("everyOtherWeek"),
        "twiceAMonth" => tr("twiceAMonth"),
        "every4Weeks" => tr("every4Weeks"),
        "monthly" => tr("monthly"),
        "everyOtherMonth" => tr("everyOtherMonth"),
        "every3Months" => tr("every3Months"),
        "every4Months" => tr("every4Months"),
        "twiceAYear" => tr("twiceAYear"),
        "yearly" => tr("yearly"),
        "everyOtherYear" => tr("everyOtherYear"),
        _ => key,
    };
    if language() == Language::English { key } else { word }
}
fn cents(n: i64) -> String {
    format!("{}{}.{:02}", if n < 0 { "−" } else { "" }, n.abs() / 100, n.abs() % 100)
}
/// A conflict's value in words, for the records and split lists the review can't show plainly.
fn value_text(path: &str, value: &Value, local: &Budget, remote: &Budget) -> Option<String> {
    let account = |id: &str| local.accounts.iter().chain(&remote.accounts).find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
    let category = |id: &str| local.categories.iter().chain(&remote.categories).find(|c| c.id == id).map(|c| c.name.clone());
    let lines = |splits: &Value| -> Option<String> {
        let list: Vec<String> = splits.as_array()?.iter().map(|s| {
            let what = if let Some(id) = s["categoryId"].as_str() {
                category(id).unwrap_or_else(|| t!("Unknown category"))
            } else if let Some(id) = s["transferAccountId"].as_str() {
                t!("Transfer: {account}", account = account(id))
            } else if s["unassigned"] == true {
                t!("Ready to Assign")
            } else {
                t!("No category")
            };
            let memo = s["memo"].as_str().filter(|m| !m.is_empty()).map(|m| format!(" ({m})")).unwrap_or_default();
            format!("{what} {}{memo}", cents(s["amount"].as_i64().unwrap_or(0)))
        }).collect();
        Some(list.join(" · "))
    };
    let (head, field) = path.split_once('.').map_or((path, None), |(h, f)| (h, Some(f)));
    match (head.split_once(' ').map(|(kind, _)| kind), field) {
        (Some("Transaction"), None) => {
            let payee = value["payee"].as_str().filter(|p| !p.is_empty()).unwrap_or(tr("No payee"));
            let amount = cents(value["amount"].as_i64()?);
            Some(t!("{payee}, {date}, {account}, {amount}", payee, date = value["date"].as_str().unwrap_or(""), account = account(value["accountId"].as_str().unwrap_or("")), amount))
        }
        (Some("Transaction"), Some("splits")) => lines(value),
        (Some("Schedule"), None) => {
            let t = &value["transaction"];
            let payee = t["payee"].as_str().filter(|p| !p.is_empty()).unwrap_or(tr("No payee"));
            let amount = cents(t["amount"].as_i64()?);
            Some(t!("{payee}, {frequency}, next {date}, {amount}", payee, frequency = repeat(value["frequency"].as_str().unwrap_or("")), date = value["nextDate"].as_str().unwrap_or(""), amount))
        }
        _ => None,
    }
}
/// Names a conflict's record and field from its path ("Transaction {id}.memo") for the review.
fn describe(path: &str, local: &Budget, remote: &Budget) -> (Option<String>, Option<String>) {
    let account = |id: &str| local.accounts.iter().chain(&remote.accounts).find(|a| a.id == id).map(|a| a.name.clone());
    let category = |id: &str| local.categories.iter().chain(&remote.categories).find(|c| c.id == id).map(|c| c.name.clone());
    let transaction = |id: &str| {
        local.transactions.iter().chain(&remote.transactions).find(|t| t.id == id).map(|t| {
            let payee = if t.payee.is_empty() { tr("No payee") } else { t.payee.as_str() };
            t!("{payee}, {date}, {account}", payee, date = t.date, account = account(&t.account_id).unwrap_or_default())
        })
    };
    let (head, field) = match path.split_once('.') {
        Some((head, rest)) => (head, Some(rest.replace('.', " › "))),
        None => (path, None),
    };
    let about = if let Some(id) = head.strip_prefix("Transaction ") {
        transaction(id)
    } else if let Some(id) = head.strip_prefix("Duplicate scheduled entry ") {
        transaction(id).map(|entry| t!("Scheduled entry: {entry}", entry))
    } else if let Some(id) = head.strip_prefix("Category ") {
        category(id)
    } else if let Some(id) = head.strip_prefix("Account ") {
        account(id)
    } else if let Some(key) = head.strip_prefix("Assignment ") {
        // Assignments are keyed by their month and category, each as JSON: "2026-10":"food".
        let (month, id) = key.split_once(':').unwrap_or((key, ""));
        let id = id.trim_matches('"');
        Some(t!("{category}, assigned in {month}", category = category(id).unwrap_or_else(|| id.into()), month = month.trim_matches('"')))
    } else if let Some(month) = head.strip_prefix("Month ") {
        Some(t!("Note for {month}", month = month.trim_end_matches(" note").trim_matches('"')))
    } else if head == "name" || head == "currency" {
        return (Some(t!("This budget")), Some(head.into()));
    } else {
        None
    };
    (about, field)
}
#[derive(Default)]
struct Context {
    choices: BTreeMap<String, String>,
    conflicts: Vec<Conflict>,
}
fn field(
    base: Option<&Value>,
    local: Option<&Value>,
    remote: Option<&Value>,
    path: &str,
    context: &mut Context,
) -> Result<Option<Value>, String> {
    // A split choice also chooses its total, even if only one side changed that total.
    // Otherwise the one-sided fast path below can combine one side's amount with the
    // other side's lines and reject an otherwise valid resolved import.
    if path.starts_with("Transaction ") && (path.ends_with(".amount") || path.ends_with(".splits")) {
        match context.choices.get(path).map(String::as_str) {
            Some("local") => return Ok(local.cloned()),
            Some("remote") => return Ok(remote.cloned()),
            Some(_) => return Err(t!("Choose Local or YNAB for each conflict.")),
            None => {}
        }
    }
    if local == base || local == remote {
        return Ok(remote.cloned());
    }
    if remote == base {
        return Ok(local.cloned());
    }
    if let (Some(a), Some(l), Some(r)) = (
        base.and_then(Value::as_object),
        local.and_then(Value::as_object),
        remote.and_then(Value::as_object),
    ) {
        let keys: BTreeSet<_> = a.keys().chain(l.keys()).chain(r.keys()).collect();
        let mut out = serde_json::Map::new();
        for k in keys {
            if let Some(v) = field(
                a.get(k),
                l.get(k),
                r.get(k),
                &format!("{path}.{k}"),
                context,
            )? {
                out.insert(k.clone(), v);
            }
        }
        return Ok(Some(Value::Object(out)));
    }
    match context.choices.get(path).map(String::as_str) {
        Some("local") => Ok(local.cloned()),
        Some("remote") => Ok(remote.cloned()),
        Some(_) => Err(t!("Choose Local or YNAB for each conflict.")),
        None => {
            context.conflicts.push(Conflict {
                path: path.into(),
                local: local.cloned(),
                remote: remote.cloned(),
                about: None,
                field: None,
                local_text: None,
                remote_text: None,
            });
            Ok(local.cloned())
        }
    }
}
fn restore_metadata(raw: &Value, edited: &Value) -> Value {
    match (raw, edited) {
        (Value::Object(r), Value::Object(e)) => {
            let mut out = e.clone();
            if let Some(source) = r.get("source") {
                out.insert("source".into(), source.clone());
            }
            for (k, v) in e {
                if k == "transferSource" && v.is_object() {
                    let mut source = r
                        .get(k)
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    source.extend(v.as_object().unwrap().clone());
                    out.insert(k.clone(), Value::Object(source));
                } else if let Some(original) = r.get(k) {
                    out.insert(k.clone(), restore_metadata(original, v));
                }
            }
            Value::Object(out)
        }
        (Value::Array(r), Value::Array(e)) => Value::Array(
            e.iter()
                .enumerate()
                .map(|(i, v)| {
                    let original = v
                        .get("id")
                        .and_then(|id| r.iter().find(|a| a.get("id") == Some(id)))
                        .or_else(|| r.get(i));
                    original
                        .map(|o| restore_metadata(o, v))
                        .unwrap_or(v.clone())
                })
                .collect(),
        ),
        _ => edited.clone(),
    }
}
fn records_with<T: serde::Serialize + serde::de::DeserializeOwned>(
    base: &[T],
    local: &[T],
    remote: &[T],
    label: &str,
    context: &mut Context,
) -> Result<Vec<T>, String> {
    fn index<T: serde::Serialize>(items: &[T]) -> BTreeMap<String, Value> {
        items
            .iter()
            .map(|x| {
                let v = serde_json::to_value(x).unwrap();
                let id = v["id"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| format!("{}:{}", v["month"], v["categoryId"]));
                (id, v)
            })
            .collect()
    }
    let a = index(base);
    let l = index(local);
    let r = index(remote);
    let mut order: Vec<String> = remote
        .iter()
        .chain(local)
        .map(|x| {
            let m = index(std::slice::from_ref(x));
            m.keys().next().unwrap().clone()
        })
        .collect();
    let mut seen = BTreeSet::new();
    order.retain(|k| seen.insert(k.clone()));
    let mut out = vec![];
    for id in order {
        let av = a.get(&id).cloned().map(editable);
        let lv = l.get(&id).cloned().map(editable);
        let rv = r.get(&id).cloned().map(editable);
        if let Some(v) = field(
            av.as_ref(),
            lv.as_ref(),
            rv.as_ref(),
            &format!("{label} {id}"),
            context,
        )? {
            let raw = restore_metadata(r.get(&id).or_else(|| l.get(&id)).unwrap(), &v);
            out.push(serde_json::from_value(raw).map_err(|e| e.to_string())?);
        }
    }
    Ok(out)
}
/// The imported budget, keeping the name the user gave this one. The first launch's unnamed
/// budget ("My budget") and a name an earlier import brought take the plan's name instead.
pub fn keep_name(local: &Budget, mut imported: Budget) -> Budget {
    let from_import = local.extra.get("sourceExport").and_then(|s| s["data"]["plan"]["name"].as_str());
    let name = local.name.trim();
    if !name.is_empty() && name != Budget::default().name && Some(name) != from_import {
        imported.name = local.name.clone();
    }
    imported
}
pub fn merge_local(local: &Budget, remote: Budget) -> Result<Budget, String> {
    let (budget, conflicts) = merge_review(local, remote, BTreeMap::new())?;
    if !conflicts.is_empty() {
        return Err(t!("IMPORT CONFLICT: {count} competing edits need review. Nothing was overwritten.", count = conflicts.len()));
    }
    Ok(budget)
}
pub fn merge_review(
    local: &Budget,
    mut remote: Budget,
    choices: BTreeMap<String, String>,
) -> Result<(Budget, Vec<Conflict>), String> {
    let mut local = local.clone();
    // A transaction's amount and its split lines hold the same money, so they're one choice:
    // taking one side's amount with the other's lines can't add up.
    let mut choices = choices;
    for (path, choice) in choices.clone() {
        for (from, to) in [(".amount", ".splits"), (".splits", ".amount")] {
            if let Some(head) = path.strip_suffix(from).filter(|h| h.starts_with("Transaction ")) {
                choices.entry(format!("{head}{to}")).or_insert_with(|| choice.clone());
            }
        }
    }
    let mut context = Context {
        choices,
        conflicts: vec![],
    };
    let Some(source) = local.extra.get("sourceExport") else {
        if local.transactions.is_empty() && local.accounts.is_empty() {
            return Ok((keep_name(&local, remote), vec![]));
        }
        return Err(t!("This budget has no YNAB import baseline. Choose Replace from YNAB; a backup is created first."));
    };
    if local.extra.get("ynabPlanId") != remote.extra.get("ynabPlanId") {
        return Err(t!("Different YNAB plan. Select Replace from YNAB to switch plans."));
    }
    // The last import, as it was saved: its totals may differ from YNAB's when the person
    // chose to import anyway, so it's rebuilt without refusing them again.
    let base = ynab::import_unverified(source)?;
    let mut discard_local = std::collections::HashSet::new();
    let mut discard_remote = std::collections::HashSet::new();
    for t in local.transactions.iter().filter(|t| {
        t.extra.contains_key("scheduleId") && !base.transactions.iter().any(|a| a.id == t.id)
    }) {
        for r in remote.transactions.iter().filter(|r| {
            !base.transactions.iter().any(|a| a.id == r.id)
                && r.account_id == t.account_id
                && r.date == t.date
                && r.amount == t.amount
                && r.payee == t.payee
                && r.transfer_account_id == t.transfer_account_id
        }) {
            let path = format!("Duplicate scheduled entry {}", t.id);
            match context.choices.get(&path).map(String::as_str) {
                Some("local") => {
                    discard_remote.insert(r.id.clone());
                }
                Some("remote") => {
                    discard_local.insert(t.id.clone());
                }
                _ => context.conflicts.push(Conflict {
                    path,
                    local: Some(json!(t)),
                    remote: Some(json!(r)),
                    about: None,
                    field: None,
                    local_text: None,
                    remote_text: None,
                }),
            }
        }
    }
    local
        .transactions
        .retain(|t| !discard_local.contains(&t.id));
    remote
        .transactions
        .retain(|t| !discard_remote.contains(&t.id));
    let mut merged = remote.clone();
    merged.accounts = records_with(
        &base.accounts,
        &local.accounts,
        &remote.accounts,
        "Account",
        &mut context,
    )?;
    merged.categories = records_with(
        &base.categories,
        &local.categories,
        &remote.categories,
        "Category",
        &mut context,
    )?;
    merged.transactions = records_with(
        &base.transactions,
        &local.transactions,
        &remote.transactions,
        "Transaction",
        &mut context,
    )?;
    merged.assignments = records_with(
        &base.assignments,
        &local.assignments,
        &remote.assignments,
        "Assignment",
        &mut context,
    )?;
    for key in ["name", "currency"] {
        let a = json!(if key == "name" {
            &base.name
        } else {
            &base.currency
        });
        let l = json!(if key == "name" {
            &local.name
        } else {
            &local.currency
        });
        let r = json!(if key == "name" {
            &remote.name
        } else {
            &remote.currency
        });
        let value = field(Some(&a), Some(&l), Some(&r), key, &mut context)?
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        if key == "name" {
            merged.name = value
        } else {
            merged.currency = value
        }
    }
    // Locally configured planning tools are not replaced by absent API fields.
    for (k, v) in &local.extra {
        if !remote.extra.contains_key(k) {
            merged.extra.insert(k.clone(), v.clone());
        }
    }
    let a_months = base
        .extra
        .get("ynabMonths")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let l_months = local
        .extra
        .get("ynabMonths")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(months) = merged
        .extra
        .get_mut("ynabMonths")
        .and_then(Value::as_array_mut)
    {
        for m in months {
            let a = a_months
                .iter()
                .find(|v| v["month"] == m["month"])
                .and_then(|v| v.get("note"));
            let l = l_months
                .iter()
                .find(|v| v["month"] == m["month"])
                .and_then(|v| v.get("note"));
            if let Some(note) = field(
                a,
                l,
                m.get("note"),
                &format!("Month {} note", m["month"]),
                &mut context,
            )? {
                m["note"] = note;
            }
        }
    }
    for (key, label) in [("schedules", "Schedule"), ("payees", "Payee")] {
        let a = base
            .extra
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let l = local
            .extra
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let r = remote
            .extra
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let records: Vec<Value> = records_with(&a, &l, &r, label, &mut context)?;
        merged.extra.insert(key.into(), json!(records));
    }
    if context.conflicts.is_empty() {
        validate(&merged)?;
    }
    let paths: std::collections::HashSet<String> = context.conflicts.iter().map(|c| c.path.clone()).collect();
    let both = |path: &str| path.starts_with("Transaction ")
        && path.strip_suffix(".amount").is_some_and(|head| paths.contains(&format!("{head}.splits")));
    context.conflicts.retain(|c| !c.path.strip_suffix(".splits").is_some_and(|head| paths.contains(&format!("{head}.amount"))));
    let find = |b: &Budget, path: &str| path.strip_prefix("Transaction ").and_then(|p| p.split_once('.')).and_then(|(id, _)| b.transactions.iter().find(|t| t.id == id)).map(|t| json!(t));
    for c in &mut context.conflicts {
        (c.about, c.field) = describe(&c.path, &local, &remote);
        if both(&c.path) {
            c.field = Some("amount and split lines".into());
            let side = |t: Option<Value>| t.and_then(|t| Some(format!("{}: {}", cents(t["amount"].as_i64()?), value_text("Transaction t.splits", &t["splits"], &local, &remote)?)));
            c.local_text = side(find(&local, &c.path));
            c.remote_text = side(find(&remote, &c.path));
        } else {
            c.local_text = c.local.as_ref().and_then(|v| value_text(&c.path, v, &local, &remote));
            c.remote_text = c.remote.as_ref().and_then(|v| value_text(&c.path, v, &local, &remote));
        }
    }
    Ok((merged, context.conflicts))
}

#[cfg(test)]
fn records<T: serde::Serialize + serde::de::DeserializeOwned>(
    base: &[T],
    local: &[T],
    remote: &[T],
    label: &str,
) -> Result<Vec<T>, String> {
    let mut context = Context::default();
    let out = records_with(base, local, remote, label, &mut context)?;
    if context.conflicts.is_empty() {
        Ok(out)
    } else {
        Err("IMPORT CONFLICT".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choosing_split_lines_also_resolves_an_amount_changed_on_only_one_side() {
        let source = json!({"data":{"plan":{
            "id":"plan", "name":"Budget", "currency_format":{"iso_code":"USD"},
            "accounts":[{"id":"cash","name":"Cash","type":"checking","on_budget":true,
                "balance":-10000,"cleared_balance":0,"uncleared_balance":-10000}],
            "category_groups":[{"id":"living","name":"Living"}],
            "categories":[{"id":"income","name":"Inflow: Ready to Assign","internal":true},
                {"id":"food","name":"Food","category_group_id":"living"},
                {"id":"fun","name":"Fun","category_group_id":"living"}],
            "payees":[], "subtransactions":[], "scheduled_transactions":[], "scheduled_subtransactions":[],
            "months":[{"month":"2026-10-01","to_be_budgeted":0,"categories":[]}],
            "transactions":[{"id":"t","account_id":"cash","date":"2026-10-01",
                "amount":-10000,"category_id":"food","memo":"Original","cleared":"uncleared"}]
        }}});
        let base = ynab::import_unverified(&source).unwrap();
        for amount_is_local in [true, false] {
            let mut local = base.clone();
            let mut remote = base.clone();
            let (amount_side, category_side) = if amount_is_local { (&mut local, &mut remote) } else { (&mut remote, &mut local) };
            amount_side.transactions[0].amount = -2000;
            amount_side.transactions[0].splits[0].amount = -2000;
            category_side.transactions[0].splits[0].category_id = Some("fun".into());
            remote.transactions[0].memo = "Independent remote memo".into();
            let (_, conflicts) = merge_review(&local, remote.clone(), BTreeMap::new()).unwrap();
            assert_eq!(conflicts.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["Transaction t.splits"]);
            for choice in ["local", "remote"] {
                let selected = if choice == "local" { &local.transactions[0] } else { &remote.transactions[0] };
                let choices = BTreeMap::from([("Transaction t.splits".into(), choice.into())]);
                let (merged, remaining) = merge_review(&local, remote.clone(), choices).unwrap();
                assert!(remaining.is_empty());
                let t = &merged.transactions[0];
                assert_eq!((t.amount, t.splits[0].amount, &t.splits[0].category_id),
                    (selected.amount, selected.splits[0].amount, &selected.splits[0].category_id));
                assert_eq!(t.memo, "Independent remote memo");
            }
        }
    }
    #[test]
    fn an_import_keeps_the_name_the_user_gave_the_budget() {
        let plan = Budget { name: "Stress Test".into(), ..Budget::default() };
        let named = |name: &str| Budget { name: name.into(), ..Budget::default() };
        // Into an empty budget: the first launch's takes the plan's name, a named one keeps its own.
        assert_eq!(merge_review(&named("My budget"), plan.clone(), BTreeMap::new()).unwrap().0.name, "Stress Test");
        assert_eq!(merge_review(&named("Household"), plan.clone(), BTreeMap::new()).unwrap().0.name, "Household");
        assert_eq!(keep_name(&named("  "), plan.clone()).name, "Stress Test");
        // Replacing a budget an earlier import named takes the new plan's name.
        let mut earlier = named("Old plan");
        earlier.extra.insert("sourceExport".into(), json!({"data":{"plan":{"name":"Old plan"}}}));
        assert_eq!(keep_name(&earlier, plan.clone()).name, "Stress Test");
        earlier.name = "Renamed here".into();
        assert_eq!(keep_name(&earlier, plan).name, "Renamed here");
    }
    #[test]
    fn conflicts_are_named_by_their_record_and_field() {
        let mut b = crate::domain::tests::sample();
        b.transactions.push(crate::domain::tests::purchase());
        let named = |path: &str| describe(path, &b, &b);
        let field = |s: &str| Some(s.to_string());
        assert_eq!(named("Transaction tx.memo"), (field("Shop, 2026-10-15, Card"), field("memo")));
        assert_eq!(named("Transaction tx.splits"), (field("Shop, 2026-10-15, Card"), field("splits")));
        assert_eq!(named("Category food.name"), (field("Food"), field("name")));
        assert_eq!(named("Account cash"), (field("Cash"), None));
        assert_eq!(named("Assignment \"2026-10\":\"food\""), (field("Food, assigned in 2026-10"), None));
        assert_eq!(named("Month \"2026-10-01\" note"), (field("Note for 2026-10-01"), None));
        assert_eq!(named("name"), (field("This budget"), field("name")));
        assert_eq!(named("Transaction gone.memo"), (None, field("memo")));
    }
    #[test]
    fn delta_keeps_unchanged_entities_and_applies_deletions_and_month_categories() {
        let a = json!({"data":{"plan":{"id":"p","transactions":[{"id":"a","amount":1},{"id":"b","amount":2}],"months":[{"month":"2026-10-01","categories":[{"id":"c","balance":1},{"id":"d","balance":2}]}]}}});
        let d = json!({"data":{"server_knowledge":2,"plan":{"id":"p","transactions":[{"id":"a","deleted":true}],"months":[{"month":"2026-10-01","categories":[{"id":"c","balance":3}]}]}}});
        let out = merge_delta(&a, &d).unwrap();
        assert_eq!(out["data"]["plan"]["transactions"][0]["deleted"], true);
        assert_eq!(
            out["data"]["plan"]["transactions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            out["data"]["plan"]["months"][0]["categories"][1]["balance"],
            2
        );
    }
    #[test]
    fn independent_edits_merge_and_competing_edits_stop() {
        let a = json!({"id":"a","memo":"old","amount":100,"source":{"x":1}});
        let l = json!({"id":"a","memo":"local","amount":100,"source":{"x":1}});
        let r = json!({"id":"a","memo":"old","amount":200,"source":{"x":2}});
        let merged: Vec<Value> = records(&[a.clone()], &[l.clone()], &[r], "Transaction").unwrap();
        assert_eq!(merged[0]["memo"], "local");
        assert_eq!(merged[0]["amount"], 200);
        let r = json!({"id":"a","memo":"remote","amount":100});
        assert!(records(&[a.clone()], &[l], &[r], "Transaction").is_err());
        assert!(records::<Value>(&[a.clone()], &[], &[a], "Transaction")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn review_resolves_fields_and_preserves_nested_source_metadata() {
        let a = json!({"id":"t","memo":"old","splits":[{"id":"s","memo":"old","source":{"id":"original","extra":"keep"}}]});
        let mut l = a.clone();
        l["memo"] = json!("local");
        l["splits"][0]["memo"] = json!("local split");
        let mut r = a.clone();
        r["memo"] = json!("remote");
        let mut context = Context::default();
        let out: Vec<Value> = records_with(
            &[a.clone()],
            &[l.clone()],
            &[r.clone()],
            "Transaction",
            &mut context,
        )
        .unwrap();
        assert_eq!(context.conflicts.len(), 1);
        assert_eq!(context.conflicts[0].path, "Transaction t.memo");
        assert_eq!(out[0]["splits"][0]["source"]["extra"], "keep");
        context
            .choices
            .insert("Transaction t.memo".into(), "remote".into());
        context.conflicts.clear();
        let out: Vec<Value> = records_with(&[a], &[l], &[r], "Transaction", &mut context).unwrap();
        assert!(context.conflicts.is_empty());
        assert_eq!(out[0]["memo"], "remote");
        assert_eq!(out[0]["splits"][0]["memo"], "local split");
    }
}
