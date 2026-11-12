// Accounts without passwords: a six-digit code sent by email signs a browser in (cookie) or the
// desktop app in (bearer token). Only hashes of codes and session secrets are stored.
import { Hono, type Context, type MiddlewareHandler } from "hono";
import { deleteCookie, getCookie, setCookie } from "hono/cookie";
import type { AppEnv, Env, Session, User } from "./env";
import { sendEmail } from "./email";
import {
  ApiError,
  DAY,
  inSeconds,
  jsonBody,
  normalizeEmail,
  now,
  randomBelow,
  randomId,
  randomToken,
  countEvent,
  rateLimit,
  sameString,
  sha256,
} from "./util";

const COOKIE = "ub_session";
const CODE_SECONDS = 10 * 60;
const CODE_ATTEMPTS = 5;
const LIFETIME = { web: 30 * DAY, desktop: 365 * DAY, link: 5 * 60 };

const codeHash = (email: string, code: string) => sha256(`login:${email}:${code}`);
/**
 * The access-code hash of a store reviewer's account, or null for every other address. The first is
 * `PLAY_REVIEW_EMAIL` with `PLAY_REVIEW_CODE_HASH` (named before there was more than one; it's App
 * Review's); `REVIEW_ACCOUNTS` adds more, as a JSON object of email to hash. A malformed entry or
 * setting enables nothing.
 */
function reviewHash(env: Env, email: string): string | null {
  const hashes = new Map<string, string>();
  const add = (address: unknown, hash: unknown) => {
    if (typeof address === "string" && typeof hash === "string" && /^[a-f0-9]{64}$/.test(hash)) hashes.set(address.trim().toLowerCase(), hash);
  };
  add(env.PLAY_REVIEW_EMAIL, env.PLAY_REVIEW_CODE_HASH);
  try {
    const more: unknown = JSON.parse(env.REVIEW_ACCOUNTS ?? "{}");
    if (more && typeof more === "object" && !Array.isArray(more)) for (const [address, hash] of Object.entries(more)) add(address, hash);
  } catch {
    // Not JSON: no extra review accounts.
  }
  return hashes.get(email) ?? null;
}

/** The account for `email`, created on first sign-in. */
export async function userFor(db: D1Database, email: string): Promise<User> {
  const user = await db
    .prepare(
      `INSERT INTO users (id, email, created_at) VALUES (?1, ?2, ?3)
       ON CONFLICT(email) DO UPDATE SET email = excluded.email
       RETURNING id, email, billing_customer_id, created_at`,
    )
    .bind(randomId("usr"), email, now())
    .first<User>();
  if (!user) throw new ApiError(500, "server_error", "The account couldn't be created.");
  return user;
}

export async function createSession(db: D1Database, userId: string, kind: Session["kind"], deviceName: string | null) {
  const token = randomToken(kind === "desktop" ? "ubd_" : "ubs_");
  await db
    .prepare(
      `INSERT INTO sessions (id, user_id, kind, device_name, created_at, last_seen_at, expires_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)`,
    )
    .bind(await sha256(token), userId, kind, deviceName, now(), inSeconds(LIFETIME[kind]))
    .run();
  return token;
}

function setSessionCookie(c: Context<AppEnv>, token: string) {
  setCookie(c, COOKIE, token, {
    path: "/",
    httpOnly: true,
    secure: true,
    sameSite: "Lax",
    maxAge: LIFETIME.web,
  });
}

async function sessionFor(env: Env, token: string, kinds: Session["kind"][]) {
  const row = await env.DB.prepare(
    `SELECT s.id, s.user_id, s.kind, s.device_name, s.last_seen_at, s.expires_at,
            u.email, u.billing_customer_id, u.created_at
     FROM sessions s JOIN users u ON u.id = s.user_id
     WHERE s.id = ?1`,
  )
    .bind(await sha256(token))
    .first<Session & Omit<User, "id">>();
  if (!row || !kinds.includes(row.kind) || row.expires_at < now()) return null;
  // Sliding expiry, written at most once a day.
  if (Date.parse(row.last_seen_at) < Date.now() - DAY * 1000)
    await env.DB.prepare("UPDATE sessions SET last_seen_at = ?2, expires_at = ?3 WHERE id = ?1")
      .bind(row.id, now(), inSeconds(LIFETIME[row.kind]))
      .run();
  const session: Session = {
    id: row.id,
    user_id: row.user_id,
    kind: row.kind,
    device_name: row.device_name,
    last_seen_at: row.last_seen_at,
    expires_at: row.expires_at,
  };
  const user: User = {
    id: row.user_id,
    email: row.email,
    billing_customer_id: row.billing_customer_id,
    created_at: row.created_at,
  };
  return { session, user };
}

/**
 * The signed-in user, if any: a desktop bearer token or the site's cookie. A cookie request that
 * changes something must come from this site (the browser's Origin header), so other sites can't
 * act with a visitor's cookie.
 */
async function signedIn(c: Context<AppEnv>) {
  const bearer = c.req.header("Authorization")?.match(/^Bearer\s+(\S+)$/i)?.[1];
  const cookie = getCookie(c, COOKIE);
  const found = bearer
    ? await sessionFor(c.env, bearer, ["desktop"])
    : cookie
      ? await sessionFor(c.env, cookie, ["web"])
      : null;
  if (found && !bearer && c.req.method !== "GET" && c.req.method !== "HEAD") {
    const origin = c.req.header("Origin");
    if (!origin || origin !== new URL(c.req.url).origin)
      throw new ApiError(403, "bad_origin", "This request must come from the UrBudget website.");
  }
  return found && { ...found, viaCookie: !bearer };
}

/** Requires a signed-in user (see `signedIn`). */
export const requireUser: MiddlewareHandler<AppEnv> = async (c, next) => {
  const found = await signedIn(c);
  if (!found) throw new ApiError(401, "unauthorized", "Sign in to continue.");
  c.set("user", found.user);
  c.set("session", found.session);
  c.set("viaCookie", found.viaCookie);
  await next();
};

/** A path on this site to go to after signing in; anything else becomes the account page. */
const safeNext = (next: unknown) =>
  typeof next === "string" && /^\/(?![/\\])[\w\-./?=&%#]*$/.test(next) ? next : "/account/";

export const auth = new Hono<AppEnv>();

auth.post("/start", async (c) => {
  const body = await jsonBody(c.req.raw);
  const email = normalizeEmail(body.email);
  await rateLimit(c.env.DB, `code:ip:${c.req.header("CF-Connecting-IP") ?? "local"}`, 20, 3600);
  // Explicitly configured review identity only. Ordinary customers always use email OTP.
  if (reviewHash(c.env, email)) return c.json({ ok: true, reviewAccess: true });
  await rateLimit(c.env.DB, `code:email:${email}`, 5, 3600);
  const code = String(randomBelow(1_000_000)).padStart(6, "0");
  await c.env.DB.prepare(
    `INSERT INTO login_codes (email, code_hash, attempts, expires_at, created_at) VALUES (?1, ?2, 0, ?3, ?4)
     ON CONFLICT(email) DO UPDATE SET code_hash = ?2, attempts = 0, expires_at = ?3, created_at = ?4`,
  )
    .bind(email, await codeHash(email, code), inSeconds(CODE_SECONDS), now())
    .run();
  await sendEmail(c.env, {
    kind: "sign-in code",
    to: email,
    subject: `${code} is your UrBudget sign-in code`,
    text: `Your UrBudget sign-in code is ${code}\n\nIt works for 10 minutes. If you didn't ask for it, you can ignore this email; nobody can sign in without the code.\n\n— UrBudget\n${c.env.APP_URL}`,
    html: `<p>Your UrBudget sign-in code is</p><p style="font-size:28px;font-weight:700;letter-spacing:4px;font-family:ui-monospace,Menlo,Consolas,monospace">${code}</p><p>It works for 10 minutes. If you didn't ask for it, you can ignore this email; nobody can sign in without the code.</p><p style="color:#666">— UrBudget · <a href="${c.env.APP_URL}">${c.env.APP_URL.replace(/^https?:\/\//, "")}</a></p>`,
  });
  return c.json(c.env.DEV_LOGIN_CODES === "1" ? { ok: true, devCode: code } : { ok: true });
});

auth.post("/verify", async (c) => {
  const body = await jsonBody(c.req.raw);
  const email = normalizeEmail(body.email);
  const code = typeof body.code === "string" ? body.code.replace(/\s/g, "") : "";
  const client = body.client === "desktop" ? "desktop" : "web";
  const deviceName = typeof body.deviceName === "string" ? body.deviceName.slice(0, 80) : null;
  const reviewCode = reviewHash(c.env, email);
  if (reviewCode) {
    await rateLimit(c.env.DB, `review:ip:${c.req.header("CF-Connecting-IP") ?? "local"}`, 20, 3600);
    await rateLimit(c.env.DB, `review:email:${email}`, 60, 3600);
    if (client !== "desktop" || !/^[A-Za-z0-9_-]{24,128}$/.test(code) ||
        !sameString(reviewCode, await sha256(`review:${email}:${code}`)))
      throw new ApiError(400, "invalid_code", "The review access code is not correct.");
    const user = await userFor(c.env.DB, email);
    const token = await createSession(c.env.DB, user.id, "desktop", deviceName);
    await countEvent(c.env.DB, "sign_in", "review");
    return c.json({ user: { id: user.id, email: user.email, createdAt: user.created_at }, token });
  }
  await rateLimit(c.env.DB, `verify:ip:${c.req.header("CF-Connecting-IP") ?? "local"}`, 120, 3600);
  // Reserve a guess in the same statement that checks the ceiling. Concurrent
  // requests cannot all validate against an earlier attempts count.
  const pending = await c.env.DB.prepare(
    `UPDATE login_codes SET attempts = attempts + 1
     WHERE email = ?1 AND expires_at >= ?2 AND attempts < ?3
     RETURNING code_hash, attempts, expires_at`,
  )
    .bind(email, now(), CODE_ATTEMPTS)
    .first<{ code_hash: string; attempts: number; expires_at: string }>();
  if (!pending) {
    const current = await c.env.DB.prepare("SELECT expires_at FROM login_codes WHERE email = ?1").bind(email).first<{ expires_at: string }>();
    if (!current) throw new ApiError(400, "invalid_code", "That code isn't right. Check the latest email we sent.");
    if (current.expires_at < now()) throw new ApiError(400, "expired_code", "That code has expired. Ask for a new one.");
    throw new ApiError(400, "too_many_attempts", "Too many wrong codes. Ask for a new one.");
  }
  if (!/^\d{6}$/.test(code) || !sameString(pending.code_hash, await codeHash(email, code))) {
    throw new ApiError(400, "invalid_code", "That code isn't right. Check the latest email we sent.");
  }
  const consumed = await c.env.DB.prepare(
    "DELETE FROM login_codes WHERE email = ?1 AND code_hash = ?2 AND expires_at >= ?3 RETURNING email",
  ).bind(email, pending.code_hash, now()).first();
  if (!consumed) throw new ApiError(400, "invalid_code", "That code was already used or replaced. Ask for a new one.");
  const user = await userFor(c.env.DB, email);
  const token = await createSession(c.env.DB, user.id, client, deviceName);
  await countEvent(c.env.DB, "sign_in", client);
  const shown = { id: user.id, email: user.email, createdAt: user.created_at };
  if (client === "desktop") return c.json({ user: shown, token });
  setSessionCookie(c, token);
  return c.json({ user: shown });
});

auth.post("/logout", async (c) => {
  const bearer = c.req.header("Authorization")?.match(/^Bearer\s+(\S+)$/i)?.[1];
  const token = bearer ?? getCookie(c, COOKIE);
  if (token) await c.env.DB.prepare("DELETE FROM sessions WHERE id = ?1").bind(await sha256(token)).run();
  if (!bearer) deleteCookie(c, COOKIE, { path: "/", secure: true });
  return c.json({ ok: true });
});

/** The desktop app opens the website already signed in (to subscribe or manage billing). */
auth.post("/web-link", requireUser, async (c) => {
  if (c.get("viaCookie")) throw new ApiError(403, "desktop_only", "Only the desktop app can make sign-in links.");
  const body = await jsonBody(c.req.raw);
  const token = await createSession(c.env.DB, c.get("user").id, "link", null);
  const url = new URL("/api/v1/auth/link", c.env.APP_URL);
  url.searchParams.set("token", token);
  url.searchParams.set("next", safeNext(body.next));
  return c.json({ url: url.toString() });
});

auth.get("/link", async (c) => {
  const token = c.req.query("token") ?? "";
  const next = safeNext(c.req.query("next"));
  const found = token ? await sessionFor(c.env, token, ["link"]) : null;
  if (!found) return c.redirect("/account/?link=expired", 302);
  const consumed = await c.env.DB.prepare("DELETE FROM sessions WHERE id = ?1 AND kind = 'link' AND expires_at >= ?2 RETURNING id")
    .bind(found.session.id, now()).first();
  if (!consumed) return c.redirect("/account/?link=expired", 302);
  setSessionCookie(c, await createSession(c.env.DB, found.user.id, "web", null));
  await countEvent(c.env.DB, "sign_in", "link");
  return c.redirect(next, 302);
});
