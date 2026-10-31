# Sync review — 28 October 2026

Reviewed local main at **068a347**. Scope: shared Rust sync and merge engine, Worker storage protocol, desktop integration, Android and iOS lifecycle and editing behavior. This is a review; no production code was changed.

Extended into a [redesign specification](../plans/2026-10-28-sync-v2-design.md) and [phased implementation plan](../plans/2026-10-28-sync-v2-implementation-plan.md). The extended review contains 13 actionable findings; these plans are proposed and await implementation authorization.

**Scope note:** import/transfer-related production edits appeared in the shared checkout during final verification. They were left untouched and are not included in this baseline sync audit. This review's evidence and line references target the inspected `068a347` code; recheck overlapping workflow/bridge changes before implementation. This review changed documentation only apart from temporary, removed probes.

## Assessment

The current system automatically exchanges changes **inside budgets already participating in sync**. It does not synchronize the account's budget library as a coherent collection. Enrollment, discovery/download, local removal and cloud removal are separate manual operations. That explains the unintuitive experience.

There are also correctness defects beyond the workflow. I would address the data-loss and convergence findings before making synchronization more aggressive. Keep the existing encrypted storage and three-way merge foundation, but give budget identity, deletion, retry acknowledgements and editor concurrency a stronger protocol.

## What happens today

| Area | Current behavior | Consequence |
|---|---|---|
| First setup | Creating the account's first sync key enrolls the budgets then on that device. Unlocking an existing key does not do the same. | Setup behaves differently on the first and subsequent devices. |
| New budgets | Later budgets remain local-only until explicitly tracked. | Turning on Sync does not mean future budgets will sync. |
| Other devices | Remote-only budgets are reported but are not added to the ordinary local library automatically. | Users must find Account & sync and download each budget. |
| Local deletion | Moves the budget into local trash; cloud copies and other devices remain. | The budget still appears as cloud-only and can be downloaded again. |
| Remove from sync | Physically deletes the server copy; devices retain their local budgets and eventually stop tracking. | This is neither local eviction nor deletion everywhere. |
| Desktop scheduling | Startup, five-minute polling, focus after at least a minute, eight seconds after a save. | Ordinary enrolled-budget edits do sync automatically while the app is running. |
| Android scheduling | Resume, every minute while STARTED, two seconds after a save. | Leaving the app can cancel the pending attempt. |
| iOS scheduling | Foreground start, every minute, four seconds after a change. Background stops the coordinator and cancels its pending save task. | Changes can wait until the phone app is reopened. |

The manual policy is explicitly encoded in the QA scenarios: `second_budget` expects a new budget to stay local-only, then invokes tracking and download; `remove_and_track_again` expects device copies to remain. This is a product-model gap, not just a broken timer.

Sources: [engine dispatch](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:630), [first setup](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:718), [QA scenarios](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/examples/sync_qa.rs:641), [desktop scheduling](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/cloud.ts:79), [Android scheduling](C:/Users/Admin/Documents/GitHub/urbudget/android/app/src/main/java/app/urbudget/nativeapp/MainActivity.kt:46), [iOS scheduling](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Core/SyncCoordinator.swift:35).

## Correctness findings

### R1 — P1: Removing and re-adding a budget lets an offline device overwrite its replacement

**Locations:** [server deletion](C:/Users/Admin/Documents/GitHub/urbudget/web/src/sync.ts:257), [revision allocation and update](C:/Users/Admin/Documents/GitHub/urbudget/web/src/sync.ts:209), [client change detection](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:68).

The server deletes the row completely. Re-uploading the same budget ID starts at revision 1 again. Clients identify content/history using only that revision and the account's key generation; removing a single budget does not change the key generation.

Reproduction:

1. A and B share budget X at revision 1. B goes offline.
2. A removes X from sync, edits its retained local copy, then starts syncing it again. X is revision 1 again, with different contents.
3. B reconnects. If its local copy is unchanged, `need()` reports nothing to do and labels it synced despite different data.
4. If B edited offline, it uploads its old branch with `If-Match: 1`. The server accepts it and replaces A's newer contents without a merge.

**Evidence:** executed against the real Worker routes with synthetic in-memory D1/R2. Expected stale-upload rejection was **409**; actual was **200**, and a subsequent GET returned the stale device's payload. The client skip/upload paths were source traced.

**Required change:** preserve a non-reusable history identity across removal/restoration. Use deletion tombstones with a monotonic version and/or a distinct budget incarnation, require it in reads/writes, and reject uploads against a deleted or superseded incarnation. A numeric revision that restarts at 1 is insufficient. Fence older clients that cannot honor the new deletion protocol.

**Regression:** offline device returning after remove/re-add, both unchanged and edited; replacement eventually reaching the offline device; repeated delete/restore; delayed writes to an earlier incarnation.

### R2 — P1: A lost upload acknowledgement can count an assignment change twice

**Locations:** [upload acknowledgement/base update](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:499), [additive assignment merge](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/sync_merge.rs:463).

The local sync base advances only after the upload returns successfully. A server commit followed by a lost response leaves the client unable to distinguish its already-published change from an independent local change.

Reproduction, with amounts shown in whole units for readability:

1. Shared assignment starts at 200.
2. A changes it to 250. The server accepts the upload, but A loses the response and retains base 200.
3. B downloads 250, adds 70 and uploads 320.
4. A downloads 320. Merging base 200, local 250, remote 320 calculates **370**, with no conflict. Correct result is **320**.

The local +50 has already reached B's ancestry, but is added again. This is a plausible intermittent-network failure, not a simultaneous-edit edge case alone.

**Evidence:** a focused test invokes the real merge function with those states and confirms 37,000 minor units instead of 32,000. The HTTP acknowledgement/base transition was source traced; this was not a full network fault-injection test.

**Required change:** persist an upload intent before sending, with an operation ID, parent version and exact intended snapshot. Make server acceptance discoverable/idempotent even after subsequent devices upload. Resolve an uncertain upload outcome before treating that delta as unpublished and merging it again. Updating the base before sending would create a different data-loss bug.

**Regression:** server commits but response is lost; another device downloads and edits; original device restarts and retries. Assert final amounts, exactly-once application and convergence, not merely a successful status.

### R3 — P1: Desktop automatic sync can discard a pending crash-recovery draft

**Locations:** [sync safety predicate and reload](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/cloud.ts:123), [missing recovery state in hook inputs](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/App.tsx:217), [reload discards recovery](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/useBudget.ts:233).

At startup, the saved database is loaded and marked saved while the recovery draft remains a separate pending choice. The cloud hook receives neither recovery readiness nor the pending recovery state. It can therefore report the budget clean, download a newer cloud version, and call `fetchDatabase()`. That function unconditionally discards the recovery file and clears the recovery prompt.

The native unsaved-draft check does not rescue this: reading a recovery file does not install it as the store's in-memory draft.

**Evidence:** a focused test using the real desktop hooks showed a pending recovery become null after an incoming automatic sync. Source inspection confirms the same loader calls `api.discardRecovery()`.

**Required change:** block application of incoming changes until startup recovery is resolved. Background refresh must preserve recovery; it must not reuse a loader whose side effects include discarding it. Recheck the condition when applying the response, not only when starting the request.

**Regression:** recovery prompt remains open while a newer remote revision arrives, including a response racing recovery initialization and a request started before a recovery becomes pending.

### R4 — P1: Desktop open editors can overwrite changes just received through automatic sync

**Locations:** [sync/reload](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/cloud.ts:123), [editor state is omitted from cloud inputs](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/App.tsx:199), [whole-category replacement](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/App.tsx:1021), [form submits retained fields](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/forms.tsx:436).

An open form keeps its own values, which do not make the main budget dirty. Sync replaces the database and the frontend adopts the new save token, but the form retains its old record. Submitting then replaces the entire record using the new token, bypassing the protection a stale database token would have provided.

**Executed reproduction:** open category with note “Old note”; locally change only its name to “Groceries”; receive a synced snapshot whose note is “Changed on phone”; submit the form. The real CategoryForm/useBudget/useCloud flow successfully saves using the new token, but the note becomes **“Old note”**.

**Required change:** protect open editors from automatic baseline replacement, including editors opened during an in-flight request. Prefer retaining original record/field values and applying only changed fields against a checked current record, so unrelated incoming changes can still arrive safely. Do not throw away the user's draft.

**Regression:** local name edit plus remote note edit; remotely deleted edited record; editor opens during a download; unrelated remote changes while drafting a new record.

### R5 — P2: Signing back into the same account can resurrect records deleted elsewhere

**Locations:** [sign-out forgets tracking](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:391), [base trust requires tracking](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:54), [base selection](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:254), [untracked existing copies merge](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:648).

Signing out clears the tracking ledger. The SQLite sync base remains but is no longer considered trustworthy, even after signing back into the same account and key generation. Existing local and remote copies merge without a base.

Reproduction: both devices share transaction T; A signs out; B deletes T and syncs; A signs back into the same account, unlocks and syncs. Without the base, T looks like a local addition and returns to the cloud without a conflict. With the original base, the same merge correctly preserves the deletion.

**Evidence:** both merge variants executed against the real Rust merge function; authentication and base selection source traced.

**Required change:** retain non-secret ancestry scoped to account ID, key generation and budget incarnation across sign-out. Reuse it only after validating that identity on sign-in. If ancestry genuinely cannot be established, explain that copies need reconciliation rather than silently interpreting all missing records as additions. Never reuse another account's ancestry.

**Regression:** same-account re-login with remote deletion and offline edits, different-account login, passphrase reset, backup restore and removed/re-created budget.

### R6 — P2: iOS accepts a remotely deleted record as safe to save from an old editor

**Locations:** [missing-record guard](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Core/BudgetStore.swift:155), [transaction save](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Core/BudgetStore.swift:507), [upsert inserts absent transaction](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/workflows.rs:883).

`verify()` rejects a changed record only if it still exists. A record deleted by incoming sync passes the check because `current` is nil.

Reproduction: open an existing transaction on iPhone; delete it on desktop; let iPhone sync; tap Save in the retained editor. The original record passes verification, is inserted again by the transaction upsert, and can be uploaded back to other devices.

**Evidence:** complete source call path; not executed on an iOS simulator. The helper also serves other guarded record editors.

**Required change:** when an original record was supplied, absence must count as a conflict. Preserve the draft and offer an explicit recovery/create-copy action if appropriate.

**Regression:** deletion before Save and deletion during Save; verify that retry against a fresh snapshot still rejects the missing original.

### R7 — P2: iOS category editing captures the concurrency guard too late

**Locations:** [original captured at Save](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Screens/Budget/CategorySheet.swift:706), [form values initialized earlier](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Screens/Budget/CategorySheet.swift:717).

The category form captures name/group/note/hidden when it opens, but fetches its guard's “original” from the current budget when Save is tapped. After a remote update, that freshly captured original matches current data, so the guard passes while stale form values overwrite the downloaded fields.

Reproduction: open category details with note A; desktop changes it to B; phone downloads B; change only the name and save. Note A is written back. Group and hidden have the same exposure.

**Evidence:** source traced; iOS execution unavailable on this host.

**Required change:** retain the original record at form initialization and validate it at commit, or merge only fields changed by the user against their original values.

**Regression:** local rename concurrent with remote note/group/visibility changes, preserving both edits or showing an explicit conflict.

### R8 — P2: Conflict answers are reused after the values being answered have changed

**Locations:** [resolve fetches newest cloud version](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:840), [choices contain only field key and side](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/sync_merge.rs:179).

A resolution submits key → local/cloud, without the revision or conflicting values shown to the user. The handler downloads the latest cloud contents and applies those answers to any conflict with the same key.

Reproduction: the UI shows local category name “Food and drink” versus cloud “Groceries”. Before the user applies Keep local, another device changes the cloud name to “Food, utilities, and school”. The old answer silently overwrites that unseen new value; it is not surfaced as a refreshed conflict.

**Evidence:** the real merge function accepted the old key-based choice without producing a conflict; resolve's latest-download path source traced.

**Required change:** bind answers to the displayed local/base/remote values or a resolution snapshot token. Recompute against current data and ask again for conflicts whose inputs changed. Unaffected answers can remain.

**Regression:** same conflict key changes again while the prompt is open, local changes during resolution, a record changes from edit conflict to deletion conflict, and unrelated changes that should not invalidate the answer.

### R9 — P2: Desktop reports “Synced” for local-only and waiting budgets

**Location:** [title-bar status](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/CloudBadges.tsx:11).

The chip treats report-level “ok” as success, and only checks budget-level conflict/error/update-app. “ok” means the pass ran; it does not mean every budget is synchronized. A budget that has never been uploaded, or one currently waiting, still produces “Synced”.

**Evidence:** separate rendered component tests reproduced both local-only and waiting cases.

**Required change:** derive status from the active budget, pending local operations and actual acknowledgements. Distinguish “Saved on this device”, “Waiting to sync”, “Synced” and “Needs attention”; do not imply a deliberately local-only budget is in the cloud.

**Regression:** newly created local-only budget, unsent changes after a successful earlier pass, waiting download, and mixed states across several budgets.

### R10 — P2: Android automatic sync failures/conflicts are invisible outside Account & sync

**Locations:** [automatic report handling](C:/Users/Admin/Documents/GitHub/urbudget/android/app/src/main/java/app/urbudget/nativeapp/MainActivity.kt:94), [status is rendered in account screen](C:/Users/Admin/Documents/GitHub/urbudget/android/app/src/main/java/app/urbudget/nativeapp/ui/CloudScreen.kt:167).

Automatic sync consumes only `openChanged`, refreshing the budget when necessary. Other report states and exceptions are not published to the ordinary application UI. A conflict can block convergence repeatedly while the user continues budgeting without an actionable indication.

**Evidence:** source traced across the Android scheduling and status UI. iOS already has an app-level conflict indicator.

**Required change:** keep app-level observable sync status, with persistent attention indicators for conflicts, expired authentication and subscription failures, and a direct route to resolve them. Avoid repeated intrusive notifications for unchanged failures.

**Regression:** automatic same-field conflict while the register is visible, authentication expiry, retry after reconnect, and clearing the warning after successful resolution.

### R11 — P2: A saved merge can lose the notification that the open budget changed

**Locations:** [failure after local merge](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:538), [resolution replaces its report](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:862), [explicit cloud replacement](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/cloud/sync.rs:879).

Three paths commit incoming changes but fail to preserve `open_changed`: a merge saves before its upload fails; a successful conflict merge returns a second pass's report without copying the first step's flag; and explicit Use cloud replaces the database before returning a new pass that sees it already synced.

The Android automatic caller refreshes the visible budget only when this flag is true. For example, a merge can save incoming changes locally, fail its upload, then upload successfully on the next pass with no additional local change. Neither pass tells Android to refresh, so the screen stays stale until another refresh trigger. Desktop resolution callers also do not explicitly reload, although the separate three-second database watcher normally limits the stale display there. iOS's coordinator reloads independently and is less exposed.

**Evidence:** source traced through the saved merge, error/report handling and application callers; no new runtime fault-injection test for this finding.

**Required change:** carry local-change notifications independently of network success. Preserve them across errors and follow-up passes; return the changed budget identity/token so callers can safely refresh the correct view.

**Regression:** successful local merge followed by failed upload, successful retry with no additional local change, field-level conflict resolution and explicit Use cloud. Assert that the visible budget refreshes, not only that SQLite contains the right data.

### R12 — P1: Concurrent equal money moves can inflate the total assigned amount

**Locations:** [per-category assignment merge](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/sync_merge.rs:463), [equal values collapse](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/sync_merge.rs:69), [atomic local move](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/workflows.rs:490).

The merge treats each category's Assigned amount separately. When two independent moves have the same source and amount, both devices have the same new source total, so that deduction is counted only once. Different destination credits are both retained.

**Executed reproduction using the real workflow and merge functions:** start with Food assigned 20,000 minor units; A moves 5,000 Food → Card; B moves 5,000 Food → Rent while offline. Each device still has total assignments of 20,000. The merged result contains Food 15,000, Card 5,000 and Rent 5,000: **25,000 assigned**, with no conflict. Both move-log entries remain. This inflates budget allocations; it does not create money in bank accounts.

**Required change:** merge monetary operations atomically. For the first v2 release, treat each month's full assignment vector, its assignment extras and associated money-move history as one conflict domain. Different months still merge; two changed versions of the same month require a coherent choice. Do not infer authoritative operations from today's mutable/coalesced move logs. Operation-aware merging can be a later, separately specified feature.

**Regression:** equal moves sharing a source, equal moves sharing a destination, crossed moves, moves involving Ready to Assign, concurrent absolute assignment edits, and multi-category auto-assignment. Assert allocation conservation and consistent history, not only field-level equality.

### R13 — P2: Another app instance can erase a newly created budget from the library

**Locations:** [registry cached at load](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/budgets.rs:70), [whole registry rewritten](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/budgets.rs:110), [switching writes the registry](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/budgets.rs:182).

Each instance retains its own full `budgets.json` snapshot. Create/adopt/remove/switch rewrites that snapshot without a fresh cross-process read or compare-and-swap. The cloud operation lock reloads the sync ledger, but does not reload this library.

**Executed reproduction using public Budgets and Store APIs:** load instances A and B; A creates X and saves a real budget database; a fresh library read contains X; B switches to its already-known original budget; a fresh library read no longer contains X. X's SQLite file still exists, but its registry entry is gone. A later sync may also drop its tracking because it now appears remote-only.

**Required change:** authoritative transactional library rows with fresh reads and cross-process coordination. As immediate containment, reload under an exclusive registry lock before every registry mutation. The v2 SQLite catalog must not carry this cached-whole-registry write pattern forward.

**Regression:** simultaneous create/download/switch/delete across two processes, restart after each interleaving, and preservation of every independently created entry and its tracking.

## Product and lifecycle changes needed

### G1 — Account-level automatic library participation

Once a person enables Sync for an account, new budgets should join automatically by default. An explicitly local-only budget can remain an advanced choice. Existing local budgets need a clear, one-time account association; switching accounts must not silently publish one account's budgets into another.

Unlocking another device should reconcile its library automatically. Remote budgets should appear in the normal picker and be downloaded automatically or when opened, without a separate per-budget “Download” workflow.

Adding `sync_track()` to budget creation is only one part of this. A durable catalog must represent identity, account association, active/deleted state, version/incarnation and local download state.

### G2 — Budget deletion must be a synchronized operation

Provide separate, clearly named actions in the library:

- **Remove from this device:** remove cached/local availability while retaining the account's budget.
- **Delete everywhere:** record a deletion that other devices receive, with recoverable trash and an explicit restore action.
- **Keep only on this device / stop sharing:** if retained as a feature, describe exactly what remains elsewhere.

Deletion needs a durable tombstone, not disappearance from a list. An offline device must observe the deletion before uploading its old copy. Pending offline edits should be retained in recovery/trash and explained; they must not silently undelete the budget.

Do not infer global deletion merely because a local folder is missing. A missing file, cache eviction and explicit Delete everywhere are different events.

The current library also refuses deletion of the current or last budget ([guard](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/budgets.rs:193)). Support an empty library and switch safely to another budget or the library screen, rather than requiring a throwaway budget to delete the last real one.

### G3 — Persist pending work and handle app exit/reconnect

**Executed desktop behavior:** after the initial sync, editing and closing immediately performs the local save and destroys the window, but does not invoke another cloud sync. The eight-second timer disappears with the window. Phones have the same practical gap when backgrounded before their debounce; iOS explicitly cancels it.

This does not lose the local save, but it makes “save here, open there” unreliable unless the user waits or manually syncs first. It should be addressed along with enrollment.

Persist pending create/update/delete operations, retry when connectivity or account access returns, and coalesce bursts without losing intent. Desktop should attempt a bounded final flush or hand work to a background service. Mobile should attempt a lifecycle-appropriate flush and resume reliably; background delivery should be best effort with truthful pending status, not a promise the operating system cannot guarantee.

### G4 — Keep editor drafts usable as incoming changes arrive

Android currently protects stale transactions more conservatively: it rejects a save if the entire budget snapshot changed, even for an unrelated record. That avoids the iOS resurrection bug, but can force a person drafting a new transaction to close and re-enter it after any sync update.

Use original-record/dependency guards and preserve/rebase drafts where safe. Blanket UI blocking for all background activity is a useful immediate containment measure on desktop, but a poor final interaction model.

## Recommended implementation order

1. **Contain the data-loss paths:** R1–R4, iOS record guards and stale conflict answers. Preserve recovery and drafts; make misleading status truthful.
2. **Define the account catalog and version protocol:** stable budget identity, incarnation/version, tombstones, explicit restore, server preconditions and compatibility rules.
3. **Add durable, idempotent pending operations:** resolve uncertain upload acceptance before merging; include creates/deletes, not just content PUTs.
4. **Make library reconciliation automatic:** enroll new budgets, discover remote budgets, unify deletion UX, support an empty library.
5. **Finish lifecycle/status integration:** app-wide reports, reconnect retries, final/background attempts, record-aware editors.
6. **Roll out with migration and mixed-version tests:** legacy clients must not resurrect deleted budgets or overwrite a new incarnation. Tombstone cleanup needs a safe stale-client/full-resync policy.

This does not require replacing everything with a CRDT or abandoning encrypted whole-budget snapshots. It requires reliable ancestry and lifecycle semantics around the existing merge.

## Validation performed

| Check | Result |
|---|---|
| Worker lifecycle suite | 24/24 passed |
| Worker API sync group | 7/7 passed |
| Existing Rust core sync-merge tests | 18/18 passed |
| Existing service cloud-sync tests | 17/17 passed after using a writable workspace temporary directory |
| Existing desktop useBudget / CloudSignIn tests | 41/41 passed |
| New isolated Worker ABA probe | Failed the expected safety assertion: stale upload accepted and newer ciphertext overwritten |
| New desktop probes | Six expected regression failures covering recovery, editor safety/actual overwrite, two incorrect status cases, and close-before-upload |
| Three focused Rust merge probes | Confirmed duplicate assignment, re-login resurrection and stale conflict-answer behavior by asserting current faulty outcomes |
| Extended monetary workflow/merge probe | Confirmed two individually conserving moves merge into 25,000 assigned from a 20,000 starting total |
| Extended two-instance library probe | Confirmed a stale instance removes a newly created library entry while its SQLite database survives |
| Android/iOS findings | Source traced; no physical-device or iOS simulator execution |

The initial service test run hit temporary-file access errors; the isolated writable-temp rerun passed all 17 tests. These were test-environment errors, not reported product defects.

All temporary probe source files were removed. No production code, branches, tags or live budgets were modified. Passing existing suites does not cover the failure sequences above.

## Extended review coverage and limits

The redesign follow-up rechecked the same unchanged HEAD, including account identity and passphrase reset, encryption boundaries, local registry and database persistence, backup restore/import, native startup, background lifecycle, storage cleanup, quotas and CI/deployment. The original focused suite counts above are evidence from this same revision; the two extended probes are additional executed checks.

| Area | Assessment / remaining obligation |
|---|---|
| Ordinary record merges | Existing field grouping, unknown-field preservation, validation and atomic budget/base saves are useful foundations. Monetary grouping and missing ancestry need stronger rules. |
| Authentication/account isolation | Captured sessions and account-change guards exist. Ancestry and future pending work need stable account IDs rather than email/token identity. |
| Encryption | Existing Argon2id, authenticated encryption, random nonces, budget-bound context and decompression limits were inspected. This was not an independent cryptographic audit. V2 must preserve ciphertext privacy and bind its new identity context. |
| Storage durability | A SQLite transaction covers a budget and its base; it cannot cover separate JSON registries, another database and R2. Migration/outbox/file staging need explicit recovery protocols. |
| Server objects/cleanup | Existing durable garbage queue is valuable. V2 staging, trash and migration must protect every live reference and handle termination after R2 upload but before publication. This is a design requirement, not a newly fault-injected production incident. |
| Backups/restore | Current explicit restore applies budget content to the live store; preserve that distinction from restoring identity/outbox state. Import/fresh start should receive new identities. |
| Empty library | Runtime assumes an open Store and fabricates an initial registry entry. Android legacy import is tied to `isNew`; a naive empty-library change can reimport a deleted budget. Separate first-run migration completion from empty state. |
| Scale/resources | Whole snapshots and full-budget clones require large-budget/memory tests before automatic multi-budget downloads. Current decompression ceiling is 512 MiB; that ceiling is not evidence that all phones can handle it. |
| Background delivery | No fixed mobile background deadline can be promised. Durable queue plus lifecycle-aware retries and honest status are required. Locked-device credential behavior needs physical-device validation. |
| Compatibility/rollout | Legacy writers must be fenced at database commit, not only at request entry. Shared services/core changes currently do not automatically trigger the iOS simulator workflow. Android validation lacks lifecycle instrumentation. |

The companion redesign and implementation plan are proposals, not completed changes. Production implementation, migration, publishing and tagging remain unperformed.

## Minimal reproductions for executor regression tests

### Server budget-history reuse

Using an authenticated synthetic account with a sync key, and the same key generation throughout:

```text
PUT    /sync/budgets/X   If-Match: 0   body: original        -> revision 1
DELETE /sync/budgets/X                                      -> 200
PUT    /sync/budgets/X   If-Match: 0   body: replacement     -> revision 1
PUT    /sync/budgets/X   If-Match: 1   body: stale-offline    -> currently 200
GET    /sync/budgets/X                                      -> currently stale-offline
```

The last PUT must not overwrite the replacement. The real client can issue it because its remembered revision equals the reused server revision.

### Lost acknowledgement and remote deletion merge states

The following uses actual core test fixtures. These assertions describe the **desired** behavior and fail on the current implementation. The final fix may need protocol-level tests instead of changing `merge()`, because the merge function is being given incorrect ancestry.

```rust
use urbudget_core::sync_merge::{merge, Choices};
use urbudget_core::test_support::{purchase, sample};

// Accepted upload whose acknowledgement was lost; another device then edits.
let base = sample(); // first assignment = 20_000
let mut local = base.clone();
local.assignments[0].amount += 5_000;
let mut remote = local.clone();
remote.assignments[0].amount += 7_000;
let result = merge(Some(&base), &local, &remote, &Choices::new()).unwrap();
assert_eq!(result.budget.assignments[0].amount, 32_000); // currently 37_000

// Same-account re-login currently causes absorb() to discard the retained base.
let mut local = sample();
local.transactions.push(purchase());
let mut remote = local.clone();
remote.transactions.clear();
let result = merge(None, &local, &remote, &Choices::new()).unwrap();
assert!(result.budget.transactions.is_empty()); // currently resurrects one
```

Add integration tests around the actual upload/re-login transitions as well. Testing only these constructed inputs cannot verify that an idempotent protocol or account-scoped ancestry is correctly persisted and recovered after restart.

