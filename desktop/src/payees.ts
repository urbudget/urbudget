import { leftUnassigned } from "./accounting";
import { placeholder } from "./bankNames";
import type { Budget, Transaction } from "./types";

export type PayeeInfo = {
  name: string;
  /** Transactions with this payee. */
  count: number;
  /** Date of the most recent one. */
  last: string;
  /** The payee's default category, or else that of its most recent single-category transaction; null means Ready to Assign. */
  categoryId?: string | null;
  /** `categoryId` is the payee's default, set on the Payees page. */
  fixed?: boolean;
};

/** What a payee's transactions are usually like, for filling in its next one (TransactionEditor). */
export type PayeeHabits = {
  /** Most of its transactions were money in. */
  income: boolean;
  /** Its most recent amount that way (signed cents), when it has transactions. */
  amount?: number;
  /** The category to fill in: its default, else that of its latest transaction with one, where
   * UNASSIGNED is income for Unassigned (or money taken from it on purpose); undefined when
   * neither. */
  category?: string;
  /** The category can be taken as read: it's the default, or the only one among the payee's
   * latest 10 transactions with a category. */
  oneCategory: boolean;
  /** More than half of its latest 10 transactions typed in by hand had a memo. */
  memo: boolean;
};
/** How many of a payee's latest transactions say what its next one is likely to need. */
const RECENT = 10;
/** Typed in by hand, rather than from a bank file (whose memo is the bank's text). */
const byHand = (t: Transaction) => !t.hasImportSource && !("source" in t) && !("bankImportIds" in t);
/** Unassigned as a category: the category pickers' READY_TO_ASSIGN, as the Payees page stores it. */
const UNASSIGNED = "__rta";
type StoredPayee = NonNullable<Budget["payees"]>[number];
/** A payee of the budget's own: not one deleted, nor YNAB's transfer payees ("Transfer : Savings"),
 * which are offered as real transfers instead. */
const listed = (p: StoredPayee) => {
  const imported = p as { transfer_account_id?: string | null; deleted?: boolean };
  return !!p.name && !imported.transfer_account_id && !imported.deleted && !p.name.startsWith("Transfer : ");
};
/** A payee's default category, set on the Payees page (UNASSIGNED for Unassigned): undefined for
 * none, or for one since deleted when the categories are known. */
const defaultCategory = (p: StoredPayee, exists?: Set<string>) =>
  !p.categoryId || (p.categoryId !== UNASSIGNED && exists && !exists.has(p.categoryId)) ? undefined : p.categoryId;

/** Latest first: by date, then by when it was added. */
const latestFirst = (a: Transaction, c: Transaction) =>
  a.date !== c.date ? (a.date < c.date ? 1 : -1) : (a.createdAt ?? "") < (c.createdAt ?? "") ? 1 : -1;

/** Each payee's habits, by name, from its transactions latest first and its default category. */
export function payeeHabits(
  b: Pick<Budget, "transactions" | "categories"> & Partial<Pick<Budget, "payees" | "accounts">>,
) {
  const byPayee = new Map<string, Transaction[]>();
  for (const t of b.transactions) {
    const name = t.payee.trim();
    if (!name || t.transferAccountId || !t.amount) continue;
    const list = byPayee.get(name) ?? [];
    list.push(t);
    byPayee.set(name, list);
  }
  // A category counts, and so does Unassigned for income with none (as splitRole has it, marked
  // or not) or money taken from it on purpose; spending still waiting for one doesn't, nor YNAB's
  // Uncategorized, nor anything in an account off the budget, which has no categories.
  const empty = new Set(b.categories.filter((c) => placeholder(b, c.id)).map((c) => c.id)).add("local-uncategorized");
  const offBudget = new Set((b.accounts ?? []).filter((a) => a.kind === "tracking" || a.onBudget === false).map((a) => a.id));
  const categoryOf = (t: Transaction) => {
    const [only, ...rest] = t.splits;
    if (rest.length || !only || only.transferAccountId || offBudget.has(t.accountId)) return null;
    if (!only.categoryId) return only.amount >= 0 || leftUnassigned(t, only) ? UNASSIGNED : null;
    return empty.has(only.categoryId) ? null : only.categoryId;
  };
  const habits = new Map<string, PayeeHabits>();
  for (const [name, list] of byPayee) {
    list.sort(latestFirst);
    const income = list.filter((t) => t.amount > 0).length * 2 > list.length;
    const categories = list.map(categoryOf).filter((c) => c !== null).slice(0, RECENT);
    const typed = list.filter(byHand).slice(0, RECENT);
    habits.set(name, {
      income,
      amount: list.find((t) => (t.amount > 0) === income)?.amount,
      category: categories[0],
      oneCategory: categories.length > 0 && categories.every((c) => c === categories[0]),
      memo: typed.filter((t) => t.memo.trim()).length * 2 > typed.length,
    });
  }
  const exists = new Set(b.categories.map((c) => c.id));
  for (const p of b.payees ?? []) {
    const category = defaultCategory(p, exists);
    if (!listed(p) || category === undefined) continue;
    const habit = habits.get(p.name) ?? { income: false, oneCategory: false, memo: false };
    habits.set(p.name, { ...habit, category, oneCategory: true });
  }
  return habits;
}

/** Every payee with usage counts and the category it was last used with, most used first. */
export function payeeIndex(b: Pick<Budget, "transactions" | "payees"> & Partial<Pick<Budget, "categories">>): PayeeInfo[] {
  const index = new Map<string, PayeeInfo>();
  // The date of the transaction each payee's category came from: whatever order they come in,
  // a later split doesn't hide the latest single-category one.
  const categoryDate = new Map<string, string>();
  for (const t of b.transactions) {
    const name = t.payee.trim();
    if (!name || t.transferAccountId) continue;
    const info = index.get(name) ?? { name, count: 0, last: "" };
    info.count++;
    if (t.date >= info.last) info.last = t.date;
    if (t.splits.length === 1 && !t.splits[0].transferAccountId && t.date >= (categoryDate.get(name) ?? "")) {
      categoryDate.set(name, t.date);
      info.categoryId = t.splits[0].categoryId;
    }
    index.set(name, info);
  }
  const exists = b.categories && new Set(b.categories.map((c) => c.id));
  for (const p of b.payees ?? []) {
    if (!listed(p)) continue;
    const info = index.get(p.name) ?? { name: p.name, count: 0, last: "" };
    // A default category (skipped if that category has since been deleted) beats the last one used.
    const category = defaultCategory(p, exists);
    if (category !== undefined) {
      info.categoryId = category === UNASSIGNED ? null : category;
      info.fixed = true;
    }
    index.set(p.name, info);
  }
  return [...index.values()].sort((a, c) => c.count - a.count || a.name.localeCompare(c.name));
}
