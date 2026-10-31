# Releasing

One repository holds three apps, and each is released on its own: its own version, its own tag
and its own release workflow. The website (`web/`) has no tag: a push to `main` that changes it
deploys it ([deploy.yml](.github/workflows/deploy.yml), see the [website guide](web/README.md)).

| App | Version is in | Tag | Workflow | Where it goes |
| --- | --- | --- | --- | --- |
| Desktop (Windows, macOS) | `desktop/` (package, lockfile, Tauri and Cargo files) and the shared crates | `desktop-v1.2.3` | [release.yml](.github/workflows/release.yml) makes a draft GitHub release with the installers | Publishing the draft puts them on urbudget.app ([publish.yml](.github/workflows/publish.yml)) and sends the Store package to the Microsoft Store ([store.yml](.github/workflows/store.yml)) |
| Android | `android/app/build.gradle.kts` | `android-v1.2.3` | [android-release.yml](.github/workflows/android-release.yml) builds the signed App Bundle and sends it to Play | Play's internal track (or the `PLAY_TRACK` variable's), then promote it in Play Console. The very first bundle is uploaded by hand from the run's artifacts |
| iOS | `ios/project.yml` and the Xcode project | `ios-v1.2.3` | [ios-release.yml](.github/workflows/ios-release.yml) uploads the build to App Store Connect | TestFlight, then submit it for review in App Store Connect |

## Making a release

```sh
node scripts/versions.mjs                 # each app's version
node scripts/versions.mjs android 1.0.8   # set the version of the app you're releasing
git commit -am "Android 1.0.8"
git tag android-v1.0.8
git push origin main android-v1.0.8
```

The script writes the version into every file that app keeps it in and refuses a lower version.
Each release workflow first checks that its tag names the version in the source, and the
Android and iOS ones that the commit is on `main`. A release workflow can also run from the
Actions tab on `main` with the version already in the source.

- **Bump only the app you're releasing.** Releasing Android doesn't change desktop's version, and
  the other way around. Versions differ between apps from here on; the data format they share
  doesn't depend on them.
- **The shared crates follow desktop.** `crates/` is built into every app but released with
  none of them on its own; its version moves with desktop's (the script does both), and it's the
  version the engine reports to urbudget.app.
- **Desktop:** write the release's notes from [CHANGELOG.md](desktop/CHANGELOG.md) into the draft
  before publishing; they show on the download page and in the app's update notice. Desktop
  releases up to 1.0.7 are tagged `v1.0.7` and so on; those tags stay as they are.
  The Release workflow gates installer builds on updater regression tests. For updater changes,
  complete the [two-version Store flight and standalone upgrade checks](desktop/README.md#verifying-an-updater-change)
  before publishing the draft: publication submits the Store package for certification as well as
  offering standalone updates. Record the tested package versions, artifact hashes, and results
  with the release evidence. [desktop/README.md › Releases](desktop/README.md#releases) covers signing.
- **Android:** the version code comes from the version, in the scheme the Tauri Android app used
  (1.2.3 is 1002003). To upload the same version again, run **Android release** from the Actions
  tab with a higher version code. Upload `mapping.txt` with the bundle so crash reports are readable.
- **iOS:** the build number is the workflow's run number, so each upload has a new one.

The native apps in `android/` and `ios/` replaced the Tauri phone builds `desktop/` used to have,
with the same app IDs (`app.urbudget`). Neither Tauri phone app was published, and their
configuration was removed from `desktop/` on 21 October 2026.

## One-time setup for the phone apps

Android's release workflow has built a signed bundle; iOS's first ran for `ios-v1.0.0`. Watch the first upload to each store.

**Android** needs the upload key, in the repository's `google-play` environment (Settings →
Environments; GitHub makes it on the first run). It's the key made for Play App Signing
([mobile guide › Google Play](docs/MOBILE.md#google-play)), kept in `desktop/.signing` on
the computer that made it; the native app signs with it as the Tauri one did:

```sh
base64 -i upload.jks | gh secret set ANDROID_UPLOAD_KEYSTORE_BASE64 --env google-play
gh secret set ANDROID_UPLOAD_KEY_ALIAS --env google-play
gh secret set ANDROID_UPLOAD_STORE_PASSWORD --env google-play
gh secret set ANDROID_UPLOAD_KEY_PASSWORD --env google-play
```

To have the workflow upload to Play itself:

1. Upload the first bundle by hand (Play Console → Test and release → Internal testing → Create
   new release, with the bundle from an **Android release** run's artifacts) and roll it out to
   internal testers. Play's API can't upload to an app that has never had a release.
2. In Google Cloud, make a service account (no roles needed in Cloud) and a JSON key for it, and
   enable the Google Play Android Developer API in the same project. In Play Console → Users and
   permissions, invite the service account's email with **Release apps to testing tracks** for
   UrBudget (add **Release to production** only if tags should go to production).
3. Store the key: `gh secret set GOOGLE_PLAY_SERVICE_ACCOUNT_JSON --env google-play < key.json`,
   then delete the downloaded file.

Each `android-v*` tag then goes to the internal track. `PLAY_TRACK` (a repository variable) picks
another track, such as `alpha` for closed testing; `PLAY_RELEASE_STATUS=draft` leaves the release
as a draft to roll out from Play Console. Keep this key apart from the Worker's
`GOOGLE_PLAY_SERVICE_ACCOUNT`, which only checks purchases.

**iOS** needs the app in App Store Connect and an API key for uploading:

1. If the bundle ID `app.urbudget` isn't in [Identifiers](https://developer.apple.com/account/resources/identifiers/list)
   yet, add it (App IDs → App). Then make the app in App Store Connect (Apps → + → New App)
   with that bundle ID.
2. In App Store Connect → Users and Access → Integrations → App Store Connect API, make a team
   key with **Admin** access, so Xcode can make the distribution certificate and profile. Download
   its `.p8` file (it can be downloaded once) and note the key ID and the issuer ID.
3. Store them in the `app-store` environment:

```sh
gh secret set APP_STORE_CONNECT_KEY_ID --env app-store
gh secret set APP_STORE_CONNECT_ISSUER_ID --env app-store
gh secret set APP_STORE_CONNECT_KEY --env app-store < AuthKey_XXXXXXXXXX.p8
```

The first iOS version that sells Sync in the app goes to App Review with the subscription: set
it up as [web/README.md › App Store purchases](web/README.md#app-store-purchases) describes, then
add it on the version's page in App Store Connect before submitting.

## Verifying a desktop release

The desktop release workflow also installs the draft Windows installer on a fresh hosted runner.
Its bounded smoke test checks the bundled first-run screen, a local save, and persistence after
restart. The screenshots, hashes, and result are retained as the run's Windows smoke artifact.
Do not run that installer test in a personal Windows profile: release builds use the OS data directory.

Publishing the draft starts installer publication and Microsoft Store submission. A successful
Store submission run only confirms that Microsoft accepted the submission for processing. To
check certification, run **Microsoft Store status** with the exact submission ID from that run
and the four-part MSIX version (for example, `1.0.11.0`). It makes read-only requests and records
a sanitized status artifact. Availability is confirmed only when that exact submission is
Published, is the application's last published submission, contains the expected uploaded
package, and is fully rolled out. Pending certification is not release completion.

Current published versions are in [release status](docs/RELEASE-STATUS.md).
