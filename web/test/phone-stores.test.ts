// Google Play's and the App Store's install numbers for the admin page. The report files in
// test/fixtures/stores/ are made up in the shape of the real ones: Play Console's monthly installs
// reports (an overview with a row a day, a country report with a row a day and country), stored
// here as UTF-8 and served as UTF-16LE with a byte-order mark like Play's; and an App Store Summary
// Sales Report (tab-separated), served gzipped like Apple's.
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import worker, { app } from "../src/index";
import { appStoreUnits, playBucket, playInstalls, playMonths, reportText } from "../src/channels";
import { base64url, fromBase64url } from "../src/util";
import { testEnv, type TestEnv } from "./fakes";

const fixture = (name: string) => readFileSync(join(__dirname, "fixtures", "stores", name), "utf8");
const NOVEMBER = "stats/installs/installs_app.urbudget_202611_overview.csv";
const OCTOBER_COUNTRIES = "stats/installs/installs_app.urbudget_202610_country.csv";
const BUCKET = "pubsite_prod_rev_01234567890987654321";
const NOW = "2026-11-22T06:00:00.000Z";

/** Text as Play writes it: UTF-16LE after a byte-order mark. */
function utf16le(text: string, bom = true) {
  const start = bom ? 2 : 0;
  const bytes = new Uint8Array(start + text.length * 2);
  if (bom) bytes.set([0xff, 0xfe]);
  for (let i = 0; i < text.length; i++) {
    bytes[start + 2 * i] = text.charCodeAt(i) & 0xff;
    bytes[start + 2 * i + 1] = text.charCodeAt(i) >> 8;
  }
  return bytes;
}
const gzip = async (text: string) =>
  new Uint8Array(await new Response(new Blob([text]).stream().pipeThrough(new CompressionStream("gzip"))).arrayBuffer());

let env: TestEnv;
let requests: { url: string; headers: Headers; body: string }[];
let google: { token: number; storage: number; files: Map<string, Uint8Array> };
let apple: { status: number; reports: Map<string, Uint8Array> };
let rsaKey: string;
let ecKeys: CryptoKeyPair;
let ecPem: string;
let accounts = 0;

beforeAll(async () => {
  const rsa = await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );
  rsaKey = base64url(await crypto.subtle.exportKey("pkcs8", rsa.privateKey));
  ecKeys = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const der = new Uint8Array(await crypto.subtle.exportKey("pkcs8", ecKeys.privateKey));
  const base64 = btoa(String.fromCharCode(...der));
  ecPem = `-----BEGIN PRIVATE KEY-----\n${base64.match(/.{1,64}/g)!.join("\n")}\n-----END PRIVATE KEY-----\n`;
});

beforeEach(async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(NOW));
  env = await testEnv();
  requests = [];
  google = { token: 200, storage: 200, files: new Map() };
  apple = { status: 200, reports: new Map() };
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.stubGlobal("fetch", async (input: string, init?: RequestInit) => {
    const url = String(input);
    requests.push({ url, headers: new Headers(init?.headers), body: String(init?.body ?? "") });
    if (url === "https://oauth2.googleapis.com/token")
      return google.token === 200 ? Response.json({ access_token: "storage-token", expires_in: 3600 }) : Response.json({ error: "invalid_grant" }, { status: google.token });
    if (url.startsWith("https://storage.googleapis.com/storage/v1/b/")) {
      if (google.storage !== 200) return Response.json({ error: { code: google.storage, message: "Forbidden" } }, { status: google.storage });
      const [, bucket, name] = /^https:\/\/storage\.googleapis\.com\/storage\/v1\/b\/([^/]+)\/o\/([^?/]+)\?alt=media$/.exec(url) ?? [];
      const file = bucket === BUCKET ? google.files.get(decodeURIComponent(name)) : undefined;
      return file ? new Response(file) : Response.json({ error: { code: 404, message: "No such object" } }, { status: 404 });
    }
    if (url.startsWith("https://api.appstoreconnect.apple.com/v1/salesReports?")) {
      if (apple.status !== 200)
        return Response.json(
          { errors: [{ status: String(apple.status), code: "NOT_AUTHORIZED", title: "Authentication credentials are missing or invalid.", detail: "Provide a properly configured and signed bearer token." }] },
          { status: apple.status },
        );
      const report = apple.reports.get(new URL(url).searchParams.get("filter[reportDate]")!);
      return report
        ? new Response(report, { headers: { "Content-Type": "application/a-gzip" } })
        : Response.json({ errors: [{ status: "404", code: "NOT_FOUND", title: "The specified resource does not exist", detail: "There were no sales for the date specified." }] }, { status: 404 });
    }
    return new Response("unexpected", { status: 500 });
  });
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const call = (path: string) => app.request(path, {}, env);
const read = async <T>(response: Response | Promise<Response>) => (await (await response).json()) as T;
const cron = () => worker.scheduled({} as ScheduledController, env);
const total = (values: number[]) => values.reduce((a, b) => a + b, 0);
/** Moves the clock on, and the last attempt back past the three hours a failure waits. */
const later = (hours: number) => {
  vi.setSystemTime(new Date(Date.now() + hours * 3_600_000));
  env.DB.sqlite.prepare("UPDATE channel_fetches SET tried_at = ?").run(new Date(Date.now() - 4 * 3_600_000).toISOString());
};

interface Fetched {
  triedAt: string;
  okAt: string | null;
  error: string;
}
interface Downloads {
  days: string[];
  total: number[];
  store: { installs: number[]; since: string | null };
  android: { configured: boolean; fetched: Fetched | null; userInstalls: number[]; deviceInstalls: number[]; since: string | null };
  ios: { configured: boolean; fetched: Fetched | null; downloads: number[]; redownloads: number[]; updates: number[]; since: string | null };
}
interface Phone {
  configured: boolean;
  fetched: Fetched | null;
  last30: number;
  previous30: number;
}
interface Overview {
  downloads: { last30: number };
  phones: { android: Phone; ios: Phone };
  daily: { days: string[]; downloads: number[]; android: number[]; ios: number[] };
  weeks: { downloads: number; android: number; ios: number }[];
}
const on = (d: { days: string[] }, values: number[], day: string) => values[d.days.indexOf(day)];

function setUpPlay() {
  env.GOOGLE_PLAY_SERVICE_ACCOUNT = JSON.stringify({ client_email: `reports-${++accounts}@urbudget.iam.gserviceaccount.com`, private_key: rsaKey });
  env.PLAY_REPORTS_BUCKET = `gs://${BUCKET}/`;
}
function setUpAppStore() {
  Object.assign(env, { ASC_ISSUER_ID: "57246542-96fe-1a63-e053-0824d011072a", ASC_KEY_ID: "2X9R4HXF34", ASC_PRIVATE_KEY: ecPem, ASC_VENDOR_NUMBER: "85000000" });
}

describe("reading the stores' report files", () => {
  it("decodes Play's UTF-16 and plain UTF-8", () => {
    const text = "Date,Package Name\n2026-11-01,app.urbudget\n";
    expect(reportText(utf16le(text))).toBe(text);
    expect(reportText(utf16le(text, false))).toBe(text);
    const bigEndian = utf16le(text).map((_, i, bytes) => bytes[i ^ 1]);
    expect(reportText(bigEndian)).toBe(text);
    expect(reportText(new TextEncoder().encode(`﻿${text}`))).toBe(text);
  });

  it("knows the bucket from Play Console's Cloud Storage URI", () => {
    expect(playBucket(`gs://${BUCKET}/`)).toBe(BUCKET);
    expect(playBucket(` gs://${BUCKET}/stats/installs/ `)).toBe(BUCKET);
    expect(playBucket(BUCKET)).toBe(BUCKET);
    expect(playBucket("gs://Not A Bucket/")).toBe("");
    expect(playBucket(undefined)).toBe("");
    expect(playMonths(new Date("2026-01-05T00:00:00Z"))).toEqual(["202512", "202601"]);
  });

  it("adds up Play's installs per day, from the overview or a row per country", () => {
    const overview = playInstalls(fixture("installs_app.urbudget_202611_overview.csv"));
    expect(overview.users.size).toBe(17);
    expect([overview.users.get("2026-11-14"), overview.users.get("2026-11-15"), overview.users.get("2026-11-17")]).toEqual([3, 1, 2]);
    expect(overview.devices.get("2026-11-14")).toBe(4);
    const countries = playInstalls(fixture("installs_app.urbudget_202610_country.csv"));
    expect(Object.fromEntries(countries.users)).toEqual({ "2026-10-30": 3, "2026-10-31": 2 });
    expect(Object.fromEntries(countries.devices)).toEqual({ "2026-10-30": 4, "2026-10-31": 2 });

    // Quoted fields, another app's rows and anything that isn't a day are left out.
    const quoted = playInstalls('Date,Package Name,Daily Device Installs,Daily User Installs\n2026-11-01,"app.urbudget",2,1\n2026-11-01,app.other,5,5\n"Total, all days",,7,6\n');
    expect(Object.fromEntries(quoted.users)).toEqual({ "2026-11-01": 1 });
    expect(() => playInstalls("Date,Package Name,Installs\n2026-11-01,app.urbudget,3\n")).toThrow(/no "Daily User Installs" column/);
    expect(playInstalls("").users.size).toBe(0);
  });

  it("counts the App Store's units of the app by what they are", () => {
    // First-time 1 and 1F; re-downloads 3 and 3F; updates 7 and 7T. The subscription and another
    // app have their own Apple Identifier. A quote in the title doesn't upset the columns.
    expect(appStoreUnits(fixture("sales-summary-2026-11-20.tsv"))).toEqual({ downloads: 5, redownloads: 2, updates: 5 });
    expect(() => appStoreUnits("Provider\tSKU\tUnits\nAPPLE\tapp.urbudget\t3\n")).toThrow(/no "Apple Identifier" column/);
  });
});

describe("phone installs on the admin page", () => {
  it("shows Android and iOS as not set up until their settings exist", async () => {
    await cron();
    expect(requests.filter((r) => /storage\.googleapis|appstoreconnect/.test(r.url))).toEqual([]);
    const o = await read<Overview>(call("/api/v1/admin/overview"));
    expect(o.phones).toEqual({
      android: { configured: false, fetched: null, last30: 0, previous30: 0 },
      ios: { configured: false, fetched: null, last30: 0, previous30: 0 },
    });
    const d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.android).toMatchObject({ configured: false, fetched: null, since: null });
    expect(d.ios).toMatchObject({ configured: false, fetched: null, since: null });
    const health = await read<{ settings: { name: string; ok: boolean }[] }>(call("/api/v1/admin/health"));
    expect(health.settings).toContainEqual({ name: "PLAY_REPORTS_BUCKET", ok: false });
    expect(health.settings).toContainEqual({ name: "ASC_ISSUER_ID / ASC_KEY_ID / ASC_PRIVATE_KEY / ASC_VENDOR_NUMBER", ok: false });

    // The service account alone, for purchases, isn't enough.
    env.GOOGLE_PLAY_SERVICE_ACCOUNT = JSON.stringify({ client_email: "billing@urbudget.iam.gserviceaccount.com", private_key: rsaKey });
    await cron();
    expect(requests.filter((r) => r.url.includes("storage.googleapis"))).toEqual([]);
  });

  it("reads Google Play's installs reports once a day, apart from the desktop numbers", async () => {
    setUpPlay();
    google.files.set(NOVEMBER, utf16le(fixture("installs_app.urbudget_202611_overview.csv")));
    // October has no overview here, so its country report stands in.
    google.files.set(OCTOBER_COUNTRIES, utf16le(fixture("installs_app.urbudget_202610_country.csv")));
    // A Microsoft Store row from before migration 0015's store column: its kind is "installs" too.
    env.DB.sqlite.prepare("INSERT INTO store_counts (day, kind, count) VALUES ('2026-11-15', 'installs', 7)").run();

    await cron();
    const token = requests.find((r) => r.url === "https://oauth2.googleapis.com/token")!;
    const assertion = new URLSearchParams(token.body).get("assertion")!;
    expect(JSON.parse(new TextDecoder().decode(fromBase64url(assertion.split(".")[1])))).toMatchObject({
      scope: "https://www.googleapis.com/auth/devstorage.read_only",
      aud: "https://oauth2.googleapis.com/token",
    });
    const files = requests.filter((r) => r.url.startsWith("https://storage.googleapis.com/"));
    expect(files.map((r) => decodeURIComponent(new URL(r.url).pathname.split("/o/")[1]))).toEqual([
      "stats/installs/installs_app.urbudget_202610_overview.csv",
      OCTOBER_COUNTRIES,
      NOVEMBER,
    ]);
    expect(files.every((r) => r.headers.get("Authorization") === "Bearer storage-token")).toBe(true);

    let d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.android).toMatchObject({ configured: true, since: "2026-10-30", fetched: { okAt: NOW, error: "" } });
    expect(total(d.android.userInstalls)).toBe(11);
    expect(total(d.android.deviceInstalls)).toBe(13);
    expect(["2026-10-30", "2026-10-31", "2026-11-14", "2026-11-17"].map((day) => on(d, d.android.userInstalls, day))).toEqual([3, 2, 3, 2]);
    // The Microsoft Store's numbers are its own, and the desktop total has no phones in it.
    expect(total(d.store.installs)).toBe(7);
    expect(d.store.since).toBe("2026-11-15");
    expect(total(d.total)).toBe(0);

    const o = await read<Overview>(call("/api/v1/admin/overview"));
    expect(o.phones.android).toMatchObject({ configured: true, last30: 11, previous30: 0 });
    expect(o.phones.ios.configured).toBe(false);
    expect(o.downloads.last30).toBe(0);
    expect(total(o.daily.android)).toBe(11);
    expect(total(o.daily.downloads)).toBe(0);
    expect(o.weeks.reduce((n, w) => n + w.android, 0)).toBe(11);

    // Once a day is enough.
    const asked = requests.length;
    await cron();
    expect(requests.length).toBe(asked);

    // The next day, a late day has filled in: the new count replaces the old one.
    google.files.set(NOVEMBER, utf16le(fixture("installs_app.urbudget_202611_overview.csv").replace("2026-11-17,app.urbudget,2,0,0,46,2,", "2026-11-17,app.urbudget,6,0,0,49,5,")));
    vi.setSystemTime(new Date("2026-11-23T06:00:00Z"));
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(on(d, d.android.userInstalls, "2026-11-17")).toBe(5);
    expect(total(d.android.userInstalls)).toBe(14);
  });

  it("says why Play's reports couldn't be read", async () => {
    setUpPlay();
    google.storage = 403;
    await cron();
    let d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.android.fetched).toMatchObject({ okAt: null, error: expect.stringMatching(/answered 403.*View app information and download bulk reports/) });

    // Not asked again for a few hours.
    const asked = requests.length;
    await cron();
    expect(requests.length).toBe(asked);

    google.storage = 200;
    later(4);
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.android.fetched!.error).toMatch(/has no installs report for app\.urbudget for 202610 or 202611/);

    env.PLAY_REPORTS_BUCKET = "gs://Not A Bucket/";
    later(4);
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.android.fetched!.error).toMatch(/PLAY_REPORTS_BUCKET isn't a bucket/);

    setUpPlay();
    google.token = 400;
    later(4);
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.android.fetched!.error).toMatch(/GOOGLE_PLAY_SERVICE_ACCOUNT/);

    google.token = 200;
    google.files.set(NOVEMBER, utf16le("Date,Package Name,Installs\n2026-11-01,app.urbudget,3\n"));
    later(4);
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.android.fetched!.error).toMatch(/no "Daily User Installs" column/);
    expect(total(d.android.userInstalls)).toBe(0);

    const health = await read<{ settings: { name: string; ok: boolean }[] }>(call("/api/v1/admin/health"));
    expect(health.settings).toContainEqual({ name: "PLAY_REPORTS_BUCKET", ok: true });
  });

  it("fetches the App Store's daily sales summaries with a signed key, apart from the desktop numbers", async () => {
    setUpAppStore();
    const report = fixture("sales-summary-2026-11-20.tsv");
    apple.reports.set("2026-11-20", await gzip(report));
    // Already unpacked on the way: read as it is.
    apple.reports.set("2026-11-18", new TextEncoder().encode(report.replace(/\t3\t0\t11\/20\/2026/, "\t1\t0\t11/18/2026")));

    await cron();
    const asked = requests.filter((r) => r.url.startsWith("https://api.appstoreconnect.apple.com/"));
    expect(asked.map((r) => new URL(r.url).searchParams.get("filter[reportDate]"))).toEqual([
      "2026-11-15",
      "2026-11-16",
      "2026-11-17",
      "2026-11-18",
      "2026-11-19",
      "2026-11-20",
      "2026-11-21",
    ]);
    expect(Object.fromEntries(new URL(asked[0].url).searchParams)).toMatchObject({
      "filter[frequency]": "DAILY",
      "filter[reportType]": "SALES",
      "filter[reportSubType]": "SUMMARY",
      "filter[vendorNumber]": "85000000",
      "filter[version]": "1_0",
    });

    // An ES256 token signed with the key, as App Store Connect wants it.
    const jwt = asked[0].headers.get("Authorization")!.replace(/^Bearer /, "");
    const [header, payload, signature] = jwt.split(".");
    const decode = (part: string) => JSON.parse(new TextDecoder().decode(fromBase64url(part)));
    expect(decode(header)).toEqual({ alg: "ES256", kid: "2X9R4HXF34", typ: "JWT" });
    const claims = decode(payload);
    expect(claims).toMatchObject({ iss: "57246542-96fe-1a63-e053-0824d011072a", aud: "appstoreconnect-v1", iat: Date.parse(NOW) / 1000 });
    expect(claims.exp - claims.iat).toBe(1200);
    const signed = await crypto.subtle.verify({ name: "ECDSA", hash: "SHA-256" }, ecKeys.publicKey, fromBase64url(signature), new TextEncoder().encode(`${header}.${payload}`));
    expect(signed).toBe(true);

    const d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.ios).toMatchObject({ configured: true, since: "2026-11-15", fetched: { okAt: NOW, error: "" } });
    expect(d.ios.downloads.slice(-8)).toEqual([0, 0, 0, 3, 0, 5, 0, 0]);
    expect(d.ios.redownloads.slice(-8)).toEqual([0, 0, 0, 2, 0, 2, 0, 0]);
    expect(d.ios.updates.slice(-8)).toEqual([0, 0, 0, 5, 0, 5, 0, 0]);
    expect(total(d.total)).toBe(0);

    const o = await read<Overview>(call("/api/v1/admin/overview"));
    expect(o.phones.ios).toMatchObject({ configured: true, last30: 8 });
    expect(o.phones.android.configured).toBe(false);
    expect(o.downloads.last30).toBe(0);
    expect(o.weeks.reduce((n, w) => n + w.ios, 0)).toBe(8);

    const before = requests.length;
    await cron();
    expect(requests.length).toBe(before);
  });

  it("says why App Store Connect refused", async () => {
    setUpAppStore();
    apple.status = 401;
    await cron();
    let d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.ios.fetched).toMatchObject({
      okAt: null,
      error: expect.stringMatching(/answered 401.*ASC_ISSUER_ID, ASC_KEY_ID and ASC_PRIVATE_KEY.*Apple says: Provide a properly configured/),
    });

    apple.status = 403;
    later(4);
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.ios.fetched!.error).toMatch(/answered 403.*Sales and Reports role/);

    // A key pasted on one line with \n for its line breaks works; a key that isn't one says so.
    env.ASC_PRIVATE_KEY = ecPem.replace(/\n/g, "\\n");
    apple.status = 200;
    later(4);
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.ios.fetched).toMatchObject({ error: "", okAt: expect.any(String) });

    env.ASC_PRIVATE_KEY = "AuthKey_2X9R4HXF34.p8";
    later(24);
    await cron();
    d = await read<Downloads>(call("/api/v1/admin/downloads"));
    expect(d.ios.fetched!.error).toMatch(/ASC_PRIVATE_KEY isn't an App Store Connect API key/);

    const health = await read<{ settings: { name: string; ok: boolean }[] }>(call("/api/v1/admin/health"));
    expect(health.settings).toContainEqual({ name: "ASC_ISSUER_ID / ASC_KEY_ID / ASC_PRIVATE_KEY / ASC_VENDOR_NUMBER", ok: true });
  });
});

it("keeps the Microsoft Store's numbers when store_counts gains its store column", () => {
  const db = new DatabaseSync(":memory:");
  try {
    const directory = new URL("../migrations/", import.meta.url);
    const migrations = readdirSync(directory).sort();
    for (const name of migrations.filter((name) => name < "0015")) db.exec(readFileSync(new URL(name, directory), "utf8"));
    db.exec(`INSERT INTO store_counts (day, kind, count) VALUES ('2026-11-01', 'acquisitions', 4), ('2026-11-01', 'installs', 3)`);
    db.exec(readFileSync(new URL("0015_phone_store_counts.sql", directory), "utf8"));
    expect(db.prepare("SELECT store, day, kind, count FROM store_counts ORDER BY kind").all()).toEqual([
      { store: "microsoft", day: "2026-11-01", kind: "acquisitions", count: 4 },
      { store: "microsoft", day: "2026-11-01", kind: "installs", count: 3 },
    ]);
    // Another store's row for the same day and kind is a row of its own.
    db.exec(`INSERT INTO store_counts (store, day, kind, count) VALUES ('play', '2026-11-01', 'installs', 9)`);
    expect(db.prepare("SELECT COUNT(*) AS n FROM store_counts").get()).toEqual({ n: 3 });
  } finally {
    db.close();
  }
});
