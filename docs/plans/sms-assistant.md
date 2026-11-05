# Bank texts: what the assistant reads, and how to make it better

Status: proposal, 2026-11-02. Goals: speed and quality of the reading; privacy isn't a goal
here, so the assistant gets whatever context makes it faster or better. Scope: the SMS analysis session in
`crates/urbudget-services/src/assistant/sms.rs`, its validation in
`crates/urbudget-core/src/sms_import.rs`, the provider requests in `assistant/openai.rs` and
`assistant/anthropic.rs`, and the Android side that sends texts and reads replies
(`SmsAnalysis.kt`, `ui/SmsAnalysisReview.kt`). The on-phone templates that would read known
formats are designed in `docs/plans/sms-templates.md`: the assistant writes a template for
each format as it reads, the phone reads known formats first, and the assistant reviews those
readings and reads only new formats in full. This document covers the assistant's side.

## What we saw on the Fold

A stress test on 2 Nov 2026 read the same ~205 texts from one bank several times with
ChatGPT gpt-6-astra:

- **It's slow.** A 200-text request took 2 to 2.5 minutes; 205 texts took about 5 minutes.
- **It's not consistent.** The same texts read twice gave different results. A "Card Payment" text
  with a card number and an amount came back as one transaction plus one unclear text the first
  time and two transactions the second. Four same-amount deposit placements on one day were
  grouped differently between runs.
- **A third of the work is junk.** Of 205 texts, 39 were one-time codes, 10 security
  alerts, 20 rewards and 1 offer: read, and only then skipped.
- **Payees are noisy.** Many came back as the bank's truncated merchant strings (a payment
  processor's prefix and a few letters, or a name cut off mid-word), and most rows carried "Check the payee" or "Check the date".
- **Some guesses need a question.** A large credit from a rental platform came back as plain
  income, with nothing marked unsure.
- **Real and separate isn't always obvious.** The four same-amount CD transfers are four
  deposits (each names a different deposit number), not repeats. The model got that right; a naive
  same-amount-same-day check would not.

## How it works today

### What the model receives

One user message with a JSON object:

| Key | Content |
|---|---|
| `budgetCurrency` | e.g. "SAR" |
| `today` | the phone's date |
| `accounts` | every open account's name and kind (no ids, no balances) |
| `categories` | every spending category's id, name and group (card-payment categories left out) |
| `texts` | `[number, sender, "YYYY-MM-DD HH:MM" received, body]`, numbered from 1 |

An optional second message carries the user's note ("Prepare again with a note"). The
instructions are in English with `{LANGUAGE}` filled for notes. Nothing else from the budget is
sent: no remembered cards, no past decisions for this sender, no payees, no transactions.

### What it can do

Two tools: `payee_history` (how the user categorised payees, following payee rules) and
`submit_sms_analysis`. The loop allows six model turns. Each turn resends the whole
conversation, including all 200 texts, because the OpenAI route uses `store: false`. So every
`payee_history` call costs a full extra pass over the batch.

The model can't see balances or transactions, can't choose accounts (the phone matches
`accountRef` and the user decides), can't group texts across two requests, and can't turn one
text into two items (each text is in exactly one item).

### What it returns

`items` (texts about money: `texts`, `disposition`, `note`, `quote`, `transaction`, `unsure`)
and `skipped` (text numbers under `code`, `security`, `balance`, `declined`, `rewards`,
`marketing`, `other`). The service maps numbers back to the phone's ids, turns the quote into
evidence, and the core validates exact money, dates, quoted references, categories and that
every text is placed exactly once. Any validation error is sent back and the model
**resubmits the whole reply**.

### Provider settings

- **OpenAI / ChatGPT:** tools are sent with `"strict": false`, `tool_choice: "auto"`,
  `parallel_tool_calls: true`, reasoning summaries on (`reasoning.summary: "auto"`) and encrypted
  reasoning included. No reasoning effort is set. The SMS session discards the streamed deltas,
  so the summaries are generated for nobody.
- **Anthropic:** `max_tokens: 32000`, adaptive thinking with summaries, `tool_choice: auto`.

## 1. Context: what's missing

| Add | Why | Cost |
|---|---|---|
| **The budget's templates**: each format with an example text and how it reads (see `sms-templates.md`) | The biggest lever on consistency: a format the budget knows reads the same every time, and a half-matching text is read the way its siblings were. | A few thousand tokens; stable, so it caches. |
| **Remembered cards**: `[{sender, ref, account}]` with the account's name and kind | The model knows a card's digits are the user's Visa, so "Card Payment … ####" is a repayment of that card or a purchase on it, not a guess. Transfers vs payments get easier. | Small. |
| **A payee directory**: the user's payees (up to ~300, most used first) with their usual category id, payee rules (`match → renameTo`), and the bank's raw names already mapped to them | Replaces most `payee_history` calls, each of which resends the whole batch. A cut-off merchant string maps to the user's payee the first time. | A few thousand tokens; caches. |
| **Recent transactions** per account the texts name (last 14 days: date, amount, payee, category) | Consistency with what the user already entered by hand or from a bank file: the same payee spelling and category, and the model can say "already in the budget" itself. | ~50 tokens a transaction. |
| **Account balances** | Texts carry "Balance : SAR …": the model can check its reading against the account and flag a missing or doubled transaction. | Trivial. |
| **Account kinds made explicit**: "credit", "cash", "savings", "tracking" | Already sent as `kind`; the prompt should say how to use them (a cash withdrawal to a cash account is a transfer). | None. |
| **The phone's time zone offset** | "On:02/11/26 07:47" and the received time never disagree by a day. | Trivial. |

## 2. Tools

Keep the toolset small: every tool call is a full extra pass over the batch.

- **Put the payee directory in the context** and keep `payee_history` only as a fallback for
  a payee the directory doesn't list. Return `categoryId` beside the category name in
  `payee_history`, since the submission needs ids and names can repeat across groups.
- **Don't add account lookups** (by digits, by name). The phone already does that and the user
  decides.
- **Don't add a "past texts' outcomes" tool.** Send the templates and payee directory in the
  context instead (section 1): the same information without a round trip.

## 3. Output schema

The item design (texts, disposition, quote, transaction, unsure) is right. Changes:

- **Add `balanceAfter`** (the balance the text reports, with its currency) and **`bankRef`**
  (a reference or transaction number the text gives). The balance lets the app check "the
  bank says 9,120.40 after this purchase; the budget says 9,120.40" and say where the two are
  in sync. The reference catches the same alert stored twice (SMS and MMS copies) and pairs
  both sides of a transfer. Both are copied from the text, so the core can check them the way
  it checks `accountRef`.
- **Add `time`** ("HH:MM" from the text, when it has one), to tell same-day, same-amount
  purchases apart without guessing.
- **Make `note` optional for transactions.** The review already shows payee, amount and kind;
  "Purchase at Noon" repeats them. Keep it required for `pending` and `unclear`, where it
  explains. This cuts output tokens noticeably on purchase-heavy batches.
- **Make `writtenAmount` and `currency` optional when they equal the amount in the budget's
  currency.** The core can fill them. Most Saudi texts are in SAR.
- **Don't add a confidence score.** Self-reported confidence is poorly calibrated. Keep
  `unsure` for fields, and measure reliability from agreement instead: a template's reading
  against the model's, or two runs against each other (see `sms-templates.md`).
- **The model writes a template for every format it reads**, in a `templates` array, and the
  review lane's `corrections` (both in `sms-templates.md`, sections 3 and 5). The phone keeps a
  template only when replaying it on the batch reproduces the model's own readings.
- **Allow one text to give two items** only if the eval shows a real need (a purchase with a
  separately charged fee in the same text). Today such a text has to be summed or marked
  unclear. Defer.

## 4. Determinism and consistency

Why the "Card Payment … ####" text flipped: nothing in the prompt says what a text with an
amount but no direction means. "Card Payment" can be paying with the card or paying the card.
The prompt allows `transaction` or `unclear`, and the model picked differently each time.
Changes:

1. **Turn on strict structured output** for `submit_sms_analysis` on OpenAI (`"strict": true`).
   The schema already has every property required, `additionalProperties: false` and nulls
   through `anyOf`, so it qualifies. That removes the "Use the exact submit_sms_analysis
   schema" retries entirely.
2. **Drop reasoning summaries for SMS requests.** Nobody reads them. Leave encrypted reasoning
   in, since it carries thinking across `payee_history` calls.
3. **Set the reasoning effort explicitly** (start with "low" for gpt-6-class models) and
   compare against the default on the eval set (section 7). This is likely the largest single
   latency lever on the current route. On Anthropic, cap thinking for SMS rather than
   adaptive.
4. **Rule for direction:** "If a text doesn't say whether money left or arrived, it's
   unclear: say what's missing in the note. Never pick a direction." Together with the
   decided formats from section 1, the next "Card Payment" reads the way the user settled it.
5. **Accept what's valid and ask only about the rest.** A 200-text reply with one text missing
   is resubmitted in full today, another 2 minutes. Instead, keep the valid items and send a
   short follow-up ("Add texts 12 and 47 in one more call"), then merge. The core still
   validates the merged whole.
6. **Never split a day across requests.** Cut a batch at the last full day before 200 texts,
   so both alerts of a transfer, or a hold and its settlement, are read together.

## 5. Speed and cost

- **Templates and the review lane** (`sms-templates.md`): the biggest speed win. A text in a
  known format costs the model nothing to confirm; only corrections and new formats are
  written out. Estimated 5× less output and a 3–5× shorter wait once formats are learned.
- **Skip junk on the phone, before sending**: trusted skip templates and a cold-start code
  filter take codes, logins and miles (about a third of texts) out of the request. The rule
  never drops a text with an amount and a direction.
- **Shorter replies**: optional `note` and `writtenAmount` (section 3) shrink the output on
  every item.
- **Cache the stable prefix.** Keep the instructions, the tool schemas, and the budget's
  accounts, categories, cards, payee directory and decided formats first, and the texts last.
  Move `today` out of the leading context into the text message, so the cached prefix survives
  a day change. OpenAI caches long identical prefixes automatically; on Anthropic, add
  `cache_control` to the system prompt and the context block.
- **Two parallel requests of 100** instead of one of 200 should roughly halve wall time. The
  service already allows four jobs. Check the ChatGPT plan's rate limits on the eval first.

## 6. Prompt text

### Contradictions and noise

- **"declined: a declined, failed or reversed attempt."** A reversed hold moves no money, but
  a reversed settled purchase is money back, which is a refund. Say so explicitly.
- **The salary example flags the date** ("the received date with 'date' in unsure"). Banks
  send these alerts within minutes, so this produces a "Check the date" chip on every
  undated text. Flag the date only when the text implies another day ("yesterday", a date
  that disagrees with the received date by more than a day).
- **Payee guidance produces "Check the payee" on nearly every row.** With the payee directory
  in the context the model can map the bank's string to the user's payee; when it can't, it
  keeps the string verbatim and doesn't mark it unsure, since the user can't do better from the
  text either. It marks the payee unsure only when it could be a person or a merchant and the
  kind depends on it. The user's edit is learned (the payee table), so the next text finds it.
- **"Put texts in one item only when they report the same event … or the same alert sent
  twice."** Say what "twice" looks like: the same words, minutes apart, or the same reference
  number.

### Missing rules

- **Deposits and financing (وديعة، مرابحة، CD, Murabaha, investment account).** Placing money
  in a deposit is a transfer to the user's own deposit account: `otherRef` is the deposit
  number, and each placement is its own item even when the amounts match. Profit paid at
  maturity is income ("Deposit profit"); the principal coming back is a transfer from the
  deposit.
- **Cash withdrawals.** A transfer to the cash account when the account list has one;
  otherwise an expense with memo "ATM".
- **Fees.** A fee charged in its own text is an expense to the bank. A fee mentioned beside a
  purchase is in the amount only when the text gives the billed total.
- **Incoming money from a platform or a person.** Rent, government and marketplace platforms
  (منصة ايجار, Absher), or a named person: income unless the account list shows it's the
  user's own account. Add "kind" to `unsure` when the text doesn't say what the money is for.
  That's the large rental-platform credit.
- **Hijri dates.** A date in the Hijri calendar (14xx) isn't converted: use the received date
  and add "date" to unsure.

### Examples

Keep the six worked examples, but replace the synthetic ANB ones with real formats filled with made-up values
once the templates exist, and add:

- a deposit placement;
- an ATM withdrawal;
- a reversed hold (skipped) next to a refund (transaction);
- an incoming transfer from a platform with "kind" in unsure;
- "Card Payment" with no direction, read as unclear.

Move bank-specific knowledge out of the global prompt and into the budget's decided formats,
so the prompt stays short and general.

### Length

About 2,000 tokens today, which is fine. With the directory and decided formats added, keep
the instructions themselves under ~2,500 tokens and let the cached context carry the rest.

## 7. Prioritised changes

Ranked by what they do for speed and quality of work:

| # | Change | Effort | Expected effect | How to check it |
|---|---|---|---|---|
| 1 | Templates written by the model, the review lane, and the junk lane (`sms-templates.md`) | L (phased) | 3–5× shorter waits once formats are learned; the same format reads the same way every time | Seconds and output tokens per 100 texts; run-to-run agreement on known formats (target 100%); corrections caught in seeded tests |
| 2 | OpenAI strict mode for `submit_sms_analysis`; no reasoning summaries for SMS; explicit reasoning effort | S | No schema retries; noticeably faster | Retries per request; seconds per 100 texts at each effort; field accuracy must not drop |
| 3 | Accept valid items and ask only for the missing or wrong ones | M | No full resubmission for one bad text | Retries and wall time on 200-text batches |
| 4 | Prompt fixes: direction rule, deposits/CDs, ATM, reversal vs decline, fees, platforms, Hijri, and less date/payee noise | S | Fewer wrong kinds; far fewer chips | Kind accuracy; chips per row; agreement across two runs on the ambiguous formats |
| 5 | Context: templates, remembered cards, payee directory (with learned raw names), recent transactions, balances; `categoryId` in `payee_history` | M | Better payees, categories and transfers; fewer tool round trips | Payee and category exact-match rate; tool calls per request; transfer accuracy |
| 6 | Cut batches at day boundaries; 2×100 in parallel for the read lane; 400–500 texts per request in the review lane | S–M | Fewer split transfers; about half the wall time for full reads | Grouping errors at boundaries; wall time; rate-limit errors |
| 7 | Schema: `balanceAfter`, `bankRef`, `time`; optional `note` and `writtenAmount` | M (core validation too) | Balance checks against the account; shorter replies | Output tokens per item; balance mismatches found |

### The eval set

Build it from the Fold:

1. On the phone, recompute each system text's fingerprint (`smsId`) and match it against the
   budget's `smsImportedMessageIds` (through `smsBankIds`) to pair a text with the
   transaction the user actually added: amount, date, account, payee and category as saved.
   Texts the user cleared or dismissed are labelled with that.
2. Export each text with its label to a local eval set (kept out of the public repo). For
   the review lane, add seeded mistakes (a wrong amount, date or kind in a filled row) to
   measure how many the model catches.
3. Run each candidate change twice on the same set and compare:
   - **Field accuracy**: amount exact, date, kind, `accountRef`, payee (normalised), category.
   - **Disposition agreement** with the label.
   - **Run-to-run agreement**: share of texts read the same both times.
   - **Chips per row**: `unsure` fields per transaction.
   - **Cost and latency**: input and output tokens, model turns and seconds per 100 texts.

A change ships when it improves its target metric without lowering field accuracy.
