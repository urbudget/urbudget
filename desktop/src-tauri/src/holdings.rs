//! Desktop bindings for what an investment account holds when it comes from a positions file
//! or a value typed in (urbudget-core investments.rs and positions_file.rs). Interactive
//! Brokers has its own (ibkr.rs), and a bank connection's holdings come with its sync
//! (simplefin.rs).
use crate::{patch::Patch, Database};
use tauri::State;
use urbudget_services::investments::{self, Snapshot};
use urbudget_services::positions_file::{self, Read};

/// What a broker's file says an account holds, to look over before it's recorded.
#[tauri::command(async)]
pub fn holdings_file(content: String) -> Result<Read, String> {
    positions_file::read(&content)
}
/// Records what `account` holds on a day, and rewrites its value history.
#[tauri::command(async)]
pub fn holdings_record(db: State<'_, Database>, patch: Option<Patch>, account: String, snapshot: Snapshot) -> Result<Patch, String> {
    db.change(patch, |b| investments::record(b, &account, snapshot))
}
