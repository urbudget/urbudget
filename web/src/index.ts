// urbudget.app: the static site (./public, served before this Worker runs) and the API.
import { Hono } from "hono";
import type { ContentfulStatusCode } from "hono/utils/http-status";
import { adminFor } from "./access";
import { admin } from "./admin";
import { assistantReports } from "./assistant-reports";
import { auth, requireUser } from "./auth";
import type { AppEnv, Env } from "./env";
import { billing, cancelSubscriptions } from "./billing";
import { COMPLIMENTARY } from "./billing/provider";
import { appleBilling, appleNotifications, settleAppleSubscriptions } from "./billing/apple";
import { googleBilling, refreshGooglePurchases } from "./billing/google";
import { googleNotifications } from "./billing/google-rtdn";
import { downloads, files, releases, TEAM_COOKIE } from "./releases";
import { visits } from "./visits";
import { usage } from "./usage";
import { fetchStore, snapshotGitHub } from "./channels";
import { cleanupSyncObjects, sync, syncActive, syncUsage } from "./sync";
import { syncV2 } from "./sync-v2";
import { maintainSyncV2, reclaimObjects } from "./sync-v2/objects";
import { ApiError, countEvent, jsonBody, now, randomId } from "./util";

const api = new Hono<AppEnv>();

api.get("/config", (c) => {
  c.header("Cache-Control", "public, max-age=300");
  return c.json({
    checkout: {
      ready: Boolean(c.env.POLAR_ACCESS_TOKEN && c.env.POLAR_PRODUCT_SYNC),
      environment: c.env.BILLING_ENV === "production" ? "production" : "sandbox",
    },
    // UrBudget itself is free and open source; Sync is the only thing for sale.
    prices: { sync: "$3" },
    supportEmail: c.env.SUPPORT_EMAIL,
  });
});

api.get("/me", requireUser, async (c) => {
  const user = c.get("user");
  await refreshGooglePurchases(c.env, user.id);
  await settleAppleSubscriptions(c.env, user.id);
  const [subscription, synced] = await Promise.all([
    c.env.DB.prepare(
      `SELECT id, status, current_period_end, cancel_at FROM subscriptions
       WHERE user_id = ?1 ORDER BY (status IN ('active', 'trialing', 'past_due')) DESC, updated_at DESC LIMIT 1`,
    )
      .bind(user.id)
      .first<{ id: string; status: string; current_period_end: string | null; cancel_at: string | null }>(),
    syncUsage(c.env.DB, user.id),
  ]);
  return c.json({
    user: { id: user.id, email: user.email, createdAt: user.created_at },
    subscription: subscription && {
      provider: subscription.id.startsWith("google:") ? "google" : subscription.id.startsWith("apple:") ? "apple"
        : subscription.id.startsWith(COMPLIMENTARY) ? "comp" : "polar",
      status: subscription.status,
      renewsAt: subscription.cancel_at || subscription.status === "canceled" ? null : subscription.current_period_end,
      endsAt: subscription.cancel_at ?? (subscription.status === "canceled" ? subscription.current_period_end : null),
    },
    sync: {
      active: await syncActive(c.env.DB, user.id),
      budgets: synced?.budgets ?? 0,
      bytes: synced?.bytes ?? 0,
    },
  });
});

api.delete("/me", requireUser, async (c) => {
  const user = c.get("user");
  const body = await jsonBody(c.req.raw);
  if (typeof body.confirm !== "string" || body.confirm.trim().toLowerCase() !== user.email)
    throw new ApiError(400, "confirm_email", "Type your account's email address to confirm.");
  // Freeze new recurring attachments before taking cancellation snapshots. Failed
  // attempts release the guard; interrupted attempts can be reclaimed after five
  // minutes, while billing writes stay blocked until an attempt finishes.
  const token = randomId("delete");
  const claimed = await c.env.DB.prepare(`INSERT INTO account_deletions (user_id, token, started_at)
    VALUES (?1, ?2, ?3) ON CONFLICT(user_id) DO UPDATE SET token = ?2, started_at = ?3
    WHERE started_at < ?4 RETURNING token`)
    .bind(user.id, token, now(), new Date(Date.now() - 5 * 60_000).toISOString()).first();
  if (!claimed) throw new ApiError(409, "account_deleting", "Account deletion is already in progress. Try again shortly.");
  try {
    // If a payment provider cannot be reached, keep the account and cloud copies.
    await cancelSubscriptions(c.env, user.id);
    if (!await c.env.DB.prepare("SELECT 1 FROM account_deletions WHERE user_id = ?1 AND token = ?2").bind(user.id, token).first())
      throw new ApiError(409, "account_deleting", "Another account deletion attempt is in progress. Try again shortly.");
    // Capture legacy keys before their cascading rows disappear; v2's ownership
    // ledger survives deletion and can be drained directly regardless of backlog.
    const legacyObjects = await c.env.DB.prepare("SELECT object_key FROM sync_budgets WHERE user_id=?1").bind(user.id).all<{ object_key: string }>();
    // The user cascade removes sync references in this same transaction. If the
    // commit fails or this attempt loses its lease, cloud copies stay intact.
    const results = await c.env.DB.batch([
      c.env.DB.prepare(`INSERT OR IGNORE INTO deleted_billing_customers (customer_id, deleted_at)
        SELECT billing_customer_id, ?2 FROM users WHERE id = ?1 AND billing_customer_id IS NOT NULL
          AND EXISTS (SELECT 1 FROM account_deletions WHERE user_id = ?1 AND token = ?3)
        UNION SELECT customer_id, ?2 FROM subscriptions WHERE user_id = ?1 AND id NOT LIKE 'google:%' AND id NOT LIKE 'apple:%'
          AND id NOT LIKE 'comp:%'
          AND EXISTS (SELECT 1 FROM account_deletions WHERE user_id = ?1 AND token = ?3)`)
        .bind(user.id, now(), token),
      c.env.DB.prepare(`DELETE FROM users WHERE id = ?1
        AND EXISTS (SELECT 1 FROM account_deletions WHERE user_id = ?1 AND token = ?2)`)
        .bind(user.id, token),
    ]);
    if (!results[1].meta.changes)
      throw new ApiError(409, "account_deleting", "Another account deletion attempt is in progress. Try again shortly.");
    // Deletion triggers queued every removed object durably; a cleanup outage
    // must not turn a committed account deletion into an apparent failure.
    try { await reclaimObjects(c.env, legacyObjects.results.map(row => row.object_key)); await cleanupSyncObjects(c.env, user.id); } catch { /* The scheduler retries the durable queue. */ }
  } finally {
    await c.env.DB.prepare("DELETE FROM account_deletions WHERE user_id = ?1 AND token = ?2").bind(user.id, token).run();
  }
  await countEvent(c.env.DB, "account_deleted");
  return c.json({ ok: true });
});

api.route("/auth", auth);
api.route("/billing", billing);
api.route("/billing/google/notifications", googleNotifications);
api.route("/billing/google", googleBilling);
api.route("/billing/apple/notifications", appleNotifications);
api.route("/billing/apple", appleBilling);
api.route("/sync", sync);
api.route("/releases", releases);
api.route("/visits", visits);
api.route("/usage", usage);
api.route("/admin", admin);
api.route("/assistant-reports", assistantReports);

export const app = new Hono<AppEnv>();

// public/_headers covers static files; responses made here get the same basics.
app.use("*", async (c, next) => {
  await next();
  c.header("X-Content-Type-Options", "nosniff");
  c.header("Referrer-Policy", "strict-origin-when-cross-origin");
  c.header("Strict-Transport-Security", "max-age=63072000; includeSubDomains; preload");
  if (c.req.path.startsWith("/api/") && !c.res.headers.has("Cache-Control")) c.header("Cache-Control", "no-store");
});

app.onError((error, c) => {
  if (error instanceof ApiError)
    return c.json({ error: error.code, message: error.message, ...error.extra }, error.status as ContentfulStatusCode);
  console.error(error);
  return c.json({ error: "server_error", message: "Something went wrong on our side. Try again in a minute." }, 500);
});
app.notFound((c) =>
  c.req.path.startsWith("/api/")
    ? c.json({ error: "not_found", message: "There's no such API endpoint." }, 404)
    : c.env.ASSETS.fetch(c.req.raw),
);

app.route("/api/v1", api);
app.route("/api/v2/sync", syncV2);

/** The admin page's files, only for an admin (Cloudflare Access also stands in front of them). */
app.on(["GET", "HEAD"], ["/admin", "/admin/*"], async (c) => {
  if (!(await adminFor(c.env, c.req.raw)))
    return c.text("The admin page needs a sign-in through Cloudflare Access.", 403, { "Cache-Control": "no-store" });
  const file = await c.env.ASSETS.fetch(c.req.raw);
  const page = new Response(file.body, file);
  page.headers.set("Cache-Control", "no-store");
  page.headers.set("X-Robots-Tag", "noindex, nofollow");
  // This browser's page views and downloads are the team's, not a customer's.
  page.headers.append("Set-Cookie", `${TEAM_COOKIE}=1; Path=/; Max-Age=31536000; Secure; HttpOnly; SameSite=Lax`);
  return page;
});
app.route("/download", downloads);
app.route("/releases", files);

export default {
  fetch: app.fetch,
  /**
   * Hourly: refresh older Play receipts, end lapsed App Store subscriptions, note GitHub's and the
   * Microsoft Store's download numbers, and forget expired sessions, rate limits, problems and
   * reports.
   */
  async scheduled(_controller: ScheduledController, env: Env) {
    await refreshGooglePurchases(env);
    await settleAppleSubscriptions(env);
    try { await maintainSyncV2(env); } catch (error) { console.error("[sync-v2-maintenance]", error); }
    await cleanupSyncObjects(env);
    // Numbers from elsewhere never hold up the clean-up; a failure is noted for the admin page.
    await Promise.all([snapshotGitHub(env), fetchStore(env)].map((p) => p.catch((error) => console.error("[channels]", error))));
    const at = now();
    const dayAgo = new Date(Date.now() - 86_400_000).toISOString();
    await env.DB.batch([
      env.DB.prepare("DELETE FROM login_codes WHERE expires_at < ?1").bind(at),
      env.DB.prepare("DELETE FROM sessions WHERE expires_at < ?1").bind(at),
      env.DB.prepare("DELETE FROM rate_limits WHERE window_start < ?1").bind(dayAgo),
      env.DB.prepare("DELETE FROM problems WHERE at < ?1").bind(new Date(Date.now() - 90 * 86_400_000).toISOString()),
      env.DB.prepare("DELETE FROM assistant_reports WHERE created_at < ?1").bind(new Date(Date.now() - 90 * 86_400_000).toISOString()),
    ]);
    await countEvent(env.DB, "cleanup");
  },
} satisfies ExportedHandler<Env>;
