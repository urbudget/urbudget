import { Hono, type Context } from "hono";
import { requireUser } from "../auth";
import type { AppEnv } from "../env";
import { refreshGooglePurchases } from "../billing/google";
import { syncActive } from "../sync";
import { ApiError, now, rateLimit } from "../util";
import { counter, hasBlob, manifestDigest, parseCancellationManifest, parseManifest, validId, type Head, type Manifest } from "./protocol";
import { account, cancelUnprepared, checkSequence, finish, headJson, installation, mutationState, prepare, register, requireProtocolAccess, requirePublication, staged, statement } from "./repository";
import { uploadCiphertext } from "./upload";

export const syncV2 = new Hono<AppEnv>();
syncV2.use("*", requireUser);
const user = (c: Context<AppEnv>) => c.get("user").id;
// Manifests and catalog ID lists are small. Apply the bound to bytes actually
// read, including chunked bodies, before JSON parsing or canonical recursion.
async function jsonBody(request: Request): Promise<Record<string, unknown>> {
  const maximum = 64 * 1024;
  if (Number(request.headers.get("Content-Length")) > maximum) throw new ApiError(413, "too_large", "The sync request is too large.");
  if (!request.body) throw new ApiError(400, "invalid_body", "Provide a JSON object.");
  const reader = request.body.getReader(), decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: false });
  let size = 0, text = "";
  try {
    for (;;) {
      const part = await reader.read(); if (part.done) break;
      size += part.value.byteLength;
      if (size > maximum) throw new ApiError(413, "too_large", "The sync request is too large.");
      text += decoder.decode(part.value, { stream: true });
    }
    text += decoder.decode(); const value: unknown = JSON.parse(text);
    if (value && typeof value === "object" && !Array.isArray(value)) return value as Record<string, unknown>;
  } catch (error) { if (error instanceof ApiError) throw error; }
  finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
  throw new ApiError(400, "invalid_body", "Provide a JSON object.");
}
function installationId(c: Context<AppEnv>) {
  const id = c.req.header("X-Sync-Installation");
  if (!validId(id)) throw new ApiError(400, "installation_required", "Provide X-Sync-Installation.");
  return id;
}
function digest(value: unknown): string {
  if (typeof value !== "string" || !/^[a-f0-9]{64}$/.test(value)) throw new ApiError(400, "invalid_digest", "Provide the prepared manifest's SHA-256 digest.");
  return value;
}
async function scope(c: Context<AppEnv>) {
  const a = await account(c.env.DB, user(c));
  if (!a || a.protocol !== 2) throw new ApiError(409, "upgrade_required", "Activate sync v2 for this account first.");
  const generation = c.req.query("keyGeneration") ?? c.req.header("X-Sync-Generation");
  const epoch = c.req.query("catalogEpoch") ?? c.req.header("X-Sync-Epoch");
  if (generation !== a.key_generation || epoch !== a.catalog_epoch) throw new ApiError(409, "scope_changed", "The account key or catalog changed. Bootstrap again.", { keyGeneration: a.key_generation, catalogEpoch: a.catalog_epoch });
  return a;
}
syncV2.get("/bootstrap", async c => {
  // Clients gate new publication on bootstrap, so renewal verification must run
  // here too. The shared refresh is leased, cached and bounded to three seconds.
  await refreshGooglePurchases(c.env, user(c));
  const a = await account(c.env.DB, user(c));
  const legacy = !a || a.protocol === 1 ? await statement(c.env.DB, "SELECT CAST(generation AS TEXT) AS generation,salt,key_check FROM sync_keys WHERE user_id=$user", { user: user(c) }).first<{ generation: string; salt: string; key_check: string }>() : null;
  return c.json({ accountId: user(c), protocol: a?.protocol ?? 1,
    keyGeneration: a?.protocol === 2 ? a.key_generation : legacy?.generation ?? null,
    salt: a?.protocol === 2 ? a.salt : legacy?.salt ?? null, check: a?.protocol === 2 ? a.key_check : legacy?.key_check ?? null,
    catalogEpoch: a?.catalog_epoch ?? null, highwater: a?.catalog_sequence ?? "0", minCursor: a?.min_cursor ?? "0",
    entitlement: { active: await syncActive(c.env.DB, user(c)) }, activationEnabled: c.env.SYNC_V2_ENABLED === "1", publicationPaused: c.env.SYNC_V2_PUBLICATION_PAUSED === "1" });
});
syncV2.post("/installations", async c => {
  const body = await jsonBody(c.req.raw);
  if (!validId(body.installationId)) throw new ApiError(400, "invalid_installation", "Provide a random installation ID.");
  await rateLimit(c.env.DB, `sync-v2:register:${user(c)}`, 120, 3600);
  const row = await register(c.env, user(c), body.installationId);
  return c.json({ installationId: body.installationId, lastSequence: row.last_sequence, receipt: row.receipt ? JSON.parse(row.receipt) : null });
});
syncV2.get("/installations", async c => {
  const rows = await statement(c.env.DB, `SELECT installation_id AS installationId,status,created_at AS createdAt,last_sequence AS lastSequence,
    EXISTS(SELECT 1 FROM sync_staging_v2 s WHERE s.user_id=i.user_id AND s.installation_id=i.installation_id) AS pending
    FROM sync_installations_v2 i WHERE user_id=$user AND status='active' ORDER BY created_at,installation_id`, { user: user(c) }).all();
  return c.json({ installations: rows.results });
});
syncV2.delete("/installations/:id", async c => {
  const id = c.req.param("id");
  if (!validId(id)) throw new ApiError(400, "invalid_installation", "The installation ID is invalid.");
  await statement(c.env.DB, `UPDATE sync_installations_v2 SET status='retired' WHERE user_id=$user AND installation_id=$id
    AND NOT EXISTS(SELECT 1 FROM sync_staging_v2 WHERE user_id=$user AND installation_id=$id)`, { user: user(c), id }).run();
  const row = await installation(c.env.DB, user(c), id);
  if (!row) throw new ApiError(404, "installation_required", "The installation is not registered.");
  if (row.status !== "retired") throw new ApiError(409, "pending_mutation", "Resolve or cancel the pending mutation before retiring this installation.");
  return c.json({ installationId: id, status: "retired" });
});
syncV2.get("/installations/:id/receipt", async c => {
  const row = await installation(c.env.DB, user(c), c.req.param("id"));
  if (!row) throw new ApiError(404, "installation_required", "The installation is not registered.");
  const pending = await staged(c.env.DB, user(c), c.req.param("id"));
  if (pending && pending.expires_at <= now()) await finish(c.env, user(c), c.req.param("id"), pending.sequence, pending.digest, "expire");
  const latest = await installation(c.env.DB, user(c), c.req.param("id"));
  const next = await staged(c.env.DB, user(c), c.req.param("id"));
  return c.json({ installationId: c.req.param("id"), lastSequence: latest!.last_sequence, receipt: latest!.receipt ? JSON.parse(latest!.receipt) : null,
    pending: next ? { sequence: next.sequence, digest: next.digest, expiresAt: next.expires_at } : null });
});

syncV2.get("/catalog/snapshot", async c => {
  await scope(c);
  const result = await statement(c.env.DB, `SELECT catalog_epoch AS epoch,key_generation AS generation,catalog_sequence AS highwater,min_cursor AS minimum,
    (SELECT COALESCE(json_group_array(json(head)),'[]') FROM (SELECT ${headJson()} AS head FROM sync_heads_v2 h WHERE h.user_id=a.user_id AND h.state='active' ORDER BY h.budget_id)) AS heads
    FROM sync_accounts_v2 a WHERE user_id=$user`, { user: user(c) }).first<{ epoch: string; generation: string; highwater: string; minimum: string; heads: string }>();
  // A reset can occur after scope's preliminary read; report the actual snapshot
  // scope and reject instead of applying rows under an obsolete client key.
  if (result!.epoch !== (c.req.query("catalogEpoch") ?? c.req.header("X-Sync-Epoch"))) throw new ApiError(409, "scope_changed", "Bootstrap after the catalog changed.");
  return c.json({ accountId: user(c), catalogEpoch: result!.epoch, keyGeneration: result!.generation, highwater: result!.highwater, minCursor: result!.minimum, budgets: JSON.parse(result!.heads) as Head[] });
});
syncV2.get("/catalog/changes", async c => {
  const a = await scope(c);
  const after = counter(c.req.query("after") ?? "0", "after");
  const through = counter(c.req.query("through") ?? a.catalog_sequence, "through");
  if (BigInt(after) < BigInt(a.min_cursor)) throw new ApiError(410, "cursor_expired", "Fetch a new active snapshot and reconcile known IDs.");
  if (BigInt(through) > BigInt(a.catalog_sequence) || BigInt(through) < BigInt(after)) throw new ApiError(400, "invalid_cursor", "The requested catalog bounds are invalid.");
  const limit = Math.min(100, Math.max(1, Math.floor(Number(c.req.query("limit")) || 100)));
  const rows = await statement(c.env.DB, `SELECT sequence,event FROM sync_events_v2 WHERE user_id=$user AND epoch=$epoch
    AND CAST(sequence AS INTEGER)>CAST($after AS INTEGER) AND CAST(sequence AS INTEGER)<=CAST($through AS INTEGER)
    ORDER BY CAST(sequence AS INTEGER) LIMIT $limit`, { user: user(c), epoch: a.catalog_epoch, after, through, limit }).all<{ sequence: string; event: string }>();
  const latest = await scope(c);
  if (BigInt(after) < BigInt(latest.min_cursor)) throw new ApiError(410, "cursor_expired", "The event journal was compacted while reading. Fetch a new snapshot and reconcile known IDs.");
  const next = rows.results.length === limit ? rows.results.at(-1)!.sequence : through;
  return c.json({ catalogEpoch: a.catalog_epoch, keyGeneration: a.key_generation, through, cursor: next, hasMore: BigInt(next) < BigInt(through), events: rows.results.map(r => ({ sequence: r.sequence, ...JSON.parse(r.event) })) });
});
syncV2.post("/budgets/status", async c => {
  const a = await scope(c), body = await jsonBody(c.req.raw);
  if (!Array.isArray(body.ids) || body.ids.length > 100 || body.ids.some(id => !validId(id))) throw new ApiError(400, "invalid_ids", "Provide at most 100 budget IDs.");
  const ids = [...new Set(body.ids as string[])];
  const rows = await statement(c.env.DB, `SELECT budget_id,${headJson()} AS head FROM sync_heads_v2 h WHERE user_id=$user AND budget_id IN (SELECT value FROM json_each($ids))`, { user: user(c), ids: JSON.stringify(ids) }).all<{ budget_id: string; head: string }>();
  await scope(c);
  return c.json({ catalogEpoch: a.catalog_epoch, keyGeneration: a.key_generation, budgets: ids.map(id => ({ budgetId: id, head: JSON.parse(rows.results.find(r => r.budget_id === id)?.head ?? "null") })) });
});
syncV2.get("/trash", async c => {
  const a = await scope(c);
  const after = c.req.query("after") ?? "";
  const rows = await statement(c.env.DB, `SELECT budget_id,${headJson()} AS head FROM sync_heads_v2 h WHERE user_id=$user AND state='trashed' AND retained_until>$at
    AND budget_id>$after ORDER BY budget_id LIMIT 101`, { user: user(c), after, at: now() }).all<{ budget_id: string; head: string }>();
  await scope(c);
  const page = rows.results.slice(0, 100);
  return c.json({ catalogEpoch: a.catalog_epoch, budgets: page.map(r => JSON.parse(r.head)), next: rows.results.length > 100 ? page.at(-1)!.budget_id : null });
});
syncV2.get("/budgets/:id/content", async c => {
  const a = await scope(c);
  const expected = counter(c.req.query("version"), "version"), incarnation = c.req.query("incarnation");
  const read = () => statement(c.env.DB, `SELECT object_key,${headJson()} AS head FROM sync_heads_v2 h WHERE user_id=$user AND budget_id=$budget`, { user: user(c), budget: c.req.param("id") }).first<{ object_key: string | null; head: string }>();
  const row = await read();
  const head: Head | null = row ? JSON.parse(row.head) : null;
  if (!head) throw new ApiError(404, "unknown_budget", "The account has no such budget identity.");
  if (head.version !== expected || head.incarnation !== incarnation) throw new ApiError(409, "version_changed", "The budget head changed.", { head });
  if (!row!.object_key || head.state === "purged" || (head.state === "trashed" && (!head.retainedUntil || head.retainedUntil <= now()))) throw new ApiError(410, "gone", "The retained payload is no longer available.", { head });
  const object = await c.env.SYNC.get(row!.object_key);
  await scope(c);
  if (!object) {
    const latest = await read();
    if (!latest || latest.head !== row!.head) throw new ApiError(409, "version_changed", "The head changed while reading its payload.");
    throw new ApiError(503, "storage_unavailable", "A referenced encrypted payload is temporarily unavailable; the budget has not been deleted.");
  }
  return new Response(object.body, { headers: { "Content-Type": "application/octet-stream", "Content-Length": String(object.size), "Cache-Control": "no-store",
    "X-Sync-Generation": a.key_generation!, "X-Sync-Epoch": a.catalog_epoch, "X-Sync-Incarnation": head.incarnation, "X-Sync-Version": head.version,
    "X-Sync-Payload": JSON.stringify(head.payload) } });
});

syncV2.post("/mutations/:sequence/prepare", async c => {
  const id = installationId(c), seq = counter(c.req.param("sequence"), "sequence");
  const m = parseManifest(await jsonBody(c.req.raw), user(c), id), hash = await manifestDigest(m);
  const old = checkSequence(await installation(c.env.DB, user(c), id), seq, hash);
  if (old) return c.json({ state: "terminal", receipt: old });
  await requireProtocolAccess(c.env, user(c));
  requirePublication(c.env, m.kind);
  await rateLimit(c.env.DB, `sync-v2:prepare:${user(c)}`, 1200, 3600);
  await refreshGooglePurchases(c.env, user(c));
  return c.json(await prepare(c.env, user(c), id, seq, hash, m));
});
syncV2.put("/mutations/:sequence/blob", async c => {
  const id = installationId(c), seq = counter(c.req.param("sequence"), "sequence"), hash = digest(c.req.header("X-Sync-Digest"));
  const old = checkSequence(await installation(c.env.DB, user(c), id), seq, hash);
  if (old) return c.json({ state: "terminal", receipt: old });
  const s = await staged(c.env.DB, user(c), id);
  if (!s || s.sequence !== seq || s.digest !== hash || !s.object_key) throw new ApiError(409, "prepare_required", "Prepare the payload manifest first.");
  if (s.expires_at <= now()) return c.json(await finish(c.env, user(c), id, seq, hash, "expire"));
  const m = JSON.parse(s.manifest) as Manifest;
  if (!hasBlob(m.kind) || !m.payload) throw new ApiError(400, "unexpected_blob", "This mutation has no payload.");
  requirePublication(c.env, m.kind);
  const a = await account(c.env.DB, user(c));
  if (a?.key_generation !== m.keyGeneration || a.catalog_epoch !== m.catalogEpoch) return c.json(await finish(c.env, user(c), id, seq, hash));
  const existing = await statement(c.env.DB, "SELECT state FROM sync_objects_v2 WHERE object_key=$object", { object: s.object_key }).first<{ state: string }>();
  if (existing?.state === "ready") return c.json(await mutationState(c.env, user(c), id, seq, hash));
  if (existing?.state !== "uploading") throw new ApiError(409, "staging_retired", "This staging object is no longer eligible for publication.");
  const length = c.req.header("Content-Length");
  if (length !== undefined && Number(length) !== m.payload.length) throw new ApiError(400, "size_mismatch", "The upload length does not match its immutable manifest.");
  if (!c.req.raw.body) throw new ApiError(400, "empty", "The encrypted payload is missing.");
  await uploadCiphertext(c.env.SYNC, s.object_key, c.req.raw.body, m.payload.length, m.payload.sha256!);
  // Cancellation/reset/expiry while the upload ran prevents adoption. The durable
  // ledger and inventory retain cleanup even if R2 completes after that fence.
  await statement(c.env.DB, `UPDATE sync_objects_v2 SET state='ready' WHERE object_key=$object AND state='uploading' AND expires_at>$at
    AND EXISTS(SELECT 1 FROM sync_staging_v2 WHERE user_id=$user AND installation_id=$id AND sequence=$seq AND digest=$digest AND object_key=$object)`,
    { object: s.object_key, at: now(), user: user(c), id, seq, digest: hash }).run();
  return c.json(await mutationState(c.env, user(c), id, seq, hash));
});
syncV2.post("/mutations/:sequence/commit", async c => {
  const id = installationId(c), seq = counter(c.req.param("sequence"), "sequence"), body = await jsonBody(c.req.raw);
  const hash = digest(body.digest), old = checkSequence(await installation(c.env.DB, user(c), id), seq, hash);
  if (old) return c.json({ state: "terminal", receipt: old });
  const pending = await staged(c.env.DB, user(c), id);
  if (pending?.sequence === seq && pending.digest === hash && hasBlob((JSON.parse(pending.manifest) as Manifest).kind))
    await refreshGooglePurchases(c.env, user(c));
  return c.json(await finish(c.env, user(c), id, seq, hash));
});
syncV2.post("/mutations/:sequence/cancel", async c => {
  const id = installationId(c), seq = counter(c.req.param("sequence"), "sequence"), body = await jsonBody(c.req.raw), hash = digest(body.digest);
  const old = checkSequence(await installation(c.env.DB, user(c), id), seq, hash);
  if (old) return c.json({ state: "terminal", receipt: old });
  if (!await staged(c.env.DB, user(c), id)) {
    if (!body.manifest || typeof body.manifest !== "object" || Array.isArray(body.manifest)) throw new ApiError(400, "manifest_required", "Cancelling an unprepared sequence requires its manifest.");
    const m = parseCancellationManifest(body.manifest as Record<string, unknown>, user(c), id);
    if (await manifestDigest(m) !== hash) throw new ApiError(400, "digest_mismatch", "The cancellation manifest differs.");
    const reserved = await cancelUnprepared(c.env, user(c), id, seq, hash, m);
    if (reserved.state === "terminal") return c.json(reserved);
  }
  return c.json(await finish(c.env, user(c), id, seq, hash, "cancel"));
});
