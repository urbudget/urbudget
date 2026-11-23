//! The budget a setup walkthrough ends with, from its answers: accounts, a category template
//! with some left out and some added, what a month costs, and expected income. The three apps
//! ask the questions; this builds the budget, so they can't differ
//! (docs/plans/first-budget-walkthrough.md).
//!
//! Nothing is assigned here. The apps preview Quick assign › Underfunded on what this returns
//! (`preview`, the same `workflows::auto_assign_preview`) and apply it as a normal edit, so Undo
//! takes it back.
//!
//! Answers name categories by where they are, never by name, so answers kept in one language
//! build the same budget in another: a template's category is `"<group>.<category>"` counting
//! from 0 (`"0.2"` is the third category of the first group), and an added one is `"+<n>"`, its
//! place in `added`.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::budget_templates::{self, BudgetTemplate};
use crate::budgets::new_id;
use crate::domain::{self, valid_month, Account, AccountKind, Budget, Category};
use crate::i18n::tr;
use crate::workflows::{self, AssignChange};
use crate::t;

/// The group every card's payment category is in, as the apps name it when an account is added.
pub const CARD_PAYMENTS: &str = "Credit Card Payments";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Answers {
    /// Blank for the name a new budget has anyway.
    pub name: String,
    pub currency: String,
    /// The budget's own display settings, kept as the app sent them.
    pub number_format: Option<Value>,
    pub date_format: Option<Value>,
    pub accounts: Vec<AccountAnswer>,
    /// A template's id (budget_templates.rs); none for no categories.
    pub template: Option<String>,
    /// Template categories left out, by place.
    pub left_out: Vec<String>,
    /// Names of categories added; a blank one is skipped and keeps its place.
    pub added: Vec<String>,
    pub amounts: Vec<AmountAnswer>,
    pub expected_income: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AccountAnswer {
    /// A row without a name isn't an account yet and is skipped.
    pub name: String,
    #[serde(rename = "type")]
    pub kind: AccountType,
    /// Today's balance in cents. For a credit card, what is owed, as a plain number: it's stored
    /// as the negative balance the engine expects.
    #[serde(default)]
    pub balance: i64,
}

/// The types the walkthrough offers, by the keys the apps keep in an account's `accountType`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AccountType {
    Checking,
    Savings,
    Cash,
    CreditCard,
}
impl AccountType {
    fn key(self) -> &'static str {
        match self {
            Self::Checking => "checking",
            Self::Savings => "savings",
            Self::Cash => "cash",
            Self::CreditCard => "creditCard",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AmountAnswer {
    /// The category, by place (see the module's note).
    pub category: String,
    /// What it costs, in cents; zero or less leaves no target.
    pub amount: i64,
    #[serde(default)]
    pub period: Period,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Period {
    #[default]
    Month,
    /// Saved as a twelfth a month, rounded up so twelve months cover it.
    Year,
}

/// The budget these answers describe, in the language in use. Nothing is assigned.
pub fn build(answers: &Answers) -> Result<Budget, String> {
    assemble(answers, &budget_templates::list(), tr("Other"))
}

/// `build` with the templates and the added categories' group given, whatever the language.
fn assemble(answers: &Answers, templates: &[BudgetTemplate], own_group: &str) -> Result<Budget, String> {
    let mut budget = Budget { currency: answers.currency.trim().to_owned(), ..Budget::default() };
    if !answers.name.trim().is_empty() {
        budget.name = answers.name.trim().to_owned();
    }
    for (key, value) in [("numberFormat", &answers.number_format), ("dateFormat", &answers.date_format)] {
        if let Some(value) = value.as_ref().filter(|v| !v.is_null()) {
            budget.extra.insert(key.into(), value.clone());
        }
    }
    if let Some(income) = answers.expected_income.filter(|n| *n > 0) {
        budget.extra.insert("expectedIncome".into(), json!(income));
    }

    let mut cards = vec![];
    for row in answers.accounts.iter().filter(|a| !a.name.trim().is_empty()) {
        let card = row.kind == AccountType::CreditCard;
        let account = Account {
            extra: Map::from_iter([("accountType".to_owned(), json!(row.kind.key())), ("onBudget".to_owned(), json!(true))]),
            id: new_id(),
            name: row.name.trim().to_owned(),
            kind: if card { AccountKind::Credit } else { AccountKind::Cash },
            opening_balance: if card { -row.balance } else { row.balance },
            archived: false,
        };
        if card {
            cards.push((account.id.clone(), account.name.clone()));
        }
        budget.accounts.push(account);
    }

    // Each category with the place the answers know it by.
    let mut places: Vec<String> = vec![];
    let mut order: Vec<String> = vec![];
    let mut add = |budget: &mut Budget, place: String, name: &str, group: &str| {
        if !order.iter().any(|g| g == group) {
            order.push(group.to_owned());
        }
        budget.categories.push(Category { extra: Map::new(), id: new_id(), name: name.to_owned(), group: group.to_owned(), target: 0 });
        places.push(place);
    };
    if let Some(id) = answers.template.as_deref().filter(|id| !id.trim().is_empty()) {
        let template = templates.iter().find(|t| t.id == id).ok_or_else(|| t!("There's no budget template called “{id}”.", id))?;
        for (g, group) in template.groups.iter().enumerate() {
            for (c, name) in group.categories.iter().enumerate() {
                let place = format!("{g}.{c}");
                if !answers.left_out.contains(&place) {
                    add(&mut budget, place, name, group.name);
                }
            }
        }
    }
    for (n, name) in answers.added.iter().enumerate() {
        if !name.trim().is_empty() {
            add(&mut budget, format!("+{n}"), name.trim(), own_group);
        }
    }
    if !order.is_empty() {
        budget.extra.insert("groupOrder".into(), json!(order));
    }
    // An amount for a category that was left out, or that a newer template no longer has, is
    // passed over: answers kept from an earlier sitting still build.
    for answer in answers.amounts.iter().filter(|a| a.amount > 0) {
        if let Some(i) = places.iter().position(|place| *place == answer.category) {
            budget.categories[i].target = match answer.period {
                Period::Month => answer.amount,
                Period::Year => answer.amount.saturating_add(11) / 12,
            };
        }
    }
    // A card's payment category, named after the card, after the person's own categories.
    for (account, name) in cards {
        budget.categories.push(Category {
            extra: Map::from_iter([("paymentAccountId".to_owned(), json!(account))]),
            id: new_id(),
            name,
            group: CARD_PAYMENTS.to_owned(),
            target: 0,
        });
    }
    domain::validate(&budget)?;
    Ok(budget)
}

/// What Quick assign › Underfunded would do to a budget in `month`, for one that isn't saved yet.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// Unassigned before anything is assigned.
    pub ready_to_assign: i64,
    /// The categories whose Assigned would change. Never more in all than Unassigned holds.
    pub changes: Vec<AssignChange>,
    /// What Underfunded would assign with money for all of it: what the month asks for.
    pub needed: i64,
}

pub fn preview(budget: &Budget, month: &str) -> Result<Preview, String> {
    if !valid_month(month) {
        return Err(t!("Invalid calculation reference month."));
    }
    domain::validate(budget)?;
    let mut budget = budget.clone();
    budget.extra.insert("calculationAsOfMonth".into(), json!(month));
    let ready_to_assign = domain::calculate(&budget, month)?.ready_to_assign;
    let categories: Vec<String> = budget.categories.iter().map(|c| c.id.clone()).collect();
    if categories.is_empty() {
        return Ok(Preview { ready_to_assign, changes: vec![], needed: 0 });
    }
    let underfunded = |limit: bool| -> Result<Vec<AssignChange>, String> {
        Ok(workflows::auto_assign_preview(&budget, month, &categories, limit)?.remove("underfunded").flatten().unwrap_or_default())
    };
    let needed = underfunded(false)?.iter().map(|c| c.after - c.before).sum();
    Ok(Preview { ready_to_assign, changes: underfunded(true)?, needed })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{arabic, tr_in, Language};

    const MONTH: &str = "2026-11";

    fn answers(value: Value) -> Answers {
        serde_json::from_value(value).unwrap()
    }
    fn category<'a>(b: &'a Budget, name: &str) -> &'a Category {
        b.categories.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no category {name}"))
    }
    /// The answers a first sitting gives: two cash accounts, a card owing 420, and what a month costs.
    fn household() -> Answers {
        answers(json!({
            "name": "Home", "currency": "USD",
            "numberFormat": {"decimal": ".", "group": ",", "symbol": "before"}, "dateFormat": "mdy",
            "accounts": [
                {"name": " Checking ", "type": "checking", "balance": 320000},
                {"name": "Wallet", "type": "cash", "balance": 50000},
                {"name": "Visa", "type": "creditCard", "balance": 42000},
                {"name": "  ", "type": "savings", "balance": 999}
            ],
            "template": "everyday",
            "amounts": [
                {"category": "0.0", "amount": 150000, "period": "month"},
                {"category": "1.0", "amount": 60000},
                {"category": "1.3", "amount": 90000, "period": "year"}
            ]
        }))
    }

    #[test]
    fn accounts_are_made_with_a_card_owing_its_balance_and_its_payment_category() {
        let budget = build(&household()).unwrap();
        assert_eq!((budget.name.as_str(), budget.currency.as_str()), ("Home", "USD"));
        assert_eq!(budget.extra["dateFormat"], "mdy");
        assert_eq!(budget.extra["numberFormat"]["symbol"], "before");
        // The row without a name isn't an account.
        let rows: Vec<_> = budget.accounts.iter().map(|a| (a.name.as_str(), a.kind, a.opening_balance, a.extra["accountType"].as_str().unwrap())).collect();
        assert_eq!(rows, [
            ("Checking", AccountKind::Cash, 320000, "checking"),
            ("Wallet", AccountKind::Cash, 50000, "cash"),
            ("Visa", AccountKind::Credit, -42000, "creditCard"),
        ]);
        assert!(budget.accounts.iter().all(|a| a.on_budget() && !a.archived));
        let payment = budget.categories.last().unwrap();
        assert_eq!((payment.name.as_str(), payment.group.as_str(), payment.target), ("Visa", CARD_PAYMENTS, 0));
        assert_eq!(payment.payment_account(), Some(budget.accounts[2].id.as_str()));
        // The card's group follows the template's, which keep their order.
        assert_eq!(budget.extra["groupOrder"], json!(["Bills", "Needs", "Wants", "Savings"]));
        assert_eq!(budget.categories.len(), 17);
        // Nothing is assigned, and the cash is all there to plan with.
        assert!(budget.assignments.is_empty() && budget.transactions.is_empty());
        assert_eq!(domain::calculate(&budget, MONTH).unwrap().ready_to_assign, 370000);
    }

    #[test]
    fn accounts_alone_make_a_budget_without_categories() {
        let budget = build(&answers(json!({"currency": "EUR", "accounts": [{"name": "Bank", "type": "savings", "balance": 100}]}))).unwrap();
        assert_eq!(budget.name, "My budget");
        assert!(budget.categories.is_empty() && !budget.extra.contains_key("groupOrder"));
        assert!(!budget.extra.contains_key("numberFormat") && !budget.extra.contains_key("expectedIncome"));
        // No answers at all is still a budget: Skip setup on the first step.
        let bare = build(&answers(json!({"currency": "EUR"}))).unwrap();
        assert!(bare.accounts.is_empty() && bare.categories.is_empty());
        assert!(build(&answers(json!({}))).is_err(), "a budget needs a currency");
        assert!(build(&answers(json!({"currency": "EUR", "template": "nope"}))).is_err());
        assert!(build(&answers(json!({"currency": "EUR", "template": "empty"}))).unwrap().categories.is_empty());
    }

    #[test]
    fn a_template_leaves_categories_out_and_takes_added_ones() {
        let budget = build(&answers(json!({
            "currency": "USD", "template": "everyday",
            // Water, and every category of Savings.
            "leftOut": ["0.2", "3.0", "3.1", "3.2", "9.9"],
            "added": ["Pets", " ", " Gym "],
            "amounts": [
                {"category": "0.2", "amount": 5000},
                {"category": "+0", "amount": 4000},
                {"category": "+1", "amount": 7000},
                {"category": "+2", "amount": 3000}
            ]
        }))).unwrap();
        let names: Vec<_> = budget.categories.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"Water") && !names.contains(&"Emergency fund"));
        assert_eq!(budget.categories.len(), 16 - 4 + 2);
        // A group with nothing left in it isn't made; added categories get their own.
        assert_eq!(budget.extra["groupOrder"], json!(["Bills", "Needs", "Wants", "Other"]));
        assert_eq!((category(&budget, "Pets").group.as_str(), category(&budget, "Pets").target), ("Other", 4000));
        // The blank one keeps its place, so "+2" is still Gym.
        assert_eq!((category(&budget, "Gym").group.as_str(), category(&budget, "Gym").target), ("Other", 3000));
        assert_eq!(budget.categories.iter().map(|c| c.target).sum::<i64>(), 7000);
        // Without a template the added ones are the budget's categories.
        let own = build(&answers(json!({"currency": "USD", "added": ["Rent"]}))).unwrap();
        assert_eq!(own.extra["groupOrder"], json!(["Other"]));
        assert_eq!(own.categories.len(), 1);
    }

    #[test]
    fn a_years_amount_is_saved_as_its_monthly_target_and_a_blank_leaves_none() {
        let mut given = household();
        given.amounts.extend([
            AmountAnswer { category: "0.1".into(), amount: 100000, period: Period::Year },
            AmountAnswer { category: "0.2".into(), amount: 0, period: Period::Month },
            AmountAnswer { category: "0.3".into(), amount: -500, period: Period::Year },
            AmountAnswer { category: "7.7".into(), amount: 500, period: Period::Month },
        ]);
        let budget = build(&given).unwrap();
        assert_eq!(category(&budget, "Rent or mortgage").target, 150000);
        assert_eq!(category(&budget, "Groceries").target, 60000);
        // 900 a year is 75 a month.
        assert_eq!(category(&budget, "Insurance").target, 7500);
        // 1,000 a year doesn't divide: 83.34, so twelve months cover it.
        assert_eq!(category(&budget, "Electricity").target, 8334);
        assert_eq!(category(&budget, "Water").target, 0);
        assert_eq!(category(&budget, "Internet").target, 0);
        let with_target = budget.categories.iter().filter(|c| c.target > 0).count();
        assert_eq!(with_target, 4);
        assert!(budget.categories.iter().all(|c| !c.extra.contains_key("goal")));
        // The plain monthly target is what the month asks for.
        let summary = domain::calculate(&budget, MONTH).unwrap();
        let needed = |name: &str| summary.categories.iter().find(|c| c.id == category(&budget, name).id).unwrap().needed;
        assert_eq!((needed("Insurance"), needed("Water")), (7500, 0));
    }

    #[test]
    fn expected_income_is_kept_when_given() {
        let mut given = household();
        assert!(!build(&given).unwrap().extra.contains_key("expectedIncome"));
        given.expected_income = Some(0);
        assert!(!build(&given).unwrap().extra.contains_key("expectedIncome"));
        given.expected_income = Some(500000);
        assert_eq!(build(&given).unwrap().extra["expectedIncome"], 500000);
        given.expected_income = Some(i64::MAX);
        assert!(build(&given).is_err());
    }

    #[test]
    fn the_same_answers_give_the_same_budget_in_arabic_apart_from_names() {
        let mut given = household();
        given.left_out = vec!["2.1".into()];
        given.added = vec!["Pets".into()];
        given.amounts.push(AmountAnswer { category: "+0".into(), amount: 12000, period: Period::Year });
        let english = budget_templates::list();
        let translated: Vec<BudgetTemplate> = english.iter().map(|t| BudgetTemplate {
            name: tr_in(Language::Arabic, t.name),
            groups: t.groups.iter().map(|g| budget_templates::TemplateGroup {
                name: tr_in(Language::Arabic, g.name),
                categories: g.categories.iter().map(|c| tr_in(Language::Arabic, c)).collect(),
            }).collect(),
            ..t.clone()
        }).collect();
        let en = assemble(&given, &english, "Other").unwrap();
        let ar = assemble(&given, &translated, arabic("Other").unwrap()).unwrap();
        assert_eq!(en.categories.len(), ar.categories.len());
        assert_eq!(category(&ar, "الإيجار أو قسط السكن").target, 150000);
        assert_eq!(ar.extra["groupOrder"][0], "الفواتير");
        assert_eq!(ar.extra["groupOrder"].as_array().unwrap().len(), en.extra["groupOrder"].as_array().unwrap().len());
        // Category by category: the same target, the same group (by its place) and the same link.
        let place = |b: &Budget, group: &str| b.extra["groupOrder"].as_array().unwrap().iter().position(|g| g == group);
        let card = |b: &Budget, c: &Category| c.payment_account().map(|id| b.accounts.iter().position(|a| a.id == id));
        for (a, b) in en.categories.iter().zip(&ar.categories) {
            assert_eq!((a.target, place(&en, &a.group), card(&en, a)), (b.target, place(&ar, &b.group), card(&ar, b)), "{} / {}", a.name, b.name);
        }
        // What the person typed is theirs in both.
        assert_eq!(category(&ar, "Pets").target, 1000);
        assert_eq!(category(&ar, "Visa").group, CARD_PAYMENTS);
        let plain = |b: &Budget| b.accounts.iter().map(|a| (a.name.clone(), a.kind, a.opening_balance)).collect::<Vec<_>>();
        assert_eq!(plain(&en), plain(&ar));
        assert_eq!((&en.name, &en.currency, &en.extra["dateFormat"]), (&ar.name, &ar.currency, &ar.extra["dateFormat"]));
    }

    #[test]
    fn the_preview_funds_targets_and_the_card_from_what_there_is() {
        let budget = build(&household()).unwrap();
        let seen = preview(&budget, MONTH).unwrap();
        assert_eq!(seen.ready_to_assign, 370000);
        let after = |name: &str| seen.changes.iter().find(|c| c.id == category(&budget, name).id).map(|c| (c.before, c.after));
        assert_eq!(after("Rent or mortgage"), Some((0, 150000)));
        assert_eq!(after("Insurance"), Some((0, 7500)));
        // The card owes 420: its payment category is funded too.
        assert_eq!(after("Visa"), Some((0, 42000)));
        assert_eq!(after("Water"), None);
        assert_eq!(seen.changes.iter().map(|c| c.after).sum::<i64>(), 150000 + 60000 + 7500 + 42000);
        assert_eq!(seen.needed, 259500);
        // The budget itself is untouched.
        assert!(budget.assignments.is_empty() && !budget.extra.contains_key("calculationAsOfMonth"));

        // With less than a month costs, it stops at what there is.
        let mut short = household();
        short.accounts.truncate(2);
        short.accounts[0].balance = 100000;
        short.accounts[1].balance = 0;
        let budget = build(&short).unwrap();
        let seen = preview(&budget, MONTH).unwrap();
        assert_eq!(seen.ready_to_assign, 100000);
        assert_eq!(seen.changes.iter().map(|c| c.after).sum::<i64>(), 100000);
        assert_eq!(seen.needed, 150000 + 60000 + 7500);

        let bare = build(&answers(json!({"currency": "USD"}))).unwrap();
        assert!(preview(&bare, MONTH).unwrap().changes.is_empty());
        assert!(preview(&bare, "2026-13").is_err());
    }
}
