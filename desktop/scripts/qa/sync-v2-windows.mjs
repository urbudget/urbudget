// Isolated Windows app launcher for the existing cdp.mjs developer-test driver.
// Builds are deliberately separate: never invoke scripts/tauri.mjs/signing setup.
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { startLocal, web } from "../../../web/scripts/sync-v2-local.mjs";
const desktop = resolve(web, "../desktop"), root = resolve(web, "../crates/target/desktop-sync-v2-qa", Date.now().toString());
await mkdir(root, { recursive: true });
const local = await startLocal({ port: 8791 });
const children = [], logs = {};
const launch = (name, executable, args, env) => {
  const child = spawn(executable, args, { cwd: desktop, env: { ...process.env, ...env }, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  logs[name] = ""; child.stdout.on("data", b => { logs[name] += b; }); child.stderr.on("data", b => { logs[name] += b; }); children.push(child); return child;
};
const vite = launch("vite", process.execPath, [resolve(desktop, "node_modules/vite/bin/vite.js"), "--host", "127.0.0.1", "--port", "1420", "--strictPort"], {});
for (let i = 0; i < 100; i++) {
  if (vite.exitCode !== null) throw new Error(logs.vite);
  try { if ((await fetch("http://127.0.0.1:1420")).ok) break; } catch {}
  await new Promise(r => setTimeout(r, 100));
}
const appEnvironment = {
  LOCAL_BUDGET_DATA_DIR: resolve(root, "data"),
  URBUDGET_API: local.url,
  WEBVIEW2_USER_DATA_FOLDER: resolve(root, "webview"),
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9436",
};
const app = launch("app", resolve(desktop, "src-tauri/target/debug/urbudget.exe"), [], appEnvironment);
const state = { root, appPid: app.pid, vitePid: vite.pid, workerPid: local.child.pid, cdpPort: 9436, worker: { url: local.url, email: local.email, accountId: local.accountId, token: local.token }, appEnvironment };
await writeFile(resolve(root, "state.json"), JSON.stringify(state, null, 2));
console.log(JSON.stringify(state));
const stop = async () => { for (const child of children) child.kill(); await local.stop(); await writeFile(resolve(root, "logs.json"), JSON.stringify(logs, null, 2)); process.exit(); };
process.on("SIGINT", stop); process.on("SIGTERM", stop);
