//! End-to-end encryption for sync. The key comes from the sync passphrase (Argon2id with a salt
//! the server keeps); budgets are compressed, then sealed with XChaCha20-Poly1305 and bound to
//! their budget id, so the server can neither read nor swap them.
use crate::t;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use std::io::{Read, Write};

const MAGIC: &[u8; 4] = b"UBS1";
const NONCE: usize = 24;
pub const SALT_LEN: usize = 16;
pub const WRONG_KEY: &str = "WRONG_KEY";
/// Decompressed budgets larger than this are refused rather than unpacked.
const MAX_PLAINTEXT: u64 = 512 * 1024 * 1024;

pub type Key = [u8; 32];

/// The sync key for `passphrase`: Argon2id, 64 MiB, 3 passes. Slow on purpose (about a second).
pub fn derive(passphrase: &str, salt: &[u8]) -> Result<Key, String> {
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| e.to_string())?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

pub fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// `UBS1 | nonce | ciphertext+tag`, authenticated together with `aad`.
pub fn seal(key: &Key, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let nonce: [u8; NONCE] = random()?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|e| e.to_string())?;
    let sealed = cipher
        .encrypt(&XNonce::from(nonce), Payload { msg: plaintext, aad })
        .map_err(|_| t!("Encryption failed."))?;
    Ok([MAGIC.as_slice(), &nonce, &sealed].concat())
}

/// Opens what `seal` made; a wrong key (or tampering) fails with `WRONG_KEY`.
pub fn open(key: &Key, aad: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < MAGIC.len() + NONCE + 16 || &data[..4] != MAGIC {
        return Err(t!("This isn't an UrBudget sync file."));
    }
    let nonce: [u8; NONCE] = data[4..4 + NONCE].try_into().expect("length checked");
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|e| e.to_string())?;
    cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: &data[4 + NONCE..], aad })
        .map_err(|_| WRONG_KEY.to_string())
}

const CHECK_AAD: &[u8] = b"urbudget:key-check";
/// A value only the right key opens, kept by the server so a device can test a passphrase.
pub fn key_check(key: &Key) -> Result<Vec<u8>, String> {
    seal(key, CHECK_AAD, b"urbudget")
}
pub fn matches_check(key: &Key, check: &[u8]) -> bool {
    open(key, CHECK_AAD, check).is_ok_and(|p| p == b"urbudget")
}

pub fn compress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_PLAINTEXT + 1)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    if out.len() as u64 > MAX_PLAINTEXT {
        return Err(t!("This synced budget is too large to open."));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens_only_with_the_same_key_and_context() {
        let key = derive("correct horse battery staple", b"0123456789abcdef").unwrap();
        let other = derive("correct horse battery stapler", b"0123456789abcdef").unwrap();
        assert_ne!(key, other);
        let sealed = seal(&key, b"budget:a", b"hello").unwrap();
        assert_eq!(open(&key, b"budget:a", &sealed).unwrap(), b"hello");
        assert_eq!(open(&other, b"budget:a", &sealed).unwrap_err(), WRONG_KEY);
        assert_eq!(open(&key, b"budget:b", &sealed).unwrap_err(), WRONG_KEY, "bound to its budget");
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(open(&key, b"budget:a", &tampered).is_err());
        assert_ne!(seal(&key, b"budget:a", b"hello").unwrap(), sealed, "fresh nonce each time");
    }

    #[test]
    fn checks_a_passphrase_without_a_budget() {
        let key = derive("one", b"saltsaltsaltsalt").unwrap();
        let check = key_check(&key).unwrap();
        assert!(matches_check(&key, &check));
        assert!(!matches_check(&derive("two", b"saltsaltsaltsalt").unwrap(), &check));
    }

    #[test]
    fn compression_round_trips() {
        let text = "budget ".repeat(10_000);
        let packed = compress(text.as_bytes()).unwrap();
        assert!(packed.len() < text.len() / 10);
        assert_eq!(decompress(&packed).unwrap(), text.as_bytes());
    }
}
