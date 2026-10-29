//! Transactional local library. Only active selection is cached per application window;
//! catalog mutations always operate on fresh SQLite rows instead of replacing a JSON snapshot.
use crate::t;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub folder: String,
    #[serde(default)]
    pub last_opened: Option<String>,
}
#[derive(Default, Deserialize)]
struct Registry {
    budgets: Vec<Entry>,
    current: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub id: String,
    pub name: String,
    pub currency: String,
    pub current: bool,
    pub last_opened: Option<String>,
    pub path: String,
    pub missing: bool,
    pub scope: String,
    pub account_id: Option<String>,
    pub remote_id: Option<String>,
    pub incarnation: Option<String>,
    pub lifecycle: String,
    pub availability: String,
    pub pending: bool,
    pub status: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogHead {
    pub account_id: String,
    pub key_generation: String,
    pub catalog_epoch: String,
    pub budget_id: String,
    pub incarnation: String,
    pub version: String,
    pub state: String,
    pub name: String,
    pub currency: String,
    pub metadata: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleJob {
    pub operation_id: String,
    pub local_budget_id: Option<String>,
    pub kind: String,
    pub payload: Value,
    pub state: String,
    pub receipt: Option<Value>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverySlot {
    pub account_id: String,
    pub installation_id: String,
    pub sequence: String,
    pub operation_id: String,
    pub local_budget_id: Option<String>,
    pub manifest: Value,
    pub payload: Vec<u8>,
    pub metadata: String,
    pub digest: String,
    pub state: String,
    pub receipt: Option<Value>,
}
pub struct Budgets {
    data_dir: PathBuf,
    connection: Connection,
    current: String,
    local_leases: std::collections::HashMap<String, std::fs::File>,
}
fn try_local_lock(file: &std::fs::File) -> std::io::Result<bool> {
    #[cfg(not(target_os = "android"))]
    {
        match file.try_lock() {
            Ok(()) => Ok(true),
            Err(std::fs::TryLockError::WouldBlock) => Ok(false),
            Err(std::fs::TryLockError::Error(e)) => Err(e),
        }
    }
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        loop {
            // SAFETY: file owns the descriptor and closing the handle releases this lease.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(true);
            }
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(false);
            }
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}
fn migration_lock(file: &std::fs::File) -> std::io::Result<()> {
    #[cfg(not(target_os = "android"))]
    {
        file.lock()
    }
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        loop {
            // SAFETY: the file owns its descriptor, and closing it releases this advisory lock.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Ok(());
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e);
            }
        }
    }
}
fn file_hash(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(error)?;
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        let n = file.read(&mut bytes).map_err(error)?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
/// Publish a flushed file and its directory entry before recording the new path in SQLite.
fn publish_file(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        }
        let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: these are NUL-terminated UTF-16 paths valid for the call. WRITE_THROUGH
        // completes the rename before the library transaction can publish its pathname.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 {
            return Err(error(std::io::Error::last_os_error()));
        }
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(source, destination).map_err(error)?;
        if let Some(parent) = destination.parent() {
            std::fs::File::open(parent)
                .map_err(error)?
                .sync_all()
                .map_err(error)?;
        }
    }
    Ok(())
}
fn copy_preserved(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    let meta = std::fs::symlink_metadata(source).map_err(error)?;
    if meta.file_type().is_symlink()
        || std::fs::symlink_metadata(destination).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("Migration will not follow a link outside managed budget storage.".into());
    }
    if meta.is_dir() {
        std::fs::create_dir_all(destination).map_err(error)?;
        for item in std::fs::read_dir(source).map_err(error)? {
            let item = item.map_err(error)?;
            copy_preserved(&item.path(), &destination.join(item.file_name()))?;
        }
        return Ok(());
    }
    if destination.exists() && file_hash(source)? == file_hash(destination)? {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or("Missing migration destination folder.")?;
    std::fs::create_dir_all(parent).map_err(error)?;
    let name = destination
        .file_name()
        .ok_or("Missing migration filename.")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.migration-copy"));
    if std::fs::symlink_metadata(&temporary).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Migration will not overwrite a linked temporary file.".into());
    }
    // A partial temporary copy is owned by this replayable operation. Never expose it at
    // the real filename; kill/disk-full retries simply recopy and verify it.
    std::fs::copy(source, &temporary).map_err(error)?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .map_err(error)?
        .sync_all()
        .map_err(error)?;
    if file_hash(source)? != file_hash(&temporary)? {
        return Err("A migration source changed while it was copied; retrying is safe.".into());
    }
    if destination.exists() {
        // Preserve divergent pre-existing bytes, including partial copies made by an older
        // migration build. They cannot permanently block access to the verified database.
        let hash = file_hash(destination)?;
        let preserved = parent.join(format!("{name}.{hash}.preserved"));
        if preserved.exists() && file_hash(&preserved)? != hash {
            return Err("A preserved migration copy changed; review both files.".into());
        }
        publish_file(destination, &preserved)?;
    }
    publish_file(&temporary, destination)
}
fn copy_budget_sidecars(
    source: &Path,
    target: &Path,
    root_budget: bool,
    completed: &mut std::collections::BTreeSet<String>,
    checkpoint: &mut impl FnMut(&std::collections::BTreeSet<String>) -> Result<()>,
) -> Result<()> {
    fn walk(
        source: &Path,
        target: &Path,
        relative: &Path,
        completed: &mut std::collections::BTreeSet<String>,
        checkpoint: &mut impl FnMut(&std::collections::BTreeSet<String>) -> Result<()>,
    ) -> Result<()> {
        if !source.exists() {
            return Ok(());
        }
        let metadata = std::fs::symlink_metadata(source).map_err(error)?;
        if metadata.file_type().is_symlink()
            || std::fs::symlink_metadata(target).is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err("Migration will not follow a link outside managed budget storage.".into());
        }
        if metadata.is_dir() {
            std::fs::create_dir_all(target).map_err(error)?;
            for item in std::fs::read_dir(source).map_err(error)? {
                let item = item.map_err(error)?;
                walk(
                    &item.path(),
                    &target.join(item.file_name()),
                    &relative.join(item.file_name()),
                    completed,
                    checkpoint,
                )?;
            }
        } else {
            let key = relative.to_string_lossy().into_owned();
            // Once published and journaled, a sidecar belongs to the new application.
            // A retry must not replace a newer chat/draft or resurrect a deleted attachment.
            if completed.contains(&key) {
                return Ok(());
            }
            copy_preserved(source, target)?;
            completed.insert(key);
            checkpoint(completed)?;
        }
        Ok(())
    }
    if root_budget {
        // The single-budget legacy root also holds account credentials and other budgets.
        for owned in [
            "recovery.json",
            "backups",
            "imports",
            "exports",
            "assistant",
        ] {
            walk(
                &source.join(owned),
                &target.join(owned),
                Path::new(owned),
                completed,
                checkpoint,
            )?;
        }
    } else {
        for item in std::fs::read_dir(source).map_err(error)? {
            let item = item.map_err(error)?;
            if matches!(
                item.file_name().to_str(),
                Some(
                    "budget.sqlite3"
                        | "budget.sqlite3-wal"
                        | "budget.sqlite3-shm"
                        | "budget.sqlite3-journal"
                )
            ) {
                continue;
            }
            walk(
                &item.path(),
                &target.join(item.file_name()),
                Path::new(&item.file_name()),
                completed,
                checkpoint,
            )?;
        }
    }
    Ok(())
}
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static MADE: AtomicU64 = AtomicU64::new(0);
    let mut random = std::collections::hash_map::RandomState::new().build_hasher();
    random.write_u64(MADE.fetch_add(1, Ordering::Relaxed));
    format!(
        "b{:x}{:08x}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        random.finish() as u32
    )
}
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Err(t!("That budget id isn't valid."))
    } else {
        Ok(())
    }
}
fn folder_path(root: &Path, folder: &str) -> Result<PathBuf> {
    if !folder.is_empty()
        && Path::new(folder)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Unsafe budget storage path.".into());
    }
    Ok(root.join(folder))
}
fn bump(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE app_state SET library_version=library_version+1 WHERE id=1",
        [],
    )
    .map_err(error)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::Store, test_support::sample};
    fn save(store: &mut Store, budget: &crate::domain::Budget) {
        let token = store.status().unwrap().token;
        store.save(budget, &token).unwrap();
    }
    #[test]
    fn empty_library_is_valid_and_stale_instances_preserve_new_rows() {
        let d = tempfile::tempdir().unwrap();
        let mut first = Budgets::load(d.path()).unwrap();
        assert_eq!(first.current(), "");
        assert!(first.list().is_empty());
        let mut stale = Budgets::load(d.path()).unwrap();
        let one = first.create().unwrap();
        stale.set_current(&one).unwrap();
        let two = first.create().unwrap();
        stale.set_current(&one).unwrap();
        drop(first);
        drop(stale);
        let mut reopened = Budgets::load(d.path()).unwrap();
        assert_eq!(reopened.list().len(), 2);
        assert!(reopened.contains(&two));
        reopened.remove(&one).unwrap();
        reopened.remove(&two).unwrap();
        assert_eq!(reopened.current(), "");
        assert!(reopened.list().is_empty());
        drop(reopened);
        assert!(Budgets::load(d.path()).unwrap().list().is_empty());
    }
    #[test]
    fn account_enrollment_survives_creation_before_file_initialization() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let id = l.create_owned(Some("account")).unwrap();
        drop(l);
        let l = Budgets::load(d.path()).unwrap();
        let rows = l.list_all();
        let row = rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(row.account_id.as_deref(), Some("account"));
        assert!(row.remote_id.is_some() && row.incarnation.is_some());
        assert!(row.missing && row.pending);
        assert!(l.job(&format!("initialize-{id}")).unwrap().is_some());
    }
    #[test]
    fn incomplete_creation_and_download_are_repaired_only_after_owner_lease_ends() {
        let d = tempfile::tempdir().unwrap();
        let mut owner = Budgets::load(d.path()).unwrap();
        let id = owner.create_owned(Some("account")).unwrap();
        let observer = Budgets::load(d.path()).unwrap();
        let operation = format!("initialize-{id}");
        assert!(owner
            .claim_abandoned_local_job(&operation)
            .unwrap()
            .is_none());
        assert!(observer
            .claim_abandoned_local_job(&operation)
            .unwrap()
            .is_none());
        drop(owner);
        let lease = observer
            .claim_abandoned_local_job(&operation)
            .unwrap()
            .expect("creator is no longer live");
        let mut competitor = Budgets::load(d.path()).unwrap();
        assert!(competitor
            .claim_abandoned_local_job(&operation)
            .unwrap()
            .is_none());
        drop(lease);
        let id = competitor
            .observe_catalog(&head("account", "inc", "1", "active"))
            .unwrap();
        let operation = format!("download-{id}");
        competitor
            .enqueue_job(&LifecycleJob {
                operation_id: operation.clone(),
                local_budget_id: Some(id),
                kind: "download".into(),
                payload: json!({}),
                state: "pending".into(),
                receipt: None,
            })
            .unwrap();
        assert!(observer
            .claim_abandoned_local_job(&operation)
            .unwrap()
            .is_none());
        drop(competitor);
        assert!(observer
            .claim_abandoned_local_job(&operation)
            .unwrap()
            .is_some());
    }
    fn head(account: &str, incarnation: &str, version: &str, state: &str) -> CatalogHead {
        CatalogHead {
            account_id: account.into(),
            key_generation: "1".into(),
            catalog_epoch: "opaque-epoch".into(),
            budget_id: "remote".into(),
            incarnation: incarnation.into(),
            version: version.into(),
            state: state.into(),
            name: "Household".into(),
            currency: "SAR".into(),
            metadata: json!({}),
        }
    }
    #[test]
    fn deletion_pins_displayed_ancestry_and_review_cancellation_is_atomic() {
        let d = tempfile::tempdir().unwrap();
        let mut catalog = Budgets::load(d.path()).unwrap();
        let id = catalog
            .observe_catalog(&head("account", "inc", "1", "active"))
            .unwrap();
        let path = catalog.database(&id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut store = Store::open_budget(&path, &id).unwrap();
        save(&mut store, &sample());
        let revision = store.status().unwrap().revision;
        store
            .bind_sync(&crate::sync_storage::Binding {
                identity: crate::sync_storage::RemoteIdentity {
                    account_id: "account".into(),
                    key_generation: "1".into(),
                    catalog_epoch: "opaque-epoch".into(),
                    budget_id: "remote".into(),
                    incarnation: "inc".into(),
                },
                remote_version: "1".into(),
                acknowledged_local_revision: revision,
                base: sample(),
            })
            .unwrap();
        catalog.mark_available(&id).unwrap();
        // Metadata may already describe an incoming edit still waiting behind an editor.
        catalog
            .observe_catalog(&head("account", "inc", "2", "active"))
            .unwrap();
        catalog.remove(&id).unwrap();
        let job = catalog.latest_trash_job(&id).unwrap().unwrap();
        assert_eq!(job.payload["reviewedHead"]["version"], "1");
        drop(catalog);
        let mut catalog = Budgets::load(d.path()).unwrap();
        assert_eq!(catalog.latest_trash_job(&id).unwrap(), Some(job.clone()));
        let conflict = LifecycleJob {
            operation_id: "deletion-review".into(),
            local_budget_id: Some(id.clone()),
            kind: "deletion_conflict".into(),
            payload: json!({"accountId":"account"}),
            state: "recovery".into(),
            receipt: None,
        };
        catalog.enqueue_job(&conflict).unwrap();
        catalog.connection.execute_batch("CREATE TRIGGER reject_review BEFORE UPDATE OF lifecycle ON local_budgets WHEN NEW.lifecycle='active' BEGIN SELECT RAISE(ABORT,'injected review failure'); END;").unwrap();
        assert!(catalog.review_deletion(&id).is_err());
        assert_eq!(
            catalog.job(&conflict.operation_id).unwrap(),
            Some(conflict.clone())
        );
        assert_eq!(catalog.job(&job.operation_id).unwrap(), Some(job));
        catalog
            .connection
            .execute_batch("DROP TRIGGER reject_review;")
            .unwrap();
        catalog.review_deletion(&id).unwrap();
        assert_eq!(catalog.list_all()[0].lifecycle, "active");
        assert!(catalog
            .jobs()
            .unwrap()
            .iter()
            .all(|j| j.local_budget_id.as_deref() != Some(&id)));
    }
    #[test]
    fn catalog_is_account_scoped_and_quarantine_keeps_the_old_copy() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let old = l.observe_catalog(&head("a", "old", "1", "active")).unwrap();
        let other = l
            .observe_catalog(&head("b", "other", "1", "active"))
            .unwrap();
        assert_ne!(old, other);
        std::fs::create_dir_all(l.database(&old).unwrap().parent().unwrap()).unwrap();
        let mut s = Store::open_budget(&l.database(&old).unwrap(), &old).unwrap();
        save(&mut s, &sample());
        l.mark_available(&old).unwrap();
        l.set_current(&old).unwrap();
        l.quarantine_binding(&old, &json!({"reason":"remote restore with local edits"}))
            .unwrap();
        let restored = l.observe_catalog(&head("a", "new", "3", "active")).unwrap();
        assert_ne!(old, restored);
        assert_eq!(s.saved().unwrap().0, sample());
        assert_eq!(l.current(), "");
        let row = l.list_all().into_iter().find(|r| r.id == old).unwrap();
        assert_eq!(row.lifecycle, "recovery");
        assert!(row.account_id.is_none());
        assert_eq!(
            l.recovery_hint(&old).unwrap().unwrap()["quarantinedOrigin"]["incarnation"],
            "old"
        );
        l.observe_catalog(&head("a", "new", "4", "trashed"))
            .unwrap();
        l.observe_catalog(&head("a", "old", "2", "active")).unwrap();
        assert_eq!(l.catalog_head("a", "remote").unwrap().unwrap().version, "4");
        assert!(!l.contains(&restored));
        assert!(l.contains(&other));
    }
    #[test]
    fn migration_copies_live_wal_and_recovery_then_fences_both_old_writers() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("budget.sqlite3");
        let mut old = Store::open_budget(&source, "legacy").unwrap();
        save(&mut old, &sample());
        std::fs::write(d.path().join("recovery.json"), br#"{"draft":"-"}"#).unwrap();
        std::fs::create_dir(d.path().join("backups")).unwrap();
        std::fs::write(d.path().join("backups/keep.json"), b"retained backup").unwrap();
        std::fs::write(
            d.path().join("budgets.json"),
            serde_json::to_vec(
                &json!({"current":"legacy","budgets":[{"id":"legacy","folder":""}]}),
            )
            .unwrap(),
        )
        .unwrap();
        let raw_old = Connection::open(&source).unwrap();
        let l = Budgets::load(d.path()).unwrap();
        let destination = l.database("legacy").unwrap();
        assert_ne!(destination, source);
        assert!(source.is_file());
        assert_eq!(
            std::fs::read(destination.parent().unwrap().join("recovery.json")).unwrap(),
            br#"{"draft":"-"}"#
        );
        assert_eq!(
            std::fs::read(destination.parent().unwrap().join("backups/keep.json")).unwrap(),
            b"retained backup"
        );
        assert!(raw_old
            .execute("UPDATE metadata SET name='old raw writer' WHERE id=1", [])
            .is_err());
        let mut edit = sample();
        edit.name = "old compatible writer".into();
        let token = old.status().unwrap().token;
        assert!(old.save(&edit, &token).is_err());
        let mut modern = Store::open_budget(&destination, "legacy").unwrap();
        assert_eq!(modern.saved().unwrap().0, sample());
        edit.name = "new storage".into();
        save(&mut modern, &edit);
        drop(l);
        let l = Budgets::load(d.path()).unwrap();
        assert_eq!(l.database("legacy").unwrap(), destination);
        assert_eq!(modern.saved().unwrap().0, edit);
        assert_eq!(l.job("migrate-legacy").unwrap().unwrap().state, "complete");
    }
    #[test]
    fn migration_replays_after_snapshot_publication_before_catalog_commit() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("budget.sqlite3");
        let mut s = Store::open_budget(&source, "legacy").unwrap();
        save(&mut s, &sample());
        std::fs::write(
            d.path().join("budgets.json"),
            serde_json::to_vec(
                &json!({"current":"legacy","budgets":[{"id":"legacy","folder":""}]}),
            )
            .unwrap(),
        )
        .unwrap();
        let l = Budgets::load(d.path()).unwrap();
        let target = l.database("legacy").unwrap();
        let mut job = l.job("migrate-legacy").unwrap().unwrap();
        job.state = "staged".into();
        job.receipt = None;
        l.connection
            .execute("UPDATE local_budgets SET folder='' WHERE id='legacy'", [])
            .unwrap();
        l.connection.execute("UPDATE lifecycle_jobs SET state='staged',data=?1 WHERE operation_id='migrate-legacy'",[serde_json::to_string(&job).unwrap()]).unwrap();
        drop(l);
        let l = Budgets::load(d.path()).unwrap();
        assert_eq!(l.database("legacy").unwrap(), target);
        assert_eq!(l.job("migrate-legacy").unwrap().unwrap().state, "complete");
    }
    #[test]
    fn late_legacy_creations_are_recovered_without_replacing_catalog_selection() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let current = l.create().unwrap();
        let mut selected = Store::open_budget(&l.database(&current).unwrap(), &current).unwrap();
        save(&mut selected, &sample());
        l.mark_available(&current).unwrap();
        l.set_current(&current).unwrap();
        drop(l);
        let legacy = d.path().join("budgets/late/budget.sqlite3");
        let mut s = Store::open_budget(&legacy, "late").unwrap();
        save(&mut s, &sample());
        std::fs::write(
            d.path().join("budgets.json"),
            serde_json::to_vec(
                &json!({"current":"late","budgets":[{"id":"late","folder":"budgets/late"}]}),
            )
            .unwrap(),
        )
        .unwrap();
        let l = Budgets::load(d.path()).unwrap();
        assert_eq!(l.current(), current);
        assert!(l.contains(&current));
        assert!(!l.contains("late"));
        let row = l.list_all().into_iter().find(|r| r.id == "late").unwrap();
        assert_eq!(row.lifecycle, "recovery");
        assert!(row.account_id.is_none());
        assert_ne!(l.database("late").unwrap(), legacy);
        assert!(legacy.is_file());
        assert_eq!(
            l.recovery_hint("late").unwrap().unwrap()["quarantineReason"]["reason"],
            "created_by_legacy_process_after_migration"
        );
    }
    #[test]
    fn eviction_freezes_other_writers_and_preserves_a_backup() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let id = l.create_owned(Some("account")).unwrap();
        let row = l.list_all().pop().unwrap();
        let path = l.database(&id).unwrap();
        let mut s = Store::open_budget(&path, &id).unwrap();
        save(&mut s, &sample());
        let revision = s.revision().unwrap();
        s.bind_sync(&crate::sync_storage::Binding {
            identity: crate::sync_storage::RemoteIdentity {
                account_id: "account".into(),
                key_generation: "1".into(),
                catalog_epoch: "epoch".into(),
                budget_id: row.remote_id.unwrap(),
                incarnation: row.incarnation.unwrap(),
            },
            remote_version: "1".into(),
            acknowledged_local_revision: revision,
            base: sample(),
        })
        .unwrap();
        l.mark_available(&id).unwrap();
        // On Windows an open reader can postpone physical removal; the frozen state and
        // journal must survive either result, and its existing Store can never save again.
        let _attempt = l.evict(&id);
        let mut changed = sample();
        changed.name = "Late write".into();
        let token = s.status().unwrap().token;
        assert!(s.save(&changed, &token).is_err());
        assert!(s.replace_draft(changed).is_err());
        drop(s);
        l.evict(&id).unwrap();
        assert!(!path.exists());
        assert_eq!(l.list_all()[0].availability, "evicted");
        assert!(std::fs::read_dir(path.parent().unwrap().join("backups"))
            .unwrap()
            .next()
            .is_some());
    }
    #[test]
    fn load_resumes_eviction_after_retirement_before_file_removal() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let id = l.create_owned(Some("account")).unwrap();
        let row = l.list_all().pop().unwrap();
        let path = l.database(&id).unwrap();
        let mut s = Store::open_budget(&path, &id).unwrap();
        save(&mut s, &sample());
        s.bind_sync(&crate::sync_storage::Binding {
            identity: crate::sync_storage::RemoteIdentity {
                account_id: "account".into(),
                key_generation: "1".into(),
                catalog_epoch: "epoch".into(),
                budget_id: row.remote_id.unwrap(),
                incarnation: row.incarnation.unwrap(),
            },
            remote_version: "1".into(),
            acknowledged_local_revision: s.revision().unwrap(),
            base: sample(),
        })
        .unwrap();
        l.mark_available(&id).unwrap();
        l.enqueue_job(&LifecycleJob {
            operation_id: format!("evict-{id}"),
            local_budget_id: Some(id.clone()),
            kind: "evict".into(),
            payload: json!({"path":path}),
            state: "pending".into(),
            receipt: None,
        })
        .unwrap();
        s.connection
            .execute("UPDATE sync_local_state SET retired=1 WHERE id=1", [])
            .unwrap();
        l.set_current(&id).unwrap(); // Another window selected it before noticing retirement.
        drop(s);
        drop(l);
        let l = Budgets::load(d.path()).unwrap();
        assert_eq!(l.current(), "");
        assert_eq!(l.list_all()[0].availability, "evicted");
        assert!(!path.exists());
        assert!(l.job(&format!("evict-{id}")).unwrap().is_none());
        assert!(std::fs::read_dir(path.parent().unwrap().join("backups"))
            .unwrap()
            .next()
            .is_some());
    }
}
#[cfg(test)]
mod migration_failure_tests {
    use super::*;
    use crate::{store::Store, test_support::sample};
    fn save(s: &mut Store) {
        let token = s.status().unwrap().token;
        s.save(&sample(), &token).unwrap();
    }
    #[test]
    fn corrupt_legacy_source_is_isolated_while_other_budgets_migrate() {
        let d = tempfile::tempdir().unwrap();
        let good = d.path().join("budgets/good/budget.sqlite3");
        let mut s = Store::open_budget(&good, "good").unwrap();
        save(&mut s);
        let bad = d.path().join("budgets/bad/budget.sqlite3");
        std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
        std::fs::write(&bad, b"not a SQLite database").unwrap();
        std::fs::write(d.path().join("budgets.json"),serde_json::to_vec(&json!({"current":"bad","budgets":[{"id":"bad","folder":"budgets/bad"},{"id":"good","folder":"budgets/good"}]})).unwrap()).unwrap();
        let l = Budgets::load(d.path()).unwrap();
        assert_eq!(l.current(), "");
        assert!(l.contains("good"));
        assert_ne!(l.database("good").unwrap(), good);
        let row = l.list_all().into_iter().find(|r| r.id == "bad").unwrap();
        assert_eq!(
            (row.lifecycle.as_str(), row.availability.as_str()),
            ("active", "unavailable")
        );
        assert_eq!(std::fs::read(bad).unwrap(), b"not a SQLite database");
        let job = l.job("migrate-bad").unwrap().unwrap();
        assert_eq!(job.state, "recovery");
        assert_eq!(job.payload["sourceFenced"], false);
        assert!(job.payload["error"].is_string());
    }
    #[test]
    fn partial_sidecar_copies_are_replaced_atomically_and_divergent_bytes_survive() {
        let d = tempfile::tempdir().unwrap();
        let mut source = Store::open_budget(&d.path().join("budget.sqlite3"), "legacy").unwrap();
        save(&mut source);
        std::fs::write(d.path().join("recovery.json"), b"original draft").unwrap();
        let target = d.path().join("v2/budgets/legacy");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("recovery.json"), b"partial draft").unwrap();
        std::fs::write(
            target.join(".recovery.json.migration-copy"),
            b"interrupted temporary",
        )
        .unwrap();
        std::fs::write(
            d.path().join("budgets.json"),
            br#"{"current":"legacy","budgets":[{"id":"legacy","folder":""}]}"#,
        )
        .unwrap();
        let l = Budgets::load(d.path()).unwrap();
        assert_eq!(l.current(), "legacy");
        assert!(source.sync_blocked().unwrap());
        assert_eq!(
            std::fs::read(target.join("recovery.json")).unwrap(),
            b"original draft"
        );
        let archived = std::fs::read_dir(&target)
            .unwrap()
            .filter_map(|p| p.ok())
            .find(|p| p.file_name().to_string_lossy().ends_with(".preserved"))
            .unwrap();
        assert_eq!(std::fs::read(archived.path()).unwrap(), b"partial draft");
        assert!(!target.join(".recovery.json.migration-copy").exists());
        assert_eq!(l.list_all()[0].lifecycle, "active");
        assert_eq!(l.job("migrate-legacy").unwrap().unwrap().state, "complete");
        drop(l);
        let l = Budgets::load(d.path()).unwrap();
        assert_eq!(l.current(), "legacy");
    }
    #[test]
    fn assistant_and_future_sidecars_survive_both_legacy_layouts() {
        for root_budget in [false, true] {
            let d = tempfile::tempdir().unwrap();
            let folder = if root_budget { "" } else { "budgets/legacy" };
            let source = d.path().join(folder);
            let mut store = Store::open_budget(&source.join("budget.sqlite3"), "legacy").unwrap();
            save(&mut store);
            for (path, bytes) in [
                (
                    "assistant/threads/chat.json",
                    b"pending proposal".as_slice(),
                ),
                (
                    "assistant/files/statement.pdf",
                    b"statement attachment".as_slice(),
                ),
                ("backups/keep.json", b"backup".as_slice()),
            ] {
                let path = source.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, bytes).unwrap();
            }
            if !root_budget {
                std::fs::write(source.join("future-sidecar.txt"), b"keep future data").unwrap();
            }
            std::fs::write(d.path().join("account.json"), b"account-wide identity").unwrap();
            std::fs::write(
                d.path().join("budgets.json"),
                serde_json::to_vec(
                    &json!({"current":"legacy","budgets":[{"id":"legacy","folder":folder}]}),
                )
                .unwrap(),
            )
            .unwrap();
            let library = Budgets::load(d.path()).unwrap();
            let target = library
                .database("legacy")
                .unwrap()
                .parent()
                .unwrap()
                .to_owned();
            assert_eq!(
                std::fs::read(target.join("assistant/threads/chat.json")).unwrap(),
                b"pending proposal"
            );
            assert_eq!(
                std::fs::read(target.join("assistant/files/statement.pdf")).unwrap(),
                b"statement attachment"
            );
            assert!(!target.join("account.json").exists());
            assert!(!target.join("v2").exists());
            if !root_budget {
                assert_eq!(
                    std::fs::read(target.join("future-sidecar.txt")).unwrap(),
                    b"keep future data"
                );
            }
        }
    }
    #[test]
    fn failed_sidecar_copy_keeps_verified_database_available_and_retries_without_replacing_edits() {
        let d = tempfile::tempdir().unwrap();
        let mut source = Store::open_budget(&d.path().join("budget.sqlite3"), "legacy").unwrap();
        save(&mut source);
        std::fs::create_dir_all(d.path().join("assistant/threads")).unwrap();
        std::fs::write(
            d.path().join("assistant/threads/chat.json"),
            b"conversation",
        )
        .unwrap();
        std::fs::write(d.path().join("recovery.json"), b"original raw draft").unwrap();
        let target = d.path().join("v2/budgets/legacy");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("assistant"), b"blocked directory").unwrap();
        std::fs::write(
            d.path().join("budgets.json"),
            br#"{"current":"legacy","budgets":[{"id":"legacy","folder":""}]}"#,
        )
        .unwrap();
        let library = Budgets::load(d.path()).unwrap();
        assert_eq!(library.current(), "legacy");
        assert_eq!(library.list_all()[0].availability, "local");
        assert_eq!(
            library.job("migrate-legacy").unwrap().unwrap().state,
            "recovery"
        );
        let mut current =
            Store::open_budget(&library.database("legacy").unwrap(), "legacy").unwrap();
        let mut updated = current.saved().unwrap().0;
        updated.name = "Edit during sidecar retry".into();
        let token = current.status().unwrap().token;
        current.save(&updated, &token).unwrap();
        std::fs::write(
            target.join("recovery.json"),
            b"new raw draft after database publication",
        )
        .unwrap();
        drop(current);
        drop(library);
        std::fs::remove_file(target.join("assistant")).unwrap();
        let library = Budgets::load(d.path()).unwrap();
        let mut current =
            Store::open_budget(&library.database("legacy").unwrap(), "legacy").unwrap();
        assert_eq!(
            std::fs::read(target.join("recovery.json")).unwrap(),
            b"new raw draft after database publication"
        );
        assert_eq!(current.saved().unwrap().0.name, "Edit during sidecar retry");
        assert_eq!(library.current(), "legacy");
        assert_eq!(
            std::fs::read(target.join("assistant/threads/chat.json")).unwrap(),
            b"conversation"
        );
        assert_eq!(
            library.job("migrate-legacy").unwrap().unwrap().state,
            "complete"
        );
    }
    #[test]
    fn retry_after_snapshot_failure_restores_original_active_selection() {
        let d = tempfile::tempdir().unwrap();
        let mut source = Store::open_budget(&d.path().join("budget.sqlite3"), "legacy").unwrap();
        save(&mut source);
        let obstruction = d.path().join("v2/budgets/legacy/migration.sqlite3");
        std::fs::create_dir_all(&obstruction).unwrap();
        std::fs::write(
            d.path().join("budgets.json"),
            br#"{"current":"legacy","budgets":[{"id":"legacy","folder":""}]}"#,
        )
        .unwrap();
        let library = Budgets::load(d.path()).unwrap();
        assert_eq!(library.current(), "");
        assert_eq!(library.list_all()[0].lifecycle, "active");
        assert_eq!(library.list_all()[0].availability, "unavailable");
        drop(library);
        std::fs::remove_dir(&obstruction).unwrap();
        std::fs::write(&obstruction, b"interrupted SQLite snapshot").unwrap();
        let library = Budgets::load(d.path()).unwrap();
        assert_eq!(library.current(), "legacy");
        assert_eq!(library.list_all()[0].lifecycle, "active");
        assert_eq!(library.list_all()[0].availability, "local");
        assert_eq!(
            library.job("migrate-legacy").unwrap().unwrap().state,
            "complete"
        );
    }
    fn remove_catalog_for_test(root: &Path) {
        for file in [
            "library.sqlite3",
            "library.sqlite3-wal",
            "library.sqlite3-shm",
        ] {
            let path = root.join(file);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
    }
    #[test]
    fn missing_current_file_is_unavailable_without_creating_an_empty_replacement() {
        let d = tempfile::tempdir().unwrap();
        let mut library = Budgets::load(d.path()).unwrap();
        assert!(library.first_run().unwrap());
        let id = library.create().unwrap();
        let path = library.database(&id).unwrap();
        let mut store = Store::open_budget(&path, &id).unwrap();
        save(&mut store);
        library.mark_available(&id).unwrap();
        library.set_current(&id).unwrap();
        assert!(!library.first_run().unwrap());
        drop(store);
        drop(library);
        std::fs::remove_file(&path).unwrap();
        let mut library = Budgets::load(d.path()).unwrap();
        assert_eq!(library.current(), "");
        assert_eq!(library.list_all()[0].availability, "unavailable");
        assert!(!path.exists());
        assert!(!library.first_run().unwrap());
        library.remove(&id).unwrap();
        assert!(
            !library.first_run().unwrap(),
            "deleting the last budget must not replay onboarding"
        );
    }
    #[test]
    fn losing_the_catalog_recovers_native_files_without_guessing_remote_delivery_ancestry() {
        let d = tempfile::tempdir().unwrap();
        let mut library = Budgets::load(d.path()).unwrap();
        let id = library.create_owned(Some("account")).unwrap();
        let row = library.list_all().pop().unwrap();
        let path = library.database(&id).unwrap();
        let mut store = Store::open_budget(&path, &id).unwrap();
        save(&mut store);
        let binding = crate::sync_storage::Binding {
            identity: crate::sync_storage::RemoteIdentity {
                account_id: "account".into(),
                key_generation: "1".into(),
                catalog_epoch: "epoch".into(),
                budget_id: row.remote_id.unwrap(),
                incarnation: row.incarnation.unwrap(),
            },
            remote_version: "7".into(),
            acknowledged_local_revision: store.revision().unwrap(),
            base: sample(),
        };
        store.bind_sync(&binding).unwrap();
        library.mark_available(&id).unwrap();
        library.set_current(&id).unwrap();
        std::fs::create_dir_all(path.parent().unwrap().join("assistant")).unwrap();
        std::fs::write(
            path.parent().unwrap().join("assistant/retained.txt"),
            b"history",
        )
        .unwrap();
        drop(store);
        drop(library);
        remove_catalog_for_test(d.path());
        let library = Budgets::load(d.path()).unwrap();
        let row = library.list_all().pop().unwrap();
        assert_eq!(row.id, id);
        assert_eq!(row.lifecycle, "recovery");
        assert!(row.account_id.is_none());
        assert!(row.remote_id.is_none());
        assert_eq!(library.current(), "");
        assert!(!library.first_run().unwrap());
        let mut recovered = Store::open_budget(&path, &id).unwrap();
        assert_eq!(recovered.saved().unwrap().0, sample());
        assert_eq!(recovered.sync_binding().unwrap(), Some(binding));
        assert_eq!(
            std::fs::read(path.parent().unwrap().join("assistant/retained.txt")).unwrap(),
            b"history"
        );
        assert_eq!(
            library.recovery_hint(&id).unwrap().unwrap()["quarantinedOrigin"]["accountId"],
            "account"
        );
    }
    #[test]
    fn losing_the_catalog_prefers_verified_managed_files_over_retired_legacy_sentinels() {
        let d = tempfile::tempdir().unwrap();
        let mut source = Store::open_budget(&d.path().join("budget.sqlite3"), "legacy").unwrap();
        save(&mut source);
        std::fs::write(
            d.path().join("budgets.json"),
            br#"{"current":"legacy","budgets":[{"id":"legacy","folder":""}]}"#,
        )
        .unwrap();
        let library = Budgets::load(d.path()).unwrap();
        let path = library.database("legacy").unwrap();
        drop(library);
        remove_catalog_for_test(d.path());
        let library = Budgets::load(d.path()).unwrap();
        assert_eq!(library.database("legacy").unwrap(), path);
        assert_eq!(library.current(), "");
        assert_eq!(library.list_all().len(), 1);
        assert_eq!(library.list_all()[0].lifecycle, "recovery");
        let mut recovered = Store::open_budget(&path, "legacy").unwrap();
        assert_eq!(recovered.saved().unwrap().0, sample());
        assert!(!recovered.sync_blocked().unwrap());
        assert!(source.sync_blocked().unwrap());
    }
    #[test]
    fn catalog_activation_matches_the_durable_identity_and_updates_lifecycle_together() {
        let d = tempfile::tempdir().unwrap();
        let mut l = Budgets::load(d.path()).unwrap();
        let id = l.create_owned(Some("account")).unwrap();
        let entry = l.list_all().pop().unwrap();
        let remote = entry.remote_id.unwrap();
        let mut s = Store::open_budget(&l.database(&id).unwrap(), &id).unwrap();
        save(&mut s);
        s.bind_sync(&crate::sync_storage::Binding {
            identity: crate::sync_storage::RemoteIdentity {
                account_id: "account".into(),
                key_generation: "1".into(),
                catalog_epoch: "epoch".into(),
                budget_id: remote.clone(),
                incarnation: "restored".into(),
            },
            remote_version: "3".into(),
            acknowledged_local_revision: s.revision().unwrap(),
            base: sample(),
        })
        .unwrap();
        l.set_lifecycle(&id, "trashed").unwrap();
        assert!(l
            .activate_binding(&id, "account", &remote, "wrong")
            .is_err());
        l.activate_binding(&id, "account", &remote, "restored")
            .unwrap();
        let row = l.list_all().pop().unwrap();
        assert_eq!(row.incarnation.as_deref(), Some("restored"));
        assert_eq!(
            (row.lifecycle.as_str(), row.availability.as_str()),
            ("active", "local")
        );
        assert!(l.job(&format!("initialize-{id}")).unwrap().is_none());
    }
}
fn read_json<T: serde::de::DeserializeOwned>(
    c: &Connection,
    sql: &str,
    key: &str,
) -> Result<Option<T>> {
    let raw: Option<String> = c
        .query_row(sql, [key], |r| r.get(0))
        .optional()
        .map_err(error)?;
    raw.map(|s| serde_json::from_str(&s).map_err(error))
        .transpose()
}
impl Budgets {
    fn claim_local_lease(&self, id: &str) -> Result<Option<std::fs::File>> {
        valid_id(id)?;
        if self.local_leases.contains_key(id) {
            return Ok(None);
        }
        let folder = self.data_dir.join("library-leases");
        std::fs::create_dir_all(&folder).map_err(error)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(folder.join(format!("{id}.lock")))
            .map_err(error)?;
        if try_local_lock(&file).map_err(error)? {
            Ok(Some(file))
        } else {
            Ok(None)
        }
    }
    /// A returned OS lease proves no live creator/downloader owns this local job. Keep it
    /// alive throughout repair. None never authorizes classifying an incomplete file.
    pub fn claim_abandoned_local_job(&self, operation_id: &str) -> Result<Option<std::fs::File>> {
        let Some(job) = self.job(operation_id)? else {
            return Ok(None);
        };
        if job.state != "pending"
            || !matches!(job.kind.as_str(), "initialize" | "download" | "evict")
        {
            return Ok(None);
        }
        let Some(id) = job.local_budget_id.as_deref() else {
            return Ok(None);
        };
        self.claim_local_lease(id)
    }
    /// Called only when the host has finished or abandoned its create/download attempt.
    pub fn release_local_job(&mut self, id: &str) {
        self.local_leases.remove(id);
    }
    /// Replay physical evictions. Failures preserve the frozen source and journal for retry;
    /// initialization/download recovery needs the host's account and editor decisions.
    pub fn repair_local_jobs(&mut self) -> Result<()> {
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.data_dir.join("library-migration.lock"))
            .map_err(error)?;
        migration_lock(&lock).map_err(error)?;
        self.repair_evictions()
    }
    fn repair_evictions(&mut self) -> Result<()> {
        for job in self.jobs()?.into_iter().filter(|j| j.kind == "evict") {
            let Some(id) = job.local_budget_id.as_deref() else {
                continue;
            };
            let Some(_lease) = self.claim_abandoned_local_job(&job.operation_id)? else {
                continue;
            };
            if self.current == id {
                let path = self.database(id)?;
                let retired = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .ok()
                    .and_then(|db| {
                        db.query_row("SELECT retired FROM sync_local_state WHERE id=1", [], |r| {
                            r.get::<_, bool>(0)
                        })
                        .ok()
                    })
                    .unwrap_or(false);
                if retired {
                    self.connection.execute("UPDATE app_state SET current_id=NULL,library_version=library_version+1 WHERE current_id=?1",[id]).map_err(error)?;
                    self.current.clear();
                }
            }
            if let Err(error) = self.evict_locked(id) {
                self.set_entry_status(id, &format!("Download removal waiting: {error}"))?;
            }
        }
        Ok(())
    }
    /// Older running processes can still create a new legacy-path file. Import only unknown
    /// identities as recovery copies; an old registry can never delete, rebind or select a row.
    fn recover_late_legacy_creations(&mut self) -> Result<()> {
        let path = self.data_dir.join("budgets.json");
        if !path.exists() {
            return Ok(());
        }
        let registry: Registry =
            serde_json::from_slice(&std::fs::read(path).map_err(error)?).map_err(error)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let mut changed = false;
        for entry in registry.budgets {
            valid_id(&entry.id)?;
            let folder = folder_path(&self.data_dir, &entry.folder)?;
            if !folder.join("budget.sqlite3").is_file() {
                continue;
            }
            if tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM local_budgets WHERE id=?1 OR folder=?2)",
                    params![entry.id, entry.folder],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(error)?
            {
                continue;
            }
            tx.execute("INSERT INTO local_budgets(id,folder,last_opened,lifecycle,status) VALUES(?1,?2,?3,'recovery','Needs attention')",params![entry.id,entry.folder,entry.last_opened]).map_err(error)?;
            tx.execute("INSERT INTO sync_hints VALUES(?1,?2)",params![entry.id,serde_json::to_string(&json!({"quarantineReason":{"reason":"created_by_legacy_process_after_migration","sourceFolder":entry.folder}})).map_err(error)?]).map_err(error)?;
            changed = true;
        }
        if changed {
            bump(&tx)?;
        }
        tx.commit().map_err(error)
    }
    /// Copy a SQLite-consistent snapshot into v2-only paths. The source becomes a fenced
    /// recovery sentinel; even an already-open compatible handle cannot modify it afterward.
    /// A process/file lock serializes replays; a library job journals publication separately.
    pub fn migrate_storage_v2(&mut self) -> Result<()> {
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.data_dir.join("library-migration.lock"))
            .map_err(error)?;
        migration_lock(&lock).map_err(error)?;
        let entries = {
            let mut q = self.connection.prepare("SELECT b.id,COALESCE(json_extract(j.data,'$.payload.source'),b.folder) FROM local_budgets b LEFT JOIN lifecycle_jobs j ON j.operation_id='migrate-'||b.id WHERE b.folder NOT LIKE 'v2/budgets/%' OR (j.kind='migrate' AND j.state!='complete')").map_err(error)?;
            let rows = q
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(error)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(error)?
        };
        for (id, folder) in entries {
            let source_folder = folder_path(&self.data_dir, &folder)?;
            let source_path = source_folder.join("budget.sqlite3");
            let target_folder = format!("v2/budgets/{id}");
            let target = folder_path(&self.data_dir, &target_folder)?;
            let operation = format!("migrate-{id}");
            let migration = (|| -> Result<()> {
                let (current_folder, lifecycle, status): (String, String, Option<String>) = self
                    .connection
                    .query_row(
                        "SELECT folder,lifecycle,status FROM local_budgets WHERE id=?1",
                        [&id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .map_err(error)?;
                let mut job = self.job(&operation)?.unwrap_or(LifecycleJob {
                    operation_id:operation.clone(),local_budget_id:Some(id.clone()),kind:"migrate".into(),
                    payload:json!({"source":folder,"target":target_folder,"originalLifecycle":lifecycle,"originalStatus":status,"wasCurrent":self.current==id}),
                    state:"pending".into(),receipt:None,
                });
                // Repair retry journals created by the earlier migration implementation.
                if job.payload.get("originalLifecycle").is_none() {
                    let migration_failure = status
                        .as_deref()
                        .is_some_and(|s| s.starts_with("Storage migration"));
                    job.payload["originalLifecycle"] =
                        json!(if migration_failure && lifecycle == "recovery" {
                            "active"
                        } else {
                            &lifecycle
                        });
                    job.payload["originalStatus"] = if migration_failure {
                        Value::Null
                    } else {
                        json!(status)
                    };
                    let legacy_selected = std::fs::read(self.data_dir.join("budgets.json"))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<Registry>(&bytes).ok())
                        .and_then(|r| r.current);
                    job.payload["wasCurrent"] = json!(
                        self.current == id
                            || (migration_failure && legacy_selected.as_deref() == Some(&id))
                    );
                }
                self.connection.execute("INSERT INTO lifecycle_jobs VALUES(?1,?2,?3,?4,?5) ON CONFLICT(operation_id) DO UPDATE SET state=excluded.state,data=excluded.data",
                    params![operation,id,job.kind,job.state,serde_json::to_string(&job).map_err(error)?]).map_err(error)?;
                let published = current_folder == target_folder;
                if !published {
                    if !source_path.is_file() {
                        return Err(
                            "The original budget file is missing; no replacement was created."
                                .into(),
                        );
                    }
                    let source = crate::store::Store::open_budget(&source_path, &id)?;
                    source
                        .connection
                        .execute("UPDATE sync_local_state SET retired=1 WHERE id=1", [])
                        .map_err(error)?;
                    std::fs::create_dir_all(&target).map_err(error)?;
                    let destination = target.join("budget.sqlite3");
                    let staging = target.join("migration.sqlite3");
                    if job.payload.get("sha256").and_then(Value::as_str).is_none() {
                        if destination.exists() {
                            return Err("An unjournaled destination database already exists; both copies were preserved.".into());
                        }
                        // No checksum has been acknowledged: these are disposable files
                        // from this journal's interrupted backup, never the source budget.
                        for suffix in ["", "-wal", "-shm", "-journal"] {
                            let partial = target.join(format!("migration.sqlite3{suffix}"));
                            if partial.is_file() {
                                std::fs::remove_file(partial).map_err(error)?;
                            }
                        }
                        {
                            let mut output = Connection::open(&staging).map_err(error)?;
                            {
                                let backup =
                                    rusqlite::backup::Backup::new(&source.connection, &mut output)
                                        .map_err(error)?;
                                backup
                                    .run_to_completion(
                                        128,
                                        std::time::Duration::from_millis(5),
                                        None,
                                    )
                                    .map_err(error)?;
                            }
                            crate::sync_storage::register_writer(&output)?;
                            output
                                .execute("UPDATE sync_local_state SET retired=0 WHERE id=1", [])
                                .map_err(error)?;
                            let integrity: String = output
                                .query_row("PRAGMA quick_check", [], |r| r.get(0))
                                .map_err(error)?;
                            if integrity != "ok" {
                                return Err("The migrated SQLite snapshot did not validate.".into());
                            }
                            output
                                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                                .map_err(error)?;
                        }
                        std::fs::OpenOptions::new()
                            .write(true)
                            .open(&staging)
                            .map_err(error)?
                            .sync_all()
                            .map_err(error)?;
                        job.payload["sha256"] = json!(file_hash(&staging)?);
                        job.state = "staged".into();
                        self.connection.execute("UPDATE lifecycle_jobs SET state='staged',data=?2 WHERE operation_id=?1",
                            params![operation,serde_json::to_string(&job).map_err(error)?]).map_err(error)?;
                    }
                    let expected = job.payload["sha256"]
                        .as_str()
                        .ok_or("Migration has no snapshot checksum.")?;
                    if destination.exists() {
                        if file_hash(&destination)? != expected {
                            return Err("Published migration snapshot changed before library adoption; preserve it for recovery.".into());
                        }
                    } else {
                        if file_hash(&staging)? != expected {
                            return Err("Staged migration snapshot failed its checksum.".into());
                        }
                        publish_file(&staging, &destination)?;
                        #[cfg(unix)]
                        for directory in [
                            &target,
                            &self.data_dir.join("v2/budgets"),
                            &self.data_dir.join("v2"),
                            &self.data_dir,
                        ] {
                            std::fs::File::open(directory)
                                .map_err(error)?
                                .sync_all()
                                .map_err(error)?;
                        }
                    }
                    // Publishing the verified financial database must not depend on a large,
                    // locked or interrupted assistant/backup sidecar copy. Sidecars retain a
                    // separate replayable pending phase and the original files stay intact.
                    job.payload["published"] = json!(true);
                    job.state = "pending".into();
                    let tx = self
                        .connection
                        .transaction_with_behavior(TransactionBehavior::Immediate)
                        .map_err(error)?;
                    let original = job.payload["originalLifecycle"]
                        .as_str()
                        .unwrap_or("active");
                    tx.execute("UPDATE local_budgets SET folder=?2,availability='local',lifecycle=CASE WHEN lifecycle='recovery' AND status LIKE 'Storage migration%' THEN ?4 ELSE lifecycle END,status=?5 WHERE id=?1 AND folder=?3",
                        params![id,target_folder,folder,original,job.payload["originalStatus"].as_str()]).map_err(error)?;
                    let restored = if job.payload["wasCurrent"] == true {
                        tx.execute("UPDATE app_state SET current_id=?1 WHERE id=1 AND current_id IS NULL AND EXISTS(SELECT 1 FROM local_budgets WHERE id=?1 AND lifecycle='active')",[&id]).map_err(error)? > 0
                    } else {
                        false
                    };
                    tx.execute(
                        "UPDATE lifecycle_jobs SET state='pending',data=?2 WHERE operation_id=?1",
                        params![operation, serde_json::to_string(&job).map_err(error)?],
                    )
                    .map_err(error)?;
                    bump(&tx)?;
                    tx.commit().map_err(error)?;
                    if restored {
                        self.current = id.clone();
                    }
                }
                let mut copied: std::collections::BTreeSet<String> =
                    serde_json::from_value(job.payload["copiedSidecars"].clone())
                        .unwrap_or_default();
                copy_budget_sidecars(
                    &source_folder,
                    &target,
                    folder.is_empty(),
                    &mut copied,
                    &mut |completed| {
                        job.payload["copiedSidecars"] = json!(completed);
                        self.connection
                            .execute(
                                "UPDATE lifecycle_jobs SET data=?2 WHERE operation_id=?1",
                                params![operation, serde_json::to_string(&job).map_err(error)?],
                            )
                            .map_err(error)?;
                        Ok(())
                    },
                )?;
                job.state = "complete".into();
                job.receipt = Some(
                    json!({"localMigration":true,"sha256":job.payload["sha256"],"sidecarsCopied":true}),
                );
                if let Some(payload) = job.payload.as_object_mut() {
                    payload.remove("error");
                }
                let tx = self
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(error)?;
                tx.execute(
                    "UPDATE lifecycle_jobs SET state='complete',data=?2 WHERE operation_id=?1",
                    params![operation, serde_json::to_string(&job).map_err(error)?],
                )
                .map_err(error)?;
                tx.execute("UPDATE local_budgets SET status=?2 WHERE id=?1 AND status LIKE 'Storage migration%'",params![id,job.payload["originalStatus"].as_str()]).map_err(error)?;
                bump(&tx)?;
                tx.commit().map_err(error)?;
                Ok(())
            })();
            if let Err(problem) = migration {
                let fenced =
                    Connection::open_with_flags(&source_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .ok()
                        .and_then(|db| {
                            db.query_row(
                                "SELECT retired FROM sync_local_state WHERE id=1",
                                [],
                                |r| r.get::<_, bool>(0),
                            )
                            .ok()
                        })
                        .unwrap_or(false);
                let mut job = self
                    .job(&operation)?
                    .ok_or("Missing retryable storage migration journal.")?;
                job.state = "recovery".into();
                job.payload["error"] = json!(problem);
                job.payload["sourceFenced"] = json!(fenced);
                let published = self
                    .connection
                    .query_row(
                        "SELECT folder=?2 FROM local_budgets WHERE id=?1",
                        params![id, target_folder],
                        |r| r.get::<_, bool>(0),
                    )
                    .map_err(error)?;
                let tx = self
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(error)?;
                tx.execute("UPDATE local_budgets SET availability=CASE WHEN ?3 THEN availability ELSE 'unavailable' END,status=?2 WHERE id=?1",
                    params![id,format!("Storage migration will retry: {problem}"),published]).map_err(error)?;
                if !published {
                    tx.execute(
                        "UPDATE app_state SET current_id=NULL WHERE current_id=?1",
                        [&id],
                    )
                    .map_err(error)?;
                }
                tx.execute(
                    "UPDATE lifecycle_jobs SET state='recovery',data=?2 WHERE operation_id=?1",
                    params![operation, serde_json::to_string(&job).map_err(error)?],
                )
                .map_err(error)?;
                bump(&tx)?;
                tx.commit().map_err(error)?;
                if !published && self.current == id {
                    self.current.clear();
                }
            }
        }
        self.repair_evictions()
    }
    /// Rebuild visibility, never account delivery ancestry, when the catalog was lost.
    /// A budget database cannot prove the missing installation sequence or trash state.
    /// Retain it intact for explicit recovery instead of silently publishing it again.
    fn recover_managed_files(&mut self, rebuilt: bool) -> Result<()> {
        let root = self.data_dir.join("v2/budgets");
        if !root.exists() {
            return Ok(());
        }
        for item in std::fs::read_dir(root).map_err(error)? {
            let item = item.map_err(error)?;
            let metadata = std::fs::symlink_metadata(item.path()).map_err(error)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                continue;
            }
            let Some(id) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if valid_id(&id).is_err() {
                continue;
            }
            let path = item.path().join("budget.sqlite3");
            if !path.is_file() {
                continue;
            }
            if std::fs::symlink_metadata(&path)
                .map_err(error)?
                .file_type()
                .is_symlink()
            {
                continue;
            }
            let prior: Option<String> = self
                .connection
                .query_row("SELECT folder FROM local_budgets WHERE id=?1", [&id], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(error)?;
            let folder = format!("v2/budgets/{id}");
            if prior.as_deref() == Some(&folder) || (prior.is_some() && !rebuilt) {
                continue;
            }
            let mut name = "Recovered budget".to_owned();
            let mut currency = String::new();
            let mut binding = Value::Null;
            let mut readable = false;
            if let Ok(db) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
                readable = db
                    .query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                    .is_ok_and(|s| s == "ok");
                if let Ok((n, c)) =
                    db.query_row("SELECT name,currency FROM metadata WHERE id=1", [], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })
                {
                    name = n;
                    currency = c;
                }
                binding =
                    read_json::<Value>(&db, "SELECT data FROM sync_binding_v2 WHERE id=?1", "1")
                        .ok()
                        .flatten()
                        .unwrap_or(Value::Null);
            }
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(error)?;
            tx.execute("INSERT INTO local_budgets(id,folder,lifecycle,availability,name,currency,status) VALUES(?1,?2,'recovery',?3,?4,?5,'Recovered after library loss; review a copy before editing.') ON CONFLICT(id) DO UPDATE SET folder=excluded.folder,account_id=NULL,remote_id=NULL,incarnation=NULL,lifecycle='recovery',availability=excluded.availability,name=excluded.name,currency=excluded.currency,status=excluded.status",
                params![id,folder,if readable {"local"} else {"unavailable"},name,currency]).map_err(error)?;
            tx.execute("INSERT INTO sync_hints VALUES(?1,?2) ON CONFLICT(local_budget_id) DO UPDATE SET data=excluded.data",
                params![id,json!({"quarantineReason":{"reason":"catalog_recovered_from_managed_storage"},"quarantinedOrigin":binding["identity"],"remoteVersion":binding["remoteVersion"]}).to_string()]).map_err(error)?;
            tx.execute(
                "UPDATE app_state SET current_id=NULL WHERE current_id=?1",
                [&id],
            )
            .map_err(error)?;
            bump(&tx)?;
            tx.commit().map_err(error)?;
            if self.current == id {
                self.current.clear();
            }
        }
        Ok(())
    }
    fn repair_missing_selection(&mut self) -> Result<()> {
        if self.current.is_empty() {
            return Ok(());
        }
        let id = self.current.clone();
        let active:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM local_budgets WHERE id=?1 AND lifecycle='active' AND availability='local')",[&id],|r|r.get(0)).map_err(error)?;
        if active && self.database(&id)?.is_file() {
            return Ok(());
        }
        // A concurrent window's newer selection is not ours to clear.
        self.connection.execute("UPDATE app_state SET current_id=NULL,library_version=library_version+1 WHERE current_id=?1",[&id]).map_err(error)?;
        self.current.clear();
        Ok(())
    }
    pub fn load(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir).map_err(error)?;
        let mut c = Connection::open(data_dir.join("library.sqlite3")).map_err(error)?;
        c.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(error)?;
        c.execute_batch("PRAGMA foreign_keys=ON;PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;
   CREATE TABLE IF NOT EXISTS app_state(id INTEGER PRIMARY KEY CHECK(id=1),initialized INTEGER NOT NULL,current_id TEXT,library_version INTEGER NOT NULL);INSERT OR IGNORE INTO app_state VALUES(1,0,NULL,0);
   CREATE TABLE IF NOT EXISTS local_budgets(id TEXT PRIMARY KEY,folder TEXT NOT NULL UNIQUE,last_opened TEXT,account_id TEXT,remote_id TEXT,incarnation TEXT,lifecycle TEXT NOT NULL DEFAULT 'active',availability TEXT NOT NULL DEFAULT 'local',evicted INTEGER NOT NULL DEFAULT 0,name TEXT NOT NULL DEFAULT 'My budget',currency TEXT NOT NULL DEFAULT 'SAR',status TEXT);
   CREATE UNIQUE INDEX IF NOT EXISTS library_remote_binding ON local_budgets(account_id,remote_id) WHERE account_id IS NOT NULL AND remote_id IS NOT NULL;
   CREATE TABLE IF NOT EXISTS account_catalog(account_id TEXT NOT NULL,remote_id TEXT NOT NULL,data TEXT NOT NULL CHECK(json_valid(data)),PRIMARY KEY(account_id,remote_id));
   CREATE TABLE IF NOT EXISTS account_sync(account_id TEXT PRIMARY KEY,installation_id TEXT NOT NULL,next_sequence TEXT NOT NULL DEFAULT '1',data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data)));
   CREATE TABLE IF NOT EXISTS lifecycle_jobs(operation_id TEXT PRIMARY KEY,local_budget_id TEXT,kind TEXT NOT NULL,state TEXT NOT NULL,data TEXT NOT NULL CHECK(json_valid(data)));
   CREATE TABLE IF NOT EXISTS delivery_slot(account_id TEXT PRIMARY KEY,data TEXT NOT NULL CHECK(json_valid(data)));
   CREATE TABLE IF NOT EXISTS delivery_completed(account_id TEXT NOT NULL,sequence TEXT NOT NULL,digest TEXT NOT NULL,PRIMARY KEY(account_id,sequence));
   CREATE TABLE IF NOT EXISTS sync_hints(local_budget_id TEXT PRIMARY KEY,data TEXT NOT NULL CHECK(json_valid(data)));
  ").map_err(error)?;
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let initializing = !tx
            .query_row("SELECT initialized FROM app_state WHERE id=1", [], |r| {
                r.get::<_, bool>(0)
            })
            .map_err(error)?;
        if initializing {
            let old = data_dir.join("budgets.json");
            let mut registry: Registry = if old.exists() {
                serde_json::from_slice(&std::fs::read(old).map_err(error)?).map_err(error)?
            } else {
                Registry::default()
            };
            if registry.budgets.is_empty() && data_dir.join("budget.sqlite3").is_file() {
                let id = new_id();
                registry.current = Some(id.clone());
                registry.budgets.push(Entry {
                    id,
                    folder: String::new(),
                    last_opened: None,
                });
            }
            for e in &registry.budgets {
                valid_id(&e.id)?;
                folder_path(data_dir, &e.folder)?;
                tx.execute(
                    "INSERT INTO local_budgets(id,folder,last_opened) VALUES(?1,?2,?3)",
                    params![e.id, e.folder, e.last_opened],
                )
                .map_err(error)?;
            }
            let current = registry
                .current
                .filter(|id| registry.budgets.iter().any(|b| &b.id == id));
            tx.execute("UPDATE app_state SET initialized=1,current_id=?1,library_version=library_version+1 WHERE id=1",[current]).map_err(error)?;
        }
        tx.commit().map_err(error)?;
        let current: Option<String> = c
            .query_row("SELECT current_id FROM app_state WHERE id=1", [], |r| {
                r.get(0)
            })
            .map_err(error)?;
        let mut library = Self {
            data_dir: data_dir.into(),
            connection: c,
            current: current.unwrap_or_default(),
            local_leases: Default::default(),
        };
        library.recover_managed_files(initializing)?;
        library.recover_late_legacy_creations()?;
        library.migrate_storage_v2()?;
        library.repair_missing_selection()?;
        Ok(library)
    }
    pub fn peek(root: &Path) -> Result<(String, PathBuf)> {
        if root.join("library.sqlite3").exists() {
            let c = Connection::open_with_flags(
                root.join("library.sqlite3"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(error)?;
            let row:Option<(String,String)>=c.query_row("SELECT b.id,b.folder FROM app_state a JOIN local_budgets b ON b.id=a.current_id WHERE a.id=1 AND b.lifecycle='active'",[],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(error)?;
            let (id, folder) = row.ok_or_else(|| t!("UrBudget has no budgets yet."))?;
            return Ok((id, folder_path(root, &folder)?.join("budget.sqlite3")));
        }
        let registry: Registry =
            serde_json::from_slice(&std::fs::read(root.join("budgets.json")).map_err(error)?)
                .map_err(error)?;
        let e = registry
            .current
            .as_ref()
            .and_then(|id| registry.budgets.iter().find(|b| &b.id == id))
            .or(registry.budgets.first())
            .ok_or_else(|| t!("UrBudget has no budgets yet."))?;
        Ok((
            e.id.clone(),
            folder_path(root, &e.folder)?.join("budget.sqlite3"),
        ))
    }
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
    /// Tombstones remain catalog rows, so deleting the last budget is not first launch.
    pub fn first_run(&self) -> Result<bool> {
        let has_rows: bool = self
            .connection
            .query_row("SELECT EXISTS(SELECT 1 FROM local_budgets)", [], |r| {
                r.get(0)
            })
            .map_err(error)?;
        if has_rows {
            return Ok(false);
        }
        let managed = self.data_dir.join("v2/budgets");
        if managed.exists() {
            for item in std::fs::read_dir(managed).map_err(error)? {
                if item.map_err(error)?.path().join("budget.sqlite3").is_file() {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    pub fn current(&self) -> &str {
        &self.current
    }
    pub fn version(&self) -> Result<String> {
        self.connection
            .query_row(
                "SELECT library_version FROM app_state WHERE id=1",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n.to_string())
            .map_err(error)
    }
    pub fn database(&self, id: &str) -> Result<PathBuf> {
        let folder: Option<String> = self
            .connection
            .query_row("SELECT folder FROM local_budgets WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(error)?;
        Ok(folder_path(
            &self.data_dir,
            &folder.ok_or_else(|| t!("That budget no longer exists."))?,
        )?
        .join("budget.sqlite3"))
    }
    pub fn contains(&self, id: &str) -> bool {
        self.connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_budgets WHERE id=?1 AND lifecycle='active')",
                [id],
                |r| r.get(0),
            )
            .unwrap_or(false)
    }
    pub fn create(&mut self) -> Result<String> {
        self.create_owned(None)
    }
    /// Enrollment is part of creating the library row, before the budget file is initialized.
    pub fn create_owned(&mut self, account: Option<&str>) -> Result<String> {
        if account.is_some_and(str::is_empty) {
            return Err("A stable account ID is required.".into());
        }
        let id = new_id();
        self.adopt_owned(&id, account)?;
        Ok(id)
    }
    pub fn adopt(&mut self, id: &str) -> Result<()> {
        self.adopt_owned(id, None)
    }
    fn adopt_owned(&mut self, id: &str, account: Option<&str>) -> Result<()> {
        valid_id(id)?;
        let lease = self
            .claim_local_lease(id)?
            .ok_or("Another process is initializing this budget.")?;
        let folder = format!("v2/budgets/{id}");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let remote = account.map(|_| new_id());
        let incarnation = account.map(|_| new_id());
        tx.execute("INSERT INTO local_budgets(id,folder,account_id,remote_id,incarnation) VALUES(?1,?2,?3,?4,?5)",params![id,folder,account,remote,incarnation]).map_err(|_|t!("That budget is already on this computer."))?;
        let job = LifecycleJob {
            operation_id: format!("initialize-{id}"),
            local_budget_id: Some(id.into()),
            kind: "initialize".into(),
            payload: json!({"folder":folder}),
            state: "pending".into(),
            receipt: None,
        };
        tx.execute(
            "INSERT INTO lifecycle_jobs VALUES(?1,?2,?3,?4,?5)",
            params![
                job.operation_id,
                id,
                job.kind,
                job.state,
                serde_json::to_string(&job).map_err(error)?
            ],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        self.local_leases.insert(id.into(), lease);
        std::fs::create_dir_all(folder_path(&self.data_dir, &folder)?).map_err(error)
    }
    pub fn mark_available(&mut self, id: &str) -> Result<()> {
        let db = Connection::open_with_flags(self.database(id)?, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(error)?;
        let (name, currency): (String, String) = db
            .query_row("SELECT name,currency FROM metadata WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(error)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        tx.execute("UPDATE local_budgets SET availability='local',evicted=0,name=?2,currency=?3 WHERE id=?1",params![id,name,currency]).map_err(error)?;
        tx.execute("DELETE FROM lifecycle_jobs WHERE local_budget_id=?1 AND kind IN ('initialize','download')",[id]).map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        self.local_leases.remove(id);
        Ok(())
    }
    pub fn forget_new(&mut self, id: &str) -> Result<()> {
        if self.current == id {
            return Err(t!("Only a budget that was just created can be discarded."));
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        if !tx.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_jobs WHERE local_budget_id=?1 AND kind='initialize' AND state='pending')",[id],|r|r.get::<_,bool>(0)).map_err(error)?{return Err(t!("Only a budget that was just created can be discarded."))}
        tx.execute(
            "UPDATE local_budgets SET lifecycle='recovery' WHERE id=?1",
            [id],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        self.local_leases.remove(id);
        Ok(())
    }
    pub fn set_current(&mut self, id: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        if !tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_budgets WHERE id=?1 AND lifecycle='active')",
                [id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(error)?
        {
            return Err(t!("That budget no longer exists."));
        }
        tx.execute(
            "UPDATE local_budgets SET last_opened=?2 WHERE id=?1",
            params![id, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(error)?;
        tx.execute("UPDATE app_state SET current_id=?1 WHERE id=1", [id])
            .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        self.current = id.into();
        Ok(())
    }
    pub fn clear_current(&mut self) -> Result<()> {
        self.connection
            .execute(
                "UPDATE app_state SET current_id=NULL,library_version=library_version+1 WHERE id=1",
                [],
            )
            .map_err(error)?;
        self.current.clear();
        Ok(())
    }
    pub fn set_entry_status(&mut self, id: &str, status: &str) -> Result<()> {
        self.connection
            .execute(
                "UPDATE local_budgets SET status=?2 WHERE id=?1",
                params![id, status],
            )
            .map_err(error)?;
        bump(&self.connection)
    }
}

impl Budgets {
    /// Logical deletion preserves files and recovery. Hosts close the active Store afterward.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let row: Option<(Option<String>, String)> = tx
            .query_row(
                "SELECT account_id,lifecycle FROM local_budgets WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(error)?;
        let (account, old) = row.ok_or_else(|| t!("That budget no longer exists."))?;
        tx.execute(
            "UPDATE local_budgets SET lifecycle='trashed' WHERE id=?1",
            [id],
        )
        .map_err(error)?;
        tx.execute(
            "UPDATE app_state SET current_id=NULL WHERE current_id=?1",
            [id],
        )
        .map_err(error)?;
        if old != "trashed" {
            if let Some(account) = account {
                // Pin the version the user could review, never a later catalog head fetched
                // while draining an offline deletion. A local file's verified base is more
                // precise than the catalog when incoming content is waiting behind an editor.
                let head = read_json::<CatalogHead>(&tx,
                    "SELECT c.data FROM account_catalog c JOIN local_budgets b ON c.account_id=b.account_id AND c.remote_id=b.remote_id WHERE b.id=?1", id)?;
                let folder: String = tx
                    .query_row("SELECT folder FROM local_budgets WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .map_err(error)?;
                let path = folder_path(&self.data_dir, &folder)?.join("budget.sqlite3");
                let binding = if path.is_file() {
                    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                        .map_err(error)?;
                    read_json::<crate::sync_storage::Binding>(
                        &db,
                        "SELECT data FROM sync_binding_v2 WHERE id=?1",
                        "1",
                    )?
                } else {
                    None
                };
                let reviewed = if let Some(binding) =
                    binding.filter(|b| b.identity.account_id == account)
                {
                    json!({"version":binding.remote_version,"incarnation":binding.identity.incarnation,"keyGeneration":binding.identity.key_generation,"catalogEpoch":binding.identity.catalog_epoch})
                } else if let Some(head) = head {
                    json!({"version":head.version,"incarnation":head.incarnation,"keyGeneration":head.key_generation,"catalogEpoch":head.catalog_epoch})
                } else {
                    Value::Null
                };
                let job = LifecycleJob {
                    operation_id: new_id(),
                    local_budget_id: Some(id.into()),
                    kind: "trash".into(),
                    payload: json!({"accountId":account,"reviewedHead":reviewed}),
                    state: "pending".into(),
                    receipt: None,
                };
                tx.execute(
                    "INSERT INTO lifecycle_jobs VALUES(?1,?2,?3,?4,?5)",
                    params![
                        job.operation_id,
                        id,
                        job.kind,
                        job.state,
                        serde_json::to_string(&job).map_err(error)?
                    ],
                )
                .map_err(error)?;
            }
        }
        bump(&tx)?;
        tx.commit().map_err(error)?;
        if self.current == id {
            self.current.clear();
        }
        Ok(())
    }
    pub fn restore_local(&mut self, id: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        if tx.execute("UPDATE local_budgets SET lifecycle='active' WHERE id=?1 AND account_id IS NULL AND lifecycle IN ('trashed','recovery')",[id]).map_err(error)?==0{return Err("An account budget requires a checked cloud restore.".into())}
        bump(&tx)?;
        tx.commit().map_err(error)
    }
    pub fn set_lifecycle(&mut self, id: &str, state: &str) -> Result<()> {
        if !matches!(state, "active" | "trashed" | "purged" | "recovery") {
            return Err("Invalid lifecycle state.".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        tx.execute(
            "UPDATE local_budgets SET lifecycle=?2 WHERE id=?1",
            params![id, state],
        )
        .map_err(error)?;
        if state != "active" {
            tx.execute(
                "UPDATE app_state SET current_id=NULL WHERE current_id=?1",
                [id],
            )
            .map_err(error)?;
        }
        bump(&tx)?;
        tx.commit().map_err(error)?;
        if state != "active" && self.current == id {
            self.current.clear();
        }
        Ok(())
    }
    pub fn associate(
        &mut self,
        id: &str,
        account: &str,
        remote: &str,
        incarnation: &str,
    ) -> Result<()> {
        if account.is_empty() || remote.is_empty() || incarnation.is_empty() {
            return Err("Incomplete account binding.".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let row: Option<(Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT account_id,remote_id FROM local_budgets WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(error)?;
        let (owner, prior) = row.ok_or_else(|| t!("That budget no longer exists."))?;
        if owner.as_deref().is_some_and(|a| a != account)
            || prior.as_deref().is_some_and(|r| r != remote)
        {
            return Err("Account transfer requires a new copy.".into());
        }
        tx.execute(
            "UPDATE local_budgets SET account_id=?2,remote_id=?3,incarnation=?4 WHERE id=?1",
            params![id, account, remote, incarnation],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)
    }
    /// Publish an already-committed financial identity in one catalog transaction. The host
    /// separately verifies the current remote head; this cannot manufacture budget ancestry.
    pub fn activate_binding(
        &mut self,
        id: &str,
        account: &str,
        remote: &str,
        incarnation: &str,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let (folder, owner, prior): (String, Option<String>, Option<String>) = tx
            .query_row(
                "SELECT folder,account_id,remote_id FROM local_budgets WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(error)?;
        if owner.as_deref().is_some_and(|a| a != account)
            || prior.as_deref().is_some_and(|r| r != remote)
        {
            return Err("Account transfer requires a new copy.".into());
        }
        let db = Connection::open_with_flags(
            folder_path(&self.data_dir, &folder)?.join("budget.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(error)?;
        let binding = crate::sync_storage::read_binding(&db)?
            .ok_or("The financial database has no verified sync identity.")?;
        if binding.identity.account_id != account
            || binding.identity.budget_id != remote
            || binding.identity.incarnation != incarnation
        {
            return Err("Catalog activation must match the committed financial identity.".into());
        }
        let (name, currency): (String, String) = db
            .query_row("SELECT name,currency FROM metadata WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(error)?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_jobs WHERE local_budget_id=?1 AND kind='trash' AND state='pending')",[id],|r|r.get::<_,bool>(0)).map_err(error)?{return Err("Resolve the pending deletion before activating this budget.".into())}
        tx.execute("UPDATE local_budgets SET account_id=?2,remote_id=?3,incarnation=?4,lifecycle='active',availability='local',evicted=0,name=?5,currency=?6 WHERE id=?1",params![id,account,remote,incarnation,name,currency]).map_err(error)?;
        tx.execute("DELETE FROM lifecycle_jobs WHERE local_budget_id=?1 AND kind IN ('initialize','download')",[id]).map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        self.local_leases.remove(id);
        Ok(())
    }
    /// Detach only the catalog association, retaining the file and its original ancestry/intent.
    /// The original owner remains in durable recovery metadata and cannot auto-enroll again.
    pub fn quarantine_binding(&mut self, id: &str, reason: &Value) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let row: Option<(Option<String>, Option<String>, Option<String>)> = tx
            .query_row(
                "SELECT account_id,remote_id,incarnation FROM local_budgets WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(error)?;
        let (account, remote, incarnation) =
            row.ok_or_else(|| t!("That budget no longer exists."))?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT data FROM sync_hints WHERE local_budget_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        let mut hint = prior
            .map(|raw| serde_json::from_str::<Value>(&raw).map_err(error))
            .transpose()?
            .unwrap_or_else(|| json!({}));
        if account.is_some() {
            hint["quarantinedOrigin"] =
                json!({"accountId":account,"remoteId":remote,"incarnation":incarnation});
        }
        hint["quarantineReason"] = reason.clone();
        tx.execute("INSERT INTO sync_hints VALUES(?1,?2) ON CONFLICT(local_budget_id) DO UPDATE SET data=excluded.data",params![id,serde_json::to_string(&hint).map_err(error)?]).map_err(error)?;
        tx.execute("UPDATE local_budgets SET lifecycle='recovery',account_id=NULL,remote_id=NULL,incarnation=NULL,status='Needs attention' WHERE id=?1",[id]).map_err(error)?;
        tx.execute(
            "UPDATE app_state SET current_id=NULL WHERE current_id=?1",
            [id],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        if self.current == id {
            self.current.clear();
        }
        Ok(())
    }
    pub fn recovery_hint(&self, id: &str) -> Result<Option<Value>> {
        read_json(
            &self.connection,
            "SELECT data FROM sync_hints WHERE local_budget_id=?1",
            id,
        )
    }
    pub fn observe_catalog(&mut self, head: &CatalogHead) -> Result<String> {
        crate::sync_storage::valid_identity(&crate::sync_storage::RemoteIdentity {
            account_id: head.account_id.clone(),
            key_generation: head.key_generation.clone(),
            catalog_epoch: head.catalog_epoch.clone(),
            budget_id: head.budget_id.clone(),
            incarnation: head.incarnation.clone(),
        })?;
        crate::sync_storage::decimal(&head.version)?;
        if !matches!(head.state.as_str(), "active" | "trashed" | "purged") {
            return Err("Unknown remote lifecycle state.".into());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT data FROM account_catalog WHERE account_id=?1 AND remote_id=?2",
                params![head.account_id, head.budget_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        let local: Option<String> = tx
            .query_row(
                "SELECT id FROM local_budgets WHERE account_id=?1 AND remote_id=?2",
                params![head.account_id, head.budget_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        if let Some(prior) = prior {
            let prior: CatalogHead = serde_json::from_str(&prior).map_err(error)?;
            if prior.catalog_epoch == head.catalog_epoch
                && crate::sync_storage::decimal(&prior.version)?
                    > crate::sync_storage::decimal(&head.version)?
            {
                return local.ok_or("Older observation without a local catalog row.".into());
            }
        }
        tx.execute("INSERT INTO account_catalog VALUES(?1,?2,?3) ON CONFLICT(account_id,remote_id) DO UPDATE SET data=excluded.data",params![head.account_id,head.budget_id,serde_json::to_string(head).map_err(error)?]).map_err(error)?;
        let id = local.unwrap_or_else(new_id);
        tx.execute("INSERT OR IGNORE INTO local_budgets(id,folder,account_id,remote_id,incarnation,lifecycle,availability,name,currency) VALUES(?1,?2,?3,?4,?5,?6,'remote',?7,?8)",params![id,format!("v2/budgets/{id}"),head.account_id,head.budget_id,head.incarnation,head.state,head.name,head.currency]).map_err(error)?;
        // A restored incarnation is adopted only after services resolve old uploads/recovery.
        if head.state != "active" {
            tx.execute(
                "UPDATE local_budgets SET lifecycle=?2 WHERE id=?1",
                params![id, head.state],
            )
            .map_err(error)?;
            tx.execute(
                "UPDATE app_state SET current_id=NULL WHERE current_id=?1",
                [&id],
            )
            .map_err(error)?;
        }
        tx.execute(
            "UPDATE local_budgets SET name=?2,currency=?3 WHERE id=?1",
            params![id, head.name, head.currency],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        if head.state != "active" && self.current == id {
            self.current.clear();
        }
        Ok(id)
    }
    pub fn catalog_head(&self, account: &str, remote: &str) -> Result<Option<CatalogHead>> {
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT data FROM account_catalog WHERE account_id=?1 AND remote_id=?2",
                params![account, remote],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        raw.map(|s| serde_json::from_str(&s).map_err(error))
            .transpose()
    }
    pub fn catalog_heads(&self, account: &str) -> Result<Vec<CatalogHead>> {
        let mut q = self
            .connection
            .prepare("SELECT data FROM account_catalog WHERE account_id=?1 ORDER BY remote_id")
            .map_err(error)?;
        let rows = q
            .query_map([account], |r| r.get::<_, String>(0))
            .map_err(error)?;
        rows.map(|r| serde_json::from_str(&r.map_err(error)?).map_err(error))
            .collect()
    }
    pub fn observe_catalog_json(
        &mut self,
        account: &str,
        generation: &str,
        epoch: &str,
        head: &Value,
        name: Option<&str>,
        currency: Option<&str>,
    ) -> Result<String> {
        let text = |keys: &[&str]| -> Result<String> {
            keys.iter()
                .find_map(|k| head.get(k).and_then(Value::as_str))
                .map(str::to_owned)
                .ok_or_else(|| format!("Catalog head omitted {}.", keys[0]))
        };
        self.observe_catalog(&CatalogHead {
            account_id: account.into(),
            key_generation: generation.into(),
            catalog_epoch: epoch.into(),
            budget_id: text(&["budgetId", "id"])?,
            incarnation: text(&["incarnation"])?,
            version: text(&["version"])?,
            state: text(&["state"])?,
            name: name.unwrap_or("Budget").into(),
            currency: currency.unwrap_or("").into(),
            metadata: head.clone(),
        })
    }
    pub fn cached_catalog_head(&self, account: &str, remote: &str) -> Result<Option<Value>> {
        Ok(self.catalog_head(account, remote)?.map(|h| h.metadata))
    }
    pub fn list(&self) -> Vec<Listing> {
        self.list_all()
            .into_iter()
            .filter(|b| b.lifecycle == "active")
            .collect()
    }
    pub fn list_all(&self) -> Vec<Listing> {
        self.try_list_all().unwrap_or_default()
    }
    pub fn try_list_all(&self) -> Result<Vec<Listing>> {
        let mut q=self.connection.prepare("SELECT id,folder,last_opened,account_id,remote_id,incarnation,lifecycle,availability,name,currency,status FROM local_budgets").map_err(error)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    r.get::<_, String>(9)?,
                    r.get::<_, Option<String>>(10)?,
                ))
            })
            .map_err(error)?;
        let mut out = vec![];
        for row in rows {
            let (
                id,
                folder,
                last_opened,
                account_id,
                remote_id,
                incarnation,
                lifecycle,
                mut availability,
                mut name,
                mut currency,
                explicit,
            ) = row.map_err(error)?;
            let path = folder_path(&self.data_dir, &folder)?.join("budget.sqlite3");
            let mut dirty = false;
            if path.is_file() {
                if let Ok(db) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                {
                    if let Ok((n, c)) =
                        db.query_row("SELECT name,currency FROM metadata WHERE id=1", [], |r| {
                            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                        })
                    {
                        name = n;
                        currency = c;
                    }
                    dirty=db.query_row("SELECT dirty OR EXISTS(SELECT 1 FROM sync_upload_v2 WHERE state='prepared') FROM sync_local_state WHERE id=1",[],|r|r.get(0)).unwrap_or(true);
                }
            } else if availability == "local" {
                availability = "unavailable".into();
            }
            let pending:bool=self.connection.query_row("SELECT EXISTS(SELECT 1 FROM lifecycle_jobs WHERE local_budget_id=?1 AND state='pending')",[&id],|r|r.get(0)).map_err(error)?;
            let dirty = dirty && account_id.is_some();
            let explicit = explicit.filter(|status| {
                !((dirty || pending)
                    && matches!(
                        status.as_str(),
                        "Synced" | "synced" | "uploaded" | "downloaded" | "merged"
                    ))
            });
            let status = explicit.unwrap_or_else(|| {
                if lifecycle == "trashed" {
                    if pending {
                        "Deletion waiting"
                    } else {
                        "In Trash"
                    }
                } else if lifecycle == "recovery" || availability == "unavailable" {
                    "Needs attention"
                } else if availability == "remote" {
                    "Available online"
                } else if availability == "evicted" {
                    "Download removed"
                } else if account_id.is_none() {
                    "Saved on this device"
                } else if dirty || pending {
                    "Waiting"
                } else {
                    "Synced"
                }
                .into()
            });
            out.push(Listing {
                id: id.clone(),
                name,
                currency,
                current: self.current == id,
                last_opened,
                path: path.to_string_lossy().into_owned(),
                missing: !path.is_file(),
                scope: if account_id.is_some() {
                    "account"
                } else {
                    "local"
                }
                .into(),
                account_id,
                remote_id,
                incarnation,
                lifecycle,
                availability,
                pending: pending || dirty,
                status,
            });
        }
        out.sort_by(|a, b| {
            b.current
                .cmp(&a.current)
                .then(b.last_opened.cmp(&a.last_opened))
                .then(a.name.cmp(&b.name))
        });
        Ok(out)
    }
}

impl Budgets {
    pub fn ensure_account(&mut self, account: &str) -> Result<Value> {
        if account.is_empty() {
            return Err("A stable account ID is required.".into());
        }
        self.connection
            .execute(
                "INSERT OR IGNORE INTO account_sync(account_id,installation_id) VALUES(?1,?2)",
                params![account, new_id()],
            )
            .map_err(error)?;
        self.account_state(account)
    }
    pub fn account_state(&self, account: &str) -> Result<Value> {
        let row: Option<(String, String, String)> = self
            .connection
            .query_row(
                "SELECT installation_id,next_sequence,data FROM account_sync WHERE account_id=?1",
                [account],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(error)?;
        let Some((installation, sequence, raw)) = row else {
            return Ok(Value::Null);
        };
        let mut value: Value = serde_json::from_str(&raw).map_err(error)?;
        value["accountId"] = json!(account);
        value["installationId"] = json!(installation);
        value["nextSequence"] = json!(sequence);
        Ok(value)
    }
    pub fn set_account_state(&mut self, account: &str, value: &Value) -> Result<()> {
        if !value.is_object() {
            return Err("Account state must be an object.".into());
        }
        self.ensure_account(account)?;
        let mut value = value.clone();
        for key in ["accountId", "installationId", "nextSequence"] {
            value.as_object_mut().unwrap().remove(key);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        tx.execute(
            "UPDATE account_sync SET data=?2 WHERE account_id=?1",
            params![account, serde_json::to_string(&value).map_err(error)?],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)
    }
    pub fn enqueue_job(&mut self, job: &LifecycleJob) -> Result<()> {
        if matches!(job.kind.as_str(), "initialize" | "download") {
            let id = job
                .local_budget_id
                .as_deref()
                .ok_or("A local file job requires a budget ID.")?;
            if !self.local_leases.contains_key(id) {
                let lease = self
                    .claim_local_lease(id)?
                    .ok_or("Another process is initializing this budget.")?;
                self.local_leases.insert(id.into(), lease);
            }
        }
        let raw = serde_json::to_string(job).map_err(error)?;
        let existing: Option<String> = self
            .connection
            .query_row(
                "SELECT data FROM lifecycle_jobs WHERE operation_id=?1",
                [&job.operation_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)?;
        if let Some(existing) = existing {
            return if existing == raw {
                Ok(())
            } else {
                Err("A lifecycle operation ID cannot be reused.".into())
            };
        }
        self.connection
            .execute(
                "INSERT INTO lifecycle_jobs VALUES(?1,?2,?3,?4,?5)",
                params![
                    job.operation_id,
                    job.local_budget_id,
                    job.kind,
                    job.state,
                    raw
                ],
            )
            .map_err(error)?;
        bump(&self.connection)
    }
    pub fn job(&self, id: &str) -> Result<Option<LifecycleJob>> {
        read_json(
            &self.connection,
            "SELECT data FROM lifecycle_jobs WHERE operation_id=?1",
            id,
        )
    }
    /// Return a deferred deletion to the library for review without starting a restore.
    /// The caller verifies the authoritative same-incarnation active head first.
    pub fn review_deletion(&mut self, id: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let jobs: Vec<(String, String)> = {
            let mut q = tx.prepare("SELECT operation_id,data FROM lifecycle_jobs WHERE local_budget_id=?1 AND kind IN ('trash','deletion_conflict') AND state!='complete'").map_err(error)?;
            let rows = q
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(error)?;
            rows.collect::<std::result::Result<_, _>>().map_err(error)?
        };
        for (operation, raw) in jobs {
            let mut job: LifecycleJob = serde_json::from_str(&raw).map_err(error)?;
            job.state = "complete".into();
            job.receipt = Some(json!({"outcome":"cancelled","code":"review_newer_version"}));
            tx.execute(
                "UPDATE lifecycle_jobs SET state='complete',data=?2 WHERE operation_id=?1",
                params![operation, serde_json::to_string(&job).map_err(error)?],
            )
            .map_err(error)?;
        }
        tx.execute(
            "UPDATE local_budgets SET lifecycle='active',status='waiting' WHERE id=?1",
            [id],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)
    }
    pub fn latest_trash_job(&self, id: &str) -> Result<Option<LifecycleJob>> {
        read_json(&self.connection,
            "SELECT data FROM lifecycle_jobs WHERE local_budget_id=?1 AND kind='trash' ORDER BY rowid DESC LIMIT 1", id)
    }
    pub fn jobs(&self) -> Result<Vec<LifecycleJob>> {
        let mut q=self.connection.prepare("SELECT data FROM lifecycle_jobs WHERE state IN ('pending','recovery') ORDER BY rowid").map_err(error)?;
        let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(error)?;
        rows.map(|r| serde_json::from_str(&r.map_err(error)?).map_err(error))
            .collect()
    }
    pub fn finish_job(&mut self, id: &str, receipt: &Value) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let mut job = read_json::<LifecycleJob>(
            &tx,
            "SELECT data FROM lifecycle_jobs WHERE operation_id=?1",
            id,
        )?
        .ok_or("Lifecycle operation not found.")?;
        if job.state == "complete" {
            return if job.receipt.as_ref() == Some(receipt) {
                Ok(())
            } else {
                Err("A lifecycle receipt changed.".into())
            };
        }
        job.state = "complete".into();
        job.receipt = Some(receipt.clone());
        tx.execute(
            "UPDATE lifecycle_jobs SET state='complete',data=?2 WHERE operation_id=?1",
            params![id, serde_json::to_string(&job).map_err(error)?],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)?;
        if matches!(job.kind.as_str(), "initialize" | "download") {
            if let Some(id) = job.local_budget_id.as_deref() {
                self.local_leases.remove(id);
            }
        }
        Ok(())
    }
    pub fn pending_delivery(&self, account: &str) -> Result<Option<DeliverySlot>> {
        read_json(
            &self.connection,
            "SELECT data FROM delivery_slot WHERE account_id=?1",
            account,
        )
    }
    pub fn prepare_delivery(
        &mut self,
        account: &str,
        op: &str,
        local: Option<&str>,
        manifest: &Value,
        payload: &[u8],
        metadata: &str,
        digest: &str,
    ) -> Result<DeliverySlot> {
        self.ensure_account(account)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        if let Some(saved) = read_json::<DeliverySlot>(
            &tx,
            "SELECT data FROM delivery_slot WHERE account_id=?1",
            account,
        )? {
            return if saved.operation_id == op
                && saved.local_budget_id.as_deref() == local
                && saved.manifest == *manifest
                && saved.payload == payload
                && saved.metadata == metadata
                && saved.digest == digest
            {
                Ok(saved)
            } else {
                Err("Resolve the outstanding delivery before allocating another sequence.".into())
            };
        }
        if let Some(id) = local {
            let folder: String = tx
                .query_row("SELECT folder FROM local_budgets WHERE id=?1", [id], |r| {
                    r.get(0)
                })
                .map_err(error)?;
            let path = folder_path(&self.data_dir, &folder)?.join("budget.sqlite3");
            let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(error)?;
            let intent = read_json::<crate::sync_storage::UploadIntent>(
                &db,
                "SELECT data FROM sync_upload_v2 WHERE operation_id=?1 AND state='prepared'",
                op,
            )?
            .ok_or("Persist the budget upload intent before allocating account delivery.")?;
            if intent.identity.account_id != account
                || intent.manifest != *manifest
                || intent.payload != payload
                || intent.metadata != metadata
                || intent.digest != digest
            {
                return Err(
                    "Account delivery must exactly match its durable budget upload intent.".into(),
                );
            }
        }
        let (installation, sequence): (String, String) = tx
            .query_row(
                "SELECT installation_id,next_sequence FROM account_sync WHERE account_id=?1",
                [account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(error)?;
        if manifest
            .get("installationId")
            .and_then(Value::as_str)
            .is_some_and(|i| i != installation)
            || manifest
                .get("sequence")
                .and_then(Value::as_str)
                .is_some_and(|s| s != sequence)
        {
            return Err("Manifest installation/sequence does not match its durable slot.".into());
        }
        let slot = DeliverySlot {
            account_id: account.into(),
            installation_id: installation,
            sequence,
            operation_id: op.into(),
            local_budget_id: local.map(str::to_owned),
            manifest: manifest.clone(),
            payload: payload.into(),
            metadata: metadata.into(),
            digest: digest.into(),
            state: "prepared".into(),
            receipt: None,
        };
        tx.execute(
            "INSERT INTO delivery_slot VALUES(?1,?2)",
            params![account, serde_json::to_string(&slot).map_err(error)?],
        )
        .map_err(error)?;
        tx.commit().map_err(error)?;
        Ok(slot)
    }
    pub fn record_delivery_receipt(
        &mut self,
        account: &str,
        sequence: &str,
        digest: &str,
        receipt: &Value,
    ) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        let mut slot = read_json::<DeliverySlot>(
            &tx,
            "SELECT data FROM delivery_slot WHERE account_id=?1",
            account,
        )?
        .ok_or("Missing delivery slot; preserve the outcome for recovery.")?;
        if slot.sequence != sequence || slot.digest != digest {
            return Err("Receipt identity does not match delivery.".into());
        }
        if slot.receipt.as_ref().is_some_and(|r| r != receipt) {
            return Err("A terminal delivery result cannot change.".into());
        }
        slot.state = "received".into();
        slot.receipt = Some(receipt.clone());
        tx.execute(
            "UPDATE delivery_slot SET data=?2 WHERE account_id=?1",
            params![account, serde_json::to_string(&slot).map_err(error)?],
        )
        .map_err(error)?;
        tx.commit().map_err(error)
    }
    /// Advance only after the separate per-budget/lifecycle receipt marker is durable.
    pub fn complete_delivery(&mut self, account: &str, sequence: &str, digest: &str) -> Result<()> {
        let Some(slot) = self.pending_delivery(account)? else {
            let prior: Option<String> = self
                .connection
                .query_row(
                    "SELECT digest FROM delivery_completed WHERE account_id=?1 AND sequence=?2",
                    params![account, sequence],
                    |r| r.get(0),
                )
                .optional()
                .map_err(error)?;
            return if prior.as_deref() == Some(digest) {
                Ok(())
            } else {
                Err("The delivery slot is missing.".into())
            };
        };
        if slot.sequence != sequence || slot.digest != digest {
            return Err("Completed operation differs from the delivery slot.".into());
        }
        let receipt = slot
            .receipt
            .as_ref()
            .ok_or("Persist the receipt before completing delivery.")?;
        if let Some(id) = &slot.local_budget_id {
            let db =
                Connection::open_with_flags(self.database(id)?, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(error)?;
            let applied: Option<Value> = read_json(
                &db,
                "SELECT receipt FROM sync_upload_v2 WHERE operation_id=?1 AND state='terminal'",
                &slot.operation_id,
            )?;
            if applied.as_ref() != Some(receipt) {
                return Err(
                    "Acknowledge the budget before completing its account delivery.".into(),
                );
            }
        } else if self
            .job(&slot.operation_id)?
            .is_none_or(|j| j.state != "complete" || j.receipt.as_ref() != Some(receipt))
        {
            return Err("Apply the lifecycle/control receipt before completing delivery.".into());
        }
        let next = crate::sync_storage::decimal(sequence)?
            .checked_add(1)
            .ok_or("Installation sequence exhausted.")?
            .to_string();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        if read_json::<DeliverySlot>(
            &tx,
            "SELECT data FROM delivery_slot WHERE account_id=?1",
            account,
        )?
        .as_ref()
            != Some(&slot)
        {
            return Err("Delivery changed during completion.".into());
        }
        if tx
            .execute(
                "UPDATE account_sync SET next_sequence=?2 WHERE account_id=?1 AND next_sequence=?3",
                params![account, next, sequence],
            )
            .map_err(error)?
            != 1
        {
            return Err("Installation sequence changed during completion.".into());
        }
        tx.execute(
            "INSERT OR IGNORE INTO delivery_completed VALUES(?1,?2,?3)",
            params![account, sequence, digest],
        )
        .map_err(error)?;
        tx.execute("DELETE FROM delivery_slot WHERE account_id=?1", [account])
            .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)
    }
    pub fn evict(&mut self, id: &str) -> Result<()> {
        let _lease = self
            .claim_local_lease(id)?
            .ok_or("Another process is initializing or removing this budget.")?;
        self.evict_locked(id)
    }
    fn evict_locked(&mut self, id: &str) -> Result<()> {
        if self.current == id {
            return Err("Close this budget before removing its download.".into());
        }
        let entry = self
            .try_list_all()?
            .into_iter()
            .find(|b| b.id == id)
            .ok_or("Budget not found.")?;
        let account = entry
            .account_id
            .as_deref()
            .ok_or("A local-only budget cannot be evicted.")?;
        if entry.availability == "evicted" {
            return Ok(());
        }
        if self
            .pending_delivery(account)?
            .is_some_and(|d| d.local_budget_id.as_deref() == Some(id))
        {
            return Err("Resolve this budget's delivery before eviction.".into());
        }
        let path = self.database(id)?;
        if !path.starts_with(self.data_dir.join("v2/budgets")) {
            return Err("Migrate legacy storage before eviction.".into());
        }
        let job = LifecycleJob {
            operation_id: format!("evict-{id}"),
            local_budget_id: Some(id.into()),
            kind: "evict".into(),
            payload: json!({"path":path}),
            state: "pending".into(),
            receipt: None,
        };
        self.enqueue_job(&job)?;
        if path.is_file() {
            let mut store = crate::store::Store::open_budget(&path, id)?
                .with_backup_preferences(&self.data_dir);
            if store.folder().join("recovery.json").exists() {
                return Err(
                    "Save, sync and resolve recovery before removing this download.".into(),
                );
            }
            // Freeze the checked snapshot under the same SQLite write lock. A different
            // process cannot save between the cleanliness check and physical eviction.
            let tx = store
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(error)?;
            let unsafe_state:bool=tx.query_row("SELECT dirty OR EXISTS(SELECT 1 FROM sync_upload_v2 WHERE state='prepared') OR EXISTS(SELECT 1 FROM editor_checkpoints) FROM sync_local_state WHERE id=1",[],|r|r.get(0)).map_err(error)?;
            if unsafe_state {
                return Err(
                    "Save, sync and resolve recovery before removing this download.".into(),
                );
            }
            tx.execute("UPDATE sync_local_state SET retired=1 WHERE id=1", [])
                .map_err(error)?;
            tx.commit().map_err(error)?;
            store.backup()?;
        }
        for file in [
            path.clone(),
            PathBuf::from(format!("{}-wal", path.display())),
            PathBuf::from(format!("{}-shm", path.display())),
        ] {
            if file.exists() {
                std::fs::remove_file(file).map_err(error)?;
            }
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(error)?;
        tx.execute(
            "UPDATE local_budgets SET availability='evicted',evicted=1 WHERE id=?1",
            [id],
        )
        .map_err(error)?;
        tx.execute(
            "DELETE FROM lifecycle_jobs WHERE operation_id=?1",
            [&job.operation_id],
        )
        .map_err(error)?;
        bump(&tx)?;
        tx.commit().map_err(error)
    }
}
