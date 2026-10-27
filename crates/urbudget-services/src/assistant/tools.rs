//! What the assistant may look up and propose. Read tools answer from a copy of the budget;
//! propose tools build a change on a copy and hand back a card for the user to apply. Nothing
//! here writes to the database.
use super::attachments::{self, Files};
use super::workbook;
use super::proposals::{self, Change, Edit, Move, NewTransaction, Proposal, Rename, Rule};
use super::{money, new_id, parse_money};
use crate::domain::*;
use crate::{engine, ledger, loans, reports, schedules};
use chrono::Datelike;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

/// Everything a tool call can see.
pub struct Context<'a> {
    pub budget: &'a Budget,
    /// Today, as YYYY-MM-DD in the user's time zone.
    pub today: &'a str,
    /// The month the user is looking at, as YYYY-MM.
    pub month: &'a str,
    pub share_memos: bool,
    pub files: &'a Files,
    /// Bank sync as the app knows it this session (see SimpleFin::snapshot); null outside the app.
    pub bank_sync: &'a Value,
    /// The bank file the latest message was sent from the Import page with, as it read then.
    pub import: Option<&'a crate::bank_statement::Brief>,
}

pub struct Outcome {
    /// What the model reads back.
    pub output: Value,
    /// One line for the panel, such as "Checked October 2026".
    pub step: String,
    pub proposal: Option<Proposal>,
}

pub struct Spec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    /// Proposes a change rather than only reading. Not offered outside the app.
    pub proposes: bool,
}

const MONTH: &str = "Month as YYYY-MM.";
const DATE: &str = "Date as YYYY-MM-DD.";
const AMOUNT: &str = "Amount in the budget's currency, like 12.50.";

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

/// The tools, in the order the model sees them.
pub fn specs() -> Vec<Spec> {
    let id_list = |what: &str| json!({ "type": "array", "items": { "type": "string" }, "description": what });
    vec![
        Spec {
            name: "get_month",
            description: "This month's budget: Ready to Assign, every category's assigned, activity and available amounts (negative available means overspent), what targets still need, and account balances.",
            parameters: object(json!({ "month": { "type": "string", "description": MONTH } }), &[]),
            proposes: false,
        },
        Spec {
            name: "find_transactions",
            description: "Search transactions. All filters are optional and combine. Returns the newest first.",
            parameters: object(
                json!({
                    "text": { "type": "string", "description": "Words to find in payee, memo, category or amount." },
                    "payee": { "type": "string", "description": "Exact payee name, any case." },
                    "accountId": { "type": "string" },
                    "categoryId": { "type": "string" },
                    "from": { "type": "string", "description": DATE },
                    "to": { "type": "string", "description": DATE },
                    "uncategorized": { "type": "boolean", "description": "Only spending that has no category yet." },
                    "unapproved": { "type": "boolean", "description": "Only imported transactions waiting for approval." },
                    "minAmount": { "type": "number", "description": "Smallest absolute amount." },
                    "maxAmount": { "type": "number", "description": "Largest absolute amount." },
                    "limit": { "type": "integer", "description": "At most this many, up to 200. Default 50." }
                }),
                &[],
            ),
            proposes: false,
        },
        Spec {
            name: "payee_history",
            description: "How the user has categorized each payee before, how often, and any payee rule that matches it. Use before categorizing.",
            parameters: object(json!({ "payees": id_list("Payee names, up to 30.") }), &["payees"]),
            proposes: false,
        },
        Spec {
            name: "get_report",
            description: "Income and spending between two months: totals by month, by category and by payee, plus net worth.",
            parameters: object(
                json!({
                    "from": { "type": "string", "description": MONTH },
                    "to": { "type": "string", "description": MONTH },
                    "accountIds": id_list("Only these accounts. Empty for all."),
                    "categoryIds": id_list("Only these categories. Empty for all.")
                }),
                &["from", "to"],
            ),
            proposes: false,
        },
        Spec {
            name: "get_forecast",
            description: "Projected balances of budget accounts through a date, from transactions already entered for later dates and scheduled ones. Includes each account's lowest point.",
            parameters: object(json!({ "through": { "type": "string", "description": DATE } }), &[]),
            proposes: false,
        },
        Spec {
            name: "get_setup",
            description: "Accounts, category groups with targets, payee rules, scheduled transactions and loans. Use to find ids.",
            parameters: object(json!({}), &[]),
            proposes: false,
        },
        Spec {
            name: "get_bank_sync",
            description: "Bank sync through SimpleFIN Bridge: whether it's connected, which accounts are linked to which bank accounts, the first day and last sync of each, the bank's balance from this session beside the budget's, how many imported transactions wait for approval, today's requests, and SimpleFIN's messages.",
            parameters: object(json!({}), &[]),
            proposes: false,
        },
        Spec {
            name: "project_loan",
            description: "Payoff date and interest for a loan account, with and without an extra monthly payment. The loan's terms must be saved in the account's loan details (Edit account) or imported.",
            parameters: object(
                json!({
                    "accountId": { "type": "string" },
                    "extraMonthly": { "type": "number", "description": AMOUNT },
                    "firstPayment": { "type": "string", "description": "First payment date, YYYY-MM-DD. Defaults to the 1st of next month." }
                }),
                &["accountId"],
            ),
            proposes: false,
        },
        Spec {
            name: "read_attachment",
            description: "Read rows of a spreadsheet or CSV the user attached, a page at a time. Rows are numbered as in the file. PDFs and images were already given to you with the message.",
            parameters: object(
                json!({
                    "attachmentId": { "type": "string" },
                    "sheet": { "type": "string", "description": "Sheet name. Defaults to the first sheet." },
                    "startRow": { "type": "integer", "description": "First row number to return. Default 1." },
                    "rowCount": { "type": "integer", "description": "How many rows, up to 200. Default 60." }
                }),
                &["attachmentId"],
            ),
            proposes: false,
        },
        Spec {
            name: "propose_budget_moves",
            description: "Propose assigning or moving money between categories for one month. Use \"ready_to_assign\" as from or to for Ready to Assign. The user reviews the card and applies it.",
            parameters: object(
                json!({
                    "month": { "type": "string", "description": MONTH },
                    "title": { "type": "string", "description": "Short title for the card, like \"Cover 2 overspent categories\"." },
                    "moves": {
                        "type": "array",
                        "items": object(json!({
                            "from": { "type": "string", "description": "Category id or ready_to_assign." },
                            "to": { "type": "string", "description": "Category id or ready_to_assign." },
                            "amount": { "type": "number", "description": AMOUNT }
                        }), &["from", "to", "amount"])
                    }
                }),
                &["month", "moves"],
            ),
            proposes: true,
        },
        Spec {
            name: "propose_auto_assign",
            description: "Propose one of the app's Auto-assign methods for a month: underfunded (fund what targets need), assignedLastMonth, spentLastMonth, averageAssigned, averageSpent, reduceOverfunding, resetAvailable, resetAssigned.",
            parameters: object(
                json!({
                    "month": { "type": "string", "description": MONTH },
                    "method": { "type": "string", "enum": ["underfunded", "assignedLastMonth", "spentLastMonth", "averageAssigned", "averageSpent", "reduceOverfunding", "resetAvailable", "resetAssigned"] },
                    "categoryIds": id_list("Categories to include. Empty for every visible category."),
                    "title": { "type": "string" }
                }),
                &["month", "method"],
            ),
            proposes: true,
        },
        Spec {
            name: "propose_transaction_edits",
            description: "Propose changes to existing transactions: category, payee, memo, cleared, approval or flag. Can also add payee rules so future imports are categorized automatically. Mark edits you are unsure about.",
            parameters: object(
                json!({
                    "title": { "type": "string" },
                    "edits": {
                        "type": "array",
                        "items": object(json!({
                            "id": { "type": "string", "description": "Transaction id." },
                            "categoryId": { "type": "string", "description": "Category id, or ready_to_assign for income." },
                            "payee": { "type": "string" },
                            "memo": { "type": "string" },
                            "cleared": { "type": "string", "enum": ["cleared", "uncleared"] },
                            "approve": { "type": "boolean" },
                            "flag": { "type": "string", "enum": ["red", "orange", "yellow", "green", "blue", "purple", "none"] },
                            "unsure": { "type": "boolean", "description": "True when the user should check this one." }
                        }), &["id"])
                    },
                    "rules": {
                        "type": "array",
                        "items": object(json!({
                            "match": { "type": "string", "description": "Text the payee contains." },
                            "payee": { "type": "string", "description": "Rename matching payees to this. Optional." },
                            "categoryId": { "type": "string" }
                        }), &["match"])
                    }
                }),
                &["edits"],
            ),
            proposes: true,
        },
        Spec {
            name: "propose_transactions",
            description: "Propose adding new transactions to one account, for example from a receipt. Negative amounts are spending. Give splits when one purchase covers several categories.",
            parameters: object(
                json!({
                    "accountId": { "type": "string" },
                    "title": { "type": "string" },
                    "fromFile": { "type": "boolean", "description": "True when read from a photo or document, so the user checks the amounts." },
                    "transactions": {
                        "type": "array",
                        "items": object(json!({
                            "date": { "type": "string", "description": DATE },
                            "payee": { "type": "string" },
                            "memo": { "type": "string" },
                            "amount": { "type": "number", "description": "Negative for spending, positive for income." },
                            "categoryId": { "type": "string", "description": "Category id, or ready_to_assign for income." },
                            "cleared": { "type": "boolean" },
                            "splits": {
                                "type": "array",
                                "items": object(json!({
                                    "categoryId": { "type": "string" },
                                    "amount": { "type": "number", "description": "Same sign as the transaction." },
                                    "memo": { "type": "string" }
                                }), &["amount"])
                            }
                        }), &["date", "payee", "amount"])
                    }
                }),
                &["accountId", "transactions"],
            ),
            proposes: true,
        },
        Spec {
            name: "propose_payee_changes",
            description: "Propose renaming or merging payees (every transaction with the old name gets the new one) and adding payee rules.",
            parameters: object(
                json!({
                    "title": { "type": "string" },
                    "renames": {
                        "type": "array",
                        "items": object(json!({ "from": { "type": "string" }, "to": { "type": "string" } }), &["from", "to"])
                    },
                    "rules": {
                        "type": "array",
                        "items": object(json!({
                            "match": { "type": "string" },
                            "payee": { "type": "string" },
                            "categoryId": { "type": "string" }
                        }), &["match"])
                    }
                }),
                &[],
            ),
            proposes: true,
        },
        Spec {
            name: "fix_import",
            description: "Propose fixes for the bank file the user is importing on the Import page: a value for a line's date (YYYY-MM-DD), payee, memo or amount (negative for money out, in the budget's currency), including or leaving out a line that's left out, or how the file reads (the date order, which column holds what, signs). Only for lines in the import's note, with `from` the value the note shows. Use values the file supports, never a guess, and give a reason for each. The user applies them from the card, and the import checks each again.",
            parameters: object(
                json!({
                    "fields": {
                        "type": "array",
                        "items": object(json!({
                            "line": { "type": "string", "description": "The line's id, from the import's note." },
                            "field": { "type": "string", "enum": ["date", "payee", "memo", "amount"] },
                            "from": { "type": "string", "description": "The value the note shows for it now." },
                            "to": { "type": "string" },
                            "reason": { "type": "string", "description": "Why, in a few words the user can check." }
                        }), &["line", "field", "from", "to", "reason"])
                    },
                    "include": {
                        "type": "array",
                        "items": object(json!({
                            "line": { "type": "string" },
                            "include": { "type": "boolean", "description": "True to import a line that's left out, false to leave one out again." },
                            "reason": { "type": "string" }
                        }), &["line", "include", "reason"])
                    },
                    "readAs": object(json!({
                        "dateOrder": { "type": "string", "enum": ["ymd", "dmy", "mdy"], "description": "The order the file writes day and month in." },
                        "signsByType": { "type": "boolean", "description": "An OFX whose amounts are all positive: money out is told by the transaction types." },
                        "flipSigns": { "type": "boolean", "description": "A CSV's purchases are written as positive numbers." },
                        "columns": object(json!({
                            "date": { "type": "string" }, "payee": { "type": "string" }, "memo": { "type": "string" },
                            "amount": { "type": "string" }, "outflow": { "type": "string" }, "inflow": { "type": "string" }
                        }), &[])
                    }), &[]),
                    "note": { "type": "string", "description": "One or two sentences for the card." }
                }),
                &["note"],
            ),
            proposes: true,
        },
        Spec {
            name: "prepare_import",
            description: "Turn an attached statement into a file for the app's import review. For a regular table, give a column mapping (exact: the app converts every row). For a messy file, a PDF or a photo, give the rows. Pass the totals the statement shows as expected so the app can check them. The user reviews duplicates and matches before anything is added.",
            parameters: object(
                json!({
                    "accountId": { "type": "string" },
                    "attachmentId": { "type": "string" },
                    "mapping": object(json!({
                        "sheet": { "type": "string" },
                        "headerRow": { "type": "integer", "description": "Row number of the column headers. Data starts on the next row." },
                        "lastRow": { "type": "integer", "description": "Last data row, when totals or notes follow the table." },
                        "date": { "type": "string", "description": "Column letter (A, B…) of the date." },
                        "payee": { "type": "string", "description": "Column letter of the payee or description." },
                        "memo": { "type": "string", "description": "Column letter of a memo or reference. Optional." },
                        "amount": { "type": "string", "description": "Column letter of a signed amount. Or use outflow and inflow." },
                        "outflow": { "type": "string", "description": "Column letter of money out (debit)." },
                        "inflow": { "type": "string", "description": "Column letter of money in (credit)." },
                        "dateOrder": { "type": "string", "enum": ["ymd", "dmy", "mdy"], "description": "Order of day and month in text dates." },
                        "negate": { "type": "boolean", "description": "True when the amount column shows spending as positive." },
                        "skipRows": { "type": "array", "items": { "type": "integer" }, "description": "Row numbers to leave out, like an opening balance line." }
                    }), &["headerRow", "date", "payee", "dateOrder"]),
                    "rows": {
                        "type": "array",
                        "items": object(json!({
                            "date": { "type": "string", "description": DATE },
                            "payee": { "type": "string" },
                            "memo": { "type": "string" },
                            "amount": { "type": "number", "description": "Negative for money out." }
                        }), &["date", "payee", "amount"])
                    },
                    "expected": object(json!({
                        "count": { "type": "integer" },
                        "outflow": { "type": "number", "description": "Total money out as a positive amount." },
                        "inflow": { "type": "number" }
                    }), &[])
                }),
                &["accountId"],
            ),
            proposes: true,
        },
        Spec {
            name: "create_report",
            description: "Write a report the user can preview and save as a PDF, such as a month in review or spending by category. Gather the numbers with the other tools first and use only those; never estimate figures. The body is Markdown: a short intro, then sections with ## headings, short paragraphs, bullet lists and tables (amounts right-aligned). Don't repeat the report in chat; say it's ready.",
            parameters: object(
                json!({
                    "title": { "type": "string", "description": "Like \"October 2026 in review\"." },
                    "period": { "type": "string", "description": "What it covers, like \"October 2026\" or \"Jan – Jun 2026\"." },
                    "body": { "type": "string", "description": "The report in Markdown." }
                }),
                &["title", "body"],
            ),
            proposes: true,
        },
        Spec {
            name: "create_spreadsheet",
            description: "Make an Excel spreadsheet the user can open or save, with one or more sheets. The app fills data sheets from the budget, so use them for any list or breakdown: transactions (every matching transaction, one row per split, oldest first; filters combine like find_transactions), budget (one month's categories: assigned, activity, available and target), spending (spending by category for each month in a range, with totals and the monthly average) and months (income, spending, net income and net worth by month). Use a table sheet only for a short summary you worked out from figures you looked up. Don't repeat the data in chat; say it's ready.",
            parameters: object(
                json!({
                    "title": { "type": "string", "description": "Also the file name, like \"Groceries since June 2026\"." },
                    "sheets": {
                        "type": "array",
                        "description": "Up to 12 sheets, in order.",
                        "items": object(
                            json!({
                                "name": { "type": "string", "description": "Sheet name, up to 31 characters." },
                                "type": { "type": "string", "enum": ["transactions", "budget", "spending", "months", "table"] },
                                "from": { "type": "string", "description": "transactions: first date, YYYY-MM-DD. spending and months: first month, YYYY-MM." },
                                "to": { "type": "string", "description": "transactions: last date, YYYY-MM-DD. spending and months: last month, YYYY-MM." },
                                "month": { "type": "string", "description": "budget: the month, YYYY-MM. Defaults to the month on screen." },
                                "accountIds": id_list("transactions, spending, months: only these accounts."),
                                "categoryIds": id_list("transactions, spending: only these categories."),
                                "payee": { "type": "string", "description": "transactions: exact payee name, any case." },
                                "text": { "type": "string", "description": "transactions: words to find in payee, memo or category." },
                                "uncategorized": { "type": "boolean", "description": "transactions: only spending with no category yet." },
                                "unapproved": { "type": "boolean", "description": "transactions: only imported ones waiting for approval." },
                                "columns": {
                                    "type": "array",
                                    "description": "table: its columns.",
                                    "items": object(
                                        json!({
                                            "header": { "type": "string" },
                                            "type": { "type": "string", "enum": ["text", "money", "number", "percent", "date", "month"], "description": "money as 1240.50, percent as 12.5 for 12.5%, date as YYYY-MM-DD, month as YYYY-MM." }
                                        }),
                                        &["header", "type"],
                                    )
                                },
                                "rows": {
                                    "type": "array",
                                    "description": "table: one list of cells per row, in column order. At most 1,000 rows.",
                                    "items": { "type": "array", "items": { "type": ["string", "number", "null"] } }
                                },
                                "total": { "type": "boolean", "description": "table: add a totals row for its money and number columns." }
                            }),
                            &["name", "type"],
                        )
                    }
                }),
                &["title", "sheets"],
            ),
            proposes: true,
        },
    ]
}

/// Names and lookups shared by the tools.
pub struct Names<'a> {
    pub budget: &'a Budget,
    pub accounts: HashMap<&'a str, &'a Account>,
    pub categories: HashMap<&'a str, &'a Category>,
}
impl<'a> Names<'a> {
    pub fn new(budget: &'a Budget) -> Self {
        Self {
            budget,
            accounts: budget.accounts.iter().map(|a| (a.id.as_str(), a)).collect(),
            categories: budget.categories.iter().map(|c| (c.id.as_str(), c)).collect(),
        }
    }
    pub fn account(&self, id: &str) -> String {
        self.accounts.get(id).map_or_else(|| id.to_owned(), |a| a.name.clone())
    }
    pub fn category(&self, id: Option<&str>) -> String {
        match id {
            None => "Ready to assign".into(),
            Some(id) => self.categories.get(id).map_or_else(|| id.to_owned(), |c| c.name.clone()),
        }
    }
    fn on_budget(&self, account: &str) -> bool {
        self.accounts.get(account).is_some_and(|a| a.on_budget())
    }
    /// Spending in a budget account that has no category yet.
    pub fn needs_category(&self, t: &Transaction) -> bool {
        t.transfer_account_id.is_none()
            && self.on_budget(&t.account_id)
            && t.splits.iter().any(|s| s.category_id.is_none() && ledger::transfer(s).is_none() && s.amount < 0)
    }
    pub fn category_label(&self, t: &Transaction) -> String {
        if let Some(other) = &t.transfer_account_id {
            return format!("Transfer: {}", self.account(other));
        }
        if t.splits.len() > 1 {
            return "Split".into();
        }
        let Some(s) = t.splits.first() else { return String::new() };
        if let Some(other) = ledger::transfer(s) {
            return format!("Transfer: {}", self.account(other));
        }
        if !self.on_budget(&t.account_id) {
            return "Tracking".into();
        }
        match s.category_id.as_deref() {
            Some(id) => self.category(Some(id)),
            None if s.amount < 0 => "Uncategorized".into(),
            None => "Ready to assign".into(),
        }
    }
    pub fn row(&self, t: &Transaction, share_memos: bool) -> Value {
        let mut row = json!({
            "id": t.id,
            "date": t.date,
            "account": self.account(&t.account_id),
            "payee": t.payee,
            "category": self.category_label(t),
            "amount": money(t.amount),
            "cleared": t.status,
        });
        if share_memos && !t.memo.is_empty() {
            row["memo"] = json!(truncate(&t.memo, 120));
        }
        if t.extra.get("approved") == Some(&json!(false)) {
            row["approved"] = json!(false);
        }
        if let Some(flag) = t.extra.get("flag").and_then(Value::as_str) {
            row["flag"] = json!(flag);
        }
        if t.splits.len() > 1 {
            row["splits"] = t
                .splits
                .iter()
                .map(|s| json!({ "category": self.category(s.category_id.as_deref()), "amount": money(s.amount) }))
                .collect();
        }
        row
    }
}

pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut s: String = text.chars().take(max).collect();
    s.push('…');
    s
}
pub(super) fn hidden(c: &Category) -> bool {
    c.extra.get("hidden") == Some(&json!(true))
}
pub(super) fn month_label(month: &str) -> String {
    chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map(|d| d.format("%B %Y").to_string())
        .unwrap_or_else(|_| month.to_owned())
}
pub(super) fn arg<'v>(args: &'v Value, key: &str) -> Option<&'v str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}
pub(super) fn month_arg(args: &Value, key: &str, default: &str) -> Result<String, String> {
    let month = arg(args, key).unwrap_or(default);
    if month.len() == 7 && chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").is_ok() {
        Ok(month.to_owned())
    } else {
        Err(format!("{key} must be a month like 2026-10."))
    }
}
pub(super) fn date_arg(args: &Value, key: &str) -> Result<Option<String>, String> {
    match arg(args, key) {
        None => Ok(None),
        Some(d) if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok() => Ok(Some(d.to_owned())),
        Some(_) => Err(format!("{key} must be a date like 2026-10-31.")),
    }
}
pub(super) fn strings(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Runs one tool. Errors go back to the model as `{"error": …}` so it can correct itself.
pub fn call(name: &str, args: &Value, ctx: &Context) -> Outcome {
    let result = match name {
        "get_month" => get_month(args, ctx),
        "find_transactions" => find_transactions(args, ctx),
        "payee_history" => payee_history(args, ctx),
        "get_report" => get_report(args, ctx),
        "get_forecast" => get_forecast(args, ctx),
        "get_setup" => get_setup(ctx),
        "get_bank_sync" => get_bank_sync(ctx),
        "project_loan" => project_loan(args, ctx),
        "read_attachment" => read_attachment(args, ctx),
        "propose_budget_moves" => propose_budget_moves(args, ctx),
        "propose_auto_assign" => propose_auto_assign(args, ctx),
        "propose_transaction_edits" => propose_transaction_edits(args, ctx),
        "propose_transactions" => propose_transactions(args, ctx),
        "propose_payee_changes" => propose_payee_changes(args, ctx),
        "prepare_import" => prepare_import(args, ctx),
        "fix_import" => fix_import(args, ctx),
        "create_report" => create_report(args),
        "create_spreadsheet" => create_spreadsheet(args, ctx),
        _ => Err(format!("There is no tool named {name}.")),
    };
    result.unwrap_or_else(|error| Outcome {
        output: json!({ "error": error }),
        step: format!("{}: {}", failed_step(name), truncate(&error, 90)),
        proposal: None,
    })
}
fn failed_step(name: &str) -> &'static str {
    if name.starts_with("propose") || name == "prepare_import" || name == "fix_import" {
        "Couldn't prepare that change"
    } else if name == "create_spreadsheet" {
        "Couldn't make that spreadsheet"
    } else {
        "Couldn't look that up"
    }
}
fn read(output: Value, step: String) -> Result<Outcome, String> {
    Ok(Outcome { output, step, proposal: None })
}

fn get_month(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let month = month_arg(args, "month", ctx.month)?;
    let s = engine::calculate(b, &month)?;
    let names = Names::new(b);
    let rows: HashMap<&str, &CategorySummary> = s.categories.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut groups: Vec<(String, Vec<Value>)> = vec![];
    let mut overspent = vec![];
    let mut underfunded = 0;
    for c in &b.categories {
        let Some(r) = rows.get(c.id.as_str()) else { continue };
        if hidden(c) && r.assigned == 0 && r.activity == 0 && r.available == 0 {
            continue;
        }
        let mut row = json!({
            "id": c.id,
            "name": c.name,
            "assigned": money(r.assigned),
            "activity": money(r.activity),
            "available": money(r.available),
        });
        if c.target > 0 {
            row["target"] = json!(money(c.target));
        }
        if r.needed > 0 {
            row["needed"] = json!(money(r.needed));
            underfunded += 1;
        }
        if r.upcoming > 0 {
            row["upcomingScheduled"] = json!(money(r.upcoming));
        }
        if hidden(c) {
            row["hidden"] = json!(true);
        }
        if let Some(card) = c.payment_account() {
            row["paysCreditCard"] = json!(names.account(card));
        }
        if r.available < 0 {
            overspent.push(json!({ "id": c.id, "name": c.name, "available": money(r.available) }));
        }
        match groups.iter_mut().find(|(g, _)| g == &c.group) {
            Some((_, list)) => list.push(row),
            None => groups.push((c.group.clone(), vec![row])),
        }
    }
    let accounts: Vec<Value> = b
        .accounts
        .iter()
        .filter_map(|a| {
            let r = s.accounts.iter().find(|x| x.id == a.id)?;
            let mut row = json!({
                "id": a.id,
                "name": a.name,
                "type": a.kind,
                "onBudget": a.on_budget(),
                "balance": money(r.balance),
                "cleared": money(r.cleared),
                "uncleared": money(r.uncleared),
            });
            if a.archived {
                row["closed"] = json!(true);
            }
            Some(row)
        })
        .collect();
    let output = json!({
        "month": month,
        "currency": b.currency,
        "readyToAssign": money(s.ready_to_assign),
        "assigned": money(s.assigned),
        "activity": money(s.activity),
        "available": money(s.available),
        "income": money(s.income),
        "netWorth": money(s.net_worth),
        "overspent": overspent,
        "groups": groups.into_iter().map(|(group, categories)| json!({ "group": group, "categories": categories })).collect::<Vec<_>>(),
        "accounts": accounts,
    });
    let mut step = format!("Checked {}", month_label(&month));
    if !overspent.is_empty() || underfunded > 0 {
        let mut parts = vec![];
        if !overspent.is_empty() {
            parts.push(plural(overspent.len(), "category is overspent", "categories are overspent"));
        }
        if underfunded > 0 {
            parts.push(plural(underfunded, "target needs money", "targets need money"));
        }
        step = format!("{step}: {}", parts.join(", "));
    }
    read(output, step)
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn find_transactions(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let text = arg(args, "text").map(str::to_lowercase);
    let payee = arg(args, "payee").map(str::to_lowercase);
    let account = arg(args, "accountId");
    let category = arg(args, "categoryId");
    let from = date_arg(args, "from")?;
    let to = date_arg(args, "to")?;
    let uncategorized = args["uncategorized"] == json!(true);
    let unapproved = args["unapproved"] == json!(true);
    let min = args.get("minAmount").filter(|v| !v.is_null()).map(parse_money).transpose()?;
    let max = args.get("maxAmount").filter(|v| !v.is_null()).map(parse_money).transpose()?;
    let limit = args["limit"].as_u64().unwrap_or(50).clamp(1, 200) as usize;
    if let Some(id) = account {
        if !names.accounts.contains_key(id) {
            return Err(format!("No account has the id {id}. Use get_setup to find ids."));
        }
    }
    if let Some(id) = category {
        if !names.categories.contains_key(id) {
            return Err(format!("No category has the id {id}. Use get_setup to find ids."));
        }
    }
    let mut found: Vec<&Transaction> = b
        .transactions
        .iter()
        .filter(|t| {
            account.is_none_or(|a| {
                t.account_id == a
                    || t.transfer_account_id.as_deref() == Some(a)
                    || t.splits.iter().any(|s| ledger::transfer(s) == Some(a))
            }) && category.is_none_or(|c| t.splits.iter().any(|s| s.category_id.as_deref() == Some(c)))
                && from.as_deref().is_none_or(|f| t.date.as_str() >= f)
                && to.as_deref().is_none_or(|x| t.date.as_str() <= x)
                && payee.as_deref().is_none_or(|p| t.payee.to_lowercase() == p)
                && (!uncategorized || names.needs_category(t))
                && (!unapproved || t.extra.get("approved") == Some(&json!(false)))
                && min.is_none_or(|m| t.amount.abs() >= m)
                && max.is_none_or(|m| t.amount.abs() <= m)
                && text.as_deref().is_none_or(|q| {
                    t.payee.to_lowercase().contains(q)
                        || (ctx.share_memos && t.memo.to_lowercase().contains(q))
                        || names.category_label(t).to_lowercase().contains(q)
                        || money(t.amount.abs()).contains(q)
                })
        })
        .collect();
    // Newest first, as the register lists them.
    sort_newest_first(&mut found);
    let total = found.len();
    let rows: Vec<Value> = found.iter().take(limit).map(|t| names.row(t, ctx.share_memos)).collect();
    let mut what = vec![];
    if let Some(q) = arg(args, "text").or(arg(args, "payee")) {
        what.push(format!("\"{}\"", truncate(q, 30)));
    }
    if uncategorized {
        what.push("uncategorized".into());
    }
    if unapproved {
        what.push("waiting for approval".into());
    }
    if let Some(a) = account {
        what.push(names.account(a));
    }
    if let Some(c) = category {
        what.push(names.category(Some(c)));
    }
    let step = format!(
        "Searched transactions{}: {}",
        if what.is_empty() { String::new() } else { format!(" ({})", what.join(", ")) },
        plural(total, "found", "found")
    );
    read(json!({ "total": total, "shown": rows.len(), "transactions": rows }), step)
}

fn payee_history(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let wanted = strings(args, "payees");
    if wanted.is_empty() || wanted.len() > 30 {
        return Err("Give between 1 and 30 payee names.".into());
    }
    let mut out = vec![];
    for name in &wanted {
        let lower = name.to_lowercase();
        let exact: Vec<&Transaction> = b.transactions.iter().filter(|t| t.payee.to_lowercase() == lower).collect();
        let list = if exact.is_empty() {
            b.transactions.iter().filter(|t| t.payee.to_lowercase().contains(&lower)).collect()
        } else {
            exact
        };
        let mut categories: BTreeMap<String, usize> = BTreeMap::new();
        let mut last = String::new();
        for t in &list {
            if t.transfer_account_id.is_none() {
                for s in &t.splits {
                    if s.category_id.is_some() {
                        *categories.entry(names.category(s.category_id.as_deref())).or_default() += 1;
                    }
                }
            }
            if t.date > last {
                last = t.date.clone();
            }
        }
        let mut categories: Vec<(String, usize)> = categories.into_iter().collect();
        categories.sort_by(|a, b| b.1.cmp(&a.1));
        let rule = crate::workflows::payee_rule(b, name);
        let mut row = json!({
            "payee": name,
            "transactions": list.len(),
            "categories": categories.iter().take(5).map(|(n, c)| json!({ "category": n, "times": c })).collect::<Vec<_>>(),
        });
        if !last.is_empty() {
            row["lastDate"] = json!(last);
        }
        if let Some(rule) = rule {
            row["rule"] = json!({
                "match": rule["match"],
                "renameTo": rule["payee"],
                "category": rule["categoryId"].as_str().map(|id| names.category(Some(id))),
            });
        }
        out.push(row);
    }
    let step = format!("Looked up how you categorized {} before", plural(wanted.len(), "payee", "payees"));
    read(json!({ "payees": out }), step)
}

fn get_report(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let from = month_arg(args, "from", ctx.month)?;
    let to = month_arg(args, "to", ctx.month)?;
    let filter = reports::Filter {
        from: from.clone(),
        to: to.clone(),
        account_ids: strings(args, "accountIds"),
        category_ids: strings(args, "categoryIds"),
    };
    let r = reports::calculate(b, &filter)?;
    let mut categories: Vec<(&String, &i64)> = r.categories.iter().collect();
    categories.sort_by(|a, b| b.1.cmp(a.1));
    let mut payees: Vec<(&String, &reports::PayeeTotal)> = r.payees.iter().collect();
    payees.sort_by(|a, b| b.1.amount.cmp(&a.1.amount));
    let output = json!({
        "from": from,
        "to": to,
        "income": money(r.income),
        "spending": money(r.spending),
        "months": r.months.iter().map(|m| json!({
            "month": m.month,
            "income": money(m.income),
            "spending": money(m.spending),
            "netIncome": money(m.net_income),
            "netWorth": money(m.net_worth),
        })).collect::<Vec<_>>(),
        "spendingByCategory": categories.iter().take(40).map(|(id, amount)| json!({
            "categoryId": id,
            "category": names.category(Some(id)),
            "spent": money(**amount),
        })).collect::<Vec<_>>(),
        "spendingByCategoryPerMonth": if r.months.len() <= 12 && filter.category_ids.len() <= 5 && !filter.category_ids.is_empty() {
            json!(r.months.iter().map(|m| json!({
                "month": m.month,
                "categories": m.categories.iter().map(|(id, v)| (names.category(Some(id)), json!(money(*v)))).collect::<serde_json::Map<_, _>>(),
            })).collect::<Vec<_>>())
        } else {
            Value::Null
        },
        "topPayees": payees.iter().take(20).map(|(name, p)| json!({
            "payee": name,
            "spent": money(p.amount),
            "transactions": p.count,
        })).collect::<Vec<_>>(),
        "incomeSources": r.income_sources.iter().map(|(name, v)| json!({ "source": name, "amount": money(*v) })).collect::<Vec<_>>(),
        "ageOfMoneyDays": r.age_of_money,
    });
    let range = if from == to { month_label(&from) } else { format!("{} to {}", month_label(&from), month_label(&to)) };
    let focus = if filter.category_ids.is_empty() {
        String::new()
    } else {
        format!(
            " for {}",
            filter.category_ids.iter().map(|id| names.category(Some(id))).collect::<Vec<_>>().join(", ")
        )
    };
    read(output, format!("Ran a spending report{focus}: {range}"))
}

fn get_forecast(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let through = match date_arg(args, "through")? {
        Some(d) => d,
        None => {
            let today = chrono::NaiveDate::parse_from_str(ctx.today, "%Y-%m-%d").map_err(|e| e.to_string())?;
            let next = today.checked_add_months(chrono::Months::new(2)).ok_or("Invalid date")?;
            next.with_day0(0).and_then(|d| d.pred_opt()).ok_or("Invalid date")?.to_string()
        }
    };
    let f = reports::forecast(b, ctx.today, &through)?;
    let mut running: BTreeMap<String, i64> = f.balances.clone();
    let mut lowest: BTreeMap<String, (i64, String)> =
        f.balances.iter().map(|(id, v)| (id.clone(), (*v, ctx.today.to_owned()))).collect();
    for e in &f.events {
        let v = running.entry(e.account_id.clone()).or_default();
        *v += e.amount;
        let low = lowest.entry(e.account_id.clone()).or_insert((*v, e.date.clone()));
        if *v < low.0 {
            *low = (*v, e.date.clone());
        }
    }
    let output = json!({
        "through": through,
        "accounts": f.balances.iter().map(|(id, v)| json!({
            "account": names.account(id),
            "today": money(*v),
            "end": money(running[id]),
            "lowest": money(lowest[id].0),
            "lowestOn": lowest[id].1,
        })).collect::<Vec<_>>(),
        "events": f.events.iter().take(80).map(|e| json!({
            "date": e.date,
            "account": names.account(&e.account_id),
            "payee": e.payee,
            "amount": money(e.amount),
            "scheduled": e.schedule_id.is_some(),
        })).collect::<Vec<_>>(),
        "moreEvents": f.events.len().saturating_sub(80),
        "averageMonthlySpending": money(f.recent_spending),
    });
    read(output, format!("Ran the forecast through {through}"))
}

fn get_bank_sync(ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let sync = ctx.bank_sync;
    let listed = sync["accounts"].as_array().map(Vec::as_slice).unwrap_or_default();
    let summary = engine::calculate(b, &ctx.today[..7])?;
    let cents = |v: &Value| v.as_i64().map(money);
    let mut linked_ids = vec![];
    let linked: Vec<Value> = b
        .accounts
        .iter()
        .filter(|a| !a.archived)
        .filter_map(|a| {
            let link = a.extra.get("simplefin")?;
            linked_ids.push(link["id"].clone());
            let balances = summary.accounts.iter().find(|s| s.id == a.id);
            let mut row = json!({
                "accountId": a.id,
                "account": a.name,
                "type": a.kind,
                "bankAccount": link["name"],
                "bank": link["org"],
                "firstDay": link["from"],
                "lastSync": link["lastSync"],
                "purchasesArriveAsMoneyIn": link["flipSigns"].as_bool().unwrap_or(false),
                "balance": balances.map(|s| money(s.balance)),
                "clearedBalance": balances.map(|s| money(s.cleared)),
                "waitingForApproval": b.transactions.iter().filter(|t| t.account_id == a.id && t.extra.get("approved") == Some(&json!(false))).count(),
            });
            match listed.iter().find(|l| l["id"] == link["id"]) {
                Some(l) => {
                    row["bankBalance"] = json!(cents(&l["balance"]));
                    row["bankBalanceDate"] = l["balanceDate"].clone();
                }
                None => row["bankBalance"] = json!("not listed this session; shown after a sync"),
            }
            Some(row)
        })
        .collect();
    let not_linked: Vec<Value> = listed
        .iter()
        .filter(|l| !linked_ids.contains(&l["id"]))
        .map(|l| json!({ "bankAccount": l["name"], "bank": l["org"], "currency": l["currency"], "balance": cents(&l["balance"]) }))
        .collect();
    let output = json!({
        "available": sync.get("available").cloned().unwrap_or(Value::Null),
        "connected": sync.get("connected").cloned().unwrap_or(Value::Null),
        "requestsToday": sync["requestsToday"],
        "dailyLimit": sync["dailyLimit"],
        "linked": linked,
        "notLinked": not_linked,
        "messages": sync["messages"],
    });
    let step = match linked.len() {
        0 => "Checked bank sync: no linked accounts".to_string(),
        1 => "Checked bank sync for 1 account".to_string(),
        n => format!("Checked bank sync for {n} accounts"),
    };
    read(output, step)
}
fn get_setup(ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let mut groups: Vec<(String, Vec<Value>)> = vec![];
    for c in &b.categories {
        let mut row = json!({ "id": c.id, "name": c.name });
        if c.target > 0 {
            row["target"] = json!(money(c.target));
        }
        if let Some(goal) = c.extra.get("goal").filter(|g| g.is_object()) {
            let mut g = serde_json::Map::new();
            for key in ["goal_type", "goal_target", "goal_target_date", "goal_cadence"] {
                if let Some(v) = goal.get(key).filter(|v| !v.is_null()) {
                    let v = if key == "goal_target" { json!(v.as_i64().map(money)) } else { v.clone() };
                    g.insert(key.trim_start_matches("goal_").into(), v);
                }
            }
            if !g.is_empty() {
                row["goal"] = Value::Object(g);
            }
        }
        if hidden(c) {
            row["hidden"] = json!(true);
        }
        if let Some(card) = c.payment_account() {
            row["paysCreditCard"] = json!(names.account(card));
        }
        match groups.iter_mut().find(|(g, _)| g == &c.group) {
            Some((_, list)) => list.push(row),
            None => groups.push((c.group.clone(), vec![row])),
        }
    }
    let schedules = schedules::read(b).unwrap_or_default();
    let output = json!({
        "budget": b.name,
        "currency": b.currency,
        "accounts": b.accounts.iter().map(|a| json!({
            "id": a.id, "name": a.name, "type": a.kind, "onBudget": a.on_budget(), "closed": a.archived,
        })).collect::<Vec<_>>(),
        "categoryGroups": groups.into_iter().map(|(group, categories)| json!({ "group": group, "categories": categories })).collect::<Vec<_>>(),
        "payeeRules": b.extra.get("payeeRules").and_then(Value::as_array).map(|rules| rules.iter().map(|r| json!({
            "match": r["match"],
            "renameTo": r["payee"],
            "category": r["categoryId"].as_str().map(|id| names.category(Some(id))),
        })).collect::<Vec<_>>()).unwrap_or_default(),
        "scheduled": schedules.iter().filter(|s| !s.paused).take(60).map(|s| json!({
            "next": s.next_date,
            "frequency": s.frequency,
            "payee": s.transaction.payee,
            "account": names.account(&s.transaction.account_id),
            "category": names.category_label(&s.transaction),
            "amount": money(s.transaction.amount),
        })).collect::<Vec<_>>(),
        "loans": b.extra.get("loans").and_then(Value::as_array).map(|loans| loans.iter().map(|l| json!({
            "account": l["accountId"].as_str().map(|id| names.account(id)),
            "accountId": l["accountId"],
            "annualRatePercent": l["annualRateBasisPoints"].as_i64().map(|r| r as f64 / 100.0),
            "monthlyPayment": l["monthlyPayment"].as_i64().map(money),
        })).collect::<Vec<_>>()).unwrap_or_default(),
    });
    read(output, "Looked at your accounts and categories".into())
}

fn project_loan(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let account = arg(args, "accountId").ok_or("Give the loan's accountId.")?;
    if !names.accounts.contains_key(account) {
        return Err(format!("No account has the id {account}."));
    }
    let saved: Option<loans::Terms> = b
        .extra
        .get("loans")
        .and_then(Value::as_array)
        .and_then(|l| l.iter().find(|t| t["accountId"] == json!(account)))
        .and_then(|t| serde_json::from_value(t.clone()).ok());
    let terms = match saved {
        Some(t) => t,
        None => loans::source_terms(b, account)
            .map_err(|_| "This loan has no saved terms. Ask the user to add them in the account's loan details: Edit account, or Add loan details over its register.".to_string())?,
    };
    let month = &ctx.today[..7];
    let s = engine::calculate(b, month)?;
    let balance = (-s.accounts.iter().find(|a| a.id == account).map_or(0, |a| a.balance)).max(0);
    if balance == 0 {
        return Err("This loan has no balance left.".into());
    }
    let start = match date_arg(args, "firstPayment")? {
        Some(d) => d,
        None => {
            let today = chrono::NaiveDate::parse_from_str(ctx.today, "%Y-%m-%d").map_err(|e| e.to_string())?;
            today
                .checked_add_months(chrono::Months::new(1))
                .and_then(|d| d.with_day0(0))
                .ok_or("Invalid date")?
                .to_string()
        }
    };
    let extra = args.get("extraMonthly").filter(|v| !v.is_null()).map(parse_money).transpose()?.unwrap_or(0);
    let base = loans::project(balance, &terms, &start, 0)?;
    let faster = loans::project(balance, &terms, &start, extra)?;
    let output = json!({
        "loan": names.account(account),
        "balance": money(balance),
        "monthlyPayment": money(terms.monthly_payment),
        "annualRatePercent": terms.annual_rate_basis_points as f64 / 100.0,
        "asIs": { "months": base.months, "payoffDate": base.payoff_date, "totalInterest": money(base.total_interest), "totalFees": money(base.total_fees) },
        "withExtra": { "extraMonthly": money(extra), "months": faster.months, "payoffDate": faster.payoff_date, "totalInterest": money(faster.total_interest), "totalFees": money(faster.total_fees) },
        "interestSaved": money(base.total_interest - faster.total_interest),
    });
    let step = if extra > 0 {
        format!("Ran the payoff planner for {} with {} extra a month", names.account(account), money(extra))
    } else {
        format!("Ran the payoff planner for {}", names.account(account))
    };
    read(output, step)
}

fn read_attachment(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let id = arg(args, "attachmentId").ok_or("Give the attachmentId.")?;
    let file = ctx.files.get(id)?;
    if !file.is_table() {
        return read(
            json!({ "name": file.name, "note": "This file was given to you directly with the user's message. Read it from there." }),
            format!("Opened {}", file.name),
        );
    }
    let table = attachments::read_table(ctx.files, id, arg(args, "sheet"))?;
    let start = args["startRow"].as_u64().unwrap_or(1).max(1) as usize;
    let count = args["rowCount"].as_u64().unwrap_or(60).clamp(1, 200) as usize;
    let rows: Vec<Value> = table
        .rows
        .iter()
        .filter(|r| r.number >= start)
        .take(count)
        .map(|r| {
            let mut cells = vec![json!(r.number)];
            cells.extend(r.cells.iter().map(|c| json!(c)));
            Value::Array(cells)
        })
        .collect();
    let last = table.rows.last().map_or(0, |r| r.number);
    let output = json!({
        "name": file.name,
        "sheets": table.sheets,
        "sheet": table.sheet,
        "lastRow": last,
        "columns": attachments::column_letters(table.width),
        "rowsFormat": "Each row is [row number, column A, column B, …]. Empty rows are left out.",
        "rows": rows,
        "more": table.rows.iter().filter(|r| r.number >= start).count() > count,
    });
    read(output, format!("Read {} rows from \"{}\" in {}", rows.len(), table.sheet, file.name))
}

// ---------- Proposals ----------

fn title(args: &Value, default: String) -> String {
    arg(args, "title").map(|t| truncate(t, 80)).unwrap_or(default)
}
fn category_or_rta(names: &Names, id: &str) -> Result<Option<String>, String> {
    if id == "ready_to_assign" || id == "__rta" {
        return Ok(None);
    }
    if names.categories.contains_key(id) {
        Ok(Some(id.to_owned()))
    } else {
        Err(format!("No category has the id {id}. Use get_month or get_setup to find ids."))
    }
}
fn propose(change: Change, title: String, ctx: &Context, step: String) -> Result<Outcome, String> {
    let proposal = proposals::build(change, title, ctx.budget)?;
    let output = json!({
        "proposalId": proposal.id,
        "shownToUser": true,
        "summary": proposal.summary,
        "note": "The user sees this as a card and decides whether to apply it. Nothing has changed yet.",
    });
    Ok(Outcome { output, step, proposal: Some(proposal) })
}

fn propose_budget_moves(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let names = Names::new(ctx.budget);
    let month = month_arg(args, "month", ctx.month)?;
    let list = args["moves"].as_array().filter(|m| !m.is_empty()).ok_or("Give at least one move.")?;
    if list.len() > 100 {
        return Err("Propose at most 100 moves at a time.".into());
    }
    let mut moves = vec![];
    for m in list {
        let from = category_or_rta(&names, m["from"].as_str().unwrap_or_default())?;
        let to = category_or_rta(&names, m["to"].as_str().unwrap_or_default())?;
        if from == to {
            return Err("A move needs two different places.".into());
        }
        let amount = parse_money(&m["amount"])?;
        if amount <= 0 {
            return Err("Move amounts must be positive.".into());
        }
        moves.push(Move { from, to, amount });
    }
    let default = format!("Move money in {}", month_label(&month));
    let n = moves.len();
    propose(
        Change::Moves { month, moves },
        title(args, default),
        ctx,
        format!("Prepared {}", plural(n, "budget move", "budget moves")),
    )
}

fn propose_auto_assign(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let names = Names::new(ctx.budget);
    let month = month_arg(args, "month", ctx.month)?;
    let method = arg(args, "method").ok_or("Choose a method.")?.to_owned();
    let mut categories = strings(args, "categoryIds");
    for id in &categories {
        category_or_rta(&names, id)?;
    }
    if categories.is_empty() {
        categories = ctx.budget.categories.iter().filter(|c| !hidden(c)).map(|c| c.id.clone()).collect();
    }
    let label = match method.as_str() {
        "underfunded" => "Fund what targets need",
        "assignedLastMonth" => "Assign what you assigned last month",
        "spentLastMonth" => "Assign what you spent last month",
        "averageAssigned" => "Assign your average assigned",
        "averageSpent" => "Assign your average spending",
        "reduceOverfunding" => "Reduce overfunding",
        "resetAvailable" => "Reset available amounts",
        "resetAssigned" => "Reset assigned amounts",
        _ => return Err("Unknown auto-assign method.".into()),
    };
    let default = format!("{label} in {}", month_label(&month));
    propose(
        Change::AutoAssign { month, method, categories },
        title(args, default),
        ctx,
        format!("Prepared auto-assign: {}", label.to_lowercase()),
    )
}

fn rules_arg(args: &Value, names: &Names) -> Result<Vec<Rule>, String> {
    let mut rules = vec![];
    for r in args["rules"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let matching = r["match"].as_str().map(str::trim).filter(|m| !m.is_empty()).ok_or("A rule needs text to match.")?;
        let category = match r["categoryId"].as_str().filter(|s| !s.is_empty()) {
            Some(id) => category_or_rta(names, id)?,
            None => None,
        };
        rules.push(Rule {
            id: new_id("rule"),
            matching: truncate(matching, 100),
            payee: r["payee"].as_str().map(|p| truncate(p.trim(), 200)).unwrap_or_default(),
            category,
        });
    }
    if rules.len() > 50 {
        return Err("Propose at most 50 rules at a time.".into());
    }
    Ok(rules)
}

fn propose_transaction_edits(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let by_id: HashMap<&str, &Transaction> = b.transactions.iter().map(|t| (t.id.as_str(), t)).collect();
    let list = args["edits"].as_array().ok_or("Give the edits.")?;
    if list.len() > 500 {
        return Err("Propose at most 500 edits at a time.".into());
    }
    let mut edits = vec![];
    for e in list {
        let id = e["id"].as_str().ok_or("Each edit needs a transaction id.")?;
        let t = by_id.get(id).ok_or_else(|| format!("No transaction has the id {id}. Use find_transactions."))?;
        let category = match e["categoryId"].as_str().filter(|s| !s.is_empty()) {
            None => None,
            Some(c) => {
                if t.transfer_account_id.is_some() || t.splits.len() != 1 || t.splits.iter().any(|s| ledger::transfer(s).is_some()) {
                    return Err(format!("{} on {} is a split or a transfer; its category can't be set here.", t.payee, t.date));
                }
                Some(category_or_rta(&names, c)?)
            }
        };
        let flag = e["flag"].as_str().map(|f| if f == "none" { None } else { Some(f.to_owned()) });
        edits.push(Edit {
            id: id.to_owned(),
            category,
            payee: e["payee"].as_str().map(|p| truncate(p.trim(), 200)).filter(|p| !p.is_empty()),
            memo: e["memo"].as_str().map(|m| truncate(m, 500)),
            cleared: e["cleared"].as_str().map(|c| c == "cleared"),
            approve: e["approve"] == json!(true),
            flag,
            unsure: e["unsure"] == json!(true),
        });
    }
    let rules = rules_arg(args, &names)?;
    if edits.is_empty() && rules.is_empty() {
        return Err("Give at least one edit or rule.".into());
    }
    let n = edits.len();
    let default = if rules.is_empty() {
        format!("Change {}", plural(n, "transaction", "transactions"))
    } else {
        format!("Change {}, add {}", plural(n, "transaction", "transactions"), plural(rules.len(), "rule", "rules"))
    };
    propose(
        Change::Edits { edits, rules },
        title(args, default),
        ctx,
        format!("Prepared changes to {}", plural(n, "transaction", "transactions")),
    )
}

fn propose_transactions(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let names = Names::new(ctx.budget);
    let account = arg(args, "accountId").ok_or("Give the accountId.")?;
    let a = names.accounts.get(account).ok_or_else(|| format!("No account has the id {account}."))?;
    if a.archived {
        return Err(format!("{} is closed.", a.name));
    }
    let list = args["transactions"].as_array().filter(|l| !l.is_empty()).ok_or("Give at least one transaction.")?;
    if list.len() > 200 {
        return Err("Propose at most 200 transactions at a time; use prepare_import for statements.".into());
    }
    let mut transactions = vec![];
    for t in list {
        let date = date_arg(t, "date")?.ok_or("Each transaction needs a date.")?;
        let payee = arg(t, "payee").map(|p| truncate(p, 200)).ok_or("Each transaction needs a payee.")?;
        let amount = parse_money(&t["amount"])?;
        let mut splits = vec![];
        if let Some(parts) = t["splits"].as_array().filter(|p| !p.is_empty()) {
            for p in parts {
                let category = match p["categoryId"].as_str().filter(|s| !s.is_empty()) {
                    Some(c) => category_or_rta(&names, c)?,
                    None => None,
                };
                splits.push((category, parse_money(&p["amount"])?, p["memo"].as_str().unwrap_or_default().to_owned()));
            }
            let total: i64 = splits.iter().map(|s| s.1).sum();
            if total != amount {
                return Err(format!(
                    "The splits for {payee} add up to {}, not {}.",
                    money(total),
                    money(amount)
                ));
            }
        } else {
            let category = match t["categoryId"].as_str().filter(|s| !s.is_empty()) {
                Some(c) => category_or_rta(&names, c)?,
                None => None,
            };
            splits.push((category, amount, String::new()));
        }
        transactions.push(NewTransaction {
            id: new_id("tx"),
            date,
            payee,
            memo: t["memo"].as_str().map(|m| truncate(m, 500)).unwrap_or_default(),
            amount,
            cleared: t["cleared"] == json!(true),
            splits,
        });
    }
    let n = transactions.len();
    let from_file = args["fromFile"] == json!(true);
    let default = if n == 1 {
        format!("Add {} · {}", transactions[0].payee, money(transactions[0].amount.abs()))
    } else {
        format!("Add {n} transactions to {}", a.name)
    };
    propose(
        Change::Add { account: account.to_owned(), transactions, from_file },
        title(args, default),
        ctx,
        format!("Prepared {}", plural(n, "new transaction", "new transactions")),
    )
}

fn propose_payee_changes(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let b = ctx.budget;
    let names = Names::new(b);
    let mut renames = vec![];
    for r in args["renames"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let from = r["from"].as_str().filter(|s| !s.is_empty()).ok_or("A rename needs from.")?;
        let to = r["to"].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("A rename needs to.")?;
        if !b.transactions.iter().any(|t| t.payee == from) {
            return Err(format!("No transaction has the payee \"{from}\". Payee names must match exactly."));
        }
        renames.push(Rename { from: from.to_owned(), to: truncate(to, 200) });
    }
    let rules = rules_arg(args, &names)?;
    if renames.is_empty() && rules.is_empty() {
        return Err("Give at least one rename or rule.".into());
    }
    let default = match (renames.len(), rules.len()) {
        (0, r) => format!("Add {}", plural(r, "payee rule", "payee rules")),
        (n, 0) => format!("Rename {}", plural(n, "payee", "payees")),
        (n, r) => format!("Rename {}, add {}", plural(n, "payee", "payees"), plural(r, "rule", "rules")),
    };
    propose(Change::Payees { renames, rules }, title(args, default), ctx, "Prepared payee changes".into())
}

fn create_report(args: &Value) -> Result<Outcome, String> {
    let title = arg(args, "title").ok_or("Give the report a title.")?;
    let body = args["body"].as_str().map(str::trim).filter(|b| !b.is_empty()).ok_or("Write the report's body.")?;
    if body.chars().count() > 40_000 {
        return Err("That report is too long. Keep it under 40,000 characters.".into());
    }
    let title = truncate(title, 120);
    let period = arg(args, "period").map(|p| truncate(p, 80));
    let proposal = proposals::build_report(title.clone(), period, body.to_owned());
    let output = json!({
        "reportId": proposal.id,
        "shownToUser": true,
        "summary": proposal.summary,
        "note": "The user can preview the report and save it as a PDF from its card. Don't repeat it in chat.",
    });
    Ok(Outcome { output, step: format!("Wrote a report: {title}"), proposal: Some(proposal) })
}

fn create_spreadsheet(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let title = truncate(arg(args, "title").ok_or("Give the spreadsheet a title.")?, 100);
    let sheets = workbook::sheets_from(args, ctx.budget, ctx.month)?;
    let proposal = workbook::build(title.clone(), sheets, ctx.budget)?;
    let output = json!({
        "spreadsheetId": proposal.id,
        "shownToUser": true,
        "summary": proposal.summary,
        "note": "The user can open it in their spreadsheet app or save it from its card. Don't repeat the data in chat.",
    });
    Ok(Outcome { output, step: format!("Made a spreadsheet: {title}"), proposal: Some(proposal) })
}

fn prepare_import(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    let names = Names::new(ctx.budget);
    let account = arg(args, "accountId").ok_or("Give the accountId to import into.")?;
    let a = names.accounts.get(account).ok_or_else(|| format!("No account has the id {account}."))?;
    let expected = attachments::Expected {
        count: args["expected"]["count"].as_u64().map(|n| n as usize),
        outflow: args["expected"].get("outflow").filter(|v| !v.is_null()).map(parse_money).transpose()?,
        inflow: args["expected"].get("inflow").filter(|v| !v.is_null()).map(parse_money).transpose()?,
    };
    let (rows, source, skipped) = if let Some(mapping) = args.get("mapping").filter(|m| m.is_object()) {
        let id = arg(args, "attachmentId").ok_or("A mapping needs the attachmentId of the spreadsheet.")?;
        let mapping: attachments::Mapping =
            serde_json::from_value(mapping.clone()).map_err(|e| format!("The mapping is incomplete: {e}"))?;
        let (rows, skipped) = attachments::convert(ctx.files, id, &mapping)?;
        (rows, ctx.files.get(id)?.name.clone(), skipped)
    } else if let Some(list) = args["rows"].as_array().filter(|l| !l.is_empty()) {
        let mut rows = vec![];
        for r in list {
            rows.push(attachments::ImportRow {
                date: date_arg(r, "date")?.ok_or("Each row needs a date.")?,
                payee: arg(r, "payee").map(|p| truncate(p, 200)).unwrap_or_default(),
                memo: r["memo"].as_str().map(|m| truncate(m, 500)).unwrap_or_default(),
                amount: parse_money(&r["amount"])?,
            });
        }
        let source = arg(args, "attachmentId")
            .and_then(|id| ctx.files.get(id).ok())
            .map_or_else(|| "the statement".to_owned(), |f| f.name.clone());
        (rows, source, vec![])
    } else {
        return Err("Give a mapping for a spreadsheet, or the rows.".into());
    };
    if rows.is_empty() {
        return Err("No transactions were found with that mapping. Check the header row and columns.".into());
    }
    if rows.len() > 20_000 {
        return Err("That is more than 20,000 rows. Split the file first.".into());
    }
    let proposal = proposals::build_import(account, &a.name, &source, rows, expected, skipped, args.get("mapping").is_some(), ctx.budget)?;
    let output = json!({
        "proposalId": proposal.id,
        "shownToUser": true,
        "summary": proposal.summary,
        "note": "The user can open the import review from the card. Nothing has been imported.",
    });
    let step = format!("Converted {} for {}", plural(proposal.import.as_ref().map_or(0, |i| i.rows.len()), "transaction", "transactions"), a.name);
    Ok(Outcome { output, step, proposal: Some(proposal) })
}

/// Fixes for the bank file being imported, against its lines as they were when the message was
/// sent: each names a line shown then and the value it had, and carries that line's version, so
/// the import applies it only to a line that hasn't changed since.
fn fix_import(args: &Value, ctx: &Context) -> Result<Outcome, String> {
    use crate::bank_statement::{BriefLine, DateOrder, Field, FieldFix, Fixes, IncludeFix, Status as LineStatus};
    let brief = ctx.import.ok_or("No bank file is open on the Import page. Ask the user to open it there, then try again.")?;
    let shown: HashMap<&str, &BriefLine> = brief.lines.iter().chain(&brief.samples).chain(&brief.left_out).map(|l| (l.id.as_str(), l)).collect();
    let line = |v: &Value| -> Result<&BriefLine, String> {
        let id = v["line"].as_str().ok_or("Each fix needs its line's id.")?;
        shown.get(id).copied().ok_or_else(|| format!("Line {id} isn't one the import's note shows. Fix only those."))
    };
    let reason = |v: &Value| -> Result<String, String> {
        v["reason"].as_str().map(str::trim).filter(|r| !r.is_empty()).map(|r| truncate(r, 200)).ok_or_else(|| "Give a reason for each fix.".to_string())
    };
    let name = proposals::field_name;
    let (mut fields, mut include, mut rows) = (vec![], vec![], vec![]);
    for f in args["fields"].as_array().into_iter().flatten() {
        let l = line(f)?;
        let field: Field = serde_json::from_value(f["field"].clone()).map_err(|_| "A fix's field is date, payee, memo or amount.")?;
        let (now, from) = (l.fields.get(field), f["from"].as_str().unwrap_or(""));
        if now != from {
            return Err(format!("Line {}'s {} is \u{201c}{now}\u{201d}, not \u{201c}{from}\u{201d}.", l.id, name(field)));
        }
        let given = f["to"].as_str().map(str::trim).ok_or("Each fix needs the value to give.")?;
        let mut cents = None;
        let to = match field {
            Field::Date => {
                chrono::NaiveDate::parse_from_str(given, "%Y-%m-%d").map_err(|_| format!("Give line {}'s date as YYYY-MM-DD.", l.id))?;
                given.to_string()
            }
            Field::Amount => match parse_money(&json!(given))? {
                0 => return Err(format!("Line {}'s amount can't be zero.", l.id)),
                c => {
                    cents = Some(c);
                    money(c)
                }
            },
            Field::Payee => truncate(given, 200),
            Field::Memo => truncate(given, 500),
        };
        if to == from.trim() {
            return Err(format!("Line {}'s {} is \u{201c}{from}\u{201d} already: fix only what needs it.", l.id, name(field)));
        }
        // An amount given can be another on the card before it's applied.
        rows.push(proposals::Row {
            label: proposals::fix_label(&l.id, field, from, &to),
            detail: Some(reason(f)?),
            amount: cents,
            key: Some(proposals::fix_key(&l.id, field)),
            edit: cents.map(|_| proposals::RowEdit { amount: true, ..Default::default() }),
            ..Default::default()
        });
        fields.push(FieldFix { line: l.id.clone(), line_version: l.version, field, from: from.into(), to });
    }
    for f in args["include"].as_array().into_iter().flatten() {
        let l = line(f)?;
        let on = f["include"].as_bool().unwrap_or(true);
        if on && !matches!(l.status, LineStatus::LeftOut { reason, .. } if reason.includable()) {
            return Err(format!("Line {} can't be included: only lines left out for a reason someone can overrule can.", l.id));
        }
        rows.push(proposals::Row {
            label: format!("Line {} · {}", l.id, if on { "include it" } else { "leave it out" }),
            detail: Some(reason(f)?),
            key: Some(format!("i:{}", l.id)),
            ..Default::default()
        });
        include.push(IncludeFix { line: l.id.clone(), line_version: l.version, include: on });
    }
    let read = &args["readAs"];
    let mut structure = brief.structure.clone();
    if let Some(order) = read["dateOrder"].as_str() {
        if !matches!(order, "ymd" | "dmy" | "mdy") {
            return Err("The date order is ymd, dmy or mdy.".into());
        }
        structure.date_order = DateOrder { order: order.into(), confirmed: true };
        rows.push(proposals::Row { label: format!("Dates written {}", match order { "dmy" => "day first", "mdy" => "month first", _ => "year first" }), ..Default::default() });
    }
    if let Some(on) = read["signsByType"].as_bool() {
        structure.signs_by_type = Some(on);
        rows.push(proposals::Row { label: if on { "Money out told by the transaction types" } else { "Amounts as written" }.into(), ..Default::default() });
    }
    if let Some(on) = read["flipSigns"].as_bool() {
        structure.flip_signs = on;
        rows.push(proposals::Row { label: if on { "Purchases written as positive" } else { "Amounts as written" }.into(), ..Default::default() });
    }
    if let Some(columns) = read.get("columns").filter(|c| c.is_object()) {
        let mut chosen = structure.columns.clone().unwrap_or_default();
        for (role, slot) in [("date", &mut chosen.date), ("payee", &mut chosen.payee), ("memo", &mut chosen.memo), ("amount", &mut chosen.amount), ("outflow", &mut chosen.outflow), ("inflow", &mut chosen.inflow)] {
            if let Some(header) = columns[role].as_str() {
                if !brief.headers.iter().any(|h| h.trim() == header.trim()) {
                    return Err(format!("The file has no column named \u{201c}{header}\u{201d}."));
                }
                *slot = Some(header.into());
                rows.push(proposals::Row { label: format!("{role} from \u{201c}{header}\u{201d}"), ..Default::default() });
            }
        }
        structure.columns = Some(chosen);
    }
    let restructured = structure != brief.structure;
    // How the file reads is one change, left out (or not) as a whole.
    for row in rows.iter_mut().filter(|r| r.key.is_none()) {
        row.key = Some("x".into());
    }
    if fields.is_empty() && include.is_empty() && !restructured {
        return Err("Give at least one fix.".into());
    }
    if fields.len() + include.len() > 200 {
        return Err("Give at most 200 fixes at a time.".into());
    }
    let n = fields.len() + include.len() + restructured as usize;
    let fixes = Fixes {
        scope: brief.scope.clone(),
        base_revision: brief.revision,
        base_structure: brief.structure.clone(),
        structure: restructured.then_some(structure),
        fields,
        include,
        ..Fixes::default()
    };
    let note = arg(args, "note").map(|n| truncate(n, 400));
    let proposal = proposals::build_fix(fixes, rows, note);
    let output = json!({
        "proposalId": proposal.id,
        "shownToUser": true,
        "summary": proposal.summary,
        "note": "The user applies these from the card, on the Import page. Each applies only to a line that hasn't changed since.",
    });
    Ok(Outcome { output, step: format!("Prepared {} for the import", plural(n, "fix", "fixes")), proposal: Some(proposal) })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn sample() -> Budget {
        serde_json::from_value(json!({
            "schemaVersion": 2, "name": "Test", "currency": "SAR",
            "accounts": [
                { "id": "checking", "name": "Everyday", "kind": "cash", "openingBalance": 500000, "archived": false },
                { "id": "card", "name": "Card", "kind": "credit", "openingBalance": 0, "archived": false }
            ],
            "categories": [
                { "id": "dining", "name": "Dining out", "group": "Fun", "target": 100000 },
                { "id": "coffee", "name": "Coffee", "group": "Fun", "target": 20000 },
                { "id": "groceries", "name": "Groceries", "group": "Needs", "target": 150000 },
                { "id": "card-pay", "name": "Card", "group": "Credit Card Payments", "target": 0, "paymentAccountId": "card" }
            ],
            "assignments": [
                { "month": "2026-10", "categoryId": "dining", "amount": 100000 },
                { "month": "2026-10", "categoryId": "coffee", "amount": 20000 }
            ],
            "transactions": [
                { "id": "t1", "date": "2026-10-03", "accountId": "checking", "payee": "Bistro", "memo": "", "amount": -124000, "status": "cleared",
                  "splits": [{ "categoryId": "dining", "amount": -124000 }], "transferAccountId": null, "transferStatus": null },
                { "id": "t2", "date": "2026-10-05", "accountId": "checking", "payee": "Corner Bakery", "memo": "croissant", "amount": -1850, "status": "uncleared",
                  "splits": [{ "categoryId": null, "amount": -1850 }], "transferAccountId": null, "transferStatus": null, "approved": false },
                { "id": "t3", "date": "2026-09-05", "accountId": "checking", "payee": "Corner Bakery", "memo": "", "amount": -2200, "status": "cleared",
                  "splits": [{ "categoryId": "coffee", "amount": -2200 }], "transferAccountId": null, "transferStatus": null }
            ]
        }))
        .unwrap()
    }
    fn ctx<'a>(b: &'a Budget, files: &'a Files) -> Context<'a> {
        Context { budget: b, today: "2026-10-16", month: "2026-10", share_memos: true, files, bank_sync: &Value::Null, import: None }
    }

    /// A file with one line whose amount doesn't read and one left out (pending), its session, and
    /// what the assistant is told of it.
    fn importing() -> (Budget, String, crate::bank_statement::Session, crate::bank_statement::Brief) {
        use crate::bank_statement::{statement, DateOrder, Session, Structure};
        let b = sample();
        let file = "Date,Payee,Amount,Status\n2026-10-01,Bakery,-3.20,Booked\n2026-10-02,Grocer,n/a,Booked\n2026-10-03,Cinema,-12.00,Pending\n".to_string();
        let structure = Structure { format: "csv".into(), date_order: DateOrder { order: "ymd".into(), confirmed: false }, ..Structure::default() };
        let session = Session::new("s1", "b", "checking", &file, structure);
        let brief = statement(&b, "checking", &file, &session).unwrap().brief(true);
        (b, file, session, brief)
    }
    fn fix(args: Value, brief: &crate::bank_statement::Brief, b: &Budget) -> Result<Proposal, String> {
        let files = Files::empty();
        let outcome = call("fix_import", &args, &Context { import: Some(brief), ..ctx(b, &files) });
        match outcome.proposal {
            Some(p) => Ok(p),
            None => Err(outcome.output["error"].as_str().unwrap_or_default().to_string()),
        }
    }

    #[test]
    fn a_fix_includes_a_line_left_out_and_reads_another_amount_column() {
        use crate::bank_statement::{apply_fixes, preview_statement, statement, DateOrder, Session, Structure};
        let b = sample();
        let file = "Date,Payee,Amount,Status,Corrected\n2026-10-01,Bakery,-3.20,Booked,-3.40\n2026-10-03,Cinema,-12.00,Pending,-12.00\n".to_string();
        let structure = Structure { format: "csv".into(), date_order: DateOrder { order: "ymd".into(), confirmed: false }, ..Structure::default() };
        let session = Session::new("s1", "b", "checking", &file, structure);
        let brief = statement(&b, "checking", &file, &session).unwrap().brief(true);
        // The pending line is shown, to include when the user asks.
        let cinema = brief.left_out.iter().find(|l| l.cells[1] == "Cinema").unwrap().id.clone();
        let p = fix(json!({ "include": [{ "line": cinema, "include": true, "reason": "The user asked for it" }], "note": "x" }), &brief, &b).unwrap();
        let applied = apply_fixes(&b, "checking", &file, &session, p.fixes.as_ref().unwrap()).unwrap();
        assert!(applied.results.iter().all(|r| r.applied));
        assert_eq!(preview_statement(&b, "checking", &file, &applied.session).unwrap().review.items.len(), 2);
        // Another amount column, named alone: the others stay as the file names them.
        let p = fix(json!({ "readAs": { "columns": { "amount": "Corrected" } }, "note": "x" }), &brief, &b).unwrap();
        let applied = apply_fixes(&b, "checking", &file, &session, p.fixes.as_ref().unwrap()).unwrap();
        assert_eq!(preview_statement(&b, "checking", &file, &applied.session).unwrap().review.items[0].row.amount, -340);
    }

    #[test]
    fn fixes_for_an_import_name_lines_it_was_shown_as_they_were() {
        let (b, _, _, brief) = importing();
        let grocer = brief.lines.iter().find(|l| l.cells[1] == "Grocer").unwrap().id.clone();
        let amount = |from: &str, to: &str| json!({ "fields": [{ "line": grocer, "field": "amount", "from": from, "to": to, "reason": "The statement's total" }], "note": "x" });
        // Nothing open: nothing to fix.
        let files = Files::empty();
        let none = call("fix_import", &amount("n/a", "-12.40"), &ctx(&b, &files));
        assert!(none.output["error"].as_str().unwrap().contains("No bank file is open"));
        // Only lines it was shown, as they were, with values that read.
        assert!(fix(json!({ "fields": [{ "line": "999", "field": "amount", "from": "", "to": "1", "reason": "r" }], "note": "x" }), &brief, &b).unwrap_err().contains("isn't one"));
        assert!(fix(amount("12", "-12.40"), &brief, &b).unwrap_err().contains("not \u{201c}12\u{201d}"));
        assert!(fix(amount("n/a", "twelve"), &brief, &b).is_err());
        assert!(fix(amount("n/a", "0"), &brief, &b).unwrap_err().contains("zero"));
        let same = json!({ "fields": [{ "line": grocer, "field": "payee", "from": "Grocer", "to": " Grocer", "reason": "r" }], "note": "x" });
        assert!(fix(same, &brief, &b).unwrap_err().contains("already"));
        assert!(fix(json!({ "fields": [{ "line": grocer, "field": "date", "from": "2026-10-02", "to": "02/10/2026", "reason": "r" }], "note": "x" }), &brief, &b).unwrap_err().contains("YYYY-MM-DD"));
        assert!(fix(json!({ "fields": [{ "line": grocer, "field": "amount", "from": "n/a", "to": "-12.40" }], "note": "x" }), &brief, &b).unwrap_err().contains("reason"));
        assert!(fix(json!({ "note": "nothing" }), &brief, &b).unwrap_err().contains("at least one"));
        // A line that reads can't be included, one pending can.
        let bakery = brief.samples.iter().find(|l| l.cells[1] == "Bakery").map(|l| l.id.clone());
        if let Some(bakery) = bakery {
            assert!(fix(json!({ "include": [{ "line": bakery, "include": true, "reason": "r" }], "note": "x" }), &brief, &b).unwrap_err().contains("can't be included"));
        }
        // The fix set is made against the lines as they were shown: their versions, the revision
        // and the way the file read then.
        let p = fix(amount("n/a", "-12.4"), &brief, &b).unwrap();
        let fixes = p.fixes.as_ref().unwrap();
        assert_eq!((p.kind.as_str(), fixes.base_revision, &fixes.scope, &fixes.base_structure), ("fix", brief.revision, &brief.scope, &brief.structure));
        assert_eq!((fixes.fields[0].line.as_str(), fixes.fields[0].line_version, fixes.fields[0].to.as_str()), (grocer.as_str(), 0, "-12.40"));
        assert!(fixes.structure.is_none());
        assert_eq!(p.card.rows[0].detail.as_deref(), Some("The statement's total"));
        // How the file reads, too: confirmed, as it's the assistant's answer to the question.
        let p = fix(json!({ "readAs": { "dateOrder": "dmy", "columns": { "amount": "Amount" } }, "note": "x" }), &brief, &b).unwrap();
        let structure = p.fixes.unwrap().structure.unwrap();
        assert_eq!((structure.date_order.order.as_str(), structure.date_order.confirmed), ("dmy", true));
        assert!(fix(json!({ "readAs": { "columns": { "amount": "Betrag" } }, "note": "x" }), &brief, &b).unwrap_err().contains("no column"));
    }

    #[test]
    fn a_fix_applies_to_the_import_only_where_its_line_hasnt_changed_since() {
        use crate::bank_statement::{apply_fixes, statement, Fixes, IncludeFix};
        let (b, file, session, brief) = importing();
        let grocer = brief.lines.iter().find(|l| l.cells[1] == "Grocer").unwrap().id.clone();
        let p = fix(json!({ "fields": [{ "line": grocer, "field": "amount", "from": "n/a", "to": "-12.40", "reason": "r" }], "note": "x" }), &brief, &b).unwrap();
        let fixes: Fixes = p.fixes.unwrap();
        // Applied to the import it was made for, it fixes the line.
        let applied = apply_fixes(&b, "checking", &file, &session, &fixes).unwrap();
        assert!(applied.results.iter().all(|r| r.applied));
        assert_eq!(statement(&b, "checking", &file, &applied.session).unwrap().counts.attention, 0);
        // The user changed that line while the assistant was answering: the fix is left alone.
        let cinema = brief.lines.iter().chain(&brief.samples).find(|l| l.cells[1] == "Cinema");
        let edited = apply_fixes(&b, "checking", &file, &session, &Fixes {
            scope: session.scope.clone(),
            base_structure: session.structure.clone(),
            fields: vec![crate::bank_statement::FieldFix { line: grocer.clone(), line_version: 0, field: crate::bank_statement::Field::Amount, from: "n/a".into(), to: "-1.00".into() }],
            include: cinema.map(|c| vec![IncludeFix { line: c.id.clone(), line_version: 0, include: true }]).unwrap_or_default(),
            ..Fixes::default()
        }).unwrap();
        let late = apply_fixes(&b, "checking", &file, &edited.session, &fixes).unwrap();
        assert_eq!(late.results[0].problem.as_deref(), Some("This line changed since this fix was made."));
        // Nor does it reach another import.
        let mut other = session.clone();
        other.scope.session = "s2".into();
        assert!(apply_fixes(&b, "checking", &file, &other, &fixes).is_err());
        // The note says which lines need what.
        let note = super::super::prompt::context(&b, &super::super::prompt::Screen { page: "import".into(), today: "2026-10-16".into(), ..Default::default() }, true, &[], Some(&brief));
        assert!(note.contains("fix_import") && note.contains(&grocer) && note.contains("Invalid amount: n/a"), "{note}");
        assert!(note.contains("on the Import page") && note.contains("into Everyday"), "{note}");
    }

    #[test]
    fn bank_sync_shows_links_beside_what_simplefin_last_said() {
        let mut b = sample();
        let checking = b.accounts.iter_mut().find(|a| a.id == "checking").unwrap();
        checking.extra.insert(
            "simplefin".into(),
            json!({"id": "ACT-1", "name": "Everyday Checking", "org": "First Bank", "from": "2026-09-16", "lastSync": "2026-10-15"}),
        );
        let files = Files::empty();
        let sync = json!({
            "available": true, "connected": true, "requestsToday": 3, "dailyLimit": 20,
            "accounts": [
                {"id": "ACT-1", "name": "Everyday Checking", "org": "First Bank", "currency": "USD", "balance": 152015, "balanceDate": "2026-10-16"},
                {"id": "ACT-2", "name": "Rewards Card", "org": "First Bank", "currency": "USD", "balance": -21234, "balanceDate": "2026-10-16"}
            ],
            "messages": ["First Bank: Sign in again."]
        });
        let context = Context { bank_sync: &sync, ..ctx(&b, &files) };
        let out = call("get_bank_sync", &json!({}), &context);
        assert_eq!(out.step, "Checked bank sync for 1 account");
        let linked = &out.output["linked"][0];
        assert_eq!((linked["account"].as_str(), linked["bankAccount"].as_str()), (Some("Everyday"), Some("Everyday Checking")));
        assert_eq!((linked["lastSync"].as_str(), linked["bankBalance"].as_str()), (Some("2026-10-15"), Some("1520.15")));
        assert_eq!(linked["purchasesArriveAsMoneyIn"], false);
        assert_eq!(out.output["notLinked"][0]["balance"], "-212.34");
        assert_eq!(out.output["messages"][0], "First Bank: Sign in again.");
        // Outside the app nothing is known about the connection, and the setup token never shows.
        let out = call("get_bank_sync", &json!({}), &ctx(&b, &files));
        assert_eq!(out.output["connected"], Value::Null);
        assert!(!out.output.to_string().contains("access"));
    }

    #[test]
    fn get_month_reports_overspending_in_the_step() {
        let b = sample();
        let files = Files::empty();
        let out = call("get_month", &json!({}), &ctx(&b, &files));
        assert_eq!(out.output["overspent"][0]["id"], "dining");
        assert_eq!(out.output["overspent"][0]["available"], "-240.00");
        assert!(out.step.starts_with("Checked October 2026: 1 category is overspent"), "{}", out.step);
    }

    #[test]
    fn find_transactions_filters_uncategorized_and_unapproved() {
        let b = sample();
        let files = Files::empty();
        let out = call("find_transactions", &json!({ "uncategorized": true }), &ctx(&b, &files));
        assert_eq!(out.output["total"], 1);
        assert_eq!(out.output["transactions"][0]["category"], "Uncategorized");
        assert_eq!(out.output["transactions"][0]["approved"], false);
        let none = call("find_transactions", &json!({ "accountId": "nope" }), &ctx(&b, &files));
        assert!(none.output["error"].as_str().unwrap().contains("No account"));
    }

    #[test]
    fn memos_stay_out_when_sharing_is_off() {
        let b = sample();
        let files = Files::empty();
        let mut c = ctx(&b, &files);
        c.share_memos = false;
        let out = call("find_transactions", &json!({ "payee": "corner bakery" }), &c);
        assert!(out.output["transactions"].as_array().unwrap().iter().all(|t| t.get("memo").is_none()));
    }

    #[test]
    fn payee_history_counts_categories() {
        let b = sample();
        let files = Files::empty();
        let out = call("payee_history", &json!({ "payees": ["Corner Bakery"] }), &ctx(&b, &files));
        assert_eq!(out.output["payees"][0]["transactions"], 2);
        assert_eq!(out.output["payees"][0]["categories"][0]["category"], "Coffee");
    }

    #[test]
    fn a_proposal_reports_back_without_changing_the_budget() {
        let b = sample();
        let before = b.clone();
        let files = Files::empty();
        let out = call(
            "propose_budget_moves",
            &json!({ "month": "2026-10", "moves": [{ "from": "ready_to_assign", "to": "dining", "amount": 240 }] }),
            &ctx(&b, &files),
        );
        assert!(out.proposal.is_some(), "{}", out.output);
        assert_eq!(b, before);
        assert_eq!(out.output["shownToUser"], true);
    }

    #[test]
    fn a_report_lists_its_sections_and_changes_nothing() {
        let b = sample();
        let before = b.clone();
        let files = Files::empty();
        let out = call(
            "create_report",
            &json!({
                "title": "October 2026 in review",
                "period": "October 2026",
                "body": "October went well.\n\n## Spending\n\n| Category | Spent |\n|---|---:|\n| Dining out | 1,240.00 |\n\n## Next month\n\n- Raise the dining target",
            }),
            &ctx(&b, &files),
        );
        let p = out.proposal.expect("a report card");
        assert_eq!(p.kind, "report");
        assert_eq!(p.card.rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["Spending", "Next month"]);
        assert!(p.report.as_ref().unwrap().body.contains("| Dining out |"));
        assert_eq!(out.step, "Wrote a report: October 2026 in review");
        assert_eq!(b, before);
        let empty = call("create_report", &json!({ "title": "Nothing", "body": "  " }), &ctx(&b, &files));
        assert!(empty.proposal.is_none());
    }

    #[test]
    fn a_spreadsheet_card_counts_each_sheet_and_changes_nothing() {
        let b = sample();
        let before = b.clone();
        let files = Files::empty();
        let out = call(
            "create_spreadsheet",
            &json!({
                "title": "Fun money",
                "sheets": [
                    { "name": "Dining", "type": "transactions", "categoryIds": ["dining"] },
                    { "name": "October", "type": "budget" },
                    { "name": "Rent", "type": "transactions", "payee": "Landlord" }
                ]
            }),
            &ctx(&b, &files),
        );
        let p = out.proposal.expect("a spreadsheet card");
        assert_eq!(p.kind, "spreadsheet");
        assert_eq!(p.card.rows[0].badge.as_deref(), Some("1 row"));
        assert_eq!(p.card.rows[1].badge.as_deref(), Some("4 categories"));
        assert_eq!(p.card.rows[2].tone.as_deref(), Some("warn"));
        assert!(p.card.warning.is_some());
        assert_eq!(out.output["summary"]["sheets"][0]["rows"], 1);
        assert_eq!(out.step, "Made a spreadsheet: Fun money");
        assert_eq!(b, before);
        let bad = call("create_spreadsheet", &json!({ "title": "X", "sheets": [] }), &ctx(&b, &files));
        assert!(bad.step.starts_with("Couldn't make that spreadsheet"), "{}", bad.step);
    }

    #[test]
    fn a_payment_app_export_with_lines_above_the_header_converts_for_review() {
        // A made-up file laid out the way payment apps export activity: statement lines above the
        // header, dates with a time, and amounts written like "- $1,240.00".
        let b = sample();
        let mut files = Files::empty();
        let csv = "Account Statement - (@sam-lee),,,,,,,,\nAccount Activity,,,,,,,,\n,ID,Datetime,Type,Status,Note,From,To,Amount (total)\n,4101,2026-10-03T23:30:00,Payment,Complete,Dinner,Sam Lee,Bistro,\"- $1,240.00\"\n,4102,2026-10-14T12:34:56,Payment,Complete,Rent share,Alex Kim,Sam Lee,+ $25.00\n,,,,,,,,\n,,,,,,,Ending balance,\"$1,265.00\"\n";
        let meta = files.insert("activity.csv", csv.as_bytes().to_vec());
        let mapping = json!({ "headerRow": 3, "date": "C", "payee": "H", "memo": "F", "amount": "I", "dateOrder": "ymd" });
        let out = call(
            "prepare_import",
            &json!({ "accountId": "checking", "attachmentId": meta.id, "mapping": mapping }),
            &ctx(&b, &files),
        );
        let p = out.proposal.expect("an import card");
        let rows = &p.import.as_ref().unwrap().rows;
        assert_eq!(rows.len(), 2);
        // The calendar date as written: 11:30 pm stays on the 3rd, whatever the computer's time zone.
        assert_eq!((rows[0].date.as_str(), rows[0].memo.as_str(), rows[0].amount), ("2026-10-03", "Dinner", -124000));
        assert_eq!((rows[1].date.as_str(), rows[1].payee.as_str(), rows[1].amount), ("2026-10-14", "Sam Lee", 2500));
        // The ending balance isn't a transaction: it's listed as left out, not imported.
        assert_eq!(out.output["summary"]["skippedRows"].as_array().unwrap().len(), 1);
        // The dinner was already entered by hand, so the review offers to match it rather than add it twice.
        assert_eq!(out.output["summary"]["matchesExisting"], 1);
        assert_eq!(out.output["summary"]["duplicates"], 0);
    }

    #[test]
    fn unknown_ids_are_explained() {
        let b = sample();
        let files = Files::empty();
        let out = call(
            "propose_transaction_edits",
            &json!({ "edits": [{ "id": "missing", "categoryId": "coffee" }] }),
            &ctx(&b, &files),
        );
        assert!(out.proposal.is_none());
        assert!(out.output["error"].as_str().unwrap().contains("find_transactions"));
    }
}
