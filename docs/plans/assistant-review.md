# The assistant: what it sees, what it can do, what it shows, and how to make it better

**Status:** proposal of 2026-11-10, being built on `claude/assistant-review` since the same day.
What's built so far is in [As built](#as-built), at the end; where that section and the plan
differ, that section is right.
**Goal:** an assistant that is safe with what it's given, behaves the same on the three apps, shows
cards whose state can be trusted, costs less to run, and can do more of what people ask it.
**Scope:** the chat assistant: the shared core in `crates/urbudget-services/src/assistant/`, and its
screens in `desktop/src/assistant/`, `ios/UrBudget/Screens/Assistant/` and
`android/app/src/main/java/app/urbudget/nativeapp/` (`AssistantController.kt`,
`BankTextsAssistant.kt`, `ui/AssistantScreen.kt`). The reading of bank texts into a review
(`assistant/sms.rs`) is covered by [sms-assistant.md](sms-assistant.md) and is touched here only
where chat reaches the same texts.
**Related:** [bank-texts-in-chat.md](bank-texts-in-chat.md), the Assistant entries in
[FOLLOW-UPS.md](../FOLLOW-UPS.md).

**How this was checked.** A reading of the code on `claude/foldable-polish` (a56482f, with that
branch's uncommitted Android edits). Nothing was run, on any app or against any model. Everything
in the core was read in full. On the apps, the findings this plan's first two phases rest on were
checked line by line; the rest come from one reading and are marked *(one reading)*, and so do the
app columns of the tables in section 1. Each of those needs a look on a device before it's fixed.
What's said about Anthropic's API (caching, kept thinking, cut-off replies) is from its published
rules, not from a request made here.

## 1. How it works today

One reply is a loop in `agent.rs`: send the conversation to the model, run the tools it calls on a
copy of the budget taken when the message was sent, send the results back, at most 12 times. Tools
only read or build a *proposal*. The user sees a proposal as a card and applies it; applying runs
the change again on the budget as it is then (`proposals::recheck`), and applies it only if it
still makes the same patch.

### What it's exposed to

| What | What the model gets | Where |
|---|---|---|
| Instructions | A fixed text, copied into the conversation when it starts and used for its whole life; an Arabic addition while the app is in Arabic | `prompt.rs:9`, `mod.rs:735`, `agent.rs:82` |
| A note with each message | Today, the budget's name and currency, the page, month, account and category on screen (when the app sends them), which app and its layout, up to 200 selected transactions as rows, the bank file being imported (its problem lines and samples), and what happened to earlier cards | `prompt.rs:143` |
| Look-ups it asks for | A month (every category and account balance), up to 200 transactions per search, payee history, reports, forecast, setup (accounts, groups, targets, rules, schedules, loans), bank sync, the app guide, a loan projection, rows of an attached sheet | `tools.rs:57` |
| Bank texts, Android only | Senders, counts, remembered cards, balances; up to 20 texts per search, 1,200 characters each | `BankTextsAssistant.kt:66-170` |
| Attachments | Images and PDFs whole, sent again with every request in that conversation; sheets and CSVs as a preview plus the rows it asks for; text files up to 50,000 characters | `attachments.rs`, `anthropic.rs:62-76`, `openai.rs:40-56` |
| Never | Keys, the sync passphrase, SimpleFIN's token, other budgets; memos when sharing them is off (it's on by default) | `mod.rs:118` |

It goes to OpenAI (ChatGPT plan or key) or Anthropic (key). Conversations and attachments are files
in the budget's folder. `urbudget --mcp` offers seven read tools to other apps.

What each app sends in the note differs:

| Field | Desktop | Android | iPhone |
|---|---|---|---|
| Page | the page | the page | always "assistant" (or "import") |
| Month | the month shown | the Budget tab's | the Budget tab's |
| Account | on Transactions only | the register's filter | never |
| Category | never | the register's filter | never |
| Selected transactions | from the register, any number (the core takes the first 200) | only from the assistant's own picker, 200 at most | from the register or the picker; over 200 fails at send |
| Bank texts | never | yes | never |

### What it can do

| Tool | The card it makes |
|---|---|
| `propose_budget_moves`, `propose_auto_assign` | Assign or move money in one month |
| `propose_transaction_edits` | Change amount, category, payee, memo, cleared, approval or flag; add payee rules |
| `propose_transactions` | Add transactions to one account, with splits |
| `propose_payee_changes` | Rename or merge payees; add rules |
| `prepare_import`, `fix_import` | A converted statement for the import review; fixes for the file on the Import page |
| `create_report`, `create_spreadsheet` | A report to preview and print; an Excel file |
| `propose_bank_texts` (Android) | Read waiting texts, add found texts to the review, remember a card's digits, set a credit limit |

What it can't do, from the tool schemas:

- Record a transfer between two accounts (`propose_transactions` has no transfer field; only import rows do).
- Change a transaction's date or account, or change the splits of one that's already split.
- Add, rename, move or hide a category or group, or set a target.
- Create, change, enter or skip a scheduled transaction.
- Delete anything, reconcile, write a note, set expected income (deleting is ruled out on purpose, `prompt.rs:60`).
- Open a page for the user. Cards carry a `highlight` (month, categories, transactions), which only the desktop uses.
- Ask a question with choices to tap. Questions are prose.
- Remember anything from one conversation to the next.

### What it shows

A conversation is a list of entries: the user's message, the assistant's text, a step line per tool
call, a card, an error, or "stopped". While a reply runs the core also sends text as it's written,
a summary of the model's thinking, and what it's doing now.

A card (`proposals.rs:251`) has a kicker and title, figures, rows (label, detail, amount, before →
after, a badge, a checkbox to leave the row out, a pencil to change its category or amount), a
total, a check line, a warning, a note and a problem. There are eight kinds and five states:

| Kind | Waiting | Afterwards |
|---|---|---|
| budget, transactions, payees | Apply (or "Apply 2 of 3"), Discard, put the assistant's version back | one line: Applied (with Undo for a while), Discarded, or Undone |
| fix | Apply to the import, Discard | one line |
| import | Review import, save the CSV, Discard | Applied once imported |
| report | Preview, print or save as PDF, Excel when it has tables, Discard | never applied |
| spreadsheet | Open, Save, Discard | never applied |
| bankTexts (Android) | one button for its action, Discard | Applied |

States: proposed, applied, discarded, undone, failed. There's no state for a card whose budget has
changed under it, or for one the assistant has since replaced.

How the three apps draw the same conversation:

| | Desktop | Android | iPhone |
|---|---|---|---|
| Text as it's written | formatted | plain text, raw Markdown | plain text, raw Markdown |
| How events arrive | pushed | polled every 300 ms | polled every 700 ms |
| "What it's doing now" | not shown | shown | not shown |
| Error buttons | chosen by the error's code | "Try again" for every code | "Try again" for every code |
| Card rows before paging | 8, then all | 20 a page | 20 a page |
| Starter prompts | send at once | send at once | fill the message field |
| Shows the card's change on the page | yes | no | no |
| "Ask the assistant" from a selection | yes | no | yes |
| "Ask the assistant" from a category | yes | no | yes |
| Notice about using the ChatGPT plan | yes | no | no |

### What should stay

Nothing changes without a card. A card is checked again on the budget as it is when applied. Rows
can be left out or changed before applying. A fix for an import goes only to a line that hasn't
changed since. A conversation's history is only ever added to. A report about a reply sends only
the text the user approved. The guide's labels are tested against each app's own text. The Arabic
catalog is complete on all three apps.

## 2. What's wrong

### A. Privacy and sending the wrong thing

1. **Chat can send bank texts under a permission given for another assistant.** Bank messages
   asks again when the provider or model changes (`smsConsented`, `SmsAnalysis.kt:677-680`: version
   2 and the same provider and model). Chat's check accepts any stored permission
   (`BankTextsAssistant.kt:46-47`). Someone who allowed ChatGPT and later switched to an Anthropic
   key has their texts searched by the new one without being asked.
2. **Searches send verification codes.** `search_bank_texts` returns each text's first 1,200
   characters with no filter (`BankTextsAssistant.kt:163`). The instructions tell the model not to
   repeat a code, but by then it has left the phone. The reading in Bank messages already settles
   codes on the phone (`sms_templates.rs:744-754`).
3. **Whole texts are kept in the conversation's file.** An "add" card stores each text's full body
   in what it does when applied (`BankTextsAssistant.kt:196`, kept by `phone.rs:95`), and it's
   returned with the conversation every time it's opened.
4. **The desktop sends the last request again.** The request that opened the panel is never cleared
   (`App.tsx:294`), and the panel forgets it took it when it closes (`Assistant.tsx:399`,
   `:592-600`). Reopening attaches the old selection, uploads the old file again and refills the old
   text, so the next question goes out with them.
5. **What gets sent is said in one place.** "What gets sent?" is on the desktop's ChatGPT sign-in
   only. Someone with an API key, or on a phone, never sees it in the chat. Memo sharing is on from
   the start.
6. **A file is saved the moment it's dropped or picked**, before Send, when signed out, and with
   "Keep conversations" off (`attachments.rs:94-105`). A removed file stays until the next
   session's clean-up (`mod.rs:355`).
7. *(one reading)* Photos under 3.5 MB go as they are, location data included. Links in a reply
   open on tap on iPhone and show only their text.

### B. The agent

1. **Anthropic requests aren't cached.** The request has no `cache_control` (`anthropic.rs:115-123`),
   so each of up to 12 rounds pays full price for the instructions, the tool schemas (19, or 22
   with bank texts) and the whole conversation, attached images and PDFs included. It's the user's
   own key.
2. **Three things change the start of an Anthropic conversation, which its newer models refuse.**
   Claude Opus 5.5 (the default here), Sonnet 5.5, Haiku 5.5 and Fable 5.1 tie each block of
   thinking to the instructions, the tool list and the messages before it. On an account opened
   since 31 August 2026, a request that sends such a block back after any of those changed gets a
   400. The messages are kept append-only for this (`history.rs:1-2`) and the instructions are
   fixed per conversation (`mod.rs:735`), but:
   - the last round adds a sentence to the instructions (`agent.rs:84-86`), so the twelfth round
     of a long reply is the one that's refused;
   - switching the app's language adds or removes the Arabic part (`prompt.rs:78-83`);
   - the tool list is today's, so it changes when bank texts are set up (`agent.rs:74`) and with
     any release that rewords a tool.

   Nothing catches the refusal; it would be shown as Anthropic's own message about a signature.
   Not run.
3. **Nothing bounds a conversation.** History only grows; tool results stay whole; attachments are
   sent every turn. Nothing counts tokens (neither provider's usage is read), nothing summarises,
   and running out of room would arrive as an unnamed error.
4. **A reply cut off at the length limit is taken as finished** (`anthropic.rs:230-235` checks only
   for a refusal). A tool call cut off the same way comes back to the model as "not valid JSON" and
   it tries the same thing again.
5. **The fixed instructions give desktop directions** ("the icon beside it in the sidebar",
   "Settings & data › Import › Bank sync", "the account's menu in the sidebar": `prompt.rs:36`,
   `:38`, `:60`). They're wrong on the phones and work against the rule to take directions from
   the guide.
6. **The user is shown messages written for the model.** A failed tool's step line is the tool's
   own error (`tools.rs:649-658`): "Couldn't prepare that change: No category has the id c_9f…. Use
   get_month or get_setup to find ids."
7. **The guide is found by English words** (`guide.rs:131-147`). A question passed on in Arabic
   matches nothing; the model gets the topic list and asks again, one round later.
8. **Stop throws away the text already shown.** Text is saved only when a round finishes
   (`agent.rs:119-134`, `mod.rs:911-921`); after Stop it's gone from the screen and from what the
   model is told next time.
9. **Every look at a conversation rewrites its file**, even to read it, under one lock for all
   conversations (`mod.rs:337-352`); listing conversations parses each file whole
   (`history.rs:116-129`).
10. **A message's limit is counted in bytes** (`mod.rs:705`), so an Arabic message is refused at
    about half the 20,000 characters the phones' field allows.
11. **Nothing measures a change.** The loop has no tests (`agent.rs`), and there's no eval set for
    chat as there is for bank texts (`examples/sms_review_eval.rs`). A report about a reply carries
    an excerpt and a reason, with no app, provider, model or tools, so it's hard to act on.

### C. Cards

1. **A finished card hides what it did.** Applied, discarded and undone cards fold to one line on
   every app, with no way to open them again.
2. **Discard is one tap and final.** It also throws away reports and spreadsheets, which aren't
   changes at all: they're modelled as proposals, so they get Discard and states that don't fit.
3. **A card doesn't know the budget moved.** It's found out only on Apply, which then refreshes the
   card instead of applying. Each app says so in a different
   place: in place of the card's warning (desktop), in a line under the header that stays across
   conversations (Android), with an error's icon (iPhone).
4. **A replaced card stays live.** When the assistant proposes a corrected version, the earlier one
   can still be applied, and both can.
5. **Undo can't be relied on.** It exists for the last apply only. The phones forget it on a new
   chat, another conversation or a restart, and keep showing the button after it can't work. The
   app's own Undo doesn't change the card *(desktop, iPhone: one reading)*.
6. **A failed card ends in "Dismiss".** Nothing offers a new version.
7. **Large proposals are slow to go through.** Every checkbox is a call to the core that disables
   every card until it returns. There's no select all or none. On Android, rows behind "+N more"
   can't be seen or left out *(one reading)*.
8. **A card is marked applied before the budget is saved** (`mod.rs:955-957`), and any app can set
   any state (`assistant_mark`). On the desktop a crash in between leaves "Applied" with nothing
   changed. A fix card is marked applied when only some of its fixes took *(one reading)*.
9. **Kinds are strings each app decides on.** A kind an app doesn't know gets a working Apply on
   the phones; the desktop's types don't list `bankTexts`.
10. *(one reading)* Bank-text cards apply without checking again: a limit for a deleted account
    shows Applied; a double tap runs twice.

The four card entries already in FOLLOW-UPS (a revision reaching a category the card doesn't show,
a deleted part failing the whole card, shared row keys, memo fixes with memos off) belong with
this group.

### D. The chat screens

1. **Errors on the phones ignore their code** (`AssistantScreen.kt:204-210`,
   `AssistantView.swift:271-281`). Signed out, over the plan's limit or on a model that's gone,
   the only button is "Try again", on every old error too.
2. **A reply on the phones arrives in lumps, as raw Markdown**, then jumps when the formatted
   version replaces it. iPhone's 700 ms poll (`HostEvents.swift:27`) undoes its own 64 ms batching.
3. **Closing the panel loses the draft**, the files and the selection on all three *(the phones:
   one reading)*. A request from elsewhere overwrites what was typed and lands in whichever
   conversation was open.
4. **The user can't see what goes with a message.** The sent bubble never shows that transactions
   were attached. The desktop says "950 selected transactions" and sends 200.
5. **Scrolling** *(one reading each)*: on Android a long reply can't be read from its start while
   it's written, and a conversation opens at its top; iPhone has no "jump to latest"; the desktop
   jumps to the end when a card is expanded.
6. **Not ready, not loaded and failed look alike** *(one reading)*. iPhone spins for ever if the
   status call fails; Android shows "Set up assistant" to someone who is set up, until the status
   loads; in a conversation, signed out is only a greyed Send.
7. **Leaving the app stops the reply's lookups.** Events are read only in the foreground, and the
   core waits 20 seconds for the phone (`phone.rs:14`).
8. *(one reading, not run)* On an Android phone, Back during "Fix with the assistant" leaves the
   app (`App.kt:344`).
9. **History** *(one reading)* is the 15 newest on the desktop with no title shown for the open
   one; nowhere can a conversation be renamed or searched. A title is the first 60 characters
   typed (`mod.rs:726`).
10. **Reading aloud and direction** *(one reading)*. The phones announce nothing when a reply or
    card arrives; the desktop's live region announces on every frame. Arabic inside an English
    screen, or the reverse, takes the screen's direction on the desktop and Android.
11. **Wording.** The plan notice says "Local Budget" (`Assistant.tsx:1066`). "Report" under a reply
    reads like the report feature. "Save as PDF" opens the print dialog.

## 3. The plan

Five phases, in the order they should land. S is under a day, M is two to four days, L is a week
or more; these are guesses from reading, not measured.

### Phase 0: stop sending or showing the wrong thing

Small, separate fixes. None changes a design.

| # | Change | Where | Size | How to check |
|---|---|---|---|---|
| 0.1 | Chat uses the same permission check as Bank messages; the sending notice says chat can search these texts | `BankTextsAssistant.available`, `sms_send_notice` | S | Allow one provider, switch, ask chat about a text: it must ask first |
| 0.2 | Leave verification codes out of search results, in the core as the phone's answer passes through, and say how many were left out | `phone.rs`, `looks_like_code` in `sms_templates.rs:756` | S | A search over a day with a code returns none; a unit test on `phone::outcome` |
| 0.3 | An "add" card keeps ids, and the phone fetches the texts when it's applied | `BankTextsAssistant.propose`/`apply` | S | The conversation's file holds no text bodies |
| 0.4 | Desktop: clear a request once taken; keep the draft, files and selection outside the panel, per budget | `App.tsx`, `Assistant.tsx` | S | Ask about a selection, close, reopen, ask something else: nothing rides along |
| 0.5 | One notice before the first message on every app and for every provider: what's sent, to whom, with the memo switch in it | all three, reusing the desktop's text | S | First send on a fresh install of each |
| 0.6 | Save an attachment when it's sent (hold it in a temporary place until then); strip location data from photos | `assistant_attach`, the three upload paths | M | Drop a file, don't send, quit: nothing under `assistant/files` |
| 0.7 | Phones: error buttons by code, as the desktop has them, on the last entry only | `AssistantScreen.kt`, `AssistantView.swift` | S | Sign out mid-conversation: the button signs in |
| 0.8 | A failed step shows a plain line; the tool's own error goes only to the model | `tools.rs:649-658` | S | Ask for a move to a category that doesn't exist |
| 0.9 | Stop keeps the text written so far, marked as cut short, and tells the model so next turn. It's added as a new entry, never by changing an earlier one (finding B.2) | `agent.rs`, `assistant_stop` | M | Stop mid-reply, reopen the conversation, and send again on an Anthropic key |
| 0.10 | "Local Budget" → UrBudget; "Report" → "Report a problem"; "Save as PDF" → "Print or save as PDF" | strings | S | |
| 0.11 | Android Back with the panel over an import, after reproducing it | `App.kt` | S | On the Fold, folded |

### Phase 1: one conversation, drawn three times

The apps have drifted because each decides for itself what an error or a card offers. Move those
decisions into what the core sends, then bring the screens level.

| # | Change | Size |
|---|---|---|
| 1.1 | The core's view of an entry says what it offers: an error carries its action (sign in, open settings, manage usage, try again, none); a card carries its actions and the tone of its message. The apps draw them | M |
| 1.2 | Show what it's doing now on the desktop and iPhone. Phones: read events every 100 ms while a reply runs, and format the text as it's written, off the main thread | M |
| 1.3 | Phones: the draft, files and selection live in the controller, per conversation. A request from elsewhere starts a new conversation and never overwrites a draft | M |
| 1.4 | A line above the message field on every app: "With this message: Budget · November · 12 transactions", each part removable; the same line on the sent bubble (the core's view already has room: `Entry.selected`). A selection over 200 is cut where it's made, with a sentence | M |
| 1.5 | iPhone sends the page, account and category it was opened from. Android gets "Ask the assistant" on a register selection and on a category | S |
| 1.6 | One scrolling rule: follow only when the user was at the end, a "Jump to latest" button otherwise, and open a conversation at its end | S each |
| 1.7 | Not ready, loading and failed each get their own banner with the one action that helps | S |
| 1.8 | While a reply runs, keep reading events when the app is left; if the system stops it, say "Paused when you left" with Continue | M |
| 1.9 | History: the title in the header, rename, search, every conversation reachable on the desktop | S |
| 1.10 | Reading aloud and direction: announce a finished reply and a new card once; each message takes its direction from its own text | M |

Done looks like the three tables in section 1 with the same answer in every column.

### Phase 2: cards that can be trusted

| # | Change | Size |
|---|---|---|
| 2.1 | Reports, spreadsheets and converted files stop being proposals. They're things the assistant made: Open, Save, Share, always there, with Remove in a menu. Only changes have Apply, Discard and a state | M |
| 2.2 | Two more states. *Changed*: the card records the budget's token when proposed; opening the conversation compares it, and the card says "Your budget changed. Update this card" before Apply is pressed. *Replaced*: a propose tool can name the card it corrects, and the earlier one folds to "Replaced by a newer version" | M |
| 2.3 | A finished card opens to show its rows as applied, and when. A discarded one has Restore | S |
| 2.4 | Apply is one step in the core on every app: the desktop moves to `assistant_apply_commit`, so a card is never applied without its save. Apps can set only discarded and restored. Undo on a card follows the budget's own history: shown while its change is the latest, and the card changes when the app's Undo reverses it | M |
| 2.5 | A failed card offers "Ask for a new version", which sends the reason | S |
| 2.6 | Ticks are instant and sent together when Apply is pressed. Select all, none, and "only the ones to check". Fix the row keys from FOLLOW-UPS here | M |
| 2.7 | One place for a card's message, inside it, in three tones: updated, check this, can't apply | S |
| 2.8 | A kind the app doesn't know is drawn without buttons, with "Update UrBudget to use this" | S |
| 2.9 | A fix card says how many fixes took and lists the ones that didn't; bank-text cards check again before applying | S |
| 2.10 | "Show in the budget" on a card: the phones use `highlight` to open the register on those transactions or the month on those categories | M |

### Phase 3: the agent

| # | Change | Size | How to check |
|---|---|---|---|
| 3.1 | Keep the start of an Anthropic request the same for a conversation's whole life. "Answer now" on the last round and the language become messages added at the end (a `system` message inside `messages` where the model takes one, a reminder in the user's turn where it doesn't). The conversation records whether it began with bank texts, and keeps that tool list. When Anthropic still refuses a block of thinking, take the thinking out of what's sent and ask once more | M | Anthropic's own three-step check: send with `prefix_mismatch_behavior: "drop_block"` and the `thinking-binding-controls-2026-08-01` header, and log `input_transformations`; it stays empty across a 12-round reply, a language switch and bank texts being set up |
| 3.2 | Cache Anthropic requests: `cache_control: {"type": "ephemeral"}` at the top of the request. It only pays once 3.1 holds, since any change at the start empties the cache | S | `usage.cache_read_input_tokens` above zero from the second round |
| 3.3 | Read usage from both providers and keep a total per conversation; show it in the conversation's menu. No prices on the ChatGPT plan | M | A three-round reply's total matches the provider's own figure |
| 3.4 | When a conversation nears the model's room, offer "Continue in a new conversation": the assistant writes what matters into a fresh one. The same offer when a provider says it's full | M | A conversation with twelve attachments and long searches |
| 3.5 | Handle a reply cut off at its length limit on both providers: don't run a cut-off tool call, ask the model to do it in parts, and show "Cut short" with Continue | S | A proposal of 500 edits with a low limit set in a test |
| 3.6 | Instructions, for conversations started from then on: take the desktop directions out (the guide has them per app), and move the bank sync and bank texts sections into those tools' descriptions, so they're read only when the tools are offered. After 3.1, because a reworded tool reaches old conversations too | S | The prompt test for "sidebar" on Android and iPhone |
| 3.7 | The guide tool's description lists the topic ids, so the model asks by id whatever the user's language | S | Arabic "how do I reconcile" answers in one round |
| 3.8 | Notes for the assistant: a short list per budget the user can read and edit in the assistant's settings, added through a card ("Remember: never take from Holiday fund") and sent with the note | M | A new conversation follows a rule set in an earlier one |
| 3.9 | Save a conversation only when it changed; keep a small index for the list; one lock per conversation | M | Opening history with 200 conversations |
| 3.10 | Count a message's limit in characters | S | A 15,000-character Arabic message sends |
| 3.11 | A stand-in provider for tests of the loop (stop, error, the 12-round limit, the phone not answering), and the eval set below | M | |
| 3.12 | A report about a reply may carry, shown to the user before sending: app and version, provider and model, the tools the reply used, the language. Thumbs up and down under a reply, kept on the device, to pick examples for the eval set | S, plus the privacy policy | |

### Phase 4: more it can do

Each is a tool, a card and a guide entry. In the order people are likely to ask:

| # | Ability | Size |
|---|---|---|
| 4.1 | Transfers in `propose_transactions`; date and account in `propose_transaction_edits` | M |
| 4.2 | Questions with choices: an `ask_user` tool whose options are buttons (an account, a category, yes or no) | M |
| 4.3 | Categories, groups and targets: add, rename, move, hide, set or remove a target | M |
| 4.4 | Splits on a transaction that exists, with every part given | M |
| 4.5 | Schedules: create, change, pause, enter or skip what's due | M |
| 4.6 | Answers as cards: a short table or bar for "what did I spend on…", with "Open in Reports" and "Open in the register" (needs 2.10) | M |
| 4.7 | Next steps under a reply, from rules not the model: overspent categories → "Cover them", uncategorized → "Categorize 14" | S |
| 4.8 | The same starting points on the phones as the desktop has: after an import, "Categorize N"; on an overspent month, "Cover with the assistant". Nothing is sent until the user sends it | S |

| 4.9 | Deleting through a card: only transactions the user selected and attached, on a red card that lists each one, as one undo step | M |

Deleting comes last. It's the one change people can't check from a card's rows after the fact, so
it waits for cards that open after they're applied (2.3) and an Undo that holds (2.4).

### The eval set

Before phase 3, so its changes can be compared:

1. Forty tasks on the sample budget and an Arabic copy of it: cover overspending, categorize
   twenty imports with three that should be marked to check, add a receipt with a split, rename
   and merge payees, a month in review, a statement to import, ten "how do I…" questions per app,
   five that should be refused or handed to the app (delete, connect a bank, investment advice),
   and five with instructions planted in a memo or a file.
2. For each: the tools called, whether the card's patch equals the expected one, ids that don't
   exist, rounds, tokens in and out, seconds. For directions: every bold label is in the guide for
   that app. Saying a change was made without a card is a failure.
3. Run each provider's default model twice and compare the two runs.

## 4. Decisions

Taken on 10 November 2026:

1. **Notes for the assistant (3.8): yes.** They live in the budget, so they sync with it and go
   to the provider with every message.
2. **Reports as things made, not proposals (2.1): yes.** Old conversations are drawn the new way
   from their kind; the stored files are left alone.
3. **More in a report about a reply (3.12): yes,** each item shown and removable in the form, and
   the privacy page says so.
4. **Deleting through a card (4.9): yes,** after the card work.
5. **iPhone:** written alongside the others and left for CI to build; nothing of it is compiled
   or run here.
6. **Checks against a real model:** a few short replies on the ChatGPT sign-in. Nothing is tried
   against Anthropic, so 3.1 and 3.2 are checked by tests of the requests they build, not by
   Anthropic accepting them.

## As built

**Phase 0, the core** (10 November 2026; `cargo test -p urbudget-services --lib assistant`, 108
passing):

- 0.2: a search's one-time codes are taken out in `phone.rs` (`without_codes`, with
  `looks_like_code`) before the model sees them, and the answer says how many. The filter is
  conservative on purpose, so a code it doesn't recognise still goes.
- 0.6: a chosen file waits in `assistant/staged` and joins `assistant/files` when its message is
  sent (`Files::stage`, `adopt`, `discard`; `assistant_detach` for a file taken off). What an
  earlier session chose and never sent goes at the first new attachment. A JPEG is kept without its
  Exif, XMP and IPTC but still says which way up it is; a PNG loses its text and Exif chunks. WebP
  and GIF go as they are.
- 0.7, 1.1: an error's `action` (`history::error_action`: sign in, settings, usage, retry or
  none) comes with the live event and with the conversation's last entry.
- 0.8: a failed step shows the tool's reason only when the engine wrote it for people
  (`tools::for_user`); what a tool says about its arguments goes to the model alone.
- 0.9: Stop keeps the round's text so far as an entry before "stopped" (`RunningTask.written`),
  and the next message's note tells the model its reply was cut short.
- 0.5: `noticeDue` in the status and `told` in a settings change, per service. The apps show the
  notice; the core doesn't refuse a message sent without it.
