import {
  useCallback,
  useDeferredValue,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { ArrowDown, ArrowLeftRight, ArrowUp, CalendarClock, CheckCheck, ChevronDown, ChevronRight, CircleCheck, CornerDownLeft, CreditCard, Info, Pause, Pencil, Play, SkipForward, Trash2 } from "lucide-react";
import { transferView, type TransferView } from "./transfers";
import { api } from "./bridge";
import { errorText, formatDate, money, tone } from "./format";
import { Menu, usePinned, useToast, type MenuItem } from "./ui";
import { useLearnPayee } from "./useLearnPayee";
import { useCategoryCreator } from "./NewCategory";
import { textWidth, useColumns } from "./ui/columns";
import { compile, type Term } from "./search";
import { byAdded } from "./order";
import { getPref, usePref } from "./prefs";
import { frequencies } from "./Planning";
import { LoanHeader } from "./LoanHeader";
import { reportTransactionIds, type LineQuery } from "./reports/TransactionsDialog";
import { BulkBar, RegisterToolbar, anyFilter, emptyFilters, type Filters } from "./RegisterToolbar";
import {
  ClearedMark,
  FlagIcon,
  FlagTag,
  TransactionEditor,
  flagName,
  flagsOf,
  inlineEditable,
  type EditorField,
} from "./TransactionRow";
import { FlagNamesDialog, setFlagNames } from "./FlagNames";
import { ScheduleDialog } from "./forms";
import { SchedulesModal } from "./SchedulesModal";
import { scheduleId as resolveScheduleId } from "./scheduleId";
import { NEEDS_CATEGORY, uncategorizedTest } from "./uncategorized";
import {
  scheduledAhead,
  today,
  type Account,
  type Budget,
  type Schedule,
  type Split,
  type Status,
  type Summary,
  type Transaction,
} from "./types";
import "./register.css";
/** What the register opens on when another page sends you there (dates are ISO days). */
export type RegisterFilter = {
  reportQuery?: LineQuery;
  accountIds?: string[];
  category?: string;
  payee?: string;
  from?: string;
  to?: string;
  /** A transaction to select and scroll to: a transfer opened from its other account. */
  focus?: string;
  /** Only spending that needs a category (the budget page's count). */
  uncategorized?: boolean;
};
type Edit = (fn: (b: Budget) => void) => void;
type Transform = (fn: (b: Budget) => Promise<Budget>) => Promise<void>;
const ROW = 34;
/** Height of the sticky column header. */
const HEAD = 36;
/**
 * The edit row's height: 4px of padding, a 40px line for its fields plus one per split line, a
 * 48px footer, and the row's 1px border (see .reg-editor in register.css).
 */
const editHeight = (lines: number) => 4 + 40 * (1 + lines) + 48 + 1;
const OVERSCAN = 12;

export function registerEntries(b: Budget, account: string) {
  const entries = b.transactions
    .flatMap((t) => {
      const rows: {
        key: string;
        t: Transaction;
        amount: number;
        status: Status;
        reverse: boolean;
        part?: number;
      }[] = [];
      if (account === "all" || account === t.accountId)
        rows.push({
          key: t.id,
          t,
          amount: t.amount,
          status: t.status,
          reverse: false,
        });
      else if (t.transferAccountId === account)
        rows.push({
          key: t.id,
          t,
          amount: -t.amount,
          status: t.transferStatus ?? "uncleared",
          reverse: true,
        });
      else
        t.splits.forEach((s, i) => {
          if (s.transferAccountId === account)
            rows.push({
              key: `${t.id}:part:${i}`,
              t,
              amount: -s.amount,
              status: s.transferStatus ?? "uncleared",
              reverse: true,
              part: i,
            });
        });
      return rows;
    })
    // Oldest first, in the order they were added (order.ts). Ties keep the budget's order, and
    // one split's parts their order in it. The running balance adds up in this order; the
    // register then shows it newest first.
    .sort((a, b) => byAdded(a.t, b.t));
  let balance = b.accounts.find((a) => a.id === account)?.openingBalance ?? 0;
  return entries
    .map((r) => ({ ...r, balance: (balance += r.amount) }))
    .reverse();
}

/**
 * Saves a transaction. Scheduled transactions are one system with the rest: one dated ahead
 * waits in Scheduled (once, or repeating with `repeat`) and is entered on its date, so an
 * existing transaction moved ahead leaves the register until then. Otherwise it's entered, and
 * with `repeat` it's also scheduled from there.
 */
export async function saveTransaction(b: Budget, t: Transaction, repeat?: string) {
  const ahead = scheduledAhead(t);
  if (!repeat && !ahead) return api.upsertTransaction(b, t);
  // A draft keeps its transaction ID across retries. Schedules have a separate ID
  // namespace, so reuse that ID instead of creating a second recurrence after an
  // ambiguous save failure. Preserve an occurrence already advanced by that save.
  // A transaction moved ahead may share its ID with the schedule it was entered with
  // (Repeat), which it must leave alone.
  const moving = ahead && b.transactions.some((x) => x.id === t.id);
  const originalId = await resolveScheduleId(b, t.id);
  const movedId = ahead ? await resolveScheduleId(b, t.id, `${t.id}:ahead`) : "";
  const movedBefore = b.schedules?.some((s) => s.id === movedId && s.transaction.id === t.id);
  const scheduleId = ahead && (movedBefore || (moving && b.schedules?.some((s) => s.id === originalId))) ? movedId : originalId;
  const previous = b.schedules?.find((s) => s.id === scheduleId);
  const frequency = repeat ?? "never";
  const advanced = previous?.anchorDate === t.date && previous.frequency === frequency && previous.nextDate !== t.date;
  const schedule: Schedule = {
    ...previous,
    id: scheduleId,
    transaction: { ...t },
    nextDate: advanced ? previous.nextDate : t.date,
    anchorDate: t.date,
    paused: previous?.paused ?? false,
    frequency,
  };
  const scheduled = {
    ...b,
    transactions: moving ? b.transactions.filter((x) => x.id !== t.id) : b.transactions,
    schedules: previous ? b.schedules!.map((s) => s.id === schedule.id ? schedule : s) : [...(b.schedules ?? []), schedule],
  };
  if (ahead) return scheduled;
  const entered = await api.upsertTransaction(scheduled, t);
  if (advanced) return entered;
  // This occurrence was just entered; the schedule continues from the next one.
  return api.previewSchedules(entered, t.date, schedule.id, true);
}
/**
 * Saves a schedule (ScheduleDialog). One that's due, dated today or earlier, is entered at
 * once, as the app would within the minute; a one-time one is then done.
 */
export async function saveSchedule(b: Budget, s: Schedule) {
  const saved = {
    ...b,
    schedules: b.schedules?.some((x) => x.id === s.id)
      ? b.schedules.map((x) => (x.id === s.id ? s : x))
      : [...(b.schedules ?? []), s],
  };
  if (s.paused || !s.nextDate || s.nextDate > today()) return saved;
  return api.previewSchedules(saved, today(), null, false);
}
/** A scheduled transaction's toast: when it's entered, and how often (`repeat`). */
const scheduledFor = (date: string, repeat?: string) => {
  const often = repeat && (frequencies[repeat] ?? repeat).toLowerCase();
  return often ? `Scheduled ${often} from ${formatDate(date)}.` : `Scheduled for ${formatDate(date)}. It's entered on that day.`;
};
/** What saving `t` scheduled, for its toast; empty when it was simply saved. */
export function scheduledMessage(t: Transaction, repeat?: string) {
  if (scheduledAhead(t)) return scheduledFor(t.date, repeat);
  return repeat ? `Saved. It repeats ${(frequencies[repeat] ?? repeat).toLowerCase()}.` : "";
}

type SortKey = "date" | "payee" | "account" | "category" | "flag" | "memo" | "outflow" | "inflow";
type ColumnKey = "select" | "date" | "payee" | "account" | "category" | "flag" | "memo" | "outflow" | "inflow" | "balance" | "cleared";
/**
 * The register's columns size themselves three ways (see `track` in Register):
 * - Icon columns (Flag, Cleared) hold an icon, and show names once dragged wider (.icon-col in
 *   register.css).
 * - Date and the amounts are as wide as the widest thing they show (`fitted`).
 * - Text columns share whatever is left, in these proportions, down to a minimum; one dragged to
 *   a width keeps it while there's room. Memo always takes a share.
 */
const ICON_WIDTHS = { select: 36, flag: 40, cleared: 56 };
const TEXT_COLUMNS: Partial<Record<ColumnKey, { min: number; share: number }>> = {
  payee: { min: 120, share: 1.2 },
  account: { min: 90, share: 0.8 },
  category: { min: 120, share: 1.3 },
  memo: { min: 100, share: 1 },
};
type Fitted = "date" | "outflow" | "inflow" | "balance";
/** Room for a heading's sort arrow. */
const SORT_ARROW = 15;
/** A text cell's padding on each side: less when the window is tight. */
const pad = (compact: boolean) => (compact ? 6 : 10);
/** A heading's width: 11px capitals, spaced 0.04em (.reg-head). */
const headingWidth = (label: string) => textWidth(label.toUpperCase(), 11, 600) + label.length * 0.44;
const columnsFor = (all: boolean): ColumnKey[] =>
  all
    ? ["select", "flag", "date", "payee", "account", "category", "memo", "outflow", "inflow", "cleared"]
    : ["select", "flag", "date", "payee", "category", "memo", "outflow", "inflow", "balance", "cleared"];
type Sort = { key: SortKey; desc: boolean };
const collator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });
const amountKeys: SortKey[] = ["date", "outflow", "inflow"];

/** What a transfer between budget accounts has in place of a category. */
const TRANSFER_CATEGORY = "No Category (Transfer)";
/**
 * A transfer's payee cell: "To: Savings" or "From: Checking", after a button that opens the
 * transfer in the other account's register.
 */
function TransferPayee({ view, onOpen }: { view: TransferView; onOpen?: () => void }) {
  const Icon = view.card ? CreditCard : ArrowLeftRight;
  return (
    <>
      <button
        type="button"
        className="transfer-link"
        aria-label={`Show in ${view.peer}`}
        title={onOpen ? `Show in ${view.peer}` : undefined}
        disabled={!onOpen}
        onClick={(e) => {
          e.stopPropagation();
          onOpen?.();
        }}
        onDoubleClick={(e) => e.stopPropagation()}
      >
        <Icon size={13} aria-hidden />
      </button>
      <span>{view.label}</span>
    </>
  );
}
/**
 * A line with no category, by what it does: money into or out of Unassigned, seen from the
 * budget side (`budgetAmount`), or nothing at all off budget (null).
 */
const noCategory = (budgetAmount: number | null) =>
  budgetAmount === null ? "No Category (Tracking)" : budgetAmount < 0 ? "From Unassigned" : "To Unassigned";

/** Spending with no category yet, as YNAB marks it: a filled label in the attention colour. */
const NeedsCategory = () => (
  <span className="needs-pill">
    <Info size={13} aria-hidden />
    {NEEDS_CATEGORY}
  </span>
);
const categoryLabel = (t: Transaction, names: Map<string, string>, none: string) =>
  t.transferAccountId && !t.splits.length
    ? "Transfer"
    : t.splits.length > 1
      ? `Split (${t.splits.length})`
      : t.splits[0]?.transferAccountId
        ? "Transfer"
        : (names.get(t.splits[0]?.categoryId ?? "") ?? none);
/** The editor field for a clicked cell. */
const fieldOf = (target: HTMLElement) =>
  (target.closest("[data-field]")?.getAttribute("data-field") as EditorField | null) ?? undefined;
const filterRange = (filters: Filters) => {
  const r = filters.range;
  // Month-granularity bounds never reach the register, but accept them anyway.
  return { from: r.from.length === 7 ? `${r.from}-01` : r.from, to: r.to.length === 7 ? `${r.to}-31` : r.to };
};
const fromInitial = (initial: RegisterFilter | undefined, b: Budget): Filters => ({
  ...emptyFilters(),
  reportQuery: initial?.reportQuery,
  terms: [
    ...(initial?.accountIds ?? []).map((id) => ({ field: "account" as const, id: id || "__no_account", value: b.accounts.find((a) => a.id === id)?.name ?? "No accounts" })),
    ...(initial?.category
      ? [
          {
            field: "category" as const,
            id: initial.category,
            value: b.categories.find((c) => c.id === initial.category)?.name ?? "Unassigned",
          },
        ]
      : []),
    ...(initial?.payee ? [{ field: "payee" as const, value: initial.payee }] : []),
    ...(initial?.uncategorized ? [{ field: "categorization" as const, value: "needed" as const }] : []),
  ],
  range:
    initial?.from || initial?.to
      ? { preset: "custom", from: initial.from ?? "", to: initial.to ?? "" }
      : emptyFilters().range,
});

export function TransactionsPage({
  budget,
  accountId,
  account,
  summary,
  netWorth,
  onEditAccount,
  onReconcile,
  onImport,
  onPayCard,
  onSync,
  ...register
}: {
  budget: Budget;
  accountId: string;
  account?: Account;
  summary?: Summary["accounts"][number];
  netWorth: number;
  edit: Edit;
  transform: Transform;
  initialFilter?: RegisterFilter;
  available?: Map<string, number>;
  undo?: () => void;
  onEditAccount: (a: Account) => void;
  onReconcile: (a: Account) => void;
  onImport: (a: Account) => void;
  /** Pays a credit card from a budget account (CardPayment.tsx). */
  onPayCard?: (a: Account) => void;
  /** Opens the other account's register on a transfer. */
  onOpenTransfer?: (accountId: string, transactionId: string) => void;
  /** Syncs an account linked to its bank. */
  onSync: (a: Account) => void;
  onEdit: (t: Transaction) => void;
  onAddDialog: (draft?: Transaction, repeat?: string) => void;
  onEditorChange?: (open: boolean) => void;
  /** Opens the assistant with these transactions attached. */
  onAsk?: (ids: string[]) => void;
}) {
  const balance = summary?.balance ?? netWorth;
  return (
    <div className="register-page">
      <Register
        budget={budget}
        accountId={accountId}
        onImport={account && !account.archived ? () => onImport(account) : undefined}
        onSync={account?.simplefin && !account.archived ? () => onSync(account) : undefined}
        lead={
          <>
            <div className="register-summary">
              <div className="stat">
                <span>{account ? "Working balance" : "Net worth"}</span>
                <strong className={tone(balance)}>
                  {money(balance)}
                </strong>
              </div>
              {summary && (
                <>
                  <div className="stat">
                    <span>Cleared</span>
                    <strong>{money(summary.cleared)}</strong>
                  </div>
                  <div className="stat">
                    <span>Uncleared</span>
                    <strong>{money(summary.uncleared)}</strong>
                  </div>
                </>
              )}
              <div className="spacer" />
              {account && (
                <>
                  {account.kind === "credit" && !account.archived && onPayCard && (
                    <button className="primary" onClick={() => onPayCard(account)}>
                      <CreditCard size={15} /> Make payment
                    </button>
                  )}
                  <button className="secondary" onClick={() => onReconcile(account)}>
                    <CheckCheck size={15} /> Reconcile
                  </button>
                  <button
                    className="icon-button bordered"
                    title="Edit account"
                    aria-label="Edit account"
                    onClick={() => onEditAccount(account)}
                  >
                    <Pencil size={15} />
                  </button>
                </>
              )}
            </div>
            {/* A loan's figures and payoff planner (LoanHeader.tsx), or a debt's prompt to add them. */}
            {account && <LoanHeader budget={budget} account={account} balance={summary?.balance ?? account.openingBalance} />}
          </>
        }
        {...register}
      />
    </div>
  );
}

export type Upcoming = {
  s: Schedule;
  amount: number;
  date: string;
  transfer: TransferView | null;
  payee: string;
  category: string;
  /** A transfer that needs no category: `category` says what it is instead. */
  noCategory: boolean;
  memo: string;
  account: string;
};
type Item<R> =
  | { kind: "row"; key: string; r: R }
  | { kind: "split"; key: string; r: R; s: Split; part: number }
  | { kind: "upcoming"; key: string; u: Upcoming }
  /** The heading over a computer's scheduled entries, which folds them away. */
  | { kind: "schedules"; key: string }
  /** The account's balance before its first transaction, where the running balance starts. */
  | { kind: "opening"; key: string; amount: number };

export function Register({
  budget,
  accountId,
  edit,
  transform,
  onEdit,
  onAddDialog,
  onImport,
  onSync,
  onEditorChange,
  initialFilter,
  available,
  undo,
  onAsk,
  lead,
  onOpenTransfer,
}: {
  budget: Budget;
  accountId: string;
  edit: Edit;
  transform: Transform;
  onAsk?: (ids: string[]) => void;
  /** Opens the full dialog for a transaction the row editor can't handle. */
  onEdit: (t: Transaction) => void;
  /** Opens the full dialog for a new transaction, with its draft and repeat choice. */
  onAddDialog?: (draft?: Transaction, repeat?: string) => void;
  onImport?: () => void;
  onSync?: () => void;
  onEditorChange?: (open: boolean) => void;
  initialFilter?: RegisterFilter;
  available?: Map<string, number>;
  undo?: () => void;
  /** Shown above the toolbar and pinned with it (the page's balances). */
  lead?: ReactNode;
  /** Opens the other account's register on this transfer (see `RegisterFilter.focus`). */
  onOpenTransfer?: (accountId: string, transactionId: string) => void;
}) {
  const toast = useToast();
  const learn = useLearnPayee(transform, undo);
  const categories = useCategoryCreator(budget, edit);
  const [filters, setFilters] = useState(() => fromInitial(initialFilter, budget));
  const [sort, setSort] = useState<Sort>({ key: "date", desc: true });
  const [showUpcoming, setShowUpcoming] = useState(() => getPref("showUpcoming"));
  const [runningBalance] = usePref("runningBalance");
  const [flagColumn] = usePref("flagColumn");
  // The flag picker opened from a row's Flag cell, and the dialog that names the flags.
  const [flagMenu, setFlagMenu] = useState<{ key: string; x: number; y: number } | null>(null);
  // A schedule's menu, from its row's calendar button or a right-click on the row.
  const [scheduleMenu, setScheduleMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  // The Scheduled button's list, and the editor for one schedule (`schedule` left out: a new one).
  const [schedulesOpen, setSchedulesOpen] = useState(false);
  const [scheduleEditor, setScheduleEditor] = useState<{ schedule?: Schedule } | null>(null);
  const onEditSchedule = (schedule?: Schedule) => setScheduleEditor({ schedule });
  const [namingFlags, setNamingFlags] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [cursor, setCursor] = useState<string | null>(null);
  // The cursor's bar shows while the keyboard moves it; a click puts it away (the selection's
  // tint says enough for the mouse).
  const [keyed, setKeyed] = useState(false);
  const anchor = useRef<string | null>(null);
  const [editing, setEditing] = useState<{ key: string; focus?: EditorField } | null>(null);
  const [adding, setAdding] = useState(false);
  const [editLines, setEditLines] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = usePinned();
  const rowsStart = useRef<HTMLDivElement>(null);
  const [rowsTop, setRowsTop] = useState(HEAD);
  const [scrollTop, setScrollTop] = useState(0);
  const [areaWidth, setAreaWidth] = useState(0);
  const [viewport, setViewport] = useState(800);
  const all = accountId === "all";
  const rowHeight = ROW;
  const head = HEAD;
  const change = (patch: Partial<Filters>) => setFilters((f) => ({ ...f, ...patch }));

  // The filters a change of account (or of initialFilter) resets to, and a transaction to show
  // once they apply: see the focus effect below.
  const reset = useRef<Filters | null>(null);
  const focus = useRef<string | null>(null);
  useEffect(() => {
    reset.current = fromInitial(initialFilter, budget);
    focus.current = initialFilter?.focus ?? null;
    setFilters(reset.current);
    setSelected(new Set());
    setEditing(null);
    setAdding(false);
    setCursor(null);
    setExpanded(new Set());
  }, [accountId, initialFilter]);
  const query = useDeferredValue(filters.text.trim().toLowerCase());
  const { from, to } = filterRange(filters);
  useEffect(() => {
    setSelected(new Set());
    scroller.current?.scrollTo?.({ top: 0 });
    setScrollTop(0);
  }, [query, filters.terms, from, to, accountId, sort]);
  useEffect(() => {
    onEditorChange?.(adding || !!editing || !!scheduleEditor);
  }, [adding, !!editing, !!scheduleEditor, onEditorChange]);
  useEffect(() => () => onEditorChange?.(false), [onEditorChange]);
  useLayoutEffect(() => {
    const at = rowsStart.current?.offsetTop;
    if (at !== undefined && at !== rowsTop) setRowsTop(at);
  });
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const measure = () => {
      setViewport(el.clientHeight || 800);
      // The table's room: the scroll area less its padding (the margin beside the table).
      const style = getComputedStyle(el);
      setAreaWidth(Math.max(0, el.clientWidth - (parseFloat(style.paddingLeft) || 0) - (parseFloat(style.paddingRight) || 0)));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const categoryNames = useMemo(
    () => new Map(budget.categories.map((c) => [c.id, c.name])),
    [budget.categories],
  );
  // Spending with no category yet (uncategorized.ts): it says "Needs a category".
  const now = today();
  const needsCategory = useMemo(() => uncategorizedTest(budget, now), [budget.accounts, budget.categories, now]);
  const accountNames = useMemo(
    () => new Map(budget.accounts.map((a) => [a.id, a.name])),
    [budget.accounts],
  );
  const accountsById = useMemo(() => new Map(budget.accounts.map((a) => [a.id, a])), [budget.accounts]);
  const onBudget = useCallback(
    (id: string | null | undefined) => {
      const a = id ? accountsById.get(id) : undefined;
      return !!a && a.kind !== "tracking" && a.onBudget !== false;
    },
    [accountsById],
  );
  const entries = useMemo(
    () => registerEntries(budget, accountId),
    // registerEntries reads only transactions and accounts.
    [budget.transactions, budget.accounts, accountId],
  );
  // Display text and a lowercase search string, computed once per budget change.
  const sampleMoney = money(-123456789);
  const indexed = useMemo(
    () =>
      entries.map((r) => {
        const t = r.t;
        const part = r.part === undefined ? undefined : t.splits[r.part];
        const source = r.reverse ? (part?.transferSource ?? t.transferSource) : undefined;
        const memo = String(source?.memo ?? t.memo ?? "");
        const searchMemo = r.reverse
          ? String((part ? part.transferSource : t.transferSource)?.memo ?? t.memo)
          : t.memo;
        const peer = r.reverse ? t.accountId : t.transferAccountId;
        const self = r.reverse ? (part?.transferAccountId ?? t.transferAccountId) : t.accountId;
        const budgetAmount = onBudget(self) ? r.amount : onBudget(peer) ? -r.amount : null;
        const uncategorized = needsCategory(t);
        // One line with no category that needs one says so; a split says so on the line.
        const categoryName =
          uncategorized && t.splits.length < 2 ? NEEDS_CATEGORY : categoryLabel(t, categoryNames, noCategory(budgetAmount));
        const flag = r.reverse ? source?.flag_color : t.flag;
        const payee = peer ? `Transfer: ${accountNames.get(peer) ?? ""}` : t.payee || "No payee";
        const account = accountNames.get(r.reverse ? accountId : t.accountId) ?? "";
        const lines = t.splits.filter((s) => !s.transferAccountId);
        const splitMemos = t.splits.map((s) => s.memo ?? "").join(" ");
        // A transfer names the other account from this row's side ("To: Savings"); one between
        // budget accounts needs no category and says so quietly.
        const transfer = transferView(accountsById, self, peer, r.amount);
        const isTransfer = !!part || categoryName === "Transfer";
        return {
          ...r,
          memo,
          flag: typeof flag === "string" && flag ? flag : "",
          category: isTransfer ? TRANSFER_CATEGORY : categoryName,
          noCategory: isTransfer,
          uncategorized,
          transfer,
          payee,
          account,
          // Lower-case fields for search chips (see search.ts).
          approved: t.approved !== false,
          searchPayee: payee.toLowerCase(),
          searchMemo: `${searchMemo} ${splitMemos}`.toLowerCase(),
          categoryIds: new Set<string | null>(lines.map((s) => s.categoryId ?? null)),
          categories: lines.map((s) => categoryNames.get(s.categoryId ?? "") ?? "unassigned").join(" ").toLowerCase(),
          accountIds: new Set<string | null>([t.accountId, t.transferAccountId, ...t.splits.map((s) => s.transferAccountId ?? null)]),
          searchAccount: `${account} ${peer ? (accountNames.get(peer) ?? "") : ""}`.toLowerCase(),
          text: `${t.payee} ${searchMemo} ${categoryName} ${splitMemos} ${t.date} ${money(Math.abs(r.amount))}`.toLowerCase(),
        };
      }),
    [entries, categoryNames, accountNames, accountsById, accountId, onBudget, needsCategory, sampleMoney],
  );
  type Row = (typeof indexed)[number];
  // Chips plus whatever is being typed (which searches everything as you type).
  const test = useMemo(() => {
    const terms: Term[] = query ? [...filters.terms, { field: "text", value: query }] : filters.terms;
    const match = compile(terms);
    return (r: Row) =>
      match({
        text: r.text,
        payee: r.searchPayee,
        memo: r.searchMemo,
        categoryIds: r.categoryIds,
        categories: r.categories,
        accountIds: r.accountIds,
        account: r.searchAccount,
        amount: r.amount,
        flag: r.flag,
        status: r.status,
        approved: r.approved,
        uncategorized: r.uncategorized,
      });
  }, [filters.terms, query]);
  const reportIds = useMemo(() => filters.reportQuery ? reportTransactionIds(budget, {
    ...filters.reportQuery, from: from || "0000-01-01", to: to || "9999-12-31",
  }) : null, [budget.transactions, budget.accounts, budget.categories, filters.reportQuery, from, to]);
  const rows = useMemo(
    () => indexed.filter((r) => (!reportIds || reportIds.has(r.t.id)) && (!from || r.t.date >= from) && (!to || r.t.date <= to) && test(r)),
    [indexed, from, to, test, reportIds],
  );
  const sorted = useMemo(() => {
    if (sort.key === "date") return sort.desc ? rows : [...rows].reverse();
    const sign = sort.desc ? -1 : 1;
    if (sort.key === "outflow" || sort.key === "inflow") {
      const value = (r: Row) => (sort.key === "outflow" ? Math.max(0, -r.amount) : Math.max(0, r.amount));
      return [...rows].sort((a, b) => sign * (value(a) - value(b)));
    }
    if (sort.key === "flag") {
      const name = (r: Row) => (r.flag ? flagName(r.flag, budget) : "");
      // Flagged rows come first either way round; the direction orders them by name.
      return [...rows].sort((a, b) => (!a.flag !== !b.flag ? (a.flag ? -1 : 1) : sign * collator.compare(name(a), name(b))));
    }
    const key = sort.key;
    return [...rows].sort((a, b) => sign * collator.compare(a[key], b[key]));
  }, [rows, sort, budget.flagNames]);
  const position = useMemo(() => new Map(sorted.map((r, i) => [r.key, i])), [sorted]);

  const upcoming = useMemo(() => {
    const list: Upcoming[] = [];
    // Paused ones too, so they can be resumed here; finished ones have no next date to list.
    for (const s of budget.schedules ?? []) {
      if (!s.nextDate) continue;
      const t = s.transaction;
      const own = all || t.accountId === accountId;
      const part = t.splits.find((p) => p.transferAccountId === accountId);
      const value = own ? t.amount : t.transferAccountId === accountId ? -t.amount : part ? -part.amount : null;
      if (value === null) continue;
      const peer = own ? t.transferAccountId : t.accountId;
      const self = own ? t.accountId : accountId;
      const transfer = transferView(accountsById, self, peer, value);
      const budgetAmount = onBudget(self) ? value : onBudget(peer) ? -value : null;
      const category = categoryLabel(t, categoryNames, noCategory(budgetAmount));
      const isTransfer = !!transfer && category === "Transfer";
      list.push({
        s,
        amount: value,
        date: s.nextDate,
        transfer,
        payee: peer ? `Transfer: ${accountNames.get(peer) ?? ""}` : t.payee || "No payee",
        category: isTransfer ? TRANSFER_CATEGORY : category,
        noCategory: isTransfer,
        memo: t.memo,
        account: accountNames.get(t.accountId) ?? "",
      });
    }
    return list.sort((a, b) => b.date.localeCompare(a.date));
  }, [budget.schedules, accountId, all, accountNames, accountsById, categoryNames, onBudget]);
  // What the date and amount columns need, without their padding: their widest value (over the
  // whole register, so filtering doesn't change them) or their heading, whichever is wider.
  const sampleDate = formatDate("2026-12-28");
  const fitted = useMemo((): Record<Fitted, number> => {
    // The starting balance shows as an amount and as the first running balance.
    const opening = all ? 0 : (accountsById.get(accountId)?.openingBalance ?? 0);
    let amount = Math.abs(opening);
    let high = Math.max(0, opening);
    let low = Math.min(0, opening);
    for (const r of entries) {
      amount = Math.max(amount, Math.abs(r.amount));
      high = Math.max(high, r.balance);
      low = Math.min(low, r.balance);
    }
    for (const u of upcoming) amount = Math.max(amount, Math.abs(u.amount));
    const figures = (n: number) => textWidth(money(n), 13, 400, true);
    // The longest date this format makes (month names differ), bold as an unapproved one is,
    // and the row editor's date field: 12.5px text, 8px and 24px of padding and its border, in a
    // cell padded 6px less than others on each side (.date-field).
    let date = 0;
    let field = 0;
    for (let m = 1; m <= 12; m++) {
      const text = formatDate(`2026-${String(m).padStart(2, "0")}-28`);
      date = Math.max(date, textWidth(text, 13, 600));
      field = Math.max(field, textWidth(text, 12.5) + 34 - 12);
    }
    // Outflow and Inflow match, so the two read as a pair.
    const amounts = Math.max(figures(amount), headingWidth("Outflow") + SORT_ARROW);
    return {
      date: Math.max(date, field, headingWidth("Date") + SORT_ARROW),
      outflow: amounts,
      inflow: amounts,
      balance: Math.max(figures(high), figures(low), headingWidth("Balance")),
    };
    // Samples change with the date and currency display formats.
  }, [entries, upcoming, all, accountId, accountsById, sampleDate, sampleMoney]);
  // Its padding and the 1px line on its left.
  const fittedWidth = (k: Fitted, compact = false) => Math.ceil(fitted[k]) + 2 * pad(compact) + 1;
  const columns = useColumns(
    "register-v3",
    {
      flag: ICON_WIDTHS.flag,
      cleared: ICON_WIDTHS.cleared,
      date: fittedWidth("date"),
      outflow: fittedWidth("outflow"),
      inflow: fittedWidth("inflow"),
      balance: fittedWidth("balance"),
      payee: TEXT_COLUMNS.payee!.min,
      account: TEXT_COLUMNS.account!.min,
      category: TEXT_COLUMNS.category!.min,
      memo: TEXT_COLUMNS.memo!.min,
    },
    // The date column always leaves room for the date field and its calendar button.
    { date: fittedWidth("date", true) },
  );
  // Upcoming entries follow the same search chips as the rows below them, and sit above them
  // while the list is newest first.
  const matchingUpcoming = useMemo(() => {
    if (filters.reportQuery) return [];
    if (sort.key !== "date" || !sort.desc) return [];
    const match = compile(query ? [...filters.terms, { field: "text", value: query }] : filters.terms);
    return upcoming.filter((u) => {
      const t = u.s.transaction;
      const lines = t.splits.filter((s) => !s.transferAccountId);
      return (
        (!to || u.date <= to) &&
        (!from || u.date >= from) &&
        match({
          text: `${u.payee} ${u.memo} ${u.category} ${u.date} ${money(Math.abs(u.amount))}`.toLowerCase(),
          payee: u.payee.toLowerCase(),
          memo: u.memo.toLowerCase(),
          categoryIds: new Set(lines.map((s) => s.categoryId ?? null)),
          categories: u.category.toLowerCase(),
          accountIds: new Set([t.accountId, t.transferAccountId]),
          account: u.account.toLowerCase(),
          amount: u.amount,
          flag: typeof t.flag === "string" ? t.flag : "",
          status: "uncleared",
          approved: false,
          uncategorized: needsCategory(t),
        })
      );
    });
  }, [upcoming, sort, from, to, query, filters.terms, filters.reportQuery, needsCategory, sampleMoney]);
  // They're listed under a heading that folds them away (Settings › Transactions sets whether they
  // start open).
  const visibleUpcoming = showUpcoming ? matchingUpcoming : [];
  // The heading's figures, as the old Scheduled page had them: what's due in the next 30 days.
  const dueSoon = useMemo(() => {
    const limit = new Date();
    limit.setDate(limit.getDate() + 30);
    const end = limit.toLocaleDateString("en-CA");
    const due = matchingUpcoming.filter((u) => !u.s.paused && u.date <= end);
    return {
      in: due.reduce((n, u) => n + Math.max(0, u.amount), 0),
      out: due.reduce((n, u) => n + Math.max(0, -u.amount), 0),
    };
  }, [matchingUpcoming]);
  // An account's register starts from its opening balance, so that's listed too (as YNAB's
  // "Starting Balance"), oldest, when the list is in date order and nothing is filtered out.
  const opening = useMemo(
    () => (all || sort.key !== "date" || anyFilter(filters) ? 0 : (budget.accounts.find((a) => a.id === accountId)?.openingBalance ?? 0)),
    [all, sort.key, filters, budget.accounts, accountId],
  );
  const items = useMemo(() => {
    const list: Item<Row>[] = [];
    if (matchingUpcoming.length) list.push({ kind: "schedules", key: "schedules" });
    for (const u of visibleUpcoming) list.push({ kind: "upcoming", key: `upcoming:${u.s.id}`, u });
    for (const r of sorted) {
      list.push({ kind: "row", key: r.key, r });
      if (expanded.has(r.key) && !r.reverse && r.t.splits.length > 1)
        r.t.splits.forEach((s, part) => list.push({ kind: "split", key: `${r.key}:line:${part}`, r, s, part }));
    }
    if (opening) {
      const start: Item<Row> = { kind: "opening", key: "opening", amount: opening };
      if (sort.desc) list.push(start);
      else list.unshift(start);
    }
    return list;
  }, [visibleUpcoming, matchingUpcoming, sorted, expanded, opening, sort.desc]);

  const ids = useMemo(() => [...new Set(sorted.map((r) => r.t.id))], [sorted]);
  const existing = useMemo(
    () => new Map(budget.transactions.map((t) => [t.id, t])),
    [budget.transactions],
  );
  // Selection changes infrequently compared with scrolling the virtual list. Keep the
  // full-selection scans out of scroll renders, especially when all rows are selected.
  const chosen = useMemo(() => [...selected].filter((id) => existing.has(id)), [selected, existing]);
  const isChosen = (id: string) => selected.has(id) && existing.has(id);
  const needsApproval = useMemo(() => chosen.some((id) => existing.get(id)?.approved === false), [chosen, existing]);
  // What the selection adds up to, as this register's Outflow and Inflow columns show it.
  const chosenTotals = useMemo(() => {
    let outflow = 0;
    let inflow = 0;
    for (const r of indexed)
      if (selected.has(r.t.id)) {
        if (r.amount < 0) outflow -= r.amount;
        else inflow += r.amount;
      }
    return { outflow, inflow };
  }, [indexed, selected]);
  const someChosen = useMemo(() => ids.some((id) => selected.has(id) && existing.has(id)), [ids, selected, existing]);
  const allChosen = useMemo(
    () => ids.length > 0 && ids.every((id) => selected.has(id) && existing.has(id)),
    [ids, selected, existing],
  );
  const earliest = useMemo(
    () => entries.reduce((min, r) => (!min || r.t.date < min ? r.t.date : min), ""),
    [entries],
  );

  const fail = (e: unknown) => toast({ message: errorText(e), tone: "error" });
  const undoable = (message: string) =>
    toast({ message, action: undo ? { label: "Undo", onClick: undo } : undefined });
  const run = async (operation: string, value: string | null, message: string) => {
    try {
      await transform((b) => api.action(b, { type: "bulk", ids: chosen, operation, value, accountId }));
      undoable(message);
      if (operation === "duplicate") setSelected(new Set());
    } catch (e) {
      fail(e);
    }
  };
  const exportListed = async () => {
    try {
      const filtered = anyFilter(filters);
      const listed = filtered ? [...new Set(rows.map((r) => r.t.id))] : null;
      const path = await api.exportTransactions(budget, all ? null : accountId, listed);
      if (!path) return;
      const n = listed ? listed.length : null;
      toast({
        message: n === null ? "Transactions exported." : `Exported ${n} listed ${n === 1 ? "transaction" : "transactions"}.`,
        action: { label: "Show in folder", onClick: () => void api.reveal(path).catch(fail) },
      });
    } catch (e) {
      fail(e);
    }
  };
  const remove = async (list: string[]) => {
    if (!list.length) return;
    try {
      await transform((b) => api.action(b, { type: "bulk", ids: list, operation: "delete", value: "", accountId }));
      setSelected(new Set());
      setEditing(null);
      undoable(list.length === 1 ? "Transaction deleted." : `${list.length.toLocaleString()} transactions deleted.`);
    } catch (e) {
      fail(e);
    }
  };
  const save = async (t: Transaction, repeat?: string) => {
    let before: Budget | undefined;
    await transform((b) => {
      before = b;
      return saveTransaction(b, t, repeat);
    });
    if (before) learn(before, t);
    const message = scheduledMessage(t, repeat);
    if (message) undoable(message);
  };
  const pauseSchedule = (s: Schedule, paused: boolean) =>
    edit((b) => {
      const found = b.schedules?.find((x) => x.id === s.id);
      if (found) found.paused = paused;
    });
  const deleteSchedule = (u: Upcoming) => {
    edit((b) => {
      b.schedules = (b.schedules ?? []).filter((x) => x.id !== u.s.id);
    });
    undoable(`Deleted the schedule for ${u.payee}.`);
  };
  const storeSchedule = async (s: Schedule, isNew: boolean) => {
    await transform((b) => saveSchedule(b, s));
    const t = s.transaction;
    const name = t.payee || `the transfer to ${accountNames.get(t.transferAccountId ?? "") ?? "another account"}`;
    const repeat = s.frequency === "never" ? undefined : s.frequency;
    const message =
      s.nextDate && s.nextDate <= today() && !s.paused
        ? `Entered ${name}.${repeat ? ` It repeats ${(frequencies[repeat] ?? repeat).toLowerCase()}.` : ""}`
        : isNew && s.nextDate
          ? scheduledFor(s.nextDate, repeat)
          : "";
    if (message) undoable(message);
  };
  const schedule = async (u: Upcoming, skip: boolean) => {
    try {
      await transform((b) => api.previewSchedules(b, today(), u.s.id, skip));
      undoable(
        !skip
          ? `Entered ${u.payee}.`
          : u.s.frequency === "never"
            ? `Skipped ${u.payee}. It won't be entered.`
            : `Skipped ${u.payee} on ${formatDate(u.date)}.`,
      );
    } catch (e) {
      fail(e);
    }
  };

  const focusGrid = () => requestAnimationFrame(() => scroller.current?.focus({ preventScroll: true }));
  const startEdit = (r: Row, focus?: EditorField) => {
    if (!inlineEditable(r.t, r.reverse, r.part) || !r.t.id) return onEdit(r.t);
    setAdding(false);
    setEditLines(0);
    setEditing({ key: r.key, focus });
  };
  const toggle = (id: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const rangeIds = (a: string, b: string) => {
    const i = position.get(a) ?? 0,
      j = position.get(b) ?? 0;
    return sorted.slice(Math.min(i, j), Math.max(i, j) + 1).map((r) => r.t.id);
  };
  const pick = (r: Row, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) => {
    const add = e.ctrlKey || e.metaKey;
    if (e.shiftKey && anchor.current && position.has(anchor.current)) {
      const range = rangeIds(anchor.current, r.key);
      setSelected((s) => new Set(add ? [...s, ...range] : range));
    } else {
      if (add) toggle(r.t.id);
      else setSelected(new Set([r.t.id]));
      anchor.current = r.key;
    }
    setCursor(r.key);
  };
  /**
   * A row's checkbox, or anywhere in its cell: adds or removes the row and keeps the rest of the
   * selection. With Shift, every row from the last one checked to this one follows it.
   */
  const check = (r: Row, shift: boolean) => {
    const on = !isChosen(r.t.id);
    if (shift && anchor.current && position.has(anchor.current)) {
      const range = rangeIds(anchor.current, r.key);
      setSelected((s) => {
        const next = new Set(s);
        for (const id of range) {
          if (on) next.add(id);
          else next.delete(id);
        }
        return next;
      });
    } else toggle(r.t.id);
    anchor.current = r.key;
    setCursor(r.key);
  };

  // Item positions, with the taller edit row accounted for.
  const editIndex = editing ? items.findIndex((i) => i.key === editing.key) : -1;
  const extra = editHeight(editLines) - rowHeight;
  const topOf = (index: number) => index * rowHeight + (editIndex >= 0 && index > editIndex ? extra : 0);
  const reveal = (key: string) => {
    const el = scroller.current;
    const index = items.findIndex((i) => i.key === key);
    if (!el || index < 0) return;
    // The pinned toolbar and the sticky header cover the top of the scroll area.
    const top = rowsTop + topOf(index);
    const covered = pinned.height.current + head;
    if (top < el.scrollTop + covered) el.scrollTop = top - covered;
    else if (top + rowHeight > el.scrollTop + viewport) el.scrollTop = top + rowHeight - viewport;
  };
  // A transfer's link from the other account opens this register on its transaction, selected and
  // mid-screen. The account change resets the filters and scroll a render or two later (the
  // search is deferred), so it's shown again each time until they have settled.
  const settled = filters === reset.current && query === filters.text.trim().toLowerCase();
  useEffect(() => {
    const id = focus.current;
    const el = scroller.current;
    if (!id || !el) return;
    const index = items.findIndex((i) => i.kind === "row" && i.r.t.id === id);
    if (index < 0) return;
    const key = items[index].key;
    setSelected(new Set([id]));
    setCursor(key);
    anchor.current = key;
    el.scrollTop = Math.max(0, rowsTop + topOf(index) - viewport / 2);
    if (settled) focus.current = null;
  }, [items, settled]);
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    // A focused action owns Enter/Space; grid shortcuts must not swallow its native click.
    if (target.closest("button,a")) return;
    // Keys belong to the field being typed in; a row's checkbox lets them through.
    const checkbox = target instanceof HTMLInputElement && target.type === "checkbox";
    if (target.closest(".reg-editor, select, textarea") || (target.closest("input") && !checkbox)) return;
    const plain = !e.ctrlKey && !e.metaKey && !e.altKey;
    const n = chosen.length === 1 ? "1 transaction" : `${chosen.length.toLocaleString()} transactions`;
    const index = cursor ? (position.get(cursor) ?? -1) : -1;
    const move = (to: number) => {
      const next = sorted[Math.min(sorted.length - 1, Math.max(0, to))];
      if (!next) return;
      e.preventDefault();
      if (e.shiftKey) {
        anchor.current ??= cursor ?? next.key;
        setSelected(new Set(rangeIds(anchor.current, next.key)));
      } else {
        setSelected(new Set([next.t.id]));
        anchor.current = next.key;
      }
      setCursor(next.key);
      setKeyed(true);
      reveal(next.key);
    };
    const page = Math.max(1, Math.floor(viewport / rowHeight) - 1);
    if (e.key === "ArrowDown") move(index + 1);
    else if (e.key === "ArrowUp") move(index < 0 ? 0 : index - 1);
    else if (e.key === "PageDown") move(index + page);
    else if (e.key === "PageUp") move(index - page);
    else if (e.key === "Home") move(0);
    else if (e.key === "End") move(sorted.length - 1);
    else if ((e.key === "Enter" || e.key === "F2") && index >= 0) {
      e.preventDefault();
      startEdit(sorted[index]);
    } else if (e.key === " " && index >= 0 && !checkbox) {
      e.preventDefault();
      setKeyed(true);
      toggle(sorted[index].t.id);
    } else if ((e.key === "Delete" || e.key === "Backspace") && chosen.length) {
      e.preventDefault();
      void remove(chosen);
    } else if (e.key === "Escape" && chosen.length) {
      e.preventDefault();
      setSelected(new Set());
      anchor.current = null;
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      setSelected(new Set(ids));
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "d" && chosen.length) {
      e.preventDefault();
      void run("duplicate", "", `Duplicated ${n}.`);
    } else if (plain && e.key.toLowerCase() === "c" && chosen.length) {
      // Clears the selection, or unclears it when it's all cleared already.
      e.preventDefault();
      const uncleared = sorted.some((r) => selected.has(r.t.id) && r.status === "uncleared");
      void run("status", uncleared ? "cleared" : "uncleared", `Marked ${n} ${uncleared ? "cleared" : "uncleared"}.`);
    } else if (plain && e.key.toLowerCase() === "a" && chosen.length && needsApproval) {
      e.preventDefault();
      void run("approve", "", `Approved ${n}.`);
    } else if (plain && (e.key.toLowerCase() === "n" || e.key === "Insert") && canAdd) {
      e.preventDefault();
      setEditing(null);
      setAdding(true);
    } else if (e.key === "/" || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f")) {
      e.preventDefault();
      document.querySelector<HTMLInputElement>('[aria-label="Search transactions"]')?.focus();
    }
  };
  // Esc clears the selection from anywhere on the page, unless it belongs to a field, a dialog or
  // a popover.
  useEffect(() => {
    if (!chosen.length) return;
    const escape = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const target = e.target as HTMLElement;
      const typing =
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLInputElement && target.type !== "checkbox");
      if (typing || target.closest?.(".reg-editor") || document.querySelector(".modal-shade, .popover")) return;
      setSelected(new Set());
      anchor.current = null;
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [chosen.length]);
  const rowClick = (r: Row) => (e: MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    if (target.closest("button,input,select,a")) return;
    // Beside the checkbox counts as on it, so a click that just misses keeps the selection.
    if (target.closest(".select-cell")) return check(r, e.shiftKey);
    const plain = !e.shiftKey && !e.ctrlKey && !e.metaKey;
    // A second click on the only selected row edits it, starting in the clicked column.
    if (plain && cursor === r.key && chosen.length === 1 && selected.has(r.t.id))
      startEdit(r, fieldOf(target));
    else pick(r, e);
  };
  const toggleSort = (key: SortKey) =>
    setSort((s) => (s.key === key ? { key, desc: !s.desc } : { key, desc: amountKeys.includes(key) }));
  /** A sortable heading; one with an `icon` shows it instead of its label until dragged wider. */
  const header = (key: SortKey, label: string, className = "", resizable = true, icon?: ReactNode) => (
    <span
      role="columnheader"
      className={className}
      aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : undefined}
    >
      <button className="sort-button" onClick={() => toggleSort(key)} title={icon ? label : undefined}>
        {icon ? (
          <>
            <span className="col-icon" aria-hidden>
              {icon}
            </span>
            <span className="col-label">{label}</span>
          </>
        ) : (
          label
        )}
        {sort.key === key && (sort.desc ? <ArrowDown size={12} /> : <ArrowUp size={12} />)}
      </button>
      {resizable ? columns.grip(key, label) : <span className="col-divider" aria-hidden />}
    </span>
  );
  // Like the budget table, the table fills its area. Icon, date and amount columns keep their
  // widths; the text columns share what's left, down to their minimums, and Memo is hidden when
  // even those don't fit. Only then does the table scroll sideways. Text cells are padded on both
  // sides (less when space is tight), icon cells a little.
  const custom = columns.set as Partial<Record<ColumnKey, number>>;
  const widthOf = (k: ColumnKey, compact: boolean) =>
    k === "select"
      ? ICON_WIDTHS.select
      : k in fitted && custom[k] === undefined
        ? fittedWidth(k as Fitted, compact)
        : columns.widths[k as keyof typeof columns.widths];
  const textMin = (k: ColumnKey) => Math.min(TEXT_COLUMNS[k]!.min, custom[k] ?? Infinity);
  const minOf = (k: ColumnKey, compact: boolean) => (TEXT_COLUMNS[k] ? textMin(k) : widthOf(k, compact));
  const track = (k: ColumnKey, compact: boolean) => {
    const text = TEXT_COLUMNS[k];
    if (!text) return `${widthOf(k, compact)}px`;
    const set = k === "memo" ? undefined : custom[k];
    return `minmax(${textMin(k)}px, ${set === undefined ? `${text.share}fr` : `${set}px`})`;
  };
  // Narrowest the table can be: every column at its minimum, plus the border.
  const tableMin = (keys: ColumnKey[], compact: boolean) => keys.reduce((n, k) => n + minOf(k, compact), 0) + 2;
  // When it doesn't fit: drop Memo and tighten the padding, then Flag (the bar at the row's edge
  // shows flags instead), then (in an account) the running balance. Flag also goes when it's
  // turned off in Settings.
  let shownColumns = columnsFor(all).filter((k) => flagColumn || k !== "flag");
  let compact = false;
  const fits = () => !areaWidth || areaWidth >= tableMin(shownColumns, compact);
  if (!fits()) {
    shownColumns = shownColumns.filter((k) => k !== "memo");
    compact = true;
  }
  if (!fits()) shownColumns = shownColumns.filter((k) => k !== "flag");
  // No running balance when it's turned off in Settings, or when nothing else makes room.
  if (!runningBalance || !fits()) shownColumns = shownColumns.filter((k) => k !== "balance");
  const showMemo = shownColumns.includes("memo");
  const showFlag = shownColumns.includes("flag");
  const showBalance = !all && sort.key === "date" && shownColumns.includes("balance");
  // Where the starting balance went: a budget account's adds to (or takes from) Unassigned; a
  // card's debt and a tracking account's balance don't touch it.
  const account = accountsById.get(accountId);
  const openingNote =
    !onBudget(accountId) || (account?.kind === "credit" && opening < 0) ? "" : noCategory(opening);
  const cols = shownColumns.map((k) => track(k, compact)).join(" ");
  const minWidth = tableMin(shownColumns, compact);

  // The edit row is taller, so items after it start lower.
  const local = Math.max(0, scrollTop - rowsTop);
  const shifted = editIndex >= 0 && local > topOf(editIndex) ? local - extra : local;
  const start = Math.max(0, Math.floor(shifted / rowHeight) - OVERSCAN);
  const end = Math.min(items.length, Math.ceil((shifted + viewport) / rowHeight) + OVERSCAN);

  const canAdd = budget.accounts.some((a) => !a.archived);
  const empty = !entries.length && !upcoming.length;
  const add = () => {
    setEditing(null);
    setAdding(true);
  };
  const approveButton = (r: Row) =>
    r.t.approved === false && (
      <button
        className="approve"
        title="Waiting for approval: click to approve (A)"
        aria-label={`Approve ${r.payee} ${r.t.date}`}
        onClick={() =>
          edit((b) => {
            b.transactions.find((x) => x.id === r.t.id)!.approved = true;
          })
        }
      />
    );
  const clearButton = (r: Row) => (
    <button
      className={`clear-button ${r.status}`}
      disabled={r.status === "reconciled"}
      title={r.status === "reconciled" ? "Reconciled" : `${r.status[0].toUpperCase() + r.status.slice(1)} (C)`}
      aria-label={`${r.status[0].toUpperCase() + r.status.slice(1)}: ${r.payee} ${r.t.date}`}
      onClick={() =>
        edit((b) => {
          const row = b.transactions.find((x) => x.id === r.t.id)!;
          const next: Status = r.status === "uncleared" ? "cleared" : "uncleared";
          if (r.part !== undefined) row.splits[r.part].transferStatus = next;
          else if (r.reverse) row.transferStatus = next;
          else row.status = next;
        })
      }
    >
      <ClearedMark status={r.status} />
    </button>
  );
  const closeEditor = () => {
    setEditing(null);
    focusGrid();
  };
  const editor = (initial?: Transaction, focus?: EditorField, reverse = false) => (
    <TransactionEditor
      key={initial?.id ?? "new"}
      budget={budget}
      accountId={accountId}
      initial={initial}
      reverse={reverse}
      onLines={initial ? setEditLines : undefined}
      focus={focus}
      available={available}
      onCreateCategory={categories.create}
      onSave={save}
      onCancel={
        initial
          ? closeEditor
          : () => {
              setAdding(false);
              focusGrid();
            }
      }
      onMore={(draft, repeat) => {
        if (initial) {
          setEditing(null);
          onEdit({ ...initial, ...draft });
        } else {
          setAdding(false);
          onAddDialog?.(draft, repeat);
        }
      }}
    />
  );
  const bulkBar = (
    <BulkBar
      budget={budget}
      count={chosen.length}
      totals={chosenTotals}
      needsApproval={needsApproval}
      available={available}
      onRun={(operation, value, message) => void run(operation, value, message)}
      onDelete={() => void remove(chosen)}
      onClear={() => setSelected(new Set())}
      onAsk={onAsk && (() => onAsk(chosen))}
    />
  );
  return (
    <section
      className={`register${all ? " all" : ""}${chosen.length ? " has-selection" : ""}${showMemo ? "" : " no-memo"}${showFlag ? "" : " no-flag"}${compact ? " compact" : ""}${!all && !shownColumns.includes("balance") ? " no-balance" : ""}`}
      style={{ "--cols": cols, "--select": `${minOf("select", compact)}px` } as CSSProperties}
    >
      <div className="register-pinned" ref={pinned.ref}>
        {lead}
        <RegisterToolbar
          budget={budget}
          filters={filters}
          onChange={change}
          earliest={earliest}
          count={rows.length}
          canAdd={canAdd}
          onAdd={add}
          onImport={onImport}
          onSync={onSync}
          onExport={() => void exportListed()}
          upcoming={upcoming.length}
          onSchedules={() => setSchedulesOpen(true)}
          showUpcoming={showUpcoming}
          onShowUpcoming={setShowUpcoming}
        />
      </div>
      {/* The scroll area runs from the top of the page to the window's edge, under the pinned
          balances and toolbar, so its scrollbar runs the page's full height outside the table. */}
      <div
        className={`register-scroll${keyed ? " keyed" : ""}`}
        ref={scroller}
        onPointerDown={() => setKeyed(false)}
        tabIndex={0}
        aria-activedescendant={cursor && position.has(cursor) ? `reg-${cursor}` : undefined}
        onKeyDown={keys}
        onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
      >
      <div
        className="register-table"
        style={{ minWidth }}
        role="grid"
        aria-label="Transactions"
        aria-rowcount={items.length + 1}
        aria-multiselectable
      >
        <div className="reg-row reg-head" role="row">
          <span
            role="columnheader"
            className="select-cell"
            onClick={(e) => {
              if (!(e.target as HTMLElement).closest("input")) setSelected(new Set(allChosen ? [] : ids));
            }}
          >
            <input
              type="checkbox"
              aria-label="Select all filtered transactions"
              checked={allChosen}
              ref={(el) => {
                if (el) el.indeterminate = someChosen && !allChosen;
              }}
              onChange={() => setSelected(new Set(allChosen ? [] : ids))}
            />
          </span>
          {header("flag", "Flag", "flag-cell icon-col", true, <FlagIcon size={13} />)}
          {header("date", "Date")}
          {header("payee", "Payee")}
          {all && header("account", "Account")}
          {header("category", "Category")}
          {header("memo", "Memo", "memo", false)}
          {header("outflow", "Outflow", "num")}
          {header("inflow", "Inflow", "num")}
          {!all && (
            <span
              role="columnheader"
              className="num balance"
              title={showBalance ? undefined : "Running balances show when sorted by date"}
            >
              {showBalance ? "Balance" : ""}
              {columns.grip("balance", "Balance")}
            </span>
          )}
          <span role="columnheader" className="icon-col" title="Cleared">
            <span className="col-icon" aria-hidden>
              <CircleCheck size={14} />
            </span>
            <span className="col-label">Cleared</span>
            {columns.grip("cleared", "Cleared")}
          </span>
        </div>
        {adding && editor()}
          <div ref={rowsStart} style={{ height: topOf(start) }} aria-hidden />
          {items.slice(start, end).map((item) => {
            if (item.kind === "schedules") {
              const paused = matchingUpcoming.filter((u) => u.s.paused).length;
              return (
                <div key={item.key} role="row" className="reg-row schedules-head">
                  <span role="cell" aria-colspan={shownColumns.length}>
                    <button
                      className="schedules-toggle"
                      aria-expanded={showUpcoming}
                      onClick={() => setShowUpcoming(!showUpcoming)}
                    >
                      {showUpcoming ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                      Scheduled
                      <span className="schedules-count">{matchingUpcoming.length}</span>
                    </button>
                    {paused > 0 && <span className="schedules-note">{paused} paused</span>}
                    <span className="spacer" />
                    {(dueSoon.out > 0 || dueSoon.in > 0) && (
                      <span className="schedules-note" title="Due in the next 30 days (each schedule's next date)">
                        Next 30 days
                        {dueSoon.out > 0 && <strong>{money(dueSoon.out)} out</strong>}
                        {dueSoon.in > 0 && <strong className="positive">{money(dueSoon.in)} in</strong>}
                      </span>
                    )}
                  </span>
                </div>
              );
            }
            if (item.kind === "upcoming") {
              const u = item.u;
              const often = frequencies[u.s.frequency] ?? u.s.frequency;
              return (
                <div
                  key={item.key}
                  role="row"
                  className={`reg-row upcoming${u.s.paused ? " paused" : ""}`}
                  title={`Scheduled · ${often}${u.s.paused ? " · Paused" : ""}`}
                  onDoubleClick={(e) => {
                    if (!(e.target as HTMLElement).closest("button")) onEditSchedule?.(u.s);
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setScheduleMenu({ id: u.s.id, x: e.clientX, y: e.clientY });
                  }}
                >
                  <span role="cell">
                    <button
                      className="schedule-menu-button"
                      aria-label={`Schedule options: ${u.payee}`}
                      title="Schedule options"
                      onClick={(e) => {
                        const box = e.currentTarget.getBoundingClientRect();
                        setScheduleMenu({ id: u.s.id, x: box.left, y: box.bottom + 2 });
                      }}
                    >
                      <CalendarClock size={13} aria-hidden />
                    </button>
                  </span>
                  <span role="cell" className="flag-cell icon-col">
                    {u.s.transaction.flag && (
                      <FlagTag flag={u.s.transaction.flag} name={flagName(u.s.transaction.flag, budget)} />
                    )}
                  </span>
                  <span role="cell" className="date">
                    {formatDate(u.date)}
                  </span>
                  <span role="cell" title={u.transfer?.label ?? u.payee} className={u.transfer ? "transfer-payee" : undefined}>
                    {u.transfer ? <TransferPayee view={u.transfer} /> : u.payee}
                  </span>
                  {all && (
                    <span role="cell" title={u.account}>
                      {u.account}
                    </span>
                  )}
                  <span role="cell" title={u.category} className={u.noCategory ? "transfer-note" : undefined}>
                    {u.category}
                  </span>
                  {/* How often it repeats leads the memo cell, which is empty on most schedules. */}
                  <span role="cell" className="memo" title={[often, u.s.paused && "Paused", u.memo].filter(Boolean).join(" · ")}>
                    {u.s.paused && <span className="repeat-label paused-label">Paused</span>}
                    <span className="repeat-label">{often}</span>
                    {u.memo}
                  </span>
                  <span role="cell" className="num">
                    {u.amount < 0 ? money(-u.amount) : ""}
                  </span>
                  <span role="cell" className="num">
                    {u.amount > 0 ? money(u.amount) : ""}
                  </span>
                  {!all && <span role="cell" className="balance" />}
                  {/* Enter posts this occurrence now, dated today; Skip moves the schedule on to
                      its next date without posting. Either way the schedule carries on. */}
                  <span role="cell" className="upcoming-actions icon-col">
                    <span className="schedule-actions">
                      <button
                        className="row-action"
                        onClick={() => void schedule(u, false)}
                        title="Enter now: adds it to the register dated today"
                      >
                        <CornerDownLeft size={14} aria-hidden />
                        <span className="action-label">Enter</span>
                      </button>
                      <button
                        className="row-action"
                        onClick={() => void schedule(u, true)}
                        title={`Skip ${formatDate(u.date)}: the schedule moves on to its next date`}
                      >
                        <SkipForward size={13} aria-hidden />
                        <span className="action-label">Skip</span>
                      </button>
                    </span>
                  </span>
                </div>
              );
            }
            if (item.kind === "opening")
              return (
                <div
                  key={item.key}
                  role="row"
                  className="reg-row opening"
                  title="The account's balance when it was added, before any transaction here. Change it from Edit account."
                >
                  <span role="cell" />
                  <span role="cell" className="flag-cell" />
                  <span role="cell" className="date" />
                  <span role="cell">Starting balance</span>
                  <span role="cell">{openingNote}</span>
                  <span role="cell" className="memo" />
                  <span role="cell" className="num">
                    {item.amount < 0 ? money(-item.amount) : ""}
                  </span>
                  <span role="cell" className="num positive">
                    {item.amount > 0 ? money(item.amount) : ""}
                  </span>
                  <span role="cell" className={`num balance ${item.amount < 0 ? "negative" : ""}`}>
                    {showBalance ? money(item.amount) : ""}
                  </span>
                  <span role="cell" />
                </div>
              );
            if (item.kind === "split") {
              const s = item.s;
              const uncategorized = needsCategory(item.r.t, s);
              const name = s.transferAccountId
                ? `Transfer: ${accountNames.get(s.transferAccountId) ?? ""}`
                : uncategorized
                  ? NEEDS_CATEGORY
                  : (categoryNames.get(s.categoryId ?? "") ?? noCategory(onBudget(item.r.t.accountId) ? s.amount : null));
              return (
                <div key={item.key} role="row" className="reg-row split-line">
                  <span role="cell" />
                  <span role="cell" className="flag-cell" />
                  <span role="cell" />
                  <span role="cell" className="muted" title={s.payee ?? ""}>
                    {s.payee ?? ""}
                  </span>
                  {all && <span role="cell" />}
                  <span role="cell" title={name} className={uncategorized ? "needs-category" : undefined}>
                    {uncategorized ? <NeedsCategory /> : name}
                  </span>
                  <span role="cell" className="memo muted" title={s.memo ?? ""}>
                    {s.memo ?? ""}
                  </span>
                  <span role="cell" className="num">
                    {s.amount < 0 ? money(-s.amount) : ""}
                  </span>
                  <span role="cell" className="num positive">
                    {s.amount > 0 ? money(s.amount) : ""}
                  </span>
                  {!all && <span role="cell" className="balance" />}
                  <span role="cell" />
                </div>
              );
            }
            const r = item.r;
            const t = r.t;
            if (editing?.key === r.key) return editor(t, editing.focus, r.reverse);
            const picked = isChosen(t.id);
            const split = !r.reverse && t.splits.length > 1;
            // Dated after today: it hasn't happened yet, so it reads quieter.
            const future = t.date > now;
            return (
              <div
                key={r.key}
                id={`reg-${r.key}`}
                role="row"
                aria-selected={picked}
                className={`reg-row${picked ? " selected" : ""}${cursor === r.key ? " cursor" : ""}${t.approved === false ? " unapproved" : ""}${future ? " future" : ""}`}
                onClick={rowClick(r)}
                onDoubleClick={(e) => {
                  const target = e.target as HTMLElement;
                  if (!target.closest("button,input,.select-cell")) startEdit(r, fieldOf(target));
                }}
              >
                <span role="cell" className="select-cell">
                  <input
                    type="checkbox"
                    aria-label={`Select ${r.payee} ${t.date}`}
                    checked={picked}
                    onChange={(e) => check(r, (e.nativeEvent as globalThis.MouseEvent).shiftKey)}
                  />
                  {/* In the margin beside the row (.reg-row .approve). */}
                  {approveButton(r)}
                </span>
                {r.flag && <i className="flag-bar" data-flag={r.flag} title={`${flagName(r.flag, budget)} flag`} aria-hidden />}
                <span role="cell" className="flag-cell icon-col">
                  <button
                    className={`flag-pick${r.flag ? "" : " empty"}`}
                    aria-label={r.flag ? `${flagName(r.flag, budget)} flag: change` : `Flag ${r.payee} ${t.date}`}
                    title={r.flag ? `${flagName(r.flag, budget)} flag` : "Add a flag"}
                    onClick={(e) => {
                      const box = e.currentTarget.getBoundingClientRect();
                      setFlagMenu({ key: r.key, x: box.left, y: box.bottom + 2 });
                    }}
                  >
                    {r.flag ? <FlagTag flag={r.flag} name={flagName(r.flag, budget)} /> : <FlagIcon size={11} outline />}
                  </button>
                </span>
                <span role="cell" className="date" data-field="Date" title={future ? "Dated in the future" : undefined}>
                  {r.flag && <span className="sr-only">{flagName(r.flag, budget)} flag, </span>}
                  {formatDate(t.date)}
                </span>
                <span role="cell" data-field="Payee" title={r.transfer?.label ?? r.payee} className={r.transfer ? "transfer-payee" : undefined}>
                  {r.transfer ? (
                    <TransferPayee
                      view={r.transfer}
                      onOpen={onOpenTransfer && (() => onOpenTransfer(r.transfer!.account, t.id))}
                    />
                  ) : (
                    r.payee
                  )}
                </span>
                {all && (
                  <span role="cell" className="muted" title={r.account}>
                    {r.account}
                  </span>
                )}
                <span
                  role="cell"
                  data-field="Category"
                  title={r.category}
                  className={r.noCategory ? "transfer-note" : r.uncategorized ? "needs-category" : undefined}
                >
                  {split ? (
                    <button
                      className="split-toggle"
                      aria-expanded={expanded.has(r.key)}
                      onClick={() =>
                        setExpanded((x) => {
                          const next = new Set(x);
                          if (next.has(r.key)) next.delete(r.key);
                          else next.add(r.key);
                          return next;
                        })
                      }
                    >
                      {expanded.has(r.key) ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                      {r.category}
                    </button>
                  ) : r.uncategorized ? (
                    <NeedsCategory />
                  ) : (
                    r.category
                  )}
                </span>
                <span role="cell" className="memo muted" data-field="Memo" title={r.memo}>
                  {r.memo}
                </span>
                <span role="cell" className="num" data-field="Outflow">
                  {r.amount < 0 ? money(-r.amount) : ""}
                </span>
                <span role="cell" className="num positive" data-field="Inflow">
                  {r.amount > 0 ? money(r.amount) : ""}
                </span>
                {!all && (
                  <span role="cell" className={`num balance ${r.balance < 0 ? "negative" : ""}`}>
                    {showBalance ? money(r.balance) : ""}
                  </span>
                )}
                <span role="cell" className="status-cell icon-col">
                  {clearButton(r)}
                </span>
              </div>
            );
          })}
          <div
            style={{ height: Math.max(0, topOf(items.length) - topOf(end)) }}
            aria-hidden
          />
          {adding ? null : empty ? (
            <div className="register-empty">
              <h3>No transactions yet</h3>
              <p>
                {all
                  ? "Add a transaction, or open an account to import a file from your bank."
                  : "Add a transaction, or import a file from your bank."}
              </p>
              <div className="button-row">
                <button className="primary" disabled={!canAdd} onClick={add}>
                  Add transaction
                </button>
                {onImport && (
                  <button className="secondary" onClick={onImport}>
                    Import file
                  </button>
                )}
              </div>
            </div>
          ) : (
            !items.length && (
              <div className="register-empty">
                <h3>No transactions match these filters.</h3>
                {anyFilter(filters) && (
                  <button className="secondary" onClick={() => setFilters(emptyFilters())}>
                    Clear filters
                  </button>
                )}
              </div>
            )
          )}
        </div>
      </div>
      {schedulesOpen && (
        <SchedulesModal
          title={all ? "Scheduled transactions" : `Scheduled · ${accountNames.get(accountId) ?? ""}`}
          items={upcoming}
          showAccount={all}
          close={() => setSchedulesOpen(false)}
          onEdit={onEditSchedule}
          onEnter={(u) => void schedule(u, false)}
          onSkip={(u) => void schedule(u, true)}
          onPause={pauseSchedule}
          onDelete={deleteSchedule}
        />
      )}
      {scheduleEditor && (
        <ScheduleDialog
          budget={budget}
          schedule={scheduleEditor.schedule}
          defaultAccount={all ? undefined : accountId}
          save={(s) => storeSchedule(s, !scheduleEditor.schedule)}
          close={() => setScheduleEditor(null)}
          onCreateCategory={categories.create}
        />
      )}
      {scheduleMenu &&
        (() => {
          const u = upcoming.find((x) => x.s.id === scheduleMenu.id);
          if (!u) return null;
          const items: MenuItem[] = [
            { label: "Enter now", icon: <CornerDownLeft size={14} />, onSelect: () => void schedule(u, false) },
            { label: `Skip ${formatDate(u.date)}`, icon: <SkipForward size={14} />, onSelect: () => void schedule(u, true) },
            u.s.paused
              ? { label: "Resume", icon: <Play size={14} />, onSelect: () => pauseSchedule(u.s, false) }
              : { label: "Pause", icon: <Pause size={14} />, onSelect: () => pauseSchedule(u.s, true) },
            "separator",
            { label: "Delete schedule", icon: <Trash2 size={14} />, danger: true, onSelect: () => deleteSchedule(u) },
          ];
          if (onEditSchedule)
            items.unshift({ label: "Edit schedule…", icon: <Pencil size={14} />, onSelect: () => onEditSchedule(u.s) });
          return (
            <Menu
              open
              onClose={() => setScheduleMenu(null)}
              anchor={{ x: scheduleMenu.x, y: scheduleMenu.y }}
              label={`Schedule for ${u.payee}`}
              items={items}
            />
          );
        })()}
      {flagMenu &&
        (() => {
          const r = indexed.find((x) => x.key === flagMenu.key);
          if (!r) return null;
          const set = (flag: string | null) =>
            edit((b) => {
              const row = b.transactions.find((x) => x.id === r.t.id);
              if (!row) return;
              // A transfer seen from its other account carries that side's flag.
              if (r.reverse) {
                const side = r.part !== undefined ? row.splits[r.part] : row;
                side.transferSource = { ...(side.transferSource ?? {}), flag_color: flag };
              } else row.flag = flag;
            });
          return (
            <Menu
              open
              onClose={() => setFlagMenu(null)}
              anchor={{ x: flagMenu.x, y: flagMenu.y }}
              label="Flag"
              items={[
                ...flagsOf(budget).map(([flag, name]) => ({
                  label: name,
                  icon: <FlagIcon flag={flag} size={11} />,
                  hint: r.flag === flag ? "✓" : undefined,
                  onSelect: () => set(flag),
                })),
                "separator" as const,
                { label: "No flag", disabled: !r.flag, onSelect: () => set(null) },
                { label: "Name the flags…", onSelect: () => setNamingFlags(true) },
              ]}
            />
          );
        })()}
      {namingFlags && (
        <FlagNamesDialog
          budget={budget}
          close={() => setNamingFlags(false)}
          save={(names) => edit((b) => setFlagNames(b, names))}
        />
      )}
      {chosen.length > 0 && bulkBar}
      {categories.dialog}
    </section>
  );
}
