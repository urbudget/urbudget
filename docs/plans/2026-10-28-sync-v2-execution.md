# Sync v2 implementation and validation record

Implementation authorized on 28 October 2026; validation continued on 29 October.
Work is on local branch `codex/sync-v2-redesign` in
`C:/Users/Admin/.codex/worktrees/sync-v2-redesign/urbudget`.
The original checkout remains untouched. No push, merge, tag, production migration,
deployment or account activation has been performed. Production v2 activation defaults off.

The implementation is available for review. Local protocol, storage, frontend and
Android runtime checks are passing. Xcode/Simulator, physical-device and staged
production acceptance remain release gates; this record does not declare the entire
release acceptance matrix complete.

## Follow-up release review

The [29 October release review](../reviews/2026-10-29-sync-v2-release-review.md) supersedes the readiness assessment and counts below. It fixes explicit private creation, reviewed-head deletion, locked deletion, billing/usage integration and Android release configuration. Deletion CAS no longer retries automatically: the newer version must be reviewed before another explicit delete. The expanded final native suite covers 25 scenarios and a 50,000-transaction snapshot.

## Implemented behavior

- An enabled account automatically owns new ordinary budgets, including while offline,
  locked or awaiting reauthentication. Explicit sign-out creates local-only budgets.
  Existing private budgets require a one-time explicit association.
- The account library discovers and downloads budgets automatically. Local, remote,
  evicted, trashed and recovery entries share the library. Deleting the last budget
  leaves a valid empty library through restart.
- Trash is durable intent, including offline. Restore creates a fresh incarnation;
  old uploads cannot overwrite it. An old dirty copy remains a separate recovery
  identity. Remove download refuses unacknowledged work and drafts.
- Each installation persists one immutable delivery slot and exact encrypted bytes.
  The server commits head, event, object adoption and receipt together. Local
  acknowledgement precedes sequence advancement. Unknown outcomes replay before
  publishing again; rejected/cancelled/expired outcomes never acknowledge content.
- Catalog events and authoritative status reconcile deletion explicitly. Missing
  listings or missing R2 objects never imply deletion. Version, generation and
  incarnation fences survive trash retention and publication pauses.
- A month's assignment amounts, extras and movement history merge as one conflict
  domain. Different months merge; competing plans for one month require review.
  Reviewed answers are tied to the values actually presented.
- Transactional local catalogs prevent lost registry updates. Storage migration uses
  separate v2 paths, SQLite backup/WAL handling, journals, OS leases and stale-writer
  SQL fences. Failed individual budgets remain recoverable without blocking the library.
- Editors persist raw incomplete fields and compact opening records. Stale/deleted
  record writes are rejected; recovery blocks unsafe adoption. Desktop close drains
  independent editor lanes. Recovery supports explicit review and re-entry of raw
  fields, rather than reconstructing an identical visual form.
- Desktop focus/connectivity/save triggers, Android WorkManager and iOS best-effort
  background work drive retries. Saved work survives an unavailable execution window.
  V2 library and receipt reconciliation continues while the content key is locked;
  content publication waits for unlock. Legacy scheduling retains its prior key gate.
- V2 sync reports carry local budget identities, exact saved snapshot tokens and a
  library version. A later upload error cannot erase a committed change notification.
  Consumers retain the old boolean fallback only for legacy reports.

Implementation is concentrated in `crates/urbudget-core/src/{budgets,store,sync_storage,sync_merge}.rs`,
`crates/urbudget-services/src/{library_api,cloud/v2,cloud/notifications}.rs`,
`web/src/sync-v2/`, and the platform library/editor/coordinator adapters.

## Concurrent import/transfer preservation

Baseline is main `068a3478e0684a8a0a94ba3be8723378d2fc68f8` plus 40 original
changed/untracked files. Original bytes and SHA-256 hashes remain under ignored
`target/sync-import-baseline/`, including `manifest.json`.

The preservation audit found **zero changed original-checkout files** against that
manifest. In this worktree **35 of 40 are byte-identical** to the copied baseline.
Three shared source files (the Arabic catalogs and desktop bridge) received sync
edits while retaining their import additions. The other two differences update the
design/implementation documents from planning status to the execution record.
Full Rust/frontend checks include the concurrent import atomicity, transfer-target
and assistant-import regressions.

## Validation evidence

- [Core storage and merge](../reviews/2026-10-28-sync-v2-core-validation.md).
- [Server and independent processes](../reviews/2026-10-28-sync-v2-server-evidence.md).
- [Platforms and Android runtime](../reviews/2026-10-29-sync-v2-platform-validation.md).
- [Actual Windows app smoke test](../reviews/2026-10-28-sync-v2-windows-evidence.md).
- [Raw Android instrumentation output](../reviews/2026-10-29-sync-v2-android-instrumentation.txt).
- [Protocol and rollback runbook](../../web/SYNC_V2.md).

The process suite uses three actual native service processes with different data
directories against local workerd, D1 and R2. Fault proxies disconnect devices, hold
requests and lose responses after server commits. The processes do not share a Store.

The expanded stress run completed **19 scenarios, 900 edits and 2,657 HTTP
requests**, plus a 5,000-transaction snapshot. The stress segment took **61.8 seconds**.
It verified convergence and individual transaction preservation, restart after lost
commit response, save during upload, same-month conflicts/stale answers, raw-draft
recovery, dirty remote deletion, restore, stale-incarnation upload, locked receipt
repair, subscription expiry, pending key reset, offline deletion undo, concurrent
restores, deletion CAS retry, eviction and empty-library restart. Native legacy
ciphertext/base migration and a late legacy device returning after restore also passed.
The 19th scenario retries a lost key-reset response after another device uploads into
the new generation and verifies the peer's data survives. After the final read-only
snapshot fix, all 19 scenarios passed again with three stress edits and explicit
remote-save/deletion notification assertions (837 requests).

The Android test launches MainActivity and observes the app's own WorkManager request
drain saved work after backgrounding. It also verifies checkpoint discard ordering,
active/last deletion, stale-save rejection, restore, eviction/download and signed-out
creation. It used only disposable AVD `CodexSyncV2Qa`, not an existing emulator or phone.
The final rebuilt JNI/APK run passed in **9.26 seconds**, including a locked-key phase:
the actual scheduled worker reconciled the catalog while financial changes remained
pending, then unlocked publication completed. The new CI helper itself performed
the local Worker setup, APK installation, ADB reverse and instrumentation invocation.

After the final core/notification fixes, `cargo test --workspace --lib --tests --quiet`
passed **594 tests, 2 pre-existing ignored**; desktop native `cargo test --lib --quiet`
passed **6 tests**. These include 394 core and 150 service unit tests, plus adapters
and integration tests. Unit counts alone are not the evidence for sync correctness.

Final frontend validation passed **690 tests across 78 files**, then version checks,
TypeScript and Vite production bundling. The build reports its large-chunk warning.
Worker TypeScript and **413 tests across 17 files** passed, including 36 v2 tests.
Final Android reports contain **136 JVM tests across 24 files, zero failures**, and
`lintDebug` reports **zero errors, 32 warnings and four hints**. Workflow YAML parses,
and `git diff --check` passes.

The actual Windows app passed empty-library creation, ordinary account editing,
hard termination/restart with raw unfinished transaction fields, recovery display,
automatic publication within five seconds, keeping an existing budget private,
restore/delete-last and empty-library restart. It exposed a missing status-label
crash in CloudSettings; the fix passed both a render regression and the repeated
actual UI flow. The final native debug executable was rebuilt before restart.

## Bugs caught during implementation

Validation led to fixes for dirty restored identities, restore/delete CAS races,
offline trash undo, interrupted R2 retry deadlock, abandoned creation/download repair,
partial catalog activation, stale refresh after committed merge, excessive checkpoint
baselines, obsolete checkpoint queues, leaf-editor close draining, deletion-status
rendering and needless cross-window save-token invalidation. The final audit
also caught a repeated key reset after a lost response; replay now completes the
original accepted operation without starting another reset. Redundant vault cleanup
cannot turn a durably accepted reset into a failure. These have targeted regressions
or retained independent-instance scenarios.

## Original review findings

| Finding | Implementation and verification |
|---|---|
| R1 reused remote revision | Monotonic versions, deletion fences and new restore incarnations; late-old-device scenarios in the real process suite. |
| R2 lost acknowledgement duplicates money | Immutable receipt replay and ordered local acknowledgement; response-loss/restart plus month-level financial merge regressions. |
| R3 discarded crash recovery | Recovery/checkpoints block unsafe adoption; real unfinished-field restart recovery and component tests. |
| R4 stale desktop editor | Opening-record guards, raw checkpoints and guarded incoming refresh; stale-choice/dirty-editor regressions. |
| R5 re-login resurrection | Stable account ownership and persisted ancestry survive expired sessions/sign-out; account-isolation and legacy migration tests. |
| R6 iOS deleted record resurrection | Native original-record existence checks; source and shared-native tests, with Xcode verification still required. |
| R7 late iOS original capture | Opening editor baseline captured before commit; iOS source change and pending macOS test gate. |
| R8 stale conflict answers | Fingerprints, identity/version and fresh local-token validation; real stale-answer scenario. |
| R9 misleading desktop status | Durable dirty/pending/library status and explicit local-only/trashed/recovery states; badge and render regressions. |
| R10 invisible Android attention | App-level status and unified library, backed by durable queue; JVM and emulator checks. |
| R11 lost committed-change notification | Identity-specific snapshot invalidations retained after subsequent failure, plus WorkManager-to-Activity forwarding; notification regressions. |
| R12 inflated concurrent assignments | Coherent month-level conflict domains; equal/crossed move and independent-month regressions. |
| R13 registry overwrites | Transactional fresh-row catalog mutations; independent catalog, migration and multi-process tests. |

## CI and remaining release gates

- `sync-v2.yml` adds Worker PR validation, real local D1/R2 checks, independent native
  stress and legacy migration. It uses no production credentials or remote command.
- Android CI adds a local-Worker emulator lifecycle check. iOS CI now also triggers
  for shared core/service changes. Hosted workflow runs have not been triggered by
  this local task; equivalent local validation is recorded separately.
- Xcode/Simulator and unsigned iOS device builds must pass. Physical Android/iOS
  connectivity, locked credentials/protected data, process termination and background
  expiration checks remain necessary. Emulator success does not prove OS delivery
  guarantees on every device.
- The 5,000-transaction fixture does not prove maximum-payload memory bounds on small
  phones. Maximum-supported payload profiling and UI responsiveness on release
  hardware remain release validation.
- Production metrics/alert wiring and thresholds, and staged restore/rollback drills,
  remain rollout work. No real account, live budget or production bucket was used.

Deploy the additive schema and compatible Worker before cutover. Keep
`SYNC_V2_ENABLED=0` until compatible clients and release gates are ready. After cutover,
roll back only to a v2-aware build. `SYNC_V2_PUBLICATION_PAUSED=1` pauses publication/key
controls while preserving reads, receipt replay, cancellation and trash. Never drop
receipt/tombstone fences, rewind D1 independently of R2, or reopen legacy writes.

## Integration

Changes remain uncommitted in the named managed worktree. Integrate after comparing
with any newer work in the original checkout; do not replace that directory wholesale.
The baseline preserves concurrent work as it existed when implementation began, not
future edits by another executor. Version/tag selection and production rollout require
a separate release decision.


## Independent blocker review follow-up

The [29 October blocker review and remediation record](../reviews/2026-10-29-sync-v2-release-blockers.md) supersedes earlier readiness statements and test totals. It adds actual 1.0.10 upgrade fixtures, interrupted sidecar recovery, stale displayed-review rejection, v1 ownership isolation, invalid delivery cancellation, per-budget failure isolation and server cleanup validation. Keep the existing cross-platform/rollout gates; a successful local test run is not production activation approval.
