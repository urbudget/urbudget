import type { Budget } from "./types";
import { t } from "./i18n";

const encoder = new TextEncoder();
const MAX_SUFFIX = 0xffff_ffff;

/**
 * Schedule IDs are limited to 64 UTF-8 bytes, while transaction IDs may have 100. Keep short
 * IDs, and use the same bounded hash as schedules.rs for long or occupied ones. `source` also
 * distinguishes a moved occurrence (`transactionId:ahead`) from its original recurrence.
 */
export async function scheduleId(budget: Budget, transactionId: string, source = transactionId): Promise<string> {
  const schedules = budget.schedules ?? [];
  const matching = schedules.filter((s) => s.transaction.id === transactionId);
  const short = encoder.encode(source).length <= 64;
  if (short && matching.some((s) => s.id === source)) return source;
  const reserved = new Set([...schedules.map((s) => s.id), ...budget.transactions.map((t) => t.id)]);
  // The original transaction and its schedule use separate namespaces. Only that exact
  // source ID may also name an existing ledger entry; generated IDs avoid every occupied ID.
  const shortFree = short && !schedules.some((s) => s.id === source) &&
    (source === transactionId || !reserved.has(source));
  if (shortFree && !matching.some((s) => s.id.startsWith("future:"))) return source;

  const digest = await crypto.subtle.digest("SHA-256", encoder.encode(`urbudget:future-schedule:v1\0${source}`));
  const hash = btoa(String.fromCharCode(...new Uint8Array(digest))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  const base = `future:${hash}`;
  const belongs = (id: string) => {
    if (id === base) return true;
    if (!id.startsWith(`${base}:`)) return false;
    const suffix = id.slice(base.length + 1);
    return /^[1-9]\d{0,9}$/.test(suffix) && Number(suffix) <= MAX_SUFFIX;
  };
  // Reuse the earlier choice before looking for a free ID: a collision that forced :1 may
  // since have been deleted, but a save retry must still update :1 instead of duplicating it.
  const previous = matching.find((s) => belongs(s.id));
  if (previous) return previous.id;
  if (shortFree) return source;
  for (let suffix = 0; suffix <= MAX_SUFFIX; suffix++) {
    const candidate = suffix ? `${base}:${suffix}` : base;
    if (!reserved.has(candidate)) return candidate;
  }
  throw new Error(t("Couldn't assign a unique scheduled transaction ID."));
}
