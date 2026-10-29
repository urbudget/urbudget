# Bank import implementation — executor handoff

Review date: 2026-10-24. Repository: `C:/Users/Admin/Documents/GitHub/urbudget`.
Branch: `import-editor`. HEAD at review: `2d99789551c47f5f28884330b5e11ed4d4d32aa3`.

This reviews the implementation and current uncommitted changes against the supplied import-editor plan. The plan was reference material, not authorization to implement, commit, push, or open a PR. No production code was changed by this review.

**The working tree changed during this review.** Several initially failing cases were fixed while testing. The status table distinguishes those from open findings; the reproduction code below preserves the evidence and regression cases. Recheck against the executor's current tree before changing code. Line numbers refer to this review snapshot and may move.

## Follow-up verification — both remaining findings fixed

Verified against HEAD `081e064461877c2edef11e1b492d3a9130a001fa`. This follow-up supersedes the open statuses at the original review cutoff below.

- **F7 resolved:** desktop `columnName` and Rust `Sheet::columns` add enough `#` characters to avoid every literal header name; both resolvers accept the resulting reference. The exact `Date,#4,Amount,Amount` case has permanent regression coverage in both layers. Fresh verification: all **26 ImportPage tests passed**, and the core regression test passed, including the selected amount `-1200` and preserved payee `Shop`.
- **F8 resolved:** `BankReviewRequest` carries request number, generation, session ID, session revision and destination account. Success/error callbacks require the same request; reset invalidates pending reviews; display and import require a preview belonging to the current import; importing also requires the current budget revision. Added Swift coverage checks file/session, revision and destination-account changes. The fix landed in `5dd136599b9634cd43d8001044583c04dd1e765e`.
- **Native validation now available:** the current commit's [Native iOS run](https://github.com/urbudget/urbudget/actions/runs/40095107689) succeeded, including the iOS Simulator test step and unsigned device build. [Native Android](https://github.com/urbudget/urbudget/actions/runs/40095107710) and [App validation](https://github.com/urbudget/urbudget/actions/runs/40095107681) also succeeded.

No unresolved finding remains from this report. This follow-up verifies the two outstanding fixes and their coverage; it is not a new exhaustive audit of every change since the original snapshot. The earlier evidence, test limitations and source hashes are retained as historical records.

## Findings and disposition

| ID | Priority | Status | Finding |
|---|---|---|---|
| F1 | P1 | Fixed during review; reproduction now passes | Repairing a foreign account's undated CSV row could import it into the selected account. |
| F2 | P1 | Fixed during review; reproduction now passes | Entering a converted amount still deducted a fee in the original currency. |
| F3 | P1 | Fixed during review; reproduction now passes | Unselected CAMT batches changed selected-batch identity and bypassed duplicate detection. |
| F4 | P1 | Fixed during review; updated race reproductions pass | Simultaneous manual/assistant responses could silently discard one successful edit. |
| F5 | P2 | Fixed during review; core and service reproductions now pass | A partial assistant column mapping was accepted but ignored. |
| F6 | P2 | Fixed during review; reproduction now passes | The assistant could not include excluded rows because its brief omitted them. |
| F7 | P2 | Fixed; desktop and Rust regressions pass in follow-up | A literal `#N` header could capture a different column. |
| F8 | P1 | Fixed; source verified, native CI passed in follow-up | iOS could accept a preview from an earlier file with the same session revision. |

P1 indicates a path to incorrect imported transactions, missed duplicate protection, or silently lost corrections. P2 indicates a functional failure that prevents a supported repair workflow.

## F8 — iOS preview acceptance lacks session identity

**Current disposition: resolved in the follow-up above.** The remainder of this section records the original finding.

**Locations:** [BankFileImportView.swift:454](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Screens/Imports/BankFileImportView.swift:454), [BankFileImportView.swift:534](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Screens/Imports/BankFileImportView.swift:534), [BankFileImportView.swift:551](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Screens/Imports/BankFileImportView.swift:551), [BankFileImportView.swift:630](C:/Users/Admin/Documents/GitHub/urbudget/ios/UrBudget/Screens/Imports/BankFileImportView.swift:630).

**Evidence level: source inspection. This was not executed in an iOS simulator on this Windows host.** The state guards expose a concrete asynchronous sequence that should be tested natively.

`startOver` increments `generation` and clears `preview`, but does not invalidate `reviews`. `runPreview` checks its request number and the session's numeric revision; it does not check generation or session ID. Fresh files normally both start at revision zero. `importRows` uses the current destination account with the stored preview and checks the budget revision, but does not verify that preview's statement belongs to the current session.

**Controlled sequence to test:**

1. Review file A at revision zero. Trigger a budget refresh so another A preview is pending; retain a controllable delayed response.
2. Choose Change file and open file B, also revision zero. B should open the editor, so no B preview has been requested yet (for example, one Ready row and one Attention row).
3. Complete A's delayed preview while B is open. If the budget has not changed again, both current guards pass: `reviews` is unchanged and B's revision is zero. A's rows become the stored preview.
4. Continue B to review. `runPreview` starts B's request without clearing that stored preview. The view and toolbar can display A's rows and enable Import while B's request is pending.
5. The import path does not independently reject the old session's rows; the budget revision check cannot distinguish the files.

**Fix direction:** Capture and validate generation, session ID, session revision and destination account for preview success/error callbacks; invalidate outstanding preview requests on reset. Clear or make old previews ineligible when starting a new review. Require matching current session identity/revision and budget revision at display and import. Apply the same guard to delayed errors so A cannot show an error in B.

**Regression coverage:** The sequence above with controllable async responses, including B in the editor, a destination-account change, a delayed old error, and a budget change during preview. An old preview must never become importable under a new file/account.

## F7 — Positional column references collide with literal header names

**Current disposition: resolved in the follow-up above.** The remainder of this section records the original finding.

**Locations:** `columnAt`, `columnName` and `withRole` in [model.ts:112](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/import/model.ts:112), and `bank_csv::position` / `Sheet::columns` in [bank_csv.rs:1063](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/bank_csv.rs:1063).

**Original bug, now fixed:** The desktop stored only header text and used `findIndex`, so choosing the fourth column of `Date,Payee,Amount,Amount` selected the third. Both Rust and desktop now support `#N` positional references for ambiguous or blank names. The original duplicate-header reproduction now passes.

**Remaining reproduced bug:** The representation does not distinguish a literal header named `#4` from the generated reference for column four. Both resolvers search literal header names before interpreting positions. With headers `['Date', '#4', 'Amount', 'Amount']`, choosing column four generates `'#4'`, then resolves to column two. The UI cannot retain the chosen role on column four, and the Rust resolver follows the same wrong-column rule.

**Reproduction:** `withRole(['Date','#4','Amount','Amount'], {date:'Date',amount:'#3'}, 3, 'amount')`, followed by `roles(...)`. The final test in Appendix C exercises this against the updated helper.

**Expected:** `roles(...)[3] === 'amount'`.

**Actual:** `null`; the Amount role is assigned to the literal `#4` header at index one.

**Impact:** In a file containing both a literal positional-looking header and an ambiguous/blank target header, selecting the intended column can read the wrong amount or produce an unrelated parsing error. This is narrower than the original duplicate-header failure, but remains a deterministic wrong-column selection.

**Fix direction:** Use an unambiguous per-session column reference consistently in role assignment, display, cell/field override handling, and assistant validation. Keep old saved unique-name mappings compatible. Do not persist an ambiguous name or an unstable positional mapping as a reusable template. If using `#N` strings, handle literal headers that themselves look like `#N` without resolving to the wrong column.

**Regression coverage:** Duplicate Date and Amount headers, multiple blank headers, header-line changes, real headers named `#4`, role display after refresh, choosing the second amount and verifying its actual imported cents, and safe remembered settings.

## F3 — Unselected CAMT batches consume identity occurrences

**Disposition: fixed during review.** The new `identities` implementation receives the statement lines and refuses to name a batch whose line has reason OtherAccount. Appendix A's round-trip reproduction now passes. The original failure and follow-up coverage are retained below.

**Location:** [bank_statement.rs:709](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/bank_statement.rs:709), in the `name` closure and its call before checking `e.reserves`.

Before the fix, `identities` named every batch using the shared transaction identity counter. `name` called `counter.identity(whole, !b.splits_today)` even when that batch belonged to an unselected account. A batch whose detail amounts do not add up has `splits_today == false`; naming it consumed a repeated-bank-ID occurrence. Consequently, another account's batch could change the selected account's aggregate ID.

**Reproduction:** Import account B's batch as one entry from a file containing only B. Then open a file containing account A followed by the same B batch and select B. Both batches have reference `BATCH`, date `2026-10-03`, total EUR 300, and child amounts EUR 100 + EUR 250. Choose “Import as one entry” for B again.

**Expected:** B is already imported, irrespective of the presence/order of A's statements.

**Original result:** The same B batch was offered with `duplicate == false`; the test reported a newly generated bank ID. The preceding unselected batch consumed the occurrence that B had in the first import. This undermined the aggregate/child marker relationship as well as ordinary duplicate detection.

**Fix direction:** Make batch naming obey selected-source membership and the same occurrence rules as imported transactions. Excluded foreign batches must not mutate the selected source's identity counter. Do not change existing IDs for clean files as a side effect. Consider explicit batch eligibility metadata instead of inferring ownership solely from the child being visited.

**Regression coverage:** Preserve IDs and duplicate status when adding/reordering unselected statements; test repeated references with equal date/total, mismatched children, ID-less batches, and aggregate-to-children/children-to-aggregate protection. Include the actual `bank_import::apply` round trip from Appendix A, not only an ID-set comparison.

## F4 — Same-render response race silently loses a successful fix

**Disposition: fixed for the reproduced schedules during review.** The latest helper follows a `WeakMap` of replaced session objects before choosing its base. That allows the next response to see an accepted predecessor before React renders it. Updated probes cover two valid replies, and a refused reply followed by a valid reply in the same React batch; both now pass. The original failure below explains the regression to retain. App-level proposal-status assertions and the other schedules listed below remain useful.

**Locations:** [model.ts:70](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/import/model.ts:70), [ImportPage.tsx:174](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/import/ImportPage.tsx:174), [App.tsx:470](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/App.tsx:470), [App.tsx:486](C:/Users/Admin/Documents/GitHub/urbudget/desktop/src/App.tsx:486).

The first `applyToLatest` retry handled a response arriving after the other change rendered, but missed two responses arriving before React rendered again. Both callers saw their original session through a ref assigned during render, and both returned a successful result. Their functional state updates then ran: the first installed its result, while the second saw `was.session !== applied.base` and silently returned `was`.

The assistant path displayed “Applied…” and marked the proposal applied based on the engine result, even if its state update was discarded. The manual path also gave no refusal for this case. A corrected amount/date could therefore remain unapplied while the user believed it succeeded.

**Reproduction:** Start a manual and assistant request from the same session, with changes to different rows. Resolve both promises inside one React `act` without an intervening render. Appendix C uses the actual `applyToLatest` and the production ref/functional-update pattern, and resolves any legitimate retry against its new base.

**Expected:** Both independent edits are retained, or the uninstalled operation is explicitly retried/refused and is not marked applied.

**Original result:** Both callers reported success (`[0, 1]`), but only the first edited row remained (`['one']`; expected two keys).

**Fix direction:** Own application and installation in one shared coordinator used by both manual and assistant routes. Serialize them or atomically install against an authoritative session reference and retry when installation loses the race. Return success only after the result is actually installed. A compare-and-set that silently drops the result is insufficient. Preserve engine line-version/structure conflict checks during retry.

**Regression coverage:** Both completion orders; responses in one React batch; unrelated-row merge; same-row conflict; structure change; switching files while a response is pending. Add an App-level assertion that the proposal is marked applied only when its changes are present. Appendix C is a focused race reproduction, not an end-to-end App test.

## F6 — Assistant “include” is unreachable for genuine excluded rows

**Disposition: fixed for the reproduced case during review.** `Brief.left_out` now contains up to 20 includable rows, the prompt exposes those rows, and the tool accepts their IDs. Appendix B's real-tool reproduction now passes. The original failure and additional coverage are retained below; rows beyond the bound still need an intentional discovery/selection workflow.

**Locations:** [bank_statement.rs:1039](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-core/src/bank_statement.rs:1039), [tools.rs:1461](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/assistant/tools.rs:1461), [tools.rs:1505](C:/Users/Admin/Documents/GitHub/urbudget/crates/urbudget-services/src/assistant/tools.rs:1505).

Before the fix, `Statement::brief` supplied only Attention lines and Ready samples. `fix_import` accepted line IDs only from those two collections, yet `include: true` required a `LeftOut` line with an includable reason. No genuine brief entry could satisfy the inclusion path.

**Reproduction:** Open a CSV with a Booked Shop row and a Pending Cinema row. Build the actual brief, then call `fix_import` with Cinema's real line ID, `include: true`, and the user's explicit reason.

**Expected:** A scoped proposal carrying that excluded row's captured line version.

**Original result:** No proposal. The tool returned `Line 55 isn't one the import's note shows. Fix only those.` This was reproduced through `tools::call`, not a manually constructed brief.

**Fix direction:** Include a bounded set of relevant excluded rows in the assistant context, with ID, version, reason and allowed fields, and teach proposal validation to use it. Preserve memo-sharing controls and payload limits. If there are more rows than the bound, provide a deliberate way to request/select the needed rows. Keep OtherAccount, RepeatedHeader, Zero, Corrected and InWholeBatch non-includable as specified.

**Regression coverage:** Pending, undated, and foreign-currency rows using a real generated brief. Foreign currency still requires a valid converted amount. Non-includable reasons remain refused; stale line versions and include-then-exclude races remain refused.

## Cases fixed during this review

### F1 — Foreign account's undated row

The original `elsewhere` check required `dated(original)` before recognizing another account. With source A selected, a B row whose original date was `n/a` was classified NoDate; editing its date made it Ready under A. Appendix A initially produced three imported rows instead of A's two.

The working-tree change now checks the original nonempty account value independently of whether the date reads. The exact reproduction now passes. Keep regression coverage for both a desktop cell repair and native/assistant field repair plus include; selecting B must still make the same row repairable under B.

### F2 — Converted amount minus unconverted fee

A GBP row in an EUR budget with amount `-9.99` and fee `1.00`, explicitly included and edited to `-11.60` EUR, originally previewed as `-12.60` EUR. A field override bypassed the fee, but an equivalent grid edit did not. Appendix A initially returned `-1260`, expected `-1160` cents.

The working-tree change now recognizes a converted amount cell and skips the raw fee deduction. The exact reproduction now passes. Retain cross-path parity tests and add direction/flip-sign, separate inflow/outflow, malformed-fee and conversion-revert coverage. Passing the fee case alone does not prove those combinations.

### F5 — Accepted partial mapping was ignored

On a newly auto-detected session, `structure.columns` is unset. An assistant request containing only `readAs.columns.amount = 'Correct Amount'` built a mapping with no date. The CSV engine previously discarded any such mapping, so both the core and actual assistant-tool reproduction previewed `-1000` instead of `-1200` cents despite successful application.

The working-tree change now merges a partial mapping with the detected/saved mapping and handles the amount-versus-inflow/outflow choice. Both the core and real assistant-tool reproductions now pass. Preserve both tests. Add partial mappings over existing saved mappings, absent requested headers, and mutually exclusive amount/inflow/outflow roles.

### Earlier corrections checked

The earlier cell-versus-field override bug, missing trailing cells, and Continue gating on unresolved sign interpretation have corresponding implementation changes and tests. They are not repeated as open findings here. The new desktop concurrency helper also passes the updated same-batch response probes described in F4.

## Validation and limits

- Desktop existing suite: **69 files / 571 tests passed** in the final run after incoming fixes and removal of temporary probes (the earlier run had 569 tests).
- TypeScript: `npx tsc --noEmit -p tsconfig.json` passed.
- Core import-focused unit tests: initially **78 passed**, then **80 passed** after the incoming fixes (`cargo test --locked -p urbudget-core bank_ --lib --quiet`).
- Core fixture/fault integration tests: **4 passed** across `bank_files` and `faults`.
- Assistant tool unit tests: initially **12 passed**, then **13 passed** after incoming fixes (`cargo test --locked -p urbudget-services assistant::tools:: --lib --quiet`).
- Initial targeted reproductions: **8 expected failures across 7 distinct findings**; F5 was exercised at both core and service boundaries. F8 is separate and static-only.
- After incoming fixes, the core reproductions became **4 passed / 0 failed**, and service reproductions **2 passed / 0 failed**. The updated desktop probes produced **3 passed / 1 failed**: both race schedules and ordinary duplicate headers passed; the literal `#4` collision failed. These tests use the actual helper, with a harness matching the production React state installation pattern.
- A broad assistant test run did **not** complete successfully: multiple tests failed and an auth callback test remained running; the run was interrupted. An isolated history test reproduced `Access is denied. (os error 5)` at `assistant/history.rs:255`. Do not interpret the tool-unit passes as a fully green services suite or assume every broad-suite failure has the same cause.
- Native code was inspected, but Android/iOS were not built or exercised on a device/simulator in this review. The export/save-dialog changes also present in the working tree are outside this import-focused sign-off.
- This review concentrated on source/currency boundaries, identity and duplicate protection, edits/revisions, assistant proposal construction, desktop state races, native preview lifecycle, and existing import/refund tests. Passing fixtures do not cover the cross-feature interactions above.

## Original executor workflow (completed findings retained for reference)

1. Re-run the reproductions against the current tree; work was arriving during review. Promote useful cases into the appropriate permanent test modules rather than treating this document's snapshot as current truth.
2. Validate and fix the remaining P1 native preview case (F8), rejecting stale previews at completion, display and import.
3. Remove the column-reference ambiguity (F7); preserve the cases already fixed (F1–F6) and their compatibility checks.
4. Run the focused suites, normal desktop checks and native CI, and record any environment failures separately from assertion failures. Do not regenerate ID snapshots just to make regressions pass.

## Reproduction appendix

The following are the exact temporary probes used for this review. They are included here so this Markdown file can be handed to another agent by itself. Tests assert the desired behavior, so an open issue produces an assertion failure. Temporary source files are removed after recording them; no failing probes are left in the production test suite.

From the repository root, create the files at the paths shown below. Run Rust commands from `crates` and the Vitest command from `desktop`:

```text
cargo test --locked -p urbudget-core --test review_handoff_probe --quiet
cargo test --locked -p urbudget-services --test review_handoff_probe --quiet
npx vitest run src/import/ReviewHandoffFinalProbe.test.tsx --reporter=verbose
```

### A — Core probes (F1, F2, F3, F5)

Path: crates/urbudget-core/tests/review_handoff_probe.rs

```rust
mod common;
use common::budget;
use urbudget_core::{bank_import, bank_statement::*};
use serde_json::json;

fn edit(file: &str, s: &Session, change: serde_json::Value) -> Session {
    let mut f: Fixes = serde_json::from_value(change).unwrap();
    f.scope = s.scope.clone(); f.base_revision = s.revision; f.base_structure = s.structure.clone();
    let applied = apply_fixes(&budget("EUR"), "main", file, s, &f).unwrap();
    assert!(applied.results.iter().all(|r| r.applied), "{:?}", applied.results);
    applied.session
}
fn session(file: &str) -> Session { start(&budget("EUR"), "main", file, "s", "b", None).unwrap() }

#[test]
fn foreign_account_date_repair_stays_foreign() {
    let file = "Account,Date,Payee,Amount\nA,2026-10-01,Shop A,-1\nA,2026-10-02,Shop A2,-2\nB,2026-10-03,Shop B,-3\nB,2026-10-04,Shop B2,-4\nB,n/a,Missed B,-5\n";
    let s = session(file);
    let st = statement(&budget("EUR"), "main", file, &s).unwrap();
    assert_eq!((st.sources.len(), st.source), (2, Some(0)));
    let s = edit(file, &s, json!({"cells":[{"line":st.lines[4].id,"lineVersion":0,"column":1,"from":"n/a","to":"2026-10-05"}]}));
    assert_eq!(preview_statement(&budget("EUR"), "main", file, &s).unwrap().review.items.len(), 2);
}

#[test]
fn converted_amount_does_not_deduct_foreign_fee() {
    let file = "Date,Payee,Amount,Fee,Currency\n2026-10-01,Bakery,-3.20,0,EUR\n2026-10-02,London,-9.99,1.00,GBP\n";
    let s = session(file);
    let id = statement(&budget("EUR"), "main", file, &s).unwrap().lines[1].id.clone();
    let s = edit(file, &s, json!({"include":[{"line":id,"include":true}]}));
    let s = edit(file, &s, json!({"cells":[{"line":id,"lineVersion":1,"column":2,"from":"-9.99","to":"-11.60"}]}));
    assert_eq!(preview_statement(&budget("EUR"), "main", file, &s).unwrap().review.items[1].row.amount, -1160);
}

#[test]
fn partial_column_mapping_is_honored() {
    let file = "Date,Payee,Amount,Correct Amount\n2026-10-01,Shop,-10.00,-12.00\n";
    let s = session(file);
    let mut structure = s.structure.clone();
    structure.columns = Some(urbudget_core::bank_csv::Columns { amount: Some("Correct Amount".into()), ..Default::default() });
    let s = edit(file, &s, json!({"structure":structure}));
    assert_eq!(preview_statement(&budget("EUR"), "main", file, &s).unwrap().review.items[0].row.amount, -1200);
}

fn camt(account: &str, amount: &str) -> String {
    format!(r#"<Stmt><Acct><Id><IBAN>{account}</IBAN></Id><Ccy>EUR</Ccy></Acct><Ntry><Amt Ccy="EUR">300.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts>BOOK</Sts><BookgDt><Dt>2026-10-03</Dt></BookgDt><AcctSvcrRef>BATCH</AcctSvcrRef><NtryDtls><TxDtls><Refs><EndToEndId>A</EndToEndId></Refs><AmtDtls><TxAmt><Amt Ccy="EUR">100.00</Amt></TxAmt></AmtDtls></TxDtls><TxDtls><Refs><EndToEndId>B</EndToEndId></Refs><AmtDtls><TxAmt><Amt Ccy="EUR">{amount}</Amt></TxAmt></AmtDtls></TxDtls></NtryDtls></Ntry></Stmt>"#)
}
#[test]
fn unselected_batches_do_not_change_selected_whole_id() {
    let alone = format!("<Document><BkToCstmrStmt>{}</BkToCstmrStmt></Document>", camt("B", "250.00"));
    let all = format!("<Document><BkToCstmrStmt>{}{}</BkToCstmrStmt></Document>", camt("A", "250.00"), camt("B", "250.00"));
    let first = session(&alone);
    let mut structure = first.structure.clone(); structure.source = Some("B".into()); structure.whole_batches = vec!["e1".into()];
    let first = edit(&alone, &first, json!({"structure":structure}));
    let p = preview_statement(&budget("EUR"), "main", &alone, &first).unwrap();
    let after = bank_import::apply(budget("EUR"), "main", p.review.items.into_iter().map(|i| bank_import::Choice { row: i.row, action:"new".into(), match_id:None }).collect()).unwrap();
    let second = session(&all);
    let mut structure = second.structure.clone(); structure.source = Some("B".into()); structure.whole_batches = vec!["e2".into()];
    let second = edit(&all, &second, json!({"structure":structure}));
    let p = preview_statement(&after, "main", &all, &second).unwrap();
    assert_eq!(p.review.items.len(), 1);
    assert!(p.review.items[0].row.duplicate, "Same batch offered again as {}", p.review.items[0].row.id);
}

```


### B — Real assistant-tool probes (F5, F6)

Path: crates/urbudget-services/tests/review_handoff_probe.rs

```rust
#[path = "../../urbudget-core/tests/common/mod.rs"] mod common;
use urbudget_core::bank_statement::*;
use urbudget_services::assistant::{attachments::Files, tools::{call, Context}};
use serde_json::{json, Value};

#[test]
fn assistant_amount_column_change_changes_amount() {
    let b = common::budget("EUR");
    let file = "Date,Payee,Amount,Correct Amount\n2026-10-01,Shop,-10.00,-12.00\n";
    let s = start(&b, "main", file, "s", "b", None).unwrap();
    let brief = statement(&b, "main", file, &s).unwrap().brief(true);
    let files = Files::empty();
    let ctx = Context { budget: &b, today: "2026-10-24", month: "2026-10", share_memos: true, files: &files, bank_sync: &Value::Null, import: Some(&brief) };
    let outcome = call("fix_import", &json!({"readAs":{"columns":{"amount":"Correct Amount"}},"note":"Use the corrected amount column"}), &ctx);
    let proposal = outcome.proposal.expect("valid column-change proposal");
    let applied = apply_fixes(&b, "main", file, &s, proposal.fixes.as_ref().unwrap()).unwrap();
    assert!(applied.results.iter().all(|r| r.applied));
    assert_eq!(preview_statement(&b, "main", file, &applied.session).unwrap().review.items[0].row.amount, -1200);
}

#[test]
fn assistant_can_include_pending_row() {
    let b = common::budget("EUR");
    let file = "Date,Payee,Amount,Status\n2026-10-01,Shop,-10.00,Booked\n2026-10-02,Cinema,-12.00,Pending\n";
    let s = start(&b, "main", file, "s", "b", None).unwrap();
    let st = statement(&b, "main", file, &s).unwrap();
    let brief = st.brief(true);
    let files = Files::empty();
    let ctx = Context { budget: &b, today: "2026-10-24", month: "2026-10", share_memos: true, files: &files, bank_sync: &Value::Null, import: Some(&brief) };
    let outcome = call("fix_import", &json!({"include":[{"line":st.lines[1].id,"include":true,"reason":"User explicitly requested this pending row"}],"note":"Include requested row"}), &ctx);
    assert!(outcome.proposal.is_some(), "{}", outcome.output);
}

```


### C — Desktop probes after incoming fixes (F4, F7)

Path: desktop/src/import/ReviewHandoffFinalProbe.test.tsx

```tsx
import { afterEach, expect, it } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { applyToLatest, roles, withRole } from "./model";
import type { BankSession, FixResult } from "../bridge";
afterEach(cleanup);

async function simultaneous(firstRefused: boolean, id: string) {
  const initial: BankSession = {
    scope: { session: id, budget: "b", account: "checking", file: "hash" }, revision: 0,
    structure: { format: "csv", dateOrder: { order: "ymd", confirmed: true }, flipSigns: false },
    edits: { cells: {}, fields: {}, include: [] }, versions: {},
  };
  const pending: Array<() => void> = [];
  const completed: number[] = [];
  function Harness() {
    const [session, setSession] = useState(initial);
    const latest = useRef(session); latest.current = session;
    const start = async (index: number) => {
      let calls = 0;
      const applied = await applyToLatest(() => latest.current, (base) => {
        const refused = index === 0 && firstRefused;
        const result = {
          session: refused ? { ...base } : { ...base, revision: base.revision + 1, edits: { ...base.edits, fields: { ...base.edits.fields, [String(index)]: { amount: "-12" } } } },
          results: [{ fix: "field", applied: !refused }] as FixResult[],
        };
        if (calls++ > 0) return Promise.resolve(result);
        return new Promise((resolve) => { pending[index] = () => resolve(result); });
      });
      if (!applied) return;
      setSession((was) => was === applied.base ? applied.session : was);
      completed.push(index);
    };
    return <><button onClick={() => void start(0)}>First</button><button onClick={() => void start(1)}>Second</button><output>{JSON.stringify(session)}</output></>;
  }
  const { container } = render(<Harness />);
  fireEvent.click(screen.getByText("First")); fireEvent.click(screen.getByText("Second"));
  await act(async () => { pending[0](); pending[1](); });
  expect(completed.sort()).toEqual([0, 1]);
  return JSON.parse(container.querySelector("output")!.textContent!) as BankSession;
}

it("merges two valid responses before a render", async () => {
  expect(Object.keys((await simultaneous(false, "valid-valid-final")).edits.fields)).toHaveLength(2);
});
it("a refused response must not cause a valid concurrent fix to be discarded", async () => {
  expect(Object.keys((await simultaneous(true, "refused-valid-final")).edits.fields)).toEqual(["1"]);
});
it("selects the second duplicate header", () => {
  const headers = ["Date", "Payee", "Amount", "Amount"];
  expect(roles(headers, withRole(headers, { date: "Date", amount: "Amount" }, 3, "amount"))[3]).toBe("amount");
});
it("a literal positional-looking header must not capture another column", () => {
  const headers = ["Date", "#4", "Amount", "Amount"];
  expect(roles(headers, withRole(headers, { date: "Date", amount: "#3" }, 3, "amount"))[3]).toBe("amount");
});

```

## Source snapshot at handoff

Captured: 2026-10-24 09:38:04 UTC. These SHA-256 hashes identify the files inspected at the final cutoff; later work can supersede the dispositions above.

```text
459212AA9833121D1914DEFB65E5529F47B715B9F5675522CC595F690D93E8B3  crates/urbudget-core/src/bank_csv.rs
6189342D8BB57CE461E432D2500739E0D41446C45A75CD15DB37FFDA3741AC58  crates/urbudget-core/src/bank_statement.rs
12F6BC313EA0BF4FA30D8EDC8CAB00237D95DA30F6D59E398CF33C5B6B8CD01C  crates/urbudget-services/src/assistant/tools.rs
B0F3063FAFA35C62912156E7106B0E5F5043895C606992C2CFC1FF49BEF14FD8  crates/urbudget-services/src/assistant/prompt.rs
492F90EF2065172F3D74A0E354AF1C74AA7011C3B5F27CD5A3EB752EB9092278  desktop/src/App.tsx
135DF65DA0A714736B219CF8C1D906FE783E6EA54517F5F15166DCA23CCA8A6E  desktop/src/import/ImportPage.tsx
54D471E94FBD2285E284A8DE1E227EEF849EE6BBC595DC13B79C2A1EFF88C91B  desktop/src/import/model.ts
FE9D5448670719924115ECD809FD9B1D44D4EB07CA15480E311AE30A74AC262B  ios/UrBudget/Screens/Imports/BankFileImportView.swift
```
