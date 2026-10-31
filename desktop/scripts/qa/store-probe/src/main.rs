//! A budget-free Store diagnostic. Run through Invoke-CommandInDesktopPackage so Windows
//! supplies the installed app's identity. Default: query only. --download-only never installs.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod probe {
    use serde_json::{json, Value};
    use std::{fs::File, io::Write, sync::{Arc, Mutex}, time::{Duration, Instant, SystemTime}};
    use windows::{
        core::{w, Interface, Result},
        ApplicationModel::Package,
        Services::Store::{StoreContext, StorePackageUpdate, StorePackageUpdateResult, StorePackageUpdateStatus},
        Win32::{
            Foundation::E_ABORT,
            System::WinRT::{RoInitialize, RO_INIT_SINGLETHREADED},
            UI::{Shell::IInitializeWithWindow, WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW, TranslateMessage,
                MSG, PM_REMOVE, WINDOW_EX_STYLE, WS_OVERLAPPED,
            }},
        },
    };
    use windows_future::{AsyncOperationProgressHandler, AsyncOperationWithProgressCompletedHandler, AsyncStatus, IAsyncOperationWithProgress};

    type Log = Arc<Mutex<File>>;
    fn log(out: &Log, mut event: Value) {
        event["unix_ms"] = json!(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis());
        let mut file = out.lock().unwrap();
        writeln!(file, "{event}").unwrap();
        file.flush().unwrap();
    }
    fn pump() {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Diagnostic failures must not silently abandon a live Store download. Cancellation is
    // cooperative: record if Windows still hasn't acknowledged it before this process exits.
    struct DownloadCleanup {
        operation: IAsyncOperationWithProgress<StorePackageUpdateResult, StorePackageUpdateStatus>,
        out: Log,
    }
    impl Drop for DownloadCleanup {
        fn drop(&mut self) {
            if self.operation.Status().map(|s| s != AsyncStatus::Started).unwrap_or(false) { return; }
            let canceled = self.operation.Cancel();
            log(&self.out, json!({"event":"cleanup_cancel", "result":format!("{canceled:?}")}));
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.operation.Status().map(|s| s == AsyncStatus::Started).unwrap_or(false)
                && Instant::now() < deadline { pump(); }
            log(&self.out, json!({"event":"cleanup_status", "result":format!("{:?}", self.operation.Status())}));
        }
    }
    fn wait<T>(status: impl Fn() -> Result<AsyncStatus>, result: impl Fn() -> Result<T>) -> Result<T> {
        let start = Instant::now();
        while status()? == AsyncStatus::Started {
            if start.elapsed() > Duration::from_secs(30) {
                return Err(windows::core::Error::new(E_ABORT, "Diagnostic query timed out"));
            }
            pump();
        }
        result()
    }
    fn queue(context: &StoreContext, out: &Log, phase: &str) -> Result<()> {
        let operation = context.GetAssociatedStoreQueueItemsAsync()?;
        let items = wait(|| operation.Status(), || operation.GetResults())?;
        let family = Package::Current()?.Id()?.FamilyName()?;
        let mut matched = 0;
        for i in 0..items.Size()? {
            let item = items.GetAt(i)?;
            // Debug package tokens can expose unrelated queue items. Never treat those as
            // this app's update or record other installed applications in this diagnostic.
            if item.PackageFamilyName()? != family { continue; }
            matched += 1;
            let status = item.GetCurrentStatus()?;
            let progress = status.UpdateStatus()?;
            log(out, json!({"event":"queue", "phase":phase,
                "product":item.ProductId()?.to_string(), "family":item.PackageFamilyName()?.to_string(),
                "state":status.PackageInstallState()?.0, "extended_state":status.PackageInstallExtendedState()?.0,
                "error":format!("{:08x}",status.ExtendedError()?.0),
                "package_state":progress.PackageUpdateState.0, "bytes":progress.PackageBytesDownloaded,
                "size":progress.PackageDownloadSizeInBytes, "progress":progress.TotalDownloadProgress}));
        }
        log(out, json!({"event":"queue_count", "phase":phase, "count":matched}));
        Ok(())
    }
    fn run(out: &Log, download: bool, drop_context: bool) -> Result<()> {
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED)?; }
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE::default(), w!("STATIC"), w!("UrBudget Store diagnostic"), WS_OVERLAPPED, 0, 0, 100, 100, None, None, None, None)? };
        let context = StoreContext::GetDefault()?;
        let owner: IInitializeWithWindow = context.cast()?;
        unsafe { owner.Initialize(hwnd)?; }
        drop(owner);
        log(out, json!({"event":"identity", "package":Package::Current()?.Id()?.FullName()?.to_string(), "can_silently_download":context.CanSilentlyDownloadStorePackageUpdates()?}));
        queue(&context, out, "before")?;
        let check = context.GetAppAndOptionalStorePackageUpdatesAsync()?;
        let found = wait(|| check.Status(), || check.GetResults())?;
        log(out, json!({"event":"updates", "count":found.Size()?}));
        if !download || found.Size()? == 0 { unsafe { let _ = DestroyWindow(hwnd); } return Ok(()); }
        if !context.CanSilentlyDownloadStorePackageUpdates()? {
            log(out,json!({"event":"consent_required"}));
            unsafe { let _ = DestroyWindow(hwnd); }
            return Ok(());
        }
        let updates = (0..found.Size()?).map(|i| found.GetAt(i).map(Some)).collect::<Result<Vec<Option<StorePackageUpdate>>>>()?;
        let iterable: windows_collections::IIterable<StorePackageUpdate> = updates.into();
        let operation = context.TrySilentDownloadStorePackageUpdatesAsync(&iterable)?;
        let _cleanup = DownloadCleanup { operation: operation.clone(), out: out.clone() };
        let progress_out = out.clone();
        operation.SetProgress(&AsyncOperationProgressHandler::<StorePackageUpdateResult, StorePackageUpdateStatus>::new(move |_, status| {
            log(&progress_out, json!({"event":"progress", "package_state":status.PackageUpdateState.0,
                "family":status.PackageFamilyName.to_string(), "bytes":status.PackageBytesDownloaded,
                "size":status.PackageDownloadSizeInBytes, "progress":status.TotalDownloadProgress}));
            Ok(())
        }))?;
        let complete_out = out.clone();
        operation.SetCompleted(&AsyncOperationWithProgressCompletedHandler::new(move |_, status| {
            log(&complete_out,json!({"event":"completed_callback", "status":status.0}));
            Ok(())
        }))?;
        let context = if drop_context { drop(context); None } else { Some(context) };
        log(out,json!({"event":"download_started", "context_retained":!drop_context}));
        let started = Instant::now();
        let mut next_report = Duration::ZERO;
        while operation.Status()? == AsyncStatus::Started && started.elapsed() < Duration::from_secs(150) {
            if started.elapsed() >= next_report {
                log(out,json!({"event":"poll", "elapsed_seconds":started.elapsed().as_secs(), "status":operation.Status()?.0}));
                if let Some(context) = &context { queue(context, out, "during")?; }
                next_report += Duration::from_secs(10);
            }
            pump();
        }
        let status = operation.Status()?;
        log(out,json!({"event":"final_status", "status":status.0}));
        if status == AsyncStatus::Started {
            operation.Cancel()?;
            let deadline = Instant::now() + Duration::from_secs(10);
            while operation.Status()? == AsyncStatus::Started && Instant::now() < deadline { pump(); }
            log(out,json!({"event":"cancel_status", "status":operation.Status()?.0}));
        } else {
            let result = operation.GetResults()?;
            log(out,json!({"event":"result", "overall_state":result.OverallState()?.0}));
        }
        if let Some(context) = &context { queue(context, out, "after")?; }
        unsafe { let _ = DestroyWindow(hwnd); }
        Ok(())
    }
    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        let Some(path) = args.get(1) else { return; };
        let out = Arc::new(Mutex::new(File::create(path).expect("diagnostic output file")));
        log(&out,json!({"event":"start", "pid":std::process::id()}));
        match run(&out, args.iter().any(|arg| arg == "--download-only"), args.iter().any(|arg| arg == "--drop-context")) {
            Ok(()) => log(&out,json!({"event":"done"})),
            Err(error) => log(&out,json!({"event":"error", "message":error.to_string(), "code":format!("{:08x}",error.code().0)})),
        }
    }
}

fn main() {
    #[cfg(windows)]
    probe::main();
}
