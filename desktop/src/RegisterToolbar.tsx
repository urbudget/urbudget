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
  ["all", "Any status"],
  ["uncleared", "Uncleared"],
  ["cleared", "Cleared"],
  ["reconciled", "Reconciled"],
  ["unreconciled", "Not reconciled"],
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
      placeholder={op === ">=" ? "Min" : "Max"}
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
        <Plus size={15} aria-hidden /> <span className="toolbar-label">Add transaction</span>
      </button>
      <button
        className="secondary schedules-button"
        onClick={onSchedules}
        title="Transactions dated ahead or repeating, entered on their dates"
      >
        <CalendarClock size={14} aria-hidden /> <span className="toolbar-label">Scheduled</span>
        {upcoming > 0 && <span className="schedules-count">{upcoming}</span>}
      </button>
      {/* What brings transactions in or takes them out sits up in the page header, by undo and redo. */}
      <HeaderSlot actions>
        {onSync && (
          <button className="header-button" onClick={onSync} title="Get new transactions from the bank">
            <RefreshCw size={15} aria-hidden />
            <span className="header-label">Sync</span>
          </button>
        )}
        {onImport && (
          <button className="header-button" onClick={onImport} title="Bring in a file your bank exported">
            <Upload size={15} aria-hidden />
            <span className="header-label">Import</span>
          </button>
        )}
        <button className="header-button" onClick={onExport} title="Save these transactions as a CSV file">
          <Download size={15} aria-hidden />
          <span className="header-label">Export</span>
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
        label="Dates"
      />
      <button
        ref={button}
        className={chips ? "secondary filters-button active" : "secondary filters-button"}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <ListFilter size={14} aria-hidden /> <span className="toolbar-label">Filters{chips ? " ·" : ""}</span>
        {chips ? chips : null}
      </button>
      {anyFilter(filters) && (
        <button className="link-button" onClick={() => onChange({ ...emptyFilters() })}>
          Clear filters
        </button>
      )}
      {filters.reportQuery && (
        <button className="secondary active" onClick={() => onChange({ reportQuery: undefined })} aria-label="Remove report scope">
          Report transactions <X size={13} aria-hidden />
        </button>
      )}
      <span className="count" aria-live="polite">
        {count.toLocaleString()} {count === 1 ? "transaction" : "transactions"}
      </span>
      <Popover open={open} onClose={() => setOpen(false)} anchor={button} label="Filters" className="filter-popover">
        <p className="help">Filters you set here also show as chips in the search bar.</p>
        <label>
          Cleared status
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
                {v}
              </option>
            ))}
          </select>
        </label>
        <label>
          Flag & review
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
            <option value="all">Any</option>
            <option value="approval">Needs approval</option>
            <option value="categorization">Needs a category</option>
            <option value="flagged">Any flag</option>
            <option value="unflagged">No flag</option>
            {flagsOf(budget).map(([k, v]) => (
              <option key={k} value={k}>
                {v} flag
              </option>
            ))}
          </select>
        </label>
        <div className="field">
          <span>Add a category</span>
          <CategoryPicker
            budget={budget}
            value={null}
            onChange={(id) => {
              const c = budget.categories.find((x) => x.id === id);
              const term: Term = { field: "category", id, value: c?.name ?? "Unassigned" };
              if (!terms.some((t) => t.field === "category" && t.id === id)) setTerms([...terms, term]);
            }}
            readyToAssign
            includeHidden
            includePayment
            label="Filter by category"
            placeholder="Any category"
          />
        </div>
        <div className="field">
          <span>Amount</span>
          <div className="amount-range">
            <AmountBound label="Minimum amount" op=">=" terms={terms} onTerms={setTerms} />
            <span className="muted">to</span>
            <AmountBound label="Maximum amount" op="<=" terms={terms} onTerms={setTerms} />
          </div>
          <small className="help">Amounts match outflows and inflows alike, e.g. {money(2500, { symbol: false })}.</small>
        </div>
        {upcoming > 0 && (
          <label className="checkbox-label">
            <input type="checkbox" checked={showUpcoming} onChange={(e) => onShowUpcoming(e.target.checked)} />
            Show upcoming scheduled transactions ({upcoming})
          </label>
        )}
        <div className="filter-popover-actions">
          <button className="link-button" disabled={!chips} onClick={() => setTerms(terms.filter((t) => t.field === "text"))}>
            Remove all filters
          </button>
          <button className="primary small" onClick={() => setOpen(false)}>
            Done
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
        <button className="primary small">Apply</button>
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
  const n = count === 1 ? "1 transaction" : `${count.toLocaleString()} transactions`;
  const panels = (
    <>
        <Popover
          open={open === "category"}
          onClose={close}
          anchor={categoryButton}
          placement="top-start"
          label="Categorize"
          className="value-popover"
          // Clicking away only closes it, so the click doesn't also change the selection.
          modal
        >
          <CategoryPicker
            budget={budget}
            value={null}
            available={available}
            readyToAssign="Unassigned (new money)"
            autoFocus
            label="Category for selected"
            onChange={(category) => {
              close();
              onRun("category", category, `Categorized ${n}.`);
            }}
          />
        </Popover>
        <Menu
          open={open === "flag"}
          onClose={close}
          anchor={flagButton}
          placement="top-start"
          label="Flag"
          items={[
            ...flagsOf(budget).map(([flag, name]) => ({
              label: name,
              icon: <FlagIcon flag={flag} size={11} />,
              onSelect: () => onRun("flag", flag, `Flagged ${n}: ${name}.`),
            })),
            "separator" as const,
            { label: "Remove flag", onSelect: () => onRun("flag", null, `Removed flags from ${n}.`) },
          ]}
        />
        <Menu
          open={open === "more"}
          onClose={close}
          anchor={moreButton}
          placement="top-start"
          label="More actions"
          items={[
            { label: "Mark uncleared", onSelect: () => onRun("status", "uncleared", `Marked ${n} uncleared.`) },
            { label: "Rename payee…", onSelect: () => setTimeout(() => setOpen("payee")) },
            { label: "Set memo…", onSelect: () => setTimeout(() => setOpen("memo")) },
            { label: "Duplicate", icon: <Copy size={14} />, hint: keyLabel("Ctrl+D"), onSelect: () => onRun("duplicate", "", `Duplicated ${n}.`) },
          ]}
        />
        <ValuePopover
          open={open === "payee"}
          anchor={moreButton}
          label="Payee"
          onClose={close}
          onApply={(value) => onRun("payee", value, `Changed the payee of ${n}.`)}
        />
        <ValuePopover
          open={open === "memo"}
          anchor={moreButton}
          label="Memo"
          onClose={close}
          onApply={(value) => onRun("memo", value, `Changed the memo of ${n}.`)}
        />
    </>
  );
  const net = totals ? totals.inflow - totals.outflow : 0;
  // A short bar over the bottom of the register: how many are selected (the pill, which also ends
  // the selection), what they come to, and icons for what can be done to them, each named in its
  // tooltip with its key.
  return (
    <div className="bulk-dock">
      <div className="bulk-bar" role="toolbar" aria-label="Selected transactions">
        <span className="bulk-count">
          <span aria-live="polite">{count.toLocaleString()} selected</span>
          <button aria-label="Clear selection" title="Clear selection (Esc)" onClick={onClear}>
            <X size={14} />
          </button>
        </span>
        {/* Inflows less outflows, as the columns show amounts; the tooltip has each side. */}
        {totals && (
          <span className="bulk-total stat" title={`${money(totals.outflow)} out · ${money(totals.inflow)} in`}>
            <span>Total</span>
            <strong className={net < 0 ? "negative" : net > 0 ? "positive" : undefined}>{money(net)}</strong>
          </span>
        )}
        <span className="bulk-actions">
          {needsApproval && (
            <button className="bulk-action bulk-approve" aria-label="Approve" title="Approve (A)" onClick={() => onRun("approve", "", `Approved ${n}.`)}>
              <Check size={16} />
            </button>
          )}
          <button className="bulk-action" aria-label="Mark cleared" title="Mark cleared (C)" onClick={() => onRun("status", "cleared", `Marked ${n} cleared.`)}>
            <CheckCheck size={16} />
          </button>
          <button ref={categoryButton} className="bulk-action" aria-label="Categorize" title="Categorize" aria-expanded={open === "category"} onClick={() => toggle("category")}>
            <Tag size={16} />
          </button>
          <button ref={flagButton} className="bulk-action" aria-label="Flag" title="Flag" aria-expanded={open === "flag"} onClick={() => toggle("flag")}>
            <Flag size={16} />
          </button>
          {onAsk && (
            <button className="bulk-action" aria-label="Ask the assistant" title="Ask the assistant about these" onClick={onAsk}>
              <MessageSquareText size={16} />
            </button>
          )}
          <button
            ref={moreButton}
            className="bulk-action"
            aria-label="More actions"
            title="More actions"
            aria-expanded={open === "more"}
            onClick={() => toggle("more")}
          >
            <MoreHorizontal size={16} />
          </button>
          <button className="bulk-action bulk-delete" aria-label="Delete" title="Delete (Del)" onClick={onDelete}>
            <Trash2 size={16} />
          </button>
        </span>
        {panels}
      </div>
    </div>
  );
}
