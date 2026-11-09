# Bank texts in the assistant's chat

**Status:** proposal, 2026-11-09. All five steps are built as of the same day, uncommitted; see As built.
**Goal:** the chat assistant can answer questions about bank texts, search the bank's texts on
the phone, check accounts against the balances the bank reports, and propose what to do. Every
transaction still goes through the review on the Bank messages page.
**Scope:** Android only; iPhone and desktop can't read texts. Services `assistant/agent.rs`,
`assistant/tools.rs`, `assistant/prompt.rs`, `assistant/proposals.rs`, `host.rs`; Android
`AssistantController.kt`, `SmsInbox.kt`, `ui/SmsPage.kt`, `ui/SmsAnalysisReview.kt`, `SmsSetup.kt`.
The card's credit limit (section 6) is the exception: it's budget data, so every platform's account
editor gets it (desktop `forms.tsx` `AccountForm`, Android `ui/AccountsReports.kt`, iPhone
`Screens/Accounts/AccountsView.swift`).
**Related:** [sms-assistant.md](sms-assistant.md) (how the reading works),
[sms-templates.md](sms-templates.md) (learned formats).

## As built

This section is current; the rest of the page is the proposal as written. Where they differ,
this section is right.

- **Round trip:** `assistant/phone.rs` (`assistant://phone`, `assistant_phone_reply`, 20 seconds),
  hooked into `agent.rs` before `tools::call`. `prompt::Screen.bank_texts` turns the three tools on;
  MCP never offers them (its read-only list is explicit).
- **Phone side:** `BankTextsAssistant.kt` answers the tools and applies `bankTexts` cards;
  `BankTexts.kt` holds the pure parts (outcomes, kept balances, the bank's figure, the review
  check, amount forms). `App.kt` answers requests while the app is open.
- **Store:** `SmsInbox` version 5 adds `messages.outcome`; `records`, `bring` and `waitingRange` are
  new. `sms_state` `"balances"` keeps each account's latest figure. Outcomes stay until Stop and
  clear.
- **Search:** `SmsReader.search`, chosen senders only, SMS and text-only MMS, at most a year.
- **Cards:** `read` and `add` open Bank messages through `BankTextsAssistant.reads`
  (`SmsPage.assistantRead`); `remember` writes the card and bumps `changed` so an open page reads
  it again; `limit` is `BudgetViewModel.setCreditLimit`, which can also remove the key.
- **Credit limit:** `creditLimit` in cents on credit accounts, edited on Android (`AccountDialog`),
  desktop (`AccountForm`) and iPhone (`AccountForm`), with the credit left on each account's
  summary. Core `Account::credit_limit`; `get_setup` lists it and the credit left. Arabic: الحد
  الائتماني and الحد المتبقي, in `docs/LOCALIZATION.md`.
- **Wording:** `sms_send_notice` and `sms_disclosure` (English and Arabic), the website's privacy
  page (and its in-app copy), and the SMS declaration in `docs/play-listing`.

Where it differs from the proposal:

- `available` is only a card's credit left. A bank account's available balance stays in
  `balance`, as formats already read it ("Avl Bal").
- `search_bank_texts` has no `direction`: the phone can't tell money in from out for texts it
  hasn't read. Results don't carry the phone's template reading.
- The review check counts the review's rows by when each text came, so same-day texts after the
  one that gives the figure don't upset it; the budget's own transactions are counted through
  that day. A card's `balance` is read as its credit left, as ANB and most banks text it, unless its texts move the other way (`smsCardOwes`: a purchase raising it means it's what's owed). The Fold audit of 2026-11-09 found ANB's card "Balance" is the credit left, consistent to the cent across 448 of 481 texts.
- A new card with a limit starts from its credit left (`smsOpening` with a limit); the setup
  dialog asks for the limit.

Not verified: the iPhone changes haven't been compiled (no Mac here; CI builds them), nothing has
been tried on the Fold, and no reply from a real model has gone through the round trip (the Rust
side and the phone's answers are each tested). Person numbers among the chosen senders are
searched like any chosen sender.

## Why the assistant can't help today

- **Two places that never meet.** Chat tools run in the shared services and see the budget, the
  SimpleFIN snapshot and attachments (`tools::Context`). Bank texts live in the phone's own
  database (`SmsInbox`: `messages`, `sms_state`, `sms_templates`, `sms_payees`), and only the Bank
  messages page talks to it. The reading itself (`assistant/sms.rs`) is a separate session that
  gets each batch from the phone and keeps nothing.
- **The "why" isn't kept.** `SmsInbox.finish` marks a text done and blanks its body. The budget
  keeps the ids of texts it added (`smsImportedMessageIds`, and each transaction's
  `bankImportIds`), but nothing records why a text was skipped, cleared or dismissed. Even with
  access, "why didn't my transfer show up?" has no answer.
- **The bank's balance is read, then dropped.** Readings already carry `balance`, the balance a
  text gives after its money moved (core `sms_import::Transaction`, the `{balance}` slot in
  learned formats). The only use is a new account's starting balance (`smsOpening`); it goes
  with the review.

## What the user could ask

| They ask | What happens |
|---|---|
| "Anything waiting in bank messages?" | `get_bank_texts`: 23 texts waiting since 6 Nov; the open review has 2 rows that need a choice. |
| "Did my salary come in?" | `search_bank_texts` for money in this month: the 1 Nov text was added as Employer Co., 18,000.00. |
| "Why is Careem missing?" | `search_bank_texts` for the payee: the 6 Nov text was cleared on 7 Nov when its row was unticked. Proposes adding it to the review. |
| "Where's my September rent transfer?" | `search_bank_texts` finds it in Messages, dated before the start date picked for collecting. Proposes adding it to the review. |
| "Is ANB right?" | `get_bank_texts`: the bank said 9,120.40 at 14:02 on 5 Nov; the budget says 9,075.40 then. The assistant searches the gap and finds a 45.00 text still waiting. |
| "Is my Visa right?" | The latest Visa text gave 8,455.00 of credit left. With a 10,000.00 limit, 1,545.00 is owed or held; the budget says 1,500.00 owed, and the 45.00 between them is a hold still unticked in the review. |
| "How much can I still spend on my Visa?" | The limit less what the budget says is owed. Works on every platform, texts or not. |
| "Card 3358 is my savings card" | Proposes remembering it, then reading the waiting texts again with that note. |

## 1. The phone answers the assistant

Tools run synchronously inside a reply today: `agent.rs` calls `tools::call(name, args, ctx)`
and sends the result back to the model. Only the phone can open its message store, so bank-text
tools need a round trip to it.

- **Request.** For a phone tool, the agent loop emits `assistant://phone`
  `{threadId, requestId, tool, args}` through `host::emit`, the way it emits `assistant://event`,
  and awaits the answer. `tools::call` stays synchronous; the loop takes the phone path first.
- **Reply.** A new dispatch command, `assistant_phone_reply {requestId, result | error}`, hands
  the answer back. Unknown or late ids are ignored.
- **Limits.** 20 seconds a request. Stopping the reply cancels it. If the phone doesn't answer,
  the tool returns an error the model can act on ("The phone didn't answer. UrBudget needs to
  stay open while it searches.").
- **On the phone.** `AssistantController` already collects `NativeServices.events`. It hands
  `assistant://phone` requests to a new `BankTextsAssistant`, which runs them off the main thread
  and replies.
- **Offered only where it works.** `prompt::Screen` gets a `bankTexts` flag, which the phone sets
  when this budget has senders chosen and the assistant's consent for texts. `tools::specs()` adds
  the bank-text tools only then. Elsewhere, the prompt says bank texts are an Android feature.

Most of this is plumbing that SimpleFIN didn't need: Rust can already push events to the phone,
and the phone can already call Rust. A second store in Rust was considered and dropped: it means
migrating the Fold's texts, cards and formats, and keeping a second copy of bank texts.

## 2. What the phone keeps

- **An outcome for each text**, written when a review settles it (`SmsReview.confirm`,
  `SmsPage.finish`, `SmsPage.dismiss`):
  `added` or `matched` (with the transaction id), `skipped` (with its reason: code, security,
  declined…), `cleared` (unticked), `dismissed`, or `kept` (still waiting on a choice). Each gets
  its date, amount, account, payee as read, and the balance the text gave. The body is still
  blanked. A new `outcome` column on `messages` (schema version 5). Kept until **Stop and clear**,
  like the rest of the budget's texts.
- **The bank's latest balance per account:** the balance, when the text came, and the text's id,
  updated whenever a text that gives a balance is added or matched. Kept in `sms_state` as
  `"balances"`.
- **Cards through their credit limit.** `SmsSetup` already notes that a card's "balance" in its
  texts is usually the credit left, so a new card account starts at zero instead of using it. A
  card's figure is kept as the credit left, and becomes what's owed once the card has a limit
  (section 6).

## 3. Two tools

### `get_bank_texts` (no arguments)

From `SmsInbox` and the open review, no text bodies:

- the chosen senders, the date collecting starts, and whether texts are read as they arrive;
- texts waiting: how many, and their first and last dates;
- the open review: ready, needing a choice, holds, already in the budget, skipped;
- the last review's summary (`sms_state` `"last"`);
- remembered cards (digits to account name) and how many formats the budget has learned;
- for each account with a bank balance: the bank's figure and when it came, the budget's balance
  at that moment, and the difference.

### `search_bank_texts`

Searches the bank's texts in the phone's Messages store, SMS and text-only MMS, the way
`SmsInbox.refresh` reads them for collecting (MMS through its `mms` reader):

- **Arguments:** `from`, `to` (default the last 30 days, at most a year), `amount`, `words`,
  `sender` (one of the chosen senders), `direction` (in or out).
- **Only the chosen senders, always.** The phone enforces this, not the prompt. Google's SMS
  policy allows SMS only for money management and bars sharing personal SMS.
- **Any date.** Includes texts before the start date, texts UrBudget never collected, and
  reviewed texts whose copy UrBudget blanked. A text deleted in Messages is gone.
- **Amounts in any written form:** "1,234.50", "1234.5", "1.234,50" and Arabic-Indic digits all
  match 1234.50. The phone narrows by sender and date first, then matches amount and words.
- **Returns** up to 20 texts, newest first, plus how many more matched. Each has its id, sender,
  time, the text, the phone's reading when a learned format reads it, and its outcome (or "never
  collected").

The prompt tells the assistant to search narrowly and to link what it finds to transactions
(`find_transactions`) before answering.

## 4. Proposals

A new proposal kind, `bankTexts`, applied on the phone (the card's button calls
`BankTextsAssistant`, then `assistant_mark` marks it applied):

- **Read the waiting texts**, optionally with a note. With a review open, this is the review
  menu's **Prepare again with a note**.
- **Add these texts to the review:** ids from a search. The phone collects them even when they
  predate the start date, reads them, and opens the review.
- **Remember a card:** digits a text gives, to an account (`smsRememberCard`).

The card says what will happen. Nothing enters the budget except through the review, as with
`prepare_import` and the import review.

## 5. The bank's balance in the review

**Balances after adding** gains the bank's figure for each account whose latest text in the
review gives a balance: a tick when the budget after adding matches it, or the difference. The
budget's side counts what's in the budget dated through that text's day, plus the review's
ticked rows received up to that text. Same-day texts after it that give no balance would make
this wrong, so the check uses the day's last text that gives one, and is left out when a later
same-day text moves money without one. A card is checked once it has a limit (section 6).

## 6. Cards: the credit limit

Card texts usually give the credit left ("Available limit SAR 8,500.00"), not what's owed. That's
why the reading is told to leave it out today, and why `smsSetupBalance` starts a new card at
zero. With the card's limit, what's owed is the limit less the credit left.

- **A limit on the account.** An optional `creditLimit`, in cents, on credit card accounts,
  beside `accountType`. It's set in Edit account on every platform and synced with the budget
  like the account's other fields. Android's save merges into the record (`upsertRecord`), so a
  limit set on another device survives an edit there.
- **Readings say which balance a text gives.** `balance` keeps its meaning: the account's
  balance, or what's owed on a card. A new `available` holds an available balance or a card's
  credit left. The reading prompt's rule changes from "never a card's available credit" to "put
  it in `available`", learned formats get an `{available}` slot beside `{balance}`, and the core
  checks it as a plain decimal like `balance`.
- **The check.** The limit less the credit left is what's owed plus any holds. The budget's card
  balance (negative when owed), with the review's ticked holds, should be minus that at the text,
  counted as in section 5.
- **Holds.** A pending charge lowers the credit left before it's in the budget, and the review's
  holds are unticked by default. When a difference equals the unticked holds, the review and the
  assistant say that instead of calling it a mismatch. The same goes for a bank account's
  available balance.
- **A new card's starting balance.** With a limit and a text giving the credit left, setup works
  out what was owed before the review's texts, as `smsOpening` does for bank accounts, instead of
  starting at zero. The setup dialog asks for the limit when a card's texts give the credit left.
- **Finding the limit.** The user enters it, or the assistant proposes it from a text that states
  it ("Credit limit SAR 10,000"). It can also be suggested as the credit left plus what the
  budget says is owed at that text, but only as a question ("Is your card's limit 10,000.00?"),
  because that assumes the budget is right at that moment.
- **Useful beyond texts.** Accounts can show a card's credit left, and `get_setup` can include
  limits, so "how much can I still spend?" works on every platform.

## Privacy and Google

| What | Where it goes | Kept |
|---|---|---|
| Status from `get_bank_texts` | the AI provider | in that chat |
| Texts `search_bank_texts` finds | the AI provider | in that chat, until it's deleted |
| Waiting texts read for the review | the AI provider, as today | not in a chat, as today |

Search sends full texts to the provider, and the chat thread file keeps them
(`<budget folder>/assistant/threads`, on the phone, excluded from Android backup). So the
wording must change before search ships:

- **`sms_send_notice`** says texts "aren't saved in your assistant chats". Suggested: "When you
  ask your assistant about bank texts, it can search texts from your chosen senders, and what it
  finds is kept in that chat until you delete it."
- **`sms_disclosure`** says "Messages go to your assistant only when you prepare them".
  Suggested: "…when you prepare them, or when you ask your assistant about them."
- Both in Arabic too, and the privacy page on the website.
- **The SMS declaration** (`docs/play-listing`, not filed yet): add that texts from chosen
  senders go to the provider when the person asks the assistant to search them.
- Data safety needs no change: SMS is already declared as shared with the AI provider.

## Testing

- **Rust:** the round trip answers, times out and is cancelled by Stop; late replies are
  ignored; the tools are offered only with `bankTexts`; tool output never carries a sender
  outside the request.
- **Core:** `available` read and checked like `balance`, in readings and learned formats; what's
  owed worked out from a limit; a card's starting balance from the credit left.
- **Each platform's account editor:** the limit is saved, shown, cleared, and kept when the
  account is edited elsewhere.
- **Kotlin unit:** amount forms (grouping, decimal comma, Arabic-Indic digits), word matching, the
  date limits, the sender limit, and outcomes from each way a review settles a text.
- **Emulator** (`UrBudgetSmsQa`): outcomes recorded through a review; search finds texts delivered
  with `adb emu sms send`, including before the start date; each proposal ends in the review.
  Instrumentation can't write to the SMS provider, so delivered texts are the way in.
- **Live:** the multi-country check (made-up texts from about 30 countries) through the Fold's
  ChatGPT, on the **SMS Trial** budget only.

## Steps

1. **The round trip and `get_bank_texts`.** The smallest useful piece: status questions work.
2. **Outcomes and `search_bank_texts`,** with the new wording. Search can't ship before the
   wording does.
3. **Proposals.**
4. **Balances:** keep the latest, add them to `get_bank_texts`, then the check in the review.
5. **Card limits:** the field on every platform, `available` in readings, card checks and a new
   card's starting balance. The field alone could ship earlier, since the credit left on a card is
   useful without texts.

## Open questions

- **Suggesting a limit.** Is working it out from the budget and a text worth offering, or should a
  limit come only from the user or a text that states it?
- **A person's number as a sender.** The sender picker allows it. Should search leave numbers out
  unless the user asks?
- **How long outcomes are kept.** Until Stop and clear, or a fixed time such as 13 months?
- **The panel.** Should a search show as a step line ("Searched ANB texts: 3 found") with the
  texts one tap away, or only in the assistant's answer?
