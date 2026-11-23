# First budget: a setup walkthrough, and what keeps a new budget going

Status: proposed 2026-11-24. Nothing here is built. It follows two reviews of the first-run
experience made that day; "What the reviews found" says what was run and what was only read.

## What it's for

A new person should finish their first sitting with a real budget: their accounts in, every
unit of money given a job, and a way chosen to keep it up to date. Today they reach an empty
grid of categories at 0.00 and a checklist that ticks itself off, and most of the help is on
the website where no app links to it.

Three things are built, in this order of value:

1. **A setup walkthrough**: five short screens after **Create my budget**, skippable at any
   point, that end with a funded budget.
2. **A "to do" strip** on Budget that says what needs doing now. It replaces the five-step
   checklist and is what a returning person sees first.
3. **Hints at the moment they matter** (first overspending, first income, first new month),
   and an opt-in weekly reminder on phones.

Small fixes that don't depend on any of this are listed first, so they can ship alone.

## What "budgeting properly" means here

The plan measures itself against five things a person has to reach, in order:

| | Milestone | Reached when |
| --- | --- | --- |
| 1 | Real money is in | Every account they spend from exists with today's balance, cards included |
| 2 | All of it has a job | Unassigned is 0.00 |
| 3 | Spending gets in | Transactions arrive within days of happening, by a route they chose |
| 4 | The plan bends | An overspent category was covered from another |
| 5 | They come back | Income was assigned when it arrived, and a second month was funded |

## What the reviews found

Run for real: the desktop app, from an empty data folder, at its default window size
(1000×662), through a first account, assigning, a target, a transaction and an overspend.
Read in code only: Android, iPhone, the website. The iPhone app has not been compiled for
this work.

**Milestone 1.**
- Desktop first launch offers SAR and 2026-10-15 to everyone: `FirstRunWelcome` builds its
  blank budget with `BUILT_IN_CURRENCY`, and `localeDefaults()` only runs on a budget that
  already exists (`App.tsx`, the `isUntouched` effect). Seen on a US-English PC.
- The first screens differ on each app: desktop asks four format questions, iPhone asks for a
  name and three formats, Android for a name and a currency.
- Desktop opens one account form and never asks for a second account or a card.
- Android's Budget tab on a new budget says "Start your plan / Create categories…". The
  account form is only reachable from the Accounts tab, and nothing points there.

**Milestone 2.**
- A new budget is 16 categories at 0.00 with nothing to go on.
- The checklist's **Assign money** button (and the phones' **Assign to a category**) opens
  with the whole of Unassigned prefilled, to go into one category.
- "Assign your money" ticks when any assignment is non-zero
  (`budget.assignments.some((a) => a.amount !== 0)`). It showed done with 1,400 unassigned.
- Quick assign › Underfunded fills a budget in one step, but needs targets, and the checklist
  puts targets two steps later. The target editor has four kinds and fifteen repeat choices.
- On desktop at the default window size the checklist leaves room for about two category
  rows, and the Quick assign panel its text points to "on the right" is `display: none`
  until a category is selected.

**Milestone 3.**
- "Record your spending" ticks after one transaction. Nobody is asked how spending will keep
  arriving.
- Every route has a condition the apps don't state up front: file import needs an account
  first; bank connections are SimpleFIN, US and Canada, paid to SimpleFIN (the phones say
  neither the countries nor that it costs); Android bank texts need a connected assistant
  (a ChatGPT plan or an API key), SMS access and two consents; a budget on two devices needs
  the sync subscription or a backup file carried across.

**Milestone 4.**
- An overspend on desktop moved the checklist on to "Plan for a bigger bill". The only sign
  was an "Overspent 1" tab, with the row scrolled out of view. The phones show a card for it.
- Covering it, once found, is good: the amount is prefilled and it takes two taps.

**Milestone 5.**
- No app reminds anyone of anything. Android's one notification is for bank texts.
- Nothing is shown when a month starts or when income lands in Unassigned.
- The website has `/getting-started/` (with a weekly routine) and guides on credit cards,
  overspending, sinking funds, irregular income and budgeting by paycheck. No app has a help
  entry or a link to any of them.
- The assistant can assign money, create categories and set targets, but can't create
  accounts, offers no setup prompt on an empty budget, and needs a paid plan or key.

**Also.**
- The phones' sample budget is one account, three categories and one purchase, with 3,200
  unassigned and nothing overspent (`sample_budget` in `crates/urbudget-android/src/lib.rs`).
  The desktop's (`demo.ts`) has three accounts including a card, ten categories and an
  overspent one.
- Android creates a bare budget: no starter categories, no checklist.
- Settings › Advanced and the assistant's instructions say "Ready to Assign"; everything else
  says "Unassigned".
- `web/public/download/index.html`: the phones-and-Linux answer says there is no Linux
  version and then that there is one, and the page never says what to do after installing.

Not known: whether these are where people actually stop. The apps count first opens and
weeks in use, not steps. The admin page's weekly cohorts (`app_usage`, `kind = 'week'` by
`cohort`) can compare copies first opened before and after each release, with no new counts.

## What exists and is reused

| Piece | Where |
| --- | --- |
| Category sets (Everyday basics, Keep it simple, Family, Student, Self-employed) | `crates/urbudget-core/src/budget_templates.rs`, being built on `claude/budget-templates` (uncommitted on 2026-11-24). This plan needs it merged first. |
| A monthly target as a plain amount | `Category.target`; shows as "Set aside each period", monthly |
| Filling a budget from its targets | Quick assign › Underfunded: `underfunded_plan` in `workflows.rs`, shown first with `auto_assign_preview`. Overspending first, then targets due soonest, then card payments; never more than Unassigned holds |
| Expected income, compared with the month's targets | `expectedIncome` on the budget; "Targets are X more than expected income" |
| Device formats for a new budget | `localeDefaults()` (desktop), `StarterBudget.localFormats` (iPhone), the system locale's currency (Android) |
| The sample budget and its banner | `demo.ts`, `SampleBanner` on all three |
| "Set up from the welcome screen" marker | `lb:setup:<id>` (desktop), `setup.<key>` (iPhone); Android has none |
| Counts the strip needs | The month's summary: Unassigned, overspent categories, uncategorized spending; unapproved transactions per account |
| Covering overspending | The Move money sheet, opened as "Cover overspending" |
| Notifications on Android | `POST_NOTIFICATIONS` is declared and WorkManager is in use (`SmsWorker`) |

## Small fixes first

Each stands alone and none waits for the walkthrough.

1. **Desktop first launch uses the device's currency and formats.** `FirstRunWelcome` starts
   from `localeDefaults()`; the sample it opens does too. `Welcome.test.tsx` pins `"SAR"`
   and changes with it.
2. **A "How this works" link in every app** to `urbudget.app/getting-started/`: on the
   welcome screen, in Settings, and on the setup card (below). Opened as the apps open the
   privacy policy today.
3. **Stop offering the whole balance to one category.** The checklist's and banner's assign
   action opens with the amount empty, or (phones) goes to the first category's keypad.
4. **Phones' sample budget matches the desktop's.** One sample, built in the engine, with a
   card, several groups and one overspent category.
5. **Android's empty Budget tab leads with the account.** "Add your first account" with its
   button, as the Accounts tab's empty state has it. (The walkthrough replaces this for new
   budgets; it stays for budgets made without it.)
6. **Download page.** Fix the Linux answer, and end each platform's steps with "Open
   UrBudget and choose Create my budget" and a link to Getting started.
7. **One word.** "Unassigned" in Settings › Advanced and in the assistant's instructions.

## The walkthrough

### Where it starts

The first screen is the same on all three apps:

- The headline and one line.
- **Create my budget**: the one filled button. No name, currency or format questions: the
  name is "My budget" and the rest comes from the device. Under the button, one line shows
  the guess, "US dollars · 10/15/2026", with **Change**.
- Two quiet links: **Look around a sample** and **I already have a budget** (YNAB, a backup
  file, sign in and sync, behind that one row).

Account and Sync leave the desktop's first screen; **I already have a budget** leads to them.

**Create my budget** opens the walkthrough. **Skip setup**, on every step, makes the budget
from whatever has been answered so far and opens it.

### The five steps

Each is one screen with one question and one filled button.

**1. Where is your money?**
- Rows of name, type and today's balance, with **Add another**. Types offered here:
  Checking, Savings, Cash, Credit card. The full list stays in the normal account form.
- A card's row asks "What you owe" and takes a plain number; it's stored as the negative
  balance the engine expects, and its payment category is made.
- Under the rows: "You have 3,700 to plan with." (the cash accounts' total).
- Next needs one account with a name.

**2. What do you spend on?**
- A category set, default Everyday basics, with its categories as chips. Tapping a chip
  leaves that category out; **Add your own** adds one.
- "No categories" is not offered here: Skip setup does that.

**3. What does a month cost?**
- One amount per category: exact for bills, a guess for the rest, blank if unsure.
- A row can be switched to **a year**: 900 a year is saved as 75 a month.
- Each amount is saved as that category's monthly target. A blank leaves no target.
- Under the rows: "Your month costs 3,365. You have 3,700."
- Optional line, folded: **What comes in each month?** Saved as expected income, so the
  budget can say whether the month fits what they earn.

**4. Give your money its jobs.**
- Shows what Underfunded would assign, by group, and what is left.
- The button **Assign it** creates the budget and applies it.
- Three endings, each one sentence:
  - Left over: "335 is left over. Put it in Emergency fund, or keep it for next month."
    with a button for the first.
  - Exactly enough: "All of it has a job."
  - Short: "Your month costs 420 more than you have right now. The rest waits for payday;
    assign it when it arrives."
- With a card owing money: "420 is set aside to pay your card." when it was funded.

**5. How will you keep it up to date?**
- One choice, listing only what works on this device, each with its condition said:
  - **Type it in as I spend.** On phones: "Remind me every Sunday" (on by default, asks for
    notification permission on Next).
  - **Import my bank's file.** "About once a week, from your bank's website." Links the
    site's import guide.
  - **Connect my bank.** "US and Canada, through SimpleFIN, a service you pay for."
  - **Read my bank's texts.** Android only. "Needs an assistant connected."
- The choice is kept per budget on the device and decides what the strip offers. Nothing is
  imported or connected inside the walkthrough.
- **Finish** opens the budget.

### What's kept, and when

- Nothing is saved to the library until **Assign it** or **Skip setup**, so a first launch
  still makes no placeholder budget.
- The answers so far are kept on the device (as a draft, not a budget). Opening the app
  again offers **Continue setup** in place of **Create my budget**.
- A budget made by skipping shows a **setup card** on Budget: "Finish setting up · 2 steps
  left" with **Continue**. It reopens the walkthrough at the first step the budget doesn't
  already answer (no accounts → 1; no targets → 3; money unassigned → 4), filled in from the
  budget. Hiding it is one tap, as the checklist is hidden today.
- The five-step checklist is removed. The setup card and the strip do its work.

### The engine

One function builds the budget, so the three apps can't differ:

`setup::build(answers) -> Budget` in `crates/urbudget-core`, exposed as a desktop command and
as `setupBudget` to Android and iPhone (as `budgetTemplates` is being exposed).

- `answers`: name, currency and formats; accounts (name, type, balance); the template id
  with the categories left out and the ones added; an amount and a period (month or year)
  per category; expected income.
- It makes the accounts and each card's payment category (today each app makes that
  category in its own code: `App.tsx`, `BudgetViewModel.kt`, `BudgetStore.swift`), applies
  the template, sets the targets, and sets expected income.
- It does not assign. Step 4 previews with `auto_assign_preview` and applies Underfunded
  through the normal edit, so **Undo** takes it back like any other change.
- Reopened on an existing budget, the walkthrough edits that budget through the normal
  edits and doesn't call `build`.

## The "to do" strip

One strip at the top of Budget, shown only when it has something to say, with up to three
items. Each item is a count and one action:

| Item | Shown when | Action |
| --- | --- | --- |
| "3,700 to assign" | Unassigned is above 0 | Opens Quick assign › Underfunded's preview when targets ask for money, else the assign keypad |
| "1 overspent" | A category's Available is below 0 | Opens Cover overspending for the first one |
| "12 to categorize" | Spending without a category, or imports waiting for approval | Opens them |
| "December: fund it from November" | First opening in a month with nothing assigned and a previous month that had assignments | Opens the preview of "Assigned last month" |
| "Nothing entered for 8 days" | The kept route is typing or file import, and the newest transaction is over a week old | Add a transaction, or the import page |

- It replaces the desktop's needs-a-category banner and the phones' overspent card: one
  place instead of three.
- With nothing to do it isn't there.

## Hints at the moment

Each is shown once per budget, as one sentence with one button, in the strip's place:

- **First overspending.** "Groceries is 150 over. Move it from a category that has some
  left: changing the plan is part of the plan." **Cover it**
- **First income after setup.** "5,000 arrived. Give it jobs before you spend it." **Assign**
- **First new month.** "It's December. Last month's plan is a good start." **Fund December**
- **First card purchase.** "Paid by card: the money moved to your card's payment category,
  ready for the bill." **How cards work** (the site's guide)

## The weekly reminder (phones)

- Chosen in step 5, changed in Settings. Off unless chosen.
- One local notification a week, on the chosen day at 18:00: "A few minutes with your
  budget: enter what you spent and check what's left." It opens Budget.
- Not sent in a week where a transaction was added in the last three days.
- Android: a channel "Budget reminders" and a weekly `PeriodicWorkRequest`. iPhone: a
  repeating `UNCalendarNotificationTrigger`; this is the app's first use of
  UserNotifications.
- Local only. Nothing about it is sent anywhere, so the stores' privacy answers don't change.

## Importing after a typed balance

Typing today's balance and then importing this month's file counts the same spending twice;
the site's bank-sync page warns of it and the file import has no date cut. So:

- The walkthrough stamps each account it makes with the day its balance was typed
  (`balanceDate`, an extra field on the account).
- The import review leaves rows dated on or before that day unticked by default, under one
  line: "14 rows are from before 24 Nov, already in the balance you typed." with **Add them
  anyway**.
- Bank connections' "First import from" defaults to the day after.

This is the one engine change outside `setup`. Until it's built, step 5's import option says
"Import from tomorrow's transactions on."

## Words on the page

English first; Arabic follows `docs/LOCALIZATION.md` and its glossary, in each app's catalog
and in the engine's `i18n/arabic.rs` for anything the engine writes. Step titles as above.
Buttons: **Create my budget**, **Next**, **Back**, **Skip setup**, **Assign it**, **Finish**,
**Continue setup**. No screen has a second filled button.

## Tests

- **Engine** (`setup` module): accounts and a card's payment category; a template with
  categories left out and added; a yearly amount saved as its monthly target; blanks leave
  no target; expected income; the same answers give the same budget in English and Arabic
  apart from names.
- **Desktop** (vitest): the first screen makes nothing until Assign it or Skip setup; skip
  at each step makes the budget that step implies; the draft comes back after a restart;
  the setup card opens the right step; the strip's items against budgets in each state;
  each hint shows once.
- **Desktop, in the window**: the whole walkthrough at 1000×662 and at a wide size, each
  screen scrolled to its end, in light and dark, English and Arabic, through the QA harness
  on a scratch data folder.
- **Android**: unit tests for the strip's rules and the reminder's "not this week" rule; an
  emulator test from the welcome screen to a funded budget.
- **iPhone**: unit tests for the same rules. The app must be compiled and run on a Mac
  before release; it can't be on this PC.
- **Import guard**: rows on or before `balanceDate` start unticked; an account without the
  field behaves as today.

## Order of work

1. **Small fixes** (1–7 above). Ships by itself.
2. **Engine `setup`**, after `claude/budget-templates` merges.
3. **Desktop**: the first screen, the walkthrough, the setup card; the checklist removed.
   Desktop goes first because it can be run and checked here.
4. **The strip and the hints**, desktop.
5. **Android**: first screen, walkthrough, setup card, strip, hints.
6. **iPhone**: the same.
7. **The weekly reminder** on both phones, and **the import guard**.
8. **The site**: `/getting-started/` rewritten around the walkthrough's five steps; the
   assistant's `guide.md` gains a "set up a budget" entry.

Steps 3–4 and 5–6 can each be released when done; nothing later is needed for an earlier
step to be useful.

## Decisions wanted

1. **The walkthrough is the default after Create my budget**, with Skip setup on every
   screen (recommended), or it's offered beside "Start with an empty budget".
2. **The five-step checklist goes** in favour of the setup card and the strip (recommended),
   or it stays for budgets made by skipping.
3. **The reminder's default**: on, Sunday 18:00, when "Type it in" is chosen (recommended),
   or off until turned on in Settings.
4. **The desktop's New budget dialog** keeps its current form (recommended), or offers the
   walkthrough too.

## Left out, for now

- Setting up by talking to the assistant. It needs a paid plan or key, so it can't be the
  default; it could be an extra entry once the assistant can create accounts.
- Importing or connecting a bank inside the walkthrough.
- Counting how far people get through setup. It would need the privacy policy and both
  stores' data answers changed; the weekly cohorts answer the main question without it.
- Tasks to try in the sample budget.
