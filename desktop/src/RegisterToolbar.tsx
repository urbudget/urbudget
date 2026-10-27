import { useRef, useState, type FormEvent, type RefObject } from "react";
import { CalendarClock, Check, CheckCheck, Copy, Download, Flag, ListFilter, MessageSquareText, MoreHorizontal, Plus, RefreshCw, Tag, Trash2, Upload, X } from "lucide-react";
import { CategoryPicker, DateRangePicker, HeaderSlot, Menu, Popover, type DateRange } from "./ui";
import { FlagIcon, flagsOf } from "./TransactionRow";
import { SearchBox } from "./SearchBox";
import { parseAmountTerm, type Term } from "./search";
import { editableAmount, money } from "./format";
import { keyLabel } from "./platform";
import type { Budget } from "./types";
import type { LineQuery } from "./reports/TransactionsDialog";
import { formatCount, msg, t, tc, tn } from "./i18n";

export type Filters = {
  /** What's being typed in the search bar; it filters as you type. */
  text: string;
  /** Filters added as chips. */
  terms: Term[];
  range: DateRange;
  reportQuery?: LineQuery;
};
export const emptyFilters = (): Filters => ({
  text: "",
  terms: [],
  range: { preset: "all", from: "", to: "" },
  reportQuery: undefined,
});
export const anyFilter = (f: Filters) => !!f.text.trim() || f.terms.length > 0 || f.range.preset !== "all" || !!f.reportQuery;

const STATUSES: [string, string][] = [
  ["all", msg("Any status")],
  ["uncleared", msg("Uncleared")],
  ["cleared", msg("Cleared")],
  ["reconciled", msg("Reconciled")],
  ["unreconciled", msg("Not reconciled")],
];
/** Replaces the chips of some fields with `next` (none to remove them). */
const replace = (terms: Term[], fields: Term["field"][], next: Term[]) => [
  ...terms.filter((t) => !fields.includes(t.field)),
  ...next,
];

function AmountBound({
  label,
  op,
  terms,
  onTerms,
}: {
  label: string;
  op: ">=" | "<=";
  terms: Term[];
  onTerms: (terms: Term[]) => void;
}) {
  const current = terms.find((t) => t.field === "amount" && t.op === op);
  const [text, setText] = useState(current && current.field === "amount" ? editableAmount(current.value) : "");
  const [invalid, setInvalid] = useState(false);
  const apply = () => {
    const others = terms.filter((t) => !(t.field === "amount" && t.op === op));
    if (!text.trim()) return onTerms(others);
    const parsed = parseAmountTerm(text);
    setInvalid(!parsed);
    if (parsed && parsed.field === "amount") onTerms([...others, { ...parsed, op }]);
  };
  return (
    <input
      aria-label={label}
      aria-invalid={invalid || undefined}
      inputMode="decimal"
      placeholder={op === ">=" ? t("Min") : t("Max")}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={apply}
      onKeyDown={(e) => e.key === "Enter" && apply()}
    />
  );
}

export function RegisterToolbar({
  budget,
  filters,
  onChange,
  earliest,
  count,
  canAdd,
  onAdd,
  onImport,
  onSync,
  onExport,
  upcoming,
  onSchedules,
  showUpcoming,
  onShowUpcoming,
}: {
  budget: Budget;
  filters: Filters;
  onChange: (patch: Partial<Filters>) => void;
  earliest: string;
  count: number;
  canAdd: boolean;
  onAdd: () => void;
  onImport?: () => void;
  /** Syncs a linked account with its bank. */
  onSync?: () => void;
  /** Exports what's listed (or the whole account) as a CSV. */
  onExport: () => void;
  /** Scheduled transactions that could be listed. */
  upcoming: number;
  /** Opens the list of scheduled transactions (SchedulesModal). */
  onSchedules: () => void;
  showUpcoming: boolean;
  onShowUpcoming: (show: boolean) => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const { terms } = filters;
  const setTerms = (next: Term[]) => onChange({ terms: next });
  const status = terms.find((t) => t.field === "status")?.value ?? "all";
  const flagTerm = terms.find((t) => t.field === "flag" || t.field === "approval" || t.field === "categorization");
  const flag = !flagTerm
    ? "all"
    : flagTerm.field === "approval" || flagTerm.field === "categorization"
      ? flagTerm.field
      : flagTerm.value === "any"
        ? "flagged"
        : flagTerm.value === "none"
          ? "unflagged"
          : flagTerm.value;
  const chips = terms.filter((t) => t.field !== "text").length;
  return (
    <div className="register-toolbar">
      <button className="primary add-button" disabled={!canAdd} onClick={onAdd}>
        <Plus size={15} aria-hidden /> <span className="toolbar-label">{t("Add transaction")}</span>
      </button>
      <button
        className="secondary schedules-button"
        onClick={onSchedules}
        title={t("Transactions dated ahead or repeating, entered on their dates")}
      >
        <CalendarClock size={14} aria-hidden /> <span className="toolbar-label">{t("Scheduled")}</span>
        {upcoming > 0 && <span className="schedules-count">{upcoming}</span>}
      </button>
      {/* What brings transactions in or takes them out sits up in the page header, by undo and redo. */}
      <HeaderSlot actions>
        {onSync && (
          <button className="header-button" onClick={onSync} title={t("Get new transactions from the bank")}>
            <RefreshCw size={15} aria-hidden />
            <span className="header-label">{tc("command", "Sync")}</span>
          </button>
        )}
        {onImport && (
          <button className="header-button" onClick={onImport} title={t("Bring in a file your bank exported")}>
            <Upload size={15} aria-hidden />
            <span className="header-label">{t("Import")}</span>
          </button>
        )}
        <button className="header-button" onClick={onExport} title={t("Save these transactions as a CSV file")}>
          <Download size={15} aria-hidden />
          <span className="header-label">{t("Export")}</span>
        </button>
      </HeaderSlot>
      <SearchBox
        budget={budget}
        text={filters.text}
        terms={terms}
        onText={(text) => onChange({ text })}
        onTerms={setTerms}
      />
      <DateRangePicker
        value={filters.range}
        onChange={(range) => onChange({ range })}
        earliest={earliest}
        label={t("Dates")}
      />
      <button
        ref={button}
        className={chips ? "secondary filters-button active" : "secondary filters-button"}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <ListFilter size={14} aria-hidden /> <span className="toolbar-label">{t("Filters")}{chips ? " ·" : ""}</span>
        {chips ? chips : null}
      </button>
      {anyFilter(filters) && (
        <button className="link-button" onClick={() => onChange({ ...emptyFilters() })}>
          {t("Clear filters")}
        </button>
      )}
      {filters.reportQuery && (
        <button className="secondary active" onClick={() => onChange({ reportQuery: undefined })} aria-label={t("Remove report scope")}>
          {t("Report transactions")} <X size={13} aria-hidden />
        </button>
      )}
      <span className="count" aria-live="polite">
        {tn(count, "{count} transaction", "{count} transactions", { count: formatCount(count) })}
      </span>
      <Popover open={open} onClose={() => setOpen(false)} anchor={button} label={t("Filters")} className="filter-popover">
        <p className="help">{t("Filters you set here also show as chips in the search bar.")}</p>
        <label>
          {t("Cleared status")}
          <select
            value={status}
            onChange={(e) =>
              setTerms(
                replace(
                  terms,
                  ["status"],
                  e.target.value === "all" ? [] : [{ field: "status", value: e.target.value as "cleared" }],
                ),
              )
            }
          >
            {STATUSES.map(([k, v]) => (
              <option key={k} value={k}>
                {t(v)}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("Flag & review")}
          <select
            value={flag}
            onChange={(e) => {
              const v = e.target.value;
              const next: Term[] =
                v === "all"
                  ? []
                  : v === "approval" || v === "categorization"
                    ? [{ field: v, value: "needed" }]
                    : [{ field: "flag", value: v === "flagged" ? "any" : v === "unflagged" ? "none" : v }];
              setTerms(replace(terms, ["flag", "approval", "categorization"], next));
            }}
          >
            <option value="all">{t("Any")}</option>
            <option value="approval">{t("Needs approval")}</option>
            <option value="categorization">{t("Needs a category")}</option>
            <option value="flagged">{t("Any flag")}</option>
            <option value="unflagged">{t("No flag")}</option>
            {flagsOf(budget).map(([k, v]) => (
              <option key={k} value={k}>
                {t("{name} flag", { name: v })}
              </option>
            ))}
          </select>
        </label>
        <div className="field">
          <span>{t("Add a category")}</span>
          <CategoryPicker
            budget={budget}
            value={null}
            onChange={(id) => {
              const c = budget.categories.find((x) => x.id === id);
              const term: Term = { field: "category", id, value: c?.name ?? "Unassigned" };
              if (!terms.some((other) => other.field === "category" && other.id === id)) setTerms([...terms, term]);
            }}
            readyToAssign
            includeHidden
            includePayment
            label={t("Filter by category")}
            placeholder={t("Any category")}
          />
        </div>
        <div className="field">
          <span>{t("Amount")}</span>
          <div className="amount-range">
            <AmountBound label={t("Minimum amount")} op=">=" terms={terms} onTerms={setTerms} />
            <span className="muted">{t("to")}</span>
            <AmountBound label={t("Maximum amount")} op="<=" terms={terms} onTerms={setTerms} />
          </div>
          <small className="help">{t("Amounts match outflows and inflows alike, e.g. {amount}.", { amount: money(2500, { symbol: false }) })}</small>
        </div>
        {upcoming > 0 && (
          <label className="checkbox-label">
            <input type="checkbox" checked={showUpcoming} onChange={(e) => onShowUpcoming(e.target.checked)} />
            {t("Show upcoming scheduled transactions ({count})", { count: upcoming })}
          </label>
        )}
        <div className="filter-popover-actions">
          <button className="link-button" disabled={!chips} onClick={() => setTerms(terms.filter((term) => term.field === "text"))}>
            {t("Remove all filters")}
          </button>
          <button className="primary small" onClick={() => setOpen(false)}>
            {t("Done")}
          </button>
        </div>
      </Popover>
    </div>
  );
}

/** One text value for the selection (payee or memo). */
function ValuePopover({
  open,
  anchor,
  label,
  onClose,
  onApply,
}: {
  open: boolean;
  anchor: RefObject<HTMLElement | null>;
  label: string;
  onClose: () => void;
  onApply: (value: string) => void;
}) {
  const [value, setValue] = useState("");
  const submit = (e: FormEvent) => {
    e.preventDefault();
    onApply(value);
    setValue("");
    onClose();
  };
  return (
    <Popover open={open} onClose={onClose} anchor={anchor} placement="top-start" label={label} className="value-popover" modal>
      <form onSubmit={submit}>
        <label>
          {label}
          <input value={value} onChange={(e) => setValue(e.target.value)} />
        </label>
        <button className="primary small">{t("Apply")}</button>
      </form>
    </Popover>
  );
}

/** Actions for the selected transactions, floating over the bottom of the register. */
export function BulkBar({
  budget,
  count,
  totals,
  needsApproval,
  available,
  onRun,
  onDelete,
  onClear,
  onAsk,
}: {
  /** Opens the assistant with the selection attached. */
  onAsk?: () => void;
  budget: Budget;
  count: number;
  /** The selection's outflows and inflows, both positive, in minor units. */
  totals?: { outflow: number; inflow: number };
  needsApproval: boolean;
  available?: Map<string, number>;
  onRun: (operation: string, value: string | null, message: string) => void;
  onDelete: () => void;
  onClear: () => void;
}) {
  const categoryButton = useRef<HTMLButtonElement>(null);
  const flagButton = useRef<HTMLButtonElement>(null);
  const moreButton = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState<"category" | "flag" | "more" | "payee" | "memo" | null>(null);
  const close = () => setOpen(null);
  // Its button closes what it opened, as well as opening it.
  const toggle = (panel: NonNullable<typeof open>) => setOpen((o) => (o === panel ? null : panel));
  const n = tn(count, "{count} transaction", "{count} transactions", { count: formatCount(count) });
  const panels = (
    <>
        <Popover
          open={open === "category"}
          onClose={close}
          anchor={categoryButton}
          placement="top-start"
          label={t("Categorize")}
          className="value-popover"
          // Clicking away only closes it, so the click doesn't also change the selection.
          modal
        >
          <CategoryPicker
            budget={budget}
            value={null}
            available={available}
            readyToAssign={t("Unassigned (new money)")}
            autoFocus
            label={t("Category for selected")}
            onChange={(category) => {
              close();
              onRun("category", category, t("Categorized {transactions}.", { transactions: n }));
            }}
          />
        </Popover>
        <Menu
          open={open === "flag"}
          onClose={close}
          anchor={flagButton}
          placement="top-start"
          label={t("Flag")}
          items={[
            ...flagsOf(budget).map(([flag, name]) => ({
              label: name,
              icon: <FlagIcon flag={flag} size={11} />,
              onSelect: () => onRun("flag", flag, t("Flagged {transactions}: {name}.", { transactions: n, name })),
            })),
            "separator" as const,
            { label: t("Remove flag"), onSelect: () => onRun("flag", null, t("Removed flags from {transactions}.", { transactions: n })) },
          ]}
        />
        <Menu
          open={open === "more"}
          onClose={close}
          anchor={moreButton}
          placement="top-start"
          label={t("More actions")}
          items={[
            { label: t("Mark uncleared"), onSelect: () => onRun("status", "uncleared", t("Marked {transactions} uncleared.", { transactions: n })) },
            { label: t("Rename payee…"), onSelect: () => setTimeout(() => setOpen("payee")) },
            { label: t("Set memo…"), onSelect: () => setTimeout(() => setOpen("memo")) },
            { label: t("Duplicate"), icon: <Copy size={14} />, hint: keyLabel("Ctrl+D"), onSelect: () => onRun("duplicate", "", t("Duplicated {transactions}.", { transactions: n })) },
          ]}
        />
        <ValuePopover
          open={open === "payee"}
          anchor={moreButton}
          label={t("Payee")}
          onClose={close}
          onApply={(value) => onRun("payee", value, t("Changed the payee of {transactions}.", { transactions: n }))}
        />
        <ValuePopover
          open={open === "memo"}
          anchor={moreButton}
          label={t("Memo")}
          onClose={close}
          onApply={(value) => onRun("memo", value, t("Changed the memo of {transactions}.", { transactions: n }))}
        />
    </>
  );
  const net = totals ? totals.inflow - totals.outflow : 0;
  // A short bar over the bottom of the register: how many are selected (the pill, which also ends
  // the selection), what they come to, and icons for what can be done to them, each named in its
  // tooltip with its key.
  return (
    <div className="bulk-dock">
      <div className="bulk-bar" role="toolbar" aria-label={t("Selected transactions")}>
        <span className="bulk-count">
          <span aria-live="polite">{tn(count, "{count} selected", "{count} selected", { count: formatCount(count) })}</span>
          <button aria-label={t("Clear selection")} title={t("Clear selection (Esc)")} onClick={onClear}>
            <X size={14} />
          </button>
        </span>
        {/* Inflows less outflows, as the columns show amounts; the tooltip has each side. */}
        {totals && (
          <span className="bulk-total stat" title={t("{outflow} out · {inflow} in", { outflow: money(totals.outflow), inflow: money(totals.inflow) })}>
            <span>{t("Total")}</span>
            <strong className={net < 0 ? "negative" : net > 0 ? "positive" : undefined}>{money(net)}</strong>
          </span>
        )}
        <span className="bulk-actions">
          {needsApproval && (
            <button className="bulk-action bulk-approve" aria-label={t("Approve")} title={t("Approve (A)")} onClick={() => onRun("approve", "", t("Approved {transactions}.", { transactions: n }))}>
              <Check size={16} />
            </button>
          )}
          <button className="bulk-action" aria-label={t("Mark cleared")} title={t("Mark cleared (C)")} onClick={() => onRun("status", "cleared", t("Marked {transactions} cleared.", { transactions: n }))}>
            <CheckCheck size={16} />
          </button>
          <button ref={categoryButton} className="bulk-action" aria-label={t("Categorize")} title={t("Categorize")} aria-expanded={open === "category"} onClick={() => toggle("category")}>
            <Tag size={16} />
          </button>
          <button ref={flagButton} className="bulk-action" aria-label={t("Flag")} title={t("Flag")} aria-expanded={open === "flag"} onClick={() => toggle("flag")}>
            <Flag size={16} />
          </button>
          {onAsk && (
            <button className="bulk-action" aria-label={t("Ask the assistant")} title={t("Ask the assistant about these")} onClick={onAsk}>
              <MessageSquareText size={16} />
            </button>
          )}
          <button
            ref={moreButton}
            className="bulk-action"
            aria-label={t("More actions")}
            title={t("More actions")}
            aria-expanded={open === "more"}
            onClick={() => toggle("more")}
          >
            <MoreHorizontal size={16} />
          </button>
          <button className="bulk-action bulk-delete" aria-label={t("Delete")} title={t("Delete (Del)")} onClick={onDelete}>
            <Trash2 size={16} />
          </button>
        </span>
        {panels}
      </div>
    </div>
  );
}
