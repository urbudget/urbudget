# Bank texts as onboarding: from an empty budget to a populated one

Status: built 2026-11-03 (`SmsSetup.kt`, `ui/SmsSetupSection.kt`). Where the build differs
from the design below, it says so.

## What it's for

Someone with a new, empty budget opens **Bank messages**. They pick their banks and how far back
to go. The app reads the texts, finds every card and account they mention, and asks once, in one
place, which account each one is: one that exists, one to create on the spot (with a name, type and
starting balance worked out from the texts), part of another (a debit card drawing on a current
account), or not tracked. Then it adds the transactions, sorted into those accounts, with payees,
categories and transfers. A new user goes from nothing to a populated budget in a few taps.

The same section replaces the per-card questions for existing users: when a review meets cards the
budget doesn't know, they're set up the same way.

## What exists and is reused unchanged

| Piece | Where |
| --- | --- |
| Senders, the start of collecting, consent, background reading | `ui/SmsPage.kt`, `SmsInbox.kt` |
| Reading: lanes, learned formats, requests sized by lane | `SmsAnalysis.kt` (`SmsReadings`), services `assistant/sms.rs` |
| A card's texts grouped so one choice answers them (`SmsItem.card`) | `SmsAnalysis.kt` |
| A choice remembered for the card's digits; several numbers to one account | `smsRememberCard`, `SmsAccounts` |
| Accounts made from a review hold nothing back as already in a balance (`SmsBatch.fresh`) | `SmsAnalysis.kt`, `SmsAnalysisReview.kt` |
| Adding, matching what's in the budget, transfers paired, payees and categories | core `sms_import.rs`, `bank_import.rs` |

Nothing changes in the reading engine, the core import, or the data model. The assistant's output
gains one optional field (below).

## The flow

1. **Banks.** The sender picker as it is.
2. **How far back.** An empty budget (no open accounts) is offered **since a date two months
   back**, saying the budget will be built from those texts, instead of today's "from now on"
   (`SmsStart.suggested`). A budget with accounts keeps today's suggestion.
3. **Reading.** As now.
4. **Set up your accounts** (new; the top of the review). One row per card or account the budget
   doesn't know, each with three choices and a suggested setup; **Create all** applies the
   suggestions. Details below.
5. **The transactions.** The review as now, its rows already in the accounts just set up, under
   **Ready to add**, with **Add all**.
6. **From then on**, new texts are read as they arrive with the cards and formats now known.

Steps 4–5 are one page: the setup section sits above the rows and the rows fill in as it's
answered. Leaving the page keeps both (the review's draft holds the setup choices).

## The setup section

### Candidates

`smsSetupCandidates(batch, items, accounts)`, a pure function in `SmsAnalysis.kt`:

- Every item still needing an account (`needs == "account"`), grouped by `(sender key, ref key)`.
  `ref key` is `smsRefKey`, so `***0026`, `0026` and `٠٠٢٦` are one.
- Every transfer whose other account the phone can't find and that names one (`otherRef`),
  grouped the same way (the savings `***7714` a transfer went to).
- A sender's texts that name no card, when the budget has no account to put them in, as one
  candidate for the sender ("M-Pesa").
- For each: its texts' count, a few payees, the kinds of money (spending, income, card payments
  received), and the latest balance a text gives with that text's date (below).

### Suggestions

`smsSetupSuggestion(candidate, all)`, pure and tested by itself:

- **Type** from the words of the candidate's own texts, in the languages the reading knows:
  - credit card: `credit card`, `Cr. Card`, `crédito`, `crédit`, `ائتمان`, `Kreditkarte`, a card
    that receives "card payment" texts;
  - current account: `mada`, `debit`, `A/c`, `account`, `current`, `checking`, `chequing`, `حساب`,
    `Konto`, `cuenta`, `compte`, `conta`;
  - savings: `savings`, `ادخار`, `Tagesgeld`, `épargne`, `ahorro`, `poupança`;
  - cash wallet: `M-PESA`, `wallet`, `محفظة`, `GCash`;
  - `VISA` or `Mastercard` with none of the above: credit card. Otherwise: current account.
  - Built: a card that says nothing more of itself ("spent on card XX5678") is a credit card
    too. Reading it as a debit card would fold it into its sender's account, which is wrong for
    both; only explicit debit words do that.
- **Name**: the sender's display name, the product word if one was found, and the digits:
  "ANB Visa 4512", "ANB Current 0026", "M-Pesa".
- **Belongs to**: a card that reads as a debit card (`mada`, `debit`), when exactly one other
  candidate from the same sender reads as a current account, is suggested as **belonging to
  that account** instead of being created. That is the trap this section is for: card 3875 and
  account 0026 are the same money, and two accounts would count it twice.
- **Starting balance** (below).

### Choices per row

- **Create** (default unless *belongs to* is suggested): the name, type and starting balance
  shown; tapping the row opens the account form (`SmsNewAccountDialog`) with them filled in.
- **Belongs to ▾**: an existing open account, or another candidate being created in this
  section ("ANB Current 0026, new").
- **Don't track**: the card's texts are cleared as they are today with **Don't add these**
  (remembered as `SmsSkipCard`).

**Create all** (or **Set up N accounts**) at the section's foot applies every row. Rows answered
by hand keep their answer. Built: with nothing ready to add yet (an empty budget), the button is
the page's own, in place of Add, so it's in reach however many rows there are.

### Applying

`applySetup(plans)` in `SmsAnalysisReview.kt`:

1. For each **Create**, `vm.saveAccount(id, name, kind, opening, type)`, in one batch;
   accounts that **belong to** a created one get no account of their own.
2. Once the budget has them (`vm.afterPending`, as `SmsNewAccountDialog` does): remember every
   candidate's digits with its account (`smsRememberCard`, one `saveCards`), and add the new ids
   to the batch's `fresh`.
3. The items resolve through the remembered cards as today: the rows move to **Ready to add**.

Creating accounts goes through the budget's normal edits, so **Undo** takes them back like any
other change.

### Starting balance

A new account's starting balance is its balance **before the review's first text**, which a new
user can't know but the texts often can:

- A text that gives the balance after its transaction ("Balance £3,402.17", "Avl Bal INR
  45,000.00", "SD: 25.000.000VND", "New M-PESA balance is Ksh1,234.00", "الرصيد") gives `B` at
  that text.
- The starting balance is `B` minus the net of the review's transactions on that account up to
  and including that text, transfers to and from the other new accounts included.
- The latest such text is used: the account's balance after **Add all** is then right as of the
  latest alert even if earlier texts are missing, because the start absorbs the gap.
- Shown in the row, editable: "Balance before these texts: 4,312.10 (from the alert on 20 Nov:
  9,120.40)". With no balance in the texts: 0 and "Enter the balance before 1 Oct".
- **Credit cards** start at 0 owed, with "What you owed before 1 Oct": the bank's "balance" on a
  card is usually the available credit (the Visa's "Balance:SAR 50000.00" right after a payment),
  and a wrong guess would make the first budget wrong on its first day.

Where the balance comes from: the transaction gains an optional **`balance`**, the account's
balance the text gives after this money moved, as a plain decimal or null.

- Core `sms_import::Transaction`: `balance: Option<String>`, `#[serde(default)]`, checked like
  `writtenAmount`.
- Templates already read `{balance}` (`Reading.balance`); `fill` sets it.
- The assistant's schema gains `balance` (nullable), and one line in its instructions: the
  balance the text gives after this transaction, never a limit, available credit or points; null
  when the text gives none or it's a card's available credit.
- No other use of it in this design; the review ignores it outside setup.

### Two rule changes

- **No silent single account.** Today a budget with one open account takes every unknown card
  (`SmsAccountSource.Only`). With the review naming two or more numbers the budget doesn't know,
  `Only` no longer applies and they're asked in the setup section. `SmsAccounts.of` gets the
  batch's unknown reference keys; one or none keeps today's behaviour.
- **Start for an empty budget**, as in step 2.

### What's saved

The review's draft (`SmsBatch.saved`, version 2) gains an optional `setup` object: per candidate
key, the action, name, type, starting balance and what it belongs to. A draft without it reads
as today. The setup's suggestions are recomputed each time; only the user's answers are saved.

## Words on the page

- Empty budget: section title **Set up your accounts**, detail **"Your texts mention these cards
  and accounts. Create them, or say which belong together; you can change any later."**
- Budget with accounts: **Cards and accounts to set up**.
- Row: name; "Credit card · 43 transactions · Noon, Amazon, Jarir"; the balance line; the
  three choices.
- Button: **Set up 3 accounts**. Arabic strings alongside, as for every SMS string.

## Tests

- **Unit** (`SmsAnalysisTest`): candidates from a batch (cards, a transfer's other account, a
  sender with no card numbers); type and name from texts in five languages; *belongs to*
  suggested for a debit card with one current account, not with two; starting balance from a
  balance alert, with transfers between new accounts; credit cards start at 0; no `Only` with
  two unknown numbers; the draft keeps the setup answers.
- **Core and service**: `balance` filled by templates and by the model; an available credit
  isn't a balance.
- **Emulator** (`SmsImportUiTest`): an empty budget with synthetic texts for a current account,
  its debit card and a credit card; **Set up 3 accounts** makes two accounts (the debit card
  belonging to the current account) with the derived starting balance; **Add all** leaves each
  balance equal to the latest alert's.
- **Live** (`SmsLiveCheck`, `-e part onboarding`): the US, Indian and Saudi month profiles into
  empty budgets, the suggestions accepted as they are: which accounts were made, whether every
  text landed in the right one, and each account's balance against the profile's.

## The test against Primary

The acceptance test: what would a new user with the same bank texts get, against the budget built
by hand from statements?

1. On the Fold, an empty local-only budget **"Onboarding test"**, the same senders as Primary,
   collecting from the start of the period Primary's SMS-era covers.
2. The flow with every suggestion accepted, nothing fixed by hand (by the live runner, using the
   same functions the page uses, so the page and the test can't differ).
3. A script over the two budget files, for the period:
   - **Accounts**: made vs Primary's (by digits in the names): the same, merged (the debit card
     kept in its account), extra, missing.
   - **Transactions**: matched (account, amount, date within a day), missed (in Primary, not from
     the texts: cash, banks that don't text), extra (from the texts, not in Primary), and wrong
     amount or date.
   - **Transfers**: card payments and deposits paired as Primary pairs them.
   - **Payees and categories**: how often they agree with Primary's.
   - **Balances**: each account's at the period's end against Primary's.
4. The report says what a new user would have to fix; each gap is either fixed in the flow or
   explained (money no text reports).

The user's ANB texts go to the assistant once more for this, through their own connection;
Primary isn't touched, and the test budget goes to the Trash afterwards.

## Order of work

1. `balance` in the core, the templates and the assistant's output, with tests.
2. Candidates, suggestions and starting balances as pure functions, with unit tests.
3. The two rule changes.
4. The setup section, applying it, the draft; emulator test.
5. The live onboarding part; then the comparison against Primary.
6. README.

## Later, if the comparison asks for it

- The assistant describing each number it saw (type, product, owed or available, the card's
  limit, the account a debit card draws on), for the suggestions to start from.
- A credit card's owed balance from its limit and available credit when the texts give both.
