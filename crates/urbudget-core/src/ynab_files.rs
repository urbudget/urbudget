//! YNAB's own data export: the .zip its web app downloads, holding a Register and a Plan CSV.
//! Reading it needs no API access or token. The files carry no ids, targets, notes, account
//! types or split-parent memos, and write amounts and dates in the plan's display format, so
//! those are inferred here. The result is a new budget, checked against the Plan's monthly
//! figures; differences are reported, never patched over.
use crate::{i18n::tr, t};
use crate::domain::*;
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;

/// One chosen file: the export's .zip, or one of its .csv files.
#[derive(Deserialize)]
pub struct ExportFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Match the picker limits in YnabFileImport.tsx; native callers are bounded too.
const MAX_FILE: usize = 8 * 1024 * 1024;
const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 8;
/// Bound both individual CSVs and the entire expanded selection, including multiple archives.
const MAX_CSV: u64 = 32 * 1024 * 1024;
const MAX_EXPANDED: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;
// Tiny records can expand into far more heap objects than their byte size suggests.
const MAX_ROWS: usize = 200_000;
const MAX_COLUMNS: usize = 128;
const TRANSFER: &str = "Transfer : ";
const CARD_PAYMENTS: &str = "Credit Card Payments";
/// Where YNAB's export lists hidden categories, in place of their own group.
const HIDDEN: &str = "Hidden Categories";

struct Table {
    name: String,
    header: Vec<String>,
    rows: Vec<csv::StringRecord>,
}
impl Table {
    fn column(&self, name: &str) -> Option<usize> {
        self.header.iter().position(|h| h == name)
    }
    fn has(&self, names: &[&str]) -> bool {
        names.iter().all(|n| self.column(n).is_some())
    }
}
fn field(r: &csv::StringRecord, column: Option<usize>) -> &str {
    column.and_then(|c| r.get(c)).unwrap_or("").trim()
}

fn table(name: &str, bytes: &[u8], remaining_rows: &mut usize) -> Result<Table, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| t!("{name} isn't a text file.", name))?;
    let text = text.trim_start_matches('\u{feff}');
    // A plan whose amounts use a decimal comma is exported with tabs between the columns.
    let tabs = text.lines().next().is_some_and(|header| header.contains('\t'));
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .delimiter(if tabs { b'\t' } else { b',' })
        .from_reader(text.as_bytes());
    let headers = reader.headers().map_err(|e| format!("{name}: {e}"))?;
    if headers.len() > MAX_COLUMNS {
        return Err(t!("{name} has more than 128 columns and isn't a supported YNAB export.", name));
    }
    let header = headers.iter().map(|h| h.trim().to_string()).collect();
    let mut rows = vec![];
    for row in reader.records() {
        if *remaining_rows == 0 {
            return Err(t!("The export contains more than 200,000 CSV rows. Use YNAB's token import instead."));
        }
        let row = row.map_err(|e| format!("{name}: {e}"))?;
        if row.len() > MAX_COLUMNS {
            return Err(t!("{name} has more than 128 columns and isn't a supported YNAB export.", name));
        }
        *remaining_rows -= 1;
        rows.push(row);
    }
    Ok(Table {
        name: name.into(),
        header,
        rows,
    })
}

/// An export's Register or Plan: YNAB names them .tsv for a plan written with a decimal comma.
fn spreadsheet(name: &str) -> bool {
    let name = name.to_lowercase();
    name.ends_with(".csv") || name.ends_with(".tsv")
}

/// Every CSV among the chosen files, opening a .zip to read the ones inside it.
fn tables(files: &[ExportFile]) -> Result<Vec<Table>, String> {
    if files.len() > MAX_FILES {
        return Err(t!("Choose at most 8 export files at once."));
    }
    let mut input = 0;
    for f in files {
        if f.bytes.len() > MAX_FILE {
            return Err(t!("{name} is larger than 8 MB. Use YNAB's token import for larger budgets.", name = f.name));
        }
        input += f.bytes.len();
        if input > MAX_INPUT {
            return Err(t!("These export files total more than 16 MB. Use YNAB's token import for larger budgets."));
        }
    }
    // Inspect all declared sizes before parsing any CSV: an archive bomb must fail before
    // earlier entries have already filled memory with parsed rows.
    let mut declared_expanded = 0;
    let mut entries = 0;
    for f in files {
        if !f.bytes.starts_with(b"PK\x03\x04") {
            declared_expanded += f.bytes.len() as u64;
        } else {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&f.bytes[..]))
                .map_err(|_| t!("{name} isn't a readable .zip file.", name = f.name))?;
            entries += archive.len();
            if entries > MAX_ENTRIES {
                return Err(t!("The export contains more than 64 archive entries. Choose only YNAB's Register and Plan files."));
            }
            for i in 0..archive.len() {
                let entry = archive.by_index(i).map_err(|_| t!("{name} isn't a readable .zip file.", name = f.name))?;
                let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
                if entry.is_dir() || name.starts_with("__MACOSX") || !spreadsheet(&name) {
                    continue;
                }
                if entry.size() > MAX_CSV {
                    return Err(t!("{name} expands to more than 32 MB. Use YNAB's token import instead.", name));
                }
                declared_expanded += entry.size();
            }
        }
        if declared_expanded > MAX_EXPANDED {
            return Err(t!("The export expands to more than 64 MB of CSV data. Use YNAB's token import instead."));
        }
    }
    let mut out = vec![];
    let mut expanded = 0;
    let mut remaining_rows = MAX_ROWS;
    for f in files {
        if !f.bytes.starts_with(b"PK\x03\x04") {
            expanded += f.bytes.len() as u64;
            if expanded > MAX_EXPANDED {
                return Err(t!("The export expands to more than 64 MB of CSV data. Use YNAB's token import instead."));
            }
            out.push(table(&f.name, &f.bytes, &mut remaining_rows)?);
            continue;
        }
        let unreadable = |_: zip::result::ZipError| t!("{name} isn't a readable .zip file.", name = f.name);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&f.bytes[..])).map_err(unreadable)?;
        for i in 0..archive.len() {
            let entry = archive.by_index(i).map_err(unreadable)?;
            let name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
            if entry.is_dir() || name.starts_with("__MACOSX") || !spreadsheet(&name) {
                continue;
            }
            if entry.size() > MAX_CSV {
                return Err(t!("{name} expands to more than 32 MB. Use YNAB's token import instead.", name));
            }
            let remaining = MAX_EXPANDED - expanded;
            if entry.size() > remaining {
                return Err(t!("The export expands to more than 64 MB of CSV data. Use YNAB's token import instead."));
            }
            let mut bytes = vec![];
            entry
                .take(MAX_CSV.min(remaining) + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| t!("{name} in {file} couldn't be read.", name, file = f.name))?;
            if bytes.len() as u64 > MAX_CSV {
                return Err(t!("{name} is too large to import.", name));
            }
            expanded += bytes.len() as u64;
            if expanded > MAX_EXPANDED {
                return Err(t!("The export expands to more than 64 MB of CSV data. Use YNAB's token import instead."));
            }
            out.push(table(&name, &bytes, &mut remaining_rows)?);
        }
    }
    Ok(out)
}

/// The decimal mark. YNAB writes every amount with the plan's fixed decimals ("$1,234.56",
/// "1200,00€", "1200-00kr", "SAR 0.00"), so it is whichever of the marks YNAB offers (. , - /)
/// has exactly two digits after it.
fn decimal_mark<'a>(amounts: impl Iterator<Item = &'a str>) -> Result<char, String> {
    let mut mark = None;
    for s in amounts {
        let core: Vec<char> = s.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '/')).collect();
        if core.is_empty() {
            continue;
        }
        match core.iter().rposition(|c| !c.is_ascii_digit()) {
            Some(i) if core.len() - i - 1 == 2 && mark.is_none_or(|m| m == core[i]) => mark = Some(core[i]),
            Some(i) if core.len() - i - 1 == 2 => return Err(t!("The export mixes number formats.")),
            _ => {
                return Err(t!(
                    "Amounts such as “{amount}” don't have two decimal places. UrBudget keeps money in cents, so it can't read this plan's currency.",
                    amount = s
                ))
            }
        }
    }
    Ok(mark.unwrap_or('.'))
}
/// An amount in cents, whatever symbol, grouping and minus style surrounds it.
fn money(s: &str, mark: char) -> Result<i64, String> {
    // With a "-" mark ("-225-00kr") a minus sign can only come before the digits.
    let first = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    let (lead, body) = if mark == '-' { s.split_at(first) } else { ("", s) };
    let core: String = body.chars().filter(|c| c.is_ascii_digit() || *c == mark).collect();
    if core.is_empty() {
        return Ok(0);
    }
    let bad = || t!("unreadable amount “{amount}”", amount = s);
    let (whole, cents) = core.split_once(mark).ok_or_else(bad)?;
    if cents.len() != 2 || cents.contains(mark) {
        return Err(bad());
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| bad())? };
    let n = whole
        .checked_mul(100)
        .and_then(|n| n.checked_add(cents.parse().ok()?))
        .ok_or_else(bad)?;
    if !valid_money(n) { return Err(t!("amount “{amount}” exceeds the supported money range", amount = s)); }
    let negative = if mark == '-' {
        lead.contains(['-', '−', '('])
    } else {
        s.contains('-') || s.contains('−') || s.starts_with('(')
    };
    Ok(if negative { -n } else { n })
}
/// The symbols and codes YNAB writes amounts with, and the currencies each can mean, the likeliest
/// first. Longer ones come first, so "R$" isn't read as "$".
const SYMBOLS: &[(&str, &[&str])] = &[
    ("SAR", &["SAR"]), ("ر.س", &["SAR"]), ("AED", &["AED"]), ("د.إ", &["AED"]), ("EUR", &["EUR"]), ("GBP", &["GBP"]),
    ("CHF", &["CHF"]), ("R$", &["BRL"]), ("zł", &["PLN"]), ("Kč", &["CZK"]), ("Ft", &["HUF"]), ("kr", &["SEK", "NOK", "DKK", "ISK"]),
    ("€", &["EUR"]), ("£", &["GBP"]), ("₹", &["INR"]), ("¥", &["JPY", "CNY"]), ("₩", &["KRW"]), ("₺", &["TRY"]), ("₽", &["RUB"]),
    ("₪", &["ILS"]), ("₱", &["PHP"]), ("฿", &["THB"]), ("₫", &["VND"]), ("₦", &["NGN"]), ("₴", &["UAH"]),
    ("$", &["USD", "CAD", "AUD", "NZD", "SGD", "HKD", "MXN"]),
];
/// The currencies a plan's amounts can be in, from the symbol or code they're written with.
fn currency_in(amount: &str) -> Option<&'static [&'static str]> {
    SYMBOLS.iter().find(|(mark, _)| amount.contains(mark)).map(|(_, codes)| *codes)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Order {
    Ymd,
    Dmy,
    Mdy,
}
/// "15/10/2026", "10-15-2026", "2026.10.15" and so on as an ISO date, read in `order`.
fn iso(s: &str, order: Order) -> Option<String> {
    if !s.starts_with(|c: char| c.is_ascii_digit()) || !s.ends_with(|c: char| c.is_ascii_digit()) { return None; }
    let parts: Vec<&str> = s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).collect();
    let [a, b, c] = parts[..] else { return None };
    let (y, m, d) = match order {
        Order::Ymd => (a, b, c),
        Order::Dmy => (c, b, a),
        Order::Mdy => (c, a, b),
    };
    if y.len() != 4 {
        return None;
    }
    NaiveDate::from_ymd_opt(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
        .map(|d| d.format("%Y-%m-%d").to_string())
}
/// The date order that reads every date: the one chosen, or the only one that fits.
fn date_order(dates: &[&str], chosen: Option<&str>) -> Result<Order, String> {
    let candidates = match chosen {
        Some("ymd") => vec![Order::Ymd],
        Some("dmy") => vec![Order::Dmy],
        Some("mdy") => vec![Order::Mdy],
        _ => vec![Order::Ymd, Order::Dmy, Order::Mdy],
    };
    let fits: Vec<Order> = candidates
        .into_iter()
        .filter(|o| dates.iter().all(|d| iso(d, *o).is_some()))
        .collect();
    match fits[..] {
        [order] => Ok(order),
        [] => Err(t!(
            "Dates in the register (such as “{date}”) don't match {expected}.",
            date = dates.iter().find(|d| [Order::Ymd, Order::Dmy, Order::Mdy].iter().all(|o| iso(d, *o).is_none())).or(dates.first()).unwrap_or(&""),
            expected = if chosen.is_some() { tr("the chosen date order") } else { tr("a date format UrBudget reads") }
        )),
        _ => Err(t!("Every date in the register could be day-first or month-first. Choose the date order, then import again.")),
    }
}
/// "Oct 2026" (or "October 2026", "2026-10") as "2026-10".
fn plan_month(s: &str) -> Option<String> {
    ["%d %b %Y", "%d %B %Y"]
        .iter()
        .find_map(|f| NaiveDate::parse_from_str(&format!("1 {s}"), f).ok())
        .or_else(|| NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d").ok())
        .map(|d| d.format("%Y-%m").to_string())
        .filter(|month| valid_month(month))
}
/// "YNAB Export - Household as of 2026-10-16 0312 PM.zip" or "Household as of … - Register.csv"
/// → "Household".
/// The day the export was made, from its name ("… as of 2026-10-16 0312 PM").
fn export_day(file: &str) -> Option<String> {
    let base = file.rsplit(['/', '\\']).next()?;
    let day = base.split_once(" as of ")?.1.get(..10)?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok().map(|d| d.to_string())
}
fn plan_name(file: &str) -> Option<String> {
    let base = file.rsplit(['/', '\\']).next()?;
    let base = base.rsplit_once('.').map_or(base, |(b, _)| b);
    let base = base.strip_prefix("YNAB Export - ").unwrap_or(base);
    let base = base.split(" - Register").next()?.split(" - Plan").next()?;
    let base = base.split(" as of ").next()?.trim();
    (!base.is_empty()).then(|| base.chars().take(200).collect())
}

#[derive(Clone, Copy, PartialEq)]
enum Cat<'a> {
    /// No category: a transfer between budget accounts, or a tracking account's own entry.
    Blank,
    /// Income, unassigned until it is budgeted ("Inflow: Ready to Assign").
    Income,
    Envelope(&'a str, &'a str),
}
fn cat<'a>(group: &'a str, name: &'a str) -> Cat<'a> {
    if group == "Inflow" && matches!(name, "Ready to Assign" | "To be Budgeted")
    {
        Cat::Income
    } else if name.is_empty() {
        Cat::Blank
    } else {
        Cat::Envelope(if group.is_empty() { "Uncategorized" } else { group }, name)
    }
}
fn category_of<'a>(r: &'a csv::StringRecord, t: &Table) -> Cat<'a> {
    match (t.column("Category Group"), t.column("Category")) {
        (Some(g), Some(c)) => cat(field(r, Some(g)), field(r, Some(c))),
        _ => {
            let both = field(r, t.column("Category Group/Category"));
            let (g, c) = both.split_once(": ").unwrap_or(("", both));
            cat(g, c)
        }
    }
}

struct Line<'a> {
    row: usize,
    account: &'a str,
    date: String,
    payee: &'a str,
    cat: Cat<'a>,
    memo: &'a str,
    flag: &'a str,
    amount: i64,
    cleared: Cleared,
}
struct PlanRow {
    month: String,
    id: String,
    name: String,
    assigned: i64,
    activity: i64,
    available: i64,
}
/// "Split (2/3) memo" → (2, 3, "memo").
fn split_part(memo: &str) -> Option<(usize, usize, &str)> {
    let (position, rest) = memo.strip_prefix("Split (")?.split_once(')')?;
    let (i, n) = position.split_once('/')?;
    let (i, n): (usize, usize) = (i.parse().ok()?, n.parse().ok()?);
    (1..=n).contains(&i).then_some((i, n, rest.strip_prefix(' ').unwrap_or(rest)))
}
/// The other account of a transfer row ("Transfer : Savings"), if it is one in this export.
fn target<'a>(l: &Line<'a>, ids: &HashMap<&str, String>) -> Option<&'a str> {
    l.payee
        .strip_prefix(TRANSFER)
        .map(str::trim)
        .filter(|a| *a != l.account && ids.contains_key(a))
}
fn flag(f: &str) -> Value {
    if f.is_empty() {
        Value::Null
    } else {
        json!(f.to_lowercase())
    }
}
fn object(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}
fn transaction_meta(f: &str, starting: bool) -> Extra {
    let mut extra = object(json!({"flag": flag(f)}));
    if starting { extra.insert("startingBalance".into(), json!(true)); }
    extra.into()
}
fn hashed(prefix: &str, parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0x1f]);
    }
    format!("{prefix}-{}", &format!("{:x}", h.finalize())[..20])
}
/// A transfer row's account, other account, date and amount.
type Key<'a> = (&'a str, &'a str, &'a str, i64);
/// The first unused transfer row with this key.
fn take<'a>(open: &mut HashMap<Key<'a>, VecDeque<usize>>, used: &mut [bool], key: Key<'a>) -> Option<usize> {
    let rows = open.get_mut(&key)?;
    while let Some(j) = rows.pop_front() {
        if !used[j] {
            used[j] = true;
            return Some(j);
        }
    }
    None
}

/// A new budget from YNAB's export files. `order` is "ymd", "dmy" or "mdy" when the user
/// chose one; `fallback_currency` applies when the amounts carry no recognizable symbol.
pub fn import_files(files: &[ExportFile], order: Option<&str>, fallback_currency: &str) -> Result<Budget, String> {
    let tables = tables(files)?;
    let register = tables
        .iter()
        .find(|t| t.has(&["Account", "Date", "Payee", "Outflow", "Inflow"]))
        .ok_or_else(|| t!("Choose YNAB's export: the .zip it downloads, or its Register and Plan files together."))?;
    let plan = tables
        .iter()
        .find(|t| t.has(&["Month", "Category", "Available"]) && (t.has(&["Assigned"]) || t.has(&["Budgeted"])))
        .ok_or_else(|| t!("The export's Plan file is missing. Choose the .zip, or the Register and Plan files together."))?;
    let rc = |name| register.column(name);
    let pc = |name| plan.column(name);
    let (assigned_c, activity_c, available_c) = (pc("Assigned").or(pc("Budgeted")), pc("Activity"), pc("Available"));

    let amounts = register
        .rows
        .iter()
        .flat_map(|r| [field(r, rc("Outflow")), field(r, rc("Inflow"))]);
    let mark = decimal_mark(amounts.clone().chain(
        plan.rows
            .iter()
            .flat_map(|r| [field(r, assigned_c), field(r, activity_c), field(r, available_c)]),
    ))?;
    let currency = match amounts.clone().find_map(currency_in) {
        // A plain "$" doesn't say which dollar, nor "kr" which krona: keep the open budget's
        // currency when it's one of them.
        Some(codes) if codes.contains(&fallback_currency) => fallback_currency,
        Some(codes) => codes[0],
        None => fallback_currency,
    }
    .to_string();
    let dates: Vec<&str> = register
        .rows
        .iter()
        .map(|r| field(r, rc("Date")))
        .filter(|d| !d.is_empty())
        .collect();
    let order = date_order(&dates, order)?;
    // YNAB holds no transaction dated after today, so the Register's later rows are the next date
    // of each scheduled transaction. The export doesn't say how those repeat; they're left out.
    let exported = files
        .iter()
        .find(|f| f.bytes.starts_with(b"PK\x03\x04"))
        .and_then(|f| export_day(&f.name))
        .or_else(|| export_day(&register.name))
        .unwrap_or_else(|| chrono::Local::now().date_naive().to_string());
    let mut scheduled = 0;

    let mut lines = vec![];
    for (i, r) in register.rows.iter().enumerate() {
        let account = field(r, rc("Account"));
        if account.is_empty() {
            continue;
        }
        let row = i + 2;
        let at = |e: String| t!("Register row {row}: {problem}", row, problem = e);
        let cleared = match field(r, rc("Cleared")).to_ascii_lowercase().as_str() {
            "" | "uncleared" => Cleared::Uncleared,
            "cleared" => Cleared::Cleared,
            "reconciled" => Cleared::Reconciled,
            other => return Err(at(t!("unknown cleared status “{status}”", status = other))),
        };
        let date = iso(field(r, rc("Date")), order).ok_or_else(|| at(t!("unreadable date")))?;
        if date > exported {
            scheduled += 1;
            continue;
        }
        let amount = money(field(r, rc("Inflow")), mark).map_err(at)?
            .checked_sub(money(field(r, rc("Outflow")), mark).map_err(at)?)
            .filter(|n| valid_money(*n)).ok_or_else(|| at(t!("transaction exceeds the supported money range")))?;
        lines.push(Line {
            row,
            account,
            date,
            payee: field(r, rc("Payee")),
            cat: category_of(r, register),
            memo: field(r, rc("Memo")),
            flag: field(r, rc("Flag")),
            amount,
            cleared,
        });
    }

    // Categories in the Plan's order, then any only the register mentions.
    let mut categories: Vec<Category> = vec![];
    let mut category_ids: HashMap<(String, String), String> = HashMap::new();
    let mut add = |c: Cat| match c {
        Cat::Envelope(group, name) => Some(
            category_ids
                .entry((group.into(), name.into()))
                .or_insert_with(|| {
                    let id = hashed("c", &[group, name]);
                    categories.push(Category {
                        id: id.clone(),
                        name: name.into(),
                        group: group.into(),
                        target: 0,
                        extra: if group == HIDDEN { object(json!({"hidden": true})) } else { Map::new() },
                    });
                    id
                })
                .clone(),
        ),
        _ => None,
    };
    let mut plan_rows = vec![];
    let mut seen = HashSet::new();
    for (i, r) in plan.rows.iter().enumerate() {
        let c = category_of(r, plan);
        let Some(id) = add(c) else { continue };
        let at = |e: String| t!("Plan row {row}: {problem}", row = i + 2, problem = e);
        let month = plan_month(field(r, pc("Month"))).ok_or_else(|| at(t!("unreadable month")))?;
        if !seen.insert((month.clone(), id.clone())) {
            let Cat::Envelope(group, name) = c else { unreachable!() };
            return Err(t!("The plan lists “{group}: {name}” twice in {month}. Rename one in YNAB and export again.", group, name, month));
        }
        let Cat::Envelope(_, name) = c else { unreachable!() };
        plan_rows.push(PlanRow {
            month,
            name: name.into(),
            id,
            assigned: money(field(r, assigned_c), mark).map_err(at)?,
            activity: money(field(r, activity_c), mark).map_err(at)?,
            available: money(field(r, available_c), mark).map_err(at)?,
        });
    }
    let line_categories: Vec<Option<String>> = lines.iter().map(|l| add(l.cat)).collect();

    // Accounts: a card has a payment category; a tracking account never has a categorized row
    // (budget accounts always do, if only their "Inflow: Ready to Assign" starting balance).
    let mut names: Vec<&str> = vec![];
    for l in &lines {
        if !names.contains(&l.account) {
            names.push(l.account);
        }
    }
    let budgeted: HashSet<&str> = lines.iter().filter(|l| l.cat != Cat::Blank).map(|l| l.account).collect();
    let accounts: Vec<Account> = names
        .iter()
        .map(|&name| {
            let kind = if categories.iter().any(|c| c.group == CARD_PAYMENTS && c.name == name) {
                AccountKind::Credit
            } else if budgeted.contains(name) {
                AccountKind::Cash
            } else {
                AccountKind::Tracking
            };
            Account {
                id: hashed("a", &[name]),
                name: name.into(),
                kind,
                opening_balance: 0,
                archived: false,
                extra: object(json!({"onBudget": kind != AccountKind::Tracking})),
            }
        })
        .collect();
    for c in categories.iter_mut().filter(|c| c.group == CARD_PAYMENTS) {
        let a = accounts.iter().find(|a| a.kind == AccountKind::Credit && a.name == c.name)
            .ok_or_else(|| t!("Payment category “{name}” ({id}) has no matching account in the Register export. Export the complete Register and Plan together, or use YNAB's token import.", name = c.name, id = c.id))?;
        c.extra.insert("paymentAccountId".into(), json!(a.id));
    }
    let ids: HashMap<&str, String> = accounts.iter().map(|a| (a.name.as_str(), a.id.clone())).collect();
    let on_budget: HashMap<&str, bool> = accounts.iter().map(|a| (a.name.as_str(), a.on_budget())).collect();
    for l in &lines {
        if let Some(to) = l.payee.strip_prefix(TRANSFER).map(str::trim) {
            if to == l.account || !ids.contains_key(to) {
                return Err(t!("Register row {row}: transfer account “{to}” is missing or is the same as “{account}”. Export the complete Register and Plan together.", row = l.row, to, account = l.account));
            }
        }
    }

    // Rows of one split are consecutive and numbered "Split (1/3)" to "Split (3/3)".
    let mut splits_found = vec![];
    let mut singles = vec![];
    let mut i = 0;
    while i < lines.len() {
        let Some((_, n, _)) = split_part(lines[i].memo) else {
            singles.push(i);
            i += 1;
            continue;
        };
        let first = &lines[i];
        let mut group: Vec<usize> = (i..lines.len())
            .take(n)
            .take_while(|&j| {
                let l = &lines[j];
                l.account == first.account && l.date == first.date && split_part(l.memo).is_some_and(|(_, m, _)| m == n)
            })
            .collect();
        group.sort_by_key(|&j| split_part(lines[j].memo).map(|(k, _, _)| k));
        if group.len() != n || group.iter().enumerate().any(|(k, &j)| split_part(lines[j].memo).map(|p| p.0) != Some(k + 1)) {
            return Err(t!(
                "Register row {row}: the split in {account} on {date} is incomplete.",
                row = first.row, account = first.account, date = first.date
            ));
        }
        i += n;
        splits_found.push(group);
    }

    // Both sides of every transfer are in the register; each pair becomes one transaction.
    let mut open: HashMap<Key<'_>, VecDeque<usize>> = HashMap::new();
    for &j in &singles {
        if let Some(to) = target(&lines[j], &ids) {
            let l = &lines[j];
            open.entry((l.account, to, l.date.as_str(), l.amount)).or_default().push_back(j);
        }
    }
    let unmatched = |l: &Line, to: &str| {
        t!(
            "Register row {row}: the transfer from {account} to {to} on {date} has no matching row in {to}. Nothing was imported.",
            row = l.row, account = l.account, to, date = l.date
        )
    };
    let mut used = vec![false; lines.len()];
    let mut transactions = vec![];
    let (mut pairs, mut split_rows) = (0, 0);
    let id = |n: usize| format!("ynab-file-{n:06}");
    for group in &splits_found {
        let first = &lines[group[0]];
        let payee = group
            .iter()
            .map(|&j| &lines[j])
            .find(|l| target(l, &ids).is_none())
            .unwrap_or(first)
            .payee;
        let mut splits = vec![];
        for &j in group {
            let l = &lines[j];
            used[j] = true;
            split_rows += 1;
            let mut extra = object(json!({"memo": split_part(l.memo).map_or("", |p| p.2)}));
            if l.cat == Cat::Income {
                extra.insert("unassigned".into(), json!(true));
            }
            if l.payee != payee {
                extra.insert("payee".into(), json!(l.payee));
            }
            if let Some(to) = target(l, &ids) {
                let peer = take(&mut open, &mut used, (to, l.account, l.date.as_str(), -l.amount))
                    .ok_or_else(|| unmatched(l, to))?;
                let peer = &lines[peer];
                extra.insert("transferAccountId".into(), json!(ids[to]));
                extra.insert("transferStatus".into(), json!(peer.cleared));
                extra.insert("transferSource".into(), json!({"memo": peer.memo, "flag_color": flag(peer.flag)}));
                pairs += 1;
            }
            splits.push(Split {
                category_id: line_categories[j].clone(),
                amount: l.amount,
                extra: extra.into(),
            });
        }
        let amount = splits.iter().try_fold(0i64, |total, s| total.checked_add(s.amount))
            .filter(|n| valid_money(*n)).ok_or_else(|| t!("Register row {row}: split total exceeds the supported money range.", row = first.row))?;
        transactions.push(Transaction {
            id: id(transactions.len() + 1),
            date: first.date.clone(),
            account_id: ids[first.account].clone(),
            payee: payee.into(),
            memo: String::new(),
            amount,
            status: first.cleared,
            splits,
            transfer_account_id: None,
            transfer_status: None,
            extra: transaction_meta(first.flag, group.iter()
                .all(|&j| lines[j].payee == "Starting Balance" && lines[j].cat == Cat::Income)),
        });
    }
    for &j in &singles {
        if used[j] {
            continue;
        }
        used[j] = true;
        let l = &lines[j];
        let Some(to) = target(l, &ids) else {
            transactions.push(Transaction {
                id: id(transactions.len() + 1),
                date: l.date.clone(),
                account_id: ids[l.account].clone(),
                payee: l.payee.into(),
                memo: l.memo.into(),
                amount: l.amount,
                status: l.cleared,
                splits: vec![Split {
                    category_id: line_categories[j].clone(),
                    amount: l.amount,
                    extra: if l.cat == Cat::Income { object(json!({"unassigned": true})).into() } else { Default::default() },
                }],
                transfer_account_id: None,
                transfer_status: None,
                extra: transaction_meta(l.flag, l.payee == "Starting Balance" && l.cat == Cat::Income),
            });
            continue;
        };
        let k = take(&mut open, &mut used, (to, l.account, l.date.as_str(), -l.amount)).ok_or_else(|| unmatched(l, to))?;
        pairs += 1;
        // Like the API import: keep the budget side of a budget/tracking transfer, otherwise
        // the side the money leaves.
        let (keep, other) = match (on_budget[l.account], on_budget[lines[k].account]) {
            (true, false) => (j, k),
            (false, true) => (k, j),
            _ if l.amount > 0 => (k, j),
            _ => (j, k),
        };
        let (kl, ol) = (&lines[keep], &lines[other]);
        let splits = if on_budget[kl.account] != on_budget[ol.account] {
            vec![Split {
                category_id: line_categories[keep].clone(),
                amount: kl.amount,
                extra: if kl.cat == Cat::Income { object(json!({"unassigned": true})).into() } else { Default::default() },
            }]
        } else {
            vec![]
        };
        transactions.push(Transaction {
            id: id(transactions.len() + 1),
            date: kl.date.clone(),
            account_id: ids[kl.account].clone(),
            payee: kl.payee.into(),
            memo: kl.memo.into(),
            amount: kl.amount,
            status: kl.cleared,
            splits,
            transfer_account_id: Some(ids[ol.account].clone()),
            transfer_status: Some(ol.cleared),
            extra: object(json!({"flag": flag(kl.flag), "transferSource": {"memo": ol.memo, "flag_color": flag(ol.flag)}})).into(),
        });
    }
    transactions.sort_by(|a, b| a.date.cmp(&b.date));

    let mut assignments: Vec<Assignment> = plan_rows
        .iter()
        .filter(|p| p.assigned != 0)
        .map(|p| Assignment { extra: Default::default(),
            month: p.month.clone(),
            category_id: p.id.clone(),
            amount: p.assigned,
        })
        .collect();
    assignments.sort_by(|a, c| (&a.month, &a.category_id).cmp(&(&c.month, &c.category_id)));
    let name = files
        .iter()
        .find(|f| f.bytes.starts_with(b"PK\x03\x04"))
        .and_then(|f| plan_name(&f.name))
        .or_else(|| plan_name(&register.name))
        .unwrap_or_else(|| t!("Imported from YNAB"));
    let mut b = Budget {
        schema_version: SCHEMA_VERSION,
        name,
        currency,
        accounts,
        categories,
        transactions,
        assignments,
        extra: Map::new(),
    };
    validate(&b)?;
    let audit = audit(&b, &plan_rows)?;
    b.extra = object(json!({
        "importedFrom": "ynabFiles",
        "importedAt": chrono::Utc::now().to_rfc3339(),
        "importAudit": audit,
        "importCounts": {"sourceTransactions": lines.len(), "transferPairs": pairs, "localTransactions": b.transactions.len(), "splits": split_rows, "scheduledRows": scheduled},
    }));
    Ok(b)
}
/// Recalculates every month the Plan covers and compares each category's assigned, activity and
/// available amounts with YNAB's.
fn audit(b: &Budget, plan: &[PlanRow]) -> Result<Value, String> {
    let mut checks = 0;
    let mut differences = vec![];
    if let Some(last) = plan.iter().map(|p| p.month.as_str()).max() {
        let history = crate::engine::calculate_history(b, last)?;
        for p in plan {
            let Some(got) = history
                .get(&p.month)
                .and_then(|s| s.categories.iter().find(|c| c.id == p.id))
            else {
                continue;
            };
            checks += 1;
            for (field, expected, actual) in [
                ("budgeted", p.assigned, got.assigned),
                ("activity", p.activity, got.activity),
                ("balance", p.available, got.available),
            ] {
                if expected != actual {
                    differences.push(json!({"month": p.month, "category": p.name, "field": field, "expected": expected, "actual": actual}));
                }
            }
        }
    }
    Ok(json!({
        "source": "ynabFiles",
        "accountChecks": 0,
        "categoryMonthChecks": checks,
        "monthChecks": 0,
        "accountDifferences": [],
        "categoryDifferences": differences,
        "readyToAssignDifferences": [],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn file(name: &str, text: &str) -> ExportFile {
        ExportFile {
            name: name.into(),
            bytes: text.as_bytes().to_vec(),
        }
    }

    fn sized_archive(sizes: &[usize]) -> ExportFile {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let chunk = [b'x'; 8192];
        for (i, size) in sizes.iter().copied().enumerate() {
            zip.start_file(format!("Part{i}.csv"), options).unwrap();
            let mut remaining = size;
            while remaining > 0 {
                let count = remaining.min(chunk.len());
                zip.write_all(&chunk[..count]).unwrap();
                remaining -= count;
            }
        }
        ExportFile { name: "Export.zip".into(), bytes: zip.finish().unwrap().into_inner() }
    }

    #[test]
    fn bounds_native_file_input_before_parsing() {
        let files: Vec<_> = (0..MAX_FILES + 1).map(|_| file("Register.csv", "")).collect();
        assert!(tables(&files).err().unwrap().contains("at most 8"));
        let oversized = ExportFile { name: "Register.csv".into(), bytes: vec![b'x'; MAX_FILE + 1] };
        assert!(tables(&[oversized]).err().unwrap().contains("larger than 8 MB"));
        let files: Vec<_> = (0..3).map(|_| ExportFile { name: "Register.csv".into(), bytes: vec![b'x'; MAX_FILE] }).collect();
        assert!(tables(&files).err().unwrap().contains("more than 16 MB"));
    }

    #[test]
    fn bounds_archive_entry_count_across_the_selection() {
        let half = MAX_ENTRIES / 2 + 1;
        let a = sized_archive(&vec![1; half]);
        let b = sized_archive(&vec![1; half]);
        assert!(tables(&[a, b]).err().unwrap().contains("more than 64 archive entries"));
    }

    #[test]
    fn rejects_an_oversized_expanded_csv_before_reading_it() {
        let archive = sized_archive(&[MAX_CSV as usize + 1]);
        assert!(archive.bytes.len() < MAX_FILE);
        assert!(tables(&[archive]).err().unwrap().contains("more than 32 MB"));
    }

    #[test]
    fn bounds_total_expansion_across_archives_before_parsing_rows() {
        let a = sized_archive(&[MAX_CSV as usize]);
        let b = sized_archive(&[MAX_CSV as usize, 1]);
        assert!(a.bytes.len() + b.bytes.len() < MAX_INPUT);
        assert!(tables(&[a, b]).err().unwrap().contains("more than 64 MB of CSV data"));
    }

    #[test]
    fn bounds_tiny_rows_across_all_files_and_overwide_records() {
        let text = format!("Column\n{}", "x\n".repeat(MAX_ROWS / 2 + 1));
        assert!(tables(&[file("Register.csv", &text), file("Plan.csv", &text)]).err().unwrap().contains("more than 200,000 CSV rows"));
        let wide = vec!["Column"; MAX_COLUMNS + 1].join(",");
        assert!(tables(&[file("Register.csv", &wide)]).err().unwrap().contains("more than 128 columns"));
        let wide_row = format!("Column\n{wide}");
        assert!(tables(&[file("Register.csv", &wide_row)]).err().unwrap().contains("more than 128 columns"));
    }
    fn split(category: Option<&str>, amount: i64, extra: Value) -> Split {
        Split {
            category_id: category.map(Into::into),
            amount,
            extra: object(extra).into(),
        }
    }
    fn tx(id: &str, date: &str, account: &str, payee: &str, amount: i64, splits: Vec<Split>) -> Transaction {
        Transaction {
            id: id.into(),
            date: date.into(),
            account_id: account.into(),
            payee: payee.into(),
            memo: String::new(),
            amount,
            status: Cleared::Cleared,
            splits,
            transfer_account_id: None,
            transfer_status: None,
            extra: Default::default(),
        }
    }
    fn transfer(id: &str, date: &str, from: &str, to: &str, amount: i64, splits: Vec<Split>) -> Transaction {
        Transaction {
            transfer_account_id: Some(to.into()),
            transfer_status: Some(Cleared::Uncleared),
            ..tx(id, date, from, "", amount, splits)
        }
    }
    /// A budget with every shape the export writes: income, card spending and payment,
    /// transfers between budget accounts and to a tracking account, and a split with a transfer leg.
    fn household() -> Budget {
        let account = |id: &str, name: &str, kind| Account {
            id: id.into(),
            name: name.into(),
            kind,
            opening_balance: 0,
            archived: false,
            extra: Map::new(),
        };
        let category = |id: &str, group: &str, name: &str| Category {
            id: id.into(),
            name: name.into(),
            group: group.into(),
            target: 0,
            extra: Map::new(),
        };
        let mut visa = category("visa", CARD_PAYMENTS, "Visa");
        visa.extra.insert("paymentAccountId".into(), json!("card"));
        let mut groceries = tx("food", "2026-08-03", "cash", "Market", -12345, vec![split(Some("food"), -12345, json!({}))]);
        groceries.memo = "Weekly shop, \"big\" one".into();
        groceries.extra.insert("flag".into(), json!("red"));
        groceries.status = Cleared::Reconciled;
        Budget {
            accounts: vec![
                account("cash", "Checking", AccountKind::Cash),
                account("card", "Visa", AccountKind::Credit),
                account("savings", "Savings", AccountKind::Cash),
                account("brokerage", "Brokerage", AccountKind::Tracking),
            ],
            categories: vec![
                category("food", "Living", "Groceries"),
                category("rent", "Living", "Rent"),
                visa,
                category("invest", "Goals", "Investing"),
            ],
            transactions: vec![
                tx("pay", "2026-08-01", "cash", "Employer", 500000, vec![split(None, 500000, json!({}))]),
                tx("start", "2026-08-01", "savings", "Starting Balance", 100000, vec![split(None, 100000, json!({}))]),
                tx("start2", "2026-08-01", "brokerage", "Starting Balance", 700000, vec![split(None, 700000, json!({}))]),
                groceries,
                tx("card-buy", "2026-08-05", "card", "Cafe", -5000, vec![split(Some("food"), -5000, json!({}))]),
                transfer("card-pay", "2026-08-20", "cash", "card", -5000, vec![]),
                transfer("invest", "2026-08-21", "cash", "brokerage", -20000, vec![split(Some("invest"), -20000, json!({}))]),
                transfer("back", "2026-09-02", "savings", "cash", -1000, vec![]),
                tx(
                    "split",
                    "2026-09-01",
                    "cash",
                    "Landlord",
                    -28000,
                    vec![
                        split(Some("rent"), -25000, json!({"memo": "September"})),
                        split(Some("food"), -3000, json!({"memo": "Fruit", "payee": "Market"})),
                    ],
                ),
                // This app's exporter writes each leg of a split with a transfer as its own row.
                tx(
                    "stash",
                    "2026-09-03",
                    "cash",
                    "Bank",
                    -2000,
                    vec![
                        split(Some("food"), -500, json!({"memo": "Fee"})),
                        split(None, -1500, json!({"memo": "Put away", "transferAccountId": "savings", "transferStatus": "cleared"})),
                    ],
                ),
            ],
            assignments: vec![
                Assignment { extra: Default::default(), month: "2026-08".into(), category_id: "food".into(), amount: 20000 },
                Assignment { extra: Default::default(), month: "2026-08".into(), category_id: "rent".into(), amount: 25000 },
                Assignment { extra: Default::default(), month: "2026-08".into(), category_id: "invest".into(), amount: 20000 },
                Assignment { extra: Default::default(), month: "2026-09".into(), category_id: "food".into(), amount: 15000 },
            ],
            ..Budget::default()
        }
    }

    #[test]
    fn rebuilds_a_budget_from_its_own_ynab_export() {
        let original = household();
        validate(&original).unwrap();
        let register = crate::ynab::export_register(&original).unwrap();
        let plan = crate::ynab::export_plan(&original).unwrap();
        let b = import_files(
            &[
                file("Household as of 2026-10-16 0312 PM - Register.csv", &register),
                file("Household as of 2026-10-16 0312 PM - Plan.csv", &plan),
            ],
            None,
            "SAR",
        )
        .unwrap();
        assert_eq!(b.name, "Household");
        assert_eq!(b.currency, "SAR");
        let kind = |name: &str| b.accounts.iter().find(|a| a.name == name).unwrap().kind;
        assert_eq!(
            [kind("Checking"), kind("Visa"), kind("Savings"), kind("Brokerage")],
            [AccountKind::Cash, AccountKind::Credit, AccountKind::Cash, AccountKind::Tracking]
        );
        let audit = &b.extra["importAudit"];
        assert!(audit["categoryMonthChecks"].as_u64().unwrap() > 0);
        assert_eq!(audit["categoryDifferences"], json!([]), "{audit}");

        // Same balances, unassigned money and category amounts in every month.
        let months = ["2026-08", "2026-09"];
        for month in months {
            let (was, now) = (calculate(&original, month).unwrap(), calculate(&b, month).unwrap());
            assert_eq!(was.ready_to_assign, now.ready_to_assign, "{month}");
            for a in &original.accounts {
                let id = &b.accounts.iter().find(|x| x.name == a.name).unwrap().id;
                let (x, y) = (
                    was.accounts.iter().find(|s| s.id == a.id).unwrap(),
                    now.accounts.iter().find(|s| &s.id == id).unwrap(),
                );
                assert_eq!((x.balance, x.cleared), (y.balance, y.cleared), "{} in {month}", a.name);
            }
        }

        let s = b.transactions.iter().find(|t| t.splits.len() == 2).unwrap();
        assert_eq!((s.payee.as_str(), s.amount), ("Landlord", -28000));
        assert_eq!(s.splits[0].extra["memo"], "September");
        assert_eq!(s.splits[1].extra["payee"], "Market");
        let shop = b.transactions.iter().find(|t| t.payee == "Market").unwrap();
        assert_eq!(shop.memo, "Weekly shop, \"big\" one");
        assert_eq!(shop.extra["flag"], "red");
        assert_eq!(shop.status, Cleared::Reconciled);
        let pay = b.transactions.iter().find(|t| t.amount == -5000 && t.transfer_account_id.is_some()).unwrap();
        assert_eq!(b.accounts.iter().find(|a| a.id == pay.account_id).unwrap().name, "Checking");
        let visa = b.categories.iter().find(|c| c.group == CARD_PAYMENTS).unwrap();
        let card = &b.accounts.iter().find(|a| a.name == "Visa").unwrap().id;
        assert_eq!(visa.payment_account(), Some(card.as_str()));
    }

    /// The register YNAB downloads: US dates and dollars, both sides of each transfer, and a
    /// split whose second row is a transfer to savings.
    const REGISTER: &str = concat!(
        "\u{feff}",
        r#""Account","Flag","Date","Payee","Category Group/Category","Category Group","Category","Memo","Outflow","Inflow","Cleared"
"Checking","","10/01/2026","Starting Balance","Inflow: Ready to Assign","Inflow","Ready to Assign","","$0.00","$1,500.00","Reconciled"
"Checking","Blue","10/15/2026","Grocer","Living: Food","Living","Food","","$1,234.56","$0.00","Cleared"
"Checking","","10/16/2026","Transfer : Savings","","","","","$100.00","$0.00","Uncleared"
"Checking","","10/20/2026","Landlord","Living: Rent","Living","Rent","Split (1/2) October","$90.00","$0.00","Cleared"
"Checking","","10/20/2026","Transfer : Savings","","","","Split (2/2) Put away","$5.00","$0.00","Cleared"
"Savings","","10/01/2026","Starting Balance","Inflow: Ready to Assign","Inflow","Ready to Assign","","$0.00","$0.00","Reconciled"
"Savings","","10/16/2026","Transfer : Checking","","","","","$0.00","$100.00","Cleared"
"Savings","Green","10/20/2026","Transfer : Checking","","","","From rent day","$0.00","$5.00","Uncleared"
"#
    );
    const PLAN: &str = r#""Month","Category Group/Category","Category Group","Category","Assigned","Activity","Available"
"Oct 2026","Living: Food","Living","Food","$1,300.00","-$1,234.56","$65.44"
"Oct 2026","Living: Rent","Living","Rent","$90.00","-$90.00","$0.00"
"#;

    #[test]
    fn malformed_csv_dates_and_extended_plan_years_are_rejected_without_panics() {
        for date in ["-0001-01-01", "+262142-12-31", "123456é89", "2026-02-30"] {
            let register = REGISTER.replace("10/01/2026", date);
            let result = std::panic::catch_unwind(|| import_files(&[file("Register.csv", &register), file("Plan.csv", PLAN)], None, "USD"));
            assert!(result.is_ok(), "{date}");
            assert!(result.unwrap().is_err(), "{date}");
        }
        for month in ["Oct +262142", "123456é89", "2026-99"] {
            let plan = PLAN.replace("Oct 2026", month);
            assert!(import_files(&[file("Register.csv", REGISTER), file("Plan.csv", &plan)], None, "USD").unwrap_err().contains("month"));
        }
        assert_eq!(iso("0000-01-01", Order::Ymd).as_deref(), Some("0000-01-01"));
        assert_eq!(iso("9999-12-31", Order::Ymd).as_deref(), Some("9999-12-31"));
        assert_eq!(plan_month("Jan 0000").as_deref(), Some("0000-01"));
        assert_eq!(plan_month("9999-12").as_deref(), Some("9999-12"));
    }
    #[test]
    fn csv_transfer_to_an_absent_account_is_not_reclassified_as_an_ordinary_transaction() {
        let register = REGISTER.replace("Transfer : Savings", "Transfer : Missing Savings");
        let error = import_files(&[file("Register.csv", &register), file("Plan.csv", PLAN)], None, "USD").unwrap_err();
        assert!(error.contains("Missing Savings") && error.contains("Register row"), "{error}");
    }
    #[test]
    fn csv_money_and_combined_transaction_amounts_are_checked_before_arithmetic() {
        let header = "Account,Date,Payee,Category Group/Category,Memo,Outflow,Inflow,Cleared\n";
        for body in [
            // Previously subtraction of these individually parseable i64 amounts panicked.
            "Checking,10/15/2026,Shop,Living: Food,,92233720368547758.07,-92233720368547758.07,Cleared\n",
            "Checking,10/15/2026,Shop,Living: Food,,10000000000.00,-10000000000.00,Cleared\n",
            "Checking,10/15/2026,Shop,Living: Food,Split (1/2),10000000000.00,0.00,Cleared\nChecking,10/15/2026,Shop,Living: Food,Split (2/2),10000000000.00,0.00,Cleared\n",
        ] {
            let register = format!("{header}{body}");
            let result = std::panic::catch_unwind(|| import_files(&[file("Register.csv", &register), file("Plan.csv", PLAN)], None, "USD"));
            assert!(result.is_ok(), "money validation must not panic");
            assert!(result.unwrap().unwrap_err().contains("supported money range"));
        }
    }

    #[test]
    fn csv_payment_category_without_a_register_account_is_rejected() {
        let plan = format!("{PLAN}\"Oct 2026\",\"Credit Card Payments: Visa\",\"Credit Card Payments\",\"Visa\",\"$0.00\",\"$0.00\",\"$0.00\"\n");
        let error = import_files(&[file("Register.csv", REGISTER), file("Plan.csv", &plan)], None, "USD").err().unwrap();
        assert!(error.contains("Visa") && error.contains("no matching account in the Register export"), "{error}");
    }

    #[test]
    fn csv_starting_balances_and_negative_rta_preserve_their_accounting_role() {
        let register = format!("{REGISTER}\"Checking\",\"\",\"10/25/2026\",\"Correction\",\"Inflow: Ready to Assign\",\"Inflow\",\"Ready to Assign\",\"\",\"$20.00\",\"$0.00\",\"Cleared\"\n");
        let files = [file("Home as of 2026-10-31 - Register.csv", &register), file("Plan.csv", PLAN)];
        let mut b = import_files(&files, None, "USD").unwrap();
        let report = |b: &Budget| crate::reports::calculate(b, &crate::reports::Filter {
            from: "2026-10".into(), to: "2026-10".into(), account_ids: vec![], category_ids: vec![],
        }).unwrap();
        for t in b.transactions.iter().filter(|t| t.payee == "Starting Balance") {
            assert_eq!(t.extra["startingBalance"], true);
            assert_eq!(t.splits[0].extra["unassigned"], true);
        }
        assert_eq!(b.transactions.iter().find(|t| t.payee == "Correction").unwrap().splits[0].extra["unassigned"], true);
        assert_eq!((report(&b).income, report(&b).spending), (-2000, 132456));
        b.transactions.iter_mut().find(|t| t.payee == "Starting Balance" && t.amount > 0).unwrap().payee = "Opening renamed".into();
        assert_eq!(report(&b).income, 148000);
    }

    #[test]
    fn csv_inflow_group_and_rta_named_envelopes_are_not_system_income() {
        let register = REGISTER.replace("Living: Food\",\"Living\",\"Food", "Inflow: Salary\",\"Inflow\",\"Salary")
            .replace("Living: Rent\",\"Living\",\"Rent", "Living: Ready to Assign\",\"Living\",\"Ready to Assign");
        let plan = PLAN.replace("Living: Food\",\"Living\",\"Food", "Inflow: Salary\",\"Inflow\",\"Salary")
            .replace("Living: Rent\",\"Living\",\"Rent", "Living: Ready to Assign\",\"Living\",\"Ready to Assign");
        let b = import_files(&[file("Register.csv", &register), file("Plan.csv", &plan)], None, "USD").unwrap();
        assert!(b.categories.iter().any(|c| c.group == "Inflow" && c.name == "Salary"));
        assert!(b.categories.iter().any(|c| c.group == "Living" && c.name == "Ready to Assign"));
        assert!(b.transactions.iter().filter(|t| t.payee == "Grocer" || t.payee == "Landlord").all(|t| t.splits[0].category_id.is_some()));
        assert_eq!(b.extra["importAudit"]["categoryDifferences"], json!([]));
    }

    #[test]
    fn register_export_roundtrip_does_not_turn_missing_outflow_category_into_rta() {
        let mut original = household();
        original.transactions.push(tx("unknown", "2026-08-23", "cash", "Unknown outflow", -2500, vec![split(None, -2500, json!({"unassigned": false}))]));
        original.transactions.push(tx("rta", "2026-08-24", "cash", "RTA adjustment", -1000, vec![split(None, -1000, json!({"unassigned": true}))]));
        let b = import_files(&[
            file("Register.csv", &crate::ynab::export_register(&original).unwrap()),
            file("Plan.csv", &crate::ynab::export_plan(&original).unwrap()),
        ], None, "USD").unwrap();
        let unknown = b.transactions.iter().find(|t| t.payee == "Unknown outflow").unwrap();
        let rta = b.transactions.iter().find(|t| t.payee == "RTA adjustment").unwrap();
        assert!(unknown.splits[0].category_id.is_none());
        assert!(!crate::ledger::explicitly_unassigned(unknown, &unknown.splits[0]));
        assert!(crate::ledger::explicitly_unassigned(rta, &rta.splits[0]));
    }

    #[test]
    fn reads_ynab_amounts_and_dates_from_the_downloaded_zip() {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, text) in [("Home as of 2026-10-20 - Register.csv", REGISTER), ("Home as of 2026-10-20 - Plan.csv", PLAN)] {
            zip.start_file(name, stored).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let b = import_files(
            &[ExportFile { name: "YNAB Export - Home as of 2026-10-20 0312 PM.zip".into(), bytes }],
            None,
            "SAR",
        )
        .unwrap();
        assert_eq!((b.name.as_str(), b.currency.as_str()), ("Home", "USD"));
        // "$" is the user's own dollar when theirs is one.
        let files = [file("Register.csv", REGISTER), file("Plan.csv", PLAN)];
        assert_eq!(import_files(&files, None, "CAD").unwrap().currency, "CAD");
        assert_eq!(import_files(&files, None, "EUR").unwrap().currency, "USD");
        let grocer = b.transactions.iter().find(|t| t.payee == "Grocer").unwrap();
        assert_eq!((grocer.date.as_str(), grocer.amount), ("2026-10-15", -123456));
        assert_eq!(grocer.extra["flag"], "blue");
        assert_eq!(b.categories.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Food", "Rent"]);
        assert_eq!(b.assignments.len(), 2);
        assert_eq!(b.extra["importAudit"]["categoryDifferences"], json!([]));
        assert_eq!(b.extra["importCounts"]["transferPairs"], 2);
        let move_out = b.transactions.iter().find(|t| t.transfer_account_id.is_some()).unwrap();
        assert_eq!((move_out.amount, move_out.status, move_out.transfer_status), (-10000, Cleared::Uncleared, Some(Cleared::Cleared)));
        // A split with a transfer leg: the leg keeps the other side's status, memo and flag.
        let rent = b.transactions.iter().find(|t| t.payee == "Landlord").unwrap();
        assert_eq!((rent.amount, rent.splits.len()), (-9500, 2));
        let leg = &rent.splits[1];
        let savings = &b.accounts.iter().find(|a| a.name == "Savings").unwrap().id;
        assert_eq!(crate::ledger::transfer(leg), Some(savings.as_str()));
        assert_eq!(crate::ledger::status(leg), Cleared::Uncleared);
        assert_eq!(leg.extra["transferSource"], json!({"memo": "From rent day", "flag_color": "green"}));
        assert_eq!(leg.extra["memo"], "Put away");
        assert_eq!(b.transactions.len(), 5);
    }

    #[test]
    fn categories_the_export_lists_as_hidden_stay_hidden() {
        let plan = format!("{PLAN}{}\n", r#""Oct 2026","Hidden Categories: Old Stuff","Hidden Categories","Old Stuff","$0.00","$0.00","$0.00""#);
        let b = import_files(&[file("Register.csv", REGISTER), file("Plan.csv", &plan)], None, "USD").unwrap();
        let hidden: Vec<_> = b.categories.iter().map(|c| (c.name.as_str(), c.extra.get("hidden") == Some(&json!(true)))).collect();
        assert_eq!(hidden, [("Food", false), ("Rent", false), ("Old Stuff", true)]);
        assert_eq!(b.extra["importAudit"]["categoryDifferences"], json!([]));
    }
    #[test]
    fn leaves_out_the_next_dates_of_scheduled_transactions_the_register_lists() {
        // The export of October 20th lists each scheduled transaction once more, at its next date.
        let register = format!("{REGISTER}{}", concat!(
            r#""Checking","","10/28/2026","Gym","Living: Food","Living","Food","","$40.00","$0.00","Uncleared""#, "\n",
            r#""Checking","","11/02/2026","Transfer : Savings","","","","Monthly saving","$500.00","$0.00","Uncleared""#, "\n",
            r#""Savings","","11/02/2026","Transfer : Checking","","","","Monthly saving","$0.00","$500.00","Uncleared""#, "\n",
        ));
        let named = |day: &str| [file(&format!("Home as of {day} 2331 - Register.csv"), &register), file("Plan.csv", PLAN)];
        let b = import_files(&named("2026-10-20"), None, "USD").unwrap();
        assert_eq!(b.transactions.len(), 5);
        assert_eq!(b.extra["importCounts"]["scheduledRows"], 3);
        assert_eq!(b.extra["importAudit"]["categoryDifferences"], json!([]));
        // Exported on November 2nd, the same rows were already entered.
        let later = import_files(&named("2026-11-02"), None, "USD").unwrap();
        assert_eq!((later.transactions.len(), &later.extra["importCounts"]["scheduledRows"]), (7, &json!(0)));
        assert_eq!((export_day("YNAB Export - Home as of 2026-10-16 0312 PM.zip"), export_day("Register.csv")), (Some("2026-10-16".into()), None));
    }
    #[test]
    fn reads_the_tsv_files_ynab_zips_for_a_decimal_comma_plan() {
        // As YNAB writes a Euro plan set to 123.456,78 and 30.12.2026: tabs between the columns,
        // .tsv names, and amounts with no grouping, the symbol right after them.
        let euro = |text: &str| {
            let mut t = text.replace("\",\"", "\"\t\"");
            for (from, to) in [("\"-$1,234.56\"", "-1234,56€"), ("\"$1,234.56\"", "1234,56€"), ("\"$1,500.00\"", "1500,00€"),
                ("\"$1,300.00\"", "1300,00€"), ("\"-$90.00\"", "-90,00€"), ("\"$100.00\"", "100,00€"), ("\"$90.00\"", "90,00€"),
                ("\"$65.44\"", "65,44€"), ("\"$5.00\"", "5,00€"), ("\"$0.00\"", "0,00€"), ("\"10/", "\"x"), ("/2026\"", ".10.2026\"")] {
                t = t.replace(from, to);
            }
            t.replace("\"x", "\"")
        };
        let (register, plan) = (euro(REGISTER), euro(PLAN));
        assert!(register.contains("\"15.10.2026\"") && register.contains("\t1234,56€\t0,00€\t") && !register.contains('$'));
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, text) in [("Home as of 2026-10-20 23-39 - Register.tsv", &register), ("Home as of 2026-10-20 23-39 - Plan.tsv", &plan)] {
            zip.start_file(name, stored).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let b = import_files(&[ExportFile { name: "YNAB Export - Home as of 2026-10-20 23-39.zip".into(), bytes }], None, "SAR").unwrap();
        assert_eq!((b.name.as_str(), b.currency.as_str()), ("Home", "EUR"));
        let grocer = b.transactions.iter().find(|t| t.payee == "Grocer").unwrap();
        assert_eq!((grocer.date.as_str(), grocer.amount), ("2026-10-15", -123456));
        assert_eq!(b.extra["importAudit"]["categoryDifferences"], json!([]));
        assert_eq!(b.transactions.len(), 5);
    }
    #[test]
    fn reads_every_decimal_mark_ynab_offers_and_the_currency_its_symbol_means() {
        assert_eq!(decimal_mark(["1200-00kr", "-225-00kr", "0-00kr"].into_iter()), Ok('-'));
        assert_eq!([money("-225-00kr", '-'), money("1200-00kr", '-'), money("0-00kr", '-')], [Ok(-22500), Ok(120000), Ok(0)]);
        assert_eq!(decimal_mark(["1200/78", "-5/00"].into_iter()), Ok('/'));
        assert_eq!([money("-5/00", '/'), money("1200/78", '/')], [Ok(-500), Ok(120078)]);
        assert_eq!(decimal_mark([" ₹1200.00", "- ₹279.75"].into_iter()), Ok('.'));
        assert_eq!(money("- ₹279.75", '.'), Ok(-27975));
        assert!(decimal_mark(["1200-00kr", "5.00"].into_iter()).unwrap_err().contains("mixes"));
        let currency = |amount: &str, open: &str| {
            let register = REGISTER.replace('$', "").replace("1,234.56", amount);
            import_files(&[file("Register.csv", &register), file("Plan.csv", &PLAN.replace('$', ""))], None, open).map(|b| b.currency)
        };
        assert_eq!(currency("₹1234.56", "SAR"), Ok("INR".into()));
        assert_eq!(currency("1234.56kr", "NOK"), Ok("NOK".into()));
        assert_eq!(currency("1234.56kr", "SAR"), Ok("SEK".into()));
        assert_eq!(currency("R$1234.56", "USD"), Ok("BRL".into()));
        assert_eq!(currency("1234.56", "SAR"), Ok("SAR".into()));
    }
    #[test]
    fn reads_the_tab_separated_export_of_a_decimal_comma_plan() {
        // YNAB writes plans whose amounts use a decimal comma with tabs between the columns.
        let euro = |text: &str| {
            [
                ("-$1,234.56", "-1.234,56 €"),
                ("$1,234.56", "1.234,56 €"),
                ("$1,500.00", "1.500,00 €"),
                ("$1,300.00", "1.300,00 €"),
                ("-$90.00", "-90,00 €"),
                ("$100.00", "100,00 €"),
                ("$90.00", "90,00 €"),
                ("$65.44", "65,44 €"),
                ("$5.00", "5,00 €"),
                ("$0.00", "0,00 €"),
            ]
            .iter()
            .fold(text.replace("\",\"", "\"\t\""), |t, (from, to)| t.replace(from, to))
        };
        let (register, plan) = (euro(REGISTER), euro(PLAN));
        assert!(register.contains("\"1.234,56 €\"\t\"0,00 €\"") && !register.contains('$'));
        let b = import_files(&[file("Register.csv", &register), file("Plan.csv", &plan)], None, "SAR").unwrap();
        assert_eq!(b.currency, "EUR");
        let grocer = b.transactions.iter().find(|t| t.payee == "Grocer").unwrap();
        assert_eq!((grocer.date.as_str(), grocer.amount), ("2026-10-15", -123456));
        assert_eq!(b.extra["importAudit"]["categoryDifferences"], json!([]));
        assert_eq!(b.transactions.len(), 5);
    }

    #[test]
    fn asks_for_the_date_order_only_when_dates_could_be_either() {
        let early = REGISTER
            .replace("10/15/2026", "10/11/2026")
            .replace("10/16/2026", "10/12/2026")
            .replace("10/20/2026", "10/10/2026");
        // Exported after every reading of its dates, so none is left out as scheduled.
        let files = [file("Home as of 2026-12-31 - Register.csv", &early), file("Plan.csv", PLAN)];
        assert!(import_files(&files, None, "SAR").unwrap_err().contains("Choose the date order"));
        let b = import_files(&files, Some("dmy"), "SAR").unwrap();
        assert!(b.transactions.iter().any(|t| t.date == "2026-11-10"));
        assert!(import_files(&files, Some("ymd"), "SAR").unwrap_err().contains("chosen date order"));
    }

    #[test]
    fn refuses_exports_it_cannot_read_faithfully() {
        let import = |register: &str, plan: &str| import_files(&[file("Register.csv", register), file("Plan.csv", plan)], None, "SAR");
        // The other side of a transfer is missing.
        let lonely: String = REGISTER.lines().filter(|l| !l.contains("Transfer : Checking")).map(|l| format!("{l}\n")).collect();
        assert!(import(&lonely, PLAN).unwrap_err().contains("no matching row in Savings"));
        // One row of a split is missing.
        let split = REGISTER.replace("\"Grocer\",\"Living: Food\",\"Living\",\"Food\",\"\"", "\"Grocer\",\"Living: Food\",\"Living\",\"Food\",\"Split (1/2) Fruit\"");
        assert!(import(&split, PLAN).unwrap_err().contains("incomplete"));
        // Three-decimal currencies don't fit in cents.
        assert!(import(&REGISTER.replace("$1,234.56", "KD 1.234"), PLAN).unwrap_err().contains("two decimal places"));
        // Only one of the two files.
        assert!(import_files(&[file("Register.csv", REGISTER)], None, "SAR").unwrap_err().contains("Plan file is missing"));
    }
}
