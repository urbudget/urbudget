//! One reply: ask the model, run the tools it calls on a copy of the budget, send the results
//! back, and repeat until it answers. The panel follows along through `assistant://event`.
use super::attachments::Files;
use super::history::{self, Item};
use super::llm::{self, Delta, FailureKind, Part, Request};
use super::proposals::Status;
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
/// What a call gets back when the reply it was written in stopped at its length limit.
const CUT_CALL: &str = "Your reply reached its length limit before this call was complete, so it wasn't run. Make it again with less in it, or in parts.";

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
        FailureKind::Full => "full",
        FailureKind::Other => "other",
    }
}

async fn turn(assistant: &Assistant, job: &Job, cancelled: &AtomicBool, written: &Mutex<String>) -> Result<(), llm::Failure> {
    let files = Files::open(history::files(&job.folder));
    let other = |e: String| llm::Failure::new(FailureKind::Other, e);
    // The tools are the ones the conversation began with, whatever this phone reads now: a
    // request's tools and instructions stay the same for its whole life (see `anthropic.rs`).
    let offered = assistant.peek_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| t.bank_texts).map_err(other)?.unwrap_or(job.bank_texts);
    let specs: Vec<_> = tools::specs().into_iter().filter(|s| offered || !phone::answers(s.name)).collect();
    // Thinking a model refused to take back is left out once, and the round asked again.
    let mut without_thinking = false;
    for round in 0..ROUNDS {
        let credentials = assistant.credentials().await?;
        let model = assistant.model().await?;
        let last = round == ROUNDS - 1;
        if last {
            // Said in the conversation, where it stays: the instructions never change.
            assistant.with_thread_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| t.items.push(Item::Note { text: LAST.into() })).map_err(other)?;
        }
        let provider = match credentials.provider {
            llm::Provider::Anthropic => "anthropic",
            _ => "openai",
        };
        // Busy services get two more tries, as long as nothing was shown yet.
        let mut attempt = 0;
        let reply = loop {
            let thread = assistant.peek_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| t.clone()).map_err(other)?;
            let instructions = if thread.instructions.is_empty() { super::prompt::in_language(super::prompt::INSTRUCTIONS) } else { thread.instructions.clone() };
            let request = Request {
                model: &model,
                instructions: &instructions,
                items: &thread.items,
                files: &files,
                tools: &specs,
                answer_only: last,
                kept: true,
            };
            let mut streamed = false;
            let mut on_delta = |delta: Delta| match delta {
                // Stopped: what arrives after is neither kept nor shown.
                _ if cancelled.load(Ordering::SeqCst) => {}
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
                Err(f) if super::anthropic::thinking_refused(&f) && !without_thinking => {
                    without_thinking = true;
                    assistant.with_thread_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| { history::drop_thinking(&mut t.items); }).map_err(other)?;
                }
                other => break other,
            }
        }?;
        let (cut, used) = (reply.cut, reply.usage);
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
        let import = assistant.with_thread_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| {
            lock(written).clear();
            t.items.extend(items);
            t.usage.add(used);
            t.last_input = used.input;
            // A reply with no calls that ran out of room is marked, for the panel to offer its rest.
            if cut && calls.is_empty() {
                t.items.push(Item::Cut);
            }
            // The import the latest message was sent from, as it read then.
            t.items.iter().rev().find_map(|i| match i {
                Item::User { import, .. } => Some(import.clone()),
                _ => None,
            }).flatten()
        }).map_err(other)?;
        if calls.is_empty() {
            if cut {
                emit(&job.thread_id, json!({ "type": "cut" }));
            }
            return Ok(());
        }
        let import = import.as_ref();
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
            let unrun = |error: &str| tools::Outcome { output: json!({ "error": error }), step: String::new(), proposal: None };
            let outcome = match serde_json::from_str::<Value>(&call.arguments) {
                // Written as the reply ran out of room: it may be cut off anywhere, even where it
                // still reads as whole.
                _ if cut => unrun(CUT_CALL),
                Ok(args) if args.is_object() && job.bank_texts && phone::answers(&call.name) => {
                    phone::outcome(&call.name, phone::ask(assistant, &job.thread_id, &call.name, &args).await)
                }
                // Offered when the conversation began, and not answered here now.
                Ok(args) if args.is_object() && phone::answers(&call.name) => unrun("Bank texts can't be read here right now: Bank messages isn't set up for this budget on this device."),
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
            let replaced = assistant
                .with_thread_guarded(&job.folder, &job.thread_id, Some(cancelled), |t| {
                    let mut replaced = None;
                    if let Some(p) = outcome.proposal {
                        // The card this one corrects is folded away, with the new one named on it.
                        let earlier = p.replaces.as_ref().and_then(|id| {
                            t.proposals.iter_mut().find(|q| &q.id == id && !q.made() && matches!(q.status, Status::Proposed | Status::Undone | Status::Failed))
                        });
                        if let Some(earlier) = earlier {
                            earlier.status = Status::Replaced;
                            earlier.replaced_by = Some(p.id.clone());
                            replaced = Some(history::proposal_view(earlier));
                        }
                        t.proposals.push(p);
                    }
                    t.items.push(Item::Result {
                        call_id: call.call_id.clone(),
                        output: outcome.output.to_string(),
                        step: outcome.step.clone(),
                        failed,
                        proposal: proposal_id,
                    });
                    replaced
                })
                .map_err(other)?;
            if !outcome.step.is_empty() {
                emit(&job.thread_id, json!({ "type": "step", "text": outcome.step, "failed": failed }));
            }
            if let Some(earlier) = replaced {
                emit(&job.thread_id, json!({ "type": "replaced", "proposal": earlier }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::llm::{script, Failure, Reply, Usage};
    use crate::assistant::testing::{finished, items, say, screen, scripted, view};
    use crate::assistant::{assistant_continue, assistant_retry, prompt};

    fn kinds(view: &Value) -> Vec<&str> {
        view["entries"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect()
    }

    #[test]
    fn a_lookup_then_an_answer_with_what_it_used_added_up() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = scripted(directory.path(), "script-lookup");
        let used = |input, output, cached| Usage { input, output, cached };
        script::play("script-lookup", vec![
            script::call("c1", "get_month", json!({})).map(|r| Reply { usage: used(900, 20, 0), ..r }),
            script::text("You have money ready to assign.").map(|r| Reply { usage: used(1500, 40, 800), ..r }),
        ]);
        let id = say(&runtime, None, "How is October?", screen());
        let shown = view(&runtime, &id);
        assert_eq!(kinds(&shown), ["user", "step", "assistant"]);
        assert_eq!(shown["usage"], json!({ "input": 2400, "output": 60, "cached": 800 }));
        assert_eq!((&shown["long"], &shown["entries"][2]["tools"]), (&json!(false), &json!(["get_month"])));
        // What went with the message is on it, for the panel.
        assert_eq!(shown["entries"][0]["with"]["page"], "budget");
        // The second request carried the lookup's answer back.
        let asked = script::asked("script-lookup");
        assert!(matches!(asked[1].items.last(), Some(Item::Result { failed: false, .. })));
    }

    #[test]
    fn the_last_step_is_asked_for_in_a_note_and_the_instructions_never_change() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = scripted(directory.path(), "script-rounds");
        let mut replies: Vec<_> = (0..ROUNDS - 1).map(|i| script::call(&format!("c{i}"), "get_setup", json!({}))).collect();
        replies.push(script::text("Here's what I found."));
        script::play("script-rounds", replies);
        let id = say(&runtime, None, "Look at everything", screen());
        let asked = script::asked("script-rounds");
        assert_eq!(asked.len(), ROUNDS);
        assert!(asked.iter().all(|a| a.instructions == asked[0].instructions && a.tools == asked[0].tools));
        assert!(asked[..ROUNDS - 1].iter().all(|a| !a.answer_only && !a.items.iter().any(|i| matches!(i, Item::Note { .. }))));
        let last = &asked[ROUNDS - 1];
        assert!(last.answer_only && matches!(last.items.last(), Some(Item::Note { text }) if text == LAST));
        assert_eq!(kinds(&view(&runtime, &id)).last(), Some(&"assistant"));
    }

    #[test]
    fn a_reply_cut_off_at_its_limit_runs_no_call_and_is_asked_for_its_rest() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = scripted(directory.path(), "script-cut");
        script::play("script-cut", vec![
            // A call written as the reply ran out of room, even one that reads as whole.
            script::call("c1", "propose_budget_moves", json!({ "month": "2026-10", "moves": [{ "from": "ready_to_assign", "to": "food", "amount": 5 }] })).map(|r| Reply { cut: true, ..r }),
            script::text("Your groceries in October were").map(|r| Reply { cut: true, ..r }),
            script::text("1,240.00 in all."),
        ]);
        let id = say(&runtime, None, "Cover groceries", screen());
        let kept = items(&runtime, &id);
        assert!(kept.iter().any(|i| matches!(i, Item::Result { output, failed: true, proposal: None, .. } if output.contains("length limit"))));
        let shown = view(&runtime, &id);
        assert_eq!(kinds(&shown), ["user", "assistant", "cut"]);
        // Asking for the rest says so in a note, and the conversation doesn't end on the model's words.
        assistant_retry(&runtime.simplefin, &runtime.db, &runtime.assistant, None, id.clone(), screen()).unwrap();
        finished(&runtime, &id);
        let asked = script::asked("script-cut");
        assert!(matches!(asked[2].items.last(), Some(Item::Note { text }) if text.contains("Carry on")));
        assert_eq!(kinds(&view(&runtime, &id)), ["user", "assistant", "cut", "assistant"]);
    }

    #[test]
    fn refused_thinking_is_left_out_once_and_a_full_conversation_is_carried_on() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = scripted(directory.path(), "script-refused");
        script::play("script-refused", vec![script::text("Hello.")]);
        let id = say(&runtime, None, "Hi", screen());
        // A reply kept from Claude, with its thinking.
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        runtime.assistant.with_thread(&folder, &id, |t| t.items.push(Item::Raw { provider: "anthropic".into(), value: json!([{ "type": "thinking", "thinking": "", "signature": "sig" }, { "type": "text", "text": "Hello." }]) })).unwrap();
        let refused = Failure { kind: FailureKind::Other, message: "x".into(), detail: "invalid_request_error messages.1.content.0: Invalid `signature` in `thinking` block.".into() };
        script::play("script-refused", vec![Err(refused), script::text("Still here."), Err(Failure::new(FailureKind::Full, "Too long."))]);
        say(&runtime, Some(&id), "Are you there?", screen());
        let asked = script::asked("script-refused");
        assert_eq!(asked.len(), 3);
        assert!(asked[1].items.iter().any(|i| matches!(i, Item::Raw { value, .. } if value.to_string().contains("signature"))));
        assert!(!asked[2].items.iter().any(|i| matches!(i, Item::Raw { value, .. } if value.to_string().contains("signature"))));
        assert_eq!(kinds(&view(&runtime, &id)).last(), Some(&"assistant"));

        // Out of room: the error offers a new conversation, which begins with what was said last.
        say(&runtime, Some(&id), "And now?", screen());
        let shown = view(&runtime, &id);
        let last = shown["entries"].as_array().unwrap().last().unwrap();
        assert_eq!((last["code"].as_str(), last["action"].as_str()), (Some("full"), Some("newConversation")));
        let next = assistant_continue(&runtime.db, &runtime.assistant, id).unwrap();
        script::play("script-refused", vec![script::text("Carrying on.")]);
        say(&runtime, Some(&next), "Where were we?", screen());
        let first = script::asked("script-refused").last().unwrap().items[0].clone();
        assert!(matches!(first, Item::User { context, .. } if context.contains("carries on an earlier one") && context.contains("The user: Are you there?")));
    }

    #[test]
    fn a_proposal_made_in_place_of_another_folds_the_earlier_one_away() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = scripted(directory.path(), "script-replaces");
        let moves = |amount: u32| json!({ "month": "2026-10", "moves": [{ "from": "ready_to_assign", "to": "food", "amount": amount }] });
        script::play("script-replaces", vec![script::call("c1", "propose_budget_moves", moves(10)), script::text("Prepared.")]);
        let id = say(&runtime, None, "Give food 10", screen());
        let first = view(&runtime, &id)["entries"][2]["proposal"]["id"].as_str().unwrap().to_owned();
        let mut corrected = moves(25);
        corrected["replaces"] = json!(first);
        script::play("script-replaces", vec![script::call("c2", "propose_budget_moves", corrected), script::text("Prepared again.")]);
        say(&runtime, Some(&id), "Make it 25", screen());
        let shown = view(&runtime, &id);
        let cards: Vec<&Value> = shown["entries"].as_array().unwrap().iter().filter(|e| e["kind"] == "proposal").map(|e| &e["proposal"]).collect();
        assert_eq!((cards[0]["status"].as_str(), cards[0]["actions"].clone()), (Some("replaced"), json!(["restore"])));
        assert_eq!((cards[0]["replacedBy"].as_str(), cards[1]["status"].as_str()), (cards[1]["id"].as_str(), Some("proposed")));
        // The next message tells the model what became of each.
        script::play("script-replaces", vec![script::text("Yes.")]);
        say(&runtime, Some(&id), "Is that right?", screen());
        let asked = script::asked("script-replaces");
        assert!(matches!(asked.last().unwrap().items.iter().rev().find(|i| matches!(i, Item::User { .. })), Some(Item::User { context, .. }) if context.contains("replaced by a newer proposal")));
        // The tool says how to name the one it corrects.
        assert!(tools::specs().iter().filter(|s| s.name.starts_with("propose_") && s.name != "propose_bank_texts").all(|s| s.parameters["properties"]["replaces"].is_object()));
    }

    #[test]
    fn the_tools_offered_are_the_ones_the_conversation_began_with() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = scripted(directory.path(), "script-tools");
        let with_texts = prompt::Screen { bank_texts: true, ..screen() };
        // Begun without bank texts: setting them up later doesn't change its tools.
        script::play("script-tools", vec![script::text("Hello."), script::text("Still no texts here.")]);
        let id = say(&runtime, None, "Hi", screen());
        say(&runtime, Some(&id), "And my texts?", with_texts.clone());
        let asked = script::asked("script-tools");
        assert!(asked[0].tools == asked[1].tools && !asked[1].tools.contains(&"get_bank_texts"));
        // Begun with them and asked from where they aren't read: offered still, and answered plainly.
        script::play("script-tools", vec![script::text("Hello."), script::call("c1", "get_bank_texts", json!({})), script::text("I can't read them here.")]);
        let id = say(&runtime, None, "Hi", prompt::Screen { bank_texts: true, ..with_texts });
        // The first reply never asked the phone, so nothing waited on it.
        say(&runtime, Some(&id), "What's waiting?", screen());
        let asked = script::asked("script-tools");
        assert!(asked[3].tools.contains(&"get_bank_texts"));
        assert!(items(&runtime, &id).iter().any(|i| matches!(i, Item::Result { output, failed: true, .. } if output.contains("isn't set up"))));
    }
}
