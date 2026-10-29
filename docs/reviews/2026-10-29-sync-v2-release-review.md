# Sync v2 release review — 29 October 2026

## Disposition

This was the initial review. The independent [release-blocker review and remediation record](2026-10-29-sync-v2-release-blockers.md) found upgrade and account-isolation failures missed by these checks and supersedes this disposition. The evidence below records the earlier source state, not release sign-off. Production activation and release remain gated on the current integrated checks and host/device rollout requirements.

Reviewed `codex/sync-v2-redesign` at `C:/Users/Admin/.codex/worktrees/sync-v2-redesign/urbudget`, based on `068a3478e0684a8a0a94ba3be8723378d2fc68f8`. The redesign and review fixes remain uncommitted in that worktree. The separate main checkout was not modified by this review. No push, merge, tag, production migration, deployment or account activation was performed.

Three independent reviews covered the Rust storage/services, Worker protocol/billing integration, and desktop/Android/iOS adapters. The checks below were rerun during this review; earlier execution evidence alone was not treated as a release sign-off.

## Findings fixed

| Priority | Reproduced issue | Result and coverage |
| --- | --- | --- |
| P1 | Both native creation handlers ignored the UI's `localOnly` flag; sample metadata also needed a consistent policy. A supposedly private budget could enroll in account sync. | Shared `create_with_scope` enforces explicit private creation and boolean/object sample metadata. Runtime and Tauri call it. Failing native reproduction, bridge tests, independent-client restart/sign-out/account-switch tests, and actual Windows UI/native SQLite verification pass. |
| P1 | Trash used the newest server head rather than the version the user reviewed, then automatically retried rejected deletion CAS operations. | Trash captures reviewed version/incarnation/generation/epoch. Newer heads create durable `deletion_conflict`; all platforms offer Review newer version. Review preserves saved edits/checkpoints and opens the current budget before another explicit deletion. Offline-head advancement, in-flight CAS, restart and SQLite rollback tests pass. Remote-observed tombstones and restored incarnations cannot manufacture or inherit old deletion intents. |
| P1 | Android's literal task-name guard let aggregate Gradle tasks package debug QA Rust into release/preview variants. | Guard checks the resolved task graph, including up-to-date native tasks. Unsafe aggregate build is rejected; explicit debug instrumentation and normal release task graphs remain permitted. |
| P2 | A locked content key stopped metadata-only queued trash/purge operations. | Receipt recovery still runs first; eligible deletion proceeds while locked. Restoration waits for an unlocked key. Independent clients verify remote trash, deferred restoration and restart. |
| P2 | v2 bootstrap returned stale Google Play entitlement, while clients used that response to decide whether to publish. | Bootstrap uses the existing bounded, cached renewal refresh. Renewed and unavailable-provider regressions pass. |
| P2 | Account/admin usage queried only the emptied legacy table after migration. | Counts include v2 active budgets, retained trash bytes and v2 key configuration. Lifecycle, account-isolation and admin tests pass. |
| P2 | Sample navigation could select trashed/recovery entries; the new review action needed the normal save/switch barrier and dismissal of the library. | Sample filtering, desktop checkpoint/save ordering and iOS ordered service calls are covered. The actual Windows review flow opens the newer transaction and closes the library. Library names/status now have their own full-width row with wrapping actions. |

The deletion reconstruction fix initially exposed a remote-restore regression in the independent-client suite. It was corrected and the complete suite rerun successfully; the earlier failed run is retained in local evidence.

## Final validation

| Check | Result |
| --- | --- |
| Rust workspace, locked dependencies, library/integration tests | **597 passed; 2 pre-existing ignored** |
| Desktop TypeScript/version checks/Vite production build | Passed; existing large-chunk warning remains |
| Desktop UI | **696 passed across 78 files** |
| Desktop native | **6 passed**, final Windows debug executable built |
| Worker TypeScript/full test suite | **417 passed across 17 files** |
| Android compilation/JVM/lint | **136 JVM tests**, no failures; lint 0 errors, 32 warnings, 4 hints |
| Android rebuilt JNI/APKs and actual API 37 emulator | **SyncV2RuntimeTest passed in 8.482 seconds** on corrected final Rust source |
| Independent native-client failure/stress suite | **25 scenarios passed**, **900 concurrent edits**, **50,000 additional transactions**, **2,996 HTTP requests** |
| Native legacy upgrade and returning old device | Passed, **90 HTTP requests** |
| Real local D1/R2 protocol suite | Passed: multi-object migration, immutable receipt replay, SQL old-writer fences, restore, R2 crash retry and key reset |
| Version tooling and CI check selection | **11 passed** |
| Resource parsing and `git diff --check` | Passed |

The final independent-client stress segment took **93.0 seconds**; the 50,000-transaction convergence segment took **20.7 seconds**. These are Windows debug/local-service timings, not release-phone benchmarks. The suite additionally covers private creation, account switching, locked receipt repair, raw editor recovery, lost responses, saves during uploads, stale money-conflict answers, locked trash, explicit deletion review, expired entitlement, key-reset response loss, eviction and empty-library restart.

### Throttle and 3,000-edit recovery

An initial 3,000-edit run reached the unchanged **1,200 prepare attempts per account per hour** limit. Its immediate convergence assertion failed because all clients were correctly receiving HTTP 429. This is not reported as an uninterrupted stress pass.

Before changing any state, read-only SQLite inspection confirmed the union of the three devices contained **all 3,000 expected transaction IDs**, with one durable pending upload per device. After restarting the same clients and simulating the next rate window only in the disposable local database, every budget converged exactly: **3,010 total transactions, each of the 3,000 stress IDs present once, no pending work**. Recovery took 8.5 seconds. The retained suite now has an explicit throttle/restart/next-window regression. Production rate limits were not changed.

### Payload boundary

A real local Worker accepted **52,428,800 bytes (50 MiB)** and returned matching length/SHA-256 on download. A descriptor one byte over the limit and a chunked stream one byte over were rejected; the overrun could not publish and its sequence could be cancelled safely. Upload/download took 1,257/167 ms locally.

The first request following the rejected oversized stream received a Miniflare internal HTTP 500 (`Network connection lost`); a bounded retry succeeded. This transient is retained in the evidence, not hidden. Local workerd does not prove production isolate memory behavior or small-phone peak memory bounds.

### Actual platform behavior

The Android test used the app's WorkManager scheduling after backgrounding, including locked catalog reconciliation, queued save drain, draft epochs, deletion of the last budget, stale-save rejection, restoration, eviction and signed-out creation. The JNI/APKs were rebuilt after the final core correction; source hashes were unchanged during the run. The disposable emulator was stopped afterward.

The real Windows executable used only synthetic local accounts and separate SQLite/WebView profiles. Creating a budget through the checked "only on this device" UI resulted in `account_id=null`, `remote_id=null` and "Saved on this device". A separately prepared two-device conflict displayed Review newer version; clicking it closed the library, opened the newer transaction, changed the displayed balance from 1,000.00 to **998.50 SAR**, and returned to Synced. No real user budget or production account was used.

## Retained local evidence

These ignored artifacts remain in the review worktree; rerun the checked-in harnesses when sharing the source alone.

- Final 25-scenario trace: `crates/target/sync-v2-instances/1793256009229/trace.json`.
- Final legacy trace: `crates/target/sync-v2-instances/1793256256348/trace.json`.
- Original throttle run and recovery: `crates/target/sync-v2-instances/1793254612517/{trace,throttle-recovery-evidence}.json`.
- Final logs: `target/sync-review-final-stress.log`, `target/sync-review-legacy.log`.
- Payload, Android source/build hashes and instrumentation: `target/sync-v2-release-review/`.
- Windows profiles/evidence: `crates/target/release-review-windows/1793255688852/` and `1793256171144/`.
- Native stress binary SHA-256: `B3BCE0389A208E0EFB14FEE1B2B83AAD25F397E7A7DEB2AFF2791429F77528DB`.

## Remaining release gates

1. Run the final source through **macOS/Xcode Simulator tests and unsigned iOS device build**, plus the macOS desktop build. Windows cannot establish those results. Localization parsing and source review do not replace compilation.
2. Complete physical Android/iOS background/termination/connectivity/protected-data checks and maximum-supported payload memory/responsiveness checks on release hardware. The emulator and 50,000-transaction desktop fixture cover narrower conditions.
3. Run hosted CI on the integrated source, review release versions/notes, and finish staged deployment/restore/rollback and monitoring checks. The target changes are still local and no hosted workflow has validated this exact uncommitted tree.

Keep `SYNC_V2_ENABLED=0` until these gates pass. Deploy the additive migration and compatible Worker before activation; rollback after cutover must remain v2-aware. `SYNC_V2_PUBLICATION_PAUSED=1` pauses new publication/key controls while retaining reads, receipts, cancellation and trash. Never rewind D1 separately from R2, discard deletion/receipt fences, or reopen legacy writes.

## Reproduction

From the worktree root:

```text
cargo test --locked --manifest-path crates/Cargo.toml --workspace --lib --tests
cargo build --locked --manifest-path crates/Cargo.toml -p urbudget-services --example sync_qa
npm --prefix desktop run build
npm --prefix desktop run test:ui
npm --prefix web run check
node web/scripts/sync-v2-protocol.mjs
node web/scripts/sync-v2-instances.mjs --rounds 300 --large-transactions 50000
node web/scripts/sync-v2-instances.mjs --legacy
```
