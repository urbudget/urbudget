# Fast edits: the open budget lives in the engine

Status: proposal, 2026-11-04. Android's phases 0, 1, 2, 3, 4, 7 and 6 are built as of 2026-11-05.
Phase 5, the iPhone, was built on 2026-11-11 and isn't measured.

## As built

This section is current. The rest of the page is the proposal as written on 2026-11-04, kept as the
record. Where the two differ this section is right, and the proposal's contract isn't what's in the
code.

### The contract

```text
edit_budget { steps, expectedToken, month, asOfMonth, light }
  -> { patch, summary, token, savedAt, databasePath, isNew, undoAvailable, redoAvailable }
```

- `steps` is a list, not one `method`. A step is `{ patch }` or `{ method, params }`. The steps run
  in order on one working copy, then make one commit and one undo. `saveRepeating` needs this: it's
  a patch and two commands.
- An edit runs on a clone of the store's draft. It doesn't use `change_draft`, which hides `source`,
  or `apply_patch`, which writes `recovery.json` every time. Both stay for desktop. An edit writes
  that file only if its commit fails.
- Totals run before the commit, so totals that fail save nothing.
- The token is checked against the store alone. A stale cache under a matching token is reloaded,
  not refused.
- An empty `patch` means nothing changed: the token stays and no undo is pushed.

The methods around it:

| Method | What it does |
| --- | --- |
| `query_budget { method, params, expectedToken }` | A read on a copy of the store's budget, or a dry run of a changing method, which replies `{ summary, transactions }` and commits nothing. It's outside the command queue. With `expectedToken`, a store that has moved on replies `CONFLICT:` |
| `undo_budget`, `redo_budget` with `patchOnly` | Reply as `edit_budget` does, except that `summary` can be null: the commit is done, so failed totals can't read as a failed undo. Without `patchOnly` they reply as they always did |
| `load_budget` with `light` | The light copy: no transaction `source`, no `sourceExport`, and `hasImportSource` on rows that have a `source` |
| `light` on `edit_budget` and `patchOnly` undo | The reply's patch in that form. The undo history keeps the stored form |
| `export_budget` with `compact` | The whole budget as compact text, or null with no budget open. Android's exports and recovery copies use it |
| `apply_due_schedules` with `ackOnly` | Replies with the status, so no budget crosses at each start |
| `saved_budget` | The rows as stored, for tests. No app calls it |

`urbudget_core::commands` holds the methods. `apply` returns `Outcome`, not
`Result<Budget, String>`: `applySchedules` and `scheduleFuture` must return a `count`, and `workflow`
needs its reference month. The read methods moved there too, as `read`, because `query_budget` is in
services, which can't call `urbudget-android`.

### The app

- Every `edit { }` caller moved at once, not one at a time. `repository.edit` compares records by
  identity (`budgetPatch`) and sends a patch step.
- A `CONFLICT:` on an edit reloads, checks the guard again and goes once more, rebuilt on what's
  stored. A second refusal shows the fresh budget. Import and restore do the same, and a preview or
  totals the store has moved past catch up and ask again.
- The light copy needed no new hint. `normalize_import_metadata` already writes the fields the app's
  readers check first (`unassigned`, `startingBalance`).
- A whole budget still crosses on create, import and restore. `save_budget` stays for import
  and restore (on iPhone too, since 2026-11-11); create uses `create_budget`.
- The compare in debug builds wasn't built. It would send the whole budget across on every edit, and
  comparing through `load_budget` repairs the drift it should find. The instrumented test kit does
  it, through `saved_budget`.
- The rollback switch is gone: phase 7 removed it, so going back means reverting the code. No table
  or file format changed.
- `save_budget`'s ordinary path now commits through the same code, so iPhone gets the faster save
  with no change of its own. A save no longer writes `recovery.json` first, so a process killed
  during a save leaves no recovery file, on either phone. That edit was never acknowledged.

### The order of the phases

The order was 0, 1, 2, 3, 4, 7, 6. `Runtime::save` treats a budget without `sourceExport` as a heavy
change, and `Store::save` then overwrites `sourceExport` and every transaction row with what it was
given. A light copy sent through `save_budget` would erase `sourceExport` and every `source`. So
nothing could be able to send the app's copy there before it went light, and that included the
rollback switch. Phase 7 removed the old path first, and phase 6 came after it.

Phase 5, the iPhone, was built on 2026-11-11, on a Mac, in one step: every change, undo and redo
through `edit_budget` and `patchOnly`, previews and reads through `query_budget`, and the light copy.
`BudgetPatch.swift` is the Swift twin of `patch.rs`; `JSON` is a value there, so it compares records
by value where Kotlin compares by identity, and a patched copy shares what the patch left alone.
A backup that replaces the open budget is the one thing still sent whole (`save_budget`), from its
file. The shared patch cases pass in Swift, and `EngineEditTests` checks the app's copy against the
store's after every kind of change. Nothing was timed: see
[FOLLOW-UPS.md](../FOLLOW-UPS.md#what-fast-edits-on-android-left).

### On the Fold

Measured on 2026-11-05 on the final preview build, with the real 2,742-transaction budget, taps
through `adb` and a Perfetto trace. Times are medians from the tap reaching the app. The old path's
times stop when the new state is published, so that's the column to compare with it.

| Action | Tries | Saved and published | First frame | Row on screen | Old path |
| --- | --- | --- | --- | --- | --- |
| Clear | 12 | 58 ms | 71 ms | 113 ms | 600 ms |
| Approve | 4 | 52 ms | 68 ms | 108 ms | 630 ms |
| Reconcile | 2 | 52 ms | 65 ms | 99 ms | 710 ms |
| Undo | 5 | 47 ms | 61 ms | 94 ms | 450 ms |

- **Edits are 10 to 14 times faster than the old path.** The 31 to 34 times on the emulator was
  against debug Kotlin, so the Fold's figure is the one to quote.
- **The 0.1 s aim is met for the save and narrowly missed for the row.** Undo and reconcile are just
  under it, clear and approve just over, and the display adds up to one more refresh. The row waits
  for `ui.registerIndex` and then `ui.registerLines`, which run after the publish and each wait for
  a frame. That's about 55 ms of a clear tap
  ([follow-up](../FOLLOW-UPS.md#what-fast-edits-on-android-left)).
- **`edit_budget` took a median of 54 ms on a clear tap.** The gate after phase 2 measured 27.8 ms,
  worst 36.9 ms, on a synthetic budget of the same size on phase 2's debug build. Why they differ
  wasn't looked into. The first edit after opening the app took 100 ms of work, in one try.
- **The real budget came through unchanged.** Exports before installing, after installing, after
  every tap and undo, and after a restart are equal field by field, and 2,656 rows keep their
  `source`. The last three are the same bytes. The first is 82 bytes longer, which is its 82 escaped
  slashes.
- **Sync ran with a real account.** It ran nine times, about 8 s after an edit, for 0.9 to 3.4 s each
  on a background thread. The three edits that overlapped a run took 40 to 51 ms, no slower than the
  rest, and both synced budgets showed Synced afterwards.
- No crash or ANR appeared in the Fold's logs.

### Unmet and unmeasured

- **The size aim isn't met.** The aim was that cost barely differs, taken as within 1.5x from 2,700
  to 10,000 transactions. A clear tap on an emulator, in two runs on 2026-11-04, took a median of
  28.1 and 30.9 ms at 2,700 and 100.9 and 137.9 ms at 10,000, which is 3.59x and 4.47x. `edit_budget`
  alone on the PC at phase 2 went from 18.3 to 64.3 ms, 3.51x. The old path took 972.3 ms at 2,700 in
  the last emulator run that had both. `calculate_at` replays the ledger on every edit, so a flat
  cost needs incremental totals, which aren't in this plan
  ([follow-up](../FOLLOW-UPS.md#what-fast-edits-on-android-left)). `EngineEditPerformanceCheck` keeps
  the 1.5x assertion, and fails it.
- **Two kinds of edit cost more.** A patch with a full `order` list updates every row, and a changed
  extra travels whole, so an assign carries all of `moneyMoves`.
- **10,000 transactions on the Fold isn't measured.** Only the emulator ran that size.

## What it's for

On a phone, every small change reprocesses the whole budget. On the Fold's 3.4 MB budget (2,742
transactions) that takes about 0.6 s per edit, nothing on screen changes until it ends, and the
cost grows with the budget's history. Edits queue one at a time, so three quick taps wait about
1.8 s for the last one, which may be the "few seconds" felt in use.

The aim, on the Fold with that budget, preview build:

- An edit is saved and on screen in under 0.1 s.
- Its cost depends on the size of the change, not the size of the budget.
- Nothing changes in what's stored, in sync, or in how a budget behaves.

## How it works today

This is how the phones worked on 2026-11-04. Neither does now: Android since 2026-11-05 and
iPhone since 2026-11-11. See [As built](#as-built) for both.

Three layers hold the budget, and each phone edit crosses them three times.

| Layer | What it is | Holds the budget? |
| --- | --- | --- |
| The app | Kotlin (`BudgetRepository`), Swift (`BudgetStore`) | Yes: a full copy, which it treats as the truth when it edits |
| Stateless engine | `urbudget_android::invoke_json`, used by Android and iPhone: about 35 methods, most given the whole budget as text | No: it forgets it after each call |
| Services and store | `Runtime`, `Store`: SQLite, undo, sync | Yes: `Cache` keeps `persisted` and `draft` in memory |

The store already holds everything an edit needs, and the phones don't use it. They ask the
stateless engine to change their own copy, ask it again for the totals, then hand the result to
the store, which works out what changed.

Desktop doesn't work this way. Its window changes its own copy at once, sends only a `Patch`
(`apply_patch`), saves about 400 ms later (`commit_budget`, changed rows only), and runs engine
commands on the store's draft (`change_draft`). So the core already has this path; the phones are
on an older one beside it.

### Where a phone edit's time goes

Marking a transaction cleared on the Fold, preview build, measured over six tries:

| Step | Time |
| --- | --- |
| Read the app's copy | 0.03 s |
| Apply the change (whole budget to the engine and back) | 0.20 s |
| Recalculate totals (whole budget to the engine again) | 0.14 s |
| Save (whole budget to the store) | 0.23 s |
| **Total** | **0.60 s** |

Approving took 0.63 s, reconciling an account 0.71 s, and undo 0.45 s. The debug build's work
before the save is 0.9 s against preview's 0.4 s, so sideloaded builds should always be preview.

### What the review found

A throwaway benchmark on a same-shaped budget (2.7 MB, 2,742 transactions, on the PC) splits the
engine's time. The PC is about three times faster than the Fold, so its numbers show shares, not
phone times.

1. **Almost all of it is format conversion.** Of 32.6 ms for one edit through the stateless
   engine, the edit and its checks take 2.2 ms. The rest is text to `Value`, a clone, `Value` to
   `Budget`, and back again.
2. **Totals are cheap; reaching them isn't.** `calculate_at` takes 1 ms on the benchmark's budget
   in memory. On the Fold the step costs 67 ms, and most of that looks like receiving the budget.
   The benchmark's budget has few categories, so the real calculation will cost more than 1 ms.
3. **A save reads the database twice for no reason.** `Runtime::save` calls `store.saved()` and
   then `load_light()`, each reading every row (12.6 ms each), though the store holds the same
   budget in memory. A save takes 45.5 ms today.
4. **The draft path is already about nine times faster.** Edit, patch and commit through
   `change_draft`, `apply_patch` and `commit` takes 8.4 ms, against 78 ms for today's apply and
   save together.
5. **Undo rewrites every row.** `undo_redo` uses `store.save` (`DELETE` then `INSERT` for all
   transactions), then `load()` sends the whole budget back.
6. **Android repeats itself.** It parses the budget four times and turns it to text four times
   per edit. `SqliteBudgetStorage.write` parses text it was just given, only to turn it back to
   text. Those conversions are about 0.18 s of the 0.6 s.
7. **`validate` runs at least seven times per edit** (about 1 ms each on the PC; harmless, but
   a sign of how the layers overlap).
8. **Reads have the same shape.** Every preview (`workflow`, `reconcile`, `smsBatchPreview`) sends
   the whole budget to the stateless engine.
9. **The phones carry `source`.** Each transaction imported from YNAB keeps the import's own copy,
   which the store calls "most of an imported budget's size". Desktop's window never receives it.
   The phones do, and their reports read it (`leftUnassigned` reads `source.category_name`), so it
   can't simply be dropped.
10. **`native_apply_patch` looks like the fix and isn't.** It takes a patch, then does a full
    `save` and `load`.

## The change

This section is the proposal. Its contract, its `apply` and its plan for `edit { }` aren't what was
built: see [As built](#as-built).

**The store's draft becomes the one working copy. Apps send what they want done, and get back what
changed.**

A new service method runs under the same command queue as `save_budget`:

```text
edit_budget { method, params, expectedToken, month, asOfMonth }
  -> { patch, summary, token, savedAt, undoAvailable, redoAvailable }
```

In `Runtime`, holding the store lock:

1. Refuse with `CONFLICT` unless `expectedToken` matches the store and its cache, as today.
2. Run the same core function the stateless engine runs for `method`, on the draft
   (`change_draft`). It gets a clone (0.5 ms), so a failure leaves the draft untouched.
3. Apply the patch and commit the changed rows. A failed commit invalidates the draft, as today.
4. Push the inverse patch onto undo, as today.
5. Calculate the totals for `month` on the draft.
6. Reply with the patch and totals. No budget crosses the boundary.

The app applies the patch to its own copy and publishes it.

Estimated on the Fold: 0.05 to 0.1 s, from 9.4 ms on the PC scaled by the Fold's measured ratio
(about 3 to 4 times), allowing for its disk and a heavier calculation. **That's an estimate;
phase 2 has to prove it.**

### What moves in the core

The methods that return a changed budget (about 10 of them, such as `workflow`,
`saveTransaction`, `reconcile`, `applySchedules`, `smsBatchApply`) live in `urbudget-android`'s
`dispatch`, which `urbudget-services` can't call. They move to a new `urbudget_core::commands`:

```rust
pub fn apply(budget: Budget, method: &str, params: &Value) -> Result<Budget, String>
```

The stateless engine and `edit_budget` both call it, so one command can't behave differently on
the two paths. The other methods stay where they are.

### What changes in the apps

Android has one place to change: all 32 `vm.runCommand` calls and `saveTransaction` reach
`BudgetRepository.command`.

- `command` calls `edit_budget` and applies the patch to a **new root object that shares every
  untouched record** with the last one, as desktop's immer does. Published snapshots stay read-only,
  and screens that compare by identity (`opened === state.budget`) keep working.
- The budget's text (`current`, `exported`) stops being kept and is made when an export asks.
- `BudgetEditGuard` stays: it checks the app's copy, which is now in memory, so nothing is parsed.
  A `whole` guard gains a `source === current` shortcut.
- `repository.edit { }` (about 27 calls: `upsert`, `remove`, `saveAccount` and so on) keeps today's
  path to start with, and moves one at a time to `edit_budget` with a plain patch. Both paths end in
  the same store under the same token, so they can live together.

iPhone's `BudgetStore.persist` and `changed(by:)` get the same change. Its `JSON` type is a value
type, so sharing is automatic, and it keeps its retry after a conflict.

Desktop doesn't change, apart from running the shared core's tests.

## The work, in order

This is the order as proposed. The order built was 0, 1, 2, 3, 4, 7, 6, and phase 5 isn't built: see
[As built](#as-built).

Each phase ships by itself, and the old path stays until the last.

| Phase | What | Size | Clear tap afterwards |
| --- | --- | --- | --- |
| 0 | Keep the benchmark as an ignored test, and `Trace` sections on Android | Small | 0.60 s, measured |
| 1 | Stop the double read in `Runtime::save`; drop Android's extra parse and text in `persist` and `write` | Small | About 0.45 s |
| 2 | `urbudget_core::commands`, `edit_budget` and tests, unused by the apps | Medium | Unchanged; measure it directly |
| 3 | Android: `command` uses it; shared-record patches; undo and redo return a patch | Medium | About 0.05 to 0.1 s |
| 4 | Android: the frequent `edit { }` callers; previews read the open budget (`query_budget`) | Medium | Same; previews faster |
| 5 | iPhone: the same as 3 and 4, on a Mac | Medium | Not measured yet |
| 6 | Replace `source` on the phones with a small hint, then send them the light budget | Small | Smaller memory and load |
| 7 | Remove the old edit path | Small | Same |

**Gate after phase 2:** if `edit_budget` on the Fold isn't under 0.15 s, stop and find out why
before touching the apps.

Showing a tap's result before it's saved isn't in this plan. I suggested it earlier as the way to
make taps feel instant, but at under 0.1 s it isn't needed, and it brings rollback code for every
kind of edit. Reconsider only if the result still feels slow.

## What could go wrong

These are the risks as proposed. The compare in debug builds wasn't built and the rollback switch is
gone: see [As built](#as-built).

| Risk | Handling |
| --- | --- |
| The app's copy and the engine's drift apart | A test applies every command both ways and compares. Debug builds compare after each edit. Any `CONFLICT` reloads the budget |
| Unknown fields lost. `preserve_extensions` protects assignments and schedules on the stateless path only | The existing extension tests run on `edit_budget` before the apps use it; a real export is compared before and after |
| Reports change. `change_draft` removes `source` from what it returns, and the phones read it | Until phase 6, `edit_budget` returns transactions with `source` |
| Sync writes while an edit runs | Same store lock and token check as `commit`. iPhone's retry stays |
| A failed edit leaves a dirty draft | Clone first, `invalidate_draft` on a failed commit, as `Runtime::save` does now |
| A recovery file on every edit (`apply_patch` writes one) | `edit_budget` commits at once, so it writes one only if the commit fails |
| Added times. `change_draft` stamps new transactions; the phones' path may not | Probably an improvement that matches desktop. Check it in phase 2 |
| Undo history changes shape | Same inverse patches and limit of 50. Undo through a sync still resets, as it does today |

Rollback: the old path stays behind one switch until phase 7. No database table or file format
changes, so nothing needs migrating and a build can be reverted freely.

## Tests

These are the tests as proposed. The size line isn't met: see [As built](#as-built).

- **Core:** `commands::apply` returns the same budget as the stateless engine for every method.
- **Services:** `edit_budget` matches `save_budget` on the same edit, including its stored rows,
  undo and redo, conflicts, a failed commit, and extension fields.
- **Patches:** shared fixtures applied in Rust, Kotlin, Swift and TypeScript give the same budget.
- **Android:** the existing suite, with every command through `edit_budget`, and `SmsImportUiTest`.
- **On the Fold:** the same taps as the baseline (clear, approve, reconcile, undo) on a real-sized
  budget. Export before and after, and compare.
- **Size:** the cost of one edit at 2,700 and at 10,000 transactions should barely differ.

## Decisions

- **Android first.** It's measured and can be tested here; iPhone needs a Mac.
- **Phase 1 first either way.** It's small, it's useful on its own, and phase 3 builds on it.
- **Whether to go past phase 1** is the real choice: phases 2 and 3 are the bulk of the benefit and
  of the work.
