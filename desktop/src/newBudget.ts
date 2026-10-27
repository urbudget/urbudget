import type { DateFormat, NumberFormat } from "./format";
import { id, today, type Budget, type Summary, type Transaction } from "./types";
import { msg, msgc, t } from "./i18n";

export type BudgetStart = "starter" | "blank" | "copy";
export type NewBudgetOptions = {
  name: string;
  currency: string;
  numberFormat: NumberFormat;
  dateFormat: DateFormat;
  start: BudgetStart;
};

// A new budget's starter categories are made in the language in use; from then on they're the
// person's own names (docs/LOCALIZATION.md). "Savings" the group isn't the savings account type,
// so its key has a context.
const STARTER: [string, string[]][] = [
  [msg("Bills"), [msg("Rent or mortgage"), msg("Electricity"), msg("Water"), msg("Internet"), msg("Phone")]],
  [msg("Needs"), [msg("Groceries"), msg("Transportation"), msg("Medical"), msg("Insurance")]],
  [msg("Wants"), [msg("Dining out"), msg("Entertainment"), msg("Shopping"), msg("Subscriptions")]],
  [msgc("category group", "Savings"), [msg("Emergency fund"), msg("Vacation"), msg("Gifts")]],
];

/** Bills, needs, wants and savings categories to rename or delete, and their group order. */
export const starterCategories = (): Pick<Budget, "categories" | "groupOrder"> => ({
  categories: STARTER.flatMap(([group, names]) =>
    names.map((name) => ({ id: id(), name: t(name), group: t(group), target: 0 })),
  ),
  groupOrder: STARTER.map(([group]) => t(group)),
});

/** Drops imported YNAB metadata that belongs to the old budget's history. */
const fresh = <T extends { source?: unknown }>(item: T): T => {
  const { source: _source, ...rest } = item;
  return rest as T;
};

/**
 * The first contents of a new budget. "copy" is YNAB's Fresh Start: accounts keep today's
 * balances as opening balances, and categories, targets, rules, schedules and loans carry
 * over without settled past transactions or assignments.
 */
export function newBudget(o: NewBudgetOptions, current?: { budget: Budget; summary: Summary }, asOf = today()): Budget {
  const base: Budget = {
    schemaVersion: 2,
    name: o.name.trim(),
    currency: o.currency,
    numberFormat: o.numberFormat,
    dateFormat: o.dateFormat,
    accounts: [],
    categories: [],
    transactions: [],
    assignments: [],
  };
  if (o.start === "starter") Object.assign(base, starterCategories());
  if (o.start === "copy" && current) {
    const { budget: b } = current;
    if (o.currency !== b.currency)
      throw new Error(t("A fresh start must use the same currency ({currency}). Choose an empty budget to use another currency.", { currency: b.currency }));
    // Pending entries must remain available for clearing and bank matching. Keep
    // the entire transfer if either side is pending so both balances stay intact.
    const keep = (t: Transaction) => t.date > asOf || t.status === "uncleared" || t.approved === false ||
      (!!t.transferAccountId && (t.transferStatus ?? "uncleared") === "uncleared") ||
      t.splits.some(s => s.transferAccountId && (s.transferStatus ?? "uncleared") === "uncleared");
    // Summary balances include future entries and may still be recalculating.
    // Capitalize settled history only; retain the other entries separately.
    const balance = new Map(b.accounts.map((a) => [a.id, a.openingBalance]));
    const add = (account: string, amount: number) => balance.set(account, (balance.get(account) ?? 0) + amount);
    for (const t of b.transactions) {
      if (keep(t)) continue;
      add(t.accountId, t.amount);
      if (t.transferAccountId) add(t.transferAccountId, -t.amount);
      else for (const split of t.splits)
        if (split.transferAccountId) add(split.transferAccountId, -split.amount);
    }
    base.accounts = b.accounts.map(({ simplefin: _link, ...a }) => ({
      ...fresh(a), openingBalance: balance.get(a.id) ?? a.openingBalance,
    }));
    // Reconnect bank accounts explicitly. Rust preserves the settled import IDs
    // when creating the file so a later overlap cannot replay capitalized history.
    base.transactions = b.transactions.filter(keep).map((t) => ({
      ...t, splits: t.splits.map((s) => fresh(s)),
    }));
    base.categories = b.categories.map((c) => fresh(c));
    base.categoryOrder = b.categoryOrder;
    base.groupOrder = b.groupOrder;
    base.payeeRules = b.payeeRules;
    base.payees = b.payees;
    base.loans = b.loans;
    base.schedules = b.schedules?.filter((s) => s.nextDate);
  }
  return base;
}

export const numberPresets: { label: string; format: Pick<NumberFormat, "decimal" | "group"> }[] = [
  { label: "1,234.56", format: { decimal: ".", group: "," } },
  { label: "1.234,56", format: { decimal: ",", group: "." } },
  { label: "1 234,56", format: { decimal: ",", group: " " } },
  { label: "1'234.56", format: { decimal: ".", group: "'" } },
  { label: "1234.56", format: { decimal: ".", group: "" } },
];
