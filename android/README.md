# Native Android

Native Kotlin / Jetpack Compose client. Open this
folder in Android Studio. The app is free and everything works without an
account; the only purchase is the optional sync subscription, $3 a month,
through Google Play. The application uses native screens, Android document
pickers and the print framework; it does not embed the React UI or Tauri runtime.

Debug installs as `app.urbudget.nativeapp` alongside the published app. Release
uses `app.urbudget`, so a correctly signed upgrade retains the existing app's
private library and Android Keystore. Do not install debug test builds over real
data or test against a connected personal phone.

## How it's built

This folder is an ordinary Gradle project. Before Gradle compiles any Kotlin, the
`buildRustEngine` task in [`app/build.gradle.kts`](app/build.gradle.kts) runs `cargo build`
on [`crates/urbudget-android`](../crates/urbudget-android) once for each phone architecture
(arm64-v8a, x86_64) with the Android NDK's compiler, and packs the resulting
`liburbudget_android.so` into the app. Kotlin calls it through JNI in
`NativeBridge.kt`: each call sends a JSON request and gets a JSON reply, the same requests
the iOS app sends. The accounting, storage, sync and imports all happen in that Rust library
(the shared crates); Kotlin draws the screens. The [mobile guide](../docs/MOBILE.md) shows
how this fits with iOS and desktop.

## Folders

Kotlin files live in folders named after their package, so the code is six levels down
(`app/src/main/java/app/urbudget/nativeapp`). The folders above it hold nothing else;
Android Studio shows them as one.

| Folder | What's in it |
| --- | --- |
| `app/build.gradle.kts` | The app's version, dependencies, signing and the Rust build task |
| `app/src/main/java/app/urbudget/nativeapp` | `MainActivity.kt`, the engine bridge (`NativeBridge.kt`), `BudgetRepository` and `BudgetViewModel`, money, register and report helpers |
| `app/src/main/java/app/urbudget/nativeapp/ui` | Every screen and shared control; `App.kt` is the root, `Theme.kt` and `Design.kt` the look |
| `app/src/main/res`, `AndroidManifest.xml` | Icon, styles, file sharing and backup rules |
| `app/src/test` | Unit tests that run on the computer |
| `app/src/androidTest` | Tests that run on an emulator |
| `app/src/preview` | The optimized, debug-signed `preview` build's manifest |
| [`../shared/android`](../shared/android) | Android Keystore keeper and Play billing client, with their tests |
| [`../crates/urbudget-android`](../crates/urbudget-android) | The Rust side of the JNI bridge |

## Shared code

```text
Desktop React -> thin Tauri commands --+-> urbudget-core
                                      |   schema / integer math / workflows / reports
Android Compose -> JNI adapter -------+   SQLite / patches / merge / import conversion
                                      |
                                      +-> urbudget-services
                                          accounts / Play subscription / encrypted sync
                                          SimpleFIN / YNAB / assistant / vault

Native Android host ---> shared/android: PlayBillingClient + SecretStore
```

These are physical shared sources, not forked copies. Desktop keeps its public
Rust module paths through re-exports and thin command adapters. Tests run against
both hosts, shared synthetic fixtures and the Android JNI boundary.

- `crates/urbudget-core`: schema, validation, ledger, envelopes, targets, Quick
  assign, schedules, loans, reports/forecast, reconciliation, bank/YNAB import,
  payee/category workflows, SQLite, multi-budget registry, backups and merges.
- `crates/urbudget-services`: actual desktop account authentication, Play
  subscription verification, encrypted sync and conflicts, bank networking,
  assistant providers, PKCE sign-in, attachments, tools, proposals, workbooks,
  history and sealed vault.
- `crates/urbudget-android`: bounded versioned JSON/JNI transport. Panics do not
  unwind through JNI. The persistent Tokio runtime keeps assistant/OAuth jobs
  alive between calls.
- `shared/android`: one Google Play client and AES-GCM/Android Keystore keeper, with
  their unit tests. The Tauri Android project that came before this app used them too.
- `shared/fixtures`: portable budgets and bank files used for cross-platform
  accounting and lossless metadata tests.

Kotlin owns presentation, navigation, exact amount text conversion, lifecycle,
SAF, intents and printing. It does not calculate envelope funding, interest,
import matches or sync merges.

## Implemented native workflows

| Area | Native coverage |
| --- | --- |
| Budget | Month navigation, assignments and money moves, Quick assign previews, targets and snoozing, hidden/filter views, category/group ordering, merge/delete history handling, notes and expected income |
| Register | Ordinary and split transactions, both sides of transfers, named flags/approval/status, search/date/amount/account/category filters, bulk edits, duplication, deletion and CSV export |
| Accounts | Cash, credit and tracking types, opening balances, archiving, reconciliation preview/adjustments, card payment reserves |
| Planning | Full split/transfer schedule templates and all repeat intervals, enter/skip/pause, automatic due schedules, loan terms/rate history/payment/amortization/extra-payment comparison |
| Reports | Twelve report modes, ranges and account/category filters, category/payee/day transaction drilldowns, shared forecasts, CSV export |
| Library | Multiple budgets, fresh start, undo/redo, JSON import/export, manual and automatic backups, retention and restore, formatting and appearance |
| Account | Email-code sign-in, sign-out, account overview/deletion; only sync needs an account |
| Sync | Existing end-to-end encryption and protocol, passphrase setup/unlock/reset, track/download/remove, per-field and whole-budget conflict review; automatic sync on foreground, after edits and every minute while open |
| Billing | The monthly sync subscription (`urbudget_sync`, base plan `monthly`) at Play's localized price, pending/cancelled states, account-bound restore, server verification, subscription management |
| Banks | SimpleFIN setup claim, secure tokens, account links, import range/sign handling, request quotas, download/match/duplicate review, import and disconnect; bank files (CSV, Excel, OFX, QFX, QIF, CAMT.053, MT940; the format their content says; a PDF goes to the assistant with the file attached) read as an import session: the lines that need attention (fix a line's date, payee, memo or amount), those left out (include them) and those changed (undo), the account and sheet in the file, signs and the date-order question, Fix with the assistant (its fixes apply to the import), then the review with transfer pairing and refund labels; the account remembers how its file read (its CSV columns are chosen on the desktop) |
| YNAB | Multiple saved tokens, plans, delta sync with full-download fallback, conflict/discrepancy review, ZIP and Register/Plan CSV imports |
| Languages | English and Arabic from string resources, chosen in Settings & data or Android's own app-language settings (System by default); Arabic lays the app out right to left, keeping amounts and charts left to right ([languages guide](../docs/LOCALIZATION.md)) |
| Assistant | ChatGPT browser sign-in, OpenAI/Anthropic keys, model/settings, streamed conversations, cancel/history/export, files/photos, context selection, shared tools, review/recheck/apply/undo/refine/discard proposals, bank-file conversion review, workbook generation, report replies/printing/sharing |

Desktop window controls, updater delivery and OS-specific shell integration stay
with their platform hosts. Android uses Google Play delivery, native pickers,
intents and Print / Save as PDF.

## Bank SMS import

**More → Bank messages** (also **Settings & data → Imports → Bank messages**) is an
opt-in, on-device SMS inbox. After the disclosure and Android permission prompts,
choose one or several sender addresses from the phone's SMS inbox (up to 50). This
includes alphanumeric bank sender IDs and phone numbers; it does not need contacts
access. Choose new messages only, or the past 30/90 days plus new messages. The
history choice applies to newly added senders. Existing choices keep their start
date. RCS and MMS are not read.

The system SMS receiver captures selected senders while the app is closed. Opening
Bank messages or using Refresh catches up from the system inbox after missed
broadcasts/restarts. Messages remain local in a separate SQLite inbox, scoped to
the current budget's database path. No capture changes a budget or calls the
assistant/network. Capture does not interpret message text; users exclude verification
codes and personal messages from the raw preview before sending. Removing a sender
removes its pending messages; **Stop and clear messages** removes that budget's
local inbox and disables its collection. Neither action changes Android's messages
or imported transactions. Deleted budget paths stop collecting when next encountered.

Select up to 50 messages (120 KB maximum) and choose **Analyze selected**. A separate
notice identifies the configured assistant provider/model and asks permission to
send those full messages, account/category names, relevant payee category history
and explicitly remembered sender/card mappings. No message is preselected. Provider
retention settings apply. The dedicated analysis session has only `payee_history`
and `submit_sms_analysis` tools, no writes, browsing, attachments or ordinary chat
history. Requests time out after five minutes, can be cancelled, and expire from
memory after ten minutes. Budget/provider changes prevent stale requests from being
accepted. Failed or interrupted analysis leaves messages available for retry.

The model accounts for every source exactly once as a transaction, grouped question,
pending event or excluded message. It quotes source evidence and prepares multiple
accounts in one batch. Missing account mappings or billed amounts in the budget
currency require answers; exchange rates are not guessed. Users can answer grouped
questions, explicitly remember a sender plus literal card reference, give a batch
instruction, or edit proposed fields. Updating analysis asks for sending consent
again. Reviews survive screen/process recreation in the private inbox; discard or
finish removes the draft. No regex bank parser is used. Deterministic core checks
validate exact money, dates, references, evidence and complete source coverage.

Review selected uses the existing bank import engine to add, match or skip rows.
The `smsBatchApply` command commits all accounts once through the normal budget revision guard and
undo/save path. SMS is not settlement evidence: new entries are uncleared, and
matching preserves existing clearing/reconciliation states, including transfer
sides. A stable sender/message-timestamp/body fingerprint is carried as the import
ID, including after manual corrections. All grouped source fingerprints are also
recorded in the budget to prevent reimport after reassignment to another account.
Raw bodies are not stored in budget metadata or normal assistant conversations.
Inbox text is erased after an acknowledged review or dismissal, with a local
fingerprint retained to prevent re-queuing. Questions and pending events remain.
**Stop and clear** also deletes saved drafts and remembered mappings. If
capture or acknowledgement is interrupted, repeat import review detects existing
import IDs. The screen pages pending messages in batches of 500.

The manifest requests only `READ_SMS` and `RECEIVE_SMS`, not default-SMS status or
permission to send/change messages. **Before distributing this build through
Google Play**, complete its SMS permissions declaration under the
[SMS-based money management exception](https://support.google.com/googleplay/android-developer/answer/10208820?hl=en)
and obtain approval; align the store description, privacy policy and Data safety
answers with this feature. Eligibility is subject to Google's review. SMS remains
optional, and denied/revoked access leaves the other budgeting features available.

Implementation: `SmsInbox.kt` owns capture, selection and private storage;
`SmsAnalysis.kt` owns identities, draft recovery and the cancellable service client;
`ui/SmsImportScreen.kt` and `ui/SmsAnalysisReview.kt` own selection/consent/review.
Services `assistant/sms.rs` owns the ephemeral model session; core `sms_import.rs`
validates output and preserves clearing state while reusing bank matching. Synthetic
coverage includes `SmsAnalysisTest`, `SmsInboxTest`, `SmsImportUiTest`, core
`tests/sms_analysis.rs`, `tests/sms_import.rs`, and service model-loop tests.

## Layout

The screens follow the phone layout first made for the Tauri phone builds
([archived guide](../docs/archive/tauri-phone-apps-2026-10.md)), which takes its
structure from YNAB's phone app and keeps UrBudget's colours and words (Unassigned,
not Ready to Assign).

- `ui/Theme.kt` holds the desktop's tokens as a phone shows them. Light is grey with
  white cards. Dark is navy, because the computer's charcoal reads green on a phone.
  Money uses tabular figures. `ui/Design.kt` has the shared pieces: the top bar of
  round buttons, list cards and rows, the solid Available pill, the target bar,
  segmented controls, chips, the cleared mark and sheets.
- Budget, Transactions, Accounts and Reports are tabs on a floating tab bar. More opens
  a drawer with the budget and the other pages. "+ Transaction" floats above the bar
  and shrinks to its plus while a list scrolls down. The assistant rises as a sheet.
- Budget: Unassigned as a solid banner, which opens Quick assign or Move money. Each
  group is a band with its Assigned and Available totals. Each category shows
  Assigned and its pill, coloured by `CategoryStatuses` (the desktop's
  `categoryState`). Tapping a category opens its details as a sheet; tapping its
  Assigned amount opens that sheet on the keypad; tapping its pill moves money.
- Adding or editing a transaction takes the whole screen. The amount comes first, on
  the keypad (`AmountPad`: digits come in as cents and it does sums), then payee,
  category, account and date, with the payee and category pickers opening in turn.
  It is a layer of the main window rather than a dialog window: a floating dialog
  window can't draw under the system bars.
- Every other form is a bottom sheet with Cancel and its action at the top. Fields and
  choices take a fill that stands out from what they sit on (`LocalFieldFill`).
- Transactions: the days as bands, categories as chips, a cleared mark to tap,
  scheduled transactions on top and a banner for transactions to approve. A long
  press selects, and the selection's actions take the tab bar's place.
- Back closes the newest thing first: a picker or sheet, the selection, folded-out
  search, the assistant. Then an account's transactions go back to Accounts, a More
  page to the tab it came from, and any other tab to Budget.

## Storage, privacy and concurrency

Both clients use the same SQLite `Store` and multi-budget registry. Native edits
are serialized off the UI thread, validated, and published only after durable
revision-checked commits. Stale writes cannot replace a newer saved revision.
Queued edits verify the affected records and budget identity under the repository
lock; switching library entries disposes editors even if both budgets have the
same contents. Report results are shown only for the current range and filters.
Assistant changes require explicit review; apply and proposal-specific undo are
atomic and tied to the reviewed budget and revision. Ordinary local saves use the
shared delta path; imports preserve the full portable schema and unknown fields,
including assignment/schedule extensions.

An earlier native `filesDir/budget.json` migrates to SQLite only after validation
and a successful commit. Its original bytes remain for recovery. A signed upgrade
from Tauri finds the existing library directly in `filesDir`; otherwise native
state lives in `filesDir/native`. Credentials stay in the same sealed vault with
its key protected by Android Keystore and excluded from backup. Only a confirmed
missing secret is treated as absent; decryption/I/O failures do not reset it.

ChatGPT sign-in keeps its PKCE callback on `127.0.0.1`. The callback page offers
**Return to UrBudget**, which targets the initiating package and resumes its
existing activity. This link carries no OAuth code or credentials and only
refreshes status; receiving it cannot sign someone in. The token exchange waits
until the activity is started again, so it runs in the foreground rather than
behind the browser. Settings refreshes on
resume and polls pending attempts, shows the current stage, and retains the last
sign-in error until the next attempt even if Settings is reopened. The
`UrBudgetSignIn` diagnostic log contains stage names only. The token
exchange has a 30-second deadline and remains cancellable after the callback.
The page acknowledges receipt instead of claiming success before credentials
have been validated and saved.

`AssistantSignInTest` covers completion, resumed status and failures;
`NativeSignInTest` exercises the real JNI callback with synthetic consent denial;
`SignInReturnActivityTest` checks activity reuse. Run the two instrumentation
classes in separate processes because the JNI test uses an isolated data folder.
The Rust auth tests also exchange a synthetic authorization code with a local
token endpoint and verify PKCE and ID-token claims. Live account consent still
needs a manual device test.
The optional `NativeSignInTest#tokenTransportWithSyntheticGrant` instrumentation
test requires `-e checkAuthNetwork true`; it uses a fabricated grant to verify
foreground handoff and connectivity to the real token endpoint, expecting rejection.

Automatic Android system backup/device transfer is disabled. Explicit portable
budget exports contain budget data, not account/provider/bank credentials.
Generated files are shared through a scoped FileProvider cache with temporary
read grants. No broad storage permission is requested.

## Build and tests

Use JDK 17+, Android SDK 37, NDK `30.0.16248370`, and Rust stable. Set `JAVA_HOME`,
`ANDROID_HOME`, `NDK_HOME` and put Cargo on `PATH`.

```powershell
rustup target add aarch64-linux-android x86_64-linux-android
cd android
.\gradlew.bat :app:testDebugUnitTest :app:lintDebug :app:assembleDebug
.\gradlew.bat :app:lintRelease :app:assembleRelease
# Optimized, debug-signed build for evaluating responsiveness on a physical device:
.\gradlew.bat :app:assemblePreview
```

Use `bash gradlew` on macOS/Linux. Default builds include ARM64 and x86_64; use
`-PandroidAbis=x86_64` for emulator work. Gradle invokes Cargo directly with the
NDK linker. The native library includes services and accounting code.

```powershell
# Repository root: dependency unit tests are a separate Cargo workspace.
cargo test --locked --manifest-path crates/Cargo.toml
# Desktop integration adapters and existing checks:
cd desktop
npm test
# android/, isolated emulator only (never choose a personal phone):
$env:ANDROID_SERIAL = "emulator-5580"
.\gradlew.bat :app:connectedDebugAndroidTest -PandroidAbis=x86_64
```

Native CI runs shared tests, Kotlin tests, debug/release lint and builds for both
ABIs; **Run workflow** also builds preview and keeps the APKs for three days. Changes to shared Rust also run the desktop checks. AGP built-in Kotlin requires custom shared sources under
`AndroidSourceSet.kotlin`, per [Android's migration guide](https://developer.android.com/build/migrate-to-built-in-kotlin).

Artifacts:
- `app/build/outputs/apk/debug/app-debug.apk`
- `app/build/outputs/apk/preview/app-preview.apk` (optimized device evaluation)
- `app/build/outputs/apk/release/app-release-unsigned.apk` (without signing secrets)
- `app/build/reports/tests/`, `app/build/reports/androidTests/`, lint reports

For a signed release, provide all four existing environment variables:
`URBUDGET_ANDROID_KEYSTORE`, `URBUDGET_ANDROID_KEY_ALIAS`,
`URBUDGET_ANDROID_STORE_PASSWORD`, `URBUDGET_ANDROID_KEY_PASSWORD`.
The default version code matches `versionName` (1.2.3 is 1002003; `scripts/versions.mjs`
sets both). Use `-PversionCode=<next Play version code>` only to upload a version again. This
configuration does not upload or publish anything. Releases are tagged `android-v1.2.3` and
built by `.github/workflows/android-release.yml` ([RELEASING.md](../RELEASING.md)). Google Play checkout requires
the release package, appropriate signing/test track and the configured subscription;
debug's separate application ID cannot validate production product availability.

Use **preview** for everyday sideloaded testing. It uses the same
`app.urbudget.nativeapp` package and debug signing certificate, preserving debug
budgets when installed with `adb install -r`, but enables R8 and resource shrinking
and disables debugging/tooling overhead. It is shell-profileable. Keep debug for
debugger/instrumentation work; its frame timings do not represent release
performance, as explained in the [Compose performance guide](https://developer.android.com/develop/ui/compose/performance).

Published budget snapshots are read-only. Unchanged revision probes reuse their
objects and calculations, read-only queries run outside the edit queue, and list
filtering happens on a worker. UI scope keys use snapshot identity rather than
serializing entire budgets while typing or rendering.

Advanced editor choices, split rows, loan schedules, import reviews and assistant
report tables use bounded lazy lists. Large choice lists are searchable without
discarding options. Payee discovery, match labels, file encoding, Quick assign
display rows and report preparation run off the UI thread; obsolete results are
cancelled or rejected when their budget or inputs change.

Assistant token updates are buffered and published at most every 64 ms while
streaming, with a final flush. The live reply updates independently of history
and settings; unchanged conversation entries retain their identities. Service
reads no longer occupy the local edit queue. In-flight identical reads and
automatic sync attempts are coalesced, with budget/credential guards and refresh
after a sync commits even if its original screen has closed.

Native bank previews use the shared Rust matching engine's compact transport.
Rows refer to pooled candidate sets instead of copying thousands of candidates
into every JSON row. Kotlin reuses those lists, and commits send only selected
matches and required acknowledgments. Rust still revalidates selections against
the current ledger. Desktop's full review format remains compatible.

## Verification and remaining external acceptance

Local verification on 2026-10-21 passed:

- 439 shared Rust tests (303 core, 125 services, 9 JNI adapter, 2 cross-platform);
  two explicitly ignored desktop benchmarks remain ignored.
- 617 desktop UI tests and TypeScript checking; desktop Rust adapter checks/tests.
- 55 native Kotlin unit tests and 13 Android emulator instrumentation tests.
- Both-ABI debug, preview and optimized release APK builds; all three variants' lint with no
  errors; retained JNI/Keystore entry points and 16 KiB native/APK alignment.
- Emulator smoke checks for first-run onboarding, sample budgets, assistant
  settings, bank setup, report drilldowns/editors, and fresh-start persistence
  after restarting the app. Tests used only an isolated API 37 emulator.
- Optimized preview smoke launch on the isolated emulator and in-place update on
  the authorized Moto G77; matching debug certificate, JNI/Keystore retention and
  16 KiB APK alignment verified. Instrumentation stayed on the emulator.

Moto G77 navigation comparison on 2026-10-21 used the same ten tab switches with
0.5-second pauses, after warming the screens and resetting `dumpsys gfxinfo`:

| Frame timing | Original debug | Optimized preview | Preview repeat |
| --- | ---: | ---: | ---: |
| Frames sampled | 998 | 1,040 | 1,014 |
| Median | 12 ms | 12 ms | 12 ms |
| 95th percentile | 30 ms | 18 ms | 18 ms |
| 99th percentile | 109 ms | 40 ms | 42 ms |

This measures the combined code changes and release optimizations during tab
navigation, not every editor or network workflow. Percentiles are bucketed and
the samples are short. The device's modern and legacy jank counters disagree,
so they are not used to claim a jank rate. Raw counters are in the local ignored
`reports/moto-performance-{debug-before,preview-after,preview-repeat}.txt` files.

The advanced-workflow stress check used an isolated emulator with 10,004
transactions, 1,003 categories and 2,000 derived payees. The old optimized build
ran out of Java heap while parsing a 1,000-row bank preview. The updated build
opened the complete review, and late match candidates remained searchable. No
rows were imported during that manual check. A separate JNI regression applies
the last match in a pooled candidate set and verifies the intended transaction;
shared Rust tests expand compact reviews back to the full desktop representation
and check a 1,000-row, 10,000-candidate review stays below 1 MiB on the wire.

The large category-editor sample improved from p95 81 / p99 350 ms (128 frames)
to p95 65 / p99 250 ms (85 frames). Signed-out service-screen navigation measured
p99 31 ms before and 32 ms after. These are short emulator samples, not physical
device or provider latency benchmarks. Compose instrumentation separately checks
that 2,000-option selectors, 2,000-row reviews and 2,000-row assistant report
tables keep the final item reachable while composing only visible rows.

The Moto advanced-workflow check opened and cancelled schedule, loan and payee
editors, typed an unsaved payee name, and visited sync, assistant and bank screens.
The same sequence ran before and after the update; no saves, provider messages,
bank connections or purchases were submitted. Final loan/schedule smoke checks
on the emulator also verified scrolling preserves drafts and reaches the lower
controls.

| Advanced workflow frame timing | Prior optimized build | Final build, first pass | Final build, repeat |
| --- | ---: | ---: | ---: |
| Frames sampled | 1,469 | 1,607 | 1,509 |
| Median | 10 ms | 11 ms | 11 ms |
| 95th percentile | 16 ms | 20 ms | 28 ms |
| 99th percentile | 77 ms | 105 ms | 97 ms |

These short physical-device runs did **not** demonstrate an aggregate navigation
speedup. Screen/editor opening still has occasional slower frames; an isolated
typing check measured p99 9 ms. Temperature and power checks showed no reported
thermal throttling or battery-saver mode. The changes demonstrably remove the
large-import heap failure, eager large lists, repeated presentation work and
service/edit-queue contention; they do not establish that every opening frame or
live provider request is faster. Raw samples are retained locally under
`reports/advanced-*.txt`.

Automated checks cover the shared accounting/service suites, HTTP protocol mocks,
identity changes, encrypted merge/conflict races, native SQLite revision/history
behavior, secret authentication, attachment failures and assistant proposal
guards. Android instrumentation uses synthetic budgets and
files; no real provider keys, bank credentials, sign-in emails or purchases are
used. The optimized release is also checked to keep JNI/Keystore entry points.

Before publishing, exercise real email-code and ChatGPT browser sign-in, provider
responses, SimpleFIN/YNAB accounts, and Play test-track subscription purchase/pending/restore
with authorized test accounts. Passing mocked and local tests establishes code
behavior, not acceptance by those live services. Test signed upgrades with a
backed-up real-version sandbox as well.

The repository's ignored desktop performance test has pre-existing stale stored
hashes: the untouched branch base produces the same actual fingerprints as the
extracted engine. Those golden expectations are not silently rewritten here.
