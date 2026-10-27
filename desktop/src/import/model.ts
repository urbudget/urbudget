import type { BankField, BankFixes, BankSession, BankStructure, FixResult, LeftOutReason, Statement, StatementLine } from "../bridge";
import type { BankColumns } from "../types";
import { getLanguage, localDate, msg, t } from "../i18n";

/** What a line left out is, by why (in English: translate it with `t`). */
export const LEFT_OUT: Record<LeftOutReason, string> = {
  pending: msg("Not booked yet"),
  declined: msg("Declined or cancelled"),
  otherAccount: msg("Another account's"),
  otherCurrency: msg("In another currency"),
  ownBalances: msg("Between your own balances"),
  balance: msg("A balance or a total"),
  aboveHeader: msg("Above the header"),
  noDate: msg("No date"),
  noAmount: msg("No amount"),
  zero: msg("Nothing moved"),
  corrected: msg("Corrected by the bank"),
  repeatedHeader: msg("The header row again"),
  inWholeBatch: msg("In a batch imported as one entry"),
  imported: msg("Imported before"),
};
/** Lines left out for these reasons can be included anyway (bank_statement.rs `includable`). */
export const INCLUDABLE: ReadonlySet<LeftOutReason> = new Set([
  "pending",
  "declined",
  "otherCurrency",
  "ownBalances",
  "balance",
  "noDate",
  "noAmount",
]);
/** The fields' names as a statement's own headings have them (matched against them, so never translated). */
export const FIELDS: Record<BankField, string> = { date: "Date", payee: "Payee", memo: "Memo", amount: "Amount" };

/** Why a line is left out, with what the file says of it (the state, the currency, the account). */
export function leftOutText(line: StatementLine) {
  if (line.status.kind !== "leftOut") return "";
  const { reason, detail } = line.status;
  if (reason === "otherAccount" && detail) return t("Account {account}", { account: detail });
  if (reason === "otherCurrency" && detail) return t("In {currency}", { currency: detail });
  return detail && ["pending", "declined", "ownBalances"].includes(reason)
    ? t("{reason} ({detail})", { reason: t(LEFT_OUT[reason]), detail })
    : t(LEFT_OUT[reason]);
}

export type Filter = "all" | "attention" | "leftOut" | "edited";
export const shown = (line: StatementLine, filter: Filter) =>
  filter === "all" ||
  (filter === "attention" && line.status.kind === "attention") ||
  (filter === "leftOut" && line.status.kind === "leftOut") ||
  (filter === "edited" && (!!line.fixed || !!line.included));
export const leftOut = (st: Statement) => Object.values(st.counts.leftOut).reduce((a, n) => a + (n ?? 0), 0);

/** A statement the editor opens on by itself for: lines to fix, none that read, or a question to answer. */
export const needsEditor = (st: Statement) =>
  st.counts.attention > 0 || st.counts.ready === 0 || !!st.dateOrder?.unconfirmed || !!st.signs?.unconfirmed;

/** Changes to a session, made against its current revision. */
export const fixesFor = (
  session: BankSession,
  change: Omit<Partial<BankFixes>, "scope" | "baseRevision" | "baseStructure">,
): BankFixes => ({
  scope: session.scope,
  baseRevision: session.revision,
  baseStructure: session.structure,
  ...change,
});
/**
 * Changes applied to an import as it is when they land. Another change can land while these are
 * being made (the user's while the assistant's apply, or the other way round): they're then made
 * again on the import as that one left it, so neither undoes the other. `now` gives the import's
 * session as it is now; `base` is the session the result is for, to keep it only if it still is.
 */
export async function applyToLatest(
  now: () => BankSession | undefined,
  apply: (session: BankSession) => Promise<{ session: BankSession; results: FixResult[] }>,
): Promise<{ base: BankSession; session: BankSession; results: FixResult[] } | null> {
  let base = latestOf(now());
  for (let tries = 0; base && tries < 3; tries++) {
    const applied = await apply(base);
    const latest = latestOf(now());
    if (latest === base) {
      replaced.set(base, applied.session);
      return { base, ...applied };
    }
    // Another file since: these were for an import that's gone.
    if (!latest || latest.scope.session !== base.scope.session) return null;
    base = latest;
  }
  if (!base) return null;
  throw new Error(t("The import kept changing while this was made. Try it again."));
}
/** The session each change made, by the one it was made on: one that lands before it shows is still the latest. */
const replaced = new WeakMap<BankSession, BankSession>();
const latestOf = (session: BankSession | undefined) => {
  while (session && replaced.has(session)) session = replaced.get(session);
  return session;
};

/** The session read another way. */
export const restructure = (session: BankSession, change: Partial<BankStructure>) =>
  fixesFor(session, { structure: { ...session.structure, ...change } });

/** What a CSV column can hold, for choosing it (in English: translate it with `t`). */
export const ROLES: [keyof BankColumns, string][] = [
  ["date", msg("Date")],
  ["payee", msg("Payee")],
  ["memo", msg("Memo")],
  ["amount", msg("Amount")],
  ["outflow", msg("Money out")],
  ["inflow", msg("Money in")],
  ["id", msg("Bank ID")],
];
/** What each column holds now, by its place. */
/** Where a chosen column is: by its name, or by its place ("#4") where its name doesn't say which (bank_csv.rs `position`). */
export function columnAt(headers: string[], name: string | null | undefined) {
  if (!name) return -1;
  const named = headers.findIndex((h) => h.trim() === name.trim());
  if (named >= 0) return named;
  const place = /^#+(\d+)$/.exec(name.trim());
  return place && Number(place[1]) >= 1 && Number(place[1]) <= headers.length ? Number(place[1]) - 1 : -1;
}
/** How a column is chosen: by its name when no other column has it, else by its place. */
export function columnName(headers: string[], column: number) {
  const name = headers[column]?.trim() ?? "";
  if (name && headers.filter((h) => h.trim() === name).length === 1) return headers[column];
  // A place, written so no column's own name reads as it ("##4" where a column is named "#4").
  let place = `#${column + 1}`;
  while (headers.some((h) => h.trim() === place)) place = `#${place}`;
  return place;
}
export function roles(headers: string[], columns: BankColumns | undefined | null) {
  const out: (keyof BankColumns | null)[] = headers.map(() => null);
  for (const [role] of ROLES) {
    const at = columnAt(headers, columns?.[role]);
    if (at >= 0) out[at] = role;
  }
  return out;
}
/** The columns with one column's role changed: a role is one column's at a time. */
export function withRole(headers: string[], columns: BankColumns | undefined | null, column: number, role: keyof BankColumns | null) {
  const next: BankColumns = { ...(columns ?? {}) };
  for (const [r] of ROLES) if (columnAt(headers, next[r]) === column) next[r] = null;
  if (role) next[role] = columnName(headers, column);
  // One amount column, or money out and in apart.
  if (role === "amount") next.outflow = next.inflow = null;
  if (role === "outflow" || role === "inflow") next.amount = null;
  return next;
}

/** A date the orders read differently, read day first and month first. */
export function readings(example: string): { dmy: string; mdy: string } | null {
  const parts = example.match(/(\d{1,2})\D(\d{1,2})\D(\d{2,4})/);
  if (!parts) return null;
  const [, a, b, y] = parts;
  const year = y.length === 2 ? 2000 + Number(y) : Number(y);
  // In Arabic, with Arabic month names and Western digits.
  const options: Intl.DateTimeFormatOptions = { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" };
  const name = (d: number, m: number) => {
    const date = new Date(Date.UTC(year, m - 1, d));
    return getLanguage() === "ar" ? localDate(date, options) : date.toLocaleDateString("en-GB", options);
  };
  return { dmy: name(Number(a), Number(b)), mdy: name(Number(b), Number(a)) };
}
