// Real local workerd test of legacy cutover, D1 fences, restore and key controls.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { startLocal } from "./sync-v2-local.mjs";
const local = await startLocal({ port: 8790 });
const auth = { Authorization: `Bearer ${local.token}` }, installationId = "protocol_qa";
const canonical = value => Array.isArray(value) ? `[${value.map(canonical).join(",")}]` : value && typeof value === "object" ? `{${Object.keys(value).sort().map(k => `${JSON.stringify(k)}:${canonical(value[k])}`).join(",")}}` : JSON.stringify(value);
const hash = data => createHash("sha256").update(data).digest("hex");
async function call(path, method = "GET", body, headers = {}) {
  const response = await fetch(local.url + path, { method, signal: AbortSignal.timeout(15000), headers: { ...auth, ...headers, ...(body && typeof body !== "string" && !Buffer.isBuffer(body) ? { "Content-Type": "application/json" } : {}) }, body: body === undefined ? undefined : typeof body === "string" || Buffer.isBuffer(body) ? body : JSON.stringify(body) });
  return response;
}
async function json(path, method = "GET", body, headers = {}) { const response = await call(path, method, body, headers); const result = await response.json(); assert.equal(response.status, 200, JSON.stringify(result)); return result; }
const v2 = "/api/v2/sync", legacy = "/api/v1/sync";
async function perform(sequence, manifest, bytes) {
  const headers = { "X-Sync-Installation": installationId }, digest = hash(canonical(manifest));
  const prepared = await json(`${v2}/mutations/${sequence}/prepare`, "POST", manifest, headers);
  if (prepared.state === "terminal") return prepared;
  assert.equal(prepared.digest, digest);
  if (bytes) {
    await json(`${v2}/mutations/${sequence}/blob`, "PUT", bytes, { ...headers, "X-Sync-Digest": digest });
    // Reproduce a Worker dying after R2 completed but before D1 marked ready.
    await local.execute(`UPDATE sync_objects_v2 SET state='uploading' WHERE user_id='${local.accountId}' AND installation_id='${installationId}' AND sequence='${sequence}'`);
    const retried = await json(`${v2}/mutations/${sequence}/blob`, "PUT", bytes, { ...headers, "X-Sync-Digest": digest });
    assert.equal(retried.blobReady, true);
  }
  return json(`${v2}/mutations/${sequence}/commit`, "POST", { digest }, headers);
}
try {
  await json(`${legacy}/key`, "PUT", { salt: "abcdefghijklmnop", check: "legacy-key-check" });
  for (let i = 0; i < 3; i++) await json(`${legacy}/budgets/legacy_${i}`, "PUT", Buffer.from(`legacy ciphertext ${i}`), { "If-Match": "0", "X-Sync-Generation": "0", "X-Sync-Meta": `legacy-meta-${i}` });
  await json(`${v2}/installations`, "POST", { installationId });
  let bootstrap = await json(`${v2}/bootstrap`);
  const migrate = { accountId: local.accountId, installationId, operationId: "migrate", kind: "migrateProtocol", keyGeneration: "0", catalogEpoch: bootstrap.catalogEpoch, expectedProtocol: 1 };
  const migration = await perform("1", migrate); assert.equal(migration.receipt.outcome, "accepted"); assert.deepEqual(await perform("1", migrate), migration);
  bootstrap = await json(`${v2}/bootstrap`); assert.equal(bootstrap.check, "legacy-key-check");
  let scope = { "X-Sync-Generation": bootstrap.keyGeneration, "X-Sync-Epoch": bootstrap.catalogEpoch };
  const catalog = await json(`${v2}/catalog/snapshot`, "GET", undefined, scope); assert.equal(catalog.budgets.length, 3);
  for (const head of catalog.budgets) {
    assert.equal(head.payload.format, "legacy-v1"); assert.equal(head.version, "1");
    const response = await call(`${v2}/budgets/${head.budgetId}/content?version=${head.version}&incarnation=${head.incarnation}`, "GET", undefined, scope);
    assert.equal(await response.text(), `legacy ciphertext ${head.budgetId.at(-1)}`);
  }
  for (const [path, method, body] of [[`${legacy}/key`, "PUT", { salt: "abcdefghijklmnop", check: "bad-reset", reset: true }], [`${legacy}/budgets/legacy_0`, "DELETE"], [`${legacy}/budgets/new`, "PUT", "stale bytes"]]) {
    assert.equal((await call(path, method, body, { "X-Sync-Generation": "0", "If-Match": "0" })).status, 426);
  }
  // Bypass middleware exactly as an already-running old Worker would.
  await assert.rejects(local.execute(`UPDATE sync_keys SET generation=generation+1 WHERE user_id='${local.accountId}'`), /sync_upgrade_required/);
  await assert.rejects(local.execute(`INSERT INTO sync_budgets(user_id,budget_id,revision,object_key,size,updated_at) VALUES('${local.accountId}','late_writer',1,'late_object',1,'2026-10-28')`), /sync_upgrade_required/);
  const first = catalog.budgets[0];
  const base = { accountId: local.accountId, installationId, keyGeneration: "0", catalogEpoch: bootstrap.catalogEpoch, budgetId: first.budgetId, incarnation: first.incarnation, expectedVersion: first.version };
  const trashed = await perform("2", { ...base, operationId: "trash", kind: "trash" }); assert.equal(trashed.receipt.budget.state, "trashed");
  const bytes = Buffer.from("new ciphertext under restore authenticated context");
  const context = { format: "v2", accountId: local.accountId, keyGeneration: "0", budgetId: first.budgetId, incarnation: "restored_incarnation", operationId: "restore", parentVersion: "2" };
  const restored = await perform("3", { ...base, expectedVersion: "2", operationId: "restore", kind: "restore", newIncarnation: "restored_incarnation", source: { incarnation: first.incarnation, version: "2", sha256: null }, payload: { format: "v2", sha256: hash(bytes), length: bytes.length, meta: "new-metadata", context } }, bytes);
  assert.equal(restored.receipt.budget.incarnation, "restored_incarnation"); assert.equal(restored.receipt.budget.version, "3");
  const reset = { accountId: local.accountId, installationId, operationId: "reset", kind: "keyReset", keyGeneration: "0", catalogEpoch: bootstrap.catalogEpoch, expectedProtocol: 2, key: { salt: "qrstuvwxyzabcdef", check: "reset-check" } };
  const receipt = await perform("4", reset); assert.equal(receipt.receipt.keyGeneration, "1"); assert.deepEqual(await perform("4", reset), receipt);
  bootstrap = await json(`${v2}/bootstrap`); scope = { "X-Sync-Generation": "1", "X-Sync-Epoch": bootstrap.catalogEpoch };
  assert.equal((await json(`${v2}/catalog/snapshot`, "GET", undefined, scope)).budgets.length, 0);
  console.log("PASS real workerd: three-object legacy migration, immutable replay, original SQL write/reset fences, legacy-source restore, crash-after-R2-upload retry, atomic key reset");
} catch (error) { console.error(local.logs().slice(-5000)); throw error; }
finally { await local.stop(); }
