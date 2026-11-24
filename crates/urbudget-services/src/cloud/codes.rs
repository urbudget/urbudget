//! The codes people scan, read and type to unlock sync on a device.
//!
//! A code is 24 characters: 112 random bits and a tag. The tag checks a typed code for typos
//! and says which kind it is, so a wrong character is said at once and a code is never tried
//! as the other kind:
//! - a *device code* is shown by one device and used once by another, within five minutes
//!   (`pairing`);
//! - a *recovery code* is kept by the person (`recovery`).
//!
//! See docs/plans/2026-11-24-sync-device-approval.md.
use super::crypto;
use crate::t;
use sha2::{Digest, Sha256};

pub(super) const LEN: usize = 15;
pub(super) type Code = [u8; LEN];
/// Crockford's base32: no I, L, O or U, so a code read aloud or typed has no look-alikes.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHARACTERS: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Device,
    Recovery,
}

/// Seven bits that depend on every random bit and on the kind, and one that says the kind.
/// The check covers the kind, so a typo in the last character can't turn one kind of code
/// into a valid code of the other.
fn tag(random: &[u8], kind: Kind) -> u8 {
    let bit = u8::from(kind == Kind::Recovery);
    let digest = Sha256::new().chain_update("urbudget:code:v1").chain_update([bit]).chain_update(random).finalize();
    (digest[0] & 0xfe) | bit
}

pub(super) fn new(kind: Kind) -> Result<Code, String> {
    let mut code: Code = crypto::random()?;
    code[LEN - 1] = tag(&code[..LEN - 1], kind);
    Ok(code)
}

/// Which kind a code is, or `None` when its tag doesn't fit: a typo, or not one of ours.
pub(super) fn kind(code: &Code) -> Option<Kind> {
    [Kind::Device, Kind::Recovery].into_iter().find(|kind| code[LEN - 1] == tag(&code[..LEN - 1], *kind))
}

/// The code as people read it, and the exact text a recovery code's key is made from: upper
/// case, six groups of four.
pub fn encode(code: &Code) -> String {
    let bits = code.iter().fold(0u128, |all, byte| (all << 8) | *byte as u128);
    let characters: Vec<u8> =
        (0..CHARACTERS).map(|i| ALPHABET[((bits >> (115 - 5 * i)) & 31) as usize]).collect();
    characters.chunks(4).map(|group| String::from_utf8_lossy(group).into_owned()).collect::<Vec<_>>().join("-")
}
/// The same without hyphens, as a QR code holds it.
pub(super) fn compact(code: &Code) -> String {
    encode(code).replace('-', "")
}

/// Reads 24 code characters however they were typed: any case, with or without hyphens and
/// spaces, the look-alikes O, I and L taken as 0 and 1. `None` for anything else. Says nothing
/// about the tag (`kind`).
pub(super) fn decode(text: &str) -> Option<Code> {
    let mut bits = 0u128;
    let mut count = 0;
    for character in text.chars().filter(|c| *c != '-' && !c.is_whitespace()) {
        let character = match character.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        };
        let value = ALPHABET.iter().position(|a| *a as char == character)?;
        count += 1;
        if count > CHARACTERS {
            return None;
        }
        bits = (bits << 5) | value as u128;
    }
    (count == CHARACTERS).then(|| std::array::from_fn(|i| (bits >> (112 - 8 * i)) as u8))
}

/// What a QR code holds before the characters: a link of UrBudget's own kind. People point
/// the phone's own camera at a code, which can do nothing with plain text ("No usable data
/// found"); with a link it offers to open UrBudget, which then does what its own scanner would.
/// A code is no use to anything else that might open the link: a device code opens nothing
/// without the account's signed-in session, and a request only asks to be approved.
pub(super) const OFFER: &str = "urbudget://pair/1/";
pub(super) const REQUEST: &str = "urbudget://ask/1/";
/// What a code held before it was a link. Still read.
const PLAIN_OFFER: &str = "URBUDGET-PAIR:1:";
const PLAIN_REQUEST: &str = "URBUDGET-ASK:1:";

/// `text` after `prefix`, whatever case the prefix came in: a phone hands a link on with its
/// first part in lower case, and a scanner hands it on as drawn.
fn after<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

/// Whether this is a scanned code shown by a device that syncs, as against one typed or one
/// shown by a device waiting to be added.
pub(super) fn scanned_offer(text: &str) -> bool {
    let text = text.trim();
    after(text, OFFER).is_some() || after(text, PLAIN_OFFER).is_some()
}

/// What someone scanned or typed where a device asks to unlock sync, or to approve another.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Entry {
    /// A device code, typed, or scanned from a device that syncs.
    Device(Code),
    /// Scanned from a device that is waiting to be added.
    Request(Code),
    Recovery(Code),
    /// 24 code characters whose tag doesn't fit: most likely a typo.
    Mistyped,
    /// Anything else: a passphrase.
    Other,
}

pub(super) fn typo() -> String {
    t!("That code has a typo. Check each character and try again.")
}
fn unreadable() -> String {
    t!("That isn't an UrBudget code. Scan the code UrBudget shows on your other device.")
}

/// Sorts what a scanner or a keyboard gave. A scanned code that isn't whole is an error; typed
/// text that isn't a code is a passphrase.
pub(super) fn read(text: &str) -> Result<Entry, String> {
    let trimmed = text.trim();
    for (prefix, request) in [(OFFER, false), (REQUEST, true), (PLAIN_OFFER, false), (PLAIN_REQUEST, true)] {
        if let Some(rest) = after(trimmed, prefix) {
            let code = decode(rest).ok_or_else(unreadable)?;
            return match (kind(&code), request) {
                (Some(Kind::Device), false) => Ok(Entry::Device(code)),
                (Some(Kind::Device), true) => Ok(Entry::Request(code)),
                _ => Err(unreadable()),
            };
        }
    }
    if after(trimmed, "urbudget://").is_some() || trimmed.starts_with("URBUDGET-") {
        return Err(t!("A newer UrBudget made that code. Update UrBudget on this device first."));
    }
    Ok(match decode(trimmed) {
        Some(code) => match kind(&code) {
            Some(Kind::Device) => Entry::Device(code),
            Some(Kind::Recovery) => Entry::Recovery(code),
            None => Entry::Mistyped,
        },
        None => Entry::Other,
    })
}

/// The QR code's squares, row by row: `1` is dark. Each app draws them itself, so the three
/// draw the same code.
pub(super) fn qr(prefix: &str, code: &Code) -> Result<serde_json::Value, String> {
    let payload = format!("{prefix}{}", compact(code));
    let grid = qrcode::QrCode::with_error_correction_level(payload.as_bytes(), qrcode::EcLevel::M)
        .map_err(|e| e.to_string())?;
    let modules: String =
        grid.to_colors().into_iter().map(|color| if color == qrcode::Color::Dark { '1' } else { '0' }).collect();
    Ok(serde_json::json!({"size": grid.width(), "modules": modules}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(kind: Kind) -> Code {
        let mut code: Code = [0x00, 0x44, 0x32, 0x14, 0xc7, 0x42, 0x54, 0xb6, 0x35, 0xcf, 0x84, 0x65, 0x3a, 0x56, 0];
        code[LEN - 1] = tag(&code[..LEN - 1], kind);
        code
    }

    #[test]
    fn a_code_reads_back_however_it_is_typed() {
        let code = fixed(Kind::Device);
        let shown = encode(&code);
        // Fixed, so every later version reads what an older one on the other device showed.
        assert_eq!(shown, "0123-4567-89AB-CDEF-GHJK-MNG8");
        assert_eq!(shown.len(), 29, "six groups of four");
        assert_eq!(decode(&shown), Some(code));
        assert_eq!(decode(" 0123 4567 89ab cdef ghjk mng8 "), Some(code));
        // The look-alikes people type for 0 and 1.
        assert_eq!(decode("oi23-4567-89AB-CDEF-GHJK-MNG8"), Some(code));
        assert_eq!(decode("0L23456789ABCDEFGHJKMNG8"), Some(code));
        let all: Code = [0xff; LEN];
        assert_eq!(encode(&all), "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ");
        assert_eq!(decode(&encode(&all)), Some(all));
        for wrong in ["", "0123-4567-89AB-CDEF-GHJK-MNQ", "0123-4567-89AB-CDEF-GHJK-MNG8R", "0123-4567-89AB-CDEF-GHJK-MNQU", "0123-4567-89AB-CDEF-GHJK-MNQ!"] {
            assert_eq!(decode(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn the_tag_says_the_kind_and_catches_a_typo() {
        let device = fixed(Kind::Device);
        let recovery = fixed(Kind::Recovery);
        assert_eq!(kind(&device), Some(Kind::Device));
        assert_eq!(kind(&recovery), Some(Kind::Recovery));
        assert_eq!(encode(&recovery), "0123-4567-89AB-CDEF-GHJK-MNM7");
        // Changing the kind bit alone doesn't make a code of the other kind: the check covers it.
        for code in [device, recovery] {
            let mut flipped = code;
            flipped[LEN - 1] ^= 1;
            assert_eq!(kind(&flipped), None);
        }
        for kind_made in [Kind::Device, Kind::Recovery] {
            for _ in 0..50 {
                assert_eq!(kind(&new(kind_made).unwrap()), Some(kind_made));
            }
        }
        // One wrong character anywhere before the tag.
        let shown = encode(&device);
        let mut caught = 0;
        for position in (0..shown.len()).filter(|i| shown.as_bytes()[*i] != b'-').take(22) {
            let mut typed = shown.clone().into_bytes();
            typed[position] = if typed[position] == b'7' { b'8' } else { b'7' };
            let typed = decode(std::str::from_utf8(&typed).unwrap()).unwrap();
            caught += usize::from(kind(&typed).is_none());
        }
        assert!(caught >= 20, "{caught} of 22 typos caught");
    }

    #[test]
    fn what_was_scanned_or_typed_is_sorted_before_anything_is_tried() {
        let device = fixed(Kind::Device);
        let recovery = fixed(Kind::Recovery);
        assert_eq!(read("urbudget://pair/1/0123456789ABCDEFGHJKMNG8"), Ok(Entry::Device(device)));
        assert_eq!(read(" urbudget://ask/1/0123456789ABCDEFGHJKMNG8\n"), Ok(Entry::Request(device)));
        // However the phone hands the link on, and as codes were written before they were links.
        assert_eq!(read("URBUDGET://PAIR/1/0123456789abcdefghjkmng8"), Ok(Entry::Device(device)));
        assert_eq!(read("URBUDGET-PAIR:1:0123456789ABCDEFGHJKMNG8"), Ok(Entry::Device(device)));
        assert_eq!(read("URBUDGET-ASK:1:0123456789ABCDEFGHJKMNG8"), Ok(Entry::Request(device)));
        assert!(scanned_offer("urbudget://pair/1/0123456789ABCDEFGHJKMNG8") && scanned_offer(" URBUDGET-PAIR:1:0123"));
        assert!(!scanned_offer("urbudget://ask/1/0123456789ABCDEFGHJKMNG8") && !scanned_offer("0123-4567-89AB-CDEF-GHJK-MNG8"));
        assert_eq!(read("0123-4567-89ab-cdef-ghjk-mng8"), Ok(Entry::Device(device)));
        assert_eq!(read("0123 4567 89AB CDEF GHJK MNM7"), Ok(Entry::Recovery(recovery)));
        assert_eq!(read("0123-4567-89AB-CDEF-GHJK-MNG9"), Ok(Entry::Mistyped));
        // A passphrase is whatever isn't 24 code characters.
        assert_eq!(read("correct horse battery staple"), Ok(Entry::Other));
        assert_eq!(read("hunter2!"), Ok(Entry::Other));
        // A scanned code is whole or refused: a recovery code is never in a QR code.
        assert!(read("URBUDGET-PAIR:1:0123456789ABCDEFGHJKMNM7").is_err());
        assert!(read("URBUDGET-PAIR:1:0123456789ABCDEFGHJKMNG9").is_err());
        assert!(read("URBUDGET-PAIR:1:0123").is_err());
        // A later version of ours this one doesn't know.
        assert!(read("URBUDGET-PAIR:2:0123456789ABCDEFGHJKMNG8").unwrap_err().contains("newer"));
        assert!(read("urbudget://pair/2/0123456789ABCDEFGHJKMNG8").unwrap_err().contains("newer"));
        // Some other QR code is just text that opens nothing.
        assert_eq!(read("https://example.com/0123456789ABCDEFGHJKMNG8"), Ok(Entry::Other));
        // A link, so the phone's camera can open it, and of UrBudget's own kind, so a browser can't.
        for prefix in [OFFER, REQUEST] {
            assert!(prefix.starts_with("urbudget://"), "{prefix}");
        }
    }

    #[test]
    fn the_qr_code_is_small_enough_to_scan_from_a_screen() {
        let code = fixed(Kind::Device);
        for prefix in [OFFER, REQUEST] {
            let drawn = qr(prefix, &code).unwrap();
            let size = drawn["size"].as_u64().unwrap() as usize;
            let modules = drawn["modules"].as_str().unwrap();
            assert_eq!(modules.len(), size * size);
            assert!(modules.bytes().all(|b| b == b'0' || b == b'1'));
            // Version 3 or below: big squares at arm's length.
            assert!(size <= 29, "{size}");
        }
        assert_ne!(qr(OFFER, &code).unwrap(), qr(REQUEST, &code).unwrap());
    }
}
