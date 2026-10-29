# Sync v2 core storage and merge evidence

Implementation worktree: `C:/Users/Admin/.codex/worktrees/sync-v2-redesign/urbudget`.
All fixtures use temporary directories. No live budgets, remote databases, commits, pushes, tags or releases were used for these checks.

## Implemented contracts

- `Budgets` uses a transactional SQLite catalog. An empty library is valid. Targeted mutations preserve entries written by another catalog instance; only a window's current selection is cached.
- `create_owned` persists account ownership, remote ID, incarnation and initialization intent together, before writing financial content. Catalog observations preserve account isolation, monotonic versions, logical trash and explicit eviction.
- `quarantine_binding` preserves the original file, ancestry and outstanding upload. It stores the original catalog ownership in `sync_hints`, frees the account/logical-ID association and exposes the old copy as recovery.
- `activate_binding` verifies the financial database's committed identity and publishes its catalog incarnation, active lifecycle, local availability and download completion together. A pending local trash intent blocks activation.
- Per-budget tables hold account-scoped ancestry, dirty state, immutable encrypted upload bytes, receipts and financial editor checkpoints. A received/merged budget, its new base and a prepared outgoing upload are one budget-database transaction.
- The account delivery slot checks its exact per-budget intent before allocation. A terminal receipt must be applied to the budget or lifecycle journal before the account sequence advances. These are separate replayable commits, not a cross-file atomic transaction.
- Rejected/cancelled uploads do not acknowledge their candidate. Accepted uploads acknowledge only the captured local revision; newer local edits remain dirty.
- Local storage schema 3 is separate from financial schema 2. Write triggers require the registered storage-protocol SQLite function, including when a legacy connection reparses a statement prepared before migration.
- Migration retires the original file, copies SQLite/WAL state through the backup API, checks the staged snapshot, copies recovery/backups/imports/exports and journals publication before catalog remapping. Original files remain preserved. Late legacy registry additions are imported only as local recovery copies; old selection and removals never rewrite the catalog.
- OS leases cover creation/download through `mark_available`. `claim_abandoned_local_job` returns a lease the host must hold through repair; an active owner cannot be mistaken for a dead creator with an empty file. Explicitly abandoned host operations can release their lease with `release_local_job`.
- Eviction persists intent, transactionally checks cleanliness and retires the source before backup/removal. Startup resumes it under the library migration lock. A failure leaves its journal/source and a per-budget status instead of failing library startup. A selected file already retired by eviction is deselected before restart repair.
- Assignment amounts, assignment extras and movement history merge as complete monthly plans. Concurrent different plans in the same month require a choice; different months merge. Unrecognized history is preserved with the whole assignment plan as one opaque conflict domain. Reviewed answers require fingerprints of the actual underlying conflicting values.

## Executed verification

| Command | Result |
|---|---|
| `cargo check --manifest-path crates/Cargo.toml -p urbudget-core` | Passed |
| `cargo test --manifest-path crates/Cargo.toml -p urbudget-core --lib --quiet` | 395 tests: 393 passed, 2 pre-existing ignored |
| `cargo test --manifest-path crates/Cargo.toml -p urbudget-services cloud::sync::tests --lib --quiet` | 16 passed at the service-fixture update checkpoint |
| `cargo test --manifest-path crates/Cargo.toml -p urbudget-core reopening_and_readonly_status --lib --quiet` | 1 passed after the final notification audit |
| `cargo test --manifest-path crates/Cargo.toml -p urbudget-services cloud::notifications::tests --lib --quiet` | 4 passed after the final notification audit |

Permanent regressions cover unknown upload acknowledgement/restart, exact immutable request identity, rejected receipts, merge/base/intent rollback, per-budget acknowledgement before global sequence completion, control receipt application, clean incarnation adoption versus dirty/checkpointed recovery, stale SQL writers, independent catalog instances, account isolation, late legacy creations, active initializer/downloader leases, WAL-preserving migration, replay after publication before catalog commit, safe eviction and restart after retirement, equal moves to different destinations, stale reviewed answers and opaque historical movements.

The five authorized legacy service tests now explicitly initialize their library fixture and use different months for independent monetary edits. Their upload-race, legacy envelope and record-convergence assertions remain. Their wire-format assertion still targets financial schema 2; the SQLite schema assertion targets the separate storage constant.

The final notification audit identified an additional save-token regression: reopening a current-schema database rewrote `user_version`, changing another window's `data_version` despite no financial edit. `Store::open_budget` now writes the schema version only when it changes. Notification snapshots use `Store::existing_status`, which opens closed files read-only and never creates or migrates them; an open budget uses its live connection's token. Two-connection regressions verify unchanged tokens after capture/reopen and invalidation after an actual save.

## Boundaries of this evidence

- Core crash tests reconstruct persisted intermediate states and reopen files; these results do not claim full OS-kill fault coverage, a real multi-device network test or physical mobile-device validation. The root implementation task owns those integrated checks.
- Old processes must still be stopped during normal upgrades. SQL fencing cannot prevent a truly old process from writing a later recovery JSON file to its preserved source directory. Those source directories are intentionally retained.
- Conflicting/corrupt legacy migration sources are isolated as unavailable recovery entries, with diagnostics and original files retained. Valid sources are fenced before copying; an unreadable corrupt source may be impossible to fence, so it is never selected or opened automatically. A repaired migration resumes from its journal and remains a recovery entry for explicit review.
- Backups retained during eviction can still contain local-heavy data such as `sourceExport`; encrypted cloud sync is not a complete source-export backup.
- Protocol activation and production rollout remain separate gates. This evidence does not authorize deployment.
