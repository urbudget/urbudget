# Release status

Reviewed 29 October 2026. Update this page after a release or a verified account milestone.

| Platform | Status |
| --- | --- |
| Desktop | Published [desktop-v1.0.11](https://github.com/urbudget/urbudget/releases/tag/desktop-v1.0.11) on 29 October 2026. Windows and universal macOS installers are live on urbudget.app, with signed updates offered to 1.0.10. Installed Windows first-run, window controls, native save and restart checks passed; macOS is notarized. Production v2 activation remains disabled. |
| Website | The [29 October deployment](https://github.com/urbudget/urbudget/actions/runs/40906203085) is live, and [desktop 1.0.11 publication](https://github.com/urbudget/urbudget/actions/runs/40916737361) updated the downloads and signed update feeds. The site has run this repository's `web/` since 22 October: the app is free, and Sync is the only paid part at $3 a month. |
| Microsoft Store | Desktop **1.0.11 is published and fully available** in the [Microsoft Store](https://apps.microsoft.com/detail/9PMQMBZJG06R). The [read-only status check](https://github.com/urbudget/urbudget/actions/runs/40926758676) at 18:01 UTC (21:01 Riyadh) on 29 October confirms submission `1152921505702270583` is `Published`, is the last-published submission, and contains uploaded package `1.0.11.0` with full rollout. Release monitoring is complete and its automation has been removed. |
| Android | Native app source version `1.0.0` (version code 1000000), package `app.urbudget`. The phone apps start their own numbering at 1.0.0 for their first store release; 1.0.7 was desktop's version when the apps were split. A [signed bundle built on GitHub](https://github.com/urbudget/urbudget/actions/runs/39681884973) from the `google-play` environment's upload key on 22 October 2026 (then still numbered 1.0.7). Not on Play yet: the developer account's identity verification is in progress, and the app can't be created in Play Console until it's done. |
| iOS | Native app source version `1.0.0`, bundle `app.urbudget`. The App Store Connect app record (6825535486) exists, and its listing, price (free), availability, age rating (4+) and privacy answers are filled in. Version 1.0.0 (build 1, from `ios-v1.0.0`) was submitted to App Review on 22 October 2026 as "UrBudget: Envelope Planner", set to **manual release**. Its privacy answers are published, and its screenshots (iPhone and iPad) and review account (`review@urbudget.app`, with complimentary Sync) are set up. It's not on the Mac App Store, by choice: Mac users get the desktop app. |

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
