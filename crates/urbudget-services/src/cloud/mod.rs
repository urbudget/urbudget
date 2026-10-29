//! UrBudget's online parts, all optional: an account (email sign-in codes, no password) and
//! end-to-end encrypted sync for subscribers. Budgeting never needs either of them.
pub mod crypto;
pub mod sync;
mod notifications;
pub mod v2;

pub(crate) use crate::vault;
use crate::t;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The website and API. Debug builds can point at `wrangler dev` with URBUDGET_API.
pub fn site() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("URBUDGET_API") {
        return url.trim_end_matches('/').to_owned();
    }
    "https://urbudget.app".to_owned()
}

const ACCOUNT_SERVICE: &str = "com.urbudget.account";

/// A credential-store entry name. Debug builds running on another data folder (QA, tests of sync
/// between two "computers") get their own entries, so they never touch the real ones.
pub fn service(name: &str) -> String {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("LOCAL_BUDGET_DATA_DIR") {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        dir.hash(&mut hasher);
        return format!("{name}.dev-{:08x}", hasher.finish() as u32);
    }
    name.to_owned()
}

/// An API answer other than success.
#[derive(Debug)]
pub struct Failure {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub body: Value,
}
impl From<Failure> for String {
    fn from(f: Failure) -> String {
        f.message
    }
}
fn offline(_: reqwest::Error) -> Failure {
    Failure { status: 0, code: "offline".into(), message: t!("Couldn't reach urbudget.app. Check your internet connection and try again."), body: Value::Null }
}
async fn failure(response: reqwest::Response) -> Failure {
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    Failure {
        status,
        code: body["error"].as_str().unwrap_or("error").to_owned(),
        // The server writes its messages in English; the ones people see have their Arabic too.
        message: body["message"]
            .as_str()
            .map(crate::i18n::translate)
            .unwrap_or_else(|| t!("urbudget.app answered with an error ({status}). Try again later.", status)),
        body,
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountFile {
    email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    session_expired: bool,
    /// Binds a persisted identity to its vault credential. Earlier email-only files remain valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential_hash: Option<String>,
}

fn credential_hash(token: &str) -> String { format!("{:x}", Sha256::digest(token.as_bytes())) }
fn persist_identity(path:&Path,account:&AccountFile)->Result<(),String>{
    use std::io::Write;
    let temporary=path.with_extension(format!("{}.tmp",crate::budgets::new_id()));
    let mut file=std::fs::OpenOptions::new().create_new(true).write(true).open(&temporary).map_err(|e|e.to_string())?;
    file.write_all(&serde_json::to_vec(account).map_err(|e|e.to_string())?).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
    drop(file);std::fs::rename(temporary,path).map_err(|e|e.to_string())
}
fn account_changed() -> Failure {
    Failure { status: 409, code: "account_changed".into(), message: t!("Your account changed in another window. Restart this window before continuing."), body: Value::Null }
}

pub struct Cloud {
    data_dir: PathBuf,
    pub sync: sync::Engine,
    client: reqwest::Client,
    account: Mutex<AccountFile>,
    /// Where the system has no credential store, a session lasts until the app closes.
    memory_token: Mutex<Option<String>>,
    account_epoch: AtomicU64,
    v2_keys: Mutex<std::collections::BTreeMap<(String,String),crypto::Key>>,
}

/// An immutable authorization snapshot. A running operation must never pick up another login.
#[derive(Clone)]
pub(crate) struct Session { token: String, epoch: u64 }

/// This computer's name, so the account and sync can say which device did what.
pub fn device_name() -> String {
    let name = std::env::var("COMPUTERNAME").ok().or_else(|| {
        if cfg!(target_os = "macos") {
            std::process::Command::new("scutil")
                .args(["--get", "ComputerName"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
        } else {
            std::env::var("HOSTNAME").ok()
        }
    });
    // Phones keep their name from apps (Android's hostname is "localhost"), so they go by their system.
    let fallback = if cfg!(target_os = "ios") {
        "iPhone or iPad"
    } else if cfg!(target_os = "android") {
        "Android device"
    } else {
        "This computer"
    };
    name.map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty() && n != "localhost")
        .unwrap_or_else(|| fallback.into())
}

impl Cloud {
    pub fn new(data_dir: &Path) -> Self {
        let account = std::fs::read(data_dir.join("account.json"))
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
        let client = reqwest::Client::builder()
            .user_agent(concat!("UrBudget/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self {
            data_dir: data_dir.to_path_buf(),
            sync: sync::Engine::load(data_dir),
            client,
            account: Mutex::new(account),
            memory_token: Mutex::new(None),
            account_epoch: AtomicU64::new(0),
            v2_keys: Mutex::default(),
        }
    }
    fn email(&self) -> Option<String> {
        self.account.lock().unwrap_or_else(|e| e.into_inner()).email.clone()
    }
    pub fn token(&self) -> Option<String> {
        let account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        self.checked_token(&account, || self.stored_token()).ok().flatten()
    }
    fn check_identity(&self, account: &AccountFile) -> Result<(), Failure> {
        let current = match std::fs::read(self.data_dir.join("account.json")) {
            Ok(raw) => serde_json::from_slice(&raw).map_err(|_| account_changed())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => AccountFile::default(),
            Err(_) => return Err(account_changed()),
        };
        if *account == current { Ok(()) } else { Err(account_changed()) }
    }
    pub(crate) fn check_account(&self) -> Result<(), Failure> {
        self.check_identity(&self.account.lock().unwrap_or_else(|e| e.into_inner()))
    }
    fn checked_token(&self, account: &AccountFile, read: impl FnOnce() -> Option<String>) -> Result<Option<String>, Failure> {
        self.check_identity(account)?;
        if account.email.is_none() || account.session_expired { return Ok(None); }
        let token = read();
        self.check_identity(account)?;
        if token.as_ref().zip(account.credential_hash.as_ref())
            .is_some_and(|(token, expected)| credential_hash(token) != *expected) {
            return Err(account_changed());
        }
        Ok(token)
    }
    fn stored_token(&self) -> Option<String> {
        if vault::STORE.is_some() {
            vault::read(&service(ACCOUNT_SERVICE))
        } else {
            self.memory_token.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }
    #[cfg(test)]
    fn remember(&self, email: Option<String>, token: Option<&str>) -> Result<(), String> {
        let mut account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        self.write_account(&mut account, email, token)
    }
    fn remember_identity(&self, email: String, account_id: String, token: &str) -> Result<(), String> {
        let mut account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        self.write_account_identity(&mut account, Some(email), Some(token),Some(account_id))
    }
    pub fn selected_account(&self) -> Option<String> {
        let account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        self.check_identity(&account).ok()?;
        account.account_id.clone()
    }
    /// Add the server identity to an older email-only session without replacing its credentials.
    /// The file lease and session check fence concurrent sign-in/sign-out in other processes.
    pub(crate) fn backfill_account_id(&self, session: &Session, id: &str) -> Result<(), String> {
        if id.is_empty() { return Err("The server returned an empty account identity.".into()); }
        let mut account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        let guard = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(self.data_dir.join("account.lock")).map_err(|e| e.to_string())?;
        vault::lock_exclusive(&guard).map_err(|e| e.to_string())?;
        self.check_session_identity(session, &account).map_err(String::from)?;
        if let Some(current) = account.account_id.as_deref() {
            return if current == id { Ok(()) } else { Err(account_changed().into()) };
        }
        let mut updated = account.clone();
        updated.account_id = Some(id.into());
        persist_identity(&self.data_dir.join("account.json"), &updated)?;
        *account = updated;
        Ok(())
    }
    fn write_account(&self, account: &mut AccountFile, email: Option<String>, token: Option<&str>) -> Result<(), String> {
        let account_id=if email.is_some()&&account.email==email{account.account_id.clone()}else{None};
        self.write_account_identity(account,email,token,account_id)
    }
    fn write_account_identity(&self, account:&mut AccountFile,email:Option<String>,token:Option<&str>,account_id:Option<String>)->Result<(),String> {
        let forget_sync = account.email != email || token.is_none();
        let previous_account=account.account_id.clone();
        self.replace_account_identity(account, email, token.map(credential_hash),account_id,false, || {
          // The identity lock also covers sync cleanup, so an older window cannot remove
          // another process's newly installed key while signing out or changing accounts.
          if forget_sync { v2::forget_keys(self,previous_account.as_deref())?;self.sync.forget()?; }
          match token {
            Some(token) if vault::STORE.is_some() => vault::save(&service(ACCOUNT_SERVICE), token)?,
            Some(token) => *self.memory_token.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.to_owned()),
            None => {
                vault::delete(&service(ACCOUNT_SERVICE))?;
                *self.memory_token.lock().unwrap_or_else(|e| e.into_inner()) = None;
            }
          }
          Ok(())
        })
    }
    #[cfg(test)]
    fn replace_account(&self, account: &mut AccountFile, email: Option<String>, credential_hash: Option<String>, update_credential: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let account_id=if email.is_some()&&account.email==email{account.account_id.clone()}else{None};
        self.replace_account_identity(account,email,credential_hash,account_id,false,update_credential)
    }
    fn replace_account_identity(&self,account:&mut AccountFile,email:Option<String>,credential_hash:Option<String>,account_id:Option<String>,session_expired:bool,update_credential:impl FnOnce()->Result<(),String>)->Result<(),String> {
        let guard = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
            .open(self.data_dir.join("account.lock")).map_err(|e| e.to_string())?;
        vault::lock_exclusive(&guard).map_err(|e| e.to_string())?;
        self.check_identity(account).map_err(String::from)?;
        // Invalidate requests and persist a signed-out identity before touching credentials.
        // A failed token or final identity write then cannot pair the old email with a new
        // account's token, including after an application restart.
        *account = AccountFile::default();
        self.account_epoch.fetch_add(1, Ordering::SeqCst);
        let path = self.data_dir.join("account.json");
        persist_identity(&path,account)?;
        update_credential()?;
        let file = AccountFile { email, account_id, session_expired, credential_hash };
        persist_identity(&path,&file)?;
        *account = file;
        Ok(())
    }
    /// Forgets the account on this computer, and with it the sync key and sync state.
    pub fn forget_account(&self) -> Result<(), String> {
        let mut account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        self.write_account(&mut account, None, None)
    }
    fn forget_session(&self, session: &Session) {
        self.forget_session_with(session, || {
            vault::delete(&service(ACCOUNT_SERVICE))?;
            *self.memory_token.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        });
    }
    fn forget_session_with(&self, session: &Session, remove_credential: impl FnOnce() -> Result<(), String>) {
        let mut account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        if self.check_session_identity(session, &account).is_ok() {
            // A 401 can arrive outside the shared operation lock (for example, account
            // overview). Invalidate authorization now, but leave key/tracking cleanup to
            // explicit sign-out or the next login, which already holds that lock.
            // Expired authorization keeps the selected account and its budget ownership.
            // Only explicit sign-out makes future budgets local-only.
            let email = account.email.clone();
            let account_id=account.account_id.clone();
            let _ = self.replace_account_identity(&mut account, email, None, account_id,true,remove_credential);
        }
    }
    pub(crate) fn session(&self) -> Result<Session, Failure> {
        let account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        self.session_with(&account, || self.stored_token())
    }
    fn session_with(&self, account: &AccountFile, read: impl FnOnce() -> Option<String>) -> Result<Session, Failure> {
        let token = self.checked_token(account, read)?.ok_or_else(|| Failure {
            status: 401, code: "signed_out".into(), message: t!("Sign in to your UrBudget account first."), body: Value::Null,
        })?;
        Ok(Session { token, epoch: self.account_epoch.load(Ordering::SeqCst) })
    }
    pub(crate) fn check_session(&self, session: &Session) -> Result<(), Failure> {
        let account = self.account.lock().unwrap_or_else(|e| e.into_inner());
        self.check_session_identity(session, &account)
    }
    fn check_session_identity(&self, session: &Session, account: &AccountFile) -> Result<(), Failure> {
        if self.account_epoch.load(Ordering::SeqCst) != session.epoch {
            return Err(account_changed());
        }
        self.check_identity(account)?;
        if account.credential_hash.as_ref().is_some_and(|expected| credential_hash(&session.token) != *expected) {
            return Err(account_changed());
        }
        Ok(())
    }
    pub(crate) fn for_session(&self, session: &Session, request: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder, Failure> {
        self.check_session(session)?;
        Ok(request.bearer_auth(&session.token))
    }

    fn url(path: &str) -> String {
        format!("{}/api/v1{path}", site())
    }
    /// Sends a JSON request and returns the JSON answer. `auth` adds the account's token; a 401
    /// then means the session was revoked, so this computer signs out.
    pub async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>, auth: bool) -> Result<Value, Failure> {
        let session = if auth { Some(self.session()?) } else { None };
        self.call_session(method, path, body, session.as_ref(), None).await
    }
    pub(crate) async fn call_session(&self, method: reqwest::Method, path: &str, body: Option<Value>, session: Option<&Session>, generation: Option<u64>) -> Result<Value, Failure> {
        let mut request = self.client.request(method, Self::url(path));
        if let Some(session) = session { request = self.for_session(session, request)?; }
        if let Some(generation) = generation { request = request.header("X-Sync-Generation", generation.to_string()); }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(offline)?;
        if let Some(session) = session { self.check_session(session)?; }
        if response.status().is_success() {
            let body = response.json().await.unwrap_or(Value::Null);
            if let Some(session) = session { self.check_session(session)?; }
            return Ok(body);
        }
        let failure = failure(response).await;
        if let Some(session) = session { self.check_session(session)?; }
        if failure.status == 401 {
            if let Some(session) = session { self.forget_session(session); }
        }
        Err(failure)
    }
    pub(crate) fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client.request(method, Self::url(path))
    }

    pub fn status(&self) -> Status {
        let catalog = crate::budgets::Budgets::load(&self.data_dir).ok();
        self.status_with_catalog(catalog.as_ref())
    }
    pub fn status_with_library(&self, library: &crate::Library) -> Status {
        self.status_with_catalog(Some(&library.lock()))
    }
    fn status_with_catalog(&self, catalog: Option<&crate::budgets::Budgets>) -> Status {
        let email = self.email().filter(|_| self.token().is_some());
        let mut sync = self.sync.status();
        if let Some(unlocked) = catalog.and_then(|c| v2::unlocked_catalog(self, c)) { sync.unlocked = unlocked; }
        sync.unlocked &= email.is_some();
        if let Some(catalog) = catalog { v2::decorate_status(self, catalog, &mut sync); }
        Status {
            email,
            credential_store: vault::STORE,
            sync,
            site: site(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The signed-in account, if any.
    pub email: Option<String>,
    pub credential_store: Option<&'static str>,
    pub sync: sync::Status,
    pub site: String,
}

pub fn open_browser(url: &str) -> Result<(), String> {
    crate::host::open_browser(url)
}

pub fn cloud_status(cloud: &Cloud) -> Status {
    cloud.status()
}

/// The account as the server sees it: the sync plan and how much is synced.
pub async fn account_overview(cloud: &Cloud) -> Result<Value, String> {
    Ok(cloud.call(reqwest::Method::GET, "/me", None, true).await?)
}

/// Emails a sign-in code. In local development the answer includes it as `devCode`.
pub async fn account_start(cloud: &Cloud, email: String) -> Result<Value, String> {
    Ok(cloud
        .call(reqwest::Method::POST, "/auth/start", Some(json!({ "email": email.trim() })), false)
        .await?)
}

pub async fn account_verify(cloud: &Cloud, email: String, code: String) -> Result<Status, String> {
    let _one = cloud.sync.operation().await?;
    let answer = cloud
        .call(
            reqwest::Method::POST,
            "/auth/verify",
            Some(json!({ "email": email.trim(), "code": code.trim(), "client": "desktop", "deviceName": device_name() })),
            false,
        )
        .await?;
    let token = answer["token"].as_str().ok_or_else(|| t!("The sign-in answer had no session."))?;
    let email = answer["user"]["email"].as_str().unwrap_or(email.trim()).to_owned();
    let account_id = answer["user"]["id"].as_str().ok_or("The sign-in answer had no account identity.")?.to_owned();
    cloud.remember_identity(email, account_id, token)?;
    Ok(cloud.status())
}

pub async fn account_sign_out(cloud: &Cloud) -> Result<Status, String> {
    let _one = cloud.sync.operation().await?;
    if cloud.token().is_some() {
        // Best effort: the session ends on the server too when it can be reached.
        let _ = cloud.call(reqwest::Method::POST, "/auth/logout", None, true).await;
    }
    cloud.forget_account()?;
    Ok(cloud.status())
}

/// Opens a page of urbudget.app in the browser, signed in as this account when there is one.
pub async fn account_open(cloud: &Cloud, next: String) -> Result<(), String> {
    #[cfg(target_os = "android")]
    if !matches!(next.as_str(), "/privacy/app/" | "/privacy/app/#your-choices") {
        return Err(t!("Use Google Play for your Sync subscription, or Account settings to delete your account."));
    }
    let next = if next.starts_with('/') && !next.starts_with("//") { next } else { "/".into() };
    if cloud.token().is_some() {
        if let Ok(answer) = cloud
            .call(reqwest::Method::POST, "/auth/web-link", Some(json!({ "next": next })), true)
            .await
        {
            if let Some(url) = answer["url"].as_str() {
                return open_browser(url);
            }
        }
    }
    open_browser(&format!("{}{next}", site()))
}

/// Delete the signed-in account without sending Android users through a payment website.
pub async fn account_delete(cloud: &Cloud, confirm: String) -> Result<Status, String> {
    let _one = cloud.sync.operation().await?;
    cloud.call(reqwest::Method::DELETE, "/me", Some(json!({ "confirm": confirm })), true).await?;
    cloud.forget_account()?;
    Ok(cloud.status())
}

#[cfg(test)]
mod session_tests {
    use super::*;
    #[test]
    fn old_sessions_backfill_identity_without_changing_credentials_and_reject_stale_answers() {
        let dir=tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("account.json"),br#"{"email":"old@example.test"}"#).unwrap();
        let cloud=Cloud::new(dir.path()); let session=fake_session(&cloud,"old-token").unwrap();
        cloud.backfill_account_id(&session,"account-a").unwrap();
        assert_eq!(cloud.selected_account().as_deref(),Some("account-a"));
        assert!(cloud.check_session(&session).is_ok());
        assert!(cloud.backfill_account_id(&session,"account-b").is_err());
        replace_without_vault(&cloud,"other@example.test","new-token");
        assert!(cloud.backfill_account_id(&session,"account-a").is_err());
    }

    #[test]
    fn credential_and_identity_write_failures_leave_no_mixed_account() {
        for stage in ["signed-out-marker", "credential", "final-identity"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("account.json");
            std::fs::write(&path, br#"{"email":"old@example.test"}"#).unwrap();
            let cloud = Cloud::new(dir.path());
            let old = Session { token: "old-token".into(), epoch: 0 };
            if stage == "signed-out-marker" {
                std::fs::remove_file(&path).unwrap(); std::fs::create_dir(&path).unwrap();
            }
            let called = std::cell::Cell::new(false);
            let result = cloud.replace_account(&mut cloud.account.lock().unwrap(), Some("new@example.test".into()), Some(credential_hash("new-token")), || {
                called.set(true);
                if stage == "credential" { return Err("storage unavailable".into()); }
                // Simulate an identity write failure after the new token was stored.
                std::fs::remove_file(&path).unwrap(); std::fs::create_dir(&path).unwrap();
                Ok(())
            });
            assert!(result.is_err());
            assert_eq!(called.get(), stage != "signed-out-marker");
            assert_eq!(cloud.email(), if stage == "signed-out-marker" { Some("old@example.test".into()) } else { None });
            assert_eq!(cloud.check_session(&old).unwrap_err().code, "account_changed");
            assert_eq!(Cloud::new(dir.path()).email(), None);
        }
    }
    #[test]
    fn requests_bind_to_the_captured_login_and_stale_401_cannot_sign_out_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let cloud = Cloud::new(dir.path());
        let session = Session { token: "synthetic-old-session".into(), epoch: 0 };
        let request = cloud.for_session(&session, cloud.request(reqwest::Method::GET, "/sync/budgets")).unwrap().build().unwrap();
        assert_eq!(request.headers()["Authorization"], "Bearer synthetic-old-session");
        // Account changes invalidate every outstanding authorization snapshot, even if a
        // late request succeeds or returns 401. No credential store is read or written here.
        cloud.account_epoch.store(1, Ordering::SeqCst);
        assert_eq!(cloud.for_session(&session, cloud.request(reqwest::Method::PUT, "/sync/budgets/test")).unwrap_err().code, "account_changed");
        cloud.forget_session(&session);
        assert_eq!(cloud.account_epoch.load(Ordering::SeqCst), 1);
        assert!(!dir.path().join("account.json").exists());
    }
    fn fake_session(cloud: &Cloud, token: &str) -> Result<Session, Failure> {
        cloud.session_with(&cloud.account.lock().unwrap(), || Some(token.into()))
    }
    fn replace_without_vault(cloud: &Cloud, email: &str, token: &str) {
        cloud.replace_account(&mut cloud.account.lock().unwrap(), Some(email.into()), Some(credential_hash(token)), || Ok(())).unwrap();
    }
    #[test]
    fn a_purchase_restore_cannot_adopt_receipts_or_continue_after_the_account_changes() {
        let dir = tempfile::tempdir().unwrap();
        let cloud = Cloud::new(dir.path());
        replace_without_vault(&cloud, "first@example.test", "first-token");
        let first = fake_session(&cloud, "first-token").unwrap();
        let calls = std::cell::Cell::new(0);
        let purchases = json!({"purchases":[
            {"state":"purchased","token":"receipt-one","products":["urbudget_sync"]},
            {"state":"purchased","token":"receipt-two","products":["urbudget_sync"]}
        ]});
        let result = crate::block_on(crate::billing::verify_for_session(&cloud, &first, purchases, |_| {
            calls.set(calls.get() + 1);
            replace_without_vault(&cloud, "second@example.test", "second-token");
            // Even a successful response must be ignored once it belongs to the login which
            // was replaced during the request.
            std::future::ready(Ok(json!({"active":true})))
        }));
        assert!(result.unwrap_err().contains("account changed"));
        assert_eq!(calls.get(), 1, "a second receipt must not be submitted with the new login");
        assert_eq!(cloud.email().as_deref(), Some("second@example.test"));
        assert!(fake_session(&cloud, "second-token").is_ok());
    }
    #[test]
    fn another_process_login_invalidates_cached_sessions_and_stale_sign_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("account.json");
        std::fs::write(&path, br#"{"email":"first@example.test"}"#).unwrap();
        let first = Cloud::new(dir.path());
        let second = Cloud::new(dir.path());
        let old = fake_session(&first, "first-token").unwrap();
        replace_without_vault(&second, "second@example.test", "second-token");
        let after = std::fs::read(&path).unwrap();
        assert_eq!(fake_session(&first, "second-token").err().unwrap().code, "account_changed");
        assert_eq!(first.check_session(&old).unwrap_err().code, "account_changed");
        assert!(first.token().is_none());
        first.forget_session(&old);
        assert!(first.forget_account().unwrap_err().contains("Restart this window"));
        assert!(first.remember(Some("third@example.test".into()), Some("third-token")).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), after, "stale requests and explicit sign-out must preserve the other process's login");
        assert_eq!(first.account_epoch.load(Ordering::SeqCst), 0);
        assert!(fake_session(&second, "second-token").is_ok());
    }
    #[test]
    fn same_email_new_token_and_credential_mismatch_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        let writer = Cloud::new(dir.path());
        replace_without_vault(&writer, "same@example.test", "first-token");
        let first = Cloud::new(dir.path());
        let old = fake_session(&first, "first-token").unwrap();
        replace_without_vault(&writer, "same@example.test", "replacement-token");
        assert_eq!(first.check_session(&old).unwrap_err().code, "account_changed");
        let current = Cloud::new(dir.path());
        assert_eq!(fake_session(&current, "first-token").err().unwrap().code, "account_changed", "a mismatched vault token must not be paired with a valid disk identity");
        assert!(fake_session(&current, "replacement-token").is_ok());
    }
    #[test]
    fn legacy_identity_remains_usable_but_change_during_credential_read_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("account.json");
        std::fs::write(&path, br#"{"email":"legacy@example.test"}"#).unwrap();
        let legacy = Cloud::new(dir.path());
        assert!(fake_session(&legacy, "legacy-token").is_ok());
        let writer = Cloud::new(dir.path());
        let error = legacy.session_with(&legacy.account.lock().unwrap(), || {
            replace_without_vault(&writer, "legacy@example.test", "new-token");
            Some("new-token".into())
        }).err().unwrap();
        assert_eq!(error.code, "account_changed");
        // A malformed or removed identity fails closed even if the vault could still be read.
        std::fs::write(&path, b"{").unwrap();
        assert_eq!(fake_session(&legacy, "legacy-token").err().unwrap().code, "account_changed");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(fake_session(&legacy, "legacy-token").err().unwrap().code, "account_changed");
    }
    #[test]
    fn automatic_401_defers_sync_cleanup_until_a_serialized_operation() {
        let dir = tempfile::tempdir().unwrap();
        let account_path = dir.path().join("account.json");
        let sync_path = dir.path().join("sync-state.json");
        std::fs::write(&account_path, br#"{"email":"first@example.test"}"#).unwrap();
        let initial = br#"{"budgets":{"pending":{"revision":0,"localRevision":-1}},"generation":7}"#;
        std::fs::write(&sync_path, initial).unwrap();
        let cloud = Cloud::new(dir.path());
        let session = fake_session(&cloud, "first-token").unwrap();
        // Another instance can be in the middle of installing a key or saving tracking.
        let other = sync::Engine::load(dir.path());
        let active = crate::block_on(other.operation()).unwrap();
        let removed = std::cell::Cell::new(false);
        cloud.forget_session_with(&session, || { removed.set(true); Ok(()) });
        assert!(removed.get());
        assert_eq!(std::fs::read(&sync_path).unwrap(), initial, "automatic401 must not write tracking while another operation owns the file lock");
        assert_eq!(cloud.email().as_deref(),Some("first@example.test"),"expired credentials preserve the selected account");
        assert_eq!(fake_session(&cloud, "first-token").err().unwrap().code, "signed_out");
        assert!(!cloud.status().sync.unlocked);
        drop(active);
        // Explicit cleanup is allowed only after acquiring the shared operation lock.
        let _operation = crate::block_on(cloud.sync.operation()).unwrap();
        let key_cleared = std::cell::Cell::new(false);
        cloud.sync.forget_with(|| { key_cleared.set(true); Ok(()) }).unwrap();
        assert!(key_cleared.get());
        let cleaned: Value = serde_json::from_slice(&std::fs::read(&sync_path).unwrap()).unwrap();
        assert_eq!(cleaned["generation"], 0);
        assert!(cleaned["budgets"].as_object().unwrap().is_empty());
    }
}
