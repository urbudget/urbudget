# Mac automation and App Review recording plan

**Target:** UrBudget 1.0.0 (1), installed through TestFlight on a physical iPhone.

**Status (31 October 2026):** Mac harness implemented under
`ios/automation/app-review/`; 20 host tests pass. A complete physical-device walkthrough
was recorded and all nine flows verified, including disposable-account deletion and
revocation of an independent browser session. The 8:55 native original and caption-guided
1:38 review copy both passed full decoding. After reviewing the edits, the owner explicitly
authorized sending. The six-part response and video were sent to Apple on 31 October 2026;
App Store Connect showed the new message at 1:19 PM and its attachment. Private access
Notes are updated. The strict `--mode final` certification was not run, and no separate
full human playback attestation was collected. No build resubmission or release was performed.

**Release baseline:** `ios-v1.0.0` (`0bd6d44`), bundle `app.urbudget`.

Use the Mac to run Appium and Apple's XCTest automation through a separately signed
WebDriverAgent helper. The helper controls the existing TestFlight app. The iPhone's built-in
Screen Recording creates the video in Photos. First prove that control and native recording
work together, then rehearse and record the review walkthrough.

This work prepares evidence for the existing submission. Keep UrBudget's installed binary,
version, signing and production services unchanged. Building the current UrBudget source would
demonstrate a different app from the submitted build. Feature work remains for 1.0.1.

## Starting facts and remaining inputs

| Item | Starting point or action needed on the Mac |
| --- | --- |
| Physical device | iPhone 16 Pro Max; select its identifier explicitly in Xcode. |
| Installed OS | Verified on the Mac: iOS 27.2 (24B119), physical iPhone 16 Pro Max. |
| Installed app | Last queried: `app.urbudget`, version 1.0.0, build 1, TestFlight distribution. Recheck before every final take. |
| Review access | `review@urbudget.app` has complimentary Sync. Its existing sign-in code is in App Store Connect's private App Review Information Password field. Recheck access before filming. |
| Disposable account | The owner selected a disposable address and approved a separate final-take alias. Both remain in private local configuration; the signed-in Gmail inbox received the preflight code. |
| Sync passphrase | Review account encryption was set up on the Mac session; the sample reached Synced. The passphrase is retained in mode-0600 local storage and saved in private App Review Notes. No existing passphrase was reset. |
| Assistant access | Owner configured provider access privately. A real response and the Report reply form were verified; the report was canceled. |
| Apple reply | The six-part response was sent with the 1:38 video, chapter timestamps, exact Assistant setup and verified iOS version. Delivery and attachment were verified in App Store Connect. |
| Existing video | The Windows screenshot test and Mac checkpoint are superseded by the complete native recorded rehearsal and its caption-guided review copy, accepted for sending by the owner. |

The Mac checkpoint original is a 56.7-second, 1320×2868 native HEVC recording.
Full decode passed; the owner confirmed smooth, readable playback without an automation
overlay. The extra Home Screen pause exposed a stop-gesture targeting problem, fixed by
explicitly selecting SpringBoard before opening Control Center. The original and hash are
retained under ignored `reports/app-review-mac/evidence/`.

The review account's reusable sign-in code was successfully tested by the owner before
capture. The recorded flow shows that account's active entitlement and Sync, then the
disposable account's normal emailed-code sign-in. This avoids filming the reusable review
credential. A recorded first complete rehearsal remains labeled rehearsal until the final
acceptance checks pass. A partial preflight must not be relabeled as a full rehearsal.

Build 1 has a free budgeting core and optional paid Sync access; it does not require a paid app
license. Its account screen predates the Subscribe, Restore and Manage subscription controls
described in the current iOS README. Use the installed build and tagged source to resolve
differences. Demonstrate access through the review account's entitlement, without claiming
that build 1 contains a purchase flow.

The Windows preparation files are under the ignored `reports/app-review-2026-10-31/` directory.
They will not arrive through an ordinary Git checkout. The steps below are self-contained;
retrieve the saved response directly from App Store Connect. Transfer private evidence separately
only if needed, following [the artifact policy](../ARTIFACTS.md).

## Mac execution results

The completed run is `reports/app-review-mac/evidence/recorded-walkthrough-02/`.
It contains the native `IMG_5782.MP4`, `review-walkthrough-guided.mp4`, a timestamped journal,
original/export/edit manifests with SHA-256 hashes, and before/after session-revocation
evidence. These are private, ignored artifacts, not source files.

The sample's Groceries assignment changed from 600.00 to 650.00 SAR. Available changed
from 475.00 to 512.50 after the 12.50 outflow; Unassigned changed from 3,200.00 to 3,150.00,
and Checking's working balance from 4,875.00 to 4,862.50. The account and report showed one
demonstration transaction and 137.50 category spending. A real Assistant reply and its
report form were shown; no report was submitted. The disposable account was registered,
deleted, and its existing browser session lost access. Local sample balances remained.

The final presentation edit is 97.6 seconds, H.264 at 30 fps, with the full 1320×2868 app
screen above a separate caption guide (1320×3136 total). The guide names each app section,
action and result. It omits production details, editing notes, step counts and speed labels.
Routine entry is accelerated up to 2×; the App Review draft discloses the editing.
Repeated Budget/Accounts verification loops, idle time and setup waits are removed.
`guided-review-manifest.json` records all 19 source intervals, speeds, captions and hashes;
`short-edit/render.py` reproduces the edit. The silent source audio track is omitted.
Full decoding and visual inspection of every segment passed. No app outcomes were
synthesized. The original remains on the phone and Mac; the superseded 6:01 edit is
retained locally. The harness dismisses the observed email keyboard with Return before
resolving the request button, avoiding the Hide My Email popup that blocked the first
code request; this was checked without sending another code.

Future presentation exports must stay under two minutes. Keep validation in the evidence
journal, cut its repeated navigation from the presentation, and show key results at normal
speed with concise captions outside the app screen.

The first recording attempt is retained separately because its Assistant tap left the
question unsent. The corrected harness waits for the composer layout and verifies send
acceptance before waiting for a provider reply. Focus was restored and the automation
session, Appium server and WDA test were stopped. UrBudget's installed binary was preserved.

## Mac and phone setup

1. Install a full Xcode version compatible with the phone's actual iOS version and the Mac's
   macOS version. Open Xcode once, complete component installation and select its command-line
   tools in Xcode Settings. Check the [Apple Xcode compatibility table](https://developer.apple.com/xcode/system-requirements).
2. In Xcode Settings, sign in to the developer account and make an Apple Development certificate
   available. WebDriverAgent needs its own provisioned bundle identifier and device registration.
   Use the appropriate team; the repository's configured team is `MXMPFG4LU5`.
3. Connect the unlocked iPhone by a data-capable USB cable. Trust this **Mac** when prompted;
   Windows trust does not establish Mac pairing. Wait until Xcode → Window → Devices and
   Simulators reports the device ready. Copy the device identifier from there.
4. Check Developer Mode and Settings → Developer → Enable UI Automation on the phone. Developer
   Mode was enabled during the Windows check, but verify the current state. The owner handles
   passcode, trust and developer-setting confirmations. Appium may adjust keyboard prediction
   and autocorrection; note the original settings if they should be restored afterward.
5. Install Node 24 LTS with npm 10 or newer, Python 3.11 or newer, and optionally FFmpeg/ffprobe
   for video inspection. Node 24 satisfies the current [Appium requirements](https://appium.io/docs/en/latest/quickstart/requirements/).
6. In TestFlight, confirm UrBudget **1.0.0 (1)**. Avoid automatic replacement with build 2 during
   this session. Keep the phone unlocked and charged, choose a consistent language, and enable
   a suitable Focus mode to keep personal notifications out of the recording.

See Appium's [device preparation](https://appium.github.io/appium-xcuitest-driver/latest/getting-started/device-setup/)
and [provisioning instructions](https://appium.github.io/appium-xcuitest-driver/latest/getting-started/provisioning-profile/).
An iOS update alone does not install or provision the automation helper. A test take can use the
currently installed OS; the final Apple request specifically asks for the latest OS, so verify
that requirement separately and label all footage with the OS actually used.

### Install an isolated automation environment

Run these commands from the repository root on the Mac. They keep dependencies, logs and device
evidence under ignored `reports/`, without changing the application's package manifests.

```sh
export REVIEW_ROOT="$PWD/reports/app-review-mac"
mkdir -p "$REVIEW_ROOT/tools" "$REVIEW_ROOT/evidence"
export APPIUM_HOME="$REVIEW_ROOT/appium"
export PATH="$REVIEW_ROOT/tools/node_modules/.bin:$PATH"

xcode-select -p
xcodebuild -version
node --version
npm --version
python3 --version

npm install --prefix "$REVIEW_ROOT/tools" appium@3
appium driver install xcuitest
appium driver doctor xcuitest
appium --version
appium driver list --installed

python3 -m venv "$REVIEW_ROOT/venv"
"$REVIEW_ROOT/venv/bin/python" -m pip install Appium-Python-Client
"$REVIEW_ROOT/venv/bin/python" -m pip freeze > "$REVIEW_ROOT/python-versions.txt"
```

Reuse this environment on later runs; repeat the three exports after opening a new terminal,
and skip installation when it is already complete. Keep the generated npm lockfile and record
the installed driver version before changing dependencies. Resolve required Doctor failures
before continuing; optional diagnostics are not all needed for a native app walkthrough.

Select a driver compatible with the actual phone and Xcode. At the time of this plan, Appium's
[compatibility table](https://appium.github.io/appium-xcuitest-driver/latest/getting-started/system-requirements/)
lists iOS 27 support from XCUITest driver 11.1.2 / WDA 14.1.1, and iOS 26.4–26.x from driver
10.23.2 / WDA 11.1.5. These are minimums, not instructions to downgrade a working current driver.

### Provision only WebDriverAgent

```sh
appium driver run xcuitest open-wda
```

This opens the installed driver's helper project, as documented in
[Appium's scripts reference](https://appium.github.io/appium-xcuitest-driver/latest/reference/scripts/).
Choose the WebDriverAgentRunner scheme and physical phone. Configure the runner's signing team,
automatic signing and a unique helper identifier owned by that team, for example
`app.urbudget.review.WebDriverAgentRunner` if available. Let Xcode resolve its profile and run
Product → Test once. Stop that Xcode test before letting Appium manage the runner.

Do not open and run the UrBudget application project for this capture. The helper is a separate
installation; its identifier must never be `app.urbudget` or `app.urbudget.native`.

## Prove control before building the full walkthrough

Prepare a disposable sample budget manually first: More → budget row → New budget → Start with
sample money. Name it `App Review Demo` if the UI offers naming. Close any unsaved editor and
leave the app on its normal tab screen. If first launch instead shows onboarding, choose Explore
with sample money. No personal budget should be active during automation or diagnostic captures.

Start Appium in the prepared terminal and leave it running:

```sh
appium --address 127.0.0.1 --port 4723 --log-level warn \
  --log "$REVIEW_ROOT/evidence/appium.log"
```

In a second terminal at the repository root, set the device and signing values. These placeholders
must be replaced with the values verified in Xcode:

```sh
export REVIEW_ROOT="$PWD/reports/app-review-mac"
export REVIEW_UDID='REPLACE_WITH_PHYSICAL_IPHONE_IDENTIFIER'
export REVIEW_TEAM_ID='REPLACE_WITH_SIGNING_TEAM'
export REVIEW_WDA_BUNDLE_ID='REPLACE_WITH_PROVISIONED_HELPER_IDENTIFIER'
```

Run the following connection probe. It activates the installed app and switches between two
tabs; it does not save a transaction, log in, delete data or start recording. Its tab locators are
initial candidates for English build 1 and must be confirmed against the actual accessibility tree.
An absent or ambiguous element stops the probe instead of guessing a coordinate.

```sh
"$REVIEW_ROOT/venv/bin/python" - <<'PY'
import os
from pathlib import Path
from appium import webdriver
from appium.options.ios import XCUITestOptions
from appium.webdriver.common.appiumby import AppiumBy
from selenium.webdriver.support.ui import WebDriverWait

required = ("REVIEW_UDID", "REVIEW_TEAM_ID", "REVIEW_WDA_BUNDLE_ID")
for name in required:
    value = os.environ.get(name, "")
    if not value or value.startswith("REPLACE_"):
        raise SystemExit(f"Set {name} from the verified Xcode configuration")
if os.environ["REVIEW_WDA_BUNDLE_ID"] in ("app.urbudget", "app.urbudget.native"):
    raise SystemExit("The helper must have its own bundle identifier")

options = XCUITestOptions().load_capabilities({
    "platformName": "iOS",
    "appium:automationName": "XCUITest",
    "appium:udid": os.environ["REVIEW_UDID"],
    "appium:bundleId": "app.urbudget",
    "appium:noReset": True,
    "appium:fullReset": False,
    "appium:autoLaunch": False,
    "appium:forceAppLaunch": False,
    "appium:shouldTerminateApp": False,
    "appium:useNewWDA": False,
    "appium:xcodeOrgId": os.environ["REVIEW_TEAM_ID"],
    "appium:xcodeSigningId": "Apple Development",
    "appium:updatedWDABundleId": os.environ["REVIEW_WDA_BUNDLE_ID"],
    "appium:autoAcceptAlerts": False,
    "appium:autoDismissAlerts": False,
    "appium:printPageSourceOnFindFailure": False,
    "appium:newCommandTimeout": 300,
})
driver = webdriver.Remote("http://127.0.0.1:4723", options=options)
try:
    driver.activate_app("app.urbudget")
    wait = WebDriverWait(driver, 20)
    for title in ("More", "Budget"):
        predicate = f'type == "XCUIElementTypeButton" AND label == "{title}"'
        def unique_button(d):
            matches = [e for e in d.find_elements(AppiumBy.IOS_PREDICATE, predicate)
                       if e.is_displayed() and e.is_enabled()]
            if len(matches) > 1:
                raise RuntimeError(f"Ambiguous tab: {title}")
            return matches[0] if matches else False
        wait.until(unique_button).click()
    evidence = Path(os.environ["REVIEW_ROOT"]) / "evidence"
    evidence.mkdir(parents=True, exist_ok=True)
    driver.save_screenshot(str(evidence / "control-probe.png"))
    print("Tab commands completed; visually confirm More then Budget on the phone.")
finally:
    driver.quit()
PY
```

The session uses an installed `bundleId` and preserves app data. Never add an `app`/IPA path,
`fullReset: true`, uninstall, clear-data or reinstall commands to resolve a capture failure.
The relevant settings are documented in [XCUITest capabilities](https://appium.github.io/appium-xcuitest-driver/latest/reference/capabilities/).

**Control checkpoint:** observe both tab changes on the physical phone; then use the Appium
Inspector or a small follow-up probe to verify a scroll and typing into a disposable memo that
is canceled. Inspect the actual tree if the candidate tab locators fail. Do not classify a
successful connection or screenshot alone as working touch control.

## Test native recording alongside automation

Before developing a long script, make a 15–30 second native recording containing the control
probe. Return to the Home Screen, open Control Center, tap Screen Recording, wait three seconds,
close Control Center, then run the probe. Stop recording on the phone and verify that Photos
saved a playable clip. Human start and stop taps are acceptable for this first checkpoint.

Use [Apple's built-in recording procedure](https://support.apple.com/en-us/102653). Apple states
that screen recording and screen mirroring cannot operate together. Close active iPhone
Mirroring, AirPlay mirroring and other capture sessions before this test. Appium's screenshots
are diagnostics; the final video must come from Photos.

Check the saved file for smooth scrolling, readable text and any “Automation Running” overlay.
Do not assume the native video matches Appium's screenshots. If the helper blocks recording,
input or legibility, retain the short failure clip and investigate the actual Xcode/WDA/OS
combination before attempting the full take. A manually operated native recording remains a
usable fallback, but it does not establish automated control.

For fully automated start/stop, inspect SpringBoard and Control Center on this phone. Implement
the gestures using measured screen dimensions and observed controls, then verify the recording
indicator and the saved Photos asset. If those controls are unavailable through XCTest, retain
the two manual recording taps and automate the app walkthrough. Do not substitute a screenshot
stream or Appium recording API and describe it as the iPhone's built-in Screen Recording.

## Implementation work on the Mac

Create the following harness only after the control and recording checkpoints pass. The paths
are planned additions, not existing executable commands. Keep it independent of the UrBudget
build system and run a single session on the selected physical phone.

| Planned file under `ios/automation/app-review/` | Responsibility and acceptance check |
| --- | --- |
| `README.md` and pinned requirements | Exact tested Mac/Xcode/Appium/driver/client versions, setup, invocation and manual handoffs. |
| `session.py` | Validate physical UDID, target bundle and observed version/build; preserve app data; allow only the separate WDA helper to be installed. Refuse final capture if identity cannot be established. |
| `locators.py` | Locators obtained from build 1's actual tree. Prefer accessible identifiers or scoped labels; require a unique visible match. Do not invent identifiers that require rebuilding the app. |
| `walkthrough.py` | Ordered stages below, baseline amounts, explicit postconditions, bounded waits and pauses for viewers. Support individual stages and a complete rehearsal. |
| `native_recording.py` | Verified system-UI start/stop or explicit manual checkpoints. Record capture method and confirmation of Photos save. |
| `verify_video.py` | Probe the selected exported file, decode it, preserve its hash and write an honest coverage manifest. Human playback review remains required. |
| `tests/` | Host tests for money-delta assertions, identity rejection, duplicate-save prevention, secret redaction and the disposable-account deletion guard. |

Wait for expected screen state instead of sleeping through unknown UI. Pause roughly two seconds
on completed results for readability. On a timeout, stop the stage, record its last confirmed
state and capture only sample-data diagnostics. Never repeat Save, Sync enrollment or Delete
blindly after an uncertain response. A rerun must inspect the existing result before retrying;
a clean take can use a newly created sample budget without deleting the previous one.

Read credentials through a private local configuration or interactive input. Keep codes, API
keys and passphrases out of command arguments, Git and console/page-source logs. Pause automation
for the owner's passcode and provider authentication. Normal email codes can use a private
handoff or a mode-0600 receipt from the explicitly authorized mailbox session. Before deleting
an account, require an explicitly selected disposable address, match it to the signed-in
identity and reject `review@urbudget.app` or any other account. Default deletion to disabled.

## Rehearsal and recording script

Use the same sequence for rehearsal and filming. Record baseline amounts before edits and use
the displayed currency; the amounts below are examples within the sample budget, not real money.
Allow about 6–8 minutes, extending it for real network responses rather than hiding failed steps.

| Approximate time | Action | Required visible result |
| --- | --- | --- |
| Before filming | Create a fresh sample budget, select a cash/checking sample account and an ordinary spending category such as Groceries. Record assigned, available, unassigned and account balances. Prepare private provider access and both account identities. | Disposable data only; enough unassigned money to allocate 50.00. No unsaved editor or personal information. |
| 0:00 | Start native recording on the Home Screen and tap the installed UrBudget icon. | A visible launch into the sample budget. A test session's programmatic activation is insufficient for this final opening shot. |
| 0:20 | Budget → selected category → increase Assigned by 50.00, then save/close. If the field is an absolute total, enter its previous value plus 50.00. | Assigned and Available each increase by 50.00; Unassigned decreases by 50.00. |
| 1:00 | Transactions → add Outflow of 12.50. Payee `Review Cafe`; same category and sample cash/checking account; today; memo `App Review demonstration`. Save once. | One new transaction in the register. Category Available and the account's working balance each decrease by 12.50; Assigned stays unchanged. Compare the working balance, not a cleared-only balance. |
| 2:00 | Accounts → same account's register; then Reports → a populated spending report, including today's date. | The transaction appears in its account and relevant report. The selected category's net Available change from the initial baseline is +37.50. |
| 2:45 | More → Account & sync → show `review@urbudget.app`, signed in privately before capture. Verify its reusable code beforehand without recording it; ordinary sign-in appears in the disposable-account stage. | Signed-in review identity and active complimentary Sync entitlement. No invented Subscribe/Restore step. |
| 3:15 | Set up or unlock sync according to the account's actual state. Start syncing only this sample budget, then Sync now. | Successful synchronization status. Record any new passphrase privately for Apple's access instructions. Verify cloud download separately during rehearsal if a second device is available. |
| 4:15 | More → Assistant. Ask “Summarize this sample budget and explain how much is available to spend.” Open the answer's flag / Report reply form, then cancel. | A real provider response and the reporting form. Do not submit a false report. If provider access is missing, show the setup accurately and mark this demonstration incomplete. |
| 5:15 | Sign out of the review account. Enter the agreed new disposable email, request its normal email code and sign in. | Normal account creation through email verification. Keep the inbox and unrelated messages outside the recording. |
| 6:00 | On that disposable identity only: Delete account and cloud data → enter the same disposable email → Delete. Reopen the local sample budget afterward. | Account deletion succeeds, the user is signed out and local sample data remains. During rehearsal, verify the old account session no longer authorizes access. If re-registering the address is allowed, a new account does not disprove deletion. |
| 6:45 | Return to a useful budget or report screen, pause, then stop native recording. | Photos saves the recording; the original remains on the phone until the exported copy is checked. |

If registration and deletion are rehearsed first, use another unused disposable address for the
final take so the registration footage actually shows a new account. Never rely on a previous
test's deletion result. The review account must stay available for Apple throughout.

Build 1 has no public user feed or direct messaging. Describe the actual assistant-reporting
mechanism instead of inventing user blocking. If showing optional Bank connections or Import
from YNAB, use sample/test credentials and show their real setup screens; do not claim that
external bank or YNAB connectivity was demonstrated unless it actually succeeded.

## Export and inspect the video

AirDrop the selected recording to the Mac, export its unmodified original from Photos, or use
Image Capture over USB. Copy only the new review recording. Preserve it under the local
evidence directory, retaining its real extension; a renamed `.mov` is not an MP4 conversion.
Record the capture time, device, actual OS, app version/build, capture method and file hash in a
private manifest. Keep Apple credentials out of that manifest.

With `REVIEW_VIDEO` set to the selected original, run:

```sh
export REVIEW_VIDEO='/absolute/path/to/exported-original.MOV'
shasum -a 256 "$REVIEW_VIDEO"
ffprobe -v error -show_entries \
  format=duration,size:stream=index,codec_name,codec_type,width,height,avg_frame_rate,r_frame_rate \
  -of json "$REVIEW_VIDEO" > "$REVIEW_ROOT/evidence/video-probe.json"
ffmpeg -v error -i "$REVIEW_VIDEO" -f null -
```

Watch the entire video. Verify launch, readable input, saved transaction, balance changes,
reports, successful login/Sync, registration, deletion and reporting coverage. Confirm there are
no personal notifications, keys, passphrases, inbox content or long stalls. Native recordings may
use variable frame rate; a nominal frame rate alone cannot establish smoothness. Do not generate
intermediate frames or repeat a few screenshots to disguise missing motion.

If the upload target needs a compatible MP4, keep the original and create a separate copy:

```sh
ffmpeg -n -i "$REVIEW_VIDEO" -map 0:v:0 -map '0:a?' \
  -c:v libx264 -crf 20 -preset medium -pix_fmt yuv420p \
  -c:a aac -movflags +faststart "$REVIEW_ROOT/evidence/review-compatible.mp4"
```

Check the copy's color, text, duration and playback too. Upload the verified artifact or prepare
a reviewer-accessible link, then test access without the owner's account. Preserve real sequence
and outcomes; any explanatory captions or omitted waits must be clear rather than imply an
untested function succeeded.

## Finish the Apple response

Open the [existing App Review conversation](https://appstoreconnect.apple.com/apps/6825535486/distribution/reviewsubmissions/details/eef5fac9-a012-4f97-bfa0-ff56ea751dbf)
and continue its saved draft. Replace `[VIDEO_LINK_OR_ATTACHMENT]` and `[IOS_VERSION]` with the
verified artifact and actual capture OS. Add useful timestamps after the take, not before it.
Confirm the app name, version 1.0.0 (1), physical device and what the video demonstrates.

Complete all six requests from Apple's message:

1. Physical-device video with the requested flows and honest OS identification.
2. Purpose and audience: personal envelope budgeting, planning spending, tracking transactions
   and understanding balances and reports.
3. Exact access steps, free sample-budget entry, private review login and current Sync/passphrase
   instructions. Account state may have changed during filming, so recheck it afterward.
4. Actual external services: Cloudflare infrastructure, Resend email, Polar website payments,
   and optional SimpleFIN, YNAB, OpenAI/Anthropic integrations where applicable to build 1.
5. Core regional behavior and any real provider, bank-coverage or service availability differences.
6. Applicable regulated-service or third-party-material explanations. Describe the app's actual
   role and licensed material; do not invent permissions or certifications.

Keep the response aligned with the installed build's free core and optional Sync entitlement.
Copy the finalized access information into App Review Information Notes as Apple requested.
Follow [Apple's reply instructions](https://developer.apple.com/help/app-store-connect/manage-submissions-to-app-review/reply-to-app-review-messages/).
Preparing a draft does not send it: leave Reply, Resubmit and release actions for the owner's
explicit instruction. Retaining the same build does not guarantee a particular review queue time.

## Mac acceptance checklist

- [x] Xcode sees the physical phone; exact installed OS and TestFlight 1.0.0 (1) are recorded.
- [x] Separately provisioned WDA starts and controls the existing app without replacing it.
- [x] Tap, scroll and canceled text-entry probes work against observed controls.
- [x] A native recording checkpoint succeeds while WDA controls the app; playback and overlays checked.
- [x] The walkthrough harness has observed locators, money assertions and safe failure handling.
- [x] A complete recorded rehearsal covers all required flows, with observed recovery from the system email suggestion.
- [x] Native footage starts with launch and visibly covers all applicable requested flows.
- [x] Original exported, hash retained, original and annotated copy fully decoded.
- [ ] Separate full human playback/privacy attestation collected (not collected; owner explicitly authorized sending after reviewing the edits).
- [x] Review login was verified before capture; the protected account was preserved and private Sync instructions updated.
- [x] Six-part Apple response and private Notes saved with actual device, build and video details.
- [x] Final six-part response and 1:38 review copy sent on explicit owner authorization; new message and attachment verified. No build resubmission or release performed.

## Handoff to the Mac session

Copy this instruction into the Mac's repository session after transferring this plan:

> Follow `docs/plans/2026-10-31-ios-app-review-mac-automation.md`. Start with the physical-device
> identity, WDA control probe and 15–30 second native-recording test. Then implement the planned
> harness against the observed build 1 UI and rehearse the sample-budget walkthrough. Preserve
> installed TestFlight `app.urbudget` 1.0.0 (1); build only the separate WDA helper. Keep credentials
> and footage in ignored reports. Pause for physical-device authentication and the disposable
> account details when needed. Record the real test results and prepare the Apple response;
> leave sending, resubmission and release pending my instruction.
