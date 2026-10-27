use crate::{domain::Budget, i18n::tr, import_sync, store::Snapshot, t, ynab, Database};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Write, sync::Mutex, time::Instant};


#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub snapshot: Snapshot,
    pub elapsed_ms: u128,
    pub backup: Option<String>,
    pub incremental: bool,
    pub unchanged: bool,
    pub review_id: Option<String>,
    pub conflicts: Vec<import_sync::Conflict>,
    /// Totals that came out different from YNAB's (see ynab::differences). Nothing is saved
    /// until they're accepted.
    pub differences: Vec<Value>,
    /// The saved token this import remembered, so the form can switch to it.
    pub saved_token: Option<String>,
}
impl ImportResult {
    fn of(snapshot: Snapshot, started: Instant, incremental: bool) -> Self {
        Self {
            snapshot,
            elapsed_ms: started.elapsed().as_millis(),
            backup: None,
            incremental,
            unchanged: false,
            review_id: None,
            conflicts: vec![],
            differences: vec![],
            saved_token: None,
        }
    }
}
/// What a downloaded import waits on before it's saved.
#[derive(Clone)]
enum Review {
    /// A choice for each change both this budget and YNAB made.
    Conflicts { remote: Budget },
    /// Acceptance of totals that differ from YNAB's.
    Differences { incoming: Budget, source: Value, replace: bool, differences: Vec<Value> },
}
#[derive(Clone)]
pub struct Pending {
    id: String,
    /// The budget this review was prepared for; another budget cannot apply it.
    budget_id: String,
    local: Budget,
    review: Review,
    expected_token: String,
    incremental: bool,
}
#[derive(Default)]
pub struct PendingImport(pub Mutex<Option<Pending>>);
fn progress(stage: &str) { crate::host::emit("ynab-import-progress", json!(stage)); }
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|_| t!("Could not start import"))
}
fn refusal(status: u16) -> String {
    match status {
        401 | 403 => t!("YNAB rejected the token. Paste an active personal access token."),
        429 => t!("YNAB’s hourly request limit was reached. Try again later."),
        n => t!("YNAB returned HTTP {n}. Your local budget is unchanged.", n),
    }
}
fn review_id() -> String {
    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0).to_string()
}
/// The pasted token, or the saved one the form chose.
fn token_for(token: String, saved_token: Option<&str>) -> Result<String, String> {
    let token = match saved_token {
        Some(id) => crate::credentials::read(id)?,
        None => token,
    };
    if token.trim().is_empty() {
        return Err(t!("Enter your personal access token."));
    }
    Ok(token)
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanChoice {
    pub id: String,
    pub name: String,
    pub last_modified_on: Option<String>,
}
/// The plans in YNAB's listing, most recently edited first, with its default plan (when it names one) on top.
fn plan_choices(listing: &Value) -> Vec<PlanChoice> {
    let data = &listing["data"];
    let plans = data["plans"].as_array().or_else(|| data["budgets"].as_array());
    let mut choices: Vec<PlanChoice> = plans
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(PlanChoice {
                id: p["id"].as_str()?.to_owned(),
                name: p["name"].as_str().unwrap_or(tr("Untitled plan")).to_owned(),
                last_modified_on: p["last_modified_on"].as_str().map(str::to_owned),
            })
        })
        .collect();
    choices.sort_by(|a, b| b.last_modified_on.cmp(&a.last_modified_on));
    let default = data["default_plan"]["id"].as_str().or_else(|| data["default_budget"]["id"].as_str());
    if let Some(i) = default.and_then(|id| choices.iter().position(|c| c.id == id)) {
        let first = choices.remove(i);
        choices.insert(0, first);
    }
    choices
}

/// Authenticated download shared by both clients. A rejected knowledge cursor falls back
/// once to a full snapshot; redirects never receive the bearer token.
async fn download_plan(http: &reqwest::Client, endpoint: &str, token: &str, plan: &str, knowledge: Option<i64>) -> Result<(Value, bool), String> {
    let url = format!("{endpoint}/{plan}");
    let mut request = http.get(&url).bearer_auth(token);
    if let Some(knowledge) = knowledge { request = request.query(&[("last_knowledge_of_server", knowledge)]); }
    let mut response = request.send().await.map_err(|_| t!("Could not reach YNAB. Check your connection and retry."))?;
    let mut incremental = knowledge.is_some();
    if response.status().as_u16() == 400 && incremental {
        progress(tr("Refreshing the full YNAB snapshot…"));
        incremental = false;
        response = http.get(&url).bearer_auth(token).send().await.map_err(|_| t!("Could not refresh the YNAB snapshot. Retry when connected."))?;
    }
    if !response.status().is_success() { return Err(refusal(response.status().as_u16())); }
    let value = response.json().await.map_err(|_| t!("YNAB returned invalid data. Your local budget is unchanged."))?;
    Ok((value, incremental))
}

/// Lists the plans a token can read, so the import form can offer them by name.
pub async fn ynab_plans(token: String, saved_token: Option<String>) -> Result<Vec<PlanChoice>, String> {
    let token = token_for(token, saved_token.as_deref())?;
    let response = client()?
        .get("https://api.ynab.com/v1/plans")
        .bearer_auth(token.trim())
        .send()
        .await
        .map_err(|_| t!("Could not reach YNAB. Check your connection and retry."))?;
    drop(token);
    let plans = match response.status().as_u16() {
        404 => vec![],
        n if !response.status().is_success() => return Err(refusal(n)),
        _ => plan_choices(
            &response
                .json::<Value>()
                .await
                .map_err(|_| t!("YNAB returned invalid data. Try again."))?,
        ),
    };
    if plans.is_empty() {
        return Err(t!("This YNAB account has no plans to import."));
    }
    Ok(plans)
}

#[allow(clippy::too_many_arguments)]
pub async fn sync_ynab(
    db: &Database,
    pending_import: &PendingImport,
    token: String,
    plan_id: String,
    expected_token: String,
    replace: bool,
    remember_token: Option<bool>,
    token_label: Option<String>,
    saved_token: Option<String>,
) -> Result<ImportResult, String> {
    let started = Instant::now();
    let plan_id = plan_id.trim().to_owned();
    if plan_id.is_empty()
        || !plan_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(t!("Choose a plan to import."));
    }
    let token = token_for(token, saved_token.as_deref())?;
    let local = {
        let mut store = db.store()?;
        let s = store.load()?;
        if s.token != expected_token {
            return Err(t!("CONFLICT: Database changed. Fetch it before importing."));
        }
        s.budget
    };
    let same = plan_id == "last-used"
        || local.extra.get("ynabPlanId").and_then(Value::as_str) == Some(&plan_id);
    let baseline = if same {
        local.extra.get("sourceExport").cloned()
    } else {
        None
    };
    let mut knowledge = baseline
        .as_ref()
        .and_then(|v| v.pointer("/data/server_knowledge"))
        .and_then(Value::as_i64);
    // Resolve last-used to the original ID when requesting a delta.
    let requested = if knowledge.is_some() {
        local
            .extra
            .get("ynabPlanId")
            .and_then(Value::as_str)
            .unwrap_or(&plan_id)
    } else {
        &plan_id
    };
    progress(
        if knowledge.is_some() {
            tr("Downloading changes from YNAB…")
        } else {
            tr("Downloading your YNAB plan…")
        },
    );
    let (downloaded, used_delta) = download_plan(&client()?, "https://api.ynab.com/v1/plans", token.trim(), requested, knowledge).await?;
    if !used_delta { knowledge = None; }
    let label = token_label.as_deref().unwrap_or("");
    let remembered = match saved_token.as_deref() {
        None if remember_token == Some(true) => Some(crate::credentials::save(token.trim(), label)?),
        // A saved token keeps the name of the plans it reads; renaming it can't stop the import.
        Some(id) => {
            let _ = crate::credentials::relabel(id, label);
            None
        }
        None => None,
    };
    drop(token);
    {
        progress(tr("Validating balances and merging changes…"));
        let incremental = knowledge.is_some();
        let source = if let Some(base) = baseline.as_ref().filter(|_| incremental) {
            import_sync::merge_delta(base, &downloaded)?
        } else {
            downloaded
        };
        let unchanged = baseline.as_ref().is_some_and(|b| {
            b.pointer("/data/plan")
                .or_else(|| b.pointer("/data/budget"))
                == source
                    .pointer("/data/plan")
                    .or_else(|| source.pointer("/data/budget"))
        });
        let state = db;
        let mut result = if unchanged && !replace {
            let mut store = state.store()?;
            let snapshot = store.load()?;
            if snapshot.token != expected_token {
                return Err(t!("CONFLICT: Database changed during import. Retry after fetching."));
            }
            ImportResult { unchanged: true, ..ImportResult::of(snapshot, started, incremental) }
        } else {
            let incoming = ynab::import_unverified(&source)?;
            let differences = ynab::differences(&incoming);
            // The same differences accepted at the last import don't need asking about again.
            let accepted = local.extra.get("ynabPlanId") == incoming.extra.get("ynabPlanId")
                && local.extra.get("importAudit").and_then(|a| a.get("acceptedDifferences"))
                    == Some(&json!(differences));
            if differences.is_empty() || accepted {
                finish(db, pending_import, local, incoming, &source, replace, incremental, &expected_token, started, &differences)?
            } else {
                let mut store = state.store()?;
                let snapshot = store.load()?;
                if snapshot.token != expected_token {
                    return Err(t!("CONFLICT: Database changed during import. Fetch and retry."));
                }
                let id = review_id();
                *pending_import.0.lock().map_err(|e| e.to_string())? = Some(Pending {
                    id: id.clone(),
                    budget_id: store.id().to_owned(),
                    local,
                    review: Review::Differences { incoming, source, replace, differences: differences.clone() },
                    expected_token,
                    incremental,
                });
                ImportResult { review_id: Some(id), differences, ..ImportResult::of(snapshot, started, incremental) }
            }
        };
        result.saved_token = remembered;
        Ok(result)
    }
}

/// Merges a verified (or accepted) import into the budget and saves it, keeping YNAB's export
/// beside it; competing changes stop for a review instead.
#[allow(clippy::too_many_arguments)]
fn finish(
    db: &Database,
    pending_import: &PendingImport,
    local: Budget,
    mut incoming: Budget,
    source: &Value,
    replace: bool,
    incremental: bool,
    expected_token: &str,
    started: Instant,
    accepted: &[Value],
) -> Result<ImportResult, String> {
    if !accepted.is_empty() {
        if let Some(audit) = incoming.extra.get_mut("importAudit").and_then(Value::as_object_mut) {
            audit.insert("acceptedDifferences".into(), json!(accepted));
        }
    }
    let state = db;
    let budget: Budget = if replace {
        import_sync::keep_name(&local, incoming)
    } else {
        let (merged, conflicts) = import_sync::merge_review(&local, incoming.clone(), BTreeMap::new())?;
        if !conflicts.is_empty() {
            let id = review_id();
            let mut store = state.store()?;
            let snapshot = store.load()?;
            if snapshot.token != expected_token {
                return Err(t!("CONFLICT: Database changed during import. Fetch and retry."));
            }
            *pending_import.0.lock().map_err(|e| e.to_string())? = Some(Pending {
                id: id.clone(),
                budget_id: store.id().to_owned(),
                local,
                review: Review::Conflicts { remote: incoming },
                expected_token: expected_token.to_owned(),
                incremental,
            });
            return Ok(ImportResult { review_id: Some(id), conflicts, ..ImportResult::of(snapshot, started, incremental) });
        }
        merged
    };
    let directory = state.store()?.folder().join("imports");
    std::fs::create_dir_all(&directory).map_err(|_| t!("Cannot create import directory"))?;
    let path = directory.join(format!(
        "ynab-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S-%f")
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| t!("Cannot preserve the source export"))?;
    file.write_all(&serde_json::to_vec(source).map_err(|_| t!("Cannot encode source"))?)
        .map_err(|_| t!("Cannot write source export"))?;
    file.sync_all().map_err(|_| t!("Cannot flush source export"))?;
    progress(tr("Backing up and saving your budget…"));
    let mut store = state.store()?;
    if store.status()?.token != expected_token {
        return Err(t!("CONFLICT: Database changed during import. Nothing was overwritten."));
    }
    let backup = store.backup()?;
    store.save(&budget, expected_token)?;
    let snapshot = store.load()?;
    Ok(ImportResult { backup: Some(backup), ..ImportResult::of(snapshot, started, incremental) })
}

/// The pending review with this id, if it's still the one waiting and the budget hasn't changed.
fn pending(db: &Database, pending_import: &PendingImport, review_id: &str, expected_token: &str) -> Result<Pending, String> {
    let pending = pending_import
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| t!("The import review expired. Import again."))?;
    if pending.id != review_id {
        return Err(t!("The import review was replaced. Import again."));
    }
    if pending.expected_token != expected_token {
        return Err(t!("CONFLICT: The budget changed since this review. Import again to review the latest changes."));
    }
    if db.store()?.id() != pending.budget_id {
        return Err(t!("This import review belongs to another budget. Import again."));
    }
    Ok(pending)
}

/// Saves an import whose totals differ from YNAB's, as the person chose to after seeing them.
pub async fn accept_ynab_import(
    db: &Database,
    pending_import: &PendingImport,
    review_id: String,
    expected_token: String,
) -> Result<ImportResult, String> {
    {
        let started = Instant::now();
        let pending = pending(db, pending_import, &review_id, &expected_token)?;
        let Review::Differences { incoming, source, replace, differences } = pending.review else {
            return Err(t!("This import is waiting on a choice for each competing change."));
        };
        let result = finish(db, pending_import, pending.local, incoming, &source, replace, pending.incremental, &expected_token, started, &differences)?;
        // A save ends the review; competing changes found while merging replace it with theirs.
        if result.review_id.is_none() {
            *pending_import.0.lock().map_err(|e| e.to_string())? = None;
        }
        Ok(result)
    }
}

pub async fn resolve_ynab(
    db: &Database,
    pending_import: &PendingImport,
    review_id: String,
    choices: BTreeMap<String, String>,
    expected_token: String,
) -> Result<ImportResult, String> {
    {
        let started = Instant::now();
        let pending = pending(db, pending_import, &review_id, &expected_token)?;
        let Review::Conflicts { remote } = pending.review else {
            return Err(t!("This import is waiting for its differences from YNAB to be accepted."));
        };
        let (budget, conflicts) = import_sync::merge_review(&pending.local, remote, choices)?;
        if !conflicts.is_empty() {
            return Err(t!("Choose Local or YNAB for every conflict."));
        }
        let state = db;
        let mut store = state.store()?;
        if store.status()?.token != expected_token {
            return Err(t!("CONFLICT: Database changed before import. Nothing was overwritten."));
        }
        let backup = store.backup()?;
        store.save(&budget, &expected_token)?;
        let snapshot = store.load()?;
        *pending_import.0.lock().map_err(|e| e.to_string())? = None;
        Ok(ImportResult { backup: Some(backup), ..ImportResult::of(snapshot, started, pending.incremental) })
    }
}

pub async fn dispatch(method: &str, p: Value, db: &Database, pending_import: &PendingImport) -> Result<Value, String> {
    let string = |key: &str| p[key].as_str().unwrap_or_default().to_owned();
    let optional = |key: &str| p[key].as_str().map(str::to_owned);
    // Use the current committed token unless the client intentionally supplies one.
    let expected = || -> Result<String, String> { match optional("expectedToken") { Some(value) => Ok(value), None => Ok(db.store()?.status()?.token) } };
    match method {
        "ynab_tokens" => serde_json::to_value(crate::credentials::ynab_tokens()?).map_err(|e| e.to_string()),
        "forget_ynab_token" => { crate::credentials::forget_ynab_token(string("id"))?; Ok(Value::Null) },
        "ynab_plans" => serde_json::to_value(ynab_plans(string("token"), optional("savedToken")).await?).map_err(|e| e.to_string()),
        "sync_ynab" => serde_json::to_value(sync_ynab(db, pending_import, string("token"), string("planId"), expected()?, p["replace"].as_bool().unwrap_or(false), p["rememberToken"].as_bool(), optional("tokenLabel"), optional("savedToken")).await?).map_err(|e| e.to_string()),
        "accept_ynab_import" => serde_json::to_value(accept_ynab_import(db, pending_import, string("reviewId"), expected()?).await?).map_err(|e| e.to_string()),
        "resolve_ynab" => {
            let choices = serde_json::from_value(p["choices"].clone()).map_err(|e| format!("Invalid import conflict choices: {e}"))?;
            serde_json::to_value(resolve_ynab(db, pending_import, string("reviewId"), choices, expected()?).await?).map_err(|e| e.to_string())
        },
        _ => Err(format!("Unknown YNAB command: {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authenticated_delta_falls_back_once_without_losing_the_token() {
        use std::io::{BufRead, BufReader, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            for index in 0..2 {
                let (mut stream, _) = server.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut path = String::new(); reader.read_line(&mut path).unwrap();
                assert!(path.starts_with("GET /plans/home"));
                assert_eq!(path.contains("last_knowledge_of_server=17"), index == 0);
                let mut auth = false;
                loop { let mut header = String::new(); reader.read_line(&mut header).unwrap(); if header == "\r\n" { break; } auth |= header.trim().eq_ignore_ascii_case("authorization: Bearer synthetic-test-token"); }
                assert!(auth);
                let (status, body) = if index == 0 { (400, "{}") } else { (200, r#"{"data":{"server_knowledge":18,"plan":{"id":"home"}}}"#) };
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        let (snapshot, incremental) = crate::block_on(download_plan(&client().unwrap(), &format!("http://{address}/plans"), "synthetic-test-token", "home", Some(17))).unwrap();
        worker.join().unwrap();
        assert!(!incremental);
        assert_eq!(snapshot["data"]["server_knowledge"], 18);
    }

    #[test]
    fn rejected_token_returns_an_actionable_error_without_credential_contents() {
        use std::io::{Read, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap(); let mut bytes = [0; 4096]; stream.read(&mut bytes).unwrap();
            write!(stream, "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let error = crate::block_on(download_plan(&client().unwrap(), &format!("http://{address}"), "synthetic-secret", "home", None)).unwrap_err();
        worker.join().unwrap();
        assert!(error.contains("rejected the token")); assert!(!error.contains("synthetic-secret"));
    }

    #[test]
    fn plans_are_listed_newest_first_with_the_default_on_top() {
        let listing = json!({ "data": {
            "plans": [
                { "id": "a", "name": "Home", "last_modified_on": "2026-01-02T10:00:00.000Z" },
                { "id": "b", "name": "Work", "last_modified_on": "2026-10-19T08:00:00.000Z" },
                { "id": "c", "name": "Old", "last_modified_on": null },
                { "name": "No id" },
            ],
            "default_plan": null,
        }});
        let ids = |l: &Value| plan_choices(l).into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&listing), ["b", "a", "c"]);
        let mut with_default = listing.clone();
        with_default["data"]["default_plan"] = json!({ "id": "a", "name": "Home" });
        assert_eq!(ids(&with_default), ["a", "b", "c"]);
        let older = json!({ "data": { "budgets": [{ "id": "x", "name": "Budget" }] } });
        assert_eq!(plan_choices(&older), [PlanChoice { id: "x".into(), name: "Budget".into(), last_modified_on: None }]);
    }
}
