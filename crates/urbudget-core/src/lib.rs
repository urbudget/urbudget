//! One accounting engine for desktop and native mobile clients.
//!
//! The platform adapters own UI, credentials, networking, and lifecycle. This
//! crate owns integer money, ledger validation, targets, reporting, imports,
//! portable data formats, merge rules, and SQLite persistence.
pub mod bank_csv;
pub mod bank_import;
pub mod bank_statement;
pub mod bank_statements;
pub mod budgets;
pub mod domain;
pub mod engine;
pub mod fresh_start;
pub mod i18n;
pub mod import_sync;
pub mod ledger;
pub mod loans;
pub mod patch;
pub mod reports;
pub mod schedules;
pub mod store;
pub mod sync_merge;
pub mod sync_storage;
pub mod targets;
pub mod workflows;
pub mod ynab;
pub mod ynab_files;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
