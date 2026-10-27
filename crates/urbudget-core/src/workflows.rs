use crate::{domain::*, engine};
use chrono::{Months, NaiveDate};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
fn assign(b: &mut Budget, month: &str, id: &str, amount: i64) {
    if let Some(a) = b
        .assignments
        .iter_mut()
        .find(|a| a.month == month && a.category_id == id)
    {
        a.amount = amount;
    } else {
        b.assignments.push(Assignment { extra: Default::default(),
            month: month.into(),
            category_id: id.into(),
            amount,
        });
    }
}
/// Each category's Assigned in `month`, to tell afterwards what an action changed.
fn assigned_in(b: &Budget, month: &str) -> HashMap<String, i64> {
    b.assignments
        .iter()
        .filter(|a| a.month == month)
        .map(|a| (a.category_id.clone(), a.amount))
        .collect()
}
/// Logs a change to a month's assignments: a line for the month's recent changes, and what it
/// did to each category's Assigned since `before` (from `assigned_in`), so each category can
/// list its moves. `kind` says how the change came about ("quickAssign" or "move"); `detail`
/// names the quick-assign option.
fn movement(
    b: &mut Budget,
    month: &str,
    description: String,
    kind: &str,
    detail: Option<&str>,
    before: &HashMap<String, i64>,
) {
    let after = assigned_in(b, month);
    let ids: HashSet<&String> = before.keys().chain(after.keys()).collect();
    let mut changes: Vec<(String, i64)> = ids
        .into_iter()
        .map(|id| {
            let change = after.get(id).unwrap_or(&0) - before.get(id).unwrap_or(&0);
            (id.clone(), change)
        })
        .filter(|(_, change)| *change != 0)
        .collect();
    changes.sort();
    let changes: Vec<Value> = changes
        .into_iter()
        .map(|(id, amount)| json!({"categoryId":id,"amount":amount}))
        .collect();
    let mut row = json!({"at":chrono::Utc::now().to_rfc3339(),"month":month,"description":description,"kind":kind,"changes":changes});
    if let Some(detail) = detail {
        row["detail"] = json!(detail);
    }
    let v = b.extra.entry("moneyMoves").or_insert(json!([]));
    if let Some(rows) = v.as_array_mut() {
        rows.push(row);
    }
}
fn strarg<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v[k].as_str().ok_or(format!("Missing {k}"))
}
/// The quick-assign options, as the inspector lists them.
pub const QUICK_ASSIGN: [&str; 8] = [
    "underfunded",
    "assignedLastMonth",
    "spentLastMonth",
    "averageAssigned",
    "averageSpent",
    "reduceOverfunding",
    "resetAvailable",
    "resetAssigned",
];
/// The option's name as the inspector shows it, not its internal name.
fn quick_assign_label(mode: &str) -> &str {
    match mode {
        "assignedLastMonth" => "assigned last month",
        "spentLastMonth" => "spent last month",
        "averageAssigned" => "average assigned",
        "averageSpent" => "average spent",
        "reduceOverfunding" => "reduce overfunding",
        "resetAvailable" => "reset available amounts",
        "resetAssigned" => "reset assigned amounts",
        other => other,
    }
}
fn row<'a>(s: &'a Summary, id: &str) -> Result<&'a CategorySummary, String> {
    s.categories.iter().find(|r| r.id == id).ok_or_else(|| format!("No budget row for category {id}."))
}
/// What a quick-assign option assigns in `month`: each selected category's new Assigned, in the
/// order it's assigned. It changes nothing, so the inspector can show every option's effect and
/// Apply does exactly what was shown. `history` is `engine::calculate_history(b, month)`.
fn auto_assign_plan(
    b: &Budget,
    month: &str,
    mode: &str,
    selected: &HashSet<&str>,
    limit: bool,
    history: &BTreeMap<String, Summary>,
) -> Result<Vec<(String, i64)>, String> {
    let date = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").map_err(|_| "Invalid month")?;
    let summary = history.get(month).ok_or("Missing month")?;
    if mode == "underfunded" {
        return underfunded_plan(b, month, selected, limit, history);
    }
    let mut plan = vec![];
    let assignment_index = crate::targets::AssignmentIndex::new(b);
    for c in b.categories.iter().filter(|c| selected.contains(c.id.as_str())) {
        let s = row(summary, &c.id)?;
        let value = match mode {
            "underfunded" => s.assigned + s.needed.max((-s.available).max(0)),
            "resetAssigned" => 0,
            "resetAvailable" => s.assigned - s.available,
            "reduceOverfunding" => {
                let explicit = c.target > 0 || c.extra.get("goal").is_some_and(|g| !g.is_null());
                if c.payment_account().is_some() && !explicit {
                    // Without a target a card payment needs what its card owes (the engine's
                    // target_funding_required) and its scheduled payments, as in YNAB.
                    let keep = s.target_funding_required.max(s.upcoming).max(0);
                    s.assigned - (s.available - keep).max(0)
                } else if c.target == 0 || c.payment_account().is_some() {
                    s.assigned
                } else {
                    // Solve for the smallest assignment meeting the same goal and scheduled obligations.
                    let mut low = 0;
                    let mut high = s.assigned.max(0);
                    let mut unsnoozed = c.clone();
                    if let Some(g) = unsnoozed
                        .extra
                        .get_mut("goal")
                        .and_then(Value::as_object_mut)
                    {
                        g.remove("goal_snoozed_at");
                    }
                    while low < high {
                        let mid = (low + high) / 2;
                        let available = s.available - s.assigned + mid;
                        let needed = crate::targets::needed_indexed(
                            &b, &unsnoozed, month, mid, available, s.carried, &history, &assignment_index,
                        )
                        .max(s.upcoming - available)
                        .max(-available);
                        if needed > 0 {
                            low = mid + 1
                        } else {
                            high = mid
                        }
                    }
                    s.assigned - (s.assigned - low).max(0).min(s.available.max(0))
                }
            }
            // A card payment category's activity is payments and moved reserves, not spending,
            // so YNAB's spending options leave it out.
            "spentLastMonth" | "averageSpent" if c.payment_account().is_some() => s.assigned,
            "assignedLastMonth" | "spentLastMonth" | "averageAssigned" | "averageSpent" => {
                let months = if mode.starts_with("average") { 12 } else { 1 };
                let mut values = vec![];
                let mut started = false;
                for n in (1..=months).rev() {
                    let key = date
                        .checked_sub_months(Months::new(n))
                        .ok_or("Invalid month")?
                        .format("%Y-%m")
                        .to_string();
                    let previous = history
                        .get(&key)
                        .and_then(|h| h.categories.iter().find(|s| s.id == c.id));
                    let (a, spent) = previous
                        .map(|s| (s.assigned, -s.activity))
                        .unwrap_or((0, 0));
                    let is_spent = mode == "spentLastMonth" || mode == "averageSpent";
                    // Averages count from the first month the category was used, assigned to or
                    // spent from, as YNAB's do: a card paid from July but first assigned in
                    // September averages over three months.
                    started |= a != 0 || spent != 0;
                    if started || months == 1 {
                        values.push(if is_spent { spent } else { a });
                    }
                }
                if values.is_empty() {
                    0
                } else {
                    {
                        let sum = values.iter().sum::<i64>();
                        let n = values.len() as i64;
                        (sum + n / 2 * sum.signum()) / n
                    }
                }
                .max(0)
            }
            _ => return Err("Unknown quick-assign option.".into()),
        };
        plan.push((c.id.clone(), value));
    }
    Ok(plan)
}
/// Global Underfunded funds overspending, dated obligations, end-of-month targets,
/// future targets, then card payments. Weekly deadlines are cumulative checkpoints.
/// Selected categories still receive their full shortfall, even beyond Ready to Assign.
fn underfunded_plan(
    b: &Budget,
    month: &str,
    selected: &HashSet<&str>,
    limit: bool,
    history: &BTreeMap<String, Summary>,
) -> Result<Vec<(String, i64)>, String> {
    struct Request { category: usize, stage: u8, due: String, amount: i64 }
    let chosen: Vec<&Category> = b.categories.iter().filter(|c| selected.contains(c.id.as_str())).collect();
    let first = engine::calculate(b, month)?;
    let capacity = engine::assignment_capacity(b, month, &first);
    let schedules = crate::schedules::upcoming_details(b, month)?;
    let assignments = crate::targets::AssignmentIndex::new(b);
    let as_of = b.extra.get("calculationAsOfMonth").and_then(Value::as_str).map(String::from)
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m").to_string());
    let target_need = |c: &Category, r: &CategorySummary| crate::targets::needed_indexed_at(
        c, month, r.assigned, r.available, r.carried, history, &assignments, &as_of);
    let target_priority = |c: &Category| {
        let due = crate::targets::due_date(c, month);
        let goal = c.extra.get("goal");
        let kind = goal.and_then(|g| g["goal_type"].as_str()).unwrap_or("MF");
        let end_of_month = (kind == "MF" || (kind == "NEED"
            && goal.is_some_and(|g| g["goal_cadence"].as_i64().unwrap_or(1) == 1
                && g["goal_cadence_frequency"].as_u64().unwrap_or(1) == 1)))
            && goal.is_none_or(|g| g["goal_day"].as_u64().is_none());
        let stage = if due.get(..7).is_some_and(|m| m > month) { 3 } else if end_of_month { 2 } else { 1 };
        (stage, due)
    };
    let mut requests = vec![];
    let mut payment_requests = vec![];
    for (index, c) in chosen.iter().enumerate() {
        let r = row(&first, &c.id)?;
        if c.payment_account().is_some() {
            if !limit {
                payment_requests.push(Request { category: index, stage: 0, due: String::new(), amount: 0 });
                continue;
            }
            if let Some(scheduled) = schedules.get(&c.id) {
                payment_requests.push(Request { category: index, stage: 4, due: scheduled.first_due.clone(), amount: 0 });
            }
            let explicit = c.target > 0 || c.extra.get("goal").is_some_and(|g| !g.is_null());
            payment_requests.push(Request { category: index, stage: if explicit { 5 } else { 6 },
                due: crate::targets::due_date(c, month), amount: 0 });
            continue;
        }
        if !limit {
            requests.push(Request { category: index, stage: 0, due: String::new(), amount: r.needed.max(-r.available).max(0) });
            continue;
        }
        let target = if crate::targets::undated_balance(c) { 0 } else { target_need(c, r) };
        let scheduled = (r.upcoming - r.available).max(0);
        if r.available < 0 {
            // Overspending gets the category's entire target and scheduled shortfall
            // before other categories, rather than only its first weekly installment.
            requests.push(Request { category: index, stage: 0, due: String::new(),
                amount: target.max(scheduled).max(-r.available) });
            continue;
        }
        if let Some(due) = schedules.get(&c.id).filter(|_| scheduled > 0) {
            // Keep a category's whole monthly scheduled total together. Earliest due
            // is our category deadline; exact YNAB tie behavior still needs a fixture.
            requests.push(Request { category: index, stage: 1, due: due.first_due.clone(), amount: scheduled });
        }
        if let Some(checkpoints) = crate::targets::weekly_funding_checkpoints(c, month, r.assigned, r.carried, &as_of) {
            for (due, amount) in checkpoints {
                if amount > 0 { requests.push(Request { category: index, stage: 1, due, amount }); }
            }
        } else if target > 0 {
            let (stage, due) = target_priority(c);
            requests.push(Request { category: index, stage, due, amount: target });
        }
    }
    requests.sort_by(|a, b| (a.stage, &a.due, a.category).cmp(&(b.stage, &b.due, b.category)));
    let mut increases = vec![0; chosen.len()];
    let mut given = 0;
    for request in requests {
        let mut n = (request.amount - increases[request.category]).max(0);
        if limit { n = n.min((capacity - given).max(0)); }
        given += n;
        increases[request.category] += n;
    }
    let mut plan = vec![];
    for (index, c) in chosen.iter().enumerate().filter(|(_, c)| c.payment_account().is_none()) {
        plan.push((c.id.clone(), row(&first, &c.id)?.assigned + increases[index]));
    }
    if !payment_requests.is_empty() {
        let mut working = b.clone();
        for (id, n) in &plan {
            assign(&mut working, month, id, *n);
        }
        payment_requests.sort_by(|a, b| (a.stage, &a.due, a.category).cmp(&(b.stage, &b.due, b.category)));
        for request in payment_requests {
            let c = chosen[request.category];
            // Covering card purchases in spending categories can increase the payment
            // reserve. Recalculate before each payment obligation to avoid funding twice.
            let current = engine::calculate(&working, month)?;
            let r = row(&current, &c.id)?;
            let mut n = match request.stage {
                4 => (r.upcoming - r.available).max(0),
                5 => (if crate::targets::undated_balance(c) { 0 } else {
                    crate::targets::needed_indexed(&working, c, month, r.assigned, r.available, r.carried,
                        history, &crate::targets::AssignmentIndex::new(&working))
                }).max(-r.available).max(0),
                _ => r.needed.max(-r.available).max(0),
            };
            if limit { n = n.min(engine::assignment_capacity(&working, month, &current)); }
            assign(&mut working, month, &c.id, r.assigned + n);
        }
        // Each category appears once in previews and money-movement records even if
        // a schedule and a target funded it in separate passes.
        for c in chosen.iter().filter(|c| c.payment_account().is_some()) {
            let amount = working.assignments.iter().find(|a| a.month == month && a.category_id == c.id)
                .map(|a| a.amount).unwrap_or(0);
            plan.push((c.id.clone(), amount));
        }
    }
    Ok(plan)
}
/// One category's Assigned before and after a quick-assign option.
#[derive(Debug, serde::Serialize)]
pub struct AssignChange {
    pub id: String,
    pub before: i64,
    pub after: i64,
}
/// Every quick-assign option's effect on the selected categories in `month`, for the inspector:
/// the categories each would change, or None for an option that can't run. One history serves
/// them all, and nothing is copied or changed.
pub fn auto_assign_preview(
    b: &Budget,
    month: &str,
    categories: &[String],
    limit: bool,
) -> Result<HashMap<String, Option<Vec<AssignChange>>>, String> {
    let selected: HashSet<&str> = categories.iter().map(String::as_str).collect();
    if selected.is_empty() {
        return Err("Select at least one category.".into());
    }
    let history = engine::calculate_history(b, month)?;
    let before = assigned_in(b, month);
    Ok(QUICK_ASSIGN
        .iter()
        .map(|mode| {
            let changes = auto_assign_plan(b, month, mode, &selected, limit, &history).ok().map(|plan| {
                plan.into_iter()
                    .filter_map(|(id, after)| {
                        let before = *before.get(&id).unwrap_or(&0);
                        (after != before).then_some(AssignChange { id, before, after })
                    })
                    .collect()
            });
            (mode.to_string(), changes)
        })
        .collect())
}
pub fn apply(mut b: Budget, action: &Value) -> Result<Budget, String> {
    validate(&b)?;
    match strarg(action, "type")? {
        "assignAmount" => {
            let month = strarg(action, "month")?; let id = strarg(action, "categoryId")?;
            if !valid_month(month) || !b.categories.iter().any(|c| c.id == id) { return Err("Choose a valid category and month.".into()); }
            let amount = action["amount"].as_i64().ok_or("Enter an assigned amount.")?;
            let before = assigned_in(&b, month);
            assign(&mut b, month, id, amount);
            if before.get(id).copied().unwrap_or_default() != amount {
                let name = b.categories.iter().find(|c| c.id == id).unwrap().name.clone();
                movement(&mut b, month, format!("Updated assigned amount for {name}"), "assign", None, &before);
            }
        }
        "setBudgetPlanning" => {
            let month = strarg(action, "month")?;
            if !valid_month(month) { return Err("Choose a valid month.".into()); }
            let note = strarg(action, "note")?;
            if note.len() > 100_000 { return Err("The month note is too long.".into()); }
            let notes = b.extra.entry("ynabMonths").or_insert(json!([])).as_array_mut().ok_or("Invalid saved month notes.")?;
            if let Some(existing) = notes.iter_mut().find(|n| n["month"].as_str().is_some_and(|s| s.starts_with(month))) { existing["note"] = json!(note); }
            else { notes.push(json!({"month":format!("{month}-01"),"note":note})); }
            if action.get("expectedIncome").is_some() { b.extra.insert("expectedIncome".into(), action["expectedIncome"].clone()); }
        }
        "organizeCategory" => {
            let id = strarg(action, "id")?;
            let group = strarg(action, "group")?.trim();
            if group.is_empty() || group.len() > 200 { return Err("Enter a category group name.".into()); }
            let mut order: Vec<String> = b.extra.get("categoryOrder").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
            for c in &b.categories { if !order.contains(&c.id) { order.push(c.id.clone()); } }
            let category = b.categories.iter_mut().find(|c| c.id == id).ok_or("Category not found.")?;
            category.group = group.into();
            order.retain(|item| item != id);
            let before = action["beforeId"].as_str().filter(|before| *before != id);
            let index = before.and_then(|before| order.iter().position(|item| item == before))
                .unwrap_or_else(|| order.iter().rposition(|item| b.categories.iter().any(|c| &c.id == item && c.group == group)).map_or(order.len(), |i| i + 1));
            order.insert(index, id.into());
            b.extra.insert("categoryOrder".into(), json!(order));
            let mut groups: Vec<String> = b.extra.get("groupOrder").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
            for c in &b.categories { if !groups.contains(&c.group) { groups.push(c.group.clone()); } }
            if action.get("groupBefore").is_some() {
                groups.retain(|g| g != group);
                let index = action["groupBefore"].as_str().and_then(|before| groups.iter().position(|g| g == before)).unwrap_or(groups.len());
                groups.insert(index, group.into());
            }
            b.extra.insert("groupOrder".into(), json!(groups));
        }
        "renameGroup" => {
            let from = strarg(action, "from")?;
            let to = strarg(action, "to")?.trim();
            if to.is_empty() || to.len() > 200 { return Err("Enter a group name.".into()); }
            for c in &mut b.categories { if c.group == from { c.group = to.into(); } }
            let mut groups: Vec<String> = b.extra.get("groupOrder").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(|g| if g == from { to.to_owned() } else { g.to_owned() }).collect();
            for c in &b.categories { if !groups.contains(&c.group) { groups.push(c.group.clone()); } }
            let mut seen = HashSet::new(); groups.retain(|g| seen.insert(g.clone()));
            b.extra.insert("groupOrder".into(), json!(groups));
        }
        "deleteCategory" => {
            let id = strarg(action, "id")?;
            let replacement = action["replacement"].as_str().filter(|id| *id != "__rta");
            let category = b.categories.iter().find(|c| c.id == id).ok_or("Category not found.")?;
            if category.payment_account().is_some() { return Err("Credit payment categories belong to their accounts and cannot be deleted.".into()); }
            if replacement == Some(id) || replacement.is_some_and(|r| !b.categories.iter().any(|c| c.id == r && c.payment_account().is_none())) { return Err("Choose another ordinary category or Ready to assign.".into()); }
            for transaction in &mut b.transactions {
                for split in &mut transaction.splits { if split.category_id.as_deref() == Some(id) { split.category_id = replacement.map(str::to_owned); split.extra.insert("unassigned".into(), json!(replacement.is_none())); } }
            }
            for schedule in b.extra.get_mut("schedules").and_then(Value::as_array_mut).into_iter().flatten() {
                for split in schedule["transaction"]["splits"].as_array_mut().into_iter().flatten() { if split["categoryId"] == id { split["categoryId"] = json!(replacement); split["unassigned"] = json!(replacement.is_none()); } }
            }
            for collection in ["payees", "payeeRules", "loans"] {
                for record in b.extra.get_mut(collection).and_then(Value::as_array_mut).into_iter().flatten() { if record["categoryId"] == id { record["categoryId"] = if collection == "loans" { json!(replacement.unwrap_or("")) } else { json!(replacement) }; } }
            }
            let source_name = category.name.clone();
            let removed: Vec<_> = b.assignments.iter().filter(|a| a.category_id == id).cloned().collect();
            let before: Vec<_> = removed.iter().map(|a| (a.month.clone(), assigned_in(&b, &a.month))).collect();
            b.assignments.retain(|a| a.category_id != id);
            if let Some(replacement) = replacement {
                for mut assignment in removed {
                    if let Some(existing) = b.assignments.iter_mut().find(|a| a.category_id == replacement && a.month == assignment.month) { existing.amount = existing.amount.checked_add(assignment.amount).ok_or("Merged assignments exceed the supported range.")?; for (key,value) in assignment.extra { existing.extra.entry(key).or_insert(value); } }
                    else { assignment.category_id = replacement.into(); b.assignments.push(assignment); }
                }
            }
            b.categories.retain(|c| c.id != id);
            if let Some(order) = b.extra.get_mut("categoryOrder").and_then(Value::as_array_mut) { order.retain(|v| v != id); }
            for (month, before) in before { movement(&mut b, &month, format!("Moved history from {source_name}"), "merge", Some(&source_name), &before); }
        }
        "autoAssign" => {
            let month = strarg(action, "month")?;
            let mode = strarg(action, "mode")?;
            // A confirmed preview may wait behind a save until after midnight. Recalculate
            // the preview instead of applying a different current/future-month target plan.
            let original_reference = if let Some(expected) = action.get("expectedAsOfMonth") {
                if expected.as_str() != Some(engine::calculation_month(&b).as_str()) {
                    return Err("The calendar month changed. Refresh the Quick assign preview before applying it.".into());
                }
                // All calculations in this action share the accepted instant, even if the
                // wall clock rolls over during a large plan. Restore the original metadata below.
                Some(b.extra.insert("calculationAsOfMonth".into(), expected.clone()))
            } else { None };
            let selected: HashSet<_> = action["categories"]
                .as_array()
                .ok_or("Select categories")?
                .iter()
                .filter_map(Value::as_str)
                .collect();
            if selected.is_empty() {
                return Err("Select at least one category.".into());
            }
            let history = engine::calculate_history(&b, month)?;
            let before = assigned_in(&b, month);
            let limit = action["limitToAvailable"] == true;
            for (id, n) in auto_assign_plan(&b, month, mode, &selected, limit, &history)? {
                assign(&mut b, month, &id, n);
            }
            let label = quick_assign_label(mode);
            movement(
                &mut b,
                month,
                format!("Quick assign: {label} ({} categories)", selected.len()),
                "quickAssign",
                Some(label),
                &before,
            );
            if let Some(original) = original_reference {
                match original {
                    Some(value) => { b.extra.insert("calculationAsOfMonth".into(), value); }
                    None => { b.extra.remove("calculationAsOfMonth"); }
                }
            }
        }
        "moveMoney" => {
            let month = strarg(action, "month")?;
            let from = strarg(action, "from")?;
            let to = strarg(action, "to")?;
            let amount = action["amount"]
                .as_i64()
                .filter(|x| *x > 0)
                .ok_or("Enter a positive amount.")?;
            if from == to {
                return Err("Choose different categories.".into());
            }
            let s = engine::calculate(&b, month)?;
            let available = if from == "__rta" {
                s.ready_to_assign
            } else {
                s.categories
                    .iter()
                    .find(|c| c.id == from)
                    .ok_or("Category not found")?
                    .available
            };
            if amount > available {
                return Err("The source does not have that much available.".into());
            }
            let before = assigned_in(&b, month);
            for (id, delta) in [(from, -amount), (to, amount)] {
                if id != "__rta" {
                    let old = s
                        .categories
                        .iter()
                        .find(|c| c.id == id)
                        .ok_or("Category not found")?
                        .assigned;
                    assign(&mut b, month, id, old + delta);
                }
            }
            let name = |id: &str| {
                b.categories
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
                    .unwrap_or("Unassigned".into())
            };
            let description = format!(
                "{}.{:02} {}: {} → {}",
                amount / 100,
                amount % 100,
                b.currency,
                name(from),
                name(to)
            );
            movement(&mut b, month, description, "move", None, &before);
        }
        "renamePayee" => {
            let from = strarg(action, "from")?;
            let to = strarg(action, "to")?.trim();
            if to.is_empty() || to.len() > 200 {
                return Err("Enter a payee name (up to 200 characters).".into());
            }
            for t in &mut b.transactions {
                if t.payee == from {
                    t.payee = to.into()
                };
                for s in &mut t.splits {
                    if s.extra.get("payee") == Some(&json!(from)) {
                        s.extra.insert("payee".into(), json!(to));
                    }
                }
            }
            if let Some(payees) = b.extra.get_mut("payees").and_then(Value::as_array_mut) {
                for p in payees.iter_mut() {
                    if p["name"] == from {
                        p["name"] = json!(to)
                    }
                }
                // A rename into an existing name is a merge. Preserve the default-category
                // entry, matching desktop's tidyPayees, across every native caller.
                let keep = payees.iter().position(|p| p["name"] == to && p["categoryId"].as_str().is_some_and(|s| !s.is_empty()))
                    .or_else(|| payees.iter().position(|p| p["name"] == to));
                if let Some(keep) = keep {
                    let mut index = 0;
                    payees.retain(|p| { let retain = p["name"] != to || index == keep; index += 1; retain });
                }
            }
            if let Some(schedules) = b.extra.get_mut("schedules").and_then(Value::as_array_mut) {
                for s in schedules {
                    if s["transaction"]["payee"] == from {
                        s["transaction"]["payee"] = json!(to)
                    };
                    if let Some(parts) = s["transaction"]["splits"].as_array_mut() {
                        for p in parts {
                            if p["payee"] == from {
                                p["payee"] = json!(to)
                            }
                        }
                    }
                }
            }
            if let Some(rules) = b.extra.get_mut("payeeRules").and_then(Value::as_array_mut) {
                for r in rules {
                    if r["payee"] == from {
                        r["payee"] = json!(to)
                    }
                }
            }
        }
        "savePayee" => {
            let mut record = action["record"].as_object().cloned().ok_or("Invalid payee.")?;
            let id = record.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("Every payee needs an ID.")?.to_owned();
            let name = record.get("name").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
            if name.is_empty() || name.len() > 200 { return Err("Enter a payee name (up to 200 characters).".into()); }
            if let Some(category) = record.get("categoryId").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                if category != "__rta" && !b.categories.iter().any(|c| c.id == category && c.payment_account().is_none()) { return Err("Choose an ordinary category or Ready to assign.".into()); }
            }
            let payees = b.extra.entry("payees").or_insert(json!([])).as_array_mut().ok_or("Invalid saved payees.")?;
            let index = payees.iter().position(|p| p["id"] == id);
            let from = index.and_then(|i| payees[i]["name"].as_str()).or_else(|| action["from"].as_str()).unwrap_or(&name).to_owned();
            record.insert("name".into(), json!(from));
            if let Some(index) = index { payees[index] = json!(record); } else { payees.push(json!(record)); }
            b = apply(b, &json!({"type":"renamePayee","from":from,"to":name}))?;
        }
        "reorderPayeeRule" => {
            let id = strarg(action, "id")?;
            let direction = strarg(action, "direction")?;
            if !matches!(direction, "up" | "down") { return Err("Choose up or down.".into()); }
            let rules = b.extra.get_mut("payeeRules").and_then(Value::as_array_mut).ok_or("No payee rules exist.")?;
            let index = rules.iter().position(|r| r["id"] == id).ok_or("Payee rule not found.")?;
            let next = if direction == "up" { index.saturating_sub(1) } else { (index + 1).min(rules.len() - 1) };
            rules.swap(index, next);
        }
        "reapplyPayeeRules" => {
            let ids: Option<Vec<String>> = action.get("ruleIds").and_then(Value::as_array).map(|ids| {
                ids.iter().filter_map(Value::as_str).map(String::from).collect()
            });
            let only: Option<HashSet<&str>> = ids.as_ref().map(|ids| ids.iter().map(String::as_str).collect());
            reapply_payee_rules(&mut b, only.as_ref());
        }
        "bulk" => {
            let ids: HashSet<_> = action["ids"]
                .as_array()
                .ok_or("Select transactions")?
                .iter()
                .filter_map(Value::as_str)
                .collect();
            let op = strarg(action, "operation")?;
            let account = action["accountId"].as_str().unwrap_or("all");
            if op == "delete" {
                b.transactions.retain(|t| !ids.contains(t.id.as_str()));
            } else if op == "duplicate" {
                let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
                // A copy is added now, after the transaction it copies.
                let added = stamp_now();
                let copies: Vec<_> = b
                    .transactions
                    .iter()
                    .filter(|t| ids.contains(t.id.as_str()))
                    .enumerate()
                    .map(|(i, t)| {
                        let mut t = t.clone();
                        t.id = format!("copy-{now}-{i}");
                        t.status = Cleared::Uncleared;
                        t.transfer_status =
                            t.transfer_account_id.as_ref().map(|_| Cleared::Uncleared);
                        t.extra.remove("source");
                        t.extra.remove("scheduleId");
                        t.extra.remove("bankImportIds");
                        t.extra.remove("bankImportMatches");
                        t.extra.remove("transferSource");
                        t.extra.insert("approved".into(), json!(true));
                        t.set_created_at(&added);
                        for s in &mut t.splits {
                            s.extra.remove("bankImportIds");
                            s.extra.remove("id");
                            s.extra.remove("source");
                            s.extra.remove("transferSource");
                            if crate::ledger::transfer(s).is_some() {
                                s.extra.insert("transferStatus".into(), json!("uncleared"));
                            }
                        }
                        t
                    })
                    .collect();
                b.transactions.extend(copies);
            } else {
                for t in b
                    .transactions
                    .iter_mut()
                    .filter(|t| ids.contains(t.id.as_str()))
                {
                    match op {
                        "approve" => {
                            t.extra.insert("approved".into(), json!(true));
                        }
                        "flag" => {
                            if account == "all" || t.account_id == account {
                                t.extra.insert("flag".into(), action["value"].clone());
                            } else if t.transfer_account_id.as_deref() == Some(account) {
                                let source = t.extra.entry("transferSource").or_insert(json!({}));
                                if !source.is_object() {
                                    *source = json!({});
                                }
                                source["flag_color"] = action["value"].clone();
                            } else {
                                for part in &mut t.splits {
                                    if crate::ledger::transfer(part) == Some(account) {
                                        let source =
                                            part.extra.entry("transferSource").or_insert(json!({}));
                                        if !source.is_object() {
                                            *source = json!({});
                                        }
                                        source["flag_color"] = action["value"].clone();
                                    }
                                }
                            }
                        }
                        "payee" => t.payee = strarg(action, "value")?.into(),
                        "memo" => t.memo = strarg(action, "value")?.into(),
                        "category" => {
                            if t.transfer_account_id.is_some()
                                || t.splits.len() != 1
                                || t.splits
                                    .iter()
                                    .any(|s| crate::ledger::transfer(s).is_some())
                            {
                                return Err("Bulk categorization only supports ordinary, unsplit transactions. Select those entries separately.".into());
                            }
                            t.splits[0].category_id = action["value"]
                                .as_str()
                                .filter(|x| *x != "__rta")
                                .map(String::from);
                        }
                        "status" => {
                            let status: Cleared = serde_json::from_value(action["value"].clone())
                                .map_err(|_| "Invalid status")?;
                            if status == Cleared::Reconciled {
                                return Err("Use Reconcile to lock a balance.".into());
                            }
                            if account == "all" || account == t.account_id {
                                if t.status != Cleared::Reconciled {
                                    t.status = status;
                                }
                            }
                            if t.transfer_account_id.as_deref() == Some(account)
                                && t.transfer_status != Some(Cleared::Reconciled)
                            {
                                t.transfer_status = Some(status);
                            }
                            for s in &mut t.splits {
                                if crate::ledger::transfer(s) == Some(account)
                                    && crate::ledger::status(s) != Cleared::Reconciled
                                {
                                    s.extra.insert("transferStatus".into(), json!(status));
                                }
                            }
                        }
                        _ => return Err("Unknown bulk operation".into()),
                    }
                }
            }
        }
        _ => return Err("Unknown operation".into()),
    }
    validate(&b)?;
    Ok(b)
}
/// Bank text as rules read it: lower case, with punctuation and runs of spaces as one space,
/// so "SQ *BLUE-BOTTLE  #12" reads "sq blue bottle 12".
pub fn normalize_payee(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
    }
    out.truncate(out.trim_end().len());
    out
}
/// Whether `pattern` appears in `text` (both normalized) at the start of a word, so "uber"
/// matches "uber eats" but not "kuber".
fn at_word_start(text: &str, pattern: &str) -> bool {
    !pattern.is_empty() && format!(" {text}").contains(&format!(" {pattern}"))
}
/// The text rules match: what the bank file said, for a transaction imported from one, else the payee.
pub fn bank_text(t: &Transaction) -> &str {
    t.extra
        .get("importedPayee")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&t.payee)
}
/// The rule for some bank text: of the rules whose text it contains (ignoring case and
/// punctuation, from the start of a word), the longest, so "Uber Eats" wins over "Uber".
/// Rules of the same length go in list order.
pub fn payee_rule<'a>(b: &'a Budget, text: &str) -> Option<&'a Value> {
    let text = normalize_payee(text);
    let mut best: Option<(&Value, usize)> = None;
    for r in b.extra.get("payeeRules").and_then(Value::as_array)? {
        let pattern = normalize_payee(r["match"].as_str().unwrap_or(""));
        let length = pattern.chars().count();
        if at_word_start(&text, &pattern) && best.is_none_or(|(_, n)| length > n) {
            best = Some((r, length));
        }
    }
    best.map(|(r, _)| r)
}
/// A category id a bank import uses for rows that still need one.
pub(crate) fn placeholder(b: &Budget, id: &str) -> bool {
    id == "local-uncategorized"
        || b.categories.iter().any(|c| {
            c.id == id && (c.name == "Uncategorized" || c.name == "Uncategorized Transactions")
        })
}
/// Renames a payee by the best rule for its bank text and fills in a missing category: the
/// rule's, or else the default category of the payee it ends up with, when it came from a bank
/// file or a rule named it. `fresh` is a transaction being added; on one already in the budget,
/// an inflow without a category is left in Ready to Assign. `only` limits it to transactions
/// whose best rule is one of these. Returns whether anything changed.
fn apply_rule(b: &Budget, t: &mut Transaction, fresh: bool, only: Option<&HashSet<&str>>) -> bool {
    if t.transfer_account_id.is_some() {
        return false;
    }
    let rule = payee_rule(b, bank_text(t));
    if only.is_some_and(|ids| !rule.and_then(|r| r["id"].as_str()).is_some_and(|id| ids.contains(id))) {
        return false;
    }
    let mut changed = false;
    // A rule may only set a category: an empty "rename to" keeps the payee.
    let name = rule.and_then(|r| r["payee"].as_str()).map(str::trim).filter(|n| !n.is_empty());
    if let Some(name) = name {
        if t.payee != name {
            t.payee = name.into();
            changed = true;
        }
    }
    let default = name.is_some() || t.extra.contains_key("importedPayee");
    fill_category(b, t, fresh, rule.and_then(|r| r["categoryId"].as_str()), default) || changed
}
fn fill_category(b: &Budget, t: &mut Transaction, fresh: bool, rule: Option<&str>, default: bool) -> bool {
    let on_budget = b.accounts.iter().any(|a| a.id == t.account_id && a.on_budget());
    let [split] = t.splits.as_slice() else { return false };
    let empty = match split.category_id.as_deref() {
        None => fresh || t.amount < 0,
        Some(id) => placeholder(b, id),
    };
    if !on_budget || !empty || crate::ledger::transfer(split).is_some() {
        return false;
    }
    let payee_default = || {
        let payees = b.extra.get("payees")?.as_array()?;
        payees.iter().find(|p| p["name"] == t.payee.as_str())?["categoryId"].as_str()
    };
    let category = rule
        .or_else(|| default.then(payee_default).flatten())
        .filter(|id| !placeholder(b, id) && b.categories.iter().any(|c| c.id == *id));
    match category {
        Some(id) if split.category_id.as_deref() != Some(id) => {
            t.splits[0].category_id = Some(id.into());
            true
        }
        _ => false,
    }
}
/// Rules for a transaction being added (see `apply_rule`).
pub fn apply_payee_rules(b: &Budget, t: &mut Transaction) {
    apply_rule(b, t, true, None);
}
/// Runs the rules again over the budget's transactions, or only where the best rule is one of
/// `only`; returns how many changed.
pub fn reapply_payee_rules(b: &mut Budget, only: Option<&HashSet<&str>>) -> usize {
    let mut list = std::mem::take(&mut b.transactions);
    let changed = list.iter_mut().map(|t| apply_rule(b, t, false, only)).filter(|c| *c).count();
    b.transactions = list;
    changed
}
/// Adds `transaction`, or puts it in place of the one with its id.
pub fn upsert_transaction(mut budget: Budget, mut transaction: Transaction) -> Result<Budget, String> {
    let is_new = !budget.transactions.iter().any(|t| t.id == transaction.id);
    if is_new {
        apply_payee_rules(&budget, &mut transaction);
    }
    let id = transaction.id.clone();
    if let Some(existing) = budget.transactions.iter_mut().find(|t| t.id == id) {
        // An edit never changes when the transaction was added, whatever the window sends
        // (an editor that rebuilds the record leaves it out).
        match existing.created_at() {
            Some(at) => transaction.set_created_at(at),
            None => {
                transaction.extra.remove("createdAt");
            }
        }
        *existing = transaction;
    } else {
        // Added now; the patch back gives the window the same time.
        transaction.set_created_at(&stamp_now());
        budget.transactions.push(transaction);
    }
    if is_new {
        // The loan helper validates the assembled input before any derived ledger work.
        crate::loans::apply_new_payments(&mut budget, &std::collections::HashSet::from([id]))?;
    }
    validate(&budget)?;
    Ok(budget)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tests::*;
    #[test]
    fn merging_payees_preserves_the_default_and_renames_splits_schedules_and_rules() {
        let mut b = sample();
        let mut t = purchase();
        t.payee = "Old name".into();
        t.splits[0].extra.insert("payee".into(), json!("Old name"));
        b.extra.insert("schedules".into(), json!([{"id":"scheduled","nextDate":"2026-11-15","anchorDate":"2026-11-15","frequency":"monthly","paused":false,"transaction":t}]));
        b.transactions.push(t);
        b.extra.insert("payees".into(), json!([
            {"id":"old","name":"Old name","categoryId":null},
            {"id":"new","name":"New name","categoryId":"food"}
        ]));
        b.extra.insert("payeeRules".into(), json!([{"id":"r","match":"merchant","payee":"Old name"}]));
        let changed = apply(b, &json!({"type":"renamePayee","from":"Old name","to":"New name"})).unwrap();
        assert_eq!(changed.extra["payees"], json!([{"id":"new","name":"New name","categoryId":"food"}]));
        assert_eq!(changed.transactions[0].payee, "New name");
        assert_eq!(changed.transactions[0].splits[0].extra["payee"], "New name");
        assert_eq!(changed.extra["payeeRules"][0]["payee"], "New name");
        assert_eq!(changed.extra["schedules"][0]["transaction"]["payee"], "New name");
        assert_eq!(changed.extra["schedules"][0]["transaction"]["splits"][0]["payee"], "New name");
    }

    #[test]
    fn payee_rule_reordering_changes_ties_and_validates_identity() {
        let mut b = sample();
        b.extra.insert("payeeRules".into(), json!([
            {"id":"a","match":"shop","payee":"First"},
            {"id":"b","match":"SHOP","payee":"Second"}
        ]));
        let changed = apply(b, &json!({"type":"reorderPayeeRule","id":"b","direction":"up"})).unwrap();
        assert_eq!(payee_rule(&changed, "Shop 12").unwrap()["payee"], "Second");
        assert!(apply(changed, &json!({"type":"reorderPayeeRule","id":"missing","direction":"up"})).is_err());
    }
    #[test]
    fn deleting_a_category_moves_assignments_splits_and_defaults_without_losing_metadata() {
        let mut b = sample();
        let mut target = b.categories.iter().find(|c| c.id == "food").unwrap().clone();
        target.id = "groceries".into(); target.name = "Groceries".into(); b.categories.push(target);
        b.transactions.push(purchase());
        b.assignments = vec![Assignment { month: "2026-10".into(), category_id: "food".into(), amount: 100, extra: serde_json::Map::from_iter([("newField".into(), json!(true))]) }];
        b.extra.insert("payees".into(), json!([{"id":"p","name":"Shop","categoryId":"food"}]));
        let changed = apply(b, &json!({"type":"deleteCategory","id":"food","replacement":"groceries"})).unwrap();
        assert_eq!(changed.transactions[0].splits[0].category_id.as_deref(), Some("groceries"));
        assert_eq!(changed.extra["payees"][0]["categoryId"], "groceries");
        assert_eq!(changed.assignments[0].category_id, "groceries");
        assert_eq!(changed.assignments[0].extra["newField"], true);
        assert!(!changed.categories.iter().any(|c| c.id == "food"));
        assert!(apply(changed.clone(), &json!({"type":"deleteCategory","id":"payment","replacement":"__rta"})).is_err());
        let unassigned = apply(changed, &json!({"type":"deleteCategory","id":"groceries","replacement":"__rta"})).unwrap();
        assert!(unassigned.transactions[0].splits[0].category_id.is_none());
        assert_eq!(unassigned.transactions[0].splits[0].extra["unassigned"], true);
        assert!(unassigned.assignments.is_empty());
    }

    #[test]
    fn category_and_group_order_preserve_existing_members_and_merge_group_names() {
        let mut b = sample();
        b.extra.insert("categoryOrder".into(), json!(["payment", "food"]));
        let changed = apply(b, &json!({"type":"organizeCategory","id":"food","group":"Everyday","beforeId":"payment","groupBefore":null})).unwrap();
        assert_eq!(changed.extra["categoryOrder"], json!(["food","payment"]));
        assert_eq!(changed.categories.iter().find(|c| c.id == "food").unwrap().group, "Everyday");
        let renamed = apply(changed, &json!({"type":"renameGroup","from":"Everyday","to":"Living"})).unwrap();
        assert_eq!(renamed.categories.iter().find(|c| c.id == "food").unwrap().group, "Living");
        assert!(renamed.extra["groupOrder"].as_array().unwrap().iter().all(|g| g != "Everyday"));
    }
    #[test]
    fn saved_payee_edits_merge_atomically_and_planning_does_not_invent_income() {
        let mut b = sample(); b.transactions.push(purchase());
        b.extra.insert("payees".into(), json!([{"id":"p","name":"Shop","categoryId":null}]));
        let renamed = apply(b, &json!({"type":"savePayee","record":{"id":"p","name":"Grocer","categoryId":"food","futureField":17}})).unwrap();
        assert_eq!(renamed.transactions[0].payee, "Grocer");
        assert_eq!(renamed.extra["payees"][0]["futureField"], 17);
        let ready = engine::calculate(&renamed, "2026-10").unwrap().ready_to_assign;
        let planned = apply(renamed, &json!({"type":"setBudgetPlanning","month":"2026-10","note":"Holiday plan","expectedIncome":500_000})).unwrap();
        assert_eq!(engine::calculate(&planned, "2026-10").unwrap().ready_to_assign, ready);
        assert_eq!(planned.extra["ynabMonths"][0]["note"], "Holiday plan");
        let assigned = apply(planned, &json!({"type":"assignAmount","month":"2026-10","categoryId":"food","amount":25_000})).unwrap();
        assert_eq!(assigned.extra["moneyMoves"][0]["changes"][0]["amount"], 5_000);
    }
    /// Underfunded as it used to be worked out: the whole budget recalculated per category.
    fn underfunded_reference(mut b: Budget, month: &str, selected: &[&str], limit: bool) -> HashMap<String, i64> {
        let summary = engine::calculate_history(&b, month).unwrap().remove(month).unwrap();
        let mut ids: Vec<String> =
            b.categories.iter().filter(|c| selected.contains(&c.id.as_str())).map(|c| c.id.clone()).collect();
        ids.sort_by_key(|id| {
            let c = b.categories.iter().find(|c| &c.id == id).unwrap();
            let s = summary.categories.iter().find(|s| &s.id == id).unwrap();
            (c.payment_account().is_some(), s.available >= 0, "9999-12-31".to_string())
        });
        for id in ids {
            let current = engine::calculate(&b, month).unwrap();
            let row = current.categories.iter().find(|s| s.id == id).unwrap();
            let mut needed = row.needed.max((-row.available).max(0));
            if limit {
                needed = needed.min(current.ready_to_assign.max(0));
            }
            assign(&mut b, month, &id, row.assigned + needed);
        }
        assigned_in(&b, month)
    }
    /// Card spending beyond what Food holds (covering it moves money into the card's payment
    /// category), cash overspending on Fun, Rent short of its target, and `cash` to start with.
    fn busy(cash: i64) -> Budget {
        let mut b = sample();
        b.accounts[0].opening_balance = cash;
        for (id, target) in [("rent", 30_000), ("fun", 0)] {
            b.categories.push(Category {
                extra: serde_json::Map::new(),
                id: id.into(),
                name: id.into(),
                group: "Living".into(),
                target,
            });
        }
        let mut card = purchase();
        card.amount = -35_000;
        card.splits[0].amount = -35_000;
        b.transactions.push(card);
        let mut fun = purchase();
        fun.id = "fun-tx".into();
        fun.account_id = "cash".into();
        fun.amount = -5_000;
        fun.splits[0].amount = -5_000;
        fun.splits[0].category_id = Some("fun".into());
        b.transactions.push(fun);
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "rent".into(), amount: 25_000 });
        b
    }
    #[test]
    fn underfunded_plan_matches_working_it_out_one_category_at_a_time() {
        let selected = ["food", "rent", "fun", "payment"];
        for cash in [100_000, 30_000, 5_000] {
            for limit in [false, true] {
                for month in ["2026-10", "2026-11"] {
                    let b = busy(cash);
                    let expected = underfunded_reference(b.clone(), month, &selected, limit);
                    let action = json!({"type":"autoAssign","month":month,"mode":"underfunded","categories":selected,"limitToAvailable":limit});
                    let got = apply(b, &action).unwrap();
                    assert_eq!(assigned_in(&got, month), expected, "cash {cash}, limit {limit}, {month}");
                }
            }
        }
    }
    #[test]
    fn the_preview_shows_what_apply_does_for_every_option() {
        let ids: Vec<String> = ["food", "rent", "fun", "payment"].map(String::from).to_vec();
        for limit in [false, true] {
            let b = busy(30_000);
            let before = assigned_in(&b, "2026-10");
            let preview = auto_assign_preview(&b, "2026-10", &ids, limit).unwrap();
            for mode in QUICK_ASSIGN {
                let action = json!({"type":"autoAssign","month":"2026-10","mode":mode,"categories":ids,"limitToAvailable":limit});
                let applied = assigned_in(&apply(b.clone(), &action).unwrap(), "2026-10");
                let changes = preview[mode].as_ref().unwrap();
                for id in &ids {
                    let shown = changes.iter().find(|c| &c.id == id).map(|c| c.after).unwrap_or(*before.get(id).unwrap_or(&0));
                    assert_eq!(shown, *applied.get(id).unwrap_or(&0), "{mode} {id} (limit {limit})");
                }
            }
        }
    }
    #[test]
    fn auto_assign_averages_skip_pre_category_months_and_reset() {
        let mut b = sample();
        b.assignments.push(Assignment { extra: Default::default(),
            month: "2026-09".into(),
            category_id: "food".into(),
            amount: 10000,
        });
        let b=apply(b,&json!({"type":"autoAssign","month":"2026-11","mode":"averageAssigned","categories":["food"]})).unwrap();
        assert_eq!(b.assignments.last().unwrap().amount, 15000);
        let b=apply(b,&json!({"type":"autoAssign","month":"2026-11","mode":"resetAssigned","categories":["food"]})).unwrap();
        assert_eq!(b.assignments.last().unwrap().amount, 0);
    }
    #[test]
    fn averages_count_from_the_first_month_a_category_was_spent_from_as_ynab_does() {
        // Spent from in June, first assigned in September: YNAB averages June to October, as it
        // did for Rent on the stress-test plan (spent from January, assigned from July).
        let mut b = sample();
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "food".into(), amount: 10000 });
        let mut june = crate::domain::tests::purchase();
        june.id = "june".into();
        june.date = "2026-06-10".into();
        b.transactions.push(june);
        let b = apply(b, &json!({"type":"autoAssign","month":"2026-11","mode":"averageAssigned","categories":["food"]})).unwrap();
        assert_eq!(b.assignments.last().unwrap().amount, 6000);
    }
    /// The "UrBudget Stress Test" plan imported from YNAB, in October 2026, cut down to the
    /// categories where YNAB's Auto-Assign and Quick assign differed.
    fn stress_test_october() -> Budget {
        let account = |id: &str, kind, opening_balance| Account {
            extra: serde_json::Map::new(), id: id.into(), name: id.into(), kind, opening_balance, archived: false,
        };
        let category = |id: &str, target, extra: Value| Category {
            extra: extra.as_object().unwrap().clone(), id: id.into(), name: id.into(), group: "Plan".into(), target,
        };
        let mut b = Budget {
            accounts: vec![account("checking", AccountKind::Cash, 2_000_000),
                account("amex", AccountKind::Credit, -11_400), account("visa", AccountKind::Credit, -164_277)],
            categories: vec![
                // 100.00 every Saturday, created on 2026-10-20.
                category("groceries", 10_000, json!({"goal":{"goal_type":"NEED", "goal_cadence":2, "goal_cadence_frequency":1,
                    "goal_day":6, "goal_needs_whole_amount":true, "goal_creation_month":"2026-10-20"}})),
                // A 15,000.00 balance with no date.
                category("emergency", 1_500_000, json!({"goal":{"goal_type":"TB", "goal_creation_month":"2026-10-01"}})),
                category("amex-payment", 0, json!({"paymentAccountId":"amex"})),
                category("visa-payment", 0, json!({"paymentAccountId":"visa"})),
            ],
            ..Budget::default()
        };
        b.extra.insert("calculationAsOfMonth".into(), json!("2026-10"));
        for (month, id, amount) in [("2026-07", "emergency", 300_000), ("2026-08", "emergency", 1_000_000),
            ("2026-08", "visa-payment", 164_277), ("2026-09", "groceries", 45_000), ("2026-09", "amex-payment", 32_935),
            ("2026-10", "groceries", 40_000)] {
            b.assignments.push(Assignment { extra: Default::default(), month: month.into(), category_id: id.into(), amount });
        }
        for (id, date, amount) in [("september-groceries", "2026-09-12", -43_024), ("october-groceries", "2026-10-10", -29_040)] {
            let mut t = purchase();
            (t.id, t.account_id, t.date, t.amount) = (id.into(), "checking".into(), date.into(), amount);
            t.splits[0].amount = amount;
            t.splits[0].category_id = Some("groceries".into());
            b.transactions.push(t);
        }
        // September's 1,642.77 Visa payment.
        let mut payment = purchase();
        (payment.id, payment.account_id, payment.date, payment.amount) = ("visa-payment".into(), "checking".into(), "2026-09-25".into(), -164_277);
        (payment.transfer_account_id, payment.transfer_status) = (Some("visa".into()), Some(Cleared::Cleared));
        payment.splits.clear();
        b.transactions.push(payment);
        b
    }
    #[test]
    fn quick_assign_matches_ynabs_auto_assign_on_the_stress_test_plan() {
        let b = stress_test_october();
        let ids: Vec<String> = b.categories.iter().map(|c| c.id.clone()).collect();
        let s = engine::calculate(&b, "2026-10").unwrap();
        let available: Vec<i64> = s.categories.iter().map(|c| c.available).collect();
        assert_eq!(available, [12_936, 1_300_000, 32_935, 0]);
        for limit in [false, true] {
            let preview = auto_assign_preview(&b, "2026-10", &ids, limit).unwrap();
            let changes = |mode: &str| preview[mode].as_ref().unwrap().iter().map(|c| (c.id.as_str(), c.before, c.after)).collect::<Vec<_>>();
            // Groceries' two October Saturdays are funded, and the emergency fund's 2,000.00 is
            // needed only eventually: Underfunded adds nothing to either.
            assert!(changes("underfunded").is_empty(), "limit {limit}: {:?}", changes("underfunded"));
            // The Visa payment is card activity, not spending: Spent last month and Average
            // spent leave the card payment categories as they are.
            assert_eq!(changes("spentLastMonth"), [("groceries", 40_000, 43_024)]);
            assert_eq!(changes("averageSpent"), [("groceries", 40_000, 43_024)]);
            // YNAB returns 344.71: Groceries' 129.36 beyond its 200.00 October target, and the
            // 215.35 the Amex payment category holds beyond the 114.00 the card owes.
            let reduce = changes("reduceOverfunding");
            assert_eq!(reduce, [("groceries", 40_000, 27_064), ("amex-payment", 0, -21_535)]);
            assert_eq!(reduce.iter().map(|(_, before, after)| before - after).sum::<i64>(), 34_471);
        }
        let action = json!({"type":"autoAssign","month":"2026-10","mode":"reduceOverfunding","categories":ids});
        let reduced = apply(b, &action).unwrap();
        let s = engine::calculate(&reduced, "2026-10").unwrap();
        assert_eq!((s.categories[0].available, s.categories[2].available), (0, 11_400));
    }
    #[test]
    fn first_matching_payee_rule_renames_and_fills_only_an_empty_category() {
        let mut b = sample();
        b.extra.insert(
            "payeeRules".into(),
            json!([
                {"id":"a","match":"coffee","payee":"","categoryId":"food"},
                {"id":"b","match":"COFFEE","payee":"Never reached","categoryId":null},
                {"id":"c","match":"shop","payee":"Corner Shop","categoryId":"payment"}
            ]),
        );
        let mut t = purchase();
        t.payee = "Coffee House 12".into();
        t.splits[0].category_id = None;
        apply_payee_rules(&b, &mut t);
        // An empty "rename to" keeps the payee; the category fills in.
        assert_eq!(t.payee, "Coffee House 12");
        assert_eq!(t.splits[0].category_id.as_deref(), Some("food"));
        let mut t = purchase();
        apply_payee_rules(&b, &mut t);
        assert_eq!(t.payee, "Corner Shop");
        // An existing category is never replaced.
        assert_eq!(t.splits[0].category_id.as_deref(), Some("food"));
    }
    #[test]
    fn bank_text_is_read_without_case_or_punctuation() {
        assert_eq!(normalize_payee("SQ *BLUE-BOTTLE  #12 "), "sq blue bottle 12");
        assert_eq!(normalize_payee("  Café ÑANDÚ "), "café ñandú");
        assert_eq!(normalize_payee("بنده — الرياض"), "بنده الرياض");
        assert_eq!(normalize_payee("*** "), "");
    }
    fn rules(list: Value) -> Budget {
        let mut b = sample();
        b.extra.insert("payeeRules".into(), list);
        b
    }
    #[test]
    fn the_longest_rule_wins_and_matches_start_at_a_word() {
        let b = rules(json!([
            {"id":"a","match":"UBER","payee":"Uber","categoryId":null},
            {"id":"b","match":"uber eats","payee":"Uber Eats","categoryId":null},
            {"id":"c","match":"Amzn Mktp","payee":"Amazon","categoryId":null},
            {"id":"d","match":"   ","payee":"Never","categoryId":null}
        ]));
        let rule = |text: &str| payee_rule(&b, text).map(|r| r["id"].as_str().unwrap().to_string());
        assert_eq!(rule("UBER *EATS PENDING").as_deref(), Some("b"));
        assert_eq!(rule("UBER   *TRIP HELP.UBER.COM").as_deref(), Some("a"));
        assert_eq!(rule("AMZN Mktp US*2K4AB").as_deref(), Some("c"));
        // Inside a word is not a match: "kuber", "mamzn".
        assert_eq!(rule("KUBER CAFE"), None);
        assert_eq!(rule("MAMZN MKTP"), None);
        // A word may continue past the rule's text, as "uber" in "ubereats".
        assert_eq!(rule("UBEREATS").as_deref(), Some("a"));
        assert_eq!(rule(""), None);
    }
    #[test]
    fn rules_read_the_bank_text_and_fill_the_payee_default() {
        let mut b = rules(json!([
            {"id":"a","match":"amzn","payee":"Amazon","categoryId":null},
            {"id":"b","match":"amzn prime","payee":"Amazon","categoryId":"payment"}
        ]));
        b.extra.insert("payees".into(), json!([{"id":"p","name":"Amazon","categoryId":"food"}]));
        let mut t = purchase();
        t.payee = "Something I typed".into();
        t.extra.insert("importedPayee".into(), json!("AMZN Mktp US*2K4"));
        t.splits[0].category_id = None;
        apply_payee_rules(&b, &mut t);
        assert_eq!(t.payee, "Amazon");
        assert_eq!(t.splits[0].category_id.as_deref(), Some("food"));
        // The rule's own category beats the payee's default.
        let mut t = purchase();
        t.extra.insert("importedPayee".into(), json!("AMZN PRIME*1A2B"));
        t.splits[0].category_id = None;
        apply_payee_rules(&b, &mut t);
        assert_eq!(t.splits[0].category_id.as_deref(), Some("payment"));
        // Typed by hand with no rule: the form already offered the default, and an inflow
        // left without a category goes to Ready to Assign on purpose.
        let mut plain = sample();
        plain.extra.insert("payees".into(), b.extra["payees"].clone());
        let mut t = purchase();
        t.payee = "Amazon".into();
        t.amount = 5_000;
        t.splits[0].amount = 5_000;
        t.splits[0].category_id = None;
        apply_payee_rules(&plain, &mut t);
        assert_eq!(t.splits[0].category_id, None);
        // A rule that names the payee brings its default along, even when typed by hand.
        let mut typed = t.clone();
        typed.payee = "amzn".into();
        apply_payee_rules(&b, &mut typed);
        assert_eq!((typed.payee.as_str(), typed.splits[0].category_id.as_deref()), ("Amazon", Some("food")));
    }
    #[test]
    fn tracking_accounts_and_transfers_get_no_category() {
        let mut b = rules(json!([{"id":"a","match":"shop","payee":"Corner Shop","categoryId":"food"}]));
        b.accounts[0].kind = AccountKind::Tracking;
        let mut t = purchase();
        t.account_id = "cash".into();
        t.splits[0].category_id = None;
        apply_payee_rules(&b, &mut t);
        assert_eq!((t.payee.as_str(), t.splits[0].category_id.as_deref()), ("Corner Shop", None));
        let mut t = purchase();
        t.transfer_account_id = Some("cash".into());
        apply_payee_rules(&b, &mut t);
        assert_eq!(t.payee, "Shop");
    }
    #[test]
    fn reapplying_rules_renames_past_transactions_and_fills_placeholders() {
        let mut b = rules(json!([
            {"id":"a","match":"sq blue bottle","payee":"Blue Bottle","categoryId":"food"},
            {"id":"b","match":"shop","payee":"Corner Shop","categoryId":null}
        ]));
        for (id, name) in [("local-uncategorized", "Uncategorized"), ("treats", "Treats")] {
            b.categories.push(Category {
                extra: Default::default(),
                id: id.into(),
                name: name.into(),
                group: "Living".into(),
                target: 0,
            });
        }
        let imported = |id: &str, text: &str, amount: i64, category: Option<&str>| {
            let mut t = purchase();
            t.id = id.into();
            t.account_id = "cash".into();
            t.payee = text.into();
            t.amount = amount;
            t.splits[0].amount = amount;
            t.splits[0].category_id = category.map(String::from);
            t.extra.insert("importedPayee".into(), json!(text));
            t
        };
        b.transactions = vec![
            imported("1", "SQ *BLUE BOTTLE 0042", -650, Some("local-uncategorized")),
            // An inflow with no category is in Ready to Assign, which may be on purpose.
            imported("2", "SQ *BLUE BOTTLE REFUND", 650, None),
            imported("3", "SQ *BLUE BOTTLE 7", -100, Some("treats")),
            imported("4", "SHOP 12", -100, Some("treats")),
            imported("5", "SOMEWHERE ELSE", -100, Some("treats")),
        ];
        let mut only_b = b.clone();
        assert_eq!(reapply_payee_rules(&mut only_b, Some(&HashSet::from(["b"]))), 1);
        assert_eq!(only_b.transactions[0].payee, "SQ *BLUE BOTTLE 0042");
        assert_eq!(only_b.transactions[3].payee, "Corner Shop");
        assert_eq!(reapply_payee_rules(&mut b.clone(), None), 4);
        let b = apply(b, &json!({"type":"reapplyPayeeRules"})).unwrap();
        let got: Vec<_> = b.transactions.iter().map(|t| (t.payee.as_str(), t.splits[0].category_id.as_deref())).collect();
        assert_eq!(
            got,
            [
                ("Blue Bottle", Some("food")),
                ("Blue Bottle", None),
                // A category already chosen stays.
                ("Blue Bottle", Some("treats")),
                ("Corner Shop", Some("treats")),
                ("SOMEWHERE ELSE", Some("treats")),
            ]
        );
        let t = &b.transactions;
        // The bank's text is kept, so running them again changes nothing.
        assert_eq!(t[0].extra["importedPayee"], "SQ *BLUE BOTTLE 0042");
        assert_eq!(reapply_payee_rules(&mut b.clone(), None), 0);
    }
    #[test]
    fn money_moves_conserve_total_assignments_and_reject_overdraw() {
        let b=apply(sample(),&json!({"type":"moveMoney","month":"2026-10","from":"food","to":"payment","amount":1000})).unwrap();
        assert_eq!(b.assignments.iter().map(|a| a.amount).sum::<i64>(), 20000);
        // The move says what it did to each category, for the categories' moves.
        let row = b.extra["moneyMoves"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(row["kind"], "move");
        assert_eq!(
            row["changes"],
            json!([{"categoryId":"food","amount":-1000},{"categoryId":"payment","amount":1000}])
        );
        assert!(apply(b,&json!({"type":"moveMoney","month":"2026-10","from":"food","to":"payment","amount":999999})).is_err());
    }
    #[test]
    fn quick_assign_logs_each_categorys_change() {
        let b = apply(
            sample(),
            &json!({"type":"autoAssign","month":"2026-10","mode":"resetAssigned","categories":["food","payment"]}),
        )
        .unwrap();
        let row = b.extra["moneyMoves"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(row["kind"], "quickAssign");
        assert_eq!(row["detail"], "reset assigned amounts");
        // Only the categories it changed are listed.
        assert_eq!(row["changes"], json!([{"categoryId":"food","amount":-20000}]));
    }
    #[test]
    fn underfunded_leaves_a_balance_with_no_date_to_be_saved_eventually() {
        let mut b = sample();
        let food = b.categories.iter_mut().find(|c| c.id == "food").unwrap();
        food.target = 50_000;
        food.extra.insert("goal".into(), json!({"goal_type":"TB"}));
        let row = |b: &Budget| {
            let s = engine::calculate(b, "2026-10").unwrap();
            s.categories.into_iter().find(|c| c.id == "food").unwrap()
        };
        // 200.00 of a 500.00 balance is in place: 300.00 more is needed eventually, nothing this
        // month, so YNAB's Auto-Assign leaves it out even when it's the only category selected.
        let before = row(&b);
        assert_eq!((before.needed, before.target_funding_required, before.target_funding_funded), (0, 50_000, 20_000));
        let b = apply(
            b,
            &json!({"type":"autoAssign","month":"2026-10","mode":"underfunded","categories":["food"]}),
        )
        .unwrap();
        assert_eq!(row(&b).assigned, 20_000);
    }
    #[test]
    fn duplicate_does_not_keep_import_identity_and_status_locks() {
        let mut b = sample();
        let mut t = purchase();
        t.status = Cleared::Reconciled;
        t.extra.insert("source".into(), json!({"id":"remote"}));
        b.transactions.push(t);
        let b = apply(
            b,
            &json!({"type":"bulk","ids":["tx"],"operation":"duplicate"}),
        )
        .unwrap();
        assert_eq!(b.transactions.len(), 2);
        assert_eq!(b.transactions[1].status, Cleared::Uncleared);
        assert!(!b.transactions[1].extra.contains_key("source"));
        let b = apply(
            b,
            &json!({"type":"bulk","ids":["tx"],"operation":"status","value":"uncleared"}),
        )
        .unwrap();
        assert_eq!(b.transactions[0].status, Cleared::Reconciled);
    }
    #[test]
    fn underfunded_funds_this_months_bill_before_next_years_goal() {
        let mut b = sample(); b.assignments.clear(); b.accounts[0].opening_balance = 10000;
        let rent = &mut b.categories[0]; rent.target=10000;
        rent.extra.insert("goal".into(),json!({"goal_type":"NEED","goal_cadence":1,"goal_day":1}));
        let mut trip=rent.clone(); trip.id="trip".into(); trip.target=120000;
        trip.extra.insert("goal".into(),json!({"goal_type":"NEED","goal_cadence":13,"goal_target_date":"2027-09-30","goal_creation_month":"2026-10-01"}));
        b.categories.push(trip);
        let result=apply(b,&json!({"type":"autoAssign","month":"2026-10","mode":"underfunded","categories":["food","trip"],"limitToAvailable":true})).unwrap();
        let amounts=assigned_in(&result,"2026-10");
        assert_eq!(amounts["food"],10000); assert_eq!(amounts["trip"],0);
    }
    fn planning_budget(cash: i64) -> Budget {
        let mut b = sample();
        b.assignments.clear();
        b.accounts[0].opening_balance = cash;
        b.accounts[1].opening_balance = 0;
        b.categories[0].target = 0;
        b.extra.insert("calculationAsOfMonth".into(), json!("2026-10"));
        b
    }
    fn planning_category(id: &str, target: i64, goal: Value) -> Category {
        Category { id: id.into(), name: id.into(), group: "Living".into(), target,
            extra: serde_json::from_value(json!({"goal":goal})).unwrap() }
    }
    fn rollover_refill_budget() -> Budget {
        let mut b = planning_budget(100000);
        b.categories[0] = planning_category("food", 10000, json!({
            "goal_type":"NEED", "goal_cadence":1, "goal_needs_whole_amount":false,
            "goal_creation_month":"2026-10-01"
        }));
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-10".into(), category_id: "food".into(), amount: 5000 });
        b
    }
    #[test]
    fn quick_assign_rejects_a_confirmed_preview_after_calendar_rollover() {
        let mut b = rollover_refill_budget();
        let preview = auto_assign_preview(&b, "2026-11", &["food".into()], true).unwrap();
        assert_eq!(preview["underfunded"].as_ref().unwrap()[0].after, 10000);
        b.extra.insert("calculationAsOfMonth".into(), json!("2026-11"));
        let action = json!({"type":"autoAssign","mode":"underfunded","month":"2026-11",
            "categories":["food"],"limitToAvailable":true,"expectedAsOfMonth":"2026-10"});
        assert!(apply(b.clone(), &action).unwrap_err().contains("calendar month changed"));
        // The new month's refill would assign less than the option just confirmed.
        let preview = auto_assign_preview(&b, "2026-11", &["food".into()], true).unwrap();
        assert_eq!(preview["underfunded"].as_ref().unwrap()[0].after, 5000);
    }
    #[test]
    fn quick_assign_accepts_matching_or_legacy_context_without_persisting_the_guard() {
        let mut b = rollover_refill_budget();
        b.extra.insert("calculationAsOfMonth".into(), json!("2026-11"));
        let mut action = json!({"type":"autoAssign","mode":"underfunded","month":"2026-11",
            "categories":["food"],"limitToAvailable":true,"expectedAsOfMonth":"2026-11"});
        let result = apply(b.clone(), &action).unwrap();
        assert_eq!(assigned_in(&result, "2026-11")["food"], 5000);
        assert!(!result.extra.contains_key("expectedAsOfMonth"));
        assert_eq!(result.extra["calculationAsOfMonth"], json!("2026-11"));
        action.as_object_mut().unwrap().remove("expectedAsOfMonth");
        assert_eq!(assigned_in(&apply(b, &action).unwrap(), "2026-11")["food"], 5000);
    }
    #[test]
    fn quick_assign_restores_original_reference_metadata_after_freezing_the_confirmation() {
        for original in [None, Some(Value::Null), Some(json!("2026-10"))] {
            let mut b = planning_budget(100000);
            b.extra.remove("calculationAsOfMonth");
            if let Some(value) = &original { b.extra.insert("calculationAsOfMonth".into(), value.clone()); }
            let expected = engine::calculation_month(&b);
            let result = apply(b, &json!({"type":"autoAssign","mode":"resetAssigned","month":"2026-10",
                "categories":["food"],"expectedAsOfMonth":expected})).unwrap();
            assert_eq!(result.extra.get("calculationAsOfMonth"), original.as_ref());
            assert!(!result.extra.contains_key("expectedAsOfMonth"));
        }
    }
    fn scheduled_spend(id: &str, category: &str, date: &str, amount: i64) -> crate::schedules::Schedule {
        let mut t = purchase();
        t.id = id.into(); t.date = date.into(); t.account_id = "cash".into();
        t.amount = -amount; t.splits[0].amount = -amount; t.splits[0].category_id = Some(category.into());
        crate::schedules::Schedule { extra: Default::default(), id: id.into(), next_date: Some(date.into()), anchor_date: date.into(),
            frequency: "never".into(), paused: false, transaction: t }
    }
    fn planned_underfunded(b: Budget, ids: &[&str], limit: bool) -> Budget {
        let ids: Vec<String> = ids.iter().map(|id| (*id).into()).collect();
        let preview = auto_assign_preview(&b, "2026-10", &ids, limit).unwrap();
        let before = assigned_in(&b, "2026-10");
        let result = apply(b, &json!({"type":"autoAssign","mode":"underfunded","month":"2026-10",
            "categories":ids,"limitToAvailable":limit})).unwrap();
        let after = assigned_in(&result, "2026-10");
        let changes = preview["underfunded"].as_ref().unwrap();
        let unique: HashSet<_> = changes.iter().map(|c| &c.id).collect();
        assert_eq!(unique.len(), changes.len(), "one final preview change per category");
        for id in &ids {
            let shown = changes.iter().find(|c| &c.id == id).map(|c| c.after).unwrap_or(*before.get(id).unwrap_or(&0));
            assert_eq!(shown, *after.get(id).unwrap_or(&0), "preview for {id}");
        }
        result
    }
    #[test]
    fn underfunded_skips_undated_saving_but_covers_its_bills_and_overspending() {
        let mut b = planning_budget(100000);
        b.categories[0] = planning_category("food", 100000, json!({"goal_type":"TB"}));
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "food".into(), amount: 20000 });
        let result = planned_underfunded(b.clone(), &["food"], true);
        assert_eq!(assigned_in(&result, "2026-10")["food"], 0);
        b.extra.insert("schedules".into(), json!([scheduled_spend("bill", "food", "2026-10-22", 50000)]));
        let result = planned_underfunded(b.clone(), &["food"], true);
        assert_eq!(assigned_in(&result, "2026-10")["food"], 30000);
        assert_eq!(engine::calculate(&result, "2026-10").unwrap().ready_to_assign, 50000);
        // Selected on its own, it still gets only its bill.
        let selected = planned_underfunded(b, &["food"], false);
        assert_eq!(assigned_in(&selected, "2026-10")["food"], 30000);
        let mut b = planning_budget(100000);
        b.categories[0] = planning_category("food", 100000, json!({"goal_type":"TB"}));
        let mut spent = purchase(); spent.account_id = "cash".into(); spent.amount = -20000; spent.splits[0].amount = -20000;
        b.transactions.push(spent);
        let result = planned_underfunded(b, &["food"], true);
        assert_eq!(assigned_in(&result, "2026-10")["food"], 20000);
    }
    #[test]
    fn global_underfunded_interleaves_weekly_installments_with_dated_monthly_targets() {
        for (cash, food, subscription) in [(15000, 10000, 5000), (25000, 15000, 10000), (100000, 40000, 10000)] {
            let mut b = planning_budget(cash);
            b.categories[0] = planning_category("food", 10000, json!({"goal_type":"NEED","goal_cadence":2,"goal_day":0}));
            b.categories.push(planning_category("subscription", 10000, json!({"goal_type":"NEED","goal_cadence":1,"goal_day":5})));
            let result = planned_underfunded(b, &["food", "subscription"], true);
            let assigned = assigned_in(&result, "2026-10");
            assert_eq!((assigned["food"], assigned["subscription"]), (food, subscription));
        }
    }
    #[test]
    fn underfunded_spreads_a_monthly_target_due_next_month_and_funds_it_after_this_months() {
        for (cash, margin, rent) in [(50000, 40000, 10000), (300000, 100000, 10000)] {
            let mut b = planning_budget(cash);
            // Created this month, first due next month: half of it is needed now.
            b.categories[0] = planning_category("margin", 200000, json!({"goal_type":"NEED", "goal_cadence":1,
                "goal_cadence_frequency":1, "goal_creation_month":"2026-10-01", "goal_target_month":"2026-11-01"}));
            b.categories.push(planning_category("rent", 10000, json!({"goal_type":"NEED", "goal_cadence":1})));
            let result = planned_underfunded(b, &["margin", "rent"], true);
            let assigned = assigned_in(&result, "2026-10");
            assert_eq!((assigned["margin"], assigned["rent"]), (margin, rent));
        }
    }
    #[test]
    fn weekly_priority_consumes_existing_assignment_and_refill_carry_before_new_money() {
        let mut b = planning_budget(25000);
        b.categories[0] = planning_category("food", 10000, json!({"goal_type":"NEED","goal_cadence":2,
            "goal_day":0,"goal_needs_whole_amount":false}));
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "food".into(), amount: 10000 });
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-10".into(), category_id: "food".into(), amount: 5000 });
        b.categories.push(planning_category("subscription", 10000, json!({"goal_type":"NEED","goal_cadence":1,"goal_day":5})));
        let result = planned_underfunded(b, &["food", "subscription"], true);
        let assigned = assigned_in(&result, "2026-10");
        assert_eq!((assigned["food"], assigned["subscription"]), (5000, 10000));
    }
    #[test]
    fn scheduled_and_target_checkpoints_fund_the_larger_obligation_without_double_counting() {
        for (cash, food) in [(20000, 10000), (40000, 30000)] {
            let mut b = planning_budget(cash);
            b.categories[0] = planning_category("food", 10000, json!({"goal_type":"NEED","goal_cadence":1,"goal_day":5}));
            b.categories.push(planning_category("bill", 10000, json!({"goal_type":"NEED","goal_cadence":1,"goal_day":10})));
            b.extra.insert("schedules".into(), json!([scheduled_spend("scheduled-food", "food", "2026-10-20", 30000)]));
            let result = planned_underfunded(b, &["food", "bill"], true);
            let assigned = assigned_in(&result, "2026-10");
            assert_eq!((assigned["food"], assigned["bill"]), (food, 10000));
        }
    }
    #[test]
    fn monthly_schedule_total_uses_one_category_deadline_before_a_later_bill() {
        let mut b = planning_budget(15000);
        b.categories.push(planning_category("bill", 10000, json!({"goal_type":"NEED","goal_cadence":1,"goal_day":10})));
        // The local category deadline is its earliest occurrence. Both scheduled
        // amounts are funded together, not interleaved as individual transactions.
        b.extra.insert("schedules".into(), json!([scheduled_spend("late", "food", "2026-10-25", 10000),
            scheduled_spend("early", "food", "2026-10-05", 10000)]));
        let result = planned_underfunded(b, &["food", "bill"], true);
        let assigned = assigned_in(&result, "2026-10");
        assert_eq!((assigned["food"], assigned["bill"]), (15000, 0));
    }
    #[test]
    fn overspent_weekly_category_gets_its_whole_target_before_other_deadlines() {
        let mut b = planning_budget(20000);
        b.categories[0] = planning_category("food", 10000, json!({"goal_type":"NEED","goal_cadence":2,"goal_day":0}));
        b.categories.push(planning_category("bill", 10000, json!({"goal_type":"NEED","goal_cadence":1,"goal_day":1})));
        let mut spent = purchase(); spent.account_id = "cash".into(); spent.amount = -5000; spent.splits[0].amount = -5000;
        b.transactions.push(spent);
        let result = planned_underfunded(b, &["food", "bill"], true);
        let assigned = assigned_in(&result, "2026-10");
        assert_eq!((assigned["food"], assigned["bill"]), (20000, 0));
    }
    #[test]
    fn payment_schedules_precede_payment_targets_and_unplanned_debt() {
        let mut b = planning_budget(25000);
        b.accounts[1].opening_balance = -50000;
        b.categories[1].target = 20000;
        b.categories[1].extra.insert("goal".into(), json!({"goal_type":"MF"}));
        for (account_id, category_id, target) in [("debt-card", "debt", 0), ("scheduled-card", "scheduled-payment", 5000)] {
            let mut account = b.accounts[1].clone(); account.id = account_id.into(); b.accounts.push(account);
            let mut category = b.categories[1].clone(); category.id = category_id.into(); category.target = target;
            category.extra.insert("paymentAccountId".into(), json!(account_id));
            if target == 0 { category.extra.remove("goal"); }
            b.categories.push(category);
        }
        let mut scheduled = scheduled_spend("payment-schedule", "food", "2026-10-25", 10000);
        scheduled.transaction.transfer_account_id = Some("scheduled-card".into());
        scheduled.transaction.transfer_status = Some(Cleared::Uncleared);
        scheduled.transaction.splits.clear();
        b.extra.insert("schedules".into(), json!([scheduled]));
        let result = planned_underfunded(b, &["payment", "debt", "scheduled-payment"], true);
        let assigned = assigned_in(&result, "2026-10");
        assert_eq!((assigned["scheduled-payment"], assigned["payment"], assigned["debt"]), (10000, 15000, 0));
    }
    #[test]
    fn global_underfunded_preserves_intermediate_future_month_assignments() {
        let mut b = planning_budget(25000);
        b.categories[0].target = 10000;
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-11".into(), category_id: "food".into(), amount: 20000 });
        b.assignments.push(Assignment { extra: Default::default(), month: "2026-12".into(), category_id: "food".into(), amount: -15000 });
        let result = planned_underfunded(b, &["food"], true);
        assert_eq!(assigned_in(&result, "2026-10")["food"], 5000);
    }
    #[test]
    fn duplicated_transactions_drop_imported_match_metadata_on_both_sides() {
        let mut b = sample();
        let mut t = purchase();
        t.extra.insert("bankImportIds".into(), json!(["old-source", "old-destination"]));
        t.extra.insert("bankImportMatches".into(), json!({"card":"old-source"}));
        t.splits[0].category_id = None;
        t.splits[0].extra.insert("transferAccountId".into(), json!("cash"));
        t.splits[0].extra.insert("bankImportIds".into(), json!(["old-destination"]));
        b.transactions.push(t);
        let b = apply(b, &json!({"type":"bulk","ids":["tx"],"operation":"duplicate"})).unwrap();
        let copy = &b.transactions[1];
        assert!(!copy.extra.contains_key("bankImportIds") && !copy.extra.contains_key("bankImportMatches"));
        assert!(!copy.splits[0].extra.contains_key("bankImportIds"));
        let incoming = |amount| crate::bank_import::BankTransaction {
            fit: "new-payment".into(), date: "2026-10-15".into(), payee: "Transfer".into(), memo: String::new(), amount,
        };
        let source = crate::bank_import::review(&b, "card", vec![incoming(-5000)]).unwrap();
        assert!(source[0].candidates.contains(&copy.id));
        assert_eq!(source[0].previously_matched_candidates, ["tx"]);
        let destination = crate::bank_import::review(&b, "cash", vec![incoming(5000)]).unwrap();
        assert_eq!(destination[0].candidate_details.len(), 2);
        let copied_leg = destination[0].candidate_details.iter().find(|c| c.transaction_id == copy.id).unwrap();
        assert!(!destination[0].previously_matched_candidates.contains(&copied_leg.id));
        assert_eq!(destination[0].previously_matched_candidates.len(), 1);
    }
}
