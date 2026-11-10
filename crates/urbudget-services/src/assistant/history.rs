//! Conversations, one JSON file each in `<budget folder>/assistant/threads`. The history is
//! append-only: earlier turns are sent back exactly as they were, which some models require.
use crate::t;
use super::attachments::Meta;
use super::llm::{Provider, Usage};
use super::proposals::{Card, Proposal, Status};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What went with a message besides its words, for the panel to show on it: where the user was
/// and what they attached. The model gets the same in the message's note.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct SentWith {
    /// The page's key as the app sent it ("budget", "transactions"…); the app names it.
    pub page: String,
    /// YYYY-MM, when the page shows a month.
    pub month: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// How many selected transactions were attached.
    pub transactions: usize,
    /// The bank file on the Import page went too.
    pub import: bool,
}

/// What a conversation was started from, when it carries on an earlier one that grew too long.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Carried {
    /// The earlier conversation's title.
    pub from: String,
    /// What the model is told of it with the first message.
    pub text: String,
}

/// A conversation is long enough to offer carrying on in a new one when its latest request sent
/// more than this many tokens: each further round costs that much again.
pub const LONG: u64 = 120_000;

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sent_with: Option<SentWith>,
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
    /// Something the app tells the model partway through (its last step, a change of language).
    /// It's sent in the user's turn and stays where it was said, so earlier turns never change;
    /// the panel doesn't show it.
    Note {
        text: String,
    },
    /// The reply before this stopped at its length limit.
    Cut,
    Error {
        text: String,
        #[serde(default)]
        code: String,
    },
    Stopped,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub created: String,
    pub updated: String,
    /// The instructions the conversation started with, the language part included; kept so
    /// earlier turns stay valid. Some models refuse a conversation whose instructions, tools or
    /// earlier messages changed since they last answered (see `anthropic.rs`).
    #[serde(default)]
    pub instructions: String,
    /// The app's language the instructions were written for ("en" or "ar"). A switch since is
    /// told in a note, not by changing the instructions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// Whether the bank-text tools were offered when the conversation began. The tool list
    /// stays that way for its whole life.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bank_texts: Option<bool>,
    pub items: Vec<Item>,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
    /// Tokens every request of the conversation used, added up.
    #[serde(default)]
    pub usage: Usage,
    /// Tokens the latest request sent: how much the next one will, at least.
    #[serde(default)]
    pub last_input: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carried: Option<Carried>,
    /// The user's thumbs on replies, by the reply's place in `items`: true is up.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ratings: BTreeMap<usize, bool>,
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
            // Only the three fields: the rest of the file is passed over, not built.
            serde_json::from_slice::<Listing>(&std::fs::read(e.path()).ok()?).ok()
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
    /// For a `bankTexts` card, what the phone does when it's applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_texts: Option<&'a Value>,
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
        bank_texts: p.bank_texts.as_ref(),
    })
    .unwrap_or(Value::Null)
}

/// What the panel offers under an error, by its code and the service in use: "signIn" (sign in
/// with ChatGPT again), "settings" (the key, the model or the service needs changing), "usage"
/// (the plan's limit: open its usage page), "newConversation" (this one no longer fits: carry on
/// in a new one), "retry", or "none" when asking again wouldn't help.
pub fn error_action(code: &str, provider: Provider) -> &'static str {
    let plan = provider == Provider::Chatgpt;
    match code {
        "full" => "newConversation",
        "signIn" if plan => "signIn",
        "signIn" | "model" | "notEligible" | "unsupported" => "settings",
        "limit" if plan => "usage",
        "refused" => "none",
        _ => "retry",
    }
}

/// The conversation as the panel shows it. Only its last entry, when that's an error, carries
/// an `action`: an earlier error's moment has passed. A reply carries its place in the history
/// (`index`, for rating it), the tools its turn had called by then, and the user's thumb.
pub fn view(t: &Thread, provider: Provider) -> Value {
    let mut entries = vec![];
    let mut called: Vec<&str> = vec![];
    for (index, item) in t.items.iter().enumerate() {
        match item {
            Item::User { text, attachments, sent_with, .. } => {
                called.clear();
                let mut entry = json!({ "kind": "user", "text": text, "attachments": attachments });
                if let Some(with) = sent_with {
                    entry["with"] = json!(with);
                }
                entries.push(entry);
            }
            Item::Call { name, .. } => {
                if !called.contains(&name.as_str()) {
                    called.push(name);
                }
            }
            Item::Assistant { text } if !text.trim().is_empty() => {
                let mut entry = json!({ "kind": "assistant", "text": text, "index": index, "tools": called });
                if let Some(up) = t.ratings.get(&index) {
                    entry["rating"] = json!(if *up { "up" } else { "down" });
                }
                entries.push(entry);
            }
            Item::Cut => entries.push(json!({ "kind": "cut" })),
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
    if let Some(last) = entries.last_mut().filter(|e| e["kind"] == "error") {
        last["action"] = json!(error_action(last["code"].as_str().unwrap_or_default(), provider));
    }
    let mut view = json!({ "id": t.id, "title": t.title, "entries": entries, "usage": t.usage, "long": t.last_input > LONG });
    if let Some(carried) = &t.carried {
        view["continuedFrom"] = json!(carried.from);
    }
    view
}

/// What a conversation hands to the one that carries it on: the words said last, oldest first,
/// and what became of its proposals. No lookups, files or thinking.
pub fn carry(t: &Thread) -> String {
    const EXCHANGES: usize = 8;
    let cut = |text: &str, max: usize| super::tools::truncate(text.trim(), max);
    let mut said: Vec<String> = t
        .items
        .iter()
        .rev()
        .filter_map(|item| match item {
            Item::User { text, .. } if !text.trim().is_empty() => Some(format!("The user: {}", cut(text, 600))),
            Item::Assistant { text } if !text.trim().is_empty() => Some(format!("You: {}", cut(text, 1200))),
            _ => None,
        })
        .take(EXCHANGES)
        .collect();
    said.reverse();
    for p in &t.proposals {
        said.push(format!("Proposal \u{201c}{}\u{201d}: {}", p.title, super::status_word(p.status)));
    }
    said.join("\n")
}

/// Takes Claude's thinking out of the kept replies. Its newer models refuse a block of thinking
/// once anything before it has changed (the instructions, the tools, an earlier message); without
/// the thinking the same conversation is accepted, and only that reasoning is lost. Says whether
/// there was any to take out.
pub fn drop_thinking(items: &mut [Item]) -> bool {
    let mut dropped = false;
    for item in items {
        let Item::Raw { provider, value } = item else { continue };
        let Some(blocks) = value.as_array_mut().filter(|_| provider == "anthropic") else { continue };
        let before = blocks.len();
        blocks.retain(|b| !matches!(b["type"].as_str(), Some("thinking" | "redacted_thinking")));
        dropped |= blocks.len() != before;
    }
    dropped
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
            items: vec![Item::User { text: "hi".into(), attachments: vec![], context: String::new(), at: String::new(), import: None, sent_with: None }],
            ..Thread::default()
        };
        save(dir.path(), &t).unwrap();
        assert_eq!(load(dir.path(), "t_1").unwrap(), t);
        assert_eq!(list(dir.path())[0].title, "Cover overspending");
        delete(dir.path(), "t_1").unwrap();
        assert!(list(dir.path()).is_empty());
        assert!(load(dir.path(), "../budget").is_err());
    }

    #[test]
    fn only_the_last_error_says_what_helps() {
        let error = |code: &str| Item::Error { text: "It failed.".into(), code: code.into() };
        let mut t = Thread { id: "t_1".into(), title: "x".into(), created: String::new(), updated: String::new(), instructions: String::new(),
            items: vec![error("busy"), error("signIn")], proposals: vec![], ..Default::default() };
        let entries = |t: &Thread, provider| view(t, provider)["entries"].as_array().unwrap().clone();
        let plan = entries(&t, Provider::Chatgpt);
        assert!(plan[0].get("action").is_none());
        assert_eq!(plan[1]["action"], "signIn");
        assert_eq!(entries(&t, Provider::Anthropic)[1]["action"], "settings");
        // Asking again helps when the service was busy, and never after a refusal.
        assert_eq!((error_action("busy", Provider::Openai), error_action("other", Provider::Chatgpt)), ("retry", "retry"));
        assert_eq!((error_action("refused", Provider::Anthropic), error_action("limit", Provider::Chatgpt), error_action("limit", Provider::Openai)), ("none", "usage", "retry"));
        assert_eq!((error_action("model", Provider::Chatgpt), error_action("unsupported", Provider::Openai)), ("settings", "settings"));
        // Once the conversation has moved on, the error offers nothing.
        t.items.push(Item::Stopped);
        assert!(entries(&t, Provider::Chatgpt).iter().all(|e| e.get("action").is_none()));
    }

    #[test]
    fn a_stopped_call_gets_a_result() {
        let mut items = vec![Item::Call { call_id: "c1".into(), name: "get_month".into(), arguments: "{}".into() }];
        settle(&mut items);
        assert!(matches!(&items[1], Item::Result { call_id, failed: true, .. } if call_id == "c1"));
    }
}
