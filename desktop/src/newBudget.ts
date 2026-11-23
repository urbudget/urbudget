import type { DateFormat, NumberFormat } from "./format";
import { id, today, type Budget, type BudgetTemplate, type Summary, type Transaction } from "./types";
import { t } from "./i18n";

export type BudgetStart = "template" | "blank" | "copy";
export type NewBudgetOptions = {
  name: string;
  currency: string;
  numberFormat: NumberFormat;
  dateFormat: DateFormat;
  start: BudgetStart;
  /** With "template", the template chosen. */
  template?: BudgetTemplate;
};

/**
 * A template's categories, with new ids, and their group order. The engine writes the names in
 * the language in use; from then on they're the person's own names (docs/LOCALIZATION.md).
 */
export const templateCategories = (template: BudgetTemplate): Pick<Budget, "categories" | "groupOrder"> => ({
  categories: template.groups.flatMap((group) =>
    group.categories.map((name) => ({ id: id(), name, group: group.name, target: 0 })),
  ),
  groupOrder: template.groups.map((group) => group.name),
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
  if (o.start === "template" && o.template?.groups.length) Object.assign(base, templateCategories(o.template));
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
