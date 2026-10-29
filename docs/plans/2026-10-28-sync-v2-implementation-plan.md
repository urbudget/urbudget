# Sync v2 — implementation and verification plan

**Status:** implementation authorized and carried out; see the [execution and validation record](2026-10-28-sync-v2-execution.md). Production deployment, account activation and tagging remain separate release work.
**Baseline:** main `068a347`, 28 October 2026, plus the 40 preserved concurrent import/transfer files recorded in the execution baseline.
**Read first:** [design specification](2026-10-28-sync-v2-design.md), [13 findings with evidence](../reviews/2026-10-28-sync-review.md).

**Concurrent work:** the original checkout remains untouched. Its 40 changed/untracked files were copied into the isolated implementation worktree before sync edits. Preservation hashes, shared-file integration and regression checks are recorded in the execution record.

This remains the acceptance checklist for implementation and release. Do not interpret a completed local test as a physical-device test or production rollout approval. Do not substitute per-budget opt-in, disappearance-based deletion, or timer-only retries.

## 1. Outcome and scope

Completion means a person enables account Sync once, creates/opens/edits/deletes budgets through the ordinary library, and compatible devices converge without routine manual enrollment or download actions. Offline work, pending uploads and recovery survive restart. Conflicts remain explicit where intent cannot be safely inferred.

Included: Worker protocol/storage, Rust persistence/coordinator/merge, desktop, Android, iOS, migration, tests, observability and a staged release runbook.

Deferred: multi-user sharing, a full operation-based monetary CRDT, push delivery, delta snapshots, permanent desktop background service, non-destructive passphrase rotation. Mobile background delivery remains best effort.

Recommended product decisions are already in the design. The major deliberate tradeoff is a **month-level assignment conflict when both devices change that month's planning**, until an operation-aware model is separately implemented. Automatic sync should not silently manufacture allocations.

## 2. Execution rules

- Preserve user files and live budgets. Use synthetic fixtures and isolated app data/vault namespaces for every test.
- Prefer an isolated implementation worktree/branch when execution starts; first inspect existing worktrees and uncommitted changes. Do not merge or reset unrelated work.
- Keep protocol activation off by default until all server/client gates pass.
- Do not run `wrangler ... --remote`, publish releases, tag or push to main merely to validate implementation.
- Worker changes pushed to main automatically run remote migrations and deployment in [.github/workflows/deploy.yml](C:/Users/Admin/Documents/GitHub/urbudget/.github/workflows/deploy.yml:1). Treat this as a production release boundary, not an ordinary test.
- Every work package ends with its relevant tests and a short evidence record. Do not mark source inspection, compilation or simulated data as physical-device validation.
- Temporary probes from the review are not retained regression coverage. Convert their **desired invariants** into permanent tests with the fixes; do not preserve tests asserting the buggy result.
- If the implementation changes a protocol/default described here, update the design and its cross-platform fixtures in the same change.

## 3. Work packages and dependency graph

| ID | Deliverable | Dependencies | Exit gate |
|---|---|---|---|
| W0 | Frozen contracts, fixtures, fault harness | None | Version/state/error formats agree across server/Rust/native consumers |
| W1 | Immediate UI/recovery/registry containment | W0 | R3/R4/R6–R11 containment and registry race regression pass |
| W2 | Additive server schema, compatibility fences, storage repository | W0 | Actual SQL concurrency/failure tests pass; v2 dormant |
| W3 | Local catalog, optional open Store, schema/intent persistence | W0 | Two-process and crash/reopen tests pass |
| W4 | Server prepare/upload/commit/receipt and cleanup | W2 | Idempotency, CAS, cancellation, R2/GC and entitlement matrix pass |
| W5 | Local delivery slot, acknowledgement recovery and coordinator | W3 + W0 mocks | Every crash boundary replays safely; lost acknowledgement does not duplicate |
| W6 | Conservative monetary merge and reviewed conflict answers | W0 | Conservation and conflict-staleness tests pass |
| W7 | Catalog reconciliation and budget lifecycle | W4 + W5 + W6 | Automatic create/discover/trash/restore/evict works headlessly |
| W8 | Checked editor commands and versioned invalidation | W3 + W5 | Drafts survive incoming changes; stale originals cannot overwrite |
| W9 | Desktop library/status/lifecycle integration | W7 + W8 | Desktop acceptance scenarios pass |
| W10 | Android library/status/background integration | W7 + W8 | Unit + instrumentation + device lifecycle checks pass |
| W11 | iOS library/status/background integration | W7 + W8 | Simulator + device/locked-state checks pass |
| W12 | End-to-end migration and mixed-version recovery | W2–W11 | Legacy cutover/restart/rollback matrix passes |
| W13 | Integrated stress, performance and release evidence | W9–W12 | All release-blocking cases and native coverage complete |
| W14 | Authorized staged deployment/activation | W13 | Canary gates pass; independent release authorization |

W2/W3/W6 can run in parallel after W0. Platform work can run in parallel after the shared bridge contract stabilizes. W12 is not postponed design work: develop its migration fixtures/fences from W2/W3 onward, then validate the complete flow after all clients exist.

### W0 — Contracts and reproducible baseline

Create shared fixtures for:

- Account/key/catalog/budget/installation identities and decimal-string sequence fields.
- Manifest normalization/hash and exact ciphertext retry identity.
- Pending/terminal mutation states, receipt contents and error codes.
- Receipted key setup/reset/activation, old-generation pending invalidation, and accepted versus rejected/cancelled/expired acknowledgement rules.
- Active/trashed/purged/remote-only/evicted/unavailable/local-only library states.
- No-open-budget snapshots and checked editor command/result types.
- Library/status versions, changed-budget revision notifications and conflict fingerprints.

Introduce a transport seam and deterministic clock/fault controller around the Rust sync engine. Build a synthetic server driver supporting pause-before-commit, commit-then-drop-response, reordered replies, key/account changes and process restart. Extend the real SQLite-backed Worker fakes with transactional assertions, not just stubbed query results.

Preserve baseline results from the review: Worker lifecycle 24, API sync 7, core merge 18, service sync 17 and focused desktop 41 passed on this revision. Those counts are not a target for the new suite; the new behavioral cases are.

**Done:** write executable tests for the exact R1/R2/R3/R4/R5/R12/R13 sequences, schema/manifest golden fixtures and a fault-boundary checklist. Known-failing scenarios may begin on the feature branch, but the delivered packages must pass their relevant assertions.

### W1 — Immediate containment

Touch existing desktop cloud/useBudget/forms/settings, iOS record/category guards, Android status handling, shared report propagation and the legacy registry.

- Separate automatic refresh from discard-recovery actions.
- Pass/recheck recovery readiness and unsafe editor state before applying incoming snapshots.
- Reject deleted originals on iOS; retain category originals at form initialization.
- Bind conflict choices to reviewed values; propagate committed local changes even if upload fails.
- Render truthful local-only/waiting/conflict status; make Android attention persistent.
- Protect legacy `budgets.json` mutation with cross-process lock + fresh reload as a short-term fix.
- Preserve drafts and avoid the Android “close and retype after any unrelated change” experience where checked record guards are available.

**Do not claim this fixes R1/R2/R12 globally:** those require the new identity/delivery/monetary paths and compatible-client rollout. A safe new client cannot repair every old client's merge behavior.

**Done:** regression tests using the real CategoryForm/hooks reproduce the old overwrite and now preserve both name/note edits or deliberately block safely; recovery survives incoming sync; two registry instances preserve X.

### W2 — Server schema and atomic repository

Add a numbered migration after the repository's latest migration at execution time; do not assume the current next number remains free.

Implement separate account/key state, heads/tombstones, installations, pending manifests, immutable objects/staging, catalog events and cleanup bookkeeping. Add indexes/constraints for identity uniqueness, one pending sequence per installation, head/event versions and bounded lookup.

Implement the conditional D1 publication batch before exposing routes:

1. Claim the prepared mutation with a unique attempt token and expected sequence/digest/account state.
2. Apply its head CAS and policy/quota predicates; that **same successful CAS statement** writes the installation/sequence application marker.
3. Gate all success-only work on that marker, not merely the attempt claim; reset/activation uses an equivalent account-CAS marker.
4. Increment catalog sequence and emit event only when applied.
5. Adopt the ready object/retire old references only when applied.
6. Store the terminal accepted/rejected receipt and advance the installation sequence.
7. Return that receipt.

Gate every statement on the claim token. A zero-row CAS is a deterministic terminal rejection; it is not a successful mutation or a SQL rollback. An actual SQL failure rolls back the batch.

Install legacy-table guards for migrated accounts, including sync-key UPDATE/reset and controlled account-deletion paths. Upgrade cleanup reference checks before any v2 row references an old object.

**Done:** races produce one outcome/event, all injected SQL failures leave no partial publication, old v1 requests paused before cutover cannot commit afterward, including reset when no budgets exist.

### W3 — Local storage and no-open runtime

Primary existing locations: [budgets.rs](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/budgets.rs), [store.rs](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/store.rs), [services lib.rs](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/lib.rs), [runtime.rs](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/runtime.rs), [Tauri adapter](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src-tauri/src/lib.rs).

- Introduce authoritative library SQLite rows and durable lifecycle jobs.
- Make active Store optional; shared budget commands return typed no-open errors.
- Remove fabricated-budget behavior from initialized empty libraries.
- Add per-budget binding/base/intent/receipt tables and durable dirty markers to all write paths.
- Separate storage schema version from financial payload schema version.
- Add a registered storage-protocol function and write fences; test old already-open/prepared connections.
- Implement replayable staging for file adoption and live paths outside legacy registry targets.
- Keep data migration, file relocation and library publication as explicit journaled steps.
- Persist legacy-import completion independently of the current budget count.

**Done:** create/open/close/delete-last/evict-last/restart works with zero phantom budgets; no lost save between content commit and global hint; crash midway through migration/adoption leaves either the old complete budget or a recoverable staged one.

### W4 — Server transport, receipt and object lifecycle

Implement the design's versioned endpoints. Prefer modules such as protocol/catalog/mutations/objects under a new sync-v2 directory; keep v1 compatibility explicit rather than turning the existing file into many interleaved modes.

- Reserve immutable manifests before R2 writes.
- Enforce streamed actual size, checksum, lease/generation and immutable object keys.
- Make prepare/upload retries resume the same staging state.
- Permit restore uploads with proposed new incarnation and retained source context; re-seal because v2 AAD changes.
- Commit/cancel races return the winning terminal receipt.
- Settle prepare-time policy/version rejection and staging expiry with durable receipts; never free a reserved sequence for another digest.
- Replay accepted/rejected receipts before evaluating the now-current head or entitlement.
- Persist highwater fences; reject reused sequence/different digest, old sequence, gaps and re-registration of retired IDs.
- Separate authenticated recovery/read/delete access from paid publication access.
- Include trash bytes in quota; bound staging independently.
- Missing referenced bytes produce a retryable storage error, never an inferred deletion.
- Retire references before GC, retain cleanup work after account deletion, and reconcile aged orphan objects.
- Make post-commit metrics best effort so logging cannot disguise success as failure.

**Done:** actual Worker route tests with real SQLite transactions plus local Wrangler D1/R2 tests cover these contracts, including accepted commit whose HTTP reply is lost.

### W5 — Durable native delivery and coordinator

Suggested new shared modules: protocol, catalog, delivery, recovery and status beneath the services cloud/sync implementation. Preserve a stable adapter API while extracting the old monolith.

- Prepare immutable per-budget intent before any HTTP.
- Allocate exactly one account-installation delivery sequence in the library.
- Resolve outstanding outcomes before any later operation.
- Persist acknowledgement/base/captured local revision in one budget transaction; complete the global slot separately and replayably.
- Advance content base/acknowledged revision only on accepted content publication. Rejection/cancellation/expiry/generation-invalidation leaves content dirty and its base intact while allowing sequence completion.
- Preserve edits made during upload as dirty; never mark the current/latest revision acknowledged accidentally.
- Persist lifecycle intents that do not have a live budget DB in the library.
- Recover unindexed prepared intents and incomplete global completion.
- Serialize delivery across foreground UI/background worker/other processes without holding Store locks during network IO.
- Persist retry state, jitter and typed pause causes. Wake on commit, startup, foreground, reconnect and unlock.
- Keep one conflicted budget from starving other independent work.

**Done:** deterministic crashes before/after every local transaction and every HTTP boundary converge without duplicate publication, assignment changes or lost edits. Signed-out/locked work stays associated with its owner.

### W6 — Monetary groups and conflict review

Implement month-atomic assignments, extras and move history as specified. Unknown/unparseable legacy movement entries must be preserved as an opaque group and conflicted when necessary, not silently discarded or guessed into a month.

Remove independent post-processing that could duplicate/re-add one side's movement history. Preserve ordinary record-field merging and transaction monetary grouping.

Generate resolution fingerprints from the actual underlying values and identity, not localized labels. Require renewed review only for changed inputs; keep unaffected choices.

**Done:** R2 and R12 scenarios cannot silently produce inflated Assigned totals; crossed/source/destination/Ready-to-Assign moves and absolute assignment edits preserve coherent blocks. Tests explicitly cover the deliberate same-month prompt tradeoff.

### W7 — Headless automatic catalog and lifecycle

- Implement active snapshot + immutable feed + expired-cursor/known-ID reconciliation.
- Persist metadata/download jobs before cursor advancement.
- Auto-associate newly created ordinary budgets under the selected account policy.
- Auto-download discovered active budgets; honor durable eviction.
- Implement local and synchronized Trash, restore with fresh incarnation/re-sealing, purge and recover-as-copy.
- Preserve dirty copies and open drafts when remote deletion arrives.
- Require reviewed versions for destructive lifecycle operations.
- Resolve outstanding uploads before deleting/moving their durable intents; queue later delete intent safely when offline.
- Keep imported backup/fresh start identities distinct; never match by name.
- Make account reset/delete operate across v2 heads, receipts, pending objects and cleanup correctly.

**Done:** a two-device synthetic scenario creates on A, discovers on B, edits both, deletes offline, reconnects, restores, evicts and reopens, without visiting account settings or losing pending work.

### W8 — Checked commands, editor protection and invalidation

Introduce explicit target identity and editor baseline in shared commit commands. Audit every financial editor/write path, including category/transaction/account/schedule/loan/payee forms, inline assignment entry, imports, assistant changes and undo.

Use record/dependency checks and field patches to accept unrelated remote changes safely. For unsaved full-budget drafts/recovery or not-yet-migrated editors, use an OS-backed editor/mutation guard to block unsafe application without blocking downloads or uploads of saved content.

Add durable raw editor-draft checkpoints and require checkpoint acknowledgement before clearing views on remote deletion or eviction. Preserve invalid/incomplete form values. Commit deletion knowledge first, reject financial writes to the old identity, then checkpoint/close safely. Process death before the UI notification must leave the last durable draft recoverable.

Replace the global `openChanged` contract with identity-specific revision invalidations and versioned status. Return these even when a later upload errors. Support snapshot reconciliation after missed notifications.

**Done:** callbacks cannot affect a new account/budget after switching; remote deletion never resurrects a record; conflicts preserve draft input; incoming local commits become visible on all clients.

### W9 — Desktop

Touch cloud/useBudget/App, Sidebar/library dialogs, cloud settings/badges, bridge types and form save paths.

- Remove the active-budget-token prerequisite for discovery.
- Show local/account/remote-only/evicted/trash entries in one library.
- Replace per-budget tracking/download/removal controls in settings with library actions.
- Route library mutations through shared services instead of duplicate Tauri implementations.
- Integrate checked editor commits, recovery-safe loading and status.
- Make close/update flush local state and persist pending work before a bounded cloud drain.
- Handle active/last deletion and startup with an empty library.

**Done:** component/integration tests and desktop smoke tests cover startup recovery, offline create/delete, two windows, local rename+remote note, immediate exit and reopened delivery.

### W10 — Android

Touch MainActivity, NativeServices, BudgetViewModel/Repository, SqliteBudgetStorage, BudgetEditGuard, NativeServicePolicy, library/cloud/editor UI, manifest and Gradle dependencies.

- Application-scoped coordinator plus unique network-constrained WorkManager work.
- Clear cached budget/token/history on no-open snapshots.
- Create a real entry before writing the first budget.
- Separate old-file import completion from `isNew`.
- Replace whole-budget reference invalidation with checked record/dependency semantics.
- Surface persistent sync attention outside Account & sync.
- Background worker startup must be independent of an Activity.

**Done:** unit/lint/build plus emulator instrumentation for process/lifecycle behavior; real-device tests for locked credentials, connectivity changes, process kill and save-then-background.

### W11 — iOS

Touch SyncCoordinator, BudgetStore/repository, NativeCore consumers, UrBudgetApp, library/account/editor screens, Info.plist, project.yml and generated Xcode project.

- Shared headless coordinator and no-open runtime support.
- Correct original-record/deleted-record handling in every guarded editor.
- UIKit background assertion for a bounded drain; BGTaskScheduler registration/retry with expiration handling.
- Credentials unavailable while locked pause work without clearing identity.
- Cache invalidation and draft preservation after catalog/account changes.
- True create-first semantics when the library is empty.

**Done:** simulator tests and unsigned device build; physical iPhone/iPad checks for locked/background/expiration scenarios. A Windows source review or passing Rust bridge test does not substitute.

### W12 — Migration and compatibility

Build fixtures from current desktop/native data folders, legacy Android JSON, v1 tracked/untracked budgets, remote-only budgets, missing files, recovery drafts, backups and same-name unrelated budgets.

Test local migration under an upgrade lock, storage write fences and separate v2 live paths. If quiescence cannot be established, preserve data and ask the user to close other instances through a clear upgrade UI; do not run a best-guess in-place conversion.

Server account activation must be one guarded transaction preserving key/ciphertext and labeling legacy payload context. Test resumed old PUT/delete/reset requests after activation. Copy no unverified ancestry; ambiguous old IDs stay local pending explicit reconciliation.

Test same-account sign-out/login, a new account reusing the same email, account switching, encryption reset, restored full-data snapshots and cloned installation IDs.

**Done:** interrupted upgrade is restartable at every step, all compatible clients recover the same account catalog, legacy clients cannot mutate the migrated account, and rollback-to-compatible-paused-mode preserves data.

### W13 — Integrated validation, observability and performance

Extend the synthetic sync QA harness into unattended lifecycle scenarios; remove its manual enroll/download assumptions for v2. Add seeded two/three-device interleavings and deterministic crash/retry tests.

Record aggregate metrics for pending age, receipt recovery, conflicts, rejected stale writes, catalog lag, staging/GC backlog and migration phase failures. No budget names/amounts/notes/plaintext conflict contents, keys or tokens in logs.

Profile small, realistic large and maximum-supported encrypted budgets on desktop and phones. Measure memory during decrypt/decompress/parse/merge, download disk usage and UI responsiveness. Reject or stage safely on unsupported/resource-constrained cases; never truncate or corrupt the previous local budget.

Update CI:

- Add Worker PR validation; current production deploy testing alone is not enough as a merge gate.
- Add native Android instrumentation/lifecycle job.
- Expand iOS shared-core/services change triggers or require its simulator job for every sync PR.
- Add cross-protocol migration/fault tests using the supported local Cloudflare runtime.
- Keep formatting/version checks and existing core/import/unknown-field coverage.

**Done:** publish a release evidence document mapping every acceptance case to test name/platform/result, with remaining limitations stated.

## 4. Release-blocking acceptance matrix

| Group | Required scenarios |
|---|---|
| Automatic library | Create on each platform; discover/download elsewhere; zero-local-budget login; local-only/sample opt-out; closed-budget updates |
| Identity | Sign-out/re-login; sign-out→create→sign-in stays local-only; A→B→create binds B only; same email/new account; import-as-new; backup content restore; fresh start |
| Deletion | Active/last delete and eviction; offline delete; remote delete with dirty editor/recovery; process death before deletion UI callback; blocked eviction retains drafts; trash expiry; clean-device auto-adoption of restore; explicit restore/purge |
| Ancestry/retry | Lost response before/after server commit; another device advances; restart; newer local edit during upload; stale/clone sequence |
| Ordering | Update↔delete; delete↔restore; prepare↔cancel↔commit; same-sequence different hash; old incarnation replay |
| Financial invariants | R12 equal moves; crossed moves; shared destination; Ready to Assign; absolute set; auto-assign; coherent history |
| Editor safety | Local name/remote note; same-field conflict; remote record deletion; editor opens during download; switched identity callback |
| Durable storage | Crash at every journal phase; disk full; corrupted download; missing file; incomplete migration; two processes modifying library |
| Cursor/feed | Updates while paging; cursor expiry; snapshot then deletion/update; known ID absent; stale event replay; unknown catalog epoch |
| Account/key | Expiry; revoked session; unavailable vault; reset during staging; lost reset reply; old-generation terminal receipt lookup/invalidation; account delete during commit; old-generation replay |
| Storage/quotas | Concurrent count/byte quota use; retained trash; staging cap; missing R2 object; GC/publication race; late orphan upload |
| Legacy | v1 request paused before cutover; empty-account key reset; old SQL connection/prepared statement; old filesystem deletion |
| Lifecycle | Immediate close/update; Activity stop; process death; iOS expiration; foreground/reconnect; locked device; no active budget |
| Status | Pending/local-only never “Synced”; committed incoming data visible despite failed upload; persistent attention; last success retained |
| Compatibility | Unknown fields; older payload formats; schema upgrade required; account activation followed by compatible server rollback |

For every mutating operation, inject failures: before preparation, after local preparation, after sequence allocation, after blob upload, before D1 decision, after D1 commit before response, after per-budget acknowledgement, before global completion, and after completion before UI notification.

Properties to assert: no unexpected resurrection; no overwritten unseen branch; acknowledged snapshots match receipts; no accepted mutation applies twice; no cross-account upload; no discarded recovery; no ghost library entries; assignment groups remain coherent; every referenced blob remains retrievable or reports an explicit storage fault.

## 5. Commands and environments

Use existing project scripts as applicable; new focused suites should be added to them:

```text
cargo test --locked --manifest-path crates/Cargo.toml
npm --prefix web run check
npm --prefix desktop run build
npm --prefix desktop run test:ui
npm --prefix desktop test
```

Android: run Gradle unit tests, lint and debug/release builds from `android/`, then the new emulator instrumentation task. iOS: use the repository's macOS xcodebuild simulator/device workflow. Do not report unavailable platform checks as passed.

Run end-to-end sync only against explicitly configured local/staging services with synthetic accounts and isolated `LOCAL_BUDGET_DATA_DIR`/vault entries. Add a harness guard that rejects the production host by default. Exercise real D1/R2 semantics locally in addition to SQLite/Map fakes.

No need to rerun every platform build for each documentation change. During implementation, run focused checks for each package, then the complete gate before activation.

## 6. Authorized rollout runbook

Execution of this runbook requires a separate production release decision after the implementation is reviewable.

1. Verify backups, restore procedure, schema compatibility and account deletion/GC behavior.
2. Deploy additive schema + compatibility server, with migration disabled. Existing accounts remain v1.
3. Publish compatible clients for all supported platforms; do not auto-migrate accounts while a required platform lacks a usable build.
4. Enable synthetic/internal accounts first. Hold destructive GC during the short initial canary window, with bounded storage monitoring.
5. Verify genuine offline/response-loss/delete/restore/locked-device scenarios and the metrics thresholds defined in W13.
6. Enable a small cohort with explicit account upgrade messaging. Existing local-only budgets get the one-time batch association screen.
7. Expand only when no data-integrity/migration failure remains unexplained. Treat any resurrection, duplicate financial effect, cross-account binding or lost recovery as a stop condition.
8. On incident: disable new activation/publication as appropriate; preserve outboxes/tombstones/receipts; roll back only to a compatible build; recover through versioned changes/export. Do not rewind D1 independently of R2 or reopen v1 writes.
9. Retire v1 paths only after a separately verified migration/support window. Compact receipts/tombstones retain their safety fences.

Tag names and version increments are selected at release time, not invented in this plan.

## 7. Definition of done

- All 13 reviewed findings have a linked fix or an explicit documented behavior change, with regression coverage.
- Every required acceptance group passes on its relevant platforms; native physical-device limits are honestly recorded.
- The user can manage the complete account library through ordinary app flows, with no routine manual budget tracking/download.
- Migration, mixed-version rejection, recovery, trash and rollback are exercised, not only described.
- Encryption/privacy, unknown-field preservation, backups and accounting validation remain intact.
- No pending correctness issue is hidden by “Synced”, silent fallback, disabled tests or a catch-all error handler.
- Implementation and release evidence are reviewable before any production activation.

## 8. Findings-to-work mapping

| Finding | Primary work |
|---|---|
| R1 reused server revision | W2/W4/W7/W12 identity, tombstones and legacy fencing |
| R2 lost acknowledgement duplication | W4/W5 receipts and local acknowledgement; W6 monetary safety |
| R3 recovery discarded | W1/W8/W9 recovery-safe adoption |
| R4 desktop stale form overwrite | W1/W8/W9 checked editor commits |
| R5 re-login resurrection | W3/W5/W12 account-scoped ancestry |
| R6 iOS deleted record resurrection | W1/W8/W11 record existence guards |
| R7 iOS late original capture | W1/W8/W11 editor baseline |
| R8 stale conflict choices | W6/W8 reviewed-input fingerprints |
| R9 misleading desktop status | W1/W5/W9 persistent acknowledgement state |
| R10 invisible Android problems | W1/W5/W10 app-wide status |
| R11 lost local-change notification | W1/W5/W8 versioned invalidation |
| R12 non-conserving money moves | W6 whole monetary conflict domains |
| R13 registry overwrites | W1 containment; W3 transactional library |
