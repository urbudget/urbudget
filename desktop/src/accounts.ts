import type { Account } from "./types";
import { msg, t } from "./i18n";

/** Account types: budget accounts, then tracking assets and liabilities. The type sets the kind. */
export const ACCOUNT_TYPES: { group: string; kind: Account["kind"]; types: [string, string][] }[] = [
  {
    group: msg("Budget accounts"),
    kind: "cash",
    types: [
      ["checking", msg("Checking")],
      ["savings", msg("Savings")],
      ["cash", msg("Cash")],
    ],
  },
  {
    group: msg("Budget accounts"),
    kind: "credit",
    types: [
      ["creditCard", msg("Credit card")],
      ["lineOfCredit", msg("Line of credit")],
    ],
  },
  {
    group: msg("Tracking assets"),
    kind: "tracking",
    types: [
      ["investment", msg("Investment")],
      ["retirement", msg("Retirement")],
      ["property", msg("Property")],
      ["vehicle", msg("Vehicle")],
      ["receivable", msg("Receivable (money owed to you)")],
      ["otherAsset", msg("Other asset")],
    ],
  },
  {
    group: msg("Tracking liabilities"),
    kind: "tracking",
    types: [
      ["mortgage", msg("Mortgage")],
      ["autoLoan", msg("Auto loan")],
      ["studentLoan", msg("Student loan")],
      ["personalLoan", msg("Personal loan")],
      ["medicalDebt", msg("Medical debt")],
      ["otherDebt", msg("Other debt")],
    ],
  },
];
const LIABILITIES = new Set(["creditCard", "lineOfCredit", "mortgage", "autoLoan", "studentLoan", "personalLoan", "medicalDebt", "otherDebt"]);
/** Balance-sheet headings for each type: keys reports compare, so translate them with t() when shown. */
const SECTIONS: Record<string, string> = {
  checking: msg("Cash and bank"),
  savings: msg("Cash and bank"),
  cash: msg("Cash and bank"),
  investment: msg("Investments"),
  retirement: msg("Investments"),
  property: msg("Property"),
  vehicle: msg("Vehicles"),
  receivable: msg("Receivables"),
  otherAsset: msg("Other assets"),
  creditCard: msg("Credit cards"),
  lineOfCredit: msg("Lines of credit"),
  mortgage: msg("Mortgages"),
  autoLoan: msg("Loans"),
  studentLoan: msg("Loans"),
  personalLoan: msg("Loans"),
  medicalDebt: msg("Other debts"),
  otherDebt: msg("Other debts"),
};
export const kindForType = (type: string): Account["kind"] =>
  ACCOUNT_TYPES.find((g) => g.types.some(([t]) => t === type))?.kind ?? "tracking";
/** An account type's name, in the language in use. */
export const typeName = (type: string) =>
  t(ACCOUNT_TYPES.flatMap((g) => g.types).find(([key]) => key === type)?.[1] ?? type);
export const isLiability = (type: string) => LIABILITIES.has(type);
/** Debts tracked outside the budget (mortgages, loans): the types with loan details and a payoff planner. */
export const isLoanType = (type: string) => isLiability(type) && kindForType(type) === "tracking";
export const sectionOf = (type: string) => SECTIONS[type] ?? (isLiability(type) ? msg("Other debts") : msg("Other assets"));

/**
 * An account's type: the one chosen, else YNAB's imported type, else a guess from its kind
 * (tracking accounts by the sign of `balance`).
 */
export function accountType(a: Account, balance = a.openingBalance): string {
  if (a.accountType && SECTIONS[a.accountType]) return a.accountType;
  const imported = typeof a.source?.type === "string" ? a.source.type : "";
  if (imported === "otherLiability") return "otherDebt";
  if (SECTIONS[imported]) return imported;
  if (a.kind === "credit") return "creditCard";
  if (a.kind === "cash") return "checking";
  return balance < 0 ? "otherDebt" : "otherAsset";
}
/** Whether the account's balance counts toward net worth (tracking accounts can opt out). */
export const inNetWorth = (a: Account) => a.netWorth !== false;
