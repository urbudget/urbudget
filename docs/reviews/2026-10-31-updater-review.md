# Desktop updater review and repair — 31 October 2026

Reviewed and changed the desktop updater in the working tree based on `068a347`. This review
covers Microsoft Store and standalone desktop updates, saving/closing, retry policy, resource
lifetime, signature verification, and release checks. Concurrent bank-import, assistant, phone,
and sync work in the checkout is outside this change. No release was published.

## Observed incident

The installed 1.0.10 app showed **Downloading** while Windows had downloaded and staged 1.0.11.
Windows events recorded “Download Complete. The item was paused.”, `StageButDoNotInstall=1`,
and a transition to `BlockedOnUser`. This was not evidence of a network transfer still running.
The previous code awaited a Store completion notification without progress reporting or an
independent completion poll, and disabled its Store escape button during the request.

The evidence does not distinguish a Store operation that remained pending from a lost completion
notification. Both are covered by the repair. It also does not establish that every 1.0.10 copy
was affected. The installed copy subsequently became 1.0.11 through Windows; an earlier deployment
attempt had failed with `0x80073D02` while the old app was running. That recovery predates the patch
and is not a validation of the new updater.

For an affected existing Store copy, save and close UrBudget, then update it through Microsoft
Store's Library. Its external update path does not depend on UrBudget's stuck in-app state. Do
not reset or uninstall the app to recover the updater. The new behavior reaches existing users
only after a tested package containing it is released and installed.

## Repairs

| Finding | Implemented behavior |
| --- | --- |
| A downloaded Store package could leave an uninformative busy state. | Progress is shown; 100% says Windows is still being awaited. Status polling supplements completion notifications. Two minutes without advancing download progress requests cancellation; the Store button remains available. |
| Store context, collection access, and asynchronous lifetime were not managed consistently. | WinRT calls execute on the owning UI thread; the context and native operation lease live through the operation and cancellation. Collection access is completed on that thread. |
| A timed-out queued UI action could run after its caller reported failure. | An atomic dispatch ticket prevents an unstarted timed-out action from running later. Already-started work remains awaited. |
| Canceling an install is cooperative, so a timeout alone cannot safely unlock budget edits. | Installation retains the saved-budget lock until Windows reaches a terminal state. A stalled installation shows recovery controls for normal close or Microsoft Store. Downloads/checks may return an error after a short cancellation grace, but retain the native lease while settling. |
| Standalone metadata had a timeout, but the actual transfer did not. | The transfer has a separate 30-minute total deadline. Failed transfers can be retried; readiness still requires signature and signed-version verification. |
| Automatic callbacks could become stale while saving or bypass retry suppression. | Preference, readiness, and suppression are checked at entry and again after saving. Automatic attempts never fall through to a consent prompt. The exit path also rechecks policy. |
| An unfinished import could be interrupted by a manual update. | Manual installation waits for import/assistant work to finish. Existing save, backup, edit-lock, and bounded saved-close behavior remains in place. |
| Late async results could retain resources or start work after unmount. | Newly discovered standalone resources are released after unmount; live operations release them on completion. Late progress/status messages are ignored. |

Core implementation: [native Store orchestration](../../desktop/src-tauri/src/updates.rs),
[frontend policy](../../desktop/src/updates.ts), and [update controls](../../desktop/src/UpdateNotice.tsx).

## Verification

- Full desktop UI suite: **698 tests passed across 76 files**. Includes 12 new stress scenarios;
  nine initially failed before the frontend repairs. Scenarios include 40 competing operations,
  changed preferences during saving, cleanup races, signature failure after receiving all bytes,
  and an installation remaining locked through an hour of simulated cancellation delay.
- Native updater tests: **15 passed**, using real WinRT interfaces backed by fault-injection objects, plus
  the production polling/dispatch logic. They cover missing callbacks, stalled or failed status
  reads, terminal and unacknowledged cancellation, completion/cancel races, delayed UI dispatch,
  absolute deadlines, 1,000 terminal operations, and 1,000 lease acquire/release cycles. The native
  translation catalog audit also passed. The complete native desktop suite passed **20 unit tests
  and six transport tests**; its unrelated opt-in performance benchmark remains ignored by default.
- Real Tauri updater transport suite: **6 passed**. A loopback HTTP server serves four harmless
  signed bytes, stalls before headers or midway, truncates responses, returns HTTP errors, and
  serves tampered or incorrectly versioned payloads. Retry succeeds; invalid packages are rejected.
  No installer runs.
- Existing release API tests: **22 passed**, including platform routing and no-update responses.
- TypeScript and the production frontend build passed. Vite reports the existing large bundle
  advisory. `git diff --check` passed.
- The production native commands ran under the installed Store package identity through an
  isolated hidden Tauri example: five sequential checks completed, ten overlapping checks produced
  one result and nine busy rejections, and a subsequent download request returned `none`. The
  run completed in 5.668 seconds. A separate WinRT probe confirmed package 1.0.11 and no pending
  update. These probes initialize no budget services and call no installation method.

Local evidence is retained under `reports/update-audit-20261031/`: `store-native-query.jsonl`,
`store-query-filtered.jsonl`, `native-tests.log`, and the isolated WebView profile cleanup manifest. The probe source
and reproduction instructions are in [Store diagnostics](../../desktop/scripts/qa/store-probe/README.md).

## Release boundary

Automated tests and the live current-package query do **not** establish that a patched Store
package can install its successor. Before publication, use a disposable Windows test profile/VM
and a Store package flight: install package **A already containing this repair**, offer a higher
package **B**, and verify download interruption/retry, consent, saved edits/backups, installation,
and reopening. Standalone Windows/macOS installers also need the signed two-version upgrade
check. No customer's live installation should be downgraded to create this test.

The [release checklist](../../desktop/README.md#verifying-an-updater-change) records that procedure.
The Release workflow now gates installer builds on the frontend build, updater/save UI tests,
native Store tests, and real transport/signature tests. No version bump, publishing, or Store
submission was performed by this review.
