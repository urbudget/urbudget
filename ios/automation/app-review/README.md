# Physical iPhone App Review walkthrough

Controls the **existing TestFlight `app.urbudget` 1.0.0 (1)** with a separately
signed WebDriverAgent (WDA). It never builds, installs or resets UrBudget. Native
iPhone Screen Recording creates the footage in Photos; Appium screenshots are
diagnostics only. See the [recording plan](../../../docs/plans/2026-10-31-ios-app-review-mac-automation.md).

## Tested environment

Observed on 31 October 2026:

| Component | Version |
| --- | --- |
| macOS | 27.0.1 (26A434) |
| Xcode | 27.1 (27A9275) |
| Physical phone | iPhone 16 Pro Max, iOS 27.2 (24B119) |
| Node / npm | 24.14.1 / 11.11.0 |
| Python | 3.12.14 (3.11+ required) |
| Appium | 3.9.0 |
| XCUITest driver / WDA | 12.22.7 / 16.17.6 |
| Python Appium client / Selenium | 6.0.12 / 4.52.0 |

The device's `builtByDeveloper` flag did not establish distribution: TestFlight's
Previous Builds screen showed **Open** for 1.0.0 (1) and **Install** for build 2.
Opening build 1 was verified to activate `app.urbudget`. Every session checks
the physical UDID and installed version before connecting; final takes also require
the TestFlight UI check. Apple's [security release list](https://support.apple.com/en-sa/100100)
listed iOS 27.2 as current when checked on 31 October; recheck for later takes.

## Isolated installation

Run from the repository root. Use the working installation again instead of
reinstalling it between stages. All dependencies, device identifiers, credentials,
videos and logs belong under ignored `reports/app-review-mac/`.

```sh
export REVIEW_ROOT="$PWD/reports/app-review-mac"
export APPIUM_HOME="$REVIEW_ROOT/appium"
export PATH="$REVIEW_ROOT/tools/node_modules/.bin:$PATH"
umask 077
mkdir -p "$REVIEW_ROOT/tools" "$REVIEW_ROOT/evidence"
npm install --prefix "$REVIEW_ROOT/tools" appium@3.9.0
appium driver install xcuitest@12.22.7
appium driver doctor xcuitest
python3 -m venv "$REVIEW_ROOT/venv"
"$REVIEW_ROOT/venv/bin/python" -m pip install -r ios/automation/app-review/requirements.txt
```

Use Xcode to provision **only** WebDriverAgentRunner, with an identifier such as
`app.urbudget.review.WebDriverAgentRunner` and the selected signing team. The
identifier must be separate from both UrBudget apps. Keep Xcode's derived data in
`$REVIEW_ROOT/wda-build`. Connect by USB, trust the Mac, unlock the phone and enable
Developer Mode/UI Automation. The owner handles system authentication prompts.

The successful setup built the helper for testing, then ran `xcodebuild
test-without-building`. Appium attached to the local WDA URL reported by that test.
Keep that test alive while using `wda_url`; do not start a second WDA manager.
Omit `wda_url` to let Appium use the already built helper in `wda-build` instead.
Do not supply an IPA or an app build path to solve a connection error.

Start the isolated server in a separate terminal:

```sh
APPIUM_XCUITEST_PREFER_DEVICECTL=1 appium --address 127.0.0.1 --port 4723 \
  --log-level warn --log "$REVIEW_ROOT/evidence/appium.log"
```

Avoid debug/HTTP logs, Inspector snapshots and screenshots during credential
entry. WDA is an automation endpoint: keep its local URL on a trusted network and
stop your helper/server after use. The harness disables automatic alert handling,
app reset, app termination and page-source-on-failure logging.

## Private configuration

Create `$REVIEW_ROOT/private.json` locally, mode `0600`. Replace placeholders with
the selected phone/team/helper. The budget must be a disposable sample, with no
unsaved editor. Configure the actual visible name/currency rather than assuming
the sample is called App Review Demo.

```json
{
  "udid": "REPLACE_WITH_PHYSICAL_UDID",
  "team_id": "REPLACE_WITH_SIGNING_TEAM",
  "wda_bundle_id": "app.urbudget.review.WebDriverAgentRunner",
  "sample_budget_confirmed_by_owner": true,
  "sample_budget_name": "My budget",
  "currency": "SAR",
  "category": "Groceries",
  "account": "Checking",
  "disposable_email": "unused-address@example.invalid"
}
```

Optionally add `"wda_url": "http://VERIFIED_LOCAL_IP:8100"` for the running helper.
Only local/private numeric addresses or localhost are accepted. API keys and
passphrases are **not** configuration fields: enter those privately on the phone.
An authorized mailbox session can supply a disposable OTP through `--otp-file`
as described below. The reusable review code is in App Store Connect's private
App Review Information Password field. Do not reset an existing Sync passphrase.

Leave TestFlight on UrBudget → Previous Builds, where build 1 has **Open**.
The harness fails instead of pressing Install or Update. Keep English UI and
portrait orientation; these locators target build 1, not current source. Account
and report navigation retains its tab stack, which the harness handles explicitly.

## Rehearsal and recovery

Run the host checks first (no phone required):

```sh
"$REVIEW_ROOT/venv/bin/python" -m unittest discover -s ios/automation/app-review/tests -v
"$REVIEW_ROOT/venv/bin/python" ios/automation/app-review/session.py
```

Run individual stages to validate the installed UI, or use `--stage all` for the
ordered sequence. The same run name preserves the journal and money baseline:

```sh
"$REVIEW_ROOT/venv/bin/python" ios/automation/app-review/walkthrough.py \
  --run rehearsal-01 --stage baseline
"$REVIEW_ROOT/venv/bin/python" ios/automation/app-review/walkthrough.py \
  --run rehearsal-01 --stage assign
```

Stages: `baseline`, `launch`, `assign`, `outflow`, `reports`, `review_access`,
`sync`, `assistant`, `registration`, `deletion`. A full rehearsal on a fresh
sample uses a **new** run name and `--stage all --allow-disposable-deletion`.
Deletion is disabled unless that explicit flag is passed. The selected address,
signed-in identity and confirmation text must all match; the review identity and
its plus-addressed variants are always rejected. Use the deletion flag only after
the owner authorizes this named disposable-account test. The selected account's
cloud data is permanently deleted; local sample balances are checked afterward.

Unconfigured launch, credential entry and Sync enrollment require owner handoffs.
Already unlocked Sync is verified against the selected sample's own status row.
The `review_access` stage checks the protected identity and active entitlement;
ordinary sign-in is demonstrated by the registration stage. At each terminal prompt,
type `done` only after performing and checking the displayed instruction, or
`stop` to leave the stage incomplete. No private value is requested by the prompt.
If an agent runs this CLI, it must relay each handoff and wait for the owner's
actual response; it must never supply `done` speculatively. An observed Home icon
can be configured with `verified_home_icon: {"swipes_left": 1, "x": 130, "y": 200}`
only after checking that this exact icon opens the target bundle on that phone.
Recheck after changing Home Screen layout; these coordinates are not portable.

For an explicitly authorized mailbox, `--otp-file reports/app-review-mac/private-otp.json`
waits up to five minutes for a regular mode-0600 JSON file with `email`, `code`,
`received_at` and `expires_at` (timezone-aware ISO dates). The receipt must match
the selected address, arrive after the request, and expire within ten minutes.
Only a six-digit ASCII code is accepted; the file is consumed once and its contents
are never journaled. Read only the selected service's authentication messages.
Do not pass codes through command arguments, source dumps or terminal prompts.
Deletion records whether an independent old-session revocation check was performed;
sign-out on the phone alone does not establish that separate check.

Mutating actions record intent **before** acting. A lost response leaves that
intent on disk. A rerun checks the existing result and never automatically repeats
Save, Sync now, the Assistant send, or Delete. Some preparation failures therefore
require inspecting/canceling the draft and starting a new run with a fresh sample.
Do not erase an action record or reuse a changed sample to force a retry. Failure
diagnostics contain stage/error type; screenshots are limited to sample-budget
stages. Do not commit journals or raw device logs.

The assignment is the baseline absolute total plus 50.00. The outflow is 12.50
with Review Cafe, Groceries, Checking, today and the demonstration memo. Assertions
check available, assigned, unassigned, the account's **working** balance, the
transaction and category spending. Final available must be baseline +37.50.

## Final native take and verification

Only attempt final mode after all rehearsal stages pass. `--capture-rehearsal`
can retain a first complete rehearsal as native footage; its manifest remains
ineligible for final certification. Create a fresh sample and
configure **another unused email address**. Configure Assistant access privately
before recording; keep inboxes, keys, passphrases and personal notifications out
of the take. Review access must remain available for Apple afterward.

```sh
"$REVIEW_ROOT/venv/bin/python" ios/automation/app-review/walkthrough.py \
  --run final-01 --mode final --allow-disposable-deletion \
  --rehearsal "$REVIEW_ROOT/evidence/rehearsal-01/journal.json"
```

The opening requires a real Home Screen icon tap, then verifies the
active bundle (two UrBudget variants can be installed). Control Center uses the
observed Screen Recording button, verifies its state, and explicitly activates
SpringBoard before gestures. Use `--manual-recording` if that system UI differs:
the owner starts/stops the native recording and confirms Photos saved it.
If any stage fails, stop the native recording and retain the incomplete clip.

Export that take's **unmodified original** from Photos, AirDrop or Image Capture
into the run's evidence directory, retaining its real extension. Keep the phone
original. Match capture time and contents, then run:

For a USB export, first identify the new native video's exact DCIM path and capture
time using the installed driver's AFC service. Then copy that one original with:

```sh
node ios/automation/app-review/export_original.mjs "$REVIEW_ROOT/private.json" \
  /DCIM/NNNAPPLE/IMG_NNNN.MP4 "$REVIEW_ROOT/evidence/final-01"
```

The exporter refuses overwrites, verifies the source/copy sizes and retains device
timestamps. It does not delete the Photos original. A successful copy is evidence
of a saved device file; it does not establish human playback quality.

```sh
"$REVIEW_ROOT/venv/bin/python" ios/automation/app-review/verify_video.py \
  "$REVIEW_ROOT/evidence/final-01/EXPORTED_ORIGINAL.MP4" \
  --journal "$REVIEW_ROOT/evidence/final-01/journal.json" \
  --output "$REVIEW_ROOT/evidence/final-01/verification"
```

This hashes, probes and fully decodes the file. Watch the **entire** original for
legible text, real motion, every required flow, privacy and long stalls. Then
repeat with `--original-confirmed --playback-reviewed` only if those checks passed.
The playback flag is an explicit human attestation, not automatic video understanding.
The original-match flag can be established by matching device capture metadata
and inspected frame contents; it does not imply human playback acceptance.
The manifest also requires a final-mode journal with all stage timestamps inside
the same capture. A decoded test clip or passed rehearsal cannot certify a final
walkthrough. Nothing uploads, sends an Apple response, resubmits or releases.

## Execution results and remaining acceptance

The physical-device connection, observed tab/scroll/canceled memo controls and
native recording checkpoint passed. The test original was 56.7 seconds (including
a stop-gesture delay), 1320×2868 HEVC, fully decoded, and reviewed by the owner as
smooth/readable with no automation overlay. The harness fixes the SpringBoard
targeting that caused that delay.

The complete recorded rehearsal is in ignored
`reports/app-review-mac/evidence/recorded-walkthrough-02/`. All nine flows passed,
including real Assistant output/report-form cancellation, normal email registration,
deletion and revocation of an independent browser session. The local sample was retained.
The original `IMG_5782.MP4` is 8:55, 1320×2868 HEVC. The current H.264 review copy,
`review-walkthrough-guided.mp4`, is 97.6 seconds with captions focused on each app section,
action and result. Production details, step counts and speed labels are omitted from the
guide. Playback is accelerated up to 2× and repeated verification navigation and waits are
removed, as disclosed in the App Review draft.
The app screen is preserved above a separate caption area (1320×3136 total, 30 fps).
The silent source audio track is omitted. Full decoding and visual inspection of every
segment passed. `guided-review-manifest.json` retains all 19 source intervals, speeds,
captions and hashes; `short-edit/render.py` reproduces the edit. Original footage remains
on the phone and Mac, and the superseded 6:01 edit remains locally. Presentation exports
must stay under two minutes; preserve full verification evidence separately.

Two observed UI issues are handled explicitly: the multiline Assistant composer must
settle before Send, and iOS Hide My Email can cover the code-request button while the
email field is focused. Return on the observed email keyboard dismisses that keyboard
without requesting a code; this was verified on the phone. Uncertain sends/requests
retain their original journal intent and require inspection, never blind repetition.

20 host tests pass. The strict final-mode recording certification was not run and no
separate full human playback attestation was collected. After reviewing the edits, the
owner explicitly authorized sending. On 31 October 2026 the six-part reply and 1:38
caption-guided video were sent to Apple; App Store Connect showed Messages (2), the new
reply at 1:19 PM, and its downloadable attachment. Private Notes include the same access
instructions and clarify the optional Assistant's provider setup. The exact sent text,
receipt and screenshot are retained under ignored `reports/app-review-mac/`.
No build resubmission or release was performed. Focus was restored and local automation stopped.
