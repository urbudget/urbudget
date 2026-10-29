//! Durable, account-bound sync state. Network delivery belongs to services; these methods
//! deliberately never advance a library delivery sequence or claim a cross-file transaction.
use crate::{domain::Budget, store::Store};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

type Result<T> = std::result::Result<T, String>;
pub const STORAGE_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteIdentity {
    pub account_id: String,
    pub key_generation: String,
    pub catalog_epoch: String,
    pub budget_id: String,
    pub incarnation: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        budgets::{Budgets, LifecycleJob},
        test_support::sample,
    };
    use serde_json::json;
    fn identity() -> RemoteIdentity {
        RemoteIdentity {
            account_id: "account-a".into(),
            key_generation: "1".into(),
            catalog_epoch: "epoch-one".into(),
            budget_id: "remote-a".into(),
            incarnation: "incarnation-one".into(),
        }
    }
    fn save(store: &mut Store, budget: &Budget) -> crate::store::Status {
        let token = store.status().unwrap().token;
        store.save(budget, &token).unwrap()
    }
    fn bound(store: &mut Store) -> Binding {
        let budget = sample();
        let status = save(store, &budget);
        let binding = Binding {
            identity: identity(),
            remote_version: "1".into(),
            acknowledged_local_revision: status.revision,
            base: budget,
        };
        store.bind_sync(&binding).unwrap();
        binding
    }
    fn prepare(store: &mut Store, name: &str) -> UploadIntent {
        let mut candidate = store.saved().unwrap().0;
        candidate.name = name.into();
        let status = save(store, &candidate);
        let intent = UploadIntent {
            operation_id: "operation-one".into(),
            identity: identity(),
            expected_version: "1".into(),
            local_revision: status.revision,
            candidate,
            manifest: json!({"kind":"update"}),
            payload: vec![1, 2, 3],
            metadata: "sealed metadata".into(),
            digest: "request-digest".into(),
        };
        store.prepare_upload(&intent, &status.token).unwrap();
        intent
    }

    #[test]
    fn unknown_upload_survives_restart_and_acknowledges_only_captured_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("budget.sqlite3");
        let mut store = Store::open_budget(&path, "local-a").unwrap();
        bound(&mut store);
        let intent = prepare(&mut store, "Uploaded snapshot");
        let mut later = intent.candidate.clone();
        later.name = "Later local edit".into();
        save(&mut store, &later);
        drop(store);
        let mut store = Store::open_budget(&path, "local-a").unwrap();
        assert_eq!(store.pending_upload().unwrap(), Some(intent.clone()));
        let receipt = json!({"accepted":true,"version":"2"});
        store
            .acknowledge_upload(&intent.operation_id, &intent.digest, Some("2"), &receipt)
            .unwrap();
        assert_eq!(
            store.sync_binding().unwrap().unwrap().base,
            intent.candidate
        );
        assert_eq!(store.saved().unwrap().0, later);
        assert!(store.sync_dirty().unwrap());
        store
            .acknowledge_upload(&intent.operation_id, &intent.digest, Some("2"), &receipt)
            .unwrap();
        assert!(store.pending_upload().unwrap().is_none());
    }
    #[test]
    fn storage_relocation_preserves_an_unknown_upload_and_its_ancestry() {
        let d = tempfile::tempdir().unwrap();
        let mut old = Store::open_budget(&d.path().join("budget.sqlite3"), "local-a").unwrap();
        let binding = bound(&mut old);
        let intent = prepare(&mut old, "Pending migration");
        std::fs::write(
            d.path().join("budgets.json"),
            serde_json::to_vec(
                &json!({"current":"local-a","budgets":[{"id":"local-a","folder":""}]}),
            )
            .unwrap(),
        )
        .unwrap();
        let catalog = Budgets::load(d.path()).unwrap();
        let mut moved =
            Store::open_budget(&catalog.database("local-a").unwrap(), "local-a").unwrap();
        assert_eq!(moved.pending_upload().unwrap(), Some(intent.clone()));
        assert_eq!(moved.sync_binding().unwrap(), Some(binding));
        let receipt = json!({"outcome":"accepted","version":"2"});
        assert!(old
            .acknowledge_upload(&intent.operation_id, &intent.digest, Some("2"), &receipt)
            .is_err());
        moved
            .acknowledge_upload(&intent.operation_id, &intent.digest, Some("2"), &receipt)
            .unwrap();
        assert!(!moved.sync_dirty().unwrap());
        assert_eq!(
            moved.sync_binding().unwrap().unwrap().base,
            intent.candidate
        );
    }
    #[test]
    fn rejected_receipt_keeps_ancestry_and_dirty_content() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("b.db")).unwrap();
        let base = bound(&mut s);
        let intent = prepare(&mut s, "Pending edit");
        let receipt = json!({"accepted":false,"code":"conflict"});
        s.acknowledge_upload(&intent.operation_id, &intent.digest, None, &receipt)
            .unwrap();
        assert_eq!(s.sync_binding().unwrap(), Some(base));
        assert!(s.sync_dirty().unwrap());
        assert_eq!(s.saved().unwrap().0, intent.candidate);
    }
    #[test]
    fn prepared_bytes_and_identity_are_immutable() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("b.db")).unwrap();
        bound(&mut s);
        let intent = prepare(&mut s, "Pending");
        let token = s.status().unwrap().token;
        let mut changed = intent.clone();
        changed.payload.push(9);
        assert!(s.prepare_upload(&changed, &token).is_err());
        assert_eq!(s.pending_upload().unwrap(), Some(intent));
        let mut other = s.sync_binding().unwrap().unwrap();
        other.identity.account_id = "other-account".into();
        assert!(s.bind_sync(&other).is_err());
    }
    #[test]
    fn an_unpublished_or_wrong_base_cannot_claim_saved_content_is_synced() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("b.db")).unwrap();
        let mut binding = bound(&mut s);
        binding.base.name = "Different content".into();
        assert!(s.bind_sync(&binding).is_err());
        binding.base = sample();
        binding.remote_version = "0".into();
        assert!(s.bind_sync(&binding).is_err());
    }
    #[test]
    fn plain_binding_cannot_bypass_incarnation_adoption_checks() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("b.db")).unwrap();
        let mut binding = bound(&mut s);
        binding.identity.incarnation = "restored".into();
        assert!(s.bind_sync(&binding).is_err());
        assert_eq!(s.sync_binding().unwrap().unwrap().identity, identity());
    }
    #[test]
    fn delivery_slot_cannot_substitute_bytes_for_the_prepared_budget_intent() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let id = l.create().unwrap();
        let mut s = Store::open_budget(&l.database(&id).unwrap(), &id).unwrap();
        bound(&mut s);
        let i = prepare(&mut s, "Pending");
        assert!(l
            .prepare_delivery(
                "account-a",
                &i.operation_id,
                Some(&id),
                &i.manifest,
                &[9, 9],
                &i.metadata,
                &i.digest
            )
            .is_err());
        assert!(l.pending_delivery("account-a").unwrap().is_none());
        assert_eq!(l.account_state("account-a").unwrap()["nextSequence"], "1");
    }
    #[test]
    fn merge_content_base_and_intent_roll_back_together() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("b.db")).unwrap();
        let original = bound(&mut s);
        let token = s.status().unwrap().token;
        let mut remote = original.base.clone();
        remote.currency = "EUR".into();
        let mut merged = remote.clone();
        merged.name = "Local and remote".into();
        let binding = Binding {
            remote_version: "2".into(),
            base: remote,
            ..original.clone()
        };
        let intent = UploadIntent {
            operation_id: "merge-operation".into(),
            identity: identity(),
            expected_version: "2".into(),
            local_revision: -1,
            candidate: merged.clone(),
            manifest: json!({}),
            payload: vec![4],
            metadata: String::new(),
            digest: "merge-digest".into(),
        };
        s.connection.execute_batch("CREATE TRIGGER fail_preparation BEFORE INSERT ON sync_upload_v2 BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
        assert!(s
            .save_sync_v2(&merged, &token, &binding, Some(&intent))
            .is_err());
        assert_eq!(s.saved().unwrap().0, original.base);
        assert_eq!(s.sync_binding().unwrap(), Some(original));
        assert!(s.pending_upload().unwrap().is_none());
    }
    #[test]
    fn global_sequence_waits_for_per_budget_ack_and_restarts_idempotently() {
        let d = tempfile::tempdir().unwrap();
        let mut library = Budgets::load(d.path()).unwrap();
        let id = library.create().unwrap();
        let mut s = Store::open_budget(&library.database(&id).unwrap(), &id).unwrap();
        bound(&mut s);
        library.mark_available(&id).unwrap();
        let intent = prepare(&mut s, "Upload");
        let slot = library
            .prepare_delivery(
                "account-a",
                &intent.operation_id,
                Some(&id),
                &intent.manifest,
                &intent.payload,
                &intent.metadata,
                &intent.digest,
            )
            .unwrap();
        let receipt = json!({"accepted":true,"version":"2"});
        library
            .record_delivery_receipt("account-a", &slot.sequence, &slot.digest, &receipt)
            .unwrap();
        assert!(library
            .complete_delivery("account-a", &slot.sequence, &slot.digest)
            .is_err());
        s.acknowledge_upload(&intent.operation_id, &intent.digest, Some("2"), &receipt)
            .unwrap();
        drop(library);
        let mut library = Budgets::load(d.path()).unwrap();
        library
            .complete_delivery("account-a", &slot.sequence, &slot.digest)
            .unwrap();
        library
            .complete_delivery("account-a", &slot.sequence, &slot.digest)
            .unwrap();
        assert_eq!(
            library.account_state("account-a").unwrap()["nextSequence"],
            "2"
        );
        assert!(library.pending_delivery("account-a").unwrap().is_none());
    }
    #[test]
    fn control_receipt_requires_durable_local_application() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let job = LifecycleJob {
            operation_id: "key-operation".into(),
            local_budget_id: None,
            kind: "keySetup".into(),
            payload: json!({}),
            state: "pending".into(),
            receipt: None,
        };
        l.enqueue_job(&job).unwrap();
        let slot = l
            .prepare_delivery(
                "account-a",
                &job.operation_id,
                None,
                &json!({"kind":"keySetup"}),
                &[],
                "",
                "digest",
            )
            .unwrap();
        let receipt = json!({"generation":"1"});
        l.record_delivery_receipt("account-a", &slot.sequence, &slot.digest, &receipt)
            .unwrap();
        assert!(l
            .complete_delivery("account-a", &slot.sequence, &slot.digest)
            .is_err());
        l.finish_job(&job.operation_id, &receipt).unwrap();
        l.complete_delivery("account-a", &slot.sequence, &slot.digest)
            .unwrap();
    }
    #[test]
    fn already_prepared_legacy_sql_is_fenced_after_schema_migration() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("b.db");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE metadata(id INTEGER PRIMARY KEY,revision INTEGER NOT NULL,saved_at TEXT NOT NULL,name TEXT NOT NULL,currency TEXT NOT NULL);INSERT INTO metadata VALUES(1,0,'','Old budget','SAR');PRAGMA user_version=2;").unwrap();
        let mut statement = legacy
            .prepare("UPDATE metadata SET name=?1 WHERE id=1")
            .unwrap();
        let mut modern = Store::open(&path).unwrap();
        assert!(statement.execute(["Legacy overwrite"]).is_err());
        assert!(legacy
            .execute("UPDATE metadata SET name='New legacy statement'", [])
            .is_err());
        let mut budget = modern.saved().unwrap().0;
        budget.name = "Modern save".into();
        save(&mut modern, &budget);
        assert_eq!(modern.saved().unwrap().0.name, "Modern save");
    }
    #[test]
    fn clean_restore_rebinds_but_dirty_or_checkpointed_content_is_preserved() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::open(&d.path().join("b.db")).unwrap();
        let mut binding = bound(&mut s);
        s.set_sync_blocked(true).unwrap();
        binding.identity.incarnation = "restored".into();
        binding.remote_version = "3".into();
        binding.base.name = "Restored".into();
        let token = s.status().unwrap().token;
        s.adopt_sync_v2(&binding.base, &token, &binding).unwrap();
        assert!(!s.sync_blocked().unwrap());
        let mut edited = binding.base.clone();
        edited.name = "Keep my edit".into();
        save(&mut s, &edited);
        binding.identity.incarnation = "restored-again".into();
        binding.remote_version = "5".into();
        let token = s.status().unwrap().token;
        assert!(s.adopt_sync_v2(&binding.base, &token, &binding).is_err());
        assert_eq!(s.saved().unwrap().0, edited);
        let checkpoint = EditorCheckpoint {
            editor_id: "editor".into(),
            epoch: "one".into(),
            process_owner: "process".into(),
            target: json!({"field":"amount"}),
            baseline: Value::Null,
            draft: json!({"amount":"-"}),
        };
        s.checkpoint_editor(&checkpoint).unwrap();
        s.set_sync_blocked(true).unwrap();
        assert_eq!(s.editor_checkpoints().unwrap(), [checkpoint]);
        assert!(s.save(&edited, &token).is_err());
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub identity: RemoteIdentity,
    pub remote_version: String,
    pub acknowledged_local_revision: i64,
    pub base: Budget,
}

/// The exact request prepared before HTTP. Neither a later save nor a retry may alter it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadIntent {
    pub operation_id: String,
    pub identity: RemoteIdentity,
    pub expected_version: String,
    pub local_revision: i64,
    pub candidate: Budget,
    pub manifest: Value,
    pub payload: Vec<u8>,
    pub metadata: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCheckpoint {
    pub editor_id: String,
    pub epoch: String,
    #[serde(default)]
    pub process_owner: String,
    pub target: Value,
    pub baseline: Value,
    pub draft: Value,
}

pub fn decimal(value: &str) -> Result<u64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("Invalid decimal sync version.".into());
    }
    value
        .parse()
        .map_err(|_| "Sync version exceeds the supported range.".into())
}
pub fn valid_identity(identity: &RemoteIdentity) -> Result<()> {
    if [
        &identity.account_id,
        &identity.budget_id,
        &identity.incarnation,
        &identity.catalog_epoch,
    ]
    .iter()
    .any(|s| s.is_empty() || s.len() > 256)
    {
        return Err("Incomplete sync identity.".into());
    }
    decimal(&identity.key_generation)?;
    Ok(())
}

pub(crate) fn register_writer(connection: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    connection
        .create_scalar_function(
            "urbudget_storage_protocol",
            0,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |_| Ok(STORAGE_SCHEMA_VERSION as i64),
        )
        .map_err(|e| e.to_string())
}

/// Called inside Store's schema transaction. A connection opened by an older application
/// cannot evaluate these triggers, including after SQLite reparses a cached old statement.
pub(crate) fn install_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch("
        CREATE TABLE IF NOT EXISTS sync_binding_v2(id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL CHECK(json_valid(data)));
        CREATE TABLE IF NOT EXISTS sync_upload_v2(operation_id TEXT PRIMARY KEY, data TEXT NOT NULL CHECK(json_valid(data)), state TEXT NOT NULL CHECK(state IN ('prepared','terminal')), receipt TEXT CHECK(receipt IS NULL OR json_valid(receipt)));
        CREATE UNIQUE INDEX IF NOT EXISTS sync_one_prepared_upload ON sync_upload_v2(state) WHERE state='prepared';
        CREATE TABLE IF NOT EXISTS sync_local_state(id INTEGER PRIMARY KEY CHECK(id=1), dirty INTEGER NOT NULL DEFAULT 1, blocked INTEGER NOT NULL DEFAULT 0, retired INTEGER NOT NULL DEFAULT 0);
        INSERT OR IGNORE INTO sync_local_state(id) VALUES(1);
        CREATE TABLE IF NOT EXISTS editor_checkpoints(editor_id TEXT PRIMARY KEY, epoch TEXT NOT NULL, data TEXT NOT NULL CHECK(json_valid(data)));
    ").map_err(|e| e.to_string())?;
    let has_retired = {
        let mut query = connection
            .prepare("PRAGMA table_info(sync_local_state)")
            .map_err(|e| e.to_string())?;
        let fields = query
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?;
        fields
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|name| name == "retired")
    };
    if !has_retired {
        connection
            .execute_batch(
                "ALTER TABLE sync_local_state ADD COLUMN retired INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(|e| e.to_string())?;
    }
    for table in [
        "metadata",
        "accounts",
        "categories",
        "transactions",
        "assignments",
        "assignment_extras",
        "budget_extras",
        "sync_base",
        "sync_binding_v2",
        "sync_upload_v2",
        "sync_local_state",
        "editor_checkpoints",
    ] {
        for action in ["INSERT", "UPDATE", "DELETE"] {
            connection.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS storage_v3_{table}_{action} BEFORE {action} ON {table} BEGIN SELECT CASE WHEN urbudget_storage_protocol() < 3 THEN RAISE(ABORT, 'This budget requires a newer UrBudget storage writer.') END; END;"))
                .map_err(|e| e.to_string())?;
        }
    }
    for table in [
        "accounts",
        "categories",
        "transactions",
        "assignments",
        "assignment_extras",
        "budget_extras",
    ] {
        for action in ["INSERT", "UPDATE", "DELETE"] {
            connection.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS sync_dirty_{table}_{action} AFTER {action} ON {table} BEGIN UPDATE sync_local_state SET dirty=1 WHERE id=1; END;"))
                .map_err(|e| e.to_string())?;
        }
    }
    for table in [
        "metadata",
        "accounts",
        "categories",
        "transactions",
        "assignments",
        "assignment_extras",
        "budget_extras",
        "sync_base",
        "sync_binding_v2",
        "sync_upload_v2",
        "editor_checkpoints",
    ] {
        for action in ["INSERT", "UPDATE", "DELETE"] {
            connection.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS retired_storage_{table}_{action} BEFORE {action} ON {table} WHEN (SELECT retired FROM sync_local_state WHERE id=1)=1 BEGIN SELECT RAISE(ABORT,'This budget moved to newer managed storage. Reopen UrBudget.'); END;")).map_err(|e|e.to_string())?;
        }
    }
    connection.execute_batch("CREATE TRIGGER IF NOT EXISTS sync_dirty_metadata AFTER UPDATE OF revision,name,currency ON metadata BEGIN UPDATE sync_local_state SET dirty=1 WHERE id=1; END;")
        .map_err(|e| e.to_string())
}

pub(crate) fn read_binding(connection: &Connection) -> Result<Option<Binding>> {
    let raw: Option<String> = connection
        .query_row("SELECT data FROM sync_binding_v2 WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| e.to_string())?;
    raw.map(|s| serde_json::from_str(&s).map_err(|e| format!("Unreadable sync ancestry: {e}")))
        .transpose()
}
pub(crate) fn write_binding(connection: &Connection, binding: &Binding) -> Result<()> {
    valid_identity(&binding.identity)?;
    decimal(&binding.remote_version)?;
    crate::domain::validate(&binding.base)?;
    connection.execute("INSERT INTO sync_binding_v2 VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
        [serde_json::to_string(binding).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) fn write_intent(connection: &Connection, intent: &UploadIntent) -> Result<()> {
    valid_identity(&intent.identity)?;
    decimal(&intent.expected_version)?;
    if intent.operation_id.is_empty() || intent.digest.is_empty() {
        return Err("Upload intent requires an operation ID and request digest.".into());
    }
    crate::domain::validate(&intent.candidate)?;
    let raw = serde_json::to_string(intent).map_err(|e| e.to_string())?;
    let existing: Option<(String, String)> = connection
        .query_row(
            "SELECT data,state FROM sync_upload_v2 WHERE operation_id=?1",
            [&intent.operation_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((saved, state)) = existing {
        return if saved == raw && state == "prepared" {
            Ok(())
        } else {
            Err("An immutable sync operation cannot be reused or changed.".into())
        };
    }
    connection
        .execute(
            "INSERT INTO sync_upload_v2(operation_id,data,state) VALUES(?1,?2,'prepared')",
            params![intent.operation_id, raw],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

impl Store {
    pub fn sync_binding(&self) -> Result<Option<Binding>> {
        read_binding(&self.connection)
    }

    /// Establishes verified ancestry. Callers must never infer this from budget names.
    pub fn bind_sync(&mut self, binding: &Binding) -> Result<()> {
        let (mut saved, status) = self.saved()?;
        for key in crate::store::HEAVY_EXTRAS {
            saved.extra.remove(*key);
        }
        if binding.acknowledged_local_revision == status.revision
            && (binding.remote_version == "0" || binding.base != saved)
        {
            return Err("Unpublished or different content cannot be marked acknowledged.".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        if tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sync_upload_v2 WHERE state='prepared')",
                [],
                |r| r.get::<_, bool>(0),
            )
            .map_err(|e| e.to_string())?
        {
            return Err("Resolve the pending sync operation before changing its binding.".into());
        }
        if let Some(previous) = read_binding(&tx)? {
            if previous.identity != binding.identity {
                return Err(
                    "Changing a sync identity requires checked adoption or a new local copy."
                        .into(),
                );
            }
            if decimal(&binding.remote_version)? < decimal(&previous.remote_version)? {
                return Err("An older remote version cannot replace verified ancestry.".into());
            }
        }
        write_binding(&tx, binding)?;
        let revision: i64 = tx
            .query_row("SELECT revision FROM metadata WHERE id=1", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE sync_local_state SET dirty=?1 WHERE id=1",
            [revision != binding.acknowledged_local_revision],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    pub fn sync_dirty(&self) -> Result<bool> {
        self.connection.query_row("SELECT dirty OR EXISTS(SELECT 1 FROM sync_upload_v2 WHERE state='prepared') FROM sync_local_state WHERE id=1", [], |r| r.get(0)).map_err(|e| e.to_string())
    }
    pub fn sync_blocked(&self) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT blocked OR retired FROM sync_local_state WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }
    pub fn set_sync_blocked(&self, blocked: bool) -> Result<()> {
        self.connection
            .execute(
                "UPDATE sync_local_state SET blocked=?1 WHERE id=1",
                [blocked],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub(crate) fn check_sync_writable(&self) -> Result<()> {
        if self.sync_blocked()? {
            Err("This budget was removed from its account. Recover your changes as a copy before editing.".into())
        } else {
            Ok(())
        }
    }
    pub fn pending_upload(&self) -> Result<Option<UploadIntent>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT data FROM sync_upload_v2 WHERE state='prepared'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        raw.map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .transpose()
    }
    pub fn upload_receipt(&self, operation_id: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT receipt FROM sync_upload_v2 WHERE operation_id=?1 AND state='terminal'",
                [operation_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        raw.map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .transpose()
    }
    pub fn prepare_upload(&mut self, intent: &UploadIntent, expected_token: &str) -> Result<()> {
        self.check_sync_writable()?;
        let (mut saved, status) = self.saved()?;
        for key in crate::store::HEAVY_EXTRAS {
            saved.extra.remove(*key);
        }
        if status.token != expected_token
            || status.revision != intent.local_revision
            || saved != intent.candidate
        {
            return Err("The saved budget changed before its upload was prepared.".into());
        }
        let local_id = self.id().to_owned();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let revision: i64 = tx
            .query_row("SELECT revision FROM metadata WHERE id=1", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let data_version: i64 = tx
            .pragma_query_value(None, "data_version", |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if revision != intent.local_revision
            || format!("{local_id}:{revision}:{data_version}") != expected_token
        {
            return Err("The saved budget changed before its upload was prepared.".into());
        }
        if let Some(binding) = read_binding(&tx)? {
            if binding.identity != intent.identity
                || binding.remote_version != intent.expected_version
            {
                return Err("The upload does not match this budget's verified ancestry.".into());
            }
        } else if intent.expected_version != "0" {
            return Err("An existing remote budget requires verified ancestry.".into());
        }
        write_intent(&tx, intent)?;
        tx.commit().map_err(|e| e.to_string())
    }

    /// Accepted content advances ancestry to the *captured* snapshot. Rejection/cancellation
    /// advances only the receipt, keeping the old base and dirty local content. The service
    /// completes its library delivery slot only after this transaction succeeds.
    pub fn acknowledge_upload(
        &mut self,
        operation_id: &str,
        digest: &str,
        accepted_version: Option<&str>,
        receipt: &Value,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        let row: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT data,state,receipt FROM sync_upload_v2 WHERE operation_id=?1",
                [operation_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let (raw, state, previous) =
            row.ok_or("The sync operation is missing; preserve this budget for recovery.")?;
        let intent: UploadIntent = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        if intent.digest != digest {
            return Err("The receipt does not match the immutable request.".into());
        }
        let raw_receipt = serde_json::to_string(receipt).map_err(|e| e.to_string())?;
        if state == "terminal" {
            return if previous.as_deref() == Some(&raw_receipt) {
                Ok(())
            } else {
                Err("The server returned a different result for a completed operation.".into())
            };
        }
        if let Some(version) = accepted_version {
            if decimal(version)? <= decimal(&intent.expected_version)? {
                return Err("Accepted sync version did not advance.".into());
            }
            if read_binding(&tx)?.is_some_and(|b| b.identity != intent.identity) {
                return Err(
                    "The pending operation belongs to another binding; preserve it for recovery."
                        .into(),
                );
            }
            write_binding(
                &tx,
                &Binding {
                    identity: intent.identity,
                    remote_version: version.into(),
                    acknowledged_local_revision: intent.local_revision,
                    base: intent.candidate,
                },
            )?;
            let revision: i64 = tx
                .query_row("SELECT revision FROM metadata WHERE id=1", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE sync_local_state SET dirty=?1 WHERE id=1",
                [revision != intent.local_revision],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE sync_upload_v2 SET state='terminal',receipt=?2 WHERE operation_id=?1",
            params![operation_id, raw_receipt],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
    /// Only after the library's delivery slot completion is durable.
    pub fn forget_completed_upload(&self, operation_id: &str) -> Result<()> {
        self.connection
            .execute(
                "DELETE FROM sync_upload_v2 WHERE operation_id=?1 AND state='terminal'",
                [operation_id],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn checkpoint_editor(&self, checkpoint: &EditorCheckpoint) -> Result<()> {
        if checkpoint.editor_id.is_empty() || checkpoint.epoch.is_empty() {
            return Err("An editor checkpoint requires its identity and epoch.".into());
        }
        self.connection.execute("INSERT INTO editor_checkpoints VALUES(?1,?2,?3) ON CONFLICT(editor_id) DO UPDATE SET epoch=excluded.epoch,data=excluded.data",
            params![checkpoint.editor_id,checkpoint.epoch,serde_json::to_string(checkpoint).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn editor_checkpoints(&self) -> Result<Vec<EditorCheckpoint>> {
        let mut statement = self
            .connection
            .prepare("SELECT data FROM editor_checkpoints ORDER BY editor_id")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        })
        .collect()
    }
    pub fn clear_editor_checkpoint(&self, editor_id: &str, epoch: &str) -> Result<()> {
        self.connection
            .execute(
                "DELETE FROM editor_checkpoints WHERE editor_id=?1 AND epoch=?2",
                params![editor_id, epoch],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
