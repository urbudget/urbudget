// Download numbers from outside urbudget.app, fetched by the hourly cron so the admin page can add
// them up per day next to its own: GitHub's running count of each file on the repository's
// releases, kept as a daily snapshot; the Microsoft Store's acquisitions and installs from
// Partner Center's analytics API; and the phone apps' installs from Google Play's monthly installs
// reports and the App Store's daily sales summaries. All are totals; none says anything about who.
import { APP_STORE_APP_ID } from "./billing/apple";
import { PLAY_PACKAGE, serviceAccountToken } from "./billing/google";
import type { Env } from "./env";
import { STORE_ID } from "./releases";
import { base64url, fromBase64url, now, utf8 } from "./util";

export type Source = "github" | "store" | "play" | "appstore";
/** Which store a row of store_counts is from (migration 0015). */
export type Store = "microsoft" | "play" | "appstore";

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
export function installerPlatform(name: string): "windows" | "macos" | "linux" | null {
  if (/\.(exe|msi)$/i.test(name)) return "windows";
  if (/\.dmg$/i.test(name)) return "macos";
  if (/\.(AppImage|deb)$/i.test(name)) return "linux";
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
  const daily: { day: string; platform: "windows" | "macos" | "linux"; n: number }[] = [];
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

/**
 * Whether a source asked once a day is due: it hasn't answered today (UTC), and wasn't asked in the
 * last three hours, so a failure is retried three hours later.
 */
async function dailyDue(db: D1Database, source: Source) {
  const last = await db.prepare("SELECT tried_at, ok_at FROM channel_fetches WHERE source = ?1").bind(source).first<{
    tried_at: string;
    ok_at: string | null;
  }>();
  if (last?.ok_at?.slice(0, 10) === now().slice(0, 10)) return false;
  return !last || Date.now() - Date.parse(last.tried_at) >= 3 * HOUR_MS;
}

/** A store's daily numbers by kind, each day's replacing what was there. One statement per store. */
async function saveStoreCounts(db: D1Database, store: Store, kinds: Record<string, Map<string, number>>) {
  const rows = Object.entries(kinds).flatMap(([kind, days]) => [...days].map(([day, n]) => [day, kind, n]));
  if (!rows.length) return;
  // The WHERE keeps SQLite from reading ON CONFLICT as a join's ON.
  await db
    .prepare(
      `INSERT INTO store_counts (store, day, kind, count)
       SELECT ?1, json_extract(value, '$[0]'), json_extract(value, '$[1]'), json_extract(value, '$[2]') FROM json_each(?2) WHERE 1
       ON CONFLICT(store, day, kind) DO UPDATE SET count = excluded.count`,
    )
    .bind(store, JSON.stringify(rows))
    .run();
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Fetches the Store's last 30 days once a day; after a failure, again three hours later. */
export async function fetchStore(env: Env) {
  if (!storeConfigured(env) || !(await dailyDue(env.DB, "store"))) return;
  const today = now().slice(0, 10);
  const start = new Date(Date.now() - (STORE_DAYS - 1) * 86_400_000).toISOString().slice(0, 10);
  try {
    const token = await storeToken(env);
    const [acquisitions, installs] = await Promise.all([
      storeReport(token, "appacquisitions", "acquisitionQuantity", start, today),
      storeReport(token, "installs", "successfulInstallCount", start, today),
    ]);
    await saveStoreCounts(env.DB, "microsoft", { acquisitions, installs });
    await noteFetch(env.DB, "store");
  } catch (error) {
    await noteFetch(env.DB, "store", message(error));
  }
}

/* ---------- Report files ---------- */

/** UTF-16 code units as a string, a chunk at a time (spreading a whole file overflows the stack). */
function utf16(bytes: Uint8Array, start: number, littleEndian: boolean) {
  let text = "";
  const units: number[] = [];
  for (let i = start; i + 1 < bytes.length; i += 2) {
    units.push(littleEndian ? bytes[i] | (bytes[i + 1] << 8) : (bytes[i] << 8) | bytes[i + 1]);
    if (units.length === 8192) {
      text += String.fromCharCode(...units);
      units.length = 0;
    }
  }
  return text + String.fromCharCode(...units);
}

/**
 * A report file's text. Play Console writes its CSVs in UTF-16 (little-endian, with a byte-order
 * mark); anything without one is read as UTF-16LE if it looks like it, else UTF-8.
 */
export function reportText(bytes: Uint8Array) {
  if (bytes[0] === 0xff && bytes[1] === 0xfe) return utf16(bytes, 2, true);
  if (bytes[0] === 0xfe && bytes[1] === 0xff) return utf16(bytes, 2, false);
  if (bytes.length > 1 && bytes[0] !== 0 && bytes[1] === 0) return utf16(bytes, 0, true);
  return new TextDecoder().decode(bytes).replace(/^﻿/, "");
}

/** A CSV line's fields; a quoted field may hold commas and doubled quotes. */
function csvFields(line: string) {
  const fields: string[] = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (quoted) {
      if (ch === '"' && line[i + 1] === '"') (field += '"'), i++;
      else if (ch === '"') quoted = false;
      else field += ch;
    } else if (ch === '"') quoted = true;
    else if (ch === ",") fields.push(field), (field = "");
    else field += ch;
  }
  fields.push(field);
  return fields;
}

/**
 * A report's rows by its header's names (trimmed). CSV (Play) is comma-separated with quotes;
 * TSV (Apple) splits on tabs only, since its titles may hold a quote.
 */
export function reportRows(text: string, format: "csv" | "tsv") {
  const lines = text.split(/\r?\n/).filter((line) => line.trim());
  const split = format === "csv" ? csvFields : (line: string) => line.split("\t");
  const header = lines.length ? split(lines[0]).map((name) => name.trim()) : [];
  return {
    header,
    rows: lines.slice(1).map((line) => {
      const fields = split(line);
      return Object.fromEntries(header.map((name, i) => [name, (fields[i] ?? "").trim()]));
    }),
  };
}

/** Adds `n` to a day's number. */
const tally = (days: Map<string, number>, day: string, n: number) => days.set(day, (days.get(day) ?? 0) + n);

/* ---------- Google Play ---------- */

/** The bucket's name from Play Console's Cloud Storage URI (`gs://pubsite_prod_rev_…/`), or "" if it isn't one. */
export function playBucket(uri: string | undefined) {
  const name = (uri ?? "").trim().replace(/^gs:\/\//i, "").replace(/\/.*$/s, "");
  return /^[a-z0-9][a-z0-9._-]{1,220}$/.test(name) ? name : "";
}

export const playReportsConfigured = (env: Env) => Boolean(env.GOOGLE_PLAY_SERVICE_ACCOUNT && env.PLAY_REPORTS_BUCKET);

/** This month and last (UTC) as YYYYMM, oldest first: Play adds days to a month's file 3 to 7 days late. */
export function playMonths(at = new Date()) {
  const y = at.getUTCFullYear();
  const m = at.getUTCMonth();
  return [Date.UTC(y, m - 1, 1), Date.UTC(y, m, 1)].map((t) => new Date(t).toISOString().slice(0, 7).replace("-", ""));
}

/**
 * Installs per day from one of Play Console's monthly installs reports: "Daily User Installs" (people
 * installing the app for the first time on any device: the downloads) and "Daily Device Installs".
 * The overview report has a row a day; the country report a row a day and country, added up here.
 */
export function playInstalls(csv: string) {
  const { header, rows } = reportRows(csv, "csv");
  const users = new Map<string, number>();
  const devices = new Map<string, number>();
  if (!rows.length) return { users, devices };
  for (const column of ["Date", "Daily User Installs", "Daily Device Installs"])
    if (!header.includes(column))
      throw new Error(`Play's installs report has no "${column}" column; its columns are ${header.join(", ")}.`);
  for (const row of rows) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(row["Date"]) || (row["Package Name"] && row["Package Name"] !== PLAY_PACKAGE)) continue;
    tally(users, row["Date"], Number(row["Daily User Installs"]) || 0);
    tally(devices, row["Date"], Number(row["Daily Device Installs"]) || 0);
  }
  return { users, devices };
}

/** A report file from Play's bucket, or null when there's no such file. */
async function playFile(token: string, bucket: string, name: string) {
  const response = await fetch(
    `https://storage.googleapis.com/storage/v1/b/${encodeURIComponent(bucket)}/o/${encodeURIComponent(name)}?alt=media`,
    { headers: { Authorization: `Bearer ${token}` } },
  );
  if (response.status === 404) return null;
  if (!response.ok)
    throw new Error(
      `Cloud Storage answered ${response.status} for gs://${bucket}/${name}.${
        response.status === 401 || response.status === 403
          ? ' In Play Console, Users and permissions, give the GOOGLE_PLAY_SERVICE_ACCOUNT the account permission "View app information and download bulk reports (read-only)". It can take a day to apply.'
          : ""
      }`,
    );
  return new Uint8Array(await response.arrayBuffer());
}

/** Reads Play's installs reports for this month and last once a day; after a failure, again three hours later. */
export async function fetchPlay(env: Env) {
  if (!playReportsConfigured(env) || !(await dailyDue(env.DB, "play"))) return;
  try {
    const bucket = playBucket(env.PLAY_REPORTS_BUCKET);
    if (!bucket)
      throw new Error("PLAY_REPORTS_BUCKET isn't a bucket. Copy it from Play Console, Download reports, Statistics: it looks like gs://pubsite_prod_rev_0123456789/.");
    let token: string;
    try {
      token = await serviceAccountToken(env, "https://www.googleapis.com/auth/devstorage.read_only");
    } catch {
      throw new Error("Google didn't sign the GOOGLE_PLAY_SERVICE_ACCOUNT in for Cloud Storage. Check that the secret is the service account's whole JSON key.");
    }
    const users = new Map<string, number>();
    const devices = new Map<string, number>();
    const months = playMonths();
    let found = 0;
    for (const month of months) {
      // The overview has a row a day. Google's help lists only the per-dimension files, so the
      // country one stands in if a month has no overview.
      const prefix = `stats/installs/installs_${PLAY_PACKAGE}_${month}`;
      const file = (await playFile(token, bucket, `${prefix}_overview.csv`)) ?? (await playFile(token, bucket, `${prefix}_country.csv`));
      if (!file) continue;
      found++;
      const report = playInstalls(reportText(file));
      for (const [day, n] of report.users) users.set(day, n);
      for (const [day, n] of report.devices) devices.set(day, n);
    }
    if (!found)
      throw new Error(
        `gs://${bucket} has no installs report for ${PLAY_PACKAGE} for ${months.join(" or ")}. Check PLAY_REPORTS_BUCKET; a new app's first report can take a few days.`,
      );
    await saveStoreCounts(env.DB, "play", { user_installs: users, device_installs: devices });
    await noteFetch(env.DB, "play");
  } catch (error) {
    await noteFetch(env.DB, "play", message(error));
  }
}

/* ---------- App Store ---------- */

export const appStoreConfigured = (env: Env) =>
  Boolean(env.ASC_ISSUER_ID && env.ASC_KEY_ID && env.ASC_PRIVATE_KEY && env.ASC_VENDOR_NUMBER);

/** The days asked for each time: Apple posts a day's report the day after, and may correct it. */
const APP_STORE_DAYS = 7;

/**
 * What a sales report's Product Type Identifier counts, for the app itself (in-app purchases have
 * their own identifiers and Apple Identifier): first-time downloads for iPhone, universal, iPad
 * and Mac; re-downloads; updates.
 */
const PRODUCT_TYPES: Record<string, "downloads" | "redownloads" | "updates"> = {
  "1": "downloads",
  "1F": "downloads",
  "1T": "downloads",
  F1: "downloads",
  "3": "redownloads",
  "3F": "redownloads",
  "7": "updates",
  "7F": "updates",
  "7T": "updates",
  F7: "updates",
};

/** One day's units of the app from a Summary Sales Report (tab-separated, a row per country, device and so on). */
export function appStoreUnits(tsv: string, appleId = String(APP_STORE_APP_ID)) {
  const { header, rows } = reportRows(tsv, "tsv");
  const units = { downloads: 0, redownloads: 0, updates: 0 };
  if (!rows.length) return units;
  for (const column of ["Apple Identifier", "Product Type Identifier", "Units"])
    if (!header.includes(column)) throw new Error(`The App Store's sales report has no "${column}" column; its columns are ${header.join(", ")}.`);
  for (const row of rows) {
    const kind = PRODUCT_TYPES[row["Product Type Identifier"]];
    if (row["Apple Identifier"] === appleId && kind) units[kind] += Number(row["Units"]) || 0;
  }
  return units;
}

/** A 20-minute ES256 token for the App Store Connect API from the ASC_* secrets. */
async function appStoreToken(env: Env) {
  const seconds = Math.floor(Date.now() / 1000);
  const json = (value: unknown) => base64url(utf8(JSON.stringify(value)));
  const body = `${json({ alg: "ES256", kid: env.ASC_KEY_ID!.trim(), typ: "JWT" })}.${json({
    iss: env.ASC_ISSUER_ID!.trim(),
    iat: seconds,
    exp: seconds + 1200,
    aud: "appstoreconnect-v1",
  })}`;
  let key: CryptoKey;
  try {
    // The .p8 file as it is, or squeezed onto one line with \n for its line breaks.
    const pem = env.ASC_PRIVATE_KEY!.replace(/\\n/g, "\n").replace(/-----[^-]+-----|\s/g, "");
    key = await crypto.subtle.importKey("pkcs8", fromBase64url(pem), { name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]);
  } catch {
    throw new Error("ASC_PRIVATE_KEY isn't an App Store Connect API key: it should be the whole AuthKey_….p8 file.");
  }
  // WebCrypto's ECDSA signature is r‖s, as JWTs want it.
  const signature = await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key, utf8(body));
  return `${body}.${base64url(signature)}`;
}

/** Apple's reason, from its error body, for a failed request; "" when there's none. */
async function appleReason(response: Response) {
  const body = (await response.json().catch(() => ({}))) as { errors?: { detail?: string; title?: string }[] };
  const reason = body.errors?.[0]?.detail || body.errors?.[0]?.title || "";
  return reason ? ` Apple says: ${reason.slice(0, 200)}` : "";
}

/** A day's Summary Sales Report as text, or null when Apple has none (no sales that day, or not out yet). */
async function appStoreReport(env: Env, token: string, day: string) {
  const query = new URLSearchParams({
    "filter[frequency]": "DAILY",
    "filter[reportType]": "SALES",
    "filter[reportSubType]": "SUMMARY",
    "filter[vendorNumber]": env.ASC_VENDOR_NUMBER!.trim(),
    "filter[reportDate]": day,
    "filter[version]": "1_0",
  });
  const response = await fetch(`https://api.appstoreconnect.apple.com/v1/salesReports?${query}`, {
    headers: { Authorization: `Bearer ${token}`, Accept: "application/a-gzip" },
  });
  if (response.status === 404) return null;
  if (!response.ok) {
    const hint =
      response.status === 401
        ? " Check ASC_ISSUER_ID, ASC_KEY_ID and ASC_PRIVATE_KEY: the key may have been revoked."
        : response.status === 403
          ? " The key needs the Sales and Reports role (a Team key, made in Users and Access, Integrations)."
          : response.status === 400
            ? " Check ASC_VENDOR_NUMBER (Payments and Financial Reports, top left)."
            : "";
    throw new Error(`App Store Connect answered ${response.status} for ${day}'s sales report.${hint}${await appleReason(response)}`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  // A gzip file, unless something on the way already unpacked it.
  if (bytes[0] !== 0x1f || bytes[1] !== 0x8b) return new TextDecoder().decode(bytes);
  return new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip"))).text();
}

/** Fetches the App Store's last week of daily sales summaries once a day; after a failure, again three hours later. */
export async function fetchAppStore(env: Env) {
  if (!appStoreConfigured(env) || !(await dailyDue(env.DB, "appstore"))) return;
  try {
    const token = await appStoreToken(env);
    const kinds = { downloads: new Map<string, number>(), redownloads: new Map<string, number>(), updates: new Map<string, number>() };
    for (let ago = APP_STORE_DAYS; ago >= 1; ago--) {
      const day = new Date(Date.now() - ago * 86_400_000).toISOString().slice(0, 10);
      const report = await appStoreReport(env, token, day);
      const units = report === null ? { downloads: 0, redownloads: 0, updates: 0 } : appStoreUnits(report);
      for (const kind of Object.keys(kinds) as (keyof typeof kinds)[]) kinds[kind].set(day, units[kind]);
    }
    await saveStoreCounts(env.DB, "appstore", kinds);
    await noteFetch(env.DB, "appstore");
  } catch (error) {
    await noteFetch(env.DB, "appstore", message(error));
  }
}
