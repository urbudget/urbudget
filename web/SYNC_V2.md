# Sync v2 server

The Worker serves the protocol under `/api/v2/sync`. `src/sync-v2/protocol.ts`
defines the wire types and validation. All counters are canonical decimal strings.
Every route requires the ordinary account bearer session (or the existing
same-origin cookie protection). A subscription is required at commit for
create/update/restore; it is not required for reads, receipts, cancellation,
deletion, or recovery of already stored content.

## Deployment boundary

Migration `0012_sync_v2.sql` is additive. `SYNC_V2_ENABLED` is absent from the
production configuration, so first-time key setup and account migration are
disabled. Installation registration and new prepares are also rejected for
accounts that have not activated v2. Existing v2 accounts keep access when the
activation flag is disabled. Production deployment, migrations, and account activation have not
been performed by this implementation.

The deploy workflow first applies every migration to a disposable local database,
after TypeScript and Worker tests pass, before its remote migration step.

The compatibility Worker must be deployed before enabling account activation.
It understands both reference schemas during cleanup. D1 triggers fence old
budget writes and key resets after cutover, including requests already past an
old Worker's middleware. Another trigger prevents an old Worker's ambiguous
upload cleanup from receiving an adopted object key via `INSERT ... RETURNING`.
Migration preserves existing key material, versions, and immutable ciphertext;
legacy payload descriptors retain their original budget-only AAD context.

After activation, rollback must retain this compatible schema/protocol. Set
`SYNC_V2_ENABLED=0` to stop further activations. Set
`SYNC_V2_PUBLICATION_PAUSED=1` to pause new create/update/restore and key controls;
the server returns retryable 503 without consuming their delivery slot. Read,
receipt replay, cancellation, trash, and purge continue. Existing staged leases
can expire normally with a terminal receipt. Remove the pause to resume. Never
drop tombstones/receipts, reopen legacy writes, or rewind D1 independently of R2.

## Routes

| Route | Request and result |
| --- | --- |
| `GET /bootstrap` | Account ID, protocol, nullable key generation/salt/check, catalog epoch/highwater/minCursor, entitlement and flags. Requires account authentication but no installation header. |
| `POST /installations` | `{installationId}` with a client-generated random ID. Returns `lastSequence` and last terminal `receipt`. Re-registering never resets a sequence. |
| `GET /installations` | Active installation IDs, creation times, last sequences, and whether each has a pending mutation. |
| `DELETE /installations/:id` | Retire an installation with no pending mutation; idempotent once retired. Retains its sequence and receipt, prevents registration or new mutations under that ID, and frees active-device capacity. |
| `GET /installations/:id/receipt` | Last terminal receipt plus pending sequence/digest/expiry. |
| `GET /catalog/snapshot` | Active heads and a highwater read in one SQL snapshot. |
| `GET /catalog/changes?after=&through=&limit=` | Immutable events through a fixed bound, `cursor`, and `hasMore`; 410 requires resnapshot and known-ID reconciliation. |
| `POST /budgets/status` | `{ids:[...]}` (at most 100); authoritative head or null for each ID. |
| `GET /trash?after=` | Recoverable trash, keyset pagination with `next`. |
| `GET /budgets/:id/content?version=&incarnation=` | Exact version bytes plus `X-Sync-Payload` containing the original encrypted descriptor/AAD context. Missing referenced storage is 503, never inferred deletion. |
| `POST /mutations/:sequence/prepare` | Complete immutable manifest; returns prepared state/digest/expiry/blobReady or terminal receipt. |
| `PUT /mutations/:sequence/blob` | Exact sealed bytes, `X-Sync-Digest`; R2 verifies checksum while consuming a bounded known-length stream. |
| `POST /mutations/:sequence/commit` | `{digest}`; atomic CAS/head/event/object adoption/terminal receipt. |
| `POST /mutations/:sequence/cancel` | `{digest,manifest?}`; exact manifest required if prepare never arrived. Checks authenticated account/installation, operation ID, known kind, canonical digest and sequence fences, then records a cancelled receipt without applying the body. Malformed or oversized payload descriptors can therefore be safely cancelled. Existing terminal receipts replay unchanged. |

Bootstrap, prepare, and content commit refresh due Google Play receipts using the same leased, cached,
three-second-bounded refresh as the account endpoint. This lets a renewed
subscription resume publication without waiting for hourly maintenance or opening
account settings. Provider failure still leaves bootstrap and existing-data reads
available; it does not grant unverified entitlement. Commit replays its existing
terminal receipt before contacting the provider.

Scoped catalog/status/trash/content reads require `X-Sync-Generation` and
`X-Sync-Epoch` (equivalent query parameters are accepted). Mutation requests
require `X-Sync-Installation`. Old receipt replay precedes entitlement, head,
generation, and publication-pause checks.

There is one pending mutation and one retained last terminal receipt per
installation. The next sequence is legal only after its predecessor is locally
acknowledged. Older sequences, gaps, reused digest mismatches, and retired IDs
fail closed. Different devices changing the head cannot invalidate that last
receipt. A restored filesystem clone must reconcile rather than reset the fence.

Budget manifests identify account/installation/operation, key generation/epoch,
budget ID, expected version/incarnation, and optional new incarnation/payload.
Create uses expectedVersion `"0"` and `newIncarnation`. Restore identifies the
current tombstone in `source:{incarnation,version,sha256}` and supplies newly
encrypted bytes with a fresh incarnation. `source.sha256` can be null for migrated
legacy content. Its original decrypt context is in `head.payload.context`.

V2 payload context is exactly:

```json
{"format":"v2","accountId":"...","keyGeneration":"0","budgetId":"...","incarnation":"...","operationId":"...","parentVersion":"0"}
```

Recursive key-sorted compact JSON is hashed with SHA-256. Clients persist both
the manifest and exact sealed bytes before sending; retries cannot reseal with
a different nonce. Control kinds are `keySetup`, `keyReset`, and
`migrateProtocol`; they use the same sequence and receipt protocol.

## Retention and limits

The initial limits are 50 active budgets, 1 GiB referenced content including
trash, 50 MiB per encrypted payload, 200 MiB pending staging reservations,
100 active installations, and 64 KiB control JSON. Retiring an installation
requires resolving or expiring its pending mutation first; retirement never
resets or removes its sequence fence. Payload metadata must be unpadded ASCII
base64url, at most 2,048 characters. Staging expires after
one hour; trash content and immutable catalog events are retained 30 days.
Account and admin usage summaries count active budgets across both protocols,
include retained trash in storage bytes, and exclude purged content. The support
view identifies trash and v2 key setup without exposing key material.

Tombstones and installation sequence fences are retained. Retired object ledger
metadata can be compacted after 30 days because it is not the identity fence.

Rejected prepares do not allocate object or garbage rows. Accepted prepares
reserve their bytes atomically, including the new reservation in the 200 MiB
staging limit.

Finishing a mutation immediately attempts to reclaim its replaced or cancelled
objects. Cleanup checks both legacy and v2 heads plus live staging, atomically
retires unreferenced objects, and retries through the durable garbage queue.
Storage failures do not undo an accepted receipt. R2 deletion uses batches of up
to 1,000 keys; a sweep processes at most 20 batches and checks a ten-second budget
between batches. Account deletion selects that owner's keys directly so unrelated
garbage cannot delay its first cleanup attempt; failures remain queued for retry.
Paged R2 inventory catches late orphan puts even after a prior delete or ledger
pruning, with one reference query for each inventory page. Event compaction only
updates accounts whose minimum cursor advances. Compaction racing a page read
produces 410 rather than skipping events.

## Local verification

`wrangler.sync-v2-local.jsonc` uses a dummy database ID, synthetic email delivery,
and isolated local D1/R2. The scripts always pass `--local` and store unique
state beneath `.wrangler/sync-v2-qa/`. No production data or endpoints are used.

```text
npm run check
node scripts/sync-v2-protocol.mjs
node scripts/sync-v2-instances.mjs
node scripts/sync-v2-instances.mjs --rounds 300
node scripts/sync-v2-instances.mjs --rounds 300 --large-transactions 50000
node scripts/sync-v2-instances.mjs --legacy
```

The instance suite needs the current
`crates/target/debug/examples/sync_qa.exe` built from the Rust workspace. It runs
three separate processes/data folders behind fault proxies. Default stress is
210 edits; `--rounds 300` performs 900 edits. It checks full budget equality,
individual transaction preservation, editor recovery, explicit money conflicts,
stale choices, restart after lost commit response, save during a paused commit,
late old-incarnation upload, eviction, deletion/restore, 5000 transactions, and
empty-library restart. It also tests locked receipt recovery, expired entitlement,
key reset during upload, offline deletion undo, concurrent restores and deletion
CAS rejection followed by explicit review, private creation, account switching and durable
rate-limit recovery. It also checks displayed-review replacement by background sync,
unrelated downloads during blocked publication, safe cancellation of malformed candidates,
unreadable-cache isolation, and downloading without changing the selected budget.
`--large-transactions` changes the default 5,000-row large-budget
case. The rate-limit test advances only its disposable database window; production
limits remain unchanged. `--legacy` exercises real native v1 encryption and ancestry migration,
including old email-only session backfill, a Worker without v2 routes, v1 syncing with
no open budget, and a stale legacy device returning after restore. Scheduler wakes are driven through `sync_now`; this
does not substitute for desktop/mobile background-lifecycle testing. Each run
saves an inspectable trace under `crates/target/sync-v2-instances/`.
