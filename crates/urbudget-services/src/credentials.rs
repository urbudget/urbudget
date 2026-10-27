//! YNAB personal access tokens, remembered with the app's other secrets (vault.rs) when the
//! person chooses Remember. Several can be kept (one per YNAB account, say), each named by the
//! plans it reads. The window only ever sees those names and each token's last four characters;
//! the tokens themselves stay here. Where secrets can't be kept, a token is pasted for each import.
use crate::vault::{self, STORE};
use crate::{i18n::tr, t};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.localbudget.desktop.ynab.tokens";
/// The one token earlier versions kept, moved into the list the first time tokens are read.
const EARLIER: &str = "com.localbudget.desktop.ynab";
fn unsupported() -> String {
    t!("This system has no supported credential store. Paste a one-time token.")
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Saved {
    id: String,
    label: String,
    token: String,
    saved_at: String,
    #[serde(default)]
    last_used: Option<String>,
}
/// A saved token as the window shows it.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedToken {
    pub id: String,
    pub label: String,
    /// The token's last four characters, to tell two saved tokens apart.
    pub hint: String,
    pub saved_at: String,
    pub last_used: Option<String>,
}

/// Adds a token, or renames and touches the one already saved with the same value. Returns its id.
fn remember(saved: &mut Vec<Saved>, token: &str, label: &str, now: &str) -> String {
    // An unnamed token is named in the language in use when it's saved; after that the name is the person's.
    let label = match label.trim() {
        "" => tr("YNAB token"),
        named => named,
    };
    if let Some(s) = saved.iter_mut().find(|s| s.token == token) {
        s.label = label.into();
        s.last_used = Some(now.into());
        return s.id.clone();
    }
    // Ids only need to differ from each other; the time they were saved is enough.
    let mut id = format!("t{}", now.chars().filter(char::is_ascii_digit).collect::<String>());
    while saved.iter().any(|s| s.id == id) {
        id.push('x');
    }
    saved.push(Saved { id: id.clone(), label: label.into(), token: token.into(), saved_at: now.into(), last_used: Some(now.into()) });
    id
}
/// Most recently used first; never-used ones by when they were saved.
fn listed(saved: &[Saved]) -> Vec<SavedToken> {
    let mut list: Vec<SavedToken> = saved
        .iter()
        .map(|s| SavedToken {
            id: s.id.clone(),
            label: s.label.clone(),
            hint: s.token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect(),
            saved_at: s.saved_at.clone(),
            last_used: s.last_used.clone(),
        })
        .collect();
    list.sort_by(|a, b| {
        b.last_used.as_ref().unwrap_or(&b.saved_at).cmp(a.last_used.as_ref().unwrap_or(&a.saved_at))
    });
    list
}

fn load() -> Result<Vec<Saved>, String> {
    STORE.ok_or_else(unsupported)?;
    let mut saved: Vec<Saved> = match vault::read(SERVICE) {
        Some(json) => serde_json::from_str(&json).map_err(|_| t!("The YNAB tokens saved in {store} can't be read.", store = vault::store_name()))?,
        None => vec![],
    };
    if let Some(token) = vault::read(EARLIER) {
        remember(&mut saved, &token, tr("Saved token"), &crate::domain::stamp_now());
        keep(&saved)?;
        let _ = vault::delete(EARLIER);
    }
    Ok(saved)
}
fn keep(saved: &[Saved]) -> Result<(), String> {
    STORE.ok_or_else(unsupported)?;
    if saved.is_empty() {
        return vault::delete(SERVICE).map(|_| ());
    }
    let json = serde_json::to_string(saved).map_err(|e| e.to_string())?;
    vault::save(SERVICE, &json).map_err(|_| t!("{store} could not save the token.", store = vault::store_name()))
}

/// The saved token with this id, marked as just used.
pub fn read(id: &str) -> Result<String, String> {
    let mut saved = load()?;
    let s = saved
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| t!("That saved token is no longer here. Choose another, or paste one."))?;
    s.last_used = Some(crate::domain::stamp_now());
    let token = s.token.clone();
    keep(&saved)?;
    Ok(token)
}
/// Names a saved token for the plans it reads now (the one moved from earlier versions starts as
/// "Saved token").
pub fn relabel(id: &str, label: &str) -> Result<(), String> {
    let mut saved = load()?;
    let label = label.trim();
    let Some(s) = saved.iter_mut().find(|s| s.id == id) else { return Ok(()) };
    if label.is_empty() || s.label == label {
        return Ok(());
    }
    s.label = label.into();
    keep(&saved)
}
/// Saves a token under a name (the plans it reads); one already saved is renamed instead.
pub fn save(token: &str, label: &str) -> Result<String, String> {
    let mut saved = load()?;
    let id = remember(&mut saved, token, label, &crate::domain::stamp_now());
    keep(&saved)?;
    Ok(id)
}
pub fn ynab_tokens() -> Result<Vec<SavedToken>, String> {
    if STORE.is_none() {
        return Ok(vec![]);
    }
    Ok(listed(&load()?))
}
pub fn forget_ynab_token(id: String) -> Result<(), String> {
    let mut saved = load()?;
    let before = saved.len();
    saved.retain(|s| s.id != id);
    if saved.len() == before {
        return Err(t!("That token was already removed."));
    }
    keep(&saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_kept_once_each_and_listed_by_last_use_without_their_values() {
        let mut saved = vec![];
        let home = remember(&mut saved, "home-token-abcd", "Home", "2026-10-01T10:00:00Z");
        let work = remember(&mut saved, "work-token-wxyz", " ", "2026-10-02T10:00:00Z");
        assert_ne!(home, work);
        assert_eq!(saved[1].label, "YNAB token");
        // Saving the same token again renames it and moves it to the top.
        assert_eq!(remember(&mut saved, "home-token-abcd", "Home, Stress Test", "2026-10-03T10:00:00Z"), home);
        assert_eq!(saved.len(), 2);
        let list = listed(&saved);
        assert_eq!(list.iter().map(|t| (t.label.as_str(), t.hint.as_str())).collect::<Vec<_>>(), [("Home, Stress Test", "abcd"), ("YNAB token", "wxyz")]);
        assert!(!serde_json::to_string(&list).unwrap().contains("token-"));
        // Two saved in the same instant still get different ids.
        let mut twins = vec![];
        let a = remember(&mut twins, "a", "A", "2026-10-01T10:00:00Z");
        let b = remember(&mut twins, "b", "B", "2026-10-01T10:00:00Z");
        assert_ne!(a, b);
    }
}
