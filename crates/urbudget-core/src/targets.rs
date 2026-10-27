//! Funding targets operate on assignment history, separately from account balances.
use crate::t;
use crate::domain::*;
use chrono::{Datelike, Duration, Months, NaiveDate};
use std::collections::{BTreeMap, HashMap};

/// Per-category cumulative assignments. One index serves every historical month and target.
pub struct AssignmentIndex<'a>(HashMap<&'a str, Vec<(&'a str, i64)>>);
impl<'a> AssignmentIndex<'a> {
    pub fn new(b: &'a Budget) -> Self {
        let mut rows: HashMap<&str, Vec<(&str, i64)>> = HashMap::new();
        for a in &b.assignments {
            rows.entry(&a.category_id).or_default().push((&a.month, a.amount));
        }
        for values in rows.values_mut() {
            values.sort_unstable_by_key(|(month, _)| *month);
            let mut sum = 0;
            for (_, value) in values { sum += *value; *value = sum; }
        }
        Self(rows)
    }
    fn between(&self, id: &str, start: &str, end: &str) -> i64 {
        let Some(rows) = self.0.get(id) else { return 0 };
        let before = |month: &str| {
            rows.partition_point(|(m, _)| *m < month).checked_sub(1).map(|i| rows[i].1).unwrap_or(0)
        };
        before(end) - before(start)
    }
}

/// Validate only the metadata used by the calculator. Provider fields we do not
/// interpret remain intact, and older targets may omit fields or store nulls.
pub(crate) fn validate_goal(c: &Category) -> Result<(), String> {
    let Some(goal) = c.extra.get("goal").filter(|v| !v.is_null()) else { return Ok(()) };
    let invalid = |field: &str| t!("Category “{name}” ({id}) has invalid target field {field}.", name = c.name, id = c.id, field);
    let goal = goal.as_object().ok_or_else(|| invalid("goal"))?;
    for field in ["goal_creation_month", "goal_target_date", "goal_target_month"] {
        if let Some(value) = goal.get(field).filter(|v| !v.is_null()) {
            if !value.as_str().is_some_and(|s| valid_month(s) || valid_date(s)) {
                return Err(invalid(field));
            }
        }
    }
    // YNAB's API sends the snooze time as a date-time, with or without a time zone
    // ("2026-10-20T16:30:50.427"). Older local editors wrote a month or date. Only its month
    // is used, so any of these is accepted without rewriting the source value.
    if let Some(value) = goal.get("goal_snoozed_at").filter(|v| !v.is_null()) {
        let valid = value.as_str().is_some_and(|s| valid_month(s) || valid_date(s)
            || (s.get(..10).is_some_and(valid_date)
                && (chrono::DateTime::parse_from_rfc3339(s).is_ok()
                    || chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").is_ok())));
        if !valid { return Err(t!("{problem} It reads “{value}”.", problem = invalid("goal_snoozed_at"), value)); }
    }
    for (field, maximum) in [("goal_cadence", 14), ("goal_cadence_frequency", 100), ("goal_day", 31)] {
        if let Some(value) = goal.get(field).filter(|v| !v.is_null()) {
            if !value.as_u64().is_some_and(|n| n <= maximum) { return Err(invalid(field)); }
        }
    }
    if goal.get("goal_type").is_some_and(|v| !v.is_null() && !v.is_string()) {
        return Err(invalid("goal_type"));
    }
    if goal.get("goal_needs_whole_amount").is_some_and(|v| !v.is_null() && !v.is_boolean()) {
        return Err(invalid("goal_needs_whole_amount"));
    }
    Ok(())
}

fn month_date(s: &str) -> Option<NaiveDate> {
    if !valid_month(s) && !valid_date(s) { return None; }
    NaiveDate::parse_from_str(&format!("{}-01", s.get(..7)?), "%Y-%m-%d").ok()
}
fn span(a: NaiveDate, b: NaiveDate) -> i64 {
    ((b.year() - a.year()) * 12 + b.month() as i32 - a.month() as i32) as i64
}
fn weekly_anchor(goal: &serde_json::Value, created: NaiveDate) -> Option<NaiveDate> {
    let weekday = goal["goal_day"].as_u64().unwrap_or(0).min(6) as u32;
    let anchor = goal["goal_target_date"].as_str()
        .filter(|s| valid_date(s)).and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).unwrap_or(created);
    // Old editors/imports may retain a due date on another weekday. Both funding and
    // quick-assign priority must use the same aligned anchor and recurrence phase.
    anchor.checked_add_signed(Duration::days((7 + weekday - anchor.weekday().num_days_from_sunday()) as i64 % 7))
}
/// The day a target was created. YNAB exports a weekly target's creation month with its day
/// ("2026-10-20"); every other target carries the 1st.
fn creation_day(goal: &serde_json::Value) -> Option<NaiveDate> {
    goal["goal_creation_month"].as_str().filter(|s| valid_date(s))
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}
/// The month's weekly due dates. In the month a target was created, only those from its
/// creation day on count: Saturdays from October 20th are the 24th and the 31st.
fn weekly_dates(goal: &serde_json::Value, date: NaiveDate, created: NaiveDate) -> Vec<NaiveDate> {
    let Some(next) = date.checked_add_months(Months::new(1)) else { return vec![] };
    let weekday = goal["goal_day"].as_u64().unwrap_or(0).min(6) as u32;
    let frequency = goal["goal_cadence_frequency"].as_u64().unwrap_or(1).clamp(1, 100) as i64;
    let Some(anchor) = weekly_anchor(goal, created) else { return vec![] };
    let from = creation_day(goal).unwrap_or(date);
    (0..(next - date).num_days()).filter_map(|n| date.checked_add_signed(Duration::days(n)))
        .filter(|d| *d >= from && d.weekday().num_days_from_sunday() == weekday
            && (*d - anchor).num_days().rem_euclid(7 * frequency) == 0)
        .collect()
}

/// Additional assignment required by each weekly deadline, measured from the same
/// starting row. Cumulative amounts let other dated bills fit between occurrences
/// without counting existing assignments, refill carry, or earlier funding twice.
pub fn weekly_funding_checkpoints(
    c: &Category, month: &str, assigned: i64, carry: i64, as_of: &str,
) -> Option<Vec<(String, i64)>> {
    let g = c.extra.get("goal")?;
    if g["goal_type"] != "NEED" || g["goal_cadence"].as_i64() != Some(2) { return None; }
    let date = month_date(month)?;
    let created = g["goal_creation_month"].as_str().and_then(month_date).unwrap_or(date);
    if date < created || g["goal_snoozed_at"].as_str().is_some_and(|d| d.starts_with(month)) {
        return Some(vec![]);
    }
    let inherited = if g["goal_needs_whole_amount"].as_bool() == Some(false) && month <= as_of { carry } else { 0 };
    Some(weekly_dates(g, date, created).into_iter().enumerate()
        .map(|(i, due)| (due.to_string(), (c.target * (i as i64 + 1) - assigned - inherited).max(0)))
        .collect())
}

/// An undated balance is an optional saving goal, not a global funding obligation: the
/// summary's `needed` and Underfunded leave it out ("2,000.00 more needed eventually").
/// `needed` below still measures its shortfall, so Reduce overfunding keeps the balance.
pub fn undated_balance(c: &Category) -> bool {
    c.extra.get("goal").is_some_and(|g| g["goal_type"] == "TB")
}
pub fn needed(
    b: &Budget,
    c: &Category,
    month: &str,
    assigned: i64,
    available: i64,
    carry: i64,
    history: &BTreeMap<String, Summary>,
) -> i64 {
    needed_indexed(b, c, month, assigned, available, carry, history, &AssignmentIndex::new(b))
}
pub fn needed_indexed(
    b: &Budget, c: &Category, month: &str, assigned: i64, available: i64, carry: i64,
    history: &BTreeMap<String, Summary>, assignments: &AssignmentIndex,
) -> i64 {
    let as_of = b
        .extra
        .get("calculationAsOfMonth")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m").to_string());
    needed_indexed_at(c, month, assigned, available, carry, history, assignments, &as_of)
}

/// Evaluate rollover against the same reference month used by the budget calculation.
pub fn needed_indexed_at(
    c: &Category, month: &str, assigned: i64, available: i64, carry: i64,
    history: &BTreeMap<String, Summary>, assignments: &AssignmentIndex, as_of: &str,
) -> i64 {
    needed_with_available(c, month, assigned, available, carry, |previous| {
        history.get(previous).and_then(|s| s.categories.iter().find(|x| x.id == c.id))
            .map(|x| x.available).unwrap_or(0)
    }, assignments, as_of)
}

/// Target history only needs a category's closing availability, not account balances,
/// other category rows, or previously evaluated targets. The engine supplies its
/// compact month/category index; existing callers can keep using full summaries.
pub(crate) fn needed_with_available(
    c: &Category, month: &str, assigned: i64, available: i64, carry: i64,
    previous_available: impl Fn(&str) -> i64, assignments: &AssignmentIndex, as_of: &str,
) -> i64 {
    let Some(g) = c.extra.get("goal").filter(|g| !g.is_null()) else {
        return (c.target - assigned).max(0);
    };
    if g["goal_snoozed_at"]
        .as_str()
        .is_some_and(|d| d.starts_with(month))
    {
        return 0;
    }
    let Some(date) = month_date(month) else { return 0 };
    let created = g["goal_creation_month"]
        .as_str()
        .and_then(month_date)
        .unwrap_or(date);
    if date < created {
        return 0;
    }
    let kind = g["goal_type"].as_str().unwrap_or("MF");
    if kind == "TB" {
        return (c.target - available).max(0);
    }
    if kind == "MF" || kind == "DEBT" {
        return (c.target - assigned).max(0);
    }
    let cadence = g["goal_cadence"].as_i64().unwrap_or(1);
    let frequency = g["goal_cadence_frequency"]
        .as_u64()
        .unwrap_or(1)
        .clamp(1, 100) as u32;
    // Future refill targets cannot rely on carryover that may still be spent.
    let refill = g["goal_needs_whole_amount"].as_bool() == Some(false) && month <= as_of;
    if kind == "NEED" && cadence == 2 {
        let count = weekly_dates(g, date, created).len() as i64;
        return (c.target * count - assigned - if refill { carry } else { 0 }).max(0);
    }
    let repeat = match cadence {
        1 => frequency,
        3..=12 => (cadence - 1) as u32,
        13 => 12 * frequency,
        14 => 24,
        _ => 0,
    };
    let target = g["goal_target_date"]
        .as_str()
        .or(g["goal_target_month"].as_str())
        .and_then(month_date);
    let mut due = target.unwrap_or(date);
    if kind == "TBD" {
        if date > due { return 0; }
        let months = (span(date, due) + 1).max(1);
        return (((c.target - (available - assigned)).max(0) + months - 1) / months - assigned)
            .max(0);
    }
    // A monthly target's first due month can fall after its creation month. YNAB then
    // spreads that first amount over every month up to and including the due month
    // (created 2025-02, due 2025-03-01: 2 months to budget), like the cadences below.
    if repeat == 1 && !target.is_some_and(|t| t > created && t >= date) {
        return (c.target - assigned - if refill { carry } else { 0 }).max(0);
    }
    let mut start = created;
    if date > due {
        if repeat == 0 {
            return 0;
        }
        let cycles = (span(due, date) as u32 + repeat - 1) / repeat;
        let Some(next_start) = due.checked_add_months(Months::new((cycles - 1) * repeat + 1)) else { return 0 };
        let Some(next_due) = due.checked_add_months(Months::new(cycles * repeat)) else { return 0 };
        start = next_start;
        due = next_due;
    }
    let start_key = start.format("%Y-%m").to_string();
    let funded = assignments.between(&c.id, &start_key, month);
    let inherited = if refill {
        if start == date {
            carry
        } else {
            start.checked_sub_months(Months::new(1))
                .map(|d| previous_available(&d.format("%Y-%m").to_string()).max(0)).unwrap_or(0)
        }
    } else {
        0
    };
    let months = (span(date, due) + 1).max(1);
    (((c.target - funded - inherited).max(0) + months - 1) / months - assigned).max(0)
}

/// The applicable due date in the selected month/cycle, for quick-assign priority.
pub fn due_date(c: &Category, month: &str) -> String {
    let Some(date) = month_date(month) else { return "9999-12-31".into() };
    let last = date.checked_add_months(Months::new(1)).and_then(|d| d.pred_opt()).unwrap_or(date);
    let format_due = |d: NaiveDate| if (0..=9999).contains(&d.year()) { d.to_string() } else { "9999-12-31".into() };
    let Some(g) = c.extra.get("goal") else { return last.to_string() };
    let kind = g["goal_type"].as_str().unwrap_or("MF");
    let cadence = g["goal_cadence"].as_u64().unwrap_or(1);
    let frequency = g["goal_cadence_frequency"].as_u64().unwrap_or(1).clamp(1, 100) as u32;
    if kind == "NEED" && cadence == 2 {
        let created = g["goal_creation_month"].as_str().and_then(month_date).unwrap_or(date);
        let Some(anchor) = weekly_anchor(g, created) else { return last.to_string() };
        // The first due date in the month it was created comes on or after its creation day.
        let from = creation_day(g).filter(|d| *d > date).unwrap_or(date);
        let offset = (anchor - from).num_days().rem_euclid(7 * frequency as i64);
        return from.checked_add_signed(Duration::days(offset)).map(format_due).unwrap_or_else(|| last.to_string());
    }
    let target = g["goal_target_date"].as_str().or(g["goal_target_month"].as_str())
        .and_then(|s| if valid_date(s) { NaiveDate::parse_from_str(s, "%Y-%m-%d").ok() } else { month_date(s) });
    // Until a monthly target's first due month, it is funded like any later-dated target.
    let first_due_later = kind == "NEED" && target.is_some_and(|d| d > last);
    if kind != "TBD" && !first_due_later && (kind != "NEED" || (cadence == 1 && frequency == 1)) {
        let day = g["goal_day"].as_u64().unwrap_or(last.day() as u64).clamp(1, last.day() as u64) as u32;
        return date.with_day(day).unwrap_or(last).to_string();
    }
    let mut due = target.unwrap_or(last);
    let repeat = match cadence { 1 => frequency, 3..=12 => cadence as u32 - 1, 13 => 12 * frequency, 14 => 24, _ => 0 };
    if kind == "NEED" && repeat > 0 && due < date {
        let cycles = (span(due, date) as u32 + repeat - 1) / repeat;
        due = due.checked_add_months(Months::new(cycles * repeat)).unwrap_or(last);
    }
    format_due(due)
}

/// The deadline this month's target funding is due by, for the target text. A monthly
/// target's recurs on a day of every month ("needed by the 31st"); a dated target's is named
/// when it falls in this month ("needed by October 1st"). None when there's no single deadline
/// this month: a savings builder or balance has none, a weekly target has several, and a
/// dated target due later asks only for this month's share ("needed this month").
pub fn target_due(c: &Category, month: &str) -> Option<TargetDue> {
    if c.target <= 0 { return None; }
    let monthly = match c.extra.get("goal").filter(|g| !g.is_null()) {
        None => true,
        Some(g) => match g["goal_type"].as_str().unwrap_or("MF") {
            "MF" | "TB" => return None,
            "DEBT" => true,
            "NEED" => match (g["goal_cadence"].as_u64().unwrap_or(1), g["goal_cadence_frequency"].as_u64().unwrap_or(1).max(1)) {
                (2, _) => return None,
                (1, 1) => true,
                _ => false,
            },
            _ => false,
        },
    };
    let date = due_date(c, month);
    date.starts_with(month).then_some(TargetDue { date, monthly })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn cat(goal: serde_json::Value) -> Category {
        Category {
            id: "c".into(),
            name: "Travel".into(),
            group: "Goals".into(),
            target: 12000,
            extra: serde_json::from_value(json!({"goal":goal})).unwrap(),
        }
    }
    #[test]
    fn malformed_target_dates_and_consumed_metadata_are_rejected_before_calculation() {
        for value in [json!(NaiveDate::MAX.to_string()), json!("123456é89"), json!("2026-02-30"), json!("2026-1"), json!(17)] {
            let c = cat(json!({"goal_type":"NEED", "goal_cadence":2, "goal_target_date":value}));
            let b = Budget { categories:vec![c.clone()], ..Budget::default() };
            let result = std::panic::catch_unwind(|| crate::engine::calculate_at(&b, "2026-10", "2026-10"));
            assert!(result.is_ok(), "validation must return an error, not panic");
            assert!(result.unwrap().unwrap_err().contains("goal_target_date"));
            // Even direct helper calls must never perform unchecked chrono-MAX arithmetic.
            assert!(std::panic::catch_unwind(|| due_date(&c, "2026-10")).is_ok());
        }
        for (field, value) in [
            ("goal_type", json!(1)), ("goal_needs_whole_amount", json!("false")),
            ("goal_day", json!(32)), ("goal_cadence", json!(-1)),
            ("goal_cadence_frequency", json!(u64::MAX)),
        ] {
            let mut c = cat(json!({"goal_type":"NEED"}));
            c.extra.get_mut("goal").unwrap()[field] = value;
            assert!(validate_goal(&c).unwrap_err().contains(field));
        }
        assert!(validate_goal(&cat(json!([]))).is_err());
    }
    #[test]
    fn target_metadata_preserves_null_legacy_dates_and_calendar_boundaries() {
        for value in [json!(null), json!("0000-01"), json!("0000-01-01"), json!("9999-12"), json!("9999-12-31")] {
            let c = cat(json!({"goal_type":"NEED", "goal_target_date":value,
                "goal_creation_month":null, "goal_snoozed_at":null, "goal_cadence_frequency":0,
                "unknown_provider_field":{"nested":"preserved"}}));
            validate_goal(&c).unwrap();
        }
        validate_goal(&cat(json!(null))).unwrap();
        let b = Budget::default();
        for month in ["0000-01", "9999-12"] {
            let c = cat(json!({"goal_type":"TBD", "goal_target_date":format!("{month}-01")}));
            assert_eq!(needed(&b, &c, month, 0, 0, 0, &BTreeMap::new()), 12000);
        }
        let last = NaiveDate::from_ymd_opt(9999, 12, 31).unwrap();
        let c = cat(json!({"goal_type":"NEED", "goal_cadence":2, "goal_cadence_frequency":100,
            "goal_creation_month":"9999-12", "goal_target_date":"9999-12-31",
            "goal_day":(last.weekday().num_days_from_sunday() + 1) % 7}));
        validate_goal(&c).unwrap();
        assert_eq!(due_date(&c, "9999-12"), "9999-12-31");
        assert!(weekly_funding_checkpoints(&c, "9999-12", 0, 0, "9999-12").unwrap().is_empty());
    }
    #[test]
    fn snooze_accepts_api_timestamps_and_legacy_dates_without_changing_month_behavior() {
        let b = Budget::default();
        // YNAB's API leaves the time zone off ("2026-10-15T12:34:56.427"), seen on a live plan.
        for at in ["2026-10", "2026-10-15", "2026-10-15T12:34:56Z", "2026-10-15T12:34:56.427", "2026-10-15T12:34:56",
            "2026-10-15T12:34:56.123456+03:00", "2026-10-15T12:34:56-05:00"] {
            let c = cat(json!({"goal_type":"NEED", "goal_cadence":2, "goal_day":0, "goal_snoozed_at":at}));
            validate_goal(&c).unwrap();
            assert_eq!(c.extra["goal"]["goal_snoozed_at"], at);
            assert_eq!(needed(&b, &c, "2026-10", 0, 0, 0, &BTreeMap::new()), 0);
            assert!(weekly_funding_checkpoints(&c, "2026-10", 0, 0, "2026-10").unwrap().is_empty());
            assert_eq!(needed(&b, &c, "2026-11", 0, 0, 0, &BTreeMap::new()), 60000);
        }
        for at in ["0000-01-01T00:00:00Z", "9999-12-31T23:59:59Z"] {
            validate_goal(&cat(json!({"goal_snoozed_at":at}))).unwrap();
        }
        for at in ["2026-10-15T25:00:00Z", "2026-10-15T25:00:00", "2026-02-30T12:34:56Z",
            "+262142-12-31T00:00:00Z", "2026-10-15T12:34:56Zjunk", "123456é89T12:34:56Z"] {
            assert!(validate_goal(&cat(json!({"goal_snoozed_at":at}))).unwrap_err().contains("goal_snoozed_at"), "{at}");
        }
        // A timestamp is appropriate for snoozing, not for the API's date-only fields.
        assert!(validate_goal(&cat(json!({"goal_target_date":"2026-10-15T12:34:56Z"}))).is_err());
    }
    #[test]
    fn yearly_set_aside_uses_assignments_even_after_spending_and_repeats() {
        let c = cat(
            json!({"goal_type":"NEED","goal_cadence":13,"goal_target_date":"2026-12-31","goal_creation_month":"2026-01-01","goal_needs_whole_amount":true}),
        );
        let mut b = Budget::default();
        b.assignments.push(Assignment { extra: Default::default(),
            month: "2026-01".into(),
            category_id: "c".into(),
            amount: 6000,
        });
        assert_eq!(needed(&b, &c, "2026-02", 0, 0, 0, &BTreeMap::new()), 546);
        assert_eq!(
            needed(&b, &c, "2027-01", 0, 6000, 6000, &BTreeMap::new()),
            1000
        );
    }
    #[test]
    fn monthly_target_spreads_its_first_amount_over_the_months_up_to_a_later_due_month() {
        // As imported from YNAB: 2,000.00 each month, created 2025-02 with target month
        // 2025-03-01. YNAB needs 1,000.00 in February (2 months to budget), the rest by March.
        let mut c = cat(json!({"goal_type":"NEED", "goal_cadence":1, "goal_cadence_frequency":1,
            "goal_creation_month":"2025-02-01", "goal_target_month":"2025-03-01", "goal_day":null,
            "goal_needs_whole_amount":true}));
        c.target = 200000;
        for (february, february_needed, march_needed) in [(0, 100000, 200000), (40000, 60000, 160000), (100000, 0, 100000)] {
            let mut b = Budget { categories: vec![c.clone()], ..Budget::default() };
            b.assignments.push(Assignment { extra: Default::default(), month: "2025-02".into(), category_id: "c".into(), amount: february });
            let row = |month: &str| {
                let row = &crate::engine::calculate_at(&b, month, month).unwrap().categories[0];
                (row.needed, row.target_funding_required)
            };
            assert_eq!(row("2025-01"), (0, 0));
            assert_eq!(row("2025-02"), (february_needed, 100000));
            assert_eq!(row("2025-03"), (march_needed, march_needed));
            // Later months are ordinary monthly periods again.
            assert_eq!(row("2025-04"), (200000, 200000));
        }
        // Quick assign treats it as due next month until its due month arrives.
        assert_eq!(due_date(&c, "2025-02"), "2025-03-01");
        assert_eq!(due_date(&c, "2025-03"), "2025-03-31");
    }
    #[test]
    fn refill_and_weekly_calendar_and_snooze() {
        let c = cat(json!({"goal_type":"NEED","goal_cadence":1,"goal_needs_whole_amount":false}));
        assert_eq!(
            needed(
                &Budget::default(),
                &c,
                "2026-10",
                2000,
                0,
                3000,
                &BTreeMap::new()
            ),
            7000
        );
        let mut c = cat(
            json!({"goal_type":"NEED","goal_cadence":2,"goal_day":4,"goal_cadence_frequency":1}),
        );
        c.target = 100;
        assert_eq!(
            needed(&Budget::default(), &c, "2026-10", 0, 0, 0, &BTreeMap::new()),
            500
        );
        c.extra.get_mut("goal").unwrap()["goal_snoozed_at"] = json!("2026-10-01");
        assert_eq!(
            needed(&Budget::default(), &c, "2026-10", 0, 0, 0, &BTreeMap::new()),
            0
        );
    }
    #[test]
    fn a_weekly_target_counts_only_due_dates_from_its_creation_day_in_its_first_month() {
        // As YNAB exports Groceries: 100.00 every Saturday, created on Tuesday 2026-10-20.
        let mut c = cat(json!({"goal_type":"NEED", "goal_cadence":2, "goal_cadence_frequency":1, "goal_day":6,
            "goal_needs_whole_amount":true, "goal_creation_month":"2026-10-20"}));
        c.target = 10000;
        let b = Budget { categories: vec![c.clone()], ..Budget::default() };
        let history = BTreeMap::new();
        // October's Saturdays from the 20th are the 24th and the 31st: 400.00 assigned is funded.
        assert_eq!(needed(&b, &c, "2026-10", 40000, 12936, 1976, &history), 0);
        assert_eq!(needed(&b, &c, "2026-10", 0, 0, 0, &history), 20000);
        assert_eq!(weekly_funding_checkpoints(&c, "2026-10", 0, 0, "2026-10").unwrap(),
            vec![("2026-10-24".into(), 10000), ("2026-10-31".into(), 20000)]);
        assert_eq!(due_date(&c, "2026-10"), "2026-10-24");
        // November has four Saturdays, all of them due.
        assert_eq!(needed(&b, &c, "2026-11", 0, 12936, 12936, &history), 40000);
        assert_eq!(weekly_funding_checkpoints(&c, "2026-11", 0, 0, "2026-10").unwrap().len(), 4);
        assert_eq!(due_date(&c, "2026-11"), "2026-11-07");
        // A creation month without a day (other targets, older editors) counts the whole month.
        c.extra.get_mut("goal").unwrap()["goal_creation_month"] = json!("2026-10-01");
        assert_eq!(needed(&b, &c, "2026-10", 0, 0, 0, &history), 50000);
        assert_eq!(due_date(&c, "2026-10"), "2026-10-03");
    }
    #[test]
    fn dated_balance_uses_remaining_months_and_cents_round_up() {
        let c = cat(json!({"goal_type":"TBD","goal_target_date":"2026-12-31"}));
        assert_eq!(
            needed(
                &Budget::default(),
                &c,
                "2026-10",
                1000,
                4000,
                3000,
                &BTreeMap::new()
            ),
            2000
        );
    }
    #[test]
    fn dated_balance_stops_requesting_funds_after_its_due_month() {
        let c = cat(json!({"goal_type":"TBD","goal_target_date":"2026-09-15","goal_creation_month":"2026-08-01"}));
        let b = Budget::default();
        assert_eq!(needed(&b, &c, "2026-08", 0, 0, 0, &BTreeMap::new()), 6000);
        assert_eq!(needed(&b, &c, "2026-09", 0, 0, 0, &BTreeMap::new()), 12000);
        // Spending the saved balance after its one-time deadline must not restart it.
        assert_eq!(needed(&b, &c, "2026-10", 0, 0, 0, &BTreeMap::new()), 0);
        assert_eq!(needed(&b, &c, "2027-01", 0, 0, 0, &BTreeMap::new()), 0);
    }
    #[test]
    fn weekly_checkpoints_count_existing_funding_and_refill_carry_once() {
        let mut c = cat(json!({"goal_type":"NEED","goal_cadence":2,"goal_day":0,"goal_needs_whole_amount":false}));
        c.target = 10000;
        let checkpoints = weekly_funding_checkpoints(&c, "2026-10", 5000, 10000, "2026-10").unwrap();
        assert_eq!(checkpoints, vec![("2026-10-04".into(), 0), ("2026-10-11".into(), 5000),
            ("2026-10-18".into(), 15000), ("2026-10-25".into(), 25000)]);
        assert_eq!(weekly_funding_checkpoints(&c, "2026-10", 5000, 10000, "2026-09").unwrap()[0].1, 5000);
        c.extra.get_mut("goal").unwrap()["goal_snoozed_at"] = json!("2026-10-01");
        assert!(weekly_funding_checkpoints(&c, "2026-10", 0, 0, "2026-10").unwrap().is_empty());
    }
    #[test]
    fn future_refill_ignores_unspent_current_month_carryover() {
        let c = cat(json!({"goal_type":"NEED","goal_cadence":1,"goal_needs_whole_amount":false}));
        let mut b = Budget::default();
        b.extra
            .insert("calculationAsOfMonth".into(), json!("2026-10"));
        assert_eq!(
            needed(&b, &c, "2026-11", 0, 3000, 3000, &BTreeMap::new()),
            12000
        );
        assert_eq!(
            needed(&b, &c, "2026-10", 0, 3000, 3000, &BTreeMap::new()),
            9000
        );
    }
    #[test]
    fn explicit_reference_month_controls_refill_carryover() {
        let b = Budget::default();
        let assignments = AssignmentIndex::new(&b);
        let history = BTreeMap::new();
        for (cadence, full) in [(1, 12000), (2, 48000)] {
            let c = cat(json!({"goal_type":"NEED", "goal_cadence":cadence,
                "goal_day":0, "goal_needs_whole_amount":false}));
            assert_eq!(needed_indexed_at(&c, "2026-10", 2000, 3000, 3000,
                &history, &assignments, "2026-09"), full - 2000);
            assert_eq!(needed_indexed_at(&c, "2026-10", 2000, 3000, 3000,
                &history, &assignments, "2026-10"), full - 5000);
        }
    }
    #[test]
    fn hidden_weekly_fields_cannot_disable_weekly_or_dated_balance_targets() {
        let mut c = cat(json!({"goal_type":"NEED","goal_cadence":2,"goal_day":0,"goal_target_date":"2026-12-31","goal_creation_month":"2026-10-01"}));
        c.target = 10000;
        assert_eq!(needed(&Budget::default(), &c, "2026-10",0,0,0,&BTreeMap::new()),40000);
        c.extra.get_mut("goal").unwrap()["goal_type"] = json!("TBD");
        assert_eq!(needed(&Budget::default(), &c, "2026-10",0,0,0,&BTreeMap::new()),3334);
    }
    #[test]
    fn due_priority_uses_monthly_weekly_and_rolled_recurring_dates() {
        let monthly = cat(json!({"goal_type":"NEED","goal_cadence":1,"goal_day":1}));
        let weekly = cat(json!({"goal_type":"NEED","goal_cadence":2,"goal_day":0}));
        let annual = cat(json!({"goal_type":"NEED","goal_cadence":13,"goal_target_date":"2025-09-30"}));
        assert_eq!(due_date(&monthly,"2026-10"),"2026-10-01");
        assert_eq!(due_date(&weekly,"2026-10"),"2026-10-04");
        assert_eq!(due_date(&annual,"2026-10"),"2027-09-30");
    }
    #[test]
    fn target_due_names_this_months_deadline_or_leaves_it_to_the_month() {
        let due = |goal: serde_json::Value, month: &str| target_due(&cat(goal), month).map(|d| (d.date, d.monthly));
        let day = |date: &str, monthly| Some((date.to_string(), monthly));
        // Monthly needs fall due on their day each month, the last day unless one is set.
        assert_eq!(due(json!({"goal_type":"NEED","goal_cadence":1}), "2026-10"), day("2026-10-31", true));
        assert_eq!(due(json!({"goal_type":"NEED","goal_cadence":1}), "2026-11"), day("2026-11-30", true));
        assert_eq!(due(json!({"goal_type":"NEED","goal_cadence":1,"goal_day":15}), "2026-10"), day("2026-10-15", true));
        assert_eq!(due(json!({"goal_type":"NEED","goal_cadence":1,"goal_day":31}), "2027-02"), day("2027-02-28", true));
        assert_eq!(due(json!({"goal_type":"DEBT","goal_day":1}), "2026-10"), day("2026-10-01", true));
        assert_eq!(target_due(&Category { extra: Default::default(), ..cat(json!(null)) }, "2026-10"), Some(TargetDue { date: "2026-10-31".into(), monthly: true }));
        // A dated target is named in its own month and spread over the months before it.
        let annual = json!({"goal_type":"NEED","goal_cadence":13,"goal_target_date":"2025-10-01"});
        assert_eq!(due(annual.clone(), "2026-10"), day("2026-10-01", false));
        assert_eq!(due(annual, "2026-11"), None, "next due 2027-10-01");
        let every_two_months = json!({"goal_type":"NEED","goal_cadence":1,"goal_cadence_frequency":2,"goal_target_date":"2026-11-20"});
        assert_eq!(due(every_two_months.clone(), "2026-10"), None);
        assert_eq!(due(every_two_months, "2026-11"), day("2026-11-20", false));
        let dated = json!({"goal_type":"TBD","goal_target_date":"2026-12-15"});
        assert_eq!(due(dated.clone(), "2026-10"), None);
        assert_eq!(due(dated.clone(), "2026-12"), day("2026-12-15", false));
        assert_eq!(due(dated, "2027-01"), None, "past its date");
        // No single deadline: savings builders and balances have none, weekly targets several.
        for goal in [json!({"goal_type":"MF"}), json!({"goal_type":"TB"}), json!({"goal_type":"NEED","goal_cadence":2,"goal_day":0})] {
            assert_eq!(due(goal.clone(), "2026-10"), None, "{goal}");
        }
        assert_eq!(target_due(&Category { target: 0, ..cat(json!({"goal_type":"NEED","goal_cadence":1})) }, "2026-10"), None);
    }
    #[test]
    fn assignment_index_respects_exclusive_end_and_negative_adjustments() {
        let mut b=Budget::default();
        for (month,amount) in [("2026-03",300),("2026-01",100),("2026-02",-50)] {
            b.assignments.push(Assignment { extra: Default::default(),month:month.into(),category_id:"c".into(),amount});
        }
        let index=AssignmentIndex::new(&b);
        assert_eq!(index.between("c","2026-01","2026-03"),50);
        assert_eq!(index.between("c","2026-02","2026-04"),250);
        assert_eq!(index.between("missing","2026-01","2026-04"),0);
    }
    #[test]
    fn weekly_due_priority_respects_the_recurrence_phase_in_each_month() {
        let c = cat(json!({"goal_type":"NEED","goal_cadence":2,"goal_cadence_frequency":2,"goal_day":0,"goal_target_date":"2026-10-11","goal_creation_month":"2026-10-01"}));
        assert_eq!(due_date(&c, "2026-10"), "2026-10-11");
        assert_eq!(due_date(&c, "2026-11"), "2026-11-08");
        assert_eq!(needed(&Budget::default(), &c, "2026-10", 0, 0, 0, &BTreeMap::new()), 24000);
        let mut old = c.clone();
        old.extra.get_mut("goal").unwrap()["goal_target_date"] = serde_json::json!("2026-10-08");
        assert_eq!(due_date(&old, "2026-10"), "2026-10-11", "aligned stale weekday keeps the same phase as funding");
    }
}
