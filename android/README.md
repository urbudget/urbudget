# Native Android

Native Kotlin / Jetpack Compose client. Open this
folder in Android Studio. The app is free and everything works without an
account; the only purchase is the optional sync subscription, $3 a month,
through Google Play. The application uses native screens, Android document
pickers and the print framework; it does not embed the React UI or Tauri runtime.

Debug installs as `app.urbudget.nativeapp` alongside the published app. Release
uses `app.urbudget`, so a correctly signed upgrade retains the existing app's
private library and Android Keystore. Do not install debug test builds over real
data or test against a connected personal phone.

## How it's built

This folder is an ordinary Gradle project. Before Gradle compiles any Kotlin, the
`buildRustEngine` task in [`app/build.gradle.kts`](app/build.gradle.kts) runs `cargo build`
on [`crates/urbudget-android`](../crates/urbudget-android) once for each phone architecture
(arm64-v8a, x86_64) with the Android NDK's compiler, and packs the resulting
`liburbudget_android.so` into the app. Kotlin calls it through JNI in
`NativeBridge.kt`: each call sends a JSON request and gets a JSON reply. Edits, previews and
totals send no budget: they run on the store's copy through `edit_budget` and `query_budget`
([below](#storage-privacy-and-concurrency)). The iOS app doesn't use those yet, so the two apps
no longer send the same requests. The accounting, storage, sync and imports all happen in that
Rust library (the shared crates); Kotlin draws the screens. The
[mobile guide](../docs/MOBILE.md) shows how this fits with iOS and desktop.

## Folders

Kotlin files live in folders named after their package, so the code is six levels down
(`app/src/main/java/app/urbudget/nativeapp`). The folders above it hold nothing else;
Android Studio shows them as one.

| Folder | What's in it |
| --- | --- |
| `app/build.gradle.kts` | The app's version, dependencies, signing and the Rust build task |
| `app/src/main/java/app/urbudget/nativeapp` | `MainActivity.kt`, the engine bridge (`NativeBridge.kt`), `BudgetRepository`, `BudgetPatch` and `BudgetViewModel`, money, register and report helpers |
| `app/src/main/java/app/urbudget/nativeapp/ui` | Every screen and shared control; `App.kt` is the root, `Theme.kt` and `Design.kt` the look |
| `app/src/main/res`, `AndroidManifest.xml` | Icon, styles, file sharing and backup rules |
| `app/src/test` | Unit tests that run on the computer |
| `app/src/androidTest` | Tests that run on an emulator ([how](#instrumented-checks)); `cases/` holds their scripts |
| `app/src/preview` | The optimized, debug-signed `preview` build's manifest |
| [`../shared/android`](../shared/android) | Android Keystore keeper and Play billing client, with their tests |
| [`../crates/urbudget-android`](../crates/urbudget-android) | The Rust side of the JNI bridge |

## Shared code

```text
Desktop React -> thin Tauri commands --+-> urbudget-core
                                      |   schema / integer math / workflows / reports
Android Compose -> JNI adapter -------+   SQLite / patches / merge / import conversion
                                      |
                                      +-> urbudget-services
                                          accounts / Play subscription / encrypted sync
                                          SimpleFIN / YNAB / assistant / vault

Native Android host ---> shared/android: PlayBillingClient + SecretStore
```

These are physical shared sources, not forked copies. Desktop keeps its public
Rust module paths through re-exports and thin command adapters. Tests run against
both hosts, shared synthetic fixtures and the Android JNI boundary.

- `crates/urbudget-core`: schema, validation, ledger, envelopes, targets, Quick
  assign, schedules, loans, reports/forecast, reconciliation, bank/YNAB import,
  payee/category workflows, SQLite, multi-budget registry, backups and merges.
- `crates/urbudget-services`: actual desktop account authentication, Play
  subscription verification, encrypted sync and conflicts, bank networking,
  assistant providers, PKCE sign-in, attachments, tools, proposals, workbooks,
  history and sealed vault.
- `crates/urbudget-android`: bounded versioned JSON/JNI transport. Panics do not
  unwind through JNI. The persistent Tokio runtime keeps assistant/OAuth jobs
  alive between calls.
- `shared/android`: one Google Play client and AES-GCM/Android Keystore keeper, with
  their unit tests. The Tauri Android project that came before this app used them too.
- `shared/fixtures`: portable budgets and bank files used for cross-platform
  accounting and lossless metadata tests.

Kotlin owns presentation, navigation, exact amount text conversion, lifecycle,
SAF, intents and printing. It does not calculate envelope funding, interest,
import matches or sync merges.

## Implemented native workflows

| Area | Native coverage |
| --- | --- |
| Budget | Month navigation, assignments and money moves, Quick assign previews, targets and snoozing, hidden/filter views, category/group ordering, merge/delete history handling, notes and expected income |
| Register | Ordinary and split transactions, both sides of transfers, named flags/approval/status, search/date/amount/account/category filters, bulk edits, duplication, deletion and CSV export |
| Accounts | Cash, credit and tracking types, opening balances, archiving, reconciliation preview/adjustments, card payment reserves |
| Planning | Full split/transfer schedule templates and all repeat intervals, enter/skip/pause, automatic due schedules, loan terms/rate history/payment/amortization/extra-payment comparison |
| Reports | Twelve report modes, ranges and account/category filters, category/payee/day transaction drilldowns, shared forecasts, CSV export |
| Library | Multiple budgets, fresh start, undo/redo, JSON import/export, manual and automatic backups, retention and restore, formatting and appearance |
| Account | Email-code sign-in, sign-out, account overview/deletion; only sync needs an account |
| Sync | Existing end-to-end encryption and protocol, passphrase setup/unlock/reset, track/download/remove, per-field and whole-budget conflict review; automatic sync on foreground, after edits and every minute while open |
| Billing | The monthly sync subscription (`urbudget_sync`, base plan `monthly`) at Play's localized price, pending/cancelled states, account-bound restore, server verification, subscription management |
| Banks | SimpleFIN setup claim, secure tokens, account links, import range/sign handling, request quotas, download/match/duplicate review, import and disconnect; bank files (CSV, Excel, OFX, QFX, QIF, CAMT.053, MT940; the format their content says; a PDF goes to the assistant with the file attached) read as an import session: the lines that need attention (fix a line's date, payee, memo or amount), those left out (include them) and those changed (undo), the account and sheet in the file, signs and the date-order question, Fix with the assistant (its fixes apply to the import), then the review with transfer pairing and refund labels; the account remembers how its file read (its CSV columns are chosen on the desktop) |
| YNAB | Multiple saved tokens, plans, delta sync with full-download fallback, conflict/discrepancy review, ZIP and Register/Plan CSV imports |
| Languages | English and Arabic from string resources, chosen in Settings & data or Android's own app-language settings (System by default); Arabic lays the app out right to left, keeping amounts and charts left to right ([languages guide](../docs/LOCALIZATION.md)) |
| Assistant | ChatGPT browser sign-in, OpenAI/Anthropic keys, model/settings, streamed conversations, cancel/history/export, files/photos, context selection, shared tools, review/recheck/apply/undo/refine/discard proposals, bank-file conversion review, workbook generation, report replies/printing/sharing |

Desktop window controls, updater delivery and OS-specific shell integration stay
with their platform hosts. Android uses Google Play delivery, native pickers,
intents and Print / Save as PDF.

## Bank SMS import

**More → Bank messages** (also **Settings & data → Imports → Bank messages**) turns the
texts a bank sends to this phone into transactions. It is opt-in: after the disclosure
and Android's permission prompts, choose the senders your bank texts from (up to 50).
The picker lists names and short codes first, by how many messages each has sent (SMS
and MMS together), already chosen ones first; a numeric short code shows its latest
message to recognise it, while named senders and people's phone numbers (listed apart)
are never previewed. No contacts access is needed. The first choice of
senders also picks where collecting starts: new messages only (from that moment), a day
(from its start, so "Since 3 Oct" holds all of 3 Oct), or everything on the phone. A
budget with no accounts starts two months back, and the picker says the texts will set its
accounts up (below); one whose accounts have no transactions starts with new messages only,
since its balances were entered as they are now and already include earlier texts; one with
history starts 30 days back, but not before its first transaction. **Collect messages from**
changes it at any time for every sender: an earlier start brings older texts in on the
next scan, a later one takes earlier texts out of what's waiting and out of the open
review. Changing senders keeps the review's items from the senders that stay.

Its row says where collecting has got to ("newest handled 2 Nov 2026, 07:47": the newest
text the budget finished with, kept on this phone; older texts can still wait when the start
moved earlier), and the dialog lists that and,
from the budget itself so it holds on another phone or after a reinstall, each account's
last day of bank data (transactions from texts or files) and of reconciling. Each has a
one-tap start: right after the last text handled (nothing read twice, nothing missed), or
from that day. The dialog also says what a choice does, counted without reading any text:
"Brings in about 207 texts · 2 requests · about 5 min", or "Takes out 5 waiting texts".
Minutes come from this budget's last request of 100 texts or more. A start that brings in
more than one request's worth is asked first, since reading begins as soon as it's saved;
the first choice of senders, the sending notice and a long wait to read show the same size.

Texts come from both of Android's stores. Most arrive as SMS, but a phone moved from an
iPhone keeps that history as MMS (on the test Fold, 2,858 of a bank's 2,913 texts), so
the scan also reads text-only MMS from each chosen sender's one-to-one conversation;
MMS with pictures or files, and RCS chats (which Android keeps private to the messaging
app), aren't read. **On this phone** shows, per sender, how many texts the phone holds
since collecting starts (SMS and MMS), the first and latest dates, and whether all were
collected, so coverage can be checked against an export.

The system SMS receiver captures selected senders while the app is closed, and opening
the page catches up from the system stores after missed broadcasts or restarts, saving a
scan in one transaction. Messages stay in a separate local SQLite inbox scoped to the
budget's database path; a review, which can hold a phone's whole history, is a private
file beside it. Capture never interprets text, changes a budget or calls the network.
Removing a sender removes its waiting messages; **Stop and clear messages** removes the
budget's inbox, review, remembered cards, senders, sending permission and background
reading. Neither changes Android's messages or transactions already added.

The page is built around what needs the user. The first time, a notice names the
assistant and what is sent (the texts, account and category names, payee category
history) and asks once per provider and model; permission given before reading became
automatic is asked again. With it, opening the page reads what's waiting, oldest first,
into one review; each request carries 200 texts, and the review is saved after every
request, so **Stop**, or a request that fails, keeps what's been read and the rest wait.
Readings continue when the page closes. Texts that arrive while a review is open are read
into it. **Read new texts as they arrive** (off by default) does the same in the
background (`SmsWorker`) for the open budget soon after a text arrives: texts that are
only codes and alerts leave without a word, and anything about money waits in the
review, counted in a notification that opens the page (`POST_NOTIFICATIONS` is asked
when it's turned on). The waiting texts themselves are folded below; **Choose** reads or
dismisses a selection. Provider retention settings apply.

The analysis session (`assistant/sms.rs`) has only `payee_history` and
`submit_sms_analysis`: no writes, browsing, attachments or chat history. It times out
after five minutes, can be cancelled and expires from memory after ten. The model gets
the texts numbered, as `[number, sender, received, text]`, never the phone's ids, and
writes out only the money: an item per transaction, hold or unclear text, with a short
note in the app's language, a quote copied from its text, and the transaction's date,
signed amount in the budget's currency (or none for a foreign amount the text doesn't
convert), written currency and amount, cleaned payee, kind, the card or account
reference copied from the text, and an optional category. Every other text is just its
number under a reason (`code`, `security`, `balance`, `declined`, `rewards`,
`marketing`, `other`). On real bank texts that's about a third of writing every text
out. The service maps numbers back to ids, places each quote as evidence, and rejects a
reply that misses or repeats a text; the core (`sms_import.rs`) validates the rest:
exact money, dates, references found in the texts, quotes, and every text accounted for
once. Any account id the model supplies is discarded; it may only name a transfer's other
account from the account list (below), which the phone resolves. `payee_history` follows a payee rule
to the renamed payee's history, so "S136 M" finds the user's categories for "Tamimi", and
finds the user's payee a bank's name holds ("MASHAR E" is their "Mashar").

The model also gets the user's payees, the 300 used most recently, each with the category
they usually give it, and the bank's names they renamed for these senders, so it writes the
user's spelling and category; for a new merchant whose business is clear it picks the likely
category and marks it to check. A code text the phone settled is sent along with the payment
that follows it (same sender, within fifteen minutes, the same amount), its code hidden: a
bank often names the biller only there ("مفوتر: زين" before a "Bill Payment" of SAR 299).
It also gets the user's transfer routes over the last year (from, to, how often, the last
date and amount), so it can tell a transfer from a payment and, when a text describes the
other account without digits the phone can match ("your term deposit", a card payment
that names only the card), name it from the account list (`otherAccount`); the service
sends back a name that isn't one of the user's open accounts.

**Templates.** As the assistant reads, it writes a template for each format it sees: the
text's lines with the bank's fixed words and typed slots (`{amount}`, `{currency}`, `{ref}`,
`{payee}`, `{date:dd/mm/yy}` …), and what the format means (money in or out and its kind, or a
reason to skip). Core `sms_templates.rs` compiles templates itself (no regular expressions), so
one can't hang or match everything, and keeps one only when replaying it on the batch reads
every text it matches exactly as the assistant's items do. Templates and the payees learned
from the bank's names are kept per budget and sender in the inbox database. Next time, texts
in a known format are read on the phone first and sent in a `review` list with that reading;
the assistant answers only with corrections (silence confirms) and reads the rest (`read`) in
full, writing templates for new formats and rewriting one its corrections show is wrong.
Templates read banks' own ways of writing: an amount grouped and pointed either way
("1,234.56", "1.234,56", "12,50", "1'234", "1 249,00", "1,23,456.78"; before three digits a
separator groups, "Rp 150.000", except a point in a currency with three decimals, "KWD 1.250"),
digits in any script (Arabic-Indic "١٢٠٫٥٠", Persian, Devanagari, Bengali, Thai, full-width
"１，５００"), a currency as a code or a mark ("$", "£", "€", "Rs.", "₹", "円", "원", "R",
"Ksh", "ريال", "جنيه"; a mark several currencies share is the budget's when it's one of them,
and a currency given again beside a balance isn't the amount's), a date in any order with
the month as a number or a name, in characters ("11月02日") or with no separator ("02Nov"),
with or without a year (one without is the year that puts it on or before the day the text
came; a Thai Buddhist-era year, "69" for 2569, is turned into ours, and any other year far
from the text's is left to the assistant), and a time with AM or PM. Free text may end at a
space before a slot ("セブンイレブン １，５００円"), tried at a bounded number of places so a
template still can't make reading hang. A reading reports each template the replay check
drops and why. The same alert sent twice (one sender, the same words, within two minutes)
is one item, whatever the assistant answered. The code filter knows one-time codes in English, Arabic, Spanish, French,
German, Italian, Portuguese, Turkish and Indonesian, with the code before or after its word.
The assistant's own examples come from several countries' banks, not one.
A template's payee is the one learned for the bank's name, else the user's payee the name
surely is (theirs followed by a letter or two or a branch number: "MASHAR E", "S136 M"), with
its category. Every template counts its rows filled, confirmed and corrected, and the user's
edits to them; a changed payee or category doesn't count against it (it's the user's name for
the bank's, learned for the next text, not the format misreading);
a row's editor says how often its format was confirmed, and settings list the **Learned
formats** by sender with an example text and how each is doing, to forget one or all.
Junk stays on the phone: a code the filter in `sms_templates.rs` recognises, or a text in a
skip format (rewards, a security alert) the assistant has confirmed three times over two
requests with no correction; one in 50 of those is still reviewed as an audit, and any
correction takes the trust away. Requests are sized by lane (`assistant_sms_lanes`): up to
200 read in full and 400 reviewed, with any junk, in one request of up to 1,000 texts. A
budget's first request, before it has learned any format, reads 60 in full: 200 of a phone's
real texts read in full didn't come back within the session's five minutes, and what the first
request learns makes the next ones mostly reviews.
A format rewritten three times in a month, or unused for 180 days, retires; a sender whose
recent texts are suddenly mostly new formats is pointed out in the formats list. The
assistant stays in the loop for every text but junk: templates make its work a review.

The phone matches each reference to an account: a card remembered for that sender,
otherwise the one open account whose name carries the card's digits ("4417" is "Visa
4417"), otherwise the only open account, unless the review names two or more cards the
budget doesn't know: then each is set up instead. A text that names no card (a mobile
wallet's) goes to the account chosen for its sender's texts that name none. A transfer's other account is, in order: the
account its other reference names; the other side of the one transfer of that money the
budget already has within five days (the card payment entered from the current account);
for a text naming none, the account the user chose for that card's earlier ones; the one
they chose for texts in its format; the account the assistant named; and for a text naming none, the account this one's money
moved with that way in nearly all of its last ten transfers (at least three: what pays the
card). The last two say **Check the other account**. Money to or from a person the user
once turned into a transfer (their own account at another bank, which the bank names by
the holder) is a transfer with that account from then on. Then it settles what changes nothing: a text
whose transaction is already in the budget (one candidate from the bank import engine in
the same account on the same day, or the same transfer entered from its other side, not
already claimed by another text) is matched, two texts about one transfer, one from each
account, read as two items are one transfer (the later says "The other side of a transfer
already here"), and texts where no money moved are cleared. When that's everything, the page finishes by
itself and says what it handled ("Checked 11:38 · 2 already in your budget · 3 codes").
Otherwise it counts what needs the user, and the review is a page of transactions:

- **Set up your accounts** (in a budget with accounts, **Cards and accounts to set up**):
  every card and account the texts name that no account matches, once per sender and
  digits, whether its texts are on it or a transfer names it as the other side (the savings
  a transfer went to), and a sender whose texts name none when there's nowhere to put them.
  Numbers only transfers name are one account when the words by them name the same account
  but for the first ("to/from your deposit account no."), or texts of one shape do: a bank
  that numbers each term deposit afresh is one account, not one per placement.
  Each row says what its texts are (how many, a few payees) and suggests what to do,
  `smsSetupSuggestions` in `SmsSetup.kt`: an account named for the sender, the card's network
  or the type, and its last digits ("ANB Visa 4512"), of the type the words by its digits
  say in any of the reading's languages, most of its texts deciding (credit card, savings, a
  wallet, a debit card, an account; a card that receives payments, or says nothing more of
  itself, is a credit card). A debit card (debit, mada, ATM) is suggested as **Part of** its
  sender's current account (one being set up whose own texts are on it, the main one when it
  has three times the texts of any other, or one already in the budget under the sender's
  name), since the card and the account are the same money and two accounts would count it
  twice; a credit card never is. A number only transfers name that the texts don't call the
  user's ("your …") or say what kind of account it is ("To: 6902***") is suggested as not
  tracked: it's more often someone else's. A new account's balance before the texts is worked out from them: the balance
  the latest text about it gives after its money moved (the reading's `balance`, never a
  limit or the credit left), less what the review adds to it up to that text, so after
  adding it's the bank's as of that text; a credit card starts at nothing owed, since its
  texts usually give the credit left, and with no balance in its texts an account starts at
  zero, saying so. Each row can instead be made with another name, type or balance (tap
  it), be part of an open account or one being made, or not be tracked: its texts are
  cleared and remembered so (Remembered cards says "Not added"), and transfers to it become
  spending or income. **Set up 3 accounts** makes them in one change (one Undo) with a
  credit card's payment category, remembers every card with its account, and their rows
  move to Ready to add; with nothing ready yet it's the page's button, in place of Add.
  Answers are kept with the review.
- **Needs a choice**: transfers whose other account the phone can't tell get one choice per
  account and kind of transfer (the sender's texts about one account that say the same of
  it: a card's payments, placements into term deposits the bank numbers afresh each time),
  remembered by default for the sender's next texts by the other account they all name, the
  card whose texts name none, and the formats that read them (Remembered cards lists it as
  the other side of the card's or format's transfers), or **Don't add these** for this review. A foreign amount is typed in its row; a
  message the assistant couldn't read is entered in the transaction editor; any of these
  rows can be dropped (**Don't add**, **Dismiss**). A transfer's row shows the way the money
  went ("Checking → Visa" for a card payment). These stay in the review, as they were read, until
  answered: adding the rest doesn't send them to the assistant again. A card, or a
  transfer's other side, that the budget has no account for (a new CD) gets one from
  the review (**New account…**), with its balance from before these texts; its digits are
  remembered, and nothing in it is held back as already in a starting balance.
- **Ready to add**, ticked by default, and **Holds**, unticked, newest first under the day
  they're dated, each with **Tick all** / **Untick all** and **Tick from a day** (ticks what's
  dated that day or later, unticks the rest): tap a row to change it in the transaction editor, with its messages
  shown beneath. An unticked row says **Won't be added**: it's cleared with the review. A text that may already be in the budget asks on its row (**Already in your budget?**, **Choose…**): a sheet offers adding it as new or each match from the bank import engine, and **Add** waits for the answer. These rows come first, under **Check these first**, and stay there once answered. A
  transaction dated before its account's records start (its first transaction, or today
  for an account with none) is already in that account's starting balance: it's tagged
  and unticked. Every edit applies to the review as it is when it runs, so a row the list
  didn't redraw can't undo an earlier tick.
- Above **Add**, each account the ticked rows change shows its balance now and after
  ("Checking 5,000.00 → −13,506.45"; a transfer moves both sides, matches nothing), in the
  warning colour when it would go below zero from zero or more.
- **Handled for you**, folded: texts already in the budget, matched (each can be added
  instead), and texts that aren't transactions, counted by reason.

**Prepare again with a note** re-reads the same messages with the user's note sent
apart from them. **Discard review** asks first, then clears its messages without adding
anything (left waiting, they would only be read again). Reviews survive screen and
process recreation; drafts from an older format are dropped and their messages wait again.

Adding uses the existing bank import engine through `smsBatchApply`, which commits all
ticked rows, across accounts, in one undoable step through the normal budget revision
guard. SMS is not settlement evidence: new entries are uncleared, and matching preserves
existing clearing and reconciliation, including transfer sides. A stable
sender/timestamp/body fingerprint is the import ID, kept after corrections, and every
grouped fingerprint is recorded in the budget to prevent reimport. Raw bodies are never
stored in budget metadata or assistant conversations. The review clears every message
it settles (added, unticked, held, not a transaction, dismissed), erasing its text and
keeping a local fingerprint; messages still waiting on a choice stay.

The manifest requests `READ_SMS` and `RECEIVE_SMS`, not default-SMS status or
permission to send/change messages, and `POST_NOTIFICATIONS` for background reading. **Before distributing this build through
Google Play**, complete its SMS permissions declaration under the
[SMS-based money management exception](https://support.google.com/googleplay/android-developer/answer/10208820?hl=en)
and obtain approval; align the store description, privacy policy and Data safety
answers with this feature. Eligibility is subject to Google's review. SMS remains
optional, and denied/revoked access leaves the other budgeting features available.

Implementation: `SmsInbox.kt` owns capture (SMS and text-only MMS), sender summaries, coverage and private storage;
`SmsAnalysis.kt` owns identities, card matching, draft recovery, consent, readings and the
cancellable service client; `SmsSetup.kt` the cards and accounts to set up, what's suggested for
each and what applying does (`ui/SmsSetupSection.kt` draws them); `SmsWorker.kt` reads in the background. `ui/SmsPage.kt` holds the
page's state: what's stored, the open review and a stage (loading, saving, settling or idle;
reading is `SmsReadings`'). Every change to storage runs in one lane and ends by reading
everything back and showing it at once, and the page reads what's waiting by itself only from
such a snapshot, never from texts a change has just settled. `ui/SmsImportScreen.kt` draws
the page, sender picker and remembered cards, and `ui/SmsAnalysisReview.kt` the review. Services `assistant/sms.rs`
owns the model session and its prompt; core `sms_import.rs` validates output, previews
and imports. Synthetic coverage includes `SmsAnalysisTest`, `SmsInboxTest`,
`SmsImportUiTest`, core `tests/sms_analysis.rs` and `tests/sms_import.rs`, `SmsSetupTest`,
and the service's model-loop tests. `SmsLiveCheck` (instrumented, skipped unless run with
`-e live 1`, `-e banks USD,EUR` for some) sends made-up texts in the ways banks in some 30
countries write (`shared/fixtures/sms-live-cases.json`, written by
`androidTest/cases/make_sms_live_cases.py`; a copy pushed to the app's external files folder
is used instead) through the phone's own assistant connection, each currency into a
local-only budget, compares what comes back with what each text says (telling harmful misses
from a payee written another way or a text left unclear), and sends a second round through
the formats the first learned, counting how often the assistant corrected their readings.
`-e part months` instead reads two months each of a US, an Indian and a Saudi profile's texts,
mostly junk, through the app's own reading (`SmsReadings`), and reports each request's size and
time and how much the phone handled. `-e part onboarding` reads each profile's two months into
an empty budget, takes every setup suggestion and adds every row, as a new user tapping Set up
and Add would, and reports the accounts made against the profile's, whether each money text
landed in the account its digits name, and, where a text gives a balance, whether the account
ends at it. `-e part primary` does the same with the user's own texts from the senders the open
budget (`-e home`, Primary) collects from, since `-e from` (two months back by default), into an
empty "Onboarding test" budget, and writes both budgets' accounts and transactions to
`sms-live-primary-data.json`; `androidTest/cases/compare_onboarding.py` then says how the
accounts line up, which transactions match, which each has that the other doesn't, how transfers
pair, how often payees and categories agree, and each account's balance at the end in both.
Primary is only read. `-e part all` runs every part. Its budgets ("SMS check" and the
name) live in the Trash between runs; it reopens the budget that was open (`-e home <id>` when a
stopped run left one of its own open) and puts every check budget back in the Trash. The
reports are `sms-live-check.txt`, `sms-live-months.txt`, `sms-live-onboarding.txt` and
`sms-live-primary.txt` in the app's external files folder, every reading in the `.json` beside
each. Start it without `-w` so a USB drop doesn't stop it.

## Layout

The screens follow the phone layout first made for the Tauri phone builds
([archived guide](../docs/archive/tauri-phone-apps-2026-10.md)), which takes its
structure from YNAB's phone app and keeps UrBudget's colours and words (Unassigned,
not Ready to Assign).

- `ui/Theme.kt` holds the desktop's tokens as a phone shows them. Light is grey with
  white cards. Dark is navy, because the computer's charcoal reads green on a phone.
  Money uses tabular figures. `ui/Design.kt` has the shared pieces: the top bar of
  round buttons, list cards and rows, the solid Available pill, the target bar,
  segmented controls, chips, the cleared mark and sheets.
- Budget, Transactions, Accounts and Reports are tabs on a floating tab bar. More opens
  a drawer with the budget and the other pages. "+ Transaction" floats above the bar
  and shrinks to its plus while a list scrolls down. The assistant rises as a sheet.
- Budget: Unassigned as a solid banner, which opens Quick assign or Move money. Each
  group is a band with its Assigned and Available totals. Each category shows
  Assigned and its pill, coloured by `CategoryStatuses` (the desktop's
  `categoryState`). Tapping a category opens its details as a sheet; tapping its
  Assigned amount opens that sheet on the keypad; tapping its pill moves money.
- Adding or editing a transaction takes the whole screen. The amount comes first, on
  the keypad (`AmountPad`: digits come in as cents and it does sums), then payee,
  category, account and date, with the payee and category pickers opening in turn.
  It is a layer of the main window rather than a dialog window: a floating dialog
  window can't draw under the system bars.
- Every other form is a bottom sheet with Cancel and its action at the top. Fields and
  choices take a fill that stands out from what they sit on (`LocalFieldFill`).
- Transactions: the days as bands, categories as chips, a cleared mark to tap,
  scheduled transactions on top and a banner for transactions to approve. A long
  press selects, and the selection's actions take the tab bar's place.
- Back closes the newest thing first: a picker or sheet, the selection, folded-out
  search, the assistant. Then an account's transactions go back to Accounts, a More
  page to the tab it came from, and any other tab to Budget.

## Storage, privacy and concurrency

Both clients use the same SQLite `Store` and multi-budget registry. Native edits
are serialized off the UI thread, validated, and published only after durable
revision-checked commits. Stale writes cannot replace a newer saved revision.
Queued edits verify the affected records and budget identity under the repository
lock; switching library entries disposes editors even if both budgets have the
same contents. Report results are shown only for the current range and filters.
Assistant changes require explicit review; apply and proposal-specific undo are
atomic and tied to the reviewed budget and revision. Ordinary local saves use the
shared delta path; imports preserve the full portable schema and unknown fields,
including assignment/schedule extensions.

The open budget lives in the store. `BudgetRepository` keeps a light copy for the
screens: no transaction `source` and no `sourceExport`, with `hasImportSource` on
rows that have a `source`. Commands and `edit { }` go to `edit_budget` as steps; an
`edit { }` is first turned into a patch by comparing records by identity
(`BudgetPatch.kt`). The store runs the steps on its own copy, commits the changed
rows and replies with a patch and the totals. The app applies that patch to a new
root that shares every untouched record. Undo and redo reply the same way. Previews
and totals go to `query_budget`, which never commits. Exports and recovery copies
come from `export_budget`, so they stay whole. A whole budget crosses from the app
only on create, import and restore.

An edit refused with `CONFLICT:` retries once: the repository reloads, checks the
guard against what's stored now and sends the edit again, rebuilt on that budget. A
second refusal shows the fresh budget and the error, and nothing is saved. Import
and restore do the same. A preview or a month change the store has moved past
catches up and asks once more. An edit that saved but can't be shown
(`SavedNotShown`) isn't treated as a failed edit: nothing is offered to retry and
the app refreshes. Its error shows until that refresh succeeds, and an editor that
closes on success, such as a transaction's, stays open.

An earlier native `filesDir/budget.json` migrates to SQLite only after validation
and a successful commit. Its original bytes remain for recovery. A signed upgrade
from Tauri finds the existing library directly in `filesDir`; otherwise native
state lives in `filesDir/native`. Credentials stay in the same sealed vault with
its key protected by Android Keystore and excluded from backup. Only a confirmed
missing secret is treated as absent; decryption/I/O failures do not reset it.

ChatGPT sign-in keeps its PKCE callback on `127.0.0.1`. The callback page offers
**Return to UrBudget**, which targets the initiating package and resumes its
existing activity. This link carries no OAuth code or credentials and only
refreshes status; receiving it cannot sign someone in. The token exchange waits
until the activity is started again, so it runs in the foreground rather than
behind the browser. Settings refreshes on
resume and polls pending attempts, shows the current stage, and retains the last
sign-in error until the next attempt even if Settings is reopened. The
`UrBudgetSignIn` diagnostic log contains stage names only. The token
exchange has a 30-second deadline and remains cancellable after the callback.
The page acknowledges receipt instead of claiming success before credentials
have been validated and saved.

`AssistantSignInTest` covers completion, resumed status and failures;
`NativeSignInTest` exercises the real JNI callback with synthetic consent denial;
`SignInReturnActivityTest` checks activity reuse. Run the two instrumentation
classes in separate processes because the JNI test uses an isolated data folder.
The Rust auth tests also exchange a synthetic authorization code with a local
token endpoint and verify PKCE and ID-token claims. Live account consent still
needs a manual device test.
The optional `NativeSignInTest#tokenTransportWithSyntheticGrant` instrumentation
test requires `-e checkAuthNetwork true`; it uses a fabricated grant to verify
foreground handoff and connectivity to the real token endpoint, expecting rejection.

Automatic Android system backup/device transfer is disabled. Explicit portable
budget exports contain budget data, not account/provider/bank credentials.
Generated files are shared through a scoped FileProvider cache with temporary
read grants. No broad storage permission is requested.

## Build and tests

Use JDK 17+, Android SDK 37, NDK `30.0.16248370`, and Rust stable. Set `JAVA_HOME`,
`ANDROID_HOME`, `NDK_HOME` and put Cargo on `PATH`.

```powershell
rustup target add aarch64-linux-android x86_64-linux-android
cd android
.\gradlew.bat :app:testDebugUnitTest :app:lintDebug :app:assembleDebug
.\gradlew.bat :app:lintRelease :app:assembleRelease
# Optimized, debug-signed build for evaluating responsiveness on a physical device:
.\gradlew.bat :app:assemblePreview
```

Use `bash gradlew` on macOS/Linux. Default builds include ARM64 and x86_64; use
`-PandroidAbis=x86_64` for emulator work. Gradle invokes Cargo directly with the
NDK linker. The native library includes services and accounting code.

```powershell
# Repository root: dependency unit tests are a separate Cargo workspace.
cargo test --locked --manifest-path crates/Cargo.toml
# Desktop integration adapters and existing checks:
cd desktop
npm test
```

Tests on an emulator have their own steps: [Instrumented checks](#instrumented-checks).

Native CI runs shared tests, Kotlin tests, debug/release lint and builds for both
ABIs; **Run workflow** also builds preview and keeps the APKs for three days. Changes to shared Rust also run the desktop checks. AGP built-in Kotlin requires custom shared sources under
`AndroidSourceSet.kotlin`, per [Android's migration guide](https://developer.android.com/build/migrate-to-built-in-kotlin).

Artifacts:
- `app/build/outputs/apk/debug/app-debug.apk`
- `app/build/outputs/apk/preview/app-preview.apk` (optimized device evaluation)
- `app/build/outputs/apk/release/app-release-unsigned.apk` (without signing secrets)
- `app/build/reports/tests/`, lint reports

For a signed release, provide all four existing environment variables:
`URBUDGET_ANDROID_KEYSTORE`, `URBUDGET_ANDROID_KEY_ALIAS`,
`URBUDGET_ANDROID_STORE_PASSWORD`, `URBUDGET_ANDROID_KEY_PASSWORD`.
The default version code matches `versionName` (1.2.3 is 1002003; `scripts/versions.mjs`
sets both). Use `-PversionCode=<next Play version code>` only to upload a version again. This
configuration does not upload or publish anything. Releases are tagged `android-v1.2.3` and
built by `.github/workflows/android-release.yml` ([RELEASING.md](../RELEASING.md)). Google Play checkout requires
the release package, appropriate signing/test track and the configured subscription;
debug's separate application ID cannot validate production product availability.

Use **preview** for everyday sideloaded testing. It uses the same
`app.urbudget.nativeapp` package and debug signing certificate, preserving debug
budgets when installed with `adb install -r`, but enables R8 and resource shrinking
and disables debugging/tooling overhead. It is shell-profileable. Keep debug for
debugger/instrumentation work; its frame timings do not represent release
performance, as explained in the [Compose performance guide](https://developer.android.com/develop/ui/compose/performance).

Published budget snapshots are read-only. Unchanged revision probes reuse their
objects and calculations, read-only queries run outside the edit queue, and list
filtering happens on a worker. UI scope keys use snapshot identity rather than
serializing entire budgets while typing or rendering.

Advanced editor choices, split rows, loan schedules, import reviews and assistant
report tables use bounded lazy lists. Large choice lists are searchable without
discarding options. Payee discovery, match labels, file encoding, Quick assign
display rows and report preparation run off the UI thread; obsolete results are
cancelled or rejected when their budget or inputs change.

Assistant token updates are buffered and published at most every 64 ms while
streaming, with a final flush. The live reply updates independently of history
and settings; unchanged conversation entries retain their identities. Service
reads no longer occupy the local edit queue. In-flight identical reads and
automatic sync attempts are coalesced, with budget/credential guards and refresh
after a sync commits even if its original screen has closed.

Native bank previews use the shared Rust matching engine's compact transport.
Rows refer to pooled candidate sets instead of copying thousands of candidates
into every JSON row. Kotlin reuses those lists, and commits send only selected
matches and required acknowledgments. Rust still revalidates selections against
the current ledger. Desktop's full review format remains compatible.

## Instrumented checks

Run these on an isolated emulator, one process per class: the services are set up
once per process, and two setups clash ("The service directory cannot change").

**Never use a `connected*AndroidTest` Gradle task.** It installs on every attached
device and uninstalls the app afterwards, so a phone plugged in loses its debug or
preview install and the budgets in it.

```powershell
# android/, with the emulator running:
.\gradlew.bat :app:assembleDebug :app:assembleDebugAndroidTest -PandroidAbis=x86_64
adb -s emulator-5584 install -r app/build/outputs/apk/debug/app-debug.apk
adb -s emulator-5584 install -r -t app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
python app/src/androidTest/cases/run_edit_checks.py --out $env:TEMP\edit-checks
```

`run_edit_checks.py` runs the engine-edit classes and `ui.SmsImportUiTest` (with
`-e large 1`, 3,000 more rows), each in its own process. Then come 10 cycles of
`EngineEditDeathCheck`, which force-stops the app while it's editing and checks the
store in a new process. A class passes only on its exact `OK (n tests)`, so a lost
test fails it and the counts in the script's `CLASSES` change with the tests. Its
output must also hold none of the script's `BAD` marks, because `am instrument` can
exit zero after a failure. A test skipped by `assumeTrue` still counts in that `OK`,
so the script runs everything with `-e strict 1`, which makes a phone a failure and
not a skip. The emulator check is the only `assumeTrue` in `CLASSES`.

It targets `emulator-5584`, or `EDIT_CHECK_SERIAL` if that's set, and refuses a
serial that isn't `emulator-…`. `--only Class ...` runs some of the classes,
`--deaths n` or `--no-deaths` changes the kills. Without `--out` the logs go to a
dated folder beside the script, which git doesn't ignore. From
`app/src/androidTest/cases`, `python -m unittest test_run_edit_checks` tests the
script with adb stubbed.

The other classes aren't in the script. Run each by itself, and read the output, not
the exit code:

```powershell
adb -s emulator-5584 shell am instrument -w -e class app.urbudget.nativeapp.NativeEngineTest app.urbudget.nativeapp.test/androidx.test.runner.AndroidJUnitRunner
```

These arguments go with `am instrument`:

| Argument | What it does |
| --- | --- |
| `-e quick 1` | Cuts the synthetic budgets to 300 transactions, for a run during development. At full size `EngineEditEquivalenceTest` and `EngineEditUndoTest` took about 24 and 29 minutes in the last whole check. The script doesn't pass it |
| `-e perf 1` | Runs `EngineEditPerformanceCheck` (30 clear taps at 2,700 and 10,000 transactions, emulator only, result in `edit-perf.txt`) and `EngineEditGateCheck` (30 status taps through `edit_budget` alone at 2,742, result in `edit-gate.txt`), both in the app's external files folder. The first fails its "ratio under 1.5" assertion, because an edit's cost still grows with the budget ([plan](../docs/plans/engine-edits.md#as-built)). The second doesn't insist on an emulator: its budget, "SMS check gate", is local, goes to the Trash afterwards, and the budget that was open is reopened (`-e home <library id>` after a stopped run) |
| `-e perf 1 -e trace 1` | `EngineEditPerformanceCheck#leaveABudgetOpenForATrace` leaves a real-sized budget open. Install preview over it and take a Perfetto trace: `Tracing.kt` adds `service:<method>`, `repo.*`, `storage.*`, `vm.publish` and `ui.*` sections |

`SyncV2RuntimeTest` isn't in the script either, and a plain `am instrument` of it
proves nothing: without `-e syncQaOrigin` it's skipped, and a skip reads as a pass,
`OK (1 test)`. A real run needs a local server, which
`web/scripts/sync-v2-android-ci.mjs` starts, a debug Rust build, which is what allows
that server, and cleared app data:

```powershell
# Repository root. The script installs the two APKs and passes syncQaOrigin.
npm ci --prefix web
.\android\gradlew.bat -p android :app:assembleDebug :app:assembleDebugAndroidTest -PandroidAbis=x86_64 -PrustDebug=true
adb -s emulator-5584 shell pm clear app.urbudget.nativeapp
$env:ANDROID_SERIAL = "emulator-5584"
node web/scripts/sync-v2-android-ci.mjs
```

Each run starts a new server and the app remembers the last one, so a second run
without `pm clear` is refused. A library left by other classes slows sync past the
test's waits, and on one with 462 budgets a run never finished
([follow-ups](../docs/FOLLOW-UPS.md#what-fast-edits-on-android-left)). Build again
without `-PrustDebug=true` before timing anything. The `sync-lifecycle` job in
[android-native.yml](../.github/workflows/android-native.yml) runs this test in CI.

## Verification and remaining external acceptance

Local verification on 2026-10-21 passed:

- 439 shared Rust tests (303 core, 125 services, 9 JNI adapter, 2 cross-platform);
  two explicitly ignored desktop benchmarks remain ignored.
- 617 desktop UI tests and TypeScript checking; desktop Rust adapter checks/tests.
- 55 native Kotlin unit tests and 13 Android emulator instrumentation tests.
- Both-ABI debug, preview and optimized release APK builds; all three variants' lint with no
  errors; retained JNI/Keystore entry points and 16 KiB native/APK alignment.
- Emulator smoke checks for first-run onboarding, sample budgets, assistant
  settings, bank setup, report drilldowns/editors, and fresh-start persistence
  after restarting the app. Tests used only an isolated API 37 emulator.
- Optimized preview smoke launch on the isolated emulator and in-place update on
  the authorized Moto G77; matching debug certificate, JNI/Keystore retention and
  16 KiB APK alignment verified. Instrumentation stayed on the emulator.

Moto G77 navigation comparison on 2026-10-21 used the same ten tab switches with
0.5-second pauses, after warming the screens and resetting `dumpsys gfxinfo`:

| Frame timing | Original debug | Optimized preview | Preview repeat |
| --- | ---: | ---: | ---: |
| Frames sampled | 998 | 1,040 | 1,014 |
| Median | 12 ms | 12 ms | 12 ms |
| 95th percentile | 30 ms | 18 ms | 18 ms |
| 99th percentile | 109 ms | 40 ms | 42 ms |

This measures the combined code changes and release optimizations during tab
navigation, not every editor or network workflow. Percentiles are bucketed and
the samples are short. The device's modern and legacy jank counters disagree,
so they are not used to claim a jank rate. Raw counters are in the local ignored
`reports/moto-performance-{debug-before,preview-after,preview-repeat}.txt` files.

The advanced-workflow stress check used an isolated emulator with 10,004
transactions, 1,003 categories and 2,000 derived payees. The old optimized build
ran out of Java heap while parsing a 1,000-row bank preview. The updated build
opened the complete review, and late match candidates remained searchable. No
rows were imported during that manual check. A separate JNI regression applies
the last match in a pooled candidate set and verifies the intended transaction;
shared Rust tests expand compact reviews back to the full desktop representation
and check a 1,000-row, 10,000-candidate review stays below 1 MiB on the wire.

The large category-editor sample improved from p95 81 / p99 350 ms (128 frames)
to p95 65 / p99 250 ms (85 frames). Signed-out service-screen navigation measured
p99 31 ms before and 32 ms after. These are short emulator samples, not physical
device or provider latency benchmarks. Compose instrumentation separately checks
that 2,000-option selectors, 2,000-row reviews and 2,000-row assistant report
tables keep the final item reachable while composing only visible rows.

The Moto advanced-workflow check opened and cancelled schedule, loan and payee
editors, typed an unsaved payee name, and visited sync, assistant and bank screens.
The same sequence ran before and after the update; no saves, provider messages,
bank connections or purchases were submitted. Final loan/schedule smoke checks
on the emulator also verified scrolling preserves drafts and reaches the lower
controls.

| Advanced workflow frame timing | Prior optimized build | Final build, first pass | Final build, repeat |
| --- | ---: | ---: | ---: |
| Frames sampled | 1,469 | 1,607 | 1,509 |
| Median | 10 ms | 11 ms | 11 ms |
| 95th percentile | 16 ms | 20 ms | 28 ms |
| 99th percentile | 77 ms | 105 ms | 97 ms |

These short physical-device runs did **not** demonstrate an aggregate navigation
speedup. Screen/editor opening still has occasional slower frames; an isolated
typing check measured p99 9 ms. Temperature and power checks showed no reported
thermal throttling or battery-saver mode. The changes demonstrably remove the
large-import heap failure, eager large lists, repeated presentation work and
service/edit-queue contention; they do not establish that every opening frame or
live provider request is faster. Raw samples are retained locally under
`reports/advanced-*.txt`.

Automated checks cover the shared accounting/service suites, HTTP protocol mocks,
identity changes, encrypted merge/conflict races, native SQLite revision/history
behavior, secret authentication, attachment failures and assistant proposal
guards. Android instrumentation uses synthetic budgets and
files; no real provider keys, bank credentials, sign-in emails or purchases are
used. The optimized release is also checked to keep JNI/Keystore entry points.

Before publishing, exercise real email-code and ChatGPT browser sign-in, provider
responses, SimpleFIN/YNAB accounts, and Play test-track subscription purchase/pending/restore
with authorized test accounts. Passing mocked and local tests establishes code
behavior, not acceptance by those live services. Test signed upgrades with a
backed-up real-version sandbox as well.

The repository's ignored desktop performance test has pre-existing stale stored
hashes: the untouched branch base produces the same actual fingerprints as the
extracted engine. Those golden expectations are not silently rewritten here.
