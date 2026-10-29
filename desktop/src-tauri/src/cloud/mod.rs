//! Tauri adapters for the shared native services.
use tauri::State;
use serde_json::Value;
pub use urbudget_services::cloud::{Cloud, Status, site};
pub mod sync;

#[tauri::command(async)]
pub fn cloud_status(cloud: State<'_, Cloud>, library: State<'_, crate::Library>) -> Status {
    cloud.status_with_library(&library)
}

#[tauri::command(async)]
pub async fn account_overview(cloud: State<'_, Cloud>) -> Result<Value, String> {
    urbudget_services::cloud::account_overview(&cloud).await
}

#[tauri::command(async)]
pub async fn account_start(cloud: State<'_, Cloud>, email: String) -> Result<Value, String> {
    urbudget_services::cloud::account_start(&cloud, email).await
}

#[tauri::command(async)]
pub async fn account_verify(cloud: State<'_, Cloud>, email: String, code: String) -> Result<Status, String> {
    urbudget_services::cloud::account_verify(&cloud, email, code).await
}

#[tauri::command(async)]
pub async fn account_sign_out(cloud: State<'_, Cloud>) -> Result<Status, String> {
    urbudget_services::cloud::account_sign_out(&cloud).await
}

#[tauri::command(async)]
pub async fn account_open(cloud: State<'_, Cloud>, next: String) -> Result<(), String> {
    urbudget_services::cloud::account_open(&cloud, next).await
}

#[tauri::command(async)]
pub async fn account_delete(cloud: State<'_, Cloud>, confirm: String) -> Result<Status, String> {
    urbudget_services::cloud::account_delete(&cloud, confirm).await
}
