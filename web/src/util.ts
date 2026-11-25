/** An error the API reports as `{ error, message }` with an HTTP status. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly extra: Record<string, unknown> = {},
  ) {
    super(message);
  }
}

export const now = () => new Date().toISOString();
export const inSeconds = (seconds: number) => new Date(Date.now() + seconds * 1000).toISOString();
export const DAY = 86_400;

/** The Monday (UTC) of the week a YYYY-MM-DD day falls in. */
export function mondayOf(date: string) {
  const d = new Date(`${date}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() - ((d.getUTCDay() + 6) % 7));
  return d.toISOString().slice(0, 10);
}

const encoder = new TextEncoder();
export const utf8 = (s: string) => encoder.encode(s);

export function base64url(bytes: ArrayBuffer | Uint8Array): string {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let binary = "";
  for (const b of view) binary += String.fromCharCode(b);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
export function fromBase64url(s: string): Uint8Array {
  const padded = s.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(s.length / 4) * 4, "=");
  return Uint8Array.from(atob(padded), (c) => c.charCodeAt(0));
}
export const hex = (bytes: ArrayBuffer) =>
  [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");

export const randomBytes = (n: number) => crypto.getRandomValues(new Uint8Array(n));
/** An unguessable secret for a client to hold (a session token, a link). */
export const randomToken = (prefix = "") => prefix + base64url(randomBytes(32));
export const randomId = (prefix: string) => `${prefix}_${base64url(randomBytes(12))}`;

/** A uniformly random integer in [0, max). */
export function randomBelow(max: number): number {
  const limit = Math.floor(0x1_0000_0000 / max) * max;
  const word = new Uint32Array(1);
  do crypto.getRandomValues(word);
  while (word[0] >= limit);
  return word[0] % max;
}

export async function sha256(s: string): Promise<string> {
  return hex(await crypto.subtle.digest("SHA-256", utf8(s)));
}
/** Compares two strings without leaking where they differ. */
export function sameString(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export function normalizeEmail(value: unknown): string {
  const email = typeof value === "string" ? value.trim().toLowerCase() : "";
  if (email.length > 254 || !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email))
    throw new ApiError(400, "invalid_email", "Enter a valid email address.");
  return email;
}

/** Reads a JSON object body, or fails with a 400. */
export async function jsonBody(request: Request): Promise<Record<string, unknown>> {
  try {
    const body = await request.json();
    if (body && typeof body === "object" && !Array.isArray(body)) return body as Record<string, unknown>;
  } catch {
    /* fall through */
  }
  throw new ApiError(400, "invalid_body", "The request body must be a JSON object.");
}

/**
 * Counts an event against a fixed window and fails with 429 past `limit`. Used for sign-in codes
 * and Play purchase checks, where an exact count matters less than a cheap ceiling.
 */
export async function rateLimit(db: D1Database, key: string, limit: number, windowSeconds: number) {
  const windowStart = new Date(Math.floor(Date.now() / (windowSeconds * 1000)) * windowSeconds * 1000).toISOString();
  const row = await db
    .prepare(
      `INSERT INTO rate_limits (key, count, window_start) VALUES (?1, 1, ?2)
       ON CONFLICT(key) DO UPDATE SET
         count = CASE WHEN window_start = ?2 THEN count + 1 ELSE 1 END,
         window_start = ?2
       RETURNING count`,
    )
    .bind(key, windowStart)
    .first<{ count: number }>();
  // Noted once per window, the first time it's crossed.
  if (row?.count === limit + 1)
    await recordProblem(db, "rate_limit", key, `More than ${limit} in ${Math.round(windowSeconds / 60)} minutes.`);
  if ((row?.count ?? 0) > limit)
    throw new ApiError(429, "rate_limited", "Too many attempts. Wait a few minutes and try again.");
}

/** What the admin page counts per day (migrations/0006_events.sql says what each one is). */
export type EventName = "email" | "sign_in" | "checkout" | "sync_upload" | "account_deleted" | "cleanup";

/** Adds one to today's count of an event. Only the count is kept. Never fails the caller. */
export async function countEvent(db: D1Database, name: EventName, detail = "") {
  try {
    await db
      .prepare(
        `INSERT INTO events (day, name, detail, count) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(day, name, detail) DO UPDATE SET count = count + 1`,
      )
      .bind(now().slice(0, 10), name, detail)
      .run();
  } catch (error) {
    console.error("[events]", error);
  }
}

/** Notes something that went wrong for the admin page's Health tab. Never fails the caller. */
export async function recordProblem(db: D1Database, kind: "email" | "webhook" | "rate_limit", subject: string, detail = "") {
  try {
    await db
      .prepare("INSERT INTO problems (at, kind, subject, detail) VALUES (?1, ?2, ?3, ?4)")
      .bind(now(), kind, subject.slice(0, 200), detail.slice(0, 1000))
      .run();
  } catch (error) {
    console.error("[problems]", error);
  }
}
