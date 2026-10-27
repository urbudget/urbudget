//! OpenAI's Responses API, used both with a ChatGPT sign-in (the plan pays) and with an API key.
//! The ChatGPT route asks for `store: false` and streaming, takes guidance only as
//! `instructions`, and wants tools grouped in a namespace; the same request works with a key.
use super::attachments::Kind;
use super::history::Item;
use super::llm::{excerpt, user_text, Call, Credentials, Delta, Failure, FailureKind, Model, Part, Provider, Reply, Request, Sse};
use serde_json::{json, Value};

pub const API: &str = "https://api.openai.com/v1";
const NAMESPACE: &str = "budget";

fn tools(request: &Request, flat: bool) -> Value {
    let functions: Vec<Value> = request
        .tools
        .iter()
        .map(|s| json!({ "type": "function", "name": s.name, "description": s.description, "parameters": s.parameters, "strict": false }))
        .collect();
    if flat {
        json!(functions)
    } else {
        json!([{
            "type": "namespace",
            "name": NAMESPACE,
            "description": "Look up and propose changes to the user's budget in UrBudget.",
            "tools": functions,
        }])
    }
}

/// The conversation as Responses input items. Reasoning records are only sent back within the
/// turn that made them.
fn input(request: &Request, flat: bool) -> Result<Vec<Value>, Failure> {
    let last_user = request.items.iter().rposition(|i| matches!(i, Item::User { .. }));
    let mut out = vec![];
    for (index, item) in request.items.iter().enumerate() {
        match item {
            Item::User { attachments, .. } => {
                let mut content = vec![json!({ "type": "input_text", "text": user_text(item, request.files) })];
                for meta in attachments {
                    let data = || {
                        request.files.base64(&meta.id).map_err(|e| Failure::new(FailureKind::Other, e))
                    };
                    match meta.kind {
                        Kind::Image => content.push(json!({
                            "type": "input_image",
                            "image_url": format!("data:{};base64,{}", meta.mime, data()?),
                            "detail": "auto",
                        })),
                        Kind::Pdf => content.push(json!({
                            "type": "input_file",
                            "filename": meta.name,
                            "file_data": format!("data:application/pdf;base64,{}", data()?),
                        })),
                        _ => {}
                    }
                }
                out.push(json!({ "type": "message", "role": "user", "content": content }));
            }
            Item::Assistant { text } if !text.is_empty() => out.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }],
            })),
            Item::Call { call_id, name, arguments } => {
                let mut call = json!({ "type": "function_call", "call_id": call_id, "name": name, "arguments": arguments });
                if !flat {
                    call["namespace"] = json!(NAMESPACE);
                }
                out.push(call);
            }
            Item::Result { call_id, output, .. } => {
                out.push(json!({ "type": "function_call_output", "call_id": call_id, "output": output }))
            }
            Item::Raw { provider, value } if provider == "openai" && last_user.is_some_and(|u| index > u) => {
                out.push(value.clone())
            }
            _ => {}
        }
    }
    Ok(out)
}

pub async fn respond(
    client: &reqwest::Client,
    credentials: &Credentials,
    request: &Request<'_>,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Reply, Failure> {
    // Reasoning summaries show what the model is working on; encrypted reasoning carries its
    // thinking between tool calls. Either is dropped if the model or route refuses it.
    let (mut flat, mut include, mut summary) = (false, true, true);
    for _ in 0..4 {
        let mut body = json!({
            "model": request.model,
            "instructions": request.instructions,
            "input": input(request, flat)?,
            "tools": tools(request, flat),
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "stream": true,
            "store": false,
        });
        if include {
            body["include"] = json!(["reasoning.encrypted_content"]);
        }
        if summary {
            body["reasoning"] = json!({ "summary": "auto" });
        }
        match stream(client, credentials, &body, on_delta).await {
            // Some routes or models reject a feature; drop it and ask again.
            Err(f) if f.kind == FailureKind::Unsupported || f.detail.contains("invalid_request") => {
                let about = format!("{} {}", f.detail, f.message).to_lowercase();
                if !flat && (about.contains("namespace") || about.contains("tools")) {
                    flat = true;
                } else if summary && about.contains("summar") {
                    summary = false;
                } else if include && (about.contains("include") || about.contains("encrypted")) {
                    include = false;
                } else if (summary || include) && about.contains("reasoning") {
                    summary = false;
                    include = false;
                } else {
                    return Err(f);
                }
            }
            other => return other,
        }
    }
    Err(Failure::new(FailureKind::Unsupported, "This model doesn't support what the assistant needs. Choose another model in Settings."))
}

async fn stream(
    client: &reqwest::Client,
    credentials: &Credentials,
    body: &Value,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Reply, Failure> {
    let mut response = client
        .post(format!("{API}/responses"))
        .bearer_auth(&credentials.secret)
        .header("accept", "text/event-stream")
        .json(body)
        .send()
        .await
        .map_err(Failure::network)?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(failure(credentials.provider, status, &text));
    }
    let mut sse = Sse::default();
    let mut reply = Reply::default();
    let mut completed = false;
    while let Some(chunk) = response.chunk().await.map_err(Failure::network)? {
        for (event, data) in sse.push(&chunk) {
            if data == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
            let kind = v["type"].as_str().unwrap_or(event.as_str()).to_owned();
            match kind.as_str() {
                "response.output_text.delta" => {
                    if let Some(d) = v["delta"].as_str() {
                        on_delta(Delta::Text(d));
                    }
                }
                "response.reasoning_summary_text.delta" => {
                    if let Some(d) = v["delta"].as_str() {
                        on_delta(Delta::Thinking(d));
                    }
                }
                "response.reasoning_summary_part.added" => on_delta(Delta::Thinking("\n\n")),
                "response.output_item.done" => {
                    let item = &v["item"];
                    match item["type"].as_str() {
                        Some("message") => {
                            let text: String = item["content"]
                                .as_array()
                                .map(|c| {
                                    c.iter()
                                        .filter_map(|p| p["text"].as_str().or(p["refusal"].as_str()))
                                        .collect::<Vec<_>>()
                                        .join("")
                                })
                                .unwrap_or_default();
                            if !text.is_empty() {
                                reply.parts.push(Part::Text(text));
                            }
                        }
                        Some("function_call") => {
                            let name = item["name"].as_str().unwrap_or_default();
                            let name = name.strip_prefix("budget.").or(name.strip_prefix("budget__")).unwrap_or(name);
                            reply.parts.push(Part::Call(Call {
                                call_id: item["call_id"].as_str().unwrap_or_default().to_owned(),
                                name: name.to_owned(),
                                arguments: item["arguments"].as_str().unwrap_or("{}").to_owned(),
                            }));
                        }
                        Some("reasoning") => reply.parts.push(Part::Raw(item.clone())),
                        _ => {}
                    }
                }
                "response.completed" => completed = true,
                "response.incomplete" => {
                    let reason = v["response"]["incomplete_details"]["reason"].as_str().unwrap_or("unknown").to_owned();
                    if reply.parts.iter().any(|p| matches!(p, Part::Text(_))) {
                        completed = true;
                    } else {
                        return Err(Failure::new(FailureKind::Other, format!("The reply stopped early ({}).", reason.replace('_', " "))));
                    }
                }
                "response.failed" => {
                    let e = &v["response"]["error"];
                    return Err(classify(credentials.provider, 0, e["code"].as_str().unwrap_or_default(), e["message"].as_str().unwrap_or("The model couldn't answer.")));
                }
                "error" => {
                    let e = if v["error"].is_object() { &v["error"] } else { &v };
                    return Err(classify(credentials.provider, 0, e["code"].as_str().unwrap_or_default(), e["message"].as_str().unwrap_or("The model couldn't answer.")));
                }
                _ => {}
            }
        }
    }
    if !completed {
        return Err(Failure::new(FailureKind::Busy, "The connection closed before the reply finished. Try again."));
    }
    Ok(reply)
}

fn failure(provider: Provider, status: u16, body: &str) -> Failure {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let e = &v["error"];
    let code = e["code"].as_str().or(e["type"].as_str()).unwrap_or_default();
    let mut message = e["message"].as_str().or(v["detail"].as_str()).map(str::to_owned).unwrap_or_else(|| excerpt(body));
    if let Some(param) = e["param"].as_str() {
        message = format!("{message} ({param})");
    }
    classify(provider, status, code, &message)
}

/// Turns the service's error into something the panel can act on.
pub fn classify(provider: Provider, status: u16, code: &str, message: &str) -> Failure {
    use FailureKind::*;
    let plan = provider == Provider::Chatgpt;
    let (kind, text) = if code.contains("usage_limit_exceeded") {
        (Limit, "You've reached your ChatGPT plan's usage limit.".to_string())
    } else if code.contains("user_not_eligible") {
        (NotEligible, "This ChatGPT account can't use its plan in other apps. ChatGPT Plus and Pro can.".to_string())
    } else if code.contains("client_not_enabled") {
        (NotEligible, "ChatGPT plan usage isn't turned on for UrBudget yet.".to_string())
    } else if code.contains("usage_unavailable") || code.contains("user_unavailable") || status == 503 || status >= 500 {
        (Busy, format!("{} is busy right now. Try again in a moment.", if plan { "ChatGPT" } else { "OpenAI" }))
    } else if code.contains("unsupported_capability") {
        (Unsupported, message.to_string())
    } else if code == "model_not_found" || status == 404 {
        (Model, "That model isn't available on this account. Choose another model in Settings.".to_string())
    } else if code == "invalid_token"
        || code == "invalid_api_key"
        || code.contains("invalid_user")
        || code.contains("scope_not_authorized")
        || code.contains("invalid_authorization_context")
        || status == 401
    {
        if plan {
            (SignIn, "Sign in with ChatGPT again to keep using the assistant.".to_string())
        } else {
            (SignIn, "OpenAI didn't accept the API key. Check it in Settings.".to_string())
        }
    } else if status == 429 || code == "rate_limit_exceeded" {
        (Limit, "Too many requests right now. Wait a minute and try again.".to_string())
    } else if status == 403 || code.contains("route_not_supported") {
        (NotEligible, format!("The request was refused: {message}"))
    } else {
        (Other, message.to_string())
    };
    Failure { kind, message: text, detail: format!("{code} {message}") }
}

pub async fn models(client: &reqwest::Client, credentials: &Credentials) -> Result<Vec<Model>, Failure> {
    let response = client
        .get(format!("{API}/models"))
        .bearer_auth(&credentials.secret)
        .send()
        .await
        .map_err(Failure::network)?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(failure(credentials.provider, status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| Failure::new(FailureKind::Other, "The model list couldn't be read."))?;
    // The ChatGPT route lists the account's models in order; an API key lists every model.
    if let Some(list) = v["models"].as_array() {
        return Ok(list
            .iter()
            .filter(|m| m["visibility"].as_str().is_none_or(|x| x == "list"))
            .filter_map(|m| {
                let id = m["slug"].as_str().or(m["id"].as_str())?;
                Some(Model { id: id.into(), name: m["display_name"].as_str().unwrap_or(id).into() })
            })
            .collect());
    }
    let mut out: Vec<Model> = v["data"]
        .as_array()
        .map(|l| {
            l.iter()
                .filter_map(|m| m["id"].as_str())
                .filter(|id| chat_model(id))
                .map(|id| Model { id: id.into(), name: id.into() })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(out)
}

/// General chat models from an API key's full list: no audio, image, embedding or search models.
fn chat_model(id: &str) -> bool {
    let general = id.starts_with("gpt-") && id[4..].starts_with(|c: char| c.is_ascii_digit());
    general
        && !["audio", "realtime", "transcribe", "tts", "image", "search", "embedding", "instruct", "codex", "preview"]
            .iter()
            .any(|x| id.contains(x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::attachments::Files;
    use crate::assistant::tools::specs;

    #[test]
    fn calls_and_results_go_back_with_the_namespace() {
        let items = vec![
            Item::User { text: "hi".into(), attachments: vec![], context: "Today".into(), at: String::new(), import: None },
            Item::Raw { provider: "openai".into(), value: json!({ "type": "reasoning", "id": "rs_1" }) },
            Item::Call { call_id: "c1".into(), name: "get_month".into(), arguments: "{}".into() },
            Item::Result { call_id: "c1".into(), output: "{}".into(), step: String::new(), failed: false, proposal: None },
        ];
        let files = Files::empty();
        let tools = specs();
        let request = Request { model: "m", instructions: "i", items: &items, files: &files, tools: &tools };
        let input = input(&request, false).unwrap();
        assert_eq!(input[0]["content"][0]["text"], "<context>\nToday\n</context>\n\nhi");
        assert_eq!(input[1]["type"], "reasoning");
        assert_eq!(input[2]["namespace"], "budget");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(super::tools(&request, false)[0]["type"], "namespace");
    }

    #[test]
    fn usage_limits_and_sign_in_problems_are_told_apart() {
        assert_eq!(classify(Provider::Chatgpt, 429, "subscription_sharing_usage_limit_exceeded", "").kind, FailureKind::Limit);
        assert_eq!(classify(Provider::Chatgpt, 401, "", "").kind, FailureKind::SignIn);
        assert_eq!(classify(Provider::Chatgpt, 503, "subscription_sharing_usage_unavailable", "").kind, FailureKind::Busy);
        assert!(chat_model("gpt-5.5") && !chat_model("gpt-4o-realtime-preview") && !chat_model("dall-e-3"));
    }
}
