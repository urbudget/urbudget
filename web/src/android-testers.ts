// The home page's Android test sign-up: an address to add to Google Play's closed test by hand.
// No account, no email sent, and the same answer whether or not the address was already there.
import { Hono } from "hono";
import type { AppEnv } from "./env";
import { ApiError, jsonBody, normalizeEmail, now, rateLimit, sha256 } from "./util";

export const androidTesters = new Hono<AppEnv>();

androidTesters.post("/", async (c) => {
  // A daily hash of the address the request came from, so rate-limit rows hold no raw IP.
  const day = now().slice(0, 10);
  const bucket = await sha256(`${day}:${c.req.header("CF-Connecting-IP") ?? "local"}`);
  await rateLimit(c.env.DB, `android-tester:hour:${bucket}`, 5, 3600);
  await rateLimit(c.env.DB, "android-tester:total", 2000, 86_400);
  const body = await jsonBody(c.req.raw);
  if (Object.keys(body).some((key) => key !== "email"))
    throw new ApiError(400, "invalid_body", "Send only an email address.");
  const email = normalizeEmail(body.email);
  await c.env.DB.prepare("INSERT INTO android_testers (email, created_at) VALUES (?1, ?2) ON CONFLICT(email) DO NOTHING")
    .bind(email, now())
    .run();
  return c.json({ ok: true }, 201);
});
