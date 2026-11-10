//! Talking to a model: which service, what a reply is made of, and the server-sent events both
//! services stream. Each service's request and stream format lives in its own file.
use crate::t;
use super::attachments::{self, Files, Kind};
use super::history::Item;
use super::tools::Spec;
use super::{anthropic, openai};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// OpenAI models on the user's ChatGPT plan, through Sign in with ChatGPT.
    #[default]
    Chatgpt,
    /// OpenAI models with the user's own API key.
    Openai,
    /// Claude models with the user's own Anthropic API key.
    Anthropic,
}

/// What a request is sent with: the service and its bearer token or key.
pub struct Credentials {
    pub provider: Provider,
    pub secret: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}
/// One piece of a reply, in the order it arrived.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Call(Call),
    /// The service's own record of the reply, sent back verbatim next time.
    Raw(Value),
}
/// A piece of a reply as it streams in.
pub enum Delta<'a> {
    /// The service took the request and the model is working on it, before it has anything to show.
    Started,
    Text(&'a str),
    /// A readable summary of the model's reasoning while it works.
    Thinking(&'a str),
    /// A tool call's arguments as the model writes them.
    Arguments(&'a str),
}

/// What a request used, in tokens. `input` counts everything sent, `cached` the part of it the
/// service read from its cache, which costs a fraction of the rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}
impl Usage {
    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cached += other.cached;
    }
}

#[derive(Debug, Default)]
pub struct Reply {
    pub parts: Vec<Part>,
    pub usage: Usage,
    /// The reply stopped at its length limit: its text is unfinished, and a call it was writing
    /// is incomplete and must not run.
    pub cut: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// Sign in again, or the key was rejected.
    SignIn,
    /// The plan's usage limit or a rate limit.
    Limit,
    /// This account can't use the assistant this way.
    NotEligible,
    /// The service is busy; worth retrying.
    Busy,
    /// The model declined the request.
    Refused,
    /// Something in the request isn't supported on this route.
    Unsupported,
    /// The chosen model isn't available.
    Model,
    /// The conversation no longer fits what the model can read.
    Full,
    Other,
}
#[derive(Clone, Debug)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    /// The service's own code or parameter, for deciding how to retry.
    pub detail: String,
}
impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), detail: String::new() }
    }
    pub fn network(e: reqwest::Error) -> Self {
        let message = if e.is_timeout() {
            t!("The model took too long to answer. Try again.")
        } else if e.is_connect() {
            t!("Couldn't reach the model. Check your internet connection.")
        } else {
            t!("The connection to the model was interrupted. Try again.")
        };
        Self::new(FailureKind::Busy, message)
    }
}

pub struct Request<'a> {
    pub model: &'a str,
    pub instructions: &'a str,
    pub items: &'a [Item],
    pub files: &'a Files,
    pub tools: &'a [Spec],
    /// The reply's last step: the model answers with what it has, calling no tools.
    pub answer_only: bool,
    /// A conversation that will be sent again with more at its end, so what the service has read
    /// of it is worth keeping. A request made once and not again isn't: it would pay to be kept
    /// and never be read back.
    pub kept: bool,
}

/// Something the app tells the model in the middle of a conversation, as each service takes it:
/// text in the user's turn, marked as the app's own.
pub fn reminder(text: &str) -> String {
    format!("<system-reminder>\n{text}\n</system-reminder>")
}

/// Sends the conversation and streams the reply (and any reasoning summary) to `on_delta`.
pub async fn respond(
    client: &reqwest::Client,
    credentials: &Credentials,
    request: &Request<'_>,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Reply, Failure> {
    #[cfg(test)]
    if let Some(played) = script::next(request, on_delta) {
        return played;
    }
    match credentials.provider {
        Provider::Chatgpt | Provider::Openai => openai::respond(client, credentials, request, on_delta).await,
        Provider::Anthropic => anthropic::respond(client, credentials, request, on_delta).await,
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub name: String,
}

pub async fn models(client: &reqwest::Client, credentials: &Credentials) -> Result<Vec<Model>, Failure> {
    match credentials.provider {
        Provider::Chatgpt | Provider::Openai => openai::models(client, credentials).await,
        Provider::Anthropic => anthropic::models(client, credentials).await,
    }
}

/// The text of a user message: the screen note, what they typed, and previews of files that
/// aren't sent as images or documents.
pub fn user_text(item: &Item, files: &Files) -> String {
    let Item::User { text, attachments, context, .. } = item else { return String::new() };
    let mut parts = vec![];
    if !context.is_empty() {
        parts.push(format!("<context>\n{context}\n</context>"));
    }
    for meta in attachments.iter().filter(|m| !matches!(m.kind, Kind::Image | Kind::Pdf)) {
        parts.push(format!("<attachment>\n{}\n</attachment>", attachments::preview(files, meta)));
    }
    for meta in attachments.iter().filter(|m| matches!(m.kind, Kind::Image | Kind::Pdf)) {
        parts.push(format!("Attachment {} \u{201c}{}\u{201d} is included.", meta.id, meta.name));
    }
    parts.push(if text.trim().is_empty() { "(No message; see the attachments.)".to_string() } else { text.clone() });
    parts.join("\n\n")
}

/// Replies for tests of the loop, in place of a service: a test queues them under a model name
/// of its own, and `respond` plays the next one for a request that names that model.
#[cfg(test)]
pub mod script {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    /// A request as the loop made it.
    #[derive(Clone, Debug)]
    pub struct Asked {
        pub instructions: String,
        pub items: Vec<Item>,
        pub tools: Vec<&'static str>,
        pub answer_only: bool,
    }
    #[derive(Default)]
    struct Stage {
        replies: VecDeque<Result<Reply, Failure>>,
        asked: Vec<Asked>,
    }
    static STAGES: Mutex<Option<HashMap<String, Stage>>> = Mutex::new(None);

    fn stage<T>(model: &str, f: impl FnOnce(&mut Stage) -> T) -> T {
        let mut stages = STAGES.lock().unwrap_or_else(|e| e.into_inner());
        f(stages.get_or_insert_with(HashMap::new).entry(model.to_owned()).or_default())
    }
    /// Queues replies for requests that name `model`.
    pub fn play(model: &str, replies: Vec<Result<Reply, Failure>>) {
        stage(model, |s| s.replies.extend(replies));
    }
    /// The requests made of `model` so far.
    pub fn asked(model: &str) -> Vec<Asked> {
        stage(model, |s| s.asked.clone())
    }
    pub fn text(text: &str) -> Result<Reply, Failure> {
        Ok(Reply { parts: vec![Part::Text(text.into())], ..Reply::default() })
    }
    pub fn call(id: &str, name: &str, arguments: Value) -> Result<Reply, Failure> {
        Ok(Reply { parts: vec![Part::Call(Call { call_id: id.into(), name: name.into(), arguments: arguments.to_string() })], ..Reply::default() })
    }
    pub(super) fn next(request: &Request<'_>, on_delta: &mut (dyn FnMut(Delta) + Send)) -> Option<Result<Reply, Failure>> {
        let played = stage(request.model, |s| {
            if s.replies.is_empty() && s.asked.is_empty() {
                return None;
            }
            s.asked.push(Asked {
                instructions: request.instructions.to_owned(),
                items: request.items.to_vec(),
                tools: request.tools.iter().map(|t| t.name).collect(),
                answer_only: request.answer_only,
            });
            Some(s.replies.pop_front().unwrap_or_else(|| Err(Failure::new(FailureKind::Other, "The script has no more replies."))))
        })?;
        if let Ok(reply) = &played {
            on_delta(Delta::Started);
            for part in &reply.parts {
                if let Part::Text(text) = part {
                    on_delta(Delta::Text(text));
                }
            }
        }
        Some(played)
    }
}

/// Splits a byte stream into server-sent events: (event name, data).
#[derive(Default)]
pub struct Sse {
    buffer: Vec<u8>,
}
impl Sse {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<(String, String)> {
        self.buffer.extend(chunk.iter().filter(|b| **b != b'\r'));
        let mut events = vec![];
        while let Some(end) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let block: Vec<u8> = self.buffer.drain(..end + 2).collect();
            let text = String::from_utf8_lossy(&block);
            let mut event = String::new();
            let mut data = vec![];
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("event:") {
                    event = v.trim().to_owned();
                } else if let Some(v) = line.strip_prefix("data:") {
                    data.push(v.strip_prefix(' ').unwrap_or(v).to_owned());
                }
            }
            if !data.is_empty() {
                events.push((event, data.join("\n")));
            }
        }
        events
    }
}

/// The first few hundred characters of an error body, for messages.
pub fn excerpt(body: &str) -> String {
    super::tools::truncate(body.trim(), 300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_split_across_chunks() {
        let mut sse = Sse::default();
        assert!(sse.push(b"event: response.output_text.delta\r\ndata: {\"delta\":\"Hel").is_empty());
        let events = sse.push(b"lo\"}\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(events[0], ("response.output_text.delta".into(), "{\"delta\":\"Hello\"}".into()));
        assert_eq!(events[1].1, "[DONE]");
    }
}
