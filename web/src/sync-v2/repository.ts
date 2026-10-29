import type { Env } from "../env";
import { reclaimObjects } from "./objects";
import { SYNC_STATUSES } from "../billing/provider";
import { ApiError, now, randomId } from "../util";
import { canonical, hasBlob, isControl, MAX_BUDGETS, MAX_COUNTER, MAX_INSTALLATIONS, MAX_STAGING_BYTES, MAX_TOTAL_BYTES, RETENTION_MS, STAGING_MS, validId, type Manifest, type Receipt } from "./protocol";

type Params = Record<string, string | number | null>;
export function statement(db: D1Database, sql: string, values: Params = {}) {
  const names: string[] = [];
  const prepared = sql.replace(/\$(\w+)/g, (_, name: string) => {
    if (!(name in values)) throw new Error(`Missing SQL parameter ${name}`);
    if (!names.includes(name)) names.push(name);
    return `?${names.indexOf(name) + 1}`;
  });
  return db.prepare(prepared).bind(...names.map(name => values[name]));
}
export const headJson = (h = "h") => `json_object('budgetId',${h}.budget_id,'incarnation',${h}.incarnation,'version',${h}.version,
 'keyGeneration',${h}.key_generation,'state',${h}.state,'payload',json(${h}.payload),'updatedSequence',${h}.updated_sequence,
 'updatedAt',${h}.updated_at,'deletedAt',${h}.deleted_at,'retainedUntil',${h}.retained_until,'device',${h}.device)`;
export type Account = { user_id: string; protocol: number; key_generation: string | null; salt: string | null; key_check: string | null; catalog_epoch: string; catalog_sequence: string; min_cursor: string };
export type Installation = { last_sequence: string; last_digest: string | null; receipt: string | null; status: string };
export type Staging = { sequence: string; digest: string; manifest: string; object_key: string | null; expires_at: string; claim: string | null; decision: string | null };
export type MutationResult = { state: "terminal"; receipt: Receipt } | { state: "prepared"; sequence: string; digest: string; expiresAt: string; blobReady: boolean };
export function requirePublication(env: Env, kind: Manifest["kind"]) {
  if (env.SYNC_V2_PUBLICATION_PAUSED === "1" && (hasBlob(kind) || isControl(kind)))
    throw new ApiError(503, "publication_paused", "Cloud publication is temporarily paused. Saved local work and pending sync will resume when service returns.");
}
export async function account(db: D1Database, user: string) {
  return statement(db, "SELECT * FROM sync_accounts_v2 WHERE user_id=$user", { user }).first<Account>();
}
export async function installation(db: D1Database, user: string, id: string) {
  return statement(db, "SELECT * FROM sync_installations_v2 WHERE user_id=$user AND installation_id=$id", { user, id }).first<Installation>();
}
export async function staged(db: D1Database, user: string, id: string) {
  return statement(db, "SELECT * FROM sync_staging_v2 WHERE user_id=$user AND installation_id=$id", { user, id }).first<Staging>();
}
export async function requireProtocolAccess(env: Env, user: string) {
  if (env.SYNC_V2_ENABLED !== "1" && (await account(env.DB, user))?.protocol !== 2)
    throw new ApiError(409, "activation_disabled", "Sync v2 is not enabled for this account.");
}
export async function register(env: Env, user: string, id: string) {
  await requireProtocolAccess(env, user);
  const p = { user, id, at: now(), epoch: randomId("epoch"), limit: MAX_INSTALLATIONS };
  await env.DB.batch([
    statement(env.DB, `INSERT INTO sync_accounts_v2(user_id,key_generation,salt,key_check,catalog_epoch,updated_at)
      SELECT $user,(SELECT CAST(generation AS TEXT) FROM sync_keys WHERE user_id=$user),
       (SELECT salt FROM sync_keys WHERE user_id=$user),(SELECT key_check FROM sync_keys WHERE user_id=$user),$epoch,$at
      WHERE EXISTS(SELECT 1 FROM users WHERE id=$user) ON CONFLICT(user_id) DO NOTHING`, p),
    statement(env.DB, `INSERT INTO sync_installations_v2(user_id,installation_id,created_at)
      SELECT $user,$id,$at WHERE (SELECT COUNT(*) FROM sync_installations_v2 WHERE user_id=$user AND status='active')<$limit
        AND NOT EXISTS(SELECT 1 FROM account_deletions WHERE user_id=$user)
      ON CONFLICT(user_id,installation_id) DO NOTHING`, p),
  ]);
  const row = await installation(env.DB, user, id);
  if (!row) throw new ApiError(409, "installation_limit", "The installation could not be registered. Check the account's device limit or deletion status.");
  if (row.status !== "active") throw new ApiError(409, "installation_retired", "This installation identity is retired. Reconcile local data before registering a new identity.");
  return row;
}
export function checkSequence(row: Installation | null, sequence: string, digest: string): Receipt | null {
  if (!row) throw new ApiError(404, "installation_required", "Register this installation first.");
  if (sequence === row.last_sequence && row.receipt) {
    if (digest !== row.last_digest) throw new ApiError(409, "sequence_reused", "This sequence was used for different content.");
    return JSON.parse(row.receipt) as Receipt;
  }
  if (row.status !== "active") throw new ApiError(409, "installation_retired", "The installation is retired.");
  if (BigInt(sequence) <= BigInt(row.last_sequence)) throw new ApiError(409, "stale_sequence", "This local state is older than the installation's receipt. Preserve it and reconcile.", { lastSequence: row.last_sequence });
  if (BigInt(sequence) !== BigInt(row.last_sequence) + 1n) throw new ApiError(409, "sequence_gap", "Resolve the earlier installation mutation first.", { lastSequence: row.last_sequence });
  return null;
}

const entitlement = `EXISTS(SELECT 1 FROM subscriptions WHERE user_id=$user AND status IN (${SYNC_STATUSES.map(s => `'${s}'`).join(",")})
 AND ((id NOT LIKE 'google:%' AND id NOT LIKE 'apple:%') OR current_period_end > $at))`;
const claimed = `EXISTS(SELECT 1 FROM sync_staging_v2 s WHERE s.user_id=$user AND s.installation_id=$id AND s.sequence=$seq AND s.digest=$digest AND s.claim=$claim)`;
const readyClaim = `${claimed} AND EXISTS(SELECT 1 FROM sync_staging_v2 s WHERE s.user_id=$user AND s.installation_id=$id AND s.decision='ok')`;
const appliedHead = `EXISTS(SELECT 1 FROM sync_heads_v2 WHERE user_id=$user AND budget_id=$budget AND applied_by=$marker)`;
const appliedAccount = `EXISTS(SELECT 1 FROM sync_accounts_v2 WHERE user_id=$user AND applied_by=$marker)`;

function parameters(env: Env, user: string, id: string, seq: string, digest: string, m: Manifest, claim: string): Params {
  return { user, id, seq, previous: (BigInt(seq) - 1n).toString(), digest, claim, marker: `${id}:${seq}`, at: now(),
    generation: m.keyGeneration, epoch: m.catalogEpoch, budget: m.budgetId ?? "", incarnation: m.incarnation ?? "",
    version: m.expectedVersion ?? "0", nextIncarnation: m.newIncarnation ?? m.incarnation ?? "", kind: m.kind,
    bytes: m.payload?.length ?? 0, payload: m.payload ? canonical(m.payload) : null, sha: m.payload?.sha256 ?? null,
    expectedProtocol: m.expectedProtocol ?? 2, salt: m.key?.salt ?? null, check: m.key?.check ?? null,
    sourceSha: m.source?.sha256 ?? null, enabled: env.SYNC_V2_ENABLED === "1" ? 1 : 0,
    maxBudgets: MAX_BUDGETS, maxBytes: MAX_TOTAL_BYTES, maxStaging: MAX_STAGING_BYTES, maxCounter: MAX_COUNTER.toString(),
    retained: new Date(Date.now() + RETENTION_MS).toISOString(), nextEpoch: randomId("epoch"),
  };
}
/** Evaluated INSIDE the batch, against the same serialized state as the mutation. */
function decision(m: Manifest, commit: boolean) {
  const scope = `CASE WHEN a.protocol=1 THEN (SELECT CAST(generation AS TEXT) FROM sync_keys WHERE user_id=$user) ELSE a.key_generation END`;
  let checks = `WHEN EXISTS(SELECT 1 FROM account_deletions WHERE user_id=$user) THEN 'account_deleting'
    WHEN s.expires_at <= $at THEN 'expired'
    WHEN ${scope} IS NOT $generation THEN 'invalidated_generation'`;
  if (isControl(m.kind)) {
    checks += ` WHEN a.protocol <> $expectedProtocol THEN 'protocol_changed'`;
    if (m.kind === "keySetup") checks += ` WHEN $enabled=0 THEN 'activation_disabled' WHEN a.protocol<>1 OR ${scope} IS NOT NULL THEN 'key_exists'`;
    if (m.kind === "migrateProtocol") checks += ` WHEN $enabled=0 THEN 'activation_disabled' WHEN a.protocol<>1 THEN 'protocol_changed' WHEN ${scope} IS NULL THEN 'no_key'`;
    if (m.kind === "keyReset") checks += ` WHEN a.protocol<>2 THEN 'upgrade_required' WHEN a.key_generation IS NULL THEN 'no_key' WHEN a.catalog_epoch IS NOT $epoch THEN 'epoch_changed' WHEN CAST(a.key_generation AS INTEGER)>=CAST($maxCounter AS INTEGER) THEN 'counter_exhausted'`;
  } else {
    checks += ` WHEN a.protocol<>2 THEN 'upgrade_required' WHEN a.catalog_epoch IS NOT $epoch THEN 'epoch_changed'
      WHEN CAST(a.catalog_sequence AS INTEGER)>=CAST($maxCounter AS INTEGER) THEN 'counter_exhausted'
      WHEN EXISTS(SELECT 1 FROM sync_heads_v2 WHERE user_id=$user AND budget_id=$budget AND CAST(version AS INTEGER)>=CAST($maxCounter AS INTEGER)) THEN 'counter_exhausted'`;
    if (m.kind === "create") checks += ` WHEN EXISTS(SELECT 1 FROM sync_heads_v2 WHERE user_id=$user AND budget_id=$budget) THEN 'identity_exists'`;
    else checks += ` WHEN NOT EXISTS(SELECT 1 FROM sync_heads_v2 h WHERE h.user_id=$user AND h.budget_id=$budget
      AND h.key_generation=$generation AND h.incarnation=$incarnation AND h.version=$version
      AND h.state='${m.kind === "restore" || m.kind === "purge" ? "trashed" : "active"}') THEN 'conflict'`;
    if (m.kind === "restore") checks += ` WHEN NOT EXISTS(SELECT 1 FROM sync_heads_v2 h WHERE h.user_id=$user AND h.budget_id=$budget AND h.object_key IS NOT NULL AND h.retained_until>$at AND json_extract(h.payload,'$.sha256') IS $sourceSha) THEN 'source_missing'`;
    if (hasBlob(m.kind)) {
      checks += ` WHEN NOT ${entitlement} THEN 'subscription_required'
        WHEN (SELECT COALESCE(SUM(size),0) FROM sync_heads_v2 WHERE user_id=$user)
          - COALESCE((SELECT size FROM sync_heads_v2 WHERE user_id=$user AND budget_id=$budget),0)+$bytes>$maxBytes THEN 'quota'`;
      if (m.kind !== "update") checks += ` WHEN (SELECT COUNT(*) FROM sync_heads_v2 WHERE user_id=$user AND state='active') >= $maxBudgets THEN 'too_many_budgets'`;
      checks += ` WHEN (SELECT COALESCE(SUM(size),0) FROM sync_objects_v2 WHERE user_id=$user AND state IN ('uploading','ready'))${commit ? '' : '+$bytes'}>$maxStaging THEN 'staging_quota'`;
      if (commit) checks += ` WHEN NOT EXISTS(SELECT 1 FROM sync_objects_v2 o WHERE o.object_key=s.object_key AND o.state='ready' AND o.expires_at>$at AND o.size=$bytes AND o.sha256=$sha AND o.digest=$digest) THEN 'blob_required'`;
    }
  }
  return `UPDATE sync_staging_v2 AS s SET decision=(SELECT CASE ${checks} ELSE 'ok' END FROM sync_accounts_v2 a WHERE a.user_id=$user)
    WHERE s.user_id=$user AND s.installation_id=$id AND s.claim=$claim`;
}

function receiptSql(applied: string) {
  return `UPDATE sync_installations_v2 AS i SET last_sequence=$seq,last_digest=$digest,
    receipt=(SELECT json_object('sequence',$seq,'digest',$digest,'operationId',json_extract(s.manifest,'$.operationId'),
      'kind',json_extract(s.manifest,'$.kind'),'outcome',CASE WHEN s.decision='ok' AND ${applied} THEN 'accepted'
        WHEN s.decision='cancelled' THEN 'cancelled' WHEN s.decision='expired' THEN 'expired' ELSE 'rejected' END,
      'code',CASE WHEN s.decision='ok' AND ${applied} THEN 'accepted' WHEN s.decision='ok' THEN 'conflict' ELSE s.decision END,
      'accountId',$user,'keyGeneration',a.key_generation,'catalogEpoch',a.catalog_epoch,'protocol',a.protocol,
      'budget',json((SELECT ${headJson()} FROM sync_heads_v2 h WHERE h.user_id=$user AND h.budget_id=$budget)))
      FROM sync_staging_v2 s JOIN sync_accounts_v2 a ON a.user_id=s.user_id WHERE s.user_id=$user AND s.installation_id=$id)
    WHERE i.user_id=$user AND i.installation_id=$id AND i.last_sequence=$previous AND ${claimed}`;
}
function finalize(db: D1Database, p: Params, applied: string, prepare = false) {
  const terminal = prepare ? " AND (SELECT decision FROM sync_staging_v2 WHERE user_id=$user AND installation_id=$id)<>'ok'" : " AND (SELECT decision FROM sync_staging_v2 WHERE user_id=$user AND installation_id=$id)<>'blob_required'";
  return [statement(db, receiptSql(applied) + terminal, p),
    statement(db, `DELETE FROM sync_staging_v2 WHERE user_id=$user AND installation_id=$id AND claim=$claim
      AND EXISTS(SELECT 1 FROM sync_installations_v2 WHERE user_id=$user AND installation_id=$id AND last_sequence=$seq AND last_digest=$digest)`, p),
    statement(db, `UPDATE sync_staging_v2 SET claim=NULL,decision=NULL WHERE user_id=$user AND installation_id=$id AND claim=$claim`, p)];
}
/** An unprepared slot can be cancelled even if its payload is too large or no
 * longer valid. No object, key, budget or catalog mutation is evaluated here. */
export async function cancelUnprepared(env: Env, user: string, id: string, seq: string, digest: string, manifest: Record<string, unknown>) {
  const old = checkSequence(await installation(env.DB, user, id), seq, digest);
  if (old) return { state: "terminal" as const, receipt: old };
  const p = { user, id, seq, previous: (BigInt(seq) - 1n).toString(), digest, manifest: canonical(manifest),
    claim: randomId("cancel"), expiry: now(), budget: validId(manifest.budgetId) ? manifest.budgetId : "" };
  await env.DB.batch([
    statement(env.DB, `INSERT INTO sync_staging_v2(user_id,installation_id,sequence,digest,manifest,expires_at,claim,decision)
      SELECT $user,$id,$seq,$digest,$manifest,$expiry,$claim,'cancelled' FROM sync_installations_v2
      WHERE user_id=$user AND installation_id=$id AND last_sequence=$previous AND status='active'
      ON CONFLICT(user_id,installation_id) DO NOTHING`, p),
    ...finalize(env.DB, p, "0"),
  ]);
  return mutationState(env, user, id, seq, digest);
}

export async function prepare(env: Env, user: string, id: string, seq: string, digest: string, m: Manifest) {
  const old = checkSequence(await installation(env.DB, user, id), seq, digest);
  if (old) return { state: "terminal" as const, receipt: old };
  const existing = await staged(env.DB, user, id);
  if (existing && (existing.digest !== digest || existing.sequence !== seq)) throw new ApiError(409, "pending_mutation", "Resolve the installation's existing pending manifest first.");
  if (existing) return mutationState(env, user, id, seq, digest);
  const p = { ...parameters(env, user, id, seq, digest, m, randomId("claim")), manifest: canonical(m),
    object: hasBlob(m.kind) ? `sync-v2/${user}/${id}/${seq}/${randomId("object")}` : null,
    expiry: new Date(Date.now() + STAGING_MS).toISOString() };
  await env.DB.batch([
    statement(env.DB, `INSERT INTO sync_staging_v2(user_id,installation_id,sequence,digest,manifest,object_key,expires_at,claim)
      SELECT $user,$id,$seq,$digest,$manifest,NULL,$expiry,$claim FROM sync_installations_v2
      WHERE user_id=$user AND installation_id=$id AND last_sequence=$previous AND status='active'
      ON CONFLICT(user_id,installation_id) DO NOTHING`, p),
    statement(env.DB, decision(m, false), p),
    ...(hasBlob(m.kind) ? [statement(env.DB, `INSERT INTO sync_objects_v2(object_key,user_id,installation_id,sequence,digest,sha256,size,state,created_at,expires_at)
      SELECT $object,$user,$id,$seq,$digest,$sha,$bytes,'uploading',$at,$expiry WHERE ${readyClaim}`, p),
      statement(env.DB, `UPDATE sync_staging_v2 SET object_key=$object WHERE user_id=$user AND installation_id=$id AND claim=$claim AND decision='ok'`, p)] : []),
    ...finalize(env.DB, p, "0", true),
  ]);
  return mutationState(env, user, id, seq, digest);
}

function budgetWrites(db: D1Database, m: Manifest, p: Params) {
  const object = `(SELECT object_key FROM sync_staging_v2 WHERE user_id=$user AND installation_id=$id)`;
  const statements: D1PreparedStatement[] = [];
  if (m.kind === "create") statements.push(statement(db, `INSERT INTO sync_heads_v2(user_id,budget_id,incarnation,version,key_generation,state,object_key,payload,size,updated_sequence,updated_at,device,applied_by)
    SELECT $user,$budget,$nextIncarnation,'1',$generation,'active',${object},$payload,$bytes,'0',$at,$id,$marker
    WHERE ${readyClaim} ON CONFLICT(user_id,budget_id) DO NOTHING`, p));
  else statements.push(statement(db, `UPDATE sync_heads_v2 SET version=CAST(CAST(version AS INTEGER)+1 AS TEXT),applied_by=$marker,updated_at=$at,device=$id,
    ${hasBlob(m.kind) ? `incarnation=$nextIncarnation,state='active',object_key=${object},payload=$payload,size=$bytes,deleted_at=NULL,retained_until=NULL` : m.kind === "trash" ? "state='trashed',deleted_at=$at,retained_until=$retained" : "state='purged',object_key=NULL,payload=NULL,size=0,retained_until=NULL"}
    WHERE user_id=$user AND budget_id=$budget AND version=$version AND incarnation=$incarnation AND key_generation=$generation AND ${readyClaim}`, p));
  statements.push(
    statement(db, `UPDATE sync_accounts_v2 SET catalog_sequence=CAST(CAST(catalog_sequence AS INTEGER)+1 AS TEXT),updated_at=$at WHERE user_id=$user AND ${readyClaim} AND ${appliedHead}`, p),
    statement(db, `UPDATE sync_heads_v2 SET updated_sequence=(SELECT catalog_sequence FROM sync_accounts_v2 WHERE user_id=$user)
      WHERE user_id=$user AND budget_id=$budget AND applied_by=$marker AND ${readyClaim}`, p),
    statement(db, `INSERT INTO sync_events_v2(user_id,sequence,epoch,event,created_at)
      SELECT $user,a.catalog_sequence,a.catalog_epoch,json_object('type','budget','head',json(${headJson()})),$at
      FROM sync_accounts_v2 a JOIN sync_heads_v2 h ON h.user_id=a.user_id AND h.budget_id=$budget
      WHERE a.user_id=$user AND h.applied_by=$marker AND ${readyClaim}`, p),
    statement(db, `UPDATE sync_objects_v2 SET state='adopted',expires_at=NULL WHERE object_key=${object} AND ${readyClaim} AND ${appliedHead}`, p),
  );
  return statements;
}

function controlWrites(db: D1Database, m: Manifest, p: Params) {
  const statements: D1PreparedStatement[] = [];
  if (m.kind === "migrateProtocol") {
    statements.push(statement(db, `UPDATE sync_accounts_v2 SET protocol=2,key_generation=(SELECT CAST(generation AS TEXT) FROM sync_keys WHERE user_id=$user),
      salt=(SELECT salt FROM sync_keys WHERE user_id=$user),key_check=(SELECT key_check FROM sync_keys WHERE user_id=$user),
      catalog_epoch=$nextEpoch,catalog_sequence='1',min_cursor='0',updated_at=$at,applied_by=$marker,legacy_cleanup=1
      WHERE user_id=$user AND protocol=1 AND ${readyClaim}`, p),
      statement(db, `INSERT OR IGNORE INTO sync_objects_v2(object_key,user_id,size,state,created_at)
        SELECT object_key,$user,size,'adopted',$at FROM sync_budgets WHERE user_id=$user AND ${readyClaim} AND ${appliedAccount}`, p),
      statement(db, `INSERT INTO sync_heads_v2(user_id,budget_id,incarnation,version,key_generation,state,object_key,payload,size,updated_sequence,updated_at,device,applied_by)
        SELECT b.user_id,b.budget_id,lower(hex(randomblob(16))),CAST(b.revision AS TEXT),a.key_generation,'active',b.object_key,
          json_object('format','legacy-v1','sha256',NULL,'length',b.size,'meta',b.meta,'context',json_object('budgetId',b.budget_id)),
          b.size,'1',b.updated_at,b.device,$marker FROM sync_budgets b JOIN sync_accounts_v2 a ON a.user_id=b.user_id
        WHERE b.user_id=$user AND ${readyClaim} AND ${appliedAccount}`, p));
  } else {
    statements.push(statement(db, `UPDATE sync_accounts_v2 SET protocol=2,key_generation=${m.kind === "keySetup" ? "'0'" : "CAST(CAST(key_generation AS INTEGER)+1 AS TEXT)"},
      salt=$salt,key_check=$check,catalog_epoch=$nextEpoch,catalog_sequence='1',min_cursor='0',updated_at=$at,applied_by=$marker,legacy_cleanup=1
      WHERE user_id=$user AND ${readyClaim}`, p));
    if (m.kind === "keyReset") {
      statements.push(statement(db, `DELETE FROM sync_heads_v2 WHERE user_id=$user AND ${readyClaim} AND ${appliedAccount}`, p));
      // Other reserved slots get a terminal receipt rather than being stranded by
      // the new generation. Their last completed receipts otherwise remain intact.
      statements.push(statement(db, `UPDATE sync_installations_v2 AS i SET
        last_sequence=(SELECT sequence FROM sync_staging_v2 s WHERE s.user_id=i.user_id AND s.installation_id=i.installation_id),
        last_digest=(SELECT digest FROM sync_staging_v2 s WHERE s.user_id=i.user_id AND s.installation_id=i.installation_id),
        receipt=(SELECT json_object('sequence',s.sequence,'digest',s.digest,'operationId',json_extract(s.manifest,'$.operationId'),'kind',json_extract(s.manifest,'$.kind'),
          'outcome','rejected','code','invalidated_generation','accountId',$user,'keyGeneration',a.key_generation,'catalogEpoch',a.catalog_epoch,'protocol',2,'budget',NULL)
          FROM sync_staging_v2 s JOIN sync_accounts_v2 a ON a.user_id=s.user_id WHERE s.user_id=i.user_id AND s.installation_id=i.installation_id)
        WHERE i.user_id=$user AND i.installation_id<>$id AND EXISTS(SELECT 1 FROM sync_staging_v2 s WHERE s.user_id=i.user_id AND s.installation_id=i.installation_id)
          AND ${readyClaim} AND ${appliedAccount}`, p),
      statement(db, `DELETE FROM sync_staging_v2 WHERE user_id=$user AND installation_id<>$id AND ${readyClaim} AND ${appliedAccount}`, p));
    }
  }
  statements.push(statement(db, `DELETE FROM sync_events_v2 WHERE user_id=$user AND ${readyClaim} AND ${appliedAccount}`, p),
    statement(db, `INSERT INTO sync_events_v2(user_id,sequence,epoch,event,created_at)
      SELECT $user,catalog_sequence,catalog_epoch,json_object('type','bootstrap','keyGeneration',key_generation,'protocol',protocol),$at
      FROM sync_accounts_v2 WHERE user_id=$user AND ${readyClaim} AND ${appliedAccount}`, p),
    // Frozen v1 heads must not indefinitely pin every superseded v2 object. The
    // new references already exist in this transaction before these are removed.
    statement(db, `DELETE FROM sync_budgets WHERE user_id=$user AND ${readyClaim} AND ${appliedAccount}`, p),
    statement(db, `UPDATE sync_accounts_v2 SET legacy_cleanup=0 WHERE user_id=$user AND applied_by=$marker AND ${readyClaim}`, p));
  return statements;
}

export async function finish(env: Env, user: string, id: string, seq: string, digest: string, mode: "commit" | "cancel" | "expire" = "commit"): Promise<MutationResult> {
  const old = checkSequence(await installation(env.DB, user, id), seq, digest);
  if (old) return { state: "terminal" as const, receipt: old };
  const s = await staged(env.DB, user, id);
  if (!s) throw new ApiError(409, "prepare_required", "Prepare this immutable mutation first.");
  if (s.sequence !== seq || s.digest !== digest) throw new ApiError(409, "pending_mutation", "The pending manifest is different.");
  const m = JSON.parse(s.manifest) as Manifest;
  if (mode === "commit") requirePublication(env, m.kind);
  const p = parameters(env, user, id, seq, digest, m, randomId("claim"));
  const applied = isControl(m.kind) ? appliedAccount : appliedHead;
  const commands = [statement(env.DB, `UPDATE sync_staging_v2 SET claim=$claim WHERE user_id=$user AND installation_id=$id AND sequence=$seq AND digest=$digest AND claim IS NULL
    AND EXISTS(SELECT 1 FROM sync_installations_v2 WHERE user_id=$user AND installation_id=$id AND last_sequence=$previous AND status='active')`, p)];
  if (mode === "commit") commands.push(statement(env.DB, decision(m, true), p), ...(isControl(m.kind) ? controlWrites(env.DB, m, p) : budgetWrites(env.DB, m, p)));
  else commands.push(statement(env.DB, `UPDATE sync_staging_v2 SET decision='${mode === "cancel" ? "cancelled" : "expired"}' WHERE user_id=$user AND installation_id=$id AND claim=$claim${mode === "expire" ? " AND expires_at<=$at" : ""}`, p));
  commands.push(...finalize(env.DB, p, applied));
  // Capture keys at the same serialized boundary as the mutation. Reclamation
  // checks fresh references again and cannot turn a committed receipt into failure.
  commands.unshift(statement(env.DB, `SELECT object_key FROM sync_heads_v2 WHERE user_id=$user AND object_key IS NOT NULL
    AND ($kind='keyReset' OR budget_id=$budget)
    UNION SELECT object_key FROM sync_staging_v2 WHERE user_id=$user AND object_key IS NOT NULL
    AND ($kind='keyReset' OR installation_id=$id)`, p));
  const results = await env.DB.batch(commands);
  await reclaimObjects(env, results[0].results.map(row => (row as { object_key: string }).object_key));
  return mutationState(env, user, id, seq, digest);
}

export async function mutationState(env: Env, user: string, id: string, seq: string, digest: string): Promise<MutationResult> {
  const row = await installation(env.DB, user, id);
  const receipt = checkSequence(row, seq, digest);
  if (receipt) return { state: "terminal" as const, receipt };
  const s = await staged(env.DB, user, id);
  if (!s || s.sequence !== seq || s.digest !== digest) throw new ApiError(409, "pending_mutation", "The pending mutation changed. Read the installation receipt.");
  if (s.expires_at <= now() && !s.claim) return finish(env, user, id, seq, digest, "expire");
  const blobReady = !s.object_key || !!await statement(env.DB, "SELECT 1 FROM sync_objects_v2 WHERE object_key=$key AND state='ready'", { key: s.object_key }).first();
  return { state: "prepared" as const, sequence: seq, digest, expiresAt: s.expires_at, blobReady };
}
