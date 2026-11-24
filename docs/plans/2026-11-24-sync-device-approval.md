# Sync without a typed passphrase: device approval and a recovery code

**Status:** built on `claude/sync-device-approval` (24 November 2026). Section 12 says what was
run and what wasn't.
**Baseline:** main `7e77b1a`, with sync v2 live since 22 November.
**Replaces:** the 10 November design on `claude/sync-v2-activation-blockers` (closed PR #19),
which was built before v2 went live and never merged. Section 11 lists what changed from it and why.
**Companions:** [sync v2 server contract](../../web/SYNC_V2.md).

## 1. What changes for the person

- **Turning sync on** is one tap. The app makes a recovery code and shows it; nothing is chosen
  or typed.
- **Adding a device** needs no typing when either device is a phone, which does the scanning:
  - *A new phone:* a device that already syncs shows a code under Add a device; the phone scans it.
  - *A new computer, and a phone that syncs:* the computer shows a code where it asks to unlock
    sync; the phone scans it under Add a device and approves.
  - *Two computers:* one shows 24 characters and they are typed on the other.
- **With no other device at hand** they type the recovery code.
- **A lost recovery code** is replaced from any device that still syncs, with nothing uploaded
  again.

Sync stays end-to-end encrypted. The server relays one short-lived sealed message per approval
and holds nothing that opens a budget.

## 2. What doesn't change (the transition)

This is the rule the rest follows: **nobody who syncs today has to do anything, and no released
app stops working.**

- The account key is still `Argon2id(secret, salt)`, with `salt` and `check` kept by the server
  exactly as now. For an account set up before this work the secret is the passphrase. For one
  set up after it, the secret is the recovery code the app made.
- So an account that has a passphrase keeps it. It unlocks any device, on any version. Nothing is
  re-encrypted, no key generation changes, nothing uploads again, and a device that is unlocked
  stays unlocked through the update.
- And an older app can join a new account: the recovery code, typed as it is shown into that
  app's passphrase box, makes the same key. No version gate, no "update to continue".
- The server change is additive: one table, three columns, five routes, two bootstrap fields.
  Old apps ignore all of it. Key setup and reset keep their shape, with one optional field.
- Moving an account from protocol 1 to protocol 2 works as before; a device that gets the key by
  approval moves the account exactly as one that typed the passphrase would.

## 3. Terms

- **Account key:** the 32-byte key that seals budgets. Each device keeps it in its vault.
- **Secret:** what the key is made from: a passphrase, or a recovery code.
- **Code:** 24 characters in Crockford base32 (no I, L, O or U), shown in six groups of four.
  It carries 112 random bits and a tag of 8 bits: seven check a typed code for typos before
  anything slow or online happens, and one says which kind it is.
  - **Device code** (kind 0): one-time, five minutes, hands the key to one device.
  - **Recovery code** (kind 1): kept by the person.
- **Recovery copy:** the key sealed with a key made from a *replacement* recovery code, kept by
  the server. Only an account whose recovery code was replaced has one.

## 4. Codes

`code = random(14 bytes) ‖ tag`, where
`tag = (SHA-256("urbudget:code:v1" ‖ kind ‖ random)[0] & 0xFE) | kind` and `kind` is one byte, 0 or 1.
The check covers the kind, so a typo in the last character can't turn a device code into a
valid recovery code.

Reading one accepts any case, hyphens or spaces, and O, I and L for 0 and 1. A code whose tag
doesn't fit is a typo, said at once (127 in 128 single typos; the rest fail as a wrong code).
Where the account's key is made from a passphrase, 24 code characters with a tag that doesn't
fit are tried as the passphrase first, since a passphrase could be exactly that. The canonical text is upper case with hyphens,
`XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`: that exact text is what a recovery code's key is made from, so
it works typed into an app from before this work.

## 5. Handing the key to a new device

Either device can show the code, so that the one with a camera scans. A QR code says which kind
it is; a typed code doesn't need to.

**An offer** is shown by the device that syncs (`sync_pair_offer`):

1. Makes a device code `S`.
2. `id = hex(SHA-256("urbudget:pairing:id:v1" ‖ S))[..32]`, `k = SHA-256("urbudget:pairing:key:v1" ‖ S)`.
3. Seals the account key with `k`, bound to `{accountId, id, keyGeneration, purpose:"pairing"}`.
4. `PUT /pairings/<id>`. The QR holds `URBUDGET-PAIR:1:<24 characters>`: plain text, not a link.
5. While the screen is open it asks `sync_pair_state` every few seconds and closes itself with
   "Device added" once the server says the message was collected. A code that ran out says
   neither waiting nor collected, so the two are never confused.

The new device gives what it scanned or what was typed to `sync_unlock`, which collects the
message (`GET /pairings/<id>`, handed out once), opens it, checks the key against the account's
`check`, and unlocks as a right passphrase would.

**A request** is shown by the new device (`sync_pair_request`), which then waits. Its QR holds
`URBUDGET-ASK:1:<24 characters>`. Nothing is sent. A phone that syncs scans it, the person
confirms, and `sync_pair_approve` seals the key for it as an offer does. The new device asks
`sync_pair_wait` every three seconds until the key is there.

Closing either screen takes the message back (`sync_pair_cancel`).

## 6. The recovery code

- **Made with the key.** `sync_setup` with no passphrase, on an account with no key, makes a
  recovery code and sets sync up with that code as the secret. A reset with no passphrase does
  the same for the new key. The key details say `secret: "code"`, so other devices ask for a
  recovery code and not a passphrase.
- **Kept until it's saved.** The code goes into the device's vault before the request leaves,
  and stays until the person says they saved it (`sync_recovery_saved`). If the app closes or
  the answer is lost, the code is still shown the next time (`recoveryUnsaved` in status,
  `sync_recovery_unsaved`). An interrupted setup is finished with that same code.
- **Replaced without a reset.** `sync_recovery_create`, where sync is unlocked on protocol 2,
  makes a new code and leaves the server a recovery copy: the key sealed with
  `Argon2id(new code, new salt)`, bound to `{accountId, keyGeneration, purpose:"recovery"}`
  (`PUT /recovery`). A new device given that code opens the copy. The secret the key was made
  from (the first code, or the passphrase) still works: only a reset retires it, and the screen
  says so.
- A reset clears the recovery copy with the key it belonged to.

## 7. One call unlocks

`sync_unlock {secret}` takes whatever the person scanned or typed:

| What it is | What happens |
| --- | --- |
| `URBUDGET-PAIR:1:…` or a device code | Collects the sealed key for it. |
| `URBUDGET-ASK:1:…` | Refused: that code belongs on Add a device, on a device that syncs. |
| A recovery code | Makes the key from it with the account's `salt`. If that isn't the key, opens the recovery copy with it. |
| Anything else | A passphrase: makes the key from it as typed. |

Every key is checked against `check` before it is kept. `sync_setup {passphrase}` stays for
callers that have one.

## 8. Engine calls and status

| Call | On | Answer |
| --- | --- | --- |
| `sync_key_info` | signed in | `{exists, secret: "code"\|"passphrase"\|null, recovery, protocol}` from a fresh bootstrap |
| `sync_setup {passphrase?}` | first device, or unlock | `{created, protocol?, recoveryCode?}` |
| `sync_unlock {secret}` | a device that doesn't sync yet | `{created:false, protocol?}` |
| `sync_reset {passphrase?}` | any | `{created:false, protocol?, recoveryCode?}` |
| `sync_recovery_create` | unlocked, protocol 2 | `{recoveryCode}` |
| `sync_recovery_unsaved` | any | `{recoveryCode}` or `null` |
| `sync_recovery_saved` | any | `null` |
| `sync_pair_offer` | unlocked | `{id, code, qr:{size,modules}, expiresAt}` |
| `sync_pair_state {id}` | unlocked | `{waiting, collected}` |
| `sync_pair_cancel {id}` | any | `null` |
| `sync_pair_request` | signed in | `{id, code, qr, expiresAt}` |
| `sync_pair_wait {code}` | the device showing a request | `{ready:false}` or `{ready:true, created:false, protocol?}` |
| `sync_pair_approve {code}` | unlocked | `null` |

Status (`sync` in `cloud_status`) gains, from the bootstrap the last sync saved:
`secret` (`"code"`, `"passphrase"` or `null`), `recovery` (the account has a recovery copy) and
`recoveryUnsaved` (this device holds a code the person hasn't confirmed saving).

## 9. Server

Under `/api/v2/sync`, for a signed-in session, on either protocol:

| Route | Contract |
| --- | --- |
| `PUT /pairings/:id` | `{wrapped, keyGeneration}`. Kept 5 minutes. At most 3 live per account, 30 an hour. Refused if the generation isn't the account's. |
| `GET /pairings/:id` | `{wrapped, keyGeneration}`, once: the row is marked collected and its sealed key blanked in the same transaction. 404 if absent, expired or collected. |
| `GET /pairings/:id/state` | `{waiting, collected}`. Reads only. Neither once the code ran out or was taken back. |
| `DELETE /pairings/:id` | Idempotent. |
| `PUT /recovery` | `{keyGeneration, salt, wrapped}`. Protocol 2 only; refused if the generation isn't the account's. |

`GET /bootstrap` gains `secret` and `recovery: {salt, wrapped} | null`. `keySetup` and
`keyReset` take an optional `key.secret: "code"`, and both clear the recovery copy; a reset also
deletes the account's pairing messages. Migrations `0017_sync_pairings.sql` and
`0018_sync_recovery.sql`.

## 10. What the server and an attacker can get

| Who | What they hold | What they can do |
| --- | --- | --- |
| The server, or a copy of its database | Ciphertext, `salt`, `check`, the recovery copy, a pairing message for up to 5 minutes | Nothing against a code: 112 random bits. Against a chosen passphrase, guess weak ones offline, as today. |
| Someone who can read the person's email | A signed-in session | Fetch `salt`, `check` and the recovery copy, none of which opens without the secret. Replace the recovery copy or delete a pairing message, which blocks one recovery or approval and exposes nothing: a wrong copy fails `check`. Show a request on a device of their own and get the key only if the person scans it and confirms. |
| Someone the person reads a device code to, who is also signed in | The account key | Everything. The screen says to use a code only on a device they hold. |
| Someone holding an unlocked device | The key already | Add devices and make a recovery code, as they could already read the budgets. |

Removing a device or replacing a code doesn't take the key back. Only a reset does.

## 11. What changed from the 10 November design

| Then | Now | Why |
| --- | --- | --- |
| A new account's key is random; the recovery code opens a server copy of it. | The key is made from the recovery code, as a passphrase's is. A server copy exists only for a replacement code. | Every released app can still join every account. No protocol field changes meaning. |
| `X-Sync-Client: 2` gate; older apps told to update. | No gate. | Released v2 apps don't send it; the gate would have locked them all out. |
| The recovery code is shown once and not stored. | Kept in the vault until confirmed saved. | A crash or a lost answer no longer loses the code. |
| Codes are 120 random bits. A typed code is tried against the server, then stretched as a recovery code. | 112 random bits and a tag. A typo, and which kind a code is, are known before any request. | Clear, immediate errors for typed codes. |
| `sync_unlock_code`, `sync_recover`, `sync_pair_accept`, and `sync_setup` for a passphrase. | `sync_unlock` takes all four. | One box on the screen; one path to test. |
| The offer screen stays until closed. | It closes itself when the new device has the key. | The person sees it worked on both devices. |
| Random keys only through protocol 2. | Any setup makes a code. | The key is made the same way on both protocols. |

Left out, as before: a new phone showing a request, a short code (needs a PAKE), moving an
account to a new key without a reset, and platform keychain sync.

## 12. As built and tested

**Where it is.** Engine: `crates/urbudget-services/src/cloud/{codes,pairing,recovery}.rs`, with
the setup path in `v2.rs` and the calls in `sync.rs`. Server: `web/src/sync-v2/index.ts`,
migrations 0017 and 0018. Screens: `desktop/src/CloudSettings.tsx`,
`android/.../ui/{CloudScreen,DevicePairing,RecoveryCode,CodeScanner}.kt`,
`ios/UrBudget/Screens/Sync/`.

**The screens.**
- *No key yet:* Turn on sync, one tap, then the recovery code, whose only way out is "I've saved
  it". It is shown again whenever the device still holds an unconfirmed code.
- *A device that doesn't sync yet:* a phone scans; a computer shows its own code for a phone that
  syncs to scan. Either has one box that takes a typed code, the recovery code, or the passphrase
  of an account that has one. A reset is under "Can't unlock?".
- *Where sync is unlocked:* Sync now and Add a device, which closes itself with "Device added".
  New recovery code, Reset sync and Lock are under More.
- *Reset* says what it does on each protocol. On protocol 2 the device that resets uploads the
  budgets it was syncing again by itself, and every other device receives them and keeps its own
  copy to recover from the budget library (below).

**A reset on the device that asks for it.** As first built, a protocol-2 reset left every
device, the one that reset included, with each budget that was syncing as a copy to recover by
hand, which is how protocol 2 had always treated a reset. The device that resets now puts its
budgets back by itself. When its reset is accepted the engine notes the key generation it made,
and the sync pass `sync_reset` runs before it answers gives each budget that was syncing there a
new identity under the new key and uploads it as it is. The person does nothing, and the budget
they have open stays open.
- Only the device that asked. With the server's copies gone, no device can show that its copy is
  the same budget as another device's, so two devices adding theirs back would make two of each.
  The others keep today's behaviour, and receive the resetting device's budgets as new ones.
- Not a budget with a conflict waiting for review, nor one holding a draft that was never saved.
  Those stay copies to recover on the resetting device too, from its next sync on: the app hears
  about that sync, so an open budget is never closed without the screen being told.
- Nothing from before the reset is sent again: the file drops its old identity and any upload
  prepared for it, and the budget's first request under the new key creates it under new ids.
- A reset whose answer was lost ends the same way, whether the person retries or the next sync
  completes it.

The mechanism and its limits are in `web/SYNC_V2.md`, After a key reset.

**Scanning.** iPhone: VisionKit's scanner, with the camera permission. Android: the app's own
camera screen (CameraX, ZXing reading frames on the phone), with the camera permission asked for
at the first scan. Google's code scanner needs no permission, but it reports usage to Google, and
the privacy policy says the app has no third-party analytics. Either phone falls back to typing.

**Tested.**
- Engine and server unit tests, including fixed vectors for codes and pairing, and the
  server's hand-out-once, limits, expiry and reset rules.
- `web/scripts/sync-v2-approval.mjs`: eleven scenarios through real engine processes and a local
  Worker (`web/SYNC_V2.md`, Local verification). One of them runs the engine as it was before
  this work (`URBUDGET_OLD_QA`) beside this one: it joins a new account with the recovery code
  in its passphrase box, keeps syncing through a replacement code and an approval, and resets
  with a passphrase that this engine then unlocks with. Another resets from a device that can't
  unlock and finds its budget back in sync when the reset answers.
- `sync-v2-instances.mjs`: its scenarios from before, unchanged, and four for the reset. The
  resetting device's budgets are active, under new identities and on the server when
  `sync_reset` answers; a second device receives them after unlocking and still recovers its own
  copy with an edit only it had; a reset whose answer was lost ends the same way after a retry
  and after the next sync alone; and a budget with a conflict or a draft stays a copy to recover.
  They fail with the engine's note of the reset taken out.
- `--legacy` and `sync-v2-protocol.mjs` unchanged.
- Desktop: type-check and component tests with the engine faked, for every screen above.
- Android: unit tests (among them ZXing reading a QR code the engine drew), lint, and debug and
  preview builds.
- iPhone: unit tests on the Simulator, and the translation check.

**Not tested.** No screen was used on a phone, an emulator or the desktop app against a real
server, the reset's new wording included, and no camera has scanned a code from a screen. Those need a person and two devices:
scanning in poor light and at an angle, the camera permission asked and refused on each phone,
a computer's code scanned from a phone, and Add a device closing itself.

**Before release.**
- Merging to main deploys `web/`: migrations 0017 and 0018, the routes, and new wording in the
  privacy policy (the recovery copy, the five-minute pairing copy, the camera), the terms and the
  pricing answers. Read those first.
- The server goes first, as it does on merge. An app with these screens against a server without
  them can't turn sync on.
- Android adds the CAMERA permission, and iPhone a camera usage string. Both need the stores'
  declarations updated, and the Play listing still says "encrypted with your own passphrase"
  (`docs/play-listing/README.md`), as do the review notes that tell a reviewer to set one.

## 13. Found on real devices

The first run between a real iPhone and the desktop app (24 November) found three things the
automated suites had not, all fixed on this branch.

- **Budgets sync unless someone says otherwise.** The phone's camera read the desktop's code
  and the phone unlocked, but the budget never arrived: it was made before sync was turned on,
  and such a budget stayed "only on this device" until added by hand in the budget library.
  Now every budget with no account joins the signed-in account once sync is on and unlocked,
  on every device and on every pass (`v2::enroll_local`), and turning sync on adds the
  device's budgets at once. The exceptions are a budget created with "Keep this budget only on
  this device" (now remembered, in the library's `device_only` table, and taken back by adding
  the budget to sync), the app's samples, a budget that belongs to another account, and an
  unlinked copy of a budget the account already has. Nothing is matched by name: a budget a
  second device made on its own becomes a second budget in the account. An earlier version did
  not record the "keep on this device" choice, so a budget it was made for syncs after this.
  A "This device only" row in Account & sync has its Sync button on protocol 2 as well.
- **The iPhone app ran out of stack in a sync pass.** Swift calls the engine from its
  concurrency threads, which have 512 KB. An unoptimized engine overflows that as soon as a
  pass has a budget to work on (a bus error at the stack guard, in `process_budget`). Every
  service call from the iPhone app now runs on its own thread with 8 MB
  (`urbudget-ios`, `on_service_stack`), in every build.
- **A debug build forgot its test server.** Reopened from the Home Screen, it talked to
  urbudget.app. A debug build now remembers the server it was last launched against
  (`DebugServer`, debug builds only).

## 14. Account & sync, simplified, with the account's devices

Asked for by the owner after using the screens: show which devices are connected, and make the
screen simpler. One screen, top to bottom. Each part shows only when it has something to say.

1. **Account.** Signed out: the email, then the code, one button each. Signed in: one row with
   the email; Sign out is a quiet action on it.
2. **Plan.** Only when there is something to do or to know: no plan (the price, Subscribe, and
   the links the stores require beside it), a plan that is ending, a payment that failed. A plan
   in good standing takes no room here; its details are under More.
3. **Sync.**
   - Off: **Turn on sync** and one sentence.
   - Locked: as section 12 (scan or be scanned, one box, "Can't unlock?").
   - On: one status line, "Sync is on. Last synced 2 minutes ago.", with **Sync now** beside
     it. When something needs the person, the line says what, in plain words.
   - The offer to move to the new sync stays here while it applies.
4. **Devices**, while sync is on. One row per device that syncs (`sync_devices`): its name; the
   device in hand reads "This iPhone", "This phone" or "This computer"; the others say when they
   last synced ("Last synced 5 minutes ago", from `lastSeenAt`). A device with no name yet reads
   "A device added on 3 November". **Add a device** ends the list. Under it, one line: "To stop
   a device syncing, sign out on it. To cut off a device you no longer have, reset sync." The
   list only shows: removing a device from here would not take back the key it holds.
5. **Budgets**, only the ones that need something: differences to review, a kept copy, one kept
   only on this device (with its Sync button), one that failed. A row is a name, one plain
   sentence and one action. Budgets that are simply syncing are not listed; one line counts them
   and leads to the budget library ("3 budgets are syncing").
6. **More**, closed until opened: Sync automatically (phones), New recovery code, Lock sync on
   this device, Reset sync, the plan's details with Manage subscription and Restore purchases,
   Privacy & your choices, Delete account.

No state names from the engine reach the screen ("local_only", "Signed Out"), and no words about
how it works ("protocol", "key generation"). Every flow of section 12 behaves as it did; this
changes where things are, not what they do.

The server keeps each device's name and when it was last seen (migration 0019,
`POST /installations {device}`, `GET /installations`), and a device that registered before
that sends its name once more.

