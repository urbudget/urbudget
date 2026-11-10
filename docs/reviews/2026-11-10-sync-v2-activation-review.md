# Sync v2 activation review — 10 November 2026

Review by Claude Code. **Sync v2 is not ready to activate.** Six defects confirmed in source
block it (B1–B6), and three rollout prerequisites are missing (R1–R3). No sync defect blocks
shipping clients while `SYNC_V2_ENABLED` stays off. The Android release is held by a red CI job
and one data-loss path (M1, M2); the iOS release needs a restore test on a device (M3).

Nothing in the repository, production or any store was changed by this review.

## Fixes on `claude/sync-v2-activation-blockers`

The same day, the six blockers were fixed on a branch off main `b36854a`. The findings below
are kept as written; this table says what changed. S2 was fixed with B1 because it sits on the
same path. R1–R3, the other S items and every M item are untouched, and R4 is new.

| Finding | Change |
| --- | --- |
| B1 | `drain_lifecycle` does nothing while a delivery is unsettled: it stops before the first action instead of finishing or superseding the slot's own job. Errors left by a delivery that later completed at the top of a pass are cleared (S2). |
| B2 | A reset on an account still on protocol 1 goes through the v1 engine, with activation on or off. The account moves to protocol 2 at its next unlock. |
| B3 | The Worker records each migrated head's origin (`legacy`: the incarnation and v1 revision the migration gave it; migration `0013`). A device links its v1 copy while the head keeps that incarnation, however often it has been rewritten, then merges what it missed against its own last synced base. A copy that still can't be linked (restored since, or the device's ledger is gone) stays local as before, and now says so on its row every pass. |
| B4 | On `stale_sequence` or `sequence_reused` for a request with no receipt, the client drops that request, takes a new installation identity and runs the pass again. The request is not replayed: its content goes through the usual version checks and merge. This is narrower than the design's "quarantine and review" for a clone, on purpose; see `web/SYNC_V2.md`. |
| B5 | Incoming changes to the same budget wait only for checkpoints written by the running process. A dead process's checkpoint stays as a draft to recover. Replacing the budget with another incarnation still waits for every draft. A clean budget held back this way reads "waiting", not "synced". |
| B6 | A one-time schedule that is gone on the other side counts as moved past, so its entry merges as deleted there. A repeating schedule that was deleted still says nothing about an entered occurrence. |
| S4 | Fixed later the same day with the device-approval work ([plan](../plans/2026-11-10-sync-device-approval.md)): a device unlocked on protocol 1 takes its key over at its first protocol 2 pass. |

**Evidence.** Each scenario was run against a local Worker with the client built from main
before the fix, and failed as the finding describes; then with the fixed client:

| Scenario (`web/scripts/sync-v2-instances.mjs`) | Before the fix | After |
| --- | --- | --- |
| B1: a restore held back at prepare, asked for again | "A lifecycle receipt changed." on every later pass | passes |
| B2: reset on a protocol-1 account with activation on (`--legacy`) | `upgrade_required: Sync operation rejected` | passes |
| B3: a device unlocks after another rewrote the migrated head (`--legacy`) | two live budgets on that device | one, linked, with both devices' edits |
| B4: data folder restored from an earlier copy | prepare refused with 409, no recovery | new identity, both edits kept, uploads continue |
| B5: checkpoint from a process that has since restarted | status "synced" while held; incoming change never applied | "waiting" while the editor lives; applied after restart, draft kept |

B6 has unit tests only (five new cases in `sync_merge.rs`). The Worker has a new test for the
origin field. Run locally on the fixed branch: the Rust workspace (788 passed, 3 ignored), the
Worker's type check and tests (429 passed), `sync-v2-protocol.mjs` on real workerd, and the
instance suite with `--rounds 300` (31 scenarios) and `--legacy` (2). Not run: the desktop UI
suite and native build, any Gradle or Xcode build, any device.

**R4 (new): desktop 1.0.11 in the field has none of these fixes.** It is the only released
v2-aware client, and on an activated account it would still fork (B3), wedge (B1, B4) and
stall (B5). Activation needs a way to keep it out: the client sends only
`User-Agent: UrBudget/<version>`, so the Worker could require a minimum version on
`/api/v2/sync` for protocol-2 accounts and report `activationEnabled: false` to older ones.
Bootstrap itself must keep answering them, because 1.0.11 probes it on every v1 pass.

**Later the same day: R1, R2 and R4.** With the last step of the
[device-approval plan](../plans/2026-11-10-sync-device-approval.md):

| Finding | Change |
| --- | --- |
| R1 | `SYNC_V2_ACCOUNTS` lists the account ids that may set up or move to protocol 2 while `SYNC_V2_ENABLED` is off. |
| R2 | The three variables are documented where they go, as comments in `web/wrangler.jsonc`, and in `web/SYNC_V2.md` under Switching it on. Nothing is set. |
| R4 | The client sends `X-Sync-Client: 2`; a version number wouldn't tell this build from 1.0.11. A client that sends none gets `activationEnabled: false` from bootstrap on a protocol-1 account, and `426 sync_upgrade_required` from every route on a protocol-2 account. |

R3 stands: the iPhone build in review is v1-only, and no store has a build with any of this.
The other S items and every M item are still untouched.

## What was reviewed and how

- Source at `a56482f` (`claude/foldable-polish`, unpushed, with uncommitted Android UI edits from
  other work). `web/`, `crates/`, `desktop/`, `ios/` and `shared/` are identical to main at
  `f71c884`; the branch adds Android UI and docs only.
- Five independent read-only passes: the Worker (`web/src/sync-v2`, `sync.ts`, migration 0012),
  the client engine (`cloud/v2.rs`, `sync.rs`, `notifications.rs`), the merge
  (`sync_merge.rs` against today's data model), the store and the new edit paths, and the three
  apps' integration. Every finding marked **confirmed** below was then rechecked by reading the
  cited code. Findings marked **reviewer trace** were not rechecked.
- **Nothing was reproduced at runtime.** "Confirmed" means the path was traced in source.
- Run locally: `cargo test --locked --workspace --lib --tests` — 780 passed, 0 failed, 3 ignored.
- CI on the same `web/` and `crates/` code (main `f71c884`): Sync protocol validation, App
  validation and Native iOS pass. Native Android fails (M1).
- Not run: the Worker and desktop UI suites locally, any Gradle build, any device or emulator.

## Blocks v2 activation

### B1: A failed delivery lets the same pass invent a receipt for its own job, wedging the installation for good — confirmed

**Where:** `cloud/v2.rs:1914-1917` (`run_inner`), `:1706-1711` and the other `finish_job` calls
in `drain_lifecycle` (`:1734`, `:1752`, `:1759`, `:1766`, `:1773`), `:698-711` (`deliver_inner`);
`budgets.rs:2748-2753` (`finish_job`).

`run_inner` swallows a failed top-of-pass `deliver` and carries on to `drain_lifecycle`. That
function finishes jobs locally with made-up receipts (`superseded_before_dispatch`,
`already_trashed`, …) without checking whether the job owns the pending delivery slot. When the
real receipt arrives, `finish_job` sees a completed job with a different receipt and returns
"A lifecycle receipt changed." before `complete_delivery` runs. The slot already holds its
receipt, so every later pass repeats the error. That installation never uploads again.

**Scenarios:**
- Restore a trashed budget while `SYNC_V2_PUBLICATION_PAUSED=1` (prepare returns 503, the slot is
  kept). The row shows an error, the user taps Restore again, and the next pass marks the first
  job superseded. After the pause is lifted the first restore commits and the device is wedged.
  Using the documented incident switch is what triggers it.
- A trash commits on the server but its response is lost. The next pass's delivery fails for any
  reason that still lets the catalog reconcile (429, 5xx). The catalog shows the budget trashed,
  the job is finished as `already_trashed`, and the replayed real receipt then mismatches.

**Fix:** never finish a job locally while its operation id is in the delivery slot; skip it (or
skip `drain_lifecycle`) until the slot settles. Add an instance-suite scenario: `prepareError` on
a trash, `library_restore`, wake, clear the fault, wake.

### B2: Nobody on a v1 account can reset the passphrase once the flag is on — confirmed

**Where:** `cloud/v2.rs:836-840` and `:858` (`try_setup`); `web/src/sync-v2/repository.ts:99`;
`cloud/sync.rs:807` (`sync_reset`).

With activation enabled, `try_setup(reset=true)` sends `keyReset` whatever the account's
protocol. The server rejects `keyReset` on protocol 1 with `upgrade_required`. `sync_reset`
propagates that error with `?`, so the v1 reset below it is never reached. A subscriber who forgot
the passphrase can't reset, and can't migrate either, because migration verifies the old
passphrase. With the flag off `try_setup` returns `None` and v1 reset works, so no test sees this.

**Fix:** on a protocol-1 account send the reset through v1, or let the server accept a reset that
also migrates.

### B3: Activation forks budgets on every device that unlocks after the first v2 upload — confirmed

**Where:** `cloud/v2.rs:2095-2108` and `:2151-2154` (`adopt_legacy_bindings`);
`budgets.rs:2366-2385` (`observe_catalog`).

A device's v1 copy is linked to the v2 head only while the head's payload is still `legacy-v1`.
The first upload from any v2 device rewrites it as `v2`. A device that unlocks later skips its
copy without setting any status, marks adoption done (`legacyAdopted`, one-shot), and
`observe_catalog` inserts a second row for the same remote budget and downloads it. The user then
has two budgets with one name: the old copy, now local-only and still the open one, holding any
unsynced v1 edits; and the cloud copy. Edits made to the open one no longer sync.

This is the expected launch path, not an edge: install the phone app, enter the passphrase (which
activates), add a transaction, then open the desktop.

Reviewer trace, same function: a device whose `sync-state.json` was cleared by sign-out and
sign-in duplicates every budget, where v1 used a base-less merge.

**Fix:** decide the behaviour explicitly. Options: keep the migration incarnation provable after
a rewrite (the incarnation doesn't change on update) and merge with the stored v1 base; or fall
back to a base-less merge as v1 does; at minimum set a visible status and never leave the fork
silent.

### B4: A restored or cloned installation can never upload again, and on iPhone that is the normal new-phone path — confirmed in code; needs a device restore test

**Where:** `cloud/v2.rs:486-488` (`connect_bootstrap` discards the registration answer, including
`lastSequence`), `:575-583` (`deliver_inner` handles only `invalid_manifest`, `too_large`,
`invalid_body`); `web/src/sync-v2/repository.ts:62,66`; `budgets.rs:1726`
(`account_sync.installation_id`, `next_sequence`). No client code reads `lastSequence`,
`stale_sequence`, `sequence_reused` or the receipt route. No `isExcludedFromBackup` anywhere
under `ios/`; the data folder is Application Support (`BudgetStore.swift:40`).

`SYNC_V2.md` says a restored clone "must reconcile rather than reset the fence", but the client
has no reconcile path. A phone restored from last night's backup after syncing this morning gets
`stale_sequence` on every upload, permanently. Two live copies of one installation give
`sequence_reused`. Desktop data restored from a backup behaves the same. Android is safe: its
backup rules exclude everything.

**Fix:** on `stale_sequence` or `sequence_reused`, treat the library as a clone: register a new
installation id, keep local changes dirty, and merge against the current heads.

### B5: A leftover editor checkpoint stalls a budget in both directions while the status says "Synced" — confirmed

**Where:** `cloud/v2.rs:1461-1463` and `:1501-1510` (`process_budget`), `:317-329`
(`decorate_status`); `android/.../ui/EditorDrafts.kt:14-21`, `EditorCheckpoint.kt`;
iOS `BudgetForms.swift:284-346`; desktop `editorCheckpoint.ts`, `App.tsx:230-237`.

Every editor writes a checkpoint row when it opens, before any typing, and discards it only on a
normal close. `process_budget` treats any row as a live editor: once another device has uploaded,
the budget returns "waiting" and neither downloads nor uploads. `process_owner` is stored and
never read, and nothing clears rows at launch. `decorate_status` keeps "waiting" only for dirty
entries, so a clean budget with an orphan row reads "synced".

Rows are orphaned by process death with a sheet open (routine on phones), an Android text-size
change with the transaction form open, any failed operation while a sheet was open, and quitting
desktop with a dialog open. The only exit is Library → the budget's menu → Unfinished edits →
Discard, which nothing prompts for. Three of the five passes found this independently; the
29 October review already called leaked checkpoints an activation blocker.

**Fix:** don't let rows from another `process_owner` hold merges (clear or offer them at launch),
and keep "waiting" in `decorate_status` when the entry is clean.

### B6: A one-time scheduled entry deleted or skipped on one device is silently re-entered by another — confirmed

**Where:** `sync_merge.rs:1064-1070` (`removed_occurrence`); `schedules.rs:414-415`.

The guard that stops a removed occurrence coming back looks the schedule up on the other side
with `find(..)?`. A one-time schedule is removed from the list once entered or skipped, so the
lookup returns `None` and the entry counts as newly added. Every hand-entered future-dated
transaction is a one-time schedule, and all three apps auto-enter due schedules on open, before
the first sync.

**Scenario:** desktop enters the occurrence and the user deletes it (or skips it). A phone with a
stale base opens and auto-enters the same id. After the merge the deleted expense is back on both
devices with no conflict and no note. The existing tests and `sync_qa` use monthly schedules only.

**Fix:** treat a schedule the base had and the other side no longer has as moved past. Add the
`"never"` variants of the three existing tests.

## Rollout prerequisites

### R1: There is no way to activate one account first — confirmed

`SYNC_V2_ENABLED` is one global switch (`repository.ts:38`, `:84`, `:97-98`). Once on, every
account whose user types the passphrase on a v2 client migrates, the v1 rows are deleted, and no
path sets `protocol` back to 1. The staged rollout the release docs require can't be done.
**Fix:** gate `register`, `keySetup` and `migrateProtocol` on an account allowlist as well.

### R2: The two flags have no defined home — plausible

Neither flag is in `wrangler.jsonc`, and `SYNC_V2.md` says "set" without saying how. A plain-text
variable set in the dashboard is replaced by the config's `vars` on the next `wrangler deploy`
unless `keep_vars` is set, and every push to main that touches `web/**` deploys. A pause set
during an incident could be lifted silently. **Fix:** commit the flags to `wrangler.jsonc`, or
document them as secrets.

### R3: Compatible clients aren't all shipped — confirmed

The design requires v2-aware clients on every platform before activation. The iOS 1.0.0 build in
App Review (tag `ios-v1.0.0`, 22 October) has only the v1 client: a migrated account gets
`426 sync_upgrade_required` from it. Keep the review account on v1 while that build is in review.
Android is unreleased. Desktop 1.0.10 and earlier also get 426.

## Should fix before activation

| # | Finding | Where | Status |
| --- | --- | --- | --- |
| S1 | v1 list and delete aren't fenced during cutover. A v1 `GET /budgets` in flight while another device migrates returns an empty list, and the v1 engine drops its tracking for every budget ("Removed from sync on another computer"). | `web/src/sync.ts:105-109`, `:154-173`; `cloud/sync.rs:673-680` | confirmed |
| S2 | A lifecycle error is never cleared when the slot completes at the top of a later pass. Any network drop during a trash or restore leaves that budget "error" and the account "attention" for good. | `cloud/v2.rs:1843-1851`, `:2045-2048` | confirmed |
| S3 | A trash whose receipt is `expired` or `cancelled` is marked complete and never retried. The app suspended between prepare and commit for over an hour leaves the budget in the local Trash and active in the cloud. | `cloud/v2.rs:698-711`, `:1832-1837`, `:1659-1660` | confirmed path |
| S4 | After activation every other unlocked device goes "locked" until the passphrase is typed again. The v1 key for the same generation is in the vault and could be verified against `bootstrap.check`. | `cloud/v2.rs:192-204` | confirmed |
| S5 | A budget evicted, then trashed, then restored stays in Trash on that device with no way out. | `cloud/v2.rs:1407-1409`, `:1724-1737` | reviewer trace |
| S6 | Permanently rejected uploads retry every pass with no backoff. An account with 60 budgets sends 10 doomed creates per pass and can exhaust the account-wide 1,200 prepares an hour, after which every device gets 429 (which also feeds B1). | `cloud/v2.rs:1359-1367`, `:1496`; `web/src/sync-v2/index.ts:168` | reviewer trace |
| S7 | Per-pass cost grows with every library row, Trash included. This answers the open question in FOLLOW-UPS: `try_list_all` runs at least 7 times a pass and opens each file; every account budget is opened read-write; each trashed one gets an unconditional `set_sync_blocked(true)`; `set_entry_status` writes every row every pass. Files of trashed account budgets can't be removed, so the cost is permanent. | `budgets.rs:2475-2588`; `cloud/v2.rs:1943`, `:1958`, `:2055`; `cloud/notifications.rs:18-31` | reviewer trace |
| S8 | Import bookkeeping merges as whole values. Two devices matching the same transaction to a bank text and a statement row conflict on `bankImportIds` and `bankImportMatches`; two phones importing texts conflict on `smsImportedMessageIds`. Sync stops on a prompt showing raw hashes, and the losing side's import ids are dropped, so those rows are offered again. An account's `simplefin.lastSync` conflicts the same way. | `sync_merge.rs:148-155`, `:974-991`; `bank_import.rs:1216-1224`; `sms_import.rs:501-503` | confirmed for `smsImportedMessageIds`; rest reviewer trace |
| S9 | The phones show most conflict values raw: a target as "60000" for 600.00, a goal as JSON, the header as the field's key. Desktop formats all of these. | `android/.../ui/CloudScreen.kt:274-293`; `ios/.../AccountSyncView.swift:398-454` | confirmed (Android) |
| S10 | The phones' sync label hides failures. With any local-only budget in the library (the sample always is), a pass-level error reads "Saved on device". It also goes stale after Sync now or a conflict answer. | `android/.../ui/App.kt:790-804`; `NativeServices.kt:94` | confirmed (Android) |
| S11 | Hourly maintenance scans the whole event journal in one batch with the ledger prune, and a single failed trash purge skips compaction and inventory for that hour. Fine at first-activation scale. | `web/src/sync-v2/objects.ts:42-80` | confirmed SQL; thresholds estimated |
| S12 | Every incoming merge takes a full SQLite backup under the store lock and keeps it forever by default. On a phone with background sync this grows without limit. | `cloud/v2.rs:1580-1583`; `store.rs:561-590` | reviewer trace |
| S13 | The client ignores `bootstrap.publicationPaused`: a dirty budget takes the slot, gets 503, and blocks trash from that device for the whole pause. | `cloud/v2.rs:1817` | reviewer trace |

Smaller, from the merge pass: a category's `target` and `goal`, and a loan's rate and rate
history, are answered separately and can combine into a state neither device meant; a schedule's
`nextDate` moved forward on both devices asks with no user edit on either side.

## Mobile releases, independent of v2

| # | Finding | Where | Status |
| --- | --- | --- | --- |
| M1 | Native Android CI has failed on main since 9 November: lint error `StateFlowValueCalledInComposition`. The fix is in the open [PR #17](https://github.com/urbudget/urbudget/pull/17) (`9ac352a`), not on main or this branch. | `android/.../ui/AccountsReports.kt:303` | confirmed |
| M2 | Android's "Restore previous budget" is one global file, written on every import and create. Import into budget A, open budget B, tap Restore: B is overwritten with A's old content, no copy of B is kept, and sync carries it to other devices. | `android/.../BudgetRepository.kt:54`, `:292-296`, `:318` | confirmed by reading; not run |
| M3 | iPhone: the vault file is in the backed-up folder but its key is a non-synchronizable Keychain item. After an iCloud restore to a new phone the vault would stay locked and sign-in would fail every time; the only remedy is deleting the app. Needs a restore test before release. | `crates/urbudget-services/src/vault.rs:128-134`, `:175-180`, `:373-377` | plausible |
| M4 | iPhone erases the YNAB baseline (`sourceExport`) on the first edit after opening a budget from the library. Already in FOLLOW-UPS; it doesn't propagate through sync. Smallest fix: call `load_budget` after `library_open`. | `ios/.../BudgetStore.swift:199-203` | reviewer trace |
| M5 | The bundled SQLite is still 3.51.1 (`rusqlite` 0.38, `libsqlite3-sys` 0.36), the version with the WAL deadlock in FOLLOW-UPS. `rusqlite` 0.41.1 is published. Sync opens and closes every budget file each pass (S7), which is the pattern that hit it. | `crates/Cargo.lock` | confirmed |
| M6 | Android shows sync errors on the page under the bottom sheet that caused them (stale review, wrong passphrase). | `android/.../ui/CloudScreen.kt:61-69`, `:210-228` | plausible |
| M7 | An expired session removes every sync indicator on all three apps, with no "sign in again" notice. Rare: sessions slide for 365 days. | `cloud/mod.rs:369`; `App.kt:368`; `CloudBadges.tsx:24` | reviewer trace |
| M8 | One Rust test is flaky (`browser_callback_exchanges_code_with_pkce…`, failed once in CI at `8f87f44`). The cause is in the product: `wait_for_callback` parses the request from a single `read`, so a request split across TCP segments fails sign-in. | `crates/urbudget-services/src/assistant/auth.rs:248` | confirmed |

## Test coverage gaps

- The multi-process stress suite (`sync_qa`) edits only through `save_budget`. Android's
  engine-edit path is covered by one `smsBatchApply` call in `SyncV2RuntimeTest`.
- No scenario blocks a lifecycle delivery slot (B1), resets a protocol-1 account with activation
  on (B2), unlocks a second device after the first v2 upload without a trash and restore (B3), or
  replays a stale installation sequence (B4).
- Merge tests and `sync_qa` use monthly schedules only (B6).
- No Worker test drives v1 HTTP requests against a concurrent migration (S1), or calls `/trash`
  or `/budgets/status`.
- `SyncV2RuntimeTest` has no incoming merge under the light copy, no conflict review, no orphaned
  checkpoint, no account switch and no status label.

## Verified sound

- **Server:** every v2 query is bound to the session's user; R2 keys are server-generated. The
  commit decision and CAS run in SQL inside the batch; concurrent commits and replays resolve to
  one accepted and one terminal receipt. Garbage collection deletes only after the same batch
  found no reference. Migration is one batch and preserves key material. Counters are compared
  as integers, never as strings. Account deletion cascades. The earlier C6, A6, installation
  retirement, Google refresh at commit and deploy dry-run fixes hold.
- **Client content delivery:** bytes are persisted before send, replay is correct at every crash
  point, the sequence advances only after acknowledgement, and rejected outcomes keep edits
  dirty. Download applies content, base and dirty flag in one transaction. No path deletes local
  data on a remote deletion, a 410 or a missing snapshot. The earlier C1, C2, A1, A2, A4 and A7
  fixes hold. B1 is the cost of A4's "reads continue while blocked".
- **Merge:** `merge(b,b,r)==r`, `merge(b,l,b)==l` and `merge(b,x,x)==x` hold; iteration is
  deterministic; transfers can't unpair; the same import id on both sides gives one transaction;
  a month plan is never mixed. The earlier reconciled-amount, dangling-reference, A3, A5 and A7
  fixes hold. Unknown fields are preserved everywhere.
- **Store and the new edit paths:** every content write goes through `write_changes` or
  `Store::save` in one transaction with the revision bump and the dirty trigger. No lost update
  between sync and an engine edit. Undo history resets when sync changes the budget. Android
  never sends its light copy to `save_budget`. Delete for good refuses account budgets.
- **Apps:** all three echo the displayed review token, version and fingerprints. A stale copy
  can't be saved over a merge on any platform. WorkManager is constrained, unique and cancelled
  when signed out. Android backups are disabled. Most of the earlier platform "should fix" items
  are fixed.

## Not verified

- Anything at runtime beyond the Rust workspace tests: no finding here was reproduced.
- iOS backup and restore behaviour (B4, M3), background grants, and a pass that straddles
  device lock.
- Real D1 query plans and limits for S11; whether R2 list cursors expire.
- How often the desktop's "database changed while you were editing" banner appears once v2
  starts a pass 2 s after every save (reviewer trace, `desktop/src/useBudget.ts:355-362`).
- The earlier gates still stand: physical-device process death, Doze, protected data, changing
  connectivity and maximum-payload memory; staging, restore, rollback and production monitoring.
