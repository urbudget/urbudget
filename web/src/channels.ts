// Download numbers from outside urbudget.app, fetched by the hourly cron so the admin page can add
// them up per day next to its own: GitHub's running count of each file on the repository's
// releases, kept as a daily snapshot, and the Microsoft Store's acquisitions and installs from
// Partner Center's analytics API. Both are totals; neither says anything about who.
import type { Env } from "./env";
import { STORE_ID } from "./releases";
import { now } from "./util";

export type Source = "github" | "store";

/** When a source was last asked and last answered, and what went wrong the last time it didn't. */
export interface FetchStatus {
  triedAt: string;
  okAt: string | null;
  error: string;
}

async function noteFetch(db: D1Database, source: Source, error = "") {
  const at = now();
  await db
    .prepare(
      `INSERT INTO channel_fetches (source, tried_at, ok_at, error) VALUES (?1, ?2, ?3, ?4)
       ON CONFLICT(source) DO UPDATE SET tried_at = ?2, ok_at = COALESCE(?3, ok_at), error = ?4`,
    )
    .bind(source, at, error ? null : at, error)
    .run();
}

export async function fetchStatus(db: D1Database): Promise<Partial<Record<Source, FetchStatus>>> {
  const rows = await db
    .prepare("SELECT source, tried_at, ok_at, error FROM channel_fetches")
    .all<{ source: Source; tried_at: string; ok_at: string | null; error: string }>();
  return Object.fromEntries(rows.results.map((r) => [r.source, { triedAt: r.tried_at, okAt: r.ok_at, error: r.error }]));
}

/* ---------- GitHub ---------- */

interface GitHubRelease {
  tag_name: string;
  name: string | null;
  draft: boolean;
  prerelease: boolean;
  published_at: string | null;
  assets: { name: string; size: number; download_count: number }[];
}

/** The system an installer on GitHub is for, by its name; null for signatures, update packages and the Store's package. */
export function installerPlatform(name: string): "windows" | "macos" | null {
  if (/\.(exe|msi)$/i.test(name)) return "windows";
  if (/\.dmg$/i.test(name)) return "macos";
  return null;
}

/** Download counts of the files on GitHub's releases. A token is optional: it lifts the rate limit. */
export async function githubReleases(env: Env) {
  if (!env.GITHUB_REPO) return { configured: false as const };
  try {
    const response = await fetch(`https://api.github.com/repos/${env.GITHUB_REPO}/releases?per_page=20`, {
      headers: {
        Accept: "application/vnd.github+json",
        "User-Agent": "urbudget-admin",
        "X-GitHub-Api-Version": "2022-11-28",
        ...(env.GITHUB_TOKEN ? { Authorization: `Bearer ${env.GITHUB_TOKEN}` } : {}),
      },
    });
    if (!response.ok) {
      const hint =
        response.status === 404
          ? " Check GITHUB_REPO."
          : response.status === 401
            ? " Check GITHUB_TOKEN."
            : response.status === 403 || response.status === 429
              ? " That's GitHub's rate limit: set GITHUB_TOKEN."
              : "";
      return { configured: true as const, error: `GitHub answered ${response.status}.${hint}` };
    }
    const releases = (await response.json()) as GitHubRelease[];
    return {
      configured: true as const,
      releases: releases
        .filter((r) => !r.draft)
        .map((r) => ({
          tag: r.tag_name,
          name: r.name || r.tag_name,
          prerelease: r.prerelease,
          publishedAt: r.published_at,
          assets: r.assets.map((a) => ({ name: a.name, size: a.size, downloads: a.download_count })),
        })),
    };
  } catch (error) {
    return { configured: true as const, error: `GitHub couldn't be reached: ${error instanceof Error ? error.message : String(error)}` };
  }
}

/** Keeps today's count of every file on GitHub's releases (the latest of the day wins). */
export async function snapshotGitHub(env: Env) {
  if (!env.GITHUB_REPO) return;
  const github = await githubReleases(env);
  if (!github.configured) return;
  if ("error" in github) return noteFetch(env.DB, "github", github.error);
  const day = now().slice(0, 10);
  const statements = github.releases.flatMap((release) =>
    release.assets.map((asset) =>
      env.DB.prepare(
        `INSERT INTO github_downloads (day, tag, asset, count) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(day, tag, asset) DO UPDATE SET count = ?4`,
      ).bind(day, release.tag, asset.name, asset.downloads),
    ),
  );
  if (statements.length) await env.DB.batch(statements);
  await noteFetch(env.DB, "github");
}

/**
 * Installers downloaded from GitHub per day and system, from `first` on: each file's rise from its
 * count the day before. Files already there when the snapshots began start from their count then,
 * since GitHub doesn't say when those downloads happened. A file that appears later starts from zero.
 * These are unfiltered file downloads, including release workflows, QA and other automation.
 * GitHub cannot identify people; never include these counts in user totals or conversion rates.
 */
export async function githubDaily(db: D1Database, first: string) {
  const rows = await db
    .prepare(
      `SELECT day, asset, n FROM (
         SELECT day, asset, count - COALESCE(
           LAG(count) OVER (PARTITION BY tag, asset ORDER BY day),
           CASE WHEN day > (SELECT MIN(day) FROM github_downloads) THEN 0 ELSE count END
         ) AS n FROM github_downloads
       ) WHERE day >= ?1 AND n > 0`,
    )
    .bind(first)
    .all<{ day: string; asset: string; n: number }>();
  const since = await db.prepare("SELECT MIN(day) AS day FROM github_downloads").first<{ day: string | null }>();
  const daily: { day: string; platform: "windows" | "macos"; n: number }[] = [];
  for (const row of rows.results) {
    const platform = installerPlatform(row.asset);
    if (platform) daily.push({ day: row.day, platform, n: row.n });
  }
  return { daily, since: since?.day ?? null };
}

/* ---------- Microsoft Store ---------- */

export const storeConfigured = (env: Env) => Boolean(env.MSSTORE_TENANT_ID && env.MSSTORE_CLIENT_ID && env.MSSTORE_CLIENT_SECRET);

/** The last 30 days are fetched each time: Partner Center fills in the latest days a few days late. */
const STORE_DAYS = 30;
const HOUR_MS = 3_600_000;
/** Partner Center's date format: 10/23/2026. */
const storeDate = (day: string) => {
  const [y, m, d] = day.split("-");
  return `${Number(m)}/${Number(d)}/${y}`;
};

async function storeToken(env: Env) {
  const response = await fetch(`https://login.microsoftonline.com/${encodeURIComponent(env.MSSTORE_TENANT_ID!)}/oauth2/token`, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "client_credentials",
      client_id: env.MSSTORE_CLIENT_ID!,
      client_secret: env.MSSTORE_CLIENT_SECRET!,
      resource: "https://manage.devcenter.microsoft.com",
    }),
  });
  const body = (await response.json().catch(() => ({}))) as { access_token?: string };
  if (!response.ok || !body.access_token)
    throw new Error(
      `Microsoft Entra refused the sign-in (${response.status}). Check MSSTORE_TENANT_ID, MSSTORE_CLIENT_ID and MSSTORE_CLIENT_SECRET: Partner Center keys expire.`,
    );
  return body.access_token;
}

/** One of Partner Center's daily reports, as a total per YYYY-MM-DD day. */
async function storeReport(token: string, report: string, field: string, start: string, end: string) {
  const query = new URLSearchParams({
    applicationId: STORE_ID,
    startDate: storeDate(start),
    endDate: storeDate(end),
    aggregationLevel: "day",
    groupby: "date",
    top: "10000",
  });
  const response = await fetch(`https://manage.devcenter.microsoft.com/v1.0/my/analytics/${report}?${query}`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok)
    throw new Error(
      `Partner Center answered ${response.status} for ${report}.${response.status === 401 || response.status === 403 ? " The Entra application needs the Manager role in Partner Center." : ""}`,
    );
  const body = (await response.json()) as { Value?: Record<string, unknown>[] };
  const days = new Map<string, number>();
  for (const row of body.Value ?? []) {
    const day = String(row.date ?? "").slice(0, 10);
    if (/^\d{4}-\d{2}-\d{2}$/.test(day)) days.set(day, (days.get(day) ?? 0) + (Number(row[field]) || 0));
  }
  return days;
}

/** Fetches the Store's last 30 days once a day; after a failure, again three hours later. */
export async function fetchStore(env: Env) {
  if (!storeConfigured(env)) return;
  const today = now().slice(0, 10);
  const last = await env.DB.prepare("SELECT tried_at, ok_at FROM channel_fetches WHERE source = 'store'").first<{
    tried_at: string;
    ok_at: string | null;
  }>();
  if (last?.ok_at?.slice(0, 10) === today) return;
  if (last && Date.now() - Date.parse(last.tried_at) < 3 * HOUR_MS) return;
  const start = new Date(Date.now() - (STORE_DAYS - 1) * 86_400_000).toISOString().slice(0, 10);
  try {
    const token = await storeToken(env);
    const [acquisitions, installs] = await Promise.all([
      storeReport(token, "appacquisitions", "acquisitionQuantity", start, today),
      storeReport(token, "installs", "successfulInstallCount", start, today),
    ]);
    const reports = [["acquisitions", acquisitions], ["installs", installs]] as const;
    const statements = reports.flatMap(([kind, days]) =>
      [...days].map(([day, n]) =>
        env.DB.prepare(
          `INSERT INTO store_counts (day, kind, count) VALUES (?1, ?2, ?3)
           ON CONFLICT(day, kind) DO UPDATE SET count = ?3`,
        ).bind(day, kind, n),
      ),
    );
    if (statements.length) await env.DB.batch(statements);
    await noteFetch(env.DB, "store");
  } catch (error) {
    await noteFetch(env.DB, "store", error instanceof Error ? error.message : String(error));
  }
}
