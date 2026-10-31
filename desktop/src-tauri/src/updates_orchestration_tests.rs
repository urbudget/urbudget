// These tests use real WinRT async interfaces with deliberately missing callbacks, then run
// the same status/cancel orchestration production uses. No Store account, package or budget
// is touched. The small deadlines keep timeout/race coverage deterministic and fast.
mod orchestration {
    use super::*;
    use std::sync::atomic::{AtomicI32, AtomicUsize};
    use windows::core::{Error, Ref, Result as WinResult, HRESULT};
    use windows_future::{
        AsyncOperationWithProgressCompletedHandler, IAsyncInfo_Impl, IAsyncOperationWithProgress,
        IAsyncOperationWithProgress_Impl,
    };

    struct Control {
        state: AtomicI32,
        cancel_state: i32,
        cancellations: AtomicUsize,
    }

    #[windows::core::implement(IAsyncInfo, IAsyncOperationWithProgress<u32, f64>)]
    struct SilentOperation(Arc<Control>);

    impl IAsyncInfo_Impl for SilentOperation_Impl {
        fn Id(&self) -> WinResult<u32> {
            Ok(1)
        }
        fn Status(&self) -> WinResult<AsyncStatus> {
            Ok(AsyncStatus(self.0.state.load(Ordering::Acquire)))
        }
        fn ErrorCode(&self) -> WinResult<HRESULT> {
            Ok(HRESULT(0))
        }
        fn Cancel(&self) -> WinResult<()> {
            self.0.cancellations.fetch_add(1, Ordering::AcqRel);
            self.0.state.store(self.0.cancel_state, Ordering::Release);
            Ok(())
        }
        fn Close(&self) -> WinResult<()> {
            Ok(())
        }
    }

    impl IAsyncOperationWithProgress_Impl<u32, f64> for SilentOperation_Impl {
        fn SetProgress(&self, _: Ref<AsyncOperationProgressHandler<u32, f64>>) -> WinResult<()> {
            Ok(())
        }
        fn Progress(&self) -> WinResult<AsyncOperationProgressHandler<u32, f64>> {
            Err(Error::empty())
        }
        // Deliberately accept a handler without ever notifying it.
        fn SetCompleted(
            &self,
            _: Ref<AsyncOperationWithProgressCompletedHandler<u32, f64>>,
        ) -> WinResult<()> {
            Ok(())
        }
        fn Completed(&self) -> WinResult<AsyncOperationWithProgressCompletedHandler<u32, f64>> {
            Err(Error::empty())
        }
        fn GetResults(&self) -> WinResult<u32> {
            if self.0.state.load(Ordering::Acquire) == AsyncStatus::Completed.0 {
                Ok(42)
            } else {
                Err(Error::from_hresult(HRESULT(0x80004004u32 as i32)))
            }
        }
    }

    fn fake(
        initial: AsyncStatus,
        after_cancel: AsyncStatus,
    ) -> (IAsyncOperationWithProgress<u32, f64>, Arc<Control>) {
        let control = Arc::new(Control {
            state: AtomicI32::new(initial.0),
            cancel_state: after_cancel.0,
            cancellations: AtomicUsize::new(0),
        });
        (SilentOperation(control.clone()).into(), control)
    }

    fn test_policy(installs: bool) -> WaitPolicy {
        WaitPolicy {
            idle: Duration::from_millis(4),
            limit: Duration::from_millis(30),
            installs,
            timeout_message: "Microsoft Store stopped reporting download progress. Try again, or open Microsoft Store.",
        }
    }

    async fn drive(
        request: &IAsyncOperationWithProgress<u32, f64>,
        installs: bool,
        notices: &AtomicUsize,
    ) -> WaitDecision {
        let activity = Arc::new(Mutex::new(DownloadActivity::new(Instant::now())));
        wait_state(
            || std::future::ready(request.Status().map_err(store_error)),
            || {
                let _ = request.Cancel();
                std::future::ready(())
            },
            &activity,
            test_policy(installs),
            || {
                notices.fetch_add(1, Ordering::AcqRel);
            },
            Duration::from_millis(1),
            Duration::from_millis(3),
        )
        .await
    }

    #[test]
    fn polls_winrt_completion_even_when_no_completion_handler_fires() {
        tauri::async_runtime::block_on(async {
            let (request, control) = fake(AsyncStatus::Started, AsyncStatus::Canceled);
            let changed = control.clone();
            let notices = AtomicUsize::new(0);
            let activity = Arc::new(Mutex::new(DownloadActivity::new(Instant::now())));
            let decision = wait_state(
                || {
                    changed
                        .state
                        .store(AsyncStatus::Completed.0, Ordering::Release);
                    std::future::ready(request.Status().map_err(store_error))
                },
                || {
                    let _ = request.Cancel();
                    std::future::ready(())
                },
                &activity,
                test_policy(false),
                || {
                    notices.fetch_add(1, Ordering::AcqRel);
                },
                Duration::from_millis(1),
                Duration::from_millis(3),
            )
            .await;
            assert!(matches!(decision, WaitDecision::Terminal));
            assert_eq!(request.GetResults().unwrap(), 42);
            assert_eq!(control.cancellations.load(Ordering::Acquire), 0);
            assert_eq!(notices.load(Ordering::Acquire), 0);
        });
    }

    #[test]
    fn stalled_request_waits_for_terminal_cancellation_and_reports_once() {
        tauri::async_runtime::block_on(async {
            for installs in [false, true] {
                let (request, control) = fake(AsyncStatus::Started, AsyncStatus::Canceled);
                let notices = AtomicUsize::new(0);
                assert!(matches!(
                    drive(&request, installs, &notices).await,
                    WaitDecision::Failed(_)
                ));
                assert_eq!(request.Status().unwrap(), AsyncStatus::Canceled);
                assert_eq!(control.cancellations.load(Ordering::Acquire), 1);
                assert_eq!(notices.load(Ordering::Acquire), 1);
            }
        });
    }

    #[test]
    fn canceled_download_can_detach_but_uncertain_install_cannot_unlock() {
        tauri::async_runtime::block_on(async {
            let (request, control) = fake(AsyncStatus::Started, AsyncStatus::Started);
            let notices = AtomicUsize::new(0);
            assert!(matches!(
                drive(&request, false, &notices).await,
                WaitDecision::Detached(_)
            ));
            assert_eq!(control.cancellations.load(Ordering::Acquire), 1);
            let (install, control) = fake(AsyncStatus::Started, AsyncStatus::Started);
            let notices = AtomicUsize::new(0);
            assert!(tokio::time::timeout(
                Duration::from_millis(35),
                drive(&install, true, &notices)
            )
            .await
            .is_err());
            assert_eq!(control.cancellations.load(Ordering::Acquire), 1);
            assert_eq!(notices.load(Ordering::Acquire), 1);
        });
    }

    #[test]
    fn completion_winning_the_cancel_race_keeps_its_successful_result() {
        tauri::async_runtime::block_on(async {
            let (request, _) = fake(AsyncStatus::Started, AsyncStatus::Completed);
            assert!(matches!(
                drive(&request, true, &AtomicUsize::new(0)).await,
                WaitDecision::Terminal
            ));
            assert_eq!(request.GetResults().unwrap(), 42);
        });
    }

    #[test]
    fn status_failure_cancels_once_and_waits_for_terminal_acknowledgement() {
        tauri::async_runtime::block_on(async {
            let (request, control) = fake(AsyncStatus::Started, AsyncStatus::Canceled);
            let reads = AtomicUsize::new(0);
            let notices = AtomicUsize::new(0);
            let activity = Arc::new(Mutex::new(DownloadActivity::new(Instant::now())));
            let decision = wait_state(
                || {
                    std::future::ready(if reads.fetch_add(1, Ordering::AcqRel) == 0 {
                        Err("status unavailable".to_owned())
                    } else {
                        request.Status().map_err(store_error)
                    })
                },
                || {
                    let _ = request.Cancel();
                    std::future::ready(())
                },
                &activity,
                test_policy(true),
                || {
                    notices.fetch_add(1, Ordering::AcqRel);
                },
                Duration::from_millis(1),
                Duration::from_millis(3),
            )
            .await;
            assert!(
                matches!(decision, WaitDecision::Failed(reason) if reason == "status unavailable")
            );
            assert_eq!(control.cancellations.load(Ordering::Acquire), 1);
            assert_eq!(notices.load(Ordering::Acquire), 1);
            assert_eq!(request.Status().unwrap(), AsyncStatus::Canceled);
        });
    }

    #[test]
    fn repeated_status_failures_cannot_release_an_uncertain_install() {
        tauri::async_runtime::block_on(async {
            let (request, control) = fake(AsyncStatus::Started, AsyncStatus::Started);
            let notices = AtomicUsize::new(0);
            let activity = Arc::new(Mutex::new(DownloadActivity::new(Instant::now())));
            let decision = wait_state(
                || std::future::ready(Err("status unavailable".to_owned())),
                || {
                    let _ = request.Cancel();
                    std::future::ready(())
                },
                &activity,
                test_policy(true),
                || {
                    notices.fetch_add(1, Ordering::AcqRel);
                },
                Duration::from_millis(1),
                Duration::from_millis(3),
            );
            assert!(tokio::time::timeout(Duration::from_millis(20), decision)
                .await
                .is_err());
            assert_eq!(control.cancellations.load(Ordering::Acquire), 1);
            assert_eq!(notices.load(Ordering::Acquire), 1);
        });
    }

    #[test]
    fn total_deadline_stops_a_request_even_when_its_progress_keeps_advancing() {
        tauri::async_runtime::block_on(async {
            let (request, control) = fake(AsyncStatus::Started, AsyncStatus::Canceled);
            let activity = Arc::new(Mutex::new(DownloadActivity::new(Instant::now())));
            let policy = WaitPolicy {
                idle: Duration::from_secs(60),
                limit: Duration::from_millis(4),
                ..test_policy(false)
            };
            let decision = wait_state(
                || {
                    let mut current = activity.lock().unwrap();
                    let progress = current.progress + 0.01;
                    current.record(progress, Instant::now());
                    std::future::ready(request.Status().map_err(store_error))
                },
                || {
                    let _ = request.Cancel();
                    std::future::ready(())
                },
                &activity,
                policy,
                || {},
                Duration::from_millis(1),
                Duration::from_millis(3),
            )
            .await;
            assert!(matches!(decision, WaitDecision::Failed(_)));
            assert_eq!(control.cancellations.load(Ordering::Acquire), 1);
            assert!(activity.lock().unwrap().idle_for(Instant::now()) < Duration::from_secs(60));
        });
    }

    #[test]
    fn repeated_terminal_requests_do_not_send_cancellation() {
        tauri::async_runtime::block_on(async {
            for index in 0..1000 {
                let initial = if index % 2 == 0 {
                    AsyncStatus::Completed
                } else {
                    AsyncStatus::Error
                };
                let (request, control) = fake(initial, AsyncStatus::Canceled);
                assert!(matches!(
                    drive(&request, false, &AtomicUsize::new(0)).await,
                    WaitDecision::Terminal
                ));
                assert_eq!(control.cancellations.load(Ordering::Acquire), 0);
            }
        });
    }

    #[test]
    fn busy_lease_blocks_retries_until_the_final_request_owner_drops() {
        // This is the same ownership split used by a caller and a detached cancellation
        // monitor. Releasing the caller alone must not permit an overlapping Store request.
        for _ in 0..1000 {
            let operation = Operation::acquire().unwrap();
            let request_lease = operation.0.clone();
            assert!(Operation::acquire().is_err());
            drop(operation);
            assert!(Operation::acquire().is_err());
            drop(request_lease);
            drop(Operation::acquire().unwrap());
        }
    }

    #[test]
    fn timed_out_ui_job_never_starts_later() {
        tauri::async_runtime::block_on(async {
            let queued = Arc::new(Mutex::new(None::<UiJob>));
            let work = queued.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let called = calls.clone();
            let result = dispatch(
                move |job| {
                    *work.lock().unwrap() = Some(job);
                    Ok(())
                },
                move || {
                    called.fetch_add(1, Ordering::AcqRel);
                    Ok(42)
                },
                Duration::from_millis(1),
            )
            .await;
            assert!(result.is_err());
            queued.lock().unwrap().take().unwrap()();
            assert_eq!(calls.load(Ordering::Acquire), 0);
        });
    }

    #[test]
    fn dropped_ui_response_is_an_error() {
        tauri::async_runtime::block_on(async {
            let result = dispatch(
                |job| {
                    drop(job);
                    Ok(())
                },
                || Ok(42),
                Duration::from_secs(1),
            )
            .await;
            assert!(result.unwrap_err().contains("window closed"));
        });
    }

    #[test]
    fn ui_work_already_started_is_not_abandoned_at_its_dispatch_deadline() {
        tauri::async_runtime::block_on(async {
            let (release, wait) = std::sync::mpsc::channel();
            let (started, acknowledged) = std::sync::mpsc::channel();
            let job = dispatch(
                move |job| {
                    std::thread::spawn(job);
                    acknowledged.recv().unwrap();
                    Ok(())
                },
                move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                    Ok(42)
                },
                Duration::from_millis(1),
            );
            tokio::pin!(job);
            assert!(tokio::time::timeout(Duration::from_millis(10), &mut job)
                .await
                .is_err());
            release.send(()).unwrap();
            assert_eq!(job.await.unwrap(), 42);
        });
    }
}
