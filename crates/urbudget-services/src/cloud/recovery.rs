//! The recovery code: what unlocks sync when no other device is at hand.
//!
//! A device that sets sync up without a passphrase makes a recovery code and makes the key
//! from it, exactly as a passphrase would (`v2::setup_once`). So the code unlocks any device
//! on any version: typed as it is shown into an older app's passphrase box, it makes the same
//! key.
//!
//! Two things here make that safe to rely on:
//! - **The code is kept until it's saved.** It goes into the vault before the request leaves
//!   and stays until the person confirms saving it, so a crash or a lost answer doesn't lose
//!   the only thing that unlocks a second device.
//! - **A lost code is replaced without a reset.** A device that holds the key makes a new code
//!   and leaves the server a *recovery copy*: the key sealed with a key stretched from that
//!   code. The server holds the copy and its salt, neither of which opens a budget, and never
//!   sees a code. What the key was first made from still works; only a reset retires it.
//!
//! See docs/plans/2026-11-24-sync-device-approval.md.
use super::{codes, crypto, pairing, v2, Cloud};
use crate::{t, Library};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as BASE64, Engine as _};
use reqwest::Method;
use serde_json::{json, Value};

fn service(account: &str) -> String {
    super::service(&format!("com.urbudget.sync-recovery.{account}"))
}

/// Keeps `code` on this device until the person confirms saving it. `made_for` is what the
/// server holds for the key this code opens (a key check, or a recovery copy), so a code is
/// never shown once that key is gone.
pub(super) fn keep(cloud: &Cloud, account: &str, code: &str, made_for: &str) -> Result<(), String> {
    let kept = json!({"code":code,"for":made_for}).to_string();
    if super::vault::store().is_some() {
        super::vault::save(&service(account), &kept)?;
    }
    cloud.kept_codes.lock().unwrap_or_else(|e| e.into_inner()).insert(account.into(), kept);
    Ok(())
}
pub(super) fn forget(cloud: &Cloud, account: &str) -> Result<(), String> {
    super::vault::delete(&service(account))?;
    cloud.kept_codes.lock().unwrap_or_else(|e| e.into_inner()).remove(account);
    Ok(())
}
fn kept(cloud: &Cloud, account: &str) -> Option<(String, String)> {
    let raw = super::vault::read(&service(account))
        .or_else(|| cloud.kept_codes.lock().unwrap_or_else(|e| e.into_inner()).get(account).cloned())?;
    let kept: Value = serde_json::from_str(&raw).ok()?;
    Some((kept["code"].as_str()?.to_owned(), kept["for"].as_str()?.to_owned()))
}
/// The kept code, when it was made for exactly `made_for`.
pub(super) fn kept_for(cloud: &Cloud, account: &str, made_for: &str) -> Option<String> {
    kept(cloud, account).filter(|(_, kept_for)| kept_for == made_for).map(|(code, _)| code)
}
/// The code this device holds that the person hasn't confirmed saving, while it still opens
/// the account's key: it made that key, or the account's recovery copy was sealed with it.
pub(super) fn unsaved(cloud: &Cloud, account: &str, bootstrap: &Value) -> Option<String> {
    let (code, made_for) = kept(cloud, account)?;
    [&bootstrap["check"], &bootstrap["recovery"]["wrapped"]]
        .into_iter()
        .any(|held| held.as_str() == Some(made_for.as_str()))
        .then_some(code)
}

/// Binds the recovery copy to the account and the key generation it was made for.
fn context(account: &str, generation: &str) -> Vec<u8> {
    v2::canonical(&json!({"purpose":"recovery","accountId":account,"keyGeneration":generation})).into_bytes()
}

/// Makes a recovery code for `key`: the code as people read it, and what the server keeps for
/// it, `{salt, wrapped}`. Stretching the code takes as long as a passphrase does.
fn wrap(account: &str, generation: &str, key: &crypto::Key) -> Result<(String, Value), String> {
    let code = codes::encode(&codes::new(codes::Kind::Recovery)?);
    let salt: [u8; crypto::SALT_LEN] = crypto::random()?;
    let sealed = crypto::seal(&crypto::derive(&code, &salt)?, &context(account, generation), key)?;
    Ok((code, json!({"salt":BASE64.encode(salt),"wrapped":BASE64.encode(sealed)})))
}

/// Opens the server's copy of the key with the code. `None` when the copy wasn't sealed with
/// this code, or is for another account or generation.
fn open(account: &str, generation: &str, recovery: &Value, code: &str) -> Option<crypto::Key> {
    let salt = BASE64.decode(recovery["salt"].as_str()?).ok()?;
    let sealed = BASE64.decode(recovery["wrapped"].as_str()?).ok()?;
    let wrapping = crypto::derive(code, &salt).ok()?;
    crypto::open(&wrapping, &context(account, generation), &sealed).ok()?.try_into().ok()
}

/// The account's key from a recovery code, given the account's `bootstrap`: the key the code
/// makes with the account's salt, or failing that the key in the recovery copy the code opens.
/// `None` when the code is neither. Slow on purpose, so call it off the async executor.
pub(super) fn key_from_code(account: &str, bootstrap: &Value, code: &codes::Code) -> Option<crypto::Key> {
    let code = codes::encode(code);
    let made = bootstrap["salt"]
        .as_str()
        .and_then(|salt| BASE64.decode(salt).ok())
        .and_then(|salt| crypto::derive(&code, &salt).ok());
    if let Some(key) = v2::verified_key(made, bootstrap) {
        return Some(key);
    }
    let generation = bootstrap["keyGeneration"].as_str()?;
    // Never keep a key that doesn't open this account, whatever the server's copy held.
    v2::verified_key(open(account, generation, &bootstrap["recovery"], &code), bootstrap)
}

/// On a device where sync is unlocked: makes a new recovery code and leaves the server a copy
/// of the key sealed for it, replacing any copy made for an earlier one. The key, its
/// generation and the budgets stay as they are. Returns `{recoveryCode}`, kept here until the
/// person confirms saving it.
pub async fn create(cloud: &Cloud, library: &Library) -> Result<Value, String> {
    let _operation = cloud.sync.operation().await?;
    Ok(json!({ "recoveryCode": replace(cloud, library).await? }))
}

/// `create`, for a caller that already holds the sync operation. Returns the code.
pub(super) async fn replace(cloud: &Cloud, library: &Library) -> Result<Value, String> {
    let (account, bootstrap) = v2::probe_bootstrap(cloud, library).await.map_err(pairing::told)?.ok_or_else(pairing::offline)?;
    if bootstrap["protocol"] != 2 {
        return Err(t!("Move this account to the new sync first, then make a recovery code."));
    }
    let generation = bootstrap["keyGeneration"].as_str().unwrap_or_default().to_owned();
    let key = v2::held_key(cloud, &account, &bootstrap)
        .ok_or_else(|| t!("Unlock sync on this device before making a recovery code."))?;
    let (code, recovery) = {
        let (account, generation) = (account.clone(), generation.clone());
        tokio::task::spawn_blocking(move || wrap(&account, &generation, &key)).await.map_err(|e| e.to_string())??
    };
    // Kept before the request leaves, so a lost answer doesn't lose a code the server took.
    let wrapped = recovery["wrapped"].as_str().unwrap_or_default().to_owned();
    let before = kept(cloud, &account);
    keep(cloud, &account, &code, &wrapped)?;
    let sent = v2::Transport::new(cloud, String::new())?
        .json(Method::PUT, "/recovery", Some(json!({"keyGeneration":generation,"salt":recovery["salt"],"wrapped":recovery["wrapped"]})))
        .await;
    if let Err(error) = sent {
        // Nothing changed on the server as far as this device knows: the code it held before,
        // if any, is still the one to show. An answer lost after the server took the new copy
        // leaves the old code kept and still valid for the key it made.
        match before {
            Some((code, made_for)) => keep(cloud, &account, &code, &made_for)?,
            None => forget(cloud, &account)?,
        }
        return Err(pairing::told(error));
    }
    v2::save_recovery(library, &account, &recovery)?;
    Ok(json!(code))
}

/// The recovery code this device still holds unconfirmed, as `{recoveryCode}`, or `null`.
pub fn unsaved_code(cloud: &Cloud, library: &Library) -> Result<Value, String> {
    let Some(account) = cloud.selected_account() else { return Ok(Value::Null) };
    let state = library.lock().account_state(&account)?;
    Ok(match unsaved(cloud, &account, &state["bootstrap"]) {
        Some(code) => json!({ "recoveryCode": code }),
        None => Value::Null,
    })
}

/// The person has saved the code: this device stops holding it.
pub fn saved(cloud: &Cloud) -> Result<(), String> {
    match cloud.selected_account() {
        Some(account) => forget(cloud, &account),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(shown: &str) -> codes::Code {
        codes::decode(shown).unwrap()
    }
    fn account_key(code: &str, salt: &[u8]) -> (crypto::Key, Value) {
        let key = crypto::derive(code, salt).unwrap();
        let bootstrap = json!({"keyGeneration":"3","salt":BASE64.encode(salt),"check":BASE64.encode(crypto::key_check(&key).unwrap()),"recovery":null});
        (key, bootstrap)
    }

    #[test]
    fn the_code_an_account_was_set_up_with_makes_its_key_however_it_is_typed() {
        let shown = codes::encode(&codes::new(codes::Kind::Recovery).unwrap());
        let (key, bootstrap) = account_key(&shown, b"0123456789abcdef");
        assert_eq!(key_from_code("usr_a", &bootstrap, &decoded(&shown)), Some(key));
        // Lower case, no hyphens: read back to the text the key was made from.
        assert_eq!(key_from_code("usr_a", &bootstrap, &decoded(&shown.replace('-', "").to_lowercase())), Some(key));
        // An app from before recovery codes makes the same key from the code typed as shown.
        assert_eq!(crypto::derive(&shown, b"0123456789abcdef").unwrap(), key);
        let other = codes::new(codes::Kind::Recovery).unwrap();
        assert_eq!(key_from_code("usr_a", &bootstrap, &other), None);
    }

    #[test]
    fn a_replacement_code_opens_only_the_copy_made_for_its_account_and_generation() {
        let (key, mut bootstrap) = account_key("a passphrase chosen long ago", b"0123456789abcdef");
        let (shown, recovery) = wrap("usr_a", "3", &key).unwrap();
        assert_eq!(codes::kind(&decoded(&shown)), Some(codes::Kind::Recovery));
        assert_eq!(open("usr_a", "3", &recovery, &shown), Some(key));
        assert_eq!(open("usr_b", "3", &recovery, &shown), None, "another account");
        assert_eq!(open("usr_a", "4", &recovery, &shown), None, "another key generation");
        let other = codes::encode(&codes::new(codes::Kind::Recovery).unwrap());
        assert_eq!(open("usr_a", "3", &recovery, &other), None, "another code");
        for broken in [json!({}), json!(null), json!({"salt":recovery["salt"]}), json!({"salt":"!","wrapped":recovery["wrapped"]}), json!({"salt":recovery["salt"],"wrapped":"AAAA"})] {
            assert_eq!(open("usr_a", "3", &broken, &shown), None, "{broken}");
        }
        // Through the account: the code isn't what the key was made from, so the copy is opened.
        assert_eq!(key_from_code("usr_a", &bootstrap, &decoded(&shown)), None, "no copy yet");
        bootstrap["recovery"] = recovery.clone();
        assert_eq!(key_from_code("usr_a", &bootstrap, &decoded(&shown)), Some(key));
        assert_eq!(key_from_code("usr_b", &bootstrap, &decoded(&shown)), None);
        // A copy that opens to some other key is never kept.
        let (_, forged) = wrap("usr_a", "3", &[9; 32]).unwrap();
        bootstrap["recovery"] = forged;
        assert_eq!(key_from_code("usr_a", &bootstrap, &decoded(&shown)), None);
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
