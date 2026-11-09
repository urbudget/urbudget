//! Bank-text tools the phone answers. Texts live in Android's own store and in its Messages app,
//! which only the phone can open, so a reply asks the phone (`assistant://phone`) and waits for
//! `assistant_phone_reply`. They're offered only when the phone says this budget reads bank texts.
use super::proposals::{Card, Proposal, Status};
use super::tools::Outcome;
use super::{lock, new_id, Assistant};
use crate::t;
use serde_json::{json, Value};
use std::time::Duration;

/// The tools the phone answers.
pub const TOOLS: [&str; 3] = ["get_bank_texts", "search_bank_texts", "propose_bank_texts"];
/// How long a reply waits for the phone.
const WAIT: Duration = Duration::from_secs(20);

pub fn answers(name: &str) -> bool {
    TOOLS.contains(&name)
}

/// Takes a request off the list when its reply stops waiting, answered or not.
struct Waiting<'a> {
    assistant: &'a Assistant,
    id: String,
}
impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        lock(&self.assistant.phone).remove(&self.id);
    }
}

/// Asks the phone and waits for its answer. Stopping the reply drops the wait.
pub async fn ask(assistant: &Assistant, thread_id: &str, tool: &str, args: &Value) -> Result<Value, String> {
    let id = new_id("phone");
    let (send, receive) = tokio::sync::oneshot::channel();
    lock(&assistant.phone).insert(id.clone(), send);
    let _waiting = Waiting { assistant, id: id.clone() };
    crate::host::emit("assistant://phone", json!({ "threadId": thread_id, "requestId": id, "tool": tool, "args": args }));
    match tokio::time::timeout(WAIT, receive).await {
        Ok(Ok(answer)) => Ok(answer),
        _ => Err(t!("The phone didn't answer. UrBudget needs to stay open while it looks at bank texts.")),
    }
}

/// The phone's answer to a request. One that no reply waits for any more is dropped.
pub fn reply(assistant: &Assistant, id: &str, answer: Value) {
    if let Some(send) = lock(&assistant.phone).remove(id) {
        let _ = send.send(answer);
    }
}

/// What the model and the panel get from the phone's answer: its output and step line, and for
/// `propose_bank_texts` a card the phone applies (`bankTexts`, with what to do in `bankTexts`).
pub fn outcome(tool: &str, answer: Result<Value, String>) -> Outcome {
    let failed = |error: String| Outcome {
        step: super::tools::failed_step(tool).to_owned(),
        output: json!({ "error": error }),
        proposal: None,
    };
    let answer = match answer {
        Ok(answer) => answer,
        Err(error) => return failed(error),
    };
    if let Some(error) = answer.get("error").and_then(Value::as_str) {
        return failed(error.to_owned());
    }
    let output = answer.get("output").cloned().unwrap_or(Value::Null);
    let step = answer.get("step").and_then(Value::as_str).unwrap_or_default().to_owned();
    if tool != "propose_bank_texts" {
        return Outcome { output, step, proposal: None };
    }
    let card: Card = match answer.get("card").cloned().map(serde_json::from_value) {
        Some(Ok(card)) => card,
        _ => return failed("The phone's card was incomplete.".into()),
    };
    let Some(apply) = answer.get("apply").filter(|a| a.is_object()).cloned() else {
        return failed("The phone didn't say what the card does.".into());
    };
    let title = answer.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
    let proposal = Proposal {
        id: new_id("p"),
        title,
        kind: "bankTexts".into(),
        status: Status::Proposed,
        change: None,
        import: None,
        report: None,
        workbook: None,
        fixes: None,
        saved: None,
        expected: None,
        card,
        summary: output.clone(),
        problem: None,
        review: None,
        bank_texts: Some(apply),
    };
    Outcome { output, step, proposal: Some(proposal) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_reaches_the_request_and_a_late_one_is_dropped() {
        let dir = std::env::temp_dir().join(new_id("phone-test"));
        let assistant = Assistant::new(dir);
        // The phone answers from another thread, as the app does.
        let phone = assistant.clone();
        let answering = std::thread::spawn(move || loop {
            let id = lock(&phone.phone).keys().next().cloned();
            if let Some(id) = id { reply(&phone, &id, json!({ "output": { "waiting": 3 }, "step": "Looked at bank messages" })); break; }
            std::thread::sleep(Duration::from_millis(2));
        });
        let asked = crate::block_on(ask(&assistant, "t1", "get_bank_texts", &json!({})));
        answering.join().unwrap();
        assert_eq!(asked.unwrap()["output"]["waiting"], 3);
        assert!(lock(&assistant.phone).is_empty());
        // Nothing waits for this one.
        reply(&assistant, "phone_gone", json!({}));
        assert!(lock(&assistant.phone).is_empty());
    }

    #[test]
    fn the_phone_answer_becomes_output_a_step_and_for_a_proposal_a_card() {
        let read = outcome("search_bank_texts", Ok(json!({ "output": { "texts": [] }, "step": "Searched ANB texts: none found" })));
        assert_eq!(read.output, json!({ "texts": [] }));
        assert_eq!(read.step, "Searched ANB texts: none found");
        assert!(read.proposal.is_none());

        let failed = outcome("get_bank_texts", Ok(json!({ "error": "Bank messages isn't set up for this budget." })));
        assert_eq!(failed.output["error"], "Bank messages isn't set up for this budget.");
        assert!(outcome("get_bank_texts", Err("no answer".into())).output.get("error").is_some());

        let card = json!({ "kicker": "Bank messages", "rows": [{ "label": "Read 23 waiting texts" }], "note": "They go to your review." });
        let proposed = outcome("propose_bank_texts", Ok(json!({
            "output": { "action": "read", "texts": 23 }, "step": "Prepared a card", "title": "Read waiting texts",
            "card": card, "apply": { "action": "read", "note": "" },
        })));
        let p = proposed.proposal.unwrap();
        assert_eq!(p.kind, "bankTexts");
        assert_eq!(p.bank_texts, Some(json!({ "action": "read", "note": "" })));
        assert_eq!(p.card.rows[0].label, "Read 23 waiting texts");
        // A card the phone didn't finish isn't offered.
        assert!(outcome("propose_bank_texts", Ok(json!({ "output": {}, "card": card }))).proposal.is_none());
    }
}
