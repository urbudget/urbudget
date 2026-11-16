// Installers live in R2: `latest.json` describes the newest release and `files/<name>` holds each
// installer and update package. `npm run release` uploads both.
import { Hono, type Context } from "hono";
import type { AppEnv, Env } from "./env";
import { ApiError } from "./util";

export interface Release {
  version: string;
  date: string;
  notes: string;
  platforms: Partial<Record<Platform, { file: string; size: number; sha256: string }>>;
  /** What installed copies update to, by Tauri target and architecture ("windows-x86_64",
   * "darwin-aarch64"), and on Linux by package too ("linux-x86_64-appimage", "linux-x86_64-deb").
   * Each keeps its own version: a system not published again stays put. */
  updates?: Record<string, Package>;
}
/** A package the app's updater installs, and the contents of its `.sig` file. */
export interface Package {
  version: string;
  /** When it was published, as an ISO timestamp. */
  date: string;
  notes: string;
  file: string;
  signature: string;
}
/** Linux has two installers: `linux` is the AppImage, `linux-deb` the Debian and Ubuntu package. */
type Platform = "windows" | "macos" | "linux" | "linux-deb";
const PLATFORMS: Platform[] = ["windows", "macos", "linux", "linux-deb"];

async function latest(env: Env): Promise<Release | null> {
  const object = await env.RELEASES.get("latest.json");
  return object ? ((await object.json()) as Release) : null;
}

export const releases = new Hono<AppEnv>();

releases.get("/latest", async (c) => {
  const release = await latest(c.env);
  if (!release) throw new ApiError(404, "no_release", "No version has been published yet.");
  const origin = c.env.APP_URL.replace(/\/+$/, "");
  const platforms = Object.fromEntries(
    PLATFORMS.map((p) => {
      const file = release.platforms[p];
      return [p, file ? { ...file, url: `${origin}/releases/${encodeURIComponent(file.file)}` } : null];
    }),
  );
  c.header("Cache-Control", "public, max-age=300");
  return c.json({ version: release.version, date: release.date, notes: release.notes, platforms });
});

/** Major, minor, patch and pre-release identifiers of a semantic version, or null. */
function semver(version: string) {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(version);
  if (!match) return null;
  return { core: [Number(match[1]), Number(match[2]), Number(match[3])], pre: match[4]?.split(".") ?? [] };
}
export const isVersion = (version: string) => version.length <= 32 && semver(version) !== null;

/** Semantic-version precedence: negative when `a` is older than `b`. */
export function compareVersions(a: string, b: string): number {
  const x = semver(a);
  const y = semver(b);
  if (!x || !y) throw new Error(`Not a version: ${x ? b : a}`);
  for (let i = 0; i < 3; i++) if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i];
  // A pre-release comes before its release; identifiers compare numerically when they're numbers.
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.min(x.pre.length, y.pre.length); i++) {
    const [p, q] = [x.pre[i], y.pre[i]];
    if (p === q) continue;
    const [pn, qn] = [/^\d+$/.test(p), /^\d+$/.test(q)];
    if (pn && qn) return Number(p) - Number(q);
    if (pn !== qn) return pn ? -1 : 1;
    return p < q ? -1 : 1;
  }
  return x.pre.length - y.pre.length;
}

/**
 * The app's updater (tauri-plugin-updater) asks here, as
 * `/update/{{target}}/{{arch}}/{{current_version}}`: 204 when that version is the newest for its
 * system, otherwise the newer version with its notes, package URL and signature. Linux copies add
 * `?bundle={{bundle_type}}` (appimage or deb): each updates with a package of its own kind, and a
 * copy that came from neither is offered nothing.
 */
const LAUNCH_PLATFORMS = new Set(["windows", "darwin", "linux"]);
const LAUNCH_ARCHES = new Set(["x86_64", "aarch64", "i686", "armv7"]);

/** Adds one to today's launches of this version on this system. Only the count is kept. */
export async function countLaunch(db: D1Database, platform: string, arch: string, version: string) {
  await db
    .prepare(
      `INSERT INTO app_launches (day, platform, arch, version, count) VALUES (?1, ?2, ?3, ?4, 1)
       ON CONFLICT(day, platform, arch, version) DO UPDATE SET count = count + 1`,
    )
    .bind(new Date().toISOString().slice(0, 10), platform, arch, version)
    .run();
}

releases.get("/update/:target/:arch/:current", async (c) => {
  const { target, arch, current } = c.req.param();
  if (!semver(current)) throw new ApiError(400, "invalid_version", "That isn't a version number.");
  // Copies ask a few seconds after they start (and once a day while open): about one ask a launch.
  if (c.req.method === "GET" && LAUNCH_PLATFORMS.has(target) && LAUNCH_ARCHES.has(arch)) {
    const counted = countLaunch(c.env.DB, target, arch, current).catch((error) => console.error("[launches]", error));
    try {
      c.executionCtx.waitUntil(counted);
    } catch {
      await counted;
    }
  }
  const updates = (await latest(c.env))?.updates;
  const bundle = c.req.query("bundle") ?? "";
  const update = (/^[a-z]{1,16}$/.test(bundle) ? updates?.[`${target}-${arch}-${bundle}`] : undefined) ?? updates?.[`${target}-${arch}`];
  c.header("Cache-Control", "public, max-age=300");
  if (!update || compareVersions(update.version, current) <= 0) return c.body(null, 204);
  const origin = c.env.APP_URL.replace(/\/+$/, "");
  return c.json({
    version: update.version,
    notes: update.notes,
    pub_date: update.date,
    url: `${origin}/releases/${encodeURIComponent(update.file)}`,
    signature: update.signature,
  });
});

// Crawlers, link checkers, scripts, headless browsers and AI agents' browsers (Claude's says
// "Claude/") follow download links too.
const AUTOMATED =
  /bot|crawl|spider|slurp|preview|fetch|scan|monitor|curl|wget|python|java\/|go-http|okhttp|axios|node|undici|headless|lighthouse|playwright|puppeteer|selenium|electron|claude|chatgpt|openai|anthropic|perplexity/i;

/** Set on a browser that opened the admin page, so its downloads count as the team's. */
export const TEAM_COOKIE = "ub_team";

export type DownloadKind = "person" | "team" | "bot";

/**
 * Who a download click came from, by kind only: the team (a browser that opened the admin page),
 * a bot (automation by its user agent, or a bare request with neither a referrer nor the fetch
 * metadata every current browser sends), or else a person.
 */
export function downloadKind(request: { agent?: string; referer?: string; fetchSite?: string; cookie?: string }): DownloadKind {
  if (new RegExp(`(^|;\\s*)${TEAM_COOKIE}=1(;|$)`).test(request.cookie ?? "")) return "team";
  if (!request.agent || AUTOMATED.test(request.agent)) return "bot";
  if (!request.referer && !request.fetchSite) return "bot";
  return "person";
}

/**
 * Where a download click came from, without anything about who clicked: the page of this site the
 * button was on, or the host name of the site that linked here, or '' when the browser didn't say.
 */
export function downloadSource(referer: string | undefined, origin: string): string {
  if (!referer) return "";
  try {
    const url = new URL(referer);
    if (url.origin === origin) return url.pathname.slice(0, 100);
    return /^https?:$/.test(url.protocol) ? url.hostname.replace(/^www\./, "").slice(0, 100) : "";
  } catch {
    return "";
  }
}

/**
 * The tag a marketing link carries (`urbudget.app/download/?ref=hn`, passed on to the buttons by
 * assets/ref.js): lowercase letters, digits and `-`, up to 32. Anything else is no tag.
 */
export function normalizeRef(value: string | undefined): string | null {
  const ref = value?.trim().toLowerCase() ?? "";
  return /^[a-z0-9-]{1,32}$/.test(ref) ? ref : null;
}

/** Adds one to today's count for this installer, source and kind. */
export async function countDownload(db: D1Database, platform: string, version: string, source: string, kind: DownloadKind) {
  await db
    .prepare(
      `INSERT INTO download_counts (day, platform, version, source, kind, count) VALUES (?1, ?2, ?3, ?4, ?5, 1)
       ON CONFLICT(day, platform, version, source, kind) DO UPDATE SET count = count + 1`,
    )
    .bind(new Date().toISOString().slice(0, 10), platform, version, source, kind)
    .run();
}

/** UrBudget's Store ID in Partner Center (as in .github/workflows/store.yml). */
export const STORE_ID = "9PMQMBZJG06R";
/** UrBudget in the Microsoft Store. */
export const STORE_URL = `https://apps.microsoft.com/detail/${STORE_ID}`;

/** Counts a click on a download button. Bots and the team are counted, but apart. */
async function countClick(c: Context<AppEnv>, platform: "windows" | "macos" | "linux", version: string, ref: string | null) {
  const referer = c.req.header("Referer");
  const kind = downloadKind({ agent: c.req.header("User-Agent"), referer, fetchSite: c.req.header("Sec-Fetch-Site"), cookie: c.req.header("Cookie") });
  // A link's tag says more than the page it was clicked on, so it wins: `ref:hn`.
  const source = ref ? `ref:${ref}` : downloadSource(referer, new URL(c.req.url).origin);
  const counted = countDownload(c.env.DB, platform, version, source, kind).catch((error) => console.error("[downloads]", error));
  // After the redirect has gone out; tests have no ExecutionContext and wait for it instead.
  try {
    c.executionCtx.waitUntil(counted);
  } catch {
    await counted;
  }
}

/**
 * `/download/windows`, `/download/macos`, `/download/linux` and `/download/linux-deb` (the newest
 * installer for that system) and `/download/store`, counted.
 */
export const downloads = new Hono<AppEnv>();

/**
 * `/download/store`: UrBudget in the Microsoft Store, the site's main Windows download, since Store
 * installs open without a SmartScreen warning. Counted as a Windows download whose version is
 * "Microsoft Store": the Store, not this site, decides which version it installs.
 */
downloads.get("/store", async (c) => {
  // HEAD requests are link checkers, not downloads.
  if (c.req.method === "GET") await countClick(c, "windows", "Microsoft Store", normalizeRef(c.req.query("ref")));
  c.header("Cache-Control", "no-store");
  return c.redirect(STORE_URL, 302);
});

downloads.get("/:platform{windows|macos|linux-deb|linux}", async (c) => {
  const platform = c.req.param("platform") as Platform;
  const release = await latest(c.env);
  const file = release?.platforms[platform];
  const ref = normalizeRef(c.req.query("ref"));
  // HEAD requests are link checkers, not downloads. Both Linux installers count as Linux.
  if (release && file && c.req.method === "GET") await countClick(c, platform === "linux-deb" ? "linux" : platform, release.version, ref);
  c.header("Cache-Control", "no-store");
  if (!file) return c.redirect(`/download/?unavailable=${platform}${ref ? `&ref=${ref}` : ""}`, 302);
  return c.redirect(`/releases/${encodeURIComponent(file.file)}`, 302);
});

/** Installer files, with range requests so interrupted downloads can resume. */
export const files = new Hono<AppEnv>();

files.on(["GET", "HEAD"], "/:file", async (c) => {
  const name = c.req.param("file");
  if (!/^[\w.\-+ ]{1,200}$/.test(name) || name === "latest.json") return c.notFound();
  const object = await c.env.RELEASES.get(`files/${name}`, { range: c.req.raw.headers, onlyIf: c.req.raw.headers });
  if (!object) return c.notFound();
  const headers = new Headers();
  object.writeHttpMetadata(headers);
  headers.set("ETag", object.httpEtag);
  headers.set("Accept-Ranges", "bytes");
  headers.set("Cache-Control", "public, max-age=31536000, immutable");
  headers.set("Content-Disposition", `attachment; filename="${name}"`);
  if (!headers.has("Content-Type")) headers.set("Content-Type", "application/octet-stream");
  if (!("body" in object)) return new Response(null, { status: 304, headers });
  const range = object.range as { offset?: number; length?: number; suffix?: number } | undefined;
  if (range && c.req.header("Range")) {
    const offset = range.suffix !== undefined ? object.size - range.suffix : (range.offset ?? 0);
    const length = range.suffix ?? range.length ?? object.size - offset;
    headers.set("Content-Range", `bytes ${offset}-${offset + length - 1}/${object.size}`);
    headers.set("Content-Length", String(length));
    return new Response(c.req.method === "HEAD" ? null : object.body, { status: 206, headers });
  }
  headers.set("Content-Length", String(object.size));
  return new Response(c.req.method === "HEAD" ? null : object.body, { headers });
});
