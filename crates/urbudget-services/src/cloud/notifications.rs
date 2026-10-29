//! Identity-specific invalidations derived from committed state, including a partial failed pass.
use super::{
    sync::{ChangedBudget, Report},
    Cloud,
};
use crate::{store::Store, Database, Library};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Snapshot {
    entries: BTreeMap<String, (String, String)>,
    version: String,
}

pub(super) fn capture(db: &Database, library: &Library) -> Result<Snapshot, String> {
    let (entries, version) = {
        let catalog = library.lock();
        (catalog.try_list_all()?, catalog.version()?)
    };
    let mut states = BTreeMap::new();
    for entry in entries {
        // Opening an absent or evicted file here would manufacture a local budget.
        let token = if entry.availability == "local" && !entry.missing {
            // Use the actual live connection for the open budget; a closed-file read
            // must never install schema or write user_version behind another window.
            let live = db
                .store()
                .ok()
                .and_then(|store| (store.id() == entry.id).then(|| store.status()));
            live.unwrap_or_else(|| {
                Store::existing_status(std::path::Path::new(&entry.path), &entry.id)
            })
            .map(|status| status.token)
            .unwrap_or_default()
        } else {
            String::new()
        };
        let signature = json!({"token":token,"lifecycle":entry.lifecycle,
            "availability":entry.availability,"accountId":entry.account_id,
            "incarnation":entry.incarnation})
        .to_string();
        states.insert(entry.id, (signature, token));
    }
    Ok(Snapshot {
        entries: states,
        version,
    })
}

pub(super) fn finish(
    cloud: &Cloud,
    db: &Database,
    library: &Library,
    before: Snapshot,
    result: Result<Report, String>,
) -> Result<Report, String> {
    let after = match capture(db, library) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return result.map(|mut report| {
                report.state = "error";
                report.message = Some(error);
                cloud.sync.finish(report)
            })
        }
    };
    let ids: BTreeSet<_> = before
        .entries
        .keys()
        .chain(after.entries.keys())
        .cloned()
        .collect();
    let changed: Vec<_> = ids
        .into_iter()
        .filter(|id| before.entries.get(id) != after.entries.get(id))
        .map(|id| ChangedBudget {
            revision: after
                .entries
                .get(&id)
                .map(|s| s.1.clone())
                .unwrap_or_default(),
            id,
        })
        .collect();
    let mut report = match result {
        Ok(report) => report,
        Err(error) if changed.is_empty() => return Err(error),
        Err(error) => Report {
            at: super::sync::now(),
            state: "error",
            message: Some(error),
            budgets: vec![],
            open_changed: false,
            changed_budgets: None,
            library_version: None,
        },
    };
    // Compatibility boolean reflects the identity open now, not a window captured before HTTP.
    report.open_changed |= db
        .store()
        .ok()
        .is_some_and(|s| changed.iter().any(|c| c.id == s.id()));
    report.changed_budgets = Some(changed);
    report.library_version = Some(after.version);
    Ok(cloud.sync.finish(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;
    fn call(runtime: &Runtime, method: &str, params: serde_json::Value) -> serde_json::Value {
        crate::block_on(runtime.dispatch(method, params)).unwrap()
    }
    #[test]
    fn closed_budget_capture_is_readonly_for_another_open_window() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path()).unwrap();
        let id = call(
            &runtime,
            "create_budget",
            json!({"budget":urbudget_core::test_support::sample()}),
        );
        let id = id.as_str().unwrap();
        let path = runtime.library.lock().database(id).unwrap();
        let mut other = Store::open_budget(&path, id).unwrap();
        let before = other.status().unwrap().token;
        let closed_db = Database::new(None);
        let first = capture(&closed_db, &runtime.library).unwrap();
        capture(&closed_db, &runtime.library).unwrap();
        assert_eq!(
            other.status().unwrap().token,
            before,
            "notification reads must not write a schema header"
        );
        let mut changed = urbudget_core::test_support::sample();
        changed.name = "Actual saved edit".into();
        other.save(&changed, &before).unwrap();
        let after = capture(&closed_db, &runtime.library).unwrap();
        assert_ne!(first.entries.get(id), after.entries.get(id));
        call(&runtime, "library_open", json!({"id":id}));
        let live = capture(&runtime.db, &runtime.library).unwrap();
        assert_eq!(
            live.entries[id].1,
            runtime.db.store().unwrap().status().unwrap().token
        );
    }
    #[test]
    fn committed_change_survives_later_failure_and_targets_its_identity() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path()).unwrap();
        let id = call(
            &runtime,
            "create_budget",
            json!({"budget":urbudget_core::test_support::sample()}),
        );
        call(&runtime, "library_open", json!({"id":id}));
        let before = capture(&runtime.db, &runtime.library).unwrap();
        let mut saved = call(&runtime, "load_budget", json!({}));
        saved["budget"]["name"] = json!("Committed merge");
        call(
            &runtime,
            "save_budget",
            json!({"budget":saved["budget"],"expectedToken":saved["token"]}),
        );
        let token = call(&runtime, "load_budget", json!({}))["token"].clone();
        let report = finish(
            &runtime.cloud,
            &runtime.db,
            &runtime.library,
            before,
            Err("Upload failed after merge".into()),
        )
        .unwrap();
        let changed = report.changed_budgets.unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(json!(changed[0].id), id);
        assert_eq!(json!(changed[0].revision), token);
        assert_eq!(report.state, "error");
        assert!(report.library_version.is_some());
    }
    #[test]
    fn deleted_last_budget_invalidates_old_identity_after_it_is_closed() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path()).unwrap();
        let id = call(
            &runtime,
            "create_budget",
            json!({"budget":urbudget_core::test_support::sample()}),
        );
        call(&runtime, "library_open", json!({"id":id}));
        let before = capture(&runtime.db, &runtime.library).unwrap();
        call(&runtime, "library_trash", json!({"id":id}));
        let report = finish(
            &runtime.cloud,
            &runtime.db,
            &runtime.library,
            before,
            Err("Later network failure".into()),
        )
        .unwrap();
        assert!(!runtime.db.is_open());
        assert_eq!(report.changed_budgets.unwrap()[0].id, id.as_str().unwrap());
    }
    #[test]
    fn failed_review_without_mutation_keeps_its_original_error() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Runtime::new(dir.path()).unwrap();
        let before = capture(&runtime.db, &runtime.library).unwrap();
        let error = finish(
            &runtime.cloud,
            &runtime.db,
            &runtime.library,
            before,
            Err("Review changed".into()),
        )
        .unwrap_err();
        assert_eq!(error, "Review changed");
    }
}
