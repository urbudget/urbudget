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
static INITIALIZE: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
        let _initializing = INITIALIZE.lock().map_err(|_| "Initialization was interrupted.")?;
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
        on_service_stack(move || urbudget_services::block_on(runtime.dispatch(&request.method, request.params)))
    })
    .unwrap_or_else(|_| Err(t!("The service could not complete this request.")));
    into_c(reply(result))
}

/// The stack a service call runs on. Swift calls in from its concurrency threads, whose stacks
/// are 512 KB. A sync pass nests its futures deeply: an unoptimized build overflows that stack
/// as soon as a pass has a budget to work on (the app dies with a bus error at the guard page),
/// and an optimized one has less to spare with everything the engine gains.
const SERVICE_STACK: usize = 8 * 1024 * 1024;

/// Runs `work` on a thread of its own with `SERVICE_STACK` to use, and waits for it, so the
/// caller sees the same blocking call. The thread takes the caller's urgency, or the system
/// would run a call the screen is waiting for behind background work.
fn on_service_stack<T: Send>(work: impl FnOnce() -> Result<T, String> + Send) -> Result<T, String> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("urbudget-service".into())
            .stack_size(SERVICE_STACK)
            .spawn_scoped(scope, || {
                raise_urgency();
                work()
            })
            .map_err(|e| e.to_string())?
            .join()
            .unwrap_or_else(|_| Err(t!("The service could not complete this request.")))
    })
}

#[cfg(target_vendor = "apple")]
fn raise_urgency() {
    extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    /// QOS_CLASS_USER_INITIATED, as Swift's calling threads have (<sys/qos.h>).
    const USER_INITIATED: u32 = 0x19;
    // SAFETY: sets this thread's own scheduling class; a refusal leaves it as it was.
    unsafe { pthread_set_qos_class_self_np(USER_INITIATED, 0) };
}
#[cfg(not(target_vendor = "apple"))]
fn raise_urgency() {}

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
