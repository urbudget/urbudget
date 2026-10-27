import { invoke, isTauri } from "@tauri-apps/api/core";
import { apply, diff, type Patch } from "./patch";
import { getPref } from "./prefs";
import type {
  BankColumns,
  BankFileSettings,
  Budget,
  DatabaseStatus,
  Snapshot,
  Summary,
  ImportResult,
  LoanTerms,
  LoanProjection,
  Transaction,
  SavedYnabToken,
  YnabImportRequest,
  YnabPlan,
} from "./types";
export const desktop = isTauri();

// Rust holds a draft of the budget. `synced` is the budget object that draft
// matches; calls send the difference between their budget and `synced`, so an
// edit transfers only the records it touched. Every call goes through one queue,
// keeping patches, saves and reads in order. When the draft's state is unknown
// (a reset failed), the next call sends the complete budget once.
let synced: Budget | null = null;
let tail: Promise<unknown> = Promise.resolve();
function queue<T>(job: () => Promise<T>): Promise<T> {
  const run = tail.then(job, job);
  tail = run.catch(() => {});
  return run;
}
async function relative(budget: Budget): Promise<Patch | null> {
  if (!synced) {
    await invoke("replace_draft", { budget });
    synced = budget;
  }
  return diff(synced, budget);
}
/** Obsolete queued reads never reach Rust. An already-running command still completes before
 * the queue advances, preserving the order of draft changes and saves. */
export const read = <T>(budget: Budget, command: string, args: Record<string, unknown> = {}, signal?: AbortSignal) =>
  queue(async () => {
    signal?.throwIfAborted();
    const patch = await relative(budget);
    signal?.throwIfAborted();
    const result = await invoke<T>(command, { ...args, patch });
    signal?.throwIfAborted();
    return result;
  });
/** Runs a command that returns a patch, and applies it to `budget`. For feature modules. */
export const change = (budget: Budget, command: string, args: Record<string, unknown>) =>
  queue(async () => {
    const patch = await invoke<Patch>(command, {
      ...args,
      patch: await relative(budget),
    });
    return apply(budget, patch);
  });
/** Runs a command that replaces the draft with the budget it returns. */
export const reset = <T>(command: string, args: Record<string, unknown>, budget: (r: T) => Budget | null) =>
  queue(async () => {
    try {
      const result = await invoke<T>(command, args);
      synced = budget(result) ?? synced;
      return result;
    } catch (e) {
      synced = null;
      throw e;
    }
  });

export type Applied = { recoveryError?: string | null };
export type CalculationOptions = { asOfMonth?: string; signal?: AbortSignal };
export const api = {
  load: () => reset<Snapshot>("load_budget", { legacyBackupKeep: getPref("keepBackups") }, (s) => s.budget),
  /** Makes `budget` the draft. Rust writes the unsaved changes to its recovery file. */
  push: (budget: Budget) =>
    queue(async (): Promise<Applied> => {
      if (synced && budget === synced) return {};
      try {
        const patch = await relative(budget);
        const result = patch ? await invoke<Applied>("apply_patch", { patch }) : {};
        synced = budget;
        return result;
      } catch {
        // Fall back to sending everything; Rust's draft becomes exactly `budget`.
        const result = await invoke<Applied>("replace_draft", { budget });
        synced = budget;
        return result;
      }
    }),
  /** Saves the draft, writing only rows that changed since the last save. */
  commit: (expectedToken: string) =>
    queue(() => invoke<DatabaseStatus>("commit_budget", { expectedToken })),
  status: () => invoke<DatabaseStatus>("database_status"),
  calculate: (budget: Budget, month: string, options: CalculationOptions = {}) =>
    read<Summary>(budget, "calculate_budget", { month, asOfMonth: options.asOfMonth }, options.signal),
  validate: (budget: Budget) => read<void>(budget, "validate_budget"),
  recoveryRead: () =>
    invoke<{ budget: Budget; matches: boolean } | null>("read_recovery"),
  discardRecovery: () => invoke<void>("discard_recovery"),
  runSchedules: (expectedToken: string, through: string) =>
    reset<Snapshot>("run_schedules", { expectedToken, through }, (s) => s.budget),
  syncYnab: ({ token, planId, replace, remember, savedToken, tokenLabel }: YnabImportRequest, expectedToken: string) =>
    reset<ImportResult>(
      "sync_ynab",
      {
        token,
        planId,
        expectedToken,
        replace,
        rememberToken: !!remember,
        tokenLabel: tokenLabel ?? null,
        savedToken: savedToken ?? null,
      },
      // A review leaves the saved budget, which the window already shows.
      (r) => (r.reviewId ? null : r.snapshot.budget),
    ),
  /** Saves an import whose totals differ from YNAB's, once the person has seen them. */
  acceptYnab: (reviewId: string, expectedToken: string) =>
    reset<ImportResult>("accept_ynab_import", { reviewId, expectedToken }, (r) => (r.reviewId ? null : r.snapshot.budget)),
  resolveYnab: (
    reviewId: string,
    choices: Record<string, string>,
    expectedToken: string,
  ) =>
    reset<ImportResult>(
      "resolve_ynab",
      { reviewId, choices, expectedToken },
      (r) => r.snapshot.budget,
    ),
  /** The plans a YNAB token can read, most recently edited first. */
  ynabPlans: (token: string, savedToken?: string) =>
    invoke<YnabPlan[]>("ynab_plans", { token, savedToken: savedToken ?? null }),
  /** YNAB tokens kept on this device, most recently used first. */
  ynabTokens: () => invoke<SavedYnabToken[]>("ynab_tokens"),
  forgetToken: (id: string) => invoke<void>("forget_ynab_token", { id }),
  listBudgets: () => invoke<BudgetListing[]>("list_budgets"),
  /** Saves a new budget file; the open budget is unchanged. Returns its id. */
  createBudget: (budget: Budget, source?: { budget: Budget; token: string }) => source
    ? read<string>(source.budget, "create_budget", { budget, freshStartToken: source.token })
    : invoke<string>("create_budget", { budget }),
  /** Switches the window to another budget (save the open one first). */
  openBudget: (id: string) => reset<Snapshot>("open_budget", { id }, (s) => s.budget),
  deleteBudget: (id: string) => invoke<BudgetListing[]>("delete_budget", { id }),
  /** Shows a file from the app's data folder, or an export it just saved, in the system file manager. */
  reveal: (path: string) => invoke<void>("reveal_path", { path }),
  importYnab: (exportData: unknown) =>
    invoke<Budget>("import_ynab_export", { exportData }),
  /**
   * A new, unsaved budget from YNAB's own export (its .zip, or the Register and Plan CSVs).
   * `dateOrder` is "dmy", "mdy" or "ymd", or null to detect it; `currency` applies when the
   * amounts carry no symbol.
   */
  importYnabFiles: (files: { name: string; bytes: number[] }[], dateOrder: string | null, currency: string) =>
    invoke<Budget>("import_ynab_files", { files, dateOrder, currency }),
  backup: () => invoke<string>("backup_database"),
  backupRetention: () => invoke<number>("backup_retention"),
  setBackupRetention: (keep: number) => invoke<void>("set_backup_retention", { keep }),
  restoreBackup: (budget: Budget, expectedToken: string) =>
    reset<Snapshot>("restore_budget", { budget, expectedToken }, (s) => s.budget),
  /** Deletes all but the newest `keep` backups. */
  pruneBackups: (keep: number) => invoke<number>("prune_backups", { keep }),
  // Exports open a save panel in Downloads and return where the file went, or null when cancelled.
  export: (budget: Budget) => read<string | null>(budget, "export_budget"),
  /** Preserve an invalid unsaved draft for repair; ordinary backup exports still validate. */
  exportRecoveryDraft: (budget: Budget) => read<string | null>(budget, "export_budget", { recovery: true }),
  /** A folder with YNAB's Register and Plan CSVs. */
  exportYnab: (budget: Budget) => read<string | null>(budget, "export_ynab_csv"),
  /** One account's transactions (or all), optionally just the listed ones, as a register CSV. */
  exportTransactions: (budget: Budget, accountId: string | null, ids: string[] | null) =>
    read<string | null>(budget, "export_transactions", { accountId, ids }),
  /** Saves a CSV the app made (a template), suggesting `name`. */
  writeExport: (name: string, contents: string) => invoke<string | null>("write_export", { name, contents }),
  upsertTransaction: (budget: Budget, transaction: Transaction) =>
    change(budget, "upsert_transaction", { transaction }),
  sourceLoanTerms: (budget: Budget, accountId: string) =>
    read<LoanTerms>(budget, "source_loan_terms", { accountId }),
  previewBank: (
    budget: Budget,
    account: string,
    content: string,
    format: string,
    dateFormat: string,
    /** For a CSV whose one Amount column writes purchases as positive numbers. */
    flipSigns: boolean,
    /** A CSV's columns as chosen; without them, the account's saved choice or the ones found by name. */
    columns: BankColumns | null = null,
  ) =>
    // Rows that could be the same transactions share one list of them (an account with thousands
    // of the same charge would otherwise send thousands of lists), and each row reads its own.
    read<CompactReview>(budget, "preview_bank_file", {
      account,
      content,
      format,
      dateFormat,
      flipSigns,
      columns,
    }).then(expand),
  /**
   * A statement that isn't UTF-8, as ISO-8859-1 (a character for each byte): its text in the encoding the
   * engine finds, or a workbook's sheet (the one named, else the one most like a statement) and its sheets.
   */
  decodeStatement: (latin1: string, sheet: string | null = null) =>
    invoke<{ text: string; encoding: string; sheet?: string; sheets?: string[] }>("decode_statement", { latin1, sheet }),
  /**
   * An import of a file into an account, read as the account's last file was (bank_statement.rs);
   * one the assistant `prepared` reads the standard way.
   */
  bankSession: (budget: Budget, account: string, content: string, session: string, budgetId: string, sheet: string | null, prepared = false) =>
    read<BankSession>(budget, "bank_session", { account, content, session, budget: budgetId, sheet, prepared }),
  /** The file as lines, each with what became of it. */
  bankStatement: (budget: Budget, account: string, content: string, session: BankSession, signal?: AbortSignal) =>
    read<Statement>(budget, "bank_statement", { account, content, session }, signal),
  /** The statement and the review of the lines that read; `lines[i]` is the line `rows[i]` is. */
  bankStatementPreview: (budget: Budget, account: string, content: string, session: BankSession) =>
    read<{ statement: Statement; review: CompactReview; lines: string[] }>(budget, "bank_statement_preview", { account, content, session }).then(
      ({ statement, review, lines }) => ({ statement, lines, rows: expand(review) }),
    ),
  /** Changes to an import where they still fit it, and what became of each. */
  bankApplyFixes: (budget: Budget, account: string, content: string, session: BankSession, fixes: BankFixes) =>
    read<{ session: BankSession; results: FixResult[] }>(budget, "bank_apply_fixes", { account, content, session, fixes }),
  /** A CSV's header row, first rows and the columns an import would read, for choosing them. */
  bankFileColumns: (budget: Budget, account: string, content: string, columns: BankColumns | null = null) =>
    read<BankFileLayout>(budget, "bank_file_columns", { account, content, columns }),
  importBank: (
    budget: Budget,
    account: string,
    choices: { row: BankRow; action: string; matchId: string | null }[],
    /** How the file read, for the account to keep for its next one. */
    remember: BankFileSettings | null = null,
  ) =>
    change(budget, "import_bank_file", {
      account,
      // The engine checks a match against the budget again; of the rows' candidates it needs only
      // whether the one chosen was an earlier bank row's.
      choices: choices.map(({ row, action, matchId }) => ({
        row: {
          ...row,
          candidates: [],
          candidateDetails: undefined,
          transferCandidates: undefined,
          previouslyMatchedCandidates: matchId && row.previouslyMatchedCandidates?.includes(matchId) ? [matchId] : undefined,
        },
        action,
        matchId,
      })),
      remember,
    }),
  action: (budget: Budget, action: Record<string, unknown>) =>
    change(budget, "budget_action", { action }),
  /**
   * What every quick-assign option would change for these categories, in one call: per option,
   * the categories whose Assigned changes, or null for an option that can't run.
   */
  autoAssignPreview: (budget: Budget, month: string, categories: string[], limitToAvailable: boolean, options: CalculationOptions = {}) =>
    read<Record<string, { id: string; before: number; after: number }[] | null>>(budget, "auto_assign_preview", {
      month,
      categories,
      limitToAvailable,
      asOfMonth: options.asOfMonth,
    }, options.signal),
  /** The actual summary after applying one option to an ephemeral native snapshot. */
  autoAssignResult: (budget: Budget, month: string, mode: string, categories: string[], limitToAvailable: boolean, options: CalculationOptions = {}) =>
    read<Summary>(budget, "preview_auto_assign_result", { month, mode, categories, limitToAvailable, asOfMonth: options.asOfMonth }, options.signal),
  reports: (budget: Budget, filter: ReportFilter, signal?: AbortSignal) =>
    read<Report>(budget, "calculate_reports", { filter }, signal),
  exportReports: (budget: Budget, filter: ReportFilter) =>
    read<string | null>(budget, "export_reports", { filter }),
  previewSchedules: (
    budget: Budget,
    through: string,
    selected: string | null,
    skip: boolean,
  ) => change(budget, "preview_schedules", { through, selected, skip }),
  /** `lump` is a one-off paid with the first payment, after that month's interest and fees. */
  projectLoan: (
    balance: number,
    terms: LoanTerms,
    start: string,
    extra: number,
    lump = 0,
  ) => invoke<LoanProjection>("project_loan", { balance, terms, start, extra, lump }),
  loanPaymentByDate: (
    balance: number,
    terms: LoanTerms,
    start: string,
    due: string,
    lump = 0,
  ) => invoke<number>("loan_payment_by_date", { balance, terms, start, due, lump }),
  recordLoanPayment: (
    budget: Budget,
    terms: LoanTerms,
    from: string,
    amount: number,
    date: string,
    id: string,
  ) =>
    change(budget, "record_loan_payment", { terms, from, amount, date, id }),
  reconcile: (
    budget: Budget,
    accountId: string,
    balance: number,
    date: string,
    adjustmentId: string,
  ) =>
    change(budget, "reconcile_budget", {
      accountId,
      balance,
      date,
      adjustmentId,
    }),
};

export type BudgetListing = {
  id: string;
  name: string;
  currency: string;
  current: boolean;
  lastOpened: string | null;
  path: string;
  missing: boolean;
};

export type ReportFilter = {
  from: string;
  to: string;
  accountIds: string[];
  categoryIds: string[];
};
export type Report = {
  months: {
    month: string;
    income: number;
    spending: number;
    netIncome: number;
    assets: number;
    debt: number;
    netWorth: number;
    categories: Record<string, number>;
    incomeSources: Record<string, number>;
    accounts: Record<string, number>;
  }[];
  categories: Record<string, number>;
  incomeSources: Record<string, number>;
  income: number;
  spending: number;
  ageOfMoney: number | null;
};

export type BankRow = {
  id: string;
  date: string;
  payee: string;
  memo: string;
  amount: number;
  duplicate: boolean;
  candidates: string[];
  previouslyMatchedCandidates?: string[];
  candidateDetails?: { id: string; transactionId: string; splitIndex: number }[];
  /** Candidates in another account that matching makes into a transfer with this one. */
  transferCandidates?: string[];
  /** The bank text backs the transfer up, so it's chosen; otherwise it's only suggested. */
  transferSure?: boolean;
  idless?: boolean;
  possibleDuplicate?: boolean;
  groupId?: string;
  /** A stable ID for a deliberately separate, otherwise identical ID-less transaction. */
  importId?: string;
  /** The payee a rule gives the row, when it differs from the bank's. */
  rename?: string;
  /** The category the payee rules give it. */
  categoryId?: string;
  /** When the bank says it happened (bank sync), which becomes its createdAt; a file has none. */
  at?: string;
  /** More IDs it's known by: a line that read only once fixed, by the line as the file wrote it. */
  aliases?: string[];
  /** The batch it belongs to, by the ID of its entry as one transaction. */
  batch?: string;
  /** The purchase it may be a refund of; imported, it takes that purchase's category when it has none. */
  refundOf?: RefundOf;
};
export type RefundOf = {
  id: string;
  date: string;
  payee: string;
  /** Cents, negative. */
  amount: number;
  /** The purchases' category, when they agree on one. */
  categoryId?: string;
  /** How many purchases it may be a refund of. */
  purchases: number;
};
/** Rows read from a compact review, each with its own list of possible matches. */
const expand = ({ items, candidateSets }: CompactReview): BankRow[] =>
  items.map(({ candidateSet, ...row }) => ({ ...row, ...candidateSets[candidateSet] }));
/** A preview with each list of possible matches sent once, for every row it's for (bank_import.rs). */
type CompactReview = {
  items: (BankRow & { candidateSet: number })[];
  candidateSets: Pick<BankRow, "candidates" | "previouslyMatchedCandidates" | "candidateDetails" | "transferCandidates" | "transferSure">[];
};
/** A part of a transaction a line gives. */
export type BankField = "date" | "payee" | "memo" | "amount";
/** Why a line isn't imported unless someone includes it (bank_statement.rs). */
export type LeftOutReason =
  | "pending"
  | "declined"
  | "otherAccount"
  | "otherCurrency"
  | "ownBalances"
  | "balance"
  | "aboveHeader"
  | "noDate"
  | "noAmount"
  | "zero"
  | "corrected"
  | "repeatedHeader"
  | "inWholeBatch"
  | "imported";
/** What became of a line. */
export type LineStatus =
  | { kind: "ready" }
  | { kind: "attention"; field: BankField; column?: number | null; reason: string }
  | { kind: "leftOut"; reason: LeftOutReason; detail: string };
export type BankScope = { session: string; budget: string; account: string; file: string };
/** Everything that changes how a file's cells read. */
export type BankStructure = {
  format: string;
  sheet?: string | null;
  source?: string | null;
  headerLine?: number | null;
  columns?: BankColumns | null;
  dateOrder: { order: string; confirmed: boolean };
  flipSigns: boolean;
  signsByType?: boolean | null;
  wholeBatches?: string[];
};
/** An import in progress (bank_statement.rs): pass it back as it came. */
export type BankSession = {
  scope: BankScope;
  revision: number;
  structure: BankStructure;
  edits: { cells: Record<string, Record<string, string>>; fields: Record<string, Record<string, string>>; include: string[] };
  versions: Record<string, number>;
};
export type StatementLine = {
  /** Where it starts in the file (a CSV), or which record it is (`t1`, `e1.2`). */
  id: string;
  version: number;
  /** Its cells with any fixes: a CSV's, or a statement's fields. */
  cells: string[];
  /** Its cells as the file writes them, when some were fixed. */
  original?: string[];
  fields: Record<BankField, string>;
  status: LineStatus;
  included?: boolean;
  fixed?: boolean;
  batch?: number;
};
export type BatchInfo = { entry: string; line?: number; payments: number[]; total?: number; addsUp?: boolean; asOne: boolean };
export type Statement = {
  scope: BankScope;
  revision: number;
  structure: BankStructure;
  format: string;
  sources?: { label: string; currency?: string; lines: number }[];
  source?: number;
  headers: string[];
  columns?: BankColumns;
  headerLine?: number;
  /** The columns are the account's saved choice. */
  saved?: boolean;
  order: string;
  lines: StatementLine[];
  counts: { ready: number; attention: number; leftOut: Partial<Record<LeftOutReason, number>> };
  dateOrder?: { order: string; example: string; unconfirmed: boolean };
  signs?: { byType: boolean; unconfirmed: boolean };
  currency?: string;
  batches?: BatchInfo[];
  /** What the account keeps for its next file, once this one is imported. */
  remember: BankFileSettings;
};
/** Changes to an import, made against one revision of it. */
export type BankFixes = {
  scope: BankScope;
  baseRevision: number;
  baseStructure: BankStructure;
  structure?: BankStructure | null;
  cells?: { line: string; lineVersion: number; column: number; from: string; to: string }[];
  fields?: { line: string; lineVersion: number; field: BankField; from: string; to: string }[];
  include?: { line: string; lineVersion: number; include: boolean }[];
  revert?: { line: string; lineVersion: number }[];
};
export type FixResult = { line?: string; fix: string; applied: boolean; problem?: string };
/** A CSV's columns, for choosing which holds what (bank_csv.rs). */
export type BankFileLayout = {
  /** "csv", or the format the file turned out to be in (an MT940 statement saved as .txt), with no columns. */
  format: string;
  headers: string[];
  /** The first rows under the header. */
  samples: string[][];
  /** What an import reads now: the account's saved choice when the file has those columns, or else the ones found by name. */
  columns: BankColumns;
  /** The columns are the account's saved choice. */
  saved: boolean;
  /** The account's date order, or else the only one every date in the file fits. */
  dateFormat?: string | null;
  flipSigns: boolean;
};
