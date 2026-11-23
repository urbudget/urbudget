// The setup walkthrough's answers and what the device keeps about them
// (docs/plans/first-budget-walkthrough.md): the draft of a budget that isn't made yet, and, per
// budget, whether it shows the setup card and what was chosen on the last step. The engine
// builds the budget from the answers (setup.rs); nothing here decides what a budget holds.
import type { FormatValue } from "./BudgetFormatFields";
import { cardPaymentCategory } from "./accounts";
import { groupNames } from "./categories";
import { evaluateAmount, numberFormatFor, type DateFormat, type NumberFormat } from "./format";
import { msg } from "./i18n";
import { ar } from "./i18n/ar";
import { localeDefaults } from "./locale";
import { id, type Account, type Budget, type BudgetTemplate, type Category } from "./types";

/** The account types the first step offers; the full list stays in the account form. */
export const SETUP_ACCOUNT_TYPES = ["checking", "savings", "cash", "creditCard"] as const;
export type SetupAccountType = (typeof SETUP_ACCOUNT_TYPES)[number];
/** An account as typed: the balance is text until the budget is made. */
export type AccountRow = { name: string; type: SetupAccountType; balance: string };
/** What a category costs, as typed, for a month or for a year. */
export type AmountEntry = { amount: string; yearly?: boolean };
/** How spending will keep arriving: typed in, from the bank's file, or from a bank connection. */
export type KeepRoute = "type" | "file" | "bank";
/** The answers so far for a budget that isn't made yet. */
export type SetupDraft = {
  step: 1 | 2 | 3 | 4;
  name: string;
  format: FormatValue;
  accounts: AccountRow[];
  template: string;
  /** Template categories left out, by place: "<group>.<category>" from 0 (setup.rs). */
  leftOut: string[];
  /** Names of categories added; one taken back is left blank so the others keep their place. */
  added: string[];
  /** By the category's place: a template's, or "+<n>" for an added one. */
  amounts: Record<string, AmountEntry>;
  income: string;
};

/** The answers as the engine takes them (setup::Answers). Amounts are cents. */
export type SetupAnswers = {
  name: string;
  currency: string;
  numberFormat?: NumberFormat;
  dateFormat?: DateFormat;
  accounts: { name: string; type: SetupAccountType; balance: number }[];
  template?: string | null;
  leftOut?: string[];
  added?: string[];
  amounts?: { category: string; amount: number; period: "month" | "year" }[];
  expectedIncome?: number | null;
};

/** What Underfunded would assign to a budget that isn't in the library yet (setup::Preview). */
export type SetupPreview = {
  readyToAssign: number;
  needed: number;
  /** The categories whose Assigned would change, with the money there is. */
  changes: { id: string; before: number; after: number }[];
  /** The same with money for everything, which says what went short. */
  wanted: { id: string; before: number; after: number }[];
};

export const blankRow = (): AccountRow => ({ name: "", type: "checking", balance: "" });

/** A new draft, in the formats the first screen settled on or the device's. */
export function newDraft(start: { name: string; format?: FormatValue }): SetupDraft {
  const guess = localeDefaults();
  return {
    step: 1,
    name: start.name,
    format: start.format ?? guess,
    accounts: [blankRow()],
    template: "everyday",
    leftOut: [],
    added: [],
    amounts: {},
    income: "",
  };
}

/** Where the first screen's draft is kept. A walkthrough on a budget that exists needs none: its answers are in the budget. */
const DRAFT_KEY = "lb:setup-draft:welcome";
/** The first screen's draft, if a walkthrough was started there and not finished. */
export function readDraft(): SetupDraft | null {
  try {
    const saved = JSON.parse(localStorage.getItem(DRAFT_KEY) ?? "null") as Partial<SetupDraft> | null;
    if (!saved || typeof saved !== "object" || typeof saved.name !== "string" || !saved.format?.currency) return null;
    const fresh = newDraft({ name: saved.name });
    const step = [1, 2, 3, 4].includes(saved.step as number) ? saved.step! : 1;
    return {
      step,
      name: saved.name,
      income: typeof saved.income === "string" ? saved.income : fresh.income,
      template: typeof saved.template === "string" ? saved.template : fresh.template,
      format: { ...saved.format, numberFormat: numberFormatFor(saved.format.currency, saved.format.numberFormat) },
      accounts: Array.isArray(saved.accounts) && saved.accounts.length ? saved.accounts : fresh.accounts,
      leftOut: Array.isArray(saved.leftOut) ? saved.leftOut : [],
      added: Array.isArray(saved.added) ? saved.added : [],
      amounts: saved.amounts && typeof saved.amounts === "object" ? saved.amounts : {},
    };
  } catch {
    return null;
  }
}
export function writeDraft(draft: SetupDraft) {
  try {
    localStorage.setItem(DRAFT_KEY, JSON.stringify(draft));
  } catch {
    // Without storage the answers last until the app closes.
  }
}
export function clearDraft() {
  try {
    localStorage.removeItem(DRAFT_KEY);
  } catch {
    // Nothing was kept.
  }
}

/** An amount as typed, in cents; null for a blank. Throws what `evaluateAmount` does for one that doesn't read. */
export function typedCents(text: string, decimal: NumberFormat["decimal"]): number | null {
  return text.trim() ? evaluateAmount(text, decimal) : null;
}
/** The same, with one that doesn't read counted as blank: for totals shown while typing, and for Skip setup. */
export function looseCents(text: string, decimal: NumberFormat["decimal"]): number {
  try {
    return typedCents(text, decimal) ?? 0;
  } catch {
    return 0;
  }
}

/** A year's amount as its monthly target, rounded up so twelve months cover it (setup.rs). */
export const monthly = (cents: number, yearly?: boolean) => (yearly ? Math.ceil(cents / 12) : cents);

/**
 * The answers a draft holds when it is left on `step`. Typed accounts always count. The
 * categories and what they cost count from the third step on: until someone has been through
 * the second, the set on show is the app's suggestion, not their answer.
 */
export function answersOf(draft: SetupDraft, step: number = draft.step): SetupAnswers {
  const decimal = draft.format.numberFormat.decimal;
  const answers: SetupAnswers = {
    name: draft.name.trim(),
    currency: draft.format.currency,
    numberFormat: draft.format.numberFormat,
    dateFormat: draft.format.dateFormat,
    accounts: draft.accounts
      .filter((row) => row.name.trim())
      .map((row) => ({ name: row.name.trim(), type: row.type, balance: looseCents(row.balance, decimal) })),
  };
  if (step < 3) return answers;
  const income = looseCents(draft.income, decimal);
  return {
    ...answers,
    template: draft.template,
    leftOut: draft.leftOut,
    added: draft.added,
    amounts: Object.entries(draft.amounts)
      .map(([category, entry]) => ({ category, amount: looseCents(entry.amount, decimal), period: entry.yearly ? ("year" as const) : ("month" as const) }))
      .filter((entry) => entry.amount > 0),
    expectedIncome: income > 0 ? income : null,
  };
}

/** A category the walkthrough lists, with the place or id its amount is kept under. */
export type SetupCategory = { key: string; name: string; group: string };
/** The categories a draft's answers make, in order: the template's that weren't left out, then the added ones. */
export function draftCategories(draft: Pick<SetupDraft, "leftOut" | "added">, template: BudgetTemplate | undefined, ownGroup: string): SetupCategory[] {
  const out: SetupCategory[] = [];
  template?.groups.forEach((group, g) =>
    group.categories.forEach((name, c) => {
      const key = `${g}.${c}`;
      if (!draft.leftOut.includes(key)) out.push({ key, name, group: group.name });
    }),
  );
  draft.added.forEach((name, n) => {
    if (name.trim()) out.push({ key: `+${n}`, name: name.trim(), group: ownGroup });
  });
  return out;
}

// ---------- What the device keeps for a budget ----------

/**
 * A budget's setup on this device: whether its setup card shows, whether its money was given jobs
 * in the walkthrough, how its spending will keep arriving, and whether Budget still has to say
 * it's ready. Kept on the device, never in the budget. A budget gets one when the walkthrough
 * makes or opens it, or when it's opened while it isn't in use yet (`needsSetup`).
 */
export type SetupMark = {
  card: boolean;
  assigned?: boolean;
  keep?: KeepRoute;
  /** The walkthrough was just finished: Budget says so once, until that line is closed or used. */
  ready?: boolean;
};
const markKey = (budgetId: string) => `lb:setup:${budgetId}`;
/**
 * A budget's mark, or null for one this device hasn't given one. The same key once held "on" or
 * "off" for the getting-started checklist: one hidden there ("off") stays hidden; "on" reads as
 * no mark, and the budget is offered setup again if it still isn't in use.
 */
export function readMark(budgetId: string): SetupMark | null {
  try {
    const raw = localStorage.getItem(markKey(budgetId));
    if (raw === "off") return { card: false };
    const saved = JSON.parse(raw ?? "null") as SetupMark | null;
    return saved && typeof saved === "object" && typeof saved.card === "boolean" ? saved : null;
  } catch {
    return null;
  }
}
export function writeMark(budgetId: string, mark: SetupMark) {
  if (!budgetId) return;
  try {
    localStorage.setItem(markKey(budgetId), JSON.stringify(mark));
    window.dispatchEvent(new CustomEvent(MARK_EVENT, { detail: budgetId }));
  } catch {
    // Without storage the card shows until the app closes.
  }
}
/** Changes a budget's mark, making one (with the card showing) if it has none. */
export const updateMark = (budgetId: string, change: Partial<SetupMark>) =>
  writeMark(budgetId, { card: true, ...readMark(budgetId), ...change });
/** Sent on `window` when a budget's mark changes, with its id. */
export const MARK_EVENT = "urbudget-setup-mark";

/**
 * Whether a budget this device has no mark for is offered setup: it isn't in use yet (nothing
 * has been spent or received in it) and still lacks accounts, categories, what a month costs or
 * jobs for its money. What made it doesn't matter: New budget, another device, or an old start
 * that was left. A sample or an import was set up somewhere else.
 */
export function needsSetup(budget: Budget, unassigned: number): boolean {
  if (budget.sample || budget.importedFrom || budget.ynabPlanId || budget.transactions.length) return false;
  return stepsLeft(budget, unassigned, null).some((step) => step < 5);
}

/** Categories that are the person's own: every one but a card's payment category. */
export const ownCategories = (budget: Pick<Budget, "categories">): Category[] => budget.categories.filter((c) => !c.paymentAccountId);

/**
 * The walkthrough's steps a budget doesn't answer yet, in order: no accounts (1), no categories
 * of its own (2), no targets (3), money still unassigned that the walkthrough never assigned
 * (4), no choice of how to keep it up to date (5).
 */
export function stepsLeft(budget: Budget, unassigned: number, mark: SetupMark | null): number[] {
  const own = ownCategories(budget);
  const answered = [
    budget.accounts.length > 0,
    own.length > 0,
    own.some((c) => c.target > 0 || !!c.goal),
    unassigned <= 0 || !!mark?.assigned,
    !!mark?.keep,
  ];
  return answered.flatMap((done, i) => (done ? [] : [i + 1]));
}

// ---------- The same steps on a budget that exists, as normal edits ----------

/** Adds typed accounts to a budget, each card owing its balance and with its payment category. */
export function addAccounts(draft: Budget, rows: AccountRow[], decimal: NumberFormat["decimal"]) {
  for (const row of rows) {
    if (!row.name.trim()) continue;
    const card = row.type === "creditCard";
    const balance = typedCents(row.balance, decimal) ?? 0;
    const account: Account = {
      id: id(),
      name: row.name.trim(),
      kind: card ? "credit" : "cash",
      accountType: row.type,
      onBudget: true,
      openingBalance: card ? -balance : balance,
      archived: false,
    };
    draft.accounts.push(account);
    if (card) draft.categories.push(cardPaymentCategory(account));
  }
}

/** Adds categories to a budget in their groups, after the ones it has. */
export function addCategories(draft: Budget, categories: SetupCategory[]) {
  // The groups as they're shown now, then the new ones. A group of card payments alone stays
  // out of the order, where an unlisted group goes: last.
  const order = groupNames(draft, { empty: true }).filter(
    (group) => !draft.categories.some((c) => c.group === group) || draft.categories.some((c) => c.group === group && !c.paymentAccountId),
  );
  for (const c of categories) if (!order.includes(c.group)) order.push(c.group);
  draft.groupOrder = order;
  // Before any card's payment category, so the person's own groups come first.
  const at = draft.categories.findIndex((c) => c.paymentAccountId);
  const made = categories.map((c) => ({ id: id(), name: c.name, group: c.group, target: 0 }));
  draft.categories.splice(at < 0 ? draft.categories.length : at, 0, ...made);
}

/** The category leftover money is offered to: Emergency fund, by the name either language gives it. */
export function leftoverCategory<C extends { name: string }>(categories: C[]): C | undefined {
  // The engine names it when the budget is made, in the language then in use (budget_templates.rs).
  const english = msg("Emergency fund");
  const names: unknown[] = [english, ar[english]];
  return categories.find((c) => names.includes(c.name));
}
