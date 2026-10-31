# Consolidation review — 1 November 2026

The review combines the Android preview work and the remaining desktop/import fixes with
`main` at `926e3e7`. Future builds should start from the merged `main` checkout.

## Inventory and preservation

- The original checkout was at `068a347`, 17 commits behind remote main. Its tracked and
  untracked source changes were preserved as `0f2dd1f` on
  `codex/unmerged-snapshot-20261031`, using an alternate index without changing that checkout.
- The tested Android changes were committed as `ed2ef56`, then the preserved source changes
  were merged into that branch. Existing Sync v2 lifecycle, library, and translation changes
  were retained when resolving conflicts with older copies.
- Website branding, transfer/import engine work, report filters, and earlier Sync work in the
  old checkout were already present upstream. Their duplicate copies introduce no new changes.
- The iOS StoreKit branch was already integrated by `58fd171`; the iOS type-check branch's
  three fixes were integrated by `88bf807`, `228149f`, and `18c3ef8`.
- The historical Sync QA worktree contributed two configurable local test ports. Its browser
  profiles, synthetic databases, logs, screenshots, and local Wrangler config remain local.
  Its older release review is superseded by the tracked Sync v2 release-blocker review.

## Changes retained

- Android sender selection, explicit SMS sharing consent, LLM analysis, editable review,
  atomic import and duplicate protection; raw messages remain out of cloud budgets.
- Android ChatGPT return links, foreground token exchange, cancellation/progress reporting,
  and larger gaps between Assistant controls and suggestion buttons.
- Desktop Store updater polling, cancellation, native request lifetime management, saved-budget
  protection, diagnostics and transport tests; release workflow regression checks.
- Searchable import matching with readable descriptions, editable dates and invalid-date
  protection; categories without targets no longer ask to replenish money moved elsewhere.

## Findings resolved during consolidation

- An automatic merge retained an obsolete native-select test beside its combobox replacement.
  Removed the duplicate; the transfer destination/import identity regression remains covered.
- A canceled or superseded ChatGPT callback could persist its client registration after a newer
  attempt. Persistence now holds the attempt lock and checks identity/cancellation, with a
  regression covering the stored value after reloading settings.
- Suppressed the import match overflow note when a retained selection means every match is shown.

## Validation

- Shared Rust workspace: 639 tests passed before the additional cancellation regression;
  the affected Assistant suite then passed all 76 tests, including the new regression.
- Desktop TypeScript/Vite build passed. The UI suite passed 735 tests after removing the
  obsolete duplicate (79 files passed in the full run; the corrected import file passed on rerun).
- Native Windows updater: 20 unit tests and 6 real HTTP/signature transport tests passed.
- Web type-check and tests: 428 tests in 17 files passed.
- Real local workerd/D1/R2 protocol checks passed on an overridden port, including migration,
  replay, stale-client fences, interrupted-upload recovery, restore and key reset.
- Android unit tests, preview lint and optimized ARM64 preview assembly passed, including
  the final cancellation fix. The Windows desktop application build also passed. Hosted
  platform checks are recorded in the consolidation PR.
- Earlier synthetic emulator tests covered SMS capture/review/import, sign-in return and
  Sync v2 foreground/background lifecycle. Live ChatGPT sign-in and the Assistant spacing were
  also verified on the Fold before this consolidation; no real SMS was sent for testing.

## Remaining release checks

Merging this work does not enable production Sync v2 for accounts that have not been activated.
Automatic discovery depends on that rollout and an unlocked sync key. Microsoft Store deployment
still requires the two-version package-flight check described in `2026-10-31-updater-review.md`.
Android SMS permissions require the documented Play declaration before a store release.
This consolidation does not publish a store release or alter the production rollout.
