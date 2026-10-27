//! Application behavior shared by desktop's Tauri adapter and native Android's JNI adapter.
//! UI frameworks belong in those adapters; accounting, persistence and remote protocols live here.
pub use urbudget_core::{domain, engine, ledger, targets, schedules, workflows, reports, loans, patch, store, sync_merge, bank_import, bank_statement, ynab, ynab_files, fresh_start, import_sync, budgets};
/// The language of the messages people see (`i18n::set_language("ar" | "en")`), shared with the core.
pub use urbudget_core::{i18n, t};
pub mod cloud;
pub mod vault;
pub mod host;
pub mod runtime;
pub mod billing;
pub mod assistant;
pub mod simplefin;
pub mod ynab_capture;
pub mod credentials;
pub mod usage;
#[cfg(target_os = "android")]
pub mod keystore;

use domain::Budget;
use patch::Patch;
use store::Store;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// The open budget. SQL commits, draft tokens and sync bases use the same Store on every host.
pub struct Database(pub Mutex<Store>);
/// Lock the library before the database when a command needs both.
pub struct Library(pub Mutex<budgets::Budgets>);
impl Library {
    pub fn lock(&self) -> MutexGuard<'_, budgets::Budgets> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}
impl Database {
    pub fn store(&self) -> Result<MutexGuard<'_, Store>, String> {
        Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn read<T>(&self, patch: Option<Patch>, f: impl FnOnce(&Budget) -> Result<T, String>) -> Result<T, String> {
        f(&self.snapshot(patch)?)
    }
    pub fn snapshot(&self, patch: Option<Patch>) -> Result<Budget, String> {
        self.store()?.snapshot_draft(patch)
    }
    pub fn change(&self, patch: Option<Patch>, f: impl FnOnce(Budget) -> Result<Budget, String>) -> Result<Patch, String> {
        self.store()?.change_draft(patch, f)
    }
}

/// One Tokio runtime keeps background assistant jobs alive after the JNI call returns.
pub fn executor() -> &'static tokio::runtime::Runtime {
    static EXECUTOR: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    EXECUTOR.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("service runtime"))
}
pub fn block_on<F: std::future::Future>(future: F) -> F::Output { executor().block_on(future) }
