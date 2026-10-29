//! Host-neutral library and editor commands. UI adapters must not invent lifecycle semantics.
use crate::{store::Store, Database, Library};
use serde_json::{json, Value};

pub fn empty_snapshot() -> Value {
    json!({"budget":null,"token":"","savedAt":"","databasePath":"","isNew":true,
        "undoAvailable":false,"redoAvailable":false})
}
pub(crate) fn process_owner() -> String {
    static OWNER: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    OWNER
        .get_or_init(|| format!("{}-{}", std::process::id(), crate::budgets::new_id()))
        .clone()
}

/// Repair only jobs whose OS lease has been released (including process death). A live
/// creator's empty database is never mistaken for an abandoned financial budget.
pub fn repair_local_catalog(catalog: &mut crate::budgets::Budgets) -> Result<(), String> {
    catalog.repair_local_jobs()?;
    for job in catalog.jobs()? {
        if !matches!(job.kind.as_str(), "initialize" | "download") {
            continue;
        }
        let Some(_lease) = catalog.claim_abandoned_local_job(&job.operation_id)? else {
            continue;
        };
        let Some(id) = job.local_budget_id.as_deref() else {
            continue;
        };
        let repaired = (|| -> Result<(), String> {
            let path = catalog.database(id)?;
            let saved = if path.is_file() {
                let mut store = Store::open_budget(&path, id)?;
                let status = store.status()?;
                if !status.saved_at.is_empty() {
                    crate::domain::validate(&store.saved()?.0)?;
                    job.kind == "initialize" || store.sync_binding()?.is_some()
                } else {
                    false
                }
            } else {
                false
            };
            if saved {
                catalog.mark_available(id)?;
            } else if job.kind == "initialize" {
                catalog.quarantine_binding(
                    id,
                    &json!({"reason":"interrupted_initialization","operationId":job.operation_id}),
                )?;
                catalog.finish_job(
                    &job.operation_id,
                    &json!({"outcome":"preserved","code":"interrupted_initialization"}),
                )?;
            }
            // An interrupted download remains remote-only and is fetched again by the coordinator.
            Ok(())
        })();
        if let Err(error) = repaired {
            catalog.quarantine_binding(id,&json!({"reason":"local_job_repair_failed","operationId":job.operation_id,"error":error}))?;
            catalog.set_entry_status(id, &format!("Local recovery needed: {error}"))?;
            catalog.finish_job(
                &job.operation_id,
                &json!({"outcome":"preserved","code":"local_job_repair_failed"}),
            )?;
        }
    }
    Ok(())
}

/// Lock order is library then Store. No guard escapes into an HTTP request.
pub(crate) fn with_store<T>(
    db: &Database,
    library: &Library,
    id: &str,
    f: impl FnOnce(&mut Store) -> Result<T, String>,
) -> Result<T, String> {
    let library = library.lock();
    if let Ok(mut store) = db.store() {
        if store.id() == id {
            return f(&mut store);
        }
    }
    let path = library.database(id)?;
    if !path.exists() {
        return Err("Download this budget before opening it.".into());
    }
    let mut store = Store::open_budget(&path, id)?.with_backup_preferences(library.data_dir());
    f(&mut store)
}

pub fn checkpoint(
    db: &Database,
    library: &Library,
    budget_id: &str,
    id: String,
    epoch: String,
    target: Value,
    baseline: Value,
    values: Value,
) -> Result<(), String> {
    // This endpoint is for financial editors only. Credential and account forms never call it.
    let raw = serde_json::to_vec(&json!({"values":values,"baseline":baseline,"target":target}))
        .map_err(|e| e.to_string())?;
    if raw.len() > 16 * 1024 * 1024 {
        return Err("This editor draft is too large to checkpoint.".into());
    }
    with_store(db, library, budget_id, |s| {
        s.checkpoint_editor(&urbudget_core::sync_storage::EditorCheckpoint {
            editor_id: id,
            epoch,
            process_owner: process_owner(),
            target,
            baseline,
            draft: values,
        })
    })
}
pub fn discard_checkpoint(
    db: &Database,
    library: &Library,
    budget_id: &str,
    id: &str,
    epoch: &str,
) -> Result<(), String> {
    with_store(db, library, budget_id, |s| {
        s.clear_editor_checkpoint(id, epoch)
    })
}
/// Normal editor unmount discards only the active, live editor. Remote trash/switch keeps recovery.
pub fn discard_checkpoint_if_active(db: &Database, library: &Library, budget_id: &str, id: &str, epoch: &str) -> Result<(), String> {
    let catalog = library.lock();
    if !catalog.try_list_all()?.iter().any(|e| e.id == budget_id && e.lifecycle == "active") { return Ok(()); }
    if let Ok(store) = db.store() {
        if store.id() == budget_id { store.clear_editor_checkpoint(id, epoch)?; }
    }
    Ok(())
}
pub fn editor_recovery(db: &Database, library: &Library, budget_id: &str) -> Result<Value, String> {
    if budget_id.is_empty() {
        return Ok(json!([]));
    }
    let mut result = with_store(db, library, budget_id, |s| {
        serde_json::to_value(s.editor_checkpoints()?).map_err(|e| e.to_string())
    })?;
    for entry in result.as_array_mut().ok_or("Invalid checkpoint list.")? {
        entry["budgetId"] = json!(budget_id);
    }
    Ok(result)
}

pub fn open_local(db: &Database, library: &Library, id: &str) -> Result<Value, String> {
    let mut library = library.lock();
    if db.store().is_ok_and(|s| s.has_unsaved_draft()) {
        return Err("Save or discard your changes before opening another budget.".into());
    }
    let path = library.database(id)?;
    if !path.exists() {
        return Err("Download this budget before opening it.".into());
    }
    let mut store = Store::open_budget(&path, id)?.with_backup_preferences(library.data_dir());
    let snapshot = store.load_light()?;
    library.set_current(id)?;
    db.replace(Some(store));
    Ok(
        json!({"budget":snapshot.budget,"token":snapshot.token,"savedAt":snapshot.saved_at,
        "databasePath":snapshot.database_path,"isNew":snapshot.saved_at.is_empty(),"undoAvailable":false,"redoAvailable":false}),
    )
}

pub fn snapshot(db: &Database, library: &Library) -> Result<Value, String> {
    let library = library.lock();
    let active = db.store().ok().map(|s| s.id().to_owned());
    Ok(json!({"version":library.version()?,"firstRun":library.first_run()?,"activeId":active,"entries":library.try_list_all()?}))
}
fn active_snapshot(db: &Database) -> Result<Value, String> {
    if !db.is_open() {
        return Ok(Value::Null);
    }
    let mut store = db.store()?;
    let budget = store.snapshot_draft(None)?;
    let status = store.status()?;
    Ok(json!({"budget":budget,"token":status.token,"savedAt":status.saved_at,"databasePath":status.database_path,"isNew":status.saved_at.is_empty(),"undoAvailable":false,"redoAvailable":false}))
}

pub async fn lifecycle(
    cloud: &crate::cloud::Cloud,
    db: &Database,
    library: &Library,
    kind: &str,
    id: &str,
) -> Result<Value, String> {
    let entry = library
        .lock()
        .try_list_all()?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("That budget no longer exists.")?;
    let mut recovered = None;
    match kind {
        "trash" => {
            // Logical deletion is durable before editor invalidation or file cleanup.
            library.lock().remove(id)?;
            if !entry.missing {
                with_store(db, library, id, |s| {
                    if s.has_unsaved_draft() {
                        let draft =
                            serde_json::to_value(s.full_draft(None)?).map_err(|e| e.to_string())?;
                        s.checkpoint_editor(&urbudget_core::sync_storage::EditorCheckpoint {
                            editor_id: format!("closed-budget-draft-{}", process_owner()),
                            epoch: s.status()?.token,
                            process_owner: process_owner(),
                            target: json!({"kind":"budget"}),
                            baseline: json!({}),
                            draft,
                        })?;
                    }
                    s.set_sync_blocked(true)
                })?;
            }
            if db.store().is_ok_and(|s| s.id() == id) {
                db.replace(None);
            }
        }
        "restore" => {
            if let Some(account) = entry.account_id.as_deref() {
                if cloud.selected_account().as_deref() != Some(account) {
                    return Err("Sign in to the account that owns this budget.".into());
                }
                let job = crate::budgets::LifecycleJob {
                    operation_id: crate::budgets::new_id(),
                    local_budget_id: Some(id.into()),
                    kind: "restore".into(),
                    payload: json!({"accountId":account}),
                    state: "pending".into(),
                    receipt: None,
                };
                library.lock().enqueue_job(&job)?;
            } else {
                if !entry.missing {
                    with_store(db, library, id, |s| s.set_sync_blocked(false))?;
                }
                library.lock().restore_local(id)?;
            }
        }
        "evict" => {
            if db.store().is_ok_and(|s| s.id() == id) {
                with_store(db, library, id, |s| {
                    if s.sync_dirty()?
                        || s.has_unsaved_draft()
                        || s.read_recovery()?.is_some()
                        || !s.editor_checkpoints()?.is_empty()
                    {
                        Err("Save, sync and resolve recovery before removing this download.".into())
                    } else {
                        Ok(())
                    }
                })?;
                library.lock().clear_current()?;
                db.replace(None);
            }
            library.lock().evict(id)?;
        }
        "recover" => {
            let (mut budget, mut checkpoints, recovery) = with_store(db, library, id, |s| {
                Ok((s.saved()?.0, s.editor_checkpoints()?, s.read_recovery()?))
            })?;
            if let Some(recovery) = recovery {
                checkpoints.push(urbudget_core::sync_storage::EditorCheckpoint {
                    editor_id: "recovery-file".into(),
                    epoch: crate::budgets::new_id(),
                    process_owner: process_owner(),
                    target: json!({"kind":"budget"}),
                    baseline: json!({}),
                    draft: recovery,
                });
            }
            budget.name = format!("{} (Recovered)", budget.name);
            let mut catalog = library.lock();
            let new_id = catalog.create()?;
            let path = catalog.database(&new_id)?;
            let mut store =
                Store::open_budget(&path, &new_id)?.with_backup_preferences(catalog.data_dir());
            let token = store.status()?.token;
            store.save(&budget, &token)?;
            for checkpoint in checkpoints {
                store.checkpoint_editor(&checkpoint)?;
            }
            catalog.mark_available(&new_id)?;
            recovered = Some(new_id);
        }
        _ => return Err("Unknown budget lifecycle action.".into()),
    }
    Ok(
        json!({"library":snapshot(db,library)?,"active":active_snapshot(db)?,"recoveredId":recovered}),
    )
}

#[cfg(test)]
mod responsiveness_tests {
    use super::*;
    #[test]
    fn cached_open_and_local_lifecycle_do_not_wait_for_a_network_operation() {
        let folder=tempfile::tempdir().unwrap();let runtime=crate::runtime::Runtime::new(folder.path()).unwrap();
        crate::block_on(async {
            let id=runtime.dispatch("create_budget",json!({"budget":urbudget_core::test_support::sample()})).await.unwrap();
            let _network=runtime.cloud.sync.operation().await.unwrap();
            tokio::time::timeout(std::time::Duration::from_millis(500),runtime.dispatch("library_open",json!({"id":id}))).await.expect("cached open waited for network lock").unwrap();
            tokio::time::timeout(std::time::Duration::from_millis(500),runtime.dispatch("library_trash",json!({"id":id}))).await.expect("local trash waited for network lock").unwrap();
        });
    }
}
