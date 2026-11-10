// Real Rust service processes, distinct local SQLite stores, and local workerd D1/R2.
// Run after: cargo build --manifest-path ../crates/Cargo.toml -p urbudget-services --example sync_qa
// Every sync_now below represents a native scheduler wake, never per-budget enrollment.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { createInterface } from "node:readline";
import { mkdir, writeFile, readFile, cp, rm } from "node:fs/promises";
import { resolve } from "node:path";
import { startLocal, web } from "./sync-v2-local.mjs";
const pause = ms => new Promise(r => setTimeout(r, ms));
const root = resolve(web, "../crates/target/sync-v2-instances", Date.now().toString());
await mkdir(root, { recursive: true });
if (process.env.URBUDGET_QA_URL) {
  const endpoint = new URL(process.env.URBUDGET_QA_URL);
  assert.ok(endpoint.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(endpoint.hostname) && !endpoint.username && !endpoint.password && endpoint.pathname === "/" && !endpoint.search && !endpoint.hash,
    "URBUDGET_QA_URL must be an HTTP loopback origin; this suite must never target production.");
  assert.match(process.env.URBUDGET_QA_EMAIL ?? "", /^[A-Za-z0-9._+-]+@example\.test$/, "Use a synthetic @example.test account.");
}
const local = process.env.URBUDGET_QA_URL ? { url: process.env.URBUDGET_QA_URL, email: process.env.URBUDGET_QA_EMAIL, stop() {}, logs() { return ""; } } : await startLocal({ port: Number(process.env.SYNC_V2_QA_PORT ?? 8789) });
const binary = resolve(web, "../crates/target/debug/examples/sync_qa.exe");
let passphrase = "correct horse battery staple";
function positiveArgument(name, fallback) {
  const index = process.argv.indexOf(name); if (index < 0) return fallback;
  const value = Number(process.argv[index + 1]);
  assert.ok(Number.isSafeInteger(value) && value > 0, name + " requires a positive integer");
  return value;
}
const rounds = positiveArgument("--rounds", 70);
const largeTransactions = positiveArgument("--large-transactions", 5000);
const devices = [], passed = [];
const traces = [];
async function proxy() {
  const state = { offline: false, dropCommit: false, dropped: 0, requests: [], hold: null, legacy: false, noV2: false, prepareError: null };
  const server = createServer(async (request, response) => {
    if (state.offline) return request.socket.destroy();
    try {
      const chunks = []; for await (const chunk of request) chunks.push(chunk);
      const body = Buffer.concat(chunks); const headers = { ...request.headers }; delete headers.host; delete headers.connection;
      if (state.hold && request.url.endsWith(`/${state.hold.kind}`)) { const held = state.hold; state.hold = null; held.arrived(); await held.wait; }
      if ((state.noV2 && request.url.startsWith("/api/v2/")) || (state.prepareError && request.url.endsWith("/prepare"))) {
        const error = state.noV2 ? "not_found" : state.prepareError;
        const status = state.noV2 ? 404 : 400;
        state.requests.push({method:request.method,path:request.url,status});
        response.writeHead(status,{"Content-Type":"application/json"}); response.end(JSON.stringify({error,message:"Injected local protocol failure"})); return;
      }
      const upstream = await fetch(local.url + request.url, { method: request.method, headers, ...(body.length ? { body } : {}) });
      let bytes = Buffer.from(await upstream.arrayBuffer());
      if (state.legacy && request.url === "/api/v2/sync/bootstrap" && upstream.ok) { const bootstrap = JSON.parse(bytes); bootstrap.activationEnabled = false; bytes = Buffer.from(JSON.stringify(bootstrap)); }
      state.requests.push({ method: request.method, path: request.url, status: upstream.status });
      if (state.dropCommit && /\/mutations\/\d+\/commit$/.test(request.url) && upstream.ok) { state.dropCommit = false; state.dropped++; return request.socket.destroy(); }
      const out = Object.fromEntries(upstream.headers); delete out["transfer-encoding"]; delete out["content-encoding"]; out["content-length"] = bytes.length;
      response.writeHead(upstream.status, out); response.end(bytes);
    } catch (error) { response.writeHead(502); response.end(String(error)); }
  });
  await new Promise(r => server.listen(0, "127.0.0.1", r));
  return { ...state, get offline() { return state.offline; }, set offline(v) { state.offline = v; }, get dropCommit() { return state.dropCommit; }, set dropCommit(v) { state.dropCommit = v; }, get dropped() { return state.dropped; },
    get legacy() { return state.legacy; }, set legacy(v) { state.legacy = v; },
    get noV2() { return state.noV2; }, set noV2(v) { state.noV2 = v; },
    get prepareError() { return state.prepareError; }, set prepareError(v) { state.prepareError = v; },
    hold(kind) { let arrived, release; const reached = new Promise(r => { arrived = r; }), wait = new Promise(r => { release = r; }); state.hold = { kind, arrived, wait }; return { reached, release }; },
    url: `http://127.0.0.1:${server.address().port}`, stop: () => { server.closeAllConnections(); server.close(); } };
}
async function device(name) {
  const fault = await proxy(), folder = resolve(root, name); await mkdir(folder, { recursive: true });
  const d = { name, fault, folder, pending: new Map(), child: null, stderr: "", next: 0 };
  d.start = () => {
    d.child = spawn(binary, ["device", folder], { cwd: web, env: { ...process.env, URBUDGET_API: fault.url, LOCAL_BUDGET_DATA_DIR: folder, COMPUTERNAME: name }, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
    d.child.stderr.on("data", data => { d.stderr += data; });
    createInterface({ input: d.child.stdout }).on("line", line => {
      try { const reply = JSON.parse(line), pending = d.pending.get(reply.id); if (!pending) return; d.pending.delete(reply.id); clearTimeout(pending.timer); reply.ok ? pending.resolve(reply.value) : pending.reject(new Error(`${name}: ${pending.method}: ${reply.error}`)); } catch (error) { d.stderr += `${error}\n${line}\n`; }
    });
  };
  d.call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++d.next; const timer = setTimeout(() => { d.pending.delete(id); reject(new Error(`${name}: ${method} timed out: ${d.stderr}`)); }, 45000);
    d.pending.set(id, { resolve, reject, timer, method }); d.child.stdin.write(JSON.stringify({ id, method, params }) + "\n");
  });
  d.halt = async () => { const child = d.child; child.kill(); await new Promise(r => child.once("exit", r)); };
  d.restart = async () => { await d.halt(); d.start(); };
  d.stop = () => { d.child.kill(); fault.stop(); for (const p of d.pending.values()) clearTimeout(p.timer); };
  d.start(); devices.push(d); return d;
}
async function signIn(d, email = local.email) { const start = await d.call("account_start", { email }); assert.ok(start.devCode); await d.call("account_verify", { email, code: start.devCode }); }
async function snapshot(d) { return d.call("library_snapshot"); }
async function wake(d) { const r = await d.call("sync_now"); traces.push({ device: d.name, report: r }); return r; }
async function settle(rounds = 4) { for (let i = 0; i < rounds; i++) for (const d of devices) if (!d.fault.offline) await wake(d); }
async function entry(d, remoteId) { return (await snapshot(d)).entries.find(e => e.remoteId === remoteId); }
async function open(d, remoteId) { const e = await entry(d, remoteId); assert.ok(e, `${d.name}: remote budget missing`); await d.call("library_open", { id: e.id }); return e; }
async function edit(d, change) { const state = await d.call("load_budget"); change(state.budget); await d.call("save_budget", { budget: state.budget, expectedToken: state.token, ackOnly: true }); }
async function equalBudgets(remoteId) { const values = []; for (const d of devices) { await open(d, remoteId); values.push((await d.call("load_budget")).budget); } for (const b of values.slice(1)) assert.deepEqual(b, values[0]); return values[0]; }
// `--only <words>` runs the scenarios whose name has them. Most build on the ones before, so it
// is for a scenario that stands alone, such as the last.
const only = process.argv.includes("--only") ? process.argv[process.argv.indexOf("--only") + 1] : null;
async function test(name, run) { if (only && !name.includes(only)) return; const start = Date.now(); await run(); passed.push(name); console.log(`PASS ${name} (${((Date.now() - start) / 1000).toFixed(1)}s)`); }
const initial = () => ({ schemaVersion: 2, name: "Three device budget", currency: "SAR", accounts: [{ id: "checking", name: "Checking", kind: "cash", openingBalance: 100000, archived: false }], categories: [{ id: "food", name: "Food", group: "Daily", target: 0 }], transactions: [], assignments: [] });
const transaction = (id, month = "10") => ({ id, date: `2026-${month}-10`, accountId: "checking", payee: id, amount: -100, status: "uncleared", memo: id, splits: [{ categoryId: "food", amount: -100 }], transferAccountId: null, transferStatus: null });
try {
  const [a, b, c] = await Promise.all([device("desktop"), device("phone"), device("third")]);
  for (const d of devices) { assert.equal((await d.call("load_budget")).budget, null); await signIn(d); }
  if (process.argv.includes("--legacy")) {
    await test("native legacy ciphertext and ancestry migrate without duplicate identities", async () => {
      for (const d of devices) {
        d.fault.legacy = true;
        const path=resolve(d.folder,"account.json"), account=JSON.parse(await readFile(path,"utf8")); delete account.accountId;
        await writeFile(path,JSON.stringify(account)); await d.restart();
        d.fault.noV2 = true; await d.call("sync_setup", { passphrase });
        assert.ok(!d.fault.requests.some(r => r.path.endsWith("/installations")));
        d.fault.noV2 = false; await d.call("sync_setup", { passphrase });
        assert.ok(JSON.parse(await readFile(path,"utf8")).accountId, "authenticated bootstrap backfills the1.0.10 session");
      }
      a.id = await a.call("create_budget", { budget: initial() }); await a.call("library_open", { id: a.id });
      await a.call("sync_track", { ids: [a.id] }); await wake(a);
      for (const d of [b, c]) { await d.call("sync_download", { id: a.id }); await d.call("library_open", { id: a.id }); }
      await edit(a,budget=>budget.transactions.push(transaction("legacy-no-open-budget")));
      const temporary=await a.call("create_budget",{budget:{...initial(),name:"Temporary selection"},localOnly:true});
      await a.call("library_open",{id:temporary}); await a.call("library_trash",{id:temporary});
      assert.equal((await a.call("load_budget")).budget,null); await wake(a); await wake(b);
      assert.ok((await b.call("load_budget")).budget.transactions.some(t=>t.id==="legacy-no-open-budget"));
      await a.call("library_open",{id:a.id});
      await settle(); const oldIds = new Map(devices.map(d => [d, a.id]));
      const linked = async d => { const list = await snapshot(d); const live=list.entries.filter(e=>e.lifecycle==="active"); assert.equal(live.length, 1, JSON.stringify(list)); assert.equal(live[0].id, oldIds.get(d)); assert.equal(live[0].remoteId, a.id); };
      a.fault.legacy = false; await a.call("sync_setup", { passphrase }); await wake(a); await linked(a);
      a.remote = a.id;
      await edit(a, budget => budget.transactions.push(transaction("after-legacy-migration"))); await wake(a);
      // B unlocks only now, after another device has rewritten the migrated head, holding work
      // it never synced: its copy links at the revision it last had and merges what it missed.
      await edit(b, budget => budget.transactions.push(transaction("late-device-unsynced-work")));
      // It was unlocked on protocol 1 and the move keeps the key, so nobody types it again.
      b.fault.legacy = false; b.fault.requests.length = 0; await wake(b); await wake(b); await linked(b);
      assert.notEqual((await b.call("cloud_status")).sync.unlocked, false);
      await wake(a);
      const both = (await a.call("load_budget")).budget;
      for (const id of ["after-legacy-migration", "late-device-unsynced-work"]) assert.equal(both.transactions.filter(t => t.id === id).length, 1, id);
      assert.deepEqual(both, (await b.call("load_budget")).budget);
      // Keep C's legacy ledger/base until the ID has been restored under a new incarnation.
      await edit(c, budget => budget.transactions.push(transaction("late-legacy-private-work")));
      await a.call("library_trash", { id: a.id }); await wake(a); await a.call("library_restore", { id: a.id }); await wake(a); await wake(b);
      c.fault.legacy = false; await c.call("sync_setup", { passphrase }); await wake(c); const told = await wake(c);
      const list = await snapshot(c), old = list.entries.find(e => e.id === oldIds.get(c)), current = list.entries.find(e => e.remoteId === a.remote);
      assert.equal(old.scope, "local"); assert.equal(old.accountId, null); assert.ok(current && current.id !== old.id, JSON.stringify(list));
      // A copy that can't be linked stays, and every pass says why on its row.
      assert.match(told.budgets.find(e => e.id === old.id)?.message ?? "", /Kept on this device only/, JSON.stringify(told));
      await c.call("library_open", { id: old.id }); assert.ok((await c.call("load_budget")).budget.transactions.some(t => t.id === "late-legacy-private-work"));
      await c.call("library_open", { id: current.id }); assert.ok(!(await c.call("load_budget")).budget.transactions.some(t => t.id === "late-legacy-private-work"));
    });
    await test("an account still on protocol 1 can reset its passphrase while activation is on", async () => {
      assert.ok(local.execute, "This scenario requires the isolated managed Worker");
      const otherId = local.accountId + "_v1", otherEmail = "v1-" + local.email;
      await local.execute(`INSERT INTO users(id,email,created_at) VALUES ('${otherId}','${otherEmail}','2026-01-01'); INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:${otherId}','${otherId}','synthetic','active','2026-01-01','2026-01-01');`);
      const d = await device("fourth"); await signIn(d, otherEmail);
      const forgotten = "a passphrase nobody remembers", chosen = "the passphrase chosen at reset";
      d.fault.legacy = true; await d.call("sync_setup", { passphrase: forgotten });
      const id = await d.call("create_budget", { budget: { ...initial(), name: "Reset on protocol 1" } }); await d.call("library_open", { id });
      await d.call("sync_track", { ids: [id] }); await wake(d);
      // A second device of this protocol 1 account takes the key from the first by code: the
      // same hand-over as on protocol 2, with no activation and no passphrase typed.
      const e = await device("fifth"); await signIn(e, otherEmail); e.fault.legacy = true;
      const offered = await d.call("sync_pair_offer"); assert.deepEqual(await e.call("sync_pair_accept", { code: offered.code }), { created: false });
      await e.call("sync_download", { id }); await e.call("library_open", { id });
      assert.equal((await e.call("load_budget")).budget.name, "Reset on protocol 1");
      // The server now offers protocol 2. Moving to it proves the old passphrase, which is
      // the one thing someone resetting doesn't have, so the reset stays on protocol 1.
      d.fault.legacy = false; await d.call("sync_reset", { passphrase: chosen }); await wake(d);
      await assert.rejects(d.call("sync_setup", { passphrase: forgotten }), /isn't the sync passphrase/);
      await d.call("sync_setup", { passphrase: chosen }); await wake(d); await wake(d);
      const live = (await snapshot(d)).entries.filter(e => e.lifecycle === "active");
      assert.equal(live.length, 1, JSON.stringify(live)); assert.equal(live[0].id, id); assert.equal(live[0].accountId, otherId); assert.equal(live[0].remoteId, id);
    });
  } else {
  await test("setup empty account and discover automatically enrolled budget", async () => {
    for (const d of devices) await d.call("sync_setup", { passphrase });
    a.id = await a.call("create_budget", { budget: initial() }); await a.call("library_open", { id: a.id }); await settle();
    const list = await snapshot(a); a.remote = list.entries.find(e => e.id === a.id)?.remoteId; assert.ok(a.remote, JSON.stringify(list));
    for (const d of devices) assert.equal((await snapshot(d)).entries.length, 1);
    await equalBudgets(a.remote);
  });
  await test("explicitly private budgets never enroll, including after restart", async () => {
    const id = await a.call("create_budget", { budget: { ...initial(), name: "Explicit private budget" }, localOnly: true });
    const checkPrivate = async () => {
      const item = (await snapshot(a)).entries.find(e => e.id === id);
      assert.equal(item.scope, "local"); assert.equal(item.accountId, null); assert.equal(item.remoteId, null);
      for (const peer of [b, c]) assert.ok(!(await snapshot(peer)).entries.some(e => e.name === "Explicit private budget"));
    };
    await checkPrivate(); await settle(); await checkPrivate();
    await a.restart(); await a.call("sync_setup", { passphrase }); await settle(); await checkPrivate();
    await a.call("library_trash", { id }); await open(a, a.remote);
  });
  await test("budgets created after sign-out stay private after signing in again", async () => {
    await a.call("account_sign_out");
    const id = await a.call("create_budget", { budget: { ...initial(), name: "Signed-out private budget" } });
    await signIn(a); await a.call("sync_setup", { passphrase }); await settle();
    const item = (await snapshot(a)).entries.find(e => e.id === id);
    assert.equal(item.scope, "local"); assert.equal(item.accountId, null); assert.equal(item.remoteId, null);
    for (const peer of [b, c]) assert.ok(!(await snapshot(peer)).entries.some(e => e.name === "Signed-out private budget"));
    await a.call("library_trash", { id }); await open(a, a.remote);
  });
  await test("switching accounts cannot upload another account's cached edits", async () => {
    assert.ok(local.execute, "This scenario requires the isolated managed Worker");
    const otherId = local.accountId + "_other", otherEmail = "other-" + local.email;
    await local.execute(`INSERT INTO users(id,email,created_at) VALUES ('${otherId}','${otherEmail}','2026-01-01'); INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:${otherId}','${otherId}','synthetic','active','2026-01-01','2026-01-01');`);
    await a.call("account_sign_out"); await signIn(a, otherEmail); await a.call("sync_setup", { passphrase });
    await open(a, a.remote);
    await edit(a, budget => budget.transactions.push(transaction("account-a-cached-edit")));
    await wake(a);
    const old = await entry(a, a.remote); assert.equal(old.accountId, local.accountId);
    assert.ok(!(await snapshot(a)).entries.some(e => e.accountId === otherId));
    await wake(b); await open(b, a.remote);
    assert.ok(!(await b.call("load_budget")).budget.transactions.some(t => t.id === "account-a-cached-edit"));
    const id = await a.call("create_budget", { budget: { ...initial(), name: "Second account budget" } });
    await wake(a); const owned = (await snapshot(a)).entries.find(e => e.id === id);
    assert.equal(owned.accountId, otherId); assert.ok(owned.remoteId);
    await a.call("account_sign_out"); await signIn(a); await a.call("sync_setup", { passphrase }); await settle();
    assert.ok((await equalBudgets(a.remote)).transactions.some(t => t.id === "account-a-cached-edit"));
    for (const peer of [b, c]) assert.ok(!(await snapshot(peer)).entries.some(e => e.remoteId === owned.remoteId));
    // Retain all later scenarios' initial transaction-count contract.
    await edit(a, budget => { budget.transactions = budget.transactions.filter(t => t.id !== "account-a-cached-edit"); });
    await settle(); await equalBudgets(a.remote);
  });
  await test("nonconflicting concurrent updates from different months", async () => {
    await Promise.all(devices.map((d, i) => edit(d, budget => budget.transactions.push(transaction(`first-${d.name}`, String(10 + i))))));
    await Promise.all(devices.map(wake)); await settle(); assert.equal((await equalBudgets(a.remote)).transactions.length, 3);
    await edit(a, budget => budget.transactions.push(transaction("remote-notification"))); await wake(a);
    const report = await wake(b);
    const bEntry = await entry(b, a.remote), loaded = await b.call("load_budget");
    assert.ok(report.changedBudgets?.some(row => row.id === bEntry.id && row.revision === loaded.token), JSON.stringify(report));
    assert.ok(report.libraryVersion); await settle(); await equalBudgets(a.remote);
  });
  await test("offline edit survives process restart and lost committed response", async () => {
    a.fault.offline = true; await edit(a, budget => budget.transactions.push(transaction("offline-survivor"))); await wake(a);
    await a.restart(); a.fault.offline = false; await a.call("sync_setup", { passphrase }); a.fault.dropCommit = true;
    await wake(a); assert.equal(a.fault.dropped, 1); await a.restart(); await a.call("sync_setup", { passphrase }); await settle();
    assert.equal((await equalBudgets(a.remote)).transactions.filter(t => t.id === "offline-survivor").length, 1);
  });
  await test("a data folder restored from a backup syncs under a new identity and keeps its edits", async () => {
    await open(a, a.remote); await settle();
    const backup = resolve(root, "desktop-backup");
    await a.halt(); await cp(a.folder, backup, { recursive: true }); a.start(); await a.call("sync_setup", { passphrase });
    // The device goes on syncing, so the server's sequence for its identity passes the backup's.
    await edit(a, budget => budget.transactions.push(transaction("after-the-backup"))); await settle();
    await a.halt(); await rm(a.folder, { recursive: true, force: true }); await cp(backup, a.folder, { recursive: true }); a.start();
    await a.call("sync_setup", { passphrase });
    const registrations = () => a.fault.requests.filter(r => r.method === "POST" && r.path.endsWith("/installations")).length, before = registrations();
    await edit(a, budget => budget.transactions.push(transaction("after-the-restore")));
    const report = await wake(a);
    assert.ok(a.fault.requests.some(r => r.path.endsWith("/prepare") && r.status === 409), "the restored sequence must have been refused");
    assert.equal(registrations(), before + 1, "the restored folder registers a new installation");
    assert.equal(report.state, "ok", JSON.stringify(report));
    await settle();
    const budget = await equalBudgets(a.remote);
    for (const id of ["after-the-backup", "after-the-restore"]) assert.equal(budget.transactions.filter(t => t.id === id).length, 1, id);
    // It keeps uploading under the new identity.
    await edit(a, budget => budget.transactions.push(transaction("second-after-the-restore"))); await settle();
    assert.ok((await equalBudgets(a.remote)).transactions.some(t => t.id === "second-after-the-restore"));
  });
  await test("a signed-in device unlocks with a code from one that syncs, and no passphrase", async () => {
    await c.call("sync_lock"); assert.equal((await wake(c)).state, "locked");
    const offered = await a.call("sync_pair_offer");
    assert.match(offered.code, /^[0-9A-HJKMNP-TV-Z]{4}(-[0-9A-HJKMNP-TV-Z]{4}){5}$/);
    assert.equal(offered.qr.modules.length, offered.qr.size ** 2);
    await assert.rejects(c.call("sync_pair_accept", { code: "0000-0000-0000-0000-0000-0000" }), /expired or was already used/);
    await assert.rejects(c.call("sync_pair_accept", { code: offered.code.slice(0, 20) }), /all 24 characters/);
    // Typed the way people type: lower case, no hyphens.
    await c.call("sync_pair_accept", { code: offered.code.replaceAll("-", "").toLowerCase() });
    assert.notEqual((await wake(c)).state, "locked");
    // The server handed the sealed key out once.
    await assert.rejects(b.call("sync_pair_accept", { code: offered.code }), /expired or was already used/);
    const withdrawn = await a.call("sync_pair_offer"); await a.call("sync_pair_cancel", { id: withdrawn.id });
    await assert.rejects(c.call("sync_pair_accept", { code: withdrawn.code }), /expired or was already used/);
    await open(c, a.remote); await edit(c, budget => budget.transactions.push(transaction("from-the-paired-device"))); await settle();
    assert.ok((await equalBudgets(a.remote)).transactions.some(t => t.id === "from-the-paired-device"));
    // A locked device has no key to hand over.
    await c.call("sync_lock"); await assert.rejects(c.call("sync_pair_offer"), /Unlock sync on this device/);
    await c.call("sync_setup", { passphrase }); await settle();
  });
  await test("a new device shows a code, and a device that syncs approves it by scanning", async () => {
    await c.call("sync_lock"); assert.equal((await wake(c)).state, "locked");
    // The new device makes the code and waits; nothing is on the server until someone approves.
    const asked = await c.call("sync_pair_request"), scanned = "URBUDGET-ASK:1:" + asked.code.replaceAll("-", "");
    assert.equal(asked.qr.modules.length, asked.qr.size ** 2);
    assert.deepEqual(await c.call("sync_pair_wait", { code: asked.code }), { ready: false });
    assert.equal((await wake(c)).state, "locked");
    // A scan on the wrong screen is told where it belongs, and hands nothing over.
    await assert.rejects(b.call("sync_pair_accept", { code: scanned }), /waiting to be added/);
    const shown = await a.call("sync_pair_offer");
    await assert.rejects(b.call("sync_pair_approve", { code: "URBUDGET-PAIR:1:" + shown.code.replaceAll("-", "") }), /for the device being added/);
    await a.call("sync_pair_cancel", { id: shown.id });
    // A locked device can't approve; one that syncs can.
    await b.call("sync_lock"); await assert.rejects(b.call("sync_pair_approve", { code: scanned }), /Unlock sync on this device/);
    await b.call("sync_setup", { passphrase });
    await a.call("sync_pair_approve", { code: scanned });
    const ready = await c.call("sync_pair_wait", { code: asked.code });
    assert.equal(ready.ready, true, JSON.stringify(ready));
    assert.notEqual((await wake(c)).state, "locked");
    // Collected once: asking again finds nothing waiting.
    assert.deepEqual(await c.call("sync_pair_wait", { code: asked.code }), { ready: false });
    await open(c, a.remote); await edit(c, budget => budget.transactions.push(transaction("from-the-approved-device"))); await settle();
    assert.ok((await equalBudgets(a.remote)).transactions.some(t => t.id === "from-the-approved-device"));
  });
  await test("eviction keeps discovery and restores download", async () => {
    const e = await entry(b, a.remote); await b.call("library_evict", { id: e.id }); assert.equal((await entry(b, a.remote)).missing, true);
    await open(b, a.remote); assert.equal((await entry(b, a.remote)).missing, false); await equalBudgets(a.remote);
  });
  await test("locked client repairs an accepted pending receipt without a content key", async () => {
    await edit(a, budget => budget.transactions.push(transaction("locked-receipt"))); a.fault.dropCommit = true; await wake(a);
    await a.call("sync_lock"); const report = await wake(a); assert.equal(report.state, "locked");
    assert.equal((await entry(a, a.remote)).pending, false);
    await a.call("sync_setup", { passphrase }); await settle(); assert.equal((await equalBudgets(a.remote)).transactions.filter(t => t.id === "locked-receipt").length, 1);
  });
  await test("save during a paused commit stays dirty and reaches every device", async () => {
    await edit(a, budget => budget.transactions.push(transaction("before-upload")));
    const held = a.fault.hold("commit"), running = wake(a); await held.reached;
    await edit(a, budget => budget.transactions.push(transaction("during-upload"))); held.release(); await running; await settle();
    const budget = await equalBudgets(a.remote); for (const id of ["before-upload", "during-upload"]) assert.equal(budget.transactions.filter(t => t.id === id).length, 1);
  });
  await test("blocked publication keeps reconciling other budgets across restart", async () => {
    await open(a,a.remote); a.fault.prepareError="sequence_gap";
    await edit(a,budget=>budget.transactions.push(transaction("blocked-delivery-edit")));
    const stopped=await wake(a); assert.equal(stopped.state,"attention");
    const other=await b.call("create_budget",{budget:{...initial(),name:"Download during blocked delivery"}}); await wake(b);
    const remote=(await snapshot(b)).entries.find(e=>e.id===other).remoteId;
    await a.restart(); await assert.rejects(a.call("sync_setup",{passphrase}),/sequence_gap/);
    const status=await a.call("cloud_status"); assert.equal(status.sync.last.state,"attention");
    const result=await wake(a); assert.equal(result.state,"attention");
    const downloaded=await entry(a,remote); assert.ok(downloaded && !downloaded.missing,JSON.stringify(result));
    await a.call("library_open",{id:downloaded.id}); assert.equal((await a.call("load_budget")).budget.name,"Download during blocked delivery");
    a.fault.prepareError=null; await a.call("sync_setup",{passphrase}); await settle();
    assert.equal((await equalBudgets(a.remote)).transactions.filter(t=>t.id==="blocked-delivery-edit").length,1);
    await open(a,a.remote);
    await a.call("library_evict",{id:downloaded.id});
    const selected=(await snapshot(a)).activeId;
    await a.call("sync_download",{id:downloaded.id});
    assert.equal((await snapshot(a)).activeId,selected,"download must not switch the persisted selection");
    await a.restart(); assert.equal((await snapshot(a)).activeId,selected); await a.call("sync_setup",{passphrase});
    await b.call("library_trash",{id:other}); await settle();
  });
  await test("invalid immutable manifests can cancel without losing saved edits", async () => {
    await open(a,a.remote); a.fault.prepareError="invalid_manifest";
    await edit(a,budget=>budget.transactions.push(transaction("cancel-invalid-candidate")));
    await wake(a);
    assert.ok(a.fault.requests.some(r=>r.path.endsWith("/cancel") && r.status===200));
    a.fault.prepareError=null; await settle();
    assert.equal((await equalBudgets(a.remote)).transactions.filter(t=>t.id==="cancel-invalid-candidate").length,1);
  });
  await test("one unreadable local budget cannot stop other account downloads", async () => {
    const bad=await a.call("create_budget",{budget:{...initial(),name:"Temporarily unreadable cache"}}); await settle();
    await a.call("library_open",{id:bad}); const path=(await a.call("load_budget")).databasePath;
    await open(a,a.remote); const original=await readFile(path);
    const good=await b.call("create_budget",{budget:{...initial(),name:"Incoming despite damaged cache"}}); await wake(b);
    const goodRemote=(await snapshot(b)).entries.find(e=>e.id===good).remoteId;
    try {
      await writeFile(path,"deliberately corrupted disposable QA cache");
      const report=await wake(a); assert.equal(report.state,"attention");
      const incoming=await entry(a,goodRemote); assert.ok(incoming && !incoming.missing,JSON.stringify(report));
      assert.ok(report.budgets.some(e=>e.id===bad && e.state==="error"));
    } finally { await writeFile(path,original); }
    await settle(); await a.call("library_trash",{id:bad}); await b.call("library_trash",{id:good}); await settle();
  });
  await test("raw editor fields block incoming changes and survive restart", async () => {
    const state = await b.call("load_budget"), id = (await entry(b, a.remote)).id, epoch = state.token;
    const values = { amount: "-", payee: "partially typed", date: "2026-", note: "draft survives process death" };
    await b.call("editor_checkpoint", { budgetId: id, id: "qa-raw-editor", epoch, target: { kind: "transaction", id: "unsaved" }, baseline: {}, values });
    await edit(a, budget => budget.transactions.push(transaction("incoming-while-editor"))); await wake(a);
    const report = await wake(b); assert.ok(report.budgets.some(e => e.id === id && e.state === "waiting"), JSON.stringify(report));
    // This copy has nothing of its own to upload, but it is behind: its status can't say synced.
    assert.equal((await b.call("cloud_status")).sync.last.budgets.find(e => e.id === id)?.state, "waiting");
    await b.restart(); await b.call("sync_setup", { passphrase });
    // The process that had the form open is gone. Its draft stays to recover, and no longer
    // holds this budget's sync in both directions until someone finds and discards it.
    const after = await wake(b); assert.ok(!after.budgets.some(e => e.id === id && e.state === "waiting"), JSON.stringify(after));
    assert.ok((await b.call("load_budget")).budget.transactions.some(t => t.id === "incoming-while-editor"));
    const recovered = await b.call("editor_recovery", { budgetId: id }); assert.deepEqual(recovered.find(e => e.editorId === "qa-raw-editor")?.draft, values);
    await b.call("editor_discard", { budgetId: id, id: "qa-raw-editor", epoch }); await settle(); await equalBudgets(a.remote);
  });
  await test("same-month assignments ask and stale conflict choices are rejected", async () => {
    await edit(a, budget => { budget.assignments = [{ month: "2026-10", categoryId: "food", amount: 1000 }]; }); await settle();
    await edit(a, budget => { budget.assignments[0].amount = 2000; }); await edit(b, budget => { budget.assignments[0].amount = 3000; });
    await wake(a); const conflict = await wake(b), id = (await entry(b, a.remote)).id; assert.ok(conflict.budgets.some(e => e.id === id && e.state === "conflict"), JSON.stringify(conflict));
    await wake(c); await edit(c, budget => budget.transactions.push(transaction("third-device-during-review"))); await wake(c);
    const displayed = conflict.budgets.find(e => e.id === id).review;
    await wake(b); // The background pass replaces its persisted review while the old dialog stays open.
    await assert.rejects(b.call("sync_resolve", { id, keep: "local", review: displayed }), /changed since.*reviewed/);
    const refreshed = await wake(b); await b.call("sync_resolve", { id, keep: "local", review: refreshed.budgets.find(e => e.id === id).review }); await settle();
    const budget = await equalBudgets(a.remote); assert.equal(budget.assignments[0].amount, 3000); assert.ok(budget.transactions.some(t => t.id === "third-device-during-review"));
  });
  await test("whole-budget fallback retains the discarded remote version as a readable backup", async () => {
    await edit(a,budget=>{
      budget.accounts.push({id:"qa-card",name:"Review card",kind:"credit",openingBalance:0,archived:false});
      budget.categories.push({id:"qa-payment",name:"Card payment",group:"Cards",target:0,paymentAccountId:"qa-card"});
    }); await settle();
    await edit(a,budget=>{budget.categories.find(c=>c.id==="qa-payment").id="qa-payment-a";budget.transactions.push(transaction("remote-only-before-whole-choice"));});
    await edit(b,budget=>{budget.categories.find(c=>c.id==="qa-payment").id="qa-payment-b";});
    await wake(a); const report=await wake(b), id=(await entry(b,a.remote)).id;
    const choice=report.budgets.find(e=>e.id===id);
    assert.ok(choice.conflicts.some(c=>c.key==="budget/validation"),JSON.stringify(report));
    await b.call("sync_resolve",{id,keep:"local",review:choice.review}); await settle();
    const merged=await equalBudgets(a.remote);
    assert.ok(merged.categories.some(c=>c.id==="qa-payment-b"));
    assert.ok(!merged.transactions.some(t=>t.id==="remote-only-before-whole-choice"));
    const files=await b.call("list_backups"), copies=await Promise.all(files.filter(f=>f.name.endsWith(".json")).map(async f=>JSON.parse(await readFile(f.path,"utf8"))));
    assert.ok(copies.some(b=>b.categories.some(c=>c.id==="qa-payment-a") && b.transactions.some(t=>t.id==="remote-only-before-whole-choice")),"discarded remote budget must remain restorable");
  });
  await test("rate-limited publication stays durable across restart and resumes next window", async () => {
    await edit(a, budget => budget.transactions.push(transaction("throttle-survivor")));
    const key = "sync-v2:prepare:" + local.accountId;
    const window = new Date(Math.floor(Date.now() / 3600000) * 3600000).toISOString();
    await local.execute(`INSERT INTO rate_limits(key,count,window_start) VALUES ('${key}',1200,'${window}') ON CONFLICT(key) DO UPDATE SET count=1200,window_start='${window}';`);
    const report = await wake(a); assert.match(JSON.stringify(report), /rate_limited/);
    assert.equal((await entry(a, a.remote)).pending, true);
    await a.restart(); await assert.rejects(a.call("sync_setup", { passphrase }), /rate_limited/);
    assert.ok((await a.call("load_budget")).budget.transactions.some(t => t.id === "throttle-survivor"));
    // Simulate the next hourly rate window only in the disposable local database.
    await local.execute(`UPDATE rate_limits SET window_start='1970-01-01T00:00:00.000Z' WHERE key='${key}'`);
    await a.call("sync_setup", { passphrase }); await settle(); assert.equal((await equalBudgets(a.remote)).transactions.filter(t => t.id === "throttle-survivor").length, 1);
  });
  await test(`${rounds * 3} deterministic independent device edits converge without loss`, async () => {
    let state = 0x17015; const expected = new Set();
    for (let i = 0; i < rounds * 3; i++) { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; const d = devices[state % devices.length], id = `stress-${i}-${state}`; expected.add(id); await edit(d, budget => budget.transactions.push(transaction(id, String(10 + devices.indexOf(d))))); if (i % 7 === 0) await Promise.all(devices.map(wake)); }
    await settle(6); const actual = new Set((await equalBudgets(a.remote)).transactions.map(t => t.id)); for (const id of expected) assert.ok(actual.has(id), `lost ${id}`);
  });
  await test("remote trash preserves dirty local work and restore fences old incarnation", async () => {
    const old = await entry(a, a.remote), priorC = await entry(c, a.remote); c.fault.offline = true; await edit(c, budget => budget.transactions.push(transaction("recover-me")));
    const bEntry = await entry(b, a.remote);
    await a.call("library_trash", { id: old.id }); await wake(a); const deleted = await wake(b); c.fault.offline = false; await wake(c);
    assert.ok(deleted.changedBudgets?.some(row => row.id === bEntry.id), JSON.stringify(deleted));
    assert.ok(deleted.libraryVersion);
    assert.equal((await entry(c, a.remote)).lifecycle, "trashed");
    const recovered = await c.call("library_recover", { id: (await entry(c, a.remote)).id }); assert.ok(recovered.recoveredId);
    await c.call("library_open", { id: recovered.recoveredId }); assert.ok((await c.call("load_budget")).budget.transactions.some(t => t.id === "recover-me"));
    await a.call("library_restore", { id: old.id }); await settle(); assert.notEqual((await entry(a, a.remote)).incarnation, old.incarnation);
    assert.ok(!(await equalBudgets(a.remote)).transactions.some(t => t.id === "recover-me"));
    assert.equal((await snapshot(c)).entries.find(e => e.id === priorC.id).lifecycle, "recovery");
  });
  await test("late old-incarnation upload cannot overwrite a restored budget", async () => {
    const old = await entry(a, a.remote); await edit(c, budget => budget.transactions.push(transaction("late-old-incarnation")));
    const held = c.fault.hold("blob"), running = wake(c); await held.reached;
    await a.call("library_trash", { id: old.id }); await wake(a); await a.call("library_restore", { id: old.id }); await wake(a);
    held.release(); await running; await settle();
    assert.notEqual((await entry(a, a.remote)).incarnation, old.incarnation);
    assert.ok(!(await equalBudgets(a.remote)).transactions.some(t => t.id === "late-old-incarnation"));
  });
  await test("offline trash then undo keeps the same incarnation and saved edits", async () => {
    const before = await entry(a, a.remote); a.fault.offline = true;
    await edit(a, budget => budget.transactions.push(transaction("offline-delete-undo")));
    await a.call("library_trash", { id: before.id }); await a.call("library_restore", { id: before.id });
    a.fault.offline = false; await settle();
    assert.equal((await entry(a, a.remote)).incarnation, before.incarnation);
    assert.ok((await equalBudgets(a.remote)).transactions.some(t => t.id === "offline-delete-undo"));
  });
  await test("a restore the server holds back waits for its own receipt when the person asks again", async () => {
    const before = await entry(a, a.remote);
    await a.call("library_trash", { id: before.id }); await settle();
    // The server refuses the restore for now, as a publication pause does: its request keeps
    // the installation's delivery slot, and its outcome is unknown.
    a.fault.prepareError = "publication_paused";
    await a.call("library_restore", { id: before.id });
    assert.equal((await wake(a)).state, "attention");
    // The row shows an error, so the person asks again. This pass must not finish the first
    // request as superseded: when its real receipt arrives, the two would disagree for good.
    await a.call("library_restore", { id: before.id }); await wake(a);
    a.fault.prepareError = null; await settle();
    assert.equal((await entry(a, a.remote)).lifecycle, "active");
    const report = await wake(a); assert.equal(report.state, "ok", JSON.stringify(report));
    assert.ok(!(await a.call("cloud_status")).sync.last.budgets.some(e => e.state === "error"));
    // The installation still uploads.
    await open(a, a.remote); await edit(a, budget => budget.transactions.push(transaction("after-a-held-restore"))); await settle();
    assert.ok((await equalBudgets(a.remote)).transactions.some(t => t.id === "after-a-held-restore"));
  });
  await test("locked clients publish trash while content restoration waits for unlock", async () => {
    const before = await entry(a, a.remote);
    await a.call("sync_lock"); await a.call("library_trash", { id: before.id });
    const report = await wake(a); assert.equal(report.state, "locked");
    await wake(b); assert.equal((await entry(b, a.remote)).lifecycle, "trashed");
    await a.call("library_restore", { id: before.id }); await wake(a); await wake(b);
    assert.equal((await entry(b, a.remote)).lifecycle, "trashed", "restore must not bypass the content key");
    await a.restart(); await a.call("sync_setup", { passphrase }); await settle();
    assert.notEqual((await entry(a, a.remote)).incarnation, before.incarnation); await equalBudgets(a.remote);
  });
  await test("concurrent queued restores settle after another device restores first", async () => {
    await a.call("library_trash", { id: (await entry(a, a.remote)).id }); await settle();
    await a.call("library_restore", { id: (await entry(a, a.remote)).id });
    await b.call("library_restore", { id: (await entry(b, a.remote)).id });
    await wake(a); await settle();
    assert.equal((await entry(b, a.remote)).lifecycle, "active"); await equalBudgets(a.remote);
  });
  await test("offline deletion requires review when another device has saved a newer head", async () => {
    const id = (await entry(a, a.remote)).id;
    a.fault.offline = true; await a.call("library_trash", { id });
    await edit(b, budget => budget.transactions.push(transaction("newer-before-delete-dispatch"))); await wake(b);
    a.fault.offline = false; await wake(a);
    assert.equal((await entry(a, a.remote)).status, "deletion_conflict");
    await a.restart(); await a.call("sync_setup", { passphrase }); await settle();
    assert.equal((await entry(a, a.remote)).status, "deletion_conflict", "restart must not retry unseen deletion");
    assert.equal((await entry(b, a.remote)).lifecycle, "active");
    await a.call("library_review_deletion", { id });
    assert.ok((await a.call("load_budget")).budget.transactions.some(t => t.id === "newer-before-delete-dispatch"));
    await a.call("library_trash", { id }); await wake(a); await wake(b);
    assert.equal((await entry(b, a.remote)).lifecycle, "trashed");
    await a.call("library_restore", { id }); await settle(); await equalBudgets(a.remote);
  });
  await test("a deletion commit race preserves newer work until explicit review", async () => {
    const id = (await entry(a, a.remote)).id;
    await a.call("library_trash", { id });
    const held = a.fault.hold("commit"), running = wake(a); await held.reached;
    await edit(b, budget => budget.transactions.push(transaction("remote-edit-during-delete"))); await wake(b);
    held.release(); await running; await settle();
    assert.equal((await entry(a, a.remote)).status, "deletion_conflict");
    assert.equal((await entry(b, a.remote)).lifecycle, "active");
    await a.call("library_review_deletion", { id });
    assert.ok((await a.call("load_budget")).budget.transactions.some(t => t.id === "remote-edit-during-delete"));
    await a.call("library_trash", { id }); await wake(a); await settle();
    for (const d of devices) assert.equal((await entry(d, a.remote)).lifecycle, "trashed");
    await a.call("library_restore", { id }); await settle();
    assert.ok((await equalBudgets(a.remote)).transactions.some(t => t.id === "remote-edit-during-delete"));
  });
  await test(`large encrypted snapshot with ${largeTransactions} transactions converges`, async () => {
    const before = (await a.call("load_budget")).budget.transactions.length;
    await edit(a, budget => { for (let i = 0; i < largeTransactions; i++) budget.transactions.push(transaction(`large-${i}`)); }); await settle();
    assert.equal((await equalBudgets(a.remote)).transactions.length, before + largeTransactions);
  });
  await test("expired subscription retains reads and deletion but blocks fresh publication", async () => {
    assert.ok(local.execute, "This scenario requires the isolated managed Worker");
    await local.execute(`UPDATE subscriptions SET status='canceled' WHERE user_id='${local.accountId}'`);
    await b.call("library_evict", { id: (await entry(b, a.remote)).id }); await open(b, a.remote);
    await edit(a, budget => budget.transactions.push(transaction("expired-local-only")));
    const report = await wake(a); assert.ok(report.budgets.some(e => e.state === "subscription_required"), JSON.stringify(report));
    const id = (await entry(a, a.remote)).id; await a.call("library_trash", { id }); await wake(a); await wake(b); assert.equal((await entry(b, a.remote)).lifecycle, "trashed");
    await local.execute(`UPDATE subscriptions SET status='active' WHERE user_id='${local.accountId}'`);
    await a.call("library_restore", { id }); await settle(); await equalBudgets(a.remote);
  });
  await test("key reset invalidates an old prepared upload without republishing old identity", async () => {
    const oldRemote = a.remote, oldC = await entry(c, oldRemote); await edit(c, budget => budget.transactions.push(transaction("old-key-pending")));
    const held = c.fault.hold("blob"), running = wake(c); await held.reached;
    passphrase = "replacement correct horse battery staple"; await a.call("sync_reset", { passphrase }); held.release(); await running;
    for (const d of devices) await d.call("sync_setup", { passphrase }); await settle();
    assert.equal((await snapshot(c)).entries.find(e => e.id === oldC.id).lifecycle, "recovery");
    const saved = await c.call("library_recover", { id: oldC.id }); await c.call("library_open", { id: saved.recoveredId });
    assert.ok((await c.call("load_budget")).budget.transactions.some(t => t.id === "old-key-pending"));
    for (const d of devices) assert.ok(!(await snapshot(d)).entries.some(e => e.remoteId === oldRemote && e.lifecycle === "active"));
    a.id = await a.call("create_budget", { budget: initial() }); await a.call("library_open", { id: a.id }); await settle();
    a.remote = (await snapshot(a)).entries.find(e => e.id === a.id).remoteId; assert.notEqual(a.remote, oldRemote); await equalBudgets(a.remote);
  });
  await test("retrying a key reset after its lost response preserves the new generation and peer data", async () => {
    passphrase = "third correct horse battery staple"; a.fault.dropCommit = true;
    await assert.rejects(a.call("sync_reset", { passphrase }), /OFFLINE/);
    await b.call("sync_setup", { passphrase });
    const peerId = await b.call("create_budget", { budget: { ...initial(), name: "Peer data after reset" } });
    await b.call("library_open", { id: peerId }); await wake(b);
    const peerRemote = (await snapshot(b)).entries.find(e => e.id === peerId).remoteId;
    const bootstrap = async () => (await fetch(`${local.url}/api/v2/sync/bootstrap`, { headers: { Authorization: `Bearer ${local.token}`, "X-Sync-Client": "2" } })).json();
    const before = await bootstrap(); await a.call("sync_reset", { passphrase }); const after = await bootstrap();
    assert.equal(after.keyGeneration, before.keyGeneration, "retry repeated the already committed key reset");
    await c.call("sync_setup", { passphrase }); await settle(); a.remote = peerRemote;
    assert.equal((await equalBudgets(a.remote)).name, "Peer data after reset");
  });
  await test("deleting the final active budget stays empty after process restart", async () => {
    await a.call("library_trash", { id: (await entry(a, a.remote)).id }); await wake(a); assert.equal((await a.call("load_budget")).budget, null);
    await a.restart(); assert.equal((await a.call("load_budget")).budget, null); assert.equal((await snapshot(a)).activeId, null);
  });
  // Last: its devices are another account's, and the scenarios above compare every device.
  await test("sync set up with nothing typed: a key made on the first device, its recovery code, and a reset", async () => {
    assert.ok(local.execute, "This scenario requires the isolated managed Worker");
    const id = local.accountId + "_code", email = "code-" + local.email, token = "code-" + local.token;
    await local.execute(`INSERT INTO users(id,email,created_at) VALUES ('${id}','${email}','2026-01-01');
      INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:${id}','${id}','synthetic','active','2026-01-01','2026-01-01');
      INSERT INTO sessions(id,user_id,kind,created_at,last_seen_at,expires_at) VALUES ('${createHash("sha256").update(token).digest("hex")}','${id}','desktop','2026-01-01','2099-01-01','2099-12-31');`);
    const server = async () => (await fetch(`${local.url}/api/v2/sync/bootstrap`, { headers: { Authorization: `Bearer ${token}`, "X-Sync-Client": "2" } })).json();
    const shape = /^([0-9A-HJKMNP-TV-Z]{4}-){5}[0-9A-HJKMNP-TV-Z]{4}$/;
    const first = await device("code-first"), second = await device("code-second"), third = await device("code-third"), fourth = await device("code-fourth");
    for (const d of [first, second, third, fourth]) await signIn(d, email);
    assert.deepEqual(await first.call("sync_key_info"), { exists: false, passphrase: false, recovery: false, setup: "code", reset: "passphrase" });
    // The answer to the first try is lost after the server took it. The code made for that try
    // was never shown, so the retry ends with one that works.
    first.fault.dropCommit = true;
    await assert.rejects(first.call("sync_setup"), /OFFLINE/);
    const lost = (await server()).recovery;
    const made = await first.call("sync_setup");
    assert.match(made.recoveryCode, shape, JSON.stringify(made));
    let account = await server();
    assert.equal(account.protocol, 2); assert.equal(account.keyGeneration, "0"); assert.equal(account.salt, null);
    assert.ok(account.recovery && account.recovery.wrapped !== lost.wrapped, "the retry replaced the copy sealed for the lost code");
    assert.deepEqual(await second.call("sync_key_info"), { exists: true, passphrase: false, recovery: true, setup: "code", reset: "code" });
    let status = (await first.call("cloud_status")).sync;
    assert.equal(status.unlocked, true); assert.equal(status.passphrase, false); assert.equal(status.recovery, true);
    const budget = await first.call("create_budget", { budget: { ...initial(), name: "Nothing typed" } });
    await first.call("library_open", { id: budget }); await wake(first); await wake(first);
    const remote = (await snapshot(first)).entries.find(e => e.id === budget).remoteId; assert.ok(remote);
    const has = async d => { await wake(d); await wake(d); return (await snapshot(d)).entries.some(e => e.remoteId === remote && e.lifecycle === "active"); };
    // Nothing typed opens this account, and a wrong or short code leaves the device locked.
    await assert.rejects(second.call("sync_setup", { passphrase: "any passphrase at all" }), /has no passphrase/);
    await assert.rejects(second.call("sync_setup"), /already set up/);
    await assert.rejects(second.call("sync_recover", { code: "0000-0000-0000-0000-0000-0000" }), /isn't the recovery code/);
    await assert.rejects(second.call("sync_recover", { code: made.recoveryCode.slice(0, 20) }), /all 24 characters/);
    assert.equal((await wake(second)).state, "locked");
    // The recovery code, on a device with no other device at hand.
    assert.deepEqual(await second.call("sync_recover", { code: made.recoveryCode.replaceAll("-", "").toLowerCase() }), { created: false, protocol: 2 });
    assert.ok(await has(second));
    // A third device takes the key from the second by code, as on any account. Its screen has
    // one box for a code, whichever kind it is, so the engine works out which.
    await assert.rejects(third.call("sync_unlock_code", { code: "0000-0000-0000-0000-0000-0000" }), /didn't work/);
    assert.equal((await wake(third)).state, "locked");
    const offered = await second.call("sync_pair_offer");
    assert.deepEqual(await third.call("sync_unlock_code", { code: offered.code }), { created: false, protocol: 2 });
    assert.ok(await has(third));
    // A new recovery code: the old one stops working, and nothing else changes.
    const before = await server(), heads = (await snapshot(first)).entries.map(e => [e.remoteId, e.incarnation]);
    const replaced = await third.call("sync_recovery_create");
    assert.match(replaced.recoveryCode, shape); assert.notEqual(replaced.recoveryCode, made.recoveryCode);
    assert.deepEqual(await server(), { ...before, recovery: (await server()).recovery });
    assert.notEqual((await server()).recovery.wrapped, before.recovery.wrapped);
    await assert.rejects(fourth.call("sync_unlock_code", { code: made.recoveryCode }), /didn't work/);
    assert.deepEqual(await fourth.call("sync_unlock_code", { code: replaced.recoveryCode.toLowerCase() }), { created: false, protocol: 2 });
    for (const d of [first, second, third, fourth]) assert.ok(await has(d), d.name);
    assert.deepEqual((await snapshot(first)).entries.map(e => [e.remoteId, e.incarnation]), heads, "no budget was uploaded again");
    // A device that only locked takes none of this back: it can't make a code.
    await fourth.call("sync_lock");
    await assert.rejects(fourth.call("sync_recovery_create"), /Unlock sync on this device/);
    await assert.rejects(fourth.call("sync_pair_offer"), /Unlock sync on this device/);
    // A reset with nothing typed: a new key with its own code. The other devices hold the old
    // key, so they are locked until one of the three ways gives them the new one.
    const reset = await first.call("sync_reset");
    assert.match(reset.recoveryCode, shape); assert.notEqual(reset.recoveryCode, replaced.recoveryCode);
    account = await server();
    assert.equal(account.keyGeneration, "1"); assert.equal(account.salt, null); assert.ok(account.recovery);
    // As after any reset, each device keeps its copy of a budget sealed with the old key, for
    // the person to bring back. A budget made now is sealed with the new one.
    await wake(first);
    assert.equal((await snapshot(first)).entries.find(e => e.id === budget).lifecycle, "recovery");
    const later = await first.call("create_budget", { budget: { ...initial(), name: "After the reset" } });
    await first.call("library_open", { id: later }); await wake(first); await wake(first);
    assert.equal((await wake(second)).state, "locked");
    await assert.rejects(second.call("sync_recover", { code: replaced.recoveryCode }), /isn't the recovery code/);
    await second.call("sync_recover", { code: reset.recoveryCode });
    status = (await second.call("cloud_status")).sync;
    assert.equal(status.unlocked, true); assert.equal(status.passphrase, false); assert.equal(status.recovery, true);
    await wake(second); await wake(second);
    const arrived = (await snapshot(second)).entries.filter(e => e.lifecycle === "active" && e.remoteId);
    assert.equal(arrived.length, 1, JSON.stringify(await snapshot(second))); assert.equal(arrived[0].name, "After the reset");
  });
  }
  console.log(JSON.stringify({ passed, root, requests: devices.map(d => ({ device: d.name, count: d.fault.requests.length })) }, null, 2));
} catch (error) {
  console.error(error.stack); console.error("Last native reports:", JSON.stringify(traces.slice(-6), null, 2));
  console.error("Worker logs:", local.logs().slice(-12000)); process.exitCode = 1;
} finally {
  await writeFile(resolve(root, "trace.json"), JSON.stringify({ passed, traces, devices: devices.map(d => ({ name: d.name, stderr: d.stderr, requests: d.fault.requests })) }, null, 2));
  for (const d of devices) d.stop(); await local.stop();
}
