//! One reply: ask the model, run the tools it calls on a copy of the budget, send the results
//! back, and repeat until it answers. The panel follows along through `assistant://event`.
use super::attachments::Files;
use super::history::{self, Item};
use super::llm::{self, Delta, FailureKind, Part, Request};
use super::{lock, now, phone, tools, Assistant};
use crate::domain::Budget;
use crate::t;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};


/// At most this many model calls per message, so a confused model can't run on forever. The last
/// answers with what it found ([LAST]) rather than ending the reply without a word.
const ROUNDS: usize = 12;
const LAST: &str = "This is the last step of this reply. Answer now from what you found, without calling tools, and say briefly what you couldn't finish so the user can ask again.";

pub struct Job {
    pub thread_id: String,
    pub folder: PathBuf,
    /// The budget as the window had it when the message was sent.
    pub budget: Budget,
    pub today: String,
    pub month: String,
    pub share_memos: bool,
    /// Bank sync as the app knew it then (SimpleFin::snapshot).
    pub bank_sync: Value,
    /// The phone reads bank texts for this budget, so it answers the bank-text tools (`phone.rs`).
    pub bank_texts: bool,
    /// The app the message came from ("android", "ios"; the desktop sends none), for the app guide.
    pub platform: String,
}

pub fn emit(thread_id: &str, mut event: Value) {
    event["threadId"] = json!(thread_id);
    crate::host::emit("assistant://event", event);
}

/// `written` is the text of the round in progress as far as it has got, for Stop to keep.
pub async fn run(assistant: Assistant, job: Job, cancelled: Arc<AtomicBool>, written: Arc<Mutex<String>>) {
    emit(&job.thread_id, json!({ "type": "started" }));
    let outcome = turn(&assistant, &job, &cancelled, &written).await;
    if cancelled.load(Ordering::SeqCst) { return; }
    if let Err(failure) = outcome {
        let action = history::error_action(code(failure.kind), assistant.settings().provider);
        let _ = assistant.with_thread_guarded(&job.folder, &job.thread_id, Some(&cancelled), |t| {
            t.items.push(Item::Error { text: failure.message.clone(), code: code(failure.kind).into() })
        });
        emit(&job.thread_id, json!({ "type": "error", "text": failure.message, "code": code(failure.kind), "action": action }));
    }
    let _ = assistant.with_thread_guarded(&job.folder, &job.thread_id, Some(&cancelled), |t| t.updated = now());
    let completed = {
        let mut running = lock(&assistant.running);
        if running.get(&job.thread_id).is_some_and(|task| Arc::ptr_eq(&task.cancelled, &cancelled)) {
            running.remove(&job.thread_id); true
        } else { false }
    };
    if completed { emit(&job.thread_id, json!({ "type": "done" })); }
}

fn code(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::SignIn => "signIn",
        FailureKind::Limit => "limit",
        FailureKind::NotEligible => "notEligible",
        FailureKind::Busy => "busy",
        FailureKind::Refused => "refused",
        FailureKind::Unsupported => "unsupported",
        FailureKind::Model => "model",
        FailureKind::Other => "other",
    }
}

async fn turn(assistant: &Assistant, job: &Job, cancelled: &AtomicBool, written: &Mutex<String>) -> Result<(), llm::Failure> {
    let specs: Vec<_> = tools::specs().into_iter().filter(|s| job.bank_texts || !phone::answers(s.name)).collect();
    let files = Files::open(history::files(&job.folder));
    let other = |e: String| llm::Failure::new(FailureKind::Other, e);
    for round in 0..ROUNDS {
        let credentials = assistant.credentials().await?;
        let model = assistant.model().await?;
        let thread = assistant.with_thread_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| t.clone()).map_err(other)?;
        // The app's language is read for every request, so a switch applies to the next reply.
        let mut instructions = super::prompt::in_language(if thread.instructions.is_empty() { super::prompt::INSTRUCTIONS } else { &thread.instructions });
        let last = round == ROUNDS - 1;
        if last {
            instructions = format!("{instructions}\n\n{LAST}");
        }
        let request = Request {
            model: &model,
            instructions: &instructions,
            items: &thread.items,
            files: &files,
            tools: &specs,
            answer_only: last,
        };
        let provider = match credentials.provider {
            llm::Provider::Anthropic => "anthropic",
            _ => "openai",
        };
        // Busy services get two more tries, as long as nothing was shown yet.
        let mut attempt = 0;
        let reply = loop {
            let mut streamed = false;
            let mut on_delta = |delta: Delta| match delta {
                Delta::Text(text) => {
                    streamed = true;
                    lock(written).push_str(text);
                    emit(&job.thread_id, json!({ "type": "delta", "text": text }));
                }
                Delta::Thinking(text) => emit(&job.thread_id, json!({ "type": "thinking", "text": text })),
                Delta::Started | Delta::Arguments(_) => {}
            };
            match llm::respond(&assistant.http, &credentials, &request, &mut on_delta).await {
                Err(f) if f.kind == FailureKind::Busy && !streamed && attempt < 2 => {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(1500 * attempt as u64 * attempt as u64)).await;
                }
                other => break other,
            }
        }?;
        let mut calls = vec![];
        let mut items = vec![];
        for part in reply.parts {
            match part {
                Part::Raw(value) => items.push(Item::Raw { provider: provider.into(), value }),
                Part::Text(text) => {
                    emit(&job.thread_id, json!({ "type": "message", "text": text }));
                    items.push(Item::Assistant { text });
                }
                Part::Call(call) => {
                    items.push(Item::Call { call_id: call.call_id.clone(), name: call.name.clone(), arguments: call.arguments.clone() });
                    calls.push(call);
                }
            }
        }
        // The round is kept whole from here, so there's nothing of it left for Stop to save. The
        // text is cleared inside the lock on the conversation: a Stop gets one or the other.
        assistant.with_thread_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| {
            lock(written).clear();
            t.items.extend(items)
        }).map_err(other)?;
        if calls.is_empty() {
            return Ok(());
        }
        // The import the latest message was sent from, as it read then.
        let import = thread.items.iter().rev().find_map(|i| match i {
            Item::User { import, .. } => Some(import.as_ref()),
            _ => None,
        }).flatten();
        let context = tools::Context {
            budget: &job.budget,
            today: &job.today,
            month: &job.month,
            share_memos: job.share_memos,
            files: &files,
            bank_sync: &job.bank_sync,
            import,
            platform: super::guide::Platform::of(&job.platform),
        };
        for call in calls {
            // What it's doing now, for the panel while a slow lookup (the phone's search) runs.
            emit(&job.thread_id, json!({ "type": "activity", "text": tools::working(&call.name) }));
            let outcome = match serde_json::from_str::<Value>(&call.arguments) {
                Ok(args) if args.is_object() && job.bank_texts && phone::answers(&call.name) => {
                    phone::outcome(&call.name, phone::ask(assistant, &job.thread_id, &call.name, &args).await)
                }
                Ok(args) if args.is_object() => tools::call(&call.name, &args, &context),
                _ => tools::Outcome {
                    output: json!({ "error": "The arguments weren't a valid JSON object. Call the tool again." }),
                    step: String::new(),
                    proposal: None,
                },
            };
            let failed = outcome.output.get("error").is_some();
            let proposal_id = outcome.proposal.as_ref().map(|p| p.id.clone());
            let view = outcome.proposal.as_ref().map(history::proposal_view);
            assistant
                .with_thread_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| {
                    if let Some(p) = outcome.proposal {
                        t.proposals.push(p);
                    }
                    t.items.push(Item::Result {
                        call_id: call.call_id.clone(),
                        output: outcome.output.to_string(),
                        step: outcome.step.clone(),
                        failed,
                        proposal: proposal_id,
                    });
                })
                .map_err(other)?;
            if !outcome.step.is_empty() {
                emit(&job.thread_id, json!({ "type": "step", "text": outcome.step, "failed": failed }));
            }
            if let Some(view) = view {
                emit(&job.thread_id, json!({ "type": "proposal", "proposal": view }));
            }
        }
        if round == ROUNDS - 1 {
            return Err(llm::Failure::new(
                FailureKind::Other,
                t!("The assistant took too many steps and stopped. Ask again, a bit more specifically."),
            ));
        }
    }
    Ok(())
}
