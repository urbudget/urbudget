//! Entity-level changes between two budgets.
//!
//! The UI and the store exchange patches instead of complete budgets: an edit
//! sends only the records it touched, and a save writes only the rows that
//! differ from the database. `diff` and `apply` are inverses:
//! `apply(before, diff(before, after)) == after`.
use crate::domain::{Account, Assignment, Budget, Category, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub trait Keyed {
    fn key(&self) -> &str;
}
impl Keyed for Account {
    fn key(&self) -> &str {
        &self.id
    }
}
impl Keyed for Category {
    fn key(&self) -> &str {
        &self.id
    }
}
impl Keyed for Transaction {
    fn key(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "T: Deserialize<'de>"))]
pub struct Collection<T> {
    #[serde(default)]
    pub upsert: Vec<T>,
    #[serde(default)]
    pub delete: Vec<String>,
    /// The complete key order, present only when records moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentKey {
    pub month: String,
    pub category_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignments {
    #[serde(default)]
    pub upsert: Vec<Assignment>,
    #[serde(default)]
    pub delete: Vec<AssignmentKey>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extras {
    #[serde(default)]
    pub set: Map<String, Value>,
    #[serde(default)]
    pub delete: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accounts: Option<Collection<Account>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub categories: Option<Collection<Category>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Collection<Transaction>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignments: Option<Assignments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<Extras>,
}

fn diff_collection<T: Keyed + PartialEq + Clone>(before: &[T], after: &[T]) -> Option<Collection<T>> {
    let previous: HashMap<&str, &T> = before.iter().map(|x| (x.key(), x)).collect();
    let present: HashSet<&str> = after.iter().map(Keyed::key).collect();
    let upsert: Vec<T> = after
        .iter()
        .filter(|x| previous.get(x.key()).is_none_or(|old| *old != *x))
        .cloned()
        .collect();
    let delete: Vec<String> = before
        .iter()
        .filter(|x| !present.contains(x.key()))
        .map(|x| x.key().to_owned())
        .collect();
    // Without an explicit order, apply keeps survivors in place and appends new records.
    let implied = before
        .iter()
        .map(Keyed::key)
        .filter(|k| present.contains(k))
        .chain(after.iter().map(Keyed::key).filter(|k| !previous.contains_key(k)));
    let order = (!implied.eq(after.iter().map(Keyed::key)))
        .then(|| after.iter().map(|x| x.key().to_owned()).collect());
    (!upsert.is_empty() || !delete.is_empty() || order.is_some()).then_some(Collection {
        upsert,
        delete,
        order,
    })
}

fn apply_collection<T: Keyed>(items: &mut Vec<T>, change: Collection<T>) {
    if !change.delete.is_empty() {
        let gone: HashSet<&str> = change.delete.iter().map(String::as_str).collect();
        items.retain(|x| !gone.contains(x.key()));
    }
    let mut index: HashMap<String, usize> = items
        .iter()
        .enumerate()
        .map(|(i, x)| (x.key().to_owned(), i))
        .collect();
    for item in change.upsert {
        match index.get(item.key()) {
            Some(&i) => items[i] = item,
            None => {
                index.insert(item.key().to_owned(), items.len());
                items.push(item);
            }
        }
    }
    if let Some(order) = change.order {
        let rank: HashMap<&str, usize> = order
            .iter()
            .enumerate()
            .map(|(i, k)| (k.as_str(), i))
            .collect();
        // Stable: records missing from the order keep their relative position at the end.
        items.sort_by_key(|x| rank.get(x.key()).copied().unwrap_or(usize::MAX));
    }
}

fn diff_assignments(before: &[Assignment], after: &[Assignment]) -> Option<Assignments> {
    let key = |a: &Assignment| (a.month.clone(), a.category_id.clone());
    let previous: HashMap<(String, String), &Assignment> =
        before.iter().map(|a| (key(a), a)).collect();
    let present: HashSet<(String, String)> = after.iter().map(key).collect();
    let upsert: Vec<Assignment> = after
        .iter()
        .filter(|a| previous.get(&key(a)).is_none_or(|old| *old != *a))
        .cloned()
        .collect();
    let delete: Vec<AssignmentKey> = before
        .iter()
        .filter(|a| !present.contains(&key(a)))
        .map(|a| AssignmentKey {
            month: a.month.clone(),
            category_id: a.category_id.clone(),
        })
        .collect();
    (!upsert.is_empty() || !delete.is_empty()).then_some(Assignments { upsert, delete })
}

fn apply_assignments(items: &mut Vec<Assignment>, change: Assignments) {
    if !change.delete.is_empty() {
        let gone: HashSet<(&str, &str)> = change
            .delete
            .iter()
            .map(|k| (k.month.as_str(), k.category_id.as_str()))
            .collect();
        items.retain(|a| !gone.contains(&(a.month.as_str(), a.category_id.as_str())));
    }
    let mut index: HashMap<(String, String), usize> = items
        .iter()
        .enumerate()
        .map(|(i, a)| ((a.month.clone(), a.category_id.clone()), i))
        .collect();
    for a in change.upsert {
        let key = (a.month.clone(), a.category_id.clone());
        match index.get(&key) {
            Some(&i) => items[i] = a,
            None => {
                index.insert(key, items.len());
                items.push(a);
            }
        }
    }
}

fn diff_extras(before: &Map<String, Value>, after: &Map<String, Value>) -> Option<Extras> {
    let set: Map<String, Value> = after
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let delete: Vec<String> = before
        .keys()
        .filter(|k| !after.contains_key(*k))
        .cloned()
        .collect();
    (!set.is_empty() || !delete.is_empty()).then_some(Extras { set, delete })
}

impl Patch {
    pub fn diff(before: &Budget, after: &Budget) -> Self {
        let changed = |a: &str, b: &str| (a != b).then(|| b.to_owned());
        Self {
            schema_version: (before.schema_version != after.schema_version)
                .then_some(after.schema_version),
            name: changed(&before.name, &after.name),
            currency: changed(&before.currency, &after.currency),
            accounts: diff_collection(&before.accounts, &after.accounts),
            categories: diff_collection(&before.categories, &after.categories),
            transactions: diff_collection(&before.transactions, &after.transactions),
            assignments: diff_assignments(&before.assignments, &after.assignments),
            extras: diff_extras(&before.extra, &after.extra),
        }
    }
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    pub fn apply(self, budget: &mut Budget) {
        if let Some(v) = self.schema_version {
            budget.schema_version = v;
        }
        if let Some(v) = self.name {
            budget.name = v;
        }
        if let Some(v) = self.currency {
            budget.currency = v;
        }
        if let Some(c) = self.accounts {
            apply_collection(&mut budget.accounts, c);
        }
        if let Some(c) = self.categories {
            apply_collection(&mut budget.categories, c);
        }
        if let Some(c) = self.transactions {
            apply_collection(&mut budget.transactions, c);
        }
        if let Some(c) = self.assignments {
            apply_assignments(&mut budget.assignments, c);
        }
        if let Some(e) = self.extras {
            for k in e.delete {
                budget.extra.remove(&k);
            }
            budget.extra.extend(e.set);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tests::{purchase, sample};

    fn roundtrip(before: &Budget, after: &Budget) -> Patch {
        let patch = Patch::diff(before, after);
        let mut applied = before.clone();
        // Through JSON, exactly as the UI sends it.
        serde_json::from_value::<Patch>(serde_json::to_value(&patch).unwrap())
            .unwrap()
            .apply(&mut applied);
        assert_eq!(&applied, after);
        patch
    }

    #[test]
    fn identical_budgets_produce_an_empty_patch() {
        let b = sample();
        assert!(Patch::diff(&b, &b).is_empty());
    }

    #[test]
    fn edits_send_only_changed_records() {
        let mut before = sample();
        for i in 0..50 {
            let mut t = purchase();
            t.id = format!("t{i}");
            before.transactions.push(t);
        }
        let mut after = before.clone();
        after.transactions[10].memo = "Edited".into();
        after.transactions.retain(|t| t.id != "t20");
        let mut added = purchase();
        added.id = "new".into();
        after.transactions.push(added);
        after.name = "Renamed".into();
        let patch = roundtrip(&before, &after);
        let t = patch.transactions.unwrap();
        assert_eq!(t.upsert.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["t10", "new"]);
        assert_eq!(t.delete, ["t20"]);
        assert!(t.order.is_none());
        assert!(patch.accounts.is_none() && patch.categories.is_none());
    }

    #[test]
    fn reordering_carries_the_full_order() {
        let before = sample();
        let mut after = before.clone();
        after.categories.reverse();
        let patch = roundtrip(&before, &after);
        assert!(patch.categories.unwrap().order.is_some());
    }

    #[test]
    fn assignments_and_extras_round_trip() {
        let mut before = sample();
        before.extra.insert("ynabMonths".into(), serde_json::json!([{"month": "2026-01-01", "note": ""}]));
        before.extra.insert("obsolete".into(), serde_json::json!(true));
        let mut after = before.clone();
        after.assignments.push(Assignment { extra: Default::default(),
            month: "2026-02".into(),
            category_id: after.categories[0].id.clone(),
            amount: 500,
        });
        if let Some(a) = after.assignments.first_mut() {
            a.amount += 1;
        }
        after.extra.insert("ynabMonths".into(), serde_json::json!([{"month": "2026-01-01", "note": "Hi"}]));
        after.extra.remove("obsolete");
        let patch = roundtrip(&before, &after);
        assert_eq!(patch.extras.unwrap().delete, ["obsolete"]);
    }

    #[test]
    fn the_cases_every_client_shares() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../../../shared/fixtures/patch-cases.json")).unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let mut budget: Budget = serde_json::from_value(case["before"].clone()).unwrap();
            let patch: Patch = serde_json::from_value(case["patch"].clone()).unwrap();
            patch.apply(&mut budget);
            let after: Budget = serde_json::from_value(case["after"].clone()).unwrap();
            assert_eq!(budget, after, "{name}");
        }
    }
}
