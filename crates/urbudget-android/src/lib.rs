//! Native Android transport. All financial decisions live in `urbudget-core`.
use serde::Deserialize;
use serde_json::{json, Value};
use urbudget_core::commands::{self, encode, optional, required};
use urbudget_core::{bank_csv, domain, loans, t, ynab, ynab_files};

const API_VERSION: u32 = 1;
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;

#[derive(Deserialize)]
struct Request {
    version: u32,
    method: String,
    budget: Option<Value>,
    #[serde(default = "empty_object")]
    params: Value,
}

fn empty_object() -> Value {
    json!({})
}
impl Request {
    fn budget(&self) -> Result<domain::Budget, String> {
        let budget: domain::Budget =
            serde_json::from_value(self.budget.clone().ok_or("Missing budget")?)
                .map_err(|error| format!("Invalid budget: {error}"))?;
        domain::validate(&budget)?;
        Ok(budget)
    }

    fn changed(&self, budget: domain::Budget) -> Result<Value, String> {
        domain::validate(&budget)?;
        self.encoded(budget)
    }

    /// The budget as this request's reply carries it.
    fn encoded(&self, budget: domain::Budget) -> Result<Value, String> {
        let mut value = encode(budget)?;
        // Assignments and schedules keep unknown fields themselves now. `applyPatch` replaces
        // rows whole, so what a client left out of one comes back from the row it replaced.
        if let Some(before) = self.budget.as_ref() {
            preserve_extensions(
                before,
                &mut value,
                "assignments",
                &["month", "categoryId"],
                &["month", "categoryId", "amount"],
            );
            preserve_extensions(
                before,
                &mut value,
                "schedules",
                &["id"],
                &[
                    "id",
                    "nextDate",
                    "anchorDate",
                    "frequency",
                    "paused",
                    "transaction",
                ],
            );
        }
        Ok(value)
    }
}

fn preserve_extensions(
    before: &Value,
    after: &mut Value,
    collection: &str,
    keys: &[&str],
    known: &[&str],
) {
    use std::collections::HashMap;
    let (Some(originals), Some(updated)) = (
        before.get(collection).and_then(Value::as_array),
        after.get_mut(collection).and_then(Value::as_array_mut),
    ) else {
        return;
    };
    let key = |row: &Value| -> Option<Vec<String>> {
        keys.iter()
            .map(|field| row.get(field)?.as_str().map(str::to_owned))
            .collect()
    };
    let originals: HashMap<_, _> = originals
        .iter()
        .filter_map(|row| Some((key(row)?, row.as_object()?)))
        .collect();
    for row in updated {
        if let Some(original) = key(row).and_then(|key| originals.get(&key)) {
            if let Some(object) = row.as_object_mut() {
                for (name, value) in *original {
                    if !known.contains(&name.as_str()) {
                        object.entry(name).or_insert_with(|| value.clone());
                    }
                }
            }
        }
    }
}

fn dispatch(request: Request) -> Result<Value, String> {
    if request.version != API_VERSION {
        return Err(format!(
            "Unsupported engine API version: {}",
            request.version
        ));
    }
    if !request.params.is_object() {
        return Err("params must be a JSON object".into());
    }
    let p = &request.params;
    match request.method.as_str() {
        "defaultBudget" => {
            let mut budget = domain::Budget::default();
            if let Some(name) = optional(p, "name")? {
                budget.name = name;
            }
            if let Some(currency) = optional(p, "currency")? {
                budget.currency = currency;
            }
            request.changed(budget)
        }
        "sampleBudget" => sample_budget(p).and_then(|budget| request.changed(budget)),
        "importYnab" => encode(ynab::import_export(&required::<Value>(p, "export")?)?),
        // A statement that isn't UTF-8, sent as ISO-8859-1 so its bytes cross unchanged.
        // With `sheet`, a workbook's sheet of that name.
        "decodeStatement" => encode(bank_csv::decode_latin1(&required::<String>(p, "latin1")?, optional::<String>(p, "sheet")?.as_deref())?),
        "importYnabFiles" => encode(ynab_files::import_files(
            &required::<Vec<ynab_files::ExportFile>>(p, "files")?,
            optional::<String>(p, "dateOrder")?.as_deref(),
            &optional::<String>(p, "currency")?.unwrap_or_else(|| "SAR".into()),
        )?),
        "loanProjection" => encode(loans::project_with(
            required(p, "balance")?,
            &required(p, "terms")?,
            &required::<String>(p, "start")?,
            optional(p, "extra")?.unwrap_or(0),
            optional(p, "lump")?.unwrap_or(0),
        )?),
        "loanPaymentByDate" => encode(loans::payment_by_date_with(
            required(p, "balance")?,
            &required(p, "terms")?,
            &required::<String>(p, "start")?,
            &required::<String>(p, "due")?,
            optional(p, "lump")?.unwrap_or(0),
        )?),
        _ => {
            let budget = request.budget()?;
            let method = request.method.as_str();
            if !commands::changes_budget(method) {
                return commands::read(&budget, method, p);
            }
            let outcome = commands::apply(budget, method, p)?;
            let summary = outcome.preview(p)?;
            let count = outcome.count;
            let value = request.encoded(outcome.budget)?;
            Ok(match (summary, count) {
                (Some(summary), _) => json!({"budget":value,"summary":summary}),
                (_, Some(count)) => json!({"budget":value,"count":count}),
                _ => value,
            })
        }
    }
}

/// A new budget to look around in. Its names become the person's data, so they're written in
/// the language in use when it's made (the ids stay the same in every language).
fn sample_budget(params: &Value) -> Result<domain::Budget, String> {
    let month: String = required(params, "month")?;
    if !domain::valid_month(&month) {
        return Err(t!("Invalid sample budget month"));
    }
    let name = optional::<String>(params, "name")?.unwrap_or_else(|| t!("Sample budget"));
    let currency = optional::<String>(params, "currency")?.unwrap_or_else(|| "SAR".into());
    let everyday = t!("Everyday");
    serde_json::from_value(json!({
        "schemaVersion": 2, "name": name, "currency": currency,
        "accounts": [{"id":"sample-checking","name":t!("Checking"),"kind":"cash","openingBalance":500000,"archived":false}],
        "categories": [
            {"id":"sample-groceries","name":t!("Groceries"),"group":everyday,"target":60000},
            {"id":"sample-transport","name":t!("Transport"),"group":everyday,"target":20000},
            {"id":"sample-emergency","name":t!("Emergency fund"),"group":t!("Savings"),"target":100000}
        ],
        "assignments": [
            {"month":month,"categoryId":"sample-groceries","amount":60000},
            {"month":month,"categoryId":"sample-transport","amount":20000},
            {"month":month,"categoryId":"sample-emergency","amount":100000}
        ],
        "transactions": [{"id":"sample-market","date":format!("{month}-01"),"accountId":"sample-checking","payee":t!("Neighborhood market"),"memo":t!("Sample purchase"),"amount":-12500,"status":"cleared","splits":[{"categoryId":"sample-groceries","amount":-12500}],"transferAccountId":null,"transferStatus":null}]
    })).map_err(|error| error.to_string())
}

/// Stable transport boundary. Invalid JSON, validation failures, and panics return
/// structured errors; unwinding never crosses JNI and no request data is logged.
pub fn invoke_json(input: &str) -> String {
    let result = std::panic::catch_unwind(|| {
        if input.len() > MAX_REQUEST_BYTES {
            return Err("Engine request exceeds 64 MiB".into());
        }
        let request = serde_json::from_str(input)
            .map_err(|error| format!("Invalid engine request: {error}"))?;
        dispatch(request)
    })
    .unwrap_or_else(|_| Err(t!("The engine could not complete this request")));
    match result {
        Ok(value) => json!({"ok":true,"value":value}).to_string(),
        Err(error) => json!({"ok":false,"error":error}).to_string(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_urbudget_nativeapp_NativeBridge_invoke(
    mut env: jni::JNIEnv,
    _receiver: jni::objects::JObject,
    request: jni::objects::JString,
) -> jni::sys::jstring {
    let response = match env.get_string(&request) {
        Ok(request) => invoke_json(&String::from(request)),
        Err(_) => json!({"ok":false,"error":"Unable to read engine request"}).to_string(),
    };
    match env.new_string(response) {
        Ok(value) => value.into_raw(),
        Err(_) => {
            // JNI retains its pending allocation exception. Never return a
            // fabricated successful response after an allocation failure.
            std::ptr::null_mut()
        }
    }
}

// The tests compare replies with what these give directly.
#[cfg(test)]
use {serde::Serialize, urbudget_core::{engine, ledger, reports}};
#[cfg(test)]
mod tests;

// The native app owns one shared service runtime. Keeping its executor alive preserves OAuth
// callbacks, assistant jobs, sync keys and SQLite revision checks between individual JNI calls.
#[cfg(target_os = "android")]
static SERVICES: std::sync::OnceLock<urbudget_services::runtime::Runtime> = std::sync::OnceLock::new();
#[cfg(target_os = "android")]
static INITIALIZING: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "android")]
fn native_result(result: Result<Value, String>) -> String {
    match result { Ok(value) => json!({"ok":true,"value":value}), Err(error) => json!({"ok":false,"error":error}) }.to_string()
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_urbudget_nativeapp_NativeBridge_initialize(
    mut env: jni::JNIEnv, _receiver: jni::objects::JObject,
    context: jni::objects::JObject, directory: jni::objects::JString,
) -> jni::sys::jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value, String> {
        let path = std::path::PathBuf::from(String::from(env.get_string(&directory).map_err(|e| e.to_string())?));
        let _guard = INITIALIZING.lock().map_err(|_| "Initialization failed. Restart the app.")?;
        if let Some(runtime) = SERVICES.get() {
            if runtime.data_dir != path { return Err("The service directory cannot change while the app is running.".into()); }
        } else {
            urbudget_services::keystore::init(&mut env, &context)?;
            let runtime = urbudget_services::runtime::Runtime::new(&path)?;
            SERVICES.set(runtime).map_err(|_| "The runtime was already initialized.")?;
        }
        Ok(Value::Null)
    })).unwrap_or_else(|_| Err("Unable to initialize secure budget storage.".into()));
    env.new_string(native_result(result)).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_urbudget_nativeapp_NativeBridge_service(
    mut env: jni::JNIEnv, _receiver: jni::objects::JObject, request: jni::objects::JString,
) -> jni::sys::jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value, String> {
        let input = String::from(env.get_string(&request).map_err(|e| e.to_string())?);
        if input.len() > MAX_REQUEST_BYTES { return Err("Service request exceeds 64 MiB".into()); }
        let request: Request = serde_json::from_str(&input).map_err(|e| format!("Invalid service request: {e}"))?;
        if request.version != API_VERSION { return Err("Unsupported service version".into()); }
        let runtime = SERVICES.get().ok_or("Initialize the native services first.")?;
        urbudget_services::block_on(runtime.dispatch(&request.method, request.params))
    })).unwrap_or_else(|_| Err(t!("The service could not complete this request.")));
    env.new_string(native_result(result)).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}
