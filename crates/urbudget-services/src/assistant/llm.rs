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
    Text(&'a str),
    /// A readable summary of the model's reasoning while it works.
    Thinking(&'a str),
}

#[derive(Debug, Default)]
pub struct Reply {
    pub parts: Vec<Part>,
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
}

/// Sends the conversation and streams the reply (and any reasoning summary) to `on_delta`.
pub async fn respond(
    client: &reqwest::Client,
    credentials: &Credentials,
    request: &Request<'_>,
    on_delta: &mut (dyn FnMut(Delta) + Send),
) -> Result<Reply, Failure> {
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
