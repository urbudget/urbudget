//! Conversations, one JSON file each in `<budget folder>/assistant/threads`. The history is
//! append-only: earlier turns are sent back exactly as they were, which some models require.
use crate::t;
use super::attachments::Meta;
use super::proposals::{Card, Proposal, Status};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Item {
    #[serde(rename_all = "camelCase")]
    User {
        text: String,
        #[serde(default)]
        attachments: Vec<Meta>,
        /// What the user was looking at, written when the message was sent.
        #[serde(default)]
        context: String,
        at: String,
        /// The bank file they were importing, as it read then: fixes the assistant proposes for it
        /// are made against this, never the import as it is later.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        import: Option<crate::bank_statement::Brief>,
    },
    Assistant {
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    Call {
        call_id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename_all = "camelCase")]
    Result {
        call_id: String,
        output: String,
        /// The line the panel shows for this call.
        #[serde(default)]
        step: String,
        #[serde(default)]
        failed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        proposal: Option<String>,
    },
    /// A provider's own record of a reply (reasoning, thinking blocks), sent back verbatim.
    Raw {
        provider: String,
        value: Value,
    },
    Error {
        text: String,
        #[serde(default)]
        code: String,
    },
    Stopped,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub created: String,
    pub updated: String,
    /// The instructions the conversation started with; kept so earlier turns stay valid.
    #[serde(default)]
    pub instructions: String,
    pub items: Vec<Item>,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub id: String,
    pub title: String,
    pub updated: String,
}

pub fn root(folder: &Path) -> PathBuf {
    folder.join("assistant")
}
fn threads(folder: &Path) -> PathBuf {
    root(folder).join("threads")
}
pub fn files(folder: &Path) -> PathBuf {
    root(folder).join("files")
}
fn valid(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn save(folder: &Path, thread: &Thread) -> Result<(), String> {
    if !valid(&thread.id) {
        return Err(t!("Invalid conversation id."));
    }
    let dir = threads(folder);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", thread.id));
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec(thread).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}
pub fn load(folder: &Path, id: &str) -> Result<Thread, String> {
    if !valid(id) {
        return Err(t!("That conversation no longer exists."));
    }
    let raw = std::fs::read(threads(folder).join(format!("{id}.json")))
        .map_err(|_| t!("That conversation no longer exists."))?;
    serde_json::from_slice(&raw).map_err(|e| t!("That conversation couldn't be read: {e}", e))
}
pub fn list(folder: &Path) -> Vec<Listing> {
    let Ok(entries) = std::fs::read_dir(threads(folder)) else { return vec![] };
    let mut out: Vec<Listing> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let raw = std::fs::read(e.path()).ok()?;
            let t: Thread = serde_json::from_slice(&raw).ok()?;
            Some(Listing { id: t.id, title: t.title, updated: t.updated })
        })
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}
pub fn delete(folder: &Path, id: &str) -> Result<(), String> {
    if !valid(id) {
        return Ok(());
    }
    match std::fs::remove_file(threads(folder).join(format!("{id}.json"))) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}
/// Removes every conversation and attached file of this budget.
pub fn delete_all(folder: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(root(folder)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// What the panel needs to draw a proposal: no stored change or import rows.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalView<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub kind: &'a str,
    pub status: Status,
    pub card: &'a Card,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<&'a str>,
    pub rows: usize,
    /// Sheets the card can save as a spreadsheet: a spreadsheet's sheets, or a report's tables.
    pub sheets: usize,
    /// Fixes for a bank file being imported, for the import to apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixes: Option<&'a crate::bank_statement::Fixes>,
    /// What the user left out or changed on the card, to change again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<&'a super::proposals::Revision>,
}
pub fn proposal_view(p: &Proposal) -> Value {
    serde_json::to_value(ProposalView {
        id: &p.id,
        title: &p.title,
        kind: &p.kind,
        status: p.status,
        card: &p.card,
        problem: p.problem.as_deref(),
        rows: p.import.as_ref().map_or(0, |i| i.rows.len()),
        sheets: match (&p.workbook, &p.report) {
            (Some(w), _) => w.sheets.len(),
            (None, Some(r)) => super::workbook::report_tables(&r.body).len(),
            _ => 0,
        },
        fixes: p.fixes.as_ref(),
        choices: p.review.as_ref().map(|r| &r.revision),
    })
    .unwrap_or(Value::Null)
}

/// The conversation as the panel shows it.
pub fn view(t: &Thread) -> Value {
    let mut entries = vec![];
    for item in &t.items {
        match item {
            Item::User { text, attachments, .. } => entries.push(json!({
                "kind": "user",
                "text": text,
                "attachments": attachments,
            })),
            Item::Assistant { text } if !text.trim().is_empty() => {
                entries.push(json!({ "kind": "assistant", "text": text }))
            }
            Item::Result { step, failed, proposal, .. } => {
                if !step.is_empty() {
                    entries.push(json!({ "kind": "step", "text": step, "failed": failed }));
                }
                if let Some(p) = proposal.as_ref().and_then(|id| t.proposals.iter().find(|p| &p.id == id)) {
                    entries.push(json!({ "kind": "proposal", "proposal": proposal_view(p) }));
                }
            }
            Item::Error { text, code } => entries.push(json!({ "kind": "error", "text": text, "code": code })),
            Item::Stopped => entries.push(json!({ "kind": "stopped" })),
            _ => {}
        }
    }
    json!({ "id": t.id, "title": t.title, "entries": entries })
}

/// Pairs every call with a result, so a stopped turn can still be sent back.
pub fn settle(items: &mut Vec<Item>) {
    let answered: std::collections::HashSet<String> = items
        .iter()
        .filter_map(|i| match i {
            Item::Result { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .collect();
    let missing: Vec<String> = items
        .iter()
        .filter_map(|i| match i {
            Item::Call { call_id, .. } if !answered.contains(call_id) => Some(call_id.clone()),
            _ => None,
        })
        .collect();
    for call_id in missing {
        items.push(Item::Result {
            call_id,
            output: json!({ "error": "Not run: the user stopped this turn." }).to_string(),
            step: String::new(),
            failed: true,
            proposal: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threads_save_list_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let t = Thread {
            id: "t_1".into(),
            title: "Cover overspending".into(),
            created: "2026-10-16T10:00:00Z".into(),
            updated: "2026-10-16T10:01:00Z".into(),
            instructions: "x".into(),
            items: vec![Item::User { text: "hi".into(), attachments: vec![], context: String::new(), at: String::new(), import: None }],
            proposals: vec![],
        };
        save(dir.path(), &t).unwrap();
        assert_eq!(load(dir.path(), "t_1").unwrap(), t);
        assert_eq!(list(dir.path())[0].title, "Cover overspending");
        delete(dir.path(), "t_1").unwrap();
        assert!(list(dir.path()).is_empty());
        assert!(load(dir.path(), "../budget").is_err());
    }

    #[test]
    fn a_stopped_call_gets_a_result() {
        let mut items = vec![Item::Call { call_id: "c1".into(), name: "get_month".into(), arguments: "{}".into() }];
        settle(&mut items);
        assert!(matches!(&items[1], Item::Result { call_id, failed: true, .. } if call_id == "c1"));
    }
}
