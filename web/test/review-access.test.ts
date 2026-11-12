import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { app } from "../src/index";
import { sha256 } from "../src/util";
import { testEnv, type TestEnv } from "./fakes";
let env: TestEnv;
const email = "review@example.com";
const code = "only-a-fixture-not-a-live-credential-123456789";
const call = (path: string, body: unknown) => app.request(`/api/v1/auth/${path}`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }, env);
beforeEach(async () => {
  env = await testEnv();
  env.PLAY_REVIEW_EMAIL = email;
  env.PLAY_REVIEW_CODE_HASH = await sha256(`review:${email}:${code}`);
  vi.spyOn(console, "log").mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());
it("allows repeatable access to only the designated app review account without email OTP", async () => {
  expect(await (await call("start", { email })).json()).toEqual({ ok: true, reviewAccess: true });
  for (let i = 0; i < 2; i++) {
    const response = await call("verify", { email, code, client: "desktop" });
    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ user: { email }, token: expect.any(String) });
  }
  expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM login_codes").first("n")).toBe(0);
  expect(await env.DB.prepare("SELECT COUNT(*) AS n FROM subscriptions").first("n")).toBe(0);
});
it("does not make the review code work for another account or the web client", async () => {
  expect((await call("verify", { email: "someone@example.com", code, client: "desktop" })).status).toBe(400);
  expect((await call("verify", { email, code, client: "web" })).status).toBe(400);
});
it("is disabled without a complete explicit configuration and rejects short/wrong codes", async () => {
  expect((await call("verify", { email, code: "123456", client: "desktop" })).status).toBe(400);
  expect((await call("verify", { email, code: `${code}wrong`, client: "desktop" })).status).toBe(400);
  env.PLAY_REVIEW_CODE_HASH = undefined;
  const start = await (await call("start", { email })).json() as { devCode: string; reviewAccess?: boolean };
  expect(start.reviewAccess).toBeUndefined();
  expect(start.devCode).toMatch(/^\d{6}$/);
});
it("rate-limits review credential attempts", async () => {
  for (let i = 0; i < 20; i++) expect((await call("verify", { email, code: "wrong", client: "desktop" })).status).toBe(400);
  expect((await call("verify", { email, code, client: "desktop" })).status).toBe(429);
});
it("lets a second reviewer's account in from REVIEW_ACCOUNTS, each with only its own code", async () => {
  const play = "play-review@example.com";
  const playCode = "only-a-second-fixture-not-a-live-credential-987654321";
  env.REVIEW_ACCOUNTS = JSON.stringify({ [play.toUpperCase()]: await sha256(`review:${play}:${playCode}`) });
  expect(await (await call("start", { email: play })).json()).toEqual({ ok: true, reviewAccess: true });
  expect((await call("verify", { email: play, code: playCode, client: "desktop" })).status).toBe(200);
  expect((await call("verify", { email, code, client: "desktop" })).status).toBe(200);
  expect((await call("verify", { email: play, code, client: "desktop" })).status).toBe(400);
  expect((await call("verify", { email, code: playCode, client: "desktop" })).status).toBe(400);
  expect((await call("verify", { email: play, code: playCode, client: "web" })).status).toBe(400);
});
it("enables no extra review account from a malformed REVIEW_ACCOUNTS, and keeps the first", async () => {
  for (const setting of ["not json", "[]", JSON.stringify({ "play-review@example.com": "not-a-hash" })]) {
    env.REVIEW_ACCOUNTS = setting;
    const start = await (await call("start", { email: "play-review@example.com" })).json() as { reviewAccess?: boolean };
    expect(start.reviewAccess).toBeUndefined();
    expect((await call("verify", { email, code, client: "desktop" })).status).toBe(200);
  }
});
