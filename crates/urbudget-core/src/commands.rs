//! The engine's methods on a budget, for every caller. The stateless JSON bridge and the store's
//! `edit_budget` both run these, so one command can't behave differently on the two paths.
use crate::domain::{self, Budget, Summary};
use crate::patch::Patch;
use crate::{bank_csv, bank_import, bank_statement, engine, ledger, loans, reports, schedules, sms_import, t, workflows, ynab};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

/// A changing method's result.
pub struct Outcome {
    pub budget: Budget,
    /// `applySchedules` and `scheduleFuture`: how many transactions moved.
    pub count: Option<usize>,
    /// `workflow`: the month its calculations were referred to.
    pub reference: Option<String>,
}

/// A step of an edit: a plain patch, or a command. Never both: a step's fields decide which it is,
/// and an unknown field would otherwise drop the other half without a word.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Step {
    Patch {
        patch: Patch,
    },
    Command {
        method: String,
        #[serde(default = "empty_object")]
        params: Value,
    },
}

fn empty_object() -> Value {
    json!({})
}
pub fn encode(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}
pub fn required<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, String> {
    let value = params
        .get(key)
        .ok_or_else(|| format!("Missing parameter: {key}"))?;
    serde_json::from_value(value.clone()).map_err(|error| format!("Invalid {key}: {error}"))
}
pub fn optional<T: DeserializeOwned>(params: &Value, key: &str) -> Result<Option<T>, String> {
    params
        .get(key)
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|error| format!("Invalid {key}: {error}"))
        })
        .transpose()
}

pub(crate) fn apply_workflow(
    mut budget: Budget,
    params: &Value,
    current_month: &str,
) -> Result<(Budget, String), String> {
    let action: Value = required(params, "action")?;
    let reference =
        optional::<String>(params, "asOfMonth")?.unwrap_or_else(|| current_month.to_owned());
    if !domain::valid_month(&reference) {
        return Err(t!("Invalid calculation reference month."));
    }
    // Never let a saved/imported reference or an old preview bypass the calendar
    // guard. Android supplies the instant at which it prepared this request.
    if reference != current_month
        || action
            .get("expectedAsOfMonth")
            .is_some_and(|expected| expected.as_str() != Some(current_month))
    {
        return Err(t!(
            "The calendar month changed. Refresh the Quick assign preview before applying it."
        ));
    }
    let previous = budget
        .extra
        .insert("calculationAsOfMonth".into(), json!(reference));
    let mut changed = workflows::apply(budget, &action)?;
    match previous {
        Some(value) => {
            changed.extra.insert("calculationAsOfMonth".into(), value);
        }
        None => {
            changed.extra.remove("calculationAsOfMonth");
        }
    }
    Ok((changed, reference))
}

/// Whether `method` returns a changed budget. `apply` runs those, `read` the rest.
pub fn changes_budget(method: &str) -> bool {
    matches!(
        method,
        "workflow"
            | "saveTransaction"
            | "reconcile"
            | "applyPatch"
            | "applySchedules"
            | "scheduleFuture"
            | "loanRecord"
            | "applyLoanPayments"
            | "bankApply"
            | "smsBatchApply"
    )
}

/// Runs a changing method. What it returns has been validated, whatever the method checked itself.
pub fn apply(mut budget: Budget, method: &str, p: &Value) -> Result<Outcome, String> {
    let mut count = None;
    let mut reference = None;
    match method {
        "workflow" => {
            let (changed, month) = apply_workflow(budget, p, &engine::current_month())?;
            budget = changed;
            reference = Some(month);
        }
        "saveTransaction" => {
            budget = workflows::upsert_transaction(budget, required(p, "transaction")?)?;
            if let Some(through) = optional::<String>(p, "through")? {
                schedules::schedule_future(&mut budget, &through)?;
            }
        }
        "reconcile" => {
            budget = domain::reconcile(
                budget,
                &required::<String>(p, "accountId")?,
                required(p, "balance")?,
                &required::<String>(p, "date")?,
                &required::<String>(p, "adjustmentId")?,
            )?;
        }
        "applyPatch" => required::<Patch>(p, "patch")?.apply(&mut budget),
        "applySchedules" => {
            count = Some(schedules::apply(
                &mut budget,
                &required::<String>(p, "through")?,
                optional::<String>(p, "selected")?.as_deref(),
                optional(p, "skip")?.unwrap_or(false),
            )?);
        }
        "scheduleFuture" => {
            count = Some(schedules::schedule_future(
                &mut budget,
                &required::<String>(p, "through")?,
            )?);
        }
        "loanRecord" => {
            loans::record(
                &mut budget,
                &required(p, "terms")?,
                &required::<String>(p, "from")?,
                required(p, "amount")?,
                &required::<String>(p, "date")?,
                &required::<String>(p, "id")?,
            )?;
        }
        "applyLoanPayments" => {
            loans::apply_new_payments(
                &mut budget,
                &required::<std::collections::HashSet<String>>(p, "ids")?,
            )?;
        }
        // With `remember`, the account keeps how this file read for its next one.
        "bankApply" => {
            let account = required::<String>(p, "accountId")?;
            budget = bank_import::apply(budget, &account, required(p, "choices")?)?;
            if let Some(settings) = optional(p, "remember")? {
                bank_csv::remember(&mut budget, &account, settings)?;
            }
        }
        "smsBatchApply" => {
            budget = sms_import::apply_batch(budget, &required::<Vec<_>>(p, "messages")?, &required(p, "analysis")?, required(p, "decisions")?)?;
        }
        _ => return Err(format!("Unknown engine method: {method}")),
    }
    domain::validate(&budget)?;
    Ok(Outcome { budget, count, reference })
}

/// Answers a method that leaves the budget as it is.
pub fn read(budget: &Budget, method: &str, p: &Value) -> Result<Value, String> {
    match method {
        "validate" => Ok(Value::Null),
        "calculate" => {
            let month: String = required(p, "month")?;
            match optional::<String>(p, "asOfMonth")? {
                Some(as_of) => encode(engine::calculate_at(budget, &month, &as_of)?),
                None => encode(engine::calculate(budget, &month)?),
            }
        }
        "ledger" => encode(ledger::transactions(budget)),
        "reportDetails" => encode(reports::details(
            budget,
            &serde_json::from_value(p.clone()).map_err(|error| format!("Invalid report filter: {error}"))?,
            &required::<String>(p, "dimension")?,
            &required::<String>(p, "key")?,
        )?),
        "reportCsv" => encode(reports::csv(budget, &reports::calculate(budget, &serde_json::from_value(p.clone()).map_err(|error| format!("Invalid report filter: {error}"))?)?)?),
        "reports" => encode(reports::calculate(
            budget,
            &serde_json::from_value(p.clone())
                .map_err(|error| format!("Invalid report filter: {error}"))?,
        )?),
        "forecast" => encode(reports::forecast_for_accounts(
            budget,
            &required::<String>(p, "today")?,
            &required::<String>(p, "through")?,
            &optional::<Vec<String>>(p, "accountIds")?.unwrap_or_default(),
        )?),
        "autoAssignPreview" => encode(workflows::auto_assign_preview(
            budget,
            &required::<String>(p, "month")?,
            &required::<Vec<String>>(p, "categories")?,
            optional(p, "limitToAvailable")?.unwrap_or(true),
        )?),
        "schedules" => encode(schedules::read(budget)?),
        "loanTerms" => encode(loans::source_terms(
            budget,
            &required::<String>(p, "accountId")?,
        )?),
        "bankPreview" => encode(bank_import::preview_compact(
            budget,
            &required::<String>(p, "accountId")?,
            &required::<String>(p, "content")?,
            &required::<String>(p, "format")?,
            &optional::<String>(p, "dateFormat")?.unwrap_or_else(|| "ymd".into()),
            optional(p, "flipSigns")?.unwrap_or(false),
            optional::<bank_csv::Columns>(p, "columns")?.as_ref(),
        )?),
        // An import of a file into an account, as its last file read (see
        // `bank_statement::start`); its statement, every line or with `problemsOnly` those
        // that don't read; its review; and changes to it.
        // With `prepared`, a file the assistant wrote the standard way.
        "bankSession" => {
            let (account, content) = (required::<String>(p, "accountId")?, required::<String>(p, "content")?);
            let (id, budget_id) = (required::<String>(p, "sessionId")?, optional::<String>(p, "budgetId")?.unwrap_or_default());
            if optional(p, "prepared")?.unwrap_or(false) {
                encode(bank_statement::start_prepared(&content, &account, &id, &budget_id))
            } else {
                encode(bank_statement::start(budget, &account, &content, &id, &budget_id, optional(p, "sheet")?)?)
            }
        }
        "bankStatement" => {
            let read = bank_statement::statement(
                budget,
                &required::<String>(p, "accountId")?,
                &required::<String>(p, "content")?,
                &required(p, "session")?,
            )?;
            encode(if optional(p, "problemsOnly")?.unwrap_or(false) { read.problems() } else { read })
        }
        "bankImportPreview" => {
            let mut preview = bank_statement::preview_statement(
                budget,
                &required::<String>(p, "accountId")?,
                &required::<String>(p, "content")?,
                &required(p, "session")?,
            )?;
            if optional(p, "problemsOnly")?.unwrap_or(false) {
                preview.statement = preview.statement.problems();
            }
            encode(preview)
        }
        "bankApplyFixes" => encode(bank_statement::apply_fixes(
            budget,
            &required::<String>(p, "accountId")?,
            &required::<String>(p, "content")?,
            &required(p, "session")?,
            &required(p, "fixes")?,
        )?),
        "smsBatchPreview" => encode(sms_import::preview(budget, &required::<Vec<_>>(p, "messages")?, &required(p, "analysis")?)?),
        // With `ids` or `accountId`, only those transactions (one account's side of a transfer),
        // newest first as the register lists them; without, the whole register.
        "exportRegister" => {
            let ids = optional::<std::collections::HashSet<String>>(p, "ids")?;
            let account = optional::<String>(p, "accountId")?;
            if ids.is_none() && account.is_none() {
                encode(ynab::export_register(budget)?)
            } else {
                encode(ynab::export_register_for(
                    budget,
                    |record_account, id| {
                        account.as_deref().is_none_or(|a| a == record_account)
                            && ids.as_ref().is_none_or(|ids| ids.contains(id))
                    },
                    true,
                )?)
            }
        }
        "exportPlan" => encode(ynab::export_plan(budget)?),
        "auditYnab" => ynab::audit(budget),
        _ => Err(format!("Unknown engine method: {method}")),
    }
}

/// Runs `steps` in order on one budget. Each leaves it valid, so a later one starts from sound data.
pub fn apply_steps(mut budget: Budget, steps: Vec<Step>) -> Result<Budget, String> {
    for step in steps {
        budget = match step {
            Step::Patch { patch } => {
                patch.apply(&mut budget);
                domain::validate(&budget)?;
                budget
            }
            Step::Command { method, params } => apply(budget, &method, &params)?.budget,
        };
    }
    Ok(budget)
}

impl Outcome {
    /// `previewMonth`'s totals, on a `workflow`.
    pub fn preview(&self, params: &Value) -> Result<Option<Summary>, String> {
        let Some(reference) = &self.reference else { return Ok(None); };
        let Some(month) = optional::<String>(params, "previewMonth")? else { return Ok(None); };
        engine::calculate_at(&self.budget, &month, reference).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::sample;

    #[test]
    fn workflow_reference_is_ephemeral_and_cannot_use_stale_imported_context() {
        let mut budget = sample();
        budget
            .extra
            .insert("calculationAsOfMonth".into(), json!("2025-01"));
        let params = json!({"asOfMonth":"2026-10","action":{"type":"autoAssign","month":"2026-10","mode":"underfunded","categories":["food"],"limitToAvailable":true,"expectedAsOfMonth":"2026-10"}});
        let (changed, reference) = apply_workflow(budget.clone(), &params, "2026-10").unwrap();
        assert_eq!(reference, "2026-10");
        assert_eq!(
            changed.extra["calculationAsOfMonth"], "2025-01",
            "ephemeral context never becomes a saved edit"
        );
        let mut expected = budget.clone();
        expected
            .extra
            .insert("calculationAsOfMonth".into(), json!("2026-10"));
        let direct = workflows::apply(expected, &params["action"]).unwrap();
        assert_eq!(changed.assignments, direct.assignments);
        assert!(apply_workflow(budget.clone(), &params, "2026-11")
            .unwrap_err()
            .contains("calendar month changed"));
        let mut stale = params.clone();
        stale["asOfMonth"] = json!("2026-11");
        assert!(apply_workflow(budget, &stale, "2026-11")
            .unwrap_err()
            .contains("calendar month changed"));
    }

    #[test]
    fn every_method_has_one_home_and_unknown_ones_are_refused() {
        for method in ["workflow", "saveTransaction", "reconcile", "applyPatch", "applySchedules", "scheduleFuture", "loanRecord", "applyLoanPayments", "bankApply", "smsBatchApply"] {
            assert!(changes_budget(method), "{method}");
            assert_eq!(read(&sample(), method, &json!({})).unwrap_err(), format!("Unknown engine method: {method}"));
        }
        for method in ["validate", "calculate", "ledger", "reports", "schedules", "exportPlan"] {
            assert!(!changes_budget(method), "{method}");
            assert_eq!(apply(sample(), method, &json!({})).err().unwrap(), format!("Unknown engine method: {method}"));
        }
    }

    #[test]
    fn apply_ends_by_validating_what_a_method_left() {
        let params = json!({"patch":{"assignments":{"upsert":[{"month":"2026-99","categoryId":"food","amount":1}]}}});
        assert!(apply(sample(), "applyPatch", &params).is_err());
    }

    #[test]
    fn steps_run_in_order_and_a_bad_one_stops_them() {
        let steps: Vec<Step> = serde_json::from_value(json!([
            {"patch":{"name":"Renamed"}},
            {"method":"workflow","params":{"action":{"type":"assignAmount","month":"2026-10","categoryId":"food","amount":123}}}
        ])).unwrap();
        let after = apply_steps(sample(), steps).unwrap();
        assert_eq!(after.name, "Renamed");
        assert_eq!(after.assignments[0].amount, 123);

        let bad: Vec<Step> = serde_json::from_value(json!([
            {"patch":{"name":"Renamed"}},
            {"patch":{"assignments":{"upsert":[{"month":"2026-99","categoryId":"food","amount":1}]}}}
        ])).unwrap();
        assert!(apply_steps(sample(), bad).is_err());
        assert_eq!(apply_steps(sample(), vec![]).unwrap(), sample());
    }

    #[test]
    fn a_step_is_a_patch_or_a_command_and_never_both() {
        let step = |value: Value| serde_json::from_value::<Step>(value);
        assert!(matches!(step(json!({"patch":{}})).unwrap(), Step::Patch { .. }));
        assert!(matches!(step(json!({"method":"workflow"})).unwrap(), Step::Command { params, .. } if params == json!({})));
        assert!(step(json!({"patch":{},"method":"workflow"})).is_err());
        assert!(step(json!({"method":"workflow","params":{},"patch":{}})).is_err());
        assert!(step(json!({})).is_err());
    }
}
