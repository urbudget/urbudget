// The admin page's API, under /api/v1/admin: accounts, the assistant reports inbox, and the
// numbers worth watching. Every route needs an admin signed in through Cloudflare Access
// (./access.ts), and every change is written to admin_log with who made it.
import { Hono } from "hono";
import { requireAdmin } from "./access";
import { provider, SYNC_STATUSES } from "./billing";
import { fetchStatus, githubDaily, githubReleases, storeConfigured } from "./channels";
import type { AppEnv, Env } from "./env";
import type { Release } from "./releases";
import { ApiError, mondayOf, now } from "./util";
import { syncUsage } from "./sync";

const PAGE = 50;
const DAY_MS = 86_400_000;

const day = (offset = 0) => new Date(Date.now() - offset * DAY_MS).toISOString().slice(0, 10);
const since = (days: number) => new Date(Date.now() - days * DAY_MS).toISOString();
/** The last `count` days, oldest first, as YYYY-MM-DD (UTC). */
const lastDays = (count: number) => Array.from({ length: count }, (_, i) => day(count - 1 - i));

/** Daily rows (`day`, `n`) as one number per day of `days`: their sum, 0 where there's no row. */
function perDay(days: string[], rows: { day: string; n: number }[]) {
  const counts = new Map<string, number>();
  for (const row of rows) counts.set(row.day, (counts.get(row.day) ?? 0) + row.n);
  return days.map((d) => counts.get(d) ?? 0);
}
/** Series of the same length added up, day by day. */
const add = (...series: number[][]) => series[0].map((_, i) => series.reduce((n, values) => n + values[i], 0));

const text = (value: unknown, max: number) => (typeof value === "string" ? value.trim().slice(0, max) : "");

export const admin = new Hono<AppEnv>();
admin.use("*", requireAdmin);

/** User-submitted AI reports are private to support. No payload is copied into the admin log. */
admin.get("/assistant-reports", async (c) => {
  const offset = Number(c.req.query("offset") ?? 0);
  if (!Number.isSafeInteger(offset) || offset < 0)
    throw new ApiError(400, "invalid_offset", "Choose a valid reports page.");
  const reports = await c.env.DB.prepare(
    "SELECT id, created_at AS createdAt, reply, reason FROM assistant_reports ORDER BY created_at DESC, id DESC LIMIT 51 OFFSET ?1",
  ).bind(offset).all<{ id: string; createdAt: string; reply: string; reason: string }>();
  return c.json({ reports: reports.results.slice(0, 50), hasMore: reports.results.length > 50 });
});

admin.delete("/assistant-reports/:id", async (c) => {
  const id = c.req.param("id");
  const report = await c.env.DB.prepare("SELECT id FROM assistant_reports WHERE id = ?1").bind(id).first();
  if (!report) throw new ApiError(404, "not_found", "That report is already gone.");
  await c.env.DB.batch([
    c.env.DB.prepare("DELETE FROM assistant_reports WHERE id = ?1").bind(id),
    c.env.DB.prepare("INSERT INTO admin_log (at, admin, action, detail) VALUES (?1, ?2, ?3, ?4)")
      .bind(now(), c.get("admin"), "delete_assistant_report", id),
  ]);
  return c.json({ ok: true });
});

/* ---------- Overview ---------- */

/** In SQL: the Monday on or before a YYYY-MM-DD day. */
const weekOf = (column: string) => `date(${column}, '-6 days', 'weekday 1')`;
const WEEKS = 12;

/** Resend's free plan; the admin page warns as sending nears either limit. */
const EMAIL_LIMITS = { perDay: 100, perMonth: 3000 };

/** Emails Resend accepted today and this calendar month (UTC). */
async function emailsSent(db: D1Database) {
  const today = day();
  const row = await db
    .prepare(
      `SELECT COALESCE(SUM(count), 0) AS month, COALESCE(SUM(CASE day WHEN ?2 THEN count END), 0) AS today
       FROM events WHERE name = 'email' AND day >= ?1`,
    )
    .bind(`${today.slice(0, 7)}-01`, today)
    .first<{ month: number; today: number }>();
  return { today: row?.today ?? 0, month: row?.month ?? 0, ...EMAIL_LIMITS };
}
/** The day the daily clean-up last ran, and the first day anything was counted in `events`. */
async function eventDays(db: D1Database) {
  const row = await db
    .prepare("SELECT MIN(day) AS first, MAX(CASE name WHEN 'cleanup' THEN day END) AS cleanup FROM events")
    .first<{ first: string | null; cleanup: string | null }>();
  return { countingSince: row?.first ?? null, lastCleanup: row?.cleanup ?? null };
}

/**
 * Downloads from every channel and first openings of the app, as one number per day of `days`:
 * people's clicks on urbudget.app's installers (bots and the team are left out) and on its Store
 * button, installers downloaded from GitHub, the Store's acquisitions and installs from Partner
 * Center, and copies opened for the first time, by where they came from.
 */
async function channelSeries(env: Env, days: string[]) {
  const db = env.DB;
  const first = days[0];
  const [website, github, store, opens, status] = await Promise.all([
    db
      .prepare(
        `SELECT day, platform, version = 'Microsoft Store' AS store, SUM(count) AS n FROM download_counts
         WHERE day >= ?1 AND kind = 'person' GROUP BY day, platform, store`,
      )
      .bind(first)
      .all<{ day: string; platform: string; store: number; n: number }>(),
    githubDaily(db, first),
    db.prepare("SELECT day, kind, count AS n FROM store_counts WHERE day >= ?1").bind(first).all<{ day: string; kind: string; n: number }>(),
    db
      .prepare("SELECT day, channel, SUM(count) AS n FROM app_usage WHERE day >= ?1 AND kind = 'install' GROUP BY day, channel")
      .bind(first)
      .all<{ day: string; channel: string; n: number }>(),
    fetchStatus(db),
  ]);
  const series = {
    websiteWindows: perDay(days, website.results.filter((r) => !r.store && r.platform === "windows")),
    websiteMacos: perDay(days, website.results.filter((r) => !r.store && r.platform === "macos")),
    storeClicks: perDay(days, website.results.filter((r) => r.store)),
    githubWindows: perDay(days, github.daily.filter((r) => r.platform === "windows")),
    githubMacos: perDay(days, github.daily.filter((r) => r.platform === "macos")),
    storeAcquisitions: perDay(days, store.results.filter((r) => r.kind === "acquisitions")),
    storeInstalls: perDay(days, store.results.filter((r) => r.kind === "installs")),
    opensDirect: perDay(days, opens.results.filter((r) => r.channel === "direct")),
    opensStore: perDay(days, opens.results.filter((r) => r.channel === "store")),
  };
  // The Store's downloads are its acquisitions once Partner Center has answered. Until then, clicks
  // on the site's Store button stand in, which miss everyone who found UrBudget in the Store itself.
  const partnerCenter = storeConfigured(env) && Boolean(status.store?.okAt);
  const websiteAll = add(series.websiteWindows, series.websiteMacos);
  const githubAll = add(series.githubWindows, series.githubMacos);
  const storeAll = partnerCenter ? series.storeAcquisitions : series.storeClicks;
  return {
    ...series,
    website: websiteAll,
    github: githubAll,
    store: storeAll,
    total: add(websiteAll, githubAll, storeAll),
    windows: add(series.websiteWindows, series.githubWindows, storeAll),
    macos: add(series.websiteMacos, series.githubMacos),
    opens: add(series.opensDirect, series.opensStore),
    partnerCenter,
    githubSince: github.since,
    status,
  };
}

/** Copies used each day, and each week of `weekStarts` (Mondays), as the app reports them. */
async function activeSeries(db: D1Database, days: string[], weekStarts: string[]) {
  const [daily, weekly, since] = await Promise.all([
    db
      .prepare("SELECT day, channel, SUM(count) AS n FROM app_usage WHERE day >= ?1 AND kind = 'day' GROUP BY day, channel")
      .bind(days[0])
      .all<{ day: string; channel: string; n: number }>(),
    db
      .prepare(
        `SELECT ${weekOf("day")} AS week, channel, platform, SUM(count) AS n FROM app_usage
         WHERE day >= ?1 AND kind = 'week' GROUP BY week, channel, platform`,
      )
      .bind(weekStarts.at(-1) ?? days[0])
      .all<{ week: string; channel: string; platform: string; n: number }>(),
    db.prepare("SELECT MIN(day) AS day FROM app_usage").first<{ day: string | null }>(),
  ]);
  const inWeek = (start: string, where: (r: { channel: string; platform: string }) => boolean = () => true) =>
    weekly.results.filter((r) => r.week === start && where(r)).reduce((n, r) => n + r.n, 0);
  return {
    direct: perDay(days, daily.results.filter((r) => r.channel === "direct")),
    store: perDay(days, daily.results.filter((r) => r.channel === "store")),
    weeks: weekStarts.map((start) => ({
      start,
      active: inWeek(start),
      direct: inWeek(start, (r) => r.channel === "direct"),
      store: inWeek(start, (r) => r.channel === "store"),
      windows: inWeek(start, (r) => r.platform === "windows"),
      macos: inWeek(start, (r) => r.platform === "macos"),
    })),
    since: since?.day ?? null,
  };
}

/** The last WEEKS weeks (Monday to Sunday, UTC) of `days`, newest first; the first is this week so far. */
const recentWeeks = (days: string[]) => [...new Set(days.map(mondayOf))].slice(-WEEKS).reverse();

admin.get("/overview", async (c) => {
  const db = c.env.DB;
  const days = lastDays(90);
  const first = days[0];
  const weekStarts = recentWeeks(days);
  const [
    users,
    active,
    subscriptions,
    sync,
    downloads,
    webhook,
    webhooks,
    problems,
    signups,
    channels,
    inUse,
    views,
    home,
    checkouts,
    tags,
    emails,
    counted,
  ] = await Promise.all([
    db.prepare("SELECT COUNT(*) AS total FROM users").first<{ total: number }>(),
    db
      .prepare(
        `SELECT kind, COUNT(DISTINCT user_id) AS n FROM sessions
         WHERE kind IN ('web', 'desktop') AND last_seen_at >= ?1 GROUP BY kind`,
      )
      .bind(since(30))
      .all<{ kind: string; n: number }>(),
    // Which store a subscription came from is in its id's prefix (Polar's ids have none).
    db
      .prepare(
        `SELECT status, CASE WHEN id LIKE 'google:%' THEN 'google' WHEN id LIKE 'apple:%' THEN 'apple'
           WHEN id LIKE 'comp:%' THEN 'comp' ELSE 'polar' END AS provider, COUNT(*) AS n
         FROM subscriptions GROUP BY status, provider`,
      )
      .all<{ status: string; provider: string; n: number }>(),
    syncUsage(db),
    // The site's clicks by kind, for the bots and the team left out of every other number.
    db
      .prepare("SELECT kind, SUM(count) AS n FROM download_counts WHERE day >= ?1 GROUP BY kind")
      .bind(day(29))
      .all<{ kind: string; n: number }>(),
    db.prepare("SELECT type, received_at FROM webhook_events ORDER BY received_at DESC LIMIT 1").first<{ type: string; received_at: string }>(),
    db.prepare("SELECT COUNT(*) AS n FROM webhook_events WHERE received_at >= ?1").bind(since(30)).first<{ n: number }>(),
    db.prepare("SELECT kind, COUNT(*) AS n FROM problems WHERE at >= ?1 GROUP BY kind").bind(since(30)).all<{ kind: string; n: number }>(),
    db
      .prepare("SELECT substr(created_at, 1, 10) AS day, COUNT(*) AS n FROM users WHERE created_at >= ?1 GROUP BY day")
      .bind(first)
      .all<{ day: string; n: number }>(),
    channelSeries(c.env, days),
    activeSeries(db, days, weekStarts),
    db
      .prepare("SELECT day, SUM(count) AS n FROM page_views WHERE day >= ?1 AND kind = 'person' GROUP BY day")
      .bind(first)
      .all<{ day: string; n: number }>(),
    db
      .prepare("SELECT SUM(count) AS n FROM page_views WHERE day >= ?1 AND kind = 'person' AND path = '/'")
      .bind(day(29))
      .first<{ n: number | null }>(),
    // Sync checkouts opened (the app itself is free, so Sync is the only sale).
    db
      .prepare("SELECT day, SUM(count) AS n FROM events WHERE day >= ?1 AND name = 'checkout' AND detail = 'sync' GROUP BY day")
      .bind(first)
      .all<{ day: string; n: number }>(),
    // People's downloads from the site per week and link tag, for the weekly log.
    db
      .prepare(
        `SELECT ${weekOf("day")} AS week, substr(source, 5) AS tag, SUM(count) AS n FROM download_counts
         WHERE day >= ?1 AND kind = 'person' AND source LIKE 'ref:%' GROUP BY week, tag`,
      )
      .bind(first)
      .all<{ week: string; tag: string; n: number }>(),
    emailsSent(db),
    eventDays(db),
  ]);

  const sum = <T>(rows: T[], pick: (row: T) => number, where: (row: T) => boolean = () => true) =>
    rows.filter(where).reduce((total, row) => total + (pick(row) ?? 0), 0);
  const live = subscriptions.results.filter((s) => SYNC_STATUSES.includes(s.status));
  const tally = <T extends { n: number }>(rows: T[], key: (row: T) => string) =>
    rows.reduce<Record<string, number>>((counts, row) => ({ ...counts, [key(row)]: (counts[key(row)] ?? 0) + row.n }), {});

  // Every 30-day number, the 30 days before it and each week come from these, so they agree.
  const series = {
    checkouts: perDay(days, checkouts.results),
    signups: perDay(days, signups.results),
    views: perDay(days, views.results),
    downloads: channels.total,
    website: channels.website,
    github: channels.github,
    store: channels.store,
    opens: channels.opens,
    active: add(inUse.direct, inUse.store),
  };
  const total = (values: number[], from: number, to?: number) => values.slice(from, to).reduce((a, b) => a + b, 0);
  /** Today and the 29 days before; and the 30 days before those. */
  const last30 = (values: number[]) => total(values, -30);
  const prev30 = (values: number[]) => total(values, -60, -30);

  const weekOfDay = days.map(mondayOf);
  const weeks = weekStarts.map((start, i) => {
    const inWeek = (values: number[]) => values.reduce((n, v, k) => (weekOfDay[k] === start ? n + v : n), 0);
    return {
      start,
      views: inWeek(series.views),
      downloads: inWeek(series.downloads),
      website: inWeek(series.website),
      github: inWeek(series.github),
      store: inWeek(series.store),
      tags: tags.results
        .filter((t) => t.week === start)
        .sort((a, b) => b.n - a.n || a.tag.localeCompare(b.tag))
        .map((t) => ({ tag: t.tag, n: t.n })),
      opens: inWeek(series.opens),
      active: inUse.weeks[i].active,
      checkouts: inWeek(series.checkouts),
      accounts: inWeek(series.signups),
    };
  });

  return c.json({
    admin: c.get("admin"),
    environment: c.env.BILLING_ENV,
    accounts: {
      total: users?.total ?? 0,
      last30: last30(series.signups),
      activeWeb: sum(active.results, (r) => r.n, (r) => r.kind === "web"),
      activeDesktop: sum(active.results, (r) => r.n, (r) => r.kind === "desktop"),
    },
    sync: {
      subscribers: sum(live, (s) => s.n),
      byStatus: tally(subscriptions.results, (s) => s.status),
      // Live subscriptions per store: polar, google (Play), apple (App Store) and comp (given by UrBudget).
      byProvider: tally(live, (s) => s.provider),
      budgets: sync?.budgets ?? 0,
      users: sync?.users ?? 0,
      bytes: sync?.bytes ?? 0,
    },
    // People's downloads from every channel, last 30 days; the site's bots and team are counted apart.
    downloads: {
      last30: last30(series.downloads),
      website: last30(series.website),
      github: last30(series.github),
      store: last30(series.store),
      storeSource: channels.partnerCenter ? "partner-center" : "clicks",
      windows: last30(channels.windows),
      macos: last30(channels.macos),
      team: sum(downloads.results, (r) => r.n, (r) => r.kind === "team"),
      bots: sum(downloads.results, (r) => r.n, (r) => r.kind === "bot"),
    },
    // Copies opened for the first time, and copies used (by week, newest first), as the app reports them.
    usage: {
      opens30: last30(series.opens),
      since: inUse.since,
      thisWeek: inUse.weeks[0]?.active ?? 0,
      lastWeek: inUse.weeks[1]?.active ?? 0,
      weekBefore: inUse.weeks[2]?.active ?? 0,
      yesterday: series.active.at(-2) ?? 0,
    },
    // People's page views, last 30 days.
    pageViews: { last30: last30(series.views), home: home?.n ?? 0 },
    // The same numbers for the 30 days before, to compare with.
    previous30: {
      checkouts: prev30(series.checkouts),
      downloads: prev30(series.downloads),
      opens: prev30(series.opens),
      pageViews: prev30(series.views),
      accounts: prev30(series.signups),
    },
    // From a visit to a Sync checkout, last 30 days. Checkouts are counted from `countingSince` on.
    funnel: {
      views: last30(series.views),
      downloads: last30(series.downloads),
      opens: last30(series.opens),
      checkouts: last30(series.checkouts),
    },
    countingSince: counted.countingSince,
    health: {
      lastWebhook: webhook ? { type: webhook.type, at: webhook.received_at } : null,
      webhooks30: webhooks?.n ?? 0,
      problems30: Object.fromEntries(problems.results.map((p) => [p.kind, p.n])),
      emails,
      lastCleanup: counted.lastCleanup,
    },
    daily: { days, ...series },
    weeks,
  });
});

/* ---------- Admin log ---------- */

admin.get("/log", async (c) => {
  const rows = await c.env.DB.prepare("SELECT at, admin, action, detail FROM admin_log ORDER BY id DESC LIMIT 200")
    .all<{ at: string; admin: string; action: string; detail: string }>();
  return c.json({ entries: rows.results });
});

/* ---------- Accounts ---------- */

const LIVE_SYNC = SYNC_STATUSES.map((s) => `'${s}'`).join(", ");
const ACCOUNT_FILTERS: Record<string, string> = {
  all: "1",
  sync: `EXISTS (SELECT 1 FROM subscriptions s WHERE s.user_id = u.id AND s.status IN (${LIVE_SYNC}))`,
  app: "EXISTS (SELECT 1 FROM sessions s WHERE s.user_id = u.id AND s.kind = 'desktop')",
  // Never subscribed to Sync.
  nothing: "NOT EXISTS (SELECT 1 FROM subscriptions s WHERE s.user_id = u.id)",
};
const like = (s: string) => `%${s.replace(/[\\%_]/g, (ch) => `\\${ch}`)}%`;
/** A filter's SQL by name; `all` for any other name, including ones every object inherits (`constructor`). */
const filterBy = (filters: Record<string, string>, name: string | undefined) =>
  name !== undefined && Object.hasOwn(filters, name) ? filters[name] : filters.all;

interface AccountRow {
  id: string;
  email: string;
  created_at: string;
  subscription: string | null;
  devices: number;
  last_seen_at: string | null;
  budgets: number;
}

/** Search by email (or part of one), account id or the payment provider's customer id; newest first. */
admin.get("/accounts", async (c) => {
  const q = text(c.req.query("q"), 200);
  const where = filterBy(ACCOUNT_FILTERS, c.req.query("filter"));
  const page = Math.max(0, Number.parseInt(c.req.query("page") ?? "0", 10) || 0);
  const match = `(?1 = '' OR u.email LIKE ?2 ESCAPE '\\' OR u.id = ?1 OR u.billing_customer_id = ?1) AND ${where}`;
  const binds = [q, like(q.toLowerCase())];
  const [rows, count] = await Promise.all([
    c.env.DB.prepare(
      `SELECT u.id, u.email, u.created_at,
         (SELECT status FROM subscriptions s WHERE s.user_id = u.id
          ORDER BY (s.status IN (${LIVE_SYNC})) DESC, s.updated_at DESC LIMIT 1) AS subscription,
         (SELECT COUNT(*) FROM sessions s WHERE s.user_id = u.id AND s.kind = 'desktop') AS devices,
         (SELECT MAX(last_seen_at) FROM sessions s WHERE s.user_id = u.id AND s.kind IN ('web', 'desktop')) AS last_seen_at,
         (SELECT COUNT(*) FROM sync_budgets b WHERE b.user_id = u.id)
         + (SELECT COUNT(*) FROM sync_heads_v2 b WHERE b.user_id = u.id AND b.state='active') AS budgets
       FROM users u WHERE ${match} ORDER BY u.created_at DESC, u.id LIMIT ?3 OFFSET ?4`,
    )
      .bind(...binds, PAGE, page * PAGE)
      .all<AccountRow>(),
    c.env.DB.prepare(`SELECT COUNT(*) AS n FROM users u WHERE ${match}`)
      .bind(...binds)
      .first<{ n: number }>(),
  ]);
  return c.json({
    accounts: rows.results.map((a) => ({
      id: a.id,
      email: a.email,
      createdAt: a.created_at,
      subscription: a.subscription,
      devices: a.devices,
      lastSeenAt: a.last_seen_at,
      budgets: a.budgets,
    })),
    total: count?.n ?? 0,
    page,
    pageSize: PAGE,
  });
});

/** One account for support: its sync plan, signed-in devices and synced budgets (sizes only). */
admin.get("/accounts/:id", async (c) => {
  const db = c.env.DB;
  const user = await db
    .prepare("SELECT id, email, billing_customer_id, created_at FROM users WHERE id = ?1")
    .bind(c.req.param("id"))
    .first<{ id: string; email: string; billing_customer_id: string | null; created_at: string }>();
  if (!user) throw new ApiError(404, "not_found", "There's no such account. It may have been deleted.");
  const [subscriptions, sessions, budgets, key] = await Promise.all([
    db
      .prepare("SELECT id, status, current_period_end, cancel_at, updated_at FROM subscriptions WHERE user_id = ?1 ORDER BY updated_at DESC")
      .bind(user.id)
      .all<{ id: string; status: string; current_period_end: string | null; cancel_at: string | null; updated_at: string }>(),
    // Sign-in links last minutes and aren't devices.
    db
      .prepare(
        `SELECT kind, device_name, created_at, last_seen_at, expires_at FROM sessions
         WHERE user_id = ?1 AND kind IN ('web', 'desktop') ORDER BY last_seen_at DESC`,
      )
      .bind(user.id)
      .all<{ kind: string; device_name: string | null; created_at: string; last_seen_at: string; expires_at: string }>(),
    db
      .prepare(`SELECT budget_id,revision,size,device,updated_at,'active' AS state FROM sync_budgets WHERE user_id=?1
        UNION ALL SELECT budget_id,version AS revision,size,device,updated_at,state FROM sync_heads_v2
        WHERE user_id=?1 AND state<>'purged' ORDER BY updated_at DESC`)
      .bind(user.id)
      .all<{ budget_id: string; revision: number | string; size: number; device: string; updated_at: string; state: string }>(),
    db.prepare(`SELECT NULL AS created_at FROM sync_accounts_v2 WHERE user_id=?1 AND protocol=2 AND key_generation IS NOT NULL
      UNION ALL SELECT created_at FROM sync_keys WHERE user_id=?1
      AND NOT EXISTS(SELECT 1 FROM sync_accounts_v2 WHERE user_id=?1 AND protocol=2) LIMIT 1`)
      .bind(user.id).first<{ created_at: string | null }>(),
  ]);
  return c.json({
    account: { id: user.id, email: user.email, customerId: user.billing_customer_id, createdAt: user.created_at },
    subscriptions: subscriptions.results.map((s) => ({
      id: s.id,
      status: s.status,
      live: SYNC_STATUSES.includes(s.status),
      currentPeriodEnd: s.current_period_end,
      cancelAt: s.cancel_at,
      updatedAt: s.updated_at,
    })),
    sessions: sessions.results.map((s) => ({
      kind: s.kind,
      deviceName: s.device_name,
      createdAt: s.created_at,
      lastSeenAt: s.last_seen_at,
      expiresAt: s.expires_at,
    })),
    // The passphrase for end-to-end encryption was set then; the server never holds a key.
    syncKeySetAt: key?.created_at ?? null,
    syncKeySet: !!key,
    budgets: budgets.results.map((b) => ({ id: b.budget_id, revision: b.revision, size: b.size, device: b.device, updatedAt: b.updated_at, state: b.state })),
  });
});

/* ---------- Downloads: every channel ---------- */

admin.get("/downloads", async (c) => {
  const db = c.env.DB;
  const days = lastDays(90);
  const first = days[0];
  const [channels, versions, sources, others, storeSince, release, github] = await Promise.all([
    channelSeries(c.env, days),
    db
      .prepare(
        `SELECT version, platform, SUM(count) AS n, MIN(day) AS first, MAX(day) AS last FROM download_counts
         WHERE kind = 'person' GROUP BY version, platform ORDER BY last DESC, version DESC`,
      )
      .all<{ version: string; platform: string; n: number; first: string; last: string }>(),
    db
      .prepare("SELECT source, SUM(count) AS n FROM download_counts WHERE day >= ?1 AND kind = 'person' GROUP BY source ORDER BY n DESC LIMIT 25")
      .bind(first)
      .all<{ source: string; n: number }>(),
    db
      .prepare("SELECT kind, day >= ?2 AS recent, SUM(count) AS n FROM download_counts WHERE day >= ?1 AND kind != 'person' GROUP BY kind, recent")
      .bind(first, days[days.length - 30])
      .all<{ kind: string; recent: number; n: number }>(),
    db.prepare("SELECT MIN(day) AS day FROM store_counts").first<{ day: string | null }>(),
    c.env.RELEASES.get("latest.json").then((o) => (o ? (o.json() as Promise<Release>) : null)),
    githubReleases(c.env),
  ]);
  const sum = <T>(rows: T[], pick: (row: T) => number, where: (row: T) => boolean = () => true) =>
    rows.filter(where).reduce((n, row) => n + pick(row), 0);
  return c.json({
    days,
    // People's clicks on urbudget.app: its installers, and its button to the Microsoft Store.
    website: { windows: channels.websiteWindows, macos: channels.websiteMacos, store: channels.storeClicks },
    // Installers downloaded from GitHub, per day since the first snapshot; and every file's count now.
    github: { ...github, windows: channels.githubWindows, macos: channels.githubMacos, since: channels.githubSince, fetched: channels.status.github ?? null },
    // Partner Center's numbers. Until it has answered, the site's Store button stands in for the Store.
    store: {
      configured: storeConfigured(c.env),
      partnerCenter: channels.partnerCenter,
      acquisitions: channels.storeAcquisitions,
      installs: channels.storeInstalls,
      since: storeSince?.day ?? null,
      fetched: channels.status.store ?? null,
    },
    // Every channel added up: the series the Overview's downloads are.
    total: channels.total,
    // Copies opened for the first time: the downloads that became a copy in use.
    opens: { direct: channels.opensDirect, store: channels.opensStore },
    versions: versions.results,
    sources: sources.results,
    // Not customers: browsers that opened this page, and bots.
    others: {
      team: { last30: sum(others.results, (r) => r.n, (r) => r.kind === "team" && r.recent === 1), last90: sum(others.results, (r) => r.n, (r) => r.kind === "team") },
      bots: { last30: sum(others.results, (r) => r.n, (r) => r.kind === "bot" && r.recent === 1), last90: sum(others.results, (r) => r.n, (r) => r.kind === "bot") },
    },
    latest: release && { version: release.version, date: release.date },
  });
});

/* ---------- Usage: copies of the app in use ---------- */

/** How many weeks after its first week the retention table follows a group of copies. */
const RETENTION_WEEKS = 8;

admin.get("/usage", async (c) => {
  const db = c.env.DB;
  const days = lastDays(90);
  const first = days[0];
  const weekStarts = recentWeeks(days);
  const [thisWeek, lastWeek] = weekStarts;
  const [inUse, opens, months, cohorts, versions, launches, launchVersions] = await Promise.all([
    activeSeries(db, days, weekStarts),
    db
      .prepare(`SELECT day, ${weekOf("day")} AS week, channel, SUM(count) AS n FROM app_usage WHERE kind = 'install' GROUP BY day, channel`)
      .all<{ day: string; week: string; channel: string; n: number }>(),
    db
      .prepare(
        "SELECT substr(day, 1, 7) AS month, SUM(count) AS n FROM app_usage WHERE kind = 'month' GROUP BY month ORDER BY month DESC LIMIT 6",
      )
      .all<{ month: string; n: number }>(),
    // Each group of copies first opened the same week, by the weeks it was used in.
    db
      .prepare(`SELECT cohort, ${weekOf("day")} AS week, SUM(count) AS n FROM app_usage WHERE kind = 'week' AND cohort != '' GROUP BY cohort, week`)
      .all<{ cohort: string; week: string; n: number }>(),
    db
      .prepare(
        `SELECT version, platform, channel, SUM(CASE WHEN day >= ?1 THEN count ELSE 0 END) AS thisWeek,
           SUM(CASE WHEN day < ?1 THEN count ELSE 0 END) AS lastWeek
         FROM app_usage WHERE kind = 'week' AND day >= ?2 GROUP BY version, platform, channel
         ORDER BY thisWeek + lastWeek DESC, version DESC`,
      )
      .bind(thisWeek, lastWeek)
      .all<{ version: string; platform: string; channel: string; thisWeek: number; lastWeek: number }>(),
    // Update checks: copies installed outside the Store, including those from before usage counts.
    db
      .prepare("SELECT day, platform, SUM(count) AS n FROM app_launches WHERE day >= ?1 GROUP BY day, platform")
      .bind(first)
      .all<{ day: string; platform: string; n: number }>(),
    db
      .prepare(
        `SELECT version, platform, SUM(count) AS n, MAX(day) AS last FROM app_launches WHERE day >= ?1
         GROUP BY version, platform ORDER BY last DESC, version DESC`,
      )
      .bind(first)
      .all<{ version: string; platform: string; n: number; last: string }>(),
  ]);

  const addWeeks = (monday: string, n: number) => new Date(Date.parse(`${monday}T00:00:00Z`) + n * 7 * DAY_MS).toISOString().slice(0, 10);
  const opened = new Map<string, number>();
  for (const row of opens.results) opened.set(row.week, (opened.get(row.week) ?? 0) + row.n);
  const used = new Map(cohorts.results.map((r) => [`${r.cohort}:${r.week}`, r.n]));
  return c.json({
    days,
    since: inUse.since,
    // Copies used each day, and copies opened for the first time, by where they came from.
    daily: { direct: inUse.direct, store: inUse.store },
    opens: {
      direct: perDay(days, opens.results.filter((r) => r.channel === "direct")),
      store: perDay(days, opens.results.filter((r) => r.channel === "store")),
    },
    weeks: inUse.weeks.map((week) => ({ ...week, opens: opened.get(week.start) ?? 0 })),
    months: months.results.map((m) => ({ month: m.month, active: m.n })),
    // Newest first. `back[k]` is how many were used again k + 1 weeks after the week they were first
    // opened, up to this week (which isn't over).
    cohorts: [...opened]
      .sort(([a], [b]) => b.localeCompare(a))
      .slice(0, 10)
      .map(([week, n]) => {
        const back = [];
        for (let k = 1; k <= RETENTION_WEEKS && addWeeks(week, k) <= thisWeek; k++) back.push(used.get(`${week}:${addWeeks(week, k)}`) ?? 0);
        return { week, opens: n, back };
      }),
    versions: versions.results,
    launches: {
      windows: perDay(days, launches.results.filter((r) => r.platform === "windows")),
      macos: perDay(days, launches.results.filter((r) => r.platform === "darwin")),
      versions: launchVersions.results,
    },
  });
});

/* ---------- Traffic: page views ---------- */

admin.get("/traffic", async (c) => {
  const db = c.env.DB;
  const days = lastDays(90);
  const first = days[0];
  const recent = days[days.length - 30];
  const [views, pages, sources, kinds, downloads, tags] = await Promise.all([
    db
      .prepare("SELECT day, SUM(count) AS n FROM page_views WHERE day >= ?1 AND kind = 'person' GROUP BY day")
      .bind(first)
      .all<{ day: string; n: number }>(),
    db
      .prepare("SELECT path, SUM(count) AS n FROM page_views WHERE day >= ?1 AND kind = 'person' GROUP BY path ORDER BY n DESC, path LIMIT 20")
      .bind(first)
      .all<{ path: string; n: number }>(),
    db
      .prepare(
        "SELECT source, SUM(count) AS n FROM page_views WHERE day >= ?1 AND kind = 'person' AND source != 'internal' GROUP BY source ORDER BY n DESC, source LIMIT 20",
      )
      .bind(first)
      .all<{ source: string; n: number }>(),
    db
      .prepare("SELECT kind, SUM(count) AS n FROM page_views WHERE day >= ?1 GROUP BY kind")
      .bind(recent)
      .all<{ kind: string; n: number }>(),
    db
      .prepare("SELECT SUM(count) AS n FROM download_counts WHERE day >= ?1 AND kind = 'person'")
      .bind(recent)
      .first<{ n: number | null }>(),
    // Every link tag ever counted: the visits that came with it and the downloads after them. The
    // ORDER BY spells out SUM(): a bare `views` there would be the subquery's column, one row's count.
    db
      .prepare(
        `SELECT substr(source, 5) AS tag, SUM(views) AS views, SUM(downloads) AS downloads, MIN(day) AS first, MAX(day) AS last FROM (
           SELECT source, day, count AS views, 0 AS downloads FROM page_views WHERE kind = 'person' AND source LIKE 'ref:%'
           UNION ALL
           SELECT source, day, 0, count FROM download_counts WHERE kind = 'person' AND source LIKE 'ref:%'
         ) GROUP BY tag ORDER BY last DESC, SUM(views) + SUM(downloads) DESC, tag LIMIT 100`,
      )
      .all<{ tag: string; views: number; downloads: number; first: string; last: string }>(),
  ]);
  const of = (kind: string) => kinds.results.find((r) => r.kind === kind)?.n ?? 0;
  return c.json({
    days,
    views: perDay(days, views.results),
    pages: pages.results,
    sources: sources.results,
    // Last 30 days: people's page views, the team's and bots' apart, and people's clicks on the
    // site's download buttons (the Store's included).
    last30: { people: of("person"), team: of("team"), bots: of("bot"), downloads: downloads?.n ?? 0 },
    tags: tags.results,
  });
});

/* ---------- Revenue ---------- */

const METRICS = [
  "revenue",
  "net_revenue",
  "orders",
  "monthly_recurring_revenue",
  "active_subscriptions",
  "new_subscriptions",
  "churned_subscriptions",
  "checkouts",
  "checkouts_conversion",
];
const RANGES: Record<string, { days: number; interval: "day" | "week" | "month" }> = {
  "30": { days: 30, interval: "day" },
  "90": { days: 90, interval: "week" },
  "365": { days: 365, interval: "month" },
};

admin.get("/revenue", async (c) => {
  const range = RANGES[c.req.query("days") ?? "30"] ?? RANGES["30"];
  const start = day(range.days - 1);
  const metrics = await provider.metrics(c.env, { start, end: day(), interval: range.interval }, METRICS);
  return c.json({ environment: c.env.BILLING_ENV, interval: range.interval, start, ...metrics });
});

/* ---------- Health ---------- */

admin.get("/health", async (c) => {
  const db = c.env.DB;
  const [problems, webhooks, types, activity, emails, counted] = await Promise.all([
    db
      .prepare("SELECT id, at, kind, subject, detail FROM problems ORDER BY id DESC LIMIT 100")
      .all<{ id: number; at: string; kind: string; subject: string; detail: string }>(),
    db
      .prepare("SELECT type, received_at FROM webhook_events ORDER BY received_at DESC LIMIT 20")
      .all<{ type: string; received_at: string }>(),
    db
      .prepare("SELECT type, COUNT(*) AS n, MAX(received_at) AS last FROM webhook_events WHERE received_at >= ?1 GROUP BY type ORDER BY n DESC")
      .bind(since(30))
      .all<{ type: string; n: number; last: string }>(),
    db
      .prepare(
        `SELECT name, detail, SUM(CASE WHEN day >= ?2 THEN count ELSE 0 END) AS last7, SUM(count) AS last30
         FROM events WHERE day >= ?1 AND name != 'cleanup' GROUP BY name, detail ORDER BY name, detail`,
      )
      .bind(day(29), day(6))
      .all<{ name: string; detail: string; last7: number; last30: number }>(),
    emailsSent(db),
    eventDays(db),
  ]);
  const env = c.env;
  return c.json({
    // What happened on the API, counted per day since `countingSince`.
    activity: activity.results,
    emails,
    ...counted,
    problems: problems.results,
    webhooks: webhooks.results.map((w) => ({ type: w.type, at: w.received_at })),
    webhookTypes: types.results,
    // What's set, never the values.
    settings: [
      { name: "BILLING_ENV", ok: env.BILLING_ENV === "production", value: env.BILLING_ENV },
      { name: "POLAR_ACCESS_TOKEN", ok: Boolean(env.POLAR_ACCESS_TOKEN) },
      { name: "POLAR_WEBHOOK_SECRET", ok: Boolean(env.POLAR_WEBHOOK_SECRET) },
      { name: "RESEND_API_KEY", ok: Boolean(env.RESEND_API_KEY) },
      { name: "GOOGLE_PLAY_SERVICE_ACCOUNT", ok: Boolean(env.GOOGLE_PLAY_SERVICE_ACCOUNT) },
      { name: "GOOGLE_PLAY_PUSH_EMAIL / GOOGLE_PLAY_PUSH_AUDIENCE", ok: Boolean(env.GOOGLE_PLAY_PUSH_EMAIL && env.GOOGLE_PLAY_PUSH_AUDIENCE) },
      { name: "ACCESS_TEAM_DOMAIN / ACCESS_AUD", ok: Boolean(env.ACCESS_TEAM_DOMAIN && env.ACCESS_AUD) },
      { name: "GITHUB_TOKEN", ok: Boolean(env.GITHUB_TOKEN) },
      { name: "MSSTORE_TENANT_ID / MSSTORE_CLIENT_ID / MSSTORE_CLIENT_SECRET", ok: storeConfigured(env) },
    ],
  });
});
