//! Tauri adapters for the shared native services.
use tauri::State;
use serde_json::Value;
pub use urbudget_services::cloud::sync::{Report, Status};
use crate::{Database, Library};
use crate::cloud::Cloud;
use crate::sync_merge::Choices;

#[tauri::command]
pub fn sync_status(cloud: State<'_, Cloud>) -> Status {
    urbudget_services::cloud::sync::sync_status(&cloud)
}

#[tauri::command(async)]
pub async fn sync_now(cloud: State<'_, Cloud>, db: State<'_, Database>, library: State<'_, Library>, open_clean: bool) -> Result<Report, String> {
    urbudget_services::cloud::sync::sync_now(&cloud, &db, &library, open_clean).await
}

#[tauri::command(async)]
pub async fn sync_has_key(cloud: State<'_, Cloud>) -> Result<bool, String> {
    urbudget_services::cloud::sync::sync_has_key(&cloud).await
}

/// What the account's sync key is made from and whether it has one, asked fresh.
#[tauri::command(async)]
pub async fn sync_key_info(cloud: State<'_, Cloud>, library: State<'_, Library>) -> Result<Value, String> {
    urbudget_services::cloud::sync::sync_key_info(&cloud, &library).await
}

/// Without a passphrase, sets sync up with a recovery code made here and answers with it.
#[tauri::command(async)]
pub async fn sync_setup(cloud: State<'_, Cloud>, library: State<'_, Library>, passphrase: Option<String>) -> Result<Value, String> {
    urbudget_services::cloud::sync::sync_setup(&cloud, &library, passphrase).await
}

/// Unlocks sync with whatever was typed: a code another device shows, the recovery code, or
/// the passphrase.
#[tauri::command(async)]
pub async fn sync_unlock(cloud: State<'_, Cloud>, library: State<'_, Library>, secret: String) -> Result<Value, String> {
    urbudget_services::cloud::sync::sync_unlock(&cloud, &library, secret).await
}

#[tauri::command(async)]
pub async fn sync_recovery_create(cloud: State<'_, Cloud>, library: State<'_, Library>) -> Result<Value, String> {
    urbudget_services::cloud::recovery::create(&cloud, &library).await
}

#[tauri::command]
pub fn sync_recovery_unsaved(cloud: State<'_, Cloud>, library: State<'_, Library>) -> Result<Value, String> {
    urbudget_services::cloud::recovery::unsaved_code(&cloud, &library)
}

#[tauri::command]
pub fn sync_recovery_saved(cloud: State<'_, Cloud>) -> Result<(), String> {
    urbudget_services::cloud::recovery::saved(&cloud)
}

#[tauri::command(async)]
pub async fn sync_pair_offer(cloud: State<'_, Cloud>, library: State<'_, Library>) -> Result<Value, String> {
    urbudget_services::cloud::pairing::offer(&cloud, &library).await
}

#[tauri::command(async)]
pub async fn sync_pair_state(cloud: State<'_, Cloud>, id: String) -> Result<Value, String> {
    urbudget_services::cloud::pairing::state(&cloud, &id).await
}

#[tauri::command(async)]
pub async fn sync_pair_cancel(cloud: State<'_, Cloud>, id: String) -> Result<(), String> {
    urbudget_services::cloud::pairing::cancel(&cloud, &id).await
}

#[tauri::command]
pub fn sync_pair_request(cloud: State<'_, Cloud>) -> Result<Value, String> {
    urbudget_services::cloud::pairing::request(&cloud)
}

#[tauri::command(async)]
pub async fn sync_pair_wait(cloud: State<'_, Cloud>, library: State<'_, Library>, code: String) -> Result<Value, String> {
    urbudget_services::cloud::pairing::wait(&cloud, &library, &code).await
}

#[tauri::command(async)]
pub async fn sync_pair_approve(cloud: State<'_, Cloud>, library: State<'_, Library>, code: String) -> Result<(), String> {
    urbudget_services::cloud::pairing::approve(&cloud, &library, &code).await
}

/// Moves an account that still syncs on protocol 1 to the new sync; `passphrase` only when
/// this computer has no saved key that opens the account (`Status::upgrade_needs_passphrase`).
#[tauri::command(async)]
pub async fn sync_upgrade(cloud: State<'_, Cloud>, library: State<'_, Library>, passphrase: Option<String>) -> Result<Value, String> {
    urbudget_services::cloud::sync::sync_upgrade(&cloud, &library, passphrase).await
}

/// A new key, from a passphrase or, without one, from a recovery code made here.
#[tauri::command(async)]
pub async fn sync_reset(cloud: State<'_, Cloud>, library: State<'_, Library>, passphrase: Option<String>) -> Result<Value, String> {
    urbudget_services::cloud::sync::sync_reset(&cloud, &library, passphrase).await
}

#[tauri::command(async)]
pub async fn sync_track(cloud: State<'_, Cloud>, ids: Vec<String>) -> Result<(), String> {
    urbudget_services::cloud::sync::sync_track(&cloud, ids).await
}

#[tauri::command(async)]
pub async fn sync_download(cloud: State<'_, Cloud>, library: State<'_, Library>, id: String) -> Result<String, String> {
    urbudget_services::cloud::sync::sync_download(&cloud, &library, id).await
}

#[tauri::command(async)]
pub async fn sync_resolve(
    cloud: State<'_, Cloud>,
    db: State<'_, Database>,
    library: State<'_, Library>,
    id: String,
    keep: String,
    choices: Option<Choices>,
    open_clean: bool,
    review: Option<serde_json::Value>,
) -> Result<Report, String> {
    urbudget_services::cloud::sync::sync_resolve(&cloud, &db, &library, id, keep, choices, open_clean, review).await
}

#[tauri::command(async)]
pub async fn sync_remove(cloud: State<'_, Cloud>, id: String) -> Result<(), String> {
    urbudget_services::cloud::sync::sync_remove(&cloud, id).await
}

#[tauri::command(async)]
pub async fn sync_lock(cloud: State<'_, Cloud>) -> Result<(), String> {
    urbudget_services::cloud::sync::sync_lock(&cloud).await
}
