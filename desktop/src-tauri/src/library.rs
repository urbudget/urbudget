//! Thin desktop adapters for the shared library lifecycle.
use crate::{cloud::Cloud, Database, Library};
use serde_json::Value;
use tauri::State;
use urbudget_services::library_api;

#[tauri::command(async)]
pub fn library_snapshot(
    db: State<'_, Database>,
    library: State<'_, Library>,
) -> Result<Value, String> {
    library_api::snapshot(&db, &library)
}
#[tauri::command(async)]
pub async fn library_open(
    cloud: State<'_, Cloud>,
    db: State<'_, Database>,
    library: State<'_, Library>,
    id: String,
) -> Result<Value, String> {
    urbudget_services::cloud::v2::open(&cloud, &db, &library, &id).await
}
#[tauri::command(async)]
pub async fn library_trash(
    cloud: State<'_, Cloud>,
    db: State<'_, Database>,
    library: State<'_, Library>,
    id: String,
) -> Result<Value, String> {
    library_api::lifecycle(&cloud, &db, &library, "trash", &id).await
}
#[tauri::command(async)]
pub async fn library_restore(
    cloud: State<'_, Cloud>,
    db: State<'_, Database>,
    library: State<'_, Library>,
    id: String,
) -> Result<Value, String> {
    library_api::lifecycle(&cloud, &db, &library, "restore", &id).await
}
#[tauri::command(async)]
pub async fn library_evict(
    cloud: State<'_, Cloud>,
    db: State<'_, Database>,
    library: State<'_, Library>,
    id: String,
) -> Result<Value, String> {
    library_api::lifecycle(&cloud, &db, &library, "evict", &id).await
}
#[tauri::command(async)]
pub async fn library_recover(
    cloud: State<'_, Cloud>,
    db: State<'_, Database>,
    library: State<'_, Library>,
    id: String,
) -> Result<Value, String> {
    library_api::lifecycle(&cloud, &db, &library, "recover", &id).await
}
#[tauri::command(async)]
pub fn sync_enable(
    cloud: State<'_, Cloud>,
    library: State<'_, Library>,
    enabled: bool,
    ids: Option<Vec<String>>,
) -> Result<urbudget_services::cloud::Status, String> {
    urbudget_services::cloud::v2::enable(&cloud, &library, enabled, &ids.unwrap_or_default())
}
#[tauri::command(async)]
pub fn editor_checkpoint(
    db: State<'_, Database>,
    library: State<'_, Library>,
    budget_id: String,
    id: String,
    epoch: String,
    target: Option<Value>,
    baseline: Value,
    values: Value,
) -> Result<(), String> {
    library_api::checkpoint(
        &db,
        &library,
        &budget_id,
        id,
        epoch,
        target.unwrap_or(Value::Null),
        baseline,
        values,
    )
}
#[tauri::command(async)]
pub fn editor_discard(
    db: State<'_, Database>,
    library: State<'_, Library>,
    budget_id: String,
    id: String,
    epoch: String,
    only_if_active: Option<bool>,
) -> Result<(), String> {
    if only_if_active.unwrap_or(false) {
        library_api::discard_checkpoint_if_active(&db, &library, &budget_id, &id, &epoch)
    } else {
        library_api::discard_checkpoint(&db, &library, &budget_id, &id, &epoch)
    }
}
#[tauri::command(async)]
pub fn editor_recovery(
    db: State<'_, Database>,
    library: State<'_, Library>,
    budget_id: Option<String>,
) -> Result<Value, String> {
    let id = budget_id.unwrap_or_else(|| library.lock().current().into());
    library_api::editor_recovery(&db, &library, &id)
}

#[tauri::command(async)]
pub async fn library_review_deletion(
    cloud: State<'_, Cloud>,
    db: State<'_, Database>,
    library: State<'_, Library>,
    id: String,
) -> Result<Value, String> {
    urbudget_services::cloud::v2::review_deletion(&cloud, &db, &library, &id).await
}