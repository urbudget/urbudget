import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { app } from "../src/index";
import { testEnv, type TestEnv } from "./fakes";
import { hex, sha256 } from "../src/util";
import { canonical, manifestDigest, type Manifest } from "../src/sync-v2/protocol";
import { maintainSyncV2, reclaimObjects } from "../src/sync-v2/objects";
import { cleanupSyncObjects } from "../src/sync";

let env: TestEnv;
const accountId = "usr_v2", installationId = "device_a";
const auth = { Authorization: "Bearer v2-test-session" };
const json = (body: unknown, extra: Record<string, string> = {}) => ({ ...auth, "Content-Type": "application/json", ...extra });
const headers = (device = installationId) => ({ ...auth, "X-Sync-Installation": device });
const call = (path: string, init: RequestInit = {}) => app.request(`/api/v2/sync${path}`, { ...init, headers: { ...auth, ...init.headers } }, env);
const post = (path: string, body: unknown, device = installationId) => call(path, { method: "POST", headers: json(body, { "X-Sync-Installation": device }), body: JSON.stringify(body) });
const boot = async () => (await call("/bootstrap")).json();
async function scope() { const b = await boot(); return { "X-Sync-Generation": b.keyGeneration, "X-Sync-Epoch": b.catalogEpoch }; }
async function registered(device = installationId) { return post("/installations", { installationId: device }, device); }
async function setup(device = installationId) {
  await registered(device);
  const m: Manifest = { accountId, installationId: device, operationId: "setup", kind: "keySetup", keyGeneration: null, catalogEpoch: null, expectedProtocol: 1, key: { salt: "abcdefghijklmnop", check: "synthetic-check" } };
  return perform("1", m);
}
async function content(sequence: string, options: Partial<Manifest> = {}, bytes = "synthetic encrypted snapshot"): Promise<{ m: Manifest; bytes: Uint8Array }> {
  const b = await boot(), kind = options.kind ?? "create";
  const m: Manifest = { accountId, installationId, operationId: `operation_${sequence}`, kind, keyGeneration: b.keyGeneration, catalogEpoch: b.catalogEpoch,
    budgetId: "budget_a", expectedVersion: "0", newIncarnation: `incarnation_${sequence}`, ...options };
  const data = new TextEncoder().encode(bytes);
  m.payload = { format: "v2", sha256: hex(await crypto.subtle.digest("SHA-256", data)), length: data.length, meta: "encrypted-meta",
    context: { format: "v2", accountId, keyGeneration: m.keyGeneration, budgetId: m.budgetId, incarnation: m.newIncarnation ?? m.incarnation, operationId: m.operationId, parentVersion: m.expectedVersion } };
  return { m, bytes: data };
}
async function perform(sequence: string, m: Manifest, bytes?: Uint8Array) {
  const prepared = await post(`/mutations/${sequence}/prepare`, m, m.installationId);
  const p = await prepared.json();
  expect(prepared.status, JSON.stringify(p)).toBe(200);
  if (p.state === "terminal") return p;
  if (bytes) {
    const uploaded = await call(`/mutations/${sequence}/blob`, { method: "PUT", headers: { ...headers(m.installationId), "X-Sync-Digest": p.digest }, body: bytes });
    expect(uploaded.status, await uploaded.clone().text()).toBe(200);
  }
  const committed = await post(`/mutations/${sequence}/commit`, { digest: p.digest }, m.installationId);
  const result = await committed.json();
  expect(committed.status, JSON.stringify(result)).toBe(200);
  return result;
}
beforeEach(async () => {
  env = await testEnv(); env.SYNC_V2_ENABLED = "1";
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("External network is forbidden in sync v2 tests"); }));
  await env.DB.prepare("INSERT INTO users(id,email,created_at) VALUES (?1,'sync-v2@example.test','2026-01-01')").bind(accountId).run();
  await env.DB.prepare("INSERT INTO sessions(id,user_id,kind,created_at,last_seen_at,expires_at) VALUES (?1,?2,'desktop','2026-01-01','2099-01-01','2099-12-31')").bind(await sha256("v2-test-session"), accountId).run();
  await env.DB.prepare("INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:sync-v2',?1,'synthetic','active','2026-01-01','2026-01-01')").bind(accountId).run();
});
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); env.DB.sqlite.close(); });

describe("sync v2 publication", () => {
  it("sets up an empty account and exposes no fabricated budget", async () => {
    expect((await boot()).keyGeneration).toBeNull();
    const result = await setup();
    expect(result.receipt).toMatchObject({ outcome: "accepted", protocol: 2, keyGeneration: "0", kind: "keySetup" });
    const catalog = await call("/catalog/snapshot", { headers: await scope() });
    expect(await catalog.json()).toMatchObject({ budgets: [], highwater: "1" });
  });
  it("publishes, downloads and replays a receipt without another event", async () => {
    await setup(); const { m, bytes } = await content("2");
    const first = await perform("2", m, bytes);
    expect(first.receipt).toMatchObject({ outcome: "accepted", budget: { version: "1", incarnation: "incarnation_2" } });
    const again = await perform("2", m, bytes);
    expect(again).toEqual(first);
    const download = await call("/budgets/budget_a/content?version=1&incarnation=incarnation_2", { headers: await scope() });
    expect(download.status).toBe(200); expect(await download.text()).toBe(new TextDecoder().decode(bytes));
    expect(JSON.parse(download.headers.get("X-Sync-Payload")!)).toEqual(m.payload);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_events_v2").first("n")).toBe(2);
  });
  it("retains the original acknowledgement after another device advances the head", async () => {
    await setup(); const a = await content("2"); const first = await perform("2", a.m, a.bytes);
    await registered("device_b");
    const b = await content("1", { installationId: "device_b", kind: "update", incarnation: "incarnation_2", newIncarnation: undefined, expectedVersion: "1" }, "later encrypted snapshot");
    expect((await perform("1", b.m, b.bytes)).receipt.budget.version).toBe("2");
    expect(await perform("2", a.m, a.bytes)).toEqual(first);
  });
  it("stores a terminal CAS rejection once and does not publish a false event", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    const stale = await content("3", { kind: "update", incarnation: "incarnation_2", newIncarnation: undefined, expectedVersion: "0" });
    const result = await perform("3", stale.m, stale.bytes);
    expect(result.receipt).toMatchObject({ outcome: "rejected", code: "conflict", sequence: "3" });
    expect(await perform("3", stale.m, stale.bytes)).toEqual(result);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_events_v2").first("n")).toBe(2);
  });
  it("rejects different content at the same sequence and gaps", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    const different = { ...a.m, operationId: "different" };
    different.payload = { ...a.m.payload!, context: { ...a.m.payload!.context, operationId: "different" } };
    expect((await post("/mutations/2/prepare", different)).status).toBe(409);
    expect((await post("/mutations/5/prepare", a.m)).status).toBe(409);
  });
  it("cancels a reserved or never-prepared manifest and ignores a late blob", async () => {
    await setup(); const a = await content("2"), d = await manifestDigest(a.m);
    const cancel = await post("/mutations/2/cancel", { digest: d, manifest: a.m });
    expect((await cancel.json()).receipt.outcome).toBe("cancelled");
    const late = await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": d }, body: a.bytes });
    expect((await late.json()).receipt.outcome).toBe("cancelled");
    expect(env.SYNC.objects.size).toBe(0);
  });
  it("rejects bytes inconsistent with the immutable manifest", async () => {
    await setup(); const a = await content("2"); const p = await (await post("/mutations/2/prepare", a.m)).json();
    const bad = a.bytes.slice(); bad[0] ^= 1;
    const response = await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: bad });
    expect(response.status).toBe(400); expect((await response.json()).error).toBe("checksum_mismatch");
    expect(env.SYNC.objects.size).toBe(0);
  });
  it("allows deletion and receipt recovery after subscription ends", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    await env.DB.prepare("UPDATE subscriptions SET status='canceled'").run();
    expect((await perform("2", a.m)).receipt.outcome).toBe("accepted");
    const trash: Manifest = { ...a.m, operationId: "trash", kind: "trash", incarnation: "incarnation_2", expectedVersion: "1" };
    delete trash.payload; delete trash.newIncarnation;
    expect((await perform("3", trash)).receipt.budget.state).toBe("trashed");
    const b = await content("4", { budgetId: "budget_b" });
    expect((await perform("4", b.m)).receipt.code).toBe("subscription_required");
  });
  it("restores under a new incarnation and refuses stale writers or ID recreation", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    const trash: Manifest = { ...a.m, operationId: "trash", kind: "trash", incarnation: "incarnation_2", expectedVersion: "1" }; delete trash.payload; delete trash.newIncarnation;
    await perform("3", trash);
    const restore = await content("4", { kind: "restore", incarnation: "incarnation_2", expectedVersion: "2", source: { incarnation: "incarnation_2", version: "2", sha256: a.m.payload!.sha256 } }, "resealed restored bytes");
    expect((await perform("4", restore.m, restore.bytes)).receipt.budget).toMatchObject({ incarnation: "incarnation_4", version: "3", state: "active" });
    const stale = await content("5", { kind: "update", newIncarnation: undefined, incarnation: "incarnation_2", expectedVersion: "3" });
    expect((await perform("5", stale.m)).receipt.code).toBe("conflict");
    const recreate = await content("6"); expect((await perform("6", recreate.m)).receipt.code).toBe("identity_exists");
  });
  it("reports missing referenced bytes as storage failure rather than deletion", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes); env.SYNC.objects.clear();
    const response = await call("/budgets/budget_a/content?version=1&incarnation=incarnation_2", { headers: await scope() });
    expect(response.status).toBe(503); expect((await response.json()).error).toBe("storage_unavailable");
  });
});

describe("sync v2 account controls and retention", () => {
  it("is dormant by default without affecting existing v1 accounts", async () => {
    delete env.SYNC_V2_ENABLED;
    const registration = await registered();
    expect(registration.status).toBe(409);
    expect((await registration.json()).error).toBe("activation_disabled");
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_accounts_v2").first("n")).toBe(0);
    expect((await boot()).protocol).toBe(1);
  });
  it("compatible pause retains pending work and allows replay, cancellation, reads and deletion", async () => {
    await setup(); const a = await content("2"); const accepted = await perform("2", a.m, a.bytes);
    const b = await content("3", { kind: "update", incarnation: "incarnation_2", newIncarnation: undefined, expectedVersion: "1" });
    const p = await (await post("/mutations/3/prepare", b.m)).json();
    await call("/mutations/3/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: b.bytes });
    env.SYNC_V2_PUBLICATION_PAUSED = "1"; delete env.SYNC_V2_ENABLED;
    expect((await post("/mutations/3/commit", { digest: p.digest })).status).toBe(503);
    expect(await (await post("/mutations/2/prepare", a.m)).json()).toEqual(accepted);
    const pending = await (await call("/installations/device_a/receipt")).json(); expect(pending.pending.sequence).toBe("3");
    expect((await call("/catalog/snapshot", { headers: await scope() })).status).toBe(200);
    expect((await (await post("/mutations/3/cancel", { digest: p.digest })).json()).receipt.outcome).toBe("cancelled");
    const trash: Manifest = { ...a.m, operationId: "paused_trash", kind: "trash", incarnation: "incarnation_2", expectedVersion: "1" }; delete trash.payload; delete trash.newIncarnation;
    expect((await perform("4", trash)).receipt.outcome).toBe("accepted");
    const unsent = await content("5", { budgetId: "other_budget" });
    expect((await post("/mutations/5/prepare", unsent.m)).status).toBe(503);
    expect((await (await post("/mutations/5/cancel", { digest: await manifestDigest(unsent.m), manifest: unsent.m })).json()).receipt.outcome).toBe("cancelled");
    delete env.SYNC_V2_PUBLICATION_PAUSED;
    const resumed = await content("6", { budgetId: "resumed_budget" });
    expect((await perform("6", resumed.m, resumed.bytes)).receipt.outcome).toBe("accepted");
  });
  it("resets atomically, invalidates pending old-generation slots and preserves receipts", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    await registered("device_b"); const b = await content("1", { installationId: "device_b", budgetId: "budget_b" });
    await post("/mutations/1/prepare", b.m, "device_b");
    const current = await boot();
    const reset: Manifest = { accountId, installationId, operationId: "reset", kind: "keyReset", keyGeneration: current.keyGeneration, catalogEpoch: current.catalogEpoch, expectedProtocol: 2, key: { salt: "qrstuvwxyzabcdef", check: "new-synthetic-check" } };
    const result = await perform("3", reset);
    expect(result.receipt).toMatchObject({ outcome: "accepted", keyGeneration: "1" });
    expect(await perform("3", reset)).toEqual(result);
    const other = await (await call("/installations/device_b/receipt")).json();
    expect(other).toMatchObject({ lastSequence: "1", receipt: { code: "invalidated_generation" }, pending: null });
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_heads_v2").first("n")).toBe(0);
    await cleanupSyncObjects(env); expect(env.SYNC.objects.size).toBe(0);
  });
  it("migrates ciphertext without a key rotation and fences original v1 SQL", async () => {
    await env.DB.prepare("INSERT INTO sync_keys(user_id,salt,key_check,created_at,generation) VALUES (?1,'abcdefghijklmnop','check','2026-01-01',7)").bind(accountId).run();
    await env.SYNC.put("legacy-ciphertext", "legacy bytes");
    await env.DB.prepare("INSERT INTO sync_budgets(user_id,budget_id,revision,object_key,size,updated_at) VALUES (?1,'legacy_budget',42,'legacy-ciphertext',12,'2026-01-01')").bind(accountId).run();
    await registered(); const b = await boot();
    const m: Manifest = { accountId, installationId, operationId: "migrate", kind: "migrateProtocol", keyGeneration: "7", catalogEpoch: b.catalogEpoch, expectedProtocol: 1 };
    const result = await perform("1", m); expect(result.receipt).toMatchObject({ outcome: "accepted", keyGeneration: "7", protocol: 2 });
    await cleanupSyncObjects(env); expect(env.SYNC.objects.has("legacy-ciphertext")).toBe(true);
    // An old Worker may have committed this upload before activation but receive
    // its ambiguous D1 error afterwards. Its original cleanup query must not hand
    // it the adopted key, even though the v1 row has been retired.
    const oldCleanup = await env.DB.prepare(`INSERT OR IGNORE INTO sync_object_garbage(object_key)
      SELECT ?1 WHERE NOT EXISTS(SELECT 1 FROM sync_budgets WHERE object_key=?1) RETURNING object_key`).bind("legacy-ciphertext").first();
    expect(oldCleanup).toBeNull();
    expect(await env.DB.prepare("SELECT version FROM sync_heads_v2 WHERE budget_id='legacy_budget'").first("version")).toBe("42");
    await expect(env.DB.prepare("UPDATE sync_keys SET generation=generation+1 WHERE user_id=?1").bind(accountId).run()).rejects.toThrow("sync_upgrade_required");
    await expect(env.DB.prepare("INSERT INTO sync_budgets(user_id,budget_id,revision,object_key,size,updated_at) VALUES (?1,'resurrect',1,'bad',1,'2026-01-01')").bind(accountId).run()).rejects.toThrow("sync_upgrade_required");
    expect((await app.request("/api/v1/sync/key", { headers: auth }, env)).status).toBe(426);
  });
  it("expires staging with a terminal receipt and retains sequence fences", async () => {
    await setup(); const a = await content("2"); await post("/mutations/2/prepare", a.m);
    await env.DB.prepare("UPDATE sync_staging_v2 SET expires_at='2000-01-01'").run();
    await maintainSyncV2(env);
    const receipt = await (await call("/installations/device_a/receipt")).json();
    expect(receipt).toMatchObject({ lastSequence: "2", receipt: { outcome: "expired" }, pending: null });
  });
  it("purges expired trash bytes but keeps identity and emits a deletion fence", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    const trash: Manifest = { ...a.m, operationId: "trash", kind: "trash", incarnation: "incarnation_2", expectedVersion: "1" }; delete trash.payload; delete trash.newIncarnation;
    await perform("3", trash);
    await env.DB.prepare("UPDATE sync_heads_v2 SET retained_until='2000-01-01'").run();
    await maintainSyncV2(env); await cleanupSyncObjects(env);
    expect(await env.DB.prepare("SELECT state,version,object_key FROM sync_heads_v2").first()).toEqual({ state: "purged", version: "3", object_key: null });
    expect(env.SYNC.objects.size).toBe(0);
    const recreate = await content("4"); expect((await perform("4", recreate.m)).receipt.code).toBe("identity_exists");
  });
  it("never reclaims an adopted reference, including migrated shared objects", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    const key = await env.DB.prepare("SELECT object_key FROM sync_heads_v2").first<string>("object_key");
    await reclaimObjects(env, [key!]); expect(env.SYNC.objects.has(key!)).toBe(true);
  });
  it("uses recursively sorted canonical manifests", () => {
    expect(canonical({ z: { b: 2, a: 1 }, a: "v" })).toBe('{"a":"v","z":{"a":1,"b":2}}');
  });
});

describe("sync v2 adversarial ordering and faults", () => {
  it("decides racing prepared updates against the commit-time head", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes); await registered("device_b");
    const edits = await Promise.all([
      content("3", { kind: "update", incarnation: "incarnation_2", newIncarnation: undefined, expectedVersion: "1" }, "A's edit"),
      content("1", { installationId: "device_b", kind: "update", incarnation: "incarnation_2", newIncarnation: undefined, expectedVersion: "1" }, "B's edit"),
    ]);
    const sequences = ["3", "1"], digests: string[] = [];
    for (let n = 0; n < edits.length; n++) {
      const { m, bytes } = edits[n];
      const p = await (await post(`/mutations/${sequences[n]}/prepare`, m, m.installationId)).json(); digests.push(p.digest);
      expect((await call(`/mutations/${sequences[n]}/blob`, { method: "PUT", headers: { ...headers(m.installationId), "X-Sync-Digest": p.digest }, body: bytes })).status).toBe(200);
    }
    const results = await Promise.all(edits.map(async ({ m }, n) => (await post(`/mutations/${sequences[n]}/commit`, { digest: digests[n] }, m.installationId)).json()));
    expect(results.map(r => r.receipt.outcome).sort()).toEqual(["accepted", "rejected"]);
    expect(await env.DB.prepare("SELECT version FROM sync_heads_v2").first("version")).toBe("2");
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_events_v2").first("n")).toBe(3);
  });
  it("rolls back a failure after head mutation and safely retries the same sequence", async () => {
    await setup(); const a = await content("2"); const p = await (await post("/mutations/2/prepare", a.m)).json();
    await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: a.bytes });
    const batch = env.DB.batch.bind(env.DB);
    env.DB.batch = (async (commands: D1PreparedStatement[]) => batch([...commands.slice(0, 4), env.DB.prepare("INSERT INTO missing_table VALUES (1)"), ...commands.slice(4)])) as typeof env.DB.batch;
    expect((await post("/mutations/2/commit", { digest: p.digest })).status).toBe(500);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_heads_v2").first("n")).toBe(0);
    expect(await env.DB.prepare("SELECT last_sequence FROM sync_installations_v2").first("last_sequence")).toBe("1");
    env.DB.batch = batch;
    expect((await (await post("/mutations/2/commit", { digest: p.digest })).json()).receipt.outcome).toBe("accepted");
  });
  it("recovers an uncertain database result after publication actually committed", async () => {
    await setup(); const a = await content("2"); const p = await (await post("/mutations/2/prepare", a.m)).json();
    await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: a.bytes });
    const batch = env.DB.batch.bind(env.DB);
    env.DB.batch = (async (commands: D1PreparedStatement[]) => { await batch(commands); throw new Error("Lost database acknowledgement"); }) as typeof env.DB.batch;
    expect((await post("/mutations/2/commit", { digest: p.digest })).status).toBe(500);
    env.DB.batch = batch;
    const receipt = await (await post("/mutations/2/commit", { digest: p.digest })).json();
    expect(receipt.receipt).toMatchObject({ outcome: "accepted", budget: { version: "1" } });
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_events_v2").first("n")).toBe(2);
    expect(env.SYNC.objects.size).toBe(1);
  });
  it("allows only one of cancel/commit to decide and replays that outcome", async () => {
    await setup(); const a = await content("2"); const p = await (await post("/mutations/2/prepare", a.m)).json();
    await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: a.bytes });
    const results = await Promise.all([post("/mutations/2/cancel", { digest: p.digest }), post("/mutations/2/commit", { digest: p.digest })]);
    const bodies = await Promise.all(results.map(r => r.json()));
    expect(bodies[0]).toEqual(bodies[1]);
    expect(["accepted", "cancelled"]).toContain(bodies[0].receipt.outcome);
  });
  it("does not publish a staged payload after another device resets the key", async () => {
    await setup(); await registered("device_b"); const a = await content("2"); const p = await (await post("/mutations/2/prepare", a.m)).json();
    const b = await boot(); const reset: Manifest = { accountId, installationId: "device_b", operationId: "reset_other", kind: "keyReset", keyGeneration: b.keyGeneration, catalogEpoch: b.catalogEpoch, expectedProtocol: 2, key: { salt: "qrstuvwxyzabcdef", check: "replacement" } };
    await perform("1", reset);
    const late = await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: a.bytes });
    expect((await late.json()).receipt.code).toBe("invalidated_generation");
    expect(env.SYNC.objects.size).toBe(0);
  });
  it("consumes cancellation of a frozen old-generation manifest that never reached prepare", async () => {
    await setup(); await registered("device_b"); const frozen = await content("2"); const hash = await manifestDigest(frozen.m);
    const b = await boot(); const reset: Manifest = { accountId, installationId: "device_b", operationId: "reset_before_prepare", kind: "keyReset", keyGeneration: b.keyGeneration, catalogEpoch: b.catalogEpoch, expectedProtocol: 2, key: { salt: "qrstuvwxyzabcdef", check: "replacement" } };
    await perform("1", reset);
    const cancellation = await post("/mutations/2/cancel", { digest: hash, manifest: frozen.m });
    const result = await cancellation.json();
    expect(result).toMatchObject({ state: "terminal", receipt: { sequence: "2", outcome: "cancelled", code: "cancelled", keyGeneration: "1" } });
    expect(await (await post("/mutations/2/cancel", { digest: hash, manifest: frozen.m })).json()).toEqual(result);
    expect(await (await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": hash }, body: frozen.bytes })).json()).toEqual(result);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_heads_v2").first("n")).toBe(0);
    expect(env.SYNC.objects.size).toBe(0);
  });
  it("consumes quota failures without changing heads or pinning staging quota", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    await env.DB.prepare("UPDATE sync_heads_v2 SET size=1073741824").run();
    const b = await content("3", { budgetId: "budget_b" });
    expect((await perform("3", b.m)).receipt.code).toBe("quota");
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2 WHERE state IN ('uploading','ready')").first("n")).toBe(0);
  });
  it("pages immutable events through a frozen bound even while heads change", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    const first = await (await call("/catalog/changes?after=0&through=2&limit=1", { headers: await scope() })).json();
    expect(first).toMatchObject({ cursor: "1", hasMore: true });
    const b = await content("3", { kind: "update", incarnation: "incarnation_2", newIncarnation: undefined, expectedVersion: "1" }, "changed"); await perform("3", b.m, b.bytes);
    const second = await (await call("/catalog/changes?after=1&through=2&limit=1", { headers: await scope() })).json();
    expect(second).toMatchObject({ cursor: "2", hasMore: false, events: [{ sequence: "2", head: { version: "1" } }] });
    const snapshot = await (await call("/catalog/snapshot", { headers: await scope() })).json();
    expect(snapshot).toMatchObject({ highwater: "3", budgets: [{ version: "2" }] });
  });
  it("requires a new snapshot after event expiry while preserving tombstones", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    await env.DB.prepare("UPDATE sync_events_v2 SET created_at='2000-01-01'").run(); await maintainSyncV2(env);
    const old = await call("/catalog/changes?after=0", { headers: await scope() });
    expect(old.status).toBe(410); expect((await old.json()).error).toBe("cursor_expired");
    expect((await call("/catalog/snapshot", { headers: await scope() })).status).toBe(200);
  });
  it("deletes the account with pending blobs and migrated guards without losing cleanup", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    const b = await content("3", { budgetId: "budget_b" }); const p = await (await post("/mutations/3/prepare", b.m)).json();
    await call("/mutations/3/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: b.bytes });
    const response = await app.request("/api/v1/me", { method: "DELETE", headers: json({}), body: JSON.stringify({ confirm: "sync-v2@example.test" }) }, env);
    expect(response.status, await response.clone().text()).toBe(200);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_heads_v2").first("n")).toBe(0);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_installations_v2").first("n")).toBe(0);
    expect(env.SYNC.objects.size).toBe(0);
    expect((await call("/bootstrap")).status).toBe(401);
  });
  it("inventory reclaims an unreferenced late upload while keeping live content", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    // Direct insertion models an R2 write completing after its old handler/lease
    // was already cleaned; no active staging reference can adopt this object.
    env.SYNC.objects.set(`sync-v2/${accountId}/late-object`, new Uint8Array([1, 2, 3]));
    await maintainSyncV2(env); await cleanupSyncObjects(env);
    expect(env.SYNC.objects.has(`sync-v2/${accountId}/late-object`)).toBe(false);
    expect(env.SYNC.objects.size).toBe(1);
  });
  it("compacts retired ownership independently of live staging and still catches a late orphan", async () => {
    await setup(); const pending = await content("2"); await post("/mutations/2/prepare", pending.m);
    const key = `sync-v2/${accountId}/very-late`;
    await env.DB.prepare("INSERT INTO sync_objects_v2(object_key,user_id,size,state,created_at) VALUES (?1,?2,3,'retired','2000-01-01')").bind(key, accountId).run();
    await maintainSyncV2(env);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2 WHERE object_key=?1").bind(key).first("n")).toBe(0);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_staging_v2").first("n")).toBe(1);
    env.SYNC.objects.set(key, new Uint8Array([1, 2, 3]));
    await maintainSyncV2(env); await cleanupSyncObjects(env);
    expect(env.SYNC.objects.has(key)).toBe(false);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2 WHERE state='uploading'").first("n")).toBe(1);
  });
  it("rejects cross-account manifests and retired installation reuse", async () => {
    await setup(); const a = await content("2");
    expect((await post("/mutations/2/prepare", { ...a.m, accountId: "another_account" })).status).toBe(400);
    await env.DB.prepare("UPDATE sync_installations_v2 SET status='retired'").run();
    expect((await registered()).status).toBe(409);
    expect((await post("/mutations/2/prepare", a.m)).status).toBe(409);
  });
  it("bounds JSON bytes and rejects nested fields outside the protocol", async () => {
    await setup(); const a = await content("2");
    expect((await post("/mutations/2/prepare", { ...a.m, padding: "x".repeat(65536) })).status).toBe(413);
    expect((await post("/mutations/2/prepare", { ...a.m, payload: { ...a.m.payload, ignored: { deep: [1] } } })).status).toBe(400);
    expect((await call("/installations/device_a/receipt")).status).toBe(200);
    expect((await (await call("/installations/device_a/receipt")).json()).lastSequence).toBe("1");
  });
  it("rejects short and oversized streams without marking their object ready", async () => {
    await setup(); const a = await content("2"); const p = await (await post("/mutations/2/prepare", a.m)).json();
    for (const size of [a.bytes.length - 1, a.bytes.length + 1]) {
      const response = await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: new Uint8Array(size) });
      expect([400, 413]).toContain(response.status);
    }
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2 WHERE state='ready'").first("n")).toBe(0);
  });
  it("recovers an upload that reached R2 before its ready marker committed", async () => {
    await setup(); const a = await content("2"); const p = await (await post("/mutations/2/prepare", a.m)).json();
    const key = await env.DB.prepare("SELECT object_key FROM sync_staging_v2").first<string>("object_key");
    await env.SYNC.put(key!, a.bytes, { sha256: a.m.payload!.sha256! });
    const response = await call("/mutations/2/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": p.digest }, body: a.bytes });
    expect(response.status).toBe(200); expect((await response.json()).blobReady).toBe(true);
    expect((await (await post("/mutations/2/commit", { digest: p.digest })).json()).receipt.outcome).toBe("accepted");
  });
  it("does not silently skip events compacted between the cursor check and query", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes); const requestScope = await scope();
    const prepare = env.DB.prepare.bind(env.DB); let fired = false;
    env.DB.prepare = ((sql: string) => {
      if (!fired && sql.includes("SELECT sequence,event FROM sync_events_v2")) {
        fired = true; env.DB.sqlite.exec("UPDATE sync_accounts_v2 SET min_cursor='2'; DELETE FROM sync_events_v2;");
      }
      return prepare(sql);
    }) as typeof env.DB.prepare;
    const response = await call("/catalog/changes?after=0&through=2", { headers: requestScope });
    expect(response.status).toBe(410); expect((await response.json()).error).toBe("cursor_expired");
  });
  it("fails closed before SQLite counter arithmetic can leave canonical integer range", async () => {
    await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
    await env.DB.prepare("UPDATE sync_heads_v2 SET version='9223372036854775806'").run();
    const b = await content("3", { kind: "update", incarnation: "incarnation_2", newIncarnation: undefined, expectedVersion: "9223372036854775806" });
    expect((await perform("3", b.m, b.bytes)).receipt.code).toBe("counter_exhausted");
    expect(await env.DB.prepare("SELECT version FROM sync_heads_v2").first("version")).toBe("9223372036854775806");
  });
});


it("reports v2 active budgets and retained trash bytes in account usage", async () => {
  await setup(); const a = await content("2"); await perform("2", a.m, a.bytes);
  const me = () => app.request("/api/v1/me", { headers: auth }, env);
  expect(await (await me()).json()).toMatchObject({ sync: { budgets: 1, bytes: a.bytes.length } });
  const admin = (path: string) => app.request(`/api/v1/admin${path}`, {}, env);
  expect(await (await admin("/accounts")).json()).toMatchObject({ accounts: [{ id: accountId, budgets: 1 }] });
  expect(await (await admin(`/accounts/${accountId}`)).json()).toMatchObject({ syncKeySet: true, budgets: [{ id: "budget_a", state: "active", revision: "1" }] });
  // A different legacy account remains in aggregate totals, never this account's usage.
  await env.DB.prepare("INSERT INTO users(id,email,created_at) VALUES ('usr_other','other@example.test','2026-01-01')").run();
  await env.DB.prepare("INSERT INTO sync_budgets(user_id,budget_id,revision,object_key,size,updated_at) VALUES ('usr_other','legacy',1,'other-object',3,'2026-01-01')").run();
  expect(await (await me()).json()).toMatchObject({ sync: { budgets: 1, bytes: a.bytes.length } });
  expect(await (await admin("/overview")).json()).toMatchObject({ sync: { budgets: 2, bytes: a.bytes.length + 3 } });
  const trash: Manifest = { ...a.m, operationId: "usage-trash", kind: "trash", incarnation: "incarnation_2", expectedVersion: "1" };
  delete trash.payload; delete trash.newIncarnation;
  await perform("3", trash);
  expect(await (await me()).json()).toMatchObject({ sync: { budgets: 0, bytes: a.bytes.length } });
  expect(await (await admin(`/accounts/${accountId}`)).json()).toMatchObject({ budgets: [{ state: "trashed", revision: "2", size: a.bytes.length }] });
  expect(await (await admin("/overview")).json()).toMatchObject({ sync: { budgets: 1, bytes: a.bytes.length + 3 } });
  await perform("4", { ...trash, operationId: "usage-purge", kind: "purge", expectedVersion: "2" });
  expect(await (await me()).json()).toMatchObject({ sync: { budgets: 0, bytes: 0 } });
  expect(await (await admin(`/accounts/${accountId}`)).json()).toMatchObject({ budgets: [] });
});


it("durably reclaims active, trash and staged content after account deletion during an R2 outage", async () => {
  await setup();
  const active = await content("2"); await perform("2", active.m, active.bytes);
  const retained = await content("3", { budgetId: "retained" }); await perform("3", retained.m, retained.bytes);
  const trash: Manifest = { ...retained.m, kind: "trash", operationId: "retained-trash", incarnation: "incarnation_3", expectedVersion: "1" };
  delete trash.payload; delete trash.newIncarnation;
  await perform("4", trash);
  const pending = await content("5", { budgetId: "pending" });
  const prepared = await (await post("/mutations/5/prepare", pending.m)).json();
  expect((await call("/mutations/5/blob", { method: "PUT", headers: { ...headers(), "X-Sync-Digest": prepared.digest }, body: pending.bytes })).status).toBe(200);
  expect(env.SYNC.objects.size).toBe(3);
  const remove = env.SYNC.delete.bind(env.SYNC);
  env.SYNC.delete = async () => { throw new Error("Synthetic R2 outage"); };
  const deleted = await app.request("/api/v1/me", { method: "DELETE", headers: json({}), body: JSON.stringify({ confirm: "sync-v2@example.test" }) }, env);
  expect(deleted.status, await deleted.clone().text()).toBe(200);
  for (const table of ["sync_accounts_v2", "sync_heads_v2", "sync_installations_v2", "sync_staging_v2", "sync_events_v2"])
    expect(await env.DB.prepare(`SELECT COUNT(*) AS n FROM ${table}`).first("n")).toBe(0);
  expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_object_garbage").first("n")).toBe(3);
  expect(env.SYNC.objects.size).toBe(3);
  env.SYNC.delete = remove;
  await cleanupSyncObjects(env);
  expect(env.SYNC.objects.size).toBe(0);
  expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_object_garbage").first("n")).toBe(0);
});


describe("release review server fences and cleanup", () => {
  it("does not reserve dormant accounts or allocate objects for terminal policy rejections", async () => {
    await registered(); delete env.SYNC_V2_ENABLED;
    const a = await content("1", { keyGeneration: "0" });
    for (let n=0; n<150; n++) expect((await post("/mutations/1/prepare", a.m)).status).toBe(409);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_staging_v2").first("n")).toBe(0);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2").first("n")).toBe(0);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_object_garbage").first("n")).toBe(0);
    env.SYNC_V2_ENABLED = "1"; await setup();
    await env.DB.prepare("DELETE FROM subscriptions").run();
    for (let n=2; n<152; n++) {
      const rejected = await content(String(n));
      expect((await perform(String(n), rejected.m)).receipt.code).toBe("subscription_required");
    }
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2").first("n")).toBe(0);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_object_garbage").first("n")).toBe(0);
  });
  it("counts a pending reservation exactly once and rejects excess staging without orphan rows", async () => {
    await setup();
    for (let n=0;n<5;n++) {
      const device = `quota_${n}`; await registered(device);
      const a = await content("1", { installationId: device, budgetId: device }); a.m.payload!.length=50*1024*1024;
      const result = await (await post("/mutations/1/prepare", a.m, device)).json();
      expect(n<4 ? result.state : result.receipt.code).toBe(n<4 ? "prepared" : "staging_quota");
    }
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2").first("n")).toBe(4);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_object_garbage").first("n")).toBe(0);
  });
  it("reclaims replacements and cancelled uploads inline without deleting the accepted head", async () => {
    await setup(); const a=await content("2"); await perform("2",a.m,a.bytes);
    const b=await content("3",{kind:"update",incarnation:"incarnation_2",newIncarnation:undefined,expectedVersion:"1"},"replacement");
    await perform("3",b.m,b.bytes); expect(env.SYNC.objects.size).toBe(1);
    const c=await content("4",{budgetId:"cancelled"}); const p=await (await post("/mutations/4/prepare",c.m)).json();
    await call("/mutations/4/blob",{method:"PUT",headers:{...headers(),"X-Sync-Digest":p.digest},body:c.bytes});
    expect(env.SYNC.objects.size).toBe(2); await post("/mutations/4/cancel",{digest:p.digest});
    expect(env.SYNC.objects.size).toBe(1);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_object_garbage").first("n")).toBe(0);
    const download=await call("/budgets/budget_a/content?version=2&incarnation=incarnation_2",{headers:await scope()});
    expect(await download.text()).toBe("replacement");
  });
  it("drains multiple cleanup pages in bulk and prioritizes a deleted account over unrelated backlog", async () => {
    await setup(); const a=await content("2"); await perform("2",a.m,a.bytes);
    const insert=env.DB.sqlite.prepare("INSERT INTO sync_object_garbage(object_key) VALUES (?)");
    for(let n=0;n<1200;n++){const key=`old-${String(n).padStart(4,"0")}`;insert.run(key);env.SYNC.objects.set(key,new Uint8Array([1]));}
    const remove=vi.spyOn(env.SYNC,"delete");
    const deleted=await app.request("/api/v1/me",{method:"DELETE",headers:json({}),body:JSON.stringify({confirm:"sync-v2@example.test"})},env);
    expect(deleted.status).toBe(200); expect(env.SYNC.objects.size).toBe(1200);
    expect(remove).toHaveBeenCalledTimes(1);
    await cleanupSyncObjects(env); expect(env.SYNC.objects.size).toBe(0);
    expect(remove).toHaveBeenCalledTimes(3);
  });
  it("cancels an oversized unprepared manifest by exact digest without allocating or publishing content", async () => {
    await setup(); const a=await content("2"); a.m.payload!.length=50*1024*1024+1;
    expect((await post("/mutations/2/prepare",a.m)).status).toBe(400);
    const digest=await manifestDigest(a.m);
    const cancelled=await (await post("/mutations/2/cancel",{digest,manifest:a.m})).json();
    expect(cancelled).toMatchObject({state:"terminal",receipt:{sequence:"2",outcome:"cancelled",code:"cancelled",operationId:a.m.operationId}});
    expect(await (await post("/mutations/2/cancel",{digest,manifest:a.m})).json()).toEqual(cancelled);
    expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM sync_objects_v2").first("n")).toBe(0);
    const next=await content("3");expect((await perform("3",next.m,next.bytes)).receipt.outcome).toBe("accepted");
  });
  it("does not let malformed cancellation cross account identity, replace a pending digest or skip sequences", async () => {
    await setup(); const a=await content("2"); const pending=await (await post("/mutations/2/prepare",a.m)).json();
    const malformed={...a.m,operationId:"different",payload:{length:"not-a-size"}};
    expect((await post("/mutations/2/cancel",{digest:await manifestDigest(malformed),manifest:malformed})).status).toBe(409);
    expect((await post("/mutations/3/cancel",{digest:await manifestDigest(malformed),manifest:malformed})).status).toBe(409);
    await post("/mutations/2/cancel",{digest:pending.digest});
    const other={...malformed,accountId:"another_account"};
    expect((await post("/mutations/3/cancel",{digest:await manifestDigest(other),manifest:other})).status).toBe(400);
    expect((await post("/mutations/3/cancel",{digest:"0".repeat(64),manifest:malformed})).status).toBe(400);
    expect((await (await post("/mutations/3/cancel",{digest:await manifestDigest(malformed),manifest:malformed})).json()).receipt.outcome).toBe("cancelled");
  });
  it("validates encrypted metadata before it can become an unreadable content header", async () => {
    await setup();const a=await content("2");a.m.payload!.meta="بيانات";
    expect((await post("/mutations/2/prepare",a.m)).status).toBe(400);
    a.m.payload!.meta="A";expect((await post("/mutations/2/prepare",a.m)).status).toBe(400);
    a.m.payload!.meta="ZW5jcnlwdGVkLW1ldGFkYXRh";expect((await perform("2",a.m,a.bytes)).receipt.outcome).toBe("accepted");
    expect((await call("/budgets/budget_a/content?version=1&incarnation=incarnation_2",{headers:await scope()})).status).toBe(200);
  });
  it("retires resolved installations to free active-device capacity while preserving receipt fences", async () => {
    await setup();
    for(let n=0;n<99;n++) expect((await registered(`old_${n}`)).status).toBe(200);
    expect((await registered("new_device")).status).toBe(409);
    const a=await content("2");await post("/mutations/2/prepare",a.m);
    expect((await call("/installations/device_a",{method:"DELETE"})).status).toBe(409);
    const digest=await manifestDigest(a.m);const cancelled=await (await post("/mutations/2/cancel",{digest})).json();
    expect((await call("/installations/device_a",{method:"DELETE"})).status).toBe(200);
    expect((await registered("new_device")).status).toBe(200);
    expect((await registered()).status).toBe(409);
    expect(await (await post("/mutations/2/commit",{digest})).json()).toEqual(cancelled);
    const next=await content("3");expect((await post("/mutations/3/prepare",next.m)).status).toBe(409);
    const list=await (await call("/installations")).json();expect(list.installations).toHaveLength(100);
    expect(list.installations.some((i:{installationId:string})=>i.installationId==="device_a")).toBe(false);
  });
});
