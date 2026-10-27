//! `urbudget --mcp`: the assistant's read tools as an MCP server on stdin/stdout, for apps
//! like Claude Desktop. It reads the saved budget each time and never changes it; changes are
//! made in UrBudget, where they can be reviewed.
use super::attachments::Files;
use super::tools;
use crate::store::Store;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const PROTOCOL: &str = "2025-06-18";
/// Tools that only read. Proposals need the app's review, and attachments need its panel.
const READ_ONLY: &[&str] = &["get_month", "find_transactions", "payee_history", "get_report", "get_forecast", "get_setup", "project_loan"];

/// Where the app keeps its data: the same folder Tauri chooses for `com.localbudget.desktop`.
fn data_dir() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--data-dir") {
        return args.get(i + 1).map(PathBuf::from);
    }
    if let Some(dir) = std::env::var_os("LOCAL_BUDGET_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    const ID: &str = "com.localbudget.desktop";
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(ID))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support").join(ID))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join(ID))
    }
}

/// The open budget as last saved, and whether memos may be shared.
fn budget() -> Result<(crate::domain::Budget, bool), String> {
    let dir = data_dir().ok_or("Couldn't find UrBudget's data folder. Pass --data-dir.")?;
    let (id, path) = crate::budgets::Budgets::peek(&dir)?;
    let mut store = Store::open_budget(&path, &id)?;
    let budget = store.load()?.budget;
    let share_memos = std::fs::read(dir.join("assistant.json"))
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .and_then(|v| v["shareMemos"].as_bool())
        .unwrap_or(true);
    Ok((budget, share_memos))
}

fn list() -> Value {
    let tools: Vec<Value> = tools::specs()
        .into_iter()
        .filter(|s| READ_ONLY.contains(&s.name))
        .map(|s| json!({
            "name": s.name,
            "description": s.description,
            "inputSchema": s.parameters,
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        }))
        .collect();
    json!({ "tools": tools })
}

fn call(params: &Value) -> Value {
    let name = params["name"].as_str().unwrap_or_default();
    if !READ_ONLY.contains(&name) {
        return json!({ "content": [{ "type": "text", "text": format!("Unknown tool {name}.") }], "isError": true });
    }
    let args = params.get("arguments").filter(|a| a.is_object()).cloned().unwrap_or(json!({}));
    let (budget, share_memos) = match budget() {
        Ok(b) => b,
        Err(e) => return json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    };
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let files = Files::empty();
    let ctx = tools::Context { budget: &budget, today: &today, month: &today[..7], share_memos, files: &files, bank_sync: &Value::Null, import: None };
    let outcome = tools::call(name, &args, &ctx);
    let failed = outcome.output.get("error").is_some();
    json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&outcome.output).unwrap_or_default() }],
        "isError": failed,
    })
}

/// Answers one JSON-RPC message; None for notifications.
pub fn handle(message: &Value) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message["method"].as_str().unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "urbudget", "title": "UrBudget", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Read-only access to the budget open in UrBudget: months, transactions, reports, forecasts and setup. Amounts are decimal strings in the budget's currency; negative is money out. To change anything, the user does it in UrBudget, whose own assistant can propose changes for review.",
        }),
        "ping" => json!({}),
        "tools/list" => list(),
        "tools/call" => call(&message["params"]),
        _ => {
            return Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("Unknown method {method}") } }))
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// Serves until stdin closes.
pub fn serve() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(&message),
            Err(_) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "Parse error" } })),
        };
        if let Some(reply) = reply {
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_read_tools_and_answers_initialize() {
        let init = handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        let listed = handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"get_month"));
        assert!(!names.iter().any(|n| n.starts_with("propose")));
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let refused = handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "propose_budget_moves" } })).unwrap();
        assert_eq!(refused["result"]["isError"], true);
    }
}
