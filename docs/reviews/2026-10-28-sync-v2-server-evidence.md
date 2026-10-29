# Sync v2 server and independent-instance evidence

Validated October 28–29, 2026, in the isolated `sync-v2-redesign` worktree.
No production deployment, remote migration, push, tag, or account activation was performed.

## Implemented server boundary

- Additive D1 migration `web/migrations/0012_sync_v2.sql`; separate account scope, installation receipts, staging ownership, heads/tombstones and immutable catalog events.
- Complete `/api/v2/sync` bootstrap/register/catalog/status/trash/content and prepare/blob/commit/cancel/receipt routes. Account controls share the same durable sequence protocol.
- Atomic D1 publication uses a successful head/account CAS marker to gate its event, object adoption and terminal receipt. Terminal receipts survive later writes by other installations.
- Streaming R2 upload verifies exact size and SHA-256, with durable ownership established before upload. GC checks legacy heads, v2 heads and live staging; inventory catches late writes. Retired ownership metadata is compacted independently of retained identity/sequence fences.
- Migration retains key material and legacy ciphertext context. D1 guards reject old budget mutations and even an old key reset of an empty library. Adopted-object protection also blocks an old Worker's ambiguous-write garbage insertion.
- Reads/receipts/delete remain available without publication entitlement. Trash bytes count against account quota. Cursor compaction racing a page request returns 410 rather than silently skipping deleted events.

## Exact checks

Run from `web/`:

| Command | Result |
| --- | --- |
| `npm run check` | TypeScript passes; **413 tests pass across 17 files**, including **36 sync v2 tests**. |
| `node scripts/sync-v2-protocol.mjs` | Pass against real local workerd/D1/R2: three legacy objects migrated, exact receipt replay, old HTTP writes rejected, direct old SQL insert/key reset rejected, legacy-source restore, crash-after-R2-put retry, atomic key reset. |
| `node scripts/sync-v2-instances.mjs` | **15 scenarios pass**, including 210 deterministic edits across three native processes. |
| `node scripts/sync-v2-instances.mjs --rounds 300` | **All 19 scenarios pass**, including **900 edits**, **2,657 HTTP requests**, an additional 5,000-transaction snapshot, and lost-key-reset-response retry without erasing peer data. Stress segment: 61.8 seconds. |
| `node scripts/sync-v2-instances.mjs --rounds 1` | Final core/notification candidate: **19 scenarios pass**, **837 HTTP requests**, including remote-save revision notifications, remote-deletion budget IDs, offline deletion undo, concurrent restores and a deletion racing a remote save. |
| `node scripts/sync-v2-instances.mjs --legacy` | Real Rust legacy encryption/base/tracking migration passes without duplicate entries; a late legacy device retains its dirty old identity as private instead of claiming a restored incarnation. |

The native executable was built by the coordinating agent from the changed Rust
workspace. These runs use distinct data directories and processes, not multiple
clients sharing a singleton Store. Fault proxies can disconnect a device, drop
the response after the server commits, and pause a request before blob upload
or commit.

Final `sync_qa.exe` SHA-256:
`590EC8486F9F5FBAE82A9DB08305A94A8389523C48206B441C56F936D6E25860`.

The 900-edit run preceded the final read-only notification snapshot fix; all 19
scenarios were rerun on that final binary with three stress edits and explicit
remote-save/deletion notification assertions. The final snapshot fix also has
dedicated core tests run by the coordinating agent.

The scenarios check automatic enrollment/discovery; concurrent edits in
different months; offline edit/restart/lost commit response; eviction/download;
save while a frozen upload awaits commit; raw incomplete editor fields surviving
restart while incoming changes wait; explicit same-month assignment conflict
and rejected stale choices; deterministic stress convergence; dirty remote
trash/recovery/restore; late old-incarnation upload; large content; and deletion
of the final budget followed by empty-library restart. Further scenarios verify
locked receipt repair, expired subscription read/delete access, key reset while
an old upload is prepared, offline deletion undo, concurrent queued restores,
deletion CAS retry, and repeating a key reset after its committed response was
lost while another device already added data to the new generation.

Trace evidence:

- `crates/target/sync-v2-instances/1793220255614/trace.json` — 210-edit run.
- `crates/target/sync-v2-instances/1793220317680/trace.json` — 900-edit run.
- `crates/target/sync-v2-instances/1793221221433/trace.json` — 15 scenarios, 210 edits.
- `crates/target/sync-v2-instances/1793221614762/trace.json` — 18 scenarios, targeted lifecycle rerun.
- `crates/target/sync-v2-instances/1793221204803/trace.json` — native legacy migration and late-device identity protection.
- `crates/target/sync-v2-instances/1793221792758/trace.json` — frozen final candidate, all 18 scenarios, 900 edits.
- `crates/target/sync-v2-instances/1793221973321/trace.json` — final candidate legacy migration rerun.
- `crates/target/sync-v2-instances/1793222635583/trace.json` — 19 scenarios, 900 edits, key-reset retry protection.
- `crates/target/sync-v2-instances/1793222906631/trace.json` — legacy migration after owned-process cleanup hardening.
- `crates/target/sync-v2-instances/1793223006319/trace.json` — final core/notification binary, all 19 scenarios.
- `crates/target/sync-v2-instances/1793223108298/trace.json` — final binary, all 19 scenarios with retained changed-budget/revision assertions.

The process harness exposed a dirty-restore binding bug during implementation.
After the coordinator preserved/quarantined the old local identity and discovered
the restored incarnation under a fresh local identity, the same scenario passed.
Assertions retain the old recovery entry and verify its dirty transaction never
overwrites the restored remote content.

The server tests also inject transaction rollback after the head write, a lost
D1 acknowledgement after commit, competing CAS commits/cancel, generation reset
while pending, old-scope cancellation before prepare, missing referenced R2,
quota rejection, expired leases/trash/events, late orphan objects, oversized
streams/JSON, counter exhaustion, and compatible publication pause/resume.

A newly added interrupted-upload test exposed backpressure deadlock when an R2
conditional create returned without consuming its stream. Cleanup now cancels
the transform's readable side along with the source and writer. The regression
passes in both the SQLite/R2 fakes and real workerd after forcibly reverting the
D1 ready marker while keeping the already uploaded immutable R2 object.

The existing privacy-generation test compared LF and CRLF as content changes.
Its comparison now normalizes line endings; production privacy content is unchanged.

## Local-only harness guard

`wrangler.sync-v2-local.jsonc` has a dummy database ID and synthetic development
email settings. Scripts invoke Wrangler with `--local`, unique persistence under
`.wrangler/sync-v2-qa/`, and no remote binding. The instance harness only accepts
an HTTP loopback `URBUDGET_QA_URL` (`127.0.0.1`, `localhost`, or `[::1]`) and an
`@example.test` email. A deliberate non-loopback configuration was rejected
before spawning a device or making a network request.

All native QA instances and per-run Workers stopped after their runs. The shared
QA-only Worker on port 8788 was stopped after the Android and coordinating-agent
checks completed; no listener remained on that port. Isolated local persistence
and traces remain under ignored build/testing directories for inspection.

Worker cleanup now terminates only its spawned Wrangler process tree and awaits
release of the loopback port. Sequential protocol/legacy suites passed. A
separate same-port 8792 start/stop/start/stop check also passed, with both stops
awaited; no manual process cleanup was required for those runs.

Scheduler wakes are driven by native `sync_now` calls in the process harness.
These results validate the service/protocol and do not replace separate desktop
and phone background-lifecycle/UI checks.

## Release and rollback flags

`SYNC_V2_ENABLED` is absent from production configuration: activation defaults
off. Deploy the additive SQL migration and compatibility Worker under separately
authorized release work before allowing any account cutover. Existing v1 accounts
continue using the legacy protocol until migration.

After cutover, roll back only to a compatible v2-aware build. Set
`SYNC_V2_ENABLED=0` to stop new activations. For an incident,
`SYNC_V2_PUBLICATION_PAUSED=1` returns retryable 503 for create/update/restore and
key controls without consuming their pending delivery sequence. It preserves
reads, receipt replay, cancellation, trash and purge. Pending leases may expire
with an explicit terminal receipt. Remove the flag to resume. Never flip a
migrated client back to v1 or discard outboxes, tombstones, or receipt fences.

Wire details and runnable local commands are in `web/SYNC_V2.md`.
