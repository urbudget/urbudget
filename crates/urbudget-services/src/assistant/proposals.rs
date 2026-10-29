//! A proposal is a change the assistant prepared on a copy of the budget, waiting for the user.
//! It keeps the change itself (so it can run again on the budget as it is when applied) and the
//! patch it made when proposed. Applying runs the change again: if the patch is the same, it is
//! applied; if the budget moved underneath it, the card is refreshed for another look instead.
use super::attachments::{Expected, ImportRow};
use super::tools::Names;
use super::workbook::WorkbookDoc;
use super::{money, new_id};
use crate::domain::*;
use crate::i18n::{comma, list_separator};
use crate::patch::Patch;
use crate::{bank_import, engine, ledger, loans, t, workflows};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    /// None is Ready to Assign.
    pub from: Option<String>,
    pub to: Option<String>,
    pub amount: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    pub id: String,
    /// The signed total, from the transaction's recorded account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    /// The total when proposed: an amount changed meanwhile must not be overwritten.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_amount: Option<i64>,
    /// Some(None) sets Ready to Assign.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared: Option<bool>,
    #[serde(default)]
    pub approve: bool,
    /// Some(None) removes the flag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<Option<String>>,
    #[serde(default)]
    pub unsure: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewTransaction {
    /// Chosen once, so applying later makes the same records the card showed.
    pub id: String,
    pub date: String,
    pub payee: String,
    pub memo: String,
    pub amount: i64,
    pub cleared: bool,
    /// (category, amount, memo); a None category is Ready to Assign.
    pub splits: Vec<(Option<String>, i64, String)>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rename {
    pub from: String,
    pub to: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    #[serde(rename = "match")]
    pub matching: String,
    pub payee: String,
    pub category: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Change {
    Moves { month: String, moves: Vec<Move> },
    AutoAssign { month: String, method: String, categories: Vec<String> },
    Edits { edits: Vec<Edit>, rules: Vec<Rule> },
    #[serde(rename_all = "camelCase")]
    Add { account: String, transactions: Vec<NewTransaction>, from_file: bool },
    Payees { renames: Vec<Rename>, rules: Vec<Rule> },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Proposed,
    Applied,
    Discarded,
    Undone,
    /// The budget changed and the change can't be made any more.
    Failed,
}

/// A converted statement, ready for the import review.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportFile {
    pub account: String,
    pub source: String,
    pub rows: Vec<ImportRow>,
}
impl ImportFile {
    /// The CSV the bank import reads: ISO dates and one signed amount column.
    pub fn csv(&self) -> Result<String, String> {
        let mut w = csv::Writer::from_writer(vec![]);
        // Preserve old files exactly when there are no explicit transfers.
        let transfers = self.rows.iter().any(|r| r.transfer_account_id.is_some());
        let mut headers = vec!["Date", "Payee", "Memo", "Amount"];
        if transfers { headers.push("TransferAccountId"); }
        w.write_record(headers).map_err(|e| e.to_string())?;
        for r in &self.rows {
            let amount = money(r.amount);
            let mut cells = vec![r.date.as_str(), r.payee.as_str(), r.memo.as_str(), amount.as_str()];
            if transfers { cells.push(r.transfer_account_id.as_deref().unwrap_or_default()); }
            w.write_record(cells).map_err(|e| e.to_string())?;
        }
        String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// A signed amount: the change for budget moves, the transaction amount otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    /// "warn" for a badge the user should look at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    #[serde(default)]
    pub indent: bool,
    /// The part of the change the row shows, for the user to leave out or change (see `Revision`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Left out: shown, not applied.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    /// Given another category or amount by the user.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
    /// What the user can change on the row before applying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<RowEdit>,
}

/// What can be changed on a card's row: the category it gives (`category_id`, None being Ready
/// to Assign), and its amount (the row's `amount`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RowEdit {
    #[serde(default)]
    pub category: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<String>,
    #[serde(default)]
    pub amount: bool,
}

/// The user's choices on a card: rows left out and rows given another category or amount, by key.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    #[serde(default)]
    pub skip: Vec<String>,
    #[serde(default)]
    pub set: Vec<RowSet>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RowSet {
    pub key: String,
    /// The category to give: Some(None) is Ready to Assign.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub category: Option<Option<String>>,
    /// The amount, signed as the row shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
}
/// A field that's there, even as null, is Some.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::deserialize(d)?))
}
impl Revision {
    pub fn is_empty(&self) -> bool {
        self.skip.is_empty() && self.set.is_empty()
    }
    fn skips(&self, key: &str) -> bool {
        self.skip.iter().any(|k| k == key)
    }
    fn get(&self, key: &str) -> Option<&RowSet> {
        self.set.iter().find(|s| s.key == key)
    }
}

/// A card the user changed: the assistant's own version, and what the user chose on it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub original: Original,
    pub revision: Revision,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Original {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<Change>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixes: Option<crate::bank_statement::Fixes>,
    pub card: Card,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Highlight {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub transactions: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub kicker: String,
    pub rows: Vec<Row>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<Row>,
    #[serde(default)]
    pub stats: Vec<Stat>,
    /// A check that passed (true) or failed (false), with what it compared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<(bool, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Rows left off the card.
    #[serde(default)]
    pub more: usize,
    #[serde(default)]
    pub highlight: Highlight,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: String,
    pub title: String,
    /// "budget", "transactions", "payees", "import", "report" or "spreadsheet".
    pub kind: String,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<Change>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<ImportFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<ReportDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workbook: Option<WorkbookDoc>,
    /// Fixes for a bank file being imported: the import applies them, not the budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixes: Option<crate::bank_statement::Fixes>,
    /// The spreadsheet file last saved from this card, reused by Open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<String>,
    /// The patch the change made when proposed, without timestamps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    pub card: Card,
    /// What the model is told about the card.
    pub summary: Value,
    /// Why it couldn't be applied, when it can't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// What the user left out or changed on the card, when they did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
}

/// Rows the model is told about; the card shows them all.
const MAX_ROWS: usize = 12;

/// A report the assistant wrote, for the user to preview and save as a PDF.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportDoc {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    /// Markdown: headings, paragraphs, lists and tables.
    pub body: String,
    pub created: String,
}

/// A report card: its sections as rows, ready to preview or save.
pub fn build_report(title: String, period: Option<String>, body: String) -> Proposal {
    let sections: Vec<String> = body
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix('#'))
        .map(|h| h.trim_start_matches('#').trim().trim_end_matches('#').trim().to_owned())
        .filter(|h| !h.is_empty())
        .collect();
    let tables = body.lines().filter(|l| l.trim_start().starts_with("|---") || l.trim_start().starts_with("| ---")).count();
    let mut rows: Vec<Row> = sections.iter().map(|s| Row { label: s.clone(), ..Row::default() }).collect();
    let more = rows.len().saturating_sub(MAX_ROWS);
    rows.truncate(MAX_ROWS);
    let words = body.split_whitespace().count();
    let mut stats = vec![];
    if let Some(p) = &period {
        stats.push(Stat { label: t!("Period"), text: Some(p.clone()), amount: None });
    }
    stats.push(Stat { label: t!("Length"), text: Some(t!("{n} words", n = words)), amount: None });
    if tables > 0 {
        stats.push(Stat { label: t!("Tables"), text: Some(tables.to_string()), amount: None });
    }
    let card = Card {
        kicker: t!("Report"),
        rows,
        stats,
        note: Some(t!("Preview it, then save it as a PDF from the print dialog.")),
        more,
        ..Card::default()
    };
    let summary = json!({ "title": title, "sections": sections, "words": words });
    Proposal {
        id: new_id("p"),
        title: title.clone(),
        kind: "report".into(),
        status: Status::Proposed,
        change: None,
        import: None,
        report: Some(ReportDoc { title, period, body, created: super::now() }),
        workbook: None,
        saved: None,
        expected: None,
        card,
        summary,
        problem: None,
        fixes: None,
        review: None,
    }
}

impl Change {
    pub fn kind(&self) -> &'static str {
        match self {
            Change::Moves { .. } | Change::AutoAssign { .. } => "budget",
            Change::Edits { .. } | Change::Add { .. } => "transactions",
            Change::Payees { .. } => "payees",
        }
    }
    /// The budget after the change, or why it can't be made.
    pub fn run(&self, mut b: Budget) -> Result<Budget, String> {
        match self {
            Change::Moves { month, moves } => {
                for m in moves {
                    let source = Names::new(&b).category_shown(m.from.as_deref());
                    b = workflows::apply(
                        b,
                        &json!({
                            "type": "moveMoney",
                            "month": month,
                            "from": m.from.as_deref().unwrap_or("__rta"),
                            "to": m.to.as_deref().unwrap_or("__rta"),
                            "amount": m.amount,
                        }),
                    )
                    .map_err(|e| t!("Moving {amount} from {source}: {problem}", amount = money(m.amount), source, problem = e))?;
                }
            }
            Change::AutoAssign { month, method, categories } => {
                b = workflows::apply(
                    b,
                    &json!({ "type": "autoAssign", "month": month, "mode": method, "categories": categories }),
                )?;
            }
            Change::Edits { edits, rules } => {
                for e in edits {
                    let t = b
                        .transactions
                        .iter_mut()
                        .find(|t| t.id == e.id)
                        .ok_or_else(|| t!("A transaction in this proposal was deleted."))?;
                    if let Some(amount) = e.amount {
                        if amount == 0 {
                            return Err(t!("Enter an amount other than zero."));
                        }
                        if t.splits.len() > 1 || t.splits.iter().any(|s| ledger::transfer(s).is_some()) {
                            return Err(t!("{payee} on {date} has split amounts. Edit its splits in the register so they add up to the new total.", payee = t.payee, date = t.date));
                        }
                        if e.previous_amount.is_some_and(|was| t.amount != was && t.amount != amount) {
                            return Err(t!("{payee} on {date} has a different amount now. Ask for a new proposal using its current amount.", payee = t.payee, date = t.date));
                        }
                        t.amount = amount;
                        // Whole transfers have no category within the budget, and one when
                        // crossing its edge. A normal transaction has its one category split.
                        if let Some(split) = t.splits.first_mut() {
                            split.amount = amount;
                        }
                    }
                    if let Some(category) = &e.category {
                        if t.transfer_account_id.is_some() || t.splits.len() != 1 || ledger::transfer(&t.splits[0]).is_some() {
                            return Err(t!("{payee} on {date} is now a split or a transfer.", payee = t.payee, date = t.date));
                        }
                        t.splits[0].category_id = category.clone();
                    }
                    if let Some(p) = &e.payee {
                        t.payee = p.clone();
                    }
                    if let Some(m) = &e.memo {
                        t.memo = m.clone();
                    }
                    if let Some(c) = e.cleared {
                        if t.status != Cleared::Reconciled {
                            t.status = if c { Cleared::Cleared } else { Cleared::Uncleared };
                        }
                    }
                    if e.approve {
                        t.extra.insert("approved".into(), json!(true));
                    }
                    if let Some(flag) = &e.flag {
                        t.extra.insert("flag".into(), json!(flag));
                    }
                }
                add_rules(&mut b, rules);
            }
            Change::Add { account, transactions, .. } => {
                let mut ids = HashSet::new();
                let now = stamp_now();
                for n in transactions {
                    let mut t = Transaction {
                        extra: serde_json::Map::from_iter([
                            ("approved".to_string(), json!(true)),
                            ("createdAt".to_string(), json!(now)),
                        ])
                        .into(),
                        id: n.id.clone(),
                        date: n.date.clone(),
                        account_id: account.clone(),
                        payee: n.payee.clone(),
                        memo: n.memo.clone(),
                        amount: n.amount,
                        status: if n.cleared { Cleared::Cleared } else { Cleared::Uncleared },
                        splits: n
                            .splits
                            .iter()
                            .map(|(category, amount, memo)| Split {
                                extra: if memo.is_empty() {
                                    Default::default()
                                } else {
                                    serde_json::Map::from_iter([("memo".to_string(), json!(memo))]).into()
                                },
                                category_id: category.clone(),
                                amount: *amount,
                            })
                            .collect(),
                        transfer_account_id: None,
                        transfer_status: None,
                    };
                    workflows::apply_payee_rules(&b, &mut t);
                    ids.insert(t.id.clone());
                    b.transactions.push(t);
                }
                loans::apply_new_payments(&mut b, &ids)?;
            }
            Change::Payees { renames, rules } => {
                for r in renames {
                    b = workflows::apply(b, &json!({ "type": "renamePayee", "from": r.from, "to": r.to }))?;
                }
                add_rules(&mut b, rules);
            }
        }
        validate(&b)?;
        Ok(b)
    }
}

fn add_rules(b: &mut Budget, rules: &[Rule]) {
    if rules.is_empty() {
        return;
    }
    let list = b.extra.entry("payeeRules").or_insert(json!([]));
    if !list.is_array() {
        *list = json!([]);
    }
    if let Some(list) = list.as_array_mut() {
        for r in rules {
            list.push(json!({ "id": r.id, "match": r.matching, "payee": r.payee, "categoryId": r.category }));
        }
    }
}

/// The patch as JSON, without the timestamps budget moves log or the times new transactions
/// were added, for comparing two runs.
pub fn normalize(patch: &Patch) -> Value {
    let mut v = serde_json::to_value(patch).unwrap_or(Value::Null);
    if let Some(set) = v.pointer_mut("/extras/set").and_then(Value::as_object_mut) {
        set.remove("moneyMoves");
    }
    if let Some(upsert) = v.pointer_mut("/transactions/upsert").and_then(Value::as_array_mut) {
        for t in upsert.iter_mut().filter_map(Value::as_object_mut) {
            t.remove("createdAt");
        }
    }
    v
}

pub fn build(change: Change, title: String, before: &Budget) -> Result<Proposal, String> {
    let after = change.run(before.clone())?;
    let patch = Patch::diff(before, &after);
    if patch.is_empty() {
        return Err(t!("That wouldn't change anything."));
    }
    let card = card(&change, before, &after)?;
    let summary = summary(&title, &card);
    Ok(Proposal {
        id: new_id("p"),
        title,
        kind: change.kind().into(),
        status: Status::Proposed,
        expected: Some(normalize(&patch)),
        import: None,
        report: None,
        workbook: None,
        saved: None,
        change: Some(change),
        card,
        summary,
        problem: None,
        fixes: None,
        review: None,
    })
}

pub enum Recheck {
    /// Still the same change: apply this patch.
    Same(Patch),
    /// The budget moved; the proposal now looks like this.
    Changed(Proposal),
}

/// Runs a proposal again on the budget as it is now.
pub fn recheck(p: &Proposal, current: &Budget) -> Recheck {
    let mut refreshed = p.clone();
    if p.fixes.is_some() {
        refreshed.problem = Some(t!("Apply these from the import they're for."));
        return Recheck::Changed(refreshed);
    }
    let Some(change) = &p.change else {
        refreshed.status = Status::Failed;
        refreshed.problem = Some(if p.report.is_some() { t!("A report doesn't change the budget.") } else { t!("Open the import review to add these.") });
        return Recheck::Changed(refreshed);
    };
    // Every row left out by the user: there's nothing to apply until one is chosen again.
    if change.is_empty() && p.review.is_some() {
        refreshed.problem = Some(t!("Choose at least one change to apply."));
        return Recheck::Changed(refreshed);
    }
    match change.run(current.clone()) {
        Ok(after) => {
            let patch = Patch::diff(current, &after);
            let normalized = normalize(&patch);
            if p.expected.as_ref() == Some(&normalized) {
                return Recheck::Same(patch);
            }
            if patch.is_empty() {
                refreshed.status = Status::Failed;
                refreshed.problem = Some(t!("Your budget already has these changes."));
                return Recheck::Changed(refreshed);
            }
            // The card again, with what the user chose on it.
            let again = match &p.review {
                Some(r) => revise(p, r.revision.clone(), current),
                None => card(change, current, &after).map(|c| {
                    let mut q = p.clone();
                    q.summary = summary(&q.title, &c);
                    q.card = c;
                    q.expected = Some(normalized);
                    q
                }),
            };
            match again {
                Ok(q) => {
                    refreshed = q;
                    refreshed.problem = Some(t!("Your budget changed since this was proposed, so it was updated. Check it, then apply."));
                }
                Err(e) => {
                    refreshed.status = Status::Failed;
                    refreshed.problem = Some(e);
                }
            }
            Recheck::Changed(refreshed)
        }
        Err(e) => {
            refreshed.status = Status::Failed;
            refreshed.problem = Some(t!("This can't be applied any more: {problem}", problem = e));
            Recheck::Changed(refreshed)
        }
    }
}

/// The proposal as the user chose on its card (rows left out, categories and amounts given),
/// made on the budget as it is now. Every choice is made on the assistant's own version, so a row
/// left out can be chosen again; an empty revision puts that version back.
pub fn revise(p: &Proposal, revision: Revision, current: &Budget) -> Result<Proposal, String> {
    if !matches!(p.status, Status::Proposed | Status::Undone) {
        return Err(t!("This proposal was already handled."));
    }
    let original = match &p.review {
        Some(r) => r.original.clone(),
        None => Original { change: p.change.clone(), fixes: p.fixes.clone(), card: p.card.clone() },
    };
    let mut out = p.clone();
    out.problem = None;
    if let Some(fixes) = &original.fixes {
        let revised = revised_fixes(fixes, &revision)?;
        let mut card = original.card.clone();
        for row in &mut card.rows {
            let Some(key) = row.key.clone() else { continue };
            row.skipped = revision.skips(&key);
            let given = revision.get(&key).and_then(|s| s.amount);
            if let (Some(amount), Some(fix)) = (given, revised.fields.iter().find(|x| fix_key(&x.line, x.field) == key)) {
                row.label = fix_label(&fix.line, fix.field, &fix.from, &fix.to);
                row.amount = Some(amount);
                row.edited = true;
            }
        }
        out.fixes = Some(revised);
        out.card = card;
    } else if let Some(change) = &original.change {
        let all = change.run(current.clone())?;
        let full = card(change, current, &all)?;
        let revised = change.revised(&revision, &full)?;
        let mut part = None;
        out.expected = None;
        if !revised.is_empty() {
            let after = revised.run(current.clone())?;
            let patch = Patch::diff(current, &after);
            if !patch.is_empty() {
                part = Some(card_as(&revised, current, &after, change.single())?);
                out.expected = Some(normalize(&patch));
            }
        }
        if let (Change::Add { transactions, .. }, Some(part)) = (change, part.as_mut()) {
            // One transaction's splits are named by their place: the place each had before others were left out.
            if transactions.len() == 1 {
                let kept: Vec<usize> = (0..transactions[0].splits.len()).filter(|i| !revision.skips(&format!("s:{i}"))).collect();
                for row in &mut part.rows {
                    if let Some(i) = row.key.as_deref().and_then(|k| k.strip_prefix("s:")).and_then(|i| i.parse::<usize>().ok()) {
                        row.key = kept.get(i).map(|k| format!("s:{k}"));
                    }
                }
            }
        }
        out.card = merge(full, part, &revision);
        out.change = Some(revised);
    } else {
        return Err(t!("Nothing on this card can be left out or changed."));
    }
    out.review = (!revision.is_empty()).then(|| Review { original, revision });
    out.summary = summary(&out.title, &out.card);
    Ok(out)
}

/// The card of the whole change, each row as the part the user chose shows it (its amounts and
/// balances as they'll be), the rest left out.
fn merge(full: Card, part: Option<Card>, r: &Revision) -> Card {
    let Some(part) = part else {
        let mut card = Card { total: None, warning: None, ..full };
        for row in card.rows.iter_mut().filter(|row| row.key.is_some()) {
            row.skipped = true;
        }
        return card;
    };
    let mut rows = vec![];
    for row in &full.rows {
        let key = row.key.as_deref();
        rows.push(match key.and_then(|k| part.rows.iter().find(|p| p.key.as_deref() == Some(k))) {
            Some(shown) => {
                // What can be changed is the whole change's; the category is the one it gives now, the user's.
                let edit = row.edit.clone().map(|e| match &shown.edit {
                    Some(now) => RowEdit { category_id: now.category_id.clone(), ..e },
                    None => e,
                });
                Row { edit, edited: key.is_some_and(|k| r.get(k).is_some()), ..shown.clone() }
            }
            None => Row { skipped: key.is_some(), ..row.clone() },
        });
    }
    Card { rows, ..part }
}

impl Change {
    /// Nothing left to do: every part of it left out.
    pub fn is_empty(&self) -> bool {
        match self {
            Change::Moves { moves, .. } => moves.is_empty(),
            Change::AutoAssign { categories, .. } => categories.is_empty(),
            Change::Edits { edits, rules } => edits.is_empty() && rules.is_empty(),
            Change::Add { transactions, .. } => transactions.is_empty(),
            Change::Payees { renames, rules } => renames.is_empty() && rules.is_empty(),
        }
    }
    /// One new transaction, shown by its splits.
    fn single(&self) -> Option<bool> {
        match self {
            Change::Add { transactions, .. } => Some(transactions.len() == 1),
            _ => None,
        }
    }
    /// The change as the user chose on its card (`full`, the card of all of it).
    fn revised(&self, r: &Revision, full: &Card) -> Result<Change, String> {
        let label = |key: &str| full.rows.iter().find(|row| row.key.as_deref() == Some(key)).map_or_else(String::new, |row| row.label.clone());
        Ok(match self {
            Change::Moves { month, moves } => {
                // A category left out keeps what it has: the moves to it and from it are left out.
                let left = |c: &Option<String>| c.as_ref().is_some_and(|c| r.skips(&format!("c:{c}")));
                let mut kept: Vec<Move> = moves.iter().filter(|m| !left(&m.from) && !left(&m.to)).cloned().collect();
                for s in &r.set {
                    let (Some(id), Some(amount)) = (s.key.strip_prefix("c:"), s.amount) else { continue };
                    let mine: Vec<usize> = kept.iter().enumerate().filter(|(_, m)| touches(m, id)).map(|(i, _)| i).collect();
                    let [i] = mine[..] else {
                        if mine.is_empty() {
                            continue;
                        }
                        return Err(t!("{label} gets money from more than one place: leave it out, or ask for another amount.", label = label(&s.key)));
                    };
                    // The amount the category changes by: more is given to it, less is taken from it.
                    let other = if kept[i].to.as_deref() == Some(id) { kept[i].from.clone() } else { kept[i].to.clone() };
                    match amount.cmp(&0) {
                        Ordering::Greater => kept[i] = Move { from: other, to: Some(id.into()), amount },
                        Ordering::Less => kept[i] = Move { from: Some(id.into()), to: other, amount: -amount },
                        Ordering::Equal => {
                            kept.remove(i);
                        }
                    }
                }
                Change::Moves { month: month.clone(), moves: kept }
            }
            Change::AutoAssign { month, .. } if !r.is_empty() => {
                // Chosen from: what it assigns, as moves from Ready to Assign, with the user's amounts.
                let moves = full
                    .rows
                    .iter()
                    .filter_map(|row| {
                        let key = row.key.as_deref()?;
                        let id = key.strip_prefix("c:")?;
                        if r.skips(key) {
                            return None;
                        }
                        let amount = r.get(key).and_then(|s| s.amount).or(row.amount)?;
                        match amount.cmp(&0) {
                            Ordering::Greater => Some(Move { from: None, to: Some(id.into()), amount }),
                            Ordering::Less => Some(Move { from: Some(id.into()), to: None, amount: -amount }),
                            Ordering::Equal => None,
                        }
                    })
                    .collect();
                Change::Moves { month: month.clone(), moves }
            }
            Change::AutoAssign { .. } => self.clone(),
            Change::Edits { edits, rules } => {
                for given in &r.set {
                    if let Some(edit) = given.key.strip_prefix("t:").and_then(|id| edits.iter().find(|e| e.id == id)) {
                        if given.amount.is_some() && edit.amount.is_none() {
                            return Err(t!("This card doesn't propose an amount change for {label}. Ask for a new proposal.", label = label(&given.key)));
                        }
                    }
                }
                Change::Edits {
                    edits: edits
                        .iter()
                        .filter(|e| !r.skips(&format!("t:{}", e.id)))
                        .map(|e| {
                            let mut e = e.clone();
                            if let Some(category) = r.get(&format!("t:{}", e.id)).and_then(|s| s.category.clone()).filter(|_| e.category.is_some()) {
                                e.category = Some(category);
                                // The user chose it: nothing to check any more.
                                e.unsure = false;
                            }
                            if let Some(amount) = r.get(&format!("t:{}", e.id)).and_then(|s| s.amount).filter(|_| e.amount.is_some()) {
                                e.amount = Some(amount);
                                e.unsure = false;
                            }
                            e
                        })
                        .collect(),
                    rules: rules.iter().filter(|x| !r.skips(&format!("r:{}", x.id))).cloned().collect(),
                }
            },
            Change::Add { account, transactions, from_file } => {
                let zero = |key: &str| t!("{label}'s amount can't be zero: leave it out instead.", label = label(key));
                let mut chosen = vec![];
                if let [t] = &transactions[..] {
                    let mut t = t.clone();
                    let mut splits = vec![];
                    for (i, (category, amount, memo)) in t.splits.iter().enumerate() {
                        let key = format!("s:{i}");
                        if r.skips(&key) {
                            continue;
                        }
                        let s = r.get(&key);
                        let amount = s.and_then(|s| s.amount).unwrap_or(*amount);
                        if amount == 0 {
                            return Err(zero(&key));
                        }
                        splits.push((s.and_then(|s| s.category.clone()).unwrap_or_else(|| category.clone()), amount, memo.clone()));
                    }
                    if !splits.is_empty() {
                        t.amount = splits.iter().map(|s| s.1).sum();
                        t.splits = splits;
                        chosen.push(t);
                    }
                } else {
                    for t in transactions {
                        let key = format!("t:{}", t.id);
                        if r.skips(&key) {
                            continue;
                        }
                        let mut t = t.clone();
                        if let Some(s) = r.get(&key) {
                            if let Some(amount) = s.amount {
                                if amount == 0 {
                                    return Err(zero(&key));
                                }
                                if let [split] = &mut t.splits[..] {
                                    split.1 = amount;
                                } else if amount != t.amount {
                                    return Err(t!("{label} is split: leave it out, or ask for another amount.", label = label(&key)));
                                }
                                t.amount = amount;
                            }
                            if let (Some(category), [split]) = (&s.category, &mut t.splits[..]) {
                                split.0 = category.clone();
                            }
                        }
                        chosen.push(t);
                    }
                }
                Change::Add { account: account.clone(), transactions: chosen, from_file: *from_file }
            }
            Change::Payees { renames, rules } => Change::Payees {
                renames: renames.iter().filter(|x| !r.skips(&format!("p:{}", x.from))).cloned().collect(),
                rules: rules.iter().filter(|x| !r.skips(&format!("r:{}", x.id))).cloned().collect(),
            },
        })
    }
}

/// Fixes for a bank file as the user chose on their card: fixes left out, amounts given.
fn revised_fixes(f: &crate::bank_statement::Fixes, r: &Revision) -> Result<crate::bank_statement::Fixes, String> {
    use crate::bank_statement::Field;
    let mut out = f.clone();
    out.fields.retain(|x| !r.skips(&fix_key(&x.line, x.field)));
    for x in &mut out.fields {
        let Some(amount) = r.get(&fix_key(&x.line, x.field)).and_then(|s| s.amount).filter(|_| x.field == Field::Amount) else { continue };
        if amount == 0 {
            return Err(t!("Line {line}'s amount can't be zero: leave the fix out instead.", line = x.line));
        }
        x.to = money(amount);
    }
    out.include.retain(|x| !r.skips(&format!("i:{}", x.line)));
    if r.skips("x") {
        out.structure = None;
    }
    Ok(out)
}

/// A fix to a line's field: its row's key on the card, and its label.
pub fn fix_key(line: &str, field: crate::bank_statement::Field) -> String {
    format!("f:{line}:{}", field_name(field))
}
/// The card names the field in the app's language; `field_name` stays the key's (and the model's).
pub fn fix_label(line: &str, field: crate::bank_statement::Field, from: &str, to: &str) -> String {
    use crate::bank_statement::Field;
    let from = if from.is_empty() { t!("empty") } else { from.to_owned() };
    match field {
        Field::Date => t!("Line {line} · date: {from} \u{2192} {to}", line, from, to),
        Field::Payee => t!("Line {line} · payee: {from} \u{2192} {to}", line, from, to),
        Field::Memo => t!("Line {line} · memo: {from} \u{2192} {to}", line, from, to),
        Field::Amount => t!("Line {line} · amount: {from} \u{2192} {to}", line, from, to),
        Field::TransferAccountId => t!("Line {line} · transfer account: {from} \u{2192} {to}", line, from, to),
    }
}
pub fn field_name(field: crate::bank_statement::Field) -> &'static str {
    use crate::bank_statement::Field;
    match field {
        Field::Date => "date",
        Field::Payee => "payee",
        Field::Memo => "memo",
        Field::Amount => "amount",
        Field::TransferAccountId => "transferAccountId",
    }
}

impl Proposal {
    /// What the user changed on the card, for the assistant: "2 of 5 left out, 1 changed by the user".
    pub fn choices(&self) -> Option<String> {
        self.review.as_ref()?;
        let rows = self.card.rows.iter().filter(|r| r.key.is_some()).count();
        let left = self.card.rows.iter().filter(|r| r.skipped).count();
        let changed = self.card.rows.iter().filter(|r| r.edited).count();
        let mut parts = vec![];
        if left > 0 {
            parts.push(format!("{left} of {rows} left out"));
        }
        if changed > 0 {
            parts.push(format!("{changed} changed"));
        }
        (!parts.is_empty()).then(|| format!("{} by the user", parts.join(", ")))
    }
}

fn summary(title: &str, card: &Card) -> Value {
    let row = |r: &Row| {
        let mut v = json!({ "label": r.label });
        if let Some(d) = &r.detail {
            v["detail"] = json!(d);
        }
        if let Some(a) = r.amount {
            v["amount"] = json!(money(a));
        }
        if let (Some(b), Some(a)) = (r.before, r.after) {
            v["before"] = json!(money(b));
            v["after"] = json!(money(a));
        }
        if let Some(b) = &r.badge {
            v["badge"] = json!(b);
        }
        if r.skipped {
            v["leftOutByUser"] = json!(true);
        }
        if r.edited {
            v["changedByUser"] = json!(true);
        }
        v
    };
    json!({
        "title": title,
        "rows": card.rows.iter().take(MAX_ROWS).map(row).collect::<Vec<_>>(),
        "total": card.total.as_ref().map(row),
        "rowsNotShown": card.rows.len().saturating_sub(MAX_ROWS) + card.more,
        "stats": card.stats.iter().map(|s| json!({ "label": s.label, "value": s.text.clone().or(s.amount.map(money)) })).collect::<Vec<_>>(),
        "check": card.check.as_ref().map(|(ok, text)| json!({ "passed": ok, "text": text })),
        "warning": card.warning,
        "note": card.note,
    })
}

fn card(change: &Change, before: &Budget, after: &Budget) -> Result<Card, String> {
    card_as(change, before, after, change.single())
}

/// A change's card; `single` (for new transactions) shows the one transaction by its splits.
fn card_as(change: &Change, before: &Budget, after: &Budget, single: Option<bool>) -> Result<Card, String> {
    let names = Names::new(after);
    match change {
        Change::Moves { month, .. } | Change::AutoAssign { month, .. } => {
            let (sb, sa) = (engine::calculate(before, month)?, engine::calculate(after, month)?);
            let mut rows = vec![];
            for c in &after.categories {
                let (Some(b), Some(a)) = (
                    sb.categories.iter().find(|s| s.id == c.id),
                    sa.categories.iter().find(|s| s.id == c.id),
                ) else {
                    continue;
                };
                if a.assigned != b.assigned {
                    // A category's amount can be changed where one move gives it (or takes from it).
                    let amount = match change {
                        Change::Moves { moves, .. } => moves.iter().filter(|m| touches(m, &c.id)).count() == 1,
                        _ => true,
                    };
                    rows.push(Row {
                        label: c.name.clone(),
                        amount: Some(a.assigned - b.assigned),
                        before: Some(b.available),
                        after: Some(a.available),
                        key: Some(format!("c:{}", c.id)),
                        edit: amount.then(|| RowEdit { amount: true, ..RowEdit::default() }),
                        ..Row::default()
                    });
                }
            }
            if rows.is_empty() {
                return Err(t!("That wouldn't change any category."));
            }
            let highlight = Highlight {
                month: Some(month.clone()),
                categories: after
                    .categories
                    .iter()
                    .filter(|c| rows.iter().any(|r| r.label == c.name))
                    .map(|c| c.id.clone())
                    .collect(),
                transactions: vec![],
            };
            Ok(Card {
                kicker: t!("Proposal · Budget"),
                rows,
                total: Some(Row {
                    label: t!("Ready to assign"),
                    amount: Some(sa.ready_to_assign - sb.ready_to_assign),
                    before: Some(sb.ready_to_assign),
                    after: Some(sa.ready_to_assign),
                    ..Row::default()
                }),
                warning: (sa.ready_to_assign < 0 && sb.ready_to_assign >= 0)
                    .then(|| t!("This would assign more than you have ready to assign.")),
                highlight,
                ..Card::default()
            })
        }
        Change::Edits { edits, rules } => {
            let by_id: std::collections::HashMap<&str, &Transaction> =
                before.transactions.iter().map(|t| (t.id.as_str(), t)).collect();
            let was = Names::new(before);
            let mut rows = vec![];
            for e in edits {
                let Some(old) = by_id.get(e.id.as_str()) else { continue };
                let new = after.transactions.iter().find(|t| t.id == e.id).unwrap_or(old);
                // Each change as it was and as it will be.
                let mut parts = vec![];
                if e.category.is_some() {
                    let (from, to) = (was.category_label_shown(old), names.category_label_shown(new));
                    parts.push(if from == to || from.is_empty() { to } else { t!("{from} \u{2192} {to}", from, to) });
                }
                if e.payee.is_some() && new.payee != old.payee {
                    parts.push(t!("Payee: {from} \u{2192} {to}", from = old.payee, to = new.payee));
                }
                if e.memo.is_some() {
                    parts.push(if new.memo.is_empty() {
                        t!("Clear memo")
                    } else if old.memo.is_empty() {
                        t!("Memo: {memo}", memo = super::tools::truncate(&new.memo, 30))
                    } else {
                        t!("Memo: {from} \u{2192} {to}", from = super::tools::truncate(&old.memo, 20), to = super::tools::truncate(&new.memo, 30))
                    });
                }
                if let Some(c) = e.cleared {
                    parts.push(if c { t!("Uncleared \u{2192} Cleared") } else { t!("Cleared \u{2192} Uncleared") });
                }
                if e.approve {
                    parts.push(t!("Approve"));
                }
                if let Some(f) = &e.flag {
                    parts.push(f.as_ref().map_or_else(|| t!("Remove flag"), |f| flag_label(f)));
                }
                let rule = rules.iter().any(|r| new.payee.to_lowercase().contains(&r.matching.to_lowercase()));
                rows.push(Row {
                    label: format!("{} · {}", new.payee, short_date(&new.date)),
                    detail: Some(parts.join(" · ")),
                    amount: Some(new.amount),
                    before: e.amount.map(|_| old.amount),
                    after: e.amount.map(|_| new.amount),
                    badge: if e.unsure {
                        Some(t!("Check this"))
                    } else {
                        rule.then(|| t!("+ rule"))
                    },
                    tone: e.unsure.then(|| "warn".into()),
                    key: Some(format!("t:{}", e.id)),
                    edit: (e.category.is_some() || e.amount.is_some()).then(|| RowEdit {
                        category: e.category.is_some(),
                        category_id: new.splits.first().and_then(|s| s.category_id.clone()),
                        amount: e.amount.is_some(),
                    }),
                    ..Row::default()
                });
            }
            // Rules are rows of their own, to leave out apart from the edits they'd have made.
            rows.extend(rules.iter().map(|r| rule_row(r, &names)));
            let unsure = edits.iter().filter(|e| e.unsure).count();
            Ok(Card {
                kicker: t!("Proposal · Transactions"),
                rows,
                note: (unsure > 0).then(|| t!("{n|# is|# are} marked to check.", n = unsure)),
                highlight: Highlight { transactions: edits.iter().map(|e| e.id.clone()).collect(), ..Highlight::default() },
                ..Card::default()
            })
        }
        Change::Add { account, transactions, from_file } => {
            let mut rows = vec![];
            for n in transactions {
                let t = after.transactions.iter().find(|t| t.id == n.id).ok_or_else(|| t!("Missing new transaction"))?;
                if single.unwrap_or(transactions.len() == 1) {
                    for (i, s) in t.splits.iter().enumerate() {
                        rows.push(Row {
                            label: names.category_shown(s.category_id.as_deref()),
                            detail: s.extra.get("memo").and_then(Value::as_str).map(str::to_owned),
                            amount: Some(s.amount),
                            key: Some(format!("s:{i}")),
                            edit: Some(RowEdit { category: true, category_id: s.category_id.clone(), amount: true }),
                            ..Row::default()
                        });
                    }
                } else {
                    rows.push(Row {
                        label: format!("{} · {}", t.payee, short_date(&t.date)),
                        detail: Some(names.category_label_shown(t)),
                        amount: Some(t.amount),
                        key: Some(format!("t:{}", n.id)),
                        edit: Some(RowEdit {
                            category: t.splits.len() == 1,
                            category_id: (t.splits.len() == 1).then(|| t.splits[0].category_id.clone()).flatten(),
                            amount: true,
                        }),
                        ..Row::default()
                    });
                }
            }
            let mut stats = vec![];
            if let (Some(true), [t]) = (single, &transactions[..]) {
                stats.push(Stat { label: t!("Payee"), text: Some(t.payee.clone()), amount: None });
                stats.push(Stat { label: t!("Date"), text: Some(short_date(&t.date)), amount: None });
                stats.push(Stat { label: t!("Amount"), text: None, amount: Some(t.amount) });
            }
            Ok(Card {
                kicker: t!("Proposal · {account}", account = names.account(account)),
                rows,
                stats,
                warning: from_file.then(|| t!("Read from a file. Check the amounts.")),
                ..Card::default()
            })
        }
        Change::Payees { renames, rules } => {
            let mut rows = vec![];
            for r in renames {
                let count = before.transactions.iter().filter(|t| t.payee == r.from).count();
                rows.push(Row {
                    label: t!("{from} → {to}", from = r.from, to = r.to),
                    detail: Some(t!("{n|# transaction|# transactions}", n = count)),
                    key: Some(format!("p:{}", r.from)),
                    ..Row::default()
                });
            }
            for r in rules {
                rows.push(rule_row(r, &names));
            }
            Ok(Card { kicker: t!("Proposal · Payees"), rows, ..Card::default() })
        }
    }
}

fn touches(m: &Move, category: &str) -> bool {
    m.from.as_deref() == Some(category) || m.to.as_deref() == Some(category)
}

fn rule_row(r: &Rule, names: &Names) -> Row {
    let mut parts = vec![];
    if !r.payee.is_empty() {
        parts.push(t!("rename to {payee}", payee = r.payee));
    }
    if r.category.is_some() {
        parts.push(names.category_shown(r.category.as_deref()));
    }
    Row {
        label: t!("New rule: payees containing \u{201c}{text}\u{201d}", text = r.matching),
        detail: (!parts.is_empty()).then(|| parts.join(" · ")),
        badge: Some(t!("rule")),
        key: Some(format!("r:{}", r.id)),
        ..Row::default()
    }
}
/// A flag as a card names it: "Red flag".
fn flag_label(color: &str) -> String {
    match color {
        "red" => t!("Red flag"),
        "orange" => t!("Orange flag"),
        "yellow" => t!("Yellow flag"),
        "green" => t!("Green flag"),
        "blue" => t!("Blue flag"),
        "purple" => t!("Purple flag"),
        other => t!("{color} flag", color = capitalize(other)),
    }
}
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}
fn short_date(date: &str) -> String {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or_else(|_| date.to_owned(), crate::i18n::day_month)
}

/// A proposal to open the import review with converted rows.
#[allow(clippy::too_many_arguments)]
pub fn build_import(
    account: &str,
    account_name: &str,
    source: &str,
    rows: Vec<ImportRow>,
    expected: Expected,
    skipped: Vec<String>,
    mapped: bool,
    budget: &Budget,
) -> Result<Proposal, String> {
    let file = ImportFile { account: account.into(), source: source.into(), rows };
    let outflow: i64 = file.rows.iter().filter(|r| r.amount < 0).map(|r| -r.amount).sum();
    let inflow: i64 = file.rows.iter().filter(|r| r.amount > 0).map(|r| r.amount).sum();
    let first = file.rows.iter().map(|r| r.date.as_str()).min().unwrap_or_default();
    let last = file.rows.iter().map(|r| r.date.as_str()).max().unwrap_or_default();
    let preview = bank_import::preview(budget, account, &file.csv()?, "csv", "ymd", false)?;
    let duplicates = preview.iter().filter(|r| r.duplicate).count();
    let matches = preview.iter().filter(|r| !r.duplicate && !r.candidates.is_empty()).count();
    let mut problems = vec![];
    if let Some(n) = expected.count.filter(|n| *n != file.rows.len()) {
        problems.push(t!("{expected} rows expected, {found} found", expected = n, found = file.rows.len()));
    }
    if let Some(x) = expected.outflow.filter(|x| *x != outflow) {
        problems.push(t!("money out {expected} expected, {found} found", expected = money(x), found = money(outflow)));
    }
    if let Some(x) = expected.inflow.filter(|x| *x != inflow) {
        problems.push(t!("money in {expected} expected, {found} found", expected = money(x), found = money(inflow)));
    }
    let checked = expected.count.is_some() || expected.outflow.is_some() || expected.inflow.is_some();
    let check = if checked {
        Some(if problems.is_empty() {
            (true, t!("Totals match the statement"))
        } else {
            (false, t!("Doesn't match the statement: {problems}", problems = problems.join(list_separator())))
        })
    } else if mapped {
        Some((true, t!("Every row converted from the sheet's own columns")))
    } else {
        None
    };
    let mut notes = vec![];
    if duplicates + matches > 0 {
        let mut parts = vec![];
        if duplicates > 0 {
            parts.push(t!("{n|# looks like a duplicate|# look like duplicates}", n = duplicates));
        }
        if matches > 0 {
            parts.push(t!("{n|# matches an existing entry|# match existing entries}", n = matches));
        }
        notes.push(t!("In the review: {parts}.", parts = parts.join(comma())));
    } else {
        notes.push(t!("The import review shows each row before anything is added."));
    }
    if !skipped.is_empty() {
        let shown: Vec<_> = skipped.iter().take(4).cloned().collect();
        let rows = format!("{}{}", shown.join(list_separator()), if skipped.len() > 4 { "…" } else { "" });
        notes.push(t!("Left out {n|# row|# rows}: {rows}.", n = skipped.len(), rows));
    }
    let card = Card {
        kicker: t!("Import file · {account}", account = account_name),
        rows: vec![],
        stats: vec![
            Stat { label: t!("Dates"), text: Some(format!("{} – {}", short_date(first), short_date(last))), amount: None },
            Stat { label: t!("Outflow"), text: None, amount: Some(outflow) },
            Stat { label: t!("Inflow"), text: None, amount: Some(inflow) },
        ],
        check,
        warning: (!mapped).then(|| t!("Read by the assistant. Check the amounts in the review.")),
        note: Some(notes.join(" ")),
        ..Card::default()
    };
    let title = t!("{n|# transaction|# transactions} ready to review", n = file.rows.len());
    let mut summary = summary(&title, &card);
    summary["duplicates"] = json!(duplicates);
    summary["matchesExisting"] = json!(matches);
    summary["skippedRows"] = json!(skipped);
    Ok(Proposal {
        id: new_id("p"),
        title,
        kind: "import".into(),
        status: Status::Proposed,
        change: None,
        import: Some(file),
        report: None,
        workbook: None,
        saved: None,
        expected: None,
        card,
        summary,
        problem: None,
        fixes: None,
        review: None,
    })
}

/// A card for fixes to a bank file being imported: each fix, with why.
pub fn build_fix(fixes: crate::bank_statement::Fixes, rows: Vec<Row>, note: Option<String>) -> Proposal {
    let count = fixes.fields.len() + fixes.include.len() + fixes.structure.is_some() as usize;
    let title = t!("{n|# fix|# fixes} for the import", n = count);
    let card = Card { kicker: t!("Import fixes"), rows, note, ..Card::default() };
    let summary = summary(&title, &card);
    Proposal {
        id: new_id("p"),
        title,
        kind: "fix".into(),
        status: Status::Proposed,
        change: None,
        import: None,
        report: None,
        workbook: None,
        saved: None,
        expected: None,
        card,
        summary,
        problem: None,
        fixes: Some(fixes),
        review: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::tools::tests::sample;

    #[test]
    fn covering_overspending_shows_before_and_after() {
        let b = sample();
        let change = Change::Moves {
            month: "2026-10".into(),
            moves: vec![Move { from: None, to: Some("dining".into()), amount: 24000 }],
        };
        let p = build(change, "Cover dining".into(), &b).unwrap();
        assert_eq!(p.card.rows[0].label, "Dining out");
        assert_eq!(p.card.rows[0].before, Some(-24000));
        assert_eq!(p.card.rows[0].after, Some(0));
        assert_eq!(p.card.total.as_ref().unwrap().amount, Some(-24000));
        assert_eq!(p.card.highlight.categories, vec!["dining".to_string()]);
    }

    #[test]
    fn applying_again_on_the_same_budget_is_the_same_change() {
        let b = sample();
        let change = Change::Moves {
            month: "2026-10".into(),
            moves: vec![Move { from: None, to: Some("dining".into()), amount: 24000 }],
        };
        let p = build(change, "Cover dining".into(), &b).unwrap();
        match recheck(&p, &b) {
            Recheck::Same(patch) => {
                let mut after = b.clone();
                patch.apply(&mut after);
                assert_eq!(engine::calculate(&after, "2026-10").unwrap().categories.iter().find(|c| c.id == "dining").unwrap().available, 0);
            }
            Recheck::Changed(_) => panic!("expected the same change"),
        }
    }

    #[test]
    fn an_edit_to_the_same_row_makes_the_proposal_out_of_date() {
        let b = sample();
        let change = Change::Edits {
            edits: vec![Edit {
                id: "t2".into(),
                amount: None,
                previous_amount: None,
                category: Some(Some("coffee".into())),
                payee: None,
                memo: None,
                cleared: None,
                approve: true,
                flag: None,
                unsure: false,
            }],
            rules: vec![Rule { id: "r1".into(), matching: "bakery".into(), payee: String::new(), category: Some("coffee".into()) }],
        };
        let p = build(change, "Categorize".into(), &b).unwrap();
        assert_eq!(p.card.rows[0].badge.as_deref(), Some("+ rule"));
        let mut edited = b.clone();
        edited.transactions[1].memo = "changed".into();
        assert!(matches!(recheck(&p, &edited), Recheck::Changed(_)));
        assert!(matches!(recheck(&p, &b), Recheck::Same(_)));
    }

    #[test]
    fn a_new_receipt_keeps_its_id_and_splits() {
        let b = sample();
        let change = Change::Add {
            account: "checking".into(),
            transactions: vec![NewTransaction {
                id: "new1".into(),
                date: "2026-10-15".into(),
                payee: "Corner Market".into(),
                memo: String::new(),
                amount: -18640,
                cleared: false,
                splits: vec![(Some("groceries".into()), -14290, String::new()), (Some("dining".into()), -4350, String::new())],
            }],
            from_file: true,
        };
        let p = build(change, "Receipt".into(), &b).unwrap();
        assert_eq!(p.card.rows.len(), 2);
        assert!(p.card.warning.is_some());
        let Recheck::Same(patch) = recheck(&p, &b) else { panic!() };
        // It's added when it's applied; that time isn't part of what was proposed.
        let mut later = patch.clone();
        let new = &mut later.transactions.as_mut().unwrap().upsert[0];
        assert!(new.created_at().is_some());
        new.set_created_at("2099-01-01T00:00:00.000Z");
        assert_eq!(normalize(&later), normalize(&patch));
        let mut after = b.clone();
        patch.apply(&mut after);
        assert!(after.transactions.iter().any(|t| t.id == "new1" && t.splits.len() == 2));
        // Once applied, applying again can't add it twice.
        assert!(matches!(recheck(&p, &after), Recheck::Changed(ref r) if r.status == Status::Failed));
    }

    #[test]
    fn imports_check_totals_against_the_statement() {
        let b = sample();
        let rows = vec![
            ImportRow { date: "2026-10-01".into(), payee: "Salary".into(), memo: String::new(), amount: 500000, transfer_account_id: None },
            ImportRow { date: "2026-10-03".into(), payee: "Bistro".into(), memo: String::new(), amount: -124000, transfer_account_id: None },
        ];
        let p = build_import(
            "checking",
            "Everyday",
            "statement.xlsx",
            rows.clone(),
            Expected { count: Some(2), outflow: Some(124000), inflow: Some(500000) },
            vec![],
            true,
            &b,
        )
        .unwrap();
        assert_eq!(p.card.check, Some((true, "Totals match the statement".into())));
        let bad = build_import("checking", "Everyday", "s", rows, Expected { count: Some(3), ..Expected::default() }, vec![], false, &b).unwrap();
        assert!(!bad.card.check.unwrap().0);
        assert!(p.import.unwrap().csv().unwrap().starts_with("Date,Payee,Memo,Amount"));
    }

    fn skip(keys: &[&str]) -> Revision {
        Revision { skip: keys.iter().map(|k| k.to_string()).collect(), set: vec![] }
    }
    fn row<'a>(p: &'a Proposal, key: &str) -> &'a Row {
        p.card.rows.iter().find(|r| r.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no row {key}"))
    }
    fn applied(p: &Proposal, b: &Budget) -> Budget {
        let Recheck::Same(patch) = recheck(p, b) else { panic!("expected the same change") };
        let mut after = b.clone();
        patch.apply(&mut after);
        after
    }

    #[test]
    fn a_category_left_out_of_budget_moves_keeps_what_it_has_and_another_gets_another_amount() {
        let b = sample();
        let moves = vec![
            Move { from: None, to: Some("dining".into()), amount: 24000 },
            Move { from: None, to: Some("groceries".into()), amount: 30000 },
            Move { from: Some("coffee".into()), to: Some("dining".into()), amount: 5000 },
        ];
        let p = build(Change::Moves { month: "2026-10".into(), moves }, "Cover October".into(), &b).unwrap();
        // Every category the change touches, each named; one moved to from two places can't take an amount of its own.
        assert_eq!(p.card.rows.len(), 3);
        assert_eq!(row(&p, "c:dining").amount, Some(29000));
        assert!(row(&p, "c:dining").edit.is_none());
        assert!(row(&p, "c:coffee").edit.as_ref().is_some_and(|e| e.amount));
        let revision = Revision {
            skip: vec!["c:groceries".into()],
            set: vec![RowSet { key: "c:coffee".into(), amount: Some(-2000), ..RowSet::default() }],
        };
        let q = revise(&p, revision.clone(), &b).unwrap();
        assert!(row(&q, "c:groceries").skipped);
        assert_eq!(row(&q, "c:groceries").amount, Some(30000));
        assert!(row(&q, "c:coffee").edited);
        assert_eq!(row(&q, "c:coffee").amount, Some(-2000));
        assert_eq!(row(&q, "c:dining").amount, Some(26000));
        assert_eq!(q.card.total.as_ref().unwrap().amount, Some(-24000));
        let after = applied(&q, &b);
        let month = engine::calculate(&after, "2026-10").unwrap();
        let assigned = |id: &str| month.categories.iter().find(|c| c.id == id).unwrap().assigned;
        assert_eq!((assigned("dining"), assigned("coffee"), assigned("groceries")), (126000, 18000, 0));
        // A category moved to from two places can't be given an amount.
        let two = Revision { set: vec![RowSet { key: "c:dining".into(), amount: Some(1000), ..RowSet::default() }], ..revision };
        assert!(revise(&q, two, &b).unwrap_err().contains("more than one place"));
        // Choosing again starts from the assistant's own: the category left out comes back.
        let back = revise(&q, Revision::default(), &b).unwrap();
        assert!(back.review.is_none());
        assert_eq!(back.change, p.change);
        assert!(back.card.rows.iter().all(|r| !r.skipped && !r.edited));
    }

    #[test]
    fn auto_assign_given_an_amount_becomes_moves_from_ready_to_assign() {
        let b = sample();
        let change = Change::AutoAssign { month: "2026-10".into(), method: "underfunded".into(), categories: vec!["dining".into(), "groceries".into()] };
        let p = build(change, "Fund October".into(), &b).unwrap();
        let first = p.card.rows[0].key.clone().unwrap();
        let q = revise(&p, Revision { set: vec![RowSet { key: first.clone(), amount: Some(1234), ..RowSet::default() }], ..Revision::default() }, &b).unwrap();
        assert!(matches!(q.change, Some(Change::Moves { .. })));
        assert_eq!(row(&q, &first).amount, Some(1234));
        assert_eq!(q.card.rows.len(), p.card.rows.len());
    }

    #[test]
    fn edits_show_what_changes_and_can_be_left_out_or_given_another_category() {
        let b = sample();
        let change = Change::Edits {
            edits: vec![
                Edit { id: "t1".into(), amount: None, previous_amount: None, category: None, payee: None, memo: Some("team lunch".into()), cleared: None, approve: false, flag: None, unsure: false },
                Edit { id: "t2".into(), amount: None, previous_amount: None, category: Some(Some("coffee".into())), payee: Some("Bakery".into()), memo: None, cleared: None, approve: true, flag: None, unsure: true },
            ],
            rules: vec![Rule { id: "r1".into(), matching: "bakery".into(), payee: String::new(), category: Some("coffee".into()) }],
        };
        let p = build(change, "Categorize".into(), &b).unwrap();
        // Before and after, and the rule as a row of its own.
        assert_eq!(row(&p, "t:t2").detail.as_deref(), Some("Uncategorized \u{2192} Coffee · Payee: Corner Bakery \u{2192} Bakery · Approve"));
        assert_eq!(row(&p, "t:t1").detail.as_deref(), Some("Memo: team lunch"));
        assert!(row(&p, "t:t1").edit.is_none());
        let unsupported = Revision {
            set: vec![RowSet { key: "t:t1".into(), amount: Some(-120000), ..RowSet::default() }],
            ..Revision::default()
        };
        assert!(revise(&p, unsupported, &b).unwrap_err().contains("doesn't propose an amount change"));
        assert_eq!(row(&p, "t:t2").edit.as_ref().unwrap().category_id.as_deref(), Some("coffee"));
        assert_eq!(row(&p, "r:r1").badge.as_deref(), Some("rule"));
        let revision = Revision {
            skip: vec!["t:t1".into(), "r:r1".into()],
            set: vec![RowSet { key: "t:t2".into(), category: Some(Some("groceries".into())), ..RowSet::default() }],
        };
        let q = revise(&p, revision, &b).unwrap();
        let Some(Change::Edits { edits, rules }) = &q.change else { panic!() };
        assert_eq!((edits.len(), rules.len()), (1, 0));
        assert_eq!(edits[0].category, Some(Some("groceries".into())));
        // Chosen by the user: nothing left to check.
        assert!(!edits[0].unsure);
        assert!(row(&q, "t:t1").skipped && row(&q, "r:r1").skipped);
        assert!(row(&q, "t:t2").edited);
        assert!(row(&q, "t:t2").detail.as_deref().unwrap().starts_with("Uncategorized \u{2192} Groceries"));
        // The row's category to change from is the user's, so the assistant's can be chosen again.
        assert_eq!(row(&q, "t:t2").edit.as_ref().unwrap().category_id.as_deref(), Some("groceries"));
        assert_eq!(q.choices().as_deref(), Some("2 of 3 left out, 1 changed by the user"));
        assert_eq!(q.summary["rows"][0]["leftOutByUser"], json!(true));
        let after = applied(&q, &b);
        let t2 = after.transactions.iter().find(|t| t.id == "t2").unwrap();
        assert_eq!(t2.splits[0].category_id.as_deref(), Some("groceries"));
        assert_eq!(after.transactions.iter().find(|t| t.id == "t1").unwrap().memo, "");
        // Ready to Assign is a category to give, too.
        let ready = Revision { set: vec![RowSet { key: "t:t2".into(), category: Some(None), ..RowSet::default() }], ..Revision::default() };
        let r = revise(&p, serde_json::from_value(serde_json::to_value(&ready).unwrap()).unwrap(), &b).unwrap();
        let Some(Change::Edits { edits, .. }) = &r.change else { panic!() };
        assert_eq!(edits[1].category, Some(None));
        assert_eq!(row(&r, "t:t2").edit.as_ref().unwrap().category_id, None);
    }

    fn amount_edit(b: &Budget, amount: i64) -> Proposal {
        build(Change::Edits {
            edits: vec![Edit {
                id: "t1".into(), amount: Some(amount), previous_amount: Some(b.transactions[0].amount),
                category: None, payee: None, memo: None, cleared: None, approve: false, flag: None, unsure: true,
            }],
            rules: vec![],
        }, "Correct the amount".into(), b).unwrap()
    }

    #[test]
    fn existing_amount_edits_can_be_revised_and_left_out_without_losing_the_original() {
        let b = sample();
        let p = amount_edit(&b, -120000);
        let revision = Revision { set: vec![RowSet { key: "t:t1".into(), amount: Some(-119930), ..RowSet::default() }], ..Revision::default() };
        let q = revise(&p, revision, &b).unwrap();
        let shown = row(&q, "t:t1");
        assert_eq!((shown.before, shown.after, shown.amount), (Some(-124000), Some(-119930), Some(-119930)));
        assert!(shown.edit.as_ref().unwrap().amount && shown.edited);
        assert!(shown.badge.is_none());
        let after = applied(&q, &b);
        assert_eq!((after.transactions[0].amount, after.transactions[0].splits[0].amount), (-119930, -119930));
        let restore = revise(&q, Revision::default(), &b).unwrap();
        assert_eq!(applied(&restore, &b).transactions[0].amount, -120000);
        let skipped = revise(&q, skip(&["t:t1"]), &b).unwrap();
        assert!(skipped.expected.is_none() && row(&skipped, "t:t1").skipped);
        for amount in [0, i64::MAX] {
            let invalid = Revision { set: vec![RowSet { key: "t:t1".into(), amount: Some(amount), ..RowSet::default() }], ..Revision::default() };
            assert!(revise(&p, invalid, &b).is_err());
        }
    }

    #[test]
    fn an_amount_changed_since_the_proposal_cannot_be_silently_overwritten() {
        let b = sample();
        let p = amount_edit(&b, -120000);
        assert!(matches!(recheck(&p, &b), Recheck::Same(_)));
        let mut current = b.clone();
        current.transactions[0].amount = -125000;
        current.transactions[0].splits[0].amount = -125000;
        let Recheck::Changed(refreshed) = recheck(&p, &current) else { panic!("must not silently replace a later correction") };
        assert_eq!(refreshed.status, Status::Failed);
        assert!(refreshed.problem.unwrap().contains("different amount now"));
        assert!(revise(&p, Revision::default(), &current).is_err());
        // Already applied remains an ordinary handled proposal, not another change.
        let after = applied(&p, &b);
        let Recheck::Changed(again) = recheck(&p, &after) else { panic!("must not apply twice") };
        assert_eq!(again.status, Status::Failed);
        assert!(again.problem.unwrap().contains("already has these changes"));
    }

    #[test]
    fn changing_a_whole_transfer_across_the_budget_edge_keeps_its_category() {
        let mut b = sample();
        b.accounts.push(Account {
            id: "tracking".into(), name: "Tracking".into(), kind: AccountKind::Tracking,
            opening_balance: 0, archived: false, extra: Default::default(),
        });
        let transaction = &mut b.transactions[0];
        transaction.transfer_account_id = Some("tracking".into());
        transaction.transfer_status = Some(Cleared::Cleared);
        transaction.payee.clear();
        let p = amount_edit(&b, -120000);
        let after = applied(&p, &b);
        let t = &after.transactions[0];
        assert_eq!(t.transfer_account_id.as_deref(), Some("tracking"));
        assert_eq!((t.amount, t.splits[0].amount), (-120000, -120000));
        assert_eq!(t.splits[0].category_id.as_deref(), Some("dining"));
    }

    #[test]
    fn a_receipt_keeps_its_splits_named_by_place_and_takes_their_new_amounts() {
        let b = sample();
        let change = Change::Add {
            account: "checking".into(),
            transactions: vec![NewTransaction {
                id: "new1".into(),
                date: "2026-10-15".into(),
                payee: "Corner Market".into(),
                memo: String::new(),
                amount: -18640,
                cleared: false,
                splits: vec![(Some("groceries".into()), -14290, String::new()), (Some("dining".into()), -4350, String::new())],
            }],
            from_file: true,
        };
        let p = build(change, "Receipt".into(), &b).unwrap();
        let q = revise(
            &p,
            Revision { skip: vec!["s:0".into()], set: vec![RowSet { key: "s:1".into(), amount: Some(-3000), category: Some(Some("coffee".into())) }] },
            &b,
        )
        .unwrap();
        assert!(row(&q, "s:0").skipped);
        assert_eq!(row(&q, "s:0").label, "Groceries");
        assert!(row(&q, "s:1").edited);
        assert_eq!((row(&q, "s:1").label.as_str(), row(&q, "s:1").amount), ("Coffee", Some(-3000)));
        assert_eq!(q.card.stats[2].amount, Some(-3000));
        let after = applied(&q, &b);
        let t = after.transactions.iter().find(|t| t.id == "new1").unwrap();
        assert_eq!((t.amount, t.splits.len()), (-3000, 1));
        // An amount of nothing is a row to leave out.
        let zero = Revision { set: vec![RowSet { key: "s:1".into(), amount: Some(0), ..RowSet::default() }], ..Revision::default() };
        assert!(revise(&p, zero, &b).unwrap_err().contains("leave it out"));
    }

    #[test]
    fn several_new_transactions_keep_their_rows_when_all_but_one_are_left_out() {
        let b = sample();
        let new = |id: &str, amount: i64| NewTransaction {
            id: id.into(),
            date: "2026-10-15".into(),
            payee: format!("Shop {id}"),
            memo: String::new(),
            amount,
            cleared: false,
            splits: vec![(Some("groceries".into()), amount, String::new())],
        };
        let change = Change::Add { account: "checking".into(), transactions: vec![new("a", -1000), new("b", -2000)], from_file: false };
        let p = build(change, "Two".into(), &b).unwrap();
        let q = revise(&p, skip(&["t:a"]), &b).unwrap();
        assert_eq!(q.card.rows.iter().map(|r| r.key.clone().unwrap()).collect::<Vec<_>>(), ["t:a", "t:b"]);
        assert!(row(&q, "t:a").skipped && !row(&q, "t:b").skipped);
        assert!(q.card.stats.is_empty());
    }

    #[test]
    fn a_card_with_every_row_left_out_applies_nothing_until_one_is_chosen_again() {
        let b = sample();
        let change = Change::Payees {
            renames: vec![Rename { from: "Corner Bakery".into(), to: "Bakery".into() }],
            rules: vec![Rule { id: "r1".into(), matching: "bistro".into(), payee: "Bistro".into(), category: None }],
        };
        let p = build(change, "Tidy payees".into(), &b).unwrap();
        let q = revise(&p, skip(&["p:Corner Bakery", "r:r1"]), &b).unwrap();
        assert!(q.card.rows.iter().all(|r| r.skipped));
        assert!(q.expected.is_none());
        let Recheck::Changed(r) = recheck(&q, &b) else { panic!() };
        assert_eq!(r.status, Status::Proposed);
        assert_eq!(r.problem.as_deref(), Some("Choose at least one change to apply."));
        // A budget changed since: the card is made again with what the user chose.
        let mut changed = b.clone();
        changed.transactions.push(Transaction { id: "t9".into(), payee: "Corner Bakery".into(), ..changed.transactions[1].clone() });
        let one = revise(&p, skip(&["r:r1"]), &b).unwrap();
        let Recheck::Changed(fresh) = recheck(&one, &changed) else { panic!() };
        assert!(row(&fresh, "r:r1").skipped);
        assert_eq!(row(&fresh, "p:Corner Bakery").detail.as_deref(), Some("3 transactions"));
        assert!(matches!(recheck(&fresh, &changed), Recheck::Same(_)));
    }

    #[test]
    fn fixes_for_an_import_are_left_out_or_given_another_amount() {
        use crate::bank_statement::{Field, FieldFix, Fixes, IncludeFix, Structure};
        let fixes = Fixes {
            structure: Some(Structure { flip_signs: true, ..Structure::default() }),
            fields: vec![FieldFix { line: "30".into(), line_version: 0, field: Field::Amount, from: "n/a".into(), to: "-12.40".into() }],
            include: vec![IncludeFix { line: "90".into(), line_version: 0, include: true }],
            ..Fixes::default()
        };
        let rows = vec![
            Row { label: fix_label("30", Field::Amount, "n/a", "-12.40"), amount: Some(-1240), key: Some(fix_key("30", Field::Amount)), edit: Some(RowEdit { amount: true, ..RowEdit::default() }), ..Row::default() },
            Row { label: "Line 90 · include it".into(), key: Some("i:90".into()), ..Row::default() },
            Row { label: "Purchases written as positive".into(), key: Some("x".into()), ..Row::default() },
        ];
        let p = build_fix(fixes, rows, None);
        let q = revise(
            &p,
            Revision { skip: vec!["i:90".into(), "x".into()], set: vec![RowSet { key: "f:30:amount".into(), amount: Some(-1500), ..RowSet::default() }] },
            &sample(),
        )
        .unwrap();
        let f = q.fixes.as_ref().unwrap();
        assert_eq!((f.fields[0].to.as_str(), f.include.len(), f.structure.is_none()), ("-15.00", 0, true));
        assert_eq!(q.card.rows[0].label, "Line 30 · amount: n/a \u{2192} -15.00");
        assert!(q.card.rows[0].edited && q.card.rows[1].skipped && q.card.rows[2].skipped);
        // Back to the assistant's own.
        let back = revise(&q, Revision::default(), &sample()).unwrap();
        assert_eq!(back.fixes, p.fixes);
        assert_eq!(back.card, p.card);
    }
}
