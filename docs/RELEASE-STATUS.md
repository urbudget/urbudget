# Release status

Reviewed 15 November 2026 for desktop, the website and the Microsoft Store; 12 November 2026 for Android; 29 October 2026 for iOS. Update this page after a release or a verified account milestone.

| Platform | Status |
| --- | --- |
| Desktop | Published [desktop-v1.0.12](https://github.com/urbudget/urbudget/releases/tag/desktop-v1.0.12) on 15 November 2026. Windows and universal macOS installers are live on urbudget.app, with signed updates offered to 1.0.10 and 1.0.11. The release run's updater tests, builds, notarization and installed Windows smoke checks passed. The Store updater's two-version package-flight check was **not** run before publishing (see below). Production v2 activation remains disabled. |
| Website | [Desktop 1.0.12 publication](https://github.com/urbudget/urbudget/actions/runs/43738144183) updated the downloads and signed update feeds on 15 November 2026. The site has run this repository's `web/` since 22 October: the app is free, and Sync is the only paid part at $3 a month. |
| Microsoft Store | Desktop **1.0.12 is published and fully available** in the [Microsoft Store](https://apps.microsoft.com/detail/9PMQMBZJG06R). The [read-only status check](https://github.com/urbudget/urbudget/actions/runs/43741602802) at 17:32 UTC (20:32 Riyadh) on 15 November confirms submission `1152921505702410339` is `Published`, is the last-published submission, and contains uploaded package `1.0.12.0` with full rollout, about 35 minutes after it was committed. Store copies on 1.0.11 are offered the update. |
| Android | Source version 1.0.1 (version code 1000001), package `app.urbudget`. On 12 November 2026 the [signed 1.0.1 bundle](https://github.com/urbudget/urbudget/actions/runs/43288911078) from `8a55c50` became the release on Play's closed test, superseding the 1.0.0 draft. Its SMS and foreground service declarations are saved with videos recorded on a Galaxy Z Fold, Sign in details are filled with a review account of its own and a capped assistant key, and the release has no errors. It was sent to Google for review that day, with the store listing and app content; Managed publishing is off, so the closed test goes to its testers as soon as it's approved. See [the Play listing notes](play-listing/README.md). |
| iOS | Native app source version `1.0.0`, bundle `app.urbudget`. The App Store Connect app record (6825535486) exists, and its listing, price (free), availability, age rating (4+) and privacy answers are filled in. Version 1.0.0 (build 1, from `ios-v1.0.0`) was submitted to App Review on 22 October 2026 as "UrBudget: Envelope Planner", set to **manual release**. Its privacy answers are published, and its screenshots (iPhone and iPad) and review account (`review@urbudget.app`, with complimentary Sync) are set up. It's not on the Mac App Store, by choice: Mac users get the desktop app. |

## Desktop 1.0.12

Published on 15 November 2026 from `aa25eb810f7723aeb6c2c6d0821ddf35ec57d2e1`, tagged `desktop-v1.0.12`. [PR #33](https://github.com/urbudget/urbudget/pull/33) added the assistant's local-model service (Ollama, LM Studio and other chat-completions servers); the release commit set desktop and the shared crates to 1.0.12 and closed the changelog's section. The release carries everything on `main` since 1.0.11, including the Store updater repair reviewed in [the updater review](reviews/2026-10-31-updater-review.md).

The [release run](https://github.com/urbudget/urbudget/actions/runs/43734890891) passed: updater regressions, the Windows and universal macOS builds (notarized), and the installed Windows smoke checks. App validation, Native Android, Native iOS and Sync protocol validation passed on the release commit. App validation on PR #33's own commit had failed once in `updater_transport` (a loopback connection reset on the runner); the same test passed in the release run and in App validation on the release commit.

Before publishing, the draft received release notes from the changelog's 1.0.12 summary. After the [website publication](https://github.com/urbudget/urbudget/actions/runs/43738144183):

- Windows and macOS download links redirect to the 1.0.12 installers, and Store links to the Store listing. The served Windows installer (8,388,596 bytes), macOS disk image (24,036,419 bytes) and macOS update package (24,700,757 bytes) match GitHub's SHA-256 digests.
- Windows x64 clients on 1.0.10 and 1.0.11, and macOS Apple silicon and Intel clients on 1.0.11, are offered 1.0.12 with the release's own update signatures and notes. 1.0.12 clients receive HTTP 204.

**Known gap:** the [two-version Store package-flight check](../desktop/README.md#verifying-an-updater-change) owed by the updater repair was not run; publishing went ahead by the owner's decision. Watch for reports of Store copies on 1.0.12 failing to install a later version; the external recovery is updating through Microsoft Store's Library. The standalone two-version upgrade check was also not run beyond the release run's updater tests.

The [Microsoft Store submission run](https://github.com/urbudget/urbudget/actions/runs/43738144292) committed submission `1152921505702410339` at 17:00 UTC, which entered certification with no errors. The [17:32 UTC status check](https://github.com/urbudget/urbudget/actions/runs/43741602802) confirms **Published**, this exact last-published submission, package `1.0.12.0` uploaded, full rollout and no certification reports. Version 1.0.12 is available in the [Microsoft Store](https://apps.microsoft.com/detail/9PMQMBZJG06R), where an installed 1.0.11 shows Update. Store copies reach 1.0.12 through 1.0.11's updater; 1.0.12's own Store updater is first exercised by the next release.

## Desktop 1.0.11

Published on 29 October 2026 from `a49aaf1fcf35f090adeaaaf1a6ff7ea1bbc26249`, tagged `desktop-v1.0.11`. [PR #8](https://github.com/urbudget/urbudget/pull/8) merged the reviewed release; [PR #9](https://github.com/urbudget/urbudget/pull/9) corrected inspection on elevated Windows runners, and [PR #10](https://github.com/urbudget/urbudget/pull/10) fixed the zero-height first-run titlebar found during screenshot review. The still-unpublished tag was retargeted to include that fix before the final build. Desktop is 1.0.11; Android and iOS remain 1.0.0.

The [final release run](https://github.com/urbudget/urbudget/actions/runs/40914078901) and [App validation](https://github.com/urbudget/urbudget/actions/runs/40913977262) passed on the published commit. The exact Windows installer passed all five installed-app smoke checks, including visible, unobstructed titlebar controls and native create/save/restart persistence. Screenshots were inspected. Apple accepted notarization and the macOS app was stapled. All six downloaded asset hashes match GitHub's SHA-256 digests; both updater packages verify against the existing app public key with signed version 1.0.11. The MSIX identity version is 1.0.11.0. Its executable differs from the NSIS payload only in Tauri's three-byte bundle marker; normalization produces the exact tested executable hash.

Shared Rust, [Android builds and lifecycle checks](https://github.com/urbudget/urbudget/actions/runs/40906203343), [iOS Simulator tests and unsigned device build](https://github.com/urbudget/urbudget/actions/runs/40906203041), and [sync protocol stress](https://github.com/urbudget/urbudget/actions/runs/40906202892) passed on `16ecc36`. That product code is unchanged in the final release; the subsequent product fix is the titlebar CSS declaration.

The [website publication](https://github.com/urbudget/urbudget/actions/runs/40916737361) succeeded. Public download verification confirms:

- Windows and macOS download links redirect to the 1.0.11 installers. Downloaded files match the manifest and GitHub SHA-256 digests, at 7,996,768 and 23,200,965 bytes respectively.
- Windows x64 and macOS Apple silicon/Intel clients on 1.0.10 are offered 1.0.11, with the verified package signatures and release notes. Current 1.0.11 clients receive HTTP 204.
- The release notes warn that 1.0.10 cannot open upgraded on-device storage.

The [Microsoft Store submission run](https://github.com/urbudget/urbudget/actions/runs/40916737174) succeeded and committed submission `1152921505702270583`. The [18:01 UTC status snapshot](https://github.com/urbudget/urbudget/actions/runs/40926758676) on 29 October confirms **Published**, with this exact last-published submission ID, package `1.0.11.0` uploaded, full rollout and no reported errors. Version 1.0.11 is available in the [Microsoft Store](https://apps.microsoft.com/detail/9PMQMBZJG06R). The release has reached both the website and Store; hourly monitoring is complete and its automation has been removed.

The [server deployment](https://github.com/urbudget/urbudget/actions/runs/40906203085) applied migration `0012_sync_v2.sql` successfully. Active Worker version `a7082905-e188-4d0c-b7db-8a4831de3f9a` has no `SYNC_V2_*` bindings or secrets: production activation remains disabled. Physical-device checks and staged rollout still gate mobile release and v2 activation. See [the blocker review](reviews/2026-10-29-sync-v2-release-blockers.md) for local stress evidence and remaining presentation work.

Client rollback policy: only ship a build that understands the new local library and SQLite fences. Reinstalling 1.0.10 over upgraded data cannot start the app and is not a supported rollback. Preserve the upgraded data and use a compatible repair build. The desktop 1.0.11 changelog states this limitation. Server rollback must likewise preserve v2 receipts/tombstones and use the publication pause described in [SYNC_V2.md](../web/SYNC_V2.md).

## Android bank SMS import (unreleased)

The Android source includes an opt-in Bank messages page: choose bank senders
(banks first, people's numbers apart) and how far back to collect (changeable later,
up to everything on the phone, SMS and the text-only MMS a phone moved from an iPhone
keeps, with a per-sender count to check coverage). With permission given once, opening
the page reads what's waiting, 200 texts per request, into one review that can be
stopped; codes, alerts and texts already in the budget are handled without a question,
and only new transactions and choices are left to review. Reading new texts in the
background as they arrive, with a notification, is opt-in and asks for
`POST_NOTIFICATIONS`. The assistant reads the facts and never picks
accounts; the phone matches cards to accounts by remembered choices or the digits
in account names, and one choice answers every message naming a card. Sending to
the assistant is asked once per provider and model and can be withdrawn. Model
sessions are temporary and do not create ordinary assistant chat history. A format
the assistant has confirmed often enough is read on the phone alone where the reading
needs nothing more (junk, and money from a payee the user has already named), a request
that proves too much is tried again in smaller ones, and a reading started from the page
keeps going under a foreground service when the app is left (`FOREGROUND_SERVICE_DATA_SYNC`,
which Play also asks to have declared). No SMS
is imported automatically; ticked rows across accounts commit in one undoable step.
New transactions remain uncleared. This is an unreleased source change, not a Play
submission or approval. Its `READ_SMS`/`RECEIVE_SMS` permissions require the
SMS-based money management declaration and Google Play approval before store
distribution; privacy/store disclosures must be updated for that release. See the
[Android guide](../android/README.md#bank-sms-import).

## Android fast edits (unreleased)

As of 5 November 2026 Android's edits, previews and totals run on the store's copy, on main and not yet released. iPhone followed on 11 November 2026 (unreleased, and not timed): its edits run in the store too, and its Simulator tests pass on Xcode 27.1. On the Fold, with the real budget, an edit saves in 47 to 58 ms against 450 to 710 ms before, and the changed row is on screen in 94 to 113 ms. Its exports matched before and after. See [the plan](plans/engine-edits.md#as-built) and [FOLLOW-UPS.md](FOLLOW-UPS.md#what-fast-edits-on-android-left).

## Desktop 1.0.10

The [installer build](https://github.com/urbudget/urbudget/actions/runs/40521784968) and
[app validation](https://github.com/urbudget/urbudget/actions/runs/40521770566) passed for
`c1c84aa` (PRs #3, #4 and #6, merged with merge commits), and publishing the release ran [Publish](https://github.com/urbudget/urbudget/actions/runs/40543438970)
and [Microsoft Store](https://github.com/urbudget/urbudget/actions/runs/40543438969). Before it, a review of the
release's branches kept the budget locked from its save until a Store update closes the window,
made updates wait for an import or an unsent message, and has Windows asked for a Store update's
permission only when someone's there. PR #7 (changing a
proposal on the phones) was merged after the tag, for the phones' next releases.

On urbudget.app after publishing:
- The download links redirect to the 1.0.10 Windows installer and macOS disk image. Both match their GitHub assets in size (7,558,429 and 21,974,095 bytes).
- Windows x64 and macOS (Apple silicon and Intel) clients on 1.0.9 are offered 1.0.10 with an update signature and the 1.0.10 notes. 1.0.10 clients get HTTP 204.

Not checked before publishing: installing a Store update from a packaged build with an unsaved
edit, and the exports' save panel on macOS (tried on Windows only).

## Desktop 1.0.9

The [installer build](https://github.com/urbudget/urbudget/actions/runs/40101250207) and
[app validation](https://github.com/urbudget/urbudget/actions/runs/40101238316) passed for
`081e064` (PR #5, merged with its commits), and publishing the release ran [Publish](https://github.com/urbudget/urbudget/actions/runs/40107546183)
and [Microsoft Store](https://github.com/urbudget/urbudget/actions/runs/40107546026). Before it, a review of everything
since 1.0.8 fixed what could put the wrong transactions in a budget; what it left is in
[FOLLOW-UPS.md](FOLLOW-UPS.md).

On urbudget.app after publishing:
- The download links redirect to the 1.0.9 Windows installer and macOS disk image. Both match their GitHub assets in size (7,426,398 and 21,760,921 bytes).
- Windows x64 and macOS (Apple silicon and Intel) clients on 1.0.8 are offered 1.0.9 with an update signature and the 1.0.9 notes. 1.0.9 clients get HTTP 204.

Not checked before publishing: the exports' save panel on macOS (tried on Windows only).

## Desktop 1.0.8

The [installer build](https://github.com/urbudget/urbudget/actions/runs/39684655221) and
[app validation](https://github.com/urbudget/urbudget/actions/runs/39684655298) passed for
`a2d7dcd`, and publishing the release ran [Publish](https://github.com/urbudget/urbudget/actions/runs/39686439128)
and [Microsoft Store](https://github.com/urbudget/urbudget/actions/runs/39686438937).

On urbudget.app after publishing:
- The download links redirect to the 1.0.8 Windows installer and macOS disk image. The installer's size matches the GitHub asset (6,797,945 bytes).
- Windows x64 and macOS (Apple silicon and Intel) clients on 1.0.7 are offered 1.0.8 with a valid update signature and the 1.0.8 notes. 1.0.8 clients get HTTP 204.

The Store package was attached to the Partner Center draft by hand. The workflow's `--noCommit` upload isn't processed until the submission is committed by the API (RELEASING.md).

## Earlier releases

Desktop 1.0.7 and earlier were released from the previous private repository, with tags `v1.0.7` and earlier. Their build, validation and publication runs and the local evidence under `reports/` aren't public:
- **1.0.7:** 617 UI tests and 406 Rust tests passed. Live installers matched their GitHub assets, and 1.0.6 clients were offered signed 1.0.7 updates.
- **1.0.6:** 361 Rust tests, 529 UI tests and 330 web/API tests passed. Windows, macOS, Android and iOS Simulator validation passed.

The older Android upload bundle built from `cd2efa9` is kept only as signing evidence. Build and test a new signed bundle from current `main` before any upload, and use a version code Play hasn't seen.

## Notes

The owner's private Play checklist covers the remaining account, product, review-access and real-purchase testing steps. The [mobile guide](MOBILE.md) covers the build procedure and platform limits.

These are separate actions:
- publishing a desktop release,
- uploading a mobile bundle,
- deploying the website,
- pushing source to `main`.

A source push alone doesn't update installed desktop apps.
