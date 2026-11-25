import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { signWebhook } from "../src/billing/polar";
import worker, { app } from "../src/index";
import { normalizeRef } from "../src/releases";
import { base64url, mondayOf, utf8 } from "../src/util";
import { testEnv, type TestEnv } from "./fakes";

let env: TestEnv;
let fetches: { url: string; method: string; headers: Headers }[];
let resendStatus = 200;
let polarMetricsStatus = 200;

// Cloudflare Access signs with RS256; this pair stands in for the team's key.
let accessPair: CryptoKeyPair;
let accessJwk: JsonWebKey;
beforeAll(async () => {
  accessPair = (await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  )) as CryptoKeyPair;
  accessJwk = await crypto.subtle.exportKey("jwk", accessPair.publicKey);
});

beforeEach(async () => {
  env = await testEnv();
  fetches = [];
  resendStatus = 200;
  polarMetricsStatus = 200;
  vi.spyOn(console, "log").mockImplementation(() => {});
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    fetches.push({ url, method: init?.method ?? "GET", headers: new Headers(init?.headers) });
    if (url === "https://urbudget.cloudflareaccess.com/cdn-cgi/access/certs")
      return Response.json({ keys: [{ ...accessJwk, kid: "key-1" }] });
    if (url === "https://api.resend.com/emails")
      return resendStatus === 200 ? Response.json({ id: "email_1" }) : new Response("nope", { status: resendStatus });
    if (url.startsWith("https://sandbox-api.polar.sh/v1/metrics/")) {
      if (polarMetricsStatus !== 200) return new Response("{}", { status: polarMetricsStatus });
      return Response.json({
        periods: [
          { timestamp: "2026-10-16T00:00:00Z", revenue: 900, orders: 1, monthly_recurring_revenue: 200 },
          { timestamp: "2026-10-17T00:00:00Z", revenue: 1800, orders: 2, monthly_recurring_revenue: 400 },
        ],
        totals: { revenue: 2700, orders: 3, monthly_recurring_revenue: 400 },
        metrics: {
          revenue: { slug: "revenue", display_name: "Revenue", type: "currency" },
          orders: { slug: "orders", display_name: "Orders", type: "scalar" },
          monthly_recurring_revenue: { slug: "monthly_recurring_revenue", display_name: "Monthly Recurring Revenue", type: "currency" },
        },
      });
    }
    if (url === "https://sandbox-api.polar.sh/v1/checkouts/")
      return Response.json({ id: "co_1", url: "https://sandbox.polar.sh/checkout/co_1" }, { status: 201 });
    if (url.startsWith("https://api.github.com/repos/urbudget/urbudget/releases"))
      return Response.json([
        {
          tag_name: "v1.0.1",
          name: "1.0.1",
          draft: false,
          prerelease: false,
          published_at: "2026-10-17T00:00:00Z",
          assets: [{ name: "UrBudget_1.0.1_x64-setup.exe", size: 5, download_count: 42 }],
        },
        { tag_name: "v1.0.2", name: null, draft: true, prerelease: false, published_at: null, assets: [] },
      ]);
    return new Response("unexpected", { status: 500 });
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const call = (path: string, init: RequestInit = {}) => app.request(path, init, env);
const ORIGIN = { Origin: "http://localhost" };
const post = (body: unknown, headers: Record<string, string> = ORIGIN): RequestInit => ({
  method: "POST",
  headers: { "Content-Type": "application/json", ...headers },
  body: JSON.stringify(body),
});
const read = async <T = Record<string, unknown>>(response: Response | Promise<Response>) => (await (await response).json()) as T;

async function webhook(type: string, data: Record<string, unknown>) {
  const body = JSON.stringify({ type, timestamp: new Date().toISOString(), data });
  const id = `msg_${Math.random().toString(36).slice(2)}`;
  const ts = Math.floor(Date.now() / 1000);
  return call("/api/v1/billing/webhook", {
    method: "POST",
    body,
    headers: { "webhook-id": id, "webhook-timestamp": String(ts), "webhook-signature": await signWebhook(env.POLAR_WEBHOOK_SECRET!, id, ts, body) },
  });
}

describe("admin access", () => {
  async function accessToken(claims: Record<string, unknown>, kid = "key-1") {
    const header = base64url(utf8(JSON.stringify({ alg: "RS256", kid, typ: "JWT" })));
    const payload = base64url(utf8(JSON.stringify(claims)));
    const signature = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", accessPair.privateKey, utf8(`${header}.${payload}`));
    return `${header}.${payload}.${base64url(signature)}`;
  }
  const seconds = () => Math.floor(Date.now() / 1000);
  const claims = (overrides: Record<string, unknown> = {}) => ({
    aud: ["aud-tag"],
    email: "Owner@Example.com",
    iss: "https://urbudget.cloudflareaccess.com",
    iat: seconds(),
    nbf: seconds(),
    exp: seconds() + 3600,
    ...overrides,
  });

  beforeEach(() => {
    // Production: no local-development shortcut, Access in front.
    env.DEV_LOGIN_CODES = undefined;
    env.ACCESS_TEAM_DOMAIN = "urbudget";
    env.ACCESS_AUD = "aud-tag";
    env.ADMIN_EMAILS = "owner@example.com, helper@example.com";
  });

  it("lets in an admin with a valid Access token, and nobody else", async () => {
    const as = async (token: string) => call("/api/v1/admin/overview", { headers: { "Cf-Access-Jwt-Assertion": token } });
    expect((await call("/api/v1/admin/overview")).status).toBe(403);
    const ok = await as(await accessToken(claims()));
    expect(ok.status).toBe(200);
    expect(await read(ok)).toMatchObject({ admin: "owner@example.com" });

    expect((await as(await accessToken(claims({ email: "stranger@example.com" })))).status).toBe(403);
    expect((await as(await accessToken(claims({ aud: ["another-app"] })))).status).toBe(403);
    expect((await as(await accessToken(claims({ iss: "https://evil.cloudflareaccess.com" })))).status).toBe(403);
    expect((await as(await accessToken(claims({ exp: seconds() - 600 })))).status).toBe(403);
    expect((await as(await accessToken(claims(), "unknown-key"))).status).toBe(403);
    const [h, , s] = (await accessToken(claims())).split(".");
    const forged = `${h}.${base64url(utf8(JSON.stringify(claims({ email: "helper@example.com", aud: "aud-tag" }))))}.${s}`;
    expect((await as(forged)).status).toBe(403);
  });

  it("opens without Access only in local development, never on the real site", async () => {
    env.DEV_LOGIN_CODES = "1";
    env.APP_URL = "https://urbudget.app";
    expect((await call("/api/v1/admin/overview")).status).toBe(403);
    env.APP_URL = "http://localhost:8787";
    expect((await call("/api/v1/admin/overview")).status).toBe(200);
  });

  it("stays closed until Access is configured, even with a token", async () => {
    env.ACCESS_AUD = "";
    const response = await call("/api/v1/admin/overview", { headers: { "Cf-Access-Jwt-Assertion": await accessToken(claims()) } });
    expect(response.status).toBe(403);
  });

  it("guards the page itself, and takes changes only from the admin page", async () => {
    const refused = await call("/admin/");
    expect(refused.status).toBe(403);
    expect(refused.headers.get("Set-Cookie")).toBeNull();
    const headers = { "Cf-Access-Jwt-Assertion": await accessToken(claims()) };
    const page = await call("/admin/", { headers });
    expect(await page.text()).toBe("static");
    expect(page.headers.get("X-Robots-Tag")).toContain("noindex");
    expect(page.headers.get("Set-Cookie")).toMatch(/^ub_team=1; Path=\/;.*HttpOnly/);
    env.DB.sqlite.prepare("INSERT INTO assistant_reports (id, created_at, reply) VALUES ('rep_1', ?, 'A reply')").run(new Date().toISOString());
    const remove = (origin: string) => call("/api/v1/admin/assistant-reports/rep_1", { method: "DELETE", headers: { ...headers, Origin: origin } });
    expect((await remove("https://evil.example")).status).toBe(403);
    expect((await remove("http://localhost")).status).toBe(200);
    const { entries } = await read<{ entries: { admin: string; action: string; detail: string }[] }>(
      call("/api/v1/admin/log", { headers }),
    );
    expect(entries).toEqual([expect.objectContaining({ admin: "owner@example.com", action: "delete_assistant_report", detail: "rep_1" })]);
  });
});

describe("admin numbers", () => {
  async function publish() {
    await env.RELEASES.put(
      "latest.json",
      JSON.stringify({ version: "1.0.1", date: "2026-10-17", notes: "", platforms: { windows: { file: "UrBudget_1.0.1_x64-setup.exe", size: 5, sha256: "abc" } } }),
    );
  }
  const browser = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0 Safari/537.36";

  it("counts download clicks by day, version and source, but not crawlers'", async () => {
    await publish();
    env.GITHUB_REPO = "urbudget/urbudget";
    await call("/download/windows", { headers: { "User-Agent": browser, Referer: "http://localhost/download/" } });
    await call("/download/windows", { headers: { "User-Agent": browser, Referer: "https://www.reddit.com/r/ynab/comments/x" } });
    await call("/download/windows", { headers: { "User-Agent": browser, "Sec-Fetch-Site": "none" } });
    await call("/download/windows", { headers: { "User-Agent": "Googlebot/2.1 (+http://www.google.com/bot.html)" } });
    await call("/download/windows");
    await call("/download/macos", { headers: { "User-Agent": browser } }); // no macOS build: not a download

    const downloads = await read<{
      website: { windows: number[]; macos: number[] };
      sources: { source: string; n: number }[];
      versions: { version: string; n: number }[];
      latest: { version: string };
      github: { releases: { tag: string; assets: { downloads: number }[] }[] };
    }>(call("/api/v1/admin/downloads"));
    expect(downloads.website.windows.at(-1)).toBe(3);
    expect(downloads.website.macos.at(-1)).toBe(0);
    expect(downloads.sources).toEqual(
      expect.arrayContaining([{ source: "/download/", n: 1 }, { source: "reddit.com", n: 1 }, { source: "", n: 1 }]),
    );
    expect(downloads.versions).toEqual([expect.objectContaining({ version: "1.0.1", platform: "windows", n: 3 })]);
    expect(downloads.latest.version).toBe("1.0.1");
    expect(downloads.github.releases).toEqual([expect.objectContaining({ tag: "v1.0.1", assets: [expect.objectContaining({ downloads: 42 })] })]);
    // No token set in tests: the request goes without one.
    expect(fetches.find((f) => f.url.includes("api.github.com"))?.headers.has("Authorization")).toBe(false);

    const overview = await read<{ downloads: { last30: number; windows: number; bots: number; team: number } }>(call("/api/v1/admin/overview"));
    expect(overview.downloads).toMatchObject({ last30: 3, windows: 3, bots: 2, team: 0 });
  });

  it("counts both Linux installers as Linux downloads, in the totals", async () => {
    await env.RELEASES.put(
      "latest.json",
      JSON.stringify({
        version: "1.0.13",
        date: "2026-11-17",
        notes: "",
        platforms: {
          linux: { file: "UrBudget_1.0.13_amd64.AppImage", size: 5, sha256: "abc" },
          "linux-deb": { file: "UrBudget_1.0.13_amd64.deb", size: 5, sha256: "def" },
        },
      }),
    );
    const page = { "User-Agent": browser.replace("Windows NT 10.0; Win64; x64", "X11; Linux x86_64"), Referer: "http://localhost/download/" };
    await call("/download/linux", { headers: page });
    await call("/download/linux-deb", { headers: page });

    const d = await read<{ website: { windows: number[]; linux: number[] }; total: number[]; versions: { version: string; platform: string; n: number }[] }>(
      call("/api/v1/admin/downloads"),
    );
    expect(d.website.linux.at(-1)).toBe(2);
    expect(d.website.windows.at(-1)).toBe(0);
    expect(d.total.at(-1)).toBe(2);
    expect(d.versions).toEqual([expect.objectContaining({ version: "1.0.13", platform: "linux", n: 2 })]);
    const overview = await read<{ downloads: { last30: number; website: number; linux: number; windows: number } }>(call("/api/v1/admin/overview"));
    expect(overview.downloads).toMatchObject({ last30: 2, website: 2, linux: 2, windows: 0 });
  });

  it("counts people apart from bots, AI agents' browsers and the team", async () => {
    await publish();
    const page = { Referer: "http://localhost/download/" };
    // People: a browser on a page of the site, or opened straight to the link.
    await call("/download/windows", { headers: { ...page, "User-Agent": browser } });
    await call("/download/windows", { headers: { "User-Agent": browser, "Sec-Fetch-Site": "none" } });
    // Bots: an AI agent's browser, a headless one, and a bare request that only looks like a browser.
    await call("/download/windows", { headers: { ...page, "User-Agent": `${browser.replace("Chrome/", "Claude/2.42304.0 Chrome/")} MSIX` } });
    await call("/download/windows", { headers: { ...page, "User-Agent": browser.replace("Chrome/", "HeadlessChrome/") } });
    await call("/download/windows", { headers: { "User-Agent": browser } });
    // The team: a browser that opened the admin page.
    await call("/download/windows", { headers: { ...page, "User-Agent": browser, Cookie: "theme=dark; ub_team=1" } });

    const d = await read<{ website: { windows: number[] }; others: { team: { last30: number }; bots: { last30: number } }; sources: { source: string; n: number }[] }>(
      call("/api/v1/admin/downloads"),
    );
    expect(d.website.windows.at(-1)).toBe(2);
    expect(d.others).toEqual({ team: { last30: 1, last90: 1 }, bots: { last30: 3, last90: 3 } });
    expect(d.sources.reduce((n, s) => n + s.n, 0)).toBe(2);
    const overview = await read<{ downloads: { last30: number; bots: number; team: number } }>(call("/api/v1/admin/overview"));
    expect(overview.downloads).toMatchObject({ last30: 2, bots: 3, team: 1 });
  });

  it("counts a marketing link's ?ref= tag as the source, and never a link checker's HEAD", async () => {
    await publish();
    const page = { "User-Agent": browser, Referer: "http://localhost/download/?ref=hn" };
    for (const ref of ["HN", "hn", "not_a_tag"]) await call(`/download/windows?ref=${ref}`, { headers: page });
    await call("/download/windows?ref=hn", { method: "HEAD", headers: page });
    // Nothing published for macOS: back to the download page, with the tag still on.
    expect((await call("/download/macos?ref=hn", { headers: page })).headers.get("Location")).toBe("/download/?unavailable=macos&ref=hn");

    const { sources } = await read<{ sources: { source: string; n: number }[] }>(call("/api/v1/admin/downloads"));
    // A bad tag falls back to the page the button was on.
    expect(sources).toEqual(expect.arrayContaining([{ source: "ref:hn", n: 2 }, { source: "/download/", n: 1 }]));
    expect(sources.reduce((n, s) => n + s.n, 0)).toBe(3);
  });

  it("counts the Microsoft Store button apart, as the Store's downloads until Partner Center answers", async () => {
    const page = { "User-Agent": browser, Referer: "http://localhost/download/" };
    const store = await call("/download/store", { headers: page });
    expect(store.status).toBe(302);
    expect(store.headers.get("Location")).toBe("https://apps.microsoft.com/detail/9PMQMBZJG06R");
    await call("/download/store?ref=hn", { headers: page });
    await call("/download/store", { method: "HEAD", headers: page });
    await call("/download/store"); // a bare request: a bot

    const d = await read<{
      website: { windows: number[]; store: number[] };
      total: number[];
      versions: { version: string; platform: string; n: number }[];
      sources: { source: string; n: number }[];
    }>(call("/api/v1/admin/downloads"));
    expect(d.website.windows.at(-1)).toBe(0);
    expect(d.website.store.at(-1)).toBe(2);
    expect(d.total.at(-1)).toBe(2);
    const overview = await read<{ downloads: { last30: number; store: number; windows: number; storeSource: string } }>(call("/api/v1/admin/overview"));
    expect(overview.downloads).toMatchObject({ last30: 2, store: 2, windows: 2, storeSource: "clicks" });
    expect(d.versions).toEqual([expect.objectContaining({ version: "Microsoft Store", platform: "windows", n: 2 })]);
    expect(d.sources).toEqual(expect.arrayContaining([{ source: "ref:hn", n: 1 }, { source: "/download/", n: 1 }]));
  });

  it("counts page views by page and source, people apart from bots and the team", async () => {
    const beacon = (body: unknown, headers: Record<string, string> = {}) =>
      call("/api/v1/visits", {
        method: "POST",
        headers: { "Content-Type": "text/plain;charset=UTF-8", "User-Agent": browser, Referer: "http://localhost/", ...headers },
        body: typeof body === "string" ? body : JSON.stringify(body),
      });
    expect((await beacon({ path: "/", referrer: "https://www.reddit.com/" })).status).toBe(204);
    await beacon({ path: "/", referrer: "", ref: "HN" });
    await beacon({ path: "/pricing/", referrer: "http://localhost/" });
    await beacon({ path: "/vs/ynab/", referrer: "" });
    // Bots and the team are counted apart; nonsense isn't counted at all.
    await beacon({ path: "/", referrer: "" }, { "User-Agent": browser.replace("Chrome/", "HeadlessChrome/") });
    await beacon({ path: "/", referrer: "" }, { Cookie: "ub_team=1" });
    for (const junk of [{ path: "/wp-login.php" }, { path: "https://evil.example/" }, { path: "/".repeat(80) }, "not json", { referrer: "x" }])
      expect((await beacon(junk)).status).toBe(204);

    const t = await read<{
      views: number[];
      pages: { path: string; n: number }[];
      sources: { source: string; n: number }[];
      last30: { people: number; team: number; bots: number; downloads: number };
    }>(call("/api/v1/admin/traffic"));
    expect(t.views.at(-1)).toBe(4);
    expect(t.pages).toEqual([{ path: "/", n: 2 }, { path: "/pricing/", n: 1 }, { path: "/vs/ynab/", n: 1 }]);
    // Moves between pages of the site aren't a source.
    expect(t.sources).toEqual(expect.arrayContaining([{ source: "reddit.com", n: 1 }, { source: "ref:hn", n: 1 }, { source: "", n: 1 }]));
    expect(t.sources.some((r) => r.source === "internal")).toBe(false);
    expect(t.last30).toMatchObject({ people: 4, team: 1, bots: 1 });
    const overview = await read<{ pageViews: { last30: number; home: number } }>(call("/api/v1/admin/overview"));
    expect(overview.pageViews).toEqual({ last30: 4, home: 2 });
  });

  it("counts app launches from update checks, per day, system and version", async () => {
    for (const path of ["windows/x86_64/1.0.2", "windows/x86_64/1.0.2", "darwin/aarch64/1.0.2", "darwin/x86_64/1.0.3", "linux/x86_64/1.0.13?bundle=appimage", "linux/x86_64/1.0.13?bundle=deb", "linux/x86_64/1.0.13"])
      expect([200, 204]).toContain((await call(`/api/v1/releases/update/${path}`)).status);
    // Not launches: a HEAD, a system or chip the app isn't built for, and a version that isn't one.
    await call("/api/v1/releases/update/windows/x86_64/1.0.2", { method: "HEAD" });
    await call("/api/v1/releases/update/amiga/m68k/1.0.2");
    expect((await call("/api/v1/releases/update/windows/x86_64/latest")).status).toBe(400);

    const { launches } = await read<{ launches: { windows: number[]; macos: number[]; linux: number[]; versions: { version: string; platform: string; n: number }[] } }>(
      call("/api/v1/admin/usage"),
    );
    expect(launches.windows.at(-1)).toBe(2);
    expect(launches.macos.at(-1)).toBe(2);
    expect(launches.linux.at(-1)).toBe(3);
    expect(launches.versions).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ version: "1.0.2", platform: "windows", n: 2 }),
        expect.objectContaining({ version: "1.0.2", platform: "darwin", n: 1 }),
        expect.objectContaining({ version: "1.0.3", platform: "darwin", n: 1 }),
        expect.objectContaining({ version: "1.0.13", platform: "linux", n: 3 }),
      ]),
    );
  });

  it("keeps link tags to short lowercase words", () => {
    expect(normalizeRef("hn")).toBe("hn");
    expect(normalizeRef(" Reddit-MacApps ")).toBe("reddit-macapps");
    expect(normalizeRef("a".repeat(32))).toBe("a".repeat(32));
    for (const bad of [undefined, "", "a".repeat(33), "reddit_macapps", "../etc", "café", "hn&x=1", "<script>"]) expect(normalizeRef(bad)).toBeNull();
  });

  it("shows Polar's revenue numbers, and says when the token lacks the scope", async () => {
    const revenue = await read<{ totals: Record<string, number>; metrics: { slug: string; type: string }[]; periods: unknown[]; interval: string }>(
      call("/api/v1/admin/revenue?days=30"),
    );
    expect(revenue.interval).toBe("day");
    expect(revenue.totals.revenue).toBe(2700);
    expect(revenue.metrics).toEqual(expect.arrayContaining([{ slug: "revenue", name: "Revenue", type: "currency" }]));
    expect(revenue.periods).toHaveLength(2);
    const asked = new URL(fetches.find((f) => f.url.includes("/v1/metrics/"))!.url);
    expect(asked.searchParams.getAll("metrics")).toContain("monthly_recurring_revenue");

    polarMetricsStatus = 403;
    const refused = await call("/api/v1/admin/revenue?days=90");
    expect(await read(refused)).toMatchObject({ error: "missing_scope" });
  });

  it("collects problems: rate limits, refused emails and webhooks that don't verify", async () => {
    for (let i = 0; i < 7; i++) await call("/api/v1/auth/start", post({ email: "ana@example.com" }));
    env.RESEND_API_KEY = "re_test";
    resendStatus = 422;
    const failed = await call("/api/v1/auth/start", post({ email: "ben@example.com" }));
    expect(failed.status).toBe(502);
    await call("/api/v1/billing/webhook", {
      method: "POST",
      body: "{}",
      headers: { "webhook-id": "msg_bad", "webhook-timestamp": String(Math.floor(Date.now() / 1000)), "webhook-signature": "v1,AAAA" },
    });

    const health = await read<{ problems: { kind: string; subject: string; detail: string }[]; settings: { name: string; ok: boolean }[] }>(
      call("/api/v1/admin/health"),
    );
    expect(health.problems.map((p) => p.kind).sort()).toEqual(["email", "rate_limit", "webhook"]);
    const email = health.problems.find((p) => p.kind === "email")!;
    expect(email.subject).toBe("ben@example.com");
    // The subject line holds the code; only the kind of email is kept.
    expect(email.detail).toMatch(/^sign-in code: Resend answered 422/);
    expect(health.settings.find((s) => s.name === "RESEND_API_KEY")?.ok).toBe(true);

    const overview = await read<{ health: { problems30: Record<string, number> } }>(call("/api/v1/admin/overview"));
    expect(overview.health.problems30).toEqual({ email: 1, rate_limit: 1, webhook: 1 });
  });
});

const subscription = (overrides: Record<string, unknown> = {}) => ({
  id: "sub_01",
  status: "active",
  customer_id: "cus_ana",
  customer: { email: "ana@example.com", external_id: null },
  product_id: "prod-sync",
  current_period_end: "2026-11-16T00:00:00Z",
  cancel_at_period_end: false,
  ends_at: null,
  ended_at: null,
  metadata: { plan: "sync" },
  modified_at: new Date().toISOString(),
  ...overrides,
});

async function signIn(email: string, client: "web" | "desktop") {
  const { devCode } = await read<{ devCode: string }>(call("/api/v1/auth/start", post({ email })));
  const verify = await call("/api/v1/auth/verify", post({ email, code: devCode, client, deviceName: "Test PC" }));
  expect(verify.status).toBe(200);
  const body = await read<{ token?: string; user: { id: string } }>(verify);
  return { userId: body.user.id, token: body.token ?? "" };
}

const upload = (token: string, revision: number, bytes: string) =>
  call("/api/v1/sync/budgets/b1", {
    method: "PUT",
    body: bytes,
    headers: { Authorization: `Bearer ${token}`, "If-Match": String(revision), "X-Sync-Meta": "meta", "X-Device": "Laptop" },
  });

const today = () => new Date().toISOString().slice(0, 10);
const daysAgo = (n: number) => new Date(Date.now() - n * 86_400_000).toISOString().slice(0, 10);

describe("admin tracking", () => {
  it("counts sign-ins, emails, Sync checkouts, sync uploads and deleted accounts per day, and nothing about who", async () => {
    env.RESEND_API_KEY = "re_test";
    const ana = await signIn("ana@example.com", "desktop");
    expect((await call("/api/v1/billing/checkout", { ...post({ plan: "sync" }), headers: { "Content-Type": "application/json", Authorization: `Bearer ${ana.token}` } })).status).toBe(200);
    await webhook("subscription.created", subscription({ customer: { email: "ana@example.com", external_id: ana.userId } }));
    await call("/api/v1/sync/key", { method: "PUT", headers: { "Content-Type": "application/json", Authorization: `Bearer ${ana.token}` }, body: JSON.stringify({ salt: "c2FsdHNhbHRzYWx0c2FsdA", check: "encrypted-check" }) });
    expect((await upload(ana.token, 0, "first")).status).toBe(200);
    expect((await upload(ana.token, 0, "stale")).status).toBe(409);
    const ben = await signIn("ben@example.com", "desktop");
    const deleted = await call("/api/v1/me", { ...post({ confirm: "ben@example.com" }), method: "DELETE", headers: { "Content-Type": "application/json", Authorization: `Bearer ${ben.token}` } });
    expect(deleted.status).toBe(200);

    const health = await read<{
      activity: { name: string; detail: string; last7: number; last30: number }[];
      emails: { today: number; month: number; perDay: number; perMonth: number };
      countingSince: string;
    }>(call("/api/v1/admin/health"));
    const counted = Object.fromEntries(health.activity.map((a) => [`${a.name}:${a.detail}`, a.last30]));
    expect(counted).toEqual({
      "account_deleted:": 1,
      "checkout:sync": 1,
      "email:sign-in code": 2,
      "sign_in:desktop": 2,
      "sync_upload:conflict": 1,
      "sync_upload:ok": 1,
    });
    expect(health.activity.every((a) => a.last7 === a.last30)).toBe(true);
    expect(health.emails).toEqual({ today: 2, month: 2, perDay: 100, perMonth: 3000 });
    expect(health.countingSince).toBe(today());

    // Only totals: no address, account or device anywhere in what's kept.
    const rows = env.DB.sqlite.prepare("SELECT * FROM events").all();
    expect(Object.keys(rows[0]).sort()).toEqual(["count", "day", "detail", "name"]);
    expect(JSON.stringify(rows)).not.toMatch(/example\.com|usr_|Test PC|Laptop/);

    const overview = await read<{ funnel: { checkouts: number }; weeks: { checkouts: number }[]; health: { emails: { today: number } } }>(
      call("/api/v1/admin/overview"),
    );
    expect(overview.funnel.checkouts).toBe(1);
    expect(overview.weeks[0].checkouts).toBe(1);
    expect(overview.health.emails.today).toBe(2);
  });

  it("compares the last 30 days with the 30 before, and adds the same numbers up by week", async () => {
    const download = env.DB.sqlite.prepare("INSERT INTO download_counts (day, platform, version, source, kind, count) VALUES (?, ?, '1.0.3', ?, ?, ?)");
    download.run(today(), "windows", "ref:hn", "person", 3);
    download.run(today(), "macos", "ref:reddit", "person", 1);
    download.run(today(), "windows", "ref:hn", "bot", 5);
    download.run(daysAgo(40), "windows", "", "person", 4);
    const view = env.DB.sqlite.prepare("INSERT INTO page_views (day, path, source, kind, count) VALUES (?, '/', ?, 'person', ?)");
    view.run(today(), "ref:hn", 6);
    view.run(daysAgo(40), "", 2);
    const opened = env.DB.sqlite.prepare("INSERT INTO app_usage (day, platform, channel, version, kind, cohort, count) VALUES (?, 'windows', ?, '1.0.9', 'install', '', ?)");
    opened.run(today(), "direct", 1);
    opened.run(today(), "store", 1);
    opened.run(daysAgo(45), "direct", 1);

    const o = await read<{
      downloads: { last30: number };
      pageViews: { last30: number };
      usage: { opens30: number };
      previous30: Record<string, number>;
      funnel: Record<string, number>;
      weeks: { start: string; downloads: number; views: number; opens: number; tags: { tag: string; n: number }[] }[];
    }>(call("/api/v1/admin/overview"));
    expect([o.downloads.last30, o.pageViews.last30, o.usage.opens30]).toEqual([4, 6, 2]);
    expect(o.previous30).toEqual({ downloads: 4, pageViews: 2, opens: 1, checkouts: 0, accounts: 0 });
    expect(o.funnel).toEqual({ views: 6, downloads: 4, opens: 2, checkouts: 0 });

    expect(o.weeks).toHaveLength(12);
    const monday = new Date(`${o.weeks[0].start}T00:00:00Z`);
    expect(monday.getUTCDay()).toBe(1);
    expect(Date.now() - monday.getTime()).toBeLessThan(7 * 86_400_000);
    expect(o.weeks[0]).toMatchObject({ downloads: 4, views: 6, opens: 2, tags: [{ tag: "hn", n: 3 }, { tag: "reddit", n: 1 }] });
    expect(o.weeks.reduce((n, w) => n + w.downloads, 0)).toBe(8);
    expect(o.weeks.reduce((n, w) => n + w.views, 0)).toBe(8);
  });

  it("lists every link tag with the visits that came with it and the downloads after", async () => {
    const browser = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0 Safari/537.36";
    const beacon = (ref: string) =>
      call("/api/v1/visits", {
        method: "POST",
        headers: { "Content-Type": "text/plain;charset=UTF-8", "User-Agent": browser, Referer: "http://localhost/" },
        body: JSON.stringify({ path: "/", referrer: "", ref }),
      });
    for (const ref of ["hn", "hn", "hn", "hn", "tool-loan"]) await beacon(ref);
    const page = { "User-Agent": browser, Referer: "http://localhost/download/" };
    await call("/download/store?ref=hn", { headers: page });
    await call("/download/store?ref=lobsters", { headers: page });
    await call("/download/store?ref=hn", { headers: { ...page, Cookie: "ub_team=1" } }); // the team's own click

    const { tags } = await read<{ tags: { tag: string; views: number; downloads: number; first: string; last: string }[] }>(call("/api/v1/admin/traffic"));
    expect(tags).toEqual(
      expect.arrayContaining([
        { tag: "hn", views: 4, downloads: 1, first: today(), last: today() },
        { tag: "tool-loan", views: 1, downloads: 0, first: today(), last: today() },
        { tag: "lobsters", views: 0, downloads: 1, first: today(), last: today() },
      ]),
    );
    expect(tags).toHaveLength(3);
  });

  it("lists link tags last seen the same day by how much they brought in, in total", async () => {
    const view = env.DB.sqlite.prepare("INSERT INTO page_views (day, path, source, kind, count) VALUES (?, '/', ?, 'person', ?)");
    view.run(today(), "ref:big", 1);
    view.run(daysAgo(9), "ref:big", 100);
    view.run(daysAgo(8), "ref:big", 100);
    view.run(today(), "ref:small", 5);
    view.run(daysAgo(1), "ref:older", 500);

    const { tags } = await read<{ tags: { tag: string; views: number }[] }>(call("/api/v1/admin/traffic"));
    expect(tags.map((t) => [t.tag, t.views])).toEqual([
      ["big", 201],
      ["small", 5],
      ["older", 500],
    ]);
  });

  it("notes when the daily clean-up last ran", async () => {
    expect((await read<{ lastCleanup: string | null }>(call("/api/v1/admin/health"))).lastCleanup).toBeNull();
    await worker.scheduled({} as ScheduledController, env);
    expect((await read<{ lastCleanup: string | null }>(call("/api/v1/admin/health"))).lastCleanup).toBe(today());
    const overview = await read<{ health: { lastCleanup: string | null } }>(call("/api/v1/admin/overview"));
    expect(overview.health.lastCleanup).toBe(today());
  });
});

describe("admin accounts", () => {
  it("finds an account and shows what support needs, but never a session's secret", async () => {
    const ana = await signIn("ana@example.com", "desktop");
    await signIn("ben@example.com", "web");
    await webhook("subscription.created", subscription({ customer: { email: "ana@example.com", external_id: ana.userId } }));
    await call("/api/v1/sync/key", { method: "PUT", headers: { "Content-Type": "application/json", Authorization: `Bearer ${ana.token}` }, body: JSON.stringify({ salt: "c2FsdHNhbHRzYWx0c2FsdA", check: "encrypted-check" }) });
    expect((await upload(ana.token, 0, "bytes")).status).toBe(200);

    type Row = { id: string; email: string; subscription: string | null; devices: number; budgets: number; lastSeenAt: string | null };
    const list = async (query: string) => read<{ accounts: Row[]; total: number }>(call(`/api/v1/admin/accounts?${query}`));
    const emails = async (query: string) => (await list(query)).accounts.map((a) => a.email);
    const found = await list("q=ANA");
    expect(found.total).toBe(1);
    expect(found.accounts[0]).toEqual({ id: ana.userId, email: "ana@example.com", createdAt: expect.any(String), subscription: "active", devices: 1, budgets: 1, lastSeenAt: expect.any(String) });
    expect(found.accounts[0].lastSeenAt).toBeTruthy();
    expect(await emails("q=cus_ana")).toEqual(["ana@example.com"]);
    expect(await emails("filter=sync")).toEqual(["ana@example.com"]);
    expect(await emails("filter=app")).toEqual(["ana@example.com"]);
    expect(await emails("filter=nothing")).toEqual(["ben@example.com"]);
    expect((await list("q=100%25")).total).toBe(0);
    // A name an object inherits isn't a filter.
    expect((await list("filter=constructor")).total).toBe(2);

    const detail = await read<{
      account: { email: string; customerId: string };
      subscriptions: { status: string; live: boolean }[];
      sessions: { kind: string; deviceName: string | null }[];
      budgets: { id: string; revision: number; size: number; device: string }[];
    }>(call(`/api/v1/admin/accounts/${ana.userId}`));
    expect(detail.account).toMatchObject({ email: "ana@example.com", customerId: "cus_ana" });
    expect(detail.subscriptions).toEqual([expect.objectContaining({ status: "active", live: true })]);
    expect(detail.sessions).toEqual([expect.objectContaining({ kind: "desktop", deviceName: "Test PC" })]);
    expect(detail.budgets).toEqual([expect.objectContaining({ id: "b1", revision: 1, size: 5, device: "Laptop" })]);
    // Neither the token the app holds nor its hash (the session's id).
    const session = env.DB.sqlite.prepare("SELECT id FROM sessions WHERE user_id = ?").get(ana.userId) as { id: string };
    const raw = JSON.stringify(detail);
    expect(raw).not.toContain(ana.token);
    expect(raw).not.toContain(session.id);

    expect((await call("/api/v1/admin/accounts/usr_nobody")).status).toBe(404);
  });
});
