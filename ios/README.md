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
and gets a JSON reply, the same requests the Android app sends. The accounting, storage, sync
and imports all happen in that Rust library (the shared crates); Swift draws the screens. The
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
  -destination 'platform=iOS Simulator,name=iPhone 16' test
```

The project is generated from [`project.yml`](project.yml) by
[XcodeGen](https://github.com/yonaskolb/XcodeGen). After adding or removing Swift files, run
`xcodegen` in this folder and commit the regenerated `UrBudget.xcodeproj`.

## Shared code

```text
SwiftUI screens -> BudgetStore (@Observable) -> BudgetRepository (actor) --+-> urbudget-core
                                                                         C ABI | engine, SQLite store
                                                       crates/urbudget-ios ----+-> urbudget-services
                                                                                   accounts, sync, vault
```

- `crates/urbudget-ios` exposes four C functions (`urbudget_invoke`, `urbudget_initialize`,
  `urbudget_service`, `urbudget_free`). They carry the same versioned JSON requests as Android's JNI
  adapter. Engine requests are passed straight to `urbudget_android::invoke_json`, so both apps get
  identical replies. Panics never cross into Swift.
- Budgets are stored by the shared SQLite `Store` and multi-budget registry in the app's
  Application Support folder. Every change is a revision-checked commit with undo and redo. If sync
  or another device committed first, the app reloads and asks you to make the change again.
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
| Planning | Scheduled transactions (due, coming up, paused, finished; enter, skip, pause, edit), credit card payments, the loan planner (rates, payments, extra and lump-sum comparisons, payoff by a date, amortization chart and schedule), payees and import rules with previews of what they change, each payee's bank names, and an offer to remember a renamed bank payee as a rule, as the desktop makes |
| Reports | Overview cards and all twelve Android reports (spending by category, payee, over time and calendar; income vs. expense, income breakdown, money in and out; net worth, balance sheet, balance over time, forecast, cash age), with range presets and custom months, account and category filters, drill-downs to transactions, and CSV export, drawn with Swift Charts; drag across a chart to read each month |
| Bring in data | Bank files (CSV, Excel, OFX, QFX, QIF, CAMT.053, MT940; the format their content says; a PDF goes to the assistant with the file attached) read as an import session: the lines that need attention (fix a line's date, payee, memo or amount), those left out (include them) and those changed (undo), the account and sheet in the file, signs and the date-order question, Fix with the assistant (its fixes apply to the import), then the review with transfer pairing with another account's uncategorized entry and refund labels; the account remembers how its file read (its CSV columns are chosen on the desktop), SimpleFIN bank connections with per-account review, YNAB import from its API (delta sync, conflicts) or export files |
| Account & sync | Email-code sign-in, the Sync subscription (its status, and subscribing, restoring and managing it through the App Store), end-to-end encrypted sync with desktop and Android (set up, unlock, reset, track, download, conflicts, remove), automatic sync after changes, on returning and every minute |
| Around the app | A New transaction quick action on the app icon, an Add a transaction shortcut for Siri, Spotlight and the Action button, and on iPad (iOS 18 and later) tabs that open as a sidebar, with lists kept to a readable width |
| Undo | Undo and redo in the ⋯ menu on every tab, Undo on each change's message, and shaking the phone |
| Assistant | Streamed conversations with history, attachments and transaction context, proposals with review, apply and undo, reports with sharing and printing, workbooks, ChatGPT sign-in or OpenAI/Anthropic keys |
| Library & settings | Multiple budgets (create in the device's formats with starter categories, sample, open, delete, fresh start), opening backups, statements and YNAB exports shared from Files, Mail or AirDrop, backups (now, retention, restore, prune), unsaved-change recovery, JSON backups compatible with desktop and Android, number and date formats, flag names, CSV exports, theme |

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
`-openFile statement.csv` opens a file in the app's Documents folder as if another app had shared
it (copy one there with `xcrun devicectl device copy to … --domain-type appDataContainer`).

## Verification

- `cargo test -p urbudget-ios` checks the C transport against the Android one, plus storage,
  revision conflicts and undo through the shared services.
- `UrBudgetTests` covers the JSON layer, money formats, keypad sums, category status, and a full
  create/assign/undo/transaction/stale-edit/library pass through the real Rust engine, including
  new budgets' starter categories and formats and backups opened from other apps. The bank-name
  rules are checked against the desktop's own examples (bankNames.test.ts).
- The [Native iOS workflow](../.github/workflows/ios-native.yml) runs both on GitHub's macOS runners,
  plus an unsigned Release build for devices.
