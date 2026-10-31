//! Standalone copies use signed Tauri updates; MSIX copies use Microsoft Store APIs.
//! The frontend saves and locks the budget before an installer can close the app.
use tauri::AppHandle;

#[derive(Clone, serde::Serialize)]
pub struct StoreInstallStatus {
    stage: &'static str,
    progress: Option<f64>,
}

/// Debug builds look for updates wherever `URBUDGET_API` points, such as `wrangler dev`.
#[cfg(debug_assertions)]
pub fn use_local_server<R: tauri::Runtime>(mut context: tauri::Context<R>) -> tauri::Context<R> {
    if std::env::var_os("URBUDGET_API").is_none() {
        return context;
    }
    let site = crate::cloud::site();
    let endpoints = context
        .config_mut()
        .plugins
        .0
        .get_mut("updater")
        .and_then(|updater| updater.get_mut("endpoints"))
        .and_then(|endpoints| endpoints.as_array_mut());
    for endpoint in endpoints.into_iter().flatten() {
        if let Some(url) = endpoint.as_str() {
            *endpoint = url.replacen("https://urbudget.app", &site, 1).into();
        }
    }
    context
}

/// Packaged Windows copies must use Store updates, never the standalone installer.
/// An unknown package identity is an error rather than permission to switch channels.
/// Debug builds (`tauri dev`, the QA window) are `"development"` and don't update, so a published
/// version never installs itself over the copy being worked on; with URBUDGET_API they look for
/// updates on that local server, to try updating out.
#[tauri::command]
pub fn update_channel() -> Result<&'static str, String> {
    if cfg!(debug_assertions) && std::env::var_os("URBUDGET_API").is_none() {
        return Ok("development");
    }
    if packaged()? {
        Ok("microsoftStore")
    } else {
        Ok("direct")
    }
}

#[cfg(windows)]
fn packaged() -> Result<bool, String> {
    use windows_sys::Win32::Foundation::{
        APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
    };
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut length = 0;
    match unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) } {
        APPMODEL_ERROR_NO_PACKAGE => Ok(false),
        ERROR_INSUFFICIENT_BUFFER | ERROR_SUCCESS => Ok(true),
        code => Err(urbudget_services::t!(
            "Couldn't identify this app's update channel (Windows error {code}).",
            code
        )),
    }
}
#[cfg(not(windows))]
fn packaged() -> Result<bool, String> {
    Ok(false)
}

/// Counts this copy as in use on urbudget.app, with nothing that identifies it, at most once a day
/// (urbudget_services::usage). The window asks only while the Privacy & data switch is on. Debug
/// builds count only on a local server (URBUDGET_API), never on urbudget.app.
#[tauri::command(async)]
pub async fn report_usage(
    app: AppHandle,
    usage: tauri::State<'_, urbudget_services::usage::Usage>,
) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if std::env::var_os("URBUDGET_API").is_none() {
        return Ok(());
    }
    let channel = if packaged()? { "store" } else { "direct" };
    let version = app.package_info().version.to_string();
    usage
        .report(
            &crate::cloud::site(),
            std::env::consts::OS,
            channel,
            &version,
        )
        .await
}

#[tauri::command]
pub async fn check_store_update(window: tauri::WebviewWindow) -> Result<bool, String> {
    #[cfg(windows)]
    {
        microsoft::check(window).await
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err(
            urbudget_services::i18n::tr("Microsoft Store updates are only available on Windows.")
                .into(),
        )
    }
}

#[tauri::command]
pub async fn install_store_update(
    window: tauri::WebviewWindow,
    on_status: Option<tauri::ipc::JavaScriptChannelId>,
) -> Result<bool, String> {
    #[cfg(windows)]
    {
        let on_status = on_status.map(|id| id.channel_on(window.as_ref().clone()));
        microsoft::install(window, on_status).await
    }
    #[cfg(not(windows))]
    {
        let _ = (window, on_status);
        Err(
            urbudget_services::i18n::tr("Microsoft Store updates are only available on Windows.")
                .into(),
        )
    }
}

/// Downloads a Store update in the background: `"ready"` when Windows will also install it without
/// asking, `"consent"` when it needs the person's go-ahead (Store auto-updates off, a metered
/// connection), `"none"` when there's nothing to download.
#[tauri::command]
pub async fn download_store_update(
    window: tauri::WebviewWindow,
    on_progress: tauri::ipc::Channel<f64>,
) -> Result<&'static str, String> {
    #[cfg(windows)]
    {
        microsoft::download(window, on_progress).await
    }
    #[cfg(not(windows))]
    {
        let _ = (window, on_progress);
        Err(
            urbudget_services::i18n::tr("Microsoft Store updates are only available on Windows.")
                .into(),
        )
    }
}

/// Installs a downloaded Store update without asking. Windows closes UrBudget to replace it and,
/// with `relaunch`, opens it again afterwards. `"installed"`, `"none"` when there was nothing to
/// install, or `"consent"` when Windows won't now without the person's go-ahead (they turned the
/// Store's automatic updates off, or the connection is metered): `install_store_update` asks.
#[tauri::command]
pub async fn apply_store_update(
    window: tauri::WebviewWindow,
    relaunch: bool,
    on_status: Option<tauri::ipc::JavaScriptChannelId>,
) -> Result<&'static str, String> {
    #[cfg(windows)]
    {
        let on_status = on_status.map(|id| id.channel_on(window.as_ref().clone()));
        microsoft::apply(window, relaunch, on_status).await
    }
    #[cfg(not(windows))]
    {
        let _ = (window, relaunch, on_status);
        Err(
            urbudget_services::i18n::tr("Microsoft Store updates are only available on Windows.")
                .into(),
        )
    }
}

#[tauri::command]
pub fn open_update_store() -> Result<(), String> {
    #[cfg(windows)]
    {
        // Fixed destination: callers cannot supply a command or arbitrary URI.
        use windows::core::w;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                w!("ms-windows-store://pdp/?ProductId=9PMQMBZJG06R"),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        if result.0 as isize <= 32 {
            return Err(urbudget_services::i18n::tr(
                "Couldn't open Microsoft Store. Open it from the Start menu and find UrBudget.",
            )
            .into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(urbudget_services::i18n::tr("Microsoft Store is only available on Windows.").into())
    }
}

#[cfg(windows)]
mod microsoft {
    use super::*;
    use std::{
        future::Future,
        sync::{
            atomic::{AtomicBool, AtomicU8, Ordering},
            Arc, Mutex,
        },
        time::{Duration, Instant},
    };
    use windows::{
        core::Interface,
        Services::Store::{
            StoreContext, StorePackageUpdate, StorePackageUpdateResult, StorePackageUpdateState,
            StorePackageUpdateStatus,
        },
        Win32::UI::Shell::IInitializeWithWindow,
    };
    use windows_future::{AsyncOperationProgressHandler, AsyncStatus, IAsyncInfo};

    const DOWNLOAD_STALL: Duration = Duration::from_secs(2 * 60);
    const DOWNLOAD_LIMIT: Duration = Duration::from_secs(30 * 60);
    const INSTALL_STALL: Duration = Duration::from_secs(5 * 60);
    const INSTALL_LIMIT: Duration = Duration::from_secs(45 * 60);
    const CANCEL_GRACE: Duration = Duration::from_secs(5);

    struct DownloadActivity {
        progress: f64,
        changed: Instant,
    }
    impl DownloadActivity {
        fn new(now: Instant) -> Self {
            Self {
                progress: 0.0,
                changed: now,
            }
        }

        fn record(&mut self, progress: f64, now: Instant) -> Option<f64> {
            if !progress.is_finite() || progress <= self.progress {
                return None;
            }
            let progress = progress.clamp(0.0, 1.0);
            if progress <= self.progress {
                return None;
            }
            self.progress = progress;
            self.changed = now;
            Some(progress)
        }

        #[cfg(test)]
        fn stalled(&self, now: Instant) -> bool {
            self.idle_for(now) >= DOWNLOAD_STALL
        }

        fn idle_for(&self, now: Instant) -> Duration {
            now.saturating_duration_since(self.changed)
        }
    }

    static BUSY: AtomicBool = AtomicBool::new(false);
    struct Operation(Arc<BusyLease>);
    struct BusyLease;
    impl Operation {
        fn start() -> Result<Self, String> {
            if !packaged()? {
                return Err(urbudget_services::i18n::tr(
                    "This copy wasn't installed through Microsoft Store.",
                )
                .into());
            }
            Self::acquire()
        }

        fn acquire() -> Result<Self, String> {
            BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| {
                    urbudget_services::i18n::tr(
                        "A Microsoft Store update operation is already running.",
                    )
                    .to_string()
                })?;
            Ok(Self(Arc::new(BusyLease)))
        }
    }
    impl Drop for BusyLease {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::Release);
        }
    }
    fn store_error(error: impl std::fmt::Display) -> String {
        urbudget_services::t!(
            "Microsoft Store couldn't complete the update request: {error}. Try again, or open Microsoft Store.",
            error
        )
    }

    type UiJob = Box<dyn FnOnce() + Send>;

    // 0 = queued, 1 = executing, 2 = canceled before dispatch. A timeout and the UI thread
    // compete for this ticket, so a delayed job can never start an install after an error.
    async fn dispatch<T: Send + 'static>(
        schedule: impl FnOnce(UiJob) -> Result<(), String>,
        work: impl FnOnce() -> windows::core::Result<T> + Send + 'static,
        deadline: Duration,
    ) -> Result<T, String> {
        let ticket = Arc::new(AtomicU8::new(0));
        let executing = ticket.clone();
        let (send, mut receive) = tokio::sync::oneshot::channel();
        schedule(Box::new(move || {
            if executing
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let _ = send.send(work().map_err(store_error));
            }
        }))?;
        let result = match tokio::time::timeout(deadline, &mut receive).await {
            Ok(result) => result,
            Err(_)
                if ticket
                    .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok() =>
            {
                return Err(urbudget_services::i18n::tr(
                    "Microsoft Store took too long to respond. Try again, or open Microsoft Store.",
                )
                .into());
            }
            // The call already started. Its outcome is unknown: do not release the saved
            // budget lock and allow new edits while a synchronous install request is in flight.
            Err(_) => receive.await,
        };
        result.map_err(|_| {
            urbudget_services::i18n::tr(
                "The app window closed before Microsoft Store could respond.",
            )
            .to_string()
        })?
    }

    // Keep every WinRT call on the UI thread that owns the StoreContext, including result
    // collection access and cancellation. Only Rust-owned values cross back to the task.
    async fn on_main<T: Send + 'static>(
        window: &tauri::WebviewWindow,
        work: impl FnOnce() -> windows::core::Result<T> + Send + 'static,
    ) -> Result<T, String> {
        dispatch(
            |job| window.run_on_main_thread(job).map_err(store_error),
            work,
            Duration::from_secs(30),
        )
        .await
    }

    struct StoreRequest<T> {
        request: T,
        // Retain the context for the full operation, as in Microsoft's Store examples.
        _context: StoreContext,
        // A canceled request may continue briefly. Do not allow another operation over it.
        _lease: Arc<BusyLease>,
    }

    async fn start<T: Interface + Send + Sync + 'static>(
        operation: &Operation,
        window: &tauri::WebviewWindow,
        work: impl FnOnce(&StoreContext) -> windows::core::Result<T> + Send + 'static,
    ) -> Result<StoreRequest<T>, String> {
        let owner = window.clone();
        let lease = operation.0.clone();
        on_main(window, move || {
            let context = StoreContext::GetDefault()?;
            let hwnd = owner.hwnd().map_err(|error| {
                windows::core::Error::new(
                    windows::core::HRESULT(0x80004005u32 as i32),
                    error.to_string(),
                )
            })?;
            let initialize: IInitializeWithWindow = context.cast()?;
            unsafe { initialize.Initialize(windows::Win32::Foundation::HWND(hwnd.0)) }?;
            let request = work(&context)?;
            Ok(StoreRequest {
                request,
                _context: context,
                _lease: lease,
            })
        })
        .await
    }

    fn status(
        channel: &Option<tauri::ipc::Channel<StoreInstallStatus>>,
        stage: &'static str,
        progress: Option<f64>,
    ) {
        if let Some(channel) = channel {
            let _ = channel.send(StoreInstallStatus { stage, progress });
        }
    }

    async fn request_status<T: Interface + Send + Sync + 'static>(
        window: &tauri::WebviewWindow,
        request: &T,
    ) -> Result<AsyncStatus, String> {
        let request = request.clone();
        on_main(window, move || request.cast::<IAsyncInfo>()?.Status()).await
    }

    async fn cancel<T: Interface + Send + Sync + 'static>(
        window: &tauri::WebviewWindow,
        request: &T,
    ) {
        let request = request.clone();
        let _ = on_main(window, move || request.cast::<IAsyncInfo>()?.Cancel()).await;
    }

    // Download/query cancellation cannot terminate the app. After a short grace period the
    // caller may recover, while this task keeps the request/context/lease alive until Windows
    // acknowledges cancellation. Installation never takes this detach path.
    fn settle_in_background<T: Interface + Send + Sync + 'static>(
        window: tauri::WebviewWindow,
        request: StoreRequest<T>,
    ) {
        tauri::async_runtime::spawn(async move {
            loop {
                match request_status(&window, &request.request).await {
                    Ok(state) if state != AsyncStatus::Started => break,
                    _ => tokio::time::sleep(Duration::from_secs(1)).await,
                }
            }
            drop(request);
        });
    }

    struct WaitPolicy {
        idle: Duration,
        limit: Duration,
        installs: bool,
        timeout_message: &'static str,
    }

    #[derive(Debug)]
    enum WaitDecision {
        Terminal,
        Failed(String),
        Detached(String),
    }

    async fn wait_state<S, SF, C, CF>(
        read_status: S,
        cancel_request: C,
        activity: &Arc<Mutex<DownloadActivity>>,
        policy: WaitPolicy,
        stopping_notice: impl Fn(),
        poll: Duration,
        grace: Duration,
    ) -> WaitDecision
    where
        S: Fn() -> SF,
        SF: Future<Output = Result<AsyncStatus, String>>,
        C: Fn() -> CF,
        CF: Future<Output = ()>,
    {
        let started = Instant::now();
        let mut stopping: Option<(Instant, String)> = None;
        loop {
            match read_status().await {
                Ok(state) if state != AsyncStatus::Started => {
                    if state == AsyncStatus::Completed {
                        return WaitDecision::Terminal;
                    }
                    return match stopping {
                        Some((_, reason)) => WaitDecision::Failed(reason),
                        None => WaitDecision::Terminal,
                    };
                }
                Err(error) if stopping.is_none() => {
                    stopping = Some((Instant::now(), error));
                    stopping_notice();
                    cancel_request().await;
                }
                _ => {}
            }
            let now = Instant::now();
            if stopping.is_none()
                && (activity.lock().unwrap().idle_for(now) >= policy.idle
                    || now.saturating_duration_since(started) >= policy.limit)
            {
                stopping = Some((now, policy.timeout_message.to_string()));
                stopping_notice();
                cancel_request().await;
            }
            if !policy.installs {
                if let Some((since, reason)) = &stopping {
                    if now.saturating_duration_since(*since) >= grace {
                        return WaitDecision::Detached(reason.clone());
                    }
                }
            }
            tokio::time::sleep(poll).await;
        }
    }

    /// Poll status without depending on WinRT completion notifications. A timed-out install remains
    /// pending (and the frontend's saved-budget lock stays held) until cancellation is terminal.
    /// The stopping event offers a normal, saved close if Windows will not acknowledge it.
    async fn wait<T: Interface + Send + Sync + 'static>(
        window: &tauri::WebviewWindow,
        request: StoreRequest<T>,
        activity: Arc<Mutex<DownloadActivity>>,
        policy: WaitPolicy,
        on_status: Option<tauri::ipc::Channel<StoreInstallStatus>>,
    ) -> Result<StoreRequest<T>, String> {
        match wait_state(
            || request_status(window, &request.request),
            || cancel(window, &request.request),
            &activity,
            policy,
            || status(&on_status, "stopping", None),
            Duration::from_millis(500),
            CANCEL_GRACE,
        )
        .await
        {
            WaitDecision::Terminal => Ok(request),
            WaitDecision::Failed(reason) => Err(reason),
            WaitDecision::Detached(reason) => {
                settle_in_background(window.clone(), request);
                Err(reason)
            }
        }
    }

    pub async fn check(window: tauri::WebviewWindow) -> Result<bool, String> {
        let operation = Operation::start()?;
        Ok(!pending(&operation, &window).await?.is_empty())
    }

    /// The updates the Store has for this copy, ready to hand back to the UI thread.
    async fn pending(
        operation: &Operation,
        window: &tauri::WebviewWindow,
    ) -> Result<Vec<Option<StorePackageUpdate>>, String> {
        let request = start(operation, window, |context| {
            context.GetAppAndOptionalStorePackageUpdatesAsync()
        })
        .await?;
        let request = wait(
            window,
            request,
            Arc::new(Mutex::new(DownloadActivity::new(Instant::now()))),
            WaitPolicy {
                idle: Duration::from_secs(30),
                limit: Duration::from_secs(30),
                installs: false,
                timeout_message: urbudget_services::i18n::tr(
                    "Microsoft Store took too long to check. Try again, or open Microsoft Store.",
                ),
            },
            None,
        )
        .await?;
        on_main(window, move || {
            let updates = request.request.GetResults()?;
            // Consume the apartment-bound collection on its owner thread. Individual package
            // objects are agile and can be carried back for a subsequent UI-thread request.
            (0..updates.Size()?)
                .map(|i| updates.GetAt(i).map(Some))
                .collect()
        })
        .await
    }

    /// Asks Windows to reopen UrBudget once an update has replaced it, or not to. Only an
    /// update counts: never after a crash, a hang or a restart of the computer. Best effort,
    /// since an update that doesn't reopen the app is still an update.
    fn reopen_after_update(relaunch: bool) {
        use windows_sys::Win32::System::Recovery::{
            RegisterApplicationRestart, UnregisterApplicationRestart, RESTART_NO_CRASH,
            RESTART_NO_HANG, RESTART_NO_REBOOT,
        };
        unsafe {
            if relaunch {
                RegisterApplicationRestart(
                    std::ptr::null(),
                    RESTART_NO_CRASH | RESTART_NO_HANG | RESTART_NO_REBOOT,
                );
            } else {
                UnregisterApplicationRestart();
            }
        }
    }

    pub async fn download(
        window: tauri::WebviewWindow,
        on_progress: tauri::ipc::Channel<f64>,
    ) -> Result<&'static str, String> {
        let operation = Operation::start()?;
        let updates = pending(&operation, &window).await?;
        if updates.is_empty() {
            return Ok("none");
        }
        // Windows downloads silently only when the person lets the Store update apps by
        // itself and isn't on a metered connection; otherwise the update waits for consent.
        let started = Instant::now();
        let activity = Arc::new(Mutex::new(DownloadActivity::new(started)));
        let reported = activity.clone();
        let allowed = on_main(&window, || {
            StoreContext::GetDefault()?.CanSilentlyDownloadStorePackageUpdates()
        })
        .await?;
        if !allowed {
            return Ok("consent");
        }
        let request = start(&operation, &window, move |context| {
            let updates: windows_collections::IIterable<StorePackageUpdate> = updates.into();
            let request = context.TrySilentDownloadStorePackageUpdatesAsync(&updates)?;
            let handler = AsyncOperationProgressHandler::<
                StorePackageUpdateResult,
                StorePackageUpdateStatus,
            >::new(move |_, status| {
                let progress = reported
                    .lock()
                    .unwrap()
                    .record(status.TotalDownloadProgress, Instant::now());
                if let Some(progress) = progress {
                    let _ = on_progress.send(progress);
                }
                Ok(())
            });
            // The operation already exists. A missing progress callback is handled by the
            // poll/watchdog path, including acknowledged cancellation and lease retention.
            let _ = request.SetProgress(&handler);
            Ok(request)
        })
        .await?;
        let request = wait(&window, request, activity, WaitPolicy {
            idle: DOWNLOAD_STALL,
            limit: DOWNLOAD_LIMIT,
            installs: false,
            timeout_message: urbudget_services::i18n::tr("Microsoft Store stopped reporting download progress. Try again, or open Microsoft Store."),
        }, None).await?;
        let state = on_main(&window, move || {
            request.request.GetResults()?.OverallState()
        })
        .await?;
        Ok(match state {
            StorePackageUpdateState::Completed => "ready",
            _ => "consent",
        })
    }

    pub async fn apply(
        window: tauri::WebviewWindow,
        relaunch: bool,
        on_status: Option<tauri::ipc::Channel<StoreInstallStatus>>,
    ) -> Result<&'static str, String> {
        let operation = Operation::start()?;
        status(&on_status, "preparing", None);
        let updates = pending(&operation, &window).await?;
        if updates.is_empty() {
            return Ok("none");
        }
        let allowed = on_main(&window, || {
            StoreContext::GetDefault()?.CanSilentlyDownloadStorePackageUpdates()
        })
        .await?;
        if !allowed {
            return Ok("consent");
        }
        reopen_after_update(relaunch);
        // The frontend has saved the budget and holds its edit lock. Windows closes the app
        // to replace its package, so this may be the last thing this process does.
        let state: Result<StorePackageUpdateState, String> = async {
            let activity = Arc::new(Mutex::new(DownloadActivity::new(Instant::now())));
            let reported = activity.clone();
            let progress = on_status.clone();
            let request = start(&operation, &window, move |context| {
                let updates: windows_collections::IIterable<StorePackageUpdate> = updates.into();
                let request =
                    context.TrySilentDownloadAndInstallStorePackageUpdatesAsync(&updates)?;
                install_progress(&request, reported, progress);
                Ok(request)
            })
            .await?;
            let request = wait(&window, request, activity, install_policy(), on_status).await?;
            on_main(&window, move || {
                request.request.GetResults()?.OverallState()
            })
            .await
        }
        .await;
        // Only an update that went through reopens the app: not one Windows wouldn't make
        // without asking, nor one that failed.
        if !matches!(state, Ok(StorePackageUpdateState::Completed)) {
            reopen_after_update(false);
        }
        completed(state?).map(|()| "installed")
    }

    pub async fn install(
        window: tauri::WebviewWindow,
        on_status: Option<tauri::ipc::Channel<StoreInstallStatus>>,
    ) -> Result<bool, String> {
        let operation = Operation::start()?;
        status(&on_status, "preparing", None);
        let updates = pending(&operation, &window).await?;
        if updates.is_empty() {
            return Ok(false);
        }
        // Windows may close UrBudget to replace it; it reopens it afterwards.
        reopen_after_update(true);
        // The frontend holds the saved-budget operation lock through this request.
        // Windows asks for consent and may close the app to replace its package. Any failure,
        // even before Windows answers, takes the reopening back.
        let state: Result<StorePackageUpdateState, String> = async {
            let activity = Arc::new(Mutex::new(DownloadActivity::new(Instant::now())));
            let reported = activity.clone();
            let progress = on_status.clone();
            let request = start(&operation, &window, move |context| {
                let updates: windows_collections::IIterable<StorePackageUpdate> = updates.into();
                let request =
                    context.RequestDownloadAndInstallStorePackageUpdatesAsync(&updates)?;
                install_progress(&request, reported, progress);
                Ok(request)
            })
            .await?;
            let request = wait(&window, request, activity, install_policy(), on_status).await?;
            on_main(&window, move || {
                request.request.GetResults()?.OverallState()
            })
            .await
        }
        .await;
        if !matches!(state, Ok(StorePackageUpdateState::Completed)) {
            reopen_after_update(false);
        }
        completed(state?)?;
        Ok(true)
    }

    fn install_policy() -> WaitPolicy {
        WaitPolicy {
            idle: INSTALL_STALL,
            limit: INSTALL_LIMIT,
            installs: true,
            timeout_message: urbudget_services::i18n::tr("Microsoft Store stopped responding while installing. The request was canceled. Try again, or open Microsoft Store."),
        }
    }

    fn install_progress(
        request: &windows_future::IAsyncOperationWithProgress<
            StorePackageUpdateResult,
            StorePackageUpdateStatus,
        >,
        activity: Arc<Mutex<DownloadActivity>>,
        on_status: Option<tauri::ipc::Channel<StoreInstallStatus>>,
    ) {
        status(&on_status, "installing", None);
        let handler = AsyncOperationProgressHandler::<
            StorePackageUpdateResult,
            StorePackageUpdateStatus,
        >::new(move |_, update| {
            if let Some(progress) = activity
                .lock()
                .unwrap()
                .record(update.TotalDownloadProgress, Instant::now())
            {
                status(&on_status, "installing", Some(progress));
            }
            Ok(())
        });
        // Progress is optional. Failure to attach it must never lose an already-started install.
        let _ = request.SetProgress(&handler);
    }

    fn completed(state: StorePackageUpdateState) -> Result<(), String> {
        match state {
            StorePackageUpdateState::Completed => Ok(()),
            StorePackageUpdateState::Canceled => Err(urbudget_services::i18n::tr(
                "The Microsoft Store update was canceled. You can try again.",
            )
            .into()),
            StorePackageUpdateState::ErrorLowBattery => Err(urbudget_services::i18n::tr(
                "Connect this computer to power, then try the update again.",
            )
            .into()),
            StorePackageUpdateState::ErrorWiFiRecommended
            | StorePackageUpdateState::ErrorWiFiRequired => Err(urbudget_services::i18n::tr(
                "Connect to Wi-Fi, then try the update again, or open Microsoft Store.",
            )
            .into()),
            _ => Err(urbudget_services::i18n::tr(
                "Microsoft Store hasn't installed the update. Try again, or open Microsoft Store.",
            )
            .into()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        include!("updates_orchestration_tests.rs");
        #[test]
        fn a_store_download_stalls_without_new_progress_even_after_all_bytes_arrive() {
            let start = Instant::now();
            let mut activity = DownloadActivity::new(start);
            assert!(activity.stalled(start + DOWNLOAD_STALL));
            assert_eq!(activity.record(1.0, start + DOWNLOAD_STALL), Some(1.0));
            assert_eq!(activity.record(1.0, start + DOWNLOAD_STALL * 2), None);
            assert!(activity.stalled(start + DOWNLOAD_STALL * 2));
        }

        #[test]
        fn a_slow_store_download_stays_active_only_while_it_advances() {
            let start = Instant::now();
            let mut activity = DownloadActivity::new(start);
            assert_eq!(
                activity.record(0.1, start + Duration::from_secs(90)),
                Some(0.1)
            );
            assert!(!activity.stalled(start + DOWNLOAD_STALL));
            assert_eq!(activity.record(f64::NAN, start + DOWNLOAD_STALL), None);
            assert_eq!(activity.record(0.05, start + DOWNLOAD_STALL), None);
            assert!(activity.stalled(start + Duration::from_secs(90) + DOWNLOAD_STALL));
        }

        #[test]
        fn only_a_completed_store_install_is_successful() {
            assert!(completed(StorePackageUpdateState::Completed).is_ok());
            for state in [
                StorePackageUpdateState::Pending,
                StorePackageUpdateState::Downloading,
                StorePackageUpdateState::Deploying,
                StorePackageUpdateState::Canceled,
                StorePackageUpdateState::OtherError,
                StorePackageUpdateState::ErrorLowBattery,
                StorePackageUpdateState::ErrorWiFiRequired,
                StorePackageUpdateState::ErrorWiFiRecommended,
                StorePackageUpdateState(99),
            ] {
                assert!(completed(state).is_err(), "{state:?}");
            }
        }
    }
}

/// Starts the new version after an update is installed. On Windows the installer does this itself.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}
