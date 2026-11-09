# Follow-ups

Work that's agreed but not done yet. Each entry says where things stand and what done looks like,
so it can be picked up without the conversation it came from. Update an entry as it moves, and
remove it once it's done (the changelog and the guides then describe it).

## What the review before desktop 1.0.9 left

Added 24 October 2026. A review of everything since `desktop-v1.0.8` fixed what could put the
wrong transactions in a budget before 1.0.9 (see that release's changelog); these were found
too and left for later. Each is a code reading, not yet reproduced unless it says so.

**Phones' bank import** (`ios/UrBudget/Screens/Imports/BankFileImportView.swift`,
`android/app/src/main/java/app/urbudget/nativeapp/ui/BankImportDialog.kt` and `App.kt`):

- iOS: a fix from the assistant and a change made by hand at the same moment: the later reply
  wins and the other is lost without a word (`fix()`/`applyFix`). Keep the session each request
  was made on and take a reply only if it's still current. On Android the two can't meet since
  9 November 2026: Fix with the assistant waits for a change to apply, and the sheet is closed
  while the assistant is open.
- iOS: the line sheet's Done does nothing while an earlier change applies, and Import isn't
  disabled while busy (`importRows`).
- Android: the process being killed loses the import and its fixes. A rotation, the Fold opening
  or dark mode no longer remake the screen (the activity handles them in place), so the import and
  the review's choices stay; `BankImportHolder` still brings the import back after a change the
  activity doesn't handle (a new text or display size), but then the review's choices are made again.

**Usage counts on urbudget.app** (`web/src/usage.ts`, `crates/urbudget-services/src/usage.rs`):

- The rate limit's key is `sha256("usage:<day>:<IP>")` with no secret, so an address can be
  worked out from it while it's kept, against the privacy policy's "only daily totals". Use
  Cloudflare's rate limiting binding, or a per-day secret that's deleted after the day.
- The Worker is given the Store-publishing Entra app for Partner Center analytics: give it an
  app of its own with the least role the analytics API accepts.
- Smaller: two copies running at once both count; IPv6 addresses aren't grouped by /64; the
  switch lives only in the WebView's storage; a copy first opened offline can count its cohort a
  week late.

**Bank import engine** (`crates/urbudget-core/src/bank_*.rs`):

- An OFX whose amounts are all positive is read by its transaction types without asking, even
  when every line is money in (a card statement with only a payment).
- One date that only reads month-first turns a confirmed day-first file around.
- An assistant column fix makes the account forget its saved columns (`remembered`).
- When the account's remembered section of a multi-account file isn't in the next file, the
  first section is read without asking.
- Smaller: dates without a year near the turn of the year; a ZIP is decoded whole before its
  size is checked; a fee column may already be in some banks' amounts; transfers matched by
  words in an account's name ("Visa").

**Assistant** (`crates/urbudget-services/src/assistant/`, `desktop/src/assistant/`):

- A budget-moves revision can change a category the card doesn't show (one money only passes
  through), and leaving one side of a move out shows the other as left out too.
- A part of a proposal that no longer exists (a deleted transaction) fails the whole proposal,
  even when it's left out.
- Two edits to one transaction, or two fixes to one cell, share a row key.
- Memo fixes always fail when memos aren't shared with the assistant; an import's lines go to
  the provider with any message sent from the import page.

**Desktop** (`desktop/src/bridge.ts`, `desktop/src-tauri/src/lib.rs`):

- An open save panel holds up the app's queue of saves and reads; the YNAB export names its
  folder with a file panel; outdated import previews aren't cancelled.

## What the foldable layout left

Added 9 November 2026. Android now follows a Fold's posture and hinge (`ui/Adaptive.kt`): sheets,
the assistant and a new transaction float at the window's end, a hinge running top to bottom splits
the app into a phone on one side and what's opened on the other, and half open on a table the keypad
and sheets keep to the flat half. A sheet keeps the kind it opened as, so folding with one open
leaves it a floating card on the outer screen until it closes (and unfolding, a bottom sheet). Checked
on a Pixel Fold emulator (flat, book, table, folded, dark, Arabic) and flat on the Galaxy Fold; not
yet half open on the Galaxy Fold, or on a dual-screen phone.

- Confirmation dialogs (`AlertDialog`, 20 of them) still sit in the window's middle, so half open
  like a book they straddle the fold, and on a dual-screen phone the hinge's gap. Done looks like
  them on one side of a hinge that splits the screen.
- Lists still keep 168dp (`DockRoom`) at their end beside the rail, where only the add button's room
  is needed.
- A new text or display size still remakes the screen and closes whatever editor is open (the
  activity doesn't take `fontScale` or `density` in place). Compose 1.9 makes a dialog's window
  again when the density changes and the new one never shows, so any open `AlertDialog` or panel
  would vanish while still open, with Back then leaving the app. Seen on an API 37 emulator with
  `settings put system font_scale`. Done looks like editors kept across a text-size change, perhaps
  through saveable editor state rather than handling the change in place. A foldable whose two
  screens differ in density remakes the screen when it folds, for the same reason.
- Half open like a book, Reports' report, the register's form and the budget's category take the
  far side, but Accounts and More pages show only a hint there until something is opened.

## What fast edits on Android left

Added 5 November 2026. Android's edits now run in the store
([what was built](plans/engine-edits.md#as-built)); these are what's left of that work and what it
found. Each says what was seen in a run and what comes from reading the code. Line numbers are as
of this date.

**iPhone, phase 5 of the plan** (`ios/UrBudget/Core/BudgetStore.swift`):

- Not built. iPhone still changes its own whole copy through the engine and saves it with
  `save_budget`, and calls none of `edit_budget`, `query_budget`, `patchOnly` or `light`. It needs
  a Mac. Done looks like Android's phases 3, 4 and 6 on iPhone, keeping its retry after a conflict.
  The app must stop sending its copy to `save_budget` before that copy goes light, as on Android:
  a light copy sent there erases `sourceExport` and every `source`.
- Known: `save_budget`'s ordinary path changed in shared Rust, and `crates/urbudget-ios`'s tests
  pass unchanged. Not known: what an iPhone edit costs, which was never measured, and how the
  Simulator tests take the shared change, because they haven't run on it.
- From reading, not reproduced, and older than this work: `BudgetRepository.open(id:)` adopts
  `library_open`'s reply, which has no `sourceExport`, so the next save takes the heavy path and
  overwrites the stored one. `save_without_heavy_extras_still_takes_the_full_path` pins that path.

**Edit cost** (`crates/urbudget-core/src/engine.rs`, `crates/urbudget-android/tests/edit_cost.rs`):

- A flat edit cost needs incremental totals. A clear tap on an emulator took a median of 28.1 and
  30.9 ms at 2,700 transactions and 100.9 and 137.9 ms at 10,000, in two runs: 3.59x and 4.47x, where
  the aim was 1.5x. On the PC `edit_budget` went from 18.3 to 64.3 ms, 3.51x. `calculate_at` replays
  the ledger on every edit, 4.4 to 17 ms of that. For 1.5x the part that doesn't grow would have to
  be over four times the part that does, so removing waste can't reach it. Done looks like
  `EngineEditPerformanceCheck` passing its assertion, which it fails today.
- Assigning also grows with `moneyMoves`, because a changed extra travels whole: 17.3 ms with none
  and 53.4 ms with 5,000 on the PC, with an 872 KB reply.
- The Transactions list shows a change about 55 ms after the save. It rebuilds in two steps after the
  publish, `ui.registerIndex` then `ui.registerLines`, and each waits for a frame. That puts a clear
  tap on the Fold at 113 ms and an approve at 108 ms, just over the 0.1 s aim. Done looks like the
  row on screen under 0.1 s on the Fold.
- Not known: how the rest of the growth divides up, and what 10,000 transactions cost on the Fold.

**SQLite** (`crates/Cargo.lock`):

- The bundled SQLite 3.51.1 can deadlock. It was seen once, on a debug build, in `SyncV2RuntimeTest`
  on a library of 462 budgets, and that run never finished. A thread closing a budget's last WAL
  connection and another opening it block each other forever: `unixLock` takes `pLockMutex` then
  `unixBigLock`, while `findReusableFd` holds `unixBigLock` and takes `pLockMutex`. The release notes
  for 3.51.2 name it: "an obscure deadlock in the new broken-posix-lock detection logic". 3.51.3 also
  fixes the WAL-reset corruption bug, which sqlite.org calls rare and not an emergency.
- The lock file has `rusqlite` 0.38.0 and `libsqlite3-sys` 0.36.0, which bundles 3.51.1. Sync's
  `capture` (`cloud/notifications.rs`) reads every local budget but the open one through
  `Store::existing_status`, Trash included, so with only the open budget in the library it closes
  nothing. This work added no such call.
- Not known: how often it happens, which `rusqlite` release bundles 3.51.2 or later, and whether it
  reaches a release build, where it hasn't been seen. Done looks like a `rusqlite` with 3.51.3 or
  later and a sync run on a big library. The upgrade reaches iPhone and desktop too.

**Automatic sync** (`crates/urbudget-services/src/cloud/`,
`android/app/src/main/java/app/urbudget/nativeapp/SyncWorker.kt`):

- It slows as the library grows, Trash included. `SyncV2RuntimeTest` on a debug build passed in 7 s
  on cleared data and in 45 s with 200 budgets in the Trash, with retries.
  `NativeServices.syncAutomatically` returns null while another run is going, so `SyncWorker`
  retries on its backoff of 30, 60 and 120 s. One run on a library left by other tests, of about
  150 to 250 budgets by estimate, failed at `SyncV2RuntimeTest.kt:93` with `account-sync` still
  enqueued, though sync itself worked and the uploads landed.
- Not known: where the time goes, what it costs on a release build or a phone, and whether the
  failure repeats. Nothing under `cloud/` changed in this work. Done looks like automatic sync whose
  time doesn't depend on budgets in the Trash.
