//! Native host command router. SQLite and the service state live for the process lifetime so
//! sync, bank imports and assistant edits all observe the same saved revision and draft.
use crate::{assistant, budgets, cloud, commands, domain, engine, fresh_start, host, simplefin, store::Store, t, vault, ynab_capture, Database, Library};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::collections::HashMap;

#[derive(Default)]
/// `proposal` is the assistant's card (conversation, proposal) whose change is the latest in
/// `undo`; `undone` is the one whose change is the latest in `redo`. The budget's own Undo and
/// Redo move a card between them, and its conversation is told.
struct History { token: String, undo: Vec<crate::patch::Patch>, redo: Vec<crate::patch::Patch>, proposal: Option<(String, String)>, undone: Option<(String, String)> }

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
        assert!(initial["budget"].is_null(),"an empty library has no placeholder budget");
        call(&runtime,"save_budget",json!({"budget":budget,"expectedToken":""})).unwrap();
        let initial=runtime.load().unwrap();
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
        call(&runtime,"save_budget",json!({"budget":urbudget_core::test_support::sample(),"expectedToken":""})).unwrap();
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

    /// A runtime with a saved budget that has `sourceExport`, the budget and the token of that save.
    fn imported_budget(folder: &Path) -> (Runtime, domain::Budget, String) {
        let runtime = Runtime::new(folder).unwrap();
        let mut budget = urbudget_core::test_support::sample();
        budget.extra.insert("sourceExport".into(), json!({"data":{"server_knowledge":7}}));
        call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":""})).unwrap();
        let token = runtime.load().unwrap()["token"].as_str().unwrap().to_owned();
        (runtime, budget, token)
    }

    #[test]
    fn no_op_save_pushes_no_undo_and_undo_keeps_source_export() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, mut budget, token) = imported_budget(folder.path());
        let same = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":token,"ackOnly":true})).unwrap();
        assert_eq!(same["token"], token.as_str(), "nothing changed, so nothing was written");
        assert_eq!(same["undoAvailable"], false);
        budget.name = "Edited".into();
        let edited = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":token,"ackOnly":true})).unwrap();
        assert_ne!(edited["token"], token.as_str());
        assert_eq!(edited["undoAvailable"], true);
        let again = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":edited["token"],"ackOnly":true})).unwrap();
        assert_eq!(again["token"], edited["token"]);
        let undone = call(&runtime, "undo_budget", json!({"expectedToken":edited["token"]})).unwrap();
        assert_eq!(undone["budget"]["name"], urbudget_core::test_support::sample().name);
        assert_eq!(undone["undoAvailable"], false, "the no-op saves added nothing to undo");
        assert_eq!(undone["budget"]["sourceExport"]["data"]["server_knowledge"], 7);
        let redone = call(&runtime, "redo_budget", json!({"expectedToken":undone["token"]})).unwrap();
        assert_eq!(redone["budget"]["name"], "Edited");
        assert_eq!(redone["budget"]["sourceExport"]["data"]["server_knowledge"], 7);
    }

    #[test]
    fn save_without_heavy_extras_still_takes_the_full_path() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, mut budget, token) = imported_budget(folder.path());
        budget.extra.remove("sourceExport");
        let saved = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":token,"ackOnly":true})).unwrap();
        assert_eq!(saved["undoAvailable"], true);
        // Today's behaviour, not a wanted one: it's pinned until the iPhone bug behind it is decided.
        assert!(runtime.load().unwrap()["budget"].get("sourceExport").is_none());
        let undone = call(&runtime, "undo_budget", json!({"expectedToken":saved["token"]})).unwrap();
        assert_eq!(undone["budget"]["sourceExport"]["data"]["server_knowledge"], 7);
    }

    #[test]
    fn save_ack_reports_the_saved_revision_and_undo_state_from_either_path() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, mut budget, token) = imported_budget(folder.path());
        let check = |ack: &Value, undo: bool| {
            let status = runtime.native_status().unwrap();
            for key in ["token", "savedAt", "databasePath", "isNew", "undoAvailable", "redoAvailable"] {
                assert_eq!(ack[key], status[key], "{key}");
            }
            assert_eq!(ack["undoAvailable"], undo);
            assert_eq!(ack["redoAvailable"], false);
            assert_eq!(ack["isNew"], false);
        };
        budget.name = "Row edit".into();
        let edited = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":token,"ackOnly":true})).unwrap();
        check(&edited, true);
        budget.extra.insert("sourceExport".into(), json!({"data":{"server_knowledge":8}}));
        let heavy = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":edited["token"],"ackOnly":true})).unwrap();
        check(&heavy, true);
        assert_eq!(runtime.load().unwrap()["budget"]["sourceExport"]["data"]["server_knowledge"], 8);
    }

    #[test]
    fn a_later_save_clears_redo_and_a_proposals_undo() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, mut budget, token) = imported_budget(folder.path());
        budget.name = "First".into();
        let first = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":token,"ackOnly":true})).unwrap();
        let undone = call(&runtime, "undo_budget", json!({"expectedToken":first["token"]})).unwrap();
        assert_eq!(undone["redoAvailable"], true);
        runtime.history.lock().unwrap().values_mut().for_each(|h| h.proposal = Some(("thread".into(), "proposal".into())));
        // Nothing changes: redo and the proposal's undo both stay.
        budget.name = undone["budget"]["name"].as_str().unwrap().into();
        let same = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":undone["token"],"ackOnly":true})).unwrap();
        assert_eq!(same["redoAvailable"], true);
        assert!(runtime.history.lock().unwrap().values().all(|h| h.proposal.is_some()));
        budget.name = "Second".into();
        let second = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":undone["token"],"ackOnly":true})).unwrap();
        assert_eq!(second["redoAvailable"], false);
        assert!(runtime.history.lock().unwrap().values().all(|h| h.proposal.is_none()));
    }

    #[test]
    fn no_op_save_of_a_library_copy_pushes_no_undo() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let mut budget = urbudget_core::test_support::sample();
        let mut purchase = urbudget_core::test_support::purchase();
        purchase.extra.insert("source".into(), json!({"id":"ynab-row"}));
        budget.transactions.push(purchase);
        let id = call(&runtime, "create_budget", json!({"budget":budget})).unwrap();
        let open = || call(&runtime, "library_open", json!({"id":id})).unwrap();
        let copy = open();
        let mut renamed = copy["budget"].clone();
        renamed["name"] = json!("Renamed");
        let first = call(&runtime, "save_budget", json!({"budget":renamed,"expectedToken":copy["token"],"ackOnly":true})).unwrap();
        assert_eq!(first["undoAvailable"], true);
        let undone = call(&runtime, "undo_budget", json!({"expectedToken":first["token"]})).unwrap();
        assert_eq!(undone["redoAvailable"], true);
        runtime.history.lock().unwrap().values_mut().for_each(|h| h.proposal = Some(("thread".into(), "proposal".into())));

        // A library copy leaves out heavy extras but not `source`. A recovery export leaves that out
        // too, and comes back as a whole budget if it's imported. Neither is a change.
        let copy = open();
        assert_eq!(copy["budget"]["transactions"][0]["source"]["id"], "ynab-row");
        assert!(copy["budget"]["transactions"][0].get("hasImportSource").is_none());
        let opened: domain::Budget = serde_json::from_value(copy["budget"].clone()).unwrap();
        let exported = crate::store::for_window(&opened);
        assert!(exported.transactions[0].extra.get("source").is_none());
        for (name, sent) in [("library copy", opened), ("recovery export", exported)] {
            let same = call(&runtime, "save_budget", json!({"budget":sent,"expectedToken":copy["token"],"ackOnly":true})).unwrap();
            assert_eq!(same["token"], copy["token"], "{name}");
            assert_eq!(same["undoAvailable"], false, "{name}");
            assert_eq!(same["redoAvailable"], true, "{name}");
            assert!(runtime.history.lock().unwrap().values().all(|h| h.proposal.is_some()), "{name}");
            let stored = call(&runtime, "saved_budget", json!({})).unwrap();
            assert_eq!(stored["budget"]["transactions"][0]["source"]["id"], "ynab-row", "{name}");
            assert!(stored["budget"]["transactions"][0].get("hasImportSource").is_none(), "{name}");
        }
    }

    fn status_edit(id: &str, status: &str, token: &Value) -> Value {
        json!({"steps":[{"method":"workflow","params":{"action":{"type":"bulk","ids":[id],"operation":"status","value":status,"accountId":"all"}}}],
            "expectedToken":token,"month":"2026-10","asOfMonth":"2026-10"})
    }

    /// A runtime with one saved purchase.
    fn budget_with_a_purchase(folder: &Path) -> (Runtime, Value) {
        let runtime = Runtime::new(folder).unwrap();
        let mut budget = urbudget_core::test_support::sample();
        budget.transactions.push(urbudget_core::test_support::purchase());
        call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":""})).unwrap();
        let token = runtime.load().unwrap()["token"].clone();
        (runtime, token)
    }

    #[test]
    fn edit_budget_holds_the_commands_queue_but_not_bank_commands() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        crate::block_on(async {
            let short = std::time::Duration::from_millis(200);
            let bank_network = runtime.bank_commands.lock().await;
            let saved = tokio::time::timeout(std::time::Duration::from_secs(1), runtime.dispatch("edit_budget", status_edit("tx", "uncleared", &token)))
                .await.expect("bank work cannot hold the local edit queue").unwrap();
            drop(bank_network);
            // In line with `save_budget`, so an edit never runs ahead of the one before it.
            let commit_queue = runtime.commands.lock().await;
            assert!(tokio::time::timeout(short, runtime.dispatch("edit_budget", status_edit("tx", "cleared", &saved["token"]))).await.is_err(), "edit_budget must queue");
            assert!(tokio::time::timeout(short, runtime.dispatch("saved_budget", json!({}))).await.is_err(), "saved_budget must queue");
            drop(commit_queue);
            let saved = tokio::time::timeout(std::time::Duration::from_secs(1), runtime.dispatch("edit_budget", status_edit("tx", "cleared", &saved["token"])))
                .await.unwrap().unwrap();
            assert_eq!(saved["undoAvailable"], true);
        });
    }

    #[test]
    fn proposal_cleared() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let refused = "The budget changed after this proposal. Use the budget's undo history to review later changes.";
        let undo = |token: &Value| call(&runtime, "assistant_undo_commit", json!({"threadId":"thread","proposalId":"proposal","expectedToken":token})).unwrap_err();
        let first = call(&runtime, "edit_budget", status_edit("tx", "uncleared", &token)).unwrap();
        runtime.history.lock().unwrap().values_mut().for_each(|h| h.proposal = Some(("thread".into(), "proposal".into())));
        // The proposal's undo is allowed through to the assistant, which doesn't know this one.
        assert_ne!(undo(&first["token"]), refused);
        // Nothing changed, so its undo stays.
        let same = call(&runtime, "edit_budget", status_edit("tx", "uncleared", &first["token"])).unwrap();
        assert_eq!(same["token"], first["token"]);
        assert_ne!(undo(&same["token"]), refused);
        let later = call(&runtime, "edit_budget", status_edit("tx", "cleared", &same["token"])).unwrap();
        assert_eq!(undo(&later["token"]), refused);
    }

    /// `undo_budget` or `redo_budget` the way Android asks.
    fn patch_step(runtime: &Runtime, method: &str, token: &Value) -> Result<Value, String> {
        call(runtime, method, json!({"patchOnly":true,"expectedToken":token,"month":"2026-10","asOfMonth":"2026-10"}))
    }
    fn saved_budget(runtime: &Runtime) -> Value {
        call(runtime, "saved_budget", json!({})).unwrap()["budget"].clone()
    }
    fn memo_edit(memo: &str, token: &Value) -> Value {
        json!({"steps":[{"method":"workflow","params":{"action":{"type":"bulk","ids":["tx"],"operation":"memo","value":memo}}}],
            "expectedToken":token,"month":"2026-10","asOfMonth":"2026-10"})
    }
    fn rename_edit(name: &str, token: &Value) -> Value {
        json!({"steps":[{"patch":{"name":name}}],"expectedToken":token,"month":"2026-10","asOfMonth":"2026-10"})
    }
    fn recovery_bytes(runtime: &Runtime) -> Vec<u8> {
        let path = runtime.db.store().unwrap().status().unwrap().database_path;
        std::fs::read(Path::new(&path).with_file_name("recovery.json")).unwrap()
    }
    /// Refuses any commit that names the budget "Rejected", the way a full disk would. Only those:
    /// creating or dropping a trigger is another connection's write, which moves the token and so
    /// clears the undo history, and these tests need a failure with the history still there.
    fn reject_the_name(runtime: &Runtime) -> (rusqlite::Connection, Value) {
        let path = runtime.db.store().unwrap().status().unwrap().database_path;
        let external = rusqlite::Connection::open(path).unwrap();
        external.execute_batch("CREATE TRIGGER reject_native_save BEFORE UPDATE ON metadata WHEN NEW.name='Rejected' BEGIN SELECT RAISE(ABORT, 'Simulated disk failure'); END;").unwrap();
        let token = runtime.native_status().unwrap()["token"].clone();
        (external, token)
    }
    /// A runtime whose last edit failed, so its recovery file is waiting, with an edit to undo.
    fn failed_edit(folder: &Path) -> (Runtime, rusqlite::Connection, Value) {
        let (runtime, _) = budget_with_a_purchase(folder);
        let (external, token) = reject_the_name(&runtime);
        let saved = call(&runtime, "edit_budget", memo_edit("Saved", &token)).unwrap();
        let refused = call(&runtime, "edit_budget", rename_edit("Rejected", &saved["token"])).unwrap_err();
        assert!(refused.contains("Simulated disk failure"), "{refused}");
        assert_eq!(runtime.native_status().unwrap()["token"], saved["token"], "the failure saved nothing");
        assert!(call(&runtime, "read_recovery", json!({})).unwrap().is_object());
        (runtime, external, saved["token"].clone())
    }

    #[test]
    fn undo_redo_patch() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let before = saved_budget(&runtime);
        let edited = call(&runtime, "edit_budget", memo_edit("Edited", &token)).unwrap();
        let after = saved_budget(&runtime);

        let undone = patch_step(&runtime, "undo_budget", &edited["token"]).unwrap();
        assert!(undone.get("budget").is_none(), "no budget crosses");
        assert_eq!(undone["patch"]["transactions"]["upsert"][0]["memo"], "");
        assert!(undone["patch"].get("assignments").is_none(), "only what changed");
        assert_eq!(undone["undoAvailable"], false);
        assert_eq!(undone["redoAvailable"], true);
        assert_ne!(undone["token"], edited["token"]);
        assert_eq!(saved_budget(&runtime), before);
        for key in ["token", "savedAt", "databasePath", "isNew", "undoAvailable", "redoAvailable"] {
            assert_eq!(undone[key], runtime.native_status().unwrap()[key], "{key}");
        }
        let store = serde_json::from_value::<domain::Budget>(before.clone()).unwrap();
        assert_eq!(undone["summary"], value(totals(&store, "2026-10", Some("2026-10")).unwrap()).unwrap());

        let redone = patch_step(&runtime, "redo_budget", &undone["token"]).unwrap();
        assert_eq!(redone["patch"]["transactions"]["upsert"][0]["memo"], "Edited");
        assert_eq!(redone["undoAvailable"], true);
        assert_eq!(redone["redoAvailable"], false);
        assert_eq!(saved_budget(&runtime), after);

        let stale = "CONFLICT: The budget changed after that edit. Review the current budget before undoing.";
        assert_eq!(patch_step(&runtime, "undo_budget", &undone["token"]).unwrap_err(), stale);
        assert_eq!(patch_step(&runtime, "redo_budget", &undone["token"]).unwrap_err(), stale);
        assert_eq!(saved_budget(&runtime), after, "refused, nothing changed");
    }

    #[test]
    fn undo_redo_with_nothing_to_do_keeps_the_token() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        for method in ["undo_budget", "redo_budget"] {
            let reply = patch_step(&runtime, method, &token).unwrap();
            assert_eq!(reply["patch"], json!({}), "{method}");
            assert_eq!(reply["token"], token, "{method}");
            assert_eq!(reply["undoAvailable"], false, "{method}");
            assert_eq!(reply["redoAvailable"], false, "{method}");
            assert!(reply["summary"].is_object(), "{method}");
        }
    }

    #[test]
    fn undo_redo_with_no_budget_open_is_a_conflict() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        runtime.db.replace(None);
        let refused = patch_step(&runtime, "undo_budget", &token).unwrap_err();
        assert!(refused.starts_with("CONFLICT:"), "{refused}");
    }

    #[test]
    fn undo_redo_chain_limit_50() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, first) = budget_with_a_purchase(folder.path());
        let mut token = first;
        let mut after_each = vec![saved_budget(&runtime)];
        for n in 0..60 {
            token = call(&runtime, "edit_budget", memo_edit(&format!("Memo {n}"), &token)).unwrap()["token"].clone();
            after_each.push(saved_budget(&runtime));
        }
        for back in 1..=50 {
            let undone = patch_step(&runtime, "undo_budget", &token).unwrap();
            token = undone["token"].clone();
            assert_eq!(saved_budget(&runtime), after_each[60 - back], "undo {back}");
            assert_eq!(undone["undoAvailable"], back < 50, "undo {back}");
        }
        let extra = patch_step(&runtime, "undo_budget", &token).unwrap();
        assert_eq!(extra["patch"], json!({}), "fifty is the limit");
        assert_eq!(extra["token"], token);
        for forward in 1..=50 {
            let redone = patch_step(&runtime, "redo_budget", &token).unwrap();
            token = redone["token"].clone();
            assert_eq!(saved_budget(&runtime), after_each[10 + forward], "redo {forward}");
            assert_eq!(redone["redoAvailable"], forward < 50, "redo {forward}");
        }
    }

    #[test]
    fn undo_restores_deleted_source() {
        let folder = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(folder.path()).unwrap();
        let mut budget = serde_json::to_value(urbudget_core::test_support::sample()).unwrap();
        let mut row = serde_json::to_value(urbudget_core::test_support::purchase()).unwrap();
        row["source"] = json!({"id":"ynab-row","subtransactions":[{"id":"ynab-part"}],"memo":"Café 🛒"});
        row["splits"][0]["source"] = json!({"id":"ynab-part","amount":-50_000});
        budget["transactions"] = json!([row]);
        call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":""})).unwrap();
        let token = runtime.load().unwrap()["token"].clone();
        let stored = saved_budget(&runtime);
        let row_text = |budget: &Value| serde_json::to_string(&budget["transactions"][0]).unwrap();
        assert!(row_text(&stored).contains("ynab-part"));

        let delete = json!({"steps":[{"method":"workflow","params":{"action":{"type":"bulk","ids":["tx"],"operation":"delete","value":null}}}],
            "expectedToken":token,"month":"2026-10","asOfMonth":"2026-10"});
        let deleted = call(&runtime, "edit_budget", delete).unwrap();
        assert_eq!(saved_budget(&runtime)["transactions"], json!([]));

        let undone = patch_step(&runtime, "undo_budget", &deleted["token"]).unwrap();
        assert_eq!(row_text(&saved_budget(&runtime)), row_text(&stored), "stored byte for byte");
        assert_eq!(serde_json::to_string(&undone["patch"]["transactions"]["upsert"][0]).unwrap(), row_text(&stored), "and the app gets the same");

        // A later commit mustn't strip what the undo put back.
        let later = call(&runtime, "edit_budget", memo_edit("Later", &undone["token"])).unwrap();
        let again = saved_budget(&runtime);
        assert_eq!(again["transactions"][0]["source"], stored["transactions"][0]["source"]);
        assert_eq!(again["transactions"][0]["splits"][0]["source"], stored["transactions"][0]["splits"][0]["source"]);
        assert_eq!(later["undoAvailable"], true);
    }

    #[test]
    fn undo_keeps_a_legacy_row_without_created_at() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        assert!(saved_budget(&runtime)["transactions"][0].get("createdAt").is_none(), "this row is from before the field");
        let delete = json!({"steps":[{"method":"workflow","params":{"action":{"type":"bulk","ids":["tx"],"operation":"delete","value":null}}}],
            "expectedToken":token,"month":"2026-10","asOfMonth":"2026-10"});
        let deleted = call(&runtime, "edit_budget", delete).unwrap();
        let undone = patch_step(&runtime, "undo_budget", &deleted["token"]).unwrap();
        // New to the diff, and still nothing to stamp: its place in a day depends on having no time.
        assert!(undone["patch"]["transactions"]["upsert"][0].get("createdAt").is_none());
        assert!(saved_budget(&runtime)["transactions"][0].get("createdAt").is_none());
    }

    #[test]
    fn undo_of_a_heavy_save_removes_source_export() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _, token) = imported_budget(folder.path());
        let mut budget = saved_budget(&runtime);
        budget["sourceExport"] = json!({"data":{"server_knowledge":8}});
        let heavy = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":token,"ackOnly":true})).unwrap();
        let undone = patch_step(&runtime, "undo_budget", &heavy["token"]).unwrap();
        assert_eq!(saved_budget(&runtime)["sourceExport"]["data"]["server_knowledge"], 7);
        assert_eq!(undone["patch"]["extras"]["set"]["sourceExport"]["data"]["server_knowledge"], 7);

        // One the budget had none before: taking it away has to reach the row, not just the draft.
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let mut budget = saved_budget(&runtime);
        budget["sourceExport"] = json!({"data":{"server_knowledge":9}});
        let heavy = call(&runtime, "save_budget", json!({"budget":budget,"expectedToken":token,"ackOnly":true})).unwrap();
        assert_eq!(saved_budget(&runtime)["sourceExport"]["data"]["server_knowledge"], 9);
        let undone = patch_step(&runtime, "undo_budget", &heavy["token"]).unwrap();
        assert!(saved_budget(&runtime).get("sourceExport").is_none());
        assert_eq!(undone["patch"]["extras"]["delete"], json!(["sourceExport"]));
        let redone = patch_step(&runtime, "redo_budget", &undone["token"]).unwrap();
        assert_eq!(saved_budget(&runtime)["sourceExport"]["data"]["server_knowledge"], 9);
        assert_eq!(redone["undoAvailable"], true);
    }

    #[test]
    fn patch_undo_and_full_undo_interleave() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let original = saved_budget(&runtime);
        let one = call(&runtime, "edit_budget", memo_edit("One", &token)).unwrap();
        let after_one = saved_budget(&runtime);
        let two = call(&runtime, "edit_budget", memo_edit("Two", &one["token"])).unwrap();
        let after_two = saved_budget(&runtime);

        let undone = patch_step(&runtime, "undo_budget", &two["token"]).unwrap();
        assert_eq!(saved_budget(&runtime), after_one);
        let undone = call(&runtime, "undo_budget", json!({"expectedToken":undone["token"]})).unwrap();
        assert_eq!(saved_budget(&runtime), original);
        assert_eq!(undone["budget"]["transactions"][0]["memo"], "");
        let redone = patch_step(&runtime, "redo_budget", &undone["token"]).unwrap();
        assert_eq!(saved_budget(&runtime), after_one);
        let redone = call(&runtime, "redo_budget", json!({"expectedToken":redone["token"]})).unwrap();
        assert_eq!(saved_budget(&runtime), after_two);
        assert_eq!(redone["redoAvailable"], false);
    }

    #[test]
    fn ios_shaped_undo_still_returns_the_budget() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let edited = call(&runtime, "edit_budget", memo_edit("Edited", &token)).unwrap();
        for (method, memo) in [("undo_budget", ""), ("redo_budget", "Edited")] {
            let status = runtime.native_status().unwrap();
            // `patchOnly: false` is what leaving it out means, and with neither there's no month to want.
            let plain = if memo.is_empty() { json!({"expectedToken":status["token"]}) } else { json!({"expectedToken":status["token"],"patchOnly":false}) };
            let reply = call(&runtime, method, plain).unwrap();
            assert_eq!(reply["budget"]["transactions"][0]["memo"], memo);
            assert!(reply.get("patch").is_none(), "{method}");
            assert!(reply.get("summary").is_none(), "{method}");
            assert_eq!(reply.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
                ["budget", "databasePath", "isNew", "redoAvailable", "savedAt", "token", "undoAvailable"], "{method}");
        }
        assert_eq!(edited["undoAvailable"], true);
    }

    #[test]
    fn patch_undo_leaves_a_pending_recovery_file() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _external, token) = failed_edit(folder.path());
        let held = recovery_bytes(&runtime);
        let undone = patch_step(&runtime, "undo_budget", &token).unwrap();
        assert_ne!(undone["token"], token, "a real undo");
        assert_eq!(recovery_bytes(&runtime), held, "a plain undo leaves it too");
        assert_eq!(call(&runtime, "read_recovery", json!({})).unwrap()["budget"]["name"], "Rejected");
    }

    #[test]
    fn patch_redo_leaves_a_pending_recovery_file() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _external, token) = failed_edit(folder.path());
        let undone = patch_step(&runtime, "undo_budget", &token).unwrap();
        let held = recovery_bytes(&runtime);
        let redone = patch_step(&runtime, "redo_budget", &undone["token"]).unwrap();
        assert_ne!(redone["token"], undone["token"], "a real redo");
        assert_eq!(recovery_bytes(&runtime), held);
    }

    #[test]
    fn patch_undo_with_nothing_to_do_leaves_a_pending_recovery_file() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _external, token) = failed_edit(folder.path());
        let held = recovery_bytes(&runtime);
        let reply = patch_step(&runtime, "redo_budget", &token).unwrap();
        assert_eq!(reply["patch"], json!({}));
        assert_eq!(recovery_bytes(&runtime), held);
    }

    #[test]
    fn patch_undo_after_the_trigger_is_dropped_leaves_a_pending_recovery_file() {
        // The dropped trigger is another connection's write, so the token moved and there's no
        // history left to undo: the same reply a plain undo gives, and the file stays.
        let folder = tempfile::tempdir().unwrap();
        let (runtime, external, _) = failed_edit(folder.path());
        let held = recovery_bytes(&runtime);
        external.execute_batch("DROP TRIGGER reject_native_save;").unwrap();
        let token = runtime.native_status().unwrap()["token"].clone();
        let reply = patch_step(&runtime, "undo_budget", &token).unwrap();
        assert_eq!(reply["patch"], json!({}));
        assert_eq!(recovery_bytes(&runtime), held);
    }

    #[test]
    fn failed_patch_undo_leaves_the_recovery_file_and_history() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _) = budget_with_a_purchase(folder.path());
        let token = runtime.native_status().unwrap()["token"].clone();
        // What undoing the rename puts back is what the trigger will refuse.
        let named = call(&runtime, "edit_budget", rename_edit("Rejected", &token)).unwrap();
        let (_external, token) = reject_the_name(&runtime);
        assert_ne!(token, named["token"]);
        let renamed = call(&runtime, "edit_budget", rename_edit("Renamed", &token)).unwrap();
        call(&runtime, "edit_budget", rename_edit("Rejected", &renamed["token"])).unwrap_err();
        let held = recovery_bytes(&runtime);
        let before = saved_budget(&runtime);

        let refused = patch_step(&runtime, "undo_budget", &renamed["token"]).unwrap_err();
        assert!(refused.contains("Simulated disk failure"), "{refused}");
        assert_eq!(recovery_bytes(&runtime), held, "only an edit writes this file");
        assert_eq!(saved_budget(&runtime), before);
        assert!(!runtime.db.store().unwrap().has_unsaved_draft());
        let status = runtime.native_status().unwrap();
        assert_eq!(status["token"], renamed["token"]);
        assert_eq!(status["undoAvailable"], true, "the failed undo keeps its place");
        assert_eq!(status["redoAvailable"], false);
    }

    #[test]
    fn an_edit_after_a_failure_still_clears_the_recovery_file() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _external, token) = failed_edit(folder.path());
        let undone = patch_step(&runtime, "undo_budget", &token).unwrap();
        assert!(call(&runtime, "read_recovery", json!({})).unwrap().is_object());
        call(&runtime, "edit_budget", memo_edit("Later", &undone["token"])).unwrap();
        assert!(call(&runtime, "read_recovery", json!({})).unwrap().is_null());
    }

    #[test]
    fn compact_export_parses_equal_to_the_default() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _, _) = imported_budget(folder.path());
        let pretty = call(&runtime, "export_budget", json!({})).unwrap();
        let compact = call(&runtime, "export_budget", json!({"compact":true})).unwrap();
        let (pretty, compact) = (pretty.as_str().unwrap(), compact.as_str().unwrap());
        assert!(!compact.contains('\n') && compact.len() < pretty.len());
        assert_eq!(serde_json::from_str::<Value>(compact).unwrap(), serde_json::from_str::<Value>(pretty).unwrap());
        assert_eq!(serde_json::from_str::<Value>(compact).unwrap()["sourceExport"]["data"]["server_knowledge"], 7);
    }

    #[test]
    fn default_export_stays_pretty_and_refuses_when_closed() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _, _) = imported_budget(folder.path());
        let budget = runtime.db.store().unwrap().full_draft(None).unwrap();
        assert_eq!(call(&runtime, "export_budget", json!({})).unwrap(), serde_json::to_string_pretty(&budget).unwrap());
        runtime.db.replace(None);
        assert_eq!(call(&runtime, "export_budget", json!({})).unwrap_err(), "Open or create a budget first.");
    }

    #[test]
    fn compact_export_is_null_when_closed() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _, _) = imported_budget(folder.path());
        runtime.db.replace(None);
        assert_eq!(call(&runtime, "export_budget", json!({"compact":true})).unwrap(), Value::Null);
    }

    const STALE: &str = "CONFLICT: The budget changed. Reload it before saving your edit.";
    fn totals_params() -> Value {
        json!({"month":"2026-10","asOfMonth":"2026-10"})
    }
    fn reconcile_params(balance: i64) -> Value {
        json!({"accountId":"cash","balance":balance,"date":"2026-10-21","adjustmentId":"adjustment"})
    }
    fn assign_params() -> Value {
        let month = engine::current_month();
        json!({"action":{"type":"assignAmount","month":month,"categoryId":"food","amount":1234},"previewMonth":month})
    }

    #[test]
    fn query_does_not_commit() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let draft = call(&runtime, "snapshot_budget", json!({})).unwrap();
        let rows = saved_budget(&runtime);
        for (method, params) in [("reconcile", reconcile_params(777)), ("workflow", assign_params())] {
            call(&runtime, "query_budget", json!({"method":method,"params":params,"expectedToken":token})).unwrap();
            assert_eq!(runtime.native_status().unwrap()["token"], token, "{method}");
            assert_eq!(call(&runtime, "snapshot_budget", json!({})).unwrap(), draft, "{method}");
            assert_eq!(saved_budget(&runtime), rows, "{method}");
            assert_eq!(runtime.native_status().unwrap()["undoAvailable"], false, "{method}");
        }
    }

    #[test]
    fn query_answers_as_the_stateless_call_does() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let budget: domain::Budget = serde_json::from_value(saved_budget(&runtime)).unwrap();
        let totals = call(&runtime, "query_budget", json!({"method":"calculate","params":totals_params(),"expectedToken":token})).unwrap();
        assert_eq!(totals, serde_json::to_value(engine::calculate_at(&budget, "2026-10", "2026-10").unwrap()).unwrap());
        let summary = call(&runtime, "query_budget", json!({"method":"workflow","params":assign_params()})).unwrap()["summary"].clone();
        let category = summary["categories"].as_array().unwrap().iter().find(|c| c["id"] == "food").unwrap();
        assert_eq!(category["assigned"], 1234);
    }

    #[test]
    fn query_is_independent_of_the_commands_queue() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        crate::block_on(async {
            let _commit_queue = runtime.commands.lock().await;
            let params = json!({"method":"calculate","params":totals_params(),"expectedToken":token});
            tokio::time::timeout(std::time::Duration::from_secs(1), runtime.dispatch("query_budget", params))
                .await.expect("query_budget waited for the command queue").unwrap();
        });
    }

    #[test]
    fn query_refuses_a_stale_expected_token() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let saved = call(&runtime, "edit_budget", memo_edit("Moved on", &token)).unwrap();
        assert_eq!(call(&runtime, "query_budget", json!({"method":"calculate","params":totals_params(),"expectedToken":token})).unwrap_err(), STALE);
        call(&runtime, "query_budget", json!({"method":"calculate","params":totals_params(),"expectedToken":saved["token"]})).unwrap();
        // Without a token, whatever's stored now.
        call(&runtime, "query_budget", json!({"method":"calculate","params":totals_params()})).unwrap();
        call(&runtime, "query_budget", json!({"method":"calculate","params":totals_params(),"expectedToken":null})).unwrap();
    }

    #[test]
    fn query_with_a_token_on_a_closed_budget_replies_conflict() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        runtime.db.replace(None);
        assert_eq!(call(&runtime, "query_budget", json!({"method":"calculate","params":totals_params(),"expectedToken":token})).unwrap_err(), STALE);
    }

    #[test]
    fn query_without_a_token_on_a_closed_budget_says_so() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, _) = budget_with_a_purchase(folder.path());
        runtime.db.replace(None);
        assert_eq!(call(&runtime, "query_budget", json!({"method":"calculate","params":totals_params()})).unwrap_err(), "Open or create a budget first.");
    }

    #[test]
    fn dry_run_reply_carries_the_reconcile_adjustment() {
        let folder = tempfile::tempdir().unwrap();
        let (runtime, token) = budget_with_a_purchase(folder.path());
        let budget: domain::Budget = serde_json::from_value(saved_budget(&runtime)).unwrap();
        let summary = serde_json::to_value(engine::calculate_at(&budget, "2026-10", "2026-10").unwrap()).unwrap();
        let cleared = summary["accounts"].as_array().unwrap().iter().find(|a| a["id"] == "cash").unwrap()["cleared"].as_i64().unwrap();
        let reply = call(&runtime, "query_budget", json!({"method":"reconcile","params":reconcile_params(cleared + 777),"expectedToken":token})).unwrap();
        let adjustment = reply["transactions"].as_array().unwrap().iter().find(|t| t["id"] == "adjustment").expect("the adjustment");
        assert_eq!(adjustment["amount"], 777);
        assert!(reply["summary"].is_null(), "only a workflow's preview has totals");
        // Nothing to adjust: no such row, and the dialog reads that as zero.
        let exact = call(&runtime, "query_budget", json!({"method":"reconcile","params":reconcile_params(cleared),"expectedToken":token})).unwrap();
        assert!(exact["transactions"].as_array().unwrap().iter().all(|t| t["id"] != "adjustment"));
    }
}

fn value<T: serde::Serialize>(v: T) -> Result<Value, String> { serde_json::to_value(v).map_err(|e| e.to_string()) }
fn field<T: DeserializeOwned>(p: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(p.get(key).cloned().ok_or_else(|| format!("Missing {key}."))?).map_err(|e| format!("Invalid {key}: {e}"))
}
fn optional<T: DeserializeOwned>(p: &Value, key: &str) -> Result<Option<T>, String> {
    match p.get(key).filter(|v| !v.is_null()) { Some(v) => serde_json::from_value(v.clone()).map(Some).map_err(|e| format!("Invalid {key}: {e}")), None => Ok(None) }
}
fn totals(b: &domain::Budget, month: &str, as_of: Option<&str>) -> Result<domain::Summary, String> {
    match as_of { Some(a) => engine::calculate_at(b, month, a), None => engine::calculate(b, month) }
}

impl Runtime {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        vault::init(data_dir);
        let mut library = budgets::Budgets::load(data_dir)?;
        crate::library_api::repair_local_catalog(&mut library)?;
        let id = library.current().to_owned();
        let store = if id.is_empty() { None } else {
            let path = library.database(&id)?;
            if path.is_file() { Some(Store::open_budget(&path, &id)?.with_backup_preferences(data_dir)) }
            else { library.clear_current()?; None }
        };
        Ok(Self {
            db: Database::new(store), library: Library(Mutex::new(library)),
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
        if !self.db.is_open() { return Ok(crate::library_api::empty_snapshot()); }
        let mut store = self.db.store()?;
        let snapshot = store.load()?;
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != snapshot.token { *history = History { token: snapshot.token.clone(), ..History::default() }; }
        Ok(json!({"budget":snapshot.budget,"token":snapshot.token,"savedAt":snapshot.saved_at,"databasePath":snapshot.database_path,
            "isNew":snapshot.saved_at.is_empty(),"undoAvailable":!history.undo.is_empty(),"redoAvailable":!history.redo.is_empty()}))
    }

    /// `load` for a client that keeps the light copy and lets the store hold the rest.
    pub fn load_light(&self) -> Result<Value, String> {
        if !self.db.is_open() { return Ok(crate::library_api::empty_snapshot()); }
        let mut store = self.db.store()?;
        let snapshot = store.load_light()?;
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != snapshot.token { *history = History { token: snapshot.token.clone(), ..History::default() }; }
        // for_window, because json! bypasses Snapshot's light serializer.
        Ok(json!({"budget":crate::store::for_window(&snapshot.budget),"token":snapshot.token,"savedAt":snapshot.saved_at,
            "databasePath":snapshot.database_path,"isNew":snapshot.saved_at.is_empty(),
            "undoAvailable":!history.undo.is_empty(),"redoAvailable":!history.redo.is_empty()}))
    }

    /// Cheap native refresh probe: a stable revision needs no ledger read or JSON round trip.
    fn native_status(&self) -> Result<Value, String> {
        if !self.db.is_open() { return Ok(crate::library_api::empty_snapshot()); }
        let store = self.db.store()?;
        let status = store.status()?;
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != status.token { *history = History { token: status.token.clone(), ..History::default() }; }
        Ok(json!({"token":status.token,"savedAt":status.saved_at,"databasePath":status.database_path,
            "isNew":status.saved_at.is_empty(),"undoAvailable":!history.undo.is_empty(),"redoAvailable":!history.redo.is_empty()}))
    }

    fn save(&self, budget: &domain::Budget, expected: &str) -> Result<Value, String> {
        if !self.db.is_open() {
            if !expected.is_empty(){return Err(t!("CONFLICT: The budget changed. Reload it before saving your edit."));}
            domain::validate(budget)?;
            let mut catalog=self.library.lock();
            let id=cloud::v2::create_owned(&self.cloud,&mut catalog)?;
            let mut store=Store::open_budget(&catalog.database(&id)?,&id)?.with_backup_preferences(catalog.data_dir());
            let token=store.status()?.token;store.save(budget,&token)?;
            catalog.mark_available(&id)?;catalog.set_current(&id)?;self.db.replace(Some(store));drop(catalog);
            return self.native_status();
        }
        let mut store = self.db.store()?;
        if store.status()?.token != expected { return Err(t!("CONFLICT: The budget changed. Reload it before saving your edit.")); }
        domain::validate(budget)?;
        // Native editors send a complete snapshot, but ordinary edits still use the desktop
        // delta commit path. Large imported source payloads and untouched ledger rows stay in
        // SQLite instead of being deleted and inserted again for every small mobile edit.
        let (saved, inverse) = if store.heavy_unchanged(expected, budget)? {
            let edit = store.edit_whole(expected, budget.clone())?;
            let changed = !edit.patch.is_empty();
            (edit.status, changed.then_some(edit.inverse))
        } else {
            let (before, _) = store.saved()?;
            let saved = store.save(budget, expected)?;
            (saved, (&before != budget).then(|| crate::patch::Patch::diff(budget, &before)))
        };
        // Return the revision acknowledged by this transaction, while its Store/history locks
        // are still held. A later sync must not pair its newer token with the caller's old JSON.
        let (undo, redo) = self.record(&store, expected, inverse, &saved);
        Ok(Self::ack(&saved, undo, redo))
    }

    /// A change as this client wants it: `light` for one that keeps the light copy. Only the reply:
    /// history keeps the stored form.
    fn patch_reply(mut patch: crate::patch::Patch, p: &Value) -> Result<Value, String> {
        if p["light"].as_bool().unwrap_or(false) { crate::store::for_window_patch(&mut patch); }
        value(patch)
    }

    fn ack(saved: &crate::store::Status, undo: bool, redo: bool) -> Value {
        json!({"token":saved.token,"savedAt":saved.saved_at,"databasePath":saved.database_path,
            "isNew":saved.saved_at.is_empty(),"undoAvailable":undo,"redoAvailable":redo})
    }

    /// Moves the budget's history from `previous` to `saved`, returning what can be undone and redone.
    fn record(&self, store: &Store, previous: &str, inverse: Option<crate::patch::Patch>, saved: &crate::store::Status) -> (bool, bool) {
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != previous { *history = History::default(); }
        if let Some(inverse) = inverse {
            // Keep inverse changes rather than fifty whole ledgers in a phone's memory.
            history.undo.push(inverse);
            if history.undo.len() > 50 { history.undo.remove(0); }
            history.redo.clear();
            history.proposal = None;
            history.undone = None;
        }
        history.token = saved.token.clone();
        (!history.undo.is_empty(), !history.redo.is_empty())
    }

    /// Runs `steps` on the store's budget as one edit: one commit, one undo. The reply carries what
    /// changed, so no budget crosses to the app and back.
    fn edit_budget(&self, p: &Value) -> Result<Value, String> {
        let expected = field::<String>(p, "expectedToken")?;
        let month = field::<String>(p, "month")?;
        let as_of = optional::<String>(p, "asOfMonth")?;
        let mut steps = field::<Vec<commands::Step>>(p, "steps")?;
        // One guard from the token check to the reply. Sync can close the budget at any moment.
        let mut store = self.db.store()
            .map_err(|_| t!("CONFLICT: The budget changed. Reload it before saving your edit."))?;
        store.prepare_edit(&expected)?;
        store.restore_steps(&mut steps)?;
        // Totals run before the commit, so totals that fail save nothing.
        let (edit, summary) = store.edit(&expected, |budget| commands::apply_steps(budget, steps),
            |after| totals(after, &month, as_of.as_deref()))?;
        let changed = !edit.patch.is_empty();
        let (undo, redo) = self.record(&store, &expected, changed.then_some(edit.inverse), &edit.status);
        let mut reply = Self::ack(&edit.status, undo, redo);
        reply["patch"] = Self::patch_reply(edit.patch, p)?;
        reply["summary"] = value(summary)?;
        Ok(reply)
    }

    fn undo_redo(&self, redo: bool, expected: Option<String>) -> Result<Value, String> {
        let mut store = self.db.store()?;
        let (before, status) = store.saved()?;
        if expected.as_ref().is_some_and(|token| token != &status.token) { return Err(t!("CONFLICT: The budget changed after that edit. Review the current budget before undoing.")); }
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != status.token { *history = History { token: status.token.clone(), ..History::default() }; }
        let next = if redo { history.redo.last() } else { history.undo.last() }.cloned();
        let mut moved = None;
        if let Some(patch) = next {
            let mut next = before.clone();
            patch.apply(&mut next);
            let saved = store.save(&next, &status.token)?;
            let inverse = crate::patch::Patch::diff(&next, &before);
            if redo { history.redo.pop(); history.undo.push(inverse); }
            else { history.undo.pop(); history.redo.push(inverse); }
            history.token = saved.token;
            moved = Self::card_moved(history, redo);
        }
        drop(histories); drop(store);
        self.tell_card(moved);
        self.load()
    }

    /// After an undo or redo: the assistant's card whose change it was, and where the card now
    /// stands. Undo of a card's change leaves it undone (and next in line for Redo); Redo puts
    /// it back as applied. Anything else leaves no card at either end.
    fn card_moved(history: &mut History, redo: bool) -> Option<((String, String), assistant::proposals::Status)> {
        use assistant::proposals::Status;
        if redo {
            let card = history.undone.take();
            history.proposal = card.clone();
            card.map(|c| (c, Status::Applied))
        } else {
            let card = history.proposal.take();
            history.undone = card.clone();
            card.map(|c| (c, Status::Undone))
        }
    }
    /// Tells a card's conversation where it stands after the budget's own Undo or Redo. A
    /// conversation deleted since has nothing to tell.
    fn tell_card(&self, moved: Option<((String, String), assistant::proposals::Status)>) {
        if let Some(((thread, proposal), status)) = moved {
            let _ = assistant::assistant_mark(&self.db, &self.assistant, thread, proposal, status, None);
        }
    }

    /// `undo_redo` for a client that patches its own copy: the reply says what changed and no budget
    /// crosses. Like `undo_redo` it leaves `recovery.json` alone.
    fn undo_redo_patch(&self, redo: bool, p: &Value) -> Result<Value, String> {
        let (expected, month) = (field::<String>(p, "expectedToken")?, field::<String>(p, "month")?);
        let as_of = optional::<String>(p, "asOfMonth")?;
        let conflict = || t!("CONFLICT: The budget changed after that edit. Review the current budget before undoing.");
        let mut store = self.db.store().map_err(|_| conflict())?;
        let status = store.status()?;
        if status.token != expected { return Err(conflict()); }
        let mut histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let history = histories.entry(store.id().to_owned()).or_default();
        if history.token != status.token { *history = History { token: status.token.clone(), ..History::default() }; }
        let next = if redo { history.redo.last() } else { history.undo.last() }.cloned();
        let mut moved = None;
        let (patch, saved) = match next {
            Some(next) => {
                let edit = store.edit_patch(&status.token, next)?;
                // Popped only after success, as `undo_redo` does.
                if redo { history.redo.pop(); history.undo.push(edit.inverse); }
                else { history.undo.pop(); history.redo.push(edit.inverse); }
                history.token = edit.status.token.clone();
                moved = Self::card_moved(history, redo);
                (edit.patch, edit.status)
            }
            None => (crate::patch::Patch::default(), status),
        };
        let (undo, redo_left) = (!history.undo.is_empty(), !history.redo.is_empty());
        drop(histories);
        self.tell_card(moved);
        // Committed: failed totals must not read as a failed undo. Null tells the app to ask.
        let summary = store.read_draft(None, |b| totals(b, &month, as_of.as_deref())).ok();
        let mut reply = Self::ack(&saved, undo, redo_left);
        reply["patch"] = Self::patch_reply(patch, p)?;
        reply["summary"] = value(summary)?;
        Ok(reply)
    }

    /// A read, or a dry run of a changing method, on a copy of the store's budget. Nothing commits,
    /// and no budget crosses from the app.
    fn query_budget(&self, p: &Value) -> Result<Value, String> {
        let method = field::<String>(p, "method")?;
        let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() { return Err("params must be a JSON object".into()); }
        let expected = optional::<String>(p, "expectedToken")?;
        // The guard ends with this statement: a report never holds up an edit.
        let budget = match self.db.store() {
            Ok(mut store) => store.working_copy(expected.as_deref())?,
            // A budget sync closed has moved on, for whoever asked about a revision of it.
            Err(_) if expected.is_some() => return Err(t!("CONFLICT: The budget changed. Reload it before saving your edit.")),
            Err(error) => return Err(error),
        };
        if !commands::changes_budget(&method) { return commands::read(&budget, &method, &params); }
        let outcome = commands::apply(budget.clone(), &method, &params)?;
        let summary = outcome.preview(&params)?;
        let upsert = crate::patch::Patch::diff(&budget, &outcome.budget).transactions.map(|c| c.upsert).unwrap_or_default();
        Ok(json!({"summary":summary,"transactions":upsert}))
    }

    pub async fn dispatch(&self, method: &str, p: Value) -> Result<Value, String> {
        // Preserve command order across JNI worker threads. Polling and stop remain responsive
        // during network work; assistant background jobs only create proposals, never edits.
        // Sync owns its own operation lock and checks Store revisions after every download.
        // Do not hold the UI command queue over that network wait: an offline sync must never
        // make local budgeting wait for the HTTP timeout.
        let independent = method.starts_with("account_") || method.starts_with("billing_") || method.starts_with("sync_") || method.starts_with("library_") || method.starts_with("editor_") || matches!(method,
            "set_language" | "host_events" | "native_budget_status" | "query_budget" | "database_status" | "cloud_status" | "sync_status" | "assistant_stop" | "assistant_cancel_sign_in" | "assistant_status" | "assistant_thread" | "assistant_threads" |
            "assistant_sign_in" | "assistant_sign_out" | "assistant_foreground" | "assistant_settings" | "assistant_set_key" | "assistant_forget_key" | "assistant_models" | "assistant_report_reply" | "assistant_sms_poll" | "assistant_sms_cancel" | "assistant_sms_lanes" | "assistant_phone_reply" |
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
            "load_budget" => if p["light"].as_bool().unwrap_or(false) { self.load_light() } else { self.load() },
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
            "edit_budget" => self.edit_budget(&p),
            "query_budget" => self.query_budget(&p),
            // The rows as stored, heavy extras and all. `load_budget` resets the draft, which would
            // hide any drift between the two from a test looking for it.
            "saved_budget" => {
                let mut store = self.db.store()?;
                let unsaved = store.has_unsaved_draft();
                let (budget, status) = store.saved()?;
                Ok(json!({"budget":budget,"token":status.token,"unsavedDraft":unsaved}))
            }
            "undo_budget" | "redo_budget" if p["patchOnly"].as_bool().unwrap_or(false) => self.undo_redo_patch(method == "redo_budget", &p),
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
            "assistant_notes_commit" => {
                let patch = assistant::assistant_notes(&self.db, None, field(&p, "notes")?)?;
                if !patch.is_empty() {
                    let mut store = self.db.store()?;
                    let budget = store.full_draft(Some(patch))?;
                    let token = store.status()?.token;
                    drop(store);
                    self.save(&budget, &token)?;
                }
                self.load()
            }
            "assistant_thread" => {
                let mut view = assistant::dispatch(method, p.clone(), &self.db, &self.assistant, &self.simplefin, &self.cloud).await?;
                let store = self.db.store()?;
                let (id, token) = (store.id().to_owned(), store.status()?.token);
                drop(store);
                let thread = text("id")?;
                let histories = self.history.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((_, proposal)) = histories.get(&id).filter(|h| h.token == token && !h.undo.is_empty()).and_then(|h| h.proposal.as_ref()).filter(|(of, _)| of == &thread) {
                    view["undoable"] = json!(proposal);
                }
                Ok(view)
            }
            "assistant_apply_commit" => {
                let token = self.db.store()?.status()?.token;
                // The card is marked applied once its change is saved: a save that fails leaves
                // it as it was, to apply again.
                let mut asked = p.clone();
                asked["settle"] = json!(false);
                let mut result = assistant::dispatch("assistant_apply", asked, &self.db, &self.assistant, &self.simplefin, &self.cloud).await?;
                if !result["patch"].is_null() {
                    let patch = serde_json::from_value(result["patch"].clone()).map_err(|e| e.to_string())?;
                    let budget = self.db.store()?.full_draft(Some(patch))?;
                    self.save(&budget, &token)?;
                    result["proposal"] = assistant::assistant_mark(&self.db, &self.assistant, text("threadId")?, text("proposalId")?, assistant::proposals::Status::Applied, None)?;
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
            "library_snapshot" => crate::library_api::snapshot(&self.db,&self.library),
            "library_open" => cloud::v2::open(&self.cloud,&self.db,&self.library,&text("id")?).await,
            "library_empty_trash" => crate::library_api::empty_trash(&self.db,&self.library),
            "library_review_deletion" => cloud::v2::review_deletion(&self.cloud,&self.db,&self.library,&text("id")?).await,
            "library_trash"|"library_restore"|"library_evict"|"library_recover"|"library_purge" => crate::library_api::lifecycle(&self.cloud,&self.db,&self.library,method.trim_start_matches("library_"),&text("id")?).await,
            "editor_checkpoint" => {
                let id=optional::<String>(&p,"budgetId")?.unwrap_or_else(||self.library.lock().current().into());
                crate::library_api::checkpoint(&self.db,&self.library,&id,text("id")?,text("epoch")?,p["target"].clone(),p["baseline"].clone(),p["values"].clone())?;Ok(Value::Null)
            }
            "editor_discard" => {
                let budget=optional::<String>(&p,"budgetId")?.unwrap_or_else(||self.library.lock().current().into());
                if optional::<bool>(&p,"onlyIfActive")?.unwrap_or(false) {
                    crate::library_api::discard_checkpoint_if_active(&self.db,&self.library,&budget,&text("id")?,&text("epoch")?)?;
                } else { crate::library_api::discard_checkpoint(&self.db,&self.library,&budget,&text("id")?,&text("epoch")?)?; }
                Ok(Value::Null)
            }
            "editor_recovery" => {
                let budget=optional::<String>(&p,"budgetId")?.unwrap_or_else(||self.library.lock().current().into());
                crate::library_api::editor_recovery(&self.db,&self.library,&budget)
            }
            "sync_enable" => value(cloud::v2::enable(&self.cloud,&self.library,field(&p,"enabled")?,&optional::<Vec<String>>(&p,"ids")?.unwrap_or_default())?),
            "fresh_start" => {
                let source = self.db.store()?.full_draft(None)?;
                let next = fresh_start::create(&source, &text("name")?, &text("asOf")?)?;
                let mut library = self.library.lock();
                let id = cloud::v2::create_owned(&self.cloud,&mut library)?;
                let saved = Store::open_budget(&library.database(&id)?, &id).and_then(|store| {
                    let mut store = store.with_backup_preferences(library.data_dir());
                    let token = store.status()?.token; store.save(&next, &token)
                });
                if let Err(e) = saved { library.forget_new(&id)?; return Err(e); }
                library.mark_available(&id)?;drop(library);
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
                let id = cloud::v2::create_with_scope(&self.cloud, &mut library, &budget, optional::<bool>(&p, "localOnly")?.unwrap_or(false))?;
                let saved = Store::open_budget(&library.database(&id)?, &id).and_then(|store| {
                    let mut store = store.with_backup_preferences(library.data_dir());
                    let token = store.status()?.token;
                    store.save(&budget, &token)
                });
                if let Err(e) = saved { library.forget_new(&id)?; return Err(e); }
                library.mark_available(&id)?;drop(library);
                value(id)
            }
            "open_budget" => {
                let id = text("id")?;
                cloud::v2::open(&self.cloud,&self.db,&self.library,&id).await?;
                *self.pending_import.0.lock().map_err(|e| e.to_string())? = None;
                self.load()
            }
            "delete_budget" => {
                crate::library_api::lifecycle(&self.cloud,&self.db,&self.library,"trash",&text("id")?).await?; value(self.library.lock().list())
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
                // The phones' exports and recovery copies. Compact, or a backup can pass the import
                // limit in indentation alone, and null once sync has closed the budget.
                let compact = p["compact"].as_bool().unwrap_or(false);
                let mut store = match self.db.store() {
                    Ok(store) => store,
                    Err(_) if compact => return Ok(Value::Null),
                    Err(error) => return Err(error),
                };
                let budget = store.full_draft(optional(&p, "patch")?)?;
                value(if compact { serde_json::to_string(&budget) } else { serde_json::to_string_pretty(&budget) }.map_err(|e| e.to_string())?)
            }
            "run_schedules" => {
                self.db.store()?.run_schedules(&text("expectedToken")?, &text("through")?)?;
                self.load()
            }
            "apply_due_schedules" => {
                if !self.db.is_open(){return self.load();}
                let mut store = self.db.store()?;
                let status = store.status()?;
                // An empty database is the welcome/migration state. Schedule normalization
                // may add empty extras, which must not mark it as a configured user budget.
                if !status.saved_at.is_empty() { store.run_schedules(&status.token, &text("through")?)?; }
                drop(store);
                if p["ackOnly"].as_bool().unwrap_or(false) { self.native_status() } else { self.load() }
            }
            "cloud_status" => value(self.cloud.status_with_library(&self.library)),
            "account_start" => cloud::account_start(&self.cloud, text("email")?).await,
            "account_verify" => value(cloud::account_verify(&self.cloud, text("email")?, text("code")?).await?),
            "account_overview" => cloud::account_overview(&self.cloud).await,
            "account_sign_out" => value(cloud::account_sign_out(&self.cloud).await?),
            "account_open" => { cloud::account_open(&self.cloud, text("next")?).await?; Ok(Value::Null) }
            "account_delete" => value(cloud::account_delete(&self.cloud, text("confirm")?).await?),
            "sync_status" => value(cloud::sync::sync_status(&self.cloud)),
            "sync_now" => {
                let clean = !self.db.store().is_ok_and(|s|s.has_unsaved_draft());
                value(cloud::sync::sync_now(&self.cloud, &self.db, &self.library, clean).await?)
            }
            "sync_has_key" => value(cloud::sync::sync_has_key(&self.cloud).await?),
            "sync_setup" => cloud::sync::sync_setup(&self.cloud, &self.library, text("passphrase")?).await,
            "sync_reset" => { cloud::sync::sync_reset(&self.cloud, &self.library, text("passphrase")?).await?; Ok(Value::Null) }
            "sync_track" => { cloud::sync::sync_track(&self.cloud, field(&p, "ids")?).await?; Ok(Value::Null) }
            "sync_download" => value(cloud::sync::sync_download(&self.cloud, &self.library, text("id")?).await?),
            "sync_resolve" => {
                let clean = !self.db.store().is_ok_and(|s|s.has_unsaved_draft());
                value(cloud::sync::sync_resolve(&self.cloud, &self.db, &self.library, text("id")?, text("keep")?, optional(&p, "choices")?, clean, optional(&p, "review")?).await?)
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
