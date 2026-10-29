// Local-only Android lifecycle harness. It never accepts a hosted server origin.
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { resolve } from "node:path";
import { startLocal, web } from "./sync-v2-local.mjs";

const root = resolve(web, "..");
const report = resolve(root, "android/app/build/reports/sync-v2");
const checkOnly = process.argv.includes("--check");
const port = checkOnly ? await new Promise((done, fail) => {
  const reservation = createServer();
  reservation.once("error", fail);
  reservation.listen(0, "127.0.0.1", () => { const selected = reservation.address().port; reservation.close(() => done(selected)); });
}) : 8788;
const email = "sync-v2-android-20261028@example.test";
const adb = process.env.ADB ?? "adb";
const serial = process.env.ANDROID_SERIAL ?? "emulator-5554";
if (!/^emulator-[0-9]+$/.test(serial)) throw new Error("This harness only targets a disposable emulator serial.");
await mkdir(report, { recursive: true });
let local;
let active;
async function command(executable, args, { tolerateFailure = false, quiet = false } = {}) {
  const child = spawn(executable, args, { cwd: root, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  active = child;
  let output = "";
  child.stdout.on("data", data => { output += data; if (!quiet) process.stdout.write(data); });
  child.stderr.on("data", data => { output += data; if (!quiet) process.stderr.write(data); });
  const code = await new Promise((done, fail) => { child.once("exit", done); child.once("error", fail); });
  active = undefined;
  if (code && !tolerateFailure) throw new Error(`Command failed (${code}): ${executable}`);
  return output;
}
const device = (args, options) => command(adb, ["-s", serial, ...args], options);
for (const signal of ["SIGINT", "SIGTERM"]) process.once(signal, async () => { active?.kill(); await local?.stop(); process.exit(1); });
try {
  local = await startLocal({ port, stamp: `android-${Date.now()}` });
  await local.execute(`INSERT INTO users(id,email,created_at) VALUES ('usr_android_sync_v2_qa','${email}','2026-01-01');
INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:android-sync-v2-qa','usr_android_sync_v2_qa','synthetic','active','2026-01-01','2026-01-01');`);
  if (checkOnly) {
    const post = async (path, body) => {
      const response = await fetch(`${local.url}${path}`, { method: "POST", headers: {"Content-Type":"application/json"}, body: JSON.stringify(body) });
      if (!response.ok) throw new Error(`Local seed check failed (${response.status}): ${path}`);
      return response.json();
    };
    const login = await post("/api/v1/auth/start", {email});
    if (!/^[0-9]{6}$/.test(login.devCode)) throw new Error("Local QA login codes are unavailable.");
    const verified = await post("/api/v1/auth/verify", {email, code:login.devCode, client:"desktop", deviceName:"Android CI seed check"});
    const bootstrap = await fetch(`${local.url}/api/v2/sync/bootstrap`, {headers:{Authorization:`Bearer ${verified.token}`}});
    if (!bootstrap.ok || !(await bootstrap.json()).activationEnabled) throw new Error("Synthetic Android subscription or v2 activation is unavailable.");
    console.log("Android CI local seed/login/subscription/v2 bootstrap check passed.");
  } else {
    await device(["wait-for-device"]);
    await device(["install", "-r", "android/app/build/outputs/apk/debug/app-debug.apk"]);
    await device(["install", "-r", "android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"]);
    await device(["reverse", "tcp:8788", "tcp:8788"]);
    await device(["logcat", "-c"]);
    const output = await device(["shell", "am", "instrument", "-w", "-e", "class", "app.urbudget.nativeapp.SyncV2RuntimeTest",
      "-e", "syncQaOrigin", local.url, "app.urbudget.nativeapp.test/androidx.test.runner.AndroidJUnitRunner"]);
    await writeFile(resolve(report, "instrumentation.txt"), output);
    // `adb shell am instrument` can exit zero even when the test runner failed or skipped.
    if (!/OK \(1 test\)/.test(output) || /FAILURES!!!|INSTRUMENTATION_FAILED|Process crashed/.test(output))
      throw new Error("Android sync-v2 lifecycle instrumentation did not report one passing test.");
  }
} finally {
  if (!checkOnly) {
    try { await writeFile(resolve(report, "logcat.txt"), await device(["logcat", "-d", "-t", "3000"], {tolerateFailure:true,quiet:true})); } catch {}
    try { await device(["reverse", "--remove", "tcp:8788"], {tolerateFailure:true}); } catch {}
  }
  if (local) {
    await writeFile(resolve(report, "worker.log"), local.logs());
    await local.stop();
  }
}
