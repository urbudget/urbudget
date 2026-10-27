// Desktop commands use the same accounting implementation as the native phone apps.
pub use urbudget_core::{domain, engine, ledger, targets, schedules, workflows, reports, loans, patch, store, sync_merge, bank_csv, bank_import, bank_statement, ynab, ynab_files, fresh_start, import_sync, budgets};
pub mod assistant;
mod cloud;
mod credentials;
mod save;
mod simplefin;
mod updates;
mod vault;
mod ynab_capture;
use domain::{Budget, Summary};
use patch::Patch;
use std::sync::Mutex;
use store::{Applied, Snapshot, Status, Store};
use tauri::{Manager, State, WebviewWindow};

// The window keeps a copy of the budget and sends only what changed. Commands that
// read the budget take an optional `patch`: the window's version relative to the
// draft held here (empty when they match). Commands that change it return a patch.
pub use urbudget_services::{Database, Library};

/// The language of the engine's messages from now on, "ar" or "en" (docs/LOCALIZATION.md).
#[tauri::command]
fn set_language(language: String) {
    urbudget_services::i18n::set_language(&language);
}

#[tauri::command(async)]
fn load_budget(db: State<'_, Database>, legacy_backup_keep: Option<usize>) -> Result<Snapshot, String> {
    let mut store = db.store()?;
    store.initialize_backup_retention(legacy_backup_keep.unwrap_or(0))?;
    store.load_light()
}
#[tauri::command(async)]
fn apply_patch(db: State<'_, Database>, patch: Patch) -> Result<Applied, String> {
    db.store()?.apply_patch(patch)
}
#[tauri::command(async)]
fn replace_draft(db: State<'_, Database>, budget: Budget) -> Result<Applied, String> {
    db.store()?.replace_draft(budget)
}
#[tauri::command(async)]
fn commit_budget(db: State<'_, Database>, expected_token: String) -> Result<Status, String> {
    db.store()?.commit(&expected_token)
}
#[tauri::command(async)]
fn database_status(db: State<'_, Database>) -> Result<Status, String> {
    db.store()?.status()
}
#[tauri::command(async)]
fn calculate_budget(db: State<'_, Database>, patch: Option<Patch>, month: String, as_of_month: Option<String>) -> Result<Summary, String> {
    let mut b = db.snapshot(patch)?;
    calculation_context(&mut b, as_of_month.as_deref())?;
    domain::calculate(&b, &month)
}

// The window's reference month is ephemeral calculation context, never a saved budget edit.
fn calculation_context(b: &mut Budget, as_of_month: Option<&str>) -> Result<(), String> {
    if let Some(month) = as_of_month {
        if !domain::valid_month(month) {
            return Err("Invalid calculation reference month.".into());
        }
        b.extra.insert("calculationAsOfMonth".into(), serde_json::json!(month));
    }
    Ok(())
}
#[tauri::command(async)]
fn validate_budget(db: State<'_, Database>, patch: Option<Patch>) -> Result<(), String> {
    db.read(patch, domain::validate)
}
#[tauri::command(async)]
fn backup_database(db: State<'_, Database>) -> Result<String, String> {
    db.store()?.backup()
}
#[tauri::command(async)]
fn backup_retention(db: State<'_, Database>) -> Result<usize, String> {
    db.store()?.backup_retention()
}
#[tauri::command(async)]
fn set_backup_retention(db: State<'_, Database>, keep: usize) -> Result<(), String> {
    db.store()?.set_backup_retention(keep)
}
#[tauri::command(async)]
fn restore_budget(db: State<'_, Database>, budget: Budget, expected_token: String) -> Result<Snapshot, String> {
    db.store()?.restore_backup(&budget, &expected_token)
}
#[tauri::command(async)]
fn prune_backups(db: State<'_, Database>, keep: usize) -> Result<usize, String> {
    db.store()?.prune_backups(keep)
}
/// Saves the budget as JSON where you choose and returns the path (`None` when cancelled). The
/// budget is read before the panel opens, so it doesn't hold up saving while you choose.
#[tauri::command(async)]
fn export_budget(window: WebviewWindow, db: State<'_, Database>, patch: Option<Patch>, recovery: Option<bool>) -> Result<Option<String>, String> {
    let (name, data) = {
        let budget = db.store()?.full_draft(patch)?;
        if recovery == Some(true) {
            (save::dated(&format!("{} recovery draft", budget.name), "json"), store::export_recovery_draft(&budget)?)
        } else {
            (save::dated(&budget.name, "json"), store::export(&budget)?)
        }
    };
    save::file(&window, &name, save::Kind::Json, &data)
}
#[tauri::command(async)]
fn reconcile_budget(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account_id: String,
    balance: i64,
    date: String,
    adjustment_id: String,
) -> Result<Patch, String> {
    db.change(patch, |b| {
        domain::reconcile(b, &account_id, balance, &date, &adjustment_id)
    })
}

#[tauri::command(async)]
fn import_ynab_export(export_data: serde_json::Value) -> Result<Budget, String> {
    ynab::import_export(&export_data)
}
/// A new budget from YNAB's own export files (its .zip, or the Register and Plan CSVs), with
/// no API access. The window saves it as a separate budget.
#[tauri::command(async)]
fn import_ynab_files(
    files: Vec<ynab_files::ExportFile>,
    date_order: Option<String>,
    currency: String,
) -> Result<Budget, String> {
    ynab_files::import_files(&files, date_order.as_deref(), &currency)
}
#[tauri::command(async)]
fn prepare_captured_ynab(db: State<'_, Database>, path: String) -> Result<Budget, String> {
    let directory = db
        .store()?
        .folder()
        .join("imports")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = std::path::PathBuf::from(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(directory) {
        return Err(urbudget_services::t!("Choose a downloaded YNAB export."));
    }
    ynab::import_export(
        &serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    )
}
/// Saves YNAB's Register and Plan CSVs, with the full JSON, in a folder you name and returns its
/// path (`None` when cancelled).
#[tauri::command(async)]
fn export_ynab_csv(window: WebviewWindow, db: State<'_, Database>, patch: Option<Patch>) -> Result<Option<String>, String> {
    let (name, files) = {
        let budget = db.store()?.full_draft(patch)?;
        (save::dated(&format!("{} for YNAB", budget.name), ""), store::export_ynab(&budget)?)
    };
    let Some(directory) = save::ask(&window, &name, save::Kind::Folder) else {
        return Ok(None);
    };
    std::fs::create_dir_all(&directory).map_err(|e| urbudget_services::t!("Couldn't make the folder {folder}: {e}", folder = directory.display(), e))?;
    for (file, contents) in files {
        save::write(&directory.join(file), &contents)?;
    }
    Ok(Some(save::saved(&window, &directory)))
}
/// Saves one account's transactions (or the ones listed) as a YNAB-style register CSV, newest
/// first, where you choose, and returns its path (`None` when cancelled).
#[tauri::command(async)]
fn export_transactions(
    window: WebviewWindow,
    db: State<'_, Database>,
    patch: Option<Patch>,
    account_id: Option<String>,
    ids: Option<Vec<String>>,
) -> Result<Option<String>, String> {
    let budget = db.store()?.full_draft(patch)?;
    let ids: Option<std::collections::HashSet<String>> = ids.map(|v| v.into_iter().collect());
    let data = ynab::export_register_for(
        &budget,
        |account, id| {
            account_id.as_deref().is_none_or(|a| a == account)
                && ids.as_ref().is_none_or(|set| set.contains(id))
        },
        true,
    )?;
    let name = account_id
        .as_ref()
        .and_then(|id| budget.accounts.iter().find(|a| &a.id == id))
        .map_or("All accounts", |a| a.name.as_str());
    save::file(&window, &save::dated(name, "csv"), save::Kind::Csv, data.as_bytes())
}
/// Saves a small CSV the app made (such as the bank-file template) where you choose, starting
/// with `name`, and returns its path (`None` when cancelled).
#[tauri::command(async)]
fn write_export(window: WebviewWindow, name: String, contents: String) -> Result<Option<String>, String> {
    let plain = name.len() <= 80
        && name.ends_with(".csv")
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || " ._-".contains(c));
    if !plain {
        return Err(urbudget_services::t!("Choose a plain .csv file name."));
    }
    save::file(&window, &name, save::Kind::Csv, contents.as_bytes())
}
#[tauri::command(async)]
fn read_recovery(db: State<'_, Database>) -> Result<Option<serde_json::Value>, String> {
    db.store()?.read_recovery()
}
#[tauri::command(async)]
fn discard_recovery(db: State<'_, Database>) -> Result<(), String> {
    db.store()?.discard_recovery()
}

#[tauri::command(async)]
fn preview_schedules(
    db: State<'_, Database>,
    patch: Option<Patch>,
    through: String,
    selected: Option<String>,
    skip: bool,
) -> Result<Patch, String> {
    db.change(patch, |mut b| {
        schedules::apply(&mut b, &through, selected.as_deref(), skip)?;
        Ok(b)
    })
}
#[tauri::command(async)]
fn run_schedules(
    db: State<'_, Database>,
    expected_token: String,
    through: String,
) -> Result<Snapshot, String> {
    db.store()?.run_schedules(&expected_token, &through)
}
#[tauri::command(async)]
fn project_loan(
    balance: i64,
    terms: loans::Terms,
    start: String,
    extra: i64,
    lump: Option<i64>,
) -> Result<loans::Projection, String> {
    loans::project_with(balance, &terms, &start, extra, lump.unwrap_or(0))
}
#[tauri::command(async)]
fn loan_payment_by_date(
    balance: i64,
    terms: loans::Terms,
    start: String,
    due: String,
    lump: Option<i64>,
) -> Result<i64, String> {
    loans::payment_by_date_with(balance, &terms, &start, &due, lump.unwrap_or(0))
}
#[tauri::command(async)]
fn record_loan_payment(
    db: State<'_, Database>,
    patch: Option<Patch>,
    terms: loans::Terms,
    from: String,
    amount: i64,
    date: String,
    id: String,
) -> Result<Patch, String> {
    db.change(patch, |mut b| {
        loans::record(&mut b, &terms, &from, amount, &date, &id)?;
        Ok(b)
    })
}

#[tauri::command(async)]
fn budget_action(
    db: State<'_, Database>,
    patch: Option<Patch>,
    action: serde_json::Value,
) -> Result<Patch, String> {
    db.change(patch, |b| workflows::apply(b, &action))
}
/// What every quick-assign option would change, in one read of the budget (see
/// `workflows::auto_assign_preview`): the inspector shows them all at once.
#[tauri::command(async)]
fn auto_assign_preview(
    db: State<'_, Database>,
    patch: Option<Patch>,
    month: String,
    categories: Vec<String>,
    limit_to_available: bool,
    as_of_month: Option<String>,
) -> Result<std::collections::HashMap<String, Option<Vec<workflows::AssignChange>>>, String> {
    let mut b = db.snapshot(patch)?;
    calculation_context(&mut b, as_of_month.as_deref())?;
    workflows::auto_assign_preview(&b, &month, &categories, limit_to_available)
}

fn auto_assign_result(mut b: Budget, month: &str, mode: &str, categories: &[String], limit: bool, as_of: Option<&str>) -> Result<Summary, String> {
    calculation_context(&mut b, as_of)?;
    let result = workflows::apply(b, &serde_json::json!({
        "type":"autoAssign", "month":month, "mode":mode, "categories":categories, "limitToAvailable":limit
    }))?;
    domain::calculate(&result, month)
}

/// The actual accounting result for the option being reviewed; no draft is changed.
#[tauri::command(async)]
fn preview_auto_assign_result(
    db: State<'_, Database>, patch: Option<Patch>, month: String, mode: String,
    categories: Vec<String>, limit_to_available: bool, as_of_month: Option<String>,
) -> Result<Summary, String> {
    auto_assign_result(db.snapshot(patch)?, &month, &mode, &categories, limit_to_available, as_of_month.as_deref())
}
#[tauri::command(async)]
fn calculate_reports(
    db: State<'_, Database>,
    patch: Option<Patch>,
    filter: reports::Filter,
) -> Result<reports::Report, String> {
    db.read(patch, |b| reports::calculate(b, &filter))
}
#[tauri::command(async)]
fn forecast(
    db: State<'_, Database>,
    patch: Option<Patch>,
    today: String,
    through: String,
    account_ids: Option<Vec<String>>,
) -> Result<reports::Forecast, String> {
    db.read(patch, |b| reports::forecast_for_accounts(b, &today, &through, &account_ids.unwrap_or_default()))
}
/// Saves the reports as CSV where you choose and returns the path (`None` when cancelled).
#[tauri::command(async)]
fn export_reports(
    window: WebviewWindow,
    db: State<'_, Database>,
    patch: Option<Patch>,
    filter: reports::Filter,
) -> Result<Option<String>, String> {
    let data = db.read(patch, |b| reports::csv(b, &reports::calculate(b, &filter)?))?;
    save::file(&window, &save::dated("Reports", "csv"), save::Kind::Csv, data.as_bytes())
}

#[tauri::command(async)]
fn preview_bank_file(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account: String,
    content: String,
    format: String,
    date_format: String,
    flip_signs: bool,
    columns: Option<bank_csv::Columns>,
) -> Result<bank_import::CompactReview, String> {
    db.read(patch, |b| {
        bank_import::preview_compact(b, &account, &content, &format, &date_format, flip_signs, columns.as_ref())
    })
}
/// A statement that isn't UTF-8, sent as ISO-8859-1 so its bytes cross unchanged: its text in the
/// encoding they fit (with `sheet`, a workbook's sheet of that name).
#[tauri::command(async)]
fn decode_statement(latin1: String, sheet: Option<String>) -> Result<bank_csv::Decoded, String> {
    bank_csv::decode_latin1(&latin1, sheet.as_deref())
}
/// An import of a file into an account, read as its last file was (see `bank_statement::start`).
#[tauri::command(async)]
fn bank_session(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account: String,
    content: String,
    session: String,
    budget: String,
    sheet: Option<String>,
    // A file the assistant wrote the standard way.
    prepared: Option<bool>,
) -> Result<bank_statement::Session, String> {
    if prepared == Some(true) {
        return Ok(bank_statement::start_prepared(&content, &account, &session, &budget));
    }
    db.read(patch, |b| bank_statement::start(b, &account, &content, &session, &budget, sheet))
}
/// The file as lines, each with what became of it.
#[tauri::command(async)]
fn bank_statement(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account: String,
    content: String,
    session: bank_statement::Session,
) -> Result<bank_statement::Statement, String> {
    db.read(patch, |b| bank_statement::statement(b, &account, &content, &session))
}
/// The statement, and the review of its lines that read.
#[tauri::command(async)]
fn bank_statement_preview(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account: String,
    content: String,
    session: bank_statement::Session,
) -> Result<bank_statement::StatementPreview, String> {
    db.read(patch, |b| bank_statement::preview_statement(b, &account, &content, &session))
}
/// Changes to an import, where they still fit it.
#[tauri::command(async)]
fn bank_apply_fixes(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account: String,
    content: String,
    session: bank_statement::Session,
    fixes: bank_statement::Fixes,
) -> Result<bank_statement::Applied, String> {
    db.read(patch, |b| bank_statement::apply_fixes(b, &account, &content, &session, &fixes))
}
/// A CSV's header row, first rows and the columns an import would read, for choosing them.
#[tauri::command(async)]
fn bank_file_columns(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account: String,
    content: String,
    columns: Option<bank_csv::Columns>,
) -> Result<bank_csv::Layout, String> {
    db.read(patch, |b| bank_csv::layout(b, &account, &content, columns.as_ref()))
}
/// With `remember`, the account keeps how this file read for its next one.
#[tauri::command(async)]
fn import_bank_file(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account: String,
    choices: Vec<bank_import::Choice>,
    remember: Option<bank_csv::FileSettings>,
) -> Result<Patch, String> {
    db.change(patch, |b| {
        let mut b = bank_import::apply(b, &account, choices)?;
        if let Some(settings) = remember {
            bank_csv::remember(&mut b, &account, settings)?;
        }
        Ok(b)
    })
}

#[tauri::command(async)]
fn upsert_transaction(
    db: State<'_, Database>,
    patch: Option<Patch>,
    mut transaction: domain::Transaction,
) -> Result<Patch, String> {
    let mut store = db.store()?;
    store.restore_transaction(&mut transaction)?;
    store.change_draft(patch, |budget| upsert(budget, transaction))
}
/// Use the shared transaction mutation so native Android follows the same rules.
fn upsert(budget: Budget, transaction: domain::Transaction) -> Result<Budget, String> {
    workflows::upsert_transaction(budget, transaction)
}
#[tauri::command(async)]
fn source_loan_terms(
    db: State<'_, Database>,
    patch: Option<Patch>,
    account_id: String,
) -> Result<loans::Terms, String> {
    db.read(patch, |b| loans::source_terms(b, &account_id))
}

#[tauri::command(async)]
fn list_budgets(library: State<'_, Library>) -> Vec<budgets::Listing> {
    library.lock().list()
}
/// Saves `budget` as a new budget file and returns its id. The open budget is unchanged.
#[tauri::command(async)]
fn create_budget(library: State<'_, Library>, db: State<'_, Database>, budget: Budget, fresh_start_token: Option<String>, patch: Option<Patch>) -> Result<String, String> {
    domain::validate(&budget)?;
    let mut library = library.lock();
    let budget = if let Some(token) = fresh_start_token {
        let source = db.store()?.fresh_start_source(patch, &token)?;
        fresh_start::prepare(budget, &source)?
    } else {
        budget
    };
    let id = library.create()?;
    let saved = Store::open_budget(&library.database(&id)?, &id).and_then(|store| {
        let mut store = store.with_backup_preferences(library.data_dir());
        let token = store.status()?.token;
        store.save(&budget, &token)
    });
    if let Err(e) = saved {
        library.forget_new(&id)?;
        return Err(e);
    }
    Ok(id)
}
/// Switches the window to another budget. The caller saves the open budget first.
#[tauri::command(async)]
fn open_budget(
    library: State<'_, Library>,
    db: State<'_, Database>,
    pending: State<'_, ynab_capture::PendingImport>,
    id: String,
) -> Result<Snapshot, String> {
    let mut library = library.lock();
    let mut store = Store::open_budget(&library.database(&id)?, &id)?.with_backup_preferences(library.data_dir());
    let snapshot = store.load_light()?;
    *db.store()? = store;
    library.set_current(&id)?;
    if let Ok(mut review) = pending.0.lock() {
        *review = None;
    }
    Ok(snapshot)
}
#[tauri::command(async)]
fn delete_budget(library: State<'_, Library>, id: String) -> Result<Vec<budgets::Listing>, String> {
    let mut library = library.lock();
    library.remove(&id)?;
    Ok(library.list())
}
/// Shows a file in the system file manager; a phone, which has none, offers it to the share
/// sheet instead. Only paths inside the app's data folder, and exports saved since it started.
#[tauri::command(async)]
fn reveal_path(library: State<'_, Library>, saved: State<'_, save::Saved>, path: String) -> Result<(), String> {
    let root = library
        .lock()
        .data_dir()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let target = std::path::PathBuf::from(&path)
        .canonicalize()
        .map_err(|_| urbudget_services::t!("That file no longer exists."))?;
    if !target.starts_with(&root) && !saved.contains(&target) {
        return Err(urbudget_services::t!("Only files in the app's data folder, or ones it just saved, can be shown."));
    }
    let shown = if cfg!(target_os = "windows") {
        // canonicalize returns a verbatim `\\?\` path, which Explorer does not accept.
        let plain = target.to_string_lossy().trim_start_matches(r"\\?\").to_owned();
        std::process::Command::new("explorer").arg(format!("/select,{plain}")).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(&target).spawn()
    } else {
        std::process::Command::new("xdg-open")
            .arg(target.parent().unwrap_or(&target))
            .spawn()
    };
    shown.map(|_| ()).map_err(|e| e.to_string())
}

pub fn run() {
    let context = tauri::generate_context!();
    #[cfg(debug_assertions)]
    let context = updates::use_local_server(context);
    let builder = tauri::Builder::default();
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    let builder = builder.plugin(tauri_plugin_dialog::init());
    builder
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            #[cfg(debug_assertions)]
            let directory = std::env::var_os("LOCAL_BUDGET_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(directory);
            // Before anything writes there: an empty folder is a copy opened for the first time.
            app.manage(urbudget_services::usage::Usage::open(&directory));
            vault::init(&directory);
            let library = budgets::Budgets::load(&directory).map_err(std::io::Error::other)?;
            let id = library.current().to_owned();
            let path = library.database(&id).map_err(std::io::Error::other)?;
            let store = Store::open_budget(&path, &id).map_err(std::io::Error::other)?.with_backup_preferences(&directory);
            app.manage(Database(Mutex::new(store)));
            app.manage(Library(Mutex::new(library)));
            app.manage(ynab_capture::PendingImport::default());
            app.manage(save::Saved::default());
            app.manage(cloud::Cloud::new(&directory));
            app.manage(simplefin::SimpleFin::new(&directory));
            assistant::manage(app, directory);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_language,
            upsert_transaction,
            source_loan_terms,
            preview_bank_file,
            bank_file_columns,
            decode_statement,
            bank_session,
            bank_statement,
            bank_statement_preview,
            bank_apply_fixes,
            import_bank_file,
            budget_action,
            auto_assign_preview,
            preview_auto_assign_result,
            calculate_reports,
            forecast,
            export_reports,
            export_transactions,
            write_export,
            load_budget,
            apply_patch,
            replace_draft,
            commit_budget,
            database_status,
            calculate_budget,
            validate_budget,
            backup_database,
            backup_retention,
            set_backup_retention,
            restore_budget,
            prune_backups,
            export_budget,
            reconcile_budget,
            ynab_capture::sync_ynab,
            ynab_capture::resolve_ynab,
            ynab_capture::ynab_plans,
            ynab_capture::accept_ynab_import,
            credentials::ynab_tokens,
            credentials::forget_ynab_token,
            simplefin::simplefin_status,
            simplefin::simplefin_connect,
            simplefin::simplefin_accounts,
            simplefin::simplefin_disconnect,
            simplefin::simplefin_sync,
            simplefin::simplefin_import,
            simplefin::simplefin_open,
            run_schedules,
            preview_schedules,
            project_loan,
            loan_payment_by_date,
            record_loan_payment,
            import_ynab_export,
            import_ynab_files,
            prepare_captured_ynab,
            export_ynab_csv,
            read_recovery,
            discard_recovery,
            list_budgets,
            create_budget,
            open_budget,
            delete_budget,
            reveal_path,
            updates::restart_app,
            updates::update_channel,
            updates::check_store_update,
            updates::install_store_update,
            updates::download_store_update,
            updates::apply_store_update,
            updates::open_update_store,
            updates::report_usage,
            cloud::cloud_status,
            cloud::account_overview,
            cloud::account_start,
            cloud::account_verify,
            cloud::account_sign_out,
            cloud::account_open,
            cloud::account_delete,
            cloud::sync::sync_status,
            cloud::sync::sync_now,
            cloud::sync::sync_has_key,
            cloud::sync::sync_setup,
            cloud::sync::sync_reset,
            cloud::sync::sync_track,
            cloud::sync::sync_download,
            cloud::sync::sync_resolve,
            cloud::sync::sync_remove,
            cloud::sync::sync_lock,
            assistant::assistant_status,
            assistant::assistant_sign_in,
            assistant::assistant_cancel_sign_in,
            assistant::assistant_sign_out,
            assistant::assistant_settings,
            assistant::assistant_set_key,
            assistant::assistant_forget_key,
            assistant::assistant_models,
            assistant::assistant_threads,
            assistant::assistant_thread,
            assistant::assistant_delete_thread,
            assistant::assistant_delete_all,
            assistant::assistant_attach,
            assistant::assistant_send,
            assistant::assistant_stop,
            assistant::assistant_retry,
            assistant::assistant_open,
            assistant::assistant_report_reply,
            assistant::assistant_report,
            assistant::assistant_print,
            assistant::assistant_spreadsheet,
            assistant::assistant_log_error,
            assistant::assistant_apply,
            assistant::assistant_revise,
            assistant::assistant_mark,
            assistant::assistant_import,
            assistant::assistant_save_import
        ])
        .run(context)
        .expect("Unable to start UrBudget");
}

#[cfg(test)]
mod tests {
    use super::*;
    use urbudget_core::test_support::{purchase, sample};

    #[test]
    fn returning_money_previews_the_remaining_future_deficit_instead_of_free_cash() {
        let mut b = sample();
        b.assignments[0].amount = 50_000;
        b.assignments.push(domain::Assignment { extra: Default::default(), month: "2026-11".into(), category_id: "food".into(), amount: 60_000 });
        let mut t = purchase(); t.account_id = "cash".into(); t.amount = -45_000; t.splits[0].amount = t.amount;
        b.transactions.push(t);
        let original = b.clone();
        let result = auto_assign_result(b, "2026-10", "resetAvailable", &["food".into()], true, Some("2026-10")).unwrap();
        assert_eq!((result.ready_to_assign, result.future_ready_to_assign), (0, -5000));
        assert_eq!(result.future_shortfalls[0].amount, 5000);
        assert_eq!(result.categories[0].available, 0);
        assert!(!original.extra.contains_key("calculationAsOfMonth"));
        assert_eq!(original.assignments[0].amount, 50_000);
    }

    #[test]
    fn derived_reads_release_the_store_lock_and_keep_their_own_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database(Mutex::new(Store::open(&dir.path().join("budget.db")).unwrap()));
        let original = db.snapshot(None).unwrap();
        let mut window = original.clone(); window.name = "Window preview".into();
        let patch = Patch::diff(&original, &window);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let db_ref = &db;
            let reader = scope.spawn(move || db_ref.read(Some(patch), |b| {
                started_tx.send(()).unwrap();
                resume_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                Ok(b.name.clone())
            }));
            started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            {
                let mut store = db.0.try_lock().expect("derived calculation must not hold the persistence lock");
                assert_eq!(store.snapshot_draft(None).unwrap(), original, "preview patch never becomes a draft edit");
                let mut changed = original.clone(); changed.name = "New draft".into();
                store.apply_patch(Patch::diff(&original, &changed)).unwrap();
            }
            resume_tx.send(()).unwrap();
            assert_eq!(reader.join().unwrap().unwrap(), "Window preview");
        });
        assert_eq!(db.snapshot(None).unwrap().name, "New draft");
    }

    #[test]
    fn a_new_transaction_is_stamped_when_added_and_edits_keep_the_time() {
        let mut copied = purchase();
        copied.set_created_at("2001-01-01T00:00:00.000Z");
        let before = domain::stamp_now();
        let b = upsert(sample(), copied).unwrap();
        let added = b.transactions[0].created_at().unwrap().to_owned();
        assert!(added >= before && added.len() == "2026-10-19T19:11:08.123Z".len(), "{added}");

        // An editor that rebuilds the record leaves the time out; one that sends another can't
        // change it either.
        let mut edited = b.transactions[0].clone();
        edited.memo = "Edited".into();
        edited.extra.remove("createdAt");
        let b = upsert(b, edited.clone()).unwrap();
        assert_eq!((b.transactions[0].memo.as_str(), b.transactions[0].created_at()), ("Edited", Some(added.as_str())));
        edited.set_created_at("2030-01-01T00:00:00.000Z");
        let b = upsert(b, edited).unwrap();
        assert_eq!(b.transactions[0].created_at(), Some(added.as_str()));

        // One from before the app kept the time stays without one.
        let mut old = purchase();
        old.id = "old".into();
        let mut b = b;
        b.transactions.push(old.clone());
        old.set_created_at("2030-01-01T00:00:00.000Z");
        let b = upsert(b, old).unwrap();
        assert_eq!(b.transactions[1].created_at(), None);
    }

    #[test]
    fn invalid_new_loan_payments_return_errors_without_changing_the_store_draft() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("budget.db")).unwrap();
        let token = store.load().unwrap().token;
        let mut b = sample();
        b.accounts[1].id = "loan".into();
        b.accounts[1].kind = domain::AccountKind::Tracking;
        b.accounts[1].opening_balance = -100_000;
        b.categories.retain(|c| c.payment_account().is_none());
        b.extra.insert("loans".into(), serde_json::json!([{
            "accountId":"loan", "categoryId":"food", "annualRateBasisPoints":1200,
            "monthlyPayment":10000, "monthlyFees":0
        }]));
        let saved_token = store.save(&b, &token).unwrap().token;
        let saved = store.snapshot_draft(None).unwrap();
        let mut before = saved.clone();
        before.name = "Unsaved draft to preserve".into();
        store.apply_patch(Patch::diff(&saved, &before)).unwrap();
        let mut payment = purchase();
        payment.account_id = "cash".into();
        payment.transfer_account_id = Some("loan".into());
        payment.transfer_status = Some(domain::Cleared::Uncleared);
        for date in ["x", "2026-1-1", "2026-02-30", "2026-é-01"] {
            payment.date = date.into();
            let error = store.change_draft(None, |b| upsert(b, payment.clone())).unwrap_err();
            assert!(error.contains("invalid date"), "{date}: {error}");
            assert_eq!(store.snapshot_draft(None).unwrap(), before);
            assert_eq!(store.status().unwrap().token, saved_token);
            assert!(store.has_unsaved_draft());
        }
        assert_eq!(store.saved().unwrap().0, b);
        payment.date = "2026-10-15".into();
        let changes = store.change_draft(None, |b| upsert(b, payment)).unwrap();
        let mut valid = before.clone();
        changes.apply(&mut valid);
        assert_eq!(valid.transactions.len(), 2, "a valid payment still adds its monthly interest");
        assert_eq!(valid.transactions.iter().find(|t| t.id == "loan-interest:loan:2026-10").unwrap().amount, -1000);
        assert_eq!(store.snapshot_draft(None).unwrap(), before, "commands return a patch for the caller to apply");
    }
}
