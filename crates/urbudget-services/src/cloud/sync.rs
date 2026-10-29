//! End-to-end encrypted sync of whole budgets, merged record by record. Each synced budget has a
//! revision on the server and, on each device, a sync base: the version it last synced as, kept
//! in the budget's own database. This device also remembers, per budget, the server revision and
//! its own saved revision at the last sync. Comparing both with now says what to do:
//!
//! | this device changed | server changed | action |
//! | --- | --- | --- |
//! | no | no | nothing |
//! | yes | no | upload (only if the server is still at the remembered revision) |
//! | either | yes | download, merge with this device's changes against the base, save, upload the result |
//!
//! The merge (`crate::sync_merge`) only asks the person about true conflicts: a field both sides
//! changed differently, or a record one side deleted while the other edited it. The merged budget
//! and its new base are saved in one transaction, so a merge is never applied twice, even when
//! the upload after it fails.
//!
//! Budgets are encrypted on this device before upload (see `crypto`); the server only keeps bytes.
use crate::t;
use super::{crypto, device_name, service, vault, Cloud, Failure, Session};
use crate::budgets::Listing;
use crate::domain::{Budget, SCHEMA_VERSION};
use crate::store::{Store, HEAVY_EXTRAS};
use crate::sync_merge::{self, Choices, Conflict};
use crate::{Database, Library};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const KEY_SERVICE: &str = "com.urbudget.sync-key";
const FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Tracked {
    /// The server's revision at the last sync (0: never uploaded).
    revision: u64,
    /// This device's saved revision at the last sync (-1: unknown, or changed since).
    local_revision: i64,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Ledger {
    budgets: BTreeMap<String, Tracked>,
    #[serde(default)]
    generation: u64,
}

/// Whether the sync base saved with a budget is the version of its last sync here. It's written
/// with the merged budget, before the ledger catches up, so it may be newer; an older one comes
/// from before sync was reset or a backup was restored.
fn trusted(tracked: Option<Tracked>, base_revision: u64) -> bool {
    tracked.is_some_and(|t| t.revision > 0 && base_revision >= t.revision)
}

/// What a tracked budget that's also in the cloud needs.
#[derive(Debug, PartialEq)]
enum Need {
    Nothing,
    /// Upload over this server revision.
    Upload(u64),
    /// Bring the cloud's version in. `unchanged` is this device's revision when it has no
    /// changes since the last sync.
    BringIn { unchanged: Option<i64> },
}
fn need(t: Tracked, local_revision: i64, base_revision: Option<u64>, remote_revision: u64) -> Need {
    let synced = base_revision.filter(|b| trusted(Some(t), *b)).unwrap_or(t.revision);
    let local_changed = local_revision != t.local_revision;
    match (local_changed, remote_revision != synced) {
        (false, false) => Need::Nothing,
        (true, false) => Need::Upload(synced),
        (changed, true) => Need::BringIn { unchanged: (!changed).then_some(local_revision) },
    }
}

/// What sync did or needs, budget by budget.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSync {
    pub id: String,
    pub name: String,
    /// "synced", "uploaded", "downloaded", "merged", "conflict", "cloud_only", "local_only",
    /// "waiting", "update_app" or "error".
    pub state: &'static str,
    pub device: Option<String>,
    pub updated_at: Option<String>,
    pub message: Option<String>,
    /// For "conflict": what the person needs to choose. Empty when the two versions couldn't be
    /// combined at all, and then the choice is between whole budgets.
    pub conflicts: Vec<Conflict>,
    /// What a merge decided on its own that the person may want to know.
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review: Option<Value>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedBudget {
    pub id: String,
    /// Opaque saved snapshot token; empty when this identity has no readable local content.
    pub revision: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub at: String,
    /// "ok", "signed_out", "locked", "subscription_required", "offline" or "error".
    pub state: &'static str,
    pub message: Option<String>,
    pub budgets: Vec<BudgetSync>,
    /// The open budget was replaced by a downloaded version: the window reloads it.
    pub open_changed: bool,
    /// V2 consumers target the current local identity; the legacy boolean remains compatible.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_budgets: Option<Vec<ChangedBudget>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_version: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// This computer has the sync key (the passphrase was entered here).
    pub unlocked: bool,
    pub protocol: u8,
    pub enabled: bool,
    pub last: Option<Report>,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: u32,
    app: String,
    budget: Budget,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Remote {
    id: String,
    revision: u64,
    meta: String,
    device: String,
    updated_at: String,
}

pub struct Engine {
    file: PathBuf,
    state: Mutex<Ledger>,
    last: Mutex<Option<Report>>,
    /// Where the system has no credential store, the key lasts until the app closes.
    memory_key: Mutex<Option<crypto::Key>>,
    /// One cloud operation at a time, also coordinated with other app processes.
    running: tokio::sync::Mutex<()>,
}

pub(crate) struct Operation<'a> {
    _local: tokio::sync::MutexGuard<'a, ()>,
    _file: std::fs::File,
}

/// What one pass needs to reach the budgets and the cloud.
struct Ctx<'a> {
    cloud: &'a Cloud,
    db: &'a Database,
    library: &'a Library,
    open_id: String,
    /// The window has no unsaved edits.
    open_clean: bool,
    key: crypto::Key,
    session: Session,
    generation: u64,
}

/// What came of one budget.
struct Step {
    state: &'static str,
    message: Option<String>,
    conflicts: Vec<Conflict>,
    notes: Vec<String>,
    open_changed: bool,
}
impl From<&'static str> for Step {
    fn from(state: &'static str) -> Self {
        Step { state, message: None, conflicts: vec![], notes: vec![], open_changed: false }
    }
}

fn budget_aad(id: &str) -> Vec<u8> {
    format!("urbudget:budget:{id}").into_bytes()
}
fn meta_aad(id: &str) -> Vec<u8> {
    format!("urbudget:meta:{id}").into_bytes()
}
fn without_heavy(mut budget: Budget) -> Budget {
    for k in HEAVY_EXTRAS {
        budget.extra.remove(*k);
    }
    budget
}
pub(super) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn error(message: impl Into<String>) -> Failure {
    Failure { status: 0, code: "error".into(), message: message.into(), body: Value::Null }
}

/// Runs `f` on a budget's store: the window's for the open budget (so its draft is seen), a
/// store of its own for any other.
fn with_store<T>(ctx: &Ctx, id: &str, f: impl FnOnce(&mut Store, bool) -> Result<T, String>) -> Result<T, String> {
    ctx.cloud.check_session(&ctx.session)?;
    // Match budget switching's lock order and hold both locks through the operation.
    let library = ctx.library.lock();
    ensure_legacy_budget(&library, id)?;
    let open_id = ctx.db.store().ok().map(|s| s.id().to_owned()).unwrap_or_default();
    if open_id != ctx.open_id {
        return Err(t!("The open budget changed during sync. Sync again to continue."));
    }
    if id == ctx.open_id && !id.is_empty() {
        f(&mut *ctx.db.store()?, true)
    } else {
        let path = library.database(id)?;
        if !path.is_file() { return Err(t!("Download this budget before opening it.")); }
        f(&mut Store::open_budget(&path, id)?.with_backup_preferences(library.data_dir()), false)
    }
}

/// V2 ownership never becomes implicit permission to copy ciphertext/plaintext into a v1 account.
fn ensure_legacy_budget(library: &crate::budgets::Budgets, id: &str) -> Result<(), String> {
    if library.try_list_all()?.iter().any(|e| e.id == id && (e.account_id.is_some() || e.lifecycle != "active")) {
        return Err(t!("This budget belongs to the account library and cannot use legacy sync."));
    }
    Ok(())
}

/// What came of bringing the cloud's version into this device's.
#[derive(Debug)]
enum Absorbed {
    /// The window has unsaved edits, or this device changed since the check: next time.
    Later,
    Conflicts(Vec<Conflict>),
    /// The two versions couldn't be combined; the person chooses between whole budgets.
    Unmergeable(String),
    Saved {
        merged: Budget,
        /// This device's saved revision now.
        local_revision: i64,
        /// The merge changed this device's budget.
        local_changed: bool,
        /// The merge has changes the cloud doesn't.
        upload: bool,
        notes: Vec<String>,
    },
}

/// Merges the cloud's version (at server `revision`) into this device's saved budget and saves
/// the result with the cloud's version as the new sync base. No network: the caller uploads.
fn absorb(
    store: &mut Store,
    window_dirty: bool,
    tracked: Option<Tracked>,
    unchanged: Option<i64>,
    remote: &Budget,
    revision: u64,
    choices: &Choices,
) -> Result<Absorbed, String> {
    // Never under the window's unsaved edits: wait for them to be saved first.
    if window_dirty || store.has_unsaved_draft() {
        return Ok(Absorbed::Later);
    }
    let (saved, status) = store.saved()?;
    let local = without_heavy(saved);
    let local_revision = status.revision;
    if unchanged.is_some_and(|expected| expected != local_revision) {
        // Retry from a new snapshot, so a save during the download is considered by need().
        return Ok(Absorbed::Later);
    }
    // A cloud copy at an older revision than this device synced was uploaded again from scratch
    // (sync was reset elsewhere), so it may not descend from either: merged without a base.
    let behind = |r: u64| r > revision;
    let base = match store.sync_base()?.filter(|(r, _)| trusted(tracked, *r) && !behind(*r)) {
        Some((_, base)) => Some(base),
        None if tracked.is_some_and(|t| behind(t.revision)) => None,
        // Unchanged since the last sync, this device's budget is that version.
        None if unchanged == Some(local_revision) => Some(local.clone()),
        None if unchanged.is_some() => return Ok(Absorbed::Later),
        None => None,
    };
    let merged = match sync_merge::merge(base.as_ref(), &local, remote, choices) {
        Ok(m) => m,
        Err(e) => return Ok(Absorbed::Unmergeable(e)),
    };
    if !merged.conflicts.is_empty() {
        return Ok(Absorbed::Conflicts(merged.conflicts));
    }
    let local_changed = merged.budget != local;
    if local_changed && (!local.accounts.is_empty() || !local.transactions.is_empty()) {
        store.backup()?;
    }
    let acknowledged = store.save_synced(&merged.budget, &status.token, revision, remote)?;
    Ok(Absorbed::Saved {
        upload: merged.budget != *remote,
        local_revision: acknowledged.revision,
        local_changed,
        merged: merged.budget,
        notes: merged.notes,
    })
}

/// Applies an explicit choice to keep the whole cloud version, provided the local version
/// the person chose to replace has not changed while its download was pending.
fn replace_with_cloud(store: &mut Store, expected_revision: i64, budget: &Budget, revision: u64) -> Result<i64, String> {
    if store.has_unsaved_draft() {
        return Err(t!("Save your changes first."));
    }
    let (local, status) = store.saved()?;
    if status.revision != expected_revision {
        return Err(t!("The local budget changed during the download. Review it and try again."));
    }
    if !local.accounts.is_empty() || !local.transactions.is_empty() {
        store.backup()?;
    }
    Ok(store.save_synced(budget, &status.token, revision, budget)?.revision)
}

impl Engine {
    pub fn load(data_dir: &Path) -> Self {
        let file = data_dir.join("sync-state.json");
        let state = std::fs::read(&file)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
        Self {
            file,
            state: Mutex::new(state),
            last: Mutex::new(None),
            memory_key: Mutex::new(None),
            running: tokio::sync::Mutex::new(()),
        }
    }
    fn state(&self) -> Ledger {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub(crate) async fn operation(&self) -> Result<Operation<'_>, String> {
        let local = self.running.lock().await;
        let path = self.file.with_file_name("cloud-operation.lock");
        // Waiting on another process must not block the async executor or UI thread.
        let file = tokio::task::spawn_blocking(move || -> Result<std::fs::File, String> {
            let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
                .open(path).map_err(|e| e.to_string())?;
            vault::lock_exclusive(&file).map_err(|e| e.to_string())?;
            Ok(file)
        }).await.map_err(|e| e.to_string())??;
        // The previous process may have reset sync or updated tracking while we waited.
        let state = match std::fs::read(&self.file) {
            Ok(raw) => serde_json::from_slice(&raw).map_err(|e| t!("Could not read sync state: {e}", e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ledger::default(),
            Err(e) => return Err(e.to_string()),
        };
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
        Ok(Operation { _local: local, _file: file })
    }
    fn set_state(&self, update: impl FnOnce(&mut Ledger)) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        update(&mut state);
        let raw = serde_json::to_vec_pretty(&*state).map_err(|e| e.to_string())?;
        let temporary = self.file.with_extension("json.tmp");
        std::fs::write(&temporary, raw).map_err(|e| e.to_string())?;
        std::fs::rename(temporary, &self.file).map_err(|e| e.to_string())
    }
    fn track_at(&self, id: &str, revision: u64, local_revision: i64) -> Result<(), Failure> {
        self.set_state(|s| {
            s.budgets.insert(id.into(), Tracked { revision, local_revision });
        })
        .map_err(error)
    }
    fn key(&self) -> Option<crypto::Key> {
        if vault::STORE.is_some() {
            vault::read(&service(KEY_SERVICE))
                .and_then(|k| URL_SAFE_NO_PAD.decode(k).ok())
                .and_then(|k| k.try_into().ok())
        } else {
            *self.memory_key.lock().unwrap_or_else(|e| e.into_inner())
        }
    }
    fn set_key(&self, key: Option<crypto::Key>) -> Result<(), String> {
        match key {
            Some(key) if vault::STORE.is_some() => vault::save(&service(KEY_SERVICE), &URL_SAFE_NO_PAD.encode(key)),
            other => {
                vault::delete(&service(KEY_SERVICE))?;
                *self.memory_key.lock().unwrap_or_else(|e| e.into_inner()) = other;
                Ok(())
            }
        }
    }
    fn install_key(&self, key: crypto::Key, generation: u64) -> Result<(), String> {
        self.install_key_with(key, generation, |key| self.set_key(key))
    }
    fn install_key_with(&self, key: crypto::Key, generation: u64, write_key: impl Fn(Option<crypto::Key>) -> Result<(), String>) -> Result<(), String> {
        // Clear first: a failed state write must not pair an old key with a new generation.
        write_key(None)?;
        self.set_state(|s| {
            if s.generation != generation { s.budgets.clear(); }
            s.generation = generation;
        })?;
        write_key(Some(key))
    }
    pub fn status(&self) -> Status {
        Status {
            unlocked: self.key().is_some(),
            protocol: 1,
            enabled: self.key().is_some(),
            last: self.last.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        }
    }
    /// Forgets the key and what was synced (signing out, or a new passphrase).
    pub fn forget(&self) -> Result<(), String> {
        self.forget_with(|| self.set_key(None))
    }
    pub(super) fn forget_with(&self, clear_key: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        clear_key()?;
        self.set_state(|s| { s.budgets.clear(); s.generation = 0; })?;
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
    pub(super) fn finish(&self, report: Report) -> Report {
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(report.clone());
        report
    }

    fn decode_meta(key: &crypto::Key, id: &str, meta: &str) -> Option<String> {
        let sealed = URL_SAFE_NO_PAD.decode(meta).ok()?;
        let plain = crypto::open(key, &meta_aad(id), &sealed).ok()?;
        serde_json::from_slice::<Value>(&plain).ok()?["name"].as_str().map(str::to_owned)
    }
    fn seal_budget(key: &crypto::Key, id: &str, budget: Budget) -> Result<(Vec<u8>, String), String> {
        let meta = json!({ "name": budget.name, "currency": budget.currency });
        let envelope = Envelope { format: FORMAT, app: env!("CARGO_PKG_VERSION").into(), budget };
        let plain = crypto::compress(&serde_json::to_vec(&envelope).map_err(|e| e.to_string())?)?;
        let sealed = crypto::seal(key, &budget_aad(id), &plain)?;
        let meta = URL_SAFE_NO_PAD.encode(crypto::seal(key, &meta_aad(id), meta.to_string().as_bytes())?);
        Ok((sealed, meta))
    }
    /// Decrypts a downloaded budget. `Ok(None)` means a newer UrBudget made it.
    fn open_budget(key: &crypto::Key, id: &str, bytes: &[u8]) -> Result<Option<Budget>, String> {
        let plain = crypto::open(key, &budget_aad(id), bytes).map_err(|e| {
            if e == crypto::WRONG_KEY {
                t!("This budget was encrypted with a different sync passphrase.")
            } else {
                e
            }
        })?;
        let raw: Value = serde_json::from_slice(&crypto::decompress(&plain)?).map_err(|e| e.to_string())?;
        let newer = raw["format"].as_u64().unwrap_or(0) > FORMAT as u64
            || raw["budget"]["schemaVersion"].as_u64().unwrap_or(0) > SCHEMA_VERSION as u64;
        if newer {
            return Ok(None);
        }
        let envelope: Envelope = serde_json::from_value(raw).map_err(|e| t!("The synced budget couldn't be read: {e}", e))?;
        crate::domain::validate(&envelope.budget)?;
        Ok(Some(without_heavy(envelope.budget)))
    }

    async fn remotes(&self, cloud: &Cloud, session: &Session, generation: u64) -> Result<Vec<Remote>, Failure> {
        let answer = cloud.call_session(reqwest::Method::GET, "/sync/budgets", None, Some(session), Some(generation)).await.map_err(|f| self.handle_failure(f))?;
        serde_json::from_value(answer["budgets"].clone()).map_err(|e| error(e.to_string()))
    }
    fn handle_failure(&self, failure: Failure) -> Failure {
        if matches!(failure.code.as_str(), "sync_key_changed" | "sync_generation_required") {
            let _ = self.set_key(None);
        }
        failure
    }
    /// The cloud's bytes and the revision they are (`listed` when the server doesn't say).
    async fn download(&self, cloud: &Cloud, session: &Session, generation: u64, id: &str, listed: u64) -> Result<(Vec<u8>, u64), Failure> {
        let response = cloud
            .for_session(session, cloud.request(reqwest::Method::GET, &format!("/sync/budgets/{id}")))?
            .header("X-Sync-Generation", generation.to_string())
            .send()
            .await
            .map_err(super::offline)?;
        cloud.check_session(session)?;
        if !response.status().is_success() {
            let failure = super::failure(response).await;
            cloud.check_session(session)?;
            return Err(self.handle_failure(failure));
        }
        let revision = response
            .headers()
            .get("X-Revision")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(listed);
        let bytes = response.bytes().await.map_err(super::offline)?.to_vec();
        cloud.check_session(session)?;
        Ok((bytes, revision))
    }
    /// Uploads over `expected`; `Ok(Err(current))` when another device got there first.
    async fn upload(&self, cloud: &Cloud, session: &Session, generation: u64, id: &str, expected: u64, bytes: Vec<u8>, meta: &str) -> Result<Result<u64, u64>, Failure> {
        let response = cloud
            .for_session(session, cloud.request(reqwest::Method::PUT, &format!("/sync/budgets/{id}")))?
            .header("X-Sync-Generation", generation.to_string())
            .header("If-Match", expected.to_string())
            .header("X-Sync-Meta", meta)
            .header("X-Device", device_name())
            .header("Content-Type", "application/octet-stream")
            .body(bytes)
            .send()
            .await
            .map_err(super::offline)?;
        cloud.check_session(session)?;
        if response.status().is_success() {
            let answer: Value = response.json().await.unwrap_or(Value::Null);
            cloud.check_session(session)?;
            return Ok(Ok(answer["revision"].as_u64().unwrap_or(expected + 1)));
        }
        let failure = super::failure(response).await;
        cloud.check_session(session)?;
        let failure = self.handle_failure(failure);
        if failure.status == 409 && failure.body["revision"].is_number() {
            return Ok(Err(failure.body["revision"].as_u64().unwrap_or(0)));
        }
        Err(failure)
    }

    /// Uploads `budget` over `expected` and makes it the sync base. False when another device
    /// got there first: the next sync merges with theirs.
    async fn send(&self, ctx: &Ctx<'_>, id: &str, expected: u64, budget: &Budget, local_revision: i64) -> Result<bool, Failure> {
        let (bytes, meta) = Self::seal_budget(&ctx.key, id, budget.clone()).map_err(error)?;
        match self.upload(ctx.cloud, &ctx.session, ctx.generation, id, expected, bytes, &meta).await? {
            Ok(revision) => {
                with_store(ctx, id, |s, _| s.set_sync_base(revision, budget)).map_err(error)?;
                self.track_at(id, revision, local_revision)?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }
    async fn push(&self, ctx: &Ctx<'_>, id: &str, expected: u64) -> Result<&'static str, Failure> {
        let (budget, local_revision, base) = with_store(ctx, id, |s, _| {
            let (budget, status) = s.saved()?;
            Ok((without_heavy(budget), status.revision, s.sync_base()?))
        })
        .map_err(error)?;
        // Changed and changed back: nothing to upload.
        if base.is_some_and(|(revision, base)| revision == expected && base == budget) {
            self.track_at(id, expected, local_revision)?;
            return Ok("synced");
        }
        Ok(if self.send(ctx, id, expected, &budget, local_revision).await? { "uploaded" } else { "waiting" })
    }
    /// Downloads the cloud's version, merges this device's changes into it, saves the result and
    /// uploads it when it has changes the cloud doesn't.
    async fn bring_in(&self, ctx: &Ctx<'_>, id: &str, listed: u64, tracked: Option<Tracked>, unchanged: Option<i64>, choices: &Choices) -> Result<Step, Failure> {
        let (bytes, revision) = self.download(ctx.cloud, &ctx.session, ctx.generation, id, listed).await?;
        let Some(remote) = Self::open_budget(&ctx.key, id, &bytes).map_err(error)? else {
            return Ok("update_app".into());
        };
        let absorbed = with_store(ctx, id, |store, open| {
            absorb(store, open && !ctx.open_clean, tracked, unchanged, &remote, revision, choices)
        })
        .map_err(error)?;
        match absorbed {
            Absorbed::Later => Ok("waiting".into()),
            Absorbed::Conflicts(conflicts) => Ok(Step { conflicts, ..Step::from("conflict") }),
            Absorbed::Unmergeable(e) => Ok(Step { message: Some(t!("{problem} Choose which version to keep.", problem = e)), ..Step::from("conflict") }),
            Absorbed::Saved { merged, local_revision, local_changed, upload, notes } => {
                let result=async {
                    self.track_at(id, revision, if upload { -1 } else { local_revision })?;
                    Ok::<_,Failure>(if !upload {
                        if local_changed { "downloaded" } else { "synced" }
                    } else if !self.send(ctx, id, revision, &merged, local_revision).await? {
                        "waiting"
                    } else if local_changed { "merged" } else { "uploaded" })
                }.await;
                let (state,message)=match result {Ok(state)=>(state,None),Err(f)=>("error",Some(f.message))};
                // A network or ledger error cannot erase the already committed local change.
                Ok(Step { notes, message, open_changed: local_changed && id == ctx.open_id, ..Step::from(state) })
            }
        }
    }

    /// One pass over every budget here and in the cloud.
    pub async fn run(&self, cloud: &Cloud, db: &Database, library: &Library, open_clean: bool) -> Report {
        let stop = |state: &'static str, message: Option<String>| Report {
            at: now(),
            state,
            message,
            budgets: vec![],
            open_changed: false,
            changed_budgets: None,
            library_version: None,
        };
        let _one = match self.operation().await {
            Ok(guard) => guard,
            Err(message) => return self.finish(stop("error", Some(message))),
        };
        let session = match cloud.session() {
            Ok(s) => s,
            Err(f) => return self.finish(stop("signed_out", Some(f.message))),
        };
        let Some(key) = self.key() else {
            return self.finish(stop("locked", None));
        };
        let generation = self.state().generation;
        let remotes = match self.remotes(cloud, &session, generation).await {
            Ok(r) => r,
            Err(f) => {
                let state = match f.status {
                    401 => "signed_out",
                    402 => "subscription_required",
                    0 if f.code == "offline" => "offline",
                    _ if matches!(f.code.as_str(), "sync_key_changed" | "sync_generation_required") => "locked",
                    _ => "error",
                };
                return self.finish(stop(state, Some(f.message)));
            }
        };
        let (locals, open_id): (Vec<Listing>, String) = {
            let library = library.lock();
            (library.list().into_iter().filter(|l| !l.missing && l.account_id.is_none()).collect(), db.store().ok().map(|s| s.id().to_owned()).unwrap_or_default())
        };
        let ctx = Ctx { cloud, db, library, open_id, open_clean, key, session, generation };
        let tracked = self.state().budgets;
        let ids: BTreeSet<String> = locals
            .iter()
            .map(|l| l.id.clone())
            .chain(remotes.iter().map(|r| r.id.clone()))
            .chain(tracked.keys().cloned())
            .collect();

        let mut budgets = vec![];
        let mut open_changed = false;
        let no_choices = Choices::new();
        for id in ids {
            if ensure_legacy_budget(&library.lock(), &id).is_err() { continue; }
            let local = locals.iter().find(|l| l.id == id);
            let remote = remotes.iter().find(|r| r.id == id);
            let tracked = tracked.get(&id).copied();
            let name = local
                .map(|l| l.name.clone())
                .or_else(|| remote.and_then(|r| Self::decode_meta(&ctx.key, &id, &r.meta)))
                .unwrap_or_else(|| t!("Budget"));
            let mut entry = BudgetSync {
                id: id.clone(),
                name,
                state: "synced",
                device: remote.map(|r| r.device.clone()).filter(|d| !d.is_empty()),
                updated_at: remote.map(|r| r.updated_at.clone()),
                message: None,
                conflicts: vec![],
                notes: vec![],
                review: None,
            };
            let outcome: Result<Step, Failure> = match (local, remote, tracked) {
                (None, None, Some(_)) => {
                    let _ = self.set_state(|s| {
                        s.budgets.remove(&id);
                    });
                    continue;
                }
                (None, None, None) => continue,
                (None, Some(_), t) => {
                    if t.is_some() {
                        let _ = self.set_state(|s| {
                            s.budgets.remove(&id);
                        });
                    }
                    Ok("cloud_only".into())
                }
                (Some(_), None, None) => Ok("local_only".into()),
                (Some(_), None, Some(t)) if t.revision == 0 => self.push(&ctx, &id, 0).await.map(Step::from),
                (Some(_), None, Some(_)) => {
                    // Removed from sync on another computer: keep the budget here, stop syncing it.
                    let _ = self.set_state(|s| {
                        s.budgets.remove(&id);
                    });
                    entry.message = Some(t!("Removed from sync on another computer. It's still here."));
                    Ok("local_only".into())
                }
                // Here and in the cloud but not synced from here yet (signed out and in again):
                // merged without a base, so nothing is lost and every difference is a question.
                (Some(_), Some(r), None) => self.bring_in(&ctx, &id, r.revision, None, None, &no_choices).await,
                (Some(_), Some(r), Some(t)) => match with_store(&ctx, &id, |s, _| Ok((s.revision()?, s.sync_base_revision()?))) {
                    Err(e) => Err(error(e)),
                    Ok((local_revision, base_revision)) => match need(t, local_revision, base_revision, r.revision) {
                        Need::Nothing => Ok("synced".into()),
                        Need::Upload(expected) => self.push(&ctx, &id, expected).await.map(Step::from),
                        Need::BringIn { unchanged } => self.bring_in(&ctx, &id, r.revision, Some(t), unchanged, &no_choices).await,
                    },
                },
            };
            match outcome {
                Ok(step) => {
                    entry.state = step.state;
                    entry.message = step.message.or(entry.message);
                    entry.conflicts = step.conflicts;
                    entry.notes = step.notes;
                    open_changed |= step.open_changed;
                }
                Err(f) => {
                    entry.state = "error";
                    entry.message = Some(f.message);
                }
            }
            if entry.state == "update_app" {
                entry.message = Some(t!("A newer UrBudget synced this budget. Update the app on this computer."));
            }
            budgets.push(entry);
        }
        let open_id = &ctx.open_id;
        budgets.sort_by(|a, b| (&a.id != open_id).cmp(&(&b.id != open_id)).then(a.name.cmp(&b.name)));
        self.finish(Report { at: now(), state: "ok", message: None, budgets, open_changed, changed_budgets: None, library_version: None })
    }
}

pub fn sync_status(cloud: &Cloud) -> Status {
    cloud.status().sync
}

pub async fn sync_now(cloud: &Cloud, db: &Database, library: &Library, open_clean: bool) -> Result<Report, String> {
    if super::v2::is_active(cloud,library) || super::v2::discover_active(cloud,library).await.unwrap_or(false) {
        let before = super::notifications::capture(db, library)?;
        let result = super::v2::run(cloud,db,library,open_clean).await;
        return super::notifications::finish(cloud, db, library, before, result);
    }
    Ok(cloud.sync.run(&cloud, &db, &library, open_clean).await)
}

/// Whether the account already has a sync passphrase (another computer set it up).
pub async fn sync_has_key(cloud: &Cloud) -> Result<bool, String> {
    let library=Library(Mutex::new(crate::budgets::Budgets::load(&cloud.data_dir)?));
    if super::v2::is_active(cloud,&library) || super::v2::discover_active(cloud,&library).await.unwrap_or(false) { return Ok(true); }
    match cloud.call(reqwest::Method::GET, "/sync/key", None, true).await {
        Ok(_) => Ok(true),
        Err(f) if f.code == "no_key" => Ok(false),
        Err(f) => Err(f.message),
    }
}

/// Unlocks sync on this computer with the passphrase, or sets the passphrase on first use.
/// Returns whether it was new (then this computer's budgets start syncing).
pub async fn sync_setup(cloud: &Cloud, library: &Library, passphrase: String) -> Result<Value, String> {
    if let Some(result)=super::v2::try_setup(cloud,library,&passphrase,false).await? {return Ok(result);}
    let _one = cloud.sync.operation().await?;
    if passphrase.chars().count() < 8 {
        return Err(t!("Use a passphrase of at least 8 characters."));
    }
    let session = cloud.session()?;
    match cloud.call_session(reqwest::Method::GET, "/sync/key", None, Some(&session), None).await {
        Ok(found) => {
            let salt = URL_SAFE_NO_PAD.decode(found["salt"].as_str().unwrap_or_default()).map_err(|e| e.to_string())?;
            let check = URL_SAFE_NO_PAD.decode(found["check"].as_str().unwrap_or_default()).map_err(|e| e.to_string())?;
            let key = tokio::task::spawn_blocking(move || crypto::derive(&passphrase, &salt))
                .await
                .map_err(|e| e.to_string())??;
            if !crypto::matches_check(&key, &check) {
                return Err(t!("That isn't the sync passphrase for this account."));
            }
            cloud.check_session(&session)?;
            cloud.sync.install_key(key, found["generation"].as_u64().unwrap_or(0))?;
            Ok(json!({ "created": false }))
        }
        Err(f) if f.code == "no_key" => {
            create_key(&cloud, passphrase, false).await?;
            let ids: Vec<String> = library.lock().list().into_iter().filter(|l| !l.missing).map(|l| l.id).collect();
            track(&cloud, &library, &ids)?;
            Ok(json!({ "created": true }))
        }
        Err(f) => Err(f.message),
    }
}

async fn create_key(cloud: &Cloud, passphrase: String, reset: bool) -> Result<(), String> {
    let session = cloud.session()?;
    let expected = if reset {
        let found = cloud.call_session(reqwest::Method::GET, "/sync/key", None, Some(&session), None).await?;
        found["generation"].as_u64().unwrap_or(0)
    } else { 0 };
    let salt: [u8; crypto::SALT_LEN] = crypto::random()?;
    let key = tokio::task::spawn_blocking(move || crypto::derive(&passphrase, &salt))
        .await
        .map_err(|e| e.to_string())??;
    let body = json!({
        "salt": URL_SAFE_NO_PAD.encode(salt),
        "check": URL_SAFE_NO_PAD.encode(crypto::key_check(&key)?),
        "reset": reset,
    });
    let answer = cloud.call_session(reqwest::Method::PUT, "/sync/key", Some(body), Some(&session), Some(expected)).await?;
    cloud.check_session(&session)?;
    cloud.sync.install_key(key, answer["generation"].as_u64().unwrap_or(0))
}

fn track(cloud: &Cloud, library: &Library, ids: &[String]) -> Result<(), String> {
    let catalog = library.lock();
    let allowed: Vec<_> = ids.iter().filter(|id| ensure_legacy_budget(&catalog, id).is_ok()).collect();
    cloud.sync.set_state(|s| {
        for id in allowed {
            s.budgets.entry(id.clone()).or_insert(Tracked { revision: 0, local_revision: -1 });
        }
    })
}

/// A new passphrase: the cloud copies made with the old one are deleted, and this computer's
/// budgets upload again.
pub async fn sync_reset(cloud: &Cloud, library: &Library, passphrase: String) -> Result<(), String> {
    if let Some(_)=super::v2::try_setup(cloud,library,&passphrase,true).await? {return Ok(());}
    let _one = cloud.sync.operation().await?;
    if passphrase.chars().count() < 8 {
        return Err(t!("Use a passphrase of at least 8 characters."));
    }
    create_key(&cloud, passphrase, true).await?;
    cloud.sync.set_state(|s| s.budgets.clear())?;
    let ids: Vec<String> = library.lock().list().into_iter().filter(|l| !l.missing).map(|l| l.id).collect();
    track(&cloud, &library, &ids)
}

/// Starts syncing budgets that are only on this computer.
pub async fn sync_track(cloud: &Cloud, ids: Vec<String>) -> Result<(), String> {
    let library=Library(Mutex::new(crate::budgets::Budgets::load(&cloud.data_dir)?));
    if super::v2::is_active(cloud,&library) {super::v2::enable(cloud,&library,true,&ids)?;return Ok(());}
    let _one = cloud.sync.operation().await?;
    cloud.check_account()?;
    track(&cloud, &library, &ids)
}

/// Brings a budget from the cloud onto this computer, under the same id.
pub async fn sync_download(cloud: &Cloud, library: &Library, id: String) -> Result<String, String> {
    if super::v2::is_active(cloud,library) {super::v2::download(cloud,library,&id).await?;return Ok(id);}
    let _one = cloud.sync.operation().await?;
    ensure_legacy_budget(&library.lock(), &id)?;
    let session = cloud.session()?;
    let generation = cloud.sync.state().generation;
    let key = cloud.sync.key().ok_or_else(|| t!("Enter your sync passphrase first."))?;
    let remote = cloud
        .sync
        .remotes(&cloud, &session, generation)
        .await?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| t!("That budget is no longer in the cloud."))?;
    let (bytes, revision) = cloud.sync.download(&cloud, &session, generation, &id, remote.revision).await?;
    let budget = Engine::open_budget(&key, &id, &bytes)?
        .ok_or_else(|| t!("A newer UrBudget synced this budget. Update the app on this computer first."))?;
    cloud.check_session(&session)?;
    let (path, data_dir) = {
        let mut library = library.lock();
        library.adopt(&id)?;
        (library.database(&id)?, library.data_dir().to_owned())
    };
    let saved = Store::open_budget(&path, &id).and_then(|store| {
        let mut store = store.with_backup_preferences(&data_dir);
        let token = store.status()?.token;
        let acknowledged = store.save(&budget, &token)?;
        store.set_sync_base(revision, &budget)?;
        Ok(acknowledged.revision)
    });
    match saved {
        Ok(local_revision) => {
            cloud.sync.set_state(|s| {
                s.budgets.insert(id.clone(), Tracked { revision, local_revision });
            })?;
            Ok(id)
        }
        Err(e) => {
            let _ = library.lock().forget_new(&id);
            Err(e)
        }
    }
}

/// Settles a conflict. "merge" merges again with the person's `choices` (a conflict's key →
/// "local" or "cloud"). When the versions couldn't be combined, "local" uploads this computer's
/// version over the cloud's and "cloud" replaces this computer's with the cloud's (after a backup).
pub async fn sync_resolve(
    cloud: &Cloud,
    db: &Database,
    library: &Library,
    id: String,
    keep: String,
    choices: Option<Choices>,
    open_clean: bool,
    review: Option<Value>,
) -> Result<Report, String> {
    if super::v2::is_active(cloud,library){
        let before = super::notifications::capture(db, library)?;
        let result = super::v2::resolve(cloud,db,library,&id,&keep,choices.unwrap_or_default(),open_clean,review).await;
        return super::notifications::finish(cloud, db, library, before, result);
    }
    let one = cloud.sync.operation().await?;
    ensure_legacy_budget(&library.lock(), &id)?;
    let session = cloud.session()?;
    let generation = cloud.sync.state().generation;
    let key = cloud.sync.key().ok_or_else(|| t!("Enter your sync passphrase first."))?;
    let remote = cloud.sync.remotes(&cloud, &session, generation).await?.into_iter().find(|r| r.id == id);
    let open_id = db.store().ok().map(|s| s.id().to_owned()).unwrap_or_default();
    if !open_clean && id == open_id && keep != "local" {
        return Err(t!("Save your changes first."));
    }
    let ctx = Ctx { cloud: &cloud, db: &db, library: &library, open_id, open_clean, key, session, generation };
    let engine = &cloud.sync;
    match (keep.as_str(), remote) {
        ("merge", Some(remote)) => {
            let tracked = engine.state().budgets.get(&id).copied();
            let step = engine
                .bring_in(&ctx, &id, remote.revision, tracked, None, &choices.unwrap_or_default())
                .await?;
            match step.state {
                "waiting" => return Err(t!("Save your changes first, then try again.")),
                "update_app" => return Err(t!("A newer UrBudget synced this budget. Update the app on this computer first.")),
                "conflict" if step.conflicts.is_empty() => {
                    return Err(step.message.unwrap_or_else(|| t!("The two versions couldn't be combined.")))
                }
                // New conflicts (the cloud copy changed again) show in the report.
                "conflict" => {}
                // The pass below finds it synced; the report says what the merge did.
                _ => {
                    drop(one);
                    let mut report = engine.run(&cloud, &db, &library, open_clean).await;
                    report.open_changed |= step.open_changed;
                    if let Some(entry) = report.budgets.iter_mut().find(|b| b.id == id && b.state == "synced") {
                        entry.state = step.state;
                        entry.notes = step.notes;
                    }
                    return Ok(engine.finish(report));
                }
            }
        }
        ("local", remote) => {
            let expected = remote.map_or(0, |r| r.revision);
            if !matches!(engine.push(&ctx, &id, expected).await?, "uploaded" | "synced") {
                return Err(t!("The cloud copy changed again. Try once more."));
            }
        }
        ("cloud", Some(remote)) => {
            let expected_local_revision = with_store(&ctx, &id, |store, _| store.revision())?;
            let (bytes, revision) = engine.download(&cloud, &ctx.session, generation, &id, remote.revision).await?;
            let budget = Engine::open_budget(&ctx.key, &id, &bytes)?
                .ok_or_else(|| t!("A newer UrBudget synced this budget. Update the app on this computer first."))?;
            let local_revision = with_store(&ctx, &id, |store, _| {
                replace_with_cloud(store, expected_local_revision, &budget, revision)
            })?;
            engine.track_at(&id, revision, local_revision)?;
        }
        _ => return Err(t!("That budget is no longer in the cloud.")),
    }
    drop(one);
    Ok(engine.run(&cloud, &db, &library, open_clean).await)
}

/// Stops syncing a budget and deletes its cloud copy. Copies on computers stay.
pub async fn sync_remove(cloud: &Cloud, id: String) -> Result<(), String> {
    let library=Library(Mutex::new(crate::budgets::Budgets::load(&cloud.data_dir)?));
    if super::v2::is_active(cloud,&library){crate::library_api::lifecycle(cloud,&Database::new(None),&library,"trash",&id).await?;return Ok(());}
    let _one = cloud.sync.operation().await?;
    ensure_legacy_budget(&library.lock(), &id)?;
    let session = cloud.session()?;
    cloud.call_session(reqwest::Method::DELETE, &format!("/sync/budgets/{id}"), None, Some(&session), Some(cloud.sync.state().generation)).await?;
    cloud.sync.set_state(|s| {
        s.budgets.remove(&id);
    })
}

/// Forgets the sync key on this computer; the passphrase unlocks it again.
pub async fn sync_lock(cloud: &Cloud) -> Result<(), String> {
    super::v2::forget_keys(cloud,cloud.selected_account().as_deref())?;
    let _one = cloud.sync.operation().await?;
    cloud.check_account()?;
    cloud.sync.set_key(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use urbudget_core::test_support::{purchase, sample};
    use crate::domain::Transaction;
    use crate::patch::Patch;

    #[test]
    fn v1_uses_closed_stores_with_no_selection_and_never_tracks_account_budgets() {
        let dir=tempfile::tempdir().unwrap();
        let mut catalog=crate::budgets::Budgets::load(dir.path()).unwrap();
        let local=catalog.create().unwrap();
        Store::open_budget(&catalog.database(&local).unwrap(),&local).unwrap();
        let owned=catalog.create_owned(Some("account-a")).unwrap();
        Store::open_budget(&catalog.database(&owned).unwrap(),&owned).unwrap();
        catalog.clear_current().unwrap();
        let library=Library(Mutex::new(catalog)); let db=Database::new(None); let cloud=Cloud::new(dir.path());
        let ctx=Ctx{cloud:&cloud,db:&db,library:&library,open_id:String::new(),open_clean:true,key:[0;32],session:Session{token:String::new(),epoch:0},generation:0};
        assert!(with_store(&ctx,&local,|s,open| { assert!(!open); Ok(s.id()==local) }).unwrap());
        assert!(with_store(&ctx,&owned,|_,_| Ok(())).unwrap_err().contains("legacy sync"));
        track(&cloud,&library,&[local.clone(),owned.clone()]).unwrap();
        assert!(cloud.sync.state().budgets.contains_key(&local));
        assert!(!cloud.sync.state().budgets.contains_key(&owned));
    }

    #[test]
    fn separate_app_instances_serialize_operations_and_reload_shared_tracking() {
        let dir = tempfile::tempdir().unwrap();
        let first = Engine::load(dir.path());
        let second = Engine::load(dir.path());
        let guard = crate::block_on(first.operation()).unwrap();
        first.set_state(|s| {
            s.generation = 7;
            s.budgets.insert("first".into(), Tracked { revision: 2, local_revision: 3 });
        }).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _guard = crate::block_on(second.operation()).unwrap();
            let state = second.state();
            assert_eq!(state.generation, 7);
            assert!(state.budgets.contains_key("first"));
            second.set_state(|s| { s.budgets.insert("second".into(), Tracked { revision: 4, local_revision: 5 }); }).unwrap();
            acquired_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert!(matches!(acquired_rx.recv_timeout(std::time::Duration::from_millis(100)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)), "a separate Engine must wait for the shared operation lock");
        drop(guard);
        acquired_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        worker.join().unwrap();
        let _guard = crate::block_on(first.operation()).unwrap();
        assert_eq!(first.state().budgets.len(), 2, "a later operation must preserve another process's new tracking");
    }

    #[test]
    fn legacy_sync_tracking_starts_at_generation_zero_and_persists_new_generation() {
        let old: Ledger = serde_json::from_value(json!({"budgets":{}})).unwrap();
        assert_eq!(old.generation, 0);
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::load(dir.path());
        engine.set_state(|s| s.generation = 7).unwrap();
        assert_eq!(Engine::load(dir.path()).state().generation, 7);
    }

    #[test]
    fn closed_budget_sync_backups_obey_the_persisted_device_retention() {
        let dir = tempfile::tempdir().unwrap();
        let mut budgets = crate::budgets::Budgets::load(dir.path()).unwrap();
        let open_id = budgets.create().unwrap();
        budgets.set_current(&open_id).unwrap();
        let closed_id = budgets.create().unwrap();
        Store::open_budget(&budgets.database(&closed_id).unwrap(), &closed_id).unwrap();
        let store = Store::open_budget(&budgets.database(&open_id).unwrap(), &open_id).unwrap()
            .with_backup_preferences(dir.path());
        store.set_backup_retention(5).unwrap();
        let db = Database::new(Some(store));
        let library = Library(Mutex::new(budgets));
        let cloud = Cloud::new(dir.path());
        let ctx = Ctx { cloud: &cloud, db: &db, library: &library, open_id, open_clean: true, key: [0; 32], session: Session { token: String::new(), epoch: 0 }, generation: 0 };
        for _ in 0..8 {
            with_store(&ctx, &closed_id, |store, is_open| {
                assert!(!is_open);
                assert_eq!(store.backup_retention()?, 5);
                store.backup().map(|_| ())
            }).unwrap();
        }
        let folder = library.lock().database(&closed_id).unwrap().parent().unwrap().join("backups");
        assert_eq!(std::fs::read_dir(folder).unwrap().count(), 5);
    }

    #[test]
    fn switching_budgets_rejects_operations_from_the_old_sync_context() {
        let dir = tempfile::tempdir().unwrap();
        let mut budgets = crate::budgets::Budgets::load(dir.path()).unwrap();
        let original_id = budgets.create().unwrap();
        budgets.set_current(&original_id).unwrap();
        let other_id = budgets.create().unwrap();
        let store = Store::open_budget(&budgets.database(&original_id).unwrap(), &original_id).unwrap();
        let db = Database::new(Some(store));
        let library = Library(Mutex::new(budgets));
        let cloud = Cloud::new(dir.path());
        let ctx = Ctx {
            cloud: &cloud, db: &db, library: &library, open_id: original_id.clone(), open_clean: true, key: [0; 32],
            session: Session { token: String::new(), epoch: 0 }, generation: 0,
        };
        {
            let mut budgets = library.lock();
            *db.store().unwrap() = Store::open_budget(&budgets.database(&other_id).unwrap(), &other_id).unwrap();
            budgets.set_current(&other_id).unwrap();
        }
        for id in [&original_id, &other_id] {
            let mut called = false;
            let result = with_store(&ctx, id, |_, _| {
                called = true;
                Ok(())
            });
            assert!(result.unwrap_err().contains("open budget changed"));
            assert!(!called);
        }
        cloud.account_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(with_store(&ctx, &other_id, |_, _| Ok(())).unwrap_err().contains("account changed"));
        assert_eq!(db.store().unwrap().id(), other_id);
    }

    /// The cloud: the newest version and its revision.
    #[derive(Default)]
    struct Server {
        revision: u64,
        budget: Option<Budget>,
        /// Another device uploads this just before the next upload lands.
        race: Option<Budget>,
    }
    impl Server {
        fn put(&mut self, expected: u64, budget: &Budget) -> Result<u64, u64> {
            if let Some(first) = self.race.take() {
                self.revision += 1;
                self.budget = Some(first);
            }
            if expected != self.revision {
                return Err(self.revision);
            }
            self.revision += 1;
            self.budget = Some(budget.clone());
            Ok(self.revision)
        }
    }

    /// One device: a budget database and what it remembers of the last sync.
    struct Device {
        _dir: tempfile::TempDir,
        store: Store,
        tracked: Option<Tracked>,
    }
    impl Device {
        fn new(budget: &Budget) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let mut store = Store::open_budget(&dir.path().join("budget.db"), "b").unwrap();
            let token = store.status().unwrap().token;
            store.save(budget, &token).unwrap();
            Device { _dir: dir, store, tracked: None }
        }
        /// As `sync_download` does it.
        fn download(server: &Server) -> Self {
            let mut device = Device::new(server.budget.as_ref().unwrap());
            device.store.set_sync_base(server.revision, server.budget.as_ref().unwrap()).unwrap();
            device.tracked = Some(Tracked { revision: server.revision, local_revision: device.store.revision().unwrap() });
            device
        }
        fn budget(&mut self) -> Budget {
            without_heavy(self.store.saved().unwrap().0)
        }
        /// Like the window: the edit goes to the draft as a patch, then it's saved.
        fn edit(&mut self, f: impl FnOnce(&mut Budget)) {
            let snapshot = self.store.load_light().unwrap();
            let mut after = snapshot.budget.clone();
            f(&mut after);
            self.store.apply_patch(Patch::diff(&snapshot.budget, &after)).unwrap();
            self.store.commit(&snapshot.token).unwrap();
        }
        fn upload(&mut self, server: &mut Server, expected: u64, budget: &Budget, local_revision: i64) -> bool {
            match server.put(expected, budget) {
                Ok(revision) => {
                    self.store.set_sync_base(revision, budget).unwrap();
                    self.tracked = Some(Tracked { revision, local_revision });
                    true
                }
                Err(_) => false,
            }
        }
        /// One sync of a budget that's here and in the cloud, as `Engine::run` does it.
        fn sync(&mut self, server: &mut Server, choices: &Choices) -> (&'static str, Vec<Conflict>) {
            let local_revision = self.store.revision().unwrap();
            let need = match self.tracked {
                None => Need::BringIn { unchanged: None },
                Some(t) if t.revision == 0 => Need::Upload(0),
                Some(t) => need(t, local_revision, self.store.sync_base_revision().unwrap(), server.revision),
            };
            match need {
                Need::Nothing => ("synced", vec![]),
                Need::Upload(expected) => {
                    let budget = self.budget();
                    let state = if self.upload(server, expected, &budget, local_revision) { "uploaded" } else { "waiting" };
                    (state, vec![])
                }
                Need::BringIn { unchanged } => {
                    let (remote, revision) = (server.budget.clone().unwrap(), server.revision);
                    match absorb(&mut self.store, false, self.tracked, unchanged, &remote, revision, choices).unwrap() {
                        Absorbed::Saved { merged, local_revision, local_changed, upload, .. } => {
                            self.tracked = Some(Tracked { revision, local_revision: if upload { -1 } else { local_revision } });
                            let state = if !upload {
                                if local_changed {
                                    "downloaded"
                                } else {
                                    "synced"
                                }
                            } else if self.upload(server, revision, &merged, local_revision) {
                                "merged"
                            } else {
                                "waiting"
                            };
                            (state, vec![])
                        }
                        Absorbed::Conflicts(conflicts) => ("conflict", conflicts),
                        other => panic!("{other:?}"),
                    }
                }
            }
        }
    }
    // The pre-merge desktop protocol: no stored merge base, whole-budget uploads/downloads,
    // and a choice when both versions changed. Keep this independent of need()/absorb().
    fn sync_like_desktop_1_0_4(device: &mut Device, server: &mut Server) -> &'static str {
        assert!(device.store.sync_base().unwrap().is_none());
        let tracked = device.tracked.unwrap();
        let (local, status) = device.store.saved().unwrap();
        let local = without_heavy(local);
        let local_changed = status.revision != tracked.local_revision;
        let remote_changed = server.revision != tracked.revision;
        match (local_changed, remote_changed) {
            (false, false) => "synced",
            (true, false) => {
                // The legacy sender writes the same format-1 envelope, independently of
                // the new serializer, and the new receiver must still open it.
                let envelope = json!({ "format": 1, "app": "1.0.4", "budget": local });
                let sealed = crypto::seal(&[9; 32], b"urbudget:budget:b",
                    &crypto::compress(&serde_json::to_vec(&envelope).unwrap()).unwrap()).unwrap();
                let incoming = Engine::open_budget(&[9; 32], "b", &sealed).unwrap().unwrap();
                let revision = server.put(tracked.revision, &incoming).unwrap();
                device.tracked = Some(Tracked { revision, local_revision: status.revision });
                "uploaded"
            }
            (_, true) => {
                // The new sender's encrypted envelope is read with the desktop 1.0.4
                // version limits and shape. No merge metadata is required on the wire.
                let (sealed, _) = Engine::seal_budget(&[9; 32], "b", server.budget.clone().unwrap()).unwrap();
                assert_eq!(&sealed[..4], b"UBS1");
                let plain = crypto::open(&[9; 32], b"urbudget:budget:b", &sealed).unwrap();
                let envelope: Value = serde_json::from_slice(&crypto::decompress(&plain).unwrap()).unwrap();
                assert_eq!(envelope["format"], 1);
                assert!(envelope["budget"]["schemaVersion"].as_u64().unwrap() <= 2);
                let remote: Budget = serde_json::from_value(envelope["budget"].clone()).unwrap();
                crate::domain::validate(&remote).unwrap();
                if local_changed && local != remote {
                    return "conflict";
                }
                let revision = if local_changed { status.revision }
                    else { device.store.save(&remote, &status.token).unwrap().revision };
                device.tracked = Some(Tracked { revision: server.revision, local_revision: revision });
                if local_changed { "synced" } else { "downloaded" }
            }
        }
    }

    #[test]
    fn desktop_1_0_4_and_three_way_clients_exchange_edits_without_a_protocol_upgrade() {
        let mut start = sample();
        start.transactions.push(transaction("original", -1000));
        start.extra.insert("freshStartHistory".into(), json!({
            "bankImportIds": ["already-capitalized"],
            "loanInterestMonths": { "loan": ["2026-10"] }
        }));
        let mut server = Server::default();
        let mut desktop = Device::new(&start);
        desktop.tracked = Some(Tracked { revision: 0, local_revision: -1 });
        assert_eq!(sync_like_desktop_1_0_4(&mut desktop, &mut server), "uploaded");
        let mut phone = Device::download(&server);

        desktop.edit(|b| {
            b.transactions.push(transaction("desktop-offline", -200));
            b.assignments[0].amount += 3000;
        });
        phone.edit(|b| {
            b.transactions.push(transaction("phone-offline", -300));
            let mut november = b.assignments[0].clone();
            november.month = "2026-11".into();
            november.amount = 5000;
            b.assignments.push(november);
        });
        assert_eq!(sync_like_desktop_1_0_4(&mut desktop, &mut server), "uploaded");
        assert_eq!(phone.sync(&mut server, &none()).0, "merged");
        assert_eq!(sync_like_desktop_1_0_4(&mut desktop, &mut server), "downloaded");
        assert_eq!(desktop.budget(), phone.budget());
        assert_eq!(food_october(&mut desktop), 23_000);
        assert_eq!(food_november(&mut desktop), 5000);
        assert_eq!(desktop.budget().extra["freshStartHistory"], start.extra["freshStartHistory"]);

        // Each client edits data received from the other and sends it back.
        desktop.edit(|b| b.transactions.iter_mut().find(|t| t.id == "phone-offline").unwrap().memo = "Desktop note".into());
        assert_eq!(sync_like_desktop_1_0_4(&mut desktop, &mut server), "uploaded");
        assert_eq!(phone.sync(&mut server, &none()).0, "downloaded");
        phone.edit(|b| b.transactions.iter_mut().find(|t| t.id == "desktop-offline").unwrap().memo = "Phone note".into());
        assert_eq!(phone.sync(&mut server, &none()).0, "uploaded");
        assert_eq!(sync_like_desktop_1_0_4(&mut desktop, &mut server), "downloaded");
        assert_eq!(desktop.budget(), phone.budget());
        assert!(desktop.store.sync_base().unwrap().is_none());
        assert!(phone.store.sync_base().unwrap().is_some());
        let db = rusqlite::Connection::open(phone._dir.path().join("budget.db")).unwrap();
        assert_eq!(db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).unwrap(), urbudget_core::sync_storage::STORAGE_SCHEMA_VERSION);
    }

    #[test]
    fn a_legacy_client_still_requires_a_choice_when_both_versions_changed() {
        let (mut server, mut desktop, mut phone) = two_devices();
        desktop.store.forget_sync_base().unwrap();
        desktop.edit(|b| b.transactions.push(transaction("desktop-unsynced", -200)));
        phone.edit(|b| b.transactions.push(transaction("phone-uploaded", -300)));
        assert_eq!(phone.sync(&mut server, &none()).0, "uploaded");
        let local = desktop.budget();
        let remote = server.budget.clone();
        let revision = server.revision;
        assert_eq!(sync_like_desktop_1_0_4(&mut desktop, &mut server), "conflict");
        assert_eq!(desktop.budget(), local);
        assert_eq!(server.budget, remote);
        assert_eq!(server.revision, revision);
        assert!(desktop.store.sync_base().unwrap().is_none());
    }

    fn transaction(id: &str, amount: i64) -> Transaction {
        let mut t = purchase();
        t.id = id.into();
        t.amount = amount;
        t.splits[0].amount = amount;
        t
    }
    /// A computer that synced a budget, and a phone that downloaded it.
    fn two_devices() -> (Server, Device, Device) {
        let mut start = sample();
        start.transactions.push(transaction("t1", -1000));
        let mut server = Server::default();
        let mut computer = Device::new(&start);
        computer.tracked = Some(Tracked { revision: 0, local_revision: -1 });
        assert_eq!(computer.sync(&mut server, &Choices::new()).0, "uploaded");
        let phone = Device::download(&server);
        (server, computer, phone)
    }
    fn none() -> Choices {
        Choices::new()
    }
    fn answer(key: &str, side: &str) -> Choices {
        [(key.to_string(), side.to_string())].into()
    }
    fn food_october(d: &mut Device) -> i64 {
        d.budget()
            .assignments
            .iter()
            .find(|a| a.category_id == "food" && a.month == "2026-10")
            .unwrap()
            .amount
    }
    fn food_november(d: &mut Device) -> i64 {
        d.budget().assignments.iter()
            .find(|a| a.category_id == "food" && a.month == "2026-11")
            .unwrap().amount
    }

    #[test]
    fn a_save_during_download_waits_and_the_next_sync_keeps_both_edits() {
        for retain_base in [true, false] {
            let (mut server, mut computer, mut phone) = two_devices();
            if !retain_base {
                computer.store.forget_sync_base().unwrap();
            }
            phone.edit(|budget| budget.transactions.push(transaction("from-phone", -300)));
            assert_eq!(phone.sync(&mut server, &none()).0, "uploaded");
            let before_download = computer.store.revision().unwrap();
            computer.edit(|budget| budget.transactions.push(transaction("during-download", -700)));
            let local = computer.budget();
            let result = absorb(
                &mut computer.store, false, computer.tracked, Some(before_download),
                server.budget.as_ref().unwrap(), server.revision, &none(),
            ).unwrap();
            assert!(matches!(result, Absorbed::Later));
            assert_eq!(computer.budget(), local);
            assert_eq!(computer.sync(&mut server, &none()).0, "merged");
            let merged = computer.budget();
            assert!(merged.transactions.iter().any(|t| t.id == "from-phone"));
            assert!(merged.transactions.iter().any(|t| t.id == "during-download"));
            assert_eq!(server.budget.as_ref().unwrap(), &merged);
        }
    }

    #[test]
    fn an_explicit_cloud_choice_cannot_replace_a_newer_save_or_unsaved_draft() {
        let (server, mut computer, _) = two_devices();
        let mut remote = server.budget.unwrap();
        remote.name = "Cloud choice".into();
        let before_download = computer.store.revision().unwrap();
        computer.edit(|budget| budget.name = "Saved during download".into());
        let local = computer.budget();
        let after_edit = computer.store.revision().unwrap();
        assert!(replace_with_cloud(&mut computer.store, before_download, &remote, 2)
            .unwrap_err().contains("local budget changed"));
        assert_eq!(computer.budget(), local);

        let snapshot = computer.store.load_light().unwrap();
        let mut draft = snapshot.budget.clone();
        draft.name = "Unsaved while downloading".into();
        computer.store.apply_patch(Patch::diff(&snapshot.budget, &draft)).unwrap();
        assert!(replace_with_cloud(&mut computer.store, after_edit, &remote, 2)
            .unwrap_err().contains("Save your changes"));
        assert_eq!(computer.store.full_draft(None).unwrap(), draft);
        computer.store.commit(&snapshot.token).unwrap();

        let expected = computer.store.revision().unwrap();
        let acknowledged = replace_with_cloud(&mut computer.store, expected, &remote, 2).unwrap();
        assert_eq!(computer.budget(), remote);
        assert_eq!(acknowledged, computer.store.revision().unwrap());
        assert_eq!(computer.store.sync_base().unwrap(), Some((2, remote)));
    }

    #[test]
    fn two_devices_editing_apart_sync_without_a_choice() {
        let (mut server, mut computer, mut phone) = two_devices();
        phone.edit(|b| {
            b.transactions.push(transaction("from-phone", -300));
            let mut november = b.assignments[0].clone();
            november.month = "2026-11".into();
            november.amount = 5000;
            b.assignments.push(november);
        });
        computer.edit(|b| {
            b.transactions.push(transaction("from-computer", -700));
            b.transactions[0].memo = "Lunch".into();
            b.categories[1].name = "Visa payment".into();
            b.assignments[0].amount += 10_000;
        });
        assert_eq!(phone.sync(&mut server, &none()).0, "uploaded");
        assert_eq!(computer.sync(&mut server, &none()).0, "merged");
        assert_eq!(phone.sync(&mut server, &none()).0, "downloaded");
        let merged = computer.budget();
        assert_eq!(phone.budget(), merged);
        let mut ids: Vec<&str> = merged.transactions.iter().map(|t| t.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["from-computer", "from-phone", "t1"]);
        assert_eq!((merged.transactions[0].memo.as_str(), merged.categories[1].name.as_str()), ("Lunch", "Visa payment"));
        assert_eq!(food_october(&mut phone), 30_000);
        assert_eq!(food_november(&mut phone), 5000);
        assert_eq!(computer.sync(&mut server, &none()).0, "synced");
        assert_eq!(phone.sync(&mut server, &none()).0, "synced");
    }

    #[test]
    fn a_true_conflict_is_shown_and_the_answer_syncs() {
        let (mut server, mut computer, mut phone) = two_devices();
        phone.edit(|b| b.categories[0].name = "Groceries".into());
        computer.edit(|b| {
            b.categories[0].name = "Food & drink".into();
            b.transactions[0].memo = "Kept either way".into();
        });
        assert_eq!(phone.sync(&mut server, &none()).0, "uploaded");
        let (state, conflicts) = computer.sync(&mut server, &none());
        assert_eq!(state, "conflict");
        assert_eq!(conflicts.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["category/food/name"]);
        assert_eq!((conflicts[0].local.clone(), conflicts[0].remote.clone()), (json!("Food & drink"), json!("Groceries")));
        // Nothing is saved or uploaded while the question is open.
        assert_eq!(computer.budget().categories[0].name, "Food & drink");
        assert_eq!(server.revision, 2);
        assert_eq!(computer.sync(&mut server, &answer("category/food/name", "local")).0, "merged");
        assert_eq!(phone.sync(&mut server, &none()).0, "downloaded");
        let phone_budget = phone.budget();
        assert_eq!(phone_budget.categories[0].name, "Food & drink");
        assert_eq!(phone_budget.transactions[0].memo, "Kept either way");
        assert_eq!(phone_budget, computer.budget());
    }

    #[test]
    fn editing_while_the_other_deletes_shows_the_record() {
        let (mut server, mut computer, mut phone) = two_devices();
        phone.edit(|b| b.transactions.clear());
        computer.edit(|b| b.transactions[0].memo = "Receipt kept".into());
        assert_eq!(phone.sync(&mut server, &none()).0, "uploaded");
        let (state, conflicts) = computer.sync(&mut server, &none());
        assert_eq!(state, "conflict");
        assert_eq!(conflicts[0].key, "transaction/t1");
        assert_eq!((conflicts[0].local.clone(), conflicts[0].remote.clone()), (json!({ "changed": ["memo"] }), Value::Null));
        assert_eq!(computer.sync(&mut server, &answer("transaction/t1", "cloud")).0, "downloaded");
        assert!(computer.budget().transactions.is_empty());
    }

    #[test]
    fn a_merge_whose_upload_loses_the_race_is_not_applied_twice() {
        let (mut server, mut computer, mut phone) = two_devices();
        phone.edit(|b| {
            let mut november = b.assignments[0].clone();
            november.month = "2026-11".into();
            november.amount = 5000;
            b.assignments.push(november);
        });
        computer.edit(|b| b.assignments[0].amount += 10_000);
        assert_eq!(phone.sync(&mut server, &none()).0, "uploaded");
        // The phone adds a transaction and uploads it while the computer is merging.
        phone.edit(|b| b.transactions.push(transaction("meanwhile", -50)));
        server.race = Some(phone.budget());
        assert_eq!(computer.sync(&mut server, &none()).0, "waiting");
        assert_eq!(food_october(&mut computer), 30_000);
        assert_eq!(food_november(&mut computer), 5000);
        // The next sync preserves the already-merged monthly plans exactly once.
        assert_eq!(computer.sync(&mut server, &none()).0, "merged");
        assert_eq!(food_october(&mut computer), 30_000);
        assert_eq!(food_november(&mut computer), 5000);
        assert!(computer.budget().transactions.iter().any(|t| t.id == "meanwhile"));
    }

    #[test]
    fn a_device_that_synced_before_bases_were_kept_still_downloads() {
        let (mut server, mut computer, mut phone) = two_devices();
        phone.store.forget_sync_base().unwrap();
        computer.edit(|b| b.name = "Household".into());
        assert_eq!(computer.sync(&mut server, &none()).0, "uploaded");
        assert_eq!(phone.sync(&mut server, &none()).0, "downloaded");
        assert_eq!(phone.budget().name, "Household");
    }

    #[test]
    fn signing_in_again_merges_without_losing_anything() {
        let (mut server, mut computer, _) = two_devices();
        computer.tracked = None;
        computer.edit(|b| b.transactions.push(transaction("offline", -10)));
        let mut cloud = server.budget.clone().unwrap();
        cloud.transactions.push(transaction("elsewhere", -20));
        server.budget = Some(cloud);
        server.revision += 1;
        assert_eq!(computer.sync(&mut server, &none()).0, "merged");
        let ids: Vec<String> = computer.budget().transactions.iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids, ["t1", "elsewhere", "offline"]);
        assert_eq!(server.budget.as_ref().unwrap(), &computer.budget());
    }

    #[test]
    fn a_cloud_copy_uploaded_again_from_scratch_merges_without_the_old_base() {
        let (mut server, mut computer, mut phone) = two_devices();
        computer.edit(|b| b.name = "Household".into());
        assert_eq!(computer.sync(&mut server, &none()).0, "uploaded");
        // The phone, still on the first version, resets the passphrase and uploads again.
        let mut reset = Server::default();
        reset.put(0, &phone.budget()).unwrap();
        let (state, conflicts) = computer.sync(&mut reset, &none());
        // Not quietly back to the old name: it's a question.
        assert_eq!(state, "conflict");
        assert_eq!(conflicts.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["budget/name"]);
        assert_eq!(computer.budget().name, "Household");
    }

    #[test]
    fn what_a_budget_needs_follows_the_base_saved_with_it() {
        let t = Tracked { revision: 4, local_revision: 10 };
        assert_eq!(need(t, 10, Some(4), 4), Need::Nothing);
        assert_eq!(need(t, 11, Some(4), 4), Need::Upload(4));
        assert_eq!(need(t, 10, Some(4), 5), Need::BringIn { unchanged: Some(10) });
        assert_eq!(need(t, 11, None, 5), Need::BringIn { unchanged: None });
        // A merge saved revision 5's base before the ledger caught up.
        assert_eq!(need(Tracked { revision: 4, local_revision: -1 }, 12, Some(5), 5), Need::Upload(5));
        // A base from before sync was reset isn't trusted.
        assert_eq!(need(t, 11, Some(2), 4), Need::Upload(4));
        assert!(!trusted(Some(Tracked { revision: 0, local_revision: -1 }), 7));
    }
}
    #[test]
    fn key_installation_fails_closed_when_clearing_or_saving_credentials_fails() {
        for fail_clear in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let engine = Engine::load(dir.path());
            let persisted = std::cell::Cell::new(Some([1; 32]));
            let result = engine.install_key_with([2; 32], 7, |key| {
                if key.is_none() && !fail_clear { persisted.set(None); return Ok(()); }
                Err("storage unavailable".into())
            });
            assert!(result.is_err());
            if fail_clear {
                assert_eq!(engine.state().generation, 0);
                assert_eq!(persisted.get(), Some([1; 32]));
            } else {
                assert_eq!(engine.state().generation, 7);
                assert_eq!(persisted.get(), None);
            }
        }
    }
