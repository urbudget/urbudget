//! Desktop bindings for the shared bank connection service.
use crate::{Database, patch::Patch, bank_import};
use tauri::State;
pub use urbudget_services::simplefin::{SimpleFin,Status,Synced};
use urbudget_services::simplefin as service;
#[tauri::command]
pub fn simplefin_status(state: State<'_,SimpleFin>)->Status { service::simplefin_status(&state) }
#[tauri::command(async)]
pub async fn simplefin_connect(state:State<'_,SimpleFin>,token:String)->Result<Status,String>{service::simplefin_connect(&state,token).await}
#[tauri::command(async)]
pub async fn simplefin_accounts(state:State<'_,SimpleFin>)->Result<Status,String>{service::simplefin_accounts(&state).await}
#[tauri::command]
pub fn simplefin_disconnect(state:State<'_,SimpleFin>)->Result<Status,String>{service::simplefin_disconnect(&state)}
#[tauri::command(async)]
pub async fn simplefin_sync(db:State<'_,Database>,state:State<'_,SimpleFin>,patch:Option<Patch>,refresh:bool)->Result<Synced,String>{service::simplefin_sync(&db,&state,patch,refresh).await}
#[tauri::command(async)]
pub fn simplefin_import(db:State<'_,Database>,patch:Option<Patch>,account:String,choices:Vec<bank_import::Choice>,through:String)->Result<Patch,String>{service::simplefin_import(&db,patch,account,choices,through)}
/// Brings what the linked investment accounts hold into the budget, from the last download.
#[tauri::command(async)]
pub fn simplefin_holdings(db:State<'_,Database>,state:State<'_,SimpleFin>,patch:Option<Patch>)->Result<Patch,String>{service::simplefin_holdings(&db,&state,patch)}
#[tauri::command(async)]
pub fn simplefin_open()->Result<(),String>{service::simplefin_open()}
