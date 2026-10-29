# Release blockers: sync v2 (`codex/sync-v2-redesign`)

## Release follow-up — desktop 1.0.11

[Desktop 1.0.11](https://github.com/urbudget/urbudget/releases/tag/desktop-v1.0.11) is published from `a49aaf1fcf35f090adeaaaf1a6ff7ea1bbc26249`, with **v2 activation still disabled**. The [final release build](https://github.com/urbudget/urbudget/actions/runs/40914078901) and [App validation](https://github.com/urbudget/urbudget/actions/runs/40913977262) passed.

- The final installed Windows build passed **5/5 smoke tests**, including first-run titlebar geometry/hit testing and native save/restart; screenshots were inspected. This supersedes the earlier unsuccessful visual inspection below. The check found a missing CSS variable that made the first-run titlebar zero-height; [PR #10](https://github.com/urbudget/urbudget/pull/10) fixed it before publication.
- The macOS release is notarized. Updater signatures and GitHub asset digests were verified for the published release artifacts.
- [Native iOS validation](https://github.com/urbudget/urbudget/actions/runs/40906203041) passed both Simulator tests and the unsigned device build. Hosted shared Rust, Android/lifecycle and sync-protocol checks also passed on `16ecc36`; their product code is unchanged in the final release, whose only subsequent product change is the titlebar CSS declaration.

Physical-device mobile validation and v2 staging, restore/rollback and production-monitoring checks remain gates for their respective releases or activation. This desktop publication does not enable v2 or establish mobile/store availability. Publication and distribution status are tracked in [RELEASE-STATUS.md](../RELEASE-STATUS.md).

## Remediation status — 29 October 2026

Codex reviewed the findings against the current worktree and reproduced the upgrade, ownership, conflict-review and queue-isolation failures. The historical review below is retained; its original “still present” statements describe the pre-remediation source.

The local validation snapshot below precedes the published release. Its historical results and limitations are preserved; the release follow-up above records the subsequent hosted and installed-build verification. Physical mobile and staged-rollout checks continue to gate mobile release and v2 activation.

| Finding | Current disposition |
| --- | --- |
| C1 | Fixed. Probe bootstrap before installation registration; old email-only sessions backfill authenticated account identity. Missing/offline v2 routes retain v1 setup for legacy accounts. Persisted protocol 2 cannot downgrade to v1. |
| C2 | Fixed. v1 uses a separate existing store when nothing is open; absent files are never silently initialized by sync. |
| C3 | Fixed. Migration preserves assistant history/attachments and other budget sidecars, excluding database internals and global account credentials. |
| C4–C5 | Fixed. Verified database publication is independent of sidecar copying. Flushed temporary copies and per-file retry journals preserve newer destination edits. Retryable failures retain lifecycle/current intent; successful retry restores usability. |
| C6 | Fixed. Inactive v1 accounts cannot register/prepare while activation is off; rejected preparations allocate no object or garbage rows. |
| C7 | Fixed in implementation. Durable first-run state drives Welcome; deleting the final budget still leaves an empty library. Platform regression verification is recorded below. |
| C8 | Explicit rollback policy documented in `docs/RELEASE-STATUS.md` and the desktop 1.0.11 changelog. Only a v2-aware client may roll back upgraded storage. No downgrade conversion was introduced. |
| A1 | Fixed. Legacy tracking, upload, merge, download, resolution and removal reject account-owned or inactive library entries. |
| A2 | Fixed. All platforms echo the displayed token/version/identity and conflict fingerprints. Background replacement of the saved review rejects the stale dialog. Bulk choices cover only echoed keys. |
| A3 | Fixed. Month plans show named category deltas, base/side totals, effect on Ready to Assign and move count in their own currency. |
| A4 | Fixed for bounded payloads, malformed candidates and reconciliation isolation. Oversized ciphertext fails before sequence allocation. Invalid immutable candidates cancel through an authenticated terminal server receipt. Other unresolved deliveries persist an attention state; reads/catalog reconciliation continue. An unknown server sequence is preserved for repair, never guessed or silently skipped. |
| A5 | Fixed. Transaction source account and reconciliation state join the monetary conflict group. Remaining cross-record validation failures become fingerprinted explicit whole-budget choices, preserving both valid versions for review. |
| A6 | Fixed. Replaced/cancelled ciphertext is reclaimed inline; maintenance uses bounded bulk batches and account deletion targets that owner's keys. R2 failures retain durable retry work. |
| A7 | Fixed. Lifecycle errors are recorded per job; unrelated work proceeds. Chosen money-move order is retained. v2 download no longer changes the selected budget. The earlier deletion-review fix remains in place. |

Additional fixes cover local open/lifecycle responsiveness during sync, managed-library status reads, protocol-specific sync cadence, eligible Android background scheduling, checkpoint cleanup, missing-current-file protection, conservative rediscovery after catalog loss, preserving active unsaved drafts across unrelated library actions, light open snapshots, mobile report/expiration handling, category references, Google refresh at commit, metadata validation, installation retirement, and the deployment migration dry-run. Final integration evidence and remaining presentation work follow.

### Verified evidence

- Actual 1.0.10 data and binaries upgraded through v1 coexistence and activation: **42/42 checks passed**. Includes assistant history, unfinished recovery, interrupted/blocked copies, newer edits during retries, old email-only sign-in and every transaction. Driver: `target/sync-v2-release-review/upgrade-current.mjs`; results: `target/sync-v2-release-review/upgrade-1793273089000/results.json`.
- Worker TypeScript + **426 tests across 17 files** passed; real workerd protocol harness and all **12 local D1 migrations** passed.
- First expanded native stress run: **27 scenarios passed**, **900 deterministic edits**, **50,000 extra transactions**, **3,207 HTTP requests**. This includes background review replacement, blocked-delivery reads across restart and cancellation of invalid candidates. Trace: `crates/target/sync-v2-instances/1793273036634/trace.json`.
- Extended legacy run passed, including old session backfill, missing v2 routes, sync with no open budget, migration and a returning old device: **103 HTTP requests**. Trace: `crates/target/sync-v2-instances/1793273346787/trace.json`.

### Local integrated checks (before hosted release validation)

| Check | Result |
| --- | --- |
| Rust workspace, locked dependencies, library/integration tests | **617 passed, 2 pre-existing ignored** |
| Desktop production frontend build | Passed |
| Desktop UI | **706 passed, 79 files** |
| Windows Tauri native tests/build | **6 passed**; debug executable built |
| Windows actual-window onboarding smoke | **Unverified**: native UI state capture hung and was stopped before returning a screenshot/state |
| Worker TypeScript and tests | **426 passed, 17 files** |
| Android JNI/Kotlin/debug and instrumentation APKs | Built successfully |
| Android JVM | **139 passed, 25 classes** |
| Android lint | **0 errors; 32 existing warnings and 4 hints** |
| Android API 37 emulator on rebuilt final JNI/APK | **SyncV2RuntimeTest passed, 6.89 seconds**; source/artifact hashes verified |
| iOS translation catalogs | **2,106 strings checked**; this is not a Swift build |
| Node version/CI tooling | **11 passed** |
| Final native stress/fault suite | **29 scenarios passed**, 900 independent edits, 50,000 extra transactions, **3,314 HTTP requests** |
| Formatting/patch whitespace | `git diff --check` passed |

The final stress trace is `crates/target/sync-v2-instances/1793274794983/trace.json` (`target/sync-blockers-stress-final.log`). It additionally proves that one unreadable local cache does not prevent unrelated downloads, that download preserves the selected budget across restart, and that the remote version discarded by an explicit whole-budget choice remains a readable portable backup. Both sides of that exceptional choice are protected: the selected version becomes current, and the losing version is backed up before mutation. Backup failure aborts resolution, including a backwards-clock retention failure.

Rust evidence is `target/sync-blockers-rust-final.log`; the affected **169 service tests** were rerun successfully after the final lifecycle-error cleanup (`target/sync-blockers-services-final.log`). Android evidence is `android/app/build/reports/sync-v2/{instrumentation.txt,source-evidence.json,final-gradle.txt,logcat.txt}`. Its last build recompiled no crates, the tested APK/JNI matched the source fingerprints, and logcat had no fatal exception/ANR. The disposable emulator and Worker were stopped. The earlier review's test totals are historical and do not describe this integrated source.

The additional fresh-profile Windows launch built successfully, but the native UI inspection tool hung for 867 seconds without returning pixels or accessibility state. No clicks, typing or deletion were attempted. Only the verified disposable app/Vite processes were stopped. Evidence: `target/sync-v2-release-review/windows-onboarding/1793274115361/results.json`. C7 has automated onboarding coverage; this attempted visual check is not a pass.

### Remaining work and release gates

- Presentation follow-ups: recovery still shows raw JSON with manual re-entry; the recovery action remains visible on rows without a draft; legacy accounts still see the automatic-discovery notice. Some new service error text remains English. These were left explicit rather than hiding recoverable data.
- An unknown installation sequence or a storage-provider outage cannot be safely guessed away. The app preserves the delivery and reports attention while continuing eligible reads; cleanup failures remain queued durably. Catalog loss deliberately exposes preserved files as recovery copies rather than reconstructing unprovable installation receipts/ownership.
- Windows onboarding/titlebar visual checks, Xcode/Simulator tests, the unsigned iOS device build and final macOS desktop build have since passed, as recorded in the release follow-up. Simulator/build success does not establish physical-device background correctness.
- Physical Android/iOS process termination, protected data, changing connectivity, background expiration, and maximum-payload memory/responsiveness checks remain required before their rollout gates are cleared.
- Keep activation off until staging/restore/rollback and production monitoring checks pass. At the time of the local validation snapshot, no release tag/version or production changes had been made. The branch also contains the concurrent bank-import feature described in the original review; include that scope in release review.


---

## Original independent review

Independent review by Claude Code, 29 October 2026. **Not ready to merge or release.**

This is a separate review from `2026-10-29-sync-v2-release-review.md`. That document calls the worktree a locally validated release candidate, but its checks don't cover the defects below.

- **Client update:** seven confirmed defects block shipping it (C1–C7), plus one rollback decision (C8). They hit people on today's v1 sync and the on-device upgrade, even with `SYNC_V2_ENABLED` off.
- **Before v2 activation:** six more defects (A1–A6) must be fixed before any account is moved to v2.

The core v2 protocol itself held up well:
- the commit and receipt design, deploy safety and the money arithmetic of the merge reviewed sound;
- Codex's 19-scenario stress suite passes;
- old 1.0.10 clients keep working against the new Worker.

No production service, branch, tag or remote content was changed.

## What was reviewed

- All the work is uncommitted in Codex's worktree `C:/Users/Admin/.codex/worktrees/sync-v2-redesign/urbudget`. Its base is main `068a347`.
- Codex was still editing that worktree during this review. The review used a frozen copy of its state at 09:16: ref `refs/review/sync-v2-baseline` (`a837c0e`), checked out at `.claude/worktrees/sync-v2-review`.
- **Rechecked at 14:00 against this worktree as Codex left it at 09:47** (no code has changed since then). Codex's later changes covered deletion review, local-only scope, Google refresh at bootstrap, usage and admin.
  - Fixed since the snapshot: the A7 trash-retry item (now a `deletion_conflict` review) and the `rustDebug` Gradle guard.
  - Still present: everything else. C1–C7 and A1–A6 were spot-checked in the current source.
  - Partly fixed: Google entitlement is now refreshed at bootstrap, but commit still doesn't refresh it.
- Line numbers refer to the 09:16 copy. In the current tree, `budgets.rs` lines after 331 have moved down by about 39 (for example, `:907` is now `:946`), and `cloud/v2.rs` lines after 1314 have moved by 20–100. Search by function name.
- The branch also contains the uncommitted bank-import work from the main checkout. That work is byte-identical to main's working tree, so releasing this branch releases that feature as well.

## Evidence

All local; no production endpoints.

| Check | Result |
| --- | --- |
| Rust workspace `cargo test --workspace --lib --tests` | Pass (394 core, 150 services, adapters, integration) |
| Desktop native `cargo test --lib` | Pass (6) |
| Worker `npm run check` | Pass (413 tests, 17 files) |
| Desktop `tsc -b` + Vitest | Pass (690 tests, 78 files) |
| `sync-v2-protocol.mjs` (real workerd, D1, R2) | Pass |
| Codex's three-process suite `sync-v2-instances.mjs --rounds 300` and `--legacy`, rerun independently | Pass: 19 scenarios including 900 concurrent edits, plus legacy ciphertext migration |
| **1.0.10 client's own v1 suite against the new Worker, v2 off** (25 scenarios, 447 syncs) | Pass: old clients keep working after the Worker deploys |
| Migration 0012 on a database populated by 1.0.10 | Applies cleanly |
| **Upgrade run:** 1.0.10 data and v1 account → v2 client → mixed fleet → activation | 26 of 33 checks pass, and every transaction survived end to end. The real failures are C1 and C8; the others were harness assumptions. |
| Real desktop window on 1.0.10 data (QA harness) | Three budgets migrate, the deleted one stays deleted, and the 1.0.10 recovery draft restores with its unsaved row. A fresh install fails (C7). |

The upgrade driver is `C:/Users/Admin/Documents/GitHub/urbudget/.claude/worktrees/sync-v2-review/desktop-qa/upgrade.mjs`, and its log is `upgrade-run.log` in the same folder. It builds `sync_qa` at `068a347`, writes real 1.0.10 data and v1 server state, and replaces the binaries in place. Screenshots cited below are in that folder too.

Rerun the driver against this worktree after fixing C1, C3–C5 and C8. It is the only check here that starts from real 1.0.10 data.

## Blockers for shipping the client update (v2 still off)

These reach users the day the update installs. Line numbers refer to the frozen copy.

### C1: Upgraded users can't set up or unlock sync

**Where:** `cloud/v2.rs:717-726` (`try_setup` calls `connect()` before looking at the bootstrap) and `:443-445`. `sync.rs:729` and `:790` call into it.

1.0.10 writes `account.json` with no `accountId`. `connect()` needs one, so `sync_setup` fails with "Sign in to your UrBudget account first." This happens even with v2 off, and the window still shows the person as signed in. Two groups are hit:
- Any upgraded user who still has to enter the passphrase: first-time setup, re-unlock after `sync_lock`, or a key changed on another device.
- Against a Worker without `/api/v2` (today's production, or a Worker rollback), a person signed in with the new client gets `not_found` from `POST /installations` instead of the v1 path.

**Confirmed twice:**
- In the upgrade run, both upgraded devices got this error from `sync_setup` while v1 sync was otherwise working.
- The client reviewer's fake-Worker test reproduced it with v2 off, with no request made.

**Fix:**
- Return `Ok(None)` (the v1 path) unless bootstrap says protocol 2 or activation is enabled.
- Treat a 404 or unreachable v2 as v1.
- Backfill `accountId` for 1.0.10 sessions from the bootstrap or `/me`.

### C2: v1 sync fails for every budget when no budget is open

**Where:** `cloud/sync.rs:212`. `ctx.db.store()?` errors with "Open or create a budget first."

Deleting the open budget, and an empty library, are now normal states. From then on every v1 pass reports each tracked budget as `error` and uploads nothing.

**Confirmed** by a client-review test. **Fix:** have v1 `with_store` open a separate `Store` when none is open.

### C3: Assistant history disappears after the update

**Where:** `budgets.rs:907`. The migration copies only `recovery.json`, `backups`, `imports` and `exports`.

Threads, attached statements and pending proposals live in `<budget folder>/assistant` (`assistant/history.rs:85`). After the update the app reads them from the new `v2/budgets/<id>/` folder, so every user's chats vanish. They stay orphaned on disk.

**Confirmed** by a storage-review test and the source. **Fix:** copy everything in the legacy folder except the database and its `-wal`/`-shm`.

### C4: An interrupted migration copy hides a budget for good

**Where:** `budgets.rs:151-181`.

`copy_preserved` writes side files straight to their final names. On retry, a half-written file with a different hash is a hard error. If a user kills the app mid-copy, or the disk fills, the budget stays `recovery`/`unavailable` on every later launch:
- `database()` points at the retired 1.0.10 file.
- Saves fail with "This budget was removed from its account…".
- The only option offered is "Recover as a copy", which loses backups and v1 linkage.

Migration runs synchronously in desktop `setup` with no progress UI. A user with many backups sees a hung app and kills it.

**Confirmed** by a storage-review test: three loads in a row stay wedged. **Fix:** copy to a temporary name, fsync, then rename. Recopy files the job itself was writing, and never let side files block publishing the database.

### C5: A one-off lock or disk error leaves a budget stuck as "recovery"

**Where:** failure path at `budgets.rs:970`. The success path at `:927-931` never restores `lifecycle`.

A file lock held for more than 3 s on first launch (antivirus, a still-running 1.0.10, a sharing violation) or a full disk demotes the budget. The next launch migrates it fine, but it stays `lifecycle='recovery'` with no current budget. Desktop (`BudgetDialogs.tsx:174-176`), Android (`LibraryScreen.kt:107-118`) and iOS (`LibraryView.swift:186-220`) all hide Open for recovery rows and offer only "Recover as a copy".

**Confirmed** by a storage-review test. **Fix:**
- Keep the original lifecycle in the job and restore it, plus `current_id`, on completion.
- Track migration health in `status`, not `lifecycle`.
- Treat lock and disk-full errors as retryable.

### C6: Merging deploys an open v2 endpoint that floods cleanup

**Where:** `web/src/sync-v2/repository.ts:148-151` and trigger `0012:104-107`.

With `SYNC_V2_ENABLED` unset, any signed-in free account can `POST /installations` and then send create manifests to `/mutations/N/prepare`. Each one is rejected, but it leaves an object row and a `sync_object_garbage` row behind. That is up to 1,200 per hour per account. The hourly sweep takes the 100 oldest rows across all users, so real v1 orphans and deleted accounts' ciphertext wait behind the junk.

**Confirmed** by a server-review test: 150 prepares queued 150 rows, and one sweep drained 100. **Fix:**
- Reject `installations` and `prepare` for accounts that are neither protocol 2 nor allowed to activate.
- Insert the object row only for an `ok` decision.

### C7: Desktop fresh install skips onboarding

**Where:** `Budgets::load` no longer creates a first budget, so `App.tsx:457-468` shows a bare "Budget library" page instead of the Welcome flow (sample budget, YNAB import, add account). `NewBudgetDialog` with no current budget defaults the currency to SAR. The page's heading also collides with the app icon in the title bar.

New users and Store certification reviewers see this first.

**Confirmed** in the real debug window on an empty data folder (screenshot `desktop-qa/fresh-install.png`). Codex was editing `Welcome.tsx` during the review; check its final state.

**Fix:** show Welcome when the library is empty and has never had a budget. Keep the empty library page for "deleted the last budget".

### C8: Release decision: 1.0.10 can't start on upgraded data

After the update migrates a device, 1.0.10 refuses the databases: "This database was created by a newer UrBudget version." `sync_qa` at `068a347` panicked at start, and desktop 1.0.10's `setup` fails the same way, so there is no window and no in-app updater.

No data is lost, and the fences stop old processes writing to v2 data. But a rollback build, a reinstall of an old installer, or an older second copy is unusable.

**Confirmed** in the upgrade run. **Before release:** decide this explicitly, ship rollbacks only as v2-aware builds, and say so in the release notes.

## Blockers before enabling v2 for any account

### A1: Account A's budgets upload to account B through the v1 engine

**Where:** `cloud/sync.rs:751` and `:797` (`track` every `library.list()` entry), and `:608` (v1 `run` doesn't filter by owner).

A owns v2 budgets. A signs out, B (still on v1) signs in and sets up sync. The `PUT /api/v1/sync/budgets/<A's id>` requests upload A's budgets into B.

**Confirmed** by a client-review test. **Fix:** every v1 entry point (`run`, `track`, `sync_track`, `sync_setup`, `sync_reset`, `push`, `bring_in`) must skip entries that have an `account_id`.

### A2: A conflict answer can apply to values the user never saw

**Where:** `cloud/v2.rs:2083-2130`, and the review is overwritten at `:1500-1511`.

The UI sends only `{id, keep, choices}` (`desktop/src/cloud.ts:76`). `resolve()` checks token, version and fingerprints against whatever review is stored now, and each background pass replaces it. A newer remote head that arrives while the dialog is open becomes the thing the user's "keep this device" applies against.

**Verified in source.** **Fix:**
- The client echoes the displayed review version and token plus each `{key, fingerprint}`.
- `resolve()` rejects anything else.
- Bulk keep covers only the echoed keys.

### A3: Month-plan conflicts can't be reviewed

**Where:** `sync_merge.rs:683-696` sends the raw `{assignments, moves}` block. `desktop/src/SyncConflicts.tsx:131-134` (unchanged by the branch) renders any non-flat object as "Edited".

Both sides read "Edited", and Android dumps raw JSON with category ids. Choosing a month's whole plan blind can drop the assignments the user meant to keep. Design §8 requires per-category deltas and totals.

**Verified in source.** **Fix:**
- Build a shown value in `monetary()`: per-category base → local / base → remote with names, the month total, the effect on Ready to Assign, and the number of moves.
- Render it on all three platforms.

### A4: One stuck mutation stops all sync for the account

**Where:** `deliver` at `cloud/v2.rs:480-551`, awaited with `?` at `:1787`.

Any permanent prepare error aborts every later pass before catalog, deletion, download or other uploads. Examples: ciphertext over 50 MiB (`seal` never checks the size), `installation_required`, or a sequence error. The server's cancel also rejects the manifest, so cancelling doesn't escape it. Meanwhile `decorate_status` shows "waiting"/"queued".

**Confirmed** by a client-review test. **Fix:**
- Check the size before allocating a sequence.
- Give the slot a durable "needs attention" state with cancel or quarantine.
- Keep reconciling while blocked, and report it honestly.

### A5: A merged result that fails validation stops the budget syncing

**Where:** `sync_merge.rs:1194-1197`. The transaction money group at `:109-113` leaves out `accountId`.

Example: one side moves a transfer's source account to `wallet` while the other moves its target to `wallet`. The merge returns `Err("…A transfer must connect two different accounts.")`, and the budget sits in "error" permanently, with no conflict offered.

**Confirmed** by a merge-review test. **Fix:**
- Group `accountId` with the transfer fields.
- Turn any post-merge validation failure into a conflict on the offending record.

### A6: v2 never cleans up replaced copies inline

**Where:** `finish()` doesn't reclaim objects.

Every replaced, cancelled or expired copy waits for the global sweep of 100 per hour, about 2,400 a day. R2 grows without bound, and account deletion no longer removes ciphertext in bounded time.

**Confirmed** by a server-review test. **Fix:**
- Reclaim the replaced key in `finish()`.
- Make the sweep time-budgeted and bulk.
- Have account deletion target that user's keys.

### A7: Other fixes needed before activation

- **One failing lifecycle job aborts the whole pass:** `cloud/v2.rs:1637`, `:1686`, `:1724`. Record the error on that job and continue. Reasoned.
- **Re-sorting the move history creates false month conflicts:** sorting every move by `at` (`sync_merge.rs:724-729`) changes month blocks nobody touched when device clocks disagree. **Confirmed.** Keep each chosen block's order.
- **`sync_download` changes the selection:** in v2 it persists the selected budget without opening it (`sync.rs:812` with `library_api.rs:154`). After a restart the wrong budget opens, and `editor_checkpoint` without a `budgetId` targets it. **Confirmed.**
- ~~**Trash retried against a head the user never saw**~~: fixed after the snapshot by pinning `reviewedHead` and adding the `deletion_conflict` review.

## Should fix

Fix these before the client release unless noted.

**Responsiveness**
- Opening, deleting, creating or evicting a budget waits for any running sync. `v2::open` (`cloud/v2.rs:2012`) and `library_api::lifecycle` (`library_api.rs:184`) take `cloud.sync.operation()`, which a v1 pass holds across HTTP calls (15 s connect, 120 s request).
  - Confirmed: with a 3 s slow request, `open_budget` waited 2.5 s and a queued `load_budget` waited 2.3 s.
  - On the phones this stalls the serial edit queue.
  - Take the lock only when a download is needed.
- Desktop `cloud_status` is synchronous and runs `Budgets::load` twice. That means an IMMEDIATE transaction, `budgets.json` parsing, the migration lock and a retry of failed migrations, all on the UI thread after every save and every 30 s, for every signed-in user. Make it async and use the managed `Library`.
- v1 server traffic rises 10–20× while v2 is off:
  - sync every 30 s (was 5 min), on focus after 5 s (was 60 s), 2 s after a save (was 8 s);
  - each pass adds `GET /api/v2/sync/bootstrap`.
  - Keep the v1 cadence until the account is on protocol 2, or confirm production capacity and rate limits.
- Android schedules WorkManager work (15-minute periodic plus a one-shot per save) for every user, including signed-out and local-only ones, and never cancels it (`NativeServices.kt:53`, `SyncWorker.kt:29-34`).
- The bank-import draft checkpoint serializes the whole statement on every change: `BankImportDialog.kt:157-159`, `BankFileImportView.swift:137`, and desktop's `[values]` effect, up to 16 MiB per keystroke. Checkpoint the session and decisions, not the file content.

**Correctness and data safety**
- A missing current file becomes a blank budget. `budgets.rs:921` renames without a directory fsync, and `runtime.rs:208` and desktop `lib.rs:568` open the current budget without an existence check, creating "My budget". Reasoned.
- Losing `library.sqlite3` is never repaired from `v2/budgets`. Natively created budgets are orphaned and migrated ones come back as recovery. Confirmed.
- Android can import months-old `filesDir/budget.json` as the current budget when C4 or C5 leaves no current budget (`SqliteBudgetStorage.kt:27-41`). Reasoned.
- Editor checkpoints leak. `editorCheckpoint.ts:59` starts a new session on a kind change without discarding the old one, and row editors unmount without discarding. Confirmed by test.
  - Harmless on v1.
  - On v2, a leftover checkpoint blocks incoming changes and "Remove download" for that budget indefinitely, so this is an **activation blocker**.
- Library actions reload the open store (`library_api.rs:171`), replacing an unsaved draft the window still believes Rust holds.
- `open_local` sends the full budget, including `sourceExport` and each transaction's `source`, instead of the light snapshot (`library_api.rs:153-157`).
- iOS: both background-expiration handlers finish asynchronously (`SyncCoordinator.swift:95-97`, `:194-197`). Apple requires `endBackgroundTask` / `setTaskCompleted` before the handler returns, or the app can be killed.
- iOS re-applies the stored last report on every 30 s refresh, reloading the budget (`SyncCoordinator.swift:56-60`). Android consumes its replayed report twice.
- Reconciled transactions can silently take the other side's new amount (`sync_merge.rs:109-113`). This predates the branch.
- Payee rules, default categories and loans are not considered when restoring a deleted category in a merge, leaving dangling references (`sync_merge.rs:1004-1083`).
- `merge(b, b, r)` is not exactly `r` (assignment order, empty `moneyMoves`), so download-only devices echo an upload.

**Visible to users**
- "Sync paused" shows as an alert for signed-in users who never set up sync: desktop `CloudBadges.tsx:24-26` (confirmed), the Android chip, and the iOS menu. Every v1 user also sees the "Automatic budget discovery is not enabled" notice.
- Every budget row shows "Recover unfinished edits" even with nothing to recover. It opens "No unfinished edits." (seen in the real window.)
- Recovery screens show raw JSON on all platforms, including whole budgets for `closed-budget-draft`.
- Untranslated text:
  - Rust library statuses ("Saved on this device", "In Trash", "Waiting") are English literals in `budgets.rs`, and iOS shows them verbatim.
  - New Rust errors aren't wrapped in `t!`.
  - The iOS library footer key is missing from `Localizable.xcstrings`.
  - Android checkpoint error fallbacks are hard-coded English.

**Server, before activation**
- The 100-installation limit is permanent, with no retirement or admin path.
- Commit doesn't refresh Google entitlements, so a renewal between prepare and commit is rejected. Bootstrap now refreshes them; commit still doesn't.
- `payload.meta` must be base64; a non-Latin-1 value makes the content route throw.
- Maintenance does a full-table `min_cursor` rewrite and one D1 query per inventoried object.
- `deploy.yml` applies remote migrations after only `npm run check`. Add a `wrangler d1 migrations apply --local` dry run.

**Adapters and CI**
- `#[cfg(target_os="android")]` slid onto the new `INITIALIZE` in `crates/urbudget-android/src/lib.rs:460-461`.
- ~~The `rustDebug` Gradle guard is bypassed by `build`/`assemble`~~: fixed after the snapshot.

## What was sound

- **Server deploy:**
  - migration 0012 applies to populated data;
  - no account can reach protocol 2 with the flag off;
  - the fence triggers don't fire for v1;
  - account deletion still cascades;
  - verified independently by the server reviewer with wrangler and by the 1.0.10 suite.
- **Commit protocol:** single-batch atomic commit, receipt rules, authorization scoping, reference-checked garbage collection, and contiguous events with 410 on compaction.
- **Client delivery slot:**
  - exact bytes persisted before send;
  - acknowledgement before sequence advance;
  - replay at every crash point;
  - rejected or expired outcomes keep edits dirty;
  - AAD contexts match the server.
- **Merge arithmetic:** a 3,000-run randomized check found no invented, summed or double-applied money. Unknown fields are preserved.
- **Storage fences:** the v3 triggers block old writers, including statements prepared before migration. Deletion touches only validated managed paths. Dirty tracking has no bypass.
- **Phones:**
  - release builds carry no cleartext or QA endpoint;
  - BGTask identifiers match;
  - every new Android string has Arabic;
  - JSON shapes match the native bridge;
  - no force unwraps on the empty library;
  - no likely iOS compile failure, though Xcode was not available.

## Not verified

- **iOS:** the Xcode, Simulator and device builds, background behaviour, and Swift type-checker time on the large JSON literals.
- **Android:** a Gradle build of the frozen copy (the disk is nearly full); the emulator lifecycle test was not rerun.
- **Codex's newer state:** the changes Codex made after 09:16 were read and checked against these findings, but not tested.
