# Holdings in Reports

**Status:** phases 1-7 built on 18 November 2026 (uncommitted), at the owner's "begin implementing all phases", with the Balance sheet grouped by asset type at the owner's direction (section 5.5). Checked: Rust 899 tests, desktop 898 tests and type check, Android 383 unit tests and lint; phases 1-6 seen in the real desktop app and on the Android emulator against a stand-in broker, phase 7 seen in the desktop app with a synthetic Trades section. Not checked: the iPhone code (never compiled); the Flex Trades attribute names against a real statement; the assistant's new tool with a live model; Android's phase 7 screens, Arabic and a Fold on the new reports. Known limits: the Balance sheet's start column fills only from the first month-end snapshot after this build; an older build's sync drops an entry's snapshots; an account with commissions but no income rows has no By account row; Decisions 2 and 5 (investing shown apart from spending; the desktop's duplicate stats) remain open.
**Goals:** make Reports answer the questions an investor asks of a budget: how the investments did, how much of net worth growth came from markets rather than saving, what the money is in, and what it earned and cost.
**Baseline:** branch `claude/ibkr-investments` (uncommitted working tree), 18 November 2026.
**Scope:** engine (`crates/urbudget-core`: `reports.rs`, `investment_stats.rs`, `commands.rs`), the assistant's tools, and Reports on the desktop, Android and iPhone.

Today holdings and Reports don't meet. An investment account's page (Holdings) shows one account at a time. Reports see that account only as a balance: it is in Net worth, the Balance sheet and Balance over time, and nowhere else. Nothing shows the portfolio as a whole, and nothing separates what markets did from what was saved.

## 1. What exists today

Verified in the code on the baseline; file references are to it.

**Reports**
- The engine's `reports::calculate` returns, per month: income, spending, assets, debt, net worth and every account's month-end balance (`reports.rs` 19–34). It knows nothing about account types or holdings.
- An investment account's generated lines (`Starting value`, monthly `Market change`, `Money moved in or out`; `investments.rs` 561–678) are ordinary lines in a tracking account. So they are **in** net worth and balances, and **out of** income, spending, payees, drill-downs, the forecast and cash age (`reports.rs` 178–203). Reports never read the `investment` marker.
- Twelve reports in three groups on all three apps: Spending (4), Income (3), Wealth (net worth, balance sheet, balance over time, forecast, cash age). The Balance sheet already has an "Investments" section (accounts typed investment or retirement), decided separately in each app.
- Filters are a month range and accounts grouped Budget / Tracking / Closed. There is no filter by account type.
- The forecast is the one report with its own data fetch, special-cased in all three apps. It is the pattern for a report that needs more than `Report`.
- A transfer from a budget account to an investment account reads as **spending** in its category (or as uncategorised spending), by the engine's cross-boundary rule (`ledger.rs` 33–38).

**Holdings data** (`investments.rs` 177–229)

| Kept over time | Kept as today's snapshot only | Not kept at all |
|---|---|---|
| Account value per day (`values`): daily for 400 days, month-ends before that | Positions: symbol, name, kind, currency, quantity, price, value, cost, unrealised gain; ISIN, conid, exchange (IBKR only) | A holding's value, quantity or price on any past day |
| Money in and out per day (`flows`, IBKR only; other sources use the budget's own transfers) | Cash per currency, accrued income | Trades, realised gains, lots |
| Each deposit, withdrawal and transfer (`movements`, IBKR only) | One exchange rate, on the prices' date | Dividends per holding |
| Dividends, interest, tax, fees per month, per account (IBKR only) | Trade commissions, as one total per statement | Sector, country, bond maturity, option expiry |

- `investmentPage` (`investment_stats.rs` 418) computes one account per call: gain and Modified Dietz return per period, swings, gain by month, allocation by kind and currency, the holdings rows, income and costs. Nothing adds up across accounts. The phones use it; the desktop computes the same figures again in `investmentStats.ts`.
- Every source (IBKR, positions file, bank connection, typed value) produces `Market change` lines; only IBKR has broker-recorded flows, monthly income and `Money moved` lines.

## 2. What is worth adding, and what isn't

Ranked by how much it tells the owner against what it costs.

| # | Addition | Why it helps | Data it needs | Cost |
|---|---|---|---|---|
| 1 | Net worth: split each month's change into **markets** and **everything else** | The most asked question of a net worth chart is "did I save that or did the market do it?" Today a good market month and a good saving month look the same | Already stored (the `change` lines) | Small |
| 2 | **Performance** report: the whole portfolio's value, money put in, gain and return over the report's range, by month and by account | The Holdings page answers this for one account and five fixed periods. With two brokers there is no combined figure anywhere | Already stored | Medium |
| 3 | **Allocation** report: what all the accounts hold together, by kind, account and currency, with the largest holdings merged across accounts | Concentration and cash drag only show across accounts: the same fund at two brokers, cash sitting idle in one | Already stored (today's snapshot) | Medium |
| 4 | **Income and costs** report: dividends, interest, tax, fees by month across accounts, with a yield | Shows what the portfolio pays and what it costs to hold, month by month rather than as one twelve-month line | Stored for IBKR only | Small to medium |
| 5 | Assistant: the same figures through a tool, and in the spreadsheet export | "How did my investments do this year?" is a natural question; today the assistant can only list accounts | From 2–4 | Small |
| 6 | Keep a month-end allocation snapshot at each sync | Unlocks allocation **over time** from then on; without it that view can never exist | New, about 200 bytes a month per account | Small |
| 7 | Read the Flex Trades section into monthly totals | Realised gains and commissions by month; the owner's query already sends Trades and the reader drops it | New parse | Medium |
| 8 | Keep dividends by holding | Yield per holding, "what pays me" | New aggregation | Medium |
| 9 | Show money moved into investments apart from spending | A 200,000 transfer to the broker reads as spending in its category and dwarfs real spending in the Spending reports | Already stored | Medium, and it changes long-standing figures (decision 2) |

Not proposed:
- **Benchmarks** (against an index) and **sector or country** breakdowns. Both need market reference data the app doesn't have, and fetching it would send holdings' identifiers to a third party from a local-first app.
- **Per-holding return over a period** and **past allocation before item 6 ships.** The data was never kept and no source can backfill it through what the app reads today.
- **Counting market gains as income.** The budget's income stays money that arrived to be assigned.

## 3. Principles

1. **Reconcile with Net worth to the cent.** Every monthly figure comes from the same ledger Net worth reads: an account's month-end value is its ledger balance, its gain is its `Market change` lines, and money moved is the balance's change less the gain. A report that disagreed with Net worth by rounding would not be trusted.
2. **One calculation, in the engine.** The desktop already has a second copy of the holdings stats in TypeScript. The new figures get one implementation and a read command; the three apps only lay them out.
3. **Say only what the data supports.** Allocation is "today's holdings", dated by the oldest prices in it. Income says which accounts report none. Estimated days and missing statement sections carry through as notes, as on the Holdings page.
4. **Nothing to show, nothing shown.** The group appears only once an account has holdings.
5. **No stored data changes in phases 1–5.** They read what is already in the budget, so there is no migration, no sync change and nothing for an older build to misread.

## 4. Where it goes

A fourth group, **Investments**, beside Spending, Income and Wealth, with three reports: **Performance**, **Allocation**, **Income and costs**. It is hidden while no account has holdings.

Why a group rather than more Wealth reports: Wealth already has five, each answering a net-worth question. Three more would make it eight and bury the forecast and cash age. The alternative (one long "Investments" report in Wealth) repeats the Holdings page's everything-on-one-page shape, which suits one account but not a filterable report.

Net worth (Wealth) gains the market split in place; it is not a new report.

## 5. The reports

Each uses the existing report scaffolding (tiles, panels, tables, the shared charts) and the existing range and account filters. The account filter lists only accounts with holdings, as the forecast's lists only budget accounts.

### 5.1 Net worth: where the change came from

- "Change each month" becomes two stacked series: **Markets** and **Everything else** (saving, debt paid down, revalued assets).
- A fifth tile: **From markets**, the range's total and its share of the change.
- Shown only when some month has a market figure, so a budget without investments sees no difference.

### 5.2 Performance

| Part | Content |
|---|---|
| Tiles | Value now · Gain over the range · Return · Money put in |
| Value | Columns by month, stacked by account (six, then "Other"), under a line of money put in (the starting value plus what was added since). The gap between the columns and the line is the gain |
| Gain each month | Columns up and down, the sum of the accounts' market changes |
| By account | Account, how it's kept up to date, start, put in, gain, return, end. A row opens that account's Holdings page |
| Notes | Days counted as money moved by estimate, stale prices, and accounts typed as investments that nothing fills (with the way to set one up) |

Return is Modified Dietz over the whole portfolio (section 6). Over more than twelve months the tile also gives the yearly rate.

### 5.3 Allocation

| Part | Content |
|---|---|
| Tiles | Holdings · Largest holding's share · Top five's share · Cash's share |
| What it's in | A ring with a switch: by kind, by account, by currency; the ranked list beside it |
| Largest holdings | Merged across accounts: holding, kind, which accounts, value, share, unrealised gain and return on cost. Ten shown, the rest behind "Show all" |
| Notes | "Prices from" per account, flagged when a few days old; holdings a source gave no kind for are grouped as "Holdings" |

No range picker: it is today's snapshot, and says so. Once item 6 has been collecting, a fourth panel "Over time" stacks kinds by month.

### 5.4 Income and costs

| Part | Content |
|---|---|
| Tiles | Dividends and interest · Tax, fees and commissions · Net · Yield (income over the average value) |
| By month | Stacked columns: dividends and interest up, tax and fees down |
| By account | The same totals per account |
| Notes | Which accounts report no income (everything but IBKR today); commissions are one figure per statement until item 7 |

### 5.5 Balance sheet, by what the money is in

The owner's direction (18 November 2026): the Balance sheet groups by **asset type, not by account**, so it answers "how much do I have in cash, in stocks, in bonds, in property". Accounts become the detail under each type.

Today its sections are account types, and an investment account is one line under "Investments" however mixed its contents (`Wealth.tsx` 186–401). The new statement keeps the same shape (sections, lines, start, end, change, share of assets) with these sections:

| Section | What goes in it |
|---|---|
| Cash and bank | Bank and cash accounts, as today, **plus the cash each investment account holds** ("Cash at <account>") |
| Stocks, Funds, Bonds, Options, Futures, Currencies, Crypto, Commodities, CFDs | Each investment account's holdings of that kind, one line per account ("<account>"). A section shows only when something is in it |
| Other investments | Holdings a source gave no kind for; an investment account nothing breaks down (its whole balance); and what an account's balance has moved since its prices ("Since prices from <date>") |
| Property, Vehicles, Other assets, Accounts in credit | By account type, as today |
| Receivables | As today, plus each investment account's accrued income ("Accrued income at <account>") |

Liabilities are unchanged: they are still grouped by account type.

Rules:
- **An account is split only on the asset side.** One whose balance is negative stays whole under liabilities, as today.
- **Lines sum to the account.** An account's lines across all sections add up to its ledger balance at that date; the difference from its prices is the "Since prices" line.
- **Totals don't move.** Total assets, total liabilities and net worth are the same figures as before and as in Net worth. A short option, or money borrowed from the broker, is a negative line in its section (Options, Cash and bank) rather than a liability, so each account still counts once, on one side.
- **"Cash on hand"** is the Cash and bank section, so it now includes cash at brokers.
- **Start and change need a snapshot.** The end column is split from the positions in hand. The start column is split only where a month-end snapshot exists for that month (collected from the first sync on a build with this work). Where it doesn't, a split line's start and change read "—", its section's total start and change read "—" too, and the grand totals still show (they come from the accounts). A note names the accounts concerned.
- **A range that ends before the latest prices** and has no snapshot for its last month can't be split: those accounts go whole under Other investments, with a note.
- **One sentence under the assets** carries the explanation that used to be per account: "Investments changed by X: Y put in, Z from markets."
- The "What you own" bars follow the new sections.

### 5.6 Elsewhere

- **Phones' Reports overview:** an Investments card (value, gain over six months) when there are holdings.
- **Export:** each report exports its own CSV: months with value, put in and gain per account; the merged holdings; income by month.

## 6. The figures, exactly

**Which accounts.** Those with an `investments` entry that the account filter includes. An account typed investment or retirement with no entry has a balance but no known gain: it is named in the notes and left out of the sums.

**When an account joins.** On its first valued day. Its value that day counts as money put in, so starting to track an existing account never reads as a gain. Before that day it contributes nothing to this report (it is still in Net worth).

**By month, per account** (from the ledger, so principle 1 holds):
- `value` = the account's balance at month end.
- `gain` = the sum of its `investment: "change"` lines dated in the month.
- `put` = this month's `value` less last month's, less `gain`. This covers the budget's own transfers and the `Money moved` lines alike.

**Over the range, portfolio and per account:**
- `start` = value at the end of the month before the range (0 for an account that joins inside it); `end` = value at the range's end, or at the latest prices if that is earlier.
- `gain` = the sum of the months' gains; `put` = `end − start − gain`.
- `rate` = `gain ÷ (start + Σ flowᵢ × weightᵢ)`, where each day's flow is weighted by the share of the range left after it (Modified Dietz, as `investment_stats::performance` does for one account). Flows are the broker's where kept, else the budget's transfers (`flows_of`), plus each joining account's first value on its first day.
- Transfers between two accounts in the report cancel by themselves: one account's flow out is the other's flow in.
- Yearly rate, for ranges over twelve months: `(1 + rate)^(365 ÷ days) − 1`.

**Allocation.** Positions, cash and accrued income from each entry's snapshot, already in the budget's currency. Holdings merge on ISIN, else conid, else symbol with currency (so the same ticker in two currencies stays two rows). Shares are of the positive parts' total, as on the Holdings page: a short option is listed but is not a share.

**Income.** The stored monthly rows summed over the range and the accounts. Yield = dividends and interest over the mean month-end value of the accounts that report income.

**Net worth's market figure.** `Month.market` = the sum of `change` lines in the month across accounts the filter includes and that count in net worth. "Everything else" = the month's change in net worth less `market`.

## 7. Engine

| Change | Where | Notes |
|---|---|---|
| `Month.market: i64` | `reports.rs`, in the loop that already updates balances (178–185); ledger entries carry the transaction's `extra`, so the marker is readable there | Additive field. The pinned output hashes in `desktop/src-tauri/tests/performance.rs` change and must be re-recorded |
| `investment_stats::portfolio(budget, filter) -> Portfolio` | New, beside `page` | Reuses `flows_of`, `kind`, `totals`; `performance` is generalised to take a value series and flows rather than one `Holdings` |
| Read command `investmentsReport {from, to, accountIds}` | `commands.rs` | For the phones |
| Read command `investmentsCsv` | `commands.rs`, `reports::csv`'s neighbour | |
| Tauri command `investments_report`, `export_investments` | `desktop/src-tauri/src/lib.rs`, `bridge.ts` | The desktop calls the engine rather than adding to `investmentStats.ts` |

`Portfolio` (camelCase on the wire): `total {start, end, put, gain, rate, yearly?}`, `months[{month, value, gain, put, byAccount}]`, `accounts[{accountId, source, asOf, stale, joined, start, end, put, gain, rate, estimatedDays, missing, madeOf[{key, value}], cost?, unrealised?, sincePrices}]` (the last four for the Balance sheet: the account's kinds, its cost and unrealised gain, and its balance less its snapshot's total), `allocation {asOf, byKind, byAccount, byCurrency, cash, accrued, cost?, unrealised?}`, `holdings[{key, symbol, name, group, currency, value, cost?, gain?, rate?, share, accounts}]`, `income {months, totals, yield?, commissions, reporting, silent}`, `unfilled[accountId]`.

## 8. Apps

| | Desktop | Android | iPhone |
|---|---|---|---|
| Register the group and reports | `Reports.tsx`: `ReportId`, `REPORTS`, `body()`; groups become tabs by themselves | `ReportsScreen.kt`: `ReportList`, `ReportGroups` | `ReportsView.swift`: `ReportInfo.all`, `groups`, and the `groupName`/`groupHeader` switches (their default is Wealth, so a new group must be added to both) |
| Own data fetch | As `forecast` (special-cased at four places in `Reports.tsx`) | As forecast in `ReportPage` | As forecast in `ReportPage` |
| Bodies | New `reports/Investing.tsx`; `Wealth.tsx` for the net worth split | `ReportMath.kt` view builders, `ReportViews.kt` bodies | `ReportModels.swift`, `ReportBodies.swift` |
| Hide the group without holdings | Filter `REPORTS` | Filter the list and the segmented control | Filter `all` and the title menu |
| Charts | All exist: `ColumnChart` with a line, `Donut`, `BarList` | All exist | All exist |
| Phone specifics | | Two columns unfolded and the hinge postures come from the existing list-and-detail layout; presses and menus use `SoftPress` and `UrMenu` | Written on Windows; needs a build on the Mac |

Remembered view: the desktop's per-group last report (`lb:reports:groupViews`) picks the new group up without change.

## 9. Assistant

- `get_report`: add `marketChange` per month beside `netWorth`.
- New `get_investments_report {from?, to?, accountIds?}`: totals, months, per-account rows, allocation, the 25 largest merged holdings, income. `get_holdings` stays as the per-account detail.
- `create_spreadsheet`: an `investments` sheet (months) and a `holdings` sheet (merged).
- The assistant's guide gains a line on where these reports are.

## 10. Phases

Each ships on its own and leaves the app consistent.

| Phase | Contents | Done when |
|---|---|---|
| 1 | `Month.market`; Net worth's split on all three apps; `get_report.marketChange` | The split's two series sum to the old single series in every month of the test budget and of the owner's budget |
| 2 | `portfolio` (totals, months, accounts), the commands, **Performance** on the desktop, then both phones; the Balance sheet's change split into money put in and markets | Portfolio month values equal the sum of the accounts' balances in `reports::calculate`; one account's figures equal its Holdings page for the same span; each account's two change figures sum to its Balance sheet change |
| 3 | Allocation and merged holdings; **Allocation** on all three; the Balance sheet's unfolding rows, cost line and cash-at-brokers note | Shares sum to 100%; a fund held in two accounts is one row naming both; an unfolded account's lines sum to its row |
| 4 | Income; **Income and costs** on all three; the three CSV exports | Twelve-month totals equal the Holdings pages' "Income and costs" summed |
| 5 | `get_investments_report`, spreadsheet sheets, the phones' overview card | A live model answers "how did my investments do this year?" from the tool |
| 6 | Month-end snapshots (item 6: kinds, cash, accrued, cost, unrealised gain), then "Over time" in Allocation and past dates in the Balance sheet | A second sync in a new month adds a row; an older build still reads the entry; a Balance sheet ending in a collected month unfolds |
| 7 | Trades into monthly realised gains and commissions (item 7); dividends by holding (item 8) | Needs a real statement with trades to check field names against, as Transfers did. **Checked on the owner's statement on 18 November 2026 (section 16.3).** |

Phases 1–5 change no stored data. Phases 6 and 7 add fields inside each `investments` entry; unknown keys are already kept by older builds (`Holdings.extra`), so they sync safely.

## 11. Decisions for the owner

| # | Question | Recommendation |
|---|---|---|
| 1 | A fourth group "Investments", or more reports under Wealth? | The group, hidden without holdings (section 4) |
| 2 | Should money moved into investment accounts stop reading as spending? | **Decided 18 November 2026 (the owner: "handle the decisions"): yes, always, see section 16.** First recommendation: not in this work. It changes figures people have relied on, and Performance's "Money put in" already shows it. If wanted, do it as its own change: a switch in the Spending reports' filters, off by default, that leaves out transfers to accounts with holdings and says so in the "left out" chips |
| 3 | Naming of the split: "Markets" and "Everything else"? | Yes for the first build; settle the wording with the Arabic at review |
| 4 | Start collecting month-end snapshots (phase 6) early? | Yes, alongside phase 1. It costs little, and every month not collected is a month that Allocation's "Over time" and a past Balance sheet can never show |
| 5 | Move the desktop's Holdings view onto the engine's figures, retiring `investmentStats.ts`? | **Decided 18 November 2026: yes, now, see section 16.** First recommendation: separate follow-up. Worth doing, but it is a refactor with no visible change |
| 6 | On the Balance sheet, should money borrowed from a broker (margin) move to liabilities? | No: it is a negative line under Cash and bank. Moving it would make the Balance sheet's assets and liabilities disagree with Net worth's, which counts each account once. (Broker cash counting as cash was settled by the owner's direction to group by asset type.) |

## 12. Testing

- **Engine.** A fixture budget with: an IBKR account with flows, a typed-value account with three sparse values, a transfer between the two, an account that joins mid-range, and one in another currency. Pinned: portfolio months equal `reports::calculate` balances; gain equals the `change` lines; joining is not a gain; the internal transfer cancels; holdings merge on ISIN; `market` plus "everything else" equals net worth's change.
- **Shared fixture.** `shared/fixtures/investments-report.json` with the expected `Portfolio`. There is no shared report fixture today (the phones' report tests are hand-written mirrors), so this is the first: the engine test produces it, and the desktop, Android and iPhone tests build their view models from it.
- **Desktop.** Vitest for the three bodies and the net worth split; in the real app through the QA harness against the two-account stand-in server.
- **Android.** Unit tests for the view builders; on the emulator with the sample budget plus a positions file; on the Fold folded, unfolded and half open, in English and Arabic.
- **iPhone.** XCTest written here; compile and run on the Mac. Nothing in this plan is verified on iPhone until then.
- **Owner's data.** Phase 1 and 2 figures checked against the owner's real account for structure and reconciliation only.

## 13. Risks

| Risk | Effect | Handling |
|---|---|---|
| Sparse values (positions file, typed value) | A gain lands in the month of the next snapshot, so monthly columns are lumpy | Note under the chart naming those accounts; the range's total is still right |
| Prices from different days across accounts | Allocation mixes dates | Dated by the oldest; stale accounts flagged |
| The bank connection gives no kinds | Its holdings sit in one "Holdings" slice | Said in the note; the positions file or IBKR classify properly |
| Merging on symbol where there's no ISIN | Two different securities with one ticker could merge | Key includes the currency; ISIN and conid win where present |
| History older than 400 days is month-end only | None for monthly reports; the return's day weighting is coarser for old ranges | Accept |
| `Month.market` changes report output | Pinned performance hashes fail | Re-record with the change, in the same commit |
| iPhone can't be built on this PC | Errors found late | Keep iPhone bodies thin over the shared fixture; build on the Mac at the end of each phase |

## 14. Simplifying Reports (built 18 November 2026, at the owner's "Yes, build it"; uncommitted)

The owner's reaction once the Investments group was in (18 November 2026): Reports "seems a bit overwhelming and not very intuitive to navigate". There are now fifteen reports behind two rows of tabs (four groups, then three to five reports each), with names that overlap (Income vs. expense, Income breakdown, Money in and out; Net worth, Balance sheet, Balance over time), and the desktop opens on whichever report was open last, with no starting point.

**Proposal: six reports, one row, each answering one question.** Every chart and table that exists today stays; what changes is where it lives. Reports that answer the same question become views of one report, chosen by a switch inside it.

| Report | The question | Views (the switch inside) | Replaces |
|---|---|---|---|
| Spending | Where did my money go? | By category, By payee, Over time, Calendar | Four reports |
| Income | Am I spending less than I earn? | Income and spending, Sources, Money in and out | Three reports |
| Net worth | What am I worth, and why did it change? | Over time, By account | Net worth, Balance over time |
| Balance sheet | What do I own and owe? | None | Balance sheet |
| Investments | How are my investments doing? | Performance, Allocation, Income and costs | Three reports; hidden without holdings |
| Forecast | What's coming up? | Balances ahead, Cash age | Forecast, Cash age |

Then three things on top of that:

1. **A home page of six cards**, one per report: its question, one headline figure and a small chart. A card opens its report. The phones already have an overview like this with a list of every report under it; the list goes, and the desktop gets the same home page in place of opening on the last report.
2. **One filter line.** The range and the accounts collapse into one control that reads "Last 12 months · All accounts" and opens to change them; categories join it where they apply. The range is remembered across reports rather than per report.
3. **Headline first.** Each view opens with its tiles and one chart; the long tables fold behind "Show the figures" on the phones (the desktop keeps them, below the chart).

What it costs: a navigation change in `Reports.tsx`, `ReportsScreen.kt` and `ReportsView.swift` and their remembered-view keys; the report bodies themselves barely change. The assistant's guide and each README's report list follow.

Alternatives considered: keeping fifteen reports and adding search (it treats the symptom); one long scrolling page per group (it trades tabs for scrolling, and the Holdings page showed the owner prefers less on a screen).

### 14.1 The build spec (binding for the three apps)

**Reports and views.** Each app keeps its existing report bodies and their ids as the views; six reports sit over them.

| Report (id) | Views, first is the default (existing id → short label on the switch) |
|---|---|
| Spending (`spending`) | `categories` → Category · `payees` → Payee · `trends` → Over time · `calendar` → Calendar |
| Income (`income`) | `incomeExpense` → Vs. spending · the income-sources report → Sources · `cashflow` → In and out |
| Net worth (`networth`) | `networth` → Over time · `balances` → By account |
| Balance sheet (`balance`) | none |
| Investments (`investments`) | the three investment reports → Performance · Allocation · Income and costs. Hidden without holdings |
| Forecast (`forecast`) | `forecast` → Balances · `age` → Cash age |

**Navigation.** One level. The desktop's header has one row of tabs: Overview, then the six reports. The phones open Reports on the overview; a card opens its report; the wide-screen index lists Overview and the six reports. Inside a report with views, a segmented switch sits under the title; the view last used in each report is remembered. The group tabs, the second row of report tabs or chips, and the phones' "All reports" list go. A saved or deep-linked old report id opens its report at that view (the assistant's Show, the Accounts page's net worth link, a restored session).

**Overview.** Six cards, in the table's order (Investments only with holdings): the report's name, its question, one headline figure with a short note, and a small chart. The desktop shows them three across; a phone one per row; a wide phone two across. They cover the last six months, as the phones' overview does today.

| Card | Headline | Small chart |
|---|---|---|
| Spending | Spent this month, with the monthly average as the note | Spending by month |
| Income | Income less spending over the six months | Income against spending by month |
| Net worth | Net worth now, with its change as the note | Net worth by month |
| Balance sheet | Total assets, with what is owed as the note | One bar split into owned and owed |
| Investments | Value now, with the gain as the note | Value by month |
| Forecast | The lowest balance ahead and when, from the forecast the report already reads | Balance ahead |

The desktop opens Reports on the overview when the app starts, and afterwards returns to where the person was.

**One filter line.** A single control whose label says what is in force ("Last 12 months · All accounts", with "· 3 categories" where categories are narrowed). The desktop opens a popover holding the existing range picker and the account and category filters; the phones open a sheet with a row for each (range, accounts, categories where they apply) leading to the pickers they have today, and Export CSV as its last row. The desktop keeps its Export button beside the control. The range and accounts chosen carry from one report to the next. Views that take no range (Allocation, the forecast) leave it out of the label.

**Headline first, on the phones.** A view shows its tiles and charts, and any list of nine rows or more shows six and folds the rest behind the existing "Show all N" / "Show fewer"; a list of up to eight stays whole, so a fold never hides only a row or two (first built as "longer than six", which on the emulator put "Show all 7" over a single hidden row). The Balance sheet's statement is its content and stays open.

**Wording** (reuse each app's existing phrase where it has one; new ones as here):

| English | Arabic |
|---|---|
| Overview | نظرة عامة |
| Where did my money go? | أين ذهب مالي؟ |
| Am I spending less than I earn? | هل أنفق أقل مما أكسب؟ |
| What am I worth, and why did it change? | كم صافي ثروتي، ولماذا تغيّر؟ |
| What do I own and owe? | ماذا أملك وبماذا أدين؟ |
| How are my investments doing? | كيف حال استثماراتي؟ |
| What's coming up? | ما القادم؟ |
| Category | الفئة |
| Payee | المستفيد |
| Vs. spending | مقابل الإنفاق |
| Sources | المصادر |
| In and out | الداخل والخارج |
| Balances | الأرصدة |
| Lowest balance ahead | أدنى رصيد قادم |
| {count} categories | {count} فئات |
| {count} accounts | {count} حسابات |

### 14.2 As built, and what was checked

Built on the desktop, Android and the iPhone to 14.1, with the assistant's guide rewritten for the six reports. Where the apps differ from 14.1 or from each other:

- **Fold rule.** Nine rows or more (see "Headline first" above); Largest holdings and By holding follow it too, where they used to fold at ten.
- **Android's small charts** are bars throughout (it has no small line chart), 104dp tall; the desktop draws a new `Spark` (columns or a line, no axes). The first Android build reused the old overview's 150dp chart and six cards ran to three and a half screens, so it was cut down after a look on the emulator.
- **The question** is the first line of the page on Android (the bar's subtitle would cut it), under the title on the desktop and the iPhone.
- **The id `income`** opens Income › Sources on every app, since it was the old sources report's id.
- **Opening on the overview.** The desktop keeps the place in `sessionStorage` (so a restart opens the overview) and the last view per report in `localStorage` (`lb:reports:views`); the old `lb:reports:view` and `lb:reports:groupViews` are carried over once.
- **The desktop's overview** applies the left-out accounts and categories, so a card matches the report it opens, and has no filter control of its own.
- **Nothing outside Reports opens a report by id** on the desktop or Android today; each has a resolver ready (`placeOf`, the saved-state mapping). The iPhone's Accounts link and debug route go through `ReportTopic.resolve`.
- **Unused strings** were removed on the desktop (12, its catalog test forbids them) and Android (35); the iPhone catalog keeps its old entries for Xcode to mark stale.

Checked: `cargo test --workspace` (all pass, the guide's label test included), desktop `tsc` and 904 tests, Android 387 unit tests and lint with no errors. In the real desktop app at its 1000 by 662 window, English and Arabic: the overview, each report's switch, the filter popover with the range picker inside it, the range carrying from Spending to Income, leaving Reports and coming back to the same report and view. On the Android emulator, English and Arabic: the overview, Spending with its switch, the filter sheet, a folded list, and the wide layout with the index beside the overview.

Not checked: the iPhone code (never compiled; the builder's list of lines least sure to compile is in the session's report), the desktop's dark-mode focus ring on a card and its header at a very narrow window.

On the Fold (build 1000092, the owner's real budget, dark mode), 18 November: the overview on the cover screen; Spending's four-way switch and Investments' "Income and costs" fit at 475dp; the filter sheet's Months, Accounts and Categories rows each open their picker, a preset applies at once and the label follows ("Last 6 months"), and the range carries to Investments; Export CSV opens the system's save dialog (cancelled); unfolded, the index beside the overview and each report, one card across in the report pane, so no uneven two-across rows; book posture; Arabic unfolded. One fault found and fixed: the Net worth icon used Material's auto-mirrored ShowChart, which in Arabic drew a line falling to the right, while every chart runs left to right in both languages; Android now uses `RisingChart` (Design.kt), the same icon unmirrored, there and on investment accounts and the loan planner, which had the same fault. Build 1000093, with the fix, was installed and seen in Arabic on the cover screen: the icon rises to the right, and Spending's and Investments' switches fit.

## 15. Trades in the guided setup (built 18 November 2026)

At the owner's ask. The request for Client Portal's query builder now names Trades and still fits its 200 characters (195, by dropping the word "Include" before "Currency Rates"); the by-hand steps say "Trades (Executions)"; a statement without the section lists "Trades" among what it leaves out, with a note saying what that costs (realised gains and commissions by month), and the desktop's "What the statement includes" list has a Trades line. The new request wording has not been tried in Client Portal's own builder.

## 16. The two decisions, settled (18 November 2026)

The owner asked for decisions 2 and 5 to be handled. On the owner's own budget, money moved into investment accounts was most of a year's "spending" and most of why Income read as spending more than was earned, so the switch first proposed (off by default) would have left the overview's Spending and Income cards as misleading as before.

### 16.1 Money moved into investments is not spending (decision 2)

**Which accounts.** An *investment account* is an account off the budget that either has holdings (an `investments` entry with values: the accounts the Investments report covers) or whose `extra.accountType` is `investment`, `retirement` or `otherAsset`. `otherAsset` is in because YNAB imports every tracking asset as one: on the owner's budget the brokerage account is an `otherAsset` with no holdings, fed by transfers in a "Stock Investment" category, and would otherwise have stayed spending. Loans, property, vehicles, receivables and accounts with no type are unchanged: paying a loan and buying a car stay spending.

**Rule.** In `reports::calculate`, a ledger entry that moves money between a budget account and an investment account counts in neither spending nor income. Its parts are left out of `spending`, `categories`, `payees`, `days`, `transactionCount`, `income` and `incomeSources`, and of `details` (drill-downs), exactly as if they were not there. They are added instead to a new figure, `invested`: the budget side's money moved into investment accounts, less what came back out of them. It is positive when more went in. The category filter applies to it as it applies to spending (an income part, money taken out to be assigned, counts whatever the filter, as income does today). Cash flow (`inflow`, `outflow`) is unchanged: the money did leave or reach the budget accounts. The budget itself (category activity, Ready to Assign) is unchanged; this is reports only.

**Wire.** `Month.invested: i64` and `Report.invested: i64` (camelCase `invested`). `get_report` (assistant) carries both. The report CSV gains an "Invested" column where it has a spending one, and the workbook's report sheet likewise.

**What the apps show.** In the Spending report (every view) and the Income report (every view), when the range's `invested` is not 0, one muted line under the filter line:

| When | English | Arabic |
|---|---|---|
| `invested` > 0 | {amount} moved into investments isn't counted here. | لا يُحتسب هنا {amount} نُقلت إلى الاستثمارات. |
| `invested` < 0 | {amount} taken out of investments isn't counted here. | لا يُحتسب هنا {amount} سُحبت من الاستثمارات. |

`{amount}` is the absolute value, formatted as money. On the desktop it sits in the row of left-out chips, as text rather than a chip (it can't be put back). No switch to count it back: Investments › Performance shows the same money as "Money put in". The overview's cards change only through their figures.

**Assistant guide.** The reports topic gains: "Money moved into or out of investment accounts (and other assets) counts as neither spending nor income in Reports; for accounts with holdings, Investments › Performance shows it as money put in."

### 16.2 The desktop's Holdings view on the engine's figures (decision 5)

The phones already read the engine's `investmentPage` (`investment_stats::page`). The desktop gets a Tauri command for it (`investment_page { accountId }`, read-only, as `investments_report`), and `Investments.tsx` renders from that page instead of computing with `investmentStats.ts`, which is deleted with its test. Nothing visible changes: the same periods, swings, monthly gains, day change, totals, holdings, cash, accrued, earned and estimated days. What `investmentStats.ts` did that the page doesn't carry stays in the component as display-only code (formatting a percent). The engine's own tests cover the figures; the desktop's tests check that the view shows what the page says.

### 16.3 As built, and what was checked (18 November 2026, uncommitted)

- **Engine:** `investments::invested_accounts` reads an account's type as the apps do: `extra.accountType` when it is a known type, else YNAB's `extra.source.type` (a YNAB import keeps its type only there). A fresh start drops `source`, so an account typed only by YNAB counts as spending again afterwards unless it has holdings or a type of its own. `Month.invested`, `Report.invested`, `get_report`, the report CSV and the workbook's months sheet carry the figure; CSV and workbook headers stay English like all the others. Forecast events gain `invested: true` (left off the wire when false) for money moved to or from these accounts, and all three apps leave such events out of scheduled spending, since `recentSpending` (the Reports figure) now leaves them out: otherwise the suggested everyday spending would have come out too low.
- **Apps:** the muted line under the filter line in every Spending and Income view on all three; the desktop's Holdings view reads `investment_page` and `investmentStats.ts` is gone (the figures load asynchronously, with a spinner on first open and the last page kept while it re-reads).
- **Assistant:** `get_investments_report`'s description now asks the model to lead with the gain and the return when asked how the investments did.
- **Checked:** Rust (core 513, services 276, all crates); desktop type check, 899 tests, native check; Android 389 unit tests and lint with no errors; iPhone bracket, catalog and translation checks only. In the desktop app on the QA budget: Spending fell by exactly the 3,750.00 moved into an investment account and said so under the filter line, Income said the same, Net worth didn't; the Holdings view drew from the engine with its periods, chart, holdings, cash and accrued rows. The six report fingerprints in `desktop/src-tauri/tests/performance.rs` were re-pinned (the `invested` field changed the hashes, not the figures).
- **The live model on a phone (build 1000093, before section 16):** asked "How did my investments do this year?", the assistant ran the investments report for the year and its figures matched the app's Investments › Performance for Year to date exactly (value now, money put in, gain); a follow-up drew the gain and return correctly. It said, rightly, that dividends by holding weren't there: the account had last been synced by a build before phase 7, so nothing by holding or trade had been stored yet. Its first answer didn't state the gain and return, hence the description change above.
- **A real budget can split one broker in two:** there, the Interactive Brokers connection filled one investment account while the bank transfers to Interactive Brokers went to a second, YNAB-imported "otherAsset" account with no holdings. Hence `otherAsset` among the investment types in 16.1.
- **On a real account, build 1000094:** one sync. Phase 7 holds on a real statement: Income and costs lists commissions on trades month by month and dividends by holding (one holding no longer held, so shown by its symbol alone), and Performance shows realised gains. So the Trades rows' `levelOfDetail`, `tradeDate`, `fifoPnlRealized`, `ibCommission` and the dividend rows' symbols are as the reader expects. The query already had every section ticked; nothing was missing but a sync by a build that reads them. Spending and Income both show the "moved into investments" line; the amount left out equals what the budget's investing category used to count as spending, and Income's headline turned from a shortfall into a surplus.
- **Not checked:** the iPhone code (never compiled).

## 17. What's held, what it is, and in what currency (18 November 2026, the owner's ask)

The owner, looking at the Balance sheet on the Fold: it "should have better information on what's held, what kind of instrument it is, original currency it's held in". On their budget each investment section (Stocks, Bonds, Options) listed only the account's name, with "From — · Change —" on every row; cash at the broker didn't say which currency it was in; and nothing anywhere said a holding was an ETF rather than a share, or what it is worth in the currency it trades in.

### 17.1 Engine (binding)

**Read and keep, per holding** (`investments::Holding`, stored; each left out of the JSON when empty, so older entries read as before):
- `subKind`: the broker's finer category as written (IBKR's `subCategory` on Open Positions: COMMON, ETF, REIT, ADR, PREFERRED, ...).
- `putCall`: "C" or "P" for an option (IBKR's `putCall`).
- `amount`: what it is worth in its own currency, as a decimal string the way the statement gives it (IBKR's `positionValue`, else quantity × price × multiplier as the reader already works out). Other sources set it when they give a value in the holding's own currency.

All three fill only at the next sync: until then the apps show what they show today for that part.

**Decimals.** Every own-currency amount leaves the engine (`Row`, `Merged`, `byCurrency`, `accounts[].cash`, the Holdings page's cash rows, the assistant's tools) with its currency's usual decimals, `investment_stats::in_minor_units`: two, three for KWD, BHD, OMR, JOD, TND, LYD and IQD, none for JPY, KRW, VND and the other zero-decimal currencies, rounded half away from zero. A statement writes `positionValue` with whatever decimals it likes ("17098.9999", "1160"), and a positions file none, so the apps would otherwise show "10,400 EUR" beside "17,099.00 USD". Stored amounts stay as written.

**Name what it is:** `investment_stats::instrument(kind, sub_kind, put_call) -> &'static str`, an English key the apps translate:

| From | Key | Arabic |
|---|---|---|
| STK, subKind ETF | ETF | صندوق متداول |
| STK, subKind ETN | Exchange-traded note | ورقة دين متداولة |
| STK, subKind REIT | REIT | صندوق ريت |
| STK, subKind ADR or GDR | Depositary receipt | إيصال إيداع |
| STK, subKind PREFERRED | Preferred share | سهم ممتاز |
| STK, subKind RIGHT | Right | حق اكتتاب |
| STK, subKind CLOSED-END FUND | Closed-end fund | صندوق مغلق |
| STK, anything else | Stock | سهم |
| BILL | Treasury bill | إذن خزانة |
| BOND, subKind containing GOVT | Government bond | سند حكومي |
| BOND, subKind containing CORP | Corporate bond | سند شركات |
| BOND, anything else | Bond | سند |
| OPT and the other option codes, putCall C | Call option | خيار شراء |
| OPT and the other option codes, putCall P | Put option | خيار بيع |
| any other | the kind's singular, as `kind(code).1` today (Fund, Option, Future, Currency, Crypto, Commodity, CFD, Other) | as today |

Matching on subKind ignores case and surrounding spaces.

**Wire** (camelCase; Appendix A grows by these):
- `investmentPage` rows (`Row`): `instrument`, `amount` (null when unknown).
- `investmentsReport` merged holdings (`Merged`): `instrument`, `amount`: the accounts' amounts added up when every account holding it gave one in the same currency, else null; added as decimals, never through floating point.
- `investmentsReport.allocation.byCurrency[]`: `amount`, the holdings' and cash's own-currency amounts in that currency added up, null when any is unknown (left out on `byKind` and `byAccount`).
- `investmentsReport.accounts[]`: `cash: [{currency, amount, value}]`, today's cash by currency from the account's holdings entry (empty for an account without one).
- Assistant: `get_holdings` positions and `get_investments_report` holdings carry `instrument` and the own-currency amount with its currency; `byCurrency` its amount. The tool descriptions say what they are.

### 17.2 Apps (binding for desktop, Android, iPhone)

**Own-currency amounts** are written "{amount} {CODE}" with the budget's number format (grouping and decimals of the amount as given, e.g. "61,476.00 USD"), kept left to right in Arabic. They are shown only when the currency is not the budget's: a riyal holding in a riyal budget already shows its value.

**Balance sheet, when its range ends in the current month** (what is held is today's):
- In each investment section (Stocks, Funds, Bonds, Options, Futures, Currencies, Crypto, Commodities, CFDs, Other investments) the account lines give way to the holdings themselves, merged across accounts (the report's `holdings`), largest first. Each row: the holding's name (its symbol when it has none) as the title; under it "{instrument} · {symbol} · {own-currency amount}", leaving out the symbol when it is the title and the amount when not shown by the rule above; on the right its value in the budget's currency. The account's name joins the line ("· Brokerage") only when two or more accounts hold investments. Holding rows have no start or change.
- A section of nine holdings or more shows six and folds the rest behind "Show all N" / "Show fewer", as lists do elsewhere on the phones (the desktop too, here).
- What an account's holdings don't explain (its `endParts.rest`) stays as one line under Other investments, named for the account, as today.
- "Cash at {account}" gets, under it, the cash by currency from `accounts[].cash`: "796.50 USD · 1,500.00 SAR", when any of it is in a currency other than the budget's.
- When an earlier range ends before this month, the sections keep the account lines as today.

**Balance sheet, any range:** a row whose start is unknown no longer shows "From — · Change —": just its share ("9.5% of assets"; Arabic "{share} من الأصول"). Rows with a known start are unchanged.

**Investments › Allocation:** the holdings list's rows put "{instrument} · {symbol} · {own-currency amount}" under the name (where today they show the singular kind); the By currency rows put the currency's own amount under the currency, by the same rule.

**Holdings page (one account):** a row's line under the name uses the instrument ("ETF · VWRA · 120.5 × 141.9087 USD") where it used the singular kind.

**Wording:** the instrument keys in the table above; "{share} of assets" / "{share} من الأصول". Nothing else is new.

### 17.3 As built, and what was checked (18 November 2026, uncommitted)

- **Engine:** as 17.1, plus: own-currency amounts leave the engine with their currency's usual decimals (`in_minor_units`); accrued income counts in `byCurrency[].amount` in its account's own currency, turned back with the stored rate, so an Interactive Brokers account's dollar line has an amount (without it, every IBKR account voided its base currency's amount).
- **Apps:** as 17.2, with these settled while building: a symbol is shown only when it isn't the title and is 12 characters or fewer, as the Holdings page already did (an option's exchange code, "AAPL 261218C00250000", says nothing its name doesn't); Allocation's holdings line sits under the name, not beside it, so neither is cut; on the desktop's Balance sheet table a row with an unknown start leaves Start and Change blank and keeps its share in the Share column, rather than "{share} of assets" (the phones use the phrase); and in every app a section shows its holdings only when they add up to that section's account lines, else it keeps the account lines, so a section's rows always come to its figure.
- **Checked:** Rust (core 519, services 276, all crates), desktop type check and 907 tests, Android 392 unit tests and lint with no errors, iPhone bracket, catalog and translation checks. In the desktop app on the QA budget, after a sync from the stand-in statement given an ETF, a REIT, a government bond and a call option: the Holdings view named each; the Balance sheet listed the holdings under Stocks, Bonds and Options with "ETF · VWRA · 34,200.00 USD" and "Call option · 840.00 USD"; "Cash at Long term" read "820.00 USD · 1,500.00 SAR"; Allocation's By currency put "47,465.00 USD" under USD; the same in Arabic.
- **On a real account, build 1000095, after one sync:** a real statement carries `subCategory` and `putCall` as expected. Holdings read ETF, Corporate bond (so IBKR's bond subCategory contains CORP) and Put option on the Holdings page; the Balance sheet lists them under Stocks, Bonds and Options with their dollar values, bonds without a symbol line since their symbol is their name; "Cash at" shows its dollars; rows with an unknown start read "{share} of assets"; Allocation's By currency shows the dollar total under USD.
- **Not checked:** the iPhone code (never compiled).

## 18. A deposit counted twice (18 November 2026, found on a real account)

A second Interactive Brokers account, after a sync, showed a loss about the size of one of its two bank deposits. "Money put in" read about one deposit more than went in, and the Holdings page listed one day as "counted as money moved". The broker dated a wire on the 30th and the account's value showed it on the 5th, after a weekend; the rule for unexplained jumps only looked three days around a jump for a listed deposit, so it counted that deposit once as listed and again as a guess, and that month's market change took the deposit as a loss.

Fixed in `investments::apply`: a listed deposit or transfer dated up to a week (`NEAR_DAYS`, 7) from a jump explains it, and each listed one explains one jump only, the nearest close one first, so a real unlisted transfer of the same size beside a deposit is still caught. Tests for both. The stored figures are rewritten by the next sync of the account (build 1000096 on).

Checked on the phone with build 1000096 and a sync: money put in fell to the two transfers (plus three small early-year days still estimated), and the gain since tracking began to a loss of a few percent. The day-by-day figures still showed the wire's days as a drop and recovery of the whole deposit (worst day the 1st, best day the 5th), because the deposit counted on the broker's date while the value showed it later. So a listed flow that explains a jump on another day now counts on the jump's day in the daily flows; the movement keeps the broker's date for the review. Build 1000097 has it. Checked after installing it and a sync: best day, worst day and largest drop are ordinary market days, money put in and the gain unchanged.

Still open then: over "1 year" the account's return read about −40% for a loss of under 5% of the money in it. That is Modified Dietz doing what it does when nearly all the money arrives in the period's last weeks (the gain over a small time-weighted average); a time-weighted return, or the period starting when the money arrived, would read under −5%.

Both then fixed, at the owner's "Yes, fix it" and "the transactions still dont make any sense either":
- **Return.** `investment_stats::dietz` measures from `at_work_from`: the period's first day, unless the money at work then was under a twentieth of the most there was in the period, and then the day it first reached that. The yearly rate is over the same days. Accounts that held money all along read as before; the account in question reads under −5% over a year.
- **Register.** The account's register had the user's transfer of the 30th, then a generated "Money moved in or out" taking it back out that month and putting it back the next, because the broker counted the wire on the 5th and each month's end is set to the broker's value. In `investments::revalue`, a transfer the broker counted on another day within a week, within 2% of the amount, now counts on the broker's day for those month ends: the wire is in transit, so it is in the balance at the month's end, and the only line left is the little lost to the exchange rate, in the month it arrived. Also in the register, a YNAB "Reconciliation Balance Adjustment" entered after tracking began is offset by a "Money moved" line of the same amount: the broker's value is the record, so that adjustment does nothing but add two lines.

Build 1000098 has both. Checked on the phone after installing it and a sync: the transfer's month has the transfer and its market change; the next month's extra line is "Money moved in or out" for the exchange-rate difference only; the year's return reads under −5%. Seen in passing: on the phone's Holdings page the estimated-days note shares its row with the "The days counted as money moved" button, which squeezes the note into a narrow column on the cover screen. Fixed in both phones' shared notice card (an action longer than 16 characters goes under the text), and seen right on the cover screen in build 1000099.

## 19. Review of the IBKR path for new users (19 November 2026, the owner's ask)

The owner asked for the engine and the import to be reviewed so that everything is logical and works for new users seamlessly. An independent reviewer traced the code with throwaway tests; a new user's first connect was run in the desktop app (fresh budget, a bank account, connect, review three deposits, a second login, a transfer between the two accounts, a second sync: the registers read Starting value, the broker's movements, a Market change a month, and nothing changed on the second sync). Fixed, each with a test, and re-checked by the reviewer:

- **The daily sync's sliding year.** Flows were rebuilt from each statement's own window, so a wire re-dated to the day the value showed it, a withdrawal listed after its drop, or an estimated jump came back wrong once the window slid past it (the owner's 30 September wire would have read as a 100,000 gain a year later). Flows are now worked out each sync from all kept movements and stored values over the daily-values region; before it they are frozen. Each movement remembers the day it counts on (`counted`), and an entry first stored before movements were kept freezes what came before them (`movementsFrom`), so the 400-day line and skipped sync days change nothing.
- **A statement missing a section** (Cash Transactions or Transfers) no longer wipes the deposits, transfers or income an earlier one stored; a first statement lacking both lets the budget's own transfers count.
- **The unexplained-jump guess** applies only when the statement can't list money moved (a section missing, or an entry from before movements were kept). With every section, a big day is the market, not money moved.
- **Re-pointing a broker account** to another budget account takes its generated lines and its reviewed rows with it, so net worth counts the broker once.
- **"Add with no other account"** on a deposit counted days later no longer leaves a −X/+X pair; the review and the register share one tolerance (3%) for "nearly the same amount".
- **Thinning** keeps the first valued day; a sync keeps the stored entry's unknown fields; unchanged generated lines take new wording unless the user wrote their own note there.
- **An older saved statement** adds to the history and leaves what the account holds now as it is.
- **The deposits review** offers "Add as a transfer from <account>" only for accounts whose history reaches back to the deposit's date (desktop, Android, iPhone), so a new user's bank added today with its balance isn't charged twice.

Still as designed or open: a query without Transfers still guesses at big days (the note says to add the section); deposits typed by hand into an account stay there when it is re-pointed; the generated lines' wording (payees in English, ISO months in memos, "Money moved in or out" also covering exchange-rate differences and offsets to the user's own entries) is unchanged; skipped review rows come back in every review. Old YNAB "Reconciliation Balance Adjustment" entries after tracking began are each offset by a Money moved line: correct, but twice the lines. Build 1000100 has all of this. On a phone with two real broker accounts, after it was installed and both synced: one account unchanged; the other's three small early-year estimated days became market moves (its query lists every deposit and transfer), money put in is the two transfers as the broker received them, the estimated-days note is gone, and the register's only Money moved lines are the exchange-rate difference and the offset of a YNAB adjustment.

## Appendix A. The wire shape (binding for the engine and the three apps)

Read command `investmentsReport`, params as `reports` (`from`, `to` as "YYYY-MM", `accountIds`; empty means all). Amounts are cents in the budget's currency; rates and shares are fractions (0.034 is 3.4%); a figure that can't be known is `null`. Keys are camelCase.

```json
{
  "from": "2025-12", "to": "2026-11",
  "total": { "start": 0, "end": 0, "put": 0, "gain": 0, "rate": 0.034, "yearly": null, "days": 365 },
  "months": [ { "month": "2025-12", "value": 0, "gain": 0, "put": 0, "byAccount": { "<accountId>": 0 } } ],
  "accounts": [ {
    "accountId": "", "source": "ibkr|simplefin|file|manual", "sourceAccount": "", "asOf": "2026-11-17", "joined": "2025-11-14",
    "start": 0, "end": 0, "put": 0, "gain": 0, "rate": null, "estimatedDays": 0, "missing": [], "sparse": false,
    "startParts": null,
    "endParts": { "month": "2026-11", "asOf": "2026-11-17", "byKind": [ { "key": "Stocks", "value": 0 } ], "cash": 0, "accrued": 0, "rest": 0 },
    "cash": [ { "currency": "SAR", "amount": "1406.25", "value": 37500 } ]
  } ],
  "allocation": {
    "asOf": "2026-11-17", "total": 0, "count": 5, "cash": 0, "accrued": 0, "cost": null, "unrealised": null, "unrealisedRate": null,
    "largestShare": null, "topFiveShare": null,
    "byKind": [ { "key": "Bonds", "value": 0, "share": 0.68 } ], "byAccount": [ { "key": "<accountId>", "value": 0, "share": 1 } ],
    "byCurrency": [ { "key": "USD", "value": 0, "share": 1, "amount": "88125.00" } ]
  },
  "holdings": [ { "key": "", "symbol": "", "name": "", "group": "Stocks", "one": "Stock", "instrument": "ETF", "currency": "USD", "value": 0, "amount": "39000.00", "cost": null, "gain": null, "rate": null, "share": 0.31, "accounts": ["<accountId>"] } ],
  "income": {
    "months": [ { "month": "2025-12", "dividends": 0, "interest": 0, "tax": 0, "fees": 0, "other": 0 } ],
    "dividends": 0, "interest": 0, "tax": 0, "fees": 0, "other": 0, "yield": null,
    "commissions": [ { "accountId": "", "from": "2025-11-18", "to": "2026-11-17", "amount": 0 } ],
    "reporting": ["<accountId>"], "silent": ["<accountId>"]
  },
  "unfilled": ["<accountId>"]
}
```

- `months` has one row per month of the range. `byAccount` lists an account from the month it joins.
- `accounts` are those with holdings that the filter includes, in the budget's account order. `sparse` is true for every source but IBKR. `joined` is the first valued day.
- `startParts` is the split of the account's balance at the end of the month before `from`; `endParts` at the end of `to`. Each is the stored snapshot for exactly that month, or, for a month after the newest snapshot, the positions in hand; otherwise `null`. Within one: `byKind` values, `cash`, `accrued` and `rest` sum to the account's ledger balance at that date. `byKind` keys are the engine's group names ("Stocks", "Funds", "Bonds", "Options", "Futures", "Currencies", "Crypto", "Commodities", "CFDs", "Holdings", "Other holdings").
- `allocation` and `holdings` are today's positions across the accounts, whatever the range. `holdings` are merged across accounts and sorted by value, largest first; `key` is the ISIN, else the conid, else symbol and currency. Shares are of the positive parts' total (`allocation.total`).
- `income.reporting` are the accounts with any monthly income rows; `silent` the rest. `income.byAccount` has one row per reporting account, in that order: `{accountId, dividends, interest, tax, fees, other}`, summing to the totals.
- Settled in the first build: a split line whose account held nothing at the start shows 0 there, not "—"; an account whose parts are only `rest` (a typed value) is one whole line under Other investments; if the parts and the ledger disagree, the difference goes to the "Since prices" line.
- `unfilled` are open tracking accounts typed investment or retirement with no holdings.
- `reports` gains `market` on each month: the sum of the generated market-change lines that month in the accounts counted.

Also: read command `investmentsCsv` (same params, returns the CSV text); Tauri commands `investments_report` and `export_investments`.

Stored, inside each `investments` entry: `snapshots: [{ "month": "2026-11", "asOf": "2026-11-17", "total": 0, "byKind": { "Stocks": 0 }, "cash": 0, "other": 0, "cost": null, "unrealised": null }]`, one per month, replaced when the same month is brought in again.

### Section 17 additions (what's held, what it is, its own currency)

Stored, on each holding of an `investments` entry, each left out when empty: `subKind` (the broker's finer category as written), `putCall` ("C" or "P"), `amount` (what it is worth in its own `currency`, a plain decimal with the decimals the source wrote: "61476.00", "10800", "-425.50"; worked out to the cent when a statement gives no `positionValue`; lots of one position are added up). A positions file, the bank connection or the assistant sets `amount` only when the holding's currency is the one the figures are in.

On the wire:
- `instrument` on `investmentPage` rows and on `investmentsReport.holdings`: `investment_stats::instrument(kind, subKind, putCall)`, the keys of section 17.1's table. Without a finer category or side it is the kind's singular (`one`), "" for a holding of no known kind.
- `amount` on `investmentPage` rows: the stored amount, `null` when there is none.
- `amount` on `investmentsReport.holdings`: the accounts' amounts added as decimals when every account holding it gave one in the same currency, else `null`.
- `amount` on `allocation.byCurrency[]` only (absent on `byKind` and `byAccount`): what that currency's part is in that currency, `null` when any piece of it is unknown. Pieces are each holding's amount and each cash line's amount, and accrued income, which is in the account's own currency: its value turned back at the rate the entry was converted with (`Holdings.rate`), to the currency's minor units. A holding without an amount, a value nothing breaks down, and accrued income at an entry whose rate doesn't read count at their value when the currency is the budget's (the value is then the amount), and make the figure `null` otherwise.
- `accounts[].cash`: `[{ "currency", "amount", "value" }]`, the entry's cash by currency as stored (`amount` in that currency, `value` in the budget's cents); empty when the entry lists none.

### Phase 7 additions (trades and dividends by holding)

Read from the statement only when it has them. (Since section 15, the guided setup asks for Trades and a statement without them says so.)

Stored, inside each `investments` entry:
- `trades: [{ "month": "2026-10", "realised": 0, "commissions": 0, "count": 0 }]`: from the Flex `Trades` section, by the month of each trade. Rows are counted at one level only, so nothing is counted twice: `levelOfDetail` "EXECUTION" where any exist, else "ORDER", else rows with no level; never "CLOSED_LOT" or the summaries. `realised` is `fifoPnlRealized`, `commissions` is `ibCommission`, each converted to the budget's currency as the entry's income is. A statement replaces the months inside its own period and leaves the others.
- `incomeBy: [{ "month": "2026-10", "key": "", "symbol": "", "name": "", "dividends": 0, "tax": 0 }]`: from `CashTransactions` rows that name a security (dividends, payments in lieu, withholding tax), keyed as the merged holdings are (ISIN, else conid, else symbol and currency). The last 36 months are kept.

On the wire:
- `total.realised`, each `accounts[].realised` and each `months[].realised`: realised gains in the range, `null` where no account concerned has trades.
- `income.months[].commissions`: `null` where not known by month. `income.commissions` (the statement totals) then lists only the accounts without trades by month.
- `income.byHolding: [{ "key", "symbol", "name", "dividends", "tax", "value": null, "yield": null }]`: dividends in the range by holding, largest first; `value` is today's value where still held, `yield` the dividends over it.
- `income.byAccount[].commissions`: the account's trade commissions in the range, by month where known, else its statement's total, else null.

On screen: Performance's Gain tile notes "Realised {amount}" when known. Income and costs draws commissions by month as a series and a column when known, and gains a "By holding" list (ten, then "Show all").

## Appendix B. Wording (English and Arabic, the same on all three apps)

The notes that name accounts are worded so they read the same for one account or several. A holding with no known kind is called "Other investments" in Allocation, as on the Balance sheet (the engine's keys "Holdings" and "Other holdings"). Phrases that already exist in an app (Value, Gain, Return, Money put in, Holdings, Cash, Largest holding, Income and costs, Dividends and interest, Prices from …, the kinds' names, the notes about estimated days and missing statement sections) are reused from the Holdings page as they are. New ones:

| English | Arabic |
|---|---|
| Performance | الأداء |
| How your investments did: their value, what you put in, and what they gained. | أداء استثماراتك: قيمتها وما أودعته فيها وما ربحته. |
| Allocation | التوزيع |
| What your investments are in today, across all your accounts. | فيمَ تتوزع استثماراتك اليوم في جميع حساباتك. |
| Dividends and interest earned, and the tax and fees paid. | الأرباح الموزعة والفوائد المكتسبة، والضرائب والرسوم المدفوعة. |
| Markets | الأسواق |
| Everything else | كل ما عدا ذلك |
| From markets | من الأسواق |
| {share} of the change | أي {share} من التغيّر |
| Value now | القيمة الآن |
| {rate} a year | بمعدل {rate} سنويًا |
| Gain each month | الربح كل شهر |
| By account | حسب الحساب |
| By kind | حسب النوع |
| By currency | حسب العملة |
| By month | حسب الشهر |
| Kept up to date by | يُحدَّث عبر |
| Start | البداية |
| End | النهاية |
| Put in | المودَع |
| Top five holdings | أكبر خمس حيازات |
| Largest holdings | أكبر الحيازات |
| Share | الحصة |
| Unrealised gain | الربح غير المحقق |
| Return on cost | العائد على التكلفة |
| Show all {count} | عرض الكل ({count}) |
| Today's holdings. What was held on past days isn't kept. | حيازات اليوم. لا يُحتفظ بما كان في الحسابات في أيام سابقة. |
| Net | الصافي |
| Yield | نسبة العائد |
| Income over the average value | الدخل مقسومًا على متوسط القيمة |
| Valued only when updated, so a gain shows in the month of the next update: {names}. | تُقيَّم عند التحديث فقط، لذا يظهر الربح في شهر التحديث التالي: {names}. |
| Not counted here, because nothing keeps them up to date: {names}. | غير محتسبة هنا لأن لا شيء يحدّثها: {names}. |
| No income or costs reported: {names}. | لا بيانات عن الدخل أو التكاليف: {names}. |
| Other investments | استثمارات أخرى |
| Cash at {name} | نقد لدى {name} |
| Accrued income at {name} | دخل مستحق لدى {name} |
| Since prices from {date} | منذ أسعار {date} |
| Start and change not shown by type, because what was held then isn't kept: {names}. | البداية والتغيّر لا يظهران حسب النوع لأن ما كان في الحساب حينها لا يُحتفظ به: {names}. |
| Not broken down, because what was held then isn't kept: {names}. | غير مفصّلة لأن ما كان في الحساب حينها لا يُحتفظ به: {names}. |
| Investments changed by {change}: {put} put in, {gain} from markets. | تغيّرت الاستثمارات بمقدار {change}: {put} مودَعة و{gain} من الأسواق. |
| Realised {amount} | المحقق {amount} |
| By holding | حسب الحيازة |
