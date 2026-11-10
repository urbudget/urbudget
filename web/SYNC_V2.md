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

Each head the migration seeds also records where it came from, in `legacy`
(migration `0013_sync_v2_legacy_origin.sql`): the incarnation the migration gave
it and its v1 revision. Nothing writes it again. Every head carries it as
`"legacy":{"incarnation":"…","revision":"…"}`, or `null` for a budget made on
protocol 2. A device that still has the budget's v1 sync history links its copy
while `legacy.incarnation` equals the head's incarnation and its last synced
revision is no later than `legacy.revision`, however often other devices have
rewritten the head since. It links at the revision it last had, then merges what
it missed. A restore gives the head a new incarnation, the two stop matching,
and that device keeps its copy beside the account's budget with a note on its
row. Clients from before this field link only while the payload is still
`legacy-v1`.

A passphrase reset on an account still on protocol 1 goes through `/api/v1`,
even with activation on: `keyReset` needs protocol 2, and moving to it proves
the old passphrase. The account moves over at its next unlock.

### Switching it on

Three variables, all absent in production today (`wrangler.jsonc` lists them as
comments where they go):

| Variable | What it does |
| --- | --- |
| `SYNC_V2_ACCOUNTS` | A staged start. Account ids (`usr_…`), comma-separated: only these may set up or move to protocol 2. Find an id with `SELECT id FROM users WHERE email = …`. |
| `SYNC_V2_ENABLED=1` | Protocol 2 for every account. |
| `SYNC_V2_PUBLICATION_PAUSED=1` | The incident brake described below. |

An account that has moved to protocol 2 stays there whichever of the first two is
set afterwards: they decide who may move, not who is on it.

Every client sends `X-Sync-Client` with the level of protocol 2 it understands.
The server's level is 2: an account's key may not be made from a passphrase
(`salt` is null), and the client has the fixes of
`docs/reviews/2026-11-10-sync-v2-activation-review.md`. A client from before the
level sends nothing. Bootstrap answers it with `activationEnabled: false` on an
account that isn't on protocol 2, so it keeps syncing through protocol 1 and never
starts a move. On an account that is on protocol 2 every route answers it
`426 sync_upgrade_required` with a message to update, including bootstrap. So
desktop 1.0.11, which has protocol 2 without those fixes, can't activate anyone.

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
| `GET /bootstrap` | Account ID, protocol, nullable key generation/salt/check, `recovery: {salt, wrapped} \| null`, catalog epoch/highwater/minCursor, entitlement and flags. `salt` is null for a key that isn't made from a passphrase. Requires account authentication but no installation header. |
| `PUT /recovery` | `{keyGeneration, salt, wrapped}`: replaces the recovery code's copy of the key. Protocol 2 only; 409 `sync_key_changed` unless `keyGeneration` is the account's. 20 an hour. Changes nothing else. |
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
| `PUT /pairings/:id` | Device approval. `{wrapped, keyGeneration}`: the account's sync key sealed by a device that holds it, kept 5 minutes. `id` is 32 hex characters; `wrapped` is base64url, at most 344 characters. At most 3 waiting per account, 30 an hour. 409 `sync_key_changed` unless `keyGeneration` is the account's; 404 `no_key` if it has none. Sending the same `id` again replaces it. |
| `GET /pairings/:id` | Returns `{wrapped, keyGeneration}` and deletes it in the same statement. 404 `pairing_not_found` if absent, expired or already taken. 600 an hour: a new device showing its own code asks every three seconds until one that syncs has approved it. |
| `DELETE /pairings/:id` | Takes back an unused message; idempotent. |
| `POST /mutations/:sequence/cancel` | `{digest,manifest?}`; exact manifest required if prepare never arrived. Checks authenticated account/installation, operation ID, known kind, canonical digest and sequence fences, then records a cancelled receipt without applying the body. Malformed or oversized payload descriptors can therefore be safely cancelled. Existing terminal receipts replay unchanged. |

The pairing routes (migration `0014_sync_pairings.sql`) let a device that syncs hand
the key to one that has just signed in, so the passphrase isn't typed there. The
device with the key makes a 120-bit one-time code, derives the message ID and a
sealing key from it by hashing under two labels, seals the sync key, and shows the
code on its screen as a QR code and as 24 characters. The new device is given the
code by camera or keyboard, collects the message once, opens it, and keeps the key
only if it opens the account's `check`. The code can also start on the new device:
it shows one and waits, a phone that syncs scans it, the person confirms, and the
phone seals the key under that code. The Worker sees the same `PUT` and `GET` either
way. The code never reaches the Worker, which therefore holds nothing that opens a
budget. The routes need a session and nothing
else: no installation header, no subscription, no activation, and they work on both
protocols, reading the key generation as bootstrap does. A key reset on either
protocol deletes the account's waiting messages; hourly maintenance deletes expired
ones. The design is in `docs/plans/2026-11-10-sync-device-approval.md`.

When an account moves to protocol 2, a device already unlocked on protocol 1 takes
its key over at its first pass instead of asking for the passphrase again: the move
keeps the key, and the device checks it against `check` first.

### Keys not made from a passphrase, and the recovery code

`keySetup` and `keyReset` carry `key: {check, salt?, recovery?: {salt, wrapped}}`
(migration `0015_sync_recovery.sql`). A key has `salt` when it is made from a
passphrase and `recovery` when it has a recovery code; a manifest with neither is
refused, because nothing could ever unlock a second device.

An account's first device now sets sync up with no passphrase. It makes a random
key and a 120-bit recovery code, stretches the code with Argon2id and a salt of
its own into a wrapping key, seals the key with it (bound to the account and the
key generation), and sends `key: {check, recovery}` with no `salt`. It shows the
code once, for the person to keep, and doesn't store it. From then on a device
gets the key from one that syncs (the pairing routes) or, with none at hand, by
opening `recovery.wrapped` with the code. Either way it keeps the key only if it
opens `check`.

A device where sync is unlocked can replace the code with `PUT /recovery`. The
old code stops working; the key, its generation and every budget stay as they
are. An account whose key is made from a passphrase can add a code the same way,
and its passphrase keeps working. A reset replaces the key and carries the new
key's `recovery` in the same manifest, so the two commit together; a reset to a
passphrase key leaves no recovery code behind.

If the answer to a setup or a reset is lost after the server took it, the code
made for that attempt was never shown. The client's next attempt finds the key it
made still in its vault, completes, and replaces the recovery code with one it
shows.

The server keeps `recovery.salt` and `recovery.wrapped`, neither of which opens a
budget, and never sees the code. Unlike a passphrase, the code has 120 random
bits, so a copy of the server's data gives no way to guess it. An account on
protocol 1 has no recovery code: its key is always made from a passphrase.

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

The client does that when prepare, cancel or commit answers `stale_sequence` or
`sequence_reused` for a request it holds no receipt for: its data folder was
restored from a backup or copied, and another run of it went on under the same
identity. It drops the unanswered request, takes a new installation ID starting
at sequence 1, registers it, and runs the pass again. The request is never sent
again under the new sequence. What it carried is still on the device, as the
budget's unsynced changes or a pending trash/restore, and goes through the same
checks as any device's work: the head's version and incarnation at commit, the
merge against the copy's last synced base, a deletion's reviewed version. The
old installation stays registered with its fence until someone retires it.
`sequence_gap` and `installation_retired` are not handled this way and still
stop that installation's uploads.

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
It restores a device's data folder from an earlier copy and checks that it takes a
new installation identity and keeps its edits; holds a restore back at prepare,
asks again, and checks the first request still completes with its own receipt;
and checks that an editor checkpoint blocks incoming changes only while its
process lives, with the status reading waiting meanwhile.
Its last scenario sets sync up on a new account with nothing typed. It loses the
answer to the first attempt and checks the retry ends with a recovery code that
works; unlocks a second device with that code and a third by pairing; replaces
the code and checks the old one fails while no budget is uploaded again; and
resets with nothing typed, after which the other devices are locked until the new
code or a pairing gives them the new key. `--only <words>` runs the scenarios
whose names have them, for one that stands alone, such as that one.
`--large-transactions` changes the default 5,000-row large-budget
case. The rate-limit test advances only its disposable database window; production
limits remain unchanged. `--legacy` exercises real native v1 encryption and ancestry migration,
including old email-only session backfill, a Worker without v2 routes, v1 syncing with
no open budget, a device that unlocks after another has rewritten the migrated head and
links and merges its unsynced work, a stale legacy device returning after restore, and a
passphrase reset on a protocol-1 account while activation is on. Scheduler wakes are driven through `sync_now`; this
does not substitute for desktop/mobile background-lifecycle testing. Each run
saves an inspectable trace under `crates/target/sync-v2-instances/`.
