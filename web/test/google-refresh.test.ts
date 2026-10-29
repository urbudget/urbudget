import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { app } from "../src/index";
import { createSession } from "../src/auth";
import { refreshGooglePurchases, verifyGooglePurchase } from "../src/billing/google";
import { base64url, sha256 } from "../src/util";
import { testEnv, type TestEnv } from "./fakes";

let env: TestEnv;
let privateKey: string;
let authorization: Record<string, string>;
let accountId: string;
beforeAll(async () => {
  const pair = await crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, ["sign", "verify"]);
  privateKey = base64url(await crypto.subtle.exportKey("pkcs8", pair.privateKey));
  accountId = await sha256("urbudget:usr_refresh");
});
beforeEach(async () => {
  env = await testEnv();
  // Different synthetic credentials prevent OAuth cache state leaking between tests.
  env.GOOGLE_PLAY_SERVICE_ACCOUNT = JSON.stringify({ client_email: `test-${crypto.randomUUID()}@billing.iam.gserviceaccount.com`, private_key: privateKey });
  await env.DB.prepare("INSERT INTO users (id,email,created_at) VALUES ('usr_refresh','refresh@example.com','2026-01-01')").run();
  authorization = { Authorization: `Bearer ${await createSession(env.DB, "usr_refresh", "desktop", "Test")}` };
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); env.DB.sqlite.close(); });
async function receipt(token: string, state = "SUBSCRIPTION_STATE_EXPIRED", expiry = "2000-01-01") {
  await env.DB.prepare(`INSERT INTO google_purchases (token_hash,token,user_id,account_id,product_id,kind,state,expires_at,acknowledged,checked_at,created_at)
    VALUES (?1,?2,'usr_refresh',?3,'urbudget_sync','sync',?4,?5,1,'2000-01-01','2000-01-01')`)
    .bind(await sha256(token), token, accountId, state, expiry).run();
}
const me = () => app.request("/api/v1/me", { headers: authorization }, env);
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => { resolve = r; });
  return { promise, resolve };
}
function mockGoogle(read: (token: string, init?: RequestInit) => Promise<Response>) {
  vi.stubGlobal("fetch", async (input: string, init?: RequestInit) => {
    const url = String(input);
    if (url === "https://oauth2.googleapis.com/token") return Response.json({ access_token: "synthetic", expires_in: 3600 });
    if (url.startsWith("https://androidpublisher.googleapis.com/") && url.includes("subscriptionsv2/tokens/"))
      return read(decodeURIComponent(url.split("/tokens/")[1]), init);
    throw new Error("Unexpected external request blocked by test");
  });
}

it("bounds foreground work, prioritizes current access and backs off failures", async () => {
  for (let i = 0; i < 5; i++) await receipt(`expired${i}`);
  await receipt("current", "SUBSCRIPTION_STATE_ACTIVE", "2099-01-01");
  const entered = deferred(), release = deferred();
  const seen: string[] = [];
  mockGoogle(async (token) => {
    seen.push(token);
    if (seen.length === 2) entered.resolve();
    await release.promise;
    return new Response(null, { status: 503 });
  });
  const first = me();
  await entered.promise;
  expect(seen).toHaveLength(2);
  expect(seen).toContain("current");
  release.resolve();
  expect((await first).status).toBe(200);
  const failed = [...seen];
  expect((await me()).status).toBe(200);
  expect(seen).toHaveLength(4);
  expect(seen.slice(2).every((token) => !failed.includes(token))).toBe(true);
  expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM google_purchases WHERE checked_at = '2000-01-01' AND refresh_after IS NOT NULL").first("n")).toBe(4);
});

it("suppresses duplicate provider work across concurrent foreground requests", async () => {
  await receipt("current", "SUBSCRIPTION_STATE_ACTIVE", "2099-01-01");
  const entered = deferred(), release = deferred();
  let calls = 0;
  mockGoogle(async () => { calls++; entered.resolve(); await release.promise; return new Response(null, { status: 503 }); });
  const first = me();
  await entered.promise;
  expect((await me()).status).toBe(200);
  expect(calls).toBe(1);
  release.resolve();
  expect((await first).status).toBe(200);
  expect((await me()).status).toBe(200);
  expect(calls).toBe(1);
});

it("aborts foreground refresh within its shared three-second deadline", async () => {
  await receipt("slow", "SUBSCRIPTION_STATE_ACTIVE", "2099-01-01");
  const entered = deferred();
  mockGoogle(async (_token, init) => new Promise((_resolve, reject) => {
    const abort = () => reject(new Error("synthetic provider timeout"));
    init!.signal!.addEventListener("abort", abort, { once: true });
    if (init!.signal!.aborted) abort();
    entered.resolve();
  }));
  // Capture the production deadline and expire its signal deterministically;
  // this exercises cancellation without a slow wall-clock test.
  const controller = new AbortController();
  const deadlines: number[] = [];
  vi.spyOn(AbortSignal, "timeout").mockImplementation((milliseconds) => { deadlines.push(milliseconds); return controller.signal; });
  const response = me();
  await entered.promise;
  expect(deadlines).toEqual([3000]);
  controller.abort();
  expect((await response).status).toBe(200);
  expect(await env.DB.prepare("SELECT refresh_after FROM google_purchases").first("refresh_after")).not.toBeNull();
});

it("reconciles history with bounded parallelism and defers terminal receipts for a day", async () => {
  for (let i = 0; i < 7; i++) await receipt(`expired${i}`);
  const entered = deferred(), release = deferred();
  let active = 0, maximum = 0, calls = 0;
  mockGoogle(async () => {
    calls++;
    maximum = Math.max(maximum, ++active);
    if (calls === 4) entered.resolve();
    // Hold the first batch until every request reaches the provider, independently
    // of how long its database lease and OAuth work take on this runner.
    await release.promise;
    active--;
    return Response.json({
      externalAccountIdentifiers: { obfuscatedExternalAccountId: accountId },
      subscriptionState: "SUBSCRIPTION_STATE_EXPIRED",
      acknowledgementState: "ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED",
      lineItems: [{ productId: "urbudget_sync", expiryTime: "2000-01-01T00:00:00Z" }],
    });
  });
  const refresh = refreshGooglePurchases(env);
  await entered.promise;
  expect(calls).toBe(4);
  expect(active).toBe(4);
  release.resolve();
  await refresh;
  expect(calls).toBe(7);
  expect(maximum).toBe(4);
  expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM google_purchases WHERE refresh_after > ?1")
    .bind(new Date(Date.now() + 23 * 3600_000).toISOString()).first("n")).toBe(7);
  await refreshGooglePurchases(env);
  expect(calls).toBe(7);
});

it("still denies expired sync while provider refresh fails", async () => {
  await receipt("expired");
  await env.DB.prepare("INSERT INTO subscriptions (id,user_id,customer_id,status,current_period_end,updated_at,event_at) VALUES ('google:expired','usr_refresh','google:usr_refresh','active','2000-01-01','2000-01-01','2000-01-01')").run();
  mockGoogle(async () => new Response(null, { status: 503 }));
  const response = await app.request("/api/v1/sync/budgets", { headers: authorization }, env);
  expect(response.status).toBe(402);
});

it("lets an explicit verified restore clear a historical receipt's backoff", async () => {
  await receipt("restored");
  await env.DB.prepare("UPDATE google_purchases SET refresh_after = '2099-01-01'").run();
  mockGoogle(async () => Response.json({
    externalAccountIdentifiers: { obfuscatedExternalAccountId: accountId },
    subscriptionState: "SUBSCRIPTION_STATE_ACTIVE", acknowledgementState: "ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED",
    lineItems: [{ productId: "urbudget_sync", expiryTime: "2099-01-01T00:00:00Z" }],
  }));
  expect(await verifyGooglePurchase(env, { id: "usr_refresh" }, "restored")).toMatchObject({ active: true });
  expect(await env.DB.prepare("SELECT refresh_after FROM google_purchases").first("refresh_after")).toBeNull();
});


it("refreshes renewed Play entitlement before sync v2 bootstrap gates uploads", async () => {
  await receipt("v2-renewed", "SUBSCRIPTION_STATE_ACTIVE", "2000-01-01");
  mockGoogle(async () => Response.json({
    externalAccountIdentifiers: { obfuscatedExternalAccountId: accountId },
    subscriptionState: "SUBSCRIPTION_STATE_ACTIVE", acknowledgementState: "ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED",
    lineItems: [{ productId: "urbudget_sync", expiryTime: "2099-01-01T00:00:00Z" }],
  }));
  const response = await app.request("/api/v2/sync/bootstrap", { headers: authorization }, env);
  expect(response.status).toBe(200);
  expect(await response.json()).toMatchObject({ entitlement: { active: true } });
  expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM subscriptions WHERE user_id='usr_refresh' AND status='active'").first("n")).toBe(1);
});

it("keeps sync v2 bootstrap available when Play renewal verification fails", async () => {
  await receipt("v2-unavailable");
  let calls = 0;
  mockGoogle(async () => { calls++; return new Response(null, { status: 503 }); });
  const response = await app.request("/api/v2/sync/bootstrap", { headers: authorization }, env);
  expect(response.status).toBe(200);
  expect(await response.json()).toMatchObject({ entitlement: { active: false } });
  expect(calls).toBe(1);
});


it("refreshes a Play renewal between prepare and commit, but replays receipts before provider work", async () => {
  env.SYNC_V2_ENABLED="1";
  await env.DB.prepare("INSERT INTO subscriptions(id,user_id,customer_id,status,updated_at,event_at) VALUES ('comp:prepare','usr_refresh','synthetic','active','2026-01-01','2026-01-01')").run();
  const installationId="renewal_device", account="usr_refresh", root="/api/v2/sync";
  const request=async(path:string,body:unknown)=>app.request(root+path,{method:"POST",headers:{...authorization,"Content-Type":"application/json","X-Sync-Installation":installationId},body:JSON.stringify(body)},env);
  expect((await request("/installations",{installationId})).status).toBe(200);
  const setup={accountId:account,installationId,operationId:"setup",kind:"keySetup",keyGeneration:null,catalogEpoch:null,expectedProtocol:1,key:{salt:"abcdefghijklmnop",check:"synthetic-check"}};
  const first=await (await request("/mutations/1/prepare",setup)).json();
  expect((await (await request("/mutations/1/commit",{digest:first.digest})).json()).receipt.outcome).toBe("accepted");
  const bootstrap=await (await app.request(root+"/bootstrap",{headers:authorization},env)).json();
  const bytes=new TextEncoder().encode("encrypted snapshot");
  const manifest={accountId:account,installationId,operationId:"create",kind:"create",keyGeneration:bootstrap.keyGeneration,catalogEpoch:bootstrap.catalogEpoch,budgetId:"budget",expectedVersion:"0",newIncarnation:"incarnation",
    payload:{format:"v2",sha256:await sha256("encrypted snapshot"),length:bytes.length,meta:"ZW5jcnlwdGVk",context:{format:"v2",accountId:account,keyGeneration:bootstrap.keyGeneration,budgetId:"budget",incarnation:"incarnation",operationId:"create",parentVersion:"0"}}};
  const prepared=await (await request("/mutations/2/prepare",manifest)).json();expect(prepared.state).toBe("prepared");
  expect((await app.request(root+"/mutations/2/blob",{method:"PUT",headers:{...authorization,"X-Sync-Installation":installationId,"X-Sync-Digest":prepared.digest},body:bytes},env)).status).toBe(200);
  await env.DB.prepare("DELETE FROM subscriptions WHERE id='comp:prepare'").run();
  await receipt("commit-renewal","SUBSCRIPTION_STATE_ACTIVE","2000-01-01");
  let calls=0;
  mockGoogle(async()=>{calls++;return Response.json({externalAccountIdentifiers:{obfuscatedExternalAccountId:accountId},subscriptionState:"SUBSCRIPTION_STATE_ACTIVE",acknowledgementState:"ACKNOWLEDGEMENT_STATE_ACKNOWLEDGED",lineItems:[{productId:"urbudget_sync",expiryTime:"2099-01-01T00:00:00Z"}]});});
  const committed=await (await request("/mutations/2/commit",{digest:prepared.digest})).json();
  expect(committed.receipt.outcome).toBe("accepted");expect(calls).toBe(1);
  await env.DB.prepare("UPDATE google_purchases SET checked_at='2000-01-01'").run();
  expect(await (await request("/mutations/2/commit",{digest:prepared.digest})).json()).toEqual(committed);
  expect(calls).toBe(1);
});
