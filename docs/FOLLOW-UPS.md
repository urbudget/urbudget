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

## What the assistant review left

Added 10 November 2026. The review's plan, and what was built from it on
`claude/assistant-review`, is [plans/assistant-review.md](plans/assistant-review.md); its
"As built" says what was seen working and what wasn't checked. These are left.

**Not yet run** (the whole branch):

- ChatGPT answered thirteen short messages on the desktop, and every new kind of card was
  made, applied and read in the app beside ones made by hand (the plan's "As built" has them).
  One was shaped wrongly, a transfer inside the budget, and was fixed. Left: a reply cut at its
  length, which needs a lower limit than the app sets, and the long-conversation banner on a
  conversation that has really reached 120,000 tokens. Only ChatGPT's default model was asked.
- Five short messages sent 154,060 tokens and only 13,696 came from cache; a conversation of
  one message sent 15,432 with 5,376 from cache. Nearly all of each
  request is the instructions and the tool list, which are the same every time. Not known: why
  so little is cached on the ChatGPT plan (the request may need a cache key), and how much of
  the tool list could go to tools described only when they're offered. Done looks like the
  second request of a conversation reading most of its start from cache.
- Nothing was tried against Anthropic. The requests are built to its published rules for kept
  thinking and caching and checked by tests of what they contain. Done looks like a
  twelve-round reply, a change of language and bank texts being set up, with
  `prefix_mismatch_behavior: "drop_block"` under the `thinking-binding-controls-2026-08-01`
  header leaving `input_transformations` empty, and `cache_read_input_tokens` above zero from
  the second round.
- iPhone was built and its unit tests run on a Mac on 11 November 2026 (Xcode 27.1, the iOS 27.0
  Simulator); it compiled as written. Both rounds of polish were then ported from Android, with
  their tests. Seen on the Simulator with a sample budget and saved conversations: the title row,
  folded steps, tables, the "⋯" under earlier replies, waiting and finished cards, the sign-in
  banner. The Simulator has no sign-in, so nothing was sent from it. Not seen: a reply arriving
  (live Markdown, the thinking line, the handover to the finished reply), an opened card holding
  its place, attachments, Arabic. Done looks like a signed-in pass on a phone: a message sent, a
  card applied and undone, Show in the budget both ways.
- Android was built with this branch's engine and seen on an emulator, with a budget and
  conversations copied in from the desktop's test window; `ProposalCardUiTest` passes there.
  It has no sign-in on the emulator, so nothing was sent from it. Not seen: a running reply, an
  error entry, the notice before a first message, attachments, the row editor, bank-text and
  import cards, and any of it on the Fold or a wide window. Done looks like a signed-in pass on
  the Fold: a message sent, a card applied and undone, Show in the budget both ways, and Back
  with the assistant over an import.
- The eval set in the plan isn't built. `llm::script` answers the loop in tests; the forty
  tasks, and running each provider's default model on them twice, are left.

**Left as it was, or not finished:**

- A draft doesn't outlive the app being closed, on any of the three.
- Android reads a reply's events with the app in the background only while the process lives,
  and iPhone only for the background time it's given. Neither says "Paused when you left". A
  foreground service on Android wasn't added.
- WebP and GIF files go to the provider as they are; only JPEG and PNG lose their location
  and camera details (`attachments::scrub`).
- All conversations share one lock (`mod.rs`). A look no longer writes, so it's held briefly;
  a lock per conversation would need care that a deleted one can't come back.
- "Continue in a new conversation" starts from the last eight things said and what became of
  the proposals, as they were written. The plan had the model write the summary.
- Desktop: discarding the last waiting card doesn't read the conversation again, so the
  buttons for what's waiting don't come back until the next reply. The line under the message
  field still names the service ("Using ChatGPT plan") after its sign-in has ended.
- iPhone: Show in the budget doesn't mark the categories on the Budget tab; the notice after
  an import reviewed in the assistant has no "Categorize with the assistant"; a long report
  table's pieces scroll sideways apart (one shared scroll needs iOS 18's `ScrollPosition`, and
  the app still runs on 17).
- Deleting a category or a schedule stays in the app, on purpose. Deleting transactions is
  only for ones selected and sent with the message.

**Arabic:**

- The words this work added (the core's in `i18n/arabic.rs`, and the three apps' catalogs)
  were written with the work and follow `docs/LOCALIZATION.md`, but no Arabic reader has gone
  over them. "Ask the assistant" isn't worded the same on the three apps, and the zero form
  of "Delete {n} transactions" differs between Android and iPhone.

**Desktop** (`desktop/src/bridge.ts`, `desktop/src-tauri/src/lib.rs`):

- An open save panel holds up the app's queue of saves and reads; the YNAB export names its
  folder with a file panel; outdated import previews aren't cancelled.

## What the foldable layout left

Added 9 November 2026. Android now follows a Fold's posture and hinge (`ui/Adaptive.kt`): sheets,
the assistant and a new transaction float at the window's end, a hinge running top to bottom splits
the app into a phone on one side and what's opened on the other, and half open on a table the keypad
and sheets keep to the flat half. With the assistant open the window is two screens, the app (as a
phone, with its own sheets) and the assistant, both in use; typing follows the side last touched
(`ProvideAppSide`, `SharedScreenFocus`). A sheet keeps the kind it opened as, so folding with one open
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

## iPhone Duo and wide windows

Added 10 November 2026, built on the 11th. Apple's foldable (7.6" inner screen, 951 × 669 points;
5.4" cover screen, 466 × 678) shipped in October 2026. The iPhone app follows it as Android follows
a Fold (`ios/UrBudget/UI/Fold.swift`, `WideLayout.swift`, after `ui/Adaptive.kt`):

- **Folded**, the cover screen is a phone.
- **Open flat**, the inner screen is a wide window: a report beside the reports' list, a
  transaction beside the register, a category beside the budget. Where a list's edge would fall
  near the crease it falls on it, and the assistant takes the far half with the app a phone on the
  near one.
- **Half open like a book**, the fold is never crossed. The start side is a phone of its own; a
  category, a transaction, a report or the assistant takes the far side, which says so while it's
  empty. Accounts and More keep to the start side.
- **Half open on a table**, the fold running across, a sheet stops below the fold and a
  full-screen form's keypad takes the whole flat half.
- An iPhone stays upright as before; a Duo turns once it's unfolded.

It reads iOS 27.1's own reports: `onHingeChange` for how far the hinge is open, and the window's
reserved regions of the kind `.division` for where the fold runs. Both were found in the SDK's
headers and interfaces, not in documentation.

What the iPhone Duo Simulator showed (iOS 27.1, Xcode 27.1, 11 November 2026; it's opened and
closed in Xcode's Device Hub, there being no Simulator app and nothing for it in `simctl`):

- Folded: the cover screen, 466 × 678, hinge `closed`, no region.
- Unfolded: a 951 × 669 window with one `.division` region, 40 points wide down its middle
  (x 455 to 495, the window's full height). Half open it's active and the hinge says
  `partiallyOpen`; flat it's still reported, not active, and the hinge says `fullyOpen`.
- The app's screens start in from the window's ends there (the camera's side, the corners), so a
  fold is placed against where each screen really lies (`WindowSpan`), not the window's edge.
- iOS stands its own tab bar upright at the window's far edge on the inner screen, clear of the
  fold, and puts a screen's bar buttons in a column above it. So the app's own dock for the tabs
  (`FoldDock`) is only for a pretended fold; on the Duo the system's tab bar is left alone. A
  system sheet (the file picker) kept to one side of the fold by itself.
- Seen flat: a category beside the budget, a transaction beside the register with the line
  between them on the crease, and the assistant on the far half with the app a phone (with its
  ordinary tab bar at its foot) on the near one.
- Seen half open like a book, in English and Arabic: the register, the budget and the reports'
  list each end at the fold, with a transaction, a category, a report or the assistant on the far
  side and its hint there while it's empty. In Arabic the sides swap and iOS's tab bar stays at
  the right. The app's sheets were kept to one side by iOS, as its own file picker was.
- A list on the start side kept 83 points clear before the fold: the room iOS holds for its tab
  bar at the far edge, applied inside every navigation stack wherever it lies. The start side's
  content gives that room up (`startPaneContent`); it has to be said inside the stack, on the
  content, as it does nothing from outside. The fold's own strip wears the far side's ground
  (`FoldGap`).
- Seen half open on a table (the Simulator turned a quarter, 669 × 951, the region y 455 to 495
  and active): a category's sheet and the Unassigned sheet stop just below the fold, a new
  transaction's keypad takes the flat half with its form above, and the assistant opens over the
  whole screen with what it asks at the foot.
- Seen with a pretended fold (`-fold book`, `-fold table`, debug builds) before the Simulator was
  opened: the book layout on an iPad Air 11-inch and the table layout on an iPhone 17.
  `FoldTests` holds the rules to the figures above.

- **Flat, beside iOS's side bar** (found by Ahmad on the 11th, after the polish passes): cards
  and bands ran under the bar the Duo stands at the window's right edge when it's open flat (its
  status items, a page's buttons and the tabs). Two causes. Margins set on a list
  (`contentMargins`, which centred a wide list's rows) take the place of the 84 points iOS keeps
  for the bar, so Accounts' cards slid under it; and a plain list's frame runs to the window's
  edge whatever iOS keeps, so the budget's bands and rows showed through the bar. Now a wide list
  is a column down the middle of the room iOS leaves, its frame narrowed and not its margins
  (`readableWidth`, which `pageGround` uses for the budget and the register), and a grouped list
  or a form has eight points more beside the bar than iOS's nine (`clearOfSideBar`). Seen flat
  on every tab, More's pages, a report, an account's register, the transaction form, the
  assistant beside the app and the Unassigned sheet, in English and Arabic (the bar stays at the
  right, the leading side there), and turned upright, where the tabs are at the foot. An iPhone
  and an iPad are as they were.
- **Half open, in Arabic** (seen once Ahmad half-opened it again, the same evening). English was
  right. In Arabic the bar stays at the window's right, which is the start side there, and three
  things were wrong. The budget's and the register's rows ran under it: a plain list's own view
  runs on under a bar whatever room iOS keeps, so a list beside one is now cut off where the
  bar's room begins (`EndsAtSideBar`). The far side left 84 points unused by the fold: inside a
  navigation stack every page is given the bar's room, beside the bar or not, so a page that
  doesn't reach the window's side now gives it back (`AwayFromSideBar`, with
  `WideLayout.phantomInsets` deciding; the start side in English had its own fix,
  `startPaneContent`, which stays for what isn't a list). And the assistant opened as a sheet,
  not on the far side: a view's start, counted from the window's right in Arabic, was worked out
  once before the window's width was known (`MeasuringSpan`; it's now worked out again when the
  width changes). Seen half open in English and Arabic on every tab, a report, an account's
  register, a selection, Account & sync, Unassigned and the assistant.
- Not seen since these changes: the table posture, and dark on the Duo. On an iPad the register
  beside a form is too narrow for its bar's large title, which iOS drops along with all but two
  of its buttons (the rest are under "…"); that's from the bar-title change, not this one.

Left, and what done looks like:

- **The phone itself.** Every posture has been seen on the Simulator only, where the hinge is
  set by hand in Device Hub and taps arrive seconds late, so nothing there was folded or turned
  with something open. Done looks like a Duo in the hand: folding and unfolding with a category,
  a transaction and the assistant open, in both half-open postures. The fold's region is read
  with its default mirroring, which a fold down the middle can't show to be right or wrong.
- The table posture in Arabic, and dark in any posture, weren't looked at on the Duo.
- On a table the detail of a split screen isn't shown beside its list (the window is a phone's
  width there), and a form beside the register in book posture carries on over the screen, with
  what was typed gone (the form is made again). The same happens when a wide window narrows.
- Alerts and confirmation dialogs on a half-open Duo weren't seen; they're iOS's to place.
- The assistant's first screen now starts at the top of its page (it stood on the foot, and
  flat on the Duo its "Set up assistant" button began below it). Not looked at again on the Duo.
- On an iPad whose tabs sit in the floating bar at the top, a split screen starts below that bar,
  since the bars of two panes would meet under it (`BelowTopTabs`). Reports is kept clear by iOS
  itself and the other two by 54 points of padding: seen right on iPadOS 27.0, and it leans on how
  iOS lays out the first navigation bar of a tab, which another version may do differently.
- With the assistant beside it on a wide window the app keeps 600 points or more, so a category
  or a transaction there opens as a sheet over the whole window, the assistant included.
- Registers opened from Budget, Accounts or More open a transaction over the screen on a wide
  window; on a fold they open it on the far side.
- CI builds with GitHub's current macOS image rather than a pinned Xcode. The fold's code is
  marked for iOS 27.1 and compiles away before it, but an image with an older SDK than 27.1
  wouldn't build it. Not checked.

## iOS beside Android

Added 11 November 2026. The Android app was run on a Mac for the first time (a Pixel 9 emulator on
API 37, the debug build) with the same budget as the iOS app beside it on an iPhone 17 Simulator:
the desktop's QA budget (`desktop/scripts/qa/make-qa-budget.mjs`, 2,481 transactions in riyals,
its dates moved a month on), imported into both as a backup. iOS was then set as Android is,
screen by screen: Budget, Transactions, an account's register, Accounts, the Reports overview,
More and its pages, the category and Unassigned sheets, the new-transaction form, Welcome and the
assistant's first screen. What changed is in the [iOS README](../ios/README.md#implemented)
under Look. Two faults turned up on the way and are fixed: every budget shared one key for what
the device remembers of it (`budget.sqlite3`, the file's name), so the getting-started steps and
the folded groups of one showed on all of them; and Arabic dates carried the era ("م").

Left, and what done looks like:

- **Compared in a second pass**, the same day: the payee, category, account and month pickers,
  Move money, the transaction form on an existing transaction, the account forms, reconcile,
  Spending by category, bank connections, Import bank file and Import from YNAB. What changed:
  a picker's search is at the top of its list and the payee picker opens on the list (it raised
  the keyboard over it); Move and Assign to a category are filled buttons on the page; the month
  picker's months are cells again (they had the page's own grey); reconcile is titled Reconcile
  with the account's name under it (the bar cut the name short); a schedule's name takes two lines.
- **Compared in a third pass**, the same day: the other eleven reports, a category that needs
  money with its assign keypad and its target page, the name-and-note page, card payment, loan
  details, selection in the register, the register's filters, Settings and Account & sync signed
  out. What changed: the spending calendar wears Android's blue scale (its deepest two steps were
  hard to tell apart) with the ink Android puts on each step; a report's headline tiles are laid
  out row by row (a grid sized them before a note wrapped, and squeezed the figure above a
  two-line note); a name in a report's list gives way to its figures and takes a second line
  before it's cut; the target page opens at the sheet's full height; a selection shows what it
  adds up to under its count and its actions stand in a dock with their names, Delete among them
  (they were bare symbols in a toolbar); an iPad keeps its tabs while selecting; Account & sync
  says before sign-in what the subscription is and that sync is encrypted, and its sign-in
  buttons are filled; card payment's Pay button has the page's ground behind it.
- **Compared in a fourth pass**, the same day: Settings top to bottom, the Budget library, the
  schedule form, a loan account's register, card payment, a report's three filter sheets, a
  bank file's import (its review and its "check the file" page, on `us-bofa-checking.csv`),
  Arabic on Android (set with `cmd locale set-app-locales`), and on the Z Fold 8 open flat the
  Unassigned sheet, a category and the new-transaction form. What changed: Settings has the
  three preferences iOS lacked (Show what targets still need, Open scheduled transactions, Open
  on), in Android's order, and they do what they say (`showNeeded`, `showUpcoming`,
  `startPage`); a form opens as a panel over a wide window with the page behind it, where it
  covered the whole of an unfolded Duo or an iPad (`FormCover`); a filter sheet's search is at
  its top; in the import a wrapped line's title starts where its first line does (a button
  centres its label's lines) and "Purchases in the file" has its name over its choice.
- **Sync v2 on iOS, against the local Worker** (the evening of the 11th). `web/scripts/sync-v2-local.mjs`
  on port 8788 with a synthetic subscribed account, as Android's `SyncV2RuntimeTest` has it, and
  three fresh iPhone 17 Simulators launched with `URBUDGET_API` set (a debug build's Rust reads
  it). Signing in, setting the passphrase and the first sync were driven from launch arguments
  (`App/DebugSyncQA.swift`: `-syncQa <email> <passphrase>`, `-syncQaBudget`; debug builds, and
  only for a server on this Mac). Seen: protocol 2; the subscription shown active ("Included
  with this account by UrBudget"); a budget made on one Simulator found and downloaded by a
  fresh one with nothing asked ("Synced"); a category added on the second showing on the first
  within a minute, the first left open. A sample budget didn't upload, which is as meant (it's
  made local-only). In a debug build the sign-in code a development server answers with is
  filled in. Not tried: a conflict, locking and unlocking, removing a cloud copy, deleting the
  account, and the same between iOS and Android (Android's Rust has to be built with
  `-PrustDebug=true` to take a local server).
- **The assistant, in saved conversations**: the three seeded threads the iOS assistant work
  left on the iPhone Air Simulator, copied into the Android emulator's budget
  (`files/native/v2/budgets/<id>/assistant/threads`, with `run-as`), and each opened on both.
  The same cards, steps, tables and reply buttons. Different, and left: a message of one's own
  is a solid accent bubble on iOS and a soft one on Android; a figures card is outlined with a
  small-capitals tag on iOS and filled on Android; a link in a table shows its host on iOS.
  **No reply has been watched arriving from a model on either**: that needs a ChatGPT sign-in
  or a provider key, which weren't entered.
- **Not compared, and can't be here**: the loan planner with a loan's terms entered (the QA
  budget's loan has none; entering them wasn't done on either side).
- Android's More sheet doesn't give its rows to `uiautomator`, so its pages are opened by where
  they are on the screen (Settings & data at 400, 2230 and Account & sync at 400, 1510 on a
  1080 × 2424 Pixel 9).
- Settings is ordered differently on the two (Android opens on the budget's name and formats as
  a page of their own, then appearance, language and preferences; iOS has appearance first and
  the formats in the page). Left: the same settings are there, and iOS's language belongs to the
  Settings app.
- On the Duo's start side and on an iPad a selection's count shows above its dock; the Duo's
  bar has room for it too, so it reads twice there.
- **Left as iOS's own on purpose**: More is a tab where Android's is a sheet over the tab; dark is
  iOS's black and greys where Android's is navy; section headings in a card list are iOS's grey
  ones where Android's are bold; the keypad's operator keys are grey where Android's are tinted;
  a date is picked with iOS's own picker, which writes it the system's way and not the budget's.
  A form's field shows its name only while it's empty, where Android keeps the name over what's
  typed. A report is changed from its title's menu, where Android has the kinds and reports as
  chips on the page. Forms open at full height, where Android's sheets are as tall as their content.
- Found by Ahmad on the 11th and fixed: the Unassigned banner and the notices above the
  budget's categories stood on a white band (a plain list's own row ground) where Android has
  them on the page; and scroll bars stood over figures (a wide list's at its column's edge, a pane's
  beside the Duo's side bar). The app now shows none, as Android's doesn't: said on the window,
  and again in `groupedList`, `pageGround`, `CompactForm`, every sheet (`sheetDetents`) and form
  cover, since a sheet doesn't take it from the window. A long register gives no sign of how far
  down it is; Android's gives none either.
- The top bar on an iPad has the tabs in it, so the bar's Assistant button doesn't fit beside
  them there; the assistant is in the ⋯ menu too. On an iPhone both show it.
- An account's or a category's own register leaves the Assistant button off its bar so the name
  fits; the name is also the first line of the card under it.
- The text step is taken from the system's size when the app comes forward and when the size
  changes. The budget's two figure columns are measured at the system's size, not the stepped
  one, so they keep a point or two of room they don't need. Not looked at with the larger
  accessibility sizes after this change.
- Seen on the iPhone 17 Simulator in English, Arabic and dark, and the first screens of the iPad
  Air and the Duo (book posture). Not on a phone, and not with the 2,481-transaction budget on
  the iPad or the Duo.
- Building Android on this Mac: Homebrew's `openjdk@21` and `android-commandlinetools`, then the
  SDK under `~/Library/Android/sdk` (platform 37.0, build tools 36.0.0, NDK 30.0.16248370, the
  emulator and the API 37 Google APIs arm64 image), `ANDROID_NDK_HOME` set to the NDK, and
  `./gradlew :app:assembleDebug -PandroidAbis=arm64-v8a`. On a network without working IPv4,
  Java needs `JAVA_TOOL_OPTIONS=-Djava.net.preferIPv6Addresses=true` or its downloads time out.

## A Galaxy Z Fold 8 beside the iPhone Duo

Added 11 November 2026. Ahmad's own Z Fold 8 (SM-F971B, Android 17) over USB, with the preview
build already on it and their real budget: screens were opened and captured, and nothing was
installed, changed or written down from it. Its cover screen is 475 × 751 dp and its inner one
932 × 704 dp, against the Duo's 466 × 678 and 951 × 669 points. Seen folded and open flat, in
dark, beside the Duo Simulator on the QA budget. The phone isn't held half open in practice, so
that posture wasn't compared; the Duo's own half-open layout is as the section above has it.

- **The tabs are each system's own.** Android has them along the foot when folded and in a rail
  at the left with the "+" when flat. iOS stands the Duo's tabs, a page's buttons and the status
  items in a column at the right on both of its screens, and gives no way to move them (the SDK
  has none). So folded, the Duo's page is about 382 points wide where the phone's is 475 dp, and
  a long account name there takes two lines.
- Changed on iOS to match: a wide list's column is 680 points (`WideLayout.readable`, Android's
  `ReadableWidth`; it was 760); the empty far side shows its symbol in a soft disc of the accent
  (`PaneHint`); a notice with two actions puts the second on a line of its own (`Notice`), which
  the Duo's cover screen had squeezed onto one; a long group name takes three lines before it's
  cut; and every row of the budget and the register says its own margins (`plainRow`), because
  a plain list on the Duo's inner screen gives none and the scheduled rows lay against the
  pane's edge.
- Left as they are: dark is navy on Android and iOS's black and greys; flat, Android rounds the
  top of the budget and the register beside a pane, which iOS's pinned bands would show rows
  through; a page's title on iOS is at the window's start while its column is centred, since
  the title is the bar's; and Reports' list is one card on Android and a card per kind on iOS.
- Not compared on the phone: sheets and forms open flat, Arabic, and light.

## What fast edits on Android left

Added 5 November 2026. Android's edits now run in the store
([what was built](plans/engine-edits.md#as-built)); these are what's left of that work and what it
found. Each says what was seen in a run and what comes from reading the code. Line numbers are as
of this date.

**iPhone, phase 5 of the plan** (`ios/UrBudget/Core/BudgetStore.swift`, `BudgetPatch.swift`):

- Built on 11 November 2026: iPhone's changes, undo and redo, previews, reports and exports run in
  the store (`edit_budget`, `query_budget`, `patchOnly`), on the light copy, and it keeps its retry
  after a conflict. `save_budget` is left for a backup that replaces the open budget, which comes
  whole from its file. `EngineEditTests` holds it to the store's own copy after every kind of change,
  and to an import's `source` and `sourceExport` surviving edits, and the Simulator tests pass.
- Not known: what an iPhone edit costs, before or after, which has never been measured, and how it
  behaves on a real budget's size or with sync running (the Simulator had a sample budget and no
  account). Done looks like Android's taps (clear, approve, reconcile, undo) timed on a phone with
  a real budget, and its export compared before and after.
- `BudgetRepository.open(id:)` no longer adopts `library_open`'s reply, which has no
  `sourceExport`: it loads the light copy afterwards, and nothing sends the app's copy back whole.
- A read that finds the store moved on (sync) reloads and asks again, and that reload counts as a
  change for `onChange`, so sync is nudged once more than it needs to be. It was so before, for a
  refresh after sync.
- YNAB's own interest on a loan was known by its `source`, which the light copy leaves in the
  store. `LoanMath.paymentsWithoutInterest` now knows it by its payee, "Loan interest", on a
  transaction that has an import source: a renamed payee would bring back the notice that the
  month's payments came without interest, though the engine still wouldn't charge it twice.

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
