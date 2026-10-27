//! Claude through Anthropic's Messages API, with the user's own API key. Replies are kept as
//! the content blocks Claude sent (thinking included) and sent back unchanged, which keeps the
//! conversation valid for models that check earlier turns.
use crate::{i18n::tr, t};
use super::attachments::Kind;
use super::history::Item;
use super::llm::{excerpt, user_text, Call, Credentials, Delta, Failure, FailureKind, Model, Part, Reply, Request, Sse};
use serde_json::{json, Value};
use std::collections::HashMap;

const API: &str = "https://api.anthropic.com/v1";
const VERSION: &str = "2023-06-01";
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Models that can hand a declined request to another model on the server.
fn has_fallback(model: &str) -> bool {
    model.starts_with("claude-opus-5") || model.starts_with("claude-fable-5") || model == "claude-sonnet-5-5"
}

/// Models whose thinking can come back as a readable summary, shown while Claude works.
fn summarizes(model: &str) -> bool {
    ["claude-opus-5", "claude-sonnet-5", "claude-fable-5", "claude-haiku-5", "claude-mythos-5"]
        .iter()
        .any(|p| model.starts_with(p))
}

/// After a fallback mid-reply, only text from before the switch is sent back.
fn replayable(blocks: &[Value]) -> Vec<Value> {
    let Some(boundary) = blocks.iter().rposition(|b| b["type"] == "fallback") else { return blocks.to_vec() };
    blocks
        .iter()
        .enumerate()
        .filter(|(i, b)| *i >= boundary || b["type"] == "text")
        .map(|(_, b)| b.clone())
        .collect()
}

fn messages(request: &Request) -> Result<Vec<Value>, Failure> {
    let mut out: Vec<Value> = vec![];
    fn push(out: &mut Vec<Value>, role: &str, mut blocks: Vec<Value>) {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut().filter(|m| m["role"] == role) {
            if let Some(content) = last["content"].as_array_mut() {
                content.append(&mut blocks);
                return;
            }
        }
        out.push(json!({ "role": role, "content": blocks }));
    }
    // Text and calls already covered by a kept reply aren't rebuilt.
    let mut covered = false;
    let mut pending: Vec<Value> = vec![];
    for item in request.items {
        match item {
            Item::User { attachments, .. } => {
                push(&mut out, "assistant", std::mem::take(&mut pending));
                covered = false;
                let mut blocks = vec![];
                for meta in attachments {
                    let data = || request.files.base64(&meta.id).map_err(|e| Failure::new(FailureKind::Other, e));
                    match meta.kind {
                        Kind::Image => blocks.push(json!({
                            "type": "image",
                            "source": { "type": "base64", "media_type": meta.mime, "data": data()? },
                        })),
                        Kind::Pdf => blocks.push(json!({
                            "type": "document",
                            "source": { "type": "base64", "media_type": "application/pdf", "data": data()? },
                            "title": meta.name,
                        })),
                        _ => {}
                    }
                }
                blocks.push(json!({ "type": "text", "text": user_text(item, request.files) }));
                push(&mut out, "user", blocks);
            }
            Item::Raw { provider, value } if provider == "anthropic" => {
                push(&mut out, "assistant", std::mem::take(&mut pending));
                let blocks = replayable(value.as_array().map(Vec::as_slice).unwrap_or_default());
                push(&mut out, "assistant", blocks);
                covered = true;
            }
            Item::Assistant { text } if !covered && !text.is_empty() => {
                pending.push(json!({ "type": "text", "text": text }))
            }
            Item::Call { call_id, name, arguments } if !covered => {
                let input: Value = serde_json::from_str(arguments).unwrap_or(json!({}));
                pending.push(json!({ "type": "tool_use", "id": call_id, "name": name, "input": input }));
            }
            Item::Result { call_id, output, failed, .. } => {
                push(&mut out, "assistant", std::mem::take(&mut pending));
                covered = false;
                let mut block = json!({ "type": "tool_result", "tool_use_id": call_id, "content": output });
                if *failed {
                    block["is_error"] = json!(true);
                }
                push(&mut out, "user", vec![block]);
            }
            _ => {}
        }
    }
    push(&mut out, "assistant", pending);
    Ok(out)
}

fn request_body(request: &Request) -> Result<Value, Failure> {
    let tools: Vec<Value> = request
        .tools
        .iter()
        .map(|s| json!({ "name": s.name, "description": s.description, "input_schema": s.parameters, "eager_input_streaming": true }))
        .collect();
    let mut body = json!({
        "model": request.model,
        "max_tokens": 32000,
        "system": request.instructions,
        "messages": messages(request)?,
        "tools": tools,
        "tool_choice": { "type": "auto" },
        "stream": true,
    });
    if has_fallback(request.model) {
        body["fallbacks"] = json!("default");
    }
    if summarizes(request.model) {
        body["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
    }
    Ok(body)
}

pub async fn respond(
    client: &reqwest::Client,
    credentials: &Credentials,
    request: &Request<'_>,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Reply, Failure> {
    let body = request_body(request)?;
    let mut call = client
        .post(format!("{API}/messages"))
        .header("x-api-key", &credentials.secret)
        .header("anthropic-version", VERSION)
        .header("accept", "text/event-stream")
        .json(&body);
    if has_fallback(request.model) {
        call = call.header("anthropic-beta", FALLBACK_BETA);
    }
    let mut response = call.send().await.map_err(Failure::network)?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(failure(status, &text));
    }
    let mut sse = Sse::default();
    let mut blocks: Vec<Value> = vec![];
    let mut partial: HashMap<usize, String> = HashMap::new();
    let mut stop_reason = String::new();
    let mut finished = false;
    while let Some(chunk) = response.chunk().await.map_err(Failure::network)? {
        for (_, data) in sse.push(&chunk) {
            let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
            let index = v["index"].as_u64().unwrap_or(0) as usize;
            match v["type"].as_str().unwrap_or_default() {
                "content_block_start" => {
                    while blocks.len() <= index {
                        blocks.push(Value::Null);
                    }
                    blocks[index] = v["content_block"].clone();
                }
                "content_block_delta" => {
                    let Some(block) = blocks.get_mut(index) else { continue };
                    let d = &v["delta"];
                    match d["type"].as_str().unwrap_or_default() {
                        "text_delta" => {
                            let t = d["text"].as_str().unwrap_or_default();
                            on_delta(Delta::Text(t));
                            let text = block["text"].as_str().unwrap_or_default().to_owned() + t;
                            block["text"] = json!(text);
                        }
                        "input_json_delta" => {
                            partial.entry(index).or_default().push_str(d["partial_json"].as_str().unwrap_or_default())
                        }
                        "thinking_delta" => {
                            let piece = d["thinking"].as_str().unwrap_or_default();
                            on_delta(Delta::Thinking(piece));
                            let text = block["thinking"].as_str().unwrap_or_default().to_owned() + piece;
                            block["thinking"] = json!(text);
                        }
                        "signature_delta" => {
                            let text = block["signature"].as_str().unwrap_or_default().to_owned()
                                + d["signature"].as_str().unwrap_or_default();
                            block["signature"] = json!(text);
                        }
                        _ => {}
                    }
                }
                "content_block_stop" => {
                    if let (Some(block), Some(json_text)) = (blocks.get_mut(index), partial.remove(&index)) {
                        if block["type"] == "tool_use" {
                            block["input"] = if json_text.trim().is_empty() {
                                json!({})
                            } else {
                                // A cut-off or invalid input still goes back as an object; the
                                // call itself is answered with an error below.
                                serde_json::from_str(&json_text).unwrap_or_else(|_| json!({ "_invalid": json_text }))
                            };
                        }
                    }
                }
                "message_delta" => {
                    if let Some(r) = v["delta"]["stop_reason"].as_str() {
                        stop_reason = r.to_owned();
                    }
                }
                "message_stop" => finished = true,
                "error" => {
                    let e = &v["error"];
                    let kind = e["type"].as_str().unwrap_or_default();
                    let message = e["message"].as_str().unwrap_or(tr("Claude couldn't answer."));
                    return Err(classify(if kind == "overloaded_error" { 529 } else { 0 }, kind, message));
                }
                _ => {}
            }
        }
    }
    if stop_reason == "refusal" {
        return Err(Failure::new(FailureKind::Refused, t!("Claude declined this request. Try rephrasing it.")));
    }
    if !finished {
        return Err(Failure::new(FailureKind::Busy, t!("The connection closed before the reply finished. Try again.")));
    }
    blocks.retain(|b| !b.is_null());
    let boundary = blocks.iter().rposition(|b| b["type"] == "fallback").map_or(0, |i| i + 1);
    let mut reply = Reply::default();
    reply.parts.push(Part::Raw(json!(blocks)));
    let text: String = blocks[boundary..].iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("");
    if !text.is_empty() {
        reply.parts.push(Part::Text(text));
    }
    for b in blocks[boundary..].iter().filter(|b| b["type"] == "tool_use") {
        let arguments = if b["input"].get("_invalid").is_some() { "not json".to_string() } else { b["input"].to_string() };
        reply.parts.push(Part::Call(Call {
            call_id: b["id"].as_str().unwrap_or_default().to_owned(),
            name: b["name"].as_str().unwrap_or_default().to_owned(),
            arguments,
        }));
    }
    Ok(reply)
}

fn failure(status: u16, body: &str) -> Failure {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let kind = v["error"]["type"].as_str().unwrap_or_default();
    let message = v["error"]["message"].as_str().map(str::to_owned).unwrap_or_else(|| excerpt(body));
    classify(status, kind, &message)
}
fn classify(status: u16, kind: &str, message: &str) -> Failure {
    use FailureKind::*;
    let (k, text) = match (status, kind) {
        (401, _) | (_, "authentication_error") => (SignIn, t!("Anthropic didn't accept the API key. Check it in Settings.")),
        (403, _) | (_, "permission_error") => (NotEligible, t!("This API key can't do that: {message}", message)),
        (404, _) | (_, "not_found_error") => (Model, t!("That Claude model isn't available to this API key. Choose another in Settings.")),
        (429, _) | (_, "rate_limit_error") => (Limit, t!("Anthropic's rate limit was reached. Wait a minute and try again.")),
        (529, _) | (_, "overloaded_error") => (Busy, t!("Claude is busy right now. Try again in a moment.")),
        (s, _) if s >= 500 => (Busy, t!("Anthropic's service had a problem. Try again in a moment.")),
        _ => (Other, message.to_string()),
    };
    Failure { kind: k, message: text, detail: format!("{kind} {message}") }
}

pub async fn models(client: &reqwest::Client, credentials: &Credentials) -> Result<Vec<Model>, Failure> {
    let response = client
        .get(format!("{API}/models?limit=100"))
        .header("x-api-key", &credentials.secret)
        .header("anthropic-version", VERSION)
        .send()
        .await
        .map_err(Failure::network)?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(failure(status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| Failure::new(FailureKind::Other, t!("The model list couldn't be read.")))?;
    Ok(v["data"]
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?;
                    Some(Model { id: id.into(), name: m["display_name"].as_str().unwrap_or(id).into() })
                })
                .collect()
        })
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::attachments::Files;
    use crate::assistant::tools::specs;

    #[test]
    fn a_kept_reply_is_sent_back_verbatim_and_results_follow() {
        let raw = json!([
            { "type": "thinking", "thinking": "", "signature": "sig" },
            { "type": "text", "text": "Checking." },
            { "type": "tool_use", "id": "toolu_1", "name": "get_month", "input": {} }
        ]);
        let items = vec![
            Item::User { text: "hi".into(), attachments: vec![], context: String::new(), at: String::new(), import: None },
            Item::Raw { provider: "anthropic".into(), value: raw.clone() },
            Item::Assistant { text: "Checking.".into() },
            Item::Call { call_id: "toolu_1".into(), name: "get_month".into(), arguments: "{}".into() },
            Item::Result { call_id: "toolu_1".into(), output: "{}".into(), step: String::new(), failed: false, proposal: None },
        ];
        let files = Files::empty();
        let tools = specs();
        let request = Request { model: DEFAULT_MODEL, instructions: "i", items: &items, files: &files, tools: &tools };
        let body = request_body(&request).unwrap();
        let m = body["messages"].as_array().unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[1]["content"], raw);
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["thinking"]["display"], "summarized");
        assert_eq!(body["tools"][0]["eager_input_streaming"], true);
    }

    #[test]
    fn calls_from_another_service_are_rebuilt() {
        let items = vec![
            Item::User { text: "hi".into(), attachments: vec![], context: String::new(), at: String::new(), import: None },
            Item::Raw { provider: "openai".into(), value: json!({ "type": "reasoning" }) },
            Item::Call { call_id: "call_1".into(), name: "get_month".into(), arguments: "{\"month\":\"2026-10\"}".into() },
            Item::Result { call_id: "call_1".into(), output: "{}".into(), step: String::new(), failed: true, proposal: None },
        ];
        let files = Files::empty();
        let tools = specs();
        let request = Request { model: "claude-haiku-5-5", instructions: "i", items: &items, files: &files, tools: &tools };
        let body = request_body(&request).unwrap();
        assert_eq!(body["messages"][1]["content"][0]["input"]["month"], "2026-10");
        assert_eq!(body["messages"][2]["content"][0]["is_error"], true);
        assert!(body.get("fallbacks").is_none());
    }

    #[test]
    fn only_text_before_a_fallback_is_replayed() {
        let blocks = vec![
            json!({ "type": "thinking", "signature": "a" }),
            json!({ "type": "text", "text": "partial" }),
            json!({ "type": "fallback" }),
            json!({ "type": "thinking", "signature": "b" }),
        ];
        let kept = replayable(&blocks);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0]["type"], "text");
    }
}
