//! Three-way merge for sync: two copies of a budget edited apart are combined record by record
//! against the version both last synced (the base), instead of one replacing the other.
//!
//! - A record changed on one side only takes that side's version: added there, it's added;
//!   deleted there and untouched on the other side, it's deleted.
//! - A record changed on both sides merges field by field. Only a field both changed to different
//!   values is a conflict, and so is a record one side deleted while the other edited it.
//! - A transaction's amount, splits and transfer merge as one field, so a split always adds up.
//! - A month's assignments, their extras and movement history form one monetary conflict
//!   domain. Concurrent different plans require a choice; individual move legs never merge.
//! - Lists in the budget's extras (schedules, payees, payee rules, money moves, month notes,
//!   loans, category and group order) merge by their keys the same way; other settings merge as
//!   single fields.
//! - Without a base (a budget that never synced from here), nothing counts as deleted, and every
//!   difference is a conflict.
//!
//! A record one side deleted while the other still uses it (an account the other side added
//! transactions to) is kept. The merged budget is then checked like any save.
use crate::domain::{Account, AccountKind, Assignment, Budget, Category, Transaction};
use crate::patch::Keyed;
use crate::t;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The person's answers to conflicts: a conflict's key → "local" or "cloud". The key "*"
/// answers every conflict that has no answer of its own.
pub type Choices = BTreeMap<String, String>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedChoice {
    pub side: String,
    pub fingerprint: String,
}
pub type ReviewedChoices = BTreeMap<String, ReviewedChoice>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    /// Stable address; a reviewed answer additionally requires an identical input fingerprint.
    pub key: String,
    pub fingerprint: String,
    /// "account", "category", "transaction", "assignment", "schedule", "payee", "payeeRule",
    /// "monthNote", "loan", "budget" or "setting".
    pub kind: &'static str,
    /// The record's name as both sides last synced it: a category's or account's name, a
    /// transaction's payee.
    pub label: String,
    /// What else tells the record apart: a transaction's date, account and amount, an
    /// assignment's month, a category's group.
    pub about: Value,
    /// The field both sides changed; none when one side deleted the record and the other edited it.
    pub field: Option<String>,
    /// Each side's value, with account and category ids given as names. When one side deleted
    /// the record: null for that side and `{ "changed": [fields] }` for the side that edited it.
    pub local: Value,
    pub remote: Value,
}

pub struct Merged {
    pub budget: Budget,
    /// What still needs an answer. The budget then has this side's values there and isn't saved.
    pub conflicts: Vec<Conflict>,
    /// What the merge decided on its own that the person may want to know.
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Local,
    Remote,
}

/// The rule for one value: the side that changed it wins and the same change on both sides is
/// taken once. `None` when both changed it differently.
fn three<'v, T: PartialEq + ?Sized>(
    base: Option<&'v T>,
    local: Option<&'v T>,
    remote: Option<&'v T>,
) -> Option<Option<&'v T>> {
    if local == remote || local == base {
        Some(remote)
    } else if remote == base {
        Some(local)
    } else {
        None
    }
}

struct Kind {
    name: &'static str,
    /// Fields that only make sense together, merged as one and named by the first.
    groups: &'static [&'static [&'static str]],
    /// Records new on the other side go at the end instead of after the record they follow there,
    /// so a save appends them without moving every row.
    append: bool,
}
const ACCOUNT: Kind = Kind {
    name: "account",
    groups: &[],
    append: false,
};
const CATEGORY: Kind = Kind {
    name: "category",
    groups: &[],
    append: false,
};
const TRANSACTION: Kind = Kind {
    name: "transaction",
    groups: &[&[
        "amount",
        "accountId",
        "splits",
        "transferAccountId",
        "transferStatus",
        "status",
    ]],
    append: true,
};
const SCHEDULE: Kind = Kind {
    name: "schedule",
    groups: &[],
    append: false,
};
const PAYEE: Kind = Kind {
    name: "payee",
    groups: &[],
    append: false,
};
const PAYEE_RULE: Kind = Kind {
    name: "payeeRule",
    groups: &[],
    append: false,
};
const MONTH_NOTE: Kind = Kind {
    name: "monthNote",
    groups: &[],
    append: false,
};
const LOAN: Kind = Kind {
    name: "loan",
    groups: &[],
    append: false,
};
/// Bookkeeping from imports that no one edits: when both sides differ, this side's stays. An
/// account's `fileImport` is how its last bank file read, which either side's next import resets.
const QUIET: &[&str] = &[
    "source",
    "transferSource",
    "importedAt",
    "importAudit",
    "importCounts",
    "fileImport",
];

trait Record: Clone + PartialEq {
    fn id(&self) -> String;
    fn json(&self) -> Map<String, Value>;
    fn parse(id: &str, json: Map<String, Value>) -> Result<Self, String>;
}
macro_rules! record {
    ($($t:ty),*) => {$(
        impl Record for $t {
            fn id(&self) -> String {
                self.key().to_owned()
            }
            fn json(&self) -> Map<String, Value> {
                match serde_json::to_value(self) {
                    Ok(Value::Object(m)) => m,
                    _ => Map::new(),
                }
            }
            fn parse(_: &str, json: Map<String, Value>) -> Result<Self, String> {
                serde_json::from_value(Value::Object(json)).map_err(|e| e.to_string())
            }
        }
    )*};
}
record!(Account, Category, Transaction);

/// An entry of a list in the budget's extras, keyed by one of its fields.
#[derive(Clone, PartialEq)]
struct Entry {
    id: String,
    value: Value,
}
impl Record for Entry {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn json(&self) -> Map<String, Value> {
        self.value.as_object().cloned().unwrap_or_default()
    }
    fn parse(id: &str, json: Map<String, Value>) -> Result<Self, String> {
        Ok(Entry {
            id: id.into(),
            value: Value::Object(json),
        })
    }
}

/// Account and category names from all three versions, this side's winning, for conflicts.
struct Names {
    accounts: HashMap<String, String>,
    categories: HashMap<String, String>,
}
impl Names {
    fn new(budgets: [Option<&Budget>; 3]) -> Self {
        let mut names = Names {
            accounts: HashMap::new(),
            categories: HashMap::new(),
        };
        for b in budgets.into_iter().flatten() {
            names
                .accounts
                .extend(b.accounts.iter().map(|a| (a.id.clone(), a.name.clone())));
            names
                .categories
                .extend(b.categories.iter().map(|c| (c.id.clone(), c.name.clone())));
        }
        names
    }
    fn account(&self, id: &str) -> String {
        self.accounts
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned())
    }
    fn category(&self, id: &str) -> String {
        if id == "__rta" {
            return t!("Ready to Assign");
        }
        self.categories
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned())
    }
}

struct Cx<'a> {
    choices: &'a Choices,
    reviewed: Option<&'a ReviewedChoices>,
    names: Names,
    conflicts: Vec<Conflict>,
    notes: Vec<String>,
}
impl Cx<'_> {
    /// The side the person chose. Without an answer the conflict is kept for them, and this
    /// side's value stands in until they answer.
    fn pick(&mut self, conflict: Conflict) -> Side {
        let answer = if let Some(reviewed) = self.reviewed {
            reviewed
                .get(&conflict.key)
                .filter(|choice| choice.fingerprint == conflict.fingerprint)
                .map(|choice| &choice.side)
        } else {
            self.choices
                .get(&conflict.key)
                .or_else(|| self.choices.get("*"))
        };
        match answer.map(String::as_str) {
            Some("cloud") => Side::Remote,
            Some("local") => Side::Local,
            _ => {
                self.conflicts.push(conflict);
                Side::Local
            }
        }
    }
    fn describe(&self, kind: &str, r: &Map<String, Value>) -> (String, Value) {
        let text = |r: &Map<String, Value>, k: &str| {
            r.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        match kind {
            "category" => (text(r, "name"), json!({ "group": text(r, "group") })),
            "transaction" => (
                text(r, "payee"),
                json!({ "date": r.get("date"), "account": self.names.account(&text(r, "accountId")), "amount": r.get("amount") }),
            ),
            "schedule" => {
                let t = r
                    .get("transaction")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                (
                    text(&t, "payee"),
                    json!({ "date": r.get("nextDate"), "account": self.names.account(&text(&t, "accountId")), "amount": t.get("amount") }),
                )
            }
            "payeeRule" => (text(r, "payee"), json!({ "match": text(r, "match") })),
            "monthNote" => (text(r, "month"), json!({})),
            "loan" => (self.names.account(&text(r, "accountId")), json!({})),
            _ => (text(r, "name"), json!({})),
        }
    }
    /// A transaction's money as the person reads it: the amount and where it went.
    fn money(
        &self,
        amount: Option<&Value>,
        splits: Option<&Value>,
        transfer: Option<&Value>,
    ) -> Value {
        let account =
            |v: Option<&Value>| v.and_then(Value::as_str).map(|id| self.names.account(id));
        let lines: Vec<Value> = splits
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|s| {
                json!({
                    "category": s.get("categoryId").and_then(Value::as_str).map(|id| self.names.category(id)),
                    "transfer": account(s.get("transferAccountId")),
                    "amount": s.get("amount"),
                })
            })
            .collect();
        json!({ "amount": amount, "lines": lines, "transfer": account(transfer) })
    }
    /// A field's value for a conflict, with ids given as names.
    fn shown(&self, kind: &str, unit: &[String], values: &[Option<Value>]) -> Value {
        if values.iter().all(Option::is_none) {
            return Value::Null;
        }
        if kind == "transaction" && unit[0] == "amount" {
            let field = |name: &str| {
                unit.iter()
                    .position(|k| k == name)
                    .and_then(|i| values[i].as_ref())
            };
            let mut shown =
                self.money(field("amount"), field("splits"), field("transferAccountId"));
            shown["account"] = field("accountId")
                .and_then(Value::as_str)
                .map(|id| json!(self.names.account(id)))
                .unwrap_or(Value::Null);
            shown["status"] = field("status").cloned().unwrap_or(Value::Null);
            shown["transferStatus"] = field("transferStatus").cloned().unwrap_or(Value::Null);
            return shown;
        }
        let value = values[0].clone().unwrap_or(Value::Null);
        match (unit[0].as_str(), &value) {
            ("transaction", Value::Object(t)) => {
                let mut shown =
                    self.money(t.get("amount"), t.get("splits"), t.get("transferAccountId"));
                shown["payee"] = t.get("payee").cloned().unwrap_or(Value::Null);
                shown["account"] = json!(self.names.account(
                    t.get("accountId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                ));
                shown
            }
            ("accountId" | "transferAccountId" | "paymentAccountId", Value::String(id)) => {
                json!(self.names.account(id))
            }
            ("categoryId", Value::String(id)) => json!(self.names.category(id)),
            _ => value,
        }
    }
}

fn fingerprint(key: &str, input: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(key, input)).expect("JSON values serialize"))
    )
}
fn present(value: Option<&Value>) -> Value {
    json!([value.is_some(), value])
}

/// The fields a record is merged by: its groups, then every other field on its own.
fn units(kind: &Kind, keys: &BTreeSet<String>) -> Vec<Vec<String>> {
    let mut units: Vec<Vec<String>> = kind
        .groups
        .iter()
        .filter(|g| g.iter().any(|k| keys.contains(*k)))
        .map(|g| g.iter().map(|k| k.to_string()).collect())
        .collect();
    for k in keys {
        if !units.iter().any(|u| u.contains(k)) {
            units.push(vec![k.clone()]);
        }
    }
    units
}
fn unit_name(kind: &Kind, key: &str) -> String {
    kind.groups
        .iter()
        .find(|g| g.contains(&key))
        .map_or(key, |g| g[0])
        .to_owned()
}

/// Merges a record both sides changed, field by field.
fn fields<T: Record>(
    cx: &mut Cx,
    kind: &Kind,
    id: &str,
    base: Option<&T>,
    local: &T,
    remote: &T,
) -> Result<T, String> {
    let (b, l, r) = (base.map(Record::json), local.json(), remote.json());
    let mut keys: BTreeSet<String> = l.keys().chain(r.keys()).cloned().collect();
    keys.extend(b.iter().flat_map(|b| b.keys().cloned()));
    // Named as both sides knew it, since the name may be what they changed.
    let (label, about) = cx.describe(kind.name, b.as_ref().unwrap_or(&l));
    let mut out = Map::new();
    for unit in units(kind, &keys) {
        let get = |m: &Map<String, Value>| -> Vec<Option<Value>> {
            unit.iter().map(|k| m.get(k).cloned()).collect()
        };
        let (bv, lv, rv) = (b.as_ref().map(get), get(&l), get(&r));
        let chosen = match three(bv.as_ref(), Some(&lv), Some(&rv)) {
            Some(v) => v.cloned().unwrap_or_default(),
            None if QUIET.contains(&unit[0].as_str()) => lv,
            // No one edits when a transaction was added. Two times (a scheduled transaction both
            // sides entered) settle on the earlier, the same on every device.
            None if unit[0] == "createdAt" => {
                let at =
                    |v: &[Option<Value>]| v[0].as_ref().and_then(Value::as_str).map(str::to_owned);
                match (at(&lv[..]), at(&rv[..])) {
                    (Some(l), Some(r)) if r < l => rv,
                    (None, Some(_)) => rv,
                    _ => lv,
                }
            }
            None => {
                let conflict = Conflict {
                    key: format!("{}/{id}/{}", kind.name, unit[0]),
                    fingerprint: fingerprint(
                        &format!("{}/{id}/{}", kind.name, unit[0]),
                        &json!([
                            bv.as_ref()
                                .map(|v| v.iter().map(|x| present(x.as_ref())).collect::<Vec<_>>()),
                            lv.iter().map(|x| present(x.as_ref())).collect::<Vec<_>>(),
                            rv.iter().map(|x| present(x.as_ref())).collect::<Vec<_>>()
                        ]),
                    ),
                    kind: kind.name,
                    label: label.clone(),
                    about: about.clone(),
                    field: Some(unit[0].clone()),
                    local: cx.shown(kind.name, &unit, &lv),
                    remote: cx.shown(kind.name, &unit, &rv),
                };
                if cx.pick(conflict) == Side::Remote {
                    rv
                } else {
                    lv
                }
            }
        };
        for (k, v) in unit.iter().zip(chosen) {
            if let Some(v) = v {
                out.insert(k.clone(), v);
            }
        }
    }
    T::parse(id, out)
}

/// The merged order of a list's ids: the cloud's, unless this side moved records, with the other
/// side's new records after the record they follow there. A device that only takes in the cloud's
/// changes so ends up with exactly the cloud's order.
fn order(base: &[String], local: &[String], remote: &[String], append: bool) -> Vec<String> {
    let known: HashSet<&String> = base.iter().collect();
    let moved = |side: &[String]| {
        let present: HashSet<&String> = side.iter().collect();
        !base
            .iter()
            .filter(|id| present.contains(id))
            .eq(side.iter().filter(|id| known.contains(id)))
    };
    let (skeleton, other) = if moved(local) {
        (local, remote)
    } else {
        (remote, local)
    };
    let placed: HashSet<&String> = skeleton.iter().collect();
    let mut after: HashMap<Option<&String>, Vec<&String>> = HashMap::new();
    let mut anchor = None;
    for id in other {
        if placed.contains(id) {
            if !append {
                anchor = Some(id);
            }
        } else {
            after.entry(anchor).or_default().push(id);
        }
    }
    let mut out = Vec::with_capacity(skeleton.len() + after.values().map(Vec::len).sum::<usize>());
    fn emit<'a>(
        after: &mut HashMap<Option<&'a String>, Vec<&'a String>>,
        anchor: Option<&'a String>,
        out: &mut Vec<String>,
    ) {
        if let Some(ids) = after.remove(&anchor) {
            out.extend(ids.into_iter().cloned());
        }
    }
    if !append {
        emit(&mut after, None, &mut out);
    }
    for id in skeleton {
        out.push(id.clone());
        emit(&mut after, Some(id), &mut out);
    }
    emit(&mut after, None, &mut out);
    out
}

/// Merges a list of records by id. `seed` gives a record both sides added what they made it from;
/// `removed` gives a record only one side has what it was on the other side, which made it too
/// and has since removed it (`Side` is the side without it).
fn merge_list<T: Record>(
    cx: &mut Cx,
    kind: &Kind,
    base: Option<&[T]>,
    local: &[T],
    remote: &[T],
    seed: impl Fn(&T) -> Option<T>,
    removed: impl Fn(&T, Side) -> Option<T>,
) -> Result<Vec<T>, String> {
    let index = |items: &[T]| -> HashMap<String, usize> {
        items.iter().enumerate().map(|(i, x)| (x.id(), i)).collect()
    };
    let ids = |items: &[T]| -> Vec<String> { items.iter().map(Record::id).collect() };
    let (bi, li, ri) = (
        base.map(index).unwrap_or_default(),
        index(local),
        index(remote),
    );
    let ids = order(
        &base.map(ids).unwrap_or_default(),
        &ids(local),
        &ids(remote),
        kind.append,
    );
    let mut out = Vec::with_capacity(ids.len());
    for id in &ids {
        let b = base.zip(bi.get(id)).map(|(b, &i)| &b[i]);
        let l = li.get(id).map(|&i| &local[i]);
        let r = ri.get(id).map(|&i| &remote[i]);
        // On one side only and new since the base: added there, unless the other side made it
        // too and removed it since. It then merges as a record that side deleted.
        let made_and_removed = match (b, l, r) {
            (None, Some(x), None) => removed(x, Side::Remote),
            (None, None, Some(x)) => removed(x, Side::Local),
            _ => None,
        };
        let b = b.or(made_and_removed.as_ref());
        if let Some(pick) = three(b, l, r) {
            out.extend(pick.cloned());
            continue;
        }
        match (l, r) {
            (Some(l), Some(r)) => {
                // Added on both sides: a record both made from the same thing (a scheduled
                // transaction both posted) merges against what it was made from.
                let seeded = if b.is_none() { seed(l) } else { None };
                let b = b.or(seeded.as_ref());
                match three(b, Some(l), Some(r)) {
                    Some(pick) => out.extend(pick.cloned()),
                    None => out.push(fields(cx, kind, id, b, l, r)?),
                }
            }
            (l, r) => {
                let base = b.map(Record::json).unwrap_or_default();
                let edited = l
                    .or(r)
                    .expect("a record changed on both sides is on one of them")
                    .json();
                let changed: Vec<String> = {
                    let keys: BTreeSet<&String> = base.keys().chain(edited.keys()).collect();
                    let mut names: Vec<String> = vec![];
                    for k in keys.into_iter().filter(|k| base.get(*k) != edited.get(*k)) {
                        let name = unit_name(kind, k);
                        if !names.contains(&name) {
                            names.push(name);
                        }
                    }
                    names
                };
                let side = |x: Option<&T>| x.map_or(Value::Null, |_| json!({ "changed": changed }));
                let (label, about) = cx.describe(kind.name, &base);
                let conflict = Conflict {
                    key: format!("{}/{id}", kind.name),
                    fingerprint: fingerprint(
                        &format!("{}/{id}", kind.name),
                        &json!([
                            b.map(Record::json),
                            l.map(Record::json),
                            r.map(Record::json)
                        ]),
                    ),
                    kind: kind.name,
                    label,
                    about,
                    field: None,
                    local: side(l),
                    remote: side(r),
                };
                out.extend(
                    (if cx.pick(conflict) == Side::Remote {
                        r
                    } else {
                        l
                    })
                    .cloned(),
                );
            }
        }
    }
    Ok(out)
}

fn month_name(month: &str) -> String {
    chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map(crate::i18n::month_year)
        .unwrap_or_else(|_| month.to_owned())
}

/// Shown values carry names, deltas and totals; fingerprints still cover the full raw plan.
fn month_summary(
    month: &str,
    base: Option<&Value>,
    side: Option<&Value>,
    base_budget: Option<&Budget>,
    budget: &Budget,
) -> Result<Value, String> {
    let amounts = |v: Option<&Value>| -> BTreeMap<String, i64> {
        v.and_then(|v| v["assignments"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|a| Some((a["categoryId"].as_str()?.into(), a["amount"].as_i64()?)))
            .collect()
    };
    let b = amounts(base);
    let s = amounts(side);
    let sum = |v: &BTreeMap<String, i64>| {
        v.values().try_fold(0i64, |n, v| {
            n.checked_add(*v)
                .ok_or("The month total is too large to display.".to_owned())
        })
    };
    let base_total = sum(&b)?;
    let total = sum(&s)?;
    let categories: Vec<Value> = b.keys().chain(s.keys()).collect::<BTreeSet<_>>().into_iter().map(|id| {
        let before = b.get(id).copied().unwrap_or(0);
        let amount = s.get(id).copied().unwrap_or(0);
        let name = budget.categories.iter().chain(base_budget.into_iter().flat_map(|b| &b.categories))
            .find(|c| &c.id == id).map(|c| c.name.as_str()).unwrap_or(id);
        Ok(json!({"id":id,"name":name,"base":before,"amount":amount,"delta":amount.checked_sub(before).ok_or("The category delta is too large to display.")?}))
    }).collect::<Result<_,String>>()?;
    Ok(
        json!({"type":"monthPlan","month":month,"currency":budget.currency,"total":total,"baseTotal":base_total,
        "readyToAssignChange":base_total.checked_sub(total).ok_or("The month delta is too large to display.")?,
        "moveCount":side.and_then(|v| v["moves"].as_array()).map_or(0, Vec::len),"categories":categories}),
    )
}

/// A month is an indivisible monetary plan, including all move legs and their history.
fn monetary(
    cx: &mut Cx,
    base: Option<&Budget>,
    local: &Budget,
    remote: &Budget,
) -> Result<(Vec<Assignment>, Option<Value>), String> {
    // A monetary update on only one device retains its exact representation even when
    // the other device changed unrelated records or settings. Presence of an empty history
    // and the stored assignment/history order must not become new edits during sync.
    fn plan(b: &Budget) -> (&[Assignment], Option<&Value>) {
        (&b.assignments, b.extra.get("moneyMoves"))
    }
    let own_plan = |p: (&[Assignment], Option<&Value>)| (p.0.to_vec(), p.1.cloned());
    let base_plan = base.map(plan);
    let local_plan = plan(local);
    let remote_plan = plan(remote);
    if let Some(Some(chosen)) = three(base_plan.as_ref(), Some(&local_plan), Some(&remote_plan)) {
        return Ok(own_plan(*chosen));
    }
    let malformed = |b: &Budget| {
        b.extra.get("moneyMoves").is_some_and(|v| {
            v.as_array().is_none_or(|items| {
                items.iter().any(|m| {
                    m.get("month")
                        .and_then(Value::as_str)
                        .is_none_or(|m| !crate::domain::valid_month(m))
                })
            })
        })
    };
    // Unknown history cannot safely be partitioned into months. Preserve it, together with
    // the full assignment plan, as one opaque conflict domain rather than guessing intent.
    let opaque = base.is_some_and(malformed) || malformed(local) || malformed(remote);
    let blocks = |budget: &Budget| -> BTreeMap<String, Value> {
        if opaque {
            return [("legacy".into(),json!({"assignments":budget.assignments,"historyPresent":budget.extra.contains_key("moneyMoves"),"history":budget.extra.get("moneyMoves")}))].into();
        }
        let mut assignments: BTreeMap<String, Vec<Assignment>> = BTreeMap::new();
        let mut moves: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for assignment in &budget.assignments {
            assignments
                .entry(assignment.month.clone())
                .or_default()
                .push(assignment.clone());
        }
        for list in assignments.values_mut() {
            list.sort_by(|a, b| a.category_id.cmp(&b.category_id));
        }
        for movement in budget
            .extra
            .get("moneyMoves")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            moves
                .entry(movement["month"].as_str().expect("validated month").into())
                .or_default()
                .push(movement.clone());
        }
        let months: BTreeSet<_> = assignments.keys().chain(moves.keys()).cloned().collect();
        months.into_iter().map(|month| {
            let value=json!({"assignments":assignments.remove(&month).unwrap_or_default(),"moves":moves.remove(&month).unwrap_or_default()});
            (month,value)
        }).collect()
    };
    let (b, l, r) = (base.map(blocks), blocks(local), blocks(remote));
    let months: BTreeSet<_> = b
        .iter()
        .flat_map(|m| m.keys())
        .chain(l.keys())
        .chain(r.keys())
        .cloned()
        .collect();
    let mut assignments = vec![];
    let mut moves = vec![];
    let mut opaque_history = None;
    let (mut chose_local, mut chose_remote) = (true, true);
    for month in months {
        let bv = b.as_ref().and_then(|m| m.get(&month));
        let lv = l.get(&month);
        let rv = r.get(&month);
        let key = format!("assignments/{month}");
        let chosen = match three(bv, lv, rv) {
            Some(chosen) => chosen,
            None => {
                let conflict = Conflict {
                    key: key.clone(),
                    fingerprint: fingerprint(&key, &json!([present(bv), present(lv), present(rv)])),
                    kind: "assignment",
                    label: if opaque {
                        t!("Budget")
                    } else {
                        month_name(&month)
                    },
                    about: json!({"month":month,"wholeMonth":true}),
                    field: Some("monthPlan".into()),
                    local: if opaque {
                        lv.cloned().unwrap_or(Value::Null)
                    } else {
                        month_summary(&month, bv, lv, base, local)?
                    },
                    remote: if opaque {
                        rv.cloned().unwrap_or(Value::Null)
                    } else {
                        month_summary(&month, bv, rv, base, remote)?
                    },
                };
                if cx.pick(conflict) == Side::Remote {
                    rv
                } else {
                    lv
                }
            }
        };
        chose_local &= chosen == lv;
        chose_remote &= chosen == rv;
        if let Some(chosen) = chosen {
            // The normalized block is for comparison/fingerprints; use the selected source
            // order when reconstructing a plan containing months from different devices.
            let source = if Some(chosen) == lv { local } else { remote };
            assignments.extend(
                source
                    .assignments
                    .iter()
                    .filter(|a| opaque || a.month == month)
                    .cloned(),
            );
            if opaque {
                if chosen["historyPresent"] == true {
                    opaque_history = Some(chosen["history"].clone());
                }
            } else {
                moves.extend(chosen["moves"].as_array().into_iter().flatten().cloned());
            }
        }
    }
    // Choosing every month from one version must also retain its interleaved history and
    // assignment order, rather than regrouping that version by month.
    if chose_local {
        return Ok(own_plan(local_plan));
    }
    if chose_remote {
        return Ok(own_plan(remote_plan));
    }
    let history = if opaque {
        opaque_history
    } else if !moves.is_empty()
        || local.extra.contains_key("moneyMoves")
        || remote.extra.contains_key("moneyMoves")
    {
        Some(Value::Array(moves))
    } else {
        None
    };
    Ok((assignments, history))
}

/// A list in the extras keyed by `field`, or `None` when it can't be (an entry without the key,
/// or two with the same one): it then merges as a single setting.
fn keyed(
    cx: &mut Cx,
    kind: &Kind,
    field: &str,
    has_base: bool,
    b: Option<&Value>,
    l: Option<&Value>,
    r: Option<&Value>,
) -> Result<Option<Value>, String> {
    let entries = |v: Option<&Value>| -> Option<Vec<Entry>> {
        let Some(v) = v else { return Some(vec![]) };
        let mut seen = HashSet::new();
        v.as_array()?
            .iter()
            .map(|x| {
                let id = x.get(field)?.as_str()?.to_owned();
                seen.insert(id.clone()).then(|| Entry {
                    id,
                    value: x.clone(),
                })
            })
            .collect()
    };
    let (Some(bl), Some(ll), Some(rl)) = (entries(b), entries(l), entries(r)) else {
        return Ok(None);
    };
    let merged = merge_list(
        cx,
        kind,
        has_base.then_some(&bl[..]),
        &ll,
        &rl,
        |_| None,
        |_, _| None,
    )?;
    Ok(Some(Value::Array(
        merged.into_iter().map(|e| e.value).collect(),
    )))
}

/// Payees both sides added under the same name are one payee: this side's.
fn dedupe_payees(list: &mut Value, b: Option<&Value>, l: Option<&Value>) {
    let entries = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let id = |v: &Value| {
        v.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let name = |v: &Value| {
        v.get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_lowercase()
    };
    let local = entries(l);
    let names: HashSet<String> = local.iter().map(name).collect();
    let known: HashSet<String> = local.iter().chain(entries(b).iter()).map(id).collect();
    if let Some(list) = list.as_array_mut() {
        list.retain(|p| known.contains(&id(p)) || !names.contains(&name(p)));
    }
}

/// A list of ids (the category or group order): ids either side added are kept, ids either side
/// removed stay removed. `None` when it isn't a list of ids.
fn id_list(b: Option<&Value>, l: Option<&Value>, r: Option<&Value>) -> Option<Value> {
    let ids = |v: Option<&Value>| -> Option<Vec<String>> {
        match v {
            None => Some(vec![]),
            Some(v) => v
                .as_array()?
                .iter()
                .map(|x| x.as_str().map(str::to_owned))
                .collect(),
        }
    };
    let (bl, ll, rl) = (ids(b)?, ids(l)?, ids(r)?);
    let set = |v: &[String]| -> HashSet<String> { v.iter().cloned().collect() };
    let (bs, ls, rs) = (set(&bl), set(&ll), set(&rl));
    let mut seen = HashSet::new();
    let kept: Vec<String> = order(&bl, &ll, &rl, false)
        .into_iter()
        .filter(|id| {
            let has = |s: &HashSet<String>| s.contains(id).then_some(&());
            matches!(three(has(&bs), has(&ls), has(&rs)), Some(Some(_))) && seen.insert(id.clone())
        })
        .collect();
    Some(json!(kept))
}

fn setting(
    cx: &mut Cx,
    key: &str,
    b: Option<&Value>,
    l: Option<&Value>,
    r: Option<&Value>,
) -> Option<Value> {
    if QUIET.contains(&key) {
        return l.cloned();
    }
    let conflict = Conflict {
        key: format!("setting/{key}"),
        fingerprint: fingerprint(
            &format!("setting/{key}"),
            &json!([present(b), present(l), present(r)]),
        ),
        kind: "setting",
        label: key.to_owned(),
        about: json!({}),
        field: Some(key.to_owned()),
        local: l.cloned().unwrap_or(Value::Null),
        remote: r.cloned().unwrap_or(Value::Null),
    };
    (if cx.pick(conflict) == Side::Remote {
        r
    } else {
        l
    })
    .cloned()
}

fn extras(
    cx: &mut Cx,
    base: Option<&Budget>,
    local: &Budget,
    remote: &Budget,
) -> Result<Map<String, Value>, String> {
    let mut keys: BTreeSet<&String> = local.extra.keys().chain(remote.extra.keys()).collect();
    keys.extend(base.iter().flat_map(|b| b.extra.keys()));
    let has_base = base.is_some();
    let mut out = Map::new();
    for key in keys {
        if key == "moneyMoves" {
            continue;
        }
        let (b, l, r) = (
            base.and_then(|x| x.extra.get(key)),
            local.extra.get(key),
            remote.extra.get(key),
        );
        let value = match three(b, l, r) {
            Some(pick) => pick.cloned(),
            None => {
                let list = match key.as_str() {
                    "schedules" => keyed(cx, &SCHEDULE, "id", has_base, b, l, r)?,
                    "payees" => keyed(cx, &PAYEE, "id", has_base, b, l, r)?.map(|mut list| {
                        dedupe_payees(&mut list, b, l);
                        list
                    }),
                    "payeeRules" => keyed(cx, &PAYEE_RULE, "id", has_base, b, l, r)?,
                    "ynabMonths" => keyed(cx, &MONTH_NOTE, "month", has_base, b, l, r)?,
                    "loans" => keyed(cx, &LOAN, "accountId", has_base, b, l, r)?,
                    "categoryOrder" | "groupOrder" => id_list(b, l, r),
                    _ => None,
                };
                match list {
                    // An emptied list this side never had stays absent.
                    Some(list) => (l.is_some() || list.as_array().is_none_or(|a| !a.is_empty()))
                        .then_some(list),
                    None => setting(cx, key, b, l, r),
                }
            }
        };
        if let Some(v) = value {
            out.insert(key.clone(), v);
        }
    }
    Ok(out)
}

fn scalar(
    cx: &mut Cx,
    field: &str,
    label: &str,
    base: Option<&str>,
    local: &str,
    remote: &str,
) -> String {
    if let Some(Some(v)) = three(base, Some(local), Some(remote)) {
        return v.to_owned();
    }
    let conflict = Conflict {
        key: format!("budget/{field}"),
        fingerprint: fingerprint(&format!("budget/{field}"), &json!([base, local, remote])),
        kind: "budget",
        label: label.to_owned(),
        about: json!({}),
        field: Some(field.to_owned()),
        local: json!(local),
        remote: json!(remote),
    };
    (if cx.pick(conflict) == Side::Remote {
        remote
    } else {
        local
    })
    .to_owned()
}

/// What a scheduled transaction both sides posted looked like when it was posted, from the
/// schedule as it was at the last sync.
fn posted(base: Option<&Budget>, t: &Transaction) -> Option<Transaction> {
    let id = t.extra.get("scheduleId")?.as_str()?;
    let due = t.extra.get("scheduledDate")?.as_str()?;
    let schedule = crate::schedules::read(base?)
        .ok()?
        .into_iter()
        .find(|s| s.id == id)?;
    let posted = crate::schedules::occurrence(&schedule, due, due);
    (posted.id == t.id).then_some(posted)
}

/// A scheduled transaction one side entered after the base, when the other side's schedule has
/// since gone past that occurrence without keeping it (entered there and deleted, skipped, or the
/// schedule moved on): what was entered, so the entry merges as one the other side deleted. An
/// entry nobody touched since (the phone entering due schedules as it opens) then goes; one
/// edited since asks. Without this, every device's own copy of the occurrence would bring an
/// entry deleted elsewhere back.
fn removed_occurrence(
    base: Option<&Budget>,
    other: &Budget,
    t: &Transaction,
) -> Option<Transaction> {
    let id = t.extra.get("scheduleId")?.as_str()?;
    let due = t.extra.get("scheduledDate")?.as_str()?;
    let then = crate::schedules::read(base?)
        .ok()?
        .into_iter()
        .find(|s| s.id == id)?;
    // At the last sync the occurrence was still to come.
    if then.next_date.as_deref().is_none_or(|next| next > due) {
        return None;
    }
    let now = crate::schedules::read(other)
        .ok()?
        .into_iter()
        .find(|s| s.id == id)?;
    if now.next_date.as_deref().is_some_and(|next| next <= due) {
        return None;
    }
    let mut entered = posted(base, t)?;
    // Entering it stamps when; that isn't an edit.
    if let Some(at) = t.created_at() {
        entered.set_created_at(at);
    }
    Some(entered)
}

fn insert_like<T: Record>(list: &mut Vec<T>, item: T, source: &[T]) {
    let id = item.id();
    let at = source
        .iter()
        .position(|x| x.id() == id)
        .and_then(|i| {
            source[..i]
                .iter()
                .rev()
                .find_map(|p| list.iter().position(|x| x.id() == p.id()))
        })
        .map_or(list.len(), |i| i + 1);
    list.insert(at, item);
}

/// Brings back accounts and categories one side deleted while the merged budget still uses them.
fn restore(cx: &mut Cx, merged: &mut Budget, local: &Budget, remote: &Budget) {
    let mut accounts: BTreeSet<String> = BTreeSet::new();
    let mut categories: BTreeSet<String> = BTreeSet::new();
    let mut uses = |t: &Transaction| {
        accounts.insert(t.account_id.clone());
        accounts.extend(t.transfer_account_id.clone());
        for s in &t.splits {
            categories.extend(s.category_id.clone());
            accounts.extend(crate::ledger::transfer(s).map(str::to_owned));
        }
    };
    merged.transactions.iter().for_each(&mut uses);
    crate::schedules::read(merged)
        .unwrap_or_default()
        .iter()
        .for_each(|s| uses(&s.transaction));
    categories.extend(merged.assignments.iter().map(|a| a.category_id.clone()));
    for field in ["payees", "payeeRules", "loans"] {
        for item in merged
            .extra
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // Payee defaults, import rules and loan terms all use categoryId. Loan terms
            // additionally own an account; unknown extras are not modeled references.
            categories.extend(
                item.get("categoryId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
            if field == "loans" {
                accounts.extend(
                    item.get("accountId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                );
            }
        }
    }
    // A retained payment category requires its card, and a retained on-budget card
    // requires its payment category. Follow those references after restoring records
    // used by transactions/assignments; deleting both unused records stays a deletion.
    loop {
        let before = (merged.accounts.len(), merged.categories.len());
        accounts.extend(
            merged
                .categories
                .iter()
                .filter_map(|c| c.payment_account().map(str::to_owned)),
        );
        let have: HashSet<String> = merged.accounts.iter().map(|a| a.id.clone()).collect();
        for id in accounts.iter().filter(|id| !have.contains(*id)) {
            for side in [&local.accounts, &remote.accounts] {
                if let Some(a) = side.iter().find(|a| &a.id == id) {
                    cx.notes.push(t!("Kept the account “{name}”: one device deleted it while the other still used it.", name = a.name));
                    insert_like(&mut merged.accounts, a.clone(), side);
                    break;
                }
            }
        }
        for a in merged
            .accounts
            .iter()
            .filter(|a| a.on_budget() && a.kind == AccountKind::Credit)
        {
            if merged
                .categories
                .iter()
                .any(|c| c.payment_account() == Some(a.id.as_str()))
            {
                continue;
            }
            let candidates = || {
                local
                    .categories
                    .iter()
                    .chain(&remote.categories)
                    .filter(|c| c.payment_account() == Some(a.id.as_str()))
            };
            // Prefer a category already required by a retained assignment.
            if let Some(c) = candidates()
                .find(|c| categories.contains(&c.id))
                .or_else(|| candidates().next())
            {
                categories.insert(c.id.clone());
            }
        }
        let have: HashSet<String> = merged.categories.iter().map(|c| c.id.clone()).collect();
        for id in categories.iter().filter(|id| !have.contains(*id)) {
            for side in [&local.categories, &remote.categories] {
                if let Some(c) = side.iter().find(|c| &c.id == id) {
                    cx.notes.push(t!("Kept the category “{name}”: one device deleted it while the other still used it.", name = c.name));
                    insert_like(&mut merged.categories, c.clone(), side);
                    break;
                }
            }
        }
        if before == (merged.accounts.len(), merged.categories.len()) {
            break;
        }
    }
}

/// Merges this device's budget and the cloud's against the version both last synced, if known.
/// All three are without heavy extras.
pub fn merge(
    base: Option<&Budget>,
    local: &Budget,
    remote: &Budget,
    choices: &Choices,
) -> Result<Merged, String> {
    merge_inner(base, local, remote, choices, None)
}

/// V2 conflict answers are valid only while the actual underlying inputs remain unchanged.
/// Account/incarnation/version guards are additionally enforced by the service command.
pub fn merge_reviewed(
    base: Option<&Budget>,
    local: &Budget,
    remote: &Budget,
    choices: &ReviewedChoices,
) -> Result<Merged, String> {
    merge_inner(base, local, remote, &Choices::new(), Some(choices))
}

fn merge_inner(
    base: Option<&Budget>,
    local: &Budget,
    remote: &Budget,
    choices: &Choices,
    reviewed: Option<&ReviewedChoices>,
) -> Result<Merged, String> {
    if local == remote {
        return Ok(Merged {
            budget: local.clone(),
            conflicts: vec![],
            notes: vec![],
        });
    }
    let mut cx = Cx {
        choices,
        reviewed,
        names: Names::new([base, Some(remote), Some(local)]),
        conflicts: vec![],
        notes: vec![],
    };
    let name = scalar(
        &mut cx,
        "name",
        &local.name,
        base.map(|b| b.name.as_str()),
        &local.name,
        &remote.name,
    );
    let currency = scalar(
        &mut cx,
        "currency",
        &local.name,
        base.map(|b| b.currency.as_str()),
        &local.currency,
        &remote.currency,
    );
    let (assignments, history) = monetary(&mut cx, base, local, remote)?;
    let mut extra = extras(&mut cx, base, local, remote)?;
    if let Some(history) = history {
        extra.insert("moneyMoves".into(), history);
    }
    let mut budget = Budget {
        schema_version: local.schema_version.max(remote.schema_version),
        name,
        currency,
        accounts: merge_list(
            &mut cx,
            &ACCOUNT,
            base.map(|b| &b.accounts[..]),
            &local.accounts,
            &remote.accounts,
            |_| None,
            |_, _| None,
        )?,
        categories: merge_list(
            &mut cx,
            &CATEGORY,
            base.map(|b| &b.categories[..]),
            &local.categories,
            &remote.categories,
            |_| None,
            |_, _| None,
        )?,
        transactions: merge_list(
            &mut cx,
            &TRANSACTION,
            base.map(|b| &b.transactions[..]),
            &local.transactions,
            &remote.transactions,
            |t| posted(base, t),
            |t, without| {
                removed_occurrence(
                    base,
                    if without == Side::Remote {
                        remote
                    } else {
                        local
                    },
                    t,
                )
            },
        )?,
        assignments,
        extra,
    };
    restore(&mut cx, &mut budget, local, remote);
    // Preserve the chosen representation exactly on one-sided updates after reference repair.
    // Regrouping assignment order or adding an empty history is not a user edit to echo-upload.
    if cx.conflicts.is_empty() && cx.notes.is_empty() {
        if base == Some(local) {
            budget = remote.clone();
        } else if base == Some(remote) {
            budget = local.clone();
        }
    }
    if cx.conflicts.is_empty() {
        if let Err(error) = crate::domain::validate(&budget) {
            // Valid inputs can violate a cross-record invariant only after combining. Keep both
            // versions reviewable instead of permanently failing every future sync attempt.
            crate::domain::validate(local)?;
            crate::domain::validate(remote)?;
            // The repaired combination is being discarded, so its preservation notes no
            // longer describe the selected complete version.
            cx.notes.clear();
            let key = "budget/validation";
            let shown = |b: &Budget| json!({"name":b.name,"accounts":b.accounts.len(),"categories":b.categories.len(),"transactions":b.transactions.len(),"reason":t!("The two versions couldn't be combined: {error}", error = error)});
            let conflict = Conflict {
                key: key.into(),
                fingerprint: fingerprint(key, &json!([base, local, remote])),
                kind: "budget",
                label: local.name.clone(),
                about: json!({"validation":error,"wholeBudget":true}),
                field: Some("budgetVersion".into()),
                local: shown(local),
                remote: shown(remote),
            };
            budget = if cx.pick(conflict) == Side::Remote {
                remote.clone()
            } else {
                local.clone()
            };
        }
    }
    Ok(Merged {
        budget,
        conflicts: cx.conflicts,
        notes: cx.notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tests::{purchase, sample};
    use crate::domain::Split;

    fn transaction(id: &str, amount: i64) -> Transaction {
        let mut t = purchase();
        t.id = id.into();
        t.amount = amount;
        t.splits[0].amount = amount;
        t
    }
    fn base() -> Budget {
        let mut b = sample();
        b.transactions = vec![transaction("t1", -1000), transaction("t2", -2000)];
        b
    }
    fn clean(base: &Budget, local: &Budget, remote: &Budget) -> Budget {
        let merged = merge(Some(base), local, remote, &Choices::new()).unwrap();
        assert!(merged.conflicts.is_empty(), "{:?}", merged.conflicts);
        merged.budget
    }
    fn answer(pairs: &[(&str, &str)]) -> Choices {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn valid_versions_with_incompatible_card_links_require_review_of_a_whole_version() {
        let mut b = base();
        b.categories.push(Category {
            id: "spare".into(),
            name: "Spare".into(),
            ..b.categories[0].clone()
        });
        let (mut local, mut remote) = (b.clone(), b.clone());
        local.categories.retain(|c| c.id != "spare");
        remote.extra.insert(
            "payees".into(),
            json!([{
                "id":"merchant", "name":"Merchant", "categoryId":"spare"
            }]),
        );
        // Both complete versions are valid; combining their distinct replacement IDs would
        // give the same card two payment categories.
        local.categories[1].id = "local-payment".into();
        remote.categories[1].id = "remote-payment".into();
        local.transactions.push(transaction("local-entry", -300));
        remote.transactions.push(transaction("remote-entry", -700));
        for input in [&b, &local, &remote] {
            crate::domain::validate(input).unwrap();
        }
        let unresolved =
            merge_reviewed(Some(&b), &local, &remote, &ReviewedChoices::new()).unwrap();
        assert_eq!(unresolved.conflicts.len(), 1);
        let conflict = &unresolved.conflicts[0];
        assert_eq!(conflict.key, "budget/validation");
        assert_eq!(conflict.kind, "budget");
        assert_eq!(conflict.about["wholeBudget"], true);
        assert_eq!(unresolved.budget, local);
        // Reference repair happened only in the discarded combination. Its notes must not
        // claim that the selected full version retained a category it does not contain.
        assert!(unresolved.notes.is_empty());
        crate::domain::validate(&unresolved.budget).unwrap();
        for (side, expected) in [("local", &local), ("cloud", &remote)] {
            let choices = [(
                conflict.key.clone(),
                ReviewedChoice {
                    side: side.into(),
                    fingerprint: conflict.fingerprint.clone(),
                },
            )]
            .into();
            let resolved = merge_reviewed(Some(&b), &local, &remote, &choices).unwrap();
            assert!(resolved.conflicts.is_empty());
            assert_eq!(&resolved.budget, expected);
            crate::domain::validate(&resolved.budget).unwrap();
            let mut advanced = remote.clone();
            advanced.transactions[0].memo = "Not part of the reviewed version".into();
            let stale = merge_reviewed(Some(&b), &local, &advanced, &choices).unwrap();
            assert_eq!(stale.conflicts.len(), 1);
            assert_eq!(stale.conflicts[0].key, conflict.key);
            assert_ne!(stale.conflicts[0].fingerprint, conflict.fingerprint);
        }
    }

    #[test]
    fn payee_defaults_rules_and_loan_terms_keep_the_records_they_reference() {
        let mut b = base();
        b.categories.push(Category {
            id: "linked".into(),
            name: "Linked category".into(),
            ..b.categories[0].clone()
        });
        b.accounts.push(Account {
            id: "loan".into(),
            name: "Loan".into(),
            kind: AccountKind::Tracking,
            ..b.accounts[0].clone()
        });
        let terms = json!({"accountId":"loan", "categoryId":"linked", "annualRateBasisPoints":350,
            "monthlyPayment":10000, "monthlyFees":0, "rateHistory":[]});
        serde_json::from_value::<crate::loans::Terms>(terms.clone()).unwrap();
        for (field, record) in [
            (
                "payees",
                json!({"id":"payee", "name":"Merchant", "categoryId":"linked"}),
            ),
            (
                "payeeRules",
                json!({"id":"rule", "match":"MERCHANT", "payee":"Merchant", "categoryId":"linked"}),
            ),
            ("loans", terms),
        ] {
            let mut deleted = b.clone();
            deleted.categories.retain(|c| c.id != "linked");
            deleted.accounts.retain(|a| a.id != "loan");
            let mut used = b.clone();
            used.extra.insert(field.into(), json!([record]));
            for input in [&deleted, &used] {
                crate::domain::validate(input).unwrap();
            }
            for (local, remote) in [(&deleted, &used), (&used, &deleted)] {
                let merged = merge(Some(&b), local, remote, &Choices::new()).unwrap();
                assert!(
                    merged.conflicts.is_empty(),
                    "{field}: {:?}",
                    merged.conflicts
                );
                assert!(
                    merged.budget.categories.iter().any(|c| c.id == "linked"),
                    "{field}"
                );
                assert_eq!(merged.budget.extra[field], used.extra[field]);
                assert_eq!(
                    merged.budget.accounts.iter().any(|a| a.id == "loan"),
                    field == "loans"
                );
                crate::domain::validate(&merged.budget).unwrap();
                if field == "loans" {
                    crate::loans::apply_new_payments(&mut merged.budget.clone(), &HashSet::new())
                        .unwrap();
                }
            }
            // Deleting a reference with its unused records remains a deletion.
            let mut renamed = used.clone();
            renamed.name = "Renamed".into();
            let merged = clean(&used, &deleted, &renamed);
            assert!(!merged.categories.iter().any(|c| c.id == "linked"));
            assert!(!merged.accounts.iter().any(|a| a.id == "loan"));
        }
    }

    #[test]
    fn unrelated_edits_do_not_reorder_a_selected_monthly_plan_or_its_history() {
        let mut b = base();
        b.assignments = vec![
            Assignment {
                month: "2026-11".into(),
                category_id: "payment".into(),
                amount: 200,
                extra: Default::default(),
            },
            Assignment {
                month: "2026-10".into(),
                category_id: "food".into(),
                amount: 100,
                extra: Default::default(),
            },
            Assignment {
                month: "2026-11".into(),
                category_id: "food".into(),
                amount: 300,
                extra: Default::default(),
            },
        ];
        b.extra.insert(
            "moneyMoves".into(),
            json!([
                {"month":"2026-11","at":"z","id":"first"},
                {"month":"2026-10","at":"a","id":"second"},
                {"month":"2026-11","at":"b","id":"third"}
            ]),
        );
        let mut edited = b.clone();
        edited.assignments[0].amount += 50;
        let mut renamed = b.clone();
        renamed.name = "Renamed separately".into();
        for (local, remote) in [(&renamed, &edited), (&edited, &renamed)] {
            let result = clean(&b, local, remote);
            assert_eq!(result.assignments, edited.assignments);
            assert_eq!(result.extra["moneyMoves"], edited.extra["moneyMoves"]);
            assert_eq!(result.name, renamed.name);
        }
        // Removing an empty history must not recreate it when another device edits the name.
        b.extra.insert("moneyMoves".into(), json!([]));
        edited = b.clone();
        edited.extra.remove("moneyMoves");
        renamed = b.clone();
        renamed.name = "Renamed separately".into();
        for (local, remote) in [(&renamed, &edited), (&edited, &renamed)] {
            let result = clean(&b, local, remote);
            assert_eq!(result.assignments, edited.assignments);
            assert!(!result.extra.contains_key("moneyMoves"));
        }
    }

    #[test]
    fn transfer_endpoints_and_reconciliation_are_one_reviewed_money_value() {
        let mut b = base();
        b.accounts.push(Account {
            id: "wallet".into(),
            name: "Wallet".into(),
            ..b.accounts[0].clone()
        });
        b.transactions[0].transfer_account_id = Some("wallet".into());
        b.transactions[0].transfer_status = Some(crate::domain::Cleared::Cleared);
        b.transactions[0].splits.clear();
        let (mut l, mut r) = (b.clone(), b.clone());
        l.transactions[0].account_id = "wallet".into();
        l.transactions[0].transfer_account_id = Some("cash".into());
        r.transactions[0].amount = -2000;
        let m = merge(Some(&b), &l, &r, &Choices::new()).unwrap();
        assert_eq!(m.conflicts.len(), 1);
        assert_eq!(m.conflicts[0].local["account"], "Wallet");
        for side in ["local", "cloud"] {
            let m = merge(
                Some(&b),
                &l,
                &r,
                &answer(&[("transaction/t1/amount", side)]),
            )
            .unwrap();
            assert!(m.conflicts.is_empty());
            crate::domain::validate(&m.budget).unwrap();
        }
        let (mut l, mut r) = (b.clone(), b.clone());
        l.transactions[0].status = crate::domain::Cleared::Reconciled;
        r.transactions[0].amount = -2000;
        assert_eq!(
            merge(Some(&b), &l, &r, &Choices::new())
                .unwrap()
                .conflicts
                .len(),
            1
        );
    }
    #[test]
    fn month_review_has_named_deltas_totals_and_preserves_move_order() {
        let mut b = base();
        b.assignments = vec![Assignment {
            extra: Default::default(),
            month: "2026-10".into(),
            category_id: "food".into(),
            amount: 100,
        }];
        b.extra.insert("moneyMoves".into(),json!([{ "month":"2026-10","at":"z","id":"first" },{ "month":"2026-10","at":"a","id":"second" }]));
        let (mut l, mut r) = (b.clone(), b.clone());
        l.assignments[0].amount = 200;
        r.assignments[0].amount = 300;
        let m = merge(Some(&b), &l, &r, &Choices::new()).unwrap();
        assert_eq!(m.conflicts[0].local["total"], 200);
        assert_eq!(m.conflicts[0].local["baseTotal"], 100);
        assert_eq!(m.conflicts[0].local["readyToAssignChange"], -100);
        assert_eq!(m.conflicts[0].local["categories"][0]["name"], "Food");
        let m = merge(
            Some(&b),
            &l,
            &r,
            &answer(&[("assignments/2026-10", "cloud")]),
        )
        .unwrap();
        assert_eq!(m.budget.extra["moneyMoves"], b.extra["moneyMoves"]);
        let m = merge(Some(&b), &b, &r, &Choices::new()).unwrap();
        assert_eq!(m.budget, r);
    }

    #[test]
    fn equal_moves_to_different_destinations_never_create_assigned_money() {
        let mut base = sample();
        let mut rent = base.categories[0].clone();
        rent.id = "rent".into();
        rent.name = "Rent".into();
        base.categories.push(rent);
        let (mut local, mut remote) = (base.clone(), base.clone());
        for (budget, destination) in [(&mut local, "payment"), (&mut remote, "rent")] {
            budget.assignments[0].amount -= 5000;
            budget.assignments.push(Assignment {
                month: "2026-10".into(),
                category_id: destination.into(),
                amount: 5000,
                extra: Default::default(),
            });
            budget.extra.insert("moneyMoves".into(),json!([{"month":"2026-10","at":"2026-10-01","from":"food","to":destination,"amount":5000}]));
        }
        let unresolved =
            merge_reviewed(Some(&base), &local, &remote, &ReviewedChoices::new()).unwrap();
        assert_eq!(unresolved.conflicts.len(), 1);
        let conflict = &unresolved.conflicts[0];
        assert_eq!(conflict.key, "assignments/2026-10");
        for (side, expected) in [("local", &local), ("cloud", &remote)] {
            let choices = [(
                conflict.key.clone(),
                ReviewedChoice {
                    side: side.into(),
                    fingerprint: conflict.fingerprint.clone(),
                },
            )]
            .into();
            let merged = merge_reviewed(Some(&base), &local, &remote, &choices).unwrap();
            assert!(merged.conflicts.is_empty());
            assert_eq!(
                merged
                    .budget
                    .assignments
                    .iter()
                    .map(|a| a.amount)
                    .sum::<i64>(),
                20_000
            );
            assert_eq!(
                merged.budget.extra["moneyMoves"],
                expected.extra["moneyMoves"]
            );
        }
    }
    #[test]
    fn reviewed_answer_requires_the_same_raw_values_but_not_the_same_display_names() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.transactions[0].memo = "Local".into();
        remote.transactions[0].memo = "Remote".into();
        let result = merge_reviewed(Some(&base), &local, &remote, &ReviewedChoices::new()).unwrap();
        let conflict = &result.conflicts[0];
        let choices = [(
            conflict.key.clone(),
            ReviewedChoice {
                side: "local".into(),
                fingerprint: conflict.fingerprint.clone(),
            },
        )]
        .into();
        remote.accounts[0].name = "Renamed cash".into();
        assert!(merge_reviewed(Some(&base), &local, &remote, &choices)
            .unwrap()
            .conflicts
            .is_empty());
        remote.transactions[0].memo = "Unseen new edit".into();
        let changed = merge_reviewed(Some(&base), &local, &remote, &choices).unwrap();
        assert_eq!(changed.conflicts.len(), 1);
        assert_ne!(changed.conflicts[0].fingerprint, conflict.fingerprint);
    }
    #[test]
    fn unrecognized_move_history_is_preserved_with_its_whole_assignment_plan() {
        let mut base = sample();
        base.extra
            .insert("moneyMoves".into(), json!({"legacy":"opaque"}));
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.assignments[0].amount = 25000;
        remote
            .extra
            .insert("moneyMoves".into(), json!({"legacy":"different"}));
        let result = merge_reviewed(Some(&base), &local, &remote, &ReviewedChoices::new()).unwrap();
        assert_eq!(result.conflicts.len(), 1);
        let c = &result.conflicts[0];
        assert_eq!(c.key, "assignments/legacy");
        let choices = [(
            c.key.clone(),
            ReviewedChoice {
                side: "cloud".into(),
                fingerprint: c.fingerprint.clone(),
            },
        )]
        .into();
        let merged = merge_reviewed(Some(&base), &local, &remote, &choices).unwrap();
        assert_eq!(merged.budget.assignments, remote.assignments);
        assert_eq!(
            merged.budget.extra["moneyMoves"],
            remote.extra["moneyMoves"]
        );
    }

    #[test]
    fn both_sides_adding_transactions_keeps_both() {
        let base = base();
        let (mut phone, mut computer) = (base.clone(), base.clone());
        phone.transactions.push(transaction("from-phone", -300));
        computer
            .transactions
            .push(transaction("from-computer", -700));
        let merged = clean(&base, &computer, &phone);
        let ids: Vec<&str> = merged.transactions.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["t1", "t2", "from-phone", "from-computer"]);
        // The same merge from the phone's side has the same records.
        let other = clean(&base, &phone, &computer);
        assert_eq!(other.transactions.len(), 4);
    }

    #[test]
    fn edits_to_different_fields_of_one_transaction_merge() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.transactions[0].memo = "Lunch".into();
        remote.transactions[0].amount = -1500;
        remote.transactions[0].splits[0].amount = -1500;
        remote.transactions[1].status = crate::domain::Cleared::Reconciled;
        let merged = clean(&base, &local, &remote);
        assert_eq!(merged.transactions[0].memo, "Lunch");
        assert_eq!(merged.transactions[0].amount, -1500);
        assert_eq!(
            merged.transactions[1].status,
            crate::domain::Cleared::Reconciled
        );
    }

    #[test]
    fn amount_and_splits_merge_as_one_so_a_split_always_adds_up() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.transactions[0].amount = -1200;
        local.transactions[0].splits[0].amount = -1200;
        remote.transactions[0].splits = vec![
            Split {
                extra: Default::default(),
                category_id: Some("food".into()),
                amount: -400,
            },
            Split {
                extra: Default::default(),
                category_id: None,
                amount: -600,
            },
        ];
        let merged = merge(Some(&base), &local, &remote, &Choices::new()).unwrap();
        assert_eq!(merged.conflicts.len(), 1);
        let c = &merged.conflicts[0];
        assert_eq!(
            (c.key.as_str(), c.field.as_deref()),
            ("transaction/t1/amount", Some("amount"))
        );
        assert_eq!(c.local["amount"], -1200);
        assert_eq!(
            c.remote["lines"][0],
            json!({ "category": "Food", "transfer": null, "amount": -400 })
        );
        // Either answer is a whole, valid transaction.
        let theirs = merge(
            Some(&base),
            &local,
            &remote,
            &answer(&[("transaction/t1/amount", "cloud")]),
        )
        .unwrap();
        assert_eq!(
            (
                theirs.budget.transactions[0].amount,
                theirs.budget.transactions[0].splits.len()
            ),
            (-1000, 2)
        );
        let ours = merge(
            Some(&base),
            &local,
            &remote,
            &answer(&[("transaction/t1/amount", "local")]),
        )
        .unwrap();
        assert_eq!(
            (
                ours.budget.transactions[0].amount,
                ours.budget.transactions[0].splits.len()
            ),
            (-1200, 1)
        );
    }

    #[test]
    fn one_side_editing_while_the_other_deletes_asks_and_shows_the_record() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.transactions.retain(|t| t.id != "t2");
        remote.transactions[1].memo = "Birthday present".into();
        let merged = merge(Some(&base), &local, &remote, &Choices::new()).unwrap();
        assert_eq!(
            merged.conflicts,
            [Conflict {
                key: "transaction/t2".into(),
                fingerprint: merged.conflicts[0].fingerprint.clone(),
                kind: "transaction",
                label: "Shop".into(),
                about: json!({ "date": "2026-10-15", "account": "Card", "amount": -2000 }),
                field: None,
                local: Value::Null,
                remote: json!({ "changed": ["memo"] }),
            }]
        );
        // Deleted here, kept there: each answer is honoured.
        let kept = merge(
            Some(&base),
            &local,
            &remote,
            &answer(&[("transaction/t2", "cloud")]),
        )
        .unwrap();
        assert!(kept.conflicts.is_empty());
        assert_eq!(kept.budget.transactions[1].memo, "Birthday present");
        let gone = merge(
            Some(&base),
            &local,
            &remote,
            &answer(&[("transaction/t2", "local")]),
        )
        .unwrap();
        assert_eq!(gone.budget.transactions.len(), 1);
        // A deletion the other side didn't touch just happens.
        let untouched = clean(&base, &local, &base);
        assert_eq!(untouched.transactions.len(), 1);
        let untouched = clean(&base, &base, &local);
        assert_eq!(untouched.transactions.len(), 1);
    }

    #[test]
    fn concurrent_assignments_choose_one_complete_month() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.assignments[0].amount = 30000;
        remote.assignments[0].amount = 25000;
        let merged = merge(Some(&base), &local, &remote, &Choices::new()).unwrap();
        assert_eq!(merged.conflicts.len(), 1);
        assert_eq!(merged.conflicts[0].key, "assignments/2026-10");
        let kept = merge(
            Some(&base),
            &local,
            &remote,
            &answer(&[("assignments/2026-10", "cloud")]),
        )
        .unwrap();
        assert!(kept.conflicts.is_empty());
        assert_eq!(kept.budget.assignments[0].amount, 25000);
        remote.assignments[0].amount = 30000;
        assert_eq!(clean(&base, &local, &remote).assignments[0].amount, 30000);
        remote = base.clone();
        remote.assignments.push(Assignment {
            extra: Default::default(),
            month: "2026-11".into(),
            category_id: "food".into(),
            amount: 500,
        });
        let merged = clean(&base, &local, &remote);
        assert_eq!(
            merged
                .assignments
                .iter()
                .map(|a| a.amount)
                .collect::<Vec<_>>(),
            [30000, 500]
        );
    }

    #[test]
    fn both_renaming_a_category_asks_unless_they_agree() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.categories[0].name = "Groceries".into();
        remote.categories[0].name = "Food & drink".into();
        remote.categories[0].target = 30_000;
        let merged = merge(Some(&base), &local, &remote, &Choices::new()).unwrap();
        assert_eq!(
            merged.conflicts,
            [Conflict {
                key: "category/food/name".into(),
                fingerprint: merged.conflicts[0].fingerprint.clone(),
                kind: "category",
                label: "Food".into(),
                about: json!({ "group": "Living" }),
                field: Some("name".into()),
                local: json!("Groceries"),
                remote: json!("Food & drink"),
            }]
        );
        let chosen = merge(
            Some(&base),
            &local,
            &remote,
            &answer(&[("category/food/name", "cloud")]),
        )
        .unwrap();
        assert!(chosen.conflicts.is_empty());
        assert_eq!(chosen.budget.categories[0].name, "Food & drink");
        // The other side's target change came through either way.
        assert_eq!(chosen.budget.categories[0].target, 30_000);
        // "*" answers everything not answered on its own.
        let all = merge(Some(&base), &local, &remote, &answer(&[("*", "local")])).unwrap();
        assert_eq!(all.budget.categories[0].name, "Groceries");
        // The same new name on both sides isn't a conflict.
        remote.categories[0].name = "Groceries".into();
        assert_eq!(
            clean(&base, &local, &remote).categories[0].name,
            "Groceries"
        );
    }

    #[test]
    fn accounts_categories_and_settings_merge_by_field() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.accounts[0].name = "Wallet".into();
        remote.accounts[0].archived = true;
        remote.accounts.push(Account {
            id: "savings".into(),
            name: "Savings".into(),
            ..base.accounts[0].clone()
        });
        local.categories.retain(|c| c.id != "payment");
        local.transactions.clear();
        remote.extra.insert("dateFormat".into(), json!("dmy"));
        local.name = "Household".into();
        remote.currency = "EUR".into();
        let merged = clean(&base, &local, &remote);
        assert_eq!(
            (
                merged.accounts[0].name.as_str(),
                merged.accounts[0].archived
            ),
            ("Wallet", true)
        );
        assert_eq!(
            merged
                .accounts
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["cash", "card", "savings"]
        );
        assert_eq!(merged.categories.len(), 2);
        assert_eq!(merged.extra["dateFormat"], "dmy");
        assert_eq!(
            (merged.name.as_str(), merged.currency.as_str()),
            ("Household", "EUR")
        );
    }

    #[test]
    fn a_deleted_account_the_other_side_still_uses_is_kept() {
        let mut base = base();
        base.accounts.push(Account {
            id: "savings".into(),
            name: "Savings".into(),
            ..base.accounts[0].clone()
        });
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.accounts.retain(|a| a.id != "savings");
        let mut t = transaction("deposit", 5000);
        t.account_id = "savings".into();
        remote.transactions.push(t);
        let merged = merge(Some(&base), &local, &remote, &Choices::new()).unwrap();
        assert!(merged.conflicts.is_empty());
        assert!(merged.budget.accounts.iter().any(|a| a.id == "savings"));
        assert_eq!(
            merged.notes,
            ["Kept the account “Savings”: one device deleted it while the other still used it."]
        );
    }

    #[test]
    fn card_and_payment_category_references_survive_one_sided_deletion() {
        let base = sample();
        for delete_account in [false, true] {
            let mut local = base.clone();
            if delete_account {
                local.accounts.retain(|a| a.id != "card");
            } else {
                local.categories.retain(|c| c.id != "payment");
            }
            for (left, right) in [(&local, &base), (&base, &local)] {
                let merged = merge(Some(&base), left, right, &Choices::new()).unwrap();
                assert!(merged.conflicts.is_empty());
                assert!(merged.budget.accounts.iter().any(|a| a.id == "card"));
                assert_eq!(
                    merged
                        .budget
                        .categories
                        .iter()
                        .filter(|c| c.payment_account() == Some("card"))
                        .count(),
                    1
                );
                assert_eq!(
                    merged.notes,
                    [if delete_account {
                        "Kept the account “Card”: one device deleted it while the other still used it."
                    } else {
                        "Kept the category “Card payment”: one device deleted it while the other still used it."
                    }]
                );
            }
        }
    }

    #[test]
    fn deleting_a_card_and_its_payment_category_together_is_preserved_unless_still_used() {
        let base = sample();
        let mut deleted = base.clone();
        deleted.accounts.retain(|a| a.id != "card");
        deleted.categories.retain(|c| c.id != "payment");
        let merged = merge(Some(&base), &deleted, &base, &Choices::new()).unwrap();
        assert!(merged.conflicts.is_empty() && merged.notes.is_empty());
        assert!(!merged.budget.accounts.iter().any(|a| a.id == "card"));
        assert!(!merged.budget.categories.iter().any(|c| c.id == "payment"));

        for assignment in [false, true] {
            let mut used = base.clone();
            if assignment {
                used.assignments.push(Assignment {
                    extra: Default::default(),
                    month: "2026-10".into(),
                    category_id: "payment".into(),
                    amount: 5000,
                });
            } else {
                used.transactions.push(purchase());
            }
            let merged = merge(Some(&base), &deleted, &used, &Choices::new()).unwrap();
            assert!(merged.conflicts.is_empty());
            assert!(merged.budget.accounts.iter().any(|a| a.id == "card"));
            assert!(merged
                .budget
                .categories
                .iter()
                .any(|c| c.payment_account() == Some("card")));
            assert_eq!(merged.notes.len(), 2);
        }
    }

    #[test]
    fn extras_lists_merge_by_key() {
        let mut base = base();
        base.extra
            .insert("payees".into(), json!([{ "id": "p1", "name": "Shop" }]));
        base.extra.insert(
            "moneyMoves".into(),
            json!([{ "at": "2026-10-01T10:00:00Z", "month": "2026-10", "description": "a" }]),
        );
        base.extra.insert(
            "ynabMonths".into(),
            json!([{ "month": "2026-10-01", "note": "" }]),
        );
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.extra.insert("payees".into(), json!([{ "id": "p1", "name": "Shop", "categoryId": "food" }, { "id": "p2", "name": "Café" }]));
        remote.extra.insert("payees".into(), json!([{ "id": "p1", "name": "The shop" }, { "id": "p3", "name": "café " }, { "id": "p4", "name": "Bakery" }]));
        local.extra["moneyMoves"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "at": "2026-10-03T10:00:00Z", "month": "2026-11", "description": "c" }));
        remote.extra["moneyMoves"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "at": "2026-10-02T10:00:00Z", "month": "2026-10", "description": "b" }));
        remote.extra.insert(
            "ynabMonths".into(),
            json!([{ "month": "2026-10-01", "note": "Holiday" }]),
        );
        let merged = clean(&base, &local, &remote);
        // Both added "Café": one payee. Different fields of p1 merged.
        assert_eq!(
            merged.extra["payees"],
            json!([{ "id": "p1", "name": "The shop", "categoryId": "food" }, { "id": "p2", "name": "Café" }, { "id": "p4", "name": "Bakery" }])
        );
        let moves: Vec<&str> = merged.extra["moneyMoves"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["description"].as_str().unwrap())
            .collect();
        assert_eq!(moves, ["a", "b", "c"]);
        assert_eq!(merged.extra["ynabMonths"][0]["note"], "Holiday");
    }

    #[test]
    fn a_scheduled_transaction_both_posted_merges_against_what_was_posted() {
        let mut base = base();
        let mut template = purchase();
        template.payee = "Rent".into();
        base.extra.insert(
            "schedules".into(),
            json!([{ "id": "s1", "nextDate": "2026-10-01", "anchorDate": "2026-10-01", "frequency": "monthly", "transaction": template }]),
        );
        let (mut local, mut remote) = (base.clone(), base.clone());
        crate::schedules::apply(&mut local, "2026-10-01", None, false).unwrap();
        crate::schedules::apply(&mut remote, "2026-10-01", None, false).unwrap();
        remote
            .transactions
            .last_mut()
            .unwrap()
            .extra
            .insert("approved".into(), json!(true));
        local.transactions.last_mut().unwrap().memo = "October".into();
        // Each side entered it at its own time: both settle on the earlier, without asking.
        local
            .transactions
            .last_mut()
            .unwrap()
            .set_created_at("2026-10-01T09:00:00.500Z");
        remote
            .transactions
            .last_mut()
            .unwrap()
            .set_created_at("2026-10-01T09:00:00.100Z");
        let merged = clean(&base, &local, &remote);
        let rent = merged.transactions.last().unwrap();
        assert_eq!(
            (rent.memo.as_str(), rent.extra["approved"].clone()),
            ("October", json!(true))
        );
        assert_eq!(merged.transactions.len(), 3);
        assert_eq!(rent.created_at(), Some("2026-10-01T09:00:00.100Z"));
        let other_way = clean(&base, &remote, &local);
        assert_eq!(
            other_way.transactions.last().unwrap().created_at(),
            Some("2026-10-01T09:00:00.100Z")
        );
    }

    /// A base with a monthly schedule due on 1 October, and two copies of it.
    fn with_rent_schedule() -> Budget {
        let mut base = base();
        let mut template = purchase();
        template.payee = "Rent".into();
        base.extra.insert(
            "schedules".into(),
            json!([{ "id": "s1", "nextDate": "2026-10-01", "anchorDate": "2026-10-01", "frequency": "monthly", "transaction": template }]),
        );
        base
    }
    const RENT: &str = "scheduled:s1:2026-10-01";

    #[test]
    fn a_scheduled_transaction_deleted_where_it_was_entered_stays_deleted_when_the_other_side_entered_it_too(
    ) {
        let base = with_rent_schedule();
        // The desktop entered October's rent and the person deleted it there; the phone, offline,
        // entered the same occurrence as it opened.
        let (mut desktop, mut phone) = (base.clone(), base.clone());
        crate::schedules::apply(&mut desktop, "2026-10-02", None, false).unwrap();
        desktop.transactions.retain(|t| t.id != RENT);
        crate::schedules::apply(&mut phone, "2026-10-02", None, false).unwrap();
        assert!(phone.transactions.iter().any(|t| t.id == RENT));
        for merged in [
            clean(&base, &phone, &desktop),
            clean(&base, &desktop, &phone),
        ] {
            assert!(!merged.transactions.iter().any(|t| t.id == RENT));
            assert_eq!(
                crate::schedules::read(&merged).unwrap()[0]
                    .next_date
                    .as_deref(),
                Some("2026-11-01")
            );
        }
    }

    #[test]
    fn a_scheduled_transaction_skipped_on_the_other_side_isnt_entered_by_this_one() {
        let base = with_rent_schedule();
        let (mut desktop, mut phone) = (base.clone(), base.clone());
        crate::schedules::apply(&mut desktop, "2026-10-02", Some("s1"), true).unwrap();
        crate::schedules::apply(&mut phone, "2026-10-02", None, false).unwrap();
        assert!(!desktop.transactions.iter().any(|t| t.id == RENT));
        assert!(!clean(&base, &phone, &desktop)
            .transactions
            .iter()
            .any(|t| t.id == RENT));
    }

    #[test]
    fn a_scheduled_transaction_edited_after_it_was_entered_asks_when_the_other_side_deleted_it() {
        let base = with_rent_schedule();
        let (mut desktop, mut phone) = (base.clone(), base.clone());
        crate::schedules::apply(&mut desktop, "2026-10-02", None, false).unwrap();
        desktop.transactions.retain(|t| t.id != RENT);
        crate::schedules::apply(&mut phone, "2026-10-02", None, false).unwrap();
        phone
            .transactions
            .iter_mut()
            .find(|t| t.id == RENT)
            .unwrap()
            .memo = "Paid in cash".into();
        let merged = merge(Some(&base), &phone, &desktop, &Choices::new()).unwrap();
        assert_eq!(merged.conflicts.len(), 1);
        let asked = &merged.conflicts[0];
        assert_eq!(
            (asked.key.as_str(), asked.field.as_ref()),
            (format!("transaction/{RENT}").as_str(), None)
        );
        assert_eq!(
            (asked.local.clone(), asked.remote.clone()),
            (json!({ "changed": ["memo"] }), Value::Null)
        );
        let kept = merge(Some(&base), &phone, &desktop, &answer(&[("*", "local")]))
            .unwrap()
            .budget;
        assert_eq!(
            kept.transactions
                .iter()
                .find(|t| t.id == RENT)
                .unwrap()
                .memo,
            "Paid in cash"
        );
    }

    #[test]
    fn a_scheduled_transaction_only_this_side_entered_is_kept_while_the_other_side_hasnt_reached_it(
    ) {
        let base = with_rent_schedule();
        let (mut phone, desktop) = (base.clone(), base.clone());
        crate::schedules::apply(&mut phone, "2026-10-02", None, false).unwrap();
        let merged = clean(&base, &phone, &desktop);
        assert!(merged.transactions.iter().any(|t| t.id == RENT));
        assert!(clean(&base, &desktop, &phone)
            .transactions
            .iter()
            .any(|t| t.id == RENT));
    }

    #[test]
    fn without_a_base_nothing_is_deleted_and_differences_ask() {
        let base = base();
        let (mut local, mut remote) = (base.clone(), base.clone());
        local.transactions.retain(|t| t.id != "t1");
        remote.transactions.push(transaction("new", -10));
        remote.transactions[1].memo = "There".into();
        local.transactions[0].memo = "Here".into();
        let merged = merge(None, &local, &remote, &Choices::new()).unwrap();
        assert_eq!(
            merged
                .conflicts
                .iter()
                .map(|c| c.key.as_str())
                .collect::<Vec<_>>(),
            ["transaction/t2/memo"]
        );
        let merged = merge(None, &local, &remote, &answer(&[("*", "cloud")])).unwrap();
        let ids: Vec<&str> = merged
            .budget
            .transactions
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(ids, ["t1", "t2", "new"]);
        assert_eq!(merged.budget.transactions[1].memo, "There");
    }

    #[test]
    fn the_order_of_records_follows_the_side_that_moved_them() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let base = ids(&["a", "b", "c"]);
        assert_eq!(
            order(
                &base,
                &ids(&["a", "b", "c", "d"]),
                &ids(&["c", "a", "x", "b"]),
                false
            ),
            ids(&["c", "d", "a", "x", "b"])
        );
        assert_eq!(
            order(
                &base,
                &ids(&["a", "x", "b", "c"]),
                &ids(&["a", "b", "y", "c"]),
                false
            ),
            ids(&["a", "x", "b", "y", "c"])
        );
        assert_eq!(
            order(
                &base,
                &ids(&["a", "x", "b", "c"]),
                &ids(&["a", "b", "y", "c"]),
                true
            ),
            ids(&["a", "b", "y", "c", "x"])
        );
    }
}
