//! Desktop bindings for the shared Interactive Brokers connection (urbudget-services ibkr.rs).
use crate::{bank_import, patch::Patch, Database};
use tauri::State;
use urbudget_services::ibkr as service;
pub use urbudget_services::ibkr::{Ibkr, Link, Review, Status};
#[tauri::command]
pub fn ibkr_status(state: State<'_, Ibkr>) -> Status { service::ibkr_status(&state) }
#[tauri::command(async)]
pub async fn ibkr_connect(state: State<'_, Ibkr>, token: String, query: String) -> Result<Status, String> { service::ibkr_connect(&state, token, query).await }
#[tauri::command(async)]
pub async fn ibkr_fetch(state: State<'_, Ibkr>) -> Result<Status, String> { service::ibkr_fetch(&state).await }
#[tauri::command(async)]
pub fn ibkr_file(state: State<'_, Ibkr>, content: String) -> Result<Status, String> { service::ibkr_file(&state, content) }
#[tauri::command]
pub fn ibkr_disconnect(state: State<'_, Ibkr>, query: Option<String>) -> Result<Status, String> { service::ibkr_disconnect(&state, query) }
#[tauri::command(async)]
pub fn ibkr_apply(db: State<'_, Database>, state: State<'_, Ibkr>, patch: Option<Patch>, links: Vec<Link>) -> Result<Patch, String> { service::ibkr_apply(&db, &state, patch, links) }
#[tauri::command(async)]
pub fn ibkr_review(db: State<'_, Database>, state: State<'_, Ibkr>, patch: Option<Patch>) -> Result<Review, String> { service::ibkr_review(&db, &state, patch) }
#[tauri::command(async)]
pub fn ibkr_import(db: State<'_, Database>, patch: Option<Patch>, account: String, choices: Vec<bank_import::Choice>) -> Result<Patch, String> { service::ibkr_import(&db, patch, account, choices) }
#[tauri::command(async)]
pub fn ibkr_open() -> Result<(), String> { service::ibkr_open() }
