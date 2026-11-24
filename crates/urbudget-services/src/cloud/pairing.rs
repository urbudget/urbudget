//! Device approval: a device that syncs hands the sync key to one that has just signed in, so
//! nobody types anything long on the new one.
//!
//! A one-time device code travels between the two screens, by camera or keyboard, and never
//! reaches the server. The device that has the key seals it with a key made from the code and
//! leaves it on the server for five minutes; the new device collects it (the server hands it
//! out once) and opens it. So the server holds nothing that opens a budget.
//!
//! Either device can be the one that shows the code, so that the one with a camera scans:
//! - an *offer* is shown by the device that has the key, which seals it at once. A new phone
//!   scans it, or a new computer has it typed in.
//! - a *request* is shown by the new device, and waits. A phone that syncs scans it, the person
//!   approves, and the phone seals the key for it.
//!
//! See docs/plans/2026-11-24-sync-device-approval.md.
use super::codes::{self, Code, Entry};
use super::{crypto, v2, Cloud};
use crate::{t, Library};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64, Engine as _};
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// How long a code is good for. The server keeps a sealed key this long too.
const MINUTES: i64 = 5;

/// The code is random, so a hash under its own label is all the derivation needed.
fn derived(label: &str, code: &Code) -> [u8; 32] {
    Sha256::new().chain_update(label).chain_update(code).finalize().into()
}
/// Where the sealed key waits. Made from the code, so only a device given the code can ask.
fn message_id(code: &Code) -> String {
    derived("urbudget:pairing:id:v1", code)[..16].iter().map(|byte| format!("{byte:02x}")).collect()
}
fn wrapping_key(code: &Code) -> crypto::Key {
    derived("urbudget:pairing:key:v1", code)
}
/// Binds the sealed key to the account, key generation and message it was made for.
fn context(account: &str, id: &str, generation: &str) -> Vec<u8> {
    v2::canonical(&json!({"purpose":"pairing","accountId":account,"id":id,"keyGeneration":generation})).into_bytes()
}

pub(super) fn offline() -> String {
    t!("Couldn't reach urbudget.app. Check your internet connection and try again.")
}
fn used() -> String {
    t!("That code has expired or was already used. Make a new one on the other device.")
}
pub(super) fn belongs_on_a_syncing_device() -> String {
    t!("That code is shown by a device waiting to be added. Scan it from Add a device, on a device that already syncs.")
}

/// What the server said, in the app's language, without the code it leads with.
pub(super) fn told(error: String) -> String {
    if error.starts_with("OFFLINE:") {
        return offline();
    }
    match error.split_once(": ") {
        Some((code, message)) if !code.is_empty() && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') => {
            crate::i18n::translate(message)
        }
        _ => error,
    }
}

async fn account(cloud: &Cloud, library: &Library) -> Result<(String, Value, String), String> {
    let (account, bootstrap) = v2::probe_bootstrap(cloud, library).await.map_err(told)?.ok_or_else(offline)?;
    let generation = bootstrap["keyGeneration"]
        .as_str()
        .ok_or_else(|| t!("Sync isn't set up for this account yet. Turn it on first."))?
        .to_owned();
    Ok((account, bootstrap, generation))
}

/// Seals this device's key for whoever holds `code` and leaves it on the server. Returns when
/// the server stops keeping it.
async fn leave(cloud: &Cloud, library: &Library, code: &Code) -> Result<Value, String> {
    let _operation = cloud.sync.operation().await?;
    let (account, bootstrap, generation) = account(cloud, library).await?;
    let key = v2::held_key(cloud, &account, &bootstrap)
        .ok_or_else(|| t!("Unlock sync on this device before adding another."))?;
    let id = message_id(code);
    let sealed = crypto::seal(&wrapping_key(code), &context(&account, &id, &generation), &key)?;
    let stored = v2::Transport::new(cloud, String::new())?
        .json(Method::PUT, &format!("/pairings/{id}"), Some(json!({"wrapped":BASE64.encode(sealed),"keyGeneration":generation})))
        .await
        .map_err(told)?;
    Ok(stored["expiresAt"].clone())
}

/// On a device that syncs: makes a code and leaves the sealed key for the new device. Returns
/// `{id, code, qr: {size, modules}, expiresAt}` for the screen.
pub async fn offer(cloud: &Cloud, library: &Library) -> Result<Value, String> {
    let code = codes::new(codes::Kind::Device)?;
    let expires = leave(cloud, library, &code).await?;
    Ok(json!({"id":message_id(&code),"code":codes::encode(&code),"qr":codes::qr(codes::OFFER, &code)?,"expiresAt":expires}))
}

/// On a device that syncs, given the code a new device is showing: seals the key for it. The
/// screen has asked the person first; anyone given the key can read the account's budgets.
pub async fn approve(cloud: &Cloud, library: &Library, text: &str) -> Result<(), String> {
    match codes::read(text)? {
        // Typed, a device code doesn't say who shows it: this screen is told it's a request.
        Entry::Request(code) | Entry::Device(code) if !text.trim().starts_with(codes::OFFER) => {
            leave(cloud, library, &code).await.map(|_| ())
        }
        Entry::Device(_) => Err(t!("That code is for the device being added. Scan it there, where it asks to unlock sync.")),
        Entry::Mistyped => Err(codes::typo()),
        _ => Err(t!("That isn't a code from a device waiting to be added.")),
    }
}

/// On the new device, signed in: makes a code to show, for a device that syncs to scan and
/// approve. Nothing is sent: the code means nothing to the server until a key is sealed for
/// it. Returns `{id, code, qr, expiresAt}`; the screen then asks `wait` until it answers.
pub fn request(cloud: &Cloud) -> Result<Value, String> {
    if cloud.token().is_none() {
        return Err(t!("Sign in to your UrBudget account first."));
    }
    let code = codes::new(codes::Kind::Device)?;
    let expires = (chrono::Utc::now() + chrono::Duration::minutes(MINUTES)).to_rfc3339();
    Ok(json!({"id":message_id(&code),"code":codes::encode(&code),"qr":codes::qr(codes::REQUEST, &code)?,"expiresAt":expires}))
}

fn checked_id(id: &str) -> Result<&str, String> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(used());
    }
    Ok(id)
}

/// On the device showing an offer: `{waiting, collected}`. `collected` once the new device has
/// taken the key, which the screen closes itself on; neither once the code ran out or was
/// taken back.
pub async fn state(cloud: &Cloud, id: &str) -> Result<Value, String> {
    let state = v2::Transport::new(cloud, String::new())?
        .json(Method::GET, &format!("/pairings/{}/state", checked_id(id)?), None)
        .await
        .map_err(told)?;
    Ok(json!({ "waiting": state["waiting"] == true, "collected": state["collected"] == true }))
}

/// Takes back a code nobody used, when its screen closes.
pub async fn cancel(cloud: &Cloud, id: &str) -> Result<(), String> {
    v2::Transport::new(cloud, String::new())?
        .json(Method::DELETE, &format!("/pairings/{}", checked_id(id)?), None)
        .await
        .map_err(told)?;
    Ok(())
}

/// Collects the key sealed for `code` and opens it. `Ok(None)` when nothing is waiting for the
/// code. The key is checked against the account before it is handed back.
async fn collect(cloud: &Cloud, library: &Library, code: &Code) -> Result<Option<crypto::Key>, String> {
    let id = message_id(code);
    let message = match v2::Transport::new(cloud, String::new())?.json(Method::GET, &format!("/pairings/{id}"), None).await {
        Ok(message) => message,
        Err(error) if error.starts_with("pairing_not_found:") => return Ok(None),
        Err(error) => return Err(told(error)),
    };
    let _operation = cloud.sync.operation().await?;
    let (account, bootstrap, generation) = account(cloud, library).await?;
    if message["keyGeneration"] != generation.as_str() {
        return Err(v2::key_changed());
    }
    let key: crypto::Key = message["wrapped"]
        .as_str()
        .and_then(|sealed| BASE64.decode(sealed).ok())
        .and_then(|sealed| crypto::open(&wrapping_key(code), &context(&account, &id, &generation), &sealed).ok())
        .and_then(|key| key.try_into().ok())
        .ok_or_else(|| t!("That code doesn't open what the other device sent. Make a new one and try again."))?;
    // Never keep a key that doesn't open this account, whatever sent it.
    v2::verified_key(Some(key), &bootstrap).map(Some).ok_or_else(v2::key_changed)
}

/// On the new device, given a device code a device that syncs is showing: the account's key.
pub(super) async fn accept(cloud: &Cloud, library: &Library, code: &Code) -> Result<crypto::Key, String> {
    collect(cloud, library, code).await?.ok_or_else(used)
}

/// On the new device, while it shows the code it made with `request`: `{ready: false}` until a
/// device that syncs has approved it, then `{ready: true}` with sync unlocked here.
pub async fn wait(cloud: &Cloud, library: &Library, text: &str) -> Result<Value, String> {
    let (Entry::Device(code) | Entry::Request(code)) = codes::read(text)? else {
        return Err(used());
    };
    Ok(match collect(cloud, library, &code).await? {
        Some(key) => {
            let mut unlocked = super::sync::install(cloud, library, key).await?;
            unlocked["ready"] = json!(true);
            unlocked
        }
        None => json!({ "ready": false }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sealed_key_opens_only_with_its_code_for_its_account_and_generation() {
        let code: Code = [7; codes::LEN];
        let other: Code = [8; codes::LEN];
        let id = message_id(&code);
        // Fixed, so every later version derives what an older one on the other device did.
        assert_eq!(id, "82a4d067d9aa39431ee15f1626c663e9");
        let hex: String = wrapping_key(&code).iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(hex, "384a4f8f0de4c60546854e38cee7679c1f22ed48111ac3fd3e9ccf18b5dccc78");
        assert_ne!(id, message_id(&other));
        assert_ne!(wrapping_key(&code).to_vec(), derived("urbudget:pairing:id:v1", &code).to_vec(), "the server learns the id, never the key");
        let key: crypto::Key = [42; 32];
        let sealed = crypto::seal(&wrapping_key(&code), &context("account", &id, "3"), &key).unwrap();
        let open = |code: &Code, account: &str, id: &str, generation: &str| {
            crypto::open(&wrapping_key(code), &context(account, id, generation), &sealed)
        };
        assert_eq!(open(&code, "account", &id, "3").unwrap(), key);
        assert!(open(&other, "account", &id, "3").is_err());
        assert!(open(&code, "another-account", &id, "3").is_err());
        assert!(open(&code, "account", &id, "4").is_err());
        assert!(open(&code, "account", &message_id(&other), "3").is_err());
    }

    #[test]
    fn server_messages_lose_their_code_and_follow_the_language() {
        assert_eq!(told("pairing_not_found: That code has expired or was already used. Make a new one on the other device.".into()),
            "That code has expired or was already used. Make a new one on the other device.");
        assert_eq!(told("OFFLINE: Sync will retry when a connection is available.".into()),
            "Couldn't reach urbudget.app. Check your internet connection and try again.");
        assert_eq!(told("Something without a code".into()), "Something without a code");
    }
}
