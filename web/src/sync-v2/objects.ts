import type { Env } from "../env";
import { now, randomId } from "../util";
import { finish, headJson, statement } from "./repository";
import { MAX_COUNTER, RETENTION_MS } from "./protocol";

/** All publishers are fenced before a negative reference check can retire a key. */
export const objectReferences = (key: string) => `(EXISTS(SELECT 1 FROM sync_budgets WHERE object_key=${key})
 OR EXISTS(SELECT 1 FROM sync_heads_v2 WHERE object_key=${key})
 OR EXISTS(SELECT 1 FROM sync_staging_v2 s JOIN sync_objects_v2 reserved_object ON reserved_object.object_key=s.object_key
   WHERE s.object_key=${key} AND s.expires_at>$at AND reserved_object.state IN ('uploading','ready')))`;

export async function reclaimObjects(env: Env, keys: string[]) {
  const unique = [...new Set(keys)];
  for (let offset = 0; offset < unique.length; offset += 1000) {
    const p = { keys: JSON.stringify(unique.slice(offset, offset + 1000)), at: now() };
    try {
      const result = await env.DB.batch([
        statement(env.DB, `UPDATE sync_objects_v2 SET state='retired' WHERE object_key IN (SELECT value FROM json_each($keys))
          AND NOT ${objectReferences("sync_objects_v2.object_key")}`, p),
        // Remove protected queue entries in the same snapshot as the reference check.
        // A later reference removal can then enqueue fresh cleanup without being erased.
        statement(env.DB, `DELETE FROM sync_object_garbage WHERE object_key IN (SELECT value FROM json_each($keys))
          AND ${objectReferences("sync_object_garbage.object_key")}`, p),
        statement(env.DB, `SELECT value AS object_key FROM json_each($keys) WHERE NOT ${objectReferences("value")}`, p),
      ]);
      const retired = result[2].results.map(row => (row as { object_key: string }).object_key);
      if (!retired.length) continue;
      await env.SYNC.delete(retired);
      await statement(env.DB, "DELETE FROM sync_object_garbage WHERE object_key IN (SELECT value FROM json_each($keys))", { keys: JSON.stringify(retired) }).run();
      // A late upload cannot adopt a retired reservation; inventory catches its bytes.
    } catch { /* Durable queue/inventory retries partial storage or database outages. */ }
  }
}

export async function maintainSyncV2(env: Env) {
  const at = now();
  const pending = await statement(env.DB, "SELECT user_id,installation_id,sequence,digest FROM sync_staging_v2 WHERE expires_at<=$at LIMIT 100", { at }).all<{ user_id: string; installation_id: string; sequence: string; digest: string }>();
  for (const s of pending.results) {
    try { await finish(env, s.user_id, s.installation_id, s.sequence, s.digest, "expire"); } catch { /* Retry each durable reservation independently. */ }
  }
  const trash = await statement(env.DB, "SELECT user_id,budget_id,version FROM sync_heads_v2 WHERE state='trashed' AND retained_until<=$at LIMIT 100", { at }).all<{ user_id: string; budget_id: string; version: string }>();
  for (const h of trash.results) {
    const p = { user: h.user_id, budget: h.budget_id, version: h.version, at, marker: randomId("retention"), maxCounter: MAX_COUNTER.toString() };
    const applied = "EXISTS(SELECT 1 FROM sync_heads_v2 WHERE user_id=$user AND budget_id=$budget AND applied_by=$marker)";
    await env.DB.batch([
      statement(env.DB, `UPDATE sync_heads_v2 SET state='purged',object_key=NULL,payload=NULL,size=0,retained_until=NULL,
        version=CAST(CAST(version AS INTEGER)+1 AS TEXT),updated_at=$at,applied_by=$marker
        WHERE user_id=$user AND budget_id=$budget AND version=$version AND state='trashed' AND retained_until<=$at
        AND CAST(version AS INTEGER)<CAST($maxCounter AS INTEGER)
        AND (SELECT CAST(catalog_sequence AS INTEGER) FROM sync_accounts_v2 WHERE user_id=$user)<CAST($maxCounter AS INTEGER)`, p),
      statement(env.DB, `UPDATE sync_accounts_v2 SET catalog_sequence=CAST(CAST(catalog_sequence AS INTEGER)+1 AS TEXT) WHERE user_id=$user AND ${applied}`, p),
      statement(env.DB, `UPDATE sync_heads_v2 SET updated_sequence=(SELECT catalog_sequence FROM sync_accounts_v2 WHERE user_id=$user)
        WHERE user_id=$user AND budget_id=$budget AND applied_by=$marker`, p),
      statement(env.DB, `INSERT INTO sync_events_v2(user_id,sequence,epoch,event,created_at)
        SELECT $user,a.catalog_sequence,a.catalog_epoch,json_object('type','budget','head',json(${headJson()})),$at
        FROM sync_heads_v2 h JOIN sync_accounts_v2 a ON a.user_id=h.user_id WHERE h.user_id=$user AND h.budget_id=$budget AND h.applied_by=$marker`, p),
    ]);
  }
  const before = new Date(Date.now() - RETENTION_MS).toISOString();
  await env.DB.batch([
    statement(env.DB, `UPDATE sync_accounts_v2 AS a SET min_cursor=CAST(MAX(CAST(min_cursor AS INTEGER),COALESCE(
      (SELECT MAX(CAST(sequence AS INTEGER)) FROM sync_events_v2 e WHERE e.user_id=a.user_id AND e.created_at<$before),0)) AS TEXT)
      WHERE EXISTS(SELECT 1 FROM sync_events_v2 e WHERE e.user_id=a.user_id AND e.created_at<$before AND CAST(e.sequence AS INTEGER)>CAST(a.min_cursor AS INTEGER))`, { before }),
    statement(env.DB, `DELETE FROM sync_events_v2 WHERE CAST(sequence AS INTEGER)<=(SELECT CAST(min_cursor AS INTEGER) FROM sync_accounts_v2 WHERE user_id=sync_events_v2.user_id)`),
    // Expired ownership metadata is not a sequence fence: the installation and
    // tombstone retain that fence. No retry can recreate this random object key;
    // inventory still catches any very late R2 completion after ledger pruning.
    statement(env.DB, `DELETE FROM sync_objects_v2 WHERE object_key IN (SELECT object_key FROM sync_objects_v2 o
      WHERE state='retired' AND created_at<$before AND NOT ${objectReferences("o.object_key")} LIMIT 1000)`, { before, at }),
  ]);
  // R2 listing is paged; persist the position so a large bucket is eventually
  // scanned completely instead of rechecking only its first thousand objects.
  const cursor = await env.DB.prepare("SELECT value FROM sync_maintenance_v2 WHERE name='inventory_cursor'").first<string>("value");
  const page = await env.SYNC.list({ prefix: "sync-v2/", limit: 1000, ...(cursor ? { cursor } : {}) });
  const cutoff = Date.now() - 2 * 60 * 60_000;
  const candidates = page.objects.filter(object => object.uploaded.getTime() <= cutoff).map(object => object.key);
  if (candidates.length) await statement(env.DB, `INSERT OR IGNORE INTO sync_object_garbage(object_key)
    SELECT value FROM json_each($keys) WHERE NOT ${objectReferences("value")}`, { keys: JSON.stringify(candidates), at }).run();
  await statement(env.DB, `INSERT INTO sync_maintenance_v2(name,value) VALUES('inventory_cursor',$cursor)
    ON CONFLICT(name) DO UPDATE SET value=$cursor`, { cursor: page.truncated ? page.cursor : "" }).run();
}
