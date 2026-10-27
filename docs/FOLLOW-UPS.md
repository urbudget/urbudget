# Follow-ups

Work that's agreed but not done yet. Each entry says where things stand and what done looks like,
so it can be picked up without the conversation it came from. Update an entry as it moves, and
remove it once it's done (the changelog and the guides then describe it).

## Phones: change an assistant proposal before applying it

Added 24 October 2026, on branch `import-editor` ([draft PR](https://github.com/urbudget/urbudget/pull/5)). Not started: the session for it
couldn't set up a worktree (not enough free disk space).

**Where it stands.** On the desktop, an assistant proposal can be changed on its card before it's
applied (commit `e913fcc`): each part has a checkbox to leave it out, rows that can change have a
pencil that opens a category picker and/or an amount under the row, and Apply makes what's left
("Apply 2 of 3"), still one undo step. The engine side is shared and done:
`crates/urbudget-services/src/assistant/proposals.rs` (`revise`, `Revision`, `RowEdit`),
`assistant_revise` in `mod.rs`, and the dispatcher method `assistant_revise` in `dispatch.rs`,
which the phones can already call. The phones list every row of a card (a page at a time) and show
each change as it was and as it will be, but have no checkboxes or editor yet.

**What to build**, on the proposal card of each phone (Android: `AssistantProposalCard` in
`android/app/src/main/java/app/urbudget/nativeapp/ui/AssistantScreen.kt`, with
`AssistantController.kt`; iOS: `ios/UrBudget/Screens/Assistant/AssistantProposalCard.swift`, with
`AssistantController.swift`):

- For a card whose status is `proposed` and whose `card.rows` have a `key`: a checkbox or toggle
  per keyed row, on when it's applied. A row with `skipped: true` is dimmed with its label struck
  through; a row with `edited: true` shows its amount in the accent colour.
- Rows whose `edit` has `category` or `amount` get an edit button that opens a sheet (iOS) or
  dialog (Android): the app's category picker (`edit.categoryId` is the current choice; absent is
  Ready to Assign) and/or an amount field prefilled with the row's signed `amount`. An amount typed
  without a sign keeps the row's sign.
- Each toggle or save calls `assistant_revise` with `{ threadId, proposalId, revision }` (and the
  budget/patch handling the phone already uses for `assistant_apply`), then shows the proposal it
  returns. `revision` is `{ skip: [keys], set: [{ key, category?: id or null, amount?: cents }] }`,
  built from the card's current `choices`: turning a row off adds its key to `skip`; turning it on
  removes it from `skip` and drops any `set` entry for it with `amount: 0`; an edit replaces that
  key's `set` entry.
- The apply button reads "Apply n of m" when some keyed rows are left out, and is disabled at none.
  With `choices` present, offer "Use the assistant's version" (an empty revision).

The desktop's `desktop/src/assistant/ProposalCard.tsx` and `ProposalCard.test.tsx` are the
reference for behaviour and wording.

**Done when** both phones can leave rows out, change a category or an amount, show "Apply n of m"
and apply; Android unit tests pass (`gradlew :app:testDebugUnitTest`); iOS has unit tests in
`ios/UrBudgetTests` for building revisions and builds on CI (iOS can't be built on Windows); and
the changelog entry for this feature drops "For now this is on the desktop".

## What the review before desktop 1.0.9 left

Added 24 October 2026. A review of everything since `desktop-v1.0.8` fixed what could put the
wrong transactions in a budget before 1.0.9 (see that release's changelog); these were found
too and left for later. Each is a code reading, not yet reproduced unless it says so.

**Phones' bank import** (`ios/UrBudget/Screens/Imports/BankFileImportView.swift`,
`android/app/src/main/java/app/urbudget/nativeapp/ui/BankImportDialog.kt` and `App.kt`):

- A fix from the assistant and a change made by hand at the same moment: the later reply wins
  and the other is lost without a word (iOS `fix()`/`applyFix`, Android `applyFix`). Keep the
  session each request was made on and take a reply only if it's still current.
- Android: Fix with the assistant while a change applies cancels that change (the dialog leaves
  composition). Disable it while busy, or run the change at App level.
- Both: the line sheet's Done does nothing while an earlier change applies. iOS: Import isn't
  disabled while busy (`importRows`).
- Android: rotating, or the process being killed, loses the import and its fixes (plain
  `remember`); "Importing…" can stay up if the same error comes twice; old error banners stay
  after a later read succeeds.

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
