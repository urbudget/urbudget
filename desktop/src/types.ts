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
  /** A credit account's limit in cents, positive; left out for none (see creditLeft in accounts.ts). */
  creditLimit?: number;
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
  /**
   * Written from a broker's values (investments.rs): the account's starting value, a month's
   * market change, or a month's money moved at the broker that the budget doesn't have yet.
   */
  investment?: "start" | "change" | "moved";
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
/** Something an investment account holds, as its broker reported it. */
export type Holding = {
  symbol: string;
  name: string;
  /** The broker's asset category (STK, FUND, BOND, OPT). */
  kind: string;
  currency: string;
  /** As the broker wrote them: shown, never calculated with. */
  quantity: string;
  price: string;
  /** In the budget's currency, in cents. */
  value: number;
  cost: number | null;
  gain: number | null;
  exchange?: string;
};
/** One investment account's holdings (investments.rs), in the budget's `investments`. */
export type Holdings = {
  accountId: string;
  /** Where they come from: "ibkr". */
  source: string;
  /** The account at the broker (U1234567). */
  sourceAccount: string;
  /** The day the holdings and prices are from. */
  asOf: string;
  /** The broker account's own currency, and one unit of it in the budget's on `asOf`. */
  currency: string;
  rate: string;
  manualRate?: string;
  /** The account's value in the budget's currency, in cents. */
  total: number;
  positions: Holding[];
  cash: { currency: string; amount: string; value: number }[];
  /** Value that is neither a position nor cash: dividends and interest accrued but not paid. */
  other?: number;
  /** The account's value at the end of each day, oldest first. */
  values: [string, number][];
  /**
   * Money put in (positive) or taken out on each day it happened, as the broker recorded it.
   * Left out of holdings brought in before UrBudget kept it; the engine then reads it from the
   * budget's own transfers (investment_stats.rs `flows_of`).
   */
  flows?: [string, number][];
  /** The days among `flows` that are an estimate: the value jumped with nothing listed to explain it. */
  estimated?: string[];
  /** What it earned and was charged month by month: received is positive, paid negative. */
  income?: { month: string; dividends?: number; interest?: number; tax?: number; fees?: number; other?: number }[];
  /** Commissions on trades over a statement's period. */
  commissions?: { from: string; to: string; amount: number };
  syncedAt: string;
  /** Sections the broker's statement left out. */
  missing?: string[];
  /** What it held at the end of each month it was brought in during, one entry a month, oldest first. */
  snapshots?: HoldingsSnapshot[];
};
/** What an investment account held at a month's end: its kinds (keys as `kindGroup` names them), cash and accrued income. */
export type HoldingsSnapshot = {
  month: string;
  asOf: string;
  total: number;
  byKind: Record<string, number>;
  cash: number;
  other: number;
  cost: number | null;
  unrealised: number | null;
};
/** A day and an amount on it. */
type DayAmount = { date: string; amount: number };
/** A holding named for a fact about it, with the share or return that makes it one. */
type NamedRate = { name: string; rate: number };
/**
 * Everything an investment account's page shows, as the engine works it out
 * (investment_stats.rs `page`). Amounts are cents in the budget's currency; rates and shares are
 * fractions; a figure that can't be known is null.
 */
export type HoldingsPage = {
  accountId: string;
  source: string;
  sourceAccount: string;
  asOf: string;
  total: number;
  /**
   * Every valued day, the value on it, and the gain since the first of them: a period's running
   * gain is this from its `first` day on, less the figure on that day.
   */
  days: string[];
  values: number[];
  gains: number[];
  /** "1m", "3m", "ytd", "1y" and "all", in that order; empty before the account was ever valued. */
  periods: {
    period: string;
    /** Where the period starts among `days`. */
    first: number;
    from: string;
    to: string;
    start: number;
    end: number;
    /** Money put in less money taken out after `from`, and what the value changed by beyond it. */
    put: number;
    gain: number;
    /** The gain as a share of the money at work (Modified Dietz); null with nothing at work. */
    rate: number | null;
    /** Its ups and downs, when it has more than two valued days. */
    swings: {
      best: DayAmount | null;
      worst: DayAmount | null;
      /** The days it ended up, among the days it moved at all. */
      up: number;
      moved: number;
      /** The furthest the gain fell back from a high, and that as a share of the high. */
      drop: { amount: number; from: string; to: string; rate: number | null } | null;
    } | null;
  }[];
  /** The last valued day's change beyond money put in that day, and the day it's measured from. */
  dayChange: { amount: number; since: string } | null;
  /** The last twelve months the account was valued in, each with its gain. */
  monthly: { month: string; gain: number }[];
  totals: {
    /** What the positions cost and have gained, over the ones the broker gives a cost for. */
    cost: number | null;
    unrealised: number | null;
    unrealisedRate: number | null;
    cash: number;
    /** By kind (the engine's group names, "Cash", "Accrued") and by currency, largest first. */
    byKind: PortfolioPart[];
    byCurrency: PortfolioPart[];
  };
  holdings: {
    symbol: string;
    /** Its name, or its symbol when the source gives none. */
    name: string;
    /** What a group of its kind is called, and one of them ("Stocks", "Stock"). */
    group: string;
    one: string;
    /** What it is, more exactly ("ETF", "Call option"): an English key; left out by older engines. */
    instrument?: string;
    currency: string;
    /** What it is worth in its own currency, as the source wrote it; null when unknown. */
    amount?: string | null;
    /** As the broker wrote them: shown, never calculated with. */
    quantity: string;
    price: string;
    value: number;
    gain: number | null;
    /** Its gain as a share of what it cost, and its value as a share of the account. */
    rate: number | null;
    share: number | null;
  }[];
  cash: { currency: string; amount: string; value: number; share: number | null }[];
  /** Dividends and interest earned but not yet paid, when enough to show; else 0. */
  accrued: number;
  /** The largest single line among holdings, cash and accrued: what the share bars are drawn against. */
  largest: number;
  biggest: NamedRate | null;
  /** The holdings that gained most and least against their cost, when two or more have one. */
  best: NamedRate | null;
  worst: NamedRate | null;
  /** What it earned and was charged over the twelve months to its prices' month, by kind. */
  earned: {
    from: string;
    to: string;
    dividends: number;
    interest: number;
    tax: number;
    fees: number;
    other: number;
    commissions: { from: string; to: string; amount: number } | null;
  } | null;
  /** The days counted as money moved on an estimate, with what was counted. */
  estimated: DayAmount[];
  /** Sections the broker's statement left out, by the name the broker shows. */
  missing: string[];
};
/**
 * A named part of a whole, and its share of the positive parts' total (0.31 is 31%). A currency's
 * part also has what it comes to in that currency, as a decimal; null when some of it isn't known.
 */
export type PortfolioPart = { key: string; value: number; share: number; amount?: string | null };
/** Cash in one currency: as the source wrote it in that currency, and its value in the budget's. */
export type CashAmount = { currency: string; amount: string; value: number };
/**
 * What an account's balance was made of at a month's end: its holdings by kind, cash, accrued
 * income, and `rest`, what the balance has moved since those prices. They add up to the balance.
 */
export type PortfolioParts = {
  month: string;
  asOf: string;
  byKind: { key: string; value: number }[];
  cash: number;
  accrued: number;
  rest: number;
};
/** One account in the investments report. */
export type PortfolioAccount = {
  accountId: string;
  source: string;
  sourceAccount: string;
  asOf: string;
  /** Its first valued day: its value then counts as money put in. */
  joined: string;
  start: number;
  end: number;
  put: number;
  gain: number;
  rate: number | null;
  /** Days whose jump in value was counted as money moved, with nothing listed to explain it. */
  /** Gains realised on its trades in the range; null where its trades aren't known. */
  realised: number | null;
  estimatedDays: number;
  missing: string[];
  /** Valued only when it is updated, so a gain lands in the month of the next update. */
  sparse: boolean;
  /** The balance's parts just before the range and at its end; null where what it held then isn't kept. */
  startParts: PortfolioParts | null;
  endParts: PortfolioParts | null;
  /** Today's cash by currency; left out by older engines. */
  cash?: CashAmount[];
};
/** A holding as all the accounts hold it together. */
export type PortfolioHolding = {
  key: string;
  symbol: string;
  name: string;
  /** Its kind's heading ("Stocks") and what one of it is ("Stock"). */
  group: string;
  one: string;
  /** What it is, more exactly ("ETF", "Call option"): an English key; left out by older engines. */
  instrument?: string;
  currency: string;
  /** What it is worth in its own currency, as a decimal; null when not every account holding it says. */
  amount?: string | null;
  value: number;
  cost: number | null;
  gain: number | null;
  rate: number | null;
  share: number;
  accounts: string[];
};
/**
 * The investment accounts together (investment_stats.rs `portfolio`): how they did over a range
 * of months, what they hold today, and what they earned and were charged. Amounts are cents in
 * the budget's currency; rates and shares are fractions; a figure that can't be known is null.
 */
export type Portfolio = {
  from: string;
  to: string;
  total: { start: number; end: number; put: number; gain: number; rate: number | null; yearly: number | null; days: number; realised: number | null };
  /** One row per month of the range; `byAccount` lists an account from the month it joins. */
  months: { month: string; value: number; gain: number; put: number; realised: number | null; byAccount: Partial<Record<string, number>> }[];
  accounts: PortfolioAccount[];
  /** Today's positions across the accounts, whatever the range. */
  allocation: {
    asOf: string;
    total: number;
    count: number;
    cash: number;
    accrued: number;
    cost: number | null;
    unrealised: number | null;
    unrealisedRate: number | null;
    largestShare: number | null;
    topFiveShare: number | null;
    byKind: PortfolioPart[];
    byAccount: PortfolioPart[];
    byCurrency: PortfolioPart[];
  };
  /** Merged across accounts, largest first. */
  holdings: PortfolioHolding[];
  income: {
    /** `commissions` are the month's commissions on trades, null where they aren't known by month. */
    months: { month: string; dividends: number; interest: number; tax: number; fees: number; other: number; commissions: number | null }[];
    /** Dividends in the range by holding, largest first; `value` is today's where it is still held. */
    byHolding: { key: string; symbol: string; name: string; dividends: number; tax: number; value: number | null; yield: number | null }[];
    /**
     * The same over the range by account: one row for each of `reporting`, in its order.
     * `commissions` are its trades' in the range: by month where known, else its statement's
     * total, else null.
     */
    byAccount: { accountId: string; dividends: number; interest: number; tax: number; fees: number; other: number; commissions: number | null }[];
    dividends: number;
    interest: number;
    tax: number;
    fees: number;
    other: number;
    /** Dividends and interest over the average value of the accounts that report income. */
    yield: number | null;
    /** Commissions over a statement's period, for the accounts whose commissions aren't known by month. */
    commissions: { accountId: string; from: string; to: string; amount: number }[];
    /** The accounts with any income rows, and the rest. */
    reporting: string[];
    silent: string[];
  };
  /** Open investment or retirement accounts that nothing fills. */
  unfilled: string[];
};
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
  /** What brokers report each investment account holds (investments.rs), one entry per account. */
  investments?: Holdings[];
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
  /** Short notes the assistant keeps for this budget and is sent with every message; absent when there are none. */
  assistantNotes?: string[];
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
