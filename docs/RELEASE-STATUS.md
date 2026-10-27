# Release status

Reviewed 27 October 2026. Update this page after a release or a verified account milestone.

| Platform | Status |
| --- | --- |
| Desktop | Published [desktop-v1.0.10](https://github.com/urbudget/urbudget/releases/tag/desktop-v1.0.10) on 27 October 2026: Arabic, updates that install themselves, and typing transactions by keyboard with fewer keys. Windows and universal macOS installers and signed update packages are live on urbudget.app, and 1.0.9 copies are offered it. |
| Website | urbudget.app runs this repository's `web/` since commit `e36d8f7` (22 October 2026): the app is free, and Sync is the only paid part at $3 a month. The Polar product "UrBudget Sync" is $3 a month, and the old $9 license product is archived. |
| Microsoft Store | 1.0.10 sent on 27 October 2026 by the [Microsoft Store run](https://github.com/urbudget/urbudget/actions/runs/40543438969) that publishing started, without a draft (`STORE_DRAFT` unset), so it goes to certification with the listing as it is. The run succeeded and committed submission 1152921505702247644, which Partner Center then had in pre-processing; certification isn't verified yet. 1.0.9 was sent the same way on 24 October 2026 by a [run](https://github.com/urbudget/urbudget/actions/runs/40107546026) that succeeded. 1.0.8 was submitted on 22 October 2026 with the listing rewritten for the free app. |
| Android | Native app source version `1.0.0` (version code 1000000), package `app.urbudget`. The phone apps start their own numbering at 1.0.0 for their first store release; 1.0.7 was desktop's version when the apps were split. A [signed bundle built on GitHub](https://github.com/urbudget/urbudget/actions/runs/39681884973) from the `google-play` environment's upload key on 22 October 2026 (then still numbered 1.0.7). Not on Play yet: the developer account's identity verification is in progress, and the app can't be created in Play Console until it's done. |
| iOS | Native app source version `1.0.0`, bundle `app.urbudget`. The App Store Connect app record (6825535486) exists, and its listing, price (free), availability, age rating (4+) and privacy answers are filled in. Version 1.0.0 (build 1, from `ios-v1.0.0`) was submitted to App Review on 22 October 2026 as "UrBudget: Envelope Planner", set to **manual release**. Its privacy answers are published, and its screenshots (iPhone and iPad) and review account (`review@urbudget.app`, with complimentary Sync) are set up. It's not on the Mac App Store, by choice: Mac users get the desktop app. |

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
