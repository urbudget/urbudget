//! JSON adapter for JNI. The functions it calls are also invoked by desktop's Tauri adapter.
use super::*;
use serde::de::DeserializeOwned;

fn argument<T: DeserializeOwned>(params: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(params.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("Invalid assistant argument {name}: {e}"))
}
fn optional<T: DeserializeOwned + Default>(params: &Value, name: &str) -> Result<T, String> {
    match params.get(name) {
        Some(value) if !value.is_null() => serde_json::from_value(value.clone()).map_err(|e| format!("Invalid assistant argument {name}: {e}")),
        _ => Ok(T::default()),
    }
}
fn encoded<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

pub async fn dispatch(
    method: &str,
    params: Value,
    db: &Database,
    assistant: &Assistant,
    simplefin: &crate::simplefin::SimpleFin,
    cloud: &crate::cloud::Cloud,
) -> Result<Value, String> {
    match method {
        "assistant_sms_start" => sms::start(db, assistant, argument(&params, "request")?),
        "assistant_sms_poll" => sms::poll(db, assistant, &argument::<String>(&params, "id")?).await,
        "assistant_sms_cancel" => { sms::cancel(assistant, &argument::<String>(&params, "id")?); Ok(Value::Null) }
        "assistant_status" => encoded(assistant_status(assistant)),
        "assistant_foreground" => {
            assistant.foreground.store(argument(&params, "active")?, Ordering::Relaxed);
            Ok(Value::Null)
        }
        "assistant_sign_in" => {
            let package: Option<String> = optional(&params, "returnPackage")?;
            let link = package.as_deref().map(auth::android_return_link).transpose()?;
            encoded(sign_in_returning_to(assistant, link).await?)
        }
        "assistant_cancel_sign_in" => { assistant_cancel_sign_in(assistant); Ok(Value::Null) }
        "assistant_sign_out" => encoded(assistant_sign_out(assistant).await?),
        "assistant_settings" => encoded(assistant_settings(assistant, argument(&params, "change")?)?),
        "assistant_set_key" => encoded(assistant_set_key(assistant, argument(&params, "provider")?, argument(&params, "key")?).await?),
        "assistant_forget_key" => encoded(assistant_forget_key(assistant, argument(&params, "provider")?)?),
        "assistant_models" => encoded(assistant_models(assistant, optional(&params, "refresh")?).await?),
        "assistant_threads" => encoded(assistant_threads(db, assistant)?),
        "assistant_thread" => assistant_thread(db, assistant, argument(&params, "id")?),
        "assistant_delete_thread" => encoded(assistant_delete_thread(db, assistant, argument(&params, "id")?)?),
        "assistant_delete_all" => encoded(assistant_delete_all(db, assistant)?),
        "assistant_attach" => encoded(assistant_attach(db, assistant, argument(&params, "name")?, argument(&params, "data")?)?),
        "assistant_send" => encoded(assistant_send(simplefin, db, assistant, optional(&params, "patch")?, optional(&params, "threadId")?, argument(&params, "text")?, optional(&params, "attachments")?, argument(&params, "screen")?)?),
        "assistant_retry" => encoded(assistant_retry(simplefin, db, assistant, optional(&params, "patch")?, argument(&params, "threadId")?, argument(&params, "screen")?)?),
        "assistant_stop" => encoded(assistant_stop(db, assistant, argument(&params, "threadId")?)?),
        "assistant_apply" => encoded(assistant_apply(db, assistant, optional(&params, "patch")?, argument(&params, "threadId")?, argument(&params, "proposalId")?)?),
        "assistant_revise" => assistant_revise(db, assistant, optional(&params, "patch")?, argument(&params, "threadId")?, argument(&params, "proposalId")?, argument(&params, "revision")?),
        "assistant_mark" => assistant_mark(db, assistant, argument(&params, "threadId")?, argument(&params, "proposalId")?, argument(&params, "status")?),
        "assistant_report" => encoded(assistant_report(db, assistant, argument(&params, "threadId")?, argument(&params, "proposalId")?)?),
        "assistant_report_reply" => encoded(assistant_report_reply(cloud, argument(&params, "text")?, argument(&params, "reason")?).await?),
        "assistant_import" => encoded(assistant_import(db, assistant, argument(&params, "threadId")?, argument(&params, "proposalId")?)?),
        "assistant_save_import" => encoded(assistant_save_import(db, assistant, argument(&params, "threadId")?, argument(&params, "proposalId")?)?),
        "assistant_spreadsheet" => encoded(assistant_spreadsheet(db, assistant, optional(&params, "patch")?, argument(&params, "threadId")?, argument(&params, "proposalId")?, optional(&params, "open")?)?),
        "assistant_open" => encoded(assistant_open(argument(&params, "link")?)?),
        "assistant_log_error" => encoded(assistant_log_error(assistant, argument(&params, "message")?)?),
        _ => Err(format!("Unknown assistant command: {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_required_arguments_are_rejected() {
        assert!(argument::<Provider>(&json!({"provider":"unsupported"}), "provider").is_err());
        assert!(argument::<String>(&json!({}), "text").is_err());
        assert!(optional::<Vec<String>>(&json!({"attachments":42}), "attachments").is_err());
        assert_eq!(optional::<Vec<String>>(&json!({}), "attachments").unwrap(), Vec::<String>::new());
    }
}
