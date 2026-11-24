//! UI effects are handled by each host. Android drains these events and uses native intents;
//! desktop installs a listener that forwards them to Tauri's event bus.
use crate::t;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

type Listener = Arc<dyn Fn(&str, &Value) + Send + Sync>;
static LISTENER: OnceLock<Listener> = OnceLock::new();
static EVENTS: Mutex<Vec<Value>> = Mutex::new(Vec::new());

pub fn set_listener(listener: impl Fn(&str, &Value) + Send + Sync + 'static) {
    let _ = LISTENER.set(Arc::new(listener));
}
pub fn emit(name: &str, payload: Value) {
    if let Some(listener) = LISTENER.get() { listener(name, &payload); }
    else {
        let mut events = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
        // Bound unconsumed UI notifications while the activity is stopped. Assistant history is
        // durably saved separately; clients refresh it when returning to the foreground.
        if events.len() >= 2048 { events.remove(0); }
        events.push(json!({"event":name,"payload":payload}));
    }
}
pub fn drain() -> Vec<Value> {
    std::mem::take(&mut *EVENTS.lock().unwrap_or_else(|e| e.into_inner()))
}
pub fn open_browser(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| t!("Invalid browser address."))?;
    if !matches!(parsed.scheme(), "https" | "http") { return Err(t!("Only web addresses can be opened.")); }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    { emit("host://open-url", json!({"url":url})); Ok(()) }
    #[cfg(target_os = "windows")]
    { launch(url).map_err(|_| t!("Couldn't open your browser.")) }
    #[cfg(not(any(target_os = "windows", target_os = "android", target_os = "ios")))]
    { launch(url) }
}

/// Where feedback and questions go.
pub const SUPPORT: &str = "support@urbudget.app";

/// A new email to support with `subject` filled in, as an address the system opens in the mail app.
pub fn mail_address(subject: &str) -> String {
    let mut address = reqwest::Url::parse(&format!("mailto:{SUPPORT}")).expect("the support address");
    // A query's own encoding: spaces as %20, which a mail app reads back as spaces ("+" it would show as typed).
    address.set_query(Some(&format!("subject={subject}")));
    address.into()
}

/// Starts an email to support in the computer's mail app. The phones start theirs themselves.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn open_mail(subject: &str) -> Result<(), String> {
    launch(&mail_address(subject))
}

/// Hands an address to the system, which opens it in the app set for its kind (the browser, the mail app).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn launch(address: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let action: Vec<u16> = std::ffi::OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let wide: Vec<u16> = std::ffi::OsStr::new(address).encode_wide().chain(Some(0)).collect();
        let result = unsafe { ShellExecuteW(std::ptr::null_mut(), action.as_ptr(), wide.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
        if result as isize <= 32 { return Err(String::new()); }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    { std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(address).spawn().map(|_| ()).map_err(|e| e.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_is_an_email_to_support_with_its_subject_readable_by_a_mail_app() {
        assert_eq!(mail_address("UrBudget feedback (Windows 1.0.14)"), "mailto:support@urbudget.app?subject=UrBudget%20feedback%20(Windows%201.0.14)");
    }
}
pub fn open_file(path: &Path) -> Result<(), String> {
    if !path.is_file() { return Err(t!("That file no longer exists.")); }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    { emit("host://open-file", json!({"path":path.to_string_lossy()})); Ok(()) }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let action: Vec<u16> = std::ffi::OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let result = unsafe { ShellExecuteW(std::ptr::null_mut(), action.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
        if result as isize <= 32 { return Err(t!("Couldn't open that file.")); }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "android", target_os = "ios")))]
    { std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(path).spawn().map(|_| ()).map_err(|e| e.to_string()) }
}
