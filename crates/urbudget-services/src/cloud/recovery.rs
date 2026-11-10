//! The recovery code: what unlocks sync when no other device is at hand.
//!
//! An account set up without a passphrase has a random key, which nothing typed can make
//! again. The device that makes the key also makes a code, shows it once for the person to
//! keep, and leaves on the server a copy of the key sealed with a key stretched from that code.
//! A new device given the code opens the copy. The server holds the sealed copy and the salt,
//! neither of which opens a budget, and never sees the code.
//!
//! The code has 120 random bits, so unlike a passphrase it can't be guessed from a copy of the
//! server's data. An account whose key is made from a passphrase can have one too.
//!
//! See docs/plans/2026-11-10-sync-device-approval.md.
use super::{crypto, pairing, v2, Cloud};
use crate::{t, Library};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64, Engine as _};
use reqwest::Method;
use serde_json::{json, Value};

/// What the wrapping key is stretched from: the code's 24 characters, as they read.
fn secret(code: &pairing::Code) -> String {
    pairing::encode(code).replace('-', "")
}
/// Binds the sealed copy to the account and the key generation it was made for.
fn context(account: &str, generation: &str) -> Vec<u8> {
    v2::canonical(&json!({"purpose":"recovery","accountId":account,"keyGeneration":generation})).into_bytes()
}

/// Makes a recovery code for `key`: the code as people read it, and what the server keeps for
/// it, `{salt, wrapped}`. Stretching the code takes as long as a passphrase does.
pub(super) fn wrap(account: &str, generation: &str, key: &crypto::Key) -> Result<(String, Value), String> {
    let code: pairing::Code = crypto::random()?;
    let salt: [u8; crypto::SALT_LEN] = crypto::random()?;
    let sealed = crypto::seal(&crypto::derive(&secret(&code), &salt)?, &context(account, generation), key)?;
    Ok((pairing::encode(&code), json!({"salt":BASE64.encode(salt),"wrapped":BASE64.encode(sealed)})))
}

/// Opens the server's copy of the key with the code. `None` when the code isn't the one the
/// copy was sealed with, or the copy is for another account or generation.
fn open(account: &str, generation: &str, recovery: &Value, code: &pairing::Code) -> Option<crypto::Key> {
    let salt = BASE64.decode(recovery["salt"].as_str()?).ok()?;
    let sealed = BASE64.decode(recovery["wrapped"].as_str()?).ok()?;
    let wrapping = crypto::derive(&secret(code), &salt).ok()?;
    crypto::open(&wrapping, &context(account, generation), &sealed).ok()?.try_into().ok()
}

fn offline() -> String {
    t!("Couldn't reach urbudget.app. Check your internet connection and try again.")
}

/// On a new device, signed in, with no other device at hand: unlocks sync with the recovery
/// code. Answers as `sync_setup` does.
pub async fn recover(cloud: &Cloud, library: &Library, text: &str) -> Result<Value, String> {
    recover_with(cloud, library, pairing::decode(text)?)
        .await?
        .ok_or_else(|| t!("That isn't the recovery code for this account. Check it and try again."))
}

/// `recover`, for a caller that will say what a code that opens nothing means: `Ok(None)` when
/// the account has no recovery code or this isn't it.
pub(super) async fn recover_with(cloud: &Cloud, library: &Library, code: pairing::Code) -> Result<Option<Value>, String> {
    let key = {
        let _operation = cloud.sync.operation().await?;
        let (account, bootstrap) = v2::probe_bootstrap(cloud, library).await.map_err(pairing::told)?.ok_or_else(offline)?;
        let generation = bootstrap["keyGeneration"]
            .as_str()
            .ok_or_else(|| t!("Sync hasn't been set up for this account yet."))?
            .to_owned();
        if !bootstrap["recovery"].is_object() {
            return Ok(None);
        }
        let recovery = bootstrap["recovery"].clone();
        let opened = tokio::task::spawn_blocking(move || open(&account, &generation, &recovery, &code))
            .await
            .map_err(|e| e.to_string())?;
        // Never keep a key that doesn't open this account, whatever the server's copy held.
        match opened.filter(|key| pairing::opens_check(key, &bootstrap)) {
            Some(key) => key,
            None => return Ok(None),
        }
    };
    v2::try_unlock(cloud, library, v2::Unlock::Key(key), false)
        .await?
        .ok_or_else(|| t!("Sync hasn't been set up for this account yet."))
        .map(Some)
}

/// On a device where sync is unlocked: makes a new recovery code and replaces the server's
/// copy of the key with one sealed for it, so the old code stops working. The key, its
/// generation and the budgets stay as they are. Returns `{recoveryCode}`, shown once.
pub async fn create(cloud: &Cloud, library: &Library) -> Result<Value, String> {
    let _operation = cloud.sync.operation().await?;
    Ok(json!({ "recoveryCode": replace(cloud, library).await? }))
}

/// `create`, for a caller that already holds the sync operation. Returns the code.
pub(super) async fn replace(cloud: &Cloud, library: &Library) -> Result<Value, String> {
    let (account, bootstrap) = v2::probe_bootstrap(cloud, library).await.map_err(pairing::told)?.ok_or_else(offline)?;
    let generation = bootstrap["keyGeneration"].as_str().filter(|_| bootstrap["protocol"] == 2).map(str::to_owned);
    let key = generation
        .as_deref()
        .and_then(|generation| v2::read_key(cloud, &account, generation))
        .filter(|key| pairing::opens_check(key, &bootstrap))
        .ok_or_else(|| t!("Unlock sync on this device before making a recovery code."))?;
    let generation = generation.unwrap_or_default();
    let (shown, recovery) = {
        let (account, generation) = (account.clone(), generation.clone());
        tokio::task::spawn_blocking(move || wrap(&account, &generation, &key))
            .await
            .map_err(|e| e.to_string())??
    };
    v2::Transport::new(cloud, String::new())?
        .json(Method::PUT, "/recovery", Some(json!({"keyGeneration":generation,"salt":recovery["salt"],"wrapped":recovery["wrapped"]})))
        .await
        .map_err(pairing::told)?;
    v2::save_recovery(library, &account, &recovery)?;
    Ok(json!(shown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recovery_code_opens_only_the_copy_made_for_its_account_and_generation() {
        let key: crypto::Key = [7; 32];
        let (shown, recovery) = wrap("usr_a", "3", &key).unwrap();
        let code = pairing::decode(&shown).unwrap();
        assert_eq!(shown.len(), 29, "six groups of four");
        assert_eq!(open("usr_a", "3", &recovery, &code), Some(key));
        // Typed in lower case, without hyphens.
        let typed = pairing::decode(&shown.replace('-', "").to_lowercase()).unwrap();
        assert_eq!(open("usr_a", "3", &recovery, &typed), Some(key));
        assert_eq!(open("usr_b", "3", &recovery, &code), None, "another account");
        assert_eq!(open("usr_a", "4", &recovery, &code), None, "another key generation");
        let mut other = code;
        other[0] ^= 1;
        assert_eq!(open("usr_a", "3", &recovery, &other), None, "another code");
        for broken in [json!({}), json!({"salt":recovery["salt"]}), json!({"salt":"!","wrapped":recovery["wrapped"]}), json!({"salt":recovery["salt"],"wrapped":"AAAA"})] {
            assert_eq!(open("usr_a", "3", &broken, &code), None, "{broken}");
        }
    }

    #[test]
    fn each_recovery_code_is_new_and_the_server_copy_holds_neither_code_nor_key() {
        let key: crypto::Key = [9; 32];
        let (first, kept) = wrap("usr_a", "0", &key).unwrap();
        let (second, again) = wrap("usr_a", "0", &key).unwrap();
        assert_ne!(first, second);
        assert_ne!(kept, again);
        let sealed = BASE64.decode(kept["wrapped"].as_str().unwrap()).unwrap();
        assert!(!sealed.windows(key.len()).any(|window| window == key), "the key itself");
        assert!(!kept.to_string().contains(&first.replace('-', "")), "the code");
        // What the server keeps fits its bounds (web/src/sync-v2/protocol.ts).
        assert!(kept["wrapped"].as_str().unwrap().len() <= 344);
        assert!((16..=128).contains(&kept["salt"].as_str().unwrap().len()));
    }
}
