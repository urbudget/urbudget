//! A model the user runs themselves (Ollama, LM Studio, llama.cpp's server, vLLM and the like),
//! through the chat completions API these servers share with OpenAI. The address is the user's
//! and no key is sent. The servers differ in what else they take, so a request asks only for
//! what they all have: messages, function tools and streaming. Images go as image parts; PDFs
//! can't be read this way, so the model is told one was attached and left out.
use crate::t;
use super::attachments::Kind;
use super::history::Item;
use super::llm::{excerpt, user_text, Call, Credentials, Delta, Failure, FailureKind, Model, Part, Reply, Request, Sse, Usage};
use serde_json::{json, Value};

/// Ollama's address, shown as an example of what to type.
pub const EXAMPLE: &str = "http://localhost:11434/v1";

/// The addresses worth trying for what the user typed: with `http://` when no scheme is given,
/// without a trailing slash, and the same with `/v1` added (where Ollama, LM Studio and
/// llama.cpp serve the API) when it lacks it.
pub fn addresses(typed: &str) -> Result<Vec<String>, String> {
    let invalid = || t!("Type the server's address, like {example}.", example = EXAMPLE);
    let mut text = typed.trim().to_owned();
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    if !text.contains("://") {
        text = format!("http://{text}");
    }
    let url = reqwest::Url::parse(&text).map_err(|_| invalid())?;
    // Kept in the settings file, so no name and password in it; and nothing a path can't carry.
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let base = url.as_str().trim_end_matches('/').to_owned();
    let mut out = vec![base.clone()];
    if !base.ends_with("/v1") {
        out.push(format!("{base}/v1"));
    }
    Ok(out)
}

/// The user's message as text, and its images. A PDF is named but not sent.
fn user_message(item: &Item, request: &Request) -> Result<Value, Failure> {
    let Item::User { attachments, .. } = item else { return Ok(Value::Null) };
    let mut shown = item.clone();
    if let Item::User { attachments, .. } = &mut shown {
        attachments.retain(|m| !matches!(m.kind, Kind::Pdf));
    }
    let mut text = user_text(&shown, request.files);
    for meta in attachments.iter().filter(|m| matches!(m.kind, Kind::Pdf)) {
        text.push_str(&format!(
            "\n\nAttachment {} \u{201c}{}\u{201d} is a PDF, which this model can't be sent. Ask for its figures as text or a CSV file if you need them.",
            meta.id, meta.name
        ));
    }
    let images: Vec<_> = attachments.iter().filter(|m| matches!(m.kind, Kind::Image)).collect();
    if images.is_empty() {
        return Ok(json!(text));
    }
    let mut content = vec![json!({ "type": "text", "text": text })];
    for meta in images {
        let data = request.files.base64(&meta.id).map_err(|e| Failure::new(FailureKind::Other, e))?;
        content.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{};base64,{data}", meta.mime) } }));
    }
    Ok(json!(content))
}

/// The conversation as chat messages: the instructions first, a round's text and calls as one
/// assistant message, and each result as a tool message. Some models' templates want the roles
/// to alternate, so text for the user's turn that follows another is joined to it.
fn messages(request: &Request) -> Result<Vec<Value>, Failure> {
    let mut out = vec![json!({ "role": "system", "content": request.instructions })];
    let mut said = String::new();
    let mut calls: Vec<Value> = vec![];
    fn flush(out: &mut Vec<Value>, said: &mut String, calls: &mut Vec<Value>) {
        if said.is_empty() && calls.is_empty() {
            return;
        }
        let mut message = json!({ "role": "assistant", "content": std::mem::take(said) });
        if !calls.is_empty() {
            message["tool_calls"] = json!(std::mem::take(calls));
        }
        out.push(message);
    }
    fn user(out: &mut Vec<Value>, content: Value) {
        if let (Some(last), Some(text)) = (out.last_mut().filter(|m| m["role"] == "user"), content.as_str()) {
            if let Some(before) = last["content"].as_str() {
                last["content"] = json!(format!("{before}\n\n{text}"));
                return;
            }
        }
        out.push(json!({ "role": "user", "content": content }));
    }
    for item in request.items {
        match item {
            Item::User { .. } => {
                flush(&mut out, &mut said, &mut calls);
                user(&mut out, user_message(item, request)?);
            }
            Item::Note { text } => {
                flush(&mut out, &mut said, &mut calls);
                user(&mut out, json!(super::llm::reminder(text)));
            }
            Item::Assistant { text } if !text.is_empty() => {
                if !said.is_empty() {
                    said.push_str("\n\n");
                }
                said.push_str(text);
            }
            Item::Call { call_id, name, arguments } => {
                // Servers read earlier calls' arguments as JSON to lay out the conversation.
                let arguments = if serde_json::from_str::<Value>(arguments).is_ok_and(|v| v.is_object()) { arguments.clone() } else { "{}".into() };
                calls.push(json!({ "id": call_id, "type": "function", "function": { "name": name, "arguments": arguments } }));
            }
            Item::Result { call_id, output, .. } => {
                flush(&mut out, &mut said, &mut calls);
                out.push(json!({ "role": "tool", "tool_call_id": call_id, "content": output }));
            }
            _ => {}
        }
    }
    flush(&mut out, &mut said, &mut calls);
    Ok(out)
}

fn request_body(request: &Request) -> Result<Value, Failure> {
    let mut body = json!({
        "model": request.model,
        "messages": messages(request)?,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    // The last round gets no tools at all: not every server honours "none".
    if !request.answer_only {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|s| json!({ "type": "function", "function": { "name": s.name, "description": s.description, "parameters": s.parameters } }))
            .collect();
        body["tools"] = json!(tools);
        body["tool_choice"] = json!("auto");
    }
    Ok(body)
}

pub async fn respond(
    client: &reqwest::Client,
    credentials: &Credentials,
    request: &Request<'_>,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Reply, Failure> {
    let mut body = request_body(request)?;
    match stream(client, credentials, &body, on_delta).await {
        // An older server that doesn't know the usage option is asked again without it.
        Err(f) if f.detail.contains("stream_options") => {
            body.as_object_mut().map(|b| b.remove("stream_options"));
            stream(client, credentials, &body, on_delta).await
        }
        other => other,
    }
}

/// A call as its pieces arrive.
#[derive(Default)]
struct Pending {
    id: String,
    name: String,
    arguments: String,
}

async fn stream(
    client: &reqwest::Client,
    credentials: &Credentials,
    body: &Value,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Reply, Failure> {
    let address = &credentials.address;
    let mut response = client
        .post(format!("{address}/chat/completions"))
        .header("accept", "text/event-stream")
        .json(body)
        .send()
        .await
        .map_err(|e| not_reached(address, e))?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(failure(status, &text));
    }
    let mut sse = Sse::default();
    let mut think = Think::default();
    let mut text = String::new();
    let mut calls: Vec<Pending> = vec![];
    let mut usage = Usage::default();
    let mut finish = String::new();
    let (mut started, mut done) = (false, false);
    fn write(pieces: Vec<(bool, String)>, text: &mut String, on_delta: &mut (dyn FnMut(Delta) + Send)) {
        for (thinking, piece) in pieces {
            if thinking {
                on_delta(Delta::Thinking(&piece));
            } else {
                text.push_str(&piece);
                on_delta(Delta::Text(&piece));
            }
        }
    }
    while let Some(chunk) = response.chunk().await.map_err(Failure::network)? {
        for (_, data) in sse.push(&chunk) {
            if data.trim() == "[DONE]" {
                done = true;
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
            if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
                let message = e["message"].as_str().or(e.as_str()).unwrap_or_default();
                return Err(classify(0, message));
            }
            if !started {
                started = true;
                on_delta(Delta::Started);
            }
            if v["usage"].is_object() {
                let u = &v["usage"];
                usage = Usage {
                    input: u["prompt_tokens"].as_u64().unwrap_or(0),
                    output: u["completion_tokens"].as_u64().unwrap_or(0),
                    cached: u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0),
                };
            }
            let Some(choice) = v["choices"].get(0) else { continue };
            let d = &choice["delta"];
            // Servers that read a reasoning model's thinking out of its text send it apart.
            if let Some(thought) = d["reasoning_content"].as_str().or(d["reasoning"].as_str()).filter(|r| !r.is_empty()) {
                on_delta(Delta::Thinking(thought));
            }
            if let Some(piece) = d["content"].as_str() {
                write(think.push(piece), &mut text, on_delta);
            }
            for c in d["tool_calls"].as_array().map(Vec::as_slice).unwrap_or_default() {
                let id = c["id"].as_str().unwrap_or_default();
                let mut slot = c["index"].as_u64().map_or(calls.len().saturating_sub(1), |i| i as usize);
                // Some servers send each whole call on its own at index 0; a new id is a new call.
                if !id.is_empty() && calls.get(slot).is_some_and(|k| !k.id.is_empty() && k.id != id) {
                    slot = calls.iter().position(|k| k.id == id).unwrap_or(calls.len());
                }
                while calls.len() <= slot {
                    calls.push(Pending::default());
                }
                let call = &mut calls[slot];
                if !id.is_empty() {
                    call.id = id.to_owned();
                }
                if let Some(name) = c["function"]["name"].as_str().filter(|n| !n.is_empty()) {
                    call.name = name.to_owned();
                }
                // Most servers stream the arguments as text; a few send them as an object.
                match &c["function"]["arguments"] {
                    Value::String(piece) => {
                        call.arguments.push_str(piece);
                        on_delta(Delta::Arguments(piece));
                    }
                    Value::Object(_) => call.arguments = c["function"]["arguments"].to_string(),
                    _ => {}
                }
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                finish = reason.to_owned();
            }
        }
    }
    write(think.finish(), &mut text, on_delta);
    if !done && finish.is_empty() {
        return Err(Failure::new(FailureKind::Busy, t!("The connection closed before the reply finished. Try again.")));
    }
    let mut reply = Reply { usage, cut: finish == "length", ..Reply::default() };
    if !text.trim().is_empty() {
        reply.parts.push(Part::Text(text.trim().to_owned()));
    }
    for call in calls.into_iter().filter(|c| !c.name.is_empty()) {
        reply.parts.push(Part::Call(Call {
            call_id: if call.id.is_empty() { super::new_id("call") } else { call.id },
            name: call.name,
            arguments: if call.arguments.trim().is_empty() { "{}".into() } else { call.arguments },
        }));
    }
    if reply.parts.is_empty() && !reply.cut {
        return Err(Failure::new(FailureKind::Other, t!("The model sent an empty reply. Try again, or choose another model in Settings.")));
    }
    Ok(reply)
}

/// Splits `<think>…</think>` at the start of a reply off as thinking, for servers that leave a
/// reasoning model's thinking in its text. Each piece comes back marked as thinking or not.
#[derive(Default)]
struct Think {
    state: ThinkState,
    held: String,
}
#[derive(Default, PartialEq)]
enum ThinkState {
    /// Nothing but what could be the opening tag has arrived.
    #[default]
    Start,
    Inside,
    Outside,
}
impl Think {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";

    fn push(&mut self, piece: &str) -> Vec<(bool, String)> {
        let mut out = vec![];
        self.held.push_str(piece);
        loop {
            match self.state {
                ThinkState::Outside => {
                    if !self.held.is_empty() {
                        out.push((false, std::mem::take(&mut self.held)));
                    }
                    return out;
                }
                ThinkState::Start => {
                    let start = self.held.trim_start();
                    if let Some(rest) = start.strip_prefix(Self::OPEN) {
                        self.held = rest.to_owned();
                        self.state = ThinkState::Inside;
                    } else if Self::OPEN.starts_with(start) {
                        return out;
                    } else {
                        self.state = ThinkState::Outside;
                    }
                }
                ThinkState::Inside => {
                    if let Some(at) = self.held.find(Self::CLOSE) {
                        let rest = self.held[at + Self::CLOSE.len()..].trim_start().to_owned();
                        self.held.truncate(at);
                        if !self.held.is_empty() {
                            out.push((true, std::mem::take(&mut self.held)));
                        }
                        self.held = rest;
                        self.state = ThinkState::Outside;
                        continue;
                    }
                    // What could be the start of the closing tag waits for the next piece.
                    let keep = (1..Self::CLOSE.len()).rev().find(|n| self.held.ends_with(&Self::CLOSE[..*n])).unwrap_or(0);
                    let cut = self.held.len() - keep;
                    if cut > 0 {
                        out.push((true, self.held[..cut].to_owned()));
                        self.held.drain(..cut);
                    }
                    return out;
                }
            }
        }
    }

    /// What was held back when the reply ends.
    fn finish(&mut self) -> Vec<(bool, String)> {
        let held = std::mem::take(&mut self.held);
        if held.is_empty() { vec![] } else { vec![(self.state == ThinkState::Inside, held)] }
    }
}

fn not_reached(address: &str, e: reqwest::Error) -> Failure {
    if e.is_connect() {
        Failure::new(FailureKind::Busy, t!("Couldn't reach the model server at {address}. Check that it's running.", address))
    } else {
        Failure::network(e)
    }
}

fn failure(status: u16, body: &str) -> Failure {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let e = &v["error"];
    let message = e["message"]
        .as_str()
        .or(e.as_str())
        .or(v["message"].as_str())
        .or(v["detail"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| excerpt(body));
    classify(status, &message)
}

/// Turns a server's error into something the panel can act on. The servers word their errors
/// differently, so this goes by what the message says.
fn classify(status: u16, message: &str) -> Failure {
    use FailureKind::*;
    let about = message.to_lowercase();
    let unsupported = about.contains("not support") || about.contains("unsupported");
    let (kind, text) = if about.contains("context") && ["exceed", "too long", "too large", "overflow", "length", "size"].iter().any(|x| about.contains(x)) {
        (Full, t!("This conversation no longer fits what the model can read. Carry on in a new one, or give the model a longer context on its server."))
    } else if unsupported && about.contains("tool") {
        (Unsupported, t!("This model can't use tools, which the assistant needs. Choose another model in Settings."))
    } else if (unsupported && (about.contains("image") || about.contains("vision"))) || about.contains("mmproj") {
        (Unsupported, t!("This model can't read images. Remove the image, or choose a model that can."))
    } else if status == 404 || (about.contains("model") && about.contains("not found")) {
        (Model, t!("The server doesn't have that model. Choose another model in Settings."))
    } else if status == 429 || status == 503 {
        (Busy, t!("The model server is busy. Try again in a moment."))
    } else if message.is_empty() {
        (Other, t!("The model server couldn't answer."))
    } else {
        (Other, t!("The model server couldn't answer: {message}", message))
    };
    Failure { kind, message: text, detail: message.to_owned() }
}

/// The server's models, without those that only make embeddings.
pub async fn models(client: &reqwest::Client, credentials: &Credentials) -> Result<Vec<Model>, Failure> {
    let address = &credentials.address;
    let response = client.get(format!("{address}/models")).send().await.map_err(|e| not_reached(address, e))?;
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    let unreadable = || Failure::new(FailureKind::Other, t!("{address} didn't answer like a model server. Check the address.", address));
    // Nothing there: the address is wrong, not the model.
    if status == 404 {
        return Err(unreadable());
    }
    if !(200..300).contains(&status) {
        return Err(failure(status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| unreadable())?;
    let list = v["data"].as_array().ok_or_else(unreadable)?;
    let out: Vec<Model> = list
        .iter()
        .filter_map(|m| m["id"].as_str())
        .filter(|id| !id.to_lowercase().contains("embed"))
        .map(|id| Model { id: id.into(), name: id.into() })
        .collect();
    if out.is_empty() {
        return Err(Failure::new(FailureKind::Model, t!("The server at {address} has no models yet. Download one in it, then try again.", address)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::attachments::{Files, Meta};
    use crate::assistant::tools::specs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn user(text: &str) -> Item {
        Item::User { text: text.into(), attachments: vec![], context: String::new(), at: String::new(), import: None, sent_with: None }
    }

    #[test]
    fn addresses_get_a_scheme_and_a_v1_to_try() {
        assert_eq!(addresses(" localhost:11434/ ").unwrap(), ["http://localhost:11434", "http://localhost:11434/v1"]);
        assert_eq!(addresses("http://192.168.1.20:1234/v1").unwrap(), ["http://192.168.1.20:1234/v1"]);
        assert_eq!(addresses("https://models.home.arpa/api").unwrap(), ["https://models.home.arpa/api", "https://models.home.arpa/api/v1"]);
        for wrong in ["", "ftp://host", "http://user:pass@host", "http://host/?key=1", "local host"] {
            assert!(addresses(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_round_is_one_assistant_message_and_results_are_tool_messages() {
        let items = vec![
            user("How much on food?"),
            Item::Assistant { text: "Looking.".into() },
            Item::Call { call_id: "c1".into(), name: "get_month".into(), arguments: "{\"month\":\"2026-11\"}".into() },
            Item::Call { call_id: "c2".into(), name: "get_month".into(), arguments: "not json".into() },
            Item::Result { call_id: "c1".into(), output: "{}".into(), step: String::new(), failed: false, proposal: None },
            Item::Result { call_id: "c2".into(), output: "bad".into(), step: String::new(), failed: true, proposal: None },
            Item::Raw { provider: "openai".into(), value: json!({ "type": "reasoning" }) },
            Item::Note { text: "Answer now.".into() },
            Item::Assistant { text: "You spent 120.".into() },
            user("Thanks"),
            Item::Note { text: "Today is Sunday.".into() },
        ];
        let files = Files::empty();
        let tools = specs();
        let request = Request { model: "m", instructions: "Be brief.", items: &items, files: &files, tools: &tools, answer_only: false, kept: true };
        let m = messages(&request).unwrap();
        let roles: Vec<&str> = m.iter().map(|x| x["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "tool", "tool", "user", "assistant", "user"]);
        assert_eq!(m[2]["content"], "Looking.");
        assert_eq!(m[2]["tool_calls"][1]["function"]["arguments"], "{}");
        assert_eq!(m[3]["tool_call_id"], "c1");
        assert_eq!(m[7]["content"], "Thanks\n\n<system-reminder>\nToday is Sunday.\n</system-reminder>");
        let body = request_body(&request).unwrap();
        assert_eq!(body["tools"][0]["type"], "function");
        let last = Request { answer_only: true, ..request };
        assert!(request_body(&last).unwrap().get("tools").is_none());
    }

    #[test]
    fn a_pdf_is_named_but_not_sent() {
        let pdf = Meta { id: "a_1".into(), name: "statement.pdf".into(), mime: "application/pdf".into(), size: 10, kind: Kind::Pdf };
        let item = Item::User { text: "What's in it?".into(), attachments: vec![pdf], context: String::new(), at: String::new(), import: None, sent_with: None };
        let files = Files::empty();
        let request = Request { model: "m", instructions: "", items: std::slice::from_ref(&item), files: &files, tools: &[], answer_only: false, kept: true };
        let text = user_message(&item, &request).unwrap();
        let text = text.as_str().unwrap();
        assert!(text.starts_with("What's in it?") && text.contains("“statement.pdf” is a PDF, which this model can't be sent"), "{text}");
    }

    #[test]
    fn thinking_left_in_the_text_is_split_off() {
        let mut think = Think::default();
        let mut got = vec![];
        for piece in ["  <th", "ink>Let me", " add it up.</th", "ink>\n\nYou spent ", "120."] {
            got.extend(think.push(piece));
        }
        got.extend(think.finish());
        let thinking: String = got.iter().filter(|(t, _)| *t).map(|(_, s)| s.as_str()).collect();
        let text: String = got.iter().filter(|(t, _)| !*t).map(|(_, s)| s.as_str()).collect();
        assert_eq!((thinking.as_str(), text.as_str()), ("Let me add it up.", "You spent 120."));
        let mut plain = Think::default();
        let mut got = plain.push("<b>Hi</b>");
        got.extend(plain.finish());
        assert_eq!(got, [(false, "<b>Hi</b>".to_string())]);
    }

    #[test]
    fn errors_are_told_apart_by_what_they_say() {
        let kind = |status, message| classify(status, message).kind;
        assert_eq!(kind(400, "registry.ollama.ai/library/gemma3:4b does not support tools"), FailureKind::Unsupported);
        assert_eq!(kind(400, "the request exceeds the available context size, try increasing it"), FailureKind::Full);
        assert_eq!(kind(404, "model \"qwen3:32b\" not found, try pulling it first"), FailureKind::Model);
        assert_eq!(kind(500, "image input is not supported - hint: you may need to provide the mmproj"), FailureKind::Unsupported);
        assert_eq!(kind(503, "Loading model"), FailureKind::Busy);
        assert_eq!(kind(500, "boom"), FailureKind::Other);
    }

    /// A server that answers one request with `answer` and hands back what it was sent.
    fn serve(answer: String) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = format!("http://{}/v1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut reader = BufReader::new(&mut stream);
            let (mut length, mut head) = (0, String::new());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" { break; }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") { length = value.trim().parse().unwrap(); }
                head.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            stream.write_all(answer.as_bytes()).unwrap();
            format!("{head}\r\n{}", String::from_utf8(body).unwrap())
        });
        (address, handle)
    }
    fn sse(events: &[Value]) -> String {
        let body: String = events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>() + "data: [DONE]\n\n";
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    #[test]
    fn a_streamed_reply_brings_its_text_thinking_calls_and_usage() {
        let chunk = |delta: Value| json!({ "choices": [{ "index": 0, "delta": delta, "finish_reason": null }] });
        let (address, server) = serve(sse(&[
            chunk(json!({ "role": "assistant", "reasoning_content": "Food is a category." })),
            chunk(json!({ "content": "Checking " })),
            chunk(json!({ "content": "November." })),
            // Ollama's way: each whole call at index 0, told apart by its id.
            chunk(json!({ "tool_calls": [{ "index": 0, "id": "call_a", "type": "function", "function": { "name": "get_month", "arguments": "{\"month\":\"2026-11\"}" } }] })),
            chunk(json!({ "tool_calls": [{ "index": 0, "id": "call_b", "type": "function", "function": { "name": "list_categories", "arguments": "" } }] })),
            // The OpenAI way: the first piece has the id, the rest only the index.
            chunk(json!({ "tool_calls": [{ "index": 2, "id": "call_c", "function": { "name": "find_transactions", "arguments": "{\"payee\":" } }] })),
            chunk(json!({ "tool_calls": [{ "index": 2, "function": { "arguments": "\"Lulu\"}" } }] })),
            json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }),
            json!({ "choices": [], "usage": { "prompt_tokens": 9000, "completion_tokens": 40, "prompt_tokens_details": { "cached_tokens": 8000 } } }),
        ]));
        let items = vec![user("Food this month?")];
        let files = Files::empty();
        let tools = specs();
        let request = Request { model: "qwen3:32b", instructions: "Be brief.", items: &items, files: &files, tools: &tools, answer_only: false, kept: true };
        let credentials = Credentials { provider: super::super::llm::Provider::Local, secret: String::new(), address };
        let (mut shown, mut thought) = (String::new(), String::new());
        let reply = crate::block_on(respond(&reqwest::Client::new(), &credentials, &request, &mut |d| match d {
            Delta::Text(t) => shown.push_str(t),
            Delta::Thinking(t) => thought.push_str(t),
            _ => {}
        }))
        .unwrap();
        let sent = server.join().unwrap();
        assert!(sent.starts_with("POST /v1/chat/completions"), "{sent}");
        assert!(!sent.to_lowercase().contains("authorization"));
        assert!(sent.contains("\"model\":\"qwen3:32b\"") && sent.contains("\"include_usage\":true"));
        assert_eq!((shown.as_str(), thought.as_str()), ("Checking November.", "Food is a category."));
        assert_eq!(reply.parts[0], Part::Text("Checking November.".into()));
        let calls: Vec<(&str, &str, &str)> = reply.parts[1..]
            .iter()
            .map(|p| match p { Part::Call(c) => (c.call_id.as_str(), c.name.as_str(), c.arguments.as_str()), _ => panic!("{p:?}") })
            .collect();
        assert_eq!(calls, [
            ("call_a", "get_month", "{\"month\":\"2026-11\"}"),
            ("call_b", "list_categories", "{}"),
            ("call_c", "find_transactions", "{\"payee\":\"Lulu\"}"),
        ]);
        assert_eq!((reply.usage.input, reply.usage.output, reply.usage.cached, reply.cut), (9000, 40, 8000, false));
    }

    #[test]
    fn the_model_list_leaves_out_embedding_models_and_a_refused_server_says_so() {
        let list = json!({ "object": "list", "data": [{ "id": "qwen3:32b" }, { "id": "nomic-embed-text:latest" }, { "id": "gpt-oss:20b" }] }).to_string();
        let (address, server) = serve(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{list}", list.len()));
        let credentials = Credentials { provider: super::super::llm::Provider::Local, secret: String::new(), address };
        let listed = crate::block_on(models(&reqwest::Client::new(), &credentials)).unwrap();
        assert!(server.join().unwrap().starts_with("GET /v1/models"));
        assert_eq!(listed.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["qwen3:32b", "gpt-oss:20b"]);

        let closed = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        let credentials = Credentials { address: address.clone(), ..credentials };
        let refused = crate::block_on(models(&reqwest::Client::new(), &credentials)).unwrap_err();
        assert_eq!(refused.kind, FailureKind::Busy);
        assert!(refused.message.contains(&address), "{}", refused.message);
    }
}
