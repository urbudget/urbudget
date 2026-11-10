//! Device approval: a device that syncs hands the sync key to one that has just signed in, so
//! nobody types the passphrase on the new one.
//!
//! A one-time code travels between the two screens, by camera or keyboard, and never reaches
//! the server. The device that has the key seals it with a key made from the code and leaves
//! it on the server for five minutes; the new device collects it (the server hands it out
//! once) and opens it. So the server holds nothing that opens a budget.
//!
//! Either device can be the one that shows the code, so that the one with a camera scans:
//! - an *offer* is shown by the device that has the key, which seals it at once. A new phone
//!   scans it, or a new computer has it typed in.
//! - a *request* is shown by the new device, and waits. A phone that syncs scans it, the person
//!   approves, and the phone seals the key for it.
//!
//! See docs/plans/2026-11-10-sync-device-approval.md.
use super::{crypto, v2, Cloud};
use crate::{t, Library};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64, Engine as _};
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 120 bits: 24 characters of five bits each, and far too many to guess from the sealed key.
const CODE_LEN: usize = 15;
/// A pairing code, or a recovery code (`recovery`): the same size, and read and typed alike.
pub(super) type Code = [u8; CODE_LEN];
/// Crockford's base32: no I, L, O or U, so a code read aloud or typed has no look-alikes.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// How long a code is good for. The server keeps a sealed key this long too.
const MINUTES: i64 = 5;

/// Which device shows the code. A QR code says which, so a scan on the wrong screen is told
/// what to do instead of failing as a wrong code. A typed code doesn't: its screen knows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// Shown by the device that has the key, which has already sealed it.
    Offer,
    /// Shown by the new device, which waits for a device that has the key to seal it.
    Request,
}
impl Kind {
    /// What the QR code holds before the characters. Plain text and not a link, so neither
    /// the system camera nor another app opens it.
    fn prefix(self) -> &'static str {
        match self {
            Kind::Offer => "URBUDGET-PAIR:1:",
            Kind::Request => "URBUDGET-ASK:1:",
        }
    }
}

/// The code as people read it: six groups of four.
pub fn encode(code: &Code) -> String {
    let bits = code.iter().fold(0u128, |all, byte| (all << 8) | *byte as u128);
    let characters: Vec<u8> = (0..24)
        .map(|i| ALPHABET[((bits >> (115 - 5 * i)) & 31) as usize])
        .collect();
    characters
        .chunks(4)
        .map(|group| String::from_utf8_lossy(group).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// Reads a code as typed: any case, with or without hyphens and spaces, the look-alikes O, I
/// and L taken as 0 and 1.
pub fn decode(text: &str) -> Result<Code, String> {
    let mut bits = 0u128;
    let mut count = 0;
    for character in text.chars().filter(|c| *c != '-' && !c.is_whitespace()) {
        let character = match character.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let Some(value) = ALPHABET.iter().position(|a| *a as char == character) else {
            return Err(t!("That code has a character codes never use. Check it and try again."));
        };
        bits = (bits << 5) | value as u128;
        count += 1;
        if count > 24 {
            break;
        }
    }
    if count != 24 {
        return Err(t!("Enter all 24 characters of the code."));
    }
    Ok(std::array::from_fn(|i| (bits >> (112 - 8 * i)) as u8))
}

/// Reads what a scanner or a keyboard gave: the code, and which kind a QR code said it was.
fn read(text: &str) -> Result<(Option<Kind>, Code), String> {
    let text = text.trim();
    for kind in [Kind::Offer, Kind::Request] {
        if let Some(code) = text.strip_prefix(kind.prefix()) {
            return Ok((Some(kind), decode(code)?));
        }
    }
    Ok((None, decode(text)?))
}

/// The code is uniformly random, so a hash under its own label is all the derivation needed.
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

/// The QR code's squares, row by row: `1` is dark. Each app draws them itself.
fn qr(kind: Kind, code: &Code) -> Result<Value, String> {
    let payload = format!("{}{}", kind.prefix(), encode(code).replace('-', ""));
    let grid = qrcode::QrCode::with_error_correction_level(payload.as_bytes(), qrcode::EcLevel::M)
        .map_err(|e| e.to_string())?;
    let modules: String = grid
        .to_colors()
        .into_iter()
        .map(|color| if color == qrcode::Color::Dark { '1' } else { '0' })
        .collect();
    Ok(json!({"size": grid.width(), "modules": modules}))
}

/// What the server said, in the app's language, without the code it leads with.
pub(super) fn told(error: String) -> String {
    if error.starts_with("OFFLINE:") {
        return t!("Couldn't reach urbudget.app. Check your internet connection and try again.");
    }
    match error.split_once(": ") {
        Some((code, message)) if !code.is_empty() && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') => {
            crate::i18n::translate(message)
        }
        _ => error,
    }
}

/// The key this device holds for the account's current generation, on either protocol, if it
/// still opens the account's check value.
fn held_key(cloud: &Cloud, account: &str, bootstrap: &Value, generation: &str) -> Option<crypto::Key> {
    let key = if bootstrap["protocol"] == 2 {
        v2::read_key(cloud, account, generation)?
    } else {
        let (key, held) = cloud.sync.legacy_key()?;
        (held.to_string() == generation).then_some(key)?
    };
    opens_check(&key, bootstrap).then_some(key)
}
pub(super) fn opens_check(key: &crypto::Key, bootstrap: &Value) -> bool {
    bootstrap["check"]
        .as_str()
        .and_then(|check| BASE64.decode(check).ok())
        .is_some_and(|check| crypto::matches_check(key, &check))
}
async fn account(cloud: &Cloud, library: &Library) -> Result<(String, Value, String), String> {
    let (account, bootstrap) = v2::probe_bootstrap(cloud, library)
        .await
        .map_err(told)?
        .ok_or_else(|| t!("Couldn't reach urbudget.app. Check your internet connection and try again."))?;
    let generation = bootstrap["keyGeneration"]
        .as_str()
        .ok_or_else(|| t!("Sync hasn't been set up for this account yet."))?
        .to_owned();
    Ok((account, bootstrap, generation))
}

/// Seals this device's key for whoever holds `code` and leaves it on the server. Returns when
/// the server stops keeping it.
async fn leave(cloud: &Cloud, library: &Library, code: &Code) -> Result<Value, String> {
    let _operation = cloud.sync.operation().await?;
    let (account, bootstrap, generation) = account(cloud, library).await?;
    let key = held_key(cloud, &account, &bootstrap, &generation)
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
    let code: Code = crypto::random()?;
    let expires = leave(cloud, library, &code).await?;
    Ok(json!({"id":message_id(&code),"code":encode(&code),"qr":qr(Kind::Offer, &code)?,"expiresAt":expires}))
}

/// On a device that syncs, given the code a new device is showing: seals the key for it. The
/// screen has asked the person first; anyone given the key can read the account's budgets.
pub async fn approve(cloud: &Cloud, library: &Library, text: &str) -> Result<(), String> {
    let (kind, code) = read(text)?;
    if kind == Some(Kind::Offer) {
        return Err(t!("That code is for the device being added. Scan it there, where it asks to unlock sync."));
    }
    leave(cloud, library, &code).await.map(|_| ())
}

/// On the new device, signed in: makes a code to show, for a device that syncs to scan and
/// approve. Nothing is sent: the code means nothing to the server until a key is sealed for
/// it. Returns `{id, code, qr, expiresAt}`; the screen then asks `wait` until it answers.
pub fn request(cloud: &Cloud) -> Result<Value, String> {
    if cloud.token().is_none() {
        return Err(t!("Sign in to your UrBudget account first."));
    }
    let code: Code = crypto::random()?;
    let expires = (chrono::Utc::now() + chrono::Duration::minutes(MINUTES)).to_rfc3339();
    Ok(json!({"id":message_id(&code),"code":encode(&code),"qr":qr(Kind::Request, &code)?,"expiresAt":expires}))
}

/// Takes back a code nobody used, when its screen closes.
pub async fn cancel(cloud: &Cloud, id: &str) -> Result<(), String> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(t!("That code has expired or was already used. Make a new one on the other device."));
    }
    v2::Transport::new(cloud, String::new())?
        .json(Method::DELETE, &format!("/pairings/{id}"), None)
        .await
        .map_err(told)?;
    Ok(())
}

/// Collects the key sealed for `code`, opens it, and unlocks sync with it exactly as the right
/// passphrase would. `Ok(None)` when `patient` and nothing is waiting yet.
async fn collect(cloud: &Cloud, library: &Library, code: &Code, patient: bool) -> Result<Option<Value>, String> {
    let id = message_id(code);
    let message = match v2::Transport::new(cloud, String::new())?
        .json(Method::GET, &format!("/pairings/{id}"), None)
        .await
    {
        Ok(message) => message,
        Err(error) if patient && error.starts_with("pairing_not_found:") => return Ok(None),
        Err(error) => return Err(told(error)),
    };
    let (key, generation) = {
        let _operation = cloud.sync.operation().await?;
        let (account, bootstrap, generation) = account(cloud, library).await?;
        let reset = || t!("Sync was reset on another device. Unlock sync on this device again.");
        if message["keyGeneration"] != generation.as_str() {
            return Err(reset());
        }
        let key: crypto::Key = message["wrapped"]
            .as_str()
            .and_then(|sealed| BASE64.decode(sealed).ok())
            .and_then(|sealed| crypto::open(&wrapping_key(code), &context(&account, &id, &generation), &sealed).ok())
            .and_then(|key| key.try_into().ok())
            .ok_or_else(|| t!("That code doesn't open what the other device sent. Make a new one and try again."))?;
        // Never keep a key that doesn't open this account, whatever sent it.
        if !opens_check(&key, &bootstrap) {
            return Err(reset());
        }
        (key, generation)
    };
    if let Some(unlocked) = v2::try_unlock(cloud, library, v2::Unlock::Key(key), false).await? {
        return Ok(Some(unlocked));
    }
    // The account syncs through protocol 1 and stays there for now.
    let _operation = cloud.sync.operation().await?;
    cloud.check_account()?;
    let generation = generation.parse().map_err(|_| t!("Sync hasn't been set up for this account yet."))?;
    cloud.sync.install_key(key, generation)?;
    Ok(Some(json!({ "created": false })))
}

/// On the new device, given the code a device that syncs is showing. Answers as `sync_setup`
/// does.
pub async fn accept(cloud: &Cloud, library: &Library, text: &str) -> Result<Value, String> {
    let (kind, code) = read(text)?;
    if kind == Some(Kind::Request) {
        return Err(t!("That code is shown by a device waiting to be added. Scan it from Add a device, on a device that already syncs."));
    }
    collect(cloud, library, &code, false)
        .await?
        .ok_or_else(|| t!("That code has expired or was already used. Make a new one on the other device."))
}

/// On the new device, given whichever code the person has: the one a device that syncs is
/// showing, or the account's recovery code. The two look alike, so a screen needn't ask which
/// it is. Answers as `sync_setup` does.
pub async fn unlock(cloud: &Cloud, library: &Library, text: &str) -> Result<Value, String> {
    let (kind, code) = read(text)?;
    if kind == Some(Kind::Request) {
        return Err(t!("That code is shown by a device waiting to be added. Scan it from Add a device, on a device that already syncs."));
    }
    if let Some(unlocked) = collect(cloud, library, &code, true).await? {
        return Ok(unlocked);
    }
    // No device left a key for it. A scanned code can only be a device's; a typed one may be
    // the recovery code.
    if kind.is_none() {
        if let Some(unlocked) = super::recovery::recover_with(cloud, library, code).await? {
            return Ok(unlocked);
        }
    }
    Err(t!("That code didn't work. Check it, or make a new one on the other device."))
}

/// On the new device, while it shows the code it made with `request`: `{ready: false}` until a
/// device that syncs has approved it, then `{ready: true}` with sync unlocked here.
pub async fn wait(cloud: &Cloud, library: &Library, text: &str) -> Result<Value, String> {
    let (_, code) = read(text)?;
    Ok(match collect(cloud, library, &code, true).await? {
        Some(mut unlocked) => {
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
    fn a_code_reads_back_however_it_is_typed() {
        let code: Code = [0x00, 0x44, 0x32, 0x14, 0xc7, 0x42, 0x54, 0xb6, 0x35, 0xcf, 0x84, 0x65, 0x3a, 0x56, 0xd7];
        let shown = encode(&code);
        assert_eq!(shown, "0123-4567-89AB-CDEF-GHJK-MNPQ");
        assert_eq!(decode(&shown).unwrap(), code);
        assert_eq!(decode(" 0123 4567 89ab cdef ghjk mnpq ").unwrap(), code);
        // The look-alikes people type for 0 and 1.
        assert_eq!(decode("oi23-4567-89AB-CDEF-GHJK-MNPQ").unwrap(), decode("0123-4567-89AB-CDEF-GHJK-MNPQ").unwrap());
        assert_eq!(decode("0L23456789ABCDEFGHJKMNPQ").unwrap(), code);
        let all: Code = [0xff; CODE_LEN];
        assert_eq!(encode(&all), "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ");
        assert_eq!(decode(&encode(&all)).unwrap(), all);
        for wrong in ["", "0123-4567-89AB-CDEF-GHJK-MNP", "0123-4567-89AB-CDEF-GHJK-MNPQR", "0123-4567-89AB-CDEF-GHJK-MNPU", "0123-4567-89AB-CDEF-GHJK-MNP!"] {
            assert!(decode(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_scanned_code_says_which_device_showed_it_and_a_typed_one_doesnt() {
        let code: Code = [0x00, 0x44, 0x32, 0x14, 0xc7, 0x42, 0x54, 0xb6, 0x35, 0xcf, 0x84, 0x65, 0x3a, 0x56, 0xd7];
        assert_eq!(read("URBUDGET-PAIR:1:0123456789ABCDEFGHJKMNPQ").unwrap(), (Some(Kind::Offer), code));
        assert_eq!(read(" URBUDGET-ASK:1:0123456789ABCDEFGHJKMNPQ\n").unwrap(), (Some(Kind::Request), code));
        assert_eq!(read("0123-4567-89ab-cdef-ghjk-mnpq").unwrap(), (None, code));
        // Some other QR code, or a later version of ours this one doesn't know.
        assert!(read("https://example.com/0123456789ABCDEFGHJKMNPQ").is_err());
        assert!(read("URBUDGET-PAIR:2:0123456789ABCDEFGHJKMNPQ").is_err());
        for kind in [Kind::Offer, Kind::Request] {
            assert!(!kind.prefix().contains("://"), "a link would open in another app");
        }
    }

    #[test]
    fn the_sealed_key_opens_only_with_its_code_for_its_account_and_generation() {
        let code: Code = [7; CODE_LEN];
        let other: Code = [8; CODE_LEN];
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
    fn the_qr_code_is_small_enough_to_scan_from_a_screen() {
        let code: Code = [9; CODE_LEN];
        for kind in [Kind::Offer, Kind::Request] {
            let drawn = qr(kind, &code).unwrap();
            let size = drawn["size"].as_u64().unwrap() as usize;
            let modules = drawn["modules"].as_str().unwrap();
            assert_eq!(modules.len(), size * size);
            assert!(modules.bytes().all(|b| b == b'0' || b == b'1'));
            // Version 3 or below: big squares at arm's length.
            assert!(size <= 29, "{size}");
        }
        assert_ne!(qr(Kind::Offer, &code).unwrap(), qr(Kind::Request, &code).unwrap());
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
