import { Hono, type Context } from "hono";
import { requireUser } from "../auth";
import type { AppEnv } from "../env";
import { refreshGooglePurchases } from "../billing/google";
import { syncActive } from "../sync";
import { ApiError, now, rateLimit } from "../util";
import { counter, hasBlob, manifestDigest, parseCancellationManifest, parseManifest, sealedKey, validId, validRecovery, type Head, type Manifest } from "./protocol";
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
  const keyGeneration = a?.protocol === 2 ? a.key_generation : legacy?.generation ?? null;
  return c.json({ accountId: user(c), protocol: a?.protocol ?? 1, keyGeneration,
    salt: a?.protocol === 2 ? a.salt : legacy?.salt ?? null, check: a?.protocol === 2 ? a.key_check : legacy?.key_check ?? null,
    // What the key is made from, so a device knows what to ask for: a recovery code the app made
    // ("code") or a passphrase someone chose. `recovery` is a replacement recovery code's copy of the key.
    secret: keyGeneration === null ? null : a?.protocol === 2 && a.key_secret === "code" ? "code" : "passphrase",
    recovery: a?.protocol === 2 && a.recovery_salt && a.recovery_wrapped ? { salt: a.recovery_salt, wrapped: a.recovery_wrapped } : null,
    catalogEpoch: a?.catalog_epoch ?? null, highwater: a?.catalog_sequence ?? "0", minCursor: a?.min_cursor ?? "0",
    entitlement: { active: await syncActive(c.env.DB, user(c)) }, activationEnabled: c.env.SYNC_V2_ENABLED === "1", publicationPaused: c.env.SYNC_V2_PUBLICATION_PAUSED === "1" });
});

// A replacement recovery code. A device that holds the key makes a new code and leaves here a
// copy of the key sealed with a key stretched from it, replacing any earlier copy. The key, its
// generation and every budget stay as they are. The server can't tell a right copy from a wrong
// one: a device that later opens it checks the result against `check` before keeping it.
syncV2.put("/recovery", async c => {
  const body = await jsonBody(c.req.raw);
  if (!validRecovery({ salt: body.salt, wrapped: body.wrapped })) throw new ApiError(400, "invalid_recovery", "The recovery details are malformed.");
  await rateLimit(c.env.DB, `sync-v2:recovery:${user(c)}`, 20, 3600);
  const p = { user: user(c), generation: typeof body.keyGeneration === "string" ? body.keyGeneration : "", salt: body.salt as string, wrapped: body.wrapped as string, at: now() };
  const changed = await statement(c.env.DB, `UPDATE sync_accounts_v2 SET recovery_salt=$salt,recovery_wrapped=$wrapped,updated_at=$at
    WHERE user_id=$user AND protocol=2 AND key_generation=$generation`, p).run();
  if (!changed.meta.changes) {
    const a = await account(c.env.DB, user(c));
    if (a?.protocol !== 2 || a.key_generation === null) throw new ApiError(409, "upgrade_required", "Activate sync v2 for this account first.");
    throw new ApiError(409, "sync_key_changed", "Sync was reset on another device. Unlock sync on this device again.");
  }
  return c.json({ ok: true });
});

// Device approval. A device that holds the sync key leaves it here sealed with a one-time code
// from a screen; the new device, given that code by camera or keyboard, collects it once. The
// server sees neither the code nor the key. Both protocols use the same key material, so this
// needs no activation and reads the generation as bootstrap does.
const PAIRING_MS = 5 * 60_000, MAX_PAIRINGS = 3;
function pairingId(c: Context<AppEnv>) {
  const id = c.req.param("id") ?? "";
  if (!/^[a-f0-9]{32}$/.test(id)) throw new ApiError(400, "invalid_pairing", "The pairing ID is invalid.");
  return id;
}
async function keyGeneration(c: Context<AppEnv>) {
  const a = await account(c.env.DB, user(c));
  if (a?.protocol === 2) return a.key_generation;
  return await statement(c.env.DB, "SELECT CAST(generation AS TEXT) AS generation FROM sync_keys WHERE user_id=$user", { user: user(c) }).first<string>("generation");
}
syncV2.put("/pairings/:id", async c => {
  const id = pairingId(c), body = await jsonBody(c.req.raw);
  if (!sealedKey(body.wrapped)) throw new ApiError(400, "invalid_pairing", "The sealed key is malformed.");
  const generation = await keyGeneration(c);
  if (generation === null) throw new ApiError(404, "no_key", "Set up sync on this account first.");
  if (body.keyGeneration !== generation) throw new ApiError(409, "sync_key_changed", "Sync was reset on another device. Unlock sync on this device again.");
  await rateLimit(c.env.DB, `sync-v2:pairing:${user(c)}`, 30, 3600);
  const p = { user: user(c), id, wrapped: body.wrapped, generation, at: now(), expires: new Date(Date.now() + PAIRING_MS).toISOString(), limit: MAX_PAIRINGS };
  await c.env.DB.batch([
    statement(c.env.DB, "DELETE FROM sync_pairings_v2 WHERE user_id=$user AND expires_at<=$at", p),
    statement(c.env.DB, `INSERT INTO sync_pairings_v2(user_id,id,wrapped,key_generation,expires_at)
      SELECT $user,$id,$wrapped,$generation,$expires WHERE (SELECT COUNT(*) FROM sync_pairings_v2 WHERE user_id=$user AND id<>$id AND collected=0)<$limit
      ON CONFLICT(user_id,id) DO UPDATE SET wrapped=excluded.wrapped,key_generation=excluded.key_generation,expires_at=excluded.expires_at,collected=0`, p),
  ]);
  const stored = await statement(c.env.DB, "SELECT expires_at FROM sync_pairings_v2 WHERE user_id=$user AND id=$id AND wrapped=$wrapped", p).first<string>("expires_at");
  if (!stored) throw new ApiError(429, "too_many_pairings", "Too many devices are waiting to be added. Finish or cancel one first.");
  return c.json({ id, expiresAt: stored });
});
// Asked every few seconds by a new device showing its own code, until a device that syncs has
// approved it, and by a device showing an offer, until the new one has collected it: about a
// hundred times each in the five minutes a code lasts.
const pairingAsked = (c: Context<AppEnv>) => rateLimit(c.env.DB, `sync-v2:pairing-fetch:${user(c)}`, 900, 3600);
syncV2.get("/pairings/:id", async c => {
  const id = pairingId(c);
  await pairingAsked(c);
  // Marked taken in one statement: a second device, or a second try, finds nothing. The sealed
  // key is blanked in the same transaction, and the marker stays until the code would have run
  // out, for `/state`.
  const p = { user: user(c), id, at: now() };
  const [taken] = await c.env.DB.batch<{ wrapped: string; key_generation: string }>([
    statement(c.env.DB, "UPDATE sync_pairings_v2 SET collected=1 WHERE user_id=$user AND id=$id AND collected=0 AND expires_at>$at RETURNING wrapped,key_generation", p),
    statement(c.env.DB, "UPDATE sync_pairings_v2 SET wrapped='' WHERE user_id=$user AND id=$id AND collected=1", p),
  ]);
  const row = taken.results[0];
  if (!row) throw new ApiError(404, "pairing_not_found", "That code has expired or was already used. Make a new one on the other device.");
  return c.json({ wrapped: row.wrapped, keyGeneration: row.key_generation });
});
// Whether a sealed key is still waiting, or was collected. Reads only, so the device showing an
// offer can close its screen once the new device has the key, and say so.
syncV2.get("/pairings/:id/state", async c => {
  const id = pairingId(c);
  await pairingAsked(c);
  const row = await statement(c.env.DB, "SELECT collected FROM sync_pairings_v2 WHERE user_id=$user AND id=$id AND expires_at>$at", { user: user(c), id, at: now() }).first<{ collected: number }>();
  return c.json({ waiting: !!row && !row.collected, collected: !!row?.collected });
});
syncV2.delete("/pairings/:id", async c => {
  await statement(c.env.DB, "DELETE FROM sync_pairings_v2 WHERE user_id=$user AND id=$id", { user: user(c), id: pairingId(c) }).run();
  return c.json({ ok: true });
});

syncV2.post("/installations", async c => {
  const body = await jsonBody(c.req.raw);
  if (!validId(body.installationId)) throw new ApiError(400, "invalid_installation", "Provide a random installation ID.");
  await rateLimit(c.env.DB, `sync-v2:register:${user(c)}`, 120, 3600);
  const row = await register(c.env, user(c), body.installationId);
  // What the device calls itself, for the account's list of devices. Registering again renames it.
  const device = typeof body.device === "string" ? body.device.trim().slice(0, 64) : "";
  await statement(c.env.DB, `UPDATE sync_installations_v2 SET device=CASE WHEN $device='' THEN device ELSE $device END,last_seen_at=$at
    WHERE user_id=$user AND installation_id=$id AND status='active'`, { user: user(c), id: body.installationId, device, at: now() }).run();
  return c.json({ installationId: body.installationId, lastSequence: row.last_sequence, receipt: row.receipt ? JSON.parse(row.receipt) : null });
});
syncV2.get("/installations", async c => {
  const rows = await statement(c.env.DB, `SELECT installation_id AS installationId,status,created_at AS createdAt,last_sequence AS lastSequence,device,last_seen_at AS lastSeenAt,
    EXISTS(SELECT 1 FROM sync_staging_v2 s WHERE s.user_id=i.user_id AND s.installation_id=i.installation_id) AS pending
    FROM sync_installations_v2 i WHERE user_id=$user AND status='active' AND cut_off=0 ORDER BY created_at,installation_id`, { user: user(c) }).all();
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

// A sync pass asks for the catalog, so that is where a device is seen: noted at most every two
// minutes. A device says whether it holds the account's key (X-Sync-Unlocked): one that asks
// while locked isn't syncing, and one a reset cut off is back in the list only once it has the
// new key. Apps from before that header say neither, and are noted until a reset cuts them off.
async function seen(c: Context<AppEnv>) {
  const id = c.req.header("X-Sync-Installation"), held = c.req.header("X-Sync-Unlocked");
  if (!validId(id) || held === "0") return;
  await statement(c.env.DB, `UPDATE sync_installations_v2 SET last_seen_at=$at,cut_off=0 WHERE user_id=$user AND installation_id=$id AND status='active'
    AND CASE WHEN cut_off=1 THEN $held=1 ELSE last_seen_at IS NULL OR last_seen_at<$before END`,
    { user: user(c), id, at: now(), held: held === "1" ? 1 : 0, before: new Date(Date.now() - 120_000).toISOString() }).run();
}
syncV2.get("/catalog/snapshot", async c => {
  await scope(c);
  await seen(c);
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
  await seen(c);
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
