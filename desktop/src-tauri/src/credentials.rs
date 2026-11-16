//! Desktop commands share token storage, migration, redaction and ordering with Android.
pub use urbudget_services::credentials::SavedToken;
#[tauri::command]
pub fn ynab_tokens()->Result<Vec<SavedToken>,String>{urbudget_services::credentials::ynab_tokens()}
#[tauri::command]
pub fn forget_ynab_token(id:String)->Result<(),String>{urbudget_services::credentials::forget_ynab_token(id)}
/// What protects saved secrets here, or None where they aren't kept. Off the main thread: on
/// Linux the first call asks the session bus whether a keyring is running.
#[tauri::command(async)]
pub fn secret_store()->Option<&'static str>{urbudget_services::vault::store()}
