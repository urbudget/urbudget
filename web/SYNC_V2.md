# Sync v2 server

The Worker serves the protocol under `/api/v2/sync`. `src/sync-v2/protocol.ts`
defines the wire types and validation. All counters are canonical decimal strings.
Every route requires the ordinary account bearer session (or the existing
same-origin cookie protection). A subscription is required at commit for
create/update/restore; it is not required for reads, receipts, cancellation,
deletion, or recovery of already stored content.

## Deployment boundary

Migration `0012_sync_v2.sql` is additive. Production has had it since 29 October 2026, and
activation has been on since 22 November 2026: `SYNC_V2_ENABLED` is `"1"` in `wrangler.jsonc`.
With it on, a v2 app (desktop 1.0.11 and later, Android, iOS 1.0.1 and later) that sets up a
new passphrase creates a v2 account (`keySetup`), and one that enters an existing account's
passphrase moves that account to v2 (`migrateProtocol`). Nothing moves on its own. Once an
account is on v2, a v1 app (desktop 1.0.10 and earlier, iOS 1.0.0) gets HTTP 426
`sync_upgrade_required` ("Update UrBudget to continue syncing") and keeps its local budgets.

An account whose devices are already unlocked on v1 is offered the move instead. This is in apps
built after 22 November 2026, so desktop 1.0.13, Android 1.0.1 and iPhone 1.0.0 don't have it.
Each sync saves the bootstrap, and sync status then reports `upgradeAvailable` for a signed-in
protocol-1 account that has a passphrase while `activationEnabled` is true. Account & sync shows
"Move to the new sync", warns that every device then needs desktop 1.0.11, Android 1.0.1 or
iPhone 1.0.1 or later, and calls `sync_upgrade`. A device unlocked on v1 moves without the
passphrase: `migrateProtocol` copies the v1 generation, salt and key check, so the v1 key saved
on the device is the moved account's key. It is used only once it opens the account's key check;
otherwise (a locked device, or a key from before a reset elsewhere) status reports
`upgradeNeedsPassphrase` and the dialog asks for it. For the same reason, a v2 app that was
unlocked on v1 stays unlocked after another device moves the account: its next sync adopts the
saved key for the current generation after the same check.

With the flag off, first-time key setup and account migration are disabled, and installation
registration and new prepares are rejected for accounts that have not activated v2. Existing
v2 accounts keep access when the flag is off.

The deploy workflow first applies every migration to a disposable local database,
after TypeScript and Worker tests pass, before its remote migration step.

The compatibility Worker must be deployed before enabling account activation.
It understands both reference schemas during cleanup. D1 triggers fence old
budget writes and key resets after cutover, including requests already past an
old Worker's middleware. Another trigger prevents an old Worker's ambiguous
upload cleanup from receiving an adopted object key via `INSERT ... RETURNING`.
Migration preserves existing key material, versions, and immutable ciphertext;
legacy payload descriptors retain their original budget-only AAD context.

Each head the move seeds records its origin, `legacy: {incarnation, revision}` (migration
`0016_sync_v2_legacy_origin.sql`): the incarnation the move gave it and the v1 revision it
carried. Nothing writes it again. A device that was on v1 links its copy of a budget, by the
budget's id and its own v1 ledger and base, while the head keeps that incarnation, however often
other devices have rewritten the head since. It links at the revision it last synced, and its next
pass merges what it missed against that base. A restore gives the head a new incarnation, and a
copy that can't be linked (restored since, or the device's ledger is gone) stays on the device
beside the account's budget, its row saying so on every pass. Heads moved before 0016 existed got
their origin from 0016 itself when they still had the move's incarnation (32 lowercase hex
digits, which no app makes), with their version then standing in for the revision.

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
| `GET /bootstrap` | Account ID, protocol, nullable key generation/salt/check, `secret` (what the key is made from: `"code"`, `"passphrase"`, or null with no key), `recovery` (`{salt, wrapped}` or null), catalog epoch/highwater/minCursor, entitlement and flags. Requires account authentication but no installation header. |
| `PUT /pairings/:id` | `{wrapped, keyGeneration}`: the account key sealed for a one-time device code, kept five minutes. `id` is 32 hex characters made from the code. At most 3 waiting per account and 30 an hour; 409 `sync_key_changed` for another generation. Either protocol, no activation needed. |
| `GET /pairings/:id` | `{wrapped, keyGeneration}`, deleted in the same statement, so it is handed out once. 404 `pairing_not_found` when absent or expired. |
| `GET /pairings/:id/state` | `{waiting}`. Reads only: the device showing the code learns it was collected. |
| `DELETE /pairings/:id` | Takes an uncollected message back. Idempotent. |
| `PUT /recovery` | `{keyGeneration, salt, wrapped}`: a replacement recovery code's copy of the key, replacing any earlier copy. Protocol 2 only; 409 `sync_key_changed` for another generation. The key, its generation and every head stay as they are. |
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

The client does that by taking a new identity. On `stale_sequence` or `sequence_reused` for a
request the server has not answered, the data folder was restored from a backup or copied, and
another run of it went on syncing under the same identity. The client drops that request with
the identity (`Budgets::replace_installation`), registers a new installation counting from 1,
and runs the pass again. The request is never replayed under a new sequence, because the other
run may have had it accepted and built on it. What it carried is still local, as unsynced
budget changes or a pending lifecycle job, and goes again through the usual version checks and
merge. This is narrower than quarantining the clone for review, on purpose: nothing local is
lost and nothing is published without those checks.

Lifecycle actions (trash, restore, purge) wait while a delivery is unsettled. The client never
finishes or supersedes a job locally while the server's answer for the slot is unknown, so a
real receipt can't arrive for a job already recorded with another outcome. An editor checkpoint
holds incoming changes to its budget only while the process that wrote it runs; one left by a
process that died stays as a draft to recover. Replacing a budget with another incarnation
still waits for every draft, and a clean budget held back reads "waiting", not "synced".

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

## Device approval and the recovery code

The account key is `Argon2id(secret, salt)` on both protocols, whatever the secret is: a
passphrase someone chose, or a recovery code the app made (24 characters, 112 random bits).
`keySetup` and `keyReset` carry `key: {salt, check}` as before, with an optional
`secret: "code"` that only tells other devices what to ask for. So an app from before recovery
codes unlocks any account: the code typed as shown into its passphrase box makes the same key.

A device that syncs hands the key to a new one through `/pairings`: it seals the key with a key
made from a one-time device code that travels between the two screens (camera or keyboard) and
never reaches the server. The server keeps the sealed key for five minutes and hands it out once.

`/recovery` holds at most one recovery copy per account: the key sealed with
`Argon2id(replacement code, its own salt)`, for someone who lost their recovery code or passphrase
but still has a device that syncs. `keySetup` and `keyReset` clear it, and a reset also deletes
the account's pairing messages, as does a protocol-1 reset (`PUT /api/v1/sync/key`).

None of what the server holds for this opens a budget: `salt`, `check`, the recovery copy and a
pairing message all need a secret the server never sees. The design, the engine's calls and the
threat table are in `docs/plans/2026-11-24-sync-device-approval.md`.

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
node scripts/sync-v2-approval.mjs
```

The instance suite needs the current
`crates/target/debug/examples/sync_qa` (`sync_qa.exe` on Windows) built from the Rust workspace. It runs
three separate processes/data folders behind fault proxies. Default stress is
210 edits; `--rounds 300` performs 900 edits. It checks full budget equality,
individual transaction preservation, editor recovery, explicit money conflicts,
stale choices, restart after lost commit response, save during a paused commit,
late old-incarnation upload, eviction, deletion/restore, 5000 transactions, and
empty-library restart. It also tests locked receipt recovery, expired entitlement,
key reset during upload, offline deletion undo, concurrent restores and deletion
CAS rejection followed by explicit review, private creation, account switching and durable
rate-limit recovery. It also checks displayed-review replacement by background sync,
a data folder restored from a backup (new identity, both runs' edits kept), a restore held back
by the server and asked for again, an editor checkpoint left by a process that has restarted,
unrelated downloads during blocked publication, safe cancellation of malformed candidates,
unreadable-cache isolation, and downloading without changing the selected budget.
`--large-transactions` changes the default 5,000-row large-budget
case. The rate-limit test advances only its disposable database window; production
limits remain unchanged. `--legacy` exercises real native v1 encryption and ancestry migration,
including old email-only session backfill, a Worker without v2 routes, v1 syncing with
no open budget, a device that moves only after another has rewritten the migrated head (its
copy links and both devices' edits are kept), and a stale legacy device returning after restore
(its copy stays local and its row says so). It also checks the move
offer (`upgradeAvailable`, none while activation is refused), a locked device refused without
the passphrase, a move with only the saved v1 key, one with the passphrase, and a device unlocked
on v1 adopting its saved key after the account moved elsewhere, and a passphrase reset on an
account still on protocol 1 while activation is on. Scheduler wakes are driven through `sync_now`; this
does not substitute for desktop/mobile background-lifecycle testing. Each run
saves an inspectable trace under `crates/target/sync-v2-instances/`.

`sync-v2-approval.mjs` runs device approval and the recovery code through the same real
processes, each account on its own devices: a first setup that makes a recovery code and keeps it
through a restart until it is confirmed saved; a second device unlocked by a typed device code,
handed out once, with a typo caught before any request; a new device's own code approved from a
device that syncs; the recovery code typed on this app and as a passphrase on an older one; a
replacement code that changes neither the key nor any head; a reset that retires the old code, the
recovery copy and a waiting approval; a setup whose answer is lost and still ends with its code
shown; an account with a passphrase that keeps it and gains approval and a recovery code; and
protocol 1, including an approval that moves the account to protocol 2. It does not cover a
camera, which only a device has.
