//! Standalone copies use signed Tauri updates; MSIX copies use Microsoft Store APIs.
//! The frontend saves and locks the budget before an installer can close the app.
use tauri::AppHandle;

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
        code => Err(format!(
            "Couldn't identify this app's update channel (Windows error {code})."
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
pub async fn report_usage(app: AppHandle, usage: tauri::State<'_, urbudget_services::usage::Usage>) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if std::env::var_os("URBUDGET_API").is_none() {
        return Ok(());
    }
    let channel = if packaged()? { "store" } else { "direct" };
    let version = app.package_info().version.to_string();
    usage.report(&crate::cloud::site(), std::env::consts::OS, channel, &version).await
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
        Err("Microsoft Store updates are only available on Windows.".into())
    }
}

#[tauri::command]
pub async fn install_store_update(window: tauri::WebviewWindow) -> Result<bool, String> {
    #[cfg(windows)]
    {
        microsoft::install(window).await
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("Microsoft Store updates are only available on Windows.".into())
    }
}

/// Downloads a Store update in the background: `"ready"` when Windows will also install it without
/// asking, `"consent"` when it needs the person's go-ahead (Store auto-updates off, a metered
/// connection), `"none"` when there's nothing to download.
#[tauri::command]
pub async fn download_store_update(window: tauri::WebviewWindow) -> Result<&'static str, String> {
    #[cfg(windows)]
    {
        microsoft::download(window).await
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("Microsoft Store updates are only available on Windows.".into())
    }
}

/// Installs a downloaded Store update without asking. Windows closes UrBudget to replace it and,
/// with `relaunch`, opens it again afterwards. `"installed"`, `"none"` when there was nothing to
/// install, or `"consent"` when Windows won't now without the person's go-ahead (they turned the
/// Store's automatic updates off, or the connection is metered): `install_store_update` asks.
#[tauri::command]
pub async fn apply_store_update(window: tauri::WebviewWindow, relaunch: bool) -> Result<&'static str, String> {
    #[cfg(windows)]
    {
        microsoft::apply(window, relaunch).await
    }
    #[cfg(not(windows))]
    {
        let _ = (window, relaunch);
        Err("Microsoft Store updates are only available on Windows.".into())
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
            return Err(
                "Couldn't open Microsoft Store. Open it from the Start menu and find UrBudget."
                    .into(),
            );
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Microsoft Store is only available on Windows.".into())
    }
}

#[cfg(windows)]
mod microsoft {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };
    use windows::{
        core::Interface,
        Services::Store::{StoreContext, StorePackageUpdate, StorePackageUpdateState},
        Win32::UI::Shell::IInitializeWithWindow,
    };

    static BUSY: AtomicBool = AtomicBool::new(false);
    struct Operation;
    impl Operation {
        fn start() -> Result<Self, String> {
            if !packaged()? {
                return Err("This copy wasn't installed through Microsoft Store.".into());
            }
            BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| {
                    "A Microsoft Store update operation is already running.".to_string()
                })?;
            Ok(Self)
        }
    }
    impl Drop for Operation {
        fn drop(&mut self) {
            BUSY.store(false, Ordering::Release);
        }
    }
    fn store_error(error: impl std::fmt::Display) -> String {
        format!("Microsoft Store couldn't complete the update request: {error}. Try again, or open Microsoft Store.")
    }

    // Store APIs must be started on the UI thread, with an owner HWND. Awaiting the
    // returned WinRT operation off-thread keeps the webview and native dialogs responsive.
    async fn on_ui<T: Send + 'static>(
        window: &tauri::WebviewWindow,
        work: impl FnOnce(StoreContext) -> windows::core::Result<T> + Send + 'static,
    ) -> Result<T, String> {
        let owner = window.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        window
            .run_on_main_thread(move || {
                let result = (|| {
                    let context = StoreContext::GetDefault().map_err(store_error)?;
                    let hwnd = owner.hwnd().map_err(store_error)?;
                    let initialize: IInitializeWithWindow = context.cast().map_err(store_error)?;
                    unsafe { initialize.Initialize(windows::Win32::Foundation::HWND(hwnd.0)) }
                        .map_err(store_error)?;
                    work(context).map_err(store_error)
                })();
                let _ = send.send(result);
            })
            .map_err(store_error)?;
        receive.await.map_err(|_| {
            "The app window closed before Microsoft Store could respond.".to_string()
        })?
    }

    pub async fn check(window: tauri::WebviewWindow) -> Result<bool, String> {
        let _operation = Operation::start()?;
        let request = on_ui(&window, |context| {
            context.GetAppAndOptionalStorePackageUpdatesAsync()
        })
        .await?;
        match tokio::time::timeout(Duration::from_secs(30), async { request.clone().await }).await {
            Ok(result) => Ok(result.map_err(store_error)?.Size().map_err(store_error)? > 0),
            Err(_) => {
                let _ = request.Cancel();
                Err(
                    "Microsoft Store took too long to check. Try again, or open Microsoft Store."
                        .into(),
                )
            }
        }
    }

    /// The updates the Store has for this copy, ready to hand back to the UI thread.
    async fn pending(window: &tauri::WebviewWindow) -> Result<Vec<Option<StorePackageUpdate>>, String> {
        let request = on_ui(window, |context| {
            context.GetAppAndOptionalStorePackageUpdatesAsync()
        })
        .await?;
        match tokio::time::timeout(Duration::from_secs(30), async { request.clone().await }).await {
            Ok(result) => {
                let updates = result.map_err(store_error)?;
                // StorePackageUpdate is agile, but its collection is apartment-bound.
                // Move only the individual updates and construct the iterable on the UI thread.
                (0..updates.Size().map_err(store_error)?)
                    .map(|i| updates.GetAt(i).map(Some))
                    .collect::<windows::core::Result<Vec<_>>>()
                    .map_err(store_error)
            }
            Err(_) => {
                let _ = request.Cancel();
                Err(
                    "Microsoft Store took too long to check. Try again, or open Microsoft Store."
                        .into(),
                )
            }
        }
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

    pub async fn download(window: tauri::WebviewWindow) -> Result<&'static str, String> {
        let _operation = Operation::start()?;
        let updates = pending(&window).await?;
        if updates.is_empty() {
            return Ok("none");
        }
        // Windows downloads silently only when the person lets the Store update apps by
        // itself and isn't on a metered connection; otherwise the update waits for consent.
        let request = on_ui(&window, move |context| {
            if !context.CanSilentlyDownloadStorePackageUpdates()? {
                return Ok(None);
            }
            let updates: windows_collections::IIterable<StorePackageUpdate> = updates.into();
            context.TrySilentDownloadStorePackageUpdatesAsync(&updates).map(Some)
        })
        .await?;
        let Some(request) = request else { return Ok("consent") };
        // A download Windows never finishes would hold every later check off.
        let result = match tokio::time::timeout(Duration::from_secs(30 * 60), async { request.clone().await }).await {
            Ok(result) => result.map_err(store_error)?,
            Err(_) => {
                let _ = request.Cancel();
                return Err("Microsoft Store took too long to download the update. It will try again later.".into());
            }
        };
        Ok(match result.OverallState().map_err(store_error)? {
            StorePackageUpdateState::Completed => "ready",
            _ => "consent",
        })
    }

    pub async fn apply(window: tauri::WebviewWindow, relaunch: bool) -> Result<&'static str, String> {
        let _operation = Operation::start()?;
        let updates = pending(&window).await?;
        if updates.is_empty() {
            return Ok("none");
        }
        reopen_after_update(relaunch);
        // The frontend has saved the budget and holds its edit lock. Windows closes the app
        // to replace its package, so this may be the last thing this process does.
        let state: Result<Option<StorePackageUpdateState>, String> = async {
            let request = on_ui(&window, move |context| {
                if !context.CanSilentlyDownloadStorePackageUpdates()? {
                    return Ok(None);
                }
                let updates: windows_collections::IIterable<StorePackageUpdate> = updates.into();
                context.TrySilentDownloadAndInstallStorePackageUpdatesAsync(&updates).map(Some)
            })
            .await?;
            let Some(request) = request else { return Ok(None) };
            request.await.map_err(store_error)?.OverallState().map_err(store_error).map(Some)
        }
        .await;
        // Only an update that went through reopens the app: not one Windows wouldn't make
        // without asking, nor one that failed.
        if !matches!(state, Ok(Some(StorePackageUpdateState::Completed))) {
            reopen_after_update(false);
        }
        match state? {
            None => Ok("consent"),
            Some(state) => completed(state).map(|()| "installed"),
        }
    }

    pub async fn install(window: tauri::WebviewWindow) -> Result<bool, String> {
        let _operation = Operation::start()?;
        let updates = pending(&window).await?;
        if updates.is_empty() {
            return Ok(false);
        }
        // Windows may close UrBudget to replace it; it reopens it afterwards.
        reopen_after_update(true);
        // The frontend holds the saved-budget operation lock through this request.
        // Windows asks for consent and may close the app to replace its package. Any failure,
        // even before Windows answers, takes the reopening back.
        let state: Result<StorePackageUpdateState, String> = async {
            let request = on_ui(&window, move |context| {
                let updates: windows_collections::IIterable<StorePackageUpdate> = updates.into();
                context.RequestDownloadAndInstallStorePackageUpdatesAsync(&updates)
            })
            .await?;
            request.await.map_err(store_error)?.OverallState().map_err(store_error)
        }
        .await;
        if !matches!(state, Ok(StorePackageUpdateState::Completed)) {
            reopen_after_update(false);
        }
        completed(state?)?;
        Ok(true)
    }

    fn completed(state: StorePackageUpdateState) -> Result<(), String> {
        match state {
            StorePackageUpdateState::Completed => Ok(()),
            StorePackageUpdateState::Canceled => {
                Err("The Microsoft Store update was canceled. You can try again.".into())
            }
            StorePackageUpdateState::ErrorLowBattery => {
                Err("Connect this computer to power, then try the update again.".into())
            }
            StorePackageUpdateState::ErrorWiFiRecommended
            | StorePackageUpdateState::ErrorWiFiRequired => {
                Err("Connect to Wi-Fi, then try the update again, or open Microsoft Store.".into())
            }
            _ => Err(
                "Microsoft Store hasn't installed the update. Try again, or open Microsoft Store."
                    .into(),
            ),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
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
