//! Exercises the production Store commands with a hidden, isolated Tauri window and no budget.
//! Run using Invoke-CommandInDesktopPackage; the package identity is supplied by Windows.
//! Arguments: output.jsonl [--download-only]. Neither mode installs an update.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[allow(dead_code)]
mod cloud {
    pub fn site() -> &'static str { "https://urbudget.app" }
}
#[allow(dead_code)]
#[path = "../src/updates.rs"]
mod updates;

use std::{fs::File, io::Write, sync::{Arc, Mutex}, time::Instant};
use serde_json::{json, Value};

type Log = Arc<Mutex<File>>;
fn log(out: &Log, event: Value) {
    let mut file = out.lock().unwrap();
    writeln!(file, "{event}").unwrap();
    file.flush().unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(output) = args.get(1).map(std::path::PathBuf::from) else { return; };
    let out = Arc::new(Mutex::new(File::create(&output).expect("probe output")));
    let profile = output.parent().expect("output directory").join(format!(
        "{}-webview-{}", output.file_stem().unwrap().to_string_lossy(), std::process::id(),
    ));
    let download = args.iter().any(|arg| arg == "--download-only");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let final_out = out.clone();
    let result = tauri::Builder::default().setup(move |app| {
        let window = tauri::WebviewWindowBuilder::new(app, "store-probe", tauri::WebviewUrl::External("about:blank".parse().unwrap()))
            .title("UrBudget Store test")
            .visible(false)
            .data_directory(profile)
            .build()?;
        let handle = app.handle().clone();
        let out = out.clone();
        tauri::async_runtime::spawn(async move {
            let start = Instant::now();
            log(&out, json!({"event":"start", "pid":std::process::id()}));
            for iteration in 0..5 {
                let result = updates::check_store_update(window.clone()).await;
                log(&out, json!({"event":"sequential_check", "iteration":iteration, "result":result, "elapsed_ms":start.elapsed().as_millis()}));
            }
            let mut checks = Vec::new();
            for _ in 0..10 {
                let window = window.clone();
                checks.push(tauri::async_runtime::spawn(async move { updates::check_store_update(window).await }));
            }
            for (iteration, check) in checks.into_iter().enumerate() {
                log(&out, json!({"event":"concurrent_check", "iteration":iteration, "result":check.await.unwrap(), "elapsed_ms":start.elapsed().as_millis()}));
            }
            if download {
                let progress_out = out.clone();
                let progress = tauri::ipc::Channel::new(move |body| {
                    log(&progress_out,json!({"event":"progress", "body":format!("{body:?}")}));
                    Ok(())
                });
                log(&out,json!({"event":"download_result", "result":updates::download_store_update(window.clone(), progress).await}));
            }
            log(&out,json!({"event":"done", "elapsed_ms":start.elapsed().as_millis()}));
            // Only this probe's hidden window/process exists in this application instance.
            let _ = window.destroy();
            handle.exit(0);
        });
        Ok(())
    }).run(context);
    if let Err(error) = result { log(&final_out,json!({"event":"error", "message":error.to_string()})); }
}
