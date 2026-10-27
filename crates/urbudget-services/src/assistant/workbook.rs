//! Spreadsheets the assistant puts together. The model describes the sheets; the app fills the
//! data sheets from the budget when the file is written, so every row and amount is exact. A
//! summary table the model wrote is checked cell by cell before the card is shown.
use super::proposals::{Card, Proposal, Row, Stat, Status};
use super::tools::{arg, date_arg, hidden, month_arg, month_label, strings, truncate, Names};
use super::{money, new_id, parse_money};
use crate::domain::*;
use crate::i18n::comma;
use crate::{engine, ledger, reports, t};
use rust_xlsxwriter::{Color, DocProperties, ExcelDateTime, Format, FormatAlign, FormatBorder, Formula, Workbook, XlsxError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Text,
    Money,
    Number,
    Percent,
    Date,
    Month,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub header: String,
    pub kind: Kind,
    /// Added up in the totals row.
    #[serde(default)]
    pub sum: bool,
}
fn col(header: &str, kind: Kind, sum: bool) -> Column {
    Column { header: header.into(), kind, sum }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Cell {
    Empty,
    Text(String),
    Money(i64),
    Number(f64),
    /// A fraction: 0.125 is 12.5%.
    Percent(f64),
    /// YYYY-MM-DD.
    Date(String),
    /// YYYY-MM.
    Month(String),
}

/// Which transactions a sheet lists. Everything is optional and combines.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(default)]
    pub account_ids: Vec<String>,
    #[serde(default)]
    pub category_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub uncategorized: bool,
    #[serde(default)]
    pub unapproved: bool,
}

/// Where a sheet's rows come from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Source {
    /// A summary the model wrote, already checked.
    Table { columns: Vec<Column>, rows: Vec<Vec<Cell>>, total: bool },
    Transactions { filter: Filter },
    Budget { month: String },
    #[serde(rename_all = "camelCase")]
    Spending { from: String, to: String, account_ids: Vec<String>, category_ids: Vec<String> },
    #[serde(rename_all = "camelCase")]
    Months { from: String, to: String, account_ids: Vec<String> },
}
impl Source {
    fn name(&self) -> &'static str {
        match self {
            Source::Table { .. } => "table",
            Source::Transactions { .. } => "transactions",
            Source::Budget { .. } => "budget",
            Source::Spending { .. } => "spending",
            Source::Months { .. } => "months",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub source: Source,
}

/// A spreadsheet waiting on its card. Data sheets are filled when the file is written.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkbookDoc {
    pub title: String,
    pub sheets: Vec<Sheet>,
    pub created: String,
}

/// A sheet with its rows, ready to write.
#[derive(Debug)]
pub struct Grid {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    pub total: bool,
}

const MAX_SHEETS: usize = 12;
const MAX_TABLE_ROWS: usize = 1000;
const MAX_MONTHS: usize = 60;

// --- Reading the tool's arguments --------------------------------------------------------------

/// The sheets the model asked for, checked against the budget.
pub fn sheets_from(args: &Value, b: &Budget, month: &str) -> Result<Vec<Sheet>, String> {
    let list = args["sheets"].as_array().filter(|l| !l.is_empty()).ok_or("Add at least one sheet.")?;
    if list.len() > MAX_SHEETS {
        return Err(format!("Use at most {MAX_SHEETS} sheets."));
    }
    let names = Names::new(b);
    let mut used = HashSet::new();
    list.iter()
        .enumerate()
        .map(|(i, s)| {
            let label = s["name"].as_str().unwrap_or("").trim();
            let source = source_from(s, &names, month)
                .map_err(|e| format!("Sheet {} ({}): {e}", i + 1, if label.is_empty() { "no name" } else { label }))?;
            Ok(Sheet { name: sheet_name(label, &format!("Sheet {}", i + 1), &mut used), source })
        })
        .collect()
}

fn source_from(s: &Value, names: &Names, month: &str) -> Result<Source, String> {
    let accounts = || -> Result<Vec<String>, String> {
        let ids = strings(s, "accountIds");
        match ids.iter().find(|id| !names.accounts.contains_key(id.as_str())) {
            Some(id) => Err(format!("No account has the id {id}. Use get_setup to find ids.")),
            None => Ok(ids),
        }
    };
    let categories = || -> Result<Vec<String>, String> {
        let ids = strings(s, "categoryIds");
        match ids.iter().find(|id| !names.categories.contains_key(id.as_str())) {
            Some(id) => Err(format!("No category has the id {id}. Use get_setup to find ids.")),
            None => Ok(ids),
        }
    };
    match s["type"].as_str().unwrap_or("") {
        "table" => table_from(s),
        "transactions" => {
            let filter = Filter {
                from: day_or_month(s, "from", false)?,
                to: day_or_month(s, "to", true)?,
                account_ids: accounts()?,
                category_ids: categories()?,
                payee: arg(s, "payee").map(|p| truncate(p, 200)),
                text: arg(s, "text").map(|p| truncate(p, 200)),
                uncategorized: s["uncategorized"] == json!(true),
                unapproved: s["unapproved"] == json!(true),
            };
            if let (Some(f), Some(t)) = (&filter.from, &filter.to) {
                if f > t {
                    return Err("from is after to.".into());
                }
            }
            Ok(Source::Transactions { filter })
        }
        "budget" => Ok(Source::Budget { month: month_arg(s, "month", month)? }),
        "spending" => {
            let (from, to) = range(s, month)?;
            Ok(Source::Spending { from, to, account_ids: accounts()?, category_ids: categories()? })
        }
        "months" => {
            let (from, to) = range(s, month)?;
            Ok(Source::Months { from, to, account_ids: accounts()? })
        }
        "" => Err("Give the sheet a type: transactions, budget, spending, months or table.".into()),
        other => Err(format!("There is no sheet type \"{other}\". Use transactions, budget, spending, months or table.")),
    }
}

/// A date, or a month taken as its first (or last) day.
fn day_or_month(s: &Value, key: &str, end: bool) -> Result<Option<String>, String> {
    match arg(s, key) {
        Some(m) if m.len() == 7 => {
            let first = chrono::NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d")
                .map_err(|_| format!("{key} must be a date like 2026-10-31."))?;
            let day = if end { first + chrono::Months::new(1) - chrono::Days::new(1) } else { first };
            Ok(Some(day.format("%Y-%m-%d").to_string()))
        }
        _ => date_arg(s, key),
    }
}

fn range(s: &Value, month: &str) -> Result<(String, String), String> {
    let to = month_arg(s, "to", month)?;
    let from = month_arg(s, "from", &to)?;
    if from > to {
        return Err("from is after to.".into());
    }
    if months_between(&from, &to).len() > MAX_MONTHS {
        return Err(format!("Cover at most {MAX_MONTHS} months."));
    }
    Ok((from, to))
}

fn months_between(from: &str, to: &str) -> Vec<String> {
    let mut out = vec![];
    let Ok(mut d) = chrono::NaiveDate::parse_from_str(&format!("{from}-01"), "%Y-%m-%d") else { return out };
    loop {
        let m = d.format("%Y-%m").to_string();
        if m.as_str() > to || out.len() > MAX_MONTHS {
            return out;
        }
        out.push(m);
        d = d + chrono::Months::new(1);
    }
}

fn table_from(s: &Value) -> Result<Source, String> {
    let list = s["columns"].as_array().filter(|c| !c.is_empty()).ok_or("A table needs its columns.")?;
    if list.len() > 20 {
        return Err("Use at most 20 columns.".into());
    }
    let columns = list
        .iter()
        .map(|c| {
            let header = c["header"].as_str().map(str::trim).filter(|h| !h.is_empty()).ok_or("Each column needs a header.")?;
            let kind = match c["type"].as_str().unwrap_or("text") {
                "text" => Kind::Text,
                "money" => Kind::Money,
                "number" => Kind::Number,
                "percent" => Kind::Percent,
                "date" => Kind::Date,
                "month" => Kind::Month,
                other => return Err(format!("There is no column type \"{other}\".")),
            };
            Ok(col(&truncate(header, 80), kind, matches!(kind, Kind::Money | Kind::Number)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let rows = s["rows"].as_array().ok_or("A table needs its rows.")?;
    if rows.len() > MAX_TABLE_ROWS {
        return Err(format!(
            "A table holds at most {MAX_TABLE_ROWS} rows. For a longer list, use a transactions sheet; the app fills it in."
        ));
    }
    let rows = rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            let cells = row.as_array().ok_or_else(|| format!("Row {} isn't a list of cells.", r + 1))?;
            if cells.len() > columns.len() {
                return Err(format!("Row {} has {} cells for {} columns.", r + 1, cells.len(), columns.len()));
            }
            columns
                .iter()
                .enumerate()
                .map(|(c, column)| {
                    cell(cells.get(c).unwrap_or(&Value::Null), column.kind)
                        .map_err(|e| format!("Row {}, {}: {e}", r + 1, column.header))
                })
                .collect()
        })
        .collect::<Result<Vec<Vec<Cell>>, String>>()?;
    Ok(Source::Table { columns, rows, total: s["total"] == json!(true) })
}

fn cell(v: &Value, kind: Kind) -> Result<Cell, String> {
    if v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty()) {
        return Ok(Cell::Empty);
    }
    let text = || match v {
        Value::String(s) => s.trim().to_owned(),
        other => other.to_string(),
    };
    Ok(match kind {
        Kind::Text => Cell::Text(truncate(&text(), 500)),
        Kind::Money => Cell::Money(parse_money(v)?),
        Kind::Number => Cell::Number(number(&text()).ok_or("expected a number")?),
        Kind::Percent => Cell::Percent(number(text().trim_end_matches('%')).ok_or("expected a percentage like 12.5")? / 100.0),
        Kind::Date => match chrono::NaiveDate::parse_from_str(&text(), "%Y-%m-%d") {
            Ok(_) => Cell::Date(text()),
            Err(_) => return Err("expected a date like 2026-10-31".into()),
        },
        Kind::Month => match chrono::NaiveDate::parse_from_str(&format!("{}-01", text()), "%Y-%m-%d") {
            Ok(_) if text().len() == 7 => Cell::Month(text()),
            _ => return Err("expected a month like 2026-10".into()),
        },
    })
}

fn number(text: &str) -> Option<f64> {
    let plain: String = text.chars().filter(|c| !matches!(c, ',' | ' ' | '\u{a0}' | '\u{202f}')).collect();
    plain.replace('\u{2212}', "-").parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A name Excel accepts: at most 31 characters, none of []:*?/\, unique in the workbook.
fn sheet_name(wanted: &str, fallback: &str, used: &mut HashSet<String>) -> String {
    let clean: String = wanted.chars().map(|c| if "[]:*?/\\".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let mut base: String = clean.split_whitespace().collect::<Vec<_>>().join(" ").trim_matches('\'').chars().take(31).collect();
    if base.trim().is_empty() {
        base = fallback.into();
    }
    if base.eq_ignore_ascii_case("history") {
        base = format!("{base} 1");
    }
    let mut name = base.trim().to_owned();
    let mut n = 2;
    while used.contains(&name.to_lowercase()) {
        let suffix = format!(" ({n})");
        name = format!("{}{suffix}", base.chars().take(31 - suffix.chars().count()).collect::<String>().trim_end());
        n += 1;
    }
    used.insert(name.to_lowercase());
    name
}

// --- Filling sheets from the budget ------------------------------------------------------------

/// A sheet's rows, from the budget as it is now.
pub fn fill(sheet: &Sheet, b: &Budget) -> Result<Grid, String> {
    let (columns, rows, total) = match &sheet.source {
        Source::Table { columns, rows, total } => (columns.clone(), rows.clone(), *total),
        Source::Transactions { filter } => transactions(filter, b),
        Source::Budget { month } => budget(month, b)?,
        Source::Spending { from, to, account_ids, category_ids } => spending(from, to, account_ids, category_ids, b)?,
        Source::Months { from, to, account_ids } => months(from, to, account_ids, b)?,
    };
    Ok(Grid { name: sheet.name.clone(), columns, rows, total })
}

type Rows = (Vec<Column>, Vec<Vec<Cell>>, bool);

/// One row per split, plus the other side of each transfer, oldest first: what the account
/// registers show.
fn transactions(f: &Filter, b: &Budget) -> Rows {
    let names = Names::new(b);
    let accounts: HashSet<&str> = f.account_ids.iter().map(String::as_str).collect();
    let categories: HashSet<&str> = f.category_ids.iter().map(String::as_str).collect();
    let payee = f.payee.as_ref().map(|p| p.to_lowercase());
    let text = f.text.as_ref().map(|p| p.to_lowercase());
    struct Line<'a> {
        account: &'a Account,
        payee: String,
        group: String,
        category: String,
        category_id: Option<String>,
        memo: String,
        amount: i64,
        cleared: Cleared,
        uncategorized: bool,
    }
    let legs = ledger::transactions(b);
    let mut found: Vec<(&str, Line)> = vec![];
    for t in &legs {
        if f.from.as_deref().is_some_and(|d| t.date.as_str() < d) || f.to.as_deref().is_some_and(|d| t.date.as_str() > d) {
            continue;
        }
        if f.unapproved && t.extra.get("approved") != Some(&json!(false)) {
            continue;
        }
        let Some(account) = names.accounts.get(t.account_id.as_str()).copied() else { continue };
        let peer = t.transfer_account_id.as_deref().and_then(|id| names.accounts.get(id).copied());
        let whole = [Split { extra: Default::default(), category_id: None, amount: t.amount }];
        let splits: &[Split] = if t.splits.is_empty() { &whole } else { &t.splits };
        let mut lines = vec![];
        for s in splits {
            let (group, category, uncategorized) = match s.category_id.as_deref() {
                Some(id) => match names.categories.get(id) {
                    Some(c) => (c.group.clone(), c.name.clone(), false),
                    None => (String::new(), id.to_owned(), false),
                },
                None if peer.is_some() || !account.on_budget() => (String::new(), String::new(), false),
                None if s.amount >= 0 => ("Inflow".into(), "Ready to Assign".into(), false),
                None => (String::new(), "Uncategorized".into(), true),
            };
            let memo = if splits.len() > 1 {
                s.extra.get("memo").and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or(&t.memo)
            } else {
                &t.memo
            };
            let payee = match peer {
                Some(p) => format!("Transfer: {}", p.name),
                None => s.extra.get("payee").and_then(Value::as_str).filter(|p| !p.is_empty()).unwrap_or(&t.payee).to_owned(),
            };
            lines.push(Line {
                account,
                payee,
                group,
                category,
                category_id: s.category_id.clone(),
                memo: memo.to_owned(),
                amount: s.amount,
                cleared: t.status,
                uncategorized,
            });
        }
        if let Some(p) = peer {
            let source = t.extra.get("transferSource").unwrap_or(&Value::Null);
            lines.push(Line {
                account: p,
                payee: format!("Transfer: {}", account.name),
                group: String::new(),
                category: String::new(),
                category_id: None,
                memo: source["memo"].as_str().unwrap_or(&t.memo).to_owned(),
                amount: -t.amount,
                cleared: t.transfer_status.unwrap_or(t.status),
                uncategorized: false,
            });
        }
        for line in lines {
            let keep = (accounts.is_empty() || accounts.contains(line.account.id.as_str()))
                && (categories.is_empty() || line.category_id.as_deref().is_some_and(|c| categories.contains(c)))
                && payee.as_deref().is_none_or(|p| line.payee.to_lowercase() == p)
                && (!f.uncategorized || line.uncategorized)
                && text.as_deref().is_none_or(|q| {
                    line.payee.to_lowercase().contains(q)
                        || line.memo.to_lowercase().contains(q)
                        || line.category.to_lowercase().contains(q)
                });
            if keep {
                found.push((t.date.as_str(), line));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(b.0));
    let columns = vec![
        col("Date", Kind::Date, false),
        col("Account", Kind::Text, false),
        col("Payee", Kind::Text, false),
        col("Category group", Kind::Text, false),
        col("Category", Kind::Text, false),
        col("Memo", Kind::Text, false),
        col("Amount", Kind::Money, true),
        col("Cleared", Kind::Text, false),
    ];
    let rows = found
        .into_iter()
        .map(|(date, l)| {
            vec![
                Cell::Date(date.to_owned()),
                Cell::Text(l.account.name.clone()),
                Cell::Text(l.payee),
                text_cell(l.group),
                text_cell(l.category),
                text_cell(l.memo),
                Cell::Money(l.amount),
                Cell::Text(format!("{:?}", l.cleared)),
            ]
        })
        .collect();
    (columns, rows, true)
}

fn text_cell(s: String) -> Cell {
    if s.is_empty() {
        Cell::Empty
    } else {
        Cell::Text(s)
    }
}

/// One month's categories, as the budget page shows them.
fn budget(month: &str, b: &Budget) -> Result<Rows, String> {
    let s = engine::calculate(b, month)?;
    let by_id: HashMap<&str, &CategorySummary> = s.categories.iter().map(|c| (c.id.as_str(), c)).collect();
    let rows = b
        .categories
        .iter()
        .filter_map(|c| {
            let r = by_id.get(c.id.as_str())?;
            if hidden(c) && r.assigned == 0 && r.activity == 0 && r.available == 0 {
                return None;
            }
            Some(vec![
                Cell::Text(c.group.clone()),
                Cell::Text(c.name.clone()),
                Cell::Money(r.assigned),
                Cell::Money(r.activity),
                Cell::Money(r.available),
                if c.target > 0 { Cell::Money(c.target) } else { Cell::Empty },
            ])
        })
        .collect();
    let columns = vec![
        col("Category group", Kind::Text, false),
        col("Category", Kind::Text, false),
        col("Assigned", Kind::Money, true),
        col("Activity", Kind::Money, true),
        col("Available", Kind::Money, true),
        col("Target", Kind::Money, false),
    ];
    Ok((columns, rows, true))
}

fn report(from: &str, to: &str, account_ids: &[String], category_ids: &[String], b: &Budget) -> Result<reports::Report, String> {
    reports::calculate(
        b,
        &reports::Filter { from: from.into(), to: to.into(), account_ids: account_ids.to_vec(), category_ids: category_ids.to_vec() },
    )
}

fn short_month(month: &str) -> String {
    chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").map_or_else(|_| month.to_owned(), |d| d.format("%b %Y").to_string())
}

/// Spending in each category, one column per month, with the total and monthly average.
fn spending(from: &str, to: &str, account_ids: &[String], category_ids: &[String], b: &Budget) -> Result<Rows, String> {
    let r = report(from, to, account_ids, category_ids, b)?;
    let months: Vec<&reports::Month> = r.months.iter().filter(|m| m.month.as_str() >= from && m.month.as_str() <= to).collect();
    let mut columns = vec![col("Category group", Kind::Text, false), col("Category", Kind::Text, false)];
    columns.extend(months.iter().map(|m| col(&short_month(&m.month), Kind::Money, true)));
    columns.push(col("Total", Kind::Money, true));
    columns.push(col("Monthly average", Kind::Money, true));
    let mut order: Vec<(String, String, &str)> =
        b.categories.iter().map(|c| (c.group.clone(), c.name.clone(), c.id.as_str())).collect();
    for id in r.categories.keys() {
        if !order.iter().any(|(_, _, known)| known == id) {
            order.push((String::new(), id.clone(), id.as_str()));
        }
    }
    let rows = order
        .into_iter()
        .filter_map(|(group, name, id)| {
            let values: Vec<i64> = months.iter().map(|m| m.categories.get(id).copied().unwrap_or(0)).collect();
            let total: i64 = values.iter().sum();
            if values.iter().all(|v| *v == 0) {
                return None;
            }
            let mut row = vec![text_cell(group), Cell::Text(name)];
            row.extend(values.into_iter().map(Cell::Money));
            row.push(Cell::Money(total));
            row.push(Cell::Money((total as f64 / months.len().max(1) as f64).round() as i64));
            Some(row)
        })
        .collect();
    Ok((columns, rows, true))
}

/// Income, spending and net worth for each month.
fn months(from: &str, to: &str, account_ids: &[String], b: &Budget) -> Result<Rows, String> {
    let r = report(from, to, account_ids, &[], b)?;
    let columns = vec![
        col("Month", Kind::Month, false),
        col("Income", Kind::Money, true),
        col("Spending", Kind::Money, true),
        col("Net income", Kind::Money, true),
        col("Net worth", Kind::Money, false),
    ];
    let rows = r
        .months
        .iter()
        .filter(|m| m.month.as_str() >= from && m.month.as_str() <= to)
        .map(|m| {
            vec![
                Cell::Month(m.month.clone()),
                Cell::Money(m.income),
                Cell::Money(m.spending),
                Cell::Money(m.net_income),
                Cell::Money(m.net_worth),
            ]
        })
        .collect();
    Ok((columns, rows, true))
}

// --- The card ----------------------------------------------------------------------------------

/// A range of months as the card names it, in the app's language.
fn range_label(from: &str, to: &str) -> String {
    if from == to {
        month_label(from)
    } else {
        let short = |m: &str| {
            chrono::NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").map_or_else(|_| m.to_owned(), crate::i18n::short_month_year)
        };
        format!("{} – {}", short(from), short(to))
    }
}

fn day_label(day: &str) -> String {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").map_or_else(|_| day.to_owned(), crate::i18n::day_month_year)
}

fn rows_badge(n: usize) -> String {
    t!("{n|# row|# rows}", n)
}
fn categories_badge(n: usize) -> String {
    t!("{n|# category|# categories}", n)
}
fn months_badge(n: usize) -> String {
    t!("{n|# month|# months}", n)
}

/// What a sheet holds, and how its row count reads.
fn describe(source: &Source, names: &Names) -> (String, fn(usize) -> String) {
    let list = |ids: &[String], name: &dyn Fn(&str) -> String| -> String {
        let shown: Vec<String> = ids.iter().take(2).map(|id| name(id)).collect();
        match ids.len() {
            0 | 1 => shown.concat(),
            2 => t!("{first} and {second}", first = shown[0], second = shown[1]),
            n => t!("{shown} and {count} more", shown = shown.join(comma()), count = n - 2),
        }
    };
    match source {
        Source::Table { .. } => (t!("Summary from the assistant"), rows_badge),
        Source::Transactions { filter: f } => {
            let mut parts = vec![];
            if !f.category_ids.is_empty() {
                parts.push(list(&f.category_ids, &|id| names.category_shown(Some(id))));
            }
            if !f.account_ids.is_empty() {
                parts.push(list(&f.account_ids, &|id| names.account(id)));
            }
            if let Some(p) = f.payee.as_ref().or(f.text.as_ref()) {
                parts.push(t!("\u{201c}{text}\u{201d}", text = truncate(p, 30)));
            }
            if f.uncategorized {
                parts.push(t!("uncategorized"));
            }
            if f.unapproved {
                parts.push(t!("waiting for approval"));
            }
            match (&f.from, &f.to) {
                (Some(a), Some(b)) => parts.push(format!("{} – {}", day_label(a), day_label(b))),
                (Some(a), None) => parts.push(t!("since {date}", date = day_label(a))),
                (None, Some(b)) => parts.push(t!("through {date}", date = day_label(b))),
                (None, None) => {}
            }
            let what = if parts.is_empty() { t!("All transactions") } else { t!("Transactions: {filters}", filters = parts.join(comma())) };
            (what, rows_badge)
        }
        Source::Budget { month } => (t!("Budget for {month}", month = month_label(month)), categories_badge),
        Source::Spending { from, to, .. } => (t!("Spending by category, {range}", range = range_label(from, to)), categories_badge),
        Source::Months { from, to, .. } => (t!("Income and spending, {range}", range = range_label(from, to)), months_badge),
    }
}

/// A spreadsheet card: one row per sheet with what it holds, ready to open or save.
pub fn build(title: String, sheets: Vec<Sheet>, b: &Budget) -> Result<Proposal, String> {
    let names = Names::new(b);
    let mut rows = vec![];
    let mut summary = vec![];
    let mut count = 0;
    let mut empty = false;
    for s in &sheets {
        let n = fill(s, b)?.rows.len();
        let (what, badge) = describe(&s.source, &names);
        count += n;
        empty |= n == 0;
        rows.push(Row {
            label: s.name.clone(),
            detail: Some(what),
            badge: Some(if n == 0 { t!("Empty") } else { badge(n) }),
            tone: (n == 0).then(|| "warn".into()),
            ..Row::default()
        });
        summary.push(json!({ "name": s.name, "type": s.source.name(), "rows": n }));
    }
    let from_budget = sheets.iter().any(|s| !matches!(s.source, Source::Table { .. }));
    let card = Card {
        kicker: t!("Spreadsheet"),
        rows,
        stats: vec![
            Stat { label: t!("Sheets"), text: Some(sheets.len().to_string()), amount: None },
            Stat { label: t!("Rows"), text: Some(thousands(count)), amount: None },
        ],
        warning: empty.then(|| t!("A sheet is empty: nothing in the budget matched it.")),
        note: Some(if from_budget {
            t!("Rows from your budget are filled in when you open or save it, so they're exact.")
        } else {
            t!("Open it in your spreadsheet app, or save a copy.")
        }),
        ..Card::default()
    };
    Ok(Proposal {
        id: new_id("p"),
        title: title.clone(),
        kind: "spreadsheet".into(),
        status: Status::Proposed,
        change: None,
        import: None,
        report: None,
        workbook: Some(WorkbookDoc { title: title.clone(), sheets, created: super::now() }),
        saved: None,
        expected: None,
        card,
        summary: json!({ "title": title, "sheets": summary }),
        problem: None,
        fixes: None,
        review: None,
    })
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// --- Reports -----------------------------------------------------------------------------------

/// The tables in a report's Markdown, one sheet each, named for the heading above them.
pub fn report_tables(body: &str) -> Vec<Grid> {
    let lines: Vec<&str> = body.lines().map(str::trim).collect();
    let mut grids = vec![];
    let mut used = HashSet::new();
    let mut heading = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with('#') {
            heading = plain(line.trim_start_matches('#'));
        } else if line.contains('|') && lines.get(i + 1).is_some_and(|l| separator(l)) {
            let header = cells(line);
            let right: Vec<bool> = cells(lines[i + 1]).iter().map(|c| c.ends_with(':') && !c.starts_with(':')).collect();
            let mut body = vec![];
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].is_empty() {
                body.push(cells(lines[i]));
                i += 1;
            }
            let fallback = format!("Table {}", grids.len() + 1);
            grids.push(table_grid(sheet_name(&heading, &fallback, &mut used), header, &right, body));
            continue;
        }
        i += 1;
    }
    grids
}

fn separator(line: &str) -> bool {
    line.contains('-')
        && line.contains('|')
        && cells(line).iter().all(|c| {
            let c = c.trim_matches(':');
            !c.is_empty() && c.chars().all(|x| x == '-')
        })
}

fn cells(line: &str) -> Vec<String> {
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(plain).collect()
}

/// A cell's text without Markdown emphasis or code marks.
fn plain(s: &str) -> String {
    s.replace("**", "").replace("__", "").replace('`', "").trim().trim_matches('*').trim().to_owned()
}

fn table_grid(name: String, header: Vec<String>, right: &[bool], body: Vec<Vec<String>>) -> Grid {
    let width = header.len();
    let kinds: Vec<Kind> = (0..width)
        .map(|c| {
            let values: Vec<&str> = body.iter().filter_map(|r| r.get(c)).map(String::as_str).filter(|v| !v.is_empty()).collect();
            infer(&values, right.get(c).copied().unwrap_or(false))
        })
        .collect();
    let columns = header.iter().zip(&kinds).map(|(h, k)| col(h, *k, false)).collect();
    let rows = body
        .iter()
        .map(|r| {
            (0..width)
                .map(|c| {
                    let v = r.get(c).map_or("", String::as_str);
                    if v.is_empty() {
                        return Cell::Empty;
                    }
                    match kinds[c] {
                        Kind::Money => amount(v).map_or_else(|| Cell::Text(v.into()), |n| Cell::Money((n * 100.0).round() as i64)),
                        Kind::Number => amount(v).map_or_else(|| Cell::Text(v.into()), Cell::Number),
                        Kind::Percent => amount(v.trim_end_matches('%')).map_or_else(|| Cell::Text(v.into()), |n| Cell::Percent(n / 100.0)),
                        Kind::Date => Cell::Date(v.into()),
                        Kind::Month | Kind::Text => Cell::Text(v.into()),
                    }
                })
                .collect()
        })
        .collect();
    Grid { name, columns, rows, total: false }
}

/// A column's type from its cells: dates, percentages, amounts or plain numbers, else text.
fn infer(values: &[&str], right: bool) -> Kind {
    if values.is_empty() {
        return Kind::Text;
    }
    if values.iter().all(|v| chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_ok()) {
        return Kind::Date;
    }
    if values.iter().all(|v| v.ends_with('%') && amount(v.trim_end_matches('%')).is_some()) {
        return Kind::Percent;
    }
    if values.iter().all(|v| amount(v).is_some()) {
        let cents = |v: &&str| v.rsplit_once('.').is_some_and(|(_, d)| d.len() == 2 && d.chars().all(|c| c.is_ascii_digit()));
        return if values.iter().any(cents) || (right && values.iter().any(|v| v.contains(','))) { Kind::Money } else { Kind::Number };
    }
    Kind::Text
}

/// A number as the report wrote it: 1,240.50, -12, (35.00), +4.5 or SAR 1,240.50.
fn amount(text: &str) -> Option<f64> {
    let mut t = text.trim().replace(['\u{2212}', '\u{2013}'], "-");
    for symbol in ['$', '€', '£', '¥', '₹', '﷼'] {
        t = t.trim_start_matches(symbol).trim_end_matches(symbol).trim().to_owned();
    }
    let code = |s: &str| s.len() == 3 && s.chars().all(|c| c.is_ascii_uppercase());
    if let Some((a, b)) = t.split_once(' ') {
        if code(a) {
            t = b.trim().to_owned();
        } else if code(b.trim()) {
            t = a.trim().to_owned();
        }
    }
    let negative = t.starts_with('(') && t.ends_with(')');
    let t = t.trim_matches(|c| c == '(' || c == ')').trim_start_matches('+');
    if !t.chars().any(|c| c.is_ascii_digit()) || !t.chars().all(|c| c.is_ascii_digit() || ",.- \u{a0}".contains(c)) {
        return None;
    }
    number(t).map(|n| if negative { -n } else { n })
}

// --- Writing the file --------------------------------------------------------------------------

/// Arabic, Hebrew and their presentation forms: sheets in these languages read right to left.
fn right_to_left(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'))
}

fn column_letter(mut c: usize) -> String {
    let mut s = vec![];
    loop {
        s.push((b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.iter().rev().collect()
}

/// How wide a cell's text is, in characters.
fn shown_width(cell: &Cell) -> usize {
    match cell {
        Cell::Empty => 0,
        Cell::Text(s) => s.chars().count(),
        Cell::Money(n) => {
            let digits = (n.unsigned_abs() / 100).to_string().len();
            digits + (digits - 1) / 3 + 3 + usize::from(*n < 0)
        }
        Cell::Number(n) => format!("{n}").len() + 2,
        Cell::Percent(_) => 7,
        Cell::Date(_) => 10,
        Cell::Month(_) => 8,
    }
}

/// The workbook as .xlsx bytes: a bold header that stays in view, filters, real numbers and
/// dates, and a totals row where the sheet adds up.
pub fn write(title: &str, grids: &[Grid]) -> Result<Vec<u8>, String> {
    let x = |e: XlsxError| t!("Couldn't write the spreadsheet: {e}", e);
    let mut book = Workbook::new();
    book.set_properties(&DocProperties::new().set_title(title).set_author("UrBudget"));
    let rtl = right_to_left(title) || grids.iter().any(|g| right_to_left(&g.name));
    let line = Color::RGB(0xC9CDD4);
    let head = Format::new()
        .set_bold()
        .set_background_color(Color::RGB(0xF2F3F5))
        .set_border_bottom(FormatBorder::Thin)
        .set_border_bottom_color(line);
    let money = Format::new().set_num_format("#,##0.00");
    let whole = Format::new().set_num_format("#,##0");
    let decimal = Format::new().set_num_format("#,##0.00");
    let percent = Format::new().set_num_format("0.0%");
    let date = Format::new().set_num_format("yyyy-mm-dd").set_align(FormatAlign::Left);
    let month = Format::new().set_num_format("mmm yyyy").set_align(FormatAlign::Left);
    let text = Format::new();
    let total = |f: &Format| f.clone().set_bold().set_border_top(FormatBorder::Thin).set_border_top_color(line);
    for g in grids {
        let sheet = book.add_worksheet();
        sheet.set_name(&g.name).map_err(x)?;
        sheet.set_right_to_left(rtl);
        let numeric: Vec<bool> = g.columns.iter().map(|c| !matches!(c.kind, Kind::Text | Kind::Date | Kind::Month)).collect();
        let formats: Vec<&Format> = g
            .columns
            .iter()
            .enumerate()
            .map(|(c, column)| match column.kind {
                Kind::Money => &money,
                Kind::Number if g.rows.iter().all(|r| !matches!(r[c], Cell::Number(n) if n.fract() != 0.0)) => &whole,
                Kind::Number => &decimal,
                Kind::Percent => &percent,
                Kind::Date => &date,
                Kind::Month => &month,
                Kind::Text => &text,
            })
            .collect();
        for (c, column) in g.columns.iter().enumerate() {
            let f = if numeric[c] { head.clone().set_align(FormatAlign::Right) } else { head.clone() };
            sheet.write_string_with_format(0, c as u16, &column.header, &f).map_err(x)?;
        }
        for (r, row) in g.rows.iter().enumerate() {
            let r = r as u32 + 1;
            for (c, value) in row.iter().enumerate() {
                let (c, f) = (c as u16, formats[c]);
                match value {
                    Cell::Empty => continue,
                    Cell::Text(s) => sheet.write_string(r, c, s),
                    Cell::Money(n) => sheet.write_number_with_format(r, c, *n as f64 / 100.0, f),
                    Cell::Number(n) | Cell::Percent(n) => sheet.write_number_with_format(r, c, *n, f),
                    Cell::Date(d) => sheet.write_datetime_with_format(r, c, ExcelDateTime::parse_from_str(d).map_err(x)?, f),
                    Cell::Month(m) => sheet.write_datetime_with_format(r, c, ExcelDateTime::parse_from_str(&format!("{m}-01")).map_err(x)?, f),
                }
                .map_err(x)?;
            }
        }
        let last = g.rows.len() as u32;
        if g.total && !g.rows.is_empty() && g.columns.iter().any(|c| c.sum) {
            let r = last + 1;
            for (c, column) in g.columns.iter().enumerate() {
                let f = total(formats[c]);
                let c16 = c as u16;
                if !column.sum {
                    if c == 0 {
                        sheet.write_string_with_format(r, 0, "Total", &f).map_err(x)?;
                    } else {
                        sheet.write_blank(r, c16, &f).map_err(x)?;
                    }
                    continue;
                }
                // The cached result shows in previews that don't recalculate; Excel recalculates.
                let result = match column.kind {
                    Kind::Money => money_total(g.rows.iter().map(|row| &row[c])),
                    _ => format!("{}", g.rows.iter().map(|row| if let Cell::Number(n) = row[c] { n } else { 0.0 }).sum::<f64>()),
                };
                let letter = column_letter(c);
                let formula = Formula::new(format!("=SUBTOTAL(9,{letter}2:{letter}{})", last + 1)).set_result(result);
                sheet.write_formula_with_format(r, c16, formula, &f).map_err(x)?;
            }
        }
        sheet.set_freeze_panes(1, 0).map_err(x)?;
        if !g.rows.is_empty() {
            sheet.autofilter(0, 0, last, g.columns.len().saturating_sub(1) as u16).map_err(x)?;
        }
        for (c, column) in g.columns.iter().enumerate() {
            let widest = g.rows.iter().map(|r| shown_width(&r[c])).max().unwrap_or(0).max(column.header.chars().count() + 3);
            sheet.set_column_width(c as u16, (widest + 2).clamp(8, 48) as f64).map_err(x)?;
        }
        sheet.set_repeat_rows(0, 0).map_err(x)?;
        sheet.set_print_fit_to_pages(1, 0);
        if g.columns.len() > 6 {
            sheet.set_landscape();
        }
    }
    book.save_to_buffer().map_err(x)
}

fn money_total<'a>(cells: impl Iterator<Item = &'a Cell>) -> String {
    money(cells.map(|c| if let Cell::Money(n) = c { *n } else { 0 }).sum())
}

/// A file name for the title that isn't taken yet in `dir`: "Title.xlsx", "Title (2).xlsx", …
pub fn free_path(dir: &Path, title: &str) -> PathBuf {
    let stem = stem(title);
    let mut path = dir.join(format!("{stem}.xlsx"));
    let mut n = 2;
    while path.exists() && n < 1000 {
        path = dir.join(format!("{stem} ({n}).xlsx"));
        n += 1;
    }
    path
}

/// The file name a workbook with this title is saved under: "Title.xlsx".
pub fn file_name(title: &str) -> String {
    format!("{}.xlsx", stem(title))
}

fn stem(title: &str) -> String {
    let stem: String = title.chars().map(|c| if c.is_alphanumeric() || " -_()".contains(c) { c } else { ' ' }).collect();
    let stem: String = stem.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
    if stem.trim().is_empty() { "Spreadsheet".to_owned() } else { stem.trim().to_owned() }
}

#[cfg(test)]
mod tests {
    use super::super::tools::tests::sample;
    use super::*;
    use calamine::{Data, Reader, Xlsx};

    fn sheets(args: Value) -> Vec<Sheet> {
        sheets_from(&args, &sample(), "2026-10").unwrap()
    }
    fn read_back(bytes: Vec<u8>) -> Xlsx<std::io::Cursor<Vec<u8>>> {
        Xlsx::new(std::io::Cursor::new(bytes)).expect("a valid xlsx file")
    }

    #[test]
    fn transactions_come_from_the_budget_one_row_per_split() {
        let b = sample();
        let s = sheets(json!({ "sheets": [{ "name": "Fun", "type": "transactions", "categoryIds": ["dining", "coffee"], "from": "2026-09" }] }));
        let g = fill(&s[0], &b).unwrap();
        assert_eq!(g.rows.len(), 2);
        // Oldest first, with real dates and amounts.
        assert_eq!(g.rows[0][0], Cell::Date("2026-09-05".into()));
        assert_eq!(g.rows[0][4], Cell::Text("Coffee".into()));
        assert_eq!(g.rows[1][6], Cell::Money(-124000));
        let all = sheets(json!({ "sheets": [{ "name": "All", "type": "transactions", "uncategorized": true }] }));
        let g = fill(&all[0], &b).unwrap();
        assert_eq!(g.rows.len(), 1);
        assert_eq!(g.rows[0][2], Cell::Text("Corner Bakery".into()));
    }

    #[test]
    fn unknown_ids_and_types_are_explained() {
        let b = sample();
        let err = sheets_from(&json!({ "sheets": [{ "name": "X", "type": "transactions", "categoryIds": ["nope"] }] }), &b, "2026-10").unwrap_err();
        assert!(err.contains("Sheet 1 (X)") && err.contains("No category"), "{err}");
        let err = sheets_from(&json!({ "sheets": [{ "name": "X", "type": "pivot" }] }), &b, "2026-10").unwrap_err();
        assert!(err.contains("no sheet type"), "{err}");
        let err = sheets_from(
            &json!({ "sheets": [{ "name": "X", "type": "table", "columns": [{ "header": "Spent", "type": "money" }], "rows": [["lots"]] }] }),
            &b,
            "2026-10",
        )
        .unwrap_err();
        assert!(err.contains("Row 1, Spent"), "{err}");
    }

    #[test]
    fn sheet_names_are_ones_excel_accepts() {
        let mut used = HashSet::new();
        assert_eq!(sheet_name("Spending: Jan/Feb", "Sheet 1", &mut used), "Spending Jan Feb");
        assert_eq!(sheet_name("spending jan feb", "Sheet 2", &mut used), "spending jan feb (2)");
        assert_eq!(sheet_name("", "Sheet 3", &mut used), "Sheet 3");
        assert_eq!(sheet_name("History", "Sheet 4", &mut used), "History 1");
        assert!(sheet_name(&"x".repeat(40), "Sheet 5", &mut used).chars().count() <= 31);
    }

    #[test]
    fn a_workbook_reads_back_with_numbers_dates_and_totals() {
        let b = sample();
        let s = sheets(json!({ "sheets": [
            { "name": "October", "type": "budget", "month": "2026-10" },
            { "name": "Fun", "type": "transactions", "categoryIds": ["dining", "coffee"] },
            { "name": "By month", "type": "spending", "from": "2026-09", "to": "2026-10" },
            { "name": "Notes", "type": "table", "columns": [{ "header": "Item", "type": "text" }, { "header": "Share", "type": "percent" }], "rows": [["Dining", 12.5]] }
        ] }));
        let grids: Vec<Grid> = s.iter().map(|s| fill(s, &b).unwrap()).collect();
        let bytes = write("October 2026", &grids).unwrap();
        let mut book = read_back(bytes);
        assert_eq!(book.sheet_names(), ["October", "Fun", "By month", "Notes"]);
        let fun = book.worksheet_range("Fun").unwrap();
        assert_eq!(fun.get_value((0, 6)), Some(&Data::String("Amount".into())));
        assert_eq!(fun.get_value((2, 6)), Some(&Data::Float(-1240.0)));
        assert!(matches!(fun.get_value((1, 0)), Some(Data::DateTime(_))));
        assert_eq!(fun.get_value((3, 0)), Some(&Data::String("Total".into())));
        let spending = book.worksheet_range("By month").unwrap();
        assert_eq!(spending.get_value((0, 2)), Some(&Data::String("Sep 2026".into())));
        let notes = book.worksheet_range("Notes").unwrap();
        assert_eq!(notes.get_value((1, 1)), Some(&Data::Float(0.125)));
        let formulas = book.worksheet_formula("Fun").unwrap();
        assert_eq!(formulas.get_value((3, 6)), Some(&"SUBTOTAL(9,G2:G3)".to_string()));
    }

    #[test]
    fn report_tables_become_sheets_with_real_numbers() {
        let body = "Intro.\n\n## Spending by category\n\n| Category | Spent | Share | Visits |\n|---|---:|---:|---:|\n| **Dining out** | SAR 1,240.00 | 42% | 7 |\n| Coffee | (22.50) | 1.5% | 12 |\n\n## Notes\n\nNo table here.\n\n| Month | Net |\n|---|---|\n| 2026-10-01 | 5 |";
        let grids = report_tables(body);
        assert_eq!(grids.len(), 2);
        assert_eq!(grids[0].name, "Spending by category");
        assert_eq!(grids[0].rows[0][0], Cell::Text("Dining out".into()));
        assert_eq!(grids[0].rows[0][1], Cell::Money(124000));
        assert_eq!(grids[0].rows[1][1], Cell::Money(-2250));
        assert_eq!(grids[0].rows[0][2], Cell::Percent(0.42));
        assert_eq!(grids[0].rows[1][3], Cell::Number(12.0));
        assert_eq!(grids[1].name, "Notes");
        assert_eq!(grids[1].rows[0][0], Cell::Date("2026-10-01".into()));
        assert!(write("Report", &grids).is_ok());
        assert!(report_tables("No tables at all.\n\n- a | b").is_empty());
    }

    #[test]
    fn arabic_titles_read_right_to_left_and_files_never_overwrite() {
        assert!(right_to_left("ميزانية أكتوبر"));
        assert!(!right_to_left("October budget"));
        let dir = std::env::temp_dir().join(new_id("xlsx"));
        std::fs::create_dir_all(&dir).unwrap();
        let first = free_path(&dir, "Groceries: since June?");
        assert_eq!(first.file_name().unwrap(), "Groceries since June.xlsx");
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(free_path(&dir, "Groceries: since June?").file_name().unwrap(), "Groceries since June (2).xlsx");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn column_letters_run_past_z() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
        assert_eq!(column_letter(27), "AB");
    }
}
