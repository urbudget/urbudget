# Rust budget engine

The accounting modules now live in `crates/urbudget-core`. Desktop re-exports their
public API, and the native Android client calls the same crate through
`crates/urbudget-android` (JNI). `crates/urbudget-services` contains the shared
SQLite application runtime, cloud account and encrypted sync protocols, Play
subscription verification, vault, bank networking and assistant. Run current
shared tests from the repository root:
`cargo test --manifest-path crates/Cargo.toml`. Desktop's `npm test` includes both
the shared suite and the remaining Tauri tests. The dated audit commands below
describe their original source layout.

The Rust engine implements a cash-backed envelope budget with YNAB-style credit card payment
categories. Actual Budget is a useful comparison, but its credit-card and debt model is different.
Matching Actual's results is therefore not a substitute for checking YNAB behavior.

This guide describes the current implementation. The audit observations and performance
measurement below are dated **2026-10-20**. They establish coverage for the documented scenarios,
not complete equivalence to every YNAB feature or imported budget.

## Calculation path

| Module | Responsibility |
| --- | --- |
| [domain.rs](../crates/urbudget-core/src/domain.rs) | Budget records, integer money, input validation, account/category summaries and reconciliation. |
| [ledger.rs](../crates/urbudget-core/src/ledger.rs) | Borrow ordinary transactions; expand transactions containing split transfers into accounting legs; classify income, expenses, opening entries and uncategorized outflows. |
| [engine.rs](../crates/urbudget-core/src/engine.rs) | Replay monthly activity, reserve funded credit purchases, roll categories forward and derive Ready to Assign. |
| [targets.rs](../crates/urbudget-core/src/targets.rs) | Calculate target funding from assignments, available balances, recurrence dates and the reference month. |
| [schedules.rs](../crates/urbudget-core/src/schedules.rs) | Validate recurring transactions and calculate upcoming category requirements. |
| [workflows.rs](../crates/urbudget-core/src/workflows.rs) | Preview and apply allocation workflows using engine results. |
| [commands.rs](../crates/urbudget-core/src/commands.rs) | The engine's methods by name: `apply` for those that change a budget, `read` for those that don't, `apply_steps` for a list of them. The stateless engine (`invoke_json`) and the services' `edit_budget` and `query_budget` all call these, so a method can't behave differently on the two paths. |
| [ynab.rs](../crates/urbudget-core/src/ynab.rs) | Import/export conversion and comparisons against source account and category-month totals. |
| [store.rs](../crates/urbudget-core/src/store.rs) | SQLite persistence, row-level commits, revision checks and cached budget drafts. Persistence is separate from envelope arithmetic. |

`calculate` produces a selected month's summary. `calculate_history` produces the monthly
history through that month. Both validate the budget, expand the ledger, index accounts and
categories, and replay from the earliest relevant transaction or assignment month. Assignments
are grouped by month, and target assignment totals use a cumulative index instead of repeatedly
scanning the complete assignment list.

The selected-month path builds only its final summary. Refill targets retain compact prior
availability; full history evaluates schedules through one recurrence index. Credit funding
matrices contain only credit accounts, monthly buffers are reused, and sorting reads each
transaction's creation timestamp once. Reports sweep one chronological expanded ledger.
Derived native reads clone one immutable draft snapshot and release the persistence lock before
calculating. Obsolete queued UI reads are skipped without reordering writes or saves.

Money uses signed integer cents. The YNAB importer converts milliunits only when the amount is
exactly representable in cents; it rejects sub-cent values instead of rounding them. Validation
checks dates, IDs, references, split totals, record limits and the supported numeric range.

Account summaries deliberately report working and cleared balances across the whole recorded
ledger. Monthly category calculations use balances through the selected month. An account
balance shown beside a past budget month is therefore not that month's closing balance.

Within each month, cash activity gets funding priority. Remaining category funds reserve cash
for credit purchases in transaction order, after accounting for refunds. Credit purchases
reduce the spending envelope; the funded portion increases the related payment envelope.
Tracking-account transfers use the account on the budget side to determine whether the movement
is cash or credit, regardless of which side stores the transaction.

The main accounting relationships are:

```text
category available = positive carry from last month + assigned + activity
budget cash = cash-account balances + positive on-budget credit balances
raw Ready to Assign = budget cash - total category available - credit overspending
next month's category carry = max(category available, 0)
```

Keeping current cash overspending negative until rollover postpones its Ready to Assign impact
until the next month. Subtracting current credit overspending separately prevents unfunded
credit purchases from creating spendable cash. Credit payment categories can themselves become
negative when payments exceed their reserve.

For every month, past ones included, `calculate_at` also reserves amounts assigned in later
months, as YNAB's budget screen does: it shows `max(0, ready - future)` when the later months'
total is positive and `ready` is not negative, and `ready` otherwise. It exposes both the
displayed Ready to Assign and `future_ready_to_assign`, which can reveal a future shortfall even
when the displayed current amount is zero. `calculate_history` returns the underlying monthly
figures before this selected-month presentation adjustment; those are what YNAB's API reports
as `to_be_budgeted`, so the import audit compares them.

`future_shortfalls` separately checks the cumulative assignments at every future month. A
later negative assignment cannot hide an earlier shortage. Global auto-assignment reserves the
largest future cumulative obligation; future transactions are not treated as money held now.
Shortfalls are only reported from the calculation month on: before it, money that came in later
funds the later months. A past month's assignment is limited to the Ready to Assign it shows.

One reference month now controls both future assignments and refill targets. An explicit
`calculate_at(..., as_of)` takes precedence; otherwise the engine uses `calculationAsOfMonth`
metadata when present, or the local current month. `needed_indexed_at` receives that same
reference month. This makes tests and historical simulations independent of the wall clock.
The UI shares a reference-month clock and refreshes on month boundaries, focus and visibility.
Summaries and previews are keyed by budget identity, selected month and reference month. Pending
same-month rows are inert; failed or changed-month results cannot supply financial actions.
Quick Assign confirmation displays a native calculated result, including future shortages.
Applying it checks and temporarily freezes its reference month, then restores the original
metadata. Queuing a confirmation across rollover requires a refreshed preview.

## YNAB and Actual Budget comparison

The following comparisons use official documentation reviewed on 2026-10-20. Actual's envelope
mode is the comparison unless stated otherwise.

| Area | YNAB | Actual Budget | Engine approach |
| --- | --- | --- | --- |
| Positive category carry | Unspent Available carries forward. | Positive category balances carry forward. | Preserves positive carry. |
| Negative category carry | Cash overspending reduces next month's Ready to Assign; credit overspending becomes a payment shortfall. Negative category Available resets. | Overspending normally reduces next month's To Budget, but a category can explicitly roll negative balances forward. | Uses the YNAB distinction; no per-category negative-rollover mode. |
| Credit purchases | Funded purchases move cash into a payment category. | Credit accounts do not require special payment categories; their negative balances already reduce on-budget funds. | Uses paired payment categories and separately tracks unfunded credit spending. |
| Existing card debt | Assign directly to the payment category to fund repayment. | A negative debt category with rollover can represent carried debt. | Keeps account debt separate from payment cash. |
| Future planning | Assign existing money into future months; future refill targets do not count still-spendable carry. | Supports holding income for the next month. A separate tracking-budget mode permits forecasting income. | Uses existing cash and future assignments, not forecast income. |

The rollover rules come from [YNAB's monthly rollover guide](https://support.ynab.com/en_us/when-the-month-rolls-over-a-guide-rkyyd6qC9)
and [Actual's budgeting guide](https://actualbudget.org/docs/budgeting/). The different card models
are described in [YNAB's credit-card guide](https://support.ynab.com/en_us/handling-credit-cards-overview-ry7cNub1s),
[Actual's credit-card overview](https://actualbudget.org/docs/budgeting/credit-cards/) and
[Actual's debt workflow](https://actualbudget.org/docs/budgeting/credit-cards/carrying-debt/).
See [Actual's tracking-budget documentation](https://actualbudget.org/docs/getting-started/tracking-budget/)
for its separate forecasting mode.

Additional YNAB rules guide the edge cases:

- Cash spending has priority when cash and credit share an underfunded category.
  [Overspending guide](https://support.ynab.com/overspending-in-ynab-a-guide-ryWoxEyi).
- A positive credit balance acts as cash. Spending that positive portion must not also reserve
  another card payment. Transferring a returned credit balance to checking does not add it to
  the budget a second time.
  [Positive card balances](https://support.ynab.com/en_us/credit-cards-with-a-positive-balance-an-overview-By_H6uzJo).
- A debt balance transfer between credit cards does not automatically move either payment
  category. A cash advance to a cash account raises Ready to Assign without funding repayment.
  [Balance transfers](https://support.ynab.com/en_us/how-to-make-a-credit-card-balance-transfer-ry5ETLWJo),
  [cash advances](https://support.ynab.com/en_us/credit-card-cash-advances-an-overview-Hy6PmlOC9).
- A categorized refund restores its spending category and can reduce the payment category below
  zero. A statement credit categorized to Ready to Assign reduces debt without automatically
  releasing payment reserves. Excess reserves can be moved manually.
  [Refunds](https://support.ynab.com/en_us/credit-card-refunds-and-returns-H1J7qDWkj),
  [statement credits](https://support.ynab.com/en_us/credit-card-rewards-and-statement-credits-a-guide-ryoRzYY0q).
- Refill targets ignore prior-month carry while viewing a future month. Set-aside targets ask
  for fresh funding, and weekly requirements depend on the chosen weekday's occurrences.
  [Target types](https://support.ynab.com/en_us/getting-started-with-targets-ryAEP08xC).
- Underfunded auto-assignment also considers payment categories with an unfunded debt balance
  and no target. These are funded after spending priorities; a configured payment target can
  instead express a debt-repayment plan.
  [Underfunded logic](https://support.ynab.com/en_us/underfunded-a-guide-BJwPhQO09).

## Audit fixes: 2026-10-20

The audit identified seven calculation defects and added regression coverage in
[engine.rs](../crates/urbudget-core/src/engine.rs), [targets.rs](../crates/urbudget-core/src/targets.rs)
and [workflows.rs](../crates/urbudget-core/src/workflows.rs):

| Defect | Corrected behavior |
| --- | --- |
| Transfers crossing the budget boundary always used cash-category arithmetic. | Use the actual budget-side account's cash or credit rules, including when the transaction is stored on the tracking side. |
| A card-to-card debt transfer was treated as a cash payment into the receiving card. | Leave both payment envelopes unchanged for manual reassignment. |
| A positive card balance repeatedly removed money from its payment reserve in otherwise idle months. | Preserve explicitly assigned payment money and positive carry until activity or an assignment changes it. |
| Reordering native transactions could change which card received funding for same-day purchases. | Use recorded creation timestamps for tied transaction dates. Legacy/date-only imports retain their existing fallback order. |
| Transfers and uncategorized outflows did not consume the positive-card cash pool used by later purchases. | Consume that cash once, so later same-day purchases correctly create debt and reserve payment money. |
| Explicit as-of calculations used a different clock for refill targets. | Pass one reference month through the entire calculation and target evaluation. |
| A payment category without a target reported no funding requirement despite uncovered debt. | Include the selected month's debt shortfall in Underfunded, respecting existing payment reserves and excluding later posted transactions. |

The no-target payment requirement uses the month being calculated, not the sidebar's all-ledger
working balance. Its workflow regression covers opening debt, funded purchases and a posted
charge dated in a future month. Explicit payment targets continue to express the user's chosen
repayment plan instead of automatically requesting the entire balance.

These fixes preserve the source budget records. They change derived balances or funding
requirements; no imported assignments or account balances are silently rewritten to force an
audit match.

## Four-stage follow-up implementation

Each stage passed its tests before work started on the next stage.

| Stage | Changes | Native gate |
| --- | --- | --- |
| 1. Accounting correctness | Enforce one valid payment category per on-budget card; reject payment-category expenses and unclassified boundary transfers; reject ambiguous imported links; preserve card/category dependencies during sync; post loan interest from either transfer orientation; prevent unsafe account-kind conversion. | 302 passed |
| 2. Funding guidance | Include scheduled card payments; expire completed dated balance targets; distinguish global and selected undated balance goals; interleave weekly deadlines; preserve scheduled/overspending warnings while snoozed; expose intermediate future shortages. | 317 passed |
| 3. Performance and calculation lifecycle | Reuse ledger, schedule and assignment indexes; retain fewer historical summaries; reuse credit-only buffers; release read locks; cancel obsolete queued reads; make rollover, retry and confirmation contexts explicit. | 326 passed |
| 4. Compatibility and hardening | Count original cash outflow transactions for Age of Money; validate loan-solver inputs and returned feasibility; reject malformed import/target dates and references before arithmetic; bound derived numeric totals and expansion work. | 354 passed |

Funding priority follows [YNAB's Underfunded guide](https://support.ynab.com/en_us/underfunded-a-guide-BJwPhQO09):
resolve an overspent category's requirement, then dated obligations and weekly checkpoints,
monthly end-of-month targets, future targets, and card payment obligations. An undated balance
goal remains available for selected-category funding. A scheduled category's whole remaining
monthly amount is funded as one obligation; using its earliest due date for ordering is a local
choice that still needs a captured YNAB comparison.

Age of Money now nets all qualifying cash legs of each original saved transaction before FIFO
aging. Expense parts and refunds in one split transaction count as one outflow; cash transfers
between budget cash accounts net to zero, while card payments consume cash. The ten-transaction
window and weighted FIFO behavior have representation-invariance regressions. Each outflow keeps
its exact weighted age; the window's ages are summed, divided by ten (also before there are ten)
and rounded to the nearest day, which matched YNAB in every full-window month of an imported
plan where rounding each age down had come out a day low in 12 of 27. Opening cash
still uses the first recorded date because its actual receipt dates are unknown, so this remains
an estimate for imported/opening balances.
[YNAB Age of Money](https://support.ynab.com/en_us/age-of-money-H1ZS84W1s).

Input protection now includes canonical four-digit dates, valid account references before
import lookup, checked CSV arithmetic, and validated consumed target metadata. Unknown optional
target fields are preserved. Loan payoff-by-date searches verify a supported feasible upper
bound and the final projection; malformed lump sums or impossible amounts return errors.
Snooze metadata accepts RFC3339 timestamps as specified by the
[official YNAB API model](https://github.com/ynab/ynab-sdk-python/blob/main/docs/CategoryBase.md),
as well as legacy local month/date values. Importing preserves the exact source timestamp;
other target date fields remain date-only.

Money remains integer cents with a per-item limit of 1,000,000,000,000 cents. Aggregate recorded
money plus target exposure, derived summary columns and scheduled monthly requirements must
stay within 9,000,000,000,000,000. Forecasts bound cumulative absolute exposure too. Resource
limits include 500,000 expanded ledger entries, 1,000,000 recorded splits, 2,000 schedules,
100 splits per template, 10,000 occurrences per schedule, 1,000,000 occurrences per calculation,
and 500,000 forecast events. Historical calculations allow at most 2,000,000 account/category
rows or credit matrix cells, and 50,000,000 credit-cell/month operations. These are conservative
implementation limits, not YNAB rules. Rejected work does not silently truncate a result.

## Split-transfer performance

The old ledger expansion cloned a complete parent transaction for every split leg. A parent
with `S` splits consequently copied its split vector `S` times, creating quadratic split-copy
work. It also repeatedly searched the account list while classifying transfer legs.

The revised [ledger expansion](../crates/urbudget-core/src/ledger.rs) constructs each leg from the
parent fields and only that leg's split. Ordinary transactions remain borrowed. An account
on-budget lookup is created lazily when split expansion needs it; transaction-level opening
classification is computed once per parent. Metadata and split-transfer statuses are retained.

The synthetic `benchmark_split_expansion` test measured **953.563 ms before and 252.970 ms after**
for 1,000 parent transactions with 100 splits each, averaging five expansions in an unoptimized
development build. That is about **3.77 times faster**, or **73.5% less elapsed time**, for this
specific expansion workload. It is not an end-to-end engine, import, UI or release-build
benchmark, and it contains no personal budget data.

From `desktop/src-tauri`, reproduce the measurement with:

```powershell
cargo test --lib ledger::tests::benchmark_split_expansion --offline -- --ignored --nocapture
```

## End-to-end performance verification

The ignored synthetic test in `desktop/src-tauri/tests/performance.rs` covers 10,000 transactions
over 60 months, 30,000 transactions over 120 months, and a schedule-heavy case with 48 schedules.
It measures selected summaries, full history, reports and filtered reports, with one warmup and
the median of three repetitions. Serialization and SHA-256 hashing are outside the timed region.
All twelve complete outputs matched their pre-optimization hashes at the stage 3 gate.

The final stage 4 benchmark used normal host access, matching the baseline execution context.
Each cell below shows **baseline → final milliseconds**:

| Synthetic workload | Selected month | Full history | Reports | Filtered reports |
| --- | --- | --- | --- | --- |
| 10,000 transactions / 60 months | 40.0 → 50.9 | 41.5 → 54.9 | 101.7 → 87.8 | 92.3 → 68.5 |
| 30,000 transactions / 120 months | 174.7 → 197.7 | 170.2 → 193.1 | 502.8 → 293.8 | 438.8 → 277.0 |
| 48 recurring schedules | 50.6 → 11.1 | 48.8 → 16.2 | 9.0 → 8.6 | 8.3 → 7.6 |

Schedule-heavy calculations and reports improved; ordinary selected/history calculations were
slower in the final run. Timings also varied between runs, with the same optimized representative
report ranging from about 59 to 132 ms. Some intermediate runs used the restricted execution
context, and concurrent machine load was not controlled. These development-build measurements
establish a repeatable schedule-work improvement, not a universal or release-build speedup.

All six selected/history hashes still match the pre-optimization baseline. The six report
fingerprints were deliberately updated after stage 4's Age of Money correction, supported by
regressions for original-transaction counting, split netting and weighted FIFO aging. The
original stage 3 hashes, both stage 4 benchmark runs, and final test logs are retained locally
under `reports/engine-stages-20261020`.

From `desktop`, run the end-to-end fixture explicitly:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test performance --offline -- --ignored --nocapture
```

The engine still replays accounting history, so an edit's totals grow with the ledger. Incremental
month caching and controlled release-profile benchmarks remain separate future work
([follow-up](FOLLOW-UPS.md#what-fast-edits-on-android-left)). Persistence is already incremental:
`Store::commit` writes the rows a `Patch` changed, and desktop's saves, Android's `edit_budget` and
undo through `patchOnly` all take that path.

An ignored benchmark prints the cost of one edit at 2,742 and 10,000 transactions. From the
repository root:

```powershell
cargo test --manifest-path crates/Cargo.toml -p urbudget-android --release --test edit_cost -- --ignored --nocapture
```

## Verification and remaining parity work

The final review exercised invalid drafts, failed refreshes and retained import metadata as
well as successful calculations. It found and corrected these additional interactions:

- A calculation error had hidden the controls needed to recover. Month navigation, Undo/Redo,
  confirmed reload and budget selection remain available without stale financial figures.
  A separately named `recovery-draft-*.json` export preserves invalid unsaved data for repair;
  ordinary backups, imports and saves still validate. Export does not alter the saved budget.
- Desktop reports, phone reports and forecasts now bind results and errors to the exact budget
  snapshot and filter/month context. A pending or rejected replacement cannot display old
  figures under new labels or open an obsolete drilldown. Retry and forecast range controls
  remain available after an error.
- Report counts and newly entered loan payments use saved transaction identity. Retained
  provider `parentId` metadata can no longer merge distinct spending counts, skip new payment
  interest, or exclude an older same-day payment from the interest balance.
- Loan helpers validate draft input before ledger expansion, date slicing or balance arithmetic.
  Malformed dates and amounts return errors without changing the existing draft or saved data.
- Scheduled imports reject unknown category references instead of silently turning them into
  Uncategorized. Known internal/Ready to Assign roles and null categories remain supported.

The audit uses synthetic regression cases for accounting changes and metadata-preservation
tests for split expansion. Target tests additionally cover monthly and weekly future refills.
Run the Rust library suite from `desktop/src-tauri` with:

```powershell
cargo test --lib --offline
```

The final-review 2026-10-20 Rust library run passed **359 tests, with zero failures and one ignored
benchmark**. All **521 frontend tests across 70 files** passed. The production frontend build and
Windows native debug build (`--no-bundle --ci --no-sign`) both succeeded. Tests cover all-field selected/history comparisons,
recurring refill balances, snapshot isolation, queue ordering, rollover, rejected stale
confirmations, transfer orientation, import failures and numeric/resource boundaries. The full
native run required normal host access for existing loopback, temporary-file and credential
fixtures. Performance fixtures are synthetic and use no personal budget data. Final-review
evidence is retained under `reports/engine-final-review-20261020`; the earlier stage gates and
performance runs remain under `reports/engine-stages-20261020`.
Both ignored synthetic benchmarks also passed when run explicitly. All twelve engine/report
output fingerprints still match the stage 4 expectations after the final-review fixes.

The subsequent 1.0.6 release review also covered the companion API and earlier UI/import
changes. It passed 361 Rust tests, 529 frontend tests, 330 API tests and 6 CI-selector tests,
plus the frontend/Windows builds and both explicit benchmarks. Bank previews now validate
raw dates and draft amounts before indexing; loan previews and payment-save retries retain
their correct input context. Release evidence is in `reports/release-1.0.6-20261020/`.

This result is not evidence that every YNAB scenario is matched. The existing `ynab::audit`
compares source account balances and monthly assignment/activity/available values. Rechecking
a known source export after changes gives stronger compatibility evidence than synthetic tests
alone.

The following areas remain unverified or unsupported:

- Date-only imported transactions do not establish intraday order. Exact allocation across
  multiple cards, refunds crossing zero or arriving on an already-positive card, and transfers
  from a positive card into another card require captured source fixtures. The documented
  general cash/debt rules do not fully specify those combinations.
- Newer YNAB behavior includes a special Credit Card Interest system category that does not
  automatically reserve payment money. The engine does not yet model an explicit equivalent
  role. Ordinary expense names must not be used to guess that role.
  [YNAB credit-card behavior](https://support.ynab.com/en_us/handling-credit-cards-overview-ry7cNub1s).
- Credit-card payoff-by-date targets depend on changing debt and payment availability. The
  generic dated-balance target implementation is not an exhaustive match for that feature.
  [Paired-account targets](https://support.ynab.com/en_us/paired-targets-BJJI8rdC5).
- First-plan-month target handling needs the plan's creation day, which this model does not
  retain. The exact priority between multiple scheduled dates in one category also needs a
  source fixture; the local implementation uses the earliest remaining due date.
- New-loan first-month interest and other lender-specific interest conventions need explicit
  account-creation/source fixtures. Current loan calculations retain the documented local
  monthly interest model; they are not a statement-by-statement lender simulator.
- Twice-a-month anchors on the 31st, Reduce Overfunding after negative assignments, and
  historical selected-month Ready to Assign across a reference-month rollover still need
  captured YNAB examples before changing their semantics.

Keep private exports and detailed local evidence outside committed documentation, following
[artifact retention](ARTIFACTS.md). Checked-in fixtures should be synthetic or deliberately
sanitized, with the expected YNAB behavior and its source stated.
