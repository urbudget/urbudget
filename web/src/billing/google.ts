// Google Play subscriptions to Sync, verified server-side. A client purchase state never grants access.
// The app itself is free, so the Sync subscription is the only Play product.
import { Hono } from "hono";
import { requireUser } from "../auth";
import type { AppEnv, Env, User } from "../env";
import { ApiError, base64url, fromBase64url, now, rateLimit, sha256, utf8 } from "../util";

export const PLAY_PACKAGE = "app.urbudget";
export const PLAY_SYNC = "urbudget_sync";
export const PLAY_BASE_PLAN = "monthly";
interface PurchaseRow {
  token_hash: string; token: string; user_id: string | null; account_id: string;
  product_id: string; kind: "sync"; state: string; expires_at: string | null;
  acknowledged: number; checked_at: string;
}
interface SubscriptionPurchase {
  subscriptionState?: string;
  acknowledgementState?: string;
  externalAccountIdentifiers?: { obfuscatedExternalAccountId?: string };
  linkedPurchaseToken?: string;
  lineItems?: { productId?: string; expiryTime?: string; autoRenewingPlan?: { autoRenewEnabled?: boolean } }[];
}
const unavailable = () => new ApiError(503, "play_unavailable", "Google Play purchases are not available yet. Please try again later.");
const badPurchase = () => new ApiError(400, "invalid_purchase", "Google Play could not verify this purchase for your account.");
// The public error remains deliberately indistinguishable from a bad receipt,
// but a local ownership change is not evidence that Google refunded a purchase.
class PurchaseOwnershipChanged extends ApiError {
  constructor() { super(400, "invalid_purchase", "Google Play could not verify this purchase for your account."); }
}
const tokens = new Map<string, { value: string; until: number }>();

export function playConfigured(env: Env): boolean {
  return Boolean(env.GOOGLE_PLAY_SERVICE_ACCOUNT);
}

const accessToken = (env: Env, signal?: AbortSignal) => serviceAccountToken(env, "https://www.googleapis.com/auth/androidpublisher", signal);

/**
 * An OAuth token for GOOGLE_PLAY_SERVICE_ACCOUNT with one scope, cached for most of its hour. Every
 * failure is the same vague error, so nothing from a purchase's URL can reach a log.
 */
export async function serviceAccountToken(env: Env, scope: string, signal?: AbortSignal): Promise<string> {
  if (!playConfigured(env)) throw unavailable();
  const secret = env.GOOGLE_PLAY_SERVICE_ACCOUNT!;
  const cacheKey = `${scope}\n${secret}`;
  const cached = tokens.get(cacheKey);
  if (cached && cached.until > Date.now()) return cached.value;
  let credentials: { client_email: string; private_key: string };
  try { credentials = JSON.parse(secret); } catch { throw unavailable(); }
  if (typeof credentials?.client_email !== "string" || !credentials.client_email.endsWith(".gserviceaccount.com") ||
      typeof credentials.private_key !== "string" || !credentials.private_key) throw unavailable();
  const seconds = Math.floor(Date.now() / 1000);
  const body = `${base64url(utf8(JSON.stringify({ alg: "RS256", typ: "JWT" })))}.${base64url(utf8(JSON.stringify({
    iss: credentials.client_email, scope,
    aud: "https://oauth2.googleapis.com/token", iat: seconds, exp: seconds + 3600,
  })))}`;
  const pem = credentials.private_key.replace(/-----[^-]+-----|\s/g, "");
  let key: CryptoKey;
  try {
    key = await crypto.subtle.importKey("pkcs8", fromBase64url(pem), { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["sign"]);
  } catch { throw unavailable(); }
  const signature = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", key, utf8(body));
  const response = await safeFetch("https://oauth2.googleapis.com/token", {
    method: "POST", signal: signal ?? AbortSignal.timeout(15000),
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer", assertion: `${body}.${base64url(signature)}` }),
  });
  if (!response.ok) throw unavailable();
  const answer = await safeJson(response) as { access_token?: string; expires_in?: number };
  if (!answer.access_token) throw unavailable();
  tokens.set(cacheKey, { value: answer.access_token, until: Date.now() + Math.min(3300, Math.max(0, (answer.expires_in ?? 3600) - 60)) * 1000 });
  return answer.access_token;
}

// Fetch errors may contain the requested URL, which includes a purchase token. Never propagate it.
async function safeFetch(url: string, init: RequestInit): Promise<Response> {
  try { return await fetch(url, init); } catch { throw unavailable(); }
}
async function safeJson(response: Response): Promise<unknown> {
  try { return await response.json(); } catch { throw unavailable(); }
}

async function google(env: Env, path: string, method = "GET", body?: unknown, signal?: AbortSignal): Promise<unknown> {
  const response = await safeFetch(`https://androidpublisher.googleapis.com/androidpublisher/v3/applications/${PLAY_PACKAGE}/purchases/${path}`, {
    method, signal: signal ?? AbortSignal.timeout(15000),
    headers: { Authorization: `Bearer ${await accessToken(env, signal)}`, "Content-Type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  // Do not log provider errors: URLs and response bodies can contain purchase tokens.
  if (response.status === 404 || response.status === 410) throw badPurchase();
  if (!response.ok) throw unavailable();
  return method === "GET" ? safeJson(response) : {};
}

export function subscriptionEntitlement(data: SubscriptionPurchase, accountId: string, at = Date.now()) {
  if (data.externalAccountIdentifiers?.obfuscatedExternalAccountId !== accountId || data.lineItems?.length !== 1 ||
      data.lineItems[0].productId !== PLAY_SYNC) throw badPurchase();
  const item = data.lineItems[0];
  const state = data.subscriptionState ?? "";
  const expiry = Date.parse(item.expiryTime ?? "");
  const active = ["SUBSCRIPTION_STATE_ACTIVE", "SUBSCRIPTION_STATE_IN_GRACE_PERIOD", "SUBSCRIPTION_STATE_CANCELED"].includes(state) && expiry > at;
  return { active, state, expires: Number.isFinite(expiry) ? new Date(expiry).toISOString() : null,
    acknowledged: data.acknowledgementState === "ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED",
    cancelAt: item.autoRenewingPlan?.autoRenewEnabled === false || state === "SUBSCRIPTION_STATE_CANCELED" ? item.expiryTime ?? null : null };
}

/** Persist ownership and entitlement before acknowledgement; all retries are idempotent. */
export async function verifyGooglePurchase(env: Env, user: Pick<User, "id">, token: string, signal?: AbortSignal) {
  const tokenHash = await sha256(token);
  const accountId = await sha256(`urbudget:${user.id}`);
  const existing = await env.DB.prepare("SELECT * FROM google_purchases WHERE token_hash = ?1").bind(tokenHash).first<PurchaseRow>();
  if (existing && (existing.user_id !== user.id || existing.kind !== "sync")) throw new PurchaseOwnershipChanged();
  const data = await google(env, `subscriptionsv2/tokens/${encodeURIComponent(token)}`, "GET", undefined, signal) as SubscriptionPurchase;
  const entitlement = subscriptionEntitlement(data, accountId);
  const linked = data.linkedPurchaseToken;
  if (linked) {
    const previous = await env.DB.prepare("SELECT user_id FROM google_purchases WHERE token_hash = ?1").bind(await sha256(linked)).first<{ user_id: string | null }>();
    if (previous && previous.user_id !== user.id) throw new PurchaseOwnershipChanged();
  }
  const at = now();
  const recorded = await env.DB.prepare(`INSERT INTO google_purchases (token_hash, token, user_id, account_id, product_id, kind, state, expires_at, acknowledged, checked_at, created_at)
    SELECT ?1, ?2, ?3, ?4, ?5, 'sync', ?6, ?7, ?8, ?9, ?9
    WHERE EXISTS (SELECT 1 FROM users WHERE id = ?3)
      AND NOT EXISTS (SELECT 1 FROM account_deletions WHERE user_id = ?3)
    ON CONFLICT(token_hash) DO UPDATE SET state = ?6, expires_at = ?7, acknowledged = ?8, checked_at = ?9, refresh_after = NULL
    WHERE user_id = ?3 AND kind = 'sync'
      AND NOT EXISTS (SELECT 1 FROM account_deletions WHERE user_id = ?3)`).bind(tokenHash, token, user.id, accountId, PLAY_SYNC,
    entitlement.state, entitlement.expires, Number(entitlement.acknowledged), at).run();
  if (!recorded.meta.changes) throw new PurchaseOwnershipChanged();
  const owner = await env.DB.prepare("SELECT user_id, kind FROM google_purchases WHERE token_hash = ?1").bind(tokenHash).first<PurchaseRow>();
  if (owner?.user_id !== user.id || owner.kind !== "sync") throw new PurchaseOwnershipChanged();
  const subscriptionId = `google:${tokenHash}`;
  const status = entitlement.active ? "active" : "canceled";
  const saved = await env.DB.prepare(`INSERT INTO subscriptions (id, user_id, customer_id, status, current_period_end, cancel_at, updated_at, event_at)
    SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7 WHERE EXISTS (SELECT 1 FROM users WHERE id = ?2)
      AND NOT EXISTS (SELECT 1 FROM account_deletions WHERE user_id = ?2)
    ON CONFLICT(id) DO UPDATE SET status = ?4, current_period_end = ?5, cancel_at = ?6, updated_at = ?7, event_at = ?7
    WHERE NOT EXISTS (SELECT 1 FROM account_deletions WHERE user_id = ?2)`)
    .bind(subscriptionId, user.id, `google:${user.id}`, status, entitlement.expires, entitlement.cancelAt, at).run();
  if (!saved.meta.changes) throw new PurchaseOwnershipChanged();
  if (linked && entitlement.active) await env.DB.prepare("UPDATE subscriptions SET status = 'canceled', current_period_end = ?2 WHERE id = ?1 AND user_id = ?3")
    .bind(`google:${await sha256(linked)}`, at, user.id).run();
  if (entitlement.active && !entitlement.acknowledged) {
    await google(env, `subscriptions/${PLAY_SYNC}/tokens/${encodeURIComponent(token)}:acknowledge`, "POST", {}, signal);
    await env.DB.prepare("UPDATE google_purchases SET acknowledged = 1 WHERE token_hash = ?1").bind(tokenHash).run();
  }
  // Keep ownership of a verified pending receipt so RTDN can finish a delayed payment even
  // while the app is closed. No active subscription is created for pending state.
  return { active: entitlement.active, pending: entitlement.state === "SUBSCRIPTION_STATE_PENDING" };
}

async function revokeKnownPurchase(env: Env, tokenHash: string) {
  await env.DB.batch([
    env.DB.prepare("UPDATE google_purchases SET state = 'UNAVAILABLE', checked_at = ?2 WHERE token_hash = ?1").bind(tokenHash, now()),
    env.DB.prepare("UPDATE subscriptions SET status = 'canceled' WHERE id = ?1").bind(`google:${tokenHash}`),
  ]);
}

async function refreshKnownPurchase(env: Env, row: PurchaseRow, signal?: AbortSignal) {
  try {
    if (row.user_id) {
      await verifyGooglePurchase(env, { id: row.user_id }, row.token, signal);
    } else {
      // Account deletion keeps the receipt, so no other account can claim it. Keep its state
      // current without creating an account, rebinding ownership or granting anything.
      const data = await google(env, `subscriptionsv2/tokens/${encodeURIComponent(row.token)}`, "GET", undefined, signal);
      const status = subscriptionEntitlement(data as SubscriptionPurchase, row.account_id);
      await env.DB.prepare("UPDATE google_purchases SET state = ?2, expires_at = ?3, checked_at = ?4, refresh_after = NULL WHERE token_hash = ?1")
        .bind(row.token_hash, status.state, status.expires, now()).run();
    }
  } catch (error) {
    if (error instanceof PurchaseOwnershipChanged) {
      // Deletion can detach the account after this refresh captured its owner.
      // Recheck as an unowned receipt without recreating the user or granting anything.
      const current = await env.DB.prepare("SELECT * FROM google_purchases WHERE token_hash = ?1 AND kind = 'sync'")
        .bind(row.token_hash).first<PurchaseRow>();
      if (current && current.user_id === null) await refreshKnownPurchase(env, current, signal);
    } else if (error instanceof ApiError && error.code === "invalid_purchase") await revokeKnownPurchase(env, row.token_hash);
    else throw error;
  }
}

/** A verified notification can refresh only a receipt already associated with an account. */
export async function refreshGooglePurchaseToken(env: Env, token: string) {
  const row = await env.DB.prepare("SELECT * FROM google_purchases WHERE token_hash = ?1 AND kind = 'sync'")
    .bind(await sha256(token)).first<PurchaseRow>();
  if (!row) return;
  if (!playConfigured(env)) throw unavailable();
  await refreshKnownPurchase(env, row);
}

// A database lease suppresses duplicate refreshes across Worker instances. The
// in-process promise also lets overlapping callers share a result without polling.
const refreshing = new WeakMap<D1Database, Map<string, Promise<void>>>();
async function refreshWithLease(env: Env, row: PurchaseRow, force: boolean, signal?: AbortSignal) {
  let tasks = refreshing.get(env.DB);
  if (!tasks) { tasks = new Map(); refreshing.set(env.DB, tasks); }
  const existing = tasks.get(row.token_hash);
  if (existing) {
    if (!signal) return existing;
    if (signal.aborted) return;
    // A foreground caller must not inherit a scheduler task's longer deadline.
    return new Promise<void>((resolve, reject) => {
      const finish = () => { signal.removeEventListener("abort", finish); resolve(); };
      signal.addEventListener("abort", finish, { once: true });
      existing.then(finish, (error) => { signal.removeEventListener("abort", finish); reject(error); });
    });
  }
  const work = (async () => {
    const at = now();
    const lease = new Date(Date.now() + 60_000).toISOString();
    const claimed = await env.DB.prepare(`UPDATE google_purchases SET refresh_after = ?2
      WHERE token_hash = ?1 AND (refresh_after IS NULL OR refresh_after <= ?3)
        AND (?4 = 1 OR checked_at < ?5) RETURNING token_hash`)
      .bind(row.token_hash, lease, at, Number(force), new Date(Date.now() - 5 * 60_000).toISOString()).first();
    if (!claimed) return;
    try {
      await refreshKnownPurchase(env, row, signal);
      // Terminal historical receipts need only a daily fallback. RTDN and an
      // explicit restore always verify directly, independent of this backoff.
      await env.DB.prepare(`UPDATE google_purchases SET refresh_after = CASE
        WHEN state IN ('SUBSCRIPTION_STATE_EXPIRED', 'SUBSCRIPTION_STATE_PENDING_PURCHASE_CANCELED', 'UNAVAILABLE') THEN ?3 ELSE NULL END
        WHERE token_hash = ?1 AND (refresh_after = ?2 OR refresh_after IS NULL)`)
        .bind(row.token_hash, lease, new Date(Date.now() + 86_400_000).toISOString()).run();
    } catch {
      // Keep the last verified entitlement and a one-minute retry backoff.
      // syncActive still enforces expiry even during a provider outage.
      await env.DB.prepare("UPDATE google_purchases SET refresh_after = ?3 WHERE token_hash = ?1 AND refresh_after = ?2")
        .bind(row.token_hash, lease, new Date(Date.now() + 60_000).toISOString()).run();
    }
  })();
  tasks.set(row.token_hash, work);
  try { await work; } finally { if (tasks.get(row.token_hash) === work) tasks.delete(row.token_hash); }
}

/** Bound foreground work; reconcile historical receipts in parallel on the scheduler. */
export async function refreshGooglePurchases(env: Env, userId?: string, force = false) {
  if (!playConfigured(env)) return;
  const cutoff = new Date(Date.now() - 5 * 60_000).toISOString();
  const foreground = userId !== undefined;
  const rows = await env.DB.prepare(`SELECT * FROM google_purchases
    WHERE kind = 'sync' AND (?1 IS NULL OR user_id = ?1) AND (?3 = 1 OR checked_at < ?2)
      AND (refresh_after IS NULL OR refresh_after <= ?4)
    ORDER BY CASE WHEN ?5 = 1 AND (expires_at > ?4 OR state = 'SUBSCRIPTION_STATE_PENDING') THEN 0 ELSE 1 END,
      checked_at LIMIT ?6`)
    .bind(userId ?? null, cutoff, Number(force), now(), Number(foreground), foreground ? 2 : 100)
    .all<PurchaseRow>();
  const signal = foreground ? AbortSignal.timeout(3000) : undefined;
  const parallel = foreground ? 2 : 4;
  for (let i = 0; i < rows.results.length; i += parallel)
    await Promise.all(rows.results.slice(i, i + parallel).map((row) => refreshWithLease(env, row, force, signal)));
}

export async function cancelGoogleSubscriptions(env: Env, userId: string) {
  // A paused/on-hold subscription can resume billing after its last paid period expired.
  // Pending payments may have no expiry yet. Refresh every receipt before deciding it is safe
  // to delete the account; cached expiry or cancellation state must not make that decision.
  const rows = await env.DB.prepare("SELECT token_hash, token, account_id FROM google_purchases WHERE user_id = ?1 AND kind = 'sync'")
    .bind(userId).all<Pick<PurchaseRow, "token_hash" | "token" | "account_id">>();
  for (const row of rows.results) {
    let data: SubscriptionPurchase;
    try {
      data = await google(env, `subscriptionsv2/tokens/${encodeURIComponent(row.token)}`) as SubscriptionPurchase;
    } catch (error) {
      // Google no longer recognizes this known receipt (404/410); it cannot renew.
      if (error instanceof ApiError && error.code === "invalid_purchase") continue;
      throw error;
    }
    const current = subscriptionEntitlement(data, row.account_id);
    if (["SUBSCRIPTION_STATE_CANCELED", "SUBSCRIPTION_STATE_EXPIRED", "SUBSCRIPTION_STATE_PENDING_PURCHASE_CANCELED"].includes(current.state)) continue;
    if (!["SUBSCRIPTION_STATE_ACTIVE", "SUBSCRIPTION_STATE_IN_GRACE_PERIOD", "SUBSCRIPTION_STATE_ON_HOLD", "SUBSCRIPTION_STATE_PAUSED", "SUBSCRIPTION_STATE_PENDING"].includes(current.state)) throw unavailable();
    try {
      await google(env, `subscriptionsv2/tokens/${encodeURIComponent(row.token)}:cancel`, "POST", { cancellationContext: { cancellationType: "DEVELOPER_REQUESTED_STOP_PAYMENTS" } });
    } catch {
      // In particular, if Google cannot cancel an awaiting-payment purchase, keep the account
      // so the user can resolve it in Play and retry. Never silently orphan a billable receipt.
      throw new ApiError(503, "play_cancel_failed", "Google Play could not stop a subscription or pending payment. Manage it in Google Play, then try deleting your account again. Your account has not been deleted.");
    }
    await env.DB.prepare("UPDATE google_purchases SET state = 'SUBSCRIPTION_STATE_CANCELED', checked_at = ?2 WHERE token_hash = ?1")
      .bind(row.token_hash, now()).run();
  }
}

const body = async (request: Request) => {
  const reader = request.body?.getReader();
  if (!reader) throw badPurchase();
  const parts: Uint8Array[] = []; let length = 0;
  while (true) {
    const chunk = await reader.read(); if (chunk.done) break;
    length += chunk.value.length;
    if (length > 8192) { await reader.cancel(); throw new ApiError(413, "too_large", "Purchase request is too large."); }
    parts.push(chunk.value);
  }
  const bytes = new Uint8Array(length); let offset = 0;
  for (const part of parts) { bytes.set(part, offset); offset += part.length; }
  try { return JSON.parse(new TextDecoder().decode(bytes)) as { token?: unknown; products?: unknown }; } catch { throw badPurchase(); }
};

export const googleBilling = new Hono<AppEnv>();
googleBilling.use("*", requireUser);
googleBilling.get("/context", async (c) => {
  if (!playConfigured(c.env)) throw unavailable();
  return c.json({ accountId: await sha256(`urbudget:${c.get("user").id}`), syncId: PLAY_SYNC, basePlanId: PLAY_BASE_PLAN });
});
googleBilling.post("/verify", async (c) => {
  const user = c.get("user");
  await rateLimit(c.env.DB, `google:${user.id}`, 240, 3600);
  const purchase = await body(c.req.raw);
  if (typeof purchase?.token !== "string" || purchase.token.length < 8 || purchase.token.length > 4096 ||
      !Array.isArray(purchase.products) || purchase.products.length !== 1) throw badPurchase();
  // The Sync subscription is the only Play product.
  if (purchase.products[0] !== PLAY_SYNC) throw badPurchase();
  return c.json(await verifyGooglePurchase(c.env, user, purchase.token));
});
