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
    pub body: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub source_id: String,
    pub quote: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transaction {
    pub account_id: String,
    pub date: String,
    pub amount: String,
    pub currency: String,
    pub payee: String,
    pub memo: String,
    pub kind: String,
    pub category_id: Option<String>,
    pub transfer_account_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub source_ids: Vec<String>,
    pub disposition: String,
    pub reason: String,
    pub evidence: Vec<Evidence>,
    pub transaction: Option<Transaction>,
    pub question: Option<String>,
    /// A literal masked card/account reference, used only after the user confirms a mapping.
    pub account_reference: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Analysis {
    pub items: Vec<Item>,
}

pub fn cents(text: &str) -> Result<i64, String> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
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

pub fn validate_messages(messages: &[Message]) -> Result<(), String> {
    let mut ids = HashSet::new();
    if messages.is_empty()
        || messages.len() > 50
        || messages.iter().map(|m| m.body.len()).sum::<usize>() > 120_000
    {
        return Err(crate::t!("Choose 1–50 messages, at most 120 KB in total."));
    }
    for m in messages {
        if !m.id.starts_with("sms:")
            || m.id.len() > 100
            || !ids.insert(&m.id)
            || m.sender.is_empty()
            || m.sender.len() > 200
            || m.body.trim().is_empty()
            || m.body.len() > 65_536
            || !valid_date(&m.received_date)
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

/// Validate coverage and references independently of the model and again at the save boundary.
pub fn validate_analysis(
    b: &Budget,
    messages: &[Message],
    analysis: &Analysis,
) -> Result<(), String> {
    validate(Some(b), messages, analysis)
}
/// A refinement may repair references deleted since an earlier review; provenance still must hold.
pub fn validate_sources(messages: &[Message], analysis: &Analysis) -> Result<(), String> {
    validate(None, messages, analysis)
}
fn validate(
    budget: Option<&Budget>,
    messages: &[Message],
    analysis: &Analysis,
) -> Result<(), String> {
    validate_messages(messages)?;
    let sources: HashMap<_, _> = messages.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut seen = HashSet::new();
    if analysis.items.is_empty() || analysis.items.len() > messages.len() {
        return Err(crate::t!("Account for every message."));
    }
    for item in &analysis.items {
        if item.source_ids.is_empty() || item.reason.trim().is_empty() || item.reason.len() > 2000 {
            return Err(crate::t!(
                "Every item needs its sources and an explanation."
            ));
        }
        for id in &item.source_ids {
            if !sources.contains_key(id.as_str()) || !seen.insert(id.as_str()) {
                return Err(crate::t!("Unknown or repeated source message."));
            }
        }
        if item.evidence.is_empty() || item.evidence.len() > 50 {
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
        if let Some(reference) = &item.account_reference {
            if reference.trim().is_empty()
                || reference.len() > 80
                || !item
                    .source_ids
                    .iter()
                    .all(|id| sources[id.as_str()].body.contains(reference))
            {
                return Err(crate::t!(
                    "Account references must occur in every grouped source."
                ));
            }
        }
        match item.disposition.as_str() {
            "transaction" => {
                let t = item
                    .transaction
                    .as_ref()
                    .ok_or_else(|| crate::t!("Missing transaction fields."))?;
                let Some(b) = budget else {
                    continue;
                };
                let amount = cents(&t.amount)?;
                if !active_account(b, &t.account_id)
                    || !valid_date(&t.date)
                    || t.currency != b.currency
                    || t.payee.trim().is_empty()
                    || t.payee.len() > 500
                    || t.memo.len() > 2000
                {
                    return Err(crate::t!("Check the account, currency, date and payee."));
                }
                if let Some(category) = &t.category_id {
                    if !b
                        .categories
                        .iter()
                        .any(|c| c.id == *category && !c.extra.contains_key("paymentAccountId"))
                    {
                        return Err(crate::t!("Choose an existing spending category."));
                    }
                }
                match t.kind.as_str() {
                    "expense" if amount < 0 && t.transfer_account_id.is_none() => (),
                    "income" | "refund" if amount > 0 && t.transfer_account_id.is_none() => (),
                    "transfer"
                        if t.transfer_account_id
                            .as_deref()
                            .is_some_and(|a| a != t.account_id && active_account(b, a))
                            && t.category_id.is_none() =>
                    {
                        ()
                    }
                    _ => {
                        return Err(crate::t!(
                            "Check the transaction direction and transfer accounts."
                        ))
                    }
                }
            }
            "question"
                if item
                    .question
                    .as_ref()
                    .is_some_and(|q| !q.trim().is_empty() && q.len() <= 2000)
                    && item.transaction.is_none() =>
            {
                ()
            }
            "ignore" | "pending" if item.transaction.is_none() => (),
            _ => {
                return Err(crate::t!(
                    "Choose transaction, question, ignore or pending for every source."
                ))
            }
        }
    }
    if seen.len() != messages.len() {
        return Err(crate::t!(
            "The analysis omitted messages. Account for every source exactly once."
        ));
    }
    Ok(())
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
    let t = item
        .transaction
        .as_ref()
        .expect("validated transaction item");
    let mut row = bank_import::review(
        b,
        &t.account_id,
        vec![bank_import::BankTransaction {
            fit: item.source_ids.iter().min().unwrap().clone(),
            date: t.date.clone(),
            payee: t.payee.clone(),
            memo: t.memo.clone(),
            amount: cents(&t.amount)?,
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
pub fn preview(
    b: &Budget,
    messages: &[Message],
    analysis: &Analysis,
) -> Result<Vec<Review>, String> {
    validate_analysis(b, messages, analysis)?;
    let imported = imported(b);
    let mut result = vec![];
    for (index, item) in analysis.items.iter().enumerate() {
        if let Some(t) = &item.transaction {
            let mut row = review_item(b, item)?;
            row.duplicate |= item.source_ids.iter().any(|id| imported.contains(id));
            result.push(Review {
                index,
                account_id: t.account_id.clone(),
                row,
            });
        }
    }
    Ok(result)
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    pub index: usize,
    pub action: String,
    pub match_id: Option<String>,
}

/// One caller commit/undo step for every account. Never trust model-supplied bank review rows.
pub fn apply_batch(
    mut b: Budget,
    messages: &[Message],
    analysis: &Analysis,
    decisions: Vec<Decision>,
) -> Result<Budget, String> {
    let review = preview(&b, messages, analysis)?;
    if decisions.len() != review.len() {
        return Err(crate::t!("Review every proposed transaction."));
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
