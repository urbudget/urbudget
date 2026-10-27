//! The budget assistant: a chat panel that reads the budget through tools and proposes changes
//! the user applies. It signs in with ChatGPT (the user's plan pays) or uses an OpenAI or
//! Anthropic API key. Conversations and attachments stay in the budget's folder.
mod agent;
mod dispatch;
pub use dispatch::dispatch;
mod anthropic;
pub mod attachments;
mod auth;
pub use auth::open_browser;
pub mod history;
pub mod llm;
pub mod mcp;
mod openai;
pub mod prompt;
pub mod proposals;
pub mod tools;
pub mod workbook;

use crate::patch::Patch;
use crate::vault;
use crate::Database;
use history::{Item, Thread};
use llm::{Credentials, Failure, FailureKind, Provider};
use proposals::{Recheck, Status};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};


/// A random id like `p_3f9c…`.
pub fn new_id(prefix: &str) -> String {
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).expect("system random numbers");
    format!("{prefix}_{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
/// Minor units as a plain decimal: -1240.50.
pub fn money(x: i64) -> String {
    let sign = if x < 0 { "-" } else { "" };
    let a = x.unsigned_abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}
/// An amount the model wrote, as a number or a string, in minor units.
pub fn parse_money(v: &Value) -> Result<i64, String> {
    let text = match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            let s = s.trim();
            // A decimal comma, as a bank file may have it (-12,40), would read as hundreds.
            let decimal_comma = match (s.rfind(','), s.rfind('.')) {
                (Some(c), Some(p)) => c > p,
                (Some(c), None) => matches!(s[c + 1..].len(), 1 | 2) && s[c + 1..].chars().all(|d| d.is_ascii_digit()),
                _ => false,
            };
            if decimal_comma {
                return Err(format!("{s} has a decimal comma: write amounts like -12.40."));
            }
            s.replace([',', ' '], "")
        }
        _ => return Err("Amounts must be numbers like 12.50.".into()),
    };
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(&text)),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let invalid = || format!("{text} isn't an amount like 12.50.");
    if (whole.is_empty() && fraction.is_empty())
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !fraction.chars().all(|c| c.is_ascii_digit())
    {
        return Err(invalid());
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| invalid())? };
    let mut thousandths: String = fraction.chars().take(3).collect();
    while thousandths.len() < 3 {
        thousandths.push('0');
    }
    let cents = (thousandths.parse::<i64>().map_err(|_| invalid())? + 5) / 10;
    let value = whole
        .checked_mul(100)
        .and_then(|w| w.checked_add(cents))
        .filter(|v| *v <= 1_000_000_000_000)
        .ok_or("That amount is too large.")?;
    Ok(if negative { -value } else { value })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub provider: Provider,
    /// The chosen model for each service; the service's first model when missing.
    pub models: HashMap<String, String>,
    pub share_memos: bool,
    pub keep_history: bool,
    /// A per-install id ChatGPT sign-in asks for.
    pub host_id: String,
    /// The client id ChatGPT issued UrBudget on the first sign-in.
    pub client_id: Option<String>,
    pub account: Option<auth::Account>,
    /// The first-use notice about using the ChatGPT plan was dismissed.
    pub plan_notice_seen: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Chatgpt,
            models: HashMap::new(),
            share_memos: true,
            keep_history: true,
            host_id: String::new(),
            client_id: None,
            account: None,
            plan_notice_seen: false,
        }
    }
}
fn provider_name(p: Provider) -> &'static str {
    match p {
        Provider::Chatgpt => "chatgpt",
        Provider::Openai => "openai",
        Provider::Anthropic => "anthropic",
    }
}
fn secret_service(p: Provider) -> &'static str {
    match p {
        Provider::Chatgpt => "com.localbudget.desktop.assistant.chatgpt",
        Provider::Openai => "com.localbudget.desktop.assistant.openai-key",
        Provider::Anthropic => "com.localbudget.desktop.assistant.anthropic-key",
    }
}

/// The assistant's state for the app's lifetime.
#[derive(Clone)]
pub struct Assistant(Arc<AssistantState>);

/// Shared state kept alive by background replies on every host.
pub struct AssistantState {
    data_dir: PathBuf,
    http: reqwest::Client,
    settings: Mutex<Option<Settings>>,
    /// The ChatGPT sign-in. Refreshes take this lock, so only one runs at a time.
    tokens: tokio::sync::Mutex<Option<auth::Tokens>>,
    /// Secrets that couldn't go in the system's store; gone when the app closes.
    memory: Mutex<HashMap<&'static str, String>>,
    /// Conversations in use, with the budget folder each belongs to.
    threads: Mutex<HashMap<String, (PathBuf, Thread)>>,
    running: Mutex<HashMap<String, RunningTask>>,
    signing_in: Mutex<Option<Arc<AtomicBool>>>,
    models: Mutex<HashMap<Provider, Vec<llm::Model>>>,
    /// Budget folders whose unused attachments were cleared this session.
    pruned: Mutex<std::collections::HashSet<PathBuf>>,
}

struct RunningTask {
    handle: tokio::task::JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
}
impl RunningTask {
    fn abort(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.handle.abort();
    }
}

impl std::ops::Deref for Assistant {
    type Target = AssistantState;
    fn deref(&self) -> &Self::Target { &self.0 }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Assistant {
    pub fn new(data_dir: PathBuf) -> Self {
        Self(Arc::new(AssistantState {
            data_dir,
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(20))
                .read_timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
            settings: Mutex::new(None),
            tokens: tokio::sync::Mutex::new(None),
            memory: Mutex::new(HashMap::new()),
            threads: Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            signing_in: Mutex::new(None),
            models: Mutex::new(HashMap::new()),
            pruned: Mutex::new(std::collections::HashSet::new()),
        }))
    }
    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("assistant.json")
    }
    pub fn settings(&self) -> Settings {
        let mut s = lock(&self.settings);
        if s.is_none() {
            let mut loaded: Settings = std::fs::read(self.settings_path())
                .ok()
                .and_then(|raw| serde_json::from_slice(&raw).ok())
                .unwrap_or_default();
            if loaded.host_id.is_empty() {
                loaded.host_id = auth::host_id();
                let _ = std::fs::create_dir_all(&self.data_dir);
                let _ = std::fs::write(self.settings_path(), serde_json::to_vec_pretty(&loaded).unwrap_or_default());
            }
            *s = Some(loaded);
        }
        s.clone().expect("settings loaded")
    }
    fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let mut current = self.settings();
        f(&mut current);
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        let path = self.settings_path();
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&current).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(temporary, path).map_err(|e| e.to_string())?;
        *lock(&self.settings) = Some(current.clone());
        Ok(current)
    }
    fn secret(&self, p: Provider) -> Option<String> {
        vault::read(secret_service(p)).or_else(|| lock(&self.memory).get(secret_service(p)).cloned())
    }
    fn keep_secret(&self, p: Provider, value: &str) {
        if vault::save(secret_service(p), value).is_err() {
            lock(&self.memory).insert(secret_service(p), value.to_owned());
        } else {
            lock(&self.memory).remove(secret_service(p));
        }
    }
    fn forget_secret(&self, p: Provider) {
        let _ = vault::delete(secret_service(p));
        lock(&self.memory).remove(secret_service(p));
    }

    /// The bearer token or key for the chosen service, refreshing the ChatGPT sign-in if needed.
    pub async fn credentials(&self) -> Result<Credentials, Failure> {
        let settings = self.settings();
        let provider = settings.provider;
        if provider != Provider::Chatgpt {
            let secret = self.secret(provider).ok_or_else(|| {
                Failure::new(FailureKind::SignIn, "Add your API key in Settings to use the assistant.")
            })?;
            return Ok(Credentials { provider, secret });
        }
        let signed_out = || Failure::new(FailureKind::SignIn, "Sign in with ChatGPT to use the assistant.");
        let client_id = settings.client_id.clone().ok_or_else(signed_out)?;
        let mut tokens = self.tokens.lock().await;
        let now = chrono::Utc::now().timestamp();
        let usable = tokens.as_ref().is_some_and(|t| t.fresh(now) || (now < t.earliest_refresh && t.expires_at > now));
        if !usable {
            let (refresh, scope) = match tokens.as_ref() {
                Some(t) => (t.refresh.clone(), t.scope.clone()),
                None => (self.secret(Provider::Chatgpt).ok_or_else(signed_out)?, String::new()),
            };
            match auth::refresh(&self.http, &client_id, &refresh, &scope).await {
                Ok(fresh) => {
                    self.keep_secret(Provider::Chatgpt, &fresh.refresh);
                    *tokens = Some(fresh);
                }
                Err(auth::RefreshError::SignInAgain(message)) => {
                    *tokens = None;
                    self.forget_secret(Provider::Chatgpt);
                    return Err(Failure::new(FailureKind::SignIn, message));
                }
                Err(auth::RefreshError::Retry(message)) => return Err(Failure::new(FailureKind::Busy, message)),
            }
        }
        let t = tokens.as_ref().expect("tokens present");
        if !t.plan_usage() {
            return Err(Failure::new(
                FailureKind::NotEligible,
                "UrBudget wasn't allowed to use your ChatGPT plan. Sign in again and allow it.",
            ));
        }
        Ok(Credentials { provider, secret: t.access.clone() })
    }

    async fn model_list(&self, refresh: bool) -> Result<Vec<llm::Model>, Failure> {
        let provider = self.settings().provider;
        if !refresh {
            if let Some(list) = lock(&self.models).get(&provider).filter(|l| !l.is_empty()) {
                return Ok(list.clone());
            }
        }
        let credentials = self.credentials().await?;
        let list = llm::models(&self.http, &credentials).await?;
        lock(&self.models).insert(provider, list.clone());
        Ok(list)
    }
    /// The model to use: the user's choice, or the service's first.
    pub async fn model(&self) -> Result<String, Failure> {
        let s = self.settings();
        if let Some(m) = s.models.get(provider_name(s.provider)).filter(|m| !m.is_empty()) {
            return Ok(m.clone());
        }
        if s.provider == Provider::Anthropic {
            return Ok(anthropic::DEFAULT_MODEL.into());
        }
        let list = self.model_list(false).await?;
        list.first()
            .map(|m| m.id.clone())
            .ok_or_else(|| Failure::new(FailureKind::Model, "No models are available on this account."))
    }

    /// Runs `f` on a conversation, loading it first if needed, and saves it when history is kept.
    pub fn with_thread<T>(&self, folder: &Path, id: &str, f: impl FnOnce(&mut Thread) -> T) -> Result<T, String> {
        self.with_thread_guarded(folder, id, None, f)
    }
    fn with_thread_guarded<T>(&self, folder: &Path, id: &str, cancelled: Option<&AtomicBool>, f: impl FnOnce(&mut Thread) -> T) -> Result<T, String> {
        let mut threads = lock(&self.threads);
        // Abort can arrive while a local tool is doing synchronous work. Test cancellation
        // under the history lock so that no late tool result can resurrect a deleted chat.
        if cancelled.is_some_and(|signal| signal.load(Ordering::SeqCst)) { return Err("Reply stopped.".into()); }
        if !threads.get(id).is_some_and(|(dir, _)| dir == folder) {
            let t = history::load(folder, id)?;
            threads.insert(id.to_owned(), (folder.to_owned(), t));
        }
        let (dir, thread) = threads.get_mut(id).expect("thread loaded");
        let out = f(thread);
        if self.settings().keep_history {
            history::save(dir, thread)?;
        }
        Ok(out)
    }
    /// Once per budget folder and session, before the first new attachment: removes attached
    /// files no conversation refers to, such as those of conversations that weren't kept.
    fn prune_once(&self, folder: &Path) {
        if !lock(&self.pruned).insert(folder.to_owned()) {
            return;
        }
        let mut keep = std::collections::HashSet::new();
        let mut note = |t: &Thread| {
            for item in &t.items {
                if let Item::User { attachments, .. } = item {
                    keep.extend(attachments.iter().map(|a| a.id.clone()));
                }
            }
        };
        for listing in history::list(folder) {
            if let Ok(t) = history::load(folder, &listing.id) {
                note(&t);
            }
        }
        for (dir, t) in lock(&self.threads).values() {
            if dir == folder {
                note(t);
            }
        }
        let Ok(entries) = std::fs::read_dir(history::files(folder)) else { return };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name.split('.').next().unwrap_or_default();
            if !keep.contains(id) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    fn thread_copy(&self, folder: &Path, id: &str) -> Result<Thread, String> {
        self.with_thread(folder, id, |t| t.clone())
    }
    fn is_running(&self, id: &str) -> bool {
        lock(&self.running).contains_key(id)
    }
}

fn folder(db: &Database) -> Result<PathBuf, String> {
    Ok(db.store()?.folder().to_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    provider: Provider,
    /// Signed in, or a key is saved, for the chosen service.
    ready: bool,
    account: Option<auth::Account>,
    model: Option<String>,
    share_memos: bool,
    keep_history: bool,
    plan_notice_seen: bool,
    has_openai_key: bool,
    has_anthropic_key: bool,
    /// Where secrets are kept, or None when they last only until the app closes.
    storage: Option<&'static str>,
    signing_in: bool,
    /// How to add UrBudget to an MCP client such as Claude Desktop.
    mcp: Value,
}
fn status(a: &Assistant) -> StatusView {
    let s = a.settings();
    let ready = match s.provider {
        Provider::Chatgpt => s.client_id.is_some() && a.secret(Provider::Chatgpt).is_some(),
        p => a.secret(p).is_some(),
    };
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    StatusView {
        provider: s.provider,
        ready,
        account: if s.provider == Provider::Chatgpt && ready { s.account.clone() } else { None },
        model: s.models.get(provider_name(s.provider)).cloned(),
        share_memos: s.share_memos,
        keep_history: s.keep_history,
        plan_notice_seen: s.plan_notice_seen,
        has_openai_key: a.secret(Provider::Openai).is_some(),
        has_anthropic_key: a.secret(Provider::Anthropic).is_some(),
        storage: vault::STORE,
        signing_in: lock(&a.signing_in).is_some(),
        mcp: json!({
            "command": exe,
            "args": ["--mcp"],
            "config": { "mcpServers": { "urbudget": { "command": exe, "args": ["--mcp"] } } },
        }),
    }
}

pub fn assistant_status(assistant: &Assistant) -> StatusView {
    status(&assistant)
}

pub async fn assistant_sign_in(assistant: &Assistant) -> Result<StatusView, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut busy = lock(&assistant.signing_in);
        if let Some(previous) = busy.take() {
            previous.store(true, Ordering::Relaxed);
        }
        *busy = Some(cancel.clone());
    }
    let settings = assistant.settings();
    let a: &Assistant = &assistant;
    let result = auth::sign_in(
        &a.http,
        &settings.host_id,
        settings.client_id.clone(),
        settings.account.as_ref().map(|x| x.email.clone()),
        cancel.clone(),
        auth::open_sign_in,
        |issued| {
            let _ = a.update(|s| s.client_id = Some(issued.to_owned()));
        },
    )
    .await;
    {
        let mut busy = lock(&a.signing_in);
        if busy.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
            *busy = None;
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Sign-in was cancelled.".into());
    }
    let signed = result?;
    a.keep_secret(Provider::Chatgpt, &signed.tokens.refresh);
    let plan = signed.tokens.plan_usage();
    *a.tokens.lock().await = Some(signed.tokens);
    lock(&a.models).remove(&Provider::Chatgpt);
    a.update(|s| {
        s.client_id = Some(signed.client_id.clone());
        s.account = Some(signed.account.clone());
        s.provider = Provider::Chatgpt;
    })?;
    if !plan {
        return Err("You signed in, but didn't allow UrBudget to use your ChatGPT plan. Sign in again and allow it.".into());
    }
    Ok(status(a))
}

pub fn assistant_cancel_sign_in(assistant: &Assistant) {
    if let Some(c) = lock(&assistant.signing_in).take() {
        c.store(true, Ordering::Relaxed);
    }
}

pub async fn assistant_sign_out(assistant: &Assistant) -> Result<StatusView, String> {
    let a: &Assistant = &assistant;
    let settings = a.settings();
    let refresh = a.tokens.lock().await.take().map(|t| t.refresh).or_else(|| a.secret(Provider::Chatgpt));
    a.forget_secret(Provider::Chatgpt);
    lock(&a.models).remove(&Provider::Chatgpt);
    if let (Some(client), Some(token)) = (settings.client_id.as_deref(), refresh) {
        if !auth::revoke(&a.http, client, &token).await {
            // Keep the client id and host id for the next sign-in; only the tokens go.
            return Err("Signed out here. ChatGPT didn't confirm it; you can also disconnect UrBudget in ChatGPT's settings.".into());
        }
    }
    Ok(status(a))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChange {
    provider: Option<Provider>,
    model: Option<String>,
    share_memos: Option<bool>,
    keep_history: Option<bool>,
    plan_notice_seen: Option<bool>,
}
pub fn assistant_settings(assistant: &Assistant, change: SettingsChange) -> Result<StatusView, String> {
    assistant.update(|s| {
        if let Some(p) = change.provider {
            s.provider = p;
        }
        if let Some(m) = change.model {
            let key = provider_name(s.provider).to_owned();
            if m.is_empty() {
                s.models.remove(&key);
            } else {
                s.models.insert(key, m);
            }
        }
        if let Some(v) = change.share_memos {
            s.share_memos = v;
        }
        if let Some(v) = change.keep_history {
            s.keep_history = v;
        }
        if let Some(v) = change.plan_notice_seen {
            s.plan_notice_seen = v;
        }
    })?;
    Ok(status(&assistant))
}

pub async fn assistant_set_key(assistant: &Assistant, provider: Provider, key: String) -> Result<StatusView, String> {
    let key = key.trim().to_owned();
    if provider == Provider::Chatgpt || key.len() < 20 || key.len() > 400 || key.chars().any(char::is_whitespace) {
        return Err("Paste the whole API key.".into());
    }
    let credentials = Credentials { provider, secret: key.clone() };
    // A key that can list models works.
    let list = llm::models(&assistant.http, &credentials).await.map_err(|f| f.message)?;
    assistant.keep_secret(provider, &key);
    lock(&assistant.models).insert(provider, list);
    assistant.update(|s| s.provider = provider)?;
    Ok(status(&assistant))
}
pub fn assistant_forget_key(assistant: &Assistant, provider: Provider) -> Result<StatusView, String> {
    assistant.forget_secret(provider);
    lock(&assistant.models).remove(&provider);
    if assistant.settings().provider == provider {
        assistant.update(|s| s.provider = Provider::Chatgpt)?;
    }
    Ok(status(&assistant))
}

pub async fn assistant_models(assistant: &Assistant, refresh: bool) -> Result<Vec<llm::Model>, String> {
    assistant.model_list(refresh).await.map_err(|f| f.message)
}

pub fn assistant_threads(db: &Database, assistant: &Assistant) -> Result<Vec<history::Listing>, String> {
    let folder = folder(&db)?;
    if !assistant.settings().keep_history {
        let threads = lock(&assistant.threads);
        let mut list: Vec<history::Listing> = threads
            .values()
            .filter(|(dir, _)| dir == &folder)
            .map(|(_, t)| history::Listing { id: t.id.clone(), title: t.title.clone(), updated: t.updated.clone() })
            .collect();
        list.sort_by(|a, b| b.updated.cmp(&a.updated));
        return Ok(list);
    }
    Ok(history::list(&folder))
}

pub fn assistant_thread(db: &Database, assistant: &Assistant, id: String) -> Result<Value, String> {
    let folder = folder(&db)?;
    let mut view = assistant.with_thread(&folder, &id, |t| history::view(t))?;
    view["running"] = json!(assistant.is_running(&id));
    Ok(view)
}

pub fn assistant_delete_thread(db: &Database, assistant: &Assistant, id: String) -> Result<(), String> {
    if let Some(h) = lock(&assistant.running).remove(&id) {
        h.abort();
    }
    lock(&assistant.threads).remove(&id);
    history::delete(&folder(&db)?, &id)
}

/// Deletes every conversation and attachment of the open budget.
pub fn assistant_delete_all(db: &Database, assistant: &Assistant) -> Result<(), String> {
    let folder = folder(&db)?;
    let mut threads = lock(&assistant.threads);
    let ids: Vec<String> = threads.iter().filter(|(_, (dir, _))| dir == &folder).map(|(id, _)| id.clone()).collect();
    for id in ids {
        if let Some(h) = lock(&assistant.running).remove(&id) {
            h.abort();
        }
        threads.remove(&id);
    }
    history::delete_all(&folder)
}

/// Saves a file the user attached. `data` is base64.
pub fn assistant_attach(
    db: &Database,
    assistant: &Assistant,
    name: String,
    data: String,
) -> Result<attachments::Meta, String> {
    use base64::Engine;
    assistant.prune_once(&folder(&db)?);
    if data.len() > 40 << 20 {
        return Err(format!("{name} is too large to attach."));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|_| "The file couldn't be read.")?;
    attachments::Files::open(history::files(&folder(&db)?)).save(&name, &bytes)
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Proposed => "not applied yet",
        Status::Applied => "applied",
        Status::Discarded => "discarded",
        Status::Undone => "applied, then undone",
        Status::Failed => "couldn't be applied",
    }
}

/// Sends a message and starts the reply in the background. Returns the conversation id.
#[allow(clippy::too_many_arguments)]
pub fn assistant_send(
    simplefin: &crate::simplefin::SimpleFin,
    db: &Database,
    assistant: &Assistant,
    patch: Option<Patch>,
    thread_id: Option<String>,
    text: String,
    attachments: Vec<String>,
    screen: prompt::Screen,
) -> Result<String, String> {
    let text = text.trim().to_owned();
    if text.is_empty() && attachments.is_empty() {
        return Err("Type a message or attach a file.".into());
    }
    if text.len() > 20_000 {
        return Err("That message is too long.".into());
    }
    if attachments.len() > 5 {
        return Err("Attach up to 5 files at a time.".into());
    }
    let folder = folder(&db)?;
    let budget = db.snapshot(patch)?;
    let settings = assistant.settings();
    let files = attachments::Files::open(history::files(&folder));
    let metas = attachments.iter().map(|id| files.get(id)).collect::<Result<Vec<_>, _>>()?;
    let id = match thread_id {
        Some(id) => {
            if assistant.is_running(&id) {
                return Err("Wait for the current reply, or stop it.".into());
            }
            assistant.thread_copy(&folder, &id)?;
            id
        }
        None => {
            let id = new_id("t");
            let mut title: String = text.chars().take(60).collect();
            if title.is_empty() {
                title = metas.first().map(|m| m.name.clone()).unwrap_or_else(|| "New conversation".into());
            }
            let thread = Thread {
                id: id.clone(),
                title,
                created: now(),
                updated: now(),
                instructions: prompt::INSTRUCTIONS.into(),
                items: vec![],
                proposals: vec![],
            };
            lock(&assistant.threads).insert(id.clone(), (folder.clone(), thread));
            id
        }
    };
    let attached_before: usize = assistant.with_thread(&folder, &id, |t| {
        t.items.iter().map(|i| if let Item::User { attachments, .. } = i { attachments.len() } else { 0 }).sum()
    })?;
    if attached_before + metas.len() > 12 {
        return Err("This conversation has as many files as it can hold. Start a new conversation to attach more.".into());
    }
    // The file being imported, as it reads now: kept with the message, so a fix the assistant
    // proposes is for these lines as they are now (a line changed since won't take it).
    let import = screen.import.as_ref().and_then(|i| {
        crate::bank_statement::statement(&budget, &i.account, &i.content, &i.session).ok().map(|s| s.brief(settings.share_memos))
    });
    assistant.with_thread(&folder, &id, |t| {
        history::settle(&mut t.items);
        let earlier: Vec<String> = t
            .proposals
            .iter()
            .map(|p| match p.choices() {
                Some(choices) => format!("\u{201c}{}\u{201d} {} ({choices})", p.title, status_word(p.status)),
                None => format!("\u{201c}{}\u{201d} {}", p.title, status_word(p.status)),
            })
            .collect();
        let context = prompt::context(&budget, &screen, settings.share_memos, &earlier, import.as_ref());
        t.items.push(Item::User { text: text.clone(), attachments: metas, context, at: now(), import });
        t.updated = now();
    })?;
    let job = agent::Job {
        thread_id: id.clone(),
        folder,
        budget,
        today: screen.today.clone(),
        month: if screen.month.is_empty() { screen.today.chars().take(7).collect() } else { screen.month.clone() },
        share_memos: settings.share_memos,
        bank_sync: simplefin.snapshot(),
    };
    start(assistant, id.clone(), job);
    Ok(id)
}

/// Asks again after an error, without a new message.
pub fn assistant_retry(
    simplefin: &crate::simplefin::SimpleFin,
    db: &Database,
    assistant: &Assistant,
    patch: Option<Patch>,
    thread_id: String,
    screen: prompt::Screen,
) -> Result<(), String> {
    if assistant.is_running(&thread_id) {
        return Err("Wait for the current reply, or stop it.".into());
    }
    let folder = folder(&db)?;
    let budget = db.snapshot(patch)?;
    assistant.with_thread(&folder, &thread_id, |t| history::settle(&mut t.items))?;
    let job = agent::Job {
        thread_id: thread_id.clone(),
        folder,
        budget,
        today: screen.today.clone(),
        month: if screen.month.is_empty() { screen.today.chars().take(7).collect() } else { screen.month.clone() },
        share_memos: assistant.settings().share_memos,
        bank_sync: simplefin.snapshot(),
    };
    start(assistant, thread_id, job);
    Ok(())
}

/// Register a turn before it can finish, including when credentials are missing immediately.
/// Otherwise a fast failure can leave a completed task marked as running forever.
fn start(assistant: &Assistant, id: String, job: agent::Job) {
    let (ready, waiting) = tokio::sync::oneshot::channel();
    let owned = assistant.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let handle = crate::executor().spawn(async move {
        if waiting.await.is_ok() { agent::run(owned, job, signal).await; }
    });
    lock(&assistant.running).insert(id, RunningTask { handle, cancelled });
    let _ = ready.send(());
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    title: String,
    period: Option<String>,
    body: String,
    created: String,
    budget: String,
}
/// A report the assistant wrote, for the preview and the printed copy.
pub fn assistant_report(
    db: &Database,
    assistant: &Assistant,
    thread_id: String,
    proposal_id: String,
) -> Result<ReportView, String> {
    let folder = folder(&db)?;
    let budget = db.store()?.read_draft(None, |b| Ok(b.name.clone()))?;
    let thread = assistant.thread_copy(&folder, &thread_id)?;
    let doc = thread
        .proposals
        .iter()
        .find(|p| p.id == proposal_id)
        .and_then(|p| p.report.clone())
        .ok_or("That report is gone.")?;
    Ok(ReportView { title: doc.title, period: doc.period, body: doc.body, created: doc.created, budget })
}

/// Appends an error the window couldn't handle to `<data folder>/errors.log`, so a crash can be
/// read afterwards. The file is rotated past 1 MB.
pub fn assistant_log_error(assistant: &Assistant, message: String) -> Result<(), String> {
    use std::io::Write;
    let path = assistant.data_dir.join("errors.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::rename(&path, assistant.data_dir.join("errors.old.log"));
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| e.to_string())?;
    let message: String = message.chars().take(20_000).collect();
    writeln!(file, "[{}] {}\n", now(), message).map_err(|e| e.to_string())
}

/// Opens one of a few fixed pages in the browser.
pub fn assistant_open(link: String) -> Result<(), String> {
    let url = match link.as_str() {
        "usage" => "https://chatgpt.com/settings/usage",
        "disconnect" => "https://chatgpt.com/settings",
        "openaiKeys" => "https://platform.openai.com/api-keys",
        "anthropicKeys" => "https://console.anthropic.com/settings/keys",
        _ => return Err("Unknown link.".into()),
    };
    auth::open_browser(url)
}

/// Only the excerpt and explanation explicitly approved in the in-app report form are sent.
/// This does not read a conversation, account, budget, attachment or provider credential.
pub async fn assistant_report_reply(cloud: &crate::cloud::Cloud, text: String, reason: String) -> Result<String, String> {
    let payload = report_payload(&text, &reason)?;
    let answer = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        cloud.call(reqwest::Method::POST, "/assistant-reports", Some(payload), false),
    )
    .await
    .map_err(|_| "The report timed out. Your text is still here; try again when you're online.".to_owned())?
    .map_err(|failure| {
        if failure.status == 404 {
            "Reporting isn't available right now. Your text is still here; try again later.".to_owned()
        } else {
            failure.message
        }
    })?;
    answer["id"].as_str().filter(|id| id.starts_with("rep_")).map(str::to_owned)
        .ok_or_else(|| "The report service did not confirm a reference. Please try again later.".to_owned())
}

fn report_payload(text: &str, reason: &str) -> Result<Value, String> {
    let text = text.trim();
    let reason = reason.trim();
    // Match the browser's maxlength and the API's JavaScript string lengths, including emoji.
    if text.is_empty() || text.encode_utf16().count() > 4000 || reason.encode_utf16().count() > 1000 {
        return Err("Include 1–4,000 characters from the reply and at most 1,000 characters explaining the problem.".into());
    }
    Ok(json!({ "text": text, "reason": reason }))
}

pub fn assistant_stop(db: &Database, assistant: &Assistant, thread_id: String) -> Result<(), String> {
    if let Some(h) = lock(&assistant.running).remove(&thread_id) {
        h.abort();
        let folder = folder(&db)?;
        assistant.with_thread(&folder, &thread_id, |t| {
            history::settle(&mut t.items);
            t.items.push(Item::Stopped);
        })?;
        agent::emit(&thread_id, json!({ "type": "done", "stopped": true }));
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// The change to apply to the window's budget; None when the proposal needs another look.
    patch: Option<Patch>,
    proposal: Value,
}

/// Runs a proposal again on the budget as the window has it. The same change comes back as a
/// patch to apply; a changed one comes back as a refreshed card.
pub fn assistant_apply(
    db: &Database,
    assistant: &Assistant,
    patch: Option<Patch>,
    thread_id: String,
    proposal_id: String,
) -> Result<Applied, String> {
    let folder = folder(&db)?;
    let proposal = assistant
        .thread_copy(&folder, &thread_id)?
        .proposals
        .into_iter()
        .find(|p| p.id == proposal_id)
        .ok_or("That proposal is gone.")?;
    if !matches!(proposal.status, Status::Proposed | Status::Undone) {
        return Err("This proposal was already handled.".into());
    }
    let outcome = db.read(patch, |current| Ok(proposals::recheck(&proposal, current)))?;
    assistant.with_thread(&folder, &thread_id, |t| {
        let p = t.proposals.iter_mut().find(|p| p.id == proposal_id).expect("proposal present");
        match outcome {
            Recheck::Same(patch) => {
                p.status = Status::Applied;
                Applied { patch: Some(patch), proposal: history::proposal_view(p) }
            }
            Recheck::Changed(refreshed) => {
                *p = refreshed;
                if p.status == Status::Undone {
                    p.status = Status::Proposed;
                }
                Applied { patch: None, proposal: history::proposal_view(p) }
            }
        }
    })
}

/// The proposal as the user chose on its card (rows left out, categories and amounts given),
/// made again on the budget as the window has it. The card comes back with every row, the ones
/// left out marked.
pub fn assistant_revise(
    db: &Database,
    assistant: &Assistant,
    patch: Option<Patch>,
    thread_id: String,
    proposal_id: String,
    revision: proposals::Revision,
) -> Result<Value, String> {
    let folder = folder(&db)?;
    let proposal = assistant
        .thread_copy(&folder, &thread_id)?
        .proposals
        .into_iter()
        .find(|p| p.id == proposal_id)
        .ok_or("That proposal is gone.")?;
    let revised = db.read(patch, |current| proposals::revise(&proposal, revision, current))?;
    assistant.with_thread(&folder, &thread_id, |t| {
        let p = t.proposals.iter_mut().find(|p| p.id == proposal_id).ok_or("That proposal is gone.")?;
        // Applied or discarded meanwhile: the choice is for a card that's done.
        if p.status != proposal.status {
            return Err("This proposal was already handled.".to_string());
        }
        *p = revised;
        Ok(history::proposal_view(p))
    })?
}

/// Records what happened to a proposal: discarded, undone, or back to proposed when applying failed.
pub fn assistant_mark(
    db: &Database,
    assistant: &Assistant,
    thread_id: String,
    proposal_id: String,
    status: Status,
) -> Result<Value, String> {
    let folder = folder(&db)?;
    assistant.with_thread(&folder, &thread_id, |t| {
        let p = t.proposals.iter_mut().find(|p| p.id == proposal_id).ok_or("That proposal is gone.")?;
        p.status = status;
        Ok(history::proposal_view(p))
    })?
}

/// Native atomic undo verifies history before changing money; deleted conversations cannot
/// turn a subsequent missing-card error into an apparently failed but already-applied undo.
pub fn verify_undo_proposal(db: &Database, assistant: &Assistant, thread_id: &str, proposal_id: &str) -> Result<(), String> {
    let folder = folder(db)?;
    let thread = assistant.thread_copy(&folder, thread_id)?;
    let proposal = thread.proposals.iter().find(|p| p.id == proposal_id).ok_or("That proposal is gone.")?;
    if proposal.status != Status::Applied { return Err("That proposal is no longer applied.".into()); }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportView {
    account: String,
    name: String,
    csv: String,
}
fn import_of(db: &Database, assistant: &Assistant, thread_id: &str, proposal_id: &str) -> Result<(ImportView, PathBuf), String> {
    let folder = folder(db)?;
    let thread = assistant.thread_copy(&folder, thread_id)?;
    let file = thread
        .proposals
        .iter()
        .find(|p| p.id == proposal_id)
        .and_then(|p| p.import.clone())
        .ok_or("That import is gone.")?;
    let stem: String = file
        .source
        .rsplit_once('.')
        .map_or(file.source.as_str(), |(s, _)| s)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || " -_".contains(c) { c } else { '_' })
        .take(60)
        .collect();
    Ok((ImportView { account: file.account.clone(), name: format!("{} for import.csv", stem.trim()), csv: file.csv()? }, folder))
}
/// The converted rows as the CSV the import review reads.
pub fn assistant_import(db: &Database, assistant: &Assistant, thread_id: String, proposal_id: String) -> Result<ImportView, String> {
    Ok(import_of(&db, &assistant, &thread_id, &proposal_id)?.0)
}
/// The converted rows' file name and CSV, for saving elsewhere.
pub fn import_file(db: &Database, assistant: &Assistant, thread_id: &str, proposal_id: &str) -> Result<(String, String), String> {
    let file = import_of(db, assistant, thread_id, proposal_id)?.0;
    Ok((file.name, file.csv))
}
/// Saves the converted rows in the budget's exports folder and returns the path.
pub fn assistant_save_import(db: &Database, assistant: &Assistant, thread_id: String, proposal_id: String) -> Result<String, String> {
    let (file, folder) = import_of(&db, &assistant, &thread_id, &proposal_id)?;
    let directory = folder.join("exports");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join(&file.name);
    std::fs::write(&path, file.csv).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Writes a spreadsheet card's workbook (or a report's tables) to the budget's exports folder,
/// filling data sheets from the budget as the window has it, and returns the file's path. With
/// `open`, it opens the file too, reusing the one saved last when it is still there.
pub fn assistant_spreadsheet(
    db: &Database,
    assistant: &Assistant,
    patch: Option<Patch>,
    thread_id: String,
    proposal_id: String,
    open: bool,
) -> Result<String, String> {
    let folder = folder(&db)?;
    if open {
        if let Some(saved) = spreadsheet_card(assistant, &folder, &thread_id, &proposal_id)?.saved.filter(|s| Path::new(s).is_file()) {
            crate::host::open_file(Path::new(&saved))?;
            return Ok(saved);
        }
    }
    let (title, bytes) = spreadsheet_file(db, assistant, patch, &thread_id, &proposal_id)?;
    let directory = folder.join("exports");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = workbook::free_path(&directory, &title);
    std::fs::write(&path, bytes).map_err(|e| format!("Couldn't save the spreadsheet: {e}"))?;
    let shown = path.to_string_lossy().into_owned();
    spreadsheet_saved(db, assistant, &thread_id, &proposal_id, &shown)?;
    if open {
        crate::host::open_file(&path)?;
    }
    Ok(shown)
}
fn spreadsheet_card(assistant: &Assistant, folder: &Path, thread_id: &str, proposal_id: &str) -> Result<proposals::Proposal, String> {
    assistant
        .thread_copy(folder, thread_id)?
        .proposals
        .into_iter()
        .find(|p| p.id == proposal_id)
        .ok_or_else(|| "That spreadsheet is gone.".into())
}
/// A spreadsheet card's workbook (or a report's tables), with data sheets filled from the budget
/// as the window has it: its title and the file's bytes.
pub fn spreadsheet_file(
    db: &Database,
    assistant: &Assistant,
    patch: Option<Patch>,
    thread_id: &str,
    proposal_id: &str,
) -> Result<(String, Vec<u8>), String> {
    let p = spreadsheet_card(assistant, &folder(db)?, thread_id, proposal_id)?;
    let (title, grids) = match (&p.workbook, &p.report) {
        (Some(w), _) => (w.title.clone(), db.read(patch, |b| w.sheets.iter().map(|s| workbook::fill(s, b)).collect())?),
        (None, Some(r)) => (r.title.clone(), workbook::report_tables(&r.body)),
        _ => return Err("That spreadsheet is gone.".into()),
    };
    if grids.is_empty() {
        return Err("This report has no tables to put in a spreadsheet.".into());
    }
    let bytes = workbook::write(&title, &grids)?;
    Ok((title, bytes))
}
/// Remembers where a card's spreadsheet was saved, for Open to reuse.
pub fn spreadsheet_saved(db: &Database, assistant: &Assistant, thread_id: &str, proposal_id: &str, path: &str) -> Result<(), String> {
    assistant.with_thread(&folder(db)?, thread_id, |t| {
        if let Some(p) = t.proposals.iter_mut().find(|p| p.id == proposal_id) {
            p.saved = Some(path.to_owned());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_round_trips() {
        assert_eq!(money(-124050), "-1240.50");
        assert_eq!(money(5), "0.05");
        assert_eq!(parse_money(&json!(12.5)).unwrap(), 1250);
        assert_eq!(parse_money(&json!("-1,240.50")).unwrap(), -124050);
        assert_eq!(parse_money(&json!(0.1)).unwrap(), 10);
        assert_eq!(parse_money(&json!("12.345")).unwrap(), 1235);
        assert!(parse_money(&json!("twelve")).is_err());
        assert!(parse_money(&json!(null)).is_err());
    }

    #[test]
    fn a_decimal_comma_is_refused_rather_than_read_as_hundreds() {
        assert!(parse_money(&json!("-12,40")).unwrap_err().contains("like -12.40"));
        assert!(parse_money(&json!("12,4")).is_err());
        assert!(parse_money(&json!("1.240,50")).is_err());
        // Commas and spaces between thousands still read.
        assert_eq!(parse_money(&json!("1,240")).unwrap(), 124000);
        assert_eq!(parse_money(&json!("-1,240,000")).unwrap(), -124000000);
        assert_eq!(parse_money(&json!("1,240.50")).unwrap(), 124050);
        assert_eq!(parse_money(&json!("1 240.50")).unwrap(), 124050);
    }

    #[test]
    fn report_payload_is_bounded_and_contains_only_approved_text() {
        assert_eq!(report_payload(" Selected excerpt ", " My explanation ").unwrap(), json!({ "text": "Selected excerpt", "reason": "My explanation" }));
        assert!(report_payload("  ", "reason").is_err());
        assert!(report_payload(&"a".repeat(4001), "").is_err());
        assert!(report_payload("reply", &"a".repeat(1001)).is_err());
        assert!(report_payload(&"😀".repeat(2000), "").is_ok());
        assert!(report_payload(&"😀".repeat(2001), "").is_err());
    }

    #[test]
    fn cached_conversation_cannot_be_read_from_another_budget() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let assistant = Assistant::new(a.path().join("settings"));
        let thread = Thread { id: "t_private".into(), title: "Private".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![] };
        history::save(a.path(), &thread).unwrap();
        assert!(assistant.thread_copy(a.path(), &thread.id).is_ok());
        assert!(assistant.thread_copy(b.path(), &thread.id).is_err());
        assert!(!history::files(b.path()).exists());
    }

    #[test]
    fn cancelled_tool_results_cannot_resurrect_a_conversation_during_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let assistant = Assistant::new(directory.path().join("settings"));
        let thread = Thread { id: "t_cancelled".into(), title: "Synthetic".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![] };
        history::save(directory.path(), &thread).unwrap();
        let cancelled = AtomicBool::new(true);
        // Delete has cancelled the job and removed its memory entry; its history file can
        // still exist until the next filesystem operation. A late tool must not cache/save it.
        let result = assistant.with_thread_guarded(directory.path(), &thread.id, Some(&cancelled), |t| {
            t.items.push(Item::Assistant { text: "Late tool result".into() });
        });
        assert!(result.is_err());
        assert!(!lock(&assistant.threads).contains_key(&thread.id));
        assert_eq!(history::load(directory.path(), &thread.id).unwrap(), thread);
        history::delete(directory.path(), &thread.id).unwrap();
        assert!(history::list(directory.path()).is_empty());
    }

    #[test]
    fn an_immediate_auth_failure_does_not_leave_a_turn_running() {
        let folder = tempfile::tempdir().unwrap();
        let assistant = Assistant::new(folder.path().join("settings"));
        let id = "t_no_credentials".to_owned();
        let thread = Thread { id: id.clone(), title: "Test".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![] };
        history::save(folder.path(), &thread).unwrap();
        let job = agent::Job { thread_id: id.clone(), folder: folder.path().to_path_buf(), budget: crate::domain::Budget::default(),
            today: "2026-10-21".into(), month: "2026-10".into(), share_memos: false, bank_sync: Value::Null };
        // Starting a task does not require the calling UI thread to have a Tokio context.
        start(&assistant, id.clone(), job);
        crate::block_on(async {
            for _ in 0..100 {
                if !assistant.is_running(&id) { break; }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        });
        assert!(!assistant.is_running(&id));
        let result = assistant.thread_copy(folder.path(), &id).unwrap();
        assert!(matches!(result.items.last(), Some(Item::Error { code, .. }) if code == "signIn"));
    }

    #[test]
    fn native_proposals_commit_and_undo_only_the_exact_latest_change() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = crate::runtime::Runtime::new(directory.path()).unwrap();
        let call = |method: &str, p: Value| crate::block_on(runtime.dispatch(method, p));
        let before = urbudget_core::test_support::sample();
        let initial = runtime.load().unwrap();
        call("save_budget", json!({"budget":before,"expectedToken":initial["token"]})).unwrap();
        let proposal = proposals::build(proposals::Change::Moves {
            month: "2026-10".into(), moves: vec![proposals::Move { from: None, to: Some("food".into()), amount: 1000 }],
        }, "Give food more room".into(), &before).unwrap();
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        let mut thread = Thread { id: "t_commit".into(), title: "Synthetic proposal".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![proposal.clone()] };
        history::save(&folder, &thread).unwrap();
        let request = json!({"threadId":thread.id,"proposalId":proposal.id});
        let applied = call("assistant_apply_commit", request.clone()).unwrap();
        assert_eq!(applied["proposal"]["status"], "applied");
        assert_eq!(applied["undoAvailable"], true);
        assert_eq!(applied["snapshot"]["budget"]["assignments"][0]["amount"], 21000);
        assert!(call("assistant_undo_commit", json!({"threadId":thread.id,"proposalId":"p_some_other_card","expectedToken":applied["snapshot"]["token"]})).is_err());
        let undone = call("assistant_undo_commit", json!({"threadId":thread.id,"proposalId":proposal.id,"expectedToken":applied["snapshot"]["token"]})).unwrap();
        assert_eq!(undone["proposal"]["status"], "undone");
        assert_eq!(undone["snapshot"]["budget"]["assignments"][0]["amount"], 20000);

        let reapplied = call("assistant_apply_commit", request.clone()).unwrap();
        let mut other: crate::domain::Budget = serde_json::from_value(reapplied["snapshot"]["budget"].clone()).unwrap();
        other.name = "A later manual edit".into();
        let edited = call("save_budget", json!({"budget":other,"expectedToken":reapplied["snapshot"]["token"]})).unwrap();
        assert!(call("assistant_undo_commit", json!({"threadId":thread.id,"proposalId":proposal.id,"expectedToken":edited["token"]})).is_err());
        assert_eq!(runtime.load().unwrap()["budget"]["name"], "A later manual edit");

        // Old or manually restored history can contain a no-op. It must never offer to undo
        // an unrelated earlier edit merely because the budget already has undo history.
        let mut noop = proposal.clone();
        noop.id = "p_noop".into();
        noop.change = Some(proposals::Change::Edits { edits: vec![], rules: vec![] });
        noop.expected = Some(proposals::normalize(&Patch::default()));
        noop.status = Status::Proposed;
        thread.id = "t_noop".into();
        thread.proposals = vec![noop.clone()];
        history::save(&folder, &thread).unwrap();
        let unchanged = call("assistant_apply_commit", json!({"threadId":thread.id,"proposalId":noop.id})).unwrap();
        assert_eq!(unchanged["undoAvailable"], false);
        assert!(call("assistant_undo_commit", json!({"threadId":thread.id,"proposalId":noop.id,"expectedToken":unchanged["snapshot"]["token"]})).is_err());

        // A deleted conversation must be rejected before undo touches the saved budget.
        let current: crate::domain::Budget = serde_json::from_value(runtime.load().unwrap()["budget"].clone()).unwrap();
        let deletable = proposals::build(proposals::Change::Moves {
            month: "2026-10".into(), moves: vec![proposals::Move { from: None, to: Some("food".into()), amount: 500 }],
        }, "A deleted card".into(), &current).unwrap();
        thread.id = "t_deleted".into(); thread.proposals = vec![deletable.clone()];
        history::save(&folder, &thread).unwrap();
        let applied = call("assistant_apply_commit", json!({"threadId":thread.id,"proposalId":deletable.id})).unwrap();
        call("assistant_delete_thread", json!({"id":thread.id})).unwrap();
        assert!(call("assistant_undo_commit", json!({"threadId":thread.id,"proposalId":deletable.id,"expectedToken":applied["snapshot"]["token"]})).is_err());
        assert_eq!(runtime.load().unwrap()["budget"], applied["snapshot"]["budget"]);
    }
}
