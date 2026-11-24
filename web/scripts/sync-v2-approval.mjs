// Device approval and the recovery code, end to end: real Rust service processes with their
// own data folders and vaults, and a local workerd D1/R2. See
// docs/plans/2026-11-24-sync-device-approval.md.
// Run after: cargo build --manifest-path ../crates/Cargo.toml -p urbudget-services --example sync_qa
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { createInterface } from "node:readline";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { startLocal, web } from "./sync-v2-local.mjs";

const root = resolve(web, "../crates/target/sync-v2-approval", Date.now().toString());
await mkdir(root, { recursive: true });
const local = await startLocal({ port: Number(process.env.SYNC_V2_QA_PORT ?? 8791) });
const binary = resolve(web, `../crates/target/debug/examples/sync_qa${process.platform === "win32" ? ".exe" : ""}`);
const devices = [], passed = [];
const CODE = /^[0-9A-HJKMNP-TV-Z]{4}(-[0-9A-HJKMNP-TV-Z]{4}){5}$/;
// A `sync_qa` built from before this work (URBUDGET_OLD_QA=path): the same engine every
// released app has, to prove those apps keep working against this server and these accounts.
const released = process.env.URBUDGET_OLD_QA ? resolve(process.env.URBUDGET_OLD_QA) : null;

async function proxy() {
  // `legacy`: the server doesn't offer protocol 2. `dropCommit`: the next accepted commit's
  // answer never arrives. `requests` is what this device asked the server, and `bootstrap`
  // the server's last account answer to it.
  const state = { legacy: false, dropCommit: false, requests: [], bootstrap: null };
  const server = createServer(async (request, response) => {
    try {
      const chunks = []; for await (const chunk of request) chunks.push(chunk);
      const body = Buffer.concat(chunks); const headers = { ...request.headers }; delete headers.host; delete headers.connection;
      const upstream = await fetch(local.url + request.url, { method: request.method, headers, ...(body.length ? { body } : {}) });
      let bytes = Buffer.from(await upstream.arrayBuffer());
      if (request.url === "/api/v2/sync/bootstrap" && upstream.ok) state.bootstrap = JSON.parse(bytes);
      if (state.legacy && request.url === "/api/v2/sync/bootstrap" && upstream.ok) { const bootstrap = JSON.parse(bytes); bootstrap.activationEnabled = false; bytes = Buffer.from(JSON.stringify(bootstrap)); }
      state.requests.push({ method: request.method, path: request.url, status: upstream.status });
      if (state.dropCommit && /\/mutations\/\d+\/commit$/.test(request.url) && upstream.ok) { state.dropCommit = false; return request.socket.destroy(); }
      const out = Object.fromEntries(upstream.headers); delete out["transfer-encoding"]; delete out["content-encoding"]; out["content-length"] = bytes.length;
      response.writeHead(upstream.status, out); response.end(bytes);
    } catch (error) { response.writeHead(502); response.end(String(error)); }
  });
  await new Promise(r => server.listen(0, "127.0.0.1", r));
  state.url = `http://127.0.0.1:${server.address().port}`;
  state.stop = () => { server.closeAllConnections(); server.close(); };
  return state;
}
async function device(name, program = binary) {
  const fault = await proxy(), folder = resolve(root, name); await mkdir(folder, { recursive: true });
  const d = { name, fault, folder, pending: new Map(), child: null, stderr: "", next: 0 };
  d.start = () => {
    d.child = spawn(program, ["device", folder], { cwd: web, env: { ...process.env, URBUDGET_API: fault.url, LOCAL_BUDGET_DATA_DIR: folder, COMPUTERNAME: name }, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
    d.child.stderr.on("data", data => { d.stderr += data; });
    createInterface({ input: d.child.stdout }).on("line", line => {
      try { const reply = JSON.parse(line), pending = d.pending.get(reply.id); if (!pending) return; d.pending.delete(reply.id); clearTimeout(pending.timer); reply.ok ? pending.resolve(reply.value) : pending.reject(new Error(`${name}: ${pending.method}: ${reply.error}`)); } catch (error) { d.stderr += String(error); }
    });
  };
  d.call = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++d.next; const timer = setTimeout(() => { d.pending.delete(id); reject(new Error(`${name}: ${method} timed out: ${d.stderr}`)); }, 45000);
    d.pending.set(id, { resolve, reject, timer, method }); d.child.stdin.write(JSON.stringify({ id, method, params }) + "\n");
  });
  d.restart = async () => { const child = d.child; child.kill(); await new Promise(r => child.once("exit", r)); d.start(); };
  d.stop = () => { d.child.kill(); fault.stop(); for (const p of d.pending.values()) clearTimeout(p.timer); };
  d.start(); devices.push(d); return d;
}
let accounts = 0;
/** A new account with a sync plan, as `[id, email]`. */
async function account() {
  const id = `${local.accountId}_${++accounts}`, email = `approval-${accounts}-${local.email}`;
  await local.execute(`INSERT INTO users(id,email,created_at) VALUES ('${id}','${email}','2026-01-01'); INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:${id}','${id}','synthetic','active','2026-01-01','2026-01-01');`);
  return [id, email];
}
async function signedIn(name, email, program) {
  const d = await device(name, program); const start = await d.call("account_start", { email }); assert.ok(start.devCode);
  await d.call("account_verify", { email, code: start.devCode }); return d;
}
const sync = async d => (await d.call("cloud_status")).sync;
const wake = d => d.call("sync_now");
/** What the server holds for the device's account, as it just answered that device. */
const bootstrap = async d => { await d.call("sync_key_info"); return d.fault.bootstrap; };
const initial = name => ({ schemaVersion: 2, name, currency: "SAR", accounts: [{ id: "checking", name: "Checking", kind: "cash", openingBalance: 100000, archived: false }], categories: [{ id: "food", name: "Food", group: "Daily", target: 0 }], transactions: [], assignments: [] });
const names = async d => (await d.call("library_snapshot")).entries.filter(e => e.lifecycle === "active").map(e => e.name).sort();
/** Whether `d` has asked the server for anything since `mark`. */
const asked = (d, mark) => d.fault.requests.slice(mark).map(r => `${r.method} ${r.path}`);
const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/** Whether 24 code characters carry a tag that fits (crates/urbudget-services/src/cloud/codes.rs). */
function fits(code) {
  const bits = [...code.replaceAll("-", "")].reduce((all, c) => (all << 5n) | BigInt(ALPHABET.indexOf(c)), 0n);
  const bytes = Buffer.from(bits.toString(16).padStart(30, "0"), "hex"), tag = bytes[14];
  return (createHash("sha256").update("urbudget:code:v1").update(Buffer.from([tag & 1])).update(bytes.subarray(0, 14)).digest()[0] & 0xfe) === (tag & 0xfe);
}
/** The code with one character wrong, in a way its tag is known to catch (one in 128 isn't). */
function mistype(code) {
  for (const wrong of ALPHABET) { const typed = code.slice(0, 7) + wrong + code.slice(8); if (typed !== code && !fits(typed)) return typed; }
  throw new Error("no detectable typo");
}
async function test(name, run) { const start = Date.now(); await run(); passed.push(name); console.log(`PASS ${name} (${((Date.now() - start) / 1000).toFixed(1)}s)`); }

try {
  const [, email] = await account();
  const a = await signedIn("first", email), b = await signedIn("second", email), c = await signedIn("third", email);
  let recoveryCode;

  await test("turning sync on makes a recovery code and keeps it until it's saved", async () => {
    assert.deepEqual(await a.call("sync_key_info"), { exists: false, secret: null, recovery: false, protocol: 1 });
    const id = await a.call("create_budget", { budget: initial("Made before sync") }); await a.call("library_open", { id });
    const turned = await a.call("sync_setup");
    assert.equal(turned.created, true); assert.equal(turned.protocol, 2); assert.match(turned.recoveryCode, CODE);
    recoveryCode = turned.recoveryCode;
    assert.deepEqual(await a.call("sync_key_info"), { exists: true, secret: "code", recovery: false, protocol: 2 });
    let status = await sync(a);
    assert.equal(status.unlocked, true); assert.equal(status.secret, "code"); assert.equal(status.recoveryUnsaved, true);
    // Still there after the app closes, and asking for sync again hands the same code back.
    await a.restart();
    assert.deepEqual(await a.call("sync_recovery_unsaved"), { recoveryCode });
    assert.deepEqual(await a.call("sync_setup"), { created: false, protocol: 2, recoveryCode });
    await a.call("sync_recovery_saved");
    assert.equal(await a.call("sync_recovery_unsaved"), null);
    status = await sync(a); assert.equal(status.recoveryUnsaved, false); assert.equal(status.unlocked, true);
    await a.call("sync_enable", { enabled: true, ids: [id] }); await wake(a); await wake(a);
  });

  await test("a second device unlocks with a code the first one shows, typed, once", async () => {
    assert.deepEqual(await b.call("sync_key_info"), { exists: true, secret: "code", recovery: false, protocol: 2 });
    // It can't turn sync on again over the account's key.
    await assert.rejects(b.call("sync_setup"), /already set up/);
    const offered = await a.call("sync_pair_offer");
    assert.match(offered.code, CODE); assert.equal(offered.qr.modules.length, offered.qr.size ** 2); assert.ok(fits(offered.code));
    assert.deepEqual(await a.call("sync_pair_state", { id: offered.id }), { waiting: true, collected: false });
    // A typo is caught on the device: nothing is asked of the server.
    const mark = b.fault.requests.length;
    await assert.rejects(b.call("sync_unlock", { secret: mistype(offered.code) }), /typo/);
    assert.deepEqual(asked(b, mark).filter(r => r.includes("/pairings/")), []);
    assert.deepEqual(await a.call("sync_pair_state", { id: offered.id }), { waiting: true, collected: false });
    // Typed the way people type: lower case, no hyphens.
    assert.deepEqual(await b.call("sync_unlock", { secret: offered.code.replaceAll("-", "").toLowerCase() }), { created: false, protocol: 2 });
    // The first device's screen learns the code was collected, and closes.
    assert.deepEqual(await a.call("sync_pair_state", { id: offered.id }), { waiting: false, collected: true });
    const status = await sync(b); assert.equal(status.unlocked, true); assert.equal(status.recoveryUnsaved, false);
    await wake(b); await wake(b);
    assert.deepEqual(await names(b), ["Made before sync"]);
    // The server handed the sealed key out once.
    await assert.rejects(c.call("sync_unlock", { secret: offered.code }), /expired or was already used/);
    const withdrawn = await a.call("sync_pair_offer"); await a.call("sync_pair_cancel", { id: withdrawn.id });
    assert.deepEqual(await a.call("sync_pair_state", { id: withdrawn.id }), { waiting: false, collected: false });
    await assert.rejects(c.call("sync_unlock", { secret: withdrawn.code }), /expired or was already used/);
    assert.equal((await sync(c)).unlocked, false);
    // A device that doesn't sync has no key to hand over.
    await assert.rejects(c.call("sync_pair_offer"), /Unlock sync on this device/);
  });

  await test("a new device shows a code, and a device that syncs approves it by scanning", async () => {
    const request = await c.call("sync_pair_request"), scanned = "URBUDGET-ASK:1:" + request.code.replaceAll("-", "");
    assert.match(request.code, CODE); assert.equal(request.qr.modules.length, request.qr.size ** 2);
    // Nothing is on the server until someone approves.
    assert.deepEqual(await c.call("sync_pair_wait", { code: request.code }), { ready: false });
    // A scan on the wrong screen is told where it belongs, and hands nothing over.
    await assert.rejects(b.call("sync_unlock", { secret: scanned }), /waiting to be added/);
    const shown = await a.call("sync_pair_offer");
    await assert.rejects(b.call("sync_pair_approve", { code: "URBUDGET-PAIR:1:" + shown.code.replaceAll("-", "") }), /for the device being added/);
    await a.call("sync_pair_cancel", { id: shown.id });
    await assert.rejects(c.call("sync_pair_approve", { code: scanned }), /Unlock sync on this device/);
    await b.call("sync_pair_approve", { code: scanned });
    const ready = await c.call("sync_pair_wait", { code: request.code });
    assert.deepEqual(ready, { created: false, protocol: 2, ready: true });
    // Collected once: asking again finds nothing waiting.
    assert.deepEqual(await c.call("sync_pair_wait", { code: request.code }), { ready: false });
    await wake(c); await wake(c); assert.deepEqual(await names(c), ["Made before sync"]);
  });

  await test("the recovery code unlocks a device with no other at hand, on this app and on an older one", async () => {
    await c.call("sync_lock"); assert.equal((await sync(c)).unlocked, false);
    const mark = c.fault.requests.length;
    await assert.rejects(c.call("sync_unlock", { secret: mistype(recoveryCode) }), /typo/);
    assert.deepEqual(asked(c, mark), []);
    assert.deepEqual(await c.call("sync_unlock", { secret: ` ${recoveryCode.replaceAll("-", " ").toLowerCase()} ` }), { created: false, protocol: 2 });
    assert.equal((await sync(c)).unlocked, true);
    await c.call("sync_lock");
    // Where the key is made from a code there is no passphrase to try anything else as.
    await assert.rejects(c.call("sync_unlock", { secret: "correct horse battery staple" }), /isn't a code/);
    // Another account's recovery code is a real code that opens nothing here.
    const [, otherEmail] = await account(); const stranger = await signedIn("stranger", otherEmail);
    const theirs = (await stranger.call("sync_setup")).recoveryCode;
    await assert.rejects(c.call("sync_unlock", { secret: theirs }), /isn't the recovery code for this account/);
    // An app from before recovery codes only has a passphrase box: the code, typed as it is
    // shown, makes the same key there.
    assert.deepEqual(await c.call("sync_setup", { passphrase: recoveryCode }), { created: false, protocol: 2 });
    await wake(c); assert.deepEqual(await names(c), ["Made before sync"]);
  });

  await test("a lost recovery code is replaced without a reset, and nothing uploads again", async () => {
    const before = await bootstrap(a), mark = a.fault.requests.length;
    const replaced = await a.call("sync_recovery_create");
    assert.match(replaced.recoveryCode, CODE); assert.notEqual(replaced.recoveryCode, recoveryCode);
    const after = await bootstrap(a);
    assert.deepEqual({ ...after, recovery: null }, before, "the key, its generation and the catalog are untouched");
    assert.ok(after.recovery.wrapped);
    assert.deepEqual(asked(a, mark).filter(r => r.includes("/mutations/")), []);
    let status = await sync(a); assert.equal(status.recovery, true); assert.equal(status.recoveryUnsaved, true);
    assert.deepEqual(await a.call("sync_recovery_unsaved"), replaced);
    await a.call("sync_recovery_saved"); assert.equal((await sync(a)).recoveryUnsaved, false);
    // The new code opens the server's copy of the key.
    await c.call("sync_lock");
    assert.deepEqual(await c.call("sync_unlock", { secret: replaced.recoveryCode }), { created: false, protocol: 2 });
    // The code the key was made from still makes it: only a reset retires it.
    await c.call("sync_lock");
    assert.deepEqual(await c.call("sync_unlock", { secret: recoveryCode }), { created: false, protocol: 2 });
    status = await sync(c); assert.equal(status.unlocked, true); assert.equal(status.recoveryUnsaved, false);
  });

  await test("a reset makes a new code, and the old code, the old copy and a waiting approval stop working", async () => {
    const old = await a.call("sync_pair_offer");
    const reset = await b.call("sync_reset");
    assert.match(reset.recoveryCode, CODE); assert.notEqual(reset.recoveryCode, recoveryCode);
    const now = await bootstrap(b);
    assert.equal(now.keyGeneration, "1"); assert.equal(now.secret, "code"); assert.equal(now.recovery, null);
    assert.equal((await sync(b)).recoveryUnsaved, true); await b.call("sync_recovery_saved");
    await wake(a); await wake(c);
    for (const d of [a, c]) assert.equal((await sync(d)).unlocked, false, d.name);
    // The key that approval carried is retired, and so is everything that made or held it.
    await assert.rejects(c.call("sync_unlock", { secret: old.code }), /expired or was already used/);
    await assert.rejects(c.call("sync_unlock", { secret: recoveryCode }), /isn't the recovery code for this account/);
    assert.deepEqual(await c.call("sync_unlock", { secret: reset.recoveryCode }), { created: false, protocol: 2 });
    const offered = await b.call("sync_pair_offer");
    assert.deepEqual(await a.call("sync_unlock", { secret: offered.code }), { created: false, protocol: 2 });
    recoveryCode = reset.recoveryCode;
  });

  await test("a device that can't unlock resets, and its budgets are back in sync when the reset answers", async () => {
    for (const d of [a, b, c]) { await wake(d); await wake(d); }
    const rows = async d => (await d.call("library_snapshot")).entries;
    const before = (await rows(c)).filter(e => e.lifecycle === "active");
    assert.deepEqual(before.map(e => e.name), ["Made before sync"]);
    // The third device has the budget and nothing that unlocks sync: "Can't unlock?" resets.
    await c.call("sync_lock"); assert.equal((await sync(c)).unlocked, false);
    const reset = await c.call("sync_reset");
    assert.match(reset.recoveryCode, CODE); assert.equal((await sync(c)).unlocked, true);
    // Nothing more is asked of it: the budget is the same one, under a new identity, uploaded.
    const now = (await rows(c)).filter(e => e.lifecycle === "active");
    assert.deepEqual(now.map(e => [e.id, e.name, e.pending]), before.map(e => [e.id, e.name, false]));
    assert.ok(now[0].remoteId && now[0].remoteId !== before[0].remoteId && now[0].incarnation !== before[0].incarnation, JSON.stringify(now));
    assert.ok(!(await rows(c)).some(e => e.id === before[0].id && e.lifecycle === "recovery"));
    // A device that had it too is locked by the reset. With the new code it receives the budget
    // from the device that reset, and keeps its own copy to recover.
    const mine = (await rows(a)).find(e => e.lifecycle === "active");
    await wake(a); assert.equal((await sync(a)).unlocked, false);
    assert.deepEqual(await a.call("sync_unlock", { secret: reset.recoveryCode }), { created: false, protocol: 2 });
    await wake(a); await wake(a);
    const after = await rows(a), arrived = after.filter(e => e.lifecycle === "active");
    assert.deepEqual(arrived.map(e => [e.name, e.remoteId, e.missing]), [["Made before sync", now[0].remoteId, false]]);
    assert.equal(after.find(e => e.id === mine.id).lifecycle, "recovery");
    assert.ok((await a.call("library_recover", { id: mine.id })).recoveredId);
    await c.call("sync_recovery_saved"); recoveryCode = reset.recoveryCode;
  });

  await test("a setup whose answer is lost still ends with its recovery code shown", async () => {
    const [, lostEmail] = await account(); const d = await signedIn("interrupted", lostEmail), e = await signedIn("joins-later", lostEmail);
    d.fault.dropCommit = true;
    await assert.rejects(d.call("sync_setup"));
    // The server took the key. The app closes before it learns that.
    assert.equal((await bootstrap(e)).keyGeneration, "0");
    await d.restart();
    assert.equal((await sync(d)).recoveryUnsaved, false, "nothing is shown for a setup this device hasn't confirmed");
    const finished = await d.call("sync_setup");
    assert.match(finished.recoveryCode, CODE); assert.equal(finished.protocol, 2);
    assert.equal((await sync(d)).recoveryUnsaved, true);
    assert.deepEqual(await d.call("sync_recovery_unsaved"), { recoveryCode: finished.recoveryCode });
    // It is the code the key was made from: it unlocks a second device.
    assert.deepEqual(await e.call("sync_unlock", { secret: finished.recoveryCode }), { created: false, protocol: 2 });
    assert.equal((await bootstrap(e)).recovery, null, "no replacement was needed");
  });

  await test("an account with a passphrase keeps it, and gains approval and a recovery code", async () => {
    const [, oldEmail] = await account(); const d = await signedIn("has-passphrase", oldEmail), e = await signedIn("joins-passphrase", oldEmail);
    const passphrase = "correct horse battery staple";
    // Set up the way every released app does.
    assert.deepEqual(await d.call("sync_setup", { passphrase }), { created: true, protocol: 2 });
    const before = await bootstrap(d);
    assert.equal(before.secret, "passphrase");
    let status = await sync(d); assert.equal(status.unlocked, true); assert.equal(status.secret, "passphrase"); assert.equal(status.recoveryUnsaved, false);
    assert.deepEqual(await e.call("sync_key_info"), { exists: true, secret: "passphrase", recovery: false, protocol: 2 });
    // The passphrase goes in the same box as a code.
    await assert.rejects(e.call("sync_unlock", { secret: "not the passphrase" }), /isn't the sync passphrase/);
    assert.deepEqual(await e.call("sync_unlock", { secret: passphrase }), { created: false, protocol: 2 });
    await e.call("sync_lock");
    const offered = await d.call("sync_pair_offer");
    assert.deepEqual(await e.call("sync_unlock", { secret: "URBUDGET-PAIR:1:" + offered.code.replaceAll("-", "") }), { created: false, protocol: 2 });
    // Forgot the passphrase? A device that syncs makes a recovery code, and nothing else changes.
    const made = await d.call("sync_recovery_create"); await d.call("sync_recovery_saved");
    await e.call("sync_lock");
    assert.deepEqual(await e.call("sync_unlock", { secret: made.recoveryCode }), { created: false, protocol: 2 });
    await e.call("sync_lock");
    assert.deepEqual(await e.call("sync_setup", { passphrase }), { created: false, protocol: 2 });
    const after = await bootstrap(d);
    assert.deepEqual({ ...after, recovery: null }, before);
    assert.deepEqual(await e.call("sync_key_info"), { exists: true, secret: "passphrase", recovery: true, protocol: 2 });
  });

  await test("on protocol 1, approval and the passphrase both unlock, and a setup makes a code", async () => {
    const [, v1Email] = await account(); const d = await signedIn("v1-first", v1Email), e = await signedIn("v1-second", v1Email);
    for (const each of [d, e]) each.fault.legacy = true;
    const passphrase = "a passphrase from protocol 1";
    assert.deepEqual(await d.call("sync_setup", { passphrase }), { created: true });
    assert.deepEqual(await e.call("sync_key_info"), { exists: true, secret: "passphrase", recovery: false, protocol: 1 });
    const offered = await d.call("sync_pair_offer");
    assert.deepEqual(await e.call("sync_unlock", { secret: offered.code }), { created: false });
    let status = await sync(e); assert.equal(status.unlocked, true); assert.equal(status.protocol, 1);
    await e.call("sync_lock");
    assert.deepEqual(await e.call("sync_unlock", { secret: passphrase }), { created: false });
    // The recovery copy lives with protocol 2's account.
    await assert.rejects(d.call("sync_recovery_create"), /new sync first/);
    // Once the server offers protocol 2, a device given the key by approval moves the account
    // there, exactly as one that typed the passphrase would.
    await e.call("sync_lock"); e.fault.legacy = false; d.fault.legacy = false;
    const again = await d.call("sync_pair_offer");
    assert.deepEqual(await e.call("sync_unlock", { secret: again.code }), { created: false, protocol: 2 });
    assert.equal((await bootstrap(e)).protocol, 2);
    await wake(d); status = await sync(d); assert.equal(status.unlocked, true, "the first device keeps its key through the move"); assert.equal(status.protocol, 2);

    const [, freshEmail] = await account(); const f = await signedIn("v1-fresh", freshEmail), g = await signedIn("v1-fresh-second", freshEmail);
    for (const each of [f, g]) each.fault.legacy = true;
    const turned = await f.call("sync_setup");
    assert.equal(turned.created, true); assert.match(turned.recoveryCode, CODE);
    // Kept until saved here too, once this device has heard the account's key details back.
    await f.call("sync_key_info");
    assert.equal((await sync(f)).recoveryUnsaved, true); assert.deepEqual(await f.call("sync_recovery_unsaved"), { recoveryCode: turned.recoveryCode });
    await f.call("sync_recovery_saved"); assert.equal((await sync(f)).recoveryUnsaved, false);
    assert.deepEqual(await g.call("sync_unlock", { secret: turned.recoveryCode }), { created: false });
    assert.equal((await sync(g)).unlocked, true);
  });

  if (released) await test("an app released before this work joins, syncs and resets, and this one follows", async () => {
    const [, mixedEmail] = await account(); const current = await signedIn("current-app", mixedEmail), old = await signedIn("released-app", mixedEmail, released);
    const id = await current.call("create_budget", { budget: initial("Shared with an older app") }); await current.call("library_open", { id });
    const turned = await current.call("sync_setup"); await current.call("sync_recovery_saved");
    await current.call("sync_enable", { enabled: true, ids: [id] }); await wake(current); await wake(current);
    // The older app knows nothing of codes. Its passphrase box takes the recovery code as shown.
    assert.equal(await old.call("sync_has_key"), true);
    await assert.rejects(old.call("sync_setup", { passphrase: "not what unlocks this account" }), /isn't the sync passphrase/);
    assert.deepEqual(await old.call("sync_setup", { passphrase: turned.recoveryCode }), { created: false, protocol: 2 });
    await wake(old); await wake(old); assert.deepEqual(await names(old), ["Shared with an older app"]);
    // It goes on syncing while this app adds a recovery copy and an approval beside it.
    const replaced = await current.call("sync_recovery_create"); await current.call("sync_recovery_saved");
    const offered = await current.call("sync_pair_offer");
    const report = await wake(old); assert.equal(report.state, "ok", JSON.stringify(report));
    assert.equal((await old.call("cloud_status")).sync.unlocked, true);
    await current.call("sync_pair_cancel", { id: offered.id });
    // The older app resets with a passphrase of its choosing: the account becomes one with a
    // passphrase, the recovery copy goes with the old key, and this app asks for the passphrase.
    const passphrase = "chosen on the older app";
    await old.call("sync_reset", { passphrase });
    assert.deepEqual(await current.call("sync_key_info"), { exists: true, secret: "passphrase", recovery: false, protocol: 2 });
    await wake(current); assert.equal((await sync(current)).unlocked, false);
    for (const gone of [turned.recoveryCode, replaced.recoveryCode]) await assert.rejects(current.call("sync_unlock", { secret: gone }), /isn't the recovery code for this account/);
    assert.deepEqual(await current.call("sync_unlock", { secret: passphrase }), { created: false, protocol: 2 });
    // And this app can still hand that key on, and give the account a recovery code again.
    const third = await signedIn("current-second", mixedEmail), again = await current.call("sync_pair_offer");
    assert.deepEqual(await third.call("sync_unlock", { secret: again.code }), { created: false, protocol: 2 });
    assert.match((await current.call("sync_recovery_create")).recoveryCode, CODE);
    // The older app stays unlocked on the key it made. (Its engine keeps each of its budgets as a
    // copy to recover after its own reset: only an app with docs/plans/2026-11-24-sync-device-approval.md
    // section 12's change enrolls the resetting device's budgets again.)
    const after = await wake(old); assert.notEqual(after.state, "error", JSON.stringify(after)); assert.notEqual(after.state, "locked");
    assert.equal((await old.call("cloud_status")).sync.unlocked, true);
  });

  console.log(`\n${passed.length} scenarios passed.`);
} catch (error) {
  console.error(error); console.error(local.logs().slice(-4000));
  for (const d of devices) if (d.stderr) console.error(`${d.name} stderr: ${d.stderr.slice(-2000)}`);
  process.exitCode = 1;
} finally {
  for (const d of devices) d.stop();
  await local.stop();
}
