//! Native host command router. SQLite and the service state live for the process lifetime so
//! sync, bank imports and assistant edits all observe the same saved revision and draft.
use crate::{assistant, budgets, cloud, domain, fresh_start, host, simplefin, store::Store, t, vault, ynab_capture, Database, Library};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::collections::HashMap;

#[derive(Default)]
struct History { token: String, undo: Vec<crate::patch::Patch>, redo: Vec<crate::patch::Patch>, proposal: Option<(String, String)> }

pub struct Runtime {
    pub db: Database,
    pub library: Library,
    pub cloud: cloud::Cloud,
    pub assistant: assistant::Assistant,
    pub simplefin: simplefin::SimpleFin,
    pub pending_import: ynab_capture::PendingImport,
    pub data_dir: PathBuf,
    history: Mutex<HashMap<String, History>>,
    commands: tokio::sync::Mutex<()>,
    bank_commands: tokio::sync::Mutex<()>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(runtime: &Runtime, method: &str, p: Value) -> Result<Value, String> { crate::block_on(runtime.dispatch(method, p)) }

    #[test]
    fn native_runtime_commits_reopens_undoes_and_rejects_stale_writers() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let initial = call(&runtime, "load_budget", json!({})).unwrap();
        assert_eq!(initial["isNew"], true);
        let automatic = call(&runtime, "apply_due_schedules", json!({"through":"2026-10-21"})).unwrap();
        assert_eq!(automatic["isNew"], true, "foreground upkeep must not turn the unconfigured database into a user budget");
        assert_eq!(automatic["token"], initial["token"]);
        let mut budget = urbudget_core::test_support::sample();
        budget.name = "Native first".into();
        let first = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":initial["token"]})).unwrap();
        assert_eq!(first["isNew"], false);
        let status = call(&runtime, "native_budget_status", json!({})).unwrap();
        assert_eq!(status["token"], first["token"]);
        assert!(status.get("budget").is_none(), "unchanged refresh probes must not serialize the ledger");
        budget.name = "Native second".into();
        let second = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":first["token"],"ackOnly":true})).unwrap();
        assert!(second.get("budget").is_none(), "native writes already own their committed snapshot");
        assert_eq!(second["undoAvailable"], true);
        assert!(call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":first["token"]})).unwrap_err().contains("CONFLICT"));
        assert!(call(&runtime, "undo_budget", json!({"expectedToken":first["token"]})).unwrap_err().contains("CONFLICT"));
        let undone = call(&runtime, "undo_budget", json!({"expectedToken":second["token"]})).unwrap();
        assert_eq!(undone["budget"]["name"], "Native first");
        assert_eq!(undone["redoAvailable"], true);
        let redone = call(&runtime, "redo_budget", json!({"expectedToken":undone["token"]})).unwrap();
        assert_eq!(redone["budget"]["name"], "Native second");
        drop(runtime);
        let reopened = Runtime::new(folder.path()).unwrap();
        assert_eq!(call(&reopened, "load_budget", json!({})).unwrap()["budget"]["name"], "Native second");
        let report = crate::block_on(async {
            let _editing = reopened.commands.lock().await;
            tokio::time::timeout(std::time::Duration::from_secs(2), reopened.dispatch("sync_now", json!({}))).await.expect("sync must not wait for the UI command queue").unwrap()
        });
        assert_eq!(report["state"], "signed_out");
        assert!(reopened.db.store().is_ok(), "sync must release the database lock before awaiting");
    }

    #[test]
    fn native_library_tokens_cannot_commit_into_another_budget() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let initial = runtime.load().unwrap();
        let budget = urbudget_core::test_support::sample();
        let second = call(&runtime, "create_budget", json!({"budget":budget})).unwrap();
        call(&runtime, "open_budget", json!({"id":second})).unwrap();
        assert!(call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":initial["token"]})).unwrap_err().contains("CONFLICT"));
        assert_eq!(call(&runtime, "list_budgets", json!({})).unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn compact_save_ack_cannot_adopt_a_sync_revision_before_the_client_caches_it() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let initial = runtime.load().unwrap();
        let mut local = urbudget_core::test_support::sample();
        local.name = "Local commit".into();
        let acknowledgement = runtime.save(&local, initial["token"].as_str().unwrap()).unwrap();
        // Sync finishes after the write's lock is released but before Kotlin receives/caches
        // its acknowledgement. The ack must still describe only the local JSON it submitted.
        let mut remote = local.clone();
        remote.name = "Arrived from sync".into();
        {
            let mut store = runtime.db.store().unwrap();
            let token = store.status().unwrap().token;
            store.save_synced(&remote, &token, 7, &remote).unwrap();
        }
        let probe = runtime.native_status().unwrap();
        assert_ne!(probe["token"], acknowledgement["token"]);
        local.name = "Stale follow-up edit".into();
        let rejected = call(&runtime, "save_budget", json!({"budget":local,"expectedToken":acknowledgement["token"],"ackOnly":true})).unwrap_err();
        assert!(rejected.contains("CONFLICT"));
        let loaded = runtime.load().unwrap();
        assert_eq!(loaded["token"], probe["token"]);
        assert_eq!(loaded["budget"]["name"], "Arrived from sync");
    }

    #[test]
    fn failed_native_commit_cannot_leak_a_hidden_draft_into_a_later_patch() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let first = runtime.load().unwrap();
        let original = urbudget_core::test_support::sample();
        runtime.save(&original, first["token"].as_str().unwrap()).unwrap();
        let path = runtime.db.store().unwrap().status().unwrap().database_path;
        let external = rusqlite::Connection::open(path).unwrap();
        external.execute_batch("CREATE TRIGGER reject_native_save BEFORE UPDATE ON metadata BEGIN SELECT RAISE(ABORT, 'Simulated disk failure'); END;").unwrap();
        let expected = runtime.native_status().unwrap()["token"].as_str().unwrap().to_owned();
        let mut rejected = original.clone();
        rejected.name = "Rejected edit".into();
        rejected.extra.insert("failedEditMustNotLeak".into(), json!(true));
        assert!(runtime.save(&rejected, &expected).unwrap_err().contains("Simulated disk failure"));
        {
            let mut store = runtime.db.store().unwrap();
            assert!(!store.has_unsaved_draft());
            assert_eq!(store.full_draft(None).unwrap(), original);
            assert!(store.read_recovery().unwrap().is_some(), "failed edit remains explicitly recoverable");
        }
        external.execute_batch("DROP TRIGGER reject_native_save;").unwrap();
        let expected = runtime.native_status().unwrap()["token"].as_str().unwrap().to_owned();
        let mut later = original.clone();
        later.name = "Accepted patch".into();
        let patch = crate::patch::Patch::diff(&original, &later);
        let saved = call(&runtime, "native_apply_patch", json!({"patch":patch,"expectedToken":expected})).unwrap();
        assert_eq!(saved["budget"]["name"], "Accepted patch");
        assert!(saved["budget"].get("failedEditMustNotLeak").is_none());
    }

    #[test]
    fn service_probes_and_local_edits_do_not_wait_behind_a_bank_download() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let initial = runtime.load().unwrap();
        crate::block_on(async {
            let _bank_network = runtime.bank_commands.lock().await;
            {
                let _commit_queue = runtime.commands.lock().await;
                for method in ["native_budget_status", "cloud_status", "simplefin_status", "ynab_tokens", "list_budgets", "list_backups", "backup_retention"] {
                    tokio::time::timeout(std::time::Duration::from_secs(1), runtime.dispatch(method, json!({})))
                        .await.unwrap_or_else(|_| panic!("{method} waited for the command queue")).unwrap();
                }
                // English is already the language: this checks the call, not a switch other tests could see.
                let switched = tokio::time::timeout(std::time::Duration::from_secs(1), runtime.dispatch("set_language", json!({"language":"en"})))
                    .await.expect("set_language waited for the command queue").unwrap();
                assert_eq!(switched, Value::Null);
            }
            let mut budget = urbudget_core::test_support::sample();
            budget.name = "Edited while the bank is waiting".into();
            let saved = tokio::time::timeout(std::time::Duration::from_secs(1), runtime.dispatch("save_budget", json!({"budget":budget,"expectedToken":initial["token"],"ackOnly":true})))
                .await.expect("bank work cannot hold the local edit queue").unwrap();
            assert_ne!(saved["token"], initial["token"]);
        });
        let refused = call(&runtime, "simplefin_import", json!({"account":"cash","through":"2026-10-21","choices":[],"expectedToken":initial["token"]})).unwrap_err();
        assert!(refused.contains("CONFLICT"), "bank imports must retain the reviewed revision");
    }

    #[test]
    fn assignment_and_schedule_extensions_survive_native_save_reopen_and_scheduled_posting() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let initial = runtime.load().unwrap();
        let mut budget = serde_json::to_value(urbudget_core::test_support::sample()).unwrap();
        budget["assignments"][0]["customNote"] = json!({"legacyId":"a-17"});
        budget["schedules"] = json!([{
            "id":"planned","nextDate":"2026-10-21","anchorDate":"2026-10-21","frequency":"monthly","paused":false,
            "customSchedule":{"origin":"desktop"},
            "transaction":{"id":"template","date":"2026-10-21","accountId":"cash","payee":"Rent","memo":"","amount":-100,"status":"uncleared","splits":[{"categoryId":"food","amount":-100,"customSplit":"keep"}],"customTransaction":"keep"}
        }]);
        call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":initial["token"]})).unwrap();
        call(&runtime, "apply_due_schedules", json!({"through":"2026-10-21"})).unwrap();
        drop(runtime);
        let reopened = Runtime::new(folder.path()).unwrap();
        let saved = reopened.load().unwrap();
        assert_eq!(saved["budget"]["assignments"][0]["customNote"]["legacyId"], "a-17");
        assert_eq!(saved["budget"]["schedules"][0]["customSchedule"]["origin"], "desktop");
        assert_eq!(saved["budget"]["schedules"][0]["transaction"]["customTransaction"], "keep");
    }
}

fn value<T: serde::Serialize>(v: T) -> Result<Value, String> { serde_json::to_value(v).map_err(|e| e.to_string()) }
fn field<T: DeserializeOwned>(p: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(p.get(key).cloned().ok_or_else(|| format!("Missing {key}."))?).map_err(|e| format!("Invalid {key}: {e}"))
}
fn optional<T: DeserializeOwned>(p: &Value, key: &str) -> Result<Option<T>, String> {
    match p.get(key).filter(|v| !v.is_null()) { Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| format!("Invalid {key}: {e}")), None => Ok(None) }
}

impl Runtime {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        vault::init(data_dir);
        let library = budgets::Budgets::load(data_dir)?;
        let id = library.current().to_owned();
        let store = Store::open_budget(&library.database(&id)?, &id)?.with_backup_preferences(data_dir);
        Ok(Self {
            db: Database(Mutex::new(store)), library: Library(Mutex::new(library)),
            cloud: cloud::Cloud::new(data_dir), assistant: assistant::Assistant::new(data_dir.to_path_buf()),
            simplefin: simplefin::SimpleFin::new(data_dir), pending_import: ynab_capture::PendingImport::default(),
            data_dir: data_dir.to_path_buf(),
            history: Mutex::default(),
            commands: tokio::sync::Mutex::new(()),
            bank_commands: tokio::sync::Mutex::new(()),
        })
    }

    /// A complete snapshot for native clients. Unlike the desktop IPC light serializer this
    /// carries import metadata as well, for lossless existing-Android JSON migration/export.
    pub fn load(&self) -> Result<Value, String> {
        let mut store = self.db.store()?;
        let snapshot = store.load()?;
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != snapshot.token { *history = History { token: snapshot.token.clone(), ..History::default() }; }
        Ok(json!({"budget":snapshot.budget,"token":snapshot.token,"savedAt":snapshot.saved_at,"databasePath":snapshot.database_path,
            "isNew":snapshot.saved_at.is_empty(),"undoAvailable":!history.undo.is_empty(),"redoAvailable":!history.redo.is_empty()}))
    }

    /// Cheap native refresh probe: a stable revision needs no ledger read or JSON round trip.
    fn native_status(&self) -> Result<Value, String> {
        let store = self.db.store()?;
        let status = store.status()?;
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != status.token { *history = History { token: status.token.clone(), ..History::default() }; }
        Ok(json!({"token":status.token,"savedAt":status.saved_at,"databasePath":status.database_path,
            "isNew":status.saved_at.is_empty(),"undoAvailable":!history.undo.is_empty(),"redoAvailable":!history.redo.is_empty()}))
    }

    fn save(&self, budget: &domain::Budget, expected: &str) -> Result<Value, String> {
        let mut store = self.db.store()?;
        let (before, status) = store.saved()?;
        if status.token != expected { return Err(t!("CONFLICT: The budget changed. Reload it before saving your edit.")); }
        domain::validate(budget)?;
        // Native editors send a complete snapshot, but ordinary edits still use the desktop
        // delta commit path. Large imported source payloads and untouched ledger rows stay in
        // SQLite instead of being deleted and inserted again for every small mobile edit.
        let heavy_changed = crate::store::HEAVY_EXTRAS.iter().any(|key| before.extra.get(*key) != budget.extra.get(*key));
        let saved = if heavy_changed {
            store.save(budget, expected)?
        } else {
            let mut draft = budget.clone();
            for key in crate::store::HEAVY_EXTRAS { draft.extra.remove(*key); }
            let committed = (|| {
                store.load_light()?;
                store.replace_draft(draft)?;
                store.commit(expected)
            })();
            match committed {
                Ok(status) => status,
                Err(error) => {
                    store.invalidate_draft();
                    return Err(error);
                }
            }
        };
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != status.token { *history = History::default(); }
        if &before != budget {
            // Keep inverse changes rather than fifty whole ledgers in a phone's memory.
            history.undo.push(crate::patch::Patch::diff(budget, &before));
            if history.undo.len() > 50 { history.undo.remove(0); }
            history.redo.clear();
            history.proposal = None;
        }
        history.token = saved.token.clone();
        // Return the revision acknowledged by this transaction, while its Store/history locks
        // are still held. A later sync must not pair its newer token with the caller's old JSON.
        Ok(json!({"token":saved.token,"savedAt":saved.saved_at,"databasePath":saved.database_path,
            "isNew":saved.saved_at.is_empty(),"undoAvailable":!history.undo.is_empty(),"redoAvailable":!history.redo.is_empty()}))
    }

    fn undo_redo(&self, redo: bool, expected: Option<String>) -> Result<Value, String> {
        let mut store = self.db.store()?;
        let (before, status) = store.saved()?;
        if expected.as_ref().is_some_and(|token| token != &status.token) { return Err(t!("CONFLICT: The budget changed after that edit. Review the current budget before undoing.")); }
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != status.token { *history = History { token: status.token.clone(), ..History::default() }; }
        let next = if redo { history.redo.last() } else { history.undo.last() }.cloned();
        if let Some(patch) = next {
            let mut next = before.clone();
            patch.apply(&mut next);
            let saved = store.save(&next, &status.token)?;
            let inverse = crate::patch::Patch::diff(&next, &before);
            if redo { history.redo.pop(); history.undo.push(inverse); }
            else { history.undo.pop(); history.redo.push(inverse); }
            history.token = saved.token;
            history.proposal = None;
        }
        drop(histories); drop(store);
        self.load()
    }

    pub async fn dispatch(&self, method: &str, p: Value) -> Result<Value, String> {
        // Preserve command order across JNI worker threads. Polling and stop remain responsive
        // during network work; assistant background jobs only create proposals, never edits.
        // Sync owns its own operation lock and checks Store revisions after every download.
        // Do not hold the UI command queue over that network wait: an offline sync must never
        // make local budgeting wait for the HTTP timeout.
        let independent = method.starts_with("account_") || method.starts_with("billing_") || method.starts_with("sync_") || matches!(method,
            "set_language" | "host_events" | "native_budget_status" | "database_status" | "cloud_status" | "sync_status" | "assistant_stop" | "assistant_cancel_sign_in" | "assistant_status" | "assistant_thread" | "assistant_threads" |
            "assistant_sign_in" | "assistant_sign_out" | "assistant_foreground" | "assistant_settings" | "assistant_set_key" | "assistant_forget_key" | "assistant_models" | "assistant_report_reply" | "assistant_sms_poll" | "assistant_sms_cancel" |
            "simplefin_status" | "simplefin_connect" | "simplefin_accounts" | "simplefin_disconnect" | "simplefin_open" | "simplefin_sync" |
            "ynab_tokens" | "ynab_plans" | "forget_ynab_token" | "list_budgets" | "list_backups" | "backup_retention");
        let _command = if independent {
            None
        } else { Some(self.commands.lock().await) };
        // Credential changes and downloads stay ordered with one another, while a bank's
        // network timeout cannot hold the budget edit queue or block status polling.
        let _bank = if matches!(method, "simplefin_connect" | "simplefin_accounts" | "simplefin_disconnect" | "simplefin_sync") {
            Some(self.bank_commands.lock().await)
        } else { None };
        let text = |key: &str| field::<String>(&p, key);
        match method {
            // The language of the messages the services return from now on ("ar" or "en").
            "set_language" => { crate::i18n::set_language(&text("language")?); Ok(Value::Null) }
            "host_events" => value(host::drain()),
            "load_budget" => self.load(),
            "native_budget_status" => self.native_status(),
            "snapshot_budget" => value(self.db.store()?.full_draft(None)?),
            "database_status" => value(self.db.store()?.status()?),
            "replace_draft" => value(self.db.store()?.replace_draft(field(&p, "budget")?)?),
            "apply_patch" => value(self.db.store()?.apply_patch(field(&p, "patch")?)?),
            "save_budget" => {
                let budget = field(&p, "budget")?;
                let expected = text("expectedToken")?;
                let acknowledged = self.save(&budget, &expected)?;
                if p["ackOnly"].as_bool().unwrap_or(false) { Ok(acknowledged) } else { self.load() }
            }
            "undo_budget" => self.undo_redo(false, optional(&p, "expectedToken")?),
            "redo_budget" => self.undo_redo(true, optional(&p, "expectedToken")?),
            "native_apply_patch" => {
                let mut store = self.db.store()?;
                let patch = field(&p, "patch")?;
                let budget = store.full_draft(Some(patch))?;
                let token = optional::<String>(&p, "expectedToken")?.unwrap_or(store.status()?.token);
                drop(store);
                self.save(&budget, &token)?;
                self.load()
            }
            "assistant_apply_commit" => {
                let token = self.db.store()?.status()?.token;
                let mut result = assistant::dispatch("assistant_apply", p.clone(), &self.db, &self.assistant, &self.simplefin, &self.cloud).await?;
                if !result["patch"].is_null() {
                    let saved = (|| -> Result<(), String> {
                        let patch = serde_json::from_value(result["patch"].clone()).map_err(|e| e.to_string())?;
                        let budget = self.db.store()?.full_draft(Some(patch))?;
                        self.save(&budget, &token).map(|_| ())
                    })();
                    if let Err(error) = saved {
                        let mut reset = p;
                        reset["status"] = json!("proposed");
                        let _ = assistant::dispatch("assistant_mark", reset, &self.db, &self.assistant, &self.simplefin, &self.cloud).await;
                        return Err(error);
                    }
                    let snapshot = self.load()?;
                    let changed = snapshot["token"].as_str() != Some(&token);
                    if changed {
                        let id = self.db.store()?.id().to_owned();
                        if let Some(history) = self.history.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&id) {
                            history.proposal = Some((text("threadId")?, text("proposalId")?));
                        }
                    }
                    result["undoAvailable"] = json!(changed);
                    result["snapshot"] = snapshot;
                }
                Ok(result)
            }
            "assistant_undo_commit" => {
                let thread = text("threadId")?;
                let proposal = text("proposalId")?;
                let expected = text("expectedToken")?;
                let id = self.db.store()?.id().to_owned();
                let permitted = self.history.lock().unwrap_or_else(|e| e.into_inner()).get(&id).is_some_and(|history| {
                    history.token == expected && !history.undo.is_empty() && history.proposal.as_ref() == Some(&(thread.clone(), proposal.clone()))
                });
                if !permitted { return Err(t!("The budget changed after this proposal. Use the budget's undo history to review later changes.")); }
                assistant::verify_undo_proposal(&self.db, &self.assistant, &thread, &proposal)?;
                let snapshot = self.undo_redo(false, Some(expected))?;
                let marked = assistant::dispatch("assistant_mark", json!({"threadId":thread,"proposalId":proposal,"status":"undone"}), &self.db, &self.assistant, &self.simplefin, &self.cloud).await?;
                Ok(json!({"snapshot":snapshot,"proposal":marked}))
            }
            "commit_budget" => value(self.db.store()?.commit(&text("expectedToken")?)?),
            "list_budgets" => value(self.library.lock().list()),
            "fresh_start" => {
                let source = self.db.store()?.full_draft(None)?;
                let next = fresh_start::create(&source, &text("name")?, &text("asOf")?)?;
                let mut library = self.library.lock();
                let id = library.create()?;
                let saved = Store::open_budget(&library.database(&id)?, &id).and_then(|store| {
                    let mut store = store.with_backup_preferences(library.data_dir());
                    let token = store.status()?.token; store.save(&next, &token)
                });
                if let Err(e) = saved { library.forget_new(&id)?; return Err(e); }
                value(id)
            }
            "create_budget" => {
                let budget = field::<domain::Budget>(&p, "budget")?;
                domain::validate(&budget)?;
                let mut library = self.library.lock();
                let budget = if let Some(token) = optional::<String>(&p, "freshStartToken")? {
                    let source = self.db.store()?.fresh_start_source(optional(&p, "patch")?, &token)?;
                    fresh_start::prepare(budget, &source)?
                } else { budget };
                let id = library.create()?;
                let saved = Store::open_budget(&library.database(&id)?, &id).and_then(|store| {
                    let mut store = store.with_backup_preferences(library.data_dir());
                    let token = store.status()?.token;
                    store.save(&budget, &token)
                });
                if let Err(e) = saved { library.forget_new(&id)?; return Err(e); }
                value(id)
            }
            "open_budget" => {
                let id = text("id")?;
                let mut library = self.library.lock();
                if self.db.store()?.has_unsaved_draft() { return Err(t!("Save or discard your changes before opening another budget.")); }
                let store = Store::open_budget(&library.database(&id)?, &id)?.with_backup_preferences(library.data_dir());
                library.set_current(&id)?;
                *self.db.store()? = store;
                *self.pending_import.0.lock().map_err(|e| e.to_string())? = None;
                drop(library);
                self.load()
            }
            "delete_budget" => {
                let mut library = self.library.lock(); library.remove(&text("id")?)?; value(library.list())
            }
            "backup_database" => value(self.db.store()?.backup_portable()?),
            "list_backups" => {
                let folder = self.db.store()?.folder().join("backups");
                let entries = match std::fs::read_dir(&folder) { Ok(e) => e, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!([])), Err(e) => return Err(e.to_string()) };
                let mut files: Vec<_> = entries.filter_map(Result::ok).filter(|e| e.path().is_file()).filter(|e| {
                    let name = e.file_name().to_string_lossy().into_owned(); name.starts_with("budget-") && (name.ends_with(".json") || name.ends_with(".sqlite3"))
                }).map(|e| json!({"name":e.file_name().to_string_lossy(),"path":e.path().to_string_lossy(),"bytes":e.metadata().map(|m| m.len()).unwrap_or(0)})).collect();
                files.sort_by(|a,b| b["name"].as_str().cmp(&a["name"].as_str()));
                value(files)
            }
            "restore_backup_file" => {
                let root = self.db.store()?.folder().join("backups").canonicalize().map_err(|e| e.to_string())?;
                let path = PathBuf::from(text("path")?).canonicalize().map_err(|e| e.to_string())?;
                if path.parent() != Some(root.as_path()) || !path.is_file() { return Err(t!("Choose a backup from this budget's backup folder.")); }
                let budget = match path.extension().and_then(|s| s.to_str()) {
                    Some("json") => serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?,
                    Some("sqlite3") => Store::open(&path)?.load()?.budget,
                    _ => return Err(t!("Choose a JSON or SQLite budget backup.")),
                };
                self.db.store()?.restore_backup(&budget, &text("expectedToken")?)?;
                self.load()
            }
            "backup_retention" => value(self.db.store()?.backup_retention()?),
            "set_backup_retention" => { self.db.store()?.set_backup_retention(field(&p, "keep")?)?; Ok(Value::Null) }
            "prune_backups" => value(self.db.store()?.prune_backups(field(&p, "keep")?)?),
            "restore_budget" => {
                self.db.store()?.restore_backup(&field(&p, "budget")?, &text("expectedToken")?)?;
                self.load()
            }
            "read_recovery" => value(self.db.store()?.read_recovery()?),
            "discard_recovery" => { self.db.store()?.discard_recovery()?; Ok(Value::Null) }
            "export_budget" => {
                let mut store = self.db.store()?;
                let budget = store.full_draft(optional(&p, "patch")?)?;
                value(serde_json::to_string_pretty(&budget).map_err(|e| e.to_string())?)
            }
            "run_schedules" => {
                self.db.store()?.run_schedules(&text("expectedToken")?, &text("through")?)?;
                self.load()
            }
            "apply_due_schedules" => {
                let mut store = self.db.store()?;
                let status = store.status()?;
                // An empty database is the welcome/migration state. Schedule normalization
                // may add empty extras, which must not mark it as a configured user budget.
                if !status.saved_at.is_empty() { store.run_schedules(&status.token, &text("through")?)?; }
                drop(store);
                self.load()
            }
            "cloud_status" => value(cloud::cloud_status(&self.cloud)),
            "account_start" => cloud::account_start(&self.cloud, text("email")?).await,
            "account_verify" => value(cloud::account_verify(&self.cloud, text("email")?, text("code")?).await?),
            "account_overview" => cloud::account_overview(&self.cloud).await,
            "account_sign_out" => value(cloud::account_sign_out(&self.cloud).await?),
            "account_open" => { cloud::account_open(&self.cloud, text("next")?).await?; Ok(Value::Null) }
            "account_delete" => value(cloud::account_delete(&self.cloud, text("confirm")?).await?),
            "sync_status" => value(cloud::sync::sync_status(&self.cloud)),
            "sync_now" => {
                let clean = !self.db.store()?.has_unsaved_draft();
                value(cloud::sync::sync_now(&self.cloud, &self.db, &self.library, clean).await?)
            }
            "sync_has_key" => value(cloud::sync::sync_has_key(&self.cloud).await?),
            "sync_setup" => cloud::sync::sync_setup(&self.cloud, &self.library, text("passphrase")?).await,
            "sync_reset" => { cloud::sync::sync_reset(&self.cloud, &self.library, text("passphrase")?).await?; Ok(Value::Null) }
            "sync_track" => { cloud::sync::sync_track(&self.cloud, field(&p, "ids")?).await?; Ok(Value::Null) }
            "sync_download" => value(cloud::sync::sync_download(&self.cloud, &self.library, text("id")?).await?),
            "sync_resolve" => {
                let clean = !self.db.store()?.has_unsaved_draft();
                value(cloud::sync::sync_resolve(&self.cloud, &self.db, &self.library, text("id")?, text("keep")?, optional(&p, "choices")?, clean).await?)
            }
            "sync_remove" => { cloud::sync::sync_remove(&self.cloud, text("id")?).await?; Ok(Value::Null) }
            "sync_lock" => { cloud::sync::sync_lock(&self.cloud).await?; Ok(Value::Null) }
            "billing_context" => crate::billing::context(&self.cloud).await,
            "billing_verify" => crate::billing::verify(&self.cloud, p).await,
            "billing_apple_context" => crate::billing::apple_context(&self.cloud).await,
            "billing_apple_verify" => crate::billing::apple_verify(&self.cloud, text("transaction")?).await,
            "simplefin_import" => {
                let token = optional::<String>(&p, "expectedToken")?.unwrap_or(self.db.store()?.status()?.token);
                if self.db.store()?.status()?.token != token { return Err(t!("CONFLICT: The budget changed. Refresh the bank review before importing.")); }
                let patch = simplefin::dispatch(method, p, &self.db, &self.simplefin).await?;
                let budget = self.db.store()?.full_draft(Some(serde_json::from_value(patch).map_err(|e| e.to_string())?))?;
                self.save(&budget, &token)?;
                self.load()
            }
            "simplefin_sync" => {
                let expected = optional::<String>(&p, "expectedToken")?;
                if expected.as_ref().is_some_and(|token| self.db.store().and_then(|s| s.status()).map(|s| s.token != *token).unwrap_or(true)) {
                    return Err(t!("The budget changed. Refresh the bank review before downloading."));
                }
                let result = value(simplefin::simplefin_sync_compact(&self.db, &self.simplefin, None, p["refresh"].as_bool().unwrap_or(true)).await?)?;
                if expected.as_ref().is_some_and(|token| self.db.store().and_then(|s| s.status()).map(|s| s.token != *token).unwrap_or(true)) {
                    return Err(t!("The budget changed while downloading. Refresh the bank review before importing."));
                }
                Ok(result)
            }
            m if m.starts_with("assistant_") => assistant::dispatch(m, p, &self.db, &self.assistant, &self.simplefin, &self.cloud).await,
            m if m.starts_with("simplefin_") => simplefin::dispatch(m, p, &self.db, &self.simplefin).await,
            "ynab_tokens" | "forget_ynab_token" | "ynab_plans" | "sync_ynab" | "accept_ynab_import" | "resolve_ynab" => ynab_capture::dispatch(method, p, &self.db, &self.pending_import).await,
            _ => Err(format!("Unknown native service command: {method}")),
        }
    }

}
