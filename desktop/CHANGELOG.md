# UrBudget changelog

Notable changes to UrBudget, organized using [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [1.0.10] - 2026-10-27

### Added

- UrBudget speaks Arabic, on the desktop and the phones. On the desktop, Settings › General › Language chooses System (the default, following the computer), English or العربية, and switches in place; the phones offer the same choice through the system's per-app language. In Arabic the whole window runs right to left, while amounts, dates and names keep their own direction; digits stay 0–9, and a new budget gets Arabic starter categories. Your budget's names, memos, number and date formats and exports are never translated. On a computer already set to Arabic, UrBudget opens in Arabic after this update; choose English there to keep it in English.
- Updates install themselves. A new version downloads in the background, then installs once UrBudget has sat unused in the background for a quarter of an hour with nothing to lose: no unsaved change, dialog or form open, no import under way (on its page or not) and nothing unsent in the assistant. Your budget is saved first, and UrBudget reopens on the new version. A copy from Microsoft Store also installs a downloaded update when you close it, keeping your budget locked from the save until the window goes. Settings › General › Install updates by themselves turns this off; updates then wait for Restart to update.
- When Windows wants your permission for a Microsoft Store update (the Store's automatic app updates are off, or the connection is metered), the update waits for you, and Install Store update asks. UrBudget never asks by itself while nobody's there.
- An update that installed by itself but left the same version running isn't tried by itself again for three days; installing it by hand still works.
- The first time UrBudget opens after an update, it says which version is now running.
- On the desktop, entering transactions by keyboard takes fewer keys. In a new transaction, Tab goes on to the next field you're likely to need, by the payee's latest transactions. Choosing a payee fills in its default category, else its latest one (Unassigned for pay you leave to it); Tab goes past it when it's the default, or the only category the payee's latest 10 categorized transactions had, and stops there to check it otherwise. Tab stops at the memo only when more than half of the payee's latest 10 transactions you typed in had one (a bank's text on imported ones doesn't count), and goes to Inflow rather than Outflow for a payee money usually comes from. Then come the cleared mark, Save & add another and Save. Shift+Tab still steps back through every field.
- ← and → move to the field beside once the caret reaches the end of the text, in a new transaction or one being edited, and past the Account field in All accounts without changing it. The field you arrive in is selected, so typing replaces it and the next arrow goes on.
- Entering several transactions in a row: once one is saved with Save & add another (Shift+Enter), Enter saves and adds another too, keeping the date, until Esc closes the row. The button Enter presses is the blue one, and the row counts how many were added.
- A payee's last amount is suggested in its amount field until you type one: Tab or → takes it, and Enter fills it in for you to see before the next Enter saves.
- Type + first in Outflow to make it money in, or - first in Inflow to make it money out: the field switches as soon as the sign is typed. A sign put in front of a sum already there (-5+10) leaves it where it is, as it adds up there.
- Alt+C (⌃C on a Mac, where ⌥C types ç) clears or unclears the transaction being entered or edited, from any of its fields. With the cleared mark itself focused, Space or C does it, and the hint below says so.

### Fixed

- Enter on one of the editing row's buttons (Cancel, Save & add another, Split, More options) presses that button. It used to save the transaction and close the row, even on Cancel.
- A payee's last-used category (filled in for a new transaction, and shown on Payees & rules) is its latest transaction's with one category, even when a later one was split. Depending on the order transactions were stored in, it could be an older one or none.

## [1.0.9] - 2026-10-24

### Added

- Importing a bank file no longer stops at a line it can't read. Every line of the file is shown with what became of it: it reads, it needs attention (a date or an amount that doesn't read, and why), or it's left out (pending, declined, another account's, in another currency, a balance or a total, a correction). Lines that need attention stay out of the import until they're fixed; the rest go ahead. On the desktop, importing is a page of its own with every line in a table like the register's: click a line and click it again (or press Enter) to edit its cells, saved with Enter; select several (with Ctrl, Shift or their checkboxes) to include, leave out or put them back together; choose what a column holds; and each line's actions (Fix, Include, Leave out, Use as the header row, Undo) sit at its right end, opening when the line is pointed at. It opens by itself only when something needs it; otherwise the review says what was left out, a click away. On the phones, the lines that need attention, those left out and those changed are listed, to fix a line's date, payee, memo or amount, include it, or undo the change.
- Fix with the assistant: the assistant is told the lines that need attention and proposes fixes from what the file itself says (another of the line's cells, the statement's totals, what you tell it), each with its reason. Applied, each fix goes only to a line that hasn't changed since, and the lines it fixed are marked.
- The assistant's proposals can be changed before they're applied. Each part of one has a checkbox to leave it out, and a pencil to give it another category or amount; Apply makes what's left ("Apply 2 of 3"), still as one undo step, and the assistant is told what you changed. Every part is listed (a long one a few rows at a time), and edits show each value as it was and as it will be ("Uncategorized → Dining out"). Use the assistant's version puts it back as proposed. For now this is on the desktop; the phones list every part too.
- Money back from a shop you bought from in the last 90 days is offered as a refund of that purchase ("Refund of Amazon EU · 23.99 on 7 Oct"), and takes the purchase's category when it comes in without one. When several purchases match and their categories differ, it's only marked as a possible refund. This also applies to transactions downloaded through bank sync.
- A file with several accounts' transactions (RBC, comdirect, an OFX or CAMT.053 with several statements) lets you choose which account's lines to import, and the account you import into remembers it, with the workbook sheet it was read from. comdirect's card and depot sections are read by their own columns, and each account is known by its heading, the same from one month's file to the next.
- When a file's dates don't say whether the day or the month comes first, the import asks once (is 01/02/2026 1 February or 2 January?) before anything goes on to the review. An OFX whose every amount that reads is positive asks whether its transaction types tell money out, rather than guessing, when an amount it can't read could be the one that decides.
- A CAMT.053 batch whose payments don't add up to its entry can be imported as the one entry instead, and a batch imported one way is never imported again the other way.
- Choose which CSV column holds the date, payee and amount when a bank names them differently. Each account remembers its columns, date order and signs for its next file.
- Importing an account's statement offers to pair a row with the uncategorized entry it mirrors in another of your accounts, within five days. Both become one transfer, so money moved between your accounts isn't counted as spending or income. A pair is chosen for you when the bank's text names the other account, matches earlier transfers between them, or recurs the same way throughout the file (a weekly top-up); otherwise it's suggested, and Pair suggested transfers takes them all at once.
- Add them as new and Skip them decide every row still waiting for a choice.
- Statements in Japanese, Chinese, Russian, Ukrainian, Hebrew and Arabic: their column names are known, and a file that isn't UTF-8 is read in the encoding its bytes fit (Shift_JIS, GBK, Windows-1251, -1255, -1256 and the European ones), however the bank saved it.
- Import Excel and OpenDocument statements (.xlsx, .xls, .ods): the sheet that looks like a statement is read like a CSV, with its dates and amounts as the workbook holds them. A ZIP of statements, such as a bank's daily CAMT.053 files, imports in one go.
- Choosing a PDF statement says it needs the assistant, and Read it with the assistant opens it with the PDF attached, on the desktop and the phones, instead of failing to read it.
- Import CAMT.053 (ISO 20022 XML) and MT940 statements, the formats many European banks offer beside CSV. A batch the bank booked as one entry comes in as its payments, the payee is the other party, and the purpose text (SEPA's SVWZ+, Dutch banks' /REMI/) is the memo.
- Once a day, UrBudget counts itself as in use on urbudget.app: only whether this is the copy's first opening, or its first today, this week or this month, with its version, system, whether it came from the Microsoft Store and the week it was first opened. Nothing identifies you, the computer or your budgets. Settings → Privacy & data → Count this copy as in use turns it off.

### Changed

- An account's Import, Export and (for an account linked to its bank) Sync sit in the page header beside undo and redo, leaving the register's toolbar to the list itself. Its search bar stays one line: compact until it's first clicked, it widens as it opens and stays wide, its filters scrolling sideways rather than taking another line. In a narrow window, Scheduled, Filters and then Add transaction show only their icons rather than the toolbar wrapping.
- Exports are saved where you choose: a save panel opens in Downloads with a suggested name, rather than the file going into the budget's folder in the app's data. This applies to the full backup, the recovery draft, transaction and report CSVs, the YNAB export, the bank-file template and the assistant's spreadsheets and converted files.
- CSV imports read more banks' exports as they come: Revolut's (leaving out declined and reverted payments, and taking off fees), column names in German, Dutch, French, Spanish, Italian, Portuguese, Polish, Baltic and Nordic exports, account details above the header row, totals and balances under it, dates with times, dots or month names, debit columns written with a minus, amounts marked CR or DR, and a column saying which way an unsigned amount went.
- A CSV without a header row, or with column names UrBudget doesn't know, has its columns found by what's in them, including money out and money in in columns of their own (as TD and CIBC write them).
- A CSV with every account's rows, each saying whose (as RBC exports), imports the first account's. Where a bank names the other party in a second description (RBC's Description 2, Scotiabank's Sub-description), that's the payee, and the first description is the memo.
- Plain-text statements laid out in columns lined up with spaces (as BBVA México's) are read.
- Pending rows are left out until the bank books them, so they aren't imported twice; you can include one anyway. A file with several accounts' transactions (as comdirect exports) starts with the account you imported last time, else the first in the budget's currency; a file that mixes currencies (PayPal, Wise) imports the rows in the budget's, and shows the others for you to include with their amount in your currency.
- Files already imported are still recognised. A line 1.0.8 read another way (its payee from another column, a reference in the memo, a fee now taken off, spaces tidied) is also known by the ID 1.0.8 gave it, so the transactions it imported aren't offered again. A line fixed by hand is also known by the line as the file writes it, so importing the same broken file again finds it imported.
- A file the assistant converted from a PDF is read the standard way, whatever the account's own files are like, and doesn't change what the account remembers.
- A card's statement that's mostly money in ticks Purchases are positive for you.
- The payee is the other party's name where a statement has a column for it (a merchant, a payee, the side of a transfer that isn't you), rather than the bank's description.
- Statements saved in UTF-16, as Excel writes them, or in an older encoding keep their letters, accented or not.
- Alipay and WeChat Pay statements read which way each amount went from 收/支, and leave out moves between your own balances. Refused (Geweigerd), only authorised (Autorizzato), closed (交易关闭) and held (等待确认收货) rows are left out until they're booked, like other pending ones.
- In a file without a header row, a running balance beside a signed amount (CommBank's) isn't taken for the amount, and a total row under the transactions doesn't hide them.
- Fees in columns of their own (ConversionCharge, Commission, Сума комісій) are taken off the amount, and a dash in a cell counts as empty.
- Dates without a year (FNB's '26 Sep') are the latest such day that isn't more than a month ahead.
- An OFX correction that deletes an earlier line is applied rather than imported, and OFX names in CDATA and files with an empty currency read. A QIF file with an empty first record reads. CAMT.052 account reports read like statements, and a returned direct debit names its creditor.
- When a file's dates fit only one order, they're read that way, whichever order was chosen.
- Files saved with old Mac line endings, and files whose columns come in another order, read the same as any other.
- A statement imported into an account that already holds thousands of the same charge previews in seconds; each row lists the 50 nearest possible matches.
- A statement is read as the format its content says, whatever it was chosen as or however it's named (an MT940 file saved as .txt, an OFX file saved as .csv).
- OFX files with several accounts import the first account's; two transactions with one bank ID are both kept; zero-amount lines are left out; a payee written only in the memo becomes the payee; and a bank that writes every amount as positive has its debits read from the transaction type.
- Quicken's every-account QIF export imports the first account's transactions, skipping its category and class lists. Amounts written only as U, and a last record without its closing ^, are read too.
- Dates with the month as a word in German, Dutch, French, Spanish, Italian, Portuguese, Polish or a Nordic language ("15. Okt. 2026", "15-ene-2026"), with a time after them, or as a spreadsheet's day number (46310), are read.

## [1.0.8] - 2026-10-22

### Changed

- UrBudget is now free and open source, with nothing locked: there's no trial, license or read-only mode any more.
- Sync, the only paid part, is $3 a month.

## 1.0.7 — 2026-10-21

### Added

- Manage multiple saved YNAB access tokens and choose plans by name.
- Review differences before saving a YNAB import, with an option to ask the assistant for help.
- Compare monthly targets with your expected income.
- Check for and install updates in Microsoft Store copies of UrBudget.

### Changed

- Clearer target deadlines and funding progress.
- Easier transaction selection and simpler menus in the register.
- Smoother budget updates without the page shifting while totals refresh.

### Fixed

- More reliable YNAB imports for split transfers, loan interest, hidden categories, and different currency and number formats.
- Upcoming scheduled transactions in YNAB export files no longer count toward imported balances.
- More accurate Ready to Assign, target calculations, Quick Assign suggestions, and Age of Money.
- Uncategorized spending is included consistently in monthly budget totals.
- Clearing or replacing a transaction's category now saves correctly.
- Previously imported bank transactions can be matched when importing from another source.
- YNAB imports preserve your chosen token and budget name, and correctly apply your choices when reviewing changes.

## 1.0.6 — 2026-10-20

- Loan payoff previews discard results when their inputs change. Card and loan payment dialogs preserve the existing payment when retrying a failed save.
- Bank previews reject invalid dates and unsafe draft amounts before matching transactions. Failed or superseded account-deletion attempts preserve cloud copies until the deletion commits.
- Uncategorized-spending filters refresh after midnight, and split transfers mark every affected budget account correctly.
- Calculation errors keep month navigation, undo, reload and recovery export available. Reports and forecasts hide obsolete results after scope changes or failed refreshes and provide retry controls.
- Retained import metadata no longer merges report transaction counts or skips loan interest. Loan inputs are validated before ledger arithmetic, and scheduled imports reject unknown category references.
- Validate credit-card payment links across edits, imports and sync; preserve paired categories and prevent account-kind changes that would reinterpret existing transactions. Loan payments entered from either transfer side post interest once.
- Quick Assign includes scheduled card payments, weekly funding deadlines and intermediate future shortages. Expired targets stop asking for funding, while snoozed targets still show scheduled needs and overspending.
- Calculations refresh at month rollover, discard obsolete queued reads and offer a separate retry after a calculation failure. Quick Assign shows native resulting balances and rejects confirmations queued across a changed month.
- Selected-month calculations build fewer historical summaries, use credit-only reusable buffers and share schedule indexes. Reports sweep the ledger once, and derived reads release the database lock before calculating.
- Age of Money counts original cash outflow transactions consistently across split representations. Loan payoff searches reject invalid or impossible requests; imports, targets, schedules and forecasts reject malformed dates, unsafe totals and excessive expansion before returning a result.
- Credit-card transfers to tracking accounts reserve payment cash correctly, while card-to-card debt transfers leave payment envelopes for manual reassignment. Positive card balances no longer silently drain assigned payment reserves or let later transactions spend the same cash twice.
- Underfunded Quick Assign covers existing card debt without requiring a target. Same-day card funding respects recorded creation order, and future refill targets use the calculation's reference month consistently.
- Split-transfer expansion copies only each leg's own split, reducing allocation work for large split transactions. See the [Rust engine audit](../docs/RUST-ENGINE.md) for benchmark results and YNAB compatibility limits.
- Reports distinguish missing categories from intentional Unassigned income adjustments. Imported YNAB starting balances stay out of period income, and explicit Unassigned choices survive imports and later edits.
- Desktop and phone reports include net refunds, use split payees consistently, and count transactions once. Income stays visible when filtering expenses; uncategorized spending has its own review link. Budget surplus, cash flow and credit-card payment activity now explain what their totals include.
- YNAB imports identify credit-card payment categories by their protected payment group as well as the account name, so an ordinary category with the same name stays ordinary.
- Reordering accounts no longer changes credit-card payment reserves. Funding follows outstanding purchases, including refunds, while cash overspending keeps priority.
- Amount entry respects decimal-comma formats throughout desktop and phone forms. Retrying a money move after a save failure saves the existing move once, and typing "today" uses the current date.
- Target edits clear incompatible timing settings; weekly and annual meters show this month's funding progress. Underfunded Quick Assign uses actual due dates, including biweekly targets, and can retry failed previews.
- Account forecasts use the selected accounts' spending history, and opening report transactions in the register keeps the complete report scope. Payees support keyboard selection and render large lists efficiently.
- Bank imports distinguish split-transfer legs and source totals, recognize more ID-less transaction details, and let you deliberately import an ambiguous duplicate once. Copies and new scheduled occurrences do not inherit previous bank matches.
- Large budget calculations, bank-import previews and YNAB plan exports reuse indexes and history instead of repeating full scans.
- Cloud sync binds transfers to the signed-in account and passphrase generation, preventing stale uploads after a reset. Credential failures leave account and key changes safely locked. The companion API also protects concurrent login codes and storage limits, handles delayed refunds and deletion callbacks, and routes Google subscription management to Play.
- Preserve the selected repeat frequency when opening a transaction's More options form, and choose an open account when adding a schedule from a closed account's register.
- Protect saved credentials when Android cannot read its encryption key. Running app copies observe credential replacements and deletions, and migrating older credentials preserves newer values saved by another copy.
- Use Android's supported file locking so the phone can create and update its encrypted credential vault.
- Handle future transactions with long imported IDs without blocking other scheduled transactions from posting.
- Transactions on the same date are listed in the order they were added, the latest on top, in registers, activity and report lists, the payee page and exports. Bank-synced transactions use the bank's own time, and imported files keep their statement's order. Transactions you already have keep their current order.
- Flagged transactions keep the column divider beside their flag, and unflagged ones show a hollow flag to click.
- Spending with no category says "Needs a category" in the register, instead of looking like money taken from Unassigned on purpose. The Budget page says how many need one and opens them, accounts with any are marked in the sidebar, and `is:uncategorized` finds them. Uncategorized cash spending reduces Unassigned; uncategorized card purchases add debt without reserving payment cash. Spending can now be saved without a category, as in YNAB, and emptying the category field leaves it empty instead of putting the last choice back. As in YNAB, Unassigned is offered in the category picker for income ("To Unassigned"), not for spending.
- The budget table puts each category on one line. A target meter fills the space between the name and its amounts, what's still needed sits beside Assigned, and only overspending is solid red. Group rows read as headings, a checkbox selects several categories, and each row's menu sits at its end.
- Loans live on their accounts. A debt account's interest rate, payment, escrow or fees and payment category are set in its account settings, where terms from a YNAB import are offered to use. Its register shows what's owed, the payoff date and the interest still to pay, with Record payment and a Payoff planner that tries extra or one-off payments, charging the month's interest the way a recorded payment does, without changing anything. The loan's payment category shows the same in the budget inspector. The separate Loan planner page is gone: open "Loan details & payoff…" from the account's menu.
- The sidebar uses one text size, so account names no longer look larger than everything else. The selected item has an accent bar, and unapproved counts are softer.
- Reports are picked from Spending, Income and Wealth in the page header, with that group's reports as tabs, so charts use the full width.
- A new light theme in the style of macOS: a white page, a soft grey sidebar, finer lines and a clearer blue, with every text colour meeting contrast guidelines.

## 1.0.5 — 2026-10-19

- On a Mac, saved sign-ins, keys and tokens no longer ask for your Keychain password one by one. They're kept together in one encrypted file whose key is UrBudget's only Keychain item. On Windows the file is sealed with Windows' own encryption for your account instead of each secret appearing in Credential Manager. What you've already saved moves over the first time it's used.
- Cloud Sync merges independent edits from different devices and lets you resolve conflicting changes by record, while retaining the local-edit safeguards.
- Phone layouts retain touch selection, the transaction keypad, sheet dialogs and Android Back navigation alongside the desktop interface.
- Android Back dismisses the amount keypad before closing a transaction, preserving the draft and calculating the entered amount.
- Android packages no longer advertise TV support.
- Protect local edits when cloud downloads, scheduled posting and background database checks overlap with changes. Sync stops safely if the open budget changes during a transfer.
- Retrying a failed transaction, recurring transaction or card payment keeps the same entry instead of creating a duplicate. Retrying a failed budget switch reopens the file already created.
- Retrying a YNAB file import after the new budget fails to open reuses that budget instead of creating another copy.
- Fresh Start preserves pending and future transactions, uses settled balances instead of a potentially stale summary, keeps the original currency, and remembers imported bank entries and loan interest already charged. Bank accounts need linking again.
- Transfer edits keep each account's notes and flags on the correct side. Closed-account choices match the saved account, and keyboard clearing uses the receiving account's status.
- Invalid dates stay visible and block saving. Dialogs and popovers handle keyboard focus more consistently.
- Large payee reports render fewer rows while keeping every payee reachable. Large selected registers avoid repeated selection scans while scrolling, and filtered lists no longer flash an empty window.
- Scheduled spending handles tracking-account split transfers correctly and rejects invalid dates and oversized amounts before calculation.
- The register's columns are sized by what they hold. Flag and Cleared are compact icon columns that show names when dragged wider, dates and amounts fit their values, and payee, category and memo share the remaining width. The approval dot sits beside the date, and expanded split lines line up with the table again.
- Scheduled transactions are now one system with the rest, and the Scheduled page is gone. A transaction dated after today is a scheduled transaction: it waits in Scheduled (once, or repeating) and is entered on its date, so it no longer counts in account balances early. Transactions already saved with a later date move there on their own; imported and cleared ones stay as they are. Each register lists its schedules above its transactions under a heading that folds them away and shows what's due in the next 30 days, paused ones included; the Scheduled button lists them all and adds new ones. Enter and Skip open into labelled buttons when you point at a scheduled row; its calendar opens Edit, Pause/Resume and Delete. Save becomes Schedule for a transaction dated ahead, Repeat sits in the register's add row, and a schedule moved to today is entered at once. The hint for adding a flag is an outline.

- Keep recurring schedules intact when retrying a transaction moved to a future date. Editing imported transactions, posted occurrences and loan interest keeps them in the ledger. Adding another future transaction respects its scheduled status even when new transactions normally start cleared.
- Concurrent app copies serialize credential writes; unreadable encrypted data is preserved, and removing a credential also clears a legacy copy left behind by a failed migration.

## 1.0.4 — 2026-10-19

Bank sync with SimpleFIN
- Connect supported US and Canadian banks through SimpleFIN Bridge, link them to your budget accounts, and press Sync to review new transactions. A separate SimpleFIN subscription is required.
- Review, match or skip downloaded transactions before importing. Previously imported transactions are recognized, and payee rules apply to new entries.
- See each linked account's last sync, bank balance and connection details from the sidebar. Connection credentials stay in your computer's secure credential store.

Transactions and card payments
- Record a credit-card payment from the card's register or payment category, with the amount owed, money set aside and a suggested paying account shown together. This records a payment in the budget; it does not move money at your bank.
- Transfers show To/From account names and a shortcut to the matching transaction in the other account. Account registers include a starting-balance row, and scheduled rows show how often they repeat.
- Set, clear and name flags from a dedicated Flag column. Names are saved with the budget and work in search, filters and bulk actions.
- Create a category while entering a transaction. New payee names stay as typed instead of being replaced by a partial match.

Faster everyday work
- Quick Assign calculates its previews together, and saving edits copies less transaction data.
- Large YNAB imports send less unused source data to the interface while preserving it in the saved budget and exports.
- Fast saves no longer flash busy indicators. Settings tabs stay aligned, and budget columns fit when the assistant is open.

Imports, backups and the assistant
- After a file import or bank sync, Categorize with the assistant prepares a request with the new uncategorized transactions attached. Nothing is sent until you press Send.
- Backup retention now applies when automatic and manual backups are created. Recovery and import checks reject stale changes before replacing newer budget data.
- Report an assistant reply inside the app, with a chance to edit the included text before submitting. Settings includes a privacy-policy link, and assistant privacy information explains provider processing and retention more clearly.

## 1.0.3 — 2026-10-18

Payees learn your bank's names for them
- Rename a transaction from a bank file and UrBudget offers to use that name whenever the bank's text comes up again, then to update the earlier ones.
- The Payees page lists the bank names each payee goes by, with any bank text on its transactions that no rule covers yet.
- Rules ignore case and punctuation, and when several match, the longest wins. Imported rows get the payee's default category.
- The import preview shows each new name next to the bank's own text.

Bank files
- Amounts with currency signs or codes ($12.34, 12.34 USD, -12,34 €) and decimal commas (1.234,56) are read. An amount that could be read two ways, such as 1.234, is refused rather than guessed.
- Two-digit years (10/17/26) are read as 2026.
- More column names are recognized: Post Date, Posting Date, Trans. Date, Transaction Description and Amount (USD).
- Tick "Purchases are positive" for card files that list purchases as positive amounts.
- "Help with this import" opens the assistant with the file attached when a file doesn't read the way you expect. Nothing is sent until you press Send.

Also
- YNAB exports saved as tab-separated text import.
- Quick Assign's Underfunded now funds "maintain a balance" targets.
- Notifications are easier to read in dark mode.

[Unreleased]: https://github.com/urbudget/urbudget/compare/desktop-v1.0.10...HEAD
[1.0.10]: https://github.com/urbudget/urbudget/releases/tag/desktop-v1.0.10
[1.0.9]: https://github.com/urbudget/urbudget/releases/tag/desktop-v1.0.9
[1.0.8]: https://github.com/urbudget/urbudget/releases/tag/desktop-v1.0.8
