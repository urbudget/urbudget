# Sync v2 platform implementation and validation

Worktree: `C:/Users/Admin/.codex/worktrees/sync-v2-redesign/urbudget`.
Validation performed on Windows, with a disposable Android emulator and a local synthetic account. No production service, release tag, or deployment was changed.

## Implemented platform behavior

- Desktop, Android and iOS use the shared account library for local, remote, evicted, trashed and recovered budgets. The last active budget can be trashed without creating a phantom replacement. Opening a remote entry hydrates it through `library_open`.
- Existing private budgets require explicit account association. New budgets offer a local-only option; samples remain local-only. Before account-level sync is enabled on the server, the older per-budget upload/download/remove-cloud controls remain available. Only account-owned entries describe deletion as applying on every device.
- Library workflows include trash, restore, remove download, recover as a new copy, and inspection/discard of unfinished raw editor fields. Purged entries do not offer Restore. Recovered entries offer recovery rather than an invalid Open action.
- Native record guards reject saves after the edited record was deleted. Desktop background refresh respects open editors, local dirty state, and pending recovery. iOS budget-name commit checks the editor's opening budget identity.
- Raw draft checkpoints preserve incomplete strings such as `12 +`, stable editor identity, string epochs and compact opening records. Old queued epochs are coalesced; discard is ordered behind pending writes. Large transaction history is not repeated in each checkpoint. Drafts are displayed for explicit review/re-entry rather than blindly replayed.
- Desktop's bounded exit drain includes all independent editor lanes, including inline payee/rule and assistant editors. Pending sync status comes from durable library state instead of only the last successful attempt.
- Both native Rust adapters serialize concurrent first initialization while preserving the existing same-directory check.

## Editor coverage

Desktop captures transaction/account/category/reconcile/payment dialogs; register and planning editors, including flag names; loan planner; payee and rule inline fields; assistant proposal row edits; bank-file content/session/decisions and visible partial line fields; and YNAB review choices. Inline editors use their own checkpoint lanes and participate in the global close drain.

Android and iOS capture transactions and splits, schedules, accounts/reconciliation, category details/notes/targets and assignment pads, moves, card and loan payments, loan terms/rate changes, payees/rules, group/category creation and planning fields, settings/flag names, bank-import state and individual line fields, YNAB review choices, and assistant proposal row fields. Authentication passwords and API tokens are excluded.

Recovery is manual review/re-entry of raw fields, not restoration of an identical visual form. Transient navigation, searches, report date filters, and assistant message composition are outside financial draft checkpoints. Import files and session data remain subject to the native checkpoint size limit; oversized checkpoints report failure rather than truncating data silently.

## Scheduling and OS limits

- Foreground account reconciliation runs every 30 seconds on all platforms.
- Desktop and iOS schedule after 2 seconds of settled saves, capped at a 15-second batch delay. Desktop also wakes on focus/connectivity and bounds the close drain to 3 seconds.
- Android saves and activity stop enqueue persistent network-constrained WorkManager work with a 2-second initial delay. Its foreground save collector also debounces 2 seconds. A periodic 15-minute WorkManager request provides retry coverage; a persisted wake generation prevents a save during a running request from being silently forgotten.
- iOS registers BGAppRefresh at launch, requests an earliest wake after 15 minutes, and uses an expiring UIKit background grant when the app leaves the foreground. Unavailable protected data or missing account credentials defer work safely. A selected signed-in v2 account continues catalog/receipt/deletion reconciliation while its sync key is locked; financial upload/download still waits for unlock. These are opportunistic OS facilities: no claim is made that a terminated, force-stopped or locked app always syncs immediately.

## Validation evidence

- TypeScript: `npx tsc --noEmit` passed.
- Desktop: full Vitest suite passed after the final close-drain registry changes: 78 test files, 690 tests, followed by the root production build/version/TypeScript/Vite gate. The added regressions cover preserving dirty/editor state, bounded exit behavior, string epochs, stale-discard ordering, 1,000-change coalescing, and draining independent leaf-editor writes.
- Android: Kotlin compilation and 136 JVM tests across 24 test files passed, including a 5,000-transaction fixture proving generic checkpoints retain bounded opening records instead of transaction history.
- Android JNI: the final shared Rust source was cross-compiled for `x86_64-linux-android` with the explicit debug QA configuration. App and instrumentation APKs built successfully after the final UI fixes.
- Actual Android runtime: `SyncV2RuntimeTest` passed on disposable `CodexSyncV2Qa` (API 37, x86_64, adb serial `emulator-5580`) in 9.26 seconds on the final shared-source build. The test launched the real MainActivity, saved a budget, backgrounded/resumed the Activity, observed the app's own unique WorkManager sync work, and required the durable outgoing queue to drain. Before opening the Activity it also locked the v2 account, made a pending edit, waited for a new scheduled catalog report, and verified that financial changes remained pending until unlock. It additionally verified stale epoch discard protection, deleting the active/last budget and empty storage, stale-save rejection, restore, eviction/redownload, and signed-out creation staying local-only.
- The runtime used only `sync-v2-android-20261028@example.test` on a local Worker at port 8788. ADB reverse mapped emulator loopback to that local Worker. No real phone or existing AVD was used.
- Raw instrumentation output: [2026-10-29-sync-v2-android-instrumentation.txt](2026-10-29-sync-v2-android-instrumentation.txt).
- iOS localization JSON, Info.plist and Android XML resources parse successfully. The iOS optional-budget paths were source-reviewed; no force unwraps of `budget`/`summary` remain in those screens.

## Remaining release gates

This Windows host cannot compile or run Xcode, an iOS Simulator, or physical iOS background behavior. The updated `ios-native.yml` now runs its existing Simulator tests and unsigned device build for shared core/services Rust changes as well as iOS files. That macOS build/test gate must pass before release. No physical-device Doze, force-stop or iOS protected-data background test is claimed by the emulator result.

Android QA endpoint overrides are debug-only. Release behavior retains the normal endpoint and the server's disabled-by-default v2 activation gate.

## Follow-up CI and notification coverage

`.github/workflows/android-native.yml` now contains a separate `sync-lifecycle` job. It builds only x86_64 debug QA APKs, starts an API 37 emulator, and runs `web/scripts/sync-v2-android-ci.mjs`. The helper starts its own local D1/R2 Worker, seeds the fixed synthetic Android account with an active subscription, uses adb reverse for port 8788, and runs only `SyncV2RuntimeTest`. It fails on missing/failed instrumentation output and preserves Worker, logcat and instrumentation logs as a seven-day artifact. The existing test/lint/debug/release job remains unchanged. Workflow triggers also include the server/protocol/helper files used by the lifecycle test.

The helper's syntax and workflow YAML were checked locally. Its `--check` mode passed a real local seed, development-code login, active-subscription and enabled-v2 bootstrap check using an OS-assigned loopback port. The complete CI helper was then exercised locally with the rebuilt notification-aware JNI/APKs on the disposable emulator and passed the actual lifecycle test in 10.534 seconds; its logs are under `android/app/build/reports/sync-v2/`. The final shared-source and locked-reconciliation rerun then passed in 9.26 seconds. **The GitHub-hosted job has not been executed or pushed.**

Desktop, Android and iOS now prefer `changedBudgets` identities/revisions over `openChanged`, using the identity displayed when the report is consumed. The legacy boolean is used only when the targeted field is absent. Empty active-library transitions still refresh immediately. Android forwards exact sync-completion reports from WorkManager to a started Activity; resume retains its unconditional storage refresh. iOS also consumes the durable last report before beginning another foreground attempt, covering a headless background task that already applied changes. Focused tests cover switched identities, already-displayed revisions, deletion to empty, and legacy fallback; iOS equivalents require the macOS gate.

Final Android lint gate: `:app:lintDebug` passed with 0 errors, 32 warnings and 4 hints. Its report is `android/app/build/reports/lint-results-debug.html`. The gate caught and fixed missing explicit loopback-domain subdomain settings and a transaction-save busy-state read that was not observing StateFlow changes. Final debug JNI and both APKs were rebuilt against the key-reset, targeted-notification and read-only revision fixes before the 9.26-second runtime pass. The isolated emulator was stopped after verification; the AVD profile, APKs and report artifacts were retained.

The final native badge-only follow-up adds recovery and subscription-required phases to attention precedence, so queued changes elsewhere do not hide them. Kotlin compilation/JVM checks were repeated after that enum-list change; it does not change JNI or runtime sync behavior.
