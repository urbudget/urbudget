// A local server with Sync v2 switched on, for trying sync by hand between a computer and a phone.
//   node scripts/sync-v2-try.mjs
// Sign in with any address: no email is sent, the apps fill in the sign-in code, and every account
// on this server has the Sync plan. The server's data, and the computer's test budgets, stay in
// .wrangler/sync-v2-qa/try between runs; delete that folder to start over. Nothing here reaches
// urbudget.app.
import { resolve } from "node:path";
import { startLocal } from "./sync-v2-local.mjs";

const local = await startLocal({ port: 8788, stamp: "try" });
const stop = async () => { await local.stop(); process.exit(); };
process.on("SIGINT", stop); process.on("SIGTERM", stop);
try {
  // The plan is a row no payment stands behind, for the accounts already here and each new one.
  const plan = id => `INSERT OR IGNORE INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) ${id}`;
  await local.execute(`CREATE TRIGGER IF NOT EXISTS try_everyone_has_the_plan AFTER INSERT ON users BEGIN
  ${plan("VALUES ('comp:' || NEW.id, NEW.id, 'synthetic', 'active', '2026-01-01', '2026-01-01')")}; END;
${plan("SELECT 'comp:' || id, id, 'synthetic', 'active', '2026-01-01', '2026-01-01' FROM users")}`);
  const bootstrap = await fetch(`${local.url}/api/v2/sync/bootstrap`, { headers: { Authorization: `Bearer ${local.token}`, "X-Sync-Client": "2" } }).then(r => r.json());
  if (!bootstrap.activationEnabled) throw new Error("Sync v2 isn't switched on in this server.");
} catch (error) {
  await local.stop();
  throw error;
}
console.log(`Sync v2 test server: ${local.url}
  Sync v2 is on, no email is sent, and every account has the Sync plan.

Computer (PowerShell, from desktop/):
  $env:URBUDGET_API = '${local.url}'; $env:LOCAL_BUDGET_DATA_DIR = '${resolve(local.persist, "desktop")}'; npm run desktop

Phone on USB, or an emulator (PowerShell, from the repository root):
  .\\android\\gradlew.bat -p android :app:assembleDebug "-PsyncTestServer=${local.url}" -PandroidAbis=arm64-v8a
  adb install -r android\\app\\build\\outputs\\apk\\debug\\app-debug.apk
  adb reverse tcp:8788 tcp:8788
  The app is "UrBudget sync test", beside UrBudget. An emulator takes -PandroidAbis=x86_64.
  Run the adb reverse line again whenever the phone has been unplugged.

Sign in to both with the same address, any address. Ctrl+C stops the server and keeps its data.`);
