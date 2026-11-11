# Native iOS

Native SwiftUI client. It has the same structure as the native Android
app in [`android/`](../android/README.md), drawn with iOS's own controls: tab bar, navigation
stacks, grouped lists, sheets with detents, swipe actions, context menus, and Swift Charts. It
doesn't embed the React UI or the Tauri runtime.

Debug installs as `app.urbudget.native` alongside other builds. Release uses `app.urbudget`, the
bundle ID the Tauri iOS client was configured with. That client was never published.
Releases are tagged `ios-v1.2.3`; `.github/workflows/ios-release.yml` builds and uploads them to
App Store Connect ([RELEASING.md](../RELEASING.md)).

## How it's built

`UrBudget.xcodeproj` is an ordinary Xcode project. Before Xcode compiles any Swift, its **Build
Rust engine** step runs [`scripts/build-rust.sh`](scripts/build-rust.sh), which runs
`cargo build` on [`crates/urbudget-ios`](../crates/urbudget-ios) for the iPhone or the Simulator
and leaves `liburbudget_ios.a` for the linker. Swift calls its C functions (declared in
`UrBudget/Bridge/UrBudgetCore.h`) from `Core/NativeCore.swift`: each call sends a JSON request
and gets a JSON reply. `BudgetRepository` keeps a light copy of the open budget; a change runs in
the store (`edit_budget`) and comes back as a patch to that copy. The accounting, storage, sync and imports all happen in
that Rust library (the shared crates); Swift draws the screens. The
[mobile guide](../docs/MOBILE.md) shows how this fits with Android and desktop.

## Folders

| Folder | What's in it |
| --- | --- |
| `project.yml` | The project's definition, from which XcodeGen makes `UrBudget.xcodeproj`, with the app's version |
| `UrBudget/App` | The app's entry point and tabs (`UrBudgetApp.swift`), Home Screen quick action and Shortcuts |
| `UrBudget/Core` | `BudgetStore`, the state every screen reads; the engine bridge (`NativeCore.swift`); sync; money, dates and JSON helpers |
| `UrBudget/Screens` | One folder per area: Budget, Transactions, Accounts, Reports, Planning, Imports, Assistant, Sync, Library, More |
| `UrBudget/UI` | Shared controls: keypad, pickers, theme, components |
| `UrBudget/Bridge` | The C header for the Rust library, and the bridging header that shows it to Swift |
| `UrBudgetTests` | Unit tests, run on the Simulator |
| `scripts/build-rust.sh` | Builds the Rust library for the Simulator or a device |
| `UrBudget/*.xcstrings` | English and Arabic text: the app's (`Localizable`), the Home Screen and Files names (`InfoPlist`) and Siri's phrases (`AppShortcuts`), as [the languages guide](../docs/LOCALIZATION.md) describes |
| `scripts/check-translations.mjs` | `node ios/scripts/check-translations.mjs` checks every string has its Arabic, with the same placeholders and all plural forms |
| `ExportOptions.plist` | How the release workflow signs and uploads to App Store Connect |
| [`../crates/urbudget-ios`](../crates/urbudget-ios) | The Rust side of the C bridge |

## Build and run

You need Xcode 16 or later with the iOS platform installed, plus Rust with the iOS targets:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
open ios/UrBudget.xcodeproj
```

Choose an iPhone simulator and run. Before compiling Swift, the **Build Rust engine** phase runs
[`scripts/build-rust.sh`](scripts/build-rust.sh), which builds `crates/urbudget-ios` as a static
library for that simulator or device. The first build compiles the whole Rust workspace and takes a
few minutes. Later builds only recompile what changed.

From the command line:

```sh
xcodebuild -project ios/UrBudget.xcodeproj -scheme UrBudget \
  -destination 'platform=iOS Simulator,name=iPhone 17' test
```

The project is generated from [`project.yml`](project.yml) by
[XcodeGen](https://github.com/yonaskolb/XcodeGen). After adding or removing Swift files, run
`xcodegen` in this folder and commit the regenerated `UrBudget.xcodeproj`. XcodeGen writes
`UrBudget/Info.plist` again from `project.yml` each time, so a document type or permission is added
in `project.yml`, never in the plist alone (an MT940 type added that way was dropped by the next run,
until `project.yml` was given it on 11 November 2026).

## Shared code

```text
SwiftUI screens -> BudgetStore (@Observable) -> BudgetRepository (actor) --+-> urbudget-core
                                                                         C ABI | engine, SQLite store
                                                       crates/urbudget-ios ----+-> urbudget-services
                                                                                   accounts, sync, vault
```

- `crates/urbudget-ios` exposes four C functions (`urbudget_invoke`, `urbudget_initialize`,
  `urbudget_service`, `urbudget_free`). They carry versioned JSON, as Android's JNI adapter does.
  Engine requests are passed straight to `urbudget_android::invoke_json`, so an engine request gets
  the same reply on both apps. Panics never cross into Swift.
- The open budget lives in the store, as on Android. A change says what to do (`edit_budget`: a
  patch of the records that changed, engine commands, or both as one undo) and the reply is a patch
  the app applies to its own copy (`Core/BudgetPatch.swift`, the Swift twin of `patch.rs`), with
  the month's totals. Previews, reports and exports run on the store's copy (`query_budget`), at the
  revision that's shown. Undo and redo reply with patches too. The app's copy is the light one: an
  import's own copy of each transaction (`source`) and of the whole export (`sourceExport`) stay in
  the store, so that copy never goes back as a whole budget; only a backup read from a file does
  (`save_budget`). This was phase 5 of the [fast edits plan](../docs/plans/engine-edits.md#as-built).
- Budgets are stored by the shared SQLite `Store` and multi-budget registry in the app's
  Application Support folder. Every change is a revision-checked commit with undo and redo. A save
  writes the changed rows, and `recovery.json` only if its commit fails. If sync or another device
  committed first, the app reloads and makes the change again on that version, once. A second
  refusal, or an undo that was overtaken, shows the latest version and asks you to make the change
  again.
- Credentials use the shared sealed vault, with its key in the iOS Keychain (`security-framework`,
  as on macOS).
- Swift handles presentation, text formatting and the keypad's arithmetic. It doesn't calculate
  envelope funding, targets or report totals: those come from `calculate`, `workflow` and
  `reports` in the shared engine.

## Implemented

| Area | iOS coverage |
| --- | --- |
| Budget | Month switcher and month grid, Unassigned banner, groups with Assigned/Available totals, Available pills, target bars and words, views (underfunded, overfunded, money available, overspent, snoozed), category search, hidden categories, collapse/expand (remembered per budget), note markers, drag to reorder, move to another group, swipe and context actions, the getting-started checklist |
| Categories | Details sheet (with previous and next category arrows, ⌘↑ and ⌘↓), Assigned on the keypad with Needed, Cover and per-category Quick assign shortcuts, activity drill-down, targets (all four behaviours plus imported ones, cadences, snooze), name/group/note, hide, delete with history move, this month's moves, credit card payments, the loans a category pays (with Use this payment as the target), Ask the assistant |
| Money | Move money and cover overspending; Unassigned sheet with month figures, targets against expected income, all eight Quick assign options with previews (for the whole month or one group), month notes, expected income and cash age |
| Groups | New, rename or merge, move up/down or reorder all, hide all, Quick assign, delete (hidden categories included) |
| Transactions | Register by day, search, status/flag/date/amount/needs-category filters, sorting, approve, cleared, flag, categorize, duplicate, delete, Select and multi-select bulk actions (and Ask the assistant about them) (category, cleared, flag, payee, memo, duplicate, delete) with Undo, Approve all, go to a transfer's other account, CSV export of the whole register or just what's listed, pull to refresh, statement import from an account's register, scheduled transactions with Enter/Skip and editing |
| Editor | Full-screen editor with Outflow/Inflow, keypad sums, payee picker with transfers and usual categories, category and account pickers, date, repeat, memo, cleared on both transfer sides (new ones cleared when Settings says so), flags, approval, splits, creating a category from the picker, Save & add another, a keypad with 00 that also takes a hardware keyboard's digits, operators, Delete and Return, a payee picker that opens ready to type |
| Accounts | Net worth (opening the Net worth report), cash/credit/tracking/closed sections with totals, approval and needs-a-category badges, account registers with balances, add/edit (types, net worth), reconcile, card payments, loan details, reorder, close/reopen |
| Planning | Scheduled transactions (due, coming up, paused, finished; enter, skip, pause, edit), credit card payments, the loan planner (rates, payments, extra and lump-sum comparisons, payoff by a date, amortization chart and schedule), payees and import rules with previews of what they change, each payee's bank names, and an offer to remember a renamed bank payee as a rule, as the desktop makes; a loan payment's category becomes the loan's when it had none |
| Reports | Overview cards and all twelve Android reports (spending by category, payee, over time and calendar; income vs. expense, income breakdown, money in and out; net worth, balance sheet, balance over time, forecast, cash age), with range presets and custom months, account and category filters, drill-downs to transactions, and CSV export, drawn with Swift Charts. Drag across a chart to read each month; it opens on the last complete month, with the one under way drawn lighter and its line dashed. A legend's names hide and show their series. A slice, a payee's tile (in its group's colour) and a day on the month calendar are picked with one tap and opened with a second; the overview's small charts show a month's figures under a finger |
| Bring in data | Bank files (CSV, Excel, OFX, QFX, QIF, CAMT.053, MT940; the format their content says; a PDF goes to the assistant with the file attached) read as an import session: the lines that need attention (fix a line's date, payee, memo or amount), those left out (include them) and those changed (undo), the account and sheet in the file, signs and the date-order question, Fix with the assistant (its fixes apply to the import), then the review with transfer pairing with another account's uncategorized entry and refund labels; the account remembers how its file read (its CSV columns are chosen on the desktop), SimpleFIN bank connections with per-account review, YNAB import from its API (delta sync, conflicts) or export files |
| Account & sync | Email-code sign-in, the Sync subscription (its status, and subscribing, restoring and managing it through the App Store), end-to-end encrypted sync with desktop and Android (set up, unlock, reset, track, download, conflicts, remove), automatic sync after changes, on returning and every minute |
| Around the app | A New transaction quick action on the app icon, an Add a transaction shortcut for Siri, Spotlight and the Action button, and on iPad (iOS 18 and later) tabs that open as a sidebar. A window at least 720 points wide shows list and detail side by side (`UI/WideLayout.swift`): a report beside the reports' list, a transaction beside the register (which stays in use, and asks before unsaved changes are lost), a category beside the budget; from 940 points the assistant sits beside the app, both in use. Narrower, lists keep to a readable width and these open as sheets. On an iPhone Duo (`UI/Fold.swift`): folded it's a phone; open flat, the wide layout with its lines on the crease; half open like a book, the start side is a phone and what's opened takes the far side, with nothing across the fold; half open on a table, sheets and the keypad keep to the flat half. Built from iOS 27.1's hinge and reserved-region reports; seen on the iPhone Duo Simulator folded, flat and half open both ways, not yet on the phone itself ([follow-ups](../docs/FOLLOW-UPS.md#iphone-duo-and-wide-windows)) |
| Look | The screens are set as Android's are (`UI/Theme.swift`, `UI/Platform.swift`, `UI/Components.swift`), compared with it side by side on the same budget: a tab's title on the bar's own row beside its buttons (search a button there until it's wanted, the assistant one tap away); text a step under the size iOS is set to (`AppTypeScale`, set on the window so sheets follow), which is Android's scale; the budget's groups and the register's days as bands across the list, pinned and solid; rows as close together; the budget and the register on the grey page with white rows; an account's register under a card of its balances; cards and sections a little apart; sheets solid pages, where iOS 26 would make them glass; figures never broken over two lines; a wide list a column down the middle of the window, clear of the bar an unfolded iPhone Duo stands at its side. Surfaces, bars, menus and pickers stay iOS's own ([what's left](../docs/FOLLOW-UPS.md#ios-beside-android)) |
| Undo | Undo and redo in the ⋯ menu on every tab, Undo on each change's message, and shaking the phone |
| Assistant | Streamed conversations with history (rename, search, continue in a new one), attachments and what goes with a message, cards drawn from the core's actions (review, apply, undo, restore, questions, figures, deletes, Show in the budget), notes for the assistant, reports with sharing and printing, workbooks, ChatGPT sign-in or OpenAI/Anthropic keys. A reply being written never shows half-made Markdown; steps fold to the latest; earlier replies keep their buttons behind "⋯"; drafts and their files are kept per conversation. Built and unit-tested on a Mac on 11 November 2026 and seen on the Simulator with saved conversations; no reply has been watched arriving from a model there, since the Simulator has no sign-in |
| Languages | English and Arabic from String Catalogs, chosen with iOS's per-app language (Settings → UrBudget → Language, System by default), which the app's Settings links to; Arabic lays the app out right to left, keeping amounts and charts left to right ([languages guide](../docs/LOCALIZATION.md)) |
| Library & settings | Multiple budgets (create in the device's formats with starter categories, sample, open, move to Trash, restore, delete permanently and Empty Trash for ones only on this device, fresh start), opening backups, statements and YNAB exports shared from Files, Mail or AirDrop, backups (now, retention, restore, prune), unsaved-change recovery, JSON backups compatible with desktop and Android, number and date formats, flag names, CSV exports, theme |

Everything in the app works without an account or payment. Sync is the only paid part: the
auto-renewable App Store subscription `app.urbudget.sync.monthly`, bought in Account & sync with
StoreKit 2. A purchase carries the signed-in account's `appAccountToken`, and Sync turns on only
once the server has checked the transaction Apple signed
([web/README.md › App Store purchases](../web/README.md#app-store-purchases)). `AppStoreSubscription`
also passes on whatever `Transaction.updates` delivers (renewals, Ask to Buy approvals, refunds)
and finishes a transaction only after the server has answered. A subscription from urbudget.app or
Google Play works here too after signing in with the same email.

Running from Xcode sells Sync from `UrBudget.storekit`, a local StoreKit configuration, so the
subscribe, restore and manage screens work without App Store Connect. Those purchases are signed
by Xcode, not Apple, so the server refuses them; try the whole flow with a TestFlight build, which
buys in Sandbox.

Web pages the services open (account pages, SimpleFIN setup, ChatGPT sign-in) show in an in-app
Safari view over whatever is on screen, so the app stays in the foreground and a sign-in's local
callback listener keeps running.

Debug builds can open a screen at launch for device screenshots:
`xcrun devicectl device process launch --device <id> app.urbudget.native -- -tab accounts` or
`-open sync` (also `library`, `schedules`, `loans`, `payees`, `banks`, `settings`, `assistant`,
`bankImport`, `ynab`, `unassigned`, `category`, `transaction`, `welcome`, `register`, `networth`).
`-open assistant -conversation <id>` opens a saved conversation.
`-fold book` or `-fold table` pretends an iPhone Duo's fold on any simulator (down the window's
middle, or across it), `-fold flat` and `-fold closed` only where the hinge stands, and `-foldInfo`
writes over the app what iOS reports of the hinge and the fold.
`-open` also takes, on Budget: `move`, `month`, `newCategory`, `planning`, `edit` (the last
transaction's form), `payeePicker`, `categoryPicker`, `accountPicker`; on Accounts: `addAccount`,
`editAccount`, `reconcile`, `pay`, `loanDetails`; on Reports: `report:categories` and the other
reports' ids; on Transactions: `select`, `filters`; and on Budget, for a category that still needs
money: `needs`, `assign`, `target`, `categoryDetails`.
`-openFile statement.csv` opens a file in the app's Documents folder as if another app had shared
it (copy one there with `xcrun devicectl device copy to … --domain-type appDataContainer`).

## Verification

- The [Mac App Review automation plan](../docs/plans/2026-10-31-ios-app-review-mac-automation.md)
  covers controlling the installed TestFlight 1.0.0 (1) with a separate WebDriverAgent helper,
  recording a sample-budget walkthrough with the iPhone's built-in recorder, and preparing the
  review response. It includes Mac setup and control probes; the hardware walkthrough remains
  to be tested. Build 1 predates the StoreKit subscription controls described above.
- `cargo test -p urbudget-ios` checks the C transport against the Android one, plus storage,
  revision conflicts and undo through the shared services.
- `UrBudgetTests` (200 tests, run on the Simulator) covers the JSON layer, money formats, keypad
  sums, category status, and a full create/assign/undo/transaction/stale-edit/library pass through
  the real Rust engine, including new budgets' starter categories and formats and backups opened
  from other apps. The bank-name rules are checked against the desktop's own examples
  (bankNames.test.ts). `EngineEditTests` covers edits in the store: the shared patch cases
  (`shared/fixtures/patch-cases.json`), the app's copy against what the store holds after every kind
  of change, one undo for an edit with its commands, a refused edit saving nothing, an edit made
  again after sync, an import's `source` and `sourceExport` surviving edits on the light copy, and
  previews and reads through the published store. Trash is deleted and emptied through the real
  library; the chart models, the wide layout's sizes, the fold's rules and the assistant's presentation
  have their own.
  The engine's services open one library folder per process, so these share `TestLibrary`.
- Seen on the Simulator on 11 November 2026 (Xcode 27.1, iOS 27.0): on an iPhone 17, a sample
  budget made, assigned and reported on, the new charts and Trash; on an iPad Air 11-inch, a
  category beside the budget, a transaction beside the register (opened, saved, and the
  unsaved-changes question), a report beside the reports' list, and the assistant beside the app
  in landscape. With a pretended fold, the book layout on the iPad and the table layout on the
  iPhone 17. On the iPhone Duo Simulator (iOS 27.1, opened in Xcode's Device Hub): the cover
  screen, and the inner screen flat (a category beside the budget, a transaction beside the
  register on the crease, the assistant on the far half), half open like a book in English and
  Arabic (the register, budget and reports' list ending at the fold, with a transaction, a
  category, a report and the assistant on the far side), and half open on a table (sheets and the
  keypad below the fold). Not seen: the Duo folded or turned with something open, dark there,
  a budget with real figures in it only on the iPhone 17 (a 2,481-transaction budget in riyals, the
  same one in Android's emulator beside it, in English, Arabic and dark), not on the iPad or the Duo; sync, a physical device, or a wide window other than the iPad Air's two
  orientations.
- The [Native iOS workflow](../.github/workflows/ios-native.yml) runs both on GitHub's macOS runners,
  plus an unsigned Release build for devices.
