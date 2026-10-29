// Tests the actual released NSIS installer only on a disposable GitHub-hosted Windows runner.
// Never set these runner variables locally to bypass the guard: release builds use the OS profile.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { existsSync, writeFileSync } from "node:fs";
import { mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { basename, isAbsolute, join, relative, resolve, sep } from "node:path";

assert(process.platform === "win32" && process.env.CI === "true" && process.env.GITHUB_ACTIONS === "true" &&
  process.env.RUNNER_ENVIRONMENT === "github-hosted" && process.env.RUNNER_OS === "Windows",
  "Installer smoke is restricted to disposable GitHub-hosted Windows runners; refusing local execution.");
assert(process.env.RUNNER_TEMP && isAbsolute(process.env.RUNNER_TEMP), "RUNNER_TEMP must be absolute.");
const temp = await realpath(process.env.RUNNER_TEMP);
const args = Object.fromEntries(Array.from({length: (process.argv.length - 2) / 2}, (_, i) => [process.argv[2 + i * 2], process.argv[3 + i * 2]]));
assert(/^\d+\.\d+\.\d+$/.test(args["--version"] ?? ""), "Supply --version X.Y.Z.");
function withinTemp(path) {
  assert(path && isAbsolute(path), "Smoke paths must be absolute.");
  const part = relative(temp, resolve(path));
  assert(part && !part.startsWith(".." + sep) && part !== ".." && !isAbsolute(part), "Smoke paths must stay inside RUNNER_TEMP.");
  return resolve(path);
}
const installer = withinTemp(await realpath(withinTemp(args["--installer"])));
assert(installer.toLowerCase().endsWith("-setup.exe"), "Expected the downloaded release NSIS installer.");
const evidence = withinTemp(args["--evidence"]);
await mkdir(evidence, {recursive: true});
withinTemp(await realpath(evidence));
const root = withinTemp(join(temp, "urbudget-release-smoke-" + randomUUID()));
await mkdir(root);
const installed = join(root, "app"), executable = join(installed, "urbudget.exe"), webview = join(root, "webview");
assert(!/\s/.test(installed), "NSIS /D must be the final unquoted argument; use a runner temp path without spaces.");
const started = Date.now(), deadline = started + 180_000;
const owned = new Set(), sockets = new Set();
const rule = "UrBudget-release-smoke-" + randomUUID();
const report = {version: args["--version"], installer: basename(installer), startedAt: new Date(started).toISOString(), checks: [], status: "running"};
let firewallAttempted = false, failure;
const ps = value => "'" + value.replaceAll("'", "''") + "'";
const sha = bytes => createHash("sha256").update(bytes).digest("hex");
function limit(ms, cleanup = false) {
  const remaining = deadline - Date.now() - (cleanup ? 0 : 20_000);
  assert(remaining > 0, "Global 180-second smoke deadline reached (20 seconds reserved for cleanup).");
  return Math.min(ms, remaining);
}
function killTree(child) {
  if (child.exitCode !== null || !child.pid) return;
  // Kill the tree while the parent PID is still live: killing just NSIS can orphan its children.
  const killer = spawn("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], {windowsHide: true, stdio: "ignore"});
  killer.on("error", () => child.kill()); killer.unref();
}
// Helpers reserve cleanup time; this watchdog also bounds lingering stdio/socket handles.
setTimeout(() => {
  for (const child of owned) killTree(child);
  report.status = "failed"; report.error = "Hard 180-second installer smoke deadline exceeded.";
  report.finishedAt = new Date().toISOString(); report.elapsedMs = Date.now() - started;
  try { writeFileSync(join(evidence, "result.json"), JSON.stringify(report, null, 2)); } catch {}
  if (firewallAttempted) {
    const remove = spawn("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command",
      "Get-NetFirewallRule -DisplayName " + ps(rule) + " -ErrorAction SilentlyContinue | Remove-NetFirewallRule"], {windowsHide: true, stdio: "ignore"});
    remove.on("error", () => {}); remove.unref();
  }
  process.exit(124);
}, 180_000).unref();
function command(program, params, timeout = 10_000, cleanup = false) {
  return new Promise((done, fail) => {
    const duration = limit(timeout, cleanup);
    const child = spawn(program, params, {windowsHide: true, stdio: ["ignore", "pipe", "pipe"]});
    owned.add(child);
    let output = "", settled = false;
    const finish = (error, value) => { if (settled) return; settled = true; clearTimeout(timer); error ? fail(error) : done(value); };
    const timer = setTimeout(() => { killTree(child); finish(new Error(`${basename(program)} exceeded its ${timeout}ms deadline.`)); }, duration);
    child.stdout.on("data", chunk => { output += chunk; });
    child.stderr.on("data", chunk => { output += chunk; });
    child.once("error", error => finish(error));
    child.once("close", code => { owned.delete(child); finish(code === 0 ? null : new Error(`${basename(program)} failed (${code}): ${output}`), output.trim()); });
  });
}
const powershell = (script, timeout = 10_000, cleanup = false) => command("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference='Stop'; " + script], timeout, cleanup);
async function stopApps(cleanup = false) {
  // Unique installation/browser paths prove ownership even if the NSIS process launched the app itself.
  await powershell(`Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq ${ps(executable)} -or ($_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -and $_.CommandLine.Contains(${ps(webview)})) } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`, 10_000, cleanup);
}
async function port() {
  return new Promise((done, fail) => {
    const server = createServer();
    const timer = setTimeout(() => { server.close(); fail(new Error("Could not reserve a loopback CDP port.")); }, limit(10_000));
    server.once("error", error => { clearTimeout(timer); fail(error); });
    server.listen(0, "127.0.0.1", () => { const value = server.address().port; server.close(() => { clearTimeout(timer); done(value); }); });
  });
}
async function connect(url, timeout = 10_000) {
  const address = new URL(url);
  assert(["127.0.0.1", "localhost"].includes(address.hostname) && address.protocol === "ws:", "CDP must be loopback.");
  const ws = new WebSocket(url), pending = new Map();
  sockets.add(ws);
  let next = 0;
  const rejectAll = error => { for (const request of pending.values()) { clearTimeout(request.timer); request.fail(error); } pending.clear(); };
  ws.addEventListener("close", () => { sockets.delete(ws); rejectAll(new Error("Release WebView closed its CDP connection.")); });
  ws.addEventListener("error", () => rejectAll(new Error("Release WebView CDP error.")));
  ws.addEventListener("message", event => {
    const message = JSON.parse(event.data), request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id); clearTimeout(request.timer);
    message.error ? request.fail(new Error(JSON.stringify(message.error))) : request.done(message.result);
  });
  await new Promise((done, fail) => {
    const timer = setTimeout(() => { ws.close(); fail(new Error("Release WebView CDP connection timed out.")); }, limit(timeout));
    ws.addEventListener("open", () => { clearTimeout(timer); done(); }, {once: true});
    ws.addEventListener("error", () => { clearTimeout(timer); fail(new Error("Cannot connect to release WebView CDP.")); }, {once: true});
  });
  const send = (method, params = {}, timeout = 10_000) => new Promise((done, fail) => {
    const id = ++next;
    const timer = setTimeout(() => { pending.delete(id); fail(new Error(`CDP ${method} exceeded 10 seconds.`)); }, limit(timeout));
    pending.set(id, {done, fail, timer});
    try { ws.send(JSON.stringify({id, method, params})); } catch (error) { clearTimeout(timer); pending.delete(id); fail(error); }
  });
  return {
    close: () => ws.close(),
    async evaluate(expression, timeout = 10_000) {
      const duration = Math.min(timeout, 10_000);
      const result = await send("Runtime.evaluate", {expression, awaitPromise: true, returnByValue: true, timeout: Math.max(1, Math.min(9_000, limit(duration)))}, duration);
      if (result.exceptionDetails) throw new Error("Release page evaluation failed: " + JSON.stringify(result.exceptionDetails));
      return result.result.value;
    },
    async screenshot(name) { const shot = await send("Page.captureScreenshot", {format: "png"}); await writeFile(join(evidence, name), Buffer.from(shot.data, "base64")); },
  };
}
async function launch(expectedText) {
  const cdpPort = await port();
  const env = {...process.env, WEBVIEW2_USER_DATA_FOLDER: webview,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${cdpPort} --no-first-run`};
  delete env.LOCAL_BUDGET_DATA_DIR; delete env.URBUDGET_API;
  const child = spawn(executable, [], {cwd: installed, env, windowsHide: true, stdio: ["ignore", "pipe", "pipe"]});
  owned.add(child);
  const appLog = [];
  child.stdout.on("data", data => appLog.push(data.toString())); child.stderr.on("data", data => appLog.push(data.toString()));
  child.once("close", () => { owned.delete(child); writeFileSync(join(evidence, `app-${cdpPort}.log`), appLog.join("")); });
  child.once("error", error => appLog.push(String(error)));
  const until = Date.now() + limit(30_000);
  while (Date.now() < until) {
    assert(child.exitCode === null && !child.killed, "Installed release exited before its UI became ready: " + appLog.join(""));
    let pages;
    try { pages = await (await fetch(`http://127.0.0.1:${cdpPort}/json/list`, {signal: AbortSignal.timeout(Math.min(1000, until - Date.now()))})).json(); } catch {}
    const page = pages?.find(item => item.type === "page" && item.url && !item.url.startsWith("about:") && !item.url.startsWith("devtools:"));
    if (page) {
      assert(!/1420|localhost:\d|127\.0\.0\.1:\d/.test(page.url), "Release unexpectedly loaded development web assets.");
      report.pages ??= []; report.pages.push({url: page.url, title: page.title});
      const cdp = await connect(page.webSocketDebuggerUrl, Math.max(1, Math.min(10_000, until - Date.now())));
      const view = await pageReady(cdp, expectedText, until);
      return {cdp, view};
    }
    await new Promise(done => setTimeout(done, Math.min(250, Math.max(1, until - Date.now()))));
  }
  throw new Error("Installed release did not expose a WebView CDP page within 30 seconds; no release smoke pass claimed.");
}
const invoke = (cdp, method, params = {}) => cdp.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(method)}, ${JSON.stringify(params)})`);
async function pageReady(cdp, expected, until) {
  while (Date.now() < until) {
    const view = await cdp.evaluate(`({text:document.body?.innerText ?? "",bridge:typeof window.__TAURI_INTERNALS__?.invoke === "function",scripts:[...document.scripts].map(s=>s.src).filter(Boolean)})`, Math.max(1, until - Date.now()));
    if (view.bridge && view.text.includes(expected)) return view;
    await new Promise(done => setTimeout(done, Math.min(200, Math.max(1, until - Date.now()))));
  }
  throw new Error("Installed release UI did not display: " + expected);
}
try {
  // Never overwrite a pre-existing OS profile, even if the runner guard was misconfigured.
  for (const base of [process.env.APPDATA, process.env.LOCALAPPDATA]) {
    assert(base, "Runner must expose its OS app-data paths.");
    assert(!existsSync(join(base, "com.localbudget.desktop")), "Runner profile already contains UrBudget data; refusing to touch it.");
  }
  report.installerSha256 = sha(await readFile(installer));
  firewallAttempted = true;
  for (const program of [installer, executable]) await powershell(`New-NetFirewallRule -DisplayName ${ps(rule)} -Direction Outbound -Action Block -Program ${ps(program)} -Profile Any | Out-Null`);
  await command(installer, ["/S", `/D=${installed}`], 90_000);
  assert(existsSync(executable), "Silent installer did not produce urbudget.exe at the requested destination.");
  await stopApps();
  report.executableSha256 = sha(await readFile(executable));
  report.fileVersion = await powershell(`(Get-Item -LiteralPath ${ps(executable)}).VersionInfo.ProductVersion`);
  assert(report.fileVersion === report.version || report.fileVersion === report.version + ".0", `Installed version ${report.fileVersion} differs from ${report.version}.`);
  report.checks.push("release installer/version/hash");
  const first = await launch("Let's set up your budget");
  let cdp = first.cdp;
  const firstView = first.view;
  assert(["Start fresh", "I'm coming from YNAB", "Look around first"].every(text => firstView.text.includes(text)), "First-run onboarding choices are missing.");
  assert(firstView.scripts.some(url => /\/assets\/.*\.js(?:\?|$)/.test(url)), "No bundled release JavaScript asset observed.");
  assert(!firstView.scripts.some(url => /@vite|\/src\//.test(url)), "Development script observed in installed release.");
  await writeFile(join(evidence, "first-run-dom.json"), JSON.stringify(firstView, null, 2));
  await cdp.screenshot("first-run.png");
  const initial = await invoke(cdp, "library_snapshot");
  assert(initial.firstRun === true && initial.entries.length === 0 && !initial.activeId, "Installed release did not open a pristine library.");
  assert((await invoke(cdp, "load_budget")).budget === null, "First run manufactured a budget.");
  report.checks.push("bundled first-run UI and empty native library");
  // Exact desktop lib.rs commands and domain.rs schema, all local; no account/sync commands.
  const budget = {schemaVersion: 2, name: "Release smoke local budget", currency: "SAR",
    accounts: [{id: "smoke-cash", name: "Release smoke cash", kind: "cash", openingBalance: 100000, archived: false}],
    categories: [], transactions: [], assignments: []};
  const id = await invoke(cdp, "create_budget", {budget, localOnly: true});
  const opened = await invoke(cdp, "library_open", {id});
  assert(opened.budget.name === budget.name && typeof opened.token === "string" && opened.token, "Created budget did not open.");
  await invoke(cdp, "apply_patch", {patch: {name: "Release smoke saved budget", accounts: {upsert: [{...budget.accounts[0], openingBalance: 99850}]}}});
  await invoke(cdp, "commit_budget", {expectedToken: opened.token});
  const saved = await invoke(cdp, "load_budget");
  assert(saved.budget.name === "Release smoke saved budget" && saved.budget.accounts[0].openingBalance === 99850, "Native save did not persist expected data.");
  const library = await invoke(cdp, "library_snapshot");
  assert(library.entries.length === 1 && library.entries[0].id === id && library.entries[0].scope === "local", "Smoke budget was not local-only.");
  report.checks.push("native local create/open/patch/commit");
  cdp.close(); await stopApps();
  const second = await launch("Release smoke cash"); cdp = second.cdp;
  const reopenedView = second.view;
  assert(reopenedView.text.includes("998.50"), "Restarted UI did not render the saved 998.50 account balance.");
  await writeFile(join(evidence, "restarted-dom.json"), JSON.stringify(reopenedView, null, 2));
  await cdp.screenshot("restarted.png");
  const reopened = await invoke(cdp, "load_budget"), restored = await invoke(cdp, "library_snapshot");
  assert(reopened.budget.name === saved.budget.name && reopened.budget.accounts[0].openingBalance === 99850 && restored.activeId === id && restored.firstRun === false,
    "Installed release failed save/restart persistence.");
  report.checks.push("restarted UI and native persistence");
  report.saved = {id, name: reopened.budget.name, openingBalance: reopened.budget.accounts[0].openingBalance, scope: restored.entries[0].scope};
  cdp.close(); report.status = "passed";
} catch (error) {
  failure = error; report.status = "failed"; report.error = error.stack ?? String(error);
} finally {
  for (const socket of sockets) socket.close();
  try { await stopApps(true); } catch (error) { report.cleanupError = String(error); failure ??= error; }
  for (const child of [...owned]) {
    if (child.exitCode === null && child.pid) {
      try { await command("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], 5_000, true); } catch { child.kill(); }
    }
  }
  if (firewallAttempted) {
    try { await powershell(`Get-NetFirewallRule -DisplayName ${ps(rule)} -ErrorAction SilentlyContinue | Remove-NetFirewallRule`, 10_000, true); }
    catch (error) { report.firewallCleanupError = String(error); failure ??= error; }
  }
  if (failure) report.status = "failed";
  report.finishedAt = new Date().toISOString(); report.elapsedMs = Date.now() - started;
  await writeFile(join(evidence, "result.json"), JSON.stringify(report, null, 2));
}
if (failure) throw failure;
console.log(`Installed Windows release ${report.version}: ${report.checks.length} checks passed in ${report.elapsedMs}ms. Evidence: ${evidence}`);
