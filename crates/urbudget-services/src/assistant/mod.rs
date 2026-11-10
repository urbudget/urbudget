//! The budget assistant: a chat panel that reads the budget through tools and proposes changes
//! the user applies. It signs in with ChatGPT (the user's plan pays) or uses an OpenAI or
//! Anthropic API key. Conversations and attachments stay in the budget's folder.
mod agent;
mod phone;
mod sms;
mod dispatch;
pub use dispatch::dispatch;
mod anthropic;
pub mod attachments;
mod auth;
pub mod guide;
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
use crate::{t, vault};
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
    /// The service the user was last told their messages go to (the notice before a first
    /// message). Another service asks again.
    pub told_about: Option<Provider>,
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
            told_about: None,
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
    sms_jobs: Mutex<HashMap<String, sms::Job>>,
    /// Bank-text questions waiting for the phone's answer, by request id (`phone.rs`).
    phone: Mutex<HashMap<String, tokio::sync::oneshot::Sender<Value>>>,
    signing_in: Mutex<Option<Arc<AtomicBool>>>,
    sign_in_progress: Mutex<SignInProgress>,
    foreground: AtomicBool,
    models: Mutex<HashMap<Provider, Vec<llm::Model>>>,
    /// Budget folders whose unused attachments were cleared this session.
    pruned: Mutex<std::collections::HashSet<PathBuf>>,
    /// The budget each waiting proposal was last made again on (`budget_stamp`), so a
    /// conversation is brought up to date once per change to the budget, not at every look.
    checked: Mutex<HashMap<String, u64>>,
    /// What's waiting in the budget (`next_steps`), counted once per state of it and month.
    counted: Mutex<Option<(u64, String, [usize; 3])>>,
}

struct RunningTask {
    handle: tokio::task::JoinHandle<()>,
    cancelled: Arc<AtomicBool>,
    /// The text of the round being written, as far as it has got: what Stop keeps.
    written: Arc<Mutex<String>>,
}

#[derive(Clone, Default)]
struct SignInProgress {
    phase: Option<&'static str>,
    error: Option<String>,
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
            sms_jobs: Mutex::new(HashMap::new()),
            phone: Mutex::new(HashMap::new()),
            signing_in: Mutex::new(None),
            sign_in_progress: Mutex::new(SignInProgress::default()),
            foreground: AtomicBool::new(true),
            models: Mutex::new(HashMap::new()),
            pruned: Mutex::new(std::collections::HashSet::new()),
            checked: Mutex::new(HashMap::new()),
            counted: Mutex::new(None),
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
                Failure::new(FailureKind::SignIn, t!("Add your API key in Settings to use the assistant."))
            })?;
            return Ok(Credentials { provider, secret });
        }
        let signed_out = || Failure::new(FailureKind::SignIn, t!("Sign in with ChatGPT to use the assistant."));
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
                t!("UrBudget wasn't allowed to use your ChatGPT plan. Sign in again and allow it."),
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
            .ok_or_else(|| Failure::new(FailureKind::Model, t!("No models are available on this account.")))
    }

    /// Runs `f` on a conversation, loading it first if needed, and saves it when history is kept.
    pub fn with_thread<T>(&self, folder: &Path, id: &str, f: impl FnOnce(&mut Thread) -> T) -> Result<T, String> {
        self.with_thread_guarded(folder, id, None, f)
    }
    fn with_thread_guarded<T>(&self, folder: &Path, id: &str, cancelled: Option<&AtomicBool>, f: impl FnOnce(&mut Thread) -> T) -> Result<T, String> {
        let mut threads = self.loaded(folder, id, cancelled)?;
        let (dir, thread) = threads.get_mut(id).expect("thread loaded");
        let out = f(thread);
        if self.settings().keep_history {
            history::save(dir, thread)?;
        }
        Ok(out)
    }
    /// Reads a conversation, loading it first if needed. A look never writes its file.
    fn peek<T>(&self, folder: &Path, id: &str, f: impl FnOnce(&Thread) -> T) -> Result<T, String> {
        self.peek_guarded(folder, id, None, f)
    }
    fn peek_guarded<T>(&self, folder: &Path, id: &str, cancelled: Option<&AtomicBool>, f: impl FnOnce(&Thread) -> T) -> Result<T, String> {
        let threads = self.loaded(folder, id, cancelled)?;
        Ok(f(&threads.get(id).expect("thread loaded").1))
    }
    /// The conversations in use, with this one among them.
    fn loaded(&self, folder: &Path, id: &str, cancelled: Option<&AtomicBool>) -> Result<MutexGuard<'_, HashMap<String, (PathBuf, Thread)>>, String> {
        let mut threads = lock(&self.threads);
        // Abort can arrive while a local tool is doing synchronous work. Test cancellation
        // under the history lock so that no late tool result can resurrect a deleted chat.
        if cancelled.is_some_and(|signal| signal.load(Ordering::SeqCst)) { return Err(t!("Reply stopped.")); }
        if !threads.get(id).is_some_and(|(dir, _)| dir == folder) {
            let t = history::load(folder, id)?;
            threads.insert(id.to_owned(), (folder.to_owned(), t));
        }
        Ok(threads)
    }
    /// Once per budget folder and session, before the first new attachment: removes attached
    /// files no conversation refers to, such as those of conversations that weren't kept.
    fn prune_once(&self, folder: &Path) {
        if !lock(&self.pruned).insert(folder.to_owned()) {
            return;
        }
        attachments::Files::open(history::files(folder)).discard_waiting();
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
        self.peek(folder, id, |t| t.clone())
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
    /// The notice of what's sent, and to whom, hasn't been shown for the chosen service yet.
    notice_due: bool,
    has_openai_key: bool,
    has_anthropic_key: bool,
    /// Where secrets are kept, or None when they last only until the app closes.
    storage: Option<&'static str>,
    signing_in: bool,
    sign_in_phase: Option<&'static str>,
    sign_in_error: Option<String>,
    /// How to add UrBudget to an MCP client such as Claude Desktop.
    mcp: Value,
}
fn status(a: &Assistant) -> StatusView {
    let s = a.settings();
    let progress = lock(&a.sign_in_progress).clone();
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
        notice_due: s.told_about != Some(s.provider),
        has_openai_key: a.secret(Provider::Openai).is_some(),
        has_anthropic_key: a.secret(Provider::Anthropic).is_some(),
        storage: vault::STORE,
        signing_in: lock(&a.signing_in).is_some(),
        sign_in_phase: progress.phase,
        sign_in_error: progress.error,
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
    sign_in_returning_to(assistant, None).await
}

fn remember_sign_in_client(assistant: &Assistant, cancel: &Arc<AtomicBool>, issued: &str) {
    // Keep the attempt lock through persistence: a canceled callback must not replace
    // the registration used by a newer attempt or by credentials it already saved.
    let busy = lock(&assistant.signing_in);
    if !cancel.load(Ordering::Relaxed) && busy.as_ref().is_some_and(|c| Arc::ptr_eq(c, cancel)) {
        let _ = assistant.update(|s| s.client_id = Some(issued.to_owned()));
    }
}

async fn sign_in_returning_to(assistant: &Assistant, return_link: Option<String>) -> Result<StatusView, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut busy = lock(&assistant.signing_in);
        if let Some(previous) = busy.take() {
            previous.store(true, Ordering::Relaxed);
        }
        *busy = Some(cancel.clone());
        *lock(&assistant.sign_in_progress) = SignInProgress { phase: Some("waiting"), error: None };
    }
    let settings = assistant.settings();
    let a: &Assistant = &assistant;
    let result = auth::sign_in(
        &a.http,
        &settings.host_id,
        settings.client_id.clone(),
        settings.account.as_ref().map(|x| x.email.clone()),
        cancel.clone(),
        return_link,
        auth::open_sign_in,
        |issued| remember_sign_in_client(a, &cancel, issued),
        |phase| {
            let busy = lock(&a.signing_in);
            if busy.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                lock(&a.sign_in_progress).phase = Some(phase);
            }
        },
        || a.foreground.load(Ordering::Relaxed),
    )
    .await;
    let result = async {
        let signed = result?;
        let mut tokens = a.tokens.lock().await;
        // Don't publish a stale success after Cancel, or while a newer attempt is running.
        let busy = lock(&a.signing_in);
        if cancel.load(Ordering::Relaxed) || !busy.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
            return Err(t!("Sign-in was cancelled."));
        }
        lock(&a.sign_in_progress).phase = Some("saving");
        a.keep_secret(Provider::Chatgpt, &signed.tokens.refresh);
        let plan = signed.tokens.plan_usage();
        *tokens = Some(signed.tokens);
        lock(&a.models).remove(&Provider::Chatgpt);
        a.update(|s| {
            s.client_id = Some(signed.client_id.clone());
            s.account = Some(signed.account.clone());
            s.provider = Provider::Chatgpt;
        })?;
        if !plan {
            return Err(t!("You signed in, but didn't allow UrBudget to use your ChatGPT plan. Sign in again and allow it."));
        }
        Ok(())
    }.await;
    {
        let mut busy = lock(&a.signing_in);
        if busy.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
            *lock(&a.sign_in_progress) = SignInProgress {
                phase: Some(if result.is_ok() { "complete" } else { "failed" }),
                error: result.as_ref().err().cloned(),
            };
            *busy = None;
        }
    }
    result?;
    Ok(status(a))
}

pub fn assistant_cancel_sign_in(assistant: &Assistant) {
    if let Some(c) = lock(&assistant.signing_in).take() {
        c.store(true, Ordering::Relaxed);
        *lock(&assistant.sign_in_progress) = SignInProgress { phase: Some("cancelled"), error: None };
    }
}

pub async fn assistant_sign_out(assistant: &Assistant) -> Result<StatusView, String> {
    lock(&assistant.sms_jobs).clear();
    let a: &Assistant = &assistant;
    let settings = a.settings();
    let refresh = a.tokens.lock().await.take().map(|t| t.refresh).or_else(|| a.secret(Provider::Chatgpt));
    a.forget_secret(Provider::Chatgpt);
    lock(&a.models).remove(&Provider::Chatgpt);
    if let (Some(client), Some(token)) = (settings.client_id.as_deref(), refresh) {
        if !auth::revoke(&a.http, client, &token).await {
            // Keep the client id and host id for the next sign-in; only the tokens go.
            return Err(t!("Signed out here. ChatGPT didn't confirm it; you can also disconnect UrBudget in ChatGPT's settings."));
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
    /// The user read the notice of what's sent for the chosen service.
    told: Option<bool>,
}
pub fn assistant_settings(assistant: &Assistant, change: SettingsChange) -> Result<StatusView, String> {
    if change.provider.is_some() || change.model.is_some() { lock(&assistant.sms_jobs).clear(); }
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
        if let Some(v) = change.told {
            s.told_about = v.then_some(s.provider);
        }
    })?;
    Ok(status(&assistant))
}

pub async fn assistant_set_key(assistant: &Assistant, provider: Provider, key: String) -> Result<StatusView, String> {
    let key = key.trim().to_owned();
    if provider == Provider::Chatgpt || key.len() < 20 || key.len() > 400 || key.chars().any(char::is_whitespace) {
        return Err(t!("Paste the whole API key."));
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
    lock(&assistant.sms_jobs).clear();
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

/// The conversation as the panel shows it, with its waiting proposals made again on the budget
/// as the window has it: a card that would change says so before Apply is pressed, not after.
pub fn assistant_thread(db: &Database, assistant: &Assistant, id: String, patch: Option<Patch>) -> Result<Value, String> {
    let folder = folder(&db)?;
    // A budget that can't be read doesn't keep the conversation from being shown.
    let _ = refresh_proposals(db, assistant, &folder, &id, patch.clone());
    let provider = assistant.settings().provider;
    let mut view = assistant.peek(&folder, &id, |t| history::view(t, provider))?;
    let running = assistant.is_running(&id);
    view["running"] = json!(running);
    if !running {
        let next = next_steps(db, assistant, &folder, &id, patch, &view).unwrap_or_default();
        if !next.is_empty() {
            view["next"] = json!(next);
        }
    }
    Ok(view)
}

/// What the panel offers under the conversation's last reply: what in the budget is waiting
/// to be done that the assistant can help with. Each is a button's words and the message it
/// sends. They come from the budget by rule, not from the model: overspent categories in the
/// month the user last asked about, spending without a category, and imports waiting for
/// approval. Nothing is offered while a card waits (the card is the next step), or for what
/// the reply itself just did.
fn next_steps(db: &Database, assistant: &Assistant, folder: &Path, id: &str, patch: Option<Patch>, view: &Value) -> Result<Vec<Value>, String> {
    let Some(last) = view["entries"].as_array().and_then(|e| e.last()).filter(|e| e["kind"] == "assistant") else { return Ok(vec![]) };
    let (waiting, month) = assistant.peek(folder, id, |t| {
        let month = t.items.iter().rev().find_map(|i| match i {
            Item::User { sent_with: Some(with), .. } if !with.month.is_empty() => Some(with.month.clone()),
            _ => None,
        });
        (t.proposals.iter().any(|p| matches!(p.status, Status::Proposed | Status::Undone) && !p.made()), month)
    })?;
    if waiting {
        return Ok(vec![]);
    }
    let month = month.unwrap_or_else(|| chrono::Local::now().format("%Y-%m").to_string());
    let stamp = budget_stamp(db, &patch)?;
    let counts = {
        let known = lock(&assistant.counted).clone();
        match known.filter(|(at, of, _)| *at == stamp && *of == month) {
            Some((_, _, counts)) => counts,
            None => {
                let counts = db.read(patch, |b| {
                    let names = tools::Names::new(b);
                    let overspent = crate::engine::calculate(b, &month)?.categories.iter().filter(|c| c.available < 0).count();
                    let uncategorized = b.transactions.iter().filter(|t| names.needs_category(t)).count();
                    let unapproved = b.transactions.iter().filter(|t| t.extra.get("approved") == Some(&json!(false))).count();
                    Ok([overspent, uncategorized, unapproved])
                })?;
                *lock(&assistant.counted) = Some((stamp, month.clone(), counts));
                counts
            }
        }
    };
    let used = |tool: &str| last["tools"].as_array().is_some_and(|tools| tools.iter().any(|t| t == tool));
    let [overspent, uncategorized, unapproved] = counts;
    let mut steps = vec![];
    if overspent > 0 && !used("propose_budget_moves") && !used("propose_auto_assign") {
        steps.push(json!({ "id": "cover", "text": t!("Cover {n|# overspent category|# overspent categories}", n = overspent), "prompt": t!("Cover my overspent categories this month.") }));
    }
    if uncategorized > 0 && !used("propose_transaction_edits") {
        steps.push(json!({ "id": "categorize", "text": t!("Categorize {n|# transaction|# transactions}", n = uncategorized), "prompt": t!("Categorize my transactions that have no category yet.") }));
    }
    if unapproved > 0 && !used("propose_transaction_edits") {
        steps.push(json!({ "id": "review", "text": t!("Review {n|# imported transaction|# imported transactions}", n = unapproved), "prompt": t!("Help me review the imported transactions waiting for approval.") }));
    }
    steps.truncate(2);
    Ok(steps)
}

/// What tells one state of the budget from another, cheaply: the saved budget's token and the
/// window's unsaved changes.
fn budget_stamp(db: &Database, patch: &Option<Patch>) -> Result<u64, String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    db.store()?.status()?.token.hash(&mut hasher);
    serde_json::to_string(patch).unwrap_or_default().hash(&mut hasher);
    Ok(hasher.finish())
}

/// Makes the conversation's waiting proposals again when the budget has changed since each was
/// last made: one that still makes the same change is left alone; one that would differ is
/// replaced by the card as it would be now, saying so; one that can't be made any more fails.
fn refresh_proposals(db: &Database, assistant: &Assistant, folder: &Path, id: &str, patch: Option<Patch>) -> Result<(), String> {
    let waiting: Vec<proposals::Proposal> = assistant.peek(folder, id, |t| t.proposals.iter().filter(|p| p.waiting_change()).cloned().collect())?;
    if waiting.is_empty() {
        return Ok(());
    }
    let stamp = budget_stamp(db, &patch)?;
    let due: Vec<proposals::Proposal> = {
        let checked = lock(&assistant.checked);
        waiting.into_iter().filter(|p| checked.get(&p.id) != Some(&stamp)).collect()
    };
    if due.is_empty() {
        return Ok(());
    }
    let changed: Vec<proposals::Proposal> = db.read(patch, |current| {
        Ok(due.iter().filter_map(|p| match proposals::recheck(p, current) {
            Recheck::Same(_) => None,
            Recheck::Changed(now) => Some(now),
        }).collect())
    })?;
    if !changed.is_empty() {
        assistant.with_thread(folder, id, |t| {
            for now in changed {
                // Only as it stood when it was made again: applied or set aside meanwhile, it stays.
                if let Some(p) = t.proposals.iter_mut().find(|p| p.id == now.id && p.waiting_change()) {
                    *p = now;
                }
            }
        })?;
    }
    let mut checked = lock(&assistant.checked);
    for p in due {
        checked.insert(p.id, stamp);
    }
    Ok(())
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

/// Keeps a file the user chose for their next message, until it's sent or taken off again
/// (`assistant_detach`). `data` is base64.
pub fn assistant_attach(
    db: &Database,
    assistant: &Assistant,
    name: String,
    data: String,
) -> Result<attachments::Meta, String> {
    use base64::Engine;
    assistant.prune_once(&folder(&db)?);
    if data.len() > 40 << 20 {
        return Err(t!("{name} is too large to attach.", name));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|_| t!("The file couldn't be read."))?;
    attachments::Files::open(history::files(&folder(&db)?)).stage(&name, &bytes)
}

/// Forgets a file the user took off their message before sending it.
pub fn assistant_detach(db: &Database, id: String) -> Result<(), String> {
    attachments::Files::open(history::files(&folder(db)?)).discard(&id);
    Ok(())
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Proposed => "not applied yet",
        Status::Applied => "applied",
        Status::Discarded => "discarded",
        Status::Undone => "applied, then undone",
        Status::Failed => "couldn't be applied",
        Status::Replaced => "replaced by a newer proposal",
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
        return Err(t!("Type a message or attach a file."));
    }
    // Characters, as the message fields count them: Arabic takes two bytes a letter.
    if text.chars().count() > 20_000 {
        return Err(t!("That message is too long."));
    }
    if attachments.len() > 5 {
        return Err(t!("Attach up to 5 files at a time."));
    }
    if screen.selected.len() > SELECTED {
        return Err(t!("Attach up to {limit} transactions at a time.", limit = SELECTED));
    }
    let folder = folder(&db)?;
    let budget = db.snapshot(patch)?;
    let settings = assistant.settings();
    let files = attachments::Files::open(history::files(&folder));
    let metas = attachments.iter().map(|id| files.get(id)).collect::<Result<Vec<_>, _>>()?;
    let id = match thread_id {
        Some(id) => {
            if assistant.is_running(&id) {
                return Err(t!("Wait for the current reply, or stop it."));
            }
            assistant.thread_copy(&folder, &id)?;
            id
        }
        None => {
            let id = new_id("t");
            let mut title: String = text.chars().take(60).collect();
            if title.is_empty() {
                title = metas.first().map(|m| m.name.clone()).unwrap_or_else(|| t!("New conversation"));
            }
            let thread = Thread {
                id: id.clone(),
                title,
                created: now(),
                updated: now(),
                instructions: prompt::in_language(prompt::INSTRUCTIONS),
                language: crate::i18n::language().code().into(),
                bank_texts: Some(screen.bank_texts),
                ..Thread::default()
            };
            lock(&assistant.threads).insert(id.clone(), (folder.clone(), thread));
            id
        }
    };
    let attached_before: usize = assistant.with_thread(&folder, &id, |t| {
        t.items.iter().map(|i| if let Item::User { attachments, .. } = i { attachments.len() } else { 0 }).sum()
    })?;
    if attached_before + metas.len() > 12 {
        return Err(t!("This conversation has as many files as it can hold. Start a new conversation to attach more."));
    }
    // The file being imported, as it reads now: kept with the message, so a fix the assistant
    // proposes is for these lines as they are now (a line changed since won't take it).
    let import = screen.import.as_ref().and_then(|i| {
        crate::bank_statement::statement(&budget, &i.account, &i.content, &i.session).ok().map(|s| s.brief(settings.share_memos))
    });
    // Sent: its files are the conversation's from here on.
    for id in &attachments {
        files.adopt(id)?;
    }
    let names = tools::Names::new(&budget);
    let sent_with = history::SentWith {
        page: screen.page.clone(),
        month: screen.month.clone(),
        account: screen.account_id.as_deref().filter(|a| names.accounts.contains_key(a)).map(|a| names.account(a)),
        category: screen.category_id.as_deref().filter(|c| names.categories.contains_key(c)).map(|c| names.category(Some(c))),
        transactions: budget.transactions.iter().filter(|t| screen.selected.contains(&t.id)).count(),
        selected: budget.transactions.iter().filter(|t| screen.selected.contains(&t.id)).map(|t| t.id.clone()).collect(),
        import: import.is_some(),
    };
    assistant.with_thread(&folder, &id, |t| {
        history::settle(&mut t.items);
        let stopped = matches!(t.items.last(), Some(Item::Stopped));
        settle_start(t, screen.bank_texts);
        // A question the assistant asked is answered by this message, tapped or typed.
        for asked in t.proposals.iter_mut().filter(|p| p.kind == "question" && p.status == Status::Proposed) {
            asked.status = Status::Applied;
        }
        let earlier: Vec<String> = t
            .proposals
            .iter()
            .filter(|p| p.kind != "question")
            .map(|p| match p.choices() {
                // A report or a file was made, not proposed.
                _ if p.made() => format!("\u{201c}{}\u{201d} {}", p.title, if p.status == Status::Discarded { "removed by the user" } else { "made for the user" }),
                Some(choices) => format!("\u{201c}{}\u{201d} {} ({choices})", p.title, status_word(p.status)),
                None => format!("\u{201c}{}\u{201d} {}", p.title, status_word(p.status)),
            })
            .collect();
        let mut context = prompt::context(&budget, &screen, settings.share_memos, &earlier, import.as_ref(), stopped);
        // The first message of a conversation that carries on another says what came before.
        if let Some(carried) = t.carried.as_ref().filter(|_| !t.items.iter().any(|i| matches!(i, Item::User { .. }))) {
            context.push_str(&format!("\nThis conversation carries on an earlier one, \u{201c}{}\u{201d}, that had grown too long. What was said last in it, as background and not as a request:\n{}", carried.from, carried.text));
        }
        t.items.push(Item::User { text: text.clone(), attachments: metas, context, at: now(), import, sent_with: Some(sent_with) });
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
        bank_texts: screen.bank_texts,
        platform: screen.platform.clone(),
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
        return Err(t!("Wait for the current reply, or stop it."));
    }
    let folder = folder(&db)?;
    let budget = db.snapshot(patch)?;
    assistant.with_thread(&folder, &thread_id, |t| {
        history::settle(&mut t.items);
        settle_start(t, screen.bank_texts);
        // A reply that ran out of room is asked for its rest, in a note: the conversation
        // can't end on the model's own words.
        if matches!(t.items.last(), Some(Item::Cut)) {
            t.items.push(Item::Note { text: CARRY_ON.into() });
        }
    })?;
    let job = agent::Job {
        thread_id: thread_id.clone(),
        folder,
        budget,
        today: screen.today.clone(),
        month: if screen.month.is_empty() { screen.today.chars().take(7).collect() } else { screen.month.clone() },
        share_memos: assistant.settings().share_memos,
        bank_sync: simplefin.snapshot(),
        bank_texts: screen.bank_texts,
        platform: screen.platform.clone(),
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
    let written = Arc::new(Mutex::new(String::new()));
    let writing = written.clone();
    let handle = crate::executor().spawn(async move {
        if waiting.await.is_ok() { agent::run(owned, job, signal, writing).await; }
    });
    lock(&assistant.running).insert(id, RunningTask { handle, cancelled, written });
    let _ = ready.send(());
}

/// At most this many selected transactions go with a message.
const SELECTED: usize = 200;
const CARRY_ON: &str = "Your last reply stopped at its length limit. Carry on from exactly where it stopped, without repeating what you've written.";

/// Settles what a conversation's requests begin with, so it never changes afterwards (see
/// `anthropic.rs`): the instructions with their language part, and whether the bank-text tools
/// are offered. A change of the app's language since is said in a note.
fn settle_start(t: &mut Thread, bank_texts: bool) {
    let now = crate::i18n::language().code();
    if t.language.is_empty() {
        // A conversation from before its language was kept with it. Its requests so far began
        // with the instructions in the app's language of the time: most likely this one.
        if t.instructions.is_empty() {
            t.instructions = prompt::INSTRUCTIONS.into();
        }
        t.instructions = prompt::in_language(&t.instructions);
    } else if t.language != now {
        t.items.push(Item::Note { text: prompt::language_note().into() });
    }
    t.language = now.into();
    if t.bank_texts.is_none() {
        t.bank_texts = Some(bank_texts);
    }
}

/// The change that makes the budget's notes for the assistant exactly `notes`, for the window to
/// apply: what the assistant's settings save.
pub fn assistant_notes(db: &Database, patch: Option<Patch>, notes: Vec<String>) -> Result<Patch, String> {
    db.read(patch, |current| proposals::notes_patch(current, notes))
}

/// Gives a conversation another title.
pub fn assistant_rename_thread(db: &Database, assistant: &Assistant, id: String, title: String) -> Result<(), String> {
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
    if title.is_empty() {
        return Err(t!("Give the conversation a name."));
    }
    assistant.with_thread(&folder(db)?, &id, |t| t.title = title)
}

/// Starts a conversation that carries on one that has grown long: it begins with what was said
/// last in the earlier one, and none of its lookups or files. Returns the new conversation's id.
pub fn assistant_continue(db: &Database, assistant: &Assistant, thread_id: String) -> Result<String, String> {
    if assistant.is_running(&thread_id) {
        return Err(t!("Wait for the current reply, or stop it."));
    }
    let folder = folder(db)?;
    let earlier = assistant.thread_copy(&folder, &thread_id)?;
    let id = new_id("t");
    let thread = Thread {
        id: id.clone(),
        title: t!("{title} (continued)", title = tools::truncate(&earlier.title, 50)),
        created: now(),
        updated: now(),
        instructions: prompt::in_language(prompt::INSTRUCTIONS),
        language: crate::i18n::language().code().into(),
        bank_texts: earlier.bank_texts,
        carried: Some(history::Carried { from: earlier.title.clone(), text: history::carry(&earlier) }),
        ..Thread::default()
    };
    lock(&assistant.threads).insert(id.clone(), (folder.clone(), thread));
    assistant.with_thread(&folder, &id, |_| ())?;
    Ok(id)
}

/// Keeps the user's thumb on a reply (`up`), or takes it off (`None`). `index` is the reply's
/// place in the conversation, as its view gives it. Ratings stay on the device.
pub fn assistant_rate(db: &Database, assistant: &Assistant, thread_id: String, index: usize, up: Option<bool>) -> Result<(), String> {
    assistant.with_thread(&folder(db)?, &thread_id, |t| {
        if !matches!(t.items.get(index), Some(Item::Assistant { .. })) {
            return Err(t!("That reply is gone."));
        }
        match up {
            Some(up) => t.ratings.insert(index, up),
            None => t.ratings.remove(&index),
        };
        Ok(())
    })?
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
        .ok_or_else(|| t!("That report is gone."))?;
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
    .map_err(|_| t!("The report timed out. Your text is still here; try again when you're online."))?
    .map_err(|failure| {
        if failure.status == 404 {
            t!("Reporting isn't available right now. Your text is still here; try again later.")
        } else {
            failure.message
        }
    })?;
    answer["id"].as_str().filter(|id| id.starts_with("rep_")).map(str::to_owned)
        .ok_or_else(|| t!("The report service did not confirm a reference. Please try again later."))
}

fn report_payload(text: &str, reason: &str) -> Result<Value, String> {
    let text = text.trim();
    let reason = reason.trim();
    // Match the browser's maxlength and the API's JavaScript string lengths, including emoji.
    if text.is_empty() || text.encode_utf16().count() > 4000 || reason.encode_utf16().count() > 1000 {
        return Err(t!("Include 1–4,000 characters from the reply and at most 1,000 characters explaining the problem."));
    }
    Ok(json!({ "text": text, "reason": reason }))
}

pub fn assistant_stop(db: &Database, assistant: &Assistant, thread_id: String) -> Result<(), String> {
    if let Some(h) = lock(&assistant.running).remove(&thread_id) {
        h.abort();
        // What it had written of this round stays, on the screen and for the model next time.
        let written = std::mem::take(&mut *lock(&h.written));
        let folder = folder(&db)?;
        assistant.with_thread(&folder, &thread_id, |t| {
            history::settle(&mut t.items);
            if !written.trim().is_empty() {
                t.items.push(Item::Assistant { text: written });
            }
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
///
/// With `settle` the card is marked applied here, before the budget has the change: the way
/// apps worked before they marked it themselves. Without it the card stays as it is, and the
/// app marks it applied (`assistant_mark`) once the change is in the budget, so a card never
/// says Applied for a change that was never saved.
pub fn assistant_apply(
    db: &Database,
    assistant: &Assistant,
    patch: Option<Patch>,
    thread_id: String,
    proposal_id: String,
    settle: bool,
) -> Result<Applied, String> {
    let folder = folder(&db)?;
    let proposal = assistant
        .thread_copy(&folder, &thread_id)?
        .proposals
        .into_iter()
        .find(|p| p.id == proposal_id)
        .ok_or_else(|| t!("That proposal is gone."))?;
    if !matches!(proposal.status, Status::Proposed | Status::Undone) {
        return Err(t!("This proposal was already handled."));
    }
    let outcome = db.read(patch, |current| Ok(proposals::recheck(&proposal, current)))?;
    assistant.with_thread(&folder, &thread_id, |t| {
        let p = t.proposals.iter_mut().find(|p| p.id == proposal_id).expect("proposal present");
        match outcome {
            Recheck::Same(patch) => {
                if settle {
                    applied(p);
                }
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
        .ok_or_else(|| t!("That proposal is gone."))?;
    let revised = db.read(patch, |current| proposals::revise(&proposal, revision, current))?;
    assistant.with_thread(&folder, &thread_id, |t| {
        let p = t.proposals.iter_mut().find(|p| p.id == proposal_id).ok_or_else(|| t!("That proposal is gone."))?;
        // Applied or discarded meanwhile: the choice is for a card that's done.
        if p.status != proposal.status {
            return Err(t!("This proposal was already handled."));
        }
        *p = revised;
        Ok(history::proposal_view(p))
    })?
}

/// A card whose change is in the budget.
fn applied(p: &mut proposals::Proposal) {
    p.status = Status::Applied;
    p.applied_at = Some(now());
    p.updated = false;
    p.problem = None;
}

/// Records what an app did with a proposal: applied (its change is in the budget, or its import
/// or fixes went through), set aside, put back, or undone. Only steps that can follow from where
/// the card stands are taken: an app can't make a failed card applied, or an applied one new.
/// `note` is what the app has to say about it, such as which of an import's fixes didn't take.
pub fn assistant_mark(
    db: &Database,
    assistant: &Assistant,
    thread_id: String,
    proposal_id: String,
    status: Status,
    note: Option<String>,
) -> Result<Value, String> {
    use Status::*;
    let folder = folder(&db)?;
    assistant.with_thread(&folder, &thread_id, |t| {
        let p = t.proposals.iter_mut().find(|p| p.id == proposal_id).ok_or_else(|| t!("That proposal is gone."))?;
        let follows = match (p.status, status) {
            (from, to) if from == to => true,
            (Proposed | Undone | Failed, Discarded) => true,
            (Discarded | Replaced, Proposed) => true,
            (Proposed | Undone, Applied) => true,
            (Applied, Undone) => true,
            // An app that marks a card applied before saving puts it back when the save fails.
            (Applied, Proposed) => true,
            _ => false,
        };
        if !follows {
            return Err(t!("This proposal was already handled."));
        }
        match status {
            Applied => applied(p),
            Proposed => {
                p.status = Proposed;
                p.replaced_by = None;
            }
            other => p.status = other,
        }
        if let Some(note) = note.map(|n| n.trim().chars().take(600).collect::<String>()).filter(|n| !n.is_empty()) {
            p.problem = Some(note);
        }
        Ok(history::proposal_view(p))
    })?
}

/// Native atomic undo verifies history before changing money; deleted conversations cannot
/// turn a subsequent missing-card error into an apparently failed but already-applied undo.
pub fn verify_undo_proposal(db: &Database, assistant: &Assistant, thread_id: &str, proposal_id: &str) -> Result<(), String> {
    let folder = folder(db)?;
    let thread = assistant.thread_copy(&folder, thread_id)?;
    let proposal = thread.proposals.iter().find(|p| p.id == proposal_id).ok_or_else(|| t!("That proposal is gone."))?;
    if proposal.status != Status::Applied { return Err(t!("That proposal is no longer applied.")); }
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
        .ok_or_else(|| t!("That import is gone."))?;
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
    std::fs::write(&path, bytes).map_err(|e| t!("Couldn't save the spreadsheet: {e}", e))?;
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
        .ok_or_else(|| t!("That spreadsheet is gone."))
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
        _ => return Err(t!("That spreadsheet is gone.")),
    };
    if grids.is_empty() {
        return Err(t!("This report has no tables to put in a spreadsheet."));
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

/// The services as tests need them: with a budget, and with a model that answers from a script.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// The services with the sample budget open and saved.
    pub fn with_budget(directory: &Path) -> crate::runtime::Runtime {
        let runtime = crate::runtime::Runtime::new(directory).unwrap();
        let initial = runtime.load().unwrap();
        crate::block_on(runtime.dispatch("save_budget", json!({ "budget": urbudget_core::test_support::sample(), "expectedToken": initial["token"] }))).unwrap();
        runtime
    }
    /// The same, set to a model of the test's own, whose replies come from `llm::script`.
    pub fn scripted(directory: &Path, model: &str) -> crate::runtime::Runtime {
        let runtime = with_budget(directory);
        // Kept by this assistant alone: the secrets file is one for the whole test process, and
        // another test may clear it while this one's reply is running.
        lock(&runtime.assistant.memory).insert(secret_service(Provider::Openai), "sk-a-key-for-tests-that-is-never-sent".into());
        let change: SettingsChange = serde_json::from_value(json!({ "provider": "openai", "model": model })).unwrap();
        assistant_settings(&runtime.assistant, change).unwrap();
        runtime
    }
    pub fn screen() -> prompt::Screen {
        serde_json::from_value(json!({ "page": "budget", "month": "2026-10", "today": "2026-10-21" })).unwrap()
    }
    /// Sends a message and waits for its reply to end. Returns the conversation's id.
    pub fn say(runtime: &crate::runtime::Runtime, thread: Option<&str>, text: &str, screen: prompt::Screen) -> String {
        let id = assistant_send(&runtime.simplefin, &runtime.db, &runtime.assistant, None, thread.map(str::to_owned), text.into(), vec![], screen).unwrap();
        finished(runtime, &id);
        id
    }
    pub fn finished(runtime: &crate::runtime::Runtime, id: &str) {
        crate::block_on(async {
            for _ in 0..1000 {
                if !runtime.assistant.is_running(id) { break; }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        });
        assert!(!runtime.assistant.is_running(id), "the reply never ended");
    }
    pub fn items(runtime: &crate::runtime::Runtime, id: &str) -> Vec<Item> {
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        runtime.assistant.thread_copy(&folder, id).unwrap().items
    }
    pub fn view(runtime: &crate::runtime::Runtime, id: &str) -> Value {
        assistant_thread(&runtime.db, &runtime.assistant, id.to_owned(), None).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::testing::with_budget;

    #[test]
    fn stale_sign_in_callbacks_cannot_replace_the_current_registration() {
        let directory = tempfile::tempdir().unwrap();
        let assistant = Assistant::new(directory.path().join("settings"));
        let old = Arc::new(AtomicBool::new(false));
        let current = Arc::new(AtomicBool::new(false));
        *lock(&assistant.signing_in) = Some(current.clone());
        remember_sign_in_client(&assistant, &current, "current-client");
        remember_sign_in_client(&assistant, &old, "stale-client");
        assert_eq!(assistant.settings().client_id.as_deref(), Some("current-client"));
        assistant_cancel_sign_in(&assistant);
        remember_sign_in_client(&assistant, &current, "cancelled-client");
        assert_eq!(assistant.settings().client_id.as_deref(), Some("current-client"));
        let reloaded = Assistant::new(directory.path().join("settings"));
        assert_eq!(reloaded.settings().client_id.as_deref(), Some("current-client"));
    }

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
        let thread = Thread { id: "t_private".into(), title: "Private".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![], ..Default::default() };
        history::save(a.path(), &thread).unwrap();
        assert!(assistant.thread_copy(a.path(), &thread.id).is_ok());
        assert!(assistant.thread_copy(b.path(), &thread.id).is_err());
        assert!(!history::files(b.path()).exists());
    }

    #[test]
    fn cancelled_tool_results_cannot_resurrect_a_conversation_during_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let assistant = Assistant::new(directory.path().join("settings"));
        let thread = Thread { id: "t_cancelled".into(), title: "Synthetic".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![], ..Default::default() };
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
        let thread = Thread { id: id.clone(), title: "Test".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![], ..Default::default() };
        history::save(folder.path(), &thread).unwrap();
        let job = agent::Job { thread_id: id.clone(), folder: folder.path().to_path_buf(), budget: crate::domain::Budget::default(),
            today: "2026-10-21".into(), month: "2026-10".into(), share_memos: false, bank_sync: Value::Null, bank_texts: false, platform: String::new() };
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
    fn a_conversations_start_is_settled_once_and_a_change_of_language_is_a_note() {
        // From before its language was kept with it: settled as the app is now, English here.
        let mut old = Thread { instructions: prompt::INSTRUCTIONS.into(), ..Thread::default() };
        settle_start(&mut old, true);
        assert_eq!((old.language.as_str(), old.bank_texts, old.instructions.as_str()), ("en", Some(true), prompt::INSTRUCTIONS));
        // Settled, it stays: another phone reading texts doesn't change what it was offered.
        settle_start(&mut old, false);
        assert_eq!((old.bank_texts, old.items.len()), (Some(true), 0));
        // Begun in Arabic, the app now in English: its instructions stay, and a note says so.
        let arabic = format!("{}\n\n{}", prompt::INSTRUCTIONS, prompt::ARABIC);
        let mut switched = Thread { instructions: arabic.clone(), language: "ar".into(), bank_texts: Some(false), ..Thread::default() };
        settle_start(&mut switched, false);
        assert_eq!(switched.instructions, arabic);
        assert!(matches!(&switched.items[..], [Item::Note { text }] if text.contains("switched the app to English")));
        settle_start(&mut switched, false);
        assert_eq!((switched.items.len(), switched.language.as_str()), (1, "en"));
    }

    #[test]
    fn a_conversation_is_renamed_rated_and_carried_on() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = with_budget(directory.path());
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        let said = |text: &str| Item::User { text: text.into(), attachments: vec![], context: String::new(), at: now(), import: None, sent_with: None };
        let thread = Thread { id: "t_long".into(), title: "October".into(), created: now(), updated: now(),
            items: vec![said("How is October going?"), Item::Assistant { text: "You've spent 1,240.00 so far.".into() }], bank_texts: Some(false), ..Thread::default() };
        history::save(&folder, &thread).unwrap();

        assistant_rename_thread(&runtime.db, &runtime.assistant, thread.id.clone(), "  October   so far ".into()).unwrap();
        assert!(assistant_rename_thread(&runtime.db, &runtime.assistant, thread.id.clone(), "  ".into()).is_err());
        assert_eq!(assistant_threads(&runtime.db, &runtime.assistant).unwrap()[0].title, "October so far");

        // A thumb is on a reply, by its place; it can be changed and taken off.
        assistant_rate(&runtime.db, &runtime.assistant, thread.id.clone(), 1, Some(false)).unwrap();
        assert!(assistant_rate(&runtime.db, &runtime.assistant, thread.id.clone(), 0, Some(true)).is_err());
        let view = assistant_thread(&runtime.db, &runtime.assistant, thread.id.clone(), None).unwrap();
        assert_eq!((&view["entries"][1]["rating"], &view["entries"][1]["index"]), (&json!("down"), &json!(1)));
        assistant_rate(&runtime.db, &runtime.assistant, thread.id.clone(), 1, None).unwrap();
        assert!(assistant_thread(&runtime.db, &runtime.assistant, thread.id.clone(), None).unwrap()["entries"][1].get("rating").is_none());

        // Carried on: a new conversation that begins with what was said last, and none of the rest.
        let next = assistant_continue(&runtime.db, &runtime.assistant, thread.id.clone()).unwrap();
        let carried = runtime.assistant.thread_copy(&folder, &next).unwrap();
        assert_eq!(carried.title, "October so far (continued)");
        let text = carried.carried.unwrap().text;
        assert!(text.contains("The user: How is October going?") && text.contains("You: You've spent 1,240.00 so far."));
        assert_eq!(assistant_thread(&runtime.db, &runtime.assistant, next, None).unwrap()["continuedFrom"], "October so far");
    }

    #[test]
    fn a_waiting_card_is_brought_up_to_date_when_its_conversation_is_looked_at() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = with_budget(directory.path());
        let call = |method: &str, p: Value| crate::block_on(runtime.dispatch(method, p));
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        let before = urbudget_core::test_support::sample();
        let give = proposals::build(proposals::Change::Moves {
            month: "2026-10".into(), moves: vec![proposals::Move { from: None, to: Some("food".into()), amount: 1000 }],
        }, "Give food more room".into(), &before).unwrap();
        let thread = Thread { id: "t_fresh".into(), title: "Fresh".into(), created: now(), updated: now(), proposals: vec![give.clone()], items: vec![
            Item::Result { call_id: "c1".into(), output: "{}".into(), step: String::new(), failed: false, proposal: Some(give.id.clone()) },
        ], ..Default::default() };
        history::save(&folder, &thread).unwrap();
        let card = || assistant_thread(&runtime.db, &runtime.assistant, thread.id.clone(), None).unwrap()["entries"][0]["proposal"].clone();
        // The budget as it was proposed on: the card as it was.
        assert_eq!((card()["card"]["rows"][0]["after"].as_i64(), card().get("message")), (Some(21000), None));
        // Food is given 100.00 more by hand: the card would now leave it somewhere else, and says so.
        let mut edited = before.clone();
        edited.assignments[0].amount = 30000;
        let token = runtime.load().unwrap()["token"].clone();
        call("save_budget", json!({ "budget": edited, "expectedToken": token })).unwrap();
        let now_card = card();
        assert_eq!((now_card["card"]["rows"][0]["after"].as_i64(), now_card["message"]["tone"].as_str(), now_card["status"].as_str()), (Some(31000), Some("info"), Some("proposed")));
        // Applying it now applies it, rather than coming back changed.
        let applied = call("assistant_apply_commit", json!({ "threadId": thread.id, "proposalId": give.id })).unwrap();
        assert_eq!((applied["proposal"]["status"].as_str(), applied["snapshot"]["budget"]["assignments"][0]["amount"].as_i64()), (Some("applied"), Some(31000)));
        assert!(applied["proposal"]["appliedAt"].is_string() && applied["proposal"].get("message").is_none());

        // The budget's own Undo takes the card's change back, and Redo puts it in again; the
        // conversation says which of its cards Undo would take back.
        let looked = call("assistant_thread", json!({ "id": thread.id })).unwrap();
        assert_eq!(looked["undoable"], json!(give.id));
        let undone = call("undo_budget", json!({ "expectedToken": applied["snapshot"]["token"] })).unwrap();
        assert_eq!(card()["status"], "undone");
        assert!(call("assistant_thread", json!({ "id": thread.id })).unwrap().get("undoable").is_none());
        call("redo_budget", json!({ "expectedToken": undone["token"] })).unwrap();
        assert_eq!(card()["status"], "applied");
        assert_eq!(call("assistant_thread", json!({ "id": thread.id })).unwrap()["undoable"], json!(give.id));
    }

    #[test]
    fn what_is_left_of_a_card_is_still_offered_when_a_transaction_it_names_is_deleted() {
        let mut b = urbudget_core::test_support::sample();
        let bought = |id: &str, payee: &str| urbudget_core::domain::Transaction { id: id.into(), payee: payee.into(), ..urbudget_core::test_support::purchase() };
        b.transactions = vec![bought("a", "Shop"), bought("b", "Bakery")];
        let memo = |id: &str| proposals::Edit { id: id.into(), amount: None, previous_amount: None, category: None, payee: None, memo: Some("October".into()), cleared: None, approve: false, flag: None, unsure: false, ..Default::default() };
        let both = proposals::build(proposals::Change::Edits { edits: vec![memo("a"), memo("b")], rules: vec![] }, "Note two purchases".into(), &b).unwrap();
        assert_eq!(both.card.rows.len(), 2);
        b.transactions.remove(0);
        let Recheck::Changed(rest) = proposals::recheck(&both, &b) else { panic!("the card should have changed") };
        assert_eq!((rest.status, rest.card.rows.len(), rest.updated), (Status::Proposed, 1, true));
        assert!(rest.problem.as_deref().unwrap().starts_with("1 transaction in this was deleted"));
        // What's left applies as it now reads.
        assert!(matches!(proposals::recheck(&rest, &b), Recheck::Same(_)));
        // With the other gone too, nothing is left to offer.
        b.transactions.clear();
        assert!(matches!(proposals::recheck(&rest, &b), Recheck::Changed(gone) if gone.status == Status::Failed));
    }

    #[test]
    fn an_app_can_only_take_a_card_where_it_can_go_from_where_it_stands() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = with_budget(directory.path());
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        let give = proposals::build(proposals::Change::Moves {
            month: "2026-10".into(), moves: vec![proposals::Move { from: None, to: Some("food".into()), amount: 1000 }],
        }, "Give food more room".into(), &urbudget_core::test_support::sample()).unwrap();
        let thread = Thread { id: "t_mark".into(), title: "Mark".into(), created: now(), updated: now(), proposals: vec![give.clone()], ..Default::default() };
        history::save(&folder, &thread).unwrap();
        let mark = |status: Status, note: Option<&str>| assistant_mark(&runtime.db, &runtime.assistant, thread.id.clone(), give.id.clone(), status, note.map(str::to_owned));
        // Set aside and put back; applied, undone, applied again.
        assert_eq!(mark(Status::Discarded, None).unwrap()["actions"], json!(["restore"]));
        assert!(mark(Status::Applied, None).is_err() && mark(Status::Undone, None).is_err());
        assert_eq!(mark(Status::Proposed, None).unwrap()["status"], "proposed");
        assert!(mark(Status::Applied, None).unwrap()["appliedAt"].is_string());
        assert!(mark(Status::Discarded, None).is_err() && mark(Status::Failed, None).is_err());
        assert_eq!(mark(Status::Undone, None).unwrap()["status"], "undone");
        // What the app has to say about it is the card's message.
        let noted = mark(Status::Applied, Some("2 of 3 fixes were applied.")).unwrap();
        assert_eq!(noted["message"], json!({ "tone": "warn", "text": "2 of 3 fixes were applied." }));
        // Asked to apply without settling it, the card stays as it is until the app marks it.
        mark(Status::Undone, None).unwrap();
        let asked = assistant_apply(&runtime.db, &runtime.assistant, None, thread.id.clone(), give.id.clone(), false).unwrap();
        assert!(asked.patch.is_some() && asked.proposal["status"] == "undone");
        assert_eq!(assistant_apply(&runtime.db, &runtime.assistant, None, thread.id.clone(), give.id.clone(), true).unwrap().proposal["status"], "applied");
    }

    #[test]
    fn stop_keeps_what_the_reply_had_written() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = with_budget(directory.path());
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        let said = Item::User { text: "How is October going?".into(), attachments: vec![], context: String::new(), at: now(), import: None, sent_with: None };
        let thread = Thread { id: "t_stop".into(), title: "Stop".into(), created: now(), updated: now(), instructions: String::new(), items: vec![said], proposals: vec![], ..Default::default() };
        history::save(&folder, &thread).unwrap();
        // A reply in the middle of its sentence.
        let running = |written: &str| {
            let handle = crate::executor().spawn(std::future::pending::<()>());
            lock(&runtime.assistant.running).insert(thread.id.clone(), RunningTask { handle, cancelled: Arc::new(AtomicBool::new(false)), written: Arc::new(Mutex::new(written.into())) });
        };
        running("You've spent 1,240.00 of");
        assistant_stop(&runtime.db, &runtime.assistant, thread.id.clone()).unwrap();
        let items = runtime.assistant.thread_copy(&folder, &thread.id).unwrap().items;
        assert!(matches!(&items[1..], [Item::Assistant { text }, Item::Stopped] if text == "You've spent 1,240.00 of"), "{items:?}");
        let view = assistant_thread(&runtime.db, &runtime.assistant, thread.id.clone(), None).unwrap();
        let kinds: Vec<&str> = view["entries"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["user", "assistant", "stopped"]);
        // Stopped before a word was written: only the mark.
        running("  ");
        assistant_stop(&runtime.db, &runtime.assistant, thread.id.clone()).unwrap();
        let items = runtime.assistant.thread_copy(&folder, &thread.id).unwrap().items;
        assert!(matches!(&items[3..], [Item::Stopped]), "{items:?}");
        // The next message tells the model its reply was cut short.
        let note = prompt::context(&urbudget_core::test_support::sample(), &prompt::Screen::default(), false, &[], None, true);
        assert!(note.contains("stopped your last reply"));
    }

    #[test]
    fn a_chosen_file_joins_the_conversation_only_when_sent_and_the_notice_is_per_service() {
        use base64::Engine;
        let directory = tempfile::tempdir().unwrap();
        let runtime = with_budget(directory.path());
        let folder = runtime.db.store().unwrap().folder().to_path_buf();
        let data = base64::engine::general_purpose::STANDARD.encode(b"Date,Payee,Amount\n2026-10-03,Bistro,-12.40\n");
        let meta = assistant_attach(&runtime.db, &runtime.assistant, "october.csv".into(), data.clone()).unwrap();
        assert!(!history::files(&folder).exists(), "nothing is among the conversation's files before Send");
        let gone = assistant_attach(&runtime.db, &runtime.assistant, "other.csv".into(), data).unwrap();
        assistant_detach(&runtime.db, gone.id.clone()).unwrap();
        let screen: prompt::Screen = serde_json::from_value(json!({ "page": "budget", "today": "2026-10-21" })).unwrap();
        let id = assistant_send(&runtime.simplefin, &runtime.db, &runtime.assistant, None, None, "Read this".into(), vec![meta.id.clone()], screen.clone()).unwrap();
        assert!(history::files(&folder).join(format!("{}.bin", meta.id)).is_file());
        let _ = assistant_stop(&runtime.db, &runtime.assistant, id);
        // A file taken off can't be sent.
        assert!(assistant_send(&runtime.simplefin, &runtime.db, &runtime.assistant, None, None, "And this".into(), vec![gone.id], screen.clone()).is_err());
        // Deleting every conversation takes their files, and leaves one chosen for a message not sent yet.
        let waiting = assistant_attach(&runtime.db, &runtime.assistant, "later.csv".into(), base64::engine::general_purpose::STANDARD.encode(b"a,b\n1,2\n")).unwrap();
        assistant_delete_all(&runtime.db, &runtime.assistant).unwrap();
        assert!(!history::files(&folder).exists() && assistant_threads(&runtime.db, &runtime.assistant).unwrap().is_empty());
        let id = assistant_send(&runtime.simplefin, &runtime.db, &runtime.assistant, None, None, "Now this".into(), vec![waiting.id], screen).unwrap();
        let _ = assistant_stop(&runtime.db, &runtime.assistant, id);

        // The notice of what's sent is due until it's read, and again for another service.
        assert!(serde_json::to_value(assistant_status(&runtime.assistant)).unwrap()["noticeDue"].as_bool().unwrap());
        let read: SettingsChange = serde_json::from_value(json!({ "told": true })).unwrap();
        assert_eq!(serde_json::to_value(assistant_settings(&runtime.assistant, read).unwrap()).unwrap()["noticeDue"], false);
        let switched: SettingsChange = serde_json::from_value(json!({ "provider": "anthropic" })).unwrap();
        assert_eq!(serde_json::to_value(assistant_settings(&runtime.assistant, switched).unwrap()).unwrap()["noticeDue"], true);
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
        let mut thread = Thread { id: "t_commit".into(), title: "Synthetic proposal".into(), created: now(), updated: now(), instructions: String::new(), items: vec![], proposals: vec![proposal.clone()], ..Default::default() };
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
