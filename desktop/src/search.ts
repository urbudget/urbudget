import { READY_TO_ASSIGN } from "./ui";
import { evaluateAmount, money } from "./format";
import { FLAGS, flagName } from "./TransactionRow";
import type { Budget, Status } from "./types";
import { msg, t } from "./i18n";

/**
 * One search filter, shown as a chip in the register's search bar. Chips on the same field
 * widen the search (Payee: A or Payee: B); different fields narrow it. Text chips each have
 * to match somewhere.
 */
export type Term =
  | { field: "text" | "payee" | "memo"; value: string }
  | { field: "category" | "account"; value: string; id?: string }
  | { field: "amount"; op: "=" | ">=" | "<="; value: number }
  | { field: "flag"; value: string }
  | { field: "status"; value: Status | "unreconciled" }
  | { field: "approval"; value: "needed" }
  /** Spending with no category yet (uncategorized.ts). */
  | { field: "categorization"; value: "needed" };
export type Field = Term["field"];

const STATUS: Record<string, string> = {
  uncleared: msg("Uncleared"),
  cleared: msg("Cleared"),
  reconciled: msg("Reconciled"),
  unreconciled: msg("Not reconciled"),
};
/** A chip's text. Search words (payee:, is:cleared) stay as typed; the chips speak the app's language. */
export function termLabel(term: Term, b?: Pick<Budget, "flagNames">) {
  switch (term.field) {
    case "text":
      return `“${term.value}”`;
    case "payee":
      return t("Payee: {value}", { value: term.value });
    case "memo":
      return t("Memo: {value}", { value: term.value });
    case "category":
      return t("Category: {value}", { value: term.id === READY_TO_ASSIGN ? t("Unassigned") : term.value });
    case "account":
      return t("Account: {value}", { value: term.value });
    case "amount":
      // The sign isn't isolated, so Arabic mirrors it with the sentence.
      return term.op === "="
        ? t("Amount = {amount}", { amount: money(term.value) })
        : term.op === ">="
          ? t("Amount ≥ {amount}", { amount: money(term.value) })
          : t("Amount ≤ {amount}", { amount: money(term.value) });
    case "flag":
      return term.value === "any"
        ? t("Flagged")
        : term.value === "none"
          ? t("No flag")
          : t("{name} flag", { name: flagName(term.value, b) });
    case "status":
      return t(STATUS[term.value]);
    case "approval":
      return t("Needs approval");
    case "categorization":
      return t("Needs a category");
  }
}
export const sameTerm = (a: Term, b: Term) => JSON.stringify(a) === JSON.stringify(b);

/** A row as search sees it: lower-case text for each field. */
export type Searchable = {
  text: string;
  payee: string;
  memo: string;
  categoryIds: Set<string | null>;
  categories: string;
  accountIds: Set<string | null>;
  account: string;
  amount: number;
  flag: string;
  status: Status;
  approved: boolean;
  /** Spending that needs a category (uncategorized.ts); scheduled rows never do yet. */
  uncategorized?: boolean;
};
const matches = (t: Term, r: Searchable): boolean => {
  switch (t.field) {
    case "text":
      return r.text.includes(t.value.toLowerCase());
    case "payee":
      return r.payee.includes(t.value.toLowerCase());
    case "memo":
      return r.memo.includes(t.value.toLowerCase());
    case "category":
      return t.id
        ? r.categoryIds.has(t.id === READY_TO_ASSIGN ? null : t.id)
        : r.categories.includes(t.value.toLowerCase());
    case "account":
      return t.id ? r.accountIds.has(t.id) : r.account.includes(t.value.toLowerCase());
    case "amount": {
      const a = Math.abs(r.amount);
      return t.op === "=" ? a === t.value : t.op === ">=" ? a >= t.value : a <= t.value;
    }
    case "flag":
      return t.value === "any" ? !!r.flag : t.value === "none" ? !r.flag : r.flag === t.value;
    case "status":
      return t.value === "unreconciled" ? r.status !== "reconciled" : r.status === t.value;
    case "approval":
      return !r.approved;
    case "categorization":
      return !!r.uncategorized;
  }
};
/** A predicate for a set of terms: OR within a field, AND across fields; text and amounts all AND. */
export function compile(terms: Term[]): (r: Searchable) => boolean {
  const groups = new Map<string, Term[]>();
  terms.forEach((t, i) => {
    const key = t.field === "text" || t.field === "amount" ? `${t.field}:${i}` : t.field;
    groups.set(key, [...(groups.get(key) ?? []), t]);
  });
  const all = [...groups.values()];
  return (r) => all.every((group) => group.some((t) => matches(t, r)));
}

/** "Amount" input: "25", ">100", "<= 50", "=12.5". */
export function parseAmountTerm(text: string): Term | null {
  const m = text.trim().match(/^(=|>=?|<=?|≥|≤)?\s*([\d.,+\-*/() ]+)$/);
  if (!m || !/\d/.test(m[2])) return null;
  try {
    const value = Math.abs(evaluateAmount(m[2]));
    const op = !m[1] || m[1] === "=" ? "=" : m[1].startsWith(">") || m[1] === "≥" ? ">=" : "<=";
    return { field: "amount", op, value };
  } catch {
    return null;
  }
}

export type Suggestion = { term?: Term; prefix?: string; label: string; hint?: string };
const PREFIXES: [string, Field, string][] = [
  ["payee:", "payee", msg("Payee")],
  ["category:", "category", msg("Category")],
  ["account:", "account", msg("Account")],
  ["memo:", "memo", msg("Memo")],
  ["amount:", "amount", msg("Amount")],
  ["is:", "status", msg("Status, flag, approval or category")],
];
const ALIASES: Record<string, string> = { cat: "category", acct: "account", amt: "amount", flag: "is", status: "is" };
/** What "is:" offers. A named flag answers to its name and its color. */
const states = (b?: Pick<Budget, "flagNames">): { words: string; term: Term }[] => [
  { words: "uncleared", term: { field: "status", value: "uncleared" } },
  { words: "cleared", term: { field: "status", value: "cleared" } },
  { words: "reconciled", term: { field: "status", value: "reconciled" } },
  { words: "not reconciled unreconciled", term: { field: "status", value: "unreconciled" } },
  { words: "needs approval unapproved approve", term: { field: "approval", value: "needed" } },
  { words: "needs a category uncategorized", term: { field: "categorization", value: "needed" } },
  { words: "flagged any flag", term: { field: "flag", value: "any" } },
  { words: "no flag unflagged", term: { field: "flag", value: "none" } },
  ...FLAGS.map(([value, color]) => {
    const name = flagName(value, b);
    const words = name === color ? `${color} flag` : `${name} ${color} flag`;
    return { words: words.toLowerCase(), term: { field: "flag", value } as Term };
  }),
];

/** What to offer for the text typed into the search bar. */
export function suggest(
  input: string,
  b: Pick<Budget, "categories" | "accounts" | "flagNames">,
  payees: string[],
): Suggestion[] {
  const raw = input.trim();
  if (!raw) return PREFIXES.map(([prefix, , label]) => ({ prefix, label: `${t(label)}…`, hint: prefix }));
  const fielded = raw.match(/^([a-z]+):\s*(.*)$/i);
  const field = fielded ? (ALIASES[fielded[1].toLowerCase()] ?? fielded[1].toLowerCase()) : null;
  const q = (fielded ? fielded[2] : raw).toLowerCase();
  const has = (s: string) => s.toLowerCase().includes(q);
  const out: Suggestion[] = [];
  const add = (term: Term) => out.push({ term, label: termLabel(term, b) });
  // "> 100", "<=50", "=25": clearly an amount, so offer only that.
  if (!field && /^(=|>|<|≥|≤)/.test(raw)) {
    const amount = parseAmountTerm(raw);
    if (amount) add(amount);
    return out;
  }
  if (!field) add({ field: "text", value: raw });
  if (!field || field === "payee") {
    payees.filter(has).slice(0, field ? 12 : 5).forEach((p) => add({ field: "payee", value: p }));
    if (field && q) add({ field: "payee", value: fielded![2] });
  }
  if (!field || field === "category") {
    if ("unassigned".includes(q) || t("Unassigned").toLowerCase().includes(q))
      add({ field: "category", value: "Unassigned", id: READY_TO_ASSIGN });
    // "is:" offers it too; here only when asked for by field, so it isn't listed twice.
    if (field && q && ("needs a category".startsWith(q) || "uncategorized".startsWith(q)))
      add({ field: "categorization", value: "needed" });
    b.categories
      .filter((c) => has(c.name))
      .slice(0, field ? 12 : 4)
      .forEach((c) => add({ field: "category", value: c.name, id: c.id }));
  }
  if (!field || field === "account")
    b.accounts
      .filter((a) => has(a.name))
      .slice(0, field ? 12 : 3)
      .forEach((a) => add({ field: "account", value: a.name, id: a.id }));
  if (q && (!field || field === "memo")) add({ field: "memo", value: fielded ? fielded[2] : raw });
  if (!field || field === "amount") {
    const amount = parseAmountTerm(fielded ? fielded[2] : raw);
    if (amount) {
      add(amount);
      if (amount.field === "amount" && amount.op === "=") {
        add({ ...amount, op: ">=" });
        add({ ...amount, op: "<=" });
      }
    }
  }
  if (!field || field === "is")
    // Match the start of a word ("red" is a flag, not the end of "uncleared").
    states(b).filter((s) => !q || s.words.startsWith(q) || s.words.split(" ").some((w) => w.startsWith(q))).forEach((s) =>
      add(s.term),
    );
  return out;
}
