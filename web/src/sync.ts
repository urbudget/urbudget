// End-to-end encrypted sync. Devices upload whole encrypted budgets; the server keeps the newest
// bytes and a revision per budget and never sees a key. Needs an active Sync subscription ($3 a month).
import { Hono, type Context, type MiddlewareHandler } from "hono";
import { requireUser } from "./auth";
import type { AppEnv } from "./env";
import { SYNC_STATUSES } from "./billing/provider";
import { refreshGooglePurchases } from "./billing/google";
import { ApiError, countEvent, jsonBody, now, randomId } from "./util";
import { reclaimObjects } from "./sync-v2/objects";

const MAX_BYTES = 50 * 1024 * 1024;
const MAX_TOTAL_BYTES = 1024 * 1024 * 1024;
const MAX_BUDGETS = 50;
const validId = (id: string) => /^[A-Za-z0-9_-]{1,64}$/.test(id);
const header = (value: string | undefined, max: number) => (value ?? "").slice(0, max);

export async function syncActive(db: D1Database, userId: string): Promise<boolean> {
  const row = await db
    .prepare(
      `SELECT 1 FROM subscriptions WHERE user_id = ?1 AND status IN (${SYNC_STATUSES.map((s) => `'${s}'`).join(",")})
       AND ((id NOT LIKE 'google:%' AND id NOT LIKE 'apple:%') OR current_period_end > ?2) LIMIT 1`,
    )
    .bind(userId, now())
    .first();
  return !!row;
}

const requireSync: MiddlewareHandler<AppEnv> = async (c, next) => {
  await refreshGooglePurchases(c.env, c.get("user").id);
  if (!(await syncActive(c.env.DB, c.get("user").id)))
    throw new ApiError(402, "subscription_required", "Sync needs an active subscription. Open Account & sync settings to continue.");
  await next();
};

/** Active budgets count toward the library limit; retained trash still occupies storage. */
export async function syncUsage(db: D1Database, userId: string | null = null) {
  const query = db.prepare(`SELECT COALESCE(SUM(active),0) AS budgets,
    COUNT(DISTINCT CASE WHEN active=1 THEN user_id END) AS users,COALESCE(SUM(size),0) AS bytes
    FROM (SELECT user_id,size,1 AS active FROM sync_budgets
      UNION ALL SELECT user_id,size,state='active' AS active FROM sync_heads_v2)
    ${userId === null ? "" : "WHERE user_id=?1"}`);
  return (userId === null ? query : query.bind(userId)).first<{ budgets: number; users: number; bytes: number }>();
}

/** Cleanup failures remain queued by the database triggers for the next sweep. */
async function cleanObjects(env: AppEnv["Bindings"], keys: string[]) {
  await reclaimObjects(env, keys);
}

export async function cleanupSyncObjects(env: AppEnv["Bindings"], owner?: string) {
  const deadline = Date.now() + 10_000;
  let after = "";
  // Bound each sweep's work and time between batches. Failed keys are retried next
  // sweep rather than monopolizing its first page. Owner cleanup bypasses backlog.
  for (let page = 0; page < 20 && Date.now() < deadline; page++) {
    const rows = await env.DB.prepare(owner
      ? `SELECT object_key FROM sync_objects_v2 WHERE user_id=?1 AND object_key>?2 ORDER BY object_key LIMIT 1000`
      : `SELECT object_key FROM sync_object_garbage WHERE object_key>?1 ORDER BY object_key LIMIT 1000`)
      .bind(...(owner ? [owner, after] : [after])).all<{ object_key: string }>();
    if (!rows.results.length) break;
    await cleanObjects(env, rows.results.map(row => row.object_key));
    after = rows.results.at(-1)!.object_key;
    if (rows.results.length < 1000) break;
  }
}

async function discardObject(env: AppEnv["Bindings"], key: string) {
  // A failed database call can have an uncertain commit result. Never clean up
  // bytes that the database did, in fact, adopt as the current budget.
  const queued = await env.DB.prepare(`INSERT OR IGNORE INTO sync_object_garbage (object_key)
    SELECT ?1 WHERE NOT EXISTS (SELECT 1 FROM sync_budgets WHERE object_key = ?1)
    RETURNING object_key`).bind(key).first();
  if (queued) await cleanObjects(env, [key]);
}

/** Delete database references first; never erase an object still referenced by a budget. */
export async function deleteSyncedBudgets(env: AppEnv["Bindings"], userId: string) {
  const rows = await env.DB.prepare("DELETE FROM sync_budgets WHERE user_id = ?1 RETURNING object_key")
    .bind(userId)
    .all<{ object_key: string }>();
  await cleanObjects(env, rows.results.map((row) => row.object_key));
}

async function assertGeneration(c: Context<AppEnv>, expected: number) {
  const key = await c.env.DB.prepare("SELECT generation FROM sync_keys WHERE user_id = ?1")
    .bind(c.get("user").id).first<{ generation: number }>();
  if (!key) throw new ApiError(404, "no_key", "Set up or unlock sync before using cloud budgets.");
  if (key.generation !== expected) {
    if (!c.req.header("X-Sync-Generation"))
      throw new ApiError(428, "sync_generation_required", "Sync was reset. Update UrBudget and unlock sync with the current passphrase.");
    throw new ApiError(409, "sync_key_changed", "Sync was reset on another device. Unlock sync with the current passphrase.");
  }
}

async function generationFor(c: Context<AppEnv>) {
  const value = c.req.header("X-Sync-Generation");
  const generation = value === undefined ? 0 : Number(value);
  if ((value !== undefined && !/^\d+$/.test(value)) || !Number.isSafeInteger(generation) || generation < 0)
    throw new ApiError(400, "invalid_generation", "The sync key generation is invalid.");
  await assertGeneration(c, generation);
  return generation;
}

export const sync = new Hono<AppEnv>();
sync.use("*", requireUser, async (c, next) => {
  const migrated = await c.env.DB.prepare("SELECT 1 FROM sync_accounts_v2 WHERE user_id=?1 AND protocol=2").bind(c.get("user").id).first();
  if (migrated) throw new ApiError(426, "sync_upgrade_required", "This account uses sync v2. Update UrBudget to continue syncing; local budgets remain available.");
  await next();
}, requireSync);

sync.get("/key", async (c) => {
  const key = await c.env.DB.prepare("SELECT salt, key_check, created_at, generation FROM sync_keys WHERE user_id = ?1")
    .bind(c.get("user").id)
    .first<{ salt: string; key_check: string; created_at: string; generation: number }>();
  if (!key) throw new ApiError(404, "no_key", "Sync hasn't been set up for this account yet.");
  return c.json({ salt: key.salt, check: key.key_check, createdAt: key.created_at, generation: key.generation });
});

sync.put("/key", async (c) => {
  const body = await jsonBody(c.req.raw);
  const { salt, check } = body;
  if (typeof salt !== "string" || !/^[A-Za-z0-9_-]{16,128}$/.test(salt) || typeof check !== "string" || check.length > 1024)
    throw new ApiError(400, "invalid_key", "The sync key details are malformed.");
  const userId = c.get("user").id;
  if (body.reset === true) {
    const generation = await generationFor(c);
    // D1 batch is a transaction: no upload can commit between deletion and key
    // rotation. A reset that lost its generation comparison deletes nothing.
    const results = await c.env.DB.batch([
      c.env.DB.prepare(`DELETE FROM sync_budgets WHERE user_id = ?1
        AND EXISTS (SELECT 1 FROM sync_keys WHERE user_id = ?1 AND generation = ?2) RETURNING object_key`)
        .bind(userId, generation),
      c.env.DB.prepare(`UPDATE sync_keys SET salt = ?2, key_check = ?3, created_at = ?4, generation = generation + 1
        WHERE user_id = ?1 AND generation = ?5`).bind(userId, salt, check, now(), generation),
    ]);
    if (!results[1].meta.changes) {
      await assertGeneration(c, generation);
      throw new ApiError(409, "sync_key_changed", "Sync changed while resetting. Unlock sync and try again.");
    }
    await cleanObjects(c.env, results[0].results.map((row) => (row as { object_key: string }).object_key));
    return c.json({ ok: true, generation: generation + 1 });
  }
  const result = await c.env.DB.prepare(
    `INSERT INTO sync_keys (user_id, salt, key_check, created_at) VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(user_id) DO NOTHING`,
  )
    .bind(userId, salt, check, now())
    .run();
  if (!result.meta.changes)
    throw new ApiError(409, "key_exists", "Sync already has a passphrase. Enter it, or reset sync to choose a new one.");
  return c.json({ ok: true, generation: 0 });
});

sync.get("/budgets", async (c) => {
  const generation = await generationFor(c);
  const rows = await c.env.DB.prepare(
    "SELECT budget_id, revision, size, meta, device, updated_at FROM sync_budgets WHERE user_id = ?1 ORDER BY updated_at DESC",
  )
    .bind(c.get("user").id)
    .all<{ budget_id: string; revision: number; size: number; meta: string; device: string; updated_at: string }>();
  await assertGeneration(c, generation);
  return c.json({
    generation,
    budgets: rows.results.map((r) => ({
      id: r.budget_id,
      revision: r.revision,
      size: r.size,
      meta: r.meta,
      device: r.device,
      updatedAt: r.updated_at,
    })),
  });
});

sync.get("/budgets/:id", async (c) => {
  const generation = await generationFor(c);
  const row = await c.env.DB.prepare(
    "SELECT revision, object_key, meta FROM sync_budgets WHERE user_id = ?1 AND budget_id = ?2",
  )
    .bind(c.get("user").id, c.req.param("id"))
    .first<{ revision: number; object_key: string; meta: string }>();
  const object = row ? await c.env.SYNC.get(row.object_key) : null;
  await assertGeneration(c, generation);
  if (!row || !object) throw new ApiError(404, "not_found", "That budget isn't in sync.");
  return new Response(object.body, {
    headers: {
      "Content-Type": "application/octet-stream",
      "Content-Length": String(object.size),
      "X-Revision": String(row.revision),
      "X-Sync-Meta": row.meta,
      "X-Sync-Generation": String(generation),
      "Cache-Control": "no-store",
    },
  });
});

sync.put("/budgets/:id", async (c) => {
  const userId = c.get("user").id;
  const generation = await generationFor(c);
  const budgetId = c.req.param("id");
  if (!validId(budgetId)) throw new ApiError(400, "invalid_id", "That budget id isn't valid.");
  const expected = Number(c.req.header("If-Match"));
  if (!Number.isInteger(expected) || expected < 0)
    throw new ApiError(428, "revision_required", "Say which revision this upload replaces (If-Match).");
  if (Number(c.req.header("Content-Length") ?? 0) > MAX_BYTES)
    throw new ApiError(413, "too_large", "This budget is larger than sync allows (50 MB).");
  const bytes = await c.req.arrayBuffer();
  if (!bytes.byteLength || bytes.byteLength > MAX_BYTES)
    throw new ApiError(bytes.byteLength ? 413 : 400, bytes.byteLength ? "too_large" : "empty", "The upload is empty or too large.");

  const current = await c.env.DB.prepare(
    "SELECT revision, object_key, size FROM sync_budgets WHERE user_id = ?1 AND budget_id = ?2",
  )
    .bind(userId, budgetId)
    .first<{ revision: number; object_key: string; size: number }>();
  if ((current?.revision ?? 0) !== expected) {
    await countEvent(c.env.DB, "sync_upload", "conflict");
    throw new ApiError(409, "conflict", "Another device synced first.", { revision: current?.revision ?? 0 });
  }
  const totals = await c.env.DB.prepare("SELECT COUNT(*) AS budgets, COALESCE(SUM(size), 0) AS bytes FROM sync_budgets WHERE user_id = ?1")
    .bind(userId)
    .first<{ budgets: number; bytes: number }>();
  if (!current && (totals?.budgets ?? 0) >= MAX_BUDGETS)
    throw new ApiError(413, "too_many_budgets", `Sync keeps up to ${MAX_BUDGETS} budgets.`);
  if ((totals?.bytes ?? 0) - (current?.size ?? 0) + bytes.byteLength > MAX_TOTAL_BYTES)
    throw new ApiError(413, "quota", "Your synced budgets would pass the 1 GB limit.");

  // A fresh object per upload; the row only moves to it if nobody else won in the meantime.
  const objectKey = `sync/${userId}/${budgetId}/${randomId("v")}`;
  const meta = header(c.req.header("X-Sync-Meta"), 2048);
  const device = header(c.req.header("X-Device"), 80);
  const revision = expected + 1;
  let result: D1Result;
  try {
    await c.env.SYNC.put(objectKey, bytes);
    result = current
    ? await c.env.DB.prepare(
        `UPDATE sync_budgets SET revision = ?3, object_key = ?4, size = ?5, meta = ?6, device = ?7, updated_at = ?8
         WHERE user_id = ?1 AND budget_id = ?2 AND revision = ?9
           AND EXISTS (SELECT 1 FROM sync_keys WHERE user_id = ?1 AND generation = ?10)
           AND (SELECT COALESCE(SUM(size), 0) FROM sync_budgets WHERE user_id = ?1) - size + ?5 <= ?11`,
      )
        .bind(userId, budgetId, revision, objectKey, bytes.byteLength, meta, device, now(), expected, generation, MAX_TOTAL_BYTES)
        .run()
    : await c.env.DB.prepare(
        `INSERT INTO sync_budgets (user_id, budget_id, revision, object_key, size, meta, device, updated_at)
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8
         WHERE EXISTS (SELECT 1 FROM sync_keys WHERE user_id = ?1 AND generation = ?9)
           AND (SELECT COUNT(*) FROM sync_budgets WHERE user_id = ?1) < ?10
           AND (SELECT COALESCE(SUM(size), 0) FROM sync_budgets WHERE user_id = ?1) + ?5 <= ?11
         ON CONFLICT DO NOTHING`,
      )
        .bind(userId, budgetId, revision, objectKey, bytes.byteLength, meta, device, now(), generation, MAX_BUDGETS, MAX_TOTAL_BYTES)
        .run();
  } catch (error) {
    try { await discardObject(c.env, objectKey); } catch { /* Preserve bytes if database state cannot be checked. */ }
    throw error;
  }
  if (!result.meta.changes) {
    await discardObject(c.env, objectKey);
    await assertGeneration(c, generation);
    const latest = await c.env.DB.prepare("SELECT revision FROM sync_budgets WHERE user_id = ?1 AND budget_id = ?2")
      .bind(userId, budgetId)
      .first<{ revision: number }>();
    if ((latest?.revision ?? 0) === expected) {
      const totals = await c.env.DB.prepare("SELECT COUNT(*) AS budgets, COALESCE(SUM(size), 0) AS bytes FROM sync_budgets WHERE user_id = ?1")
        .bind(userId).first<{ budgets: number; bytes: number }>();
      if (!current && (totals?.budgets ?? 0) >= MAX_BUDGETS)
        throw new ApiError(413, "too_many_budgets", `Sync keeps up to ${MAX_BUDGETS} budgets.`);
      throw new ApiError(413, "quota", "Your synced budgets would pass the 1 GB limit.");
    }
    await countEvent(c.env.DB, "sync_upload", "conflict");
    throw new ApiError(409, "conflict", "Another device synced first.", { revision: latest?.revision ?? 0 });
  }
  if (current) await cleanObjects(c.env, [current.object_key]);
  await countEvent(c.env.DB, "sync_upload", "ok");
  return c.json({ revision });
});

sync.delete("/budgets/:id", async (c) => {
  const generation = await generationFor(c);
  const row = await c.env.DB.prepare(
    `DELETE FROM sync_budgets WHERE user_id = ?1 AND budget_id = ?2
     AND EXISTS (SELECT 1 FROM sync_keys WHERE user_id = ?1 AND generation = ?3) RETURNING object_key`,
  )
    .bind(c.get("user").id, c.req.param("id"), generation)
    .first<{ object_key: string }>();
  if (row) await cleanObjects(c.env, [row.object_key]);
  else await assertGeneration(c, generation);
  return c.json({ ok: true });
});
