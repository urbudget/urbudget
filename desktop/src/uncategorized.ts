import { placeholder } from "./bankNames";
import { today, type Budget, type Split, type Transaction } from "./types";
import { leftUnassigned } from "./accounting";
import { msg } from "./i18n";
export { leftUnassigned } from "./accounting";

/** "Needs a category": what spending with no category shows instead of a category's name; show it with t(). */
export const NEEDS_CATEGORY = msg("Needs a category");

/**
 * Spending with no category yet, as YNAB's "This needs a category". Until it has one it's
 * overspending, as YNAB's Uncategorized row: cash spending comes out of Unassigned the month
 * after; credit spending creates debt without reserving payment money. Both need attention,
 * rather than passing for an intentional choice.
 * Only money leaving the budget needs one: money coming in with none is income for Unassigned,
 * and a transfer between budget accounts needs none. YNAB's Uncategorized category counts as
 * none. Scheduled transactions aren't counted until they're entered.
 *
 * Returns a test for a transaction (any of its lines) or one line of it, made once per budget;
 * its `spent` is how much a transaction spends that way.
 */
export function uncategorizedTest(b: Pick<Budget, "accounts" | "categories">, now = today()) {
  const onBudget = new Map(b.accounts.map((a) => [a.id, a.kind !== "tracking" && a.onBudget !== false]));
  const empty = new Set(b.categories.filter((c) => placeholder(b, c.id)).map((c) => c.id)).add("local-uncategorized");
  // How much a line spends with no category: its money out, seen from the budget, or 0.
  const missing = (t: Transaction, s: Split) => {
    if (t.date > now) return 0;
    const self = onBudget.get(t.accountId) ?? false;
    const peer = s.transferAccountId ?? t.transferAccountId;
    const other = peer ? (onBudget.get(peer) ?? false) : null;
    // The line's amount seen from the budget: categories only ever apply on the budget side.
    const amount = other === null ? (self ? s.amount : 0) : self === other ? 0 : self ? s.amount : -s.amount;
    if (amount >= 0) return 0;
    const none = s.categoryId === null ? !leftUnassigned(t, s) : empty.has(s.categoryId);
    return none ? -amount : 0;
  };
  const test = (t: Transaction, s?: Split) => (s ? missing(t, s) > 0 : t.splits.some((p) => missing(t, p) > 0));
  return Object.assign(test, { spent: (t: Transaction) => t.splits.reduce((n, s) => n + missing(t, s), 0) });
}

/**
 * The entered transactions that need a category: how many, how much they spend, and how many by
 * the budget account whose money they spend (for the sidebar's marks).
 */
export function uncategorizedCounts(b: Pick<Budget, "accounts" | "categories" | "transactions">, now = today()) {
  const test = uncategorizedTest(b, now);
  const tracking = new Set(b.accounts.filter((a) => a.kind === "tracking" || a.onBudget === false).map((a) => a.id));
  const byAccount = new Map<string, number>();
  let total = 0;
  let amount = 0;
  for (const t of b.transactions) {
    if (!test(t)) continue;
    total++;
    amount += test.spent(t);
    // A tracking-side split can spend several budget accounts. Count this transaction once
    // for each account with a missing-category line, excluding its categorized transfers.
    const affected = new Set<string>();
    for (const s of t.splits) {
      if (!test(t, s)) continue;
      const account = tracking.has(t.accountId) ? (s.transferAccountId ?? t.transferAccountId) : t.accountId;
      if (account) affected.add(account);
    }
    for (const account of affected) byAccount.set(account, (byAccount.get(account) ?? 0) + 1);
  }
  return { total, amount, byAccount };
}
