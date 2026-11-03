//! SMS alerts use bank matching and stable import IDs, but are not proof of settlement.
use crate::{
    bank_import,
    domain::{Budget, Cleared},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;

/// Transport data, not a bank-format parser. Source identities are supplied by Android.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub sender: String,
    pub received_date: String,
    /// When in the day it came, "HH:MM", when the phone knows; it tells apart a day's alerts.
    #[serde(default)]
    pub received_time: String,
    pub body: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub source_id: String,
    pub quote: String,
}
/// What a message says happened. The model reads the facts; accounts are the app's and the user's.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transaction {
    pub date: String,
    /// Signed, in the budget's currency. None until the user enters what a foreign purchase cost.
    pub amount: Option<String>,
    /// The amount as the message writes it, in its own currency.
    pub currency: String,
    pub written_amount: String,
    pub payee: String,
    pub memo: String,
    pub kind: String,
    /// Words of the message naming the card or account ("4417", "***0091"), copied exactly.
    pub account_ref: Option<String>,
    /// A transfer's other account, copied the same way.
    pub other_ref: Option<String>,
    /// A transfer's other account by the name the user gave it, when the message says what it is
    /// ("your term deposit") rather than naming it in a way the app can match: the model's
    /// reading, for the app to resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_account: Option<String>,
    pub category_id: Option<String>,
    /// Filled by the app from a remembered card or the account's name, or chosen by the user.
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub transfer_account_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub source_ids: Vec<String>,
    /// transaction, pending (a hold), ignore, or unclear (moves money but isn't one transaction).
    pub disposition: String,
    /// Why an ignored message isn't a transaction, as a code the review groups by.
    pub skip: Option<String>,
    pub note: String,
    pub evidence: Vec<Evidence>,
    pub transaction: Option<Transaction>,
    /// Fields the user should look at: date, amount, payee, kind, category.
    #[serde(default)]
    pub unsure: Vec<String>,
    /// The template that read this text on the phone, when one did (the assistant reviewed it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// The payee as the bank wrote it, when a template read it: what the user's payee is learned from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// Settled on the phone without going to the model: a trusted skip template's text, or a code.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kept: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    pub items: Vec<Item>,
}

pub fn cents(text: &str) -> Result<i64, String> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    // A currency with three decimals writes a third that's often 0: "12.500" is exactly 12.50.
    let fraction = if fraction.len() > 2 && fraction[2..].bytes().all(|c| c == b'0') { &fraction[..2] } else { fraction };
    if whole.is_empty()
        || !whole.bytes().all(|x| x.is_ascii_digit())
        || fraction.len() > 2
        || !fraction.bytes().all(|x| x.is_ascii_digit())
        || (digits.contains('.') && fraction.is_empty())
    {
        return Err(crate::t!(
            "Write an exact amount with at most two decimal places."
        ));
    }
    let amount = whole
        .parse::<i64>()
        .ok()
        .and_then(|v| v.checked_mul(100))
        .and_then(|v| v.checked_add(format!("{fraction:0<2}").parse::<i64>().ok()?))
        .filter(|v| *v > 0 && *v <= 1_000_000_000_000)
        .ok_or_else(|| crate::t!("Invalid transaction amount."))?;
    Ok(if text.starts_with('-') {
        -amount
    } else {
        amount
    })
}

fn total(messages: &[Message]) -> usize {
    messages.iter().map(|m| m.body.len()).sum()
}
/// The messages one request carries. Most may be read on the phone (junk) or only reviewed, so a
/// request holds up to 1,000; the service caps how many the model reads in full.
pub fn validate_messages(messages: &[Message]) -> Result<(), String> {
    if messages.is_empty() || messages.len() > 1_000 || total(messages) > 1_000_000 {
        return Err(crate::t!("Choose 1–1,000 messages, at most 1 MB in total."));
    }
    validate_sources(messages)
}
/// The messages one review holds, read 200 at a time: a phone's whole history of bank alerts.
fn validate_review(messages: &[Message]) -> Result<(), String> {
    if messages.is_empty() || messages.len() > 10_000 || total(messages) > 8_000_000 {
        return Err(crate::t!("A review holds 1–10,000 messages, at most 8 MB in total."));
    }
    validate_sources(messages)
}
fn validate_sources(messages: &[Message]) -> Result<(), String> {
    let mut ids = HashSet::new();
    for m in messages {
        if !m.id.starts_with("sms:")
            || m.id.len() > 100
            || !ids.insert(&m.id)
            || m.sender.is_empty()
            || m.sender.len() > 200
            || m.body.trim().is_empty()
            || m.body.len() > 65_536
            || !valid_date(&m.received_date)
            || !(m.received_time.is_empty() || chrono::NaiveTime::parse_from_str(&m.received_time, "%H:%M").is_ok())
        {
            return Err(crate::t!("Invalid or repeated source message."));
        }
    }
    Ok(())
}
fn valid_date(date: &str) -> bool {
    date.len() == 10 && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
}
fn active_account(b: &Budget, id: &str) -> bool {
    b.accounts.iter().any(|a| a.id == id && !a.archived)
}

pub const SKIPS: [&str; 7] = [
    "code", "security", "balance", "declined", "rewards", "marketing", "other",
];
const UNSURE: [&str; 5] = ["date", "amount", "payee", "kind", "category"];

/// An unsigned amount as a message writes it, in any currency (some use three decimals).
fn written(text: &str) -> bool {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    !whole.is_empty()
        && whole.len() <= 15
        && fraction.len() <= 3
        && (fraction.is_empty() == !text.contains('.'))
        && whole.bytes().chain(fraction.bytes()).all(|x| x.is_ascii_digit())
}
fn reference(text: &Option<String>) -> bool {
    text.as_ref()
        .map_or(true, |r| !r.trim().is_empty() && r.len() <= 80)
}

/// Validate coverage and references independently of the model, and again at the save boundary.
pub fn validate_analysis(
    b: &Budget,
    messages: &[Message],
    analysis: &Analysis,
) -> Result<(), String> {
    validate_review(messages)?;
    let sources: HashMap<_, _> = messages.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut seen = HashSet::new();
    if analysis.items.is_empty() || analysis.items.len() > messages.len() {
        return Err(crate::t!("Account for every message."));
    }
    for item in &analysis.items {
        // A skipped message is said by its skip code; everything else says what it is.
        let ignored = item.disposition == "ignore";
        if item.source_ids.is_empty() || (!ignored && item.note.trim().is_empty()) || item.note.len() > 2000 {
            return Err(crate::t!("Every item needs its sources and a note."));
        }
        for id in &item.source_ids {
            if !sources.contains_key(id.as_str()) || !seen.insert(id.as_str()) {
                return Err(crate::t!("Unknown or repeated source message."));
            }
        }
        // Money the user may add is backed by the message's own words.
        let backed = matches!(item.disposition.as_str(), "transaction" | "pending");
        if (backed && item.evidence.is_empty()) || item.evidence.len() > 50 {
            return Err(crate::t!("Include source evidence for every item."));
        }
        for e in &item.evidence {
            if !item.source_ids.contains(&e.source_id)
                || e.quote.trim().is_empty()
                || !sources[e.source_id.as_str()].body.contains(&e.quote)
            {
                return Err(crate::t!(
                    "Evidence must quote an item's original message exactly."
                ));
            }
        }
        if item.unsure.len() > UNSURE.len()
            || item.unsure.iter().any(|u| !UNSURE.contains(&u.as_str()))
        {
            return Err(crate::t!("unsure lists only date, amount, payee, kind or category."));
        }
        let fields = match (item.disposition.as_str(), &item.transaction) {
            ("transaction" | "pending", Some(t)) if item.skip.is_none() => t,
            ("ignore", None) if item.skip.as_deref().is_some_and(|s| SKIPS.contains(&s)) => continue,
            ("unclear", None) if item.skip.is_none() => continue,
            _ => {
                return Err(crate::t!(
                    "Give a transaction for transaction and pending, a skip code for ignore, and neither for unclear."
                ))
            }
        };
        transaction(b, &item.source_ids, &sources, fields)?;
    }
    if seen.len() != messages.len() {
        return Err(crate::t!(
            "The analysis omitted messages. Account for every source exactly once."
        ));
    }
    Ok(())
}
fn transaction(
    b: &Budget,
    ids: &[String],
    sources: &HashMap<&str, &Message>,
    t: &Transaction,
) -> Result<(), String> {
    if !valid_date(&t.date)
        || t.payee.trim().is_empty()
        || t.payee.len() > 500
        || t.memo.len() > 2000
        || t.currency.len() != 3
        || !t.currency.bytes().all(|c| c.is_ascii_uppercase())
        || !written(&t.written_amount)
    {
        return Err(crate::t!("Check the date, payee, currency and written amount."));
    }
    // Only a foreign amount the message doesn't convert waits for the user.
    let amount = match &t.amount {
        Some(amount) => Some(cents(amount)?),
        None if t.currency != b.currency => None,
        None => return Err(crate::t!("Give the amount in the budget's currency.")),
    };
    if !reference(&t.account_ref) || !reference(&t.other_ref) {
        return Err(crate::t!("Check the card and account references."));
    }
    // Checked against the account list when the model gives it: a later rename mustn't break the review.
    if t.other_account.as_ref().is_some_and(|n| n.trim().is_empty() || n.len() > 200 || t.kind != "transfer") {
        return Err(crate::t!("Name the other account only for a transfer."));
    }
    // A card reference must be in every grouped message; a transfer's other side in one of them.
    if t.account_ref.as_ref().is_some_and(|r| !ids.iter().all(|id| sources[id.as_str()].body.contains(r)))
        || t.other_ref.as_ref().is_some_and(|r| !ids.iter().any(|id| sources[id.as_str()].body.contains(r)))
    {
        return Err(crate::t!(
            "Copy card and account references exactly from the message."
        ));
    }
    if let Some(category) = &t.category_id {
        if t.kind == "transfer"
            || !b
                .categories
                .iter()
                .any(|c| c.id == *category && c.payment_account().is_none())
        {
            return Err(crate::t!("Choose an existing spending category, never for a transfer."));
        }
    }
    if t.account_id.as_deref().is_some_and(|a| !active_account(b, a))
        || t.transfer_account_id.as_deref().is_some_and(|a| {
            !active_account(b, a) || t.account_id.as_deref() == Some(a) || t.kind != "transfer"
        })
    {
        return Err(crate::t!("Choose open accounts, and a different one for a transfer."));
    }
    match (t.kind.as_str(), amount) {
        ("expense", Some(a)) if a < 0 => Ok(()),
        ("income" | "refund", Some(a)) if a > 0 => Ok(()),
        ("expense" | "income" | "refund" | "transfer", None) | ("transfer", Some(_)) => Ok(()),
        _ => Err(crate::t!(
            "Check the transaction direction: money out is negative."
        )),
    }
}

/// Ready to import: its amount is known and its accounts are chosen.
pub fn importable(item: &Item) -> Option<&Transaction> {
    item.transaction.as_ref().filter(|t| {
        matches!(item.disposition.as_str(), "transaction" | "pending")
            && t.account_id.is_some()
            && t.amount.is_some()
            && (t.kind != "transfer" || t.transfer_account_id.is_some())
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub index: usize,
    pub account_id: String,
    pub row: bank_import::Row,
}
fn imported(b: &Budget) -> HashSet<String> {
    b.extra
        .get("smsImportedMessageIds")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}
fn review_item(b: &Budget, item: &Item) -> Result<bank_import::Row, String> {
    let t = importable(item).ok_or_else(|| crate::t!("Choose the account and amount first."))?;
    let mut row = bank_import::review(
        b,
        t.account_id.as_deref().unwrap(),
        vec![bank_import::BankTransaction {
            fit: item.source_ids.iter().min().unwrap().clone(),
            date: t.date.clone(),
            payee: t.payee.clone(),
            memo: t.memo.clone(),
            amount: cents(t.amount.as_deref().unwrap())?,
            transfer_account_id: t.transfer_account_id.clone(),
        }],
    )?
    .remove(0);
    row.category_id = t.category_id.clone();
    // The reviewed model/user fields are authoritative. Bank matching must not re-interpret them.
    row.rename = None;
    row.refund_of = None;
    Ok(row)
}
/// Bank-review rows for the items that are ready; the rest wait for the user's choices.
pub fn preview(
    b: &Budget,
    messages: &[Message],
    analysis: &Analysis,
) -> Result<Vec<Review>, String> {
    validate_analysis(b, messages, analysis)?;
    let imported = imported(b);
    let mut result = vec![];
    for (index, item) in analysis.items.iter().enumerate() {
        if let Some(t) = importable(item) {
            let mut row = review_item(b, item)?;
            row.duplicate |= item.source_ids.iter().any(|id| imported.contains(id));
            result.push(Review {
                index,
                account_id: t.account_id.clone().unwrap(),
                row,
            });
        }
    }
    Ok(result)
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub index: usize,
    pub action: String,
    pub match_id: Option<String>,
}

/// One caller commit/undo step for every account. Never trust model-supplied bank review rows.
/// Only the items with a decision are imported; the others stay with the user.
pub fn apply_batch(
    mut b: Budget,
    messages: &[Message],
    analysis: &Analysis,
    decisions: Vec<Decision>,
) -> Result<Budget, String> {
    let review = preview(&b, messages, analysis)?;
    if decisions.is_empty() {
        return Err(crate::t!("Choose at least one transaction to add."));
    }
    let mut seen = HashSet::new();
    let mut imported = imported(&b);
    let mut matches = HashSet::new();
    for decision in decisions {
        if !seen.insert(decision.index) {
            return Err(crate::t!("Repeated review decision."));
        }
        let row = review
            .iter()
            .find(|r| r.index == decision.index)
            .ok_or_else(|| crate::t!("Unknown review decision."))?;
        if !matches!(decision.action.as_str(), "new" | "match" | "skip") {
            return Err(crate::t!("Choose add, match or skip."));
        }
        if decision.action == "skip" || row.row.duplicate {
            continue;
        }
        if decision.action == "match"
            && !matches.insert(
                decision
                    .match_id
                    .clone()
                    .ok_or_else(|| crate::t!("Choose a match."))?,
            )
        {
            return Err(crate::t!(
                "Two messages cannot match the same transaction. Group them or skip one."
            ));
        }
        // Sequential previews also catch a transfer imported by an earlier account in this batch.
        let current = review_item(&b, &analysis.items[decision.index])?;
        if decision.action == "new"
            && current
                .candidates
                .iter()
                .any(|id| !row.row.candidates.contains(id))
            && (current.transfer_account_id.is_some() || !current.transfer_candidates.is_empty())
        {
            return Err(crate::t!("These messages may describe both sides of one transfer. Group them before importing."));
        }
        let adding = decision.action == "new";
        let new_id = current.id.clone();
        let defaults: Vec<_> = if adding {
            ["payeeRules", "payees"]
                .into_iter()
                .filter_map(|key| b.extra.remove(key).map(|value| (key, value)))
                .collect()
        } else {
            vec![]
        };
        b = apply(
            b,
            &row.account_id,
            vec![bank_import::Choice {
                row: current,
                action: decision.action,
                match_id: decision.match_id,
            }],
        )?;
        for (key, value) in defaults {
            b.extra.insert(key.into(), value);
        }
        if adding {
            let proposed = analysis.items[decision.index].transaction.as_ref().unwrap();
            if let Some(category) = &proposed.category_id {
                let transaction = b
                    .transactions
                    .iter_mut()
                    .find(|t| t.id == new_id)
                    .expect("new reviewed transaction");
                if let Some(split) = transaction.splits.first_mut() {
                    split.category_id = Some(category.clone());
                    split.extra.remove("unassigned");
                }
            }
        }
        imported.extend(analysis.items[decision.index].source_ids.iter().cloned());
    }
    let mut ids: Vec<_> = imported.into_iter().collect();
    ids.sort();
    b.extra.insert("smsImportedMessageIds".into(), json!(ids));
    crate::domain::validate(&b)?;
    Ok(b)
}

pub fn apply(
    budget: Budget,
    account: &str,
    choices: Vec<bank_import::Choice>,
) -> Result<Budget, String> {
    let before: HashMap<_, _> = budget
        .transactions
        .iter()
        .map(|t| (t.id.clone(), t.clone()))
        .collect();
    let mut result = bank_import::apply(budget, account, choices)?;
    for transaction in &mut result.transactions {
        if let Some(old) = before.get(&transaction.id) {
            transaction.status = old.status;
            if transaction.transfer_account_id.is_some() {
                transaction.transfer_status =
                    Some(old.transfer_status.unwrap_or(Cleared::Uncleared));
            }
            // Matching a leg of a split transfer also must not mark that side cleared.
            for (part, previous) in transaction.splits.iter_mut().zip(&old.splits) {
                if let Some(status) = previous.extra.get("transferStatus") {
                    part.extra.insert("transferStatus".into(), status.clone());
                } else {
                    part.extra.remove("transferStatus");
                }
            }
        } else {
            transaction.status = Cleared::Uncleared;
            if transaction.transfer_account_id.is_some() {
                transaction.transfer_status = Some(Cleared::Uncleared);
            }
        }
    }
    crate::domain::validate(&result)?;
    Ok(result)
}
