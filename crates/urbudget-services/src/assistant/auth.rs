//! Sign in with ChatGPT: OAuth with PKCE in the system browser, a one-time listener on
//! 127.0.0.1 for the callback, and refresh tokens that rotate. UrBudget registers itself as
//! a client during the first sign-in, so it ships with no client secret or API key.
use crate::i18n::{language, tr, Language};
use crate::t;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const REVOKE: &str = "https://auth.openai.com/api/accounts/oauth/revoke";
/// The API the tokens are for, sent on every request to the token endpoint.
pub const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets the app use the plan; sign-in without it only identifies the user.
pub const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const REGISTER: &str = "dynamic_agent_client";
const APP_NAME: &str = "UrBudget";
/// The callback port. Only the port may change between sign-ins, so a busy port falls back to any.
const PORT: u16 = 1455;
const WAIT: Duration = Duration::from_secs(300);
const EXCHANGE_WAIT: Duration = Duration::from_secs(30);

/// This link only brings the initiating Android app forward. OAuth codes and tokens
/// stay on the loopback connection; they must never be put in an Android intent.
pub(super) fn android_return_link(package: &str) -> Result<String, String> {
    if !matches!(package, "app.urbudget" | "app.urbudget.nativeapp") {
        return Err("Invalid sign-in return application.".into());
    }
    Ok(format!("intent://assistant/sign-in#Intent;scheme={package};package={package};launchFlags=0x24000000;end"))
}

pub fn random(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("system random numbers");
    URL_SAFE_NO_PAD.encode(buf)
}
/// A per-install id, `urn:uuid:` with a version 4 UUID.
pub fn host_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("system random numbers");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("urn:uuid:{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub sub: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    /// Unix seconds.
    pub expires_at: i64,
    /// Don't refresh before this, in Unix seconds.
    pub earliest_refresh: i64,
    pub scope: String,
}
impl Tokens {
    pub fn plan_usage(&self) -> bool {
        self.scope.split_whitespace().any(|s| s == PLAN_SCOPE)
    }
    pub fn fresh(&self, now: i64) -> bool {
        self.expires_at - now > 60
    }
}

pub struct SignedIn {
    pub client_id: String,
    pub account: Account,
    pub tokens: Tokens,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn seconds(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp()))
}

/// Reqwest's top-level Display omits the DNS/TLS/socket cause. These are transport
/// errors from the fixed token endpoint, never response bodies or credentials.
fn transport_error(error: &reqwest::Error) -> String {
    let mut detail = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        let next = cause.to_string();
        if !detail.ends_with(&next) { detail.push_str(": "); detail.push_str(&next); }
        source = cause.source();
    }
    detail
}
fn read_tokens(v: &Value, previous_scope: &str) -> Result<Tokens, String> {
    let access = v["access_token"].as_str().ok_or_else(|| t!("The sign-in response had no access token."))?;
    let refresh = v["refresh_token"].as_str().ok_or_else(|| t!("The sign-in response had no refresh token."))?;
    Ok(Tokens {
        access: access.into(),
        refresh: refresh.into(),
        expires_at: now() + v["expires_in"].as_i64().unwrap_or(3600),
        earliest_refresh: seconds(&v["earliest_refresh_at"]).unwrap_or(0),
        scope: v["scope"].as_str().map_or_else(|| previous_scope.to_owned(), str::to_owned),
    })
}

/// The claims of an ID token. It comes straight from the token endpoint over TLS, which
/// OpenID Connect Core (3.1.3.7, step 6) accepts in place of checking the signature; the
/// claims are still checked.
fn claims(id_token: &str, client_id: &str, nonce: Option<&str>) -> Result<Account, String> {
    let payload = id_token.split('.').nth(1).ok_or_else(|| t!("The ID token is malformed."))?;
    let raw = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| t!("The ID token is malformed."))?;
    let c: Value = serde_json::from_slice(&raw).map_err(|_| t!("The ID token is malformed."))?;
    let audience_ok = match &c["aud"] {
        Value::String(a) => a == client_id,
        Value::Array(list) => list.iter().any(|a| a == client_id),
        _ => false,
    };
    let leeway = 5;
    if c["iss"] != ISSUER
        || !audience_ok
        || c["exp"].as_i64().is_none_or(|e| e + leeway < now())
        || c["iat"].as_i64().is_none_or(|i| i - leeway > now())
        || c["azp"].as_str().is_some_and(|a| a != client_id)
        || nonce.is_some_and(|n| c["nonce"] != n)
    {
        return Err(t!("The sign-in response didn't check out. Try signing in again."));
    }
    Ok(Account {
        sub: c["sub"].as_str().ok_or_else(|| t!("The ID token has no subject."))?.into(),
        email: c["email"].as_str().unwrap_or_default().into(),
        name: c["name"].as_str().unwrap_or_default().into(),
    })
}

struct Callback {
    code: String,
    client_id: Option<String>,
}

/// Waits for the browser to come back to `/auth/callback`, then answers it with a short page.
fn wait_for_callback(listener: TcpListener, state: &str, cancel: &AtomicBool, wait: Duration, return_link: Option<&str>) -> Result<Callback, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(t!("Sign-in was cancelled."));
        }
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            // A callback already waiting is taken even past the deadline: Android freezes the
            // app behind the browser, and the callback waits in the queue until it's back.
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() > wait {
                    return Err(t!("Sign-in timed out. Try again."));
                }
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let target = request.split_whitespace().nth(1).unwrap_or("/");
        let respond = |stream: &mut std::net::TcpStream, status: &str, title: &str, text: &str| {
            // The browser page speaks the app's language; Arabic reads right to left.
            let html = if language() == Language::Arabic { "<html lang=ar dir=rtl>" } else { "" };
            // Only a locally constructed, allowlisted link can reach this page.
            let back = return_link.map(|link| format!("<p><a href=\"{link}\">{}</a></p>", tr("Return to UrBudget"))).unwrap_or_default();
            let page = format!(
                "<!doctype html>{html}<meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>{title}</title><body style=\"font:16px system-ui;max-width:32rem;margin:15vh auto;padding:0 1rem;color:#17191d\"><h1 style=\"font-size:22px\">{title}</h1><p>{text}</p>{back}</body>"
            );
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
        };
        let Ok(url) = reqwest::Url::parse(&format!("http://127.0.0.1{target}")) else { continue };
        if url.path() != "/auth/callback" {
            respond(&mut stream, "404 Not Found", tr("Not found"), "");
            continue;
        }
        let query: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        if query.get("state").map(String::as_str) != Some(state) {
            respond(&mut stream, "400 Bad Request", tr("Sign-in didn't match"), tr("Go back to UrBudget and try again."));
            return Err(t!("The sign-in response didn't match this attempt. Try again."));
        }
        if let Some(error) = query.get("error") {
            respond(&mut stream, "200 OK", tr("Sign-in cancelled"), tr("You can close this tab and return to UrBudget."));
            return Err(if error == "access_denied" {
                t!("Sign-in was cancelled.")
            } else {
                t!("Sign-in failed: {reason}", reason = query.get("error_description").unwrap_or(error))
            });
        }
        let Some(code) = query.get("code") else {
            respond(&mut stream, "400 Bad Request", tr("Sign-in failed"), tr("Go back to UrBudget and try again."));
            return Err(t!("The sign-in response had no code. Try again."));
        };
        respond(&mut stream, "200 OK", tr("Sign-in received"), tr("Return to UrBudget to finish signing in."));
        return Ok(Callback { code: code.clone(), client_id: query.get("client_id").cloned() });
    }
}

/// Runs the whole sign-in. `saved_client` is the client id from an earlier sign-in, if any;
/// `remember_client` is called with the issued id before the code is exchanged, so a failed
/// exchange doesn't register a second client.
pub async fn sign_in(
    client: &reqwest::Client,
    host: &str,
    saved_client: Option<String>,
    login_hint: Option<String>,
    cancel: Arc<AtomicBool>,
    return_link: Option<String>,
    open: impl FnOnce(&str) -> Result<(), String>,
    remember_client: impl FnOnce(&str),
    progress: impl Fn(&'static str),
    foreground: impl Fn() -> bool,
) -> Result<SignedIn, String> {
    sign_in_at(client, host, saved_client, login_hint, cancel, return_link, open, remember_client, progress, foreground, TOKEN).await
}

async fn sign_in_at(
    client: &reqwest::Client,
    host: &str,
    saved_client: Option<String>,
    login_hint: Option<String>,
    cancel: Arc<AtomicBool>,
    return_link: Option<String>,
    open: impl FnOnce(&str) -> Result<(), String>,
    remember_client: impl FnOnce(&str),
    progress: impl Fn(&'static str),
    foreground: impl Fn() -> bool,
    token_endpoint: &str,
) -> Result<SignedIn, String> {
    let listener = TcpListener::bind(("127.0.0.1", PORT))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))
        .map_err(|e| t!("Couldn't start sign-in: {e}", e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/auth/callback");
    let verifier = random(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let (state, nonce) = (random(32), random(32));
    let first = saved_client.is_none();
    let client_id = saved_client.clone().unwrap_or_else(|| REGISTER.into());
    let mut params = vec![
        ("client_id", client_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect.as_str()),
        ("scope", SCOPES),
        ("resource", RESOURCE),
        ("state", state.as_str()),
        ("nonce", nonce.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("ext_agent_host_id", host),
    ];
    if first {
        params.push(("agent_name_hint", APP_NAME));
    }
    if let Some(hint) = login_hint.as_deref().filter(|h| !h.is_empty()) {
        params.push(("login_hint", hint));
    }
    let url = reqwest::Url::parse_with_params(AUTHORIZE, &params).map_err(|e| e.to_string())?;
    open(url.as_str())?;
    let mobile_return = return_link.is_some();
    let wait_state = state.clone();
    let wait_cancel = cancel.clone();
    let callback = tokio::task::spawn_blocking(move || wait_for_callback(listener, &wait_state, &wait_cancel, WAIT, return_link.as_deref()))
        .await
        .map_err(|e| e.to_string())??;
    let issued = match (&saved_client, callback.client_id) {
        (None, Some(id)) => id,
        (None, None) => return Err(t!("ChatGPT didn't finish registering UrBudget. Try signing in again.")),
        (Some(saved), Some(id)) if &id != saved => {
            return Err(t!("ChatGPT answered for a different app registration. Try signing in again."))
        }
        (Some(saved), _) => saved.clone(),
    };
    remember_client(&issued);
    // Android can suspend or deny networking to a cached app behind the browser.
    // Acknowledge the loopback request immediately, but exchange only after the
    // initiating activity has returned. The return intent carries no credentials.
    if mobile_return {
        progress("returning");
        finish_exchange(async {
            while !foreground() { tokio::time::sleep(Duration::from_millis(100)).await; }
            Ok(())
        }, &cancel, WAIT).await?;
    }
    progress("exchanging");
    let exchange = async {
    let response = client
        .post(token_endpoint)
        .timeout(EXCHANGE_WAIT)
        .header("accept", "application/json")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", issued.as_str()),
            ("code", callback.code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("resource", RESOURCE),
        ])
        .send()
        .await
        .map_err(|e| t!("Couldn't finish signing in: {e}", e = transport_error(&e)))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(t!(
            "ChatGPT didn't accept the sign-in: {reason}",
            reason = body["error_description"].as_str().or(body["error"].as_str()).unwrap_or(status.as_str())
        ));
    }
    let tokens = read_tokens(&body, "")?;
    progress("validating");
    let account = claims(body["id_token"].as_str().ok_or_else(|| t!("The sign-in response had no ID token."))?, &issued, Some(&nonce))?;
    Ok(SignedIn { client_id: issued, account, tokens })
    };
    finish_exchange(exchange, &cancel, EXCHANGE_WAIT).await
}

/// Cancellation and a total deadline also cover a stalled response body. Android may
/// suspend us behind the browser; returning to the app must resolve instead of spinning.
async fn finish_exchange<T>(exchange: impl std::future::Future<Output = Result<T, String>>, cancel: &AtomicBool, wait: Duration) -> Result<T, String> {
    let exchange = tokio::time::timeout(wait, exchange);
    tokio::pin!(exchange);
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(t!("Sign-in was cancelled.")); }
        match tokio::time::timeout(Duration::from_millis(100), &mut exchange).await {
            Ok(result) => return result.map_err(|_| t!("Sign-in timed out. Try again."))?,
            Err(_) => continue,
        }
    }
}

pub enum RefreshError {
    /// The refresh token is no good any more: sign in again.
    SignInAgain(String),
    /// Try again later.
    Retry(String),
}

/// Trades the refresh token for new tokens. The refresh token rotates: save the new one.
pub async fn refresh(client: &reqwest::Client, client_id: &str, refresh_token: &str, previous_scope: &str) -> Result<Tokens, RefreshError> {
    let response = client
        .post(TOKEN)
        .header("accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("resource", RESOURCE),
        ])
        .send()
        .await
        .map_err(|e| RefreshError::Retry(t!("Couldn't reach ChatGPT: {e}", e)))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let error = body["error"].as_str().unwrap_or_default();
        let gone = [
            "invalid_grant",
            "invalid_refresh_token",
            "token_expired",
            "refresh_token_expired",
            "refresh_token_invalidated",
            "refresh_token_reused",
            "invalid_client",
        ];
        return Err(if gone.contains(&error) || status.as_u16() == 401 {
            RefreshError::SignInAgain(t!("Your ChatGPT sign-in has ended. Sign in again."))
        } else {
            RefreshError::Retry(t!("ChatGPT couldn't refresh the sign-in ({status}).", status = status.as_u16()))
        });
    }
    read_tokens(&body, previous_scope).map_err(RefreshError::Retry)
}

/// Asks ChatGPT to forget the refresh token. Best effort; the caller clears it either way.
pub async fn revoke(client: &reqwest::Client, client_id: &str, refresh_token: &str) -> bool {
    client
        .post(REVOKE)
        .form(&[("token", refresh_token), ("token_type_hint", "refresh_token"), ("client_id", client_id)])
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}

/// Opens a web page using the current platform host.
pub fn open_browser(url: &str) -> Result<(), String> { crate::host::open_browser(url) }
pub fn open_sign_in(url: &str) -> Result<(), String> { crate::host::open_browser(url) }

#[cfg(test)]
mod tests {
    use super::*;

    fn token(claims: Value) -> String {
        format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()))
    }

    #[test]
    fn id_token_claims_are_checked() {
        let good = json_claims("oaiapp_1", "n1");
        assert_eq!(claims(&token(good.clone()), "oaiapp_1", Some("n1")).unwrap().email, "pat@example.com");
        assert!(claims(&token(good.clone()), "oaiapp_2", Some("n1")).is_err());
        assert!(claims(&token(good), "oaiapp_1", Some("other")).is_err());
        let mut expired = json_claims("oaiapp_1", "n1");
        expired["exp"] = serde_json::json!(now() - 3600);
        assert!(claims(&token(expired), "oaiapp_1", Some("n1")).is_err());
    }
    fn json_claims(aud: &str, nonce: &str) -> Value {
        serde_json::json!({
            "iss": ISSUER, "aud": aud, "sub": "user-1", "email": "pat@example.com",
            "exp": now() + 600, "iat": now(), "nonce": nonce
        })
    }

    #[test]
    fn plan_usage_needs_its_scope() {
        let t = read_tokens(&serde_json::json!({ "access_token": "a", "refresh_token": "r", "scope": "openid email" }), "").unwrap();
        assert!(!t.plan_usage());
        let kept = read_tokens(&serde_json::json!({ "access_token": "a", "refresh_token": "r" }), SCOPES).unwrap();
        assert!(kept.plan_usage());
    }

    #[test]
    fn the_callback_checks_state_and_returns_the_issued_client() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let cancel = AtomicBool::new(false);
        let browser = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /auth/callback?code=abc&state=s1&client_id=oaiapp_9 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut page = String::new();
            let _ = s.read_to_string(&mut page);
            page
        });
        let link = android_return_link("app.urbudget.nativeapp").unwrap();
        let cb = wait_for_callback(listener, "s1", &cancel, WAIT, Some(&link)).unwrap();
        assert_eq!(cb.code, "abc");
        assert_eq!(cb.client_id.as_deref(), Some("oaiapp_9"));
        let page = browser.join().unwrap();
        assert!(page.contains("Sign-in received"));
        assert!(page.contains(&format!("href=\"{link}\"")));
        assert!(!page.contains("You're signed in"));
        assert!(!page.contains("code=abc"));
        assert!(page.contains("Cache-Control: no-store"));
        assert!(host_id().starts_with("urn:uuid:") && host_id().len() == 45);
    }

    #[test]
    fn a_waiting_callback_is_taken_after_the_deadline() {
        // As on Android, where the browser's request queues while the app is frozen.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut browser = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        browser.write_all(b"GET /auth/callback?code=late&state=s2 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let cb = wait_for_callback(listener, "s2", &AtomicBool::new(false), Duration::ZERO, None).unwrap();
        assert_eq!(cb.code, "late");
        let empty = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let late = wait_for_callback(empty, "s3", &AtomicBool::new(false), Duration::ZERO, None);
        assert_eq!(late.err().as_deref(), Some("Sign-in timed out. Try again."));
    }

    #[test]
    fn android_return_links_are_package_bound_and_cannot_inject_html() {
        for package in ["app.urbudget", "app.urbudget.nativeapp"] {
            assert!(android_return_link(package).unwrap().contains(&format!(";package={package};")));
        }
        for package in ["evil.app", "app.urbudget;end", "app.urbudget\"><script>"] {
            assert!(android_return_link(package).is_err());
        }
    }

    #[test]
    fn exchange_timeout_and_cancel_both_end_a_stalled_sign_in() {
        crate::block_on(async {
            let cancelled = AtomicBool::new(true);
            let pending = std::future::pending::<Result<(), String>>();
            assert_eq!(finish_exchange(pending, &cancelled, WAIT).await.unwrap_err(), "Sign-in was cancelled.");
            let pending = std::future::pending::<Result<(), String>>();
            assert_eq!(finish_exchange(pending, &AtomicBool::new(false), Duration::from_millis(10)).await.unwrap_err(), "Sign-in timed out. Try again.");
            let flag = Arc::new(AtomicBool::new(false));
            let trigger = flag.clone();
            let exchange = async move {
                trigger.store(true, Ordering::Relaxed);
                std::future::pending::<Result<(), String>>().await
            };
            assert_eq!(finish_exchange(exchange, &flag, WAIT).await.unwrap_err(), "Sign-in was cancelled.");
        });
    }

    #[test]
    fn browser_callback_exchanges_code_with_pkce_and_validates_the_account() {
        use std::io::{BufRead, BufReader};
        let endpoint = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let token_url = format!("http://{}/token", endpoint.local_addr().unwrap());
        let in_foreground = Arc::new(AtomicBool::new(false));
        let resumed = in_foreground.clone();
        let stages = std::sync::Mutex::new(Vec::new());
        let mut browser = None;
        let mut remembered = String::new();
        let signed = crate::block_on(sign_in_at(
            &reqwest::Client::new(), "urn:uuid:test", None, None,
            Arc::new(AtomicBool::new(false)), Some(android_return_link("app.urbudget.nativeapp").unwrap()),
            |url| {
                let url = reqwest::Url::parse(url).unwrap();
                let query: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
                assert_eq!(query["client_id"], REGISTER);
                browser = Some(std::thread::spawn(move || {
                    let redirect = reqwest::Url::parse(&query["redirect_uri"]).unwrap();
                    let mut callback = std::net::TcpStream::connect(("127.0.0.1", redirect.port().unwrap())).unwrap();
                    callback.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    write!(callback, "GET /auth/callback?code=test-code&state={}&client_id=oaiapp_test HTTP/1.1\r\nHost: localhost\r\n\r\n", query["state"]).unwrap();
                    let mut page = String::new();
                    callback.read_to_string(&mut page).unwrap();
                    assert!(page.contains("Return to UrBudget"));
                    endpoint.set_nonblocking(true).unwrap();
                    std::thread::sleep(Duration::from_millis(200));
                    assert!(matches!(endpoint.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
                        "Token exchange must not start behind the browser");
                    endpoint.set_nonblocking(false).unwrap();
                    resumed.store(true, Ordering::Relaxed);
                    let (mut stream, _) = endpoint.accept().unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        assert!(reader.read_line(&mut line).unwrap() > 0);
                        if line == "\r\n" { break; }
                        if let Some(value) = line.to_lowercase().strip_prefix("content-length:") { length = value.trim().parse().unwrap(); }
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).unwrap();
                    let form = reqwest::Url::parse(&format!("http://test/?{}", String::from_utf8(body).unwrap())).unwrap();
                    let fields: std::collections::HashMap<String, String> = form.query_pairs().into_owned().collect();
                    assert_eq!(fields["redirect_uri"], query["redirect_uri"]);
                    assert_eq!(fields["code"], "test-code");
                    assert_eq!(fields["client_id"], "oaiapp_test");
                    assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(fields["code_verifier"].as_bytes())), query["code_challenge"]);
                    let response = serde_json::json!({"access_token":"synthetic-access", "refresh_token":"synthetic-refresh", "scope":SCOPES,
                        "id_token":token(json_claims("oaiapp_test", &query["nonce"]))}).to_string();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
                }));
                Ok(())
            },
            |id| remembered = id.to_owned(), |stage| stages.lock().unwrap().push(stage), || in_foreground.load(Ordering::Relaxed), &token_url,
        )).unwrap();
        browser.unwrap().join().unwrap();
        assert_eq!(remembered, "oaiapp_test");
        assert_eq!(signed.account.email, "pat@example.com");
        assert_eq!(signed.tokens.refresh, "synthetic-refresh");
        assert!(signed.tokens.plan_usage());
        assert_eq!(*stages.lock().unwrap(), vec!["returning", "exchanging", "validating"]);
    }
}
