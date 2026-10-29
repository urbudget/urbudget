# Actual Windows desktop sync smoke evidence

Validated October 28–29, 2026, in the isolated `sync-v2-redesign` worktree.
This used the real debug Tauri executable, WebView2, React frontend, native IPC,
SQLite files and a local workerd/D1/R2 server. No production service, user budget,
signing key or release installer was used.

## Isolation and repeatable entry points

- Build directly from `desktop/`: `cargo build --manifest-path src-tauri/Cargo.toml --bin urbudget`.
- Launch using `node scripts/qa/sync-v2-windows.mjs`; the launcher starts Vite 1420,
  an isolated local Worker 8791, and the app with separate data and WebView profiles.
- Drive the application using the existing repository developer harness:
  `CDP_PORT=9436 node scripts/qa/cdp.mjs eval ...` or `shot <path>`.
  PowerShell sets `$env:CDP_PORT='9436'` before those commands.
- Data/profile/evidence root for this run:
  `crates/target/desktop-sync-v2-qa/1793222385633/`.
- Worker used only a synthetic `@example.test` account with local development
  email codes and a synthetic active subscription. The local-only Worker flag
  enables sync v2. Normal production activation remains off.

The desktop binary was rebuilt after the final core/notification fixes and then
restarted against the same isolated profile. Final `urbudget.exe` SHA-256:
`23A540F20137AD534B960BA8F98C2C5CE3C91D74C1D5522EFD25567CF94B090C`.

## Observed checks

1. Fresh startup displayed the empty Budget library. Native `load_budget` had no
   open budget. Creating `Windows sync QA` through the UI opened and saved it.
2. Adding `QA Checking` with a 1,000 SAR opening balance through the UI saved the
   account and rendered the correct balance.
3. In the actual inline transaction editor, entered payee `Unfinished Windows QA`,
   memo `Raw draft survives desktop crash`, and incomplete Outflow `-`. The durable
   editor checkpoint contained those exact raw values while the saved transaction
   count stayed zero.
4. Abruptly terminated only the isolated app/WebView tree and restarted the same
   profile. The checkpoint survived unchanged. Budgets → Recover unfinished edits
   displayed the exact raw values. Recovery currently shows fields for manual
   review/re-entry; this check does not claim automatic reconstruction of the form.
5. Discarded that synthetic draft through the recovery UI. Signed in and initialized
   sync through the actual native IPC using the local development account, then
   refreshed the frontend. The previously created local budget stayed private.
6. Created `Automatic Windows sync` through the frontend after account setup.
   Within five seconds its native library entry was account-owned, `pending=false`,
   status `uploaded`, and the UI showed Synced. No `sync_now` call or Sync now click
   was issued by the test: the frontend scheduler triggered publication itself.
7. Deleted the private budget, then the last account budget through the Budgets UI
   and its confirmation. This exposed a frontend status-label crash described below.
   After the fix, restored the account budget, opened it, and deleted it again through
   the UI. The app immediately displayed the empty Budget library without reloading;
   `activeId=null`, `budget=null`, and both entries were in Trash with no pending work.
8. Abruptly terminated the isolated app, rebuilt the final native candidate directly,
   and reopened the same profile. It stayed empty with both Trash entries visible.
   The real local server's Trash response contained exactly one account budget,
   confirming the original private budget had not been enrolled or uploaded.

## Bug found and fixed by this run

`CloudSettings` indexed the legacy-only status map using v2 status `trashed`, then
passed `undefined` into the translation function. Deleting the last budget exposed
the settings screen and crashed the frontend into its recovery boundary. The saved
data and backend empty-library state were intact.

The map now covers every current native lifecycle state and falls back safely for
an unknown future state. The shared TypeScript union was updated by the mobile/UI
agent. `npx vitest run src/CloudSignIn.test.tsx` passed all seven tests, including
an empty-library render with nine new lifecycle states and one unknown native state.
The real restore/open/delete repetition and final restart passed after this fix.

## Retained local evidence and limits

The isolated evidence folder contains `state.json`, `draft-before-restart.png`,
`recovered-draft.png`, `delete-last-error.png`, `empty-after-fix.png`,
`final-restart-empty.png`, and the real server response `server-after-trash.json`.
The final screenshot was visually inspected and shows the empty library and both
Trash entries. These files live in ignored build output; the checked-in regression
test and launcher are retained in source.

This validates a Windows debug app with the live Vite frontend. It does not validate
a signed installer, auto-update delivery, production deployment, or macOS/iOS UI.
Credential verification and initial key setup used actual native IPC, so the email
sign-in form itself was not exercised in this smoke test. The remaining budget,
account, editor, recovery, restoration and deletion steps used actual frontend UI.

The isolated app/WebView process tree, launcher, Vite and Worker were stopped
after verification. Ports 1420, 9436 and 8791 had no remaining listener; the other
QA Worker ports 8788–8790 and 8792 were also clear. Synthetic profiles and evidence
were retained, without removing or changing normal user data.
