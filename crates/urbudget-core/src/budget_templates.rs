//! Sets of categories a new budget can start with, for the welcome screens and New budget on
//! every app. Names are written in the language in use when the budget is made; from then on
//! they're the person's own names, renamed or deleted like any other (docs/LOCALIZATION.md).
//!
//! Every name is written out as `tr("…")` where it's used, so the translation test finds it.
//! No template has a credit card payments category: a credit account gets its own linked one.
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::budgets::new_id;
use crate::domain::{Budget, Category};
use crate::i18n::{self, tr};
use crate::t;

/// The template the apps start on.
pub const DEFAULT: &str = "everyday";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// One line to show under the choice: the groups and how many categories, or the
    /// description when there are none.
    pub summary: String,
    pub groups: Vec<TemplateGroup>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TemplateGroup {
    pub name: &'static str,
    pub categories: Vec<&'static str>,
}

fn group(name: &'static str, categories: &[&'static str]) -> TemplateGroup {
    TemplateGroup { name, categories: categories.to_vec() }
}

fn template(id: &'static str, name: &'static str, description: &'static str, groups: Vec<TemplateGroup>) -> BudgetTemplate {
    let n = groups.iter().map(|g| g.categories.len()).sum::<usize>();
    let summary = if n == 0 {
        description.to_owned()
    } else {
        let names: Vec<&str> = groups.iter().map(|g| g.name).collect();
        format!("{} · {}", names.join(i18n::comma()), t!("{n|# category|# categories}", n))
    };
    BudgetTemplate { id, name, description, summary, groups }
}

/// Every template, in the order the apps list them, in the language in use.
pub fn list() -> Vec<BudgetTemplate> {
    vec![
        template("everyday", tr("Everyday basics"), tr("Bills, needs, wants and savings. A good fit for most people."), vec![
            group(tr("Bills"), &[tr("Rent or mortgage"), tr("Electricity"), tr("Water"), tr("Internet"), tr("Phone")]),
            group(tr("Needs"), &[tr("Groceries"), tr("Transportation"), tr("Medical"), tr("Insurance")]),
            group(tr("Wants"), &[tr("Dining out"), tr("Entertainment"), tr("Shopping"), tr("Subscriptions")]),
            group(tr("Savings"), &[tr("Emergency fund"), tr("Vacation"), tr("Gifts")]),
        ]),
        template("simple", tr("Keep it simple"), tr("A few broad categories to start small. Split them up later."), vec![
            group(tr("Needs"), &[tr("Home"), tr("Food"), tr("Transportation"), tr("Bills")]),
            group(tr("Wants"), &[tr("Fun"), tr("Shopping")]),
            group(tr("Savings"), &[tr("Emergency fund"), tr("Goals")]),
        ]),
        template("family", tr("Family"), tr("Home, kids, family time and saving for what's ahead."), vec![
            group(tr("Home"), &[tr("Rent or mortgage"), tr("Utilities"), tr("Household supplies")]),
            group(tr("Kids"), &[tr("Childcare"), tr("School fees"), tr("Activities"), tr("Clothes")]),
            group(tr("Needs"), &[tr("Groceries"), tr("Transportation"), tr("Medical"), tr("Insurance")]),
            group(tr("Family time"), &[tr("Dining out"), tr("Outings"), tr("Gifts"), tr("Giving")]),
            group(tr("Savings"), &[tr("Emergency fund"), tr("Education"), tr("Vacation")]),
        ]),
        template("student", tr("Student"), tr("Tuition, living costs and a little fun on a tight budget."), vec![
            group(tr("School"), &[tr("Tuition"), tr("Books and supplies")]),
            group(tr("Living"), &[tr("Rent or mortgage"), tr("Groceries"), tr("Transportation"), tr("Phone")]),
            group(tr("Fun"), &[tr("Dining out"), tr("Going out"), tr("Subscriptions")]),
            group(tr("Savings"), &[tr("Emergency fund"), tr("Next term")]),
        ]),
        template("self-employed", tr("Self-employed"), tr("Set aside taxes and business costs, and save for months with less income."), vec![
            group(tr("Business"), &[tr("Taxes"), tr("Business expenses"), tr("Tools and software")]),
            group(tr("Home"), &[tr("Rent or mortgage"), tr("Utilities"), tr("Internet"), tr("Phone")]),
            group(tr("Living"), &[tr("Groceries"), tr("Transportation"), tr("Medical"), tr("Insurance")]),
            group(tr("Savings"), &[tr("Slow months"), tr("Emergency fund"), tr("Retirement")]),
        ]),
        template("empty", tr("No categories"), tr("Make your own as you go."), vec![]),
    ]
}

/// Adds a template's categories to a budget, after any it has, with their groups in order.
pub fn apply(budget: &mut Budget, id: &str) -> Result<(), String> {
    let template = list().into_iter().find(|t| t.id == id).ok_or_else(|| t!("There's no budget template called “{id}”.", id))?;
    if template.groups.is_empty() {
        return Ok(());
    }
    let mut order: Vec<String> = budget.extra.get("groupOrder").and_then(Value::as_array).into_iter().flatten()
        .filter_map(Value::as_str).map(str::to_owned).collect();
    for c in &budget.categories {
        if !order.contains(&c.group) {
            order.push(c.group.clone());
        }
    }
    for g in template.groups {
        if !order.iter().any(|name| name == g.name) {
            order.push(g.name.to_owned());
        }
        for name in g.categories {
            budget.categories.push(Category { extra: Map::new(), id: new_id(), name: name.to_owned(), group: g.name.to_owned(), target: 0 });
        }
    }
    budget.extra.insert("groupOrder".into(), json!(order));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn templates_have_unique_ids_and_no_repeated_names_within_one() {
        let all = list();
        let ids: HashSet<_> = all.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), all.len());
        assert!(ids.contains(DEFAULT));
        assert_eq!(all.last().map(|t| t.id), Some("empty"));
        for t in &all {
            let groups: HashSet<_> = t.groups.iter().map(|g| g.name).collect();
            assert_eq!(groups.len(), t.groups.len(), "{} repeats a group", t.id);
            let names: Vec<_> = t.groups.iter().flat_map(|g| g.categories.iter()).collect();
            assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len(), "{} repeats a category", t.id);
            assert!(!t.summary.is_empty());
        }
    }

    #[test]
    fn the_default_is_the_starter_set_the_apps_had() {
        let mut budget = Budget::default();
        apply(&mut budget, DEFAULT).unwrap();
        assert_eq!(budget.categories.len(), 16);
        assert_eq!(budget.extra["groupOrder"], json!(["Bills", "Needs", "Wants", "Savings"]));
        assert!(budget.categories.iter().all(|c| c.target == 0 && !c.id.is_empty()));
        let ids: HashSet<_> = budget.categories.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids.len(), 16);
        assert_eq!(list()[0].summary, "Bills, Needs, Wants, Savings · 16 categories");
    }

    #[test]
    fn applying_keeps_what_the_budget_has_and_refuses_an_unknown_template() {
        let mut budget = Budget::default();
        budget.categories.push(Category { extra: Map::new(), id: "own".into(), name: "Pets".into(), group: "Mine".into(), target: 500 });
        apply(&mut budget, "simple").unwrap();
        assert_eq!(budget.categories[0].id, "own");
        assert_eq!(budget.categories.len(), 9);
        assert_eq!(budget.extra["groupOrder"], json!(["Mine", "Needs", "Wants", "Savings"]));
        let before = budget.clone();
        apply(&mut budget, "empty").unwrap();
        assert_eq!(budget, before);
        assert!(apply(&mut budget, "nope").is_err());
        assert_eq!(budget, before);
    }
}
