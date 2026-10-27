import { createContext, useContext, useEffect, useState } from "react";
import { api } from "./bridge";
import { accountType, isLoanType } from "./accounts";
import { activeFormat, errorText, shiftMonth } from "./format";
import {
  parseMoney,
  today,
  type Account,
  type Budget,
  type LoanProjection,
  type LoanTerms,
  type Transaction,
} from "./types";
import { msg, t } from "./i18n";
/** How often a schedule repeats, in English; show with t(). */
export const frequencies: Record<string, string> = {
  never: msg("Once"),
  daily: msg("Daily"),
  weekly: msg("Weekly"),
  everyOtherWeek: msg("Every other week"),
  twiceAMonth: msg("Twice a month"),
  every4Weeks: msg("Every four weeks"),
  monthly: msg("Monthly"),
  everyOtherMonth: msg("Every other month"),
  every3Months: msg("Every three months"),
  every4Months: msg("Every four months"),
  twiceAYear: msg("Twice a year"),
  yearly: msg("Yearly"),
  everyOtherYear: msg("Every other year"),
};

/*
 * Loans. A debt account's details (rate, payment, fees or escrow, payment category and dated
 * rate changes) live in `budget.loans`, one entry per account id; the account form edits them
 * (LoanDetails.tsx). The register shows where the loan stands (LoanHeader.tsx) and opens the
 * payoff planner (LoanPlanner.tsx), which only reads. The maths is Rust's (loans.rs), reached
 * through api.projectLoan and api.loanPaymentByDate.
 */

/** An account's saved loan details. */
export const loanTerms = (b: Pick<Budget, "loans">, accountId: string) =>
  b.loans?.find((t) => t.accountId === accountId);

/** Whether an account has loan details to show or add: a tracking debt, or any account with them saved. */
export const hasLoan = (b: Pick<Budget, "loans">, a: Account, balance?: number) =>
  !!loanTerms(b, a.id) || (a.kind === "tracking" && isLoanType(accountType(a, balance)));

/** What's still owed on a loan account, from its balance. */
export const owedOn = (balance: number) => Math.max(0, -balance);

/** The annual rate in effect in a month (YYYY-MM), in thousandths of a percent, as loans.rs reads it. */
export function rateIn(t: LoanTerms, month: string) {
  const dated = (t.rateHistory ?? [])
    .filter((r) => r.month <= month)
    .sort((a, b) => b.month.localeCompare(a.month))[0];
  return dated ? dated.annualRateMilliPercent : t.annualRateBasisPoints * 10;
}
/** Thousandths of a percent as "5.25%". */
export const percent = (milli: number) => `${Number((milli / 1000).toFixed(3))}%`;

/** A rate as typed, with the budget's decimal mark ("5,25"), as written the usual way ("5.25"). */
const pointed = (text: string) => {
  const typed = text.trim().replace("%", "");
  return activeFormat().number.decimal === "," ? typed.replace(",", ".") : typed;
};
/** Basis points as someone would type the rate: "5.25", or "5,25" with a comma decimal mark. */
export const rateText = (basisPoints: number) => String(basisPoints / 100).replace(".", activeFormat().number.decimal);
/** A typed annual rate ("5.25") in basis points; two decimals at most, from 0 to 1,000%. */
export function parseRate(text: string) {
  let n: number;
  try {
    n = parseMoney(pointed(text));
  } catch {
    throw new Error(t("Enter the interest rate as a percentage, like 5.25."));
  }
  if (n < 0 || n > 100000) throw new Error(t("The interest rate must be from 0 to 1,000%."));
  return n;
}
/** A dated rate change ("5.125") in thousandths of a percent. */
export function parseRateChange(text: string) {
  const typed = pointed(text);
  if (!/^\d+(\.\d{1,3})?$/.test(typed)) throw new Error(t("Enter a rate with up to three decimals."));
  const [whole, fraction = ""] = typed.split(".");
  const n = Number(whole) * 1000 + Number(fraction.padEnd(3, "0"));
  if (n > 1000000) throw new Error(t("Rate must be between 0 and 1,000%."));
  return n;
}

/** Whether a transaction pays money into the loan account. */
const paysInto = (t: Transaction, accountId: string) =>
  (t.transferAccountId === accountId && t.amount < 0) ||
  t.splits.some((s) => s.transferAccountId === accountId && s.amount < 0) ||
  (t.accountId === accountId && t.amount > 0 && !!t.transferAccountId);

/**
 * When the next payment falls, for projections: the 1st of next month once this month's payment
 * is in (its interest is charged), else today.
 */
export function nextPayment(b: Budget, accountId: string, now = today()) {
  const month = now.slice(0, 7);
  const paid = b.transactions.some((t) => t.date.startsWith(month) && t.date <= now && paysInto(t, accountId));
  return paid ? `${shiftMonth(month, 1)}-01` : now;
}

export type Projection = { projection: LoanProjection | null; error: string };
/**
 * The loan's payments from `start` until it's paid off (loans.rs's project), with `extra` on
 * top of each and `lump` once with the first. Nothing to project when nothing is owed; a
 * payment too small to cover the interest comes back as `error`.
 */
export function useProjection(owed: number, terms: LoanTerms | undefined, start: string, extra = 0, lump = 0): Projection {
  const [result, setResult] = useState<(Projection & { key: string }) | null>(null);
  const key = JSON.stringify([owed, terms, start, extra, lump]);
  useEffect(() => {
    if (!terms || owed <= 0) return;
    let live = true;
    Promise.resolve()
      .then(() => api.projectLoan(owed, terms, start, extra, lump))
      .then(
        (projection) => live && setResult({ key, projection, error: "" }),
        (e) => live && setResult({ key, projection: null, error: errorText(e) }),
      );
    return () => {
      live = false;
    };
  }, [key]);
  return terms && owed > 0 && result?.key === key ? result : { projection: null, error: "" };
}

/** What a loan's header, the inspector and the account menu can open. App provides them. */
export type LoanActions = {
  /** Records a payment to the loan (LoanPayment.tsx). */
  record: (a: Account) => void;
  /** Opens the account's register with its payoff planner. */
  plan: (a: Account) => void;
  /** Edits the account at its loan details. */
  details: (a: Account) => void;
};
export const LoanActionsContext = createContext<LoanActions | null>(null);
export const useLoanActions = () => useContext(LoanActionsContext);
