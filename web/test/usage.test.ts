import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker, { app } from "../src/index";
import { installerPlatform } from "../src/channels";
import { mondayOf } from "../src/util";
import { testEnv, type TestEnv } from "./fakes";

let env: TestEnv;
let fetches: { url: string; method: string; body: string }[];
let github: { status: number; releases: unknown[] };
let store: { token: number; report: number; acquisitions: unknown[]; installs: unknown[] };

beforeEach(async () => {
  env = await testEnv();
  fetches = [];
  github = { status: 200, releases: [] };
  store = { token: 200, report: 200, acquisitions: [], installs: [] };
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    fetches.push({ url, method: init?.method ?? "GET", body: String(init?.body ?? "") });
    if (url.startsWith("https://api.github.com/repos/urbudget/urbudget/releases"))
      return github.status === 200 ? Response.json(github.releases) : new Response("{}", { status: github.status });
    if (url.startsWith("https://login.microsoftonline.com/"))
      return store.token === 200 ? Response.json({ access_token: "entra-token" }) : Response.json({ error: "invalid_client" }, { status: store.token });
    if (url.startsWith("https://manage.devcenter.microsoft.com/v1.0/my/analytics/")) {
      if (store.report !== 200) return new Response("{}", { status: store.report });
      const report = new URL(url).pathname.split("/").pop();
      return Response.json({ Value: report === "appacquisitions" ? store.acquisitions : store.installs, TotalCount: 1 });
    }
    return new Response("unexpected", { status: 500 });
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const call = (path: string, init: RequestInit = {}) => app.request(path, init, env);
const read = async <T>(response: Response | Promise<Response>) => (await (await response).json()) as T;
const today = () => new Date().toISOString().slice(0, 10);
const daysAgo = (n: number) => new Date(Date.now() - n * 86_400_000).toISOString().slice(0, 10);
const weeksAfter = (monday: string, n: number) => new Date(Date.parse(`${monday}T00:00:00Z`) + n * 7 * 86_400_000).toISOString().slice(0, 10);
const thisMonday = () => mondayOf(today());

const APP = { "Content-Type": "application/json", "User-Agent": "UrBudget/1.0.9", "CF-Connecting-IP": "203.0.113.7" };
const report = (body: Record<string, unknown>, headers: Record<string, string> = APP) =>
  call("/api/v1/usage", { method: "POST", headers, body: JSON.stringify(body) });
const copy = { platform: "windows", channel: "direct", version: "1.0.9" };

interface Usage {
  since: string | null;
  daily: { direct: number[]; store: number[] };
  opens: { direct: number[]; store: number[] };
  weeks: { start: string; active: number; direct: number; store: number; windows: number; macos: number; opens: number }[];
  months: { month: string; active: number }[];
  cohorts: { week: string; opens: number; back: number[] }[];
  versions: { version: string; platform: string; channel: string; thisWeek: number; lastWeek: number }[];
}

describe("copies in use", () => {
  it("counts what's new in each report from the app, and nothing about which copy", async () => {
    // A copy opened for the first time, and one from the Store that was in use before counting began.
    const first = await report({ ...copy, first: true, day: true, week: true, month: true, cohort: thisMonday() });
    expect(first.status).toBe(204);
    await report({ ...copy, channel: "store", day: true, week: true, month: false, cohort: "" });
    // Nothing new: nothing counted.
    expect((await report({ ...copy, day: false, week: false, month: false })).status).toBe(204);

    const u = await read<Usage>(call("/api/v1/admin/usage"));
    expect(u.since).toBe(today());
    expect([u.daily.direct.at(-1), u.daily.store.at(-1)]).toEqual([1, 1]);
    expect([u.opens.direct.at(-1), u.opens.store.at(-1)]).toEqual([1, 0]);
    expect(u.weeks[0]).toEqual({ start: thisMonday(), active: 2, direct: 1, store: 1, windows: 2, macos: 0, opens: 1 });
    expect(u.months).toEqual([{ month: today().slice(0, 7), active: 1 }]);
    expect(u.cohorts).toEqual([{ week: thisMonday(), opens: 1, back: [] }]);
    expect(u.versions).toEqual(
      expect.arrayContaining([
        { version: "1.0.9", platform: "windows", channel: "direct", thisWeek: 1, lastWeek: 0 },
        { version: "1.0.9", platform: "windows", channel: "store", thisWeek: 1, lastWeek: 0 },
      ]),
    );

    // Only totals: the address isn't kept, not even in the rate limit's key.
    const rows = env.DB.sqlite.prepare("SELECT * FROM app_usage").all();
    expect(Object.keys(rows[0]).sort()).toEqual(["channel", "cohort", "count", "day", "kind", "platform", "version"]);
    expect(JSON.stringify(env.DB.sqlite.prepare("SELECT * FROM rate_limits").all())).not.toContain("203.0.113.7");

    const overview = await read<{ usage: { opens30: number; thisWeek: number }; funnel: { opens: number } }>(call("/api/v1/admin/overview"));
    expect(overview.usage).toMatchObject({ opens30: 1, thisWeek: 2 });
    expect(overview.funnel.opens).toBe(1);
  });

  it("takes reports only from the app, and only ones that make sense", async () => {
    // Not the app: a browser, or no user agent. Quietly not counted.
    expect((await report({ ...copy, day: true }, { ...APP, "User-Agent": "Mozilla/5.0" })).status).toBe(204);
    expect((await report({ ...copy, day: true }, { "Content-Type": "application/json" })).status).toBe(204);
    for (const bad of [
      { ...copy, platform: "amiga", day: true },
      { ...copy, channel: "sideways", day: true },
      { ...copy, version: "latest", day: true },
      { ...copy, day: "yes" },
      [copy],
    ])
      expect((await report(bad as Record<string, unknown>)).status, JSON.stringify(bad)).toBe(400);
    expect(env.DB.sqlite.prepare("SELECT COUNT(*) AS n FROM app_usage").get()).toEqual({ n: 0 });

    // The week a copy was first opened has to be a Monday that has come.
    await report({ ...copy, week: true, cohort: weeksAfter(thisMonday(), 1) });
    const lastWednesday = new Date(Date.parse(`${thisMonday()}T00:00:00Z`) - 5 * 86_400_000).toISOString().slice(0, 10);
    await report({ ...copy, week: true, cohort: lastWednesday });
    expect(env.DB.sqlite.prepare("SELECT cohort FROM app_usage").all()).toEqual([{ cohort: "" }]);
  });

  it("limits how many reports one address sends a day", async () => {
    for (let i = 0; i < 20; i++) expect((await report({ ...copy, day: true })).status).toBe(204);
    expect((await report({ ...copy, day: true })).status).toBe(429);
    // Another address is counted.
    expect((await report({ ...copy, day: true }, { ...APP, "CF-Connecting-IP": "198.51.100.2" })).status).toBe(204);
    expect(env.DB.sqlite.prepare("SELECT count FROM app_usage").get()).toEqual({ count: 21 });
  });

  it("follows each week's new copies through the weeks after", async () => {
    const start = weeksAfter(thisMonday(), -3);
    const add = env.DB.sqlite.prepare("INSERT INTO app_usage (day, platform, channel, version, kind, cohort, count) VALUES (?, 'macos', 'direct', '1.0.9', ?, ?, ?)");
    add.run(start, "install", "", 4);
    add.run(start, "week", start, 4);
    add.run(weeksAfter(start, 1), "week", start, 3);
    add.run(weeksAfter(start, 2), "week", start, 1);
    add.run(weeksAfter(start, 3), "week", start, 2);
    // A copy from before counting began is used, but belongs to no week's new copies.
    add.run(weeksAfter(start, 3), "week", "", 5);

    const u = await read<Usage>(call("/api/v1/admin/usage"));
    expect(u.cohorts).toEqual([{ week: start, opens: 4, back: [3, 1, 2] }]);
    expect(u.weeks[0]).toMatchObject({ start: thisMonday(), active: 7, macos: 7 });
    expect(u.weeks[3]).toMatchObject({ start, active: 4, opens: 4 });
  });
});

describe("downloads from every channel", () => {
  it("knows an installer on GitHub by its name", () => {
    expect(installerPlatform("UrBudget_1.0.8_x64-setup.exe")).toBe("windows");
    expect(installerPlatform("UrBudget_1.0.8_x64_en-US.msi")).toBe("windows");
    expect(installerPlatform("UrBudget_1.0.8_universal.dmg")).toBe("macos");
    for (const other of ["UrBudget_1.0.8_x64-setup.exe.sig", "UrBudget_1.0.8_x64.msix", "UrBudget_universal.app.tar.gz", "latest.json"])
      expect(installerPlatform(other)).toBeNull();
  });

  it("notes GitHub's counts every hour, and says when GitHub didn't answer", async () => {
    env.GITHUB_REPO = "urbudget/urbudget";
    github.releases = [
      {
        tag_name: "desktop-v1.0.8",
        name: null,
        draft: false,
        prerelease: false,
        published_at: "2026-10-21T22:25:55Z",
        assets: [
          { name: "UrBudget_1.0.8_x64-setup.exe", size: 5, download_count: 7 },
          { name: "UrBudget_1.0.8_universal.dmg", size: 9, download_count: 2 },
        ],
      },
      { tag_name: "desktop-v1.0.9", name: null, draft: true, prerelease: false, published_at: null, assets: [{ name: "x.exe", size: 1, download_count: 0 }] },
    ];
    await worker.scheduled({} as ScheduledController, env);
    github.releases[0] = { ...(github.releases[0] as object), assets: [{ name: "UrBudget_1.0.8_x64-setup.exe", size: 5, download_count: 9 }] };
    await worker.scheduled({} as ScheduledController, env);
    // One row per file and day, with the day's latest count; drafts aren't downloads.
    expect(env.DB.sqlite.prepare("SELECT tag, asset, count FROM github_downloads ORDER BY asset").all()).toEqual([
      { tag: "desktop-v1.0.8", asset: "UrBudget_1.0.8_universal.dmg", count: 2 },
      { tag: "desktop-v1.0.8", asset: "UrBudget_1.0.8_x64-setup.exe", count: 9 },
    ]);

    github.status = 403;
    await worker.scheduled({} as ScheduledController, env);
    const d = await read<{ github: { since: string; fetched: { okAt: string; error: string } } }>(call("/api/v1/admin/downloads"));
    expect(d.github.since).toBe(today());
    expect(d.github.fetched.okAt).toBeTruthy();
    expect(d.github.fetched.error).toMatch(/403.*GITHUB_TOKEN/);
  });

  it("keeps raw GitHub activity separate from user totals and leaves pre-snapshot downloads undated", async () => {
    const snapshot = env.DB.sqlite.prepare("INSERT INTO github_downloads (day, tag, asset, count) VALUES (?, ?, ?, ?)");
    // The first snapshot: downloads from before it have no day.
    snapshot.run(daysAgo(2), "desktop-v1.0.8", "UrBudget_1.0.8_x64-setup.exe", 10);
    snapshot.run(daysAgo(2), "desktop-v1.0.8", "UrBudget_1.0.8_x64.msix", 2);
    snapshot.run(daysAgo(1), "desktop-v1.0.8", "UrBudget_1.0.8_x64-setup.exe", 14);
    snapshot.run(daysAgo(1), "desktop-v1.0.8", "UrBudget_1.0.8_x64.msix", 5);
    // A release published since: keep the full raw count, including release/QA downloads.
    snapshot.run(daysAgo(1), "desktop-v1.0.9", "UrBudget_1.0.9_universal.dmg", 3);
    snapshot.run(daysAgo(1), "desktop-v1.0.9", "UrBudget_1.0.9_universal.dmg.sig", 6);
    snapshot.run(today(), "desktop-v1.0.8", "UrBudget_1.0.8_x64-setup.exe", 15);
    snapshot.run(today(), "desktop-v1.0.9", "UrBudget_1.0.9_universal.dmg", 3);

    const d = await read<{ github: { windows: number[]; macos: number[]; since: string }; total: number[] }>(call("/api/v1/admin/downloads"));
    expect(d.github.windows.slice(-3)).toEqual([0, 4, 1]);
    expect(d.github.macos.slice(-3)).toEqual([0, 3, 0]);
    expect(d.github.since).toBe(daysAgo(2));
    expect(d.total.slice(-3)).toEqual([0, 0, 0]);
    const o = await read<{ downloads: { last30: number; github: number; windows: number; macos: number } }>(call("/api/v1/admin/overview"));
    expect(o.downloads).toMatchObject({ last30: 0, github: 8, windows: 0, macos: 0 });
  });

  it.each([false, true])("excludes release/QA file activity from every user aggregate (Partner Center: %s)", async (partnerCenter) => {
    const snapshot = env.DB.sqlite.prepare("INSERT INTO github_downloads (day, tag, asset, count) VALUES (?, ?, ?, ?)");
    snapshot.run(daysAgo(61), "old", "old.exe", 10);
    snapshot.run(daysAgo(35), "old", "old.exe", 60);
    // Repeated CI and QA downloads of a new release must never become user activity.
    snapshot.run(daysAgo(1), "new", "new.exe", 100);
    snapshot.run(today(), "new", "new.exe", 200);
    const click = env.DB.sqlite.prepare("INSERT INTO download_counts (day, platform, version, source, kind, count) VALUES (?, ?, ?, '', ?, ?)");
    click.run(today(), "windows", "1.0.11", "person", 2);
    click.run(today(), "macos", "1.0.11", "person", 3);
    click.run(today(), "windows", "Microsoft Store", "person", 4);
    click.run(today(), "windows", "1.0.11", "bot", 91);
    click.run(today(), "windows", "1.0.11", "team", 37);
    click.run(daysAgo(35), "windows", "1.0.10", "person", 2);
    if (partnerCenter) {
      Object.assign(env, { MSSTORE_TENANT_ID: "tenant", MSSTORE_CLIENT_ID: "client", MSSTORE_CLIENT_SECRET: "secret" });
      env.DB.sqlite.prepare("INSERT INTO channel_fetches (source, tried_at, ok_at) VALUES ('store', ?, ?)").run(today(), today());
      env.DB.sqlite.prepare("INSERT INTO store_counts (day, kind, count) VALUES (?, 'acquisitions', 6)").run(today());
    }
    const storeCount = partnerCenter ? 6 : 4;
    const userTotal = 5 + storeCount;
    const d = await read<{
      total: number[];
      github: { windows: number[]; includesAutomation: boolean; excludedFromUserTotals: boolean };
      others: { bots: { last30: number }; team: { last30: number } };
    }>(call("/api/v1/admin/downloads"));
    expect(d.total.at(-1)).toBe(userTotal);
    expect(d.total.reduce((a, b) => a + b, 0)).toBe(userTotal + 2);
    expect(d.github.windows.reduce((a, b) => a + b, 0)).toBe(250);
    expect(d.github).toMatchObject({ includesAutomation: true, excludedFromUserTotals: true });
    expect(d.others).toMatchObject({ bots: { last30: 91 }, team: { last30: 37 } });
    const o = await read<{
      downloads: { last30: number; github: number; windows: number; macos: number };
      previous30: { downloads: number };
      funnel: { downloads: number };
      daily: { downloads: number[] };
      weeks: { downloads: number }[];
    }>(call("/api/v1/admin/overview"));
    expect(o.downloads).toMatchObject({ last30: userTotal, github: 200, windows: 2 + storeCount, macos: 3 });
    expect(o.previous30.downloads).toBe(2);
    expect(o.funnel.downloads).toBe(userTotal);
    expect(o.daily.downloads.reduce((a, b) => a + b, 0)).toBe(userTotal + 2);
    expect(o.weeks.reduce((sum, week) => sum + week.downloads, 0)).toBe(userTotal + 2);
  });

  it("takes the Store's acquisitions and installs from Partner Center once a day, in place of its button's clicks", async () => {
    const browser = { "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/141.0", Referer: "http://localhost/download/" };
    await call("/download/store", { headers: browser });
    Object.assign(env, { MSSTORE_TENANT_ID: "tenant", MSSTORE_CLIENT_ID: "client", MSSTORE_CLIENT_SECRET: "secret" });
    store.acquisitions = [
      { date: daysAgo(3), applicationId: "9PMQMBZJG06R", acquisitionQuantity: 4 },
      { date: `${daysAgo(2)}T00:00:00.0000000Z`, applicationId: "9PMQMBZJG06R", acquisitionQuantity: 6 },
    ];
    store.installs = [{ date: daysAgo(2), applicationId: "9PMQMBZJG06R", successfulInstallCount: 5 }];

    await worker.scheduled({} as ScheduledController, env);
    const token = fetches.find((f) => f.url.includes("login.microsoftonline.com"))!;
    expect(token.url).toBe("https://login.microsoftonline.com/tenant/oauth2/token");
    expect(new URLSearchParams(token.body).get("resource")).toBe("https://manage.devcenter.microsoft.com");
    const asked = new URL(fetches.find((f) => f.url.includes("/appacquisitions"))!.url);
    expect(Object.fromEntries(asked.searchParams)).toMatchObject({ applicationId: "9PMQMBZJG06R", aggregationLevel: "day", groupby: "date" });
    expect(asked.searchParams.get("startDate")).toMatch(/^\d{1,2}\/\d{1,2}\/\d{4}$/);

    const d = await read<{ store: { configured: boolean; partnerCenter: boolean; acquisitions: number[]; installs: number[]; since: string }; total: number[] }>(
      call("/api/v1/admin/downloads"),
    );
    expect(d.store).toMatchObject({ configured: true, partnerCenter: true, since: daysAgo(3) });
    expect(d.store.acquisitions.slice(-4)).toEqual([4, 6, 0, 0]);
    expect(d.store.installs.slice(-4)).toEqual([0, 5, 0, 0]);
    // Partner Center's acquisitions are the Store's downloads now, not the button's click.
    expect(d.total.slice(-4)).toEqual([4, 6, 0, 0]);
    const o = await read<{ downloads: { store: number; storeSource: string } }>(call("/api/v1/admin/overview"));
    expect(o.downloads).toEqual(expect.objectContaining({ store: 10, storeSource: "partner-center" }));

    // Once a day is enough.
    const asks = fetches.length;
    await worker.scheduled({} as ScheduledController, env);
    expect(fetches.length).toBe(asks);
  });

  it("says why Partner Center didn't answer, and keeps counting the Store button until it does", async () => {
    Object.assign(env, { MSSTORE_TENANT_ID: "tenant", MSSTORE_CLIENT_ID: "client", MSSTORE_CLIENT_SECRET: "expired" });
    store.token = 401;
    await worker.scheduled({} as ScheduledController, env);
    let d = await read<{ store: { partnerCenter: boolean; fetched: { okAt: string | null; error: string } } }>(call("/api/v1/admin/downloads"));
    expect(d.store.partnerCenter).toBe(false);
    expect(d.store.fetched).toMatchObject({ okAt: null, error: expect.stringMatching(/MSSTORE_CLIENT_SECRET/) });

    // Not asked again for a few hours.
    const asks = fetches.length;
    await worker.scheduled({} as ScheduledController, env);
    expect(fetches.length).toBe(asks);

    store.token = 200;
    store.report = 403;
    env.DB.sqlite.prepare("UPDATE channel_fetches SET tried_at = ?").run(new Date(Date.now() - 4 * 3_600_000).toISOString());
    await worker.scheduled({} as ScheduledController, env);
    d = await read(call("/api/v1/admin/downloads"));
    expect(d.store.fetched.error).toMatch(/403.*Manager role/);

    const health = await read<{ settings: { name: string; ok: boolean }[] }>(call("/api/v1/admin/health"));
    expect(health.settings).toContainEqual({ name: "MSSTORE_TENANT_ID / MSSTORE_CLIENT_ID / MSSTORE_CLIENT_SECRET", ok: true });
  });
});
