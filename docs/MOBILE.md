# UrBudget on iPhone and Android

The phone apps are native apps: SwiftUI in [`ios/`](../ios/README.md) and Kotlin with Jetpack
Compose in [`android/`](../android/README.md). Neither one embeds the desktop's React window or
Tauri. They draw their own screens with each system's controls and run the same Rust engine as
the desktop app, so budgets, sync and imports behave the same everywhere.

Before the native apps, the desktop's Tauri project was also built for phones, with a phone
layout of the React window. That was removed from `desktop/` on 21 October 2026; its notes are in
the [archive](archive/tauri-phone-apps-2026-10.md).

## How a phone app is built

```text
 ios/UrBudget  (SwiftUI screens)             android/app/src/main/java/...  (Compose screens)
        |  JSON request, JSON reply                     |  JSON request, JSON reply
        v                                               v
 crates/urbudget-ios                             crates/urbudget-android
 C functions, built as a static library          JNI functions, built as a .so library
 (liburbudget_ios.a)                             (liburbudget_android.so)
        |                                               |
        +-----------------------+-----------------------+
                                v
        crates/urbudget-services   account, sync, vault, banks, YNAB, assistant
        crates/urbudget-core       accounting, SQLite storage, imports, reports, merges
                                ^
                                |
        desktop/src-tauri  (the desktop app calls the same crates)
```

The screens never do the accounting themselves. A screen sends a request to the Rust library as
JSON text and shows the reply. An engine request carries the budget it's about, such as
`{"version": 1, "method": "calculate", "budget": {...}, "params": {...}}`, and the engine forgets
it after the call. A service request works on the budget the shared store has open.

The two apps now use these differently. iOS changes its own whole copy through engine requests
and saves it with `save_budget`. Android's edits, previews and totals send no budget: they go to
the service methods `edit_budget` and `query_budget`, run on the store's copy, and the app keeps
only a light copy ([the contract](plans/engine-edits.md#as-built)). An engine request gets the same
answer on both, because the iOS adapter passes engine requests straight to the Android one's
`invoke_json`. On iOS the requests go through `Core/NativeCore.swift`, on Android through
`NativeBridge.kt`.

**iOS.** `ios/UrBudget.xcodeproj` is an ordinary Xcode project (generated from
`ios/project.yml` by XcodeGen). Before Xcode compiles any Swift, its **Build Rust engine** step
runs `ios/scripts/build-rust.sh`. That script runs `cargo build` on `crates/urbudget-ios` for the
iPhone or the Simulator and puts `liburbudget_ios.a` where the linker finds it. Swift calls its
four C functions (`urbudget_invoke`, `urbudget_initialize`, `urbudget_service`, `urbudget_free`),
which are declared in `ios/UrBudget/Bridge/UrBudgetCore.h`.

**Android.** `android/` is an ordinary Gradle project. Before Gradle compiles any Kotlin, the
`buildRustEngine` task in `android/app/build.gradle.kts` runs `cargo build` on
`crates/urbudget-android` once for each phone architecture (arm64-v8a, x86_64), using the
Android NDK's compiler. It puts `liburbudget_android.so` in the app, and `NativeBridge.kt` calls
it through JNI. `shared/android` holds the app's Keystore keeper (`SecretStore.kt`) and Play billing client,
kept apart from the rest because the Tauri Android project compiled them too.

So you open the folder in Xcode or Android Studio and press Run. The Rust part builds itself
first, and the first build takes a few minutes. Each README covers its own commands:
[iOS](../ios/README.md#build-and-run), [Android](../android/README.md#build-and-tests).

## Where things are

| Folder | What's in it |
| --- | --- |
| `ios/UrBudget/App` | The app's entry point, tabs, quick actions and Shortcuts |
| `ios/UrBudget/Core` | `BudgetStore` (state for the screens), the engine bridge (`NativeCore.swift`), money, dates and JSON helpers, sync, App Store purchases |
| `ios/UrBudget/Screens` | One folder per area: Budget, Transactions, Accounts, Reports, Planning, Imports, Assistant, Sync, Library, More |
| `ios/UrBudget/UI` | Shared controls: keypad, pickers, theme, components |
| `ios/UrBudget/Bridge` | The C header for the Rust library |
| `ios/UrBudgetTests` | Unit tests, run on the Simulator |
| `android/app/src/main/java/app/urbudget/nativeapp` | Engine bridge (`NativeBridge.kt`), repository and view model, money and report helpers |
| `android/app/src/main/java/app/urbudget/nativeapp/ui` | Every screen and shared control (`App.kt` is the root) |
| `android/app/src/test`, `src/androidTest` | Unit tests on the computer, and tests on an emulator |
| `crates/urbudget-ios`, `crates/urbudget-android` | The two adapters: request handling for each system, and their tests |
| `shared/android` | Kotlin shared by Android builds: Keystore and Play billing |
| `shared/fixtures` | Sample budgets and bank files the tests use |

Android's code is six folders down (`app/src/main/java/app/urbudget/nativeapp`) because Kotlin
files sit in folders named after their package, `app.urbudget.nativeapp`. In a file browser the
folders above it look empty; Android Studio folds them into one.

## Data and secrets

Budgets are stored by the shared SQLite store and multi-budget registry in the app's private
folder (Application Support on iOS, the app's files folder on Android). Sync is how they reach a
computer. Release builds use the app ID `app.urbudget`; debug builds install beside them as
`app.urbudget.native` (iOS) and `app.urbudget.nativeapp` (Android), so testing never touches real
budgets.

Secrets (the assistant's API keys and ChatGPT sign-in, YNAB tokens, the account session, the sync
key) are in the same sealed `secrets.bin` as on a computer
([desktop README › Secrets](../desktop/README.md#secrets)), and its key goes where each system
keeps keys:

- **iOS:** the Keychain, as on macOS.
- **Android:** [`SecretStore.kt`](../shared/android/src/main/java/app/urbudget/SecretStore.kt)
  seals the key with AES-GCM under a key that never leaves the Android Keystore, in the app's
  `no_backup` folder, which backups and device transfers skip. Rust calls it over JNI
  ([`keystore.rs`](../crates/urbudget-services/src/keystore.rs)), so it works from any thread.
  A `secrets.bin` restored without its key can't be opened: the app keeps the ciphertext and
  blocks credential writes rather than replacing the vault, and retries when key access returns.

## The assistant's ChatGPT sign-in

Phones sign in the way a computer does
([`assistant/auth.rs`](../crates/urbudget-services/src/assistant/auth.rs)): OAuth with PKCE, and a
one-time listener on `127.0.0.1` that takes the browser's callback. OpenAI accepts no other
redirect: in October 2026, `urbudget://auth/callback` and `https://urbudget.app/auth/callback`
were both refused ("Invalid authorize request", `param: redirect_uri`). So a deep link, an
Android App Link or an iOS Universal Link can't carry the callback.

What differs is how the page opens. The iOS app shows it in Safari's in-app view
(`SFSafariViewController`, `UI/SafariSheet.swift`). iOS keeps the app active while that view is
open, so the listener can answer; opening Safari itself wouldn't work, because iOS suspends an app
soon after it leaves the screen. The Android app opens the page in the browser. Android stops an
app's network and then freezes it soon after it leaves the screen, which doesn't lose the callback:
the system queues the browser's connection, and the sign-in finishes when the user comes back to
the app. Those Android observations were made with the Tauri build, which used the same Rust
sign-in, on an API 37 emulator. Neither native app's ChatGPT sign-in has been tried on a real
phone yet.

## Checks on GitHub

- **Native Android** ([android-native.yml](../.github/workflows/android-native.yml)) runs on Linux
  when `android/`, the shared crates (except the iOS adapter), `shared/android` or
  `shared/fixtures` change: the whole Rust workspace's tests, then Kotlin tests, lint, and debug
  and release builds for both architectures, and a separate job runs `SyncV2RuntimeTest` on an
  emulator. **Run workflow** also builds preview and keeps the APKs to install. The other
  instrumented checks aren't in CI: [how to run them](../android/README.md#instrumented-checks).
- **Native iOS** ([ios-native.yml](../.github/workflows/ios-native.yml)) runs when `ios/`,
  `crates/urbudget-ios`, `crates/urbudget-core` or `crates/urbudget-services` change: the adapter's
  Rust tests on Linux, then the app's tests on the iOS Simulator. The iOS release workflow runs
  the Simulator tests again before every upload.
- Releases have their own workflows and tags: [RELEASING.md](../RELEASING.md).

CI uses Rust stable, Java 17, Android command-line tools 23.0 (build 15859902), SDK package
`platforms;android-37.0`, Build-Tools 36.0.0 and NDK 30.0.16248370 (API 37.0 needs command-line
tools 22 or newer), and GitHub's current macOS image with its Xcode.

## Store releases

How to tag and build a release, and the secrets each workflow needs, are in
[RELEASING.md](../RELEASING.md). What each store needs first:

### Apple App Store

- The App ID `app.urbudget` in the Apple Developer account (the team that signs the desktop app).
- An app record in App Store Connect, with privacy answers matching the optional account,
  purchases, sync, bank and assistant features. Encrypted sync is not a blanket "no data
  collected" claim.
- An App Store Connect API key for builds from GitHub. Xcode's automatic signing then makes the
  distribution certificate and profile.
- Screenshots for a 6.9" iPhone, and for a 13" iPad since the app runs on iPad.
- In-app purchases: the Sync subscription `app.urbudget.sync.monthly` and the App Store Server
  Notifications URLs, set up in App Store Connect as
  [web/README.md › App Store purchases](../web/README.md#app-store-purchases) lists. The first
  version that sells it goes to App Review together with the subscription.

### Google Play

- A Play Console developer account (a one-time fee, plus identity verification). A new personal
  account has to run a closed test with at least 12 testers for 14 days before the app can go to
  production. An organization account doesn't.
- An upload key, kept out of the repository (`*.jks` is ignored):
  `keytool -genkey -v -keystore upload-keystore.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload`.
  Play App Signing then holds the key that signs what users install. Back the upload key up the
  way you back up the desktop updater key.
- The Data safety form, and a target API level Play still accepts. Play raises it each August;
  check `targetSdk` in `android/app/build.gradle.kts`.

### Payments

The apps are free; the only purchase is the optional sync subscription, $3 a month. Desktop and
the website sell it through Polar. Android uses Google Play Billing 9.1.0 for the same monthly
subscription and never falls back to external checkout: prices
come from Play, purchases require an UrBudget account, and the server validates the purchase token
and its obfuscated account identity before granting access. Pending payments grant no access;
verified grants are saved before acknowledgement. Restore, foreground retry and authenticated Play
notifications recover interrupted or delayed payments. Expiry and refunds are checked on the
server.

iOS sells the same monthly subscription through the App Store with StoreKit 2, and never links to
the website to buy: prices come from the App Store, purchases carry the account's
`appAccountToken`, and the server checks each transaction Apple signed, and App Store Server
Notifications for renewals, billing retry, expiry and refunds, before granting or ending access.
Only the subscriber can cancel an App Store subscription, in their Apple Account settings, so
deleting an account says so instead of cancelling it.

### Android launch configuration

[Release status](RELEASE-STATUS.md) and the owner's private Play checklist say which of these are
done.

1. Follow the verified account status and remaining owner actions in the Play checklist. The
   owner already created a Personal account; do not restart registration. Complete the outstanding verification, then reserve `app.urbudget`.
   Answer Console declarations accurately and resolve any account-type request Google makes.
2. Create the subscription **urbudget_sync**, with the auto-renewing base plan **monthly**,
   one-month billing and no introductory offer. Set its price and countries in Console. The
   website's price is $3 a month; Android shows Play's localized price. Configure license testers
   (Play's name for test buyers) and test the subscription through a Play testing track.
3. Authorize a Google service account for the Android Publisher API and this app's purchases. Put
   its JSON in the Worker secret **GOOGLE_PLAY_SERVICE_ACCOUNT**, never in the app, the repository
   or the frontend. Configure authenticated Pub/Sub notifications with **GOOGLE_PLAY_PUSH_EMAIL**
   and **GOOGLE_PLAY_PUSH_AUDIENCE**; see `../web/README.md`.
4. Deploy the web migrations and API/privacy updates before the app: the reports inbox, the
   checkout-free `/privacy/app/`, purchase verification, refund/expiry refresh and Play
   notifications.
5. Make and back up an upload key, enable Play App Signing, and store the key in the `google-play`
   environment's four `ANDROID_UPLOAD_*` secrets ([RELEASING.md](../RELEASING.md)). An
   `android-v1.2.3` tag then builds the signed bundle; it doesn't upload or publish anything. Never
   use the debug key for submission.
6. Configure a dedicated review account with synthetic data. Optional **PLAY_REVIEW_EMAIL** and
   **PLAY_REVIEW_CODE_HASH** enable reusable app-only review access; normal accounts keep using
   email codes. Put the access code and instructions only in Play Console's App access section, and
   give that account the sync it needs. Never enable `DEV_LOGIN_CODES` in production.
   Setup and rotation are in `../web/README.md`.
7. Complete the listing, Data safety, financial features, audience and content rating, ads
   declaration, privacy and deletion URLs, and distribution details, from the owner's draft
   listing copy.
8. Try a real signed Play build: buy and restore, a pending payment completing with the app open
   and closed, refund, renewal, grace period, hold and expiry, account deletion, and another
   account's receipt. Read Play's pre-launch report. Test physical devices (ARM, low memory, 16 KB
   pages), accessibility, large imports, bank and sign-in flows, and desktop-to-phone sync with
   conflicts. Emulators and mocked providers can't establish these.
