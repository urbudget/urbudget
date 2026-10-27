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
fn wait_for_callback(listener: TcpListener, state: &str, cancel: &AtomicBool, wait: Duration) -> Result<Callback, String> {
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
            let page = format!(
                "<!doctype html>{html}<meta charset=utf-8><title>{title}</title><body style=\"font:16px system-ui;max-width:32rem;margin:15vh auto;padding:0 1rem;color:#17191d\"><h1 style=\"font-size:22px\">{title}</h1><p>{text}</p></body>"
            );
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
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
        respond(&mut stream, "200 OK", tr("You're signed in"), tr("You can close this tab and return to UrBudget."));
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
    open: impl FnOnce(&str) -> Result<(), String>,
    remember_client: impl FnOnce(&str),
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
    let wait_state = state.clone();
    let callback = tokio::task::spawn_blocking(move || wait_for_callback(listener, &wait_state, &cancel, WAIT))
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
    let response = client
        .post(TOKEN)
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
        .map_err(|e| t!("Couldn't finish signing in: {e}", e))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(t!(
            "ChatGPT didn't accept the sign-in: {reason}",
            reason = body["error_description"].as_str().or(body["error"].as_str()).unwrap_or(status.as_str())
        ));
    }
    let tokens = read_tokens(&body, "")?;
    let account = claims(body["id_token"].as_str().ok_or_else(|| t!("The sign-in response had no ID token."))?, &issued, Some(&nonce))?;
    Ok(SignedIn { client_id: issued, account, tokens })
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
        let cb = wait_for_callback(listener, "s1", &cancel, WAIT).unwrap();
        assert_eq!(cb.code, "abc");
        assert_eq!(cb.client_id.as_deref(), Some("oaiapp_9"));
        assert!(browser.join().unwrap().contains("You're signed in"));
        assert!(host_id().starts_with("urn:uuid:") && host_id().len() == 45);
    }

    #[test]
    fn a_waiting_callback_is_taken_after_the_deadline() {
        // As on Android, where the browser's request queues while the app is frozen.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut browser = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        browser.write_all(b"GET /auth/callback?code=late&state=s2 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let cb = wait_for_callback(listener, "s2", &AtomicBool::new(false), Duration::ZERO).unwrap();
        assert_eq!(cb.code, "late");
        let empty = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let late = wait_for_callback(empty, "s3", &AtomicBool::new(false), Duration::ZERO);
        assert_eq!(late.err().as_deref(), Some("Sign-in timed out. Try again."));
    }
}
