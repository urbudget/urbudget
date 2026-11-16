import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { signWebhook, verifyWebhook } from "../src/billing/polar";
import { app } from "../src/index";
import { compareVersions } from "../src/releases";
import { utf8 } from "../src/util";
import { testEnv, type TestEnv } from "./fakes";

let env: TestEnv;
let polarCalls: { url: string; method: string; body: Record<string, unknown> | null }[];

beforeEach(async () => {
  env = await testEnv();
  polarCalls = [];
  vi.spyOn(console, "log").mockImplementation(() => {});
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    const method = init?.method ?? "GET";
    polarCalls.push({ url, method, body: init?.body ? JSON.parse(String(init.body)) : null });
    if (url.endsWith("/v1/checkouts/")) return Response.json({ id: "co_1", url: "https://sandbox.polar.sh/checkout/co_1" }, { status: 201 });
    if (url.endsWith("/v1/customer-sessions/"))
      return Response.json({ customer_portal_url: "https://sandbox.polar.sh/portal/session" }, { status: 201 });
    if (/\/v1\/subscriptions\/[\w-]+$/.test(url) && method === "DELETE") return Response.json({ id: "sub" });
    return new Response("unexpected", { status: 500 });
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const call = (path: string, init: RequestInit = {}) => app.request(path, init, env);
const json = (method: string, body: unknown, headers: Record<string, string> = {}): RequestInit => ({
  method,
  headers: { "Content-Type": "application/json", ...headers },
  body: JSON.stringify(body),
});

async function signIn(email: string, client: "web" | "desktop") {
  const start = await call("/api/v1/auth/start", json("POST", { email }));
  expect(start.status).toBe(200);
  const { devCode } = (await start.json()) as { devCode: string };
  const verify = await call("/api/v1/auth/verify", json("POST", { email, code: devCode, client, deviceName: "Test PC" }));
  expect(verify.status).toBe(200);
  const body = (await verify.json()) as { token?: string; user: { id: string } };
  if (client === "desktop") return { userId: body.user.id, headers: { Authorization: `Bearer ${body.token}` } as Record<string, string> };
  const cookie = verify.headers.get("Set-Cookie")!.split(";")[0];
  return { userId: body.user.id, headers: { Cookie: cookie, Origin: "http://localhost" } as Record<string, string> };
}

let deliveries = 0;
async function webhook(type: string, data: Record<string, unknown>, timestamp = new Date().toISOString()) {
  const body = JSON.stringify({ type, timestamp, data });
  const id = `msg_${++deliveries}_${Math.random().toString(36).slice(2)}`;
  const ts = Math.floor(Date.now() / 1000);
  return call("/api/v1/billing/webhook", {
    method: "POST",
    body,
    headers: {
      "webhook-id": id,
      "webhook-timestamp": String(ts),
      "webhook-signature": await signWebhook(env.POLAR_WEBHOOK_SECRET!, id, ts, body),
    },
  });
}

const order = (overrides: Record<string, unknown> = {}) => ({
  id: "ord_01",
  status: "paid",
  billing_reason: "subscription_create",
  customer_id: "cus_ana",
  customer: { email: "ana@example.com", external_id: null },
  product_id: "prod-sync",
  checkout_id: "co_01",
  metadata: { plan: "sync" },
  ...overrides,
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

describe("sign-in", () => {
  it("signs a browser in with an emailed code and keeps the cookie to this site", async () => {
    const web = await signIn("Ana@Example.com ", "web");
    const me = await call("/api/v1/me", { headers: web.headers });
    expect(me.status).toBe(200);
    expect(((await me.json()) as { user: { email: string } }).user.email).toBe("ana@example.com");

    const crossSite = await call("/api/v1/me", { method: "DELETE", headers: { ...web.headers, Origin: "https://evil.example" } });
    expect(crossSite.status).toBe(403);

    await call("/api/v1/auth/logout", { method: "POST", headers: web.headers });
    expect((await call("/api/v1/me", { headers: web.headers })).status).toBe(401);
  });

  it("refuses wrong codes and stops after five tries", async () => {
    await call("/api/v1/auth/start", json("POST", { email: "ana@example.com" }));
    for (let i = 0; i < 5; i++) {
      const wrong = await call("/api/v1/auth/verify", json("POST", { email: "ana@example.com", code: "000000", client: "web" }));
      expect(((await wrong.json()) as { error: string }).error).toBe("invalid_code");
    }
    const locked = await call("/api/v1/auth/verify", json("POST", { email: "ana@example.com", code: "000000", client: "web" }));
    expect(((await locked.json()) as { error: string }).error).toBe("too_many_attempts");
  });

  it("rejects malformed email addresses", async () => {
    const response = await call("/api/v1/auth/start", json("POST", { email: "not-an-email" }));
    expect(response.status).toBe(400);
    expect(await response.json()).toMatchObject({ error: "invalid_email" });
  });

  it("lets the desktop app open the site already signed in, once", async () => {
    const desktop = await signIn("ana@example.com", "desktop");
    const link = await call("/api/v1/auth/web-link", json("POST", { next: "/account/?subscribe=1" }, desktop.headers));
    const { url } = (await link.json()) as { url: string };
    const path = url.replace("http://localhost", "");
    const opened = await call(path);
    expect(opened.status).toBe(302);
    expect(opened.headers.get("Location")).toBe("/account/?subscribe=1");
    const cookie = opened.headers.get("Set-Cookie")!.split(";")[0];
    expect((await call("/api/v1/me", { headers: { Cookie: cookie } })).status).toBe(200);
    expect((await call(path)).headers.get("Location")).toBe("/account/?link=expired");
  });

  it("only follows sign-in links to pages on this site", async () => {
    const desktop = await signIn("ana@example.com", "desktop");
    const link = await call("/api/v1/auth/web-link", json("POST", { next: "//evil.example/" }, desktop.headers));
    const { url } = (await link.json()) as { url: string };
    expect(new URL(url).searchParams.get("next")).toBe("/account/");
  });
});

describe("config", () => {
  it("prices only Sync: the app is free, with no trial", async () => {
    const config = (await (await call("/api/v1/config")).json()) as Record<string, unknown>;
    expect(config).toEqual({
      checkout: { ready: true, environment: "sandbox" },
      prices: { sync: "$3" },
      supportEmail: "support@urbudget.app",
    });
  });
});

describe("checkout", () => {
  it("needs an account, and opens Polar's checkout for Sync tied to it", async () => {
    expect((await call("/api/v1/billing/checkout", json("POST", { plan: "sync" }, { Origin: "http://localhost" }))).status).toBe(401);
    const web = await signIn("ana@example.com", "web");
    const opened = await call("/api/v1/billing/checkout", json("POST", { plan: "sync" }, web.headers));
    expect(await opened.json()).toEqual({ url: "https://sandbox.polar.sh/checkout/co_1" });
    expect(polarCalls[0]).toMatchObject({
      url: "https://sandbox-api.polar.sh/v1/checkouts/",
      body: {
        products: ["prod-sync"],
        customer_email: "ana@example.com",
        external_customer_id: web.userId,
        success_url: "http://localhost/account/?checkout_id={CHECKOUT_ID}",
      },
    });
    // The plan can be left out: Sync is the only one.
    expect((await call("/api/v1/billing/checkout", json("POST", {}, web.headers))).status).toBe(200);
    expect(polarCalls[1].body).toMatchObject({ products: ["prod-sync"] });
  });

  it("sells nothing else: no license plan, license routes or purchase lookups", async () => {
    const web = await signIn("ana@example.com", "web");
    const license = await call("/api/v1/billing/checkout", json("POST", { plan: "license" }, web.headers));
    expect(license.status).toBe(400);
    expect(await license.json()).toMatchObject({ error: "invalid_plan" });
    expect(polarCalls).toHaveLength(0);
    for (const [method, path] of [["POST", "/api/v1/licenses/activate"], ["GET", "/api/v1/licenses/mine"], ["POST", "/api/v1/licenses/status"], ["GET", "/api/v1/checkout/co_01_abcdef"]]) {
      const response = await call(path, method === "POST" ? json("POST", {}) : {});
      expect(response.status, path).toBe(404);
    }
    const me = (await (await call("/api/v1/me", { headers: web.headers })).json()) as Record<string, unknown>;
    expect(me).not.toHaveProperty("licenses");
  });
});

describe("webhooks", () => {
  it("ignores orders, including Sync's own renewals, and other products' subscriptions", async () => {
    expect((await webhook("order.paid", order())).status).toBe(200);
    expect((await webhook("order.paid", order({ billing_reason: "subscription_cycle" }))).status).toBe(200);
    expect((await webhook("order.refunded", order({ status: "refunded" }))).status).toBe(200);
    expect((await webhook("subscription.created", subscription({ product_id: "something-else" }))).status).toBe(200);
    expect(env.DB.sqlite.prepare("SELECT COUNT(*) AS n FROM subscriptions").get()).toEqual({ n: 0 });
    expect(env.DB.sqlite.prepare("SELECT COUNT(*) AS n FROM webhook_events").get()).toEqual({ n: 4 });
  });

  it("ignores webhooks with a bad signature", async () => {
    const body = JSON.stringify({ type: "subscription.created", data: subscription() });
    const response = await call("/api/v1/billing/webhook", {
      method: "POST",
      body,
      headers: { "webhook-id": "msg_x", "webhook-timestamp": String(Math.floor(Date.now() / 1000)), "webhook-signature": "v1,AAAA" },
    });
    expect(response.status).toBe(401);
  });

  it("checks Standard Webhooks signatures with clock tolerance, and Polar's older secrets", async () => {
    const secret = `whsec_${btoa("test-webhook-secret-key")}`;
    const ts = 1_800_000_000;
    const headers = (signature: string) =>
      new Headers({ "webhook-id": "msg_1", "webhook-timestamp": String(ts), "webhook-signature": signature });
    const good = await signWebhook(secret, "msg_1", ts, "{}");
    expect(await verifyWebhook(secret, headers(`v1,old ${good}`), "{}", ts * 1000 + 60_000)).toBe(true);
    expect(await verifyWebhook(secret, headers(good), "{}", ts * 1000 + 600_000)).toBe(false);
    expect(await verifyWebhook(secret, headers(good), "{ }", ts * 1000)).toBe(false);
    // Secrets made before September 2026 sign with the secret's own text as the key.
    const key = await crypto.subtle.importKey("raw", utf8(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    const mac = await crypto.subtle.sign("HMAC", key, utf8(`msg_1.${ts}.{}`));
    const legacy = `v1,${btoa(String.fromCharCode(...new Uint8Array(mac)))}`;
    expect(await verifyWebhook(secret, headers(legacy), "{}", ts * 1000)).toBe(true);
  });
});

describe("sync", () => {
  it("needs a subscription, then stores encrypted budgets with revision checks", async () => {
    const desktop = await signIn("ana@example.com", "desktop");
    const blocked = await call("/api/v1/sync/budgets", { headers: desktop.headers });
    expect(blocked.status).toBe(402);

    await webhook("subscription.created", subscription({ customer: { email: "ana@example.com", external_id: desktop.userId } }));
    expect((await call("/api/v1/sync/budgets", { headers: desktop.headers })).status).toBe(404);

    expect((await call("/api/v1/sync/key", { headers: desktop.headers })).status).toBe(404);
    const key = { salt: "c2FsdHNhbHRzYWx0c2FsdA", check: "encrypted-check" };
    expect((await call("/api/v1/sync/key", json("PUT", key, desktop.headers))).status).toBe(200);
    expect((await call("/api/v1/sync/budgets", { headers: desktop.headers })).status).toBe(200);
    expect((await call("/api/v1/sync/key", json("PUT", { ...key, check: "other" }, desktop.headers))).status).toBe(409);

    const upload = (revision: number, bytes: string) =>
      call("/api/v1/sync/budgets/b123", {
        method: "PUT",
        body: bytes,
        headers: { ...desktop.headers, "If-Match": String(revision), "X-Sync-Meta": "meta", "X-Device": "Laptop" },
      });
    expect(await (await upload(0, "first")).json()).toEqual({ revision: 1 });
    expect(await (await upload(1, "second")).json()).toEqual({ revision: 2 });
    const stale = await upload(1, "stale");
    expect(stale.status).toBe(409);
    expect(await stale.json()).toMatchObject({ error: "conflict", revision: 2 });
    expect(env.SYNC.objects.size).toBe(1);

    const download = await call("/api/v1/sync/budgets/b123", { headers: desktop.headers });
    expect(download.headers.get("X-Revision")).toBe("2");
    expect(await download.text()).toBe("second");
    const list = (await (await call("/api/v1/sync/budgets", { headers: desktop.headers })).json()) as { budgets: unknown[] };
    expect(list.budgets).toEqual([expect.objectContaining({ id: "b123", revision: 2, size: 6, meta: "meta", device: "Laptop" })]);

    // A new passphrase wipes what the old one encrypted.
    await call("/api/v1/sync/key", json("PUT", { ...key, check: "new", reset: true }, desktop.headers));
    expect(env.SYNC.objects.size).toBe(0);

    await webhook("subscription.revoked", subscription({ status: "canceled", ended_at: "2026-11-16T00:00:00Z" }));
    expect((await call("/api/v1/sync/budgets", { headers: desktop.headers })).status).toBe(402);
  });

  it("finds the account by the subscriber's email when the checkout wasn't tied to one", async () => {
    const desktop = await signIn("ana@example.com", "desktop");
    await webhook("subscription.active", subscription());
    await call("/api/v1/sync/key", json("PUT", { salt: "c2FsdHNhbHRzYWx0c2FsdA", check: "encrypted-check" }, desktop.headers));
    expect((await call("/api/v1/sync/budgets", { headers: desktop.headers })).status).toBe(200);
  });

  it("handles a customer that already belongs to another account", async () => {
    const first = await signIn("ana@example.com", "desktop");
    const second = await signIn("ben@example.com", "desktop");
    await webhook("subscription.created", subscription({ customer: { email: null, external_id: first.userId } }));
    const response = await webhook(
      "subscription.created",
      subscription({ id: "sub_02", customer: { email: null, external_id: second.userId } }),
    );
    expect(response.status).toBe(200);
    await call("/api/v1/sync/key", json("PUT", { salt: "c2FsdHNhbHRzYWx0c2FsdA", check: "encrypted-check" }, second.headers));
    expect((await call("/api/v1/sync/budgets", { headers: second.headers })).status).toBe(200);
  });

  it("doesn't let an older webhook undo a newer subscription state", async () => {
    const desktop = await signIn("ana@example.com", "desktop");
    const customer = { email: "ana@example.com", external_id: desktop.userId };
    await webhook("subscription.revoked", subscription({ status: "canceled", customer, modified_at: "2026-10-16T12:00:00Z" }));
    await webhook("subscription.created", subscription({ customer, modified_at: "2026-10-16T11:00:00Z" }));
    expect((await call("/api/v1/sync/budgets", { headers: desktop.headers })).status).toBe(402);
  });

  it("reports the plan on the account, including a scheduled cancellation, and opens Polar's portal", async () => {
    const web = await signIn("ana@example.com", "web");
    const customer = { email: "ana@example.com", external_id: web.userId };
    await webhook("subscription.created", subscription({ customer }));
    let me = (await (await call("/api/v1/me", { headers: web.headers })).json()) as Record<string, unknown>;
    expect(me.subscription).toEqual({ provider: "polar", status: "active", renewsAt: "2026-11-16T00:00:00Z", endsAt: null });
    expect(me.sync).toMatchObject({ active: true });

    await webhook("subscription.canceled", subscription({ customer, cancel_at_period_end: true, ends_at: "2026-11-16T00:00:00Z" }));
    me = (await (await call("/api/v1/me", { headers: web.headers })).json()) as Record<string, unknown>;
    expect(me.subscription).toEqual({ provider: "polar", status: "active", renewsAt: null, endsAt: "2026-11-16T00:00:00Z" });

    const portal = await call("/api/v1/billing/portal", { method: "POST", headers: web.headers });
    expect(await portal.json()).toEqual({ url: "https://sandbox.polar.sh/portal/session" });
    expect(polarCalls.at(-1)).toMatchObject({
      url: "https://sandbox-api.polar.sh/v1/customer-sessions/",
      body: { customer_id: "cus_ana", return_url: "http://localhost/account/" },
    });
  });

  it("deletes an account: cancels billing and removes synced copies", async () => {
    const web = await signIn("ana@example.com", "web");
    const customer = { email: "ana@example.com", external_id: web.userId };
    await webhook("subscription.created", subscription({ customer }));
    const wrong = await call("/api/v1/me", json("DELETE", { confirm: "someone@else.com" }, web.headers));
    expect(wrong.status).toBe(400);
    const deleted = await call("/api/v1/me", json("DELETE", { confirm: "ana@example.com" }, web.headers));
    expect(deleted.status).toBe(200);
    expect(polarCalls.some((c) => c.method === "DELETE" && c.url.endsWith("/v1/subscriptions/sub_01"))).toBe(true);
    expect((await call("/api/v1/me", { headers: web.headers })).status).toBe(401);
    expect(env.DB.sqlite.prepare("SELECT COUNT(*) AS n FROM users").get()).toEqual({ n: 0 });
  });

  it("honors Sync given by UrBudget (a review account's) with nothing to bill, manage or cancel", async () => {
    const web = await signIn("review@example.com", "web");
    const desktop = await signIn("review@example.com", "desktop");
    env.DB.sqlite
      .prepare("INSERT INTO subscriptions (id,user_id,customer_id,status,current_period_end,updated_at,event_at) VALUES (?,?,?,?,?,?,?)")
      .run("comp:review", web.userId, "comp", "active", null, "2026-10-22T00:00:00Z", "2026-10-22T00:00:00Z");

    const me = (await (await call("/api/v1/me", { headers: web.headers })).json()) as Record<string, unknown>;
    expect(me.subscription).toEqual({ provider: "comp", status: "active", renewsAt: null, endsAt: null });
    expect(me.sync).toMatchObject({ active: true });
    // Past the subscription check (402 without one): 404 only because no sync key is set up yet.
    expect((await call("/api/v1/sync/budgets", { headers: desktop.headers })).status).toBe(404);

    const portal = await call("/api/v1/billing/portal", { method: "POST", headers: web.headers });
    expect(portal.status).toBe(404);
    expect(await portal.json()).toMatchObject({ error: "no_customer" });

    const deleted = await call("/api/v1/me", json("DELETE", { confirm: "review@example.com" }, web.headers));
    expect(deleted.status).toBe(200);
    expect(polarCalls).toEqual([]);
    expect(env.DB.sqlite.prepare("SELECT COUNT(*) AS n FROM deleted_billing_customers").get()).toEqual({ n: 0 });
    expect(env.DB.sqlite.prepare("SELECT COUNT(*) AS n FROM users").get()).toEqual({ n: 0 });
  });
});

describe("downloads", () => {
  it("sends each system to its newest installer, or back to the download page", async () => {
    expect((await call("/download/windows")).headers.get("Location")).toBe("/download/?unavailable=windows");
    await env.RELEASES.put(
      "latest.json",
      JSON.stringify({
        version: "1.0.0",
        date: "2026-10-20",
        notes: "First release",
        platforms: { windows: { file: "UrBudget_1.0.0_x64-setup.exe", size: 5, sha256: "abc" } },
      }),
    );
    await env.RELEASES.put("files/UrBudget_1.0.0_x64-setup.exe", "hello");
    expect((await call("/download/windows")).headers.get("Location")).toBe("/releases/UrBudget_1.0.0_x64-setup.exe");
    expect((await call("/download/macos")).headers.get("Location")).toBe("/download/?unavailable=macos");
    const latest = (await (await call("/api/v1/releases/latest")).json()) as { platforms: Record<string, unknown> };
    expect(latest.platforms.windows).toMatchObject({ url: "http://localhost/releases/UrBudget_1.0.0_x64-setup.exe" });
    expect(latest.platforms.macos).toBeNull();
    const file = await call("/releases/UrBudget_1.0.0_x64-setup.exe");
    expect(file.headers.get("Content-Disposition")).toContain("attachment");
    expect(await file.text()).toBe("hello");
  });

  it("offers installed copies a newer signed package, and 204 when they're up to date", async () => {
    const update = (path: string) => call(`/api/v1/releases/update/${path}`);
    expect((await update("windows/x86_64/1.0.0")).status).toBe(204);
    const pkg = { date: "2026-11-02T09:30:00.000Z", notes: "Faster reports", signature: "dW50cnVzdGVk" };
    await env.RELEASES.put(
      "latest.json",
      JSON.stringify({
        version: "1.1.0",
        date: "2026-11-02",
        notes: "Faster reports",
        platforms: { windows: { file: "UrBudget_1.1.0_x64-setup.exe", size: 5, sha256: "abc" } },
        updates: {
          "windows-x86_64": { ...pkg, version: "1.1.0", file: "UrBudget_1.1.0_x64-setup.exe" },
          // Not published again since 1.0.0.
          "darwin-aarch64": { ...pkg, version: "1.0.0", file: "UrBudget_1.0.0_aarch64.app.tar.gz" },
        },
      }),
    );
    await env.RELEASES.put("files/UrBudget_1.1.0_x64-setup.exe", "installer");

    const offered = await update("windows/x86_64/1.0.0");
    expect(offered.status).toBe(200);
    expect(await offered.json()).toEqual({
      version: "1.1.0",
      notes: "Faster reports",
      pub_date: "2026-11-02T09:30:00.000Z",
      url: "http://localhost/releases/UrBudget_1.1.0_x64-setup.exe",
      signature: "dW50cnVzdGVk",
    });
    expect(await (await call("/releases/UrBudget_1.1.0_x64-setup.exe")).text()).toBe("installer");
    expect((await update("windows/x86_64/1.1.0-beta.2")).status).toBe(200);

    for (const current of ["windows/x86_64/1.1.0", "windows/x86_64/1.2.0", "darwin/aarch64/1.0.0", "darwin/x86_64/1.0.0", "linux/x86_64/1.0.0"]) {
      const response = await update(current);
      expect(response.status, current).toBe(204);
      expect(await response.text()).toBe("");
    }
    expect((await update("windows/x86_64/latest")).status).toBe(400);
  });

  it("gives a Linux copy the package of its own kind", async () => {
    const pkg = { version: "1.1.0", date: "2026-11-02T09:30:00.000Z", notes: "", signature: "dW50cnVzdGVk" };
    await env.RELEASES.put(
      "latest.json",
      JSON.stringify({
        version: "1.1.0",
        date: "2026-11-02",
        notes: "",
        platforms: {
          linux: { file: "UrBudget_1.1.0_amd64.AppImage", size: 5, sha256: "abc" },
          "linux-deb": { file: "UrBudget_1.1.0_amd64.deb", size: 5, sha256: "def" },
        },
        updates: {
          "windows-x86_64": { ...pkg, file: "UrBudget_1.1.0_x64-setup.exe" },
          "linux-x86_64-appimage": { ...pkg, file: "UrBudget_1.1.0_amd64.AppImage" },
          "linux-x86_64-deb": { ...pkg, file: "UrBudget_1.1.0_amd64.deb" },
        },
      }),
    );
    expect((await call("/download/linux")).headers.get("Location")).toBe("/releases/UrBudget_1.1.0_amd64.AppImage");
    expect((await call("/download/linux-deb")).headers.get("Location")).toBe("/releases/UrBudget_1.1.0_amd64.deb");
    expect(env.DB.sqlite.prepare("SELECT platform, SUM(count) AS n FROM download_counts GROUP BY platform").all()).toEqual([{ platform: "linux", n: 2 }]);
    const latest = (await (await call("/api/v1/releases/latest")).json()) as { platforms: Record<string, unknown> };
    expect(latest.platforms["linux-deb"]).toMatchObject({ url: "http://localhost/releases/UrBudget_1.1.0_amd64.deb" });

    const offered = async (path: string) => {
      const response = await call(`/api/v1/releases/update/${path}`);
      return response.status === 200 ? ((await response.json()) as { url: string }).url : response.status;
    };
    expect(await offered("linux/x86_64/1.0.0?bundle=appimage")).toBe("http://localhost/releases/UrBudget_1.1.0_amd64.AppImage");
    expect(await offered("linux/x86_64/1.0.0?bundle=deb")).toBe("http://localhost/releases/UrBudget_1.1.0_amd64.deb");
    // Built from source, or a kind of package this site doesn't publish: nothing to update with.
    for (const query of ["", "?bundle=unknown", "?bundle=rpm", "?bundle=deb/.."]) {
      expect(await offered(`linux/x86_64/1.0.0${query}`), query).toBe(204);
    }
    expect(await offered("linux/aarch64/1.0.0?bundle=deb")).toBe(204);
    // A system with one package is offered it whatever kind its copy names.
    expect(await offered("windows/x86_64/1.0.0?bundle=nsis")).toBe("http://localhost/releases/UrBudget_1.1.0_x64-setup.exe");
  });

  it("orders versions the way the updater does", () => {
    const sorted = ["0.9.9", "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "1.0.1", "1.10.0", "2.0.0"];
    for (let i = 1; i < sorted.length; i++) {
      expect(compareVersions(sorted[i - 1], sorted[i]), `${sorted[i - 1]} < ${sorted[i]}`).toBeLessThan(0);
      expect(compareVersions(sorted[i], sorted[i - 1])).toBeGreaterThan(0);
    }
    expect(compareVersions("1.2.3", "v1.2.3+build.5")).toBe(0);
  });

  it("serves the site for everything that isn't the API", async () => {
    expect(await (await call("/download/")).text()).toBe("static");
    const missing = await call("/api/v1/nothing");
    expect(missing.status).toBe(404);
    expect(missing.headers.get("X-Content-Type-Options")).toBe("nosniff");
    expect(missing.headers.get("Cache-Control")).toBe("no-store");
  });
});
