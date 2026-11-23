//! One accounting engine for desktop and native mobile clients.
//!
//! The platform adapters own UI, credentials, networking, and lifecycle. This
//! crate owns integer money, ledger validation, targets, reporting, imports,
//! portable data formats, merge rules, and SQLite persistence.
pub mod bank_csv;
pub mod bank_import;
pub mod bank_statement;
pub mod bank_statements;
pub mod budget_templates;
pub mod budgets;
pub mod commands;
pub mod domain;
pub mod engine;
pub mod flex;
pub mod fresh_start;
pub mod i18n;
pub mod import_sync;
pub mod investment_stats;
pub mod investments;
pub mod ledger;
pub mod loans;
pub mod patch;
pub mod positions_file;
pub mod reports;
pub mod schedules;
pub mod setup;
pub mod sms_import;
pub mod sms_templates;
pub mod store;
pub mod sync_merge;
pub mod sync_storage;
pub mod targets;
pub mod workflows;
pub mod ynab;
pub mod ynab_files;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
