//! Account-wide automatic snapshot sync. A durable installation delivery slot owns every
//! request until a terminal server receipt has been acknowledged locally.
use super::{crypto, Cloud, Session};
use crate::{budgets, domain::Budget, library_api, store::Store, Database, Library};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64, Engine as _};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use urbudget_core::sync_storage::{Binding, RemoteIdentity, UploadIntent};

type Result<T> = std::result::Result<T, String>;

/// Recursive key sorting matches the Worker protocol fixture. Integer counters are strings.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<_, _> = map.iter().collect();
            format!(
                "{{{}}}",
                sorted
                    .into_iter()
                    .map(|(k, v)| format!("{}:{}", json!(k), canonical(v)))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        _ => value.to_string(),
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn string(value: &Value, field: &str) -> Result<String> {
    value[field]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("Sync response is missing {field}."))
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub format: String,
    pub sha256: Option<String>,
    pub length: u64,
    pub meta: String,
    pub context: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    pub budget_id: String,
    pub incarnation: String,
    pub version: String,
    pub key_generation: String,
    pub state: String,
    pub payload: Option<Payload>,
    pub updated_sequence: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub retained_until: Option<String>,
    pub device: String,
}

struct Transport<'a> {
    cloud: &'a Cloud,
    session: Session,
    installation: String,
}
impl<'a> Transport<'a> {
    fn new(cloud: &'a Cloud, installation: String) -> Result<Self> {
        Ok(Self {
            cloud,
            session: cloud.session().map_err(String::from)?,
            installation,
        })
    }
    fn request(&self, method: Method, path: &str) -> Result<reqwest::RequestBuilder> {
        self.cloud
            .for_session(
                &self.session,
                self.cloud
                    .client
                    .request(method, format!("{}/api/v2/sync{path}", super::site())),
            )
            .map(|r| r.header("X-Sync-Installation", &self.installation))
            .map_err(String::from)
    }
    async fn response(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let response = request
            .send()
            .await
            .map_err(|_| "OFFLINE: Sync will retry when a connection is available.".to_owned())?;
        self.cloud
            .check_session(&self.session)
            .map_err(String::from)?;
        if response.status().is_success() {
            return Ok(response);
        }
        let not_found = response.status() == reqwest::StatusCode::NOT_FOUND;
        let mut error = super::failure(response).await;
        if not_found && error.code == "error" { error.code = "not_found".into(); }
        if error.status == 401 {
            self.cloud.forget_session(&self.session);
        }
        Err(format!("{}: {}", error.code, error.message))
    }
    async fn json(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self.request(method, path)?;
        if let Some(body) = body {
            request = request.json(&body);
        }
        let value = self
            .response(request)
            .await?
            .json()
            .await
            .map_err(|e| format!("Unreadable sync response: {e}"))?;
        self.cloud
            .check_session(&self.session)
            .map_err(String::from)?;
        Ok(value)
    }
    fn scoped(
        &self,
        method: Method,
        path: &str,
        bootstrap: &Value,
    ) -> Result<reqwest::RequestBuilder> {
        Ok(self
            .request(method, path)?
            .header("X-Sync-Generation", string(bootstrap, "keyGeneration")?)
            .header("X-Sync-Epoch", string(bootstrap, "catalogEpoch")?))
    }
    async fn catalog(&self, bootstrap: &Value) -> Result<Value> {
        self.response(self.scoped(Method::GET, "/catalog/snapshot", bootstrap)?)
            .await?
            .json()
            .await
            .map_err(|e| e.to_string())
    }
    async fn download(&self, bootstrap: &Value, head: &Head) -> Result<Vec<u8>> {
        let request = self
            .scoped(
                Method::GET,
                &format!("/budgets/{}/content", head.budget_id),
                bootstrap,
            )?
            .query(&[
                ("incarnation", &head.incarnation),
                ("version", &head.version),
            ]);
        let response = self.response(request).await?;
        let described = response
            .headers()
            .get("X-Sync-Payload")
            .and_then(|v| v.to_str().ok())
            .ok_or("The server did not identify its encrypted payload.")?;
        let descriptor: Payload = serde_json::from_str(described).map_err(|e| e.to_string())?;
        if head.payload.as_ref() != Some(&descriptor) {
            return Err("The encrypted content changed during download; retry sync.".into());
        }
        if response
            .content_length()
            .is_some_and(|n| n > 50 * 1024 * 1024)
        {
            return Err("The sync payload exceeds the download limit.".into());
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?.to_vec();
        self.cloud
            .check_session(&self.session)
            .map_err(String::from)?;
        if bytes.len() > 50 * 1024 * 1024 {
            return Err("The sync payload exceeds the download limit.".into());
        }
        Ok(bytes)
    }
}

fn key_service(account: &str, generation: &str) -> String {
    super::service(&format!("com.urbudget.sync-v2.{account}.{generation}"))
}
fn read_key(cloud: &Cloud, account: &str, generation: &str) -> Option<crypto::Key> {
    super::vault::read(&key_service(account, generation))
        .and_then(|k| BASE64.decode(k).ok())
        .and_then(|k| k.try_into().ok())
        .or_else(|| {
            cloud
                .v2_keys
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&(account.into(), generation.into()))
                .copied()
        })
}
fn store_key(cloud: &Cloud, account: &str, generation: &str, key: crypto::Key) -> Result<()> {
    if super::vault::STORE.is_some() {
        super::vault::save(&key_service(account, generation), &BASE64.encode(key))?;
    }
    cloud
        .v2_keys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((account.into(), generation.into()), key);
    Ok(())
}
pub(super) fn forget_keys(cloud: &Cloud, account: Option<&str>) -> Result<()> {
    if let Some(account) = account {
        let library = budgets::Budgets::load(&cloud.data_dir)?;
        let state = library.account_state(account)?;
        if let Some(generation) = state["bootstrap"]["keyGeneration"].as_str() {
            super::vault::delete(&key_service(account, generation))?;
        }
        if let Some(slot) = library.pending_delivery(account)? {
            if slot.manifest.get("key").is_some() {
                super::vault::delete(&key_service(
                    account,
                    &format!("pending-{}", slot.operation_id),
                ))?;
            }
        }
    }
    cloud
        .v2_keys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    Ok(())
}

pub fn is_active(cloud: &Cloud, library: &Library) -> bool {
    cloud
        .selected_account()
        .and_then(|id| library.lock().account_state(&id).ok())
        .is_some_and(|s| s["protocol"] == 2)
}
pub fn unlocked(cloud: &Cloud) -> Option<bool> {
    let library = budgets::Budgets::load(&cloud.data_dir).ok()?;
    unlocked_catalog(cloud, &library)
}
pub(super) fn unlocked_catalog(cloud: &Cloud, library: &budgets::Budgets) -> Option<bool> {
    let account = cloud.selected_account()?;
    let state = library.account_state(&account).ok()?;
    if state["protocol"] != 2 {
        return None;
    }
    Some(
        state["bootstrap"]["keyGeneration"]
            .as_str()
            .is_some_and(|g| read_key(cloud, &account, g).is_some()),
    )
}
/// Status describes persisted work now, rather than the last successful HTTP attempt.
pub(super) fn decorate_status(cloud: &Cloud, catalog: &budgets::Budgets, status: &mut super::sync::Status) {
    let Some(account) = cloud.selected_account() else {
        return;
    };
    let Ok(state) = catalog.account_state(&account) else {
        return;
    };
    if state["protocol"] != 2 {
        return;
    }
    status.protocol = 2;
    status.enabled = state["enabled"] == true;
    let Ok(entries) = catalog.try_list_all() else {
        return;
    };
    let mut last = status.last.clone().unwrap_or(super::sync::Report {
        at: now(),
        state: "ok",
        message: None,
        budgets: vec![],
        open_changed: false,
        changed_budgets: None,
        library_version: None,
    });
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let prior = last.budgets.iter().find(|r| r.id == entry.id).cloned();
        let blocked = state["blockedDelivery"]["localBudgetId"].as_str() == Some(entry.id.as_str());
        let phase = if blocked || entry.status == "error" {
            "error"
        } else if entry.status == "deletion_conflict" {
            "deletion_conflict"
        } else if entry.lifecycle == "recovery" {
            "recovery"
        } else if entry.lifecycle == "trashed" {
            if entry.pending {
                "queued"
            } else {
                "trashed"
            }
        } else if entry.lifecycle == "purged" {
            "purged"
        } else if entry.account_id.is_none() {
            "local_only"
        } else if entry.account_id.as_deref() != Some(&account) || cloud.token().is_none() {
            "signed_out"
        } else if entry.availability == "evicted" {
            "evicted"
        } else if entry.availability == "unavailable" {
            "error"
        } else if !status.unlocked {
            "locked"
        } else if state["enabled"] != true {
            "paused"
        } else if entry.pending {
            match prior.as_ref().map(|r| r.state) {
                Some("conflict") => "conflict",
                Some("error") => "error",
                Some("subscription_required") => "subscription_required",
                Some("waiting") => "waiting",
                _ => "queued",
            }
        } else if entry.availability == "remote" {
            "cloud_only"
        } else {
            "synced"
        };
        let mut row = prior.unwrap_or_else(|| report_entry(&entry, phase, None));
        row.state = phase;
        if blocked { row.message = state["blockedDelivery"]["message"].as_str().map(str::to_owned); }
        row.name = entry.name;
        rows.push(row);
    }
    if rows.iter().any(|r| {
        matches!(
            r.state,
            "conflict" | "error" | "recovery" | "subscription_required" | "deletion_conflict"
        )
    }) {
        last.state = "attention";
    } else if rows
        .iter()
        .any(|r| r.state == "queued" || r.state == "waiting")
    {
        last.state = "waiting";
    } else if !status.unlocked {
        last.state = if cloud.token().is_none() {
            "signed_out"
        } else {
            "locked"
        };
    }
    if state["blockedDelivery"].is_object() {
        last.state = "attention";
        last.message = state["blockedDelivery"]["message"].as_str().map(str::to_owned);
    }
    last.budgets = rows;
    status.last = Some(last);
}

fn identity(account: &str, bootstrap: &Value, head: &Head) -> Result<RemoteIdentity> {
    Ok(RemoteIdentity {
        account_id: account.into(),
        key_generation: head.key_generation.clone(),
        catalog_epoch: string(bootstrap, "catalogEpoch")?,
        budget_id: head.budget_id.clone(),
        incarnation: head.incarnation.clone(),
    })
}
fn aad(context: &Value, metadata: bool) -> Vec<u8> {
    format!(
        "urbudget:v2:{}:{}",
        if metadata { "meta" } else { "budget" },
        canonical(context)
    )
    .into_bytes()
}
const MAX_CIPHERTEXT: usize = 50 * 1024 * 1024;
fn check_upload_size(length: usize) -> Result<()> {
    if length > MAX_CIPHERTEXT {
        Err("PAYLOAD_TOO_LARGE: This budget exceeds the 50 MiB sync limit. Your edits are saved on this device; reduce the budget before syncing it.".into())
    } else { Ok(()) }
}
fn seal(key: &crypto::Key, context: &Value, budget: &Budget) -> Result<(Vec<u8>, Payload)> {
    let mut light = budget.clone();
    for field in crate::store::HEAVY_EXTRAS {
        light.extra.remove(*field);
    }
    let plain = crypto::compress(
        &serde_json::to_vec(&json!({"format":2,"app":env!("CARGO_PKG_VERSION"),"budget":light}))
            .map_err(|e| e.to_string())?,
    )?;
    let bytes = crypto::seal(key, &aad(context, false), &plain)?;
    check_upload_size(bytes.len())?;
    let meta = BASE64.encode(crypto::seal(
        key,
        &aad(context, true),
        &serde_json::to_vec(&json!({"name":budget.name,"currency":budget.currency}))
            .map_err(|e| e.to_string())?,
    )?);
    let payload = Payload {
        format: "v2".into(),
        sha256: Some(hash(&bytes)),
        length: bytes.len() as u64,
        meta,
        context: context.clone(),
    };
    Ok((bytes, payload))
}
fn decrypt(key: &crypto::Key, account: &str, head: &Head, bytes: &[u8]) -> Result<Budget> {
    let payload = head
        .payload
        .as_ref()
        .ok_or("This budget has no retained encrypted content.")?;
    if bytes.len() as u64 != payload.length
        || payload
            .sha256
            .as_ref()
            .is_some_and(|expected| *expected != hash(bytes))
    {
        return Err("The downloaded ciphertext does not match its advertised checksum.".into());
    }
    let context = &payload.context;
    let plain = if payload.format == "v2" {
        if context["accountId"] != account
            || context["keyGeneration"] != head.key_generation
            || context["budgetId"] != head.budget_id
            || context["incarnation"] != head.incarnation
        {
            return Err("The downloaded payload belongs to a different budget identity.".into());
        }
        crypto::open(key, &aad(context, false), bytes)?
    } else if payload.format == "legacy-v1" {
        crypto::open(
            key,
            format!("urbudget:budget:{}", head.budget_id).as_bytes(),
            bytes,
        )?
    } else {
        return Err("UPDATE_REQUIRED: This budget uses a newer encrypted format.".into());
    };
    let envelope: Value =
        serde_json::from_slice(&crypto::decompress(&plain)?).map_err(|e| e.to_string())?;
    if envelope["format"].as_u64().unwrap_or(u64::MAX) > 2
        || envelope["budget"]["schemaVersion"].as_u64().unwrap_or(0)
            > crate::domain::SCHEMA_VERSION as u64
    {
        return Err("UPDATE_REQUIRED: Update UrBudget to open this budget.".into());
    }
    let budget: Budget =
        serde_json::from_value(envelope["budget"].clone()).map_err(|e| e.to_string())?;
    crate::domain::validate(&budget)?;
    Ok(budget)
}

/// Probe before registering: legacy clients have no accountId and old Workers have no v2 routes.
/// A known v2 account must never silently fall back to the legacy publication engine.
async fn probe_bootstrap(cloud: &Cloud, library: &Library) -> Result<Option<(String, Value)>> {
    let transport = Transport::new(cloud, String::new())?;
    let bootstrap = match transport.json(Method::GET, "/bootstrap", None).await {
        Ok(value) => value,
        Err(error) if !is_active(cloud, library)
            && (error.starts_with("not_found:") || error.starts_with("OFFLINE:")) => return Ok(None),
        Err(error) => return Err(error),
    };
    let account = string(&bootstrap, "accountId")?;
    cloud.backfill_account_id(&transport.session, &account)?;
    let prior = library.lock().ensure_account(&account)?;
    if prior["protocol"] == 2 && bootstrap["protocol"] != 2 {
        return Err("The server returned an older account protocol. Sync is paused to protect this account's data.".into());
    }
    save_bootstrap(library, &account, &bootstrap)?;
    Ok(Some((account, bootstrap)))
}
async fn connect_bootstrap<'a>(
    cloud: &'a Cloud, library: &Library, account: String, bootstrap: Value,
) -> Result<(String, Transport<'a>, Value)> {
    if bootstrap["protocol"] != 2 && bootstrap["activationEnabled"] != true {
        return Err("Automatic budget discovery is not enabled for this account.".into());
    }
    let state = library.lock().ensure_account(&account)?;
    let transport = Transport::new(cloud, string(&state, "installationId")?)?;
    if state["registered"] != true {
        transport.json(Method::POST, "/installations", Some(json!({
            "installationId":transport.installation,"device":super::device_name()
        }))).await?;
        let mut catalog = library.lock();
        let mut fresh = catalog.account_state(&account)?;
        fresh["registered"] = json!(true);
        catalog.set_account_state(&account, &fresh)?;
    }
    Ok((account, transport, bootstrap))
}
async fn connect<'a>(cloud: &'a Cloud, library: &Library) -> Result<(String, Transport<'a>, Value)> {
    let (account, bootstrap) = probe_bootstrap(cloud, library).await?
        .ok_or("OFFLINE: Sync will retry when a connection is available.")?;
    connect_bootstrap(cloud, library, account, bootstrap).await
}

fn save_bootstrap(library: &Library, account: &str, bootstrap: &Value) -> Result<()> {
    let mut library = library.lock();
    let mut state = library.account_state(account)?;
    state["protocol"] = bootstrap["protocol"].clone();
    state["bootstrap"] = bootstrap.clone();
    library.set_account_state(account, &state)
}

/// Recover the one global delivery slot before making any other mutation. The operation's
/// body and ciphertext are read from durable storage, never re-encrypted on a retry.
async fn deliver(transport: &Transport<'_>, db: Option<&Database>, library: &Library, account: &str) -> Result<Option<Value>> {
    let result = deliver_inner(transport, db, library, account).await;
    let mut catalog = library.lock();
    let mut state = catalog.account_state(account)?;
    match &result {
        Ok(_) => { if let Some(object) = state.as_object_mut() { object.remove("blockedDelivery"); } }
        Err(message) => {
            if let Some(slot) = catalog.pending_delivery(account)? {
                state["blockedDelivery"] = json!({"sequence":slot.sequence,"operationId":slot.operation_id,
                    "localBudgetId":slot.local_budget_id,"message":message});
            }
        }
    }
    catalog.set_account_state(account, &state)?;
    result
}

async fn deliver_inner(
    transport: &Transport<'_>,
    db: Option<&Database>,
    library: &Library,
    account: &str,
) -> Result<Option<Value>> {
    let Some(slot) = library.lock().pending_delivery(account)? else {
        return Ok(None);
    };
    if slot.installation_id != transport.installation {
        return Err("The pending upload belongs to another installation. Preserve this library for recovery.".into());
    }
    let receipt = if let Some(receipt) = slot.receipt.clone() {
        receipt
    } else {
        let (obsolete, removed) = {
            let catalog = library.lock();
            let state = catalog.account_state(account)?;
            let obsolete = matches!(
                slot.manifest["kind"].as_str(),
                Some("create" | "update" | "restore")
            ) && state["bootstrap"]["protocol"] == 2
                && (state["bootstrap"]["keyGeneration"] != slot.manifest["keyGeneration"]
                    || state["bootstrap"]["catalogEpoch"] != slot.manifest["catalogEpoch"]);
            let removed = slot.local_budget_id.as_deref().is_some_and(|id| {
                catalog
                    .list_all()
                    .iter()
                    .any(|e| e.id == id && e.lifecycle != "active")
            });
            (obsolete, removed)
        };
        let prepared = if obsolete || removed {
            transport
                .json(
                    Method::POST,
                    &format!("/mutations/{}/cancel", slot.sequence),
                    Some(json!({"digest":slot.digest,"manifest":slot.manifest})),
                )
                .await?
        } else {
            let mut response = transport.json(Method::POST, &format!("/mutations/{}/prepare", slot.sequence), Some(slot.manifest.clone())).await;
            if response.as_ref().is_err_and(|e| e.starts_with("installation_required:")) {
                transport.json(Method::POST, "/installations", Some(json!({"installationId":transport.installation,"device":super::device_name()}))).await?;
                response = transport.json(Method::POST, &format!("/mutations/{}/prepare", slot.sequence), Some(slot.manifest.clone())).await;
            }
            match response {
                Err(error) if matches!(error.split(':').next(), Some("invalid_manifest" | "too_large" | "invalid_body")) => {
                    // A server-authenticated cancellation, never a guessed sequence advance.
                    // The invalid immutable candidate is discarded; saved edits remain dirty.
                    transport.json(Method::POST, &format!("/mutations/{}/cancel", slot.sequence),
                        Some(json!({"digest":slot.digest,"manifest":slot.manifest}))).await?
                }
                other => other?,
            }
        };
        if prepared["state"] == "terminal" {
            prepared["receipt"].clone()
        } else {
            if prepared["digest"] != slot.digest || prepared["sequence"] != slot.sequence {
                return Err("The server prepared a different sync operation.".into());
            }
            if slot.manifest.get("payload").is_some() && prepared["blobReady"] != true {
                let request = transport
                    .request(Method::PUT, &format!("/mutations/{}/blob", slot.sequence))?
                    .header("X-Sync-Digest", &slot.digest)
                    .header("Content-Type", "application/octet-stream")
                    .body(slot.payload.clone());
                transport.response(request).await?;
            }
            let terminal = transport
                .json(
                    Method::POST,
                    &format!("/mutations/{}/commit", slot.sequence),
                    Some(json!({"digest":slot.digest})),
                )
                .await?;
            if terminal["state"] != "terminal" {
                return Err("Sync has not confirmed this operation yet.".into());
            }
            terminal["receipt"].clone()
        }
    };
    if receipt["sequence"] != slot.sequence
        || receipt["digest"] != slot.digest
        || receipt["operationId"] != slot.operation_id
        || receipt["accountId"] != account
        || receipt["kind"] != slot.manifest["kind"]
        || !matches!(
            receipt["outcome"].as_str(),
            Some("accepted" | "rejected" | "cancelled" | "expired")
        )
    {
        return Err(
            "The sync receipt does not match this installation's pending operation.".into(),
        );
    }
    library
        .lock()
        .record_delivery_receipt(account, &slot.sequence, &slot.digest, &receipt)?;
    if let Some(local) = slot.local_budget_id.as_deref() {
        let accepted = if receipt["outcome"] == "accepted" {
            Some(string(&receipt["budget"], "version")?)
        } else {
            None
        };
        let acknowledge = |store: &mut Store| {
            store.acknowledge_upload(
                &slot.operation_id,
                &slot.digest,
                accepted.as_deref(),
                &receipt,
            )
        };
        if let Some(db) = db {
            library_api::with_store(db, library, local, acknowledge)?;
        } else {
            let library = library.lock();
            let path = library.database(local)?;
            if !path.exists() {
                return Err("The pending upload's local budget is missing. Preserve this delivery slot for recovery.".into());
            }
            let mut store = Store::open_budget(&path, local)?;
            acknowledge(&mut store)?;
        }
    }
    if receipt["outcome"] == "accepted"
        && matches!(
            receipt["kind"].as_str(),
            Some("keySetup" | "keyReset" | "migrateProtocol")
        )
    {
        let generation = string(&receipt, "keyGeneration")?;
        if receipt["kind"] != "migrateProtocol" {
            let key = read_key(
                transport.cloud,
                account,
                &format!("pending-{}", slot.operation_id),
            )
            .ok_or("Unlock the pending passphrase change before completing sync.")?;
            store_key(transport.cloud, account, &generation, key)?;
            if receipt["kind"] == "keyReset" {
                if let Some(old) = slot.manifest["keyGeneration"]
                    .as_str()
                    .filter(|old| *old != generation)
                {
                    super::vault::delete(&key_service(account, old))?;
                    transport
                        .cloud
                        .v2_keys
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&(account.into(), old.into()));
                }
            }
        }
        let mut library = library.lock();
        let mut state = library.account_state(account)?;
        state["protocol"] = json!(2);
        state["enabled"] = json!(true);
        state["bootstrap"]["keyGeneration"] = json!(generation);
        state["bootstrap"]["catalogEpoch"] = receipt["catalogEpoch"].clone();
        state["bootstrap"]["protocol"] = json!(2);
        if slot.manifest.get("key").is_some() {
            state["bootstrap"]["salt"] = slot.manifest["key"]["salt"].clone();
            state["bootstrap"]["check"] = slot.manifest["key"]["check"].clone();
        }
        library.set_account_state(account, &state)?;
    }
    if slot.local_budget_id.is_none() {
        let mut catalog = library.lock();
        if catalog.job(&slot.operation_id)?.is_none() {
            catalog.enqueue_job(&budgets::LifecycleJob {
                operation_id: slot.operation_id.clone(),
                local_budget_id: None,
                kind: slot.manifest["kind"].as_str().unwrap_or("control").into(),
                payload: slot.manifest.clone(),
                state: "pending".into(),
                receipt: None,
            })?;
        }
        catalog.finish_job(&slot.operation_id, &receipt)?;
    }
    library
        .lock()
        .complete_delivery(account, &slot.sequence, &slot.digest)?;
    if slot.manifest.get("key").is_some() {
        // The durable receipt and installation sequence already acknowledge success.
        // Credential cleanup must not turn it into an apparent failed reset: a retry
        // would otherwise allocate a new destructive operation. On failure the redundant
        // pending key remains protected in the vault; the current generation key is saved.
        let _ = super::vault::delete(&key_service(
            account,
            &format!("pending-{}", slot.operation_id),
        ));
        transport
            .cloud
            .v2_keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(account.into(), format!("pending-{}", slot.operation_id)));
    }
    if let Some(local) = slot.local_budget_id.as_deref() {
        if let Some(db) = db {
            library_api::with_store(db, library, local, |s| {
                s.forget_completed_upload(&slot.operation_id)
            })?;
        }
    }
    Ok(Some(receipt))
}

fn accepted(receipt: &Value) -> Result<()> {
    if receipt["outcome"] == "accepted" {
        Ok(())
    } else {
        Err(format!(
            "{}: Sync operation {}. Local changes have been preserved.",
            receipt["code"].as_str().unwrap_or("rejected"),
            receipt["outcome"].as_str().unwrap_or("rejected")
        ))
    }
}

/// Retrying the submitted reset completes that operation; it cannot implicitly request a
/// second destructive reset after recovering the first one's lost acknowledgement.
fn completed_reset_retry(
    reset: bool,
    manifest: &Value,
    receipt: &Value,
    pending_key_verified: bool,
) -> Result<Option<Value>> {
    if !reset || manifest["kind"] != "keyReset" {
        return Ok(None);
    }
    if !pending_key_verified {
        return Err(
            "Verify the passphrase used for the pending key change before completing it.".into(),
        );
    }
    accepted(receipt)?;
    Ok(Some(json!({"created":false,"protocol":2})))
}

pub async fn try_setup(
    cloud: &Cloud,
    library: &Library,
    passphrase: &str,
    reset: bool,
) -> Result<Option<Value>> {
    let _operation = cloud.sync.operation().await?;
    let Some((account, bootstrap)) = probe_bootstrap(cloud, library).await? else { return Ok(None); };
    if bootstrap["protocol"] != 2 && bootstrap["activationEnabled"] != true { return Ok(None); }
    let (account, transport, mut bootstrap) = connect_bootstrap(cloud, library, account, bootstrap).await?;
    if passphrase.chars().count() < 8 {
        return Err("Use a passphrase of at least 8 characters.".into());
    }
    save_bootstrap(library, &account, &bootstrap)?;
    // A pending key change owns its own salt and key check, including after process death.
    let pending = library.lock().pending_delivery(&account)?;
    if let Some(slot) = pending {
        let pending_key_verified = if let Some(salt) = slot.manifest["key"]["salt"].as_str() {
            let salt = BASE64.decode(salt).map_err(|e| e.to_string())?;
            let key = crypto::derive(passphrase, &salt)?;
            let check = BASE64
                .decode(slot.manifest["key"]["check"].as_str().unwrap_or_default())
                .map_err(|e| e.to_string())?;
            if !crypto::matches_check(&key, &check) {
                return Err("Enter the passphrase used for the pending key change.".into());
            }
            store_key(
                cloud,
                &account,
                &format!("pending-{}", slot.operation_id),
                key,
            )?;
            true
        } else {
            false
        };
        let receipt = deliver(&transport, None, library, &account)
            .await?
            .ok_or("The pending key operation disappeared before its outcome was confirmed.")?;
        if let Some(completed) =
            completed_reset_retry(reset, &slot.manifest, &receipt, pending_key_verified)?
        {
            // deliver() durably installed the accepted generation/epoch/key and enabled
            // state. A subsequent offline bootstrap fetch cannot invalidate that success.
            return Ok(Some(completed));
        }
        bootstrap = transport.json(Method::GET, "/bootstrap", None).await?;
        save_bootstrap(library, &account, &bootstrap)?;
    }
    let created = bootstrap["keyGeneration"].is_null();
    if !created && !reset {
        let salt = BASE64
            .decode(string(&bootstrap, "salt")?)
            .map_err(|e| e.to_string())?;
        let key = crypto::derive(passphrase, &salt)?;
        let check = BASE64
            .decode(string(&bootstrap, "check")?)
            .map_err(|e| e.to_string())?;
        if !crypto::matches_check(&key, &check) {
            return Err("That isn't the sync passphrase for this account.".into());
        }
        store_key(cloud, &account, &string(&bootstrap, "keyGeneration")?, key)?;
    }
    if created || reset || bootstrap["protocol"] != 2 {
        let operation = budgets::new_id();
        let mut manifest = json!({"accountId":account,"installationId":transport.installation,"operationId":operation,
            "kind":if reset {"keyReset"} else if created {"keySetup"} else {"migrateProtocol"},
            "keyGeneration":bootstrap["keyGeneration"],"catalogEpoch":bootstrap["catalogEpoch"],"expectedProtocol":bootstrap["protocol"]});
        if created || reset {
            if super::vault::STORE.is_none() {
                return Err("A secure credential store is required to safely create or reset the sync passphrase on this device.".into());
            }
            let salt: [u8; crypto::SALT_LEN] = crypto::random()?;
            let key = crypto::derive(passphrase, &salt)?;
            manifest["key"] =
                json!({"salt":BASE64.encode(salt),"check":BASE64.encode(crypto::key_check(&key)?)});
            store_key(cloud, &account, &format!("pending-{operation}"), key)?;
        }
        let digest = hash(canonical(&manifest).as_bytes());
        library
            .lock()
            .prepare_delivery(&account, &operation, None, &manifest, &[], "", &digest)?;
        let receipt = deliver(&transport, None, library, &account)
            .await?
            .ok_or("The key operation was not queued.")?;
        accepted(&receipt)?;
        if reset {
            return Ok(Some(json!({"created":created,"protocol":2})));
        }
        bootstrap = transport.json(Method::GET, "/bootstrap", None).await?;
    }
    save_bootstrap(library, &account, &bootstrap)?;
    let mut library = library.lock();
    let mut state = library.account_state(&account)?;
    state["enabled"] = json!(true);
    library.set_account_state(&account, &state)?;
    Ok(Some(json!({"created":created,"protocol":2})))
}

#[cfg(test)]
mod reset_retry_tests {
    use super::*;

    #[test]
    fn recovered_reset_acknowledgement_finishes_without_requesting_another_reset() {
        let manifest = json!({"kind":"keyReset","operationId":"original-reset","key":{"salt":"submitted-salt","check":"submitted-check"}});
        let receipt = json!({"outcome":"accepted","kind":"keyReset","operationId":"original-reset","keyGeneration":"2"});
        assert_eq!(
            completed_reset_retry(true, &manifest, &receipt, true).unwrap(),
            Some(json!({"created":false,"protocol":2}))
        );
        assert!(
            completed_reset_retry(true, &manifest, &receipt, false).is_err(),
            "an accepted receipt alone cannot bypass the submitted passphrase check"
        );
        for outcome in ["rejected", "cancelled", "expired"] {
            let failed = json!({"outcome":outcome,"code":"recorded_failure","kind":"keyReset","operationId":"original-reset"});
            let error = completed_reset_retry(true, &manifest, &failed, true).unwrap_err();
            assert!(error.contains(outcome));
        }
        assert_eq!(
            completed_reset_retry(false, &manifest, &receipt, true).unwrap(),
            None
        );
        assert_eq!(
            completed_reset_retry(true, &json!({"kind":"update"}), &receipt, false).unwrap(),
            None
        );
    }
}

fn metadata(key: &crypto::Key, head: &Head) -> Option<(String, String)> {
    let payload = head.payload.as_ref()?;
    let bytes = BASE64.decode(&payload.meta).ok()?;
    let associated = if payload.format == "v2" {
        aad(&payload.context, true)
    } else {
        format!("urbudget:meta:{}", head.budget_id).into_bytes()
    };
    let raw = crypto::open(key, &associated, &bytes).ok()?;
    let value: Value = serde_json::from_slice(&raw).ok()?;
    Some((
        value["name"].as_str()?.into(),
        value["currency"].as_str()?.into(),
    ))
}
fn observe(
    library: &Library,
    account: &str,
    bootstrap: &Value,
    key: Option<&crypto::Key>,
    head: &Head,
) -> Result<String> {
    let names = key.and_then(|key| metadata(key, head));
    library.lock().observe_catalog_json(
        account,
        &head.key_generation,
        &string(bootstrap, "catalogEpoch")?,
        &serde_json::to_value(head).map_err(|e| e.to_string())?,
        names.as_ref().map(|v| v.0.as_str()),
        names.as_ref().map(|v| v.1.as_str()),
    )
}

async fn reconcile_catalog(
    transport: &Transport<'_>,
    library: &Library,
    account: &str,
    bootstrap: &Value,
    key: Option<&crypto::Key>,
) -> Result<()> {
    let state = library.lock().account_state(account)?;
    let same_epoch = state["cursorEpoch"] == bootstrap["catalogEpoch"];
    let mut cursor = state["cursor"]
        .as_str()
        .filter(|_| same_epoch)
        .map(str::to_owned);
    if let Some(after) = cursor.clone() {
        let query = transport
            .scoped(Method::GET, "/catalog/changes", bootstrap)?
            .query(&[("after", after)]);
        match transport.response(query).await {
            Ok(response) => {
                let mut page: Value = response.json().await.map_err(|e| e.to_string())?;
                let through = string(&page, "through")?;
                loop {
                    if page["catalogEpoch"] != bootstrap["catalogEpoch"]
                        || page["keyGeneration"] != bootstrap["keyGeneration"]
                    {
                        return Err("The catalog scope changed; restart reconciliation.".into());
                    }
                    for event in page["events"]
                        .as_array()
                        .ok_or("Invalid catalog event page.")?
                    {
                        if event["type"] == "budget" {
                            let head: Head = serde_json::from_value(event["head"].clone())
                                .map_err(|e| e.to_string())?;
                            observe(library, account, bootstrap, key, &head)?;
                        } else {
                            return Err("The catalog was reset; refresh its bootstrap.".into());
                        }
                    }
                    let next = string(&page, "cursor")?;
                    if page["hasMore"] != true {
                        cursor = Some(next);
                        break;
                    }
                    if cursor.as_deref() == Some(&next) {
                        return Err("The catalog cursor did not advance.".into());
                    }
                    cursor = Some(next.clone());
                    page = transport
                        .response(
                            transport
                                .scoped(Method::GET, "/catalog/changes", bootstrap)?
                                .query(&[("after", next), ("through", through.clone())]),
                        )
                        .await?
                        .json()
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            Err(error) if error.starts_with("cursor_expired:") => cursor = None,
            Err(error) => return Err(error),
        }
    }
    if cursor.is_none() {
        let snapshot = transport.catalog(bootstrap).await?;
        if snapshot["catalogEpoch"] != bootstrap["catalogEpoch"]
            || snapshot["keyGeneration"] != bootstrap["keyGeneration"]
        {
            return Err("The catalog changed during bootstrap.".into());
        }
        let heads: Vec<Head> =
            serde_json::from_value(snapshot["budgets"].clone()).map_err(|e| e.to_string())?;
        let present: std::collections::BTreeSet<_> =
            heads.iter().map(|h| h.budget_id.clone()).collect();
        for head in &heads {
            observe(library, account, bootstrap, key, head)?;
        }
        // A missing active-snapshot entry is never evidence of deletion. Query its identity.
        let known: Vec<_> = library
            .lock()
            .list_all()
            .into_iter()
            .filter(|e| e.account_id.as_deref() == Some(account))
            .filter_map(|e| e.remote_id)
            .filter(|id| !present.contains(id))
            .collect();
        for ids in known.chunks(100) {
            let response: Value = transport
                .response(
                    transport
                        .scoped(Method::POST, "/budgets/status", bootstrap)?
                        .json(&json!({"ids":ids})),
                )
                .await?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            if response["catalogEpoch"] != bootstrap["catalogEpoch"] {
                return Err("The catalog changed during identity reconciliation.".into());
            }
            for status in response["budgets"]
                .as_array()
                .ok_or("Invalid budget status response.")?
            {
                if !status["head"].is_null() {
                    let head: Head = serde_json::from_value(status["head"].clone())
                        .map_err(|e| e.to_string())?;
                    observe(library, account, bootstrap, key, &head)?;
                }
            }
        }
        // Trash is part of the unified library even on a newly installed device.
        let mut after = String::new();
        loop {
            let page: Value = transport
                .response(
                    transport
                        .scoped(Method::GET, "/trash", bootstrap)?
                        .query(&[("after", &after)]),
                )
                .await?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            if page["catalogEpoch"] != bootstrap["catalogEpoch"] {
                return Err("The trash catalog changed during download.".into());
            }
            for raw in page["budgets"].as_array().ok_or("Invalid trash page.")? {
                let head: Head = serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
                observe(library, account, bootstrap, key, &head)?;
            }
            match page["next"].as_str() {
                Some(next) if next > after.as_str() => after = next.into(),
                Some(_) => return Err("The trash cursor did not advance.".into()),
                None => break,
            }
        }
        cursor = Some(string(&snapshot, "highwater")?);
    }
    let mut library = library.lock();
    let mut state = library.account_state(account)?;
    state["cursor"] = json!(cursor);
    state["cursorEpoch"] = bootstrap["catalogEpoch"].clone();
    library.set_account_state(account, &state)
}

pub fn enroll_created(cloud: &Cloud, library: &Library, id: &str) -> Result<()> {
    let Some(account) = cloud.selected_account() else {
        return Ok(());
    };
    let mut library = library.lock();
    let state = library.account_state(&account)?;
    if state["enabled"] == true && state["protocol"] == 2 {
        library.associate(id, &account, &budgets::new_id(), &budgets::new_id())?;
        library.set_entry_status(id, "queued")?;
    }
    Ok(())
}
/// Explicit device-only and sample budgets never acquire a cloud identity implicitly.
pub fn create_with_scope(
    cloud: &Cloud,
    library: &mut budgets::Budgets,
    budget: &Budget,
    local_only: bool,
) -> Result<String> {
    if local_only || budget.extra.get("sample").is_some_and(|sample| sample.is_object() || sample == true) {
        library.create()
    } else {
        create_owned(cloud, library)
    }
}
pub fn create_owned(cloud: &Cloud, library: &mut budgets::Budgets) -> Result<String> {
    let account = cloud.selected_account().filter(|account| {
        library
            .account_state(account)
            .is_ok_and(|s| s["enabled"] == true && s["protocol"] == 2)
    });
    library.create_owned(account.as_deref())
}
pub fn enable(
    cloud: &Cloud,
    library: &Library,
    enabled: bool,
    ids: &[String],
) -> Result<super::Status> {
    let account = cloud
        .selected_account()
        .ok_or("Sign in to your UrBudget account first.")?;
    let mut library = library.lock();
    let mut state = library.account_state(&account)?;
    if state["protocol"] != 2 {
        return Err("Unlock account Sync before enabling automatic sync.".into());
    }
    state["enabled"] = json!(enabled);
    library.set_account_state(&account, &state)?;
    if enabled {
        for id in ids {
            let entry = library
                .list_all()
                .into_iter()
                .find(|e| &e.id == id)
                .ok_or("That budget no longer exists.")?;
            if entry.account_id.is_some() {
                if entry.account_id.as_deref() != Some(&account) {
                    return Err(
                        "This budget belongs to another account. Make a local copy to move it."
                            .into(),
                    );
                }
                continue;
            }
            if entry.lifecycle != "active" || entry.availability != "local" {
                return Err(
                    "Only an active local budget can be associated with this account.".into(),
                );
            }
            library.associate(id, &account, &budgets::new_id(), &budgets::new_id())?;
            library.set_entry_status(id, "queued")?;
        }
    }
    drop(library);
    Ok(cloud.status())
}

fn head_for(library: &Library, account: &str, remote: &str) -> Result<Option<Head>> {
    library
        .lock()
        .cached_catalog_head(account, remote)?
        .map(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
        .transpose()
}
fn report_entry(
    entry: &budgets::Listing,
    state: &'static str,
    message: Option<String>,
) -> super::sync::BudgetSync {
    super::sync::BudgetSync {
        id: entry.id.clone(),
        name: entry.name.clone(),
        state,
        device: None,
        updated_at: None,
        message,
        conflicts: vec![],
        notes: vec![],
        review: None,
    }
}
fn close_removed(db: &Database, library: &Library, id: &str) -> Result<bool> {
    let open = db.store().is_ok_and(|s| s.id() == id);
    if !open {
        return Ok(false);
    }
    // Checkpoint an in-memory draft before the open Store is detached. Per-editor raw
    // fields were already checkpointed by adapters and remain in the retained database.
    library_api::with_store(db, library, id, |store| {
        if store.has_unsaved_draft() {
            let draft = serde_json::to_value(store.full_draft(None)?).map_err(|e| e.to_string())?;
            store.checkpoint_editor(&urbudget_core::sync_storage::EditorCheckpoint {
                editor_id: format!("closed-budget-draft-{}", library_api::process_owner()),
                epoch: store.status()?.token,
                process_owner: library_api::process_owner(),
                target: json!({"kind":"budget"}),
                baseline: json!({}),
                draft,
            })?;
        }
        store.set_sync_blocked(true)
    })?;
    library.lock().clear_current()?;
    db.replace(None);
    Ok(true)
}

async fn send_saved(
    transport: &Transport<'_>,
    db: &Database,
    library: &Library,
    account: &str,
    bootstrap: &Value,
    key: &crypto::Key,
    entry: &budgets::Listing,
) -> Result<Value> {
    if library.lock().pending_delivery(account)?.is_some() {
        return Err("Another sync operation needs attention before this upload can be published. Your changes are saved locally.".into());
    }
    let (intent, token) = library_api::with_store(db, library, &entry.id, |store| {
        if let Some(intent) = store.pending_upload()? {
            return Ok((intent, String::new()));
        }
        let (mut budget, status) = store.saved()?;
        for field in crate::store::HEAVY_EXTRAS {
            budget.extra.remove(*field);
        }
        let binding = store.sync_binding()?;
        let remote = entry
            .remote_id
            .as_deref()
            .ok_or("This budget is not associated with an account.")?;
        let incarnation = binding
            .as_ref()
            .map(|b| b.identity.incarnation.as_str())
            .or(entry.incarnation.as_deref())
            .ok_or("This budget has no sync incarnation.")?;
        let identity = RemoteIdentity {
            account_id: account.into(),
            key_generation: string(bootstrap, "keyGeneration")?,
            catalog_epoch: string(bootstrap, "catalogEpoch")?,
            budget_id: remote.into(),
            incarnation: incarnation.into(),
        };
        if binding.as_ref().is_some_and(|b| b.identity != identity) {
            return Err("RECOVERY_REQUIRED: This budget's remote identity changed; preserve a copy before continuing.".into());
        }
        let expected = binding
            .as_ref()
            .map(|b| b.remote_version.as_str())
            .unwrap_or("0");
        let operation = budgets::new_id();
        let context = json!({"format":"v2","accountId":account,"keyGeneration":identity.key_generation,"budgetId":remote,"incarnation":incarnation,"operationId":operation,"parentVersion":expected});
        let (bytes, payload) = seal(key, &context, &budget)?;
        let mut manifest = json!({"accountId":account,"installationId":transport.installation,"operationId":operation,"kind":if expected=="0" {"create"}else{"update"},"keyGeneration":identity.key_generation,"catalogEpoch":identity.catalog_epoch,"budgetId":remote,"expectedVersion":expected,"payload":payload});
        manifest[if expected == "0" {
            "newIncarnation"
        } else {
            "incarnation"
        }] = json!(incarnation);
        let intent = UploadIntent {
            operation_id: operation,
            identity,
            expected_version: expected.into(),
            local_revision: status.revision,
            candidate: budget,
            manifest: manifest.clone(),
            payload: bytes,
            metadata: payload.meta,
            digest: hash(canonical(&manifest).as_bytes()),
        };
        store.prepare_upload(&intent, &status.token)?;
        Ok((intent, status.token))
    })?;
    let _ = token;
    library.lock().prepare_delivery(
        account,
        &intent.operation_id,
        Some(&entry.id),
        &intent.manifest,
        &intent.payload,
        &intent.metadata,
        &intent.digest,
    )?;
    let receipt = deliver(transport, Some(db), library, account)
        .await?
        .ok_or("The upload was not queued.")?;
    if receipt["outcome"] == "accepted" {
        let head: Head =
            serde_json::from_value(receipt["budget"].clone()).map_err(|e| e.to_string())?;
        observe(library, account, bootstrap, Some(key), &head)?;
    }
    accepted(&receipt)?;
    Ok(receipt)
}

fn merged_intent(
    transport: &Transport<'_>,
    key: &crypto::Key,
    binding: &Binding,
    budget: &Budget,
    local_revision: i64,
) -> Result<UploadIntent> {
    let operation = budgets::new_id();
    let context = json!({"format":"v2","accountId":binding.identity.account_id,"keyGeneration":binding.identity.key_generation,
        "budgetId":binding.identity.budget_id,"incarnation":binding.identity.incarnation,"operationId":operation,"parentVersion":binding.remote_version});
    let (bytes, payload) = seal(key, &context, budget)?;
    let manifest = json!({"accountId":binding.identity.account_id,"installationId":transport.installation,"operationId":operation,"kind":"update",
        "keyGeneration":binding.identity.key_generation,"catalogEpoch":binding.identity.catalog_epoch,"budgetId":binding.identity.budget_id,
        "incarnation":binding.identity.incarnation,"expectedVersion":binding.remote_version,"payload":payload});
    Ok(UploadIntent {
        operation_id: operation,
        identity: binding.identity.clone(),
        expected_version: binding.remote_version.clone(),
        local_revision,
        candidate: budget.clone(),
        payload: bytes,
        metadata: payload.meta,
        digest: hash(canonical(&manifest).as_bytes()),
        manifest,
    })
}

async fn process_budget(
    transport: &Transport<'_>,
    db: &Database,
    library: &Library,
    account: &str,
    bootstrap: &Value,
    key: &crypto::Key,
    entry: &budgets::Listing,
    open_clean: bool,
    report: &mut super::sync::Report,
    resolution: Option<(&str, &crate::sync_merge::ReviewedChoices)>,
) -> Result<super::sync::BudgetSync> {
    if let Some(slot) = library.lock().pending_delivery(account)? {
        if slot.local_budget_id.as_deref() == Some(&entry.id) {
            return Ok(report_entry(entry, "error", Some("This upload's outcome is unresolved. Your edits are saved; sync will keep checking its receipt.".into())));
        }
    }
    let remote_id = entry
        .remote_id
        .as_deref()
        .ok_or("Account budget has no remote identity.")?;
    let head = head_for(library, account, remote_id)?;
    let Some(head) = head else {
        if entry.lifecycle != "active" {
            return Ok(report_entry(entry, "trashed", None));
        }
        let known =
            library_api::with_store(db, library, &entry.id, |s| Ok(s.sync_binding()?.is_some()))?;
        if known {
            return Err("RECOVERY_REQUIRED: The account no longer recognizes this budget identity. Your local copy is preserved.".into());
        }
        if bootstrap["entitlement"]["active"] != true {
            return Ok(report_entry(
                entry,
                "subscription_required",
                Some("Your local budget is safe. Renew Sync to upload it.".into()),
            ));
        }
        send_saved(transport, db, library, account, bootstrap, key, entry).await?;
        return Ok(report_entry(entry, "uploaded", None));
    };
    if head.key_generation != string(bootstrap, "keyGeneration")? {
        library
            .lock()
            .quarantine_binding(&entry.id, &json!({"reason":"key_changed","head":head}))?;
        if !entry.missing {
            report.open_changed |= close_removed(db, library, &entry.id)?;
        }
        return Ok(report_entry(entry,"recovery",Some("The account's encryption key changed. Recover this preserved local budget as a new copy.".into())));
    }
    if head.state != "active" {
        if !entry.missing {
            library_api::with_store(db, library, &entry.id, |s| s.set_sync_blocked(true))?;
            report.open_changed |= close_removed(db, library, &entry.id)?;
        }
        return Ok(report_entry(
            entry,
            if head.state == "purged" {
                "purged"
            } else {
                "trashed"
            },
            Some("Removed from this account. Local recovery data is retained.".into()),
        ));
    }
    let jobs = library.lock().jobs()?;
    if jobs.iter().any(|j| j.local_budget_id.as_deref() == Some(&entry.id) && deletion_conflict_applies(j, &head)) {
        return Ok(report_entry(entry, "deletion_conflict", Some("This budget changed on another device. Review the newer version before deleting it.".into())));
    }
    if jobs
        .iter()
        .any(|j| j.local_budget_id.as_deref() == Some(&entry.id) && j.kind == "trash")
    {
        return Ok(report_entry(
            entry,
            "queued",
            Some("Deletion will sync when its pending delivery is resolved.".into()),
        ));
    }
    if entry.availability == "evicted" {
        return Ok(report_entry(entry, "evicted", None));
    }
    if entry.availability == "unavailable" {
        return Err("The local budget file is missing. Its identity is preserved; recover it or explicitly download its cloud copy.".into());
    }
    let desired = identity(account, bootstrap, &head)?;
    if entry.missing || entry.availability == "remote" {
        let bytes = transport.download(bootstrap, &head).await?;
        let budget = decrypt(key, account, &head, &bytes)?;
        let (path, root) = {
            let catalog = library.lock();
            (catalog.database(&entry.id)?, catalog.data_dir().to_owned())
        };
        let job = budgets::LifecycleJob {
            operation_id: format!("download-{}", entry.id),
            local_budget_id: Some(entry.id.clone()),
            kind: "download".into(),
            payload: json!({"accountId":account,"remoteId":remote_id}),
            state: "pending".into(),
            receipt: None,
        };
        library.lock().enqueue_job(&job)?;
        std::fs::create_dir_all(path.parent().ok_or("Invalid budget path.")?)
            .map_err(|e| e.to_string())?;
        let mut store = Store::open_budget(&path, &entry.id)?.with_backup_preferences(&root);
        if store.read_recovery()?.is_some() || !store.editor_checkpoints()?.is_empty() {
            return Err("Recover or discard this budget's preserved draft before downloading into its file.".into());
        }
        let status = store.status()?;
        if !status.saved_at.is_empty() && store.sync_binding()?.is_none() {
            return Err("A local file appeared during download without verified ancestry. Preserve it for recovery.".into());
        }
        store.save_sync_v2(
            &budget,
            &status.token,
            &Binding {
                identity: desired,
                remote_version: head.version.clone(),
                acknowledged_local_revision: status.revision,
                base: budget.clone(),
            },
            None,
        )?;
        drop(store);
        let mut catalog = library.lock();
        catalog.activate_binding(&entry.id, account, remote_id, &head.incarnation)?;
        return Ok(report_entry(entry, "downloaded", None));
    }
    let (binding, dirty, unsafe_editor, open) =
        library_api::with_store(db, library, &entry.id, |s| {
            Ok((
                s.sync_binding()?,
                s.sync_dirty()?,
                s.has_unsaved_draft()
                    || s.read_recovery()?.is_some()
                    || !s.editor_checkpoints()?.is_empty(),
                entry.current,
            ))
        })?;
    let Some(binding) = binding else {
        return Err("RECOVERY_REQUIRED: This local file has no verified ancestry for the existing cloud budget. Preserve it as a separate copy.".into());
    };
    let incarnation_changed = binding.identity != desired;
    if !incarnation_changed && entry.incarnation.as_deref() != Some(head.incarnation.as_str()) {
        // The financial DB commit precedes catalog bookkeeping. Repair that second step
        // from verified ancestry after a crash instead of rebuilding an obsolete upload.
        let mut catalog = library.lock();
        catalog.activate_binding(&entry.id, account, remote_id, &head.incarnation)?;
    }
    if incarnation_changed && (dirty || unsafe_editor) {
        library_api::with_store(db, library, &entry.id, |s| s.set_sync_blocked(true))?;
        library.lock().quarantine_binding(
            &entry.id,
            &json!({"reason":"incarnation_changed","oldIdentity":binding.identity,"newHead":head}),
        )?;
        report.open_changed |= close_removed(db, library, &entry.id)?;
        observe(library, account, bootstrap, Some(key), &head)?;
        return Ok(report_entry(entry,"recovery",Some("This budget was restored or its account key changed. Recover your local changes as a copy before adopting the new version.".into())));
    }
    if binding.remote_version == head.version && !incarnation_changed {
        if dirty {
            if bootstrap["entitlement"]["active"] != true {
                return Ok(report_entry(
                    entry,
                    "subscription_required",
                    Some("Your local changes are saved. Renew Sync to upload them.".into()),
                ));
            }
            send_saved(transport, db, library, account, bootstrap, key, entry).await?;
            return Ok(report_entry(entry, "uploaded", None));
        }
        return Ok(report_entry(entry, "synced", None));
    }
    if unsafe_editor || (open && !open_clean) {
        return Ok(report_entry(
            entry,
            "waiting",
            Some(
                "Save or finish reviewing your local draft before incoming changes are applied."
                    .into(),
            ),
        ));
    }
    let bytes = transport.download(bootstrap, &head).await?;
    let remote = decrypt(key, account, &head, &bytes)?;
    let mut outcome = report_entry(entry, "downloaded", None);
    let mut changed = false;
    let mut upload = false;
    let mut review: Option<Value> = None;
    library_api::with_store(db, library, &entry.id, |store| {
        if store.has_unsaved_draft()
            || store.read_recovery()?.is_some()
            || !store.editor_checkpoints()?.is_empty()
        {
            return Err("Save or recover the local draft before receiving changes.".into());
        }
        let (mut local, status) = store.saved()?;
        for field in crate::store::HEAVY_EXTRAS {
            local.extra.remove(*field);
        }
        let latest = store
            .sync_binding()?
            .ok_or("The local sync ancestry changed during download.")?;
        if latest != binding {
            return Err("The local sync ancestry changed during download; retry sync.".into());
        }
        let new_binding = Binding {
            identity: desired.clone(),
            remote_version: head.version.clone(),
            acknowledged_local_revision: status.revision,
            base: remote.clone(),
        };
        if incarnation_changed {
            store.adopt_sync_v2(&remote, &status.token, &new_binding)?;
            changed = local != remote;
            return Ok(());
        }
        let merged = if let Some((reviewed_token, choices)) = resolution {
            if status.token != reviewed_token {
                return Err("The budget changed while applying the reviewed conflict choices. Review it again.".into());
            }
            crate::sync_merge::merge_reviewed(Some(&binding.base), &local, &remote, choices)?
        } else {
            crate::sync_merge::merge(Some(&binding.base), &local, &remote, &Default::default())?
        };
        if !merged.conflicts.is_empty() {
            outcome.state = "conflict";
            outcome.conflicts = merged.conflicts;
            outcome.notes = merged.notes;
            review = Some(
                json!({"token":status.token,"identity":desired,"version":head.version,"conflicts":outcome.conflicts}),
            );
            return Ok(());
        }
        changed = local != merged.budget;
        upload = merged.budget != remote;
        let intent = if upload {
            Some(merged_intent(
                transport,
                key,
                &new_binding,
                &merged.budget,
                status.revision,
            )?)
        } else {
            None
        };
        if let Some(choice) = resolution.and_then(|(_, choices)| choices.get("budget/validation")) {
            // The chosen version remains current; retain the losing version as the newest
            // restorable backup without immediately pruning it through a second backup.
            if choice.side == "local" { store.backup_sync_version(&remote)?; }
            else { store.backup()?; }
        } else if changed && (!local.accounts.is_empty() || !local.transactions.is_empty()) {
            store.backup()?;
        }
        store.save_sync_v2(&merged.budget, &status.token, &new_binding, intent.as_ref())?;
        outcome.notes = merged.notes;
        outcome.state = if upload { "merged" } else { "downloaded" };
        Ok(())
    })?;
    report.open_changed |= changed && db.store().is_ok_and(|s| s.id() == entry.id);
    if let Some(review) = review {
        let mut catalog = library.lock();
        let mut state = catalog.account_state(account)?;
        if !state["reviews"].is_object() {
            state["reviews"] = json!({});
        }
        outcome.review = Some(review.clone());
        state["reviews"][&entry.id] = review;
        catalog.set_account_state(account, &state)?;
        return Ok(outcome);
    }
    if incarnation_changed {
        let mut catalog = library.lock();
        catalog.activate_binding(&entry.id, account, remote_id, &head.incarnation)?;
    }
    if upload {
        if bootstrap["entitlement"]["active"] == true {
            send_saved(transport, db, library, account, bootstrap, key, entry).await?;
        } else {
            outcome.state = "subscription_required";
            outcome.message = Some(
                "Incoming changes were merged safely. Renew Sync to upload your remaining changes."
                    .into(),
            );
        }
    }
    Ok(outcome)
}

fn deletion_conflict_applies(job: &budgets::LifecycleJob, head: &Head) -> bool {
    job.kind == "deletion_conflict"
        && job.payload["newHead"]["incarnation"] == head.incarnation
        && job.payload["newHead"]["keyGeneration"] == head.key_generation
}
fn deletion_matches_review(job: &budgets::LifecycleJob, head: &Head, bootstrap: &Value) -> bool {
    let reviewed = &job.payload["reviewedHead"];
    reviewed["version"] == head.version
        && reviewed["incarnation"] == head.incarnation
        && reviewed["keyGeneration"] == head.key_generation
        && reviewed["catalogEpoch"] == bootstrap["catalogEpoch"]
}

fn record_deletion_conflict(library: &Library, job: &budgets::LifecycleJob, head: &Head) -> Result<()> {
    let mut catalog = library.lock();
    // If a server CAS already completed this job, retain that immutable receipt. The
    // separate durable review marker also repairs a crash immediately after its receipt.
    if catalog.job(&job.operation_id)?.is_some_and(|j| j.state != "complete") {
        catalog.finish_job(&job.operation_id, &json!({"outcome":"cancelled","code":"deletion_conflict","budget":head}))?;
    }
    let local = job.local_budget_id.as_deref().ok_or("Missing deletion budget.")?;
    let conflict_id = format!("review-{}", job.operation_id);
    if catalog.job(&conflict_id)?.is_none() {
        catalog.enqueue_job(&budgets::LifecycleJob {
            operation_id: conflict_id,
            local_budget_id: Some(local.into()),
            kind: "deletion_conflict".into(),
            payload: json!({"accountId":job.payload["accountId"],"reviewedHead":job.payload["reviewedHead"],"newHead":head}),
            state: "recovery".into(), receipt: None,
        })?;
    }
    catalog.set_entry_status(local, "deletion_conflict")
}
fn interrupted_deletion_review(library: &Library, entry: &budgets::Listing, head: &Head) -> Result<Option<budgets::LifecycleJob>> {
    // Receiving another device's tombstone never creates a local delete intent. A later
    // restored incarnation must follow normal adoption/recovery even if this device once
    // deleted an older incarnation itself.
    if head.state != "active" || entry.incarnation.as_deref() != Some(&head.incarnation) {
        return Ok(None);
    }
    let prior = library.lock().latest_trash_job(&entry.id)?;
    Ok(prior.filter(|job| job.state == "complete" && job.receipt.as_ref().is_some_and(|r|
        matches!(r["code"].as_str(), Some("conflict" | "deletion_conflict")))))
}
async fn drain_lifecycle(
    transport: &Transport<'_>,
    db: &Database,
    library: &Library,
    account: &str,
    bootstrap: &Value,
    key: Option<&crypto::Key>,
) -> Result<()> {
    // A crash after receiving a rejected CAS must preserve the original review, never
    // reconstruct an unreviewed destructive operation against the latest version.
    let pending = library.lock().jobs()?;
    let entries = library.lock().list_all();
    for entry in entries.iter().filter(|e| e.account_id.as_deref() == Some(account) && e.lifecycle == "trashed") {
        if pending.iter().any(|j| j.local_budget_id.as_deref() == Some(&entry.id)
            && matches!(j.kind.as_str(), "trash" | "restore" | "purge" | "deletion_conflict")) { continue; }
        let Some(remote) = entry.remote_id.as_deref() else { continue; };
        let Some(head) = head_for(library, account, remote)? else { continue; };
        let Some(prior) = interrupted_deletion_review(library, entry, &head)? else { continue; };
        record_deletion_conflict(library, &prior, &head)?;
    }
    let jobs = library.lock().jobs()?;
    // Only unsubmitted actions are coalesced. The allocated installation slot was repaired
    // before this pass, so its unknown server outcome is never guessed or cancelled locally.
    let mut latest = BTreeMap::new();
    for job in &jobs {
        if job.payload["accountId"] == account
            && matches!(job.kind.as_str(), "trash" | "restore" | "purge")
        {
            if let Some(local) = job.local_budget_id.as_deref() {
                latest.insert(local.to_owned(), job.operation_id.clone());
            }
        }
    }
    for job in jobs {
        if job.payload["accountId"] != account
            || !matches!(job.kind.as_str(), "trash" | "restore" | "purge")
        {
            continue;
        }
        let result: Result<()> = async {
        let local = job
            .local_budget_id
            .as_deref()
            .ok_or("A lifecycle action is missing its local budget.")?;
        if latest.get(local) != Some(&job.operation_id) {
            library.lock().finish_job(
                &job.operation_id,
                &json!({"outcome":"cancelled","code":"superseded_before_dispatch"}),
            )?;
            return Ok(());
        }
        let entry = library
            .lock()
            .list_all()
            .into_iter()
            .find(|e| e.id == local)
            .ok_or("The lifecycle budget is missing.")?;
        let remote = entry
            .remote_id
            .as_deref()
            .ok_or("The lifecycle budget has no account identity.")?;
        let head = head_for(library, account, remote)?;
        if job.kind == "restore" && head.as_ref().is_none_or(|h| h.state == "active") {
            let same = head
                .as_ref()
                .is_none_or(|h| entry.incarnation.as_deref() == Some(&h.incarnation));
            if same {
                if !entry.missing {
                    library_api::with_store(db, library, local, |s| s.set_sync_blocked(false))?;
                }
                library.lock().set_lifecycle(local, "active")?;
            }
            library.lock().finish_job(
                &job.operation_id,
                &json!({"outcome":"accepted","code":"already_active","budget":head}),
            )?;
            return Ok(());
        }
        if head.is_none() && job.kind == "trash" {
            // A never-uploaded budget can be removed without manufacturing a cloud object.
            library.lock().finish_job(
                &job.operation_id,
                &json!({"outcome":"accepted","code":"never_uploaded"}),
            )?;
            return Ok(());
        }
        let head = head.ok_or(
            "The remote budget identity is unknown; preserve the requested lifecycle action.",
        )?;
        if bootstrap["keyGeneration"] != head.key_generation {
            library.lock().finish_job(
                &job.operation_id,
                &json!({"outcome":"rejected","code":"invalidated_generation","budget":head}),
            )?;
            return Ok(());
        }
        if (head.state == "trashed" || head.state == "purged") && job.kind == "trash" {
            library.lock().finish_job(
                &job.operation_id,
                &json!({"outcome":"accepted","code":"already_trashed","budget":head}),
            )?;
            return Ok(());
        }
        if job.kind == "trash" && entry.incarnation.as_deref() != Some(&head.incarnation) {
            library.lock().finish_job(
                &job.operation_id,
                &json!({"outcome":"rejected","code":"incarnation_changed","budget":head}),
            )?;
            return Ok(());
        }
        if job.kind == "restore" && head.state == "purged" {
            library.lock().finish_job(
                &job.operation_id,
                &json!({"outcome":"rejected","code":"gone","budget":head}),
            )?;
            library.lock().set_entry_status(
                local,
                "The retained cloud copy has expired. Recover an available local copy.",
            )?;
            return Ok(());
        }
        if job.kind == "trash" && !deletion_matches_review(&job, &head, bootstrap) {
            record_deletion_conflict(library, &job, &head)?;
            return Ok(());
        }
        let mut manifest = json!({"accountId":account,"installationId":transport.installation,"operationId":job.operation_id,"kind":job.kind,
            "keyGeneration":bootstrap["keyGeneration"],"catalogEpoch":bootstrap["catalogEpoch"],"budgetId":remote,"incarnation":head.incarnation,"expectedVersion":head.version});
        let mut bytes = vec![];
        let mut meta = String::new();
        if job.kind == "restore" {
            if bootstrap["entitlement"]["active"] != true {
                library
                    .lock()
                    .set_entry_status(local, "subscription_required")?;
                return Ok(());
            }
            if head.state != "trashed" {
                return Err("Only a retained trashed budget can be restored.".into());
            }
            let Some(key) = key else {
                library.lock().set_entry_status(local, "locked")?;
                return Ok(());
            };
            let source = transport.download(bootstrap, &head).await?;
            let budget = decrypt(key, account, &head, &source)?;
            let incarnation = budgets::new_id();
            let context = json!({"format":"v2","accountId":account,"keyGeneration":bootstrap["keyGeneration"],"budgetId":remote,"incarnation":incarnation,"operationId":job.operation_id,"parentVersion":head.version});
            let (sealed, payload) = seal(key, &context, &budget)?;
            bytes = sealed;
            meta = payload.meta.clone();
            manifest["newIncarnation"] = json!(incarnation);
            manifest["source"] = json!({"incarnation":head.incarnation,"version":head.version,"sha256":head.payload.as_ref().and_then(|p|p.sha256.as_ref())});
            manifest["payload"] = serde_json::to_value(payload).map_err(|e| e.to_string())?;
        }
        let digest = hash(canonical(&manifest).as_bytes());
        library.lock().prepare_delivery(
            account,
            &job.operation_id,
            None,
            &manifest,
            &bytes,
            &meta,
            &digest,
        )?;
        let receipt = deliver(transport, Some(db), library, account)
            .await?
            .ok_or("Lifecycle delivery disappeared.")?;
        if let Ok(head) = serde_json::from_value::<Head>(receipt["budget"].clone()) {
            observe(library, account, bootstrap, key, &head)?;
        }
        if receipt["outcome"] != "accepted" && receipt["code"] == "conflict" && job.kind == "trash" {
            let latest: Head = serde_json::from_value(receipt["budget"].clone()).map_err(|e| e.to_string())?;
            record_deletion_conflict(library, &job, &latest)?;
            return Ok(());
        }
        accepted(&receipt)?;
        Ok(())
        }.await;
        let mut catalog = library.lock();
        let mut state = catalog.account_state(account)?;
        if !state["lifecycleErrors"].is_object() { state["lifecycleErrors"] = json!({}); }
        if let Err(error) = result {
            state["lifecycleErrors"][&job.operation_id] = json!({"localBudgetId":job.local_budget_id,"message":error});
            if let Some(local) = job.local_budget_id.as_deref() { catalog.set_entry_status(local, "error")?; }
        } else if let Some(errors) = state["lifecycleErrors"].as_object_mut() {
            // A successful newer action also clears failures of earlier completed actions on
            // this budget. Otherwise a repaired lifecycle would remain in attention forever.
            errors.retain(|operation, error| operation != &job.operation_id
                && (job.local_budget_id.is_none() || error["localBudgetId"].as_str() != job.local_budget_id.as_deref()));
        }
        catalog.set_account_state(account, &state)?;
    }
    Ok(())
}

pub async fn run(
    cloud: &Cloud,
    db: &Database,
    library: &Library,
    open_clean: bool,
) -> Result<super::sync::Report> {
    let mut report = super::sync::Report {
        at: now(),
        state: "ok",
        message: None,
        budgets: vec![],
        open_changed: false,
        changed_budgets: None,
        library_version: None,
    };
    let _operation = cloud.sync.operation().await?;
    let result = run_inner(cloud, db, library, open_clean, &mut report).await;
    if let Err(error) = result {
        report.state = if error.starts_with("OFFLINE:") {
            "offline"
        } else if cloud.token().is_none() {
            "signed_out"
        } else {
            "error"
        };
        report.message = Some(error);
    }
    Ok(cloud.sync.finish(report))
}
pub async fn discover_active(cloud: &Cloud, library: &Library) -> Result<bool> {
    if is_active(cloud, library) {
        return Ok(true);
    }
    if cloud.token().is_none() {
        return Ok(false);
    }
    let _operation = cloud.sync.operation().await?;
    let Some((_, bootstrap)) = probe_bootstrap(cloud, library).await? else { return Ok(false); };
    Ok(bootstrap["protocol"] == 2)
}
async fn run_inner(
    cloud: &Cloud,
    db: &Database,
    library: &Library,
    open_clean: bool,
    report: &mut super::sync::Report,
) -> Result<()> {
    if cloud.token().is_none() {
        report.state = "signed_out";
        return Ok(());
    }
    let (account, transport, mut bootstrap) = connect(cloud, library).await?;
    save_bootstrap(library, &account, &bootstrap)?;
    if bootstrap["protocol"] != 2 {
        return Err("Unlock Sync to activate the account protocol.".into());
    }
    // Receipt repair does not require a current content key or an active subscription.
    let repaired = match deliver(&transport, Some(db), library, &account).await {
        Ok(receipt) => receipt,
        Err(error) => { report.state = "attention"; report.message = Some(error); None }
    };
    if repaired.as_ref().is_some_and(|r| {
        r["keyGeneration"] != bootstrap["keyGeneration"]
            || r["catalogEpoch"] != bootstrap["catalogEpoch"]
    }) {
        bootstrap = transport.json(Method::GET, "/bootstrap", None).await?;
        save_bootstrap(library, &account, &bootstrap)?;
    }
    let generation = string(&bootstrap, "keyGeneration")?;
    let key = read_key(cloud, &account, &generation);
    let mut entry_errors = BTreeMap::new();
    adopt_legacy_bindings(&transport, db, library, &account, &bootstrap).await?;
    reconcile_catalog(&transport, library, &account, &bootstrap, key.as_ref()).await?;
    // Deletion is catalog metadata: it must invalidate editors even while content is locked.
    let observed = library.lock().list_all();
    for entry in observed
        .into_iter()
        .filter(|e| e.account_id.as_deref() == Some(&account) && !e.missing)
    {
        let head = entry
            .remote_id
            .as_deref()
            .map(|id| head_for(library, &account, id))
            .transpose()?
            .flatten();
        if head.as_ref().is_some_and(|h| h.state != "active") {
            let closed = library_api::with_store(db, library, &entry.id, |s| s.set_sync_blocked(true))
                .and_then(|_| close_removed(db, library, &entry.id));
            match closed {
                Ok(changed) => report.open_changed |= changed,
                Err(error) => { library.lock().set_entry_status(&entry.id,"error")?; entry_errors.insert(entry.id, error); report.state="attention"; }
            }
        }
    }
    let enabled = library.lock().account_state(&account)?["enabled"] == true;
    // Recover a crash after per-budget preparation but before delivery-slot allocation.
    let entries = library.lock().list_all();
    for entry in &entries {
        if entry.account_id.as_deref() != Some(&account) || entry.missing {
            continue;
        }
        let pending = match library_api::with_store(db, library, &entry.id, |s| s.pending_upload()) {
            Ok(pending) => pending,
            Err(error) => { library.lock().set_entry_status(&entry.id,"error")?; entry_errors.insert(entry.id.clone(),error); report.state="attention"; continue; }
        };
        if let Some(intent) = pending {
            if library.lock().pending_delivery(&account)?.is_some() { continue; }
            if bootstrap["entitlement"]["active"] != true || !enabled {
                continue;
            }
            let recovered = async {
                library.lock().prepare_delivery(&account, &intent.operation_id, Some(&entry.id), &intent.manifest, &intent.payload, &intent.metadata, &intent.digest)?;
                deliver(&transport, Some(db), library, &account).await
            }.await;
            if let Err(error) = recovered { report.state = "attention"; report.message = Some(error); }
        }
    }
    // Trash/purge contain no financial plaintext and remain available with a locked key.
    // Unknown content outcomes above are settled before any lifecycle sequence is allocated;
    // restore alone waits for a key because it must authenticate and re-seal retained content.
    if let Err(error) = drain_lifecycle(&transport, db, library, &account, &bootstrap, key.as_ref()).await {
        report.state = "attention"; report.message = Some(error);
    }
    if key.is_none() {
        if report.state != "attention" { report.state = "locked"; }
        return Ok(());
    }
    let key = key.unwrap();
    // Catalog cursors also advance while locked. Decode cached metadata after unlocking,
    // even when an evicted budget has no subsequent event or download.
    let cached = library.lock().catalog_heads(&account)?;
    let mapped: std::collections::BTreeSet<_> = library
        .lock()
        .list_all()
        .into_iter()
        .filter(|e| e.account_id.as_deref() == Some(&account))
        .filter_map(|e| e.remote_id)
        .collect();
    for cached in cached {
        let head: Head = serde_json::from_value(cached.metadata).map_err(|e| e.to_string())?;
        if head.key_generation == generation {
            if let Some((name, currency)) = metadata(&key, &head) {
                if name != cached.name
                    || currency != cached.currency
                    || !mapped.contains(&head.budget_id)
                {
                    observe(library, &account, &bootstrap, Some(&key), &head)?;
                }
            }
        }
    }
    let entries = library.lock().list_all();
    let lifecycle_errors = library.lock().account_state(&account)?["lifecycleErrors"].clone();
    for entry in entries {
        let mut outcome = if let Some(error) = entry_errors.remove(&entry.id) {
            report_entry(&entry,"error",Some(error))
        } else if entry.account_id.is_none() {
            report_entry(&entry, "local_only", None)
        } else if entry.account_id.as_deref() != Some(&account) {
            report_entry(
                &entry,
                "signed_out",
                Some("This budget belongs to another account.".into()),
            )
        } else if !enabled && entry.lifecycle == "active" {
            report_entry(&entry, "paused", None)
        } else {
            match process_budget(
                &transport, db, library, &account, &bootstrap, &key, &entry, open_clean, report,
                None,
            )
            .await
            {
                Ok(outcome) => outcome,
                Err(error) => {
                    report.state = "attention";
                    report_entry(
                        &entry,
                        if error.starts_with("OFFLINE:") {
                            "queued"
                        } else {
                            "error"
                        },
                        Some(error),
                    )
                }
            }
        };
        if let Some(error) = lifecycle_errors.as_object().into_iter().flat_map(|e| e.values()).find(|e| e["localBudgetId"] == entry.id) {
            outcome.state = "error";
            outcome.message = error["message"].as_str().map(str::to_owned);
        }
        if matches!(
            outcome.state,
            "conflict" | "recovery" | "error" | "subscription_required" | "deletion_conflict"
        ) {
            report.state = "attention";
        }
        library.lock().set_entry_status(&entry.id, outcome.state)?;
        report.budgets.push(outcome);
    }
    Ok(())
}

/// Legacy IDs may be associated only from the old device's explicit tracking ledger and a
/// verified persisted base in the same key generation. Display names never establish ancestry.
async fn adopt_legacy_bindings(
    transport: &Transport<'_>,
    db: &Database,
    library: &Library,
    account: &str,
    bootstrap: &Value,
) -> Result<()> {
    let state = library.lock().account_state(account)?;
    if state["legacyAdopted"] == true {
        return Ok(());
    }
    let ledger_path = library.lock().data_dir().join("sync-state.json");
    let ledger: Value = match std::fs::read(ledger_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(e) => return Err(e.to_string()),
    };
    if ledger["generation"]
        .as_u64()
        .map(|v| v.to_string())
        .as_deref()
        == bootstrap["keyGeneration"].as_str()
    {
        if let Some(tracked) = ledger["budgets"].as_object() {
            let snapshot = transport.catalog(bootstrap).await?;
            let heads: Vec<Head> =
                serde_json::from_value(snapshot["budgets"].clone()).map_err(|e| e.to_string())?;
            let entries = library.lock().list_all();
            for entry in entries
                .into_iter()
                .filter(|e| e.account_id.is_none() && !e.missing && e.lifecycle == "active")
            {
                let Some(track) = tracked.get(&entry.id) else {
                    continue;
                };
                let Some(head) = heads.iter().find(|h| h.budget_id == entry.id) else {
                    continue;
                };
                // A restored or rewritten v2 head cannot prove the legacy incarnation.
                if head
                    .payload
                    .as_ref()
                    .is_none_or(|p| p.format != "legacy-v1")
                {
                    continue;
                }
                let tracked_version = track["revision"].as_u64().unwrap_or(0);
                let ack = track["localRevision"].as_i64().unwrap_or(-1);
                let binding = library_api::with_store(db, library, &entry.id, |store| {
                    if store.sync_binding()?.is_some() {
                        return Ok(None);
                    }
                    let base = store.sync_base()?;
                    let Some((version, base)) = base.filter(|(version, _)| {
                        tracked_version > 0
                            && *version >= tracked_version
                            && *version <= head.version.parse::<u64>().unwrap_or(0)
                    }) else {
                        return Ok(None);
                    };
                    Ok(Some(Binding {
                        identity: identity(account, bootstrap, head)?,
                        remote_version: version.to_string(),
                        acknowledged_local_revision: if version == tracked_version {
                            ack
                        } else {
                            -1
                        },
                        base,
                    }))
                })?;
                if let Some(binding) = binding {
                    library_api::with_store(db, library, &entry.id, |s| s.bind_sync(&binding))?;
                    library.lock().associate(
                        &entry.id,
                        account,
                        &head.budget_id,
                        &head.incarnation,
                    )?;
                } else {
                    library.lock().set_entry_status(
                        &entry.id,
                        "Review this preserved legacy copy; cloud ancestry could not be verified.",
                    )?;
                }
            }
        }
    }
    let mut catalog = library.lock();
    let mut state = catalog.account_state(account)?;
    state["legacyAdopted"] = json!(true);
    catalog.set_account_state(account, &state)
}

/// Cancel a stale deletion and open its current incarnation for an explicit new decision.
/// Dirty saved content and raw editor checkpoints retain the normal merge/recovery guards.
pub async fn review_deletion(cloud: &Cloud, db: &Database, library: &Library, id: &str) -> Result<Value> {
    let _operation = cloud.sync.operation().await?;
    let (account, transport, bootstrap) = connect(cloud, library).await?;
    save_bootstrap(library, &account, &bootstrap)?;
    deliver(&transport, Some(db), library, &account).await?;
    let key = read_key(cloud, &account, &string(&bootstrap, "keyGeneration")?)
        .ok_or("Unlock Sync to review the newer budget before deleting it.")?;
    reconcile_catalog(&transport, library, &account, &bootstrap, Some(&key)).await?;
    let entry = library.lock().try_list_all()?.into_iter().find(|e| e.id == id)
        .ok_or("That budget no longer exists.")?;
    if entry.account_id.as_deref() != Some(&account) {
        return Err("Sign in to the account that owns this budget.".into());
    }
    let head = head_for(library, &account, entry.remote_id.as_deref().ok_or("This budget is local-only.")?)?
        .ok_or("The remote budget identity is unavailable. Preserve your local copy.")?;
    if head.state != "active" || entry.incarnation.as_deref() != Some(&head.incarnation)
        || bootstrap["keyGeneration"] != head.key_generation {
        return Err("This budget was removed or restored elsewhere. Use Restore or Recover as a copy to review it safely.".into());
    }
    if !entry.missing {
        let desired = identity(&account, &bootstrap, &head)?;
        library_api::with_store(db, library, id, |store| {
            if store.sync_binding()?.is_some_and(|b| b.identity != desired) {
                return Err("The budget's identity changed. Recover your local changes as a copy.".into());
            }
            store.set_sync_blocked(false)
        })?;
    }
    library.lock().review_deletion(id)?;
    let entry = library.lock().try_list_all()?.into_iter().find(|e| e.id == id)
        .ok_or("That budget no longer exists.")?;
    let mut report = super::sync::Report {
        at: now(), state: "ok", message: None, budgets: vec![], open_changed: false,
        changed_budgets: None, library_version: None,
    };
    let mut outcome = process_budget(&transport, db, library, &account, &bootstrap, &key, &entry, true, &mut report, None).await?;
    if matches!(outcome.state, "evicted" | "cloud_only") {
        // Explicit review, like Open, is permission to hydrate an evicted download.
        let mut download = entry.clone();
        download.availability = "remote".into();
        outcome = process_budget(&transport, db, library, &account, &bootstrap, &key, &download, true, &mut report, None).await?;
    }
    library.lock().set_entry_status(id, outcome.state)?;
    let active = library_api::open_local(db, library, id)?;
    Ok(json!({"library":library_api::snapshot(db,library)?,"active":active,"recoveredId":null}))
}
pub async fn open(cloud: &Cloud, db: &Database, library: &Library, id: &str) -> Result<Value> {
    ensure_download(cloud, db, library, id).await?;
    library_api::open_local(db, library, id)
}
/// Download without persisting a selection or replacing the window's open store.
pub async fn download(cloud: &Cloud, library: &Library, id: &str) -> Result<()> {
    ensure_download(cloud, &Database::new(None), library, id).await
}
async fn ensure_download(cloud: &Cloud, db: &Database, library: &Library, id: &str) -> Result<()> {
    let mut entry = library
        .lock()
        .list_all()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("That budget no longer exists.")?;
    if entry.lifecycle != "active" {
        return Err("Restore this budget or recover it as a copy before opening it.".into());
    }
    if entry.missing || entry.availability == "evicted" {
        let _operation = cloud.sync.operation().await?;
        let (account, transport, bootstrap) = connect(cloud, library).await?;
        if entry.account_id.as_deref() != Some(&account) {
            return Err("Sign in to the account that owns this budget.".into());
        }
        let key = read_key(cloud, &account, &string(&bootstrap, "keyGeneration")?)
            .ok_or("Enter this account's sync passphrase first.")?;
        reconcile_catalog(&transport, library, &account, &bootstrap, Some(&key)).await?;
        entry.availability = "remote".into();
        let mut report = super::sync::Report {
            at: now(),
            state: "ok",
            message: None,
            budgets: vec![],
            open_changed: false,
            changed_budgets: None,
            library_version: None,
        };
        let outcome = process_budget(
            &transport,
            db,
            library,
            &account,
            &bootstrap,
            &key,
            &entry,
            true,
            &mut report,
            None,
        )
        .await?;
        if !matches!(outcome.state, "downloaded" | "synced" | "uploaded") {
            return Err(outcome
                .message
                .unwrap_or("This budget is not available to open yet.".into()));
        }
    }
    Ok(())
}

fn validate_displayed_review(stored: &Value, displayed: &Value, choices: &crate::sync_merge::Choices) -> Result<()> {
    let stale = "The budget changed since these conflicts were reviewed. Sync again and review the current values.";
    if displayed["token"].as_str().is_none() || displayed["version"].as_str().is_none()
        || displayed["token"] != stored["token"] || displayed["version"] != stored["version"]
        || displayed["identity"] != stored["identity"] { return Err(stale.into()); }
    let shown = displayed["conflicts"].as_array().ok_or(stale)?;
    let current = stored["conflicts"].as_array().ok_or(stale)?;
    let mut keys = std::collections::BTreeSet::new();
    for conflict in shown {
        let key = conflict["key"].as_str().ok_or(stale)?;
        if !keys.insert(key) || conflict["fingerprint"].as_str().is_none()
            || !current.iter().any(|c| c["key"] == key && c["fingerprint"] == conflict["fingerprint"]) {
            return Err(stale.into());
        }
    }
    if shown.is_empty() || choices.iter().any(|(k,v)| !keys.contains(k.as_str()) || !matches!(v.as_str(), "local" | "cloud")) {
        return Err(stale.into());
    }
    Ok(())
}

pub async fn resolve(
    cloud: &Cloud,
    db: &Database,
    library: &Library,
    id: &str,
    keep: &str,
    choices: crate::sync_merge::Choices,
    open_clean: bool,
    displayed_review: Option<Value>,
) -> Result<super::sync::Report> {
    let operation = cloud.sync.operation().await?;
    let (account, transport, bootstrap) = connect(cloud, library).await?;
    deliver(&transport, Some(db), library, &account).await?;
    let key = read_key(cloud, &account, &string(&bootstrap, "keyGeneration")?)
        .ok_or("Enter this account's sync passphrase first.")?;
    reconcile_catalog(&transport, library, &account, &bootstrap, Some(&key)).await?;
    let entry = library
        .lock()
        .list_all()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("That budget no longer exists.")?;
    let review = library.lock().account_state(&account)?["reviews"][id].clone();
    let head = head_for(
        library,
        &account,
        entry
            .remote_id
            .as_deref()
            .ok_or("This budget is local-only.")?,
    )?
    .ok_or("The cloud budget is unavailable.")?;
    let displayed = displayed_review.ok_or("Review the current conflict values before choosing a resolution.")?;
    validate_displayed_review(&review, &displayed, &choices)?;
    let token = library_api::with_store(db, library, id, |s| Ok(s.status()?.token))?;
    if review["token"] != token
        || review["version"] != head.version
        || review["identity"]
            != serde_json::to_value(identity(&account, &bootstrap, &head)?)
                .map_err(|e| e.to_string())?
    {
        return Err("The budget changed since these conflicts were reviewed. Sync again and review the current values.".into());
    }
    let mut choices = choices;
    if matches!(keep, "local" | "cloud") {
        for conflict in displayed["conflicts"]
            .as_array()
            .ok_or("Review the current conflicts first.")?
        {
            if let Some(key) = conflict["key"].as_str() {
                choices.insert(key.into(), keep.into());
            }
        }
    } else if keep != "merge" {
        return Err("Choose a current conflict resolution.".into());
    }
    let reviewed: crate::sync_merge::ReviewedChoices = displayed["conflicts"]
        .as_array()
        .ok_or("Review the current conflicts first.")?
        .iter()
        .filter_map(|c| {
            let key = c["key"].as_str()?;
            let side = choices.get(key)?;
            let fingerprint = c["fingerprint"].as_str()?;
            Some((
                key.into(),
                crate::sync_merge::ReviewedChoice {
                    side: side.clone(),
                    fingerprint: fingerprint.into(),
                },
            ))
        })
        .collect();
    let mut report = super::sync::Report {
        at: now(),
        state: "ok",
        message: None,
        budgets: vec![],
        open_changed: false,
        changed_budgets: None,
        library_version: None,
    };
    let result = process_budget(
        &transport,
        db,
        library,
        &account,
        &bootstrap,
        &key,
        &entry,
        open_clean,
        &mut report,
        Some((&token, &reviewed)),
    )
    .await;
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => report_entry(&entry, "error", Some(error)),
    };
    report.budgets.push(outcome);
    let changed = report.open_changed;
    drop(operation);
    let mut next = run(cloud, db, library, open_clean).await?;
    next.open_changed |= changed;
    // The second pass cannot erase a committed merge's invalidation or its review notes.
    for prior in report.budgets {
        if let Some(entry) = next.budgets.iter_mut().find(|e| e.id == prior.id) {
            if prior.state != "error" && entry.state == "synced" {
                *entry = prior;
            }
        }
    }
    Ok(cloud.sync.finish(next))
}

#[cfg(test)]
mod deletion_review_tests {
    use super::*;
    #[test]
    fn displayed_conflicts_cannot_be_replaced_or_answer_unseen_keys() {
        let shown=json!({"token":"saved-1","version":"2","identity":{"accountId":"a"},"conflicts":[{"key":"amount","fingerprint":"before"}]});
        assert!(validate_displayed_review(&shown,&shown,&Default::default()).is_ok());
        let mut current=shown.clone(); current["version"]=json!("3"); current["conflicts"][0]["fingerprint"]=json!("after");
        assert!(validate_displayed_review(&current,&shown,&Default::default()).is_err());
        let mut choices=crate::sync_merge::Choices::new(); choices.insert("unseen".into(),"local".into());
        assert!(validate_displayed_review(&shown,&shown,&choices).is_err());
        let mut forged=shown.clone();forged["conflicts"][0]["fingerprint"]=json!("forged");
        assert!(validate_displayed_review(&shown,&forged,&Default::default()).is_err());
    }
    #[test]
    fn oversized_candidates_are_rejected_before_any_sequence_is_allocated() {
        assert!(check_upload_size(MAX_CIPHERTEXT).is_ok());
        assert!(check_upload_size(MAX_CIPHERTEXT+1).unwrap_err().contains("PAYLOAD_TOO_LARGE"));
    }

    #[test]
    fn remote_tombstones_and_restores_cannot_manufacture_local_delete_intent() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library(std::sync::Mutex::new(budgets::Budgets::load(dir.path()).unwrap()));
        let id = library.lock().observe_catalog(&budgets::CatalogHead {
            account_id:"account".into(), key_generation:"1".into(), catalog_epoch:"epoch".into(),
            budget_id:"remote".into(), incarnation:"original".into(), version:"2".into(), state:"trashed".into(),
            name:"Remote budget".into(), currency:"SAR".into(), metadata:json!({}),
        }).unwrap();
        let entry = library.lock().try_list_all().unwrap().remove(0);
        let mut head = Head {
            budget_id:"remote".into(), incarnation:"original".into(), version:"3".into(), key_generation:"1".into(),
            state:"active".into(), payload:None, updated_sequence:"3".into(),updated_at:now(),deleted_at:None,retained_until:None,device:"peer".into(),
        };
        assert!(interrupted_deletion_review(&library,&entry,&head).unwrap().is_none(), "observed remote tombstones are not local delete intents");
        let job = budgets::LifecycleJob {
            operation_id:"local-trash".into(),local_budget_id:Some(id),kind:"trash".into(),
            payload:json!({"accountId":"account"}),state:"pending".into(),receipt:None,
        };
        library.lock().enqueue_job(&job).unwrap();
        library.lock().finish_job(&job.operation_id,&json!({"outcome":"rejected","code":"conflict"})).unwrap();
        assert_eq!(interrupted_deletion_review(&library,&entry,&head).unwrap().unwrap().operation_id,job.operation_id,
            "a crash after rejected local CAS still restores its review marker");
        let conflict = budgets::LifecycleJob {
            kind:"deletion_conflict".into(), payload:json!({"newHead":head}), ..job.clone()
        };
        assert!(deletion_conflict_applies(&conflict,&head));
        head.incarnation="restored".into();
        assert!(!deletion_conflict_applies(&conflict,&head), "a persisted review marker cannot block a later restore");
        assert!(interrupted_deletion_review(&library,&entry,&head).unwrap().is_none(),
            "an older local delete cannot block a later restored incarnation");
    }
}