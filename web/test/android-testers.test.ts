import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { app } from "../src/index";
import { testEnv, type TestEnv } from "./fakes";

let env: TestEnv;
beforeEach(async () => {
  env = await testEnv();
  vi.stubGlobal("fetch", vi.fn(() => { throw new Error("Signing up must not send email or call another service."); }));
});
afterEach(() => vi.unstubAllGlobals());

const signUp = (body: unknown, ip = "192.0.2.1") => app.request("/api/v1/android-testers", {
  method: "POST",
  headers: { "Content-Type": "application/json", "CF-Connecting-IP": ip },
  body: JSON.stringify(body),
}, env);
const testers = () => env.DB.sqlite.prepare("SELECT email FROM android_testers ORDER BY email").all();

it("keeps a lowercased address once, answers the same for a repeat, and sends nothing", async () => {
  const first = await signUp({ email: "  Tester@Gmail.com " });
  expect(first.status).toBe(201);
  expect(await first.json()).toEqual({ ok: true });
  expect((await signUp({ email: "tester@gmail.com" })).status).toBe(201);
  expect(testers()).toEqual([{ email: "tester@gmail.com" }]);
  expect(env.DB.sqlite.prepare("SELECT COUNT(*) AS n FROM users").get()).toEqual({ n: 0 });
  expect(fetch).not.toHaveBeenCalled();
});

it.each([null, [], {}, { email: "" }, { email: "not an address" }, { email: 3 }, { email: `${"a".repeat(250)}@x.co` },
  { email: "tester@gmail.com", name: "Not asked for" }])("refuses anything but one valid address: %j", async (body) => {
  expect((await signUp(body)).status).toBe(400);
  expect(testers()).toEqual([]);
});

it("limits sign-ups from one address without keeping that address", async () => {
  for (let i = 0; i < 5; i++) expect((await signUp({ email: `t${i}@gmail.com` })).status).toBe(201);
  expect((await signUp({ email: "t5@gmail.com" })).status).toBe(429);
  expect((await signUp({ email: "t5@gmail.com" }, "192.0.2.2")).status).toBe(201);
  expect(JSON.stringify(env.DB.sqlite.prepare("SELECT * FROM rate_limits").all())).not.toContain("192.0.2.");
});

it("lists and removes sign-ups only for an admin, without the address in the admin log", async () => {
  await signUp({ email: "tester@gmail.com" });
  const path = `/api/v1/admin/android-testers/${encodeURIComponent("tester@gmail.com")}`;
  const devCodes = env.DEV_LOGIN_CODES;
  env.DEV_LOGIN_CODES = undefined;
  expect((await app.request("/api/v1/admin/android-testers", {}, env)).status).toBe(403);
  expect((await app.request(path, { method: "DELETE" }, env)).status).toBe(403);
  env.DEV_LOGIN_CODES = devCodes;

  const list = await app.request("/api/v1/admin/android-testers", {}, env);
  expect(await list.json()).toEqual({ testers: [{ email: "tester@gmail.com", createdAt: expect.any(String) }] });
  expect((await app.request(path, { method: "DELETE", headers: { Origin: "https://untrusted.example" } }, env)).status).toBe(403);
  expect((await app.request(path, { method: "DELETE", headers: { Origin: "http://localhost" } }, env)).status).toBe(200);
  expect(testers()).toEqual([]);
  expect((await app.request(path, { method: "DELETE", headers: { Origin: "http://localhost" } }, env)).status).toBe(404);
  const log = env.DB.sqlite.prepare("SELECT action, detail FROM admin_log").all();
  expect(log).toEqual([{ action: "delete_android_tester", detail: "" }]);
});
