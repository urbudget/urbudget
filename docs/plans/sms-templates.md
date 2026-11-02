# Bank texts: templates written by the assistant, reviewed by the assistant

**Status:** phases 1 and 2 implemented (templates written, kept and used for the review lane); the junk lane (3) and payee table, accuracy display and drift (4) are next.
**Goals:** speed and quality of the reading. The assistant stays in the loop for every text about money.
**Baseline:** branch `claude/sms-redesign-on-main` at `fce72b9`, 2 November 2026.
**Scope:** Android bank messages (`SmsInbox.kt`, `SmsAnalysis.kt`, `SmsWorker.kt`, services `assistant/sms.rs`, core `sms_import.rs`).

Today every collected text goes to the assistant: verification codes, login notices and
miles included. It's slow (about 2–2.5 minutes per 200 texts on the test phone), and it isn't
consistent: the same texts read twice on 2 November gave different answers (one card's texts
came back as one transaction plus one unclear text, then as two transactions).

Bank texts are machine-written. In this plan the assistant writes a template for each format
as it reads, and from then on the phone reads each text by its template before the assistant
sees it. The assistant reviews what the phone read and answers only with what's wrong; texts
in formats nobody has seen yet, and texts a template only half explains, it reads in full.
Junk (codes, logins, miles) that a proven template recognises is settled on the phone.

## 1. What the texts look like

Measured on the test Fold (Galaxy Z Fold 8) from the `anb` sender: **2,931 texts** (73 SMS,
2,858 text-only MMS copied over from an iPhone), 13 Dec 2024 to 2 Nov 2026, with a gap from
July to December 2025. 1,690 are Arabic, 1,241 English. Bodies were read locally with
read-only content queries and reduced to masked skeletons and counts.

A **skeleton** keeps a text's fixed words and turns everything that varies into a typed
slot. Almost every anb text is a stack of `Label : value` lines, so labels stay literal and
values become slots (`{AMOUNT}`, `{REF}`, `{DATETIME}`, `{PAYEE}`, `{CODE}`, `{TEXT}`).

| | Texts | Share | Skeletons | Concentration |
|---|---:|---:|---:|---|
| Money (purchases, transfers, deposits, payments) | 1,608 | 55% | 89 | skeletons with 3+ texts cover 97% |
| Security (logins, sessions) | 535 | 18% | 8 | top 3 cover 88% |
| Verification codes | 383 | 13% | 21 | top 10 cover 95% |
| Rewards (miles) | 338 | 12% | 30 | top 3 cover 79% |
| Other | 67 | 2% | 20 | top 3 cover 57% |
| **All** | **2,931** | | **167** | top 10: 53%, top 20: 71%, top 50: 90% |

The tail is short: 49 skeletons with 10 or more texts cover 89.6%, 87 with 3 or more cover
96.4%, and 55 texts (1.9%) are one of a kind. Since January 2026 money texts use 47
skeletons; the top 20 cover 91% of them.

Some of the commonest, masked:

| Texts | Kind | Skeleton |
|---:|---|---|
| 348 | security | `إشعار : ⏎ تم تسجيل الدخول لتطبيق anb موبايل بواسطة الدخول السريع` |
| 246 | purchase | `Online Purchase ⏎ Apple Pay ⏎ VISA Card : {REF} ⏎ Amount : SAR {AMOUNT} ⏎ Balance : SAR {BALANCE} ⏎ At : {PAYEE} ⏎ On : {DD}/{MM}/{YY} {HH}:{MI}` |
| 179 | rewards | `The Earned miles has been credited to your Al Fursan Membership No {N} ⏎ Number of Earned miles as per daily spend : {N} ⏎ VISA Credit Card` |
| 131 | purchase | `شراء-POS ⏎ بـ : SAR {AMOUNT} ⏎ من : {PAYEE} ⏎ مدى أثير : {REF} ⏎ في : {DD}-{MM}-{YY} {HH}:{MI}` |
| 94 | code | `لـ : {PURPOSE} ⏎ لا تشارك الرمز ⏎ رمز : {CODE} ⏎ مبلغ : SAR {AMOUNT} ⏎ السبب : {TEXT}` |
| 47 | foreign purchase | `International Online Purchase ⏎ VISA Card : {REF} ⏎ Country : {TEXT} ⏎ Amount : {CUR} {FOREIGN} ⏎ Exchange rate : {RATE} ⏎ Fees and VAT : SAR {FEES} ⏎ Balance : SAR {BALANCE} ⏎ Total amount : SAR {AMOUNT} ⏎ At : {PAYEE} ⏎ On : {DATETIME}` |
| 20 | transfer | `عزيزي عميل anb، ⏎ تم خصم مبلغ SAR {AMOUNT} ⏎ من حسابك رقم {REF} ⏎ الى حسابك الخاص بالمرابحة رقم {OTHER_REF} ⏎ في تاريخ {DD}-{MM}-{YY} {HH}:{MI}` |

What a template can rely on:

- **Fields.** Every money text has an amount and a date with time; 86% name a payee after
  a label (`At`, `من`, `لدى`, `المستفيد`); 53% carry the balance after it; card and
  account references come in a handful of shapes (`VISA Card : ####` 574 times,
  `مدى أثير : ####` / `*####`, transfers `####***` and `***####`, accounts `####xx##`);
  6% are in a foreign currency, where the budget's amount is `Total amount`, not `Amount`.
- **Dates.** `dd/mm/yy` (866), `dd-mm-yy` (most Arabic formats), `dd-mm-yyyy` (301) and a
  few year-first. One Arabic incoming-transfer format writes `yy-mm-dd`, so the same
  `##-##-##` shape means different things in different formats. Within one skeleton the
  order never changed, and the text's date equalled the day the phone received it in every
  case checked (131 of 131 for the commonest Arabic purchase format).
- **Amounts.** `SAR 12.50` dominates; `SAR12.50` (no space), `12.50 SAR`, `12 ريال`,
  `USD 12.50` and grouped `SAR 1,234.50` also occur.
- **Drift.** The bank rewrote its formats between mid-2025 and 2026: 104 skeletons before
  July 2025, 91 since January 2026, only 28 in both, and only 26% of 2026 texts use a
  format that existed before July 2025. Examples: `Al Fursan Infinite Card : ####` with
  `Date :` became `VISA Card : ####` with `On :`; `مدى أثير : *####` lost its star; Arabic
  purchases moved the payee from `من :` to `لدى :`. Formats do change, so templates must
  expect to be replaced.
- **Continuity.** 58 of the 73 recent SMS (79%) match a skeleton first seen in the
  iPhone-era MMS: a phone's history teaches the formats its new texts use.

### How well the assistant's answers line up with the text

From the trial budget's current review (32 transactions read by the assistant on
2 November):

| Field | Lines up with the text |
|---|---|
| Amount | 30 copied verbatim, 2 with grouping added (`1234.00` for `1,234.00`) |
| Card ref | 32 of 32 found verbatim (the core already requires it) |
| Date | 32 of 32 equal the received day |
| Payee | 12 identical to the label's value, 1 differing only in case, 10 a trimmed part of it (branch suffixes dropped), 9 had no payee label (transfers, payments) |

So amount, reference and date are copies of spans, and the kind is constant for a format.
Only the payee needs learning beyond the format: the assistant cleans `AMAZON MKTPLACE`
style values into the name the user uses, often from `payee_history`.

### How much would be read on the phone

Replaying the 2,931 texts in order, and trusting a skeleton once the assistant has read
it *k* times:

| Trust after | All texts read on the phone | Last 300 texts |
|---:|---:|---:|
| k = 1 | 94% | 95% |
| k = 3 | 88% | 89% |
| k = 5 | 83% | 88% |
| k = 10 | 73% | 83% |

A cold start on a 30-day window (208 texts from 3 October) reaches 57% within that month at
k = 3, and the history behind it makes later months look like the right-hand column. This
is an upper bound: it counts matches, not correct readings, and section 4 is what earns it.

## 2. The flow

The assistant stays in the loop for every text about money. Templates make its work smaller
and its readings consistent; they never replace it.

```
 texts waiting
      │
      ▼
 pre-assistant analyzer (phone)
      │
      ├── junk lane ───── a trusted skip template (code, login, miles, offer, declined,
      │                   balance notice) matches: settled on the phone, counted, not sent
      │
      ├── review lane ─── a money template matches: the phone fills the item; the assistant
      │                   reviews it and answers only with what's wrong
      │
      └── read lane ───── no template, or one only half matches: the assistant reads it in
                          full, as today, and writes a template for each new format
```

1. **Reading writes templates.** When the assistant reads a text in full, it also writes the
   template for that text's format. The phone checks each template against the assistant's
   own readings of the batch before keeping it (section 3).
2. **The next run goes through the analyzer first.** Each waiting text is matched against
   the sender's templates and goes to one of three lanes (section 4).
3. **The assistant reviews what the phone read.** Review texts go to it with the phone's
   reading beside each one. It confirms by saying nothing and corrects by naming the text and
   the field (section 5). Read-lane texts in the same request get the full treatment.
4. **Corrections teach.** A correction fixes that row, counts against its template, and makes
   the assistant rewrite the template in the same reply (section 6).

The junk lane is the only place a text never reaches the assistant, so it's the only lane
that needs earned trust. A money template can be used from its first match, because the
assistant reviews every row it fills.

## 3. Templates written by the assistant

### The template language

A template is a short list of lines with literal words and typed slots. It's not a regular
expression: the phone compiles it into one, so the assistant can't write a pattern that
backtracks forever or quietly matches everything.

```json
{
  "example": 12,
  "lines": [
    "Online Purchase",
    "?Apple Pay",
    "VISA Card : {ref}",
    "Amount : {currency} {amount}",
    "?Balance : SAR {balance}",
    "At : {payee}",
    "On : {date:dd/mm/yy} {time}"
  ],
  "outcome": {"disposition": "transaction", "kind": "expense", "sign": "-", "memo": "Apple Pay"},
  "payee": {"from": "payee", "clean": "Title case; drop branch numbers and a trailing city"}
}
```

| Slot | Matches | Becomes |
|---|---|---|
| `{amount}` | a number with optional grouping and up to 3 decimals | `writtenAmount`, and `amount` signed by the outcome when `{currency}` is the budget's |
| `{total}` | as `{amount}` | the budget-currency amount of a foreign purchase (`Total amount`) |
| `{currency}` | `SAR`, `USD`, `ريال`, `ر.س` … | `currency` (normalised to a code) |
| `{date:ORDER}` | `dd/mm/yy`, `dd-mm-yyyy`, `yy-mm-dd` … as declared | `date` |
| `{time}` | `HH:MM` | the item's time |
| `{ref}`, `{other}` | digits with optional `*`, `x` masks | `accountRef`, `otherRef` |
| `{payee}` | the rest of the line | the raw payee, cleaned (below) |
| `{balance}` | as `{amount}` | the balance after, kept for checks |
| `{code}`, `{n}`, `{any}` | a code, a number, any text | ignored |

A line starting with `?` is optional. The outcome is either money (`disposition`, `kind`,
`sign`, optional `memo`) or `{"skip": "code"}` and its peers. `payee.clean` is a short rule
for turning the raw value into the user's payee name. Payee names the user already has are
resolved through the payee table (section 7), not the rule.

Normalising before matching: Arabic-Indic digits to Western, direction marks and isolates
removed, `\r\n` to `\n`, runs of spaces collapsed, blank lines dropped, spaces around `:`
made optional.

### How a template is written

`submit_sms_analysis` gets a `templates` array. For every text it reads in full, the
assistant either names the template it wrote for that format (`"template": 3` on the item or
skip entry) or writes a new one. One template covers every text in the batch with the same
format. The prompt asks for templates for skip formats too: codes, logins and miles are most
of the junk lane.

### How the phone checks it

Before keeping a template, the phone replays it on every text in the batch:

- it must match its `example` text;
- every text it matches must have been read by the assistant to the same outcome, and to the
  same amount, currency, date, refs and kind;
- the payee it produces is compared, but a difference only records `payee` as weak for that
  template;
- a template that matches a text the assistant read differently is dropped, and the reply's
  `templateErrors` count says so. The batch's items are still used.

A kept template starts as **candidate** for money (usable in the review lane at once) and
**candidate** for skips (not yet used to reject; section 6).

## 4. The pre-assistant analyzer

It runs on the phone in Rust core (`sms_templates.rs`, called from `SmsReadings.read` before
any request) and takes milliseconds for thousands of texts. For each waiting text:

1. **Normalise** it and find the sender's templates.
2. **Match.** Templates are tried most specific first (most literal lines). One match is a
   match. Two equally specific matches send the text to the read lane, and that's recorded
   as a template conflict.
3. **Lane:**
   - a **trusted skip** template → **junk lane**: settled on the phone with its reason, counted
     in "Checked 11:38 · 39 codes · 20 miles";
   - a **money** template, trusted or not → **review lane**, with the item it fills;
   - a candidate skip template, or no template, or a **half match** (all lines but one) →
     **read lane**.
4. **Checks on every filled item:** the date is the received day ±1; the amount parses
   exactly; refs are spans of the text; the result passes `validate_analysis`. A failure
   sends the text to the read lane and counts against the template.

A cold-start code filter (a code phrase such as `OTP`, `رمز`, `لا تشارك الرمز` **and** a 4–8
digit code on a code line) puts codes in the junk lane before any template exists. The anb
code formats all have both. It's there for speed: codes are 13% of texts and never need
reading.

## 5. The review lane

### The request

One request carries both lanes:

```json
{
  "budgetCurrency": "SAR", "today": "2026-11-02",
  "accounts": [...], "categories": [...], "cards": [...], "payees": [...],
  "review": [
    [1, "anb", "2026-11-01 13:05", "<text>", {"template": "t7", "kind": "expense",
      "amount": "-25.50", "currency": "SAR", "date": "2026-11-01", "payee": "Al Noor Flowers",
      "accountRef": "1234", "categoryId": "flowers"}]
  ],
  "read": [
    [2, "anb", "2026-11-01 16:40", "<text>"]
  ]
}
```

The review entries carry the text itself, so the assistant can check the reading against
it, and the template id, so a correction lands on the template too.

### The reply

```json
{
  "corrections": [
    {"text": 1, "fields": {"payee": "Al Noor Flowers & Gifts"}, "why": "the user's payee"},
    {"text": 9, "disposition": "unclear", "why": "doesn't say which way the money went"}
  ],
  "templates": [ ... new and rewritten templates ... ],
  "items": [ ... full items for the read lane ... ],
  "skipped": { ... read-lane texts only ... }
}
```

A review text not named in `corrections` is confirmed as the phone read it. A correction may
change any field, the disposition, or move the text to `skipped`. When a correction shows
the template is wrong (not just one odd text), the assistant rewrites the template in
`templates` with the same id; the phone replays the rewrite on the batch like any new
template.

### Why it's faster

Today's cost is mostly output: every money text is written out in full (around 60–100
tokens each), and 200 of them take 2–2.5 minutes. In the review lane a confirmed text costs
no output at all; only corrections are written. Input grows a little (each review text
carries its reading), and input is cheap and fast next to output.

Estimated on the Fold's mix once formats are learned (88% of texts templated, 31% junk):
of 200 texts, about 60 are rejected on the phone, about 115 are reviewed, and about 25 are
read in full. Output falls from ~200 entries to ~25 items plus a handful of corrections:
**roughly 5× less output and a 3–5× shorter wait.** Because review output is small, the
review lane can take larger batches (400–500 texts per request), so a phone's history needs
fewer requests too. These are estimates; phase 2 measures them.

### Validation

The merged analysis (phone items, corrected, plus read-lane items plus junk) still goes
through `validate_analysis` before it's saved: every text accounted for once, money and
dates exact, refs and quotes in the text. A reply that misses or repeats a text is handled
as today.

## 6. Reliability

Reliability is measured from what the assistant and the user do with each template's rows.

**Per template, per version**, the phone keeps:

| Count | Meaning |
|---|---|
| `filled` | rows it filled |
| `confirmed` | rows the assistant confirmed unchanged |
| `corrected[field]` | rows the assistant corrected, by field |
| `edited[field]` | rows the user changed in the editor before adding, by field |
| `requests` | separate requests its rows were reviewed in |

**Accuracy** for a field is `1 − (corrected + edited) / filled`. **Money accuracy** is the
lowest of amount, date, kind, refs and outcome; **payee accuracy** is tracked apart, since
payee cleaning is the one thing a format can't fix.

**States**

| State | Review lane | Junk lane (skip templates) | Enters when |
|---|---|---|---|
| candidate | yes | no, its texts are read in full | written and replayed cleanly |
| trusted | yes | yes, its texts are rejected on the phone | 3+ texts confirmed over 2+ requests, no money correction |
| revised | yes (new version) | no until trusted again | the assistant rewrote it; counts restart |
| retired | no | no | unused 180 days, or rewritten 3 times in a month |

Money templates don't need to be trusted to be used, because every row is reviewed. Trust
matters for skips: they can only reject texts once the assistant has agreed with them
several times. A trusted skip template is still audited: one text in 50 goes to the read
lane, so a template that started rejecting money would be caught.

**Drift.** Per sender, the share of texts that go to the read lane is tracked. A jump (more
than 5 of the last 20, where it ran under 1 in 20) or half matches mean the bank changed its
wording. Nothing breaks: new formats are read and written as templates, and old ones retire.
The bank's 2025→2026 rewrite on the Fold is the case this handles.

**What the user sees**

- The status line: "Read 180 texts · 152 by template, reviewed by ChatGPT · 28 read in full".
- On each row, a small "From a template" note in its details, with the template's accuracy.
- Settings, per sender: "anb · 31 formats · 98% confirmed in review", the formats listed with
  an example text, their counts and accuracy, and **Forget**.

## 7. Storage

New tables in `sms-inbox.sqlite3`, per budget and sender:

```sql
CREATE TABLE sms_templates (
  budget TEXT NOT NULL, sender_key TEXT NOT NULL, id TEXT NOT NULL, version INTEGER NOT NULL,
  body TEXT NOT NULL,        -- the template JSON as the assistant wrote it
  example TEXT NOT NULL,     -- one text it was written from, for review context and the settings list
  state TEXT NOT NULL, stats TEXT NOT NULL, first_seen INTEGER, last_seen INTEGER,
  PRIMARY KEY (budget, sender_key, id));
CREATE TABLE sms_payees (    -- the payee as the bank writes it, to the user's payee and category
  budget TEXT NOT NULL, sender_key TEXT NOT NULL, raw TEXT NOT NULL,
  payee TEXT NOT NULL, category_id TEXT, seen INTEGER NOT NULL,
  PRIMARY KEY (budget, sender_key, raw));
```

- `sms_payees` learns from every confirmed or corrected row and every edit the user makes in
  the review, so a merchant's name and category come out right the next time without asking.
- The templates also go to the assistant as context in every request (a format list with
  their examples), so a read-lane text in a known-but-half-matching format is read the way
  its siblings were.
- **Stop and clear messages**, removing a sender, and deleting a budget delete their rows.
  **Forget** in settings deletes one template.
- Syncing templates between phones is possible later; relearning costs about one request
  per format, so it isn't urgent.

## 8. Rollout

| Phase | What lands | Size |
|---|---|---|
| 1 | Template language and replay check in core; `templates` in the reply schema and the prompt; `sms_templates` table. Templates are written and stored but not yet used: the assistant reads everything as today | M (4–5 days) |
| 2 | The analyzer and the review lane: money templates fill rows, the assistant reviews and corrects, rewrites templates; corrections and edits counted; the status line | M–L (5–7 days) |
| 3 | The junk lane: trusted skip templates and the cold-start code filter reject on the phone; audits | S (2 days) |
| 4 | `sms_payees`; template accuracy on rows and in settings; drift tracking and retirement | M (3–4 days) |

Phase 1 already pays off: the assistant's templates for a phone's history are written while
it reads that history, so phase 2 starts with templates for most formats.

**Tests**

- **Golden fixtures** from the masked formats in section 1 with synthetic values: each
  template compiles, matches its examples, fills the expected item, and rejects its
  neighbours (`Apple Pay` present or not, `Online` vs `PoS`).
- **Replay check:** a template that disagrees with the assistant's reading of any matched
  text is dropped; one that agrees is kept.
- **Review protocol:** a fake model that confirms everything, one that corrects a field, one
  that rewrites a template, and one that moves a text to `skipped`; the merged analysis
  passes `validate_analysis` in each case.
- **Junk lane:** a candidate skip template never rejects; a trusted one does; 1 in 50 is
  audited; a money text never matches a code template's lines.
- **Separate texts stay separate:** four same-amount, same-day transfers with different
  other refs are four items.
- **Performance:** 10,000 texts against 300 templates under 200 ms on the emulator.

## 9. Risks and open questions

- **The assistant teaches and grades.** It writes templates and reviews their rows, so a
  wrong habit could confirm itself. The replay check (a template must agree with the
  assistant's independent full readings) and the user's edits counting as corrections are
  the guards; tracking accuracy per template shows where they fail.
- **Review attention.** Reviewing 115 prefilled rows, a model may confirm too readily.
  Measure it in phase 2 by seeding known-wrong fills on the eval set and counting how many
  are caught; if too few, review in smaller groups or ask for an explicit verdict per row.
- **Payee cleaning** is a rule per template plus the payee table; new merchants still come
  back "Check the payee" until the user settles them once.
- **Other banks** that write prose rather than `Label : value` lines need `{any}` and
  literal fragments more; the language handles it, but coverage is unmeasured.
- **Open:** should a correction rewrite the template immediately, or only after two
  corrections agree? Should trusted money templates skip review for texts the user always
  confirms unchanged? (Today's answer: no, the assistant stays in the loop.)
