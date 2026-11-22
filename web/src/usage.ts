// Copies of the app in use, as daily totals (migrations/0011_channels_usage.sql). The app keeps the
// days it was last counted and reports, at most once a day, only what's new: opened for the first
// time ever, today, this week or this month. Nothing identifies the copy, the computer or the
// person, and nothing is kept per report, only the totals.
import { Hono } from "hono";
import type { AppEnv } from "./env";
import { isVersion } from "./releases";
import { ApiError, mondayOf, now, rateLimit, sha256 } from "./util";

/**
 * Where each system's copies come from. The desktop: urbudget.app or GitHub ("direct"), or the
 * Microsoft Store. Android: Google Play, or a preview APK (the debug-signed app.urbudget.nativeapp).
 * iOS: the App Store, or TestFlight.
 */
const CHANNELS: Record<string, string[]> = {
  windows: ["direct", "store"],
  macos: ["direct", "store"],
  linux: ["direct", "store"],
  android: ["play", "preview"],
  ios: ["appstore", "testflight"],
};
const PHONES = new Set(["android", "ios"]);
/** What a report can say is new, and the kind each is counted as. */
const FLAGS = { first: "install", day: "day", week: "week", month: "month" } as const;

export const usage = new Hono<AppEnv>();

usage.post("/", async (c) => {
  // Reports come from the app, which says `UrBudget/<version>`; anything else isn't a copy in use.
  if (!/^UrBudget\//.test(c.req.header("User-Agent") ?? "")) return c.body(null, 204);
  // A report is a few dozen bytes; anything bigger isn't one.
  if (Number(c.req.header("Content-Length") ?? 0) > 1024) return c.body(null, 204);
  let body: Record<string, unknown> = {};
  try {
    body = JSON.parse(await c.req.text());
  } catch {
    // Not JSON: refused below.
  }
  if (!body || typeof body !== "object" || Array.isArray(body)) body = {};
  const { platform, channel, version, cohort } = body;
  if (
    typeof platform !== "string" ||
    !Object.hasOwn(CHANNELS, platform) ||
    typeof channel !== "string" ||
    !CHANNELS[platform].includes(channel) ||
    typeof version !== "string" ||
    !isVersion(version) ||
    Object.keys(FLAGS).some((flag) => body[flag] !== undefined && typeof body[flag] !== "boolean")
  )
    throw new ApiError(400, "invalid_usage", "That isn't a usage report.");
  const kinds = Object.entries(FLAGS).flatMap(([flag, kind]) => (body[flag] === true ? [kind] : []));
  if (!kinds.length) return c.body(null, 204);

  const today = now().slice(0, 10);
  // A household or an office shares an address, but far more than this from one a day isn't copies
  // in use. Phones get their own, larger allowance: a mobile carrier puts many of its customers
  // behind one address. The address is hashed with the day, so the limit's row can't follow anyone.
  const phone = PHONES.has(platform);
  const bucket = await sha256(`usage${phone ? "-phone" : ""}:${today}:${c.req.header("CF-Connecting-IP") ?? "local"}`);
  await rateLimit(c.env.DB, `usage:${bucket}`, phone ? 200 : 20, 86_400);
  // Only a week's report carries the week the copy was first opened: a Monday, and not a future one.
  const week =
    typeof cohort === "string" && /^\d{4}-\d{2}-\d{2}$/.test(cohort) && mondayOf(cohort) === cohort && cohort <= today ? cohort : "";
  await c.env.DB.batch(
    kinds.map((kind) =>
      c.env.DB.prepare(
        `INSERT INTO app_usage (day, platform, channel, version, kind, cohort, count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
         ON CONFLICT(day, platform, channel, version, kind, cohort) DO UPDATE SET count = count + 1`,
      ).bind(today, platform, channel, version, kind, kind === "week" ? week : ""),
    ),
  );
  return c.body(null, 204);
});
