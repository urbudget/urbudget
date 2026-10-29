import { t } from "./i18n";

export type Status = "uncleared" | "cleared" | "reconciled";
export type Account = {
  id: string;
  name: string;
  kind: "cash" | "credit" | "tracking";
  onBudget?: boolean;
  /** Checking, mortgage, receivable… (see accounts.ts). */
  accountType?: string;
  /** False to leave a tracking account's balance out of net worth. */
  netWorth?: boolean;
  note?: string | null;
  source?: Record<string, unknown>;
  openingBalance: number;
  archived: boolean;
  /** The SimpleFIN Bridge account this one syncs with (see BankSync.tsx). */
  simplefin?: SimpleFinLink;
  /** How this account's bank files read, kept when one is imported (bank_csv.rs). */
  fileImport?: BankFileSettings;
};
/** Which of a CSV's columns hold what, by their header text. A column left out isn't read. */
export type BankColumns = {
  date?: string | null;
  payee?: string | null;
  memo?: string | null;
  /** One column for money in and out, negative for out. */
  amount?: string | null;
  outflow?: string | null;
  inflow?: string | null;
  /** An explicit destination account ID for a transfer. */
  transferAccountId?: string | null;
  /** The bank's own id for each transaction. */
  id?: string | null;
};
export type BankFileSettings = {
  /** Without them, the columns are found by their names. */
  columns?: BankColumns | null;
  dateFormat?: string | null;
  /** Someone confirmed that date order, for files whose dates don't settle it. */
  dateConfirmed?: boolean | null;
  flipSigns?: boolean | null;
  /** A workbook's sheet, by its name. */
  sheet?: string | null;
  /** The account in files with several, by its label. */
  source?: string | null;
};
export type SimpleFinLink = {
  /** The SimpleFIN account's id. */
  id: string;
  /** Its name and bank, to show before SimpleFIN has listed the accounts. */
  name?: string;
  org?: string;
  /** The first day the first sync brings in. */
  from: string;
  /** The day of the last sync imported; the next starts a week before it. */
  lastSync?: string;
  /** For a bank that sends a card's purchases as money in. */
  flipSigns?: boolean;
};
export type Category = {
  id: string;
  name: string;
  group: string;
  target: number;
  paymentAccountId?: string | null;
  hidden?: boolean;
  note?: string | null;
  goal?: Record<string, unknown> | null;
  source?: Record<string, unknown>;
};
export type Split = {
  transferAccountId?: string | null;
  transferStatus?: Status;
  transferSource?: Record<string, unknown> | null;
  categoryId: string | null;
  /** Left without a category on purpose, so its money goes to or comes from Unassigned. Spending
   * with no category and without this needs one (uncategorized.ts). */
  unassigned?: boolean;
  amount: number;
  id?: string;
  memo?: string | null;
  payee?: string | null;
  source?: Record<string, unknown>;
};
export type Transaction = {
  id: string;
  date: string;
  accountId: string;
  payee: string;
  memo: string;
  amount: number;
  status: Status;
  splits: Split[];
  transferAccountId: string | null;
  transferStatus: Status | null;
  flag?: string | null;
  approved?: boolean;
  /** The payee as the bank file wrote it, kept when the payee rules rename it. */
  importedPayee?: string;
  /**
   * When it was added (ISO-8601 UTC with milliseconds), which orders transactions on the same
   * date (order.ts). Rust sets it once, when it's entered, imported or copied; edits keep it.
   * Ones from before the app kept it, and YNAB's, have none.
   */
  createdAt?: string;
  // An import's own copy of the transaction (`source`) stays with Rust: the window never gets it.
  /** Derived by Rust so edits retain imported future entries in the ledger. */
  hasImportSource?: boolean;
  /** An imported opening entry; only excluded while its original payee/category role is kept. */
  startingBalance?: boolean;
  transferSource?: Record<string, unknown> | null;
};
export type Schedule = {
  id: string;
  nextDate: string | null;
  anchorDate: string;
  frequency: string;
  paused: boolean;
  transaction: Transaction;
};
export type LoanTerms = {
  rateHistory?: { month: string; annualRateMilliPercent: number }[];
  accountId: string;
  categoryId: string;
  annualRateBasisPoints: number;
  monthlyPayment: number;
  monthlyFees: number;
};
export type LoanProjection = {
  months: number;
  totalInterest: number;
  totalFees: number;
  payoffDate: string;
  schedule: {
    date: string;
    payment: number;
    interest: number;
    fees: number;
    principal: number;
    balance: number;
  }[];
};
export type ImportResult = {
  reviewId?: string | null;
  /** Changes made both here and in YNAB. `about` and `field` name them in words. */
  conflicts?: {
    path: string;
    local: unknown;
    remote: unknown;
    about?: string | null;
    field?: string | null;
    /** Each side in words, where the raw value is a record or a list of split lines. */
    localText?: string | null;
    remoteText?: string | null;
  }[];
  snapshot: Snapshot;
  elapsedMs: number;
  backup: string | null;
  incremental: boolean;
  unchanged: boolean;
  /** Totals that came out different from YNAB's; with `reviewId`, the import waits until they're accepted. */
  differences?: ImportDifference[];
  /** The saved token this import remembered. */
  savedToken?: string | null;
};
/** A total that came out different from YNAB's when importing (cents). */
export type ImportDifference = {
  kind: "account" | "category" | "readyToAssign";
  account?: string;
  category?: string;
  month?: string;
  /** balance, cleared_balance or uncleared_balance; budgeted, activity or balance for a category. */
  field?: string;
  expected: number;
  actual: number;
};
/** One YNAB import, as the import form asks for it: a pasted token, or a saved one by id. */
export type YnabImportRequest = {
  token: string;
  planId: string;
  replace: boolean;
  /** Remember the pasted token. */
  remember?: boolean;
  savedToken?: string;
  /** The names of the plans the token reads, which a saved token is named by. */
  tokenLabel?: string;
};
/** A YNAB token kept on this device. Its value never leaves Rust. */
export type SavedYnabToken = { id: string; label: string; hint: string; savedAt: string; lastUsed: string | null };
/** A plan a YNAB token can read, as the import form offers it. */
export type YnabPlan = { id: string; name: string; lastModifiedOn: string | null };
/** A change to a month's assignments, as the month's recent changes and each category's moves list it. */
export type MoneyMove = {
  at: string;
  month: string;
  description: string;
  /** How it came about. Entries made before moves were itemised have no kind or changes. */
  kind?: "assign" | "move" | "quickAssign" | "merge";
  /** The quick-assign option, or the merged (deleted) category's name. */
  detail?: string;
  /** What it did to each category's Assigned (minor units). */
  changes?: { categoryId: string; amount: number }[];
};
/** One month (and category, for underfunded amounts) where the import differs from YNAB's screen.
 * Amounts are minor units, age of money is days; null is nothing shown. */
export type YnabScreenDifference = { month: string; category?: string; expected: number | null; actual: number | null };
export type Budget = {
  /** The bank names payees go by (see bankNames.ts). */
  payeeRules?: {
    id: string;
    match: string;
    payee: string;
    categoryId: string | null;
  }[];
  moneyMoves?: MoneyMove[];
  categoryOrder?: string[];
  groupOrder?: string[];
  schedules?: Schedule[];
  loans?: LoanTerms[];
  schemaVersion: number;
  ynabPlanId?: string;
  importedAt?: string;
  /** "ynabFiles" when the budget came from YNAB's export files rather than its API. */
  importedFrom?: string;
  importAudit?: {
    source?: string;
    accountChecks: number;
    categoryMonthChecks: number;
    monthChecks: number;
    accountDifferences: unknown[];
    categoryDifferences: unknown[];
    readyToAssignDifferences: unknown[];
    /** Differences the person chose to import with. */
    acceptedDifferences?: ImportDifference[];
    /** Differences from what YNAB's screens show. They don't stop an import; older imports lack them. */
    shownReadyToAssignDifferences?: YnabScreenDifference[];
    ageOfMoneyDifferences?: YnabScreenDifference[];
    underfundedDifferences?: YnabScreenDifference[];
  };
  importCounts?: {
    sourceTransactions: number;
    transferPairs: number;
    localTransactions: number;
    splits: number;
  };
  sourceExport?: unknown;
  /** Known payees. `categoryId` is a default category for new transactions ("__rta" for Ready to Assign). */
  payees?: { id: string; name: string; categoryId?: string | null }[];
  ynabMonths?: { month: string; note: string }[];
  /** Set on the sample budget the welcome screen opens; `from` is the budget it was opened from. */
  sample?: { from?: string };
  /** The income expected each month (minor units), set beside the month's targets in the budget
   * page. YNAB's API doesn't export its expected income, so imports leave it unset. */
  expectedIncome?: number;
  /** This budget's names for the flag colors ("red": "Reimburse"); a color without one goes by its color. */
  flagNames?: Record<string, string>;
  name: string;
  currency: string;
  numberFormat?: import("./format").NumberFormat;
  dateFormat?: import("./format").DateFormat;
  accounts: Account[];
  categories: Category[];
  transactions: Transaction[];
  assignments: { month: string; categoryId: string; amount: number }[];
};
export type DatabaseStatus = {
  token: string;
  savedAt: string;
  databasePath: string;
};
export type Snapshot = DatabaseStatus & { budget: Budget };
export type Summary = {
  readyToAssign: number;
  futureAssigned?: number;
  futureReadyToAssign?: number;
  futureShortfalls?: { month: string; amount: number }[];
  assigned: number;
  activity: number;
  available: number;
  income: number;
  /** Spending this month with no category (negative): overspending, as YNAB's Uncategorized row.
   * Its cash comes out of Ready to Assign the month after. */
  uncategorized?: number;
  /** Of that, what cards spent beyond their own cash: debt, like credit overspending. */
  uncategorizedCredit?: number;
  netWorth: number;
  accounts: {
    id: string;
    balance: number;
    cleared: number;
    uncleared: number;
  }[];
  categories: {
    id: string;
    assigned: number;
    activity: number;
    available: number;
    carried: number;
    creditOverspending?: number;
    needed?: number;
    targetFundingRequired?: number;
    targetFundingFunded?: number;
    /** What the target asks for this month before carryover and this month's assignment. */
    monthTarget?: number;
    /** When this month's target funding is due: every month on this day ("by the 31st"), or a
     * date in this month ("by October 1st"). Absent when it's needed over the month. */
    targetDue?: { date: string; monthly: boolean };
    upcoming?: number;
  }[];
};
export const id = () => crypto.randomUUID();
export const today = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
/**
 * Whether `t` is a scheduled transaction rather than one in the register: dated after today,
 * not yet cleared on any side, and not from a bank file (which says when it happened). It
 * waits in Scheduled and is entered on its date; schedule_future in schedules.rs draws the
 * same line for transactions saved before.
 */
export const scheduledAhead = (t: Transaction) =>
  t.date > today() &&
  !t.hasImportSource &&
  !("source" in t) &&
  !t.id.startsWith("scheduled:") &&
  !t.id.startsWith("loan-interest:") &&
  t.status === "uncleared" &&
  (t.transferStatus ?? "uncleared") === "uncleared" &&
  t.splits.every((s) => !s.transferAccountId || (s.transferStatus ?? "uncleared") === "uncleared") &&
  !("bankImportIds" in t);
export const parseMoney = (text: string): number => {
  const cleaned = text.trim().replace(/,/g, "");
  if (!/^-?\d+(\.\d{0,2})?$/.test(cleaned))
    throw new Error(t("Enter an amount with at most two decimal places."));
  const [whole, decimal = ""] = cleaned.replace("-", "").split(".");
  const amount =
    (Number(whole) * 100 + Number(decimal.padEnd(2, "0"))) *
    (cleaned.startsWith("-") ? -1 : 1);
  if (!Number.isSafeInteger(amount) || Math.abs(amount) > 1_000_000_000_000)
    throw new Error(t("Amount is too large."));
  return amount;
};
export const fingerprint = (b: Budget) =>
  JSON.stringify({
    ...b,
    assignments: [...b.assignments].sort((a, c) =>
      `${a.month}:${a.categoryId}`.localeCompare(`${c.month}:${c.categoryId}`),
    ),
  });
