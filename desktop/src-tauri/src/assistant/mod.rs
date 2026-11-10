//! Tauri adapters for the host-neutral assistant shared with native Android.
pub use urbudget_services::assistant::{Assistant, attachments, history, llm, mcp, prompt, proposals, tools, workbook, new_id, now, money, parse_money, open_browser};
use urbudget_services::assistant as shared;
use crate::{Database, patch::Patch, save};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use serde_json::Value;
use shared::{SettingsChange, StatusView, Applied, ReportView, ImportView};
use llm::Provider;
use proposals::Status;
use std::path::PathBuf;

#[tauri::command(async)]
pub(crate) fn assistant_status(assistant: State<'_, Assistant>) -> StatusView {
    shared::assistant_status(&assistant)
}

#[tauri::command]
pub(crate) async fn assistant_sign_in(assistant: State<'_, Assistant>) -> Result<StatusView, String> {
    shared::assistant_sign_in(&assistant).await
}

#[tauri::command(async)]
pub(crate) fn assistant_cancel_sign_in(assistant: State<'_, Assistant>) {
    shared::assistant_cancel_sign_in(&assistant)
}

#[tauri::command]
pub(crate) async fn assistant_sign_out(assistant: State<'_, Assistant>) -> Result<StatusView, String> {
    shared::assistant_sign_out(&assistant).await
}

#[tauri::command(async)]
pub(crate) fn assistant_settings(assistant: State<'_, Assistant>, change: SettingsChange) -> Result<StatusView, String> {
    shared::assistant_settings(&assistant, change)
}

#[tauri::command]
pub(crate) async fn assistant_set_key(assistant: State<'_, Assistant>, provider: Provider, key: String) -> Result<StatusView, String> {
    shared::assistant_set_key(&assistant, provider, key).await
}

#[tauri::command(async)]
pub(crate) fn assistant_forget_key(assistant: State<'_, Assistant>, provider: Provider) -> Result<StatusView, String> {
    shared::assistant_forget_key(&assistant, provider)
}

#[tauri::command]
pub(crate) async fn assistant_models(assistant: State<'_, Assistant>, refresh: bool) -> Result<Vec<llm::Model>, String> {
    shared::assistant_models(&assistant, refresh).await
}

#[tauri::command(async)]
pub(crate) fn assistant_threads(db: State<'_, Database>, assistant: State<'_, Assistant>) -> Result<Vec<history::Listing>, String> {
    shared::assistant_threads(&db, &assistant)
}

#[tauri::command(async)]
pub(crate) fn assistant_thread(db: State<'_, Database>, assistant: State<'_, Assistant>, id: String, patch: Option<Patch>) -> Result<Value, String> {
    shared::assistant_thread(&db, &assistant, id, patch)
}

#[tauri::command(async)]
pub(crate) fn assistant_rename_thread(db: State<'_, Database>, assistant: State<'_, Assistant>, id: String, title: String) -> Result<(), String> {
    shared::assistant_rename_thread(&db, &assistant, id, title)
}

#[tauri::command(async)]
pub(crate) fn assistant_continue(db: State<'_, Database>, assistant: State<'_, Assistant>, thread_id: String) -> Result<String, String> {
    shared::assistant_continue(&db, &assistant, thread_id)
}

#[tauri::command(async)]
pub(crate) fn assistant_rate(db: State<'_, Database>, assistant: State<'_, Assistant>, thread_id: String, index: usize, up: Option<bool>) -> Result<(), String> {
    shared::assistant_rate(&db, &assistant, thread_id, index, up)
}

/// The change that makes the budget's notes for the assistant exactly `notes`.
#[tauri::command(async)]
pub(crate) fn assistant_notes(db: State<'_, Database>, patch: Option<Patch>, notes: Vec<String>) -> Result<Patch, String> {
    shared::assistant_notes(&db, patch, notes)
}

#[tauri::command(async)]
pub(crate) fn assistant_delete_thread(db: State<'_, Database>, assistant: State<'_, Assistant>, id: String) -> Result<(), String> {
    shared::assistant_delete_thread(&db, &assistant, id)
}

#[tauri::command(async)]
pub(crate) fn assistant_delete_all(db: State<'_, Database>, assistant: State<'_, Assistant>) -> Result<(), String> {
    shared::assistant_delete_all(&db, &assistant)
}

#[tauri::command(async)]
pub(crate) fn assistant_attach(
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    name: String,
    data: String,
) -> Result<attachments::Meta, String> {
    shared::assistant_attach(&db, &assistant, name, data)
}

#[tauri::command(async)]
pub(crate) fn assistant_detach(db: State<'_, Database>, id: String) -> Result<(), String> {
    shared::assistant_detach(&db, id)
}

#[tauri::command(async)]
pub(crate) fn assistant_send(
    app: AppHandle,
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    patch: Option<Patch>,
    thread_id: Option<String>,
    text: String,
    attachments: Vec<String>,
    screen: prompt::Screen,
) -> Result<String, String> {
    shared::assistant_send(&app.state::<crate::simplefin::SimpleFin>(), &db, &assistant, patch, thread_id, text, attachments, screen)
}

#[tauri::command(async)]
pub(crate) fn assistant_retry(
    app: AppHandle,
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    patch: Option<Patch>,
    thread_id: String,
    screen: prompt::Screen,
) -> Result<(), String> {
    shared::assistant_retry(&app.state::<crate::simplefin::SimpleFin>(), &db, &assistant, patch, thread_id, screen)
}

#[tauri::command(async)]
pub(crate) fn assistant_report(
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    thread_id: String,
    proposal_id: String,
) -> Result<ReportView, String> {
    shared::assistant_report(&db, &assistant, thread_id, proposal_id)
}

#[tauri::command(async)]
pub(crate) fn assistant_log_error(assistant: State<'_, Assistant>, message: String) -> Result<(), String> {
    shared::assistant_log_error(&assistant, message)
}

#[tauri::command(async)]
pub(crate) fn assistant_open(link: String) -> Result<(), String> {
    shared::assistant_open(link)
}

#[tauri::command]
pub(crate) async fn assistant_report_reply(cloud: State<'_, crate::cloud::Cloud>, text: String, reason: String) -> Result<String, String> {
    shared::assistant_report_reply(&cloud, text, reason).await
}

#[tauri::command(async)]
pub(crate) fn assistant_stop(db: State<'_, Database>, assistant: State<'_, Assistant>, thread_id: String) -> Result<(), String> {
    shared::assistant_stop(&db, &assistant, thread_id)
}

#[tauri::command(async)]
pub(crate) fn assistant_apply(
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    patch: Option<Patch>,
    thread_id: String,
    proposal_id: String,
    settle: Option<bool>,
) -> Result<Applied, String> {
    shared::assistant_apply(&db, &assistant, patch, thread_id, proposal_id, settle.unwrap_or(true))
}

/// The proposal as the user chose on its card, made again on the window's budget.
#[tauri::command(async)]
pub(crate) fn assistant_revise(
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    patch: Option<Patch>,
    thread_id: String,
    proposal_id: String,
    revision: proposals::Revision,
) -> Result<Value, String> {
    shared::assistant_revise(&db, &assistant, patch, thread_id, proposal_id, revision)
}

#[tauri::command(async)]
pub(crate) fn assistant_mark(
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    thread_id: String,
    proposal_id: String,
    status: Status,
    note: Option<String>,
) -> Result<Value, String> {
    shared::assistant_mark(&db, &assistant, thread_id, proposal_id, status, note)
}

#[tauri::command(async)]
pub(crate) fn assistant_import(db: State<'_, Database>, assistant: State<'_, Assistant>, thread_id: String, proposal_id: String) -> Result<ImportView, String> {
    shared::assistant_import(&db, &assistant, thread_id, proposal_id)
}

/// Saves the converted rows where you choose; `None` when cancelled.
#[tauri::command(async)]
pub(crate) fn assistant_save_import(window: WebviewWindow, db: State<'_, Database>, assistant: State<'_, Assistant>, thread_id: String, proposal_id: String) -> Result<Option<String>, String> {
    let (name, csv) = shared::import_file(&db, &assistant, &thread_id, &proposal_id)?;
    save::file(&window, &name, save::Kind::Csv, csv.as_bytes())
}

/// With `open`, opens the card's spreadsheet (a copy in the budget's folder); otherwise saves it
/// where you choose, `None` when cancelled.
#[tauri::command(async)]
pub(crate) fn assistant_spreadsheet(
    window: WebviewWindow,
    db: State<'_, Database>,
    assistant: State<'_, Assistant>,
    patch: Option<Patch>,
    thread_id: String,
    proposal_id: String,
    open: bool,
) -> Result<Option<String>, String> {
    if open {
        return shared::assistant_spreadsheet(&db, &assistant, patch, thread_id, proposal_id, true).map(Some);
    }
    let (title, bytes) = shared::spreadsheet_file(&db, &assistant, patch, &thread_id, &proposal_id)?;
    let Some(path) = save::file(&window, &workbook::file_name(&title), save::Kind::Workbook, &bytes)? else {
        return Ok(None);
    };
    shared::spreadsheet_saved(&db, &assistant, &thread_id, &proposal_id, &path)?;
    Ok(Some(path))
}

#[tauri::command(async)]
pub(crate) fn assistant_print(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| format!("Couldn't open the print dialog: {e}"))
}
pub fn manage(app: &tauri::App, data_dir: PathBuf) {
    let handle = app.handle().clone();
    urbudget_services::host::set_listener(move |event, payload| {
        let _ = handle.emit(event, payload);
    });
    app.manage(Assistant::new(data_dir));
}
