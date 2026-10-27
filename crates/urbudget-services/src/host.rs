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
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::UI::Shell::ShellExecuteW;
            let action: Vec<u16> = std::ffi::OsStr::new("open").encode_wide().chain(Some(0)).collect();
            let address: Vec<u16> = std::ffi::OsStr::new(url).encode_wide().chain(Some(0)).collect();
            let result = unsafe { ShellExecuteW(std::ptr::null_mut(), action.as_ptr(), address.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
            if result as isize <= 32 { return Err(t!("Couldn't open your browser.")); }
            Ok(())
        }
        #[cfg(not(target_os = "windows"))]
        { std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(url).spawn().map(|_| ()).map_err(|e| e.to_string()) }
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
