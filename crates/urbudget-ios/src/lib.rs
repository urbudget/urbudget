//! Native iOS transport. Swift calls the same versioned JSON requests as Android's JNI adapter,
//! over a C ABI: every function takes and returns a NUL-terminated UTF-8 JSON string, and every
//! returned string is released with `urbudget_free`. Financial decisions live in `urbudget-core`.
use serde::Deserialize;
use serde_json::{json, Value};
use std::ffi::{c_char, CStr, CString};
use std::sync::{Mutex, OnceLock};
use urbudget_services::t;

const API_VERSION: u32 = 1;
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
struct ServiceRequest {
    version: u32,
    method: String,
    #[serde(default = "empty_object")]
    params: Value,
}

fn empty_object() -> Value {
    json!({})
}

// The app owns one service runtime. Keeping its executor alive preserves OAuth callbacks,
// assistant jobs, sync keys and SQLite revision checks between individual calls.
static SERVICES: OnceLock<urbudget_services::runtime::Runtime> = OnceLock::new();
static INITIALIZING: Mutex<()> = Mutex::new(());

fn reply(result: Result<Value, String>) -> String {
    match result {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(error) => json!({"ok":false,"error":error}),
    }
    .to_string()
}

/// JSON never contains a raw NUL (serde escapes it), so the conversion only fails on a bug.
fn into_c(text: String) -> *mut c_char {
    CString::new(text)
        .unwrap_or_else(|_| CString::from(c"{\"ok\":false,\"error\":\"The engine returned an unreadable reply.\"}"))
        .into_raw()
}

/// # Safety
/// `input` is null or a NUL-terminated string that stays valid for the call.
unsafe fn read(input: *const c_char) -> Result<String, String> {
    if input.is_null() {
        return Err("Missing request".into());
    }
    let bytes = CStr::from_ptr(input).to_bytes();
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err("Request exceeds 64 MiB".into());
    }
    std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| "The request is not UTF-8.".into())
}

/// Pure engine calls (calculate, workflows, imports). Panics and invalid input return structured
/// errors; unwinding never crosses into Swift and no request data is logged.
///
/// # Safety
/// `request` is null or a NUL-terminated UTF-8 string that stays valid for the call.
#[no_mangle]
pub unsafe extern "C" fn urbudget_invoke(request: *const c_char) -> *mut c_char {
    let response = std::panic::catch_unwind(|| match read(request) {
        Ok(input) => urbudget_android::invoke_json(&input),
        Err(error) => reply(Err(error)),
    })
    .unwrap_or_else(|_| reply(Err(t!("The engine could not complete this request"))));
    into_c(response)
}

/// Opens the budget library and secure vault in `directory`. Calling it again with the same
/// directory is harmless; another directory is refused while the app runs.
///
/// # Safety
/// `directory` is null or a NUL-terminated UTF-8 path that stays valid for the call.
#[no_mangle]
pub unsafe extern "C" fn urbudget_initialize(directory: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        let path = std::path::PathBuf::from(read(directory)?);
        let _guard = INITIALIZING.lock().map_err(|_| "Initialization failed. Restart the app.")?;
        if let Some(runtime) = SERVICES.get() {
            if runtime.data_dir != path {
                return Err("The service directory cannot change while the app is running.".into());
            }
        } else {
            std::fs::create_dir_all(&path).map_err(|e| format!("Unable to create the budget folder: {e}"))?;
            let runtime = urbudget_services::runtime::Runtime::new(&path)?;
            SERVICES.set(runtime).map_err(|_| "The runtime was already initialized.")?;
        }
        Ok(Value::Null)
    })
    .unwrap_or_else(|_| Err("Unable to initialize secure budget storage.".into()));
    into_c(reply(result))
}

/// Storage, account, sync, bank and assistant services. This blocks until the service finishes,
/// so Swift calls it off the main thread.
///
/// # Safety
/// `request` is null or a NUL-terminated UTF-8 string that stays valid for the call.
#[no_mangle]
pub unsafe extern "C" fn urbudget_service(request: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(|| -> Result<Value, String> {
        let input = read(request)?;
        let request: ServiceRequest = serde_json::from_str(&input).map_err(|e| format!("Invalid service request: {e}"))?;
        if request.version != API_VERSION {
            return Err("Unsupported service version".into());
        }
        let runtime = SERVICES.get().ok_or("Initialize the native services first.")?;
        urbudget_services::block_on(runtime.dispatch(&request.method, request.params))
    })
    .unwrap_or_else(|_| Err(t!("The service could not complete this request.")));
    into_c(reply(result))
}

/// Releases a string returned by any `urbudget_` function.
///
/// # Safety
/// `value` is null or a pointer returned by this library that has not been freed yet.
#[no_mangle]
pub unsafe extern "C" fn urbudget_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests;
