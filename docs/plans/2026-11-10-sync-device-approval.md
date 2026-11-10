# Sync without a typed passphrase — device approval and a recovery code

**Status:** all three steps of section 8 are built (10 November 2026) on
`claude/sync-v2-activation-blockers`: the shared engine, the Worker, and the screens on desktop,
Android and iPhone. The engine, Worker, desktop and Android code is compiled and tested, and
the desktop window and the Android app were run against a local server (Android on an
emulator, with codes typed). The iPhone screens have not been compiled (no Mac here), and
nothing has been tried on a real phone or with a real camera. Section 12 says how each part
was built where it differs from the sections before it.
**Baseline:** `claude/sync-v2-activation-blockers` (main `b36854a` plus the activation fixes).
**Companions:** [sync v2 design](2026-10-28-sync-v2-design.md), [server contract](../../web/SYNC_V2.md),
[activation review](../reviews/2026-11-10-sync-v2-activation-review.md).

## 1. What changes for the person

Today every device asks for the sync passphrase, and the encryption key is made from it. After
this work:

- **Adding a device** needs no typing when either device is a phone. The phone does the
  scanning, whichever one is new:
  - *A new phone:* the device that already syncs shows a code under Add a device; the new
    phone scans it.
  - *A new computer, and a phone that syncs:* the computer shows a code where it asks to
    unlock sync; the phone scans it under Add a device and approves.
  - *Two computers:* one shows 24 characters and they are typed on the other.
- **With no other device at hand** they enter a recovery code, saved once when sync was set up.
- **People who already use a passphrase** keep it. It works as their recovery code, on old and
  new versions of the app.
- **Changing the recovery code** no longer deletes the cloud copies.

Sync stays end-to-end encrypted. The server relays one short-lived encrypted message per
approval and never holds anything that opens a budget.

## 2. Terms

- **Account key:** the 32-byte key that seals budgets (XChaCha20-Poly1305, as now). Each device
  keeps it in its vault under the account and key generation, as now.
- **Wrapped:** sealed with another key, so only the holder of that key can read it.
- **Pairing code:** a one-time 120-bit secret a syncing device makes to hand the account key to
  a new one. Shown as a QR code and as 24 characters.
- **Recovery code:** a 120-bit secret the person keeps. The server stores the account key
  wrapped with a key made from it.

Codes use Crockford base32 (no I, L, O or U; case and hyphens ignored on entry), shown in six
groups of four.

## 3. What exists today

- `crypto::derive` makes the key from the passphrase with Argon2id (64 MiB, 3 passes) and a salt
  the server keeps. `crypto::key_check` gives the server a value only the right key opens.
- The server keeps `salt` and `key_check` per account and returns them from
  `GET /api/v2/sync/bootstrap`, for both protocols.
- v2 stores the key in the vault as `com.urbudget.sync-v2.<account>.<generation>`; v1 stores one
  key under its own name with the generation in `sync-state.json`.
- `sync_setup` and `sync_reset` are the only entry points, each taking a passphrase:
  `CloudSettings.tsx`, `CloudScreen.kt`, `AccountSyncView.swift`.
- A reset makes a new key generation, deletes the cloud copies and re-uploads from the
  resetting device. That stays the only way to change the account key.
- No app uses the camera or draws a QR code. The crates have `sha2`, `chacha20poly1305`,
  `argon2`, `getrandom` and `base64`; no key-agreement or PAKE crate.

## 4. Design

### 4.1 Handing the key to a new device

Either device can show the code, so that the one with a camera scans. A QR code says which
kind it is, and a scan on the wrong screen is told where it belongs. A typed code doesn't say:
its screen knows.

**An offer** is shown by the device that already syncs, which seals the key at once.

On the syncing device (`sync_pair_offer`):

1. Requires an unlocked key and a connection.
2. Makes a pairing code `S` (15 random bytes).
3. Derives `id = hex(SHA-256("urbudget:pairing:id:v1" ‖ S))[..32]` and
   `k = SHA-256("urbudget:pairing:key:v1" ‖ S)`. `S` is uniformly random, so a
   domain-separated hash is a sufficient KDF and no new crate is needed.
4. Seals the account key with `crypto::seal(k, aad, key)`, where `aad` is the canonical JSON
   `{"accountId","id","keyGeneration","purpose":"pairing"}`.
5. Sends it with `PUT /pairings/<id>`.
6. Returns the 24 characters, the QR modules and the expiry for the screen. The QR holds
   `URBUDGET-PAIR:1:<24 characters>`: plain text, not a link, so no other app or the system
   camera opens it.

On the new device (`sync_pair_accept`, after sign-in):

1. Takes `S` from the scanner or the keyboard and derives `id` and `k`.
2. Fetches `GET /pairings/<id>`, which returns the message once and deletes it.
3. Opens it, and checks the result against `bootstrap.check` with `crypto::matches_check`
   before storing anything.
4. Stores the key for the bootstrap's generation, exactly where `sync_setup` stores it, and
   continues as `sync_setup` does after a correct passphrase.

Closing the screen calls `sync_pair_cancel`, which deletes the message.

**A request** is shown by the new device, which then waits. This is how a phone that syncs
adds a computer.

1. The new device (`sync_pair_request`) makes `S` and shows it. Its QR holds
   `URBUDGET-ASK:1:<24 characters>`. Nothing is sent: the code means nothing to the server
   until a key is sealed for it.
2. The phone that syncs scans it. The screen asks the person to confirm, because the key goes
   to whoever shows that code. Then `sync_pair_approve` seals the key for `S` and sends it, as
   steps 3 to 5 of an offer.
3. The new device asks `sync_pair_wait` every three seconds. It answers `{ready: false}` while
   nothing is there, and collects, checks and stores the key as `sync_pair_accept` does once
   something is. It stops asking after five minutes or when the screen closes.

The server scopes every message to the signed-in account, so a request shown by someone else's
device gets nothing from an approval unless that device is also signed in to the same account.

### 4.2 Server

Under `/api/v2/sync`, for an authenticated session of the account, on either protocol:

| Route | Contract |
| --- | --- |
| `PUT /pairings/:id` | Body `{wrapped, keyGeneration}`. `id` is 32 hex characters; `wrapped` is base64url, at most 344 characters. Stored for 5 minutes. At most 3 live per account; 30 an hour. Refused if `keyGeneration` isn't the account's. |
| `GET /pairings/:id` | Returns `{wrapped, keyGeneration}` and deletes the row in the same statement. 404 if absent or expired. 600 an hour per account: a device showing a request asks about a hundred times in five minutes. |
| `DELETE /pairings/:id` | Idempotent. |

One table, `sync_pairings_v2(user_id, id, wrapped, key_generation, expires_at)`, keyed by
`(user_id, id)` and removed with the account. Hourly maintenance deletes expired rows. A key
reset deletes the account's rows in its own batch.

These routes allocate no storage objects and touch no budget state, so they don't need the
activation flag. That lets this ship before v2 is activated.

### 4.3 Recovery code and keys not made from a passphrase

For an account set up after this work, the first device makes a random account key and a
recovery code:

- `sync_setup` with no passphrase creates the key, wraps it with
  `crypto::derive(recovery code, salt)`, and sends `keySetup` with
  `key: {check, recovery: {salt, wrapped}}` and no `salt`.
- The server stores the recovery wrap on the account and returns it from bootstrap as
  `recovery: {salt, wrapped} | null`. `salt: null` tells a client the key is not made from a
  passphrase.
- `sync_recover(code)` on a new device derives the wrapping key, opens `recovery.wrapped`,
  checks it against `bootstrap.check`, and stores it.
- `sync_recovery_create` on an unlocked device makes a new code and replaces the wrap with
  `PUT /recovery {keyGeneration, salt, wrapped}`. The old code stops working. Nothing else
  changes: same key, same generation, no re-upload.
- `keyReset` carries the new key's recovery wrap in the same manifest, so the reset and its
  recovery code commit together.

The recovery code is shown once and not stored on the device. The screen asks the person to
confirm they saved it before sync starts.

### 4.4 Accounts that already have a passphrase

- Their account key is the passphrase-derived key. Nothing is re-encrypted and no generation
  changes.
- The passphrase keeps unlocking any device, on any app version, because `salt` and `check`
  are unchanged.
- Pairing works for them as for anyone: it hands over the same key.
- They can add a recovery code. The passphrase still works afterwards, because the key is a
  pure function of it. Only a reset retires a passphrase. The screen says so.
- A reset on a new client makes a random key and a recovery code (4.3).

### 4.5 Moving to protocol 2 without asking again

When an account moves to v2, devices that were already unlocked go "locked" because v2 looks
only under its own vault name (review item S4). The v1 key for the same generation is in the
vault. At the first v2 connection, if no v2 key exists for the bootstrap's generation, the
client reads the v1 key, checks it against `bootstrap.check`, and stores it under the v2 name.
This is independent of everything else here and can ship first.

## 5. Scanning and showing codes

| Device | Shows | Takes |
| --- | --- | --- |
| Desktop | An offer under Add a device; a request where it asks to unlock sync. Both as a QR code drawn from the engine's modules; the offer also as characters. | Typed |
| Android | An offer under Add a device, as QR code and characters | Google code scanner, which needs no camera permission in the app; typed if Play services can't provide it. Scans offers to unlock, and requests to approve. |
| iPhone | Same as Android | VisionKit scanner, which needs the camera permission and a usage string; typed if refused |

QR modules come from one implementation in the shared crate (the `qrcode` crate), so the three
apps draw the same code. The screen that shows a pairing code says to enter it only on the
person's own device.

## 6. What the server and an attacker can get

| Who | What they hold | What they can do |
| --- | --- | --- |
| The server, or a copy of its database | Ciphertext, `check`, the recovery wrap and salt, a pairing message for up to 5 minutes | Nothing without a 120-bit code. For passphrase accounts, guess weak passphrases offline, as today. |
| Someone who can read the person's email | A signed-in session | Fetch the recovery wrap, which they can't open. Replace it or delete a pairing message, which blocks recovery or one approval but exposes nothing; a wrong wrap fails the check and is refused. They never see an offer's code, which exists only on the syncing device's screen. They can show a request on a device of their own, and get the key only if the person scans that code and confirms adding it: the confirmation, which says the device will read every budget, is the defence. |
| Someone the person reads a pairing code to | The account key | Everything. The screen's wording is the defence. |
| Someone holding an unlocked device | The key already | Add devices, as they could already read the budgets. |

Removing a device does not take the key back from it. Only a reset does.

## 7. Compatibility

- Pairing (4.1, 4.2) and the v1-key reuse (4.5) change nothing an older client sees.
- An account with a random key (4.3) can't be unlocked by a client from before this work: it
  needs a `salt`. The server must refuse those clients on such accounts with
  `426 sync_upgrade_required`. That is the same minimum-version gate activation needs
  (review item R4), so build it once.
- Old receipts and manifests keep their shape. `key.salt` becomes optional; `key.recovery` is
  new and optional.

## 8. Order of work

1. **Reuse the v1 key at the move to protocol 2** (4.5). Shared Rust only.
2. **Pairing** (4.1, 4.2, 5). Worker routes and table, shared Rust calls, an Add a device
   screen and a Scan or enter a code screen in each app. Works for every existing account and
   needs no version gate.
3. **Random keys and recovery codes** (4.3, 4.4). Needs the minimum-version gate. Changes
   first-time setup and reset in each app, and the wording on the site, the privacy policy and
   the store listings, which say the key is made from the passphrase.

Each step is useful alone and leaves the next one optional.

## 9. Tests

- **Rust:** code encoding round trip and rejection of bad characters and lengths; the id and
  key derivation against fixed vectors; a message sealed for one account, generation or id
  doesn't open for another; a fetched key that fails the check is not stored.
- **Worker:** a message is returned once; expiry; the three-live and hourly limits; another
  account can't fetch it; a reset deletes it; a stale `keyGeneration` is refused; the recovery
  wrap follows the generation.
- **Instance suite:** a device pairs from another and syncs with no passphrase call; a wrong
  or reused code fails and leaves the new device locked; pairing on a protocol-1 account;
  recovery on a device with no peer; a new recovery code makes the old one fail without a
  re-upload; a device unlocked on v1 syncs on v2 without a setup call.
- **By hand, on devices:** scanning in poor light and at an angle, the typed fallback, the
  iPhone permission prompt and its refusal, and the Android scanner on a phone without Play
  services.

## 10. Left out on purpose

- **A new phone showing a request.** A new phone scans an offer instead, so the phones only
  show offers.
- **A short code.** Eight digits would need a PAKE and a new crate to stay safe against the
  server.
- **Moving passphrase accounts to a random key without a reset.** That is a re-encryption.
- **Platform keychain sync.** It covers one ecosystem and not a phone with a Windows PC.

## 11. Decisions needed

1. **Can a new account choose its own passphrase as its recovery secret**, or is it always a
   generated code? A chosen passphrase can be guessed offline from a database leak; a generated
   code can't. This proposal assumes generated only.
2. **iPhone camera permission**, with a new App Review declaration, or typed codes only on
   iPhone at first.
3. **Should pairing ship before v2 activation**, on protocol-1 accounts too? This proposal
   assumes yes.

It was built with all three as assumed. None has been confirmed one by one.

## 12. As built

Where the build differs from the sections above, or settles something they left open:

- **Who gets a key with no passphrase.** An account that sets sync up through protocol 2: one
  already on it, or one the server offers it to. An account on protocol 1 still chooses a
  passphrase, because older apps share that protocol and need a `salt`. The engine's
  `sync_key_info` tells a screen which it is (`setup` and `reset` are `"code"` or
  `"passphrase"`), along with whether a key exists, whether a passphrase opens it, and whether
  it has a recovery code.
- **The engine's calls.** `sync_setup` and `sync_reset` take no passphrase to make the key
  here, and answer with `recoveryCode`. `sync_recovery_create` replaces it. With a passphrase,
  both still work as before. `sync_unlock_code {code}` takes whichever code the person has,
  one a device that syncs shows or the recovery code, so a screen has one box and needn't ask
  which it is.
- **The recovery copy is bound** to the account and the key generation (4.3 didn't say). A
  reset's copy is for the generation the reset creates.
- **A lost answer.** If the app closes, or the answer is lost, after the server took a setup or
  a reset, the code for that attempt was never shown. The next attempt completes it and
  replaces the recovery code with one it shows.
- **The version gate (section 7)** is a level the client sends, `X-Sync-Client: 2`, not a
  version number: desktop 1.0.11 and this build would otherwise both be 1.0.11. A client that
  sends none is never offered protocol 2, and is told to update on an account that is on it.
- **A staged start** was added with it: `SYNC_V2_ACCOUNTS` lists the account ids that may move
  before `SYNC_V2_ENABLED` is on (`web/SYNC_V2.md`, Switching it on).
- **The screens show one thing to do.** They were built with every option in view and then cut
  down the same day, at the owner's request ("simple, and it just works"):
  - *First device:* Turn on sync, one tap, with no screen before it. Then the recovery code,
    whose only way out is "I've saved it".
  - *A device that doesn't sync yet:* its passphrase if the account has one. Otherwise one
    screen: scan (a phone), or be scanned (a computer shows its own code), and one box that
    takes a typed code of either kind. A reset is under "Can't unlock?".
  - *Add a device, on a phone:* the code to scan, and one button for scanning a computer's.
    Typing a computer's code into a phone is gone: the computer can take the phone's instead.
  - *Where sync is unlocked:* Sync now and Add a device. Recovery code, Reset sync and Lock
    are under More.
- **What a reset costs on protocol 2.** Every device, the one that reset included, keeps its
  budgets as recovered copies to bring back and add to sync again by hand. That is how
  protocol 2 already treated a reset; the reset dialogs now say so. It is the roughest part
  of this, and worth its own look.
- **The site** (the privacy policy, the terms, the pricing answers and the comparison pages)
  no longer says the key is made from a passphrase, in words that are true for both kinds of
  account. The privacy policy now lists the recovery copy and the five-minute pairing copy.
  The store listings (`docs/play-listing`) are not changed.
